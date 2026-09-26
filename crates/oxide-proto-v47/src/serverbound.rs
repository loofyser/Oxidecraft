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

/// Serverbound Player Position And Look (play id 0x06): the reply to clientbound 0x08.
pub const PLAYER_POSITION_AND_LOOK_ID: i32 = 0x06;

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
    /// Locale, at most 7 characters, for example `en_US`.
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

/// The Client Settings payload as bytes, for byte-exact assertions.
///
/// Panics when the settings are invalid — the locale cap refuses a locale over
/// seven bytes and this helper has no error channel.
pub fn client_settings_payload(settings: &ClientSettings) -> Vec<u8> {
    let mut out = Vec::new();
    write_client_settings(&mut out, settings).expect("the locale is within its cap");
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
