//! Store tests against a fake transport: fetch, cache, verification failures,
//! on-disk permissions, the verification pass and the refusal to follow a
//! symlinked objects directory.

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
        !store.object_path(&hash).expect("the object path").exists(),
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

    let path = store.object_path(&hash).expect("the object path");
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
    assert!(!store.object_path(&hash).expect("the object path").exists());
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
    assert!(!store.object_path(&hash).expect("the object path").exists());
}

#[test]
fn a_malformed_hash_is_rejected_without_panicking() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let http = FakeHttp::new(HashMap::new());

    // A one-character hash has no path under the store: it is refused before
    // one is built, so nothing can leave the store root.
    let error = store
        .object_path("a")
        .expect_err("a malformed hash must be refused");
    assert!(matches!(error, StoreError::BadHash { .. }), "got {error:?}");

    let result = store.fetch_object(&http, "a", 1);
    assert!(matches!(result, Err(StoreError::BadHash { .. })));
    assert_eq!(
        http.calls.borrow().len(),
        0,
        "a malformed hash must not reach the network"
    );
}

#[test]
fn the_path_builders_refuse_components_that_could_escape_the_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    // An id that is not a single plain path segment has no path under the
    // store: each builder refuses it before a path is built.
    for escaping in ["", ".", "..", "a/b", "a\\b", "/etc/passwd"] {
        assert!(
            matches!(store.version_dir(escaping), Err(StoreError::BadId { .. })),
            "version id {escaping:?} must be refused"
        );
        assert!(
            matches!(
                store.version_json_path(escaping),
                Err(StoreError::BadId { .. })
            ),
            "a version document path for {escaping:?} must be refused"
        );
        assert!(
            matches!(
                store.client_jar_path(escaping),
                Err(StoreError::BadId { .. })
            ),
            "a client jar path for {escaping:?} must be refused"
        );
        assert!(
            matches!(store.index_path(escaping), Err(StoreError::BadId { .. })),
            "asset index id {escaping:?} must be refused"
        );
    }

    // A valid id still builds the path the store layout spells.
    assert_eq!(
        store.version_dir("1.8.9").expect("a valid version id"),
        dir.path().join("versions").join("1.8.9")
    );
    assert_eq!(
        store.index_path("1.8").expect("a valid index id"),
        dir.path().join("assets/indexes/1.8.json")
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

#[cfg(unix)]
#[test]
fn a_same_length_corruption_of_a_cached_object_fails_on_the_hash() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"golden bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let size = body.len() as u64;

    let path = store.fetch_object(&http, &hash, size).expect("seed fetch");

    // Rewrite the cached bytes without changing their length, the way bit rot
    // or tampering would: only the hash can catch this.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("make writable");
    std::fs::write(&path, b"rotten bytes").expect("corrupt in place");
    assert_eq!(
        std::fs::metadata(&path).expect("metadata").len(),
        size,
        "the corruption must keep the byte length identical"
    );

    let error = store
        .verify_object(&hash, size)
        .expect_err("a same-length corruption must fail verification");
    assert!(
        matches!(error, StoreError::HashMismatch { .. }),
        "the hash check must catch it, got {error:?}"
    );

    let re_fetched = store
        .fetch_object(&http, &hash, size)
        .expect("re-fetch after corruption");
    assert_eq!(std::fs::read(&re_fetched).expect("read"), body);
    assert_eq!(
        http.calls.borrow().len(),
        2,
        "the corrupted bytes must be downloaded again"
    );
}

#[test]
fn an_object_lives_under_its_two_hex_digit_shard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"layout bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let expected = dir
        .path()
        .join("assets")
        .join("objects")
        .join(&hash[..2])
        .join(&hash);
    assert_eq!(
        store.object_path(&hash).expect("the object path"),
        expected,
        "the layout must be <root>/assets/objects/<first two hex>/<hash>"
    );

    let fetched = store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect("fetch");
    assert_eq!(
        fetched, expected,
        "the fetched object must land at that exact path"
    );
    assert!(expected.is_file());
}

#[cfg(unix)]
#[test]
fn a_corrupt_read_only_cache_entry_is_removed_and_re_fetched() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"precious bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let path = store.object_path(&hash).expect("the object path");
    std::fs::create_dir_all(path.parent().expect("shard dir")).expect("shard dir");
    std::fs::write(&path, b"corrupt").expect("seed a corrupt entry");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("read-only");

    let fetched = store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect("a read-only corrupt entry is removed and re-fetched");
    assert_eq!(std::fs::read(&fetched).expect("read"), body);
    assert_eq!(
        http.calls.borrow().len(),
        1,
        "the corrupt entry must be re-downloaded"
    );

    let mode = std::fs::metadata(&fetched)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o444, "the replacement is read-only again");
}

#[test]
fn an_uppercase_hash_finds_the_lowercase_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"case matters".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url.clone(), body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let uppercase = hash.to_uppercase();

    let path = store
        .fetch_object(&http, &uppercase, body.len() as u64)
        .expect("an uppercase hash must resolve to the lowercase object");
    assert_eq!(
        path,
        store.object_path(&hash).expect("the object path"),
        "the object lands at the lowercase path"
    );
    assert_eq!(http.calls.borrow().len(), 1);
    assert_eq!(
        http.calls.borrow()[0],
        url,
        "the request uses the lowercase URL"
    );

    store
        .fetch_object(&http, &uppercase, body.len() as u64)
        .expect("cache hit through the uppercase spelling");
    assert_eq!(
        http.calls.borrow().len(),
        1,
        "the lowercase object must be reused"
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_cache_entry_surfaces_the_error_and_keeps_the_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"unreadable bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    );
    let http = FakeHttp::new(HashMap::from([(url, body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let path = store.object_path(&hash).expect("the object path");
    std::fs::create_dir_all(path.parent().expect("shard dir")).expect("shard dir");
    std::fs::write(&path, &body).expect("seed a valid entry");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("unreadable");
    if std::fs::read(&path).is_ok() {
        // A privileged user reads through the mode; there is nothing to assert.
        return;
    }

    let error = store
        .fetch_object(&http, &hash, body.len() as u64)
        .expect_err("an unreadable entry must not be silently replaced");
    assert!(matches!(error, StoreError::Io { .. }), "got {error:?}");
    assert!(path.exists(), "the unreadable entry must be left in place");
    assert_eq!(
        http.calls.borrow().len(),
        0,
        "no re-download may be attempted"
    );
}

#[test]
fn the_verification_pass_reports_verified_missing_and_mismatched_objects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    // A valid object on disk.
    let good = b"a good object".to_vec();
    let good_hash = sha1_hex(&good);
    let good_path = store.object_path(&good_hash).expect("the object path");
    std::fs::create_dir_all(good_path.parent().expect("shard dir")).expect("shard dir");
    std::fs::write(&good_path, &good).expect("write the good object");

    // An object stored under its own name but with the wrong bytes, the same
    // length as the real ones: only the hash can catch this.
    let real = b"the real bytes".to_vec();
    let mut tampered = real.clone();
    tampered[0] = b'X';
    assert_eq!(
        tampered.len(),
        real.len(),
        "the corruption keeps the length"
    );
    let tampered_hash = sha1_hex(&real);
    let tampered_path = store.object_path(&tampered_hash).expect("the object path");
    std::fs::create_dir_all(tampered_path.parent().expect("shard dir")).expect("shard dir");
    std::fs::write(&tampered_path, &tampered).expect("write the tampered object");

    // An object that was never stored.
    let absent = b"never written".to_vec();
    let absent_hash = sha1_hex(&absent);

    let expected = [
        (good_hash.as_str(), good.len() as u64),
        (tampered_hash.as_str(), real.len() as u64),
        (absent_hash.as_str(), absent.len() as u64),
    ];
    let report = store.verify_objects(&expected);

    assert_eq!(report.objects, 1, "only the good object verifies");
    assert_eq!(
        report.bytes,
        good.len() as u64,
        "the bytes of verified objects only"
    );
    assert_eq!(report.mismatched, vec![tampered_hash.clone()]);
    assert_eq!(report.missing, vec![absent_hash.clone()]);
    assert!(!report.is_clean());

    // The same pass over a store that only holds the good object is clean.
    let clean = store.verify_objects(&[(good_hash.as_str(), good.len() as u64)]);
    assert!(clean.is_clean());
    assert_eq!(clean.objects, 1);
    assert_eq!(clean.bytes, good.len() as u64);
}
