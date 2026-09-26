//! Tests for the vertex layout and the camera maths. No GPU is involved.

use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE};
use oxide_render::terrain::{VERTEX_BYTES, Vertex, vertex_bytes};

#[test]
fn a_vertex_is_twenty_four_bytes_of_little_endian_floats() {
    let vertex = Vertex {
        position: [1.0, 2.0, 3.0],
        color: [0.5, 0.25, 0.0],
    };
    let bytes = vertex_bytes(&[vertex]);
    assert_eq!(bytes.len(), VERTEX_BYTES);
    let read = |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!((read(0), read(4), read(8)), (1.0, 2.0, 3.0));
    assert_eq!((read(12), read(16), read(20)), (0.5, 0.25, 0.0));
}

#[test]
fn two_vertices_are_twice_the_bytes() {
    let mesh = vec![
        Vertex {
            position: [0.0; 3],
            color: [0.0; 3],
        },
        Vertex {
            position: [1.0; 3],
            color: [1.0; 3],
        },
    ];
    assert_eq!(vertex_bytes(&mesh).len(), 2 * VERTEX_BYTES);
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
    assert!(near_ndc.z < 0.01, "near maps to zero: {near_ndc:?}");
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
