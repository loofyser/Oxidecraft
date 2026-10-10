//! The mesh core over synthetic store columns and a synthetic model tree.
//!
//! Every expectation is a literal: the quad counts, the atlas corners, the
//! per-vertex light pairs and the tint bytes. The worlds are [`ColumnData`]
//! values through [`World::apply_column`], and the models come from a temporary
//! extraction tree the tests write themselves — the same layout the project's
//! extractor emits, flattened files with `normal` keys — so nothing here reads
//! the real game assets.

use std::collections::BTreeMap;

use oxide_assets::atlas::{AnimatedSprite, Atlas, AtlasLevel, AtlasSprite, SpriteRect};
use oxide_assets::model::ModelSource;
use oxide_game::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, ModelChoice, SmoothLighting, blockstate_target,
    build_column_meshes,
};
use oxide_proto_v47::column::{ColumnData, SectionData};
use oxide_render::terrain::{ChunkMesh, Layer, LayerMesh, Vertex};
use oxide_world::behaviour::{behaviour, covered_ids};
use oxide_world::biome::{ColorMap, TintMaps};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::world::World;

/// The block ids the tests use.
const STONE: u16 = 1;
const GRASS: u16 = 2;
const DIRT: u16 = 3;
const PLANKS: u16 = 5;
const WATER: u16 = 8;
const LEAVES: u16 = 18;
const GLASS: u16 = 20;
const LOG: u16 = 17;
const TALLGRASS: u16 = 31;
const SPAWNER: u16 = 52;
const STAIRS: u16 = 67;
const GLOWSTONE: u16 = 89;
const SNOW_LAYER: u16 = 78;
const BARRIER: u16 = 166;
const SIGN_STANDING: u16 = 63;
const SIGN_WALL: u16 = 68;

/// One state mapper arm's expectation: the block id, the metadata value, and
/// the literal `(file, key)` the mapper must answer — `None` for the six ids
/// the client builds in.
type MapperArm = (u16, u8, Option<(&'static str, &'static str)>);

/// Asserts each listed state's mapped `(file, key)` against its literal.
fn assert_targets(arms: &[MapperArm]) {
    for &(id, meta, expected) in arms {
        let row = behaviour(id).unwrap_or_else(|| panic!("id {id} is covered"));
        let answer = blockstate_target(row, meta);
        let expected = expected.map(|(file, key)| (file.to_string(), key.to_string()));
        assert_eq!(answer, expected, "id {id} meta {meta}");
    }
}

/// One wire block value: the id and its metadata.
fn state(id: u16, meta: u8) -> u16 {
    (id << 4) | u16::from(meta)
}

// -- the world side ---------------------------------------------------------

/// The linear index of a cell inside its section.
fn cell(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

/// Writes one light level into a nibble array.
fn set_nibble(array: &mut [u8; 2048], index: usize, level: u8) {
    let slot = &mut array[index / 2];
    *slot = if index % 2 == 0 {
        (*slot & 0xF0) | (level & 0x0F)
    } else {
        (*slot & 0x0F) | ((level & 0x0F) << 4)
    };
}

/// One column: sixteen sections, air but for `blocks`, one sky and block light
/// per cell, and one biome per cell.
fn column(
    blocks: &[(usize, usize, usize, u16)],
    biome: impl Fn(usize, usize) -> u8,
    light: impl Fn(usize, usize, usize) -> (u8, u8),
) -> ColumnData {
    let mut sections: [Option<SectionData>; SECTION_COUNT] = std::array::from_fn(|_| None);
    for (index, slot) in sections.iter_mut().enumerate() {
        let mut grid = Box::new([0u16; 4096]);
        for (x, y, z, id) in blocks {
            if y >> 4 == index {
                grid[cell(*x, y & 15, *z)] = *id;
            }
        }
        let mut block_light = Box::new([0u8; 2048]);
        let mut sky_light = Box::new([0u8; 2048]);
        for local in 0..4096 {
            let y = local >> 8;
            let (sky, block) = light(local & 15, index * SECTION_SIZE + y, (local >> 4) & 15);
            set_nibble(&mut sky_light, local, sky);
            set_nibble(&mut block_light, local, block);
        }
        *slot = Some(SectionData {
            blocks: grid,
            block_light,
            sky_light: Some(sky_light),
        });
    }
    let biomes = std::array::from_fn(|index| biome(index % SECTION_SIZE, index / SECTION_SIZE));
    ColumnData {
        mask: 0xFFFF,
        sections,
        biomes: Some(biomes),
    }
}

/// One plains column in full daylight, no block light.
fn flat(blocks: &[(usize, usize, usize, u16)]) -> ColumnData {
    column(blocks, |_, _| 1, |_, _, _| (15, 0))
}

/// A world of the given columns, each keyed by its chunk coordinates.
fn world_of(columns: &[(i32, i32, ColumnData)]) -> World {
    let mut world = World::new(true);
    for (cx, cz, data) in columns {
        world.apply_column(*cx, *cz, data, true);
    }
    world
}

/// A single loaded column at the origin, in full daylight.
fn daylight(blocks: &[(usize, usize, usize, u16)]) -> World {
    world_of(&[(0, 0, flat(blocks))])
}

// -- the asset side ---------------------------------------------------------

/// A synthetic atlas: one 128 x 128 level, the fallback sprite first and one
/// sprite per name after it, each an 8 x 8 content rect inside a padded cell.
fn atlas(names: &[&str]) -> Atlas {
    let mut sprites = BTreeMap::new();
    for (index, name) in names.iter().enumerate() {
        sprites.insert(
            (*name).to_string(),
            AtlasSprite {
                region: SpriteRect {
                    x: 16 * index as u32,
                    y: 0,
                    w: 16,
                    h: 16,
                },
                content: SpriteRect {
                    x: 16 * index as u32 + 1,
                    y: 1,
                    w: 8,
                    h: 8,
                },
            },
        );
    }
    let missing = *sprites
        .get("missingno")
        .expect("the fallback sprite is first");
    Atlas {
        levels: vec![AtlasLevel {
            width: 128,
            height: 128,
            rgba: vec![0u8; 128 * 128 * 4],
        }],
        width: 128,
        height: 128,
        level_count: 1,
        sprites,
        animated: BTreeMap::new(),
        missing,
    }
}

/// The atlas the tests mesh with: the fallback sprite and the three the
/// synthetic models name.
fn test_atlas() -> Atlas {
    atlas(&[
        "missingno",
        "blocks/probe",
        "blocks/probe_overlay",
        "blocks/probe_cross",
    ])
}

/// The atlas with the probe cube's own texture as an animated strip: the
/// strip is a 16 x 32 content rect (two 16 x 16 frames) in its own 32 x 32
/// cell at (64, 0), past the still sprites' cells, with the fallback first.
///
/// The strip's v pair is twice the frame's, so a model quad that maps into
/// the strip spans both frames where the source spans one.
fn strip_probe_atlas() -> Atlas {
    let mut atlas = atlas(&["missingno", "blocks/probe_overlay", "blocks/probe_cross"]);
    let region = SpriteRect {
        x: 64,
        y: 0,
        w: 32,
        h: 32,
    };
    atlas.sprites.insert(
        "blocks/probe".to_string(),
        AtlasSprite {
            region,
            content: SpriteRect {
                x: 64,
                y: 0,
                w: 16,
                h: 32,
            },
        },
    );
    atlas.animated.insert(
        "blocks/probe".to_string(),
        AnimatedSprite {
            frames: (0..2)
                .map(|row| AtlasSprite {
                    region,
                    content: SpriteRect {
                        x: 64,
                        y: row * 16,
                        w: 16,
                        h: 16,
                    },
                })
                .collect(),
            times: vec![2, 2],
            interpolate: false,
        },
    );
    atlas
}

/// One sprite's content rect inside [`test_atlas`], as its two uv corners.
fn sprite_uv(name: &str) -> [[f32; 2]; 2] {
    let atlas = test_atlas();
    atlas.uv(&atlas.sprites[name])
}

/// The four corners of a sprite's content rect, in the client's own corner
/// order for a quad's vertices.
fn sprite_corners(name: &str) -> [[f32; 2]; 4] {
    let [min, max] = sprite_uv(name);
    [
        [min[0], min[1]],
        [min[0], max[1]],
        [max[0], max[1]],
        [max[0], min[1]],
    ]
}

/// Neutral colour maps: white everywhere.
fn white_maps() -> TintMaps {
    let neutral = || ColorMap::from_rgba(&[255u8; 256 * 256 * 4]).expect("a valid map");
    TintMaps {
        grass: neutral(),
        foliage: neutral(),
    }
}

/// Colour maps whose red channel is the temperature and green the humidity —
/// the colormap's own orientation, column first — so biomes tint differently.
fn gradient_maps() -> TintMaps {
    let mut rgba = Vec::with_capacity(256 * 256 * 4);
    for row in 0..256u32 {
        for column in 0..256u32 {
            rgba.extend_from_slice(&[column as u8, row as u8, 0, 255]);
        }
    }
    let gradient = || ColorMap::from_rgba(&rgba).expect("a valid map");
    TintMaps {
        grass: gradient(),
        foliage: gradient(),
    }
}

/// A synthetic extraction tree, laid out as the project's extractor emits it.
struct Tree {
    root: tempfile::TempDir,
}

impl Tree {
    fn new() -> Tree {
        let root = tempfile::tempdir().expect("a temporary directory");
        for directory in ["models/block", "blockstates"] {
            std::fs::create_dir_all(root.path().join("assets/minecraft").join(directory))
                .expect("the tree's directories");
        }
        Tree { root }
    }

    fn write(&self, name: &str, json: &str) {
        std::fs::write(
            self.root
                .path()
                .join("assets/minecraft")
                .join(format!("{name}.json")),
            json,
        )
        .expect("a tree file");
    }

    fn model(&self, name: &str, json: &str) {
        self.write(&format!("models/block/{name}"), json);
    }

    fn blockstates(&self, name: &str, json: &str) {
        self.write(&format!("blockstates/{name}"), json);
    }

    fn source(&self) -> ModelSource {
        ModelSource::open(self.root.path()).expect("the tree opens")
    }
}

/// A full cube: one element, all six faces, each culled against its own side.
fn cube(texture: &str, tint: bool) -> String {
    let mut faces = Vec::new();
    for face in ["down", "up", "north", "south", "west", "east"] {
        let tint = if tint && face == "up" {
            r#", "tintindex": 0"#
        } else {
            ""
        };
        faces.push(format!(
            r##""{face}": {{"texture": "#all", "cullface": "{face}"{tint}}}"##
        ));
    }
    format!(
        r##"{{"textures": {{"all": "{texture}"}}, "elements": [{{"from": [0, 0, 0], "to": [16, 16, 16], "faces": {{{}}}}}]}}"##,
        faces.join(", ")
    )
}

/// The model tree the mesh tests load: a probe cube per id, a tinted grass
/// model, a plant cross, and the planks' weighted alternatives.
fn probe_tree() -> Tree {
    let tree = Tree::new();
    let plain = cube("blocks/probe", false);
    tree.model("probe_cube", &plain);
    // Two elements of the same cube, so twelve quads: the pick's other arm.
    let element = &plain[plain.find('[').expect("an elements array") + 1..plain.len() - 2];
    tree.model(
        "probe_double",
        &format!(
            r#"{{"textures": {{"all": "blocks/probe"}}, "elements": [{element}, {element}]}}"#
        ),
    );

    // A grass model: an untinted cube whose top is tinted, plus a tinted side
    // overlay over it.
    let mut overlay = Vec::new();
    for face in ["north", "south", "west", "east"] {
        overlay.push(format!(
            r##""{face}": {{"texture": "#overlay", "cullface": "{face}", "tintindex": 0}}"##
        ));
    }
    let base = &cube("blocks/probe", true);
    let base_element = &base[base.find('[').expect("an elements array") + 1..base.len() - 2];
    tree.model(
        "probe_grass",
        &format!(
            r#"{{"textures": {{"all": "blocks/probe", "overlay": "blocks/probe_overlay"}}, "elements": [{base_element}, {{"from": [0, 0, 0], "to": [16, 16, 16], "faces": {{{}}}}}]}}"#,
            overlay.join(", ")
        ),
    );

    // A plant cross: two zero-thickness planes, no cullface, and the element's
    // own `shade` off, as the client's `cross` model does. No ambient
    // occlusion either.
    tree.model(
        "probe_cross",
        r##"{"ambientocclusion": false, "textures": {"all": "blocks/probe_cross"},
            "elements": [
                {"from": [0.8, 0, 8], "to": [15.2, 16, 8], "shade": false,
                 "faces": {"north": {"texture": "#all"},
                           "south": {"texture": "#all"}}},
                {"from": [8, 0, 0.8], "to": [8, 16, 15.2], "shade": false,
                 "faces": {"west": {"texture": "#all"},
                           "east": {"texture": "#all"}}}
            ]}"##,
    );

    let normal = |model: &str| {
        format!(r#"{{"variants": {{"normal": [{{"model": "minecraft:{model}"}}]}}}}"#)
    };
    tree.blockstates("stone", &normal("probe_cube"));
    tree.blockstates("glass", &normal("probe_cube"));
    tree.blockstates("oak_leaves", &normal("probe_cube"));
    tree.blockstates("mob_spawner", &normal("probe_cube"));
    tree.blockstates("glowstone", &normal("probe_cube"));
    tree.blockstates("tall_grass", &normal("probe_cross"));
    tree.blockstates("dirt", &normal("probe_cube"));

    // A stairs-like partial element: the lower step of the stairs model, a
    // half-height box whose four side faces span the y 0..8 band of the block
    // and are culled against their own sides. The stair state the tests place
    // is meta 0: east-facing, bottom half, straight.
    tree.model(
        "probe_step",
        r##"{"textures": {"all": "blocks/probe"}, "elements": [
            {"from": [0, 0, 0], "to": [16, 8, 16], "faces": {
                "down": {"texture": "#all", "cullface": "down"},
                "north": {"texture": "#all", "cullface": "north"},
                "south": {"texture": "#all", "cullface": "south"},
                "west": {"texture": "#all", "cullface": "west"},
                "east": {"texture": "#all", "cullface": "east"}}}]}"##,
    );
    tree.blockstates(
        "stone_stairs",
        r#"{"variants": {"facing=east,half=bottom,shape=straight": [{"model": "minecraft:probe_step"}]}}"#,
    );
    tree.blockstates(
        "grass",
        r#"{"variants": {"snowy=false": [{"model": "minecraft:probe_grass"}]}}"#,
    );
    tree.blockstates(
        "oak_planks",
        r#"{"variants": {
            "normal": [{"model": "minecraft:probe_cube", "weight": 2},
                       {"model": "minecraft:probe_double", "weight": 3}],
            "spruce": [{"model": "minecraft:probe_cube", "weight": 0},
                       {"model": "minecraft:probe_double", "weight": 0}]
        }}"#,
    );
    // The planks' zero-weight arm needs its own file, because both arms share
    // the `normal` key: id 5's meta 1 names the spruce file.
    std::fs::rename(
        tree.root
            .path()
            .join("assets/minecraft/blockstates/oak_planks.json"),
        tree.root
            .path()
            .join("assets/minecraft/blockstates/spruce_planks.json"),
    )
    .expect("the rename");
    tree.blockstates(
        "oak_planks",
        r#"{"variants": {
            "normal": [{"model": "minecraft:probe_cube", "weight": 2},
                       {"model": "minecraft:probe_double", "weight": 3}]
        }}"#,
    );
    tree.blockstates(
        "spruce_planks",
        r#"{"variants": {
            "normal": [{"model": "minecraft:probe_cube", "weight": 0},
                       {"model": "minecraft:probe_double", "weight": 0}]
        }}"#,
    );

    // The snow layer's thin slab, the snow_height2 model's own element: 2/16
    // high, the down and up faces on the sprite's full rect and the sides on
    // its bottom band, each culled against its own side (the up face is not).
    // `layers=1` names it in the real blockstate file; `layers=8` names the
    // full cube model.
    tree.model(
        "probe_snow",
        r##"{"textures": {"all": "blocks/probe"}, "elements": [
            {"from": [0, 0, 0], "to": [16, 2, 16], "faces": {
                "down": {"uv": [0, 0, 16, 16], "texture": "#all", "cullface": "down"},
                "up": {"uv": [0, 0, 16, 16], "texture": "#all"},
                "north": {"uv": [0, 14, 16, 16], "texture": "#all", "cullface": "north"},
                "south": {"uv": [0, 14, 16, 16], "texture": "#all", "cullface": "south"},
                "west": {"uv": [0, 14, 16, 16], "texture": "#all", "cullface": "west"},
                "east": {"uv": [0, 14, 16, 16], "texture": "#all", "cullface": "east"}}}]}"##,
    );
    tree.blockstates(
        "snow_layer",
        r#"{"variants": {
            "layers=1": [{"model": "minecraft:probe_snow"}],
            "layers=8": [{"model": "minecraft:probe_cube"}]
        }}"#,
    );
    tree
}

/// The models and the atlas the mesh tests use.
fn loaded() -> (BlockModelSet, Atlas) {
    let tree = probe_tree();
    let models = BlockModelSet::load(&tree.source());
    (models, test_atlas())
}

/// The context over one loaded set, with the given smooth-lighting setting.
fn context<'a>(
    models: &'a BlockModelSet,
    atlas: &'a Atlas,
    maps: &'a TintMaps,
    smooth_lighting: SmoothLighting,
) -> MeshContext<'a> {
    MeshContext {
        models,
        atlas,
        tint_maps: maps,
        graphics_fast: true,
        smooth_lighting,
    }
}

/// Every section's mesh for the column at the origin.
fn meshes(world: &World, ctx: &MeshContext<'_>) -> Vec<(usize, Option<ChunkMesh>)> {
    let snapshot = ColumnSnapshot::from_world(world, 0, 0);
    build_column_meshes(&snapshot, ctx)
}

/// The one loaded section's mesh.
fn mesh_of(world: &World, ctx: &MeshContext<'_>) -> ChunkMesh {
    meshes(world, ctx)
        .into_iter()
        .find_map(|(_, mesh)| mesh)
        .expect("a loaded section")
}

/// One layer's quads, as slices of four vertices.
fn layer_quads(layer: &LayerMesh) -> Vec<&[Vertex]> {
    layer.vertices.chunks(4).collect()
}

/// A mesh's quads, as slices of four vertices: each layer in draw order.
fn quads(mesh: &ChunkMesh) -> Vec<&[Vertex]> {
    mesh.layers.iter().flat_map(layer_quads).collect()
}

/// A mesh's vertices across its layers, in draw order.
fn vertices(mesh: &ChunkMesh) -> Vec<&Vertex> {
    mesh.layers
        .iter()
        .flat_map(|layer| layer.vertices.iter())
        .collect()
}

/// The number of indices across a mesh's layers.
fn index_count(mesh: &ChunkMesh) -> usize {
    mesh.layers.iter().map(|layer| layer.indices.len()).sum()
}

// -- the tests --------------------------------------------------------------

#[test]
fn the_probe_tree_resolves_every_id_the_tests_mesh() {
    let (models, _) = loaded();
    assert_eq!(models.len(), covered_ids().len() * 16);
    // The tall grass is not here: its meta 0 is the dead-bush variant, which
    // this tree does not carry; meta 1 is the plant, checked below.
    for id in [STONE, GRASS, PLANKS, LEAVES, GLASS, SPAWNER, GLOWSTONE] {
        assert!(
            matches!(models.model(id, 0, 0, 64, 0), ModelChoice::Model(_)),
            "id {id} must resolve, or the mesh tests would read the fallback"
        );
    }
    // The plant is the tall-grass variant, meta 1.
    assert!(matches!(
        models.model(TALLGRASS, 1, 0, 64, 0),
        ModelChoice::Model(_)
    ));
    // Water has no state file at all — the client builds it in — and 200 is
    // outside the covered table.
    assert!(matches!(
        models.model(WATER, 0, 0, 64, 0),
        ModelChoice::Missing
    ));
    assert!(matches!(
        models.model(200, 0, 0, 64, 0),
        ModelChoice::Missing
    ));
    // The barrier is the client's built-in block the tests mesh: no file, so
    // every state is the missing choice — which the row's `Invisible` kind,
    // not the fallback cube, answers.
    assert!(matches!(
        models.model(BARRIER, 0, 0, 64, 0),
        ModelChoice::Missing
    ));
}

#[test]
fn a_lone_stone_block_draws_six_faces() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    let world = daylight(&[(3, 64, 7, state(STONE, 0))]);
    let mesh = mesh_of(&world, &ctx);

    assert_eq!(mesh.vertex_count(), 24, "six faces of four vertices");
    assert_eq!(index_count(&mesh), 36, "six faces of two triangles");
    assert!(!mesh.is_empty());
    // The stone's row is the solid layer, so every quad is in the first slot,
    // and its indices address that layer from zero.
    assert_eq!(&mesh.layer(Layer::Opaque).indices[..6], &[0, 1, 2, 0, 2, 3]);

    // The quads come in the client's face order — down, up, north, south, west,
    // east — each in its own plane through the block: the axis and value.
    let planes = [(1, 64.0), (1, 65.0), (2, 7.0), (2, 8.0), (0, 3.0), (0, 4.0)];
    for (quad, (axis, value)) in quads(&mesh).iter().zip(planes) {
        assert!(
            quad.iter().all(|vertex| vertex.position[axis] == value),
            "quad at {value}"
        );
    }

    // The colours: the face's brightness over white. Top 255, bottom 127,
    // north and south 204, west and east 153.
    for (quad, shade) in quads(&mesh).iter().zip([127u8, 255, 204, 204, 153, 153]) {
        assert!(
            quad.iter()
                .all(|vertex| vertex.colour == [shade, shade, shade, 255]),
            "a face's colour is its brightness over white"
        );
    }

    // Every vertex takes the cell its face looks into: full daylight, no block
    // light, so the pair is the sky field 15 * 16 + 8 and the block field
    // 0 * 16 + 8.
    for vertex in vertices(&mesh) {
        assert_eq!(vertex.light, [8, 248]);
    }

    // The uv: the probe sprite's content rect, corner for corner. The sprite
    // sits at (17, 1) and is 8 x 8 inside a 128 x 128 atlas.
    assert_eq!(
        sprite_uv("blocks/probe"),
        [[0.132_812_5, 0.007_812_5], [0.195_312_5, 0.070_312_5]]
    );
    let corners = sprite_corners("blocks/probe");
    for (index, vertex) in vertices(&mesh).into_iter().enumerate() {
        assert_eq!(vertex.uv, corners[index % 4], "vertex {index}");
    }

    // The winding: each quad's normal points out of the block.
    let normals = [
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    for (quad, normal) in quads(&mesh).iter().zip(normals) {
        let a = quad[0].position;
        let b = quad[1].position;
        let c = quad[2].position;
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        for axis in 0..3 {
            assert!(
                (cross[axis] / length - normal[axis]).abs() < 1e-5,
                "quad {normal:?} winds the other way"
            );
        }
    }
}

#[test]
fn a_model_quad_samples_the_animated_sprite_frame() {
    let (models, _) = loaded();
    let atlas = strip_probe_atlas();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    let world = daylight(&[(3, 64, 7, state(STONE, 0))]);
    let mesh = mesh_of(&world, &ctx);

    // The stone's texture is an animated strip: its quads map into the
    // strip's first frame — `TextureAtlasSprite.loadSprite` sets the sprite's
    // height to its width for an animation (`:289-294`), so the rect is one
    // frame's — and never the strip's taller content. The frame sits at
    // (64, 0) and is 16 x 16 inside the 128 x 128 atlas; the strip is 16 x 32
    // from the same corner.
    let frame = [[0.5, 0.0], [0.625, 0.125]];
    let strip = [[0.5, 0.0], [0.625, 0.25]];
    let corners = [
        [frame[0][0], frame[0][1]],
        [frame[0][0], frame[1][1]],
        [frame[1][0], frame[1][1]],
        [frame[1][0], frame[0][1]],
    ];
    for (index, vertex) in vertices(&mesh).into_iter().enumerate() {
        assert_eq!(vertex.uv, corners[index % 4], "vertex {index}");
        assert!(
            vertex.uv[1] <= strip[1][1],
            "vertex {index} maps past the frame into the strip"
        );
    }
}

#[test]
fn a_meshed_column_stands_at_its_own_chunk_in_the_world() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    // A column away from the origin on both axes, so a zero offset cannot hide
    // a slip — and negative on z, so a dropped sign cannot pass either.
    let world = world_of(&[(3, -2, flat(&[(3, 64, 7, state(STONE, 0))]))]);
    let snapshot = ColumnSnapshot::from_world(&world, 3, -2);
    let mesh = build_column_meshes(&snapshot, &ctx)
        .into_iter()
        .find_map(|(_, mesh)| mesh)
        .expect("a loaded section");

    // The vertices carry the world's own coordinates: the cell at the column's
    // own (3, 64, 7) is the world's (51, 64, -25) — 3 × 16 + 3 and -2 × 16 + 7 —
    // which is the frame the renderer projects and culls in, with no per-section
    // offset of its own.
    let planes = [
        (1, 64.0),
        (1, 65.0),
        (2, -25.0),
        (2, -24.0),
        (0, 51.0),
        (0, 52.0),
    ];
    for (quad, (axis, value)) in quads(&mesh).iter().zip(planes) {
        assert!(
            quad.iter().all(|vertex| vertex.position[axis] == value),
            "quad at {value}"
        );
    }
}

#[test]
fn adjacent_stone_culls_the_face_between_them() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    let world = daylight(&[(0, 64, 0, state(STONE, 0)), (1, 64, 0, state(STONE, 0))]);
    let mesh = mesh_of(&world, &ctx);

    // Two cubes, the pair between them culled: ten faces, not twelve.
    assert_eq!(mesh.vertex_count(), 40);
    assert_eq!(index_count(&mesh), 60);
    // Only the two outer faces sit in an x plane of their own.
    let x_planes = quads(&mesh)
        .iter()
        .filter(|quad| {
            quad.iter()
                .all(|vertex| vertex.position[0] == quad[0].position[0])
        })
        .count();
    assert_eq!(x_planes, 2);
}

#[test]
fn culling_follows_the_neighbours_opacity() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // Glass is no full cube, and a mob spawner is one whose own blocks do not
    // occlude: neither hides the stone's face, while the stone hides theirs —
    // eleven faces, and the stone's own east face is the one in the x = 1
    // plane.
    for neighbour in [GLASS, SPAWNER] {
        let world = daylight(&[(0, 64, 0, state(STONE, 0)), (1, 64, 0, state(neighbour, 0))]);
        let mesh = mesh_of(&world, &ctx);
        assert_eq!(mesh.vertex_count(), 44, "eleven faces for {neighbour}");
        assert_eq!(
            quads(&mesh)
                .iter()
                .filter(|quad| quad.iter().all(|vertex| vertex.position[0] == 1.0))
                .count(),
            1,
            "the stone's east face stays"
        );
    }

    // Leaves do hide it under the graphics setting M2 renders: ten faces, and
    // nothing at all in the x = 1 plane.
    let world = daylight(&[(0, 64, 0, state(STONE, 0)), (1, 64, 0, state(LEAVES, 0))]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(mesh.vertex_count(), 40);
    assert!(
        quads(&mesh)
            .iter()
            .all(|quad| !quad.iter().all(|vertex| vertex.position[0] == 1.0))
    );
}

#[test]
fn culling_reads_across_the_column_and_section_edges() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // A neighbour in the next column: the block at x = 15 loses its east face,
    // the only one that would lie in the column's x = 16 plane.
    let world = world_of(&[
        (0, 0, flat(&[(15, 64, 3, state(STONE, 0))])),
        (1, 0, flat(&[(0, 64, 3, state(STONE, 0))])),
    ]);
    let mesh = meshes(&world, &ctx)
        .into_iter()
        .find_map(|(_, mesh)| mesh)
        .expect("a loaded section");
    assert_eq!(mesh.vertex_count(), 20);
    assert!(
        quads(&mesh)
            .iter()
            .all(|quad| !quad.iter().all(|vertex| vertex.position[0] == 16.0))
    );

    // A neighbour in the next section: the faces between y = 15 and y = 16 go.
    let world = daylight(&[(2, 15, 2, state(STONE, 0)), (2, 16, 2, state(STONE, 0))]);
    let sections = meshes(&world, &ctx);
    assert_eq!(
        sections[0].1.as_ref().expect("section 0").vertex_count(),
        20
    );
    assert_eq!(
        sections[1].1.as_ref().expect("section 1").vertex_count(),
        20
    );
}

#[test]
fn a_block_samples_the_cell_its_face_looks_into() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // Seven sky and two block light everywhere, but full daylight in the
    // stone's own cell: the standard path never reads it. The stone sits at
    // the column's near corner, so its west and north faces look into the two
    // columns beside it — the collar copies their own edge cells, and the test
    // loads them lit so every looked-into cell carries the same pair.
    let lit = |_: usize, _: usize| 1;
    let world = world_of(&[
        (
            0,
            0,
            column(&[(0, 64, 0, state(STONE, 0))], lit, |x, y, z| {
                if (x, y, z) == (0, 64, 0) {
                    (15, 15)
                } else {
                    (7, 2)
                }
            }),
        ),
        (-1, 0, column(&[], lit, |_, _, _| (7, 2))),
        (0, -1, column(&[], lit, |_, _, _| (7, 2))),
    ]);
    // The ladder: the store kept the packet's light, the snapshot copied it —
    // the column's own cell and both collar strips — and the mesh read the
    // neighbour cell's pair for every one of the six faces.
    assert_eq!(world.sky_light(0, 65, 0), 7);
    assert_eq!(world.block_light(0, 65, 0), 2);
    let snapshot = ColumnSnapshot::from_world(&world, 0, 0);
    assert_eq!(snapshot.light(0, 65, 0), (7, 2));
    assert_eq!(snapshot.light(1, 64, 0), (7, 2));
    assert_eq!(snapshot.light(-1, 64, 0), (7, 2), "the west collar");
    assert_eq!(snapshot.light(0, 64, -1), (7, 2), "the north collar");
    for vertex in vertices(&mesh_of(&world, &ctx)) {
        assert_eq!(
            vertex.light,
            [40, 120],
            "2 * 16 + 8 in the block field and 7 * 16 + 8 in the sky one"
        );
    }
}

#[test]
fn a_cross_model_reads_its_own_cell_and_takes_no_shade() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum);

    // The plant: four quads with no cullface, so the sample is the cell the
    // plant stands in — five sky and one block light there — and no shading.
    let data = column(
        &[(4, 64, 6, state(TALLGRASS, 1))],
        |_, _| 1,
        |x, y, z| {
            if (x, y, z) == (4, 64, 6) {
                (5, 1)
            } else {
                (15, 0)
            }
        },
    );
    let world = world_of(&[(0, 0, data)]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(mesh.vertex_count(), 16);
    for vertex in vertices(&mesh) {
        assert_eq!(
            vertex.light,
            [24, 88],
            "1 * 16 + 8 in the block field and 5 * 16 + 8 in the sky one"
        );
        assert_eq!(vertex.colour, [255, 255, 255, 255]);
    }
}

#[test]
fn a_glowstone_stays_on_the_standard_path() {
    let (models, atlas) = loaded();
    let maps = white_maps();

    // A stone on a glowstone, inland so every looked-into cell is in the
    // column, with the sky around the glowstone varied cell by cell. The
    // glowstone's model asks for ambient occlusion, but the path's gate
    // (`BlockModelRenderer.java:32`) also wants the block's own
    // `getLightValue() == 0`, and the glowstone's is 15: its own quads keep
    // the standard path's single sample, the cell the face looks into, where
    // the averaging path would answer a value per vertex. The stone's bottom
    // face looks into an occluding neighbour, so it is culled — no face of
    // another block can show this emitter's cell.
    let sky = |x: usize, y: usize, z: usize| match (x, y, z) {
        (5, 62, 5) => 7,
        (5, 63, 4) => 8,
        (5, 63, 6) => 9,
        (4, 63, 5) => 10,
        (6, 63, 5) => 11,
        _ => 15,
    };
    let data = column(
        &[(5, 64, 5, state(STONE, 0)), (5, 63, 5, state(GLOWSTONE, 0))],
        |_, _| 1,
        |x, y, z| (sky(x, y, z), 0),
    );
    let world = world_of(&[(0, 0, data)]);
    let snapshot = ColumnSnapshot::from_world(&world, 0, 0);
    // The scaffold: the five cells the glowstone's drawn faces look into.
    assert_eq!(snapshot.light(5, 62, 5), (7, 0), "below");
    assert_eq!(snapshot.light(5, 63, 4), (8, 0), "north");
    assert_eq!(snapshot.light(5, 63, 6), (9, 0), "south");
    assert_eq!(snapshot.light(4, 63, 5), (10, 0), "west");
    assert_eq!(snapshot.light(6, 63, 5), (11, 0), "east");

    // The glowstone's own quads answer the same under both settings: the
    // setting never reaches an emitter's geometry, and only `AmbientOcclusion`
    // off-versus-on does.
    let columns =
        |smooth_lighting| meshes(&world, &context(&models, &atlas, &maps, smooth_lighting));
    let minimum = columns(SmoothLighting::Minimum);
    let off = columns(SmoothLighting::Off);
    let all: Vec<Vec<Vertex>> = minimum
        .iter()
        .filter_map(|(_, mesh)| mesh.as_ref())
        .flat_map(|mesh| quads(mesh).into_iter().map(<[Vertex]>::to_vec))
        .collect();
    assert_eq!(all.len(), 10, "the stone's five faces and the glowstone's");
    // The stone's bottom face is the one quad at the glowstone's top plane.
    assert!(
        all.iter()
            .all(|quad| !quad.iter().all(|vertex| vertex.position[1] == 64.0)),
        "the glowstone occludes the stone's bottom face"
    );

    // The glowstone's own quads: every vertex on its own box,
    // (5, 63, 5) to (6, 64, 6) — the bottom and the four sides, the top culled.
    let glow_of = |sections: &[(usize, Option<ChunkMesh>)]| -> Vec<Vec<Vertex>> {
        sections
            .iter()
            .filter_map(|(_, mesh)| mesh.as_ref())
            .flat_map(|mesh| quads(mesh).into_iter().map(<[Vertex]>::to_vec))
            .filter(|quad| {
                quad.iter().all(|vertex| {
                    (5.0..=6.0).contains(&vertex.position[0])
                        && (63.0..=64.0).contains(&vertex.position[1])
                        && (5.0..=6.0).contains(&vertex.position[2])
                })
            })
            .collect()
    };
    let glow = glow_of(&minimum);
    assert_eq!(glow.len(), 5, "the glowstone's bottom and four sides");
    assert_eq!(
        glow,
        glow_of(&off),
        "the setting does not reach the emitter"
    );
    for quad in &glow {
        assert!(
            quad.iter().all(|vertex| vertex.light == quad[0].light),
            "one sample for the face, where the averaging path would vary it"
        );
    }
    let mut pairs: Vec<[u16; 2]> = glow.iter().map(|quad| quad[0].light).collect();
    pairs.sort();
    // Each drawn face's sky channel is the looked-into cell's, one per face,
    // and the block channel is the block's own emission in every one of them:
    // the pair's first component is the block field and the second the sky one.
    assert_eq!(
        pairs,
        [[248, 120], [248, 136], [248, 152], [248, 168], [248, 184]],
        "15 in the block field, and 7, 8, 9, 10 and 11 in the sky field"
    );
}

#[test]
fn minimum_and_maximum_answer_the_same_mesh() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    // A block with light around it and a neighbour that occludes, so the two
    // paths would differ if the renderer read the setting.
    let world = daylight(&[
        (0, 64, 0, state(STONE, 0)),
        (1, 64, 0, state(STONE, 0)),
        (0, 65, 0, state(TALLGRASS, 1)),
    ]);
    let minimum = meshes(
        &world,
        &context(&models, &atlas, &maps, SmoothLighting::Minimum),
    );
    let maximum = meshes(
        &world,
        &context(&models, &atlas, &maps, SmoothLighting::Maximum),
    );
    assert_eq!(minimum, maximum);
}

#[test]
fn a_dark_corner_falls_back_to_its_centre_sample() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum);

    // Only the cell above the stone is lit: three of the four samples of every
    // corner are zero, and the source's substitution turns each of them into
    // the fourth — the centre — so the average stays 240 where a plain average
    // would answer 60.
    let data = column(
        &[(0, 64, 0, state(STONE, 0))],
        |_, _| 1,
        |x, y, z| {
            if (x, y, z) == (0, 65, 0) {
                (15, 0)
            } else {
                (0, 0)
            }
        },
    );
    let world = world_of(&[(0, 0, data)]);
    let mesh = mesh_of(&world, &ctx);
    let top = quads(&mesh)
        .into_iter()
        .find(|quad| quad.iter().all(|vertex| vertex.position[1] == 65.0))
        .expect("the top face")
        .to_vec();
    for vertex in &top {
        assert_eq!(
            vertex.light,
            [8, 248],
            "the sky field is 240 >> 2 plus the attribute's 8; the block field is dark"
        );
    }
    // Every other face of the stone reads dark cells throughout.
    for quad in quads(&mesh)
        .into_iter()
        .filter(|quad| !quad.iter().all(|vertex| vertex.position[1] == 65.0))
    {
        for vertex in quad {
            assert_eq!(vertex.light, [8, 8]);
        }
    }
}

#[test]
fn the_ambient_occlusion_path_weights_the_four_cells() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum);

    // Everything is dark but the cell east of the lit cell above the stone —
    // the top face's first tangent. Only the two vertices whose corner faces
    // east read it: 240 >> 2 = 60, plus the attribute's 8.
    let data = column(
        &[(0, 64, 0, state(STONE, 0))],
        |_, _| 1,
        |x, y, z| {
            if (x, y, z) == (1, 65, 0) {
                (15, 0)
            } else {
                (0, 0)
            }
        },
    );
    let world = world_of(&[(0, 0, data)]);
    let mesh = mesh_of(&world, &ctx);
    let top = quads(&mesh)
        .into_iter()
        .find(|quad| quad.iter().all(|vertex| vertex.position[1] == 65.0))
        .expect("the top face")
        .to_vec();
    let lights: Vec<[u16; 2]> = top.iter().map(|vertex| vertex.light).collect();
    assert_eq!(lights, [[8, 8], [8, 8], [8, 68], [8, 68]]);
}

#[test]
fn a_concave_corner_multiplies_its_vertex_by_the_cells_light_value() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum);

    // The stone's top face, whatever is around it: the four vertices' colours.
    let top_colours = |world: &World| -> Vec<[u8; 4]> {
        quads(&mesh_of(world, &ctx))
            .into_iter()
            .find(|quad| {
                quad.iter().all(|vertex| {
                    vertex.position[1] == 65.0
                        && (0.0..=1.0).contains(&vertex.position[0])
                        && (0.0..=1.0).contains(&vertex.position[2])
                })
            })
            .expect("the stone's top face")
            .iter()
            .map(|vertex| vertex.colour)
            .collect()
    };

    // Nothing around it: all four cells of every vertex are air —
    // `getAmbientOcclusionLightValue()` 1.0 for each — so every multiplier is
    // 1.0 and the colour is the top face's own shade byte, 255. This world
    // cannot tell a missing multiplier from a present one.
    let lone = daylight(&[(0, 64, 0, state(STONE, 0))]);
    assert_eq!(top_colours(&lone), [[255, 255, 255, 255]; 4]);

    // One solid block at the east-and-south corner of the top face's east
    // slot, (1, 65, 1). A normal cube answers 0.2 there — the material blocks
    // movement and the block is a full cube — so that vertex's multiplier is
    // (0.2 + 1.0 + 1.0 + 1.0) / 4 = 0.8: 255 * 0.8 = 204, and the other three
    // vertices keep the full byte.
    let one = daylight(&[(0, 64, 0, state(STONE, 0)), (1, 65, 1, state(STONE, 0))]);
    assert_eq!(
        top_colours(&one),
        [
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [204, 204, 204, 255],
            [255, 255, 255, 255],
        ]
    );

    // The east tangent cell solid too — (1, 65, 0) — puts two 0.2 values in
    // that same slot: (0.2 + 0.2 + 1.0 + 1.0) / 4 = 0.6, so 153. That cell is
    // also one of the *next* slot's four, which drops to
    // (0.2 + 1.0 + 1.0 + 1.0) / 4 = 0.8 = 204; the remaining two vertices hold
    // the face's own byte.
    let two = daylight(&[
        (0, 64, 0, state(STONE, 0)),
        (1, 65, 0, state(STONE, 0)),
        (1, 65, 1, state(STONE, 0)),
    ]);
    assert_eq!(
        top_colours(&two),
        [
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [153, 153, 153, 255],
            [204, 204, 204, 255],
        ]
    );

    // The barrier at that same corner answers 1.0 where the stone answers 0.2:
    // `BlockBarrier.getAmbientOcclusionLightValue()` is 1.0F
    // (`BlockBarrier.java:35-41`), and the light-value rule reads a normal cube
    // as the material blocking movement and the block being a full cube. The
    // table encodes the override as the row's full_cube false, so every
    // multiplier stays 1.0 — and the barrier itself draws nothing, so the
    // found quad is still the stone's top face.
    let barrier = daylight(&[(0, 64, 0, state(STONE, 0)), (1, 65, 1, state(BARRIER, 0))]);
    assert_eq!(top_colours(&barrier), [[255, 255, 255, 255]; 4]);
}

#[test]
fn the_corner_substitution_reads_a_barrier_behind_cell_as_translucent() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum);

    // The stone's top face at (1, 64, 1) with a barrier in every tangent cell
    // and stone in every diagonal cell: each corner slot answers its tangent's
    // 1.0 while neither behind-cell is translucent and its diagonal's 0.2 when
    // either is, so the slot's branch shows as 255 against 204. The four
    // behind-cells are the fixture's variable. Air reads translucent — 204
    // through the diagonal; stone reads opaque — 255 through the tangent; an
    // id outside the table blocks nothing — 204; and the barrier's
    // `Block.translucent = true` (`BlockBarrier.java:16`, the tree's only
    // override of the constructor's `!Material.blocksLight()` field,
    // `Block.java:297`) must answer as air does: 204, where a read through the
    // material alone answers 255. The barrier tangents also hold their own
    // light value to 1.0 here — the stone case would otherwise mix in 0.2s.
    let top_colours = |behind: Option<u16>| -> Vec<[u8; 4]> {
        let mut blocks = vec![
            (1, 64, 1, state(STONE, 0)),
            (2, 65, 1, state(BARRIER, 0)),
            (0, 65, 1, state(BARRIER, 0)),
            (1, 65, 2, state(BARRIER, 0)),
            (1, 65, 0, state(BARRIER, 0)),
            (2, 65, 2, state(STONE, 0)),
            (2, 65, 0, state(STONE, 0)),
            (0, 65, 2, state(STONE, 0)),
            (0, 65, 0, state(STONE, 0)),
        ];
        if let Some(id) = behind {
            blocks.extend([
                (2, 66, 1, state(id, 0)),
                (0, 66, 1, state(id, 0)),
                (1, 66, 2, state(id, 0)),
                (1, 66, 0, state(id, 0)),
            ]);
        }
        quads(&mesh_of(&daylight(&blocks), &ctx))
            .into_iter()
            .find(|quad| {
                quad.iter().all(|vertex| {
                    vertex.position[1] == 65.0
                        && (1.0..=2.0).contains(&vertex.position[0])
                        && (1.0..=2.0).contains(&vertex.position[2])
                })
            })
            .expect("the stone's top face")
            .iter()
            .map(|vertex| vertex.colour)
            .collect()
    };

    assert_eq!(top_colours(None), [[204, 204, 204, 255]; 4]);
    assert_eq!(top_colours(Some(STONE)), [[255, 255, 255, 255]; 4]);
    assert_eq!(top_colours(Some(200)), [[204, 204, 204, 255]; 4]);
    assert_eq!(top_colours(Some(BARRIER)), [[204, 204, 204, 255]; 4]);
}

#[test]
fn a_partial_elements_face_takes_the_quad_bounds_paths() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum);

    // The stair state must resolve, or the mesh would be the fallback cube.
    assert!(
        matches!(models.model(STAIRS, 0, 0, 64, 2), ModelChoice::Model(_)),
        "the east-facing bottom stair state"
    );

    // The step at (0, 64, 2): its west face is the block's plane at x = 0 and
    // spans y 0..8 of the 16-unit box, so `fillQuadBounds`'s flag 0 holds — the
    // block is not a full cube but the quad *is* the face's own plane — and its
    // flag 1 holds too (y does not reach 1), which puts a west face through the
    // occlusion-weighted branch. The cells the west face reads sit in the
    // western border strip, one column over; each carries its own sky level,
    // and the block at (-1, 65, 1) is solid, so that corner cell's
    // ambient-occlusion light value is 0.2 where the rest are 1.0.
    let data = column(&[(0, 64, 2, state(STAIRS, 0))], |_, _| 1, |_, _, _| (0, 0));
    let west = column(
        &[(15, 65, 1, state(STONE, 0))],
        |_, _| 1,
        |x, y, z| {
            if x != 15 {
                return (0, 0);
            }
            match (y, z) {
                (63, 1) => (7, 0),
                (63, 2) => (2, 0),
                (63, 3) => (8, 0),
                (64, 1) => (3, 0),
                (64, 2) => (6, 0),
                (64, 3) => (4, 0),
                (65, 1) => (5, 0),
                (65, 2) => (1, 0),
                (65, 3) => (6, 0),
                _ => (0, 0),
            }
        },
    );
    let mesh = mesh_of(&world_of(&[(0, 0, data), (-1, 0, west)]), &ctx);
    let face = quads(&mesh)
        .into_iter()
        .find(|quad| quad.iter().all(|vertex| vertex.position[0] == 0.0))
        .expect("the step's west face")
        .to_vec();

    // The four slots' plain light pairs, from `getAoBrightness` over the two
    // tangents, the corner between them and the centre (the neighbour's cell,
    // 6) — 68, 60, 72 and 80 in the sky field, which is the pair's second
    // component — then mixed by the WEST orientation table's quad-bounds
    // products (the WEST arrays read the y and z bounds — max y = 0.5 on the
    // first two rows, 1 - min y = 1 on the next two — and never an x bound):
    // 74, 66, 72 and 80. The light attribute is that field with the sampler's
    // eight added, and `VertexTranslations` puts slot 0 on the third vertex.
    let lights: Vec<[u16; 2]> = face.iter().map(|vertex| vertex.light).collect();
    assert_eq!(lights, [[8, 74], [8, 80], [8, 88], [8, 82]]);

    // The colours: the same products mix the four plain multipliers, which are
    // 1.0 with one corner cell at 0.2 — (0.2 + 1 + 1 + 1) / 4 = 0.8 for the
    // slot the solid cell sits in. The west face's shade byte is 153, so the
    // weighted mix is 153 * (0.8 * 0.5 + 1.0 * 0.5) = 153 * 0.9 = 137 for the
    // first vertex and 153 for the rest.
    let colours: Vec<[u8; 4]> = face.iter().map(|vertex| vertex.colour).collect();
    assert_eq!(
        colours,
        [
            [137, 137, 137, 255],
            [153, 153, 153, 255],
            [153, 153, 153, 255],
            [153, 153, 153, 255],
        ]
    );
}

#[test]
fn the_biome_seam_averages_its_nine_samples() {
    let (models, atlas) = loaded();
    let maps = gradient_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // The two pixels the expectations below read, straight out of the map the
    // mesher is handed: the map holds [column, row, 0, 255], the plains
    // (temperature 0.8, rainfall 0.4) land on column 50, row 173, and the
    // desert (temperature 2.0, rainfall 0.0) on column 0, row 255.
    assert_eq!(
        maps.grass.pixel(50, 173),
        Some([50, 173, 0, 255]),
        "the plains pixel"
    );
    assert_eq!(
        maps.grass.pixel(0, 255),
        Some([0, 255, 0, 255]),
        "the desert pixel"
    );

    // The grass sits at the east edge of a plains column whose eastern
    // neighbour is desert, so six of the nine samples are plains and three
    // desert: (6 * 50 + 3 * 0) / 9 = 33 and (6 * 173 + 3 * 255) / 9 = 200.
    let world = world_of(&[
        (
            0,
            0,
            column(&[(15, 64, 5, state(GRASS, 0))], |_, _| 1, |_, _, _| (15, 0)),
        ),
        (1, 0, column(&[], |_, _| 2, |_, _, _| (15, 0))),
    ]);
    let mesh = meshes(&world, &ctx)
        .into_iter()
        .find_map(|(_, mesh)| mesh)
        .expect("a loaded section");

    let mesh_quads = quads(&mesh);
    assert_eq!(
        mesh_quads.len(),
        10,
        "six base quads and a four-sided overlay"
    );
    // The tinted top face: the averaged tint times the top's shade byte, 255.
    assert_eq!(mesh_quads[1][0].colour, [33, 200, 0, 255], "the tinted top");
    // The base's own sides and bottom carry no tint index: white, shaded by
    // each face's own byte — down 127, north and south 204, west and east 153.
    for (index, shade) in [(0usize, 127u8), (2, 204), (3, 204), (4, 153), (5, 153)] {
        assert_eq!(
            mesh_quads[index][0].colour,
            [shade, shade, shade, 255],
            "untinted quad {index}"
        );
    }
    // The overlay's four side faces are tinted by the same average, each
    // shaded by its own face's byte: 204 * 33 / 255 = 26 and 153 * 33 / 255 =
    // 19 on the first channel, 204 * 200 / 255 = 160 and 153 * 200 / 255 = 120
    // on the second.
    for quad in &mesh_quads[6..] {
        let north_or_south = quad
            .iter()
            .all(|vertex| vertex.position[2] == quad[0].position[2]);
        let expected = if north_or_south {
            [26, 160, 0, 255]
        } else {
            [19, 120, 0, 255]
        };
        assert_eq!(quad[0].colour, expected, "the tinted overlay");
    }

    // One biome's own colour, with no seam to average over: every one of the
    // nine samples is plains, so the tint is the plains pixel itself.
    let plains = world_of(&[(
        0,
        0,
        column(&[(5, 64, 5, state(GRASS, 0))], |_, _| 1, |_, _, _| (15, 0)),
    )]);
    let mesh = mesh_of(&plains, &ctx);
    assert_eq!(
        quads(&mesh)[1][0].colour,
        [50, 173, 0, 255],
        "one biome tints with its own pixel"
    );

    // A block whose model carries no tint index stays white whatever the biome
    // is: a dirt cube in the desert tints nothing, so its top face is the up
    // shade byte over white and its north face the north byte.
    let dirt = world_of(&[(
        0,
        0,
        column(&[(5, 64, 5, state(DIRT, 0))], |_, _| 2, |_, _, _| (15, 0)),
    )]);
    let mesh = mesh_of(&dirt, &ctx);
    assert_eq!(
        quads(&mesh)[1][0].colour,
        [255, 255, 255, 255],
        "the dirt's top is untinted"
    );
    assert_eq!(
        quads(&mesh)[2][0].colour,
        [204, 204, 204, 255],
        "the dirt's north face is untinted"
    );
}

#[test]
fn an_unresolved_state_draws_the_fallback_cube() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    // The water beside it has a row and no model: its geometry is the fluid
    // renderer's, in the row's translucent layer, not the fallback cube; the
    // id outside the covered table has no row to route by and draws the
    // fallback in the opaque layer, missingno and all.
    let world = daylight(&[(0, 64, 0, state(WATER, 0)), (4, 64, 0, state(200, 0))]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(mesh.vertex_count(), 11 * 4 + 24);
    let corners = sprite_corners("missingno");
    let fallback = layer_quads(mesh.layer(Layer::Opaque));
    assert_eq!(fallback.len(), 6, "the uncovered id's six faces");
    for quad in fallback {
        for (index, vertex) in quad.iter().enumerate() {
            assert_eq!(vertex.uv, corners[index]);
        }
    }
}

#[test]
fn a_covered_snow_layer_draws_its_thin_slab() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // One layer (meta 0, `layers=1`) at (0, 64, 2): the slab's own six faces,
    // the down and up faces on the sprite's full rect and the sides reading
    // its bottom band — 14/16 through the top, the snow_height2 model's own
    // uv. The fallback cube would draw six full-height faces over the
    // missingno sprite instead.
    let world = daylight(&[(0, 64, 2, state(SNOW_LAYER, 0))]);
    let mesh = mesh_of(&world, &ctx);
    let slab = quads(&mesh);
    assert_eq!(slab.len(), 6, "the slab's six faces");
    let [min, max] = sprite_uv("blocks/probe");
    let band_top = min[1] + 0.875 * (max[1] - min[1]);
    let full = sprite_corners("blocks/probe");
    let band = [
        [min[0], band_top],
        [min[0], max[1]],
        [max[0], max[1]],
        [max[0], band_top],
    ];
    // The quads come in the client's face order — down, up, north, south,
    // west, east — with the sides' own vertex heights: the first and last
    // corners on the slab's top, the middle pair on its bottom.
    let heights: [[f32; 4]; 6] = [
        [0.0; 4],
        [0.125; 4],
        [0.125, 0.0, 0.0, 0.125],
        [0.125, 0.0, 0.0, 0.125],
        [0.125, 0.0, 0.0, 0.125],
        [0.125, 0.0, 0.0, 0.125],
    ];
    let corners = [&full, &full, &band, &band, &band, &band];
    for (index, quad) in slab.iter().enumerate() {
        for (corner, vertex) in quad.iter().enumerate() {
            assert_eq!(
                vertex.position[1] - 64.0,
                heights[index][corner],
                "quad {index} vertex {corner} sits at the slab's own height"
            );
            assert_eq!(
                vertex.uv, corners[index][corner],
                "quad {index} vertex {corner} samples the block's own sprite"
            );
        }
    }
    // Every vertex stays inside the sprite's content rect: a fallback-cube
    // sample would leave it.
    for vertex in vertices(&mesh) {
        assert!(
            vertex.uv[0] >= min[0]
                && vertex.uv[0] <= max[0]
                && vertex.uv[1] >= min[1]
                && vertex.uv[1] <= max[1],
            "vertex {:?} samples outside the block's sprite",
            vertex.uv
        );
    }

    // Eight layers (meta 7, `layers=8`) name the full model: the cube the
    // fallback would draw, but over the block's own sprite.
    let world = daylight(&[(0, 64, 2, state(SNOW_LAYER, 7))]);
    let mesh = mesh_of(&world, &ctx);
    let filled = quads(&mesh);
    assert_eq!(filled.len(), 6, "the full model's six faces");
    assert!(
        filled
            .iter()
            .flat_map(|quad| quad.iter())
            .all(|vertex| vertex.position[1] == 64.0 || vertex.position[1] == 65.0),
        "the full model fills its cell"
    );
    for quad in &filled {
        for (index, vertex) in quad.iter().enumerate() {
            assert_eq!(
                vertex.uv, full[index],
                "the full model's own sprite, corner for corner"
            );
        }
    }
}

#[test]
fn a_covered_barrier_draws_no_geometry_at_all() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // The barrier has no model — the client builds it in, so no state file
    // resolves — and its row routes it around the fallback cube too: the
    // mesher emits nothing at all for its cell. A stone beside it keeps every
    // face: the barrier hides nothing (`isOpaqueCube()` false).
    let world = daylight(&[(0, 64, 0, state(STONE, 0)), (4, 64, 0, state(BARRIER, 0))]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(mesh.vertex_count(), 24, "the stone's six faces alone");
    for vertex in vertices(&mesh) {
        assert!(
            vertex.position[0] < 4.0,
            "no vertex in the barrier's cell: {:?}",
            vertex.position
        );
    }

    // A column of barrier cells alone meshes to no section at all.
    let world = daylight(&[(0, 64, 0, state(BARRIER, 0)), (1, 64, 1, state(BARRIER, 0))]);
    for (section, mesh) in meshes(&world, &ctx) {
        assert!(mesh.is_none(), "section {section} stays empty");
    }
}

#[test]
fn sign_cells_mesh_no_fallback_cube_until_the_board_task_lands() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // The standing (63) and wall (68) signs have no behaviour row and no
    // 1.8.9 blockstate (`Block.java`:1321,1326), so the model lookup misses
    // — but the fallback cube would bury the board-fixed text the sign pass
    // draws inside the cell (LessEqual against the cube's own written
    // depth). The barrier's `Invisible` precedent answers this: the mesher
    // emits nothing for either cell until a board-meshing task lands, and
    // the text floats (recorded). A stone beside them keeps every face:
    // the signs hide nothing.
    let world = daylight(&[
        (0, 64, 0, state(STONE, 0)),
        (4, 64, 0, state(SIGN_STANDING, 0)),
        (8, 64, 0, state(SIGN_WALL, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(mesh.vertex_count(), 24, "the stone's six faces alone");
    for vertex in vertices(&mesh) {
        assert!(
            vertex.position[0] < 4.0,
            "no vertex in either sign's cell: {:?}",
            vertex.position
        );
    }

    // Sign cells alone mesh to no section at all — above all, no opaque
    // fallback quads.
    let world = daylight(&[
        (0, 64, 0, state(SIGN_STANDING, 0)),
        (1, 64, 1, state(SIGN_WALL, 0)),
    ]);
    for (section, mesh) in meshes(&world, &ctx) {
        assert!(mesh.is_none(), "section {section} stays empty");
    }
}

#[test]
fn a_section_with_no_quads_is_left_out() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // An empty column and an unloaded one both report sixteen empty sections.
    for (section, mesh) in meshes(&world_of(&[(0, 0, flat(&[]))]), &ctx) {
        assert!(mesh.is_none(), "section {section} is empty");
    }
    for (section, mesh) in meshes(&World::new(true), &ctx) {
        assert!(mesh.is_none(), "section {section} is unloaded");
    }
}

#[test]
fn reads_above_and_below_the_column_answer_air_and_darkness() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // A block at the very top and one at the very bottom: both keep the face
    // that looks out of the world, and neither read panics.
    let world = daylight(&[(0, 255, 0, state(STONE, 0)), (2, 0, 0, state(STONE, 0))]);
    let snapshot = ColumnSnapshot::from_world(&world, 0, 0);
    assert_eq!(snapshot.block(0, 256, 0), 0);
    assert_eq!(snapshot.light(0, 256, 0), (0, 0));
    let sections = build_column_meshes(&snapshot, &ctx);
    assert_eq!(
        sections[15].1.as_ref().expect("section 15").vertex_count(),
        24
    );
    assert_eq!(
        sections[0].1.as_ref().expect("section 0").vertex_count(),
        24
    );
    // The top block's own top face looks into the cell above: air, so it is
    // drawn, and the light it takes is that cell's — dark.
    let top = sections[15].1.as_ref().expect("section 15");
    let up = quads(top)
        .into_iter()
        .find(|quad| quad.iter().all(|vertex| vertex.position[1] == 256.0))
        .expect("the top face");
    for vertex in up {
        assert_eq!(vertex.light, [8, 8]);
    }
}

#[test]
fn weighted_alternatives_follow_the_positions_hash() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // Oak planks carry the six-quad cube at weight 2 and the twelve-quad
    // double at weight 3. The sorted order puts the heavier first, so indices
    // 0 to 2 pick the double and 3 and 4 the cube, and the position hash
    // decides: (0, 64, 0) hashes to 1 and (5, 64, 0) to 4.
    let world = daylight(&[(0, 64, 0, state(PLANKS, 0)), (5, 64, 0, state(PLANKS, 0))]);
    let mesh = mesh_of(&world, &ctx);
    let mesh_quads = quads(&mesh);
    assert_eq!(mesh_quads.len(), 18);
    assert_eq!(
        mesh_quads
            .iter()
            .filter(|quad| quad[0].position[0] < 2.0)
            .count(),
        12,
        "the double at the origin"
    );
    assert_eq!(
        mesh_quads
            .iter()
            .filter(|quad| quad[0].position[0] > 4.0)
            .count(),
        6,
        "the cube at x = 5"
    );

    // All-zero weights: no variant claims a weight, and the sorted order's
    // first entry — the cube, its six quads beating the double's twelve — is
    // taken at every position. That is the spruce meta, the file's other key.
    let world = daylight(&[(0, 64, 0, state(PLANKS, 1)), (5, 64, 0, state(PLANKS, 1))]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(quads(&mesh).len(), 12);
}

#[test]
fn weighted_alternatives_read_the_world_position_not_the_columns() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // The source hashes the position the block renders at: the chunk compiler
    // walks the chunk's absolute box (`RenderChunk.java:119-120`) and hands
    // each block's world `BlockPos` to the dispatcher (`:187`), which picks the
    // alternative with it (`BlockRendererDispatcher.java:100-102`). A column's
    // build reads its cells in the column's own frame, so the pick must widen
    // the cell to the world before hashing it. The column (2, 3) cell
    // (2, 64, 3) is the world (34, 64, 51): its hash picks index 1 — the
    // double, twelve quads — where the cell's own hash picks 3, the cube's six.
    let world = world_of(&[(2, 3, flat(&[(2, 64, 3, state(PLANKS, 0))]))]);
    let snapshot = ColumnSnapshot::from_world(&world, 2, 3);
    let mesh = build_column_meshes(&snapshot, &ctx)
        .into_iter()
        .find_map(|(_, mesh)| mesh)
        .expect("a loaded section");
    assert_eq!(
        quads(&mesh).len(),
        12,
        "the world position's pick, the double"
    );
}

#[test]
fn the_state_mapper_names_literals() {
    // Every arm the mapper answers with a file of its own, pinned to the
    // literal `(file, key)` the client's state mapper registers. A wrong name
    // here turns a covered id into the fallback cube without failing any other
    // test, so each arm carries at least one state, and the entries that share
    // an arm carry the values that exercise its own rule: the dropped
    // properties, the suffix, the seamless bit, the axis.
    let arms: [MapperArm; 30] = [
        // `Plain`: the registry name and the whole property string.
        (4, 0, Some(("cobblestone", "normal"))),
        (GRASS, 0, Some(("grass", "snowy=false"))),
        // `Name` on the variant: the file is the variant, the key drops it.
        (STONE, 0, Some(("stone", "normal"))),
        (12, 1, Some(("red_sand", "normal"))),
        (98, 3, Some(("chiseled_stonebrick", "normal"))),
        // `Name` with a suffix.
        (PLANKS, 1, Some(("spruce_planks", "normal"))),
        (LOG, 5, Some(("spruce_log", "axis=x"))),
        (162, 8, Some(("acacia_log", "axis=z"))),
        // Leaves: `check_decay` and `decayable` never reach the key.
        (LEAVES, 0, Some(("oak_leaves", "normal"))),
        (LEAVES, 8, Some(("oak_leaves", "normal"))),
        (161, 1, Some(("dark_oak_leaves", "normal"))),
        // The plants and wools named by their type and colour.
        (24, 2, Some(("smooth_sandstone", "normal"))),
        (31, 0, Some(("dead_bush", "normal"))),
        (31, 1, Some(("tall_grass", "normal"))),
        (31, 2, Some(("fern", "normal"))),
        (37, 0, Some(("dandelion", "normal"))),
        (38, 1, Some(("blue_orchid", "normal"))),
        (38, 8, Some(("oxeye_daisy", "normal"))),
        (35, 2, Some(("magenta_wool", "normal"))),
        // The double plant: `facing` dropped, `half` kept.
        (175, 0, Some(("sunflower", "half=lower"))),
        (175, 8, Some(("sunflower", "half=upper"))),
        // `Ignore`: one property dropped, the rest of the key kept whole.
        (46, 0, Some(("tnt", "normal"))),
        (
            64,
            0,
            Some((
                "wooden_door",
                "facing=east,half=lower,hinge=left,open=false",
            )),
        ),
        (
            64,
            8,
            Some((
                "wooden_door",
                "facing=north,half=upper,hinge=left,open=false",
            )),
        ),
        (81, 0, Some(("cactus", "normal"))),
        (83, 0, Some(("reeds", "normal"))),
        // `Dirt`: the variant names the file, `snowy` stays for the podzol.
        (DIRT, 2, Some(("podzol", "snowy=false"))),
        // `DoubleSlab`: the seamless bit answers `all` over the variant's file.
        (43, 8, Some(("stone_double_slab", "all"))),
        // The snow layer: the file is the block's own name and the key its
        // `layers` property, whose low three metadata bits plus one name the
        // count (BlockSnow.java:26, :153).
        (SNOW_LAYER, 0, Some(("snow_layer", "layers=1"))),
        (SNOW_LAYER, 7, Some(("snow_layer", "layers=8"))),
    ];
    assert_targets(&arms);

    // The remaining states of the arms whose answer turns on the state: the
    // dirt variants, the double slabs' variant and seamless combinations, the
    // quartz column's three axes, the dead bush block, and the six ids the
    // client builds in with no file at all.
    let singles: [MapperArm; 12] = [
        (DIRT, 0, Some(("dirt", "normal"))),
        (DIRT, 1, Some(("coarse_dirt", "normal"))),
        (43, 0, Some(("stone_double_slab", "normal"))),
        (43, 1, Some(("sandstone_double_slab", "normal"))),
        (43, 12, Some(("brick_double_slab", "all"))),
        (155, 0, Some(("quartz_block", "normal"))),
        (155, 1, Some(("chiseled_quartz_block", "normal"))),
        (155, 2, Some(("quartz_column", "axis=y"))),
        (155, 3, Some(("quartz_column", "axis=x"))),
        (155, 4, Some(("quartz_column", "axis=z"))),
        (32, 0, Some(("dead_bush", "normal"))),
        (4, 4, Some(("cobblestone", "normal"))),
    ];
    assert_targets(&singles);
    for id in [WATER, 9, 10, 11, 54, BARRIER] {
        let row = behaviour(id).unwrap_or_else(|| panic!("id {id} is covered"));
        assert_eq!(blockstate_target(row, 0), None, "id {id} has no file");
    }
    assert!(behaviour(200).is_none());
}

#[test]
fn the_baked_model_is_what_the_mesher_reads() {
    let (models, _) = loaded();
    let ModelChoice::Model(stone) = models.model(STONE, 0, 0, 0, 0) else {
        panic!("stone resolves");
    };
    assert_eq!(stone.quads.len(), 6);
    assert!(stone.ambient_occlusion);
    assert!(
        stone
            .quads
            .iter()
            .all(|quad| quad.texture == "blocks/probe")
    );
    assert!(stone.quads.iter().all(|quad| quad.cullface.is_some()));
}
