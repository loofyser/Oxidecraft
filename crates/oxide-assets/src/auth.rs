//! The Microsoft sign-in chain: the device-code request, the token poll,
//! the Xbox Live user authenticate, the XSTS authorize, the Minecraft
//! login-with-Xbox, the entitlement and profile checks, and the
//! session-server join.
//!
//! Every request shape is byte-pinned against the survey's live-probed
//! bodies (§3.1–§3.5, §3.7); every failure is a typed [`AuthError`] (chain)
//! or [`JoinError`] (session join). The module only ever talks through the
//! crate's [`HttpClient`][crate::http::HttpClient] trait, so tests script a
//! FIFO double instead of touching the network, and the poll's sleeper is
//! injected instead of sleeping for real.
//!
//! Credential hygiene: the token-carrying types ([`DeviceCode`],
//! [`MsaToken`], [`XboxToken`], [`XstsToken`], [`SignedIn`]) redact their
//! secrets in `Debug`, and no constructor ever places a token value into an
//! [`AuthError`] or [`JoinError`] — the chain tests poll and authenticate
//! with sentinel credentials, then fail a later step and assert the message
//! is clean.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use crate::http::{HttpClient, HttpError, MAX_ERROR_BODY_BYTES};

/// The polling wait the device-code answer advertises after a `slow_down`.
///
/// Derived rule (pinned by the `slow_down` corpus test): each `slow_down`
/// answer increases the polling interval by **5 seconds**, cumulatively —
/// RFC 8628 §3.5 ("the interval MUST be increased by 5 seconds for this and
/// all subsequent requests"), which the Microsoft device-code polling
/// contract follows. The survey flags the `interval` field as the client's
/// wait (§3.1); this constant is the only adjustment the protocol defines.
const SLOW_DOWN_BUMP_SECS: u64 = 5;

/// The registered Azure application id: the public OAuth identifier the
/// sign-in chain presents as `client_id` (owner Q2, recorded at approval
/// 2026-10-14). The value is public, never a secret — it only names the
/// application; the credentials are the tokens, which never reach a log.
/// Tests always go through [`client_id_from_env`] with the
/// `OXIDECRAFT_AUTH_CLIENT_ID` override set, so this value never affects a
/// test.
pub const AUTH_CLIENT_ID: &str = "f4e40e88-357e-416d-9351-2a87a814c323";

/// Resolves the client id: `OXIDECRAFT_AUTH_CLIENT_ID` when set and
/// non-empty, else [`AUTH_CLIENT_ID`].
pub fn client_id_from_env() -> String {
    match std::env::var("OXIDECRAFT_AUTH_CLIENT_ID") {
        Ok(value) if !value.is_empty() => value,
        _ => AUTH_CLIENT_ID.to_string(),
    }
}

/// The sign-in endpoints, injectable so tests point the chain at the
/// scripted double instead of the live services.
#[derive(Debug, Clone)]
pub struct AuthEndpoints {
    /// The Entra device-code endpoint (survey §3.1).
    pub device_code: String,
    /// The Entra token endpoint (survey §3.1).
    pub token: String,
    /// The Xbox Live user-authenticate endpoint (survey §3.2).
    pub xbl_authenticate: String,
    /// The XSTS authorize endpoint (survey §3.3).
    pub xsts_authorize: String,
    /// The Minecraft login-with-Xbox endpoint (survey §3.4; Task 6 owns it).
    pub mc_login: String,
    /// The entitlements endpoint (survey §3.5; Task 6 owns it).
    pub mc_entitlements: String,
    /// The Minecraft profile endpoint (survey §3.5; Task 6 owns it).
    pub mc_profile: String,
    /// The session-server join endpoint (survey §3.7; Task 6 owns it).
    pub session_join: String,
}

impl Default for AuthEndpoints {
    /// The survey's URLs verbatim. The two Entra URLs carry the `consumers`
    /// tenant — the survey's hard requirement ("you must use the `consumers`
    /// AAD tenant", §3.1): `common` or a tenant id errors out.
    fn default() -> Self {
        Self {
            device_code: "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode"
                .to_string(),
            token: "https://login.microsoftonline.com/consumers/oauth2/v2.0/token".to_string(),
            xbl_authenticate: "https://user.auth.xboxlive.com/user/authenticate".to_string(),
            xsts_authorize: "https://xsts.auth.xboxlive.com/xsts/authorize".to_string(),
            mc_login: "https://api.minecraftservices.com/authentication/login_with_xbox"
                .to_string(),
            mc_entitlements: "https://api.minecraftservices.com/entitlements/mcstore".to_string(),
            mc_profile: "https://api.minecraftservices.com/minecraft/profile".to_string(),
            session_join: "https://sessionserver.mojang.com/session/minecraft/join".to_string(),
        }
    }
}

/// The device-code answer: Microsoft Learn's field set verbatim via survey
/// §3.1. `interval` is the seconds to wait between polls; `expires_in`
/// bounds the whole flow (the survey's 15-minute window).
#[derive(Clone)]
pub struct DeviceCode {
    /// The long session string the client polls with.
    pub device_code: String,
    /// The short code the user types on the verification page.
    pub user_code: String,
    /// The page the user opens with the user code.
    pub verification_uri: String,
    /// Seconds until the device code expires.
    pub expires_in: u64,
    /// Seconds to wait between polls.
    pub interval: u64,
    /// Human-readable instructions for the user.
    pub message: String,
}

/// An Entra token answer: the Microsoft access token and, when the scope
/// carried `offline_access`, the long-lived refresh token.
#[derive(Clone)]
pub struct MsaToken {
    /// The Microsoft access token (feeds the Xbox chain, never logged).
    pub access_token: String,
    /// The long-lived credential (feeds silent refresh, never logged).
    pub refresh_token: Option<String>,
    /// Seconds until the access token expires.
    pub expires_in: u64,
}

/// An Xbox Live token with its user hash.
#[derive(Clone)]
pub struct XboxToken {
    /// The XBL token (feeds XSTS, never logged).
    pub token: String,
    /// The `DisplayClaims.xui[0].uhs` user hash (a user id, safe to show).
    pub user_hash: String,
}

/// An XSTS token with its user hash.
#[derive(Clone)]
pub struct XstsToken {
    /// The XSTS token (feeds `login_with_xbox`, never logged).
    pub token: String,
    /// The repeated `uhs` user hash (a user id, safe to show).
    pub user_hash: String,
}

/// A Minecraft profile: the player identity. `uuid_hex` is the 32-char
/// no-dashes form the profile endpoint returns (survey §3.6); the dashed
/// conversion Login Success consumes lives in [`uuid_with_dashes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McProfile {
    /// The player uuid without dashes (32 hex digits).
    pub uuid_hex: String,
    /// The in-game name.
    pub name: String,
}

/// The Minecraft-store entitlements: the `/entitlements/mcstore` `items`
/// array's `name` fields (survey §3.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entitlements {
    /// True iff one of the owning items is present.
    ///
    /// Derivation (recorded): the survey's owned-account response carries
    /// `product_minecraft`, `game_minecraft` (plus Bedrock/Dungeons
    /// equivalents when present) — the Java-ownership signal is the
    /// presence of `product_minecraft` **or** `game_minecraft`.
    pub owns_minecraft: bool,
    /// Every item `name` the endpoint listed (ownership-irrelevant extras
    /// ride along untouched).
    pub items: Vec<String>,
}

/// Converts a no-dashes profile uuid into the dashed form Login Success
/// consumes (`11111111222233334444555555555555` →
/// `11111111-2222-3333-4444-555555555555`; the survey's join-stage tail,
/// §3.7).
///
/// Validation rule (recorded): the input must be exactly 32 ASCII hex
/// digits — anything else (short, long, dashed, non-hex, empty) yields
/// `None`. Never a panic, never an invented uuid: hostile input is refused,
/// not repaired.
pub fn uuid_with_dashes(uuid_hex: &str) -> Option<String> {
    if uuid_hex.len() != 32 || !uuid_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}-{}-{}",
        &uuid_hex[0..8],
        &uuid_hex[8..12],
        &uuid_hex[12..16],
        &uuid_hex[16..20],
        &uuid_hex[20..32]
    ))
}

/// The account-ready record: everything the launcher stores (the refresh
/// half) and everything the client plays with (the token plus identity).
#[derive(Clone)]
pub struct SignedIn {
    /// The Minecraft access token (24 h, never logged).
    pub mc_token: MsaToken,
    /// The player identity from the profile endpoint (never the login's
    /// `username` field — §3.4 notes that is not the uuid).
    pub profile: McProfile,
    /// The ownership record; the driver only builds this when the account
    /// owns the game.
    pub entitlements: Entitlements,
    /// The MSA refresh half to persist (the keyring credential, never
    /// logged): the presented token for [`Auth::complete_sign_in`], the
    /// freshly minted one for [`Auth::silent_sign_in`].
    pub refreshed_msa_refresh_token: Option<String>,
}

impl std::fmt::Debug for SignedIn {
    /// Redacts both token halves; the identity and ownership stay visible.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedIn")
            .field("mc_token", &self.mc_token)
            .field("profile", &self.profile)
            .field("entitlements", &self.entitlements)
            .field(
                "refreshed_msa_refresh_token",
                &self
                    .refreshed_msa_refresh_token
                    .as_ref()
                    .map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl std::fmt::Debug for DeviceCode {
    /// Redacts the polling credential; the user-facing halves stay visible.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("device_code", &"<redacted>")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .field("message", &self.message)
            .finish()
    }
}

impl std::fmt::Debug for MsaToken {
    /// Redacts both credentials; only the lifetime stays visible.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MsaToken")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

impl std::fmt::Debug for XboxToken {
    /// Redacts the token; the user hash is an identifier, safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XboxToken")
            .field("token", &"<redacted>")
            .field("user_hash", &self.user_hash)
            .finish()
    }
}

impl std::fmt::Debug for XstsToken {
    /// Redacts the token; the user hash is an identifier, safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XstsToken")
            .field("token", &"<redacted>")
            .field("user_hash", &self.user_hash)
            .finish()
    }
}

/// Which chain step a failure belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStep {
    /// The device-code request.
    DeviceCode,
    /// The token poll (or refresh).
    Token,
    /// The Xbox Live user authenticate.
    XboxLive,
    /// The XSTS authorize.
    Xsts,
    /// The Minecraft login-with-Xbox (survey §3.4).
    MinecraftLogin,
    /// The entitlements check (survey §3.5).
    Entitlements,
    /// The profile fetch (survey §3.5/§3.6).
    Profile,
}

impl std::fmt::Display for AuthStep {
    /// Renders the step name used inside error messages.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthStep::DeviceCode => write!(f, "device-code"),
            AuthStep::Token => write!(f, "token"),
            AuthStep::XboxLive => write!(f, "Xbox Live"),
            AuthStep::Xsts => write!(f, "XSTS"),
            AuthStep::MinecraftLogin => write!(f, "Minecraft login"),
            AuthStep::Entitlements => write!(f, "entitlements"),
            AuthStep::Profile => write!(f, "profile"),
        }
    }
}

/// Every way the first half of the sign-in chain can fail. No variant
/// carries a token: the chain never places a credential into an error, and
/// the `Status` body is whatever the server answered (capped, lossy UTF-8).
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// A transport failure from the HTTP layer.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// A non-success status. The message reports the step, the code and the
    /// body excerpt — or, for the survey's empty-body XBL 401 (§3.2), the
    /// status plus the `x-xblcorrelationid` the transport forwarded (or a
    /// pointer at that response header when the server sent none), which is
    /// the only diagnostic an empty 401 leaves behind.
    #[error("{step} request failed{}", display_status(*code, body, correlation_id.as_deref()))]
    Status {
        /// The step that failed.
        step: AuthStep,
        /// The HTTP status code.
        code: u16,
        /// The response body, lossy UTF-8, capped at
        /// [`MAX_ERROR_BODY_BYTES`].
        body: String,
        /// The response's `x-xblcorrelationid`, forwarded from
        /// [`HttpError::Status`]; `None` when the server sent no such header.
        correlation_id: Option<String>,
    },
    /// A terminal device-flow code from the token endpoint
    /// (`expired_token`, `access_denied`, …): the raw `error` string.
    #[error("the device-code flow ended: {error}")]
    DeviceFlow {
        /// The token endpoint's `error` code.
        error: String,
    },
    /// An XSTS refusal from the survey's XErr table (§3.3).
    #[error("XSTS refused (XErr {xerr}): {message}{}", display_redirect(redirect))]
    Xsts {
        /// The `XErr` code.
        xerr: u32,
        /// The user-facing sentence for the code.
        message: String,
        /// The `Redirect` URL when the server sent one.
        redirect: Option<String>,
    },
    /// A success-shaped answer that does not parse (missing `Token`,
    /// missing `uhs`, non-JSON garbage): never a panic.
    #[error("{step} answer had an unexpected shape: {message}")]
    Shape {
        /// The step whose answer misbehaved.
        step: AuthStep,
        /// What was missing or unparsable (never a credential).
        message: String,
    },
}

/// Renders the tail of a [`AuthError::Status`] message: the body excerpt (plus
/// the forwarded correlation id when the transport captured one), or the
/// empty-body note — carrying the captured id when present, else pointing at
/// the correlation header.
fn display_status(code: u16, body: &str, correlation_id: Option<&str>) -> String {
    match (body.is_empty(), correlation_id) {
        (true, Some(id)) => {
            format!(" with HTTP {code} and an empty body (x-xblcorrelationid: {id})")
        }
        (true, None) => format!(
            " with HTTP {code} and an empty body (see the x-xblcorrelationid response header)"
        ),
        (false, Some(id)) => format!(" with HTTP {code} (x-xblcorrelationid: {id}): {body}"),
        (false, None) => format!(" with HTTP {code}: {body}"),
    }
}

/// Renders the redirect tail of an [`AuthError::Xsts`] message.
fn display_redirect(redirect: &Option<String>) -> String {
    redirect
        .as_deref()
        .map_or_else(String::new, |url| format!(" (see {url})"))
}

/// Every way the session-server join can fail. Each variant renders the
/// operator-facing sentence for the disconnect screen; no variant carries a
/// token (the join request's halves stay out of every message).
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    /// A transport failure from the HTTP layer.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// The survey-verbatim `{"error":"ForbiddenOperationException"}` shape
    /// (survey §3.7's live probe).
    #[error("the session server refused the join (ForbiddenOperationException)")]
    ForbiddenOperation,
    /// The `{"error":"InsufficientPrivilegesException"}` shape. No server
    /// message exists to carry, so the operator sentence renders from the
    /// error name plus the survey's `// Xbox multiplayer disabled`
    /// annotation (§3.7).
    #[error("InsufficientPrivilegesException: Xbox multiplayer is disabled for this account")]
    InsufficientPrivileges,
    /// The `UserBannedException` shape with its `errorMessage`.
    #[error("the account is banned from multiplayer (UserBannedException): {message}")]
    Banned {
        /// The server's `errorMessage`.
        message: String,
    },
    /// A join refusal that is neither a documented shape nor JSON at all
    /// (never a panic; never a token).
    #[error("the join answer had an unexpected shape: {message}")]
    Shape {
        /// What was missing or unparsable (never a credential).
        message: String,
    },
}

/// The chain driver: an HTTP client, the endpoints and the Azure client id.
pub struct Auth {
    http: Arc<dyn HttpClient>,
    endpoints: AuthEndpoints,
    client_id: String,
}

impl Auth {
    /// Builds the chain over `http` with `endpoints` and `client_id`.
    pub fn new(http: Arc<dyn HttpClient>, endpoints: AuthEndpoints, client_id: String) -> Self {
        Self {
            http,
            endpoints,
            client_id,
        }
    }

    /// Starts the device-code flow: POSTs the form
    /// `client_id=…&scope=XboxLive.signin offline_access` — `offline_access`
    /// is what yields the refresh token (§3.1) — and parses Microsoft
    /// Learn's field set verbatim.
    pub fn begin_device_code(&self) -> Result<DeviceCode, AuthError> {
        let body = self.http.post_form(
            &self.endpoints.device_code,
            &[
                ("client_id", self.client_id.as_str()),
                ("scope", "XboxLive.signin offline_access"),
            ],
        );
        let body = map_http(body, AuthStep::DeviceCode)?;
        let answer: DeviceCodeAnswer =
            serde_json::from_slice(&body).map_err(|error| AuthError::Shape {
                step: AuthStep::DeviceCode,
                message: format!("the device-code answer does not parse: {error}"),
            })?;
        Ok(answer.into_device_code())
    }

    /// Polls the token endpoint until the user approves, the code expires,
    /// or the flow fails: POSTs the verbatim device-code grant
    /// `grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id=…&device_code=…`.
    /// A 400 whose `error` is `authorization_pending` waits `interval` and
    /// retries; `slow_down` waits the bumped interval (see
    /// [`SLOW_DOWN_BUMP_SECS`]); `expired_token` — or the injected waits
    /// overrunning the device code's `expires_in` (the survey's 15-minute
    /// window, §3.1) — ends in [`AuthError::DeviceFlow`]. Success parses
    /// `access_token` / `refresh_token` / `expires_in`. Every wait goes
    /// through `sleep`, so tests record waits without sleeping.
    pub fn poll_token(
        &self,
        code: &DeviceCode,
        sleep: &mut dyn FnMut(Duration),
    ) -> Result<MsaToken, AuthError> {
        let mut interval = Duration::from_secs(code.interval);
        let mut waited = Duration::ZERO;
        let window = Duration::from_secs(code.expires_in);
        loop {
            let poll = self.http.post_form(
                &self.endpoints.token,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", self.client_id.as_str()),
                    ("device_code", code.device_code.as_str()),
                ],
            );
            match poll {
                Ok(body) => {
                    let answer: TokenAnswer =
                        serde_json::from_slice(&body).map_err(|error| AuthError::Shape {
                            step: AuthStep::Token,
                            message: format!("the token answer does not parse: {error}"),
                        })?;
                    return Ok(answer.into_token());
                }
                Err(HttpError::Status {
                    code: 400, body, ..
                }) => {
                    let failure: TokenFailure =
                        serde_json::from_slice(&body).map_err(|_| AuthError::Shape {
                            step: AuthStep::Token,
                            message: "the token refusal does not parse".to_string(),
                        })?;
                    match failure.error.as_str() {
                        "authorization_pending" => {
                            wait(sleep, &mut waited, interval, window)?;
                        }
                        "slow_down" => {
                            interval += Duration::from_secs(SLOW_DOWN_BUMP_SECS);
                            wait(sleep, &mut waited, interval, window)?;
                        }
                        other => {
                            return Err(AuthError::DeviceFlow {
                                error: other.to_string(),
                            });
                        }
                    }
                }
                Err(HttpError::Status {
                    code,
                    body,
                    correlation_id,
                    ..
                }) => {
                    return Err(AuthError::Status {
                        step: AuthStep::Token,
                        code,
                        body: capped_lossy(&body),
                        correlation_id,
                    });
                }
                Err(other) => return Err(AuthError::Http(other)),
            }
        }
    }

    /// Silently refreshes the Microsoft access token: POSTs the verbatim
    /// refresh grant `grant_type=refresh_token&refresh_token=…&client_id=…`.
    /// No Xbox/Minecraft state is needed to refresh — a launcher can refresh
    /// on every start (§3.1).
    pub fn refresh_msa(&self, refresh_token: &str) -> Result<MsaToken, AuthError> {
        let body = self.http.post_form(
            &self.endpoints.token,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", self.client_id.as_str()),
            ],
        );
        let body = match body {
            Ok(body) => body,
            Err(HttpError::Status {
                code: 400, body, ..
            }) => {
                let failure: TokenFailure =
                    serde_json::from_slice(&body).map_err(|_| AuthError::Shape {
                        step: AuthStep::Token,
                        message: "the refresh refusal does not parse".to_string(),
                    })?;
                return Err(AuthError::DeviceFlow {
                    error: failure.error,
                });
            }
            Err(HttpError::Status {
                code,
                body,
                correlation_id,
                ..
            }) => {
                return Err(AuthError::Status {
                    step: AuthStep::Token,
                    code,
                    body: capped_lossy(&body),
                    correlation_id,
                });
            }
            Err(other) => return Err(AuthError::Http(other)),
        };
        let answer: TokenAnswer =
            serde_json::from_slice(&body).map_err(|error| AuthError::Shape {
                step: AuthStep::Token,
                message: format!("the refresh answer does not parse: {error}"),
            })?;
        Ok(answer.into_token())
    }

    /// Authenticates with Xbox Live: POSTs the exact §3.2 JSON body — the
    /// `d=` ticket prefix is mandatory ("make sure that it is prefixed with
    /// 'd='") — and parses `Token` plus `DisplayClaims.xui[0].uhs`. A bad
    /// ticket answers 401 with an empty body (§3.2), which becomes
    /// [`AuthError::Status`] naming the status and the correlation header.
    pub fn authenticate_xbox(&self, msa_access_token: &str) -> Result<XboxToken, AuthError> {
        let ticket =
            serde_json::to_string(&format!("d={msa_access_token}")).expect("a string serializes");
        // The key order is the survey's order (§3.2 verbatim); serde_json
        // maps would sort the keys, so the body is laid out by hand.
        let body = format!(
            "{{\"Properties\":{{\"AuthMethod\":\"RPS\",\"SiteName\":\"user.auth.xboxlive.com\",\"RpsTicket\":{ticket}}},\"RelyingParty\":\"http://auth.xboxlive.com\",\"TokenType\":\"JWT\"}}"
        );
        let answer = self
            .http
            .post_json(&self.endpoints.xbl_authenticate, body.as_bytes());
        let answer = map_http(answer, AuthStep::XboxLive)?;
        let claims: XboxAnswer =
            serde_json::from_slice(&answer).map_err(|error| AuthError::Shape {
                step: AuthStep::XboxLive,
                message: format!("the Xbox Live answer does not parse: {error}"),
            })?;
        claims.into_token(AuthStep::XboxLive)
    }

    /// Authorizes with XSTS: POSTs the exact §3.3 JSON body — the `rp://`
    /// scheme in the relying party is the exact string — and parses `Token`
    /// plus the same `uhs`. A 400 carrying an `XErr` document becomes the
    /// table-driven [`AuthError::Xsts`]; any other failure becomes
    /// [`AuthError::Status`].
    pub fn authorize_xsts(&self, xbl: &XboxToken) -> Result<XstsToken, AuthError> {
        let token = serde_json::to_string(&xbl.token).expect("a string serializes");
        // The key order is the survey's order (§3.3 verbatim); serde_json
        // maps would sort the keys, so the body is laid out by hand.
        let body = format!(
            "{{\"Properties\":{{\"SandboxId\":\"RETAIL\",\"UserTokens\":[{token}]}},\"RelyingParty\":\"rp://api.minecraftservices.com/\",\"TokenType\":\"JWT\"}}"
        );
        let answer = self
            .http
            .post_json(&self.endpoints.xsts_authorize, body.as_bytes());
        let answer = match answer {
            Ok(body) => body,
            Err(HttpError::Status {
                code: 400,
                body,
                correlation_id,
                ..
            }) => {
                if let Ok(failure) = serde_json::from_slice::<XstsFailure>(&body) {
                    return Err(failure.into_error());
                }
                return Err(AuthError::Status {
                    step: AuthStep::Xsts,
                    code: 400,
                    body: capped_lossy(&body),
                    correlation_id,
                });
            }
            Err(HttpError::Status {
                code,
                body,
                correlation_id,
                ..
            }) => {
                return Err(AuthError::Status {
                    step: AuthStep::Xsts,
                    code,
                    body: capped_lossy(&body),
                    correlation_id,
                });
            }
            Err(other) => return Err(AuthError::Http(other)),
        };
        let claims: XboxAnswer =
            serde_json::from_slice(&answer).map_err(|error| AuthError::Shape {
                step: AuthStep::Xsts,
                message: format!("the XSTS answer does not parse: {error}"),
            })?;
        let token = claims.into_token(AuthStep::Xsts)?;
        Ok(XstsToken {
            token: token.token,
            user_hash: token.user_hash,
        })
    }

    /// Logs in with Xbox: POSTs the exact §3.4 JSON body — literal
    /// `XBL3.0 x=`, the `uhs` hash, a `;`, then the XSTS token — and parses
    /// `access_token` plus `expires_in` (86400 = 24 h) into the Minecraft
    /// token (which carries no refresh half).
    ///
    /// Note (§3.4): the response's `username` field "is not the uuid" — it
    /// is ignored here; the profile fetch ([`Auth::fetch_profile`]) is the
    /// identity source.
    pub fn login_with_xbox(&self, uhs: &str, xsts_token: &str) -> Result<MsaToken, AuthError> {
        let identity = format!("XBL3.0 x={uhs};{xsts_token}");
        let body = serde_json::to_string(&LoginRequest {
            identity_token: identity.as_str(),
        })
        .expect("a string map serializes");
        let answer = self
            .http
            .post_json(&self.endpoints.mc_login, body.as_bytes());
        let answer = map_http(answer, AuthStep::MinecraftLogin)?;
        let login: LoginAnswer =
            serde_json::from_slice(&answer).map_err(|error| AuthError::Shape {
                step: AuthStep::MinecraftLogin,
                message: format!("the Minecraft login answer does not parse: {error}"),
            })?;
        Ok(MsaToken {
            access_token: login.access_token,
            refresh_token: None,
            expires_in: login.expires_in,
        })
    }

    /// Checks Minecraft ownership: GETs the entitlements endpoint with the
    /// Minecraft access token as the bearer (`Authorization: Bearer …` — the
    /// space after `Bearer` is mandatory, §3.5) and parses the `items`
    /// array's `name` fields.
    ///
    /// This reports; it does not refuse — an empty `items` array parses to
    /// `owns_minecraft: false`, and the driver ([`Auth::complete_sign_in`])
    /// turns that into the no-ownership error.
    pub fn fetch_entitlements(&self, mc_access_token: &str) -> Result<Entitlements, AuthError> {
        let answer = self
            .http
            .get_with_bearer(&self.endpoints.mc_entitlements, mc_access_token);
        let answer = map_http(answer, AuthStep::Entitlements)?;
        let entitlements: EntitlementsAnswer =
            serde_json::from_slice(&answer).map_err(|error| AuthError::Shape {
                step: AuthStep::Entitlements,
                message: format!("the entitlements answer does not parse: {error}"),
            })?;
        Ok(entitlements.into_entitlements())
    }

    /// Fetches the player identity: GETs the profile endpoint with the
    /// Minecraft access token as the bearer and parses `id` (the uuid
    /// without dashes) and `name`; unknown extra fields are ignored (the
    /// §3.6 `profileActions` lesson). A malformed `id` is a [`AuthError::Shape`].
    pub fn fetch_profile(&self, mc_access_token: &str) -> Result<McProfile, AuthError> {
        let answer = self
            .http
            .get_with_bearer(&self.endpoints.mc_profile, mc_access_token);
        let answer = map_http(answer, AuthStep::Profile)?;
        let profile: ProfileAnswer =
            serde_json::from_slice(&answer).map_err(|error| AuthError::Shape {
                step: AuthStep::Profile,
                message: format!("the profile answer does not parse: {error}"),
            })?;
        profile.into_profile()
    }

    /// Runs the second half of the sign-in chain: Xbox Live → XSTS → the
    /// Minecraft login → the entitlements check (refusing when the account
    /// owns no copy — survey §3.4's crux: "Everything until here works with
    /// a normal Microsoft account!") → the profile fetch, in that order.
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use oxide_assets::auth::{Auth, AuthEndpoints, MsaToken, client_id_from_env};
    /// use oxide_assets::http::UreqClient;
    ///
    /// let auth = Auth::new(
    ///     Arc::new(UreqClient::new()),
    ///     AuthEndpoints::default(),
    ///     client_id_from_env(),
    /// );
    /// // Against the live service (Task 13's leg) this runs the whole
    /// // second half off an already-polled Microsoft token:
    /// let msa = MsaToken {
    ///     access_token: "microsoft-access-token".to_string(),
    ///     refresh_token: Some("microsoft-refresh-token".to_string()),
    ///     expires_in: 3599,
    /// };
    /// let signed_in = auth.complete_sign_in(msa).expect("live sign-in");
    /// assert!(!signed_in.profile.name.is_empty());
    /// ```
    pub fn complete_sign_in(&self, msa: MsaToken) -> Result<SignedIn, AuthError> {
        let xbl = self.authenticate_xbox(&msa.access_token)?;
        let xsts = self.authorize_xsts(&xbl)?;
        let mc = self.login_with_xbox(&xsts.user_hash, &xsts.token)?;
        let entitlements = self.fetch_entitlements(&mc.access_token)?;
        // Refusal variant (recorded): the existing taxonomy has no
        // no-ownership variant, so the empty (or ownership-less)
        // entitlements list is an `AuthError::Shape` on the entitlements
        // step — a clear typed error, never a silent unsigned-in record.
        if !entitlements.owns_minecraft {
            return Err(AuthError::Shape {
                step: AuthStep::Entitlements,
                message: "the account does not own Minecraft (no product_minecraft/game_minecraft entitlement)".to_string(),
            });
        }
        let profile = self.fetch_profile(&mc.access_token)?;
        Ok(SignedIn {
            mc_token: mc,
            profile,
            entitlements,
            refreshed_msa_refresh_token: msa.refresh_token,
        })
    }

    /// The whole client-start path: silently refreshes the Microsoft access
    /// token, then runs [`Auth::complete_sign_in`]. Silent refresh needs no
    /// Xbox/Minecraft state — a launcher can refresh on every start (§3.1).
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use oxide_assets::auth::{Auth, AuthEndpoints, client_id_from_env};
    /// use oxide_assets::http::UreqClient;
    ///
    /// let auth = Auth::new(
    ///     Arc::new(UreqClient::new()),
    ///     AuthEndpoints::default(),
    ///     client_id_from_env(),
    /// );
    /// // Against the live service (Task 13's leg) this signs in off the
    /// // stored refresh half alone:
    /// let signed_in = auth
    ///     .silent_sign_in("stored-refresh-token")
    ///     .expect("live sign-in");
    /// assert!(!signed_in.profile.name.is_empty());
    /// ```
    pub fn silent_sign_in(&self, refresh_token: &str) -> Result<SignedIn, AuthError> {
        let msa = self.refresh_msa(refresh_token)?;
        self.complete_sign_in(msa)
    }

    /// Proves ownership to a server: POSTs the verbatim §3.7 JSON body —
    /// `{"accessToken":"…","selectedProfile":"<uuid without dashes>",
    /// "serverId":"<hash>"}` — to the session server. The server answers
    /// **204 No Content** with an empty body on success, which arrives here
    /// as the Ok path. `Content-Type: application/json` is mandatory ("you
    /// *must* have the Content-Type header set to application/json or you
    /// will get a 415 … or 403", §3.7) — the production client sets it on
    /// every JSON post.
    ///
    /// A malformed `profile_uuid_hex` (not 32 hex digits — see
    /// [`uuid_with_dashes`]) never reaches the wire: it is a
    /// [`JoinError::Shape`].
    pub fn join_session(
        &self,
        profile_uuid_hex: &str,
        access_token: &str,
        server_id: &str,
    ) -> Result<(), JoinError> {
        if uuid_with_dashes(profile_uuid_hex).is_none() {
            return Err(JoinError::Shape {
                message: "the profile uuid is not 32 hex digits".to_string(),
            });
        }
        let body = serde_json::to_string(&JoinRequest {
            access_token,
            selected_profile: profile_uuid_hex,
            server_id,
        })
        .expect("a string map serializes");
        match self
            .http
            .post_json(&self.endpoints.session_join, body.as_bytes())
        {
            Ok(_) => Ok(()),
            Err(HttpError::Status { code, body, .. }) => Err(join_status_error(code, &body)),
            Err(other) => Err(JoinError::Http(other)),
        }
    }
}

/// One poll wait: ends the flow when the wait would overrun the device
/// code's `expires_in` (the survey's 15-minute window, §3.1); otherwise
/// records the wait through `sleep` and extends the waited total. The total
/// accumulates the injected waits, so no clock is needed and tests stay
/// deterministic.
fn wait(
    sleep: &mut dyn FnMut(Duration),
    waited: &mut Duration,
    interval: Duration,
    window: Duration,
) -> Result<(), AuthError> {
    if *waited + interval > window {
        return Err(AuthError::DeviceFlow {
            error: "expired_token".to_string(),
        });
    }
    sleep(interval);
    *waited += interval;
    Ok(())
}

/// Maps an HTTP result onto the chain's errors: non-success statuses become
/// [`AuthError::Status`] carrying the step, the code and the capped,
/// lossy-UTF-8 body; anything else passes through as [`AuthError::Http`].
fn map_http(result: Result<Vec<u8>, HttpError>, step: AuthStep) -> Result<Vec<u8>, AuthError> {
    match result {
        Ok(body) => Ok(body),
        Err(HttpError::Status {
            code,
            body,
            correlation_id,
            ..
        }) => Err(AuthError::Status {
            step,
            code,
            body: capped_lossy(&body),
            correlation_id,
        }),
        Err(other) => Err(AuthError::Http(other)),
    }
}

/// Lossy-UTF-8 with the HTTP error-body cap applied on a char boundary.
fn capped_lossy(body: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(body).into_owned();
    if text.len() > MAX_ERROR_BODY_BYTES {
        let mut end = MAX_ERROR_BODY_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// Accepts a JSON number or a numeric string as a `u64`: the Entra token
/// endpoint answers `expires_in` as a string (`"3599"`), the device-code
/// endpoint as a number — both shapes parse to the same value.
fn de_u64<'de, D: Deserializer<'de>>(lens: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        /// A plain JSON number.
        Plain(u64),
        /// A numeric JSON string.
        Quoted(String),
    }
    match Number::deserialize(lens)? {
        Number::Plain(value) => Ok(value),
        Number::Quoted(text) => text.parse().map_err(serde::de::Error::custom),
    }
}

/// The device-code answer as Microsoft Learn documents it (survey §3.1):
/// unknown extra fields are ignored (the §3.6 `profileActions` lesson).
#[derive(Deserialize)]
struct DeviceCodeAnswer {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(deserialize_with = "de_u64")]
    expires_in: u64,
    #[serde(deserialize_with = "de_u64")]
    interval: u64,
    message: String,
}

impl DeviceCodeAnswer {
    /// Splits the wire answer into the public device-code record.
    fn into_device_code(self) -> DeviceCode {
        DeviceCode {
            device_code: self.device_code,
            user_code: self.user_code,
            verification_uri: self.verification_uri,
            expires_in: self.expires_in,
            interval: self.interval,
            message: self.message,
        }
    }
}

/// The token answer: `access_token` and `expires_in` are required, the
/// refresh token rides along only when the scope asked for it.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(deserialize_with = "de_u64")]
    expires_in: u64,
}

impl TokenAnswer {
    /// Splits the wire answer into the public token record.
    fn into_token(self) -> MsaToken {
        MsaToken {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_in: self.expires_in,
        }
    }
}

/// A token-endpoint refusal: the `error` code drives the poll state machine.
#[derive(Deserialize)]
struct TokenFailure {
    error: String,
}

/// An XSTS refusal document: the `XErr` table row shape from survey §3.3.
/// `Message` defaults to empty and `Redirect` to absent when the server
/// omits them.
#[derive(Deserialize)]
struct XstsFailure {
    /// The `XErr` code.
    #[serde(rename = "XErr")]
    xerr: u32,
    /// The server's message (empty in the surveyed rows).
    #[serde(rename = "Message", default)]
    message: String,
    /// The redirect URL (present on the child-account row).
    #[serde(rename = "Redirect", default)]
    redirect: Option<String>,
}

impl XstsFailure {
    /// Builds the table-driven [`AuthError::Xsts`] for the refusal.
    fn into_error(self) -> AuthError {
        AuthError::Xsts {
            message: xsts_message(self.xerr, &self.message),
            redirect: self.redirect,
            xerr: self.xerr,
        }
    }
}

/// The XErr table, survey §3.3's texts verbatim. Unknown codes render a
/// generic message carrying the code (plus the server's message when it
/// sent one).
fn xsts_message(xerr: u32, server_message: &str) -> String {
    match xerr {
        2148916227 => "The account is banned from Xbox.".to_string(),
        2148916233 => "The account does not have an Xbox account. Sign up for one (or sign in through minecraft.net to create one), then proceed with the login.".to_string(),
        2148916235 => {
            "The account is from a country where Xbox Live is not available or is banned.".to_string()
        }
        2148916236 | 2148916237 => {
            "The account needs adult verification on the Xbox page. (South Korea)".to_string()
        }
        2148916238 => "The account is a child (under 18) and cannot proceed unless the account is added to a Family by an adult. This only seems to occur when using a custom Microsoft Azure application.".to_string(),
        2148916262 => {
            "An XSTS failure that happens rarely, with no additional information.".to_string()
        }
        unknown => {
            if server_message.is_empty() {
                format!("Unknown XSTS error {unknown}.")
            } else {
                format!("Unknown XSTS error {unknown}: {server_message}")
            }
        }
    }
}

/// An Xbox/XSTS success answer: `Token` plus `DisplayClaims.xui[0].uhs`
/// (survey §3.2/§3.3); unknown extra fields are ignored.
#[derive(Deserialize)]
struct XboxAnswer {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    claims: DisplayClaims,
}

/// The display claims wrapping the user-hash list.
#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

/// One user-info claim carrying the user hash.
#[derive(Deserialize)]
struct XuiClaim {
    uhs: String,
}

impl XboxAnswer {
    /// Splits the wire answer into the token plus user hash, or reports the
    /// missing half as a [`AuthError::Shape`].
    fn into_token(self, step: AuthStep) -> Result<XboxToken, AuthError> {
        let Some(claim) = self.claims.xui.first() else {
            return Err(AuthError::Shape {
                step,
                message: "the answer carries no xui user hash".to_string(),
            });
        };
        Ok(XboxToken {
            token: self.token,
            user_hash: claim.uhs.clone(),
        })
    }
}

/// The login_with_xbox request: the single `identityToken` carrying the
/// exact `XBL3.0 x=<uhs>;<xsts>` concatenation (survey §3.4). Serialized by
/// field (never by hand) so credential escaping stays correct.
#[derive(Serialize)]
struct LoginRequest<'a> {
    #[serde(rename = "identityToken")]
    identity_token: &'a str,
}

/// The login_with_xbox answer: `access_token` and `expires_in` are the
/// halves the chain keeps; `username`, `roles` and `token_type` are ignored
/// (§3.4: the `username` "is not the uuid" — the profile fetch is the
/// identity source).
#[derive(Deserialize)]
struct LoginAnswer {
    access_token: String,
    #[serde(deserialize_with = "de_u64")]
    expires_in: u64,
}

/// The entitlements answer: the `items` array whose `name` fields carry the
/// ownership signal (survey §3.5); unknown extra fields are ignored.
#[derive(Deserialize)]
struct EntitlementsAnswer {
    items: Vec<EntitlementItem>,
}

/// One entitlement item: only `name` is read (`signature` and any future
/// fields ride along ignored).
#[derive(Deserialize)]
struct EntitlementItem {
    name: String,
}

impl EntitlementsAnswer {
    /// Splits the wire answer into the ownership record. `owns_minecraft`
    /// is true iff `product_minecraft` or `game_minecraft` is present — the
    /// recorded Java-ownership derivation (survey §3.5).
    fn into_entitlements(self) -> Entitlements {
        let items: Vec<String> = self.items.into_iter().map(|item| item.name).collect();
        let owns_minecraft = items
            .iter()
            .any(|name| name == "product_minecraft" || name == "game_minecraft");
        Entitlements {
            owns_minecraft,
            items,
        }
    }
}

/// The profile answer: `id` (the uuid without dashes) and `name` (survey
/// §3.5/§3.6); unknown extra fields are ignored.
#[derive(Deserialize)]
struct ProfileAnswer {
    id: String,
    name: String,
}

impl ProfileAnswer {
    /// Splits the wire answer into the player identity. A malformed `id`
    /// (not 32 hex digits — see [`uuid_with_dashes`]) is a shape error:
    /// [`McProfile`] guarantees the no-dashes form, so a hostile answer is
    /// refused, never stored.
    fn into_profile(self) -> Result<McProfile, AuthError> {
        if uuid_with_dashes(&self.id).is_none() {
            return Err(AuthError::Shape {
                step: AuthStep::Profile,
                message: "the profile id is not 32 hex digits".to_string(),
            });
        }
        Ok(McProfile {
            uuid_hex: self.id,
            name: self.name,
        })
    }
}

/// The session join request: the verbatim §3.7 shape. Field order is the
/// survey's order (`accessToken`, `selectedProfile`, `serverId`); struct
/// serialization preserves declaration order.
#[derive(Serialize)]
struct JoinRequest<'a> {
    #[serde(rename = "accessToken")]
    access_token: &'a str,
    #[serde(rename = "selectedProfile")]
    selected_profile: &'a str,
    #[serde(rename = "serverId")]
    server_id: &'a str,
}

/// A session-join refusal document: the `error` name drives the typed
/// mapping, `errorMessage` rides along only for bans.
#[derive(Deserialize)]
struct JoinFailure {
    error: String,
    #[serde(rename = "errorMessage", default)]
    error_message: String,
}

/// Maps a non-success join status onto the typed [`JoinError`]: the three
/// documented `error` names become their variants, anything else (an
/// unknown name, or a body that is not JSON at all) becomes
/// [`JoinError::Shape`] — never a panic, never a token.
fn join_status_error(code: u16, body: &[u8]) -> JoinError {
    match serde_json::from_slice::<JoinFailure>(body) {
        Ok(failure) => match failure.error.as_str() {
            "ForbiddenOperationException" => JoinError::ForbiddenOperation,
            "InsufficientPrivilegesException" => JoinError::InsufficientPrivileges,
            "UserBannedException" => JoinError::Banned {
                message: failure.error_message,
            },
            other => JoinError::Shape {
                message: format!("the join refusal carries an unknown error: {other}"),
            },
        },
        Err(_) => {
            let excerpt = capped_lossy(body);
            let message = if excerpt.is_empty() {
                format!("the join refusal does not parse (HTTP {code})")
            } else {
                format!("the join refusal does not parse (HTTP {code}): {excerpt}")
            };
            JoinError::Shape { message }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Endpoint pins, message pins and credential redaction: the pure unit
    //! half of the auth suite (the stub-endpoint corpus lives in
    //! `tests/auth_chain.rs`).

    use super::{AuthEndpoints, AuthError, AuthStep, DeviceCode, MsaToken, XboxToken, XstsToken};

    #[test]
    fn default_endpoints_carry_the_survey_urls_verbatim() {
        let endpoints = AuthEndpoints::default();

        assert_eq!(
            endpoints.device_code,
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode"
        );
        assert_eq!(
            endpoints.token,
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/token"
        );
        assert_eq!(
            endpoints.xbl_authenticate,
            "https://user.auth.xboxlive.com/user/authenticate"
        );
        assert_eq!(
            endpoints.xsts_authorize,
            "https://xsts.auth.xboxlive.com/xsts/authorize"
        );
        assert_eq!(
            endpoints.mc_login,
            "https://api.minecraftservices.com/authentication/login_with_xbox"
        );
        assert_eq!(
            endpoints.mc_entitlements,
            "https://api.minecraftservices.com/entitlements/mcstore"
        );
        assert_eq!(
            endpoints.mc_profile,
            "https://api.minecraftservices.com/minecraft/profile"
        );
        assert_eq!(
            endpoints.session_join,
            "https://sessionserver.mojang.com/session/minecraft/join"
        );
        for url in [endpoints.device_code, endpoints.token] {
            assert!(
                url.contains("/consumers/"),
                "the consumers tenant is mandatory (§3.1): {url}"
            );
        }
    }

    #[test]
    fn step_names_render_for_messages() {
        assert_eq!(AuthStep::DeviceCode.to_string(), "device-code");
        assert_eq!(AuthStep::Token.to_string(), "token");
        assert_eq!(AuthStep::XboxLive.to_string(), "Xbox Live");
        assert_eq!(AuthStep::Xsts.to_string(), "XSTS");
    }

    #[test]
    fn stage_two_step_names_render_for_messages() {
        assert_eq!(AuthStep::MinecraftLogin.to_string(), "Minecraft login");
        assert_eq!(AuthStep::Entitlements.to_string(), "entitlements");
        assert_eq!(AuthStep::Profile.to_string(), "profile");
    }

    #[test]
    fn debug_redacts_every_credential_but_keeps_identifiers() {
        let code = DeviceCode {
            device_code: "SENTINEL-DEVICE".to_string(),
            user_code: "A1B2-C3D4".to_string(),
            verification_uri: "https://www.microsoft.com/link".to_string(),
            expires_in: 900,
            interval: 5,
            message: "instructions".to_string(),
        };
        let msa = MsaToken {
            access_token: "SENTINEL-MSA".to_string(),
            refresh_token: Some("SENTINEL-REFRESH".to_string()),
            expires_in: 3599,
        };
        let xbl = XboxToken {
            token: "SENTINEL-XBL".to_string(),
            user_hash: "SYN-UHS-001".to_string(),
        };
        let xsts = XstsToken {
            token: "SENTINEL-XSTS".to_string(),
            user_hash: "SYN-UHS-001".to_string(),
        };

        for debug in [
            format!("{code:?}"),
            format!("{msa:?}"),
            format!("{xbl:?}"),
            format!("{xsts:?}"),
        ] {
            for sentinel in [
                "SENTINEL-DEVICE",
                "SENTINEL-MSA",
                "SENTINEL-REFRESH",
                "SENTINEL-XBL",
                "SENTINEL-XSTS",
            ] {
                assert!(
                    !debug.contains(sentinel),
                    "Debug redacts credentials: {debug}"
                );
            }
        }
        assert!(
            format!("{xbl:?}").contains("SYN-UHS-001"),
            "the user hash stays visible"
        );
    }

    #[test]
    fn status_messages_carry_no_credential_slot() {
        let empty = AuthError::Status {
            step: AuthStep::XboxLive,
            code: 401,
            body: String::new(),
            correlation_id: None,
        };
        assert_eq!(
            empty.to_string(),
            "Xbox Live request failed with HTTP 401 and an empty body (see the x-xblcorrelationid response header)"
        );
        let full = AuthError::Status {
            step: AuthStep::Token,
            code: 400,
            body: "AADSTS70016".to_string(),
            correlation_id: None,
        };
        assert_eq!(
            full.to_string(),
            "token request failed with HTTP 400: AADSTS70016"
        );
    }

    #[test]
    fn status_messages_render_the_correlation_id_when_present() {
        let empty = AuthError::Status {
            step: AuthStep::XboxLive,
            code: 401,
            body: String::new(),
            correlation_id: Some("3f2504e0-0000-0000-0000-000000000000".to_string()),
        };
        let message = empty.to_string();
        assert!(
            message.contains("401"),
            "the status rides in the message: {message}"
        );
        assert!(
            message.contains("3f2504e0-0000-0000-0000-000000000000"),
            "the correlation id rides in the message: {message}"
        );
        assert!(
            message.contains("x-xblcorrelationid"),
            "the header name rides along: {message}"
        );
        let full = AuthError::Status {
            step: AuthStep::Token,
            code: 400,
            body: "AADSTS70016".to_string(),
            correlation_id: Some("3f2504e0-0000-0000-0000-000000000000".to_string()),
        };
        let message = full.to_string();
        assert!(
            message.contains("AADSTS70016"),
            "the body excerpt stays: {message}"
        );
        assert!(
            message.contains("3f2504e0-0000-0000-0000-000000000000"),
            "the correlation id rides in the message: {message}"
        );
    }
    #[test]
    fn xsts_messages_render_the_table_row_and_the_redirect() {
        let plain = AuthError::Xsts {
            xerr: 2148916227,
            message: "The account is banned from Xbox.".to_string(),
            redirect: None,
        };
        assert_eq!(
            plain.to_string(),
            "XSTS refused (XErr 2148916227): The account is banned from Xbox."
        );
        let gated = AuthError::Xsts {
            xerr: 2148916238,
            message: "child".to_string(),
            redirect: Some("https://start.ui.xboxlive.com/AddChildToFamily".to_string()),
        };
        assert!(
            gated
                .to_string()
                .ends_with(" (see https://start.ui.xboxlive.com/AddChildToFamily)"),
            "the redirect rides in the message: {gated}"
        );
    }
}
