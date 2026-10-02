//! The liquids, the Fast leaves rule and the three terrain layers, over
//! synthetic columns and the synthetic model tree.
//!
//! Every expectation is a literal taken from the client's own arithmetic:
//! `client/renderer/BlockFluidRenderer.java` for the surfaces, the uv and the
//! colour, `block/BlockLiquid.java` for the culling and the flow direction,
//! `block/BlockLeaves.java` for the two graphics branches. The worlds are
//! [`ColumnData`] values through [`World::apply_column`], and the models come
//! from a temporary extraction tree the tests write themselves, as
//! `mesher.rs` does — nothing here reads the real game assets.
//!
//! # The heights
//!
//! A corner's height is `getFluidHeight`'s average over the four cells the
//! corner spans (`BlockFluidRenderer.java:255-296`). The queried cell
//! contributes `getLiquidHeightPercent`'s air percent with weight one — and
//! with weight ten when its level is 0 or at least 8 — a neighbour of another
//! non-solid material contributes a plain one, and a solid neighbour
//! contributes nothing. A source with three air neighbours is therefore
//! `1 - ((1/9 * 10) + (1/9) + 1 + 1 + 1) / 14`, which the source's float
//! shapes land on `0.69841266` — just under a third of a unit below the block's
//! top, not at its base. The renderer sinks the surface by 0.001 when the top
//! face draws (`:78-81`), and the sides carry whichever heights the top pass
//! left behind.

use std::collections::BTreeMap;

use oxide_assets::atlas::{AnimatedSprite, Atlas, AtlasLevel, AtlasSprite, SpriteRect};
use oxide_assets::model::ModelSource;
use oxide_game::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, SmoothLighting, build_column_meshes,
};
use oxide_proto_v47::column::{ColumnData, SectionData};
use oxide_render::terrain::{ChunkMesh, Layer, Vertex};
use oxide_world::behaviour::{Material, behaviour, covered_ids};
use oxide_world::biome::{ColorMap, TintMaps};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::world::World;

/// The block ids the tests use: the flowing and still liquid pairs, the leaves,
/// the plant, and the ice the second top pass's ring reads.
const WATER: u16 = 8;
const WATER_STILL: u16 = 9;
const LAVA: u16 = 10;
const LAVA_STILL: u16 = 11;
const LEAVES: u16 = 18;
const TALLGRASS: u16 = 31;
const STONE: u16 = 1;
const GLASS: u16 = 20;
const ICE: u16 = 79;

// -- the source's own float shapes, as literals ------------------------------

/// A lone source's corner: the cell's eleven ninths of air weight and three
/// plain air samples over fourteen, before the renderer's sink and after it.
const SOURCE_RAW: f32 = 0.69841266;
const SOURCE_SURFACE: f32 = 0.69741267;
/// A pool corner: two level-0 cells' elevenths and two plain air samples over
/// twenty-four, before the sink and after it.
const POOL_RAW: f32 = 0.8148148;
const POOL_SURFACE: f32 = 0.8138148;
/// A level-5 cell's own corner: six ninths over four, before the sink and after.
const LOW_RAW: f32 = 0.08333331;
const LOW_SURFACE: f32 = 0.08233331;
/// A level-5 cell's corner over a level-0 neighbour: eleven ninths, two airs
/// and six ninths over fourteen, before the sink and after.
const RIM_RAW: f32 = 0.7222222;
const RIM_SURFACE: f32 = 0.7212222;
/// The waterfall clause's full height: a cell whose top face is culled never
/// sinks, so its sides stand at exactly one.
const FALL_HEIGHT: f32 = 1.0;

/// A surface's world y: the base the mesher adds the height to, in f32.
fn surface_y(base: f32, height: f32) -> f32 {
    base + height
}

// -- the atlas ---------------------------------------------------------------

/// The content rect of one sprite in the synthetic atlas, as its min and max
/// uv corner. The atlas holds one 128 x 128 level and each sprite is an 8 x 8
/// content rect inside a 16 x 16 cell, so the fifth sprite's rect starts at
/// 65/128 and the seventh's at 97/128.
const STILL: ([f32; 2], [f32; 2]) = ([0.5078125, 0.0078125], [0.5703125, 0.0703125]);
const FLOW: ([f32; 2], [f32; 2]) = ([0.6328125, 0.0078125], [0.6953125, 0.0703125]);
const LAVA_STILL_RECT: ([f32; 2], [f32; 2]) = ([0.7578125, 0.0078125], [0.8203125, 0.0703125]);
const LAVA_FLOW_RECT: ([f32; 2], [f32; 2]) = ([0.8828125, 0.0078125], [0.9453125, 0.0703125]);

/// A sprite rect's four corners in the order the top and bottom passes walk
/// them: `getInterpolatedU(0)`, `getInterpolatedV(0)`, `getInterpolatedU(16)`
/// and `getInterpolatedV(16)`.
fn rect_corners(rect: ([f32; 2], [f32; 2])) -> [[f32; 2]; 4] {
    let (min, max) = rect;
    [
        [min[0], min[1]],
        [min[0], max[1]],
        [max[0], max[1]],
        [max[0], min[1]],
    ]
}

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

/// The atlas these tests mesh with: the fallback, the three the probe models
/// name, and the four liquid sprites, in the order the sprite indexes above
/// assume.
fn test_atlas() -> Atlas {
    atlas(&[
        "missingno",
        "blocks/probe",
        "blocks/probe_overlay",
        "blocks/probe_cross",
        "blocks/water_still",
        "blocks/water_flow",
        "blocks/lava_still",
        "blocks/lava_flow",
    ])
}

// -- the world side ----------------------------------------------------------

/// One wire block value: the id and its metadata.
fn state(id: u16, meta: u8) -> u16 {
    (id << 4) | u16::from(meta)
}

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

/// One swampland column in full daylight — biome 6, whose water multiplier is
/// `14745518`, the `(224, 255, 174)` the tint test pins.
fn swamp(blocks: &[(usize, usize, usize, u16)]) -> ColumnData {
    column(blocks, |_, _| 6, |_, _, _| (15, 0))
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

// -- the model side ----------------------------------------------------------

/// Neutral colour maps: white everywhere.
fn white_maps() -> TintMaps {
    let neutral = || ColorMap::from_rgba(&[255u8; 256 * 256 * 4]).expect("a valid map");
    TintMaps {
        grass: neutral(),
        foliage: neutral(),
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
fn cube(texture: &str) -> String {
    let mut faces = Vec::new();
    for face in ["down", "up", "north", "south", "west", "east"] {
        faces.push(format!(
            r##""{face}": {{"texture": "#all", "cullface": "{face}"}}"##
        ));
    }
    format!(
        r##"{{"textures": {{"all": "{texture}"}}, "elements": [{{"from": [0, 0, 0], "to": [16, 16, 16], "faces": {{{}}}}}]}}"##,
        faces.join(", ")
    )
}

/// The model tree these tests load: a probe cube for the solid blocks and the
/// leaves, and the plant cross. The liquids have no state file — the client
/// builds them in, and the mesher never consults the model set for them.
fn probe_tree() -> Tree {
    let tree = Tree::new();
    tree.model("probe_cube", &cube("blocks/probe"));
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
    tree.blockstates("oak_leaves", &normal("probe_cube"));
    tree.blockstates("tall_grass", &normal("probe_cross"));
    tree
}

/// The models and the atlas these tests mesh with.
fn loaded() -> (BlockModelSet, Atlas) {
    let tree = probe_tree();
    let models = BlockModelSet::load(&tree.source());
    (models, test_atlas())
}

/// The context over one loaded set, with the given smooth-lighting setting and
/// graphics mode.
fn context<'a>(
    models: &'a BlockModelSet,
    atlas: &'a Atlas,
    maps: &'a TintMaps,
    smooth_lighting: SmoothLighting,
    graphics_fast: bool,
) -> MeshContext<'a> {
    MeshContext {
        models,
        atlas,
        tint_maps: maps,
        graphics_fast,
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

// -- the mesh side -----------------------------------------------------------

/// One layer's quads, as slices of four vertices, in the layer's own order.
fn quads(mesh: &ChunkMesh, layer: Layer) -> Vec<&[Vertex]> {
    mesh.layer(layer).vertices.chunks(4).collect()
}

/// A quad's four positions.
fn positions(quad: &[Vertex]) -> [[f32; 3]; 4] {
    std::array::from_fn(|index| quad[index].position)
}

/// A quad's four uv corners.
fn uvs(quad: &[Vertex]) -> [[f32; 2]; 4] {
    std::array::from_fn(|index| quad[index].uv)
}

/// The quads whose first vertex sits at one position, in layer order.
fn quads_at<'a>(quads: &[&'a [Vertex]], position: [f32; 3]) -> Vec<&'a [Vertex]> {
    quads
        .iter()
        .copied()
        .filter(|quad| quad[0].position == position)
        .collect()
}

/// The quads that lie flat in one plane.
fn plane_quads<'a>(quads: &[&'a [Vertex]], axis: usize, value: f32) -> Vec<&'a [Vertex]> {
    quads
        .iter()
        .copied()
        .filter(|quad| quad.iter().all(|vertex| vertex.position[axis] == value))
        .collect()
}

/// The quads whose four vertices all sit inside one cell's box.
fn cell_quads<'a>(quads: &[&'a [Vertex]], cell: [f32; 3]) -> Vec<&'a [Vertex]> {
    quads
        .iter()
        .copied()
        .filter(|quad| in_cell(quad, cell))
        .collect()
}

/// Whether every vertex of a quad sits inside one cell's box.
fn in_cell(quad: &[Vertex], cell: [f32; 3]) -> bool {
    quad.iter().all(|vertex| {
        (0..3).all(|axis| {
            vertex.position[axis] >= cell[axis] && vertex.position[axis] <= cell[axis] + 1.0
        })
    })
}

/// The distance from a section centre to a quad's centre, the mean of its four
/// vertex positions — the mesher's own order key.
fn quad_distance(quad: &[Vertex], centre: [f32; 3]) -> f32 {
    let mut sum = [0.0f32; 3];
    for vertex in quad {
        for (axis, total) in sum.iter_mut().enumerate() {
            *total += vertex.position[axis];
        }
    }
    let dx = sum[0] / 4.0 - centre[0];
    let dy = sum[1] / 4.0 - centre[1];
    let dz = sum[2] / 4.0 - centre[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// -- the tests ---------------------------------------------------------------

#[test]
fn a_two_cell_pool_keeps_the_corner_averaged_surface() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    let world = daylight(&[
        (0, 64, 0, state(WATER_STILL, 0)),
        (1, 64, 0, state(WATER_STILL, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);

    // Water is translucent: nothing lands in the other two layers, and the two
    // cells' quads share one buffer.
    assert!(mesh.layer(Layer::Opaque).is_empty());
    assert!(mesh.layer(Layer::Cutout).is_empty());
    let quads = quads(&mesh, Layer::Translucent);
    assert_eq!(quads.len(), 18, "nine quads per cell");
    assert_eq!(mesh.layer(Layer::Translucent).vertices.len(), 72);
    assert_eq!(mesh.layer(Layer::Translucent).indices.len(), 108);
    assert_eq!(mesh.vertex_count(), 72, "the sum across the layers");
    assert_eq!(
        &mesh.layer(Layer::Translucent).indices[..6],
        &[0, 1, 2, 0, 2, 3]
    );
    assert!(!mesh.is_empty());
    assert_eq!(cell_quads(&quads, [0.0, 64.0, 0.0]).len(), 9);
    assert_eq!(cell_quads(&quads, [1.0, 64.0, 0.0]).len(), 9);

    // The shared face culls on both cells: neither cell's east nor west plane
    // carries a quad, while the outer planes carry the two windings each.
    assert!(
        plane_quads(&quads, 0, 0.999).is_empty(),
        "the west cell's east face"
    );
    assert!(
        plane_quads(&quads, 0, 1.001).is_empty(),
        "the east cell's west face"
    );
    assert_eq!(plane_quads(&quads, 0, 0.001).len(), 2);
    assert_eq!(plane_quads(&quads, 0, 1.999).len(), 2);
    // The four sides against air: north and south of both cells.
    assert_eq!(plane_quads(&quads, 2, 0.001).len(), 4);
    assert_eq!(plane_quads(&quads, 2, 0.999).len(), 4);

    // The west cell's surface: its outer corners are the lone source's
    // average and the shared corners the pool's — the corner average, not a
    // flat sheet — and the source's second pass draws the same four points
    // the other way round.
    let low = surface_y(64.0, SOURCE_SURFACE);
    let high = surface_y(64.0, POOL_SURFACE);
    let surface = quads_at(&quads, [0.0, low, 0.0]);
    assert_eq!(surface.len(), 2, "the top and its reverse");
    assert_eq!(
        positions(surface[0]),
        [
            [0.0, low, 0.0],
            [0.0, low, 1.0],
            [1.0, high, 1.0],
            [1.0, high, 0.0]
        ]
    );
    assert_eq!(
        positions(surface[1]),
        [
            [0.0, low, 0.0],
            [1.0, high, 0.0],
            [1.0, high, 1.0],
            [0.0, low, 1.0]
        ]
    );
    // A top with no flow direction takes the still sprite, unrotated.
    let still = rect_corners(STILL);
    assert_eq!(uvs(surface[0]), still);
    assert_eq!(uvs(surface[1]), [still[0], still[3], still[2], still[1]]);
    assert!(
        surface[0]
            .iter()
            .all(|vertex| vertex.colour == [255, 255, 255, 255]),
        "the top is the tint at full brightness"
    );

    // The bottom face: the block's base, the still sprite's min and max
    // corners, and the renderer's untinted half-grey.
    let bottom = plane_quads(&quads, 1, 64.0);
    assert_eq!(bottom.len(), 2, "one per cell");
    assert_eq!(
        positions(bottom[0]),
        [
            [0.0, 64.0, 1.0],
            [0.0, 64.0, 0.0],
            [1.0, 64.0, 0.0],
            [1.0, 64.0, 1.0]
        ]
    );
    assert_eq!(
        uvs(bottom[0]),
        [
            [0.5078125, 0.0703125],
            [0.5078125, 0.0078125],
            [0.5703125, 0.0078125],
            [0.5703125, 0.0703125]
        ]
    );
    assert!(
        bottom.iter().all(|quad| quad
            .iter()
            .all(|vertex| vertex.colour == [127, 127, 127, 255])),
        "the bottom face takes no tint"
    );

    // The north face of the west cell: the plane 0.001 inside the block, from
    // the base to the sunk surface, with the flowing sprite and the 0.8 shade.
    let north = plane_quads(&quads, 2, 0.001);
    assert_eq!(north.len(), 4, "both cells, two windings each");
    let west_north = quads_at(&quads, [0.0, low, 0.001]);
    assert_eq!(west_north.len(), 1);
    assert_eq!(
        positions(west_north[0]),
        [
            [0.0, low, 0.001],
            [1.0, high, 0.001],
            [1.0, 64.0, 0.001],
            [0.0, 64.0, 0.001]
        ]
    );
    assert_eq!(
        uvs(west_north[0]),
        [
            [0.6328125, 0.017268354],
            [0.6640625, 0.013630787],
            [0.6640625, 0.0390625],
            [0.6328125, 0.0390625]
        ],
        "the north face's two corners: the lonely west corner's eighth of a unit \
         and the pool corner's, each through getInterpolatedV"
    );
    // The side's u runs from the flowing sprite's left edge to its middle, and
    // its two v's are the two corner heights through getInterpolatedV.
    assert_eq!(
        [uvs(west_north[0])[0][0], uvs(west_north[0])[1][0]],
        [FLOW.0[0], FLOW.0[0] + (FLOW.1[0] - FLOW.0[0]) * 0.5]
    );
    assert_eq!(POOL_RAW - 0.001, POOL_SURFACE, "the pool corner's sink");
    assert!(
        west_north[0]
            .iter()
            .all(|vertex| vertex.colour == [204, 204, 204, 255]),
        "the north face's 0.8 shade over white"
    );
}

#[test]
fn a_liquid_top_culls_only_against_the_same_liquid() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    let world = daylight(&[
        (0, 64, 0, state(WATER_STILL, 0)),
        (0, 65, 0, state(WATER_STILL, 0)),
        (2, 64, 0, state(WATER_STILL, 0)),
        (2, 65, 0, state(STONE, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);
    assert_eq!(
        quads.len(),
        30,
        "the column's 9 and 10 quads, the stone's 11"
    );

    // The lower cell of the column: water above culls its top face outright,
    // so neither the top nor the second pass draws — nothing lies at the
    // height the waterfall clause gave its corners.
    let fall = surface_y(64.0, FALL_HEIGHT);
    assert!(
        quads_at(&quads, [0.0, fall, 0.0]).is_empty(),
        "the top face is culled against the water above"
    );
    // Its sides stand at the full height all the same: the corner above the
    // queried cell returns the clause's 1.0, and a culled top face never lands
    // the sink on them. The cell above draws its own north face from its base,
    // which reaches the same vertex — so the face is matched whole.
    let expected = [
        [0.0, fall, 0.001],
        [1.0, fall, 0.001],
        [1.0, 64.0, 0.001],
        [0.0, 64.0, 0.001],
    ];
    let north_face = quads
        .iter()
        .filter(|quad| positions(quad) == expected)
        .count();
    assert_eq!(north_face, 1, "the north face of the lower cell");

    // The cell above keeps its own surface, and the cell under the stone keeps
    // its top too: the up face renders under any non-liquid neighbour.
    let surface = surface_y(65.0, SOURCE_SURFACE);
    assert_eq!(quads_at(&quads, [0.0, surface, 0.0]).len(), 2);
    let covered = surface_y(64.0, SOURCE_SURFACE);
    assert_eq!(quads_at(&quads, [2.0, covered, 0.0]).len(), 2);

    // The upper cell's bottom face culls against the water below it; the
    // stone-covered cell keeps its own.
    assert!(
        quads_at(&quads, [0.0, 65.0, 1.0]).is_empty(),
        "the bottom face culls against the liquid below"
    );
    assert_eq!(quads_at(&quads, [2.0, 64.0, 1.0]).len(), 1);
}

#[test]
fn a_water_cell_under_an_ice_sheet_keeps_the_second_top_pass() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // One water source under a 3 x 3 sheet of ice, so every cell of the ring
    // `shouldRenderSides` reads at `pos.up()` (`BlockFluidRenderer.java:128`,
    // `block/BlockLiquid.java:101-119`) holds ice. Ice does not stop that ring:
    // `block/Block.java:295` assigns the full-block field once from
    // `isOpaqueCube()` while the block is constructed, and `BlockBreakable`'s
    // override answers false (`BlockBreakable.java:29-32`, reached through
    // `BlockIce.java:22`) — so the sheet is a full cube that hides nothing and
    // the second pass draws.
    let world = daylight(&[
        (2, 64, 2, state(WATER_STILL, 0)),
        (1, 65, 1, state(ICE, 0)),
        (2, 65, 1, state(ICE, 0)),
        (3, 65, 1, state(ICE, 0)),
        (1, 65, 2, state(ICE, 0)),
        (2, 65, 2, state(ICE, 0)),
        (3, 65, 2, state(ICE, 0)),
        (1, 65, 3, state(ICE, 0)),
        (2, 65, 3, state(ICE, 0)),
        (3, 65, 3, state(ICE, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);

    // The surface: a lone source's average all round — the ice above is not
    // the same liquid, so the waterfall clause stays silent — sunk by the top
    // pass, the still sprite because the cell's flow vector is zero.
    let y = surface_y(64.0, SOURCE_SURFACE);
    let surface = quads_at(&quads, [2.0, y, 2.0]);
    assert_eq!(surface.len(), 2, "the top and the pass the ice keeps");
    assert_eq!(
        positions(surface[0]),
        [[2.0, y, 2.0], [2.0, y, 3.0], [3.0, y, 3.0], [3.0, y, 2.0]]
    );
    // The second pass walks the same four points in the source's own order —
    // the north-west, north-east, south-east and south-west corners
    // (`BlockFluidRenderer.java:130-133`) — with the top pass's uv.
    assert_eq!(
        positions(surface[1]),
        [[2.0, y, 2.0], [3.0, y, 2.0], [3.0, y, 3.0], [2.0, y, 3.0]]
    );
    let still = rect_corners(STILL);
    assert_eq!(uvs(surface[0]), still);
    assert_eq!(uvs(surface[1]), [still[0], still[3], still[2], still[1]]);
    // Its light and colour are the top pass's: the source computes the pair
    // and the tint once, above the gate, and writes them into both passes.
    assert!(
        surface
            .iter()
            .all(|quad| quad.iter().all(|vertex| vertex.light == [8, 248])),
        "the liquid's own cell and the cell above it, in daylight"
    );
    assert!(
        surface.iter().all(|quad| quad
            .iter()
            .all(|vertex| vertex.colour == [255, 255, 255, 255])),
        "the white tint at the top face's brightness"
    );

    // The water's eleven quads — the top, its reverse, the bottom and the four
    // sides twice — plus the sheet's nine unresolved cubes, six faces apiece,
    // share the layer.
    assert_eq!(quads.len(), 65);
}

#[test]
fn a_lone_water_cell_draws_the_still_top_and_the_flowing_sides() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    let world = daylight(&[(2, 64, 2, state(WATER_STILL, 0))]);
    let mesh = mesh_of(&world, &ctx);

    assert!(mesh.layer(Layer::Opaque).is_empty());
    assert!(mesh.layer(Layer::Cutout).is_empty());
    let quads = quads(&mesh, Layer::Translucent);
    assert_eq!(
        quads.len(),
        11,
        "a surface, its reverse, the bottom, eight side quads"
    );
    assert_eq!(mesh.layer(Layer::Translucent).vertices.len(), 44);
    assert_eq!(mesh.layer(Layer::Translucent).indices.len(), 66);

    // The surface: one height all round (a lone source's three air neighbours
    // give every corner the same average), the still sprite, and the reverse
    // pass behind it.
    let y = surface_y(64.0, SOURCE_SURFACE);
    assert_eq!(SOURCE_RAW, SOURCE_SURFACE + 0.001, "the sink");
    let surface = quads_at(&quads, [2.0, y, 2.0]);
    assert_eq!(surface.len(), 2);
    assert_eq!(
        positions(surface[0]),
        [[2.0, y, 2.0], [2.0, y, 3.0], [3.0, y, 3.0], [3.0, y, 2.0]]
    );
    assert_eq!(
        positions(surface[1]),
        [[2.0, y, 2.0], [3.0, y, 2.0], [3.0, y, 3.0], [2.0, y, 3.0]],
        "the second pass walks the same points in reverse"
    );
    let still = rect_corners(STILL);
    assert_eq!(uvs(surface[0]), still);
    assert_eq!(uvs(surface[1]), [still[0], still[3], still[2], still[1]]);
    // One sample per vertex for every face: the liquid path takes no
    // ambient-occlusion branch, so the two graphics paths' meshes agree.
    let other = context(&models, &atlas, &maps, SmoothLighting::Maximum, true);
    let occluded = mesh_of(&world, &other);
    assert_eq!(
        occluded.layer(Layer::Translucent),
        mesh.layer(Layer::Translucent)
    );

    // The bottom face: still sprite, the block's base, half grey with no tint.
    let bottom = quads_at(&quads, [2.0, 64.0, 3.0]);
    assert_eq!(bottom.len(), 1);
    assert_eq!(
        positions(bottom[0]),
        [
            [2.0, 64.0, 3.0],
            [2.0, 64.0, 2.0],
            [3.0, 64.0, 2.0],
            [3.0, 64.0, 3.0]
        ]
    );
    assert_eq!(
        uvs(bottom[0]),
        [
            [0.5078125, 0.0703125],
            [0.5078125, 0.0078125],
            [0.5703125, 0.0078125],
            [0.5703125, 0.0703125]
        ]
    );
    assert!(
        bottom[0]
            .iter()
            .all(|vertex| vertex.colour == [127, 127, 127, 255])
    );

    // The north face, in its own 0.001 plane: the flowing sprite's left half,
    // the 0.8 shade, and the daylight of the cell it looks into.
    let north = quads_at(&quads, [2.0, y, 2.001]);
    assert_eq!(north.len(), 1);
    assert_eq!(
        positions(north[0]),
        [
            [2.0, y, 2.001],
            [3.0, y, 2.001],
            [3.0, 64.0, 2.001],
            [2.0, 64.0, 2.001]
        ]
    );
    // `getInterpolatedV((1 - f39) * 16 * 0.5)` over the flowing sprite: the
    // surface's eighth of a unit sinks the corner three eighths of a texel.
    let inset = 0.017268354;
    assert_eq!(
        uvs(north[0]),
        [
            [0.6328125, inset],
            [0.6640625, inset],
            [0.6640625, 0.0390625],
            [0.6328125, 0.0390625]
        ]
    );
    assert!(
        north[0]
            .iter()
            .all(|vertex| vertex.colour == [204, 204, 204, 255])
    );
    assert!(north[0].iter().all(|vertex| vertex.light == [8, 248]));
}

#[test]
fn a_liquid_samples_the_cell_and_the_cell_above() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // The liquid's own cell in dim light with a brighter cell above it; the
    // cell below it and the northern neighbour each with their own pair.
    let world = world_of(&[(
        0,
        0,
        column(
            &[(2, 64, 2, state(WATER_STILL, 0))],
            |_, _| 1,
            |x, y, z| match (x, y, z) {
                (2, 64, 2) => (4, 1),
                (2, 65, 2) => (9, 3),
                (2, 63, 2) => (5, 2),
                (2, 64, 1) => (7, 0),
                (2, 65, 1) => (2, 6),
                _ => (15, 0),
            },
        ),
    )]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);
    let y = surface_y(64.0, SOURCE_SURFACE);

    // The top: the per-channel maximum of the cell and the cell above it,
    // packed as the sampler's `level * 16 + 8` in the pair's (block, sky) order.
    let surface = quads_at(&quads, [2.0, y, 2.0]);
    assert_eq!(surface.len(), 2);
    for quad in &surface {
        assert!(quad.iter().all(|vertex| vertex.light == [56, 152]));
    }
    // The bottom: the same maximum over the cell below.
    let bottom = quads_at(&quads, [2.0, 64.0, 3.0]);
    assert!(bottom[0].iter().all(|vertex| vertex.light == [40, 88]));
    // The sides: the maximum over the neighbour the face looks into.
    let north = quads_at(&quads, [2.0, y, 2.001]);
    assert!(north[0].iter().all(|vertex| vertex.light == [104, 120]));
}

#[test]
fn the_water_tint_comes_from_the_nine_sample_biome_colour() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    let world = world_of(&[(0, 0, swamp(&[(5, 64, 5, state(WATER_STILL, 0))]))]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);
    let y = surface_y(64.0, SOURCE_SURFACE);

    // Swamp's water multiplier is `(224, 255, 174)`; both passes of the
    // surface take it at full brightness.
    let surface = quads_at(&quads, [5.0, y, 5.0]);
    assert_eq!(surface.len(), 2);
    for quad in &surface {
        assert!(
            quad.iter()
                .all(|vertex| vertex.colour == [224, 255, 174, 255]),
            "the top is the tint at the top's brightness"
        );
    }
    // The north face multiplies the 0.8 shade by the tint, the west and east
    // faces the 0.6 shade.
    let north = quads_at(&quads, [5.0, y, 5.001]);
    assert!(
        north[0]
            .iter()
            .all(|vertex| vertex.colour == [179, 204, 139, 255])
    );
    let west = quads_at(&quads, [5.001, y, 6.0]);
    assert!(
        west[0]
            .iter()
            .all(|vertex| vertex.colour == [134, 153, 104, 255])
    );
    // The bottom takes the half-grey and no tint at all: the swamp's own
    // `(111, 127, 86)` would be the tinted answer.
    let bottom = quads_at(&quads, [5.0, 64.0, 6.0]);
    assert!(
        bottom[0]
            .iter()
            .all(|vertex| vertex.colour == [127, 127, 127, 255])
    );
}

#[test]
fn a_flowing_liquid_rotates_the_top_sprite() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // A source beside a falling neighbour: the same material still, so the
    // shared face culls, but both cells now have a flow direction.
    let world = daylight(&[
        (0, 64, 0, state(WATER, 5)),
        (1, 64, 0, state(WATER_STILL, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);
    assert_eq!(quads.len(), 18);
    assert!(
        plane_quads(&quads, 0, 0.999).is_empty(),
        "the shared face culls"
    );
    assert!(plane_quads(&quads, 0, 1.001).is_empty());

    // The level-5 cell: its own corner sinks to the air percent's sixth and
    // its eastern corners take the neighbour's eleventh.
    let low = surface_y(64.0, LOW_SURFACE);
    let high = surface_y(64.0, RIM_SURFACE);
    assert_eq!(LOW_RAW - 0.001, LOW_SURFACE, "the level-5 corner's sink");
    assert_eq!(RIM_RAW - 0.001, RIM_SURFACE, "the shared corner's sink");
    let surface = quads_at(&quads, [0.0, low, 0.0]);
    assert_eq!(surface.len(), 2);
    assert_eq!(
        positions(surface[0]),
        [
            [0.0, low, 0.0],
            [0.0, low, 1.0],
            [1.0, high, 1.0],
            [1.0, high, 0.0]
        ]
    );
    // The flow direction is `atan2(z, x) - PI/2` of the normalised sum: both
    // cells' neighbours point the flow west, so the top takes the flowing
    // sprite turned a quarter turn — the sprite's 4/16 and 12/16 fractions in
    // both axes, which the step lands exactly on.
    let turned = [
        [0.6484375, 0.0546875],
        [0.6796875, 0.0546875],
        [0.6796875, 0.0234375],
        [0.6484375, 0.0234375],
    ];
    assert_eq!(uvs(surface[0]), turned);
    assert_eq!(
        uvs(surface[1]),
        [turned[0], turned[3], turned[2], turned[1]]
    );

    // The level-0 cell's surface: the neighbour's falling level on the west,
    // a lone source's average on the east.
    let source = surface_y(64.0, SOURCE_SURFACE);
    let rim = quads_at(&quads, [1.0, high, 0.0]);
    assert_eq!(rim.len(), 2);
    assert_eq!(
        positions(rim[0]),
        [
            [1.0, high, 0.0],
            [1.0, high, 1.0],
            [2.0, source, 1.0],
            [2.0, source, 0.0]
        ]
    );
    assert_eq!(uvs(rim[0]), turned, "both cells' tops are flowing");

    // The side heights carry the tilted surface: the level-5 cell's north
    // face runs from its own sunk corner to the neighbour's.
    let north = quads_at(&quads, [0.0, low, 0.001]);
    assert_eq!(north.len(), 1);
    assert_eq!(
        positions(north[0]),
        [
            [0.0, low, 0.001],
            [1.0, high, 0.001],
            [1.0, 64.0, 0.001],
            [0.0, 64.0, 0.001]
        ]
    );
}

#[test]
fn lava_draws_the_same_geometry_in_the_opaque_layer() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // The swamp biome would tint water; lava's multiplier is the white one.
    let world = world_of(&[(
        0,
        0,
        swamp(&[(0, 64, 0, state(LAVA, 0)), (1, 64, 0, state(LAVA_STILL, 0))]),
    )]);
    let mesh = mesh_of(&world, &ctx);

    // Lava is a solid render layer: the geometry is the water pool's, in the
    // opaque bucket and nothing else.
    assert!(mesh.layer(Layer::Cutout).is_empty());
    assert!(mesh.layer(Layer::Translucent).is_empty());
    let quads = quads(&mesh, Layer::Opaque);
    assert_eq!(quads.len(), 18);

    let low = surface_y(64.0, SOURCE_SURFACE);
    let high = surface_y(64.0, POOL_SURFACE);
    let surface = quads_at(&quads, [0.0, low, 0.0]);
    assert_eq!(surface.len(), 2);
    assert_eq!(
        positions(surface[1]),
        [
            [0.0, low, 0.0],
            [1.0, high, 0.0],
            [1.0, high, 1.0],
            [0.0, low, 1.0]
        ]
    );
    let lava_still = rect_corners(LAVA_STILL_RECT);
    assert_eq!(uvs(surface[0]), lava_still, "the lava still sprite");
    assert!(
        surface[0]
            .iter()
            .all(|vertex| vertex.colour == [255, 255, 255, 255]),
        "lava's multiplier is white"
    );
    for quad in &surface {
        assert!(
            quad.iter().all(|vertex| vertex.light == [8, 248]),
            "the light is the world's own: the renderer asks \
             `getCombinedLight(pos, 0)`, so a block's light value never enters \
             its own quads — a real server's light data already carries the \
             emission it seeded"
        );
    }

    // The sides take the flowing sprite and the 0.8 shade over white.
    let lava_flow = rect_corners(LAVA_FLOW_RECT);
    let north = quads_at(&quads, [0.0, low, 0.001]);
    assert_eq!(north.len(), 1);
    assert_eq!(north[0][0].uv, [0.8828125, 0.017268354]);
    assert_eq!(north[0][3].uv, [lava_flow[0][0], 0.0390625]);
    assert!(
        north[0]
            .iter()
            .all(|vertex| vertex.colour == [204, 204, 204, 255])
    );
    assert!(
        north[0].iter().all(|vertex| vertex.light == [8, 8]),
        "the sides' light is the neighbour's: the cell north of the pair lies \
         outside the loaded column, whose snapshot answers air at zero light \
         while the lava's own cell carries full daylight, so the sample is the \
         neighbour's and not the cell's: {:?}",
        north[0]
            .iter()
            .map(|vertex| vertex.light)
            .collect::<Vec<_>>()
    );

    // The bottom face is the same half-grey the water's is.
    let bottom = quads_at(&quads, [0.0, 64.0, 1.0]);
    assert_eq!(bottom.len(), 1);
    assert_eq!(uvs(bottom[0])[0], [lava_still[0][0], lava_still[1][1]]);
    assert!(
        bottom[0]
            .iter()
            .all(|vertex| vertex.colour == [127, 127, 127, 255])
    );
}

#[test]
fn the_fast_leaves_rule_culls_between_leaves_and_draws_in_the_opaque_layer() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    let world = daylight(&[(0, 64, 0, state(LEAVES, 0)), (1, 64, 0, state(LEAVES, 0))]);
    let mesh = mesh_of(&world, &ctx);

    // The override keys on the material, and exactly the two leaf rows carry
    // it: id 18 and id 161.
    let leaves = covered_ids()
        .iter()
        .filter(|id| behaviour(**id).is_some_and(|entry| entry.material == Material::Leaves))
        .count();
    assert_eq!(leaves, 2);

    // Fast graphics: the table's own values stand — a solid layer and true
    // occlusion — so the leaves' shared faces cull and each draws five.
    assert!(mesh.layer(Layer::Cutout).is_empty());
    assert!(mesh.layer(Layer::Translucent).is_empty());
    let quads = quads(&mesh, Layer::Opaque);
    assert_eq!(quads.len(), 10, "five faces apiece");
    assert!(
        plane_quads(&quads, 0, 1.0).is_empty(),
        "the shared pair culls in the Fast branch"
    );
    assert_eq!(
        plane_quads(&quads, 0, 0.0).len(),
        1,
        "the west cell's west face"
    );
    assert_eq!(
        plane_quads(&quads, 0, 2.0).len(),
        1,
        "the east cell's east face"
    );
    assert_eq!(
        plane_quads(&quads, 1, 65.0).len(),
        2,
        "a top per cell against air"
    );

    // A face against air renders in the opaque layer, in daylight, with the
    // face's own shade and no tint — the probe cube carries no tint index.
    let top = plane_quads(&quads, 1, 65.0);
    for quad in &top {
        assert!(quad.iter().all(|vertex| vertex.light == [8, 248]));
        assert!(
            quad.iter()
                .all(|vertex| vertex.colour == [255, 255, 255, 255])
        );
    }
}

#[test]
fn the_fancy_leaves_rule_moves_every_leaf_quad_to_the_mipped_cutout_layer() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, false);
    let world = daylight(&[(0, 64, 0, state(LEAVES, 0)), (1, 64, 0, state(LEAVES, 0))]);
    let mesh = mesh_of(&world, &ctx);

    // Fancy graphics names the other column of the leaves pair: `CUTOUT_MIPPED`
    // (`BlockLeaves.getBlockLayer`, `BlockLeaves.java:293-296`), and no
    // occlusion — so the shared pair draws on both cells and every leaf quad
    // has left the opaque bucket. The plain cutout stays empty: it is the
    // plants' layer, drawn with the atlas's level-0 sampler.
    assert!(mesh.layer(Layer::Opaque).is_empty());
    assert!(mesh.layer(Layer::Cutout).is_empty());
    assert!(mesh.layer(Layer::Translucent).is_empty());
    let quads = quads(&mesh, Layer::CutoutMipped);
    assert_eq!(quads.len(), 12, "six faces apiece");
    assert_eq!(
        plane_quads(&quads, 0, 1.0).len(),
        2,
        "both internal faces draw"
    );
    assert_eq!(plane_quads(&quads, 0, 0.0).len(), 1);
    assert_eq!(plane_quads(&quads, 0, 2.0).len(), 1);
    assert_eq!(plane_quads(&quads, 1, 64.0).len(), 2);
    assert_eq!(plane_quads(&quads, 1, 65.0).len(), 2);
}

#[test]
fn a_plant_lands_in_the_cutout_layer_at_full_brightness() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // The tall grass's plant variant, meta 1: a cross whose elements turned
    // shading off.
    let world = daylight(&[(4, 64, 4, state(TALLGRASS, 1))]);
    let mesh = mesh_of(&world, &ctx);

    assert!(mesh.layer(Layer::Opaque).is_empty());
    assert!(mesh.layer(Layer::Translucent).is_empty());
    let cutout = mesh.layer(Layer::Cutout);
    assert_eq!(cutout.vertices.len(), 16, "two elements, two faces each");
    assert_eq!(cutout.indices.len(), 24);
    // The quad's own cell, in daylight: nothing culls a cross.
    for vertex in &cutout.vertices {
        assert_eq!(vertex.light, [8, 248]);
        assert_eq!(vertex.colour, [255, 255, 255, 255]);
    }
}

#[test]
fn a_mixed_section_splits_into_three_layers() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Maximum, true);
    // Stone at the corner, water two cells away, a plant two cells beyond it:
    // nothing culls anything else.
    let world = daylight(&[
        (0, 64, 0, state(STONE, 0)),
        (2, 64, 2, state(WATER, 0)),
        (4, 64, 4, state(TALLGRASS, 1)),
    ]);
    let mesh = mesh_of(&world, &ctx);

    // Six faces of stone, four of plant, eleven of water — one mesh, three
    // buckets, in the layers' own index order.
    assert!(!mesh.is_empty(), "the section draws");
    assert_eq!(mesh.layer(Layer::Opaque).vertices.len(), 24);
    assert_eq!(mesh.layer(Layer::Opaque).indices.len(), 36);
    assert_eq!(mesh.layer(Layer::Cutout).vertices.len(), 16);
    assert_eq!(mesh.layer(Layer::Cutout).indices.len(), 24);
    assert_eq!(mesh.layer(Layer::Translucent).vertices.len(), 44);
    assert_eq!(mesh.layer(Layer::Translucent).indices.len(), 66);
    assert_eq!(mesh.vertex_count(), 84, "the sum across the layers");
    // The indices address their own layer's vertices from zero, quad by quad.
    for layer in [Layer::Opaque, Layer::Cutout, Layer::Translucent] {
        let indices = &mesh.layer(layer).indices;
        assert_eq!(indices.len(), mesh.layer(layer).vertices.len() / 4 * 6);
        for (index, chunk) in indices.chunks(6).enumerate() {
            let base = (index * 4) as u32;
            assert_eq!(
                chunk,
                [base, base + 1, base + 2, base, base + 2, base + 3],
                "{layer:?}"
            );
        }
    }
}

#[test]
fn the_translucent_layer_sorts_back_to_front() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // Three lone cells on one row: the middle one is nearest the section's
    // centre, the outer two share the farthest distance.
    let world = daylight(&[
        (0, 64, 8, state(WATER_STILL, 0)),
        (8, 64, 8, state(WATER_STILL, 0)),
        (15, 64, 8, state(WATER_STILL, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);
    assert_eq!(quads.len(), 33, "eleven apiece");

    // Descending distance from the section's centre (8, 72, 8): every quad's
    // centre is the mean of its four vertices.
    let centre = [8.0, 72.0, 8.0];
    let distances: Vec<f32> = quads
        .iter()
        .map(|quad| quad_distance(quad, centre))
        .collect();
    for pair in distances.windows(2) {
        assert!(
            pair[0] >= pair[1],
            "the translucent layer runs back to front, {} then {}",
            pair[0],
            pair[1]
        );
    }
    // The farthest cell draws first and the nearest one last.
    let farthest = distances.iter().copied().fold(f32::MIN, f32::max);
    assert_eq!(distances[0], farthest);
    let first = quads[0];
    assert!(
        in_cell(first, [0.0, 64.0, 8.0]) || in_cell(first, [15.0, 64.0, 8.0]),
        "the first quad belongs to one of the far cells"
    );
    let last = quads[quads.len() - 1];
    assert!(
        in_cell(last, [8.0, 64.0, 8.0]),
        "the nearest cell draws last"
    );
    // Counts per cell unchanged by the order: the sort is a permutation.
    assert_eq!(cell_quads(&quads, [0.0, 64.0, 8.0]).len(), 11);
    assert_eq!(cell_quads(&quads, [8.0, 64.0, 8.0]).len(), 11);
    assert_eq!(cell_quads(&quads, [15.0, 64.0, 8.0]).len(), 11);
}

#[test]
fn a_stable_sort_keeps_the_visit_order_on_ties() {
    let (models, atlas) = loaded();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // Two cells mirrored about the centre's x: their quads' distances pair up
    // equal to the bit, and the visit order decides which of a pair leads.
    let world = daylight(&[
        (5, 64, 8, state(WATER_STILL, 0)),
        (10, 64, 8, state(WATER_STILL, 0)),
    ]);
    let mesh = mesh_of(&world, &ctx);
    let quads = quads(&mesh, Layer::Translucent);
    assert_eq!(quads.len(), 22);
    let centre = [8.0, 72.0, 8.0];

    // Descending, and every quad of the later cell whose distance ties with one
    // of the earlier cell's sits behind it — the order a stable sort keeps.
    // Nine of the eleven pairs tie to the bit; the two inner faces' own planes
    // carry the sink at 5.001 against 10.999, whose distances miss by a bit
    // and sort by the distance itself.
    let distances: Vec<f32> = quads
        .iter()
        .map(|quad| quad_distance(quad, centre))
        .collect();
    for pair in distances.windows(2) {
        assert!(pair[0] >= pair[1], "descending distance");
    }
    let mut ties = 0;
    for (index, quad) in quads.iter().enumerate() {
        if !in_cell(quad, [10.0, 64.0, 8.0]) {
            continue;
        }
        let partner = quads.iter().position(|other| {
            in_cell(other, [5.0, 64.0, 8.0])
                && quad_distance(other, centre) == quad_distance(quad, centre)
        });
        if let Some(partner) = partner {
            ties += 1;
            assert!(
                partner < index,
                "the earlier cell keeps the earlier place: quad {index} at {:?}",
                positions(quad)
            );
        }
    }
    assert_eq!(ties, 9, "nine of the eleven pairs tie to the bit");
}

// -- the animated strips -----------------------------------------------------

/// The content rect of one liquid strip in [`strip_atlas`], as its min and
/// max uv corner: the whole strip and its first frame. The atlas is 128 x 128
/// and each strip sits at its cell's top-left corner, as the stitcher places
/// content.
const STRIP_STILL: ([f32; 2], [f32; 2]) = ([0.5, 0.0], [0.625, 0.25]);
const FRAME_STILL: ([f32; 2], [f32; 2]) = ([0.5, 0.0], [0.625, 0.125]);
const STRIP_FLOW: ([f32; 2], [f32; 2]) = ([0.75, 0.0], [1.0, 0.5]);
const FRAME_FLOW: ([f32; 2], [f32; 2]) = ([0.75, 0.0], [1.0, 0.25]);
const STRIP_LAVA_STILL: ([f32; 2], [f32; 2]) = ([0.5, 0.5], [0.625, 0.75]);
const FRAME_LAVA_STILL: ([f32; 2], [f32; 2]) = ([0.5, 0.5], [0.625, 0.625]);
const STRIP_LAVA_FLOW: ([f32; 2], [f32; 2]) = ([0.75, 0.5], [1.0, 1.0]);
const FRAME_LAVA_FLOW: ([f32; 2], [f32; 2]) = ([0.75, 0.5], [1.0, 0.75]);

/// The atlas these strip tests mesh with: the fallback and the three probe
/// sprites as in [`test_atlas`], and the four liquid sprites as animated
/// strips — the still pair two 16 x 16 rows over a 16 x 32 content, the
/// flowing pair two 32 x 32 rows over a 32 x 64 content — each inside its own
/// cell, its content at the cell's top-left corner.
///
/// The strip's content rect is taller than one frame, so its uv pair differs
/// from the frame's on the v axis alone; a mesher that maps a quad into the
/// strip draws a band of both frames where the source draws one.
fn strip_atlas() -> Atlas {
    let mut atlas = atlas(&[
        "missingno",
        "blocks/probe",
        "blocks/probe_overlay",
        "blocks/probe_cross",
    ]);
    let strips = [
        ("blocks/water_still", 64u32, 0u32, 16u32, 32u32),
        ("blocks/water_flow", 96, 0, 32, 64),
        ("blocks/lava_still", 64, 64, 16, 32),
        ("blocks/lava_flow", 96, 64, 32, 64),
    ];
    for (name, x, y, side, height) in strips {
        let region = SpriteRect {
            x,
            y,
            w: side,
            h: height,
        };
        let content = SpriteRect {
            x,
            y,
            w: side,
            h: 2 * side,
        };
        atlas
            .sprites
            .insert(name.to_string(), AtlasSprite { region, content });
        atlas.animated.insert(
            name.to_string(),
            AnimatedSprite {
                frames: (0..2)
                    .map(|row| AtlasSprite {
                        region,
                        content: SpriteRect {
                            x,
                            y: y + row * side,
                            w: side,
                            h: side,
                        },
                    })
                    .collect(),
                times: vec![2, 2],
                interpolate: false,
            },
        );
    }
    atlas
}

#[test]
fn a_liquid_between_glass_panes_draws_the_sprite_frame_and_the_doubled_faces() {
    let (models, _) = loaded();
    let atlas = strip_atlas();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // One still water source with glass before and behind it and stone on
    // every other side: the sample wall's own shape for its four cells.
    let world = daylight(&[
        (1, 64, 1, state(WATER_STILL, 0)),
        (1, 64, 0, state(GLASS, 0)),
        (1, 64, 2, state(GLASS, 0)),
        (0, 64, 1, state(STONE, 0)),
        (2, 64, 1, state(STONE, 0)),
        (1, 63, 1, state(STONE, 0)),
        (1, 65, 1, state(STONE, 0)),
    ]);
    // The floor stone below the liquid puts its own faces in section 3, so
    // the liquid's are read from the section that holds y = 64.
    let mesh = meshes(&world, &ctx)
        .into_iter()
        .find_map(|(index, mesh)| (index == 4).then_some(mesh).flatten())
        .expect("the liquid's own section");

    // Glass is a full cube but not an opaque one: the base rule's
    // `!isOpaqueCube()` arm (`BlockLiquid.java:96-99`) draws the liquid's own
    // face against it, and `BlockBreakable.shouldSideBeRendered` (`:52`)
    // draws the glass's face back. The four panes' faces are the glass
    // model's; the liquid's own are the six here: the surface's two passes
    // and one face against each pane, doubled as the source doubles them.
    let quads = quads(&mesh, Layer::Translucent);
    let cell = cell_quads(&quads, [1.0, 64.0, 1.0]);
    assert_eq!(cell.len(), 6, "the surface twice, each pane's face twice");
    assert_eq!(mesh.layer(Layer::Translucent).vertices.len(), 24);
    assert_eq!(mesh.layer(Layer::Translucent).indices.len(), 36);

    // The surface: one height all round — the pane and the wall's stone are
    // both solid samples the height average drops, so the cell's corner is
    // [`POOL_SURFACE`] — the still sprite's first frame, and the reverse pass
    // behind it.
    let y = surface_y(64.0, POOL_SURFACE);
    let top = quads_at(&cell, [1.0, y, 1.0]);
    assert_eq!(top.len(), 2);
    let frame = rect_corners(FRAME_STILL);
    assert_eq!(
        uvs(top[0]),
        frame,
        "the surface maps into the still frame, not the strip"
    );
    assert_eq!(uvs(top[1]), [frame[0], frame[3], frame[2], frame[1]]);
    assert_ne!(
        uvs(top[0])[2][1],
        STRIP_STILL.1[1],
        "the surface does not reach the strip's second frame"
    );
    // The surface's shade over the plains water multiplier: white, one
    // sample per vertex, and the cell's own daylight.
    assert!(
        top.iter()
            .flat_map(|quad| quad.iter())
            .all(|vertex| vertex.colour == [255, 255, 255, 255])
    );
    assert!(
        top.iter()
            .flat_map(|quad| quad.iter())
            .all(|vertex| vertex.light == [8, 248])
    );

    // Each pane's face: the flowing sprite's first frame, the face's half of
    // its u span, and the two heights' v coordinates over the frame. The
    // fractions are the lone-cell test's own — `(1 - height) * 16 * 0.5` and
    // `8.0` of `getInterpolatedV`'s sixteen — carried over a quarter of the
    // atlas instead of a sixteenth, with the pool's corner height.
    let (flow_min, flow_max) = FRAME_FLOW;
    let span = flow_max[1] - flow_min[1];
    let v_top = span * (((1.0 - POOL_SURFACE) * 16.0 * 0.5) / 16.0);
    let v_bottom = span * (8.0 / 16.0);
    let u_mid = flow_min[0] + (flow_max[0] - flow_min[0]) * (8.0 / 16.0);
    let expected = [
        [flow_min[0], v_top],
        [u_mid, v_top],
        [u_mid, v_bottom],
        [flow_min[0], v_bottom],
    ];
    let north = quads_at(&cell, [1.0, y, 1.001]);
    assert_eq!(north.len(), 1);
    assert_eq!(uvs(north[0]), expected, "the north face's frame corners");
    assert_ne!(
        uvs(north[0])[2][1],
        STRIP_FLOW.1[1],
        "the face does not reach the strip's second frame"
    );
    let behind = quads_at(&cell, [1.0, 64.0, 1.001]);
    assert_eq!(behind.len(), 1, "the doubled face behind the first");
    assert_eq!(
        uvs(behind[0]),
        [expected[3], expected[2], expected[1], expected[0]]
    );
    let south = quads_at(&cell, [2.0, y, 1.999]);
    assert_eq!(south.len(), 1);
    assert_eq!(uvs(south[0]), expected, "the south face's frame corners");
    assert_eq!(quads_at(&cell, [2.0, 64.0, 1.999]).len(), 1);
    for quad in [north[0], behind[0], south[0]] {
        // The side shade over white, and the pane's own daylight.
        assert!(
            quad.iter()
                .all(|vertex| vertex.colour == [204, 204, 204, 255])
        );
        assert!(quad.iter().all(|vertex| vertex.light == [8, 248]));
    }
}

#[test]
fn a_flowing_liquid_strip_draws_the_flowing_frame() {
    let (models, _) = loaded();
    let atlas = strip_atlas();
    let maps = white_maps();
    let ctx = context(&models, &atlas, &maps, SmoothLighting::Off, true);
    // A lava source in the wall's own row: glass before and behind it, stone
    // on every other side. Lava draws in the opaque layer.
    let world = daylight(&[
        (1, 64, 1, state(LAVA_STILL, 0)),
        (1, 64, 0, state(GLASS, 0)),
        (1, 64, 2, state(GLASS, 0)),
        (0, 64, 1, state(STONE, 0)),
        (2, 64, 1, state(STONE, 0)),
        (1, 63, 1, state(STONE, 0)),
        (1, 65, 1, state(STONE, 0)),
    ]);
    let mesh = meshes(&world, &ctx)
        .into_iter()
        .find_map(|(index, mesh)| (index == 4).then_some(mesh).flatten())
        .expect("the liquid's own section");

    let quads = quads(&mesh, Layer::Opaque);
    let cell = cell_quads(&quads, [1.0, 64.0, 1.0]);
    // The opaque layer also carries the neighbouring stone's faces, which sit
    // on the cell's own planes; the lava's are the ones that map into the
    // lava strip's cells, the stone's into the probe sprite's.
    let lava: Vec<&[Vertex]> = cell
        .into_iter()
        .filter(|quad| {
            quad.iter()
                .all(|vertex| vertex.uv[0] >= STRIP_LAVA_STILL.0[0])
        })
        .collect();
    assert_eq!(lava.len(), 6, "the surface twice, each pane's face twice");
    let y = surface_y(64.0, POOL_SURFACE);
    let top = quads_at(&lava, [1.0, y, 1.0]);
    assert_eq!(top.len(), 2);
    let frame = rect_corners(FRAME_LAVA_STILL);
    assert_eq!(uvs(top[0]), frame, "the still frame, not the lava strip");
    assert_ne!(
        uvs(top[0])[2][1],
        STRIP_LAVA_STILL.1[1],
        "the surface does not reach the strip's second frame"
    );
    let north = quads_at(&lava, [1.0, y, 1.001]);
    let (flow_min, flow_max) = FRAME_LAVA_FLOW;
    let span = flow_max[1] - flow_min[1];
    let v_top = flow_min[1] + span * (((1.0 - POOL_SURFACE) * 16.0 * 0.5) / 16.0);
    let u_mid = flow_min[0] + (flow_max[0] - flow_min[0]) * (8.0 / 16.0);
    assert_eq!(
        uvs(north[0]),
        [
            [flow_min[0], v_top],
            [u_mid, v_top],
            [u_mid, flow_min[1] + span * (8.0 / 16.0)],
            [flow_min[0], flow_min[1] + span * (8.0 / 16.0)],
        ],
        "the flowing frame, not the lava strip"
    );
    assert_ne!(
        uvs(north[0])[2][1],
        STRIP_LAVA_FLOW.1[1],
        "the face does not reach the strip's second frame"
    );
}
