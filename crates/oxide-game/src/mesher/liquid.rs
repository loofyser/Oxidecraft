//! The fluid renderer's geometry: `client/renderer/BlockFluidRenderer.java`.
//!
//! 1.8.9 has no `BlockFluid` class. `BlockLiquid` builds the three blocks in,
//! the culling rules live on the block (`shouldSideBeRendered`,
//! `block/BlockLiquid.java:96-99`, `shouldRenderSides`, `:101-119`) and the
//! geometry is the fluid renderer's, which this module ports pass for pass:
//!
//! 1. the top surface: its four corners are `getFluidHeight`'s average over the
//!    four cells each corner spans (`client/renderer/BlockFluidRenderer.java:255-296`),
//!    and the whole surface sinks by 0.001 when it draws (`:78-81`);
//! 2. the source's second top pass (`:128-134`): the same four points the other
//!    way round, while any of the nine cells at the level above the liquid holds
//!    something that is neither the same material nor a full block;
//! 3. the bottom face at the block's base (`:137-151`), untinted at the
//!    half-grey face shade;
//! 4. the four sides (`:153-249`), each drawn twice — once outward and once
//!    reversed, so the face is visible from inside the liquid too.
//!
//! The sink the top pass applies is what the later passes read: the sides run
//! from the block's base to the *sunk* corner heights, and a cell whose top
//! face culls, culls its second pass with it. A face of another material
//! standing on the liquid cancels neither: `shouldSideBeRendered` keeps the up
//! face under any non-liquid neighbour.
//!
//! # Sprites and uv
//!
//! The still sprite draws the bottom face and a top with no flow direction; the
//! flowing sprite draws every side and a top whose cell has one. The flow
//! direction is the client's own: `BlockLiquid.getFlowDirection`
//! (`:288-292`) normalises `getFlowVector` (`:150-195`) and answers its angle
//! minus a quarter turn, or its `-1000.0` sentinel — `None` here — when the
//! vector is exactly zero. The angle enters the sine table as an `f32` (the
//! renderer narrows it, `:71`), and the uv rotation and the side's v fraction
//! run through `TextureAtlasSprite.getInterpolatedU`/`getInterpolatedV`
//! (`client/renderer/texture/TextureAtlasSprite.java:134-138`, `:159-163`),
//! which divide before multiplying for v and multiply before dividing for u —
//! both shapes are ported as written.
//!
//! # Light, colour and tint
//!
//! One sample per quad, at the source's own cell: the liquid's own cell for the
//! top, the cell below for the bottom and the neighbour for each side
//! (`:117`, `:143`, `:233`), each the per-channel maximum of the sampled cell
//! and the cell above it (`BlockLiquid.getMixedBrightnessForBlock`, `:210-219`)
//! through the same packed pair the model path uses. The face shades are the
//! six the model path bakes (`:53-56`), the bottom face takes the half-grey with
//! no tint (`:146-149`) and every other face multiplies the block's tint:
//! the water colour at the block's own position for water, white for lava
//! (`BlockLiquid.colorMultiplier`, `:40-43`).

use std::sync::LazyLock;

use oxide_assets::atlas::Atlas;
use oxide_render::terrain::{Layer, Vertex};
use oxide_world::behaviour::{BlockBehaviour, LiquidKind, Material, TintKind, behaviour};
use oxide_world::biome::tint_at_9_biome;

use super::{
    ColumnSnapshot, MeshContext, SectionBuilder, cell_pair, face_shade, light_attribute, occludes,
    shaded_channel,
};
use crate::palette::Face;

/// The renderer's corner sink (`f11`, `BlockFluidRenderer.java:65`): the
/// surface drops just below the block's top so a neighbouring block's face is
/// not z-fought.
const SINK: f32 = 0.001;

/// A sprite's two uv corners, [`Atlas::uv`]'s answer: the minimum corner first,
/// the maximum second.
type Rect = [[f32; 2]; 2];

/// Appends one liquid cell's geometry: the source's four passes, in its own
/// order.
///
/// The order matters twice over: the top pass's sink mutates the corner heights
/// every later pass reads, and the visit order is the translucent layer's own
/// where the sort's distances tie.
pub(super) fn append(
    builder: &mut SectionBuilder,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    kind: LiquidKind,
    block: Option<&BlockBehaviour>,
    layer: Layer,
    ctx: &MeshContext<'_>,
) {
    let (x, y, z) = position;
    let sprites = Sprites::resolve(ctx.atlas, kind);
    let tint = liquid_tint(block, snapshot, position, ctx);
    // `getFluidHeight` at the cell's four corners, in the source's own order
    // (`:58-61`): the north-west, south-west, south-east and north-east
    // corners, the source's f7, f8, f9 and f10.
    let mut corners = [
        fluid_height(snapshot, (x, z), y, kind),
        fluid_height(snapshot, (x, z + 1), y, kind),
        fluid_height(snapshot, (x + 1, z + 1), y, kind),
        fluid_height(snapshot, (x + 1, z), y, kind),
    ];
    let base = [x as f32, y as f32, z as f32];

    // The top face. `shouldSideBeRendered`'s up arm: a cell of the same liquid
    // above culls it, and nothing else does — not even a solid neighbour.
    if !same_liquid(snapshot, (x, y + 1, z), kind) {
        let uvs = match flow_direction(snapshot, position, kind) {
            Some(angle) => rotated_uvs(sprites.flowing, angle),
            None => still_uvs(sprites.still),
        };
        // The sink, and it stays sunk for the bottom face and the sides.
        for corner in &mut corners {
            *corner -= SINK;
        }
        let light = mixed_light(snapshot, position);
        let colour = tinted(face_shade(Face::Top), tint);
        let surface = [
            [base[0], base[1] + corners[0], base[2]],
            [base[0], base[1] + corners[1], base[2] + 1.0],
            [base[0] + 1.0, base[1] + corners[2], base[2] + 1.0],
            [base[0] + 1.0, base[1] + corners[3], base[2]],
        ];
        builder.push(layer, quad(surface, uvs, light, colour));
        if should_render_sides(snapshot, (x, y + 1, z), kind) {
            builder.push(
                layer,
                quad(
                    [surface[0], surface[3], surface[2], surface[1]],
                    [uvs[0], uvs[3], uvs[2], uvs[1]],
                    light,
                    colour,
                ),
            );
        }
    }

    // The bottom face: the source's same-material clause first — a liquid below
    // culls it — and the base rule's `!isOpaqueCube` otherwise, the still
    // sprite's own corners and the half-grey with no tint.
    let below = (x, y - 1, z);
    if !same_liquid(snapshot, below, kind) && !occludes(id_at(snapshot, below), ctx.graphics_fast) {
        let light = mixed_light(snapshot, below);
        builder.push(
            layer,
            quad(
                [
                    [base[0], base[1], base[2] + 1.0],
                    [base[0], base[1], base[2]],
                    [base[0] + 1.0, base[1], base[2]],
                    [base[0] + 1.0, base[1], base[2] + 1.0],
                ],
                [
                    [sprites.still[0][0], sprites.still[1][1]],
                    [sprites.still[0][0], sprites.still[0][1]],
                    [sprites.still[1][0], sprites.still[0][1]],
                    [sprites.still[1][0], sprites.still[1][1]],
                ],
                light,
                tinted(face_shade(Face::Bottom), WHITE),
            ),
        );
    }

    // The four sides, north to east, each twice.
    for side in 0..4 {
        let neighbour = match side {
            0 => (x, y, z - 1),
            1 => (x, y, z + 1),
            2 => (x - 1, y, z),
            _ => (x + 1, y, z),
        };
        // The same-material clause, then the base rule (`BlockLiquid.java:96-99`).
        if same_liquid(snapshot, neighbour, kind)
            || occludes(id_at(snapshot, neighbour), ctx.graphics_fast)
        {
            continue;
        }
        // The source's own per-side table (`:190-225`): the two corner heights
        // the face's top edge runs between, and the four coordinates it spans.
        let (low, high, d3, d4, d5, d6) = match side {
            0 => (
                corners[0],
                corners[3],
                base[0],
                base[2] + SINK,
                base[0] + 1.0,
                base[2] + SINK,
            ),
            1 => (
                corners[2],
                corners[1],
                base[0] + 1.0,
                base[2] + 1.0 - SINK,
                base[0],
                base[2] + 1.0 - SINK,
            ),
            2 => (
                corners[1],
                corners[0],
                base[0] + SINK,
                base[2] + 1.0,
                base[0] + SINK,
                base[2],
            ),
            _ => (
                corners[3],
                corners[2],
                base[0] + 1.0 - SINK,
                base[2],
                base[0] + 1.0 - SINK,
                base[2] + 1.0,
            ),
        };
        let face = match side {
            0 => Face::North,
            1 => Face::South,
            2 => Face::West,
            _ => Face::East,
        };
        let uvs = [
            [
                interpolated_u(sprites.flowing, 0.0),
                interpolated_v(sprites.flowing, (1.0 - low) * 16.0 * 0.5),
            ],
            [
                interpolated_u(sprites.flowing, 8.0),
                interpolated_v(sprites.flowing, (1.0 - high) * 16.0 * 0.5),
            ],
            [
                interpolated_u(sprites.flowing, 8.0),
                interpolated_v(sprites.flowing, 8.0),
            ],
            [
                interpolated_u(sprites.flowing, 0.0),
                interpolated_v(sprites.flowing, 8.0),
            ],
        ];
        let light = mixed_light(snapshot, neighbour);
        let colour = tinted(face_shade(face), tint);
        let positions = [
            [d3, base[1] + low, d4],
            [d5, base[1] + high, d6],
            [d5, base[1], d6],
            [d3, base[1], d4],
        ];
        builder.push(layer, quad(positions, uvs, light, colour));
        builder.push(
            layer,
            quad(
                [positions[3], positions[2], positions[1], positions[0]],
                [uvs[3], uvs[2], uvs[1], uvs[0]],
                light,
                colour,
            ),
        );
    }
}

/// The colour a face's vertices take: the face's shade byte times the block's
/// tint, the model path's own composition (`shaded_channel` with the
/// ambient-occlusion multiplier of one).
fn tinted(shade: u8, tint: [u8; 3]) -> [u8; 4] {
    [
        shaded_channel(shade, tint[0], 1.0),
        shaded_channel(shade, tint[1], 1.0),
        shaded_channel(shade, tint[2], 1.0),
        255,
    ]
}

/// The white multiplier a lava face takes (`BlockLiquid.colorMultiplier`,
/// `:40-43`, answers `16777215` for anything but water).
const WHITE: [u8; 3] = [255, 255, 255];

/// One quad of four vertices: the positions and uv corners as given, one packed
/// light for all four, and one colour.
fn quad(positions: [[f32; 3]; 4], uvs: [[f32; 2]; 4], light: i32, colour: [u8; 4]) -> [Vertex; 4] {
    std::array::from_fn(|index| Vertex {
        position: positions[index],
        uv: uvs[index],
        light: light_attribute(light),
        colour,
    })
}

/// `getFluidHeight` (`BlockFluidRenderer.java:255-296`): the average of the
/// four cells one corner spans, from the block's base to the corner's height.
///
/// The queried cell contributes `getLiquidHeightPercent`'s air percent with
/// weight one, and a second time with weight ten when its level is 0 or at
/// least 8; a cell of another non-solid material contributes a plain one; a
/// solid neighbour contributes nothing. A cell of the same liquid anywhere
/// above the corner answers the full height at once (`:264-267`), the waterfall
/// clause, before the other three samples are read.
fn fluid_height(snapshot: &ColumnSnapshot, corner: (i32, i32), y: i32, kind: LiquidKind) -> f32 {
    let mut sum = 0.0f32;
    let mut weight = 0i32;
    for sample in 0..4 {
        let cell = (corner.0 - (sample & 1), y, corner.1 - ((sample >> 1) & 1));
        if same_liquid(snapshot, (cell.0, y + 1, cell.2), kind) {
            return 1.0;
        }
        let value = snapshot.block(cell.0, cell.1, cell.2);
        match behaviour(value >> 4) {
            Some(entry) if entry.liquid == Some(kind) => {
                let level = (value & 0x0F) as u8;
                let air = air_percent(level);
                if level >= 8 || level == 0 {
                    sum += air * 10.0;
                    weight += 10;
                }
                sum += air;
                weight += 1;
            }
            Some(entry) if entry.material.is_solid() => {}
            _ => {
                sum += 1.0;
                weight += 1;
            }
        }
    }
    1.0 - sum / weight as f32
}

/// `BlockLiquid.getLiquidHeightPercent` (`block/BlockLiquid.java:48-56`): the
/// air percent over the level nibble, the `>= 8` clamp first.
///
/// [`oxide_world::behaviour::liquid_height_percent`] is the complement of this
/// — `1 - (level + 1) / 9`, the form a surface's height takes — but the
/// accumulation above adds the air share, and taking the complement to recover
/// it is not bit-identical to this expression: for level 0 the round trip
/// answers `0.111111104` against this `0.11111111`. The source's own form is
/// therefore the one the fluid height uses.
fn air_percent(level: u8) -> f32 {
    let level = if level >= 8 { 0 } else { level };
    (f32::from(level) + 1.0) / 9.0
}

/// `BlockLiquid.getMixedBrightnessForBlock` (`block/BlockLiquid.java:210-219`):
/// the per-channel maximum of the sampled cell's packed light pair and the pair
/// of the cell above it.
///
/// The source masks each channel with 255 before comparing — the byte each
/// channel lives in, in the game's own packing — which is the byte
/// [`cell_pair`]'s packing puts each level in as well.
fn mixed_light(snapshot: &ColumnSnapshot, cell: (i32, i32, i32)) -> i32 {
    let within = cell_pair(snapshot, cell, None);
    let above = cell_pair(snapshot, (cell.0, cell.1 + 1, cell.2), None);
    let channel = |shift: u32| {
        let byte = |packed: i32| ((packed as u32) >> shift) & 0xFF;
        byte(within).max(byte(above)) << shift
    };
    (channel(16) | channel(0)) as i32
}

/// The tint the block's quads take: the nine-sample biome colour for a tinted
/// liquid — the water colour at the block's own position, as
/// `BiomeColorHelper.getWaterColorAtPos` averages it — and white for the rest.
fn liquid_tint(
    block: Option<&BlockBehaviour>,
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    ctx: &MeshContext<'_>,
) -> [u8; 3] {
    match block.map(|entry| entry.tint) {
        Some(TintKind::None) | None => WHITE,
        Some(kind) => tint_at_9_biome(ctx.tint_maps, position.0, position.1, position.2, kind, {
            |x, z| snapshot.biome_at(x, z)
        }),
    }
}

/// The block id at a cell.
fn id_at(snapshot: &ColumnSnapshot, cell: (i32, i32, i32)) -> u16 {
    snapshot.block(cell.0, cell.1, cell.2) >> 4
}

/// Whether a cell holds the same liquid material as the cell being meshed.
///
/// The source compares materials, not blocks (`BlockLiquid.java:98`), so the
/// source and the flowing block of one liquid cull each other while water and
/// lava do not.
fn same_liquid(snapshot: &ColumnSnapshot, cell: (i32, i32, i32), kind: LiquidKind) -> bool {
    behaviour(id_at(snapshot, cell)).is_some_and(|entry| entry.liquid == Some(kind))
}

/// `Block.isFullBlock()`: whether a block fills its cell whole, the ring test's
/// own clause (`block/Block.java:295` sets the field from `isOpaqueCube()` when
/// the block is constructed). Air, and an id outside the table, do not.
fn is_full_block(snapshot: &ColumnSnapshot, cell: (i32, i32, i32)) -> bool {
    behaviour(id_at(snapshot, cell)).is_some_and(|entry| entry.full_cube)
}

/// `BlockLiquid.shouldRenderSides` (`block/BlockLiquid.java:101-119`): whether
/// any of the nine cells around one cell holds something that is neither the
/// same material nor a full block.
fn should_render_sides(snapshot: &ColumnSnapshot, cell: (i32, i32, i32), kind: LiquidKind) -> bool {
    for offset_z in -1..=1 {
        for offset_x in -1..=1 {
            let sample = (cell.0 + offset_x, cell.1, cell.2 + offset_z);
            if same_liquid(snapshot, sample, kind) || is_full_block(snapshot, sample) {
                continue;
            }
            return true;
        }
    }
    false
}

/// `BlockLiquid.getLevel` through `getEffectiveFlowDecay`
/// (`block/BlockLiquid.java:58-67`): the cell's level when it holds the same
/// liquid, `-1` when it does not, and a level at or above 8 collapsed to 0 —
/// the flowing block's falling level.
fn effective_decay(snapshot: &ColumnSnapshot, cell: (i32, i32, i32), kind: LiquidKind) -> i32 {
    if !same_liquid(snapshot, cell, kind) {
        return -1;
    }
    let level = i32::from(snapshot.block(cell.0, cell.1, cell.2) & 0x0F);
    if level >= 8 { 0 } else { level }
}

/// The cell at a horizontal direction, in `EnumFacing.Plane.HORIZONTAL`'s own
/// order: north, south, west, east.
fn horizontal(cell: (i32, i32, i32), direction: usize) -> (i32, i32, i32) {
    match direction {
        0 => (cell.0, cell.1, cell.2 - 1),
        1 => (cell.0, cell.1, cell.2 + 1),
        2 => (cell.0 - 1, cell.1, cell.2),
        _ => (cell.0 + 1, cell.1, cell.2),
    }
}

/// `Material.blocksMovement()` at a cell: whether a walker is stopped by it.
/// Air, and an id outside the table, stop nobody.
fn blocks_movement(snapshot: &ColumnSnapshot, cell: (i32, i32, i32)) -> bool {
    behaviour(id_at(snapshot, cell)).is_some_and(|entry| entry.material.blocks_movement())
}

/// `BlockLiquid.isBlockSolid` (`block/BlockLiquid.java:90-94`) at a horizontal
/// side: the material's own `isSolid()`, with the same liquid and ice answering
/// false first.
fn is_block_solid(snapshot: &ColumnSnapshot, cell: (i32, i32, i32), kind: LiquidKind) -> bool {
    match behaviour(id_at(snapshot, cell)) {
        Some(entry) if entry.liquid == Some(kind) => false,
        Some(entry) if entry.material == Material::Ice => false,
        Some(entry) => entry.material.is_solid(),
        None => false,
    }
}

/// `BlockLiquid.getFlowVector` (`block/BlockLiquid.java:150-195`) in the
/// source's own double shapes, normalised.
///
/// Each horizontal direction adds its offset scaled by the difference between
/// the neighbour's effective decay and the cell's own; a neighbour the liquid
/// does not flow into still reaches in from the cell below when the neighbour
/// blocks no movement and that cell is deeper, re-based by the source's own
/// `i - 8`. A cell whose level is at least 8 takes six units of downward flow
/// when a horizontal neighbour or the cell above it is solid.
fn flow_vector(snapshot: &ColumnSnapshot, position: (i32, i32, i32), kind: LiquidKind) -> [f64; 3] {
    let (x, y, z) = position;
    let own = effective_decay(snapshot, position, kind);
    let mut vector = [0.0f64; 3];
    for direction in 0..4 {
        let neighbour = horizontal(position, direction);
        let decay = effective_decay(snapshot, neighbour, kind);
        let scale = if decay < 0 {
            if blocks_movement(snapshot, neighbour) {
                continue;
            }
            let below = effective_decay(snapshot, (neighbour.0, y - 1, neighbour.2), kind);
            if below < 0 {
                continue;
            }
            below - (own - 8)
        } else {
            decay - own
        };
        let scale = f64::from(scale);
        vector[0] += f64::from(neighbour.0 - x) * scale;
        vector[2] += f64::from(neighbour.2 - z) * scale;
    }
    if level_at(snapshot, position) >= 8 {
        for direction in 0..4 {
            let neighbour = horizontal(position, direction);
            if is_block_solid(snapshot, neighbour, kind)
                || is_block_solid(snapshot, (neighbour.0, neighbour.1 + 1, neighbour.2), kind)
            {
                let normalised = normalize(vector);
                vector = [normalised[0], normalised[1] - 6.0, normalised[2]];
                break;
            }
        }
    }
    normalize(vector)
}

/// The level nibble at the liquid's own cell, the falling clause's test
/// (`BlockLiquid.java:180`).
fn level_at(snapshot: &ColumnSnapshot, cell: (i32, i32, i32)) -> u8 {
    (snapshot.block(cell.0, cell.1, cell.2) & 0x0F) as u8
}

/// `Vec3.normalize` (`util/Vec3.java:52-57`): the vector over its length, the
/// zero vector below the source's `1.0E-4` guard.
///
/// The length is `MathHelper.sqrt_double`'s, which is the `f32` square root
/// widened back to double (`util/MathHelper.java:48-51`), so the division runs
/// against the narrowed length.
fn normalize(vector: [f64; 3]) -> [f64; 3] {
    let squared = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    let length = f64::from(squared.sqrt() as f32);
    if length < 1.0E-4 {
        return [0.0, 0.0, 0.0];
    }
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

/// `BlockLiquid.getFlowDirection` (`block/BlockLiquid.java:288-292`): the
/// angle of the normalised flow vector minus a quarter turn, `None` where the
/// source answers its `-1000.0` sentinel because both components are exactly
/// zero.
fn flow_direction(
    snapshot: &ColumnSnapshot,
    position: (i32, i32, i32),
    kind: LiquidKind,
) -> Option<f32> {
    let vector = flow_vector(snapshot, position, kind);
    if vector[0] == 0.0 && vector[2] == 0.0 {
        return None;
    }
    Some((atan2(vector[2], vector[0]) - std::f64::consts::FRAC_PI_2) as f32)
}

// -- the sprites -------------------------------------------------------------

/// The liquid's two sprites: the still one and the flowing one, resolved from
/// the atlas by the names the renderer reads
/// (`BlockFluidRenderer.java:24-31`), each falling back to the atlas's own
/// missing sprite exactly as the model path does.
struct Sprites {
    /// The bottom face, and a top with no flow direction.
    still: Rect,
    /// Every side, and a top whose cell has a flow direction.
    flowing: Rect,
}

impl Sprites {
    /// The pair for one liquid, water or lava (`:37`).
    fn resolve(atlas: &Atlas, kind: LiquidKind) -> Sprites {
        let (still, flowing) = match kind {
            LiquidKind::Water => ("blocks/water_still", "blocks/water_flow"),
            LiquidKind::Lava => ("blocks/lava_still", "blocks/lava_flow"),
        };
        let rect = |name: &str| atlas.uv(atlas.sprites.get(name).unwrap_or(&atlas.missing));
        Sprites {
            still: rect(still),
            flowing: rect(flowing),
        }
    }
}

/// `TextureAtlasSprite.getInterpolatedU` (`client/renderer/texture/TextureAtlasSprite.java:134-138`):
/// the sprite's minimum u plus its width times the 0..16 coordinate over the
/// clone's own `16.0F` divisor.
fn interpolated_u(rect: Rect, coordinate: f32) -> f32 {
    let [min, max] = rect;
    min[0] + (max[0] - min[0]) * coordinate / 16.0
}

/// `TextureAtlasSprite.getInterpolatedV` (`:159-163`): the same mapping, with
/// the source's own order — the coordinate divides before the width multiplies.
fn interpolated_v(rect: Rect, coordinate: f32) -> f32 {
    let [min, max] = rect;
    min[1] + (max[1] - min[1]) * (coordinate / 16.0)
}

/// The top's uv for a cell with no flow direction (`:91-101`): the sprite's two
/// corners, in the quad's own vertex order.
fn still_uvs(rect: Rect) -> [[f32; 2]; 4] {
    [
        [interpolated_u(rect, 0.0), interpolated_v(rect, 0.0)],
        [interpolated_u(rect, 0.0), interpolated_v(rect, 16.0)],
        [interpolated_u(rect, 16.0), interpolated_v(rect, 16.0)],
        [interpolated_u(rect, 16.0), interpolated_v(rect, 0.0)],
    ]
}

/// The top's uv for a cell with a flow direction (`:104-115`): the sprite's
/// middle plus a quarter-turn's offsets, the four corners of the sprite's
/// rotated centre band.
fn rotated_uvs(rect: Rect, angle: f32) -> [[f32; 2]; 4] {
    let sine = sin(angle) * 0.25;
    let cosine = cos(angle) * 0.25;
    [
        [
            interpolated_u(rect, 8.0 + (-cosine - sine) * 16.0),
            interpolated_v(rect, 8.0 + (-cosine + sine) * 16.0),
        ],
        [
            interpolated_u(rect, 8.0 + (-cosine + sine) * 16.0),
            interpolated_v(rect, 8.0 + (cosine + sine) * 16.0),
        ],
        [
            interpolated_u(rect, 8.0 + (cosine + sine) * 16.0),
            interpolated_v(rect, 8.0 + (cosine - sine) * 16.0),
        ],
        [
            interpolated_u(rect, 8.0 + (cosine - sine) * 16.0),
            interpolated_v(rect, 8.0 + (-cosine - sine) * 16.0),
        ],
    ]
}

// -- the client's own maths --------------------------------------------------

/// `MathHelper.sin` (`util/MathHelper.java:30-33`): the angle times 10430.378,
/// truncated towards zero and masked into the sine table.
fn sin(value: f32) -> f32 {
    SINE_TABLE[table_index(value * 10430.378)]
}

/// `MathHelper.cos` (`:38-41`): the sine table read a quarter period along.
fn cos(value: f32) -> f32 {
    SINE_TABLE[table_index(value * 10430.378 + 16384.0)]
}

/// The source's own index arithmetic: `(int) value & 65535`, the truncation and
/// the mask both as written.
fn table_index(value: f32) -> usize {
    ((value as i32) as u32 & 0xFFFF) as usize
}

/// `MathHelper.SIN_TABLE` (`util/MathHelper.java:13`), filled once
/// (`:546-549`) with `(float) Math.sin(i * PI * 2 / 65536)`.
static SINE_TABLE: LazyLock<[f32; 65536]> = LazyLock::new(|| {
    std::array::from_fn(|index| (index as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
});

/// The source's `atan2` tables (`util/MathHelper.java:23-24`, filled at
/// `:556-562`): the arcsine of `j / 256` and its cosine, for `j` in 0..=256.
struct AtanTables {
    /// `field_181164_e`.
    angle: [f64; 257],
    /// `field_181165_f`.
    cosine: [f64; 257],
}

/// The tables, built once — the source fills them in a static initialiser.
static ATAN_TABLES: LazyLock<AtanTables> = LazyLock::new(|| {
    let mut angle = [0.0f64; 257];
    let mut cosine = [0.0f64; 257];
    for (index, (slot_angle, slot_cosine)) in angle.iter_mut().zip(cosine.iter_mut()).enumerate() {
        let ratio = index as f64 / 256.0;
        *slot_angle = ratio.asin();
        *slot_cosine = slot_angle.cos();
    }
    AtanTables { angle, cosine }
});

/// `field_181163_d` (`util/MathHelper.java:552`):
/// `Double.longBitsToDouble(4805340802404319232L)`, which is `2^44` — the
/// constant the approximation adds to the smaller normalised component so that
/// its low mantissa bits carry the table index.
const MAGIC: f64 = 17_592_186_044_416.0;

/// `MathHelper.atan2` (`util/MathHelper.java:411-472`): the client's own
/// approximation, not the platform's `atan2` — the inverse-square-root estimate
/// with one Newton step, the `2^44` magic that indexes the two tables, the
/// fitted remainder and the three quadrant reflections.
fn atan2(a: f64, b: f64) -> f64 {
    let squared = b * b + a * a;
    if squared.is_nan() {
        return f64::NAN;
    }
    let negative_a = a < 0.0;
    let a = if negative_a { -a } else { a };
    let negative_b = b < 0.0;
    let b = if negative_b { -b } else { b };
    // The source swaps so that its second argument holds the larger of the two.
    let swapped = a > b;
    let (a, b) = if swapped { (b, a) } else { (a, b) };
    let estimate = inverse_sqrt(squared);
    let b = b * estimate;
    let a = a * estimate;
    let magic = MAGIC + a;
    let index = (magic.to_bits() as u32) as usize;
    let tables = &*ATAN_TABLES;
    let (angle, cosine) = (tables.angle[index], tables.cosine[index]);
    let offset = magic - MAGIC;
    let fitted = a * cosine - b * offset;
    let mut result = angle + (6.0 + fitted * fitted) * fitted * 0.16666666666666666;
    if swapped {
        result = std::f64::consts::FRAC_PI_2 - result;
    }
    if negative_b {
        result = std::f64::consts::PI - result;
    }
    if negative_a {
        result = -result;
    }
    result
}

/// `MathHelper.func_181161_i` (`util/MathHelper.java:475-484`): the inverse
/// square root estimate — the bit trick, the `6910469410427058090` constant and
/// one Newton step, all as written.
fn inverse_sqrt(value: f64) -> f64 {
    let half = 0.5 * value;
    let bits = 6910469410427058090i64 - ((value.to_bits() as i64) >> 1);
    let estimate = f64::from_bits(bits as u64);
    estimate * (1.5 - half * estimate * estimate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_atan2_magic_is_the_sources_bit_pattern() {
        assert_eq!(MAGIC.to_bits(), 4_805_340_802_404_319_232);
        assert_eq!(MAGIC, 17_592_186_044_416.0);
    }

    #[test]
    fn the_air_percent_is_not_the_heights_complement() {
        // The source's own form against the double complement Task 5's helper
        // would give: the two differ by a bit, and the fluid height uses the
        // former.
        assert_eq!(air_percent(0), 1.0 / 9.0);
        assert_eq!(air_percent(0), 0.11111111);
        assert_eq!(
            1.0 - oxide_world::behaviour::liquid_height_percent(0),
            0.111111104
        );
        assert_eq!(air_percent(8), 1.0 / 9.0, "the >= 8 clamp");
        assert_eq!(air_percent(7), 8.0 / 9.0);
    }

    #[test]
    fn the_atan2_port_answers_the_axes_exactly() {
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        assert_eq!(atan2(-1.0, 0.0), -std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn the_atan2_port_stays_close_to_the_true_angle() {
        // The client's approximation, not the platform's: its error is bounded
        // far below a texel but it is not exact.
        for step in -8..=8 {
            let a = f64::from(step) * 0.25;
            let b = 1.0;
            assert!((atan2(a, b) - a.atan2(b)).abs() < 1.0e-4, "atan2({a}, {b})");
        }
    }

    #[test]
    fn the_sine_table_reads_the_sources_quarter_turns() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(sin(std::f32::consts::FRAC_PI_2), 1.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(cos(std::f32::consts::FRAC_PI_2), 1.2246468e-16);
    }
}
