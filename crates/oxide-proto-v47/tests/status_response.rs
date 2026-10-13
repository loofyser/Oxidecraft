//! Canned-server tests for the status exchange: what the client puts on the wire,
//! and what it makes of each reply a server can produce.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use oxide_proto::frame::{Compression, read_frame, write_frame};
use oxide_proto::varint::write_varint;
use oxide_proto_v47::handshake::handshake_payload;
use oxide_proto_v47::status::{
    PingError, StatusResponse, parse_status_json, ping_server, ping_with_latency,
};

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

/// A status reply whose player sample and styled description the ping must keep.
const SAMPLE_DESCRIPTION_JSON: &str = concat!(
    r#"{"version":{"name":"1.8.9","protocol":47},"#,
    r#""players":{"max":20,"online":3,"sample":[{"name":"Ada","id":"069a79f4-44e9-4726-a5be-fca90e38aaf5"},{"name":"Grace","id":"61699b2e-d327-4a01-9f1e-0ea8c3f06bc6"}]},"#,
    r#""description":{"text":"Hi","color":"red"}}"#
);

/// The ping-pong round trip (`OldServerPinger.java`): after the status reply the client
/// sends C01 carrying its own millisecond clock, the server echoes it in S01, and the
/// latency is `pong_received - ping_sent` — not the whole TCP attempt.
#[test]
fn the_ping_round_trip_sends_client_ms_and_measures_latency() {
    use std::sync::mpsc;
    let (seen_tx, seen_rx) = mpsc::channel();
    let (port, handle) = fake_server(move |stream| {
        let payload = status_payload(OBJECT_DESCRIPTION_JSON);
        write_frame(&mut *stream, &payload, Compression::Disabled).expect("write status response");
        let ping = read_frame(&mut *stream, Compression::Disabled).expect("ping frame");
        seen_tx.send(ping.clone()).expect("report the ping frame");
        let mut pong = vec![0x01];
        pong.extend_from_slice(&ping[1..]);
        write_frame(&mut *stream, &pong, Compression::Disabled).expect("write pong");
    });
    let before = system_ms();
    let (status, latency) =
        ping_with_latency("127.0.0.1", port, Duration::from_secs(5)).expect("ping round trip");
    let after = system_ms();
    let ping = seen_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the ping frame");
    handle.join().expect("fake server thread");

    assert_eq!(
        ping.len(),
        9,
        "the ping is the id byte plus eight client-ms bytes"
    );
    assert_eq!(ping[0], 0x01, "the ping's packet id");
    let sent_ms = i64::from_be_bytes(ping[1..9].try_into().expect("eight bytes"));
    assert!(
        (before..=after).contains(&sent_ms),
        "the ping carries the client's own clock: {sent_ms} within {before}..={after}"
    );
    assert!(latency >= 0, "the latency is a non-negative round trip");
    assert!(
        latency <= after - before,
        "the latency times the pong, not the whole attempt: {latency} within the attempt"
    );
    assert_eq!(status.version.protocol, 47);
}

/// The client's millisecond clock behind the C01 payload.
fn system_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock runs")
        .as_millis()
        .try_into()
        .expect("the millisecond clock fits")
}

/// The player sample and the raw description survive the parse: the sample's names and ids,
/// and the description's JSON byte-for-byte in compact form for the styled MOTD path.
#[test]
fn the_player_sample_and_the_raw_description_survive_the_parse() {
    let status = parse_status_json(SAMPLE_DESCRIPTION_JSON.as_bytes()).expect("sample reply");
    let sample = &status.players.sample;
    assert_eq!(sample.len(), 2, "both sampled players are kept");
    assert_eq!(sample[0].name, "Ada");
    assert_eq!(sample[0].id, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    assert_eq!(sample[1].name, "Grace");
    // The map's keys sort alphabetically through `serde_json::Value`; the
    // styled path reads the object, never the key order, so the sorted
    // compact form is the preserved shape.
    assert_eq!(
        status.description_raw().as_deref(),
        Some(r#"{"color":"red","text":"Hi"}"#),
        "the raw description is preserved for the styled path"
    );
}
