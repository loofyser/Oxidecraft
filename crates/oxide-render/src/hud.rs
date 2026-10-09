//! The hud pass: the frame's GUI-space draw list, at the source's scaled resolution.
//!
//! The pass owns the scaled resolution (`ScaledResolution.java`:14-41), the vertex
//! buffers a frame's [`HudDraw`] list builds, and the pipelines that draw them into the
//! overlay's own pass — no depth attachment, alpha blended `src_alpha` over
//! `one_minus_src_alpha` for its general primitives, with a second, unblended pipeline
//! for the glyph runs the source draws with blend off ([`HudDraw::Text`]'s `blend`): the
//! source's `drawString` never enables blend, so a run inherits the last state change —
//! the scoreboard's rects leave it off (`Gui.java`:82-83) while the chat enables it
//! around its own text (`GuiNewChat.java`:84). The frame invokes the pass after the dim
//! and before the debug overlay.
//!
//! The scale rule: the factor grows while one more step would keep `320x240` GUI pixels
//! on both axes (`ScaledResolution.java`:27-30) — the auto scale a zero gui-scale
//! setting means (`:22-25`) — and the scaled size is the ceiling of the display size
//! over the factor (`:37-40`). The unicode font's odd-factor stepdown (`:32-35`) does
//! not apply: the client draws with one font, not the unicode sheet.
//!
//! The primitives are the source's own: a solid rect samples a one-texel white texture,
//! so it runs the same pipeline as a textured rect (`Gui.drawRect` and
//! `drawTexturedModalRect` shapes), and text goes through [`crate::text::TextBuilder`]
//! in the draw's own scaled units with the source's shadow pass
//! (`FontRenderer.java`:341-358). A skin-textured rect names a registered entity texture
//! by its id ([`SkinTexId`]), resolved through [`SkinTextures`] against the registry's
//! current uploads whenever a draw list lands.

use std::sync::Arc;

use glam::Mat4;
use oxide_assets::atlas::Atlas;
use oxide_assets::font::{Font, FontError};
use oxide_assets::texture::Texture;

use crate::atlas_texture::AtlasTexture;
use crate::entity_pass::{BossStatus, SkinTexId};
use crate::gui_item::{
    ATLAS_TEXTURE, GuiItemDraw, IconShape, ItemIcon, ItemIconSource, icon_matrix, icon_z_level,
};
use crate::text::{TextBuilder, TextVertex, string_width};

/// The hud shader: map scaled GUI pixels to clip space through the orthographic
/// projection, sample the bound texture and multiply the texel by the vertex colour.
///
/// Two entry points: `fs_main` serves the blended pipeline — a solid rect samples the
/// one-texel white texture, so its fragment is exactly its colour, and a glyph's texel
/// alpha drives the blend, so a fully transparent texel leaves the frame alone — and
/// `fs_main_opaque` serves the unblended pipeline, where the source's alpha test is kept
/// as a discard (its `GL_GREATER` 0.1 threshold, `EntityRenderer.java`:1168): a fragment
/// at or below the threshold draws nothing, so a glyph keeps its shape and no full-cell
/// quad paints.
const SHADER: &str = r#"
struct Hud {
    ortho: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> hud: Hud;
@group(1) @binding(0) var hud_texture: texture_2d<f32>;
@group(1) @binding(1) var hud_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = hud.ortho * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(hud_texture, hud_sampler, input.uv) * input.color;
}

@fragment
fn fs_main_opaque(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(hud_texture, hud_sampler, input.uv) * input.color;
    if (texel.a <= 0.1) {
        discard;
    }
    return texel;
}
"#;

/// The GUI-space minimum the auto scale keeps on both axes (`ScaledResolution.java`:27).
const MIN_WIDTH: u32 = 320;

/// The GUI-space minimum height (`ScaledResolution.java`:27).
const MIN_HEIGHT: u32 = 240;

/// What a zero gui-scale setting means: keep stepping until the minimums stop the loop
/// (`ScaledResolution.java`:22-25).
const AUTO_SCALE: u32 = 1000;

/// The solid rects' uv: the white texture is one texel, so any uv inside it samples the
/// same colour; the centre keeps the lookup exact under any filtering.
const WHITE_UV: [f32; 4] = [0.5, 0.5, 0.5, 0.5];

/// The size of one hud vertex in the byte stream the GPU receives.
const VERTEX_BYTES: usize = std::mem::size_of::<TextVertex>();

/// The size of the projection uniform in bytes: one `mat4x4<f32>`.
const UNIFORM_BYTES: usize = 64;

/// The hud vertex attributes: a position at offset 0, a uv at 12 and an RGBA colour at
/// 20, [`TextVertex`]'s own layout.
static ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

/// The scaled resolution: the GUI-space size a frame lays out in, and the factor the
/// display was scaled down by (`ScaledResolution`'s three getters, `:43-66`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaledResolution {
    /// The GUI-space width: `ceil(display width / factor)`.
    pub width: u32,
    /// The GUI-space height: `ceil(display height / factor)`.
    pub height: u32,
    /// The scale factor the display was divided by.
    pub scale_factor: u32,
}

/// The source's scaled resolution for a `display_width x display_height` window and a
/// gui-scale setting (`ScaledResolution.java`:14-41).
///
/// `gui_scale` zero is the auto setting: the factor grows while `factor + 1` still
/// leaves at least `320x240` GUI pixels on both axes, to the source's own thousand-step
/// ceiling (`:22-25`, `:27-30`). Any other value caps the growth there — the size guard
/// still applies, so a display too small for the setting stops at what fits (`:27`).
pub fn scaled_resolution(
    display_width: u32,
    display_height: u32,
    gui_scale: u8,
) -> ScaledResolution {
    let ceiling = if gui_scale == 0 {
        AUTO_SCALE
    } else {
        u32::from(gui_scale)
    };
    let mut factor = 1;
    while factor < ceiling
        && display_width / (factor + 1) >= MIN_WIDTH
        && display_height / (factor + 1) >= MIN_HEIGHT
    {
        factor += 1;
    }
    ScaledResolution {
        width: ceiling_div(display_width, factor),
        height: ceiling_div(display_height, factor),
        scale_factor: factor,
    }
}

/// `MathHelper.ceiling_double_int` of `value / divisor` (`ScaledResolution.java`:37-40).
fn ceiling_div(value: u32, divisor: u32) -> u32 {
    (f64::from(value) / f64::from(divisor)).ceil() as u32
}

/// The icon sheet the boss bar's slices sample: `gui/icons.png` (`Gui.java`:14's `icons`),
/// under the key the client registers it by.
const ICONS_TEXTURE: &str = "gui/icons";

/// The boss bar's draws for a status at a scaled resolution (`GuiIngame.renderBossHealth`:901-926).
///
/// The source's own composition, in its order: the background slice `(0, 74, 182, 5)` of the
/// 256-texel sheet, drawn twice identically — a no-op, there is no dim (`:913-914`); the fill
/// slice `(0, 79, l, 5)` with `l = (int)(healthScale * (float)(182 + 1))`, the truncating cast
/// of the f32 product, only while `l > 0` (`:911`, `:916-918`); and the name centred above
/// them, white and shadowed (`:921-922`). The bar sits at `x = scaledWidth / 2 - 91`
/// (`:908-910`) and `y = 12` (`:912`); the name at `y = 2` (`:922`). Every draw is untinted:
/// the colour modifier tints the world's brightness and fog, not the GUI
/// (`EntityRenderer.java`:955-961 and `:1889-1895`).
///
/// The name's draw needs the measured font to centre it (`i / 2 - stringWidth / 2`,
/// `GuiIngame.renderBossHealth`:922); without one the bar's slices still compose and the
/// name is left out, the same nothing a text draw without a font contributes.
pub fn boss_bar_draws(
    status: &BossStatus,
    scaled: &ScaledResolution,
    font: Option<&Font>,
) -> Vec<HudDraw> {
    let i = scaled.width as i32;
    let k = i / 2 - 182 / 2;
    let l = (status.health_fraction * 183.0) as u32;
    let background = HudDraw::TexturedRect {
        texture: HudTexture::Named(ICONS_TEXTURE),
        x: k as f32,
        y: 12.0,
        width: 182.0,
        height: 5.0,
        uv: [0.0, 74.0 / 256.0, 182.0 / 256.0, 79.0 / 256.0],
        colour: [1.0, 1.0, 1.0, 1.0],
    };
    let mut draws = vec![background.clone(), background];
    if l > 0 {
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(ICONS_TEXTURE),
            x: k as f32,
            y: 12.0,
            width: l as f32,
            height: 5.0,
            uv: [0.0, 79.0 / 256.0, l as f32 / 256.0, 84.0 / 256.0],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
    }
    if let Some(font) = font {
        let width = string_width(font, &status.name);
        draws.push(HudDraw::Text {
            text: status.name.clone(),
            x: (i / 2 - width / 2) as f32,
            y: 2.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        });
    }
    draws
}

/// One texture a [`HudDraw::TexturedRect`] samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudTexture {
    /// A texture registered under a name through [`HudPass::set_texture`].
    Named(&'static str),
    /// The block atlas, through [`HudPass::set_atlas`].
    Atlas,
    /// The block atlas bound with the icon draws' no-mipmap, no-blur sampler, through
    /// [`HudPass::set_atlas_icon`]: the state the source's GUI item draws switch the
    /// atlas to before their quads and restore after
    /// (`RenderItem.renderItemIntoGUI`'s `setBlurMipmap(false, false)`,
    /// `RenderItem.java`:318/:357 — survey §1.4), so a minified icon samples the
    /// sprite's own texels where the standing binding would blend the reduced level
    /// in. Every icon draw uses this binding.
    AtlasIcon,
}

/// One item of a frame's hud draw list, in GUI-space units at the scaled resolution.
///
/// The frame fills the list in painter's order — the source's own `drawRect` and
/// `drawString` call order — and the pass draws it in that order, back to front.
#[derive(Debug, Clone, PartialEq)]
pub enum HudDraw {
    /// A solid rectangle: the top-left corner, the size in GUI pixels and the straight
    /// (non-premultiplied) RGBA colour (`Gui.drawRect`'s shape).
    Rect {
        /// The left edge.
        x: f32,
        /// The top edge.
        y: f32,
        /// The width.
        width: f32,
        /// The height.
        height: f32,
        /// The RGBA colour.
        colour: [f32; 4],
    },
    /// A textured rectangle: the atlas or a registered texture, sampled at `uv` and
    /// tinted by `colour` (`Gui.drawTexturedModalRect`'s shape).
    TexturedRect {
        /// The texture to sample.
        texture: HudTexture,
        /// The left edge.
        x: f32,
        /// The top edge.
        y: f32,
        /// The width.
        width: f32,
        /// The height.
        height: f32,
        /// The uv rectangle `[u0, v0, u1, v1]`, `(0, 0)` the texture's top-left.
        uv: [f32; 4],
        /// The RGBA tint.
        colour: [f32; 4],
    },
    /// A rectangle of a registered skin texture — the surface that samples a profile's
    /// skin, like the tab list's head cells. The texture is named by its registry id and
    /// sampled at `uv`, tinted by `colour` (`Gui.drawScaledCustomSizeModalRect`'s shape
    /// over a skin sheet bound by `TextureManager`).
    SkinRect {
        /// The registered texture's id.
        texture: SkinTexId,
        /// The left edge.
        x: f32,
        /// The top edge.
        y: f32,
        /// The width.
        width: f32,
        /// The height.
        height: f32,
        /// The uv rectangle `[u0, v0, u1, v1]`, `(0, 0)` the texture's top-left.
        uv: [f32; 4],
        /// The RGBA tint.
        colour: [f32; 4],
    },
    /// One text draw through the shared builder: the legacy `§`-coded string, the
    /// pen's start, GUI pixels per font pixel and the base RGBA
    /// (`FontRenderer.drawString`'s shape).
    Text {
        /// The draw's legacy `§`-coded text.
        text: String,
        /// The pen's start x, in GUI pixels.
        x: f32,
        /// The pen's start y.
        y: f32,
        /// GUI pixels per font pixel.
        scale: f32,
        /// The straight (non-premultiplied) RGBA base colour.
        colour: [f32; 4],
        /// Whether the darkened shadow copy draws under the text.
        shadow: bool,
        /// Whether the draw runs through the pass's `src_alpha` blend. The source's own
        /// blend state at the draw site: `drawString` never enables blend
        /// (`FontRenderer.java`:343 enables only the alpha test), so a glyph run inherits
        /// whatever the last state change left — the scoreboard's rects leave it off
        /// (`Gui.java`:82-83) while the chat enables it around its own text
        /// (`GuiNewChat.java`:84).
        blend: bool,
    },
    /// One item icon: the stack's own minimal view and the top-left corner of the
    /// source's 16x16 GUI cell (`RenderItem.renderItemAndEffectIntoGUI`'s `(x, y)`,
    /// `RenderItem.java`:399-405).
    ///
    /// The pass lays the icon out through the resolver the frame handed over
    /// ([`HudPass::set_icon_source`]): the model's mesh and its own `display.gui`
    /// transform, composed by [`icon_matrix`] with the draw's own rung of the z-level
    /// ladder, so the list's order survives the item pipeline's depth test. A stack
    /// the resolver cannot place draws its missing-sprite fallback
    /// ([`ItemIconSource::missing_icon`]); `None` draws nothing — an empty cell. An
    /// enchanted stack ([`ItemIcon::enchanted`]) draws its two glint passes after the
    /// icon, before the next draw of the list — unless its shape is builtin, which the
    /// source's draw never glints (`RenderItem.renderItem`:154-165).
    Item {
        /// The stack's own minimal view, or `None` for a cell that paints nothing.
        stack: Option<ItemIcon>,
        /// The cell's left edge.
        x: f32,
        /// The cell's top edge.
        y: f32,
    },
}

/// The skin textures a frame's head draws name by id.
///
/// The ids are the entity texture registry's ([`SkinTexId`]); the pass reads the
/// registered texture's view for the bind group a [`HudDraw::SkinRect`] samples through.
/// The registry implements this face, and the pass resolves every draw list's ids
/// against it on each upload, because a re-upload replaces the GPU texture and mints a
/// fresh id — a binding kept across frames could sample a texture the registry has
/// dropped. An id the face does not know leaves its draws out of the frame.
pub trait SkinTextures {
    /// The view registered under `id`, or `None` when nothing is — an id from an
    /// earlier frame whose upload has since been replaced.
    fn skin_view(&self, id: SkinTexId) -> Option<&wgpu::TextureView>;
}

/// The font sheet as the pass keeps it: the measured font, the sheet's texel size and
/// the bind group the glyphs sample through.
struct FontSheet {
    /// The measured widths and glyph cells the layout reads.
    font: Font,
    /// The sheet's size in texels, for the quads' uvs.
    sheet: (u32, u32),
    /// The sheet's view and sampler, bound at group 1 for the draw.
    bind: wgpu::BindGroup,
}

/// The texture key one draw batches under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchTexture {
    /// The one-texel white texture solid rects sample.
    White,
    /// The font sheet.
    Font,
    /// A named registered texture.
    Named(&'static str),
    /// The block atlas, under the standing (mipped) binding.
    Atlas,
    /// The block atlas, under the icon draws' level-0 binding.
    AtlasIcon,
    /// The glint sheet, bound under its own linear, repeating sampler
    /// ([`HudPass::set_glint`]).
    Glint,
    /// A registered skin texture, by id.
    Skin(SkinTexId),
}

/// The pipeline state one batch draws through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchState {
    /// The blended 2D pipeline.
    Blended,
    /// The unblended 2D pipeline (the source's blend-off glyph runs).
    Unblended,
    /// The item pipeline: the icon's own vertices through the depth test.
    Item,
    /// The glint pipeline: the icon's vertices again, equal-depth and write-less,
    /// blended `src_alpha` over `one`.
    Glint,
}

/// One consecutive run of draws that samples the same texture in the same pipeline
/// state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Batch {
    /// The texture the run samples.
    texture: BatchTexture,
    /// The pipeline state the run draws through; the key's second half, so a run that
    /// changes state opens its own batch.
    state: BatchState,
    /// The run's index range into the shared index buffer.
    indices: std::ops::Range<u32>,
}

/// One draw list laid out as triangles: the vertices and indices, batched per texture
/// in draw order.
#[derive(Debug, Default, PartialEq)]
struct BuiltGeometry {
    /// The frame's vertices.
    vertices: Vec<TextVertex>,
    /// The frame's triangle-list indices into `vertices`.
    indices: Vec<u32>,
    /// The per-texture runs, in draw order.
    batches: Vec<Batch>,
}

/// Lays `draws` out as triangles.
///
/// `font` is the measured font and sheet size the text draws lay out against; a text
/// draw without one contributes nothing, like the overlay before its sheet. `icons` is
/// the item icons' resolver; an item draw without one contributes nothing, like a text
/// draw before the font. `time_ms` is the frame's system time, which the glint draws'
/// scroll phases read. Consecutive draws that sample the same texture in the same
/// pipeline state share one batch, and the batches stay in draw order, so the painter's
/// order survives the batching.
fn build(
    draws: &[HudDraw],
    font: Option<(&Font, (u32, u32))>,
    icons: Option<&dyn ItemIconSource>,
    time_ms: u64,
) -> BuiltGeometry {
    let mut built = BuiltGeometry::default();
    // The rung of the z-level ladder the next icon draw takes: the list's own item
    // draws count, the 2D draws in between do not.
    let mut item_index = 0;
    for draw in draws {
        if let HudDraw::Item { stack, x, y } = draw {
            let Some(stack) = stack else { continue };
            let Some(source) = icons else { continue };
            let Some(resolved) = source
                .icon(stack.id, stack.damage)
                .or_else(|| source.missing_icon())
            else {
                continue;
            };
            let icon = GuiItemDraw::from_icon(&resolved);
            let matrix = icon_matrix(*x, *y, icon_z_level(item_index), icon.shape, icon.transform);
            item_index += 1;
            push_item(&mut built, &icon, matrix, time_ms, stack.enchanted);
            continue;
        }
        // The batch key: the texture the run samples and its pipeline state. Every
        // primitive but the text draws blends (`drawRect` and its siblings enable it for
        // their own fill); a text draw carries the state its source path left in force.
        let (texture, state) = match draw {
            HudDraw::Rect { .. } => (BatchTexture::White, BatchState::Blended),
            HudDraw::TexturedRect { texture, .. } => (
                match texture {
                    HudTexture::Named(name) => BatchTexture::Named(name),
                    HudTexture::Atlas => BatchTexture::Atlas,
                    HudTexture::AtlasIcon => BatchTexture::AtlasIcon,
                },
                BatchState::Blended,
            ),
            HudDraw::SkinRect { texture, .. } => {
                (BatchTexture::Skin(*texture), BatchState::Blended)
            }
            HudDraw::Text { blend, .. } => (
                BatchTexture::Font,
                if *blend {
                    BatchState::Blended
                } else {
                    BatchState::Unblended
                },
            ),
            // The item draws carry their own batches and never reach this key.
            HudDraw::Item { .. } => continue,
        };
        match draw {
            HudDraw::Rect {
                x,
                y,
                width,
                height,
                colour,
            } => {
                open_batch(&mut built, texture, state);
                push_quad(&mut built, (*x, *y), (*width, *height), WHITE_UV, *colour);
            }
            HudDraw::TexturedRect {
                x,
                y,
                width,
                height,
                uv,
                colour,
                ..
            } => {
                open_batch(&mut built, texture, state);
                push_quad(&mut built, (*x, *y), (*width, *height), *uv, *colour);
            }
            HudDraw::SkinRect {
                x,
                y,
                width,
                height,
                uv,
                colour,
                ..
            } => {
                open_batch(&mut built, texture, state);
                push_quad(&mut built, (*x, *y), (*width, *height), *uv, *colour);
            }
            HudDraw::Text {
                text,
                x,
                y,
                scale,
                colour,
                shadow,
                ..
            } => {
                let Some((font, sheet)) = font else {
                    continue;
                };
                open_batch(&mut built, texture, state);
                let mut builder = TextBuilder::new();
                builder.push(text, [*x, *y, 0.0], *scale, *colour, *shadow);
                let (vertices, indices) = builder.geometry(font, sheet);
                let base = built.vertices.len() as u32;
                built.vertices.extend(vertices);
                built
                    .indices
                    .extend(indices.iter().map(|index| index + base));
                close_batch(&mut built);
            }
            HudDraw::Item { .. } => {}
        }
    }
    built
}

/// Appends one icon's geometry: the item pipeline's own vertices and, for an enchanted
/// non-builtin icon, the two glint passes after them (`RenderItem.renderEffect`:170-198),
/// each under its own batch so the painter's order is the list's.
///
/// The glint's own gate is the source's: it sits in `renderItem`'s non-builtin
/// else-branch (`RenderItem.renderItem`:154-165), so a builtin model — which takes the
/// `TileEntityItemStackRenderer` branch — never glints however enchanted its stack is.
fn push_item(
    built: &mut BuiltGeometry,
    icon: &GuiItemDraw,
    matrix: Mat4,
    time_ms: u64,
    enchanted: bool,
) {
    // The atlas-mapped meshes draw through the icon binding; a model whose mesh names
    // its own texture (the folded chest trio's sheet) draws through that registration.
    let texture = if icon.mesh.texture == ATLAS_TEXTURE {
        BatchTexture::AtlasIcon
    } else {
        BatchTexture::Named(icon.mesh.texture)
    };
    push_triangles(built, &icon.vertices(matrix), texture, BatchState::Item);
    let glints = enchanted && icon.shape != IconShape::Builtin;
    if glints {
        for pass in 0..2 {
            push_triangles(
                built,
                &icon.glint_vertices(matrix, pass, time_ms),
                BatchTexture::Glint,
                BatchState::Glint,
            );
        }
    }
}

/// Appends one vertex run as quads, in order, under its own batch.
///
/// The run is the mesh's own vertices, four to a quad — the same winding [`push_quad`]
/// uses. A trailing partial quad is dropped rather than guessed at.
fn push_triangles(
    built: &mut BuiltGeometry,
    vertices: &[TextVertex],
    texture: BatchTexture,
    state: BatchState,
) {
    let quads = vertices.len() / 4;
    if quads == 0 {
        return;
    }
    open_batch(built, texture, state);
    let base = built.vertices.len() as u32;
    built.vertices.extend_from_slice(vertices);
    for quad in 0..quads {
        let first = base + (quad as u32) * 4;
        built.indices.extend_from_slice(&[
            first,
            first + 1,
            first + 2,
            first,
            first + 2,
            first + 3,
        ]);
    }
    close_batch(built);
}

/// Opens the batch `texture` and `state`'s next draws belong to: the last one when it
/// samples the same texture in the same pipeline state, a new one otherwise, and
/// returns its position.
fn open_batch(built: &mut BuiltGeometry, texture: BatchTexture, state: BatchState) -> usize {
    if built
        .batches
        .last()
        .map(|batch| (batch.texture, batch.state))
        != Some((texture, state))
    {
        built.batches.push(Batch {
            texture,
            state,
            indices: built.indices.len() as u32..0,
        });
    }
    built.batches.len() - 1
}

/// Marks the open batch's index range up to the indices pushed so far.
fn close_batch(built: &mut BuiltGeometry) {
    if let Some(batch) = built.batches.last_mut() {
        batch.indices.end = built.indices.len() as u32;
    }
}

/// Appends one axis-aligned quad as two triangles, with every vertex at `uv`'s corners
/// and carrying `colour`.
fn push_quad(
    built: &mut BuiltGeometry,
    at: (f32, f32),
    size: (f32, f32),
    uv: [f32; 4],
    colour: [f32; 4],
) {
    let (x, y) = at;
    let (width, height) = size;
    let [u0, v0, u1, v1] = uv;
    let base = built.vertices.len() as u32;
    for (position, uv) in [
        ([x, y, 0.0], [u0, v0]),
        ([x, y + height, 0.0], [u0, v1]),
        ([x + width, y + height, 0.0], [u1, v1]),
        ([x + width, y, 0.0], [u1, v0]),
    ] {
        built.vertices.push(TextVertex {
            position,
            uv,
            colour,
        });
    }
    built
        .indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    close_batch(built);
}

/// The uploaded geometry of one draw list, with the buffers kept across uploads.
#[derive(Default)]
struct Geometry {
    /// The vertex buffer.
    vertex_buffer: Option<wgpu::Buffer>,
    /// The vertex buffer's capacity in bytes; a longer list recreates it.
    vertex_capacity: u64,
    /// The index buffer.
    index_buffer: Option<wgpu::Buffer>,
    /// The index buffer's capacity in bytes; a longer list recreates it.
    index_capacity: u64,
    /// The number of indices to draw; zero draws nothing.
    index_count: u32,
    /// The per-texture runs, in draw order.
    batches: Vec<Batch>,
}

impl Geometry {
    /// Replaces the stored geometry with `built`, reusing the buffers when they are
    /// already large enough.
    fn store(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, built: BuiltGeometry) {
        let vertex_bytes = vertex_bytes(&built.vertices);
        let index_bytes = index_bytes(&built.indices);
        if self.vertex_capacity < vertex_bytes.len() as u64 {
            self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide hud vertices"),
                size: vertex_bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.vertex_capacity = vertex_bytes.len() as u64;
        }
        if self.index_capacity < index_bytes.len() as u64 {
            self.index_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide hud indices"),
                size: index_bytes.len() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.index_capacity = index_bytes.len() as u64;
        }
        if !vertex_bytes.is_empty() {
            if let Some(buffer) = &self.vertex_buffer {
                queue.write_buffer(buffer, 0, &vertex_bytes);
            }
        }
        if !index_bytes.is_empty() {
            if let Some(buffer) = &self.index_buffer {
                queue.write_buffer(buffer, 0, &index_bytes);
            }
        }
        self.index_count = built.indices.len() as u32;
        self.batches = built.batches;
    }
}

/// The hud pass's GPU state.
pub struct HudPass {
    /// The pipeline: GUI-space triangles, sampled texel times vertex colour.
    pipeline: wgpu::RenderPipeline,
    /// The unblended pipeline: the same vertex stage with `fs_main_opaque` and no blend
    /// state, for the glyph runs the source draws with blend off.
    opaque_pipeline: wgpu::RenderPipeline,
    /// The item draws' pipeline: the same vertices and the alpha test, with the depth
    /// test and write the icons' own order needs.
    item_pipeline: wgpu::RenderPipeline,
    /// The glint passes' pipeline: the icon's vertices again, equal-depth, write-less,
    /// blended `src_alpha` over `one`.
    glint_pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the scaled-resolution orthographic projection.
    ortho_buffer: wgpu::Buffer,
    /// The bind group the pipeline reads the projection through.
    ortho_bind_group: wgpu::BindGroup,
    /// The uniform buffer holding the item draws' own GUI projection, whose z carries
    /// the source's `100 + zLevel` ladder into the depth test.
    item_buffer: wgpu::Buffer,
    /// The bind group the item and glint pipelines read that projection through.
    item_bind_group: wgpu::BindGroup,
    /// The layout every drawn texture is bound through: built once, so the pipeline,
    /// the white texel, the font sheet and every registered texture agree.
    texture_layout: wgpu::BindGroupLayout,
    /// The sampler every hud texture is read through: nearest and clamp-to-edge.
    sampler: wgpu::Sampler,
    /// The sampler the glint sheet alone is read through: linear and repeating, the
    /// source's own pair for a sheet whose metadata asks for the blur and whose uvs are
    /// scaled eightfold ([`HudPass::set_glint`]'s chain).
    glint_sampler: wgpu::Sampler,
    /// The one-texel white texture solid rects sample.
    white_bind: wgpu::BindGroup,
    /// The font sheet, once [`HudPass::set_font`] has landed.
    font: Option<FontSheet>,
    /// The block atlas' bind group, once [`HudPass::set_atlas`] has landed: the
    /// standing binding, read with the atlas's mipped pair.
    atlas: Option<wgpu::BindGroup>,
    /// The block atlas' level-0 bind group, once [`HudPass::set_atlas_icon`] has
    /// landed: the icon draws' binding, read with the no-mipmap, no-blur pair.
    atlas_icon: Option<wgpu::BindGroup>,
    /// The glint sheet's bind group, once [`HudPass::set_glint`] has landed: the
    /// enchanted icons' glint passes read it through the glint sampler.
    glint_bind: Option<wgpu::BindGroup>,
    /// The named textures [`HudPass::set_texture`] registered.
    textures: Vec<(&'static str, wgpu::BindGroup)>,
    /// The skin bind groups the stored list's head draws sample: one per distinct id,
    /// rebuilt whenever a draw list lands.
    skin_binds: Vec<(SkinTexId, wgpu::BindGroup)>,
    /// The item icons' resolver, once [`HudPass::set_icon_source`] has landed.
    icons: Option<Arc<dyn ItemIconSource>>,
    /// The frame's system time in milliseconds, as the glint draws' scroll phases read
    /// it ([`HudPass::set_system_time`]).
    system_time: u64,
    /// The last draw list the frame handed over, kept so a late font still lays out.
    draws: Vec<HudDraw>,
    /// The uploaded geometry of that list.
    geometry: Geometry,
    /// The boss bar's status and the scaled resolution its draws compose against; `None`
    /// while the bar is hidden.
    boss_bar: Option<(BossStatus, ScaledResolution)>,
    /// The boss bar's composed draw list — a function of `boss_bar` and the current font —
    /// drawn ahead of the frame's own list.
    boss_draws: Vec<HudDraw>,
    /// The uploaded geometry of the boss bar's list.
    boss_geometry: Geometry,
}

impl HudPass {
    /// Builds the pipeline for colour attachments in `format`.
    ///
    /// The pipelines have no depth-stencil state and no culling: they are meant for the
    /// pass that attaches only the colour target the terrain pass has just drawn into,
    /// so wgpu rejects them in a pass that offers a depth attachment. The blended
    /// pipeline's fragment stage blends the sampled texel with
    /// `src_alpha / one_minus_src_alpha`, the client's own pair; the unblended one
    /// writes the texel straight through, with the alpha test's discard for the glyph
    /// runs the source draws with blend off.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide hud shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let ortho_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide hud ortho layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES as u64),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide hud texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ortho_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide hud ortho"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ortho_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud ortho bind group"),
            layout: &ortho_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ortho_buffer.as_entire_binding(),
            }],
        });
        // The item draws' own projection: the same uniform layout, its own buffer, so a
        // draw's GUI-space z (the source's `100 + zLevel`) reaches the depth test while
        // the 2D draws keep the flat projection.
        let item_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide hud item ortho"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let item_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud item ortho bind group"),
            layout: &ortho_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: item_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide hud pipeline layout"),
            bind_group_layouts: &[&ortho_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide hud pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            // The pass attaches depth for the item draws, so every 2D pipeline states
            // its own: always passing and never writing, the state the source's 2D GUI
            // draws leave the depth buffer in.
            depth_stencil: Some(crate::terrain_pass::depth_state_off()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format, Some(src_alpha_blend()))],
            }),
            multiview: None,
            cache: None,
        });
        let opaque_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide hud opaque pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            depth_stencil: Some(crate::terrain_pass::depth_state_off()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main_opaque"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format, None)],
            }),
            multiview: None,
            cache: None,
        });
        let item_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide hud item pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            // The item draws test and write depth, under the client's standing
            // comparison (`Minecraft.java`:540's `depthFunc(515)`, `GL_LEQUAL`), so a
            // later icon's geometry sorts in front of an earlier one's and each icon's
            // own faces resolve against each other.
            depth_stencil: Some(item_depth_state()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main_opaque"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format, Some(src_alpha_blend()))],
            }),
            multiview: None,
            cache: None,
        });
        let glint_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oxide hud glint pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout()],
            },
            primitive: primitive_state(),
            // `renderEffect`'s own depth pair: `depthFunc(514)` (`GL_EQUAL`) with
            // `depthMask(false)` (`RenderItem.java`:171-172), so a glint pass lands
            // exactly on the icon's own fragments and writes nothing.
            depth_stencil: Some(glint_depth_state()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main_opaque"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target(format, Some(glint_blend()))],
            }),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&texture_sampler_descriptor());
        // The glint sheet's own sampler: `GL_LINEAR` on both filters, no mipmaps, and
        // `GL_REPEAT`. The source's own load path reads the sheet's metadata
        // (`{"texture": {"blur": true}}`) in `SimpleTexture.loadTexture`:36-54 — `flag`
        // from `getTextureBlur()` at :44, `flag1` from `getTextureClamp()` at :45 — and
        // calls `uploadTextureImageAllocate(..., flag, flag1)` at :54;
        // `TextureUtil.uploadTextureImageSubImpl`:227 then hands the blur to
        // `setTextureBlurred`:255-258 → `setTextureBlurMipmap(true, false)`:260-274,
        // which sets min `GL_LINEAR` (no mipmaps generated) and mag `GL_LINEAR`, and
        // `setTextureClamped(false)`:250-251 sets the wrap to `GL_REPEAT`. The uvs are
        // scaled eightfold and minified over the icon, so the sheet tiles and blends
        // where the hud's clamped, nearest sampler would smear its edge texels.
        let glint_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("oxide hud glint sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            lod_max_clamp: 0.0,
            ..texture_sampler_descriptor()
        });
        let white_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud white bind group"),
            layout: &texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&white_texture(device, queue)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            pipeline,
            opaque_pipeline,
            item_pipeline,
            glint_pipeline,
            ortho_buffer,
            ortho_bind_group,
            item_buffer,
            item_bind_group,
            texture_layout,
            sampler,
            glint_sampler,
            white_bind,
            font: None,
            atlas: None,
            atlas_icon: None,
            glint_bind: None,
            textures: Vec::new(),
            skin_binds: Vec::new(),
            icons: None,
            system_time: 0,
            draws: Vec::new(),
            geometry: Geometry::default(),
            boss_bar: None,
            boss_draws: Vec::new(),
            boss_geometry: Geometry::default(),
        }
    }

    /// Rebuilds the orthographic projection for a scaled resolution and writes it out.
    ///
    /// The projection maps `(0, 0)` to the top-left corner and `(width, height)` to
    /// the bottom-right, in GUI pixels the draw list is laid out in. The queue is
    /// passed here rather than left to [`HudPass::set_draws`] so that a resize cannot
    /// leave a stale projection behind while the draws stay the same.
    pub fn set_resolution(&mut self, queue: &wgpu::Queue, width: f32, height: f32) {
        queue.write_buffer(&self.ortho_buffer, 0, &matrix_bytes(ortho(width, height)));
        queue.write_buffer(
            &self.item_buffer,
            0,
            &matrix_bytes(crate::gui_item::gui_projection(width, height)),
        );
    }

    /// Uploads `sheet` and measures it as the font every following text draws with.
    ///
    /// The widths come from the sheet through [`Font::load`], so the geometry the next
    /// upload lays out and the sheet the fragments sample cannot disagree. The draws
    /// from before the sheet are laid out again with the new metrics; until a sheet
    /// lands, text contributes nothing. A sheet that is not a 16x16 grid is
    /// [`FontError`], and the pass keeps its previous font.
    pub fn set_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sheet: &Texture,
    ) -> Result<(), FontError> {
        let font = Font::load(sheet, None)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide hud font sheet"),
            size: wgpu::Extent3d {
                width: sheet.width,
                height: sheet.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &sheet.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(sheet.width * 4),
                rows_per_image: Some(sheet.height),
            },
            wgpu::Extent3d {
                width: sheet.width,
                height: sheet.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud font bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.font = Some(FontSheet {
            font,
            sheet: (sheet.width, sheet.height),
            bind,
        });
        // The stored list was laid out with the previous sheet's metrics, or with no
        // sheet at all: lay it out again. The boss bar's draws compose with the sheet
        // too (the name's centring), so they compose again.
        self.rebuild(device, queue);
        self.rebuild_boss(device, queue);
        Ok(())
    }

    /// Registers `texture` under `name`, for [`HudDraw::TexturedRect`]s that sample
    /// [`HudTexture::Named`]. A later call under the same name replaces it.
    pub fn set_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &'static str,
        texture: &Texture,
    ) {
        let gpu_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide hud named texture"),
            size: wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture.width * 4),
                rows_per_image: Some(texture.height),
            },
            wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
        );
        let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud named texture bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        match self.textures.iter_mut().find(|(key, _)| *key == name) {
            Some(entry) => entry.1 = bind,
            None => self.textures.push((name, bind)),
        }
    }

    /// Uploads `atlas` and binds its view for [`HudDraw::TexturedRect`]s that sample
    /// [`HudTexture::Atlas`].
    ///
    /// The upload is [`AtlasTexture`]'s, the same levels-and-mips shape the terrain
    /// pass draws with, and the binding is the atlas's standing pair — nearest within a
    /// level, mipmaps live — the state the client leaves the block atlas in
    /// (`Minecraft.java:548-554`: `setBlurMipmapDirect(false, mipmapLevels > 0)`, blur
    /// off and mipmaps on) and the pair its mipped terrain layers read it through. The
    /// icon draws' own level-0 binding is [`HudPass::set_atlas_icon`]'s.
    pub fn set_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        let texture = AtlasTexture::upload(device, queue, atlas);
        self.atlas = Some(atlas_bind(
            device,
            &self.texture_layout,
            &texture,
            texture.sampler(),
            "oxide hud atlas bind group",
        ));
    }

    /// Uploads `atlas` and binds its view for [`HudDraw::TexturedRect`]s that sample
    /// [`HudTexture::AtlasIcon`]: the atlas read through the no-mipmap, no-blur pair —
    /// `GL_NEAREST` on every filter, mip level 0 alone — the state the source's GUI
    /// item draws switch the atlas to before their quads and restore after
    /// (`RenderItem.renderItemIntoGUI`'s `setBlurMipmap(false, false)`,
    /// `RenderItem.java`:318/:357 — survey §1.4). A minified icon therefore samples the
    /// sprite's own level-0 texels where the standing binding blends the reduced level
    /// in.
    pub fn set_atlas_icon(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &Atlas) {
        let texture = AtlasTexture::upload(device, queue, atlas);
        self.atlas_icon = Some(atlas_bind(
            device,
            &self.texture_layout,
            &texture,
            texture.plain_sampler(),
            "oxide hud icon atlas bind group",
        ));
    }

    /// Uploads `sheet` as the glint texture the enchanted icons' glint passes sample
    /// ([`crate::gui_item::GLINT_TEXTURE`] — the source's own `RES_ITEM_GLINT`, `RenderItem.java`:63).
    ///
    /// The sheet is bound under the glint sampler, which is the source's own pair for
    /// this texture: both filters `GL_LINEAR` — the sheet's metadata
    /// (`{"texture": {"blur": true}}`) reaches `SimpleTexture.loadTexture`:36-54's
    /// `flag`, and `TextureUtil.setTextureBlurMipmap(true, false)`:260-274 (via
    /// `setTextureBlurred`:255-258 and `uploadTextureImageSubImpl`:227) sets min and mag
    /// `GL_LINEAR` with no mipmaps — and the wrap `GL_REPEAT`
    /// (`setTextureClamped(false)`:250-251, the metadata setting no clamp). The glint's
    /// uvs are scaled eightfold, so the sheet tiles and minifies through the filters.
    /// Until a sheet lands, an enchanted icon's glint batches are skipped like any other
    /// unbound texture.
    pub fn set_glint(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, sheet: &Texture) {
        let gpu_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("oxide hud glint sheet"),
            size: wgpu::Extent3d {
                width: sheet.width,
                height: sheet.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &sheet.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(sheet.width * 4),
                rows_per_image: Some(sheet.height),
            },
            wgpu::Extent3d {
                width: sheet.width,
                height: sheet.height,
                depth_or_array_layers: 1,
            },
        );
        let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.glint_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide hud glint bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.glint_sampler),
                },
            ],
        }));
    }

    /// Hands the pass the item icons' resolver: the models the [`HudDraw::Item`] draws
    /// lay out through, and the missing-sprite fallback an unresolvable stack draws.
    ///
    /// The stored list is laid out again with the new source, so a resolver that lands
    /// after the first draw list still fills its icons in.
    pub fn set_icon_source(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: Arc<dyn ItemIconSource>,
    ) {
        self.icons = Some(source);
        self.rebuild(device, queue);
    }

    /// Hands the pass the frame's system time in milliseconds, which the glint draws'
    /// scroll phases read (`RenderItem.renderEffect`:183 and :191 —
    /// `Minecraft.getSystemTime`'s millisecond clock).
    ///
    /// A list that carries an enchanted icon is laid out again whenever the time
    /// changes: the scroll's phase is part of the glint's geometry and moves every
    /// frame. A list without one is left alone — its geometry cannot move with the
    /// clock.
    pub fn set_system_time(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, time_ms: u64) {
        if self.system_time == time_ms {
            return;
        }
        self.system_time = time_ms;
        if self.draws.iter().any(enchanted_item) {
            self.rebuild(device, queue);
        }
    }

    /// Replaces the drawn list with `draws`.
    ///
    /// Calling this again with an equal list does nothing: the geometry and the
    /// bindings the last upload made stay as they are, and the buffers are reused
    /// rather than recreated. Nothing is drawn until [`HudPass::set_resolution`] has
    /// given the pass a projection, and text waits for [`HudPass::set_font`].
    ///
    /// The skin bind groups are rebuilt whenever the list changes: a re-uploaded skin
    /// replaces the GPU texture and mints a fresh id, so every upload resolves the
    /// list's ids against the registry's current state — correctness first. The cost is
    /// one bind group per distinct id in the list, and the lists that draw heads are
    /// capped, so a frame names a bounded set.
    pub fn set_draws(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draws: &[HudDraw],
        skins: &impl SkinTextures,
    ) {
        if self.draws == draws {
            return;
        }
        self.draws.clear();
        self.draws.extend_from_slice(draws);
        self.rebuild_skins(device, skins);
        self.rebuild(device, queue);
    }

    /// Rebuilds the skin bind groups the stored list's [`HudDraw::SkinRect`]s sample:
    /// one per distinct id, in first-draw order. An id the face does not know is left
    /// unbound, and the draw's batch is skipped like a named texture that never landed.
    fn rebuild_skins(&mut self, device: &wgpu::Device, skins: &impl SkinTextures) {
        self.skin_binds.clear();
        for draw in &self.draws {
            let HudDraw::SkinRect { texture, .. } = draw else {
                continue;
            };
            if self.skin_binds.iter().any(|(id, _)| id == texture) {
                continue;
            }
            let Some(view) = skins.skin_view(*texture) else {
                continue;
            };
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("oxide hud skin bind group"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.skin_binds.push((*texture, bind));
        }
    }

    /// Lays the stored draw list out again, with the current font, icon source and
    /// system time.
    fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let font = self.font.as_ref().map(|sheet| (&sheet.font, sheet.sheet));
        let built = build(&self.draws, font, self.icons.as_deref(), self.system_time);
        self.geometry.store(device, queue, built);
    }

    /// Draws the stored list, or nothing when it is empty.
    ///
    /// The pass must attach the colour target the dim pass has just drawn into and the
    /// depth attachment the item draws test against; the 2D draws blend over what is
    /// under them, and the item draws carry their own depth states. A batch whose
    /// texture was never set is skipped.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_geometry(pass, &self.geometry);
    }

    /// Sets the boss bar's status for the following frames; `None` hides the bar.
    ///
    /// The bar's draws compose from the status and the scaled resolution through
    /// [`boss_bar_draws`] and are laid out ahead of the frame's own hud list, the source's
    /// order (`GuiIngame.java`:184's `renderBossHealth` call precedes the scoreboard and
    /// the list). Calling this again with draws equal to the stored ones does nothing, so
    /// a countdown that moves only the status' own frame count re-uploads nothing.
    pub fn set_boss_bar(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        status: Option<&BossStatus>,
        scaled: &ScaledResolution,
    ) {
        self.boss_bar = status.map(|status| (status.clone(), *scaled));
        self.rebuild_boss(device, queue);
    }

    /// Draws the boss bar's list, or nothing when it is empty.
    ///
    /// The same pipeline, projection and bindings as [`HudPass::draw`]; the frame calls
    /// this first so the bar sits under the frame's own list.
    pub fn draw_boss_bar(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_geometry(pass, &self.boss_geometry);
    }

    /// Composes the boss bar's draws from the stored status and the current font and lays
    /// them out, re-uploading only when the composition changed.
    fn rebuild_boss(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let draws = match &self.boss_bar {
            Some((status, scaled)) => {
                boss_bar_draws(status, scaled, self.font.as_ref().map(|sheet| &sheet.font))
            }
            None => Vec::new(),
        };
        if self.boss_draws == draws {
            return;
        }
        self.boss_draws = draws;
        let font = self.font.as_ref().map(|sheet| (&sheet.font, sheet.sheet));
        let built = build(
            &self.boss_draws,
            font,
            self.icons.as_deref(),
            self.system_time,
        );
        self.boss_geometry.store(device, queue, built);
    }

    /// Draws one uploaded list: the batch's pipeline, the projection and one bind group
    /// per batch; a batch whose texture was never set is skipped.
    fn draw_geometry(&self, pass: &mut wgpu::RenderPass<'_>, geometry: &Geometry) {
        let (Some(vertex_buffer), Some(index_buffer)) = (
            geometry.vertex_buffer.as_ref(),
            geometry.index_buffer.as_ref(),
        ) else {
            return;
        };
        if geometry.index_count == 0 {
            return;
        }
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let mut state = None;
        for batch in &geometry.batches {
            if state != Some(batch.state) {
                let (pipeline, group0) = match batch.state {
                    BatchState::Blended => (&self.pipeline, &self.ortho_bind_group),
                    BatchState::Unblended => (&self.opaque_pipeline, &self.ortho_bind_group),
                    BatchState::Item => (&self.item_pipeline, &self.item_bind_group),
                    BatchState::Glint => (&self.glint_pipeline, &self.item_bind_group),
                };
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group0, &[]);
                state = Some(batch.state);
            }
            let bind = match batch.texture {
                BatchTexture::White => &self.white_bind,
                BatchTexture::Font => match &self.font {
                    Some(sheet) => &sheet.bind,
                    None => continue,
                },
                BatchTexture::Named(name) => {
                    match self.textures.iter().find(|(key, _)| *key == name) {
                        Some((_, bind)) => bind,
                        None => continue,
                    }
                }
                BatchTexture::Atlas => match &self.atlas {
                    Some(bind) => bind,
                    None => continue,
                },
                BatchTexture::AtlasIcon => match &self.atlas_icon {
                    Some(bind) => bind,
                    None => continue,
                },
                BatchTexture::Glint => match &self.glint_bind {
                    Some(bind) => bind,
                    None => continue,
                },
                BatchTexture::Skin(id) => {
                    match self.skin_binds.iter().find(|(bound, _)| *bound == id) {
                        Some((_, bind)) => bind,
                        None => continue,
                    }
                }
            };
            pass.set_bind_group(1, bind, &[]);
            pass.draw_indexed(batch.indices.clone(), 0, 0..1);
        }
    }
}

/// Builds one of the hud's atlas bind groups: the uploaded atlas's view with `sampler`
/// under the hud's texture layout.
fn atlas_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &AtlasTexture,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(texture.view()),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// The one-texel white texture solid rects sample: opaque white, written at creation so
/// the bind group's view is never blank.
fn white_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oxide hud white texel"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255, 255, 255, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The orthographic projection from GUI pixels to clip space.
///
/// `(0, 0)` maps to the top-left corner of the frame and `(width, height)` to the
/// bottom-right, so GUI coordinates go in and the quads come out where the layout put
/// them. The z axis is unused: the hud has no depth state, and a vertex on the z = 0
/// plane stays inside the 0..1 clip range.
fn ortho(width: f32, height: f32) -> Mat4 {
    Mat4::orthographic_rh(0.0, width, height, 0.0, 0.0, 1.0)
}

/// The vertex buffer layout the pipeline reads, tied to [`VERTEX_BYTES`] by the tests.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// The primitive state: triangles of both windings, because the quads are axis-aligned.
fn primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        ..Default::default()
    }
}

/// The client's own blend pair: `src_alpha` over `one_minus_src_alpha`.
fn src_alpha_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The glint's own blend pair: `src_alpha` over `one` (`RenderItem.renderEffect`:172-173's
/// `blendFunc(768, 1)`, `GL_SRC_ALPHA`/`GL_ONE`), so the glint adds over the icon the
/// draw before it left.
fn glint_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The item draws' own depth state: the client's standing comparison
/// (`Minecraft.java`:540's `depthFunc(515)`, `GL_LEQUAL`) with writes on, so an icon's
/// faces resolve against each other and a later icon's geometry sorts in front of an
/// earlier one's — [`crate::terrain_pass::depth_state`]'s own shape.
fn item_depth_state() -> wgpu::DepthStencilState {
    crate::terrain_pass::depth_state(true)
}

/// The glint passes' depth state: `renderEffect`'s own pair — `depthFunc(514)`
/// (`GL_EQUAL`) with `depthMask(false)` (`RenderItem.java`:171-172) — so a glint pass
/// lands exactly on the icon's own fragments and writes nothing.
fn glint_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        depth_compare: wgpu::CompareFunction::Equal,
        ..crate::terrain_pass::depth_state(false)
    }
}

/// Whether one draw is an enchanted icon draw: the draws whose glint geometry reads the
/// frame's clock.
fn enchanted_item(draw: &HudDraw) -> bool {
    matches!(
        draw,
        HudDraw::Item {
            stack: Some(ItemIcon {
                enchanted: true,
                ..
            }),
            ..
        }
    )
}

/// The colour target for one attachment in `format`: the fragment's alpha blends over
/// the frame with the client's own `src_alpha / one_minus_src_alpha` pair when `blend`
/// is set, and writes straight through when it is not (the unblended glyph runs).
fn color_target(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

/// The sampler every hud texture is read through: nearest and clamp-to-edge, the GUI's
/// own sampler, so a texel is never blended with its neighbour and a uv on a texture's
/// edge cannot wrap around to the opposite side.
fn texture_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide hud sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    }
}

/// Packs a matrix into the 64 little-endian bytes of a WGSL `mat4x4<f32>`.
///
/// WGSL lays a uniform matrix out as four columns of four `f32`, which is the order
/// [`Mat4::to_cols_array`] returns, so the components go out as they are.
fn matrix_bytes(matrix: Mat4) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    for (index, component) in matrix.to_cols_array().iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&component.to_le_bytes());
    }
    bytes
}

/// Packs the vertices into the byte stream the vertex buffer holds.
fn vertex_bytes(vertices: &[TextVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.uv {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.colour {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// Packs `u32` indices into the little-endian bytes the index buffer holds.
fn index_bytes(indices: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(indices));
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    //! The scale rule and the geometry's arithmetic, without a GPU.

    use super::{
        Batch, BatchState, BatchTexture, BuiltGeometry, HudDraw, HudTexture, ICONS_TEXTURE,
        ItemIcon, MIN_HEIGHT, MIN_WIDTH, SkinTexId, VERTEX_BYTES, WHITE_UV, boss_bar_draws, build,
        scaled_resolution, vertex_layout,
    };
    use crate::entity_pass::{BOSS_STATUS_TIME, BossStatus};
    use crate::text::string_width;
    use oxide_assets::font::Font;
    use oxide_assets::texture::Texture;

    /// A raised status named `AA` at `fraction`, for the composer's own tests.
    fn boss_status(fraction: f32) -> BossStatus {
        BossStatus {
            name: "AA".to_owned(),
            health_fraction: fraction,
            colour_modifier: true,
            time: BOSS_STATUS_TIME,
        }
    }

    /// A 128x128 synthetic sheet whose `'A'` cell is inked in columns 0..=4: the same
    /// metric the game's chat suite and the text passes' tests measure with.
    fn font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        let code = 'A' as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            for column in 0..=4 {
                let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        Font::load(
            &Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet loads")
    }

    #[test]
    fn the_boss_bar_draws_the_sources_slices() {
        // 427 GUI units wide (1280x720 at the auto scale): the bar's left edge at
        // 427/2 - 182/2 = 122 (`GuiIngame.java`:908-910), the background slice
        // (0, 74, 182, 5) drawn twice identically (`:913-914`), and the fill
        // trunc(0.5 * 183) = 91 wide (`:911`, `:916-918`) over the same y. The name is
        // centred with integer division: 427/2 - 12/2 = 207 for "AA" (`:921-922`), white
        // and shadowed. Every draw is untinted (`:923`).
        let scaled = scaled_resolution(1280, 720, 0);
        assert_eq!(scaled.width, 427);
        let font = font();
        assert_eq!(string_width(&font, "AA"), 12, "the fixture's metric");
        let draws = boss_bar_draws(&boss_status(0.5), &scaled, Some(&font));
        assert_eq!(
            draws.len(),
            4,
            "two background slices, the fill and the name"
        );
        let background = HudDraw::TexturedRect {
            texture: HudTexture::Named(ICONS_TEXTURE),
            x: 122.0,
            y: 12.0,
            width: 182.0,
            height: 5.0,
            uv: [0.0, 74.0 / 256.0, 182.0 / 256.0, 79.0 / 256.0],
            colour: [1.0, 1.0, 1.0, 1.0],
        };
        assert_eq!(draws[0], background);
        assert_eq!(
            draws[1], background,
            "the second background draw is identical"
        );
        assert_eq!(
            draws[2],
            HudDraw::TexturedRect {
                texture: HudTexture::Named(ICONS_TEXTURE),
                x: 122.0,
                y: 12.0,
                width: 91.0,
                height: 5.0,
                uv: [0.0, 79.0 / 256.0, 91.0 / 256.0, 84.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            }
        );
        assert_eq!(
            draws[3],
            HudDraw::Text {
                text: "AA".to_owned(),
                x: 207.0,
                y: 2.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            }
        );
    }

    #[test]
    fn the_boss_bar_fill_is_the_truncated_width() {
        // `l = (int)(healthScale * (float)(182 + 1))` (`GuiIngame.java`:911): the
        // truncating cast of the f32 product — 183 at full, one pixel wider than the
        // background — 91 at half, and a fraction under 1/183 truncates to zero, which
        // the guard drops (`:916`).
        let scaled = scaled_resolution(1280, 720, 0);
        let fill_width = |fraction| match &boss_bar_draws(&boss_status(fraction), &scaled, None)[2]
        {
            HudDraw::TexturedRect { width, .. } => *width,
            other => panic!("the fill's draw, not {other:?}"),
        };
        assert_eq!(fill_width(1.0), 183.0);
        assert_eq!(fill_width(0.5), 91.0);
        assert_eq!(fill_width(0.9), 164.0);
    }

    #[test]
    fn the_boss_bar_guards_the_zero_fill() {
        // The fill draws only while `l > 0` (`GuiIngame.java`:916): a zero fraction — or
        // one that truncates to zero — composes the two background slices alone.
        let scaled = scaled_resolution(1280, 720, 0);
        let draws = boss_bar_draws(&boss_status(0.0), &scaled, None);
        assert_eq!(draws.len(), 2, "the two background slices, no fill");
        let draws = boss_bar_draws(&boss_status(0.005), &scaled, None);
        assert_eq!(draws.len(), 2, "0.005 * 183 truncates to zero, no fill");
    }

    #[test]
    fn the_scaled_resolution_keeps_the_sources_minimum() {
        // 1280x720 at the settings default (scale zero): one more step would leave
        // 1280/4 = 320 on the width but only 720/4 = 180 on the height, under the
        // 240 minimum, so the factor is three and the scaled size is the ceiling of
        // the display over it: 427x240 (`ScaledResolution.java`:27-30, `:37-40`).
        let scaled = scaled_resolution(1280, 720, 0);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (427, 240, 3)
        );
    }

    #[test]
    fn the_scaled_resolution_steps_up_with_a_larger_window() {
        // 1920x1080: five steps would leave 1080/5 = 216, under the minimum, so the
        // factor is four and the scaled size 480x270 (`ScaledResolution.java`:27-30).
        let scaled = scaled_resolution(1920, 1080, 0);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (480, 270, 4)
        );
    }

    #[test]
    fn an_explicit_gui_scale_caps_the_auto_loop() {
        // GuiScale 2 on 1280x720: the auto loop would reach three, the setting stops
        // it at two (`ScaledResolution.java`:22-25, `:27-30`).
        let scaled = scaled_resolution(1280, 720, 2);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (640, 360, 2)
        );
        // GuiScale 1 is the ceiling exactly.
        let scaled = scaled_resolution(1280, 720, 1);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (1280, 720, 1)
        );
    }

    #[test]
    fn a_display_under_the_minimum_stays_at_factor_one() {
        // The size guard runs whatever the setting says: 320x240 with GuiScale 3 has
        // no room for a second step, so the factor stays one
        // (`ScaledResolution.java`:27).
        let scaled = scaled_resolution(320, 240, 3);
        assert_eq!(
            (scaled.width, scaled.height, scaled.scale_factor),
            (320, 240, 1)
        );
    }

    #[test]
    fn the_scaled_size_is_the_ceiling_of_the_division() {
        // 1000/3 does not divide: the width rounds up (`ScaledResolution.java`:37-40).
        let scaled = scaled_resolution(1000, 720, 0);
        assert_eq!((scaled.width, scaled.height), (334, 240));
    }

    #[test]
    fn one_bar_and_its_text_batch_and_lay_out() {
        let font = font();
        let draws = [
            HudDraw::Rect {
                x: 2.0,
                y: 203.0,
                width: 324.0,
                height: 9.0,
                colour: [0.0, 0.0, 0.0, 127.0 / 255.0],
            },
            HudDraw::Text {
                text: "A§r".to_owned(),
                x: 2.0,
                y: 204.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            },
        ];
        let built = build(&draws, Some((&font, (128, 128))), None, 0);
        // The bar's quad and the text's two copies of the one glyph, shadow first.
        assert_eq!(built.vertices.len(), 4 + 8);
        assert_eq!(built.indices.len(), 6 + 12);
        // The bar first, then the text, one batch each.
        assert_eq!(
            built.batches,
            vec![
                Batch {
                    texture: BatchTexture::White,
                    state: BatchState::Blended,
                    indices: 0..6,
                },
                Batch {
                    texture: BatchTexture::Font,
                    state: BatchState::Blended,
                    indices: 6..18,
                },
            ]
        );
        // The bar's corners and its white-texel uv; the source's drawRect shape.
        let corners: Vec<[f32; 3]> = built.vertices[..4]
            .iter()
            .map(|vertex| vertex.position)
            .collect();
        assert_eq!(
            corners,
            vec![
                [2.0, 203.0, 0.0],
                [2.0, 212.0, 0.0],
                [326.0, 212.0, 0.0],
                [326.0, 203.0, 0.0],
            ]
        );
        for vertex in &built.vertices[..4] {
            assert_eq!(vertex.uv, [WHITE_UV[0], WHITE_UV[1]]);
            assert_eq!(vertex.colour, [0.0, 0.0, 0.0, 127.0 / 255.0]);
        }
        // The text's shadow copy one pixel down and right, at the darkened colour,
        // then the text itself at the draw's own place (`FontRenderer.java`:341-358).
        assert_eq!(built.vertices[4].position, [3.0, 205.0, 0.0]);
        assert_eq!(built.vertices[4].colour[0], 63.0 / 255.0);
        assert_eq!(built.vertices[8].position, [2.0, 204.0, 0.0]);
        assert_eq!(built.vertices[8].colour, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_glyph_run_that_leaves_blend_opens_its_own_batch() {
        let font = font();
        // Two text draws over the same sheet: the second carries the unblended marker
        // (the source's scoreboard state), so the batch key splits on it even though
        // the texture is one.
        let draws = [
            HudDraw::Text {
                text: "A".to_owned(),
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: false,
                blend: true,
            },
            HudDraw::Text {
                text: "A".to_owned(),
                x: 0.0,
                y: 8.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: false,
                blend: false,
            },
        ];
        let built = build(&draws, Some((&font, (128, 128))), None, 0);
        assert_eq!(
            built.batches,
            vec![
                Batch {
                    texture: BatchTexture::Font,
                    state: BatchState::Blended,
                    indices: 0..6,
                },
                Batch {
                    texture: BatchTexture::Font,
                    state: BatchState::Unblended,
                    indices: 6..12,
                },
            ],
            "the marker splits the run: one batch per blend state"
        );
    }

    #[test]
    fn a_text_draw_without_a_font_contributes_nothing() {
        let draws = [
            HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::Text {
                text: "A".to_owned(),
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: false,
                blend: true,
            },
        ];
        let built = build(&draws, None, None, 0);
        assert_eq!(built.vertices.len(), 4, "the bar keeps its quad");
        assert_eq!(built.batches.len(), 1);
    }

    #[test]
    fn a_textured_rect_batches_under_its_own_texture() {
        let draws = [
            HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                colour: [1.0; 4],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Atlas,
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
                uv: [0.0, 0.0, 0.25, 0.25],
                colour: [1.0; 4],
            },
            // The icon binding is a different sampler on the same texture: its own run.
            HudDraw::TexturedRect {
                texture: HudTexture::AtlasIcon,
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
                uv: [0.0, 0.0, 0.25, 0.25],
                colour: [1.0; 4],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/icons"),
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
                uv: [0.0, 0.0, 0.25, 0.25],
                colour: [1.0; 4],
            },
        ];
        let built: BuiltGeometry = build(&draws, None, None, 0);
        assert_eq!(
            built
                .batches
                .iter()
                .map(|batch| batch.texture)
                .collect::<Vec<BatchTexture>>(),
            vec![
                BatchTexture::White,
                BatchTexture::Atlas,
                BatchTexture::AtlasIcon,
                BatchTexture::Named("gui/icons"),
            ]
        );
        assert_eq!(built.vertices[4].uv, [0.0, 0.0]);
        assert_eq!(built.vertices[6].uv, [0.25, 0.25]);
    }

    #[test]
    fn skin_rects_batch_under_their_texture_id() {
        // Two draws of one id share a run and a fresh id opens its own; the quad's
        // corners sample the draw's uv — the face sub-rect of a skin sheet
        // (`Gui.drawScaledCustomSizeModalRect`'s shape).
        let first = SkinTexId::new(7);
        let second = SkinTexId::new(8);
        let skin = |texture| HudDraw::SkinRect {
            texture,
            x: 0.0,
            y: 0.0,
            width: 8.0,
            height: 8.0,
            uv: [8.0 / 64.0, 8.0 / 64.0, 16.0 / 64.0, 16.0 / 64.0],
            colour: [1.0; 4],
        };
        let draws = [
            skin(first),
            skin(first),
            skin(second),
            HudDraw::Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                colour: [1.0; 4],
            },
        ];
        let built = build(&draws, None, None, 0);
        assert_eq!(
            built
                .batches
                .iter()
                .map(|batch| batch.texture)
                .collect::<Vec<BatchTexture>>(),
            vec![
                BatchTexture::Skin(first),
                BatchTexture::Skin(second),
                BatchTexture::White,
            ],
            "one run per texture, in draw order"
        );
        assert_eq!(built.batches[0].indices, 0..12);
        assert_eq!(built.batches[1].indices, 12..18);
        assert_eq!(built.batches[2].indices, 18..24);
        // The first quad: the draw's rectangle and its face sub-rect at the corners.
        assert_eq!(built.vertices[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(built.vertices[0].uv, [0.125, 0.125]);
        assert_eq!(built.vertices[0].colour, [1.0; 4]);
        assert_eq!(built.vertices[2].position, [8.0, 8.0, 0.0]);
        assert_eq!(built.vertices[2].uv, [0.25, 0.25]);
    }

    #[test]
    fn the_vertex_layout_matches_the_serialised_vertices() {
        let layout = vertex_layout();
        assert_eq!(VERTEX_BYTES, 36);
        assert_eq!(layout.array_stride, VERTEX_BYTES as u64);
        let [position, uv, colour] = layout.attributes else {
            panic!("three attributes: a position, a uv and a colour");
        };
        assert_eq!(position.shader_location, 0);
        assert_eq!(position.format, wgpu::VertexFormat::Float32x3);
        assert_eq!(position.offset, 0);
        assert_eq!(uv.shader_location, 1);
        assert_eq!(uv.format, wgpu::VertexFormat::Float32x2);
        assert_eq!(uv.offset, 12);
        assert_eq!(colour.shader_location, 2);
        assert_eq!(colour.format, wgpu::VertexFormat::Float32x4);
        assert_eq!(colour.offset, 20);
        assert_eq!(colour.offset + 16, layout.array_stride);
    }

    #[test]
    fn the_scale_minimums_are_the_sources_own() {
        assert_eq!((MIN_WIDTH, MIN_HEIGHT), (320, 240));
    }

    /// A resolver for the glint gate's test: id 1 a builtin-shaped icon, id 2 the 3D
    /// shape, both one quad over a named fixture texture.
    struct GateIcons;

    /// One quad in the mesh's 1/16 units, the gate test's minimal icon.
    fn gate_quad() -> crate::entity_models::Vertices {
        crate::entity_models::Vertices {
            positions: vec![
                [0.0, 0.0, 0.0],
                [16.0, 0.0, 0.0],
                [16.0, 16.0, 0.0],
                [0.0, 16.0, 0.0],
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
        }
    }

    impl crate::gui_item::ItemIconSource for GateIcons {
        fn icon(&self, id: i16, _damage: i16) -> Option<crate::gui_item::ItemIconMesh> {
            let shape = match id {
                1 => crate::gui_item::IconShape::Builtin,
                2 => crate::gui_item::IconShape::Gui3d,
                _ => return None,
            };
            Some(crate::gui_item::ItemIconMesh {
                mesh: crate::entity_pass::ItemMesh {
                    vertices: std::sync::Arc::new(gate_quad()),
                    texture: "fixtures/items",
                },
                transform: oxide_assets::model::Transform::DEFAULT,
                first_person: oxide_assets::model::Transform::DEFAULT,
                shape,
            })
        }

        fn missing_icon(&self) -> Option<crate::gui_item::ItemIconMesh> {
            None
        }
    }

    /// The item draws' glint gate: the source's glint sits in `renderItem`'s non-builtin
    /// else-branch (`RenderItem.renderItem`:154-165) — a builtin-shaped model takes the
    /// `TileEntityItemStackRenderer` branch and never glints, however enchanted the
    /// stack, while every other shape's enchanted draw carries the two passes' glint
    /// run (one batch: both passes sample one texture in one state).
    #[test]
    fn the_glint_skips_a_builtin_shaped_stack() {
        let draw = |id: i16, enchanted: bool| HudDraw::Item {
            stack: Some(ItemIcon {
                id,
                damage: 0,
                enchanted,
            }),
            x: 0.0,
            y: 0.0,
        };
        // The builtin shape: the icon's own run alone, no glint after it.
        let built = build(&[draw(1, true)], None, Some(&GateIcons), 0);
        assert_eq!(
            built
                .batches
                .iter()
                .map(|batch| (batch.texture, batch.state))
                .collect::<Vec<_>>(),
            vec![(BatchTexture::Named("fixtures/items"), BatchState::Item)],
            "a builtin-shaped enchanted stack draws no glint"
        );
        // The 3D shape: the icon's run, then the two passes' own.
        let built = build(&[draw(2, true)], None, Some(&GateIcons), 0);
        assert_eq!(
            built
                .batches
                .iter()
                .map(|batch| (batch.texture, batch.state))
                .collect::<Vec<_>>(),
            vec![
                (BatchTexture::Named("fixtures/items"), BatchState::Item),
                (BatchTexture::Glint, BatchState::Glint),
            ],
            "the 3D shape's enchanted draw glints"
        );
        // The flag still answers for both shapes: an unenchanted draw never glints.
        let built = build(&[draw(2, false)], None, Some(&GateIcons), 0);
        assert_eq!(
            built.batches.len(),
            1,
            "an unenchanted 3D draw does not glint"
        );
    }
}
