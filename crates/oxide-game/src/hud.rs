//! The debug overlay's lines, as the F3 screen reports them.
//!
//! The builder mirrors the source's `GuiOverlayDebug` field set
//! (`GuiOverlayDebug.java`:102-217): the left column's thirteen fields in
//! order — version, the frame-rate line, `C:`, `E:`, `P:/T:`, the provider
//! name, a blank separator, `XYZ:`, `Block:`, `Chunk:`, `Facing:` and, when
//! the view block is loaded, `Biome:`, `Light:` and `Local Difficulty:`,
//! plus `Looking at:` whenever a block is targeted — and the right column's
//! nine environment rows plus the targeted block's registry section. The
//! lines are flat text so the renderer draws them without any understanding
//! of what they mean; the environment facts (memory, CPU, display, compiler)
//! ride the state because only the window can read them.

use oxide_world::behaviour::{behaviour, variant_key};
use oxide_world::biome::biome;

/// The protocol version the overlay names, the source's own `1.8.9` literal.
pub const DEBUG_GAME_VERSION: &str = "1.8.9";
/// The client's brand on the version line, standing in for the source's
/// `ClientBrandRetriever` name.
pub const DEBUG_BRAND: &str = "Oxidecraft";

/// The compass names the yaw sector maps to, indexed by 90-degree sector:
/// south, west, north, east.
const FACINGS: [&str; 4] = ["south", "west", "north", "east"];
/// The Towards-phrase each compass name carries, in the same order.
const TOWARDS: [&str; 4] = [
    "Towards positive Z",
    "Towards negative X",
    "Towards negative Z",
    "Towards positive X",
];

/// The moon's fullness per phase, the source's own eight factors
/// (`WorldProvider.java`:19): full at 0, new at 4.
const MOON_FACTORS: [f32; 8] = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];

/// What the debug overlay reports about the session this frame: the live
/// values the left column formats plus the environment facts the right
/// column reads. The window feeds every field from the session's events;
/// anything it has not heard yet keeps its default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudState {
    /// The frame rate the counter last measured.
    pub fps: f32,
    /// The dimension: -1 nether, 0 overworld, 1 end. The window's own read
    /// for the music and fog kinds — the overlay builder ignores it.
    pub dimension: i8,
    /// The player's position as the server reports it.
    pub position: [f64; 3],
    /// Yaw in degrees.
    pub yaw: f32,
    /// Pitch in degrees.
    pub pitch: f32,
    /// Meshed chunk columns the window holds.
    pub chunks_meshed: u32,
    /// Loaded chunk columns the window holds.
    pub chunks_total: u32,
    /// The effective view distance in chunks.
    pub view_distance: u8,
    /// Tracked entities the frustum keeps.
    pub entities_rendered: usize,
    /// Tracked entities in total.
    pub entities_total: usize,
    /// Every live particle, drawn or not.
    pub particles: usize,
    /// The world's provider name, from Join Game's level type.
    pub provider: String,
    /// The difficulty byte Join Game carried: 0 peaceful, 1 easy, 2 normal,
    /// 3 hard.
    pub difficulty: u8,
    /// The consumed clock's time of day in ticks.
    pub world_time: i64,
    /// The moon's phase in `0..8`, from the last sky report.
    pub moon_phase: u8,
    /// Whether the view block's column is loaded.
    pub view_loaded: bool,
    /// The view column's biome id.
    pub view_biome: u8,
    /// The view block's sky-light nibble.
    pub view_sky_light: u8,
    /// The view block's block-light nibble.
    pub view_block_light: u8,
    /// The targeted block's coordinates, when the ray meets one.
    pub aim: Option<[i32; 3]>,
    /// The packed block at the targeted cell, when the ray meets one.
    pub aim_block: Option<u16>,
    /// Whether Join Game asked for reduced debug info.
    pub server_reduced_debug: bool,
    /// The compiler version the build script read.
    pub rustc_version: String,
    /// The build's own id.
    pub build_id: String,
    /// The resident bytes, or `None` where the platform exposes nothing.
    pub mem_used_bytes: Option<u64>,
    /// The addressable ceiling the `Mem:` percent reads against.
    pub mem_max_bytes: Option<u64>,
    /// The committed bytes the `Allocated:` line reads.
    pub mem_alloc_bytes: Option<u64>,
    /// The CPU's model string, or `None` where the platform exposes nothing.
    pub cpu_name: Option<String>,
    /// The window's size in pixels, or `None` where there is no window.
    pub display_size: Option<(u32, u32)>,
    /// The GPU backend the display line names.
    pub display_backend: String,
    /// The adapter the renderer lines name.
    pub adapter_name: String,
    /// The adapter's driver, with its info.
    pub adapter_driver: String,
}

/// The overlay's left column, in the source's field order.
///
/// The thirteen fields plus the blank separator entry; the position tail —
/// `Biome:`, `Light:`, `Local Difficulty:` — lands only when the view block
/// is loaded and `Looking at:` only when a block is targeted, the source's
/// own conditions. `reduced` collapses to the source's reduced set: version,
/// the frame-rate line, `C:`, `E:`, `P:/T:`, the provider, the separator and
/// the chunk-relative cell.
pub fn debug_left(state: &HudState, reduced: bool) -> Vec<String> {
    let [x, y, z] = state.position;
    // Block and chunk coordinates floor, so a position at -0.5 reports block
    // -1 and chunk -1 rather than truncating towards zero.
    let block_x = x.floor() as i64;
    let block_y = y.floor() as i64;
    let block_z = z.floor() as i64;
    let mut lines = vec![
        format!(
            "Oxidecraft {DEBUG_GAME_VERSION} ({}/{DEBUG_BRAND})",
            env!("CARGO_PKG_VERSION")
        ),
        format!("{:.0} fps", state.fps),
        format!(
            "C: {}/{} D: {}",
            state.chunks_meshed, state.chunks_total, state.view_distance
        ),
        format!("E: {}/{}", state.entities_rendered, state.entities_total),
        format!("P: {}. T: All: {}", state.particles, state.entities_total),
        state.provider.clone(),
        String::new(),
    ];
    if reduced {
        lines.push(format!(
            "Chunk-relative: {} {} {}",
            block_x & 15,
            block_y & 15,
            block_z & 15
        ));
        return lines;
    }
    let (direction, towards) = facing(state.yaw);
    lines.push(format!("XYZ: {x:.3} / {y:.5} / {z:.3}"));
    lines.push(format!("Block: {block_x} {block_y} {block_z}"));
    lines.push(format!(
        "Chunk: {} {} {} in {} {} {}",
        block_x & 15,
        block_y & 15,
        block_z & 15,
        block_x >> 4,
        block_y >> 4,
        block_z >> 4
    ));
    lines.push(format!(
        "Facing: {direction} ({towards}) ({:.1} / {:.1})",
        wrap_angle_180(state.yaw),
        wrap_angle_180(state.pitch)
    ));
    if state.view_loaded {
        lines.push(format!("Biome: {}", biome(state.view_biome).name));
        lines.push(format!(
            "Light: {} ({} sky, {} block)",
            state.view_sky_light.max(state.view_block_light),
            state.view_sky_light,
            state.view_block_light
        ));
        lines.push(format!(
            "Local Difficulty: {:.2} (Day {})",
            additional_difficulty(
                state.difficulty,
                state.world_time,
                moon_factor(state.moon_phase)
            ),
            state.world_time.div_euclid(24_000)
        ));
    }
    if let Some([ax, ay, az]) = state.aim {
        lines.push(format!("Looking at: {ax} {ay} {az}"));
    }
    lines
}

/// The overlay's right column, in the source's field order: the compiler
/// line, `Mem:` and `Allocated:` as two lines, a separator, `CPU:`, a
/// separator, `Display:` with the adapter and driver lines, then — only when
/// a block is targeted — a separator, the block's registry name and one
/// `name: value` line per block-state property, booleans in the source's
/// green/red.
pub fn debug_right(state: &HudState) -> Vec<String> {
    let bits = if cfg!(target_pointer_width = "64") {
        64
    } else {
        32
    };
    let mut lines = vec![
        format!(
            "Java: rustc {} {bits}bit ({})",
            state.rustc_version, state.build_id
        ),
        mem_line(state),
        allocated_line(state),
        String::new(),
        state
            .cpu_name
            .as_ref()
            .map_or("CPU: n/a".to_string(), |cpu| format!("CPU: {cpu}")),
        String::new(),
        state
            .display_size
            .map_or("Display: n/a".to_string(), |(w, h)| {
                format!("Display: {w}x{h} ({})", state.display_backend)
            }),
        state.adapter_name.clone(),
        state.adapter_driver.clone(),
    ];
    if state.aim.is_some() {
        lines.push(String::new());
        lines.push(aim_block_name(state.aim_block));
        lines.extend(aim_block_properties(state.aim_block));
    }
    lines
}

/// The `Mem:` line: used percent with used/max megabytes, the source's own
/// `% 2d%% %03d/%03dMB` shape, or `n/a` where the platform exposes nothing.
fn mem_line(state: &HudState) -> String {
    let (Some(used), Some(max)) = (state.mem_used_bytes, state.mem_max_bytes) else {
        return "Mem: n/a".to_string();
    };
    if max == 0 {
        return "Mem: n/a".to_string();
    }
    format!(
        "Mem: {:2}% {:03}/{:03}MB",
        used * 100 / max,
        bytes_to_mb(used),
        bytes_to_mb(max)
    )
}

/// The `Allocated:` line: committed percent with committed megabytes, the
/// source's own `% 2d%% %03dMB` shape, or `n/a` where nothing is exposed.
fn allocated_line(state: &HudState) -> String {
    let (Some(alloc), Some(max)) = (state.mem_alloc_bytes, state.mem_max_bytes) else {
        return "Allocated: n/a".to_string();
    };
    if max == 0 {
        return "Allocated: n/a".to_string();
    }
    format!(
        "Allocated: {:2}% {:03}MB",
        alloc * 100 / max,
        bytes_to_mb(alloc)
    )
}

/// The targeted block's registry name, or the numeric id where the behaviour
/// table covers no such block.
fn aim_block_name(aim_block: Option<u16>) -> String {
    let Some(packed) = aim_block else {
        return "unknown".to_string();
    };
    let id = packed >> 4;
    behaviour(id).map_or(format!("Block {id}"), |block| block.name.to_string())
}

/// The targeted block's `name: value` property lines from its metadata, in
/// the key's own name order; booleans carry the source's green/red tints
/// through the standing `§` path.
fn aim_block_properties(aim_block: Option<u16>) -> Vec<String> {
    let Some(packed) = aim_block else {
        return Vec::new();
    };
    let Some(block) = behaviour(packed >> 4) else {
        return Vec::new();
    };
    let meta = (packed & 15) as u8;
    let mut lines = Vec::new();
    for pair in variant_key(block, meta).split(',') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let tint = match value {
            "true" => "§a",
            "false" => "§c",
            _ => "",
        };
        if tint.is_empty() {
            lines.push(format!("{name}: {value}"));
        } else {
            lines.push(format!("{name}: {tint}{value}§r"));
        }
    }
    lines
}

/// The compass direction a yaw faces with its Towards-phrase, in the source's
/// mapping: the yaw rounds into one of four 90-degree sectors — 0 south, 90
/// west, 180 north, 270 (or -90) east — and each names which axis it looks
/// along.
fn facing(yaw: f32) -> (&'static str, &'static str) {
    let sector = (yaw / 90.0 + 0.5).floor() as i64;
    let index = sector.rem_euclid(4) as usize;
    (FACINGS[index], TOWARDS[index])
}

/// Wraps an angle to [-180, 180), the source's own
/// `MathHelper.wrapAngleTo180_float` rule.
pub fn wrap_angle_180(degrees: f32) -> f32 {
    let mut wrapped = degrees % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    }
    if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

/// The moon's fullness for a phase in `0..8`, the source's own factor table.
pub fn moon_factor(phase: u8) -> f32 {
    MOON_FACTORS[(phase & 7) as usize]
}

/// Megabytes for a byte count, the source's own `bytesToMb`: twice by 1024.
pub fn bytes_to_mb(bytes: u64) -> u64 {
    bytes / 1024 / 1024
}

/// The world's additional difficulty at a position, the source's own
/// `DifficultyInstance` arithmetic (`DifficultyInstance.java`:27-50): zero on
/// peaceful, else the difficulty id times the 0.75 base plus the world-time,
/// inhabited-time and moon terms, with easy halving the second half.
///
/// The inhabited time has no port source — every call reads zero — and the
/// integrated-server re-derivation never applies, so both are recorded
/// rather than reconstructed.
pub fn additional_difficulty(difficulty: u8, world_time: i64, moon: f32) -> f32 {
    if difficulty == 0 {
        return 0.0;
    }
    let hard = difficulty >= 3;
    let f1 = clamp01((world_time as f32 - 72_000.0) / 1_440_000.0) * 0.25;
    let mut second = clamp01(0.0 / 3_600_000.0) * if hard { 1.0 } else { 0.75 };
    second += clamp_up(moon * 0.25, f1);
    if difficulty == 1 {
        second *= 0.5;
    }
    f32::from(difficulty.min(3)) * (0.75 + f1 + second)
}

/// Clamps to [0, 1], the source's own `MathHelper.clamp_float` two-sided use.
fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// Clamps to [0, `ceiling`], the moon term's one-sided use.
fn clamp_up(value: f32, ceiling: f32) -> f32 {
    value.clamp(0.0, ceiling)
}
