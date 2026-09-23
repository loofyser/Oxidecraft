//! Store tests against a fake transport: fetch, cache, verification failures,
//! on-disk permissions and the refusal to follow a symlinked objects directory.

use std::cell::RefCell;
use std::collections::HashMap;

use oxide_assets::http::{HttpClient, HttpError, UreqClient};
use oxide_assets::store::{Store, StoreError};

struct FakeHttp {
    bodies: HashMap<String, Vec<u8>>,
    calls: RefCell<Vec<String>>,
}

impl FakeHttp {
    fn new(bodies: HashMap<String, Vec<u8>>) -> Self {
        Self {
            bodies,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl HttpClient for FakeHttp {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.calls.borrow_mut().push(url.to_string());
        self.bodies
            .get(url)
            .cloned()
            .ok_or_else(|| HttpError::Status {
                url: url.to_string(),
                code: 404,
            })
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn fetches_verifies_and_caches_an_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"first sound".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url.clone(), body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let path = store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect("fetch");
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).expect("read"), body);
    assert_eq!(http.calls.borrow().len(), 1);

    // Second call must not touch the network.
    store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect("cache hit");
    assert_eq!(http.calls.borrow().len(), 1);
}

#[test]
fn corrupted_object_is_rejected_and_not_kept() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"real bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, b"tampered!!".to_vec())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let result = store.fetch_object(&http, &hash, body.len() as u64);
    assert!(result.is_err(), "tampered body must fail verification");
    assert!(
        !store.object_path(&hash).exists(),
        "nothing may land under the final name"
    );
}

#[test]
fn a_corrupted_cache_entry_is_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"good bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let path = store.object_path(&hash);
    std::fs::create_dir_all(path.parent().expect("shard dir")).expect("shard dir");
    std::fs::write(&path, b"corrupt").expect("seed a corrupt entry");

    let fetched = store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect("refetch");
    assert_eq!(std::fs::read(&fetched).expect("read"), body);
    assert_eq!(
        http.calls.borrow().len(),
        1,
        "the corrupt entry must be re-downloaded"
    );
}

#[test]
fn a_body_of_the_wrong_size_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"nine byt".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let result = store.fetch_object(&http, &hash, body.len() as u64 + 1);
    assert!(matches!(result, Err(StoreError::SizeMismatch { .. })));
    assert!(!store.object_path(&hash).exists());
}

#[test]
fn a_missing_remote_object_surfaces_as_an_http_status_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"not there".to_vec();
    let hash = sha1_hex(&body);
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let http = FakeHttp::new(HashMap::new());

    let result = store.fetch_object(&http, &hash, body.len() as u64);
    assert!(matches!(
        result,
        Err(StoreError::Http(HttpError::Status { code: 404, .. }))
    ));
    assert!(!store.object_path(&hash).exists());
}

#[test]
fn a_malformed_hash_is_rejected_without_panicking() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let http = FakeHttp::new(HashMap::new());

    // A one-character hash must not panic when it is turned into a path.
    let path = store.object_path("a");
    assert!(
        path.starts_with(dir.path()),
        "a malformed hash must stay under the store root"
    );

    let result = store.fetch_object(&http, "a", 1);
    assert!(matches!(result, Err(StoreError::BadHash { .. })));
    assert_eq!(
        http.calls.borrow().len(),
        0,
        "a malformed hash must not reach the network"
    );
}

#[test]
fn ureq_client_reports_an_unsupported_scheme() {
    let client = UreqClient::new();
    // No request is made: the scheme is rejected before any connection is attempted.
    let error = client
        .get("ftp://example.invalid/object")
        .expect_err("ftp is not supported");
    assert!(matches!(error, HttpError::UnsupportedScheme { .. }));
}

#[cfg(unix)]
#[test]
fn objects_are_written_read_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"read only bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let path = store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect("fetch");
    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o444, "object files are read-only on Linux");

    let left_behind = std::fs::read_dir(path.parent().expect("shard dir"))
        .expect("read_dir")
        .count();
    assert_eq!(
        left_behind, 1,
        "no temporary file may remain next to the object"
    );
}

#[cfg(unix)]
#[test]
fn open_refuses_a_symlinked_objects_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("assets")).expect("assets dir");
    std::os::unix::fs::symlink(outside.path(), dir.path().join("assets/objects")).expect("symlink");

    let result = Store::open(dir.path().to_path_buf());
    assert!(
        matches!(result, Err(StoreError::SymlinkedPath { .. })),
        "a symlinked objects directory must be refused"
    );
    assert!(
        std::fs::read_dir(outside.path())
            .expect("read outside dir")
            .next()
            .is_none(),
        "nothing may be written through the symlink"
    );
}

#[cfg(unix)]
#[test]
fn open_refuses_a_symlinked_assets_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    std::os::unix::fs::symlink(outside.path(), dir.path().join("assets")).expect("symlink");

    let result = Store::open(dir.path().to_path_buf());
    assert!(
        matches!(result, Err(StoreError::SymlinkedPath { .. })),
        "the objects tree must not be reached through a symlinked parent"
    );
    assert!(
        std::fs::read_dir(outside.path())
            .expect("read outside dir")
            .next()
            .is_none(),
        "nothing may be written through the symlink"
    );
}
