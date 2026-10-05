//! Golden-byte tests for the play-state packets M1 needs.

use std::io::{Cursor, ErrorKind};

use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{
    self, JoinGame, KeepAlive, PlayDisconnect, PlayerAbilities, PlayerListItem,
    PlayerPositionAndLook, TimeUpdate,
};
use oxide_proto_v47::serverbound::{
    ClientSettings, ClientStatusAction, ENTITY_ACTION_ID, EntityAction, PLAYER_ABILITIES_ID,
    PLAYER_ID, PLAYER_LOOK_ID, PLAYER_POSITION_ID, client_settings_payload,
    player_position_and_look_payload, write_client_settings, write_client_status,
    write_entity_action, write_keep_alive, write_player, write_player_abilities, write_player_look,
    write_player_position, write_player_position_and_look, write_plugin_message,
};

/// Reads the next eight bytes from a test cursor.
fn cursor_read8(cursor: &mut Cursor<&[u8]>) -> [u8; 8] {
    use std::io::Read;

    let mut bytes = [0u8; 8];
    cursor.read_exact(&mut bytes).expect("eight bytes");
    bytes
}

/// Reads the next four bytes from a test cursor.
fn cursor_read4(cursor: &mut Cursor<&[u8]>) -> [u8; 4] {
    use std::io::Read;

    let mut bytes = [0u8; 4];
    cursor.read_exact(&mut bytes).expect("four bytes");
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
fn time_update_reads_the_age_and_the_time_of_day() {
    // The 0x03 layout: the id, then the world's age and the time of day, each a
    // big-endian i64. The time is negative here, the shape a server sends while
    // the sun is frozen (S03PacketTimeUpdate negates worldTime).
    let mut body = vec![0x03];
    body.extend_from_slice(&48_000i64.to_be_bytes());
    body.extend_from_slice(&(-6001i64).to_be_bytes());
    let TimeUpdate {
        world_age,
        time_of_day,
    } = TimeUpdate::decode(&body[1..]).expect("decode");
    assert_eq!(world_age, 48_000, "the world's age");
    assert_eq!(time_of_day, -6001, "the time of day, sign kept");
    assert_eq!(TimeUpdate::ID, 0x03, "the packet id");
}

#[test]
fn a_time_update_with_extra_bytes_is_refused() {
    // The trailing check keeps the two i64s honest: /time set 6000 arrives with
    // a positive time and nothing after it.
    let mut body = vec![0x03];
    body.extend_from_slice(&48_000i64.to_be_bytes());
    body.extend_from_slice(&6000i64.to_be_bytes());
    let decoded = TimeUpdate::decode(&body[1..]).expect("decode");
    assert_eq!(decoded.time_of_day, 6000);
    body.push(0);
    assert!(
        TimeUpdate::decode(&body[1..]).is_err(),
        "an extra byte is refused"
    );
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
    let list = PlayerListItem::decode(&body[1..]).expect("decode");
    assert_eq!(list.action, PlayerListItem::ACTION_ADD, "the add action");
    let entries = list.entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].uuid, [0xab; 16]);
    assert_eq!(entries[0].name.as_deref(), Some("OxideDev"));
    assert_eq!(entries[0].gamemode, Some(0));
    assert_eq!(entries[0].ping, Some(0));
    assert!(
        entries[0].properties.is_empty(),
        "a propertyless add keeps an empty list"
    );
}

#[test]
fn a_player_list_add_entry_keeps_its_profile_properties() {
    // Two properties, one signed and one not: both name-value pairs survive
    // in wire order, and the signed one's signature is consumed with its
    // entry (a leftover byte would trip the trailing check below).
    let mut body = vec![0x38];
    oxide_proto::varint::write_varint(&mut body, 0).expect("action add");
    oxide_proto::varint::write_varint(&mut body, 1).expect("one entry");
    body.extend_from_slice(&[0xab; 16]);
    body.push(8);
    body.extend_from_slice(b"OxideDev");
    oxide_proto::varint::write_varint(&mut body, 2).expect("two properties");
    // The signed property: a name, a value, the signed flag, the signature.
    body.push(8);
    body.extend_from_slice(b"textures");
    body.push(4);
    body.extend_from_slice(b"eyJx");
    body.push(1);
    body.push(3);
    body.extend_from_slice(b"sig");
    // The unsigned property: a name, a value and its own false signed flag.
    body.push(7);
    body.extend_from_slice(b"texture");
    body.push(3);
    body.extend_from_slice(b"val");
    body.push(0);
    oxide_proto::varint::write_varint(&mut body, 0).expect("gamemode");
    oxide_proto::varint::write_varint(&mut body, 0).expect("ping");
    body.push(0); // no display name
    let list = PlayerListItem::decode(&body[1..]).expect("decode");
    let entries = list.entries;
    assert_eq!(
        entries[0].properties,
        vec![
            ("textures".to_string(), "eyJx".to_string()),
            ("texture".to_string(), "val".to_string()),
        ],
        "both pairs, in wire order"
    );
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
fn a_player_list_remove_action_decodes_its_uuids() {
    // Every action decodes; the remove action carries the UUIDs alone
    // (`S38PacketPlayerListItem.java:112-114`), so an entry's optional fields
    // stay empty and the packet carries the action beside the entries.
    let mut body = vec![0x38];
    oxide_proto::varint::write_varint(&mut body, 4).expect("action remove");
    oxide_proto::varint::write_varint(&mut body, 1).expect("one entry");
    body.extend_from_slice(&[0xcd; 16]);
    let list = PlayerListItem::decode(&body[1..]).expect("the remove action decodes");
    assert_eq!(list.action, PlayerListItem::ACTION_REMOVE);
    assert_eq!(list.entries.len(), 1);
    assert_eq!(list.entries[0].uuid, [0xcd; 16]);
    assert_eq!(list.entries[0].name, None);
    assert_eq!(list.entries[0].gamemode, None);
    assert_eq!(list.entries[0].ping, None);
    assert_eq!(list.entries[0].display_name, None);
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

#[test]
fn a_seven_byte_locale_reaches_the_payload_unchanged() {
    // Exactly seven bytes is within the wire's cap, so the helper writes the
    // locale in full: the id, the length, the bytes, then the four fields
    // after them.
    let settings = ClientSettings {
        locale: "en_US.#".to_string(),
        ..ClientSettings::default()
    };
    let payload = client_settings_payload(&settings);
    assert_eq!(payload, b"\x15\x07en_US.#\x08\x00\x01\x7f");
}

#[test]
fn an_overlong_locale_is_truncated_at_the_wire_cap() {
    // The helper has no error channel, so it stays total the one way the wire
    // allows: the locale keeps its first seven bytes, and the rest of the
    // packet is untouched. The cut is by bytes, and it is this client's own
    // contract for the helper — the source gives the cap only. Nine bytes,
    // `en_US.UTF`, reach the wire as `en_US.U`.
    let settings = ClientSettings {
        locale: "en_US.UTF".to_string(),
        ..ClientSettings::default()
    };
    let payload = client_settings_payload(&settings);
    assert_eq!(payload, b"\x15\x07en_US.U\x08\x00\x01\x7f");
}

#[test]
fn an_overlong_locale_of_multibyte_characters_is_cut_on_a_character_boundary() {
    // The field holds a string, so the cut never lands inside a character:
    // three three-byte characters exceed the cap, and the helper keeps the
    // longest prefix the cap can hold rather than put a broken sequence on
    // the wire.
    let settings = ClientSettings {
        locale: "\u{65e5}\u{672c}\u{8a9e}".to_string(), // nine bytes
        ..ClientSettings::default()
    };
    let payload = client_settings_payload(&settings);
    assert_eq!(payload, b"\x15\x06\xe6\x97\xa5\xe6\x9c\xac\x08\x00\x01\x7f");
}

#[test]
fn the_writer_refuses_the_locale_the_payload_helper_truncates() {
    // One value, two contracts: the writer is for callers with an error
    // channel and never truncates, while the helper cuts to the cap instead.
    // Pinning both against the same locale keeps them from drifting into
    // quiet disagreement.
    let settings = ClientSettings {
        locale: "en_US.UTF".to_string(),
        ..ClientSettings::default()
    };
    let mut out = Vec::new();
    let error = write_client_settings(&mut out, &settings).expect_err("nine bytes is past the cap");
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(out.is_empty(), "a refusal writes nothing");
}

#[test]
fn a_player_tick_reports_the_ground_flag_alone() {
    // 0x03's one field: `C03PacketPlayer.writePacketData` writes the ground
    // byte and nothing else (`C03PacketPlayer.java:55-58`), the "nothing
    // changed" tick a vanilla server still accepts once per tick.
    let mut out = Vec::new();
    write_player(&mut out, true).expect("write");
    assert_eq!(out, [0x03, 0x01]);
    let mut out = Vec::new();
    write_player(&mut out, false).expect("write");
    assert_eq!(out, [0x03, 0x00]);
}

#[test]
fn a_player_position_writes_the_id_then_x_feet_y_z_and_ground() {
    // `C03PacketPlayer.C04PacketPlayerPosition`'s field order
    // (`C03PacketPlayer.java:87-96`): the three doubles, then the ground byte.
    let mut out = Vec::new();
    write_player_position(&mut out, 1.5, 64.0, -2.25, true).expect("write");
    assert_eq!(out[0], 0x04, "the id");
    let mut cursor = Cursor::new(&out[1..]);
    assert_eq!(
        f64::from_be_bytes(cursor_read8(&mut cursor)),
        1.5,
        "x first"
    );
    assert_eq!(
        f64::from_be_bytes(cursor_read8(&mut cursor)),
        64.0,
        "the feet y second"
    );
    assert_eq!(
        f64::from_be_bytes(cursor_read8(&mut cursor)),
        -2.25,
        "z third"
    );
    assert_eq!(
        cursor.position(),
        24,
        "three doubles come before the ground byte"
    );
    assert_eq!(out[25], 1, "on ground is the last byte");
    assert_eq!(out.len(), 26);
}

#[test]
fn a_player_look_writes_the_id_then_yaw_pitch_and_ground() {
    // `C03PacketPlayer.C05PacketPlayerLook`'s field order
    // (`C03PacketPlayer.java:99-107`): yaw, then pitch, then the ground byte.
    let mut out = Vec::new();
    write_player_look(&mut out, 90.0, -30.0, true).expect("write");
    assert_eq!(out[0], 0x05, "the id");
    let mut cursor = Cursor::new(&out[1..]);
    assert_eq!(
        f32::from_be_bytes(cursor_read4(&mut cursor)),
        90.0,
        "yaw first"
    );
    assert_eq!(
        f32::from_be_bytes(cursor_read4(&mut cursor)),
        -30.0,
        "pitch second"
    );
    assert_eq!(out[9], 1, "the ground byte is last");
    assert_eq!(out.len(), 10);
}

#[test]
fn an_entity_action_writes_the_eid_the_action_and_the_boost() {
    // 0x0B: the entity id, the action id and the jump boost, all VarInts
    // (`C0BPacketEntityAction.java:39-44`).
    let mut out = Vec::new();
    write_entity_action(&mut out, 20, EntityAction::StartSprinting, 0).expect("write");
    assert_eq!(out, [0x0B, 0x14, 0x03, 0x00]);
    // A multi-byte entity id and a non-zero boost keep the three fields
    // provably apart: 300 is the two-byte VarInt `AC 02`.
    let mut out = Vec::new();
    write_entity_action(&mut out, 300, EntityAction::StartSneaking, 100).expect("write");
    assert_eq!(out, [0x0B, 0xAC, 0x02, 0x00, 0x64]);
}

#[test]
fn the_entity_action_ids_are_the_sources_ordinals() {
    // `C0BPacketEntityAction.Action`'s declaration order
    // (`C0BPacketEntityAction.java:63-71`), which is the id on the wire.
    assert_eq!(EntityAction::StartSneaking as i32, 0);
    assert_eq!(EntityAction::StopSneaking as i32, 1);
    assert_eq!(EntityAction::StopSleeping as i32, 2);
    assert_eq!(EntityAction::StartSprinting as i32, 3);
    assert_eq!(EntityAction::StopSprinting as i32, 4);
    assert_eq!(EntityAction::RidingJump as i32, 5);
    assert_eq!(EntityAction::OpenInventory as i32, 6);
}

#[test]
fn player_abilities_writes_the_flags_and_both_speeds() {
    // 0x13's field order: the flags byte, then the fly speed and the walk
    // speed (`C13PacketPlayerAbilities.java:64-75`). The flags byte here
    // carries the allow-flying bit 0x04 and the creative bit 0x08.
    let mut out = Vec::new();
    write_player_abilities(&mut out, 0x0C, 0.05, 0.1).expect("write");
    assert_eq!(out[0], 0x13, "the id");
    assert_eq!(
        out[1], 0x0C,
        "the flags byte: allow flying 0x04 | creative 0x08"
    );
    let mut cursor = Cursor::new(&out[2..]);
    assert_eq!(
        f32::from_be_bytes(cursor_read4(&mut cursor)),
        0.05,
        "the fly speed second"
    );
    assert_eq!(
        f32::from_be_bytes(cursor_read4(&mut cursor)),
        0.1,
        "the walk speed third"
    );
    assert_eq!(out.len(), 10);
}

#[test]
fn player_abilities_decodes_the_flags_byte_and_both_speeds() {
    // 0x39 shares 0x13's layout (`S39PacketPlayerAbilities.java:35-44`): the
    // flags byte's four bits, then the two floats.
    let mut body = vec![0x39, 0x0F];
    body.extend_from_slice(&0.05f32.to_be_bytes());
    body.extend_from_slice(&0.1f32.to_be_bytes());
    let abilities = PlayerAbilities::decode(&body[1..]).expect("decode");
    assert!(abilities.invulnerable, "bit 0x01");
    assert!(abilities.flying, "bit 0x02");
    assert!(abilities.allow_flying, "bit 0x04");
    assert!(abilities.creative, "bit 0x08");
    assert_eq!((abilities.fly_speed, abilities.walk_speed), (0.05, 0.1));
    // The creative bit alone: a bit must not smear into its neighbours.
    let mut body = vec![0x39, 0x08];
    body.extend_from_slice(&0.05f32.to_be_bytes());
    body.extend_from_slice(&0.1f32.to_be_bytes());
    let abilities = PlayerAbilities::decode(&body[1..]).expect("decode");
    assert!(abilities.creative);
    assert!(!abilities.invulnerable);
    assert!(!abilities.flying);
    assert!(!abilities.allow_flying);
}

#[test]
fn the_abilities_flag_bits_are_the_spec_bits() {
    // `S39PacketPlayerAbilities.readPacketData`'s masks (`:37-41`).
    assert_eq!(PlayerAbilities::FLAG_INVULNERABLE, 0x01);
    assert_eq!(PlayerAbilities::FLAG_FLYING, 0x02);
    assert_eq!(PlayerAbilities::FLAG_ALLOW_FLYING, 0x04);
    assert_eq!(PlayerAbilities::FLAG_CREATIVE, 0x08);
}

#[test]
fn a_player_abilities_with_extra_bytes_is_refused() {
    // The trailing check keeps the three fields honest.
    let mut body = vec![0x39, 0x00];
    body.extend_from_slice(&0.05f32.to_be_bytes());
    body.extend_from_slice(&0.1f32.to_be_bytes());
    let decoded = PlayerAbilities::decode(&body[1..]).expect("decode");
    assert_eq!(decoded.fly_speed, 0.05);
    body.push(0);
    assert!(
        PlayerAbilities::decode(&body[1..]).is_err(),
        "an extra byte is refused"
    );
}

#[test]
fn the_serverbound_movement_ids_match_the_spec() {
    // The reference's serverbound rows (`protocol-47-reference.md`): 0x03
    // Player, 0x04 Player Position, 0x05 Player Look, 0x0B Entity Action,
    // 0x13 Player Abilities.
    assert_eq!(PLAYER_ID, 0x03);
    assert_eq!(PLAYER_POSITION_ID, 0x04);
    assert_eq!(PLAYER_LOOK_ID, 0x05);
    assert_eq!(ENTITY_ACTION_ID, 0x0B);
    assert_eq!(PLAYER_ABILITIES_ID, 0x13);
}
