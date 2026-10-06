//! The player movement model: one 1/20 s step of the source's own rules.
//!
//! [`step`] is the local player's tick restated against an abstract view of
//! the world ([`CollisionView`]), so the model carries no world storage and
//! the tests drive it with synthetic fixtures. The order of operations is the
//! source's own and is followed line by line:
//!
//! * `EntityPlayerSP.onLivingUpdate` (`client/entity/EntityPlayerSP.java:715-909`)
//!   — the held keys, the sprint rule, the flight toggle, and the fly branch's
//!   vertical input (`:848-859`), all before the super call.
//! * `EntityLivingBase.onLivingUpdate` (`entity/EntityLivingBase.java:1947-2040`)
//!   — the jump cooldown, the rest-motion clamp (`:1974-1987`), the held jump
//!   (`:2007-2027`, `updateAITick` and `handleJumpLava` at `:1589-1597`) and
//!   the input damping (`:2031-2032`).
//! * `EntityLivingBase.moveEntityWithHeading` (`:1602-1682`) — the ground and
//!   air acceleration with the block's slipperiness, the ladder clamp, the
//!   gravity and the drag.
//! * `Entity.moveEntity` (`entity/Entity.java:598-854`) — the sneaking edge
//!   protection (`:626-694`), the per-axis collision resolution (`:696-719`),
//!   the step-up probe (`:721-813`), the collision flags and the ground state
//!   (`:815-822`) and the motion zeroing (`:841-849`).
//! * `EntityPlayer.moveEntityWithHeading` (`entity/player/EntityPlayer.java:1788-1809`)
//!   — the flight branch: the fly speed stands in for the air acceleration and
//!   the vertical motion keeps `d3 * 0.6` of the tick.
//!
//! `EntityPlayerSP.isServerWorld()` returns `true` (`EntityPlayerSP.java:565-568`),
//! so the local player takes the same branch of every server/client test as a
//! remote one: the full movement rules run and the client-side dead-reckoning
//! `motion *= 0.98` (`EntityLivingBase.java:1966-1973`) is not taken.
//!
//! # What this model does not carry
//!
//! * The cobweb drag (`Entity.java:611-621`) has no view concept yet.
//! * The sprint rule itself — engage and release (`EntityPlayerSP.java`:801-821)
//!   — lives with the input layer ([`crate::input::SprintTap`]); `step` reads
//!   `player.sprinting` and never sets it. The collision flag the release's
//!   `isCollidedHorizontally` clause reads rides out in [`StepOutcome`] for
//!   the session to hold across the tick.
//! * Fall damage (`Entity.updateFallState`), the riding branches, the
//!   entity-push and the walking stats are not modelled.
//! * The depth-strider scaling of the water branch
//!   (`EntityLivingBase.java:1705-1721`), the water-current push
//!   (`World.handleMaterialAcceleration`, `World.java:2120-2127`) and the
//!   unloaded-chunk motion guard (`EntityLivingBase.java:1664-1674`) are not
//!   carried.
//! * The source's sine table (`MathHelper.SIN_TABLE`, a 65536-entry lookup)
//!   is replaced by the platform's `sin`/`cos`: the table truncates each
//!   angle to one of its 65536 steps, so the difference is bounded by about
//!   one step (9.6e-5) at general angles and vanishes at the pinned vectors'
//!   yaw-0 and 45° inputs; no pinned vector depends on it.

use oxide_world::collision::CollisionBox;

use crate::input::Intent;
use crate::player::Player;

/// The player's box: half-width (double)0.3F.
///
/// `Entity.setPosition` (`Entity.java:375-383`) builds the box as
/// `posX ± (double)(this.width / 2.0F)` with `width = 0.6F`
/// (`EntityPlayer.java:580`), so the half is the float `0.3F` widened to
/// double — `0.30000001192092896`, not the double `0.3`.
const HALF_WIDTH: f64 = 0.30000001192092896;

/// The player's box height above the feet: (double)1.8F.
///
/// Same method: the box's top is `y + (double)this.height` with
/// `height = 1.8F` (`EntityPlayer.java:580`), so `1.7999999523162842`.
const BOX_HEIGHT: f64 = 1.7999999523162842;

/// How high a step the collision walk climbs without a jump:
/// `this.stepHeight = 0.6F` (`EntityLivingBase.java:208`), widened by
/// `y = (double)this.stepHeight` (`Entity.java:728`).
const STEP_HEIGHT: f64 = 0.6000000238418579;

/// Gravity: `this.motionY -= 0.08D` (`EntityLivingBase.java:1676-1678`).
const GRAVITY: f64 = 0.08;

/// Vertical drag: `this.motionY *= 0.9800000190734863D`
/// (`EntityLivingBase.java:1680`) — that literal is (double)0.98F.
const DRAG_Y: f64 = 0.9800000190734863;

/// The friction base every block's slipperiness multiplies:
/// `float f4 = 0.91F` (`EntityLivingBase.java:1610`, `:1630`).
const FRICTION_BASE: f32 = 0.91;

/// The ground acceleration's normalisation: `0.16277136F` is the cube of the
/// default ground friction `0.6F * 0.91F` = `0.546F` — about `0.162771336` —
/// so the division cancels it on default ground
/// (`EntityLivingBase.java:1617`).
const GROUND_ACCEL_FACTOR: f32 = 0.16277136;

/// The player's movement speed attribute base value
/// (`"player.generic.movementSpeed"`, `EntityPlayer.java:193`).
const WALK_SPEED: f64 = 0.10000000149011612;

/// The sprint modifier `"Sprinting speed boost"`: `+0.30000001192092896`,
/// operation 2 (`EntityLivingBase.java:57`), so `getAIMoveSpeed` multiplies
/// the base by 1.3 (`EntityPlayer.java:1814-1817`).
const SPRINT_BOOST: f64 = 0.30000001192092896;

/// The air acceleration: `this.speedInAir = 0.02F` (`EntityPlayer.java:164`).
const SPEED_IN_AIR: f32 = 0.02;

/// `EntityPlayer.onLivingUpdate` raises the air acceleration while sprinting:
/// `0.02F + 0.02F * 0.3D`, cast back to float (`EntityPlayer.java:627-631`).
const AIR_SPRINT_BOOST: f64 = 0.3;

/// The flight speed: `this.flySpeed = 0.05F` (`PlayerCapabilities.java:23-24`),
/// scaled by `3.0F` for the vertical input (`EntityPlayerSP.java:848-859`) and
/// by `2` for a sprinting flyer (`EntityPlayer.java:1798`).
const FLY_SPEED: f32 = 0.05;

/// Below this magnitude the source zeroes each motion component
/// (`EntityLivingBase.java:1974-1987`).
const MOTION_EPSILON: f64 = 0.005;

/// The sneaking edge protection's step: `for (d6 = 0.05D; ...)`
/// (`Entity.java:630-693`).
const SNEAK_EDGE_STEP: f64 = 0.05;

/// The ladder's horizontal clamp bounds: `float f6 = 0.15F` widened
/// (`EntityLivingBase.java:1639-1643`).
const LADDER_HORIZONTAL_MAX: f64 = 0.15000000596046448;

/// The ladder's descent clamp: the double literal `-0.15D`
/// (`EntityLivingBase.java:1645-1648`).
const LADDER_DESCENT_CLAMP: f64 = 0.15;

/// The climb a ladder gives on a horizontal collision: `motionY = 0.2D`
/// (`EntityLivingBase.java:1659-1662`).
const LADDER_CLIMB: f64 = 0.2;

/// The jump's vertical motion: `getJumpUpwardsMotion()` returns `0.42F`
/// (`EntityLivingBase.java:1559-1561`), widened by `jump()`'s
/// `this.motionY = (double)this.getJumpUpwardsMotion()` (`:1567-1571`).
const JUMP_MOTION: f64 = 0.41999998688697815;

/// The sprinting jump's forward shove: `\u00b1 0.2F` in the facing
/// (`EntityLivingBase.java:1576-1581`).
const SPRINT_JUMP_BOOST: f32 = 0.2;

/// Jumping while in a liquid: `motionY += 0.03999999910593033D`
/// (`EntityLivingBase.java:1589-1596`) — (double)0.04F.
const LIQUID_JUMP: f64 = 0.03999999910593033;

/// Water: `float f1 = 0.8F; float f2 = 0.02F` (`EntityLivingBase.java:1703-1704`),
/// `motionY *= 0.800000011920929D`, `motionY -= 0.02D` (`:1725-1728`).
const WATER_DRAG: f64 = 0.800000011920929;
/// Water's acceleration handed to `moveFlying` (`:1723`).
const WATER_ACCEL: f32 = 0.02;
/// Water's sink per tick (`:1728`).
const WATER_SINK: f64 = 0.02;

/// Lava: `motionX *= 0.5D` and `motionY -= 0.02D`
/// (`EntityLivingBase.java:1689-1692`), `moveFlying(..., 0.02F)` (`:1687`).
const LAVA_DRAG: f64 = 0.5;
/// Lava's acceleration handed to `moveFlying` (`:1687`).
const LAVA_ACCEL: f32 = 0.02;
/// Lava's sink per tick (`:1692`).
const LAVA_SINK: f64 = 0.02;

/// The swim-up nudge when a horizontal collision meets free liquid:
/// `motionY = 0.30000001192092896D` (`EntityLivingBase.java:1730-1732`, water;
/// `:1694-1696`, lava).
const SWIM_UP: f64 = 0.30000001192092896;

/// The fly branch's vertical retention: `this.motionY = d3 * 0.6D`
/// (`EntityPlayer.java:1800`).
const FLY_VERTICAL_RETAIN: f64 = 0.6;

/// The fluid a block holds, as the movement rules see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fluid {
    /// Which fluid.
    pub kind: FluidKind,
    /// The source's `LEVEL` metadata, 0-15: 0 is a source block and 8 or more
    /// marks a falling flow (`BlockLiquid.getLiquidHeightPercent`,
    /// `block/BlockLiquid.java:48-56`).
    pub level: u8,
}

/// The two fluids the movement rules know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    /// Water: the `0.8` drag, the `0.02` acceleration and the `0.02` sink.
    Water,
    /// Lava: the `0.5` drag, the `0.02` acceleration and the `0.02` sink.
    Lava,
}

/// What the movement model asks of the world.
///
/// One tick reads four things: the slipperiness of the block underfoot, the
/// collision boxes a cell reports, the fluid in a cell and whether a cell
/// climbs (`EntityLivingBase.isOnLadder`,
/// `entity/EntityLivingBase.java:1134-1141`). The world — or a test fixture —
/// answers without the model knowing how any of it is stored.
pub trait CollisionView {
    /// The block's slipperiness: `Block.slipperiness`, `0.6F` by default
    /// (`block/Block.java:147`, `:291`), `0.98F` for ice.
    fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32;

    /// Pushes the collision boxes of the block at the cell into `out`.
    ///
    /// `out` is appended to, not cleared: the caller hands a scratch vector
    /// per query. Blocks without a collision box (air, fluids, most
    /// plants) push nothing.
    fn collision_boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<CollisionBox>);

    /// The fluid at the cell, if the block holds one.
    fn fluid(&self, x: i32, y: i32, z: i32) -> Option<Fluid>;

    /// Whether the block climbs (`EntityLivingBase.isOnLadder`,
    /// `entity/EntityLivingBase.java:1134-1141`).
    fn climbable(&self, x: i32, y: i32, z: i32) -> bool;
}

/// What one [`step`] leaves for the rules that read it next.
///
/// The source assigns its `isCollidedHorizontally` field once, at the end of
/// `Entity.moveEntity` after the collision walk and the step-up selection
/// (`Entity.java`:816-821, the flag at `Entity.java`:818), and the next
/// tick's sprint release reads it (`EntityPlayerSP.java`:818-821). The step
/// surfaces the flag so the session can hold it for that read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StepOutcome {
    /// Whether the step's horizontal motion was cut by a collision — the
    /// source's `isCollidedHorizontally` (`Entity.java`:818).
    pub collided_horizontally: bool,
}

/// One 1/20 s step of the player movement model.
///
/// The caller supplies the tick's [`Intent`] — the held keys as the input
/// layer maps them, whose sneak scale of 0.3 already lives there
/// (`MovementInputFromOptions.java:42-46`) — and the world view. The step
/// owns the position, the motion, the ground state, the sneak flag and the
/// in-water flag; the sprint state machine and the flight toggle stay with
/// the input layer, and `step` only reads `player.sprinting` and
/// `player.flying`. The step returns the move's [`StepOutcome`] — the
/// source's `isCollidedHorizontally` (`Entity.java`:818) — for the sprint
/// release of the following tick to read.
pub fn step(player: &mut Player, input: &Intent, view: &dyn CollisionView) -> StepOutcome {
    // The held sneak state the collision walk reads; `EntityPlayerSP` sets it
    // from the input before its super call (`EntityPlayerSP.java:723-758`).
    player.sneaking = input.sneak;

    // `Entity.onEntityUpdate` (`Entity.java:411-484`) refreshes the fluid
    // state before the living update reads it: `handleWaterMovement`'s own
    // box (`:1111-1130`) and `isInLava`'s (`:1216-1219`).
    player.in_water = in_water(player, view);
    let in_lava = in_lava(player, view);

    // `EntityLivingBase.onLivingUpdate:1949-1952` — the jump cooldown.
    if player.jump_ticks > 0 {
        player.jump_ticks -= 1;
    }

    // `EntityPlayerSP.onLivingUpdate:848-859` — the fly branch's vertical
    // input, added before the tick's motion clamp runs.
    if player.flying {
        let fly_step = f64::from(FLY_SPEED * 3.0);
        if input.sneak {
            player.motion[1] -= fly_step;
        }
        if input.jump {
            player.motion[1] += fly_step;
        }
    }

    // `EntityLivingBase.onLivingUpdate:1974-1987` — a resting axis rests.
    for axis in &mut player.motion {
        if axis.abs() < MOTION_EPSILON {
            *axis = 0.0;
        }
    }

    // `EntityPlayer.getAIMoveSpeed` (`:1814-1817`) widens the attribute with
    // its modifier and casts back to float.
    let move_speed = (WALK_SPEED
        * if player.sprinting {
            1.0 + SPRINT_BOOST
        } else {
            1.0
        }) as f32;

    // `EntityPlayer.onLivingUpdate:627-631` — the sprinting air acceleration,
    // in the source's own float/double mix.
    let mut jump_movement_factor = SPEED_IN_AIR;
    if player.sprinting {
        jump_movement_factor =
            (f64::from(jump_movement_factor) + f64::from(SPEED_IN_AIR) * AIR_SPRINT_BOOST) as f32;
    }

    // `EntityLivingBase.onLivingUpdate:2007-2027` — the held jump. Water's
    // `updateAITick` (`:1589-1591`) and lava's `handleJumpLava` (`:1593-1596`)
    // add the same (double)0.04F.
    if input.jump {
        if player.in_water || in_lava {
            player.motion[1] += LIQUID_JUMP;
        } else if player.on_ground && player.jump_ticks == 0 {
            jump(player);
            player.jump_ticks = 10;
        }
    } else {
        player.jump_ticks = 0;
    }

    // `EntityLivingBase.onLivingUpdate:2031-2034` — the input is damped once,
    // then the heading rules run.
    let strafe = input.strafe * 0.98;
    let forward = input.forward * 0.98;

    let collided_horizontally = if player.flying {
        // `EntityPlayer.moveEntityWithHeading:1794-1806` — the fly speed
        // stands in for the air acceleration, and the vertical motion is
        // restored to `d3 * 0.6` of the tick.
        let d3 = player.motion[1];
        let fly_factor = FLY_SPEED * if player.sprinting { 2.0 } else { 1.0 };
        let flag = travel(
            player, strafe, forward, view, move_speed, fly_factor, in_lava,
        );
        player.motion[1] = d3 * FLY_VERTICAL_RETAIN;
        flag
    } else {
        travel(
            player,
            strafe,
            forward,
            view,
            move_speed,
            jump_movement_factor,
            in_lava,
        )
    };

    StepOutcome {
        collided_horizontally,
    }
}

/// The player's collision box at the current position — the source's own
/// extents (see [`HALF_WIDTH`] and [`BOX_HEIGHT`]).
fn player_box(player: &Player) -> CollisionBox {
    let [x, y, z] = player.position;
    CollisionBox::of(
        [x - HALF_WIDTH, y, z - HALF_WIDTH],
        [x + HALF_WIDTH, y + BOX_HEIGHT, z + HALF_WIDTH],
    )
}

/// `EntityLivingBase.jump` (`EntityLivingBase.java:1567-1584`).
fn jump(player: &mut Player) {
    player.motion[1] = JUMP_MOTION;
    if player.sprinting {
        // The sprinting jump shoves the player along the facing; the source
        // works in floats with the yaw in degrees (`:1576-1581`).
        let f = player.yaw * 0.017453292;
        player.motion[0] -= f64::from(f.sin() * SPRINT_JUMP_BOOST);
        player.motion[2] += f64::from(f.cos() * SPRINT_JUMP_BOOST);
    }
}

/// `EntityLivingBase.moveEntityWithHeading` (`:1602-1682`), with the player's
/// flight already resolved. The three branches are the source's own:
/// `!isInWater() || flying` selects the land rules, then `!isInLava()` splits
/// lava off (`:1606-1733`); each branch returns the `isCollidedHorizontally`
/// flag its move produced.
fn travel(
    player: &mut Player,
    strafe: f32,
    forward: f32,
    view: &dyn CollisionView,
    move_speed: f32,
    jump_movement_factor: f32,
    in_lava: bool,
) -> bool {
    if player.in_water && !player.flying {
        water(player, strafe, forward, view)
    } else if in_lava && !player.flying {
        lava(player, strafe, forward, view)
    } else {
        land_or_air(
            player,
            strafe,
            forward,
            view,
            move_speed,
            jump_movement_factor,
        )
    }
}

/// The friction the move is scaled by: the block underfoot's slipperiness
/// times the source's `0.91F`, or `0.91F` itself in the air
/// (`EntityLivingBase.java:1610-1614`, `:1630-1634`).
fn ground_friction(player: &Player, view: &dyn CollisionView) -> f32 {
    if player.on_ground {
        let below = [
            player.position[0].floor() as i32,
            player_box(player).min[1].floor() as i32 - 1,
            player.position[2].floor() as i32,
        ];
        view.slipperiness(below[0], below[1], below[2]) * FRICTION_BASE
    } else {
        FRICTION_BASE
    }
}

/// The land and air rules (`EntityLivingBase.java`:1608-1682); returns the
/// move's `isCollidedHorizontally` flag.
fn land_or_air(
    player: &mut Player,
    strafe: f32,
    forward: f32,
    view: &dyn CollisionView,
    move_speed: f32,
    jump_movement_factor: f32,
) -> bool {
    let mut f4 = ground_friction(player, view);
    let f = GROUND_ACCEL_FACTOR / (f4 * f4 * f4);
    let f5 = if player.on_ground {
        move_speed * f
    } else {
        jump_movement_factor
    };
    move_flying(player, strafe, forward, f5);
    f4 = ground_friction(player, view);

    if on_ladder(player, view) {
        player.motion[0] = player.motion[0].clamp(-LADDER_HORIZONTAL_MAX, LADDER_HORIZONTAL_MAX);
        player.motion[2] = player.motion[2].clamp(-LADDER_HORIZONTAL_MAX, LADDER_HORIZONTAL_MAX);
        if player.motion[1] < -LADDER_DESCENT_CLAMP {
            player.motion[1] = -LADDER_DESCENT_CLAMP;
        }
        if player.sneaking && player.motion[1] < 0.0 {
            player.motion[1] = 0.0;
        }
    }

    let collided_horizontally = move_entity(player, view);
    if collided_horizontally && on_ladder(player, view) {
        player.motion[1] = LADDER_CLIMB;
    }

    player.motion[1] -= GRAVITY;
    player.motion[1] *= DRAG_Y;
    player.motion[0] *= f64::from(f4);
    player.motion[2] *= f64::from(f4);

    collided_horizontally
}

/// Water (`EntityLivingBase.java`:1700-1734); returns the move's
/// `isCollidedHorizontally` flag.
fn water(player: &mut Player, strafe: f32, forward: f32, view: &dyn CollisionView) -> bool {
    let d0 = player.position[1];
    move_flying(player, strafe, forward, WATER_ACCEL);
    let collided_horizontally = move_entity(player, view);
    player.motion[0] *= WATER_DRAG;
    player.motion[1] *= WATER_DRAG;
    player.motion[2] *= WATER_DRAG;
    player.motion[1] -= WATER_SINK;
    if collided_horizontally
        && is_offset_position_in_liquid(
            player,
            view,
            player.motion[0],
            player.motion[1] + 0.6000000238418579 - player.position[1] + d0,
            player.motion[2],
        )
    {
        player.motion[1] = SWIM_UP;
    }

    collided_horizontally
}

/// Lava (`EntityLivingBase.java`:1684-1698); returns the move's
/// `isCollidedHorizontally` flag.
fn lava(player: &mut Player, strafe: f32, forward: f32, view: &dyn CollisionView) -> bool {
    let d1 = player.position[1];
    move_flying(player, strafe, forward, LAVA_ACCEL);
    let collided_horizontally = move_entity(player, view);
    player.motion[0] *= LAVA_DRAG;
    player.motion[1] *= LAVA_DRAG;
    player.motion[2] *= LAVA_DRAG;
    player.motion[1] -= LAVA_SINK;
    if collided_horizontally
        && is_offset_position_in_liquid(
            player,
            view,
            player.motion[0],
            player.motion[1] + 0.6000000238418579 - player.position[1] + d1,
            player.motion[2],
        )
    {
        player.motion[1] = SWIM_UP;
    }

    collided_horizontally
}

/// `Entity.moveFlying` (`Entity.java:1224-1245`): the input vector is
/// normalised, scaled by the friction, and added to the motion along the yaw's
/// facing. All of it is float arithmetic; only the add is doubled.
fn move_flying(player: &mut Player, strafe: f32, forward: f32, friction: f32) {
    let mut f = strafe * strafe + forward * forward;
    if f >= 1.0e-4 {
        f = f.sqrt();
        if f < 1.0 {
            f = 1.0;
        }
        f = friction / f;
        let strafe = strafe * f;
        let forward = forward * f;
        let f1 = (player.yaw * std::f32::consts::PI / 180.0).sin();
        let f2 = (player.yaw * std::f32::consts::PI / 180.0).cos();
        player.motion[0] += f64::from(strafe * f2 - forward * f1);
        player.motion[2] += f64::from(forward * f2 + strafe * f1);
    }
}

/// `Entity.moveEntity` (`Entity.java:598-854`), less the `noClip` and cobweb
/// branches the M3 surface cannot name. Returns the `isCollidedHorizontally`
/// flag the liquid branches and the ladder boost read after the move — and
/// the flag [`step`] surfaces in [`StepOutcome`].
fn move_entity(player: &mut Player, view: &dyn CollisionView) -> bool {
    let mut x = player.motion[0];
    let mut y = player.motion[1];
    let mut z = player.motion[2];
    let mut d3 = x;
    let d4 = y;
    let mut d5 = z;

    // The sneaking edge protection (`:626-694`): a sneaking player on the
    // ground never steps where the block below is missing. Each axis is
    // pulled back by 0.05 at a time while the position under the box is open
    // (`:630-693`).
    if player.on_ground && player.sneaking {
        let d6 = SNEAK_EDGE_STEP;
        while x != 0.0 && colliding_boxes(view, player_box(player).offset(x, -1.0, 0.0)).is_empty()
        {
            if x < d6 && x >= -d6 {
                x = 0.0;
            } else if x > 0.0 {
                x -= d6;
            } else {
                x += d6;
            }
            d3 = x;
        }
        while z != 0.0 && colliding_boxes(view, player_box(player).offset(0.0, -1.0, z)).is_empty()
        {
            if z < d6 && z >= -d6 {
                z = 0.0;
            } else if z > 0.0 {
                z -= d6;
            } else {
                z += d6;
            }
            d5 = z;
        }
        while x != 0.0
            && z != 0.0
            && colliding_boxes(view, player_box(player).offset(x, -1.0, z)).is_empty()
        {
            if x < d6 && x >= -d6 {
                x = 0.0;
            } else if x > 0.0 {
                x -= d6;
            } else {
                x += d6;
            }
            d3 = x;
            if z < d6 && z >= -d6 {
                z = 0.0;
            } else if z > 0.0 {
                z -= d6;
            } else {
                z += d6;
            }
        }
    }

    let boxes = colliding_boxes(view, add_coord(player_box(player), x, y, z));
    let mut bb = player_box(player);

    for b in &boxes {
        y = calculate_y_offset(*b, bb, y);
    }
    bb = bb.offset(0.0, y, 0.0);
    let flag1 = player.on_ground || (d4 != y && d4 < 0.0);

    for b in &boxes {
        x = calculate_x_offset(*b, bb, x);
    }
    bb = bb.offset(x, 0.0, 0.0);

    for b in &boxes {
        z = calculate_z_offset(*b, bb, z);
    }
    bb = bb.offset(0.0, 0.0, z);

    if STEP_HEIGHT > 0.0 && flag1 && (d3 != x || d5 != z) {
        // The step-up probe (`:721-813`): back to the pre-move box, ask how
        // high the requested move can rise, resolve it twice — X first and Y
        // first — and keep the candidate that travelled further horizontally.
        let d11 = x;
        let d7 = y;
        let d8 = z;
        let resolved = bb;
        bb = player_box(player);
        y = STEP_HEIGHT;
        let step_boxes = colliding_boxes(view, add_coord(bb, d3, y, d5));
        let a4 = bb;
        let a5 = add_coord(a4, d3, 0.0, d5);
        let mut d9 = y;
        for b in &step_boxes {
            d9 = calculate_y_offset(*b, a5, d9);
        }
        let mut a4 = a4.offset(0.0, d9, 0.0);
        let mut d15 = d3;
        for b in &step_boxes {
            d15 = calculate_x_offset(*b, a4, d15);
        }
        a4 = a4.offset(d15, 0.0, 0.0);
        let mut d16 = d5;
        for b in &step_boxes {
            d16 = calculate_z_offset(*b, a4, d16);
        }
        a4 = a4.offset(0.0, 0.0, d16);

        let mut a14 = bb;
        let mut d17 = y;
        for b in &step_boxes {
            d17 = calculate_y_offset(*b, a14, d17);
        }
        a14 = a14.offset(0.0, d17, 0.0);
        let mut d18 = d3;
        for b in &step_boxes {
            d18 = calculate_x_offset(*b, a14, d18);
        }
        a14 = a14.offset(d18, 0.0, 0.0);
        let mut d19 = d5;
        for b in &step_boxes {
            d19 = calculate_z_offset(*b, a14, d19);
        }
        a14 = a14.offset(0.0, 0.0, d19);

        let d20 = d15 * d15 + d16 * d16;
        let d10 = d18 * d18 + d19 * d19;
        if d20 > d10 {
            x = d15;
            z = d16;
            y = -d9;
            bb = a4;
        } else {
            x = d18;
            z = d19;
            y = -d17;
            bb = a14;
        }
        for b in &step_boxes {
            y = calculate_y_offset(*b, bb, y);
        }
        bb = bb.offset(0.0, y, 0.0);

        if d11 * d11 + d8 * d8 >= x * x + z * z {
            x = d11;
            y = d7;
            z = d8;
            bb = resolved;
        }
    }

    player.position = [
        (bb.min[0] + bb.max[0]) / 2.0,
        bb.min[1],
        (bb.min[2] + bb.max[2]) / 2.0,
    ];

    let is_collided_horizontally = d3 != x || d5 != z;
    let is_collided_vertically = d4 != y;
    player.on_ground = is_collided_vertically && d4 < 0.0;

    if d3 != x {
        player.motion[0] = 0.0;
    }
    if d5 != z {
        player.motion[2] = 0.0;
    }
    if d4 != y {
        // The block that was landed on gets its say (`Entity.java:851-854`);
        // the default `Block.onLanded` (`Block.java:1116-1119`) zeroes the
        // vertical motion — this is the reset behind the standing-on-ground
        // `motionY` of 0.0784 — and slime's bounce is the one override
        // (`BlockSlime.java:44-48`), which the behaviour table will carry.
        player.motion[1] = 0.0;
    }

    is_collided_horizontally
}

/// `World.getCollidingBoundingBoxes` (`World.java:1262-1307`): every block
/// box the query touches, in the source's own cell order.
fn colliding_boxes(view: &dyn CollisionView, query: CollisionBox) -> Vec<CollisionBox> {
    let mut out = Vec::new();
    let mut cell = Vec::new();
    let x0 = query.min[0].floor() as i32;
    let x1 = (query.max[0] + 1.0).floor() as i32;
    let y0 = query.min[1].floor() as i32;
    let y1 = (query.max[1] + 1.0).floor() as i32;
    let z0 = query.min[2].floor() as i32;
    let z1 = (query.max[2] + 1.0).floor() as i32;
    for x in x0..x1 {
        for z in z0..z1 {
            for y in (y0 - 1)..y1 {
                cell.clear();
                view.collision_boxes(x, y, z, &mut cell);
                for b in &cell {
                    if intersects(query, *b) {
                        out.push(*b);
                    }
                }
            }
        }
    }
    out
}

/// `AxisAlignedBB.addCoord` (`AxisAlignedBB.java:35-50`) — the box extended
/// in the direction of the vector.
fn add_coord(b: CollisionBox, x: f64, y: f64, z: f64) -> CollisionBox {
    let mut min = b.min;
    let mut max = b.max;
    if x < 0.0 {
        min[0] += x;
    } else if x > 0.0 {
        max[0] += x;
    }
    if y < 0.0 {
        min[1] += y;
    } else if y > 0.0 {
        max[1] += y;
    }
    if z < 0.0 {
        min[2] += z;
    } else if z > 0.0 {
        max[2] += z;
    }
    CollisionBox { min, max }
}

/// `AxisAlignedBB.expand` (`AxisAlignedBB.java:78-88`) — the box grown by the
/// amounts on both sides of each axis.
fn expand(b: CollisionBox, x: f64, y: f64, z: f64) -> CollisionBox {
    CollisionBox {
        min: [b.min[0] - x, b.min[1] - y, b.min[2] - z],
        max: [b.max[0] + x, b.max[1] + y, b.max[2] + z],
    }
}

/// `AxisAlignedBB.contract` (`AxisAlignedBB.java:260-263`) — the box shrunk
/// by the amounts (`expand` with negated amounts).
fn contract(b: CollisionBox, x: f64, y: f64, z: f64) -> CollisionBox {
    expand(b, -x, -y, -z)
}

/// `AxisAlignedBB.intersectsWith` (`AxisAlignedBB.java:233-243`): the strict
/// overlap of the three axes.
fn intersects(a: CollisionBox, b: CollisionBox) -> bool {
    b.max[0] > a.min[0]
        && b.min[0] < a.max[0]
        && b.max[1] > a.min[1]
        && b.min[1] < a.max[1]
        && b.max[2] > a.min[2]
        && b.min[2] < a.max[2]
}

/// `AxisAlignedBB.calculateXOffset` (`AxisAlignedBB.java:127-155`): how far
/// the box may move along X before it would touch the block, given it already
/// overlaps on the other two axes.
fn calculate_x_offset(block: CollisionBox, entity: CollisionBox, offset_x: f64) -> f64 {
    if entity.max[1] > block.min[1]
        && entity.min[1] < block.max[1]
        && entity.max[2] > block.min[2]
        && entity.min[2] < block.max[2]
    {
        if offset_x > 0.0 && entity.max[0] <= block.min[0] {
            let d = block.min[0] - entity.max[0];
            if d < offset_x {
                return d;
            }
        } else if offset_x < 0.0 && entity.min[0] >= block.max[0] {
            let d = block.max[0] - entity.min[0];
            if d > offset_x {
                return d;
            }
        }
    }
    offset_x
}

/// `AxisAlignedBB.calculateYOffset` (`AxisAlignedBB.java:163-193`).
fn calculate_y_offset(block: CollisionBox, entity: CollisionBox, offset_y: f64) -> f64 {
    if entity.max[0] > block.min[0]
        && entity.min[0] < block.max[0]
        && entity.max[2] > block.min[2]
        && entity.min[2] < block.max[2]
    {
        if offset_y > 0.0 && entity.max[1] <= block.min[1] {
            let d = block.min[1] - entity.max[1];
            if d < offset_y {
                return d;
            }
        } else if offset_y < 0.0 && entity.min[1] >= block.max[1] {
            let d = block.max[1] - entity.min[1];
            if d > offset_y {
                return d;
            }
        }
    }
    offset_y
}

/// `AxisAlignedBB.calculateZOffset` (`AxisAlignedBB.java:199-229`).
fn calculate_z_offset(block: CollisionBox, entity: CollisionBox, offset_z: f64) -> f64 {
    if entity.max[0] > block.min[0]
        && entity.min[0] < block.max[0]
        && entity.max[1] > block.min[1]
        && entity.min[1] < block.max[1]
    {
        if offset_z > 0.0 && entity.max[2] <= block.min[2] {
            let d = block.min[2] - entity.max[2];
            if d < offset_z {
                return d;
            }
        } else if offset_z < 0.0 && entity.min[2] >= block.max[2] {
            let d = block.max[2] - entity.min[2];
            if d > offset_z {
                return d;
            }
        }
    }
    offset_z
}

/// `EntityLivingBase.isOnLadder` (`EntityLivingBase.java:1134-1141`): the
/// block at the entity's own feet.
fn on_ladder(player: &Player, view: &dyn CollisionView) -> bool {
    let bb = player_box(player);
    view.climbable(
        player.position[0].floor() as i32,
        bb.min[1].floor() as i32,
        player.position[2].floor() as i32,
    )
}

/// `Entity.handleWaterMovement` (`Entity.java:1111-1130`): the feet box
/// shrunk 0.4 at the top and bottom — the source's `expand` with a negative
/// amount gives the mid-band `[feet + 0.4, feet + 1.4]` — and contracted by
/// 0.001.
fn in_water(player: &Player, view: &dyn CollisionView) -> bool {
    let bb = contract(
        expand(player_box(player), 0.0, -0.4000000059604645, 0.0),
        0.001,
        0.001,
        0.001,
    );
    liquid_in_bb(view, bb, FluidKind::Water)
}

/// `Entity.isInLava` (`Entity.java:1216-1219`): the box grown by −0.1 in X
/// and Z and −0.4 in Y — the source's expand with negative amounts shrinks
/// the footprint and takes the mid-body slice.
fn in_lava(player: &Player, view: &dyn CollisionView) -> bool {
    let bb = expand(
        player_box(player),
        -0.10000000149011612,
        -0.4000000059604645,
        -0.10000000149011612,
    );
    material_in_bb(view, bb, FluidKind::Lava)
}

/// `World.isMaterialInBB` (`World.java:2136-2158`): any cell of the box whose
/// block carries the material.
fn material_in_bb(view: &dyn CollisionView, bb: CollisionBox, kind: FluidKind) -> bool {
    for x in (bb.min[0].floor() as i32)..((bb.max[0] + 1.0).floor() as i32) {
        for y in (bb.min[1].floor() as i32)..((bb.max[1] + 1.0).floor() as i32) {
            for z in (bb.min[2].floor() as i32)..((bb.max[2] + 1.0).floor() as i32) {
                if view.fluid(x, y, z).is_some_and(|f| f.kind == kind) {
                    return true;
                }
            }
        }
    }
    false
}

/// `World.handleMaterialAcceleration`'s flag (`World.java:2077-2124`): a
/// block of the fluid inside the box whose surface the box's top reaches —
/// the source tests `floor(bb.maxY + 1) >= y + 1 - heightPercent(level)`.
fn liquid_in_bb(view: &dyn CollisionView, bb: CollisionBox, kind: FluidKind) -> bool {
    let top = (bb.max[1] + 1.0).floor();
    for x in (bb.min[0].floor() as i32)..((bb.max[0] + 1.0).floor() as i32) {
        for y in (bb.min[1].floor() as i32)..((bb.max[1] + 1.0).floor() as i32) {
            for z in (bb.min[2].floor() as i32)..((bb.max[2] + 1.0).floor() as i32) {
                if let Some(fluid) = view.fluid(x, y, z) {
                    if fluid.kind == kind && top >= liquid_surface(y, fluid.level) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `BlockLiquid.getLiquidHeightPercent` (`BlockLiquid.java:48-56`) and the
/// surface arithmetic of `handleMaterialAcceleration` (`World.java:2106-2110`):
/// the float `(level + 1) / 9` is subtracted from the cell's top edge, 8 or
/// more reading as a source block.
fn liquid_surface(y: i32, level: u8) -> f64 {
    let level = if level >= 8 { 0 } else { level };
    let percent = (f32::from(level) + 1.0) / 9.0;
    f64::from((y as f32 + 1.0) - percent)
}

/// `World.isAnyLiquid` (`World.java:2012-2030`): any liquid at all in the
/// cells the box covers.
fn any_liquid_in_bb(view: &dyn CollisionView, bb: CollisionBox) -> bool {
    for x in (bb.min[0].floor() as i32)..=(bb.max[0].floor() as i32) {
        for y in (bb.min[1].floor() as i32)..=(bb.max[1].floor() as i32) {
            for z in (bb.min[2].floor() as i32)..=(bb.max[2].floor() as i32) {
                if view.fluid(x, y, z).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

/// `Entity.isOffsetPositionInLiquid` (`Entity.java:581-592`): the box moved
/// by the offset touches no collision box and no liquid — the free space the
/// swim-up nudge needs.
fn is_offset_position_in_liquid(
    player: &Player,
    view: &dyn CollisionView,
    x: f64,
    y: f64,
    z: f64,
) -> bool {
    let candidate = player_box(player).offset(x, y, z);
    colliding_boxes(view, candidate).is_empty() && !any_liquid_in_bb(view, candidate)
}

#[cfg(test)]
mod tests {
    //! The extents and helpers the vectors' fixtures rest on.

    use super::{BOX_HEIGHT, HALF_WIDTH, liquid_surface, player_box};
    use crate::player::Player;

    #[test]
    fn the_player_box_is_the_sources_own_extents() {
        let bb = player_box(&Player::new());
        assert_eq!(bb.min, [-HALF_WIDTH, 0.0, -HALF_WIDTH]);
        assert_eq!(bb.max, [HALF_WIDTH, BOX_HEIGHT, HALF_WIDTH]);
        // `Entity.java:375-383` widens the floats: (double)(0.6F / 2.0F) and
        // (double)1.8F, not the doubles 0.3 and 1.8.
        assert_eq!(HALF_WIDTH, 0.30000001192092896);
        assert_eq!(BOX_HEIGHT, 1.7999999523162842);
    }

    #[test]
    fn the_liquid_surface_is_the_sources_float_arithmetic() {
        // `BlockLiquid.java:48-56` and `World.java:2106-2110`: `(level + 1) / 9` is
        // a float, subtracted from the cell's top edge in float.
        assert_eq!(liquid_surface(0, 0), f64::from(1.0f32 - 1.0f32 / 9.0f32));
        assert_eq!(liquid_surface(4, 7), f64::from(5.0f32 - 8.0f32 / 9.0f32));
        // 8 or more is a falling flow and reads as a source block.
        assert_eq!(liquid_surface(0, 8), liquid_surface(0, 0));
        assert_eq!(liquid_surface(0, 15), liquid_surface(0, 0));
    }
}
