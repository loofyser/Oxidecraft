//! The device-code flow, the Xbox Live / XSTS chain and the Minecraft
//! sign-in stage against a scripted endpoint double: every response below
//! mirrors the survey's verbatim shapes (§3.1–§3.7) with synthetic credential
//! values, every request's exact bytes are asserted at the stub, and no test
//! touches a socket, the clock, or a real Microsoft endpoint — the poll's
//! sleeper is injected and its calls are recorded.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use oxide_assets::auth::{
    AUTH_CLIENT_ID, Auth, AuthEndpoints, AuthError, AuthStep, JoinError, client_id_from_env,
    uuid_with_dashes,
};
use oxide_assets::http::{HttpClient, HttpError};

/// The client id every test polls with: fixed so the byte pins are exact.
const CLIENT_ID: &str = "TEST-CLIENT-ID-0000-000000000000";
/// The scope string, verbatim from survey §3.1 (`offline_access` is what
/// yields the refresh token).
const SCOPE: &str = "XboxLive.signin offline_access";
/// The device-code grant, verbatim from Microsoft Learn via survey §3.1.
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// One scripted exchange: a status, the body delivered verbatim, and the
/// scripted `x-xblcorrelationid` value carried directly in the Status
/// field — exactly the translation Task 4's client performs, header read
/// included. A 2xx status answers `Ok`, anything else answers
/// `HttpError::Status`.
struct Exchange {
    status: u16,
    body: Vec<u8>,
    correlation_id: Option<String>,
}

impl Exchange {
    /// A successful exchange delivering `body` verbatim.
    fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.as_bytes().to_vec(),
            correlation_id: None,
        }
    }

    /// A successful exchange with an explicit status delivering `body`
    /// verbatim: the session join answers **204 No Content** with an empty
    /// body on success (survey §3.7).
    fn ok_with_status(status: u16, body: &[u8]) -> Self {
        Self {
            status,
            body: body.to_vec(),
            correlation_id: None,
        }
    }

    /// A failed exchange delivering `body` verbatim with no correlation id
    /// (the survey-verbatim XBL shape is 401 with an empty body).
    fn fail(status: u16, body: &[u8]) -> Self {
        Self {
            status,
            body: body.to_vec(),
            correlation_id: None,
        }
    }

    /// A failed exchange delivering `body` verbatim and carrying the
    /// scripted correlation id directly in the Status field, the way the
    /// production client forwards the real response header.
    fn fail_with_correlation(status: u16, body: &[u8], correlation_id: &str) -> Self {
        Self {
            status,
            body: body.to_vec(),
            correlation_id: Some(correlation_id.to_string()),
        }
    }
}

/// One recorded form post: the URL plus the fields in order.
type FormCall = (String, Vec<(String, String)>);

/// The scripted `HttpClient` double: answers every call from a FIFO script
/// of canned exchanges and records every request. No sockets, no sleeps.
struct Script {
    exchanges: Mutex<VecDeque<Exchange>>,
    json_calls: Mutex<Vec<(String, Vec<u8>)>>,
    form_calls: Mutex<Vec<FormCall>>,
    bearer_calls: Mutex<Vec<(String, String)>>,
}

impl Script {
    /// Builds the double preloaded with `exchanges`, in call order.
    fn with(exchanges: Vec<Exchange>) -> Self {
        Self {
            exchanges: Mutex::new(exchanges.into()),
            json_calls: Mutex::new(Vec::new()),
            form_calls: Mutex::new(Vec::new()),
            bearer_calls: Mutex::new(Vec::new()),
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
                correlation_id: exchange.correlation_id,
            })
        }
    }

    /// The recorded JSON posts: `(url, raw body bytes)`.
    fn json_calls(&self) -> Vec<(String, Vec<u8>)> {
        self.json_calls.lock().expect("the call log").clone()
    }

    /// The recorded form posts: `(url, fields in order)`.
    fn form_calls(&self) -> Vec<FormCall> {
        self.form_calls.lock().expect("the call log").clone()
    }

    /// The recorded bearer GETs: `(url, bearer token)`.
    fn bearer_calls(&self) -> Vec<(String, String)> {
        self.bearer_calls.lock().expect("the call log").clone()
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

    fn get_with_bearer(&self, url: &str, bearer: &str) -> Result<Vec<u8>, HttpError> {
        self.bearer_calls
            .lock()
            .expect("the call log")
            .push((url.to_string(), bearer.to_string()));
        self.answer(url)
    }
}

/// Builds the chain under test over `exchanges`, sharing the script so the
/// test can assert on the recorded requests.
fn harness(exchanges: Vec<Exchange>) -> (Auth, std::sync::Arc<Script>) {
    let script = std::sync::Arc::new(Script::with(exchanges));
    let auth = Auth::new(
        script.clone(),
        AuthEndpoints::default(),
        CLIENT_ID.to_string(),
    );
    (auth, script)
}

/// Builds the chain the way production does: the client id comes from the
/// `OXIDECRAFT_AUTH_CLIENT_ID` override resolver, and the returned value is
/// what the tests assert on the wire — so the constant's value never affects
/// a test (the gate exports the override; the resolver rules themselves are
/// pinned by `client_id_override_rules` below).
fn harness_with_env(exchanges: Vec<Exchange>) -> (Auth, std::sync::Arc<Script>, String) {
    let client_id = client_id_from_env();
    assert!(
        !client_id.is_empty(),
        "the resolver always yields a client id"
    );
    let script = std::sync::Arc::new(Script::with(exchanges));
    let auth = Auth::new(script.clone(), AuthEndpoints::default(), client_id.clone());
    (auth, script, client_id)
}

/// The device-code success body: Microsoft Learn's field set verbatim via
/// survey §3.1, with synthetic values.
fn device_code_body() -> String {
    r#"{"user_code":"A1B2-C3D4","device_code":"SYN-DEVICE-CODE-001","verification_uri":"https://www.microsoft.com/link","expires_in":900,"interval":5,"message":"To sign in, use a web browser to open the page https://www.microsoft.com/link and enter the code A1B2-C3D4 to authenticate."}"#.to_string()
}

/// A token success body: `access_token` / `refresh_token` / `expires_in`,
/// the shape the Entra token endpoint returns.
fn token_body(access: &str, refresh: &str) -> String {
    format!(
        r#"{{"token_type":"Bearer","scope":"XboxLive.signin offline_access","expires_in":"3599","access_token":"{access}","refresh_token":"{refresh}"}}"#
    )
}

/// A token-endpoint error body: the `error` code the poll keys on.
fn token_error_body(error: &str) -> String {
    format!(
        r#"{{"error":"{error}","error_description":"AADSTS scripted description for {error}.","error_codes":[70016]}}"#
    )
}

/// An Xbox success body: `Token` plus `DisplayClaims.xui[0].uhs`, verbatim
/// field names from survey §3.2/§3.3, synthetic values.
fn xbox_body(token: &str, uhs: &str) -> String {
    format!(
        r#"{{"IssueInstant":"2026-10-14T00:00:00Z","NotAfter":"2026-10-15T00:00:00Z","Token":"{token}","DisplayClaims":{{"xui":[{{"uhs":"{uhs}"}}]}}}}"#
    )
}

/// An XSTS failure body: the `XErr` table row shape from survey §3.3.
fn xsts_error_body(xerr: u32, message: &str, redirect: Option<&str>) -> String {
    let redirect = redirect.map_or_else(|| "null".to_string(), |url| format!(r#""{url}""#));
    format!(r#"{{"Identity":"0","XErr":{xerr},"Message":"{message}","Redirect":{redirect}}}"#)
}

/// A sleeper that records every requested wait instead of sleeping.
fn recorder(sleeps: &Mutex<Vec<Duration>>) -> impl FnMut(Duration) + '_ {
    |wait| sleeps.lock().expect("the sleep log").push(wait)
}

#[test]
fn device_code_posts_the_scope_form_and_parses_every_field() {
    let (auth, script) = harness(vec![Exchange::ok(&device_code_body())]);

    let code = auth.begin_device_code().expect("the device-code request");

    assert_eq!(code.device_code, "SYN-DEVICE-CODE-001");
    assert_eq!(code.user_code, "A1B2-C3D4");
    assert_eq!(code.verification_uri, "https://www.microsoft.com/link");
    assert_eq!(code.expires_in, 900);
    assert_eq!(code.interval, 5);
    assert!(
        code.message.contains("A1B2-C3D4"),
        "the user instructions ride along"
    );

    let calls = script.form_calls();
    assert_eq!(calls.len(), 1, "one device-code request");
    assert_eq!(calls[0].0, AuthEndpoints::default().device_code);
    assert_eq!(
        calls[0].1,
        vec![
            ("client_id".to_string(), CLIENT_ID.to_string()),
            ("scope".to_string(), SCOPE.to_string()),
        ],
        "client_id then the verbatim scope string (offline_access yields the refresh token)"
    );
    assert!(script.is_exhausted());
}

#[test]
fn token_poll_posts_the_verbatim_grant_and_parses_the_tokens() {
    let (auth, script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::ok(&token_body("SYN-MS-ACCESS-001", "SYN-MS-REFRESH-001")),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let token = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect("the first poll already succeeds");

    assert_eq!(token.access_token, "SYN-MS-ACCESS-001");
    assert_eq!(token.refresh_token.as_deref(), Some("SYN-MS-REFRESH-001"));
    assert_eq!(token.expires_in, 3599);
    assert!(
        sleeps.lock().expect("the sleep log").is_empty(),
        "no wait precedes success"
    );

    let calls = script.form_calls();
    assert_eq!(calls.len(), 2, "device-code then one poll");
    assert_eq!(calls[1].0, AuthEndpoints::default().token);
    assert_eq!(
        calls[1].1,
        vec![
            ("grant_type".to_string(), DEVICE_GRANT.to_string()),
            ("client_id".to_string(), CLIENT_ID.to_string()),
            ("device_code".to_string(), "SYN-DEVICE-CODE-001".to_string()),
        ],
        "the verbatim device-code grant, field for field"
    );
    assert!(script.is_exhausted());
}

#[test]
fn refresh_posts_the_refresh_grant_with_no_xbox_state() {
    let (auth, script) = harness(vec![Exchange::ok(&token_body(
        "SYN-MS-ACCESS-002",
        "SYN-MS-REFRESH-002",
    ))]);

    let token = auth
        .refresh_msa("SYN-MS-REFRESH-001")
        .expect("the refresh succeeds");

    assert_eq!(token.access_token, "SYN-MS-ACCESS-002");
    assert_eq!(token.refresh_token.as_deref(), Some("SYN-MS-REFRESH-002"));

    let calls = script.form_calls();
    assert_eq!(
        calls.len(),
        1,
        "refresh needs no Xbox/Minecraft state — one call"
    );
    assert_eq!(calls[0].0, AuthEndpoints::default().token);
    assert_eq!(
        calls[0].1,
        vec![
            ("grant_type".to_string(), "refresh_token".to_string()),
            (
                "refresh_token".to_string(),
                "SYN-MS-REFRESH-001".to_string()
            ),
            ("client_id".to_string(), CLIENT_ID.to_string()),
        ],
        "the verbatim refresh grant"
    );
    assert!(script.is_exhausted());
}

#[test]
fn refresh_rejects_a_dead_grant_as_a_device_flow_error() {
    let (auth, _script) = harness(vec![Exchange::fail(
        400,
        token_error_body("invalid_grant").as_bytes(),
    )]);

    let error = auth
        .refresh_msa("SYN-MS-REFRESH-DEAD")
        .expect_err("the grant is dead");

    assert!(
        matches!(error, AuthError::DeviceFlow { ref error } if error == "invalid_grant"),
        "a dead grant is a flow error, not a status: {error}"
    );
}

#[test]
fn xbox_authenticate_sends_the_verbatim_body_with_the_d_prefix() {
    let (auth, script) = harness(vec![Exchange::ok(&xbox_body(
        "SYN-XBL-TOKEN-001",
        "SYN-UHS-001",
    ))]);

    let xbl = auth
        .authenticate_xbox("SYN-MS-ACCESS-001")
        .expect("Xbox Live authenticates");

    assert_eq!(xbl.token, "SYN-XBL-TOKEN-001");
    assert_eq!(xbl.user_hash, "SYN-UHS-001");

    let calls = script.json_calls();
    assert_eq!(calls.len(), 1, "one Xbox Live request");
    assert_eq!(calls[0].0, AuthEndpoints::default().xbl_authenticate);
    assert_eq!(
        String::from_utf8(calls[0].1.clone()).expect("the request is UTF-8"),
        r#"{"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":"d=SYN-MS-ACCESS-001"},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"}"#,
        "the §3.2 body byte for byte, key order included, with the mandatory d= prefix"
    );
    assert!(script.is_exhausted());
}

#[test]
fn xsts_authorize_sends_the_verbatim_body_with_the_rp_scheme() {
    let (auth, script) = harness(vec![
        Exchange::ok(&xbox_body("SYN-XBL-TOKEN-001", "SYN-UHS-001")),
        Exchange::ok(&xbox_body("SYN-XSTS-TOKEN-001", "SYN-UHS-001")),
    ]);
    let xbl = auth
        .authenticate_xbox("SYN-MS-ACCESS-001")
        .expect("Xbox Live authenticates");

    let xsts = auth.authorize_xsts(&xbl).expect("XSTS authorizes");

    assert_eq!(xsts.token, "SYN-XSTS-TOKEN-001");
    assert_eq!(xsts.user_hash, "SYN-UHS-001", "XSTS repeats the same uhs");

    let calls = script.json_calls();
    assert_eq!(calls.len(), 2, "Xbox Live then XSTS");
    assert_eq!(calls[1].0, AuthEndpoints::default().xsts_authorize);
    assert_eq!(
        String::from_utf8(calls[1].1.clone()).expect("the request is UTF-8"),
        r#"{"Properties":{"SandboxId":"RETAIL","UserTokens":["SYN-XBL-TOKEN-001"]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"}"#,
        "the §3.3 body byte for byte — the rp:// scheme is the exact string"
    );
    assert!(script.is_exhausted());
}

#[test]
fn poll_waits_the_advertised_interval_between_attempts() {
    let (auth, script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::fail(400, token_error_body("authorization_pending").as_bytes()),
        Exchange::fail(400, token_error_body("authorization_pending").as_bytes()),
        Exchange::ok(&token_body("SYN-MS-ACCESS-003", "SYN-MS-REFRESH-003")),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let token = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect("the third poll succeeds");

    assert_eq!(token.access_token, "SYN-MS-ACCESS-003");
    assert_eq!(
        *sleeps.lock().expect("the sleep log"),
        vec![Duration::from_secs(5), Duration::from_secs(5)],
        "one interval wait per pending answer, no wait after success"
    );
    assert!(script.form_calls().len() == 4 && script.is_exhausted());
}

#[test]
fn slow_down_increases_the_interval_by_five_seconds_each_time() {
    // Derived rule (RFC 8628 §3.5, which the Microsoft device-code polling
    // contract follows): a slow_down answer increases the polling interval
    // by 5 seconds for that wait and every later one — cumulative.
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::fail(400, token_error_body("slow_down").as_bytes()),
        Exchange::fail(400, token_error_body("slow_down").as_bytes()),
        Exchange::ok(&token_body("SYN-MS-ACCESS-004", "SYN-MS-REFRESH-004")),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    auth.poll_token(&code, &mut recorder(&sleeps))
        .expect("the third poll succeeds");

    assert_eq!(
        *sleeps.lock().expect("the sleep log"),
        vec![Duration::from_secs(10), Duration::from_secs(15)],
        "each slow_down adds five seconds, cumulatively"
    );
}

#[test]
fn poll_mixes_pending_and_slow_down_in_script_order() {
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::fail(400, token_error_body("authorization_pending").as_bytes()),
        Exchange::fail(400, token_error_body("slow_down").as_bytes()),
        Exchange::ok(&token_body("SYN-MS-ACCESS-005", "SYN-MS-REFRESH-005")),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    auth.poll_token(&code, &mut recorder(&sleeps))
        .expect("the third poll succeeds");

    assert_eq!(
        *sleeps.lock().expect("the sleep log"),
        vec![Duration::from_secs(5), Duration::from_secs(10)],
        "pending waits the interval, slow_down waits the bumped one"
    );
}

#[test]
fn expired_token_is_a_typed_device_flow_error() {
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::fail(400, token_error_body("expired_token").as_bytes()),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let error = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect_err("the code has expired");

    assert!(
        matches!(error, AuthError::DeviceFlow { ref error } if error == "expired_token"),
        "expired_token is terminal and typed: {error}"
    );
    assert!(
        sleeps.lock().expect("the sleep log").is_empty(),
        "no wait follows expiry"
    );
}

#[test]
fn access_denied_is_a_typed_device_flow_error() {
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::fail(400, token_error_body("access_denied").as_bytes()),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let error = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect_err("the user declined");

    assert!(
        matches!(error, AuthError::DeviceFlow { ref error } if error == "access_denied"),
        "a declined consent is terminal and typed: {error}"
    );
}

#[test]
fn the_fifteen_minute_window_is_a_typed_error_without_a_server_say_so() {
    // Survey §3.1: the user has 15 minutes. The poll tracks the injected
    // waits against the device code's expires_in, so a silent server that
    // keeps answering pending still terminates: expires_in 10 with an
    // interval of 5 allows exactly two waits.
    let pending = token_error_body("authorization_pending");
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body().replace("\"expires_in\":900", "\"expires_in\":10")),
        Exchange::fail(400, pending.as_bytes()),
        Exchange::fail(400, pending.as_bytes()),
        Exchange::fail(400, pending.as_bytes()),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let error = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect_err("the window closes");

    assert!(
        matches!(error, AuthError::DeviceFlow { ref error } if error == "expired_token"),
        "the closed window reports as expiry: {error}"
    );
    assert_eq!(
        *sleeps.lock().expect("the sleep log"),
        vec![Duration::from_secs(5), Duration::from_secs(5)],
        "two waits fit in the window; the third would overrun it"
    );
}

#[test]
fn a_non_json_poll_body_is_a_shape_error_not_a_panic() {
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::fail(400, b"this is not JSON at all"),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let error = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect_err("a non-JSON body cannot poll");

    assert!(
        matches!(
            error,
            AuthError::Shape {
                step: AuthStep::Token,
                ..
            }
        ),
        "garbage answers are typed, never panics: {error}"
    );
}

#[test]
fn a_numeric_expires_in_parses_like_the_string_form() {
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::ok(
            &token_body("SYN-MS-ACCESS-006", "SYN-MS-REFRESH-006")
                .replace("\"expires_in\":\"3599\"", "\"expires_in\":3599"),
        ),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());

    let token = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect("both expires_in shapes parse");

    assert_eq!(token.expires_in, 3599);
}

#[test]
fn xbl_401_with_an_empty_body_is_a_status_error() {
    // Survey §3.2, verbatim behaviour: a bad RpsTicket answers 401 with
    // content-length 0 — XBL errors carry no body.
    let (auth, _script) = harness(vec![Exchange::fail(401, b"")]);

    let error = auth
        .authenticate_xbox("SYN-MS-ACCESS-BAD")
        .expect_err("the ticket is bad");

    assert!(
        matches!(
            error,
            AuthError::Status { step: AuthStep::XboxLive, code: 401, ref body, .. } if body.is_empty()
        ),
        "an empty 401 is a status error carrying the code: {error}"
    );
}

#[test]
fn xbl_error_message_reports_the_status_and_the_correlation_id() {
    // The scripted exchange stands in for the full HTTP exchange: the
    // scripted x-xblcorrelationid value rides directly in the Status field,
    // the way the production client forwards the real response header.
    let (auth, _script) = harness(vec![Exchange::fail_with_correlation(
        401,
        b"",
        "3f2504e0-0000-0000-0000-000000000000",
    )]);

    let error = auth
        .authenticate_xbox("SYN-MS-ACCESS-BAD")
        .expect_err("the ticket is bad");
    let message = error.to_string();

    assert!(
        message.contains("401"),
        "the status rides in the message: {message}"
    );
    assert!(
        message.contains("3f2504e0-0000-0000-0000-000000000000"),
        "the correlation id rides in the message: {message}"
    );
}

#[test]
fn xbl_empty_body_message_names_the_correlation_header() {
    let (auth, _script) = harness(vec![Exchange::fail(401, b"")]);

    let message = auth
        .authenticate_xbox("SYN-MS-ACCESS-BAD")
        .expect_err("the ticket is bad")
        .to_string();

    assert!(
        message.contains("401"),
        "the status rides in the message: {message}"
    );
    assert!(
        message.contains("x-xblcorrelationid"),
        "the empty-body note points at the header: {message}"
    );
}

/// One XErr row: the scripted failure body plus the message pins.
fn xerr_case(xerr: u32, redirect: Option<&str>, pins: &[&str]) {
    let (auth, _script) = harness(vec![Exchange::fail(
        400,
        xsts_error_body(xerr, "", redirect).as_bytes(),
    )]);
    let xbl = oxide_assets::auth::XboxToken {
        token: "SYN-XBL-TOKEN-009".to_string(),
        user_hash: "SYN-UHS-009".to_string(),
    };

    let error = auth.authorize_xsts(&xbl).expect_err("XSTS refuses");
    let AuthError::Xsts {
        xerr: got,
        message,
        redirect: got_redirect,
    } = error
    else {
        panic!("an XErr body is an Xsts error, not {error}");
    };
    assert_eq!(got, xerr, "the code rides along");
    for pin in pins {
        assert!(
            message.contains(pin),
            "XErr {xerr} message pins {pin:?}: {message}"
        );
    }
    assert_eq!(
        got_redirect.as_deref(),
        redirect,
        "the redirect rides along"
    );
}

#[test]
fn xsts_banned_account_names_the_ban() {
    xerr_case(2148916227, None, &["banned"]);
}

#[test]
fn xsts_missing_xbox_account_names_signup() {
    xerr_case(2148916233, None, &["Xbox account"]);
}

#[test]
fn xsts_banned_region_names_the_region() {
    xerr_case(2148916235, None, &["country"]);
}

#[test]
fn xsts_korean_adult_verification_names_korea() {
    xerr_case(2148916236, None, &["South Korea"]);
    xerr_case(2148916237, None, &["South Korea"]);
}

#[test]
fn xsts_child_account_names_the_family_gate() {
    xerr_case(
        2148916238,
        Some("https://start.ui.xboxlive.com/AddChildToFamily"),
        &["child", "Family", "custom Microsoft Azure application"],
    );
}

#[test]
fn xsts_rare_failure_names_its_rarity() {
    xerr_case(2148916262, None, &["rarely", "no additional information"]);
}

#[test]
fn xsts_unknown_code_carries_the_code() {
    xerr_case(9_999_999, None, &["9999999"]);
}

#[test]
fn xsts_empty_400_is_a_status_error_not_a_panic() {
    // The live probe in survey §3.3 answers 400 with an empty body for a
    // bad token — with no XErr document there is no Xsts error to build.
    let (auth, _script) = harness(vec![Exchange::fail(400, b"")]);
    let xbl = oxide_assets::auth::XboxToken {
        token: "SYN-XBL-TOKEN-BAD".to_string(),
        user_hash: "SYN-UHS-BAD".to_string(),
    };

    let error = auth.authorize_xsts(&xbl).expect_err("XSTS refuses");

    assert!(
        matches!(
            error,
            AuthError::Status { step: AuthStep::Xsts, code: 400, ref body, .. } if body.is_empty()
        ),
        "an empty 400 is a status error, never a panic: {error}"
    );
}

#[test]
fn unknown_extra_fields_are_tolerated_in_every_success_body() {
    // Survey §3.6's profileActions lesson applied here: the services add
    // fields over time, and the parser must ignore what it does not know.
    let extra_device = device_code_body().replace(
        "\"interval\":5",
        "\"interval\":5,\"future_field\":{\"nested\":true}",
    );
    let extra_token = token_body("SYN-MS-ACCESS-010", "SYN-MS-REFRESH-010")
        .replace("\"expires_in\"", "\"profileActions\":[],\"expires_in\"");
    let extra_xbox = xbox_body("SYN-XBL-TOKEN-010", "SYN-UHS-010").replace(
        "\"DisplayClaims\"",
        "\"profileActions\":[],\"DisplayClaims\"",
    );
    let (auth, _script) = harness(vec![
        Exchange::ok(&extra_device),
        Exchange::ok(&extra_token),
        Exchange::ok(&extra_xbox.clone()),
        Exchange::ok(&extra_xbox),
    ]);
    let sleeps = Mutex::new(Vec::new());

    let code = auth.begin_device_code().expect("extra fields parse");
    assert_eq!(code.interval, 5);
    let token = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect("extra fields parse");
    assert_eq!(token.expires_in, 3599);
    let xbl = auth
        .authenticate_xbox(&token.access_token)
        .expect("extra fields parse");
    assert_eq!(xbl.user_hash, "SYN-UHS-010");
    let xsts = auth.authorize_xsts(&xbl).expect("extra fields parse");
    assert_eq!(xsts.token, "SYN-XBL-TOKEN-010");
}

#[test]
fn tokens_in_flight_never_reach_an_error_display() {
    // The chain mints two sentinel credentials, then fails at XSTS: the
    // resulting message must name the failure without echoing either.
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::ok(&token_body("LEAKCHECK-MSA-9z9z", "LEAKCHECK-REFRESH-9z9z")),
        Exchange::ok(&xbox_body("LEAKCHECK-XBL-9z9z", "SYN-UHS-LEAK")),
        Exchange::fail(400, xsts_error_body(2148916227, "", None).as_bytes()),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let sleeps = Mutex::new(Vec::new());
    let token = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect("the poll succeeds");
    let xbl = auth
        .authenticate_xbox(&token.access_token)
        .expect("Xbox Live authenticates");

    let message = auth
        .authorize_xsts(&xbl)
        .expect_err("XSTS refuses")
        .to_string();

    for secret in [
        "LEAKCHECK-MSA-9z9z",
        "LEAKCHECK-REFRESH-9z9z",
        "LEAKCHECK-XBL-9z9z",
    ] {
        assert!(
            !message.contains(secret),
            "no credential leaks into the message: {message}"
        );
    }

    // And a failed Xbox step after a good poll leaks nothing either.
    let (auth, _script) = harness(vec![
        Exchange::ok(&device_code_body()),
        Exchange::ok(&token_body("LEAKCHECK-MSA-9z9z", "LEAKCHECK-REFRESH-9z9z")),
        Exchange::fail(401, b""),
    ]);
    let code = auth.begin_device_code().expect("the device-code request");
    let token = auth
        .poll_token(&code, &mut recorder(&sleeps))
        .expect("the poll succeeds");
    let message = auth
        .authenticate_xbox(&token.access_token)
        .expect_err("the ticket is bad")
        .to_string();
    for secret in ["LEAKCHECK-MSA-9z9z", "LEAKCHECK-REFRESH-9z9z"] {
        assert!(
            !message.contains(secret),
            "no credential leaks into the message: {message}"
        );
    }
}

#[test]
fn a_huge_status_body_is_capped() {
    let big = vec![b'x'; 200 * 1024];
    let (auth, _script) = harness(vec![Exchange::fail(401, &big)]);

    let error = auth
        .authenticate_xbox("SYN-MS-ACCESS-001")
        .expect_err("the ticket is bad");

    let AuthError::Status { body, .. } = error else {
        panic!("a 401 is a Status error");
    };
    assert!(
        body.len() <= oxide_assets::http::MAX_ERROR_BODY_BYTES,
        "the Status body is capped at the HTTP error-body cap"
    );
}

// --- The stage-two corpus (survey §3.4–§3.7) --------------------------------

/// The profile uuid every stage-two test signs in as: 32 hex digits, no
/// dashes, the exact form the profile endpoint returns (§3.6).
const PROFILE_UUID_HEX: &str = "11111111222233334444555555555555";

/// A login_with_xbox success body: the survey §3.4 shape verbatim, with
/// synthetic values. `expires_in` 86400 is the 24-hour Minecraft token.
/// Note `username` is not the uuid (§3.4) — the profile fetch is the
/// identity source.
fn login_body_with(access: &str) -> String {
    format!(
        r#"{{"username":"8b98c31a3f0a4b7c9d1e2f3a4b5c6d7e","roles":[],"access_token":"{access}","token_type":"Bearer","expires_in":86400}}"#
    )
}

/// The default login body.
fn login_body() -> String {
    login_body_with("SYN-MC-ACCESS-001")
}

/// An entitlements 200: the two owning items from survey §3.5
/// (`product_minecraft`, `game_minecraft`).
fn entitlements_body() -> String {
    r#"{"items":[{"name":"product_minecraft","signature":"SYN-SIG-001"},{"name":"game_minecraft","signature":"SYN-SIG-002"}],"signature":"SYN-OUTER-SIG"}"#.to_string()
}

/// The empty-`items` case: a working Microsoft account that owns nothing —
/// survey §3.4's crux ("Everything until here works with a normal Microsoft
/// account!").
fn entitlements_empty_body() -> String {
    r#"{"items":[],"signature":"SYN-OUTER-SIG"}"#.to_string()
}

/// A profile 200: the §3.6 shape (`id` without dashes, `name`).
fn profile_body() -> String {
    r#"{"id":"11111111222233334444555555555555","name":"SynPlayer","properties":[],"profileActions":[]}"#
        .to_string()
}

/// A join 403: the survey-verbatim `ForbiddenOperationException` shape.
fn join_forbidden_body() -> String {
    r#"{"error":"ForbiddenOperationException","path":"/session/minecraft/join"}"#.to_string()
}

/// A join 403 for a disabled-multiplayer account (survey §3.7's
/// `// Xbox multiplayer disabled` annotation).
fn join_privileges_body() -> String {
    r#"{"error":"InsufficientPrivilegesException","path":"/session/minecraft/join"}"#.to_string()
}

/// A join 403 for a multiplayer-banned account, carrying its `errorMessage`.
fn join_banned_body() -> String {
    r#"{"error":"UserBannedException","errorMessage":"You are banned from multiplayer.","path":"/session/minecraft/join"}"#.to_string()
}

#[test]
fn login_with_xbox_posts_the_exact_concat_and_parses_86400() {
    let (auth, script, _) = harness_with_env(vec![Exchange::ok(&login_body())]);

    let token = auth
        .login_with_xbox("SYN-UHS-001", "SYN-XSTS-TOKEN-001")
        .expect("the login succeeds");

    assert_eq!(token.access_token, "SYN-MC-ACCESS-001");
    assert_eq!(token.expires_in, 86400, "the Minecraft token lives 24 h");
    assert!(
        token.refresh_token.is_none(),
        "the Minecraft token carries no refresh half"
    );

    let calls = script.json_calls();
    assert_eq!(calls.len(), 1, "one login_with_xbox request");
    assert_eq!(calls[0].0, AuthEndpoints::default().mc_login);
    assert_eq!(
        String::from_utf8(calls[0].1.clone()).expect("the request is UTF-8"),
        r#"{"identityToken":"XBL3.0 x=SYN-UHS-001;SYN-XSTS-TOKEN-001"}"#,
        "literal `XBL3.0 x=`, the uhs, a `;`, then the XSTS token (§3.4)"
    );
    assert!(script.is_exhausted());
}

#[test]
fn entitlements_parses_the_two_items_as_owned() {
    let (auth, script, _) = harness_with_env(vec![Exchange::ok(&entitlements_body())]);

    let entitlements = auth
        .fetch_entitlements("SYN-MC-ACCESS-001")
        .expect("the entitlements fetch succeeds");

    assert!(entitlements.owns_minecraft, "either owning item suffices");
    assert_eq!(
        entitlements.items,
        vec![
            "product_minecraft".to_string(),
            "game_minecraft".to_string()
        ]
    );

    let calls = script.bearer_calls();
    assert_eq!(calls.len(), 1, "one bearer GET");
    assert_eq!(calls[0].0, AuthEndpoints::default().mc_entitlements);
    assert_eq!(
        calls[0].1, "SYN-MC-ACCESS-001",
        "the Minecraft access token rides as the bearer"
    );
    assert!(script.is_exhausted());
}

#[test]
fn profile_parses_id_and_name() {
    let (auth, script, _) = harness_with_env(vec![Exchange::ok(&profile_body())]);

    let profile = auth
        .fetch_profile("SYN-MC-ACCESS-001")
        .expect("the profile fetch succeeds");

    assert_eq!(profile.uuid_hex, PROFILE_UUID_HEX);
    assert_eq!(profile.name, "SynPlayer");

    let calls = script.bearer_calls();
    assert_eq!(calls.len(), 1, "one bearer GET");
    assert_eq!(calls[0].0, AuthEndpoints::default().mc_profile);
    assert_eq!(
        calls[0].1, "SYN-MC-ACCESS-001",
        "the Minecraft access token rides as the bearer"
    );
    assert!(script.is_exhausted());
}

#[test]
fn complete_sign_in_runs_the_chain_in_order() {
    let (auth, script, _) = harness_with_env(vec![
        Exchange::ok(&xbox_body("SYN-XBL-TOKEN-101", "SYN-UHS-101")),
        Exchange::ok(&xbox_body("SYN-XSTS-TOKEN-101", "SYN-UHS-101")),
        Exchange::ok(&login_body_with("SYN-MC-ACCESS-101")),
        Exchange::ok(&entitlements_body()),
        Exchange::ok(&profile_body()),
    ]);
    let msa = oxide_assets::auth::MsaToken {
        access_token: "SYN-MS-ACCESS-101".to_string(),
        refresh_token: Some("SYN-MS-REFRESH-101".to_string()),
        expires_in: 3599,
    };

    let signed = auth.complete_sign_in(msa).expect("the full chain signs in");

    assert_eq!(signed.mc_token.access_token, "SYN-MC-ACCESS-101");
    assert_eq!(signed.mc_token.expires_in, 86400);
    assert_eq!(signed.profile.uuid_hex, PROFILE_UUID_HEX);
    assert_eq!(signed.profile.name, "SynPlayer");
    assert!(signed.entitlements.owns_minecraft);
    assert_eq!(
        signed.refreshed_msa_refresh_token.as_deref(),
        Some("SYN-MS-REFRESH-101"),
        "the MSA refresh half rides along for the keyring"
    );

    let endpoints = AuthEndpoints::default();
    let json_urls: Vec<String> = script
        .json_calls()
        .into_iter()
        .map(|(url, _)| url)
        .collect();
    assert_eq!(
        json_urls,
        vec![
            endpoints.xbl_authenticate,
            endpoints.xsts_authorize,
            endpoints.mc_login,
        ],
        "XBL, then XSTS, then the Minecraft login"
    );
    // The login carries the XSTS user hash and token from the steps above.
    assert_eq!(
        String::from_utf8(script.json_calls()[2].1.clone()).expect("UTF-8"),
        r#"{"identityToken":"XBL3.0 x=SYN-UHS-101;SYN-XSTS-TOKEN-101"}"#
    );
    let bearer_urls: Vec<String> = script
        .bearer_calls()
        .into_iter()
        .map(|(url, _)| url)
        .collect();
    assert_eq!(
        bearer_urls,
        vec![endpoints.mc_entitlements, endpoints.mc_profile],
        "entitlements before the profile"
    );
    assert!(script.is_exhausted());
}

#[test]
fn silent_sign_in_refreshes_then_completes() {
    let (auth, script, client_id) = harness_with_env(vec![
        Exchange::ok(&token_body("SYN-MS-ACCESS-102", "SYN-MS-REFRESH-102")),
        Exchange::ok(&xbox_body("SYN-XBL-TOKEN-102", "SYN-UHS-102")),
        Exchange::ok(&xbox_body("SYN-XSTS-TOKEN-102", "SYN-UHS-102")),
        Exchange::ok(&login_body_with("SYN-MC-ACCESS-102")),
        Exchange::ok(&entitlements_body()),
        Exchange::ok(&profile_body()),
    ]);

    let signed = auth
        .silent_sign_in("SYN-MS-REFRESH-001")
        .expect("silent refresh signs in");

    assert_eq!(signed.mc_token.access_token, "SYN-MC-ACCESS-102");
    assert_eq!(signed.profile.name, "SynPlayer");
    assert_eq!(
        signed.refreshed_msa_refresh_token.as_deref(),
        Some("SYN-MS-REFRESH-102"),
        "the refreshed token — not the presented one — is the stored half"
    );

    let forms = script.form_calls();
    assert_eq!(forms.len(), 1, "silent refresh needs no Xbox state first");
    assert_eq!(forms[0].0, AuthEndpoints::default().token);
    assert_eq!(
        forms[0].1,
        vec![
            ("grant_type".to_string(), "refresh_token".to_string()),
            (
                "refresh_token".to_string(),
                "SYN-MS-REFRESH-001".to_string()
            ),
            ("client_id".to_string(), client_id),
        ],
        "the verbatim refresh grant, with the resolver's client id"
    );
    assert!(script.is_exhausted());
}

#[test]
fn join_session_posts_the_verbatim_body_and_accepts_204() {
    let (auth, script, _) = harness_with_env(vec![Exchange::ok_with_status(204, b"")]);

    auth.join_session(PROFILE_UUID_HEX, "SYN-MC-ACCESS-201", "SYN-HASH-201")
        .expect("204 with an empty body is success");

    let calls = script.json_calls();
    assert_eq!(calls.len(), 1, "one join request");
    assert_eq!(calls[0].0, AuthEndpoints::default().session_join);
    assert_eq!(
        String::from_utf8(calls[0].1.clone()).expect("the request is UTF-8"),
        r#"{"accessToken":"SYN-MC-ACCESS-201","selectedProfile":"11111111222233334444555555555555","serverId":"SYN-HASH-201"}"#,
        "the §3.7 body byte for byte: the three keys, the uuid without dashes"
    );
    assert!(script.is_exhausted());
}

#[test]
fn join_refusals_map_to_typed_errors_with_operator_sentences() {
    let (auth, _script, _) =
        harness_with_env(vec![Exchange::fail(403, join_forbidden_body().as_bytes())]);
    let error = auth
        .join_session(PROFILE_UUID_HEX, "SYN-MC-ACCESS-202", "SYN-HASH-202")
        .expect_err("the service refuses");
    assert!(
        matches!(error, JoinError::ForbiddenOperation),
        "ForbiddenOperationException is typed: {error}"
    );
    assert_eq!(
        error.to_string(),
        "the session server refused the join (ForbiddenOperationException)"
    );

    let (auth, _script, _) =
        harness_with_env(vec![Exchange::fail(403, join_privileges_body().as_bytes())]);
    let error = auth
        .join_session(PROFILE_UUID_HEX, "SYN-MC-ACCESS-202", "SYN-HASH-202")
        .expect_err("multiplayer is disabled");
    assert!(
        matches!(error, JoinError::InsufficientPrivileges),
        "InsufficientPrivilegesException is typed: {error}"
    );
    assert_eq!(
        error.to_string(),
        "InsufficientPrivilegesException: Xbox multiplayer is disabled for this account"
    );

    let (auth, _script, _) =
        harness_with_env(vec![Exchange::fail(403, join_banned_body().as_bytes())]);
    let error = auth
        .join_session(PROFILE_UUID_HEX, "SYN-MC-ACCESS-202", "SYN-HASH-202")
        .expect_err("the account is banned");
    let JoinError::Banned { message } = &error else {
        panic!("UserBannedException is a Banned error, not {error}");
    };
    assert_eq!(message, "You are banned from multiplayer.");
    assert_eq!(
        error.to_string(),
        "the account is banned from multiplayer (UserBannedException): You are banned from multiplayer."
    );
}

#[test]
fn client_id_override_rules() {
    // The override lives in the process environment, and swapping it in the
    // parent would race parallel tests (besides needing `unsafe`, which the
    // workspace forbids) — so each rule runs in a child process, the repo's
    // established pattern (`the_bare_client_builds_with_no_extraction_tree`).
    // The child re-enters this same test with the probe case set and asserts
    // the resolver directly.
    if let Ok(case) = std::env::var("OXIDECRAFT_AUTH_CLIENT_CASE") {
        match case.as_str() {
            "override" => assert_eq!(
                client_id_from_env(),
                CLIENT_ID,
                "a set override wins over the constant"
            ),
            "unset" => assert_eq!(
                client_id_from_env(),
                AUTH_CLIENT_ID,
                "an unset override falls back to the constant"
            ),
            "empty" => assert_eq!(
                client_id_from_env(),
                AUTH_CLIENT_ID,
                "an empty override falls back to the constant"
            ),
            other => panic!("unknown client-id probe case: {other}"),
        }
        return;
    }
    let exe = std::env::current_exe().expect("the test binary path");
    for case in ["override", "unset", "empty"] {
        let mut probe = std::process::Command::new(&exe);
        probe
            .arg("--exact")
            .arg("client_id_override_rules")
            .env("OXIDECRAFT_AUTH_CLIENT_CASE", case);
        match case {
            "override" => {
                probe.env("OXIDECRAFT_AUTH_CLIENT_ID", CLIENT_ID);
            }
            // `env_remove` keeps the probe hermetic even when the runner
            // exports an override (the gate does, so stage-two tests exercise
            // the override path).
            "unset" => {
                probe.env_remove("OXIDECRAFT_AUTH_CLIENT_ID");
            }
            "empty" => {
                probe.env("OXIDECRAFT_AUTH_CLIENT_ID", "");
            }
            _ => unreachable!("the loop lists every case"),
        }
        let status = probe.status().expect("the probe process runs");
        assert!(status.success(), "the {case} client-id rule holds");
    }
}

#[test]
fn signed_in_debug_redacts_tokens_but_keeps_identity() {
    let signed = oxide_assets::auth::SignedIn {
        mc_token: oxide_assets::auth::MsaToken {
            access_token: "SENTINEL-MC-ACCESS".to_string(),
            refresh_token: None,
            expires_in: 86400,
        },
        profile: oxide_assets::auth::McProfile {
            uuid_hex: PROFILE_UUID_HEX.to_string(),
            name: "SynPlayer".to_string(),
        },
        entitlements: oxide_assets::auth::Entitlements {
            owns_minecraft: true,
            items: vec!["product_minecraft".to_string()],
        },
        refreshed_msa_refresh_token: Some("SENTINEL-REFRESH".to_string()),
    };

    let debug = format!("{signed:?}");
    for secret in ["SENTINEL-MC-ACCESS", "SENTINEL-REFRESH"] {
        assert!(
            !debug.contains(secret),
            "no token reaches the Debug path: {debug}"
        );
    }
    assert!(
        debug.contains("SynPlayer"),
        "the player identity stays visible: {debug}"
    );
}

// --- Step 3: the refusal and tolerance cases --------------------------------

#[test]
fn empty_entitlements_is_a_no_ownership_typed_error() {
    // The fetcher reports; it does not refuse.
    let (auth, _script, _) = harness_with_env(vec![Exchange::ok(&entitlements_empty_body())]);
    let entitlements = auth
        .fetch_entitlements("SYN-MC-ACCESS-301")
        .expect("an empty list parses");
    assert!(!entitlements.owns_minecraft);
    assert!(entitlements.items.is_empty());

    // The driver refuses: the scripted profile answer must never be consumed.
    let (auth, script, _) = harness_with_env(vec![
        Exchange::ok(&xbox_body("SYN-XBL-TOKEN-301", "SYN-UHS-301")),
        Exchange::ok(&xbox_body("SYN-XSTS-TOKEN-301", "SYN-UHS-301")),
        Exchange::ok(&login_body_with("SYN-MC-ACCESS-301")),
        Exchange::ok(&entitlements_empty_body()),
        Exchange::ok(&profile_body()),
    ]);
    let msa = oxide_assets::auth::MsaToken {
        access_token: "SYN-MS-ACCESS-301".to_string(),
        refresh_token: Some("SYN-MS-REFRESH-301".to_string()),
        expires_in: 3599,
    };
    let error = auth
        .complete_sign_in(msa)
        .expect_err("a merely-working Microsoft account is refused");
    let AuthError::Shape { step, message } = &error else {
        panic!("no ownership is a typed Shape error, not {error}");
    };
    assert_eq!(*step, AuthStep::Entitlements);
    assert!(
        message.contains("does not own Minecraft"),
        "the refusal names the missing ownership: {message}"
    );
    assert_eq!(
        script.bearer_calls().len(),
        1,
        "the profile is never fetched once ownership fails"
    );
}

#[test]
fn profile_401_is_a_status_error() {
    // The survey-verbatim bogus-token shape (§3.5).
    let body = r#"{"path":"/minecraft/profile"}"#;
    let (auth, _script, _) = harness_with_env(vec![Exchange::fail(401, body.as_bytes())]);

    let error = auth
        .fetch_profile("SYN-MC-ACCESS-BAD")
        .expect_err("a bogus token is refused");
    let AuthError::Status { step, code, .. } = &error else {
        panic!("a 401 is a Status error, not {error}");
    };
    assert_eq!(*step, AuthStep::Profile);
    assert_eq!(*code, 401);
    assert!(
        error.to_string().contains("profile"),
        "the step rides in the message: {error}"
    );
}

#[test]
fn unknown_extra_fields_are_tolerated_in_stage_two_bodies() {
    // The §3.6 `profileActions` lesson applied to stage two: the services
    // add fields over time, and the parser must ignore what it does not know.
    let extra_login = login_body().replace(
        "\"token_type\"",
        "\"profileActions\":[],\"future_field\":{\"nested\":true},\"token_type\"",
    );
    let extra_entitlements = entitlements_body().replace(
        "\"signature\":\"SYN-OUTER-SIG\"",
        "\"signature\":\"SYN-OUTER-SIG\",\"future_field\":7",
    );
    let extra_profile = profile_body().replace(
        "\"profileActions\"",
        "\"future_field\":{\"nested\":true},\"profileActions\"",
    );
    let (auth, _script, _) = harness_with_env(vec![
        Exchange::ok(&extra_login),
        Exchange::ok(&extra_entitlements),
        Exchange::ok(&extra_profile),
    ]);

    let token = auth
        .login_with_xbox("SYN-UHS-302", "SYN-XSTS-TOKEN-302")
        .expect("extra fields parse");
    assert_eq!(token.expires_in, 86400);
    let entitlements = auth
        .fetch_entitlements(&token.access_token)
        .expect("extra fields parse");
    assert!(entitlements.owns_minecraft);
    let profile = auth
        .fetch_profile(&token.access_token)
        .expect("extra fields parse");
    assert_eq!(profile.uuid_hex, PROFILE_UUID_HEX);
}

#[test]
fn uuid_with_dashes_validates_32_hex_digits() {
    assert_eq!(
        uuid_with_dashes("11111111222233334444555555555555"),
        Some("11111111-2222-3333-4444-555555555555".to_string())
    );
    assert_eq!(
        uuid_with_dashes("069a79f444e94726a5befca90e38aaf5"),
        Some("069a79f4-44e9-4726-a5be-fca90e38aaf5".to_string()),
        "the survey's live Notch id (§3.6) converts"
    );
    for hostile in [
        "",
        "too-short",
        "11111111-2222-3333-4444-555555555555",
        "1111111122223333444455555555555",
        "111111112222333344445555555555555",
        "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        "1111111122223333444455555555555 ",
        " 1111111122223333444455555555555",
    ] {
        assert_eq!(
            uuid_with_dashes(hostile),
            None,
            "hostile input is refused, never repaired: {hostile:?}"
        );
    }
}

#[test]
fn join_session_refuses_a_malformed_uuid_without_touching_the_wire() {
    // The scripted 204 must never be consumed: validation precedes the wire.
    let (auth, script, _) = harness_with_env(vec![Exchange::ok_with_status(204, b"")]);

    let error = auth
        .join_session("not-a-uuid", "SYN-MC-ACCESS-303", "SYN-HASH-303")
        .expect_err("a malformed uuid never reaches the wire");
    assert!(
        matches!(error, JoinError::Shape { .. }),
        "hostile input is a typed Shape error: {error}"
    );
    assert!(
        script.json_calls().is_empty(),
        "no request is issued for hostile input"
    );
}

#[test]
fn malformed_profile_id_is_a_shape_error() {
    let body = r#"{"id":"not-a-uuid","name":"SynPlayer"}"#;
    let (auth, _script, _) = harness_with_env(vec![Exchange::ok(body)]);

    let error = auth
        .fetch_profile("SYN-MC-ACCESS-304")
        .expect_err("a hostile id is refused");
    assert!(
        matches!(
            error,
            AuthError::Shape {
                step: AuthStep::Profile,
                ..
            }
        ),
        "a malformed id is a typed Shape error, never stored: {error}"
    );
}

#[test]
fn join_unknown_and_garbage_refusals_are_shape_errors() {
    // The survey's non-descriptive `{"error":"Forbidden"}` case (§3.7): a
    // well-formed refusal with an undocumented name.
    let (auth, _script, _) = harness_with_env(vec![Exchange::fail(
        403,
        br#"{"error":"Forbidden","path":"/session/minecraft/join"}"#,
    )]);
    let error = auth
        .join_session(PROFILE_UUID_HEX, "SYN-MC-ACCESS-305", "SYN-HASH-305")
        .expect_err("an undocumented error name is typed");
    assert!(
        matches!(error, JoinError::Shape { .. }),
        "unknown names are Shape errors: {error}"
    );
    assert!(
        error.to_string().contains("unknown error"),
        "the message names the gap: {error}"
    );

    // Not JSON at all: still typed, never a panic.
    let (auth, _script, _) = harness_with_env(vec![Exchange::fail(403, b"not json")]);
    let error = auth
        .join_session(PROFILE_UUID_HEX, "SYN-MC-ACCESS-305", "SYN-HASH-305")
        .expect_err("garbage is typed");
    assert!(
        matches!(error, JoinError::Shape { .. }),
        "garbage answers are typed, never panics: {error}"
    );
    assert!(
        error.to_string().contains("does not parse"),
        "the message names the gap: {error}"
    );
}
