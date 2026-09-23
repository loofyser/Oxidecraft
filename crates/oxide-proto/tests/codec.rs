//! Tests for the primitive and string codecs, including their hostile-input paths.

use std::io::{Cursor, ErrorKind};

use oxide_proto::codec::{
    CodecError, MAX_STRING_BYTES, read_bool, read_f32, read_f64, read_i16, read_i32, read_i64,
    read_string, read_u8, read_u16, read_uuid, write_bool, write_f32, write_f64, write_i16,
    write_i32, write_i64, write_string, write_u8, write_u16, write_uuid,
};

#[test]
fn integers_are_written_big_endian() {
    let mut out = Vec::new();
    write_i32(&mut out, -2).expect("write");
    assert_eq!(out, [0xff, 0xff, 0xff, 0xfe]);
}

#[test]
fn a_string_is_length_prefixed_utf8() {
    let mut out = Vec::new();
    write_string(&mut out, "OxideDev").expect("write");
    assert_eq!(out, b"\x08OxideDev");

    let mut cursor = Cursor::new(out);
    assert_eq!(read_string(&mut cursor, 16).expect("read"), "OxideDev");
}

#[test]
fn a_string_longer_than_its_cap_is_refused() {
    // 17 bytes against a 16-byte username cap.
    let mut bytes = vec![17u8];
    bytes.extend_from_slice(b"0123456789abcdefg");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::TooLong { max: 16, .. })
    ));
}

#[test]
fn a_string_longer_than_the_protocol_cap_is_refused() {
    let mut bytes = Vec::new();
    oxide_proto::varint::write_varint(&mut bytes, (MAX_STRING_BYTES + 1) as i32)
        .expect("write length");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, MAX_STRING_BYTES),
        Err(CodecError::TooLong { .. })
    ));
}

#[test]
fn invalid_utf8_is_refused() {
    let mut bytes = vec![2u8];
    bytes.extend_from_slice(&[0xff, 0xfe]);
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::BadUtf8)
    ));
}

#[test]
fn a_negative_string_length_is_refused() {
    let mut bytes = Vec::new();
    oxide_proto::varint::write_varint(&mut bytes, -1).expect("write length");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::NegativeLength(-1))
    ));
}

#[test]
fn a_truncated_string_is_reported_as_eof() {
    let mut bytes = vec![6u8];
    bytes.extend_from_slice(b"abc");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::Io(_))
    ));
}

#[test]
fn booleans_read_any_non_zero_byte_as_true() {
    for (byte, expected) in [(0u8, false), (1u8, true), (0x7f, true)] {
        let mut cursor = Cursor::new(vec![byte]);
        assert_eq!(read_bool(&mut cursor).expect("read"), expected);
    }
}

#[test]
fn unsigned_and_signed_widths_are_read_back_correctly() {
    let mut cursor = Cursor::new(vec![0xff, 0xff]);
    assert_eq!(read_u16(&mut cursor).expect("read"), 0xffff);
    // Two bytes cannot fill an i32; the same value needs four.
    let mut cursor = Cursor::new(vec![0x00, 0x00, 0xff, 0xff]);
    assert_eq!(read_i32(&mut cursor).expect("read"), 65_535);
}

#[test]
fn a_string_longer_than_the_protocol_ceiling_is_refused_on_write() {
    let oversized = "a".repeat(MAX_STRING_BYTES + 1);
    let mut out = Vec::new();
    let error = write_string(&mut out, &oversized).expect_err("write");
    assert_eq!(error.kind(), ErrorKind::InvalidInput, "error: {error:?}");
    assert!(out.is_empty(), "a refused string must stage no bytes");
}

#[test]
fn a_caller_cap_above_the_protocol_ceiling_is_clamped() {
    // 40_000 bytes under a 1_000_000-byte caller cap: the protocol ceiling
    // still refuses the string.
    let mut bytes = Vec::new();
    oxide_proto::varint::write_varint(&mut bytes, 40_000).expect("write length");
    bytes.extend_from_slice(&[b'a'; 40_000]);
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 1_000_000),
        Err(CodecError::TooLong {
            max: MAX_STRING_BYTES,
            ..
        })
    ));
}

/// The UUID the round-trip table uses: a ramp of sixteen distinct bytes, so a
/// transposed byte shows up in a failure.
const UUID: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Round-trips one value through `write` and `read`, pinning the bytes the
/// writer produced to `wire`: a reader that decodes its value from other bytes
/// cannot pass a row.
fn round_trip<T: std::fmt::Debug + PartialEq + Copy>(
    value: T,
    wire: &[u8],
    write: impl FnOnce(&mut Vec<u8>, T) -> std::io::Result<()>,
    read: impl FnOnce(&mut Cursor<Vec<u8>>) -> Result<T, CodecError>,
) {
    let mut out = Vec::new();
    write(&mut out, value).expect("write");
    assert_eq!(out, wire, "wire bytes for {value:?}");
    let mut cursor = Cursor::new(out);
    assert_eq!(
        read(&mut cursor).expect("read"),
        value,
        "round trip of {value:?}"
    );
}

/// Writes a UUID by value, so the round-trip table can carry it like a
/// primitive type.
fn write_uuid_value(out: &mut Vec<u8>, uuid: [u8; 16]) -> std::io::Result<()> {
    write_uuid(out, &uuid)
}

#[test]
fn every_primitive_round_trips_at_its_declared_width() {
    // One row per type: the value, the exact wire bytes, then its writer and
    // reader. Pinning the bytes keeps the hand-mapped widths honest: a reader
    // that decodes its value from other bytes cannot round-trip this table.
    macro_rules! row {
        ($value:expr, $wire:expr, $write:ident, $read:ident) => {
            round_trip(
                $value,
                $wire,
                |out, value| $write(out, value),
                |cursor| $read(cursor),
            )
        };
    }

    row!(0xab_u8, &[0xab], write_u8, read_u8);
    row!(0xabcd_u16, &[0xab, 0xcd], write_u16, read_u16);
    row!(-2_i16, &[0xff, 0xfe], write_i16, read_i16);
    row!(-2_i32, &[0xff, 0xff, 0xff, 0xfe], write_i32, read_i32);
    row!(
        -2_i64,
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe],
        write_i64,
        read_i64
    );
    row!(-2.5_f32, &[0xc0, 0x20, 0x00, 0x00], write_f32, read_f32);
    row!(
        -2.5_f64,
        &[0xc0, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        write_f64,
        read_f64
    );
    row!(true, &[0x01], write_bool, read_bool);
    row!(false, &[0x00], write_bool, read_bool);
    row!(UUID, &UUID, write_uuid_value, read_uuid);
}
