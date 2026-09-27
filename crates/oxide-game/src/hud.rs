//! The debug overlay's lines, as the F3 screen reports them.
//!
//! The lines are vanilla-styled: the version, the frame rate, the position as
//! the server reports it, the block and chunk it falls in, the compass facing,
//! the dimension, the server and the player's entity id. They are flat text so
//! the renderer can draw them without any understanding of what they mean.

/// One chunk's edge, in blocks.
const CHUNK_SIZE: f64 = 16.0;

/// The compass names vanilla's mapping uses, indexed by 90-degree sector:
/// south, west, north, east.
const FACINGS: [&str; 4] = ["south", "west", "north", "east"];

/// What the debug overlay reports about the session this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HudState {
    /// The frame rate the counter last measured.
    pub fps: f32,
    /// The player's position as the server reports it.
    pub position: [f64; 3],
    /// Yaw in degrees.
    pub yaw: f32,
    /// Pitch in degrees.
    pub pitch: f32,
    /// The dimension: -1 nether, 0 overworld, 1 end.
    pub dimension: i8,
    /// The server address shown on the line.
    pub server: String,
    /// The entity id the server assigned.
    pub entity_id: i32,
}

/// The overlay's lines, in draw order.
pub fn debug_lines(state: &HudState) -> Vec<String> {
    let [x, y, z] = state.position;
    // Block and chunk coordinates floor, so a position at -0.5 reports block
    // -1 and chunk -1 rather than truncating towards zero.
    let block_x = x.floor() as i64;
    let block_y = y.floor() as i64;
    let block_z = z.floor() as i64;
    let chunk_x = (x / CHUNK_SIZE).floor() as i64;
    let chunk_z = (z / CHUNK_SIZE).floor() as i64;
    // The position within its chunk wraps, so x at -0.5 is 15.5 blocks into
    // chunk -1 and reports 15.
    let local_x = x.rem_euclid(CHUNK_SIZE).floor() as i64;
    let local_z = z.rem_euclid(CHUNK_SIZE).floor() as i64;
    vec![
        "Oxidecraft 1.8.9".to_string(),
        format!("{:.0} fps", state.fps),
        format!("x/y/z: {x:.3} / {y:.5} / {z:.3}"),
        format!("Block: {block_x} {block_y} {block_z}"),
        format!("Chunk: {chunk_x} {chunk_z} in {local_x} {local_z}"),
        format!(
            "Facing: {} ({:.1} / {:.1})",
            facing(state.yaw),
            state.yaw,
            state.pitch
        ),
        format!("Dimension: {}", dimension_name(state.dimension)),
        format!("Server: {} (protocol 47)", state.server),
        format!("Entity: {}", state.entity_id),
    ]
}

/// The compass direction a yaw faces, in vanilla's mapping.
///
/// Vanilla rounds the yaw into one of four 90-degree sectors and reads the
/// name from that index: 0 is south, 90 west, 180 north and 270 (or -90) east.
fn facing(yaw: f32) -> &'static str {
    let sector = (yaw / 90.0 + 0.5).floor() as i64;
    FACINGS[sector.rem_euclid(4) as usize]
}

/// The dimension's vanilla name.
fn dimension_name(dimension: i8) -> &'static str {
    match dimension {
        -1 => "Nether",
        0 => "Overworld",
        1 => "The End",
        // Protocol 47 has no other dimension; a modded id is named as unknown
        // rather than mistaken for one of the three.
        _ => "Unknown",
    }
}
