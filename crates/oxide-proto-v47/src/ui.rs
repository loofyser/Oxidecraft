//! The UI wire surface: the chat message, the tab-list header and footer, the
//! scoreboard's four packets, and the serverbound chat writer's payload.
//!
//! The layouts and mode tables are pinned to
//! `docs/research/protocol-47-reference.md` §2.1 and to the packet classes'
//! own readers (`S02PacketChat.java:32-36`,
//! `S47PacketPlayerListHeaderFooter.java:26-29`,
//! `S3BPacketScoreboardObjective.java:32-41`,
//! `S3CPacketUpdateScore.java:48-57`, `S3DPacketDisplayScoreboard.java:35-38`,
//! `S3EPacketTeams.java:80-103`). A mode or slot outside its table is refused
//! by name before its fields are read, and every decoder applies the crate's
//! trailing-byte rule.

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES, write_string};
use oxide_proto::varint::{read_varint, write_varint};

use crate::PacketError;
use crate::serverbound::CHAT_MESSAGE_ID;

/// The most players one Teams packet may carry.
///
/// The source's reader loops on the declared count with no bound of its own
/// (`S3EPacketTeams.java:95-103`); this client caps the list so a hostile
/// count cannot stretch a packet's work, and refuses one past the cap.
pub const MAX_TEAM_PLAYERS: usize = 512;

/// Refuses a value outside a packet's own mode table, naming the value.
///
/// The refusal is the crate's invalid-data class, so a caller can tell a
/// value outside a table from a truncation or a trailing byte.
fn unsupported(what: &str, value: i32) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("unsupported {what} {value}"),
    )))
}

/// Refuses a Teams player count past [`MAX_TEAM_PLAYERS`], naming both.
fn team_over_cap(count: i32) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Teams player count {count} exceeds the {MAX_TEAM_PLAYERS} player cap"),
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

/// Clientbound Chat Message (play id 0x02).
///
/// `S02PacketChat.readPacketData:32-36`: the chat JSON, then the position
/// byte. The JSON is carried raw — composing or parsing the component is the
/// session's business — so nothing but the protocol's string ceiling is
/// refused.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    /// The chat component as JSON, exactly as sent.
    pub text: String,
    /// Where the message shows: 0 the chat box, 1 the system line, 2 above
    /// the hotbar.
    pub position: i8,
}

impl ChatMessage {
    /// The packet id.
    pub const ID: i32 = 0x02;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let text = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
        let position = codec::read_u8(&mut cursor)? as i8;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { text, position })
    }
}

/// Clientbound Player List Header And Footer (play id 0x47).
///
/// `S47PacketPlayerListHeaderFooter.readPacketData:26-29`: the header JSON,
/// then the footer JSON, both raw.
#[derive(Debug, Clone, PartialEq)]
pub struct TabHeaderFooter {
    /// The header as chat JSON, exactly as sent.
    pub header: String,
    /// The footer as chat JSON, exactly as sent.
    pub footer: String,
}

impl TabHeaderFooter {
    /// The packet id.
    pub const ID: i32 = 0x47;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let header = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
        let footer = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { header, footer })
    }
}

/// Clientbound Scoreboard Objective (play id 0x3B).
///
/// `S3BPacketScoreboardObjective.readPacketData:32-41`: the name, the mode
/// byte and, for the create and update modes, the display value and the
/// render kind. The remove mode carries neither, and any mode outside the
/// table is refused by name.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreboardObjective {
    /// The objective name.
    pub name: String,
    /// The mode: 0 create, 1 remove, 2 update (the `MODE_*` constants).
    pub mode: u8,
    /// The display value, carried by the create and update modes.
    pub value: Option<String>,
    /// The render kind, for example `integer` or `hearts`, carried by the
    /// create and update modes.
    pub kind: Option<String>,
}

impl ScoreboardObjective {
    /// The packet id.
    pub const ID: i32 = 0x3B;
    /// Mode 0: create the objective; carries the value and the kind.
    pub const MODE_CREATE: u8 = 0;
    /// Mode 1: remove the objective; carries neither.
    pub const MODE_REMOVE: u8 = 1;
    /// Mode 2: update the objective; carries the value and the kind.
    pub const MODE_UPDATE: u8 = 2;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let name = codec::read_string(&mut cursor, 16)?;
        let mode = codec::read_u8(&mut cursor)?;
        let (value, kind) = match mode {
            Self::MODE_CREATE | Self::MODE_UPDATE => (
                Some(codec::read_string(&mut cursor, 32)?),
                Some(codec::read_string(&mut cursor, 16)?),
            ),
            Self::MODE_REMOVE => (None, None),
            other => return Err(unsupported("Scoreboard Objective mode", other.into())),
        };
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            name,
            mode,
            value,
            kind,
        })
    }
}

/// Clientbound Update Score (play id 0x3C).
///
/// `S3CPacketUpdateScore.readPacketData:48-57`: the entry name, the mode
/// byte, the objective name and, for the set mode, the value. An empty
/// objective name is the source's own remove-from-every-objective signal
/// (`NetHandlerPlayClient.java:1912-1915`) and is carried as `None`; any mode
/// outside the table is refused by name.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreboardScore {
    /// The scored entry: a player name or another objective key.
    pub entry: String,
    /// The mode: 0 set, 1 remove (the `MODE_*` constants).
    pub mode: u8,
    /// The objective name; `None` for the empty name.
    pub objective: Option<String>,
    /// The score value, carried by the set mode.
    pub value: Option<i32>,
}

impl ScoreboardScore {
    /// The packet id.
    pub const ID: i32 = 0x3C;
    /// Mode 0: set the score; carries the value.
    pub const MODE_SET: u8 = 0;
    /// Mode 1: remove the score; carries no value.
    pub const MODE_REMOVE: u8 = 1;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let entry = codec::read_string(&mut cursor, 40)?;
        let mode = codec::read_u8(&mut cursor)?;
        if mode != Self::MODE_SET && mode != Self::MODE_REMOVE {
            return Err(unsupported("Update Score mode", mode.into()));
        }
        let objective = codec::read_string(&mut cursor, 16)?;
        let objective = if objective.is_empty() {
            None
        } else {
            Some(objective)
        };
        let value = if mode == Self::MODE_SET {
            Some(read_varint(&mut cursor)?)
        } else {
            None
        };
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            entry,
            mode,
            objective,
            value,
        })
    }
}

/// Clientbound Display Scoreboard (play id 0x3D).
///
/// `S3DPacketDisplayScoreboard.readPacketData:35-38`: the slot byte, then the
/// objective name. An empty name is the source's clearing signal
/// (`NetHandlerPlayClient.java:1932-1935`) and is carried as `None`; a slot
/// outside the table is refused by name.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreboardDisplay {
    /// The slot: 0 the list, 1 the sidebar, 2 below the name (the `SLOT_*`
    /// constants).
    pub slot: u8,
    /// The objective name; `None` for the empty name, the clearing.
    pub objective: Option<String>,
}

impl ScoreboardDisplay {
    /// The packet id.
    pub const ID: i32 = 0x3D;
    /// Slot 0: the player list.
    pub const SLOT_LIST: u8 = 0;
    /// Slot 1: the sidebar.
    pub const SLOT_SIDEBAR: u8 = 1;
    /// Slot 2: below the player's name.
    pub const SLOT_BELOW_NAME: u8 = 2;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let slot = codec::read_u8(&mut cursor)?;
        if slot > Self::SLOT_BELOW_NAME {
            return Err(unsupported("Display Scoreboard slot", slot.into()));
        }
        let objective = codec::read_string(&mut cursor, 16)?;
        let objective = if objective.is_empty() {
            None
        } else {
            Some(objective)
        };
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { slot, objective })
    }
}

/// Clientbound Teams (play id 0x3E).
///
/// `S3EPacketTeams.readPacketData:80-103`: the team name, the mode byte, then
/// the mode's own fields — the info block (display name, prefix, suffix,
/// friendly flags, name-tag visibility, colour) for the create and update
/// modes, and the player list for the create, add-players and remove-players
/// modes. Any mode outside the table is refused by name, and a player list
/// past [`MAX_TEAM_PLAYERS`] is refused before it is read.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreboardTeam {
    /// The team name.
    pub name: String,
    /// The mode: 0 create, 1 remove, 2 update info, 3 add players, 4 remove
    /// players (the `MODE_*` constants).
    pub mode: u8,
    /// The display name, carried by the create and update modes.
    pub display_name: Option<String>,
    /// The chat prefix, carried by the create and update modes.
    pub prefix: Option<String>,
    /// The chat suffix, carried by the create and update modes.
    pub suffix: Option<String>,
    /// The friendly-fire flags, carried by the create and update modes.
    pub friendly_flags: Option<u8>,
    /// The name-tag visibility, carried by the create and update modes:
    /// `always`, `hideForOtherTeams`, `hideForOwnTeam` or `never`.
    pub name_tag_visibility: Option<String>,
    /// The colour, carried by the create and update modes.
    pub colour: Option<u8>,
    /// The players, carried by the create, add-players and remove-players
    /// modes.
    pub players: Option<Vec<String>>,
}

impl ScoreboardTeam {
    /// The packet id.
    pub const ID: i32 = 0x3E;
    /// Mode 0: create the team; carries the info block and the player list.
    pub const MODE_CREATE: u8 = 0;
    /// Mode 1: remove the team; carries neither.
    pub const MODE_REMOVE: u8 = 1;
    /// Mode 2: update the team's info; carries no player list.
    pub const MODE_UPDATE: u8 = 2;
    /// Mode 3: add players; carries only the player list.
    pub const MODE_ADD_PLAYERS: u8 = 3;
    /// Mode 4: remove players; carries only the player list.
    pub const MODE_REMOVE_PLAYERS: u8 = 4;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let name = codec::read_string(&mut cursor, 16)?;
        let mode = codec::read_u8(&mut cursor)?;
        if !matches!(
            mode,
            Self::MODE_CREATE
                | Self::MODE_REMOVE
                | Self::MODE_UPDATE
                | Self::MODE_ADD_PLAYERS
                | Self::MODE_REMOVE_PLAYERS
        ) {
            return Err(unsupported("Teams mode", mode.into()));
        }
        let mut team = Self {
            name,
            mode,
            display_name: None,
            prefix: None,
            suffix: None,
            friendly_flags: None,
            name_tag_visibility: None,
            colour: None,
            players: None,
        };
        if mode == Self::MODE_CREATE || mode == Self::MODE_UPDATE {
            team.display_name = Some(codec::read_string(&mut cursor, 32)?);
            team.prefix = Some(codec::read_string(&mut cursor, 16)?);
            team.suffix = Some(codec::read_string(&mut cursor, 16)?);
            team.friendly_flags = Some(codec::read_u8(&mut cursor)?);
            team.name_tag_visibility = Some(codec::read_string(&mut cursor, 32)?);
            team.colour = Some(codec::read_u8(&mut cursor)?);
        }
        if mode == Self::MODE_CREATE
            || mode == Self::MODE_ADD_PLAYERS
            || mode == Self::MODE_REMOVE_PLAYERS
        {
            let count = read_varint(&mut cursor)?;
            if count < 0 {
                return Err(PacketError::Codec(codec::CodecError::NegativeLength(count)));
            }
            if count as usize > MAX_TEAM_PLAYERS {
                return Err(team_over_cap(count));
            }
            // The count is inside the cap by now, and the reservation is
            // further bounded by the bytes still available, so a declared
            // count cannot size an allocation ahead of the reads below.
            let remaining = body.len().saturating_sub(cursor.position() as usize);
            let mut players = Vec::with_capacity((count as usize).min(remaining));
            for _ in 0..count {
                players.push(codec::read_string(&mut cursor, 40)?);
            }
            team.players = Some(players);
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(team)
    }
}

/// Writes the serverbound Chat Message (play id 0x01): the packet id, then
/// the message as a length-prefixed UTF-8 string.
///
/// The message is the player's plain text, not JSON (`C01PacketChatMessage`),
/// and it reaches the wire as given: the field's own 100-character cap is the
/// caller's rule — the chat input and the session's drain enforce it — and
/// the encoder applies no cut of its own. The protocol's string ceiling still
/// applies through [`write_string`], so an unencodable message is refused
/// rather than framed with a length prefix the server would misread.
///
/// # Panics
///
/// Panics if `message` is longer than the protocol's 32767-byte string
/// ceiling; a caller keeps the message within it.
pub fn write_chat(message: &str) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, CHAT_MESSAGE_ID).expect("writing to a Vec cannot fail");
    write_string(&mut out, message)
        .expect("the caller keeps the chat message within the protocol string ceiling");
    out
}

#[cfg(test)]
mod tests {
    //! Fixed-literal tests for the packet ids, the mode tables and the chat
    //! writer: every value is the literal the protocol reference records
    //! (`docs/research/protocol-47-reference.md` §2.1, the rows for 0x02,
    //! 0x3B–0x3E and 0x47; §2.2's S 0x01 row for the writer), never derived
    //! from the constants under test.

    use std::io::Cursor;

    use oxide_proto::codec::MAX_STRING_BYTES;

    use super::{
        ChatMessage, MAX_TEAM_PLAYERS, ScoreboardDisplay, ScoreboardObjective, ScoreboardScore,
        ScoreboardTeam, TabHeaderFooter, write_chat,
    };

    #[test]
    fn the_ui_packet_ids_are_the_section_ids() {
        assert_eq!(ChatMessage::ID, 0x02, "0x02 Chat Message");
        assert_eq!(ScoreboardObjective::ID, 0x3b, "0x3B Scoreboard Objective");
        assert_eq!(ScoreboardScore::ID, 0x3c, "0x3C Update Score");
        assert_eq!(ScoreboardDisplay::ID, 0x3d, "0x3D Display Scoreboard");
        assert_eq!(ScoreboardTeam::ID, 0x3e, "0x3E Teams");
        assert_eq!(TabHeaderFooter::ID, 0x47, "0x47 Player List Header/Footer");
    }

    #[test]
    fn the_ui_mode_tables_are_the_section_tables() {
        // 0x3B: mode 0 create, 1 remove, 2 update.
        assert_eq!(ScoreboardObjective::MODE_CREATE, 0);
        assert_eq!(ScoreboardObjective::MODE_REMOVE, 1);
        assert_eq!(ScoreboardObjective::MODE_UPDATE, 2);
        // 0x3C: action 0 set, 1 remove.
        assert_eq!(ScoreboardScore::MODE_SET, 0);
        assert_eq!(ScoreboardScore::MODE_REMOVE, 1);
        // 0x3D: position 0 list, 1 sidebar, 2 below name.
        assert_eq!(ScoreboardDisplay::SLOT_LIST, 0);
        assert_eq!(ScoreboardDisplay::SLOT_SIDEBAR, 1);
        assert_eq!(ScoreboardDisplay::SLOT_BELOW_NAME, 2);
        // 0x3E: mode 0 create, 1 remove, 2 update info, 3 add players,
        // 4 remove players.
        assert_eq!(ScoreboardTeam::MODE_CREATE, 0);
        assert_eq!(ScoreboardTeam::MODE_REMOVE, 1);
        assert_eq!(ScoreboardTeam::MODE_UPDATE, 2);
        assert_eq!(ScoreboardTeam::MODE_ADD_PLAYERS, 3);
        assert_eq!(ScoreboardTeam::MODE_REMOVE_PLAYERS, 4);
        // The team player-list cap is this client's own bound.
        assert_eq!(MAX_TEAM_PLAYERS, 512);
    }

    #[test]
    fn write_chat_writes_the_id_the_length_and_the_bytes() {
        // The serverbound Chat Message's layout (§2.2, S 0x01): the packet id,
        // the length as a VarInt and the UTF-8 bytes — a byte count, not a
        // character count.
        assert_eq!(write_chat("hi"), b"\x01\x02hi", "the two-byte message");

        // A message past the field's own 100-character cap still encodes: the
        // cap is the caller's rule, and the encoder writes what it is given.
        let message = "x".repeat(101);
        let payload = write_chat(&message);
        assert_eq!(payload[0], 0x01, "the packet id");
        assert_eq!(payload[1], 101, "the length as a one-byte VarInt");
        assert_eq!(&payload[2..], message.as_bytes(), "the bytes as given");

        // Four two-byte characters are eight bytes on the wire.
        let payload = write_chat("\u{e9}\u{e9}\u{e9}\u{e9}");
        assert_eq!(
            payload, b"\x01\x08\xc3\xa9\xc3\xa9\xc3\xa9\xc3\xa9",
            "the wire length counts bytes"
        );
    }

    #[test]
    fn a_chat_message_round_trips_through_the_crate_reader() {
        // The payload reads back through the crate's own id reader and string
        // reader: the same text, and no bytes left over.
        let message = "round trip \u{2713}";
        let payload = write_chat(message);
        let (id, rest) =
            crate::clientbound::read_packet_id(&payload).expect("the payload's id reads");
        assert_eq!(id, crate::serverbound::CHAT_MESSAGE_ID, "the writer's id");
        assert_eq!(id, 0x01, "serverbound Chat Message");
        let mut cursor = Cursor::new(rest);
        let text = oxide_proto::codec::read_string(&mut cursor, MAX_STRING_BYTES)
            .expect("the payload's string reads");
        assert_eq!(text, message, "the same text comes back");
        assert_eq!(
            cursor.position() as usize,
            rest.len(),
            "nothing is left after the string"
        );
    }

    #[test]
    #[should_panic(
        expected = "the caller keeps the chat message within the protocol string ceiling"
    )]
    fn a_message_past_the_protocol_ceiling_is_refused() {
        // The protocol's 32767-byte ceiling is the crate's rule and still
        // applies: the message cannot be framed with a length prefix, and the
        // helper has no error channel, so the refusal surfaces as a panic.
        // The 100-character field cap keeps this unreachable for chat input.
        let message = "x".repeat(MAX_STRING_BYTES + 1);
        let _ = write_chat(&message);
    }
}
