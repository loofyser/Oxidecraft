//! Replay tests: a scripted 1.8.9 server stream drives the session, and the
//! client's own traffic and events are asserted byte for byte.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use oxide_game::session::{ClientEvent, Session, SessionConfig, SessionError};
use oxide_proto::conn::Conn;
use oxide_proto::frame::{Compression, write_frame};
use oxide_proto_v47::clientbound::PlayerPositionAndLook;
use oxide_proto_v47::serverbound::ClientSettings;

/// The session config the scripts are written against.
fn config() -> SessionConfig {
    SessionConfig {
        host: "127.0.0.1".into(),
        port: 25565,
        username: "OxideDev".into(),
        settings: ClientSettings::default(),
    }
}

/// The framing the scripted server switches to when it sends Set Compression.
const SERVER_FRAMING: Compression = Compression::Enabled { threshold: 256 };

/// A duplex stream: what the client writes is inspected, what the script put
/// there is read back.
struct Duplex {
    incoming: std::io::Cursor<Vec<u8>>,
    outgoing: Arc<Mutex<Vec<u8>>>,
}

impl Read for Duplex {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.incoming.read(out)
    }
}

impl Write for Duplex {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.outgoing.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A duplex carrying `script`, with its writes handed back for inspection.
fn duplex(script: Vec<u8>) -> (Duplex, Arc<Mutex<Vec<u8>>>) {
    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = Duplex {
        incoming: std::io::Cursor::new(script),
        outgoing: Arc::clone(&outgoing),
    };
    (stream, outgoing)
}

/// Appends one scripted server frame, framed as the server frames it.
fn frame(out: &mut Vec<u8>, payload: &[u8], compression: Compression) {
    write_frame(out, payload, compression).expect("a scripted frame");
}

/// Writes a VarInt into a payload under construction.
fn push_varint(out: &mut Vec<u8>, value: i32) {
    oxide_proto::varint::write_varint(out, value).expect("writing to a Vec cannot fail");
}

/// Writes a length-prefixed string into a payload under construction.
fn push_string(out: &mut Vec<u8>, value: &str) {
    oxide_proto::codec::write_string(out, value).expect("writing to a Vec cannot fail");
}

/// The login sequence every script starts with: Set Compression in the plain
/// framing, then Login Success in the compressed framing it switched on. The
/// capture fixes the order and both framings.
fn login_sequence(out: &mut Vec<u8>) {
    frame(out, &[0x03, 0x80, 0x02], Compression::Disabled);
    // Two length-prefixed strings, the UUID then the name.
    let mut login_success = vec![0x02, 0x24];
    login_success.extend_from_slice(b"069a79f4-44e9-4726-a5be-fca90e38aaf5");
    login_success.push(0x08);
    login_success.extend_from_slice(b"OxideDev");
    frame(out, &login_success, SERVER_FRAMING);
}

/// The login sequence, then each of `payloads` under the server's framing.
fn stream_with(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    login_sequence(&mut out);
    for payload in payloads {
        frame(&mut out, payload, SERVER_FRAMING);
    }
    out
}

/// The scripted stream the replay drives: the login sequence, then Join Game, a
/// teleport, a keepalive, and one column.
fn scripted_server_stream() -> Vec<u8> {
    stream_with(&[
        join_game_frame(),
        position_frame(0.5, 65.0, -12.5, 0.0, 0.0, 0),
        keep_alive_frame(7),
        chunk_data_frame(0, 0),
    ])
}

/// Join Game: entity 20, survival, the overworld, difficulty 1, 20 players, the
/// `default` level type.
fn join_game_frame() -> Vec<u8> {
    let mut join = vec![0x01];
    join.extend_from_slice(&20i32.to_be_bytes());
    join.extend_from_slice(&[0, 0, 1, 20, 7]);
    join.extend_from_slice(b"default");
    join.push(0);
    join
}

/// Player Position And Look, with `flags` naming the relative axes.
fn position_frame(x: f64, y: f64, z: f64, yaw: f32, pitch: f32, flags: u8) -> Vec<u8> {
    let mut position = vec![0x08];
    position.extend_from_slice(&x.to_be_bytes());
    position.extend_from_slice(&y.to_be_bytes());
    position.extend_from_slice(&z.to_be_bytes());
    position.extend_from_slice(&yaw.to_be_bytes());
    position.extend_from_slice(&pitch.to_be_bytes());
    position.push(flags);
    position
}

/// Keep Alive carrying `id`.
fn keep_alive_frame(id: i32) -> Vec<u8> {
    let mut keep_alive = vec![0x00];
    push_varint(&mut keep_alive, id);
    keep_alive
}

/// One ground-up Chunk Data column with a single stone block at its origin,
/// shaped as the capture records a column: the block light and sky light of the
/// section the mask selects, then the biome array.
fn chunk_data_frame(cx: i32, cz: i32) -> Vec<u8> {
    let mut column = Vec::new();
    column.extend_from_slice(&[0u8; 8192]);
    column[..2].copy_from_slice(&0x0010u16.to_le_bytes());
    column.extend_from_slice(&[0u8; 2048]); // block light
    column.extend_from_slice(&[0xFFu8; 2048]); // sky light
    column.extend_from_slice(&[1u8; 256]); // biomes
    let mut chunk = vec![0x21];
    chunk.extend_from_slice(&cx.to_be_bytes());
    chunk.extend_from_slice(&cz.to_be_bytes());
    chunk.push(1); // ground up
    chunk.extend_from_slice(&0x0001u16.to_be_bytes());
    push_varint(&mut chunk, column.len() as i32);
    chunk.extend_from_slice(&column);
    chunk
}

/// The unload shape: a ground-up Chunk Data with an empty mask and no data.
fn chunk_unload_frame(cx: i32, cz: i32) -> Vec<u8> {
    let mut chunk = vec![0x21];
    chunk.extend_from_slice(&cx.to_be_bytes());
    chunk.extend_from_slice(&cz.to_be_bytes());
    chunk.push(1); // ground up
    chunk.extend_from_slice(&0x0000u16.to_be_bytes());
    push_varint(&mut chunk, 0);
    chunk
}

/// Player List Item, add action, for one entry.
fn player_list_add_frame(uuid: [u8; 16], name: &str) -> Vec<u8> {
    let mut payload = vec![0x38];
    push_varint(&mut payload, 0); // the add action
    push_varint(&mut payload, 1); // one entry
    payload.extend_from_slice(&uuid);
    push_string(&mut payload, name);
    push_varint(&mut payload, 0); // no properties
    push_varint(&mut payload, 0); // gamemode
    push_varint(&mut payload, 5); // ping
    payload.push(0); // no display name
    payload
}

/// Player List Item, update-latency action, the shape a live server sends
/// routinely.
fn player_list_ping_frame(uuid: [u8; 16], ping: i32) -> Vec<u8> {
    let mut payload = vec![0x38];
    push_varint(&mut payload, 2); // the update-latency action
    push_varint(&mut payload, 1); // one entry
    payload.extend_from_slice(&uuid);
    push_varint(&mut payload, ping);
    payload
}

/// Plugin Message on `channel` with `data` as its payload.
fn plugin_message_frame(channel: &str, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x3f];
    push_string(&mut payload, channel);
    payload.extend_from_slice(data);
    payload
}

/// Reads one frame out of the client's own traffic, under the framing that was
/// in force when the client wrote it.
fn client_frame(cursor: &mut &[u8], compression: Compression) -> Vec<u8> {
    oxide_proto::frame::read_frame(cursor, compression).expect("a client frame")
}

/// The column a ChunkUpdated event reports, panicking on any other event.
fn updated_column(event: &ClientEvent) -> (i32, i32) {
    match event {
        ClientEvent::ChunkUpdated { cx, cz, .. } => (*cx, *cz),
        other => panic!("expected a chunk update, got {other:?}"),
    }
}

#[test]
fn the_session_logs_in_joins_and_answers_every_obligation() {
    let (stream, outgoing) = duplex(scripted_server_stream());
    let (sender, receiver) = crossbeam_channel::unbounded();
    let session = Session::new(Conn::new(stream), config());
    session
        .run_over(&sender)
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert!(matches!(events[0], ClientEvent::LoggedIn { .. }));
    assert!(matches!(
        events[1],
        ClientEvent::Joined {
            entity_id: 20,
            dimension: 0,
            ..
        }
    ));
    assert!(matches!(events[2], ClientEvent::PlayerPosition { x, .. } if x == 0.5));
    assert!(matches!(events[3], ClientEvent::KeepAlive { id: 7 }));
    match &events[4] {
        ClientEvent::ChunkUpdated {
            cx: 0,
            cz: 0,
            sections,
        } => {
            let mesh = sections[0].1.as_ref().expect("section 0 draws");
            assert_eq!(mesh.vertices.len(), 24, "one stone block, six faces");
        }
        other => panic!("expected a chunk update, got {other:?}"),
    }
    // The applied column leads its four neighbours, in the fixed order the
    // session re-meshes them: +x, -x, +z, -z.
    assert_eq!(
        events.len(),
        9,
        "the applied column and its four neighbours: {events:?}"
    );
    let columns: Vec<(i32, i32)> = events[4..].iter().map(updated_column).collect();
    assert_eq!(
        columns,
        vec![(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)],
        "the applied column leads, then its four neighbours"
    );

    let written = outgoing.lock().unwrap().clone();
    // Replay the client's traffic under the same framing and check the packets.
    let mut cursor = &written[..];
    // The handshake body, byte for byte: the id, protocol 47, the configured
    // host, the port big endian, and next state 2 for login.
    let handshake = client_frame(&mut cursor, Compression::Disabled);
    assert_eq!(
        handshake, b"\x00\x2f\x09127.0.0.1\x63\xdd\x02",
        "handshake first"
    );
    let login_start = client_frame(&mut cursor, Compression::Disabled);
    assert_eq!(login_start, b"\x00\x08OxideDev");
    // Everything after Set Compression is compressed framing, even when small.
    // The settings body, byte for byte: the id, the locale, the view distance,
    // the chat mode, chat colours and the skin parts.
    let settings = client_frame(&mut cursor, SERVER_FRAMING);
    assert_eq!(settings, b"\x15\x05en_US\x08\x00\x01\x7f");
    let brand = client_frame(&mut cursor, SERVER_FRAMING);
    assert_eq!(&brand[..2], b"\x17\x08");
    // The payload is a length-prefixed string, exactly as the capture records
    // it: 07 "vanilla".
    assert_eq!(&brand[2..], b"MC|Brand\x07vanilla");
    let echo = client_frame(&mut cursor, SERVER_FRAMING);
    assert_eq!(echo[0], 0x06);
    let keep_alive = client_frame(&mut cursor, SERVER_FRAMING);
    assert_eq!(keep_alive, [0x00, 0x07]);
    assert!(cursor.is_empty(), "no further packets were sent");
}

#[test]
fn a_relative_teleport_is_answered_with_absolute_values() {
    let (stream, outgoing) = duplex(stream_with(&[
        join_game_frame(),
        position_frame(10.0, 64.0, 20.0, 90.0, 0.0, 0),
        // x and z are deltas on the position the first teleport settled on:
        // 0x05 is their literal flag bits, x 0x01 and z 0x04. y, yaw and pitch
        // are unflagged, so they arrive absolute.
        position_frame(0.5, 1.0, -2.5, 45.0, -10.0, 0x05),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender)
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    match events.last() {
        Some(ClientEvent::PlayerPosition {
            x,
            y,
            z,
            yaw,
            pitch,
        }) => {
            assert_eq!(
                (*x, *y, *z, *yaw, *pitch),
                (10.5, 1.0, 17.5, 45.0, -10.0),
                "the deltas are applied and the absolutes are taken as they arrived"
            );
        }
        other => panic!("expected the second teleport, got {other:?}"),
    }

    // The echo carries the resolved position, not the deltas that arrived.
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    client_frame(&mut cursor, SERVER_FRAMING); // the first echo
    let echo = client_frame(&mut cursor, SERVER_FRAMING);
    assert_eq!(echo[0], 0x06, "the teleport is answered");
    let echoed = PlayerPositionAndLook::decode(&echo[1..]).expect("the echo decodes");
    assert_eq!(
        (echoed.x, echoed.y, echoed.z, echoed.yaw, echoed.pitch),
        (10.5, 1.0, 17.5, 45.0, -10.0)
    );
}

#[test]
fn an_unload_packet_removes_the_column() {
    // Preceded by the same login sequence, then a ground-up chunk with mask 0.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        chunk_data_frame(3, 4),
        chunk_unload_frame(3, 4),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender)
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    // The applied column leads its four neighbours, in the fixed order the
    // session re-meshes them: +x, -x, +z, -z, before the unload.
    assert_eq!(
        events.len(),
        8,
        "the login, the join, five chunk updates and the unload: {events:?}"
    );
    match &events[2] {
        ClientEvent::ChunkUpdated {
            cx: 3,
            cz: 4,
            sections,
        } => {
            assert!(
                sections[0].1.is_some(),
                "the column is meshed before it is unloaded"
            );
        }
        other => panic!("expected the meshed column, got {other:?}"),
    }
    let columns: Vec<(i32, i32)> = events[2..7].iter().map(updated_column).collect();
    assert_eq!(
        columns,
        vec![(3, 4), (4, 4), (2, 4), (3, 5), (3, 3)],
        "the applied column leads, then its four neighbours"
    );
    match events.last() {
        Some(ClientEvent::ChunkUnloaded { cx, cz }) => assert_eq!((*cx, *cz), (3, 4)),
        other => panic!("expected the unload, got {other:?}"),
    }
}

#[test]
fn a_malformed_packet_is_an_error_not_a_panic() {
    // A Join Game frame cut short: the entity id, then nothing.
    let mut join = vec![0x01];
    join.extend_from_slice(&20i32.to_be_bytes());
    let (stream, _outgoing) = duplex(stream_with(&[join]));
    let (sender, _receiver) = crossbeam_channel::unbounded();
    let error = Session::new(Conn::new(stream), config())
        .run_over(&sender)
        .expect_err("a cut short Join Game must be reported");
    assert!(matches!(error, SessionError::Packet(_)), "error: {error:?}");
}

#[test]
fn a_server_disconnect_reports_its_reason() {
    let reason = "{\"text\":\"the server closed the session\"}";
    let mut disconnect = vec![0x40];
    push_string(&mut disconnect, reason);
    let (stream, _outgoing) = duplex(stream_with(&[join_game_frame(), disconnect]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender)
        .expect("a disconnect ends the session cleanly");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    match events.last() {
        Some(ClientEvent::Disconnected { reason: reported }) => assert_eq!(reported, reason),
        other => panic!("expected the disconnect reason, got {other:?}"),
    }
}

#[test]
fn packets_m1_does_not_use_are_skipped_not_fatal() {
    // An id with no codec, a plugin message, and the play-state Set Compression
    // the reference calls broken: none of them may end the session, and the
    // keepalive that follows must still be answered.
    let (stream, outgoing) = duplex(stream_with(&[
        join_game_frame(),
        vec![0x7f, 0x00],
        plugin_message_frame("MC|Brand", b"\x07vanilla"),
        vec![0x46, 0x80, 0x02],
        keep_alive_frame(11),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender)
        .expect("packets M1 has no use for do not end the session");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    match events.last() {
        Some(ClientEvent::KeepAlive { id: 11 }) => {}
        other => panic!("expected the keepalive to be answered, got {other:?}"),
    }
    // Nothing was sent for the skipped packets: the keepalive echo is the
    // fifth packet, right after the settings and the brand.
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    assert_eq!(
        client_frame(&mut cursor, SERVER_FRAMING),
        [0x00, 0x0b],
        "the keepalive is the only packet those skips drew"
    );
    assert!(cursor.is_empty(), "no further packets were sent");
}

#[test]
fn player_list_updates_do_not_end_the_session() {
    // An add entry, then the latency update a live server sends routinely.
    let uuid = [7u8; 16];
    let (stream, outgoing) = duplex(stream_with(&[
        join_game_frame(),
        player_list_add_frame(uuid, "OxideDev"),
        player_list_ping_frame(uuid, 42),
        keep_alive_frame(12),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender)
        .expect("player list traffic does not end the session");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    match events.last() {
        Some(ClientEvent::KeepAlive { id: 12 }) => {}
        other => panic!("expected the keepalive to be answered, got {other:?}"),
    }
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    assert_eq!(
        client_frame(&mut cursor, SERVER_FRAMING),
        [0x00, 0x0c],
        "the keepalive is the only packet the player list drew"
    );
    assert!(cursor.is_empty(), "no further packets were sent");
}
