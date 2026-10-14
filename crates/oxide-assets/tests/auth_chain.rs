//! The device-code flow and the Xbox Live / XSTS chain against a scripted
//! endpoint double: every response below mirrors the survey's verbatim shapes
//! (§3.1–§3.3) with synthetic credential values, every request's exact bytes
//! are asserted at the stub, and no test touches a socket, the clock, or a
//! real Microsoft endpoint — the poll's sleeper is injected and its calls are
//! recorded.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use oxide_assets::auth::{Auth, AuthEndpoints, AuthError, AuthStep};
use oxide_assets::http::{HttpClient, HttpError};

/// The client id every test polls with: fixed so the byte pins are exact.
const CLIENT_ID: &str = "TEST-CLIENT-ID-0000-000000000000";
/// The scope string, verbatim from survey §3.1 (`offline_access` is what
/// yields the refresh token).
const SCOPE: &str = "XboxLive.signin offline_access";
/// The device-code grant, verbatim from Microsoft Learn via survey §3.1.
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// One scripted exchange: a status plus the body delivered verbatim. A 2xx
/// status answers `Ok`, anything else answers `HttpError::Status` — exactly
/// the translation Task 4's client performs, headers dropped.
///
/// Deviation D1 (recorded in the task report): the real XBL 401 carries an
/// empty body plus an `x-xblcorrelationid` response header, but Task 4's
/// `HttpError::Status` has no header slot, so the header cannot reach the
/// auth layer through the allowed surface. The scripted exchange therefore
/// carries the scripted correlation-id value in the delivered Status body —
/// the auth layer renders whatever the transport delivered — while a
/// separate fixture pins the survey-verbatim empty body.
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

    /// A failed exchange delivering `body` verbatim (empty for the
    /// survey-verbatim XBL shape, the D1 correlation line for the
    /// correlation-id pin).
    fn fail(status: u16, body: &[u8]) -> Self {
        Self {
            status,
            body: body.to_vec(),
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

    fn get_with_bearer(&self, _url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
        unimplemented!("the auth chain never issues a bearer GET in this task")
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
            AuthError::Status { step: AuthStep::XboxLive, code: 401, ref body } if body.is_empty()
        ),
        "an empty 401 is a status error carrying the code: {error}"
    );
}

#[test]
fn xbl_error_message_reports_the_status_and_the_correlation_id() {
    // The scripted exchange stands in for the full HTTP exchange (see D1):
    // the scripted x-xblcorrelationid value is delivered in the Status body,
    // and the message renders whatever the transport delivered.
    let (auth, _script) = harness(vec![Exchange::fail(
        401,
        b"x-xblcorrelationid: 3f2504e0-0000-0000-0000-000000000000",
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
            AuthError::Status { step: AuthStep::Xsts, code: 400, ref body } if body.is_empty()
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
