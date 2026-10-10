//! The GUI item draws: the source's own icon transform, the vertices one icon builds
//! and the glint's two passes.
//!
//! One icon's matrix composes exactly as the source's chain does
//! (`RenderItem.renderItemIntoGUI`:353-397): the setup transforms, the model's own
//! `display.gui` transform through [`display_matrix`] (`ItemCameraTransforms.applyTransform`
//! — translate, then the rotations y, x and z in that order, then scale, with the
//! deserializer's own unit rules), and the render path's tail (`RenderItem.renderItem`
//! :140-168). The vertices are the model's own mesh in GUI-space units, each coloured
//! with the source's flat lighting factor for its normal
//! (`RenderHelper.enableStandardItemLighting`:28-48, whose `shadeModel(7424)` keeps the
//! shading constant across a face; the GUI's own item lighting turns the two lights by
//! -30 about y and 165 about x, `enableGUIStandardItemLighting`:74-78).
//!
//! An enchanted icon draws its model again twice through the glint sheet
//! (`RenderItem.renderEffect`:170-198): the texture matrix scales the model's uvs
//! eightfold, scrolls them with the pass's own period (3000 ms and 4873 ms) and turns
//! them (-50 and 10 degrees about z), the colour is `0xFF8040CC`, the blend is
//! `src_alpha`/`one` and the depth test is `GL_EQUAL` with writes off, so the second
//! draw lands exactly on the first.
//!
//! The item pipeline's projection is the source's own GUI projection
//! (`EntityRenderer.setupOverlayRendering`:1748-1758), so a draw's GUI-space z orders
//! the icons: the list's own order is the source's `zLevel` ladder ([`icon_z_level`])
//! and the depth test resolves overlaps the way the source's ladder does.

use glam::{Mat3, Mat4, Vec3};
use oxide_assets::model::Transform;

use crate::entity_pass::ItemMesh;
use crate::text::TextVertex;

/// The glint sheet's own file name (`RenderItem.java`:63's `RES_ITEM_GLINT`).
pub const GLINT_TEXTURE: &str = "misc/enchanted_item_glint.png";

/// The stitched blocks atlas's own registry key (`TextureMap.java`:30's
/// `textures/atlas/blocks.png`): the key an atlas-mapped item mesh carries, which the
/// pass resolves to its icon binding instead of a named texture.
pub const ATLAS_TEXTURE: &str = "textures/atlas/blocks.png";

/// The glint's two scroll periods in milliseconds (`RenderItem.renderEffect`:183, :191).
pub const GLINT_PERIODS: [u64; 2] = [3000, 4873];

/// The glint's scroll divisor: the phase is `(time % period) / period / 8`
/// (`RenderItem.renderEffect`:183, :191).
const GLINT_SCROLL_DIVISOR: f32 = 8.0;

/// The glint's uv scale (`RenderItem.renderEffect`:181, :189's `scale(8.0F, 8.0F, 8.0F)`).
const GLINT_UV_SCALE: f32 = 8.0;

/// The glint's two z turns in degrees (`RenderItem.renderEffect`:184's `-50.0F`,
/// :192's `10.0F`).
pub const GLINT_TURNS: [f32; 2] = [-50.0, 10.0];

/// The glint's colour, `0xFF8040CC`: the RGBA of `-8372020`, the tint
/// `RenderItem.renderEffect`:185, :193 hands `renderModel`.
pub const GLINT_COLOUR: [f32; 4] = [128.0 / 255.0, 64.0 / 255.0, 204.0 / 255.0, 1.0];

/// The GUI-space z the setup chain offsets every icon by
/// (`RenderItem.setupGuiTransform`:378's `100.0F + zLevel`).
const GUI_Z_BASE: f32 = 100.0;

/// The setup chain's centring translate (`RenderItem.setupGuiTransform`:379).
const GUI_CENTRE: f32 = 8.0;

/// The 3D branch's scale (`RenderItem.setupGuiTransform`:385).
const GUI_3D_SCALE: f32 = 40.0;

/// The 3D branch's turns in degrees: 210 about x, then -135 about y
/// (`RenderItem.setupGuiTransform`:386-387).
const GUI_3D_TURNS: [f32; 2] = [210.0, -135.0];

/// The flat branch's scale (`RenderItem.setupGuiTransform`:392).
const GUI_FLAT_SCALE: f32 = 64.0;

/// The flat branch's turn about x in degrees (`RenderItem.setupGuiTransform`:393).
const GUI_FLAT_TURN: f32 = 180.0;

/// The half scale both the setup chain (`RenderItem.setupGuiTransform`:381) and the
/// render path (`RenderItem.renderItem`:145) apply.
const RENDER_SCALE: f32 = 0.5;

/// The centring translate the render path applies (`RenderItem.renderItem`:157).
const RENDER_CENTRE: f32 = -0.5;

/// The builtin trio's own tail (`TileEntityChestRenderer.renderTileEntityAt`:116-160):
/// the block-entity offset, the y/z flip and `RenderItem.renderItem`:150's builtin turn
/// about y. The renderer's own pair of 0.5 translates cancels
/// (`TileEntityChestRenderer.renderTileEntityAt`:126, :160).
const BUILTIN_OFFSET: f32 = 1.0;

/// The builtin trio's turn about y in degrees (`RenderItem.renderItem`:149).
const BUILTIN_TURN: f32 = 180.0;

/// The z-level step one icon raises the level by
/// (`RenderItem.renderItemAndEffectIntoGUI`:402's `zLevel += 50.0F`).
const Z_LEVEL_STEP: f32 = 50.0;

/// The model units: a mesh's own coordinates are sixteenths of a block
/// (`FaceBakery`'s 0..16 element space divided by 16).
const MESH_SCALE: f32 = 1.0 / 16.0;

/// The deserializer's clamp on a stated translation, in block units after its 1/16
/// scale (`ItemTransformVec3f.Deserializer`:62-71).
const TRANSLATION_CLAMP: f32 = 1.5;

/// The deserializer's clamp on a stated scale (`ItemTransformVec3f.Deserializer`
/// :62-71).
const SCALE_CLAMP: f32 = 4.0;

/// The GUI projection's depth span and bias: the source's orthographic z runs 1000..3000
/// (`EntityRenderer.setupOverlayRendering`:1754) and its `-2000` translate centres the
/// GUI's own z in it (`:1757`), so a GUI z of `z` maps to the GL clip-space `-z / 1000`
/// and the viewport's own `[-1, 1]`-to-`[0, 1]` depth range puts the stored depth at
/// `0.5 - z / 2000`. wgpu's clip space is `[0, 1]` where GL's is `[-1, 1]`, so the port's
/// projection carries the composed pair and a GUI z of `z` lands at the depth the source
/// stores.
const GUI_Z_SPAN: f32 = 2000.0;
const GUI_Z_BIAS: f32 = 0.5;

/// The light model's ambient term (`RenderHelper.enableStandardItemLighting`:35-47).
const LIGHT_AMBIENT: f32 = 0.4;

/// Each of the two lights' diffuse term (`RenderHelper.enableStandardItemLighting`:36,
/// :39, :43).
const LIGHT_DIFFUSE: f32 = 0.6;

/// The GUI item lighting's own turn: -30 about y, then 165 about x
/// (`RenderHelper.enableGUIStandardItemLighting`:74-78).
const GUI_LIGHT_TURNS: [f32; 2] = [-30.0, 165.0];

/// The two standard item lights' raw positions (`RenderHelper.java`:12-13).
const LIGHT_POSITIONS: [[f32; 3]; 2] = [[0.2, 1.0, -0.7], [-0.2, 1.0, 0.7]];

/// The minimal stack view one item draw carries: the id and damage that pick the model
/// and the enchant flag that adds the glint.
///
/// The wire's own `MetadataItem` lives in the protocol crate, which this crate has no
/// edge to; the client converts one at its draw-list seam and nothing else of the stack
/// reaches the draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemIcon {
    /// The item id.
    pub id: i16,
    /// The item's damage or metadata value.
    pub damage: i16,
    /// Whether the stack carries the enchant flag, from the source's own `hasEffect`
    /// rule (`ItemStack.hasEffect`:859-861 → the item's `hasEffect`, `Item.hasEffect`
    /// :416-419): the NBT default — a root compound's `ench` list
    /// (`ItemStack.isItemEnchanted`:902-905) — or one of the six class overrides (the enchanted
    /// book, the written book, the bottle o' enchanting, the golden apple's metadata,
    /// the potion's effect list and the nether star's always-true foil). The draw applies the source's own second gate on top:
    /// a builtin shape never glints (`RenderItem.renderItem`:154-165).
    pub enchanted: bool,
}

/// The shape one icon's model draws with: the source's own two `isGui3d` branches and
/// the folded builtin trio's own renderer tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconShape {
    /// The flat generated shape: the source's `disableLighting` branch — a 64-fold
    /// scale and a 180-degree x turn (`RenderItem.setupGuiTransform`:390-395), drawn
    /// unlit.
    Flat,
    /// The GUI-space 3D shape: the source's `enableLighting` branch — a 40-fold scale,
    /// a 210-degree x turn and a -135-degree y turn (`RenderItem.setupGuiTransform`
    /// :383-389), lit flat per face.
    Gui3d,
    /// The folded builtin trio: the 3D branch with `renderItem`'s builtin arm
    /// (`RenderItem.renderItem`:147-154) and the block-entity renderer's own tail
    /// (`TileEntityChestRenderer.renderTileEntityAt`:116-160).
    Builtin,
}

/// One icon's draw data: the mesh, the model's GUI display transform and its shape.
#[derive(Debug, Clone)]
pub struct ItemIconMesh {
    /// The icon's mesh, in 1/16 model units with its uvs already mapped into
    /// [`ItemMesh::texture`]'s sheet.
    pub mesh: ItemMesh,
    /// The model's own GUI display transform, as the file states it.
    pub transform: Transform,
    /// The model's own first-person display transform (`display.firstperson`), as
    /// the file states it — the slot the held-item pass applies
    /// (`ItemCameraTransforms.TransformType.FIRST_PERSON`). An absent key is the
    /// source's default, which `applyTransform` no-ops (`ItemCameraTransforms.java`
    /// :59).
    pub first_person: Transform,
    /// The model's own third-person display transform (`display.thirdperson`), as the
    /// file states it — the slot the entity held item's layer applies
    /// (`LayerHeldItem.java`:66's `TransformType.THIRD_PERSON`). An absent key is the
    /// source's default, which `applyTransform` no-ops (`ItemCameraTransforms.java`:59).
    pub third_person: Transform,
    /// The shape the model draws with.
    pub shape: IconShape,
}

/// The item icons' resolver: the baked item models and their display data.
///
/// Mirrors [`crate::entity_pass::ItemMeshSource`]'s shape — the client implements it
/// beside its own `item_meshes` impl over the item table and the model bake — and hands
/// back this crate's own [`ItemMesh`] beside the `oxide-assets` display data, so no
/// protocol type crosses the crate edge.
pub trait ItemIconSource {
    /// The icon one stack draws: the model's mesh, its GUI display transform and its
    /// shape. `None` means nothing resolves for the id — the draw falls back to
    /// [`ItemIconSource::missing_icon`].
    fn icon(&self, id: i16, damage: i16) -> Option<ItemIconMesh>;

    /// The icon an unresolved stack draws: the atlas's own missing sprite as the flat
    /// shape.
    fn missing_icon(&self) -> Option<ItemIconMesh>;
}

/// One GUI item draw, as the pass builds its vertices from.
#[derive(Debug, Clone)]
pub struct GuiItemDraw {
    /// The icon's mesh, in 1/16 model units with its uvs already mapped.
    pub mesh: ItemMesh,
    /// The model's own GUI display transform, as the file states it.
    pub transform: Transform,
    /// The shape the model draws with.
    pub shape: IconShape,
}

impl GuiItemDraw {
    /// The icon a resolver's answer draws.
    pub fn from_icon(icon: &ItemIconMesh) -> Self {
        Self {
            mesh: icon.mesh.clone(),
            transform: icon.transform,
            shape: icon.shape,
        }
    }

    /// The vertices in GUI space: every mesh vertex through `matrix`, carrying the
    /// source's own flat lighting factor for its normal — or white where the draw's
    /// branch disables lighting (`RenderItem.setupGuiTransform`:394).
    pub fn vertices(&self, matrix: Mat4) -> Vec<TextVertex> {
        let normal_matrix = normal_matrix(matrix);
        let lit = self.shape != IconShape::Flat;
        let mesh = &self.mesh.vertices;
        let mut out = Vec::with_capacity(mesh.positions.len());
        for (index, position) in mesh.positions.iter().enumerate() {
            let point = matrix.transform_point3(Vec3::from_array(*position));
            let colour = if lit {
                lit_colour(
                    mesh.normals.get(index).copied().unwrap_or(UP),
                    normal_matrix,
                )
            } else {
                [1.0, 1.0, 1.0, 1.0]
            };
            out.push(TextVertex {
                position: point.to_array(),
                uv: mesh.uvs.get(index).copied().unwrap_or([0.0, 0.0]),
                colour,
            });
        }
        out
    }

    /// One glint pass's vertices: the same positions as [`GuiItemDraw::vertices`], the
    /// uvs through [`glint_uv`] and the glint's own colour.
    ///
    /// `pass` selects the source's two passes (`RenderItem.renderEffect`:181-187 and
    /// :189-195) and `time_ms` is the frame's own system time
    /// (`Minecraft.getSystemTime`), which the scroll phases read.
    pub fn glint_vertices(&self, matrix: Mat4, pass: usize, time_ms: u64) -> Vec<TextVertex> {
        let mesh = &self.mesh.vertices;
        let mut out = Vec::with_capacity(mesh.positions.len());
        for (index, position) in mesh.positions.iter().enumerate() {
            let point = matrix.transform_point3(Vec3::from_array(*position));
            let uv = mesh.uvs.get(index).copied().unwrap_or([0.0, 0.0]);
            out.push(TextVertex {
                position: point.to_array(),
                uv: glint_uv(pass, time_ms, uv),
                colour: GLINT_COLOUR,
            });
        }
        out
    }
}

/// The unit up axis, the fallback for a mesh vertex whose normal is missing.
const UP: [f32; 3] = [0.0, 1.0, 0.0];

/// The matrix a mesh's normals transform by: the draw matrix's own inverse transpose
/// (the fixed-function normal matrix), with a degenerate matrix falling back to its
/// plain linear part rather than an infinity.
fn normal_matrix(matrix: Mat4) -> Mat3 {
    let linear = Mat3::from_mat4(matrix);
    let determinant = linear.determinant();
    if determinant.is_finite() && determinant.abs() > f32::EPSILON {
        linear.inverse().transpose()
    } else {
        linear
    }
}

/// The matrix one display transform applies: translate, then the rotations y, x and z
/// in that order, then scale (`ItemCameraTransforms.applyTransform`:61-65), with the
/// deserializer's own unit rules — the translation scaled by 1/16 and clamped to ±1.5,
/// and the scale clamped to ±4 (`ItemTransformVec3f.Deserializer`:62-71).
pub fn display_matrix(transform: Transform) -> Mat4 {
    let translation = (Vec3::from_array(transform.translation) * MESH_SCALE).clamp(
        Vec3::splat(-TRANSLATION_CLAMP),
        Vec3::splat(TRANSLATION_CLAMP),
    );
    let scale = Vec3::from_array(transform.scale)
        .clamp(Vec3::splat(-SCALE_CLAMP), Vec3::splat(SCALE_CLAMP));
    let mut matrix = Mat4::from_translation(translation);
    matrix *= Mat4::from_rotation_y(transform.rotation[1].to_radians());
    matrix *= Mat4::from_rotation_x(transform.rotation[0].to_radians());
    matrix *= Mat4::from_rotation_z(transform.rotation[2].to_radians());
    matrix *= Mat4::from_scale(scale);
    matrix
}

/// The GUI-space matrix one icon draw composes: the setup chain
/// (`RenderItem.renderItemIntoGUI`:376-396), the model's display transform and the
/// render path's tail (`RenderItem.renderItem`:140-168).
pub fn icon_matrix(x: f32, y: f32, z_level: f32, shape: IconShape, transform: Transform) -> Mat4 {
    let mut matrix = Mat4::from_translation(Vec3::new(x, y, GUI_Z_BASE + z_level));
    matrix *= Mat4::from_translation(Vec3::new(GUI_CENTRE, GUI_CENTRE, 0.0));
    matrix *= Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
    matrix *= Mat4::from_scale(Vec3::splat(RENDER_SCALE));
    match shape {
        IconShape::Flat => {
            matrix *= Mat4::from_scale(Vec3::splat(GUI_FLAT_SCALE));
            matrix *= Mat4::from_rotation_x(GUI_FLAT_TURN.to_radians());
        }
        IconShape::Gui3d | IconShape::Builtin => {
            matrix *= Mat4::from_scale(Vec3::splat(GUI_3D_SCALE));
            matrix *= Mat4::from_rotation_x(GUI_3D_TURNS[0].to_radians());
            matrix *= Mat4::from_rotation_y(GUI_3D_TURNS[1].to_radians());
        }
    }
    matrix *= display_matrix(transform);
    matrix *= Mat4::from_scale(Vec3::splat(RENDER_SCALE));
    if shape == IconShape::Builtin {
        matrix *= Mat4::from_rotation_y(BUILTIN_TURN.to_radians());
    }
    matrix *= Mat4::from_translation(Vec3::splat(RENDER_CENTRE));
    if shape == IconShape::Builtin {
        // The block-entity renderer's own tail, after its pair of 0.5 translates
        // cancels (`TileEntityChestRenderer.renderTileEntityAt`:126, :160).
        matrix *= Mat4::from_translation(Vec3::new(0.0, BUILTIN_OFFSET, BUILTIN_OFFSET));
        matrix *= Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0));
    }
    matrix *= Mat4::from_scale(Vec3::splat(MESH_SCALE));
    matrix
}

/// The z-level one icon draw carries, from its own place among the list's item draws.
///
/// The source raises the level by 50 before each `renderItemAndEffectIntoGUI` call
/// (`RenderItem`:402) and lowers it by 50 once the call returns (`:443`), so its own
/// hotbar loop is balanced — every call walks the same 50 (`GuiPlayerTabOverlay`'s pair
/// does the same with its own 100, `:269`/`:271`). The ladder here is the plan's
/// recorded generalization: the container contexts' own bases folded into the list's
/// order, with the first icon of a frame at 50 and every later one 50 nearer — the
/// reason a later icon's geometry sorts strictly in front of an earlier one's.
pub fn icon_z_level(index: usize) -> f32 {
    Z_LEVEL_STEP * (index as f32 + 1.0)
}

/// The GUI projection the item draws use: the source's own pair — the orthographic
/// projection onto `(0, width) x (0, height)` with its z in `1000..3000`
/// (`EntityRenderer.setupOverlayRendering`:1754) and the `-2000` modelview translate
/// that puts GUI-space z at `100 + zLevel` (`:1757`) — composed with the viewport's own
/// depth-range transform, because wgpu's clip space keeps GL's `[0, 1]` depth alone
/// where GL's clip z spans `[-1, 1]` — so `x` maps `0..width` to `-1..1`, `y` maps
/// `0..height` to `1..-1` (the GUI's y runs down), and `z` maps `0.5 - z / 2000`: a
/// larger GUI z is nearer, the way the source's depth order runs.
pub fn gui_projection(width: f32, height: f32) -> Mat4 {
    Mat4::from_cols_array_2d(&[
        [2.0 / width, 0.0, 0.0, 0.0],
        [0.0, -2.0 / height, 0.0, 0.0],
        [0.0, 0.0, -1.0 / GUI_Z_SPAN, 0.0],
        [-1.0, 1.0, GUI_Z_BIAS, 1.0],
    ])
}

/// One glint pass's uv transform, at `time_ms`: the model's uv through the source's
/// texture matrix — the eightfold scale, the pass's scroll translate and its z turn,
/// composed in the source's own order (`RenderItem.renderEffect`:181-184 and :189-192:
/// `scale`, then `translate`, then `rotate`).
///
/// A `pass` past the source's second reads as the second.
pub fn glint_uv(pass: usize, time_ms: u64, uv: [f32; 2]) -> [f32; 2] {
    let pass = pass.min(GLINT_PERIODS.len() - 1);
    let period = GLINT_PERIODS[pass];
    let phase = (time_ms % period) as f32 / period as f32 / GLINT_SCROLL_DIVISOR;
    // The first pass scrolls towards +u, the second towards -u (`:183`, `:191`).
    let scroll = if pass == 0 { phase } else { -phase };
    let (sin, cos) = GLINT_TURNS[pass].to_radians().sin_cos();
    let [u, v] = uv;
    [
        GLINT_UV_SCALE * (u * cos - v * sin + scroll),
        GLINT_UV_SCALE * (u * sin + v * cos),
    ]
}

/// The two standard item lights in the GUI's own eye space: the raw positions
/// (`RenderHelper.java`:12-13), normalised and turned by the GUI item lighting's -30
/// about y and 165 about x (`RenderHelper.enableGUIStandardItemLighting`:74-78).
fn gui_lights() -> [Vec3; 2] {
    let turn = Mat3::from_rotation_y(GUI_LIGHT_TURNS[0].to_radians())
        * Mat3::from_rotation_x(GUI_LIGHT_TURNS[1].to_radians());
    LIGHT_POSITIONS.map(|position| (turn * Vec3::from_array(position).normalize()).normalize())
}

/// The colour one vertex carries: the source's flat lighting for the vertex's normal —
/// `ambient + diffuse * (max(N·L0, 0) + max(N·L1, 0))` over the model's two lights
/// (`RenderHelper.enableStandardItemLighting`:35-47), clamped to the unit range the
/// fixed-function pipeline clamps a lit fragment to — or white where the draw's branch
/// disables lighting.
fn lit_colour(normal: [f32; 3], normal_matrix: Mat3) -> [f32; 4] {
    let normal = (normal_matrix * Vec3::from_array(normal)).normalize_or_zero();
    let mut brightness = LIGHT_AMBIENT;
    for light in gui_lights() {
        brightness += LIGHT_DIFFUSE * normal.dot(light).max(0.0);
    }
    let brightness = brightness.clamp(0.0, 1.0);
    [brightness, brightness, brightness, 1.0]
}

#[cfg(test)]
mod tests {
    //! The transform math, pinned against the source's own constants.

    use super::*;

    /// The block item's GUI matrix, with the source's own literals: `x = 4`, `y = 5`,
    /// `zLevel = 50` (the first icon of a frame), the default display transform and the
    /// 3D branch. The expected columns are the composed matrix, worked out from
    /// `RenderItem.setupGuiTransform`:378-387 and `RenderItem.renderItem`:145-157.
    #[test]
    fn the_default_gui_transform_for_a_block_item_composes_to_the_pinned_matrix() {
        let matrix = icon_matrix(4.0, 5.0, 50.0, IconShape::Gui3d, Transform::DEFAULT);
        let expected: [f32; 16] = [
            -0.441_941_74,
            0.220_970_87,
            0.382_732_77,
            0.0,
            0.0,
            -0.541_265_9,
            0.312_5,
            0.0,
            -0.441_941_74,
            -0.220_970_87,
            -0.382_732_77,
            0.0,
            19.071_068,
            17.330_127,
            147.5,
            1.0,
        ];
        let got = matrix.to_cols_array();
        for (index, (got, want)) in got.iter().zip(&expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "column-major element {index}: {got} != {want}"
            );
        }
        // The chain's own z: the setup's `100 + zLevel` (:378) with the first rung of
        // the ladder (`renderItemAndEffectIntoGUI`:402), read at the mesh's own centre
        // (the flat branch keeps it exactly on the slot's centre).
        let centre = icon_matrix(4.0, 5.0, 50.0, IconShape::Flat, Transform::DEFAULT)
            .project_point3(Vec3::new(8.0, 8.0, 8.0));
        assert_eq!((centre.x, centre.y, centre.z), (12.0, 13.0, 150.0));
    }

    /// The 3D branch's own silhouette: the block's sixteen-unit cube lands inside the
    /// icon's own 16x16 cell, on the pinned corners.
    #[test]
    fn the_block_items_cube_lands_on_the_pinned_gui_corners() {
        let matrix = icon_matrix(4.0, 5.0, 50.0, IconShape::Gui3d, Transform::DEFAULT);
        let corner = |x: f32, y: f32, z: f32| {
            let point = matrix.project_point3(Vec3::new(x, y, z));
            (point.x, point.y, point.z)
        };
        // The mesh's own corners are sixteenths of a block, so the cube runs 0..16.
        let pinned: [(f32, f32, f32); 8] = [
            (19.071_068, 17.330_127, 147.5),
            (4.928_932, 8.669_873, 152.5),
            (12.0, 13.794_593, 141.376_28),
            (12.0, 20.865_66, 153.623_72),
            (19.071_068, 8.669_873, 152.5),
            (12.0, 12.205_407, 158.623_72),
            (12.0, 5.134_339, 146.376_28),
            (4.928_932, 17.330_127, 147.5),
        ];
        let corners = [
            (0.0, 0.0, 0.0),
            (16.0, 16.0, 16.0),
            (0.0, 0.0, 16.0),
            (16.0, 0.0, 0.0),
            (0.0, 16.0, 0.0),
            (16.0, 16.0, 0.0),
            (0.0, 16.0, 16.0),
            (16.0, 0.0, 16.0),
        ];
        for (mesh, want) in corners.iter().zip(&pinned) {
            let got = corner(mesh.0, mesh.1, mesh.2);
            assert!(
                (got.0 - want.0).abs() < 1e-4
                    && (got.1 - want.1).abs() < 1e-4
                    && (got.2 - want.2).abs() < 1e-4,
                "corner {mesh:?}: {got:?} != {want:?}"
            );
            // Every corner stays inside the icon's own cell: the slot's sixteen GUI
            // pixels at (x, y).
            assert!(
                (4.0..=20.0).contains(&got.0) && (5.0..=21.0).contains(&got.1),
                "corner {mesh:?} leaves the icon's cell: {got:?}"
            );
        }
    }

    /// A model with an explicit `display.gui` differs exactly as its JSON says: the
    /// transform lands where `applyTransform` puts it, and the composed matrix is the
    /// pinned one.
    #[test]
    fn an_explicit_gui_transform_lands_exactly_where_the_json_says() {
        // The block models' own GUI transform as 1.8 states it for a model that
        // carries one: rotation [30, 225, 0], no translation, scale 0.625.
        let transform = Transform {
            rotation: [30.0, 225.0, 0.0],
            translation: [0.0, 0.0, 0.0],
            scale: [0.625, 0.625, 0.625],
        };
        let applied = display_matrix(transform);
        let expected: [f32; 16] = [
            -0.441_941_74,
            0.0,
            0.441_941_74,
            0.0,
            -0.220_970_87,
            0.541_265_9,
            -0.220_970_87,
            0.0,
            -0.382_732_77,
            -0.312_5,
            -0.382_732_77,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        let got = applied.to_cols_array();
        for (index, (got, want)) in got.iter().zip(&expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "the display matrix's element {index}: {got} != {want}"
            );
        }
        let matrix = icon_matrix(4.0, 5.0, 50.0, IconShape::Gui3d, transform);
        let expected: [f32; 16] = [
            0.0,
            -0.195_312_5,
            -0.338_291_17,
            0.0,
            0.195_312_5,
            -0.292_968_75,
            0.169_145_59,
            0.0,
            0.338_291_17,
            0.169_145_59,
            -0.097_656_25,
            0.0,
            7.731_170_7,
            15.553_085,
            152.134_41,
            1.0,
        ];
        let got = matrix.to_cols_array();
        for (index, (got, want)) in got.iter().zip(&expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "the composed matrix's element {index}: {got} != {want}"
            );
        }
    }

    /// The deserializer's own unit rules: the translation scales by 1/16 and clamps to
    /// ±1.5, the scale clamps to ±4 (`ItemTransformVec3f.Deserializer`:62-71), and the
    /// rotations apply as stated.
    #[test]
    fn the_applier_keeps_the_deserializers_unit_rules() {
        let applied = display_matrix(Transform {
            rotation: [0.0, 90.0, 0.0],
            translation: [16.0, 24.0, -24.0],
            scale: [2.0, 0.5, 1.0],
        });
        let point = applied.project_point3(Vec3::new(1.0, 0.0, 0.0));
        // The scale doubles the unit first (it is the innermost transform), the y turn
        // sends +x to -z, and the stated translation lands at its own sixteenth.
        assert!(
            (point.x - 1.0).abs() < 1e-6
                && (point.y - 1.5).abs() < 1e-6
                && (point.z - (-3.5)).abs() < 1e-6,
            "the applied point: {point:?}"
        );
        // The clamps: a translation past ±1.5 and a scale past ±4.
        let clamped = display_matrix(Transform {
            rotation: [0.0, 0.0, 0.0],
            translation: [100.0, -100.0, 0.0],
            scale: [10.0, -10.0, 1.0],
        });
        assert_eq!(clamped.w_axis.x, 1.5, "the translation clamps at +1.5");
        assert_eq!(clamped.w_axis.y, -1.5, "the translation clamps at -1.5");
        assert_eq!(clamped.x_axis.x, 4.0, "the scale clamps at +4");
        assert_eq!(clamped.y_axis.y, -4.0, "the scale clamps at -4");
    }

    /// A generated item states no transforms at all, so its display transform is the
    /// `ItemCameraTransforms` default — the identity — and the flat branch fills the
    /// icon's own 16x16 cell.
    #[test]
    fn a_generated_items_absent_transforms_use_the_sources_default() {
        assert_eq!(display_matrix(Transform::DEFAULT), Mat4::IDENTITY);
        let matrix = icon_matrix(4.0, 5.0, 50.0, IconShape::Flat, Transform::DEFAULT);
        // The generated body's own plane: mesh z 7.5..8.5 (the generator's one-pixel
        // thickness), so the front face lands half a pixel behind the icon's centre.
        let front = matrix.project_point3(Vec3::new(0.0, 0.0, 7.5));
        assert!(
            (front.x - 4.0).abs() < 1e-5
                && (front.y - 21.0).abs() < 1e-5
                && (front.z - 149.5).abs() < 1e-5,
            "the front top-left corner: {front:?}"
        );
        let back = matrix.project_point3(Vec3::new(16.0, 16.0, 8.5));
        assert!(
            (back.x - 20.0).abs() < 1e-5
                && (back.y - 5.0).abs() < 1e-5
                && (back.z - 150.5).abs() < 1e-5,
            "the back bottom-right corner: {back:?}"
        );
    }

    /// The ladder: the first icon of a frame sits at 50 and every later one 50 nearer,
    /// and the composed matrix carries the level in its own z.
    #[test]
    fn the_z_level_ladder_steps_by_fifty_from_the_first_icon() {
        assert_eq!(icon_z_level(0), 50.0);
        assert_eq!(icon_z_level(1), 100.0);
        assert_eq!(icon_z_level(2), 150.0);
        let first = icon_matrix(
            0.0,
            0.0,
            icon_z_level(0),
            IconShape::Flat,
            Transform::DEFAULT,
        );
        let second = icon_matrix(
            0.0,
            0.0,
            icon_z_level(1),
            IconShape::Flat,
            Transform::DEFAULT,
        );
        let centre = |matrix: Mat4| matrix.project_point3(Vec3::new(8.0, 8.0, 8.0)).z;
        assert_eq!(centre(first), 150.0);
        assert_eq!(centre(second), 200.0);
    }

    /// The GUI projection: the source's own composition of
    /// `EntityRenderer.setupOverlayRendering`:1754-1757, with the corners and the depth
    /// order pinned.
    ///
    /// The depth fold, derived: the source's ortho spans 1000..3000 (`:1754`) and its
    /// `-2000` modelview translate puts a GUI z of `z` at the eye z `z - 2000`
    /// (`:1757`), so the GL clip z is `-(2*(z-2000) + 1000 + 3000) / 2000 = -z / 1000`
    /// and the window depth GL stores is `(1 - z/1000) / 2 = 0.5 - z/2000`. wgpu's clip
    /// z is the `[0, 1]` depth alone, so the port's projection carries the composed
    /// pair: the z scale is `-1 / 2000` and the z bias is `0.5`.
    #[test]
    fn the_gui_projection_maps_the_sources_own_gui_space() {
        let projection = gui_projection(320.0, 240.0);
        let expected: [f32; 16] = [
            2.0 / 320.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / 240.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0 / 2000.0,
            0.0,
            -1.0,
            1.0,
            0.5,
            1.0,
        ];
        let got = projection.to_cols_array();
        for (index, (got, want)) in got.iter().zip(&expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "the projection's element {index}: {got} != {want}"
            );
        }
        let top_left = projection.project_point3(Vec3::new(0.0, 0.0, 100.0));
        assert_eq!((top_left.x, top_left.y, top_left.z), (-1.0, 1.0, 0.45));
        let bottom_right = projection.project_point3(Vec3::new(320.0, 240.0, 0.0));
        assert_eq!(
            (bottom_right.x, bottom_right.y, bottom_right.z),
            (1.0, -1.0, 0.5)
        );
        // A nearer GUI z is nearer in depth.
        let near = projection.project_point3(Vec3::new(0.0, 0.0, 200.0)).z;
        let far = projection.project_point3(Vec3::new(0.0, 0.0, 100.0)).z;
        assert!(
            near < far,
            "z 200 ({near}) must be nearer than z 100 ({far})"
        );
    }

    /// The glint's two passes: the periods, the turns, the eightfold uv scale, the
    /// colour and the uv transform itself, all pinned.
    #[test]
    fn the_glint_passes_carry_the_sources_own_scroll() {
        assert_eq!(GLINT_PERIODS, [3000, 4873]);
        assert_eq!(GLINT_TURNS, [-50.0, 10.0]);
        assert_eq!(
            GLINT_COLOUR,
            [128.0 / 255.0, 64.0 / 255.0, 204.0 / 255.0, 1.0]
        );
        // At time zero both scroll phases are zero, so only the turns and the scale
        // show: the uv turns about the origin and scales eightfold.
        let first = glint_uv(0, 0, [0.5, 0.5]);
        assert!(
            (first[0] - 5.635_328_3).abs() < 1e-6 && (first[1] + 0.493_027_33).abs() < 1e-6,
            "the first pass's uv at time zero: {first:?}"
        );
        let second = glint_uv(1, 0, [0.5, 0.5]);
        assert!(
            (second[0] - 3.244_638_2).abs() < 1e-6 && (second[1] - 4.633_824).abs() < 1e-6,
            "the second pass's uv at time zero: {second:?}"
        );
        // Halfway through the first period the scroll is an eighth of the period, so
        // the uv moves by 8 * (0.5 / 8 / 8) = 0.5 along u; the second pass scrolls the
        // other way.
        let scrolled = glint_uv(0, 1500, [0.5, 0.5]);
        assert!(
            (scrolled[0] - 6.135_328_3).abs() < 1e-6 && (scrolled[1] + 0.493_027_33).abs() < 1e-6,
            "the first pass's uv halfway through its period: {scrolled:?}"
        );
        let other = glint_uv(1, 1500, [0.5, 0.5]);
        assert!(
            (other[0] - 2.936_819_8).abs() < 1e-6 && (other[1] - 4.633_824).abs() < 1e-6,
            "the second pass's uv halfway through its period: {other:?}"
        );
        // The scroll is periodic: the phase wraps at the period.
        assert_eq!(
            glint_uv(0, 3000, [0.25, 0.75]),
            glint_uv(0, 0, [0.25, 0.75])
        );
        assert_eq!(
            glint_uv(1, 4873, [0.25, 0.75]),
            glint_uv(1, 0, [0.25, 0.75])
        );
    }

    /// The flat lighting: the two lights' own eye-space directions and the per-face
    /// factors they give a block item's six faces, pinned.
    #[test]
    fn the_flat_lighting_lights_a_block_items_faces_as_the_source_does() {
        let lights = gui_lights();
        let expected: [[f32; 3]; 2] = [
            [-0.237_910_05, -0.634_434_8, 0.735_453_1],
            [0.028_667_25, -0.927_374_7, -0.373_033_98],
        ];
        for (light, want) in lights.iter().zip(&expected) {
            assert!(
                (light.x - want[0]).abs() < 1e-6
                    && (light.y - want[1]).abs() < 1e-6
                    && (light.z - want[2]).abs() < 1e-6,
                "the light {light:?} != {want:?}"
            );
        }
        // The 3D branch's own normal transform, with the default display: the flip and
        // the two turns (`RenderItem.setupGuiTransform`:385-387).
        let normal_matrix = Mat3::from_mat4(icon_matrix(
            4.0,
            5.0,
            50.0,
            IconShape::Gui3d,
            Transform::DEFAULT,
        ));
        let faces: [([f32; 3], f32); 6] = [
            ([0.0, 1.0, 0.0], 1.0),
            ([0.0, -1.0, 0.0], 0.4),
            ([0.0, 0.0, -1.0], 0.434_702_1),
            ([0.0, 0.0, 1.0], 0.721_624_83),
            ([1.0, 0.0, 0.0], 0.636_575_48),
            ([-1.0, 0.0, 0.0], 0.745_949_8),
        ];
        for (normal, want) in faces {
            let colour = lit_colour(normal, normal_matrix);
            assert!(
                (colour[0] - want).abs() < 1e-5
                    && (colour[1] - want).abs() < 1e-5
                    && (colour[2] - want).abs() < 1e-5
                    && colour[3] == 1.0,
                "the face {normal:?}: {colour:?} != {want}"
            );
        }
    }

    /// The icon draw's vertices: the mesh's own positions through the matrix, with the
    /// normals' lighting in the colours and the uvs untouched.
    #[test]
    fn the_icon_vertices_carry_the_positions_the_colours_and_the_uvs() {
        let mut vertices = crate::entity_models::Vertices::default();
        vertices.positions.push([0.0, 0.0, 0.0]);
        vertices.uvs.push([0.25, 0.75]);
        vertices.normals.push([0.0, 1.0, 0.0]);
        let draw = GuiItemDraw {
            mesh: ItemMesh {
                vertices: std::sync::Arc::new(vertices),
                texture: "blocks/stone",
            },
            transform: Transform::DEFAULT,
            shape: IconShape::Gui3d,
        };
        let matrix = icon_matrix(4.0, 5.0, 50.0, IconShape::Gui3d, Transform::DEFAULT);
        let built = draw.vertices(matrix);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0].position, [19.071_068, 17.330_127, 147.5]);
        assert_eq!(built[0].uv, [0.25, 0.75]);
        assert_eq!(built[0].colour, [1.0, 1.0, 1.0, 1.0]);
        // The glint passes: the same position, the uvs through the glint's own
        // transform and the glint's colour.
        let glint = draw.glint_vertices(matrix, 0, 0);
        assert_eq!(glint.len(), 1);
        assert_eq!(glint[0].position, built[0].position);
        assert_eq!(glint[0].uv, glint_uv(0, 0, [0.25, 0.75]));
        assert_eq!(glint[0].colour, GLINT_COLOUR);
        let second = draw.glint_vertices(matrix, 1, 0);
        assert_eq!(second[0].uv, glint_uv(1, 0, [0.25, 0.75]));
        assert_eq!(second[0].colour, GLINT_COLOUR);
        // The flat shape's vertices are unlit: white whatever the normal.
        let flat = GuiItemDraw {
            transform: Transform::DEFAULT,
            shape: IconShape::Flat,
            ..draw
        };
        let built = flat.vertices(icon_matrix(
            0.0,
            0.0,
            50.0,
            IconShape::Flat,
            Transform::DEFAULT,
        ));
        assert_eq!(built[0].colour, [1.0, 1.0, 1.0, 1.0]);
    }
}
