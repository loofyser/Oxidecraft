//! Round-trip and hostile-input tests for length-prefixed framing.

use std::io::Cursor;

use oxide_proto::frame::{Compression, FrameError, MAX_FRAME_LEN, read_frame, write_frame};
use oxide_proto::varint::write_varint;

/// zlib stream (RFC 1950) for the 15 bytes `hello minecraft`.
const ZLIB_HELLO: &[u8] = &[
    0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0xcd, 0xcc, 0x4b, 0x4d, 0x2e, 0x4a, 0x4c,
    0x2b, 0x01, 0x00, 0x2e, 0xd5, 0x05, 0xee,
];

#[test]
fn round_trips_uncompressed_when_disabled() {
    let payload = b"hello minecraft";
    let mut out = Vec::new();
    write_frame(&mut out, payload, Compression::Disabled).expect("write");
    let mut cursor = Cursor::new(out);
    let read = read_frame(&mut cursor, Compression::Disabled).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn small_payload_gets_zero_length_marker_above_threshold() {
    // Payload below the threshold travels as: frame length, zero, raw bytes.
    let payload = [0xABu8; 10];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: 256 }).expect("write");
    assert_eq!(
        out[0] as usize,
        out.len() - 1,
        "first VarInt is the frame length"
    );
    let data_length = oxide_proto::varint::read_varint(&out[1..]).expect("data length");
    assert_eq!(data_length, 0, "uncompressed marker is Data Length 0");
}

#[test]
fn payload_at_threshold_is_compressed_and_round_trips() {
    let payload = vec![0x5Au8; 256];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: 256 }).expect("write");
    let mut cursor = Cursor::new(out);
    let read = read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn threshold_minus_one_never_compresses() {
    let payload = vec![0x11u8; 4096];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: -1 }).expect("write");
    // A 4 KiB payload makes the frame length VarInt two bytes wide, so step
    // past it before reading the Data Length marker.
    let mut cursor = &out[..];
    oxide_proto::varint::read_varint(&mut cursor).expect("frame length");
    let data_length = oxide_proto::varint::read_varint(&mut cursor).expect("data length");
    assert_eq!(data_length, 0);
}

/// Builds a frame around `body` by hand, bypassing `write_frame`.
fn frame_bytes(body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    write_varint(&mut frame, body.len() as i32).expect("frame length");
    frame.extend_from_slice(body);
    frame
}

#[test]
fn rejects_declared_data_length_beyond_the_frame_limit() {
    // Hostile declarations: a negative one (which would cast to a huge size),
    // one just past the limit, and an absurd one. Each must be rejected before
    // any buffer is sized from the declaration.
    for declared in [-1, MAX_FRAME_LEN as i32 + 1, i32::MAX] {
        let mut body = Vec::new();
        write_varint(&mut body, declared).expect("data length");
        body.extend_from_slice(ZLIB_HELLO);

        let mut cursor = Cursor::new(frame_bytes(&body));
        assert!(
            matches!(
                read_frame(&mut cursor, Compression::Enabled { threshold: 256 }),
                Err(FrameError::BadCompression)
            ),
            "declared data length {declared} must be rejected"
        );
    }
}

#[test]
fn rejects_data_length_that_disagrees_with_decompressed_size() {
    // `ZLIB_HELLO` decompresses to 15 bytes; the frame declares 16.
    let mut body = Vec::new();
    write_varint(&mut body, 16).expect("data length");
    body.extend_from_slice(ZLIB_HELLO);

    let mut cursor = Cursor::new(frame_bytes(&body));
    assert!(matches!(
        read_frame(&mut cursor, Compression::Enabled { threshold: 256 }),
        Err(FrameError::BadCompression)
    ));
}
