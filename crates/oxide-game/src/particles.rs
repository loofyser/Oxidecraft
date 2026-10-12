//! The particle simulation: the pooled engine, the 42-row behaviour table and
//! the `ParticleSetting` reduction.
//!
//! The engine owns the client-side particle state the reference keeps in
//! `EffectRenderer`'s eight `(fxLayer, alpha)` lists. The renderer reads
//! [`ParticleEngine::snapshot`], F3's `P:` reads [`ParticleEngine::stats`],
//! and the 0x2A handler feeds [`ParticleEngine::spawn_wire`] from the
//! decoded packet.
//!
//! # Source map
//!
//! * Pooling: one list per `(fxLayer, alpha != 1.0)` pair, evicting index 0
//!   past 4000 (`EffectRenderer.java:146-154`); 4 layers by 2 bins.
//! * Spawn-side reduction (`RenderGlobal.java:2073-2092`): `Decreased`
//!   degrades to `Minimal` on a one-in-three roll, spawns past 256 blocks²
//!   cull unless the row ignores range, and an effective `Minimal` rejects.
//!   The wire's long-distance flag ORs into the row's ignore-range flag on
//!   the way in (`World.java:1124-1128`).
//! * Rain count (`EntityRenderer.java:1533-1539`): `(int)(100*f*f)`, halved
//!   at `Decreased`, zeroed at `Minimal`.
//! * Wire fan-out (`NetHandlerPlayClient.java:2003-2044`): count 0 spawns
//!   once with `speed * offset` velocity; count N spawns N times with
//!   gaussian offsets and `gaussian * speed` velocities.
//! * Behaviour rows: the per-particle classes under `client/particle/`;
//!   each row's doc names its class and the exact constants ported.
//!
//! # Port notes
//!
//! * One engine `rand` ([`StdRng`]) serves every draw — lifetimes, spreads,
//!   colours, the degrade roll — so a seeded engine replays identical
//!   snapshots. The values are not bit-identical to vanilla's
//!   `java.util.Random` stream, only the shapes (uniform picks, gaussian
//!   fan-out) are ported.
//! * The constructor jitter the `EntityFX` six-argument constructor folds
//!   into every motion is folded out: a spawn's velocity is exactly the
//!   fanned-out wire velocity (or the local site's velocity) plus the row's
//!   documented initial lift. Per-tick rules — rises, damps, gravity — are
//!   exact. Residual: initial velocities differ from vanilla by the folded
//!   jitter (order ±0.04 after the classes' ×0.1/×0.01 scalings), plus two
//!   deliberate velocity scales the rows name instead: the smoke/cloud rows
//!   fly `Passthrough` where vanilla flies `motion * 0.1 + speed` (≈9% slow),
//!   and wire-spawned bubbles fly `Passthrough` where vanilla flies
//!   `speed * 0.2 + jitter` (the rare wire-bubble path).
//! * Motion collides against the [`ParticleWorld::ground_height`] plane, not
//!   a swept box: wall intersections pass through. Trigger: a particle with
//!   horizontal velocity inside solid geometry.
//! * The portal/enchantment parametric return paths render as straight
//!   drift. Trigger: the enchant-table and portal ambient streams.
//! * The cloud's player-repel needs the player entity and is folded out.
//! * Trail/twinkle firework flags need NBT the wire never carries; wire
//!   sparks fly plain.
//! * Rows without a quad in this port's [`ParticleTex`] — the barrier and
//!   footstep sheets, the take/emitter entity draws, the mob-appearance
//!   entity, the large-explosion sheet — spawn, tick and count, but the
//!   snapshot skips them. `BARRIER` drawing nothing while counting is the
//!   brief's own Step 3 shape.
//! * `tick` takes the world because the bubble/suspended death rule reads
//!   the block at the particle's position every tick
//!   (`EntityBubbleFX.java:38`); a `tick(&mut self)` shape cannot express
//!   it.
//! * `spawn_local` bypasses the setting reduction: the local sites are the
//!   port's `addEffect`-direct calls, which never pass the spawn-side gate.
//! * Fan-out children (lava smoke, huge-boom shards, drip splashes) join
//!   the lists after the tick and step from the next tick; vanilla's
//!   index loop can catch a same-frame child. A one-tick delay on fan-out
//!   children is the residual.

use std::collections::VecDeque;

use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::particle_world::ParticleWorld;
use crate::settings::ParticleSetting;

/// The per-list ceiling: one list per `(fxLayer, alpha)` pair, 4 layers by
/// 2 alpha bins, evicting index 0 past the cap
/// (`EffectRenderer.java:146-154`).
pub const PER_LIST_CAP: usize = 4000;

/// The spawn-side range cull, squared blocks
/// (`RenderGlobal.java:2073-2092`: past `d0*d0+d1*d1+d2*d2 > 256.0D`).
pub const RANGE_CULL_DIST2: f32 = 256.0;

/// The wire id an out-of-table id falls back to, the source's own rule
/// (`S2APacketParticles.readPacketData`, mirrored by the proto decoder).
const BARRIER_ID: i32 = 35;

/// The ground friction most motion rows apply on contact
/// (`0.699999988079071D` across the particle classes).
const GROUND_FRICTION: f32 = 0.7;

/// Where a particle samples its sprite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleTex {
    /// A `particles.png` cell.
    Cell(u8),
    /// A block-atlas sprite index.
    Atlas(u16),
    /// An item icon: the item id and its metadata.
    ItemAtlas(u32, u32),
}

/// One draw-ready particle: the renderer's input.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleFrame {
    /// The current position, in blocks.
    pub pos: [f32; 3],
    /// The previous tick's position, in blocks.
    pub prev_pos: [f32; 3],
    /// Ticks since spawn, as a float.
    pub age: f32,
    /// The current quad scale.
    pub size: f32,
    /// Red, green, blue and alpha.
    pub colour: [f32; 4],
    /// Where the sprite comes from.
    pub tex: ParticleTex,
    /// The sampled block light, 0-15.
    pub light: u8,
}

/// The engine statistics: F3's `P:` field reads these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticleStats {
    /// Every live particle, drawn or not.
    pub count: usize,
    /// The live particles the snapshot emits.
    pub visible: usize,
}

/// A lifetime formula, drawn from the engine `rand` at spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxAge {
    /// A fixed tick count.
    Fixed(u32),
    /// `(int)(8.0 / (r * 0.8 + 0.2))`: the smoke family.
    EightOver,
    /// `(int)(16.0 / (r * 0.8 + 0.2))`: suspended, lava, rain.
    SixteenOver,
    /// `SixteenOver + 2`: the explode tail (`EntityExplodeFX.java:15`).
    Explode,
    /// `(int)(20.0 / (r * 0.8 + 0.2))`: the auras.
    TwentyOver,
    /// `(int)(64.0 / (r * 0.8 + 0.2))`: the drips.
    SixtyFourOver,
    /// `(int)(4.0 / (r * 0.9 + 0.1))`: the `EntityFX` base (item breaks).
    FourOver,
    /// `EightOver + 4`: the flame's tail (`EntityFlameFX`).
    Flame,
    /// `(int)((int)(8.0 / (r * 0.8 + 0.3)) * 2.5)`: the cloud.
    Cloud,
    /// `(int)(6.0 / (r * 0.8 + 0.6))`: the crits.
    Crit,
    /// `(int)(r * 10.0) + 30`: the enchantment glyphs.
    Enchant,
    /// `(int)(r * 10.0) + 40`: the portal wisps.
    Portal,
    /// `48 + r * 12`: the firework sparks.
    Spark,
    /// `6 + r * 4`: the large explosions.
    LargeBoom,
}

/// How the spawn quad scale is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeDraw {
    /// The `EntityFX` base `(r * 0.5 + 0.5) * 2.0`, times a multiplier.
    Base(f32),
    /// `r * r * 6.0 + 1.0` (`EntityExplodeFX`).
    Explode,
    /// `r * 0.2 + 0.5` (`EntityPortalFX`).
    Portal,
    /// `r * 0.5 + 0.2` (`EntityEnchantmentTableParticleFX`).
    Enchant,
    /// Base times `r * 2.0 + 0.2` (`EntityLavaFX`).
    Lava,
    /// Base times `r * 0.6 + 0.2` (bubble, suspended).
    Small,
    /// Base times `r * 0.6 + 0.5` (the auras).
    Wide,
}

/// How the quad scale tracks age at snapshot time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeCurve {
    /// The spawn scale, unchanged.
    Constant,
    /// `base * clamp(age / max * 32, 0, 1)`: the smoke/note/crit/reddust
    /// grow (`EntitySmokeFX.renderParticle` and kin).
    Grow32,
    /// `base * (1 - f * f * 0.5)`: the flame shrink
    /// (`EntityFlameFX.renderParticle`).
    Flame,
    /// `base * (1 - f * f)`: the lava shrink (`EntityLavaFX.renderParticle`).
    Lava,
    /// `base * (1 - (1 - f)^2)`: the portal bloom
    /// (`EntityPortalFX.renderParticle`).
    Portal,
}

/// How the snapshot light byte is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bright {
    /// The world sample at the particle's cell.
    Sampled,
    /// `max(sample, f * 15)`: the full-brightness ramp
    /// (`EntityFlameFX.getBrightnessForRender`, and the `f^4` sky ramps of
    /// the portal/enchantment classes, folded to the same byte).
    RampToFull,
    /// Full bright: lava and the large explosions
    /// (`EntityLavaFX.getBrightness`, `EntityLargeExplodeFX:61680`).
    Full,
    /// A fixed byte: the lava drip's `257` reads dark here.
    Fixed(u8),
}

/// How the spawn colour is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColourInit {
    /// White.
    White,
    /// A grey uniform in `lo..hi` (`EntitySmokeFX`, `EntityCloudFX`,
    /// `EntityCrit2FX`, `EntityExplodeFX`, `EntityAuraFX` all draw one).
    GreyRange(f32, f32),
    /// The `0.6F` dig grey; the spawn tint multiplies it
    /// (`EntityDiggingFX`).
    Digging,
    /// The pitch rainbow from the x speed (`EntityNoteFX`).
    NotePitch,
    /// Tinted by the speeds (`EntityReddustFX`).
    RedstoneSpeeds,
    /// `RGB = speeds` (`EntitySpellParticleFX.MobFactory`).
    SpellSpeeds,
    /// `(f, 0, f)` with `f = r * 0.5 + 0.35`
    /// (`EntitySpellParticleFX.WitchFactory`).
    WitchMagenta,
    /// The drip's steady state `(0.2, 0.3, 1.0)`
    /// (`EntityDropParticleFX.onUpdate`).
    DropWater,
    /// The lava drip's early-bob colour; the tick recomputes it from the
    /// bob timer.
    DropLava,
    /// `(0.9f, 0.3f, f)` with `f = r * 0.6 + 0.4` (`EntityPortalFX`).
    PortalShade,
    /// `(0.9f, 0.9f, f)` with `f = r * 0.6 + 0.4`
    /// (`EntityEnchantmentTableParticleFX`).
    EnchantShade,
    /// The crit-magic tint over the crit base
    /// (`EntityCrit2FX.MagicFactory`: `*(0.3, 0.8, 1.0)`).
    CritMagic,
}

/// How a live colour tracks ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourTick {
    /// The spawn colour, unchanged.
    None,
    /// `green *= 0.96`, `blue *= 0.9` per tick (`EntityCrit2FX.onUpdate`).
    CritDecay,
    /// `16 / (t + 16)` green, `4 / (t + 8)` blue from the bob timer
    /// (`EntityDropParticleFX.onUpdate`, lava branch).
    DripLava,
}

/// Where the `Cell` sprite index comes from at spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellDraw {
    /// A fixed `particles.png` cell.
    Fixed(u8),
    /// `19 + r * 4` (`EntityRainFX`).
    Rain,
    /// `(r * 8)` (`EntityPortalFX`).
    Portal,
    /// `(r * 26 + 1 + 224)` (`EntityEnchantmentTableParticleFX`).
    Enchant,
}

/// How the `Cell` sprite index tracks age.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellAnim {
    /// The spawn cell, unchanged.
    Fixed,
    /// `7 - age * 8 / max`: the smoke/explode/cloud/reddust/snow countdown.
    SmokeFamily,
    /// `base + (7 - age * 8 / max)`: the spell rows and the sparks.
    SpellBase(u8),
    /// `19 + ((60 - remaining) % 4)`: the wake shimmer
    /// (`EntityFishWakeFX.onUpdate`).
    Wake,
    /// `113` while the bob timer runs, `112` after
    /// (`EntityDropParticleFX.onUpdate`).
    Drip,
}

/// Where the sprite comes from for a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexKind {
    /// `particles.png`, drawn then animated.
    Cell {
        /// The spawn draw.
        draw: CellDraw,
        /// The age animation.
        anim: CellAnim,
    },
    /// The block atlas, from the wire data run (`BLOCK_CRACK`,
    /// `BLOCK_DUST`).
    AtlasArg,
    /// The item icon, from the wire data run (`ITEM_CRACK`).
    ItemArg,
    /// A fixed item icon: the snowball (332) and slime (341) rows
    /// (`Item.java:842`, `:851`).
    ItemFixed(u32, u32),
    /// No quad in this port; the particle still ticks and counts.
    Hidden,
}

/// How the spawn velocity is built from the given velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitVel {
    /// The given velocity, unchanged.
    Passthrough,
    /// Motion zeroed, then `y += 0.2` (`EntityNoteFX`).
    Note,
    /// Motion zeroed, then `y += 0.1` (`EntityHeartFX`).
    Heart,
    /// Motion zeroed (`EntitySuspendFX`, the drips).
    Zero,
    /// `y = r * 0.4 + 0.05` (`EntityLavaFX`).
    LavaUp,
    /// `y = r * 0.2 + 0.1` (`EntityRainFX`).
    RainFall,
    /// The splash conditional: speeds kept with `y += 0.1` when `y == 0`
    /// with horizontal speed, else the rain fall (`EntitySplashFX`).
    SplashOrRain,
    /// `[r - 0.5, y, r - 0.5]` (`EntitySpellParticleFX`).
    Spell,
    /// `*= 0.02` (`EntityAuraFX`).
    Aura,
    /// `+= (r * 2 - 1) * 0.05` per axis (`EntityExplodeFX`).
    Explode,
    /// `*= 0.4` (`EntityCrit2FX`).
    Crit,
}

/// A spawn position nudge from the constructor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PosAdjust {
    /// No nudge.
    None,
    /// `y -= 0.125` (`EntitySuspendFX`).
    DownEighth,
    /// `y += 0.5` (the angry-villager factory).
    UpHalf,
}

/// What meeting the ground does beyond resting on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundAction {
    /// Rest on the surface.
    Rest,
    /// Die and fan out one splash (`EntityDropParticleFX`: the water drip).
    SplashDie,
    /// On the surface, die on a coin flip (`EntityRainFX.onUpdate`: the
    /// liquid/solid kill and the 50% on-ground death, folded onto the
    /// heightmap plane).
    RainDie,
    /// Survive with the cooled texture (`EntityDropParticleFX`: the lava
    /// drip's `114`).
    LavaSpot,
}

/// What a tick fans out into the pending queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanOut {
    /// Nothing.
    None,
    /// A smoke when `r > age / max` (`EntityLavaFX.onUpdate`).
    LavaSmoke,
    /// Six large explosions per tick for eight ticks
    /// (`EntityHugeExplodeFX.onUpdate`).
    HugeBoom,
}

/// One row of the behaviour table: ids 0-41.
///
/// Each row names its class; the constants are the class's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleBehaviour {
    /// The wire name (`EnumParticleTypes`).
    pub name: &'static str,
    /// The per-tick velocity multiplier.
    pub friction: f32,
    /// The per-tick downward velocity step.
    pub gravity_step: f32,
    /// The per-tick upward velocity step.
    pub rise_step: f32,
    /// The on-ground horizontal multiplier (`0.7`, or `1.0` where the
    /// class has no `onGround` term).
    pub ground_friction: f32,
    /// Whether the motion rules meet the ground (the `noClip` invert).
    pub collides: bool,
    /// The effect layer (0-3).
    pub layer: u8,
    /// The sprite source.
    pub tex_kind: TexKind,
    /// Whether a far viewer still spawns this row
    /// (`EnumParticleTypes.getShouldIgnoreRange`).
    pub ignore_range: bool,
    /// The trailing wire-data arity (`getArgumentCount`).
    pub args: u8,
    /// The lifetime formula.
    pub max_age: MaxAge,
    /// The spawn scale draw.
    pub size_draw: SizeDraw,
    /// The age scale curve.
    pub size_curve: SizeCurve,
    /// The light rule.
    pub bright: Bright,
    /// The spawn colour draw.
    pub colour: ColourInit,
    /// The live colour rule.
    pub colour_tick: ColourTick,
    /// The spawn velocity rule.
    pub init_vel: InitVel,
    /// The spawn position nudge.
    pub pos_adjust: PosAdjust,
    /// The spawn alpha (`AmbientMobFactory`'s 0.15, the spark's 0.99).
    pub spawn_alpha: f32,
    /// Whether alpha fades past the half-life (`SparkFX.onUpdate`).
    pub alpha_fade: bool,
    /// Whether the particle dies when its block is not water.
    pub water_death: bool,
    /// What meeting the ground does.
    pub ground_action: GroundAction,
    /// What a tick fans out.
    pub fan_out: FanOut,
    /// The age at spawn (the crit row's constructor ticks once).
    pub age_start: u32,
}

/// The behaviour table: one row per id 0-41.
///
/// Derive homes: the per-particle classes under `client/particle/` with the
/// factory map at `EffectRenderer.java:64-104`, the names, ignore-range
/// flags and arities at `util/EnumParticleTypes.java`.
pub static BEHAVIOURS: [ParticleBehaviour; 42] = [
    // 0 `explode`: rise, heavy damp, animated cells (`EntityExplodeFX`).
    ParticleBehaviour {
        name: "explode",
        friction: 0.9,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: true,
        args: 0,
        max_age: MaxAge::Explode,
        size_draw: SizeDraw::Explode,
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.7, 1.0),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Explode,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 1 `largeexplode`: a static full-bright flash on the entity sheet
    // (`EntityLargeExplodeFX`); hidden here, still counted.
    ParticleBehaviour {
        name: "largeexplode",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 3,
        tex_kind: TexKind::Hidden,
        ignore_range: true,
        args: 0,
        max_age: MaxAge::LargeBoom,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Full,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 2 `hugeexplosion`: an invisible eight-tick emitter fanning six large
    // explosions per tick (`EntityHugeExplodeFX`).
    ParticleBehaviour {
        name: "hugeexplosion",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 1,
        tex_kind: TexKind::Hidden,
        ignore_range: true,
        args: 0,
        max_age: MaxAge::Fixed(8),
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::HugeBoom,
        age_start: 0,
    },
    // 3 `fireworksSpark`: light gravity, the half-life alpha fade and the
    // trail-head alpha bin (`EntityFirework.SparkFX`).
    ParticleBehaviour {
        name: "fireworksSpark",
        friction: 0.91,
        gravity_step: 0.004,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(160),
            anim: CellAnim::SpellBase(160),
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Spark,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 0.99,
        alpha_fade: true,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 4 `bubble`: the rise and the out-of-water death
    // (`EntityBubbleFX`: `motionY += 0.002D`, `:38`).
    ParticleBehaviour {
        name: "bubble",
        friction: 0.85,
        gravity_step: 0.0,
        rise_step: 0.002,
        ground_friction: 1.0,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(32),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Small,
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: true,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 5 `splash`: the rain fall with the speed conditional and its own
    // gravity (`EntitySplashFX`: `0.04F`).
    ParticleBehaviour {
        name: "splash",
        friction: 0.98,
        gravity_step: 0.04,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Rain,
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::SplashOrRain,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::RainDie,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 6 `wake`: the gravity-free shimmer (`EntityFishWakeFX`).
    ParticleBehaviour {
        name: "wake",
        friction: 0.98,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(19),
            anim: CellAnim::Wake,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 7 `suspended`: static, and the out-of-water death
    // (`EntitySuspendFX`: zeroed motion, `:34`).
    ParticleBehaviour {
        name: "suspended",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(0),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::SixteenOver,
        size_draw: SizeDraw::Small,
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.4, 0.4),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::DownEighth,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: true,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 8 `depthsuspend`: the drifting aura, no water rule (`EntityAuraFX`).
    ParticleBehaviour {
        name: "depthsuspend",
        friction: 0.99,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(0),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::TwentyOver,
        size_draw: SizeDraw::Wide,
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.2, 0.3),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Aura,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 9 `crit`: the large smoke (`EntityCritFX` extends the smoke at 2.5x).
    ParticleBehaviour {
        name: "crit",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(1.875),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.0, 0.3),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 10 `magicCrit`: the decaying crit with the shifted cell; the
    // constructor ticks once, so the age starts at one
    // (`EntityCrit2FX.MagicFactory`).
    ParticleBehaviour {
        name: "magicCrit",
        friction: 0.7,
        gravity_step: 0.02,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(66),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Crit,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::CritMagic,
        colour_tick: ColourTick::CritDecay,
        init_vel: InitVel::Crit,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 1,
    },
    // 11 `smoke`: the rise-and-fade reference row (`EntitySmokeFX`).
    ParticleBehaviour {
        name: "smoke",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.0, 0.3),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 12 `largesmoke`: the smoke at 2.5x (`EntityCritFX.Factory`).
    ParticleBehaviour {
        name: "largesmoke",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(1.875),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.0, 0.3),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 13 `spell`: smoke stepping on the spell sheet
    // (`EntitySpellParticleFX.Factory`, base 128).
    ParticleBehaviour {
        name: "spell",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(128),
            anim: CellAnim::SpellBase(128),
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Spell,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 14 `instantSpell`: the spell on base 144
    // (`EntitySpellParticleFX.InstantFactory`).
    ParticleBehaviour {
        name: "instantSpell",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(144),
            anim: CellAnim::SpellBase(144),
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Spell,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 15 `mobSpell`: speeds as colour (`EntitySpellParticleFX.MobFactory`).
    ParticleBehaviour {
        name: "mobSpell",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(128),
            anim: CellAnim::SpellBase(128),
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::SpellSpeeds,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Spell,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 16 `mobSpellAmbient`: the mob spell at 0.15 alpha, in the alpha bin
    // (`EntitySpellParticleFX.AmbientMobFactory`).
    ParticleBehaviour {
        name: "mobSpellAmbient",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(128),
            anim: CellAnim::SpellBase(128),
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::SpellSpeeds,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Spell,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 0.15,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 17 `witchMagic`: base 144 with the magenta draw
    // (`EntitySpellParticleFX.WitchFactory`).
    ParticleBehaviour {
        name: "witchMagic",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.004,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(144),
            anim: CellAnim::SpellBase(144),
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::WitchMagenta,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Spell,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 18 `dripWater`: the bob timer, the steady blue, death plus a splash
    // on landing (`EntityDropParticleFX.WaterFactory`).
    ParticleBehaviour {
        name: "dripWater",
        friction: 0.98,
        gravity_step: 0.06,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(113),
            anim: CellAnim::Drip,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::SixtyFourOver,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::DropWater,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::SplashDie,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 19 `dripLava`: the bob colour ramp, the cooled spot on landing, the
    // dark light byte (`EntityDropParticleFX.LavaFactory`).
    ParticleBehaviour {
        name: "dripLava",
        friction: 0.98,
        gravity_step: 0.06,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(113),
            anim: CellAnim::Drip,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::SixtyFourOver,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Fixed(0),
        colour: ColourInit::DropLava,
        colour_tick: ColourTick::DripLava,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::LavaSpot,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 20 `angryVillager`: the heart at `y + 0.5` on cell 81
    // (`EntityHeartFX.AngryVillagerFactory`).
    ParticleBehaviour {
        name: "angryVillager",
        friction: 0.86,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(81),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Fixed(16),
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Heart,
        pos_adjust: PosAdjust::UpHalf,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 21 `happyVillager`: the aura in white on cell 82
    // (`EntityAuraFX.HappyVillagerFactory`).
    ParticleBehaviour {
        name: "happyVillager",
        friction: 0.99,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(82),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::TwentyOver,
        size_draw: SizeDraw::Wide,
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Aura,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 22 `townaura`: the grey aura (`EntityAuraFX.Factory`).
    ParticleBehaviour {
        name: "townaura",
        friction: 0.99,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(0),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::TwentyOver,
        size_draw: SizeDraw::Wide,
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Aura,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 23 `note`: the fast rise, the six-tick life, the pitch rainbow
    // (`EntityNoteFX`).
    ParticleBehaviour {
        name: "note",
        friction: 0.66,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(64),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Fixed(6),
        size_draw: SizeDraw::Base(1.5),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::NotePitch,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Note,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 24 `portal`: the parametric wisp as straight drift, the bloom, the
    // brightness ramp (`EntityPortalFX`).
    ParticleBehaviour {
        name: "portal",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Portal,
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Portal,
        size_draw: SizeDraw::Portal,
        size_curve: SizeCurve::Portal,
        bright: Bright::RampToFull,
        colour: ColourInit::PortalShade,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 25 `enchantmenttable`: the drifting glyph with the `f^4` light ramp
    // (`EntityEnchantmentTableParticleFX`).
    ParticleBehaviour {
        name: "enchantmenttable",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Enchant,
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Enchant,
        size_draw: SizeDraw::Enchant,
        size_curve: SizeCurve::Constant,
        bright: Bright::RampToFull,
        colour: ColourInit::EnchantShade,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 26 `flame`: the shrink-and-fade with the full-brightness ramp and no
    // water rule (`EntityFlameFX`: shrink `flameScale * (1 - f*f*0.5)`,
    // ramp `f1 * f + (1 - f)`; `onUpdate` is age-out, move, ×0.96 damp
    // and the ×0.7 ground term).
    ParticleBehaviour {
        name: "flame",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: false,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(48),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Flame,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Flame,
        bright: Bright::RampToFull,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 27 `lava`: the pop with its smoke fan-out (`EntityLavaFX`).
    ParticleBehaviour {
        name: "lava",
        friction: 0.999,
        gravity_step: 0.03,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(49),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::SixteenOver,
        size_draw: SizeDraw::Lava,
        size_curve: SizeCurve::Lava,
        bright: Bright::Full,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::LavaUp,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::LavaSmoke,
        age_start: 0,
    },
    // 28 `footstep`: the 200-tick footprint on its own sheet
    // (`EntityFootStepFX`); hidden here, still counted.
    ParticleBehaviour {
        name: "footstep",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 3,
        tex_kind: TexKind::Hidden,
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Fixed(200),
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 29 `cloud`: the large smoke without the rise (`EntityCloudFX`).
    ParticleBehaviour {
        name: "cloud",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Cloud,
        size_draw: SizeDraw::Base(1.875),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.7, 1.0),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 30 `reddust`: the speed-tinted smoke (`EntityReddustFX`).
    ParticleBehaviour {
        name: "reddust",
        friction: 0.96,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::RedstoneSpeeds,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 31 `snowballpoof`: the snowball icon break (`EntityBreakingFX`,
    // item 332).
    ParticleBehaviour {
        name: "snowballpoof",
        friction: 0.98,
        gravity_step: 0.04,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 1,
        tex_kind: TexKind::ItemFixed(332, 0),
        ignore_range: false,
        args: 0,
        max_age: MaxAge::FourOver,
        size_draw: SizeDraw::Base(0.5),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 32 `snowshovel`: the snow dig with its own gravity
    // (`EntitySnowShovelFX`: `motionY -= 0.03D`, ×0.99 damp).
    ParticleBehaviour {
        name: "snowshovel",
        friction: 0.99,
        gravity_step: 0.03,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(7),
            anim: CellAnim::SmokeFamily,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(0.75),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::GreyRange(0.7, 1.0),
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 33 `slime`: the slime-ball icon break (`EntityBreakingFX`, item
    // 341).
    ParticleBehaviour {
        name: "slime",
        friction: 0.98,
        gravity_step: 0.04,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 1,
        tex_kind: TexKind::ItemFixed(341, 0),
        ignore_range: false,
        args: 0,
        max_age: MaxAge::FourOver,
        size_draw: SizeDraw::Base(0.5),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 34 `heart`: the slow heart with its lift (`EntityHeartFX`).
    ParticleBehaviour {
        name: "heart",
        friction: 0.86,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Fixed(80),
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Fixed(16),
        size_draw: SizeDraw::Base(1.5),
        size_curve: SizeCurve::Grow32,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Heart,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 35 `barrier`: the 80-tick barrier-icon billboard (`Barrier`); the
    // icon path needs the item registry, so the row ticks and counts but
    // draws nothing.
    ParticleBehaviour {
        name: "barrier",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 1,
        tex_kind: TexKind::Hidden,
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Fixed(80),
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 36 `iconcrack_`: the item break, 2 wire args
    // (`EntityBreakingFX.Factory`).
    ParticleBehaviour {
        name: "iconcrack_",
        friction: 0.98,
        gravity_step: 0.04,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 1,
        tex_kind: TexKind::ItemArg,
        ignore_range: false,
        args: 2,
        max_age: MaxAge::FourOver,
        size_draw: SizeDraw::Base(0.5),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 37 `blockcrack_`: the block break, 1 wire arg
    // (`EntityDiggingFX.Factory`).
    ParticleBehaviour {
        name: "blockcrack_",
        friction: 0.98,
        gravity_step: 0.04,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 1,
        tex_kind: TexKind::AtlasArg,
        ignore_range: false,
        args: 1,
        max_age: MaxAge::FourOver,
        size_draw: SizeDraw::Base(0.5),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::Digging,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 38 `blockdust_`: the block dust — digging stepping with the speeds
    // kept exactly, 1 wire arg (`EntityBlockDustFX.Factory`).
    ParticleBehaviour {
        name: "blockdust_",
        friction: 0.98,
        gravity_step: 0.04,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 1,
        tex_kind: TexKind::AtlasArg,
        ignore_range: false,
        args: 1,
        max_age: MaxAge::FourOver,
        size_draw: SizeDraw::Base(0.5),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::Digging,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Passthrough,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 39 `droplet`: the rain fall (`EntityRainFX.Factory`).
    ParticleBehaviour {
        name: "droplet",
        friction: 0.98,
        gravity_step: 0.06,
        rise_step: 0.0,
        ground_friction: GROUND_FRICTION,
        collides: true,
        layer: 0,
        tex_kind: TexKind::Cell {
            draw: CellDraw::Rain,
            anim: CellAnim::Fixed,
        },
        ignore_range: false,
        args: 0,
        max_age: MaxAge::EightOver,
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::RainFall,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::RainDie,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 40 `take`: the item-pickup flight (`EntityPickupFX`, spawned direct
    // from the collect-item handler, never through the wire table);
    // hidden here, still counted.
    ParticleBehaviour {
        name: "take",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 3,
        tex_kind: TexKind::Hidden,
        ignore_range: false,
        args: 0,
        max_age: MaxAge::Fixed(3),
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
    // 41 `mobappearance`: the elder-guardian flash on layer 3
    // (`MobAppearance`); an entity draw, so hidden here, still counted.
    ParticleBehaviour {
        name: "mobappearance",
        friction: 1.0,
        gravity_step: 0.0,
        rise_step: 0.0,
        ground_friction: 1.0,
        collides: false,
        layer: 3,
        tex_kind: TexKind::Hidden,
        ignore_range: true,
        args: 0,
        max_age: MaxAge::Fixed(30),
        size_draw: SizeDraw::Base(1.0),
        size_curve: SizeCurve::Constant,
        bright: Bright::Sampled,
        colour: ColourInit::White,
        colour_tick: ColourTick::None,
        init_vel: InitVel::Zero,
        pos_adjust: PosAdjust::None,
        spawn_alpha: 1.0,
        alpha_fade: false,
        water_death: false,
        ground_action: GroundAction::Rest,
        fan_out: FanOut::None,
        age_start: 0,
    },
];

/// Looks up a behaviour row by wire id.
pub fn behaviour(id: u8) -> Option<&'static ParticleBehaviour> {
    BEHAVIOURS.get(usize::from(id))
}

/// A local spawn: the dig/footstep/splash sites' entry.
#[derive(Debug, Clone, Copy)]
pub struct ParticleSpawn {
    /// The wire id, 0-41.
    pub id: u8,
    /// The spawn position, in blocks.
    pub pos: [f32; 3],
    /// The spawn velocity, in blocks per tick.
    pub vel: [f32; 3],
    /// An explicit tint, overriding the row default (the dig tint).
    pub colour: Option<[f32; 3]>,
    /// A block-atlas sprite override (block break).
    pub atlas_sprite: Option<u16>,
    /// An item-icon override: the item id and its metadata (item break).
    pub item: Option<(u32, u32)>,
}

/// One live particle.
struct LiveParticle {
    /// The behaviour row.
    id: u8,
    /// The current position, in blocks.
    pos: [f32; 3],
    /// The previous tick's position.
    prev: [f32; 3],
    /// The current velocity, in blocks per tick.
    vel: [f32; 3],
    /// Ticks since spawn.
    age: u32,
    /// The drawn lifetime.
    max_age: u32,
    /// The spawn quad scale.
    base_size: f32,
    /// Red, green, blue.
    colour: [f32; 3],
    /// The current alpha.
    alpha: f32,
    /// The drawn sprite cell, for `Cell` rows.
    cell0: u8,
    /// The resolved sprite source for `Atlas`/`Item` rows.
    tex: ParticleTex,
    /// The sampled light byte.
    light: u8,
    /// Whether the last tick rested on the ground.
    grounded: bool,
    /// The drip bob timer (`EntityDropParticleFX.bobTimer`).
    bob: u8,
}

/// The pooled engine.
///
/// Eight lists keyed by `(layer, alpha < 1.0)`, each capped: the
/// `EffectRenderer` pooling with the alpha-bin rule its `setAlphaF` moves
/// particles across.
pub struct ParticleEngine {
    /// The eight `(layer, alpha)` lists in vanilla order.
    lists: [VecDeque<LiveParticle>; 8],
    /// Every draw the engine makes.
    rng: StdRng,
    /// The active reduction regime.
    setting: ParticleSetting,
    /// The per-list cap (vanilla 4000).
    limit: usize,
    /// The last wire spawn's viewer, for the pending fan-out cull.
    viewer: Option<[f32; 3]>,
}

/// The fixed seed behind [`ParticleEngine::new`]: deterministic like every
/// engine, just not caller-picked.
const FIXED_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// List index for a row and alpha: `fxLayers[layer][alpha != 1.0 ? 0 : 1]`
/// (`EffectRenderer.java:146-154`).
fn list_index(layer: u8, alpha: f32) -> usize {
    usize::from(layer.min(3)) * 2 + usize::from(alpha >= 1.0)
}

impl ParticleEngine {
    /// Creates an engine with the given per-list cap.
    pub fn new(limit: usize) -> Self {
        Self::seeded(FIXED_SEED, limit)
    }

    /// Creates a deterministically seeded engine for the tests.
    pub fn seeded(seed: u64, limit: usize) -> Self {
        Self {
            lists: std::array::from_fn(|_| VecDeque::new()),
            rng: StdRng::seed_from_u64(seed),
            setting: ParticleSetting::All,
            limit,
            viewer: None,
        }
    }

    /// Selects the reduction regime.
    pub fn set_limit(&mut self, setting: ParticleSetting) {
        self.setting = setting;
    }

    /// The active reduction regime.
    pub fn setting(&self) -> ParticleSetting {
        self.setting
    }

    /// The per-list cap.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Whether a spawn at `pos` survives the setting reduction.
    ///
    /// Mirrors `RenderGlobal.spawnEntityFX` (`RenderGlobal.java:2073-2092`):
    /// the one-in-three degrade at `Decreased` (rolled even for bypassed
    /// rows, as the source rolls before the branch), the ignore-range
    /// bypass of both the cull and the setting, the 256-block² cull, then
    /// the effective-`Minimal` rejection.
    fn admit(
        &mut self,
        row: &ParticleBehaviour,
        pos: [f32; 3],
        viewer: Option<[f32; 3]>,
        long_distance: bool,
    ) -> bool {
        let mut effective = self.setting;
        if effective == ParticleSetting::Decreased && self.rng.random_range(0..3) == 0 {
            effective = ParticleSetting::Minimal;
        }
        if row.ignore_range || long_distance {
            return true;
        }
        if let Some(viewer) = viewer {
            let dx = viewer[0] - pos[0];
            let dy = viewer[1] - pos[1];
            let dz = viewer[2] - pos[2];
            if dx * dx + dy * dy + dz * dz > RANGE_CULL_DIST2 {
                return false;
            }
        }
        effective != ParticleSetting::Minimal
    }

    /// Builds a live particle, drawing lifetime, scale, colour and cells.
    ///
    /// `vel` is the pre-transform velocity the pitch/speed-tinted rows
    /// read; `colour` overrides the row default (the dig tint).
    fn build(
        &mut self,
        row: &'static ParticleBehaviour,
        id: u8,
        pos: [f32; 3],
        vel: [f32; 3],
        tex: ParticleTex,
        colour: Option<[f32; 3]>,
    ) -> LiveParticle {
        let max_age = draw_max_age(&mut self.rng, row.max_age);
        let base_size = draw_size(&mut self.rng, row.size_draw);
        let spawn_colour = colour.unwrap_or_else(|| draw_colour(&mut self.rng, row.colour, vel));
        let mut p = pos;
        match row.pos_adjust {
            PosAdjust::None => {}
            PosAdjust::DownEighth => p[1] -= 0.125,
            PosAdjust::UpHalf => p[1] += 0.5,
        }
        let mut v = vel;
        match row.init_vel {
            InitVel::Passthrough => {}
            InitVel::Note => v = [0.0, 0.2, 0.0],
            InitVel::Heart => v = [0.0, 0.1, 0.0],
            InitVel::Zero => v = [0.0, 0.0, 0.0],
            InitVel::LavaUp => v = [0.0, self.rng.random::<f32>() * 0.4 + 0.05, 0.0],
            InitVel::RainFall => v = [0.0, self.rng.random::<f32>() * 0.2 + 0.1, 0.0],
            InitVel::SplashOrRain => {
                if v[1] == 0.0 && (v[0] != 0.0 || v[2] != 0.0) {
                    v[1] += 0.1;
                } else {
                    v = [0.0, self.rng.random::<f32>() * 0.2 + 0.1, 0.0];
                }
            }
            InitVel::Spell => {
                v = [
                    self.rng.random::<f32>() - 0.5,
                    v[1],
                    self.rng.random::<f32>() - 0.5,
                ];
            }
            InitVel::Aura => v = [v[0] * 0.02, v[1] * 0.02, v[2] * 0.02],
            InitVel::Explode => {
                v = [
                    v[0] + (self.rng.random::<f32>() * 2.0 - 1.0) * 0.05,
                    v[1] + (self.rng.random::<f32>() * 2.0 - 1.0) * 0.05,
                    v[2] + (self.rng.random::<f32>() * 2.0 - 1.0) * 0.05,
                ];
            }
            InitVel::Crit => v = [v[0] * 0.4, v[1] * 0.4, v[2] * 0.4],
        }
        let cell0 = match row.tex_kind {
            TexKind::Cell { draw, .. } => match draw {
                CellDraw::Fixed(cell) => cell,
                CellDraw::Rain => 19 + self.rng.random_range(0..4),
                CellDraw::Portal => (self.rng.random::<f32>() * 8.0) as u8,
                CellDraw::Enchant => (self.rng.random::<f32>() * 26.0) as u8 + 1 + 224,
            },
            TexKind::AtlasArg | TexKind::ItemArg | TexKind::ItemFixed(..) | TexKind::Hidden => 0,
        };
        LiveParticle {
            id,
            pos: p,
            prev: p,
            vel: v,
            age: row.age_start,
            max_age,
            base_size,
            colour: spawn_colour,
            alpha: row.spawn_alpha,
            cell0,
            tex,
            light: 0,
            grounded: false,
            bob: 40,
        }
    }

    /// Pushes a particle into its list, evicting the oldest past the cap.
    ///
    /// A zero cap holds nothing.
    fn push(&mut self, particle: LiveParticle) {
        if self.limit == 0 {
            return;
        }
        let row = &BEHAVIOURS[usize::from(particle.id)];
        let index = list_index(row.layer, particle.alpha);
        let list = &mut self.lists[index];
        if list.len() >= self.limit {
            list.pop_front();
        }
        list.push_back(particle);
    }

    /// Spawns from a wire event; answers how many particles were added.
    ///
    /// The fan-out mirrors `NetHandlerPlayClient.handleParticles`
    /// (`NetHandlerPlayClient.java:2003-2044`): count 0 spawns once with
    /// `speed * offset` velocity, count N spawns N times with gaussian
    /// offsets and `gaussian * speed` velocities. The wire's long-distance
    /// flag joins the row's ignore-range flag (`World.java:1124-1128`), an
    /// out-of-table id falls back to `BARRIER`, and a short data run
    /// spawns nothing. Every candidate passes the [`Self::admit`] gate.
    /// The `world` read is reserved for the digging-tint bridge: wire
    /// block breaks currently carry the default grey.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_wire(
        &mut self,
        particle_id: i32,
        long_distance: bool,
        pos: [f32; 3],
        offset: [f32; 3],
        speed: f32,
        count: i32,
        data: &[i32],
        viewer: [f32; 3],
        world: &dyn ParticleWorld,
    ) -> usize {
        let _ = world;
        let id = if matches!(particle_id, 0..=41) {
            particle_id as u8
        } else {
            BARRIER_ID as u8
        };
        let row: &'static ParticleBehaviour = &BEHAVIOURS[usize::from(id)];
        if data.len() < usize::from(row.args) {
            return 0;
        }
        if self.limit == 0 {
            return 0;
        }
        self.viewer = Some(viewer);
        let tex = wire_tex(row, data);
        if count == 0 {
            let vel = [speed * offset[0], speed * offset[1], speed * offset[2]];
            if !self.admit(row, pos, Some(viewer), long_distance) {
                return 0;
            }
            let particle = self.build(row, id, pos, vel, tex, None);
            self.push(particle);
            return 1;
        }
        if count < 0 {
            return 0;
        }
        let mut added = 0;
        for _ in 0..count {
            let p = [
                pos[0] + gauss(&mut self.rng) as f32 * offset[0],
                pos[1] + gauss(&mut self.rng) as f32 * offset[1],
                pos[2] + gauss(&mut self.rng) as f32 * offset[2],
            ];
            let v = [
                gauss(&mut self.rng) as f32 * speed,
                gauss(&mut self.rng) as f32 * speed,
                gauss(&mut self.rng) as f32 * speed,
            ];
            if !self.admit(row, p, Some(viewer), long_distance) {
                continue;
            }
            let particle = self.build(row, id, p, v, tex, None);
            self.push(particle);
            added += 1;
        }
        added
    }

    /// Spawns from a local site; answers whether the particle was added.
    ///
    /// Local sites bypass the setting reduction (the `addEffect`-direct
    /// path) but still evict past the per-list cap. An unknown id spawns
    /// nothing; a data-carrying row without its override falls back to
    /// the neutral sprite.
    pub fn spawn_local(&mut self, spawn: ParticleSpawn) -> bool {
        let Some(row) = BEHAVIOURS.get(usize::from(spawn.id)) else {
            return false;
        };
        let row: &'static ParticleBehaviour = row;
        if self.limit == 0 {
            return false;
        }
        let tex = match row.tex_kind {
            TexKind::Cell { .. } | TexKind::Hidden => ParticleTex::Cell(0),
            TexKind::AtlasArg => ParticleTex::Atlas(spawn.atlas_sprite.unwrap_or(0)),
            TexKind::ItemArg => {
                let (item, meta) = spawn.item.unwrap_or((0, 0));
                ParticleTex::ItemAtlas(item, meta)
            }
            TexKind::ItemFixed(item, meta) => ParticleTex::ItemAtlas(item, meta),
        };
        let particle = self.build(row, spawn.id, spawn.pos, spawn.vel, tex, spawn.colour);
        self.push(particle);
        true
    }

    /// Steps every live particle once.
    ///
    /// Takes the world because the bubble/suspended death rule reads the
    /// block at the particle's position every tick
    /// (`EntityBubbleFX.java:38`). Fan-out children join the lists after
    /// the tick and step from the next tick.
    pub fn tick(&mut self, world: &dyn ParticleWorld) {
        let Self {
            lists,
            rng,
            setting,
            limit: _,
            viewer,
        } = self;
        let mut pending: Vec<(u8, [f32; 3], [f32; 3])> = Vec::new();
        for list in lists.iter_mut() {
            let mut i = 0;
            while i < list.len() {
                let id = list[i].id;
                let row: &'static ParticleBehaviour = &BEHAVIOURS[usize::from(id)];
                // Vanilla's `particleAge++ >= particleMaxAge`: the death
                // tick still moves once, then the particle is gone.
                let expired = list[i].age >= list[i].max_age;
                let p = &mut list[i];
                p.prev = p.pos;
                p.vel[1] += row.rise_step - row.gravity_step;
                if p.bob > 0
                    && matches!(
                        row.ground_action,
                        GroundAction::SplashDie | GroundAction::LavaSpot
                    )
                {
                    p.vel[0] *= 0.02;
                    p.vel[1] *= 0.02;
                    p.vel[2] *= 0.02;
                    p.bob -= 1;
                }
                p.pos[0] += p.vel[0];
                p.pos[1] += p.vel[1];
                p.pos[2] += p.vel[2];
                let mut on_ground = false;
                let mut dead = false;
                let mut splash = false;
                if row.collides {
                    if let Some(surface) = world.ground_height(p.pos) {
                        if p.pos[1] <= surface {
                            p.pos[1] = surface;
                            on_ground = true;
                        }
                    }
                    match row.ground_action {
                        GroundAction::Rest => {}
                        GroundAction::SplashDie => {
                            if on_ground {
                                dead = true;
                                splash = true;
                            }
                        }
                        GroundAction::RainDie => {
                            if on_ground && rng.random::<f32>() < 0.5 {
                                dead = true;
                            }
                        }
                        GroundAction::LavaSpot => {}
                    }
                }
                p.grounded = on_ground;
                p.vel[0] *= row.friction;
                p.vel[1] *= row.friction;
                p.vel[2] *= row.friction;
                if on_ground {
                    p.vel[0] *= row.ground_friction;
                    p.vel[2] *= row.ground_friction;
                }
                if !dead && row.water_death && !world.is_water(p.pos) {
                    dead = true;
                }
                match row.colour_tick {
                    ColourTick::None => {}
                    ColourTick::CritDecay => {
                        p.colour[1] *= 0.96;
                        p.colour[2] *= 0.9;
                    }
                    ColourTick::DripLava => {
                        let t = f32::from(40 - p.bob.min(40));
                        p.colour = [1.0, 16.0 / (t + 16.0), 4.0 / (t + 8.0)];
                    }
                }
                if row.alpha_fade && p.age > p.max_age / 2 {
                    let half = (p.max_age / 2) as f32;
                    p.alpha = 1.0 - ((p.age) as f32 - half) / (p.max_age) as f32;
                }
                p.light = world.light_at(p.pos).min(15);
                match row.fan_out {
                    FanOut::None => {}
                    FanOut::LavaSmoke => {
                        // `EntityLavaFX.onUpdate` increments the age first,
                        // then emits against `age / maxAge`: the post-
                        // increment age.
                        let f = (p.age + 1) as f32 / (p.max_age.max(1)) as f32;
                        if rng.random::<f32>() > f {
                            pending.push((11, p.pos, p.vel));
                        }
                    }
                    FanOut::HugeBoom => {
                        for _ in 0..6 {
                            let child = [
                                p.pos[0] + (rng.random::<f64>() - rng.random::<f64>()) as f32 * 4.0,
                                p.pos[1] + (rng.random::<f64>() - rng.random::<f64>()) as f32 * 4.0,
                                p.pos[2] + (rng.random::<f64>() - rng.random::<f64>()) as f32 * 4.0,
                            ];
                            let speed = (p.age) as f32 / 8.0;
                            pending.push((1, child, [speed, 0.0, 0.0]));
                        }
                    }
                }
                if splash {
                    pending.push((5, p.pos, [0.0, 0.0, 0.0]));
                }
                p.age += 1;
                if dead || expired {
                    list.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        for (id, pos, vel) in pending {
            let row: &'static ParticleBehaviour = &BEHAVIOURS[usize::from(id)];
            // `RenderGlobal.spawnEntityFX:2073-2092` reads the setting fresh
            // and rolls per call: each fan-out child is admitted
            // independently, so the degrade rolls inside the loop.
            let mut effective = *setting;
            if effective == ParticleSetting::Decreased && rng.random_range(0..3) == 0 {
                effective = ParticleSetting::Minimal;
            }
            if !row.ignore_range {
                if let Some(viewer) = *viewer {
                    let dx = viewer[0] - pos[0];
                    let dy = viewer[1] - pos[1];
                    let dz = viewer[2] - pos[2];
                    if dx * dx + dy * dy + dz * dz > RANGE_CULL_DIST2 {
                        continue;
                    }
                }
                if effective == ParticleSetting::Minimal {
                    continue;
                }
            }
            let tex = ParticleTex::Cell(0);
            let particle = build_pending(rng, row, id, pos, vel, tex);
            if self.limit == 0 {
                continue;
            }
            let index = list_index(row.layer, particle.alpha);
            let list = &mut lists[index];
            if list.len() >= self.limit {
                list.pop_front();
            }
            list.push_back(particle);
        }
    }

    /// The renderer's input: one frame per drawn particle, lists in order.
    ///
    /// Rows without a quad ([`TexKind::Hidden`]) are skipped; they still
    /// count in [`Self::stats`].
    pub fn snapshot(&self) -> Vec<ParticleFrame> {
        let mut frames = Vec::new();
        for list in &self.lists {
            for p in list {
                let row = &BEHAVIOURS[usize::from(p.id)];
                let tex = match row.tex_kind {
                    TexKind::Hidden => continue,
                    TexKind::AtlasArg | TexKind::ItemArg | TexKind::ItemFixed(..) => p.tex,
                    TexKind::Cell { anim, .. } => ParticleTex::Cell(cell_at(row, p, anim)),
                };
                let max = (p.max_age.max(1)) as f32;
                let f = ((p.age) as f32 / max).clamp(0.0, 1.0);
                let size = match row.size_curve {
                    SizeCurve::Constant => p.base_size,
                    SizeCurve::Grow32 => {
                        p.base_size * ((p.age) as f32 / max * 32.0).clamp(0.0, 1.0)
                    }
                    SizeCurve::Flame => p.base_size * (1.0 - f * f * 0.5),
                    SizeCurve::Lava => p.base_size * (1.0 - f * f),
                    SizeCurve::Portal => {
                        let g = 1.0 - f;
                        p.base_size * (1.0 - g * g)
                    }
                };
                let light = match row.bright {
                    Bright::Sampled => p.light,
                    Bright::RampToFull => p.light.max((f * 15.0) as u8),
                    Bright::Full => 15,
                    Bright::Fixed(byte) => byte,
                };
                frames.push(ParticleFrame {
                    pos: p.pos,
                    prev_pos: p.prev,
                    age: (p.age) as f32,
                    size,
                    colour: [p.colour[0], p.colour[1], p.colour[2], p.alpha],
                    tex,
                    light,
                });
            }
        }
        frames
    }

    /// The F3 counters: every live particle, and the drawn subset.
    pub fn stats(&self) -> ParticleStats {
        let mut count = 0;
        let mut visible = 0;
        for list in &self.lists {
            for p in list {
                count += 1;
                if !matches!(BEHAVIOURS[usize::from(p.id)].tex_kind, TexKind::Hidden) {
                    visible += 1;
                }
            }
        }
        ParticleStats { count, visible }
    }
}

/// Builds a pending fan-out child (no colour override, neutral sprite).
fn build_pending(
    rng: &mut StdRng,
    row: &'static ParticleBehaviour,
    id: u8,
    pos: [f32; 3],
    vel: [f32; 3],
    tex: ParticleTex,
) -> LiveParticle {
    let max_age = draw_max_age(rng, row.max_age);
    let base_size = draw_size(rng, row.size_draw);
    let colour = draw_colour(rng, row.colour, vel);
    let mut v = vel;
    match row.init_vel {
        InitVel::Passthrough
        | InitVel::Note
        | InitVel::Heart
        | InitVel::Zero
        | InitVel::Spell
        | InitVel::Aura
        | InitVel::Explode
        | InitVel::Crit => {}
        InitVel::LavaUp => v = [0.0, rng.random::<f32>() * 0.4 + 0.05, 0.0],
        InitVel::RainFall => v = [0.0, rng.random::<f32>() * 0.2 + 0.1, 0.0],
        InitVel::SplashOrRain => {
            if v[1] == 0.0 && (v[0] != 0.0 || v[2] != 0.0) {
                v[1] += 0.1;
            } else {
                v = [0.0, rng.random::<f32>() * 0.2 + 0.1, 0.0];
            }
        }
    }
    let cell0 = match row.tex_kind {
        TexKind::Cell { draw, .. } => match draw {
            CellDraw::Fixed(cell) => cell,
            CellDraw::Rain => 19 + rng.random_range(0..4),
            CellDraw::Portal => (rng.random::<f32>() * 8.0) as u8,
            CellDraw::Enchant => (rng.random::<f32>() * 26.0) as u8 + 1 + 224,
        },
        TexKind::AtlasArg | TexKind::ItemArg | TexKind::ItemFixed(..) | TexKind::Hidden => 0,
    };
    LiveParticle {
        id,
        pos,
        prev: pos,
        vel: v,
        age: row.age_start,
        max_age,
        base_size,
        colour,
        alpha: row.spawn_alpha,
        cell0,
        tex,
        light: 0,
        grounded: false,
        bob: 40,
    }
}

/// The sprite cell for a `Cell` row at snapshot time.
fn cell_at(row: &ParticleBehaviour, p: &LiveParticle, anim: CellAnim) -> u8 {
    if row.ground_action == GroundAction::LavaSpot && p.grounded {
        return 114;
    }
    match anim {
        CellAnim::Fixed => p.cell0,
        CellAnim::SmokeFamily => {
            let step = p.age.saturating_mul(8) / p.max_age.max(1);
            7u8.saturating_sub(step.min(7) as u8)
        }
        CellAnim::SpellBase(base) => {
            let step = p.age.saturating_mul(8) / p.max_age.max(1);
            base.saturating_add(7u8.saturating_sub(step.min(7) as u8))
        }
        CellAnim::Wake => {
            let remaining = p.max_age.saturating_sub(p.age);
            (19 + (60u32.saturating_sub(remaining)) % 4) as u8
        }
        CellAnim::Drip => {
            if p.bob > 0 {
                113
            } else {
                112
            }
        }
    }
}

/// Resolves the wire data run to a sprite source; the arity check in
/// [`ParticleEngine::spawn_wire`] runs first, so the indices hold.
fn wire_tex(row: &ParticleBehaviour, data: &[i32]) -> ParticleTex {
    match row.tex_kind {
        TexKind::Cell { .. } | TexKind::Hidden => ParticleTex::Cell(0),
        TexKind::AtlasArg => ParticleTex::Atlas(data[0] as u16),
        TexKind::ItemArg => ParticleTex::ItemAtlas(data[0] as u32, data[1] as u32),
        TexKind::ItemFixed(item, meta) => ParticleTex::ItemAtlas(item, meta),
    }
}

/// Draws a lifetime from the engine `rand`.
fn draw_max_age(rng: &mut StdRng, shape: MaxAge) -> u32 {
    let mut unit = || rng.random::<f32>();
    match shape {
        MaxAge::Fixed(n) => n,
        MaxAge::EightOver => (8.0 / (unit() * 0.8 + 0.2)) as u32,
        MaxAge::SixteenOver => (16.0 / (unit() * 0.8 + 0.2)) as u32,
        MaxAge::Explode => (16.0 / (unit() * 0.8 + 0.2)) as u32 + 2,
        MaxAge::TwentyOver => (20.0 / (unit() * 0.8 + 0.2)) as u32,
        MaxAge::SixtyFourOver => (64.0 / (unit() * 0.8 + 0.2)) as u32,
        MaxAge::FourOver => (4.0 / (unit() * 0.9 + 0.1)) as u32,
        MaxAge::Flame => (8.0 / (unit() * 0.8 + 0.2)) as u32 + 4,
        MaxAge::Cloud => ((8.0 / (unit() * 0.8 + 0.3)) as u32 as f32 * 2.5) as u32,
        MaxAge::Crit => (6.0 / (unit() * 0.8 + 0.6)) as u32,
        MaxAge::Enchant => (unit() * 10.0) as u32 + 30,
        MaxAge::Portal => (unit() * 10.0) as u32 + 40,
        MaxAge::Spark => rng.random_range(0..12) + 48,
        MaxAge::LargeBoom => rng.random_range(0..4) + 6,
    }
}

/// Draws the spawn quad scale.
fn draw_size(rng: &mut StdRng, draw: SizeDraw) -> f32 {
    let base = (rng.random::<f32>() * 0.5 + 0.5) * 2.0;
    match draw {
        SizeDraw::Base(mult) => base * mult,
        SizeDraw::Explode => rng.random::<f32>() * rng.random::<f32>() * 6.0 + 1.0,
        SizeDraw::Portal => rng.random::<f32>() * 0.2 + 0.5,
        SizeDraw::Enchant => rng.random::<f32>() * 0.5 + 0.2,
        SizeDraw::Lava => base * (rng.random::<f32>() * 2.0 + 0.2),
        SizeDraw::Small => base * (rng.random::<f32>() * 0.6 + 0.2),
        SizeDraw::Wide => base * (rng.random::<f32>() * 0.6 + 0.5),
    }
}

/// Draws the spawn colour; `vel` is the pre-transform spawn velocity the
/// pitch/speed-tinted rows read.
fn draw_colour(rng: &mut StdRng, init: ColourInit, vel: [f32; 3]) -> [f32; 3] {
    match init {
        ColourInit::White => [1.0, 1.0, 1.0],
        ColourInit::GreyRange(lo, hi) => {
            let g = rng.random::<f32>() * (hi - lo) + lo;
            [g, g, g]
        }
        ColourInit::Digging => [0.6, 0.6, 0.6],
        ColourInit::NotePitch => {
            use std::f32::consts::PI;
            [
                (vel[0] * PI * 2.0).sin() * 0.65 + 0.35,
                ((vel[0] + 0.333_333_34) * PI * 2.0).sin() * 0.65 + 0.35,
                ((vel[0] + 0.666_666_7) * PI * 2.0).sin() * 0.65 + 0.35,
            ]
        }
        ColourInit::RedstoneSpeeds => {
            let f = rng.random::<f32>() * 0.4 + 0.6;
            let y = if vel[1] == 0.0 { 1.0 } else { vel[1] };
            let mut jitter = || rng.random::<f32>() * 0.2 + 0.8;
            [
                jitter() * y * f,
                jitter() * vel[2] * f,
                jitter() * vel[2] * f,
            ]
        }
        ColourInit::SpellSpeeds => vel,
        ColourInit::WitchMagenta => {
            let f = rng.random::<f32>() * 0.5 + 0.35;
            [f, 0.0, f]
        }
        ColourInit::DropWater => [0.2, 0.3, 1.0],
        ColourInit::DropLava => [1.0, 1.0, 0.5],
        ColourInit::PortalShade => {
            let f = rng.random::<f32>() * 0.6 + 0.4;
            [0.9 * f, 0.3 * f, f]
        }
        ColourInit::EnchantShade => {
            let f = rng.random::<f32>() * 0.6 + 0.4;
            [0.9 * f, 0.9 * f, f]
        }
        ColourInit::CritMagic => {
            let g = rng.random::<f32>() * 0.3 + 0.6;
            [g * 0.3, g * 0.8, g]
        }
    }
}

/// One standard-normal draw (Box-Muller) for the wire fan-out.
fn gauss(rng: &mut StdRng) -> f64 {
    let u1 = rng.random::<f64>().max(f64::MIN_POSITIVE);
    let u2 = rng.random::<f64>();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// The weather rain-drop count for one tick's rain pass.
///
/// `EntityRenderer.java:1533-1539`: `(int)(100*f*f)`, halved at
/// `Decreased` (`>>= 1`), zeroed at `Minimal`.
pub fn rain_drops(intensity: f32, setting: ParticleSetting) -> u32 {
    let mut count = (100.0 * intensity * intensity).trunc().max(0.0) as u32;
    match setting {
        ParticleSetting::All => {}
        ParticleSetting::Decreased => count >>= 1,
        ParticleSetting::Minimal => count = 0,
    }
    count
}

#[cfg(test)]
mod tests {
    //! The stepping, lifetime, cap and reduction fixtures.
    //!
    //! Each fixture pins a source-derived value: the rise steps, the flame
    //! shrink and brightness ramp, the bubble's out-of-water death, the
    //! splash gravity, the per-list cap, the one-in-three degrade, the
    //! 256-block² cull, the rain halve/zero.

    use super::*;
    use crate::particle_world::StillWorld;

    /// A dry, dark, groundless world: particles fall forever.
    fn air() -> StillWorld {
        StillWorld {
            water: false,
            light: 0,
            ground: None,
        }
    }

    /// A local spawn with the row defaults.
    fn local(id: u8, pos: [f32; 3], vel: [f32; 3]) -> ParticleSpawn {
        ParticleSpawn {
            id,
            pos,
            vel,
            colour: None,
            atlas_sprite: None,
            item: None,
        }
    }

    #[test]
    fn the_table_covers_all_forty_two_ids() {
        assert_eq!(BEHAVIOURS.len(), 42);
        assert_eq!(behaviour(26).expect("flame row").name, "flame");
        assert_eq!(behaviour(4).expect("bubble row").name, "bubble");
        assert_eq!(behaviour(23).expect("note row").name, "note");
        assert_eq!(behaviour(11).expect("smoke row").name, "smoke");
        assert!(behaviour(42).is_none());
    }

    #[test]
    fn smoke_rises_and_eventually_dies() {
        let world = air();
        let mut engine = ParticleEngine::seeded(1, 4000);
        assert!(engine.spawn_local(local(11, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0])));
        let y0 = engine.snapshot()[0].pos[1];
        engine.tick(&world);
        let y1 = engine.snapshot()[0].pos[1];
        assert!(y1 > y0, "smoke rises (motionY += 0.004): {y0} -> {y1}");
        for _ in 0..200 {
            engine.tick(&world);
        }
        assert_eq!(engine.stats().count, 0, "smoke ages out");
    }

    #[test]
    fn smoke_cells_animate_down_from_seven() {
        let world = air();
        let mut engine = ParticleEngine::seeded(1, 4000);
        assert!(engine.spawn_local(local(11, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0])));
        assert_eq!(engine.snapshot()[0].tex, ParticleTex::Cell(7));
        for _ in 0..8 {
            engine.tick(&world);
        }
        let cell = engine.snapshot()[0].tex;
        assert!(
            matches!(cell, ParticleTex::Cell(c) if c < 7),
            "smoke animates 7 - age*8/max: {cell:?}"
        );
    }

    #[test]
    fn flame_shrinks_and_ramps_to_full_bright() {
        let world = air();
        let mut engine = ParticleEngine::seeded(2, 4000);
        assert!(engine.spawn_local(local(26, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0])));
        let s0 = engine.snapshot()[0].size;
        let l0 = engine.snapshot()[0].light;
        for _ in 0..5 {
            engine.tick(&world);
        }
        let s1 = engine.snapshot()[0].size;
        let l1 = engine.snapshot()[0].light;
        assert!(
            s1 < s0,
            "flame shrinks (scale * (1 - f*f*0.5)): {s0} -> {s1}"
        );
        assert!(l1 > l0, "flame ramps to full brightness: {l0} -> {l1}");
        let mut peak = l1;
        for _ in 0..300 {
            engine.tick(&world);
            if let Some(frame) = engine.snapshot().first() {
                peak = peak.max(frame.light);
            }
        }
        assert_eq!(engine.stats().count, 0, "flame ages out");
        assert_eq!(peak, 15, "the ramp reaches full bright before age-out");
    }

    #[test]
    fn bubble_rises_in_water_and_dies_outside_it() {
        let sea = StillWorld {
            water: true,
            light: 0,
            ground: None,
        };
        let mut engine = ParticleEngine::seeded(4, 4000);
        assert!(engine.spawn_local(local(4, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0])));
        let y0 = engine.snapshot()[0].pos[1];
        engine.tick(&sea);
        let y1 = engine.snapshot()[0].pos[1];
        assert!(y1 > y0, "bubbles rise (motionY += 0.002): {y0} -> {y1}");

        let dry = air();
        let mut engine = ParticleEngine::seeded(4, 4000);
        assert!(engine.spawn_local(local(4, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0])));
        engine.tick(&dry);
        assert_eq!(
            engine.stats().count,
            0,
            "bubbles die when their block is not water"
        );
    }

    #[test]
    fn splash_arcs_under_gravity() {
        let world = air();
        let mut engine = ParticleEngine::seeded(8, 4000);
        assert!(engine.spawn_local(local(5, [0.0, 10.0, 0.0], [0.0, 2.0, 0.0])));
        engine.tick(&world);
        let y1 = engine.snapshot()[0].pos[1];
        engine.tick(&world);
        let y2 = engine.snapshot()[0].pos[1];
        let (d1, d2) = (y1 - 10.0, y2 - y1);
        assert!(d2 < d1, "gravity bends the arc (0.04/tick): {d1} then {d2}");
        for _ in 0..300 {
            engine.tick(&world);
        }
        let y_end = if engine.stats().count > 0 {
            engine.snapshot()[0].pos[1]
        } else {
            f32::NEG_INFINITY
        };
        assert!(y_end < 10.0, "the arc falls back past its spawn height");
    }

    #[test]
    fn block_dust_carries_the_dig_tint() {
        let world = air();
        let mut engine = ParticleEngine::seeded(6, 4000);
        assert!(engine.spawn_local(ParticleSpawn {
            id: 38,
            pos: [0.0, 10.0, 0.0],
            vel: [0.0, 0.0, 0.0],
            colour: Some([0.2, 0.5, 0.9]),
            atlas_sprite: None,
            item: None,
        }));
        engine.tick(&world);
        assert_eq!(engine.snapshot()[0].colour, [0.2, 0.5, 0.9, 1.0]);
    }

    #[test]
    fn note_rises_fast_and_expires_on_schedule() {
        let world = air();
        let mut engine = ParticleEngine::seeded(12, 4000);
        assert!(engine.spawn_local(local(23, [0.0, 10.0, 0.0], [1.0, 0.0, 0.0])));
        engine.tick(&world);
        let y1 = engine.snapshot()[0].pos[1];
        assert!(
            y1 - 10.0 > 0.15,
            "notes rise on their 0.2 initial lift: {}",
            y1 - 10.0
        );
        for _ in 0..5 {
            engine.tick(&world);
        }
        assert_eq!(engine.stats().count, 1, "a note lives six ticks");
        engine.tick(&world);
        assert_eq!(engine.stats().count, 0, "a note dies after its sixth tick");
    }

    #[test]
    fn a_full_list_evicts_oldest_first() {
        let mut engine = ParticleEngine::seeded(9, 2);
        for x in [0.0, 1.0, 2.0] {
            assert!(engine.spawn_local(local(11, [x, 10.0, 0.0], [0.0, 0.0, 0.0])));
        }
        assert_eq!(engine.stats().count, 2);
        let mut xs: Vec<f32> = engine.snapshot().iter().map(|f| f.pos[0]).collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs, [1.0, 2.0]);
    }

    #[test]
    fn the_snapshot_covers_the_visible_set_at_the_cap() {
        // The F3 `P:` guarantee: the snapshot emits every live particle the
        // stats call visible, and only those — past the per-list cap, with
        // hidden rows in the mix.
        let mut engine = ParticleEngine::seeded(13, PER_LIST_CAP);
        for i in 0..PER_LIST_CAP + 100 {
            let x = (i % 256) as f32;
            assert!(engine.spawn_local(local(39, [x, 10.0, 0.0], [0.0, 0.0, 0.0])));
        }
        for i in 0..10 {
            assert!(engine.spawn_local(local(40, [i as f32, 10.0, 0.0], [0.0, 0.0, 0.0])));
        }
        let stats = engine.stats();
        assert_eq!(
            stats.count,
            PER_LIST_CAP + 10,
            "the drops evict, the hidden rest"
        );
        assert_eq!(
            stats.visible, PER_LIST_CAP,
            "hidden rows never count as visible"
        );
        assert_eq!(
            engine.snapshot().len(),
            stats.visible,
            "the snapshot is exactly the drawn set"
        );
    }

    #[test]
    fn decreased_degrades_minimal_rejects_and_far_culls() {
        let world = air();
        let mut engine = ParticleEngine::seeded(7, 4000);
        engine.set_limit(ParticleSetting::Decreased);
        let mut added = 0;
        for i in 0..300 {
            added += engine.spawn_wire(
                11,
                false,
                [150.0 + (i % 33) as f32 - 16.0, 10.0, 0.0],
                [0.0, 0.0, 0.0],
                0.0,
                1,
                &[],
                [150.0, 10.0, 0.0],
                &world,
            );
        }
        assert!(
            (100..260).contains(&added),
            "one-in-three degrade keeps roughly two thirds: {added}"
        );
        let far = engine.spawn_wire(
            11,
            false,
            [100.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            1,
            &[],
            [0.0, 10.0, 0.0],
            &world,
        );
        assert_eq!(far, 0, "past 256 blocks² nothing spawns");
        engine.set_limit(ParticleSetting::Minimal);
        let min = engine.spawn_wire(
            11,
            false,
            [1.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            1,
            &[],
            [0.0, 10.0, 0.0],
            &world,
        );
        assert_eq!(min, 0, "minimal rejects ordinary rows");
        let far_but_ignoring = engine.spawn_wire(
            0,
            false,
            [100.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            1,
            &[],
            [0.0, 10.0, 0.0],
            &world,
        );
        assert_eq!(far_but_ignoring, 1, "ignore-range rows survive minimal");
    }

    #[test]
    fn the_rain_count_halves_then_zeroes() {
        use ParticleSetting::{All, Decreased, Minimal};
        assert_eq!(rain_drops(1.0, All), 100);
        assert_eq!(rain_drops(1.0, Decreased), 50);
        assert_eq!(rain_drops(1.0, Minimal), 0);
        assert_eq!(rain_drops(0.5, All), 25);
        assert_eq!(rain_drops(0.5, Decreased), 12);
        assert_eq!(rain_drops(0.0, All), 0);
    }

    #[test]
    fn a_zero_count_wire_spawn_emits_one_with_speed_times_offset_velocity() {
        let world = air();
        let mut engine = ParticleEngine::seeded(13, 4000);
        let added = engine.spawn_wire(
            11,
            false,
            [0.0, 10.0, 0.0],
            [1.0, 0.0, 0.0],
            2.0,
            0,
            &[],
            [0.0, 10.0, 0.0],
            &world,
        );
        assert_eq!(added, 1, "count 0 still spawns once");
        engine.tick(&world);
        let x = engine.snapshot()[0].pos[0];
        assert!(
            (x - 2.0).abs() < 1e-4,
            "velocity is speed * offset per axis: x = {x}"
        );
    }

    #[test]
    fn the_same_seed_replays_the_same_snapshots() {
        let world = air();
        let run = || {
            let mut engine = ParticleEngine::seeded(21, 4000);
            engine.spawn_wire(
                11,
                false,
                [0.0, 10.0, 0.0],
                [0.5, 0.5, 0.5],
                1.0,
                4,
                &[],
                [0.0, 10.0, 0.0],
                &world,
            );
            for _ in 0..7 {
                engine.tick(&world);
            }
            engine.snapshot()
        };
        assert_eq!(run(), run());
    }
}
