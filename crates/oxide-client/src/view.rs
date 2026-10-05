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

use std::collections::BTreeMap;
use std::time::Instant;

use oxide_assets::skins::{DefaultModel, default_skin};
use oxide_game::entity_view::EntityFrame;
use oxide_game::session::ClientEvent;
use oxide_render::entity_models::{Pose, PoseExtra};
use oxide_render::entity_pass::{DrawExtra, EntityDraw, ModelRef, TextureRef};
use oxide_world::entity::EntityKind;

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

/// The draw one frame makes, or `None` for a kind with no model yet.
///
/// The model, texture and slim flag come from the skin worker's update for the profile
/// when one exists — as it stands — else from the uuid's own default model rule
/// (`DefaultPlayerSkin.isSlimSkin`, `DefaultPlayerSkin.java:41-44`); the renderer's
/// resolver falls back the same way when the update carries no texture.
fn draw_for(
    frame: &EntityFrame,
    partial: f32,
    skins: &BTreeMap<String, SkinUpdate>,
) -> Option<EntityDraw> {
    if frame.kind != EntityKind::Player {
        tracing::debug!(kind = ?frame.kind, "the entity kind has no model yet");
        return None;
    }
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

    let pose = Pose {
        limb_swing: frame.limb_swing - frame.limb_swing_amount * (1.0 - partial),
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
        child: false,
        extra: PoseExtra::default(),
    };

    Some(EntityDraw {
        model: ModelRef::Player {
            slim,
            parts: ALL_PARTS,
        },
        position,
        body_yaw,
        head_yaw,
        head_pitch,
        pose,
        texture: TextureRef::Skin { uuid, slim },
        light: frame.brightness,
        hurt,
        death,
        health: frame.health,
        extra: DrawExtra::None,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use oxide_assets::skins::DefaultModel;
    use oxide_game::entity_view::{EntityExtra, EntityFrame};
    use oxide_render::entity_pass::{ModelRef, TextureRef};
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
            extra: EntityExtra::None,
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
}
