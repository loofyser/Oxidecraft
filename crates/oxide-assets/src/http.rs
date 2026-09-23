//! The one place HTTP happens. Everything else takes this trait.

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
    /// Non-success status code.
    #[error("HTTP {code} for {url}")]
    Status {
        /// Requested URL.
        url: String,
        /// Status code.
        code: u16,
    },
    /// The URL does not carry an http or https scheme.
    #[error("unsupported scheme in {url}")]
    UnsupportedScheme {
        /// Requested URL.
        url: String,
    },
}

/// Fetches bytes by URL. Implementations must follow redirects and set sensible timeouts.
pub trait HttpClient {
    /// Performs a GET and returns the body.
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError>;
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

        let response = with_retries(
            || {
                self.agent
                    .get(url)
                    .call()
                    .map_err(|error| classify(url, error))
            },
            std::thread::sleep,
        )?;

        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|error| HttpError::Transport {
                url: url.to_string(),
                message: error.to_string(),
            })?;
        Ok(body)
    }
}

/// The number of attempts made for a single request.
const TRIES: u32 = 3;

/// The delay before a second attempt; it grows by [`DELAY_FACTOR`] before each
/// later attempt, so the delays between attempts are 1 s and then 4 s.
const FIRST_DELAY: Duration = Duration::from_secs(1);

/// The growth factor between attempt delays.
const DELAY_FACTOR: u32 = 4;

/// Maps a `ureq` failure onto the error taxonomy: a response status keeps its
/// code, everything else is a transport failure.
fn classify(url: &str, error: ureq::Error) -> HttpError {
    match error {
        ureq::Error::StatusCode(code) => HttpError::Status {
            url: url.to_string(),
            code,
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
