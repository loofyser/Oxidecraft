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

use oxide_world::entity::EntityKind;

use super::SoundCategory;

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

/// The landed-attack sound (`NetHandlerPlayClient.handleChangeGameState`
/// reason 6, `:1412`): fixed quiet and low at the player's eye.
pub const SUCCESSFUL_HIT_SOUND: &str = "random.successful_hit";

/// The guardian curse (`:1425`, reason 10): full volume at the player.
pub const GUARDIAN_CURSE_SOUND: &str = "mob.guardian.curse";

/// The curse's particle: the mob-appearance id the same arm spawns
/// (`EnumParticleTypes.MOB_APPEARANCE`).
pub const GUARDIAN_CURSE_PARTICLE_ID: i32 = 41;

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
    /// off planks, stone stair 67 sounds piston off cobblestone), the wall
    /// copies its model the same way (`BlockWall.java:37`), the daylight
    /// detectors sound wood out of their own constructor
    /// (`BlockDaylightDetector.java:37`), the piston bases and heads sound
    /// piston out of theirs (`BlockPistonBase.java:38`,
    /// `BlockPistonExtension.java:33`), and everything unnamed — water and
    /// lava, the note block, webs, the moving piston, monster eggs, warts,
    /// the enchanting table, the brewing stand, the cauldron, the end
    /// portal, tripwire and its hooks, the command block, the beacon, the
    /// flower pot, the barrier — keeps the constructor's stone.
    #[must_use]
    pub fn of_block(id: u16) -> SoundGroup {
        match id {
            2 | 6 | 18 | 19 | 31 | 32 | 37 | 38 | 39 | 40 | 46 | 59 | 83 | 106 | 110 | 111
            | 141 | 142 | 161 | 170 | 175 => SoundGroup::Grass,
            3 | 13 | 60 | 82 => SoundGroup::Gravel,
            5 | 17 | 26 | 47 | 50 | 53 | 54 | 58 | 63 | 64 | 68 | 69 | 72 | 75 | 76 | 85 | 86
            | 91 | 93 | 94 | 96 | 99 | 100 | 103 | 104 | 105 | 107 | 125 | 126 | 127 | 134
            | 135 | 136 | 143 | 146 | 147 | 148 | 149 | 150 | 151 | 162 | 163 | 164 | 176 | 177
            | 178 | 183 | 184 | 185 | 186 | 187 | 188 | 189 | 190 | 191 | 192 | 193 | 194 | 195
            | 196 | 197 => SoundGroup::Wood,
            1 | 4 | 7 | 14 | 15 | 16 | 21 | 22 | 23 | 24 | 29 | 33 | 34 | 43 | 44 | 45 | 48
            | 49 | 56 | 61 | 62 | 67 | 70 | 73 | 74 | 77 | 84 | 87 | 98 | 108 | 109 | 112 | 113
            | 114 | 121 | 122 | 128 | 129 | 130 | 139 | 144 | 153 | 155 | 156 | 158 | 159 | 168
            | 172 | 173 | 179 | 180 | 181 | 182 => SoundGroup::Piston,
            27 | 28 | 41 | 42 | 52 | 57 | 66 | 71 | 101 | 133 | 152 | 154 | 157 | 167 => {
                SoundGroup::Metal
            }
            20 | 79 | 89 | 90 | 95 | 102 | 120 | 123 | 124 | 160 | 169 | 174 => SoundGroup::Glass,
            35 | 51 | 81 | 92 | 171 => SoundGroup::Cloth,
            12 | 88 => SoundGroup::Sand,
            78 | 80 => SoundGroup::Snow,
            65 => SoundGroup::Ladder,
            145 => SoundGroup::Anvil,
            165 => SoundGroup::Slime,
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
    /// glass, anvil and slime overrides; the ladder inherits the break
    /// sound, `dig.wood`, since only `getBreakSound` is overridden,
    /// `Block.java:67-73`).
    #[must_use]
    pub fn place_sound(&self) -> &'static str {
        match self {
            SoundGroup::Glass => "step.stone",
            SoundGroup::Anvil => "random.anvil_land",
            SoundGroup::Slime => "mob.slime.big",
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

    /// The dig-hit click volume: `(volume + 1) / 8`
    /// (`PlayerControllerMP.java:316`).
    #[must_use]
    pub fn click_volume(&self) -> f32 {
        (self.volume() + 1.0) / 8.0
    }

    /// The dig-hit click pitch: `frequency * 0.5` (`:316`).
    #[must_use]
    pub fn click_pitch(&self) -> f32 {
        self.frequency() * 0.5
    }
}

/// The per-entity state a mob's hurt/death names read: the slime size, the
/// horse type and the guardian elder flag, each off the tracked metadata
/// with the spawn default behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobVariant {
    /// The slime size (`EntitySlime.getSlimeSize`, watcher byte 16;
    /// `entityInit:47-50` seeds 1).
    pub slime_size: i32,
    /// The horse type (`EntityHorse.getHorseType`, watcher byte 19;
    /// 0 horse, 1 donkey, 2 mule, 3 zombie, 4 skeleton).
    pub horse_type: i32,
    /// The guardian elder flag (`EntityGuardian.isElder`, watcher int 16
    /// bit 4).
    pub guardian_elder: bool,
}

impl Default for MobVariant {
    fn default() -> Self {
        Self {
            slime_size: 1,
            horse_type: 0,
            guardian_elder: false,
        }
    }
}

/// A living kind's hurt and death staging: the two event names, the volume
/// and the category hint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobSounds {
    /// The hurt event (`getHurtSound`).
    pub hurt: &'static str,
    /// The death event (`getDeathSound`).
    pub death: &'static str,
    /// The volume (`getSoundVolume`, default 1.0).
    pub volume: f32,
    /// The category hint: hostile kinds speak as mobs, the rest as animals,
    /// remote players as players.
    pub category: SoundCategory,
}

/// A kind's hurt/death sounds with their volume, or `None` for a kind the
/// source never sounds on status 2/3: the witch and the squid (null sounds),
/// the snow golem (`"none"`), and every non-living kind (the base
/// `Entity.handleStatusUpdate` is a no-op).
///
/// The names are the classes' own `getHurtSound` / `getDeathSound`; the
/// slime pair follows the tracked size, the horse pair the tracked type and
/// the guardian pair the elder flag. The guardian land variants stay an
/// approximation — the port tracks no per-entity water state — and so do the
/// enderman-scream, ocelot-love and wolf-anger living variants, which the
/// hurt/death arms never stage anyway.
#[must_use]
pub fn mob_sounds(kind: EntityKind, variant: &MobVariant) -> Option<MobSounds> {
    let (hurt, death, volume, category) = match kind {
        EntityKind::Player => (HURT_SOUND, DEATH_SOUND, 1.0, SoundCategory::Players),
        EntityKind::Creeper => (
            "mob.creeper.say",
            "mob.creeper.death",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Skeleton => (
            "mob.skeleton.hurt",
            "mob.skeleton.death",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Spider | EntityKind::CaveSpider => (
            "mob.spider.say",
            "mob.spider.death",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Giant => (
            "game.hostile.hurt",
            "game.hostile.die",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Zombie => (
            "mob.zombie.hurt",
            "mob.zombie.death",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Slime | EntityKind::LavaSlime => {
            let size = if variant.slime_size > 1 {
                "big"
            } else {
                "small"
            };
            let name: &'static str = if size == "big" {
                "mob.slime.big"
            } else {
                "mob.slime.small"
            };
            (
                name,
                name,
                0.4 * variant.slime_size as f32,
                SoundCategory::Mobs,
            )
        }
        EntityKind::Ghast => (
            "mob.ghast.scream",
            "mob.ghast.death",
            10.0,
            SoundCategory::Mobs,
        ),
        EntityKind::PigZombie => (
            "mob.zombiepig.zpighurt",
            "mob.zombiepig.zpigdeath",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Enderman => (
            "mob.endermen.hit",
            "mob.endermen.death",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Silverfish | EntityKind::Endermite => (
            "mob.silverfish.hit",
            "mob.silverfish.kill",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Blaze => ("mob.blaze.hit", "mob.blaze.death", 1.0, SoundCategory::Mobs),
        EntityKind::EnderDragon => (
            "mob.enderdragon.hit",
            "game.neutral.die",
            5.0,
            SoundCategory::Mobs,
        ),
        EntityKind::WitherBoss => (
            "mob.wither.hurt",
            "mob.wither.death",
            1.0,
            SoundCategory::Mobs,
        ),
        EntityKind::Bat => ("mob.bat.hurt", "mob.bat.death", 0.1, SoundCategory::Animals),
        EntityKind::Guardian => {
            if variant.guardian_elder {
                (
                    "mob.guardian.elder.hit",
                    "mob.guardian.elder.death",
                    1.0,
                    SoundCategory::Mobs,
                )
            } else {
                (
                    "mob.guardian.hit",
                    "mob.guardian.death",
                    1.0,
                    SoundCategory::Mobs,
                )
            }
        }
        EntityKind::Pig => ("mob.pig.say", "mob.pig.death", 1.0, SoundCategory::Animals),
        EntityKind::Sheep => (
            "mob.sheep.say",
            "mob.sheep.say",
            1.0,
            SoundCategory::Animals,
        ),
        EntityKind::Cow | EntityKind::MushroomCow => {
            ("mob.cow.hurt", "mob.cow.hurt", 0.4, SoundCategory::Animals)
        }
        EntityKind::Chicken => (
            "mob.chicken.hurt",
            "mob.chicken.hurt",
            1.0,
            SoundCategory::Animals,
        ),
        EntityKind::Wolf => (
            "mob.wolf.hurt",
            "mob.wolf.death",
            0.4,
            SoundCategory::Animals,
        ),
        EntityKind::Ozelot => ("mob.cat.hitt", "mob.cat.hitt", 0.4, SoundCategory::Animals),
        EntityKind::VillagerGolem => (
            "mob.irongolem.hit",
            "mob.irongolem.death",
            1.0,
            SoundCategory::Animals,
        ),
        EntityKind::EntityHorse => {
            let (hurt, death) = match variant.horse_type {
                3 => ("mob.horse.zombie.hit", "mob.horse.zombie.death"),
                4 => ("mob.horse.skeleton.hit", "mob.horse.skeleton.death"),
                1 | 2 => ("mob.horse.donkey.hit", "mob.horse.donkey.death"),
                _ => ("mob.horse.hit", "mob.horse.death"),
            };
            (hurt, death, 0.8, SoundCategory::Animals)
        }
        EntityKind::Rabbit => (
            "mob.rabbit.hurt",
            "mob.rabbit.death",
            1.0,
            SoundCategory::Animals,
        ),
        EntityKind::Villager => (
            "mob.villager.hit",
            "mob.villager.death",
            1.0,
            SoundCategory::Animals,
        ),
        EntityKind::Witch
        | EntityKind::Squid
        | EntityKind::SnowMan
        | EntityKind::Item
        | EntityKind::XpOrb
        | EntityKind::Arrow
        | EntityKind::Snowball
        | EntityKind::Egg
        | EntityKind::EnderPearl
        | EntityKind::EyeOfEnder
        | EntityKind::Potion
        | EntityKind::XpBottle
        | EntityKind::Firework
        | EntityKind::Fireball
        | EntityKind::SmallFireball
        | EntityKind::WitherSkull
        | EntityKind::Painting
        | EntityKind::ItemFrame
        | EntityKind::Boat
        | EntityKind::Minecart
        | EntityKind::Global
        | EntityKind::Unknown => return None,
    };
    Some(MobSounds {
        hurt,
        death,
        volume,
        category,
    })
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

#[cfg(test)]
mod tests {
    //! The census pin: the row-18 classification table as an assertion, so a
    //! future regression that drops a kind's hurt/death staging fails loudly.
    //! Every living kind names its hurt, death, volume and category; every
    //! kind the source never sounds on status 2/3 answers `None`.

    use super::{MobVariant, SoundCategory, mob_sounds};
    use oxide_world::entity::EntityKind;

    /// One census row: the kind, its hurt/death names, volume and category.
    fn sounded(
        kind: EntityKind,
        variant: &MobVariant,
        hurt: &str,
        death: &str,
        volume: f32,
        category: SoundCategory,
    ) {
        let sounds = mob_sounds(kind, variant).unwrap_or_else(|| panic!("{kind:?} stays sounded"));
        assert_eq!(sounds.hurt, hurt, "{kind:?} hurt");
        assert_eq!(sounds.death, death, "{kind:?} death");
        assert_eq!(sounds.volume, volume, "{kind:?} volume");
        assert_eq!(sounds.category, category, "{kind:?} category");
    }

    #[test]
    fn the_mob_sound_table_covers_every_living_kind() {
        use EntityKind::{
            Bat, Blaze, CaveSpider, Chicken, Cow, Creeper, EnderDragon, Enderman, Endermite,
            EntityHorse, Ghast, Giant, Guardian, LavaSlime, MushroomCow, Ozelot, Pig, PigZombie,
            Player, Rabbit, Sheep, Silverfish, Skeleton, Slime, SnowMan, Spider, Squid, Villager,
            VillagerGolem, Witch, WitherBoss, Wolf, Zombie,
        };
        use SoundCategory::{Animals, Mobs, Players};
        let plain = MobVariant::default();
        sounded(
            Player,
            &plain,
            "game.player.hurt",
            "game.player.die",
            1.0,
            Players,
        );
        sounded(
            Creeper,
            &plain,
            "mob.creeper.say",
            "mob.creeper.death",
            1.0,
            Mobs,
        );
        sounded(
            Skeleton,
            &plain,
            "mob.skeleton.hurt",
            "mob.skeleton.death",
            1.0,
            Mobs,
        );
        sounded(
            Spider,
            &plain,
            "mob.spider.say",
            "mob.spider.death",
            1.0,
            Mobs,
        );
        sounded(
            CaveSpider,
            &plain,
            "mob.spider.say",
            "mob.spider.death",
            1.0,
            Mobs,
        );
        sounded(
            Giant,
            &plain,
            "game.hostile.hurt",
            "game.hostile.die",
            1.0,
            Mobs,
        );
        sounded(
            Zombie,
            &plain,
            "mob.zombie.hurt",
            "mob.zombie.death",
            1.0,
            Mobs,
        );
        sounded(
            Slime,
            &plain,
            "mob.slime.small",
            "mob.slime.small",
            0.4,
            Mobs,
        );
        sounded(
            LavaSlime,
            &plain,
            "mob.slime.small",
            "mob.slime.small",
            0.4,
            Mobs,
        );
        sounded(
            Slime,
            &MobVariant {
                slime_size: 4,
                ..plain
            },
            "mob.slime.big",
            "mob.slime.big",
            1.6,
            Mobs,
        );
        sounded(
            Ghast,
            &plain,
            "mob.ghast.scream",
            "mob.ghast.death",
            10.0,
            Mobs,
        );
        sounded(
            PigZombie,
            &plain,
            "mob.zombiepig.zpighurt",
            "mob.zombiepig.zpigdeath",
            1.0,
            Mobs,
        );
        sounded(
            Enderman,
            &plain,
            "mob.endermen.hit",
            "mob.endermen.death",
            1.0,
            Mobs,
        );
        sounded(
            Silverfish,
            &plain,
            "mob.silverfish.hit",
            "mob.silverfish.kill",
            1.0,
            Mobs,
        );
        sounded(
            Endermite,
            &plain,
            "mob.silverfish.hit",
            "mob.silverfish.kill",
            1.0,
            Mobs,
        );
        sounded(Blaze, &plain, "mob.blaze.hit", "mob.blaze.death", 1.0, Mobs);
        sounded(
            EnderDragon,
            &plain,
            "mob.enderdragon.hit",
            "game.neutral.die",
            5.0,
            Mobs,
        );
        sounded(
            WitherBoss,
            &plain,
            "mob.wither.hurt",
            "mob.wither.death",
            1.0,
            Mobs,
        );
        sounded(Bat, &plain, "mob.bat.hurt", "mob.bat.death", 0.1, Animals);
        sounded(
            Guardian,
            &plain,
            "mob.guardian.hit",
            "mob.guardian.death",
            1.0,
            Mobs,
        );
        sounded(
            Guardian,
            &MobVariant {
                guardian_elder: true,
                ..plain
            },
            "mob.guardian.elder.hit",
            "mob.guardian.elder.death",
            1.0,
            Mobs,
        );
        sounded(Pig, &plain, "mob.pig.say", "mob.pig.death", 1.0, Animals);
        sounded(
            Sheep,
            &plain,
            "mob.sheep.say",
            "mob.sheep.say",
            1.0,
            Animals,
        );
        sounded(Cow, &plain, "mob.cow.hurt", "mob.cow.hurt", 0.4, Animals);
        sounded(
            MushroomCow,
            &plain,
            "mob.cow.hurt",
            "mob.cow.hurt",
            0.4,
            Animals,
        );
        sounded(
            Chicken,
            &plain,
            "mob.chicken.hurt",
            "mob.chicken.hurt",
            1.0,
            Animals,
        );
        sounded(
            Wolf,
            &plain,
            "mob.wolf.hurt",
            "mob.wolf.death",
            0.4,
            Animals,
        );
        sounded(Ozelot, &plain, "mob.cat.hitt", "mob.cat.hitt", 0.4, Animals);
        sounded(
            VillagerGolem,
            &plain,
            "mob.irongolem.hit",
            "mob.irongolem.death",
            1.0,
            Animals,
        );
        sounded(
            EntityHorse,
            &plain,
            "mob.horse.hit",
            "mob.horse.death",
            0.8,
            Animals,
        );
        sounded(
            EntityHorse,
            &MobVariant {
                horse_type: 1,
                ..plain
            },
            "mob.horse.donkey.hit",
            "mob.horse.donkey.death",
            0.8,
            Animals,
        );
        sounded(
            EntityHorse,
            &MobVariant {
                horse_type: 3,
                ..plain
            },
            "mob.horse.zombie.hit",
            "mob.horse.zombie.death",
            0.8,
            Animals,
        );
        sounded(
            EntityHorse,
            &MobVariant {
                horse_type: 4,
                ..plain
            },
            "mob.horse.skeleton.hit",
            "mob.horse.skeleton.death",
            0.8,
            Animals,
        );
        sounded(
            Rabbit,
            &plain,
            "mob.rabbit.hurt",
            "mob.rabbit.death",
            1.0,
            Animals,
        );
        sounded(
            Villager,
            &plain,
            "mob.villager.hit",
            "mob.villager.death",
            1.0,
            Animals,
        );
        // The null-`"none"`-sounded and the non-living: silence, as the
        // source's own arms stage nothing for them.
        for kind in [
            Witch,
            Squid,
            SnowMan,
            EntityKind::Item,
            EntityKind::XpOrb,
            EntityKind::Arrow,
            EntityKind::Snowball,
            EntityKind::Egg,
            EntityKind::EnderPearl,
            EntityKind::EyeOfEnder,
            EntityKind::Potion,
            EntityKind::XpBottle,
            EntityKind::Firework,
            EntityKind::Fireball,
            EntityKind::SmallFireball,
            EntityKind::WitherSkull,
            EntityKind::Painting,
            EntityKind::ItemFrame,
            EntityKind::Boat,
            EntityKind::Minecart,
            EntityKind::Global,
            EntityKind::Unknown,
        ] {
            assert!(mob_sounds(kind, &plain).is_none(), "{kind:?} stays silent");
        }
    }
}
