//! Status state: the Status Request / Status Response exchange.
//!
//! [`ping_server`] performs the exchange: a Handshake with next state 1, a
//! Status Request, and the server's Status Response. The ping/pong round trip is
//! not implemented here; it is optional and not needed to read a response.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use oxide_proto::frame::{Compression, FrameError, read_frame, write_frame};
use oxide_proto::varint::{VarIntError, read_varint};
use serde::{Deserialize, Serialize};

/// Serverbound Status Request (state: status, id 0x00).
///
/// The packet carries no fields, so its payload is the id byte alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRequest;

impl StatusRequest {
    /// The complete packet payload: the id byte alone.
    pub const PAYLOAD: [u8; 1] = [0x00];
}

/// The `description` field of a status response.
///
/// A formatted MOTD arrives as a chat component object and an unformatted one as
/// a plain JSON string; both forms are accepted. Anything else — an array of
/// chat components, an object whose `text` is not a string — is kept as
/// [`Description::Other`] so the rest of the response still parses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Description {
    /// A chat component object.
    Object {
        /// Legacy colour code prefixed text, when present.
        #[serde(default)]
        text: Option<String>,
    },
    /// A plain JSON string, the form vanilla sends for an unformatted MOTD.
    Plain(String),
    /// A description shape this client reads no text from; it is a legal part of
    /// a status response, so the response around it is still parsed.
    Other(serde_json::Value),
}

impl Description {
    /// The description text, in whichever form it arrived.
    #[must_use]
    pub fn text(self) -> Option<String> {
        match self {
            Description::Object { text } => text,
            Description::Plain(text) => Some(text),
            Description::Other(_) => None,
        }
    }
}

/// A server's status response, as far as Oxidecraft needs it.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    /// Version information the server advertises.
    pub version: VersionInfo,
    /// Connected player counts.
    pub players: Players,
    /// Server description.
    #[serde(default)]
    pub description: Option<Description>,
    /// Favicon, when present.
    #[serde(default)]
    pub favicon: Option<String>,
}

/// The `version` object of a status response.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    /// Human-readable version name, for example `1.8.9`.
    pub name: String,
    /// Protocol number the server speaks.
    pub protocol: i32,
}

/// The `players` object of a status response.
#[derive(Debug, Clone, Deserialize)]
pub struct Players {
    /// Maximum players the server allows.
    pub max: i32,
    /// Players currently online.
    pub online: i32,
}

/// Errors from a status ping.
#[derive(Debug, thiserror::Error)]
pub enum PingError {
    /// Network failure.
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    /// The server did not answer within the timeout.
    #[error("server did not answer within {0:?}")]
    Timeout(Duration),
    /// Framing failure.
    #[error("framing error: {0}")]
    Frame(#[from] FrameError),
    /// The JSON body was not a status response.
    #[error("bad status JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The server closed the connection without answering.
    #[error("connection closed before a status response arrived")]
    Closed,
    /// The server answered with a packet that is not a Status Response.
    #[error("expected a Status Response (0x00), got packet id {0:#04x}")]
    UnexpectedPacket(u8),
    /// The response body ended before the JSON it declared.
    #[error("status response body is truncated")]
    Truncated,
    /// The response's JSON length prefix was malformed.
    #[error("bad status response length prefix: {0}")]
    BadLength(#[from] VarIntError),
}

impl PingError {
    /// Classifies a framing failure: a read that ran into the deadline is a
    /// timeout, a stream that ended before the response arrived is a closed
    /// connection, and anything else is a framing failure.
    fn from_frame(error: FrameError, timeout: Duration) -> Self {
        // The VarInt reader maps end of stream to `UnexpectedEof` itself, so a
        // `VarIntError::Io` only ever carries some other kind. Matching on the
        // kind reaches the error whether it is bare or nested in a VarInt error.
        let kind = match &error {
            FrameError::VarInt(VarIntError::Io(io)) | FrameError::Io(io) => Some(io.kind()),
            _ => None,
        };
        match kind {
            // A read that hits the socket timeout surfaces as `WouldBlock` on
            // Unix and as `TimedOut` on Windows.
            Some(ErrorKind::WouldBlock | ErrorKind::TimedOut) => PingError::Timeout(timeout),
            Some(ErrorKind::UnexpectedEof) => PingError::Closed,
            _ => match error {
                FrameError::VarInt(VarIntError::UnexpectedEof) => PingError::Closed,
                other => PingError::Frame(other),
            },
        }
    }
}

/// Sends a status ping and returns the parsed response.
///
/// The exchange is the handshake with `next_state` 1, a Status Request, and the
/// server's Status Response. The ping/pong round trip is not needed to read the
/// response, and a server that closes straight after it has still answered.
///
/// `timeout` bounds the whole attempt: the connection attempts share it, as does
/// every read and write after them, so a host that never answers cannot hold the
/// caller for longer than that.
pub fn ping_server(host: &str, port: u16, timeout: Duration) -> Result<StatusResponse, PingError> {
    // Try every address the host resolves to, the way `TcpStream::connect`
    // does. They share one deadline: a host that answers on a later address
    // still gets its chance, without the total stretching past the timeout.
    let deadline = Instant::now() + timeout;
    let mut stream = None;
    let mut failure = None;
    for address in (host, port).to_socket_addrs()? {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match TcpStream::connect_timeout(&address, remaining) {
            Ok(connected) => {
                stream = Some(connected);
                break;
            }
            Err(error) => failure = Some(error),
        }
    }
    let Some(mut stream) = stream else {
        return Err(match failure {
            // The resolver produced no address to connect to at all.
            None => PingError::Io(std::io::Error::new(
                ErrorKind::AddrNotAvailable,
                format!("no address resolved for {host}:{port}"),
            )),
            // Every address failed; report the last failure, as
            // `TcpStream::connect` does.
            Some(error) => PingError::Io(error),
        });
    };
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let handshake = crate::handshake::handshake_payload(47, host, port, 1);
    write_frame(&mut stream, &handshake, Compression::Disabled)?;

    write_frame(&mut stream, &StatusRequest::PAYLOAD, Compression::Disabled)?;

    let response = read_frame(&mut stream, Compression::Disabled)
        .map_err(|error| PingError::from_frame(error, timeout))?;
    let Some((&packet_id, body)) = response.split_first() else {
        return Err(PingError::Truncated);
    };
    if packet_id != 0x00 {
        return Err(PingError::UnexpectedPacket(packet_id));
    }
    let mut cursor = body;
    let len = match read_varint(&mut cursor) {
        Ok(len) => len as usize,
        // A body that ends before its length prefix completed is a short body.
        Err(VarIntError::UnexpectedEof) => return Err(PingError::Truncated),
        // Any other failure means the length prefix itself was malformed.
        Err(error) => return Err(error.into()),
    };
    let json = cursor.get(..len).ok_or(PingError::Truncated)?;
    Ok(serde_json::from_slice(json)?)
}
