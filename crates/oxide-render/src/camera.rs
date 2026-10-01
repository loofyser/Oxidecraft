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
/// The first-person camera's backward offset along the view axis, in blocks.
///
/// The source's first-person camera is translated back along the view axis
/// before the view rotations, so the world is rendered from a point this far
/// behind the eye (`EntityRenderer.orientCamera`'s `thirdPersonView == 0`
/// branch: `GlStateManager.translate(0.0F, 0.0F, -0.1F)`,
/// `EntityRenderer.java:720`).
pub const FIRST_PERSON_OFFSET: f32 = 0.1;

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
    ///
    /// The camera sits [`FIRST_PERSON_OFFSET`] blocks behind the eye on the
    /// view axis, as the source's first-person camera does; the eye's height
    /// and facing are what they are for the pose itself.
    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(
            self.eye() - FIRST_PERSON_OFFSET * self.forward(),
            self.forward(),
            Vec3::Y,
        )
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

#[cfg(test)]
mod tests {
    use super::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE};
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
}
