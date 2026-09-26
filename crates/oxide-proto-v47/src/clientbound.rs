//! Clientbound packets: the login state now, the play state from Task 4 on.

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES};
use oxide_proto::varint::{VarIntError, read_varint};

use crate::PacketError;

/// A malformed VarInt inside a packet is a codec failure like any other.
impl From<VarIntError> for PacketError {
    fn from(error: VarIntError) -> Self {
        PacketError::Codec(error.into())
    }
}

/// Reads a packet id as the VarInt it is, returning it with the remaining bytes.
///
/// Every dispatcher goes through this, so an id is never assumed to be one byte.
pub fn read_packet_id(payload: &[u8]) -> Result<(i32, &[u8]), PacketError> {
    let mut cursor = Cursor::new(payload);
    let id = match read_varint(&mut cursor) {
        Ok(id) => id,
        Err(VarIntError::UnexpectedEof) => {
            return Err(PacketError::Codec(codec::CodecError::Io(
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the payload ended inside the packet id",
                ),
            )));
        }
        Err(error) => return Err(PacketError::Codec(error.into())),
    };
    let consumed = cursor.position() as usize;
    Ok((id, &payload[consumed..]))
}

/// A packet received during the login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginPacket {
    /// The server refused the login; the reason is chat JSON.
    Disconnect {
        /// Chat JSON for the kick screen.
        reason: String,
    },
    /// The server asked for an encrypted session. M1 refuses this.
    EncryptionRequest {
        /// The server id (empty in 1.7 and later).
        server_id: String,
        /// The server's DER-encoded public key.
        public_key: Vec<u8>,
        /// The token that must come back RSA-encrypted.
        verify_token: Vec<u8>,
    },
    /// The login was accepted; the connection switches to the play state.
    LoginSuccess {
        /// The account UUID, hyphenated, as a string.
        uuid: String,
        /// The account name.
        username: String,
    },
    /// Compression is now enabled with this threshold.
    SetCompression {
        /// The threshold the server sent. A negative value disables compression.
        threshold: i32,
    },
}

/// Decodes a login-state packet whose id has already been read.
pub fn decode_login(body: &[u8]) -> Result<LoginPacket, PacketError> {
    // The id is consumed by `read_packet_id` before this is called; the tests
    // call `decode_login` with the id still in place, so strip it here.
    let (id, body) = read_packet_id(body)?;
    let mut cursor = Cursor::new(body);
    let packet = match id {
        0x00 => LoginPacket::Disconnect {
            reason: codec::read_string(&mut cursor, MAX_STRING_BYTES)?,
        },
        0x01 => {
            let server_id = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
            let key_len = read_varint(&mut cursor)?;
            let public_key = read_bytes(&mut cursor, key_len)?;
            let token_len = read_varint(&mut cursor)?;
            let verify_token = read_bytes(&mut cursor, token_len)?;
            LoginPacket::EncryptionRequest {
                server_id,
                public_key,
                verify_token,
            }
        }
        0x02 => LoginPacket::LoginSuccess {
            uuid: codec::read_string(&mut cursor, 36)?,
            username: codec::read_string(&mut cursor, 16)?,
        },
        0x03 => LoginPacket::SetCompression {
            threshold: read_varint(&mut cursor)?,
        },
        other => {
            return Err(PacketError::Codec(codec::CodecError::Io(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown login packet id {other:#04x}"),
                ),
            )));
        }
    };
    check_no_trailing(&cursor, body.len())?;
    Ok(packet)
}

/// Reads a length-prefixed byte array, refusing a negative length.
fn read_bytes(cursor: &mut Cursor<&[u8]>, len: i32) -> Result<Vec<u8>, PacketError> {
    if len < 0 {
        return Err(PacketError::Codec(codec::CodecError::NegativeLength(len)));
    }
    let len = len as usize;
    let start = cursor.position() as usize;
    let end = start + len;
    let bytes = body_slice(cursor, start, end)?.to_vec();
    // The cursor must sit after the array, so the trailing check sees it consumed.
    cursor.set_position(end as u64);
    Ok(bytes)
}

/// The bytes between two offsets of the cursor's own buffer.
fn body_slice<'a>(
    cursor: &'a Cursor<&'a [u8]>,
    start: usize,
    end: usize,
) -> Result<&'a [u8], PacketError> {
    cursor.get_ref().get(start..end).ok_or_else(|| {
        PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the payload ended inside a byte array",
        )))
    })
}

/// Refuses a payload that decoded with bytes to spare.
fn check_no_trailing(cursor: &Cursor<&[u8]>, len: usize) -> Result<(), PacketError> {
    let consumed = cursor.position() as usize;
    let remaining = len.saturating_sub(consumed);
    if remaining > 0 {
        return Err(PacketError::Trailing(remaining));
    }
    Ok(())
}
