//! The blockstate and model loader's tests.
//!
//! The synthetic tree is hand-written JSON in a temp directory, laid out the
//! way the extractor lays it out (`assets/minecraft/blockstates/`,
//! `assets/minecraft/models/`): no file from the game is involved and no
//! fixture carries a Mojang byte. The shapes follow the survey's examples
//! (`docs/research/render-parity-survey.md` sections 1.2 and 1.3).
//!
//! The expected corners and uvs are derived from the 1.8 client's own
//! formulas (`FaceBakery`, `BlockFaceUV`, `BlockPart`, `ModelRotation`), not
//! from this crate's implementation.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use oxide_assets::model::{BakedQuad, FaceDir, ModelError, ModelJson, ModelSource, Variant};

/// `models/block/cube.json`: one element, six faces, every face culled.
const CUBE: &[u8] = br##"{
  "elements": [
    {
      "from": [0, 0, 0],
      "to": [16, 16, 16],
      "faces": {
        "down":  { "uv": [0, 0, 16, 16], "texture": "#down",  "cullface": "down" },
        "up":    { "uv": [0, 0, 16, 16], "texture": "#up",    "cullface": "up" },
        "north": { "uv": [0, 0, 16, 16], "texture": "#north", "cullface": "north" },
        "south": { "uv": [0, 0, 16, 16], "texture": "#south", "cullface": "south" },
        "west":  { "uv": [0, 0, 16, 16], "texture": "#west",  "cullface": "west" },
        "east":  { "uv": [0, 0, 16, 16], "texture": "#east",  "cullface": "east" }
      }
    }
  ]
}"##;

/// `models/block/cube_all.json`: the survey's pure-inheritance helper.
const CUBE_ALL: &[u8] = br##"{
  "parent": "block/cube",
  "textures": {
    "particle": "#all",
    "down": "#all", "up": "#all", "north": "#all",
    "south": "#all", "west": "#all", "east": "#all"
  }
}"##;

/// `models/block/stone.json`, the survey's example.
const STONE: &[u8] = br##"{
  "parent": "block/cube_all",
  "textures": { "all": "blocks/stone" }
}"##;

/// A grass block on the same cube, so the parent chain resolves a second time.
const GRASS_NORMAL: &[u8] = br##"{
  "parent": "block/cube_all",
  "textures": { "all": "blocks/grass_top" }
}"##;

/// The snowy grass block of the `snowy=true` variant.
const GRASS_SNOWED: &[u8] = br##"{
  "parent": "block/cube_all",
  "textures": { "all": "blocks/grass_side_snowed" }
}"##;

/// `models/block/stairs.json`: the survey's two-element example. The texture
/// variables are the child's, as in the real tree.
const STAIRS: &[u8] = br##"{
  "textures": { "particle": "#side" },
  "elements": [
    {
      "from": [0, 0, 0],
      "to": [16, 8, 16],
      "faces": {
        "down":  { "uv": [0, 0, 16, 16], "texture": "#bottom", "cullface": "down" },
        "up":    { "uv": [0, 0, 16, 16], "texture": "#top" },
        "north": { "uv": [0, 8, 16, 16], "texture": "#side", "cullface": "north" },
        "south": { "uv": [0, 8, 16, 16], "texture": "#side", "cullface": "south" },
        "west":  { "uv": [0, 8, 16, 16], "texture": "#side" },
        "east":  { "uv": [0, 8, 16, 16], "texture": "#side" }
      }
    },
    {
      "from": [8, 8, 0],
      "to": [16, 16, 16],
      "faces": {
        "up":    { "uv": [0, 0, 8, 8], "texture": "#top" },
        "north": { "uv": [0, 0, 8, 8], "texture": "#side" },
        "south": { "uv": [0, 0, 8, 8], "texture": "#side" },
        "west":  { "uv": [0, 0, 16, 8], "texture": "#side" },
        "east":  { "uv": [0, 0, 16, 8], "texture": "#side" }
      }
    }
  ]
}"##;

/// The stairs' child: it defines the variables the parent's faces name.
const OAK_STAIRS: &[u8] = br##"{
  "parent": "block/stairs",
  "textures": {
    "bottom": "blocks/stairs_bottom",
    "top": "blocks/stairs_top",
    "side": "blocks/stairs_side"
  }
}"##;

/// `models/block/cross.json`: the survey's element-rotation example.
const CROSS: &[u8] = br##"{
  "ambientocclusion": false,
  "textures": { "particle": "#cross" },
  "elements": [
    {
      "from": [0.8, 0, 8],
      "to": [15.2, 16, 8],
      "rotation": { "origin": [8, 8, 8], "axis": "y", "angle": 45, "rescale": true },
      "shade": false,
      "faces": {
        "north": { "uv": [0, 0, 16, 16], "texture": "#cross" },
        "south": { "uv": [0, 0, 16, 16], "texture": "#cross" }
      }
    },
    {
      "from": [8, 0, 0.8],
      "to": [8, 16, 15.2],
      "rotation": { "origin": [8, 8, 8], "axis": "y", "angle": 45, "rescale": true },
      "shade": false,
      "faces": {
        "west": { "uv": [0, 0, 16, 16], "texture": "#cross" },
        "east": { "uv": [0, 0, 16, 16], "texture": "#cross" }
      }
    }
  ]
}"##;

/// The plant the cross bakes for: it defines the texture variable.
const PLANT: &[u8] = br##"{
  "parent": "block/cross",
  "textures": { "cross": "blocks/plant" }
}"##;

/// The ground torch's shape, as the real `block/torch` carries it.
const TORCH: &[u8] = br##"{
  "ambientocclusion": false,
  "textures": { "particle": "#torch" },
  "elements": [
    {
      "from": [7, 0, 7],
      "to": [9, 10, 9],
      "shade": false,
      "faces": {
        "up":   { "uv": [7, 6, 9, 8], "texture": "#torch" },
        "down": { "uv": [7, 13, 9, 15], "texture": "#torch" }
      }
    },
    {
      "from": [7, 0, 0],
      "to": [9, 16, 16],
      "shade": false,
      "faces": {
        "west": { "uv": [0, 0, 16, 16], "texture": "#torch" },
        "east": { "uv": [0, 0, 16, 16], "texture": "#torch" }
      }
    },
    {
      "from": [0, 0, 7],
      "to": [16, 16, 9],
      "shade": false,
      "faces": {
        "north": { "uv": [0, 0, 16, 16], "texture": "#torch" },
        "south": { "uv": [0, 0, 16, 16], "texture": "#torch" }
      }
    }
  ]
}"##;

/// The torch the `facing=up` variant names.
const NORMAL_TORCH: &[u8] = br##"{
  "parent": "block/torch",
  "textures": { "torch": "blocks/torch_on" }
}"##;

/// The wall torch's shape: one stick against the north side of its cell.
const TORCH_WALL: &[u8] = br##"{
  "ambientocclusion": false,
  "textures": { "particle": "#torch" },
  "elements": [
    {
      "from": [7, 3, 0],
      "to": [9, 13, 2],
      "shade": false,
      "faces": {
        "down":  { "uv": [7, 8, 9, 10], "texture": "#torch" },
        "up":    { "uv": [7, 6, 9, 8], "texture": "#torch" },
        "north": { "uv": [7, 6, 9, 16], "texture": "#torch" },
        "south": { "uv": [7, 6, 9, 16], "texture": "#torch" },
        "west":  { "uv": [0, 6, 2, 16], "texture": "#torch" },
        "east":  { "uv": [0, 6, 2, 16], "texture": "#torch" }
      }
    }
  ]
}"##;

/// The torch the wall variants name.
const NORMAL_TORCH_WALL: &[u8] = br##"{
  "parent": "block/torch_wall",
  "textures": { "torch": "blocks/torch_on" }
}"##;

/// A probe block: one element carrying a face `uv`, a face `rotation`, a
/// `tintindex`, a `cullface`, and a face that leaves `uv` to the default.
const PROBE: &[u8] = br##"{
  "ambientocclusion": false,
  "textures": {
    "particle": "#side",
    "side": "blocks/probe_side",
    "top": "blocks/probe_top"
  },
  "elements": [
    {
      "from": [0, 0, 0],
      "to": [16, 8, 16],
      "faces": {
        "down":  { "uv": [0, 0, 16, 16], "texture": "#side", "cullface": "down", "tintindex": 2 },
        "up":    { "uv": [2, 3, 10, 11], "texture": "#top", "rotation": 90 },
        "north": { "texture": "#side" },
        "south": { "uv": [0, 0, 16, 16], "texture": "#side" },
        "west":  { "uv": [0, 0, 16, 16], "texture": "#side", "rotation": 180 },
        "east":  { "uv": [0, 0, 16, 16], "texture": "#side", "rotation": 270 }
      }
    }
  ]
}"##;

/// An item model of the generated kind: five layers over one flat plane.
const GENERATED_ITEM: &[u8] = br##"{
  "parent": "builtin/generated",
  "textures": {
    "layer0": "items/thing",
    "layer1": "items/thing_glint",
    "layer2": "items/thing_c",
    "layer3": "items/thing_d",
    "layer4": "items/thing_e"
  }
}"##;

/// The missing-model parent.
const MISSING_ITEM: &[u8] = br##"{ "parent": "builtin/missing" }"##;

/// The block-entity parent.
const ENTITY_ITEM: &[u8] = br##"{ "parent": "builtin/entity" }"##;

/// A builtin the client does not know.
const UNKNOWN_BUILTIN: &[u8] = br##"{ "parent": "builtin/gizmo" }"##;

/// A parent that names no file in the tree.
const ORPHAN: &[u8] = br##"{ "parent": "block/no_such_model" }"##;

/// Two models whose parents name each other.
const CYCLE_A: &[u8] = br##"{ "parent": "block/cycle_b" }"##;
const CYCLE_B: &[u8] = br##"{ "parent": "block/cycle_a" }"##;

/// A face whose texture variable nothing defines.
const NO_VARIABLE: &[u8] = br##"{
  "textures": { "particle": "#ghost_texture" },
  "elements": [
    {
      "from": [0, 0, 0],
      "to": [16, 16, 16],
      "faces": {
        "up": { "uv": [0, 0, 16, 16], "texture": "#ghost_texture" }
      }
    }
  ]
}"##;

/// An element whose `from` sits past its `to` on the x axis, the way the real
/// tree's six fire models carry one. The client bakes it as its own hull.
const INVERTED: &[u8] = br##"{
  "textures": { "particle": "#side", "side": "blocks/inverted_side" },
  "elements": [
    {
      "from": [15.99, 1, 0],
      "to": [0.01, 23.4, 16],
      "faces": {
        "up": { "uv": [0, 0, 16, 16], "texture": "#side" }
      }
    }
  ]
}"##;

#[test]
fn blockstates_parse_their_variants_and_fields() {
    let tree = Tree::new();
    tree.write("assets/minecraft/models/block/stone.json", STONE);
    tree.write("assets/minecraft/blockstates/stone.json", STONE_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/torch.json", TORCH_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/grass.json", GRASS_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/probe.json", PROBE_BLOCKSTATE);

    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    let stone = source.blockstates("stone").expect("stone parses");
    assert_eq!(stone.variants.len(), 1);
    let entry = &stone.variants[""];
    assert_eq!(entry.len(), 1);
    assert_eq!(
        entry[0],
        Variant {
            model: "stone".to_string(),
            x: 0,
            y: 0,
            uvlock: false,
            weight: 1,
        },
        "the empty key variant names block/stone with every default"
    );

    let torch = source.blockstates("torch").expect("torch parses");
    assert_eq!(torch.variants.len(), 5, "the report's five facing variants");
    assert_eq!(torch.variants["facing=up"][0].y, 0);
    assert_eq!(torch.variants["facing=east"][0].model, "normal_torch_wall");
    assert_eq!(torch.variants["facing=south"][0].y, 90);
    assert_eq!(torch.variants["facing=west"][0].y, 180);
    assert_eq!(torch.variants["facing=north"][0].y, 270);
    assert!(
        torch
            .variants
            .values()
            .flatten()
            .all(|variant| !variant.uvlock && variant.weight == 1),
        "the torch states neither uvlock nor weight"
    );

    let grass = source.blockstates("grass").expect("grass parses");
    let unsnowed = &grass.variants["snowy=false"];
    assert_eq!(unsnowed.len(), 4, "the four-element weighted array");
    assert_eq!(
        unsnowed.iter().map(|v| v.y).collect::<Vec<_>>(),
        [0, 90, 180, 270],
        "the four rotations of the grass top"
    );
    assert!(
        unsnowed
            .iter()
            .all(|v| v.model == "grass_normal" && v.weight == 1),
        "the array is four identical models at the default weight"
    );
    assert_eq!(grass.variants["snowy=true"][0].model, "grass_snowed");

    let probe = source.blockstates("probe").expect("probe parses");
    assert!(probe.variants["locked"][0].uvlock, "uvlock is preserved");
    assert!(!probe.variants["free"][0].uvlock);
    assert_eq!(probe.variants["tilted"][0].x, 180);
    assert_eq!(probe.variants["tilted"][0].y, 270);
    let weighted = &probe.variants["weighted"];
    assert_eq!(weighted.len(), 2, "an array stays an array");
    assert_eq!(
        weighted.iter().map(|v| v.weight).collect::<Vec<_>>(),
        [2, 1],
        "the declared weights are kept"
    );
}

#[test]
fn the_cube_bakes_six_culled_quads_with_pinned_corners() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let stone = source.blockstates("stone").expect("stone parses");
    let baked = source
        .bake_variant(&stone.variants[""][0])
        .expect("blocks/stone bakes");

    assert_eq!(baked.quads.len(), 6, "one quad per cube face");
    assert!(baked.ambient_occlusion, "the cube does not state otherwise");
    assert!(!baked.missing);
    assert_eq!(baked.particle.as_deref(), Some("blocks/stone"));

    // The client's canonical order per face (EnumFaceDirection) with the
    // uv corners in vertex-index order: (u0,v0), (u0,v1), (u1,v1), (u1,v0).
    let full_uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    let expected: [(FaceDir, [[f32; 3]; 4]); 6] = [
        (
            FaceDir::Down,
            [
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
            ],
        ),
        (
            FaceDir::Up,
            [
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0],
            ],
        ),
        (
            FaceDir::North,
            [
                [1.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
        ),
        (
            FaceDir::South,
            [
                [0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
            ],
        ),
        (
            FaceDir::West,
            [
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
        ),
        (
            FaceDir::East,
            [
                [1.0, 1.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
        ),
    ];
    for (quad, (face, corners)) in baked.quads.iter().zip(expected) {
        assert_quad(quad, corners, full_uv, &format!("the {face:?} face"));
        assert_eq!(
            quad.texture, "blocks/stone",
            "the parent chain resolved #all"
        );
        assert_eq!(quad.cullface, Some(face), "every cube face is culled");
        assert_eq!(quad.tintindex, None);
        assert!(quad.shade);
    }
}

#[test]
fn the_face_rotation_turns_the_texture_quarter_by_quarter() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let probe = source.blockstates("probe").expect("probe parses");
    let baked = source
        .bake_variant(&probe.variants[""][0])
        .expect("probe bakes");
    assert_eq!(baked.quads.len(), 6);

    // The upper face carries uv [2, 3, 10, 11] and rotation 90, so every
    // corner steps one place through the uv cycle (u0,v0)->(u0,v1)->
    // (u1,v1)->(u1,v0).
    let up = &baked.quads[1];
    assert_uv(
        up,
        [
            [2.0 / 16.0, 11.0 / 16.0],
            [10.0 / 16.0, 11.0 / 16.0],
            [10.0 / 16.0, 3.0 / 16.0],
            [2.0 / 16.0, 3.0 / 16.0],
        ],
        "the up face turned a quarter",
    );
    assert_close(
        &up.corners,
        [
            [0.0, 0.5, 0.0],
            [0.0, 0.5, 1.0],
            [1.0, 0.5, 1.0],
            [1.0, 0.5, 0.0],
        ],
        "the up face's corners",
    );

    // The north face states no uv: the default comes from the element bounds,
    // [x0, 16 - y1, x1, 16 - y0] = [0, 8, 16, 16].
    let north = &baked.quads[2];
    assert_uv(
        north,
        [[0.0, 0.5], [0.0, 1.0], [1.0, 1.0], [1.0, 0.5]],
        "the north face's default uv",
    );

    // Rotation 180 of a full uv: the cycle steps twice.
    let west = &baked.quads[4];
    assert_uv(
        west,
        [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]],
        "the west face turned half",
    );
    // Rotation 270: three places.
    let east = &baked.quads[5];
    assert_uv(
        east,
        [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        "the east face turned three quarters",
    );

    let down = &baked.quads[0];
    assert_eq!(
        down.cullface,
        Some(FaceDir::Down),
        "the cullface is carried"
    );
    assert_eq!(down.tintindex, Some(2), "the tintindex is carried");
    assert_eq!(down.texture, "blocks/probe_side");
    assert_eq!(up.texture, "blocks/probe_top");
}

#[test]
fn an_element_rotation_rotates_the_corners_and_keeps_its_uvs() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let plant = source.blockstates("plant").expect("plant parses");
    let baked = source
        .bake_variant(&plant.variants[""][0])
        .expect("the cross bakes");

    assert_eq!(baked.quads.len(), 4, "two planes, two faces each");
    assert!(!baked.ambient_occlusion, "cross.json turns occlusion off");
    assert!(!baked.missing);
    assert_eq!(baked.particle.as_deref(), Some("blocks/plant"));

    // 45 degrees about the y axis through (0.5, 0.5, 0.5), with rescale: the
    // 0.8..15.2 plane lands on the diagonal at 0.05..0.95.
    let full_uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    assert_quad(
        &baked.quads[0],
        [
            [0.95, 1.0, 0.05],
            [0.95, 0.0, 0.05],
            [0.05, 0.0, 0.95],
            [0.05, 1.0, 0.95],
        ],
        full_uv,
        "the cross's north face",
    );
    assert_quad(
        &baked.quads[1],
        [
            [0.05, 1.0, 0.95],
            [0.05, 0.0, 0.95],
            [0.95, 0.0, 0.05],
            [0.95, 1.0, 0.05],
        ],
        full_uv,
        "the cross's south face",
    );
    assert_quad(
        &baked.quads[2],
        [
            [0.05, 1.0, 0.05],
            [0.05, 0.0, 0.05],
            [0.95, 0.0, 0.95],
            [0.95, 1.0, 0.95],
        ],
        full_uv,
        "the cross's west face",
    );
    assert_quad(
        &baked.quads[3],
        [
            [0.95, 1.0, 0.95],
            [0.95, 0.0, 0.95],
            [0.05, 0.0, 0.05],
            [0.05, 1.0, 0.05],
        ],
        full_uv,
        "the cross's east face",
    );
    assert!(
        baked.quads.iter().all(|quad| !quad.shade),
        "cross.json's elements state shade false"
    );
}

#[test]
fn the_stairs_model_bakes_both_elements_with_their_own_uvs() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let stairs = source.blockstates("oak_stairs").expect("oak_stairs parses");
    let baked = source
        .bake_variant(&stairs.variants[""][0])
        .expect("the stairs bake");

    assert_eq!(baked.quads.len(), 11, "six faces then five");
    assert_eq!(baked.particle.as_deref(), Some("blocks/stairs_side"));

    let down = &baked.quads[0];
    assert_quad(
        down,
        [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
        ],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        "the lower element's down face",
    );
    assert_eq!(down.cullface, Some(FaceDir::Down));
    assert_eq!(down.texture, "blocks/stairs_bottom");

    // The second element's west face: uv [0, 0, 16, 8] over the 8..16 x 8..16
    // box, so half the sprite in v.
    let west = &baked.quads[9];
    assert_quad(
        west,
        [
            [0.5, 1.0, 0.0],
            [0.5, 0.5, 0.0],
            [0.5, 0.5, 1.0],
            [0.5, 1.0, 1.0],
        ],
        [[0.0, 0.0], [0.0, 0.5], [1.0, 0.5], [1.0, 0.0]],
        "the upper element's west face",
    );
    assert_eq!(west.texture, "blocks/stairs_side");
    assert_eq!(west.cullface, None, "the west face states no cullface");

    let textures: BTreeSet<&str> = baked.quads.iter().map(|q| q.texture.as_str()).collect();
    assert_eq!(
        textures,
        BTreeSet::from([
            "blocks/stairs_bottom",
            "blocks/stairs_side",
            "blocks/stairs_top"
        ]),
        "the child's three variables resolve for the parent's faces"
    );
}

#[test]
fn variant_rotation_moves_the_geometry_around_the_block_centre() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let torch = source.blockstates("torch").expect("torch parses");

    // The north face of the wall torch's stick, at each of the report's y
    // rotations. The block-local box rotates about (0.5, 0.5, 0.5).
    let cases: [(u16, [[f32; 3]; 4]); 4] = [
        (
            0,
            [
                [0.5625, 0.8125, 0.0],
                [0.5625, 0.1875, 0.0],
                [0.4375, 0.1875, 0.0],
                [0.4375, 0.8125, 0.0],
            ],
        ),
        (
            90,
            [
                [1.0, 0.8125, 0.5625],
                [1.0, 0.1875, 0.5625],
                [1.0, 0.1875, 0.4375],
                [1.0, 0.8125, 0.4375],
            ],
        ),
        (
            180,
            [
                [0.4375, 0.8125, 1.0],
                [0.4375, 0.1875, 1.0],
                [0.5625, 0.1875, 1.0],
                [0.5625, 0.8125, 1.0],
            ],
        ),
        (
            270,
            [
                [0.0, 0.8125, 0.4375],
                [0.0, 0.1875, 0.4375],
                [0.0, 0.1875, 0.5625],
                [0.0, 0.8125, 0.5625],
            ],
        ),
    ];
    for (y, corners) in cases {
        let variant = Variant {
            model: "normal_torch_wall".to_string(),
            x: 0,
            y,
            uvlock: false,
            weight: 1,
        };
        let baked = source.bake_variant(&variant).expect("the wall torch bakes");
        assert_eq!(baked.quads.len(), 6);
        assert_close(&baked.quads[2].corners, corners, &format!("y={y}"));
        assert_uv(
            &baked.quads[2],
            [
                [7.0 / 16.0, 6.0 / 16.0],
                [7.0 / 16.0, 1.0],
                [9.0 / 16.0, 1.0],
                [9.0 / 16.0, 6.0 / 16.0],
            ],
            &format!("y={y} keeps its uv on the corner it started on"),
        );
    }

    // The ground torch's facing=up entry: no rotation, and its three planes.
    let up = source
        .bake_variant(&torch.variants["facing=up"][0])
        .expect("the ground torch bakes");
    assert_eq!(up.quads.len(), 6, "three planes, two faces each");

    // A variant carrying both rotations: x first, then y.
    let tilted = Variant {
        model: "probe".to_string(),
        x: 180,
        y: 270,
        uvlock: false,
        weight: 1,
    };
    let baked = source
        .bake_variant(&tilted)
        .expect("the tilted probe bakes");
    assert_close(
        &baked.quads[2].corners,
        [
            [1.0, 1.0, 1.0],
            [1.0, 0.5, 1.0],
            [1.0, 0.5, 0.0],
            [1.0, 1.0, 0.0],
        ],
        "x=180,y=270 turns the north face upside down and to the east",
    );
    assert_eq!(
        baked.quads[0].cullface,
        Some(FaceDir::Up),
        "x=180 turns the down face's cullface up"
    );
}

#[test]
fn uvlock_keeps_the_texture_locked_to_the_block_face() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    // The wall torch's north face under y=90: the geometry lies on the east
    // face, so with uvlock the client projects the texture from that face
    // (FaceBakery.lockUv: east takes u from 1-z and v from 1-y), and without
    // it the uv travels with the corner it was baked for.
    let free = Variant {
        model: "normal_torch_wall".to_string(),
        x: 0,
        y: 90,
        uvlock: false,
        weight: 1,
    };
    let locked = Variant {
        uvlock: true,
        ..free.clone()
    };

    let free_quad = &source
        .bake_variant(&free)
        .expect("the free variant bakes")
        .quads[2]
        .clone();
    assert_uv(
        free_quad,
        [
            [7.0 / 16.0, 6.0 / 16.0],
            [7.0 / 16.0, 1.0],
            [9.0 / 16.0, 1.0],
            [9.0 / 16.0, 6.0 / 16.0],
        ],
        "without uvlock the uv rotates with the geometry",
    );

    let locked_model = source
        .bake_variant(&locked)
        .expect("the locked variant bakes");
    let locked_quad = &locked_model.quads[2];
    assert_uv(
        locked_quad,
        [
            [7.0 / 16.0, 3.0 / 16.0],
            [7.0 / 16.0, 13.0 / 16.0],
            [9.0 / 16.0, 13.0 / 16.0],
            [9.0 / 16.0, 3.0 / 16.0],
        ],
        "with uvlock the uv stays on the block face",
    );
    assert_close(
        &locked_quad.corners,
        free_quad.corners,
        "uvlock never moves the geometry",
    );

    // The probe's up face carries a quarter turn of its own; uvlock composes
    // with it (the lock's store slot steps back by the face rotation).
    let probe_locked = Variant {
        model: "probe".to_string(),
        x: 0,
        y: 90,
        uvlock: true,
        weight: 1,
    };
    let baked = source
        .bake_variant(&probe_locked)
        .expect("the locked probe bakes");
    assert_uv(
        &baked.quads[1],
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        "the rotated up face's locked uv",
    );

    // The probe also states the unlocked half of the same rotation, through
    // its own blockstate.
    let probe = source.blockstates("probe").expect("probe parses");
    let baked = source
        .bake_variant(&probe.variants["free"][0])
        .expect("the free probe bakes");
    assert_uv(
        &baked.quads[1],
        [
            [10.0 / 16.0, 11.0 / 16.0],
            [10.0 / 16.0, 3.0 / 16.0],
            [2.0 / 16.0, 3.0 / 16.0],
            [2.0 / 16.0, 11.0 / 16.0],
        ],
        "the same face unlocked rotates its uv with the geometry",
    );
}

#[test]
fn builtin_missing_bakes_to_the_missing_flag_and_no_quads() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let variant = Variant {
        model: "missing_item".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };
    let baked = source
        .bake_variant(&variant)
        .expect("builtin/missing bakes");

    assert!(baked.missing, "builtin/missing is the missing marker");
    assert!(baked.quads.is_empty(), "the marker carries no quads");
    assert!(baked.ambient_occlusion);
    assert_eq!(baked.particle.as_deref(), Some("missingno"));
}

#[test]
fn builtin_generated_bakes_one_flat_quad_per_layer() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let variant = Variant {
        model: "generated_item".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };
    let baked = source
        .bake_variant(&variant)
        .expect("builtin/generated bakes");

    assert_eq!(baked.quads.len(), 5, "layers 0 through 4, one quad each");
    let layers = [
        "items/thing",
        "items/thing_glint",
        "items/thing_c",
        "items/thing_d",
        "items/thing_e",
    ];
    for (index, quad) in baked.quads.iter().enumerate() {
        assert_eq!(quad.texture, layers[index]);
        assert_eq!(
            quad.tintindex,
            Some(index as u8),
            "a layer's tintindex is its own index"
        );
        assert_eq!(quad.cullface, None);
        assert_close(
            &quad.corners,
            [
                [0.0, 1.0, 0.53125],
                [0.0, 0.0, 0.53125],
                [1.0, 0.0, 0.53125],
                [1.0, 1.0, 0.53125],
            ],
            "the flat layer plane sits at z = 8.5/16",
        );
    }
    assert_eq!(baked.particle.as_deref(), Some("items/thing"));
    assert!(!baked.missing);
    assert!(!baked.ambient_occlusion);
}

#[test]
fn other_builtin_parents_end_the_chain_without_quads() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    let entity = Variant {
        model: "entity_item".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };
    let baked = source.bake_variant(&entity).expect("builtin/entity bakes");
    assert!(baked.quads.is_empty());
    assert!(!baked.missing, "only builtin/missing is the missing marker");
    assert_eq!(baked.particle, None);

    let unknown = Variant {
        model: "unknown_builtin".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };
    let error = source
        .bake_variant(&unknown)
        .expect_err("an unknown builtin is an error");
    let message = error.to_string();
    assert!(
        message.contains("builtin/gizmo"),
        "the message names the parent: {message}"
    );
}

#[test]
fn an_unresolvable_parent_is_an_error_naming_the_path() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let variant = Variant {
        model: "orphan".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };

    let error = source
        .bake_variant(&variant)
        .expect_err("a parent that is not in the tree fails the bake");
    assert!(matches!(error, ModelError::MissingModel { .. }));
    let message = error.to_string();
    assert!(
        message.contains("no_such_model.json"),
        "the message names the missing file: {message}"
    );
}

#[test]
fn a_parent_cycle_is_an_error_naming_the_chain() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let variant = Variant {
        model: "cycle_a".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };

    let error = source
        .bake_variant(&variant)
        .expect_err("a cycle fails the bake");
    assert!(matches!(error, ModelError::ParentCycle { .. }));
    let message = error.to_string();
    assert!(
        message.contains("cycle_a") && message.contains("cycle_b"),
        "the message names the chain: {message}"
    );
}

#[test]
fn a_missing_texture_variable_is_an_error_naming_the_variable() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let variant = Variant {
        model: "no_variable".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };

    let error = source
        .bake_variant(&variant)
        .expect_err("an unresolved variable fails the bake");
    assert!(matches!(error, ModelError::TextureVariable { .. }));
    let message = error.to_string();
    assert!(
        message.contains("ghost_texture"),
        "the message names the variable: {message}"
    );
}

#[test]
fn a_blockstate_that_is_not_in_the_tree_is_an_error_naming_the_path() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    let error = source
        .blockstates("not_a_block")
        .expect_err("a blockstate that is not in the tree fails");
    assert!(matches!(error, ModelError::MissingBlockState { .. }));
    let message = error.to_string();
    assert!(
        message.contains("not_a_block.json"),
        "the message names the file: {message}"
    );
}

#[test]
fn hostile_model_values_are_refused() {
    // Every case is a small document the 1.8 formats do not allow. Each must
    // come back as an error, never as a default.
    let cases: [(&str, &str, &str); 9] = [
        (
            "a position past 32",
            r##"{"elements": [{"from": [0, 0, 40], "to": [16, 16, 16],
                "faces": {"up": {"uv": [0,0,16,16], "texture": "#t"}}}]}"##,
            "boundaries",
        ),
        (
            "a position below -16",
            r##"{"elements": [{"from": [0, -17, 0], "to": [16, 16, 16],
                "faces": {"up": {"uv": [0,0,16,16], "texture": "#t"}}}]}"##,
            "boundaries",
        ),
        (
            "an unknown face key",
            r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16],
                "faces": {"middle": {"uv": [0,0,16,16], "texture": "#t"}}}]}"##,
            "middle",
        ),
        (
            "a texture that is not a variable",
            r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16],
                "faces": {"up": {"uv": [0,0,16,16], "texture": "blocks/stone"}}}]}"##,
            "must name a variable",
        ),
        (
            "a face rotation off the quarter turns",
            r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16],
                "faces": {"up": {"uv": [0,0,16,16], "texture": "#t", "rotation": 45}}}]}"##,
            "rotation",
        ),
        (
            "an element axis that is not x, y or z",
            r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16],
                "rotation": {"origin": [8,8,8], "axis": "w", "angle": 45},
                "faces": {"up": {"uv": [0,0,16,16], "texture": "#t"}}}]}"##,
            "w",
        ),
        (
            "an element angle the client does not allow",
            r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16],
                "rotation": {"origin": [8,8,8], "axis": "y", "angle": 30},
                "faces": {"up": {"uv": [0,0,16,16], "texture": "#t"}}}]}"##,
            "30",
        ),
        (
            "an element with no faces",
            r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16], "faces": {}}]}"##,
            "faces",
        ),
        (
            "neither a parent nor elements",
            r##"{"textures": {"t": "blocks/stone"}}"##,
            "parent",
        ),
    ];
    for (label, json, fragment) in cases {
        let error = ModelJson::parse(json)
            .err()
            .unwrap_or_else(|| panic!("{label} must be refused"));
        assert!(
            error.to_string().contains(fragment),
            "{label}: the message states the reason (wanted {fragment:?}): {error}"
        );
    }

    // A model may not carry both a parent and elements.
    let error = ModelJson::parse(
        r##"{"parent": "block/cube", "elements": [{"from": [0,0,0], "to": [16,16,16],
            "faces": {"up": {"uv": [0,0,16,16], "texture": "#t"}}}]}"##,
    )
    .expect_err("a parent and elements together are refused");
    assert!(error.to_string().contains("parent"), "{error}");
}

#[test]
fn a_negative_face_rotation_is_a_named_error() {
    // The rotation is parsed signed so a negative value reaches the quarter-turn check and
    // comes back as the named value error; before the signed parse this same document failed
    // inside serde as `ModelError::Json`.
    let error = ModelJson::parse(
        r##"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16],
            "faces": {"up": {"uv": [0,0,16,16], "texture": "#t", "rotation": -90}}}]}"##,
    )
    .expect_err("a negative face rotation must be refused");
    assert!(
        matches!(error, ModelError::Value { .. }),
        "the refusal is the named value error, not a parse error: {error}"
    );
    let message = error.to_string();
    assert!(
        message.contains("element 0") && message.contains("face `up`") && message.contains("-90"),
        "the message names the element, the face and the value: {message}"
    );
}

#[test]
fn hostile_blockstate_values_are_refused() {
    let cases: [(&str, &str); 5] = [
        (
            "an x that is not a quarter turn",
            r##"{"variants": {"": {"model": "stone", "x": 45}}}"##,
        ),
        (
            "a y that is not a quarter turn",
            r##"{"variants": {"": {"model": "stone", "y": 100}}}"##,
        ),
        ("a model with no name", r##"{"variants": {"": {}}}"##),
        ("no variants at all", r##"{}"##),
        (
            "a weight that cannot be counted",
            r##"{"variants": {"": {"model": "stone", "weight": -1}}}"##,
        ),
    ];
    for (label, json) in cases {
        let error = oxide_assets::model::BlockStates::parse(json)
            .err()
            .unwrap_or_else(|| panic!("{label} must be refused"));
        assert!(!error.to_string().is_empty(), "{label}: {error}");
    }

    // A blockstate document that is not JSON at all.
    let error = oxide_assets::model::BlockStates::parse("{ not json")
        .expect_err("a broken document is refused");
    assert!(matches!(error, ModelError::Json { .. }), "{error}");
}

#[test]
fn an_inverted_element_normalises_to_its_hull() {
    // The real tree carries six fire models whose boxes run backwards on one
    // axis. The client's FaceBakery.applyFacing snaps every quad onto the
    // box's min/max hull (it runs whenever the element has no part rotation),
    // which is why such a box renders at all; a default or an error would
    // differ from the client.
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");
    let variant = Variant {
        model: "inverted".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };
    let baked = source
        .bake_variant(&variant)
        .expect("the inverted box bakes");

    assert_eq!(baked.quads.len(), 1);
    assert_close(
        &baked.quads[0].corners,
        [
            [0.000625, 1.4625, 1.0],
            [0.000625, 1.4625, 0.0],
            [0.999375, 1.4625, 0.0],
            [0.999375, 1.4625, 1.0],
        ],
        "the reversed axis is read as its hull",
    );
}

#[test]
fn texture_paths_covers_the_blockstates_and_the_builtin_locations() {
    let tree = sphere();
    let source = ModelSource::open(tree.root()).expect("the synthetic tree opens");

    let paths = source.texture_paths();
    let expected: BTreeSet<String> = [
        // The synthetic blockstates' own chains.
        "blocks/stone",
        "blocks/torch_on",
        "blocks/grass_top",
        "blocks/grass_side_snowed",
        "blocks/probe_side",
        "blocks/probe_top",
        "blocks/plant",
        "blocks/stairs_bottom",
        "blocks/stairs_top",
        "blocks/stairs_side",
        // The locations the client adds beyond the variant scan
        // (ModelBakery.LOCATIONS_BUILTIN_TEXTURES).
        "blocks/water_flow",
        "blocks/water_still",
        "blocks/lava_flow",
        "blocks/lava_still",
        "blocks/destroy_stage_0",
        "blocks/destroy_stage_1",
        "blocks/destroy_stage_2",
        "blocks/destroy_stage_3",
        "blocks/destroy_stage_4",
        "blocks/destroy_stage_5",
        "blocks/destroy_stage_6",
        "blocks/destroy_stage_7",
        "blocks/destroy_stage_8",
        "blocks/destroy_stage_9",
        "items/empty_armor_slot_helmet",
        "items/empty_armor_slot_chestplate",
        "items/empty_armor_slot_leggings",
        "items/empty_armor_slot_boots",
        // The entity sheets the built-in block models sample (fix3b): the
        // chest and the sign boards mesh through the terrain atlas.
        "entity/chest/normal",
        "entity/chest/normal_double",
        "entity/sign",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(paths, expected);

    // The answer is cached: a second call returns the same set.
    assert_eq!(source.texture_paths(), paths);
}

/// The real extraction tree, once: every blockstate and model file parses,
/// the referenced texture set carries the survey's named paths, and the stone
/// cube bakes to six culled quads.
///
/// Ignored by default because it needs the user's own store: `OXIDECRAFT_STORE`
/// must name the store root (the directory that holds `extracted/`), and the
/// test fails naming the variable when it is unset, so a run without a store
/// can never pass silently. The version under the store is the literal
/// `1.8.9`.
#[test]
#[ignore = "reads the real extraction tree; run it with OXIDECRAFT_STORE set and --ignored"]
fn the_real_extraction_tree_loads() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");
    let blockstates_dir = root.join("assets/minecraft/blockstates");
    let models_dir = root.join("assets/minecraft/models");

    let source = ModelSource::open(&root).expect("the real tree opens");

    // Every blockstate file parses, and its count is the survey's (340).
    let mut blockstates = Vec::new();
    for path in files_under(&blockstates_dir) {
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("the tree's names are UTF-8");
        source
            .blockstates(name)
            .unwrap_or_else(|error| panic!("{name} must parse: {error}"));
        blockstates.push(path);
    }
    assert_eq!(
        blockstates.len(),
        340,
        "the survey's count of blockstate files"
    );
    println!("blockstate files parsed: {}", blockstates.len());

    // Every model file parses, and their count is the survey's (1595 = 1080
    // block + 515 item).
    let mut models = 0;
    for path in files_under(&models_dir) {
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a model file reads");
        ModelJson::parse(&text)
            .unwrap_or_else(|error| panic!("{} must parse: {error}", path.display()));
        models += 1;
    }
    assert_eq!(models, 1595, "the survey's count of model files");
    println!("model files parsed: {models}");

    // The referenced texture set carries the paths the survey names, and every
    // one of them resolves in the store's own texture set.
    let paths = source.texture_paths();
    println!("referenced texture paths: {}", paths.len());
    for wanted in [
        "blocks/stone",
        "blocks/grass_top",
        "blocks/water_still",
        "blocks/lava_still",
        "blocks/leaves_oak",
        "blocks/glass",
    ] {
        assert!(
            paths.contains(wanted),
            "{wanted} must be in the referenced set"
        );
    }
    let textures =
        oxide_assets::resources::TextureSet::load(&root).expect("the real textures load");
    let unresolved: Vec<&String> = paths
        .iter()
        .filter(|path| textures.get(path).is_none())
        .collect();
    assert!(
        unresolved.is_empty(),
        "every referenced path must be a texture the store carries, missing {unresolved:?}"
    );

    // The stone cube: `block/stone` bakes to six quads, all culled.
    let variant = Variant {
        model: "stone".to_string(),
        x: 0,
        y: 0,
        uvlock: false,
        weight: 1,
    };
    let baked = source.bake_variant(&variant).expect("blocks/stone bakes");
    assert_eq!(baked.quads.len(), 6, "the stone cube is six quads");
    println!("stone quads: {}", baked.quads.len());
    assert!(
        baked.quads.iter().all(|quad| quad.cullface.is_some()),
        "all six quads carry a cullface"
    );
    let faces: BTreeSet<Option<FaceDir>> = baked.quads.iter().map(|quad| quad.cullface).collect();
    assert_eq!(
        faces,
        BTreeSet::from([
            Some(FaceDir::Down),
            Some(FaceDir::Up),
            Some(FaceDir::North),
            Some(FaceDir::South),
            Some(FaceDir::West),
            Some(FaceDir::East),
        ]),
        "one cullface per direction"
    );
}

/// The synthetic tree every test above bakes from, written file by file.
fn sphere() -> Tree {
    let tree = Tree::new();
    tree.write("assets/minecraft/blockstates/stone.json", STONE_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/torch.json", TORCH_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/grass.json", GRASS_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/probe.json", PROBE_BLOCKSTATE);
    tree.write("assets/minecraft/blockstates/plant.json", PLANT_BLOCKSTATE);
    tree.write(
        "assets/minecraft/blockstates/oak_stairs.json",
        OAK_STAIRS_BLOCKSTATE,
    );

    tree.write("assets/minecraft/models/block/cube.json", CUBE);
    tree.write("assets/minecraft/models/block/cube_all.json", CUBE_ALL);
    tree.write("assets/minecraft/models/block/stone.json", STONE);
    tree.write(
        "assets/minecraft/models/block/grass_normal.json",
        GRASS_NORMAL,
    );
    tree.write(
        "assets/minecraft/models/block/grass_snowed.json",
        GRASS_SNOWED,
    );
    tree.write("assets/minecraft/models/block/stairs.json", STAIRS);
    tree.write("assets/minecraft/models/block/oak_stairs.json", OAK_STAIRS);
    tree.write("assets/minecraft/models/block/cross.json", CROSS);
    tree.write("assets/minecraft/models/block/plant.json", PLANT);
    tree.write("assets/minecraft/models/block/torch.json", TORCH);
    tree.write(
        "assets/minecraft/models/block/normal_torch.json",
        NORMAL_TORCH,
    );
    tree.write("assets/minecraft/models/block/torch_wall.json", TORCH_WALL);
    tree.write(
        "assets/minecraft/models/block/normal_torch_wall.json",
        NORMAL_TORCH_WALL,
    );
    tree.write("assets/minecraft/models/block/probe.json", PROBE);
    tree.write(
        "assets/minecraft/models/block/generated_item.json",
        GENERATED_ITEM,
    );
    tree.write(
        "assets/minecraft/models/block/missing_item.json",
        MISSING_ITEM,
    );
    tree.write(
        "assets/minecraft/models/block/entity_item.json",
        ENTITY_ITEM,
    );
    tree.write(
        "assets/minecraft/models/block/unknown_builtin.json",
        UNKNOWN_BUILTIN,
    );
    tree.write("assets/minecraft/models/block/orphan.json", ORPHAN);
    tree.write("assets/minecraft/models/block/cycle_a.json", CYCLE_A);
    tree.write("assets/minecraft/models/block/cycle_b.json", CYCLE_B);
    tree.write(
        "assets/minecraft/models/block/no_variable.json",
        NO_VARIABLE,
    );
    tree.write("assets/minecraft/models/block/inverted.json", INVERTED);
    tree
}

/// The `stone` blockstate: one empty-key variant.
const STONE_BLOCKSTATE: &[u8] = br##"{ "variants": { "": { "model": "stone" } } }"##;

/// The `torch` blockstate: the survey's five facing variants.
const TORCH_BLOCKSTATE: &[u8] = br##"{
  "variants": {
    "facing=up":    { "model": "normal_torch" },
    "facing=east":  { "model": "normal_torch_wall" },
    "facing=south": { "model": "normal_torch_wall", "y": 90 },
    "facing=west":  { "model": "normal_torch_wall", "y": 180 },
    "facing=north": { "model": "normal_torch_wall", "y": 270 }
  }
}"##;

/// The `grass` blockstate: the survey's four-element weighted array.
const GRASS_BLOCKSTATE: &[u8] = br##"{
  "variants": {
    "snowy=false": [
      { "model": "grass_normal" },
      { "model": "grass_normal", "y": 90 },
      { "model": "grass_normal", "y": 180 },
      { "model": "grass_normal", "y": 270 }
    ],
    "snowy=true": { "model": "grass_snowed" }
  }
}"##;

/// The probe blockstate: uvlock on and off, both rotations, and a weighted
/// array.
const PROBE_BLOCKSTATE: &[u8] = br##"{
  "variants": {
    "":         { "model": "probe" },
    "locked":   { "model": "probe", "y": 90, "uvlock": true },
    "free":     { "model": "probe", "y": 90 },
    "tilted":   { "model": "probe", "x": 180, "y": 270 },
    "weighted": [
      { "model": "probe", "weight": 2 },
      { "model": "probe", "y": 180, "weight": 1 }
    ]
  }
}"##;

/// A plant block for the cross model.
const PLANT_BLOCKSTATE: &[u8] = br##"{ "variants": { "": { "model": "plant" } } }"##;

/// A stairs block for the two-element model.
const OAK_STAIRS_BLOCKSTATE: &[u8] = br##"{ "variants": { "": { "model": "oak_stairs" } } }"##;

/// Asserts a quad's corners and uvs against literals within a float epsilon.
fn assert_quad(quad: &BakedQuad, corners: [[f32; 3]; 4], uv: [[f32; 2]; 4], label: &str) {
    assert_close(&quad.corners, corners, label);
    assert_uv(quad, uv, label);
}

/// Asserts four corners against literals within a float epsilon.
fn assert_close(corners: &[[f32; 3]; 4], expected: [[f32; 3]; 4], label: &str) {
    for (index, (got, want)) in corners.iter().zip(expected).enumerate() {
        for axis in 0..3 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-5,
                "{label}: corner {index} axis {axis} is {:?}, want {:?}",
                got,
                want
            );
        }
    }
}

/// Asserts four uv pairs against literals within a float epsilon.
fn assert_uv(quad: &BakedQuad, expected: [[f32; 2]; 4], label: &str) {
    for (index, (got, want)) in quad.uv.iter().zip(expected).enumerate() {
        for axis in 0..2 {
            assert!(
                (got[axis] - want[axis]).abs() < 1e-5,
                "{label}: uv {index} axis {axis} is {:?}, want {:?}",
                got,
                want
            );
        }
    }
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

    /// The extraction root to hand [`ModelSource::open`].
    fn root(&self) -> &Path {
        &self.root
    }
}
