//! The verified on-disk asset store. Every write is atomic.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::http::{HttpClient, HttpError};

/// Where the Mojang object resource server lives.
const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

/// The content-addressed object directory, relative to the store root.
const OBJECTS_SUBDIR: &str = "assets/objects";

/// The asset index directory, relative to the store root.
const INDEXES_SUBDIR: &str = "assets/indexes";

/// The directory holding the per-version files, relative to the store root.
const VERSIONS_SUBDIR: &str = "versions";

/// Directories [`Store::open`] creates under the store root.
const STORE_SUBDIRS: [&str; 5] = [
    OBJECTS_SUBDIR,
    INDEXES_SUBDIR,
    VERSIONS_SUBDIR,
    "extracted",
    "skins",
];

/// Errors from store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Filesystem failure.
    #[error("filesystem error at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// Download failure.
    #[error("download failed: {0}")]
    Http(#[from] HttpError),
    /// The bytes did not match the expected hash.
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        /// The hash that was expected.
        expected: String,
        /// The hash of the bytes that were actually read.
        actual: String,
    },
    /// The object was the wrong size.
    #[error("size mismatch for {hash}: expected {expected} bytes, got {actual}")]
    SizeMismatch {
        /// Object hash.
        hash: String,
        /// Expected size.
        expected: u64,
        /// Observed size.
        actual: u64,
    },
    /// The hash is not a 40 character hex string.
    #[error("malformed object hash: {hash:?}")]
    BadHash {
        /// The offending hash.
        hash: String,
    },
    /// A store path that must not be a symlink is one.
    #[error("refusing symlinked store path at {path}")]
    SymlinkedPath {
        /// The symlink that was found.
        path: PathBuf,
    },
}

/// A report of a verification pass.
#[derive(Debug, Default)]
pub struct VerifyReport {
    /// Objects that were present and verified.
    pub objects: usize,
    /// Total bytes on disk of the verified objects.
    pub bytes: u64,
    /// Objects that were missing.
    pub missing: Vec<String>,
    /// Objects whose bytes did not match their hash.
    pub mismatched: Vec<String>,
}

impl VerifyReport {
    /// True when nothing is missing and nothing is mismatched.
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.mismatched.is_empty()
    }
}

/// The asset store rooted at a data directory.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens (creating if needed) the store under `data_dir`.
    ///
    /// `assets` and `assets/objects` are refused if either is a symlink, before
    /// any directory is created, so those two paths cannot point the objects
    /// tree out of the store. Deeper paths (a shard directory, say) are not
    /// inspected.
    pub fn open(data_dir: PathBuf) -> Result<Self, StoreError> {
        let root = data_dir;
        for sub in ["assets", OBJECTS_SUBDIR] {
            let path = root.join(sub);
            if is_symlink(&path) {
                return Err(StoreError::SymlinkedPath { path });
            }
        }
        for sub in STORE_SUBDIRS {
            let path = root.join(sub);
            fs::create_dir_all(&path).map_err(|source| StoreError::Io { path, source })?;
        }
        Ok(Self { root })
    }

    /// The final path for `hash`, which is used verbatim.
    ///
    /// The shard directory is the first two bytes of the hash; a hash shorter
    /// than two bytes shards on itself, so this never panics. Fetch and verify
    /// reject malformed hashes up front and normalize the rest to lowercase
    /// before asking for a path.
    pub fn object_path(&self, hash: &str) -> PathBuf {
        self.root.join(OBJECTS_SUBDIR).join(shard(hash)).join(hash)
    }

    /// The store root, the directory every store path lives under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory holding one version's files.
    ///
    /// `version` is used as a path segment; callers pass a version id from the
    /// version manifest.
    pub fn version_dir(&self, version: &str) -> PathBuf {
        self.root.join(VERSIONS_SUBDIR).join(version)
    }

    /// The version document for `version`, copied from piston-meta.
    pub fn version_json_path(&self, version: &str) -> PathBuf {
        self.version_dir(version).join("version.json")
    }

    /// The client jar for `version`.
    pub fn client_jar_path(&self, version: &str) -> PathBuf {
        self.version_dir(version).join("client.jar")
    }

    /// The asset index document for index id `id`.
    ///
    /// `id` is used as the file name's stem; callers pass the id from the
    /// version document.
    pub fn index_path(&self, id: &str) -> PathBuf {
        self.root.join(INDEXES_SUBDIR).join(format!("{id}.json"))
    }

    /// Re-hashes every expected object and reports what it finds.
    ///
    /// Each pair is an object's hash and its expected size; pass each object
    /// once. A missing object is listed in [`VerifyReport::missing`], and an
    /// object that is present but not verifiable (wrong bytes, wrong size, or
    /// unreadable) in [`VerifyReport::mismatched`]. The verified objects are
    /// counted, and their on-disk bytes summed, in the report's `objects` and
    /// `bytes`.
    pub fn verify_objects(&self, expected: &[(&str, u64)]) -> VerifyReport {
        let mut report = VerifyReport::default();
        for &(hash, size) in expected {
            let Ok(hash) = validate_hash(hash) else {
                report.mismatched.push(hash.to_string());
                continue;
            };
            let path = self.object_path(&hash);
            match fs::read(&path) {
                Ok(bytes) => match verify_bytes(&bytes, &hash, size) {
                    Ok(()) => {
                        report.objects += 1;
                        report.bytes += bytes.len() as u64;
                    }
                    Err(_) => report.mismatched.push(hash),
                },
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    report.missing.push(hash);
                }
                Err(_) => report.mismatched.push(hash),
            }
        }
        report
    }

    /// Fetches an object unless already present and valid, then returns its path.
    ///
    /// The hash is normalized to lowercase first. A cached object whose bytes
    /// are provably wrong is dropped and fetched again; a cached object that
    /// merely cannot be read is left alone, and the failure is returned.
    pub fn fetch_object(
        &self,
        http: &dyn HttpClient,
        hash: &str,
        size: u64,
    ) -> Result<PathBuf, StoreError> {
        let hash = validate_hash(hash)?;
        let path = self.object_path(&hash);
        if path.exists() {
            match self.verify_object(&hash, size) {
                Ok(()) => return Ok(path),
                // Provably wrong, or already gone: clear the way for a fetch.
                Err(error) if proves_corruption(&error) => remove_object(&path)?,
                // A failure that proves nothing about the bytes (a permission
                // problem, say) must not cost the cached object.
                Err(error) => return Err(error),
            }
        }

        let url = format!("{RESOURCES_BASE_URL}/{}/{hash}", shard(&hash));
        let body = http.get(&url)?;
        verify_bytes(&body, &hash, size)?;
        write_object_atomic(&path, &body)?;
        Ok(path)
    }

    /// Re-hashes the object already on disk; the hash is normalized to
    /// lowercase first.
    pub fn verify_object(&self, hash: &str, size: u64) -> Result<(), StoreError> {
        let hash = validate_hash(hash)?;
        let path = self.object_path(&hash);
        let bytes = fs::read(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        verify_bytes(&bytes, &hash, size)
    }
}

/// Writes bytes via a temp file in the same directory, syncs it, then renames into
/// place; a crash never leaves a partial file under the final name.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    write_atomic_impl(path, bytes, false)
}

/// Writes an object file: atomic, and read-only afterwards on Unix (mode 0o444).
fn write_object_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    write_atomic_impl(path, bytes, true)
}

/// The shared atomic write: temp file in the target's directory, then rename.
fn write_atomic_impl(path: &Path, bytes: &[u8], read_only: bool) -> Result<(), StoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| StoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|source| StoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    temp.write_all(bytes).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    temp.flush().map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // The mode lands before the sync so a crash cannot leave the final name
    // pointing at a writable object.
    apply_mode(temp.path(), read_only)?;
    temp.as_file().sync_all().map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    temp.persist(path).map_err(|error| StoreError::Io {
        path: path.to_path_buf(),
        source: error.error,
    })?;
    Ok(())
}

/// Sets the file mode: 0o444 for read-only objects, 0o644 otherwise.
#[cfg(unix)]
fn apply_mode(path: &Path, read_only: bool) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = if read_only { 0o444 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Without Unix permission bits there is nothing to set.
#[cfg(not(unix))]
fn apply_mode(_path: &Path, _read_only: bool) -> Result<(), StoreError> {
    Ok(())
}

/// Normalizes `hash` to lowercase hex, rejecting anything that is not exactly
/// forty hex characters.
fn validate_hash(hash: &str) -> Result<String, StoreError> {
    let looks_like_sha1 = hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
    if looks_like_sha1 {
        Ok(hash.to_ascii_lowercase())
    } else {
        Err(StoreError::BadHash {
            hash: hash.to_string(),
        })
    }
}

/// Checks `bytes` against the expected SHA-1, ignoring case.
///
/// This is the check the fetch flow uses for a document whose descriptor
/// records no size (the version document), and the hash half of
/// [`verify_bytes`].
pub(crate) fn verify_sha1(bytes: &[u8], hash: &str) -> Result<(), StoreError> {
    let actual = sha1_hex(bytes);
    if !actual.eq_ignore_ascii_case(hash) {
        return Err(StoreError::HashMismatch {
            expected: hash.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Checks `bytes` against the expected `hash` and `size`, cheapest check first:
/// the length, then the SHA-1. The hash has already been lowercased when it
/// comes from [`validate_hash`].
pub(crate) fn verify_bytes(bytes: &[u8], hash: &str, size: u64) -> Result<(), StoreError> {
    if bytes.len() as u64 != size {
        return Err(StoreError::SizeMismatch {
            hash: hash.to_string(),
            expected: size,
            actual: bytes.len() as u64,
        });
    }
    verify_sha1(bytes, hash)
}

/// True when a verification failure proves the bytes cannot be trusted: a hash
/// mismatch, a size mismatch, or an object that is no longer there. Any other
/// failure (an unreadable file, say) proves nothing and must not cost the
/// cached object.
fn proves_corruption(error: &StoreError) -> bool {
    match error {
        StoreError::HashMismatch { .. } | StoreError::SizeMismatch { .. } => true,
        StoreError::Io { source, .. } => source.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

/// Removes a cached object, tolerating one that is already gone.
fn remove_object(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// True when `path` exists and is a symlink (checked without following it).
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

/// The shard directory for `hash`: its first two bytes, or the hash when shorter.
fn shard(hash: &str) -> &str {
    hash.get(..2).unwrap_or(hash)
}

/// The lowercase hex SHA-1 of `bytes`.
fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

#[cfg(test)]
mod tests {
    //! Unit tests for the small pieces the store's public paths build on.

    use std::io;
    use std::path::PathBuf;

    use super::{StoreError, VerifyReport, proves_corruption};

    /// An IO failure of `kind` at an object path.
    fn io_error(kind: io::ErrorKind) -> StoreError {
        StoreError::Io {
            path: PathBuf::from("object"),
            source: io::Error::from(kind),
        }
    }

    #[test]
    fn a_verify_report_is_clean_only_when_it_lists_nothing() {
        assert!(VerifyReport::default().is_clean());

        let missing = VerifyReport {
            missing: vec!["aa".to_string()],
            ..VerifyReport::default()
        };
        assert!(!missing.is_clean(), "a missing object is not clean");

        let mismatched = VerifyReport {
            mismatched: vec!["bb".to_string()],
            ..VerifyReport::default()
        };
        assert!(!mismatched.is_clean(), "a mismatched object is not clean");
    }

    #[test]
    fn only_proven_corruption_justifies_dropping_a_cached_object() {
        assert!(proves_corruption(&io_error(io::ErrorKind::NotFound)));
        assert!(proves_corruption(&StoreError::HashMismatch {
            expected: "aa".to_string(),
            actual: "bb".to_string(),
        }));
        assert!(proves_corruption(&StoreError::SizeMismatch {
            hash: "aa".to_string(),
            expected: 3,
            actual: 4,
        }));
        assert!(
            !proves_corruption(&io_error(io::ErrorKind::PermissionDenied)),
            "a failure that proves nothing must not cost the cached object"
        );
    }
}
