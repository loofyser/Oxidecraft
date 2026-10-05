//! The entity view: the session's per-tick entity feed, and the draws a frame builds
//! from it.
//!
//! The view keeps the latest [`ClientEvent::EntitiesTick`] frames and the instant they
//! arrived, and interpolates them at the same frame fraction the player pose uses:
//! the elapsed time over the fifty-millisecond tick, clamped to one. A frame builds
//! one draw per tracked entity ([`View::entity_draws`]) — the window's own entity
//! skipped — reading the stored frames and the skin worker's updates only; nothing
//! looks at the world, which the window does not hold
//! (`RendererLivingEntity.doRender`'s interpolated terms are the model here).
//!
//! The same session that feeds the entities feeds the chat: [`ClientEvent::Chat`]
//! messages land in the mirror ([`ChatView`]), whose [`ChatLog`] holds the split lines,
//! the fade clocks and the scroll state, and whose [`ChatView::draws`] assembles the
//! frame's hud draw list at the scaled resolution — the box the source draws at
//! `GuiNewChat.drawChat`:30-114 and the record line above the hotbar
//! (`GuiIngame.java`:245-272).

use std::collections::BTreeMap;
use std::time::Instant;

use oxide_assets::font::Font;
use oxide_assets::skins::{DefaultModel, default_skin};
use oxide_game::chat::{
    self, CHAT_WIDTH, ChatLog, LOG_CAP, STYLE_BOLD, STYLE_ITALIC, STYLE_OBFUSCATED,
    STYLE_STRIKETHROUGH, STYLE_UNDERLINED, TextComponent,
};
use oxide_game::entity_view::{EntityExtra, EntityFrame, MobExtra};
use oxide_game::session::ClientEvent;
use oxide_render::entity_models::player::CapeMotion;
use oxide_render::entity_models::{Pose, PoseExtra, objects};
use oxide_render::entity_pass::{
    DrawExtra, EntityDraw, FrameContent, ModelRef, NametagDraw, TextureRef,
};
use oxide_render::hud::{HudDraw, ScaledResolution};
use oxide_render::text::string_width;
use oxide_world::entity::EntityKind;

use crate::items;
use crate::skin_worker::SkinUpdate;

/// The all-on parts byte every player draws with this milestone.
///
/// The byte is the player's model-parts set (`EnumPlayerModelParts`); the wire's
/// skin-flags metadata carries a player's own, and the local settings screen its
/// owner's — neither channel exists yet, so the window draws every player with every
/// part enabled (`RenderPlayer.java:81-86` gates each overlay through `isWearing`).
pub const ALL_PARTS: u8 = 0x7F;

/// The tick's step, in seconds: the fraction's divisor (the camera's own 50 ms rule).
const TICK_SECONDS: f32 = 0.05;

/// The tick-to-tick step past which the draw's position snaps instead of sliding: the
/// same four-block rule the camera's pose interpolation pins (`Render.doRender`'s
/// teleport class).
const SNAP_BLOCKS: f64 = 4.0;

/// The window's entity frame state: the feed, its arrival, and the self entity.
pub struct View {
    /// The frames the latest [`ClientEvent::EntitiesTick`] carried, in its own order.
    frames: Vec<oxide_game::entity_view::EntityFrame>,
    /// When that tick arrived, for the frame fraction.
    arrival: Option<Instant>,
    /// The window's own entity id, from [`ClientEvent::Joined`]; skipped when drawing.
    own: Option<i32>,
}

impl View {
    /// A view with no feed.
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            arrival: None,
            own: None,
        }
    }

    /// Folds one session event in: the join records the window's own entity, the tick
    /// feed stores the frames and stamps their arrival.
    pub fn apply(&mut self, event: &ClientEvent) {
        match event {
            ClientEvent::Joined { entity_id, .. } => self.own = Some(*entity_id),
            ClientEvent::EntitiesTick { entities } => {
                self.observe(entities.clone(), Instant::now());
            }
            _ => {}
        }
    }

    /// Stores a feed and its arrival instant — the seam the tests drive the fraction
    /// through.
    fn observe(&mut self, frames: Vec<oxide_game::entity_view::EntityFrame>, arrival: Instant) {
        self.frames = frames;
        self.arrival = Some(arrival);
    }

    /// The draws for a frame at `now`: one per tracked entity but the window's own,
    /// interpolated between the stored tick's pairs.
    ///
    /// The fraction is the elapsed time since the feed's arrival over the fifty-millisecond
    /// tick, clamped to one — the same fraction the player pose uses. Each frame's position
    /// slides between its pair unless the step is a teleport (over [`SNAP_BLOCKS`]), its
    /// rotations interpolate the wrapped difference (`RendererLivingEntity.java:99-100`
    /// `interpolateRotation`), and the head's net yaw is the head's interpolated turn minus
    /// the body's. The pose carries the source's own terms: `limbSwing - limbSwingAmount *
    /// (1 - partial)` and the eased limb amount (`RendererLivingEntity.doRender`), the age,
    /// the swing grid's interpolated step (`EntityLivingBase.getSwingProgress`), and the
    /// damage and death fractions the pass gates on — one while either window is open, and
    /// the clamped square root of the source's twenty-tick ramp
    /// (`RendererLivingEntity.rotateCorpse`).
    pub fn entity_draws(
        &self,
        now: Instant,
        skins: &BTreeMap<String, SkinUpdate>,
    ) -> Vec<EntityDraw> {
        let Some(arrival) = self.arrival else {
            return Vec::new();
        };
        let partial =
            (now.saturating_duration_since(arrival).as_secs_f32() / TICK_SECONDS).clamp(0.0, 1.0);
        let mut draws = Vec::new();
        for frame in &self.frames {
            if Some(frame.id) == self.own {
                continue;
            }
            let Some(draw) = draw_for(frame, partial, skins) else {
                continue;
            };
            draws.push(draw);
        }
        draws
    }
}

/// The draw's own texture for the item geometries: the generated shape and the block
/// meshes carry their own sheets, so the draw's `texture` field stays a present-but-unused
/// placeholder — the always-uploaded shadow sprite stands in.
const ITEM_DRAW_PLACEHOLDER: &str = "misc/shadow.png";

/// The thrown-item sheet a projectile kind draws, when the object set covers it
/// (`RenderManager.java`:177-183).
fn projectile_sprite(kind: EntityKind) -> Option<&'static str> {
    match kind {
        EntityKind::Snowball => Some("items/snowball"),
        EntityKind::Egg => Some("items/egg"),
        EntityKind::EnderPearl => Some("items/ender_pearl"),
        EntityKind::EyeOfEnder => Some("items/ender_eye"),
        EntityKind::Potion => Some("items/potion_bottle_drinkable"),
        EntityKind::XpBottle => Some("items/experience_bottle"),
        EntityKind::Firework => Some("items/fireworks"),
        _ => None,
    }
}

/// The draw one frame makes, or `None` for a kind with no model yet and for an invisible
/// entity.
///
/// The model, texture and slim flag come from the skin worker's update for the profile
/// when one exists — as it stands — else from the uuid's own default model rule
/// (`DefaultPlayerSkin.isSlimSkin`, `DefaultPlayerSkin.java:41-44`); the renderer's
/// resolver falls back the same way when the update carries no texture. The cape layer's
/// wave reads the frame pair's displacement between its ticks — the window's stand-in for
/// the smoothed chaser and camera-yaw terms its state cannot produce ([`CapeMotion`]). An
/// invisible entity yields no draw, model and shadow alike: the source skips both for one
/// (`RendererLivingEntity.java:248-249`, `Render.java:303`), and the window's shadow rides
/// the same draw.
fn draw_for(
    frame: &EntityFrame,
    partial: f32,
    skins: &BTreeMap<String, SkinUpdate>,
) -> Option<EntityDraw> {
    if frame.invisible {
        return None;
    }
    let position = interpolate_position(frame.prev, frame.pos, partial);
    let body_yaw = interpolate_rotation(
        frame.prev_render_yaw_offset,
        frame.render_yaw_offset,
        partial,
    );
    let head = interpolate_rotation(frame.prev_head_yaw, frame.head_yaw, partial);
    let head_yaw = head - body_yaw;
    let head_pitch = frame.prev_pitch + (frame.pitch - frame.prev_pitch) * partial;

    let swing = {
        let mut step = frame.swing_progress - frame.prev_swing_progress;
        if step < 0.0 {
            step += 1.0;
        }
        frame.prev_swing_progress + step * partial
    };
    // The source's damage overlay opens while the hurt window or the death animation runs
    // (`RendererLivingEntity.doRender`'s `hurtTime > 0 || deathTime > 0`); the death ramp
    // is the clamped square root of `((deathTime + partial - 1) / 20) * 1.6`.
    let hurt = if frame.hurt_ticks > 0 || frame.death_ticks > 0 {
        1.0
    } else {
        0.0
    };
    let death = if frame.death_ticks > 0 {
        (((f32::from(frame.death_ticks) + partial - 1.0) / 20.0) * 1.6)
            .sqrt()
            .min(1.0)
    } else {
        0.0
    };

    // The kind's own channel: a player's skin and cape terms, a mob's model, sheet and
    // extras. The kinds with neither draw nothing.
    let (model, texture, draw_extra, pose_extra, child) = match &frame.extra {
        EntityExtra::Player => {
            let Some(uuid) = frame.uuid.clone() else {
                tracing::debug!(
                    id = frame.id,
                    "a player without a profile cannot be looked up"
                );
                return None;
            };
            let slim = match skins.get(&uuid) {
                Some(update) => update.model == DefaultModel::Slim,
                None => default_skin(&uuid) == DefaultModel::Slim,
            };
            // The cape layer's wave reads the frame pair's own displacement — the window's
            // stand-in for the smoothed chaser and camera-yaw terms its state cannot
            // produce (`CapeMotion`).
            (
                ModelRef::Player {
                    slim,
                    parts: ALL_PARTS,
                },
                TextureRef::Skin { uuid, slim },
                DrawExtra::None,
                PoseExtra::Player(CapeMotion {
                    motion: [
                        (frame.pos[0] - frame.prev[0]) as f32,
                        (frame.pos[1] - frame.prev[1]) as f32,
                        (frame.pos[2] - frame.prev[2]) as f32,
                    ],
                }),
                false,
            )
        }
        EntityExtra::Mob(mob) => match mob_draw(frame.kind, frame.id, mob) {
            Some(terms) => terms,
            None => {
                tracing::debug!(kind = ?frame.kind, "the entity kind has no model yet");
                return None;
            }
        },
        EntityExtra::Item { id, count, damage } => {
            let model = match items::resolve(*id, *damage) {
                items::ItemResolution::Block(block) => ModelRef::BlockItem { block },
                items::ItemResolution::Sprite(key) => ModelRef::Sprite { key },
                items::ItemResolution::Missing => {
                    tracing::debug!(id, "the item id has no model");
                    return None;
                }
            };
            (
                model,
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Item {
                    id: *id,
                    count: *count,
                    damage: *damage,
                },
                PoseExtra::None,
                false,
            )
        }
        EntityExtra::Painting { title, facing } => (
            ModelRef::Painting {
                art: objects::art_index(title) as u8,
            },
            TextureRef::Named(objects::PAINTING_TEXTURE),
            DrawExtra::Painting { facing: *facing },
            PoseExtra::None,
            false,
        ),
        EntityExtra::ItemFrame { item, rotation } => {
            let content = match item {
                None => FrameContent::Empty,
                Some(stack) => match items::resolve(stack.id, stack.damage) {
                    items::ItemResolution::Block(block) => FrameContent::Block(block),
                    items::ItemResolution::Sprite(key) => FrameContent::Sprite(key),
                    items::ItemResolution::Missing => FrameContent::Empty,
                },
            };
            (
                ModelRef::ItemFrame { content },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Frame {
                    rotation: *rotation,
                },
                PoseExtra::None,
                false,
            )
        }
        EntityExtra::Boat => (
            ModelRef::Boat,
            TextureRef::Named(objects::BOAT_TEXTURE),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        EntityExtra::Minecart => (
            ModelRef::Minecart { body: 0 },
            TextureRef::Named(objects::MINECART_TEXTURE),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        EntityExtra::Orb => (
            ModelRef::Orb { value: 1 },
            TextureRef::Named(objects::ORB_TEXTURE),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        EntityExtra::Projectile => match frame.kind {
            EntityKind::Arrow => (
                ModelRef::Arrow,
                TextureRef::Named(objects::ARROW_TEXTURE),
                DrawExtra::None,
                PoseExtra::None,
                false,
            ),
            EntityKind::Fireball => (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Projectile {
                    billboard: objects::Billboard::Fireball,
                    scale: 2.0,
                },
                PoseExtra::None,
                false,
            ),
            // The blaze's small fireball draws under the same class at its own
            // registration scale (`RenderManager.java`:185).
            EntityKind::SmallFireball => (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Projectile {
                    billboard: objects::Billboard::Fireball,
                    scale: 0.5,
                },
                PoseExtra::None,
                false,
            ),
            EntityKind::WitherSkull => (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Projectile {
                    billboard: objects::Billboard::Fireball,
                    scale: 1.0,
                },
                PoseExtra::None,
                false,
            ),
            kind => {
                let Some(key) = projectile_sprite(kind) else {
                    tracing::debug!(kind = ?kind, "the projectile has no sprite");
                    return None;
                };
                (
                    ModelRef::Sprite { key },
                    TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                    DrawExtra::Projectile {
                        billboard: objects::Billboard::Snowball,
                        scale: 0.5,
                    },
                    PoseExtra::None,
                    false,
                )
            }
        },
        other => {
            tracing::debug!(
                kind = ?frame.kind,
                extra = ?other,
                "the entity kind has no model yet"
            );
            return None;
        }
    };

    // The dragon's flight clock rides the frame's age at the source's at-rest rate
    // (`EntityDragon.onLivingUpdate`:158-167): the wing wave advances a fifth of a tick.
    // The slowed flag's halving, the motion factor and the AI-disabled `0.5` lock are not
    // carried by the frames, so the clock runs at the at-rest rate (the exotics ledger
    // records the same pins).
    let pose_extra = match pose_extra {
        PoseExtra::Dragon { .. } => PoseExtra::Dragon {
            anim_time: (frame.age as f32 + partial) * 0.2,
        },
        other => other,
    };

    // A child's limb swing runs three times as fast before the pose reads it
    // (`RendererLivingEntity.doRender`:140-143).
    let mut limb_swing = frame.limb_swing - frame.limb_swing_amount * (1.0 - partial);
    if child {
        limb_swing *= 3.0;
    }

    let pose = Pose {
        limb_swing,
        limb_swing_amount: frame.prev_limb_swing_amount
            + (frame.limb_swing_amount - frame.prev_limb_swing_amount) * partial,
        age: frame.age as f32 + partial,
        head_yaw,
        head_pitch,
        body_yaw,
        sneak: frame.sneaking,
        swing_progress: swing,
        hurt,
        death,
        child,
        extra: pose_extra,
    };

    Some(EntityDraw {
        model,
        position,
        body_yaw,
        head_yaw,
        head_pitch,
        pose,
        texture,
        light: frame.brightness,
        hurt,
        death,
        health: frame.health,
        // The frame carries the composed text the session resolved; a frame without a name
        // leaves the field empty and the pass writes nothing.
        nametag: frame.nametag.clone().map(|text| NametagDraw { text }),
        extra: draw_extra,
    })
}

/// The draw terms one mob frame's kind and metadata name: the model, the sheet, the
/// renderer's extras and the pose's own, plus whether the frame is a child.
///
/// The mapping is each renderer's own: the zombie's villager flag swaps in the villager
/// zombie's model and sheet (`RenderZombie.getEntityTexture`:66-69 for the sheet,
/// `RenderZombie.func_82427_a`:71-85 for the model), the skeleton draws its thin-limbed
/// model off the skeleton sheet (`RenderSkeleton`), the villager's profession picks the
/// sheet out of the renderer's table with its own default
/// (`RenderVillager.getEntityTexture`:32-54), the witch's nose gate reads the held
/// stack (`RenderWitch`, not carried by the frames; [`PoseExtra::Witch`]), the giant
/// draws the zombie model and sheet sixfold (`RenderGiantZombie`), the quadrupeds
/// draw their class sheets (`RenderPig`/`RenderCow`/`RenderSheep`/`RenderMooshroom`),
/// and the crawler families theirs (`RenderCreeper` through `RenderEndermite`), the
/// bats', cubes' and eyes layers' state riding the terms' extras.
/// `None` for a kind without a model.
fn mob_draw(
    kind: EntityKind,
    entity_id: i32,
    mob: &MobExtra,
) -> Option<(ModelRef, TextureRef, DrawExtra, PoseExtra, bool)> {
    let terms = match (kind, mob) {
        (EntityKind::Zombie, MobExtra::Zombie { villager: true }) => (
            ModelRef::ZombieVillager,
            TextureRef::Named("entity/zombie/zombie_villager.png"),
            DrawExtra::ZombieVillager,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Zombie, _) => (
            ModelRef::Zombie,
            TextureRef::Named("entity/zombie/zombie.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        // The zombie pigman draws the zombie's own model on its own sheet
        // (`RenderPigZombie.java`:15, `:11`).
        (EntityKind::PigZombie, _) => (
            ModelRef::Zombie,
            TextureRef::Named("entity/zombie_pigman.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Skeleton, _) => (
            ModelRef::Skeleton,
            TextureRef::Named("entity/skeleton/skeleton.png"),
            DrawExtra::None,
            PoseExtra::Skeleton { aimed_bow: false },
            false,
        ),
        (EntityKind::Villager, MobExtra::Villager { profession, child }) => (
            ModelRef::Villager {
                profession: *profession,
                child: *child,
            },
            TextureRef::Named(villager_sheet(*profession)),
            DrawExtra::Villager {
                profession: *profession,
                child: *child,
            },
            PoseExtra::None,
            *child,
        ),
        (EntityKind::Witch, _) => (
            ModelRef::Witch,
            TextureRef::Named("entity/witch.png"),
            DrawExtra::None,
            PoseExtra::Witch {
                holding: false,
                entity_id,
            },
            false,
        ),
        (EntityKind::Giant, _) => (
            ModelRef::Giant,
            TextureRef::Named("entity/zombie/zombie.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::SnowMan, _) => (
            ModelRef::SnowGolem,
            TextureRef::Named("entity/snowman.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::VillagerGolem, _) => (
            ModelRef::IronGolem,
            TextureRef::Named("entity/iron_golem.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Pig, MobExtra::Pig { saddle }) => (
            ModelRef::Pig { saddle: *saddle },
            TextureRef::Named("entity/pig/pig.png"),
            DrawExtra::Pig { saddle: *saddle },
            PoseExtra::None,
            false,
        ),
        (EntityKind::Cow, _) => (
            ModelRef::Cow,
            TextureRef::Named("entity/cow/cow.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Sheep, MobExtra::Sheep { wool, sheared }) => (
            ModelRef::Sheep {
                wool: *wool,
                sheared: *sheared,
            },
            TextureRef::Named("entity/sheep/sheep.png"),
            DrawExtra::Sheep {
                wool: *wool,
                sheared: *sheared,
            },
            PoseExtra::None,
            false,
        ),
        (EntityKind::MushroomCow, _) => (
            ModelRef::Mooshroom,
            TextureRef::Named("entity/cow/mooshroom.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Creeper, _) => (
            ModelRef::Creeper,
            TextureRef::Named("entity/creeper/creeper.png"),
            DrawExtra::Creeper,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Spider, _) => (
            ModelRef::Spider,
            TextureRef::Named("entity/spider/spider.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::CaveSpider, _) => (
            ModelRef::CaveSpider,
            TextureRef::Named("entity/spider/cave_spider.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        // The screaming flag is not read by the frames; the pose pins it off.
        (EntityKind::Enderman, _) => (
            ModelRef::Enderman,
            TextureRef::Named("entity/enderman/enderman.png"),
            DrawExtra::None,
            PoseExtra::Enderman { attacking: false },
            false,
        ),
        // The chick's fold is not read by the frames and the flap is client-side
        // tick state they do not carry; the pose pins the flap at its rest.
        (EntityKind::Chicken, _) => (
            ModelRef::Chicken { child: false },
            TextureRef::Named("entity/chicken.png"),
            DrawExtra::None,
            PoseExtra::Chicken { flap: 0.0 },
            false,
        ),
        // The tentacle aim is client-side tick state the frames do not carry.
        (EntityKind::Squid, _) => (
            ModelRef::Squid,
            TextureRef::Named("entity/squid.png"),
            DrawExtra::None,
            PoseExtra::Squid {
                tentacle_angle: 0.0,
            },
            false,
        ),
        (EntityKind::Slime, MobExtra::Slime { size }) => (
            ModelRef::Slime { size: *size },
            TextureRef::Named("entity/slime/slime.png"),
            // The squash is client-side tick state the frames do not carry; the
            // pair rests, the draw scaling by the size alone.
            DrawExtra::Slime {
                size: *size,
                squish: 0.0,
            },
            PoseExtra::None,
            false,
        ),
        (EntityKind::LavaSlime, MobExtra::Slime { size }) => (
            ModelRef::MagmaCube { size: *size },
            TextureRef::Named("entity/slime/magmacube.png"),
            DrawExtra::None,
            PoseExtra::MagmaCube { squish: 0.0 },
            false,
        ),
        (EntityKind::Bat, MobExtra::Bat { hanging }) => (
            ModelRef::Bat { hanging: *hanging },
            TextureRef::Named("entity/bat.png"),
            DrawExtra::Bat { hanging: *hanging },
            PoseExtra::Bat { hanging: *hanging },
            false,
        ),
        (EntityKind::Silverfish, _) => (
            ModelRef::Silverfish,
            TextureRef::Named("entity/silverfish.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Endermite, _) => (
            ModelRef::EnderMite,
            TextureRef::Named("entity/endermite.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        // The exotic families. The motions the frames do not carry draw pinned at their
        // rest (the ghast's tentacle sway, the blaze's rod spin, the guardian's spike
        // and tail phases, the rabbit's hop, the wolf's health-scaled tail droop reads
        // the health the frames carry at its own watcher).
        (
            EntityKind::EntityHorse,
            MobExtra::Horse {
                variant,
                colour,
                markings,
                tamed: _,
                saddle,
                chested,
                armour,
                adult,
            },
        ) => (
            ModelRef::Horse {
                variant: *variant,
                colour: *colour,
                markings: *markings,
                saddle: *saddle,
                armour: *armour,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::horse_sheet(
                *variant, *colour,
            )),
            DrawExtra::Horse {
                markings: *markings,
                armour: *armour,
            },
            PoseExtra::Horse {
                saddle: *saddle,
                chested: *chested,
                adult: *adult,
                variant: *variant,
            },
            !*adult,
        ),
        (
            EntityKind::Wolf,
            MobExtra::Wolf {
                tamed,
                collar,
                angry,
                sitting,
                health,
            },
        ) => (
            ModelRef::Wolf {
                tamed: *tamed,
                collar: *collar,
                angry: *angry,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::wolf_sheet(
                *tamed, *angry,
            )),
            DrawExtra::Wolf {
                tamed: *tamed,
                collar: *collar,
            },
            PoseExtra::Wolf {
                tamed: *tamed,
                angry: *angry,
                sitting: *sitting,
                health: *health,
            },
            false,
        ),
        (
            EntityKind::Ozelot,
            MobExtra::Ocelot {
                variant,
                tamed,
                sitting,
            },
        ) => (
            ModelRef::Ocelot {
                variant: *variant,
                child: false,
                tamed: *tamed,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::ocelot_sheet(*variant)),
            DrawExtra::None,
            PoseExtra::Ocelot { sitting: *sitting },
            false,
        ),
        (EntityKind::Rabbit, MobExtra::Rabbit { variant, child }) => (
            ModelRef::Rabbit {
                variant: *variant,
                child: *child,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::rabbit_sheet(*variant)),
            DrawExtra::None,
            PoseExtra::Rabbit { hop: 0.0 },
            *child,
        ),
        (EntityKind::Ghast, MobExtra::Ghast { shooting }) => (
            ModelRef::Ghast {
                shooting: *shooting,
            },
            TextureRef::Named(if *shooting {
                "entity/ghast/ghast_shooting.png"
            } else {
                "entity/ghast/ghast.png"
            }),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Blaze, _) => (
            ModelRef::Blaze,
            TextureRef::Named("entity/blaze.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Guardian, MobExtra::Guardian { elder }) => (
            ModelRef::Guardian { elder: *elder },
            TextureRef::Named(if *elder {
                "entity/guardian_elder.png"
            } else {
                "entity/guardian.png"
            }),
            DrawExtra::None,
            PoseExtra::Guardian {
                spikes: 1.0,
                tail_phase: 0.0,
            },
            false,
        ),
        (EntityKind::EnderDragon, _) => (
            ModelRef::EnderDragon,
            TextureRef::Named("entity/enderdragon/dragon.png"),
            DrawExtra::None,
            PoseExtra::Dragon { anim_time: 0.0 },
            false,
        ),
        (EntityKind::WitherBoss, MobExtra::Wither { invul_time }) => (
            ModelRef::Wither {
                invul_time: *invul_time,
            },
            // The spawn shield's flicker (`RenderWither.getEntityTexture`:33-37): while
            // the timer runs, the invulnerable sheet draws except a beat every fifth
            // tick in the first eighty.
            TextureRef::Named(
                if *invul_time > 0 && (*invul_time > 80 || (*invul_time / 5) % 2 != 1) {
                    "entity/wither/wither_invulnerable.png"
                } else {
                    "entity/wither/wither.png"
                },
            ),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        _ => return None,
    };
    Some(terms)
}

/// The villager's sheet for a profession: the renderer's table indexed by the profession
/// with its own fallback (`RenderVillager.getEntityTexture`:32-54: farmer, librarian,
/// priest, smith, butcher; anything else the plain villager sheet).
fn villager_sheet(profession: u8) -> &'static str {
    match profession {
        0 => "entity/villager/farmer.png",
        1 => "entity/villager/librarian.png",
        2 => "entity/villager/priest.png",
        3 => "entity/villager/smith.png",
        4 => "entity/villager/butcher.png",
        _ => "entity/villager/villager.png",
    }
}

/// The position between the pair: the current one outright when the step is a teleport
/// (past [`SNAP_BLOCKS`]), else the slide at the fraction.
fn interpolate_position(prev: [f64; 3], cur: [f64; 3], partial: f32) -> [f64; 3] {
    let step = [cur[0] - prev[0], cur[1] - prev[1], cur[2] - prev[2]];
    if step[0] * step[0] + step[1] * step[1] + step[2] * step[2] > SNAP_BLOCKS * SNAP_BLOCKS {
        return cur;
    }
    let fraction = f64::from(partial);
    [
        prev[0] + step[0] * fraction,
        prev[1] + step[1] * fraction,
        prev[2] + step[2] * fraction,
    ]
}

/// The source's rotation lerp: the previous angle plus the wrapped difference times the
/// fraction (`interpolateRotation`, `RendererLivingEntity.java:65`).
fn interpolate_rotation(prev: f32, cur: f32, partial: f32) -> f32 {
    let mut difference = cur - prev;
    while difference >= 180.0 {
        difference -= 360.0;
    }
    while difference < -180.0 {
        difference += 360.0;
    }
    prev + difference * partial
}

/// The sixteen legacy colour codes' characters, the palette's index order
/// (`ChatComponentStyle.getFormattedText`:87-99 over `EnumChatFormatting`'s table).
const PALETTE: &[u8; 16] = b"0123456789abcdef";

/// The chat opacity the assembly reads — `GameSettings.chatOpacity`'s default
/// (`GameSettings.java`:85, `1.0F`). A settings store that owns the option is a later
/// milestone's, so the default stands.
const CHAT_OPACITY: f32 = 1.0;

/// The newest line's bar bottom, 28 pixels above the screen's bottom edge: the
/// `(2, 20)` translate (`GuiNewChat.java`:49-51) under the `height - 48` one
/// (`GuiIngame.java`:343).
const CHAT_BASE: f32 = 28.0;

/// The bar's x, the source's translate origin (`GuiNewChat.java`:49-51).
const CHAT_X: f32 = 2.0;

/// One line's pitch in pixels (`GuiNewChat.java`:348-351).
const LINE_PITCH: f32 = 9.0;

/// The bar's height: the pitch's own nine (`GuiNewChat.java`:81-82 draws `j2 - 9` to
/// `j2`).
const BAR_HEIGHT: f32 = 9.0;

/// The bar's width: the wrap budget plus four (`GuiNewChat.java`:82,
/// `getChatWidth`:343-346).
const BAR_WIDTH: f32 = CHAT_WIDTH as f32 + 4.0;

/// The record line's hold in ticks: `recordPlayingUpFor = 60`
/// (`GuiIngame.java`:1118-1122), decremented once per tick (`:1070-1073`).
const SYSTEM_HOLD: u64 = 60;

/// The chat mirror: the session's chat messages, and the draws a frame shows for them.
///
/// The messages land from [`ClientEvent::Chat`]: a message at position code `2` becomes
/// the record line above the hotbar, and every other position enters the box's log —
/// `NetHandlerPlayClient.java`:849-861 sends `2` to `setRecordPlaying` and the rest to
/// `printChatMessage`. The log ([`ChatLog`]) owns the split, the fade clocks and the
/// scroll state; the mirror adds what only the frame knows: the scaled resolution and
/// the per-frame draw assembly.
///
/// The assembly is the source's chat block (`GuiIngame.java`:339-347) over
/// `GuiNewChat.drawChat`:30-114. The box: the newest line's base sits [`CHAT_BASE`]
/// pixels above the bottom edge with a [`LINE_PITCH`]-pixel pitch, each line a black
/// bar at `alpha / 2`, four pixels wider than the 320-pixel wrap budget (`:82` over
/// `GuiNewChat.getChatWidth`:343-346 and `calculateChatboxWidth`:361-366), its text one
/// pixel below the bar's top at the line's alpha (`GuiNewChat.drawChat`:85). The record
/// line is centred above the hotbar, holds for [`SYSTEM_HOLD`]
/// ticks and fades as the tick it arrived at recedes (`GuiIngame.java`:245-272,
/// `:1118-1122`, `:1166-1169`).
pub struct ChatView {
    /// The split lines, the scroll state and the fade clock.
    log: ChatLog,
    /// The measured font the wrap and the runs' widths use; the client hands it over
    /// when the asset store lands.
    font: Option<Font>,
    /// Messages that arrived before the font, parsed and waiting — the mirror cannot
    /// wrap them yet. Bounded at [`LOG_CAP`], oldest dropped first: what the log itself
    /// would keep.
    pending: Vec<(TextComponent, u64)>,
    /// The system line — the newest position-2 message — and the tick it arrived at.
    system: Option<(String, u64)>,
    /// The tick a frame draws at; the record line's own fade clock.
    tick: u64,
}

impl ChatView {
    /// An empty mirror, closed, at tick zero, with no font.
    pub fn new() -> Self {
        Self {
            log: ChatLog::new(),
            font: None,
            pending: Vec::new(),
            system: None,
            tick: 0,
        }
    }

    /// Sets the measured font and wraps whatever arrived before it — a message can
    /// outrun the asset store by a frame.
    pub fn set_font(&mut self, font: Font) {
        for (component, tick) in self.pending.drain(..) {
            self.log.push(&component, tick, CHAT_WIDTH, &font);
        }
        self.font = Some(font);
    }

    /// Folds one [`ClientEvent::Chat`] message in: position `2` replaces the record
    /// line, everything else enters the log at `tick`
    /// (`NetHandlerPlayClient.java`:849-861).
    pub fn observe(&mut self, text: &str, position: i8, tick: u64) {
        let component = chat::parse_json(text);
        if position == 2 {
            // The record line is the message's unformatted text — every element's own
            // characters, styles cut (`GuiIngame.java`:1166-1169 over
            // `ChatComponentStyle.java`:72-81).
            self.system = Some((plain_text(&component), tick));
            return;
        }
        match &self.font {
            Some(font) => self.log.push(&component, tick, CHAT_WIDTH, font),
            None => {
                self.pending.push((component, tick));
                if self.pending.len() > LOG_CAP {
                    self.pending.remove(0);
                }
            }
        }
    }

    /// Ages the mirror to `tick`: the log's fade reads against it and the record line's
    /// hold counts from its receipt.
    pub fn update(&mut self, tick: u64) {
        self.tick = tick;
        self.log.update(tick);
    }

    /// Sets whether the chat window is open: the drawn line count and the log's
    /// pinning follow it (`GuiNewChat.getChatOpen`:305-308). The chat screen that
    /// opens and scrolls the box is a later milestone's, so nothing calls this yet.
    #[allow(dead_code)]
    pub fn set_open(&mut self, open: bool) {
        self.log.set_open(open);
    }

    /// Scrolls the box by `amount` lines, the source's own clamp
    /// (`GuiNewChat.scroll`:222-237). The chat screen's wheel input is a later
    /// milestone's, so the tests are the only caller for now.
    #[allow(dead_code)]
    pub fn scroll(&mut self, amount: i32) {
        self.log.scroll(amount);
    }

    /// Resets the scroll (`GuiNewChat.resetScroll`:211-215). Called when the chat
    /// screen closes, which a later milestone lands.
    #[allow(dead_code)]
    pub fn reset_scroll(&mut self) {
        self.log.reset_scroll();
    }

    /// The frame's draw list at `resolution`.
    ///
    /// The record line first (`GuiIngame.java`:245-272 draws before the chat block at
    /// `:339-347`), then the box's lines newest first; empty until the font is set —
    /// nothing can be measured before it.
    pub fn draws(&self, resolution: ScaledResolution) -> Vec<HudDraw> {
        let Some(font) = &self.font else {
            return Vec::new();
        };
        let mut draws = Vec::new();
        self.system_draws(font, resolution, &mut draws);
        self.box_draws(resolution, &mut draws);
        draws
    }

    /// The record line: centred above the hotbar, white, unshadowed, while its sixty
    /// ticks have not run out (`GuiIngame.java`:245-272).
    fn system_draws(&self, font: &Font, resolution: ScaledResolution, draws: &mut Vec<HudDraw>) {
        let Some((text, received)) = &self.system else {
            return;
        };
        let remaining = SYSTEM_HOLD.saturating_sub(self.tick.saturating_sub(*received));
        if remaining == 0 {
            return;
        }
        // `l1 = (int)(f2 * 255.0F / 20.0F)` at whole ticks, clamped to 255
        // (`GuiIngame.java`:248-254), drawn while it clears eight (`:256`); the
        // source subtracts the frame's partial tick, which this port leaves to the
        // tick the frame draws at.
        let alpha = (((remaining as f32) * 255.0 / 20.0) as u32).min(255) as u8;
        if alpha <= 8 {
            return;
        }
        // The line is centred: the translate's `width / 2` minus half the text's own
        // width, both integer divisions (`GuiIngame.java`:259, `:269`).
        let half = (string_width(font, text) / 2) as f32;
        draws.push(HudDraw::Text {
            text: text.clone(),
            x: (resolution.width / 2) as f32 - half,
            y: resolution.height as f32 - 72.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, f32::from(alpha) / 255.0],
            shadow: false,
        });
    }

    /// The box's drawn lines, newest first: one bar and one text each
    /// (`GuiNewChat.drawChat`:53-91).
    fn box_draws(&self, resolution: ScaledResolution, draws: &mut Vec<HudDraw>) {
        let height = resolution.height as f32;
        let factor = opacity_factor(CHAT_OPACITY);
        for (index, line) in self.log.drawn().iter().enumerate() {
            // The source's opacity multiply and draw gate (`GuiNewChat.java`:75-78).
            let alpha = (f32::from(line.alpha) * factor) as u8;
            if alpha <= 3 {
                continue;
            }
            // The bar's top: the newest line's base bottom ([`CHAT_BASE`]) minus the
            // pitch and the line's own step up the box (`GuiNewChat.java`:81-82 with
            // `:49-51`).
            let top = height - CHAT_BASE - LINE_PITCH * (index as f32 + 1.0);
            draws.push(HudDraw::Rect {
                x: CHAT_X,
                y: top,
                width: BAR_WIDTH,
                height: BAR_HEIGHT,
                colour: [0.0, 0.0, 0.0, f32::from(alpha / 2) / 255.0],
            });
            draws.push(HudDraw::Text {
                text: run_text(line.runs),
                x: CHAT_X,
                y: top + 1.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, f32::from(alpha) / 255.0],
                shadow: true,
            });
        }
    }
}

/// One component's unformatted text: every element's own characters, `§` codes and
/// styles as sent, depth first — `ChatComponentStyle.getUnformattedText`:72-81, the
/// walk the record line reads.
fn plain_text(component: &TextComponent) -> String {
    let mut text = component.text.clone();
    for child in &component.children {
        text.push_str(&plain_text(child));
    }
    text
}

/// One line's runs as the `§`-coded string the text builder draws: the source's
/// `getFormattedText` shape (`ChatComponentStyle.java`:87-99) with
/// `ChatStyle.getFormattingCode`:306-346 — each run's colour and style codes in the
/// source's order, its characters, then a reset.
fn run_text(runs: &[chat::StyledRun]) -> String {
    let mut text = String::new();
    for run in runs {
        if let Some(index) = run.colour {
            text.push('§');
            text.push(char::from(PALETTE[usize::from(index.min(15))]));
        }
        if run.styles & STYLE_BOLD != 0 {
            text.push_str("§l");
        }
        if run.styles & STYLE_ITALIC != 0 {
            text.push_str("§o");
        }
        if run.styles & STYLE_UNDERLINED != 0 {
            text.push_str("§n");
        }
        if run.styles & STYLE_OBFUSCATED != 0 {
            text.push_str("§k");
        }
        if run.styles & STYLE_STRIKETHROUGH != 0 {
            text.push_str("§m");
        }
        text.push_str(&run.text);
        text.push_str("§r");
    }
    text
}

/// The source's chat-opacity factor (`GuiNewChat.java`:38): `chatOpacity * 0.9 + 0.1`,
/// all f32. At the settings default `1.0` (`GameSettings.java`:85) the sum lands on
/// `1.0` exactly, so the fade byte passes through untouched.
fn opacity_factor(chat_opacity: f32) -> f32 {
    chat_opacity * 0.9 + 0.1
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::time::Duration;

    use oxide_assets::skins::DefaultModel;

    use oxide_game::entity_view::{EntityExtra, EntityFrame, MobExtra};
    use oxide_proto_v47::entity::MetadataItem;
    use oxide_render::entity_models::PoseExtra;
    use oxide_render::entity_models::player::{CapeMotion, cape_rotation};
    use oxide_render::entity_pass::{FrameContent, ModelRef, NametagDraw, TextureRef};
    use oxide_render::hud::scaled_resolution;
    use oxide_world::entity::EntityKind;

    /// The uuid whose default model the rule calls wide (its last bit is zero).
    const UUID_WIDE: &str = "00000000-0000-0000-0000-000000000000";
    /// The uuid whose default model the rule calls slim (its last bit is one).
    const UUID_SLIM: &str = "00000000-0000-0000-0000-000000000001";

    /// The tick's 50 ms step, as the fraction's divisor.
    const TICK: Duration = Duration::from_millis(50);

    /// A player frame whose pairs all move, so one interpolation pins them all: the
    /// position slides 0 -> 2 on x, the pitch 0 -> 20, the head yaw 0 -> 10, the render
    /// (body) yaw 0 -> 90, and the limb pair and swing progress move too.
    fn player_frame(id: i32, uuid: &str) -> EntityFrame {
        EntityFrame {
            id,
            kind: EntityKind::Player,
            uuid: Some(uuid.to_owned()),
            prev: [0.0, 0.0, 0.0],
            pos: [2.0, 0.0, 0.0],
            prev_yaw: 0.0,
            yaw: 0.0,
            prev_pitch: 0.0,
            pitch: 20.0,
            prev_head_yaw: 0.0,
            head_yaw: 10.0,
            render_yaw_offset: 90.0,
            prev_render_yaw_offset: 0.0,
            on_ground: true,
            invisible: false,
            sneaking: false,
            age: 100,
            limb_swing: 1.0,
            limb_swing_amount: 0.5,
            prev_limb_swing_amount: 0.25,
            swing_progress: 0.5,
            prev_swing_progress: 0.25,
            hurt_ticks: 0,
            death_ticks: 0,
            brightness: 0.65,
            health: None,
            nametag: None,
            extra: EntityExtra::Player,
        }
    }

    /// The join event that names the window's own entity.
    fn joined(entity_id: i32) -> ClientEvent {
        ClientEvent::Joined {
            entity_id,
            gamemode: 0,
            dimension: 0,
            difficulty: 1,
            max_players: 20,
            level_type: "default".to_owned(),
        }
    }

    /// The tick feed event carrying the frames.
    fn ticked(entities: Vec<EntityFrame>) -> ClientEvent {
        ClientEvent::EntitiesTick { entities }
    }

    /// The draws at `elapsed` past the arrival.
    fn draws_at(view: &View, arrival: Instant, elapsed: Duration) -> Vec<EntityDraw> {
        view.entity_draws(arrival + elapsed, &BTreeMap::new())
    }

    #[test]
    fn the_draws_interpolate_at_the_arrivals_fraction() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(vec![player_frame(8, UUID_SLIM)], t0);
        let draws = draws_at(&view, t0, TICK / 2);
        assert_eq!(draws.len(), 1);
        let draw = &draws[0];
        // The position slides half way in one half tick's fraction, the rotations lerp and
        // the head's net yaw is the head's turn minus the body's.
        assert_eq!(draw.position, [1.0, 0.0, 0.0]);
        assert!((draw.body_yaw - 45.0).abs() < 1.0e-3);
        assert!((draw.head_yaw + 40.0).abs() < 1.0e-3);
        assert!((draw.head_pitch - 10.0).abs() < 1.0e-3);
        // The pose's own terms: `limbSwing - limbSwingAmount * (1 - partial)`, the eased
        // limb amount, the age, and the swing grid's step.
        assert!((draw.pose.limb_swing - 0.75).abs() < 1.0e-4);
        assert!((draw.pose.limb_swing_amount - 0.375).abs() < 1.0e-4);
        assert!((draw.pose.age - 100.5).abs() < 1.0e-4);
        assert!((draw.pose.swing_progress - 0.375).abs() < 1.0e-4);
        // Nothing hurts or dies, the light is the frame's brightness, and no update means
        // the uuid's default model — this uuid's bit is one, so slim.
        assert_eq!(draw.hurt, 0.0);
        assert_eq!(draw.death, 0.0);
        assert_eq!(draw.light, 0.65);
        assert_eq!(
            draw.model,
            ModelRef::Player {
                slim: true,
                parts: 0x7F
            }
        );
        assert_eq!(
            draw.texture,
            TextureRef::Skin {
                uuid: UUID_SLIM.to_owned(),
                slim: true
            }
        );
    }

    #[test]
    fn a_frames_nametag_maps_onto_the_draw_and_an_absent_one_stays_none() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut named = player_frame(8, UUID_SLIM);
        named.nametag = Some(Arc::from("Notch"));
        view.observe(vec![named], t0);
        assert_eq!(
            draws_at(&view, t0, TICK / 2)[0].nametag,
            Some(NametagDraw {
                text: Arc::from("Notch")
            })
        );
        // A frame without a name leaves the draw's field empty.
        let plain = player_frame(8, UUID_SLIM);
        view.observe(vec![plain], t0);
        assert_eq!(draws_at(&view, t0, TICK / 2)[0].nametag, None);
    }

    #[test]
    fn the_jump_over_four_blocks_snaps() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut far = player_frame(8, UUID_WIDE);
        far.pos = [5.0, 0.0, 0.0];
        view.observe(vec![far], t0);
        // A five-block step is a teleport: the draw is the current position, not a blur
        // between the two.
        assert_eq!(draws_at(&view, t0, TICK / 2)[0].position, [5.0, 0.0, 0.0]);
        // Three blocks interpolate.
        let mut near = player_frame(8, UUID_WIDE);
        near.pos = [3.0, 0.0, 0.0];
        view.observe(vec![near], t0);
        assert_eq!(draws_at(&view, t0, TICK / 2)[0].position, [1.5, 0.0, 0.0]);
    }

    #[test]
    fn the_step_at_exactly_four_blocks_still_slides() {
        // The snap starts only past the boundary (spec P5, `docs/specs/oxidecraft-v1-design.md:92`,
        // restated in section 9 at `:315`: "snapping when a teleport exceeds 4 blocks").
        let mut view = View::new();
        let t0 = Instant::now();
        let mut edge = player_frame(8, UUID_WIDE);
        edge.pos = [4.0, 0.0, 0.0];
        view.observe(vec![edge], t0);
        assert_eq!(
            draws_at(&view, t0, TICK / 2)[0].position,
            [2.0, 0.0, 0.0],
            "a step of exactly four blocks interpolates"
        );
        let mut over = player_frame(8, UUID_WIDE);
        over.pos = [4.1, 0.0, 0.0];
        view.observe(vec![over], t0);
        assert_eq!(
            draws_at(&view, t0, TICK / 2)[0].position,
            [4.1, 0.0, 0.0],
            "a step past four blocks snaps"
        );
    }

    #[test]
    fn the_cape_motion_reads_the_frames_displacement() {
        let mut view = View::new();
        let t0 = Instant::now();
        // The pair moves two blocks on x: the draw's cape motion term is the pair's own
        // displacement, and at pose level the wave turns the box off its rest [6, 180, 0].
        view.observe(vec![player_frame(8, UUID_WIDE)], t0);
        let draw = &draws_at(&view, t0, Duration::ZERO)[0];
        assert_eq!(
            draw.pose.extra,
            PoseExtra::Player(CapeMotion {
                motion: [2.0, 0.0, 0.0]
            }),
            "the cape's motion term is the frame pair's displacement"
        );
        let PoseExtra::Player(cape) = draw.pose.extra else {
            panic!("the player's draw carries the cape motion");
        };
        let moved = cape_rotation(&draw.pose, cape.motion);
        assert_eq!(moved, [6.0, 280.0, -100.0]);
        assert_ne!(moved, cape_rotation(&draw.pose, [0.0; 3]));
        // A still pair carries no motion at all.
        let mut still = player_frame(8, UUID_WIDE);
        still.prev = still.pos;
        view.observe(vec![still], t0);
        assert_eq!(
            draws_at(&view, t0, Duration::ZERO)[0].pose.extra,
            PoseExtra::Player(CapeMotion {
                motion: [0.0, 0.0, 0.0]
            })
        );
    }

    #[test]
    fn the_own_entity_is_skipped() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.apply(&joined(7));
        view.apply(&ticked(vec![
            player_frame(7, UUID_WIDE),
            player_frame(8, UUID_SLIM),
        ]));
        let draws = view.entity_draws(t0, &BTreeMap::new());
        assert_eq!(draws.len(), 1, "the window's own entity draws nothing");
        assert_eq!(
            draws[0].texture,
            TextureRef::Skin {
                uuid: UUID_SLIM.to_owned(),
                slim: true
            }
        );
    }

    #[test]
    fn an_invisible_frame_draws_nothing() {
        let mut view = View::new();
        let t0 = Instant::now();
        // The flag alone: the model and the shadow both hang off the draw, and the
        // source skips both for an invisible entity (`RendererLivingEntity.java:248-249`,
        // `Render.java:303`).
        let mut unseen = player_frame(8, UUID_WIDE);
        unseen.invisible = true;
        view.observe(vec![unseen], t0);
        assert_eq!(
            draws_at(&view, t0, Duration::ZERO).len(),
            0,
            "an invisible entity draws nothing"
        );
        view.observe(vec![player_frame(8, UUID_WIDE)], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO).len(), 1);
    }

    #[test]
    fn the_texture_falls_back_by_uuid() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![player_frame(8, UUID_WIDE), player_frame(9, UUID_SLIM)],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(
            draws[0].texture,
            TextureRef::Skin {
                uuid: UUID_WIDE.to_owned(),
                slim: false
            }
        );
        assert_eq!(
            draws[1].texture,
            TextureRef::Skin {
                uuid: UUID_SLIM.to_owned(),
                slim: true
            }
        );
        assert_eq!(
            draws[0].model,
            ModelRef::Player {
                slim: false,
                parts: 0x7F
            }
        );
        // A stored update overrides the rule as it stands, as-is.
        let mut skins = BTreeMap::new();
        skins.insert(
            UUID_WIDE.to_owned(),
            SkinUpdate {
                uuid: UUID_WIDE.to_owned(),
                texture: None,
                cape: None,
                model: DefaultModel::Slim,
            },
        );
        let draw = &view.entity_draws(t0, &skins)[0];
        assert_eq!(
            draw.model,
            ModelRef::Player {
                slim: true,
                parts: 0x7F
            }
        );
        assert_eq!(
            draw.texture,
            TextureRef::Skin {
                uuid: UUID_WIDE.to_owned(),
                slim: true
            }
        );
    }

    #[test]
    fn the_light_reads_the_frames_brightness() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut frame = player_frame(8, UUID_WIDE);
        frame.brightness = 0.2;
        view.observe(vec![frame], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO)[0].light, 0.2);
        let mut frame = player_frame(8, UUID_WIDE);
        frame.brightness = 1.0;
        view.observe(vec![frame], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO)[0].light, 1.0);
    }

    #[test]
    fn the_death_ramp_pins_three_points() {
        let mut view = View::new();
        let t0 = Instant::now();
        // One death tick, half a tick in: sqrt(((1 + 0.5 - 1) / 20) * 1.6) = 0.2.
        let mut frame = player_frame(8, UUID_WIDE);
        frame.death_ticks = 1;
        view.observe(vec![frame], t0);
        let draw = &draws_at(&view, t0, TICK / 2)[0];
        assert!((draw.death - 0.2).abs() < 1.0e-4, "got {}", draw.death);
        // The death's own gate: the damage overlay opens with it, as the source's flag
        // does (`hurtTime > 0 || deathTime > 0`).
        assert_eq!(draw.hurt, 1.0);
        // Thirteen ticks, on the tick: sqrt((12 / 20) * 1.6) = 0.9797959.
        let mut frame = player_frame(8, UUID_WIDE);
        frame.death_ticks = 13;
        view.observe(vec![frame], t0);
        let draw = &draws_at(&view, t0, Duration::ZERO)[0];
        assert!(
            (draw.death - 0.979_795_9).abs() < 1.0e-4,
            "got {}",
            draw.death
        );
        // Well past the ramp's end the fraction clamps at one.
        let mut frame = player_frame(8, UUID_WIDE);
        frame.death_ticks = 40;
        view.observe(vec![frame], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO)[0].death, 1.0);
    }

    /// A mob frame: the player template's pairs with the kind's own channel.
    fn mob_frame(id: i32, kind: EntityKind, extra: EntityExtra) -> EntityFrame {
        let mut frame = player_frame(id, UUID_WIDE);
        frame.uuid = None;
        frame.kind = kind;
        frame.extra = extra;
        frame
    }

    #[test]
    fn the_crawler_families_map_to_their_models_sheets_and_extras() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(1, EntityKind::Creeper, EntityExtra::Mob(MobExtra::Creeper)),
                mob_frame(2, EntityKind::Spider, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(3, EntityKind::CaveSpider, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    4,
                    EntityKind::Enderman,
                    EntityExtra::Mob(MobExtra::Enderman),
                ),
                mob_frame(5, EntityKind::Chicken, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(6, EntityKind::Squid, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    7,
                    EntityKind::Slime,
                    EntityExtra::Mob(MobExtra::Slime { size: 3 }),
                ),
                mob_frame(
                    8,
                    EntityKind::LavaSlime,
                    EntityExtra::Mob(MobExtra::Slime { size: 2 }),
                ),
                mob_frame(
                    9,
                    EntityKind::Bat,
                    EntityExtra::Mob(MobExtra::Bat { hanging: true }),
                ),
                mob_frame(
                    10,
                    EntityKind::Silverfish,
                    EntityExtra::Mob(MobExtra::Other),
                ),
                mob_frame(11, EntityKind::Endermite, EntityExtra::Mob(MobExtra::Other)),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 11, "every mapped mob draws");
        let expected = [
            (ModelRef::Creeper, "entity/creeper/creeper.png"),
            (ModelRef::Spider, "entity/spider/spider.png"),
            (ModelRef::CaveSpider, "entity/spider/cave_spider.png"),
            (ModelRef::Enderman, "entity/enderman/enderman.png"),
            (ModelRef::Chicken { child: false }, "entity/chicken.png"),
            (ModelRef::Squid, "entity/squid.png"),
            (ModelRef::Slime { size: 3 }, "entity/slime/slime.png"),
            (
                ModelRef::MagmaCube { size: 2 },
                "entity/slime/magmacube.png",
            ),
            (ModelRef::Bat { hanging: true }, "entity/bat.png"),
            (ModelRef::Silverfish, "entity/silverfish.png"),
            (ModelRef::EnderMite, "entity/endermite.png"),
        ];
        for (draw, (model, sheet)) in draws.iter().zip(expected) {
            assert_eq!(draw.model, model);
            assert_eq!(draw.texture, TextureRef::Named(sheet));
        }
        // The pinned extras ride the draws: the creeper's marker, the bat's hang on
        // both terms, the cubes' sizes with their squash pairs at rest, and the
        // client-side-only states held off.
        assert_eq!(draws[0].extra, DrawExtra::Creeper);
        assert_eq!(
            draws[3].pose.extra,
            PoseExtra::Enderman { attacking: false }
        );
        assert_eq!(draws[4].pose.extra, PoseExtra::Chicken { flap: 0.0 });
        assert_eq!(
            draws[5].pose.extra,
            PoseExtra::Squid {
                tentacle_angle: 0.0
            }
        );
        assert_eq!(
            draws[6].extra,
            DrawExtra::Slime {
                size: 3,
                squish: 0.0
            }
        );
        assert_eq!(draws[7].pose.extra, PoseExtra::MagmaCube { squish: 0.0 });
        assert_eq!(draws[8].extra, DrawExtra::Bat { hanging: true });
        assert_eq!(draws[8].pose.extra, PoseExtra::Bat { hanging: true });
    }

    #[test]
    fn every_new_kind_maps_to_its_model_sheet_and_extras() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(
                    1,
                    EntityKind::Zombie,
                    EntityExtra::Mob(MobExtra::Zombie { villager: false }),
                ),
                mob_frame(
                    2,
                    EntityKind::Zombie,
                    EntityExtra::Mob(MobExtra::Zombie { villager: true }),
                ),
                mob_frame(3, EntityKind::Skeleton, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    4,
                    EntityKind::Villager,
                    EntityExtra::Mob(MobExtra::Villager {
                        profession: 4,
                        child: false,
                    }),
                ),
                mob_frame(5, EntityKind::Witch, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(6, EntityKind::Giant, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(7, EntityKind::SnowMan, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    8,
                    EntityKind::VillagerGolem,
                    EntityExtra::Mob(MobExtra::Other),
                ),
                mob_frame(
                    9,
                    EntityKind::Pig,
                    EntityExtra::Mob(MobExtra::Pig { saddle: true }),
                ),
                mob_frame(10, EntityKind::Cow, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    11,
                    EntityKind::Sheep,
                    EntityExtra::Mob(MobExtra::Sheep {
                        wool: 9,
                        sheared: false,
                    }),
                ),
                mob_frame(
                    12,
                    EntityKind::MushroomCow,
                    EntityExtra::Mob(MobExtra::Other),
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 12, "every mapped mob draws");
        let expected = [
            (ModelRef::Zombie, "entity/zombie/zombie.png"),
            (
                ModelRef::ZombieVillager,
                "entity/zombie/zombie_villager.png",
            ),
            (ModelRef::Skeleton, "entity/skeleton/skeleton.png"),
            (
                ModelRef::Villager {
                    profession: 4,
                    child: false,
                },
                "entity/villager/butcher.png",
            ),
            (ModelRef::Witch, "entity/witch.png"),
            (ModelRef::Giant, "entity/zombie/zombie.png"),
            (ModelRef::SnowGolem, "entity/snowman.png"),
            (ModelRef::IronGolem, "entity/iron_golem.png"),
            (ModelRef::Pig { saddle: true }, "entity/pig/pig.png"),
            (ModelRef::Cow, "entity/cow/cow.png"),
            (
                ModelRef::Sheep {
                    wool: 9,
                    sheared: false,
                },
                "entity/sheep/sheep.png",
            ),
            (ModelRef::Mooshroom, "entity/cow/mooshroom.png"),
        ];
        for (draw, (model, sheet)) in draws.iter().zip(expected) {
            assert_eq!(draw.model, model);
            assert_eq!(draw.texture, TextureRef::Named(sheet));
        }
        // The extras: the zombie villager's flag, the villager's profession pair, the
        // pig's saddle and the sheep's wool; the pose carries the skeleton's aim state
        // and the witch's hold gate seeded by the entity's own id
        // (`ModelWitch.setRotationAngles`:51-60).
        assert_eq!(draws[1].extra, DrawExtra::ZombieVillager);
        assert_eq!(
            draws[3].extra,
            DrawExtra::Villager {
                profession: 4,
                child: false
            }
        );
        assert_eq!(draws[8].extra, DrawExtra::Pig { saddle: true });
        assert_eq!(
            draws[10].extra,
            DrawExtra::Sheep {
                wool: 9,
                sheared: false
            }
        );
        assert_eq!(
            draws[2].pose.extra,
            PoseExtra::Skeleton { aimed_bow: false }
        );
        assert_eq!(
            draws[4].pose.extra,
            PoseExtra::Witch {
                holding: false,
                entity_id: 5
            }
        );
        // Every draw names a zone the window and the pass share; the non-villager mobs
        // carry no child term.
        assert!(draws.iter().all(|draw| !draw.pose.child));
    }

    /// The exotic families' sheets and extras, per their renderers: the horse's colour,
    /// type and marking/armour tables with its saddle and chest terms
    /// (`RenderHorse.getEntityTexture`:51-78), the wolf's tamed/angry sheet pair with the
    /// collar byte (`RenderWolf.getEntityTexture`:46-49), the ocelot's and the rabbit's
    /// variant tables (`RenderOcelot.getEntityTexture`:23-40,
    /// `RenderRabbit.getEntityTexture`:27-62), the ghast's shooting sheet
    /// (`RenderGhast.getEntityTexture`:21-24), the guardian's elder sheet
    /// (`RenderGuardian.getEntityTexture`:177-180), the dragon, and the wither's
    /// spawn-shield flicker (`RenderWither.getEntityTexture`:33-37).
    #[test]
    fn the_exotic_families_map_to_their_models_sheets_and_extras() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(
                    1,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 1,
                        markings: 2,
                        tamed: true,
                        saddle: true,
                        adult: true,
                        chested: true,
                        armour: 3,
                    }),
                ),
                mob_frame(
                    2,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    3,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 4,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    4,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 1,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    5,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 2,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    6,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 3,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    7,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 4,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    8,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 3,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: false,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    9,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: true,
                        collar: 14,
                        angry: false,
                        sitting: false,
                        health: 20.0,
                    }),
                ),
                mob_frame(
                    10,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: false,
                        collar: 14,
                        angry: true,
                        sitting: false,
                        health: 20.0,
                    }),
                ),
                mob_frame(
                    11,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: false,
                        collar: 14,
                        angry: false,
                        sitting: false,
                        health: 20.0,
                    }),
                ),
                mob_frame(
                    12,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: true,
                        collar: 3,
                        angry: false,
                        sitting: true,
                        health: 8.0,
                    }),
                ),
                mob_frame(
                    13,
                    EntityKind::Ozelot,
                    EntityExtra::Mob(MobExtra::Ocelot {
                        variant: 0,
                        tamed: false,
                        sitting: false,
                    }),
                ),
                mob_frame(
                    14,
                    EntityKind::Ozelot,
                    EntityExtra::Mob(MobExtra::Ocelot {
                        variant: 2,
                        tamed: true,
                        sitting: false,
                    }),
                ),
                mob_frame(
                    15,
                    EntityKind::Ozelot,
                    EntityExtra::Mob(MobExtra::Ocelot {
                        variant: 3,
                        tamed: true,
                        sitting: true,
                    }),
                ),
                mob_frame(
                    16,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 0,
                        child: false,
                    }),
                ),
                mob_frame(
                    17,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 3,
                        child: false,
                    }),
                ),
                mob_frame(
                    18,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 99,
                        child: false,
                    }),
                ),
                mob_frame(
                    19,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 1,
                        child: true,
                    }),
                ),
                mob_frame(
                    20,
                    EntityKind::Ghast,
                    EntityExtra::Mob(MobExtra::Ghast { shooting: false }),
                ),
                mob_frame(
                    21,
                    EntityKind::Ghast,
                    EntityExtra::Mob(MobExtra::Ghast { shooting: true }),
                ),
                mob_frame(22, EntityKind::Blaze, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    23,
                    EntityKind::Guardian,
                    EntityExtra::Mob(MobExtra::Guardian { elder: false }),
                ),
                mob_frame(
                    24,
                    EntityKind::Guardian,
                    EntityExtra::Mob(MobExtra::Guardian { elder: true }),
                ),
                mob_frame(
                    25,
                    EntityKind::EnderDragon,
                    EntityExtra::Mob(MobExtra::Other),
                ),
                mob_frame(
                    26,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither { invul_time: 0 }),
                ),
                mob_frame(
                    27,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither { invul_time: 100 }),
                ),
                mob_frame(
                    28,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither { invul_time: 5 }),
                ),
                mob_frame(
                    29,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither { invul_time: 10 }),
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 29, "every exotic draws");
        let expected = [
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 1,
                    markings: 2,
                    saddle: true,
                    armour: 3,
                },
                "entity/horse/horse_creamy.png",
            ),
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_white.png",
            ),
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 4,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_black.png",
            ),
            (
                ModelRef::Horse {
                    variant: 1,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/donkey.png",
            ),
            (
                ModelRef::Horse {
                    variant: 2,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/mule.png",
            ),
            (
                ModelRef::Horse {
                    variant: 3,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_zombie.png",
            ),
            (
                ModelRef::Horse {
                    variant: 4,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_skeleton.png",
            ),
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 3,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_brown.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: true,
                    collar: 14,
                    angry: false,
                },
                "entity/wolf/wolf_tame.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: false,
                    collar: 14,
                    angry: true,
                },
                "entity/wolf/wolf_angry.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: false,
                    collar: 14,
                    angry: false,
                },
                "entity/wolf/wolf.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: true,
                    collar: 3,
                    angry: false,
                },
                "entity/wolf/wolf_tame.png",
            ),
            (
                ModelRef::Ocelot {
                    variant: 0,
                    child: false,
                    tamed: false,
                },
                "entity/cat/ocelot.png",
            ),
            (
                ModelRef::Ocelot {
                    variant: 2,
                    child: false,
                    tamed: true,
                },
                "entity/cat/red.png",
            ),
            (
                ModelRef::Ocelot {
                    variant: 3,
                    child: false,
                    tamed: true,
                },
                "entity/cat/siamese.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 0,
                    child: false,
                },
                "entity/rabbit/brown.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 3,
                    child: false,
                },
                "entity/rabbit/white_splotched.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 99,
                    child: false,
                },
                "entity/rabbit/caerbannog.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 1,
                    child: true,
                },
                "entity/rabbit/white.png",
            ),
            (
                ModelRef::Ghast { shooting: false },
                "entity/ghast/ghast.png",
            ),
            (
                ModelRef::Ghast { shooting: true },
                "entity/ghast/ghast_shooting.png",
            ),
            (ModelRef::Blaze, "entity/blaze.png"),
            (ModelRef::Guardian { elder: false }, "entity/guardian.png"),
            (
                ModelRef::Guardian { elder: true },
                "entity/guardian_elder.png",
            ),
            (ModelRef::EnderDragon, "entity/enderdragon/dragon.png"),
            (
                ModelRef::Wither { invul_time: 0 },
                "entity/wither/wither.png",
            ),
            (
                ModelRef::Wither { invul_time: 100 },
                "entity/wither/wither_invulnerable.png",
            ),
            (
                ModelRef::Wither { invul_time: 5 },
                "entity/wither/wither.png",
            ),
            (
                ModelRef::Wither { invul_time: 10 },
                "entity/wither/wither_invulnerable.png",
            ),
        ];
        for (draw, (model, sheet)) in draws.iter().zip(expected) {
            assert_eq!(draw.model, model);
            assert_eq!(draw.texture, TextureRef::Named(sheet));
        }
        // The extras: the horse's marking and armour terms, the wolf's collar byte, and
        // the two pose terms the arms pin at rest (the ghast's sway, the blaze's spin,
        // the guardian's spikes, the dragon's flight clock, the rabbit's hop).
        assert_eq!(
            draws[0].extra,
            DrawExtra::Horse {
                markings: 2,
                armour: 3
            }
        );
        assert_eq!(
            draws[0].pose.extra,
            PoseExtra::Horse {
                saddle: true,
                chested: true,
                adult: true,
                variant: 0
            }
        );
        assert_eq!(
            draws[1].extra,
            DrawExtra::Horse {
                markings: 0,
                armour: 0
            }
        );
        assert!(draws[7].pose.child, "the horse's growing age folds it");
        assert_eq!(
            draws[8].extra,
            DrawExtra::Wolf {
                tamed: true,
                collar: 14
            }
        );
        assert_eq!(
            draws[8].pose.extra,
            PoseExtra::Wolf {
                tamed: true,
                angry: false,
                sitting: false,
                health: 20.0
            }
        );
        assert_eq!(
            draws[9].pose.extra,
            PoseExtra::Wolf {
                tamed: false,
                angry: true,
                sitting: false,
                health: 20.0
            }
        );
        assert_eq!(
            draws[11].pose.extra,
            PoseExtra::Wolf {
                tamed: true,
                angry: false,
                sitting: true,
                health: 8.0
            }
        );
        assert_eq!(draws[12].pose.extra, PoseExtra::Ocelot { sitting: false });
        assert_eq!(draws[14].pose.extra, PoseExtra::Ocelot { sitting: true });
        assert_eq!(draws[15].pose.extra, PoseExtra::Rabbit { hop: 0.0 });
        assert!(draws[18].pose.child, "the rabbit's growing age folds it");
        assert_eq!(
            draws[22].pose.extra,
            PoseExtra::Guardian {
                spikes: 1.0,
                tail_phase: 0.0
            }
        );
        assert_eq!(draws[24].pose.extra, PoseExtra::Dragon { anim_time: 20.0 });
    }

    /// The dragon's wing clock (`PoseExtra::Dragon`): the frames carry the age, and the
    /// view advances the clock at the source's at-rest rate, `0.2` a tick
    /// (`EntityDragon.onLivingUpdate`:158-167).
    #[test]
    fn the_dragon_wing_clock_advances_with_the_frames_age() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut frame = mob_frame(
            1,
            EntityKind::EnderDragon,
            EntityExtra::Mob(MobExtra::Other),
        );
        frame.age = 40;
        view.observe(vec![frame], t0);
        let anim_time = |draws: Vec<EntityDraw>| match draws[0].pose.extra {
            PoseExtra::Dragon { anim_time } => anim_time,
            _ => panic!("the dragon's draw carries its flight clock"),
        };
        // Forty ticks at the at-rest rate: eight waves in.
        let anim = anim_time(draws_at(&view, t0, Duration::ZERO));
        assert!(
            (anim - 8.0).abs() < 1.0e-4,
            "the clock at {anim} against 8.0"
        );
        // The partial tick walks the clock on with the frame's fraction.
        let anim = anim_time(draws_at(&view, t0, TICK / 4));
        assert!(
            (anim - 8.05).abs() < 1.0e-4,
            "the clock at {anim} against 8.05"
        );
    }

    /// The roster's own gate: every mob §6.3's spawn-mob table names has a draw — no
    /// kind falls through to the debug-log arm.
    ///
    /// The list is the table's own roster transcribed — creeper `50` through guardian
    /// `68`, pig `90` through rabbit `101`, villager `120` — each member paired with
    /// the extras its metadata extracts to (the session's own per-kind results). A
    /// member the wire can spawn but the mapping cannot draw fails here.
    #[test]
    fn every_roster_mob_maps_to_a_draw() {
        let cases: &[(EntityKind, MobExtra)] = &[
            // 50..=68.
            (EntityKind::Creeper, MobExtra::Creeper),
            (EntityKind::Skeleton, MobExtra::Other),
            (EntityKind::Spider, MobExtra::Other),
            (EntityKind::Giant, MobExtra::Other),
            (EntityKind::Zombie, MobExtra::Zombie { villager: false }),
            (EntityKind::Slime, MobExtra::Slime { size: 1 }),
            (EntityKind::Ghast, MobExtra::Ghast { shooting: false }),
            (EntityKind::PigZombie, MobExtra::Other),
            (EntityKind::Enderman, MobExtra::Enderman),
            (EntityKind::CaveSpider, MobExtra::Other),
            (EntityKind::Silverfish, MobExtra::Other),
            (EntityKind::Blaze, MobExtra::Other),
            (EntityKind::LavaSlime, MobExtra::Slime { size: 1 }),
            (EntityKind::EnderDragon, MobExtra::Other),
            (EntityKind::WitherBoss, MobExtra::Wither { invul_time: 0 }),
            (EntityKind::Bat, MobExtra::Bat { hanging: false }),
            (EntityKind::Witch, MobExtra::Other),
            (EntityKind::Endermite, MobExtra::Other),
            (EntityKind::Guardian, MobExtra::Guardian { elder: false }),
            // 90..=101.
            (EntityKind::Pig, MobExtra::Pig { saddle: false }),
            (
                EntityKind::Sheep,
                MobExtra::Sheep {
                    wool: 0,
                    sheared: false,
                },
            ),
            (EntityKind::Cow, MobExtra::Other),
            (EntityKind::Chicken, MobExtra::Other),
            (EntityKind::Squid, MobExtra::Other),
            (
                EntityKind::Wolf,
                MobExtra::Wolf {
                    tamed: false,
                    collar: 14,
                    angry: false,
                    sitting: false,
                    health: 20.0,
                },
            ),
            (EntityKind::MushroomCow, MobExtra::Other),
            (EntityKind::SnowMan, MobExtra::Other),
            (
                EntityKind::Ozelot,
                MobExtra::Ocelot {
                    variant: 0,
                    tamed: false,
                    sitting: false,
                },
            ),
            (EntityKind::VillagerGolem, MobExtra::Other),
            (
                EntityKind::EntityHorse,
                MobExtra::Horse {
                    variant: 0,
                    colour: 0,
                    markings: 0,
                    tamed: false,
                    saddle: false,
                    adult: true,
                    chested: false,
                    armour: 0,
                },
            ),
            (
                EntityKind::Rabbit,
                MobExtra::Rabbit {
                    variant: 0,
                    child: false,
                },
            ),
            // 120.
            (
                EntityKind::Villager,
                MobExtra::Villager {
                    profession: 0,
                    child: false,
                },
            ),
        ];
        assert_eq!(cases.len(), 32, "§6.3's spawn-mob roster is 32 ids");
        for (index, (kind, mob)) in cases.iter().enumerate() {
            assert!(
                mob_draw(*kind, index as i32 + 1, mob).is_some(),
                "{kind:?} has no model mapping — it would fall through to the debug-log arm"
            );
        }
    }

    #[test]
    fn a_child_villager_runs_its_limbs_threefold_and_carries_the_child_term() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![mob_frame(
                4,
                EntityKind::Villager,
                EntityExtra::Mob(MobExtra::Villager {
                    profession: 0,
                    child: true,
                }),
            )],
            t0,
        );
        let draw = &draws_at(&view, t0, TICK / 2)[0];
        assert_eq!(
            draw.model,
            ModelRef::Villager {
                profession: 0,
                child: true
            }
        );
        assert_eq!(
            draw.texture,
            TextureRef::Named("entity/villager/farmer.png")
        );
        assert!(draw.pose.child);
        // `RendererLivingEntity.doRender`:140-143: a child's limb swing runs threefold
        // before the pose reads it — (1 - 0.5 * (1 - 0.5)) * 3 = 2.25.
        assert!(
            (draw.pose.limb_swing - 2.25).abs() < 1.0e-4,
            "the child's limb swing: {}",
            draw.pose.limb_swing
        );
    }

    #[test]
    fn an_unmapped_kind_draws_nothing() {
        let mut view = View::new();
        let t0 = Instant::now();
        // A wire kind the session could not name, and a mapped kind whose channel is not
        // a mob's: both draw nothing.
        view.observe(
            vec![
                mob_frame(1, EntityKind::Unknown, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(2, EntityKind::Pig, EntityExtra::None),
            ],
            t0,
        );
        assert_eq!(draws_at(&view, t0, Duration::ZERO).len(), 0);
    }

    /// An object frame: the player template's pairs with the object kind and its own
    /// channel.
    fn object_frame(id: i32, kind: EntityKind, extra: EntityExtra) -> EntityFrame {
        let mut frame = player_frame(id, UUID_WIDE);
        frame.uuid = None;
        frame.kind = kind;
        frame.extra = extra;
        frame
    }

    /// The item entities: the wire table's own split — a block id draws the baked
    /// model, a sprite id the generated shape, and an id no entry names nothing at all
    /// (`EntityItem`'s stack, resolved through the renderer's own model table).
    #[test]
    fn the_item_entities_resolve_through_the_wire_table() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                object_frame(
                    1,
                    EntityKind::Item,
                    EntityExtra::Item {
                        id: 5,
                        count: 3,
                        damage: 9,
                    },
                ),
                object_frame(
                    2,
                    EntityKind::Item,
                    EntityExtra::Item {
                        id: 280,
                        count: 2,
                        damage: 0,
                    },
                ),
                object_frame(
                    3,
                    EntityKind::Item,
                    EntityExtra::Item {
                        id: 259,
                        count: 1,
                        damage: 0,
                    },
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 2, "the id with no model draws nothing");
        assert_eq!(draws[0].model, ModelRef::BlockItem { block: 5 });
        assert_eq!(
            draws[0].extra,
            DrawExtra::Item {
                id: 5,
                count: 3,
                damage: 9,
            }
        );
        assert_eq!(draws[1].model, ModelRef::Sprite { key: "items/stick" });
        assert_eq!(
            draws[1].extra,
            DrawExtra::Item {
                id: 280,
                count: 2,
                damage: 0,
            }
        );
    }

    /// Every projectile kind maps to its class's billboard: the arrow's own geometry,
    /// the snowball family's generated shape under `RenderSnowball`'s transform, and
    /// the fireball family's icon quad under `RenderFireball`'s own scale — the
    /// ghast's `2.0`, the blaze's small `0.5` and the wither skull's `1.0`
    /// (`RenderManager.java`:177-186).
    #[test]
    fn every_projectile_kind_maps_to_its_billboard() {
        use oxide_render::entity_models::objects::Billboard;

        let mut view = View::new();
        let t0 = Instant::now();
        let kinds = [
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
        ];
        view.observe(
            kinds
                .iter()
                .enumerate()
                .map(|(index, kind)| object_frame(index as i32 + 1, *kind, EntityExtra::Projectile))
                .collect(),
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), kinds.len(), "every projectile kind draws");
        let thrown = || DrawExtra::Projectile {
            billboard: Billboard::Snowball,
            scale: 0.5,
        };
        let fireball = |scale: f32| DrawExtra::Projectile {
            billboard: Billboard::Fireball,
            scale,
        };
        let expected: [(ModelRef, TextureRef, DrawExtra); 11] = [
            (
                ModelRef::Arrow,
                TextureRef::Named("entity/arrow.png"),
                DrawExtra::None,
            ),
            (
                ModelRef::Sprite {
                    key: "items/snowball",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite { key: "items/egg" },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/ender_pearl",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/ender_eye",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/potion_bottle_drinkable",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/experience_bottle",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireworks",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named("misc/shadow.png"),
                fireball(2.0),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named("misc/shadow.png"),
                fireball(0.5),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named("misc/shadow.png"),
                fireball(1.0),
            ),
        ];
        for (index, (draw, (model, texture, extra))) in
            draws.iter().zip(expected.iter()).enumerate()
        {
            assert_eq!(&draw.model, model, "kind {index}'s model");
            assert_eq!(&draw.texture, texture, "kind {index}'s sheet");
            assert_eq!(&draw.extra, extra, "kind {index}'s billboard");
        }
    }

    /// A painting's draw: the art's own table index and the hanging's facing byte
    /// (`EntityPainting`'s spawn, folded by `EntityHanging`'s yaw rule); an unknown
    /// title falls back to `Kebab`, the table's first art.
    #[test]
    fn a_painting_maps_its_art_and_facing() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            (0..4u8)
                .map(|facing| {
                    object_frame(
                        i32::from(facing) + 1,
                        EntityKind::Painting,
                        EntityExtra::Painting {
                            title: Arc::from("Wither"),
                            facing,
                        },
                    )
                })
                .collect(),
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 4);
        for (facing, draw) in draws.iter().enumerate() {
            assert_eq!(draw.model, ModelRef::Painting { art: 19 });
            assert_eq!(draw.texture, TextureRef::Named(objects::PAINTING_TEXTURE));
            assert_eq!(
                draw.extra,
                DrawExtra::Painting {
                    facing: facing as u8
                }
            );
        }
        // The unknown title falls back to the first art.
        view.observe(
            vec![object_frame(
                9,
                EntityKind::Painting,
                EntityExtra::Painting {
                    title: Arc::from("no such art"),
                    facing: 0,
                },
            )],
            t0,
        );
        assert_eq!(
            draws_at(&view, t0, Duration::ZERO)[0].model,
            ModelRef::Painting { art: 0 }
        );
    }

    /// The item frame's content resolution: the empty frame, a block stack's small
    /// block, an item stack's generated shape, an id with no model's empty frame, and
    /// the rotation slot riding the draw.
    #[test]
    fn the_frame_maps_its_content_and_rotation() {
        let mut view = View::new();
        let t0 = Instant::now();
        let frame = |id: i32, item: Option<MetadataItem>, rotation: u8| {
            object_frame(
                id,
                EntityKind::ItemFrame,
                EntityExtra::ItemFrame { item, rotation },
            )
        };
        view.observe(
            vec![
                frame(1, None, 0),
                frame(
                    2,
                    Some(MetadataItem {
                        id: 5,
                        count: 1,
                        damage: 0,
                    }),
                    3,
                ),
                frame(
                    3,
                    Some(MetadataItem {
                        id: 280,
                        count: 1,
                        damage: 0,
                    }),
                    7,
                ),
                frame(
                    4,
                    Some(MetadataItem {
                        id: 259,
                        count: 1,
                        damage: 0,
                    }),
                    1,
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 4);
        assert_eq!(
            draws[0].model,
            ModelRef::ItemFrame {
                content: FrameContent::Empty
            }
        );
        assert_eq!(
            draws[1].model,
            ModelRef::ItemFrame {
                content: FrameContent::Block(5)
            }
        );
        assert_eq!(
            draws[2].model,
            ModelRef::ItemFrame {
                content: FrameContent::Sprite("items/stick")
            }
        );
        assert_eq!(
            draws[3].model,
            ModelRef::ItemFrame {
                content: FrameContent::Empty
            }
        );
        assert_eq!(draws[1].extra, DrawExtra::Frame { rotation: 3 });
        assert_eq!(draws[2].extra, DrawExtra::Frame { rotation: 7 });
    }

    /// The vehicles and the orb map to their own models and sheets.
    ///
    /// The wire's cart sub-types are not carried by the game's frames yet — the
    /// session folds every cart kind to one (`oxide-game`'s spawn mapping) — so every
    /// cart draws the plain body here; the cargo table itself is pinned in `objects.rs`
    /// and the pass's own case.
    #[test]
    fn the_vehicles_and_orb_map_to_their_models_and_sheets() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                object_frame(1, EntityKind::Boat, EntityExtra::Boat),
                object_frame(2, EntityKind::Minecart, EntityExtra::Minecart),
                object_frame(3, EntityKind::XpOrb, EntityExtra::Orb),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 3);
        assert_eq!(draws[0].model, ModelRef::Boat);
        assert_eq!(draws[0].texture, TextureRef::Named(objects::BOAT_TEXTURE));
        assert_eq!(draws[1].model, ModelRef::Minecart { body: 0 });
        assert_eq!(
            draws[1].texture,
            TextureRef::Named(objects::MINECART_TEXTURE)
        );
        assert_eq!(draws[2].model, ModelRef::Orb { value: 1 });
        assert_eq!(draws[2].texture, TextureRef::Named(objects::ORB_TEXTURE));
    }

    // ---- the chat mirror ----

    /// A 128x128 synthetic sheet whose `'A'` cell is inked in columns 0..=4: the same
    /// metric the game's chat suite and the render-side text tests measure with — `'A'`
    /// advances six font pixels, the space the source's own four, every other blank
    /// cell one.
    fn chat_font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        let code = 'A' as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            for column in 0..=4 {
                let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        Font::load(
            &oxide_assets::texture::Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet loads")
    }

    /// The scaled resolution the draws assemble against: the 1280x720 window at the
    /// settings default, 427x240 (`ScaledResolution.java`:27-30, `:37-40`).
    fn chat_resolution() -> ScaledResolution {
        scaled_resolution(1280, 720, 0)
    }

    /// The mirror's draws at `tick`: the log's fade and the record line's clock read
    /// the tick the frame draws at.
    fn chat_draws(chat: &mut ChatView, tick: u64) -> Vec<HudDraw> {
        chat.update(tick);
        chat.draws(chat_resolution())
    }

    /// A text draw's own fields, for the pins.
    fn chat_text(draw: &HudDraw) -> (String, f32, f32, f32, [f32; 4], bool) {
        match draw {
            HudDraw::Text {
                text,
                x,
                y,
                scale,
                colour,
                shadow,
            } => (text.clone(), *x, *y, *scale, *colour, *shadow),
            other => panic!("a text draw: {other:?}"),
        }
    }

    /// A rect draw's own fields, for the pins.
    fn chat_rect(draw: &HudDraw) -> (f32, f32, f32, f32, [f32; 4]) {
        match draw {
            HudDraw::Rect {
                x,
                y,
                width,
                height,
                colour,
            } => (*x, *y, *width, *height, *colour),
            other => panic!("a rect draw: {other:?}"),
        }
    }

    #[test]
    fn a_chat_line_draws_at_its_tick_and_not_past_the_fade() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        // A position-code-1 message — the box's own kind — logged at tick 1000.
        chat.observe("\"A\"", 1, 1_000);

        assert_eq!(
            (chat_resolution().width, chat_resolution().height),
            (427, 240)
        );
        let draws = chat_draws(&mut chat, 1_000);
        assert_eq!(draws.len(), 2, "one bar and its text");
        // The bar: x 2, top 240 - 37, 324 = the 320 wrap budget plus four wide, nine
        // tall — the pitch's own — and black at 255 / 2 = 127 over 255
        // (`GuiNewChat.java`:49-51, `:81-82`).
        assert_eq!(
            chat_rect(&draws[0]),
            (
                2.0,
                240.0 - 37.0,
                324.0,
                9.0,
                [0.0, 0.0, 0.0, 127.0 / 255.0]
            )
        );
        // The text: x 2, one pixel below the bar's top, scale one, the line's runs as
        // their legacy string, white at 255 over 255, shadowed
        // (`GuiNewChat.java`:83-85).
        assert_eq!(
            chat_text(&draws[1]),
            (
                "A§r".to_owned(),
                2.0,
                240.0 - 36.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );

        // The fade at whole ticks (`GuiNewChat.java`:63-68, `:78`): age 197 leaves the
        // last drawable alpha, five, and its bar halves to two; ages 198 and 200 are
        // gone — the arithmetic leaves two and zero, under the gate.
        let draws = chat_draws(&mut chat, 1_000 + 197);
        assert_eq!(draws.len(), 2, "the last drawn tick");
        assert_eq!(
            chat_rect(&draws[0]).4,
            [0.0, 0.0, 0.0, 2.0 / 255.0],
            "5 / 2 = 2 at age 197"
        );
        assert_eq!(chat_text(&draws[1]).4, [1.0, 1.0, 1.0, 5.0 / 255.0]);
        assert!(chat_draws(&mut chat, 1_000 + 198).is_empty());
        assert!(chat_draws(&mut chat, 1_000 + 200).is_empty());
    }

    #[test]
    fn the_scroll_shifts_the_drawn_slice() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        for index in 1..=12 {
            chat.observe(&format!("\"x{index}\""), 1, 0);
        }
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(draws.len(), 20, "ten lines closed, two draws each");
        assert_eq!(chat_text(&draws[1]).0, "x12§r", "newest first");
        assert_eq!(chat_text(&draws[19]).0, "x3§r", "the tenth kept line");

        // Two lines of scroll move the slice two older (`GuiNewChat.scroll`:222-237);
        // the closed window still shows ten, so the oldest kept line is in view.
        chat.scroll(2);
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(draws.len(), 20);
        assert_eq!(chat_text(&draws[1]).0, "x10§r");
        assert_eq!(chat_text(&draws[19]).0, "x1§r", "the oldest kept line");

        // Resetting walks it back: the slice starts at the newest again.
        chat.reset_scroll();
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(chat_text(&draws[1]).0, "x12§r");
    }

    #[test]
    fn a_position_two_message_draws_above_the_hotbar_and_stays_out_of_the_box() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        chat.observe("\"hi\"", 1, 1_000);
        chat.observe("\"tip\"", 2, 1_000);

        // The record line draws first (`GuiIngame.java`:245-272 runs before the chat
        // block at `:339-347`), centred: 427 / 2 - 3 / 2 = 212 (`:259`, `:269`), four
        // pixels above the box's `height - 68` line, white at full alpha, no shadow
        // (`:269`).
        let draws = chat_draws(&mut chat, 1_000);
        assert_eq!(draws.len(), 3);
        assert_eq!(
            chat_text(&draws[0]),
            (
                "tip".to_owned(),
                212.0,
                240.0 - 72.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                false
            )
        );
        // The position-1 message is not lost: it is the box's own line, and the
        // position-2 message never entered the box.
        assert_eq!(
            chat_rect(&draws[1]),
            (
                2.0,
                240.0 - 37.0,
                324.0,
                9.0,
                [0.0, 0.0, 0.0, 127.0 / 255.0]
            )
        );
        assert_eq!(chat_text(&draws[2]).0, "hi§r");

        // The record line holds sixty ticks (`GuiIngame.java`:1118-1122): its last
        // full one reads (int)(1 * 255 / 20) = 12, and the next tick is gone.
        let draws = chat_draws(&mut chat, 1_000 + 59);
        assert_eq!(chat_text(&draws[0]).4, [1.0, 1.0, 1.0, 12.0 / 255.0]);
        assert_eq!(draws.len(), 3, "the box's own line is still fading");
        let draws = chat_draws(&mut chat, 1_000 + 60);
        assert_eq!(draws.len(), 2, "the record line's sixty ticks are up");
        assert_eq!(chat_text(&draws[1]).0, "hi§r");
    }

    #[test]
    fn a_line_recomposes_its_styles_into_the_legacy_string() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        // The runs recompose the way `getFormattedText` does
        // (`ChatComponentStyle.java`:87-99, `ChatStyle.getFormattingCode`:306-346):
        // the colour code, the style codes in the source's order, the characters,
        // then a reset.
        chat.observe(
            "{\"text\":\"A\",\"color\":\"red\",\"bold\":true,\"italic\":true}",
            1,
            0,
        );
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(chat_text(&draws[1]).0, "§c§l§oA§r");
    }

    #[test]
    fn the_opacity_factor_is_one_at_the_settings_default() {
        // `chatOpacity * 0.9F + 0.1F` (`GuiNewChat.java`:38) at the source's default
        // 1.0F (`GameSettings.java`:85): the f32 sum lands on 1.0 exactly, so the
        // fade byte passes through untouched.
        assert_eq!(opacity_factor(1.0), 1.0);
    }

    #[test]
    fn a_message_before_the_font_waits_for_it() {
        // The window can see chat before the asset store lands: the mirror holds the
        // parsed component until the font arrives, then wraps it.
        let mut chat = ChatView::new();
        chat.observe("\"A\"", 0, 7);
        assert!(chat_draws(&mut chat, 7).is_empty(), "nothing measures yet");
        chat.set_font(chat_font());
        let draws = chat_draws(&mut chat, 7);
        assert_eq!(draws.len(), 2);
        assert_eq!(chat_text(&draws[1]).0, "A§r");
    }
}
