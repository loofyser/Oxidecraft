//! The built-in blocks' geometry: the chest and the sign boards.
//!
//! The client builds these in — `BlockModelShapes.registerBuiltInBlocks`
//! names `chest`, `standing_sign` and `wall_sign` among the blocks with no
//! blockstate file — so the model set never resolves them. The mesher's
//! answer used to be the magenta fallback cube (and, for the signs, nothing
//! at all so the text floats). This module synthesizes their quads from the
//! source's block-entity renderers instead, in the closed pose:
//!
//! - the chest (`TileEntityChestRenderer` + `ModelChest`/`ModelLargeChest`)
//!   over `entity/chest/normal` (single) or `entity/chest/normal_double`
//!   (two adjacent chests, drawn once from the cell with no negative
//!   neighbour);
//! - the sign boards (`TileEntitySignRenderer` + `ModelSign`) over
//!   `entity/sign`, the standing board with its post, the wall board alone,
//!   each turned by its metadata.
//!
//! The lid animation is a block-entity carry the port has no stream for, so
//! every chest draws closed (recorded). The Christmas sheets, the trapped
//! chest and the ender chest are out of scope: only the plain chest (id 54)
//! routes here.

use oxide_assets::model::{BakedModel, BakedQuad};

use super::snapshot::ColumnSnapshot;

/// The plain chest's block id (`Block.java`: the chest row).
const CHEST_ID: u16 = 54;
/// The standing sign's block id (`Block.java`:1321).
const STANDING_ID: u16 = 63;
/// The wall sign's block id (`Block.java`:1326).
const WALL_ID: u16 = 68;

/// The single chest's sheet (`TileEntityChestRenderer.java`:19).
const NORMAL_SHEET: &str = "entity/chest/normal";
/// The double chest's sheet (`:16`).
const DOUBLE_SHEET: &str = "entity/chest/normal_double";
/// The sign board's sheet (`TileEntitySignRenderer.java`:17).
const SIGN_SHEET: &str = "entity/sign";

/// What a built-in id meshes as.
#[derive(Debug)]
pub enum Builtin {
    /// A synthesized model: these quads, each over its own sprite.
    Model(BakedModel),
    /// No geometry: the double chest's silent half.
    Empty,
    /// Not a built-in id: the caller falls through to the fallback cube.
    Absent,
}

/// The geometry a built-in id meshes: the chest (single or double) or a
/// sign board, or [`Builtin::Absent`] for every other id.
///
/// `position` is the cell in the snapshot's column frame, for the double
/// chest's neighbour reads; the quads come back in the cell's own frame,
/// like every baked model's.
pub fn builtin(snapshot: &ColumnSnapshot, id: u16, meta: u8, position: (i32, i32, i32)) -> Builtin {
    match id {
        CHEST_ID => chest(snapshot, meta, position),
        STANDING_ID | WALL_ID => sign(id, meta),
        _ => Builtin::Absent,
    }
}

/// One `ModelRenderer` box: the `addBox` origin and size in model units,
/// the texture offset, and the rotation point the part turns about (the
/// closed pose turns nothing, so the point is only an offset).
struct Part {
    /// The box's minimum corner, in model units.
    origin: [f32; 3],
    /// The box's size, in model units.
    size: [f32; 3],
    /// The texture offset (`textureX`, `textureY`).
    tex: [i32; 2],
    /// The part's rotation point, in model units.
    pivot: [f32; 3],
}

/// One box face: the four corner slots and the texture rect as fractions
/// of the sheet.
///
/// The slots index the box's eight corners; the rects are
/// `ModelBox.java:82-87`, corner for corner with the slots, and each
/// vertex takes its corner of the rect the way `TexturedQuad`'s constructor
/// hands them out (vertex 0 the (U2, V1) corner, then (U1, V1), (U1, V2),
/// (U2, V2)).
///
/// The six faces' rects in `ModelBox.java:82-87` order (+x, −x, −y, +y,
/// −z, +z): (U1, V1, U2, V2) in texels of the model's sheet, from the box
/// size and the texture offset.
fn face_rects(size: [f32; 3], tex: [i32; 2]) -> [[f32; 4]; 6] {
    let (w, h, d) = (size[0], size[1], size[2]);
    let (tx, ty) = (tex[0] as f32, tex[1] as f32);
    [
        [tx + d + w, ty + d, tx + d + w + d, ty + d + h],
        [tx, ty + d, tx + d, ty + d + h],
        [tx + d, ty, tx + d + w, ty + d],
        [tx + d + w, ty + d, tx + d + w + w, ty],
        [tx + d, ty + d, tx + d + w, ty + d + h],
        [tx + d + w + d, ty + d, tx + d + w + d + w, ty + d + h],
    ]
}

/// The face's corners, in `ModelBox.java:82-87` order: +x, −x, −y, +y,
/// −z, +z, indexing the eight corners
/// `(x1,y1,z1) (x2,y1,z1) (x2,y2,z1) (x1,y2,z1)`
/// `(x1,y1,z2) (x2,y1,z2) (x2,y2,z2) (x1,y2,z2)`.
const FACE_CORNERS: [[usize; 4]; 6] = [
    [5, 1, 2, 6],
    [0, 4, 7, 3],
    [5, 4, 0, 1],
    [2, 3, 7, 6],
    [1, 0, 3, 2],
    [4, 5, 6, 7],
];

/// The faces' outward normals in model space, in [`FACE_CORNERS`] order.
const FACE_NORMALS: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, -1.0],
    [0.0, 0.0, 1.0],
];

/// The eight corners of a part's box, in model units.
fn box_corners(part: &Part) -> [[f32; 3]; 8] {
    let [x1, y1, z1] = part.origin;
    let [x2, y2, z2] = [
        part.origin[0] + part.size[0],
        part.origin[1] + part.size[1],
        part.origin[2] + part.size[2],
    ];
    [
        [x1, y1, z1],
        [x2, y1, z1],
        [x2, y2, z1],
        [x1, y2, z1],
        [x1, y1, z2],
        [x2, y1, z2],
        [x2, y2, z2],
        [x1, y2, z2],
    ]
}

/// The sine and cosine of a rotation, snapped exact at the quarter turns:
/// every angle here is a multiple of 22.5°, and the float noise on the
/// axes would skew the hulls the tests pin.
fn trig(degrees: f32) -> (f32, f32) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let snap = |value: f32| {
        if value.abs() < 1e-6 {
            0.0
        } else if (value.abs() - 1.0).abs() < 1e-6 {
            value.signum()
        } else {
            value
        }
    };
    (snap(cos), snap(sin))
}

/// A right-handed quarter-friendly turn about the y axis: the source's own
/// `rotateY` shape, as the port's model baker reads it.
fn turn_y(cos: f32, sin: f32, point: [f32; 3]) -> [f32; 3] {
    [
        point[0] * cos + point[2] * sin,
        point[1],
        -point[0] * sin + point[2] * cos,
    ]
}

/// The cross product of two vectors.
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Emits one part's six faces through `place`: each face's four corners in
/// counter-clockwise order seen from outside (the terrain culls back
/// faces), with its sprite-space uvs glued to the corners.
fn emit(
    part: &Part,
    sheet: (u32, u32),
    texture: &str,
    place: &dyn Fn([f32; 3]) -> [f32; 3],
    linear: &dyn Fn([f32; 3]) -> [f32; 3],
    quads: &mut Vec<BakedQuad>,
) {
    let corners = box_corners(part);
    let rects = face_rects(part.size, part.tex);
    for (face, slots) in FACE_CORNERS.iter().enumerate() {
        let rect = rects[face];
        let (u1, v1, u2, v2) = (rect[0], rect[1], rect[2], rect[3]);
        let sheet_uv = [(u2, v1), (u1, v1), (u1, v2), (u2, v2)];
        let mut placed = [[0.0f32; 3]; 4];
        let mut uv = [[0.0f32; 2]; 4];
        for (index, slot) in slots.iter().enumerate() {
            let corner = corners[*slot];
            let model = [
                (corner[0] + part.pivot[0]) * 0.0625,
                (corner[1] + part.pivot[1]) * 0.0625,
                (corner[2] + part.pivot[2]) * 0.0625,
            ];
            placed[index] = place(model);
            let (u, v) = sheet_uv[index];
            uv[index] = [u / sheet.0 as f32, v / sheet.1 as f32];
        }
        // The outward side after the chain: the model-space normal through
        // its linear half. A face wound against it is turned around.
        let outward = linear(FACE_NORMALS[face]);
        let edge_a = [
            placed[1][0] - placed[0][0],
            placed[1][1] - placed[0][1],
            placed[1][2] - placed[0][2],
        ];
        let edge_b = [
            placed[2][0] - placed[1][0],
            placed[2][1] - placed[1][1],
            placed[2][2] - placed[1][2],
        ];
        let normal = cross(edge_a, edge_b);
        let facing = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];
        if facing < 0.0 {
            placed.swap(1, 3);
            uv.swap(1, 3);
        }
        quads.push(BakedQuad {
            corners: placed,
            uv,
            texture: texture.to_string(),
            cullface: None,
            tintindex: None,
            shade: true,
        });
    }
}

/// The chest's yaw in degrees: the metadata's facing
/// (`TileEntityChestRenderer.java:129-147`), anything else at 0°.
fn chest_yaw(meta: u8) -> f32 {
    match meta {
        2 => 180.0,
        4 => 90.0,
        5 => -90.0,
        _ => 0.0,
    }
}

/// The chest at a cell: the closed model, single or double.
///
/// A cell whose west or north neighbour is a chest meshes nothing — the
/// source renders the large model once, from the cell with no negative
/// neighbour (`TileEntityChestRenderer.java:59-60`) — and a cell with an
/// east or south chest neighbour takes the double sheet and shape
/// (`ModelLargeChest`), shifted for the north/east facings (`:149-157`).
/// Only the plain chest pairs here: the trapped and ender chests are other
/// ids, and the source pairs equal chest types only
/// (`TileEntityChest.isChestAt`, `:311-322`).
fn chest(snapshot: &ColumnSnapshot, meta: u8, position: (i32, i32, i32)) -> Builtin {
    let (x, y, z) = position;
    let neighbour = |dx: i32, dz: i32| snapshot.block(x + dx, y, z + dz) >> 4 == CHEST_ID;
    if neighbour(-1, 0) || neighbour(0, -1) {
        return Builtin::Empty;
    }
    let double = neighbour(1, 0) || neighbour(0, 1);
    let (width, sheet, texture, knob_x) = if double {
        (30.0f32, (128u32, 64u32), DOUBLE_SHEET, 16.0f32)
    } else {
        (14.0, (64, 64), NORMAL_SHEET, 8.0)
    };
    // `ModelChest.java:14-30` / `ModelLargeChest.java:5-28`, closed: the
    // lid and the knob ride at angle 0, so every part is axis-aligned.
    let parts = [
        Part {
            origin: [0.0, -5.0, -14.0],
            size: [width, 5.0, 14.0],
            tex: [0, 0],
            pivot: [1.0, 7.0, 15.0],
        },
        Part {
            origin: [-1.0, -2.0, -15.0],
            size: [2.0, 4.0, 1.0],
            tex: [0, 0],
            pivot: [knob_x, 7.0, 15.0],
        },
        Part {
            origin: [0.0, 0.0, 0.0],
            size: [width, 10.0, 14.0],
            tex: [0, 19],
            pivot: [1.0, 6.0, 1.0],
        },
    ];
    // `TileEntityChestRenderer.java:124-160`, in the cell's own frame: the
    // world chain translates by the cell plus (0, 1, 1), so the cell-local
    // corner recentres, shifts the double chest's halves into place, turns
    // the facing, and lands at (x, 1 - y, 1 - z).
    let (cos, sin) = trig(chest_yaw(meta));
    let shift = if meta == 2 && neighbour(1, 0) {
        [1.0f32, 0.0, 0.0]
    } else if meta == 5 && neighbour(0, 1) {
        [0.0, 0.0, -1.0]
    } else {
        [0.0, 0.0, 0.0]
    };
    let place = |model: [f32; 3]| {
        let centred = [
            model[0] - 0.5 + shift[0],
            model[1] - 0.5 + shift[1],
            model[2] - 0.5 + shift[2],
        ];
        let turned = turn_y(cos, sin, centred);
        let lifted = [turned[0] + 0.5, turned[1] + 0.5, turned[2] + 0.5];
        [lifted[0], 1.0 - lifted[1], 1.0 - lifted[2]]
    };
    let linear = |normal: [f32; 3]| {
        let turned = turn_y(cos, sin, normal);
        [turned[0], -turned[1], -turned[2]]
    };
    let mut quads = Vec::with_capacity(18);
    for part in &parts {
        emit(part, sheet, texture, &place, &linear, &mut quads);
    }
    Builtin::Model(BakedModel {
        quads,
        ambient_occlusion: true,
        particle: None,
        missing: false,
    })
}

/// A sign's board at a cell: the standing board with its post, the wall
/// board alone, turned by the metadata.
///
/// The chain is the renderer's (`TileEntitySignRenderer.java:28-59,
/// :75-79`): stand the board at `y + 0.75 * f`, turn the negative yaw, set
/// the wall board back, and scale `(f, -f, -f)` — the same numbers the
/// sign-text pass lays its glyphs with. The stick draws for the standing
/// sign only (`:33` vs `:58`).
fn sign(id: u16, meta: u8) -> Builtin {
    // `ModelSign.java:11-17`: the board and the post, texture 64x32 (the
    // model base's own default).
    let board = Part {
        origin: [-12.0, -14.0, -1.0],
        size: [24.0, 12.0, 2.0],
        tex: [0, 0],
        pivot: [0.0, 0.0, 0.0],
    };
    let stick = Part {
        origin: [-1.0, -2.0, -1.0],
        size: [2.0, 14.0, 2.0],
        tex: [0, 14],
        pivot: [0.0, 0.0, 0.0],
    };
    let yaw = if id == STANDING_ID {
        f32::from(meta) * 360.0 / 16.0
    } else {
        match meta {
            2 => 180.0,
            4 => 90.0,
            5 => -90.0,
            _ => 0.0,
        }
    };
    let (cos, sin) = trig(-yaw);
    let wall = id != STANDING_ID;
    let place = |model: [f32; 3]| {
        let f = 0.6666667f32;
        let scaled = [model[0] * f, -model[1] * f, -model[2] * f];
        let set = if wall {
            [scaled[0], scaled[1] - 0.3125, scaled[2] - 0.4375]
        } else {
            scaled
        };
        let turned = turn_y(cos, sin, set);
        [turned[0] + 0.5, turned[1] + 0.75 * f, turned[2] + 0.5]
    };
    let linear = |normal: [f32; 3]| {
        let scaled = [normal[0], -normal[1], -normal[2]];
        turn_y(cos, sin, scaled)
    };
    let mut quads = Vec::with_capacity(12);
    emit(&board, (64, 32), SIGN_SHEET, &place, &linear, &mut quads);
    if !wall {
        emit(&stick, (64, 32), SIGN_SHEET, &place, &linear, &mut quads);
    }
    Builtin::Model(BakedModel {
        quads,
        ambient_occlusion: true,
        particle: None,
        missing: false,
    })
}
