//! The mesh core over synthetic store columns and a synthetic model tree.
//!
//! Every expectation is a literal: the quad counts, the atlas corners, the
//! per-vertex light pairs and the tint bytes. The worlds are [`ColumnData`]
//! values through [`World::apply_column`], and the models come from a temporary
//! extraction tree the tests write themselves — the same layout the project's
//! extractor emits, flattened files with `normal` keys — so nothing here reads
//! the real game assets.

use std::collections::BTreeMap;

use oxide_assets::atlas::{Atlas, AtlasLevel, AtlasSprite, SpriteRect};
use oxide_assets::model::ModelSource;
use oxide_game::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, ModelChoice, SmoothLighting, blockstate_target,
    build_column_meshes,
};
use oxide_proto_v47::column::{ColumnData, SectionData};
use oxide_render::terrain::{ChunkMesh, Vertex};
use oxide_world::behaviour::{TintKind, behaviour, covered_ids};
use oxide_world::biome::{ColorMap, TintMaps, tint_at_9_biome};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::world::World;

/// The block ids the tests use.
const STONE: u16 = 1;
const GRASS: u16 = 2;
const PLANKS: u16 = 5;
const WATER: u16 = 8;
const LEAVES: u16 = 18;
const GLASS: u16 = 20;
const LOG: u16 = 17;
const TALLGRASS: u16 = 31;
const SPAWNER: u16 = 52;
const GLOWSTONE: u16 = 89;

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

/// A mesh's quads, as slices of four vertices.
fn quads(mesh: &ChunkMesh) -> Vec<&[Vertex]> {
    mesh.vertices.chunks(4).collect()
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
}

#[test]
fn a_lone_stone_block_draws_six_faces() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    let world = daylight(&[(3, 64, 7, state(STONE, 0))]);
    let mesh = mesh_of(&world, &ctx);

    assert_eq!(mesh.vertices.len(), 24, "six faces of four vertices");
    assert_eq!(mesh.indices.len(), 36, "six faces of two triangles");
    assert_eq!(mesh.vertex_count(), 24);
    assert!(!mesh.is_empty());
    assert_eq!(&mesh.indices[..6], &[0, 1, 2, 0, 2, 3]);

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
    // light, so both channels are 15 * 16 + 8.
    for vertex in &mesh.vertices {
        assert_eq!(vertex.light, [248, 8]);
    }

    // The uv: the probe sprite's content rect, corner for corner. The sprite
    // sits at (17, 1) and is 8 x 8 inside a 128 x 128 atlas.
    assert_eq!(
        sprite_uv("blocks/probe"),
        [[0.132_812_5, 0.007_812_5], [0.195_312_5, 0.070_312_5]]
    );
    let corners = sprite_corners("blocks/probe");
    for (index, vertex) in mesh.vertices.iter().enumerate() {
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
fn adjacent_stone_culls_the_face_between_them() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    let world = daylight(&[(0, 64, 0, state(STONE, 0)), (1, 64, 0, state(STONE, 0))]);
    let mesh = mesh_of(&world, &ctx);

    // Two cubes, the pair between them culled: ten faces, not twelve.
    assert_eq!(mesh.vertices.len(), 40);
    assert_eq!(mesh.indices.len(), 60);
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
        assert_eq!(mesh.vertices.len(), 44, "eleven faces for {neighbour}");
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
    assert_eq!(mesh.vertices.len(), 40);
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
    assert_eq!(mesh.vertices.len(), 20);
    assert!(
        quads(&mesh)
            .iter()
            .all(|quad| !quad.iter().all(|vertex| vertex.position[0] == 16.0))
    );

    // A neighbour in the next section: the faces between y = 15 and y = 16 go.
    let world = daylight(&[(2, 15, 2, state(STONE, 0)), (2, 16, 2, state(STONE, 0))]);
    let sections = meshes(&world, &ctx);
    assert_eq!(
        sections[0].1.as_ref().expect("section 0").vertices.len(),
        20
    );
    assert_eq!(
        sections[1].1.as_ref().expect("section 1").vertices.len(),
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
    for vertex in &mesh_of(&world, &ctx).vertices {
        assert_eq!(vertex.light, [120, 40], "7 * 16 + 8 and 2 * 16 + 8");
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
    assert_eq!(mesh.vertices.len(), 16);
    for vertex in &mesh.vertices {
        assert_eq!(vertex.light, [88, 24], "5 * 16 + 8 and 1 * 16 + 8");
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
        .flat_map(|mesh| mesh.vertices.chunks(4).map(<[Vertex]>::to_vec))
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
            .flat_map(|mesh| mesh.vertices.chunks(4).map(<[Vertex]>::to_vec))
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
    // and the block channel is the block's own emission in every one of them.
    assert_eq!(
        pairs,
        [[120, 248], [136, 248], [152, 248], [168, 248], [184, 248]],
        "7, 8, 9, 10 and 11 in the sky field, 15 in the block field"
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
        assert_eq!(vertex.light, [248, 8], "240 >> 2, plus the attribute's 8");
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
    assert_eq!(lights, [[8, 8], [8, 8], [68, 8], [68, 8]]);
}

#[test]
fn the_biome_seam_averages_its_nine_samples() {
    let (models, atlas) = loaded();
    let maps = gradient_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);

    // The grass sits at the east edge of a plains column whose eastern
    // neighbour is desert: six of the nine samples are the block's own biome
    // and three are the neighbour's.
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

    let plains = tint_at_9_biome(&maps, 15, 64, 5, TintKind::Grass, |_, _| 1);
    let desert = tint_at_9_biome(&maps, 15, 64, 5, TintKind::Grass, |_, _| 2);
    assert_ne!(plains, desert, "the two biomes must tint differently");
    let average: Vec<u8> = (0..3)
        .map(|channel| {
            ((6 * u32::from(plains[channel]) + 3 * u32::from(desert[channel])) / 9) as u8
        })
        .collect();

    let mesh_quads = quads(&mesh);
    assert_eq!(
        mesh_quads.len(),
        10,
        "six base quads and a four-sided overlay"
    );
    assert_eq!(
        mesh_quads[1][0].colour,
        [average[0], average[1], average[2], 255],
        "the tinted top"
    );
    // The base's own sides and bottom carry no tint index: white, shaded.
    for (index, shade) in [(0usize, 0.5f32), (2, 0.8), (3, 0.8), (4, 0.6), (5, 0.6)] {
        let channel = (255.0 * shade) as u8;
        assert_eq!(
            mesh_quads[index][0].colour,
            [channel, channel, channel, 255],
            "untinted quad {index}"
        );
    }
    // The overlay's four side faces are tinted by the same average, each
    // shaded by its own face's brightness: north and south 0.8, west and east
    // 0.6 — the values the base's untinted sides carry above.
    for quad in &mesh_quads[6..] {
        let shade = if quad
            .iter()
            .all(|vertex| vertex.position[2] == quad[0].position[2])
        {
            0.8
        } else {
            0.6
        };
        let expected = [
            (f32::from(average[0]) * shade) as u8,
            (f32::from(average[1]) * shade) as u8,
            (f32::from(average[2]) * shade) as u8,
            255,
        ];
        assert_eq!(quad[0].colour, expected, "the tinted overlay");
    }
}

#[test]
fn an_unresolved_state_draws_the_fallback_cube() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off);
    let world = daylight(&[(0, 64, 0, state(WATER, 0)), (4, 64, 0, state(200, 0))]);
    let mesh = mesh_of(&world, &ctx);
    assert_eq!(mesh.vertices.len(), 48);
    let corners = sprite_corners("missingno");
    let fallback = quads(&mesh);
    for quad in [fallback[0], fallback[6]] {
        for (index, vertex) in quad.iter().enumerate() {
            assert_eq!(vertex.uv, corners[index]);
        }
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
        sections[15].1.as_ref().expect("section 15").vertices.len(),
        24
    );
    assert_eq!(
        sections[0].1.as_ref().expect("section 0").vertices.len(),
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
fn the_state_mapper_names_literals() {
    // Stone: the variant property names the file, and the key drops it.
    let stone = behaviour(STONE).expect("stone is covered");
    assert_eq!(
        blockstate_target(stone, 0),
        Some(("stone".to_string(), "normal".to_string()))
    );
    let planks = behaviour(PLANKS).expect("planks are covered");
    assert_eq!(
        blockstate_target(planks, 1),
        Some(("spruce_planks".to_string(), "normal".to_string()))
    );
    // A log keeps its axis in the key: meta 5 is the spruce variant on the x
    // axis.
    let log = behaviour(LOG).expect("logs are covered");
    assert_eq!(
        blockstate_target(log, 5),
        Some(("spruce_log".to_string(), "axis=x".to_string()))
    );
    // Grass keeps its whole property string, and the client builds water in.
    let grass = behaviour(GRASS).expect("grass is covered");
    assert_eq!(
        blockstate_target(grass, 0),
        Some(("grass".to_string(), "snowy=false".to_string()))
    );
    let water = behaviour(WATER).expect("water is covered");
    assert_eq!(blockstate_target(water, 0), None);
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
