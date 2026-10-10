//! The texture-set tests: a synthetic extraction tree in a temp directory,
//! laid out the way the extractor lays out a real one
//! (`<root>/assets/minecraft/textures/...`).
//!
//! The tree's PNGs are built by the shared writer in `common.rs`, so no file
//! from the game is involved. Each integration test is a crate of its own
//! and reaches the shared module with `mod common;`.

mod common;

use common::{rgba_png, solid_png};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use oxide_assets::font::Font;
use oxide_assets::resources::{ResourceError, TextureSet};

/// The four texels of the synthetic `blocks/stone` fixture, in row-major
/// order, including a transparent one.
const STONE_TEXELS: [u8; 16] = [
    1, 2, 3, 255, //
    4, 5, 6, 254, //
    7, 8, 9, 0, //
    10, 11, 12, 128,
];

#[test]
fn a_synthetic_tree_loads_its_textures_and_sidecars() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/stone.png",
        &rgba_png(2, 2, &STONE_TEXELS),
    );
    tree.write(
        "assets/minecraft/textures/blocks/stone.png.mcmeta",
        br#"{"animation":{"frametime":2,"frames":[0,1,{"index":0,"time":7}]}}"#,
    );
    tree.write(
        "assets/minecraft/textures/font/ascii.png",
        &solid_png(1, 1, [200, 200, 200, 255]),
    );
    // The real tree carries four sidecars that state a `texture` section and
    // no `animation` object; they parse to the default meta and attach.
    tree.write(
        "assets/minecraft/textures/misc/shadow.png",
        &solid_png(1, 1, [0, 0, 0, 128]),
    );
    tree.write(
        "assets/minecraft/textures/misc/shadow.png.mcmeta",
        br#"{"texture":{"clamp":true}}"#,
    );

    let set = TextureSet::load(tree.root()).expect("the tree loads");

    let stone = set.get("blocks/stone").expect("blocks/stone is loaded");
    assert_eq!(stone.width, 2);
    assert_eq!(stone.height, 2);
    assert_eq!(stone.rgba, STONE_TEXELS);

    assert_eq!(
        set.get("font/ascii").expect("font/ascii is loaded").width,
        1
    );
    assert!(
        set.get("blocks/dirt").is_none(),
        "a path the tree does not hold is None"
    );
    assert!(
        set.animation("font/ascii").is_none(),
        "a texture without a sidecar has no animation"
    );

    let meta = set.animation("blocks/stone").expect("the sidecar parsed");
    assert_eq!(meta.frametime, 2);
    assert_eq!(meta.frames, [0, 1, 0]);
    assert_eq!(meta.frame_times, [None, None, Some(7)]);
    assert!(!meta.interpolate);

    let shadow = set
        .animation("misc/shadow")
        .expect("a texture-section sidecar attaches");
    assert_eq!(shadow.frametime, 1);
    assert!(shadow.frames.is_empty());
    assert!(shadow.frame_times.is_empty());
    assert!(!shadow.interpolate);
}

#[test]
fn an_invalid_sidecar_fails_the_load_and_names_the_file() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/stone.png",
        &solid_png(1, 1, [1, 2, 3, 255]),
    );
    tree.write(
        "assets/minecraft/textures/blocks/stone.png.mcmeta",
        b"{ not json at all",
    );

    let error = TextureSet::load(tree.root()).expect_err("an unreadable sidecar fails the load");
    assert!(matches!(error, ResourceError::Mcmeta { .. }));
    let message = error.to_string();
    assert!(
        message.contains("stone.png.mcmeta"),
        "the message names the file: {message}"
    );
}

#[test]
fn a_sidecar_without_its_texture_fails_the_load_and_names_the_file() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/ghost.png.mcmeta",
        br#"{"animation":{}}"#,
    );

    let error =
        TextureSet::load(tree.root()).expect_err("a sidecar with no texture fails the load");
    assert!(matches!(error, ResourceError::OrphanSidecar { .. }));
    let message = error.to_string();
    assert!(
        message.contains("ghost.png.mcmeta"),
        "the message names the file: {message}"
    );
}

#[test]
fn a_texture_that_does_not_decode_fails_the_load_and_names_the_file() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/blocks/broken.png",
        b"not a PNG file",
    );

    let error = TextureSet::load(tree.root()).expect_err("a broken texture fails the load");
    assert!(matches!(error, ResourceError::Texture { .. }));
    let message = error.to_string();
    assert!(
        message.contains("broken.png"),
        "the message names the file: {message}"
    );
}

#[test]
fn the_unicode_font_pages_are_skipped() {
    let tree = Tree::new();
    tree.write(
        "assets/minecraft/textures/font/ascii.png",
        &solid_png(1, 1, [200, 200, 200, 255]),
    );
    tree.write(
        "assets/minecraft/textures/font/unicode_page_00.png",
        &solid_png(1, 1, [0, 0, 0, 255]),
    );
    tree.write(
        "assets/minecraft/textures/font/unicode_page_00.png.mcmeta",
        br#"{"animation":{}}"#,
    );

    let set = TextureSet::load(tree.root()).expect("a tree with pages loads");

    assert!(
        set.get("font/unicode_page_00").is_none(),
        "the page is skipped"
    );
    assert!(set.animation("font/unicode_page_00").is_none());
    assert!(
        set.get("font/ascii").is_some(),
        "the pages' neighbours under font/ still load"
    );
}

/// The real tree, once: load the extraction tree a store holds and check it
/// against the survey's floors.
///
/// Ignored by default because it needs the user's own store: `OXIDECRAFT_STORE`
/// must name the store root (the directory that holds `extracted/`), and the
/// test fails naming the variable when it is unset, so a run without a store
/// can never pass silently. The version under the store is the literal
/// `1.8.9`.
///
/// The floors are floors, not equalities. They sit below the tree's census on
/// purpose: the tree holds 373 block PNGs and 227 item PNGs (checked against
/// the store and the jar, which agree file-for-file), while the survey's
/// 382/229 counts the `.mcmeta` sidecars alongside the PNGs. A fuller tree
/// passes as well.
#[test]
#[ignore = "reads the real extraction tree; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_extraction_tree_loads() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");
    let textures = root.join("assets/minecraft/textures");

    let set = TextureSet::load(&root).expect("the real tree loads");
    assert!(
        set.get("font/unicode_page_00").is_none(),
        "the unicode pages are skipped in the real tree too"
    );

    // Every PNG the tree holds must be in the set, counted by its top-level
    // directory, except the skipped pages.
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in files_under(&textures) {
        if path.extension().and_then(|extension| extension.to_str()) != Some("png") {
            continue;
        }
        let relative = path
            .strip_prefix(&textures)
            .expect("the walk starts at the texture tree");
        let key = relative
            .with_extension("")
            .to_str()
            .expect("the tree's names are UTF-8")
            .to_string();
        if key.starts_with("font/unicode_page_") {
            assert!(
                set.get(&key).is_none(),
                "{key} is a page and must be skipped"
            );
            continue;
        }
        assert!(
            set.get(&key).is_some(),
            "{key} is on disk but not in the set"
        );
        let group = key
            .split('/')
            .next()
            .expect("a texture key has a top-level directory");
        *counts.entry(group.to_string()).or_default() += 1;
    }

    let count = |group: &str| counts.get(group).copied().unwrap_or(0);
    assert!(
        count("blocks") >= 370,
        "at least 370 block textures, counted {}",
        count("blocks")
    );
    assert!(
        count("items") >= 225,
        "at least 225 item textures, counted {}",
        count("items")
    );
    assert!(
        count("environment") >= 6,
        "at least 6 environment textures, counted {}",
        count("environment")
    );
    assert!(
        count("colormap") >= 2,
        "at least 2 colormaps, counted {}",
        count("colormap")
    );
    assert!(
        count("font") >= 1,
        "at least the ascii font texture besides the skipped pages, counted {}",
        count("font")
    );

    for key in [
        "blocks/stone",
        "blocks/water_still",
        "blocks/grass_top",
        "colormap/grass",
        "font/ascii",
    ] {
        assert!(
            set.get(key).is_some(),
            "{key} must be in the real tree's set"
        );
    }

    // Every sheet the client registers with the hud pass is in the tree — the
    // gui sheets, the book, the SGA glyph sheet and the chest trio's icon
    // sheets, one real container sheet among them. A wrong key fails here,
    // naming it, rather than loading silently.
    for key in oxide_assets::resources::GUI_SHEETS {
        assert!(
            set.get(key).is_some(),
            "the gui sheet {key} is not in the real tree's set"
        );
    }
    assert!(
        set.get("gui/container/generic_54").is_some(),
        "the generic container frame is in the real tree's set"
    );

    // The water sidecar states frametime 2; the real tree's sidecars attach.
    let water = set
        .animation("blocks/water_still")
        .expect("the water sidecar parsed");
    assert_eq!(water.frametime, 2, "the sidecar states frametime 2");
}

/// The real font sheet: `font/ascii` loads and the ink-column scan agrees with the width the
/// store census recorded for `'A'`.
///
/// Ignored by default because it needs the user's own store, exactly like
/// [`the_real_extraction_tree_loads`]: `OXIDECRAFT_STORE` must name the store root.
#[test]
#[ignore = "reads the real extraction tree; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_font_sheet_loads_and_measures() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");
    let set = TextureSet::load(&root).expect("the real tree loads");

    let sheet = set
        .get("font/ascii")
        .expect("font/ascii is in the real tree");
    assert_eq!(
        (sheet.width, sheet.height),
        (128, 128),
        "the ascii sheet is the 16x16 grid of 8-texel cells"
    );
    let font = Font::load(sheet, None).expect("the real sheet loads");
    assert_eq!(font.advance('A'), 6, "the store-verified width of 'A'");
    assert_eq!(font.height(), 9);
}

/// Every file under `dir`, recursively. The test's own walk, independent of
/// the loader's.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).expect("the real tree's directories") {
            let path = entry.expect("the real tree's entries").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
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
