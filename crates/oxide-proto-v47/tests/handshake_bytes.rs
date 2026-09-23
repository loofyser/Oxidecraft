//! Golden-byte test for the handshake packet body.

use oxide_proto_v47::handshake::handshake_payload;

#[test]
fn handshake_payload_matches_wire_format() {
    // Protocol 47, host "localhost", port 25565, next state 1 (status).
    let payload = handshake_payload(47, "localhost", 25565, 1);
    // The golden bytes are laid out one protocol field per line.
    #[rustfmt::skip]
    let expected: &[u8] = &[
        0x00, // packet id: handshake
        0x2f, // protocol version 47
        0x09, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't',
        0x63, 0xdd, // port 25565, big endian
        0x01, // next state: status
    ];
    assert_eq!(payload, expected);
}
