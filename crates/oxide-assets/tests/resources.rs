//! The texture-set tests: a synthetic extraction tree in a temp directory,
//! laid out the way the extractor lays out a real one
//! (`<root>/assets/minecraft/textures/...`).
//!
//! The tree's PNGs are built by the small writer at the bottom of this file,
//! so no file from the game is involved. The writer is a second copy of the
//! one `tests/texture.rs` carries: each integration test is a crate of its
//! own and cannot reach another one's helpers without a module file of its
//! own.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        &png(2, 2, &STONE_TEXELS),
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

    // The water sidecar states frametime 2; the real tree's sidecars attach.
    let water = set
        .animation("blocks/water_still")
        .expect("the water sidecar parsed");
    assert_eq!(water.frametime, 2, "the sidecar states frametime 2");
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

/// Builds an 8-bit RGBA PNG whose every texel is `rgba`, with the writer
/// below.
fn solid_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..width as usize * height as usize {
        pixels.extend_from_slice(&rgba);
    }
    png(width, height, &pixels)
}

/// Builds an 8-bit RGBA PNG of `width` x `height` texels from raw pixel bytes
/// in row-major order.
///
/// Every scanline is written with filter type `None`; the scanlines go into a
/// zlib stream of stored (uncompressed) blocks; every chunk's CRC and the
/// stream's Adler-32 are computed here, so the bytes are a complete PNG built
/// without an encoder.
fn png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    assert_eq!(
        pixels.len(),
        width as usize * height as usize * 4,
        "the fixture must supply whole RGBA scanlines"
    );

    let mut scanlines = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(width as usize * 4) {
        scanlines.push(0); // the filter type: None
        scanlines.extend_from_slice(row);
    }

    let mut out = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a].to_vec();
    out.extend_from_slice(&ihdr(width, height));
    out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&scanlines)));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

/// The IHDR chunk of an 8-bit RGBA image: dimensions, colour type 6, and the
/// three method bytes, all zero (no compression, adaptive filtering, no
/// interlace).
fn ihdr(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(13);
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &data)
}

/// One PNG chunk: length, type, data and the CRC of type and data.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);

    let mut out = Vec::with_capacity(12 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&crc_input);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// Wraps `raw` in a zlib stream: the header, stored deflate blocks of at most
/// 65535 bytes each (the last one marked as final), and the Adler-32.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];

    let mut offset = 0;
    loop {
        let end = (offset + 65_535).min(raw.len());
        let block = &raw[offset..end];
        let last = end == raw.len();

        out.push(u8::from(last)); // BFINAL in bit 0, BTYPE 00: stored
        let length = block.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(block);

        offset = end;
        if last {
            break;
        }
    }

    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// The CRC-32 PNG chunks carry: the reflected polynomial `0xedb88320`.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// The Adler-32 of `bytes`, the checksum a zlib stream ends with.
fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in bytes {
        a = (a + u32::from(byte)) % MODULUS;
        b = (b + a) % MODULUS;
    }
    (b << 16) | a
}
