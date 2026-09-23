//! Canned-server tests for the status exchange: what the client puts on the wire,
//! and what it makes of each reply a server can produce.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use oxide_proto::frame::{Compression, read_frame, write_frame};
use oxide_proto::varint::write_varint;
use oxide_proto_v47::handshake::handshake_payload;
use oxide_proto_v47::status::{PingError, StatusResponse, ping_server};

/// A status response whose description is a chat component object.
const OBJECT_DESCRIPTION_JSON: &str = concat!(
    r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":0},"#,
    r#""description":{"text":"A Minecraft Server"},"favicon":"data:image/png;base64,AAAA"}"#
);

/// A status response whose description is a plain JSON string, the form vanilla
/// sends for an unformatted MOTD.
const PLAIN_DESCRIPTION_JSON: &str = concat!(
    r#"{"description":"Oxidecraft test server","players":{"max":20,"online":0},"#,
    r#""version":{"name":"1.8.9","protocol":47}}"#
);

/// A status response whose description is an array of chat components, a shape
/// this client reads no plain-text MOTD from.
const ARRAY_DESCRIPTION_JSON: &str = concat!(
    r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":0},"#,
    r#""description":["Oxidecraft","test server"]}"#
);

/// A status response whose description is an object whose `text` is not a string.
const NON_STRING_TEXT_DESCRIPTION_JSON: &str = concat!(
    r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":3},"#,
    r#""description":{"text":42}}"#
);

/// What a fake server read from the client before it answered.
#[derive(Debug)]
struct Seen {
    handshake: Vec<u8>,
    request: Vec<u8>,
}

/// Starts a one-shot server on a loopback port: it accepts one connection, reads
/// the handshake and status request frames, then hands the socket to `reply`.
fn fake_server<F>(reply: F) -> (u16, JoinHandle<Seen>)
where
    F: FnOnce(&mut TcpStream) + Send + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().expect("local address").port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let handshake = read_frame(&mut stream, Compression::Disabled).expect("handshake frame");
        let request = read_frame(&mut stream, Compression::Disabled).expect("request frame");
        reply(&mut stream);
        Seen { handshake, request }
    });
    (port, handle)
}

/// Serves one canned reply and returns the port used, the client's result, and
/// the bytes the client sent.
fn ping_against<F>(reply: F) -> (u16, Result<StatusResponse, PingError>, Seen)
where
    F: FnOnce(&mut TcpStream) + Send + 'static,
{
    let (port, handle) = fake_server(reply);
    let result = ping_server("127.0.0.1", port, Duration::from_secs(5));
    (port, result, handle.join().expect("fake server thread"))
}

/// Builds a Status Response payload: packet id, VarInt JSON length, then the JSON.
fn status_payload(json: &str) -> Vec<u8> {
    let mut payload = vec![0x00];
    write_varint(&mut payload, json.len() as i32).expect("write length");
    payload.extend_from_slice(json.as_bytes());
    payload
}

#[test]
fn status_response_is_parsed_from_a_canned_reply() {
    let (port, result, seen) = ping_against(move |stream| {
        let payload = status_payload(OBJECT_DESCRIPTION_JSON);
        write_frame(stream, &payload, Compression::Disabled).expect("write status response");
    });
    let status = result.expect("status response");

    assert_eq!(status.version.name, "1.8.9");
    assert_eq!(status.version.protocol, 47);
    assert_eq!(status.players.max, 20);
    assert_eq!(status.players.online, 0);
    assert_eq!(
        status
            .description
            .and_then(|description| description.text())
            .as_deref(),
        Some("A Minecraft Server")
    );
    assert_eq!(
        status.favicon.as_deref(),
        Some("data:image/png;base64,AAAA")
    );

    assert_eq!(
        seen.request,
        vec![0x00],
        "a Status Request is the id byte alone"
    );
    assert_eq!(seen.handshake, handshake_payload(47, "127.0.0.1", port, 1));
}

#[test]
fn plain_string_description_is_accepted() {
    let (_, result, _) = ping_against(move |stream| {
        let payload = status_payload(PLAIN_DESCRIPTION_JSON);
        write_frame(stream, &payload, Compression::Disabled).expect("write status response");
    });
    let status = result.expect("status response");

    assert_eq!(
        status
            .description
            .and_then(|description| description.text())
            .as_deref(),
        Some("Oxidecraft test server")
    );
}

#[test]
fn unexpected_packet_id_is_rejected() {
    let (_, result, _) = ping_against(|stream| {
        write_frame(stream, &[0x7f, 0x00], Compression::Disabled).expect("write response");
    });

    assert!(matches!(result, Err(PingError::UnexpectedPacket(0x7f))));
}

#[test]
fn empty_frame_is_rejected_as_truncated() {
    let (_, result, _) = ping_against(|stream| {
        // A zero-length frame: the client reads no packet id at all.
        stream.write_all(&[0x00]).expect("write empty frame");
    });

    assert!(matches!(result, Err(PingError::Truncated)));
}

#[test]
fn truncated_body_is_rejected() {
    let (_, result, _) = ping_against(|stream| {
        // Declares a 127 byte JSON body, then sends none of it.
        write_frame(stream, &[0x00, 0x7f], Compression::Disabled).expect("write response");
    });

    assert!(matches!(result, Err(PingError::Truncated)));
}

#[test]
fn malformed_json_is_reported_as_a_json_error() {
    let (_, result, _) = ping_against(|stream| {
        write_frame(stream, &[0x00, 0x01, b'{'], Compression::Disabled).expect("write response");
    });

    assert!(matches!(result, Err(PingError::Json(_))));
}

#[test]
fn connection_closed_before_a_response_is_reported_as_closed() {
    let (_, result, _) = ping_against(|_| {});

    assert!(matches!(result, Err(PingError::Closed)));
}

#[test]
fn silent_server_is_reported_as_a_timeout() {
    let (port, handle) = fake_server(|_| {
        // Hold the connection open, without answering, well past the client's
        // deadline, so the read times out instead of seeing the server close.
        thread::sleep(Duration::from_secs(1));
    });
    let result = ping_server("127.0.0.1", port, Duration::from_millis(100));
    handle.join().expect("fake server thread");

    match result {
        Err(PingError::Timeout(reported)) => assert_eq!(reported, Duration::from_millis(100)),
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[test]
fn a_spent_deadline_is_reported_as_a_timeout() {
    // A zero-length socket timeout is not settable, so a deadline that leaves
    // no time must be reported as a timeout instead of reaching the platform.
    // No connection is attempted: the answer is immediate.
    let result = ping_server("127.0.0.1", 1, Duration::ZERO);
    match result {
        Err(PingError::Timeout(reported)) => assert_eq!(reported, Duration::ZERO),
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[test]
fn an_unrepresentably_large_timeout_does_not_panic() {
    // A caller-supplied `Duration` beyond what the clock can represent must
    // not panic the deadline arithmetic; the exchange itself is a normal one.
    let (port, handle) = fake_server(|stream| {
        let payload = status_payload(PLAIN_DESCRIPTION_JSON);
        write_frame(stream, &payload, Compression::Disabled).expect("write status response");
    });
    let result = ping_server("127.0.0.1", port, Duration::MAX);
    let status = result.expect("status response");
    assert_eq!(status.version.protocol, 47);
    handle.join().expect("fake server thread");
}

#[test]
fn array_description_yields_version_and_players_without_a_motd() {
    let (_, result, _) = ping_against(move |stream| {
        let payload = status_payload(ARRAY_DESCRIPTION_JSON);
        write_frame(stream, &payload, Compression::Disabled).expect("write status response");
    });
    let status = result.expect("status response");

    assert_eq!(status.version.name, "1.8.9");
    assert_eq!(status.version.protocol, 47);
    assert_eq!(status.players.max, 20);
    assert_eq!(status.players.online, 0);
    assert_eq!(
        status
            .description
            .and_then(|description| description.text()),
        None
    );
}

#[test]
fn description_with_a_non_string_text_yields_no_motd() {
    let (_, result, _) = ping_against(move |stream| {
        let payload = status_payload(NON_STRING_TEXT_DESCRIPTION_JSON);
        write_frame(stream, &payload, Compression::Disabled).expect("write status response");
    });
    let status = result.expect("status response");

    assert_eq!(status.version.protocol, 47);
    assert_eq!(status.players.online, 3);
    assert_eq!(
        status
            .description
            .and_then(|description| description.text()),
        None
    );
}

#[test]
fn literal_bytes_reply_is_parsed() {
    // The successful reply as hand-written wire bytes: the frame length prefix,
    // the packet id, the JSON length prefix, then the JSON. Only the two length
    // prefixes are computed, and `write_frame` is never used, so a mistake the
    // writer and the reader share cannot cancel out in a round trip.
    const LITERAL_JSON: &[u8] = b"{\"version\":{\"name\":\"1.8.9\",\"protocol\":47},\"players\":{\"max\":20,\"online\":2},\"description\":\"literal bytes\"}";

    let (_, result, _) = ping_against(move |stream| {
        let mut payload = vec![0x00];
        write_varint(&mut payload, LITERAL_JSON.len() as i32).expect("write JSON length");
        payload.extend_from_slice(LITERAL_JSON);

        let mut frame = Vec::new();
        write_varint(&mut frame, payload.len() as i32).expect("write frame length");
        frame.extend_from_slice(&payload);
        stream.write_all(&frame).expect("write literal reply");
    });

    let status = result.expect("status response");
    assert_eq!(status.version.name, "1.8.9");
    assert_eq!(status.version.protocol, 47);
    assert_eq!(status.players.max, 20);
    assert_eq!(status.players.online, 2);
    assert_eq!(
        status
            .description
            .and_then(|description| description.text())
            .as_deref(),
        Some("literal bytes")
    );
}

#[test]
fn ping_succeeds_when_the_first_resolved_address_refuses() {
    // `localhost` resolves to more than one address on most hosts (`::1` and
    // `127.0.0.1` here) and the fake server listens on one of them, so the
    // exchange only succeeds if the addresses after the first are tried too.
    let (port, handle) = fake_server(|stream| {
        let payload = status_payload(PLAIN_DESCRIPTION_JSON);
        write_frame(stream, &payload, Compression::Disabled).expect("write status response");
    });
    let result = ping_server("localhost", port, Duration::from_secs(5));

    // Assert before joining: if every resolved address is refused, the server
    // thread is still waiting to accept, and joining it would hang rather than
    // report the failure.
    let status = result.expect("status response");
    assert_eq!(status.version.protocol, 47);
    handle.join().expect("fake server thread");
}

#[test]
fn overlong_json_length_is_reported_as_a_bad_length() {
    let (_, result, _) = ping_against(|stream| {
        // A length prefix whose five bytes all carry the continuation bit: the
        // reader refuses it rather than looking for a sixth.
        write_frame(
            stream,
            &[0x00, 0x80, 0x80, 0x80, 0x80, 0x80],
            Compression::Disabled,
        )
        .expect("write response");
    });

    assert!(matches!(result, Err(PingError::BadLength(_))));
}
