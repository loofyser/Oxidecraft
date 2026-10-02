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
