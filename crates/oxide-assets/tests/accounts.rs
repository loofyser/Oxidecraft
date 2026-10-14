//! The account store against a memory secret backend: the round-trip, the
//! tier-3 path, and the tolerance and removal rules. Every secret value
//! below is synthetic; no test touches the OS keyring, a socket, or the
//! real data directory — each case owns a fresh temporary directory.

use std::collections::HashMap;
use std::sync::Mutex;

use oxide_assets::accounts::{AccountRecord, AccountStore, SaveOutcome, SecretStore, StoreError};

/// An in-memory [`SecretStore`]: the tier-1 shape with no daemon behind it.
struct MemorySecrets {
    /// The stored secrets by account uuid.
    inner: Mutex<HashMap<String, String>>,
}

impl MemorySecrets {
    /// An empty backend.
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.inner.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.inner
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.inner.lock().unwrap().remove(key);
        Ok(())
    }
}

/// A fresh store over a fresh temporary directory and a memory backend.
fn fresh_store(dir: &tempfile::TempDir) -> AccountStore {
    AccountStore::open_with(dir.path().to_path_buf(), Box::new(MemorySecrets::new())).unwrap()
}

/// A [`SecretStore`] shaped like a keyring with no daemon behind it: every
/// operation reports the tier-3 shape.
struct UnavailableSecrets;

impl SecretStore for UnavailableSecrets {
    fn get(&self, _key: &str) -> Result<Option<String>, StoreError> {
        Err(StoreError::Unavailable {
            reason: "the OS keyring is not reachable (test double)".to_string(),
        })
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), StoreError> {
        Err(StoreError::Unavailable {
            reason: "the OS keyring is not reachable (test double)".to_string(),
        })
    }

    fn delete(&self, _key: &str) -> Result<(), StoreError> {
        Err(StoreError::Unavailable {
            reason: "the OS keyring is not reachable (test double)".to_string(),
        })
    }
}

/// A fresh store over a fresh temporary directory and an unavailable backend.
fn session_store(dir: &tempfile::TempDir) -> AccountStore {
    AccountStore::open_with(dir.path().to_path_buf(), Box::new(UnavailableSecrets)).unwrap()
}

/// The non-secret half of the first test account.
fn record() -> AccountRecord {
    AccountRecord {
        uuid_hex: "069a79f444e94726a5befca90e38aaf5".to_string(),
        name: "Notch".to_string(),
        xuid: Some("2533274792417640".to_string()),
    }
}

/// A synthetic refresh token: recognizable, never a real credential.
const TOKEN: &str = "REFRESH-TOKEN-synthetic-value-0001";

#[test]
fn round_trip_keeps_the_secret_out_of_the_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);
    let record = record();

    let outcome = store.save(&record, TOKEN).unwrap();
    assert_eq!(outcome, SaveOutcome::Saved);

    assert_eq!(
        store.refresh_token(&record.uuid_hex).unwrap().as_deref(),
        Some(TOKEN)
    );
    assert_eq!(store.list(), vec![record.clone()]);
    assert_eq!(store.record(&record.uuid_hex), Some(record.clone()));
    assert_eq!(
        store.refresh_token("missing-uuid").unwrap(),
        None,
        "an unknown account has no secret"
    );

    let raw = std::fs::read(dir.path().join("accounts.json")).unwrap();
    let text = String::from_utf8(raw).unwrap();
    assert!(
        !text.contains(TOKEN),
        "the refresh token must never reach the file"
    );
    assert!(text.contains(&record.uuid_hex));
    assert!(text.contains("Notch"));
}

#[test]
fn an_unavailable_keyring_yields_session_only_and_writes_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = session_store(&dir);
    let record = record();

    let outcome = store.save(&record, TOKEN).unwrap();
    let reason = match outcome {
        SaveOutcome::SessionOnly { reason } => reason,
        SaveOutcome::Saved => panic!("nothing may persist without a keyring"),
    };
    assert!(
        !reason.is_empty(),
        "the outcome must say why nothing persisted"
    );
    assert!(
        !reason.contains(TOKEN),
        "the reason must not carry the secret"
    );

    assert!(
        !dir.path().join("accounts.json").exists(),
        "no orphan record may be left behind"
    );
    assert!(
        store.list().is_empty(),
        "the next list must not show the account"
    );
    assert!(store.record(&record.uuid_hex).is_none());

    let error = store.refresh_token(&record.uuid_hex).unwrap_err();
    assert!(
        !error.to_string().contains(TOKEN),
        "the error text must not carry the secret"
    );
}

#[test]
fn a_corrupt_file_is_backed_up_beside_itself_and_reads_as_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);
    let garbage = b"{\x00not json{{{{";
    std::fs::write(dir.path().join("accounts.json"), garbage).unwrap();

    assert!(store.list().is_empty());
    assert!(store.record("anything").is_none());

    let backup = std::fs::read(dir.path().join("accounts.json.bak")).unwrap();
    assert_eq!(
        backup,
        garbage.to_vec(),
        "the corrupt bytes stay recoverable beside the store"
    );

    // The store still works afterwards: a save starts from empty.
    let outcome = store.save(&record(), TOKEN).unwrap();
    assert_eq!(outcome, SaveOutcome::Saved);
    assert_eq!(store.list().len(), 1);
}

#[test]
fn an_unknown_version_is_backed_up_and_read_as_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);
    let future = serde_json::json!({
        "version": 999,
        "accounts": [{"uuid_hex": record().uuid_hex, "name": "Notch"}],
    });
    std::fs::write(
        dir.path().join("accounts.json"),
        serde_json::to_vec(&future).unwrap(),
    )
    .unwrap();

    assert!(
        store.list().is_empty(),
        "a version we do not understand is not merged"
    );
    assert!(dir.path().join("accounts.json.bak").exists());
}

#[test]
fn unknown_fields_drop_and_stay_dropped_after_a_save() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);
    let with_extras = serde_json::json!({
        "version": 1,
        "operator_note": "dropped",
        "accounts": [{
            "uuid_hex": record().uuid_hex,
            "name": "Notch",
            "xuid": "2533274792417640",
            "future_field": "dropped",
        }],
    });
    std::fs::write(
        dir.path().join("accounts.json"),
        serde_json::to_vec(&with_extras).unwrap(),
    )
    .unwrap();

    assert_eq!(store.list(), vec![record()]);

    let outcome = store.save(&record(), TOKEN).unwrap();
    assert_eq!(outcome, SaveOutcome::Saved);
    let text = std::fs::read_to_string(dir.path().join("accounts.json")).unwrap();
    assert!(!text.contains("future_field"));
    assert!(!text.contains("operator_note"));
    assert!(!text.contains(TOKEN));
}

#[test]
fn remove_deletes_both_halves_and_is_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);
    let second = AccountRecord {
        uuid_hex: "069a79f444e94726a5befca90e38aaf6".to_string(),
        name: "jeb_".to_string(),
        xuid: None,
    };
    store.save(&record(), TOKEN).unwrap();
    store
        .save(&second, "REFRESH-TOKEN-synthetic-value-0002")
        .unwrap();
    assert_eq!(store.list().len(), 2);

    store.remove(&record().uuid_hex).unwrap();
    assert_eq!(store.refresh_token(&record().uuid_hex).unwrap(), None);
    assert_eq!(store.record(&record().uuid_hex), None);
    assert_eq!(store.list(), vec![second.clone()]);

    let text = std::fs::read_to_string(dir.path().join("accounts.json")).unwrap();
    assert!(!text.contains(&record().uuid_hex));
    assert!(!text.contains(TOKEN));

    store.remove(&record().uuid_hex).unwrap();
    assert_eq!(store.list(), vec![second.clone()]);
}

#[test]
fn removing_an_absent_account_writes_no_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);

    store.remove("no-such-uuid").unwrap();
    assert!(
        !dir.path().join("accounts.json").exists(),
        "a no-op remove must not materialise a file"
    );
}

#[test]
fn saving_an_existing_uuid_replaces_both_halves() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = fresh_store(&dir);
    store.save(&record(), TOKEN).unwrap();

    let updated = AccountRecord {
        name: "NotchRenamed".to_string(),
        xuid: None,
        ..record()
    };
    let rotated = "REFRESH-TOKEN-synthetic-value-0003";
    let outcome = store.save(&updated, rotated).unwrap();
    assert_eq!(outcome, SaveOutcome::Saved);

    assert_eq!(store.list(), vec![updated.clone()]);
    assert_eq!(store.record(&updated.uuid_hex), Some(updated));
    assert_eq!(
        store.refresh_token(&record().uuid_hex).unwrap().as_deref(),
        Some(rotated)
    );

    let text = std::fs::read_to_string(dir.path().join("accounts.json")).unwrap();
    assert!(
        !text.contains(TOKEN),
        "the old secret must be gone from the file"
    );
    assert!(
        !text.contains(rotated),
        "the new secret must never reach the file"
    );
    assert!(text.contains("NotchRenamed"));
}
