//! The local sound sites: the block sound groups and the call-site staging
//! rules.
//!
//! Every value traces to the decompiled 1.8.9 client under
//! `refs/_src/MCP-919/src/minecraft/net/minecraft/`; only names and values
//! are carried over, no source text is reproduced.
//!
//! A block's group is its `Block.SoundType` instance (`Block.java:43-101`)
//! with the anonymous-subclass overrides (`:1515-1553`), resolved per block
//! through its own `stepSound` assignment (`Block.java`'s `registerBlock`
//! lines, the block classes' constructors, and `BlockStairs`' model copy at
//! `BlockStairs.java:47`). A block the table does not name falls back to the
//! constructor's own default, stone (`Block.java:289`).

/// The hurt sound (`EntityPlayer.java:747-750`).
pub const HURT_SOUND: &str = "game.player.hurt";

/// The death sound (`EntityPlayer.java:755-758`).
pub const DEATH_SOUND: &str = "game.player.die";

/// The item pickup sound
/// (`NetHandlerPlayClient.handleCollectItem:819-844`).
pub const PICKUP_SOUND: &str = "random.pop";

/// The experience-orb pickup sound (`:819-844` plays it for `EntityXPOrb`).
pub const ORB_SOUND: &str = "random.orb";

/// The level-up sound (`EntityPlayer.addExperienceLevel:2037-2054`).
pub const LEVEL_SOUND: &str = "random.levelup";

/// The own player's swim sound (`EntityPlayer.java:423-425`).
pub const SWIM_SOUND: &str = "game.player.swim";

/// The own player's splash sound (`EntityPlayer.java:427-430`).
pub const SPLASH_SOUND: &str = "game.player.swim.splash";

/// The ticks a level-up stays quiet after the previous one
/// (`EntityPlayer.java:2047`: `lastXPSound < ticksExisted - 100`).
pub const LEVEL_SOUND_COOLDOWN: u64 = 100;

/// A block's sound group: one `Block.SoundType` instance plus its
/// anonymous-subclass overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundGroup {
    /// The `stone` type at full volume and pitch.
    Stone,
    /// The `stone` type through `soundTypePiston`: the same events as
    /// [`SoundGroup::Stone`].
    Piston,
    /// The `wood` type.
    Wood,
    /// The `gravel` type.
    Gravel,
    /// The `grass` type.
    Grass,
    /// The `stone` type at 1.5 pitch (`soundTypeMetal`).
    Metal,
    /// The `stone` type breaking as glass and placing as stone
    /// (`soundTypeGlass`).
    Glass,
    /// The `cloth` type.
    Cloth,
    /// The `sand` type.
    Sand,
    /// The `snow` type.
    Snow,
    /// The `ladder` type, breaking as wood (`soundTypeLadder`).
    Ladder,
    /// The `anvil` type at 0.3 volume, breaking as stone and placing the
    /// anvil land (`soundTypeAnvil`).
    Anvil,
    /// The `slime` type, breaking and placing big and stepping small
    /// (`SLIME_SOUND`).
    Slime,
}

impl SoundGroup {
    /// The group a block id sounds through: its own `stepSound` assignment,
    /// or stone for an id the table does not name.
    ///
    /// The assignments are the `registerBlock` lines' `setStepSound` calls
    /// with the classes' own constructors behind them: logs sound wood
    /// (`BlockLog.java:22`), leaves sound grass (`BlockLeaves.java:34`),
    /// crops, stems, flowers, mushrooms, reeds, vines, lilies, mycelium,
    /// double plants, tall grass and dead bushes sound grass, stairs copy
    /// their model block (`BlockStairs.java:47` — oak stair 53 sounds wood
    /// off planks, stone stair 67 sounds piston off cobblestone), and
    /// everything unnamed — water and lava, pumpkins, the enchanting table,
    /// the cauldron, the beacon, the barrier — keeps the constructor's
    /// stone.
    #[must_use]
    pub fn of_block(id: u16) -> SoundGroup {
        match id {
            2 | 18 | 31 | 32 | 37 | 38 | 39 | 40 | 46 | 59 | 83 | 106 | 110 | 111 | 141 | 142
            | 161 | 175 => SoundGroup::Grass,
            3 | 13 | 60 | 82 => SoundGroup::Gravel,
            5 | 17 | 47 | 50 | 53 | 54 | 58 | 63 | 64 | 68 | 72 | 75 | 76 | 85 | 93 | 94 | 96
            | 99 | 100 | 104 | 105 | 107 | 162 => SoundGroup::Wood,
            1 | 4 | 7 | 14 | 15 | 16 | 21 | 22 | 23 | 24 | 43 | 44 | 45 | 48 | 49 | 56 | 61
            | 62 | 67 | 70 | 73 | 74 | 77 | 84 | 87 | 97 | 98 | 113 | 121 | 129 | 130 | 144
            | 153 | 155 | 156 | 158 | 159 | 168 => SoundGroup::Piston,
            41 | 42 | 52 | 57 | 101 | 133 | 154 | 157 | 167 => SoundGroup::Metal,
            20 | 79 | 89 | 90 | 95 | 102 | 120 | 123 | 124 | 160 | 169 => SoundGroup::Glass,
            35 | 51 | 81 | 92 => SoundGroup::Cloth,
            12 | 88 => SoundGroup::Sand,
            78 | 80 => SoundGroup::Snow,
            65 => SoundGroup::Ladder,
            145 => SoundGroup::Anvil,
            _ => SoundGroup::Stone,
        }
    }

    /// The break event: `getBreakSound` (`Block.java:1538-1541` plus the
    /// overrides at `:55-58`, `:69-72`, `:76-79` and `:87-95`).
    #[must_use]
    pub fn break_sound(&self) -> &'static str {
        match self {
            SoundGroup::Glass => "dig.glass",
            SoundGroup::Ladder => "dig.wood",
            SoundGroup::Anvil => "dig.stone",
            SoundGroup::Slime => "mob.slime.big",
            SoundGroup::Stone | SoundGroup::Piston | SoundGroup::Metal => "dig.stone",
            SoundGroup::Wood => "dig.wood",
            SoundGroup::Gravel => "dig.gravel",
            SoundGroup::Grass => "dig.grass",
            SoundGroup::Cloth => "dig.cloth",
            SoundGroup::Sand => "dig.sand",
            SoundGroup::Snow => "dig.snow",
        }
    }

    /// The step event: `getStepSound` (`Block.java:1543-1546` plus the slime
    /// override).
    #[must_use]
    pub fn step_sound(&self) -> &'static str {
        match self {
            SoundGroup::Slime => "mob.slime.small",
            SoundGroup::Ladder => "step.ladder",
            SoundGroup::Anvil => "step.anvil",
            SoundGroup::Stone | SoundGroup::Piston | SoundGroup::Metal | SoundGroup::Glass => {
                "step.stone"
            }
            SoundGroup::Wood => "step.wood",
            SoundGroup::Gravel => "step.gravel",
            SoundGroup::Grass => "step.grass",
            SoundGroup::Cloth => "step.cloth",
            SoundGroup::Sand => "step.sand",
            SoundGroup::Snow => "step.snow",
        }
    }

    /// The place event: `getPlaceSound` (`Block.java:1548-1551` plus the
    /// glass, anvil and slime overrides).
    #[must_use]
    pub fn place_sound(&self) -> &'static str {
        match self {
            SoundGroup::Glass => "step.stone",
            SoundGroup::Anvil => "random.anvil_land",
            SoundGroup::Slime => "mob.slime.big",
            SoundGroup::Ladder => "dig.ladder",
            other => other.break_sound(),
        }
    }

    /// The group's volume (`getVolume`).
    #[must_use]
    pub fn volume(&self) -> f32 {
        match self {
            SoundGroup::Anvil => 0.3,
            _ => 1.0,
        }
    }

    /// The group's pitch (`getFrequency`).
    #[must_use]
    pub fn frequency(&self) -> f32 {
        match self {
            SoundGroup::Metal => 1.5,
            _ => 1.0,
        }
    }

    /// The break/place volume: `(volume + 1) / 2`
    /// (`RenderGlobal.java:2280`, `ItemBlock.java:71`).
    #[must_use]
    pub fn break_volume(&self) -> f32 {
        (self.volume() + 1.0) / 2.0
    }

    /// The break/place pitch: `frequency * 0.8` (`:2280`, `:71`).
    #[must_use]
    pub fn break_pitch(&self) -> f32 {
        self.frequency() * 0.8
    }

    /// The footstep volume: `volume * 0.15` (`Entity.java:986-998`).
    #[must_use]
    pub fn step_volume(&self) -> f32 {
        self.volume() * 0.15
    }
}

/// The hurt/death pitch: `(first - second) * 0.2 + 1.0`
/// (`EntityLivingBase.handleStatusUpdate:1364-1379`, `getSoundPitch:1501`).
#[must_use]
pub fn hurt_pitch(first: f32, second: f32) -> f32 {
    (first - second) * 0.2 + 1.0
}

/// The pickup pitch: `((first - second) * 0.7 + 1.0) * 2.0`
/// (`NetHandlerPlayClient.java:838`, `EntityItem.java:409`).
#[must_use]
pub fn pickup_pitch(first: f32, second: f32) -> f32 {
    ((first - second) * 0.7 + 1.0) * 2.0
}

/// The swim/splash pitch: `1.0 + (first - second) * 0.4`
/// (`Entity.java:888`, `:1144`).
#[must_use]
pub fn swim_pitch(first: f32, second: f32) -> f32 {
    1.0 + (first - second) * 0.4
}

/// The displacement term a motion vector stages: the horizontal axes at
/// `0.2` with the vertical whole (`Entity.java:883`,
/// `resetHeight:1137`).
fn motion_term(motion: [f64; 3]) -> f32 {
    ((motion[0] * motion[0] * 0.2 + motion[1] * motion[1] + motion[2] * motion[2] * 0.2) as f32)
        .sqrt()
}

/// The splash volume off a motion vector: the displacement term times `0.2`,
/// capped at `1.0` (`Entity.resetHeight:1136-1144`).
#[must_use]
pub fn splash_volume(motion: [f64; 3]) -> f32 {
    (motion_term(motion) * 0.2).min(1.0)
}

/// The swim-step volume off a motion vector: the displacement term times
/// `0.35`, capped at `1.0` (`Entity.java:883-888`).
#[must_use]
pub fn swim_volume(motion: [f64; 3]) -> f32 {
    (motion_term(motion) * 0.35).min(1.0)
}

/// The level-up volume off a level: `level / 30`, or `1.0` past 30, times
/// `0.75` (`EntityPlayer.java:2049-2051`).
#[must_use]
pub fn level_volume(level: i32) -> f32 {
    let full = if level > 30 { 1.0 } else { level as f32 / 30.0 };
    full * 0.75
}

/// Whether a level gain sounds: the level rose, it lands on five, and the
/// cooldown since the previous level-up passed
/// (`EntityPlayer.addExperienceLevel:2047`).
#[must_use]
pub fn level_sounds(previous: i32, level: i32, tick: u64, last_sound_tick: u64) -> bool {
    level > previous
        && level % 5 == 0
        && tick.saturating_sub(last_sound_tick) > LEVEL_SOUND_COOLDOWN
}
