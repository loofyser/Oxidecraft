//! Replay tests: a scripted 1.8.9 server stream drives the session, and the
//! client's own traffic and events are asserted byte for byte.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oxide_game::session::{ClientEvent, Session, SessionConfig, SessionError};
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
        .run_over(&sender)
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
        .run_over(&sender)
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
        .run_over(&sender)
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

/// A duplex with a quiet stretch: after `head` is consumed, the stream reports
/// nothing readable for `stalls` waits, then serves `tail`.
struct GappedDuplex {
    head: std::io::Cursor<Vec<u8>>,
    tail: std::io::Cursor<Vec<u8>>,
    stalls: usize,
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
        // Three deadlines of quiet, so the pool has three tick windows to
        // finish the mesh the chunk packet queued before the tail arrives.
        stalls: 3,
        outgoing: Arc::clone(&outgoing),
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender)
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
        .run_over(&sender)
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
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
        .run_over(&sender)
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
        .run_over(&sender)
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
    // plains, `/time set 6000`'s own frame, the same frame with the time negated — the frozen
    // sun — a teleport inside that column, and a third day's frame. The clock comes back per
    // update and the sky follows it and the view block, because the client cannot sample the
    // world itself.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0),
        chunk_data_frame(0, 0),
        time_update_frame(48_000, 6000),
        time_update_frame(48_000, -6001),
        position_frame(12.5, 65.0, 4.5, 0.0, 0.0, 0),
        time_update_frame(48_000, 72_000),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    let session = Session::new(Conn::new(stream), config());
    session
        .run_over(&sender)
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
        vec![(48_000, 6000), (48_000, -6001), (48_000, 72_000)],
        "every Time Update is reported, the sign of the time kept as received"
    );

    // The sky's shape carries the view-block colour and the other world-derived values.
    let skies: Vec<SkyReport> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Sky {
                celestial_angle,
                colour,
                sun_brightness,
                star_brightness,
                cloud_colour,
                moon_phase,
            } => Some(SkyReport {
                celestial_angle: *celestial_angle,
                colour: *colour,
                sun_brightness: *sun_brightness,
                star_brightness: *star_brightness,
                cloud_colour: *cloud_colour,
                moon_phase: *moon_phase,
            }),
            _ => None,
        })
        .collect();
    // The JVM harness's own frozen angle (`refs/m2-task-12/sky_literals.out`, `angle t=-6001`).
    let midnight = 0.49993455;
    let frozen = SkyReport {
        celestial_angle: midnight,
        colour: [0.0, 0.0, 0.0],
        sun_brightness: 0.2,
        star_brightness: 0.5,
        cloud_colour: [0.1, 0.1, 0.15],
        moon_phase: 0,
    };
    assert_eq!(
        skies,
        vec![
            // The plains noon the loaded column's biome gives.
            SkyReport {
                celestial_angle: 0.0,
                colour: [120.0 / 255.0, 167.0 / 255.0, 1.0],
                sun_brightness: 1.0,
                star_brightness: 0.0,
                cloud_colour: [1.0, 1.0, 1.0],
                moon_phase: 0,
            },
            // The frozen midnight, reported again for the moved view block.
            frozen,
            frozen,
            // The third day's sunrise angle (`angle t=0` is the same day fraction), whose
            // `worldTime / 24000 % 8` is phase 3.
            SkyReport {
                celestial_angle: 0.8535534,
                colour: [120.0 / 255.0, 167.0 / 255.0, 1.0],
                sun_brightness: 1.0,
                star_brightness: 0.0,
                cloud_colour: [1.0, 1.0, 1.0],
                moon_phase: 3,
            },
        ],
        "the sky colour follows the clock and the biome under the player"
    );
    let last_position = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::PlayerPosition { .. }))
        .expect("the teleports are reported");
    let last_sky = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Sky { .. }))
        .expect("the sky is reported");
    assert!(
        last_position < last_sky,
        "a moved view block reports the sky again: {events:?}"
    );
}
