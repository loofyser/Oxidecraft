//! The movement model's vectors and cases, driven against a synthetic view.
//!
//! The five exit vectors — `walk_speed`, `sprint_speed`, `sneak_speed`,
//! `jump_apex`, `terminal_velocity` — are the numbers the live acceptance
//! measures. Their literals come from the recorded research
//! (`docs/research/render-parity-survey.md` §5.4) and the decompiled 1.8.9
//! client under `refs/_src/MCP-919/`, whose file and line each derivation
//! cites. A literal the source refutes is corrected here, with the
//! refutation in its own doc comment.
//!
//! The fixtures are synthetic by rule: every block is a class the movement
//! model branches on — a full cube, a half-height step, a chest-high box, a
//! ladder, water and lava — and no fixture reads anything from the user's
//! store.

use std::collections::HashMap;

use oxide_game::input::{Intent, Key};
use oxide_game::physics::{CollisionView, Fluid, FluidKind, step};
use oxide_game::player::Player;
use oxide_world::collision::CollisionBox;

/// The collision classes the cases pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// A full cube: the collision box `[0,1]³`.
    Full,
    /// A bottom slab, a single stair step: the box `[0, 0.5]³`.
    Half,
    /// A ladder: no collision box, climbable.
    Ladder,
    /// A water source.
    Water,
    /// A lava source.
    Lava,
    /// A soul sand: the full footprint topped 0.875 high — a chest-high step
    /// the collision walk refuses (`BlockSoulSand.java:20-23`).
    SoulSand,
}

/// The synthetic view: a block map, keyed by cell.
struct TestView {
    blocks: HashMap<(i32, i32, i32), Block>,
}

impl TestView {
    /// An empty world.
    fn new() -> TestView {
        TestView {
            blocks: HashMap::new(),
        }
    }

    /// Places one block.
    fn put(&mut self, x: i32, y: i32, z: i32, block: Block) {
        self.blocks.insert((x, y, z), block);
    }

    /// A floor of full blocks whose top face is `y = 0`.
    fn ground(&mut self, x0: i32, x1: i32, z0: i32, z1: i32) {
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.put(x, -1, z, Block::Full);
            }
        }
    }

    /// A vertical column of one block class.
    fn column(&mut self, x: i32, z: i32, y0: i32, y1: i32, block: Block) {
        for y in y0..=y1 {
            self.put(x, y, z, block);
        }
    }
}

impl CollisionView for TestView {
    fn slipperiness(&self, _x: i32, _y: i32, _z: i32) -> f32 {
        // The default every block carries (`Block.java:291`); the fixtures are
        // grass-like, so the vectors' 0.6 × 0.91 friction chain holds.
        0.6
    }

    fn collision_boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<CollisionBox>) {
        let at = |b: CollisionBox| b.offset(f64::from(x), f64::from(y), f64::from(z));
        match self.blocks.get(&(x, y, z)) {
            Some(Block::Full) => out.push(at(CollisionBox::full())),
            // `BlockSlab.java:34`: `setBlockBounds(0.0F, 0.0F, 0.0F, 1.0F, 0.5F, 1.0F)`.
            Some(Block::Half) => out.push(at(CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]))),
            // `BlockSoulSand.java:20-23`: `(1.0F - 0.125F)`, so the top sits
            // at 0.875 — above the step probe's 0.6.
            Some(Block::SoulSand) => {
                out.push(at(CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.875, 1.0])))
            }
            _ => {}
        }
    }

    fn fluid(&self, x: i32, y: i32, z: i32) -> Option<Fluid> {
        match self.blocks.get(&(x, y, z)) {
            Some(Block::Water) => Some(Fluid {
                kind: FluidKind::Water,
                level: 0,
            }),
            Some(Block::Lava) => Some(Fluid {
                kind: FluidKind::Lava,
                level: 0,
            }),
            _ => None,
        }
    }

    fn climbable(&self, x: i32, y: i32, z: i32) -> bool {
        self.blocks.get(&(x, y, z)) == Some(&Block::Ladder)
    }
}

/// The intent with the given keys held.
fn held(keys: &[Key]) -> Intent {
    let mut intent = Intent::neutral();
    for key in keys {
        intent.apply_key(*key, true);
    }
    intent
}

/// A player standing on the floor at `(x, 0, z)`.
fn standing_at(x: f64, z: f64) -> Player {
    let mut player = Player::new();
    player.position = [x, 0.0, z];
    player.last_tick_position = [x, 0.0, z];
    player.on_ground = true;
    player
}

/// One tick's horizontal displacement.
fn tick_displacement(player: &mut Player, intent: &Intent, view: &TestView) -> [f64; 3] {
    let before = player.position;
    step(player, intent, view);
    [
        player.position[0] - before[0],
        player.position[1] - before[1],
        player.position[2] - before[2],
    ]
}

/// The walk vector: 0.21586 blocks a tick, 4.317 m/s.
///
/// Derived: the ground acceleration is `WALK_SPEED × 0.16277136 / (0.6 × 0.91)³`
/// = 0.09999998658895493 (`EntityLivingBase.java:1614-1622`), the move adds
/// `0.98 × f5` and the horizontal drag is 0.546 (`:1681-1682`); the recurrence
/// settles at `a / (1 - f4)` = 0.2158590·… blocks a tick.
#[test]
fn walk_speed() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 48);
    let mut player = standing_at(0.5, 0.5);
    let intent = held(&[Key::W]);
    for _ in 0..199 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((d[2] - 0.21586).abs() < 5e-5, "walk per tick: {}", d[2]);
    assert!(
        (d[2] * 20.0 - 4.317).abs() < 1e-3,
        "walk m/s: {}",
        d[2] * 20.0
    );
    assert!(player.on_ground);
}

/// The sprint vector: 0.28061 blocks a tick, 5.612 m/s.
///
/// The sprint modifier `"Sprinting speed boost"` multiplies the attribute by
/// 1.3 (`EntityLivingBase.java:57`, `EntityPlayer.java:1814-1817`): same
/// recurrence as the walk, `0.98 × 0.13 / (1 - 0.546)` = 0.2806168·…
#[test]
fn sprint_speed() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 60);
    let mut player = standing_at(0.5, 0.5);
    player.sprinting = true;
    let intent = held(&[Key::W, Key::ControlLeft]);
    for _ in 0..199 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((d[2] - 0.28061).abs() < 5e-5, "sprint per tick: {}", d[2]);
    assert!(
        (d[2] * 20.0 - 5.612).abs() < 2e-3,
        "sprint m/s: {}",
        d[2] * 20.0
    );
}

/// The sneak vector: 0.064758 blocks a tick, 1.295 m/s.
///
/// The sneak input scale is 0.3 — `MovementInputFromOptions.java:42-46`
/// multiplies both movement axes while sneak is held, so the walk vector
/// scales by the same 0.3 through the one recurrence.
#[test]
fn sneak_speed() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 20);
    let mut player = standing_at(0.5, 0.5);
    let intent = held(&[Key::W, Key::ShiftLeft]);
    assert_eq!(intent.forward, 0.3, "the sneak scale");
    for _ in 0..199 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((d[2] - 0.064758).abs() < 5e-5, "sneak per tick: {}", d[2]);
    assert!(
        (d[2] * 20.0 - 1.295).abs() < 1e-3,
        "sneak m/s: {}",
        d[2] * 20.0
    );
}

/// The jump vector: the standing jump's apex.
///
/// The corrected pin: 1.25220 is the apex of a source
/// that keeps the motion after the clamp; 1.8.9 clamps `motionY` to zero at
/// 0.005 *before* the move (`EntityLivingBase.java:1974-1987`) and the
/// achieved apex is 1.24919. The recurrence, `jump()`'s `0.42F` widened
/// (`:1567-1571`) and the gravity/drag order of `:1676-1680`, is settled by
/// the wiki's own jump page (1.2492 for 1.8) and the source lines above.
#[test]
fn jump_apex() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 4);
    let mut player = standing_at(0.5, 0.5);
    let jump = held(&[Key::Space]);
    let idle = Intent::neutral();
    let mut apex: f64 = 0.0;
    for tick in 0..25 {
        let intent = if tick == 0 { &jump } else { &idle };
        step(&mut player, intent, &view);
        apex = apex.max(player.position[1]);
    }
    assert!((apex - 1.24919).abs() < 1e-4, "jump apex: {apex}");
}

/// The terminal-velocity vector: 3.92 blocks a tick.
///
/// Gravity 0.08 and the vertical drag `0.9800000190734863`
/// (`EntityLivingBase.java:1676-1680`) settle the fall at 0.08 / 0.02 = 3.92.
#[test]
fn terminal_velocity() {
    let view = TestView::new();
    let mut player = Player::new();
    player.position = [0.5, 200.0, 0.5];
    player.on_ground = false;
    let intent = Intent::neutral();
    for _ in 0..600 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((-d[1] - 3.92).abs() < 1e-2, "terminal fall: {}", -d[1]);
}

/// A full block's side stops the walk dead (`Δ` below 1e-6 while held).
#[test]
fn walking_into_a_full_block_stops() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 4);
    for x in -1..=1 {
        view.column(x, 4, 0, 4, Block::Full);
    }
    let mut player = standing_at(0.5, 0.5);
    let intent = held(&[Key::W]);
    for _ in 0..200 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(d[2].abs() < 1e-6, "still moving: {}", d[2]);
    // The stop is the wall's face less the box's own half-width,
    // (double)0.3F (`Entity.java:375-383`).
    assert!(
        (player.position[2] - 3.7).abs() < 1e-6,
        "stopped at {}",
        player.position[2]
    );
    assert!(player.on_ground);
}

/// A half-high slab is climbed without jumping — the `stepHeight` probe
/// (`Entity.java:721-813`).
#[test]
fn a_half_block_step_up_needs_no_jump() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 30);
    for x in -1..=1 {
        view.put(x, 0, 4, Block::Half);
    }
    let mut player = standing_at(0.5, 0.5);
    let intent = held(&[Key::W]);
    let mut on_slab = false;
    for _ in 0..80 {
        step(&mut player, &intent, &view);
        if (4.0..5.0).contains(&player.position[2]) && (player.position[1] - 0.5).abs() < 1e-9 {
            on_slab = true;
        }
    }
    assert!(on_slab, "never stood on the slab");
    assert!(
        player.position[2] > 5.0,
        "did not cross: {}",
        player.position[2]
    );
    assert!(player.on_ground);
}

/// A full block is a wall to the walk and a step to a jump.
#[test]
fn a_full_block_needs_a_jump() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 30);
    for x in -1..=1 {
        view.put(x, 0, 4, Block::Full);
    }

    let mut walker = standing_at(0.5, 0.5);
    let walk = held(&[Key::W]);
    for _ in 0..80 {
        step(&mut walker, &walk, &view);
    }
    assert!(
        walker.position[2] < 3.7 + 1e-6,
        "walked through: {}",
        walker.position[2]
    );
    assert!(walker.position[1].abs() < 1e-9);

    let mut jumper = standing_at(0.5, 0.5);
    let jump_and_walk = held(&[Key::W, Key::Space]);
    let mut on_block = false;
    for _ in 0..120 {
        step(&mut jumper, &jump_and_walk, &view);
        if (4.0..5.0).contains(&jumper.position[2]) && (jumper.position[1] - 1.0).abs() < 1e-9 {
            on_block = true;
        }
    }
    assert!(on_block, "never stood on the block");
    assert!(
        jumper.position[2] > 5.0,
        "did not cross: {}",
        jumper.position[2]
    );
}

/// The sneaking edge protection (`Entity.java:626-694`): creeping toward a
/// platform edge for forty ticks never leaves the ground.
#[test]
fn sneaking_holds_the_player_at_a_platform_edge() {
    let mut view = TestView::new();
    view.ground(0, 10, 0, 10);
    let mut player = standing_at(5.5, 10.0);
    let sneak = held(&[Key::W, Key::ShiftLeft]);
    // Two settling ticks: the walk vector's own start is the tick after the
    // ground state is first refreshed.
    step(&mut player, &sneak, &view);
    step(&mut player, &sneak, &view);
    let mut grounded = true;
    for _ in 0..40 {
        step(&mut player, &sneak, &view);
        grounded &= player.on_ground;
    }
    assert!(grounded, "left the ground while sneaking at the edge");
    assert!(
        player.position[1].abs() < 1e-9,
        "fell to {}",
        player.position[1]
    );
    assert!(
        player.position[2] < 11.3,
        "crept past the edge: {}",
        player.position[2]
    );

    // The control: the same walk without sneak runs off the platform.
    let mut walker = standing_at(5.5, 10.0);
    let walk = held(&[Key::W]);
    let mut fell = false;
    for _ in 0..40 {
        step(&mut walker, &walk, &view);
        fell |= !walker.on_ground;
    }
    assert!(fell, "the control walk never left the platform");
}

/// Water drag: the terminal descent is 0.1 blocks a tick, `0.02 / (1 - 0.8)`
/// from `EntityLivingBase.java:1725-1728` — far below the air's 3.92.
#[test]
fn water_drag_slows_the_descent() {
    let mut view = TestView::new();
    for x in -2..=2 {
        for z in -2..=2 {
            view.column(x, z, -30, 8, Block::Water);
        }
    }
    let mut player = Player::new();
    player.position = [0.5, 6.0, 0.5];
    player.on_ground = false;
    let intent = Intent::neutral();
    for _ in 0..200 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(player.in_water, "not in water at {}", player.position[1]);
    assert!((-d[1] - 0.1).abs() < 1e-6, "water sink: {}", -d[1]);

    // The control: the same fall in air.
    let air = TestView::new();
    let mut free = Player::new();
    free.position = [0.5, 6.0, 0.5];
    free.on_ground = false;
    for _ in 0..200 {
        step(&mut free, &intent, &air);
    }
    let bare = tick_displacement(&mut free, &intent, &air);
    assert!(
        -bare[1] > -d[1] * 30.0,
        "air {} is not faster than water {}",
        -bare[1],
        -d[1]
    );
}

/// Lava drag: the terminal descent is 0.04 blocks a tick, `0.02 / (1 - 0.5)`
/// from `EntityLivingBase.java:1689-1692` — the liquid branch the fixtures
/// must enter for it to be falsifiable.
#[test]
fn lava_sinks_slower_than_water() {
    let mut view = TestView::new();
    for x in -2..=2 {
        for z in -2..=2 {
            view.column(x, z, -30, 8, Block::Lava);
        }
    }
    let mut player = Player::new();
    player.position = [0.5, 6.0, 0.5];
    player.on_ground = false;
    let intent = Intent::neutral();
    for _ in 0..200 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((-d[1] - 0.04).abs() < 1e-6, "lava sink: {}", -d[1]);
    assert!(-d[1] < 0.1, "lava is not slower than water");
}

/// The landing reset: the default `Block.onLanded`
/// (`block/Block.java:1116-1119`) zeroes the vertical motion when a fall is
/// caught (`Entity.java:851-854`), so standing still's `motionY` is the
/// gravity chain's `0.08 × 0.98`, and a step off a ledge falls its own first
/// tick at that same 0.0784 rather than an accumulated speed.
#[test]
fn landing_zeroes_the_vertical_motion() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 4);
    let mut player = standing_at(0.5, 0.5);
    let intent = Intent::neutral();
    for _ in 0..20 {
        step(&mut player, &intent, &view);
    }
    assert!(player.on_ground);
    assert!(
        (player.motion[1] + 0.08 * 0.9800000190734863).abs() < 1e-12,
        "standing motionY: {}",
        player.motion[1]
    );

    let mut walker = standing_at(0.5, 4.6);
    let walk = held(&[Key::W]);
    // Settle first: the very first tick of a fresh player refreshes the
    // ground state from a zero motion, which a live tick never does.
    step(&mut walker, &walk, &view);
    step(&mut walker, &walk, &view);
    let mut first_air = None;
    for _ in 0..15 {
        let was_ground = walker.on_ground;
        let before_y = walker.position[1];
        step(&mut walker, &walk, &view);
        if was_ground && !walker.on_ground && first_air.is_none() {
            first_air = Some(before_y - walker.position[1]);
        }
    }
    let fall = first_air.expect("never left the platform");
    assert!(
        (fall - 0.08 * 0.9800000190734863).abs() < 1e-9,
        "first air tick fell {fall}"
    );
}

/// A ladder column climbs at the derived rate while forward is held into it:
/// the boost's `motionY = 0.2` (`EntityLivingBase.java:1659-1662`), gravity
/// 0.08 and the drag 0.9800000190734863 give `(0.2 - 0.08) × 0.98` per tick.
#[test]
fn a_ladder_column_climbs() {
    let mut view = TestView::new();
    view.ground(-2, 2, 0, 1);
    for y in 0..=12 {
        for x in -2..=2 {
            view.put(x, y, 1, Block::Full);
            view.put(x, y, 0, Block::Ladder);
        }
    }
    let mut player = standing_at(0.5, 0.5);
    let intent = held(&[Key::W]);
    for _ in 0..20 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((d[1] - 0.1176).abs() < 1e-6, "climb: {}", d[1]);
    assert!(
        (d[1] - (0.2 - 0.08) * 0.9800000190734863).abs() < 1e-9,
        "climb does not match the derived chain: {}",
        d[1]
    );
    assert!(
        (player.position[2] - 0.7).abs() < 1e-6,
        "not pressed against the wall: {}",
        player.position[2]
    );
}

/// Level flight at the derived speed: the fly speed `0.05F`
/// (`PlayerCapabilities.java:23-24`) replaces the air acceleration and the
/// horizontal drag is 0.91 (`EntityLivingBase.java:1681-1682`):
/// `0.98 × 0.05 / (1 - 0.91)` = 0.544444·… blocks a tick.
#[test]
fn level_flight_holds_the_derived_speed() {
    let view = TestView::new();
    let mut player = Player::new();
    player.position = [0.5, 50.0, 0.5];
    player.flying = true;
    player.on_ground = false;
    let intent = held(&[Key::W]);
    for _ in 0..300 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((d[2] - 0.5444444).abs() < 1e-6, "fly per tick: {}", d[2]);
    assert!(
        (d[2] * 20.0 - 10.888888).abs() < 1e-4,
        "fly m/s: {}",
        d[2] * 20.0
    );
    assert!(
        (player.position[1] - 50.0).abs() < 1e-9,
        "height drifted: {}",
        player.position[1]
    );
}

/// Sneak-flying descends at the derived speed: the vertical input
/// `0.05F × 3.0F` (`EntityPlayerSP.java:848-859`) and the retention `d3 * 0.6`
/// (`EntityPlayer.java:1800`) settle at `2.5 × 0.15000000596046448` =
/// 0.37500001·… blocks a tick.
#[test]
fn sneak_flying_descends_at_the_derived_speed() {
    let view = TestView::new();
    let mut player = Player::new();
    player.position = [0.5, 50.0, 0.5];
    player.flying = true;
    player.on_ground = false;
    let intent = held(&[Key::ShiftLeft]);
    for _ in 0..200 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((-d[1] - 0.375).abs() < 1e-6, "fly descent: {}", -d[1]);
    assert!(
        (-d[1] - 2.5 * f64::from(0.05f32 * 3.0)).abs() < 1e-9,
        "descent does not match the derived chain: {}",
        -d[1]
    );
}

/// The swim-up nudge needs a horizontal collision: rising in a wall-less
/// one-deep pond never leaves the held jump's own chain. The source gates
/// `motionY = 0.30000001192092896D` on `isCollidedHorizontally`
/// (`EntityLivingBase.java:1730-1732`, water; `:1694-1696`, lava); without
/// the clause the nudge fires as soon as the offset probe finds open space
/// and the rise jumps from ~0.1 to 0.34.
#[test]
fn swim_up_needs_a_horizontal_collision() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 4);
    for x in -2..=2 {
        for z in -2..=1 {
            view.put(x, 0, z, Block::Water);
        }
    }
    let mut player = standing_at(0.5, 1.699999988079071);
    let intent = held(&[Key::Space]);
    let mut max_rise = 0.0f64;
    let mut max_motion = 0.0f64;
    for _ in 0..150 {
        let d = tick_displacement(&mut player, &intent, &view);
        max_rise = max_rise.max(d[1]);
        max_motion = max_motion.max(player.motion[1]);
    }
    // The held jump's chain: `motionY` settles at `0.8 × (s + 0.04) - 0.02`
    // ≈ 0.06 and each tick rises `s + 0.04` ≈ 0.1.
    assert!(max_rise > 0.05, "never rose: {max_rise}");
    assert!(max_motion > 0.04, "the chain never built: {max_motion}");
    assert!(max_rise < 0.11, "rise took the swim-up nudge: {max_rise}");
    assert!(
        max_motion < 0.11,
        "motion took the swim-up nudge: {max_motion}"
    );
}

/// The same pond with a wall at the edge: the collision clause holds, so the
/// nudge fires and the rise runs at `0.30000001192092896 + 0.03999999910593033`
/// a tick (`EntityLivingBase.java:1730-1732`).
#[test]
fn swim_up_kicks_against_a_wall() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 4);
    for x in -2..=2 {
        for z in -2..=1 {
            view.put(x, 0, z, Block::Water);
        }
    }
    for x in -2..=2 {
        view.column(x, 2, 0, 4, Block::Full);
    }
    let mut player = standing_at(0.5, 1.699999988079071);
    let intent = held(&[Key::W, Key::Space]);
    let mut max_rise = 0.0f64;
    for _ in 0..150 {
        let d = tick_displacement(&mut player, &intent, &view);
        max_rise = max_rise.max(d[1]);
    }
    assert!(
        (max_rise - 0.3400000110268593).abs() < 1e-6,
        "the kick chain never fired: {max_rise}"
    );
}

/// The lava branch carries the same clause (`EntityLivingBase.java:1694-1696`):
/// rising in wall-less lava never kicks either.
#[test]
fn swim_up_needs_a_horizontal_collision_in_lava() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 4);
    for x in -2..=2 {
        for z in -2..=1 {
            view.put(x, 0, z, Block::Lava);
        }
    }
    let mut player = standing_at(0.5, 1.699999988079071);
    let intent = held(&[Key::Space]);
    let mut max_rise = 0.0f64;
    for _ in 0..150 {
        let d = tick_displacement(&mut player, &intent, &view);
        max_rise = max_rise.max(d[1]);
    }
    // The lava chain: `0.5 × (s + 0.04) - 0.02`, a rise of 0.04 a tick.
    assert!(max_rise > 0.03, "never rose: {max_rise}");
    assert!(max_rise < 0.11, "rise took the swim-up nudge: {max_rise}");
}

/// A 0.875-high box is not stepped up: the walk stops at its face like a
/// wall's, while a jump gets the walker on top. `stepHeight = 0.6F`
/// (`EntityLivingBase.java:208`) makes the step probe blind to anything
/// above `y = 0.6` (`Entity.java:721-813`).
#[test]
fn a_soul_sand_box_needs_a_jump() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 30);
    for x in -1..=1 {
        view.put(x, 0, 4, Block::SoulSand);
    }

    let mut walker = standing_at(0.5, 0.5);
    let walk = held(&[Key::W]);
    for _ in 0..120 {
        step(&mut walker, &walk, &view);
    }
    assert!(
        (walker.position[2] - 3.7).abs() < 1e-6,
        "did not stop at the face: {}",
        walker.position[2]
    );
    assert!(
        walker.position[1].abs() < 1e-9,
        "stepped up: {}",
        walker.position[1]
    );

    let mut jumper = standing_at(0.5, 0.5);
    let jump_and_walk = held(&[Key::W, Key::Space]);
    let mut on_box = false;
    for _ in 0..120 {
        step(&mut jumper, &jump_and_walk, &view);
        if (4.0..5.0).contains(&jumper.position[2]) && (jumper.position[1] - 0.875).abs() < 1e-9 {
            on_box = true;
        }
    }
    assert!(on_box, "never stood on the box");
    assert!(
        jumper.position[2] > 5.0,
        "did not cross: {}",
        jumper.position[2]
    );
}

/// The airborne walk control: the air acceleration is `speedInAir = 0.02F`
/// (`EntityPlayer.java:164`) and the air friction the bare `0.91F`
/// (`EntityLivingBase.java:1610-1614`), with `a = 0.98 × 0.02F` widened
/// through `moveFlying` (`EntityLivingBase.java:1629`): the horizontal
/// motion settles at `a / (1 - 0.9100000262260437)` = 0.21777784·… a tick.
#[test]
fn air_control_matches_the_speed_in_air() {
    let view = TestView::new();
    let mut player = Player::new();
    player.position = [0.5, 300.0, 0.5];
    player.on_ground = false;
    let intent = held(&[Key::W]);
    for _ in 0..300 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(!player.on_ground);
    assert!(
        (d[2] - 0.21777784382111293).abs() < 1e-9,
        "air walk per tick: {}",
        d[2]
    );
}

/// The sprinting air control: `jumpMovementFactor` becomes
/// `0.02F + 0.02F × 0.3` widened (`EntityPlayer.java:627-631`), so the
/// airborne sprint settles at 0.28311117627138355 a tick.
#[test]
fn sprint_air_control_matches_the_boosted_speed() {
    let view = TestView::new();
    let mut player = Player::new();
    player.position = [0.5, 300.0, 0.5];
    player.sprinting = true;
    let intent = held(&[Key::W]);
    for _ in 0..300 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(
        (d[2] - 0.28311117627138355).abs() < 1e-9,
        "air sprint per tick: {}",
        d[2]
    );
}

/// The water acceleration drives the swim: `moveFlying(..., 0.02F)` with the
/// `0.8` drags (`EntityLivingBase.java:1703-1704`, `:1723-1728`) settles the
/// horizontal motion at 0.09800000700354615 a tick.
#[test]
fn water_acceleration_sets_the_horizontal_speed() {
    let mut view = TestView::new();
    for x in -2..=2 {
        for z in -2..=40 {
            view.column(x, z, -30, 8, Block::Water);
        }
    }
    let mut player = Player::new();
    player.position = [0.5, 6.0, 0.5];
    player.on_ground = false;
    let intent = held(&[Key::W]);
    for _ in 0..300 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(player.in_water, "left the water at {}", player.position[1]);
    assert!(
        (d[2] - 0.09800000700354615).abs() < 1e-9,
        "water swim per tick: {}",
        d[2]
    );
}

/// Jumping in deep water rises at the liquid-jump chain: the held jump adds
/// `motionY += 0.03999999910593033D` (`EntityLivingBase.java:1589-1596`)
/// before the move and the `0.8` drags after, so the rise settles just past
/// `5 × 0.03999999910593033 - 0.1` = 0.1 a tick — the widened drags put the
/// fixed point at 0.10000000148994004.
#[test]
fn jump_held_in_water_rises_at_the_liquid_jump_rate() {
    let mut view = TestView::new();
    for x in -2..=2 {
        for z in -2..=2 {
            view.column(x, z, -30, 30, Block::Water);
        }
    }
    let mut player = standing_at(0.5, 0.5);
    let intent = held(&[Key::Space]);
    for _ in 0..120 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(player.in_water);
    assert!(
        (d[1] - 0.10000000148994004).abs() < 1e-9,
        "liquid jump rise: {}",
        d[1]
    );
}

/// The lava acceleration: `moveFlying(..., 0.02F)` with the `0.5` drags
/// (`EntityLivingBase.java:1687-1692`) settles the horizontal motion at
/// 0.03920000046491623 a tick — `2a` against water's `5a`.
#[test]
fn lava_acceleration_sets_the_horizontal_speed() {
    let mut view = TestView::new();
    for x in -2..=2 {
        for z in -2..=40 {
            view.column(x, z, -30, 8, Block::Lava);
        }
    }
    let mut player = Player::new();
    player.position = [0.5, 6.0, 0.5];
    player.on_ground = false;
    let intent = held(&[Key::W]);
    for _ in 0..200 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!(
        (d[2] - 0.03920000046491623).abs() < 1e-9,
        "lava swim per tick: {}",
        d[2]
    );
}

/// Crossing a ladder strip clamps the horizontal motion to `(double)0.15F`
/// = 0.15000000596046448 (`EntityLivingBase.java:1639-1641`): a sprinted
/// crossing advances exactly the clamp each tick while the approach on open
/// ground keeps the sprint's ~0.28.
#[test]
fn the_ladder_clamp_caps_the_horizontal_speed() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 40);
    for z in 10..=30 {
        view.put(0, 0, z, Block::Ladder);
    }
    let mut player = standing_at(0.5, 0.5);
    player.sprinting = true;
    let intent = held(&[Key::W, Key::ControlLeft]);
    let mut clamped = 0;
    let mut fastest = 0.0f64;
    for _ in 0..200 {
        let before = player.position[2];
        let cell = before.floor() as i32;
        step(&mut player, &intent, &view);
        let d = player.position[2] - before;
        if (10..=30).contains(&cell) {
            assert!(
                (d - 0.15000000596046448).abs() < 1e-9,
                "a clamped tick moved {d}"
            );
            clamped += 1;
        } else if cell < 10 {
            fastest = fastest.max(d);
        }
    }
    assert!(
        clamped >= 30,
        "the run was clamped for only {clamped} ticks"
    );
    assert!(fastest > 0.25, "the sprint never ran: {fastest}");
}

/// The ladder's descent clamp: a fall touching a ladder column slides at
/// `-0.15` — the double literal (`EntityLivingBase.java:1644-1646`) — rather
/// than accelerating toward the terminal fall.
#[test]
fn the_ladder_descent_clamp_caps_the_slide() {
    let mut view = TestView::new();
    for y in 20..=41 {
        view.put(0, y, 0, Block::Ladder);
    }
    let mut player = Player::new();
    player.position = [0.5, 40.0, 0.5];
    let intent = Intent::neutral();
    for _ in 0..40 {
        step(&mut player, &intent, &view);
    }
    let d = tick_displacement(&mut player, &intent, &view);
    assert!((d[1] + 0.15).abs() < 1e-9, "ladder slide: {}", d[1]);
}

/// A sprinting jump shoves the player along the facing: `jump()` adds
/// `± 0.2F` to `motionX`/`motionZ` (`EntityLivingBase.java:1576-1581`), so
/// the jump tick advances 0.20000000298023224 past the settled sprint tick.
#[test]
fn a_sprint_jump_shoves_the_player_along_the_facing() {
    let mut view = TestView::new();
    view.ground(-4, 4, -4, 60);
    let mut player = standing_at(0.5, 0.5);
    player.sprinting = true;
    let run = held(&[Key::W, Key::ControlLeft]);
    for _ in 0..199 {
        step(&mut player, &run, &view);
    }
    let before = tick_displacement(&mut player, &run, &view);
    let jump = held(&[Key::W, Key::ControlLeft, Key::Space]);
    let d = tick_displacement(&mut player, &jump, &view);
    assert!(
        (d[2] - (before[2] + 0.20000000298023224)).abs() < 1e-9,
        "jump tick {} against the settled {}",
        d[2],
        before[2]
    );
    assert!(
        (d[1] - 0.41999998688697815).abs() < 1e-9,
        "jump rise: {}",
        d[1]
    );
}
