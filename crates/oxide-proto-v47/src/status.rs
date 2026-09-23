//! Status state: request, response, ping and pong.

use std::net::TcpStream;
use std::time::Duration;

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
/// a plain JSON string; both forms are accepted.
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
}

impl Description {
    /// The description text, in whichever form it arrived.
    #[must_use]
    pub fn text(self) -> Option<String> {
        match self {
            Description::Object { text } => text,
            Description::Plain(text) => Some(text),
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
}

impl PingError {
    /// Classifies a framing failure: a stream that ended before the response
    /// arrived is a closed connection, anything else is a framing failure.
    fn from_frame(error: FrameError) -> Self {
        match &error {
            FrameError::VarInt(VarIntError::UnexpectedEof) => PingError::Closed,
            FrameError::VarInt(VarIntError::Io(io)) | FrameError::Io(io)
                if io.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                PingError::Closed
            }
            _ => PingError::Frame(error),
        }
    }
}

/// Sends a status ping and returns the parsed response.
///
/// The exchange is the handshake with `next_state` 1, a Status Request, and the
/// server's Status Response. The ping/pong round trip is not needed to read the
/// response, and a server that closes straight after it has still answered.
pub fn ping_server(host: &str, port: u16, timeout: Duration) -> Result<StatusResponse, PingError> {
    let mut stream = TcpStream::connect((host, port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let handshake = crate::handshake::handshake_payload(47, host, port, 1);
    write_frame(&mut stream, &handshake, Compression::Disabled)?;

    write_frame(&mut stream, &StatusRequest::PAYLOAD, Compression::Disabled)?;

    let response = read_frame(&mut stream, Compression::Disabled).map_err(PingError::from_frame)?;
    let Some((&packet_id, body)) = response.split_first() else {
        return Err(PingError::Truncated);
    };
    if packet_id != 0x00 {
        return Err(PingError::UnexpectedPacket(packet_id));
    }
    let mut cursor = body;
    let len = read_varint(&mut cursor).map_err(|_| PingError::Truncated)? as usize;
    let json = cursor.get(..len).ok_or(PingError::Truncated)?;
    Ok(serde_json::from_slice(json)?)
}
