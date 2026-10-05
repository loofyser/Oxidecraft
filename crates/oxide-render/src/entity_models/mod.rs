//! The box-model framework the entity passes draw through.
//!
//! A model is a tree of parts, each hanging from a pivot with a rest rotation and its own box
//! list, in the same 1/16-metre model units and with the same shapes as the client's own tables
//! (`ModelRenderer.java`, `ModelBox.java`). A part's transform composes as `T(point) · Rz · Ry
//! · Rx` (`ModelRenderer.render`) and its children hang inside it; a part whose `showModel`
//! flag is off draws neither its boxes nor its children (`ModelRenderer.render`'s `showModel`
//! gate, which `RenderPlayer.setModelVisibilities` writes from the player's parts byte). A box
//! is emitted as the six quads of `ModelBox`'s table — the source's corner order, its UV
//! arithmetic and its mirror rule, all derived from `ModelBox.java` and `TexturedQuad.java` —
//! each quad with the normal those two classes compute from the quad's own corners. A box with
//! a non-positive dimension emits nothing (`ModelBox.addBox` returns early for one).
//!
//! [`Pose`] is the per-frame input: the shared fields every animated model reads plus a
//! per-model extension the tasks that declare a model also declare. A model's own pose
//! function writes the [`Rot`] slot of each part — its pivot, its angles and whether it draws
//! — over the `rest()` seed; [`build_vertices`] then walks the tree and the slots together and
//! hands the vertex pass its geometry.
//!
//! The registry pairs each draw reference with the model that draws it, the entity class's own
//! height and the shadow quad its renderer draws: the height offsets a nametag, and the shadow
//! pair sizes the quad beneath the feet.

use glam::{Mat4, Vec3};

use crate::entity_pass::ModelRef;

pub mod player;

/// A box of a part: the cuboid `ModelBox` expands into six quads.
///
/// The fields are the arguments of one `ModelRenderer.addBox` call — the corner offsets, the
/// extents, the UV cell on the texture sheet and the inflation `ModelBox` grows every side by
/// — plus the mirror flag `ModelRenderer.addBox(..., boolean)`, which swaps the box's x and
/// reverses every quad's winding (`ModelBox.addBox`). Units are the source's: 1/16 metre, with
/// the origin at the part's pivot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box {
    /// The box's origin corner relative to the part's pivot, in 1/16 model units.
    pub origin: [f32; 3],
    /// The box's extents along x, y and z, in 1/16 model units.
    pub size: [f32; 3],
    /// The box's cell on the texture sheet: the column and row `ModelBox` reads its face UVs
    /// from.
    pub uv: [f32; 2],
    /// The inflation applied to every side (`ModelBox`'s `delta` argument).
    pub inflate: f32,
    /// Whether the box swaps its x corners and reverses its quad windings — the mirrored
    /// variant `ModelRenderer.addBox(..., boolean)` builds.
    pub mirror: bool,
}

/// A part of a model: a pivot, a rest rotation, its boxes and its children.
///
/// The rest rotation seeds the part's [`Rot`] slot; a part with no rest rotation leaves the
/// slot at zero. Children are drawn inside the part's transform, in order, after the part's
/// own boxes (`ModelRenderer.render`).
#[derive(Debug)]
pub struct Part {
    /// The pivot the part turns around, in 1/16 model units (`ModelRenderer.rotationPoint*`).
    pub point: [f32; 3],
    /// The rest rotation in radians, `(x, y, z)` (`ModelRenderer.rotateAngle*`).
    pub rest: [f32; 3],
    /// The boxes the part draws, in the order the tables declare them.
    pub boxes: &'static [Box],
    /// The parts hanging under this one.
    pub children: &'static [Part],
}

/// A model: its parts, in the order the pose's transforms pair with them.
#[derive(Debug)]
pub struct Model {
    /// The model's root parts, in draw order.
    pub parts: &'static [Part],
}

/// One part's pose state: where it hangs, how it is turned and whether it draws.
///
/// The three fields are the three mutable pieces of a `ModelRenderer` a pose function writes:
/// `rotationPointX/Y/Z`, `rotateAngleX/Y/Z` and `showModel`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rot {
    /// The pivot, in 1/16 model units.
    pub point: [f32; 3],
    /// The rotation in radians, `(x, y, z)`.
    pub angles: [f32; 3],
    /// Whether the part (and so its children) draws this frame.
    pub visible: bool,
}

/// The per-model extension a [`Pose`] carries: the inputs a model's own pose reads beyond the
/// shared fields. A model declares its variant with itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PoseExtra {
    /// A model whose pose reads only the shared fields.
    #[default]
    None,
    /// The player model's own: the cape layer's motion term.
    Player(player::CapeMotion),
}

/// The frame's shared pose input.
///
/// The fields are the values the source's `RendererLivingEntity.doRender` derives for the
/// frame and hands to `setRotationAngles`: the limb swing pair already slid by the frame's
/// fraction, the entity's age, the head's yaw relative to the body and its pitch, the body's
/// render yaw, and the flags and ramps the renderer's own branches read. The source measures
/// head yaw and pitch in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
    /// The limb swing's accumulated distance, with the fractional tick applied
    /// (`RendererLivingEntity.java`: `limbSwing - limbSwingAmount * (1 - partialTicks)`).
    pub limb_swing: f32,
    /// The limb swing's eased amount, interpolated over the frame
    /// (`prevLimbSwingAmount + (limbSwingAmount - prevLimbSwingAmount) * partialTicks`).
    pub limb_swing_amount: f32,
    /// The entity's age in ticks, with the frame's fraction (`ticksExisted + partialTicks`).
    pub age: f32,
    /// The head's yaw relative to the body, in degrees (`rotationYawHead` less the render yaw
    /// offset).
    pub head_yaw: f32,
    /// The head's pitch in degrees.
    pub head_pitch: f32,
    /// The body's render yaw in degrees (`renderYawOffset`, interpolated).
    pub body_yaw: f32,
    /// Whether the entity sneaks.
    pub sneak: bool,
    /// The arm swing's progress, `0.0..1.0` — the source's six-tick grid, never reaching
    /// `1.0` (`EntityLivingBase.getSwingProgress`).
    pub swing_progress: f32,
    /// The hurt window's fraction: one while `hurtTime` is open, zero once it has run out.
    pub hurt: f32,
    /// The death ramp's fraction: the source's clamped square root, one at its end
    /// (`RendererLivingEntity.rotateCorpse`).
    pub death: f32,
    /// Whether the entity is a child.
    pub child: bool,
    /// The model's own extension.
    pub extra: PoseExtra,
}

/// The vertex data one [`build_vertices`] call emits: four corners per quad.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vertices {
    /// Quad corners' positions in 1/16 model units.
    pub positions: Vec<[f32; 3]>,
    /// The corners' uvs, parallel to `positions`.
    pub uvs: Vec<[f32; 2]>,
    /// The quads' normals — the source's own cross product, one per corner, constant within a
    /// quad — parallel to `positions`.
    pub normals: Vec<[f32; 3]>,
}

impl Model {
    /// The rest transform of every part, depth first, in the order the parts draw.
    ///
    /// The seed a pose function writes over: the part's own pivot and rest rotation, with
    /// every part drawing.
    pub fn rest(&self) -> Vec<Rot> {
        let mut out = Vec::new();
        for part in self.parts {
            rest_of(part, &mut out);
        }
        out
    }
}

/// Collects one part's rest transform and its children's, depth first.
fn rest_of(part: &Part, out: &mut Vec<Rot>) {
    out.push(Rot {
        point: part.point,
        angles: part.rest,
        visible: true,
    });
    for child in part.children {
        rest_of(child, out);
    }
}

/// Builds the vertices of every visible part of `model`, pairing each part with its
/// transform in `transforms`.
///
/// The pairing is positional: `transforms` holds one [`Rot`] per part of `model`, in
/// [`Model::rest`]'s depth-first order, and the call stops quietly at a part it has no
/// transform for. A part whose transform is not visible emits nothing, children included. The
/// uv values divide through `texture`, the sheet's size in texels — the source's per-renderer
/// texture size, which is why a layer drawn through a sheet of its own (the cape's 64x32)
/// builds separately from the model it belongs to.
pub fn build_vertices(model: &Model, transforms: &[Rot], texture: [f32; 2]) -> Vertices {
    let mut out = Vertices::default();
    let mut cursor = 0;
    for part in model.parts {
        walk(
            part,
            Mat4::IDENTITY,
            transforms,
            &mut cursor,
            texture,
            &mut out,
        );
    }
    out
}

/// Walks one part and its children, composing the parents' transforms in.
fn walk(
    part: &Part,
    parent: Mat4,
    transforms: &[Rot],
    cursor: &mut usize,
    texture: [f32; 2],
    out: &mut Vertices,
) {
    let Some(rot) = transforms.get(*cursor) else {
        return;
    };
    *cursor += 1;
    if !rot.visible {
        return;
    }
    // `ModelRenderer.render`'s own composition: the pivot first, then z, y and x — each turn
    // about its axis, in that order.
    let local = Mat4::from_translation(Vec3::from(rot.point))
        * Mat4::from_rotation_z(rot.angles[2])
        * Mat4::from_rotation_y(rot.angles[1])
        * Mat4::from_rotation_x(rot.angles[0]);
    let world = parent * local;
    for b in part.boxes {
        push_box(b, world, texture, out);
    }
    for child in part.children {
        walk(child, world, transforms, cursor, texture, out);
    }
}

/// Emits one box's six quads.
///
/// The corners and the six faces, with their UV rectangles, are `ModelBox`'s table verbatim:
/// east, west, down, up, north and south, each face's rectangle read off the box's extents.
/// A mirrored box swaps its x corners first and then reverses each face's corner order — the
/// two steps `ModelBox.addBox` takes, in its order, so the UVs stay with the corners as they
/// were assigned. Each face's normal is the source's `(v1 - v2) x (v1 - v0)`, computed from
/// the corners as emitted and turned by the part's own transform.
fn push_box(b: &Box, world: Mat4, texture: [f32; 2], out: &mut Vertices) {
    let (w, h, d) = (b.size[0], b.size[1], b.size[2]);
    if w <= 0.0 || h <= 0.0 || d <= 0.0 {
        return;
    }
    let (u, v) = (b.uv[0], b.uv[1]);
    let mut x1 = b.origin[0] - b.inflate;
    let mut x2 = b.origin[0] + w + b.inflate;
    let y1 = b.origin[1] - b.inflate;
    let y2 = b.origin[1] + h + b.inflate;
    let z1 = b.origin[2] - b.inflate;
    let z2 = b.origin[2] + d + b.inflate;
    if b.mirror {
        core::mem::swap(&mut x1, &mut x2);
    }
    // The eight corners under `ModelBox`'s own names: p1..p7 and p.
    let a = [x1, y1, z1];
    let c = [x2, y2, z1];
    let dd = [x1, y2, z1];
    let e = [x1, y1, z2];
    let f = [x2, y1, z2];
    let g = [x2, y2, z2];
    let hh = [x1, y2, z2];
    let bb = [x2, y1, z1];
    // The six faces, in the table's order, with each face's UV rectangle: `u1, v1, u2, v2`.
    let quads: [([[f32; 3]; 4], [f32; 4]); 6] = [
        ([f, bb, c, g], [u + d, v + d, u + d + w, v + d + h]),
        ([a, e, hh, dd], [u, v + d, u + d, v + d + h]),
        ([f, e, a, bb], [u + d, v, u + d + w, v + d]),
        ([c, dd, hh, g], [u + d + w, v + d, u + d + w + w, v]),
        ([bb, a, dd, c], [u + d, v + d, u + d + w, v + d + h]),
        (
            [e, f, g, hh],
            [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
        ),
    ];
    for (corners, rect) in quads {
        // `TexturedQuad`'s constructor assigns the rectangle's corners to the quad's corners
        // in its own order — (u2, v1), (u1, v1), (u1, v2), (u2, v2) — and `ModelBox`'s mirror
        // step then reverses the array, carrying each corner's uv with it.
        let [cu1, cv1, cu2, cv2] = rect;
        let uvs = [
            [cu2 / texture[0], cv1 / texture[1]],
            [cu1 / texture[0], cv1 / texture[1]],
            [cu1 / texture[0], cv2 / texture[1]],
            [cu2 / texture[0], cv2 / texture[1]],
        ];
        let corners: [[f32; 3]; 4] = if b.mirror {
            [corners[3], corners[2], corners[1], corners[0]]
        } else {
            corners
        };
        let uvs = if b.mirror {
            [uvs[3], uvs[2], uvs[1], uvs[0]]
        } else {
            uvs
        };
        // The face's normal, the source's cross product of the corners as they stand.
        let v1 = Vec3::from(corners[1]);
        let v0 = Vec3::from(corners[0]);
        let v2 = Vec3::from(corners[2]);
        let normal = (v1 - v2).cross(v1 - v0).normalize_or_zero();
        let normal = world.transform_vector3(normal);
        for i in 0..4 {
            out.positions
                .push(world.transform_point3(Vec3::from(corners[i])).into());
            out.uvs.push(uvs[i]);
            out.normals.push(normal.into());
        }
    }
}

/// The model a draw's reference names.
pub fn model_for(reference: ModelRef) -> &'static Model {
    match reference {
        ModelRef::Player { slim, .. } => {
            if slim {
                &player::MODEL_PLAYER_SLIM
            } else {
                &player::MODEL_PLAYER_WIDE
            }
        }
    }
}

/// The drawn entity class's own height in blocks: the size constant its class sets
/// (`EntityPlayer` sets `0.6` by `1.8`), which the nametag offset stands on.
pub fn height(reference: ModelRef) -> f32 {
    match reference {
        ModelRef::Player { .. } => 1.8,
    }
}

/// The shadow quad's size in blocks and its opacity factor, per the class's renderer: the
/// player's renderer is built with `0.5F`, and the base renderer's opacity default is one.
pub fn shadow(reference: ModelRef) -> [f32; 2] {
    match reference {
        ModelRef::Player { .. } => [0.5, 1.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_pass::ModelRef;
    use std::f32::consts::PI;

    /// A 4x4x4 cube at the origin, for the transform cases.
    static CUBE: [Box; 1] = [Box {
        origin: [0.0, 0.0, 0.0],
        size: [4.0, 4.0, 4.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }];

    /// One part drawing the cube.
    static PLAIN_PARTS: [Part; 1] = [Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &CUBE,
        children: &[],
    }];

    /// The one-part, one-cube model.
    static PLAIN_MODEL: Model = Model {
        parts: &PLAIN_PARTS,
    };

    /// The 1x2x3 box at the origin with the UV cell (5, 7).
    static SMALL: [Box; 1] = [Box {
        origin: [0.0, 0.0, 0.0],
        size: [1.0, 2.0, 3.0],
        uv: [5.0, 7.0],
        inflate: 0.0,
        mirror: false,
    }];

    /// The same box mirrored.
    static SMALL_MIRRORED: [Box; 1] = [Box {
        mirror: true,
        ..SMALL[0]
    }];

    /// One-part models over the small boxes.
    static SMALL_MODEL: Model = Model {
        parts: &[Part {
            point: [0.0, 0.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &SMALL,
            children: &[],
        }],
    };
    static SMALL_MIRRORED_MODEL: Model = Model {
        parts: &[Part {
            point: [0.0, 0.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &SMALL_MIRRORED,
            children: &[],
        }],
    };

    /// A 16x16 sheet for the integer literals.
    const SHEET: [f32; 2] = [16.0, 16.0];

    /// The identity transform for one part.
    fn identity() -> Vec<Rot> {
        vec![Rot {
            point: [0.0, 0.0, 0.0],
            angles: [0.0, 0.0, 0.0],
            visible: true,
        }]
    }

    /// Whether two corners agree to within a quarter of a texel — the turns below are exact
    /// only up to a float's own rounding.
    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1.0e-4)
    }

    #[test]
    fn a_part_at_a_pivot_turned_ninety_degrees_maps_the_box() {
        let transforms = [Rot {
            point: [2.0, 12.0, 2.0],
            angles: [PI / 2.0, 0.0, 0.0],
            visible: true,
        }];
        let vertices = build_vertices(&PLAIN_MODEL, &transforms, [64.0, 64.0]);
        assert_eq!(vertices.positions.len(), 24);
        assert_eq!(vertices.uvs.len(), 24);
        assert_eq!(vertices.normals.len(), 24);
        // The east face's first corner, (x2, y1, z2) = (4, 0, 4), a quarter of a turn about
        // X around the pivot (2, 12, 2).
        assert!(close(vertices.positions[0], [6.0, 8.0, 2.0]));
        // The north face's first corner, (0, 0, 0): the pivot itself.
        assert!(close(vertices.positions[4], [2.0, 12.0, 2.0]));
        // The south face's last corner, (0, 4, 4), where the turn leaves it.
        assert!(close(vertices.positions[23], [2.0, 8.0, 6.0]));
        // The east face's normal survives the turn about X; the down face's turns to -Z.
        assert!(close(vertices.normals[0], [1.0, 0.0, 0.0]));
        assert!(close(vertices.normals[8], [0.0, 0.0, -1.0]));
    }

    #[test]
    fn a_child_inherits_its_parents_transform() {
        static NESTED_CHILDREN: [Part; 1] = [Part {
            point: [0.0, 8.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &CUBE,
            children: &[],
        }];
        static NESTED_PARTS: [Part; 1] = [Part {
            point: [2.0, 12.0, 2.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[],
            children: &NESTED_CHILDREN,
        }];
        static NESTED: Model = Model {
            parts: &NESTED_PARTS,
        };
        let transforms = [
            Rot {
                point: [2.0, 12.0, 2.0],
                angles: [0.0, PI / 2.0, 0.0],
                visible: true,
            },
            Rot {
                point: [0.0, 8.0, 0.0],
                angles: [PI / 2.0, 0.0, 0.0],
                visible: true,
            },
        ];
        let vertices = build_vertices(&NESTED, &transforms, [64.0, 64.0]);
        // The child's east-face first corner (4, 0, 4): a quarter about X around the child's
        // own pivot (0, 8, 0), then the parent's quarter about Y around (2, 12, 2).
        assert_eq!(vertices.positions.len(), 24);
        assert!(
            close(vertices.positions[0], [2.0, 16.0, -2.0]),
            "child first corner: {:?}",
            vertices.positions[0]
        );
    }

    #[test]
    fn the_box_builder_emits_the_uvs_and_positions_of_the_source() {
        let vertices = build_vertices(&SMALL_MODEL, &identity(), SHEET);
        let uv = |u: f32, v: f32| [u / 16.0, v / 16.0];
        let expected_positions: [[f32; 3]; 24] = [
            // East, the (u + d, v + d) cell.
            [1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0],
            [1.0, 2.0, 0.0],
            [1.0, 2.0, 3.0],
            // West, the (u, v + d) cell.
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 3.0],
            [0.0, 2.0, 3.0],
            [0.0, 2.0, 0.0],
            // Down, the (u + d, v) cell.
            [1.0, 0.0, 3.0],
            [0.0, 0.0, 3.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            // Up, the (u + d + w, v + d) cell.
            [1.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 2.0, 3.0],
            [1.0, 2.0, 3.0],
            // North, the (u + d, v + d) cell again.
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 2.0, 0.0],
            // South, the (u + d + w + d, v + d) cell.
            [0.0, 0.0, 3.0],
            [1.0, 0.0, 3.0],
            [1.0, 2.0, 3.0],
            [0.0, 2.0, 3.0],
        ];
        let expected_uvs: [[f32; 2]; 24] = [
            uv(9.0, 10.0),
            uv(8.0, 10.0),
            uv(8.0, 12.0),
            uv(9.0, 12.0),
            uv(8.0, 10.0),
            uv(5.0, 10.0),
            uv(5.0, 12.0),
            uv(8.0, 12.0),
            uv(9.0, 7.0),
            uv(8.0, 7.0),
            uv(8.0, 10.0),
            uv(9.0, 10.0),
            uv(10.0, 10.0),
            uv(9.0, 10.0),
            uv(9.0, 7.0),
            uv(10.0, 7.0),
            uv(9.0, 10.0),
            uv(8.0, 10.0),
            uv(8.0, 12.0),
            uv(9.0, 12.0),
            uv(13.0, 10.0),
            uv(12.0, 10.0),
            uv(12.0, 12.0),
            uv(13.0, 12.0),
        ];
        assert_eq!(vertices.positions, expected_positions);
        assert_eq!(vertices.uvs, expected_uvs);
    }

    #[test]
    fn a_mirrored_box_flips_its_quads_and_swaps_its_xs() {
        let vertices = build_vertices(&SMALL_MIRRORED_MODEL, &identity(), SHEET);
        let uv = |u: f32, v: f32| [u / 16.0, v / 16.0];
        let expected_positions: [[f32; 3]; 24] = [
            [0.0, 2.0, 3.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 3.0],
            [1.0, 2.0, 0.0],
            [1.0, 2.0, 3.0],
            [1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 3.0],
            [0.0, 0.0, 3.0],
            [0.0, 2.0, 3.0],
            [1.0, 2.0, 3.0],
            [1.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 2.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            [0.0, 2.0, 3.0],
            [0.0, 0.0, 3.0],
            [1.0, 0.0, 3.0],
        ];
        let expected_uvs: [[f32; 2]; 24] = [
            uv(9.0, 12.0),
            uv(8.0, 12.0),
            uv(8.0, 10.0),
            uv(9.0, 10.0),
            uv(8.0, 12.0),
            uv(5.0, 12.0),
            uv(5.0, 10.0),
            uv(8.0, 10.0),
            uv(9.0, 10.0),
            uv(8.0, 10.0),
            uv(8.0, 7.0),
            uv(9.0, 7.0),
            uv(10.0, 7.0),
            uv(9.0, 7.0),
            uv(9.0, 10.0),
            uv(10.0, 10.0),
            uv(9.0, 12.0),
            uv(8.0, 12.0),
            uv(8.0, 10.0),
            uv(9.0, 10.0),
            uv(13.0, 12.0),
            uv(12.0, 12.0),
            uv(12.0, 10.0),
            uv(13.0, 10.0),
        ];
        assert_eq!(vertices.positions, expected_positions);
        assert_eq!(vertices.uvs, expected_uvs);
    }

    #[test]
    fn an_inflated_box_grows_on_every_axis() {
        static HAT: [Box; 1] = [Box {
            origin: [-4.0, -8.0, -4.0],
            size: [8.0, 8.0, 8.0],
            uv: [32.0, 0.0],
            inflate: 0.5,
            mirror: false,
        }];
        static HAT_MODEL: Model = Model {
            parts: &[Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &HAT,
                children: &[],
            }],
        };
        let vertices = build_vertices(&HAT_MODEL, &identity(), [64.0, 32.0]);
        // The overlay's east face reaches half a unit past the head on every axis.
        assert_eq!(vertices.positions[0], [4.5, -8.5, 4.5]);
    }

    #[test]
    fn a_degenerate_box_emits_nothing() {
        static FLAT: [Box; 1] = [Box {
            origin: [0.0, 0.0, 0.0],
            size: [4.0, 0.0, 4.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }];
        static FLAT_MODEL: Model = Model {
            parts: &[Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &FLAT,
                children: &[],
            }],
        };
        let vertices = build_vertices(&FLAT_MODEL, &identity(), [64.0, 64.0]);
        assert!(vertices.positions.is_empty());
        assert!(vertices.uvs.is_empty());
        assert!(vertices.normals.is_empty());
    }

    #[test]
    fn a_hidden_part_takes_its_children_with_it() {
        static HIDDEN_CHILDREN: [Part; 1] = [Part {
            point: [0.0, 0.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &CUBE,
            children: &[],
        }];
        static PARENT_PARTS: [Part; 1] = [Part {
            point: [0.0, 0.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[],
            children: &HIDDEN_CHILDREN,
        }];
        static PARENT: Model = Model {
            parts: &PARENT_PARTS,
        };
        let mut transforms = PARENT.rest();
        transforms[0].visible = false;
        let vertices = build_vertices(&PARENT, &transforms, [64.0, 64.0]);
        assert!(vertices.positions.is_empty());
    }

    #[test]
    fn the_registry_pairs_the_player_with_its_height_and_models() {
        let wide = ModelRef::Player {
            slim: false,
            parts: 0x7F,
        };
        let slim = ModelRef::Player {
            slim: true,
            parts: 0x7F,
        };
        assert_eq!(height(wide), 1.8);
        assert_eq!(height(slim), 1.8);
        assert!(std::ptr::eq(
            model_for(wide),
            &crate::entity_models::player::MODEL_PLAYER_WIDE
        ));
        assert!(std::ptr::eq(
            model_for(slim),
            &crate::entity_models::player::MODEL_PLAYER_SLIM
        ));
        assert_eq!(shadow(wide), [0.5, 1.0]);
    }
}
