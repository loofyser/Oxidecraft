//! Golden-byte tests for the login-state packets, including the hostile paths.

use oxide_proto_v47::clientbound::{LoginPacket, decode_login};
use oxide_proto_v47::serverbound::{LOGIN_START_ID, write_login_start};

#[test]
fn login_start_is_id_then_a_length_prefixed_name() {
    let mut out = Vec::new();
    write_login_start(&mut out, "OxideDev").expect("write");
    assert_eq!(out, b"\x00\x08OxideDev");
    assert_eq!(LOGIN_START_ID, 0x00);
}

#[test]
fn set_compression_carries_the_threshold_as_a_varint() {
    // 256 encodes as 0x80 0x02.
    let mut payload = vec![0x03];
    oxide_proto::varint::write_varint(&mut payload, 256).expect("write");
    match decode_login(&payload).expect("decode") {
        LoginPacket::SetCompression { threshold } => assert_eq!(threshold, 256),
        other => panic!("expected Set Compression, got {other:?}"),
    }
}

#[test]
fn login_success_carries_a_hyphenated_uuid_and_the_name() {
    // The wire form: the id, the 36-character UUID string, then the name.
    let uuid = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
    let username = "OxideDev";
    let mut payload = vec![0x02, uuid.len() as u8];
    payload.extend_from_slice(uuid.as_bytes());
    payload.push(username.len() as u8);
    payload.extend_from_slice(username.as_bytes());
    match decode_login(&payload).expect("decode") {
        LoginPacket::LoginSuccess { uuid, username } => {
            assert_eq!(uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
            assert_eq!(username, "OxideDev");
        }
        other => panic!("expected Login Success, got {other:?}"),
    }
}

#[test]
fn a_disconnect_carries_chat_json() {
    let json = r#"{"text":"You are not whitelisted"}"#;
    let mut payload = vec![0x00, json.len() as u8];
    payload.extend_from_slice(json.as_bytes());
    match decode_login(&payload).expect("decode") {
        LoginPacket::Disconnect { reason } => assert_eq!(reason, json),
        other => panic!("expected Disconnect, got {other:?}"),
    }
}

#[test]
fn an_encryption_request_is_decoded_so_it_can_be_refused_clearly() {
    let payload = vec![
        0x01, 0x00, 0x03, 0xaa, 0xbb, 0xcc, 0x04, 0x01, 0x02, 0x03, 0x04,
    ];
    match decode_login(&payload).expect("decode") {
        LoginPacket::EncryptionRequest {
            server_id,
            public_key,
            verify_token,
        } => {
            assert_eq!(server_id, "");
            assert_eq!(public_key, vec![0xaa, 0xbb, 0xcc]);
            assert_eq!(verify_token, vec![1, 2, 3, 4]);
        }
        other => panic!("expected Encryption Request, got {other:?}"),
    }
}

#[test]
fn a_truncated_payload_is_an_error() {
    assert!(decode_login(&[0x02, 0x10, b'x']).is_err());
    assert!(decode_login(&[]).is_err());
}

#[test]
fn trailing_bytes_are_refused() {
    // Strictness is a bug-catcher: a payload that decodes with leftovers means
    // the field list and the wire disagree.
    let mut payload = vec![0x03];
    oxide_proto::varint::write_varint(&mut payload, 100).expect("write");
    payload.push(0x00);
    assert!(decode_login(&payload).is_err());
}
