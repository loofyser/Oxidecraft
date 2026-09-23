//! The fetch flow: resolve the piston-meta chain into the verified store.
//!
//! A run takes the store's single-instance lock, resolves the version document
//! (a local copy is reused when it names the requested version), checks the
//! pinned client jar constants, checks the store's filesystem has room for the
//! download, resolves the asset index, downloads every object the index lists
//! (reusing or repairing the ones already present) and the client jar, and,
//! when asked, re-hashes every object and the jar through the store's
//! verification pass. Nothing enters the store before it matches the hash its
//! descriptor records.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::asset_index::AssetIndex;
use crate::http::{HttpClient, HttpError};
use crate::store::{Store, StoreError, VerifyReport, verify_bytes, verify_sha1, write_atomic};
use crate::version::{
    CLIENT_1_8_9_SHA1, CLIENT_1_8_9_SIZE, DownloadInfo, VERSION_MANIFEST_URL, VersionJson,
    find_version, parse_manifest, parse_version_json,
};

/// The lock file's name inside the store root.
const LOCK_FILE_NAME: &str = "lock";

/// Options for a fetch run.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// Version to fetch, for example `1.8.9`.
    pub version: String,
    /// Resolve and report only: no object or jar is downloaded or stored.
    pub dry_run: bool,
    /// Re-hash every object and the jar after downloading.
    pub verify: bool,
}

/// What a fetch run did.
#[derive(Debug, Default)]
pub struct FetchReport {
    /// Transfers this run made over the transport, the version manifest
    /// included; the manifest is fetched but never stored.
    pub downloaded: usize,
    /// Stored files already present and valid: the version document, the
    /// index, every reused object and the jar.
    pub reused: usize,
    /// Total bytes transferred by those downloads.
    pub bytes: u64,
    /// Index objects and the jar a dry run still has to download.
    pub planned: usize,
    /// Bytes those planned downloads would transfer.
    pub planned_bytes: u64,
    /// The verification pass over the index objects, when `verify` was set.
    pub verification: Option<VerifyReport>,
}

/// A progress event from a fetch run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// The step's name, for example `objects`.
    pub name: String,
    /// Units completed so far.
    pub done: u64,
    /// Total units in the step.
    pub total: u64,
}

impl Progress {
    /// Builds a progress event for `name`, `done` of `total` units done.
    pub fn step(name: impl Into<String>, done: u64, total: u64) -> Self {
        Self {
            name: name.into(),
            done,
            total,
        }
    }
}

/// Errors from a fetch run.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// A store operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A request failed.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// A piston-meta document was not UTF-8 text.
    #[error("malformed {what}: {source}")]
    Text {
        /// Which document.
        what: &'static str,
        /// Underlying error.
        source: std::str::Utf8Error,
    },
    /// A piston-meta document did not parse.
    #[error("malformed {what}: {source}")]
    Json {
        /// Which document.
        what: &'static str,
        /// Underlying error.
        source: serde_json::Error,
    },
    /// The manifest has no entry for the requested version.
    #[error("version {version} is not in the version manifest")]
    VersionNotFound {
        /// The requested version.
        version: String,
    },
    /// The version document names a version other than the one requested.
    #[error("the version document describes {found}, not {requested}")]
    VersionMismatch {
        /// The requested version.
        requested: String,
        /// The version the document names.
        found: String,
    },
    /// The 1.8.9 client jar descriptor no longer matches the pinned values.
    #[error(
        "the 1.8.9 client jar descriptor changed: sha1 {found_sha1}, size {found_size} \
         (pinned sha1 {pinned_sha1}, size {pinned_size})"
    )]
    JarDescriptor {
        /// The hash the version document records.
        found_sha1: String,
        /// The size the version document records.
        found_size: u64,
        /// The pinned hash.
        pinned_sha1: &'static str,
        /// The pinned size.
        pinned_size: u64,
    },
    /// An id that would escape the store when used as a path segment.
    #[error("{what} id {id:?} is not usable as a path segment")]
    BadId {
        /// Which id.
        what: &'static str,
        /// The offending id.
        id: String,
    },
    /// Another fetch run holds the store lock.
    #[error("another fetch is already running for this store; lock file: {}", path.display())]
    Locked {
        /// The lock file that is held.
        path: PathBuf,
    },
    /// The lock file could not be created or written.
    #[error("could not create the lock file {}: {source}", path.display())]
    LockIo {
        /// The lock file.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The store's filesystem does not have room for this fetch.
    #[error(
        "not enough free space at {}: {required} bytes are required including a 25% margin, \
         {available} available",
        path.display()
    )]
    InsufficientSpace {
        /// The store root.
        path: PathBuf,
        /// Required bytes, margin included.
        required: u64,
        /// Available bytes.
        available: u64,
    },
    /// The free-space lookup failed.
    #[error("could not check free space at {}: {source}", path.display())]
    SpaceCheck {
        /// The store root.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The strict pass found the client jar corrupt.
    #[error("client jar verification failed at {}: {source}", path.display())]
    JarVerify {
        /// The jar path.
        path: PathBuf,
        /// Underlying error.
        source: StoreError,
    },
}

/// Fetches, verifies and stores everything the client needs for one version.
///
/// The steps run in a fixed order: take the store lock, resolve the version
/// document, check the jar descriptor against the pinned 1.8.9 constants,
/// check the store's filesystem for room, resolve the asset index, download
/// every object the index lists, download the client jar and, when
/// `options.verify` is set, re-hash every object and the jar; the result of
/// that pass is the report's `verification` field.
///
/// With `options.dry_run`, the run stops after the index: the local store is
/// only read, the plan lands in the report's `planned` fields, and nothing is
/// written beyond the transient lock file.
pub fn fetch_version(
    store: &Store,
    http: &dyn HttpClient,
    options: &FetchOptions,
    mut progress: impl FnMut(Progress),
) -> Result<FetchReport, FetchError> {
    let version = options.version.as_str();
    if !is_safe_id(version) {
        return Err(FetchError::BadId {
            what: "version",
            id: options.version.clone(),
        });
    }

    // Held for the whole run and dropped, and so removed, on every exit path.
    let _lock = Lock::acquire(store.root().join(LOCK_FILE_NAME))?;
    let transport = Transfer::new(http);
    let mut report = FetchReport::default();

    let document = resolve_version_json(store, &transport, options, &mut report, &mut progress)?;

    if !options.dry_run {
        let required = required_bytes(
            document.asset_index.total_size,
            document.downloads.client.size,
        );
        require_free_space(store.root(), required)?;
    }

    let index = resolve_index(
        store,
        &transport,
        &document,
        options,
        &mut report,
        &mut progress,
    )?;
    let objects = distinct_objects(&index);

    if options.dry_run {
        plan_dry_run(store, &document, &objects, version, &mut report)?;
        return Ok(report);
    }

    let total = objects.len() as u64;
    for (position, (hash, size)) in objects.iter().enumerate() {
        let before = transport.calls();
        store.fetch_object(&transport, hash, *size)?;
        if transport.calls() == before {
            report.reused += 1;
        }
        progress(Progress::step("objects", position as u64 + 1, total));
    }

    if fetch_jar(store, &transport, &document, version)? {
        report.reused += 1;
    }
    progress(Progress::step("jar", 1, 1));

    if options.verify {
        let verification = store.verify_objects(&objects);
        verify_jar(store, &document, version)?;
        report.verification = Some(verification);
    }

    report.downloaded = transport.calls() as usize;
    report.bytes = transport.transferred();
    Ok(report)
}

/// Resolves the version document: the local copy when it is present, names the
/// requested version and passes the pinned-constant guard, the manifest chain
/// otherwise.
///
/// A local copy that does not parse, names another version or fails the guard
/// is re-resolved from piston-meta, the way a corrupt object is re-fetched,
/// and replaced once the fresh, verified document passes the same checks.
/// Nothing is stored when a check refuses, and a dry run stores nothing at all.
fn resolve_version_json(
    store: &Store,
    http: &dyn HttpClient,
    options: &FetchOptions,
    report: &mut FetchReport,
    progress: &mut impl FnMut(Progress),
) -> Result<VersionJson, FetchError> {
    let version = options.version.as_str();
    let path = store.version_json_path(version);
    if let Ok(bytes) = fs::read(&path) {
        if let Ok(document) = parse_version_json(as_str(&bytes, VERSION_DOCUMENT)?) {
            if document.id == version && check_jar_descriptor(&document, version).is_ok() {
                report.reused += 1;
                progress(Progress::step("version", 1, 1));
                return Ok(document);
            }
        }
        // Unreadable, unparsable, for another version or guarded: fetch below.
    }

    let body = http.get(VERSION_MANIFEST_URL)?;
    let manifest =
        parse_manifest(as_str(&body, VERSION_MANIFEST)?).map_err(|source| FetchError::Json {
            what: VERSION_MANIFEST,
            source,
        })?;
    progress(Progress::step("manifest", 1, 1));

    let entry = find_version(&manifest, version).ok_or_else(|| FetchError::VersionNotFound {
        version: version.to_string(),
    })?;

    // The manifest entry carries a hash but no size, so the document is
    // checked against that hash alone.
    let body = http.get(&entry.url)?;
    verify_sha1(&body, &entry.sha1)?;
    let document = parse_version_json(as_str(&body, VERSION_DOCUMENT)?).map_err(|source| {
        FetchError::Json {
            what: VERSION_DOCUMENT,
            source,
        }
    })?;
    if document.id != version {
        return Err(FetchError::VersionMismatch {
            requested: version.to_string(),
            found: document.id,
        });
    }
    check_jar_descriptor(&document, version)?;
    if !options.dry_run {
        write_atomic(&path, &body)?;
    }
    progress(Progress::step("version", 1, 1));
    Ok(document)
}

/// Resolves the asset index: the stored copy when it re-hashes against the
/// version document, a fresh verified download otherwise.
///
/// A stored copy that was unreadable or did not re-hash is replaced by the
/// fresh download; a dry run stores nothing at all.
fn resolve_index(
    store: &Store,
    http: &dyn HttpClient,
    document: &VersionJson,
    options: &FetchOptions,
    report: &mut FetchReport,
    progress: &mut impl FnMut(Progress),
) -> Result<AssetIndex, FetchError> {
    let info = &document.asset_index;
    if !is_safe_id(&info.id) {
        return Err(FetchError::BadId {
            what: "asset index",
            id: info.id.clone(),
        });
    }
    let path = store.index_path(&info.id);
    if let Ok(bytes) = fs::read(&path) {
        if verify_bytes(&bytes, &info.sha1, info.size).is_ok() {
            let index = AssetIndex::parse(as_str(&bytes, ASSET_INDEX)?).map_err(|source| {
                FetchError::Json {
                    what: ASSET_INDEX,
                    source,
                }
            })?;
            report.reused += 1;
            progress(Progress::step("index", 1, 1));
            return Ok(index);
        }
    }

    let body = http.get(&info.url)?;
    verify_bytes(&body, &info.sha1, info.size)?;
    let index =
        AssetIndex::parse(as_str(&body, ASSET_INDEX)?).map_err(|source| FetchError::Json {
            what: ASSET_INDEX,
            source,
        })?;
    if !options.dry_run {
        write_atomic(&path, &body)?;
    }
    progress(Progress::step("index", 1, 1));
    Ok(index)
}

/// The distinct objects the index lists, in logical-path order.
///
/// The index may name one object under several logical paths; each hash is
/// returned once, so a shared object is fetched and verified once.
fn distinct_objects(index: &AssetIndex) -> Vec<(&str, u64)> {
    let mut seen = BTreeSet::new();
    let mut objects = Vec::new();
    for object in index.objects.values() {
        if seen.insert(object.hash.as_str()) {
            objects.push((object.hash.as_str(), object.size));
        }
    }
    objects
}

/// Fills in the plan for a dry run: what a real run would still download.
///
/// Every index object and the jar are checked against the store as they are;
/// the ones that are missing or not verifiable are the plan.
fn plan_dry_run(
    store: &Store,
    document: &VersionJson,
    objects: &[(&str, u64)],
    version: &str,
    report: &mut FetchReport,
) -> Result<(), FetchError> {
    for (hash, size) in objects {
        if store.verify_object(hash, *size).is_ok() {
            report.reused += 1;
        } else {
            report.planned += 1;
            report.planned_bytes += size;
        }
    }
    let client = &document.downloads.client;
    if jar_is_valid(store, client, version)? {
        report.reused += 1;
    } else {
        report.planned += 1;
        report.planned_bytes += client.size;
    }
    Ok(())
}

/// Downloads and stores the client jar unless the stored one is valid.
///
/// Returns true when the stored jar was reused. A jar that does not verify is
/// replaced by the atomic write.
fn fetch_jar(
    store: &Store,
    http: &dyn HttpClient,
    document: &VersionJson,
    version: &str,
) -> Result<bool, FetchError> {
    let client = &document.downloads.client;
    if jar_is_valid(store, client, version)? {
        return Ok(true);
    }
    let body = http.get(&client.url)?;
    verify_bytes(&body, &client.sha1, client.size)?;
    write_atomic(&store.client_jar_path(version), &body)?;
    Ok(false)
}

/// True when the stored client jar re-hashes against its descriptor.
fn jar_is_valid(store: &Store, client: &DownloadInfo, version: &str) -> Result<bool, FetchError> {
    let path = store.client_jar_path(version);
    match fs::read(&path) {
        Ok(bytes) => Ok(verify_bytes(&bytes, &client.sha1, client.size).is_ok()),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StoreError::Io { path, source }.into()),
    }
}

/// Re-hashes the stored client jar for the strict verification pass.
fn verify_jar(store: &Store, document: &VersionJson, version: &str) -> Result<(), FetchError> {
    let path = store.client_jar_path(version);
    let client = &document.downloads.client;
    let bytes = fs::read(&path).map_err(|source| StoreError::Io {
        path: path.clone(),
        source,
    })?;
    verify_bytes(&bytes, &client.sha1, client.size)
        .map_err(|source| FetchError::JarVerify { path, source })
}

/// Asserts the pinned client jar constants for 1.8.9.
///
/// The constants pin what the version document must say about the client jar,
/// so a change upstream is refused instead of fetched.
fn check_jar_descriptor(document: &VersionJson, version: &str) -> Result<(), FetchError> {
    if version != "1.8.9" {
        return Ok(());
    }
    let client = &document.downloads.client;
    if client.sha1 == CLIENT_1_8_9_SHA1 && client.size == CLIENT_1_8_9_SIZE {
        return Ok(());
    }
    Err(FetchError::JarDescriptor {
        found_sha1: client.sha1.clone(),
        found_size: client.size,
        pinned_sha1: CLIENT_1_8_9_SHA1,
        pinned_size: CLIENT_1_8_9_SIZE,
    })
}

/// The free bytes a fetch requires: the index total plus the client jar, plus
/// a 25% margin on top.
fn required_bytes(index_total: u64, jar_size: u64) -> u64 {
    let needed = index_total.saturating_add(jar_size);
    needed.saturating_add(needed / 4)
}

/// The space error when `available` is below `required`, and nothing otherwise.
fn space_shortfall(path: &Path, available: u64, required: u64) -> Option<FetchError> {
    (available < required).then(|| FetchError::InsufficientSpace {
        path: path.to_path_buf(),
        required,
        available,
    })
}

/// Refuses a run when the store's filesystem has less room than required.
fn require_free_space(path: &Path, required: u64) -> Result<(), FetchError> {
    let available = fs4::available_space(path).map_err(|source| FetchError::SpaceCheck {
        path: path.to_path_buf(),
        source,
    })?;
    match space_shortfall(path, available, required) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Borrows `bytes` as UTF-8 text, naming `what` in the error.
fn as_str<'a>(bytes: &'a [u8], what: &'static str) -> Result<&'a str, FetchError> {
    std::str::from_utf8(bytes).map_err(|source| FetchError::Text { what, source })
}

/// True when `id` can be used as a single path segment: non-empty, without
/// separators or a NUL, and not a dot name.
fn is_safe_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\']) && !id.contains('\0')
}

/// The name of the version manifest document, for errors.
const VERSION_MANIFEST: &str = "version manifest";
/// The name of the version document, for errors.
const VERSION_DOCUMENT: &str = "version document";
/// The name of the asset index document, for errors.
const ASSET_INDEX: &str = "asset index";

/// Wraps a transport to count the transfers a run makes, so the report says
/// exactly what went over the wire.
struct Transfer<'a> {
    inner: &'a dyn HttpClient,
    calls: Cell<u64>,
    bytes: Cell<u64>,
}

impl<'a> Transfer<'a> {
    /// Wraps `inner`.
    fn new(inner: &'a dyn HttpClient) -> Self {
        Self {
            inner,
            calls: Cell::new(0),
            bytes: Cell::new(0),
        }
    }

    /// The successful transfers so far.
    fn calls(&self) -> u64 {
        self.calls.get()
    }

    /// The bytes transferred so far.
    fn transferred(&self) -> u64 {
        self.bytes.get()
    }
}

impl HttpClient for Transfer<'_> {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let body = self.inner.get(url)?;
        self.calls.set(self.calls.get() + 1);
        self.bytes.set(self.bytes.get() + body.len() as u64);
        Ok(body)
    }
}

/// The fetch lock: a file created with `create_new`, so exactly one process
/// can hold it, removed when the guard drops.
#[derive(Debug)]
struct Lock {
    path: PathBuf,
}

impl Lock {
    /// Creates the lock file, refusing when another run holds it.
    fn acquire(path: PathBuf) -> Result<Self, FetchError> {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(source) = writeln!(file, "{}", std::process::id()) {
                    // The lock is the file itself, so a failed write must not
                    // leave it behind.
                    let _ = fs::remove_file(&path);
                    return Err(FetchError::LockIo { path, source });
                }
                Ok(Self { path })
            }
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                Err(FetchError::Locked { path })
            }
            Err(source) => Err(FetchError::LockIo { path, source }),
        }
    }
}

impl Drop for Lock {
    /// Releases the lock, whatever the run's outcome.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the fetch flow's pieces that need no transport: the
    //! space arithmetic, the id guard and the lock file.

    use std::path::Path;

    use super::{FetchError, Lock, is_safe_id, required_bytes, space_shortfall};

    #[test]
    fn the_space_requirement_is_the_index_total_plus_the_jar_plus_25_percent() {
        assert_eq!(required_bytes(0, 0), 0);
        assert_eq!(required_bytes(100, 100), 250, "200 bytes plus a 25% margin");
        assert_eq!(
            required_bytes(114_885_064, 8_461_484),
            154_183_185,
            "the 1.8.9 index total plus the jar, plus a 25% margin"
        );
    }

    #[test]
    fn a_shortfall_reports_both_numbers_and_an_exact_fit_passes() {
        let path = Path::new("/store");
        assert!(
            space_shortfall(path, 154_183_184, 154_183_185).is_some(),
            "a single byte short must refuse"
        );
        assert!(
            space_shortfall(path, 154_183_185, 154_183_185).is_none(),
            "the exact requirement must pass"
        );

        let error = space_shortfall(path, 1_000, 2_000).expect("a shortfall");
        let message = error.to_string();
        assert!(
            message.contains("2000"),
            "the requirement is reported: {message}"
        );
        assert!(
            message.contains("1000"),
            "the available space is reported: {message}"
        );
    }

    #[test]
    fn a_version_id_that_could_escape_the_store_is_rejected() {
        assert!(is_safe_id("1.8.9"));
        assert!(is_safe_id("1.8.9-pre1"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("."));
        assert!(!is_safe_id(".."));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id("a\\b"));
        assert!(!is_safe_id("a\0b"));
    }

    #[test]
    fn the_lock_is_exclusive_removed_on_drop_and_carries_the_pid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("lock");

        let lock = Lock::acquire(path.clone()).expect("first acquire");
        assert!(path.is_file(), "the lock file exists while it is held");

        let error = Lock::acquire(path.clone()).expect_err("a second acquire must fail");
        assert!(matches!(error, FetchError::Locked { .. }), "got {error:?}");
        assert!(
            error.to_string().contains(&path.display().to_string()),
            "the error names the lock file"
        );
        assert!(path.is_file(), "the holder's lock file is left alone");

        let contents = std::fs::read_to_string(&path).expect("read the lock");
        assert_eq!(
            contents.trim(),
            std::process::id().to_string(),
            "the lock records the process id"
        );

        drop(lock);
        assert!(!path.exists(), "dropping the guard releases the lock");
    }
}
