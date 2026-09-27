//! Tests for the light engine: the vanilla sky-light rules, the block-light
//! spread, the region recomputations, the query the mesher uses and the light
//! setters.
//!
//! Every expected number is derived from the decompiled 1.8.9 client under
//! `refs/_src/MCP-919` before it is written here:
//!
//! * `World.getRawLight` (`world/World.java:2783-2834`): a cell's level is the
//!   greater of its own emission and, for each of the six neighbours, the
//!   neighbour's level minus the cell's own attenuation; the attenuation is
//!   `max(1, lightOpacity)`, it is 1 for a cell that is both opaque and
//!   luminous (`:2795-2798`), and an opaque cell that does not emit takes no
//!   inflow at all (`:2805-2808`).
//! * `Chunk.canSeeSky` (`world/chunk/Chunk.java:904-910`): a cell is directly
//!   exposed when its y is at or above the height map, which
//!   `Chunk.generateSkylightMap` (`:246-311`) sets one above the topmost cell
//!   of the column whose opacity is not zero. `getRawLight` returns 15 for
//!   such a cell.
//! * The column fill's decrement (`Chunk.java:277-297`): the running level
//!   starts at 15, a transparent cell takes nothing off it while it is still
//!   15 (`:281-286`), and every other step takes the receiving cell's opacity
//!   off it. Water and ice carry opacity 3 (`Block.java:1261-1262`, `:1337`)
//!   and leaves opacity 1 (`BlockLeaves.java:33`), so a light-filtering block
//!   costs its own opacity, not one.
//! * Block light: emitters start at `getLightValue()`; the same recurrence
//!   spreads them six ways with no direct-exposure term (`World.java:2792`).

use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
use oxide_world::chunk::{Chunk, SECTION_COUNT, SECTION_SIZE, Section};
use oxide_world::light::{light_at, recompute, recompute_column};
use oxide_world::world::World;

/// Air.
const AIR: u16 = 0;
/// Stone, id 1: opaque (opacity 255), no emission.
const STONE: u16 = 1 << 4;
/// Water, id 9: opacity 3, no emission.
const WATER: u16 = 9 << 4;
/// Leaves, id 18: opacity 1, no emission.
const LEAVES: u16 = 18 << 4;
/// A torch, id 50: emission 14, opacity 0.
const TORCH: u16 = 50 << 4;
/// Glowstone, id 89: opaque (opacity 255) and luminous (emission 15).
const GLOWSTONE: u16 = 89 << 4;

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// One section's wire data: `block_at(local x, y, local z)` gives each packed
/// block value with `y` the world height 0..256, and every nibble of both
/// light arrays is `light` (the sky-light array is dropped in a dimension
/// without sky, as the wire drops it).
fn section_of(
    has_sky: bool,
    block_light: u8,
    sky_light: u8,
    block_at: impl Fn(usize, usize, usize) -> u16,
) -> SectionData {
    let mut blocks = Box::new([0u16; 4096]);
    for ly in 0..SECTION_SIZE {
        for lz in 0..SECTION_SIZE {
            for lx in 0..SECTION_SIZE {
                blocks[block_index(lx, ly, lz)] = block_at(lx, ly, lz);
            }
        }
    }
    SectionData {
        blocks,
        block_light: Box::new([(block_light & 0x0F) * 0x11; 2048]),
        sky_light: has_sky.then(|| Box::new([(sky_light & 0x0F) * 0x11; 2048])),
    }
}

/// A column whose sixteen sections are all present, every cell's block value
/// from `block_at(x, y, z)` with `y` the world height, and every light nibble
/// `block_light` or `sky_light`.
fn column_of(
    block_light: u8,
    sky_light: u8,
    block_at: impl Fn(usize, usize, usize) -> u16,
) -> ColumnData {
    let mut data = ColumnData::empty();
    for sy in 0..SECTION_COUNT {
        data.sections[sy] = Some(section_of(true, block_light, sky_light, |lx, ly, lz| {
            block_at(lx, sy * SECTION_SIZE + ly, lz)
        }));
        data.mask |= 1u16 << sy;
    }
    data
}

/// The wire data of the section update the change arrives as: the two sections
/// the change touches (3 and 4, y 48..79), both light arrays zero, the shape a
/// section update carries: `block_at(x, y, z)` gives each packed block value
/// with `y` the world height.
fn update_of(block_at: impl Fn(usize, usize, usize) -> u16) -> ColumnData {
    let mut data = ColumnData::empty();
    for section in [3usize, 4] {
        data.sections[section] = Some(section_of(true, 0, 0, |lx, ly, lz| {
            block_at(lx, section * SECTION_SIZE + ly, lz)
        }));
        data.mask |= 1u16 << section;
    }
    data
}

/// A world of `size` x `size` chunk columns centred on the origin, every column
/// from `column_at(cx, cz)`.
fn world_of(size: i32, column_at: impl Fn(i32, i32) -> ColumnData) -> World {
    let half = size / 2;
    let mut world = World::new(true);
    for cx in -half..=half {
        for cz in -half..=half {
            world.apply_column(cx, cz, &column_at(cx, cz), true);
        }
    }
    world
}

/// A world of `size` x `size` chunk columns of stone through y 63 and air
/// above, every light nibble zero.
fn flat_world(size: i32) -> World {
    world_of(size, |_, _| {
        column_of(0, 0, |_, y, _| if y <= 63 { STONE } else { AIR })
    })
}

/// The stored sky and block light at a world position, read through the
/// store's column getters.
fn stored(world: &World, x: i32, y: i32, z: i32) -> (u8, u8) {
    let chunk = world
        .chunk(x.div_euclid(16), z.div_euclid(16))
        .expect("the column is loaded");
    let lx = x.rem_euclid(16) as usize;
    let lz = z.rem_euclid(16) as usize;
    (
        chunk.sky_light_at(lx, y as usize, lz),
        chunk.block_light_at(lx, y as usize, lz),
    )
}

/// The stored sky light at a world position.
fn sky(world: &World, x: i32, y: i32, z: i32) -> u8 {
    stored(world, x, y, z).0
}

/// The stored block light at a world position.
fn block(world: &World, x: i32, y: i32, z: i32) -> u8 {
    stored(world, x, y, z).1
}

/// The stored sky and block light of every cell of a whole column, y varying
/// fastest inside each (x, z) position.
fn stored_column(world: &World, cx: i32, cz: i32) -> Vec<(u8, u8)> {
    let chunk = world.chunk(cx, cz).expect("the column is loaded");
    let mut cells = Vec::with_capacity(SECTION_SIZE * SECTION_SIZE * SECTION_COUNT * SECTION_SIZE);
    for lz in 0..SECTION_SIZE {
        for lx in 0..SECTION_SIZE {
            for y in 0..SECTION_COUNT * SECTION_SIZE {
                cells.push((
                    chunk.sky_light_at(lx, y, lz),
                    chunk.block_light_at(lx, y, lz),
                ));
            }
        }
    }
    cells
}

/// The stored block values of every cell of a whole column, y varying fastest
/// inside each (x, z) position.
fn stored_blocks(world: &World, cx: i32, cz: i32) -> Vec<u16> {
    let chunk = world.chunk(cx, cz).expect("the column is loaded");
    let mut blocks = Vec::with_capacity(SECTION_SIZE * SECTION_SIZE * SECTION_COUNT * SECTION_SIZE);
    for lz in 0..SECTION_SIZE {
        for lx in 0..SECTION_SIZE {
            for y in 0..SECTION_COUNT * SECTION_SIZE {
                blocks.push(chunk.block(lx, y, lz));
            }
        }
    }
    blocks
}

/// The biome array of a whole column, in the wire's order: z-major, then x.
fn stored_biomes(world: &World, cx: i32, cz: i32) -> Vec<u8> {
    let chunk = world.chunk(cx, cz).expect("the column is loaded");
    let mut biomes = Vec::with_capacity(SECTION_SIZE * SECTION_SIZE);
    for lz in 0..SECTION_SIZE {
        for lx in 0..SECTION_SIZE {
            biomes.push(chunk.biome(lx, lz));
        }
    }
    biomes
}

/// How many section slots a column holds.
///
/// The store answers no presence query, and in a dimension with sky a present
/// air section reads 15 above a column's blockers — exactly what an absent
/// section's default answers — so the slots are counted out of the column's
/// own debug view.
fn held_sections(world: &World, cx: i32, cz: i32) -> usize {
    let chunk = world.chunk(cx, cz).expect("the column is loaded");
    format!("{chunk:?}").matches("Some(Section").count()
}

// ---------------------------------------------------------------------------
// Sky light.
// ---------------------------------------------------------------------------

#[test]
fn a_flat_world_keeps_sky_fifteen_all_the_way_down() {
    let mut world = flat_world(3);
    recompute(&mut world, 0, 64, 0);

    for y in 64..256 {
        assert_eq!(sky(&world, 0, y, 0), 15, "an open column is 15 at y {y}");
    }
    for y in 0..64 {
        assert_eq!(sky(&world, 0, y, 0), 0, "stone stops it at y {y}");
        assert_eq!(block(&world, 0, y, 0), 0, "and nothing emits");
    }
    assert_eq!(light_at(&world, 8, 255, 8), 15, "up to the world's ceiling");
    assert_eq!(
        light_at(&world, -16, 64, -16),
        15,
        "and out to the region's corner"
    );
}

#[test]
fn a_hole_keeps_fifteen_down_and_loses_one_sideways() {
    // Stone through y 63, a 1x1x5 hole at (0, 59..=63, 0), and a one-cell
    // notch beside the hole's bottom at (1, 59, 0).
    let mut world = world_of(3, |_, _| {
        column_of(0, 0, |x, y, z| {
            if y <= 63 && !(x == 0 && z == 0 && y >= 59) && !(x == 1 && z == 0 && y == 59) {
                STONE
            } else {
                AIR
            }
        })
    });
    recompute(&mut world, 0, 64, 0);

    for y in 59..64 {
        assert_eq!(sky(&world, 0, y, 0), 15, "the hole stays 15 at y {y}");
    }
    assert_eq!(
        sky(&world, 1, 59, 0),
        14,
        "the notch is one step sideways: 15 - max(1, 0)"
    );
    assert_eq!(
        sky(&world, 0, 58, 0),
        0,
        "the stone floor under it stops light"
    );
    assert_eq!(sky(&world, 1, 60, 0), 0, "and so does the notch's ceiling");
    assert_eq!(sky(&world, 2, 59, 0), 0, "and the far wall");
    assert_eq!(sky(&world, 1, 59, 1), 0, "and the side wall");
}

#[test]
fn a_leaf_canopy_reads_fifteen_minus_the_filter_and_spreads() {
    // Stone through y 60, then a two-layer 7x7 canopy of leaves at y 64 and 65
    // over x, z in 0..=6.
    let mut world = world_of(3, |_, _| {
        column_of(0, 0, |x, y, z| {
            if y <= 60 {
                STONE
            } else if (64..=65).contains(&y) && x <= 6 && z <= 6 {
                LEAVES
            } else {
                AIR
            }
        })
    });
    recompute(&mut world, 3, 65, 3);

    assert_eq!(sky(&world, 3, 65, 3), 14, "the upper leaf cell: 15 - 1");
    assert_eq!(sky(&world, 3, 64, 3), 13, "the lower one: 14 - 1");
    assert_eq!(
        sky(&world, 3, 63, 3),
        12,
        "and one below the canopy: 13 - 1"
    );
    // Under the canopy the light enters from the open columns at its edge and
    // loses one a step: 14 at the rim, 13 a step in, 12 in the middle.
    assert_eq!(sky(&world, 0, 63, 3), 14, "the canopy's rim");
    assert_eq!(sky(&world, 1, 63, 3), 13, "one step in");
    assert_eq!(sky(&world, 2, 63, 3), 12, "and the middle of the 7x7 patch");
    assert_eq!(sky(&world, 4, 63, 3), 12, "which is symmetric");
    assert_eq!(sky(&world, 5, 63, 3), 13);
    assert_eq!(sky(&world, 6, 63, 3), 14);
    assert_eq!(sky(&world, 7, 63, 3), 15, "outside the patch is open sky");
    assert_eq!(sky(&world, 7, 65, 3), 15, "at the canopy's own level too");
    assert_eq!(sky(&world, 3, 62, 3), 11, "and the shade deepens below it");
    assert_eq!(sky(&world, 3, 60, 3), 0, "until the stone floor stops it");
}

#[test]
fn an_opaque_roof_lights_by_the_side_spread_only() {
    // Stone through y 60, an opaque 11x11 roof at y 64 over x, z in 0..=10.
    let mut world = world_of(3, |_, _| {
        column_of(0, 0, |x, y, z| {
            if y <= 60 || (y == 64 && x <= 10 && z <= 10) {
                STONE
            } else {
                AIR
            }
        })
    });
    recompute(&mut world, 5, 64, 5);

    assert_eq!(sky(&world, 5, 65, 5), 15, "the sky above the roof is full");
    assert_eq!(sky(&world, 5, 64, 5), 0, "the roof itself stops light");
    assert_eq!(
        sky(&world, 11, 63, 5),
        15,
        "beside the roof, the column is open"
    );
    // Under the roof nothing comes down. The pinned cells take the chain that
    // comes in sideways off the open column at x = -1 (the local x = 15 edge of
    // the neighbouring chunk): 15 - 1 at x = 0 down to 15 - 6 at x = 5. The open
    // column at x = 11 also sends a chain in, but it is one step weaker at these
    // cells (4..=9 at x = 0..=5, the two chains tying at 9 at x = 5), so it
    // dominates only from x = 6 east.
    for (x, expected) in [(0, 14), (1, 13), (2, 12), (3, 11), (4, 10), (5, 9)] {
        assert_eq!(
            sky(&world, x, 63, 5),
            expected,
            "the side-spread chain at x {x}"
        );
    }
    assert_eq!(
        sky(&world, 5, 62, 5),
        9,
        "and the same chain one level lower"
    );
}

#[test]
fn a_light_filtering_block_costs_its_own_opacity() {
    // Stone through y 63, a shaft of five water cells at (0, 59..=63, 0).
    // Water's opacity is 3 (`Block.java:1261-1262`), so each step into a water
    // cell costs 3: the stack reads 12, 9, 6, 3, 0 from its top down. The spec
    // and the protocol research report both say "reduces by exactly one"; the
    // source wins, and this test pins the source.
    let mut world = world_of(3, |_, _| {
        column_of(0, 0, |x, y, z| {
            if y <= 58 {
                STONE
            } else if x == 0 && z == 0 && (59..=63).contains(&y) {
                WATER
            } else if y <= 63 {
                STONE
            } else {
                AIR
            }
        })
    });
    recompute(&mut world, 0, 64, 0);

    assert_eq!(sky(&world, 0, 64, 0), 15, "the open air above the shaft");
    assert_eq!(
        sky(&world, 0, 63, 0),
        12,
        "15 - 3 entering the first water cell"
    );
    assert_eq!(sky(&world, 0, 62, 0), 9, "12 - 3");
    assert_eq!(sky(&world, 0, 61, 0), 6, "9 - 3");
    assert_eq!(sky(&world, 0, 60, 0), 3, "6 - 3");
    assert_eq!(sky(&world, 0, 59, 0), 0, "3 - 3");
    assert_eq!(
        sky(&world, 1, 63, 0),
        0,
        "the shaft's stone wall takes nothing"
    );
}

#[test]
fn a_glowstone_cell_receives_sky_light_and_emits_block_light() {
    // Stone through y 63 with a glowstone cell at (8, 64, 8), open to the sky.
    // Glowstone is opaque (opacity 255, `behaviour.rs` id 89) and luminous
    // (emission 15, `Block.java:1348`): the opaque-and-luminous clause of
    // `World.getRawLight` (`:2795-2798`) lifts its attenuation from "no inflow"
    // to 1, so the cell takes the sky one step down from full and still holds
    // and spreads its own emission. With the clause off it would take nothing
    // and read sky 0 — the one regression this test exists to catch.
    let mut world = world_of(1, |_, _| {
        column_of(0, 0, |x, y, z| {
            if x == 8 && y == 64 && z == 8 {
                GLOWSTONE
            } else if y <= 63 {
                STONE
            } else {
                AIR
            }
        })
    });
    recompute_column(&mut world, 0, 0);

    assert_eq!(sky(&world, 8, 64, 8), 14, "the glowstone takes 15 - 1");
    assert_eq!(block(&world, 8, 64, 8), 15, "and holds its emission");
    assert_eq!(block(&world, 9, 64, 8), 14, "which spreads one step out");
    assert_eq!(sky(&world, 8, 65, 8), 15, "the sky above it stays full");
}

// ---------------------------------------------------------------------------
// Block light.
// ---------------------------------------------------------------------------

#[test]
fn a_torch_lights_a_manhattan_diamond() {
    let mut world = world_of(3, |_, _| column_of(0, 0, |_, _, _| AIR));
    let mut torch = ColumnData::empty();
    torch.sections[4] = Some(section_of(true, 0, 0, |lx, ly, lz| {
        if lx == 8 && ly == 0 && lz == 8 {
            TORCH
        } else {
            AIR
        }
    }));
    torch.mask = 1u16 << 4;
    world.apply_column(0, 0, &torch, false);
    recompute(&mut world, 8, 64, 8);

    assert_eq!(block(&world, 8, 64, 8), 14, "the torch cell itself");
    assert_eq!(block(&world, 9, 64, 8), 13, "14 - manhattan 1");
    assert_eq!(block(&world, 8, 65, 8), 13, "up as much as sideways");
    assert_eq!(block(&world, 8, 64, 7), 13, "and the other way");
    assert_eq!(block(&world, 10, 64, 8), 12, "14 - manhattan 2");
    assert_eq!(block(&world, 9, 64, 9), 12, "a diagonal step is two steps");
    assert_eq!(block(&world, 10, 65, 8), 11, "14 - manhattan 3");
    assert_eq!(block(&world, 8, 78, 8), 0, "14 - manhattan 14 is zero");
    assert_eq!(block(&world, 8, 64, 22), 0, "and so is every further cell");
    assert_eq!(block(&world, 8, 64, 23), 0);
    assert_eq!(sky(&world, 8, 64, 8), 15, "the sky is not disturbed");
}

#[test]
fn an_opaque_wall_leaves_block_light_zero_behind_it() {
    // Air, a torch at (8, 64, 8), and a full-height stone wall at x = 10 that
    // spans the whole region.
    let mut world = world_of(3, |cx, _| {
        column_of(
            0,
            0,
            |lx, _, _| if cx == 0 && lx == 10 { STONE } else { AIR },
        )
    });
    let mut torch = ColumnData::empty();
    torch.sections[4] = Some(section_of(true, 0, 0, |lx, ly, lz| {
        if lx == 8 && ly == 0 && lz == 8 {
            TORCH
        } else if lx == 10 {
            STONE
        } else {
            AIR
        }
    }));
    torch.mask = 1u16 << 4;
    world.apply_column(0, 0, &torch, false);
    recompute(&mut world, 8, 64, 8);

    assert_eq!(
        block(&world, 8, 64, 8),
        14,
        "the torch is in front of the wall"
    );
    assert_eq!(block(&world, 9, 64, 8), 13, "one step short of it");
    assert_eq!(block(&world, 9, 65, 8), 12, "and diagonally above");
    assert_eq!(block(&world, 10, 64, 8), 0, "the wall cell takes nothing");
    for y in 0..256 {
        for z in -16..32 {
            for x in 10..32 {
                assert_eq!(
                    block(&world, x, y, z),
                    0,
                    "block light {x} {y} {z} behind the wall"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The recomputations.
// ---------------------------------------------------------------------------

/// The after-change world of [`recompute_after_a_change_equals_a_scratch_pass`]:
/// a stone floor through y 63 with a 1x1x5 hole at (5, 59..=63, 5) and a torch
/// on the surface at (2, 64, 2).
fn changed_blocks(x: usize, y: usize, z: usize) -> u16 {
    if y == 64 && x == 2 && z == 2 {
        TORCH
    } else if y <= 63 && !(x == 5 && z == 5 && y >= 59) {
        STONE
    } else {
        AIR
    }
}

#[test]
fn recompute_after_a_change_equals_a_scratch_pass() {
    // The loaded world is exactly the affected region: one chunk column, with
    // its eight neighbours unloaded, so a pass over the region is a pass over
    // the whole world and the equality is exact.
    let mut changed = world_of(1, |_, _| column_of(0, 0, changed_blocks));
    recompute_column(&mut changed, 0, 0);

    // The same world built from the same blocks but with the wire's light,
    // then the change applied as a section update.
    let mut incremental = flat_world(1);
    incremental.apply_column(0, 0, &update_of(changed_blocks), false);
    recompute(&mut incremental, 2, 64, 2);

    assert_eq!(
        sky(&incremental, 5, 63, 5),
        15,
        "the hole's floor is exposed"
    );
    assert_eq!(sky(&incremental, 5, 59, 5), 15, "and its bottom");
    assert_eq!(block(&incremental, 2, 64, 2), 14, "the torch it gained");
    assert_eq!(block(&incremental, 3, 64, 2), 13, "and its light");

    for y in 0..256 {
        for z in 0..16 {
            for x in 0..16 {
                assert_eq!(
                    stored(&incremental, x, y, z),
                    stored(&changed, x, y, z),
                    "the incremental pass agrees at ({x}, {y}, {z})"
                );
            }
        }
    }

    // A second column pass over a now-correct column changes nothing.
    let mut before_column = Vec::new();
    for z in 0..16 {
        for x in 0..16 {
            for y in 0..256 {
                before_column.push(stored(&changed, x, y, z));
            }
        }
    }
    recompute_column(&mut changed, 0, 0);
    let mut after_column = Vec::new();
    for z in 0..16 {
        for x in 0..16 {
            for y in 0..256 {
                after_column.push(stored(&changed, x, y, z));
            }
        }
    }
    assert_eq!(
        before_column, after_column,
        "a column pass over a correct column is a no-op"
    );

    // A second pass over the same world changes nothing.
    let mut before = Vec::new();
    for z in 0..16 {
        for x in 0..16 {
            for y in 0..256 {
                before.push(stored(&incremental, x, y, z));
            }
        }
    }
    recompute(&mut incremental, 2, 64, 2);
    let mut after = Vec::new();
    for z in 0..16 {
        for x in 0..16 {
            for y in 0..256 {
                after.push(stored(&incremental, x, y, z));
            }
        }
    }
    assert_eq!(
        before, after,
        "a recompute over a correct region is a no-op"
    );
}

#[test]
fn cells_outside_the_region_keep_their_light() {
    // A 5x5 world of flat stone: the sixteen chunks of the outer ring carry a
    // sentinel light, the nine inner chunks carry another one.
    const SENTINEL: (u8, u8) = (3, 7);
    const INNER: (u8, u8) = (0, 15);
    let mut world = world_of(5, |cx, cz| {
        let inner = cx.abs() <= 1 && cz.abs() <= 1;
        let (sky_light, block_light) = if inner { INNER } else { SENTINEL };
        column_of(
            block_light,
            sky_light,
            |_, y, _| if y <= 63 { STONE } else { AIR },
        )
    });

    // A change inside the middle chunk: a 1x1x5 hole and the notch beside its
    // bottom.
    let mut changed = ColumnData::empty();
    for sy in [3usize, 4] {
        changed.sections[sy] = Some(section_of(true, 0, 0, |lx, ly, lz| {
            let y = sy * SECTION_SIZE + ly;
            if y <= 63 && !(lx == 8 && lz == 8 && y >= 59) && !(lx == 9 && lz == 8 && y == 59) {
                STONE
            } else {
                AIR
            }
        }));
        changed.mask |= 1u16 << sy;
    }
    world.apply_column(0, 0, &changed, false);
    recompute(&mut world, 8, 64, 8);

    assert_eq!(sky(&world, 8, 63, 8), 15, "the hole keeps 15 down");
    assert_eq!(
        sky(&world, 9, 59, 8),
        14,
        "and the notch loses one sideways"
    );
    assert_eq!(block(&world, 8, 64, 8), 0, "the sentinel does not leak in");

    for cx in -2i32..=2 {
        for cz in -2i32..=2 {
            if cx.abs() <= 1 && cz.abs() <= 1 {
                continue;
            }
            let chunk = world.chunk(cx, cz).expect("the ring is loaded");
            for lz in 0..16 {
                for lx in 0..16 {
                    for y in 0..256 {
                        assert_eq!(
                            (
                                chunk.sky_light_at(lx, y, lz),
                                chunk.block_light_at(lx, y, lz)
                            ),
                            SENTINEL,
                            "the ring chunk ({cx}, {cz}) at ({lx}, {y}, {lz})"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_column_pass_reads_a_loaded_neighbours_sky_at_the_border() {
    // Two loaded columns side by side. The west one is flat stone through
    // y 62, so its y 63 layer sits under open sky at 15. The east one is stone
    // through y 63 with a one-cell pocket at its west edge, local (0, 63, 0),
    // covered by stone at (0, 64, 0) and walled by stone on every other side:
    // the west column's edge cell (15, 63, 0) is the pocket's only source, and
    // the one-cell border read `recompute_column` makes is what carries it
    // across, at 15 - max(1, 0) = 14. With the border read off the pocket
    // stays 0.
    let mut world = world_of(1, |_, _| {
        column_of(0, 0, |_, y, _| if y <= 62 { STONE } else { AIR })
    });
    let mut east = ColumnData::empty();
    for sy in 0..SECTION_COUNT {
        east.sections[sy] = Some(section_of(true, 0, 0, |lx, ly, lz| {
            let y = sy * SECTION_SIZE + ly;
            let pocket = lx == 0 && lz == 0 && y == 63;
            let cover = lx == 0 && lz == 0 && y == 64;
            if (y <= 63 && !pocket) || cover {
                STONE
            } else {
                AIR
            }
        }));
        east.mask |= 1u16 << sy;
    }
    world.apply_column(1, 0, &east, true);

    // The west column's own pass gives it its light; the pocket starts dark.
    recompute_column(&mut world, 0, 0);
    assert_eq!(sky(&world, 15, 63, 0), 15, "the west column's open edge");
    assert_eq!(sky(&world, 16, 63, 0), 0, "the pocket before the pass");
    assert_eq!(sky(&world, 16, 64, 0), 0, "and its cover takes nothing");

    recompute_column(&mut world, 1, 0);
    assert_eq!(
        sky(&world, 16, 63, 0),
        14,
        "the pocket takes 15 - 1 across the border"
    );

    // A second pass over a now-correct column changes nothing.
    let before = stored_column(&world, 1, 0);
    recompute_column(&mut world, 1, 0);
    assert_eq!(
        stored_column(&world, 1, 0),
        before,
        "a second column pass is a no-op"
    );
}

#[test]
fn a_column_pass_materialises_the_sections_the_column_does_not_hold() {
    // A section outside the mask "keeps the light it had before a recompute"
    // in the region sense: the loaded columns outside the pass's region keep
    // their stored light, which `cells_outside_the_region_keep_their_light`
    // pins. In the store's own sense — a section the column does not hold — the
    // pass changes the value: it materialises the absent sections as air
    // carrying the computed light, so their reads become the computed ones.
    // That is the defined behaviour for M2; sparse-column semantics are
    // deferred to M3.
    //
    // The world is one column holding only a stone-filled section 5 (y 80..95):
    // nothing lights the cells below the stone, and the cell above it is
    // directly exposed.
    let mut world = World::new(true);
    let mut sparse = ColumnData::empty();
    sparse.sections[5] = Some(section_of(true, 0, 0, |_, _, _| STONE));
    sparse.mask = 1u16 << 5;
    world.apply_column(0, 0, &sparse, true);

    assert_eq!(held_sections(&world, 0, 0), 1, "only section 5 is held");
    assert_eq!(
        sky(&world, 0, 40, 0),
        15,
        "an absent section reads its sky default"
    );

    recompute_column(&mut world, 0, 0);

    assert_eq!(
        held_sections(&world, 0, 0),
        SECTION_COUNT,
        "the pass materialises all sixteen sections"
    );
    assert_eq!(sky(&world, 0, 40, 0), 0, "the y-40 read is now computed");
    assert_eq!(
        sky(&world, 0, 96, 0),
        15,
        "the cell above the stone is direct"
    );
    for y in 80..96 {
        assert_eq!(world.block(0, y, 0), STONE, "the stone at y {y} survives");
    }
    assert_eq!(world.block(0, 100, 0), AIR, "and the air above is air");
    for y in 0..96 {
        assert_eq!(sky(&world, 0, y, 0), 0, "nothing below the stone lights");
    }
}

#[test]
fn a_column_pass_keeps_the_biomes_and_the_blocks() {
    // The write-back rebuilds the column from its blocks and its recomputed
    // light and carries no biome array, and the store keeps the biomes it
    // holds, so a relit column must come through with its 256-byte biome array
    // and its blocks unchanged.
    let mut world = World::new(true);
    let mut column = column_of(0, 0, |_, y, _| if y <= 63 { STONE } else { AIR });
    column.biomes = Some([7u8; 256]);
    world.apply_column(0, 0, &column, true);

    let biomes_before = stored_biomes(&world, 0, 0);
    let blocks_before = stored_blocks(&world, 0, 0);
    assert_eq!(
        biomes_before,
        vec![7u8; 256],
        "the column holds a biome array"
    );

    recompute_column(&mut world, 0, 0);

    assert_eq!(
        stored_biomes(&world, 0, 0),
        biomes_before,
        "the biome array is unchanged"
    );
    assert_eq!(
        stored_blocks(&world, 0, 0),
        blocks_before,
        "and every block is preserved"
    );
}

#[test]
fn a_relight_in_a_dimension_without_sky_leaves_the_sky_dark() {
    // A dimension without sky: the passes gate the sky kind on `has_sky`, the
    // write-back stores no sky-light array, and the store drops one for such a
    // dimension. Block light still spreads, and every sky read stays 0.
    let mut world = World::new(false);
    let column = column_of(0, 0, |x, y, z| {
        if x == 8 && y == 64 && z == 8 {
            TORCH
        } else {
            AIR
        }
    });
    world.apply_column(0, 0, &column, true);

    recompute_column(&mut world, 0, 0);

    assert_eq!(block(&world, 8, 64, 8), 14, "the torch cell holds 14");
    assert_eq!(block(&world, 9, 64, 8), 13, "and one step of spread");
    assert_eq!(
        light_at(&world, 8, 64, 8),
        14,
        "the query reads the block light, not a sky default"
    );
    let chunk = world.chunk(0, 0).expect("the column is loaded");
    for lz in 0..SECTION_SIZE {
        for lx in 0..SECTION_SIZE {
            for y in 0..SECTION_COUNT * SECTION_SIZE {
                assert_eq!(
                    chunk.sky_light_at(lx, y, lz),
                    0,
                    "no sky light at ({lx}, {y}, {lz})"
                );
            }
        }
    }
}

#[test]
fn the_recomputations_are_no_ops_on_an_unloaded_column() {
    // One loaded chunk whose light is a sentinel the passes must not touch.
    let mut world = world_of(1, |_, _| {
        column_of(5, 9, |_, y, _| if y <= 63 { STONE } else { AIR })
    });

    recompute(&mut world, 32, 64, 32);
    recompute_column(&mut world, 3, 3);

    let chunk = world.chunk(0, 0).expect("the loaded column");
    for lz in 0..16 {
        for lx in 0..16 {
            for y in 0..256 {
                assert_eq!(
                    (
                        chunk.sky_light_at(lx, y, lz),
                        chunk.block_light_at(lx, y, lz)
                    ),
                    (9, 5),
                    "the loaded column at ({lx}, {y}, {lz}) did not move"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The query.
// ---------------------------------------------------------------------------

#[test]
fn light_at_is_the_greater_of_the_two_kinds() {
    // Air with a torch at (8, 64, 8): the sky is full, the block light is not.
    let mut open = world_of(1, |_, _| column_of(0, 0, |_, _, _| AIR));
    let mut torch = ColumnData::empty();
    torch.sections[4] = Some(section_of(true, 0, 0, |lx, ly, lz| {
        if lx == 8 && ly == 0 && lz == 8 {
            TORCH
        } else {
            AIR
        }
    }));
    torch.mask = 1u16 << 4;
    open.apply_column(0, 0, &torch, false);
    recompute(&mut open, 8, 64, 8);

    assert_eq!(light_at(&open, 8, 64, 8), 15, "the sky wins over the torch");
    assert_eq!(light_at(&open, 0, 255, 0), 15);
    assert_eq!(
        light_at(&open, 8, 0, 8),
        15,
        "the sky of an open column reaches its floor"
    );

    // Stone through y 63 with a 3x3x3 chamber at (1..=3, 60..=62, 1..=3) and a
    // torch on the chamber floor: the sky is zero in there, the block light is
    // not.
    let mut chamber = world_of(1, |_, _| {
        column_of(0, 0, |x, y, z| {
            if y <= 63 && !((1..=3).contains(&x) && (60..=62).contains(&y) && (1..=3).contains(&z))
            {
                STONE
            } else if y == 60 && x == 2 && z == 2 {
                TORCH
            } else {
                AIR
            }
        })
    });
    recompute(&mut chamber, 2, 60, 2);

    assert_eq!(light_at(&chamber, 2, 60, 2), 14, "the torch's own cell");
    assert_eq!(light_at(&chamber, 2, 61, 2), 13, "13 - manhattan 1");
    assert_eq!(light_at(&chamber, 3, 60, 2), 13);
    assert_eq!(light_at(&chamber, 2, 63, 2), 0, "the ceiling has neither");
    assert_eq!(
        light_at(&chamber, 2, 64, 2),
        15,
        "and the sky above is full"
    );
    assert_eq!(light_at(&chamber, 0, 0, 0), 0, "deep stone is dark");

    assert_eq!(light_at(&chamber, 0, -1, 0), 0, "below the world");
    assert_eq!(light_at(&chamber, 0, 256, 0), 0, "above the world");
    assert_eq!(
        light_at(&chamber, 64, 64, 0),
        0,
        "outside the loaded columns"
    );
}

// ---------------------------------------------------------------------------
// The setters.
// ---------------------------------------------------------------------------

#[test]
fn the_section_setters_pack_their_nibbles() {
    let mut section = Section::air(true);
    // Cells 0 and 1 share a byte in the stored array: the even cell's level
    // must reach the low nibble and the odd cell's the high one
    // (`unpack_nibble`'s layout).
    section.set_block_light(0, 0, 0, 12);
    section.set_block_light(1, 0, 0, 5);
    section.set_sky_light(0, 0, 0, 9);
    section.set_sky_light(1, 0, 0, 7);

    assert_eq!(section.block_light(0, 0, 0), 12, "the even cell");
    assert_eq!(section.block_light(1, 0, 0), 5, "the odd cell beside it");
    assert_eq!(section.sky_light(0, 0, 0), 9, "the sky's even cell");
    assert_eq!(section.sky_light(1, 0, 0), 7, "and its odd cell");
    assert_eq!(
        section.block_light(2, 0, 0),
        0,
        "and the next cell is untouched"
    );
    assert_eq!(
        section.sky_light(0, 1, 0),
        15,
        "as is the air's sky default"
    );

    // A section without a sky store: the sky setter cannot write.
    let mut dark = Section::air(false);
    dark.set_sky_light(0, 0, 0, 9);
    assert_eq!(dark.sky_light(0, 0, 0), 0, "no store, no write");
}

#[test]
#[should_panic(expected = "above 15")]
fn a_section_level_above_fifteen_panics() {
    let mut section = Section::air(true);
    section.set_block_light(0, 0, 0, 16);
}

#[test]
#[should_panic(expected = "above 15")]
fn a_chunk_level_above_fifteen_panics() {
    let mut chunk = Chunk::new(0, 0, true);
    chunk.set_sky_light(0, 0, 0, 16);
}

#[test]
fn the_chunk_setters_reach_the_section_and_no_further() {
    // A column that holds only its lowest section.
    let mut data = ColumnData::empty();
    data.sections[0] = Some(section_of(true, 0, 0, |_, _, _| AIR));
    data.mask = 1;
    let mut chunk = Chunk::new(0, 0, true);
    chunk.apply(&data, true, true);

    chunk.set_block_light(3, 4, 5, 7);
    chunk.set_sky_light(3, 4, 5, 6);
    assert_eq!(chunk.block_light_at(3, 4, 5), 7, "the held section's cell");
    assert_eq!(chunk.sky_light_at(3, 4, 5), 6);

    chunk.set_block_light(3, 20, 5, 7);
    chunk.set_sky_light(3, 20, 5, 6);
    assert_eq!(chunk.block_light_at(3, 20, 5), 0, "section 1 is not held");
    assert_eq!(
        chunk.sky_light_at(3, 20, 5),
        15,
        "so its cells keep the absent-section default"
    );
}

#[test]
fn the_chunk_setter_is_a_no_op_without_the_section() {
    let mut chunk = Chunk::new(0, 0, true);
    chunk.set_block_light(0, 0, 0, 5);
    chunk.set_sky_light(0, 0, 0, 5);
    assert_eq!(chunk.block_light_at(0, 0, 0), 0, "nowhere to store it");
    assert_eq!(chunk.sky_light_at(0, 0, 0), 15, "and nothing changed");
}
