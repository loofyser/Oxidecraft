//! The atlas tests: a synthetic extraction tree in a temp directory, the
//! stitcher's observable rules, the mip clamp, the blend kernel, the
//! animated frame rects, the fallback sprite, and the ignored real-tree
//! case.
//!
//! The tree's PNGs are built by the shared writer in `common.rs`, so no file
//! from the game is involved. Each integration test is a crate of its own
//! and reaches the shared module with `mod common;`.

mod common;

use common::{block_png, rows_png, solid_png};
use std::collections::{BTreeSet, hash_map::DefaultHasher};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use oxide_assets::atlas::{Atlas, AtlasError, AtlasLevel, SpriteRect, build_atlas};
use oxide_assets::model::ModelSource;
use oxide_assets::resources::TextureSet;

/// The flat colour of `blocks/red`.
const RED: [u8; 4] = [255, 0, 0, 255];
/// The flat colour of `blocks/blue`.
const BLUE: [u8; 4] = [0, 0, 255, 255];
/// The flat colour of `blocks/slab`.
const GREEN: [u8; 4] = [0, 255, 0, 255];
/// The first row of the identity strip.
const YELLOW: [u8; 4] = [255, 255, 0, 255];
/// The second row of the identity strip.
const CYAN: [u8; 4] = [0, 255, 255, 255];
/// The first row of the out-of-order strip.
const ORANGE: [u8; 4] = [255, 128, 0, 255];
/// The second row of the out-of-order strip.
const PURPLE: [u8; 4] = [128, 0, 255, 255];
/// The fallback checkerboard's light cell: the client's `0xFFF800F8`.
const MAGENTA: [u8; 4] = [0xf8, 0, 0xf8, 0xff];
/// The fallback checkerboard's dark cell: the client's `0xFF000000`.
const BLACK: [u8; 4] = [0, 0, 0, 0xff];
/// The white texel of the two-tone blend kernel block.
const WHITE: [u8; 4] = [255, 255, 255, 255];
/// A fully transparent texel whose stored colour is white.
const CLEAR_WHITE: [u8; 4] = [255, 255, 255, 0];
/// A fully transparent texel.
const CLEAR: [u8; 4] = [0, 0, 0, 0];
/// The green texel of the alpha-cutoff block, three quarters opaque.
const FADED_GREEN: [u8; 4] = [0, 255, 0, 160];
/// The item sheet's flat colour: the item sprite the item set stitches beside
/// the block set.
const AZURE: [u8; 4] = [0, 128, 255, 255];

#[test]
fn the_atlas_is_power_of_two_and_holds_every_requested_sprite() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let requested = paths(&[
        "blocks/red",
        "blocks/blue",
        "blocks/slab",
        "blocks/strip",
        "blocks/reversed",
    ]);
    let atlas = build_atlas(&set, &requested, &no_items(), 4).expect("the requested set stitches");

    // The atlas is a power of two on both axes, and every level halves each
    // axis, floored at one.
    assert!(
        atlas.width.is_power_of_two() && atlas.height.is_power_of_two(),
        "the atlas is {}x{}, not a power of two on both axes",
        atlas.width,
        atlas.height
    );
    assert!(atlas.width >= 16 && atlas.height >= 16, "the atlas floor");
    assert_eq!(
        atlas.level_count as usize,
        atlas.levels.len(),
        "level_count counts the levels"
    );
    assert_eq!(atlas.levels[0].width, atlas.width);
    assert_eq!(atlas.levels[0].height, atlas.height);
    for (index, level) in atlas.levels.iter().enumerate() {
        let rank = index as u32;
        assert_eq!(
            level.width,
            (atlas.width >> rank).max(1),
            "level {rank} halves each axis"
        );
        assert_eq!(level.height, (atlas.height >> rank).max(1));
        assert_eq!(
            level.rgba.len(),
            (level.width * level.height * 4) as usize,
            "level {rank} is four bytes per texel"
        );
    }

    // Every requested sprite is present, in a padded power-of-two cell at
    // least as large as its content, with the content inside the cell.
    for path in &requested {
        let sprite = atlas
            .sprites
            .get(path)
            .unwrap_or_else(|| panic!("{path} must be present"));
        assert!(
            sprite.region.w.is_power_of_two() && sprite.region.h.is_power_of_two(),
            "{path}'s region is not a power of two: {:?}",
            sprite.region
        );
        assert_eq!(sprite.region.w, sprite.region.h, "{path}'s cell is square");
        assert!(
            sprite.region.w >= 16,
            "{path}'s cell is padded to the 16-texel floor"
        );
        assert!(
            sprite.region.w >= sprite.content.w && sprite.region.h >= sprite.content.h,
            "{path}'s cell holds its content"
        );
        assert!(
            sprite.content.x + sprite.content.w <= sprite.region.x + sprite.region.w
                && sprite.content.y + sprite.content.h <= sprite.region.y + sprite.region.h,
            "{path}'s content lies inside its cell"
        );
    }
    assert_no_overlapping_regions(&atlas);

    // Each sprite's level-0 pixels are the source's own, in the right place.
    assert_content_is(&atlas, "blocks/red", 16, &[RED; 16]);
    assert_content_is(&atlas, "blocks/blue", 16, &[BLUE; 16]);
    assert_content_is(&atlas, "blocks/slab", 32, &[GREEN; 32]);
    assert_content_is(
        &atlas,
        "blocks/strip",
        16,
        &blocks(&[(16, YELLOW), (16, CYAN)]),
    );
    assert_content_is(
        &atlas,
        "blocks/reversed",
        16,
        &blocks(&[(16, ORANGE), (16, PURPLE)]),
    );
}

/// The item set stitches beside the block set: a synthetic item sheet registers
/// and samples like any other sprite, the sprite count grows by the item set's
/// own size, and a path both sets name is one sprite.
///
/// The item sheet is `stitching_tree`'s own — `items/widget`, the flat
/// [`AZURE`]. The client hands the registry's sprite list in beside the block
/// model set so one atlas carries both (the survey's §1.4 decision).
#[test]
fn item_sprites_stitch_beside_the_block_set() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let blocks = paths(&["blocks/red", "blocks/slab"]);
    let items = paths(&["items/widget"]);
    let atlas = build_atlas(&set, &blocks, &items, 4).expect("the two sets stitch");

    // The item sheet registers and samples: its own pixels at its place.
    assert_content_is(&atlas, "items/widget", 16, &[AZURE; 16]);

    // The block set's own sprites, the item set's, and the fallback.
    assert_eq!(
        atlas.sprites.len(),
        blocks.len() + items.len() + 1,
        "the block set, the item set and the fallback: {}",
        atlas.sprites.len()
    );
    assert!(atlas.sprites.contains_key("missingno"));
    assert_no_overlapping_regions(&atlas);

    // A path both sets name stitches once: `blocks/red` is in the block set
    // here as well, and the count does not double-count it.
    let shared = build_atlas(&set, &blocks, &paths(&["items/widget", "blocks/red"]), 4)
        .expect("the two sets stitch");
    assert_content_is(&shared, "blocks/red", 16, &[RED; 16]);
    assert_eq!(
        shared.sprites.len(),
        blocks.len() + 1 + 1,
        "the shared path is one sprite: {}",
        shared.sprites.len()
    );
}

#[test]
fn uv_and_level_rect_read_the_content_rect() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(
        &set,
        &paths(&["blocks/red", "blocks/slab", "blocks/strip"]),
        &no_items(),
        4,
    )
    .expect("the requested set stitches");

    let sprite = atlas.sprites["blocks/red"];
    let [[u0, v0], [u1, v1]] = atlas.uv(&sprite);
    assert_eq!(
        [u0, v0, u1, v1],
        [
            sprite.content.x as f32 / atlas.width as f32,
            sprite.content.y as f32 / atlas.height as f32,
            (sprite.content.x + sprite.content.w) as f32 / atlas.width as f32,
            (sprite.content.y + sprite.content.h) as f32 / atlas.height as f32,
        ],
        "the uv pair is the content rect over the atlas size"
    );
    assert!(u0 < u1 && v0 < v1, "the uv pair is ordered");

    // The strip's cell is 32x32 while its content is 16x32: its uv pair is
    // the content rect over the atlas size, not the cell.
    assert_eq!(
        (atlas.width, atlas.height),
        (64, 64),
        "the case's atlas size"
    );
    let strip = atlas.sprites["blocks/strip"];
    assert_eq!(
        atlas.uv(&strip),
        [[0.5, 0.0], [0.75, 0.5]],
        "the strip's uv is its content rect over the atlas size"
    );

    // A level rect is the level-0 content rect shifted right by the level.
    for path in atlas.sprites.keys() {
        let sprite = &atlas.sprites[path];
        assert_eq!(atlas.level_rect(sprite, 0), sprite.content);
        for level in 1..atlas.level_count {
            assert_eq!(
                atlas.level_rect(sprite, level),
                shift(sprite.content, level),
                "{path} at level {level}"
            );
        }
    }
}

#[test]
fn the_default_mip_setting_keeps_five_levels_and_setting_zero_keeps_one() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let sixteen_only = paths(&["blocks/red", "blocks/blue"]);

    let default =
        build_atlas(&set, &sixteen_only, &no_items(), 4).expect("the default setting stitches");
    assert_eq!(
        default.level_count, 5,
        "the default setting of 4 on an all-16x16 set builds the base image and 4 reductions"
    );
    assert_eq!(
        (default.width, default.height),
        (32, 32),
        "two 16x16 sprites and the fallback fill the first 32-texel atlas"
    );

    let none = build_atlas(&set, &sixteen_only, &no_items(), 0).expect("setting 0 stitches");
    assert_eq!(none.level_count, 1, "setting 0 builds the base level only");
    assert_eq!(none.levels.len(), 1);

    let one = build_atlas(&set, &sixteen_only, &no_items(), 1).expect("setting 1 stitches");
    assert_eq!(
        one.level_count, 2,
        "setting 1 keeps the base image and one reduction"
    );
}

#[test]
fn a_sprite_whose_size_clamps_the_level_count() {
    // The clone's clamp takes the minimum over the set of
    // `min(lowestOneBit(width), lowestOneBit(height))` and reduces the
    // setting to its log2: a 16x24 sprite hides an 8 (2^3), so the default
    // setting's four reductions clamp to three.
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/red.png",
        &solid_png(16, 16, RED),
    );
    tree.write(
        "assets/minecraft/textures/blocks/odd.png",
        &solid_png(16, 24, GREEN),
    );
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(&set, &paths(&["blocks/red", "blocks/odd"]), &no_items(), 4)
        .expect("the tree stitches");

    assert_eq!(
        atlas.level_count, 4,
        "the 16x24 sprite clamps the default's four reductions to three"
    );
    let odd = atlas.sprites["blocks/odd"];
    assert_eq!(odd.content.h, 24);
    assert_eq!(
        odd.content.h >> 2,
        6,
        "the 16x24 content still reduces exactly down to level 2"
    );
    let last = atlas.level_count - 1;
    assert_eq!(
        atlas.levels[last as usize].width,
        (atlas.width >> last).max(1),
        "the last level halves each axis {last} times"
    );
}

#[test]
fn the_mip_kernel_blends_in_gamma_space_like_the_client() {
    // The client reduces a 2x2 block in gamma-2.2 space, not with a plain
    // byte average: a block of two white and two black texels gives 186,
    // where the plain `(a + b + c + d + 2) / 4` average gives 128. Each
    // sprite's top-left 2x2 block sits under the top-left texel of its
    // level-1 content.
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/twotone.png",
        &block_png(16, [WHITE, BLACK, BLACK, WHITE], BLACK),
    );
    tree.write(
        "assets/minecraft/textures/blocks/cutout.png",
        &block_png(16, [RED, CLEAR_WHITE, CLEAR_WHITE, CLEAR_WHITE], CLEAR),
    );
    tree.write(
        "assets/minecraft/textures/blocks/faded.png",
        &block_png(16, [FADED_GREEN, CLEAR, CLEAR, CLEAR], CLEAR),
    );
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(
        &set,
        &paths(&["blocks/twotone", "blocks/cutout", "blocks/faded"]),
        &no_items(),
        4,
    )
    .expect("the three sprites stitch");
    assert_eq!(atlas.level_count, 5, "the case has a level to check");

    let level = &atlas.levels[1];
    let top_left = |path: &str| {
        let rect = atlas.level_rect(&atlas.sprites[path], 1);
        texel(&level.rgba, level.width, rect.x, rect.y)
    };

    assert_eq!(
        top_left("blocks/twotone"),
        [186, 186, 186, 255],
        "two white and two black texels blend in gamma space"
    );

    // The sprite holds a fully transparent texel, so the client takes the
    // branch that skips the transparent texels' stored colour entirely (the
    // three white texels are not mixed in) and still divides the sums by
    // four: the alpha lands on 135, above the cutoff.
    assert_eq!(
        top_left("blocks/cutout"),
        [135, 0, 0, 135],
        "a cutout sprite's block counts only its opaque texels"
    );

    // And a blended alpha below 96 is forced to zero, while the colour
    // channels keep their blend.
    assert_eq!(
        top_left("blocks/faded"),
        [0, 135, 0, 0],
        "an alpha of 85 is cut to zero"
    );
}

#[test]
fn no_pixel_of_a_reduced_level_bleeds_across_two_adjacent_sprites() {
    // Pure red and pure blue, both 16x16, beside a padded 16x32 sprite
    // whose 32x32 cell leaves a whole column of padding a whole-level
    // reduction would have its chance to bleed into, and an animated strip
    // whose drawn frame must reduce at its own place inside its cell while
    // the rest of the cell stays empty: a reduction over the whole stitched
    // image, or over the whole strip, would show there. The packer lays the
    // red and blue cells edge to edge.
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/padded.png",
        &solid_png(16, 32, GREEN),
    );
    tree.write(
        "assets/minecraft/textures/blocks/strip.png",
        &rows_png(16, &blocks(&[(16, YELLOW), (16, CYAN)])),
    );
    tree.write(
        "assets/minecraft/textures/blocks/strip.png.mcmeta",
        br#"{"animation":{"frametime":2}}"#,
    );
    tree.write(
        "assets/minecraft/textures/blocks/red.png",
        &solid_png(16, 16, RED),
    );
    tree.write(
        "assets/minecraft/textures/blocks/blue.png",
        &solid_png(16, 16, BLUE),
    );
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(
        &set,
        &paths(&["blocks/red", "blocks/blue", "blocks/padded", "blocks/strip"]),
        &no_items(),
        4,
    )
    .expect("the four sprites stitch");

    assert_eq!(atlas.level_count, 5, "the case has reductions to check");
    let blue = atlas.sprites["blocks/blue"].content;
    let red = atlas.sprites["blocks/red"].content;
    let padded = atlas.sprites["blocks/padded"];
    assert!(
        padded.region.w > padded.content.w,
        "the case exercises a padded cell: {padded:?}"
    );
    let (left, right) = (blue.x.min(red.x), blue.x.max(red.x));
    assert_eq!(blue.y, red.y, "the two cells share a shelf");
    assert_eq!(
        right,
        left + 16,
        "the two 16-texel cells are adjacent, which is what makes this case \
         able to bleed"
    );

    for level in 1..atlas.level_count {
        let image = &atlas.levels[level as usize];
        let blue_rect = shift(blue, level);
        let red_rect = shift(red, level);
        let padded_rect = shift(padded.content, level);
        let strip_frame = shift(atlas.drawn("blocks/strip").content, level);
        assert_rect_is(image, blue_rect, BLUE, &format!("blue at level {level}"));
        assert_rect_is(image, red_rect, RED, &format!("red at level {level}"));
        assert_rect_is(
            image,
            padded_rect,
            GREEN,
            &format!("the padded sprite at level {level}"),
        );
        assert_rect_is(
            image,
            strip_frame,
            YELLOW,
            &format!("the strip's drawn frame at level {level}"),
        );

        // The texels that touch the shared edge are the first candidates.
        let (left_rect, left_colour, right_colour) = if blue.x < red.x {
            (blue_rect, BLUE, RED)
        } else {
            (red_rect, RED, BLUE)
        };
        assert_eq!(
            texel(
                &image.rgba,
                image.width,
                left_rect.x + left_rect.w - 1,
                left_rect.y
            ),
            left_colour,
            "the edge texel on the left at level {level}"
        );
        let right_rect = if blue.x < red.x { red_rect } else { blue_rect };
        assert_eq!(
            texel(&image.rgba, image.width, right_rect.x, right_rect.y),
            right_colour,
            "the edge texel on the right at level {level}"
        );

        // And no texel of the whole level is a blend of the two colours,
        // outside the fallback's own cell: the fallback's checkerboard
        // averages its magenta and black into a purple that no blend
        // detector can tell from a red/blue mix, and it is not a bleed
        // between the two sprites.
        let fallback = atlas.level_rect(&atlas.missing, level);
        for y in 0..image.height {
            for x in 0..image.width {
                if inside(fallback, x, y) {
                    continue;
                }
                let texel = texel(&image.rgba, image.width, x, y);
                assert!(
                    !is_red_blue_mix(texel),
                    "level {level} blends red and blue at ({x}, {y}): {texel:?}"
                );
            }
        }

        // Outside the sprites' own reduced content every texel of the level
        // is zero: the generator writes each sprite's own content — each
        // still sprite's, and each animated strip's drawn frame — and
        // nothing else, so no cell's padding and no gap ever carries a
        // blend. A reduction over the whole stitched image would leave
        // marks here even where its content-side texels happen to come out
        // equal, which is what makes the rule testable.
        let written = [blue_rect, red_rect, padded_rect, strip_frame, fallback];
        for y in 0..image.height {
            for x in 0..image.width {
                if written.iter().any(|rect| inside(*rect, x, y)) {
                    continue;
                }
                assert_eq!(
                    texel(&image.rgba, image.width, x, y),
                    [0, 0, 0, 0],
                    "level {level} writes outside a sprite's content at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn the_strip_frames_follow_the_playback_list_with_effective_times() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(
        &set,
        &paths(&["blocks/strip", "blocks/reversed"]),
        &no_items(),
        4,
    )
    .expect("the strips stitch");

    // The identity strip: no `frames` list, so the frames are the rows in
    // order, at the strip's width per frame, with the sidecar's frametime.
    let strip = atlas.sprites["blocks/strip"];
    let identity = atlas
        .animated
        .get("blocks/strip")
        .expect("the strip's sidecar makes it animated");
    assert_eq!(identity.frames.len(), 2, "the 16x32 strip holds two rows");
    assert_eq!(identity.times, [2, 2], "the sidecar's frametime, per frame");
    assert!(identity.interpolate, "the sidecar asks for interpolation");
    assert_eq!(
        identity.frames[0].content,
        SpriteRect {
            x: strip.content.x,
            y: strip.content.y,
            w: 16,
            h: 16
        },
        "frame 0 is row 0"
    );
    assert_eq!(
        identity.frames[1].content,
        SpriteRect {
            x: strip.content.x,
            y: strip.content.y + 16,
            w: 16,
            h: 16
        },
        "frame 1 is row 1, one frame-height down"
    );
    assert_eq!(
        identity.frames[0].region, strip.region,
        "a frame's region is the strip's padded cell"
    );
    assert_eq!(identity.frames[1].region, strip.region);

    // The out-of-order strip: `frames` [1, 0], so frame 0 shows row 1 and
    // frame 1 shows row 0; the first frame carries its own time.
    let reversed_strip = atlas.sprites["blocks/reversed"];
    let reversed = atlas
        .animated
        .get("blocks/reversed")
        .expect("the reversed strip's sidecar makes it animated");
    assert_eq!(reversed.frames.len(), 2);
    assert_eq!(
        reversed.times,
        [7, 5],
        "the frame's own time, then frametime"
    );
    assert!(!reversed.interpolate);
    assert_eq!(
        reversed.frames[0].content.y,
        reversed_strip.content.y + 16,
        "frame 0 shows the strip's second row"
    );
    assert_eq!(
        reversed.frames[1].content.y, reversed_strip.content.y,
        "frame 1 shows the strip's first row"
    );

    // The pixels under each frame rect are the source's rows.
    let level = &atlas.levels[0];
    for (frame, colour) in [
        (reversed.frames[0].content, PURPLE),
        (reversed.frames[1].content, ORANGE),
    ] {
        for y in frame.y..frame.y + frame.h {
            for x in frame.x..frame.x + frame.w {
                assert_eq!(
                    texel(&level.rgba, level.width, x, y),
                    colour,
                    "the frame's row at ({x}, {y})"
                );
            }
        }
    }

    // The strip draws its first playback entry, and its reduced levels
    // carry that frame's own pixels at the frame's place; the row the
    // strip does not draw stays out of them.
    for level in 1..atlas.level_count {
        let image = &atlas.levels[level as usize];
        let frame = shift(reversed.frames[0].content, level);
        assert_rect_is(
            image,
            frame,
            PURPLE,
            &format!("the drawn frame at level {level}"),
        );
        let other = shift(reversed.frames[1].content, level);
        for y in other.y..other.y + other.h {
            for x in other.x..other.x + other.w {
                assert_ne!(
                    texel(&image.rgba, image.width, x, y),
                    ORANGE,
                    "level {level} carries the row the strip does not draw at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn the_drawn_frame_of_a_strip_reduces_at_its_own_place() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(
        &set,
        &paths(&["blocks/strip", "blocks/reversed"]),
        &no_items(),
        4,
    )
    .expect("the strips stitch");

    // Each strip reduces the frame it draws — the first playback entry —
    // at the frame's own place: the clone generates every sprite's mipmaps
    // from its frames (`TextureMap.loadTextureAtlas`, `TextureMap.java:179`,
    // `TextureAtlasSprite.generateMipmaps` `:330-374`) and uploads the
    // first frame's chain (`:235`), so a minified liquid samples its
    // frame's own averaging rather than an empty cell. The row the strip
    // does not draw must stay out of its frame's cell.
    for (path, drawn_colour, other_colour) in [
        ("blocks/strip", YELLOW, CYAN),
        ("blocks/reversed", PURPLE, ORANGE),
    ] {
        let cell = atlas.sprites[path].region;
        let frame = atlas.drawn(path).content;
        assert_eq!(frame.w, frame.h, "{path}: the drawn frame is square");
        for level in 1..atlas.level_count {
            let image = &atlas.levels[level as usize];
            assert_rect_is(
                image,
                shift(frame, level),
                drawn_colour,
                &format!("{path}: the drawn frame at level {level}"),
            );
            let reduced_cell = shift(cell, level);
            for y in reduced_cell.y..reduced_cell.y + reduced_cell.h {
                for x in reduced_cell.x..reduced_cell.x + reduced_cell.w {
                    assert_ne!(
                        texel(&image.rgba, image.width, x, y),
                        other_colour,
                        "{path}: the row the strip does not draw reaches level {level} at ({x}, {y})"
                    );
                }
            }
        }
    }
}

#[test]
fn the_drawn_sprite_of_a_strip_is_its_first_frame() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas = build_atlas(
        &set,
        &paths(&["blocks/red", "blocks/strip", "blocks/reversed"]),
        &no_items(),
        4,
    )
    .expect("the tree stitches");

    // A still path draws from its stitched sprite.
    assert_eq!(atlas.drawn("blocks/red"), &atlas.sprites["blocks/red"]);

    // An animated path draws from its first frame, never the strip:
    // `TextureAtlasSprite.loadSprite` sets the sprite's height to its width
    // for an animation (`:289-294`), so every quad's rect is one frame's, and
    // the strip's taller content rect reaches no quad.
    let strip = atlas.sprites["blocks/strip"];
    let frames = &atlas.animated["blocks/strip"].frames;
    assert_eq!(atlas.drawn("blocks/strip"), &frames[0]);
    assert_eq!(
        atlas.drawn("blocks/strip").content,
        SpriteRect {
            x: strip.content.x,
            y: strip.content.y,
            w: 16,
            h: 16
        },
        "the drawn rect is the strip's first row, one frame tall"
    );
    assert_ne!(
        atlas.uv(atlas.drawn("blocks/strip")),
        atlas.uv(&strip),
        "the frame's uv pair is not the whole strip's"
    );

    // The out-of-order strip draws its own first playback entry — the
    // strip's second row — so the answer follows the frame list, not the
    // strip's pixel order.
    let reversed = &atlas.animated["blocks/reversed"].frames;
    assert_eq!(atlas.drawn("blocks/reversed"), &reversed[0]);
    assert_eq!(
        atlas.drawn("blocks/reversed").content.y,
        atlas.sprites["blocks/reversed"].content.y + 16,
        "the reversed strip's first frame is its second row"
    );

    // A path the atlas never stitched falls back to the missing sprite.
    assert_eq!(atlas.drawn("blocks/absent"), &atlas.missing);
}

#[test]
fn the_missing_sprite_is_the_generated_checkerboard() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let atlas =
        build_atlas(&set, &paths(&["blocks/red"]), &no_items(), 4).expect("the tree stitches");

    let missing = atlas.missing;
    assert_eq!(missing.region.w, 16);
    assert_eq!(missing.region.h, 16);
    assert_eq!(missing.content.w, 16);
    assert_eq!(missing.content.h, 16);
    assert_eq!(
        atlas.sprites["missingno"], missing,
        "the fallback is indexed under the client's name for it"
    );

    // A 16x16 checkerboard of magenta and black in 8-texel cells, the
    // client's own generator's pattern.
    let level = &atlas.levels[0];
    for y in 0..16 {
        for x in 0..16 {
            let expected = if ((x / 8) + (y / 8)) % 2 == 0 {
                MAGENTA
            } else {
                BLACK
            };
            assert_eq!(
                texel(
                    &level.rgba,
                    level.width,
                    missing.content.x + x,
                    missing.content.y + y
                ),
                expected,
                "the fallback at ({x}, {y})"
            );
        }
    }
}

#[test]
fn a_path_the_set_does_not_hold_is_an_error_unless_it_is_the_fallback() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");

    let error = build_atlas(
        &set,
        &paths(&["blocks/red", "blocks/ghost"]),
        &no_items(),
        4,
    )
    .expect_err("a path the set does not hold cannot stitch");
    assert!(
        matches!(error, AtlasError::MissingTexture { .. }),
        "the error says the texture is missing: {error}"
    );
    assert!(
        error.to_string().contains("blocks/ghost"),
        "the message names the path: {error}"
    );

    // The fallback's own name is not an error: the atlas carries it itself.
    let atlas = build_atlas(&set, &paths(&["blocks/red", "missingno"]), &no_items(), 4)
        .expect("the fallback's name resolves");
    assert_eq!(atlas.sprites["missingno"], atlas.missing);
    assert_eq!(
        atlas.sprites.len(),
        2,
        "blocks/red and missingno, and nothing else: {}",
        atlas.sprites.len()
    );
}

#[test]
fn a_frame_index_past_the_rows_is_an_error_naming_the_sprite() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/badidx.png",
        &solid_png(16, 16, RED),
    );
    tree.write(
        "assets/minecraft/textures/blocks/badidx.png.mcmeta",
        br#"{"animation":{"frames":[1]}}"#,
    );
    let set = TextureSet::load(tree.root()).expect("the tree loads");

    let error = build_atlas(&set, &paths(&["blocks/badidx"]), &no_items(), 4)
        .expect_err("row 1 does not exist in a one-row sprite");
    assert!(
        matches!(error, AtlasError::FrameIndex { .. }),
        "the error names the frame index: {error}"
    );
    assert!(
        error.to_string().contains("blocks/badidx"),
        "the message names the sprite: {error}"
    );
}

#[test]
fn a_strip_that_is_not_whole_rows_is_an_error_naming_the_sprite() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/ragged.png",
        &solid_png(16, 24, RED),
    );
    tree.write(
        "assets/minecraft/textures/blocks/ragged.png.mcmeta",
        br#"{"animation":{}}"#,
    );
    let set = TextureSet::load(tree.root()).expect("the tree loads");

    let error = build_atlas(&set, &paths(&["blocks/ragged"]), &no_items(), 4)
        .expect_err("24 texels are not a whole number of 16-texel rows");
    assert!(
        matches!(error, AtlasError::RaggedStrip { .. }),
        "the error says the strip is ragged: {error}"
    );
    assert!(
        error.to_string().contains("blocks/ragged"),
        "the message names the sprite: {error}"
    );
}

#[test]
fn a_sprite_that_cannot_fit_the_atlas_is_an_error() {
    // Two 4096x16 sprites: each pads to a 4096x4096 cell, and two of them
    // cannot share the largest atlas.
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/wide_a.png",
        &solid_png(4096, 16, RED),
    );
    tree.write(
        "assets/minecraft/textures/blocks/wide_b.png",
        &solid_png(4096, 16, BLUE),
    );
    let set = TextureSet::load(tree.root()).expect("the tree loads");

    let error = build_atlas(
        &set,
        &paths(&["blocks/wide_a", "blocks/wide_b"]),
        &no_items(),
        4,
    )
    .expect_err("two 4096-texel cells cannot share the atlas");
    assert!(
        matches!(error, AtlasError::TooLarge { .. }),
        "the error says the sprite is too large: {error}"
    );
    assert!(
        error.to_string().contains("blocks/wide"),
        "the message names the sprite: {error}"
    );
}

#[test]
fn the_same_input_builds_the_same_bytes() {
    let tree = stitching_tree();
    let set = TextureSet::load(tree.root()).expect("the tree loads");
    let requested = paths(&[
        "blocks/red",
        "blocks/blue",
        "blocks/slab",
        "blocks/strip",
        "blocks/reversed",
    ]);

    let first = build_atlas(&set, &requested, &no_items(), 4).expect("the first build");
    let second = build_atlas(&set, &requested, &no_items(), 4).expect("the second build");

    assert_eq!(
        layout_hash(&first),
        layout_hash(&second),
        "the same input set lays out the same bytes"
    );
}

/// The stand-in atlas an asset-less session meshes with: the fallback sprite
/// alone, at its cell, with one level.
#[test]
fn the_stand_in_atlas_is_the_fallback_sprite_alone() {
    let atlas = Atlas::fallback();
    assert_eq!((atlas.width, atlas.height, atlas.level_count), (16, 16, 1));
    assert_eq!(atlas.levels.len(), 1);
    assert_eq!(
        (atlas.levels[0].width, atlas.levels[0].height),
        (16, 16),
        "one level, the sprite's own cell"
    );
    assert_eq!(
        atlas.sprites.get("missingno").copied(),
        Some(atlas.missing),
        "the fallback is indexed under the client's own name for it"
    );
    assert_eq!(
        (atlas.missing.content.x, atlas.missing.content.y),
        (0, 0),
        "the sprite sits at its cell's top-left corner"
    );
    assert_eq!(
        (atlas.missing.content.w, atlas.missing.content.h),
        (16, 16),
        "the fallback is 16x16"
    );
    // The checkerboard's first texel is the light magenta the generator's own
    // tests pin (`TextureUtil.java:363-373`), and its 8-texel neighbour is the
    // dark one: a flat image would pass neither.
    let texel = |x: usize, y: usize| {
        let offset = (y * 16 + x) * 4;
        atlas.levels[0].rgba[offset..offset + 4].to_vec()
    };
    assert_eq!(texel(0, 0), vec![0xf8, 0, 0xf8, 0xff]);
    assert_eq!(texel(8, 0), vec![0, 0, 0, 0xff]);
    assert_eq!(atlas.uv(&atlas.missing), [[0.0, 0.0], [1.0, 1.0]]);
}

/// The real extraction tree, once: every path the model tree resolves
/// stitches, the fallback is present among the sprites, the level count is
/// the setting's, and the survey's animated strips carry their frames.
/// Ignored by default because it needs the user's own store: `OXIDECRAFT_STORE`
/// must name the store root (the directory that holds `extracted/`), and the
/// test fails naming the variable when it is unset, so a run without a store
/// can never pass silently. The version under the store is the literal
/// `1.8.9`.
#[test]
#[ignore = "reads the real extraction tree; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_extraction_tree_stitches() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");

    let set = TextureSet::load(&root).expect("the real tree loads");
    let source = ModelSource::open(&root).expect("the real tree opens");
    let requested = source.texture_paths();
    assert_eq!(requested.len(), 376, "the survey's resolved path count");

    // The registry's sprite list, bounded here: this crate has no edge to the
    // item registry, so the test names a real item set of the tree — a sword, a
    // food and a plain sprite. The client hands the registry's own list in
    // beside the block set; every entry here is a real sheet of the store.
    let items = paths(&["items/diamond_sword", "items/apple", "items/stick"]);
    for path in &items {
        assert!(
            !requested.contains(path),
            "{path} is in both sets; the counts below would double-count it"
        );
    }

    let atlas = build_atlas(&set, &requested, &items, 4).expect("the real tree stitches");

    // Every resolved path is a sprite, every item sprite is stitched beside it,
    // plus the fallback under its own key.
    assert_eq!(
        atlas.sprites.len(),
        requested.len() + items.len() + 1,
        "the resolved paths, the item sprites and the fallback"
    );
    for path in &requested {
        assert!(
            atlas.sprites.contains_key(path),
            "{path} is resolved but not in the atlas"
        );
    }
    for path in &items {
        assert!(
            atlas.sprites.contains_key(path),
            "{path} is an item sprite but not in the atlas"
        );
    }
    assert!(atlas.sprites.contains_key("missingno"));

    // The all-16x16 path set keeps the default setting's four reductions,
    // five images.
    assert_eq!(atlas.level_count, 5);
    assert_eq!(
        (atlas.width, atlas.height),
        (2048, 2048),
        "the first-fit atlas for the real tree"
    );
    println!(
        "atlas: {}x{}, {} levels",
        atlas.width, atlas.height, atlas.level_count
    );
    println!(
        "sprites: {}, animated: {}",
        atlas.sprites.len(),
        atlas.animated.len()
    );

    // The real sidecars, pinned: the block tree's nine `blocks/` sidecars —
    // empty `animation` objects read as the identity strips, and explicit
    // lists keep their playback order.
    let animated: Vec<&str> = atlas.animated.keys().map(String::as_str).collect();
    assert_eq!(
        animated,
        [
            "blocks/fire_layer_0",
            "blocks/fire_layer_1",
            "blocks/lava_flow",
            "blocks/lava_still",
            "blocks/portal",
            "blocks/prismarine_rough",
            "blocks/sea_lantern",
            "blocks/water_flow",
            "blocks/water_still",
        ],
        "the tree's nine sidecars"
    );
    let water = &atlas.animated["blocks/water_still"];
    assert_eq!(water.frames.len(), 32, "the 16x512 water strip");
    assert_eq!(water.times, [2; 32], "frametime 2");
    let lava = &atlas.animated["blocks/lava_still"];
    assert_eq!(lava.frames.len(), 38, "the sidecar's 38 playback entries");
    assert_eq!(lava.frames[0].content.h, 16);
    assert_eq!(
        lava.frames[0].content.y,
        atlas.sprites["blocks/lava_still"].content.y
    );
    assert_eq!(
        lava.frames[1].content.y,
        atlas.sprites["blocks/lava_still"].content.y + 16
    );
    let fire = &atlas.animated["blocks/fire_layer_0"];
    assert_eq!(fire.frames.len(), 32);
    assert_eq!(
        fire.frames[0].content.y,
        atlas.sprites["blocks/fire_layer_0"].content.y + 16 * 16
    );
    let prismarine = &atlas.animated["blocks/prismarine_rough"];
    assert_eq!(prismarine.frames.len(), 22);
    assert!(prismarine.interpolate);
    assert_eq!(prismarine.times, [300; 22]);
}

/// The destroy stages the crack draws, straight out of the real tree: the ten
/// `blocks/destroy_stage_{0..9}` sprites are stitched because the model bakery's builtin
/// texture list carries them (`BUILTIN_TEXTURE_LOCATIONS`,
/// `crates/oxide-assets/src/model.rs:80-89`, folded into the requested set at `:665-668`),
/// and the reference registers the same ten icons (`RenderGlobal.java:210-212`) for
/// `TextureMap.loadTextureAtlas` to stitch (`TextureMap.java:81`). Each is a full 16x16
/// cell, and `Atlas::drawn` — the lookup the crack's stage table resolves through —
/// answers the stage's own sprite rather than the fallback.
/// Ignored by default like [`the_real_extraction_tree_stitches`], and for the same reason:
/// it needs the user's own store, named by `OXIDECRAFT_STORE`, and fails naming the
/// variable when it is unset.
#[test]
#[ignore = "reads the real extraction tree; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_extraction_tree_stitches_the_destroy_stages() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");
    let set = TextureSet::load(&root).expect("the real tree loads");
    let source = ModelSource::open(&root).expect("the real tree opens");
    let requested = source.texture_paths();
    let atlas = build_atlas(&set, &requested, &no_items(), 4).expect("the real tree stitches");

    for stage in 0..10 {
        let path = format!("blocks/destroy_stage_{stage}");
        assert!(
            requested.contains(&path),
            "{path} is not in the model tree's requested paths"
        );
        assert!(atlas.sprites.contains_key(&path), "{path} is not stitched");
        let sprite = atlas.drawn(&path);
        // A 16x16 sprite keeps a 16x16 padded cell, so the content and the region agree.
        assert_eq!(sprite.content.w, 16, "{path}'s width");
        assert_eq!(sprite.content.h, 16, "{path}'s height");
        assert_eq!(sprite.region.w, 16, "{path}'s cell width");
        assert_eq!(sprite.region.h, 16, "{path}'s cell height");
        assert!(
            sprite.content.x + sprite.content.w <= atlas.width
                && sprite.content.y + sprite.content.h <= atlas.height,
            "{path}'s rect is inside the atlas"
        );
        // `drawn` resolves the stage's own sprite, not the fallback: the uv rect the crack
        // draws the stage with is the stitched sprite's own.
        let stitched = &atlas.sprites[&path];
        assert_eq!(
            atlas.uv(sprite),
            atlas.uv(stitched),
            "{path}'s resolved rect"
        );
        assert_ne!(
            atlas.uv(stitched),
            atlas.uv(&atlas.missing),
            "{path} resolved to the fallback sprite"
        );
    }
}

/// Writes the shared synthetic tree the stitching tests build from.
///
/// Two 16x16 sprites with distinct flat colours, one 32x32, an identity strip
/// whose sidecar states frametime 2 and interpolation, an out-of-order
/// strip whose sidecar lists its second row first and gives that frame its
/// own time, and one 16x16 item sheet — the sprite the item set stitches
/// beside the block set.
fn stitching_tree() -> Tree {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/red.png",
        &solid_png(16, 16, RED),
    );
    tree.write(
        "assets/minecraft/textures/blocks/blue.png",
        &solid_png(16, 16, BLUE),
    );
    tree.write(
        "assets/minecraft/textures/blocks/slab.png",
        &solid_png(32, 32, GREEN),
    );
    tree.write(
        "assets/minecraft/textures/blocks/strip.png",
        &rows_png(16, &blocks(&[(16, YELLOW), (16, CYAN)])),
    );
    tree.write(
        "assets/minecraft/textures/blocks/strip.png.mcmeta",
        br#"{"animation":{"frametime":2,"interpolate":true}}"#,
    );
    tree.write(
        "assets/minecraft/textures/blocks/reversed.png",
        &rows_png(16, &blocks(&[(16, ORANGE), (16, PURPLE)])),
    );
    tree.write(
        "assets/minecraft/textures/blocks/reversed.png.mcmeta",
        br#"{"animation":{"frametime":5,"frames":[{"index":1,"time":7},0]}}"#,
    );
    tree.write(
        "assets/minecraft/textures/items/widget.png",
        &solid_png(16, 16, AZURE),
    );
    tree
}

/// A path set from literal path names.
fn paths(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// The empty item set: the item-sprite input beside the block set that the
/// cases not about items stitch with.
fn no_items() -> BTreeSet<String> {
    BTreeSet::new()
}

/// The row colours of a strip built from `(count, colour)` blocks, top block
/// first.
fn blocks(blocks: &[(usize, [u8; 4])]) -> Vec<[u8; 4]> {
    let mut colours = Vec::new();
    for (count, colour) in blocks {
        colours.extend(std::iter::repeat_n(*colour, *count));
    }
    colours
}

/// The RGBA texel at `(x, y)` of an image `width` texels wide.
fn texel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    rgba[at..at + 4].try_into().expect("four channels")
}

/// True when `(x, y)` lies inside `rect`.
fn inside(rect: SpriteRect, x: u32, y: u32) -> bool {
    x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h
}

/// True when a texel mixes red and blue: some of each, one of them partial.
fn is_red_blue_mix([r, _, b, _]: [u8; 4]) -> bool {
    r != 0 && b != 0 && (r != 255 || b != 255)
}

/// The rect `level` reductions down: each axis halved.
fn shift(rect: SpriteRect, level: u32) -> SpriteRect {
    SpriteRect {
        x: rect.x >> level,
        y: rect.y >> level,
        w: rect.w >> level,
        h: rect.h >> level,
    }
}

/// Asserts the sprite's level-0 pixels are exactly `colours` row for row,
/// each row `width` texels, in the right place.
fn assert_content_is(atlas: &Atlas, path: &str, width: u32, colours: &[[u8; 4]]) {
    let sprite = atlas
        .sprites
        .get(path)
        .unwrap_or_else(|| panic!("{path} must be present"));
    assert_eq!(sprite.content.w, width, "{path}'s width");
    assert_eq!(sprite.content.h as usize, colours.len(), "{path}'s height");
    let image = &atlas.levels[0];
    for (y, colour) in colours.iter().enumerate() {
        for x in 0..width {
            let got = texel(
                &image.rgba,
                image.width,
                sprite.content.x + x,
                sprite.content.y + y as u32,
            );
            assert_eq!(&got, colour, "{path}'s texel at ({x}, {y})");
        }
    }
}

/// Asserts every texel of `rect` in `image` is exactly `colour`.
fn assert_rect_is(image: &AtlasLevel, rect: SpriteRect, colour: [u8; 4], label: &str) {
    assert!(rect.w > 0 && rect.h > 0, "{label} has no texels");
    for y in rect.y..rect.y + rect.h {
        for x in rect.x..rect.x + rect.w {
            assert_eq!(texel(&image.rgba, image.width, x, y), colour, "{label}");
        }
    }
}

/// True when two regions share any texel.
fn overlaps(a: SpriteRect, b: SpriteRect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Asserts no two sprite regions share a texel.
fn assert_no_overlapping_regions(atlas: &Atlas) {
    let regions: Vec<(&str, SpriteRect)> = atlas
        .sprites
        .iter()
        .map(|(path, sprite)| (path.as_str(), sprite.region))
        .collect();
    for (index, (path, region)) in regions.iter().enumerate() {
        for (other_path, other) in &regions[index + 1..] {
            assert!(
                !overlaps(*region, *other),
                "{path} and {other_path} overlap: {region:?} and {other:?}"
            );
        }
    }
}

/// A hash of everything the layout decides: the level images, the sprite
/// rects and the animated frame rects.
fn layout_hash(atlas: &Atlas) -> u64 {
    let mut hasher = DefaultHasher::new();
    atlas.width.hash(&mut hasher);
    atlas.height.hash(&mut hasher);
    atlas.level_count.hash(&mut hasher);
    for level in &atlas.levels {
        level.width.hash(&mut hasher);
        level.height.hash(&mut hasher);
        level.rgba.hash(&mut hasher);
    }
    for (path, sprite) in &atlas.sprites {
        path.hash(&mut hasher);
        (
            sprite.region.x,
            sprite.region.y,
            sprite.region.w,
            sprite.region.h,
        )
            .hash(&mut hasher);
        (
            sprite.content.x,
            sprite.content.y,
            sprite.content.w,
            sprite.content.h,
        )
            .hash(&mut hasher);
    }
    for (path, animated) in &atlas.animated {
        path.hash(&mut hasher);
        animated.times.hash(&mut hasher);
        animated.interpolate.hash(&mut hasher);
        for frame in &animated.frames {
            (
                frame.content.x,
                frame.content.y,
                frame.content.w,
                frame.content.h,
            )
                .hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// A synthetic extraction tree in a temp directory, removed on drop.
struct Tree {
    /// Keeps the temp directory alive for the test's duration.
    _dir: tempfile::TempDir,
    /// The extraction root the tree's files live under.
    root: PathBuf,
}

impl Tree {
    /// An empty tree in a fresh temp directory.
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp directory");
        let root = dir.path().to_path_buf();
        Self { _dir: dir, root }
    }

    /// Writes `bytes` at `relative` under the extraction root, creating any
    /// parent directories.
    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the tree's directories");
        }
        fs::write(&path, bytes).expect("the tree's files");
    }

    /// The extraction root to hand [`TextureSet::load`].
    fn root(&self) -> &Path {
        &self.root
    }
}
