//! Clientbound packets: the login state now, the play state from Task 4 on.

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES};
use oxide_proto::varint::{VarIntError, read_varint};

use crate::PacketError;
use crate::column::{self, ColumnData};

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

/// Clientbound Time Update (play id 0x03).
///
/// The world's age in ticks and its time of day in ticks, both big-endian `i64`s
/// (`S03PacketTimeUpdate.java:36-49`). The server negates the time of day to freeze the
/// sun (`:17-31`), so a negative [`Self::time_of_day`] is kept as received rather than
/// normalised; the age is the counter that never stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeUpdate {
    /// The world's age in ticks.
    pub world_age: i64,
    /// The world's time of day in ticks; negative while the sun is frozen.
    pub time_of_day: i64,
}

impl TimeUpdate {
    /// The packet id.
    pub const ID: i32 = 0x03;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let world_age = codec::read_i64(&mut cursor)?;
        let time_of_day = codec::read_i64(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            world_age,
            time_of_day,
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

/// Clientbound Player Abilities (play id 0x39).
///
/// The flags byte's bits are `0x01` invulnerable, `0x02` flying, `0x04` allow
/// flying and `0x08` creative; the two floats are the fly speed and the walk
/// speed (`S39PacketPlayerAbilities.java:35-44`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerAbilities {
    /// Whether damage is disabled (`disableDamage`).
    pub invulnerable: bool,
    /// Whether the player is flying (`isFlying`).
    pub flying: bool,
    /// Whether flight may be toggled (`allowFlying`).
    pub allow_flying: bool,
    /// Whether creative mode is on (`isCreativeMode`).
    pub creative: bool,
    /// The flight speed (`PlayerCapabilities.getFlySpeed`, `0.05F` by default).
    pub fly_speed: f32,
    /// The walking speed (`PlayerCapabilities.getWalkSpeed`, `0.1F` by default).
    pub walk_speed: f32,
}

impl PlayerAbilities {
    /// The packet id.
    pub const ID: i32 = 0x39;

    /// Flags bit: damage is disabled (`S39PacketPlayerAbilities.java:38`).
    pub const FLAG_INVULNERABLE: u8 = 0x01;

    /// Flags bit: the player is flying (`:39`).
    pub const FLAG_FLYING: u8 = 0x02;

    /// Flags bit: flight may be toggled (`:40`).
    pub const FLAG_ALLOW_FLYING: u8 = 0x04;

    /// Flags bit: creative mode is on (`:41`).
    pub const FLAG_CREATIVE: u8 = 0x08;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let flags = codec::read_u8(&mut cursor)?;
        let fly_speed = codec::read_f32(&mut cursor)?;
        let walk_speed = codec::read_f32(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            invulnerable: flags & Self::FLAG_INVULNERABLE != 0,
            flying: flags & Self::FLAG_FLYING != 0,
            allow_flying: flags & Self::FLAG_ALLOW_FLYING != 0,
            creative: flags & Self::FLAG_CREATIVE != 0,
            fly_speed,
            walk_speed,
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

/// Clientbound Chunk Data (play id 0x21).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkData {
    /// The column's chunk x.
    pub chunk_x: i32,
    /// The column's chunk z.
    pub chunk_z: i32,
    /// Whether the packet replaces the whole column.
    pub ground_up: bool,
    /// The primary bitmask.
    pub mask: u16,
    /// The decoded column. For `ground_up` with an empty mask this carries no
    /// sections and is the unload shape.
    pub column: ColumnData,
}

impl ChunkData {
    /// The packet id.
    pub const ID: i32 = 0x21;

    /// Decodes the fields after the packet id, given the dimension's sky flag.
    ///
    /// The sky flag is a property of the world, not of the packet: the caller
    /// keeps it from Join Game or Respawn.
    pub fn decode(body: &[u8], sky_light_sent: bool) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let chunk_x = codec::read_i32(&mut cursor)?;
        let chunk_z = codec::read_i32(&mut cursor)?;
        let ground_up = codec::read_bool(&mut cursor)?;
        let mask = codec::read_u16(&mut cursor)?;
        let size = read_varint(&mut cursor)?;
        if size < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(size)));
        }
        let size = size as usize;
        let start = cursor.position() as usize;
        let end = start.saturating_add(size);
        let data = body.get(start..end).ok_or(PacketError::BadColumnSize {
            got: body.len().saturating_sub(start),
            expected: size,
        })?;
        cursor.set_position(end as u64);
        check_no_trailing(&cursor, body.len())?;
        let column = if ground_up && mask == 0 {
            ColumnData::empty()
        } else {
            column::parse_column(data, mask, sky_light_sent, ground_up)?
        };
        Ok(Self {
            chunk_x,
            chunk_z,
            ground_up,
            mask,
            column,
        })
    }
}

/// One column of a Map Chunk Bulk packet.
#[derive(Debug, Clone, PartialEq)]
pub struct BulkColumn {
    /// The column's chunk x.
    pub chunk_x: i32,
    /// The column's chunk z.
    pub chunk_z: i32,
    /// The column's primary bitmask.
    pub mask: u16,
    /// The decoded column.
    pub column: ColumnData,
}

/// The bytes one Map Chunk Bulk metadata entry occupies: two ints and the mask.
const BULK_ENTRY_BYTES: usize = 10;

/// Clientbound Map Chunk Bulk (play id 0x26).
#[derive(Debug, Clone, PartialEq)]
pub struct MapChunkBulk {
    /// Whether the columns carry sky light; the flag is per packet.
    pub sky_light: bool,
    /// The decoded columns, in the order the packet lists them.
    pub columns: Vec<BulkColumn>,
}

impl MapChunkBulk {
    /// The packet id.
    pub const ID: i32 = 0x26;

    /// Decodes the fields after the packet id.
    ///
    /// The packet carries no per-column length: the metadata block lists every
    /// column first, and each column's payload is then sized from its own mask
    /// and the packet's sky-light flag, biome array included.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let sky_light = codec::read_bool(&mut cursor)?;
        let count = read_varint(&mut cursor)?;
        if count < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(count)));
        }
        let count = count as usize;
        // The count is hostile until checked: the reservation is bounded by the
        // bytes still available, so a huge declared count cannot size an
        // allocation ahead of the reads that would run out of payload.
        let remaining = body.len().saturating_sub(cursor.position() as usize);
        let mut metadata = Vec::with_capacity(count.min(remaining / BULK_ENTRY_BYTES));
        for _ in 0..count {
            let chunk_x = codec::read_i32(&mut cursor)?;
            let chunk_z = codec::read_i32(&mut cursor)?;
            let mask = codec::read_u16(&mut cursor)?;
            metadata.push((chunk_x, chunk_z, mask));
        }
        let mut columns = Vec::with_capacity(metadata.len());
        for (chunk_x, chunk_z, mask) in metadata {
            let size = column::column_size(mask, sky_light, true);
            let start = cursor.position() as usize;
            let end = start.saturating_add(size);
            let data = body.get(start..end).ok_or(PacketError::BadColumnSize {
                got: body.len().saturating_sub(start),
                expected: size,
            })?;
            cursor.set_position(end as u64);
            let column = column::parse_column(data, mask, sky_light, true)?;
            columns.push(BulkColumn {
                chunk_x,
                chunk_z,
                mask,
                column,
            });
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { sky_light, columns })
    }
}

/// Reads a Location Position: x, y and z packed into one big-endian `i64`.
///
/// The packing is
/// `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)` — x and
/// z are 26 signed bits, y is 12 — and the read mirrors `BlockPos.fromLong`
/// (`util/BlockPos.java:208-214`, bit counts at `:11-18`): each field is
/// shifted into the sign position and back, which sign-extends it. This is
/// 1.8.9's own packing; later versions reordered it, so it must not be
/// "updated" from modern documentation.
fn read_position(cursor: &mut Cursor<&[u8]>) -> Result<(i32, i32, i32), PacketError> {
    let raw = codec::read_i64(cursor)?;
    let x = (raw >> 38) as i32;
    let y = (raw << 26 >> 52) as i32;
    let z = (raw << 38 >> 38) as i32;
    Ok((x, y, z))
}

/// Reads a block-change BlockID: the VarInt carrying `id << 4 | meta`.
///
/// The value is validated into the store's own 16-bit field — `oxide-world`'s
/// packed `(id << 4) | meta` — before it is used: a value outside it cannot
/// name a block, so it is refused rather than truncated.
fn read_block_value(cursor: &mut Cursor<&[u8]>) -> Result<u16, PacketError> {
    let raw = read_varint(cursor)?;
    u16::try_from(raw).map_err(|_| {
        PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("block value {raw} does not fit the 16-bit id/meta field"),
        )))
    })
}

/// Clientbound Block Change (play id 0x23).
///
/// One block's new value at a world position: the Location Position and the
/// BlockID VarInt (`S23PacketBlockChange.readPacketData`). The packet carries
/// no light data, so the client recomputes the light locally
/// (`docs/research/protocol-47-reference.md` §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChange {
    /// The world x.
    pub x: i32,
    /// The world y.
    pub y: i32,
    /// The world z.
    pub z: i32,
    /// The new block value, `id << 4 | meta`.
    pub value: u16,
}

impl BlockChange {
    /// The packet id.
    pub const ID: i32 = 0x23;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let (x, y, z) = read_position(&mut cursor)?;
        let value = read_block_value(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { x, y, z, value })
    }
}

/// One Multi Block Change record: a changed cell inside the packet's chunk.
///
/// The record's two position bytes are read as one big-endian short `v`
/// (`S22PacketMultiBlockChange.BlockUpdateData.getPos`) and split with three
/// logical shifts: `x = (v >> 12) & 15`, `y = v & 255`, `z = (v >> 8) & 15`.
/// The shifts are logical by construction — `v` is unsigned here — while the
/// source's `>>` sign-propagates on its signed short; the masks are what keep
/// the fields apart, and a fixture whose high bit is set cannot by itself tell
/// the two apart, so the rule is stated rather than inferred. The world
/// position composes as `chunk * 16 + local`, which the session does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdate {
    /// The local x in the chunk, 0..16: bits 12..15 of the record.
    pub x: i32,
    /// The local y, 0..256: the record's low byte.
    pub y: i32,
    /// The local z in the chunk, 0..16: bits 8..11 of the record.
    pub z: i32,
    /// The new block value, `id << 4 | meta`.
    pub value: u16,
}

/// Clientbound Multi Block Change (play id 0x22).
///
/// A batch of changed cells inside one chunk: the chunk coordinates, a record
/// count and the records (`S22PacketMultiBlockChange.readPacketData`). The
/// packet carries no light data, so the client recomputes the light locally.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiBlockChange {
    /// The chunk x the records are local to.
    pub chunk_x: i32,
    /// The chunk z the records are local to.
    pub chunk_z: i32,
    /// The records, in the packet's order.
    pub updates: Vec<BlockUpdate>,
}

impl MultiBlockChange {
    /// The packet id.
    pub const ID: i32 = 0x22;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let chunk_x = codec::read_i32(&mut cursor)?;
        let chunk_z = codec::read_i32(&mut cursor)?;
        let count = read_varint(&mut cursor)?;
        if count < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(count)));
        }
        let count = count as usize;
        // The count is hostile until checked: each record occupies at least
        // three bytes — the two-byte position and a one-byte value VarInt — so
        // the reservation is bounded by the bytes still available rather than
        // trusting the declared count.
        let remaining = body.len().saturating_sub(cursor.position() as usize);
        let mut updates = Vec::with_capacity(count.min(remaining / 3));
        for _ in 0..count {
            let v = codec::read_u16(&mut cursor)?;
            let value = read_block_value(&mut cursor)?;
            updates.push(BlockUpdate {
                x: ((v >> 12) & 15) as i32,
                y: (v & 255) as i32,
                z: ((v >> 8) & 15) as i32,
                value,
            });
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            chunk_x,
            chunk_z,
            updates,
        })
    }
}

#[cfg(test)]
mod tests {
    //! Fixed-literal fixtures for the block-change packets: every byte is
    //! hand-packed from the layout the protocol reference records
    //! (`docs/research/protocol-47-reference.md` §2) and from the source's own
    //! packing (`BlockPos.toLong`), never rebuilt with the decoder's
    //! arithmetic, so a wrong shift cannot be confirmed by its own twin.

    use super::{BlockChange, MultiBlockChange};
    use crate::PacketError;

    #[test]
    fn block_change_decodes_the_position_and_the_packed_value() {
        // 0x23: Location Position then BlockID VarInt
        // (`S23PacketBlockChange.readPacketData`). The position's literal
        // bytes are (-5, 70, -33) under the packing
        // `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)`
        // (`BlockPos.toLong`, `util/BlockPos.java:200-203`); the negative x
        // and z are the sign-extension case. 0x97 0x0B is the VarInt 1431,
        // the packed value 89 << 4 | 7.
        let body: &[u8] = &[
            0xff, 0xff, 0xfe, 0xc1, 0x1b, 0xff, 0xff, 0xdf, // the position
            0x97, 0x0b, // the block value
        ];
        let change = BlockChange::decode(body).expect("the fixture decodes");
        assert_eq!(change.x, -5, "the packed x, sign-extended");
        assert_eq!(change.y, 70, "the packed y");
        assert_eq!(change.z, -33, "the packed z");
        assert_eq!(change.value, 1431, "the block value, `id << 4 | meta`");
        assert_eq!(change.value >> 4, 89, "the id nibbles");
        assert_eq!(change.value & 0x0F, 7, "the meta nibble");
        assert_eq!(BlockChange::ID, 0x23, "the packet id");
    }

    #[test]
    fn multi_block_change_decodes_every_record_of_a_negative_chunk() {
        // 0x22: ChunkX Int, ChunkZ Int, RecordCount VarInt, then per record the
        // big-endian short `v` and the BlockID VarInt
        // (`S22PacketMultiBlockChange.readPacketData`). The chunk is (-3, -7).
        // The first record's `v` is 0x4A25: x = (v >> 12) & 15 = 4,
        // y = v & 255 = 37, z = (v >> 8) & 15 = 10; its value 151 is 9 << 4 | 7.
        // The second record's `v` is 0xF8A5, the high bit set: x = 15,
        // y = 165, z = 8; its value 1424 is 89 << 4 | 0.
        let body: &[u8] = &[
            0xff, 0xff, 0xff, 0xfd, // chunk x: -3
            0xff, 0xff, 0xff, 0xf9, // chunk z: -7
            0x02, // two records
            0x4a, 0x25, 0x97, 0x01, // the first: (4, 37, 10), water meta 7
            0xf8, 0xa5, 0x90, 0x0b, // the second: (15, 165, 8), glowstone
        ];
        let change = MultiBlockChange::decode(body).expect("the fixture decodes");
        assert_eq!(change.chunk_x, -3, "the chunk x, negative");
        assert_eq!(change.chunk_z, -7, "the chunk z, negative");
        assert_eq!(change.updates.len(), 2, "both records");
        let first = &change.updates[0];
        assert_eq!(
            (first.x, first.y, first.z),
            (4, 37, 10),
            "the first record's nibbles from 0x4A25"
        );
        assert_eq!(first.value, 151, "the first value, `id << 4 | meta`");
        assert_eq!(
            (first.value >> 4, first.value & 0x0F),
            (9, 7),
            "id 9, meta 7"
        );
        let second = &change.updates[1];
        assert_eq!(
            (second.x, second.y, second.z),
            (15, 165, 8),
            "the second record's nibbles from 0xF8A5"
        );
        assert_eq!(second.value, 1424, "the second value");
        assert_eq!(
            (second.value >> 4, second.value & 0x0F),
            (89, 0),
            "glowstone, no meta"
        );
        assert_eq!(MultiBlockChange::ID, 0x22, "the packet id");
    }

    #[test]
    fn a_block_change_with_trailing_bytes_is_refused() {
        let body: &[u8] = &[
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, // (0, 64, 0)
            0x10, // stone
            0x00, // one byte too many
        ];
        assert!(
            matches!(BlockChange::decode(body), Err(PacketError::Trailing(1))),
            "the trailing byte is refused"
        );
    }

    #[test]
    fn a_multi_block_change_with_a_cut_record_is_refused() {
        // Two records declared; the payload ends inside the second one.
        let body: &[u8] = &[
            0xff, 0xff, 0xff, 0xfd, 0xff, 0xff, 0xff, 0xf9, 0x02, 0x4a, 0x25, 0x97, 0x01, 0xf8,
        ];
        assert!(
            MultiBlockChange::decode(body).is_err(),
            "a cut record is an error, not a panic"
        );
    }

    #[test]
    fn a_negative_record_count_is_refused() {
        // A VarInt -1: five bytes, the two's-complement encoding.
        let body: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x0f,
        ];
        assert!(
            MultiBlockChange::decode(body).is_err(),
            "a negative count is refused"
        );
    }

    #[test]
    fn an_out_of_range_block_value_is_refused() {
        // The VarInt 65536 does not fit the 16-bit `id << 4 | meta` field.
        let body: &[u8] = &[
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, // (0, 64, 0)
            0x80, 0x80, 0x04, // 65536
        ];
        assert!(
            BlockChange::decode(body).is_err(),
            "a value outside the 16-bit field is refused"
        );
    }
}
