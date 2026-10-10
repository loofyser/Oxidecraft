//! The movement model against real worlds: the adapter and the behaviour
//! table carry the same rules the synthetic fixtures pin, built here through
//! the light tests' column path.
//!
//! Each scene builds one or more columns with [`ColumnData`] and applies them
//! to a [`World`], then steps the real [`WorldView`] over it: the collision
//! walk, the fluid branches, the sneak edge protection and the ladder boost
//! all read the table's shapes, not a fixture's.

use oxide_game::input::{Intent, Key};
use oxide_game::physics::step;
use oxide_game::player::Player;
use oxide_game::world_view::WorldView;
use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::world::World;

/// Air.
const AIR: u16 = 0;
/// Stone, id 1.
const STONE: u16 = 1 << 4;
/// Water, id 9.
const WATER: u16 = 9 << 4;
/// An oak stair, id 53, facing east on the bottom half (metadata 0).
const STAIR_EAST: u16 = 53 << 4;
/// A ladder, id 65, facing north (metadata 2): the 1/8 plate on the cell's
/// south edge, against the wall at z + 1.
const LADDER_NORTH: u16 = 65 << 4 | 2;

/// One section: every cell's value from `block_at(local x, local y, local
/// z)`, all light zero.
fn section_of(block_at: impl Fn(usize, usize, usize) -> u16) -> SectionData {
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
        block_light: Box::new([0; 2048]),
        sky_light: Some(Box::new([0; 2048])),
    }
}

/// A world of one column at chunk (0, 0): every cell's block value from
/// `block_at(x, y, z)` in world coordinates.
fn world_of(block_at: impl Fn(i32, i32, i32) -> u16) -> World {
    let mut data = ColumnData::empty();
    for sy in 0..SECTION_COUNT {
        data.sections[sy] = Some(section_of(|lx, ly, lz| {
            block_at(lx as i32, (sy * SECTION_SIZE + ly) as i32, lz as i32)
        }));
        data.mask |= 1u16 << sy;
    }
    let mut world = World::new(true);
    world.apply_column(0, 0, &data, true);
    world
}

/// A floor at y 63 across the whole column: its top surface is y 64, where
/// every scene's player starts.
fn floor(block_at: impl Fn(i32, i32, i32) -> u16) -> impl Fn(i32, i32, i32) -> u16 {
    move |x, y, z| if y == 63 { STONE } else { block_at(x, y, z) }
}

/// A player standing on the floor at (x, 64, z).
fn standing_at(x: f64, z: f64) -> Player {
    let mut player = Player::new();
    player.position = [x, 64.0, z];
    player.on_ground = true;
    player
}

/// The intent with every key of `keys` held.
fn held(keys: &[Key]) -> Intent {
    let mut intent = Intent::neutral();
    for key in keys {
        intent.apply_key(*key, true);
    }
    intent
}

/// The position change one step makes, without keeping it.
fn tick_displacement(player: &mut Player, intent: &Intent, view: &WorldView<'_>) -> [f64; 3] {
    let before = player.position;
    step(player, intent, view);
    [
        player.position[0] - before[0],
        player.position[1] - before[1],
        player.position[2] - before[2],
    ]
}

/// The walk climbs the stair's lower box without jumping: the probe rises
/// the walk onto the 0.5-high base, then onto the step, then crosses.
///
/// The brief's scene reaches for a half slab (id 44); that id is outside the
/// covered set, so its equal-height covered shape — the bottom stair's base
/// box, `[0, 0]..[1, 0.5]` (`BlockStairs.setBaseCollisionBounds`,
/// `BlockStairs.java:80-90`) — carries the check.
#[test]
fn a_walk_steps_up_the_lower_box_of_a_stair() {
    // The stair at (4, 64, 4) faces east: its base is the whole cell at half
    // height, its step the cell's east half at the top
    // (`BlockStairs.java:292-406`, the east arm at `:313-335`).
    let world = world_of(floor(|x, y, z| {
        if (x, y, z) == (4, 64, 4) {
            STAIR_EAST
        } else {
            AIR
        }
    }));
    let view = WorldView(&world);
    // Facing east is yaw -90: forward walks +X (`EntityLivingBase.java:1608-1682`).
    let mut player = standing_at(0.5, 4.5);
    player.yaw = -90.0;
    let intent = held(&[Key::W]);

    let mut on_the_base = false;
    let mut on_the_step = false;
    for _ in 0..200 {
        step(&mut player, &intent, &view);
        if (player.position[1] - 64.5).abs() < 1e-9 {
            on_the_base = true;
        }
        if (player.position[1] - 65.0).abs() < 1e-9 {
            on_the_step = true;
        }
    }
    assert!(on_the_base, "never stood on the stair's lower box");
    assert!(on_the_step, "never stepped onto the stair's top");
    assert!(
        player.position[0] > 5.0,
        "did not cross the stair: {}",
        player.position[0]
    );
}

/// A full block is a wall to the walk and a step to a jump: the walker stops
/// at the face, the jumper lands on top and crosses.
#[test]
fn a_full_block_needs_a_jump() {
    let world = world_of(floor(
        |x, y, z| {
            if (x, y, z) == (4, 64, 4) { STONE } else { AIR }
        },
    ));
    let view = WorldView(&world);

    let mut walker = standing_at(0.5, 4.5);
    walker.yaw = -90.0;
    let walk = held(&[Key::W]);
    for _ in 0..120 {
        step(&mut walker, &walk, &view);
    }
    assert!(
        walker.position[0] < 3.7 + 1e-6,
        "walked through the block: {}",
        walker.position[0]
    );
    assert!(
        (walker.position[1] - 64.0).abs() < 1e-9,
        "the walk rose: {}",
        walker.position[1]
    );

    let mut jumper = standing_at(0.5, 4.5);
    jumper.yaw = -90.0;
    let jump_and_walk = held(&[Key::W, Key::Space]);
    let mut on_block = false;
    for _ in 0..200 {
        step(&mut jumper, &jump_and_walk, &view);
        if (4.0..5.0).contains(&jumper.position[0]) && (jumper.position[1] - 65.0).abs() < 1e-9 {
            on_block = true;
        }
    }
    assert!(on_block, "never stood on the block");
    assert!(
        jumper.position[0] > 5.0,
        "did not cross: {}",
        jumper.position[0]
    );
}

/// The sneaking edge protection (`Entity.java:626-694`): creeping toward the
/// platform's edge never leaves the ground; the control walk runs off it.
#[test]
fn sneaking_holds_the_player_at_the_platform_edge() {
    // A platform: floor only over z 0..4 (the cells' tops span z 0..4).
    let world = world_of(|_x, y, z| {
        if y == 63 && (0..4).contains(&z) {
            STONE
        } else {
            AIR
        }
    });
    let view = WorldView(&world);

    let mut player = standing_at(4.5, 1.0);
    let sneak = held(&[Key::W, Key::ShiftLeft]);
    for _ in 0..2 {
        step(&mut player, &sneak, &view);
    }
    let mut grounded = true;
    for _ in 0..40 {
        step(&mut player, &sneak, &view);
        grounded &= player.on_ground;
    }
    assert!(grounded, "left the ground while sneaking at the edge");
    assert!(
        (player.position[1] - 64.0).abs() < 1e-9,
        "fell to {}",
        player.position[1]
    );
    assert!(
        player.position[2] < 4.0,
        "crept past the edge: {}",
        player.position[2]
    );

    // The control: the same walk without sneak runs off the platform.
    let mut walker = standing_at(4.5, 1.0);
    let walk = held(&[Key::W]);
    let mut fell = false;
    for _ in 0..40 {
        step(&mut walker, &walk, &view);
        fell |= !walker.on_ground;
    }
    assert!(fell, "the control walk never left the platform");
}

/// Water's terminal sink: 0.1 blocks a tick, `0.02 / (1 - 0.8)` from
/// `EntityLivingBase.java:1726-1737` — far below the air's fall.
#[test]
fn water_reaches_the_terminal_sink() {
    let world = world_of(|_, y, _| if y < 70 { WATER } else { AIR });
    let view = WorldView(&world);
    let mut player = Player::new();
    player.position = [4.5, 65.0, 4.5];
    player.on_ground = false;
    let intent = Intent::neutral();
    for _ in 0..200 {
        step(&mut player, &intent, &view);
    }
    assert!(player.in_water, "not in water at {}", player.position[1]);
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((-d[1] - 0.1).abs() < 1e-6, "water sink: {}", -d[1]);

    // The control: the same fall through air is far faster, and the flag is
    // off there.
    let air = world_of(|_, _, _| AIR);
    let bare_view = WorldView(&air);
    let mut free = Player::new();
    free.position = [4.5, 65.0, 4.5];
    free.on_ground = false;
    for _ in 0..200 {
        step(&mut free, &intent, &bare_view);
    }
    let bare = tick_displacement(&mut free, &intent, &bare_view);
    assert!(
        -bare[1] > -d[1] * 30.0,
        "air {} vs water {}",
        -bare[1],
        -d[1]
    );
    assert!(!free.in_water, "water reported with no water");
}

/// The `in_water` flag transitions once, on entry: false in the air above the
/// pool, true from the tick the box meets the water down.
#[test]
fn the_water_flag_transitions_on_entry() {
    let world = world_of(|_, y, _| if y < 70 { WATER } else { AIR });
    let view = WorldView(&world);
    let mut player = Player::new();
    player.position = [4.5, 71.5, 4.5];
    player.on_ground = false;
    let intent = Intent::neutral();

    let mut flags = Vec::new();
    for _ in 0..120 {
        step(&mut player, &intent, &view);
        flags.push(player.in_water);
    }
    assert_eq!(flags.first(), Some(&false), "started outside the water");
    assert_eq!(flags.last(), Some(&true), "never entered the water");
    let entries = flags.windows(2).filter(|pair| pair[0] != pair[1]).count();
    assert_eq!(entries, 1, "the flag flipped more than once: {flags:?}");
}

/// A ladder climbs: pressed into it, the horizontal collision feeds the 0.2
/// climb (`EntityLivingBase.java:1673-1682`), and the wall holds the walk.
#[test]
fn a_pressed_ladder_carries_the_player_up() {
    // A wall at z 5 with the ladder on its north face: the ladder at
    // (4, 64..68, 4) faces north, its plate at the cell's south edge.
    let world = world_of(floor(|x, y, z| {
        if x == 4 && (64..68).contains(&y) {
            if z == 5 {
                STONE
            } else if z == 4 {
                LADDER_NORTH
            } else {
                AIR
            }
        } else {
            AIR
        }
    }));
    let view = WorldView(&world);
    let mut player = standing_at(4.5, 4.2);
    let intent = held(&[Key::W]);
    for _ in 0..30 {
        step(&mut player, &intent, &view);
    }
    assert!(
        player.position[1] > 65.5,
        "the ladder did not carry the climb: {}",
        player.position[1]
    );
    assert!(
        player.position[2] < 4.7,
        "the wall did not hold the walk: {}",
        player.position[2]
    );
    assert!(!player.in_water, "no water in the scene");
}
