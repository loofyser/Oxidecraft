//! Golden-vector and round-trip tests for the VarInt codec.

use oxide_proto::varint::{read_varint, write_varint};

#[test]
fn encodes_known_vectors() {
    let cases: &[(i32, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (255, &[0xff, 0x01]),
        (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
    ];
    for (value, expected) in cases {
        let mut out = Vec::new();
        write_varint(&mut out, *value).expect("write");
        assert_eq!(&out, expected, "encoding {value}");
    }
}

#[test]
fn round_trips_every_value_in_sampled_range() {
    for value in (0..).step_by(997).take(4_000) {
        let mut out = Vec::new();
        write_varint(&mut out, value).expect("write");
        let mut cursor = &out[..];
        let decoded = read_varint(&mut cursor).expect("read");
        assert_eq!(decoded, value);
    }
}

#[test]
fn rejects_overlong_encoding() {
    // Six continuation bytes cannot be a valid VarInt.
    let bytes = [0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    let mut cursor = &bytes[..];
    assert!(read_varint(&mut cursor).is_err());
}
