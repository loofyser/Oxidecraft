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
    let mut cursor = &out[..];
    let frame_len = oxide_proto::varint::read_varint(&mut cursor).expect("frame length");
    assert_eq!(
        frame_len as usize,
        cursor.len(),
        "first VarInt is the frame length"
    );
    let data_length = oxide_proto::varint::read_varint(&mut cursor).expect("data length");
    assert_eq!(data_length, 0, "uncompressed marker is Data Length 0");
    assert_eq!(cursor, &payload[..], "payload bytes follow the marker");
}

#[test]
fn payload_at_threshold_is_compressed_and_round_trips() {
    let payload = vec![0x5Au8; 256];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: 256 }).expect("write");
    // At the threshold the frame must carry the compressed branch: a Data
    // Length equal to the payload length, then fewer bytes than the payload.
    let mut cursor = &out[..];
    oxide_proto::varint::read_varint(&mut cursor).expect("frame length");
    let data_length = oxide_proto::varint::read_varint(&mut cursor).expect("data length");
    assert_eq!(
        data_length, 256,
        "at the threshold the payload is compressed"
    );
    assert!(
        cursor.len() < payload.len(),
        "compressed bytes ({}) must be shorter than the payload ({})",
        cursor.len(),
        payload.len()
    );

    let mut cursor = Cursor::new(out);
    let read = read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn a_negative_threshold_uses_plain_framing() {
    // A server threshold of -1 disables compression entirely: no Data Length field is
    // written, exactly as before Set Compression ever arrived.
    let payload = vec![0x11u8; 4096];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: -1 }).expect("write");
    let mut cursor = &out[..];
    let frame_len = oxide_proto::varint::read_varint(&mut cursor).expect("frame length");
    assert_eq!(
        frame_len as usize,
        payload.len(),
        "body is the payload alone"
    );
    assert_eq!(
        cursor,
        &payload[..],
        "no Data Length marker precedes the payload"
    );

    let mut round_trip = &out[..];
    let read = read_frame(&mut round_trip, Compression::Enabled { threshold: -1 }).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn from_server_threshold_maps_negative_to_disabled() {
    assert_eq!(
        Compression::from_server_threshold(-1),
        Compression::Disabled
    );
    assert_eq!(
        Compression::from_server_threshold(-64),
        Compression::Disabled
    );
    assert_eq!(
        Compression::from_server_threshold(0),
        Compression::Enabled { threshold: 0 }
    );
    assert_eq!(
        Compression::from_server_threshold(256),
        Compression::Enabled { threshold: 256 }
    );
}

#[test]
fn from_server_threshold_minus_one_round_trips_as_plain_framing() {
    // The constructor is the entry point for wiring Set Compression: its
    // result frames both directions as plain frames.
    let payload = vec![0x22u8; 512];
    let mode = Compression::from_server_threshold(-1);
    let mut out = Vec::new();
    write_frame(&mut out, &payload, mode).expect("write");
    let mut cursor = Cursor::new(out);
    let read = read_frame(&mut cursor, mode).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn decodes_fixed_compressed_vector() {
    // The frame is built by hand from the fixed vector, so the decoder is
    // pinned independently of our encoder. The declared 15 bytes meet a
    // threshold of 15.
    let mut body = Vec::new();
    write_varint(&mut body, 15).expect("data length");
    body.extend_from_slice(ZLIB_HELLO);

    let mut cursor = Cursor::new(frame_bytes(&body));
    let read = read_frame(&mut cursor, Compression::Enabled { threshold: 15 }).expect("read");
    assert_eq!(read, b"hello minecraft");
}

#[test]
fn reads_zero_length_marker_above_threshold_as_plain_payload() {
    // Data Length 0 marks an uncompressed payload; the threshold check only
    // applies to non-zero declarations.
    let payload = b"hello minecraft";
    let mut body = Vec::new();
    write_varint(&mut body, 0).expect("data length");
    body.extend_from_slice(payload);

    let mut cursor = Cursor::new(frame_bytes(&body));
    let read = read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).expect("read");
    assert_eq!(read, payload);
}

/// Builds a frame around `body` by hand, bypassing `write_frame`.
fn frame_bytes(body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    write_varint(&mut frame, body.len() as i32).expect("frame length");
    frame.extend_from_slice(body);
    frame
}

#[test]
fn rejects_oversized_frame_length() {
    // Hand-built prefix only: the length is refused before any body is read.
    let mut out = Vec::new();
    write_varint(&mut out, MAX_FRAME_LEN as i32 + 1).expect("frame length");

    let mut cursor = Cursor::new(out);
    assert!(matches!(
        read_frame(&mut cursor, Compression::Disabled),
        Err(FrameError::TooLong(len, MAX_FRAME_LEN)) if len == MAX_FRAME_LEN + 1
    ));
}

#[test]
fn rejects_negative_frame_length() {
    // A negative prefix used to be clamped to zero and read as an empty
    // payload; it is never valid.
    let mut out = Vec::new();
    write_varint(&mut out, -1).expect("frame length");

    let mut cursor = Cursor::new(&out);
    assert!(matches!(
        read_frame(&mut cursor, Compression::Disabled),
        Err(FrameError::NegativeLength(-1))
    ));
    cursor.set_position(0);
    assert!(matches!(
        read_frame(&mut cursor, Compression::Enabled { threshold: 256 }),
        Err(FrameError::NegativeLength(-1))
    ));
}

#[test]
fn rejects_declared_data_length_beyond_the_frame_limit() {
    // Hostile declarations: one just past the limit and an absurd one. Each
    // must be rejected before any buffer is sized from the declaration.
    for declared in [MAX_FRAME_LEN as i32 + 1, i32::MAX] {
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
fn rejects_negative_data_length() {
    // A negative declaration is not a size: it is refused by the dedicated
    // variant instead of being cast to a huge one.
    let mut body = Vec::new();
    write_varint(&mut body, -1).expect("data length");
    body.extend_from_slice(ZLIB_HELLO);

    let mut cursor = Cursor::new(frame_bytes(&body));
    assert!(matches!(
        read_frame(&mut cursor, Compression::Enabled { threshold: 256 }),
        Err(FrameError::NegativeLength(-1))
    ));
}

#[test]
fn rejects_compressed_frame_below_the_threshold() {
    // `ZLIB_HELLO` declares 15 bytes, below the 256 threshold: vanilla's
    // decoder refuses such a frame, so ours must too.
    let mut body = Vec::new();
    write_varint(&mut body, 15).expect("data length");
    body.extend_from_slice(ZLIB_HELLO);

    let mut cursor = Cursor::new(frame_bytes(&body));
    assert!(matches!(
        read_frame(&mut cursor, Compression::Enabled { threshold: 256 }),
        Err(FrameError::BadCompression)
    ));
}

#[test]
fn rejects_data_length_that_disagrees_with_decompressed_size() {
    // `ZLIB_HELLO` decompresses to 15 bytes; the frame declares 16, which
    // meets the threshold, so the size comparison is what rejects it.
    let mut body = Vec::new();
    write_varint(&mut body, 16).expect("data length");
    body.extend_from_slice(ZLIB_HELLO);

    let mut cursor = Cursor::new(frame_bytes(&body));
    assert!(matches!(
        read_frame(&mut cursor, Compression::Enabled { threshold: 15 }),
        Err(FrameError::BadCompression)
    ));
}

#[test]
fn rejects_oversized_outbound_payload() {
    // A payload past the limit cannot be read back, in either mode, so the
    // writer refuses it instead of emitting it.
    let payload = vec![0u8; MAX_FRAME_LEN + 1];
    let mut out = Vec::new();
    assert!(matches!(
        write_frame(&mut out, &payload, Compression::Disabled),
        Err(FrameError::TooLong(len, MAX_FRAME_LEN)) if len == MAX_FRAME_LEN + 1
    ));
    assert!(matches!(
        write_frame(&mut out, &payload, Compression::Enabled { threshold: 0 }),
        Err(FrameError::TooLong(len, MAX_FRAME_LEN)) if len == MAX_FRAME_LEN + 1
    ));
    assert!(
        out.is_empty(),
        "nothing is written for an oversized payload"
    );
}

#[test]
fn rejects_writing_an_empty_payload() {
    // A zero-length payload has no representation in compressed mode: the
    // marker would have to be both zero and a length.
    let mut out = Vec::new();
    assert!(matches!(
        write_frame(&mut out, &[], Compression::Disabled),
        Err(FrameError::EmptyPayload)
    ));
    assert!(matches!(
        write_frame(&mut out, &[], Compression::Enabled { threshold: 256 }),
        Err(FrameError::EmptyPayload)
    ));
    assert!(out.is_empty(), "nothing is written for an empty payload");
}
