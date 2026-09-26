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

/// Decodes a login-state packet, reading the packet id itself; pass the payload with the id in place.
pub fn decode_login(body: &[u8]) -> Result<LoginPacket, PacketError> {
    // The id is read here, so callers pass the payload with the id still in place.
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
    let end = start.saturating_add(len);
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

/// Clientbound Keep Alive (play id 0x00).
pub const PLAY_KEEP_ALIVE_ID: i32 = 0x00;

/// A keepalive the client must echo back with the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAlive {
    /// The id to echo.
    pub id: i32,
}

impl KeepAlive {
    /// The packet id.
    pub const ID: i32 = PLAY_KEEP_ALIVE_ID;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let id = read_varint(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { id })
    }
}

/// Clientbound Join Game (play id 0x01).
#[derive(Debug, Clone, PartialEq)]
pub struct JoinGame {
    /// The player's entity id.
    pub entity_id: i32,
    /// Gamemode; the 0x08 bit means hardcore.
    pub gamemode: u8,
    /// Dimension: -1 nether, 0 overworld, 1 end.
    pub dimension: i8,
    /// Difficulty.
    pub difficulty: u8,
    /// Maximum player count the server advertises.
    pub max_players: u8,
    /// Level type, for example `default`.
    pub level_type: String,
    /// Whether the server asks for reduced debug info.
    pub reduced_debug_info: bool,
}

impl JoinGame {
    /// The packet id.
    pub const ID: i32 = 0x01;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let entity_id = codec::read_i32(&mut cursor)?;
        let gamemode = codec::read_u8(&mut cursor)?;
        let dimension = codec::read_u8(&mut cursor)? as i8;
        let difficulty = codec::read_u8(&mut cursor)?;
        let max_players = codec::read_u8(&mut cursor)?;
        let level_type = codec::read_string(&mut cursor, 16)?;
        let reduced_debug_info = codec::read_bool(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            entity_id,
            gamemode,
            dimension,
            difficulty,
            max_players,
            level_type,
            reduced_debug_info,
        })
    }
}

/// Clientbound Player Position And Look (play id 0x08).
///
/// A set flag bit means that value is a delta to apply to the current position;
/// [`Self::ABSOLUTE`] means every value is absolute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPositionAndLook {
    /// X, absolute unless [`Self::FLAG_X`] is set.
    pub x: f64,
    /// Y, absolute unless [`Self::FLAG_Y`] is set.
    pub y: f64,
    /// Z, absolute unless [`Self::FLAG_Z`] is set.
    pub z: f64,
    /// Yaw, absolute unless [`Self::FLAG_YAW`] is set.
    pub yaw: f32,
    /// Pitch, absolute unless [`Self::FLAG_PITCH`] is set.
    pub pitch: f32,
    /// The relative-axis flags.
    pub flags: u8,
}

impl PlayerPositionAndLook {
    /// The packet id.
    pub const ID: i32 = 0x08;
    /// Every axis is absolute.
    pub const ABSOLUTE: u8 = 0x00;
    /// X is a relative delta.
    pub const FLAG_X: u8 = 0x01;
    /// Y is a relative delta.
    pub const FLAG_Y: u8 = 0x02;
    /// Z is a relative delta.
    pub const FLAG_Z: u8 = 0x04;
    /// Yaw is a relative delta.
    pub const FLAG_YAW: u8 = 0x08;
    /// Pitch is a relative delta.
    pub const FLAG_PITCH: u8 = 0x10;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let x = codec::read_f64(&mut cursor)?;
        let y = codec::read_f64(&mut cursor)?;
        let z = codec::read_f64(&mut cursor)?;
        let yaw = codec::read_f32(&mut cursor)?;
        let pitch = codec::read_f32(&mut cursor)?;
        let flags = codec::read_u8(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            x,
            y,
            z,
            yaw,
            pitch,
            flags,
        })
    }
}

/// Clientbound Plugin Message (play id 0x3F).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMessage {
    /// The channel name, for example `MC|Brand`.
    pub channel: String,
    /// The payload after the channel: the rest of the body, byte for byte.
    pub data: Vec<u8>,
}

impl PluginMessage {
    /// The packet id.
    pub const ID: i32 = 0x3F;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let channel = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
        // The payload is whatever remains after the channel: unlike every other
        // field it carries no length of its own, so it runs to the end of the
        // body and the trailing check below has nothing left to refuse.
        let start = cursor.position() as usize;
        let data = body_slice(&cursor, start, body.len())?.to_vec();
        cursor.set_position(body.len() as u64);
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { channel, data })
    }
}

/// Clientbound Disconnect (play id 0x40).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayDisconnect {
    /// The kick reason as chat JSON.
    pub reason: String,
}

impl PlayDisconnect {
    /// The packet id.
    pub const ID: i32 = 0x40;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let reason = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { reason })
    }
}

/// One entry of a Player List Item add block.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerListEntry {
    /// The player's UUID.
    pub uuid: [u8; 16],
    /// The name, present for the add action.
    pub name: Option<String>,
    /// The gamemode, present for the add action.
    pub gamemode: Option<i32>,
    /// The ping, present for the add action.
    pub ping: Option<i32>,
    /// The display name, when the entry carries one.
    pub display_name: Option<String>,
}

/// Clientbound Player List Item (play id 0x38), add action.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerListItem {
    /// The entries in this packet.
    pub entries: Vec<PlayerListEntry>,
}

impl PlayerListItem {
    /// The packet id.
    pub const ID: i32 = 0x38;
    /// The add action.
    pub const ACTION_ADD: i32 = 0;

    /// Decodes an add-action packet. Any other action is refused: M1 has no use
    /// for them, and a silent partial read would desynchronise the stream.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let action = read_varint(&mut cursor)?;
        if action != Self::ACTION_ADD {
            return Err(PacketError::Codec(codec::CodecError::Io(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unsupported Player List Item action {action}"),
                ),
            )));
        }
        let count = read_varint(&mut cursor)?;
        if count < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(count)));
        }
        // The count comes off the wire and is hostile until checked: bound the
        // reservation by the bytes the body can still hold, so a huge declared
        // count cannot size an allocation before the reads below run out of
        // payload.
        let remaining = body.len().saturating_sub(cursor.position() as usize);
        let mut entries = Vec::with_capacity((count as usize).min(remaining));
        for _ in 0..count {
            let uuid = codec::read_uuid(&mut cursor)?;
            let name = codec::read_string(&mut cursor, 16)?;
            let properties = read_varint(&mut cursor)?;
            // The properties count is an Int-safe VarInt: a negative value
            // cannot be a real list, so it is clamped to zero rather than
            // trusted.
            for _ in 0..properties.max(0) {
                let _name = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
                let _value = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
                let is_signed = codec::read_bool(&mut cursor)?;
                if is_signed {
                    let _signature = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
                }
            }
            let gamemode = read_varint(&mut cursor)?;
            let ping = read_varint(&mut cursor)?;
            let has_display_name = codec::read_bool(&mut cursor)?;
            let display_name = if has_display_name {
                Some(codec::read_string(&mut cursor, MAX_STRING_BYTES)?)
            } else {
                None
            };
            entries.push(PlayerListEntry {
                uuid,
                name: Some(name),
                gamemode: Some(gamemode),
                ping: Some(ping),
                display_name,
            });
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { entries })
    }
}
