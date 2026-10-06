//! The UI codec fixture corpus: one hand-built byte vector per chat, tab-list,
//! scoreboard and team packet and per mode of every mode table.
//!
//! Every byte is packed by hand from the layouts the protocol reference
//! records (`docs/research/protocol-47-reference.md` §2.1) and from the packet
//! classes' own readers (`S02PacketChat.java:32-36`,
//! `S38PacketPlayerListItem.java:48-117`,
//! `S47PacketPlayerListHeaderFooter.java:26-29`,
//! `S3BPacketScoreboardObjective.java:32-41`,
//! `S3CPacketUpdateScore.java:48-57`, `S3DPacketDisplayScoreboard.java:35-38`,
//! `S3EPacketTeams.java:80-103`), never rebuilt with the decoder's arithmetic,
//! so a wrong field order or shift cannot be confirmed by its own twin.

use std::io::ErrorKind;

use oxide_proto::codec::CodecError;
use oxide_proto::varint::VarIntError;
use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::PlayerListItem;
use oxide_proto_v47::ui::{
    ChatMessage, MAX_TEAM_PLAYERS, ScoreboardDisplay, ScoreboardObjective, ScoreboardScore,
    ScoreboardTeam, TabHeaderFooter,
};

/// Chat Message (0x02): the chat JSON `{"text":"Hi"}`, position 0.
const CHAT: &[u8] = &[
    0x0d, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'H', b'i', b'"', b'}', //
    0x00, // position 0: chat
];

/// Chat Message (0x02): the chat JSON `{"text":"!"}`, position 2.
const CHAT_ABOVE_HOTBAR: &[u8] = &[
    0x0c, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'!', b'"', b'}', //
    0x02, // position 2: above the hotbar
];

/// Chat Message (0x02): the chat JSON `{}`, position byte 0xFF.
const CHAT_SIGNED: &[u8] = &[
    0x02, b'{', b'}', //
    0xff, // position -1 read signed
];

/// Player List Header And Footer (0x47): header `{"text":"Oxide"}`, footer
/// `{"text":""}`.
const TAB_HEADER_FOOTER: &[u8] = &[
    0x10, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'O', b'x', b'i', b'd', b'e', b'"',
    b'}', // header
    0x0b, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'"', b'}', // footer
];

/// Scoreboard Objective (0x3B), mode 0 create: name "obj", value "Display",
/// kind "integer".
const OBJECTIVE_CREATE: &[u8] = &[
    0x03, b'o', b'b', b'j', // the objective name
    0x00, // mode 0: create
    0x07, b'D', b'i', b's', b'p', b'l', b'a', b'y', // the display value
    0x07, b'i', b'n', b't', b'e', b'g', b'e', b'r', // the render kind
];

/// Scoreboard Objective (0x3B), mode 1 remove: name "obj".
const OBJECTIVE_REMOVE: &[u8] = &[
    0x03, b'o', b'b', b'j', //
    0x01, // mode 1: remove
];

/// Scoreboard Objective (0x3B), mode 2 update: name "obj", value "Hearts",
/// kind "hearts".
const OBJECTIVE_UPDATE: &[u8] = &[
    0x03, b'o', b'b', b'j', //
    0x02, // mode 2: update
    0x06, b'H', b'e', b'a', b'r', b't', b's', // the new value
    0x06, b'h', b'e', b'a', b'r', b't', b's', // the new kind
];

/// Scoreboard Objective (0x3B), mode 3: outside the table.
const OBJECTIVE_BAD_MODE: &[u8] = &[
    0x03, b'o', b'b', b'j', //
    0x03, // not a mode
];

/// Update Score (0x3C), mode 0 set: entry "Steve", objective "obj", value -1.
const SCORE_SET_NEGATIVE: &[u8] = &[
    0x05, b'S', b't', b'e', b'v', b'e', // the entry
    0x00, // mode 0: set
    0x03, b'o', b'b', b'j', // the objective
    0xff, 0xff, 0xff, 0xff, 0x0f, // value -1
];

/// Update Score (0x3C), mode 1 remove with an objective: entry "Steve",
/// objective "obj".
const SCORE_REMOVE: &[u8] = &[
    0x05, b'S', b't', b'e', b'v', b'e', //
    0x01, // mode 1: remove
    0x03, b'o', b'b', b'j', // the objective
];

/// Update Score (0x3C), mode 1 remove with the empty objective: entry "Steve".
const SCORE_REMOVE_ALL: &[u8] = &[
    0x05, b'S', b't', b'e', b'v', b'e', //
    0x01, // mode 1: remove
    0x00, // the empty name: every objective
];

/// Update Score (0x3C), mode 2: outside the table.
const SCORE_BAD_MODE: &[u8] = &[
    0x05, b'S', b't', b'e', b'v', b'e', //
    0x02, // not a mode
];

/// Display Scoreboard (0x3D), slot 0 list: objective "obj".
const DISPLAY_LIST: &[u8] = &[
    0x00, // slot 0: the list
    0x03, b'o', b'b', b'j',
];

/// Display Scoreboard (0x3D), slot 1 sidebar: objective "obj".
const DISPLAY_SIDEBAR: &[u8] = &[
    0x01, // slot 1: the sidebar
    0x03, b'o', b'b', b'j',
];

/// Display Scoreboard (0x3D), slot 2 below name: objective "obj".
const DISPLAY_BELOW_NAME: &[u8] = &[
    0x02, // slot 2: below the name
    0x03, b'o', b'b', b'j',
];

/// Display Scoreboard (0x3D), slot 1 sidebar cleared: the empty objective
/// name, the signal the client's own handler reads as "no objective"
/// (`NetHandlerPlayClient.java:1932-1935`).
const DISPLAY_CLEARED: &[u8] = &[
    0x01, // slot 1: the sidebar
    0x00, // the empty name: clear
];

/// Display Scoreboard (0x3D), slot 3: the first team-coloured sidebar slot
/// (`sidebar.team.<colour>` — `Scoreboard.java:479-486`): objective "obj".
const DISPLAY_TEAM_SIDEBAR: &[u8] = &[
    0x03, // slot 3: the first team slot
    0x03, b'o', b'b', b'j',
];

/// Display Scoreboard (0x3D), slot 19: outside the nineteen-entry table
/// (`Scoreboard.java:20`).
const DISPLAY_BAD_SLOT: &[u8] = &[
    0x13, // not a slot
];

/// Teams (0x3E), mode 0 create: "red", display "The Reds", prefix "§c",
/// suffix "S", friendly flags 1, visibility "always", colour 4, and the
/// players "alice", "bob" and "carol".
const TEAM_CREATE: &[u8] = &[
    0x03, b'r', b'e', b'd', // the team name
    0x00, // mode 0: create
    0x08, b'T', b'h', b'e', b' ', b'R', b'e', b'd', b's', // the display name
    0x03, 0xc2, 0xa7, b'c', // the prefix "§c", three UTF-8 bytes
    0x01, b'S', // the suffix
    0x01, // friendly flags
    0x06, b'a', b'l', b'w', b'a', b'y', b's', // the name-tag visibility
    0x04, // the colour
    0x03, // three players
    0x05, b'a', b'l', b'i', b'c', b'e', //
    0x03, b'b', b'o', b'b', //
    0x05, b'c', b'a', b'r', b'o', b'l',
];

/// Teams (0x3E), mode 1 remove: "red".
const TEAM_REMOVE: &[u8] = &[
    0x03, b'r', b'e', b'd', //
    0x01, // mode 1: remove
];

/// Teams (0x3E), mode 2 update info: "red", display "Reds", prefix "P:",
/// suffix empty, flags 2, visibility "hideForOtherTeams", colour 1.
const TEAM_UPDATE: &[u8] = &[
    0x03, b'r', b'e', b'd', //
    0x02, // mode 2: update the info
    0x04, b'R', b'e', b'd', b's', // the display name
    0x02, b'P', b':', // the prefix
    0x00, // the empty suffix
    0x02, // friendly flags
    0x11, b'h', b'i', b'd', b'e', b'F', b'o', b'r', b'O', b't', b'h', b'e', b'r', b'T', b'e', b'a',
    b'm', b's', // the name-tag visibility
    0x01, // the colour
];

/// Teams (0x3E), mode 3 add players: "red" gains "alice" and "bob".
const TEAM_ADD_PLAYERS: &[u8] = &[
    0x03, b'r', b'e', b'd', //
    0x03, // mode 3: add players
    0x02, // two players
    0x05, b'a', b'l', b'i', b'c', b'e', //
    0x03, b'b', b'o', b'b',
];

/// Teams (0x3E), mode 4 remove players: "red" drops "bob".
const TEAM_REMOVE_PLAYERS: &[u8] = &[
    0x03, b'r', b'e', b'd', //
    0x04, // mode 4: remove players
    0x01, // one player
    0x03, b'b', b'o', b'b',
];

/// Teams (0x3E), mode 5: outside the table.
const TEAM_BAD_MODE: &[u8] = &[
    0x03, b'r', b'e', b'd', //
    0x05, // not a mode
];

/// Teams (0x3E), mode 4 with 513 declared players: one past the cap.
const TEAM_OVER_CAP: &[u8] = &[
    0x03, b'r', b'e', b'd', //
    0x04, // mode 4: remove players
    0x81, 0x04, // 513 players
];

/// Player List Item (0x38), action 1 update gamemode: one entry whose
/// gamemode is 1.
const PLAYER_LIST_GAMEMODE: &[u8] = &[
    0x01, // action 1: update the gamemode
    0x01, // one entry
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, // the UUID …
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, // … 1111…
    0x01, // gamemode 1: creative
];

/// Player List Item (0x38), action 2 update latency: a zero and a negative,
/// both kept as sent.
const PLAYER_LIST_LATENCY: &[u8] = &[
    0x02, // action 2: update the latency
    0x02, // two entries
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, // the first UUID …
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, // … 2222…
    0x00, // latency 0
    0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, // the second UUID …
    0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, // … 2323…
    0xff, 0xff, 0xff, 0xff, 0x0f, // latency -1
];

/// Player List Item (0x38), action 3 update display name: one present and one
/// null.
const PLAYER_LIST_DISPLAY_NAME: &[u8] = &[
    0x03, // action 3: update the display name
    0x02, // two entries
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, // the first UUID …
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, // … 3333…
    0x01, // the display name is present
    0x0d, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'H', b'i', b'"', b'}', 0x44, 0x44,
    0x44, 0x44, 0x44, 0x44, 0x44, 0x44, // the second UUID …
    0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, // … 4444…
    0x00, // the display name is null
];

/// Player List Item (0x38), action 4 remove: the UUID alone.
const PLAYER_LIST_REMOVE: &[u8] = &[
    0x04, // action 4: remove
    0x01, // one entry
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, // the UUID …
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, // … 5555…
];

/// Player List Item (0x38), action 5: outside the table.
const PLAYER_LIST_BAD_ACTION: &[u8] = &[
    0x05, // not an action
];

/// Unwraps a named refusal, asserting it is the invalid-data class, and
/// returns its message.
fn refusal_message(error: PacketError) -> String {
    match error {
        PacketError::Codec(CodecError::Io(io)) => {
            assert_eq!(
                io.kind(),
                ErrorKind::InvalidData,
                "a named refusal is invalid data"
            );
            io.to_string()
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
}

#[test]
fn chat_message_decodes_the_json_and_the_position() {
    let chat = ChatMessage::decode(CHAT).expect("the fixture decodes");
    assert_eq!(chat.text, r#"{"text":"Hi"}"#, "the raw chat JSON");
    assert_eq!(chat.position, 0, "position 0: the chat box");
    assert_eq!(ChatMessage::ID, 0x02, "the packet id");

    let chat = ChatMessage::decode(CHAT_ABOVE_HOTBAR).expect("the fixture decodes");
    assert_eq!(chat.text, r#"{"text":"!"}"#, "a one-character message");
    assert_eq!(chat.position, 2, "position 2: above the hotbar");

    // The position byte is read signed: 0xFF is -1, not 255.
    let chat = ChatMessage::decode(CHAT_SIGNED).expect("the fixture decodes");
    assert_eq!(chat.text, "{}", "an empty chat object");
    assert_eq!(chat.position, -1, "the byte is signed");

    // A trailing byte is refused, and a cut payload is an error, never a
    // panic.
    let mut trailing = CHAT.to_vec();
    trailing.push(0x00);
    assert!(matches!(
        ChatMessage::decode(&trailing),
        Err(PacketError::Trailing(1))
    ));
    assert!(ChatMessage::decode(&CHAT[..8]).is_err());
}

#[test]
fn tab_header_footer_decodes_both_json_strings() {
    let tab = TabHeaderFooter::decode(TAB_HEADER_FOOTER).expect("the fixture decodes");
    assert_eq!(tab.header, r#"{"text":"Oxide"}"#, "the header JSON");
    assert_eq!(tab.footer, r#"{"text":""}"#, "the footer JSON");
    assert_eq!(TabHeaderFooter::ID, 0x47, "the packet id");

    // A trailing byte is refused, and a cut payload is an error.
    let mut trailing = TAB_HEADER_FOOTER.to_vec();
    trailing.push(0x00);
    assert!(matches!(
        TabHeaderFooter::decode(&trailing),
        Err(PacketError::Trailing(1))
    ));
    assert!(TabHeaderFooter::decode(&TAB_HEADER_FOOTER[..10]).is_err());
}

#[test]
fn scoreboard_objective_decodes_create_update_and_remove() {
    let objective = ScoreboardObjective::decode(OBJECTIVE_CREATE).expect("the fixture decodes");
    assert_eq!(objective.name, "obj", "the objective name");
    assert_eq!(
        objective.mode,
        ScoreboardObjective::MODE_CREATE,
        "mode 0: create"
    );
    assert_eq!(
        objective.value.as_deref(),
        Some("Display"),
        "the display value"
    );
    assert_eq!(
        objective.kind.as_deref(),
        Some("integer"),
        "the render kind"
    );
    assert_eq!(ScoreboardObjective::ID, 0x3b, "the packet id");

    let objective = ScoreboardObjective::decode(OBJECTIVE_REMOVE).expect("the fixture decodes");
    assert_eq!(objective.mode, ScoreboardObjective::MODE_REMOVE, "mode 1");
    assert_eq!(objective.value, None, "a remove carries no value");
    assert_eq!(objective.kind, None, "a remove carries no kind");

    let objective = ScoreboardObjective::decode(OBJECTIVE_UPDATE).expect("the fixture decodes");
    assert_eq!(objective.mode, ScoreboardObjective::MODE_UPDATE, "mode 2");
    assert_eq!(objective.value.as_deref(), Some("Hearts"), "the new value");
    assert_eq!(objective.kind.as_deref(), Some("hearts"), "the new kind");
}

#[test]
fn scoreboard_objective_refuses_a_mode_outside_the_table() {
    let error =
        ScoreboardObjective::decode(OBJECTIVE_BAD_MODE).expect_err("mode 3 is outside the table");
    assert_eq!(
        refusal_message(error),
        "unsupported Scoreboard Objective mode 3"
    );
}

#[test]
fn scoreboard_score_decodes_a_negative_set_and_the_removes() {
    let score = ScoreboardScore::decode(SCORE_SET_NEGATIVE).expect("the fixture decodes");
    assert_eq!(score.entry, "Steve", "the scored entry");
    assert_eq!(score.mode, ScoreboardScore::MODE_SET, "mode 0: set");
    assert_eq!(score.objective.as_deref(), Some("obj"), "the objective");
    assert_eq!(score.value, Some(-1), "a negative score is kept as sent");
    assert_eq!(ScoreboardScore::ID, 0x3c, "the packet id");

    let score = ScoreboardScore::decode(SCORE_REMOVE).expect("the fixture decodes");
    assert_eq!(score.mode, ScoreboardScore::MODE_REMOVE, "mode 1: remove");
    assert_eq!(score.objective.as_deref(), Some("obj"), "the objective");
    assert_eq!(score.value, None, "a remove carries no value");

    // The empty objective name is the source's remove-from-every-objective
    // signal (`NetHandlerPlayClient.java:1912-1915`), carried as `None`.
    let score = ScoreboardScore::decode(SCORE_REMOVE_ALL).expect("the fixture decodes");
    assert_eq!(
        score.objective, None,
        "the empty name reads as no objective"
    );
    assert_eq!(score.value, None, "a remove carries no value");
}

#[test]
fn scoreboard_score_refuses_a_mode_outside_the_table() {
    let error = ScoreboardScore::decode(SCORE_BAD_MODE).expect_err("mode 2 is outside the table");
    assert_eq!(refusal_message(error), "unsupported Update Score mode 2");
}

#[test]
fn scoreboard_display_decodes_each_slot_and_a_clearing() {
    let display = ScoreboardDisplay::decode(DISPLAY_LIST).expect("the fixture decodes");
    assert_eq!(display.slot, ScoreboardDisplay::SLOT_LIST, "slot 0");
    assert_eq!(display.objective.as_deref(), Some("obj"), "the objective");
    assert_eq!(ScoreboardDisplay::ID, 0x3d, "the packet id");

    let display = ScoreboardDisplay::decode(DISPLAY_SIDEBAR).expect("the fixture decodes");
    assert_eq!(display.slot, ScoreboardDisplay::SLOT_SIDEBAR, "slot 1");
    assert_eq!(display.objective.as_deref(), Some("obj"), "the objective");

    let display = ScoreboardDisplay::decode(DISPLAY_BELOW_NAME).expect("the fixture decodes");
    assert_eq!(display.slot, ScoreboardDisplay::SLOT_BELOW_NAME, "slot 2");
    assert_eq!(display.objective.as_deref(), Some("obj"), "the objective");

    // The sixteen team-coloured sidebar slots: slot 3 is the first of them
    // (`Scoreboard.java:20`, `:479-486`).
    let display = ScoreboardDisplay::decode(DISPLAY_TEAM_SIDEBAR).expect("the fixture decodes");
    assert_eq!(display.slot, 3, "slot 3: the first team slot");
    assert_eq!(display.objective.as_deref(), Some("obj"), "the objective");

    // The empty name clears the slot (`NetHandlerPlayClient.java:1932-1935`),
    // carried as `None`.
    let display = ScoreboardDisplay::decode(DISPLAY_CLEARED).expect("the fixture decodes");
    assert_eq!(display.slot, ScoreboardDisplay::SLOT_SIDEBAR, "slot 1");
    assert_eq!(display.objective, None, "the clearing carries no objective");
}

#[test]
fn scoreboard_display_refuses_a_slot_outside_the_table() {
    let error =
        ScoreboardDisplay::decode(DISPLAY_BAD_SLOT).expect_err("slot 19 is outside the table");
    assert_eq!(
        refusal_message(error),
        "unsupported Display Scoreboard slot 19"
    );
}

#[test]
fn scoreboard_team_decodes_a_create_with_every_field() {
    let team = ScoreboardTeam::decode(TEAM_CREATE).expect("the fixture decodes");
    assert_eq!(team.name, "red", "the team name");
    assert_eq!(team.mode, ScoreboardTeam::MODE_CREATE, "mode 0: create");
    assert_eq!(
        team.display_name.as_deref(),
        Some("The Reds"),
        "the display name"
    );
    assert_eq!(team.prefix.as_deref(), Some("\u{a7}c"), "the prefix");
    assert_eq!(team.suffix.as_deref(), Some("S"), "the suffix");
    assert_eq!(team.friendly_flags, Some(1), "the friendly flags");
    assert_eq!(
        team.name_tag_visibility.as_deref(),
        Some("always"),
        "the name-tag visibility"
    );
    assert_eq!(team.colour, Some(4), "the colour");
    assert_eq!(
        team.players.as_deref(),
        Some(["alice".to_string(), "bob".to_string(), "carol".to_string()].as_slice()),
        "the three players"
    );
    assert_eq!(ScoreboardTeam::ID, 0x3e, "the packet id");
}

#[test]
fn scoreboard_team_decodes_a_remove_and_an_update() {
    let team = ScoreboardTeam::decode(TEAM_REMOVE).expect("the fixture decodes");
    assert_eq!(team.mode, ScoreboardTeam::MODE_REMOVE, "mode 1: remove");
    assert_eq!(team.display_name, None, "a remove carries no info");
    assert_eq!(team.prefix, None, "a remove carries no prefix");
    assert_eq!(team.suffix, None, "a remove carries no suffix");
    assert_eq!(team.friendly_flags, None, "a remove carries no flags");
    assert_eq!(
        team.name_tag_visibility, None,
        "a remove carries no visibility"
    );
    assert_eq!(team.colour, None, "a remove carries no colour");
    assert_eq!(team.players, None, "a remove carries no players");

    let team = ScoreboardTeam::decode(TEAM_UPDATE).expect("the fixture decodes");
    assert_eq!(team.mode, ScoreboardTeam::MODE_UPDATE, "mode 2: update");
    assert_eq!(
        team.display_name.as_deref(),
        Some("Reds"),
        "the display name"
    );
    assert_eq!(team.prefix.as_deref(), Some("P:"), "the prefix");
    assert_eq!(team.suffix.as_deref(), Some(""), "the empty suffix");
    assert_eq!(team.friendly_flags, Some(2), "the friendly flags");
    assert_eq!(
        team.name_tag_visibility.as_deref(),
        Some("hideForOtherTeams"),
        "the name-tag visibility"
    );
    assert_eq!(team.colour, Some(1), "the colour");
    assert_eq!(team.players, None, "an info update carries no player array");
}

#[test]
fn scoreboard_team_decodes_add_and_remove_players() {
    let team = ScoreboardTeam::decode(TEAM_ADD_PLAYERS).expect("the fixture decodes");
    assert_eq!(
        team.mode,
        ScoreboardTeam::MODE_ADD_PLAYERS,
        "mode 3: add players"
    );
    assert_eq!(
        team.players.as_deref(),
        Some(["alice".to_string(), "bob".to_string()].as_slice()),
        "the players to add"
    );
    assert_eq!(team.display_name, None, "no info fields in mode 3");

    let team = ScoreboardTeam::decode(TEAM_REMOVE_PLAYERS).expect("the fixture decodes");
    assert_eq!(
        team.mode,
        ScoreboardTeam::MODE_REMOVE_PLAYERS,
        "mode 4: remove players"
    );
    assert_eq!(
        team.players.as_deref(),
        Some(["bob".to_string()].as_slice()),
        "the players to remove"
    );
    assert_eq!(team.display_name, None, "no info fields in mode 4");
}

#[test]
fn scoreboard_team_refuses_a_mode_outside_the_table() {
    let error = ScoreboardTeam::decode(TEAM_BAD_MODE).expect_err("mode 5 is outside the table");
    assert_eq!(refusal_message(error), "unsupported Teams mode 5");
}

#[test]
fn a_team_over_the_player_cap_is_refused() {
    let error = ScoreboardTeam::decode(TEAM_OVER_CAP).expect_err("513 players are over the cap");
    assert_eq!(
        refusal_message(error),
        "Teams player count 513 exceeds the 512 player cap"
    );

    // Exactly the cap is accepted: a mode-4 packet with 512 one-byte names.
    // The body is assembled with the crate's own string writer because the
    // cap, not the parsing, is what this boundary pins.
    let mut body = vec![0x03, b'r', b'e', b'd', 0x04];
    oxide_proto::varint::write_varint(&mut body, MAX_TEAM_PLAYERS as i32).expect("the count");
    for _ in 0..MAX_TEAM_PLAYERS {
        oxide_proto::codec::write_string(&mut body, "p").expect("a player name");
    }
    let team = ScoreboardTeam::decode(&body).expect("512 players ride the cap");
    assert_eq!(
        team.players.as_deref().map(<[String]>::len),
        Some(MAX_TEAM_PLAYERS),
        "every player at the cap decodes"
    );
    assert_eq!(MAX_TEAM_PLAYERS, 512, "the cap is the plan's literal");
}

#[test]
fn a_string_past_the_crate_cap_is_refused() {
    // The chat text declares 32768 bytes, one past the protocol ceiling.
    let mut chat = vec![0x80, 0x80, 0x02];
    chat.extend_from_slice(b"{}");
    match ChatMessage::decode(&chat) {
        Err(PacketError::Codec(CodecError::TooLong { len, max })) => {
            assert_eq!((len, max), (32768, 32767), "the declared length and cap");
        }
        other => panic!("expected the crate's length refusal, got {other:?}"),
    }

    // The objective name's own field cap is 16 bytes; 17 are refused.
    let mut objective = vec![0x11];
    objective.extend_from_slice(b"seventeen-bytes!!");
    objective.push(0x00);
    match ScoreboardObjective::decode(&objective) {
        Err(PacketError::Codec(CodecError::TooLong { len, max })) => {
            assert_eq!((len, max), (17, 16), "the field's own cap");
        }
        other => panic!("expected the field's length refusal, got {other:?}"),
    }
}

#[test]
fn an_optional_field_missing_in_a_mode_that_requires_it_is_refused() {
    // Mode 0's value string is missing entirely: the payload ends where the
    // value's length VarInt would start.
    let objective = &[0x03, b'o', b'b', b'j', 0x00];
    assert!(matches!(
        ScoreboardObjective::decode(objective),
        Err(PacketError::Codec(CodecError::VarInt(
            VarIntError::UnexpectedEof
        )))
    ));

    // Mode 0's value declares a length the payload cannot fill: the read
    // runs off the end inside the string body.
    let cut_string = vec![0x03, b'o', b'b', b'j', 0x00, 0x07, b'D'];
    match ScoreboardObjective::decode(&cut_string) {
        Err(PacketError::Codec(CodecError::Io(io))) => {
            assert_eq!(io.kind(), ErrorKind::UnexpectedEof, "cut inside the value");
        }
        other => panic!("expected a cut string, got {other:?}"),
    }

    // The sidebar's objective is missing: the payload ends where its length
    // VarInt would start.
    assert!(matches!(
        ScoreboardDisplay::decode(&[0x01]),
        Err(PacketError::Codec(CodecError::VarInt(
            VarIntError::UnexpectedEof
        )))
    ));

    // Mode 0's player count is missing: the payload ends after the colour.
    let team = &[
        0x03, b'r', b'e', b'd', 0x00, 0x08, b'T', b'h', b'e', b' ', b'R', b'e', b'd', b's', 0x03,
        0xc2, 0xa7, b'c', 0x01, b'S', 0x01, 0x06, b'a', b'l', b'w', b'a', b'y', b's', 0x04,
    ];
    assert!(matches!(
        ScoreboardTeam::decode(team),
        Err(PacketError::Codec(CodecError::VarInt(
            VarIntError::UnexpectedEof
        )))
    ));

    // A player name declares five bytes and carries two: cut inside the
    // string.
    let team = &[0x03, b'r', b'e', b'd', 0x04, 0x01, 0x05, b'b', b'o'];
    match ScoreboardTeam::decode(team) {
        Err(PacketError::Codec(CodecError::Io(io))) => {
            assert_eq!(io.kind(), ErrorKind::UnexpectedEof, "cut inside a player");
        }
        other => panic!("expected a cut player name, got {other:?}"),
    }
}

#[test]
fn the_ui_decoders_refuse_trailing_bytes() {
    let cases: [(&[u8], &str); 6] = [
        (CHAT, "chat"),
        (TAB_HEADER_FOOTER, "the tab header and footer"),
        (OBJECTIVE_CREATE, "an objective"),
        (SCORE_SET_NEGATIVE, "a score"),
        (DISPLAY_SIDEBAR, "a display"),
        (TEAM_CREATE, "a team"),
    ];
    for (body, what) in cases {
        let mut trailing = body.to_vec();
        trailing.push(0x00);
        let refused = match what {
            "chat" => matches!(
                ChatMessage::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "the tab header and footer" => matches!(
                TabHeaderFooter::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "an objective" => matches!(
                ScoreboardObjective::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a score" => matches!(
                ScoreboardScore::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a display" => matches!(
                ScoreboardDisplay::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            _ => matches!(
                ScoreboardTeam::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
        };
        assert!(refused, "one trailing byte after {what} is refused");
    }
}

#[test]
fn player_list_gamemode_action_decodes_the_gamemode() {
    let list = PlayerListItem::decode(PLAYER_LIST_GAMEMODE).expect("the fixture decodes");
    assert_eq!(
        list.action,
        PlayerListItem::ACTION_UPDATE_GAME_MODE,
        "action 1: update the gamemode"
    );
    assert_eq!(
        PlayerListItem::ACTION_UPDATE_GAME_MODE,
        1,
        "the action's wire value"
    );
    assert_eq!(list.entries.len(), 1, "one entry");
    let entry = &list.entries[0];
    assert_eq!(entry.uuid, [0x11; 16], "the UUID");
    assert_eq!(entry.gamemode, Some(1), "the gamemode");
    assert_eq!(entry.name, None, "no name in a gamemode update");
    assert_eq!(entry.ping, None, "no ping in a gamemode update");
    assert_eq!(entry.display_name, None, "no display name");
}

#[test]
fn player_list_latency_action_decodes_a_zero_and_a_negative() {
    let list = PlayerListItem::decode(PLAYER_LIST_LATENCY).expect("the fixture decodes");
    assert_eq!(
        list.action,
        PlayerListItem::ACTION_UPDATE_LATENCY,
        "action 2: update the latency"
    );
    assert_eq!(
        PlayerListItem::ACTION_UPDATE_LATENCY,
        2,
        "the action's wire value"
    );
    assert_eq!(list.entries.len(), 2, "two entries");
    assert_eq!(list.entries[0].uuid, [0x22; 16], "the first UUID");
    assert_eq!(list.entries[0].ping, Some(0), "a zero latency");
    assert_eq!(list.entries[1].uuid, [0x23; 16], "the second UUID");
    assert_eq!(
        list.entries[1].ping,
        Some(-1),
        "a negative latency is kept as sent"
    );
    assert_eq!(
        list.entries[0].gamemode, None,
        "no gamemode in a latency update"
    );
}

#[test]
fn player_list_display_name_action_decodes_present_and_null() {
    let list = PlayerListItem::decode(PLAYER_LIST_DISPLAY_NAME).expect("the fixture decodes");
    assert_eq!(
        list.action,
        PlayerListItem::ACTION_UPDATE_DISPLAY_NAME,
        "action 3: update the display name"
    );
    assert_eq!(
        PlayerListItem::ACTION_UPDATE_DISPLAY_NAME,
        3,
        "the action's wire value"
    );
    assert_eq!(list.entries.len(), 2, "two entries");
    assert_eq!(list.entries[0].uuid, [0x33; 16], "the first UUID");
    assert_eq!(
        list.entries[0].display_name.as_deref(),
        Some(r#"{"text":"Hi"}"#),
        "a present display name"
    );
    assert_eq!(list.entries[1].uuid, [0x44; 16], "the second UUID");
    assert_eq!(
        list.entries[1].display_name, None,
        "a null display name stays None"
    );
}

#[test]
fn player_list_remove_action_decodes_the_uuids() {
    let list = PlayerListItem::decode(PLAYER_LIST_REMOVE).expect("the fixture decodes");
    assert_eq!(
        list.action,
        PlayerListItem::ACTION_REMOVE,
        "action 4: remove"
    );
    assert_eq!(PlayerListItem::ACTION_REMOVE, 4, "the action's wire value");
    assert_eq!(list.entries.len(), 1, "one entry");
    let entry = &list.entries[0];
    assert_eq!(entry.uuid, [0x55; 16], "the UUID");
    assert_eq!(entry.name, None, "a remove carries no name");
    assert_eq!(entry.gamemode, None, "a remove carries no gamemode");
    assert_eq!(entry.ping, None, "a remove carries no latency");
    assert_eq!(entry.display_name, None, "a remove carries no display name");

    // A trailing byte is refused, and a cut entry is an error.
    let mut trailing = PLAYER_LIST_REMOVE.to_vec();
    trailing.push(0x00);
    assert!(matches!(
        PlayerListItem::decode(&trailing),
        Err(PacketError::Trailing(1))
    ));
    assert!(PlayerListItem::decode(&PLAYER_LIST_GAMEMODE[..10]).is_err());
}

#[test]
fn player_list_refuses_an_action_outside_the_table() {
    let error =
        PlayerListItem::decode(PLAYER_LIST_BAD_ACTION).expect_err("action 5 is outside the table");
    assert_eq!(
        refusal_message(error),
        "unsupported Player List Item action 5"
    );
}
