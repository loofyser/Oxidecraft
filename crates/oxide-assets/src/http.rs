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

        let mut delay = Duration::from_secs(1);
        let mut last = None;
        for _ in 0..3 {
            match self.agent.get(url).call() {
                Ok(response) => {
                    let mut body = Vec::new();
                    response
                        .into_body()
                        .into_reader()
                        .read_to_end(&mut body)
                        .map_err(|error| HttpError::Transport {
                            url: url.to_string(),
                            message: error.to_string(),
                        })?;
                    return Ok(body);
                }
                // A non-success status will not change on a retry.
                Err(ureq::Error::StatusCode(code)) => {
                    return Err(HttpError::Status {
                        url: url.to_string(),
                        code,
                    });
                }
                Err(error) => {
                    last = Some(error.to_string());
                    std::thread::sleep(delay);
                    delay *= 4;
                }
            }
        }
        Err(HttpError::Transport {
            url: url.to_string(),
            message: last.unwrap_or_default(),
        })
    }
}
