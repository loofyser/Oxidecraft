//! The one place HTTP happens. Everything else takes this trait.
//!
//! Non-2xx responses surface as [`HttpError::Status`] carrying the response
//! body (capped at [`MAX_ERROR_BODY_BYTES`]): the sign-in chain reads failure
//! documents (403 `ForbiddenOperationException`, 401 shapes, `AADSTS…` JSON)
//! out of them. Mechanism, verified first-hand against the vendored ureq 3.4.2
//! source (`src/config.rs`, `src/request.rs`, `src/run.rs`): every request is
//! built as `agent.<verb>(url).<headers>.config().http_status_as_error(false)`
//! `.build().<call|send>`, so a non-2xx status returns `Ok(Response<Body>)`
//! instead of `Err(Error::StatusCode)` (which drops the body) and the status
//! is read back off the response with `response.status().as_u16()`. The
//! success path is untouched: 2xx bodies stream fully, exactly as before.

use std::io::Read;
use std::time::Duration;

/// Errors from an HTTP fetch.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// Transport failure.
    #[error("transport error for {url}: {message}")]
    Transport {
        /// Requested URL.
        url: String,
        /// Underlying message.
        message: String,
    },
    /// Non-success status code. The message names only the code and the URL;
    /// the response body rides in `body` for the caller to read.
    #[error("HTTP {code} for {url}")]
    Status {
        /// Requested URL.
        url: String,
        /// Status code.
        code: u16,
        /// The response body, capped at [`MAX_ERROR_BODY_BYTES`].
        body: Vec<u8>,
    },
    /// The URL does not carry an http or https scheme.
    #[error("unsupported scheme in {url}")]
    UnsupportedScheme {
        /// Requested URL.
        url: String,
    },
}

/// Fetches bytes by URL. Implementations must follow redirects and set sensible timeouts.
///
/// The trait is `Send + Sync` so one client can cross the session thread
/// behind an `Arc<dyn HttpClient>`.
pub trait HttpClient: Send + Sync {
    /// Performs a GET and returns the body.
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError>;

    /// POSTs `body` as JSON and returns the response body. Wire shape:
    /// `Content-Type: application/json` and `Accept: application/json` — XBL
    /// and Minecraft Services complain without both (survey §3.2/§3.4).
    fn post_json(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, HttpError>;

    /// POSTs `fields` as `application/x-www-form-urlencoded` and returns the
    /// response body. The encoder percent-encodes (space as `%20`, never `+`),
    /// so `client_id=…&scope=XboxLive.signin%20offline_access` is reproduced
    /// byte-exactly (survey §3.1). Note ureq's own `send_form` encodes space
    /// as `+`, which is why this client encodes the body itself.
    fn post_form(&self, url: &str, fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError>;

    /// Performs a GET with a bearer token and returns the body. Wire shape:
    /// `Authorization: Bearer <token>` — the space after `Bearer` is
    /// mandatory (survey §3.5). The token is never logged.
    fn get_with_bearer(&self, url: &str, bearer: &str) -> Result<Vec<u8>, HttpError>;
}

/// Production client backed by `ureq`, with three tries and exponential backoff.
pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    /// Builds a client with 30 second timeouts.
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

/// True when `url` carries an `http` or `https` scheme.
fn is_http_url(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        if !is_http_url(url) {
            return Err(HttpError::UnsupportedScheme {
                url: url.to_string(),
            });
        }

        exchange(
            url,
            || {
                self.agent
                    .get(url)
                    .config()
                    .http_status_as_error(false)
                    .build()
                    .call()
                    .map_err(|error| classify(url, error))
            },
            std::thread::sleep,
        )
    }

    fn post_json(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, HttpError> {
        if !is_http_url(url) {
            return Err(HttpError::UnsupportedScheme {
                url: url.to_string(),
            });
        }

        exchange(
            url,
            || {
                self.agent
                    .post(url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json")
                    .config()
                    .http_status_as_error(false)
                    .build()
                    .send(body)
                    .map_err(|error| classify(url, error))
            },
            std::thread::sleep,
        )
    }

    fn post_form(&self, url: &str, fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError> {
        if !is_http_url(url) {
            return Err(HttpError::UnsupportedScheme {
                url: url.to_string(),
            });
        }
        let body = encode_form(fields);

        exchange(
            url,
            || {
                self.agent
                    .post(url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .config()
                    .http_status_as_error(false)
                    .build()
                    .send(body.as_str())
                    .map_err(|error| classify(url, error))
            },
            std::thread::sleep,
        )
    }

    fn get_with_bearer(&self, url: &str, bearer: &str) -> Result<Vec<u8>, HttpError> {
        if !is_http_url(url) {
            return Err(HttpError::UnsupportedScheme {
                url: url.to_string(),
            });
        }
        // The token rides in the header value only; it is never logged.
        let authorization = format!("Bearer {bearer}");

        exchange(
            url,
            || {
                self.agent
                    .get(url)
                    .header("Authorization", authorization.as_str())
                    .config()
                    .http_status_as_error(false)
                    .build()
                    .call()
                    .map_err(|error| classify(url, error))
            },
            std::thread::sleep,
        )
    }
}

/// The error-body cap: 64 KiB.
///
/// The failure documents the sign-in chain reads are small JSON objects — the
/// survey's `ForbiddenOperationException` (~70 bytes), the `{"path": …}` 401s
/// (~30 bytes), the `AADSTS…` descriptions (hundreds of bytes). 64 KiB sits
/// orders of magnitude above any legitimate auth error, so no real failure is
/// ever truncated, while a hostile or broken server cannot make every failed
/// request allocate megabytes before the typed error even exists.
pub const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Encodes `fields` as an `application/x-www-form-urlencoded` body:
/// `name=value` pairs joined with `&`, every byte outside the unreserved set
/// (`A-Z a-z 0-9 - _ . ~`) percent-encoded with uppercase hex, space as
/// `%20`. Deliberately not ureq's `send_form`, which encodes space as `+`.
fn encode_form(fields: &[(&str, &str)]) -> String {
    fn push_escaped(out: &mut String, text: &str) {
        for byte in text.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                _ => {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }

    let mut body = String::new();
    for (index, (name, value)) in fields.iter().enumerate() {
        if index > 0 {
            body.push('&');
        }
        push_escaped(&mut body, name);
        body.push('=');
        push_escaped(&mut body, value);
    }
    body
}

/// One request under the retry law: transport failures and 5xx statuses try
/// again (up to [`TRIES`]), 4xx and scheme errors return at once.
///
/// `call` is one HTTP exchange. A 2xx or 4xx response leaves the loop at
/// once — the 2xx body then streams exactly once, and a body-read transport
/// failure returns without another attempt, the old GET shape — while a 5xx
/// response becomes a retryable [`HttpError::Status`] (its body read capped,
/// the final attempt's travelling with the error).
fn exchange(
    url: &str,
    mut call: impl FnMut() -> Result<ureq::http::Response<ureq::Body>, HttpError>,
    sleep: impl FnMut(Duration),
) -> Result<Vec<u8>, HttpError> {
    let response = with_retries(
        || {
            let response = call()?;
            if is_final(response.status().as_u16()) {
                Ok(response)
            } else {
                Err(status_error(url, response))
            }
        },
        sleep,
    )?;
    finish(url, response)
}

/// True when a status leaves the retry loop at once: any 2xx success, or any
/// non-retryable failure (a 4xx will not change on another attempt).
fn is_final(code: u16) -> bool {
    (200..300).contains(&code) || !is_retryable(&shell_status(code))
}

/// A [`HttpError::Status`] shell carrying only `code`, for retry decisions
/// before the body is read.
fn shell_status(code: u16) -> HttpError {
    HttpError::Status {
        url: String::new(),
        code,
        body: Vec::new(),
    }
}

/// Builds the retryable 5xx error for one attempt's response, reading its
/// body capped; a body that will not read becomes an empty one — the status
/// still retries.
fn status_error(url: &str, response: ureq::http::Response<ureq::Body>) -> HttpError {
    let code = response.status().as_u16();
    let body = read_capped(response.into_body()).unwrap_or_default();
    HttpError::Status {
        url: url.to_string(),
        code,
        body,
    }
}
/// Splits a finished response: a 2xx status streams its full body as the
/// success value (a body-read transport failure returns at once, the old GET
/// shape), anything else becomes a [`HttpError::Status`] carrying the capped
/// response body.
fn finish(url: &str, response: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, HttpError> {
    let code = response.status().as_u16();
    let body = response.into_body();
    if (200..300).contains(&code) {
        let mut bytes = Vec::new();
        body.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|error| HttpError::Transport {
                url: url.to_string(),
                message: error.to_string(),
            })?;
        Ok(bytes)
    } else {
        Err(HttpError::Status {
            url: url.to_string(),
            code,
            body: read_capped(body).map_err(|error| HttpError::Transport {
                url: url.to_string(),
                message: error.to_string(),
            })?,
        })
    }
}

/// Reads an error body, capped: at most one byte past [`MAX_ERROR_BODY_BYTES`]
/// is pulled before the tail is cut, so a hostile body can neither grow the
/// allocation nor fail the read into an untyped error.
fn read_capped(body: ureq::Body) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    body.into_reader()
        .take(MAX_ERROR_BODY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    bytes.truncate(MAX_ERROR_BODY_BYTES);
    Ok(bytes)
}

/// The number of attempts made for a single request.
const TRIES: u32 = 3;

/// The delay before a second attempt; it grows by [`DELAY_FACTOR`] before each
/// later attempt, so the delays between attempts are 1 s and then 4 s.
const FIRST_DELAY: Duration = Duration::from_secs(1);

/// The growth factor between attempt delays.
const DELAY_FACTOR: u32 = 4;

/// Maps a `ureq` failure onto the error taxonomy: a response status keeps its
/// code, everything else is a transport failure. With
/// `http_status_as_error(false)` on every request the `StatusCode` arm below
/// is unreachable — statuses arrive as responses, not errors — and stays only
/// so a taxonomy gap can never compile into a wrong variant.
fn classify(url: &str, error: ureq::Error) -> HttpError {
    match error {
        ureq::Error::StatusCode(code) => HttpError::Status {
            url: url.to_string(),
            code,
            body: Vec::new(),
        },
        other => HttpError::Transport {
            url: url.to_string(),
            message: other.to_string(),
        },
    }
}

/// True when another attempt could succeed: transport failures and server-side
/// (5xx) statuses. A 4xx status will not change on a retry.
fn is_retryable(error: &HttpError) -> bool {
    match error {
        HttpError::Status { code, .. } => *code >= 500,
        HttpError::Transport { .. } => true,
        HttpError::UnsupportedScheme { .. } => false,
    }
}

/// Runs `call` until it succeeds, fails in a way a retry cannot fix, or the
/// attempt limit is reached; the final failure is returned unchanged.
///
/// A pause is taken only when another attempt remains, so the last attempt is
/// never followed by a sleep.
fn with_retries<T>(
    mut call: impl FnMut() -> Result<T, HttpError>,
    mut sleep: impl FnMut(Duration),
) -> Result<T, HttpError> {
    let mut delay = FIRST_DELAY;
    let mut failures = 0;
    loop {
        match call() {
            Ok(value) => return Ok(value),
            Err(error) => {
                failures += 1;
                if failures >= TRIES || !is_retryable(&error) {
                    return Err(error);
                }
            }
        }
        // Reached only when another attempt remains.
        sleep(delay);
        delay *= DELAY_FACTOR;
    }
}

#[cfg(test)]
mod tests {
    //! Retry policy: the attempt budget, the pauses between attempts, and which
    //! failures are worth another attempt.

    use std::time::Duration;

    use super::{HttpError, TRIES, is_retryable, with_retries};

    const URL: &str = "https://resources.download.minecraft.net/aa/object";

    /// A transport failure, the kind a retry is for.
    fn transport() -> HttpError {
        HttpError::Transport {
            url: URL.to_string(),
            message: "connection reset".to_string(),
        }
    }

    /// A non-success response.
    fn status(code: u16) -> HttpError {
        HttpError::Status {
            url: URL.to_string(),
            code,
            body: Vec::new(),
        }
    }

    #[test]
    fn a_lasting_transport_failure_is_attempted_three_times() {
        let mut attempts = 0;
        let result: Result<u8, HttpError> = with_retries(
            || {
                attempts += 1;
                Err(transport())
            },
            |_| {},
        );

        assert!(matches!(result, Err(HttpError::Transport { .. })));
        assert_eq!(attempts, TRIES, "the attempt limit is three");
    }

    #[test]
    fn pauses_are_one_then_four_seconds_between_attempts_only() {
        let mut slept = Vec::new();
        let result: Result<u8, HttpError> =
            with_retries(|| Err(transport()), |delay| slept.push(delay));

        assert!(result.is_err());
        assert_eq!(
            slept,
            vec![Duration::from_secs(1), Duration::from_secs(4)],
            "the pauses are 1 s and 4 s, between attempts only"
        );
    }

    #[test]
    fn a_server_error_is_retried_until_the_attempt_limit() {
        let mut attempts = 0;
        let result: Result<u8, HttpError> = with_retries(
            || {
                attempts += 1;
                Err(status(503))
            },
            |_| {},
        );

        assert_eq!(attempts, TRIES, "a 5xx may be transient and is retried");
        assert!(matches!(result, Err(HttpError::Status { code: 503, .. })));
    }

    #[test]
    fn a_client_error_ends_the_request_immediately() {
        let mut attempts = 0;
        let mut slept = Vec::new();
        let result: Result<u8, HttpError> = with_retries(
            || {
                attempts += 1;
                Err(status(404))
            },
            |delay| slept.push(delay),
        );

        assert_eq!(attempts, 1, "a retry cannot change a 404");
        assert!(slept.is_empty(), "no pause may precede a final status");
        assert!(matches!(result, Err(HttpError::Status { code: 404, .. })));
    }

    #[test]
    fn a_retry_can_succeed() {
        let mut attempts = 0;
        let mut slept = Vec::new();
        let result: Result<u8, HttpError> = with_retries(
            || {
                attempts += 1;
                if attempts < 2 {
                    Err(transport())
                } else {
                    Ok(7)
                }
            },
            |delay| slept.push(delay),
        );

        assert_eq!(result.expect("the second attempt succeeds"), 7);
        assert_eq!(attempts, 2);
        assert_eq!(slept, vec![Duration::from_secs(1)]);
    }

    #[test]
    fn only_transport_failures_and_server_errors_are_retryable() {
        assert!(is_retryable(&transport()));
        assert!(is_retryable(&status(503)));
        assert!(!is_retryable(&status(404)));
        assert!(!is_retryable(&HttpError::UnsupportedScheme {
            url: URL.to_string(),
        }));
    }
}

/// The wire tests: request shapes and error bodies against a stub server.
///
/// A `TcpListener` on an ephemeral localhost port serves one canned response
/// per connection; each test asserts the raw request bytes (request line,
/// headers, body) and, for failures, the typed error the client returns.
#[cfg(test)]
mod wire_tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::{HttpClient, HttpError, MAX_ERROR_BODY_BYTES, UreqClient};

    /// The survey's device-code body shape (§3.1): the exact bytes the token
    /// endpoint must receive.
    const SURVEY_FORM_BODY: &str =
        "client_id=00000000402b5328&scope=XboxLive.signin%20offline_access";

    /// Serves canned raw responses on an ephemeral localhost port and records
    /// every raw request. The client pools keep-alive connections, so each
    /// connection is served request-by-request until it idles out or closes;
    /// every request pops the next canned response (the last one repeats when
    /// the queue runs dry).
    struct Stub {
        port: u16,
        requests: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Stub {
        /// Starts the stub; it answers requests with `responses` in order,
        /// repeating the last one when they run dry, until `join` stops it.
        fn start(responses: Vec<Vec<u8>>) -> Self {
            use std::collections::VecDeque;
            use std::sync::atomic::{AtomicBool, Ordering};

            let listener =
                TcpListener::bind("127.0.0.1:0").expect("the stub binds an ephemeral port");
            let port = listener.local_addr().expect("the stub has a port").port();
            listener
                .set_nonblocking(true)
                .expect("a nonblocking listener");
            let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let recorded = std::sync::Arc::clone(&requests);
            let queue = std::sync::Arc::new(std::sync::Mutex::new(VecDeque::from(responses)));
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let stopped = std::sync::Arc::clone(&stop);
            let handle = std::thread::spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(pair) => pair,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(error) => panic!("the stub accepts: {error}"),
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("a read timeout");
                    // One connection, request by request: a pooled client
                    // reuses it, so attempt counting must see requests, not
                    // connections.
                    while let Some(raw) = read_request(&stream) {
                        recorded.lock().expect("the log").push(raw);
                        let response = {
                            let mut queue = queue.lock().expect("the queue");
                            if queue.len() > 1 {
                                queue.pop_front().expect("a response")
                            } else {
                                queue.front().expect("a response").clone()
                            }
                        };
                        if stream.write_all(&response).is_err() {
                            break;
                        }
                    }
                }
            });
            Self {
                port,
                requests,
                stop,
                handle: Some(handle),
            }
        }

        /// The stub's URL for `path`.
        fn url(&self, path: &str) -> String {
            format!("http://127.0.0.1:{}{path}", self.port)
        }

        /// The raw requests so far.
        fn requests(&self) -> Vec<Vec<u8>> {
            self.requests.lock().expect("the log").clone()
        }

        /// Waits for the stub thread to finish serving.
        fn join(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(handle) = self.handle.take() {
                handle.join().expect("the stub thread");
            }
        }
    }

    /// Reads one request: the headers, then exactly the `Content-Length`
    /// body when one is present. `None` when the connection idles out or
    /// closes with no further request on it.
    fn read_request(stream: &std::net::TcpStream) -> Option<Vec<u8>> {
        let mut raw = Vec::new();
        let mut byte = [0u8; 1];
        while !raw.ends_with(b"\r\n\r\n") {
            match stream.take(1).read(&mut byte) {
                Ok(0) | Err(_) => return None,
                Ok(_) => raw.push(byte[0]),
            }
            if raw.len() > 1 << 20 {
                panic!("the request headers exceed 1 MiB");
            }
        }
        let head = String::from_utf8_lossy(&raw).into_owned();
        let length = head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        stream
            .take(length as u64)
            .read_exact(&mut body)
            .expect("the request body");
        raw.extend_from_slice(&body);
        Some(raw)
    }

    /// A canned raw response with `status` and `body`.
    fn canned(status: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    /// The lowercased head of a raw request, for case-insensitive header
    /// assertions.
    fn head(raw: &[u8]) -> String {
        let end = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|at| at + 4)
            .unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).to_lowercase()
    }

    /// The body of a raw request: the bytes after the blank line.
    fn request_body(raw: &[u8]) -> &[u8] {
        let at = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("the blank line")
            + 4;
        &raw[at..]
    }

    #[test]
    fn post_json_sends_json_headers_and_the_bytes() {
        let body = br#"{"Properties":{"AuthMethod":"RPS"}}"#;
        let mut stub = Stub::start(vec![canned("200 OK", b"{}")]);
        let http = UreqClient::new();

        let answer = http
            .post_json(&stub.url("/user/authenticate"), body)
            .expect("the stub answers");
        assert_eq!(answer, b"{}");
        drop(http);
        stub.join();

        let requests = stub.requests();
        assert_eq!(requests.len(), 1, "one attempt");
        let head = head(&requests[0]);
        assert!(
            head.starts_with("post /user/authenticate http/1.1\r\n"),
            "the request line: {head}"
        );
        assert!(
            head.contains("content-type: application/json"),
            "the JSON content type: {head}"
        );
        assert!(
            head.contains("accept: application/json"),
            "the JSON accept: {head}"
        );
        assert_eq!(
            request_body(&requests[0]),
            body,
            "the JSON bytes travel unchanged"
        );
    }

    #[test]
    fn post_form_sends_the_survey_body_byte_exactly() {
        let mut stub = Stub::start(vec![canned("200 OK", b"{}")]);
        let http = UreqClient::new();

        let answer = http
            .post_form(
                &stub.url("/devicecode"),
                &[
                    ("client_id", "00000000402b5328"),
                    ("scope", "XboxLive.signin offline_access"),
                ],
            )
            .expect("the stub answers");
        assert_eq!(answer, b"{}");
        drop(http);
        stub.join();

        let requests = stub.requests();
        assert_eq!(requests.len(), 1, "one attempt");
        let head = head(&requests[0]);
        assert!(
            head.starts_with("post /devicecode http/1.1\r\n"),
            "the request line: {head}"
        );
        assert!(
            head.contains("content-type: application/x-www-form-urlencoded"),
            "the form content type: {head}"
        );
        assert_eq!(
            request_body(&requests[0]),
            SURVEY_FORM_BODY.as_bytes(),
            "the survey's body byte-exactly, space as %20"
        );
    }

    #[test]
    fn get_with_bearer_sends_the_authorization_header() {
        let mut stub = Stub::start(vec![canned("200 OK", b"{}")]);
        let http = UreqClient::new();

        let answer = http
            .get_with_bearer(&stub.url("/entitlements/mcstore"), "test-token")
            .expect("the stub answers");
        assert_eq!(answer, b"{}");
        drop(http);
        stub.join();

        let requests = stub.requests();
        assert_eq!(requests.len(), 1, "one attempt");
        let head = head(&requests[0]);
        assert!(
            head.starts_with("get /entitlements/mcstore http/1.1\r\n"),
            "the request line: {head}"
        );
        assert!(
            head.contains("authorization: bearer test-token"),
            "the bearer header, `Bearer ` with its trailing space: {head}"
        );
    }

    #[test]
    fn a_400_carries_its_json_body() {
        let failure =
            br#"{"error":"ForbiddenOperationException","path":"/session/minecraft/join"}"#;
        let mut stub = Stub::start(vec![canned("403 Forbidden", failure)]);
        let http = UreqClient::new();
        let url = stub.url("/session/minecraft/join");

        let error = http.post_json(&url, b"{}").expect_err("a 403 is an error");
        drop(http);
        stub.join();

        assert!(matches!(&error, HttpError::Status { code: 403, .. }));
        match error {
            HttpError::Status {
                url: got,
                code,
                body,
            } => {
                assert_eq!(got, url);
                assert_eq!(code, 403);
                assert_eq!(body, failure, "the failure document travels");
            }
            other => panic!("a status error, not {other:?}"),
        }
        assert_eq!(
            stub.requests().len(),
            1,
            "a 4xx is never retried: one attempt"
        );
    }

    #[test]
    fn a_401_with_an_empty_body_carries_no_bytes() {
        let mut stub = Stub::start(vec![canned("401 Unauthorized", b"")]);
        let http = UreqClient::new();
        let url = stub.url("/user/authenticate");

        let error = http.post_json(&url, b"{}").expect_err("a 401 is an error");
        drop(http);
        stub.join();

        match error {
            HttpError::Status { code, body, .. } => {
                assert_eq!(code, 401);
                assert!(body.is_empty(), "the XBL shape: an empty body");
            }
            other => panic!("a status error, not {other:?}"),
        }
    }

    #[test]
    fn a_204_is_an_empty_success() {
        let mut stub = Stub::start(vec![canned("204 No Content", b"")]);
        let http = UreqClient::new();

        let answer = http
            .post_json(&stub.url("/session/minecraft/join"), b"{}")
            .expect("a 204 is a success");
        drop(http);
        stub.join();

        assert!(answer.is_empty());
    }

    #[test]
    fn a_500_is_retried_until_the_attempt_limit() {
        let failure = b" transient";
        let mut stub = Stub::start(vec![
            canned("500 Internal Server Error", failure),
            canned("500 Internal Server Error", failure),
            canned("500 Internal Server Error", failure),
        ]);
        let http = UreqClient::new();
        let url = stub.url("/xsts/authorize");

        let error = http
            .post_json(&url, b"{}")
            .expect_err("a lasting 500 is an error");
        drop(http);
        stub.join();

        assert_eq!(stub.requests().len(), 3, "three attempts");
        match error {
            HttpError::Status { code, body, .. } => {
                assert_eq!(code, 500);
                assert_eq!(body, failure, "the final attempt's body");
            }
            other => panic!("a status error, not {other:?}"),
        }
    }

    #[test]
    fn an_error_body_is_capped_before_it_travels() {
        let big = vec![b'x'; MAX_ERROR_BODY_BYTES + 1024];
        let mut stub = Stub::start(vec![canned("400 Bad Request", &big)]);
        let http = UreqClient::new();

        let error = http
            .post_json(&stub.url("/token"), b"{}")
            .expect_err("a 400 is an error");
        drop(http);
        stub.join();

        match error {
            HttpError::Status { body, .. } => {
                assert_eq!(
                    body.len(),
                    MAX_ERROR_BODY_BYTES,
                    "the cap holds on a hostile body"
                );
                assert_eq!(body, &big[..MAX_ERROR_BODY_BYTES]);
            }
            other => panic!("a status error, not {other:?}"),
        }
    }

    #[test]
    fn the_status_message_is_unchanged_by_the_body_field() {
        let error = HttpError::Status {
            url: "https://x.invalid/join".to_string(),
            code: 403,
            body: br#"{"error":"ForbiddenOperationException"}"#.to_vec(),
        };
        assert_eq!(
            error.to_string(),
            "HTTP 403 for https://x.invalid/join",
            "the message names only the code and the URL"
        );
    }
}
