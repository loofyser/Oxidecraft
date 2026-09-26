//! The camera: the server-reported pose, turned into a view and a projection.

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
}

/// The eye height above the feet position, as vanilla uses.
pub const EYE_HEIGHT: f32 = 1.62;
/// Vanilla's default vertical field of view.
pub const DEFAULT_FOV: f32 = 70.0;
/// Vanilla's near plane.
pub const NEAR_PLANE: f32 = 0.05;

impl Camera {
    /// The eye position: the pose's feet position plus [`EYE_HEIGHT`].
    pub fn eye(&self) -> Vec3 {
        Vec3::new(
            self.pose.position[0] as f32,
            self.pose.position[1] as f32 + EYE_HEIGHT,
            self.pose.position[2] as f32,
        )
    }

    /// The unit forward vector for the pose's yaw and pitch.
    pub fn forward(&self) -> Vec3 {
        let yaw = self.pose.yaw.to_radians();
        let pitch = self.pose.pitch.to_radians();
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        Vec3::new(-sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch)
    }

    /// The view matrix.
    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.eye(), self.forward(), Vec3::Y)
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
