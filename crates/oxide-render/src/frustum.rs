//! The frame's view frustum and the axis-aligned boxes it culls.
//!
//! [`Frustum::from_view_projection`] extracts the six planes from the matrix the camera
//! uploads, with the Gribb–Hartmann row combination that matches the convention
//! [`Camera::view_projection`](crate::camera::Camera::view_projection) produces: clip space
//! x and y in -1..1, depth in 0..1, right-handed. Each plane's normal is normalised, so
//! [`Plane::signed_distance`] is in world units and points into the visible half-space.
//!
//! [`Frustum::intersects`] is a rejection test: it answers false only when the box lies fully
//! outside one plane, so a box touching a plane — or containing the eye — is never culled.
//! That is the rule the client's own per-section cull follows (`ViewFrustum` /
//! `RenderGlobal.setupTerrain`: a chunk is skipped only when it is entirely outside), and the
//! reason the maths here is tested on synthetic cameras rather than by a read-back: a wrong
//! cull is a visible hole.

use glam::{Mat4, Vec3, Vec4};

/// One plane of the frustum, in world space.
///
/// The normal points into the half-space the frustum keeps, so a point `p` is inside the
/// plane when `normal.dot(p) + offset >= 0`; the normal is unit length, which makes that
/// expression the distance from `p` to the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// The unit normal, pointing at the half-space the frustum keeps.
    pub normal: Vec3,
    /// The plane's offset along its normal.
    pub offset: f32,
}

impl Plane {
    /// The signed distance from `point` to the plane: positive inside the frustum,
    /// negative behind the plane, zero on it.
    pub fn signed_distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.offset
    }
}

/// An axis-aligned box in world space, its `min` corner at or below its `max` corner on
/// every axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb3 {
    /// The corner with the smallest coordinate on every axis.
    pub min: [f32; 3],
    /// The corner with the largest coordinate on every axis.
    pub max: [f32; 3],
}

impl Aabb3 {
    /// A box from two corners.
    pub fn new(min: [f32; 3], max: [f32; 3]) -> Aabb3 {
        Aabb3 { min, max }
    }

    /// The 16-block cube of the section at chunk `x`/`z` and section index `section_y`:
    /// `x * 16 .. x * 16 + 16`, likewise z, and `section_y * 16 .. + 16`.
    ///
    /// The corners multiply in integer space and convert to `f32` afterwards, so a negative
    /// chunk coordinate keeps its exact edge — chunk -1 spans -16.0 .. 0.0, never
    /// -15.999... — and the multiply cannot overflow a 32-bit coordinate, however hostile the
    /// key the wire delivered.
    pub fn section(x: i32, section_y: u8, z: i32) -> Aabb3 {
        let x = i64::from(x) * 16;
        let y = i64::from(section_y) * 16;
        let z = i64::from(z) * 16;
        Aabb3 {
            min: [x as f32, y as f32, z as f32],
            max: [(x + 16) as f32, (y + 16) as f32, (z + 16) as f32],
        }
    }

    /// The box's centre, the point the translucent draw order sorts by.
    pub fn centre(&self) -> [f32; 3] {
        let centre = |axis: usize| (self.min[axis] + self.max[axis]) * 0.5;
        [centre(0), centre(1), centre(2)]
    }
}

/// The frame's six planes, with the names [`Frustum::planes`] documents.
#[derive(Debug, Clone)]
pub struct Frustum {
    /// Left, right, bottom, top, near, far, in that order.
    planes: [Plane; 6],
}

impl Frustum {
    /// Extracts the six planes of `view_projection`.
    ///
    /// A world point is inside the frustum when its clip coordinates are inside the unit
    /// cube, which for the 0..1-depth convention is `0 <= z <= w` and `-w <= x, y <= w`. Each
    /// of those six inequalities is a linear expression in the matrix rows, so the planes are
    /// row combinations of the view-projection: left is `row3 + row0`, right `row3 - row0`,
    /// bottom `row3 + row1`, top `row3 - row1`, near `row2` and far `row3 - row2`. Each
    /// normal is then normalised.
    ///
    /// A degenerate matrix — one whose rows all vanish — yields NaN planes, which reject
    /// every box; no camera this crate builds is degenerate.
    pub fn from_view_projection(view_projection: Mat4) -> Frustum {
        let row0 = view_projection.row(0);
        let row1 = view_projection.row(1);
        let row2 = view_projection.row(2);
        let row3 = view_projection.row(3);
        Frustum {
            planes: [
                plane(row3 + row0),
                plane(row3 - row0),
                plane(row3 + row1),
                plane(row3 - row1),
                plane(row2),
                plane(row3 - row2),
            ],
        }
    }

    /// The six planes, in the order left, right, bottom, top, near, far.
    pub fn planes(&self) -> [Plane; 6] {
        self.planes
    }

    /// Whether `aabb` intersects the frustum.
    ///
    /// False only when the box lies fully outside one of the planes: the test compares each
    /// plane against the box's positive vertex — the corner farthest along the plane's normal
    /// — and a box whose farthest corner is still behind a plane has all eight corners behind
    /// it. A box touching a plane is inside, which keeps a section that merely grazes the
    /// view edge (or holds the eye) drawn.
    pub fn intersects(&self, aabb: &Aabb3) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance(farthest_corner(aabb, plane.normal)) >= 0.0)
    }
}

/// The box's corner farthest along `normal`: the point a plane test must measure.
fn farthest_corner(aabb: &Aabb3, normal: Vec3) -> Vec3 {
    Vec3::new(
        if normal.x >= 0.0 {
            aabb.max[0]
        } else {
            aabb.min[0]
        },
        if normal.y >= 0.0 {
            aabb.max[1]
        } else {
            aabb.min[1]
        },
        if normal.z >= 0.0 {
            aabb.max[2]
        } else {
            aabb.min[2]
        },
    )
}

/// Normalises one extracted plane: its `xyz` is the normal, its `w` the offset.
fn plane(vector: Vec4) -> Plane {
    let normalised = vector / vector.truncate().length();
    Plane {
        normal: normalised.truncate(),
        offset: normalised.w,
    }
}

#[cfg(test)]
mod tests {
    use super::{Aabb3, Frustum, Plane, farthest_corner};
    use glam::{Mat4, Vec3};

    #[test]
    fn a_plane_measures_signed_distance_along_its_normal() {
        let plane = Plane {
            normal: Vec3::Y,
            offset: -3.0,
        };
        assert_eq!(plane.signed_distance(Vec3::new(0.0, 4.0, 0.0)), 1.0);
        assert_eq!(plane.signed_distance(Vec3::new(0.0, 3.0, 0.0)), 0.0);
        assert_eq!(plane.signed_distance(Vec3::new(0.0, 2.0, 0.0)), -1.0);
    }

    #[test]
    fn the_farthest_corner_follows_each_axis_sign() {
        let box3 = Aabb3::new([-1.0, -2.0, -3.0], [1.0, 2.0, 3.0]);
        assert_eq!(
            farthest_corner(&box3, Vec3::new(1.0, 0.0, -1.0)),
            Vec3::new(1.0, 2.0, -3.0)
        );
    }

    #[test]
    fn a_camera_inside_a_box_keeps_it() {
        // The eye's own section spans the camera and must survive every plane.
        let frustum = Frustum::from_view_projection(
            Mat4::perspective_rh(70_f32.to_radians(), 1.0, 0.05, 256.0)
                * Mat4::look_to_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y),
        );
        assert!(frustum.intersects(&Aabb3::section(-1, 0, -1)));
    }
}
