//! Tests for the vertex layout and the camera maths. No GPU is involved.

use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE};
use oxide_render::terrain::{VERTEX_BYTES, Vertex, vertex_bytes};

#[test]
fn a_vertex_is_twenty_eight_bytes_of_little_endian_fields() {
    let vertex = Vertex {
        position: [1.0, 2.0, 3.0],
        uv: [0.5, 0.25],
        light: [8, 248],
        colour: [255, 128, 64, 255],
    };
    let bytes = vertex_bytes(&[vertex]);
    assert_eq!(bytes.len(), VERTEX_BYTES);
    assert_eq!(
        bytes,
        vec![
            // The position: three f32, 1.0, 2.0 and 3.0.
            0x00, 0x00, 0x80, 0x3f, // 1.0
            0x00, 0x00, 0x00, 0x40, // 2.0
            0x00, 0x00, 0x40, 0x40, // 3.0
            // The uv: two f32, 0.5 and 0.25.
            0x00, 0x00, 0x00, 0x3f, // 0.5
            0x00, 0x00, 0x80, 0x3e, // 0.25
            // The light: two u16, the block field 8 and the sky field 248.
            0x08, 0x00, // 8
            0xf8, 0x00, // 248
            // The colour: four u8, no padding between the light and it.
            0xff, 0x80, 0x40, 0xff,
        ]
    );
}

#[test]
fn two_vertices_are_twice_the_bytes() {
    let mesh = vec![
        Vertex {
            position: [0.0; 3],
            uv: [0.0; 2],
            light: [0; 2],
            colour: [0; 4],
        },
        Vertex {
            position: [1.0; 3],
            uv: [1.0; 2],
            light: [u16::MAX; 2],
            colour: [255; 4],
        },
    ];
    let bytes = vertex_bytes(&mesh);
    assert_eq!(bytes.len(), 2 * VERTEX_BYTES);
    // The second vertex starts at offset 28 and is the only one holding a non-zero position;
    // its three position floats sit at offsets 28, 32 and 36, and its colour closes the
    // stream at offsets 52..56.
    let read = |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!((read(28), read(32), read(36)), (1.0, 1.0, 1.0));
    assert_eq!(&bytes[52..56], &[255, 255, 255, 255]);
}

fn pose(yaw: f32, pitch: f32) -> CameraPose {
    CameraPose {
        position: [0.0, 64.0, 0.0],
        yaw,
        pitch,
    }
}

#[test]
fn yaw_zero_faces_south_and_ninety_faces_west() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let forward = camera.forward();
    assert!(
        (forward.z - 1.0).abs() < 1e-5,
        "yaw 0 is south: {forward:?}"
    );
    assert!(forward.x.abs() < 1e-5);

    let camera = Camera {
        pose: pose(90.0, 0.0),
        ..camera
    };
    let forward = camera.forward();
    assert!(
        (forward.x + 1.0).abs() < 1e-5,
        "yaw 90 is west: {forward:?}"
    );
}

#[test]
fn positive_pitch_looks_down() {
    let camera = Camera {
        pose: pose(0.0, 90.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    assert!(camera.forward().y < -0.99, "pitch 90 looks down");
}

#[test]
fn the_eye_sits_one_and_a_six_above_the_feet() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    assert!((camera.eye().y - (64.0 + EYE_HEIGHT)).abs() < 1e-5);
}

#[test]
fn a_point_ahead_projects_to_the_centre_and_near_maps_to_zero_depth() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let view_projection = camera.view_projection(16.0 / 9.0);
    // Ten blocks ahead along +Z, at eye height.
    let ahead = view_projection * glam::Vec4::new(0.0, camera.eye().y, 10.0, 1.0);
    let ndc = ahead.truncate() / ahead.w;
    assert!(
        ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4,
        "centre of view: {ndc:?}"
    );
    assert!(
        ndc.z > 0.0 && ndc.z < 1.0,
        "inside the depth range: {ndc:?}"
    );

    // Just beyond the near plane: depth is almost zero. With the near plane at 0.05 and a
    // 0..1 depth range, a sample 0.0001 blocks past the plane lands at roughly 0.002.
    let near_point = view_projection
        * glam::Vec4::new(
            0.0,
            camera.eye().y,
            (NEAR_PLANE as f64 + 0.0001) as f32,
            1.0,
        );
    let near_ndc = near_point.truncate() / near_point.w;
    assert!(near_ndc.z.abs() < 0.01, "near maps to zero: {near_ndc:?}");

    // The far plane is far_chunks * 16 * SQRT_2 = 181.019 blocks away here: a point at that
    // distance reaches depth 1, and a point twice as far lies beyond the plane.
    let far = 8.0 * 16.0 * std::f32::consts::SQRT_2;
    let at_far = view_projection * glam::Vec4::new(0.0, camera.eye().y, far, 1.0);
    let at_far_ndc = at_far.truncate() / at_far.w;
    assert!(
        (at_far_ndc.z - 1.0).abs() < 1e-5,
        "far maps to one: {at_far_ndc:?}"
    );
    let past_far = view_projection * glam::Vec4::new(0.0, camera.eye().y, 2.0 * far, 1.0);
    let past_far_ndc = past_far.truncate() / past_far.w;
    assert!(past_far_ndc.z > 1.0, "beyond far: {past_far_ndc:?}");

    // Yaw 0 faces +Z, so a point one block east of the eye sits on the left of the view: its
    // x lands near -0.080 under a right-handed basis (a mirrored basis would land at +0.080).
    let east = view_projection * glam::Vec4::new(1.0, camera.eye().y, 10.0, 1.0);
    let east_ndc = east.truncate() / east.w;
    assert!(
        (east_ndc.x + 0.080).abs() < 1e-3,
        "the basis is right-handed: {east_ndc:?}"
    );
}

#[test]
fn a_point_behind_the_camera_has_negative_w() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let behind = camera.view_projection(1.0) * glam::Vec4::new(0.0, camera.eye().y, -5.0, 1.0);
    assert!(behind.w < 0.0, "behind the camera clips: {behind:?}");
}
