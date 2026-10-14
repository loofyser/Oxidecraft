//! The `--account` resolution: a stored account becomes the session's
//! signed-in identity.
//!
//! [`resolve`] finds the record by name or uuid, reads its refresh token
//! through the store, runs the silent sign-in chain, and builds the session
//! account around the freshly minted Minecraft access token with the
//! production [`JoinServer`][oxide_game::session::JoinServer]
//! implementation ([`HttpJoinServer`]). The offline default path never
//! reaches here: without `--account` no store is opened and no chain runs.
//!
//! Credential hygiene: the refresh half and the minted tokens never reach a
//! returned string. Every failure sentence below is pinned byte-exact by the
//! inline suite, which signs in with sentinel credentials and then asserts
//! the messages are clean.

use std::sync::Arc;

use oxide_assets::accounts::AccountStore;
use oxide_assets::auth::{Auth, JoinError};
use oxide_game::session::{JoinServer, SessionAccount};

/// The resolved startup identity: the profile name Login Start sends and
/// the session account the encrypted login joins with.
///
/// The `Debug` names the username and forwards the session's own redacted
/// rendering — never a token.
#[derive(Debug)]
pub struct ResolvedAccount {
    /// The signed-in profile's name — the session username when resolved.
    pub username: String,
    /// The session account: the profile uuid, the freshly minted Minecraft
    /// access token and the production join implementation.
    pub session: Arc<SessionAccount>,
}

/// Resolves `query` — a stored account's name or uuid — into the session
/// identity: finds the record (`uuid` first, then `name`), reads its refresh
/// token, runs [`Auth::silent_sign_in`], and builds the session account
/// around the freshly minted Minecraft access token plus [`HttpJoinServer`].
///
/// Failure sentences (pinned): no record matches → the sign-in instruction
/// naming the query; the secret backend is unavailable → the store's own
/// tier-3 sentence; the record carries no secret → the sign-in instruction
/// naming the account; a chain failure → the typed error text. No sentence
/// carries a token.
pub fn resolve(
    store: &AccountStore,
    query: &str,
    auth: Arc<Auth>,
) -> Result<ResolvedAccount, String> {
    let record = store
        .record(query)
        .or_else(|| store.list().into_iter().find(|known| known.name == query))
        .ok_or_else(|| {
            format!("no signed-in account matches `{query}` — run `oxide-launcher auth` first")
        })?;
    let secret = match store.refresh_token(&record.uuid_hex) {
        Ok(Some(secret)) => secret,
        Ok(None) => {
            return Err(format!(
                "no secret is stored for `{}` — run `oxide-launcher auth` first",
                record.name
            ));
        }
        Err(error) => return Err(error.to_string()),
    };
    let signed = auth
        .silent_sign_in(&secret)
        .map_err(|error| error.to_string())?;
    Ok(ResolvedAccount {
        username: signed.profile.name.clone(),
        session: Arc::new(SessionAccount {
            uuid_hex: signed.profile.uuid_hex.clone(),
            access_token: signed.mc_token.access_token.clone(),
            joiner: Arc::new(HttpJoinServer {
                auth: Arc::clone(&auth),
            }),
        }),
    })
}

/// The production [`JoinServer`]: posts the session join through
/// [`Auth::join_session`] and renders [`JoinError`]'s operator sentence —
/// the text the disconnected screen shows verbatim. The shapes follow the
/// source's `disconnect.loginFailedInfo.*` family
/// (`NetHandlerLoginClient.java:71-82`).
pub struct HttpJoinServer {
    /// The chain driver the join posts through.
    auth: Arc<Auth>,
}

impl HttpJoinServer {
    /// Wraps the chain driver as the session's join seam.
    pub fn new(auth: Arc<Auth>) -> Self {
        Self { auth }
    }
}

impl JoinServer for HttpJoinServer {
    /// Posts the join and renders the failure's operator sentence:
    /// `ForbiddenOperation` → the stale-login sentence,
    /// `InsufficientPrivileges` → the Xbox-settings sentence, `Banned` →
    /// the server's own message, `Shape`/`Http` → their text. Never a token.
    fn join(&self, uuid_hex: &str, access_token: &str, server_id: &str) -> Result<(), String> {
        self.auth
            .join_session(uuid_hex, access_token, server_id)
            .map_err(|error| match error {
                JoinError::ForbiddenOperation => {
                    "the session service refused the join (the account's login is no longer valid)"
                        .to_string()
                }
                JoinError::InsufficientPrivileges => {
                    "this account's Xbox settings do not allow multiplayer".to_string()
                }
                JoinError::Banned { message } => message,
                JoinError::Shape { .. } | JoinError::Http(_) => error.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    //! Resolution pins: a temp store plus a memory secret backend plus a
    //! scripted stub [`HttpClient`][oxide_assets::http::HttpClient] answering
    //! the silent chain (the auth_chain double's shape, replicated locally).
    //! Every credential below is synthetic; no test touches the OS keyring, a
    //! socket, or the real data directory.

    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use oxide_assets::accounts::{AccountRecord, SecretStore, StoreError};
    use oxide_assets::auth::AuthEndpoints;
    use oxide_assets::http::{HttpClient, HttpError};

    use super::*;

    /// The profile uuid every test signs in as: 32 hex digits, no dashes.
    const PROFILE_UUID_HEX: &str = "11111111222233334444555555555555";
    /// The profile name the scripted profile endpoint answers.
    const PROFILE_NAME: &str = "SynPlayer";
    /// The stored refresh half the memory backend answers.
    const STORED_REFRESH: &str = "SYN-MS-REFRESH-001";

    /// One scripted exchange: a status plus the body delivered verbatim. A
    /// 2xx status answers `Ok`, anything else answers `HttpError::Status`.
    struct Exchange {
        status: u16,
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

        /// A successful exchange with an explicit status (the session join
        /// answers 204 with an empty body).
        fn ok_with_status(status: u16, body: &[u8]) -> Self {
            Self {
                status,
                body: body.to_vec(),
            }
        }

        /// A failed exchange delivering `body` verbatim.
        fn fail(status: u16, body: &[u8]) -> Self {
            Self {
                status,
                body: body.to_vec(),
            }
        }
    }

    /// One posted form: the url plus its fields.
    type FormCall = (String, Vec<(String, String)>);

    /// The scripted `HttpClient` double: answers every call from a FIFO
    /// script of canned exchanges and records the JSON posts. No sockets.
    struct Script {
        exchanges: Mutex<VecDeque<Exchange>>,
        json_calls: Mutex<Vec<(String, Vec<u8>)>>,
        form_calls: Mutex<Vec<FormCall>>,
    }

    impl Script {
        /// Builds the double preloaded with `exchanges`, in call order.
        fn with(exchanges: Vec<Exchange>) -> Self {
            Self {
                exchanges: Mutex::new(exchanges.into()),
                json_calls: Mutex::new(Vec::new()),
                form_calls: Mutex::new(Vec::new()),
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

        /// Whether every scripted exchange was consumed.
        fn is_exhausted(&self) -> bool {
            self.exchanges.lock().expect("the script").is_empty()
        }
    }

    impl HttpClient for Script {
        fn get(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
            unimplemented!("the silent chain and the join never issue a bare GET")
        }

        fn post_json(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, HttpError> {
            self.json_calls
                .lock()
                .expect("the call log")
                .push((url.to_string(), body.to_vec()));
            self.answer(url)
        }

        fn post_form(&self, url: &str, fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError> {
            self.form_calls.lock().expect("the call log").push((
                url.to_string(),
                fields
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                    .collect(),
            ));
            self.answer(url)
        }

        fn get_with_bearer(&self, url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
            self.answer(url)
        }
    }

    /// An in-memory [`SecretStore`]: the tier-1 shape with no daemon behind
    /// it. The inner map rides an `Arc` so the test keeps a handle beside
    /// the store.
    #[derive(Clone)]
    struct MemorySecrets {
        inner: Arc<Mutex<HashMap<String, String>>>,
    }

    impl MemorySecrets {
        /// An empty backend.
        fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
            Ok(self.inner.lock().expect("the secrets").get(key).cloned())
        }

        fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
            self.inner
                .lock()
                .expect("the secrets")
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), StoreError> {
            self.inner.lock().expect("the secrets").remove(key);
            Ok(())
        }
    }

    /// A [`SecretStore`] shaped like a keyring with no daemon behind it:
    /// every operation reports the tier-3 shape.
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

    /// A fresh temporary directory under the system temp dir, removed on
    /// drop. Each case owns one, so parallel tests never share a path.
    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        /// Creates the directory, retrying with a fresh counter value when
        /// the path exists.
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            for _ in 0..100 {
                let count = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!(
                    "oxide-client-accounts-{tag}-{}-{count}",
                    std::process::id()
                ));
                if std::fs::create_dir(&path).is_ok() {
                    return Self { path };
                }
            }
            panic!("the temp dir could not be created");
        }

        /// The directory's path.
        fn path(&self) -> std::path::PathBuf {
            self.path.clone()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// A token-endpoint success body with synthetic halves.
    fn token_body(access: &str, refresh: &str) -> String {
        format!(
            r#"{{"token_type":"Bearer","scope":"XboxLive.signin offline_access","expires_in":"3599","access_token":"{access}","refresh_token":"{refresh}"}}"#
        )
    }

    /// An Xbox/XSTS success body with a synthetic token and user hash.
    fn xbox_body(token: &str, uhs: &str) -> String {
        format!(
            r#"{{"IssueInstant":"2026-10-14T00:00:00Z","NotAfter":"2026-10-15T00:00:00Z","Token":"{token}","DisplayClaims":{{"xui":[{{"uhs":"{uhs}"}}]}}}}"#
        )
    }

    /// A login_with_xbox success body minting `access`.
    fn login_body(access: &str) -> String {
        format!(
            r#"{{"username":"8b98c31a3f0a4b7c9d1e2f3a4b5c6d7e","roles":[],"access_token":"{access}","token_type":"Bearer","expires_in":86400}}"#
        )
    }

    /// An entitlements 200 carrying the two owning items.
    fn entitlements_body() -> String {
        r#"{"items":[{"name":"product_minecraft","signature":"SYN-SIG-001"},{"name":"game_minecraft","signature":"SYN-SIG-002"}],"signature":"SYN-OUTER-SIG"}"#.to_string()
    }

    /// A profile 200 for the test identity.
    fn profile_body() -> String {
        format!(
            r#"{{"id":"{PROFILE_UUID_HEX}","name":"{PROFILE_NAME}","properties":[],"profileActions":[]}}"#
        )
    }

    /// The six exchanges the silent chain consumes, minting `mc_access` as
    /// the fresh Minecraft token.
    fn silent_exchanges(mc_access: &str) -> Vec<Exchange> {
        vec![
            Exchange::ok(&token_body("SYN-MS-ACCESS-002", "SYN-MS-REFRESH-002")),
            Exchange::ok(&xbox_body("SYN-XBL-TOKEN-002", "SYN-UHS-002")),
            Exchange::ok(&xbox_body("SYN-XSTS-TOKEN-002", "SYN-UHS-002")),
            Exchange::ok(&login_body(mc_access)),
            Exchange::ok(&entitlements_body()),
            Exchange::ok(&profile_body()),
        ]
    }

    /// The stored test record.
    fn test_record() -> AccountRecord {
        AccountRecord {
            uuid_hex: PROFILE_UUID_HEX.to_string(),
            name: PROFILE_NAME.to_string(),
            xuid: Some("SYN-XUID-002".to_string()),
        }
    }

    /// A fresh store holding the test record plus its refresh half over a
    /// memory backend. Returns the temp dir (kept alive by the caller), the
    /// store, the backend handle and the scripted chain driver.
    fn stored_harness(
        exchanges: Vec<Exchange>,
    ) -> (TempDir, AccountStore, MemorySecrets, Arc<Auth>, Arc<Script>) {
        let dir = TempDir::new("stored");
        let secrets = MemorySecrets::new();
        let store = AccountStore::open_with(dir.path(), Box::new(secrets.clone()))
            .expect("the store opens");
        store
            .save(&test_record(), STORED_REFRESH)
            .expect("the record saves");
        let script = Arc::new(Script::with(exchanges));
        let auth = Arc::new(Auth::new(
            script.clone(),
            AuthEndpoints::default(),
            "TEST-CLIENT-ID-0000-000000000000".to_string(),
        ));
        (dir, store, secrets, auth, script)
    }

    /// Asserts no sentinel credential reaches `text`.
    fn assert_clean(text: &str) {
        for sentinel in [
            "SYN-MS-REFRESH",
            "SYN-MS-ACCESS",
            "SYN-XBL-TOKEN",
            "SYN-XSTS-TOKEN",
            "SYN-MC-ACCESS",
        ] {
            assert!(
                !text.contains(sentinel),
                "no token reaches the message: {text:?}"
            );
        }
    }

    #[test]
    fn resolve_by_name_signs_in_silently_and_builds_the_session() {
        let (_dir, store, _secrets, auth, script) =
            stored_harness(silent_exchanges("SYN-MC-ACCESS-002"));

        let resolved = resolve(&store, PROFILE_NAME, auth).expect("the name resolves");

        assert_eq!(resolved.username, PROFILE_NAME);
        assert_eq!(resolved.session.uuid_hex, PROFILE_UUID_HEX);
        assert_eq!(resolved.session.access_token, "SYN-MC-ACCESS-002");
        assert_clean(&resolved.username);
        assert_clean(&resolved.session.uuid_hex);
        let forms = script.form_calls.lock().expect("the call log");
        assert_eq!(forms.len(), 1, "silent refresh posts one grant");
        assert!(
            forms[0]
                .1
                .iter()
                .any(|(name, value)| name == "refresh_token" && value == STORED_REFRESH),
            "the grant presents the stored refresh half"
        );
        drop(forms);
        assert!(script.is_exhausted(), "the chain consumes the whole script");
    }

    #[test]
    fn the_resolved_debug_names_the_user_and_hides_the_tokens() {
        let (_dir, store, _secrets, auth, _script) =
            stored_harness(silent_exchanges("SYN-MC-ACCESS-020"));

        let resolved = resolve(&store, PROFILE_NAME, auth).expect("the name resolves");

        let rendered = format!("{:?}", resolved);
        assert!(
            rendered.contains(PROFILE_NAME),
            "the username renders: {rendered:?}"
        );
        assert_clean(&rendered);
    }

    #[test]
    fn resolve_by_uuid_finds_the_same_record() {
        let (_dir, store, _secrets, auth, script) =
            stored_harness(silent_exchanges("SYN-MC-ACCESS-003"));

        let resolved = resolve(&store, PROFILE_UUID_HEX, auth).expect("the uuid resolves");

        assert_eq!(resolved.username, PROFILE_NAME);
        assert_eq!(resolved.session.uuid_hex, PROFILE_UUID_HEX);
        assert_eq!(resolved.session.access_token, "SYN-MC-ACCESS-003");
        assert!(script.is_exhausted(), "the chain consumes the whole script");
    }

    #[test]
    fn a_missing_account_names_the_query_and_the_launcher_command() {
        let dir = TempDir::new("missing");
        let store = AccountStore::open_with(dir.path(), Box::new(MemorySecrets::new()))
            .expect("the store opens");
        let script = Arc::new(Script::with(vec![]));
        let auth = Arc::new(Auth::new(
            script,
            AuthEndpoints::default(),
            "TEST-CLIENT-ID-0000-000000000000".to_string(),
        ));

        let error = resolve(&store, "Nobody", auth).expect_err("nothing matches");

        assert_eq!(
            error,
            "no signed-in account matches `Nobody` — run `oxide-launcher auth` first"
        );
        assert_clean(&error);
    }

    #[test]
    fn a_chain_failure_reports_the_typed_text_and_no_token() {
        let (_dir, store, _secrets, auth, script) = stored_harness(vec![
            Exchange::ok(&token_body("SYN-MS-ACCESS-004", "SYN-MS-REFRESH-004")),
            Exchange::fail(401, b""),
        ]);

        let error = resolve(&store, PROFILE_NAME, auth).expect_err("the chain fails");

        assert_eq!(
            error,
            "Xbox Live request failed with HTTP 401 and an empty body (see the x-xblcorrelationid response header)"
        );
        assert_clean(&error);
        assert!(script.is_exhausted(), "the failing step ends the script");
    }

    #[test]
    fn no_keyring_reports_the_tier3_sentence() {
        let dir = TempDir::new("tier3");
        let memory = MemorySecrets::new();
        let seeding =
            AccountStore::open_with(dir.path(), Box::new(memory)).expect("the store opens");
        seeding
            .save(&test_record(), STORED_REFRESH)
            .expect("the record saves");
        let store = AccountStore::open_with(dir.path(), Box::new(UnavailableSecrets))
            .expect("the store opens");
        let script = Arc::new(Script::with(vec![]));
        let auth = Arc::new(Auth::new(
            script,
            AuthEndpoints::default(),
            "TEST-CLIENT-ID-0000-000000000000".to_string(),
        ));

        let error = resolve(&store, PROFILE_NAME, auth).expect_err("the keyring is out");

        assert_eq!(
            error,
            "secret store unavailable: the OS keyring is not reachable (test double)"
        );
        assert_clean(&error);
    }

    #[test]
    fn a_record_without_a_secret_points_at_sign_in() {
        let (_dir, store, secrets, auth, _script) = stored_harness(vec![]);
        secrets
            .delete(PROFILE_UUID_HEX)
            .expect("the secret deletes");

        let error = resolve(&store, PROFILE_NAME, auth).expect_err("no secret is stored");

        assert_eq!(
            error,
            "no secret is stored for `SynPlayer` — run `oxide-launcher auth` first"
        );
        assert_clean(&error);
    }

    /// A join harness: the driver over `exchanges`, sharing the script.
    fn join_harness(exchanges: Vec<Exchange>) -> (HttpJoinServer, Arc<Script>) {
        let script = Arc::new(Script::with(exchanges));
        let auth = Arc::new(Auth::new(
            script.clone(),
            AuthEndpoints::default(),
            "TEST-CLIENT-ID-0000-000000000000".to_string(),
        ));
        (HttpJoinServer::new(auth), script)
    }

    #[test]
    fn join_success_posts_the_verbatim_body() {
        let (joiner, script) = join_harness(vec![Exchange::ok_with_status(204, b"")]);

        joiner
            .join(PROFILE_UUID_HEX, "SYN-MC-ACCESS-010", "SYN-HASH-010")
            .expect("204 with an empty body is success");

        let calls = script.json_calls.lock().expect("the call log");
        assert_eq!(calls.len(), 1, "one join post");
        assert_eq!(calls[0].0, AuthEndpoints::default().session_join);
        assert_eq!(
            String::from_utf8(calls[0].1.clone()).expect("the request is UTF-8"),
            r#"{"accessToken":"SYN-MC-ACCESS-010","selectedProfile":"11111111222233334444555555555555","serverId":"SYN-HASH-010"}"#
        );
        assert!(script.is_exhausted());
    }

    #[test]
    fn forbidden_renders_the_stale_login_sentence() {
        let (joiner, script) = join_harness(vec![Exchange::fail(
            403,
            br#"{"error":"ForbiddenOperationException","path":"/session/minecraft/join"}"#,
        )]);

        let error = joiner
            .join(PROFILE_UUID_HEX, "SYN-MC-ACCESS-011", "SYN-HASH-011")
            .expect_err("the service refuses");

        assert_eq!(
            error,
            "the session service refused the join (the account's login is no longer valid)"
        );
        assert_clean(&error);
        assert!(script.is_exhausted());
    }

    #[test]
    fn disabled_multiplayer_renders_the_xbox_sentence() {
        let (joiner, script) = join_harness(vec![Exchange::fail(
            403,
            br#"{"error":"InsufficientPrivilegesException","path":"/session/minecraft/join"}"#,
        )]);

        let error = joiner
            .join(PROFILE_UUID_HEX, "SYN-MC-ACCESS-012", "SYN-HASH-012")
            .expect_err("multiplayer is disabled");

        assert_eq!(
            error,
            "this account's Xbox settings do not allow multiplayer"
        );
        assert_clean(&error);
        assert!(script.is_exhausted());
    }

    #[test]
    fn banned_carries_the_server_message() {
        let (joiner, script) = join_harness(vec![Exchange::fail(
            403,
            br#"{"error":"UserBannedException","errorMessage":"You are banned from multiplayer.","path":"/session/minecraft/join"}"#,
        )]);

        let error = joiner
            .join(PROFILE_UUID_HEX, "SYN-MC-ACCESS-013", "SYN-HASH-013")
            .expect_err("the account is banned");

        assert_eq!(error, "You are banned from multiplayer.");
        assert_clean(&error);
        assert!(script.is_exhausted());
    }

    #[test]
    fn an_unknown_refusal_reports_its_text() {
        let (joiner, script) = join_harness(vec![Exchange::fail(
            403,
            br#"{"error":"SomethingElse","path":"/session/minecraft/join"}"#,
        )]);

        let error = joiner
            .join(PROFILE_UUID_HEX, "SYN-MC-ACCESS-014", "SYN-HASH-014")
            .expect_err("the shape is unknown");

        assert_eq!(
            error,
            "the join answer had an unexpected shape: the join refusal carries an unknown error: SomethingElse"
        );
        assert_clean(&error);
        assert!(script.is_exhausted());
    }

    #[test]
    fn a_transport_failure_reports_its_text() {
        struct Failing;
        impl HttpClient for Failing {
            fn get(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
                unreachable!("no GET in the join")
            }

            fn post_json(&self, url: &str, _body: &[u8]) -> Result<Vec<u8>, HttpError> {
                Err(HttpError::Transport {
                    url: url.to_string(),
                    message: "the test transport is down".to_string(),
                })
            }

            fn post_form(
                &self,
                _url: &str,
                _fields: &[(&str, &str)],
            ) -> Result<Vec<u8>, HttpError> {
                unreachable!("no form post in the join")
            }

            fn get_with_bearer(&self, _url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
                unreachable!("no bearer GET in the join")
            }
        }
        let auth = Arc::new(Auth::new(
            Arc::new(Failing),
            AuthEndpoints::default(),
            "TEST-CLIENT-ID-0000-000000000000".to_string(),
        ));
        let joiner = HttpJoinServer::new(auth);

        let error = joiner
            .join(PROFILE_UUID_HEX, "SYN-MC-ACCESS-015", "SYN-HASH-015")
            .expect_err("the transport fails");

        assert!(
            error.contains("the test transport is down"),
            "the transport text rides through: {error:?}"
        );
        assert_clean(&error);
    }
}
