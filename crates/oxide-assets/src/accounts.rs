//! The account store: non-secret records in `accounts.json`, refresh tokens
//! in the OS keyring.
//!
//! Tiers 1 and 3 only (owner Q1): tier 1 persists the AAD refresh token in
//! the OS keyring and the account record beside it; tier 3 — no keyring
//! available — persists nothing and says so through
//! [`SaveOutcome::SessionOnly`]. Tier 2 (an encrypted-at-rest passphrase
//! file) is deferred past v1 and is **not** implemented here: a plaintext
//! fallback would be worse than none (survey §4.3).
//!
//! The no-secret rule is absolute: the refresh token's value never reaches a
//! file byte, a log line, or an error string. [`AccountRecord`] carries only
//! the non-secret half (uuid, name, xuid); [`StoreError`]'s reasons are fixed
//! literals chosen from the failure kind — platform text, keys and values
//! are never interpolated.
//!
//! Decided load rules (pinned by the `accounts` integration suite):
//!
//! * Unknown JSON fields are dropped (serde's default); a file whose
//!   `version` is not [`FILE_VERSION`] is treated like a corrupt file.
//! * A corrupt file is renamed beside itself to `accounts.json.bak`
//!   (overwriting any previous backup) and treated as empty; the bytes stay
//!   recoverable next to the store, never merged.
//! * [`AccountStore::remove`] is idempotent and writes the file only when it
//!   actually removed a record.
//! * [`AccountStore::save`] for an existing uuid replaces both halves.
//!
//! Ordering rules: `save` writes the secret half first, then the record — a
//! secret failure (of any kind) leaves no orphan record behind. `remove`
//! deletes the secret half first, then the record — a secret-backend failure
//! aborts with the record intact.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The account file's name inside the data directory.
const ACCOUNTS_FILE_NAME: &str = "accounts.json";
/// Where a corrupt account file is renamed to, beside itself.
const BACKUP_FILE_NAME: &str = "accounts.json.bak";
/// The account file's schema version.
const FILE_VERSION: u32 = 1;
/// The keyring service every refresh token is stored under.
const KEYRING_SERVICE: &str = "oxidecraft";

/// The keyring is locked or access was denied.
const UNAVAILABILITY_LOCKED: &str = "the OS keyring is locked or access was denied";
/// The keyring failed at the platform level.
const UNAVAILABILITY_FAILED: &str = "the OS keyring failed";
/// The stored secret is not usable text.
const BACKEND_BAD_DATA: &str = "the stored secret is not usable";
/// The account id is not usable as a keyring key.
const BACKEND_BAD_KEY: &str = "the account id is not usable as a keyring key";
/// More than one keyring entry matches the account.
const BACKEND_AMBIGUOUS: &str = "more than one keyring entry matches the account";
/// A keyring answer the caller already handled (defensive arm only).
const BACKEND_UNEXPECTED: &str = "unexpected keyring response";

/// The non-secret half of an account: the only thing that ever reaches
/// `accounts.json`. No secret lives in this type, so its `Debug` is safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRecord {
    /// The player's uuid, 32 lowercase hex digits, no dashes.
    pub uuid_hex: String,
    /// The player's current profile name.
    pub name: String,
    /// The Xbox user id, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xuid: Option<String>,
}

/// How a [`AccountStore::save`] landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The secret half and the record half both persisted.
    Saved,
    /// Nothing persisted: the keyring is not available in this session, so
    /// the account lives for this process only and the next start needs a
    /// fresh sign-in. The reason is a fixed literal, never a secret.
    SessionOnly {
        /// Why nothing persisted.
        reason: String,
    },
}

/// Failures from the account store. No variant carries a secret: reasons are
/// fixed literals (see the module docs).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A filesystem failure.
    #[error("filesystem error at {path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The secret backend is present but unusable (no session bus, no
    /// provider, a locked store): the tier-3 trigger. `save` maps this to
    /// [`SaveOutcome::SessionOnly`]; every other method returns it.
    #[error("secret store unavailable: {reason}")]
    Unavailable {
        /// The fixed reason.
        reason: String,
    },
    /// The secret backend failed in a way that is not availability (stored
    /// bytes that are not text, an unusable key, an ambiguous entry).
    #[error("secret store failed: {reason}")]
    Backend {
        /// The fixed reason.
        reason: String,
    },
}

/// The secret half's backend: the key is the account's uuid, the value is
/// the refresh token. `get` answers `Ok(None)` for an unknown account;
/// `delete` succeeds for one; an unavailable backend reports
/// [`StoreError::Unavailable`] (the tier-3 shape `save` reacts to).
pub trait SecretStore: Send + Sync {
    /// Reads the secret stored under `key`, if any.
    fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    /// Stores `value` under `key`, replacing any previous value.
    fn set(&self, key: &str, value: &str) -> Result<(), StoreError>;
    /// Deletes the secret stored under `key`, if any.
    fn delete(&self, key: &str) -> Result<(), StoreError>;
}

/// The production [`SecretStore`] over the `keyring` crate, service
/// `oxidecraft`, one entry per account uuid holding its refresh token.
///
/// Linux backend: Secret Service, with the survey §4.3 caveats as the doc
/// comment. A running session D-Bus plus a provider (`gnome-keyring-daemon`,
/// `kwalletd`, or a bridge) must be installed and unlocked, or every
/// operation reports [`StoreError::Unavailable`] and tier 3 applies.
/// Per-user entries are enumerable by other processes running as the user:
/// the threat model is other users and casual file exfiltration, not malware
/// running as you — stated, not solved.
///
/// Constructing this backend touches nothing: the keyring entry is built per
/// operation, so [`AccountStore::open`] succeeds (and this type constructs)
/// with no daemon running.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringSecrets;

impl KeyringSecrets {
    /// The production backend: service `oxidecraft`. Touches no daemon.
    pub fn new() -> Self {
        Self
    }

    /// Builds the keyring entry for `key`, mapping a construction failure to
    /// the tier-3 shape.
    fn entry(&self, key: &str) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(KEYRING_SERVICE, key).map_err(|error| classify(&error))
    }
}

impl SecretStore for KeyringSecrets {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        let entry = self.entry(key)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(other) => Err(classify(&other)),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let entry = self.entry(key)?;
        entry.set_password(value).map_err(|error| classify(&error))
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        let entry = self.entry(key)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(other) => Err(classify(&other)),
        }
    }
}

/// Maps a keyring failure to the store's error shape: availability failures
/// (locked store, platform failure) become the tier-3 trigger
/// ([`StoreError::Unavailable`]); everything else becomes
/// [`StoreError::Backend`]. Every reason is a fixed literal — platform text,
/// keys and values never cross into an error string.
fn classify(error: &keyring::Error) -> StoreError {
    match error {
        keyring::Error::NoStorageAccess(_) => StoreError::Unavailable {
            reason: UNAVAILABILITY_LOCKED.to_string(),
        },
        keyring::Error::PlatformFailure(_) => StoreError::Unavailable {
            reason: UNAVAILABILITY_FAILED.to_string(),
        },
        keyring::Error::BadEncoding(_) => StoreError::Backend {
            reason: BACKEND_BAD_DATA.to_string(),
        },
        keyring::Error::TooLong(_, _) | keyring::Error::Invalid(_, _) => StoreError::Backend {
            reason: BACKEND_BAD_KEY.to_string(),
        },
        keyring::Error::Ambiguous(_) => StoreError::Backend {
            reason: BACKEND_AMBIGUOUS.to_string(),
        },
        keyring::Error::NoEntry => StoreError::Backend {
            reason: BACKEND_UNEXPECTED.to_string(),
        },
        // `keyring::Error` is non-exhaustive: a future variant fails loud
        // rather than silently taking the tier-3 path.
        _ => StoreError::Backend {
            reason: BACKEND_UNEXPECTED.to_string(),
        },
    }
}

/// The account store: a data directory holding `accounts.json` plus a boxed
/// [`SecretStore`] holding the refresh tokens.
pub struct AccountStore {
    /// The data directory `accounts.json` lives in.
    dir: PathBuf,
    /// The secret half's backend.
    secrets: Box<dyn SecretStore>,
}

impl std::fmt::Debug for AccountStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountStore")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl AccountStore {
    /// Opens the store under `data_dir` with the keyring backend, creating
    /// the directory when needed. Building the backend touches no daemon.
    pub fn open(data_dir: PathBuf) -> Result<Self, StoreError> {
        Self::open_with(data_dir, Box::new(KeyringSecrets::new()))
    }

    /// Opens the store under `data_dir` with `secrets` as the secret backend
    /// (tests pass a memory backend), creating the directory when needed.
    pub fn open_with(data_dir: PathBuf, secrets: Box<dyn SecretStore>) -> Result<Self, StoreError> {
        std::fs::create_dir_all(&data_dir).map_err(|source| StoreError::Io {
            path: data_dir.clone(),
            source,
        })?;
        Ok(Self {
            dir: data_dir,
            secrets,
        })
    }

    /// Every stored record. Best-effort: an unreadable file reads as empty.
    pub fn list(&self) -> Vec<AccountRecord> {
        self.load().unwrap_or_default()
    }

    /// The record stored under `uuid_hex`, if any. Best-effort like
    /// [`AccountStore::list`].
    pub fn record(&self, uuid_hex: &str) -> Option<AccountRecord> {
        self.load()
            .unwrap_or_default()
            .into_iter()
            .find(|record| record.uuid_hex == uuid_hex)
    }

    /// The refresh token stored under `uuid_hex`, if any.
    pub fn refresh_token(&self, uuid_hex: &str) -> Result<Option<String>, StoreError> {
        self.secrets.get(uuid_hex)
    }

    /// Saves both halves: the secret first, then the record, replacing any
    /// previous entry for the uuid. A secret failure of any kind writes
    /// nothing — no orphan record is ever left behind. A keyring
    /// availability failure ([`StoreError::Unavailable`]) lands as
    /// [`SaveOutcome::SessionOnly`]: tier 3, the account lives for this
    /// process only.
    pub fn save(
        &self,
        record: &AccountRecord,
        refresh_token: &str,
    ) -> Result<SaveOutcome, StoreError> {
        if let Err(error) = self.secrets.set(&record.uuid_hex, refresh_token) {
            match error {
                StoreError::Unavailable { reason } => {
                    return Ok(SaveOutcome::SessionOnly { reason });
                }
                other => return Err(other),
            }
        }
        let mut accounts = self.load()?;
        if let Some(slot) = accounts
            .iter_mut()
            .find(|known| known.uuid_hex == record.uuid_hex)
        {
            *slot = record.clone();
        } else {
            accounts.push(record.clone());
        }
        self.store(&accounts)?;
        Ok(SaveOutcome::Saved)
    }

    /// Deletes both halves: the secret first, then the record. Idempotent —
    /// removing an unknown uuid succeeds and writes no file. A
    /// secret-backend failure aborts with the record intact.
    pub fn remove(&self, uuid_hex: &str) -> Result<(), StoreError> {
        self.secrets.delete(uuid_hex)?;
        let mut accounts = self.load()?;
        let kept = accounts.len();
        accounts.retain(|known| known.uuid_hex != uuid_hex);
        if accounts.len() != kept {
            self.store(&accounts)?;
        }
        Ok(())
    }

    /// The account file's path.
    fn accounts_path(&self) -> PathBuf {
        self.dir.join(ACCOUNTS_FILE_NAME)
    }

    /// The backup path beside the account file.
    fn backup_path(&self) -> PathBuf {
        self.dir.join(BACKUP_FILE_NAME)
    }

    /// Reads the records. A missing file is empty. A file that does not
    /// parse, or whose `version` is not [`FILE_VERSION`], is renamed beside
    /// itself to `accounts.json.bak` (replacing any previous backup; a
    /// backup failure is best-effort and never fails the load) and reads as
    /// empty — the bytes stay recoverable next to the store, never merged.
    fn load(&self) -> Result<Vec<AccountRecord>, StoreError> {
        let path = self.accounts_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let file: AccountsFile = match serde_json::from_slice::<AccountsFile>(&bytes) {
            Ok(file) if file.version == FILE_VERSION => file,
            _ => {
                let _ = std::fs::rename(&path, self.backup_path());
                return Ok(Vec::new());
            }
        };
        Ok(file.accounts)
    }

    /// Writes the records atomically (temp file in the same directory, then
    /// rename — a crash never leaves a partial file under the final name).
    /// A small local helper: `store::write_atomic` returns that module's own
    /// error type, so reusing it would only wrap one `Io` in another.
    fn store(&self, accounts: &[AccountRecord]) -> Result<(), StoreError> {
        let path = self.accounts_path();
        let file = AccountsFile {
            version: FILE_VERSION,
            accounts: accounts.to_vec(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|source| StoreError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
        })?;
        write_file_atomic(&path, &bytes)
    }
}

/// The on-disk shape: `{"version": 1, "accounts": [...]}`. Unknown fields
/// are dropped on read by serde's default.
#[derive(Serialize, Deserialize)]
struct AccountsFile {
    /// The schema version.
    version: u32,
    /// The stored records.
    #[serde(default)]
    accounts: Vec<AccountRecord>,
}

/// Writes `bytes` via a temp file in the target's directory, syncs it, then
/// renames into place.
fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o644)).map_err(
            |source| StoreError::Io {
                path: path.to_path_buf(),
                source,
            },
        )?;
    }
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

#[cfg(test)]
mod tests {
    //! Unit pins: the file shape, the fixed error strings, the keyring
    //! failure mapping, and the daemon-free construction. Store-level
    //! behaviour lives in `tests/accounts.rs` over a memory backend.

    use std::io;

    use super::*;

    /// A record with every field set.
    fn full_record() -> AccountRecord {
        AccountRecord {
            uuid_hex: "069a79f444e94726a5befca90e38aaf5".to_string(),
            name: "Notch".to_string(),
            xuid: Some("2533274792417640".to_string()),
        }
    }

    #[test]
    fn the_file_shape_round_trips_with_version_and_accounts_keys() {
        let file = AccountsFile {
            version: FILE_VERSION,
            accounts: vec![full_record()],
        };
        let text = serde_json::to_string_pretty(&file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["accounts"][0]["uuid_hex"], full_record().uuid_hex);
        assert_eq!(parsed["accounts"][0]["name"], "Notch");
        assert_eq!(parsed["accounts"][0]["xuid"], "2533274792417640");

        let bare = AccountRecord {
            xuid: None,
            ..full_record()
        };
        let bare_text = serde_json::to_string(&bare).unwrap();
        assert!(
            !bare_text.contains("xuid"),
            "an absent xuid persists as an absent key, not null"
        );
    }

    #[test]
    fn a_missing_xuid_loads_as_none_and_unknown_fields_drop() {
        let text = serde_json::json!({
            "version": 1,
            "accounts": [{
                "uuid_hex": full_record().uuid_hex,
                "name": "Notch",
                "future_field": "dropped",
                "nested": {"a": 1},
            }],
            "operator_note": "dropped",
        });
        let file: AccountsFile = serde_json::from_value(text).unwrap();
        assert_eq!(file.accounts.len(), 1);
        assert_eq!(file.accounts[0].xuid, None);
        assert_eq!(file.accounts[0].name, "Notch");
    }

    #[test]
    fn the_error_strings_are_fixed_literals() {
        assert_eq!(
            StoreError::Unavailable {
                reason: UNAVAILABILITY_LOCKED.to_string(),
            }
            .to_string(),
            "secret store unavailable: the OS keyring is locked or access was denied"
        );
        assert_eq!(
            StoreError::Backend {
                reason: BACKEND_BAD_DATA.to_string(),
            }
            .to_string(),
            "secret store failed: the stored secret is not usable"
        );
    }

    #[test]
    fn every_keyring_failure_maps_without_its_platform_text() {
        let platform = |message: &str| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(io::Error::other(message))
        };
        let cases: Vec<(keyring::Error, &str)> = vec![
            (
                keyring::Error::NoStorageAccess(platform("dbus is down: s3cr3t")),
                "secret store unavailable: the OS keyring is locked or access was denied",
            ),
            (
                keyring::Error::PlatformFailure(platform("provider blew up: s3cr3t")),
                "secret store unavailable: the OS keyring failed",
            ),
            (
                keyring::Error::BadEncoding(vec![0xff, 0xfe]),
                "secret store failed: the stored secret is not usable",
            ),
            (
                keyring::Error::TooLong("user".to_string(), 7),
                "secret store failed: the account id is not usable as a keyring key",
            ),
            (
                keyring::Error::Invalid("user".to_string(), "empty".to_string()),
                "secret store failed: the account id is not usable as a keyring key",
            ),
            (
                keyring::Error::Ambiguous(Vec::new()),
                "secret store failed: more than one keyring entry matches the account",
            ),
            (
                keyring::Error::NoEntry,
                "secret store failed: unexpected keyring response",
            ),
        ];
        for (error, expected) in cases {
            let text = classify(&error).to_string();
            assert_eq!(text, expected);
            assert!(
                !text.contains("s3cr3t"),
                "platform text must never cross into an error string"
            );
        }
    }

    #[test]
    fn the_keyring_backend_constructs_without_touching_the_daemon() {
        let backend = KeyringSecrets::new();
        let _ = KeyringSecrets;
        let debug = format!("{backend:?}");
        assert!(debug.contains("KeyringSecrets"));
    }
}
