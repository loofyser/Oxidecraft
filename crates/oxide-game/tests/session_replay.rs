//! Replay tests: a scripted 1.8.9 server stream drives the session, and the
//! client's own traffic and events are asserted byte for byte.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oxide_game::input::{InputEvent, Key, MouseButton};
use oxide_game::session::{ClientEvent, Session, SessionConfig, SessionError};
use oxide_game::ticker::TICK_CATCHUP_CAP;
use oxide_proto::conn::{Conn, DeadlineStream};
use oxide_proto::frame::{Compression, write_frame};
use oxide_proto_v47::clientbound::{MapChunkBulk, PlayerPositionAndLook};
use oxide_proto_v47::column::block_index;
use oxide_proto_v47::serverbound::ClientSettings;
use oxide_render::terrain::{ChunkMesh, Vertex};
use oxide_world::world::World;

/// The session config the scripts are written against.
fn config() -> SessionConfig {
    SessionConfig {
        host: "127.0.0.1".into(),
        port: 25565,
        username: "OxideDev".into(),
        settings: ClientSettings::default(),
        mesh: None,
    }
}

/// The input receiver a scripted session drains when the script sends no
/// input: nothing is ever queued, and nothing is dropped.
fn silent_inputs() -> crossbeam_channel::Receiver<InputEvent> {
    crossbeam_channel::unbounded().1
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

impl DeadlineStream for Duplex {
    /// An in-memory stream never idles: its bytes — or its end — are always
    /// there, so a deadline always finds the stream readable.
    fn wait_readable(&mut self, _timeout: Duration) -> std::io::Result<bool> {
        Ok(true)
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

/// Time Update: the world's age and the time of day, each a big-endian i64.
fn time_update_frame(world_age: i64, time_of_day: i64) -> Vec<u8> {
    let mut time = vec![0x03];
    time.extend_from_slice(&world_age.to_be_bytes());
    time.extend_from_slice(&time_of_day.to_be_bytes());
    time
}

/// The payload one simple ground-up column carries: one stone block at its
/// origin in section 0, dark block light, full sky light, plains biomes.
fn simple_column_payload() -> Vec<u8> {
    let mut column = vec![0u8; 8192];
    column[..2].copy_from_slice(&0x0010u16.to_le_bytes());
    column.extend_from_slice(&[0u8; 2048]); // block light
    column.extend_from_slice(&[0xFFu8; 2048]); // sky light
    column.extend_from_slice(&[1u8; 256]); // biomes
    column
}

/// One Chunk Data frame (0x21) with the fields and payload given.
fn column_frame(cx: i32, cz: i32, ground_up: bool, mask: u16, data: &[u8]) -> Vec<u8> {
    let mut chunk = vec![0x21];
    chunk.extend_from_slice(&cx.to_be_bytes());
    chunk.extend_from_slice(&cz.to_be_bytes());
    chunk.push(ground_up as u8);
    chunk.extend_from_slice(&mask.to_be_bytes());
    push_varint(&mut chunk, data.len() as i32);
    chunk.extend_from_slice(data);
    chunk
}

/// One ground-up Chunk Data column with a single stone block at its origin,
/// shaped as the capture records a column: the block light and sky light of the
/// section the mask selects, then the biome array.
fn chunk_data_frame(cx: i32, cz: i32) -> Vec<u8> {
    column_frame(cx, cz, true, 0x0001, &simple_column_payload())
}

/// One ground-up column payload with a single stone block at the given local
/// position of section 0, dark block light, full sky light and plains biomes.
fn column_payload_at(x: usize, z: usize) -> Vec<u8> {
    let mut column = vec![0u8; 8192];
    let index = block_index(x, 0, z);
    column[index * 2..index * 2 + 2].copy_from_slice(&0x0010u16.to_le_bytes());
    column.extend_from_slice(&[0u8; 2048]); // block light
    column.extend_from_slice(&[0xFFu8; 2048]); // sky light
    column.extend_from_slice(&[1u8; 256]); // biomes
    column
}

/// One ground-up Chunk Data column with a single stone block at the given
/// local position of section 0.
fn chunk_data_frame_at(cx: i32, cz: i32, x: usize, z: usize) -> Vec<u8> {
    column_frame(cx, cz, true, 0x0001, &column_payload_at(x, z))
}

/// The unload shape: a ground-up Chunk Data with an empty mask and no data.
fn chunk_unload_frame(cx: i32, cz: i32) -> Vec<u8> {
    column_frame(cx, cz, true, 0x0000, &[])
}

/// One Map Chunk Bulk frame (0x26) carrying a simple ground-up column at each
/// of `columns`: the metadata block first, then the payloads.
fn bulk_frame_of(columns: &[(i32, i32)]) -> Vec<u8> {
    let payload = simple_column_payload();
    let mut bulk = vec![0x26, 1]; // sky light: the Overworld carries it
    push_varint(&mut bulk, columns.len() as i32);
    for (cx, cz) in columns {
        bulk.extend_from_slice(&cx.to_be_bytes());
        bulk.extend_from_slice(&cz.to_be_bytes());
        bulk.extend_from_slice(&0x0001u16.to_be_bytes());
    }
    for _ in columns {
        bulk.extend_from_slice(&payload);
    }
    bulk
}

/// The mask selecting `sections`.
fn mask_of(sections: &[usize]) -> u16 {
    sections
        .iter()
        .fold(0u16, |mask, &section| mask | (1 << section))
}

/// The payload a light-carrying column sends: one stone block at local
/// (1, 1, 1) in every listed section, and the same uniform light in all four
/// cells around it, so every vertex of the section's mesh reads one pair. A
/// ground-up payload carries the biome array; the section-update shape
/// (`GroundUpContinuous = false`) does not.
fn lit_column_payload(
    sections: &[usize],
    block_light: u8,
    sky_light: u8,
    ground_up: bool,
) -> Vec<u8> {
    let mut data = Vec::new();
    let index = block_index(1, 1, 1);
    for _ in sections {
        let mut blocks = vec![0u8; 8192];
        blocks[index * 2..index * 2 + 2].copy_from_slice(&0x0010u16.to_le_bytes());
        data.extend_from_slice(&blocks);
    }
    for _ in sections {
        // Both nibbles of every byte: one uniform level over the array.
        data.extend_from_slice(&[block_light * 17; 2048]);
    }
    for _ in sections {
        data.extend_from_slice(&[sky_light * 17; 2048]);
    }
    if ground_up {
        data.extend_from_slice(&[1u8; 256]); // plains
    }
    data
}

/// One Chunk Data frame whose listed sections carry one stone block each and
/// uniform light, in the shape `ground_up` names.
fn lit_column_frame(
    cx: i32,
    cz: i32,
    sections: &[usize],
    block_light: u8,
    sky_light: u8,
    ground_up: bool,
) -> Vec<u8> {
    let payload = lit_column_payload(sections, block_light, sky_light, ground_up);
    column_frame(cx, cz, ground_up, mask_of(sections), &payload)
}

/// The committed capture payload for chunk (0, 11): the ground-up column the
/// rig recorded, mask 0x001f, 61,696 bytes of sections, light and biomes.
fn fixture_column() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oxide-proto-v47/tests/fixtures/m1-capture/column-0_11.bin");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// One Map Chunk Bulk frame carrying the fixture payload twice: at chunk
/// (0, 11), where the capture recorded it, and again at (1, 11). The 0x26
/// shape lists every column's metadata first, then the full payloads.
fn bulk_frame() -> Vec<u8> {
    let column = fixture_column();
    let mut bulk = vec![0x26, 1]; // sky light: the Overworld carries it
    push_varint(&mut bulk, 2); // two columns
    for (cx, cz) in [(0i32, 11i32), (1, 11)] {
        bulk.extend_from_slice(&cx.to_be_bytes());
        bulk.extend_from_slice(&cz.to_be_bytes());
        bulk.extend_from_slice(&0x001fu16.to_be_bytes());
    }
    bulk.extend_from_slice(&column);
    bulk.extend_from_slice(&column);
    bulk
}

/// One ground-up Chunk Data frame carrying the committed capture column: a
/// heavy five-section payload, used where a burst has to keep the pool busy.
fn fixture_chunk_frame(cx: i32, cz: i32) -> Vec<u8> {
    column_frame(cx, cz, true, 0x001f, &fixture_column())
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

/// The world-derived values a Sky event carries, in the test's own shape.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SkyReport {
    /// The celestial angle.
    celestial_angle: f32,
    /// The sky's colour.
    colour: [f32; 3],
    /// The sun's brightness.
    sun_brightness: f32,
    /// The stars' brightness.
    star_brightness: f32,
    /// The clouds' tint.
    cloud_colour: [f32; 3],
    /// The moon's phase.
    moon_phase: u8,
    /// The view block's light level, which the fog colour's brightness factor reads.
    light_level: u8,
}

/// The sky values a `Sky` event carries, when it is one.
fn sky_report(event: &ClientEvent) -> Option<SkyReport> {
    match event {
        ClientEvent::Sky {
            celestial_angle,
            colour,
            sun_brightness,
            star_brightness,
            cloud_colour,
            moon_phase,
            light_level,
        } => Some(SkyReport {
            celestial_angle: *celestial_angle,
            colour: *colour,
            sun_brightness: *sun_brightness,
            star_brightness: *star_brightness,
            cloud_colour: *cloud_colour,
            moon_phase: *moon_phase,
            light_level: *light_level,
        }),
        _ => None,
    }
}

/// The skies the frames themselves report: each `Sky` directly preceded by the
/// Time Update or the snapped tick that produced it. A tick that advances a
/// running clock adds skies of its own between the frames — the sun travels at
/// the tick rate — so the frame-driven reports are identified by their marker,
/// not by position.
fn frame_skies(events: &[ClientEvent]) -> Vec<SkyReport> {
    events
        .windows(2)
        .filter(|pair| {
            matches!(
                pair[0],
                ClientEvent::Time { .. } | ClientEvent::PlayerTick { snapped: true, .. }
            )
        })
        .filter_map(|pair| sky_report(&pair[1]))
        .collect()
}

/// The section slots of the first `ChunkUpdated` for a column, panicking when
/// no event reports the column.
fn updated_slots(events: &[ClientEvent], cx: i32, cz: i32) -> &[(usize, Option<ChunkMesh>)] {
    events
        .iter()
        .find_map(|event| match event {
            ClientEvent::ChunkUpdated {
                cx: event_cx,
                cz: event_cz,
                sections,
            } if *event_cx == cx && *event_cz == cz => Some(sections.as_slice()),
            _ => None,
        })
        .expect("the column is reported")
}

/// The section slots of the last `ChunkUpdated` for a column, panicking when
/// no event reports the column.
fn last_updated_slots(events: &[ClientEvent], cx: i32, cz: i32) -> &[(usize, Option<ChunkMesh>)] {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            ClientEvent::ChunkUpdated {
                cx: event_cx,
                cz: event_cz,
                sections,
            } if *event_cx == cx && *event_cz == cz => Some(sections.as_slice()),
            _ => None,
        })
        .expect("the column is reported")
}

/// The light pair every vertex of a single-block section's mesh carries, and a
/// panic when the vertices disagree.
fn uniform_light(mesh: &ChunkMesh) -> [u16; 2] {
    let vertices: Vec<&Vertex> = mesh
        .layers
        .iter()
        .flat_map(|layer| layer.vertices.iter())
        .collect();
    let light = vertices.first().expect("the mesh draws a block").light;
    assert!(
        vertices.iter().all(|vertex| vertex.light == light),
        "every vertex was expected to read the same light"
    );
    light
}

/// The columns some `ChunkUpdated` reports, deduplicated.
fn updated_columns(events: &[ClientEvent]) -> BTreeSet<(i32, i32)> {
    events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::ChunkUpdated { cx, cz, .. } => Some((*cx, *cz)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_session_logs_in_joins_and_answers_every_obligation() {
    let (stream, outgoing) = duplex(scripted_server_stream());
    let (sender, receiver) = crossbeam_channel::unbounded();
    let session = Session::new(Conn::new(stream), config());
    session
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    // The ticks the session's own clock adds are its own business here; the
    // unsnapped ones are dropped so the frame-driven story reads as before.
    let events: Vec<ClientEvent> = receiver
        .try_iter()
        .filter(|event| !matches!(event, ClientEvent::PlayerTick { snapped: false, .. }))
        .collect();
    assert!(matches!(events[0], ClientEvent::LoggedIn { .. }));
    assert!(matches!(
        events[1],
        ClientEvent::Joined {
            entity_id: 20,
            dimension: 0,
            ..
        }
    ));
    assert!(matches!(
        events[2],
        ClientEvent::PlayerTick {
            x,
            snapped: true,
            ..
        } if x == 0.5
    ));
    assert!(matches!(events[3], ClientEvent::KeepAlive { id: 7 }));
    match &events[4] {
        ClientEvent::ChunkUpdated {
            cx: 0,
            cz: 0,
            sections,
        } => {
            assert_eq!(sections.len(), 16, "all sixteen section slots are reported");
            let mesh = sections[0].1.as_ref().expect("section 0 draws");
            assert_eq!(mesh.vertex_count(), 24, "one stone block, six faces");
        }
        other => panic!("expected a chunk update, got {other:?}"),
    }
    assert_eq!(
        events.len(),
        5,
        "the login, the join, the teleport, the echo and one mesh: {events:?}"
    );
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 0)]),
        "the applied column is the only one the change marked: {events:?}"
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
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let corrected = events
        .iter()
        .rev()
        .find(|event| matches!(event, ClientEvent::PlayerTick { snapped: true, .. }))
        .expect("the second teleport is reported");
    match corrected {
        ClientEvent::PlayerTick {
            x,
            y,
            z,
            yaw,
            pitch,
            ..
        } => {
            assert_eq!(
                (*x, *y, *z, *yaw, *pitch),
                (10.5, 1.0, 17.5, 45.0, -10.0),
                "the deltas are applied and the absolutes are taken as they arrived"
            );
        }
        other => panic!("expected the snapped tick, got {other:?}"),
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
fn the_look_input_turns_the_player_and_a_correction_reports_it_on_a_snapped_tick() {
    // The window's mouse deltas arrive on the input channel and are applied where the source
    // applies them — when the mouse is polled, not on the tick — and the correction that
    // follows reports the player immediately, on a snapped tick, carrying the input's
    // rotation plus its own relative deltas. The sign chain is pinned end to end: a rightward
    // delta raises the yaw (yaw 0 faces south and 90 faces west, so a right turn increases
    // it), and a downward delta raises the pitch (positive pitch looks down,
    // `Entity.getVectorForRotation`, `Entity.java:1476-1483`).
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    input_tx
        .send(InputEvent::MouseDelta { dx: 10.0, dy: 5.0 })
        .expect("the input channel is open");
    input_tx
        .send(InputEvent::MouseDelta { dx: -4.0, dy: 0.0 })
        .expect("the input channel is open");
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        // The correction's yaw and pitch are deltas on what the session holds: the wire's
        // relative-flag bits 0x08 and 0x10.
        position_frame(0.5, 65.0, 4.5, 1.0, 0.5, 0x18),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, input_rx)
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let snapped = events
        .iter()
        .find(|event| matches!(event, ClientEvent::PlayerTick { snapped: true, .. }))
        .expect("the correction reports a snapped tick");
    match snapped {
        ClientEvent::PlayerTick { yaw, pitch, .. } => {
            // At the fixed 0.5 sensitivity 10 px right is 1.5°, the leftward 4 px take 0.6°
            // back off, and the correction adds its own 1.0°: 1.9°. The 5 px fall is 0.75°,
            // the correction adds 0.5°: 1.25°.
            assert!(
                (*yaw - 1.9).abs() < 1e-4,
                "the yaw carries the input's turn plus the correction's delta: {yaw}"
            );
            assert!(
                (*pitch - 1.25).abs() < 1e-4,
                "the pitch carries the input's fall plus the correction's delta: {pitch}"
            );
        }
        other => panic!("expected the snapped tick, got {other:?}"),
    }
}

#[test]
fn an_unload_packet_removes_the_column_and_remeshes_its_neighbours() {
    // The unloaded column and its four neighbours are loaded first; then the
    // server unloads the column in the middle. The unload is reported as its
    // packet arrives, and the four loaded neighbours are re-meshed after it:
    // a column that leaves the store changes the collar their meshes read.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        chunk_data_frame(3, 4),
        chunk_data_frame(4, 4),
        chunk_data_frame(2, 4),
        chunk_data_frame(3, 5),
        chunk_data_frame(3, 3),
        chunk_unload_frame(3, 4),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let unloaded = events
        .iter()
        .position(|event| matches!(event, ClientEvent::ChunkUnloaded { cx: 3, cz: 4 }))
        .unwrap_or_else(|| panic!("the unload is reported: {events:?}"));
    let neighbours: BTreeSet<(i32, i32)> = BTreeSet::from([(4, 4), (2, 4), (3, 5), (3, 3)]);
    let reported = updated_columns(&events);
    assert!(
        neighbours.is_subset(&reported),
        "every loaded neighbour is re-meshed: {events:?}"
    );
    assert!(
        reported.is_subset(&neighbours.iter().copied().chain([(3, 4)]).collect()),
        "no other column is reported: {events:?}"
    );
    for (cx, cz) in neighbours {
        let last = events
            .iter()
            .rposition(|event| {
                matches!(event, ClientEvent::ChunkUpdated { cx: event_cx, cz: event_cz, .. }
                    if *event_cx == cx && *event_cz == cz)
            })
            .unwrap_or_else(|| panic!("({cx}, {cz}) is re-meshed"));
        assert!(
            last > unloaded,
            "({cx}, {cz})'s last report follows the unload: {events:?}"
        );
        let slots = last_updated_slots(&events, cx, cz);
        assert_eq!(slots.len(), 16, "({cx}, {cz}) reports all sixteen sections");
        let mesh = slots[0].1.as_ref().expect("({cx}, {cz}) draws its block");
        assert_eq!(
            mesh.vertex_count(),
            24,
            "({cx}, {cz}) draws its stone block"
        );
    }
    // A stale build of the unloaded column is discarded and rebuilt against
    // the world as it stands: the report, when the race leaves one, draws
    // nothing. The window drops the column's meshes from the unload report
    // itself.
    if let Some(sections) = events
        .iter()
        .skip(unloaded)
        .rev()
        .find_map(|event| match event {
            ClientEvent::ChunkUpdated {
                cx: 3,
                cz: 4,
                sections,
            } => Some(sections.as_slice()),
            _ => None,
        })
    {
        assert!(
            sections.iter().all(|(_, mesh)| mesh.is_none()),
            "the removed column draws nothing"
        );
    }
}

#[test]
fn a_section_update_carries_its_light_and_keeps_unlisted_sections() {
    // A ground-up column with sections 1 and 2, light 3/7, then the 0x21
    // section-update shape (GroundUpContinuous = false) replacing section 1
    // alone with light 4/9. The listed section draws with the payload's light;
    // section 2, outside the mask, keeps the light the store held; and no
    // other column is touched. The exact values pin the rule the spec's §9 and
    // the protocol reference's §3.3/§4.2 state: a section update replaces the
    // listed sections' blocks *and* their light, and no local relight runs on
    // this path.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        // The store's initial column: sections 1 and 2, block light 3, sky 7.
        lit_column_frame(0, 0, &[1, 2], 3, 7, true),
        // The section update: section 1 alone, block light 4, sky 9.
        lit_column_frame(0, 0, &[1], 4, 9, false),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 0)]),
        "no other column is touched: {events:?}"
    );
    let slots = last_updated_slots(&events, 0, 0);
    assert_eq!(slots.len(), 16, "all sixteen section slots are reported");
    let listed = slots[1].1.as_ref().expect("section 1 draws its block");
    assert_eq!(
        uniform_light(listed),
        [4 * 16 + 8, 9 * 16 + 8],
        "section 1 draws with the light the payload carried"
    );
    let unlisted = slots[2].1.as_ref().expect("section 2 draws its block");
    assert_eq!(
        uniform_light(unlisted),
        [3 * 16 + 8, 7 * 16 + 8],
        "section 2 outside the mask keeps its stored light"
    );
}

#[test]
fn a_malformed_packet_is_an_error_not_a_panic() {
    // A Join Game frame cut short: the entity id, then nothing.
    let mut join = vec![0x01];
    join.extend_from_slice(&20i32.to_be_bytes());
    let (stream, _outgoing) = duplex(stream_with(&[join]));
    let (sender, _receiver) = crossbeam_channel::unbounded();
    let error = Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
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
        .run_over(&sender, silent_inputs())
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
        .run_over(&sender, silent_inputs())
        .expect("packets M1 has no use for do not end the session");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let keepalive = events
        .iter()
        .rev()
        .find(|event| matches!(event, ClientEvent::KeepAlive { .. }))
        .expect("the keepalive is answered");
    assert!(
        matches!(keepalive, ClientEvent::KeepAlive { id: 11 }),
        "expected keepalive 11, got {keepalive:?}"
    );
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
        .run_over(&sender, silent_inputs())
        .expect("player list traffic does not end the session");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let keepalive = events
        .iter()
        .rev()
        .find(|event| matches!(event, ClientEvent::KeepAlive { .. }))
        .expect("the keepalive is answered");
    assert!(
        matches!(keepalive, ClientEvent::KeepAlive { id: 12 }),
        "expected keepalive 12, got {keepalive:?}"
    );
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

#[test]
fn a_keepalive_behind_a_column_burst_is_answered() {
    // A live server sends its initial columns as a burst with the keepalives
    // behind it on the wire. The echo is a connection obligation, and the
    // server closes a session that leaves it unanswered for about thirty
    // seconds, so the burst must not hold up the read loop: the loop hands
    // jobs to the pool and drains finished results without ever blocking.
    // Six heavy fixture columns make the builds outlive the read, so the
    // echo must be reported while at least one build is still outstanding —
    // a session that answered the keepalive only after the burst's meshes
    // would report every mesh first. The pool's results arrive in no fixed
    // order, so the meshes are compared as a set.
    let (stream, outgoing) = duplex(stream_with(&[
        join_game_frame(),
        fixture_chunk_frame(0, 11),
        fixture_chunk_frame(1, 11),
        fixture_chunk_frame(2, 11),
        fixture_chunk_frame(3, 11),
        fixture_chunk_frame(4, 11),
        fixture_chunk_frame(5, 11),
        keep_alive_frame(21),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let answered = events
        .iter()
        .position(|event| matches!(event, ClientEvent::KeepAlive { id: 21 }))
        .expect("the keepalive is answered");
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 11), (1, 11), (2, 11), (3, 11), (4, 11), (5, 11)]),
        "every burst column is meshed: {events:?}"
    );
    let last_mesh = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::ChunkUpdated { .. }))
        .expect("the burst is meshed");
    assert!(
        answered < last_mesh,
        "the echo was answered before the burst's last meshes, not after them: {events:?}"
    );
    // The echo is written: it is the only packet the burst drew, right after
    // the settings and the brand.
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    assert_eq!(
        client_frame(&mut cursor, SERVER_FRAMING),
        [0x00, 0x15],
        "the keepalive is the only packet the burst drew"
    );
    assert!(cursor.is_empty(), "no further packets were sent");
}

#[test]
fn a_keepalive_behind_a_large_burst_is_answered_before_the_burst_is_meshed() {
    // The burst a live server sends on join is hundreds of column frames, and
    // the first keepalive sits behind all of them on the wire. The server
    // closes a session whose echo goes unanswered for about thirty seconds, so
    // the loop must read the burst at frame-parse speed: the frames are read
    // one after another and the meshes are handed to the pool only in the idle
    // wait, so a stream that never idles is read without a single snapshot
    // copy on the way. Every frame here carries work for the mesh queue — the
    // frames re-send the few columns a busy server keeps updating, as its
    // blocks change — so every frame finds the queue dirty, and each applied
    // column's snapshot copy costs tens of milliseconds in a debug build: a
    // loop that pumped between packets would need far longer than the deadline
    // below for these 240 frames, and the diagnostic names how much of the
    // burst was meshed while the echo went unanswered. The harness's stream has
    // no idle wait, so the burst's meshes can only be reported after the
    // stream ends: an echo answered here was answered while the burst was
    // still unmeshed.
    const BURST: i32 = 240;
    // The columns the burst re-sends. Few enough that the meshes left for the
    // end-of-session drain stay few — the drain pays their snapshot copies on
    // this thread, so a large leftover set would slow the test down without
    // pinning anything — and any path that pumps per frame hands several of
    // them to the pool on every one of the 240 frames.
    const COLUMNS: i32 = 8;
    // Far above what reading 240 frames costs a loop that only parses them,
    // and far below what meshing them between packets would cost.
    const ECHO_DEADLINE: Duration = Duration::from_secs(4);

    let mut script = Vec::new();
    login_sequence(&mut script);
    frame(&mut script, &join_game_frame(), SERVER_FRAMING);
    for frame_index in 0..BURST {
        frame(
            &mut script,
            &chunk_data_frame(frame_index % COLUMNS, 0),
            SERVER_FRAMING,
        );
    }
    frame(&mut script, &keep_alive_frame(97), SERVER_FRAMING);

    let (stream, outgoing) = duplex(script);
    let (sender, receiver) = crossbeam_channel::unbounded();
    let started = Instant::now();
    let session = std::thread::spawn(move || {
        Session::new(Conn::new(stream), config()).run_over(&sender, silent_inputs())
    });

    let deadline = started + ECHO_DEADLINE;
    let mut meshed = 0usize;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(ClientEvent::KeepAlive { id: 97 }) => break,
            Ok(ClientEvent::ChunkUpdated { .. }) => meshed += 1,
            Ok(_) => {}
            Err(error) => panic!(
                "the echo went unanswered for {ECHO_DEADLINE:?} with {meshed} burst meshes \
                 already reported: {error}"
            ),
        }
    }
    assert_eq!(
        meshed, 0,
        "the echo must be answered before any of the burst is meshed"
    );
    session
        .join()
        .expect("the session thread ends")
        .expect("the session runs to the end of the stream");

    // The echo is on the wire: it is the only packet the burst drew, right
    // after the settings and the brand.
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    assert_eq!(
        client_frame(&mut cursor, SERVER_FRAMING),
        [0x00, 0x61],
        "the keepalive is the only packet the burst drew"
    );
    assert!(cursor.is_empty(), "no further packets were sent");
}

/// A duplex with a quiet stretch: after `head` is consumed, the stream reports
/// nothing readable for `stalls` waits, then serves `tail`; once `tail` is
/// consumed too, a second stretch of `tail_stalls` waits precedes the end.
struct GappedDuplex {
    head: std::io::Cursor<Vec<u8>>,
    tail: std::io::Cursor<Vec<u8>>,
    stalls: usize,
    tail_stalls: usize,
    outgoing: Arc<Mutex<Vec<u8>>>,
}

impl Read for GappedDuplex {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.head.position() < self.head.get_ref().len() as u64 {
            self.head.read(out)
        } else {
            self.tail.read(out)
        }
    }
}

impl Write for GappedDuplex {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.outgoing.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl DeadlineStream for GappedDuplex {
    fn wait_readable(&mut self, timeout: Duration) -> std::io::Result<bool> {
        if self.head.position() < self.head.get_ref().len() as u64 {
            Ok(true)
        } else if self.stalls > 0 {
            self.stalls -= 1;
            // A real deadline blocks for up to the timeout before it reports
            // nothing; the fake waits the same span, so the pool gets the
            // window the tick gives it.
            std::thread::sleep(timeout);
            Ok(false)
        } else if self.tail.position() < self.tail.get_ref().len() as u64 {
            Ok(true)
        } else if self.tail_stalls > 0 {
            self.tail_stalls -= 1;
            std::thread::sleep(timeout);
            Ok(false)
        } else {
            Ok(true)
        }
    }
}

#[test]
fn a_quiet_stretch_is_used_to_rebuild_the_pending_meshes() {
    // The burst has arrived and a keepalive has been answered; the connection
    // then goes quiet before the next keepalive. The queue's meshes are
    // handed to the pool and drained during that quiet stretch — not deferred
    // to the end of the stream — and the next keepalive is still read after
    // them.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &chunk_data_frame(0, 0), SERVER_FRAMING);
    frame(&mut head, &keep_alive_frame(31), SERVER_FRAMING);
    let mut tail = Vec::new();
    frame(&mut tail, &keep_alive_frame(32), SERVER_FRAMING);

    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(tail),
        // The queue's mesh is handed over in the first of these quiet
        // windows — the loop reads frames first, so the copy and the spawn
        // happen here, not between the head's frames — and the rest are the
        // wall clock the pool's build needs (cold code, a loaded machine)
        // before the tail arrives: twenty deadlines is a few hundred
        // milliseconds of quiet.
        stalls: 20,
        tail_stalls: 0,
        outgoing: Arc::clone(&outgoing),
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let last_mesh = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::ChunkUpdated { .. }))
        .expect("the column is meshed");
    let second = events
        .iter()
        .position(|event| matches!(event, ClientEvent::KeepAlive { id: 32 }))
        .expect("the second keepalive is answered");
    assert!(
        last_mesh < second,
        "the meshes are built in the quiet stretch, before the next keepalive: {events:?}"
    );
}

/// A script holding one keepalive.
fn keepalive_script(id: i32) -> Vec<u8> {
    let mut out = Vec::new();
    frame(&mut out, &keep_alive_frame(id), SERVER_FRAMING);
    out
}

/// Runs a quiet session — a join, one running Time Update, then `stalls` idle waits before
/// `tail` is served — with `inputs` queued, and returns every event it reported with the
/// wall-clock span the run took.
fn run_quiet_session(
    stalls: usize,
    tail: Vec<u8>,
    inputs: Vec<InputEvent>,
) -> (Vec<ClientEvent>, Duration) {
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    // A positive wire time: the day-night cycle runs, so every tick advances the clock.
    frame(&mut head, &time_update_frame(48_000, 6000), SERVER_FRAMING);

    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(tail),
        stalls,
        tail_stalls: 0,
        outgoing,
    };
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    for input in inputs {
        input_tx.send(input).expect("the input channel is open");
    }
    let (sender, receiver) = crossbeam_channel::unbounded();
    let started = Instant::now();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, input_rx)
        .expect("the session runs to the end of the stream");
    let elapsed = started.elapsed();
    (receiver.try_iter().collect(), elapsed)
}

#[test]
fn the_tick_advances_through_a_quiet_stretch_and_every_clock_tick_reports_the_sky() {
    // The loop's idle waits are where the ticks run: the connection goes quiet after the
    // join and one Time Update, and the session's own clock keeps the player stepping with
    // no frame in sight — the source's arrangement, where the game tick is driven by the
    // frame clock but needs no frame per tick. A held key reaches the tick's report, every
    // tick that advances the running clock reports the sky it moved, and the keepalive
    // behind the quiet stretch is read after those tick batches, not before them.
    let (events, elapsed) = run_quiet_session(
        16,
        keepalive_script(71),
        vec![InputEvent::Key {
            key: Key::ShiftLeft,
            pressed: true,
        }],
    );

    let ticks: Vec<(u64, bool)> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick { tick, sneaking, .. } => Some((*tick, *sneaking)),
            _ => None,
        })
        .collect();
    assert!(
        ticks.len() >= 3,
        "the quiet stretch owes whole steps: {ticks:?}"
    );
    assert_eq!(
        ticks.first().map(|(tick, _)| *tick),
        Some(1),
        "the first tick is tick 1: the player starts at zero"
    );
    for pair in ticks.windows(2) {
        assert_eq!(
            pair[1].0,
            pair[0].0 + 1,
            "the tick count is gap-free: {ticks:?}"
        );
    }
    // A tick batch is capped and no debt is carried, so the whole run cannot report more
    // steps than the elapsed time owed.
    let budget = (elapsed.as_millis() / 50) as usize + TICK_CATCHUP_CAP as usize;
    assert!(
        ticks.len() <= budget,
        "{} ticks in {elapsed:?} exceed the elapsed steps plus one capped batch",
        ticks.len()
    );
    assert!(
        ticks.iter().all(|(_, sneaking)| *sneaking),
        "the held key is on every tick's report: {ticks:?}"
    );
    // The Time Update's own sky, plus exactly one for every tick that advanced the clock.
    let skies = events
        .iter()
        .filter(|event| matches!(event, ClientEvent::Sky { .. }))
        .count();
    assert_eq!(
        skies,
        ticks.len() + 1,
        "one sky per clock-advancing tick plus the Time Update's: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::KeepAlive { id: 71 })),
        "the quiet stretch does not starve the read loop: {events:?}"
    );
}

#[test]
fn a_focus_loss_releases_the_held_keys_before_the_next_tick() {
    // The same quiet session with the sneak key held reports it on every tick; the same
    // script with a FocusLost behind the key — and a mouse button edge riding along, which
    // must not disturb the intent — reports the neutral intent instead, because a window
    // that stopped receiving holds nothing. The two runs differ by those events alone.
    let (held, _) = run_quiet_session(
        12,
        Vec::new(),
        vec![InputEvent::Key {
            key: Key::ShiftLeft,
            pressed: true,
        }],
    );
    let (released, _) = run_quiet_session(
        12,
        Vec::new(),
        vec![
            InputEvent::Key {
                key: Key::ShiftLeft,
                pressed: true,
            },
            InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: true,
            },
            InputEvent::FocusLost,
        ],
    );

    let sneaking = |events: &[ClientEvent]| -> Vec<bool> {
        events
            .iter()
            .filter_map(|event| match event {
                ClientEvent::PlayerTick { sneaking, .. } => Some(*sneaking),
                _ => None,
            })
            .collect()
    };
    let held_ticks = sneaking(&held);
    let released_ticks = sneaking(&released);
    assert!(!held_ticks.is_empty(), "the quiet stretch ticks: {held:?}");
    assert!(
        held_ticks.iter().all(|sneaking| *sneaking),
        "the held key is on every tick: {held:?}"
    );
    assert!(
        !released_ticks.is_empty(),
        "the quiet stretch ticks: {released:?}"
    );
    assert!(
        released_ticks.iter().all(|sneaking| !*sneaking),
        "the focus loss released the held key: {released:?}"
    );
}

#[test]
fn a_correction_between_two_ticks_moves_the_player_and_reports_one_snapped_tick() {
    // A join and a first teleport settle the player; the connection then goes quiet long
    // enough for whole ticks to run, a second correction arrives, and the connection goes
    // quiet again. The correction is reported at once — the window must not interpolate
    // across it — and its snapped tick carries the tick count it landed on: the same count
    // the last regular tick reported, and one before the next.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &position_frame(100.0, 70.0, -50.0, 30.0, 10.0, 0),
        SERVER_FRAMING,
    );

    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(tail),
        // Ten idle waits on each side: several whole steps fall inside each stretch.
        stalls: 10,
        tail_stalls: 10,
        outgoing,
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let snapped = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::PlayerTick { snapped: true, .. }))
        .expect("the correction is reported");
    let (x, y, z, tick) = match &events[snapped] {
        ClientEvent::PlayerTick { x, y, z, tick, .. } => (*x, *y, *z, *tick),
        other => panic!("expected the snapped tick, got {other:?}"),
    };
    assert_eq!(
        (x, y, z),
        (100.0, 70.0, -50.0),
        "the correction moved the player to the values it carried"
    );
    let before = events[..snapped]
        .iter()
        .rev()
        .find_map(|event| match event {
            ClientEvent::PlayerTick {
                tick,
                snapped: false,
                ..
            } => Some(*tick),
            _ => None,
        })
        .expect("a regular tick ran before the correction");
    let after = events[snapped + 1..]
        .iter()
        .find_map(|event| match event {
            ClientEvent::PlayerTick {
                tick,
                snapped: false,
                ..
            } => Some(*tick),
            _ => None,
        })
        .expect("a regular tick ran after the correction");
    assert_eq!(
        before, tick,
        "the snapped tick carries the count of the tick it landed on"
    );
    assert_eq!(after, tick + 1, "the next regular tick is the one after it");
}

#[test]
fn a_frame_flood_does_not_starve_the_ticks_and_a_quiet_tail_owes_no_debt() {
    // The burst a live server sends on join arrives back to back with a keepalive behind it,
    // and then the stream goes quiet. Each pass reads one frame and then runs the ticks that
    // have come due, so the flood does not starve them, and the quiet stretch cannot pay off
    // debt the ticker never accumulated: the total stays within the elapsed time's steps
    // plus one capped batch. The burst's columns are still meshed and both keepalives are
    // answered.
    const BURST: i32 = 96;
    let mut script = Vec::new();
    login_sequence(&mut script);
    frame(&mut script, &join_game_frame(), SERVER_FRAMING);
    for index in 0..BURST {
        frame(&mut script, &chunk_data_frame(index % 4, 0), SERVER_FRAMING);
    }
    frame(&mut script, &keep_alive_frame(81), SERVER_FRAMING);

    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(script),
        tail: std::io::Cursor::new(keepalive_script(82)),
        stalls: 10,
        tail_stalls: 0,
        outgoing,
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    let started = Instant::now();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");
    let elapsed = started.elapsed();

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    for id in [81, 82] {
        assert!(
            events.iter().any(
                |event| matches!(event, ClientEvent::KeepAlive { id: answered } if *answered == id)
            ),
            "keepalive {id} is answered: {events:?}"
        );
    }
    let ticks: Vec<u64> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick {
                tick,
                snapped: false,
                ..
            } => Some(*tick),
            _ => None,
        })
        .collect();
    assert!(
        ticks.len() >= 2,
        "the flood and the quiet stretch owe whole steps: {ticks:?}"
    );
    assert_eq!(ticks.first().copied(), Some(1), "the first tick is tick 1");
    for pair in ticks.windows(2) {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "the tick count is gap-free: {ticks:?}"
        );
    }
    let budget = (elapsed.as_millis() / 50) as usize + TICK_CATCHUP_CAP as usize;
    assert!(
        ticks.len() <= budget,
        "{} ticks in {elapsed:?} exceed the elapsed steps plus one capped batch",
        ticks.len()
    );
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 0), (1, 0), (2, 0), (3, 0)]),
        "the flood's columns are still meshed: {events:?}"
    );
}

#[test]
fn a_bulk_frame_serves_two_columns_and_the_session_stays_live() {
    // The shape a live 1.8.9 server sends on join: one 0x26 frame carrying the
    // metadata for every column, then their full payloads. The second column
    // repeats the fixture one chunk east; the two payloads are byte-identical,
    // so a cross-copy mix-up is not detectable by these assertions — a decoder
    // that filled the second column from the first copy would pass every probe.
    let bulk = bulk_frame();
    let (stream, outgoing) = duplex(stream_with(&[
        join_game_frame(),
        bulk.clone(),
        keep_alive_frame(41),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver
        .try_iter()
        .filter(|event| !matches!(event, ClientEvent::PlayerTick { snapped: false, .. }))
        .collect();
    assert!(matches!(events[0], ClientEvent::LoggedIn { .. }));
    assert!(matches!(
        events[1],
        ClientEvent::Joined { dimension: 0, .. }
    ));
    // The bulk frame draws no reply of its own, and the keepalive behind it is
    // still answered.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::KeepAlive { id: 41 })),
        "the keepalive is answered: {events:?}"
    );
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    assert_eq!(
        client_frame(&mut cursor, SERVER_FRAMING),
        [0x00, 0x29],
        "the keepalive echo is the only packet the bulk frame drew"
    );
    assert!(cursor.is_empty(), "no further packets were sent");

    // Both columns are handed to the pool and each is reported once, with all
    // sixteen section slots; the results arrive in no fixed order, so the
    // comparison is a set.
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 11), (1, 11)]),
        "both bulk columns are meshed, not just the first: {events:?}"
    );
    let sixteen: Vec<usize> = (0..16).collect();
    for (cx, cz) in [(0, 11), (1, 11)] {
        let slots = updated_slots(&events, cx, cz);
        assert_eq!(slots.len(), 16, "({cx}, {cz}) reports all sixteen sections");
        let indices: Vec<usize> = slots.iter().map(|(index, _)| *index).collect();
        assert_eq!(indices, sixteen, "({cx}, {cz}) reports them in order");
        // The sixteen slots are shape, not content: they come back even for a
        // world that held no blocks. The fixture's bottom section is bedrock,
        // so the applied column must have drawn it.
        let mesh = slots[0]
            .1
            .as_ref()
            .expect("({cx}, {cz}) draws its bottom section");
        assert!(
            !mesh.is_empty(),
            "({cx}, {cz}) draws its bottom section: a world that held no blocks could not"
        );
    }

    // The values the fixture carries, read through `World::block` on a world
    // built from the same bytes by the same decode-and-apply path the session
    // runs — the session owns its world privately, so the test rebuilds it
    // rather than reaching inside. The fixture's first block is bedrock, the
    // block the manifest counts 785 of, and its local (14, 62, 5) is grass,
    // from the manifest's 235.
    let packet = MapChunkBulk::decode(&bulk[1..]).expect("the scripted frame decodes");
    let mut world = World::new(true);
    assert_eq!(world.apply_bulk(&packet), 2, "both columns apply");
    let first = [world.block(0, 0, 176), world.block(14, 62, 181)];
    let second = [world.block(16, 0, 176), world.block(30, 62, 181)];
    assert_eq!(
        first,
        [0x0070, 0x0020],
        "bedrock at the column's bottom, grass at local (14, 62, 5)"
    );
    assert_eq!(
        second, first,
        "the column at (1, 11) is a faithful copy of the column at (0, 11)"
    );
}

#[test]
fn a_burst_of_six_columns_is_meshed_exactly_once_through_the_pool() {
    // Three plain 0x21 columns and one 0x26 bulk covering three more, applied
    // back to back with a keepalive between. The columns abut, so a session
    // whose boundary refresh marked anything beyond the burst would report a
    // column outside the six; the dirty set records each applied column and
    // its loaded neighbours, all of them inside the burst. The pool's results
    // arrive in no fixed order and a refreshed column may be reported more
    // than once, so the reported set is compared as a map.
    let bulk = bulk_frame_of(&[(3, 0), (4, 0), (5, 0)]);
    let (stream, outgoing) = duplex(stream_with(&[
        join_game_frame(),
        chunk_data_frame(0, 0),
        chunk_data_frame(1, 0),
        keep_alive_frame(61),
        chunk_data_frame(2, 0),
        bulk,
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::KeepAlive { id: 61 })),
        "the keepalive between the bursts is answered: {events:?}"
    );
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)]),
        "exactly the six columns: {events:?}"
    );
    for (cx, cz) in [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)] {
        let slots = updated_slots(&events, cx, cz);
        assert_eq!(slots.len(), 16, "({cx}, {cz}) reports all sixteen sections");
        let indices: Vec<usize> = slots.iter().map(|(index, _)| *index).collect();
        assert_eq!(
            indices,
            (0..16).collect::<Vec<_>>(),
            "({cx}, {cz}) reports them in order"
        );
        let mesh = slots[0].1.as_ref().expect("({cx}, {cz}) draws its block");
        assert_eq!(
            mesh.vertex_count(),
            24,
            "({cx}, {cz}) draws its stone block"
        );
    }
    // The keepalive echo is the only packet the burst drew, after the settings
    // and the brand.
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    assert_eq!(
        client_frame(&mut cursor, SERVER_FRAMING),
        [0x00, 0x3d],
        "the keepalive echo is the only packet the six columns drew"
    );
    assert!(cursor.is_empty(), "no further packets were sent");
}

#[test]
fn an_applied_column_refreshes_its_loaded_neighbours() {
    // Column (0, 0) holds a block on the face it shares with (1, 0), and
    // (1, 0) arrives after it. The neighbour's block lands in the first
    // column's collar and culls the shared face, so the first column must be
    // rebuilt after the neighbour applies. Without the boundary refresh it
    // keeps the six faces it drew while the collar was air.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        chunk_data_frame_at(0, 0, 15, 0),
        chunk_data_frame_at(1, 0, 0, 0),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let slots = last_updated_slots(&events, 0, 0);
    let mesh = slots[0].1.as_ref().expect("(0, 0) draws its block");
    assert_eq!(
        mesh.vertex_count(),
        20,
        "the shared face is culled once the neighbour applied: {events:?}"
    );
    // The neighbour reads the first column's block through its own collar, so
    // its shared face is culled from its first build.
    let slots = last_updated_slots(&events, 1, 0);
    let mesh = slots[0].1.as_ref().expect("(1, 0) draws its block");
    assert_eq!(
        mesh.vertex_count(),
        20,
        "the neighbour culls its shared face too"
    );
}

#[test]
fn the_session_reports_the_clock_and_the_sky_it_moves() {
    // A join, a teleport into the column the script loads, the column whose biome array is
    // plains, `/time set 6000`'s own frame, the same frame with the time negated — what a
    // stopped day-night cycle puts on the wire — a teleport inside that column, and a third
    // day's frame. The clock comes back per update; the receive rule negates the frozen
    // frame's negative time back (`WorldClient.java:468-483`), so its clock and sky are the
    // noon values. The sky follows the clock and the view block, because the client cannot
    // sample the world itself.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0),
        chunk_data_frame(0, 0),
        time_update_frame(48_000, 6000),
        time_update_frame(48_000, -6000),
        position_frame(12.5, 65.0, 4.5, 0.0, 0.0, 0),
        time_update_frame(48_000, 72_000),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    let session = Session::new(Conn::new(stream), config());
    session
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let clocks: Vec<(i64, i64)> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Time {
                world_age,
                time_of_day,
            } => Some((*world_age, *time_of_day)),
            _ => None,
        })
        .collect();
    assert_eq!(
        clocks,
        vec![(48_000, 6000), (48_000, 6000), (48_000, 72_000)],
        "every Time Update is reported, the frozen frame's time negated by the receive rule"
    );

    // The sky's shape carries the view-block colour and the other world-derived values. The
    // frames' own reports are identified by their markers — a running clock's ticks add their
    // own between them, because the sun travels at the tick rate.
    let skies = frame_skies(&events);
    // The plains noon the loaded column's biome gives, reported by the plain frame and — the
    // frozen frame's negative wire time negated back to `+6000` by the receive rule — by the
    // frozen frame too, for the teleport that followed it as well.
    let noon = SkyReport {
        celestial_angle: 0.0,
        colour: [120.0 / 255.0, 167.0 / 255.0, 1.0],
        sun_brightness: 1.0,
        star_brightness: 0.0,
        cloud_colour: [1.0, 1.0, 1.0],
        moon_phase: 0,
        // The script's column carries full sky light at the view block, the value the fog's
        // brightness factor reads as one.
        light_level: 15,
    };
    assert_eq!(
        skies,
        vec![
            noon,
            noon,
            // The frozen frame's sky, reported again for the moved view block.
            noon,
            // The third day's sunrise angle (`angle t=0` is the same day fraction), whose
            // `worldTime / 24000 % 8` is phase 3.
            SkyReport {
                celestial_angle: 0.8535534,
                colour: [120.0 / 255.0, 167.0 / 255.0, 1.0],
                sun_brightness: 1.0,
                star_brightness: 0.0,
                cloud_colour: [1.0, 1.0, 1.0],
                moon_phase: 3,
                light_level: 15,
            },
        ],
        "the sky colour follows the clock and the biome under the player"
    );
    let last_position = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::PlayerTick { snapped: true, .. }))
        .expect("the corrections are reported");
    let last_sky = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Sky { .. }))
        .expect("the sky is reported");
    assert!(
        last_position < last_sky,
        "a moved view block reports the sky again: {events:?}"
    );
}

/// The frozen-sun convention end to end: a server with the day-night cycle stopped negates
/// the time it sends (`S03PacketTimeUpdate.java:17-31`), and the client negates a negative
/// time back before it becomes the world clock (`WorldClient.setWorldTime`,
/// `WorldClient.java:468-483`). Each frozen frame's reported clock and sky must equal the
/// plain positive frame's — the noon the acceptance's `/time set 6000` means — not the night
/// the raw negative value would answer.
#[test]
fn a_frozen_time_update_reports_the_negated_clock_and_sky() {
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0),
        chunk_data_frame(0, 0),
        time_update_frame(48_000, -6000),
        time_update_frame(48_000, 6000),
        time_update_frame(48_000, -6001),
        time_update_frame(48_000, 6001),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    let session = Session::new(Conn::new(stream), config());
    session
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let clocks: Vec<(i64, i64)> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Time {
                world_age,
                time_of_day,
            } => Some((*world_age, *time_of_day)),
            _ => None,
        })
        .collect();
    assert_eq!(
        clocks,
        vec![
            (48_000, 6000),
            (48_000, 6000),
            (48_000, 6001),
            (48_000, 6001),
        ],
        "each frozen frame reports the clock its negation produces, not its wire sign"
    );

    let skies = frame_skies(&events);
    assert_eq!(skies.len(), 4, "one sky per clock update: {events:?}");
    assert_eq!(
        skies[0].celestial_angle, 0.0,
        "the frozen noon: `-6000` and `+6000` are the same half day"
    );
    assert_eq!(
        skies[0], skies[1],
        "the frozen -6000 frame's clock and sky are the plain 6000 frame's"
    );
    assert_eq!(
        skies[2], skies[3],
        "the same for the frozen -6001 frame against the plain 6001 frame"
    );
}

#[test]
fn the_frozen_clock_holds_the_time_of_day_while_the_ticks_advance() {
    // A stopped day-night cycle puts a negative time on the wire
    // (`S03PacketTimeUpdate.java:17-31`); the receive rule reads its sign into
    // the clock's frozen flag (`WorldClient.setWorldTime`, `WorldClient.java:468-483`)
    // and the tick's advance of the time of day is gated on it (`WorldClient.tick`,
    // `:71-74`). The connection then goes quiet for whole ticks: the player steps and the
    // world age moves with every tick, the time of day holds at the frozen value, and no
    // tick reports a sky — a step reports a sky exactly when it moved the time of day.
    // The quiet stretch's keepalive is still answered.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(&mut head, &chunk_data_frame(0, 0), SERVER_FRAMING);
    // `-6000` on the wire: the cycle is stopped and the hour is noon.
    frame(&mut head, &time_update_frame(48_000, -6000), SERVER_FRAMING);

    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(keepalive_script(91)),
        // Sixteen idle waits: a few hundred milliseconds of quiet, whole steps of it.
        stalls: 16,
        tail_stalls: 0,
        outgoing,
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    let ticks: Vec<u64> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick {
                tick,
                snapped: false,
                ..
            } => Some(*tick),
            _ => None,
        })
        .collect();
    assert!(
        ticks.len() >= 3,
        "the quiet stretch owes whole steps even with the cycle stopped: {ticks:?}"
    );
    assert_eq!(ticks.first().copied(), Some(1), "the first tick is tick 1");
    for pair in ticks.windows(2) {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "the tick count is gap-free: {ticks:?}"
        );
    }
    // One clock update — the frozen frame's, its wire sign negated — and no other.
    let clocks: Vec<(i64, i64)> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Time {
                world_age,
                time_of_day,
            } => Some((*world_age, *time_of_day)),
            _ => None,
        })
        .collect();
    assert_eq!(
        clocks,
        vec![(48_000, 6000)],
        "the frozen frame is the only clock: {events:?}"
    );
    // No tick advanced the time of day, so no tick reports a sky: the only sky is the
    // frozen frame's own, and it is the noon the negated time renders.
    let skies: Vec<SkyReport> = events.iter().filter_map(sky_report).collect();
    assert_eq!(
        skies.len(),
        1,
        "a tick reports a sky only when it moved the time of day: {events:?}"
    );
    assert_eq!(
        skies[0].celestial_angle, 0.0,
        "the frozen noon: `-6000` is 6000"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::KeepAlive { id: 91 })),
        "the quiet stretch does not starve the read loop: {events:?}"
    );
}

#[test]
fn the_pitch_look_clamps_at_ninety_degrees_each_way() {
    // `Entity.setAngles` clamps the pitch to ±90 (`Entity.java:395`). The look applies its
    // delta where the source applies it — when the mouse is polled, not on the tick — so a
    // delta far past the limit leaves the pitch at the limit exactly, in either direction.
    // The clamped value rides the snapped tick the correction reports: the pitch arrives
    // with the relative flag and a zero delta, so the report adds nothing of its own.
    let pitch_after_delta = |dy: f64| -> f32 {
        let (input_tx, input_rx) = crossbeam_channel::unbounded();
        input_tx
            .send(InputEvent::MouseDelta { dx: 0.0, dy })
            .expect("the input channel is open");
        let (stream, _outgoing) = duplex(stream_with(&[
            join_game_frame(),
            position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0x10),
        ]));
        let (sender, receiver) = crossbeam_channel::unbounded();
        Session::new(Conn::new(stream), config())
            .run_over(&sender, input_rx)
            .expect("the session runs to the end of the stream");
        let events: Vec<ClientEvent> = receiver.try_iter().collect();
        events
            .iter()
            .find_map(|event| match event {
                ClientEvent::PlayerTick {
                    pitch,
                    snapped: true,
                    ..
                } => Some(*pitch),
                _ => None,
            })
            .expect("the correction reports a snapped tick")
    };
    // Ten thousand pixels down: 1,500° at the fixed 0.5 sensitivity, far past the limit.
    assert_eq!(
        pitch_after_delta(10_000.0),
        90.0,
        "a delta far down clamps the pitch at +90"
    );
    assert_eq!(
        pitch_after_delta(-10_000.0),
        -90.0,
        "a delta far up clamps the pitch at -90"
    );
}

/// One ground-up column whose section 3 is solid stone: the floor's top
/// surface at y 64, air above and below the sent sections.
fn floor_column_frame(cx: i32, cz: i32) -> Vec<u8> {
    let mut section = vec![0u8; 8192];
    for cell in section.chunks_exact_mut(2) {
        cell.copy_from_slice(&0x0010u16.to_le_bytes());
    }
    let mut payload = section;
    payload.extend_from_slice(&[0u8; 2048]); // block light
    payload.extend_from_slice(&[0xFFu8; 2048]); // sky light
    payload.extend_from_slice(&[1u8; 256]); // biomes
    column_frame(cx, cz, true, 0x0008, &payload)
}

#[test]
fn a_held_key_walks_the_player_over_the_floor() {
    // The first live-movement session test: a join carrying a stone floor, the
    // player teleported onto its surface facing south (yaw 0), and a quiet
    // stretch with the forward key held. Every due tick steps the movement
    // model against the world — the player walks along the floor, the height
    // and the ground contact hold, and the walk keeps its heading.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );

    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(Vec::new()),
        // Half a second of quiet: enough whole steps for the walk to show.
        stalls: 24,
        tail_stalls: 0,
        outgoing: Arc::new(Mutex::new(Vec::new())),
    };
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    input_tx
        .send(InputEvent::Key {
            key: Key::W,
            pressed: true,
        })
        .expect("the input channel is open");
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, input_rx)
        .expect("the session runs to the end of the stream");

    let walks: Vec<(f64, f64, f64, bool)> = receiver
        .try_iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick {
                x,
                y,
                z,
                on_ground,
                snapped: false,
                ..
            } => Some((x, y, z, on_ground)),
            _ => None,
        })
        .collect();
    assert!(
        walks.len() >= 4,
        "the quiet stretch owes whole steps: {walks:?}"
    );
    // The floor carries the walk: the feet stay on its surface, never sink.
    assert!(
        walks.iter().all(|(_, y, _, _)| (*y - 64.0).abs() < 1e-9),
        "the floor holds the height: {walks:?}"
    );
    // The forward key walks south (+Z at yaw 0): every step gains ground.
    assert!(
        walks.windows(2).all(|pair| pair[1].2 > pair[0].2),
        "every step walks south: {walks:?}"
    );
    // The heading holds: no drift across the floor.
    assert!(
        walks.iter().all(|(x, _, _, _)| (*x - 0.5).abs() < 1e-9),
        "the walk keeps its heading: {walks:?}"
    );
    assert!(
        walks.last().expect("steps").3,
        "the last step is on the ground: {walks:?}"
    );
}
