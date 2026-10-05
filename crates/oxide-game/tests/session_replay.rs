//! Replay tests: a scripted 1.8.9 server stream drives the session, and the
//! client's own traffic and events are asserted byte for byte.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oxide_game::entity_view::{EntityExtra, EntityFrame, PlayerListRecord, display_name};
use oxide_game::input::{InputEvent, Key, MouseButton};
use oxide_game::interaction::{Aim, Face};
use oxide_game::player::MAX_HURT_TIME;
use oxide_game::scoreboard::{Objective, Scoreboard, Team, entry_colour};
use oxide_game::session::{ClientEvent, Session, SessionConfig, SessionError, recompute_passes};
use oxide_game::ticker::TICK_CATCHUP_CAP;
use oxide_proto::conn::{Conn, DeadlineStream};
use oxide_proto::frame::{Compression, write_frame};
use oxide_proto_v47::clientbound::{MapChunkBulk, PlayerPositionAndLook};
use oxide_proto_v47::column::block_index;
use oxide_proto_v47::serverbound::ClientSettings;
use oxide_render::terrain::{ChunkMesh, Vertex};
use oxide_world::entity::EntityKind;
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

/// One ground-up Chunk Data column for a dimension without sky light: one
/// stone block at its origin, dark block light and plains biomes — the sky
/// light array the Overworld's columns carry is absent, because the reader
/// takes it only when the dimension has a sky
/// (`NetHandlerPlayClient.handleChunkData:1187-1196`).
fn sky_less_column_frame(cx: i32, cz: i32) -> Vec<u8> {
    let mut column = vec![0u8; 8192];
    column[..2].copy_from_slice(&0x0010u16.to_le_bytes());
    column.extend_from_slice(&[0u8; 2048]); // block light
    column.extend_from_slice(&[1u8; 256]); // biomes
    column_frame(cx, cz, true, 0x0001, &column)
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

/// One Spawn Player frame (0x0C): the id, the profile UUID, the fixed-point
/// position, the two angle bytes, the current item and an empty metadata
/// block.
fn spawn_player_frame(
    entity_id: i32,
    uuid: [u8; 16],
    x: f64,
    y: f64,
    z: f64,
    yaw: u8,
    pitch: u8,
) -> Vec<u8> {
    let mut payload = vec![0x0c];
    push_varint(&mut payload, entity_id);
    payload.extend_from_slice(&uuid);
    for value in [x, y, z] {
        payload.extend_from_slice(&((value * 32.0) as i32).to_be_bytes());
    }
    payload.extend_from_slice(&[yaw, pitch]);
    payload.extend_from_slice(&0i16.to_be_bytes()); // the current item
    payload.push(0x7f); // the metadata terminator
    payload
}

/// One Spawn Mob frame (0x0F): the id, the type byte, the fixed-point
/// position, the three angle bytes, a zeroed velocity triple and an empty
/// metadata block.
#[allow(clippy::too_many_arguments)]
fn spawn_mob_frame(
    entity_id: i32,
    type_id: u8,
    x: f64,
    y: f64,
    z: f64,
    yaw: u8,
    pitch: u8,
    head_yaw: u8,
) -> Vec<u8> {
    let mut payload = vec![0x0f];
    push_varint(&mut payload, entity_id);
    payload.push(type_id);
    for value in [x, y, z] {
        payload.extend_from_slice(&((value * 32.0) as i32).to_be_bytes());
    }
    payload.extend_from_slice(&[yaw, pitch, head_yaw]);
    payload.extend_from_slice(&[0u8; 6]); // the velocity triple
    payload.push(0x7f); // the metadata terminator
    payload
}

/// One Spawn Object frame (0x0E); a positive `data` adds the velocity triple.
fn spawn_object_frame(entity_id: i32, type_id: u8, x: f64, y: f64, z: f64, data: i32) -> Vec<u8> {
    let mut payload = vec![0x0e];
    push_varint(&mut payload, entity_id);
    payload.push(type_id);
    for value in [x, y, z] {
        payload.extend_from_slice(&((value * 32.0) as i32).to_be_bytes());
    }
    payload.extend_from_slice(&[0, 0]); // the pitch and yaw bytes
    payload.extend_from_slice(&data.to_be_bytes());
    if data > 0 {
        payload.extend_from_slice(&[0u8; 6]); // the velocity triple
    }
    payload
}

/// One Spawn Experience Orb frame (0x11).
fn spawn_xp_orb_frame(entity_id: i32, x: f64, y: f64, z: f64, count: i16) -> Vec<u8> {
    let mut payload = vec![0x11];
    push_varint(&mut payload, entity_id);
    for value in [x, y, z] {
        payload.extend_from_slice(&((value * 32.0) as i32).to_be_bytes());
    }
    payload.extend_from_slice(&count.to_be_bytes());
    payload
}

/// One Spawn Painting frame (0x10): the title, the packed block position and
/// the facing byte.
fn spawn_painting_frame(
    entity_id: i32,
    title: &str,
    x: i32,
    y: i32,
    z: i32,
    facing: u8,
) -> Vec<u8> {
    let mut payload = vec![0x10];
    push_varint(&mut payload, entity_id);
    push_string(&mut payload, title);
    let packed =
        ((x as i64 & 0x3ffffff) << 38) | ((y as i64 & 0xfff) << 26) | (z as i64 & 0x3ffffff);
    payload.extend_from_slice(&packed.to_be_bytes());
    payload.push(facing);
    payload
}

/// One Spawn Global Entity frame (0x2C).
fn spawn_global_frame(entity_id: i32, type_id: u8, x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut payload = vec![0x2c];
    push_varint(&mut payload, entity_id);
    payload.push(type_id);
    for value in [x, y, z] {
        payload.extend_from_slice(&((value * 32.0) as i32).to_be_bytes());
    }
    payload
}

/// One Entity Relative Move frame (0x15): the deltas in 1/32 blocks and the
/// trailing ground flag.
fn relative_move_frame(entity_id: i32, dx: i8, dy: i8, dz: i8) -> Vec<u8> {
    let mut payload = vec![0x15];
    push_varint(&mut payload, entity_id);
    payload.extend_from_slice(&[dx as u8, dy as u8, dz as u8, 1]);
    payload
}

/// One Entity Look frame (0x16).
fn entity_look_frame(entity_id: i32, yaw: u8, pitch: u8) -> Vec<u8> {
    let mut payload = vec![0x16];
    push_varint(&mut payload, entity_id);
    payload.extend_from_slice(&[yaw, pitch, 1]);
    payload
}

/// One Entity Look And Relative Move frame (0x17): the deltas in 1/32 blocks,
/// the angle bytes and the trailing ground flag.
fn entity_look_and_move_frame(
    entity_id: i32,
    dx: i8,
    dy: i8,
    dz: i8,
    yaw: u8,
    pitch: u8,
) -> Vec<u8> {
    let mut payload = vec![0x17];
    push_varint(&mut payload, entity_id);
    payload.extend_from_slice(&[dx as u8, dy as u8, dz as u8, yaw, pitch, 1]);
    payload
}

/// One Entity Teleport frame (0x18).
fn entity_teleport_frame(entity_id: i32, x: f64, y: f64, z: f64, yaw: u8, pitch: u8) -> Vec<u8> {
    let mut payload = vec![0x18];
    push_varint(&mut payload, entity_id);
    for value in [x, y, z] {
        payload.extend_from_slice(&((value * 32.0) as i32).to_be_bytes());
    }
    payload.extend_from_slice(&[yaw, pitch, 1]);
    payload
}

/// One Entity Head Look frame (0x19).
fn entity_head_look_frame(entity_id: i32, head_yaw: u8) -> Vec<u8> {
    let mut payload = vec![0x19];
    push_varint(&mut payload, entity_id);
    payload.push(head_yaw);
    payload
}

/// One Entity Velocity frame (0x12): the shorts in 1/8000 blocks per tick.
fn entity_velocity_frame(entity_id: i32, vx: i16, vy: i16, vz: i16) -> Vec<u8> {
    let mut payload = vec![0x12];
    push_varint(&mut payload, entity_id);
    for value in [vx, vy, vz] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}

/// One Attach Entity frame (0x1B): the attached id and the holder as the
/// source's two ints (`S1BPacketEntityAttach.readPacketData:29-34`), then the
/// leash byte.
fn attach_frame(entity_id: i32, holder: i32, leash: bool) -> Vec<u8> {
    let mut payload = vec![0x1b];
    payload.extend_from_slice(&entity_id.to_be_bytes());
    payload.extend_from_slice(&holder.to_be_bytes());
    payload.push(leash as u8);
    payload
}

/// One Collect Item frame (0x0D).
fn collect_item_frame(collected: i32, collector: i32) -> Vec<u8> {
    let mut payload = vec![0x0d];
    push_varint(&mut payload, collected);
    push_varint(&mut payload, collector);
    payload
}

/// One Destroy Entities frame (0x13).
fn destroy_entities_frame(entity_ids: &[i32]) -> Vec<u8> {
    let mut payload = vec![0x13];
    push_varint(&mut payload, entity_ids.len() as i32);
    for id in entity_ids {
        push_varint(&mut payload, *id);
    }
    payload
}

/// One Entity Equipment frame (0x04): one non-empty item with no NBT.
fn entity_equipment_frame(entity_id: i32, slot: i16, item: (i16, u8, i16)) -> Vec<u8> {
    let mut payload = vec![0x04];
    push_varint(&mut payload, entity_id);
    payload.extend_from_slice(&slot.to_be_bytes());
    payload.extend_from_slice(&item.0.to_be_bytes());
    payload.push(item.1);
    payload.extend_from_slice(&item.2.to_be_bytes());
    payload.push(0); // no NBT
    payload
}

/// One Animation frame (0x0B).
fn animation_frame(entity_id: i32, animation: u8) -> Vec<u8> {
    let mut payload = vec![0x0b];
    push_varint(&mut payload, entity_id);
    payload.push(animation);
    payload
}

/// One Entity frame (0x14) — the no-op.
fn entity_frame(entity_id: i32) -> Vec<u8> {
    let mut payload = vec![0x14];
    push_varint(&mut payload, entity_id);
    payload
}

/// One Entity Metadata frame (0x1C) from literal `(index, tag, payload)`
/// entries.
fn entity_metadata_frame(entity_id: i32, entries: &[(u8, u8, Vec<u8>)]) -> Vec<u8> {
    let mut payload = vec![0x1c];
    push_varint(&mut payload, entity_id);
    for (index, tag, value) in entries {
        payload.push((tag << 5) | index);
        payload.extend_from_slice(value);
    }
    payload.push(0x7f); // the metadata terminator
    payload
}

/// A byte metadata entry's payload.
fn meta_byte(value: i8) -> Vec<u8> {
    vec![value as u8]
}

/// A float metadata entry's payload.
fn meta_float(value: f32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// A string metadata entry's payload.
fn meta_string(value: &str) -> Vec<u8> {
    let mut bytes = vec![value.len() as u8];
    bytes.extend_from_slice(value.as_bytes());
    bytes
}

/// Player List Item, add action, one entry with a property and an optional
/// display name.
fn player_list_add_full_frame(uuid: [u8; 16], name: &str, display: Option<&str>) -> Vec<u8> {
    let mut payload = vec![0x38];
    push_varint(&mut payload, 0); // the add action
    push_varint(&mut payload, 1); // one entry
    payload.extend_from_slice(&uuid);
    push_string(&mut payload, name);
    push_varint(&mut payload, 1); // one property
    push_string(&mut payload, "textures");
    push_string(&mut payload, "eyJx");
    payload.push(0); // unsigned
    push_varint(&mut payload, 1); // gamemode: creative
    push_varint(&mut payload, 42); // ping
    match display {
        Some(display) => {
            payload.push(1);
            push_string(&mut payload, display);
        }
        None => payload.push(0),
    }
    payload
}

/// Player List Item, update-game-mode action (1).
fn player_list_gamemode_frame(uuid: [u8; 16], gamemode: i32) -> Vec<u8> {
    let mut payload = vec![0x38];
    push_varint(&mut payload, 1);
    push_varint(&mut payload, 1);
    payload.extend_from_slice(&uuid);
    push_varint(&mut payload, gamemode);
    payload
}

/// Player List Item, update-display-name action (3); a null clears the name.
fn player_list_display_frame(uuid: [u8; 16], display: Option<&str>) -> Vec<u8> {
    let mut payload = vec![0x38];
    push_varint(&mut payload, 3);
    push_varint(&mut payload, 1);
    payload.extend_from_slice(&uuid);
    match display {
        Some(display) => {
            payload.push(1);
            push_string(&mut payload, display);
        }
        None => payload.push(0),
    }
    payload
}

/// Player List Item, remove action (4).
fn player_list_remove_frame(uuid: [u8; 16]) -> Vec<u8> {
    let mut payload = vec![0x38];
    push_varint(&mut payload, 4);
    push_varint(&mut payload, 1);
    payload.extend_from_slice(&uuid);
    payload
}

/// Scoreboard Objective (0x3B): the name, the mode and — for the create and
/// update modes — the display value and the render kind.
fn scoreboard_objective_frame(name: &str, mode: u8, value: &str, kind: &str) -> Vec<u8> {
    let mut payload = vec![0x3b];
    push_string(&mut payload, name);
    payload.push(mode);
    if mode == 0 || mode == 2 {
        push_string(&mut payload, value);
        push_string(&mut payload, kind);
    }
    payload
}

/// Update Score (0x3C): the entry, the mode, the objective name and — for
/// the set mode — the value.
fn scoreboard_score_frame(entry: &str, mode: u8, objective: &str, value: i32) -> Vec<u8> {
    let mut payload = vec![0x3c];
    push_string(&mut payload, entry);
    payload.push(mode);
    push_string(&mut payload, objective);
    if mode == 0 {
        push_varint(&mut payload, value);
    }
    payload
}

/// Display Scoreboard (0x3D): the slot and the objective name.
fn scoreboard_display_frame(slot: u8, objective: &str) -> Vec<u8> {
    let mut payload = vec![0x3d];
    payload.push(slot);
    push_string(&mut payload, objective);
    payload
}

/// Teams (0x3E) with its info block (create and update modes) and, for the
/// create mode, the player list.
#[allow(clippy::too_many_arguments)]
fn scoreboard_team_info_frame(
    name: &str,
    mode: u8,
    display_name: &str,
    prefix: &str,
    suffix: &str,
    friendly_flags: u8,
    name_tag_visibility: &str,
    colour: u8,
    players: &[&str],
) -> Vec<u8> {
    let mut payload = vec![0x3e];
    push_string(&mut payload, name);
    payload.push(mode);
    if mode == 0 || mode == 2 {
        push_string(&mut payload, display_name);
        push_string(&mut payload, prefix);
        push_string(&mut payload, suffix);
        payload.push(friendly_flags);
        push_string(&mut payload, name_tag_visibility);
        payload.push(colour);
    }
    if mode == 0 {
        push_varint(&mut payload, players.len() as i32);
        for player in players {
            push_string(&mut payload, player);
        }
    }
    payload
}

/// Teams (0x3E) with a player list (add-players and remove-players modes).
fn scoreboard_team_players_frame(name: &str, mode: u8, players: &[&str]) -> Vec<u8> {
    let mut payload = vec![0x3e];
    push_string(&mut payload, name);
    payload.push(mode);
    push_varint(&mut payload, players.len() as i32);
    for player in players {
        push_string(&mut payload, player);
    }
    payload
}

/// Teams (0x3E), the remove mode: the name alone.
fn scoreboard_team_remove_frame(name: &str) -> Vec<u8> {
    let mut payload = vec![0x3e];
    push_string(&mut payload, name);
    payload.push(1);
    payload
}

/// Player List Header And Footer (0x47): the header and the footer.
fn tab_header_footer_frame(header: &str, footer: &str) -> Vec<u8> {
    let mut payload = vec![0x47];
    push_string(&mut payload, header);
    push_string(&mut payload, footer);
    payload
}

/// Runs one feed session: the caller's `head` bytes, `stalls` idle windows,
/// the `tail` bytes, then `tail_stalls` more idle windows before the end.
fn run_feed_session(
    head: Vec<u8>,
    tail: Vec<u8>,
    stalls: usize,
    tail_stalls: usize,
) -> (Vec<ClientEvent>, Arc<Mutex<Vec<u8>>>) {
    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(tail),
        stalls,
        tail_stalls,
        outgoing: Arc::clone(&outgoing),
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");
    (receiver.try_iter().collect(), outgoing)
}

/// The scripted prefix a feed session starts with: the login sequence, Join
/// Game and one running Time Update, then `payloads` under the server's
/// framing.
fn feed_head(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &time_update_frame(48_000, 6000), SERVER_FRAMING);
    for payload in payloads {
        frame(&mut head, payload, SERVER_FRAMING);
    }
    head
}

/// The `payloads` under the server's framing.
fn framed(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for payload in payloads {
        frame(&mut out, payload, SERVER_FRAMING);
    }
    out
}

/// The entity frames of every feed in `events`, in tick order.
fn feeds(events: &[ClientEvent]) -> Vec<Vec<EntityFrame>> {
    events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::EntitiesTick { entities } => Some(entities.clone()),
            _ => None,
        })
        .collect()
}

/// One frame by entity id.
fn frame_of(frames: &[EntityFrame], id: i32) -> &EntityFrame {
    frames
        .iter()
        .find(|frame| frame.id == id)
        .unwrap_or_else(|| panic!("id {id} is tracked: {frames:?}"))
}

/// Plugin Message on `channel` with `data` as its payload.
fn plugin_message_frame(channel: &str, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x3f];
    push_string(&mut payload, channel);
    payload.extend_from_slice(data);
    payload
}

/// Stone, id 1, meta 0: the packed value `1 << 4 | 0`.
const STONE: u16 = 0x0010;

/// One Block Change frame (0x23): the Location Position and the block value.
///
/// The literal-level pinning of the packing lives in the decoder's own
/// fixtures (`oxide-proto-v47`'s `clientbound` tests); here the frame only
/// has to drive the session.
fn block_change_frame(x: i32, y: i32, z: i32, value: u16) -> Vec<u8> {
    let mut payload = vec![0x23];
    let packed =
        ((x as i64 & 0x3FFFFFF) << 38) | ((y as i64 & 0xFFF) << 26) | (z as i64 & 0x3FFFFFF);
    payload.extend_from_slice(&packed.to_be_bytes());
    push_varint(&mut payload, value as i32);
    payload
}

/// One Multi Block Change frame (0x22): the chunk, then each record's crammed
/// position short and block value.
fn multi_block_change_frame(chunk_x: i32, chunk_z: i32, records: &[(u16, u16)]) -> Vec<u8> {
    let mut payload = vec![0x22];
    payload.extend_from_slice(&chunk_x.to_be_bytes());
    payload.extend_from_slice(&chunk_z.to_be_bytes());
    push_varint(&mut payload, records.len() as i32);
    for (crammed, value) in records {
        payload.extend_from_slice(&crammed.to_be_bytes());
        push_varint(&mut payload, *value as i32);
    }
    payload
}

/// The light every vertex of a relit mesh must carry: block light 0 — the
/// wire's sentinel 15 was recomputed away, since nothing emits — and sky
/// light at least 14 — the wire's sentinel 0 was replaced by the computed
/// values of the open column.
fn assert_relit(mesh: &ChunkMesh) {
    for vertex in mesh.layers.iter().flat_map(|layer| layer.vertices.iter()) {
        assert_eq!(
            vertex.light[0], 8,
            "the block kind was recomputed to 0: {vertex:?}"
        );
        assert!(
            vertex.light[1] >= 14 * 16 + 8,
            "and the sky kind to at least 14: {vertex:?}"
        );
    }
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

/// The skies the frames themselves report: each `Sky` preceded by the Time
/// Update or the snapped tick that produced it, with the snapped tick's own
/// entity feed between them. A tick that advances a running clock adds skies
/// of its own between the frames — the sun travels at the tick rate — so the
/// frame-driven reports are identified by their marker, not by position.
fn frame_skies(events: &[ClientEvent]) -> Vec<SkyReport> {
    let mut skies = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let Some(report) = sky_report(event) else {
            continue;
        };
        // The marker is the nearest event behind the sky, looking one step
        // further back through a snapped tick's own feed.
        let mut behind = events[..index].iter().rev();
        let mut marker = behind.next();
        if matches!(marker, Some(ClientEvent::EntitiesTick { .. })) {
            marker = behind.next();
        }
        if matches!(
            marker,
            Some(ClientEvent::Time { .. } | ClientEvent::PlayerTick { snapped: true, .. })
        ) {
            skies.push(report);
        }
    }
    skies
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
    // unsnapped ticks and the per-tick entity feeds are dropped so the
    // frame-driven story reads as before.
    let events: Vec<ClientEvent> = receiver
        .try_iter()
        .filter(|event| {
            !matches!(
                event,
                ClientEvent::PlayerTick { snapped: false, .. } | ClientEvent::EntitiesTick { .. }
            )
        })
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
    assert_eq!(keep_alive, [0x00, 0x07], "the keepalive echo");
    // The ticks that ran while the script was read sent their walking reports
    // after it; nothing else follows the preamble.
    tail_reports(&mut cursor);
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
fn a_block_change_lands_with_its_light_and_a_chunk_update() {
    // A ground-up column whose wire light is a sentinel: block light 15 and
    // sky light 0 everywhere, with one stone block at local (1, 1, 1). The
    // 0x23 then places a second stone at world (4, 1, 1). The packet carries
    // no light, so the local pipeline recomputes it: the block kind drops to
    // 0 everywhere (nothing emits) and the sky kind becomes the computed one
    // — `assert_relit` proves both — while the mesh's two blocks prove the
    // write landed and the `ChunkUpdated` proves the invalidation ran.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        lit_column_frame(0, 0, &[0], 15, 0, true),
        block_change_frame(4, 1, 1, STONE),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 0)]),
        "the changed column is the only one the change marked: {events:?}"
    );
    let slots = last_updated_slots(&events, 0, 0);
    let mesh = slots[0].1.as_ref().expect("section 0 draws its blocks");
    assert_eq!(
        mesh.vertex_count(),
        48,
        "the column's stone and the changed one, six faces each"
    );
    assert_relit(mesh);
}

#[test]
fn a_multi_block_change_applies_every_record_in_a_negative_chunk() {
    // One column at chunk (-1, -1) with the same sentinel light (block 15,
    // sky 0) and one stone at local (1, 1, 1). The 0x22 carries two records
    // in that chunk, crammed as the literals 0x4101 — local (4, 1, 1) — and
    // 0x6203 — local (6, 3, 2). Both records must land: the mesh draws three
    // blocks. The world coordinates compose as chunk * 16 + local, so the
    // negative chunk is the column that rebuilds, with the same local relight
    // as the single-block path.
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        lit_column_frame(-1, -1, &[0], 15, 0, true),
        multi_block_change_frame(-1, -1, &[(0x4101, STONE), (0x6203, STONE)]),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(-1, -1)]),
        "the negative chunk is the only column touched: {events:?}"
    );
    let slots = last_updated_slots(&events, -1, -1);
    let mesh = slots[0].1.as_ref().expect("section 0 draws its blocks");
    assert_eq!(mesh.vertex_count(), 72, "three blocks, six faces each");
    assert_relit(mesh);
}

#[test]
fn a_multi_block_change_with_an_extreme_chunk_coordinate_is_refused() {
    // The chunk coordinate is a raw i32 off the wire and the world position
    // composes as `chunk * 16 + local`: i32::MAX and i32::MIN overflow that
    // composition, which a checked build panics on. The handler refuses the
    // packet instead — an error, never a panic.
    for chunk_x in [i32::MAX, i32::MIN] {
        let (stream, _outgoing) = duplex(stream_with(&[
            join_game_frame(),
            lit_column_frame(0, 0, &[0], 15, 0, true),
            multi_block_change_frame(chunk_x, 0, &[(0x4101, STONE)]),
        ]));
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let error = Session::new(Conn::new(stream), config())
            .run_over(&sender, silent_inputs())
            .expect_err("an extreme chunk coordinate must be refused");
        assert!(
            matches!(error, SessionError::Packet(_)),
            "chunk_x {chunk_x}: error: {error:?}"
        );
    }
}

/// The largest chunk coordinate magnitude the world carries: the range the
/// light engine's region arithmetic composes without overflow (see
/// `oxide_world::world`).
const CHUNK_RANGE_EDGE: i32 = 134_217_726;

#[test]
fn a_column_below_the_supported_chunk_range_is_refused() {
    // The load is the only way a coordinate reaches the light engine: a
    // change lands in a loaded column, and its recomputation composes the
    // region around that column, one chunk past its edges. A column loaded
    // at -2^27 puts the region's far base one chunk below -2^27, where the
    // region math multiplies past i32::MIN and a checked build panics. The
    // load must refuse the coordinate instead of loading the column whose
    // next change panics — an error, never a panic.
    let cx = -(1 << 27);
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        chunk_data_frame(cx, 0),
        multi_block_change_frame(cx, 0, &[(0x4101, STONE)]),
    ]));
    let (sender, _receiver) = crossbeam_channel::unbounded();
    let error = Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect_err("a column below the supported range must be refused");
    assert!(matches!(error, SessionError::Packet(_)), "error: {error:?}");
}

#[test]
fn a_column_above_the_supported_chunk_range_is_refused() {
    // 2^27 - 1 is the other side of the same edge: the region's rightmost
    // cells sit at i32::MAX, and the spread's one-cell probe past one of
    // them adds one more, which a checked build panics on. The load must
    // refuse the coordinate; the change behind it proves the panic the
    // refused load forecloses.
    let cx = (1 << 27) - 1;
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        chunk_data_frame(cx, 0),
        multi_block_change_frame(cx, 0, &[(0x4101, STONE)]),
    ]));
    let (sender, _receiver) = crossbeam_channel::unbounded();
    let error = Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect_err("a column above the supported range must be refused");
    assert!(matches!(error, SessionError::Packet(_)), "error: {error:?}");
}

#[test]
fn a_column_at_the_supported_chunk_range_edge_loads_and_relights() {
    // The edge is the largest magnitude the region math composes: the edge
    // times 16, plus the spread's one-cell probe, is 2147483632, fifteen
    // below i32::MAX. A column at the edge must load, run a change and
    // rebuild its mesh with no overflow, in both signs and on both axes.
    for (cx, cz) in [
        (CHUNK_RANGE_EDGE, 0),
        (-CHUNK_RANGE_EDGE, 0),
        (0, CHUNK_RANGE_EDGE),
        (0, -CHUNK_RANGE_EDGE),
    ] {
        let (stream, _outgoing) = duplex(stream_with(&[
            join_game_frame(),
            lit_column_frame(cx, cz, &[0], 15, 0, true),
            multi_block_change_frame(cx, cz, &[(0x4101, STONE)]),
        ]));
        let (sender, receiver) = crossbeam_channel::unbounded();
        Session::new(Conn::new(stream), config())
            .run_over(&sender, silent_inputs())
            .expect("a column at the range's edge runs");
        let events: Vec<ClientEvent> = receiver.try_iter().collect();
        assert_eq!(
            updated_columns(&events),
            BTreeSet::from([(cx, cz)]),
            "({cx}, {cz}) is the only column the change marks: {events:?}"
        );
        let slots = last_updated_slots(&events, cx, cz);
        let mesh = slots[0].1.as_ref().expect("section 0 draws its blocks");
        assert_eq!(
            mesh.vertex_count(),
            48,
            "({cx}, {cz}): the column's stone and the changed one, six faces each"
        );
        assert_relit(mesh);
    }
}

#[test]
fn a_column_one_step_outside_the_supported_chunk_range_is_refused() {
    // One step past the edge is the first magnitude that fails to compose:
    // the probe past 2^27 - 1 overflows. The store refuses it, on every sign
    // and axis, and each run carries the change that would panic if the load
    // were accepted.
    let outside = CHUNK_RANGE_EDGE + 1;
    for (cx, cz) in [(outside, 0), (-outside, 0), (0, outside), (0, -outside)] {
        let (stream, _outgoing) = duplex(stream_with(&[
            join_game_frame(),
            chunk_data_frame(cx, cz),
            multi_block_change_frame(cx, cz, &[(0x4101, STONE)]),
        ]));
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let error = Session::new(Conn::new(stream), config())
            .run_over(&sender, silent_inputs())
            .expect_err("a column one step outside the range must be refused");
        assert!(
            matches!(error, SessionError::Packet(_)),
            "({cx}, {cz}): error: {error:?}"
        );
    }
}

#[test]
fn a_bulk_column_outside_the_supported_chunk_range_is_refused() {
    // Map Chunk Bulk is the other load route and takes the same check: a
    // bulk carrying one column below the range is refused whole, before any
    // of its columns applies, so even the legal column is not loaded.
    let cx = -(1 << 27);
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        bulk_frame_of(&[(0, 0), (cx, 0)]),
        multi_block_change_frame(cx, 0, &[(0x4101, STONE)]),
    ]));
    let (sender, _receiver) = crossbeam_channel::unbounded();
    let error = Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect_err("a bulk with a column outside the range must be refused");
    assert!(matches!(error, SessionError::Packet(_)), "error: {error:?}");
}

#[test]
fn a_multi_block_change_runs_one_light_pass_for_the_whole_packet() {
    // The packet's records all compose into its own chunk, so their recompute
    // regions coincide and one pass covers the union of them. The count comes
    // from the session's own pass counter, read around the run.
    let before = recompute_passes();
    let (stream, _outgoing) = duplex(stream_with(&[
        join_game_frame(),
        lit_column_frame(0, 0, &[0], 15, 0, true),
        multi_block_change_frame(0, 0, &[(0x4101, STONE), (0x6203, STONE), (0x8305, STONE)]),
    ]));
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, silent_inputs())
        .expect("the session runs to the end of the stream");

    let passes = recompute_passes() - before;
    assert_eq!(
        passes, 1,
        "three records in one chunk are one region: one pass, not one per record"
    );
    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert_eq!(
        updated_columns(&events),
        BTreeSet::from([(0, 0)]),
        "the packet's column is invalidated: {events:?}"
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
    // Nothing was sent for the skipped packets: the keepalive echo is on the
    // wire once, among the walking reports the ticks sent around it.
    tail_with_echo(&outgoing, 11);
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
    // The keepalive echo is on the wire once, among the walking reports the
    // ticks sent around it; the player list drew nothing else.
    tail_with_echo(&outgoing, 12);
}

#[test]
fn every_tick_feeds_the_tracked_entities_exactly_once() {
    // One zombie spawns and a burst of relative moves lands between ticks;
    // the quiet stretch then runs the session's own ticks. Every tick's
    // PlayerTick is followed at once by exactly one entities feed — the
    // burst adds no events of its own — and once the moves have settled,
    // identical consecutive ticks are still fed: the cadence has no change
    // detection.
    let mut extra = vec![spawn_mob_frame(21, 54, 10.0, 64.0, -3.5, 64, 0, 64)];
    for _ in 0..8 {
        extra.push(relative_move_frame(21, 1, 0, 0));
    }
    let (events, outgoing) =
        run_feed_session(feed_head(&extra), framed(&[keep_alive_frame(33)]), 8, 0);

    let player_ticks = events
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { .. }))
        .count();
    let all_feeds = feeds(&events);
    assert!(
        player_ticks >= 3,
        "the quiet stretch owes ticks: {events:?}"
    );
    assert_eq!(
        all_feeds.len(),
        player_ticks,
        "one feed per player tick: {events:?}"
    );
    for (index, event) in events.iter().enumerate() {
        if matches!(event, ClientEvent::EntitiesTick { .. }) {
            assert!(
                index > 0 && matches!(events.get(index - 1), Some(ClientEvent::PlayerTick { .. })),
                "the feed at {index} does not sit behind a player tick: {events:?}"
            );
        }
    }
    // The burst's moves are all in the tick the first feed reports — the
    // spawn's 10.0 plus eight 1/32 steps — and the pose pair has settled:
    // every feed from the first on carries the same pair.
    let first = &all_feeds[0];
    let zombie = frame_of(first, 21);
    assert!(
        (zombie.pos[0] - 10.25).abs() < 1e-9,
        "the burst landed in the first feed: {:?}",
        zombie.pos
    );
    for axis in 0..3 {
        assert!(
            (zombie.pos[axis] - zombie.prev[axis]).abs() < 1e-9,
            "the pair has settled: {:?} / {:?}",
            zombie.prev,
            zombie.pos
        );
    }
    assert!(all_feeds.len() >= 2, "a pair of ticks to compare");
    // The entity's own tick keeps moving its counters — the age rises and the
    // limb swing pair eases — while everything the window reads has settled.
    // Both ticks are still fed, frame for frame, once those counters are
    // neutral: the cadence has no change detection.
    let settled_view = |frames: &[EntityFrame]| -> Vec<EntityFrame> {
        frames
            .iter()
            .map(|frame| {
                let mut frame = frame.clone();
                frame.age = 0;
                frame.limb_swing = 0.0;
                frame.limb_swing_amount = 0.0;
                frame.prev_limb_swing_amount = 0.0;
                frame
            })
            .collect()
    };
    assert_eq!(
        settled_view(&all_feeds[all_feeds.len() - 2]),
        settled_view(&all_feeds[all_feeds.len() - 1]),
        "identical consecutive ticks are still fed"
    );
    // The session stayed healthy across all of it.
    tail_with_echo(&outgoing, 33);
}

#[test]
fn the_entity_arm_family_reaches_the_feed() {
    // Every entity packet this milestone decodes, in one script: the spawns
    // (player, mob, object — including one the world's tables cannot name —
    // orb, painting, global), metadata, look, the look-and-relative-move,
    // teleport, head look, relative move, velocity, status, equipment, the
    // swing animation and the 0x14 no-op, then — behind a quiet stretch —
    // attach, collect, destroy and a player list display-name update.
    let uuid = [0x4a; 16];
    let hyphenated = "4a4a4a4a-4a4a-4a4a-4a4a-4a4a4a4a4a4a";
    let extra = vec![
        player_list_add_full_frame(uuid, "OxideDev", None),
        spawn_player_frame(30, uuid, 0.5, 64.0, -12.5, 64, 32),
        spawn_mob_frame(31, 63, 10.0, 64.0, 0.0, 0, 0, 32),
        entity_metadata_frame(
            31,
            &[(6, 3, meta_float(180.0)), (2, 4, meta_string("Boss"))],
        ),
        spawn_object_frame(32, 78, 2.0, 64.0, 2.0, 0),
        entity_metadata_frame(32, &[(2, 4, meta_string("Stand"))]),
        spawn_object_frame(33, 2, 3.0, 64.0, 3.0, 276),
        spawn_xp_orb_frame(34, 4.0, 64.0, 4.0, 7),
        spawn_painting_frame(35, "Kebab", 5, 64, 5, 2),
        spawn_global_frame(36, 1, 6.0, 64.0, 6.0),
        entity_velocity_frame(31, 800, -400, 0),
        entity_teleport_frame(31, 5.0, 70.0, 2.0, 64, 0),
        entity_look_and_move_frame(31, 8, 0, 0, 128, 8),
        entity_look_frame(31, 32, 0),
        entity_head_look_frame(31, 96),
        relative_move_frame(31, 32, 0, 0),
        entity_status_frame(31, 2),
        entity_equipment_frame(31, 0, (276, 1, 0)),
        animation_frame(31, 0),
        entity_frame(999),
    ];
    let (events, outgoing) = run_feed_session(
        feed_head(&extra),
        framed(&[
            attach_frame(32, 31, false),
            collect_item_frame(33, 31),
            destroy_entities_frame(&[34]),
            player_list_display_frame(uuid, Some("§bOxideDev")),
            keep_alive_frame(45),
        ]),
        6,
        6,
    );

    let all_feeds = feeds(&events);
    assert!(
        all_feeds.len() >= 2,
        "both quiet stretches tick: {events:?}"
    );
    let early = &all_feeds[0];
    assert_eq!(
        early.iter().map(|frame| frame.id).collect::<Vec<_>>(),
        vec![30, 31, 32, 33, 34, 35, 36],
        "every spawned entity is tracked, ascending"
    );
    let player = frame_of(early, 30);
    assert_eq!(player.kind, EntityKind::Player);
    assert_eq!(player.uuid.as_deref(), Some(hyphenated));
    assert_eq!(player.nametag.as_deref(), Some("OxideDev"));
    assert!((player.pos[2] - -12.5).abs() < 1e-9, "{:?}", player.pos);
    let dragon = frame_of(early, 31);
    assert_eq!(dragon.kind, EntityKind::EnderDragon);
    assert_eq!(
        dragon.health,
        Some((180.0, 200.0)),
        "the health pair against the pinned class maximum"
    );
    assert_eq!(dragon.nametag.as_deref(), Some("Boss"));
    assert_eq!(
        dragon.pos,
        [6.25, 70.0, 2.0],
        "the teleport, the look-and-move and the relative move landed in order"
    );
    assert_eq!(dragon.yaw, 45.0, "the look's angle");
    // The head look's raw angle (byte 96 -> 135) lands on the moving mob's
    // chase: the body snaps to the body yaw (45) and the head bounds to 75
    // of it (`EntityBodyHelper.updateRenderAngles:24-58`), so the head reads
    // 120; the pre-bound angle stays as the pair's partner.
    assert_eq!(dragon.head_yaw, 120.0, "the bounded head look's angle");
    assert_eq!(
        dragon.prev_head_yaw, 135.0,
        "the pair's partner keeps the raw head look"
    );
    assert_eq!(
        dragon.hurt_ticks, 9,
        "status 2's ten-tick window, one tick in"
    );
    assert_eq!(dragon.brightness, 0.0, "no columns are loaded");
    let stand = frame_of(early, 32);
    assert_eq!(
        stand.kind,
        EntityKind::Unknown,
        "an id the world's tables cannot name is tracked"
    );
    assert_eq!(stand.nametag.as_deref(), Some("Stand"));
    assert_eq!(stand.extra, EntityExtra::None);
    assert_eq!(
        frame_of(early, 33).extra,
        EntityExtra::Item {
            id: 276,
            count: 1,
            damage: 0
        }
    );
    assert_eq!(frame_of(early, 34).extra, EntityExtra::Orb);
    assert_eq!(
        frame_of(early, 35).extra,
        EntityExtra::Painting {
            title: "Kebab".into(),
            facing: 2
        }
    );
    assert_eq!(frame_of(early, 36).kind, EntityKind::Global);
    assert_eq!(frame_of(early, 36).extra, EntityExtra::None);

    let late = all_feeds.last().expect("a last feed");
    assert!(
        !late.iter().any(|frame| frame.id == 33),
        "the collected item is gone: {late:?}"
    );
    assert!(
        !late.iter().any(|frame| frame.id == 34),
        "the destroyed orb is gone: {late:?}"
    );
    assert_eq!(
        frame_of(late, 32).nametag.as_deref(),
        Some("Stand"),
        "the passenger keeps its name"
    );
    assert_eq!(
        frame_of(late, 31).nametag,
        None,
        "the ridden mount's name hides"
    );
    assert_eq!(
        frame_of(late, 30).nametag.as_deref(),
        Some("§bOxideDev"),
        "the display-name update landed"
    );
    assert!(
        frame_of(late, 31).hurt_ticks < 9,
        "the hurt window counts down"
    );
    tail_with_echo(&outgoing, 45);
}

#[test]
fn the_player_list_orders_names_and_updates_around_the_feed() {
    // Three players: the second's list entry arrives before its spawn — the
    // ordering obligation — and names it in the first feed; then, behind the
    // quiet stretch, the first player's entry, a display-name update for the
    // third, the second player's removal, and updates naming no entry all
    // arrive at once.
    let uuid = [0x11; 16];
    let other = [0x22; 16];
    let third = [0x33; 16];
    let (events, outgoing) = run_feed_session(
        feed_head(&[
            player_list_add_full_frame(other, "Other", None),
            spawn_player_frame(40, uuid, 0.0, 64.0, 0.0, 0, 0),
            spawn_player_frame(41, other, 1.0, 64.0, 0.0, 0, 0),
            spawn_player_frame(42, third, 2.0, 64.0, 0.0, 0, 0),
        ]),
        framed(&[
            player_list_add_full_frame(uuid, "OxideDev", None),
            player_list_add_full_frame(third, "Cee", None),
            player_list_display_frame(third, Some("§cCee")),
            player_list_remove_frame(other),
            player_list_gamemode_frame([0x44; 16], 1),
            keep_alive_frame(47),
        ]),
        6,
        6,
    );

    let all_feeds = feeds(&events);
    assert!(
        all_feeds.len() >= 2,
        "both quiet stretches tick: {events:?}"
    );
    let early = &all_feeds[0];
    assert_eq!(
        frame_of(early, 40).nametag,
        None,
        "a spawn before its entry has no name"
    );
    assert_eq!(
        frame_of(early, 41).nametag.as_deref(),
        Some("Other"),
        "the entry that arrived first names its player"
    );
    assert_eq!(frame_of(early, 42).nametag, None);
    let late = all_feeds.last().expect("a last feed");
    assert_eq!(frame_of(late, 40).nametag.as_deref(), Some("OxideDev"));
    assert_eq!(
        frame_of(late, 41).nametag,
        None,
        "the removal dropped the name"
    );
    assert_eq!(
        frame_of(late, 42).nametag.as_deref(),
        Some("§cCee"),
        "the display name is the composed text"
    );
    tail_with_echo(&outgoing, 47);
}

#[test]
fn a_rebuilt_dimension_clears_the_tracked_world() {
    // A zombie is tracked across a same-dimension respawn — the source's own
    // rule — and the overworld-to-nether respawn that follows rebuilds the
    // world: the feed clears with it and tracks what spawns after.
    let (events, outgoing) = run_feed_session(
        feed_head(&[
            spawn_mob_frame(50, 54, 8.0, 64.0, 8.0, 0, 0, 0),
            respawn_frame(0, 1, 0, "default"),
        ]),
        framed(&[
            respawn_frame(-1, 1, 2, "default"),
            spawn_mob_frame(51, 56, 9.0, 64.0, 9.0, 0, 0, 0),
            keep_alive_frame(48),
        ]),
        6,
        6,
    );

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ClientEvent::WorldCleared))
            .count(),
        1,
        "exactly the dimension change clears: {events:?}"
    );
    let all_feeds = feeds(&events);
    assert!(
        all_feeds.len() >= 2,
        "both quiet stretches tick: {events:?}"
    );
    let early = &all_feeds[0];
    assert!(
        early.iter().any(|frame| frame.id == 50),
        "the same-dimension respawn keeps the tracked world: {early:?}"
    );
    let late = all_feeds.last().expect("a last feed");
    assert!(
        !late.iter().any(|frame| frame.id == 50),
        "the rebuild cleared it: {late:?}"
    );
    assert_eq!(
        frame_of(late, 51).kind,
        EntityKind::Ghast,
        "and the store tracks again after the rebuild"
    );
    tail_with_echo(&outgoing, 48);
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
    // The echo is written once, among the walking reports the ticks sent while
    // the burst was read.
    tail_with_echo(&outgoing, 21);
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
    // blocks change — and the burst's tail is an entity flood, the spawn and
    // movement frames a busy server interleaves with the columns, so the
    // entity arms read at the same speed. Each applied column's snapshot copy
    // costs tens of milliseconds in a debug build: a loop that pumped between
    // packets would need far longer than the deadline below for these
    // hundreds of frames, and the diagnostic names how much of the burst was
    // meshed while the echo went unanswered. The harness's stream has no idle
    // wait, so the burst's meshes can only be reported after the stream ends:
    // an echo answered here was answered while the burst was still unmeshed.
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
    // The entity flood: one spawn then a movement frame per burst frame,
    // with metadata updates dripped in — all of it ahead of the keepalive.
    frame(
        &mut script,
        &spawn_mob_frame(90, 54, 0.0, 64.0, 0.0, 0, 0, 0),
        SERVER_FRAMING,
    );
    for move_index in 0..BURST {
        frame(
            &mut script,
            &relative_move_frame(90, 1, 0, 0),
            SERVER_FRAMING,
        );
        if move_index % 8 == 0 {
            frame(
                &mut script,
                &entity_metadata_frame(90, &[(13, 0, meta_byte(1))]),
                SERVER_FRAMING,
            );
        }
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

    // The echo is on the wire once — the burst drew it, plus whatever walking
    // reports the ticks sent during the burst — and nothing else.
    let frames = tail_with_echo(&outgoing, 97);
    assert!(
        frames.iter().any(|frame| *frame == [0x00, 0x61]),
        "the keepalive is answered"
    );
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
    // The keepalive echo is on the wire once, among the walking reports the
    // ticks sent around it; the bulk frame drew nothing else.
    tail_with_echo(&outgoing, 41);

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
    // The keepalive echo is on the wire once, among the walking reports the
    // ticks sent around it; the six columns drew nothing else.
    tail_with_echo(&outgoing, 61);
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

/// A duplex with scheduled input flips: it wraps a [`GappedDuplex`] and, after
/// the `after`-th idle wait, sends one input event on the session's channel.
///
/// A flip lands by idle-wait count because that is where the script can reach
/// the channel: the session drains its input at the top of a pass, so an event
/// queued before the run would be held from the first tick on, and only a
/// quiet stretch lets a key go down — or come back up — mid-session.
struct FlipDuplex {
    inner: GappedDuplex,
    waits: usize,
    flips: Vec<(usize, InputEvent)>,
    input_tx: crossbeam_channel::Sender<InputEvent>,
}

impl Read for FlipDuplex {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(out)
    }
}

impl Write for FlipDuplex {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.inner.write(data)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl DeadlineStream for FlipDuplex {
    fn wait_readable(&mut self, timeout: Duration) -> std::io::Result<bool> {
        let readable = self.inner.wait_readable(timeout)?;
        if !readable {
            self.waits += 1;
            for (after, event) in &self.flips {
                if *after == self.waits {
                    let _ = self.input_tx.send(*event);
                }
            }
        }
        Ok(readable)
    }
}

/// Reads the client's remaining frames and checks each is a walking report
/// (0x03–0x06): the only packet a live tick sends on its own.
fn tail_reports(cursor: &mut &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    while !cursor.is_empty() {
        let packet = client_frame(cursor, SERVER_FRAMING);
        assert!(
            matches!(packet[0], 0x03..=0x06),
            "only walking reports follow: {packet:?}"
        );
        frames.push(packet);
    }
    frames
}

/// Reads the client's frames after the preamble, asserts the keepalive echo is
/// among them exactly once and that every other frame is a walking report
/// (0x03–0x06) — the only packet a live tick sends on its own — and returns
/// the frames.
fn tail_with_echo(outgoing: &Arc<Mutex<Vec<u8>>>, id: i32) -> Vec<Vec<u8>> {
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    let echo = [0x00, u8::try_from(id).expect("a one-byte keepalive id")];
    let mut frames = Vec::new();
    let mut echoes = 0;
    while !cursor.is_empty() {
        let packet = client_frame(&mut cursor, SERVER_FRAMING);
        if packet == echo {
            echoes += 1;
        } else {
            assert!(
                matches!(packet[0], 0x03..=0x06),
                "only the keepalive echo and the walking reports follow the preamble: {packet:?}"
            );
        }
        frames.push(packet);
    }
    assert_eq!(echoes, 1, "the keepalive echo is on the wire: {frames:?}");
    frames
}

/// The frames the client wrote after the preamble — the handshake, Login
/// Start, Client Settings and the brand — as payloads.
fn client_payloads(outgoing: &Arc<Mutex<Vec<u8>>>) -> Vec<Vec<u8>> {
    let written = outgoing.lock().unwrap().clone();
    let mut cursor = &written[..];
    client_frame(&mut cursor, Compression::Disabled); // handshake
    client_frame(&mut cursor, Compression::Disabled); // login start
    client_frame(&mut cursor, SERVER_FRAMING); // client settings
    client_frame(&mut cursor, SERVER_FRAMING); // brand
    let mut frames = Vec::new();
    while !cursor.is_empty() {
        frames.push(client_frame(&mut cursor, SERVER_FRAMING));
    }
    frames
}

/// Runs one scripted session with flips scheduled at idle-wait counts, and
/// returns every event it reported and every frame it wrote after the
/// preamble.
fn flip_session(
    head: Vec<u8>,
    tail: Vec<u8>,
    stalls: usize,
    tail_stalls: usize,
    flips: Vec<(usize, InputEvent)>,
) -> (Vec<ClientEvent>, Vec<Vec<u8>>) {
    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    let stream = FlipDuplex {
        inner: GappedDuplex {
            head: std::io::Cursor::new(head),
            tail: std::io::Cursor::new(tail),
            stalls,
            tail_stalls,
            outgoing: Arc::clone(&outgoing),
        },
        waits: 0,
        flips,
        input_tx,
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
        .run_over(&sender, input_rx)
        .expect("the session runs to the end of the stream");
    (receiver.try_iter().collect(), client_payloads(&outgoing))
}

/// The unsnapped ticks a session reported, as (y, on_ground, flying).
fn tick_states(events: &[ClientEvent]) -> Vec<(f64, bool, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick {
                y,
                on_ground,
                flying,
                snapped: false,
                ..
            } => Some((*y, *on_ground, *flying)),
            _ => None,
        })
        .collect()
}

/// The index of the `n`-th frame whose id is `id`.
fn nth_frame(frames: &[Vec<u8>], id: u8, n: usize) -> usize {
    frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame[0] == id)
        .nth(n)
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("frame {id:#04x} #{n} is on the wire: {frames:?}"))
}

/// A join carrying the stone floor, then a teleport onto its surface at
/// (0.5, 64, 0.5) facing south.
fn floor_head() -> Vec<u8> {
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    head
}

/// The Player Abilities payload a scripted server sends: `flags` and the two
/// speeds.
fn abilities_frame(flags: u8, fly_speed: f32, walk_speed: f32) -> Vec<u8> {
    let mut abilities = vec![0x39, flags];
    abilities.extend_from_slice(&fly_speed.to_be_bytes());
    abilities.extend_from_slice(&walk_speed.to_be_bytes());
    abilities
}

/// The 0x13 payload the abilities `flags` and speeds produce.
fn abilities_echo(flags: u8, fly_speed: f32, walk_speed: f32) -> Vec<u8> {
    let mut echo = vec![0x13, flags];
    echo.extend_from_slice(&fly_speed.to_be_bytes());
    echo.extend_from_slice(&walk_speed.to_be_bytes());
    echo
}

#[test]
fn the_quiet_tick_reports_the_ground_only() {
    // A stationary player on the floor: every tick's walking report is the
    // ground byte alone (0x03) — neither position nor rotation moved, so
    // `EntityPlayerSP.onUpdateWalkingPlayer:225-247` sends the one packet that
    // carries nothing. The correction's echo (0x06) leads them.
    let (events, frames) = flip_session(floor_head(), Vec::new(), 16, 0, Vec::new());
    let echo = nth_frame(&frames, 0x06, 0);
    let reports = &frames[echo + 1..];
    assert!(
        reports.len() >= 3,
        "the quiet stretch owes ticks: {frames:?}"
    );
    // The form is the pin: the ground byte alone. The byte's value is the
    // physics's own (the first tick after a teleport has not probed the floor
    // yet), so only the last report is checked for the settled flag.
    assert!(
        reports
            .iter()
            .all(|frame| frame[0] == 0x03 && frame.len() == 2),
        "every tick reports the ground alone: {reports:?}"
    );
    assert_eq!(
        reports.last(),
        Some(&vec![0x03, 0x01]),
        "the ground settles onto the floor: {reports:?}"
    );
    let ticks = events
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { snapped: false, .. }))
        .count();
    assert!(
        reports.len() <= ticks,
        "one walking report per tick at most: {frames:?}"
    );
}

#[test]
fn a_look_change_reports_the_rotation_only() {
    // The mouse turns the player (10 px right at the fixed 0.5 sensitivity is
    // 1.5°) with no step of its own; the next tick's report is the rotation
    // alone (0x05) with the position untouched, and the ticks after it settle
    // back to the ground byte alone.
    let flips = vec![(2, InputEvent::MouseDelta { dx: 10.0, dy: 0.0 })];
    let (events, frames) = flip_session(floor_head(), Vec::new(), 16, 0, flips);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::PlayerTick { yaw, .. } if (*yaw - 1.5).abs() < 1e-4)),
        "the turn reached the tick: {events:?}"
    );
    let turn = nth_frame(&frames, 0x05, 0);
    let mut expected = vec![0x05];
    expected.extend_from_slice(&1.5f32.to_be_bytes());
    expected.extend_from_slice(&0.0f32.to_be_bytes());
    assert_eq!(
        frames[turn][..9],
        expected,
        "the rotation report, byte for byte"
    );
    assert_eq!(frames[turn].len(), 10, "and its ground byte");
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x06).count(),
        1,
        "only the correction's echo carries a position: {frames:?}"
    );
    assert!(
        frames.iter().all(|frame| frame[0] != 0x04),
        "nothing moved, so no position report: {frames:?}"
    );
    assert!(
        frames[turn + 1..]
            .iter()
            .all(|frame| frame == &[0x03, 0x01]),
        "the ticks after the turn report the ground alone: {frames:?}"
    );
}

#[test]
fn a_move_reports_the_position_only() {
    // The forward key walks the player south with no turn; the first tick past
    // the source's 9.0E-4 threshold reports the position alone (0x04), the
    // feet on the floor and the heading held, and no rotation report follows.
    let flips = vec![(
        2,
        InputEvent::Key {
            key: Key::W,
            pressed: true,
        },
    )];
    let (_events, frames) = flip_session(floor_head(), Vec::new(), 24, 0, flips);
    let echo = nth_frame(&frames, 0x06, 0);
    let first_move = nth_frame(&frames, 0x04, 0);
    assert!(
        first_move > echo,
        "the walk is reported after the correction: {frames:?}"
    );
    let moved = &frames[first_move];
    assert_eq!(moved[0], 0x04, "the position report's id");
    let x = f64::from_be_bytes(moved[1..9].try_into().expect("eight bytes"));
    let y = f64::from_be_bytes(moved[9..17].try_into().expect("eight bytes"));
    let z = f64::from_be_bytes(moved[17..25].try_into().expect("eight bytes"));
    assert_eq!(x, 0.5, "the heading holds: no x drift");
    assert!((y - 64.0).abs() < 1e-9, "the floor holds the height: {y}");
    assert!(z > 0.5 && z < 0.8, "the first reported step is small: {z}");
    assert_eq!(moved[25], 0x01, "the feet are on the ground");
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x06).count(),
        1,
        "only the correction's echo carries a rotation: {frames:?}"
    );
    assert!(
        frames.iter().all(|frame| frame[0] != 0x05),
        "nothing turned, so no rotation report: {frames:?}"
    );
}

#[test]
fn a_move_and_a_look_in_one_tick_report_both() {
    // A falling player whose mouse turns mid-fall: the tick that both moved
    // and looked sends 0x06, the position and the rotation in one packet.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 90.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let flips = vec![(8, InputEvent::MouseDelta { dx: 10.0, dy: 0.0 })];
    let (_events, frames) = flip_session(head, Vec::new(), 16, 0, flips);
    let both = nth_frame(&frames, 0x06, 1); // the echo is the first 0x06
    assert!(
        both > nth_frame(&frames, 0x06, 0),
        "the combined report follows the echo: {frames:?}"
    );
    let frame = &frames[both];
    let x = f64::from_be_bytes(frame[1..9].try_into().expect("eight bytes"));
    let y = f64::from_be_bytes(frame[9..17].try_into().expect("eight bytes"));
    let z = f64::from_be_bytes(frame[17..25].try_into().expect("eight bytes"));
    let yaw = f32::from_be_bytes(frame[25..29].try_into().expect("four bytes"));
    let pitch = f32::from_be_bytes(frame[29..33].try_into().expect("four bytes"));
    assert_eq!((x, z), (0.5, 0.5), "the fall keeps the column");
    assert!(y < 90.0 && y > 88.0, "the tick fell: {y}");
    assert!((yaw - 1.5).abs() < 1e-4, "the turn is in the packet: {yaw}");
    assert_eq!(pitch, 0.0, "no vertical mouse movement");
    assert_eq!(frame[33], 0x00, "the fall is airborne");
}

#[test]
fn the_stale_tick_reports_the_position_unchanged() {
    // A quiet player is still re-sent: the counter reaches the source's twenty
    // (`EntityPlayerSP.java:230`), and the twenty-first tick after the
    // correction echoes the unchanged position (0x04). The correction reset
    // the counter with its echo, so the count starts there.
    let (events, frames) = flip_session(floor_head(), Vec::new(), 70, 0, Vec::new());
    let echo = nth_frame(&frames, 0x06, 0);
    let reports = &frames[echo + 1..];
    assert!(
        reports.len() >= 22,
        "the stretch owes more than twenty-one ticks: {}",
        reports.len()
    );
    assert!(
        reports[..20]
            .iter()
            .all(|frame| frame[0] == 0x03 && frame.len() == 2),
        "the first twenty ticks report the ground alone: {:?}",
        &reports[..20]
    );
    let stale = &reports[20];
    assert_eq!(stale[0], 0x04, "the twenty-first tick reports the position");
    let x = f64::from_be_bytes(stale[1..9].try_into().expect("eight bytes"));
    let y = f64::from_be_bytes(stale[9..17].try_into().expect("eight bytes"));
    let z = f64::from_be_bytes(stale[17..25].try_into().expect("eight bytes"));
    assert_eq!(
        (x, y, z),
        (0.5, 64.0, 0.5),
        "the stale report carries the unchanged position"
    );
    assert!(
        reports[21..]
            .iter()
            .all(|frame| frame[0] == 0x03 && frame.len() == 2),
        "the counter restarts with the report: {:?}",
        &reports[21..]
    );
    let ticks = events
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { snapped: false, .. }))
        .count();
    assert!(
        reports.len() <= ticks,
        "one report per tick at most: {frames:?}"
    );
}

#[test]
fn the_sprint_and_sneak_edges_are_sent_once_per_change() {
    // The crouch goes down and comes back up, then the player walks and starts
    // sprinting with the control key and stops when the walk ends: each change
    // sends one Entity Action (0x0B) with the player's own entity id (20) and
    // the action's ordinal, and a stable state sends nothing.
    let flips = vec![
        (
            4,
            InputEvent::Key {
                key: Key::ShiftLeft,
                pressed: true,
            },
        ),
        (
            8,
            InputEvent::Key {
                key: Key::ShiftLeft,
                pressed: false,
            },
        ),
        (
            12,
            InputEvent::Key {
                key: Key::W,
                pressed: true,
            },
        ),
        (
            14,
            InputEvent::Key {
                key: Key::ControlLeft,
                pressed: true,
            },
        ),
        (
            20,
            InputEvent::Key {
                key: Key::W,
                pressed: false,
            },
        ),
    ];
    let (events, frames) = flip_session(floor_head(), Vec::new(), 30, 0, flips);
    let actions: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x0B).collect();
    assert_eq!(
        actions,
        vec![
            &vec![0x0B, 0x14, 0x00, 0x00], // Start Sneaking (0)
            &vec![0x0B, 0x14, 0x01, 0x00], // Stop Sneaking (1)
            &vec![0x0B, 0x14, 0x03, 0x00], // Start Sprinting (3)
            &vec![0x0B, 0x14, 0x04, 0x00], // Stop Sprinting (4)
        ],
        "one action per change, in order: {frames:?}"
    );
    assert!(
        frames
            .iter()
            .all(|frame| frame[0] == 0x0B || matches!(frame[0], 0x03..=0x06)),
        "only the actions and the walking report are sent: {frames:?}"
    );
    let sprints: Vec<bool> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick {
                sprinting,
                snapped: false,
                ..
            } => Some(*sprinting),
            _ => None,
        })
        .collect();
    assert!(
        sprints.contains(&true) && !*sprints.last().expect("ticks"),
        "the ticks show the sprint on and off: {sprints:?}"
    );
}

#[test]
fn a_lone_fresh_press_only_arms_the_flight_toggle() {
    // One fresh jump press in the air does not flip flight: it arms the
    // seven-tick window (`EntityPlayerSP.java:834-838`), so no abilities
    // packet goes out and the player keeps falling.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &abilities_frame(0x0C, 0.05, 0.1), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 90.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let flips = vec![
        (
            4,
            InputEvent::Key {
                key: Key::Space,
                pressed: true,
            },
        ),
        (
            8,
            InputEvent::Key {
                key: Key::Space,
                pressed: false,
            },
        ),
    ];
    let (events, frames) = flip_session(head, Vec::new(), 30, 0, flips);
    assert!(
        frames.iter().all(|frame| frame[0] != 0x13),
        "arming sends nothing: {frames:?}"
    );
    assert!(
        tick_states(&events).iter().all(|(_, _, flying)| !flying),
        "flight never flips: {:?}",
        tick_states(&events)
    );
}

#[test]
fn the_flight_toggle_flips_anywhere_and_lifts_the_flyer() {
    // Two fresh jump presses inside the window flip flight — anywhere, with no
    // ground under the player (`EntityPlayerSP.java:833-845`) — and the 0x13
    // goes out at once with the flags the server set (allow flying and
    // creative, so 0x0E with the flying bit). The movement model then flies:
    // with jump held the flyer stops falling and climbs.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &abilities_frame(0x0C, 0.05, 0.1), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 90.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let space = |pressed| InputEvent::Key {
        key: Key::Space,
        pressed,
    };
    let flips = vec![(4, space(true)), (8, space(false)), (12, space(true))];
    let (events, frames) = flip_session(head, Vec::new(), 40, 0, flips);
    let abilities: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x13).collect();
    assert_eq!(
        abilities,
        vec![&abilities_echo(0x0E, 0.05, 0.1)],
        "one abilities packet, with the server's flags and the flying bit: {frames:?}"
    );
    let states = tick_states(&events);
    let flipped = states
        .iter()
        .position(|(_, _, flying)| *flying)
        .expect("the toggle flips flight");
    assert!(
        states[..flipped].iter().all(|(_, on_ground, _)| !on_ground),
        "the toggle happens in the air: {states:?}"
    );
    assert!(
        states[flipped..].iter().all(|(_, _, flying)| *flying),
        "flight holds after the toggle: {states:?}"
    );
    assert!(
        states[flipped..]
            .windows(2)
            .any(|pair| pair[1].0 > pair[0].0),
        "the flyer climbs with jump held: {states:?}"
    );
}

#[test]
fn landing_cancels_flight() {
    // The player jumps (the first fresh press), toggles flight at the top of
    // the jump, and then descends with the sneak key: the landing cancels
    // flight (`EntityPlayerSP.java:904-908`) and the cancel's own 0x13 carries
    // the flying bit cleared.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &abilities_frame(0x0C, 0.05, 0.1), SERVER_FRAMING);
    frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let space = |pressed| InputEvent::Key {
        key: Key::Space,
        pressed,
    };
    let flips = vec![
        (3, space(true)),
        (6, space(false)),
        (9, space(true)),
        (12, space(false)),
        (
            15,
            InputEvent::Key {
                key: Key::ShiftLeft,
                pressed: true,
            },
        ),
    ];
    let (events, frames) = flip_session(head, Vec::new(), 60, 0, flips);
    let abilities: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x13).collect();
    assert_eq!(
        abilities,
        vec![
            &abilities_echo(0x0E, 0.05, 0.1),
            &abilities_echo(0x0C, 0.05, 0.1),
        ],
        "the toggle's packet, then the landing cancel's: {frames:?}"
    );
    let states = tick_states(&events);
    let flipped = states
        .iter()
        .position(|(_, _, flying)| *flying)
        .expect("the toggle flips flight");
    assert!(
        states[flipped..].iter().any(|(_, _, flying)| !flying),
        "the landing flips it back: {states:?}"
    );
    let landed = states
        .iter()
        .rposition(|(_, _, flying)| *flying)
        .expect("flight ran");
    assert!(
        states[landed + 1..]
            .iter()
            .all(|(_, on_ground, flying)| *on_ground && !flying),
        "the flyer lands and stays down: {states:?}"
    );
}

#[test]
fn the_servers_abilities_set_flight_without_an_echo() {
    // A 0x39 with the flying bit on: the handler takes the packet's own flying
    // value (`NetHandlerPlayClient.java:1674-1683`) and the movement model
    // flies at once — an airborne player holds its altitude instead of
    // falling. Applying abilities is not itself a change this client makes,
    // so no 0x13 is sent for the packet.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &abilities_frame(0x0F, 0.05, 0.1), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 90.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let (events, frames) = flip_session(head, Vec::new(), 16, 0, Vec::new());
    assert!(
        frames.iter().all(|frame| frame[0] != 0x13),
        "applying abilities sends nothing: {frames:?}"
    );
    let states = tick_states(&events);
    assert!(
        states.len() >= 3 && states.iter().all(|(_, _, flying)| *flying),
        "the server's flying bit holds: {states:?}"
    );
    assert!(
        states.iter().all(|(y, _, _)| (*y - 90.0).abs() < 1e-9),
        "the flyer holds its altitude instead of falling: {states:?}"
    );
    let echo = nth_frame(&frames, 0x06, 0);
    assert!(
        frames[echo + 1..]
            .iter()
            .all(|frame| frame == &[0x03, 0x00]),
        "the hovering flyer's reports carry the ground byte alone: {frames:?}"
    );
}

#[test]
fn a_correction_quiets_the_reporters_and_a_move_reports_the_position() {
    // The second correction (the tail's 0x08) echoes the corrected pose and
    // resets the walking reporters with it: the tick after the echo reports
    // nothing of the four blocks the correction moved the player, and the walk
    // that starts afterwards reports the small move (0x04) from the corrected
    // position.
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &position_frame(0.5, 64.0, 4.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    let flips = vec![(
        28, // four idle waits into the tail's own quiet stretch
        InputEvent::Key {
            key: Key::W,
            pressed: true,
        },
    )];
    let (_events, frames) = flip_session(floor_head(), tail, 24, 20, flips);
    let echo = nth_frame(&frames, 0x06, 1); // the tail correction's echo
    let mut expected = vec![0x06];
    expected.extend_from_slice(&0.5f64.to_be_bytes());
    expected.extend_from_slice(&64.0f64.to_be_bytes());
    expected.extend_from_slice(&4.5f64.to_be_bytes());
    expected.extend_from_slice(&0.0f32.to_be_bytes());
    expected.extend_from_slice(&0.0f32.to_be_bytes());
    expected.push(0x00);
    assert_eq!(frames[echo], expected, "the echo, byte for byte");
    let quiet = &frames[echo + 1..];
    let moved = quiet
        .iter()
        .position(|frame| frame[0] == 0x04)
        .expect("the walk is reported");
    assert!(
        quiet[..moved].iter().all(|frame| frame == &[0x03, 0x01]),
        "the ticks between the echo and the walk report the ground alone: {quiet:?}"
    );
    assert!(
        moved >= 1,
        "the tick after the echo reports nothing: {quiet:?}"
    );
    let z = f64::from_be_bytes(quiet[moved][17..25].try_into().expect("eight bytes"));
    assert!(
        z > 4.5 && z < 4.8,
        "the small move is reported from the corrected position: {z}"
    );
}

/// Join Game with the given gamemode byte: entity 20, the overworld, difficulty
/// 1, 20 players, the `default` level type.
fn join_game_frame_as(gamemode: u8) -> Vec<u8> {
    let mut join = vec![0x01];
    join.extend_from_slice(&20i32.to_be_bytes());
    join.extend_from_slice(&[gamemode, 0, 1, 20, 7]);
    join.extend_from_slice(b"default");
    join.push(0);
    join
}

/// A join carrying the stone floor, a teleport onto its surface at
/// (0.5, 64, 0.5) facing south, and a stone placed two cells south at
/// (0, 65, 2): the block the aim finds.
fn aim_head() -> Vec<u8> {
    let mut head = floor_head();
    frame(
        &mut head,
        &block_change_frame(0, 65, 2, STONE),
        SERVER_FRAMING,
    );
    head
}

#[test]
fn a_rotation_change_emits_a_new_aim_between_ticks() {
    // The player faces a stone block; a mouse turn to the west leaves every
    // block. The turn's aim is reported before the next tick — the look path
    // recomputes it — and it is the last aim reported: no tick follows it.
    let flip = InputEvent::MouseDelta { dx: 600.0, dy: 0.0 };
    let (events, _frames) = flip_session(aim_head(), Vec::new(), 12, 0, vec![(12, flip)]);

    let aims: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, ClientEvent::Aim { .. }))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(aims.len(), 2, "the aim appears and leaves: {events:?}");
    match &events[aims[0]] {
        ClientEvent::Aim { aim: Some(aim) } => {
            assert_eq!(
                (aim.x, aim.y, aim.z, aim.face),
                (0, 65, 2, Face::North),
                "the stone the player faces"
            );
            assert!(
                (aim.hit[0] - 0.5).abs() < 1e-9
                    && (aim.hit[1] - 65.62).abs() < 1e-9
                    && (aim.hit[2] - 2.0).abs() < 1e-9,
                "the hit point on the north face: {aim:?}"
            );
        }
        other => panic!("expected the stone the player faces, got {other:?}"),
    }
    let last = *aims.last().expect("an aim");
    assert!(
        matches!(&events[last], ClientEvent::Aim { aim: None }),
        "the turn left every block: {:?}",
        events[last]
    );
    assert!(
        events[..last]
            .iter()
            .any(|event| matches!(event, ClientEvent::PlayerTick { snapped: false, .. })),
        "a tick ran before the turn: {events:?}"
    );
    assert!(
        events[last + 1..]
            .iter()
            .all(|event| !matches!(event, ClientEvent::PlayerTick { .. })),
        "the turn was reported before any further tick: {events:?}"
    );
}

#[test]
fn a_tick_over_a_changed_world_re_emits_the_aim() {
    // The stone the player faces is removed by a block change; the first tick
    // that steps the changed world reports the new aim — `None` — right after
    // its own tick event.
    let head = aim_head();
    let mut tail = Vec::new();
    frame(&mut tail, &block_change_frame(0, 65, 2, 0), SERVER_FRAMING);
    let (events, _frames) = flip_session(head, tail, 12, 6, Vec::new());

    let first = events
        .iter()
        .position(|event| matches!(event, ClientEvent::Aim { aim: Some(_), .. }))
        .expect("the stone was aimed before the change");
    let last = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("the aim changed");
    assert!(last > first, "the change moved the aim: {events:?}");
    assert!(
        matches!(&events[last], ClientEvent::Aim { aim: None }),
        "the stone is gone: {:?}",
        events[last]
    );
    assert!(
        matches!(events[last - 1], ClientEvent::EntitiesTick { .. })
            && matches!(
                events[last - 2],
                ClientEvent::PlayerTick { snapped: false, .. }
            ),
        "the tick that stepped the changed world re-emitted the aim, its feed between: {:?}",
        &events[last - 2..=last]
    );
}

#[test]
fn the_aim_uses_the_gamemodes_reach() {
    // A stone whose north face is 4.55 from the eye: inside the creative 5.0
    // reach and beyond the survival 4.5 one. The same script aims it under a
    // creative Join Game and never aims it under a survival one.
    let script = |gamemode: u8| {
        let mut head = Vec::new();
        login_sequence(&mut head);
        frame(&mut head, &join_game_frame_as(gamemode), SERVER_FRAMING);
        frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
        frame(
            &mut head,
            &position_frame(0.5, 64.0, 0.45, 0.0, 0.0, 0),
            SERVER_FRAMING,
        );
        frame(
            &mut head,
            &block_change_frame(0, 65, 5, STONE),
            SERVER_FRAMING,
        );
        head
    };
    let (creative, _frames) = flip_session(script(1), Vec::new(), 12, 0, Vec::new());
    let aimed = creative
        .iter()
        .find_map(|event| match event {
            ClientEvent::Aim { aim: Some(aim) } => Some(*aim),
            _ => None,
        })
        .expect("the creative reach aims the stone");
    assert_eq!(
        (aimed.x, aimed.y, aimed.z, aimed.face),
        (0, 65, 5, Face::North),
        "the block 4.55 away"
    );

    let (survival, _frames) = flip_session(script(0), Vec::new(), 12, 0, Vec::new());
    assert!(
        survival
            .iter()
            .all(|event| !matches!(event, ClientEvent::Aim { aim: Some(_), .. })),
        "4.55 is beyond the survival reach: {survival:?}"
    );
}

/// Dirt, id 3, meta 0: the packed value `3 << 4 | 0`.
const DIRT: u16 = 0x0030;

/// Bedrock, id 7, meta 0: `setBlockUnbreakable`'s hardness -1.0
/// (`Block.java:1260`), the row the behaviour table carries.
const BEDROCK: u16 = 0x0070;

/// A join carrying the stone floor, a teleport onto its surface at
/// (0.5, 64, 0.5) facing south, and `value` placed two cells south at
/// (0, 65, 2) — the block a dig aims.
fn dig_head(value: u16) -> Vec<u8> {
    let mut head = floor_head();
    frame(
        &mut head,
        &block_change_frame(0, 65, 2, value),
        SERVER_FRAMING,
    );
    head
}

/// One Player Digging payload (0x07): the status, the Location Position and
/// the face byte.
fn digging_frame(status: u8, x: i32, y: i32, z: i32, face: u8) -> Vec<u8> {
    let mut payload = vec![0x07, status];
    let packed =
        ((x as i64 & 0x3FFFFFF) << 38) | ((y as i64 & 0xFFF) << 26) | (z as i64 & 0x3FFFFFF);
    payload.extend_from_slice(&packed.to_be_bytes());
    payload.push(face);
    payload
}

/// One Block Break Animation payload (0x25): the breaker's entity id, the
/// Location Position and the stage byte.
fn block_break_animation_frame(entity_id: i32, x: i32, y: i32, z: i32, stage: u8) -> Vec<u8> {
    let mut payload = vec![0x25];
    push_varint(&mut payload, entity_id);
    let packed =
        ((x as i64 & 0x3FFFFFF) << 38) | ((y as i64 & 0xFFF) << 26) | (z as i64 & 0x3FFFFFF);
    payload.extend_from_slice(&packed.to_be_bytes());
    payload.push(stage);
    payload
}

/// The destroy stages a session reported, in order: each position with the
/// stage a set landed, or `None` for a cleared entry.
fn reported_stages(events: &[ClientEvent]) -> Vec<((i32, i32, i32), Option<u8>)> {
    events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::BreakStage { x, y, z, stage } => Some(((*x, *y, *z), Some(*stage))),
            ClientEvent::BreakCleared { x, y, z } => Some(((*x, *y, *z), None)),
            _ => None,
        })
        .collect()
}

/// A left press as the scripted window sends it.
fn left_press() -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
    }
}

/// The walking reports (0x03–0x06) between two frames, exclusive.
fn reports_between(frames: &[Vec<u8>], from: usize, to: usize) -> usize {
    frames[from + 1..to]
        .iter()
        .filter(|frame| matches!(frame[0], 0x03..=0x06))
        .count()
}

#[test]
fn a_held_press_digs_dirt_at_the_hands_rate_and_removes_it_locally() {
    // The press starts the dig and the held ticks step the hand's dirt rate
    // (1/0.5/30 per tick, `Block.java:590-594`): fifteen damage ticks complete
    // it, so exactly fourteen walking reports — one per damage tick between —
    // separate the start from the finish. The completion sends the finish,
    // removes the block locally, and clears the stage; the server sent
    // nothing after the press, so the local path waited on no round trip.
    let (events, frames) = flip_session(dig_head(DIRT), Vec::new(), 4, 60, vec![(2, left_press())]);

    let digs: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x07).collect();
    assert_eq!(
        digs,
        vec![
            &digging_frame(0x00, 0, 65, 2, 2),
            &digging_frame(0x02, 0, 65, 2, 2),
        ],
        "one start and one finish, nothing else: {frames:?}"
    );
    let swings: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame[0] == 0x0a)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        swings.len(),
        2,
        "one swing per press and one at the completion: {frames:?}"
    );
    let start = nth_frame(&frames, 0x07, 0);
    let finish = nth_frame(&frames, 0x07, 1);
    assert!(
        swings[0] < start,
        "the press swings before it starts: {frames:?}"
    );
    assert!(
        finish < swings[1],
        "the completion's finish precedes its swing: {frames:?}"
    );
    assert_eq!(
        reports_between(&frames, start, finish),
        14,
        "fifteen damage ticks complete dirt: {frames:?}"
    );
    assert!(
        frames[swings[1] + 1..]
            .iter()
            .all(|frame| matches!(frame[0], 0x03..=0x06)),
        "nothing follows the completion's swing but walking reports: {frames:?}"
    );

    // The stage map stepped 0..8 once each — repeats are not re-reported —
    // and the completion's reset index cleared it.
    let expected: Vec<((i32, i32, i32), Option<u8>)> = (0..9)
        .map(|stage| ((0, 65, 2), Some(stage)))
        .chain([((0, 65, 2), None)])
        .collect();
    assert_eq!(
        reported_stages(&events),
        expected,
        "the stages step 0..8 and then clear: {events:?}"
    );
    // The removal was local and immediate: the clear lands before the next
    // tick, and the next recomputed aim passes through the hole.
    let cleared = events
        .iter()
        .position(|event| matches!(event, ClientEvent::BreakCleared { .. }))
        .expect("the dig cleared");
    let next_tick = events[cleared + 1..]
        .iter()
        .position(|event| matches!(event, ClientEvent::PlayerTick { .. }))
        .map(|offset| cleared + 1 + offset)
        .expect("ticks continue after the dig");
    assert!(
        events[cleared + 1..next_tick]
            .iter()
            .all(|event| !matches!(event, ClientEvent::Aim { .. })),
        "the clear lands in the completion's own pass: {events:?}"
    );
    let last_aim = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("an aim");
    assert!(
        matches!(&events[last_aim], ClientEvent::Aim { aim: None }),
        "the block is air: {:?}",
        events[last_aim]
    );
    assert!(
        events[cleared..]
            .iter()
            .any(|event| matches!(event, ClientEvent::ChunkUpdated { cx: 0, cz: 0, .. })),
        "the removal invalidated the column's meshes: {events:?}"
    );
}

#[test]
fn a_held_press_on_stone_digs_at_the_slow_rate() {
    // The same press on stone: the material refuses the hand, so the rate is
    // 1/1.5/100 (`Block.java:590-594`) — a fifteenth of dirt's — and no
    // completion lands in this window. The stage map steps its own pace:
    // stage k first lands at damage tick 15(k+1), so four steps prove the
    // slow rate, where dirt's would have finished at tick fifteen.
    let (events, frames) =
        flip_session(dig_head(STONE), Vec::new(), 4, 190, vec![(2, left_press())]);

    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x07).count(),
        1,
        "the start alone, no finish: {frames:?}"
    );
    assert_eq!(
        frames[nth_frame(&frames, 0x07, 0)],
        digging_frame(0x00, 0, 65, 2, 2),
        "the start"
    );
    let stages = reported_stages(&events);
    assert!(
        stages.len() >= 4,
        "the slow rate stepped at least 0..3: {events:?}"
    );
    assert_eq!(
        stages,
        (0..stages.len() as u8)
            .map(|stage| ((0, 65, 2), Some(stage)))
            .collect::<Vec<_>>(),
        "the stages ascend once each: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ClientEvent::BreakCleared { .. })),
        "nothing clears: {events:?}"
    );
}

#[test]
fn bedrock_never_completes_and_never_reports_a_stage() {
    // A negative hardness is the unbreakable zero (`Block.java:590-594`), so a
    // held press on bedrock sends the start and then nothing: no progress, no
    // stage to land (every tick's index is the -1 that clears an entry that
    // was never set), and no finish however long the button is held.
    let (events, frames) = flip_session(
        dig_head(BEDROCK),
        Vec::new(),
        4,
        40,
        vec![(2, left_press())],
    );
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x07).count(),
        1,
        "the start alone: {frames:?}"
    );
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x0a).count(),
        1,
        "the press's swing alone: {frames:?}"
    );
    assert!(
        reported_stages(&events).is_empty(),
        "no stage lands or clears: {events:?}"
    );
    let last_aim = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("an aim");
    assert!(
        matches!(
            &events[last_aim],
            ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 2)
        ),
        "the bedrock is still aimed: {:?}",
        events[last_aim]
    );
}

#[test]
fn a_creative_press_destroys_the_block_in_one_click() {
    // clickBlock's creative branch (`PlayerControllerMP.java:230-235`): the
    // start and the instant destroy, no running dig, no stage — and the block
    // is gone locally, so the next recomputed aim passes through it.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame_as(1), SERVER_FRAMING);
    frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &block_change_frame(0, 65, 2, STONE),
        SERVER_FRAMING,
    );
    let (events, frames) = flip_session(head, Vec::new(), 4, 30, vec![(2, left_press())]);

    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x07).count(),
        1,
        "the start alone, no finish: {frames:?}"
    );
    assert_eq!(
        frames[nth_frame(&frames, 0x07, 0)],
        digging_frame(0x00, 0, 65, 2, 2),
        "the start"
    );
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x0a).count(),
        1,
        "the press's swing alone: {frames:?}"
    );
    assert!(
        reported_stages(&events).is_empty(),
        "no stage in creative: {events:?}"
    );
    let last_aim = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("an aim");
    assert!(
        matches!(&events[last_aim], ClientEvent::Aim { aim: None }),
        "the block is air: {:?}",
        events[last_aim]
    );
}

#[test]
fn a_block_break_animation_sets_steps_and_clears_by_position() {
    // The decoded 0x25 lands on the stage map: 0..=9 sets, reporting only a
    // change, and anything else clears. The breaker's id is carried but not
    // filtered on — the map is keyed by position until M4's entity work — so
    // two breakers on one block share the one entry, and a repeat stage and a
    // repeat removal report nothing.
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &block_break_animation_frame(1, 0, 65, 2, 2),
        SERVER_FRAMING,
    );
    frame(
        &mut tail,
        &block_break_animation_frame(2, 0, 65, 2, 5),
        SERVER_FRAMING,
    );
    frame(
        &mut tail,
        &block_break_animation_frame(2, 0, 65, 2, 5),
        SERVER_FRAMING,
    );
    frame(
        &mut tail,
        &block_break_animation_frame(2, 0, 65, 2, 255),
        SERVER_FRAMING,
    );
    frame(
        &mut tail,
        &block_break_animation_frame(2, 0, 65, 2, 255),
        SERVER_FRAMING,
    );
    let (events, _frames) = flip_session(dig_head(STONE), tail, 6, 6, Vec::new());

    assert_eq!(
        reported_stages(&events),
        vec![
            ((0, 65, 2), Some(2)),
            ((0, 65, 2), Some(5)),
            ((0, 65, 2), None),
        ],
        "two setters, a repeat, a clear and a no-op: {events:?}"
    );
}

#[test]
fn the_servers_echo_of_a_predicted_removal_is_idempotent() {
    // The completion removes the block locally; the server's own 0x23 with
    // air for the same block follows on the tail, then a keepalive. The echo
    // writes air over air: no error, no stage, and the aim keeps passing
    // through — the prediction is already the server's answer.
    let mut tail = Vec::new();
    frame(&mut tail, &block_change_frame(0, 65, 2, 0), SERVER_FRAMING);
    frame(&mut tail, &keep_alive_frame(77), SERVER_FRAMING);
    let (events, frames) = flip_session(dig_head(DIRT), tail, 60, 8, vec![(2, left_press())]);

    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x07).count(),
        2,
        "a start and a finish: {frames:?}"
    );
    let cleared = events
        .iter()
        .position(|event| matches!(event, ClientEvent::BreakCleared { .. }))
        .expect("the dig cleared");
    let echo = events
        .iter()
        .position(|event| matches!(event, ClientEvent::KeepAlive { id: 77 }))
        .expect("the tail was read");
    assert!(
        cleared < echo,
        "the local removal preceded the echo: {events:?}"
    );
    let expected: Vec<((i32, i32, i32), Option<u8>)> = (0..9)
        .map(|stage| ((0, 65, 2), Some(stage)))
        .chain([((0, 65, 2), None)])
        .collect();
    assert_eq!(
        reported_stages(&events),
        expected,
        "the echo added no stage and cleared nothing twice: {events:?}"
    );
    assert!(
        events[echo..].iter().all(|event| !matches!(
            event,
            ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 2)
        )),
        "the block never reappears: {events:?}"
    );
}

#[test]
fn an_aim_change_aborts_the_running_block_and_clears_its_stage() {
    // A held press digs the block the player faces; a quarter turn east moves
    // the aim to a second block. The switch is the source's own
    // (`PlayerControllerMP.clickBlock:238-243`): the abort for the running
    // block goes out carrying the incoming face, the new dig starts, and the
    // breaker's damage entry — the old block's crack — is dropped as the new
    // dig starts (`sendBlockBreakProgress` with a negative progress, `:263`).
    // The release then aborts the new dig with the DOWN face (`:278`) and
    // drops its entry the same way (`:281`).
    let mut head = floor_head();
    frame(
        &mut head,
        &block_change_frame(0, 65, 2, DIRT),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &block_change_frame(2, 65, 0, DIRT),
        SERVER_FRAMING,
    );
    let flips = vec![
        (2, left_press()),
        (
            12,
            InputEvent::MouseDelta {
                dx: -600.0,
                dy: 0.0,
            },
        ),
        (
            16,
            InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: false,
            },
        ),
    ];
    let (events, frames) = flip_session(head, Vec::new(), 4, 48, flips);

    let digs: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x07).collect();
    assert_eq!(
        digs,
        vec![
            &digging_frame(0x00, 0, 65, 2, 2), // the press starts the south block
            &digging_frame(0x01, 0, 65, 2, 4), // the turn aborts it, with the incoming face
            &digging_frame(0x00, 2, 65, 0, 4), // and starts the east block
            &digging_frame(0x01, 2, 65, 0, 0), // the release aborts that one, with DOWN
        ],
        "the start, the switch's abort and start, and the release's abort: {frames:?}"
    );
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x0a).count(),
        1,
        "one swing, the press's own: {frames:?}"
    );

    // The first block's stage ladder ends in its clear: the abort dropped the
    // entry, and nothing lands on the block again.
    let stages = reported_stages(&events);
    let first: Vec<Option<u8>> = stages
        .iter()
        .filter(|(position, _)| *position == (0, 65, 2))
        .map(|(_, stage)| *stage)
        .collect();
    assert!(
        first.len() >= 2,
        "the dig stepped before the turn: {events:?}"
    );
    assert_eq!(
        first.last(),
        Some(&None),
        "the abort cleared the stage: {events:?}"
    );
    let stepped: Vec<u8> = first[..first.len() - 1]
        .iter()
        .map(|stage| stage.expect("a set"))
        .collect();
    assert_eq!(
        stepped,
        (0..stepped.len() as u8).collect::<Vec<_>>(),
        "the stages ascend once each before the clear: {events:?}"
    );
}

#[test]
fn a_block_break_animation_stage_expires_after_four_hundred_ticks() {
    // A decoded 0x25 lands stage 4 on a block and nothing refreshes it: the
    // sweep every twentieth tick removes an entry more than 400 ticks old
    // (`RenderGlobal.cleanupDamagedBlocks:1131`), so the clear lands 401 to
    // 420 ticks after the set — here the first sweep past 400, tick 420 —
    // and exactly once.
    let mut head = floor_head();
    frame(
        &mut head,
        &block_break_animation_frame(1, 0, 65, 2, 4),
        SERVER_FRAMING,
    );
    let (events, _frames) = flip_session(head, Vec::new(), 4, 1120, Vec::new());

    let stages = reported_stages(&events);
    assert_eq!(
        stages,
        vec![((0, 65, 2), Some(4)), ((0, 65, 2), None)],
        "one set, then the expiry's one clear: {events:?}"
    );
    let set = events
        .iter()
        .position(|event| matches!(event, ClientEvent::BreakStage { .. }))
        .expect("the set landed");
    let clear = events
        .iter()
        .position(|event| matches!(event, ClientEvent::BreakCleared { .. }))
        .expect("the expiry cleared it");
    let ticks = events[set + 1..=clear]
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { .. }))
        .count();
    assert!(
        (400..=420).contains(&ticks) && ticks > 400,
        "the expiry lands 401 to 420 ticks after the set, not {ticks}: {events:?}"
    );
}

/// A fence, id 85, meta 0: its post is 0.375..0.625 across and 1.5 tall
/// (`BlockFence.java:50-107`), and its material is not replaceable — the
/// block a placement cannot land in.
const FENCE: u16 = 85 << 4;

/// One Player Block Placement payload (0x08): the Location Position, the
/// face byte, the empty held item stack (the short -1, two `FF` bytes) and
/// the cursor's three bytes. Hand-packed, never through the writer's own
/// arithmetic.
fn placement_frame(x: i32, y: i32, z: i32, face: u8, cursor: [u8; 3]) -> Vec<u8> {
    let mut payload = vec![0x08];
    let packed =
        ((x as i64 & 0x3FFFFFF) << 38) | ((y as i64 & 0xFFF) << 26) | (z as i64 & 0x3FFFFFF);
    payload.extend_from_slice(&packed.to_be_bytes());
    payload.push(face);
    payload.extend_from_slice(&[0xFF, 0xFF]);
    payload.extend_from_slice(&cursor);
    payload
}

/// A right press as the scripted window sends it.
fn right_press() -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
    }
}

#[test]
fn a_right_press_places_the_block_and_predicts_it_locally() {
    // The press runs `rightClickMouse`'s path (`Minecraft.java:1570-1603`):
    // the checks pass on the stone the ray meets, so 0x08 goes out
    // (`PlayerControllerMP.java:424`) and the block lands locally at once
    // (`:436`, `:443`). Exactly one placement frame follows — no dig, no
    // swing — and the next recomputed aim reads the placed block the
    // prediction left in front of the original stone.
    let (events, frames) = flip_session(aim_head(), Vec::new(), 4, 60, vec![(2, right_press())]);

    let placements: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x08).collect();
    assert_eq!(
        placements,
        vec![&placement_frame(0, 65, 2, 2, [8, 9, 0])],
        "one placement frame, its bytes the aim's own: {frames:?}"
    );
    assert!(
        frames
            .iter()
            .all(|frame| frame[0] != 0x07 && frame[0] != 0x0a),
        "no dig and no swing follow a placement: {frames:?}"
    );

    // The prediction is readable: the aim moved to the cell the block landed
    // in — one north of the stone, met through the new block's north face —
    // and it stays there.
    let aims: Vec<Aim> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Aim { aim: Some(aim) } => Some(*aim),
            _ => None,
        })
        .collect();
    let placed_aim = aims
        .iter()
        .find(|aim| (aim.x, aim.y, aim.z) == (0, 65, 1))
        .expect("the placed block is aimed");
    assert_eq!(placed_aim.face, Face::North, "the new block's north face");
    let last = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("an aim");
    assert!(
        matches!(
            &events[last],
            ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 1)
        ),
        "the placed block stays the aim: {:?}",
        events[last]
    );
    // The local write invalidated the column's meshes like any block change.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::ChunkUpdated { cx: 0, cz: 0, .. })),
        "the prediction invalidated the column: {events:?}"
    );
}

#[test]
fn the_servers_echo_of_the_predicted_placement_is_idempotent() {
    // The prediction wrote stone at (0, 65, 1); the server's 0x23 with the
    // same stone follows on the tail, then a keepalive. The echo writes the
    // value over itself (`NetHandlerPlayClient.handleBlockChange`, `:776-780`,
    // applies whatever arrives): the aim never leaves the placed block, and
    // the session keeps reading — the keepalive is answered after it.
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &block_change_frame(0, 65, 1, STONE),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(78), SERVER_FRAMING);
    let (events, frames) = flip_session(aim_head(), tail, 60, 10, vec![(2, right_press())]);

    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x08).count(),
        1,
        "one placement frame: {frames:?}"
    );
    let placed = events
        .iter()
        .position(|event| {
            matches!(
                event,
                ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 1)
            )
        })
        .expect("the prediction is readable");
    let echo = events
        .iter()
        .position(|event| matches!(event, ClientEvent::KeepAlive { id: 78 }))
        .expect("the tail was read");
    assert!(placed < echo, "the placement preceded the echo: {events:?}");
    assert!(
        events[echo..].iter().all(|event| match event {
            ClientEvent::Aim { aim: Some(aim) } => (aim.x, aim.y, aim.z) == (0, 65, 1),
            ClientEvent::Aim { aim: None } => false,
            _ => true,
        }),
        "the echo changed nothing: {events:?}"
    );
}

#[test]
fn the_servers_correction_of_the_predicted_placement_replaces_it() {
    // The prediction wrote stone at (0, 65, 1); the server's own answer is
    // air — it refused the placement — so its 0x23 replaces the prediction
    // (`handleBlockChange` applies the server's value over whatever the
    // client holds, `:776-780`): the next recomputed aim passes through the
    // gap and reads the stone behind it again.
    let mut tail = Vec::new();
    frame(&mut tail, &block_change_frame(0, 65, 1, 0), SERVER_FRAMING);
    frame(&mut tail, &keep_alive_frame(79), SERVER_FRAMING);
    let (events, frames) = flip_session(aim_head(), tail, 60, 12, vec![(2, right_press())]);

    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x08).count(),
        1,
        "one placement frame: {frames:?}"
    );
    assert!(
        events
            .iter()
            .position(|event| matches!(event, ClientEvent::KeepAlive { id: 79 }))
            .is_some(),
        "the tail was read: {events:?}"
    );
    let last = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("an aim");
    assert!(
        matches!(
            &events[last],
            ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 2)
        ),
        "the prediction was replaced and the stone behind shows: {:?}",
        events[last]
    );
}

#[test]
fn a_stack_of_two_placements_lands_both() {
    // Two presses: the first at the stone the player faces — the block lands
    // at (0, 65, 1) — then a quarter turn east and a press at a second stone,
    // landing at (1, 65, 0). Both predictions are readable as they land, in
    // order, and each frame carries its own aim's bytes.
    let mut head = floor_head();
    frame(
        &mut head,
        &block_change_frame(0, 65, 2, STONE),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &block_change_frame(2, 65, 0, STONE),
        SERVER_FRAMING,
    );
    let flips = vec![
        (4, right_press()),
        (
            16,
            InputEvent::MouseDelta {
                dx: -600.0,
                dy: 0.0,
            },
        ),
        (28, right_press()),
    ];
    let (events, frames) = flip_session(head, Vec::new(), 32, 40, flips);

    let placements: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x08).collect();
    assert_eq!(
        placements,
        vec![
            &placement_frame(0, 65, 2, 2, [8, 9, 0]),
            &placement_frame(2, 65, 0, 4, [0, 9, 8]),
        ],
        "both placements, each with its own aim's bytes: {frames:?}"
    );
    let aims: Vec<(i32, i32, i32)> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Aim { aim: Some(aim) } => Some((aim.x, aim.y, aim.z)),
            _ => None,
        })
        .collect();
    let first = aims
        .iter()
        .position(|at| *at == (0, 65, 1))
        .expect("the first placed block is aimed");
    let second = aims
        .iter()
        .position(|at| *at == (1, 65, 0))
        .expect("the second placed block is aimed");
    assert!(first < second, "the placed blocks read in order: {aims:?}");
    assert_eq!(
        aims.last(),
        Some(&(1, 65, 0)),
        "the second block stays the aim: {aims:?}"
    );
}

#[test]
fn a_right_press_without_an_aim_places_nothing() {
    // The source's null-mouse-over branch (`Minecraft.java:1577-1581`): a
    // press with nothing aimed logs and does nothing. The floor alone leaves
    // the level ray aiming nothing, and no placement frame follows the press.
    let (events, frames) = flip_session(floor_head(), Vec::new(), 4, 20, vec![(2, right_press())]);
    assert!(
        frames.iter().all(|frame| frame[0] != 0x08),
        "no placement frame: {frames:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, ClientEvent::Aim { aim: Some(_) })),
        "nothing is aimed: {events:?}"
    );
}

#[test]
fn a_right_press_at_an_occupied_target_places_nothing() {
    // A fence stands on the stone's north neighbour: the aim finds the stone
    // through the gap beside the fence post, but the landing cell holds the
    // fence, whose material is not replaceable — the check refuses before the
    // packet (`PlayerControllerMP.java:417-421`) and nothing changes.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.1, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &block_change_frame(0, 65, 1, FENCE),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &block_change_frame(0, 65, 2, STONE),
        SERVER_FRAMING,
    );
    let (events, frames) = flip_session(head, Vec::new(), 6, 24, vec![(2, right_press())]);

    let aimed = events
        .iter()
        .find_map(|event| match event {
            ClientEvent::Aim { aim: Some(aim) } => Some(*aim),
            _ => None,
        })
        .expect("the stone through the gap is aimed");
    assert_eq!(
        (aimed.x, aimed.y, aimed.z, aimed.face),
        (0, 65, 2, Face::North),
        "the aim reaches past the fence post"
    );
    assert!(
        frames.iter().all(|frame| frame[0] != 0x08),
        "the refusal keeps the packet: {frames:?}"
    );
    let last = events
        .iter()
        .rposition(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("an aim");
    assert!(
        matches!(
            &events[last],
            ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 2)
        ),
        "the world is untouched: {:?}",
        events[last]
    );
}

#[test]
fn a_right_press_during_a_dig_places_nothing() {
    // `rightClickMouse`'s own guard refuses while the controller is hitting a
    // block (`Minecraft.java:1572`'s `getIsHittingBlock`): a right press that
    // lands mid-dig is dropped — the dig's own frames continue and no
    // placement follows.
    let (events, frames) = flip_session(
        dig_head(DIRT),
        Vec::new(),
        30,
        6,
        vec![(2, left_press()), (12, right_press())],
    );
    let digs: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x07).collect();
    assert!(
        !digs.is_empty() && digs.iter().all(|frame| frame[1] == 0x00),
        "the dig started and nothing finished inside the window: {frames:?}"
    );
    assert!(
        frames.iter().all(|frame| frame[0] != 0x08),
        "the guarded press sends no placement: {frames:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::BreakStage { .. })),
        "the dig did run: {events:?}"
    );
}

/// The player's own entity id, as `join_game_frame` names it.
const OWN_ENTITY_ID: i32 = 20;

/// The source's hurt status byte: `EntityLivingBase.handleStatusUpdate`'s
/// status 2.
const HURT_STATUS: u8 = 2;

/// One Update Health payload (0x06): the health f32, the food VarInt and the
/// saturation f32 (`S06PacketUpdateHealth.readPacketData:28-32`).
fn update_health_frame(health: f32, food: i32, saturation: f32) -> Vec<u8> {
    let mut payload = vec![0x06];
    payload.extend_from_slice(&health.to_be_bytes());
    push_varint(&mut payload, food);
    payload.extend_from_slice(&saturation.to_be_bytes());
    payload
}

/// One Respawn payload (0x07): the dimension i32, the difficulty and gamemode
/// bytes and the level type string (`S07PacketRespawn`).
fn respawn_frame(dimension: i32, difficulty: u8, gamemode: u8, level_type: &str) -> Vec<u8> {
    let mut payload = vec![0x07];
    payload.extend_from_slice(&dimension.to_be_bytes());
    payload.push(difficulty);
    payload.push(gamemode);
    push_string(&mut payload, level_type);
    payload
}

/// One Entity Status payload (0x1A): the entity id i32 and the status byte
/// (`S19PacketEntityStatus`).
fn entity_status_frame(entity_id: i32, status: u8) -> Vec<u8> {
    let mut payload = vec![0x1a];
    payload.extend_from_slice(&entity_id.to_be_bytes());
    payload.push(status);
    payload
}

/// The Client Status payload that asks the server to respawn the player:
/// action 0 (`C16PacketClientStatus.EnumState.PERFORM_RESPAWN`).
fn respawn_request_frame() -> Vec<u8> {
    vec![0x16, 0x00]
}

/// One fresh Space press and its release, for the death view's second key.
fn space_event(pressed: bool) -> InputEvent {
    InputEvent::Key {
        key: Key::Space,
        pressed,
    }
}

/// One fresh Shift press and its release, the sneak key.
fn shift_event(pressed: bool) -> InputEvent {
    InputEvent::Key {
        key: Key::ShiftLeft,
        pressed,
    }
}

/// The walking reports (0x03–0x06) in a frame list, in order.
fn walking_frames(frames: &[Vec<u8>]) -> Vec<&Vec<u8>> {
    frames
        .iter()
        .filter(|frame| matches!(frame[0], 0x03..=0x06))
        .collect()
}

#[test]
fn a_zero_health_death_cancels_the_dig_and_gates_the_window_input() {
    // Health at or below zero enters the death
    // (`EntityLivingBase.onEntityUpdate:344-349` reads the health off the
    // field every tick): the running dig is cancelled the way the death
    // screen cancels it (`Minecraft.java:1515-1518` runs
    // `sendClickBlockToController` with no left click), the state is reported
    // once, and from then on the window's input is the death view's: a click
    // or Space asks for the respawn, and a movement key or a look moves
    // nothing.
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &update_health_frame(0.0, 20, 5.0),
        SERVER_FRAMING,
    );
    let flips = vec![
        // Four waits in: the death follows eight waits later, so the
        // press-to-death window is at least 160 ms — three fifty-millisecond
        // tick boundaries at any phase, where two carry the dig's start and
        // its stage ladder — and late enough that the first ticks have found
        // the block.
        (4, left_press()),
        // All of these land after the death, during the tail's quiet stretch.
        (
            16,
            InputEvent::Key {
                key: Key::W,
                pressed: true,
            },
        ),
        (20, InputEvent::MouseDelta { dx: 40.0, dy: 0.0 }),
        (24, left_press()),
    ];
    let (events, frames) = flip_session(dig_head(DIRT), tail, 12, 60, flips);

    // The dig: the press's start, then the death's abort with the DOWN face
    // (`PlayerControllerMP.resetBlockRemoving:274-283`) and nothing else.
    let digs: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x07).collect();
    assert_eq!(
        digs,
        vec![
            &digging_frame(0x00, 0, 65, 2, 2),
            &digging_frame(0x01, 0, 65, 2, 0),
        ],
        "the start and the death's abort, and the click sends nothing: {frames:?}"
    );
    assert_eq!(
        frames.iter().filter(|frame| frame[0] == 0x0a).count(),
        1,
        "the press swung once; the click while dead does not swing: {frames:?}"
    );
    let requests: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x16).collect();
    assert_eq!(
        requests,
        vec![&respawn_request_frame()],
        "the click asks for the respawn once: {frames:?}"
    );

    // Movement and look are gated: the walk key and the mouse produce no new
    // pose — the walking reports the source keeps sending repeat the
    // teleport's own position whenever the twenty-tick stale rule fires
    // (`EntityPlayerSP.onUpdateWalkingPlayer:225-247`), so every report is
    // checked to carry exactly the teleported pose.
    let echo = &frames[nth_frame(&frames, 0x06, 0)];
    for frame in frames
        .iter()
        .filter(|frame| matches!(frame[0], 0x04..=0x06))
    {
        match frame[0] {
            0x04 => assert_eq!(
                &frame[1..25],
                &echo[1..25],
                "a position report repeats the teleport's position: {frame:?}"
            ),
            0x05 => assert_eq!(
                &frame[1..9],
                &echo[25..33],
                "a rotation report repeats the teleport's rotation: {frame:?}"
            ),
            _ => {
                assert_eq!(
                    &frame[1..25],
                    &echo[1..25],
                    "the combined report's position: {frame:?}"
                );
                assert_eq!(
                    &frame[25..33],
                    &echo[25..33],
                    "the combined report's rotation: {frame:?}"
                );
            }
        }
    }
    let died = events
        .iter()
        .position(|event| matches!(event, ClientEvent::Died))
        .expect("the death was reported");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ClientEvent::Died))
            .count(),
        1,
        "the death is entered once: {events:?}"
    );
    assert!(
        events[..died].iter().any(|event| matches!(
            event,
            ClientEvent::Health {
                health: 0.0,
                food: 20,
                ..
            }
        )),
        "the health landed before the death: {events:?}"
    );
    let after: Vec<&ClientEvent> = events[died + 1..]
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { .. }))
        .collect();
    assert!(after.len() >= 3, "the tick kept running: {events:?}");
    for event in after {
        if let ClientEvent::PlayerTick {
            x,
            y,
            z,
            yaw,
            pitch,
            ..
        } = event
        {
            assert_eq!(
                (*x, *y, *z, *yaw, *pitch),
                (0.5, 64.0, 0.5, 0.0, 0.0),
                "the walk key and the look moved nothing: {event:?}"
            );
        }
    }

    // The dig's stage ladder ended in its clear (`sendBlockBreakProgress`
    // with a negative progress drops the entry, `:281`).
    let stages = reported_stages(&events);
    let dug: Vec<Option<u8>> = stages
        .iter()
        .filter(|(position, _)| *position == (0, 65, 2))
        .map(|(_, stage)| *stage)
        .collect();
    assert!(dug.len() >= 2, "the dig stepped: {events:?}");
    assert_eq!(dug.last(), Some(&None), "the death cleared the stage");
}

#[test]
fn a_click_or_space_while_dead_asks_for_one_respawn_per_press() {
    // The death view's own input: one client-status request per press
    // (`GuiGameOver.actionPerformed:57-77` sends one from the button's click),
    // and a held button does not repeat — the request is the press, not the
    // held state. A later health above zero does not leave the death: only the
    // server's respawn does.
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &update_health_frame(0.0, 20, 5.0),
        SERVER_FRAMING,
    );
    frame(
        &mut tail,
        &update_health_frame(0.0, 20, 5.0),
        SERVER_FRAMING,
    );
    frame(
        &mut tail,
        &update_health_frame(20.0, 20, 5.0),
        SERVER_FRAMING,
    );
    let flips = vec![
        (6, left_press()),
        (
            8,
            InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: false,
            },
        ),
        (10, left_press()),
        (12, space_event(true)),
        (14, space_event(false)),
        (16, space_event(true)),
        (
            18,
            InputEvent::Key {
                key: Key::W,
                pressed: true,
            },
        ),
    ];
    let (events, frames) = flip_session(floor_head(), tail, 4, 60, flips);

    let requests = frames
        .iter()
        .filter(|frame| frame.as_slice() == respawn_request_frame())
        .count();
    assert_eq!(
        requests, 4,
        "two clicks and two Space presses ask four times, and nothing repeats: {frames:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ClientEvent::Died))
            .count(),
        1,
        "the repeated death frame does not re-enter the state: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ClientEvent::Health { .. }))
            .count(),
        3,
        "every Update Health is reported: {events:?}"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            ClientEvent::Respawned { .. } | ClientEvent::WorldCleared
        )),
        "nothing but the server's respawn leaves the death: {events:?}"
    );
    assert!(
        frames
            .iter()
            .all(|frame| frame[0] != 0x0a && frame[0] != 0x07),
        "the clicks while dead swing and dig nothing: {frames:?}"
    );
}

#[test]
fn a_tick_between_the_respawn_and_its_placement_sends_no_movement_packet() {
    // The respawn leaves the fresh player unplaced: the movement reports wait
    // for the 0x08 that carries the placement, so the ticks between the two
    // send nothing — while the session's own tick keeps running. The keepalive
    // the server follows the respawn with marks the wire, and every frame
    // after its echo is checked.
    let mut head = floor_head();
    frame(&mut head, &keep_alive_frame(7), SERVER_FRAMING);
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &respawn_frame(0, 1, 0, "default"),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(9), SERVER_FRAMING);
    let flips = vec![(
        6,
        InputEvent::Key {
            key: Key::W,
            pressed: true,
        },
    )];
    let (events, frames) = flip_session(head, tail, 4, 60, flips);

    let echo9 = nth_frame(&frames, 0x00, 1);
    assert_eq!(
        frames[echo9],
        vec![0x00, 0x09],
        "the second keepalive's echo marks the respawn's wire: {frames:?}"
    );
    assert!(
        walking_frames(&frames[echo9 + 1..]).is_empty(),
        "no movement packet follows the respawn before its placement: {frames:?}"
    );
    let respawned = events
        .iter()
        .position(|event| matches!(event, ClientEvent::Respawned { dimension: 0, .. }))
        .expect("the respawn was reported");
    let ticks_after = events[respawned + 1..]
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { .. }))
        .count();
    assert!(
        ticks_after >= 3,
        "the ticks kept running through the hold: {ticks_after}"
    );
}

#[test]
fn the_placements_echo_re_arms_the_reporters() {
    // The 0x08 ends the hold: its echo is the position report that reconciles
    // the placement, and the ticks after it report again — the frames between
    // the respawn and the echo carry nothing, and the frames after it carry
    // the walking reports.
    let mut head = floor_head();
    frame(&mut head, &keep_alive_frame(7), SERVER_FRAMING);
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &respawn_frame(0, 1, 0, "default"),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(9), SERVER_FRAMING);
    frame(
        &mut tail,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(11), SERVER_FRAMING);
    let (events, frames) = flip_session(head, tail, 4, 60, Vec::new());

    let echo9 = nth_frame(&frames, 0x00, 1);
    let echo11 = nth_frame(&frames, 0x00, 2);
    let placement_echo = nth_frame(&frames, 0x06, 1);
    assert!(
        echo9 < placement_echo && placement_echo < echo11,
        "the placement's echo sits between the two keepalives: {frames:?}"
    );
    assert!(
        frames[echo9 + 1..placement_echo].is_empty(),
        "the hold sends nothing between the respawn and the placement: {frames:?}"
    );
    assert!(
        !walking_frames(&frames[echo11 + 1..]).is_empty(),
        "the reporters re-arm after the placement's echo: {frames:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::Respawned { dimension: 0, .. })),
        "the respawn was reported: {events:?}"
    );
}

#[test]
fn a_same_dimension_respawn_keeps_the_world_and_restarts_the_player() {
    // The dimension matches the one the world was built for, so the world is
    // kept: the source's handler rebuilds only on a change
    // (`NetHandlerPlayClient.handleRespawn:1056-1073`), no chunk is dropped,
    // and the fresh player starts over inside it — the flags per the
    // abilities, the motion and the pitch zeroed
    // (`Entity.preparePlayerToSpawn:315-333`), the dig and the aim gone. A
    // held key re-asserts itself once the placement's echo re-arms the
    // reporters, because the respawn replaced the server's own player too.
    let mut head = dig_head(DIRT);
    // The server's abilities with the flying bit: the respawn's reset must
    // follow them, not clear flight blindly.
    frame(&mut head, &abilities_frame(0x0E, 0.05, 0.1), SERVER_FRAMING);
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &respawn_frame(0, 1, 0, "default"),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(9), SERVER_FRAMING);
    frame(
        &mut tail,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(11), SERVER_FRAMING);
    let flips = vec![(4, shift_event(true)), (18, left_press())];
    let (events, frames) = flip_session(head, tail, 30, 60, flips);

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::Respawned { dimension: 0, .. })),
        "the respawn was reported: {events:?}"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            ClientEvent::WorldCleared | ClientEvent::ChunkUnloaded { .. }
        )),
        "the world was kept: {events:?}"
    );
    // The dig is gone: the respawn aborted it with the DOWN face
    // (`PlayerControllerMP.resetBlockRemoving:274-283`), and the held button
    // starts a fresh dig on the block the recomputed aim found — the source's
    // `sendClickBlockToController` runs on the held button whether or not a
    // dig was underway, and the reset left none underway.
    let digs: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x07).collect();
    assert_eq!(
        digs,
        vec![
            &digging_frame(0x00, 0, 65, 2, 2),
            &digging_frame(0x01, 0, 65, 2, 0),
            &digging_frame(0x00, 0, 65, 2, 2),
            &digging_frame(0x02, 0, 65, 2, 2),
        ],
        "the respawn aborted the running dig, and the held button started over: {frames:?}"
    );
    let stages = reported_stages(&events);
    let dug: Vec<Option<u8>> = stages
        .iter()
        .filter(|(position, _)| *position == (0, 65, 2))
        .map(|(_, stage)| *stage)
        .collect();
    assert_eq!(dug.last(), Some(&None), "the stage map dropped the dig");
    // The aim was cleared at the respawn and recomputed against the kept
    // world: the clear goes out before the respawn reports itself (the window
    // learns the state changed, then that it is a new player), and the first
    // recompute after it names the same dirt block the kept world holds.
    let respawned = events
        .iter()
        .position(|event| matches!(event, ClientEvent::Respawned { .. }))
        .expect("the respawn was reported");
    assert!(
        matches!(
            events[..respawned]
                .iter()
                .rev()
                .find(|event| matches!(event, ClientEvent::Aim { .. })),
            Some(ClientEvent::Aim { aim: None })
        ),
        "the respawn cleared the aim: {respawning:?}",
        respawning = &events[respawned.saturating_sub(3)..=respawned]
    );
    let recomputed = events[respawned..]
        .iter()
        .find(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("the aim was recomputed against the kept world");
    assert!(
        matches!(
            recomputed,
            ClientEvent::Aim { aim: Some(aim) } if (aim.x, aim.y, aim.z) == (0, 65, 2)
        ),
        "the aim was recomputed against the kept world: {recomputed:?}"
    );
    // The held sneak re-asserts itself once the reporters re-arm: one Start
    // Sneaking before the respawn, one after the placement's echo.
    let starts = frames
        .iter()
        .filter(|frame| frame.as_slice() == [0x0B, 0x14, 0x00, 0x00])
        .count();
    assert_eq!(
        starts, 2,
        "the held key re-asserts after the respawn: {frames:?}"
    );
    let echo11 = nth_frame(&frames, 0x00, 1);
    let restarts: Vec<&Vec<u8>> = frames[echo11..]
        .iter()
        .filter(|frame| frame.as_slice() == [0x0B, 0x14, 0x00, 0x00])
        .collect();
    assert_eq!(
        restarts.len(),
        1,
        "the re-assertion is the only action after the respawn: {frames:?}"
    );
}

#[test]
fn a_respawn_into_another_dimension_rebuilds_the_world_and_clears_the_queue() {
    // The dimension changed: the source rebuilds the world
    // (`NetHandlerPlayClient.handleRespawn:1056-1073`), this client builds a
    // new one and empties the mesh queue in place — the column set belongs to
    // a world that is gone — and the window is told its chunk store is stale
    // before the respawn is reported. The fresh columns then replace the old
    // world's meshes, and the new world reads them without sky light because
    // the dimension has no sky.
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &respawn_frame(-1, 1, 2, "default"),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(9), SERVER_FRAMING);
    frame(&mut tail, &sky_less_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut tail,
        &position_frame(0.5, 64.0, 0.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(&mut tail, &keep_alive_frame(11), SERVER_FRAMING);
    let (events, frames) = flip_session(floor_head(), tail, 4, 60, Vec::new());

    let cleared = events
        .iter()
        .position(|event| matches!(event, ClientEvent::WorldCleared))
        .expect("the world clear was reported");
    let respawned = events
        .iter()
        .position(|event| matches!(event, ClientEvent::Respawned { dimension: -1, .. }))
        .expect("the respawn was reported");
    assert!(
        cleared < respawned,
        "the world clear precedes the respawn: {events:?}"
    );
    assert!(
        events[respawned..].iter().any(|event| matches!(
            event,
            ClientEvent::Respawned {
                gamemode: 2,
                dimension: -1
            }
        )),
        "the respawn carries the packet's dimension and gamemode: {events:?}"
    );
    assert!(
        events[cleared..]
            .iter()
            .any(|event| matches!(event, ClientEvent::ChunkUpdated { cx: 0, cz: 0, .. })),
        "the fresh world's column replaced the old meshes: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ClientEvent::ChunkUnloaded { .. })),
        "the rebuild does not unload columns; the clear is the window's cue: {events:?}"
    );
    let echo11 = nth_frame(&frames, 0x00, 1);
    assert!(
        !walking_frames(&frames[echo11 + 1..]).is_empty(),
        "the reporters re-arm in the new world: {frames:?}"
    );
}

#[test]
fn an_entity_status_for_the_player_fills_the_hurt_flash_and_the_ticks_count_it_down() {
    // Status 2 fills the hurt flash to its maximum
    // (`EntityLivingBase.handleStatusUpdate:1362-1363`, `hurtTime = maxHurtTime
    // = 10`), and the ticks count it down one per tick
    // (`onEntityUpdate:337-340`). A status for another entity is not the
    // player's and fills nothing: a second fill would make the flash jump back
    // up, and the sequence never does.
    let mut head = floor_head();
    frame(
        &mut head,
        &entity_status_frame(21, HURT_STATUS),
        SERVER_FRAMING,
    );
    let mut tail = Vec::new();
    frame(
        &mut tail,
        &entity_status_frame(OWN_ENTITY_ID, HURT_STATUS),
        SERVER_FRAMING,
    );
    let (events, _frames) = flip_session(head, tail, 8, 60, Vec::new());
    let flashes: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerTick {
                hurt_time,
                snapped: false,
                ..
            } => Some(*hurt_time),
            _ => None,
        })
        .collect();
    let filled = flashes
        .iter()
        .position(|flash| *flash > 0)
        .expect("the status filled the flash");
    // The step counts the flash down before it reports, so the first value the
    // events carry is one below the maximum the status set.
    assert_eq!(
        flashes[filled],
        MAX_HURT_TIME - 1,
        "the flash starts at `maxHurtTime` (`EntityLivingBase:336-340`): {flashes:?}"
    );
    assert!(
        flashes[..filled].iter().all(|flash| *flash == 0),
        "nothing before the status filled it: {flashes:?}"
    );
    assert!(
        flashes[filled..].windows(2).all(|pair| pair[1] <= pair[0]),
        "the flash only counts down: {flashes:?}"
    );
    assert_eq!(
        flashes.last(),
        Some(&0),
        "the flash reaches zero: {flashes:?}"
    );
}

/// One Change Game State payload (0x2B): the reason byte and the value float,
/// hand-packed as the source reads them
/// (`S2BPacketChangeGameState.readPacketData:27-31`: an unsigned byte then a
/// big-endian float), never through a writer's own arithmetic.
fn change_game_state_frame(reason: u8, value: f32) -> Vec<u8> {
    let mut payload = vec![0x2B, reason];
    payload.extend_from_slice(&value.to_be_bytes());
    payload
}

/// A join under `gamemode` carrying the stone floor, a teleport onto its
/// surface at (0.5, 64.0, 0.389) facing south, and a stone at (0, 65, 5)
/// whose north face sits 4.611 from the pose eye: inside creative's 5.0 reach
/// and outside survival's 4.5 — the step-3 scene's own edge distance.
fn reach_edge_head(gamemode: u8) -> Vec<u8> {
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame_as(gamemode), SERVER_FRAMING);
    frame(&mut head, &floor_column_frame(0, 0), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 64.0, 0.389, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &block_change_frame(0, 65, 5, STONE),
        SERVER_FRAMING,
    );
    head
}

#[test]
fn a_mid_session_game_mode_change_arms_the_reach_for_a_placement() {
    // The join carries survival, so the 4.611 stone is out of reach while the
    // session ticks. The server's 0x2B reason 3 value 1.0 then restates the
    // mode (`NetHandlerPlayClient.handleChangeGameState:1360-1383`), and the
    // next aim recompute reads creative's 5.0
    // (`PlayerControllerMP.java:344-346`): the stone is aimed and the right
    // press sends exactly one placement naming it.
    let mut tail = Vec::new();
    frame(&mut tail, &change_game_state_frame(3, 1.0), SERVER_FRAMING);
    let (events, frames) = flip_session(reach_edge_head(0), tail, 6, 48, vec![(8, right_press())]);

    let placements: Vec<&Vec<u8>> = frames.iter().filter(|frame| frame[0] == 0x08).collect();
    assert_eq!(
        placements,
        vec![&placement_frame(0, 65, 5, 2, [8, 9, 0])],
        "one placement frame, its bytes the aim's own: {frames:?}"
    );

    // The reach is visible in the aim: the first aim the session reports is
    // the stone only creative's reach can touch.
    let first = events
        .iter()
        .position(|event| matches!(event, ClientEvent::Aim { .. }))
        .expect("the mode change armed the aim");
    match &events[first] {
        ClientEvent::Aim { aim: Some(aim) } => assert_eq!(
            (aim.x, aim.y, aim.z, aim.face),
            (0, 65, 5, Face::North),
            "the stone 4.611 away, met through its north face"
        ),
        other => panic!("expected the stone the mode change armed, got {other:?}"),
    }
}

#[test]
fn a_zero_game_mode_value_leaves_the_reach_at_survival() {
    // The same scene and press with value 0.0: `getByID(0)` is survival, the
    // mode byte stays 0, the 4.611 stone is never aimed, and the press sends
    // no placement.
    let mut tail = Vec::new();
    frame(&mut tail, &change_game_state_frame(3, 0.0), SERVER_FRAMING);
    let (events, frames) = flip_session(reach_edge_head(0), tail, 6, 48, vec![(8, right_press())]);

    assert!(
        frames.iter().all(|frame| frame[0] != 0x08),
        "no placement leaves for a target out of reach: {frames:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, ClientEvent::Aim { aim: Some(_), .. })),
        "the survival reach never arms the 4.611 stone: {events:?}"
    );
}

#[test]
fn an_inert_change_game_state_reason_leaves_the_game_mode_alone() {
    // Reason 7 (the fade value) with value 1.0 — the weather-side reasons the
    // acceptance's own log carried per tick — decodes but changes nothing:
    // only reason 3 touches the mode, so the reach stays survival and no
    // placement leaves.
    let mut tail = Vec::new();
    frame(&mut tail, &change_game_state_frame(7, 1.0), SERVER_FRAMING);
    let (events, frames) = flip_session(reach_edge_head(0), tail, 6, 48, vec![(8, right_press())]);

    assert!(
        frames.iter().all(|frame| frame[0] != 0x08),
        "an inert reason arms nothing: {frames:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, ClientEvent::Aim { aim: Some(_), .. })),
        "the mode is unchanged, so the 4.611 stone stays out of reach: {events:?}"
    );
}

// -------------------------------------------------------------------------
// The scoreboard, the team clauses and the tab text.
// -------------------------------------------------------------------------

/// The boards the Scoreboard Changed events carry, in order.
fn boards(events: &[ClientEvent]) -> Vec<&Scoreboard> {
    events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::ScoreboardChanged { board } => Some(board),
            _ => None,
        })
        .collect()
}

/// The entry sets the Player List events carry, in order.
fn player_lists(events: &[ClientEvent]) -> Vec<&Vec<PlayerListRecord>> {
    events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::PlayerList { entries } => Some(entries),
            _ => None,
        })
        .collect()
}

/// One team row of the fixtures' own fields, for an expected board.
fn team_row(
    display_name: &str,
    prefix: &str,
    suffix: &str,
    friendly_flags: u8,
    name_tag_visibility: &str,
    colour: Option<u8>,
    players: &[&str],
) -> Team {
    Team {
        display_name: display_name.to_owned(),
        prefix: prefix.to_owned(),
        suffix: suffix.to_owned(),
        friendly_flags,
        name_tag_visibility: name_tag_visibility.to_owned(),
        colour,
        players: players.iter().map(|player| (*player).to_owned()).collect(),
    }
}

#[test]
fn the_scoreboard_reports_each_change_once() {
    // A scripted 0x3B/0x3C/0x3D/0x3E sequence: every packet that changes the
    // board reports the whole state, and a packet that writes what the board
    // already holds reports nothing — the tests count events.
    let (events, _) = run_feed_session(
        feed_head(&[
            scoreboard_objective_frame("kills", 0, "Kills", "integer"),
            // A second create with the same fields: the board does not move.
            scoreboard_objective_frame("kills", 0, "Kills", "integer"),
            scoreboard_display_frame(1, "kills"),
            scoreboard_display_frame(1, "kills"),
            scoreboard_score_frame("Alpha", 0, "kills", 5),
            scoreboard_score_frame("Alpha", 0, "kills", 5),
            scoreboard_team_info_frame(
                "red",
                0,
                "Red",
                "§c[Red] ",
                "§r",
                1,
                "always",
                0x0c,
                &["Alpha"],
            ),
            // The add-players mode re-adds the member the create carried.
            scoreboard_team_players_frame("red", 3, &["Alpha"]),
            scoreboard_team_players_frame("red", 4, &["Alpha"]),
            scoreboard_team_remove_frame("red"),
            // The remove mode carries neither the value nor the kind.
            scoreboard_objective_frame("kills", 1, "", ""),
        ]),
        framed(&[keep_alive_frame(21)]),
        0,
        0,
    );

    let reported = boards(&events);
    assert_eq!(
        reported.len(),
        7,
        "seven changes and four no-ops: {events:?}"
    );
    let mut expected = Scoreboard::new();
    expected.objectives.insert(
        "kills".to_owned(),
        Objective {
            name: "kills".to_owned(),
            value: "Kills".to_owned(),
            kind: "integer".to_owned(),
        },
    );
    assert_eq!(*reported[0], expected, "the create's board");
    expected.display[1] = Some("kills".to_owned());
    assert_eq!(*reported[1], expected, "the display slot's board");
    expected.scores.insert(
        "Alpha".to_owned(),
        BTreeMap::from([("kills".to_owned(), 5)]),
    );
    assert_eq!(*reported[2], expected, "the score's board");
    expected.teams.insert(
        "red".to_owned(),
        team_row("Red", "§c[Red] ", "§r", 1, "always", Some(0x0c), &["Alpha"]),
    );
    expected
        .member_of
        .insert("Alpha".to_owned(), "red".to_owned());
    assert_eq!(
        *reported[3], expected,
        "the create's board: the info block and the membership"
    );
    expected
        .teams
        .get_mut("red")
        .expect("the team is held")
        .players
        .clear();
    expected.member_of.clear();
    assert_eq!(
        *reported[4], expected,
        "the remove-players board: the membership alone left"
    );
    expected.teams.clear();
    assert_eq!(
        *reported[5], expected,
        "the team's removal: the rows fell, the rest held"
    );
    assert_eq!(
        *reported[6],
        Scoreboard::new(),
        "the objective's remove dropped it, its slot and its scores"
    );
}

#[test]
fn the_tab_text_reports_each_change_once() {
    let (events, _) = run_feed_session(
        feed_head(&[
            tab_header_footer_frame("{\"text\":\"one\"}", "{\"text\":\"two\"}"),
            // The same text again: no event.
            tab_header_footer_frame("{\"text\":\"one\"}", "{\"text\":\"two\"}"),
            tab_header_footer_frame("{\"text\":\"one\"}", "{\"text\":\"three\"}"),
        ]),
        framed(&[keep_alive_frame(22)]),
        0,
        0,
    );
    let texts: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::TabText { header, footer } => Some((header.as_str(), footer.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            ("{\"text\":\"one\"}", "{\"text\":\"two\"}"),
            ("{\"text\":\"one\"}", "{\"text\":\"three\"}"),
        ],
        "the two changes, the raw JSON strings as sent: {events:?}"
    );
}

#[test]
fn the_player_list_reports_the_whole_set_once_per_change() {
    let alpha = [0x11; 16];
    let beta = [0x02; 16];
    let (events, _) = run_feed_session(
        feed_head(&[
            player_list_add_full_frame(alpha, "Alpha", Some("§bAlpha")),
            // The add carried ping 42: the same ping again is no change.
            player_list_ping_frame(alpha, 42),
            player_list_ping_frame(alpha, 42),
            player_list_ping_frame(alpha, 60),
            player_list_add_full_frame(beta, "Beta", None),
            player_list_gamemode_frame(beta, 2),
            player_list_remove_frame(alpha),
        ]),
        framed(&[keep_alive_frame(23)]),
        0,
        0,
    );

    let sets = player_lists(&events);
    assert_eq!(sets.len(), 5, "five changes and two no-ops: {events:?}");
    assert_eq!(
        sets[0][0],
        PlayerListRecord {
            uuid: "11111111-1111-1111-1111-111111111111".to_owned(),
            name: "Alpha".to_owned(),
            properties: vec![("textures".to_owned(), "eyJx".to_owned())],
            gamemode: 1,
            latency: 42,
            display_name: Some("§bAlpha".to_owned()),
        },
        "the add's record as the wire carried it"
    );
    assert_eq!(
        sets[1][0].latency, 60,
        "the latency update reaches the record"
    );
    assert_eq!(
        sets[2]
            .iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Beta", "Alpha"],
        "the full set, ascending uuid"
    );
    assert_eq!(sets[3][0].gamemode, 2, "the gamemode update");
    assert_eq!(
        sets[4]
            .iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Beta"],
        "the remove dropped Alpha"
    );
}

#[test]
fn a_player_on_a_team_carries_the_team_clauses_in_the_feed() {
    // The composition is `prefix + text + suffix`
    // (`ScorePlayerTeam.formatString:95-98`), reached from the feed's nametag
    // (`EntityPlayer.getDisplayName:2316-2324`) and from the list entry
    // (`GuiPlayerTabOverlay.getPlayerName:45-51`); the colour byte is not
    // part of the text — the second team carries the colour with empty
    // clauses and adds nothing.
    let alpha = [0x03; 16];
    let beta = [0x04; 16];
    let (events, _) = run_feed_session(
        feed_head(&[
            player_list_add_frame(alpha, "Alpha"),
            player_list_add_frame(beta, "Beta"),
            scoreboard_team_info_frame(
                "red",
                0,
                "Red",
                "§c[Red] ",
                "§r",
                1,
                "always",
                0x0c,
                &["Alpha"],
            ),
            scoreboard_team_info_frame("plain", 0, "Plain", "", "", 0, "always", 0x0c, &["Beta"]),
            spawn_player_frame(30, alpha, 0.5, 64.0, 0.5, 0, 0),
            spawn_player_frame(31, beta, 1.5, 64.0, 0.5, 0, 0),
        ]),
        framed(&[keep_alive_frame(24)]),
        6,
        6,
    );

    let all_feeds = feeds(&events);
    let feed = all_feeds.last().expect("the quiet stretch ticks");
    assert_eq!(
        frame_of(feed, 30).nametag.as_deref(),
        Some("§c[Red] Alpha§r"),
        "the team's clauses wrap the nametag"
    );
    assert_eq!(
        frame_of(feed, 31).nametag.as_deref(),
        Some("Beta"),
        "a coloured team with empty clauses adds nothing: the colour is not the text"
    );
    // The same composition for the list entry, and the colour for the paths
    // that colour from it.
    let board = boards(&events)
        .last()
        .copied()
        .expect("the team create reports the board");
    let record = player_lists(&events)
        .last()
        .and_then(|set| set.iter().find(|record| record.name == "Alpha"))
        .expect("Alpha's entry");
    assert_eq!(
        display_name(Some(record), board).as_deref(),
        Some("§c[Red] Alpha§r"),
        "the list entry composes with the same clauses"
    );
    assert_eq!(entry_colour(board, "Alpha"), Some(0x0c));
    assert_eq!(entry_colour(board, "Beta"), Some(0x0c));
    assert_eq!(entry_colour(board, "Nobody"), None);
}

#[test]
fn a_re_sent_join_game_starts_the_scoreboard_and_the_list_over() {
    // A re-sent Join Game builds a fresh world, and the world's scoreboard
    // is fresh with it (`handleJoinGame:281` makes a new world; a dimension
    // change is the case that carries the old board over,
    // `handleRespawn:1065`).
    let alpha = [0x05; 16];
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(
        &mut head,
        &player_list_add_frame(alpha, "Alpha"),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &scoreboard_objective_frame("kills", 0, "Kills", "integer"),
        SERVER_FRAMING,
    );
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);

    let (events, _) = run_feed_session(head, Vec::new(), 0, 0);
    let reported = boards(&events);
    assert_eq!(
        reported.len(),
        2,
        "the create and then the emptied board: {events:?}"
    );
    assert_eq!(
        reported[0].objectives.len(),
        1,
        "the first board holds the objective"
    );
    assert_eq!(
        *reported[1],
        Scoreboard::new(),
        "the rebuilt world starts the board over"
    );
    let sets = player_lists(&events);
    assert_eq!(
        sets.len(),
        2,
        "the add and then the emptied set: {events:?}"
    );
    assert_eq!(sets[0][0].name, "Alpha");
    assert!(sets[1].is_empty(), "the rebuilt world names no players");
}

#[test]
fn a_dimension_respawn_keeps_the_scoreboard_and_reports_the_list_cleared() {
    // `handleRespawn:1058-1065` hands the old scoreboard to the new world, so
    // a dimension change keeps it whole — objectives, scores and teams — and
    // reports no board; the session's rebuilt world still starts its player
    // list over.
    let alpha = [0x06; 16];
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(
        &mut head,
        &player_list_add_frame(alpha, "Alpha"),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &scoreboard_objective_frame("kills", 0, "Kills", "integer"),
        SERVER_FRAMING,
    );
    frame(
        &mut head,
        &respawn_frame(-1, 1, 2, "default"),
        SERVER_FRAMING,
    );
    // One more objective after the respawn: the reported board still holds
    // the first, so the respawn kept it.
    frame(
        &mut head,
        &scoreboard_objective_frame("deaths", 0, "Deaths", "integer"),
        SERVER_FRAMING,
    );

    let (events, _) = run_feed_session(head, Vec::new(), 0, 0);
    let reported = boards(&events);
    assert_eq!(
        reported.len(),
        2,
        "the two creates report; the respawn reports no board: {events:?}"
    );
    assert_eq!(reported[0].objectives.len(), 1, "the first board");
    assert!(
        reported[1].objectives.contains_key("kills"),
        "the pre-respawn objective survived: {:?}",
        reported[1].objectives
    );
    assert!(reported[1].objectives.contains_key("deaths"));
    let sets = player_lists(&events);
    assert_eq!(sets.len(), 2, "the add, then the cleared set: {events:?}");
    assert_eq!(sets[0][0].name, "Alpha");
    assert!(sets[1].is_empty());
    // The cleared set sits with the world's rebuild: behind the clear and
    // before the World Cleared report of the same block.
    let set_positions: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, ClientEvent::PlayerList { .. }))
        .map(|(index, _)| index)
        .collect();
    let cleared = events
        .iter()
        .position(|event| matches!(event, ClientEvent::WorldCleared))
        .expect("the respawn rebuilt the world");
    assert!(
        set_positions[1] < cleared,
        "the clear precedes the world's own report: {events:?}"
    );
}

#[test]
fn a_time_update_of_the_smallest_value_freezes_the_clock_without_panicking() {
    // The receive rule's negation is a two's-complement wrap on every `i64`
    // (`S03PacketTimeUpdate.java:17-31`, `WorldClient.setWorldTime`,
    // `WorldClient.java:468-483`): the smallest value negates to itself —
    // still negative, so the frozen gate holds — and the sky the clock
    // renders from it is computed without the read loop faulting.
    let mut head = Vec::new();
    login_sequence(&mut head);
    frame(&mut head, &join_game_frame(), SERVER_FRAMING);
    frame(
        &mut head,
        &position_frame(0.5, 65.0, 4.5, 0.0, 0.0, 0),
        SERVER_FRAMING,
    );
    frame(&mut head, &chunk_data_frame(0, 0), SERVER_FRAMING);
    // `i64::MIN` on the wire: the negation's fixed point.
    frame(
        &mut head,
        &time_update_frame(48_000, i64::MIN),
        SERVER_FRAMING,
    );

    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let stream = GappedDuplex {
        head: std::io::Cursor::new(head),
        tail: std::io::Cursor::new(keepalive_script(92)),
        // Sixteen idle waits: a few hundred milliseconds of quiet, whole
        // steps of it.
        stalls: 16,
        tail_stalls: 0,
        outgoing,
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    Session::new(Conn::new(stream), config())
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
        vec![(48_000, i64::MIN)],
        "the smallest value negates to itself and is the clock's value: {events:?}"
    );
    // The receipt reports the sky the clock produces — the whole chain runs
    // on the smallest value — and the frozen gate holds it: no tick reports
    // another.
    let skies: Vec<SkyReport> = events.iter().filter_map(sky_report).collect();
    assert_eq!(
        skies.len(),
        1,
        "one sky, the receipt's; the frozen ticks report none: {events:?}"
    );
    assert!(
        skies[0].moon_phase < 8,
        "the moon phase stays in the source's range: {}",
        skies[0].moon_phase
    );
    let ticks = events
        .iter()
        .filter(|event| matches!(event, ClientEvent::PlayerTick { .. }))
        .count();
    assert!(ticks >= 3, "the quiet stretch owes whole steps: {ticks}");
}
