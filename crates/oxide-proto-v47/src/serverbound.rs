//! Serverbound packets: the login state now, the play state from Task 4 on.

use std::io::{self, Write};

use oxide_proto::codec::write_string;

/// Serverbound Login Start (id 0x00).
pub const LOGIN_START_ID: i32 = 0x00;

/// Writes Login Start: the packet id, then the name (16 characters at most).
pub fn write_login_start(mut out: impl Write, username: &str) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, LOGIN_START_ID)?;
    write_string(&mut out, username)
}

/// Serverbound Keep Alive (play id 0x00).
pub const KEEP_ALIVE_ID: i32 = 0x00;

/// Serverbound Chat Message (play id 0x01).
///
/// The payload writer lives in [`crate::ui::write_chat`], beside the chat's
/// decode side; the id sits here with the other write-side ids.
pub const CHAT_MESSAGE_ID: i32 = 0x01;

/// Serverbound Player (play id 0x03): the ground flag alone.
pub const PLAYER_ID: i32 = 0x03;

/// Serverbound Player Position (play id 0x04).
pub const PLAYER_POSITION_ID: i32 = 0x04;

/// Serverbound Player Look (play id 0x05).
pub const PLAYER_LOOK_ID: i32 = 0x05;

/// Serverbound Player Position And Look (play id 0x06): the reply to clientbound 0x08.
pub const PLAYER_POSITION_AND_LOOK_ID: i32 = 0x06;

/// Serverbound Entity Action (play id 0x0B).
pub const ENTITY_ACTION_ID: i32 = 0x0B;

/// Serverbound Player Abilities (play id 0x13).
pub const PLAYER_ABILITIES_ID: i32 = 0x13;

/// Serverbound Client Settings (play id 0x15).
pub const CLIENT_SETTINGS_ID: i32 = 0x15;

/// Serverbound Client Status (play id 0x16).
pub const CLIENT_STATUS_ID: i32 = 0x16;

/// Serverbound Plugin Message (play id 0x17).
pub const PLUGIN_MESSAGE_ID: i32 = 0x17;

/// The protocol's cap on the Client Settings locale field, in bytes.
const LOCALE_MAX_BYTES: usize = 7;

/// The client settings the connection uses until a settings screen exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettings {
    /// Locale, at most 7 bytes, for example `en_US`.
    pub locale: String,
    /// View distance in chunks.
    pub view_distance: u8,
    /// Chat mode: 0 enabled, 1 commands only, 2 hidden.
    pub chat_mode: u8,
    /// Whether chat keeps its colours.
    pub chat_colors: bool,
    /// Displayed skin parts, a bitmask.
    pub skin_parts: u8,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            locale: "en_US".to_string(),
            view_distance: 8,
            chat_mode: 0,
            chat_colors: true,
            skin_parts: 0x7F,
        }
    }
}

/// The actions Client Status can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientStatusAction {
    /// Perform a respawn.
    Respawn = 0,
    /// Ask for statistics.
    RequestStats = 1,
    /// The open-inventory achievement.
    OpenInventory = 2,
}

/// Writes the packet id and a VarInt.
///
/// The two calls reborrow `out`: [`oxide_proto::varint::write_varint`] takes its
/// writer by value, and a `&mut` cannot be used twice without one.
fn write_id_and_varint(out: &mut impl Write, id: i32, value: i32) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut *out, id)?;
    oxide_proto::varint::write_varint(&mut *out, value)
}

/// Writes Keep Alive with `id`.
pub fn write_keep_alive(mut out: impl Write, id: i32) -> io::Result<()> {
    write_id_and_varint(&mut out, KEEP_ALIVE_ID, id)
}

/// Writes Player: the ground flag alone (`C03PacketPlayer.writePacketData`,
/// `C03PacketPlayer.java:55-58`).
///
/// The packet reports no position and no rotation. The source's walking report
/// sends it exactly when neither changed, so a server still hears the ground
/// state once per tick.
pub fn write_player(mut out: impl Write, on_ground: bool) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_ID)?;
    out.write_all(&[u8::from(on_ground)])
}

/// Writes Player Position (`C03PacketPlayer.C04PacketPlayerPosition`,
/// `C03PacketPlayer.java:87-96`): X, the feet Y, Z, then the ground flag.
pub fn write_player_position(
    mut out: impl Write,
    x: f64,
    y: f64,
    z: f64,
    on_ground: bool,
) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_POSITION_ID)?;
    out.write_all(&x.to_be_bytes())?;
    out.write_all(&y.to_be_bytes())?;
    out.write_all(&z.to_be_bytes())?;
    out.write_all(&[u8::from(on_ground)])
}

/// Writes Player Look (`C03PacketPlayer.C05PacketPlayerLook`,
/// `C03PacketPlayer.java:99-107`): yaw, pitch, then the ground flag.
pub fn write_player_look(
    mut out: impl Write,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_LOOK_ID)?;
    out.write_all(&yaw.to_be_bytes())?;
    out.write_all(&pitch.to_be_bytes())?;
    out.write_all(&[u8::from(on_ground)])
}

/// Writes Player Position And Look.
pub fn write_player_position_and_look(
    mut out: impl Write,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_POSITION_AND_LOOK_ID)?;
    out.write_all(&x.to_be_bytes())?;
    out.write_all(&y.to_be_bytes())?;
    out.write_all(&z.to_be_bytes())?;
    out.write_all(&yaw.to_be_bytes())?;
    out.write_all(&pitch.to_be_bytes())?;
    out.write_all(&[u8::from(on_ground)])
}

/// The actions Player Digging carries (`C07PacketPlayerDigging.Action`,
/// `network/play/client/C07PacketPlayerDigging.java:63-71`).
///
/// The variants' declaration order is the status id on the wire. The source's
/// fourth action, `DROP_ITEM`, belongs to the drop key and is not written by
/// this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiggingStatus {
    /// Start destroying a block (status 0).
    Start = 0,
    /// Abort destroying a block (status 1).
    Abort = 1,
    /// Finish destroying a block (status 2).
    Finish = 2,
}

/// Serverbound Player Digging (play id 0x07).
pub const PLAYER_DIGGING_ID: i32 = 0x07;

/// Writes Player Digging: the status VarInt, the Location Position and the
/// face byte (`C07PacketPlayerDigging.writePacketData`, `:39-45`).
///
/// The position is packed as `BlockPos.toLong` packs it — x and z are 26
/// signed bits, y is 12 (`util/BlockPos.java:200-203`) — and the face byte is
/// `EnumFacing.getIndex()`'s ordinal (`util/EnumFacing.java:53-58`), which
/// `Face::wire` answers.
pub fn write_player_digging(
    mut out: impl Write,
    status: DiggingStatus,
    x: i32,
    y: i32,
    z: i32,
    face: u8,
) -> io::Result<()> {
    write_id_and_varint(&mut out, PLAYER_DIGGING_ID, status as i32)?;
    out.write_all(&pack_position(x, y, z).to_be_bytes())?;
    out.write_all(&[face])
}

/// Packs a block position the way `BlockPos.toLong` packs it
/// (`util/BlockPos.java:200-203`):
/// `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)`.
///
/// The masks keep every field's bits clear of its neighbours, and the shifts
/// run on the widened `i64`; a negative x or z sign-extends through its 26
/// bits when the reader takes them back (`BlockPos.fromLong`, `:208-214`).
/// This is 1.8.9's own packing; later versions reordered it, so it must not be
/// "updated" from modern documentation.
fn pack_position(x: i32, y: i32, z: i32) -> i64 {
    (i64::from(x & 0x03FF_FFFF) << 38) | (i64::from(y & 0xFFF) << 26) | i64::from(z & 0x03FF_FFFF)
}

/// Serverbound Player Block Placement (play id 0x08).
pub const PLAYER_BLOCK_PLACEMENT_ID: i32 = 0x08;

/// Writes Player Block Placement: the Location Position, the face byte, the
/// held item stack and the cursor's three bytes
/// (`C08PacketPlayerBlockPlacement.writePacketData`, `:55-63`).
///
/// The position is packed as `BlockPos.toLong` packs it
/// (`util/BlockPos.java:200-203`) — the same packing the digging packet
/// carries — and the face byte is `EnumFacing.getIndex()`'s ordinal
/// (`util/EnumFacing.java:53-58`), which `Face::wire` answers. The cursor
/// bytes are the source's `(int)(facing * 16.0F)` per axis (`:60-62`),
/// already scaled by the caller.
///
/// The held stack is empty: this client's inventory is M5's, and an empty
/// stack is the short `-1` (`PacketBuffer.writeItemStackToBuffer`,
/// `network/PacketBuffer.java:232-237`) — two `FF` bytes on the wire. The
/// server reads the placed item from the inventory it holds for the account
/// (`NetHandlerPlayServer.processPlayerBlockPlacement`,
/// `:578-601`'s `this.playerEntity.inventory.getCurrentItem()`, `:582`), not
/// from the packet's stack, so an empty stack still places the server's item.
pub fn write_player_block_placement(
    mut out: impl Write,
    x: i32,
    y: i32,
    z: i32,
    face: u8,
    cursor: [u8; 3],
) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_BLOCK_PLACEMENT_ID)?;
    out.write_all(&pack_position(x, y, z).to_be_bytes())?;
    out.write_all(&[face, 0xff, 0xff, cursor[0], cursor[1], cursor[2]])
}

/// Serverbound Animation (play id 0x0A).
pub const ANIMATION_ID: i32 = 0x0A;

/// Writes Animation: the packet id alone — the packet carries no fields
/// (`C0APacketAnimation.writePacketData`, `network/play/client/C0APacketAnimation.java:21-23`).
pub fn write_animation(mut out: impl Write) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, ANIMATION_ID)
}

/// The actions Entity Action carries (`C0BPacketEntityAction.Action`,
/// `C0BPacketEntityAction.java:63-71`).
///
/// The variants' declaration order is the id on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityAction {
    /// Crouch (action id 0).
    StartSneaking = 0,
    /// Uncrouch (action id 1).
    StopSneaking = 1,
    /// Leave bed (action id 2).
    StopSleeping = 2,
    /// Start sprinting (action id 3).
    StartSprinting = 3,
    /// Stop sprinting (action id 4).
    StopSprinting = 4,
    /// Jump with a horse (action id 5); the charge rides the jump-boost field.
    RidingJump = 5,
    /// Open the inventory (action id 6).
    OpenInventory = 6,
}

/// Writes Entity Action: the entity id, the action id and the jump boost, all
/// VarInts (`C0BPacketEntityAction.writePacketData`,
/// `C0BPacketEntityAction.java:39-44`).
///
/// The jump boost is the horse jump's charge, `0` for every action but
/// [`EntityAction::RidingJump`].
pub fn write_entity_action(
    mut out: impl Write,
    entity_id: i32,
    action: EntityAction,
    jump_boost: i32,
) -> io::Result<()> {
    write_id_and_varint(&mut out, ENTITY_ACTION_ID, entity_id)?;
    oxide_proto::varint::write_varint(&mut out, action as i32)?;
    oxide_proto::varint::write_varint(&mut out, jump_boost)
}

/// Writes Player Abilities: the flags byte, then the fly speed and the walk
/// speed (`C13PacketPlayerAbilities.writePacketData`,
/// `C13PacketPlayerAbilities.java:64-75`).
///
/// The flags byte's bits are `0x01` invulnerable, `0x02` flying, `0x04` allow
/// flying and `0x08` creative (`:66-71`); the two speeds are the ability
/// packet's own floats.
pub fn write_player_abilities(
    mut out: impl Write,
    flags: u8,
    fly_speed: f32,
    walk_speed: f32,
) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_ABILITIES_ID)?;
    out.write_all(&[flags])?;
    out.write_all(&fly_speed.to_be_bytes())?;
    out.write_all(&walk_speed.to_be_bytes())
}

/// Writes Client Settings.
///
/// The locale is capped at seven bytes, the protocol's limit for the field. A
/// longer one is refused with [`io::ErrorKind::InvalidInput`] before anything
/// is written, so no partial packet reaches the stream.
pub fn write_client_settings(mut out: impl Write, settings: &ClientSettings) -> io::Result<()> {
    if settings.locale.len() > LOCALE_MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "locale exceeds the seven byte field cap",
        ));
    }
    oxide_proto::varint::write_varint(&mut out, CLIENT_SETTINGS_ID)?;
    write_string(&mut out, &settings.locale)?;
    out.write_all(&[
        settings.view_distance,
        settings.chat_mode,
        u8::from(settings.chat_colors),
        settings.skin_parts,
    ])
}

/// Writes Client Status with `action`.
pub fn write_client_status(mut out: impl Write, action: ClientStatusAction) -> io::Result<()> {
    write_id_and_varint(&mut out, CLIENT_STATUS_ID, action as i32)
}

/// Writes Plugin Message on `channel`.
///
/// The data is written verbatim: whatever framing a payload needs is the
/// caller's to pass in.
pub fn write_plugin_message(mut out: impl Write, channel: &str, data: &[u8]) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLUGIN_MESSAGE_ID)?;
    write_string(&mut out, channel)?;
    out.write_all(data)
}

/// The locale cut to the wire's seven-byte cap, on a character boundary.
///
/// A locale at or under the cap is returned unchanged; a longer one keeps its
/// longest prefix of at most seven bytes. The field carries a string, so the
/// cut stops at a character boundary rather than inside one.
fn capped_locale(locale: &str) -> &str {
    let mut end = locale.len().min(LOCALE_MAX_BYTES);
    while !locale.is_char_boundary(end) {
        end -= 1;
    }
    &locale[..end]
}

/// The Client Settings payload as bytes, for byte-exact assertions.
///
/// The helper is total: every value produces a payload, and no locale makes
/// it panic. The locale field is capped at seven bytes by the protocol — the
/// reference's §2.2 Client Settings row — so an overlong locale is cut to its
/// longest prefix of at most seven bytes, never inside a character, before
/// the packet is written. The cap is the protocol's; the cut is this client's
/// own contract for a helper with no error channel. A caller that would
/// rather hear about an overlong locale uses [`write_client_settings`], which
/// refuses one instead of cutting it.
pub fn client_settings_payload(settings: &ClientSettings) -> Vec<u8> {
    let capped = ClientSettings {
        locale: capped_locale(&settings.locale).to_string(),
        ..settings.clone()
    };
    let mut out = Vec::new();
    write_client_settings(&mut out, &capped).expect("a locale within the cap always writes");
    out
}

/// The position-echo payload as bytes, for byte-exact assertions.
pub fn player_position_and_look_payload(
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    write_player_position_and_look(&mut out, x, y, z, yaw, pitch, on_ground)
        .expect("writing to a Vec cannot fail");
    out
}

#[cfg(test)]
mod tests {
    //! Fixed-literal fixtures for the digging and animation packets: every
    //! byte is hand-packed from the layout the protocol reference records
    //! (`docs/research/protocol-47-reference.md` §2.2, the S 0x07 and S 0x0A
    //! rows) and from the source's own packing (`BlockPos.toLong`), never
    //! rebuilt with the writer's arithmetic, so a wrong shift cannot be
    //! confirmed by its own twin.

    use super::{
        DiggingStatus, write_animation, write_player_block_placement, write_player_digging,
    };

    /// The bytes `write_player_block_placement` writes, for byte-exact
    /// assertions.
    fn placement_bytes(x: i32, y: i32, z: i32, face: u8, cursor: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        write_player_block_placement(&mut out, x, y, z, face, cursor)
            .expect("writing to a Vec cannot fail");
        out
    }

    /// The bytes `write_player_digging` writes, for byte-exact assertions.
    fn digging_bytes(status: DiggingStatus, x: i32, y: i32, z: i32, face: u8) -> Vec<u8> {
        let mut out = Vec::new();
        write_player_digging(&mut out, status, x, y, z, face)
            .expect("writing to a Vec cannot fail");
        out
    }

    #[test]
    fn player_digging_writes_the_finish_status_and_a_negative_position() {
        // 0x07: Status VarInt, Location Position (one big-endian i64), Face
        // Byte (`C07PacketPlayerDigging.writePacketData`, `:39-45`; the
        // reference's §2.2 row). The position (-5, 70, -33) packs as
        // `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)`
        // (`BlockPos.toLong`, `util/BlockPos.java:200-203`) — the same
        // hand-derived literal the block-change fixture carries — and the
        // face is 5, EAST's ordinal (`EnumFacing.getIndex`, `:53-58`).
        assert_eq!(
            digging_bytes(DiggingStatus::Finish, -5, 70, -33, 5),
            vec![
                0x07, // the packet id
                0x02, // status 2: finish
                0xff, 0xff, 0xfe, 0xc1, 0x1b, 0xff, 0xff, 0xdf, // (-5, 70, -33)
                0x05, // the face byte
            ],
            "the finish at a negative position"
        );
    }

    #[test]
    fn player_digging_writes_the_start_and_abort_statuses() {
        // Status 0 (start) at (0, 65, 2) with face 2 (NORTH) — the block the
        // aim tests meet — and status 1 (abort) at (16, 70, -1) with face 0
        // (DOWN, the face `resetBlockRemoving` sends, `PlayerControllerMP.java:278`).
        assert_eq!(
            digging_bytes(DiggingStatus::Start, 0, 65, 2, 2),
            vec![
                0x07, 0x00, // the id and status 0
                0x00, 0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x02, // (0, 65, 2)
                0x02, // the face byte
            ],
            "the start"
        );
        assert_eq!(
            digging_bytes(DiggingStatus::Abort, 16, 70, -1, 0),
            vec![
                0x07, 0x01, // the id and status 1
                0x00, 0x00, 0x04, 0x01, 0x1b, 0xff, 0xff, 0xff, // (16, 70, -1)
                0x00, // the face byte
            ],
            "the abort at a negative z"
        );
    }

    #[test]
    fn animation_writes_the_id_alone() {
        // 0x0A carries no fields (`C0APacketAnimation.writePacketData`,
        // `network/play/client/C0APacketAnimation.java:21-23`).
        let mut out = Vec::new();
        write_animation(&mut out).expect("writing to a Vec cannot fail");
        assert_eq!(out, vec![0x0a], "the id alone");
    }

    #[test]
    fn player_block_placement_writes_the_empty_stack_and_the_cursor() {
        // 0x08: Location Position, Face Byte, the held item stack, then the
        // CursorX/Y/Z bytes (`C08PacketPlayerBlockPlacement.writePacketData`,
        // `:55-63`; the reference's §2.2 row). The stack is empty — this
        // client carries no inventory until M5 — and an empty stack is the
        // short `-1` (`PacketBuffer.writeItemStackToBuffer`, `:232-237`): two
        // `FF` bytes. Each cursor byte is `(int)(facing * 16.0F)` for the
        // hit's fraction, 0 through 16, a face-exact hit writing 16 (`:60-62`).
        // The position (-5, 70, -33) packs as `BlockPos.toLong` packs it
        // (`util/BlockPos.java:200-203`), hand-derived here as in the digging
        // fixture, and the face is 5, EAST's ordinal (`EnumFacing.getIndex`,
        // `:53-58`).
        let bytes = placement_bytes(-5, 70, -33, 5, [8, 16, 1]);
        assert_eq!(
            bytes,
            vec![
                0x08, // the packet id
                0xff, 0xff, 0xfe, 0xc1, 0x1b, 0xff, 0xff, 0xdf, // (-5, 70, -33)
                0x05, // the face byte
                0xff, 0xff, // the empty held item stack: the short -1
                0x08, 0x10, 0x01, // the cursor's x, y and z bytes
            ],
            "the placement at a negative position"
        );
        // The item-stack bytes and the cursor bytes, named so a wrong middle
        // shift cannot hide inside a whole-vector match.
        assert_eq!(&bytes[10..12], &[0xff, 0xff], "the empty held item stack");
        assert_eq!(&bytes[12..15], &[8, 16, 1], "the cursor's three bytes");

        // The shape the session's first placement sends: the aimed block
        // (0, 65, 2) through its north face (ordinal 2) with the fractions
        // (0.5, 0.62, 0.0) scaled to (8, 9, 0).
        let bytes = placement_bytes(0, 65, 2, 2, [8, 9, 0]);
        assert_eq!(
            bytes,
            vec![
                0x08, 0x00, 0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x02, // (0, 65, 2)
                0x02, // the face byte
                0xff, 0xff, // the empty held item stack
                0x08, 0x09, 0x00, // the cursor's three bytes
            ],
            "the placement at the aim's own position"
        );
        assert_eq!(&bytes[10..12], &[0xff, 0xff], "the empty held item stack");
        assert_eq!(&bytes[12..15], &[8, 9, 0], "the cursor's three bytes");
    }
}
