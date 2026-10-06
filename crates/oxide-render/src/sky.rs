//! The sky pass and the cloud layer: the horizon band, the sun, the moon, the stars, the
//! celestial rotation and the flat clouds, from the decompiled 1.8.9 client's own geometry.
//!
//! # What the passes draw
//!
//! [`SkyPass`] draws, in the source's order, the horizon band, the sun, the moon and the stars
//! in the celestial frame, then the void the source fills in under the horizon and the
//! below-horizon plane (`RenderGlobal.renderSky`, `RenderGlobal.java:1213-1417`):
//!
//! * The band is the source's generated 64-unit grid spanning `-384..=384` on both axes
//!   (`RenderGlobal.java:340-365`) at `y = +16` (`:325`), flat in the frame's sky colour. The
//!   source draws it with fog enabled (`:1231`), disables fog for the celestial quads (`:1248`)
//!   and re-enables it for the void and the below-horizon plane (`:1349`), all under the sky
//!   range `setupFog(-1)` installs (`EntityRenderer.java:1348`, `:2009-2010`).
//! * The below-horizon plane is the same grid generated at `y = -16` with its winding reversed
//!   (`:291`, `renderSky(worldrenderer, -16.0F, true)`), lifted so it sits at the world horizon:
//!   the source translates it by `16 - (eyeY - 63)` `RenderGlobal.java`:1412, reaching
//!   `y = -(eyeY - 63)`, and tints it `(c * 0.2 + 0.04, c * 0.2 + 0.04, c * 0.6 + 0.1)`
//!   (`:1402-1409`). It is fogged (`:1349`). There is no alpha fade in the band or the plane;
//!   the transparent edge the source fades at sunset is the deferred sunset fan (`:1243-1288`).
//! * The void is the source's black box (`:1375-1399`): a `±1` column hanging from one unit under
//!   the eye down to `-(d0 + 65)` with `d0 = eyeY - 63`, drawn only while the eye is below the
//!   horizon (`d0 < 0`), together with the same reversed grid lifted to `y = -4`
//!   `RenderGlobal.java`:1358. It is fogged too (`:1349`); [`void_box_low`] answers the floor.
//! * The sun is a 60 by 60 quad at `y = +100`, textured by `environment/sun` with uvs `0..1`
//!   (`:1301-1308`); the moon a 40 by 40 quad at `y = -100` whose uv picks the phase's cell from
//!   the 128x64 4x2 `environment/moon_phases` sheet (`:1309-1323`, [`moon_uv`]). Both are drawn
//!   with the additive `(SRC_ALPHA, ONE)` blend the source installs (`:1295`), alpha
//!   `1 - rain` (`:1294`; M2 has no weather and uses one), and no fog.
//! * The stars are one sphere of quads at radius 100, generated once from
//!   `java.util.Random(10842L)` by the source's own loop (`:367-451`, [`star_field`]), drawn
//!   position-only in `(b, b, b, b)` with `b` the frame's star brightness and skipped entirely
//!   when `b <= 0` (`:1325-1345`). The blend is the same additive pair.
//!
//! # The celestial rotation
//!
//! The sun, the moon and the stars share one rotation, applied to their local positions before
//! the view: `rotate(-90, Y)` then `rotate(celestialAngle * 360, X)` (`:1296-1300`). The two
//! `glRotate` calls post-multiply, so a vertex is transformed by `Ry(-90) * Rx(angle * 360)`,
//! which [`celestial_rotation`] builds in glam's own order. The result is the source's: at noon
//! the sun is overhead, a quarter turn later it is on the western horizon `(-100, 0, 0)`.
//!
//! # The clouds
//!
//! [`CloudPass`] draws the source's fast cloud arm (`RenderGlobal.renderClouds`,
//! `RenderGlobal.java:1420-1481`): one flat layer of 32-block cells spanning `-256..256` around
//! the camera's own x and z (`:1432`, `:1467-1474`) at `y = 128 - eyeY + 0.33` in the camera's
//! frame (`:1462`; `WorldProvider.getCloudHeight` is 128.0 for the Overworld, `:206-209`),
//! textured by `environment/clouds` at `1/2048` of a uv per block (`:1454`), tinted by the
//! frame's cloud colour at alpha 0.8 (`:1471-1474`) and blended
//! `SRC_ALPHA`/`ONE_MINUS_SRC_ALPHA` (`:1438`). Culling is off for the layer (`:1430`), the depth
//! test is on and — because `renderSky` restores `depthMask(true)` before this (`:1416`) and the
//! fast arm masks nothing — the depth writes are on too. The fog is the terrain range
//! (`EntityRenderer.setupFog(0)`, `EntityRenderer.java:1361`, `:1500`).
//!
//! The uv offset is the source's own chain (`:1454-1464`): the camera's interpolated x plus
//! `(cloudTickCounter + partialTicks) * 0.03` blocks of drift, wrapped on `±2048` on each axis
//! and scaled by `4.8828125E-4` per block. So one counter tick advances the drift by 0.03 blocks
//! and the uv by `1.46484375E-5`, one block of drift by `4.8828125E-4`. **The counter is
//! client-local**: `RenderGlobal.cloudTickCounter` starts at zero with the render global, nothing
//! in the level data seeds it, and `updateClouds` increments it once per client tick
//! (`RenderGlobal.java:1138-1142`, called from `Minecraft.runTick`, `Minecraft.java:2193-2196`).
//! M2 has no client tick loop (M3's), so the client advances its counter once per rendered frame
//! as a stand-in; two clients therefore show independent cloud phases, and the acceptance note
//! records that masking rule.
//!
//! The layer draws twice, and the entity eye's height — the reported feet position plus
//! `eyeHeight`, never an interpolated position and never the displaced first-person camera —
//! picks the arm. The under-arm draws it before the terrain while that height is `< 128.0`
//! (`EntityRenderer.java:1364-1367`, [`cloud_under_layer`]); the at-or-above arm draws the same
//! layer after the translucent layer once it is `>= 128.0` (`:1474-1478`,
//! [`cloud_at_or_above_layer`]). Both call the same `renderCloudsCheck`, which installs the
//! layer's own `* 4` projection (`:1497`) and the terrain fog range (`setupFog(0)`, `:1500`)
//! around the draw and disables the fog after it (`:1502`); the `disableFog()` the at-or-above
//! call site runs first (`:1472`) is undone by that `setupFog(0)`, so both arms draw the deck
//! fogged the same way.
//!
//! # Projections, the eye and the frame
//!
//! The source gives every pass its own far plane: the sky's projection ends at
//! `farPlaneDistance * 2` (`EntityRenderer.java:1352`), the terrain's at `* sqrt(2)` (`:1357`)
//! and the clouds' at `* 4` (`:1497`). The passes mirror that: the sky's view-projection ends at
//! `SkyParams.far_plane * 2` and the cloud's at `* 4`, with `SkyParams.far_plane` the source's
//! `farPlaneDistance` — the render distance in blocks, 128 for the default eight chunks — which
//! is also the fog's reference distance (the sky's range is `0..far_plane`, the terrain's and the
//! clouds' `setupFog(0)` range comes from [`crate::fog::linear_params`]).
//!
//! The modelview the source draws the sky with ends with
//! `GlStateManager.translate(0.0F, -f, 0.0F)` with `f = getEyeHeight()` (`EntityRenderer.java:738`),
//! so the local geometry — the band's `+16`, the sun's `+100`, the grids' `±384`, the box's `-1`
//! — is measured from the ground the entity stands on and the camera sits the eye height
//! (`EYE_HEIGHT` standing, `EYE_HEIGHT_SNEAK` while sneaking) above the frame's origin. The sky
//! pass builds its view with the eye at `(0, eye height, 0)` less the
//! first-person backward offset the source's camera transform carries
//! (`GlStateManager.translate(0.0F, 0.0F, -0.1F)`, `:720`, which the terrain's own view has too);
//! the below-plane lift and the void floor already carry the absolute eye.
//!
//! The sky's pipelines write no depth, matching `depthMask(false)` (`:1230`) and the restore at
//! `:1416`; the depth test is off as the brief specifies, which is what the source's test
//! degenerates to on the freshly cleared buffer the sky is the first draw of. The clouds and the
//! terrain that follow write and test depth as the source's state does.
//!
//! The three environment textures arrive through [`SkyTextures`] and are uploaded as their own
//! small `Rgba8Unorm` textures; the sampler wraps (`GL_REPEAT` is what `TextureUtil` installs
//! for the environment textures, which ask for neither blur nor clamp) and filters nearest, as
//! vanilla's fixed pipeline does for them.

use glam::{Mat4, Vec3};

use oxide_assets::texture::Texture;

use crate::camera::{Camera, FIRST_PERSON_OFFSET};
use crate::fog::FogParams;
use crate::terrain_pass::DEPTH_FORMAT;

/// One cell of the sky's grids, in blocks (`RenderGlobal.java:342`).
pub const BAND_CELL: f32 = 64.0;

/// How far the sky's grids reach on each axis (`RenderGlobal.java:346-348`).
pub const BAND_EXTENT: f32 = 384.0;

/// The horizon band's height, camera-relative (`RenderGlobal.java:325`).
pub const BAND_HEIGHT: f32 = 16.0;

/// The height the below-horizon grid is generated at, camera-relative
/// (`RenderGlobal.java:291`); it is lifted to the world horizon when drawn
/// `RenderGlobal.java`:1412.
pub const BELOW_HEIGHT: f32 = -16.0;

/// The Overworld's horizon (`World.getHorizon`, `World.java:3702-3705`).
pub const HORIZON: f32 = 63.0;

/// The void box's half width (`RenderGlobal.java:1379`).
pub const VOID_BOX_HALF: f32 = 1.0;

/// The void box's lid, one unit under the eye (`RenderGlobal.java:1381`).
pub const VOID_BOX_LID: f32 = -1.0;

/// The constant the void box's floor drops by, on top of the eye's own drop below the horizon
/// (`RenderGlobal.java:1376`).
pub const VOID_BOX_DROP: f32 = 65.0;

/// The sun quad's half size (`RenderGlobal.java:1301`).
pub const SUN_HALF_SIZE: f32 = 30.0;

/// The sun quad's height, camera-relative (`RenderGlobal.java:1304-1307`).
pub const SUN_HEIGHT: f32 = 100.0;

/// The moon quad's half size (`RenderGlobal.java:1309`).
pub const MOON_HALF_SIZE: f32 = 20.0;

/// The moon quad's height, camera-relative (`RenderGlobal.java:1319-1322`).
pub const MOON_HEIGHT: f32 = -100.0;

/// The moon phase sheet's width in texels (`textures/environment/moon_phases.png`).
pub const MOON_SHEET_WIDTH: u32 = 128;

/// The moon phase sheet's height in texels.
pub const MOON_SHEET_HEIGHT: u32 = 64;

/// The phase sheet's cell columns: four phases across (`RenderGlobal.java:1313`).
pub const MOON_SHEET_COLUMNS: u32 = 4;

/// The phase sheet's cell rows: two rows of phases (`RenderGlobal.java:1314`).
pub const MOON_SHEET_ROWS: u32 = 2;

/// The star field's seed, `new Random(10842L)` (`RenderGlobal.java:405`).
pub const STAR_SEED: i64 = 10842;

/// The star loop's iteration count (`RenderGlobal.java:408`).
pub const STAR_COUNT: usize = 1500;

/// The radius the accepted stars' unit positions are scaled to (`RenderGlobal.java:420-424`).
pub const STAR_RADIUS: f64 = 100.0;

/// One cloud cell, in blocks (`RenderGlobal.java:1432`).
pub const CLOUD_CELL: f32 = 32.0;

/// How far the cloud layer reaches around the camera on each axis (`RenderGlobal.java:1467-1469`).
pub const CLOUD_EXTENT: f32 = 256.0;

/// The Overworld's cloud height (`WorldProvider.getCloudHeight`, `WorldProvider.java:206-209`).
pub const CLOUD_HEIGHT: f32 = 128.0;

/// The lift the cloud quad's height takes (`RenderGlobal.java:1462`).
pub const CLOUD_LIFT: f32 = 0.33;

/// The cloud quad's alpha (`RenderGlobal.java:1471-1474`).
pub const CLOUD_ALPHA: f32 = 0.8;

/// The block count the cloud uv wraps on, per axis (`RenderGlobal.java:1457-1458`).
pub const CLOUD_WRAP: f64 = 2048.0;

/// The uv one block of the cloud texture spans (`RenderGlobal.java:1454`).
pub const CLOUD_UV_PER_BLOCK: f64 = 4.8828125E-4;

/// The blocks one cloud counter tick drifts: the source's own `0.03` float literal, widened to
/// `f64` (`RenderGlobal.java:1456`), which is `0.029999999329447746` and not the round decimal.
/// One tick is therefore `0.03 * 4.8828125E-4 = 1.46484375E-5` of uv, not one uv per tick.
pub const CLOUD_DRIFT_PER_TICK: f64 = 0.029999999329447746;

/// The lift that puts the below-horizon grid at the world horizon: the source's
/// `translate(0, 12, 0)` over the generated `-16` `RenderGlobal.java`:1358, `RenderGlobal.java`:291.
const BELOW_LIFT: f32 = 12.0;

/// The sky projection's far plane as a multiple of the frame's `far_plane`
/// (`EntityRenderer.java:1352`).
const SKY_FAR_MULTIPLIER: f32 = 2.0;

/// The cloud projection's far plane as a multiple of the frame's `far_plane`
/// (`EntityRenderer.java:1497`).
const CLOUD_FAR_MULTIPLIER: f32 = 4.0;

/// The origin of every cell of the sky's grid, from `-BAND_EXTENT..=BAND_EXTENT` by `BAND_CELL`.
pub fn band_origins() -> Vec<f32> {
    (-(BAND_EXTENT as i32)..=(BAND_EXTENT as i32))
        .step_by(BAND_CELL as usize)
        .map(|origin| origin as f32)
        .collect()
}

/// The origin of every cell of the cloud layer, from `-CLOUD_EXTENT..CLOUD_EXTENT` by
/// `CLOUD_CELL`.
pub fn cloud_origins() -> Vec<f32> {
    (-(CLOUD_EXTENT as i32)..(CLOUD_EXTENT as i32))
        .step_by(CLOUD_CELL as usize)
        .map(|origin| origin as f32)
        .collect()
}

/// The celestial rotation the sun, the moon and the stars share: `rotate(-90, Y)` then
/// `rotate(celestialAngle * 360, X)` (`RenderGlobal.java:1296-1300`), the two calls' product in
/// the order the source's `glRotate` composes them.
pub fn celestial_rotation(celestial_angle: f32) -> Mat4 {
    Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2)
        * Mat4::from_rotation_x((celestial_angle * 360.0).to_radians())
}

/// The moon phase's cell uvs, in the order the source's quad carries its corners
/// (`RenderGlobal.java:1312-1322`).
///
/// The phase indexes a 4 by 2 sheet: the column is `phase % 4` and the row `phase / 4 % 2`, with
/// `u` from `k / 4` to `(k + 1) / 4` and `v` from `i1 / 2` to `(i1 + 1) / 2`. A negative phase
/// wraps as the source's own integer division does.
pub fn moon_uv(phase: i32) -> [[f32; 2]; 4] {
    let k = phase % MOON_SHEET_COLUMNS as i32;
    let row = phase / MOON_SHEET_COLUMNS as i32 % MOON_SHEET_ROWS as i32;
    let left = k as f32 / MOON_SHEET_COLUMNS as f32;
    let right = (k + 1) as f32 / MOON_SHEET_COLUMNS as f32;
    let top = row as f32 / MOON_SHEET_ROWS as f32;
    let bottom = (row + 1) as f32 / MOON_SHEET_ROWS as f32;
    [[right, bottom], [left, bottom], [left, top], [right, top]]
}

/// The drift the cloud counter and the partial tick add to the camera's x, in blocks
/// (`RenderGlobal.java:1455`): `((float) cloudTickCounter + partialTicks) * 0.03`.
pub fn cloud_drift(ticks: i64, partial_ticks: f32) -> f64 {
    f64::from(ticks as f32 + partial_ticks) * CLOUD_DRIFT_PER_TICK
}

/// The wrapped, scaled u coordinate of the cloud layer for a view x, a counter and a partial tick
/// (`RenderGlobal.java:1455-1459`): the drift is added to the view's x, the sum wrapped into
/// `0..2048`, and the wrapped blocks scaled by `1/2048`.
pub fn cloud_uv_x(view_x: f64, ticks: i64, partial_ticks: f32) -> f32 {
    wrap_uv(view_x + cloud_drift(ticks, partial_ticks))
}

/// The wrapped, scaled v coordinate of the cloud layer for a view z (`RenderGlobal.java:1456-1460`);
/// the v axis takes no drift.
pub fn cloud_uv_z(view_z: f64) -> f32 {
    wrap_uv(view_z)
}

/// One axis of the cloud uv chain: wrapped into `0..2048` by the source's floor division and
/// scaled by `4.8828125E-4` (`RenderGlobal.java:1457-1460`).
fn wrap_uv(blocks: f64) -> f32 {
    let wraps = (blocks / CLOUD_WRAP).floor();
    ((blocks - wraps * CLOUD_WRAP) * CLOUD_UV_PER_BLOCK) as f32
}

/// The cloud layer's height in the camera's own frame, from the interpolated eye y
/// (`RenderGlobal.java:1462`): `cloudHeight - eyeY + 0.33`.
pub fn cloud_layer_y(eye_y: f32) -> f32 {
    CLOUD_HEIGHT - eye_y + CLOUD_LIFT
}

/// The entity eye's height above the world origin: the basis both of the source's cloud gates
/// read (`entity.posY + (double) entity.getEyeHeight()`, `EntityRenderer.java:1364`, `:1474`).
///
/// The basis is the entity's own eye — the reported feet position plus the pose's eye
/// height (`CameraPose::eye_height`: 1.62 standing, 1.54 while sneaking, the game side's
/// own two-value rule). The source's guards interpolate nothing and read the entity, not
/// the first-person camera the view pulls a tenth of a block back along the view axis.
fn entity_eye_y(camera: &Camera) -> f64 {
    camera.pose.position[1] + f64::from(camera.pose.eye_height())
}

/// Whether the camera's eye is under the cloud layer, the source's gate for the first draw
/// (`EntityRenderer.java:1364`, `entity.posY + eyeHeight < 128.0`).
pub fn cloud_under_layer(camera: &Camera) -> bool {
    entity_eye_y(camera) < f64::from(CLOUD_HEIGHT)
}

/// Whether the camera's eye stands at or above the cloud layer, the source's gate for the
/// second draw (`EntityRenderer.java:1474`, `entity.posY + eyeHeight >= 128.0`).
pub fn cloud_at_or_above_layer(camera: &Camera) -> bool {
    entity_eye_y(camera) >= f64::from(CLOUD_HEIGHT)
}

/// The void box's floor for an eye height: `-(d0 + 65)` with `d0 = eyeY - horizon`
/// (`RenderGlobal.java:1376`).
pub fn void_box_low(eye_y: f32) -> f32 {
    -(eye_y - HORIZON + VOID_BOX_DROP)
}

/// One star of the source's field: its centre on the sphere of [`STAR_RADIUS`], the size the
/// quad is built from, and the four corners of that quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    /// The star's centre, in the camera-relative celestial frame.
    pub centre: [f64; 3],
    /// The star's extent: its half-extent along each of the quad's two basis directions
    /// (`0.15F + nextFloat() * 0.1F`, `RenderGlobal.java:411`), so a corner sits
    /// `size * sqrt(2)` from the centre.
    pub size: f64,
    /// The quad's four corners, in the order the source winds them
    /// (`RenderGlobal.java:429-448`).
    pub corners: [[f32; 3]; 4],
}

/// Generates the star field exactly as `RenderGlobal.renderStars` does
/// (`RenderGlobal.java:367-451`): 1500 iterations of three `nextFloat() * 2 - 1` coordinates and
/// one size, the draws whose squared radius sits strictly inside `0.01..1` kept, normalised and
/// scaled to the sphere of [`STAR_RADIUS`].
///
/// The stream is the same `java.util.Random(10842L)` Task 6 ported
/// (`oxide_world::noise::JavaRandom`); this crate cannot reach that one — the crate graph gives
/// `oxide-render` no edge to `oxide-world` — so the little generator below repeats it, and the
/// pinned star literals are what keeps the two from drifting.
pub fn star_field() -> Vec<Star> {
    let mut random = FloatRandom::new(STAR_SEED);
    let mut stars = Vec::new();
    for _ in 0..STAR_COUNT {
        let d0 = f64::from(random.next_float() * 2.0 - 1.0);
        let d1 = f64::from(random.next_float() * 2.0 - 1.0);
        let d2 = f64::from(random.next_float() * 2.0 - 1.0);
        let size = f64::from(0.15f32 + random.next_float() * 0.1f32);
        let squared = d0 * d0 + d1 * d1 + d2 * d2;
        if !(0.01 < squared && squared < 1.0) {
            continue;
        }
        let scale = 1.0 / squared.sqrt();
        let (d0, d1, d2) = (d0 * scale, d1 * scale, d2 * scale);
        let centre = [d0 * STAR_RADIUS, d1 * STAR_RADIUS, d2 * STAR_RADIUS];

        // The quad's own basis, as the source derives it from the star's polar angles
        // (`RenderGlobal.java:412-418`).
        let azimuth = d0.atan2(d2);
        let (sin_azimuth, cos_azimuth) = (azimuth.sin(), azimuth.cos());
        let polar = (d0 * d0 + d2 * d2).sqrt().atan2(d1);
        let (sin_polar, cos_polar) = (polar.sin(), polar.cos());
        let spin = random.next_double() * std::f64::consts::PI * 2.0;
        let (sin_spin, cos_spin) = (spin.sin(), spin.cos());

        let mut corners = [[0.0f32; 3]; 4];
        for (index, corner) in corners.iter_mut().enumerate() {
            let j = index as i32;
            let across = f64::from((j & 2) - 1) * size;
            let down = f64::from(((j + 1) & 2) - 1) * size;
            let spun_across = across * cos_spin - down * sin_spin;
            let spun_down = down * cos_spin + across * sin_spin;
            let lifted = spun_across * sin_polar;
            let radial = -spun_across * cos_polar;
            *corner = [
                (centre[0] + radial * sin_azimuth - spun_down * cos_azimuth) as f32,
                (centre[1] + lifted) as f32,
                (centre[2] + spun_down * sin_azimuth + radial * cos_azimuth) as f32,
            ];
        }
        stars.push(Star {
            centre,
            size,
            corners,
        });
    }
    stars
}

/// The `java.util.Random` draw the star field needs: the 48-bit linear congruential generator
/// behind `new java.util.Random(10842L)`.
///
/// Task 6's port lives in `oxide_world::noise::JavaRandom`; this crate has no edge to
/// `oxide-world` (section 5.1's table, `scripts/check-graph.sh`), so the two constants and the
/// float draws are repeated here rather than reached across crates. The star literals
/// `refs/m2-task-12/star_field.java` printed pin both copies.
struct FloatRandom {
    /// The generator's state: the low 48 bits of the scrambled seed.
    seed: u64,
}

/// `java.util.Random`'s multiplier.
const RANDOM_MULTIPLIER: u64 = 0x5_DEEC_E66D;
/// `java.util.Random`'s addend.
const RANDOM_ADDEND: u64 = 0xB;
/// The generator's state width, 48 bits.
const RANDOM_MASK: u64 = (1 << 48) - 1;

impl FloatRandom {
    /// A generator seeded as `new java.util.Random(seed)`.
    fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ RANDOM_MULTIPLIER) & RANDOM_MASK,
        }
    }

    /// The next float in `0.0..1.0`: `java.util.Random.nextFloat`, the top 24 bits of a draw
    /// over `2^24`.
    fn next_float(&mut self) -> f32 {
        (self.next_bits(24) as f32) / (1u32 << 24) as f32
    }

    /// The next double in `0.0..1.0`: `java.util.Random.nextDouble`, a 26-bit and a 27-bit draw
    /// over `2^53`, which the star quad's spin uses.
    fn next_double(&mut self) -> f64 {
        let high = u64::from(self.next_bits(26)) << 27;
        let low = u64::from(self.next_bits(27));
        (high + low) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// The next `bits` bits of the stream.
    fn next_bits(&mut self, bits: u32) -> u32 {
        self.seed = self
            .seed
            .wrapping_mul(RANDOM_MULTIPLIER)
            .wrapping_add(RANDOM_ADDEND)
            & RANDOM_MASK;
        (self.seed >> (48u32.wrapping_sub(bits) & 63)) as u32
    }
}

/// One frame's sky, from the session's clock and the client's own frame parameters.
///
/// The session owns the world and the clock, so it computes the values a client without an
/// `oxide-world` edge cannot: the angle, the sky's colour at the view block, the brightnesses and
/// the clouds' tint. The client adds the fog colour it derives from the same clock
/// ([`crate::fog::fog_colour`]), the frame's `far_plane` and the cloud counter it advances.
///
/// `sun_brightness` is carried for the lightmap's later live clock (M6); the sky pass itself does
/// not read it, because the source's sky does not either — the sun quad's tint is a round one.
/// `cloud_offset_ticks` is the client-local counter [`cloud_uv_x`] consumes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyParams {
    /// The celestial angle in `0..1`, from the world time.
    pub celestial_angle: f32,
    /// The sky's colour at the view block.
    pub sky_colour: [f32; 3],
    /// The sun's brightness.
    pub sun_brightness: f32,
    /// The stars' brightness, the rain already folded in.
    pub star_brightness: f32,
    /// The frame's fog colour, the band fades towards it.
    pub fog_colour: [f32; 3],
    /// The frame's far plane: the source's `farPlaneDistance`, the render distance in blocks.
    pub far_plane: f32,
    /// The client's cloud counter; see the module comment for its rule.
    pub cloud_offset_ticks: i64,
    /// The clouds' tint.
    pub cloud_colour: [f32; 3],
    /// The moon's phase in `0..8`, from the clock's `getMoonPhase`; the pass draws that cell of
    /// the phase sheet. Any value outside `0..8` wraps.
    pub moon_phase: u8,
}

/// The three environment textures the sky and the clouds draw with, decoded by the caller's
/// `oxide_assets` store.
///
/// Task 14's bootstrap loads `environment/sun`, `environment/moon_phases` and
/// `environment/clouds` through the texture set and hands them here; the GPU tests build their
/// own synthetic stand-ins.
#[derive(Debug, Clone)]
pub struct SkyTextures {
    /// `textures/environment/sun.png`: the sun quad's texture.
    pub sun: Texture,
    /// `textures/environment/moon_phases.png`: the 128x64 phase sheet.
    pub moon_phases: Texture,
    /// `textures/environment/clouds.png`: the cloud layer's texture.
    pub clouds: Texture,
}

/// The vertex a sky buffer holds: a position, the kind that picks its colour path, a uv and a
/// colour.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SkyVertex {
    /// The local position; the kind's own shader rule maps or rotates it.
    position: [f32; 3],
    /// What the fragment stage draws for the vertex; see the `KIND_*` constants.
    kind: u32,
    /// The texture coordinate, for the textured kinds.
    uv: [f32; 2],
    /// The vertex colour, for the textured and star kinds.
    colour: [u8; 4],
}

/// The byte size of one [`SkyVertex`].
const SKY_VERTEX_BYTES: usize = 28;

/// The band: the horizon plane in the frame's sky colour.
const KIND_BAND: u32 = 0;
/// The below-horizon plane in the darkened sky colour.
const KIND_BELOW: u32 = 1;
/// The void box; its vertical coordinate is a sentinel the shader maps to lid and floor.
const KIND_VOID_BOX: u32 = 2;
/// The black grid the source draws four units under the eye while it is below the horizon.
const KIND_VOID_PLANE: u32 = 3;
/// A textured celestial quad: the sun or the moon.
const KIND_TEXTURED: u32 = 4;
/// A star: its colour is the frame's star brightness.
const KIND_STAR: u32 = 5;

/// The sky vertex attributes: a position at offset 0, the kind at 12, the uv at 16 and the
/// colour at 24.
static SKY_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Uint32,
    2 => Float32x2,
    3 => Unorm8x4
];

/// The cloud vertex attributes: one `Float32x3` position.
static CLOUD_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];

/// The sky's frame uniform: 16 floats of view-projection, 16 of celestial rotation, then the four
/// colours and the two parameter `vec4`s.
const SKY_UNIFORM_BYTES: usize = (16 + 16 + 4 + 4 + 4 + 4 + 4) * 4;

/// The cloud frame uniform: the view-projection, the colour, the origin and uv offset, the
/// fog's colour and range, and the eye.
const CLOUD_UNIFORM_BYTES: usize = (16 + 4 + 4 + 4 + 4 + 4) * 4;

/// One byte-packing helper: the floats little-endian, in order.
fn f32_bytes(values: &[f32], bytes: &mut [u8]) {
    for (index, value) in values.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
}

/// The sky shader source.
const SKY_SHADER: &str = r#"
struct Sky {
    view_projection: mat4x4<f32>,
    celestial: mat4x4<f32>,
    sky_colour: vec4<f32>,
    below_colour: vec4<f32>,
    fog_colour: vec4<f32>,
    // x: the fog's start, y: its end, z: the below plane's lift, w: the void box's floor.
    fog_range: vec4<f32>,
    // x: the stars' brightness, y/z/w: the eye's position in the frame's local coordinates, the
    // point the fog's radial distance is measured from (`GL_EYE_RADIAL_NV`).
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> sky: Sky;
@group(1) @binding(0) var sky_texture: texture_2d<f32>;
@group(1) @binding(1) var sky_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) kind: u32,
    @location(2) uv: vec2<f32>,
    @location(3) colour: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) distance: f32,
    @location(3) @interpolate(flat) kind: u32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    var local = input.position;
    // The void box's vertical coordinates are sentinels: 1 is the floor, 0 the lid.
    if (input.kind == 2u) {
        local.y = local.y * (sky.fog_range.w + 1.0) - 1.0;
    }
    // The below-horizon grid is generated at the source's -16 and lifted to the world horizon.
    if (input.kind == 1u) {
        local.y = local.y + sky.fog_range.z;
    }
    var position = vec4<f32>(local, 1.0);
    // The sun, the moon and the stars ride the celestial rotation; the planes do not.
    if (input.kind >= 4u) {
        position = sky.celestial * position;
    }
    output.clip_position = sky.view_projection * position;
    // The interpolated varying is the vertex's own distance from the eye in the frame's local
    // coordinates — the fog's radial measure, the straight line the source's `GL_EYE_RADIAL_NV`
    // mode uses.
    output.distance = length(local - sky.params.yzw);
    output.uv = input.uv;
    output.colour = input.colour;
    output.kind = input.kind;
    return output;
}

// The kind picks the colour the source's colour register carried for that draw.
fn base(input: VertexOutput) -> vec4<f32> {
    switch input.kind {
        case 0u: {
            return sky.sky_colour;
        }
        case 1u: {
            return sky.below_colour;
        }
        case 2u, 3u: {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
        case 4u: {
            return textureSample(sky_texture, sky_sampler, input.uv) * input.colour;
        }
        default: {
            return vec4<f32>(vec3<f32>(sky.params.x), sky.params.x);
        }
    }
}

// The linear fog every ground-facing sky surface mixes in, the same mix the terrain uses: the
// distance is the eye's radial one.
fn fogged(colour: vec4<f32>, distance: f32) -> vec4<f32> {
    let span = sky.fog_range.y - sky.fog_range.x;
    if (span <= 0.0) {
        return colour;
    }
    let factor = clamp((sky.fog_range.y - distance) / span, 0.0, 1.0);
    return vec4<f32>(mix(sky.fog_colour.rgb, colour.rgb, factor), colour.a);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let colour = base(input);
    // The band, the void and the below-horizon plane are fogged: the source enables fog for the
    // band (`RenderGlobal.java:1231`) and, after the celestial quads, for the under-horizon
    // geometry (`:1349`). The sun, the moon and the stars draw between those two with fog
    // disabled (`:1248`), so kinds 4 and 5 are left alone.
    if (input.kind <= 3u) {
        return fogged(colour, input.distance);
    }
    return colour;
}
"#;

/// The cloud shader source, with the layer's constants injected.
fn cloud_shader() -> String {
    format!(
        r#"
struct Cloud {{
    view_projection: mat4x4<f32>,
    colour: vec4<f32>,
    // x: the view x, y: the view z, z: the uv's u offset, w: its v offset.
    origin: vec4<f32>,
    fog_colour: vec4<f32>,
    // x: the fog's start, y: its end.
    fog_range: vec4<f32>,
    // x, y, z: the eye's world position, the point the fog's radial distance is measured from;
    // the fourth component is unused.
    eye: vec4<f32>,
}};

@group(0) @binding(0) var<uniform> cloud: Cloud;
@group(1) @binding(0) var cloud_texture: texture_2d<f32>;
@group(1) @binding(1) var cloud_sampler: sampler;

// The layer's world height (`RenderGlobal.java:1462`, `WorldProvider.java:206-209`).
const CLOUD_Y: f32 = {CLOUD_Y};
// The uv one block spans (`RenderGlobal.java:1454`).
const CLOUD_UV_PER_BLOCK: f32 = {CLOUD_UV_PER_BLOCK};
// The quad's alpha (`RenderGlobal.java:1471-1474`).
const CLOUD_ALPHA: f32 = {CLOUD_ALPHA};

struct VertexInput {{
    @location(0) position: vec3<f32>,
}};

struct VertexOutput {{
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) distance: f32,
}};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {{
    var output: VertexOutput;
    // The vertex carries its local cell coordinate in x and z; the layer follows the camera.
    let world = vec3<f32>(
        input.position.x + cloud.origin.x,
        CLOUD_Y,
        input.position.z + cloud.origin.y,
    );
    output.clip_position = cloud.view_projection * vec4<f32>(world, 1.0);
    // The fog's radial measure: the straight line from the eye to the vertex.
    output.distance = length(world - cloud.eye.xyz);
    output.uv = vec2<f32>(input.position.x, input.position.z) * CLOUD_UV_PER_BLOCK
        + cloud.origin.zw;
    return output;
}}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(cloud_texture, cloud_sampler, input.uv);
    let colour = texel * vec4<f32>(cloud.colour.rgb, CLOUD_ALPHA);
    let span = cloud.fog_range.y - cloud.fog_range.x;
    if (span <= 0.0) {{
        return colour;
    }}
    let factor = clamp((cloud.fog_range.y - input.distance) / span, 0.0, 1.0);
    return vec4<f32>(mix(cloud.fog_colour.rgb, colour.rgb, factor), colour.a);
}}
"#,
        CLOUD_Y = CLOUD_HEIGHT + CLOUD_LIFT,
        CLOUD_UV_PER_BLOCK = CLOUD_UV_PER_BLOCK,
        CLOUD_ALPHA = CLOUD_ALPHA,
    )
}

/// The additive blend the sun, the moon and the stars are drawn with
/// (`GlStateManager.tryBlendFuncSeparate(770, 1, 1, 0)`, `RenderGlobal.java:1295`): the source's
/// colour over the frame's, the alpha pair left alone.
fn celestial_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The cloud layer's blend (`GlStateManager.tryBlendFuncSeparate(770, 771, 1, 0)`,
/// `RenderGlobal.java:1438`): the source over one minus it, the destination's alpha kept.
fn cloud_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The depth state the sky's pipelines carry: no writes (`GlStateManager.depthMask(false)`,
/// `RenderGlobal.java:1230`) and no test — the sky is the frame's first draw over a freshly
/// cleared buffer, so the source's test lets everything through; `Always` says that outright.
fn sky_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: false,
        depth_compare: wgpu::CompareFunction::Always,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The cloud layer's depth state: the test on and the writes on, as the source's restored
/// `depthMask(true)` and unmasked fast arm leave them (`RenderGlobal.java:1416`, `:1430-1481`).
fn cloud_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::Less,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The colour target for the sky and cloud pipelines: the frame's format, every channel written.
fn color_target(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    }
}

/// The sky's vertex buffer layout, tied to [`SKY_VERTEX_BYTES`] by the unit tests.
fn sky_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: SKY_VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &SKY_ATTRIBUTES,
    }
}

/// The cloud vertex layout: the local cell coordinate.
fn cloud_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &CLOUD_ATTRIBUTES,
    }
}

/// The primitive state: triangles wound counter-clockwise seen from outside, culling the back
/// faces the source's enabled cull would drop, or nothing for the cloud layer's `disableCull()`.
fn primitive_state(cull: Option<wgpu::Face>) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: cull,
        ..Default::default()
    }
}

/// The texture-plus-sampler bind group layout every sky and cloud pipeline reads through.
fn texture_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// A GPU texture with the sampler its pass reads it through.
struct GpuTexture {
    /// A view of the whole texture.
    view: wgpu::TextureView,
    /// The sampler the pass reads it through.
    sampler: wgpu::Sampler,
}

impl GpuTexture {
    /// Uploads `texture` as an `Rgba8Unorm` image with the environment sampler.
    ///
    /// A texture whose byte count does not match its declared size — only a hand-built one can —
    /// is refused with a warning and a one-texel stand-in takes its place, so a malformed
    /// hand-off can neither panic the uploader nor stop the frame.
    fn upload(device: &wgpu::Device, queue: &wgpu::Queue, texture: &Texture, label: &str) -> Self {
        let width = texture.width.max(1);
        let height = texture.height.max(1);
        if texture.rgba.len() != (width * height * 4) as usize {
            tracing::warn!(
                label,
                width = texture.width,
                height = texture.height,
                bytes = texture.rgba.len(),
                "the texture does not match its declared size; a one-texel stand-in is used"
            );
            return Self::upload_placeholder(device, queue);
        }
        let gpu = device.create_texture(&upload_descriptor(width, height));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = gpu.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&env_sampler_descriptor());
        Self { view, sampler }
    }

    /// A one-texel opaque white texture, used both as the pipelines' stand-in before
    /// [`SkyPass::set_textures`] lands and for a malformed hand-off.
    fn upload_placeholder(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self::upload_raw(device, queue, &[255, 255, 255, 255], 1, 1)
    }

    /// Uploads raw RGBA bytes with a declared size.
    fn upload_raw(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Self {
        let gpu = device.create_texture(&upload_descriptor(width, height));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = gpu.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&env_sampler_descriptor());
        Self { view, sampler }
    }

    /// Builds the bind group for this texture under `layout`.
    fn bind_group(&self, device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide sky texture bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// The texture descriptor for one of the environment textures.
fn upload_descriptor(width: u32, height: u32) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("oxide sky environment texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}

/// The environment sampler: nearest filtering and wrapping on every axis, the state vanilla's
/// texture loader installs for the environment textures, which ask for neither blur nor clamp.
fn env_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide sky environment sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    }
}

/// The four corners of one grid cell, wound as the source emits them; `reverse` flips x first,
/// which is how `renderSky` builds its second list (`RenderGlobal.java:350-360`).
fn grid_cell(origin_x: f32, origin_z: f32, reverse: bool) -> [[f32; 2]; 4] {
    let (x0, x1) = if reverse {
        (origin_x + BAND_CELL, origin_x)
    } else {
        (origin_x, origin_x + BAND_CELL)
    };
    let (z0, z1) = (origin_z, origin_z + BAND_CELL);
    [[x0, z0], [x1, z0], [x1, z1], [x0, z1]]
}

/// Appends one quad's four corners as the two triangles the source's `GL_QUADS` draw expands
/// to, with the source's winding: `0-1-2` then `0-2-3`.
fn push_quad(vertices: &mut Vec<SkyVertex>, kind: u32, corners: [[f32; 2]; 4], y: f32) {
    let corner = |[x, z]: [f32; 2]| SkyVertex {
        position: [x, y, z],
        kind,
        uv: [0.0, 0.0],
        colour: [255, 255, 255, 255],
    };
    let [a, b, c, d] = corners;
    for vertex in [
        corner(a),
        corner(b),
        corner(c),
        corner(a),
        corner(c),
        corner(d),
    ] {
        vertices.push(vertex);
    }
}

/// Builds one of the sky's grids at `y`, with the winding the source generates it with.
fn grid_vertices(kind: u32, y: f32, reverse: bool) -> Vec<SkyVertex> {
    let mut vertices = Vec::new();
    for origin_z in band_origins() {
        for origin_x in band_origins() {
            push_quad(
                &mut vertices,
                kind,
                grid_cell(origin_x, origin_z, reverse),
                y,
            );
        }
    }
    vertices
}

/// Builds the void box's five quads: the four sides from the lid sentinel `0` to the floor
/// sentinel `1`, and the lid itself, wound as the source winds them
/// (`RenderGlobal.java:1383-1399`).
fn void_box_vertices() -> Vec<SkyVertex> {
    const HALF: f32 = VOID_BOX_HALF;
    /// One corner: x, the vertical sentinel and z.
    type Corner = [f32; 3];
    let quads: [[Corner; 4]; 5] = [
        // The +z face.
        [
            [-HALF, 1.0, HALF],
            [HALF, 1.0, HALF],
            [HALF, 0.0, HALF],
            [-HALF, 0.0, HALF],
        ],
        // The -z face.
        [
            [-HALF, 0.0, -HALF],
            [HALF, 0.0, -HALF],
            [HALF, 1.0, -HALF],
            [-HALF, 1.0, -HALF],
        ],
        // The +x face.
        [
            [HALF, 0.0, -HALF],
            [HALF, 0.0, HALF],
            [HALF, 1.0, HALF],
            [HALF, 1.0, -HALF],
        ],
        // The -x face.
        [
            [-HALF, 1.0, -HALF],
            [-HALF, 1.0, HALF],
            [-HALF, 0.0, HALF],
            [-HALF, 0.0, -HALF],
        ],
        // The lid, seen from the eye above it.
        [
            [-HALF, 0.0, -HALF],
            [-HALF, 0.0, HALF],
            [HALF, 0.0, HALF],
            [HALF, 0.0, -HALF],
        ],
    ];
    let mut vertices = Vec::new();
    for quad in quads {
        let corner = |[x, y, z]: Corner| SkyVertex {
            position: [x, y, z],
            kind: KIND_VOID_BOX,
            uv: [0.0, 0.0],
            colour: [255, 255, 255, 255],
        };
        let [a, b, c, d] = quad;
        for vertex in [
            corner(a),
            corner(b),
            corner(c),
            corner(a),
            corner(c),
            corner(d),
        ] {
            vertices.push(vertex);
        }
    }
    vertices
}

/// The sun quad at its source's height and size (`RenderGlobal.java:1301-1308`), textured `0..1`.
fn sun_quad_vertices() -> Vec<SkyVertex> {
    let positions = [
        [-SUN_HALF_SIZE, SUN_HEIGHT, -SUN_HALF_SIZE],
        [SUN_HALF_SIZE, SUN_HEIGHT, -SUN_HALF_SIZE],
        [SUN_HALF_SIZE, SUN_HEIGHT, SUN_HALF_SIZE],
        [-SUN_HALF_SIZE, SUN_HEIGHT, SUN_HALF_SIZE],
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    quad_vertices(positions, uvs)
}

/// The moon quad at its source's height and size (`RenderGlobal.java:1309-1323`), textured by
/// [`moon_uv`] for `phase`.
///
/// [`SkyPass::new`] builds one quad per phase cell, so the pass draws the clock's phase without
/// rebuilding a buffer.
fn moon_quad_vertices(phase: i32) -> Vec<SkyVertex> {
    let positions = [
        [-MOON_HALF_SIZE, MOON_HEIGHT, MOON_HALF_SIZE],
        [MOON_HALF_SIZE, MOON_HEIGHT, MOON_HALF_SIZE],
        [MOON_HALF_SIZE, MOON_HEIGHT, -MOON_HALF_SIZE],
        [-MOON_HALF_SIZE, MOON_HEIGHT, -MOON_HALF_SIZE],
    ];
    quad_vertices(positions, moon_uv(phase))
}

/// One textured quad from four positions and four uvs, as the source winds it.
fn quad_vertices(positions: [[f32; 3]; 4], uvs: [[f32; 2]; 4]) -> Vec<SkyVertex> {
    let corner = |(position, uv): ([f32; 3], [f32; 2])| SkyVertex {
        position,
        kind: KIND_TEXTURED,
        uv,
        colour: [255, 255, 255, 255],
    };
    let [a, b, c, d] = std::array::from_fn(|index| corner((positions[index], uvs[index])));
    vec![a, b, c, a, c, d]
}

/// The star field as one position-only vertex list, each star a quad from [`star_field`].
fn star_vertices() -> Vec<SkyVertex> {
    let mut vertices = Vec::new();
    for star in star_field() {
        let corner = |position: [f32; 3]| SkyVertex {
            position,
            kind: KIND_STAR,
            uv: [0.0, 0.0],
            colour: [255, 255, 255, 255],
        };
        let [a, b, c, d] = star.corners.map(corner);
        for vertex in [a, b, c, a, c, d] {
            vertices.push(vertex);
        }
    }
    vertices
}

/// The raw bytes of one [`SkyVertex`] list.
fn sky_vertex_bytes(vertices: &[SkyVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * SKY_VERTEX_BYTES);
    for vertex in vertices {
        for value in vertex.position {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&vertex.kind.to_le_bytes());
        for value in vertex.uv {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&vertex.colour);
    }
    bytes
}

/// Creates one vertex buffer from a vertex list and returns it with its vertex count.
fn vertex_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    vertices: &[SkyVertex],
) -> (wgpu::Buffer, u32) {
    let bytes = sky_vertex_bytes(vertices);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &bytes);
    (buffer, vertices.len() as u32)
}

/// The eye the sky's view is built from: the local frame's origin plus the pose's live eye
/// height, less `crate::camera::FIRST_PERSON_OFFSET` along the view axis.
fn sky_eye(camera: &Camera) -> Vec3 {
    Vec3::new(0.0, camera.pose.eye_height(), 0.0) - FIRST_PERSON_OFFSET * camera.forward()
}

/// The sky pass: the pipelines, the frame uniform, the environment textures and the geometry.
pub struct SkyPass {
    /// The band and the below-horizon plane, in the source's draw order.
    background_pipeline: wgpu::RenderPipeline,
    /// The sun, the moon and the stars, additive.
    celestial_pipeline: wgpu::RenderPipeline,
    /// The frame uniform buffer.
    frame_buffer: wgpu::Buffer,
    /// The bind group for the frame uniform.
    frame_bind_group: wgpu::BindGroup,
    /// The layout group 1 binds the environment textures through.
    texture_layout: wgpu::BindGroupLayout,
    /// A one-texel stand-in's bind group, so the pipelines have a binding before
    /// [`SkyPass::set_textures`] lands; the band and the stars never sample it. The bind group
    /// keeps the texture and its view alive.
    fallback_bind_group: wgpu::BindGroup,
    /// `environment/sun`, once uploaded, and its bind group.
    sun: Option<(GpuTexture, wgpu::BindGroup)>,
    /// `environment/moon_phases`, once uploaded, and its bind group.
    moon: Option<(GpuTexture, wgpu::BindGroup)>,
    /// The horizon band and the below-horizon plane, band first.
    background: wgpu::Buffer,
    /// How many vertices the band holds; the below-horizon plane follows it.
    band_vertices: u32,
    /// How many vertices the background holds.
    background_vertices: u32,
    /// The void box and the black plane drawn while the eye is below the horizon.
    void: wgpu::Buffer,
    /// How many vertices the void holds.
    void_vertices: u32,
    /// The sun quad.
    sun_quad: wgpu::Buffer,
    /// The moon's eight phase quads, indexed by the phase.
    moon_quads: [wgpu::Buffer; 8],
    /// The star field.
    stars: wgpu::Buffer,
    /// How many vertices the star field holds.
    star_vertices: u32,
    /// The frame's parameters, once the client has set them.
    params: Option<SkyParams>,
    /// The camera the last frame was built with.
    camera: Option<Camera>,
    /// The surface aspect ratio the last camera was given.
    aspect: f32,
    /// Whether the eye sits below the horizon, the gate on the void's draw.
    below_horizon: bool,
}

impl SkyPass {
    /// Builds the sky pipelines, the frame uniform and the geometry for colour attachments in
    /// `format`.
    ///
    /// The geometry is generated once, from the same loops and figures the source's
    /// `generateSky`, `generateSky2` and `generateStars` run (`RenderGlobal.java:279-358`,
    /// `:367-451`).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide sky shader"),
            source: wgpu::ShaderSource::Wgsl(SKY_SHADER.into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide sky frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(SKY_UNIFORM_BYTES as u64),
                },
                count: None,
            }],
        });
        let texture_layout = texture_layout(device, "oxide sky texture layout");
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide sky frame"),
            size: SKY_UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&frame_buffer, 0, &[0u8; SKY_UNIFORM_BYTES]);
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide sky frame bind group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide sky pipeline layout"),
            bind_group_layouts: &[&frame_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = |label: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[sky_vertex_layout()],
                },
                primitive: primitive_state(Some(wgpu::Face::Back)),
                depth_stencil: Some(sky_depth_state()),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(color_target(format, blend))],
                }),
                multiview: None,
                cache: None,
            })
        };
        let background_pipeline = pipeline("oxide sky background pipeline", None);
        let celestial_pipeline = pipeline("oxide sky celestial pipeline", Some(celestial_blend()));

        let fallback = GpuTexture::upload_placeholder(device, queue);
        let fallback_bind_group = fallback.bind_group(device, &texture_layout);

        let background = {
            let band = grid_vertices(KIND_BAND, BAND_HEIGHT, false);
            let band_vertices = band.len() as u32;
            let mut vertices = band;
            vertices.extend(grid_vertices(KIND_BELOW, BELOW_HEIGHT, true));
            let (buffer, total) = vertex_buffer(device, queue, "oxide sky background", &vertices);
            (buffer, band_vertices, total)
        };
        let void = {
            let mut vertices = grid_vertices(KIND_VOID_PLANE, BELOW_HEIGHT + BELOW_LIFT, true);
            vertices.extend(void_box_vertices());
            vertex_buffer(device, queue, "oxide sky void", &vertices)
        };
        let sun = vertex_buffer(device, queue, "oxide sky sun", &sun_quad_vertices());
        // One quad per phase cell, so the pass picks the clock's phase without rebuilding.
        let moon_quads = std::array::from_fn(|phase| {
            vertex_buffer(
                device,
                queue,
                "oxide sky moon",
                &moon_quad_vertices(phase as i32),
            )
            .0
        });
        let stars = vertex_buffer(device, queue, "oxide sky stars", &star_vertices());

        Self {
            background_pipeline,
            celestial_pipeline,
            frame_buffer,
            frame_bind_group,
            texture_layout,
            fallback_bind_group,
            sun: None,
            moon: None,
            background: background.0,
            band_vertices: background.1,
            background_vertices: background.2,
            void: void.0,
            void_vertices: void.1,
            sun_quad: sun.0,
            moon_quads,
            stars: stars.0,
            star_vertices: stars.1,
            params: None,
            camera: None,
            aspect: 1.0,
            below_horizon: false,
        }
    }

    /// Stores the frame's sky parameters and refreshes the uniform.
    pub fn set_params(&mut self, queue: &wgpu::Queue, params: SkyParams) {
        self.params = Some(params);
        self.refresh(queue);
    }

    /// Stores the camera and the surface aspect the next draw is built with, and refreshes the
    /// uniform.
    ///
    /// The view is the camera's pose with the eye at `(0, eye height, 0)` less
    /// [`FIRST_PERSON_OFFSET`] along the view axis: the sky's geometry is measured from the
    /// ground the entity stands on, as the source's own modelview makes it
    /// (`GlStateManager.translate(0.0F, -f, 0.0F)`, `EntityRenderer.java:738`), and every
    /// normally-played pass sits the offset's own distance behind the eye
    /// (`GlStateManager.translate(0.0F, 0.0F, -0.1F)`, `:720`).
    pub fn set_camera(&mut self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        self.camera = Some(camera);
        self.aspect = aspect;
        self.refresh(queue);
    }

    /// Uploads the sun and the moon phase sheet, replacing any earlier pair.
    pub fn set_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sun: &Texture,
        moon_phases: &Texture,
    ) {
        let sun_gpu = GpuTexture::upload(device, queue, sun, "oxide sky sun");
        let moon_gpu = GpuTexture::upload(device, queue, moon_phases, "oxide sky moon");
        let sun_bind = sun_gpu.bind_group(device, &self.texture_layout);
        let moon_bind = moon_gpu.bind_group(device, &self.texture_layout);
        self.sun = Some((sun_gpu, sun_bind));
        self.moon = Some((moon_gpu, moon_bind));
    }

    /// Writes the frame uniform from the stored parameters and camera.
    fn refresh(&mut self, queue: &wgpu::Queue) {
        let (Some(params), Some(camera)) = (self.params.as_ref(), self.camera.as_ref()) else {
            return;
        };
        let aspect = self.aspect.max(0.01);
        let far_plane = params.far_plane * SKY_FAR_MULTIPLIER;
        // The eye the source's camera transform puts the sky at: the local frame's origin plus
        // the eye height — 1.62 standing, 1.54 while sneaking (`EntityRenderer.java`:738's
        // closing `translate(0.0F, -f, 0.0F)` reads the live entity's own eye height,
        // `EntityRenderer.java`:637) — less the first-person backward offset every
        // normally-played pass carries
        // (`EntityRenderer.setupCameraTransform`'s `translate(0.0F, 0.0F, -0.1F)`, `:720`) — the
        // same offset the terrain's own view has (`crate::camera::FIRST_PERSON_OFFSET`).
        let eye = sky_eye(camera);
        let view_projection = Mat4::perspective_rh(
            camera.fov_degrees.to_radians(),
            aspect,
            camera.near,
            far_plane,
        ) * Mat4::look_to_rh(eye, camera.forward(), Vec3::Y);
        let celestial = celestial_rotation(params.celestial_angle);
        let eye_y = camera.eye().y;
        let below_lift = 16.0 - (eye_y - HORIZON);
        self.below_horizon = eye_y < HORIZON;

        let mut values: Vec<f32> = Vec::with_capacity(SKY_UNIFORM_BYTES / 4);
        values.extend_from_slice(&view_projection.to_cols_array());
        values.extend_from_slice(&celestial.to_cols_array());
        values.extend_from_slice(&[
            params.sky_colour[0],
            params.sky_colour[1],
            params.sky_colour[2],
            1.0,
        ]);
        values.extend_from_slice(&[
            params.sky_colour[0] * 0.2 + 0.04,
            params.sky_colour[1] * 0.2 + 0.04,
            params.sky_colour[2] * 0.6 + 0.1,
            1.0,
        ]);
        values.extend_from_slice(&[
            params.fog_colour[0],
            params.fog_colour[1],
            params.fog_colour[2],
            1.0,
        ]);
        values.extend_from_slice(&[0.0, params.far_plane, below_lift, void_box_low(eye_y)]);
        values.extend_from_slice(&[params.star_brightness, eye.x, eye.y, eye.z]);
        let mut bytes = [0u8; SKY_UNIFORM_BYTES];
        f32_bytes(&values, &mut bytes);
        queue.write_buffer(&self.frame_buffer, 0, &bytes);
    }

    /// Draws the sky in the source's own order: the band, the sun, the moon and the stars, then
    /// — while the eye is below the horizon — the void, and last the below-horizon plane
    /// (`RenderGlobal.java:1231-1414`). The order decides occlusion, because the sky writes no
    /// depth: the celestial quads paint over the band, the void over both, and the plane over
    /// everything before it.
    ///
    /// Nothing draws until both parameters and a camera have been set; the sun and the moon are
    /// skipped until their textures land, the moon's quad is the frame's phase cell, and the
    /// stars are skipped when the frame's brightness is not above zero
    /// (`RenderGlobal.java:1327`).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (Some(params), Some(_)) = (self.params.as_ref(), self.camera.as_ref()) else {
            return;
        };
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, &self.fallback_bind_group, &[]);

        pass.set_pipeline(&self.background_pipeline);
        pass.set_vertex_buffer(0, self.background.slice(..));
        pass.draw(0..self.band_vertices, 0..1);

        if self.sun.is_some() || self.moon.is_some() {
            pass.set_pipeline(&self.celestial_pipeline);
            if let Some((_, bind)) = self.sun.as_ref() {
                pass.set_bind_group(1, bind, &[]);
                pass.set_vertex_buffer(0, self.sun_quad.slice(..));
                pass.draw(0..6, 0..1);
            }
            if let Some((_, bind)) = self.moon.as_ref() {
                pass.set_bind_group(1, bind, &[]);
                let phase = usize::from(params.moon_phase % 8);
                pass.set_vertex_buffer(0, self.moon_quads[phase].slice(..));
                pass.draw(0..6, 0..1);
            }
        }
        if params.star_brightness > 0.0 {
            pass.set_pipeline(&self.celestial_pipeline);
            pass.set_bind_group(1, &self.fallback_bind_group, &[]);
            pass.set_vertex_buffer(0, self.stars.slice(..));
            pass.draw(0..self.star_vertices, 0..1);
        }

        pass.set_pipeline(&self.background_pipeline);
        if self.below_horizon {
            pass.set_vertex_buffer(0, self.void.slice(..));
            pass.draw(0..self.void_vertices, 0..1);
        }
        pass.set_vertex_buffer(0, self.background.slice(..));
        pass.draw(self.band_vertices..self.background_vertices, 0..1);
    }
}

/// The cloud pass: the flat layer's pipeline, its uniform and its texture.
pub struct CloudPass {
    /// The layer's pipeline.
    pipeline: wgpu::RenderPipeline,
    /// The frame uniform buffer.
    frame_buffer: wgpu::Buffer,
    /// The bind group for the frame uniform.
    frame_bind_group: wgpu::BindGroup,
    /// The layout group 1 binds the cloud texture through.
    texture_layout: wgpu::BindGroupLayout,
    /// `environment/clouds`, once uploaded, and its bind group.
    clouds: Option<(GpuTexture, wgpu::BindGroup)>,
    /// The layer's 16 by 16 cells, one triangle pair each.
    vertices: wgpu::Buffer,
    /// How many vertices the layer holds.
    vertex_count: u32,
    /// The frame's sky parameters, once the client has set them.
    params: Option<SkyParams>,
    /// The camera the last frame was built with.
    camera: Option<Camera>,
    /// The surface aspect ratio the last camera was given.
    aspect: f32,
    /// The frame's fog, as [`crate::fog::FogParams`] carried it.
    fog: FogParams,
}

/// The fog a cloud pass draws with until one is set: a range that does not run forwards, which
/// the shader reads as "leave the fragment as it is".
const NO_FOG: FogParams = FogParams {
    colour: [0.0; 3],
    start: 0.0,
    end: 0.0,
    far_plane: 0.0,
};

impl CloudPass {
    /// Builds the cloud pipeline, the frame uniform and the layer's geometry for colour
    /// attachments in `format`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide cloud shader"),
            source: wgpu::ShaderSource::Wgsl(cloud_shader().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide cloud frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(CLOUD_UNIFORM_BYTES as u64),
                },
                count: None,
            }],
        });
        let texture_layout = texture_layout(device, "oxide cloud texture layout");
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide cloud frame"),
            size: CLOUD_UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&frame_buffer, 0, &[0u8; CLOUD_UNIFORM_BYTES]);
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide cloud frame bind group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide cloud pipeline layout"),
            bind_group_layouts: &[&frame_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide cloud pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[cloud_vertex_layout()],
            },
            // The source's fast arm disables culling for the layer
            // (`GlStateManager.disableCull()`, `RenderGlobal.java:1430`).
            primitive: primitive_state(None),
            depth_stencil: Some(cloud_depth_state()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(color_target(format, Some(cloud_blend())))],
            }),
            multiview: None,
            cache: None,
        });

        let mut cells = Vec::new();
        for origin_z in cloud_origins() {
            for origin_x in cloud_origins() {
                // The source's own corner order (`RenderGlobal.java:1467-1474`), expanded to
                // the two triangles `GL_QUADS` draws.
                let corners = [
                    [origin_x, origin_z + CLOUD_CELL],
                    [origin_x + CLOUD_CELL, origin_z + CLOUD_CELL],
                    [origin_x + CLOUD_CELL, origin_z],
                    [origin_x, origin_z],
                ];
                let [a, b, c, d] = corners;
                for [x, z] in [a, b, c, a, c, d] {
                    cells.push([x, 0.0, z]);
                }
            }
        }
        let vertex_count = cells.len() as u32;
        let mut bytes = Vec::with_capacity(cells.len() * 12);
        for cell in &cells {
            for value in cell {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide cloud layer"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertices, 0, &bytes);

        Self {
            pipeline,
            frame_buffer,
            frame_bind_group,
            texture_layout,
            clouds: None,
            vertices,
            vertex_count,
            params: None,
            camera: None,
            aspect: 1.0,
            fog: NO_FOG,
        }
    }

    /// Stores the frame's sky parameters and refreshes the uniform.
    pub fn set_params(&mut self, queue: &wgpu::Queue, params: SkyParams) {
        self.params = Some(params);
        self.refresh(queue);
    }

    /// Stores the camera and the surface aspect the next draw is built with, and refreshes the
    /// uniform.
    pub fn set_camera(&mut self, queue: &wgpu::Queue, camera: Camera, aspect: f32) {
        self.camera = Some(camera);
        self.aspect = aspect;
        self.refresh(queue);
    }

    /// Stores the frame's fog, the terrain range the clouds fade over
    /// (`EntityRenderer.setupFog(0)`, `EntityRenderer.java:1361`, `:1500`), and refreshes the
    /// uniform.
    pub fn set_fog(&mut self, queue: &wgpu::Queue, fog: FogParams) {
        self.fog = fog;
        self.refresh(queue);
    }

    /// Uploads the cloud texture, replacing any earlier one.
    pub fn set_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, clouds: &Texture) {
        let gpu = GpuTexture::upload(device, queue, clouds, "oxide cloud texture");
        let bind = gpu.bind_group(device, &self.texture_layout);
        self.clouds = Some((gpu, bind));
    }

    /// Writes the frame uniform from the stored parameters, camera and fog.
    fn refresh(&mut self, queue: &wgpu::Queue) {
        let (Some(params), Some(camera)) = (self.params.as_ref(), self.camera.as_ref()) else {
            return;
        };
        let aspect = self.aspect.max(0.01);
        let far_plane = params.far_plane * CLOUD_FAR_MULTIPLIER;
        let view_projection = Mat4::perspective_rh(
            camera.fov_degrees.to_radians(),
            aspect,
            camera.near,
            far_plane,
        ) * camera.view();
        let view_x = camera.pose.position[0];
        let view_z = camera.pose.position[2];
        // M2 has no tick loop; the partial tick the source interpolates with is zero.
        let partial_ticks = 0.0;
        let uv_x = cloud_uv_x(view_x, params.cloud_offset_ticks, partial_ticks);
        let uv_z = cloud_uv_z(view_z);

        let mut values: Vec<f32> = Vec::with_capacity(CLOUD_UNIFORM_BYTES / 4);
        values.extend_from_slice(&view_projection.to_cols_array());
        values.extend_from_slice(&[
            params.cloud_colour[0],
            params.cloud_colour[1],
            params.cloud_colour[2],
            1.0,
        ]);
        values.extend_from_slice(&[view_x as f32, view_z as f32, uv_x, uv_z]);
        values.extend_from_slice(&[
            self.fog.colour[0],
            self.fog.colour[1],
            self.fog.colour[2],
            1.0,
        ]);
        values.extend_from_slice(&[self.fog.start, self.fog.end, self.fog.far_plane, 0.0]);
        // The cloud's view is the world view (`camera.view()`), whose origin is the eye less the
        // first-person offset; the fog's radial distance is measured from the same point.
        let eye = camera.eye() - FIRST_PERSON_OFFSET * camera.forward();
        values.extend_from_slice(&[eye.x, eye.y, eye.z, 0.0]);
        let mut bytes = [0u8; CLOUD_UNIFORM_BYTES];
        f32_bytes(&values, &mut bytes);
        queue.write_buffer(&self.frame_buffer, 0, &bytes);
    }

    /// Draws the cloud layer.
    ///
    /// Nothing draws until parameters, a camera and the cloud texture have all been set; the
    /// caller decides whether the eye is under the layer with [`cloud_under_layer`].
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let (Some(_), Some(_)) = (self.params.as_ref(), self.camera.as_ref()) else {
            return;
        };
        let Some((_, bind)) = self.clouds.as_ref() else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, bind, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use wgpu::{BlendFactor, CompareFunction, Face, FrontFace};

    use super::{
        CLOUD_DRIFT_PER_TICK, CLOUD_UV_PER_BLOCK, KIND_TEXTURED, SKY_VERTEX_BYTES, celestial_blend,
        celestial_rotation, cloud_at_or_above_layer, cloud_blend, cloud_drift, cloud_layer_y,
        cloud_under_layer, cloud_uv_x, cloud_uv_z, grid_cell, moon_uv, sky_depth_state, sky_eye,
        sky_vertex_layout, star_field, sun_quad_vertices, void_box_low,
    };
    use crate::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE, NO_VIEW_EFFECT};

    #[test]
    fn the_sky_vertex_layout_matches_the_byte_stream() {
        let layout = sky_vertex_layout();
        assert_eq!(layout.array_stride, SKY_VERTEX_BYTES as u64);
        let offsets: Vec<u64> = layout.attributes.iter().map(|a| a.offset).collect();
        assert_eq!(offsets, vec![0, 12, 16, 24]);
    }

    #[test]
    fn the_celestial_blend_is_the_sources_additive_pair() {
        let blend = celestial_blend();
        assert_eq!(blend.color.src_factor, BlendFactor::SrcAlpha);
        assert_eq!(blend.color.dst_factor, BlendFactor::One);
        assert_eq!(blend.alpha.src_factor, BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, BlendFactor::Zero);
    }

    #[test]
    fn the_cloud_blend_is_the_sources_alpha_pair() {
        let blend = cloud_blend();
        assert_eq!(blend.color.src_factor, BlendFactor::SrcAlpha);
        assert_eq!(blend.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.alpha.src_factor, BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, BlendFactor::Zero);
    }

    #[test]
    fn the_sky_writes_no_depth_and_the_cloud_writes() {
        let sky = sky_depth_state();
        assert!(!sky.depth_write_enabled, "the sky never writes depth");
        assert_eq!(sky.depth_compare, CompareFunction::Always);
        let cloud = super::cloud_depth_state();
        assert!(cloud.depth_write_enabled, "the cloud layer writes depth");
        assert_eq!(cloud.depth_compare, CompareFunction::Less);
    }

    #[test]
    fn the_sky_culls_back_faces_as_the_source_does() {
        assert_eq!(
            super::primitive_state(Some(Face::Back)).cull_mode,
            Some(Face::Back)
        );
        assert_eq!(super::primitive_state(None).cull_mode, None);
        assert_eq!(
            super::primitive_state(Some(Face::Back)).front_face,
            FrontFace::Ccw
        );
    }

    #[test]
    fn the_grid_cells_wind_the_way_the_source_emits_them() {
        let forward = grid_cell(0.0, 0.0, false);
        assert_eq!(
            forward,
            [[0.0, 0.0], [64.0, 0.0], [64.0, 64.0], [0.0, 64.0]]
        );
        let reversed = grid_cell(0.0, 0.0, true);
        assert_eq!(
            reversed,
            [[64.0, 0.0], [0.0, 0.0], [0.0, 64.0], [64.0, 64.0]]
        );
    }

    #[test]
    fn the_sun_quad_carries_its_own_uvs() {
        let vertices = sun_quad_vertices();
        assert_eq!(vertices.len(), 6);
        // The two triangles carry the four corners in the source's order: 0, 1, 2 and 0, 2, 3.
        let uvs: Vec<[f32; 2]> = [0, 1, 2, 5].map(|index| vertices[index].uv).to_vec();
        assert_eq!(uvs, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert!(vertices.iter().all(|v| v.kind == KIND_TEXTURED));
    }

    #[test]
    fn the_moon_cell_wraps_across_the_sheet() {
        assert_eq!(moon_uv(0)[0], [0.25, 0.5]);
        assert_eq!(moon_uv(5)[0], [0.5, 1.0]);
    }

    #[test]
    fn the_moon_quads_carry_each_phase_cell() {
        // The pass keeps one quad per phase; each carries its own cell's uvs. Phase 5 is the
        // sheet's second column, second row: 0.25..0.5 across, 0.5..1 down.
        let vertices = super::moon_quad_vertices(5);
        assert_eq!(vertices[0].uv, [0.5, 1.0], "the cell's bottom-right corner");
        assert_eq!(vertices[5].uv, [0.5, 0.5], "the cell's top-right corner");
        for phase in 0..8 {
            let cell = moon_uv(phase);
            let quad = super::moon_quad_vertices(phase);
            for (index, corner) in [(0usize, 0usize), (1, 1), (2, 2), (5, 3)] {
                assert_eq!(quad[index].uv, cell[corner], "phase {phase}");
            }
        }
    }

    #[test]
    fn the_star_count_and_first_centre_come_from_the_seeded_generator() {
        let stars = star_field();
        assert_eq!(stars.len(), 780);
        assert!((stars[0].centre[0] - -53.24686660681045).abs() < 1e-9);
    }

    #[test]
    fn the_cloud_uv_chain_is_the_sources_own() {
        assert_eq!(cloud_drift(1, 0.0), CLOUD_DRIFT_PER_TICK);
        assert_eq!(cloud_uv_x(1.0, 0, 0.0), CLOUD_UV_PER_BLOCK as f32);
        assert_eq!(cloud_uv_z(2048.5), cloud_uv_z(0.5));
        assert_eq!(cloud_layer_y(64.0), 64.33);
    }

    #[test]
    fn the_void_floor_and_the_under_layer_gate_hold_their_edges() {
        assert_eq!(void_box_low(70.0), -72.0);
        let under = Camera {
            pose: CameraPose {
                position: [0.5, 126.0, 0.5],
                yaw: 0.0,
                pitch: 0.0,
                sneak: false,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: NO_VIEW_EFFECT,
        };
        assert!(cloud_under_layer(&under));
        assert_eq!(under.eye().y, 127.62);
    }

    #[test]
    fn the_cloud_gates_split_at_the_entity_eye_at_the_layers_height() {
        // The source's two guards are exact complements around the layer's 128: the under-arm
        // while `entity.posY + entity.getEyeHeight() < 128.0` (`EntityRenderer.java:1364`) and
        // the at-or-above arm once it is `>= 128.0` (`:1474`). The basis is the entity eye —
        // the reported feet position plus EYE_HEIGHT — not the first-person camera the view
        // pulls a tenth of a block back along the view axis.
        let at = |feet_y: f64, pitch: f32| Camera {
            pose: CameraPose {
                position: [0.5, feet_y, 0.5],
                yaw: 0.0,
                pitch,
                sneak: false,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: NO_VIEW_EFFECT,
        };
        // The wall pose's feet 57.0: eye 58.62, under the layer.
        let wall = at(57.0, 0.0);
        assert!(cloud_under_layer(&wall));
        assert!(!cloud_at_or_above_layer(&wall));
        // The acceptance's mark pose: feet 150.0, eye 151.62, at or above the layer.
        let mark = at(150.0, 20.0);
        assert!(!cloud_under_layer(&mark));
        assert!(cloud_at_or_above_layer(&mark));
        // The boundary itself: the feet height whose eye is exactly the layer's own 128
        // (`128.0 - EYE_HEIGHT`, the f32 widening included), which the source's `>=` draws
        // the at-or-above arm for; a hundredth lower is still under the layer.
        let at_layer = at(128.0 - f64::from(EYE_HEIGHT), 0.0);
        assert_eq!(at_layer.eye().y, 128.0, "the boundary pose's eye");
        assert!(!cloud_under_layer(&at_layer));
        assert!(cloud_at_or_above_layer(&at_layer));
        // The sneak flag moves the basis: the same pose with the flag set
        // drops the eye 0.08 under the layer and the two arms swap
        // (`EntityPlayer.getEyeHeight`'s `f -= 0.08F`,
        // `EntityPlayer.java`:2335-2338).
        let crouched = Camera {
            pose: CameraPose {
                sneak: true,
                ..at_layer.pose
            },
            ..at_layer
        };
        assert!(
            (crouched.eye().y - 127.92).abs() < 1e-3,
            "the sneak eye drops the 0.08: {}",
            crouched.eye().y
        );
        assert!(cloud_under_layer(&crouched));
        assert!(!cloud_at_or_above_layer(&crouched));
        let last_under = at(126.37, 0.0);
        assert!(cloud_under_layer(&last_under));
        assert!(!cloud_at_or_above_layer(&last_under));
        // The basis discriminator: looking 89 degrees up pulls the first-person camera below
        // the layer while the entity eye stands above it, so a gate reading the view's origin
        // instead of the entity eye would answer "under" here.
        let lifted = at(126.43, -89.0);
        let origin = lifted.view().inverse().transform_point3(Vec3::ZERO);
        assert!(
            origin.y < 128.0,
            "the view origin sits below the layer: {}",
            origin.y
        );
        assert!(cloud_at_or_above_layer(&lifted));
        assert!(!cloud_under_layer(&lifted));
        // The sky view's own eye reads the same live pose: at pitch 0 it is the eye height
        // straight up — 1.62 standing, 1.54 while sneaking (`EntityRenderer.java`:738's
        // closing translate reads the live eye height, `:637`).
        assert_eq!(
            sky_eye(&at_layer).y,
            1.62,
            "the standing sky eye reads the pose's height"
        );
        assert_eq!(
            sky_eye(&crouched).y,
            1.54,
            "the sneaking sky eye reads the live height"
        );
    }

    #[test]
    fn the_celestial_rotation_keeps_noon_overhead_and_puts_dusk_west() {
        let rotation = celestial_rotation(0.0);
        let sun = rotation.transform_point3(Vec3::new(0.0, 100.0, 0.0));
        assert!(sun.abs_diff_eq(Vec3::new(0.0, 100.0, 0.0), 1e-5));
        let rotation = celestial_rotation(0.25);
        let sun = rotation.transform_point3(Vec3::new(0.0, 100.0, 0.0));
        assert!(
            sun.abs_diff_eq(Vec3::new(-100.0, 0.0, 0.0), 1e-4),
            "the quarter turn puts the sun on the western horizon: {sun:?}"
        );
    }
}
