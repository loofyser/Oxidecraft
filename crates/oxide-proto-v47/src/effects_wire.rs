//! The sound, particle and weather wire surface: the sound-effect, particle,
//! game-state and global-entity packets.
//!
//! The field order and conversions are pinned to
//! `docs/research/protocol-47-reference.md` §2.1; the particle table carries
//! the 42 rows of `util/EnumParticleTypes.java` (ids 0-41), and the decoders
//! follow this crate's hostile-input rules — framing never panics, a payload
//! that ends early or breaks a cap is refused with [`PacketError`].

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES};
use oxide_proto::varint::read_varint;

use crate::PacketError;

/// The ceiling on one particle packet's spawn count.
///
/// The source sizes nothing from this count — it spawns straight from the
/// wire (`S2APacketParticles.readPacketData`) — so this client refuses a
/// count past the cap instead of looping without bound.
pub const MAX_PARTICLE_COUNT: i32 = 512;

/// The ceiling on one particle packet's trailing data run.
///
/// The table's widest row carries 2 arguments (`ITEM_CRACK`); the cap sits
/// above it so a future row cannot size the run's list without bound.
pub const MAX_PARTICLE_DATA: usize = 4;

/// Clientbound Sound Effect (play id 0x29).
///
/// The sound name, the block position in 1/8-block units, and the source's
/// own volume/pitch scales (`S29PacketSoundEffect.readPacketData`): volume
/// rides the packet as a float, pitch as a byte with 63 as full pitch
/// (`getPitch` divides the byte by `63.0`). The name is read under the
/// crate's string cap; the source's own reader caps it at 256 bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEffect {
    /// The sound to play, e.g. `mob.creeper.say`.
    pub name: String,
    /// The x in 1/8-block units; divide by 8 for blocks.
    pub x: i32,
    /// The y in 1/8-block units; divide by 8 for blocks.
    pub y: i32,
    /// The z in 1/8-block units; divide by 8 for blocks.
    pub z: i32,
    /// The volume, the source's own float scale.
    pub volume: f32,
    /// The pitch byte, 63 for full pitch.
    pub pitch: u8,
}

impl SoundEffect {
    /// The packet id.
    pub const ID: i32 = 0x29;
}

/// Decodes the Sound Effect payload after the packet id.
pub fn decode_sound_effect(body: &[u8]) -> Result<SoundEffect, PacketError> {
    let mut cursor = Cursor::new(body);
    let name = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
    let x = codec::read_i32(&mut cursor)?;
    let y = codec::read_i32(&mut cursor)?;
    let z = codec::read_i32(&mut cursor)?;
    let volume = codec::read_f32(&mut cursor)?;
    let pitch = codec::read_u8(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(SoundEffect {
        name,
        x,
        y,
        z,
        volume,
        pitch,
    })
}

/// Reads a sound-effect fixed-point coordinate as blocks.
///
/// The wire carries 1/8-block units: the constructor multiplies each axis by
/// `8.0` (`S29PacketSoundEffect.java:27-29`) and the getters divide back by
/// `8.0` (`:66-79`).
pub fn read_effect_position(value: i32) -> f64 {
    value as f64 / 8.0
}

/// Reads a sound-effect pitch byte as a pitch factor.
///
/// The constructor stores `(int)(pitch * 63.0F)` and the getter divides back
/// (`S29PacketSoundEffect.java:31`, `:86-89`), so 63 reads as full pitch.
pub fn read_sound_pitch(pitch: u8) -> f32 {
    pitch as f32 / 63.0
}

/// Clientbound Particle (play id 0x2A).
///
/// One spawn event (`S2APacketParticles.readPacketData`): the particle id,
/// the long-distance flag, the position, spread and speed floats, the spawn
/// count, and the trailing data run — exactly the table's argument count for
/// the id, with no wire length prefix. An id outside 0-41 decodes as
/// `BARRIER` (id 35), the source's own fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    /// The particle id, mapped into 0-41 at decode (unknown becomes 35).
    pub particle_id: i32,
    /// Whether the particle renders past the vanilla range cull.
    pub long_distance: bool,
    /// The spawn position.
    pub x: f32,
    /// The spawn position.
    pub y: f32,
    /// The spawn position.
    pub z: f32,
    /// The per-axis spawn spread.
    pub offset_x: f32,
    /// The per-axis spawn spread.
    pub offset_y: f32,
    /// The per-axis spawn spread.
    pub offset_z: f32,
    /// The particle speed.
    pub speed: f32,
    /// The spawn count, within [`MAX_PARTICLE_COUNT`].
    pub count: i32,
    /// The trailing data run (block/item ids), exactly the table's count.
    pub data: Vec<i32>,
}

impl Particle {
    /// The packet id.
    pub const ID: i32 = 0x2A;
}

/// Decodes the Particle payload after the packet id.
pub fn decode_particle(body: &[u8]) -> Result<Particle, PacketError> {
    let mut cursor = Cursor::new(body);
    let raw_id = codec::read_i32(&mut cursor)?;
    // The source substitutes `BARRIER` when its lookup misses
    // (`S2APacketParticles.readPacketData`), so the argument count below is
    // always `BARRIER`'s (empty) count for an out-of-table id.
    let particle_id = if matches!(raw_id, 0..=41) {
        raw_id
    } else {
        BARRIER_ID
    };
    let long_distance = codec::read_bool(&mut cursor)?;
    let x = codec::read_f32(&mut cursor)?;
    let y = codec::read_f32(&mut cursor)?;
    let z = codec::read_f32(&mut cursor)?;
    let offset_x = codec::read_f32(&mut cursor)?;
    let offset_y = codec::read_f32(&mut cursor)?;
    let offset_z = codec::read_f32(&mut cursor)?;
    let speed = codec::read_f32(&mut cursor)?;
    let count = codec::read_i32(&mut cursor)?;
    if !(0..=MAX_PARTICLE_COUNT).contains(&count) {
        return Err(particle_count_over_cap(count));
    }
    let arguments = particle_argument_count(particle_id);
    if arguments > MAX_PARTICLE_DATA {
        return Err(particle_data_over_cap(arguments));
    }
    let mut data = Vec::with_capacity(arguments);
    for _ in 0..arguments {
        data.push(read_varint(&mut cursor)?);
    }
    check_no_trailing(&cursor, body.len())?;
    Ok(Particle {
        particle_id,
        long_distance,
        x,
        y,
        z,
        offset_x,
        offset_y,
        offset_z,
        speed,
        count,
        data,
    })
}

/// Clientbound Change Game State (play id 0x2B).
///
/// The reason byte and its float value
/// (`S2BPacketChangeGameState.readPacketData`): 0 shows the bed message, 1
/// ends rain, 2 begins rain, 3 changes the gamemode, 4 shows the credits, 5
/// runs the demo branch, 6 plays the arrow-hit sound, 7 sets the rain fade,
/// 8 sets the thunder fade, 10 spawns the guardian effect; reason 9 has no
/// handler and reason bytes above 10 are kept, never refused — the source
/// ignores what it does not know
/// (`NetHandlerPlayClient.handleChangeGameState:1358-1433`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeGameState {
    /// The reason byte, kept as read.
    pub reason: u8,
    /// The reason's float value.
    pub value: f32,
}

impl ChangeGameState {
    /// The packet id.
    pub const ID: i32 = 0x2B;
}

/// Decodes the Change Game State payload after the packet id.
pub fn decode_change_game_state(body: &[u8]) -> Result<ChangeGameState, PacketError> {
    let mut cursor = Cursor::new(body);
    let reason = codec::read_u8(&mut cursor)?;
    let value = codec::read_f32(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(ChangeGameState { reason, value })
}

/// Clientbound Spawn Global Entity (play id 0x2C).
///
/// The entity id, the kind byte (1 is lightning — any other kind keeps the
/// record and draws nothing), and the raw fixed-point position
/// (`S2CPacketSpawnGlobalEntity.readPacketData`; the handler divides each
/// axis by `32.0` in `NetHandlerPlayClient.handleSpawnGlobalEntity:463-483`).
/// The entity id registers for 0x13 removal downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnGlobalEntity {
    /// The entity id.
    pub entity_id: i32,
    /// The kind byte; 1 is lightning.
    pub kind: u8,
    /// The raw fixed-point x; divide by 32 for blocks.
    pub x: i32,
    /// The raw fixed-point y; divide by 32 for blocks.
    pub y: i32,
    /// The raw fixed-point z; divide by 32 for blocks.
    pub z: i32,
}

impl SpawnGlobalEntity {
    /// The packet id.
    pub const ID: i32 = 0x2C;
}

/// Decodes the Spawn Global Entity payload after the packet id.
pub fn decode_spawn_global_entity(body: &[u8]) -> Result<SpawnGlobalEntity, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let kind = codec::read_u8(&mut cursor)?;
    let x = codec::read_i32(&mut cursor)?;
    let y = codec::read_i32(&mut cursor)?;
    let z = codec::read_i32(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnGlobalEntity {
        entity_id,
        kind,
        x,
        y,
        z,
    })
}

/// The particle id-to-name table: the 42 rows of
/// `util/EnumParticleTypes.java`, index = wire id, carried verbatim —
/// including the trailing underscores on `iconcrack_`, `blockcrack_` and
/// `blockdust_`.
pub const PARTICLE_NAMES: [&str; 42] = [
    "explode",          // 0
    "largeexplode",     // 1
    "hugeexplosion",    // 2
    "fireworksSpark",   // 3
    "bubble",           // 4
    "splash",           // 5
    "wake",             // 6
    "suspended",        // 7
    "depthsuspend",     // 8
    "crit",             // 9
    "magicCrit",        // 10
    "smoke",            // 11
    "largesmoke",       // 12
    "spell",            // 13
    "instantSpell",     // 14
    "mobSpell",         // 15
    "mobSpellAmbient",  // 16
    "witchMagic",       // 17
    "dripWater",        // 18
    "dripLava",         // 19
    "angryVillager",    // 20
    "happyVillager",    // 21
    "townaura",         // 22
    "note",             // 23
    "portal",           // 24
    "enchantmenttable", // 25
    "flame",            // 26
    "lava",             // 27
    "footstep",         // 28
    "cloud",            // 29
    "reddust",          // 30
    "snowballpoof",     // 31
    "snowshovel",       // 32
    "slime",            // 33
    "heart",            // 34
    "barrier",          // 35
    "iconcrack_",       // 36
    "blockcrack_",      // 37
    "blockdust_",       // 38
    "droplet",          // 39
    "take",             // 40
    "mobappearance",    // 41
];

/// The particle id an unknown wire id decodes as: `BARRIER`.
///
/// `S2APacketParticles.readPacketData` substitutes `EnumParticleTypes.BARRIER`
/// when the lookup misses, so an out-of-table id never reaches the session.
pub const BARRIER_ID: i32 = 35;

/// The name a particle id carries, or `None` outside 0-41.
pub fn particle_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| PARTICLE_NAMES.get(index).copied())
}

/// The trailing data-run length a particle id carries.
///
/// `ITEM_CRACK` (36) reads 2 VarInts, `BLOCK_CRACK` (37) and `BLOCK_DUST`
/// (38) read 1, every other id reads none
/// (`EnumParticleTypes.getArgumentCount`); ids outside 0-41 report
/// `BARRIER`'s count, matching the decode fallback.
pub fn particle_argument_count(id: i32) -> usize {
    match id {
        36 => 2,
        37 | 38 => 1,
        _ => 0,
    }
}

/// The refusal for a particle count outside `0..=MAX_PARTICLE_COUNT`.
fn particle_count_over_cap(count: i32) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("particle count {count} is outside the 0..={MAX_PARTICLE_COUNT} range"),
    )))
}

/// The refusal for a data run above [`MAX_PARTICLE_DATA`].
fn particle_data_over_cap(arguments: usize) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("particle data run of {arguments} VarInts is over the {MAX_PARTICLE_DATA} cap"),
    )))
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

#[cfg(test)]
#[allow(clippy::std_instead_of_core)]
mod tests {
    use super::*;

    #[test]
    fn effect_position_divides_by_eight() {
        assert_eq!(read_effect_position(-100), -12.5);
        assert_eq!(read_effect_position(512), 64.0);
    }

    #[test]
    fn sound_pitch_divides_by_sixty_three() {
        assert_eq!(read_sound_pitch(63), 1.0);
        assert_eq!(read_sound_pitch(0), 0.0);
    }

    #[test]
    fn particle_caps_match_the_brief() {
        assert_eq!(MAX_PARTICLE_COUNT, 512);
        assert_eq!(MAX_PARTICLE_DATA, 4);
        assert_eq!(BARRIER_ID, 35);
    }
}
