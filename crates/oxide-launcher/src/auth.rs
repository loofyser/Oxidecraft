//! The `auth` command: the device-code sign-in, the account listing and the
//! sign-out, so `main.rs` stays a dispatcher.
//!
//! The store root follows the one precedence shared with the client —
//! `--data-dir` (when given) > `OXIDECRAFT_STORE` (when set and non-empty,
//! the client's `STORE_VAR`, `oxide-client/src/assets.rs:40`) > `<data
//! dir>/oxidecraft` — and no token value ever reaches a printed string: the
//! typed errors pass through `anyhow` untouched, and the flow prints only
//! the user-facing halves (the instructions, the user code, the
//! verification URI, the player name and uuid).

use std::path::PathBuf;
use std::time::Duration;

use oxide_assets::accounts::{AccountRecord, AccountStore, SaveOutcome};
use oxide_assets::auth::{Auth, AuthEndpoints, client_id_from_env};
use oxide_assets::http::{HttpClient, UreqClient};

/// The environment variable that names the store root when `--data-dir` is
/// absent: the client's own override (`oxide-client/src/assets.rs:40`). The
/// launcher cannot depend on `oxide-client` (no new crate edge this
/// milestone), so the name is repeated here and pinned by
/// `store_var_names_the_client_binding` — the two literals must stay
/// identical.
pub const STORE_VAR: &str = "OXIDECRAFT_STORE";

/// Resolves the store root with the one precedence shared with the client:
/// `--data-dir` (when given) > [`STORE_VAR`] (when set and non-empty) >
/// `<data dir>/oxidecraft` (the launcher's `default_data_dir`,
/// `super::default_data_dir`, which honours the variable itself).
pub fn resolve_store_root(data_dir: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match data_dir {
        Some(root) => Ok(root),
        None => super::default_data_dir(),
    }
}

/// The line `auth --status` prints when no account is signed in.
const NO_ACCOUNTS: &str = "no accounts are signed in";

/// Lists the signed-in accounts (name + uuid, never a secret) into `out`,
/// or the no-accounts line when the store is empty. Reads the record file
/// only — no keyring operation runs.
pub fn run_status(store: &AccountStore, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let accounts = store.list();
    if accounts.is_empty() {
        writeln!(out, "{NO_ACCOUNTS}")?;
    } else {
        for record in &accounts {
            writeln!(out, "{} {}", record.name, record.uuid_hex)?;
        }
    }
    Ok(())
}

/// Signs the account addressed by `target` (name or uuid) out of the store:
/// both halves are removed through [`AccountStore::remove`]. Addressing an
/// unknown account is clean, not an error — the message says so either way.
pub fn run_logout(
    store: &AccountStore,
    target: &str,
    out: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let hit = store
        .list()
        .into_iter()
        .find(|record| record.uuid_hex == target || record.name == target);
    match hit {
        Some(record) => {
            store.remove(&record.uuid_hex)?;
            writeln!(out, "signed out {}", record.name)?;
        }
        None => {
            writeln!(out, "no account matches '{target}'")?;
        }
    }
    Ok(())
}

/// Runs the device-code flow end to end: `begin_device_code`, the
/// print-and-poll loop (no browser is opened), `complete_sign_in`, and the
/// account save — then the signed-in name, uuid and outcome.
///
/// Only the user-facing halves ever print: the instructions, the user code,
/// the verification URI, the player name and uuid, and the save outcome. The
/// typed errors pass through `anyhow` untouched — and the chain never places
/// a token value into an error.
pub fn run_sign_in(
    http: std::sync::Arc<dyn HttpClient>,
    store: &AccountStore,
    client_id: &str,
    sleep: &mut dyn FnMut(Duration),
    out: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let auth = Auth::new(http, AuthEndpoints::default(), client_id.to_string());
    let code = auth.begin_device_code()?;
    writeln!(out, "{}", code.message)?;
    writeln!(out, "user code: {}", code.user_code)?;
    writeln!(out, "verify at: {}", code.verification_uri)?;
    let msa = auth.poll_token(&code, sleep)?;
    let signed = auth.complete_sign_in(msa)?;
    let record = AccountRecord {
        uuid_hex: signed.profile.uuid_hex.clone(),
        name: signed.profile.name.clone(),
        xuid: None,
    };
    let outcome = match signed.refreshed_msa_refresh_token.as_deref() {
        Some(refresh) => store.save(&record, refresh)?,
        // The scope always yields a refresh half; a token without one still
        // signs in for this process rather than failing the flow.
        None => SaveOutcome::SessionOnly {
            reason: "the sign-in yielded no refresh token".to_string(),
        },
    };
    writeln!(out, "signed in as {} ({})", record.name, record.uuid_hex)?;
    match outcome {
        SaveOutcome::Saved => writeln!(out, "account saved")?,
        SaveOutcome::SessionOnly { reason } => writeln!(out, "session only: {reason}")?,
    }
    Ok(())
}

/// Runs the `auth` command: `auth --status` lists, `auth --logout <name|uuid>`
/// removes, and a bare `auth` runs the device-code flow end to end.
pub fn run(data_dir: Option<PathBuf>, status: bool, logout: Option<String>) -> anyhow::Result<()> {
    let root = resolve_store_root(data_dir)?;
    if status {
        // Status reads without creating: a missing data directory is an
        // empty store, so the no-accounts line prints with no directory
        // created and no keyring operation. (The store's `open` creates the
        // directory eagerly; an existing directory opens as a no-op and the
        // listing itself never touches the secrets.)
        if !root.exists() {
            println!("{NO_ACCOUNTS}");
            return Ok(());
        }
        let store = AccountStore::open(root)?;
        run_status(&store, &mut std::io::stdout().lock())?;
        return Ok(());
    }
    if let Some(target) = logout {
        let store = AccountStore::open(root)?;
        run_logout(&store, &target, &mut std::io::stdout().lock())?;
        return Ok(());
    }
    let store = AccountStore::open(root)?;
    let http = std::sync::Arc::new(UreqClient::new());
    let client_id = client_id_from_env();
    let mut sleep = |wait: Duration| std::thread::sleep(wait);
    run_sign_in(
        http,
        &store,
        &client_id,
        &mut sleep,
        &mut std::io::stdout().lock(),
    )
}

#[cfg(test)]
mod tests {
    //! Steps 1–3: the CLI shapes; status/logout against a temp dir with the
    //! memory backend (plus the `OXIDECRAFT_STORE` precedence the launcher
    //! shares with the client); the full flow against a scripted stub
    //! `HttpClient`.

    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;
    use std::time::Duration;

    use oxide_assets::accounts::{AccountRecord, AccountStore, SaveOutcome, SecretStore};
    use oxide_assets::auth::AuthEndpoints;
    use oxide_assets::http::{HttpClient, HttpError};

    use super::{STORE_VAR, resolve_store_root, run_logout, run_sign_in, run_status};
    use crate::{Cli, Command};
    use clap::Parser;

    /// An in-memory [`SecretStore`]: the tier-1 shape with no daemon behind
    /// it, so no test touches the OS keyring.
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
        fn get(&self, key: &str) -> Result<Option<String>, oxide_assets::accounts::StoreError> {
            Ok(self.inner.lock().expect("the secrets").get(key).cloned())
        }

        fn set(&self, key: &str, value: &str) -> Result<(), oxide_assets::accounts::StoreError> {
            self.inner
                .lock()
                .expect("the secrets")
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), oxide_assets::accounts::StoreError> {
            self.inner.lock().expect("the secrets").remove(key);
            Ok(())
        }
    }

    /// A fresh store over a fresh temporary directory and a memory backend:
    /// no test touches the keyring or the real data directory.
    fn fresh_store(dir: &tempfile::TempDir) -> AccountStore {
        AccountStore::open_with(dir.path().to_path_buf(), Box::new(MemorySecrets::new()))
            .expect("a temp-dir store opens")
    }

    /// The non-secret half of the test account.
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
    fn a_bare_auth_parses_with_no_flags_set() {
        let cli = Cli::try_parse_from(["oxide-launcher", "auth"]).expect("a bare auth parses");
        match cli.command {
            Command::Auth { status, logout } => {
                assert!(!status, "status stays off");
                assert_eq!(logout, None, "no logout target");
            }
            _ => panic!("expected the auth command"),
        }
    }

    #[test]
    fn an_auth_status_parses_with_status_set() {
        let cli = Cli::try_parse_from(["oxide-launcher", "auth", "--status"])
            .expect("auth --status parses");
        match cli.command {
            Command::Auth { status, logout } => {
                assert!(status, "the flag is set");
                assert_eq!(logout, None, "no logout target");
            }
            _ => panic!("expected the auth command"),
        }
    }

    #[test]
    fn an_auth_logout_parses_with_its_target() {
        let cli = Cli::try_parse_from(["oxide-launcher", "auth", "--logout", "Notch"])
            .expect("auth --logout parses");
        match cli.command {
            Command::Auth { status, logout } => {
                assert!(!status, "status stays off");
                assert_eq!(logout.as_deref(), Some("Notch"), "the target rides along");
            }
            _ => panic!("expected the auth command"),
        }
    }

    #[test]
    fn an_unknown_auth_flag_is_refused() {
        let error = Cli::try_parse_from(["oxide-launcher", "auth", "--bogus"])
            .err()
            .expect("an unknown flag must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::UnknownArgument,
            "{error}"
        );
    }

    // --- Step 2: status and logout ------------------------------------------

    #[test]
    fn store_var_names_the_client_binding() {
        assert_eq!(
            STORE_VAR, "OXIDECRAFT_STORE",
            "the launcher reads the client's own variable \
             (oxide-client/src/assets.rs:40); the two literals must stay identical"
        );
    }

    #[test]
    fn the_store_root_precedence_matches_the_client() {
        // The precedence lives in the process environment, and swapping it in
        // the parent would race parallel tests (besides needing `unsafe`,
        // which the workspace forbids) — so each case runs in a child
        // process, the repo's established pattern (Task 6's
        // `client_id_override_rules`). The child re-enters this same test
        // with the probe case set and asserts the resolution directly.
        // Expected values are concrete paths: the client's
        // `default_store_root` (oxide-client/src/assets.rs:890) applies the
        // same order over the same variable, so these are the roots the
        // client resolves too.
        if let Ok(case) = std::env::var("OXIDECRAFT_STORE_CASE") {
            match case.as_str() {
                "env" => assert_eq!(
                    resolve_store_root(None).expect("the env root resolves"),
                    std::path::PathBuf::from("/tmp/oxide-launcher-probe-store"),
                    "the variable wins over the default"
                ),
                "empty" => assert_eq!(
                    resolve_store_root(None).expect("the default resolves"),
                    dirs::data_dir().expect("a data dir").join("oxidecraft"),
                    "an empty variable falls back, like the client's empty filter"
                ),
                "unset" => assert_eq!(
                    resolve_store_root(None).expect("the default resolves"),
                    dirs::data_dir().expect("a data dir").join("oxidecraft"),
                    "an unset variable falls back"
                ),
                "arg" => assert_eq!(
                    resolve_store_root(Some(std::path::PathBuf::from(
                        "/tmp/oxide-launcher-probe-arg"
                    )))
                    .expect("the argument resolves"),
                    std::path::PathBuf::from("/tmp/oxide-launcher-probe-arg"),
                    "the argument wins over the variable"
                ),
                other => panic!("unknown store-root probe case: {other}"),
            }
            return;
        }
        let exe = std::env::current_exe().expect("the test binary path");
        for case in ["env", "empty", "unset", "arg"] {
            let mut probe = std::process::Command::new(&exe);
            probe
                .arg("auth::tests::the_store_root_precedence_matches_the_client")
                .env("OXIDECRAFT_STORE_CASE", case);
            match case {
                "env" | "arg" => {
                    probe.env(STORE_VAR, "/tmp/oxide-launcher-probe-store");
                }
                // `env_remove` keeps the probe hermetic even when the runner
                // exports the variable.
                "unset" => {
                    probe.env_remove(STORE_VAR);
                }
                "empty" => {
                    probe.env(STORE_VAR, "");
                }
                _ => unreachable!("the loop lists every case"),
            }
            let status = probe.status().expect("the probe process runs");
            assert!(status.success(), "the {case} store-root rule holds");
        }
    }

    #[test]
    fn an_empty_store_reports_no_accounts() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let store = fresh_store(&dir);

        let mut out = Vec::new();
        run_status(&store, &mut out).expect("status reports");

        assert_eq!(
            String::from_utf8(out).expect("the output is UTF-8"),
            "no accounts are signed in\n"
        );
    }

    #[test]
    fn a_seeded_account_lists_by_name_and_uuid_without_its_secret() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let store = fresh_store(&dir);
        let record = record();
        assert_eq!(
            store.save(&record, TOKEN).expect("the seed saves"),
            SaveOutcome::Saved
        );

        let mut out = Vec::new();
        run_status(&store, &mut out).expect("status reports");
        let text = String::from_utf8(out).expect("the output is UTF-8");

        assert!(text.contains(&record.name), "the name lists: {text}");
        assert!(text.contains(&record.uuid_hex), "the uuid lists: {text}");
        assert!(
            !text.contains(TOKEN),
            "the secret never reaches the listing"
        );
    }

    #[test]
    fn a_logout_removes_and_a_second_logout_reports_cleanly() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let store = fresh_store(&dir);
        let record = record();
        store.save(&record, TOKEN).expect("the seed saves");

        let mut out = Vec::new();
        run_logout(&store, &record.name, &mut out).expect("the logout runs");
        let text = String::from_utf8(out).expect("the output is UTF-8");
        assert!(
            text.contains(&record.name),
            "the logout names the account: {text}"
        );
        assert!(store.list().is_empty(), "the record half is gone");
        assert_eq!(
            store
                .refresh_token(&record.uuid_hex)
                .expect("the backend reads"),
            None,
            "the secret half is gone"
        );

        let mut again = Vec::new();
        run_logout(&store, &record.name, &mut again).expect("a repeat logout is clean");
        let repeated = String::from_utf8(again).expect("the output is UTF-8");
        assert!(
            repeated.contains(&record.name),
            "the repeat names its target: {repeated}"
        );
        assert!(
            !repeated.contains(&record.uuid_hex),
            "no uuid leaks into the miss message"
        );
    }

    #[test]
    fn a_logout_by_uuid_removes_too() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let store = fresh_store(&dir);
        let record = record();
        store.save(&record, TOKEN).expect("the seed saves");

        let mut out = Vec::new();
        run_logout(&store, &record.uuid_hex, &mut out).expect("the logout runs");
        let text = String::from_utf8(out).expect("the output is UTF-8");
        assert!(
            text.contains(&record.name),
            "the logout names the account even when addressed by uuid: {text}"
        );
        assert!(store.list().is_empty(), "the record half is gone");
    }

    // --- Step 3: the full flow against the script ---------------------------

    /// One scripted exchange: a status plus the body delivered verbatim. A
    /// 2xx status answers `Ok`, anything else answers `HttpError::Status` —
    /// the translation the production client performs.
    struct Exchange {
        /// The scripted HTTP status.
        status: u16,
        /// The body delivered verbatim.
        body: Vec<u8>,
    }

    impl Exchange {
        /// A successful exchange delivering `body` verbatim.
        fn ok(body: &str) -> Self {
            Self {
                status: 200,
                body: body.as_bytes().to_vec(),
            }
        }

        /// A failed exchange delivering `body` verbatim.
        fn fail(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.as_bytes().to_vec(),
            }
        }
    }

    /// The scripted `HttpClient` double: answers every call from a FIFO
    /// script of canned exchanges. No sockets, no sleeps.
    struct Script {
        /// The remaining exchanges, in call order.
        exchanges: Mutex<VecDeque<Exchange>>,
    }

    impl Script {
        /// Builds the double preloaded with `exchanges`, in call order.
        fn with(exchanges: Vec<Exchange>) -> Self {
            Self {
                exchanges: Mutex::new(exchanges.into()),
            }
        }

        /// Pops the next scripted exchange for `url`, translating it the way
        /// the production client does.
        fn answer(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            let exchange = self
                .exchanges
                .lock()
                .expect("the script")
                .pop_front()
                .expect("the script holds an exchange for every request");
            if (200..300).contains(&exchange.status) {
                Ok(exchange.body)
            } else {
                Err(HttpError::Status {
                    url: url.to_string(),
                    code: exchange.status,
                    body: exchange.body,
                    correlation_id: None,
                })
            }
        }

        /// True once every scripted exchange has been consumed.
        fn is_exhausted(&self) -> bool {
            self.exchanges.lock().expect("the script").is_empty()
        }
    }

    impl HttpClient for Script {
        fn get(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
            unimplemented!("the auth chain never issues a bare GET")
        }

        fn post_json(&self, url: &str, _body: &[u8]) -> Result<Vec<u8>, HttpError> {
            self.answer(url)
        }

        fn post_form(&self, url: &str, _fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError> {
            self.answer(url)
        }

        fn get_with_bearer(&self, _url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
            self.answer(_url)
        }
    }

    /// The client id the flow test polls with: fixed so the run is exact.
    const FLOW_CLIENT_ID: &str = "TEST-CLIENT-ID-0000-000000000000";
    /// The profile uuid the scripted chain signs in as: 32 hex digits, no
    /// dashes, the exact form the profile endpoint returns.
    const FLOW_UUID_HEX: &str = "11111111222233334444555555555555";

    /// The whole scripted chain: device code, one pending poll, the token,
    /// Xbox Live, XSTS, the Minecraft login, the entitlements and the
    /// profile — every body the survey's verbatim shapes (§3.1–§3.6) with
    /// synthetic credential values.
    fn flow_script() -> Vec<Exchange> {
        vec![
            Exchange::ok(
                r#"{"user_code":"A1B2-C3D4","device_code":"SYN-DEVICE-CODE-001","verification_uri":"https://www.microsoft.com/link","expires_in":900,"interval":5,"message":"To sign in, use a web browser to open the page https://www.microsoft.com/link and enter the code A1B2-C3D4 to authenticate."}"#,
            ),
            Exchange::fail(
                400,
                r#"{"error":"authorization_pending","error_description":"AADSTS scripted wait.","error_codes":[70016]}"#,
            ),
            Exchange::ok(
                r#"{"token_type":"Bearer","scope":"XboxLive.signin offline_access","expires_in":"3599","access_token":"SYN-MS-ACCESS-001","refresh_token":"SYN-MS-REFRESH-001"}"#,
            ),
            Exchange::ok(
                r#"{"IssueInstant":"2026-10-14T00:00:00Z","NotAfter":"2026-10-15T00:00:00Z","Token":"SYN-XBL-TOKEN-001","DisplayClaims":{"xui":[{"uhs":"SYN-UHS-001"}]}}"#,
            ),
            Exchange::ok(
                r#"{"IssueInstant":"2026-10-14T00:00:00Z","NotAfter":"2026-10-15T00:00:00Z","Token":"SYN-XSTS-TOKEN-001","DisplayClaims":{"xui":[{"uhs":"SYN-UHS-001"}]}}"#,
            ),
            Exchange::ok(
                r#"{"username":"8b98c31a3f0a4b7c9d1e2f3a4b5c6d7e","roles":[],"access_token":"SYN-MC-ACCESS-001","token_type":"Bearer","expires_in":86400}"#,
            ),
            Exchange::ok(
                r#"{"items":[{"name":"product_minecraft","signature":"SYN-SIG-001"},{"name":"game_minecraft","signature":"SYN-SIG-002"}],"signature":"SYN-OUTER-SIG"}"#,
            ),
            Exchange::ok(
                r#"{"id":"11111111222233334444555555555555","name":"SynPlayer","properties":[],"profileActions":[]}"#,
            ),
        ]
    }

    #[test]
    fn the_full_flow_signs_in_saves_and_prints_no_secret() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let store = fresh_store(&dir);
        let script = std::sync::Arc::new(Script::with(flow_script()));
        let http: std::sync::Arc<dyn HttpClient> = script.clone();
        let sleeps = Mutex::new(Vec::new());
        let mut sleep = |wait: Duration| sleeps.lock().expect("the sleep log").push(wait);
        let mut out: Vec<u8> = Vec::new();

        run_sign_in(http, &store, FLOW_CLIENT_ID, &mut sleep, &mut out).expect("the flow signs in");

        let text = String::from_utf8(out).expect("the output is UTF-8");
        assert!(text.contains("A1B2-C3D4"), "the user code prints: {text}");
        assert!(
            text.contains("https://www.microsoft.com/link"),
            "the verification URI prints: {text}"
        );
        assert!(
            text.contains("SynPlayer"),
            "the signed-in name prints: {text}"
        );
        assert!(
            text.contains(FLOW_UUID_HEX),
            "the signed-in uuid prints: {text}"
        );
        assert!(
            text.contains("Saved") || text.contains("saved"),
            "the outcome prints: {text}"
        );
        for secret in [
            "SYN-DEVICE-CODE-001",
            "SYN-MS-ACCESS-001",
            "SYN-MS-REFRESH-001",
            "SYN-XBL-TOKEN-001",
            "SYN-XSTS-TOKEN-001",
            "SYN-MC-ACCESS-001",
        ] {
            assert!(
                !text.contains(secret),
                "no token value reaches any printed string: {text}"
            );
        }

        assert_eq!(
            store.list(),
            vec![AccountRecord {
                uuid_hex: FLOW_UUID_HEX.to_string(),
                name: "SynPlayer".to_string(),
                xuid: None,
            }],
            "the non-secret half lands in the store"
        );
        assert_eq!(
            store
                .refresh_token(FLOW_UUID_HEX)
                .expect("the backend reads")
                .as_deref(),
            Some("SYN-MS-REFRESH-001"),
            "the refresh half lands behind the secret backend"
        );
        assert!(
            !sleeps.lock().expect("the sleep log").is_empty(),
            "the pending poll waits through the injected sleeper, never for real"
        );
        assert!(script.is_exhausted(), "the whole chain is consumed");
        assert_eq!(
            AuthEndpoints::default().device_code,
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode",
            "the test double is addressed at the survey's endpoints"
        );
    }
}
