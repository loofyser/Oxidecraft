//! Fetch-flow tests against a fake transport: the piston-meta chain resolved
//! into the store end to end, with reuse, repair, locking, dry runs and the
//! strict verification pass.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

use oxide_assets::fetch::{FetchError, FetchOptions, Progress, fetch_version};
use oxide_assets::http::{HttpClient, HttpError};
use oxide_assets::store::{Store, StoreError};
use oxide_assets::version::{CLIENT_1_8_9_SHA1, CLIENT_1_8_9_SIZE, VERSION_MANIFEST_URL};

/// The fake versions live under this (reserved, unreachable) domain.
const VERSION_URL: &str = "https://piston-meta.invalid/versions/1.8.10.json";
/// URL of the fake asset index.
const INDEX_URL: &str = "https://piston-meta.invalid/indexes/1.8.json";
/// URL of the fake client jar.
const JAR_URL: &str = "https://piston-meta.invalid/versions/1.8.10/client.jar";

/// A transport that serves canned bodies and counts every request.
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

/// The object URL for `hash`, the way the resource server shards it.
fn object_url(hash: &str) -> String {
    format!(
        "https://resources.download.minecraft.net/{}/{hash}",
        &hash[..2]
    )
}

/// A synthetic version chain: the manifest, a version document, an index with
/// three entries over two distinct objects, and a client jar, all served by
/// the fake transport.
struct Fixture {
    http: FakeHttp,
    version: String,
    /// The distinct objects, in index order: `(hash, size)`.
    objects: Vec<(String, u64)>,
    /// The index total size, which counts the repeated entry twice, the way
    /// the real 1.8 index does.
    total_size: u64,
    jar_sha1: String,
    jar_size: u64,
}

/// How the fixture's version document describes the client jar.
#[derive(Clone, Copy)]
enum ClientDescriptor {
    /// The descriptor of the stand-in body the fake transport serves.
    Served,
    /// The pinned 1.8.9 constants, so the pinned-constant guard passes even
    /// though the stand-in body cannot carry them.
    Pinned,
}

/// Builds the fixture for `version`.
fn fixture(version: &str) -> Fixture {
    fixture_for(version, version, ClientDescriptor::Served)
}

/// Builds the fixture with a manifest entry advertising `advertised` while the
/// version document names `document_id`, so a mismatch can be exercised.
fn fixture_for(advertised: &str, document_id: &str, client: ClientDescriptor) -> Fixture {
    let first = b"first sound".to_vec();
    let second = b"second sound".to_vec();
    let first_hash = sha1_hex(&first);
    let second_hash = sha1_hex(&second);

    let index_json = serde_json::json!({
        "objects": {
            "minecraft/sounds/a.ogg": { "hash": first_hash, "size": first.len() },
            "minecraft/sounds/b.ogg": { "hash": second_hash, "size": second.len() },
            "minecraft/sounds/c.ogg": { "hash": first_hash, "size": first.len() },
        }
    });
    let index_body = serde_json::to_vec(&index_json).expect("serialize the index");
    let total_size = first.len() as u64 * 2 + second.len() as u64;

    let jar_body = b"fake client jar".to_vec();
    let jar_sha1 = sha1_hex(&jar_body);
    let jar_size = jar_body.len() as u64;

    let (client_sha1, client_size) = match client {
        ClientDescriptor::Served => (jar_sha1.clone(), jar_size),
        ClientDescriptor::Pinned => (CLIENT_1_8_9_SHA1.to_string(), CLIENT_1_8_9_SIZE),
    };

    let version_json = serde_json::json!({
        "id": document_id,
        "assets": "1.8",
        "assetIndex": {
            "id": "1.8",
            "url": INDEX_URL,
            "sha1": sha1_hex(&index_body),
            "size": index_body.len(),
            "totalSize": total_size,
        },
        "downloads": {
            "client": { "url": JAR_URL, "sha1": client_sha1, "size": client_size }
        }
    });
    let version_body = serde_json::to_vec(&version_json).expect("serialize the version document");

    let manifest_json = serde_json::json!({
        "latest": { "release": advertised, "snapshot": advertised },
        "versions": [
            { "id": advertised, "url": VERSION_URL, "sha1": sha1_hex(&version_body) }
        ]
    });
    let manifest_body = serde_json::to_vec(&manifest_json).expect("serialize the manifest");

    let mut bodies = HashMap::new();
    bodies.insert(VERSION_MANIFEST_URL.to_string(), manifest_body);
    bodies.insert(VERSION_URL.to_string(), version_body);
    bodies.insert(INDEX_URL.to_string(), index_body);
    bodies.insert(JAR_URL.to_string(), jar_body);
    for (body, hash) in [(&first, &first_hash), (&second, &second_hash)] {
        bodies.insert(object_url(hash), body.clone());
    }

    Fixture {
        http: FakeHttp::new(bodies),
        version: advertised.to_string(),
        objects: vec![
            (first_hash, first.len() as u64),
            (second_hash, second.len() as u64),
        ],
        total_size,
        jar_sha1,
        jar_size,
    }
}

/// The options for a real run of `version`.
fn options(version: &str) -> FetchOptions {
    FetchOptions {
        version: version.to_string(),
        dry_run: false,
        verify: false,
    }
}

/// Makes a file writable so a test can corrupt it in place.
#[cfg(unix)]
fn make_writable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).expect("make writable");
}

#[cfg(not(unix))]
fn make_writable(_path: &std::path::Path) {}

/// Asserts the store lock is free again: the lock file stays on disk (it is
/// reused, never removed), but an exclusive lock can be taken over it.
fn assert_lock_released(root: &std::path::Path) {
    let path = root.join("lock");
    assert!(path.is_file(), "the lock file stays in place");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open the lock file");
    fs4::FileExt::try_lock(&file).expect("the lock must be free after the run");
}

#[test]
fn fetch_is_idempotent_and_repairs_corruption() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.10");
    let opts = options("1.8.10");

    let events = RefCell::new(Vec::new());
    let first = fetch_version(&store, &f.http, &opts, |p| events.borrow_mut().push(p))
        .expect("first fetch");
    let events = events.into_inner();

    // Manifest, version document, index, jar and two distinct objects: the
    // index's third entry repeats the first object's hash and is not fetched
    // again.
    assert_eq!(f.http.calls.borrow().len(), 6, "first run transfers");
    assert_eq!(first.downloaded, 6);
    assert_eq!(first.reused, 0);
    assert!(
        events.contains(&Progress::step("manifest", 1, 1)),
        "the manifest step reports: {events:?}"
    );
    assert!(
        events.contains(&Progress::step("objects", 2, 2)),
        "progress counts the distinct objects: {events:?}"
    );
    assert!(events.contains(&Progress::step("jar", 1, 1)));
    assert_eq!(
        f.http
            .calls
            .borrow()
            .iter()
            .filter(|url| url.contains(&f.objects[0].0))
            .count(),
        1,
        "a repeated hash is fetched once"
    );

    // Everything landed with the hashes it was fetched under.
    for (hash, size) in &f.objects {
        let path = store.object_path(hash);
        assert!(path.is_file(), "object {hash} must exist");
        let bytes = std::fs::read(&path).expect("read the object");
        assert_eq!(bytes.len() as u64, *size);
        assert_eq!(sha1_hex(&bytes), *hash);
    }
    let stored = std::fs::read_to_string(store.version_json_path(&f.version))
        .expect("read the stored version document");
    let document = oxide_assets::version::parse_version_json(&stored).expect("parse it");
    assert_eq!(
        document.asset_index.total_size, f.total_size,
        "the version document is stored verbatim"
    );
    let jar_path = store.client_jar_path(&f.version);
    assert!(store.index_path(&document.assets).is_file());
    assert_eq!(
        std::fs::read(&jar_path).expect("read the jar").len() as u64,
        f.jar_size
    );
    assert_lock_released(dir.path());

    // Second run: everything is present and valid, nothing goes over the wire.
    let second = fetch_version(&store, &f.http, &opts, |_| {}).expect("second fetch");
    assert_eq!(
        f.http.calls.borrow().len(),
        6,
        "a second run must download nothing"
    );
    assert_eq!(second.downloaded, 0);
    assert_eq!(
        second.reused, 5,
        "two objects, the jar, the index and the version document"
    );

    // A same-length corruption of one object is repaired instead of trusted.
    let (hash, size) = &f.objects[0];
    let path = store.object_path(hash);
    make_writable(&path);
    std::fs::write(&path, b"rotten soun").expect("corrupt in place");
    assert_eq!(
        std::fs::metadata(&path).expect("metadata").len(),
        *size,
        "the corruption must keep the byte length identical"
    );

    let third = fetch_version(&store, &f.http, &opts, |_| {}).expect("repair fetch");
    assert_eq!(
        f.http.calls.borrow().len(),
        7,
        "exactly one object is re-downloaded"
    );
    assert_eq!(third.downloaded, 1);
    assert_eq!(third.reused, 4);
    assert_eq!(
        sha1_hex(&std::fs::read(&path).expect("read the repaired object")),
        *hash,
        "the repaired object verifies again"
    );
}

#[test]
fn dry_run_resolves_the_plan_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.10");
    let opts = FetchOptions {
        version: "1.8.10".to_string(),
        dry_run: true,
        verify: false,
    };

    let report = fetch_version(&store, &f.http, &opts, |_| {}).expect("dry run");

    // The chain is resolved over the wire but nothing is stored.
    assert_eq!(
        f.http.calls.borrow().len(),
        3,
        "manifest, version document, index"
    );
    assert_eq!(report.downloaded, 0);
    assert_eq!(report.reused, 0);
    assert_eq!(report.planned, 3, "two objects and the jar");
    assert_eq!(
        report.planned_bytes,
        f.objects.iter().map(|(_, size)| size).sum::<u64>() + f.jar_size
    );
    assert!(report.verification.is_none());

    // No fetched content landed; the store skeleton and the lock file are
    // there (a dry run still creates the directories and takes the lock), but
    // the lock is free again.
    assert!(!store.version_json_path("1.8.10").exists());
    assert!(!store.index_path("1.8").exists());
    assert!(!store.client_jar_path("1.8.10").exists());
    assert_lock_released(dir.path());
    assert_eq!(
        std::fs::read_dir(dir.path().join("assets/objects"))
            .expect("objects directory")
            .count(),
        0,
        "no object may be written by a dry run"
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("versions"))
            .expect("versions directory")
            .count(),
        0,
        "no version file may be written by a dry run"
    );
}

#[test]
fn a_second_run_is_refused_while_the_lock_is_held() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.10");
    let lock = dir.path().join("lock");

    // Another run: the file exists and a live process holds the exclusive
    // operating-system lock over it.
    let mut holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock)
        .expect("open the lock file");
    holder
        .write_all(b"4242\n")
        .expect("the holder's status line");
    fs4::FileExt::try_lock(&holder).expect("hold the lock");

    let error = fetch_version(&store, &f.http, &options("1.8.10"), |_| {})
        .expect_err("the lock is held by another run");
    assert!(matches!(error, FetchError::Locked { .. }), "got {error:?}");
    let message = error.to_string();
    assert!(
        message.contains(&lock.display().to_string()),
        "the message names the lock file: {message}"
    );
    assert!(
        message.contains("holds the lock"),
        "the message says another fetch holds the lock: {message}"
    );
    assert_eq!(
        f.http.calls.borrow().len(),
        0,
        "no network work may happen before the lock"
    );
    assert_eq!(
        std::fs::read_to_string(&lock).expect("read the lock file"),
        "4242\n",
        "the refused run must leave the holder's file alone"
    );
}

#[test]
fn the_lock_is_released_when_a_run_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let http = FakeHttp::new(HashMap::new());

    fetch_version(&store, &http, &options("1.8.10"), |_| {})
        .expect_err("the manifest is unreachable");
    assert_lock_released(dir.path());
}

#[test]
fn verify_reports_the_store_through_its_verification_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.10");
    fetch_version(&store, &f.http, &options("1.8.10"), |_| {}).expect("fetch");
    let calls = f.http.calls.borrow().len();

    let opts = FetchOptions {
        version: "1.8.10".to_string(),
        dry_run: false,
        verify: true,
    };
    let events = RefCell::new(Vec::new());
    let report = fetch_version(&store, &f.http, &opts, |progress| {
        events.borrow_mut().push(progress);
    })
    .expect("verify fetch");
    assert_eq!(
        f.http.calls.borrow().len(),
        calls,
        "verifying downloads nothing"
    );

    let events = events.into_inner();
    let verify_events: Vec<Progress> = events
        .iter()
        .filter(|event| event.name == "verify")
        .cloned()
        .collect();
    assert_eq!(
        verify_events,
        vec![
            Progress::step("verify", 0, 1),
            Progress::step("verify", 1, 1)
        ],
        "the strict pass is bracketed by progress: {events:?}"
    );

    let verification = report.verification.expect("a verification report");
    assert!(
        verification.is_clean(),
        "the store is clean: {verification:?}"
    );
    assert_eq!(verification.objects, 2, "one per distinct object");
    assert!(verification.missing.is_empty());
    assert!(verification.mismatched.is_empty());
    assert_eq!(
        verification.bytes,
        f.objects.iter().map(|(_, size)| size).sum::<u64>(),
        "the bytes on disk are the distinct objects' bytes"
    );
}

#[test]
fn an_unknown_version_is_refused_after_the_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.10");

    let error = fetch_version(&store, &f.http, &options("1.0.0"), |_| {})
        .expect_err("1.0.0 is not in the manifest");
    assert!(
        matches!(error, FetchError::VersionNotFound { .. }),
        "got {error:?}"
    );
    assert_eq!(
        f.http.calls.borrow().len(),
        1,
        "only the manifest is fetched"
    );
    assert_lock_released(dir.path());
}

#[test]
fn a_version_document_for_another_version_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture_for("1.8.10", "1.8.11", ClientDescriptor::Served);

    let error = fetch_version(&store, &f.http, &options("1.8.10"), |_| {})
        .expect_err("the document names 1.8.11");
    assert!(
        matches!(error, FetchError::VersionMismatch { .. }),
        "got {error:?}"
    );
    assert_eq!(f.http.calls.borrow().len(), 2, "manifest and the document");
    assert!(
        !store.version_json_path("1.8.10").exists(),
        "a document for another version must not be stored"
    );
    assert_lock_released(dir.path());
}

#[test]
fn the_1_8_9_client_jar_descriptor_is_guarded_by_the_constants() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.9");
    assert_ne!(
        f.jar_sha1, CLIENT_1_8_9_SHA1,
        "the fixture jar cannot carry the pinned hash"
    );
    assert_eq!(CLIENT_1_8_9_SIZE, 8_461_484);

    let error = fetch_version(&store, &f.http, &options("1.8.9"), |_| {})
        .expect_err("the descriptor must match the pinned constants");
    assert!(
        matches!(error, FetchError::JarDescriptor { .. }),
        "got {error:?}"
    );
    assert_eq!(
        f.http.calls.borrow().len(),
        2,
        "the guard fires before the index"
    );
    assert!(
        !store.version_json_path("1.8.9").exists(),
        "nothing is stored when the guard refuses"
    );
    assert!(!store.client_jar_path("1.8.9").exists());
    assert_lock_released(dir.path());
}

#[test]
fn an_unparsable_stored_version_document_is_re_resolved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    // The pinned descriptor for 1.8.9, so the fresh document gets past the
    // guard; the stand-in jar still cannot match the pinned size, so the run
    // stops at the jar step. Everything before that — the manifest chain and
    // the replacement of the stored document — is what this test asserts.
    let f = fixture_for("1.8.9", "1.8.9", ClientDescriptor::Pinned);

    // Bytes that are not UTF-8 at all: a corruption of the stored document
    // that a reader without care cannot even look at. The run must re-resolve
    // instead of refusing until someone deletes the file.
    let path = store.version_json_path("1.8.9");
    std::fs::create_dir_all(path.parent().expect("the version directory"))
        .expect("create the version directory");
    std::fs::write(&path, [b'{', b'"', 0xff, 0xfe, b'"', b'}']).expect("write invalid UTF-8");

    let error = fetch_version(&store, &f.http, &options("1.8.9"), |_| {})
        .expect_err("the stand-in jar cannot satisfy the pinned descriptor");

    assert_eq!(
        f.http.calls.borrow().first().map(String::as_str),
        Some(VERSION_MANIFEST_URL),
        "the stored document must not be fatal: the manifest chain runs"
    );
    assert_eq!(
        f.http.calls.borrow().len(),
        6,
        "manifest, document, index, two objects and the jar"
    );
    assert!(
        matches!(error, FetchError::Store(StoreError::SizeMismatch { .. })),
        "the run reaches the jar step: {error:?}"
    );

    let stored = std::fs::read_to_string(&path).expect("the stored document is replaced");
    let document =
        oxide_assets::version::parse_version_json(&stored).expect("parse the replacement");
    assert_eq!(document.id, "1.8.9");
    assert_eq!(
        document.downloads.client.sha1, CLIENT_1_8_9_SHA1,
        "the replacement is the fresh, verified document"
    );
}

#[test]
fn a_stored_document_that_fails_the_pin_is_re_resolved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture_for("1.8.9", "1.8.9", ClientDescriptor::Pinned);

    // A stored document that parses and names the requested version but does
    // not carry the pinned client jar descriptor: it must be re-resolved from
    // the manifest chain, not trusted.
    let stale = serde_json::json!({
        "id": "1.8.9",
        "assets": "1.8",
        "assetIndex": { "id": "1.8", "url": INDEX_URL, "sha1": "0000", "size": 1, "totalSize": 1 },
        "downloads": {
            "client": {
                "url": JAR_URL,
                "sha1": "0000000000000000000000000000000000000000",
                "size": 1
            }
        }
    });
    let path = store.version_json_path("1.8.9");
    std::fs::create_dir_all(path.parent().expect("the version directory"))
        .expect("create the version directory");
    std::fs::write(
        &path,
        serde_json::to_vec(&stale).expect("serialize the stale document"),
    )
    .expect("write the stale document");

    let error = fetch_version(&store, &f.http, &options("1.8.9"), |_| {})
        .expect_err("the stand-in jar cannot satisfy the pinned descriptor");

    assert_eq!(
        f.http.calls.borrow().first().map(String::as_str),
        Some(VERSION_MANIFEST_URL),
        "the stored document must not be trusted: the manifest chain runs"
    );
    assert_eq!(
        f.http.calls.borrow().len(),
        6,
        "manifest, document, index, two objects and the jar"
    );
    assert!(
        matches!(error, FetchError::Store(StoreError::SizeMismatch { .. })),
        "the run reaches the jar step: {error:?}"
    );

    let stored = std::fs::read_to_string(&path).expect("the stored document is replaced");
    let document =
        oxide_assets::version::parse_version_json(&stored).expect("parse the replacement");
    assert_eq!(document.id, "1.8.9");
    assert_eq!(
        document.downloads.client.sha1, CLIENT_1_8_9_SHA1,
        "the replacement carries the pinned descriptor"
    );
    assert_eq!(document.downloads.client.size, CLIENT_1_8_9_SIZE);
}

#[test]
fn a_lock_file_left_by_a_dead_run_does_not_block_the_next() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().to_path_buf()).expect("open");
    let f = fixture("1.8.10");

    // A lock file a killed run left behind: on disk, but no live process
    // holds the operating-system lock over it. The next run must take it
    // instead of claiming another fetch is running.
    let path = dir.path().join("lock");
    std::fs::write(&path, b"4242\n").expect("a stale lock file");

    let report = fetch_version(&store, &f.http, &options("1.8.10"), |_| {})
        .expect("a stale lock file must not lock out the next run");
    assert_eq!(report.downloaded, 6, "the run completed");
    assert_eq!(
        std::fs::read_to_string(&path)
            .expect("read the lock file")
            .trim(),
        std::process::id().to_string(),
        "the run left its own process id behind"
    );
    assert_lock_released(dir.path());
}
