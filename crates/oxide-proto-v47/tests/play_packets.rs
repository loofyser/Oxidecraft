//! Golden-byte tests for the play-state packets M1 needs.

use std::io::{Cursor, ErrorKind};

use oxide_proto::codec::CodecError;
use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{
    self, JoinGame, KeepAlive, PlayDisconnect, PlayerListItem, PlayerPositionAndLook,
};
use oxide_proto_v47::serverbound::{
    ClientSettings, ClientStatusAction, client_settings_payload, player_position_and_look_payload,
    write_client_settings, write_client_status, write_keep_alive, write_player_position_and_look,
    write_plugin_message,
};

/// Reads the next eight bytes from a test cursor.
fn cursor_read8(cursor: &mut Cursor<&[u8]>) -> [u8; 8] {
    use std::io::Read;

    let mut bytes = [0u8; 8];
    cursor.read_exact(&mut bytes).expect("eight bytes");
    bytes
}

#[test]
fn a_serverbound_keep_alive_echoes_the_id() {
    // The worked layout: Length 02, id 00, id 01.
    let mut out = Vec::new();
    write_keep_alive(&mut out, 1).expect("write");
    assert_eq!(out, [0x00, 0x01]);
}

#[test]
fn join_game_decodes_all_seven_fields() {
    let mut body = vec![0x01];
    body.extend_from_slice(&20i32.to_be_bytes()); // entity id
    body.push(0); // gamemode: survival
    body.push(0); // dimension: overworld
    body.push(1); // difficulty: easy
    body.push(20); // max players
    body.push(7); // level type length
    body.extend_from_slice(b"default");
    body.push(0); // reduced debug info
    let JoinGame {
        entity_id,
        gamemode,
        dimension,
        difficulty,
        max_players,
        level_type,
        reduced_debug_info,
    } = JoinGame::decode(&body[1..]).expect("decode");
    assert_eq!((entity_id, gamemode, dimension), (20, 0, 0));
    assert_eq!((difficulty, max_players), (1, 20));
    assert_eq!(level_type, "default");
    assert!(!reduced_debug_info);
}

#[test]
fn player_position_and_look_reads_flags_for_relative_axes() {
    let mut body = vec![0x08];
    body.extend_from_slice(&8.5f64.to_be_bytes());
    body.extend_from_slice(&65.0f64.to_be_bytes());
    body.extend_from_slice(&(-12.25f64).to_be_bytes());
    body.extend_from_slice(&90.0f32.to_be_bytes());
    body.extend_from_slice(&(-30.0f32).to_be_bytes());
    body.push(0x01); // X is relative
    let PlayerPositionAndLook {
        x,
        y,
        z,
        yaw,
        pitch,
        flags,
    } = PlayerPositionAndLook::decode(&body[1..]).expect("decode");
    assert_eq!((x, y, z), (8.5, 65.0, -12.25));
    assert_eq!((yaw, pitch), (90.0, -30.0));
    assert_eq!(flags, 0x01);
}

#[test]
fn the_position_echo_is_the_same_absolute_value() {
    let mut out = Vec::new();
    write_player_position_and_look(&mut out, 8.5, 65.0, -12.25, 90.0, -30.0, true).expect("write");
    assert_eq!(out[0], 0x06);
    let mut cursor = Cursor::new(&out[1..]);
    assert_eq!(f64::from_be_bytes(cursor_read8(&mut cursor)), 8.5);
    // on_ground is the last byte
    assert_eq!(*out.last().unwrap(), 1);
}

#[test]
fn client_settings_matches_the_specified_defaults() {
    let settings = ClientSettings::default();
    let payload = client_settings_payload(&settings);
    assert_eq!(payload[0], 0x15);
    assert_eq!(&payload[1..7], b"\x05en_US"); // locale, five bytes
    assert_eq!(payload[7], 8); // view distance
    assert_eq!(payload[8], 0); // chat mode: enabled
    assert_eq!(payload[9], 1); // chat colours
    assert_eq!(payload[10], 0x7F); // skin parts
    assert_eq!(payload.len(), 11);
}

#[test]
fn client_status_actions_carry_their_ids() {
    let mut out = Vec::new();
    write_client_status(&mut out, ClientStatusAction::Respawn).expect("write");
    assert_eq!(out, [0x16, 0x00]);
    let mut out = Vec::new();
    write_client_status(&mut out, ClientStatusAction::RequestStats).expect("write");
    assert_eq!(out, [0x16, 0x01]);
}

#[test]
fn a_player_list_add_entry_decodes_name_and_uuid() {
    let mut body = vec![0x38];
    oxide_proto::varint::write_varint(&mut body, 0).expect("action add");
    oxide_proto::varint::write_varint(&mut body, 1).expect("one entry");
    body.extend_from_slice(&[0xab; 16]);
    body.push(8);
    body.extend_from_slice(b"OxideDev");
    oxide_proto::varint::write_varint(&mut body, 0).expect("no properties");
    oxide_proto::varint::write_varint(&mut body, 0).expect("gamemode");
    oxide_proto::varint::write_varint(&mut body, 0).expect("ping");
    body.push(0); // no display name
    let PlayerListItem { entries } = PlayerListItem::decode(&body[1..]).expect("decode");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].uuid, [0xab; 16]);
    assert_eq!(entries[0].name.as_deref(), Some("OxideDev"));
    assert_eq!(entries[0].gamemode, Some(0));
    assert_eq!(entries[0].ping, Some(0));
}

#[test]
fn a_play_disconnect_carries_its_reason() {
    let json = r#"{"text":"Server closed"}"#;
    let mut body = vec![0x40, json.len() as u8];
    body.extend_from_slice(json.as_bytes());
    let PlayDisconnect { reason } = PlayDisconnect::decode(&body[1..]).expect("decode");
    assert_eq!(reason, json);
}

#[test]
fn the_packet_id_reader_returns_the_id_and_the_remaining_body() {
    let (id, rest) = clientbound::read_packet_id(&[0x7f, 0xaa, 0xbb]).expect("id");
    assert_eq!(id, 0x7f);
    assert_eq!(rest, &[0xaa, 0xbb]);
}

#[test]
fn a_multi_byte_packet_id_is_read_as_a_varint_not_a_byte() {
    // No M1 packet id is above 0x7f, but the reader must not assume that.
    let (id, rest) = clientbound::read_packet_id(&[0x80, 0x01, 0x00]).expect("id");
    assert_eq!(id, 128);
    assert_eq!(rest, &[0x00]);
}

#[test]
fn a_clientbound_keep_alive_decodes_its_id() {
    // The server's 0x00: the id the client must echo back unchanged.
    let mut body = Vec::new();
    oxide_proto::varint::write_varint(&mut body, 7).expect("keep alive id");
    let KeepAlive { id } = KeepAlive::decode(&body).expect("decode");
    assert_eq!(id, 7);
}

#[test]
fn a_keep_alive_with_extra_bytes_is_refused() {
    // The trailing check keeps the decoder honest about its field list: bytes
    // after the id mean the payload and the codec disagree.
    match KeepAlive::decode(&[0x07, 0x00]) {
        Err(PacketError::Trailing(1)) => {}
        other => panic!("expected a trailing-byte refusal, got {other:?}"),
    }
}

#[test]
fn a_clientbound_plugin_message_keeps_the_rest_of_its_body_as_data() {
    let mut body = vec![0x3f, 4];
    body.extend_from_slice(b"REG\x00");
    body.extend_from_slice(&[0xaa, 0xbb]);
    let clientbound::PluginMessage { channel, data } =
        clientbound::PluginMessage::decode(&body[1..]).expect("decode");
    assert_eq!(channel, "REG\x00");
    assert_eq!(data, vec![0xaa, 0xbb]);
}

#[test]
fn a_plugin_message_carries_the_channel_then_the_data() {
    // The brand packet the session sends right after Join Game: the id, the
    // length-prefixed channel, then the payload as given.
    let mut out = Vec::new();
    write_plugin_message(&mut out, "MC|Brand", b"vanilla").expect("write");
    assert_eq!(out, b"\x17\x08MC|Brandvanilla");
}

#[test]
fn plugin_message_data_is_written_verbatim() {
    // Nothing is framed inside the data: a payload that carries its own length
    // prefix reaches the wire unchanged.
    let mut out = Vec::new();
    write_plugin_message(&mut out, "MC|Brand", b"\x07vanilla").expect("write");
    assert_eq!(out, b"\x17\x08MC|Brand\x07vanilla");
}

#[test]
fn a_player_list_action_other_than_add_is_refused() {
    // M1 handles the add action only; any other action has a different field
    // list, and a silent partial read would desynchronise the stream.
    let mut body = vec![0x38];
    oxide_proto::varint::write_varint(&mut body, 4).expect("action remove");
    match PlayerListItem::decode(&body[1..]) {
        Err(PacketError::Codec(CodecError::Io(error))) => {
            assert_eq!(error.kind(), ErrorKind::InvalidData);
            assert_eq!(error.to_string(), "unsupported Player List Item action 4");
        }
        other => panic!("expected an unsupported-action refusal, got {other:?}"),
    }
}

#[test]
fn the_play_packet_ids_match_the_spec() {
    assert_eq!(KeepAlive::ID, 0x00);
    assert_eq!(JoinGame::ID, 0x01);
    assert_eq!(PlayerPositionAndLook::ID, 0x08);
    assert_eq!(PlayerListItem::ID, 0x38);
    assert_eq!(PlayDisconnect::ID, 0x40);
    assert_eq!(clientbound::PluginMessage::ID, 0x3f);
}

#[test]
fn the_position_flags_are_the_spec_bits() {
    assert_eq!(PlayerPositionAndLook::ABSOLUTE, 0x00);
    assert_eq!(PlayerPositionAndLook::FLAG_X, 0x01);
    assert_eq!(PlayerPositionAndLook::FLAG_Y, 0x02);
    assert_eq!(PlayerPositionAndLook::FLAG_Z, 0x04);
    assert_eq!(PlayerPositionAndLook::FLAG_YAW, 0x08);
    assert_eq!(PlayerPositionAndLook::FLAG_PITCH, 0x10);
}

#[test]
fn the_position_echo_payload_is_the_id_then_the_fields() {
    let payload = player_position_and_look_payload(8.5, 65.0, -12.25, 90.0, -30.0, false);
    let mut expected = vec![0x06];
    expected.extend_from_slice(&8.5f64.to_be_bytes());
    expected.extend_from_slice(&65.0f64.to_be_bytes());
    expected.extend_from_slice(&(-12.25f64).to_be_bytes());
    expected.extend_from_slice(&90.0f32.to_be_bytes());
    expected.extend_from_slice(&(-30.0f32).to_be_bytes());
    expected.push(0);
    assert_eq!(payload, expected);
}

#[test]
fn an_overlong_locale_is_refused() {
    // The protocol caps the locale at 7 bytes; a longer one is refused before
    // anything is written, so no partial packet ever reaches the stream.
    let settings = ClientSettings {
        locale: "en_US_POSIX".to_string(),
        ..ClientSettings::default()
    };
    let mut out = Vec::new();
    match write_client_settings(&mut out, &settings) {
        Err(error) => assert_eq!(error.kind(), ErrorKind::InvalidInput),
        Ok(()) => panic!("expected an overlong locale to be refused"),
    }
    assert!(out.is_empty());
    // Exactly seven bytes is the cap and is still accepted.
    let settings = ClientSettings {
        locale: "en_US.#".to_string(),
        ..ClientSettings::default()
    };
    write_client_settings(&mut out, &settings).expect("seven bytes is within the cap");
}
