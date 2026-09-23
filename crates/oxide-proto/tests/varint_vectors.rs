//! Golden-vector and round-trip tests for the VarInt codec.

use oxide_proto::varint::{VarIntError, read_varint, write_varint};

/// Boundary-complete golden vectors: 0 and 1, the 127/128 continuation boundary,
/// a two-byte value, i32::MAX, and a negative value that encodes as five bytes.
const VECTORS: &[(i32, &[u8])] = &[
    (0, &[0x00]),
    (1, &[0x01]),
    (127, &[0x7f]),
    (128, &[0x80, 0x01]),
    (255, &[0xff, 0x01]),
    (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
    (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
];

#[test]
fn encodes_known_vectors() {
    for (value, expected) in VECTORS {
        let mut out = Vec::new();
        write_varint(&mut out, *value).expect("write");
        assert_eq!(&out, expected, "encoding {value}");
    }
}

#[test]
fn decodes_known_vectors() {
    for (value, bytes) in VECTORS {
        let mut cursor = *bytes;
        assert_eq!(
            read_varint(&mut cursor).expect("read"),
            *value,
            "decoding {value}"
        );
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
    // Five continuation bytes, one past the five-byte limit.
    let bytes = [0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    let mut cursor = &bytes[..];
    assert!(matches!(
        read_varint(&mut cursor),
        Err(VarIntError::TooLong)
    ));
}

#[test]
fn reports_truncated_input_as_unexpected_eof() {
    // A continuation byte with nothing after it is a truncated VarInt.
    let bytes = [0x80];
    let mut cursor = &bytes[..];
    assert!(matches!(
        read_varint(&mut cursor),
        Err(VarIntError::UnexpectedEof)
    ));
}

#[test]
fn propagates_non_eof_io_errors() {
    struct FailingReader;

    impl std::io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "connection reset",
            ))
        }
    }

    let mut reader = FailingReader;
    assert!(matches!(read_varint(&mut reader), Err(VarIntError::Io(_))));
}
