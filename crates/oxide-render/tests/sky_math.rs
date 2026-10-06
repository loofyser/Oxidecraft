//! The sky and cloud maths, pinned against the source's own numbers.
//!
//! Every literal below was derived from the MCP-919 clone before the code existed: the
//! geometry figures from `RenderGlobal`'s own loops and quads, the rotation from its two
//! `rotate` calls, the cloud chain from `renderClouds`' fast arm, and the star field from
//! `renderStars`. The star figures come from the JVM harness
//! `refs/m2-task-12/star_field.java`, which copies the source's generator expression for
//! expression and runs the real `java.util.Random(10842L)` — the same method Task 6 used
//! for its random vectors. The first harness, `sky_literals.java`, omitted the per-star spin
//! draw (`RenderGlobal.java:431`), so the accepted count and the later stars' centres it
//! printed were not the source's; the corrected harness's figures are the ones pinned here.
//!
//! Nothing here needs a device: these are the values the pass's geometry and uniforms are
//! built from, so a change to any of them moves the picture and fails here first.

use glam::Vec3;
use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NO_VIEW_EFFECT};
use oxide_render::sky::{
    BAND_CELL, BAND_EXTENT, BAND_HEIGHT, BELOW_HEIGHT, CLOUD_ALPHA, CLOUD_CELL,
    CLOUD_DRIFT_PER_TICK, CLOUD_EXTENT, CLOUD_HEIGHT, CLOUD_LIFT, CLOUD_UV_PER_BLOCK, CLOUD_WRAP,
    HORIZON, MOON_HALF_SIZE, MOON_HEIGHT, MOON_SHEET_COLUMNS, MOON_SHEET_HEIGHT, MOON_SHEET_ROWS,
    MOON_SHEET_WIDTH, STAR_COUNT, STAR_RADIUS, STAR_SEED, SUN_HALF_SIZE, SUN_HEIGHT, VOID_BOX_DROP,
    VOID_BOX_HALF, VOID_BOX_LID, band_origins, celestial_rotation, cloud_drift, cloud_layer_y,
    cloud_origins, cloud_under_layer, cloud_uv_x, cloud_uv_z, moon_uv, star_field, void_box_low,
};

/// The sun's and moon's quads are laid out in the camera-relative frame the celestial
/// rotation turns; these are the source's own figures (`RenderGlobal.java:1301-1322`).
#[test]
fn the_sun_and_moon_quads_are_the_sources_own() {
    assert_eq!(SUN_HALF_SIZE, 30.0, "the sun's half-size (`:1301`)");
    assert_eq!(SUN_HEIGHT, 100.0, "and its height (`:1304-1307`)");
    assert_eq!(MOON_HALF_SIZE, 20.0, "the moon's half-size (`:1309`)");
    assert_eq!(MOON_HEIGHT, -100.0, "and its height (`:1319-1322`)");
    assert_eq!(
        (MOON_SHEET_WIDTH, MOON_SHEET_HEIGHT),
        (128, 64),
        "the moon sheet"
    );
    assert_eq!(
        (MOON_SHEET_COLUMNS, MOON_SHEET_ROWS),
        (4, 2),
        "its 4x2 phase grid"
    );
}

/// The moon's phase cell: `k = phase % 4`, `i1 = phase / 4 % 2` over the sheet, sampled at
/// the quad's four corners in the source's own order (`RenderGlobal.java:1312-1322`).
#[test]
fn the_moon_uvs_pick_the_phase_cell() {
    // Phase 0 is the sheet's first cell: u 0..0.25, v 0..0.5.
    assert_eq!(
        moon_uv(0),
        [[0.25, 0.5], [0.0, 0.5], [0.0, 0.0], [0.25, 0.0]],
        "phase 0"
    );
    // Phase 3 is the first row's last cell; phase 4 wraps to the second row's first.
    assert_eq!(
        moon_uv(3),
        [[1.0, 0.5], [0.75, 0.5], [0.75, 0.0], [1.0, 0.0]],
        "phase 3"
    );
    assert_eq!(
        moon_uv(4),
        [[0.25, 1.0], [0.0, 1.0], [0.0, 0.5], [0.25, 0.5]],
        "phase 4"
    );
    assert_eq!(
        moon_uv(7),
        [[1.0, 1.0], [0.75, 1.0], [0.75, 0.5], [1.0, 0.5]],
        "phase 7"
    );
}

/// The celestial rotation: `rotate(-90, Y)` then `rotate(angle * 360, X)`
/// (`RenderGlobal.java:1299-1300`). The rotation's product is `Ry(-90) * Rx(angle * 360)`, the
/// order the source's two post-multiplying `glRotate` calls build. The sun's centre is
/// `(0, 100, 0)` and the moon's `(0, -100, 0)` before it (`:1304`, `:1319`).
#[test]
fn the_celestial_rotation_turns_the_sun_from_noon_to_midnight() {
    let at = |angle: f32| {
        let rotation = celestial_rotation(angle);
        let sun = rotation.transform_point3(Vec3::new(0.0, SUN_HEIGHT, 0.0));
        let moon = rotation.transform_point3(Vec3::new(0.0, MOON_HEIGHT, 0.0));
        (sun, moon)
    };

    let (sun, moon) = at(0.0);
    assert!(
        sun.abs_diff_eq(Vec3::new(0.0, 100.0, 0.0), 1e-5),
        "at noon the sun is overhead, got {sun:?}"
    );
    assert!(
        moon.abs_diff_eq(Vec3::new(0.0, -100.0, 0.0), 1e-5),
        "and the moon underfoot"
    );

    let (sun, moon) = at(0.5);
    assert!(
        sun.abs_diff_eq(Vec3::new(0.0, -100.0, 0.0), 1e-5),
        "at midnight the sun is underfoot, got {sun:?}"
    );
    assert!(
        moon.abs_diff_eq(Vec3::new(0.0, 100.0, 0.0), 1e-5),
        "and the moon overhead"
    );

    // A quarter turn carries the sun to the western horizon: `Ry(-90)` maps the rotated
    // `(0, 0, 100)` to `(-100, 0, 0)`, which is the source's composition order applied to its
    // own sun position. A port that multiplied the rotations the other way would leave the sun
    // on `+z`, an eastern evening sky.
    let (sun, _) = at(0.25);
    assert!(
        sun.abs_diff_eq(Vec3::new(-100.0, 0.0, 0.0), 1e-4),
        "a quarter turn puts the sun on the western horizon, got {sun:?}"
    );
}

/// The band and the below-horizon plane: 64-unit cells spanning ±384 on both axes
/// (`RenderGlobal.java:340-365`), the band at `y = +16` (`:325`) and the plane at
/// `y = -16` (`:291`), with the horizon `World.getHorizon` answers for the Overworld
/// (63.0).
#[test]
fn the_band_cells_span_the_sources_extent() {
    assert_eq!(BAND_CELL, 64.0, "a cell is 64 units (`:342`)");
    assert_eq!(BAND_EXTENT, 384.0, "the loop runs -384..=384 (`:346-348`)");
    assert_eq!(BAND_HEIGHT, 16.0, "the band's height (`:325`)");
    assert_eq!(BELOW_HEIGHT, -16.0, "the plane's (`:291`)");
    assert_eq!(HORIZON, 63.0, "the Overworld's horizon");

    let origins = band_origins();
    assert_eq!(origins.len(), 13, "(-384..=384).step_by(64) is 13 cells");
    assert_eq!(origins[0], -384.0);
    assert_eq!(origins[12], 384.0);
    for pair in origins.windows(2) {
        assert_eq!(pair[1] - pair[0], BAND_CELL, "the cells tile without a gap");
    }
}

/// The void box the source fills under the horizon while the eye is below it: a column
/// `±1` wide whose lid is one unit under the eye-space origin and whose floor is
/// `-(d0 + 65)` (`RenderGlobal.java:1375-1399`).
#[test]
fn the_void_box_hangs_from_the_source_drop() {
    assert_eq!(VOID_BOX_HALF, 1.0, "the box is ±1 wide (`:1379`)");
    assert_eq!(VOID_BOX_LID, -1.0, "its lid sits at -1 (`:1381`)");
    assert_eq!(VOID_BOX_DROP, 65.0, "the drop constant (`:1376`)");
    // f19 = -((d0 + 65)) with d0 = eye - 63: at eye 70 the floor is -(7 + 65) = -72.
    assert_eq!(void_box_low(70.0), -72.0);
    assert_eq!(void_box_low(10.0), -12.0);
}

/// The cloud layer: 32-block cells spanning ±256 around the camera (`RenderGlobal.java:1432`,
/// `:1467-1474`), the layer's height from `WorldProvider.getCloudHeight` at 128.0 with the
/// source's 0.33 lift (`:1462`), drawn at `128 - camera_y + 0.33` (`:1462`), tinted at the
/// source's alpha (`:1471`).
#[test]
fn the_cloud_layer_sits_at_the_sources_height() {
    assert_eq!(CLOUD_CELL, 32.0, "a cell is 32 blocks (`:1432`)");
    assert_eq!(
        CLOUD_EXTENT, 256.0,
        "the loop runs -256..256 (`:1467-1469`)"
    );
    assert_eq!(CLOUD_HEIGHT, 128.0, "the Overworld's cloud height");
    assert_eq!(CLOUD_LIFT, 0.33, "the source's lift (`:1462`)");
    assert_eq!(CLOUD_ALPHA, 0.8, "the quad's alpha (`:1471-1474`)");

    let origins = cloud_origins();
    assert_eq!(origins.len(), 16, "(-256..256).step_by(32) is 16 cells");
    assert_eq!(origins[0], -256.0);
    assert_eq!(origins[15], 224.0);

    // The layer is camera-relative: 128 above the camera's own feet, plus the lift.
    assert_eq!(cloud_layer_y(64.0), 64.33);
    assert_eq!(cloud_layer_y(70.0), 58.33);

    // The eye-under-the-layer gate (`EntityRenderer.java:1364`): the eye, not the
    // feet, at 128 and above draws no clouds.
    assert!(
        cloud_under_layer(&camera_at(126.0)),
        "the eye at 127.62 is under"
    );
    assert!(
        !cloud_under_layer(&camera_at(126.5)),
        "the eye at 128.12 is not"
    );
}

/// The cloud offset chain (`RenderGlobal.java:1454-1464`): the view's x plus
/// `(cloudTickCounter + partialTicks) * 0.03` blocks of drift, wrapped into `0..2048` on
/// each axis on its own, times the `1/2048` per block scale.
///
/// The drift constant is the source's own float literal, the `f32` 0.03 widened to `f64`
/// — `0.029999999329447746`, not the round decimal — so one counter tick advances the uv
/// by `1.46484372E-5`, the brief's `0.03 * 4.8828125E-4` to seven figures.
#[test]
fn the_cloud_uv_advances_by_the_sources_drift() {
    assert_eq!(CLOUD_DRIFT_PER_TICK, 0.029999999329447746);
    assert_eq!(cloud_drift(1, 0.0), CLOUD_DRIFT_PER_TICK);
    assert_eq!(cloud_drift(100, 0.0), 2.9999999329447746);
    assert_eq!(cloud_drift(0, 0.5), 0.014999999664723873);

    // One tick's uv, and a hundred's: the per-tick rate is the brief's 1.46484375E-5
    // to the float's own precision.
    let per_tick = f64::from(cloud_uv_x(0.0, 1, 0.0)) - f64::from(cloud_uv_x(0.0, 0, 0.0));
    assert!(
        (per_tick - 1.46484375E-5).abs() < 1e-9,
        "one tick advances the uv by {per_tick}, want 1.46484375E-5"
    );
    let hundred = f64::from(cloud_uv_x(0.0, 100, 0.0));
    assert!(
        (hundred - 1.46484375E-3).abs() < 1e-7,
        "a hundred ticks advance it by {hundred}, want 1.46484375E-3"
    );

    // A block of the view's own position is worth exactly the 1/2048 scale.
    assert_eq!(cloud_uv_x(1.0, 0, 0.0), CLOUD_UV_PER_BLOCK as f32);
    assert_eq!(cloud_uv_z(1.0), CLOUD_UV_PER_BLOCK as f32);

    // The wrap: each axis into 0..2048 on its own, so 2048.5 is 0.5 again.
    assert_eq!(cloud_uv_x(2048.5, 0, 0.0), cloud_uv_x(0.5, 0, 0.0));
    assert_eq!(cloud_uv_z(2048.5), cloud_uv_z(0.5));
    assert_eq!(cloud_uv_x(-0.5, 0, 0.0), cloud_uv_x(2047.5, 0, 0.0));
    assert_eq!(cloud_uv_z(-0.5), cloud_uv_z(2047.5));
    assert_eq!(CLOUD_WRAP, 2048.0);
}

/// The star field: 1500 iterations of the source's generator under seed 10842
/// (`RenderGlobal.java:405-408`), the accepted stars' centres and the first three's
/// coordinates from the JVM harness.
#[test]
fn the_star_field_is_the_sources_generator() {
    assert_eq!(STAR_COUNT, 1500, "the loop count (`:408`)");
    assert_eq!(STAR_SEED, 10842, "the seed (`:405`)");
    assert_eq!(STAR_RADIUS, 100.0, "the normalised radius (`:422-424`)");

    let stars = star_field();
    // 780 of the 1500 draws pass `0.01 < |p|^2 < 1` (`:416`), from the corrected harness.
    assert_eq!(stars.len(), 780, "the accepted stars");

    // The first three stars' centres, straight from the corrected harness.
    let want = [
        [-53.24686660681045, 69.92574001276357, 47.69865910299712],
        [-96.89088600474103, -18.466674924841698, 16.466272390450047],
        [-98.79929024201796, 9.651537235814288, 12.064330758860493],
    ];
    for (index, want) in want.into_iter().enumerate() {
        let got = stars[index].centre;
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-9,
                "star {index} axis {axis}: got {}, want {}",
                got[axis],
                want[axis]
            );
        }
    }
    // The sizes are `0.15 + nextFloat() * 0.1`, so a quad is at most a 0.25 half-diagonal
    // and every corner sits within 0.25 * sqrt(2) of its centre.
    for star in &stars {
        assert!(
            (0.15..0.25).contains(&star.size),
            "a star's size is in [0.15, 0.25), got {}",
            star.size
        );
        for corner in star.corners {
            let offset = Vec3::new(corner[0], corner[1], corner[2])
                - Vec3::new(
                    star.centre[0] as f32,
                    star.centre[1] as f32,
                    star.centre[2] as f32,
                );
            assert!(
                offset.length() <= 0.25 * std::f32::consts::SQRT_2 + 1e-4,
                "a corner sits at {}",
                offset.length()
            );
        }
    }
}

/// A camera at a feet height, looking north, for the under-the-layer gate.
fn camera_at(feet_y: f64) -> Camera {
    Camera {
        pose: CameraPose {
            position: [0.5, feet_y, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            sneak: false,
        },
        fov_degrees: DEFAULT_FOV,
        near: 0.05,
        far_chunks: 8.0,
        view_effect: NO_VIEW_EFFECT,
    }
}

/// The eye height the gate measures with is the client's own `EYE_HEIGHT`.
#[test]
fn the_under_layer_gate_measures_the_eye() {
    assert_eq!(EYE_HEIGHT, 1.62);
    assert_eq!(camera_at(126.0).eye().y, 127.62);
}
