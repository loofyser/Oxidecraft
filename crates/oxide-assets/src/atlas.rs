//! The texture atlas: one stitched image, its mip levels, the sprite index
//! and the animated sprites' frame rects.
//!
//! [`build_atlas`] takes the complete set of texture paths the caller wants
//! (Task 14 hands over
//! [`ModelSource::texture_paths`](crate::model::ModelSource::texture_paths))
//! and stitches them with [`TextureSet`]'s decoded pixels. It adds exactly
//! one sprite of its own: the procedurally generated fallback, generated in
//! memory, always present as [`Atlas::missing`] and indexed under the
//! client's own name for it, `missingno`. Every other requested path must be
//! in the set; a path in neither is an error naming it.
//!
//! The stitch is this project's own deterministic stitcher (the M2 plan's
//! Decision 11): every sprite is padded to a square power-of-two cell at
//! least 16 texels on a side, sprites are sorted by cell side descending
//! then path ascending, shelves take them left to right, and the atlas is
//! the smallest square power-of-two side from 16 to 4096 that holds them
//! all. The same input set lays out the same bytes every time.
//!
//! Mip levels follow the clone's clamp: `TextureMap.loadTextureAtlas`
//! (`TextureMap.java:154-155`) lowers the setting per sprite to
//! `min(lowestOneBit(iconWidth), lowestOneBit(iconHeight))` and reports the
//! final count as `log2` of the smallest over the set (`:166-172`). Levels
//! are generated per sprite from that sprite's own content, so a reduction
//! never blends two neighbouring sprites; animated strips are nearest
//! sampled and stay level-0 only.

use std::collections::{BTreeMap, BTreeSet};

use crate::mcmeta::AnimationMeta;
use crate::resources::TextureSet;

/// The smallest atlas side, in texels.
const MIN_SIDE: u32 = 16;

/// The largest atlas side, in texels. The texture loader's own ceiling.
const MAX_SIDE: u32 = 4096;

/// The side of the procedural fallback sprite, in texels.
const MISSING_SIDE: u32 = 16;

/// The fallback checkerboard's cell, in texels.
const MISSING_CELL: u32 = 2;

/// The fallback checkerboard's light colour, magenta.
const MISSING_LIGHT: [u8; 4] = [255, 0, 255, 255];

/// The fallback checkerboard's dark colour, black.
const MISSING_DARK: [u8; 4] = [0, 0, 0, 255];

/// The client's name for the fallback sprite
/// (`TextureMap.LOCATION_MISSING_TEXTURE`): the model layer resolves
/// `builtin/missing` to it, and every atlas carries it under this key.
const MISSING_SPRITE: &str = "missingno";

/// A rectangle of an atlas image, in texels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteRect {
    /// The left edge.
    pub x: u32,
    /// The top edge.
    pub y: u32,
    /// The width, in texels.
    pub w: u32,
    /// The height, in texels.
    pub h: u32,
}

/// One sprite's place in the atlas.
///
/// `region` is the padded power-of-two cell the stitcher reserved, and
/// `content` is the sprite's own pixels inside it, at its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasSprite {
    /// The padded cell, at level 0.
    pub region: SpriteRect,
    /// The sprite's own pixels, at level 0.
    pub content: SpriteRect,
}

/// An animated sprite's frames, in playback order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimatedSprite {
    /// One frame per playback entry. Each frame's `content` is the row of
    /// the strip that entry names, inside the strip's region at level 0;
    /// each frame's `region` is the strip's padded cell.
    pub frames: Vec<AtlasSprite>,
    /// `times[i]` is frame `i`'s own duration in ticks when its sidecar
    /// entry states one, else the sidecar's `frametime`.
    pub times: Vec<u32>,
    /// The sidecar's `interpolate` flag.
    pub interpolate: bool,
}

/// One level of the atlas: a full RGBA image, the first row at the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasLevel {
    /// The level's width, in texels.
    pub width: u32,
    /// The level's height, in texels.
    pub height: u32,
    /// `width * height * 4` bytes: red, green, blue and alpha per texel, in
    /// row-major order.
    pub rgba: Vec<u8>,
}

/// The stitched atlas: the level images, the sprite index and the animated
/// sprites.
#[derive(Debug, Clone)]
pub struct Atlas {
    /// `levels[0]` is the full atlas; `levels[l]` is its `l`-th reduction,
    /// each axis halved and floored at one. The uploader walks the vector in
    /// order.
    pub levels: Vec<AtlasLevel>,
    /// The level-0 width, in texels (a power of two).
    pub width: u32,
    /// The level-0 height, in texels (a power of two).
    pub height: u32,
    /// The number of levels, `levels.len()`.
    pub level_count: u32,
    /// Every stitched sprite, keyed by its texture path. An animated strip
    /// appears here as its whole sprite and in [`Atlas::animated`] as its
    /// frames; the fallback appears under `missingno`.
    pub sprites: BTreeMap<String, AtlasSprite>,
    /// The animated strips, keyed by their texture path.
    pub animated: BTreeMap<String, AnimatedSprite>,
    /// The fallback sprite: the 16x16 magenta and black checkerboard the
    /// model baker's `builtin/missing` and any unresolvable sprite path use.
    pub missing: AtlasSprite,
}

impl Atlas {
    /// The `(u0, v0)`/`(u1, v1)` pair a quad's uv maps into, from the
    /// sprite's content rect divided by the level-0 atlas size.
    pub fn uv(&self, sprite: &AtlasSprite) -> [[f32; 2]; 2] {
        let u0 = sprite.content.x as f32 / self.width as f32;
        let v0 = sprite.content.y as f32 / self.height as f32;
        let u1 = (sprite.content.x + sprite.content.w) as f32 / self.width as f32;
        let v1 = (sprite.content.y + sprite.content.h) as f32 / self.height as f32;
        [[u0, v0], [u1, v1]]
    }

    /// The sprite's level-0 content rect shifted right by `level`: where the
    /// sprite's own pixels sit in that level's image.
    pub fn level_rect(&self, sprite: &AtlasSprite, level: u32) -> SpriteRect {
        SpriteRect {
            x: sprite.content.x >> level,
            y: sprite.content.y >> level,
            w: sprite.content.w >> level,
            h: sprite.content.h >> level,
        }
    }
}

/// Errors from stitching an atlas.
#[derive(Debug, thiserror::Error)]
pub enum AtlasError {
    /// A requested path the texture set does not hold.
    #[error("the atlas was asked for {path}, which the texture set does not hold")]
    MissingTexture {
        /// The path the atlas was asked for.
        path: String,
    },
    /// An animation sidecar whose frame index the strip's rows do not cover.
    #[error(
        "the animation sidecar of {path} names frame index {index}, but the strip has {rows} rows"
    )]
    FrameIndex {
        /// The sprite's texture path.
        path: String,
        /// The row the sidecar names.
        index: u32,
        /// The rows the strip holds.
        rows: u32,
    },
    /// An animated sprite whose height is not a whole number of square rows.
    #[error(
        "the animated sprite {path} is {width}x{height} texels, not a whole number of \
         {width}x{width} frames"
    )]
    RaggedStrip {
        /// The strip's texture path.
        path: String,
        /// The strip's width, in texels.
        width: u32,
        /// The strip's height, in texels.
        height: u32,
    },
    /// A sprite that fits no atlas size up to the ceiling.
    #[error(
        "the sprite {path} ({width}x{height} texels, a {cell}x{cell} cell) does not fit the \
         atlas, which stops at 4096x4096"
    )]
    TooLarge {
        /// The sprite's texture path.
        path: String,
        /// The sprite's width, in texels.
        width: u32,
        /// The sprite's height, in texels.
        height: u32,
        /// The padded cell's side, in texels.
        cell: u32,
    },
}

/// Stitches `paths` into one atlas whose filtering keeps `mipmap_levels`
/// levels.
///
/// `paths` is the complete set of textures to stitch and `textures` holds
/// every one of them. The atlas adds exactly one sprite of its own: the
/// generated fallback, in [`Atlas::missing`] and in the index under
/// `missingno` — a `paths` entry naming `missingno` resolves to it rather
/// than being an error. Any other path the set does not hold is
/// [`AtlasError::MissingTexture`].
///
/// The layout is deterministic: sprites are sorted by cell side descending
/// then path ascending, shelves take them left to right, and the atlas is
/// the smallest square power-of-two side, from 16 up to 4096, that holds
/// them all; a sprite that fits none is [`AtlasError::TooLarge`] naming it.
///
/// A sprite with an animation sidecar becomes an [`AnimatedSprite`] whose
/// frames stay part of level 0 only: the strip's rows are sliced at the
/// sprite's own width (`TextureAtlasSprite.loadSprite` sets the sprite's
/// height to its width for an animation) and the sidecar's playback list
/// selects them. A height that is not a whole number of rows, or a frame
/// index past the rows, is an error naming the sprite. Every other sprite
/// gets its levels `1..level_count` generated from its own content, so no
/// reduction blends two sprites.
pub fn build_atlas(
    textures: &TextureSet,
    paths: &BTreeSet<String>,
    mipmap_levels: u32,
) -> Result<Atlas, AtlasError> {
    let fallback = missing_pixels();

    // Classify every requested path, in the set's own (sorted) order, so the
    // same input fails on the same path every time.
    let mut plans: Vec<Plan> = Vec::with_capacity(paths.len() + 1);
    for path in paths {
        let Some(texture) = textures.get(path) else {
            if path == MISSING_SPRITE {
                // The fallback is in every atlas already.
                continue;
            }
            return Err(AtlasError::MissingTexture { path: path.clone() });
        };
        let animation = textures.animation(path);
        let playback = match animation {
            Some(meta) => Some(playback(path, texture.width, texture.height, meta)?),
            None => None,
        };
        plans.push(Plan {
            path,
            width: texture.width,
            height: texture.height,
            side: cell_side(texture.width, texture.height),
            pixels: &texture.rgba,
            playback,
            interpolate: animation.is_some_and(|meta| meta.interpolate),
            missing: false,
        });
    }

    // The fallback is stitched like any other sprite, at the cell floor.
    plans.push(Plan {
        path: MISSING_SPRITE,
        width: MISSING_SIDE,
        height: MISSING_SIDE,
        side: MIN_SIDE,
        pixels: &fallback,
        playback: None,
        interpolate: false,
        missing: true,
    });

    // The stitcher's order: tallest cell first, ties by path.
    plans.sort_by(|a, b| b.side.cmp(&a.side).then_with(|| a.path.cmp(b.path)));

    let (atlas_side, regions) = match place(&plans) {
        Ok(placed) => placed,
        // The fallback is always planned, so the refused index is in range.
        Err(refused) => {
            let plan = &plans[refused];
            return Err(AtlasError::TooLarge {
                path: plan.path.to_string(),
                width: plan.width,
                height: plan.height,
                cell: plan.side,
            });
        }
    };

    let count = level_count(&plans, mipmap_levels);

    // Level 0: every sprite's own pixels, at the top-left corner of its cell.
    let mut levels = Vec::with_capacity(count as usize);
    levels.push(image(atlas_side, atlas_side));
    for (plan, region) in plans.iter().zip(&regions) {
        blit(
            &mut levels[0].rgba,
            atlas_side,
            plan.pixels,
            plan.width,
            plan.height,
            region.x,
            region.y,
        );
    }

    // Levels 1..: each still sprite reduced from its own content, so
    // neighbouring cells never blend. Animated strips stay at level 0.
    let mut mips: Vec<Mip> = plans
        .iter()
        .zip(&regions)
        .filter(|(plan, _)| plan.playback.is_none())
        .map(|(plan, region)| Mip {
            x: region.x,
            y: region.y,
            w: plan.width,
            h: plan.height,
            rgba: plan.pixels.to_vec(),
        })
        .collect();
    for rank in 1..count {
        let side = (atlas_side >> rank).max(1);
        let mut image = image(side, side);
        for mip in &mut mips {
            let (w, h) = (mip.w / 2, mip.h / 2);
            if w == 0 || h == 0 {
                // Unreachable: the level count is clamped so a still sprite
                // keeps at least a 2x2 content down to the last level. Kept
                // total rather than unwrapped.
                continue;
            }
            let reduced = reduce(&mip.rgba, mip.w, mip.h);
            blit(
                &mut image.rgba,
                image.width,
                &reduced,
                w,
                h,
                mip.x >> rank,
                mip.y >> rank,
            );
            mip.w = w;
            mip.h = h;
            mip.rgba = reduced;
        }
        levels.push(image);
    }

    // The index: every stitched sprite by path, plus the animated strips.
    let mut sprites = BTreeMap::new();
    let mut animated = BTreeMap::new();
    for (plan, region) in plans.iter().zip(&regions) {
        let sprite = AtlasSprite {
            region: *region,
            content: SpriteRect {
                x: region.x,
                y: region.y,
                w: plan.width,
                h: plan.height,
            },
        };
        if let Some(playback) = &plan.playback {
            let frames = playback
                .iter()
                .map(|(row, _)| AtlasSprite {
                    region: *region,
                    content: SpriteRect {
                        x: sprite.content.x,
                        y: sprite.content.y + row * plan.width,
                        w: plan.width,
                        h: plan.width,
                    },
                })
                .collect();
            let times = playback.iter().map(|(_, ticks)| *ticks).collect();
            animated.insert(
                plan.path.to_string(),
                AnimatedSprite {
                    frames,
                    times,
                    interpolate: plan.interpolate,
                },
            );
        }
        sprites.insert(plan.path.to_string(), sprite);
    }

    // The fallback is planned unconditionally, so its cell is always placed;
    // the `unwrap_or` arm is unreachable and kept total rather than
    // unwrapped.
    let fallback_index = plans
        .iter()
        .position(|plan| plan.missing)
        .filter(|index| *index < regions.len())
        .unwrap_or(0);
    let region = regions[fallback_index];
    let missing = AtlasSprite {
        region,
        content: SpriteRect {
            x: region.x,
            y: region.y,
            w: MISSING_SIDE,
            h: MISSING_SIDE,
        },
    };

    Ok(Atlas {
        levels,
        width: atlas_side,
        height: atlas_side,
        level_count: count,
        sprites,
        animated,
        missing,
    })
}

/// One sprite the stitcher works on: its cell, its content and its playback
/// list, before any pixels move.
struct Plan<'a> {
    /// The path the sprite is indexed under.
    path: &'a str,
    /// The content's width, in texels.
    width: u32,
    /// The content's height, in texels.
    height: u32,
    /// The padded cell's side, in texels.
    side: u32,
    /// The content's RGBA pixels, row-major.
    pixels: &'a [u8],
    /// For an animated strip: `(row, ticks)` per playback entry, validated.
    playback: Option<Vec<(u32, u32)>>,
    /// For an animated strip: the sidecar's interpolation flag.
    interpolate: bool,
    /// True for the fallback sprite.
    missing: bool,
}

/// A still sprite's working buffer while the levels are built.
struct Mip {
    /// The sprite's level-0 content x.
    x: u32,
    /// The sprite's level-0 content y.
    y: u32,
    /// The buffer's width, in texels.
    w: u32,
    /// The buffer's height, in texels.
    h: u32,
    /// The buffer's pixels.
    rgba: Vec<u8>,
}

/// The smallest square power-of-two atlas side that holds every cell, with
/// the cell rects in plan order, or the index of the plan the largest side
/// refused.
fn place(plans: &[Plan]) -> Result<(u32, Vec<SpriteRect>), usize> {
    let mut side = MIN_SIDE;
    let mut refused = 0;
    while side <= MAX_SIDE {
        match pack(plans, side) {
            Ok(regions) => return Ok((side, regions)),
            Err(index) => refused = index,
        }
        side <<= 1;
    }
    Err(refused)
}

/// Places every cell on shelves inside a `side` x `side` atlas, in plan
/// order: left to right on a shelf, a new shelf below when the row is full.
/// Returns each cell's rect in plan order, or the index of the first plan
/// that does not fit.
fn pack(plans: &[Plan], side: u32) -> Result<Vec<SpriteRect>, usize> {
    let mut regions = Vec::with_capacity(plans.len());
    let mut x = 0;
    let mut y = 0;
    let mut shelf = 0;
    for (index, plan) in plans.iter().enumerate() {
        if plan.side > side {
            return Err(index);
        }
        if x + plan.side > side {
            y += shelf;
            x = 0;
            shelf = 0;
        }
        if y + plan.side > side {
            return Err(index);
        }
        regions.push(SpriteRect {
            x,
            y,
            w: plan.side,
            h: plan.side,
        });
        x += plan.side;
        shelf = shelf.max(plan.side);
    }
    Ok(regions)
}

/// The padded cell's side for content `width` x `height` texels: the
/// smallest power of two at least `max(width, height)`, floored at the atlas
/// floor. A side past the ceiling is refused by the packer.
fn cell_side(width: u32, height: u32) -> u32 {
    let largest = width.max(height);
    let mut side = MIN_SIDE;
    while side < largest {
        match side.checked_mul(2) {
            Some(next) => side = next,
            // The ceiling is far below; the packer refuses the side.
            None => break,
        }
    }
    side
}

/// The level count for the plans: the setting clamped by the clone's rule
/// and floored at one.
///
/// `TextureMap.loadTextureAtlas` lowers `mipmapLevels` to
/// `min(lowestOneBit(w), lowestOneBit(h))` per sprite
/// (`TextureMap.java:154-155`) and then to `log2` of the smallest over the
/// set (`:166-172`), so a set of 16x16 sprites keeps the default setting of
/// four levels and a sprite that hides only an 8 clamps it to three. An
/// animated strip counts as its frame, a square of the strip's width. The
/// fallback is not counted: the clone adds its missing image after the clamp
/// (`:211-212`).
fn level_count(plans: &[Plan], mipmap_levels: u32) -> u32 {
    let mut limit: Option<u32> = None;
    for plan in plans {
        if plan.missing {
            continue;
        }
        let low = if plan.playback.is_some() {
            lowest_one_bit(plan.width)
        } else {
            lowest_one_bit(plan.width).min(lowest_one_bit(plan.height))
        };
        limit = Some(limit.map_or(low, |current| current.min(low)));
    }
    match limit {
        Some(limit) => mipmap_levels.min(limit.trailing_zeros()).max(1),
        // Nothing to clamp against: the setting alone decides, floored at
        // the base level.
        None => mipmap_levels.max(1),
    }
}

/// The lowest set bit of `value`: the largest power of two dividing it, zero
/// for zero.
fn lowest_one_bit(value: u32) -> u32 {
    value & value.wrapping_neg()
}

/// An animated strip's validated playback list: `(row, ticks)` per entry, in
/// the sidecar's own order.
///
/// The strip's frames are square rows the width of the strip, the clone's
/// own slicing (`TextureAtlasSprite.loadSprite`: `height / width` frames of
/// `width` x `width`), so a height that is not a whole number of rows and a
/// frame index past the rows are both errors naming the sprite. An empty
/// `frames` list is the identity list over the rows.
fn playback(
    path: &str,
    width: u32,
    height: u32,
    meta: &AnimationMeta,
) -> Result<Vec<(u32, u32)>, AtlasError> {
    if width == 0 || height % width != 0 {
        return Err(AtlasError::RaggedStrip {
            path: path.to_string(),
            width,
            height,
        });
    }
    let rows = height / width;
    if meta.frames.is_empty() {
        return Ok((0..rows).map(|row| (row, meta.frametime)).collect());
    }
    let mut playback = Vec::with_capacity(meta.frames.len());
    for (position, &row) in meta.frames.iter().enumerate() {
        if row >= rows {
            return Err(AtlasError::FrameIndex {
                path: path.to_string(),
                index: row,
                rows,
            });
        }
        let ticks = meta
            .frame_times
            .get(position)
            .copied()
            .flatten()
            .unwrap_or(meta.frametime);
        playback.push((row, ticks));
    }
    Ok(playback)
}

/// The fallback sprite's pixels: a 16x16 checkerboard of magenta and black
/// in 2-texel cells, generated here so no file carries it.
fn missing_pixels() -> Vec<u8> {
    let mut rgba = vec![0u8; (MISSING_SIDE * MISSING_SIDE * 4) as usize];
    for y in 0..MISSING_SIDE {
        for x in 0..MISSING_SIDE {
            let light = ((x / MISSING_CELL) + (y / MISSING_CELL)) % 2 == 0;
            let colour = if light { MISSING_LIGHT } else { MISSING_DARK };
            let at = ((y * MISSING_SIDE + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&colour);
        }
    }
    rgba
}

/// An empty RGBA image of `width` x `height` texels.
fn image(width: u32, height: u32) -> AtlasLevel {
    AtlasLevel {
        width,
        height,
        rgba: vec![0u8; (width * height * 4) as usize],
    }
}

/// Copies a `width` x `height` RGBA image into an `image_w`-wide image at
/// `(to_x, to_y)`.
///
/// The packer reserves the cell, so the copy stays inside the image.
fn blit(
    image: &mut [u8],
    image_w: u32,
    pixels: &[u8],
    width: u32,
    height: u32,
    to_x: u32,
    to_y: u32,
) {
    let len = (width * 4) as usize;
    for row in 0..height {
        let from = (row * width * 4) as usize;
        let to = (((to_y + row) * image_w + to_x) * 4) as usize;
        image[to..to + len].copy_from_slice(&pixels[from..from + len]);
    }
}

/// One 2x2 box-average reduction of a `w` x `h` RGBA image: each axis
/// halved, every channel averaged as `(a + b + c + d + 2) / 4`, and a block
/// whose edge falls outside an odd axis sampling the edge texel again.
fn reduce(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let out_w = w / 2;
    let out_h = h / 2;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    for y in 0..out_h {
        let y0 = (2 * y).min(h.saturating_sub(1));
        let y1 = (2 * y + 1).min(h.saturating_sub(1));
        for x in 0..out_w {
            let x0 = (2 * x).min(w.saturating_sub(1));
            let x1 = (2 * x + 1).min(w.saturating_sub(1));
            for channel in 0..4 {
                let a = u32::from(rgba[texel_at(w, x0, y0, channel)]);
                let b = u32::from(rgba[texel_at(w, x1, y0, channel)]);
                let c = u32::from(rgba[texel_at(w, x0, y1, channel)]);
                let d = u32::from(rgba[texel_at(w, x1, y1, channel)]);
                out[((y * out_w + x) * 4 + channel) as usize] = ((a + b + c + d + 2) / 4) as u8;
            }
        }
    }
    out
}

/// The byte offset of channel `channel` of the texel `(x, y)` in a `w`-wide
/// RGBA image.
fn texel_at(w: u32, x: u32, y: u32, channel: u32) -> usize {
    ((y * w + x) * 4 + channel) as usize
}
