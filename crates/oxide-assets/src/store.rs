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

/// Directories [`Store::open`] creates under the store root.
const STORE_SUBDIRS: [&str; 5] = [
    "assets/objects",
    "assets/indexes",
    "versions",
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
    /// The objects tree is never reached through a symlink: a symlinked `assets` or
    /// `assets/objects` is refused, so object paths stay inside the store.
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

    /// The final path an object with `hash` lives at.
    ///
    /// The shard directory is the first two bytes of the hash; a hash shorter than
    /// two bytes shards on itself, so this never panics. Fetch and verify reject
    /// malformed hashes up front.
    pub fn object_path(&self, hash: &str) -> PathBuf {
        self.root.join(OBJECTS_SUBDIR).join(shard(hash)).join(hash)
    }

    /// Fetches an object unless already present and valid, then returns its path.
    pub fn fetch_object(
        &self,
        http: &dyn HttpClient,
        hash: &str,
        size: u64,
    ) -> Result<PathBuf, StoreError> {
        validate_hash(hash)?;
        let path = self.object_path(hash);
        if path.exists() {
            if self.verify_object(hash, size).is_ok() {
                return Ok(path);
            }
            // Corrupted: remove and re-fetch.
            fs::remove_file(&path).map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
        }

        let url = format!("{RESOURCES_BASE_URL}/{}/{hash}", shard(hash));
        let body = http.get(&url)?;
        if body.len() as u64 != size {
            return Err(StoreError::SizeMismatch {
                hash: hash.to_string(),
                expected: size,
                actual: body.len() as u64,
            });
        }
        let actual = sha1_hex(&body);
        if !actual.eq_ignore_ascii_case(hash) {
            return Err(StoreError::HashMismatch {
                expected: hash.to_string(),
                actual,
            });
        }
        write_object_atomic(&path, &body)?;
        Ok(path)
    }

    /// Re-hashes an object already on disk.
    pub fn verify_object(&self, hash: &str, size: u64) -> Result<(), StoreError> {
        validate_hash(hash)?;
        let path = self.object_path(hash);
        let bytes = fs::read(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        if bytes.len() as u64 != size {
            return Err(StoreError::SizeMismatch {
                hash: hash.to_string(),
                expected: size,
                actual: bytes.len() as u64,
            });
        }
        let actual = sha1_hex(&bytes);
        if !actual.eq_ignore_ascii_case(hash) {
            return Err(StoreError::HashMismatch {
                expected: hash.to_string(),
                actual,
            });
        }
        Ok(())
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
    temp.as_file().sync_all().map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    apply_mode(temp.path(), read_only)?;
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

/// True when `hash` is a 40 character hex string.
fn validate_hash(hash: &str) -> Result<(), StoreError> {
    let looks_like_sha1 = hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
    if looks_like_sha1 {
        Ok(())
    } else {
        Err(StoreError::BadHash {
            hash: hash.to_string(),
        })
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
