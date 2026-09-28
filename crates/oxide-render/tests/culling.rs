//! Tests for the frame's frustum and for the section boxes it culls. Pure maths: no GPU is
//! involved, so these run everywhere.
//!
//! The frame the geometry cases use is the one the task pins: a camera at the origin looking
//! down -Z, the vanilla 70-degree vertical field of view, an aspect of one, the near plane
//! 0.05 and the far plane 256 blocks. It is built from `glam`'s right-handed, 0..1-depth
//! projection, the same convention `camera::Camera::view_projection` produces; the final test
//! drives the camera type itself, so the extraction the pass uses is pinned against the
//! matrix the renderer really uploads.
//!
//! A section is a 16-block cube: x * 16 .. x * 16 + 16, likewise z, and sy * 16 .. + 16. The
//! cull is a rejection test — a box is dropped only when it lies fully outside one of the six
//! planes — so every boundary case here is "the box touches the plane and must still draw".

use glam::{Mat4, Vec3};
use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, NEAR_PLANE};
use oxide_render::frustum::{Aabb3, Frustum, Plane};

/// The far plane of the pinned test frame, in blocks.
const FAR: f32 = 256.0;

/// Half the vertical field of view, in radians: 35 degrees.
fn half_angle() -> f32 {
    (DEFAULT_FOV / 2.0).to_radians()
}

/// The pinned frame: eye at the origin, forward -Z, up +Y, near 0.05, far 256.
fn frame() -> Frustum {
    Frustum::from_view_projection(
        Mat4::perspective_rh(DEFAULT_FOV.to_radians(), 1.0, NEAR_PLANE, FAR)
            * Mat4::look_to_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y),
    )
}

/// Whether the section at chunk `x`/`z` and section index `section_y` survives the cull.
fn survives(frustum: &Frustum, x: i32, section_y: u8, z: i32) -> bool {
    frustum.intersects(&Aabb3::section(x, section_y, z))
}

/// Asserts one extracted plane against a hand-computed normal and offset.
fn expect_plane(planes: &[Plane; 6], index: usize, what: &str, normal: Vec3, offset: f32) {
    let plane = planes[index];
    let close = |got: f32, want: f32| (got - want).abs() < 1e-5;
    assert!(
        close(plane.normal.x, normal.x)
            && close(plane.normal.y, normal.y)
            && close(plane.normal.z, normal.z),
        "the {what} plane's normal: got {:?}, want {normal:?}",
        plane.normal
    );
    assert!(
        close(plane.offset, offset),
        "the {what} plane's offset: got {}, want {offset}",
        plane.offset
    );
    assert!(
        close(plane.normal.length(), 1.0),
        "the {what} plane's normal is not unit length: {}",
        plane.normal.length()
    );
}

#[test]
fn the_six_planes_have_the_hand_computed_normals_and_offsets() {
    let frustum = frame();
    let planes = frustum.planes();
    let (sin, cos) = half_angle().sin_cos();
    // The order the frustum documents: left, right, bottom, top, near, far. The side planes
    // pass through the eye and tip back by the half angle; the near and far planes are the
    // 0..1-depth convention's own, which is what makes them (0, 0, -1, -0.05) and
    // (0, 0, 1, 256) rather than an OpenGL -1..1 pair.
    expect_plane(&planes, 0, "left", Vec3::new(cos, 0.0, -sin), 0.0);
    expect_plane(&planes, 1, "right", Vec3::new(-cos, 0.0, -sin), 0.0);
    expect_plane(&planes, 2, "bottom", Vec3::new(0.0, cos, -sin), 0.0);
    expect_plane(&planes, 3, "top", Vec3::new(0.0, -cos, -sin), 0.0);
    expect_plane(&planes, 4, "near", Vec3::new(0.0, 0.0, -1.0), -NEAR_PLANE);
    // The far plane's normal is exact, but its offset is not: the projection stores z = -1 -
    // near/(far - near), and at this frame's far/near ratio of 5120 the far distance lives in
    // the low bits of that f32 sum, so the extracted plane sits 0.044 blocks short of the
    // nominal 256. The row combination is what the task pins, and no extraction of the same
    // f32 matrix does better (the loss is in the stored matrix, not in the combination), so
    // the assertion allows the loss and fails if it ever widens.
    let far = planes[5];
    assert_eq!(far.normal, Vec3::new(0.0, 0.0, 1.0));
    assert!(
        (far.offset - FAR).abs() < 0.05,
        "the far plane's offset: got {}, want {FAR} within the f32 loss",
        far.offset
    );
    assert!(
        far.offset <= FAR,
        "the far plane never reaches past its distance"
    );
}

#[test]
fn a_section_spanning_the_origin_is_visible() {
    // The section at chunk -1 spans x and z -16..0 and y 0..16: the eye stands on its corner,
    // so a cull must never drop it, whatever the other planes say.
    assert!(
        survives(&frame(), -1, 0, -1),
        "the section under the camera was culled"
    );
}

#[test]
fn a_section_directly_behind_the_camera_is_culled() {
    // Chunk z 1 spans z 16..32: entirely behind the -Z camera, so the near plane rejects it.
    assert!(
        !survives(&frame(), 0, 0, 1),
        "a section behind the camera was drawn"
    );
}

#[test]
fn a_section_whose_face_touches_the_far_plane_is_visible() {
    let frustum = frame();
    // A section-sized box placed so its near face lies exactly on the extracted far plane:
    // touching a plane is intersecting it, so the box is kept. The face is taken from the
    // plane itself because the extraction places this frame's far plane 0.044 blocks short of
    // its nominal 256 (see `the_six_planes_...`), which is what makes the boundary exact.
    let far = frustum.planes()[5].offset;
    let touching = Aabb3::new([-8.0, -8.0, -far - 16.0], [8.0, 8.0, -far]);
    assert!(
        frustum.intersects(&touching),
        "a box with a face exactly on the far plane was culled"
    );
    // The same box a hair beyond the plane is fully outside it and is culled.
    let past = Aabb3::new([-8.0, -8.0, -far - 16.0], [8.0, 8.0, -far - 0.01]);
    assert!(
        !frustum.intersects(&past),
        "a box entirely beyond the far plane was drawn"
    );
}

#[test]
fn the_last_section_inside_the_far_plane_is_visible() {
    // Chunk z -15 spans z -240..-224: the last section a 256-block view holds, sixteen blocks
    // clear of the far plane. Chunk z -18 spans z -288..-272 and is entirely beyond it. The
    // margin is sixteen blocks, so the extraction's own rounding cannot decide either case.
    let frustum = frame();
    assert!(
        survives(&frustum, 0, 0, -15),
        "the last section inside the far plane was culled"
    );
    assert!(
        !survives(&frustum, 0, 0, -18),
        "a section beyond the far plane was drawn"
    );
}

#[test]
fn a_section_at_a_negative_coordinate_in_view_is_not_culled() {
    let frustum = frame();
    // x -16..0 and z -144..-128: in front of the eye and inside the 70-degree view. The
    // negative arithmetic must not land the box somewhere else.
    assert!(
        survives(&frustum, -1, 0, -9),
        "a section at a negative coordinate that is in view was culled"
    );
    // The same negative chunk column behind the eye is still culled.
    assert!(
        !survives(&frustum, -1, 0, 1),
        "a section behind the camera at negative x was drawn"
    );
}

#[test]
fn a_section_beside_the_frustum_is_culled() {
    // Chunk x 20 spans x 320..336, far outside the horizontal half angle at every depth the
    // box covers.
    assert!(
        !survives(&frame(), 20, 0, -10),
        "a section beside the frustum was drawn"
    );
}

#[test]
fn a_zero_size_box_on_a_plane_boundary_is_visible() {
    let frustum = frame();
    let on_near = Aabb3::new([0.0, 0.0, -NEAR_PLANE], [0.0, 0.0, -NEAR_PLANE]);
    assert!(frustum.intersects(&on_near), "a point on the near plane");
    let on_far = Aabb3::new(
        [0.0, 0.0, -frustum.planes()[5].offset],
        [0.0, 0.0, -frustum.planes()[5].offset],
    );
    assert!(frustum.intersects(&on_far), "a point on the far plane");
    let edge = 100.0 * half_angle().tan();
    let on_side = Aabb3::new([edge, 0.0, -100.0], [edge, 0.0, -100.0]);
    assert!(frustum.intersects(&on_side), "a point on the right plane");
    let outside = Aabb3::new([edge * 1.001, 0.0, -100.0], [edge * 1.001, 0.0, -100.0]);
    assert!(
        !frustum.intersects(&outside),
        "a point past the right plane was kept"
    );
}

#[test]
fn a_zero_size_box_across_the_near_plane_is_split_by_it() {
    let frustum = frame();
    let behind = Aabb3::new([0.0, 0.0, 0.01], [0.0, 0.0, 0.01]);
    assert!(!frustum.intersects(&behind), "a point behind the eye");
    let inside = Aabb3::new(
        [0.0, 0.0, -(NEAR_PLANE + 0.001)],
        [0.0, 0.0, -(NEAR_PLANE + 0.001)],
    );
    assert!(
        frustum.intersects(&inside),
        "a point just past the near plane"
    );
}

#[test]
fn a_section_box_spans_its_sixteen_blocks_exactly() {
    // The corners multiply in integer space and convert afterwards, so a negative coordinate
    // keeps its exact edge: -1 * 16 is -16, not -15.9999.
    assert_eq!(Aabb3::section(-1, 0, -1).min, [-16.0, 0.0, -16.0]);
    assert_eq!(Aabb3::section(-1, 0, -1).max, [0.0, 16.0, 0.0]);
    assert_eq!(Aabb3::section(3, 7, -12).min, [48.0, 112.0, -192.0]);
    assert_eq!(Aabb3::section(3, 7, -12).max, [64.0, 128.0, -176.0]);
    // A far negative chunk column stays exact too: 1000000 * 16 is 16_000_000.
    assert_eq!(
        Aabb3::section(-1_000_000, 15, 1_000_000).min,
        [-16_000_000.0, 240.0, 16_000_000.0]
    );
    assert_eq!(
        Aabb3::section(-1_000_000, 15, 1_000_000).max,
        [-15_999_984.0, 256.0, 16_000_016.0]
    );
    // The centre the translucent order sorts by.
    assert_eq!(Aabb3::section(0, 0, 0).centre(), [8.0, 8.0, 8.0]);
    assert_eq!(Aabb3::section(-1, 3, -1).centre(), [-8.0, 56.0, -8.0]);
}

#[test]
fn the_frustum_matches_the_cameras_own_view_projection() {
    // The camera type's matrix, not a hand-built one: yaw 0 faces south (+Z), so the section
    // five chunks out is in front and the one five chunks behind is culled. This is the
    // row combination the pass derives every frame from `Camera::view_projection`.
    let camera = Camera {
        pose: CameraPose {
            position: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        },
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let frustum = Frustum::from_view_projection(camera.view_projection(16.0 / 9.0));
    assert!(
        survives(&frustum, 0, 3, 5),
        "a section in front of the camera was culled"
    );
    assert!(
        !survives(&frustum, 0, 3, -5),
        "a section behind the camera was drawn"
    );
    assert!(
        survives(&frustum, 0, 4, 0),
        "the section the camera stands in was culled"
    );
}
