//! Tests for the primitive and string codecs, including their hostile-input paths.

use std::io::Cursor;

use oxide_proto::codec::{
    CodecError, MAX_STRING_BYTES, read_bool, read_i32, read_string, read_u16, write_i32,
    write_string,
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
