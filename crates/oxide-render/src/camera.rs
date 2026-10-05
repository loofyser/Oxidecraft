//! The camera: the server-reported pose, turned into a view and a projection, plus the
//! frame's formula set — the FOV chain with its smoother, the view bobbing, the hurt roll,
//! and the pose interpolation between the two most recent ticks.

use glam::{Mat4, Vec3};

/// Where the camera is and where it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    /// Feet position, as the server reports it.
    pub position: [f64; 3],
    /// Yaw in degrees: 0 faces south (+Z), 90 faces west (-X), as vanilla.
    pub yaw: f32,
    /// Pitch in degrees: positive looks down, as vanilla.
    pub pitch: f32,
}

impl CameraPose {
    /// The eye position: the pose's feet position plus [`EYE_HEIGHT`].
    ///
    /// This is the entity eye the source's gates measure from (the cloud split at
    /// `EntityRenderer.java:1364` and `:1474`, the raycast origin); the first-person
    /// camera's own offset is [`render_eye`]'s, not this.
    pub fn eye(&self) -> Vec3 {
        Vec3::new(
            self.position[0] as f32,
            self.position[1] as f32 + EYE_HEIGHT,
            self.position[2] as f32,
        )
    }

    /// The unit forward vector for the pose's yaw and pitch.
    pub fn forward(&self) -> Vec3 {
        let yaw = self.yaw.to_radians();
        let pitch = self.pitch.to_radians();
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        Vec3::new(-sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch)
    }
}

/// A perspective camera following a pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where the camera is.
    pub pose: CameraPose,
    /// Vertical field of view in degrees.
    pub fov_degrees: f32,
    /// The near plane.
    pub near: f32,
    /// The far plane in chunks; the plane itself is `far_chunks * 16 * √2`.
    pub far_chunks: f32,
    /// The frame's view-space effect, composed before the camera's own view: the view
    /// bobbing and the hurt roll, as [`camera_effect`] builds them. [`NO_VIEW_EFFECT`]
    /// when neither applies.
    pub view_effect: Mat4,
}

/// The eye height above the feet position, as vanilla uses.
pub const EYE_HEIGHT: f32 = 1.62;
/// Vanilla's default vertical field of view.
pub const DEFAULT_FOV: f32 = 70.0;
/// Vanilla's near plane.
pub const NEAR_PLANE: f32 = 0.05;
/// The first-person camera's backward offset along the view axis, in blocks.
///
/// The source's first-person camera is translated back along the view axis
/// before the view rotations, so the world is rendered from a point this far
/// behind the eye (`EntityRenderer.orientCamera`'s `thirdPersonView == 0`
/// branch: `GlStateManager.translate(0.0F, 0.0F, -0.1F)`,
/// `EntityRenderer.java:720`).
pub const FIRST_PERSON_OFFSET: f32 = 0.1;
/// The identity view effect: no bobbing and no hurt roll on the frame.
pub const NO_VIEW_EFFECT: Mat4 = Mat4::IDENTITY;

/// The capabilities' walk speed, `private float walkSpeed = 0.1F`
/// (`PlayerCapabilities.java:24`), widened to the double the attribute arithmetic divides by.
const WALK_SPEED: f32 = 0.1;

/// The renderer's eye: the pose's eye less [`FIRST_PERSON_OFFSET`] along the view axis.
///
/// The source's first-person camera translates a tenth of a block back along the view axis
/// before the view rotations (`EntityRenderer.orientCamera`'s `thirdPersonView == 0` branch,
/// `EntityRenderer.java:718-721`). The interaction raycast starts at the plain eye
/// ([`CameraPose::eye`]); only the renderer's camera takes the offset.
pub fn render_eye(pose: &CameraPose) -> Vec3 {
    pose.eye() - FIRST_PERSON_OFFSET * pose.forward()
}

impl Camera {
    /// The eye position: the pose's eye, without the renderer's offset.
    pub fn eye(&self) -> Vec3 {
        self.pose.eye()
    }

    /// The unit forward vector for the pose's yaw and pitch.
    pub fn forward(&self) -> Vec3 {
        self.pose.forward()
    }

    /// The view matrix.
    ///
    /// The camera sits [`FIRST_PERSON_OFFSET`] blocks behind the eye on the view axis, as
    /// the source's first-person camera does, and the frame's [`Self::view_effect`] is
    /// composed in front of it.
    pub fn view(&self) -> Mat4 {
        self.view_effect * Mat4::look_to_rh(render_eye(&self.pose), self.pose.forward(), Vec3::Y)
    }

    /// The projection matrix for an aspect ratio. The far plane is
    /// `far_chunks * 16 * √2`, matching the vanilla projection's depth range for
    /// the configured render distance (spec section 10).
    ///
    /// The convention is right-handed with a 0..1 depth range: near maps to
    /// depth 0 and the far plane to depth 1, which is what wgpu's depth test
    /// expects. The OpenGL variants (`perspective_*_gl`) map to -1..1 instead
    /// and would fail it.
    pub fn projection(&self, aspect: f32) -> Mat4 {
        let far = self.far_chunks * 16.0 * std::f32::consts::SQRT_2;
        Mat4::perspective_rh(
            self.fov_degrees.to_radians(),
            aspect.max(0.01),
            self.near,
            far,
        )
    }

    /// The combined view-projection matrix.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }
}

/// The smoothable FOV modifier's inputs, read by `AbstractClientPlayer.getFovModifier`
/// (`AbstractClientPlayer.java:109-144`): the two movement flags and the bow's item-use
/// counter.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FovInputs {
    /// Whether the player is sprinting.
    pub sprinting: bool,
    /// Whether the player is flying.
    pub flying: bool,
    /// The bow's item-use duration in ticks, 0 while no bow is drawn. The counter arrives
    /// with M5; the term is the identity at zero.
    pub item_use_ticks: i32,
}

/// The movement-speed attribute value the client player carries: the base of
/// `EntityPlayer.applyEntityAttributes` (`EntityPlayer.java:193`, the double nearest the
/// float `0.1F`) and, while sprinting, `EntityLivingBase`'s "Sprinting speed boost"
/// modifier (`EntityLivingBase.java:56-57`: amount `0.30000001192092896`, operation 2 —
/// multiply the total — applied by `setSprinting`, `:1467-1481`).
fn movement_speed(sprinting: bool) -> f64 {
    // The base is the double nearest the float `0.1F` (`EntityPlayer.java:193`); the sprint
    // modifier multiplies the total by `1 + 0.30000001192092896`
    // (`EntityLivingBase.java:56-57`).
    let mut value = f64::from(0.1_f32);
    if sprinting {
        value *= 1.0 + f64::from(0.3_f32);
    }
    value
}

/// The smoothable FOV modifier (`AbstractClientPlayer.getFovModifier`,
/// `AbstractClientPlayer.java:109-144`).
///
/// The flying factor is ×1.1; the movement-speed attribute arithmetic
/// `(attribute / walk_speed + 1) / 2` gives 1.15 under sprint; the bow term is
/// `1 − f1² × 0.15` with `f1 = min(item_use_ticks / 20, 1)` below twenty ticks and 1
/// beyond, and the identity when no bow is in use. The water and death terms are not
/// part of this — they are [`fov`]'s per-frame factors.
pub fn fov_modifier(inputs: &FovInputs) -> f32 {
    let mut modifier = 1.0_f64;
    if inputs.flying {
        modifier *= 1.1;
    }
    modifier *= (movement_speed(inputs.sprinting) / f64::from(WALK_SPEED) + 1.0) / 2.0;
    if inputs.item_use_ticks > 0 {
        let mut item_use = f64::from(inputs.item_use_ticks) / 20.0;
        if item_use > 1.0 {
            item_use = 1.0;
        } else {
            item_use *= item_use;
        }
        modifier *= 1.0 - item_use * 0.15;
    }
    modifier as f32
}

/// The FOV smoother: `EntityRenderer.fovModifierHand` and its previous tick's copy
/// (`updateFovModifierHand`, `EntityRenderer.java:522-543`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FovSmoother {
    /// The smoothed hand.
    pub hand: f32,
    /// The hand's previous tick's copy.
    pub prev: f32,
}

impl Default for FovSmoother {
    /// A hand at the identity, 1.0.
    ///
    /// The source's raw fields start at zero and are only blended inside a running world;
    /// this client starts at the identity so the first tick does not zoom the world.
    fn default() -> Self {
        Self {
            hand: 1.0,
            prev: 1.0,
        }
    }
}

impl FovSmoother {
    /// One tick's blend toward the modifier (`updateFovModifierHand`,
    /// `EntityRenderer.java:532-537`): the previous hand takes the current one, then the
    /// current one steps half the remaining distance toward the target and clamps to
    /// [0.1, 1.5].
    pub fn step(&mut self, target: f32) {
        self.prev = self.hand;
        self.hand += (target - self.hand) * 0.5;
        self.hand = self.hand.clamp(0.1, 1.5);
    }
}

/// The vertical field of view for a frame (`EntityRenderer.getFOVModifier`,
/// `EntityRenderer.java:551-583`).
///
/// The base is the setting times the smoothed hand's frame-fraction lerp; the death
/// division `1 / ((1 − 500/(death_time + partial + 500)) × 2 + 1)` and the water factor
/// `× 60/70` are applied after it, unsmoothed, each only while its condition holds. The
/// source's debug-camera branch is not ported.
pub fn fov(
    setting: f32,
    smoother: &FovSmoother,
    partial: f32,
    in_water: bool,
    dead: bool,
    death_time: u32,
) -> f32 {
    let mut field_of_view = setting * (smoother.prev + (smoother.hand - smoother.prev) * partial);
    if dead {
        let clock = death_time as f32 + partial;
        field_of_view /= (1.0 - 500.0 / (clock + 500.0)) * 2.0 + 1.0;
    }
    if in_water {
        field_of_view *= 60.0 / 70.0;
    }
    field_of_view
}

/// The walk-distance pair the view bob reads: `Entity.distanceWalkedModified` and its
/// previous tick's copy (`Entity.java:872`, `:420`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WalkDistance {
    /// The walk distance the frame interpolates from.
    pub distance: f32,
    /// The previous tick's walk distance.
    pub previous: f32,
}

/// One damped camera sensor and its previous tick's copy: `cameraYaw`/`prevCameraYaw` or
/// `cameraPitch`/`prevCameraPitch` (`EntityPlayer.java:618`, `:653-654`;
/// `EntityLivingBase.java:335`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CameraSensor {
    /// The sensor's current value.
    pub value: f32,
    /// The sensor's previous tick's copy.
    pub previous: f32,
}

/// One rotation of a view-space effect: the axis and the angle in degrees about it,
/// right-handed, as the source's `GlStateManager.rotate` calls carry them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewRotation {
    /// The rotation's axis in view space.
    pub axis: [f32; 3],
    /// The angle in degrees about the axis.
    pub degrees: f32,
}

/// The view bob's translation in view space (`setupViewBobbing`'s `translate`,
/// `EntityRenderer.java:615-629`): `sin(f1 π) × f2 × 0.5` across, `−|cos(f1 π) × f2|`
/// up, zero along the view axis.
///
/// `f1 = −(distance + (distance − previous) × partial)` is the walk phase and
/// `f2` the yaw sensor at the frame fraction.
pub fn bob_translate(distance: WalkDistance, camera_yaw: CameraSensor, partial: f32) -> [f32; 3] {
    let step = distance.distance - distance.previous;
    let walk = -(distance.distance + step * partial);
    let yaw = camera_yaw.previous + (camera_yaw.value - camera_yaw.previous) * partial;
    let (sin, cos) = (walk * std::f32::consts::PI).sin_cos();
    [sin * yaw * 0.5, -(cos * yaw).abs(), 0.0]
}

/// The view bob's three rotations (`setupViewBobbing`, `EntityRenderer.java:615-629`):
/// the roll `sin(f1 π) × f2 × 3` about z, the sway `|cos(f1 π − 0.2) × f2| × 5` about x,
/// then the pitch sensor's frame value about x.
pub fn bob_rotations(
    distance: WalkDistance,
    camera_yaw: CameraSensor,
    camera_pitch: CameraSensor,
    partial: f32,
) -> [ViewRotation; 3] {
    let step = distance.distance - distance.previous;
    let walk = -(distance.distance + step * partial);
    let yaw = camera_yaw.previous + (camera_yaw.value - camera_yaw.previous) * partial;
    let pitch = camera_pitch.previous + (camera_pitch.value - camera_pitch.previous) * partial;
    let sin = (walk * std::f32::consts::PI).sin();
    let lean = (walk * std::f32::consts::PI - 0.2).cos();
    [
        ViewRotation {
            axis: [0.0, 0.0, 1.0],
            degrees: sin * yaw * 3.0,
        },
        ViewRotation {
            axis: [1.0, 0.0, 0.0],
            degrees: (lean * yaw).abs() * 5.0,
        },
        ViewRotation {
            axis: [1.0, 0.0, 0.0],
            degrees: pitch,
        },
    ]
}

/// The hurt roll's view-space rotations (`hurtCameraEffect`,
/// `EntityRenderer.java:585-609`).
///
/// While the player is dead the death tilt `40 − 8000/(death_time + partial + 200)`
/// about z comes first (`:590-595`). The flash's own rotations follow while
/// `hurt_time − partial` is not negative (`:597-600`): `−sin((t/max)⁴ π) × 14` about z
/// inside the `attackedAtYaw` sandwich — `rotate(−yaw, y)`, the roll, `rotate(+yaw, y)` —
/// of `:602-607`.
pub fn hurt_roll(
    hurt_time: u32,
    max_hurt_time: u32,
    partial: f32,
    attacked_at_yaw: f32,
    dead: bool,
    death_time: u32,
) -> Vec<ViewRotation> {
    let mut rotations = Vec::with_capacity(4);
    if dead {
        let clock = death_time as f32 + partial;
        rotations.push(ViewRotation {
            axis: [0.0, 0.0, 1.0],
            degrees: 40.0 - 8000.0 / (clock + 200.0),
        });
    }
    let mut flash = hurt_time as f32 - partial;
    if flash < 0.0 {
        return rotations;
    }
    flash /= max_hurt_time as f32;
    flash = (flash * flash * flash * flash * std::f32::consts::PI).sin();
    rotations.push(ViewRotation {
        axis: [0.0, 1.0, 0.0],
        degrees: -attacked_at_yaw,
    });
    rotations.push(ViewRotation {
        axis: [0.0, 0.0, 1.0],
        degrees: -flash * 14.0,
    });
    rotations.push(ViewRotation {
        axis: [0.0, 1.0, 0.0],
        degrees: attacked_at_yaw,
    });
    rotations
}

/// The pose a frame draws between the two most recent ticks, at the frame fraction.
///
/// A tick-to-tick jump of more than four blocks is a server correction, not movement:
/// the frame takes the current pose whole instead of sweeping the camera through the
/// world (spec P5's snap rule). The fraction is expected in `0..=1`; the client clamps it.
pub fn interpolate_pose(prev: CameraPose, cur: CameraPose, partial: f32) -> CameraPose {
    let dx = cur.position[0] - prev.position[0];
    let dy = cur.position[1] - prev.position[1];
    let dz = cur.position[2] - prev.position[2];
    if dx * dx + dy * dy + dz * dz > SNAP_BLOCKS * SNAP_BLOCKS {
        return cur;
    }
    let fraction = f64::from(partial);
    CameraPose {
        position: [
            prev.position[0] + dx * fraction,
            prev.position[1] + dy * fraction,
            prev.position[2] + dz * fraction,
        ],
        yaw: prev.yaw + (cur.yaw - prev.yaw) * partial,
        pitch: prev.pitch + (cur.pitch - prev.pitch) * partial,
    }
}

/// The teleport threshold, in blocks: a jump beyond four is a correction.
const SNAP_BLOCKS: f64 = 4.0;

/// The view-space transform of one effect: a translation, then the rotations in the given
/// order. The source's camera effects post-multiply the modelview, so a later call acts
/// on a point first; composing a translation then each rotation reproduces that order.
fn view_transform(translate: [f32; 3], rotations: &[ViewRotation]) -> Mat4 {
    let mut transform = Mat4::from_translation(Vec3::from(translate));
    for rotation in rotations {
        transform *=
            Mat4::from_axis_angle(Vec3::from(rotation.axis), rotation.degrees.to_radians());
    }
    transform
}

/// The frame's view-space effect: the hurt roll composed over the view bobbing, in the
/// order `setupCameraTransform` applies them (`hurtCameraEffect` before `setupViewBobbing`,
/// `EntityRenderer.java:775-780`).
pub fn camera_effect(
    translate: [f32; 3],
    rotations: &[ViewRotation; 3],
    hurt: &[ViewRotation],
) -> Mat4 {
    view_transform([0.0; 3], hurt) * view_transform(translate, rotations)
}

#[cfg(test)]
mod tests {
    use super::{
        Camera, CameraPose, CameraSensor, DEFAULT_FOV, EYE_HEIGHT, FIRST_PERSON_OFFSET, FovInputs,
        FovSmoother, NEAR_PLANE, NO_VIEW_EFFECT, ViewRotation, WalkDistance, bob_rotations,
        bob_translate, camera_effect, fov, fov_modifier, hurt_roll, interpolate_pose, render_eye,
    };
    use glam::{Vec3, Vec4};

    /// The sample wall's face plane: the wall occupies z = 164, so the face the
    /// camera sees is at z = 165.
    const WALL_PLANE_Z: f32 = 165.0;

    /// The M2 acceptance's wall pose: feet (7.5, 57, z), yaw 180 (facing the
    /// wall square-on), pitch 0, with the acceptance's fov and render distance.
    fn wall_camera(feet_z: f64) -> Camera {
        Camera {
            pose: CameraPose {
                position: [7.5, 57.0, feet_z],
                yaw: 180.0,
                pitch: 0.0,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: NO_VIEW_EFFECT,
        }
    }

    /// The wall face's horizontal span in pixels through the camera's own
    /// view-projection at the capture resolution's 1280x720 aspect.
    fn face_span_px(camera: &Camera) -> f32 {
        let projector = camera.view_projection(1280.0 / 720.0);
        let project = |x: f32| {
            let clip = projector * Vec4::new(x, 57.0 + EYE_HEIGHT, WALL_PLANE_Z, 1.0);
            (clip.x / clip.w * 0.5 + 0.5) * 1280.0
        };
        project(16.0) - project(0.0)
    }

    #[test]
    fn the_first_person_camera_is_a_tenth_of_a_block_behind_the_eye() {
        // The source's first-person camera is translated back before the view
        // rotations (`EntityRenderer.orientCamera`'s `thirdPersonView == 0`
        // branch: `GlStateManager.translate(0.0F, 0.0F, -0.1F)`,
        // `EntityRenderer.java:720`), so at the wall pose the camera sits at
        // z = 174.6, a tenth of a block farther from the wall than the eye.
        let camera = wall_camera(174.5);
        let origin = camera.view().inverse().transform_point3(Vec3::ZERO);
        let expected = camera.eye() - 0.1 * camera.forward();
        assert!(
            (origin - expected).length() < 5e-4,
            "the camera {origin:?} is not the eye minus the tenth-block offset {expected:?}"
        );
        assert!(
            (origin.z - 174.6).abs() < 5e-4,
            "the camera sits at z {}, not 174.6",
            origin.z
        );
    }

    #[test]
    fn the_wall_face_projects_to_the_reference_span_at_two_distances() {
        // The reference client's face span with the tenth-block offset camera is
        // 16 * (360 / tan 35 deg) / (distance + 0.1): 856.9 px at the wall pose
        // (distance 9.5; the live capture refs/rig/evidence/m2/vanilla-wall.png
        // measures 857.06 px) and 1082.4 px at the second pose (distance 7.5; the
        // live capture refs/m2-render-align/vanilla-wall-d75.png measures
        // 1082.07 px). A frame without the offset projects 865.9 px and
        // 1096.8 px instead.
        for (distance, reference) in [(9.5, 856.9_f32), (7.5, 1082.4_f32)] {
            let camera = wall_camera(165.0 + distance);
            let span = face_span_px(&camera);
            assert!(
                (span - reference).abs() < 1.0,
                "the wall face spans {span:.2} px at distance {distance}, the reference {reference:.1}"
            );
        }
    }

    #[test]
    fn the_fov_literals_are_the_sources_own() {
        // `AbstractClientPlayer.getFovModifier` (`AbstractClientPlayer.java:109-144`) at
        // `EntityRenderer.getFOVModifier`'s product (`:564-565`) with the default setting:
        // the walk modifier is the identity, the sprint attribute arithmetic gives 1.15,
        // flying multiplies by 1.1, and flying under sprint multiplies both; the water
        // factor at the viewpoint is × 60/70 (`:570-574`) and the death division
        // `1 / ((1 − 500/(deathTime + partial + 500)) × 2 + 1)` (`:561-567`) at
        // deathTime 10 is ≈ 67.36.
        let walk = fov_modifier(&FovInputs::default());
        let sprint = fov_modifier(&FovInputs {
            sprinting: true,
            ..Default::default()
        });
        let flying = fov_modifier(&FovInputs {
            flying: true,
            ..Default::default()
        });
        let both = fov_modifier(&FovInputs {
            sprinting: true,
            flying: true,
            ..Default::default()
        });
        assert!(
            (walk - 1.0).abs() < 1e-6,
            "the walk modifier is the identity"
        );
        assert!(
            (sprint - 1.15).abs() < 1e-4,
            "the attribute arithmetic gives 1.15 under sprint, got {sprint}"
        );
        assert!((flying - 1.1).abs() < 1e-4, "flying multiplies by 1.1");
        assert!(
            (both - 1.265).abs() < 1e-4,
            "flying under sprint multiplies both, got {both}"
        );
        let steady = |hand: f32| FovSmoother { hand, prev: hand };
        let quiet = |hand: f32| fov(DEFAULT_FOV, &steady(hand), 1.0, false, false, 0);
        assert!((quiet(walk) - 70.0).abs() < 1e-3, "walk: 70.0");
        assert!((quiet(sprint) - 80.5).abs() < 1e-3, "sprint: 80.5");
        assert!((quiet(flying) - 77.0).abs() < 1e-3, "flying: 77.0");
        assert!((quiet(both) - 88.55).abs() < 1e-3, "flying + sprint: 88.55");
        assert!(
            (fov(DEFAULT_FOV, &steady(1.0), 0.0, true, false, 0) - 60.0).abs() < 1e-3,
            "water: 60.0"
        );
        assert!(
            (fov(DEFAULT_FOV, &steady(1.0), 0.0, false, true, 10) - 67.36).abs() < 0.01,
            "the death division at deathTime 10, got {}",
            fov(DEFAULT_FOV, &steady(1.0), 0.0, false, true, 10)
        );
    }

    #[test]
    fn the_bow_term_is_the_sources_expression_at_a_synthetic_use_count() {
        // `AbstractClientPlayer.java:126-141`: with a bow in use the duration over twenty
        // clamps to one at the top and squares below it, and `f *= 1 − f1 × 0.15` — ten
        // ticks is 1 − 0.25 × 0.15 = 0.9625, twenty and beyond is 1 − 1 × 0.15 = 0.85.
        // The counter itself arrives with M5; the expression is exercised here at
        // synthetic counts and the identity at zero.
        let bow = |ticks| {
            fov_modifier(&FovInputs {
                item_use_ticks: ticks,
                ..Default::default()
            })
        };
        assert!((bow(0) - 1.0).abs() < 1e-6, "no draw is the identity");
        assert!((bow(10) - 0.9625).abs() < 1e-4);
        assert!((bow(20) - 0.85).abs() < 1e-4);
        assert!((bow(25) - 0.85).abs() < 1e-4, "the ratio clamps at one");
    }

    #[test]
    fn the_smoother_blends_toward_the_modifier_and_clamps() {
        // `updateFovModifierHand` (`EntityRenderer.java:532-537`): prev = hand;
        // hand += (f − hand) × 0.5, clamped to [0.1, 1.5] by the two `if`s. From the
        // identity toward the sprint modifier: 1.075, then 1.1125.
        let mut smoother = FovSmoother::default();
        assert_eq!(smoother.hand, 1.0);
        assert_eq!(smoother.prev, 1.0);
        smoother.step(1.15);
        assert!(
            (smoother.hand - 1.075).abs() < 1e-6,
            "got {}",
            smoother.hand
        );
        assert!(
            (smoother.prev - 1.0).abs() < 1e-6,
            "the pre-blend hand is kept"
        );
        smoother.step(1.15);
        assert!(
            (smoother.hand - 1.1125).abs() < 1e-6,
            "got {}",
            smoother.hand
        );
        assert!((smoother.prev - 1.075).abs() < 1e-6);
        // The clamps: a hand above 1.5 comes back down to it, below 0.1 up to it.
        let mut high = FovSmoother {
            hand: 1.45,
            prev: 1.45,
        };
        high.step(3.0);
        assert_eq!(high.hand, 1.5);
        assert_eq!(high.prev, 1.45, "the clamp lands on the new hand only");
        let mut low = FovSmoother {
            hand: 0.15,
            prev: 0.15,
        };
        low.step(0.0);
        assert_eq!(low.hand, 0.1);
        assert_eq!(low.prev, 0.15);
    }

    #[test]
    fn the_water_and_death_terms_are_not_smoothed() {
        // The smoother's target is the flag-borne modifier only: `getFovModifier` reads no
        // water and no death state, and both of `getFOVModifier`'s terms divide the frame's
        // full base after the lerp (`EntityRenderer.java:561-574`) — so standing in water
        // is the 60/70 factor at any frame fraction while the hand sits still, and the
        // death division follows its own frame fraction rather than the hand's.
        let mut smoother = FovSmoother::default();
        smoother.step(fov_modifier(&FovInputs::default()));
        assert_eq!(smoother.hand, 1.0, "the target carries no water term");
        assert!((fov(DEFAULT_FOV, &smoother, 0.0, true, false, 0) - 60.0).abs() < 1e-3);
        assert!((fov(DEFAULT_FOV, &smoother, 1.0, true, false, 0) - 60.0).abs() < 1e-3);
        let dead_0 = fov(DEFAULT_FOV, &smoother, 0.0, false, true, 10);
        let dead_1 = fov(DEFAULT_FOV, &smoother, 1.0, false, true, 10);
        assert!((dead_0 - 67.36).abs() < 0.01, "got {dead_0}");
        assert!(
            (dead_1 - 67.11).abs() < 0.01,
            "the division follows the frame fraction: got {dead_1}"
        );
        assert!(dead_1 < dead_0, "a deeper death division as the clock runs");
    }

    #[test]
    fn the_frame_fraction_lerps_the_two_smoothed_hands() {
        // `f = fovSetting × (fovModifierHandPrev + (fovModifierHand − fovModifierHandPrev)
        // × partialTicks)` (`EntityRenderer.java:564-565`): half a tick from the identity
        // toward the sprint modifier is 70 × 1.075 = 75.25.
        let smoother = FovSmoother {
            hand: 1.15,
            prev: 1.0,
        };
        assert!((fov(DEFAULT_FOV, &smoother, 0.5, false, false, 0) - 75.25).abs() < 1e-3);
        assert!((fov(DEFAULT_FOV, &smoother, 0.0, false, false, 0) - 70.0).abs() < 1e-3);
        assert!((fov(DEFAULT_FOV, &smoother, 1.0, false, false, 0) - 80.5).abs() < 1e-3);
    }

    #[test]
    fn the_view_bob_literals_are_the_sources_own() {
        // `setupViewBobbing` (`EntityRenderer.java:615-629`) at a worked pair: walk
        // distance 1.2 with its previous 1.0, the yaw sensor at 0.08 from 0.06, the pitch
        // sensor at 0.03 from 0.02, half a tick in. f = 0.2, f1 = −1.3, f2 = 0.07,
        // f3 = 0.025, sin(f1 π) = 0.80901699, cos(f1 π) = −0.58778525,
        // cos(f1 π − 0.2) = −0.41534182.
        let distance = WalkDistance {
            distance: 1.2,
            previous: 1.0,
        };
        let yaw = CameraSensor {
            value: 0.08,
            previous: 0.06,
        };
        let pitch = CameraSensor {
            value: 0.03,
            previous: 0.02,
        };
        let partial = 0.5;
        let translate = bob_translate(distance, yaw, partial);
        assert!(
            (translate[0] - 0.0283156).abs() < 1e-4,
            "x is sin(f1 π) × f2 × 0.5, got {}",
            translate[0]
        );
        assert!(
            (translate[1] - -0.0411450).abs() < 1e-4,
            "y is −|cos(f1 π) × f2|, got {}",
            translate[1]
        );
        assert_eq!(translate[2], 0.0, "no view-axis translation");
        let rotations = bob_rotations(distance, yaw, pitch, partial);
        assert_eq!(
            rotations[0].axis,
            [0.0, 0.0, 1.0],
            "the roll about z comes first"
        );
        assert!(
            (rotations[0].degrees - 0.1698936).abs() < 1e-4,
            "roll is sin(f1 π) × f2 × 3, got {}",
            rotations[0].degrees
        );
        assert_eq!(rotations[1].axis, [1.0, 0.0, 0.0]);
        assert!(
            (rotations[1].degrees - 0.1453696).abs() < 1e-4,
            "sway is |cos(f1 π − 0.2) × f2| × 5, got {}",
            rotations[1].degrees
        );
        assert_eq!(rotations[2].axis, [1.0, 0.0, 0.0]);
        assert!(
            (rotations[2].degrees - 0.025).abs() < 1e-4,
            "the pitch sensor's frame value, got {}",
            rotations[2].degrees
        );
    }

    #[test]
    fn the_hurt_roll_literals_are_the_sources_own() {
        // `hurtCameraEffect` (`EntityRenderer.java:585-609`): f = hurtTime − partial; at
        // (5, 10, 0) the ratio is 0.5, sin(0.5⁴ π) = 0.19509032, and the z-roll is
        // −0.19509032 × 14 ≈ −2.7313°, inside the attackedAtYaw sandwich at 30:
        // rotate(−30, y), the roll, rotate(+30, y).
        let roll = hurt_roll(5, 10, 0.0, 30.0, false, 0);
        assert_eq!(roll.len(), 3);
        assert_eq!(
            roll[0],
            ViewRotation {
                axis: [0.0, 1.0, 0.0],
                degrees: -30.0
            }
        );
        assert_eq!(roll[1].axis, [0.0, 0.0, 1.0]);
        assert!(
            (roll[1].degrees - -2.7312645).abs() < 1e-3,
            "the flash's z-roll, got {}",
            roll[1].degrees
        );
        assert_eq!(
            roll[2],
            ViewRotation {
                axis: [0.0, 1.0, 0.0],
                degrees: 30.0
            }
        );
        // The death tilt `40 − 8000/(deathTime + partial + 200)` about z (`:590-595`)
        // comes before the flash's own rotations; at deathTime 10 and partial 0 it is
        // 1.9048°. A fraction past the flash (hurtTime 0, partial 0.5) leaves only it.
        let dead = hurt_roll(0, 10, 0.0, 0.0, true, 10);
        assert_eq!(dead.len(), 4, "the tilt and the flash's three rotations");
        assert_eq!(dead[0].axis, [0.0, 0.0, 1.0]);
        assert!(
            (dead[0].degrees - 1.904762).abs() < 1e-4,
            "the death tilt, got {}",
            dead[0].degrees
        );
        let late = hurt_roll(0, 10, 0.5, 0.0, true, 10);
        assert_eq!(
            late.len(),
            1,
            "a negative f returns before the flash's rotations"
        );
    }

    #[test]
    fn a_teleport_of_over_four_blocks_is_not_interpolated() {
        // Spec P5's snap rule: a tick-to-tick jump of more than four blocks is a
        // correction, so the frame takes the current pose whole; three blocks interpolate
        // at the frame fraction.
        let origin = CameraPose {
            position: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        };
        let far = CameraPose {
            position: [5.0, 64.0, 0.0],
            yaw: 90.0,
            pitch: 10.0,
        };
        assert_eq!(
            interpolate_pose(origin, far, 0.5),
            far,
            "a five-block jump snaps"
        );
        let near = CameraPose {
            position: [3.0, 64.0, 0.0],
            yaw: 90.0,
            pitch: 10.0,
        };
        let mid = interpolate_pose(origin, near, 0.5);
        assert!((mid.position[0] - 1.5).abs() < 1e-9, "the midpoint");
        assert!((mid.yaw - 45.0).abs() < 1e-4);
        assert!((mid.pitch - 5.0).abs() < 1e-4);
        assert_eq!(interpolate_pose(origin, near, 0.0), origin);
        assert_eq!(interpolate_pose(origin, near, 1.0), near);
    }

    #[test]
    fn the_step_at_exactly_four_blocks_still_slides() {
        // The snap starts only past the boundary (spec P5, `docs/specs/oxidecraft-v1-design.md:92`,
        // restated in section 9 at `:315`: "snapping when a teleport exceeds 4 blocks").
        let origin = CameraPose {
            position: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        };
        let exactly = CameraPose {
            position: [4.0, 64.0, 0.0],
            yaw: 90.0,
            pitch: 10.0,
        };
        let mid = interpolate_pose(origin, exactly, 0.5);
        assert_eq!(
            mid.position,
            [2.0, 64.0, 0.0],
            "a step of exactly four blocks interpolates"
        );
        assert!((mid.yaw - 45.0).abs() < 1e-4);
        assert!((mid.pitch - 5.0).abs() < 1e-4);
        let over = CameraPose {
            position: [4.1, 64.0, 0.0],
            ..exactly
        };
        assert_eq!(
            interpolate_pose(origin, over, 0.5),
            over,
            "a step past four blocks snaps"
        );
    }

    #[test]
    fn the_render_eye_is_the_eye_minus_a_tenth_of_a_block() {
        // `orientCamera`'s first-person branch (`EntityRenderer.java:718-721`) translates
        // the view a tenth of a block back along the view axis; the plain eye — the
        // interaction raycast's origin, `Player::eye_height` on the game side — keeps its
        // standing height and takes no displacement. At feet (1, 2, 3), yaw 90, pitch 30:
        // eye (1, 3.62, 3), forward (−√3/2, −1/2, 0), render eye (1.0866025, 3.67, 3).
        let pose = CameraPose {
            position: [1.0, 2.0, 3.0],
            yaw: 90.0,
            pitch: 30.0,
        };
        let eye = pose.eye();
        assert_eq!(EYE_HEIGHT, 1.62, "the eye height the game side carries too");
        assert!(
            (eye.y - (2.0 + 1.62)).abs() < 1e-6,
            "the eye is not displaced"
        );
        assert_eq!(FIRST_PERSON_OFFSET, 0.1);
        let displaced = render_eye(&pose);
        assert!((displaced.x - 1.0866025).abs() < 1e-5, "got {displaced:?}");
        assert!((displaced.y - 3.67).abs() < 1e-5);
        assert!((displaced.z - 3.0).abs() < 1e-5);
        let back = eye - displaced;
        assert!(
            (back - pose.forward() * FIRST_PERSON_OFFSET).length() < 1e-6,
            "the displacement is exactly the offset along the view axis"
        );
    }

    #[test]
    fn the_camera_effect_is_the_hurt_roll_over_the_view_bobbing() {
        // `setupCameraTransform` (`EntityRenderer.java:775-780`) applies the hurt roll
        // first and the view bobbing after it, both post-multiplied onto the modelview, so
        // a view-space point meets the bobbing first and the roll last. With the bobbing a
        // (1, 2, 3) translation and the hurt roll a right angle about z, the view's own
        // origin (the render eye) lands at (−2, 1, 3): translate first, then rotate.
        let effect = camera_effect(
            [1.0, 2.0, 3.0],
            &[ViewRotation {
                axis: [0.0, 0.0, 1.0],
                degrees: 0.0,
            }; 3],
            &[ViewRotation {
                axis: [0.0, 0.0, 1.0],
                degrees: 90.0,
            }],
        );
        let pose = CameraPose {
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        };
        let camera = Camera {
            pose,
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: effect,
        };
        let origin = render_eye(&pose);
        let image = camera.view().transform_point3(origin);
        assert!(
            (image.x - -2.0).abs() < 1e-5
                && (image.y - 1.0).abs() < 1e-5
                && (image.z - 3.0).abs() < 1e-5,
            "the effect lands the view origin at (−2, 1, 3), got {image:?}"
        );
        let plain = Camera {
            view_effect: NO_VIEW_EFFECT,
            ..camera
        };
        assert!(
            plain.view().transform_point3(origin).length() < 1e-6,
            "without an effect the view origin is the render eye itself"
        );
    }
}
