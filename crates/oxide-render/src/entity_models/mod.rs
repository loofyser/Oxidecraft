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

pub mod layers;
pub mod player;
pub mod quadrupeds;

pub mod bipeds;

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
#[derive(Debug, Clone, Copy)]
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
/// The fields are the mutable pieces of a `ModelRenderer` a pose function writes:
/// `rotationPointX/Y/Z`, `rotateAngleX/Y/Z`, `offsetX/Y/Z` and `showModel`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rot {
    /// The pivot, in 1/16 model units.
    pub point: [f32; 3],
    /// The rotation in radians, `(x, y, z)`.
    pub angles: [f32; 3],
    /// The part's own offset, `ModelRenderer.render`'s `offsetX/Y/Z`, in the source's own
    /// value: it translates by the offset unscaled where its pivot is weighted a sixteenth
    /// of a block, so the offset is in blocks — the local step weights it sixteenfold into
    /// the model's units. It slides the part, its boxes and its children before the pivot
    /// turn and leaves the offset behind after them (`ModelRenderer.render`:137,`:202`).
    pub offset: [f32; 3],
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
    /// The skeleton's own: whether the class is the wither type, whose living-animation rule
    /// turns on the aim pose (`ModelSkeleton.setLivingAnimations`:43).
    Skeleton {
        /// Whether the skeleton type is the wither one (`getSkeletonType() == 1`).
        aimed_bow: bool,
    },
    /// The witch's own: the held-item gate for the nose's hold state and the entity's id,
    /// which the nose's idle sway reads (`ModelWitch.setRotationAngles`:51-60).
    Witch {
        /// Whether the witch holds an item (`getHeldItem() != null`).
        holding: bool,
        /// The entity's own id, the sway's seed (`getEntityId() % 10`).
        entity_id: i32,
    },
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
        offset: [0.0; 3],
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
    // `ModelRenderer.render`'s own composition: the part's offset first — the source's
    // value in blocks, weighted sixteenfold into the model's units — then the pivot and
    // the turns z, y and x, each turn about its axis, in that order.
    let local = Mat4::from_translation(Vec3::from(rot.offset) * 16.0)
        * Mat4::from_translation(Vec3::from(rot.point))
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
        ([f, bb, c, g], [u + d + w, v + d, u + d + w + d, v + d + h]),
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

/// The player renderer's own pre-render scale (`RenderPlayer.preRenderCallback`), shared by
/// the villager's and the witch's (`RenderVillager.preRenderCallback`:62-74,
/// `RenderWitch.preRenderCallback`:47-48): the source's `0.9375F`, the default every
/// pre-render callback starts from.
pub const RENDER_SCALE: f32 = 0.9375;

/// The drop a sneaking player's position takes, in blocks (`RenderPlayer.doRender`'s
/// `-0.125`): the player's renderer alone drops the position; a mob renderer does not.
pub const SNEAK_POSITION_DROP: f32 = 0.125;

/// The model's own lift while it sneaks, in the model's pre-scale blocks
/// (`ModelBiped.render`:107-110's `translate(0, 0.2, 0)`, inherited by every biped subclass;
/// `ModelVillager`, `ModelIronGolem`, `ModelSnowMan` and the quadrupeds add none).
pub const SNEAK_MODEL_LIFT: f32 = 0.2;

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
        ModelRef::Zombie | ModelRef::Giant => &bipeds::MODEL_ZOMBIE,
        ModelRef::ZombieVillager => &bipeds::MODEL_ZOMBIE_VILLAGER,
        ModelRef::Skeleton => &bipeds::MODEL_SKELETON,
        ModelRef::Villager { .. } => &bipeds::MODEL_VILLAGER,
        ModelRef::Witch => &bipeds::MODEL_WITCH,
        ModelRef::SnowGolem => &bipeds::MODEL_SNOW_GOLEM,
        ModelRef::IronGolem => &bipeds::MODEL_IRON_GOLEM,
        ModelRef::Pig { .. } => &quadrupeds::MODEL_PIG,
        ModelRef::Cow | ModelRef::Mooshroom => &quadrupeds::MODEL_COW,
        ModelRef::Sheep { .. } => &quadrupeds::MODEL_SHEEP,
    }
}

/// The sheet size the model's uvs divide through: the `setTextureSize` call of the model
/// class the reference names (`ModelBase`'s own `64` by `32` default for the classes that
/// never set one).
pub fn texture_size(reference: ModelRef) -> [f32; 2] {
    match reference {
        ModelRef::Player { .. } => player::PLAYER_TEXTURE_SIZE,
        ModelRef::Zombie | ModelRef::ZombieVillager | ModelRef::Giant => [64.0, 64.0],
        ModelRef::Skeleton => [64.0, 32.0],
        ModelRef::Villager { .. } => [64.0, 64.0],
        ModelRef::Witch => [64.0, 128.0],
        ModelRef::SnowGolem => [64.0, 64.0],
        ModelRef::IronGolem => [128.0, 128.0],
        ModelRef::Pig { .. } | ModelRef::Cow | ModelRef::Sheep { .. } | ModelRef::Mooshroom => {
            [64.0, 32.0]
        }
    }
}

/// The sheets the model draws with, in draw order: its own sheet and then its layers', by the
/// pass's registry keys.
///
/// The player is absent: a player's sheet resolves through the skin registry by profile, not
/// by a key.
pub fn textures(reference: ModelRef) -> &'static [&'static str] {
    match reference {
        ModelRef::Player { .. } => &[],
        ModelRef::Zombie | ModelRef::Giant => &["entity/zombie/zombie.png"],
        ModelRef::ZombieVillager => &["entity/zombie/zombie_villager.png"],
        ModelRef::Skeleton => &["entity/skeleton/skeleton.png"],
        ModelRef::Villager { profession, .. } => match profession {
            0 => &["entity/villager/farmer.png"],
            1 => &["entity/villager/librarian.png"],
            2 => &["entity/villager/priest.png"],
            3 => &["entity/villager/smith.png"],
            4 => &["entity/villager/butcher.png"],
            _ => &["entity/villager/villager.png"],
        },
        ModelRef::Witch => &["entity/witch.png"],
        ModelRef::SnowGolem => &["entity/snowman.png"],
        ModelRef::IronGolem => &["entity/iron_golem.png"],
        ModelRef::Pig { .. } => &["entity/pig/pig.png", "entity/pig/pig_saddle.png"],
        ModelRef::Cow => &["entity/cow/cow.png"],
        ModelRef::Sheep { .. } => &["entity/sheep/sheep.png", "entity/sheep/sheep_fur.png"],
        ModelRef::Mooshroom => &["entity/cow/mooshroom.png"],
    }
}

/// The model's pose for a draw, dispatched to the model class the reference names.
pub fn pose(reference: ModelRef, pose: &Pose, out: &mut [Rot]) {
    match reference {
        ModelRef::Player { parts, .. } => player::pose(pose, parts & !player::PART_CAPE, out),
        ModelRef::Zombie | ModelRef::Giant | ModelRef::ZombieVillager => {
            bipeds::pose_zombie(pose, out);
        }
        ModelRef::Skeleton => bipeds::pose_skeleton(pose, out),
        ModelRef::Villager { .. } => bipeds::pose_villager(pose, out),
        ModelRef::Witch => bipeds::pose_witch(pose, out),
        ModelRef::SnowGolem => bipeds::pose_snow_golem(pose, out),
        ModelRef::IronGolem => bipeds::pose_iron_golem(pose, out),
        ModelRef::Pig { .. } => quadrupeds::pose_pig(pose, out),
        ModelRef::Cow | ModelRef::Mooshroom => quadrupeds::pose_quadruped(pose, out),
        ModelRef::Sheep { .. } => quadrupeds::pose_sheep(pose, out),
    }
}

/// The drawn entity class's own height in blocks: the size constant its class sets — the
/// zombie's `0.6` by `1.95` (`EntityZombie.java`:79), the skeleton's (`EntitySkeleton.java`:388),
/// the witch's (`EntityWitch.java`:47), the villager's `1.8` (`EntityVillager.java`:108), the
/// giant's sixfold `1.8` (`EntityGiantZombie.java`:12 over `Entity`'s own size), the snow
/// golem's `1.9` (`EntitySnowman.java`:29), the iron golem's `2.9` (`EntityIronGolem.java`:48)
/// and the quadrupeds' `0.9`/`1.3` (`EntityPig.java`:35, `EntityCow.java`:27) — which the
/// nametag offset stands on.
pub fn height(reference: ModelRef) -> f32 {
    match reference {
        ModelRef::Player { .. } => 1.8,
        ModelRef::Zombie | ModelRef::ZombieVillager | ModelRef::Witch => 1.95,
        ModelRef::Skeleton => 1.95,
        ModelRef::Villager { child, .. } => {
            // The child's own scale half (`EntityAgeable.java`:229-231): 1.8 becomes 0.9.
            if child { 0.9 } else { 1.8 }
        }
        ModelRef::Giant => 10.8,
        ModelRef::SnowGolem => 1.9,
        ModelRef::IronGolem => 2.9,
        ModelRef::Pig { .. } => 0.9,
        ModelRef::Cow | ModelRef::Sheep { .. } | ModelRef::Mooshroom => 1.3,
    }
}

/// The shadow quad's size in blocks and its opacity factor, per the class's renderer: the
/// size each renderer is registered with (`RenderManager.java`:142-166 — `0.7F` for the
/// quadrupeds, `0.5F` for the bipeds; the giant's is its `0.5F` grown by its sixfold scale,
/// `RenderGiantZombie`'s constructor) and the child villager's own `0.25F`
/// (`RenderVillager.preRenderCallback`:67); the base renderer's opacity default is one.
pub fn shadow(reference: ModelRef) -> [f32; 2] {
    match reference {
        ModelRef::Player { .. }
        | ModelRef::Zombie
        | ModelRef::ZombieVillager
        | ModelRef::Skeleton
        | ModelRef::Witch
        | ModelRef::SnowGolem
        | ModelRef::IronGolem => [0.5, 1.0],
        ModelRef::Villager { child, .. } => {
            if child {
                [0.25, 1.0]
            } else {
                [0.5, 1.0]
            }
        }
        ModelRef::Giant => [3.0, 1.0],
        ModelRef::Pig { .. } | ModelRef::Cow | ModelRef::Sheep { .. } | ModelRef::Mooshroom => {
            [0.7, 1.0]
        }
    }
}

/// The pre-render scale the class scales its model by: [`RENDER_SCALE`] for the player, the
/// villager and the witch (`RenderPlayer`'s, `RenderVillager`'s and `RenderWitch`'s own
/// callbacks; the mob renderers off `RenderLiving` never scale), the villager child's own
/// half of it (`RenderVillager.preRenderCallback`:66), and the giant's sixfold one
/// (`RenderGiantZombie.preRenderCallback`:44, registered with `6.0F` at
/// `RenderManager.java`:162).
pub fn render_scale(reference: ModelRef) -> f32 {
    match reference {
        ModelRef::Player { .. } | ModelRef::Witch => RENDER_SCALE,
        ModelRef::Villager { child, .. } => {
            if child {
                RENDER_SCALE * 0.5
            } else {
                RENDER_SCALE
            }
        }
        ModelRef::Giant => 6.0,
        _ => 1.0,
    }
}

/// The sneak terms a model's render path carries, in blocks: the position drop the renderer
/// takes and the lift the model itself adds.
///
/// The player's renderer drops the position an eighth (`RenderPlayer.doRender`) and the model
/// lifts [`SNEAK_MODEL_LIFT`] (`ModelBiped.render`); the mob bipeds that inherit
/// `ModelBiped.render` lift but do not drop, and the classes off `ModelBiped` do neither.
pub fn sneak_terms(reference: ModelRef) -> [f32; 2] {
    match reference {
        ModelRef::Player { .. } => [SNEAK_POSITION_DROP, SNEAK_MODEL_LIFT],
        ModelRef::Zombie | ModelRef::ZombieVillager | ModelRef::Skeleton | ModelRef::Giant => {
            [0.0, SNEAK_MODEL_LIFT]
        }
        _ => [0.0, 0.0],
    }
}

/// The extra roll the class's renderer turns into `rotateCorpse`, in degrees, after the death
/// tilt: the iron golem's walking lean (`RenderIronGolem.rotateCorpse`:31-37 — the folded
/// thirteen-tick wave of its limb pair, `6.5` degrees at its full swing, only while the walk
/// is running). Every other class adds none.
pub fn corpse_roll(reference: ModelRef, pose: &Pose) -> f32 {
    match reference {
        ModelRef::IronGolem if pose.limb_swing_amount >= 0.01 => {
            6.5 * folded_wave(pose.limb_swing + 6.0, 13.0)
        }
        _ => 0.0,
    }
}

/// The folded wave `ModelIronGolem.func_78172_a` folds a value by: a saw-tooth about a
/// period, one at the fold's peaks and minus one at its trough —
/// `(|value % period - period * 0.5| - period * 0.25) / (period * 0.25)`.
pub fn folded_wave(value: f32, period: f32) -> f32 {
    ((value % period - period * 0.5).abs() - period * 0.25) / (period * 0.25)
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
            offset: [0.0; 3],
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
            offset: [0.0; 3],
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
                offset: [0.0; 3],
                visible: true,
            },
            Rot {
                point: [0.0, 8.0, 0.0],
                angles: [PI / 2.0, 0.0, 0.0],
                offset: [0.0; 3],
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
            // East, the (u + d + w, v + d) cell.
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
            uv(12.0, 10.0),
            uv(9.0, 10.0),
            uv(9.0, 12.0),
            uv(12.0, 12.0),
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
            uv(12.0, 12.0),
            uv(9.0, 12.0),
            uv(9.0, 10.0),
            uv(12.0, 10.0),
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

    #[test]
    fn the_mob_registry_pairs_each_kind_with_its_model_height_and_sheet() {
        // The heights are each class's own `setSize` (`EntityZombie.java`:79, the skeleton
        // type 0 at `EntitySkeleton.java`:388, `EntityWitch.java`:47, `EntityVillager.java`:108,
        // `EntityIronGolem.java`:48, `EntitySnowman.java`:29, `EntityPig.java`:35,
        // `EntityCow.java`:27, `EntitySheep.java`:66) and the giant's sixfold scale over the
        // base class's default (`EntityGiantZombie.java`:12, `EntityMob`'s 0.6x1.8).
        let cases = [
            (ModelRef::Zombie, 1.95, [0.5, 1.0], [64.0, 64.0]),
            (ModelRef::ZombieVillager, 1.95, [0.5, 1.0], [64.0, 64.0]),
            (ModelRef::Skeleton, 1.95, [0.5, 1.0], [64.0, 32.0]),
            (
                ModelRef::Villager {
                    profession: 0,
                    child: false,
                },
                1.8,
                [0.5, 1.0],
                [64.0, 64.0],
            ),
            (
                ModelRef::Villager {
                    profession: 0,
                    child: true,
                },
                0.9,
                [0.25, 1.0],
                [64.0, 64.0],
            ),
            (ModelRef::Witch, 1.95, [0.5, 1.0], [64.0, 128.0]),
            (ModelRef::Giant, 10.8, [3.0, 1.0], [64.0, 64.0]),
            (ModelRef::SnowGolem, 1.9, [0.5, 1.0], [64.0, 64.0]),
            (ModelRef::IronGolem, 2.9, [0.5, 1.0], [128.0, 128.0]),
            (
                ModelRef::Pig { saddle: false },
                0.9,
                [0.7, 1.0],
                [64.0, 32.0],
            ),
            (ModelRef::Cow, 1.3, [0.7, 1.0], [64.0, 32.0]),
            (
                ModelRef::Sheep {
                    wool: 0,
                    sheared: false,
                },
                1.3,
                [0.7, 1.0],
                [64.0, 32.0],
            ),
            (ModelRef::Mooshroom, 1.3, [0.7, 1.0], [64.0, 32.0]),
        ];
        for (reference, height_wanted, shadow_wanted, size_wanted) in cases {
            assert_eq!(height(reference), height_wanted, "{reference:?}'s height");
            assert_eq!(shadow(reference), shadow_wanted, "{reference:?}'s shadow");
            assert_eq!(
                texture_size(reference),
                size_wanted,
                "{reference:?}'s sheet size"
            );
        }
        // The giant draws the zombie's own model six times over.
        assert!(std::ptr::eq(
            model_for(ModelRef::Giant),
            &crate::entity_models::bipeds::MODEL_ZOMBIE
        ));
        // The mooshroom is the cow's model (`RenderMooshroom`'s `ModelCow`, line 9).
        assert!(std::ptr::eq(
            model_for(ModelRef::Mooshroom),
            &crate::entity_models::quadrupeds::MODEL_COW
        ));
    }

    #[test]
    fn the_mob_textures_name_the_sources_sheets() {
        // Each key is the class's own resource location, `textures/` dropped for the
        // registry's namespace: `RenderZombie.java`:18-19, `RenderSkeleton.java`:12,
        // `RenderVillager.java`:11-16, `RenderWitch.java`:11, `RenderGiantZombie.java`:13,
        // `RenderSnowMan.java`:10, `RenderIronGolem.java`:11, `RenderPig.java`:10,
        // `RenderCow.java`:9, `RenderSheep.java`:10, `RenderMooshroom.java`:10.
        assert_eq!(textures(ModelRef::Zombie), ["entity/zombie/zombie.png"]);
        assert_eq!(
            textures(ModelRef::ZombieVillager),
            ["entity/zombie/zombie_villager.png"]
        );
        assert_eq!(
            textures(ModelRef::Skeleton),
            ["entity/skeleton/skeleton.png"]
        );
        assert_eq!(textures(ModelRef::Witch), ["entity/witch.png"]);
        // The giant has no sheet of its own: it draws `RenderGiantZombie`'s zombie sheet
        // (line 13), not the `entity/giant.png` a quick guess would name.
        assert_eq!(textures(ModelRef::Giant), ["entity/zombie/zombie.png"]);
        assert_eq!(textures(ModelRef::SnowGolem), ["entity/snowman.png"]);
        assert_eq!(textures(ModelRef::IronGolem), ["entity/iron_golem.png"]);
        assert_eq!(textures(ModelRef::Cow), ["entity/cow/cow.png"]);
        assert_eq!(textures(ModelRef::Mooshroom), ["entity/cow/mooshroom.png"]);
        // The layered kinds carry their layers' sheets beside the base one: the saddle and
        // the fur come from `LayerSaddle.java`:10 and `LayerSheepWool.java`:12.
        assert_eq!(
            textures(ModelRef::Pig { saddle: true }),
            ["entity/pig/pig.png", "entity/pig/pig_saddle.png"]
        );
        assert_eq!(
            textures(ModelRef::Sheep {
                wool: 0,
                sheared: false
            }),
            ["entity/sheep/sheep.png", "entity/sheep/sheep_fur.png"]
        );
        // The professions pick their sheets (`RenderVillager.getEntityTexture`:32-56); any
        // value off the wire falls back to the plain villager sheet, the source's default.
        let sheet = |profession| match textures(ModelRef::Villager {
            profession,
            child: false,
        })[0]
        {
            "entity/villager/farmer.png" => 0,
            "entity/villager/librarian.png" => 1,
            "entity/villager/priest.png" => 2,
            "entity/villager/smith.png" => 3,
            "entity/villager/butcher.png" => 4,
            "entity/villager/villager.png" => 5,
            other => panic!("unexpected villager sheet {other}"),
        };
        assert_eq!(
            [
                sheet(0),
                sheet(1),
                sheet(2),
                sheet(3),
                sheet(4),
                sheet(5),
                sheet(255)
            ],
            [0, 1, 2, 3, 4, 5, 5]
        );
    }

    #[test]
    fn the_sneak_terms_and_corpse_rolls_are_the_kinds_own() {
        // The player's renderer drops a sneak's eighth (`RenderPlayer.doRender`) and the
        // biped models lift the model a fifth (`ModelBiped.render`); the mob models off
        // `ModelBase` carry no sneak term at all — `ModelVillager` and `ModelWitch` never
        // read `isSneak` — and the quadrupeds take neither.
        let player = ModelRef::Player {
            slim: false,
            parts: 0x7F,
        };
        assert_eq!(sneak_terms(player), [0.125, 0.2]);
        assert_eq!(sneak_terms(ModelRef::Zombie), [0.0, 0.2]);
        assert_eq!(sneak_terms(ModelRef::ZombieVillager), [0.0, 0.2]);
        assert_eq!(sneak_terms(ModelRef::Skeleton), [0.0, 0.2]);
        assert_eq!(sneak_terms(ModelRef::Giant), [0.0, 0.2]);
        assert_eq!(
            sneak_terms(ModelRef::Villager {
                profession: 0,
                child: false
            }),
            [0.0, 0.0]
        );
        assert_eq!(sneak_terms(ModelRef::Witch), [0.0, 0.0]);
        assert_eq!(sneak_terms(ModelRef::Pig { saddle: false }), [0.0, 0.0]);
        assert_eq!(sneak_terms(ModelRef::IronGolem), [0.0, 0.0]);
        // The iron golem leans into its walk (`RenderIronGolem.rotateCorpse`:28-34): on a
        // whole stride (limb swing 6.5 of the fold) the lean is six and a half degrees over
        // the wave's four-thirteenths.
        let walking = Pose {
            limb_swing: 6.5,
            limb_swing_amount: 1.0,
            ..Pose::default()
        };
        assert_eq!(corpse_roll(ModelRef::IronGolem, &walking), 5.5);
        // Below a hundredth of a stride the source's own guard leaves the body upright.
        let still = Pose {
            limb_swing: 6.5,
            limb_swing_amount: 0.005,
            ..Pose::default()
        };
        assert_eq!(corpse_roll(ModelRef::IronGolem, &still), 0.0);
        // Every other kind stands upright, the walking or not.
        assert_eq!(corpse_roll(ModelRef::Pig { saddle: false }, &walking), 0.0);
        assert_eq!(corpse_roll(player, &walking), 0.0);
        // The villager's child half-scales the model (`RenderVillager.preRenderCallback`);
        // the witch's and the adult villager's own scale is the player renderer's.
        assert_eq!(render_scale(player), 0.9375);
        assert_eq!(
            render_scale(ModelRef::Villager {
                profession: 0,
                child: true
            }),
            0.46875
        );
        assert_eq!(
            render_scale(ModelRef::Villager {
                profession: 0,
                child: false
            }),
            0.9375
        );
        assert_eq!(render_scale(ModelRef::Witch), 0.9375);
        assert_eq!(render_scale(ModelRef::Giant), 6.0);
        assert_eq!(render_scale(ModelRef::Zombie), 1.0);
        assert_eq!(render_scale(ModelRef::Cow), 1.0);
    }
}
