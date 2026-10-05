//! The object set's geometry tables: the painting arts, the arrow, the boat and minecart
//! models, the XP orb's sheet cells and the generated item shape.
//!
//! Every value here is a transcription of one class in the source tree, and every one of
//! them is pinned by the tests below. The tables that depend on a sprite's pixels (the
//! generated item shape) take the pixels as an argument: the sheet lives in the atlas,
//! which this crate reaches only through the client's own source, so the shape is a pure
//! function of the sprite's bytes.
//!
//! Angles are degrees in the draw code and radians in the tables, as the source writes
//! them; distances are in 1/16 model units unless a comment says blocks.

use super::{Box, Model, Part, Vertices};

/// One painting art of `EntityPainting.EnumArt`, in the table's own order — the ordinal
/// is the index the art table is keyed by (`EntityPainting.java`:144-169).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Art {
    /// The art's title, as the wire's spawn packet carries it.
    pub title: &'static str,
    /// The art's size in pixels; 16 pixels make one block.
    pub size: [u16; 2],
    /// The art's top-left corner on the sheet, in pixels.
    pub offset: [u16; 2],
}

/// The 26 arts, in ordinal order (`EntityPainting.EnumArt`'s declaration order;
/// `EntityPainting.java`:144-169).
pub static ART: [Art; 26] = [
    Art {
        title: "Kebab",
        size: [16, 16],
        offset: [0, 0],
    },
    Art {
        title: "Aztec",
        size: [16, 16],
        offset: [16, 0],
    },
    Art {
        title: "Alban",
        size: [16, 16],
        offset: [32, 0],
    },
    Art {
        title: "Aztec2",
        size: [16, 16],
        offset: [48, 0],
    },
    Art {
        title: "Bomb",
        size: [16, 16],
        offset: [64, 0],
    },
    Art {
        title: "Plant",
        size: [16, 16],
        offset: [80, 0],
    },
    Art {
        title: "Wasteland",
        size: [16, 16],
        offset: [96, 0],
    },
    Art {
        title: "Pool",
        size: [32, 16],
        offset: [0, 32],
    },
    Art {
        title: "Courbet",
        size: [32, 16],
        offset: [32, 32],
    },
    Art {
        title: "Sea",
        size: [32, 16],
        offset: [64, 32],
    },
    Art {
        title: "Sunset",
        size: [32, 16],
        offset: [96, 32],
    },
    Art {
        title: "Creebet",
        size: [32, 16],
        offset: [128, 32],
    },
    Art {
        title: "Wanderer",
        size: [16, 32],
        offset: [0, 64],
    },
    Art {
        title: "Graham",
        size: [16, 32],
        offset: [16, 64],
    },
    Art {
        title: "Match",
        size: [32, 32],
        offset: [0, 128],
    },
    Art {
        title: "Bust",
        size: [32, 32],
        offset: [32, 128],
    },
    Art {
        title: "Stage",
        size: [32, 32],
        offset: [64, 128],
    },
    Art {
        title: "Void",
        size: [32, 32],
        offset: [96, 128],
    },
    Art {
        title: "SkullAndRoses",
        size: [32, 32],
        offset: [128, 128],
    },
    Art {
        title: "Wither",
        size: [32, 32],
        offset: [160, 128],
    },
    Art {
        title: "Fighters",
        size: [64, 32],
        offset: [0, 96],
    },
    Art {
        title: "Pointer",
        size: [64, 64],
        offset: [0, 192],
    },
    Art {
        title: "Pigscene",
        size: [64, 64],
        offset: [64, 192],
    },
    Art {
        title: "BurningSkull",
        size: [64, 64],
        offset: [128, 192],
    },
    Art {
        title: "Skeleton",
        size: [64, 48],
        offset: [192, 64],
    },
    Art {
        title: "DonkeyKong",
        size: [64, 48],
        offset: [192, 112],
    },
];

/// The fallback art's index (`Kebab`, the table's first entry).
pub const KEBAB_FALLBACK: usize = 0;

/// The art an index names, clamped: an index off the table draws the first art rather
/// than panicking.
pub fn art(index: u8) -> &'static Art {
    ART.get(usize::from(index)).unwrap_or(&ART[0])
}

/// The index of an art title, or the fallback when no title matches.
///
/// The source scans for `title.equals` and keeps the constructor's random pick when
/// nothing matches; the fallback here is the name the wire's unknown cases read, `Kebab`
/// (`EntityPainting.java`:48-62, :88-91).
pub fn art_index(title: &str) -> usize {
    ART.iter()
        .position(|art| art.title == title)
        .unwrap_or(KEBAB_FALLBACK)
}

/// The art a title names, falling back to `Kebab`.
pub fn art_for_title(title: &str) -> &'static Art {
    &ART[art_index(title)]
}

/// The painting sheet's texture key (`RenderPainting.java`:16).
pub const PAINTING_TEXTURE: &str = "painting/paintings_kristoffer_zetterstrand.png";

/// The painting sheet's side, in pixels (`RenderPainting.java`:77-80's `/ 256`).
pub const PAINTING_SHEET: f32 = 256.0;

/// The painting's yaw for a wire facing byte: `rotationYaw = facingIndex * 90`
/// (`EntityHanging.updateFacingWithBoundingBox`:46; the wire byte is
/// `EnumFacing.getHorizontal`'s index — `S10PacketSpawnPainting.java`:33-39 reads it
/// through `EnumFacing.getHorizontal(byte & 3)`).
pub fn painting_yaw(facing: u8) -> f32 {
    f32::from(facing & 3) * 90.0
}

/// The item frame's yaw for a wire facing byte: the same fold (`EntityItemFrame` shares
/// `EntityHanging`'s placement).
pub fn frame_yaw(facing: u8) -> f32 {
    painting_yaw(facing)
}

/// The content's rotation within a frame, in degrees: `rotation * 45` over the eight
/// states (`RenderItemFrame.java`:103-110).
pub fn frame_rotation_degrees(rotation: u8) -> f32 {
    (rotation % 8) as f32 * 45.0
}

/// The painting's drawn position from the spawn packet's hanging position: the source's
/// `updateBoundingBox` centre shifts (`EntityHanging.java`:53-88).
///
/// The wire position is the hanging block's corner; the entity's own position — the one
/// the renderer translates by — is the corner plus the centre, minus the facing's own
/// `0.46875` front offset, plus half a block along `rotateYCCW` when the art's width is a
/// multiple of 32 pixels, and up half a block when its height is.
pub fn painting_position(block: [f64; 3], facing: u8, art: &Art) -> [f32; 3] {
    // `EnumFacing.getHorizontal`'s S-W-N-E order (`EnumFacing.java`:14-17): the front
    // offsets and the rotateYCCW step the source's own `EnumFacing` names.
    let (front, ccw) = match facing & 0x03 {
        0 => ([0.0_f64, 1.0], [1.0, 0.0]), // SOUTH: front +Z, rotateYCCW EAST (+X)
        1 => ([-1.0, 0.0], [0.0, 1.0]),    // WEST: front -X, rotateYCCW SOUTH (+Z)
        2 => ([0.0, -1.0], [-1.0, 0.0]),   // NORTH: front -Z, rotateYCCW WEST (-X)
        _ => ([1.0, 0.0], [0.0, -1.0]),    // EAST: front +X, rotateYCCW NORTH (-Z)
    };
    let width_half = if art.size[0] % 32 == 0 { 0.5 } else { 0.0 };
    let height_half = if art.size[1] % 32 == 0 { 0.5 } else { 0.0 };
    let x = block[0] + 0.5 - front[0] * 0.46875 + width_half * ccw[0];
    let z = block[2] + 0.5 - front[1] * 0.46875 + width_half * ccw[1];
    let y = block[1] + 0.5 + height_half;
    [x as f32, y as f32, z as f32]
}

/// The painting's vertex set for one art, in 1/16 model units: six quads per 16-pixel
/// cell of the art (`RenderPainting.renderPainting`:50-111).
///
/// The cell loops run x-major, then y; per cell the art face, the back panel and the four
/// frame strips follow in the source's vertex order with its mirrored uvs.
pub fn painting_vertices(art: &Art) -> Vertices {
    let mut out = Vertices::default();
    let width = f32::from(art.size[0]);
    let height = f32::from(art.size[1]);
    let u0 = f32::from(art.offset[0]);
    let v0 = f32::from(art.offset[1]);
    let half = 0.5_f32;
    let face = 0.75_f32;
    let face_edge = 0.8125_f32;
    let back = 0.0_f32;
    let back_edge = 0.0625_f32;
    let side = 0.001_953_125_f32;
    let x0 = -width / 2.0;
    let y0 = -height / 2.0;
    let mut cell_x = 0.0_f32;
    while cell_x < width {
        let mut cell_y = 0.0_f32;
        while cell_y < height {
            let x1 = x0 + cell_x + 16.0;
            let x2 = x0 + cell_x;
            let y1 = y0 + cell_y + 16.0;
            let y2 = y0 + cell_y;
            // The art face's uvs mirror per cell: `f19`/`f20` walk left from the art's
            // right edge (`RenderPainting.java`:77-80).
            let u_left = (u0 + width - cell_x) / PAINTING_SHEET;
            let u_right = (u0 + width - cell_x - 16.0) / PAINTING_SHEET;
            let v_low = (v0 + height - cell_y) / PAINTING_SHEET;
            let v_high = (v0 + height - cell_y - 16.0) / PAINTING_SHEET;
            let face_quad = [
                ([x1, y2, -half], [u_right, v_low]),
                ([x2, y2, -half], [u_left, v_low]),
                ([x2, y1, -half], [u_left, v_high]),
                ([x1, y1, -half], [u_right, v_high]),
            ];
            push_quad(&mut out, &face_quad, [0.0, 0.0, -1.0]);
            let back_quad = [
                ([x1, y1, half], [face, back]),
                ([x2, y1, half], [face_edge, back]),
                ([x2, y2, half], [face_edge, back_edge]),
                ([x1, y2, half], [face, back_edge]),
            ];
            push_quad(&mut out, &back_quad, [0.0, 0.0, 1.0]);
            let top = [
                ([x1, y1, -half], [face, side]),
                ([x2, y1, -half], [face_edge, side]),
                ([x2, y1, half], [face_edge, back_edge]),
                ([x1, y1, half], [face, back_edge]),
            ];
            push_quad(&mut out, &top, [0.0, 1.0, 0.0]);
            let bottom = [
                ([x1, y2, half], [face, side]),
                ([x2, y2, half], [face_edge, side]),
                ([x2, y2, -half], [face_edge, back_edge]),
                ([x1, y2, -half], [face, back_edge]),
            ];
            push_quad(&mut out, &bottom, [0.0, -1.0, 0.0]);
            let left = [
                ([x2, y1, half], [side_far(), 0.0]),
                ([x2, y2, half], [side_far(), 0.0625]),
                ([x2, y2, -half], [side_far(), 0.0625]),
                ([x2, y1, -half], [side_far(), 0.0]),
            ];
            push_quad(&mut out, &left, [-1.0, 0.0, 0.0]);
            let right = [
                ([x1, y1, -half], [side_far(), 0.0]),
                ([x1, y2, -half], [side_far(), 0.0625]),
                ([x1, y2, half], [side_far(), 0.0625]),
                ([x1, y1, half], [side_far(), 0.0]),
            ];
            push_quad(&mut out, &right, [1.0, 0.0, 0.0]);
            cell_y += 16.0;
        }
        cell_x += 16.0;
    }
    out
}

/// The frame strip's far uv: `f11`/`f12 = 0.7519531` (`RenderPainting.java`:63-64).
fn side_far() -> f32 {
    0.751_953_1
}

/// Pushes one quad's four corners, uvs and a constant normal; the consumer triangulates.
fn push_quad(out: &mut Vertices, corners: &[([f32; 3], [f32; 2]); 4], normal: [f32; 3]) {
    for index in [0, 1, 2, 3] {
        out.positions.push(corners[index].0);
        out.uvs.push(corners[index].1);
        out.normals.push(normal);
    }
}

/// The arrow's texture key (`RenderArrow.java`:14).
pub const ARROW_TEXTURE: &str = "entity/arrow.png";

/// The arrow's scale: `f8 = 0.05625` (`RenderArrow.java`:43).
pub const ARROW_SCALE: f32 = 0.05625;

/// The arrow's vertex set: the tail's two quads, then the four shaft quads, in the
/// arrow's own pre-scale space (`RenderArrow.java`:56-81).
///
/// The four shaft quads are drawn under a cumulative `rotate(90, x)` each; each is
/// carried into its own frame here, so the whole set bakes as one mesh.
pub fn arrow_vertices() -> Vertices {
    let mut out = Vertices::default();
    let u_mid = 0.156_25_f32;
    let v_tex = 5.0 / 32.0;
    let v_tip = 10.0 / 32.0;
    let tail_a = [
        ([-7.0, -2.0, -2.0], [0.0, v_tex]),
        ([-7.0, -2.0, 2.0], [u_mid, v_tex]),
        ([-7.0, 2.0, 2.0], [u_mid, v_tip]),
        ([-7.0, 2.0, -2.0], [0.0, v_tip]),
    ];
    push_quad(&mut out, &tail_a, [1.0, 0.0, 0.0]);
    let tail_b = [
        ([-7.0, 2.0, -2.0], [0.0, v_tex]),
        ([-7.0, 2.0, 2.0], [u_mid, v_tex]),
        ([-7.0, -2.0, 2.0], [u_mid, v_tip]),
        ([-7.0, -2.0, -2.0], [0.0, v_tip]),
    ];
    push_quad(&mut out, &tail_b, [-1.0, 0.0, 0.0]);
    for step in 1..=4 {
        let angle = (90.0 * step as f32).to_radians();
        let (sin, cos) = angle.sin_cos();
        let turn = move |corner: [f32; 3]| -> [f32; 3] {
            [
                corner[0],
                corner[1] * cos - corner[2] * sin,
                corner[1] * sin + corner[2] * cos,
            ]
        };
        let normal = turn([0.0, 0.0, 1.0]);
        let shaft = [
            (turn([-8.0, -2.0, 0.0]), [0.0, 0.0]),
            (turn([8.0, -2.0, 0.0]), [0.5, 0.0]),
            (turn([8.0, 2.0, 0.0]), [0.5, v_tex]),
            (turn([-8.0, 2.0, 0.0]), [0.0, v_tex]),
        ];
        push_quad(&mut out, &shaft, normal);
    }
    out
}

/// The boat's texture key (`RenderBoat.java`:12).
pub const BOAT_TEXTURE: &str = "entity/boat.png";

/// The minecart's texture key (`RenderMinecart.java`:16).
pub const MINECART_TEXTURE: &str = "entity/minecart.png";

/// The item entity's and the orb's shadow, `(size, opacity)`: the flat pair
/// (`RenderEntityItem.java`:24-25, `RenderXPOrb.java`:19-20).
pub const ITEM_SHADOW: (f32, f32) = (0.15, 0.75);

/// An empty model: what the object models answer for the mob table's part lookups, which
/// their own geometry paths never reach.
pub static EMPTY: Model = Model { parts: &[] };

/// The boat's and the minecart's shadow, `(size, opacity)`
/// (`RenderBoat.java`:20, `RenderMinecart.java`:24).
pub const VEHICLE_SHADOW: (f32, f32) = (0.5, 1.0);

/// A part with one box, the shape every boat and minecart part has: `part!(point,
/// rest, origin, size, uv)`. A macro rather than a function, so each part stays a
/// constant expression inside the static tables.
macro_rules! part {
    ($point:expr, $rest:expr, $origin:expr, $size:expr, $uv:expr $(,)?) => {
        Part {
            point: $point,
            rest: $rest,
            boxes: &[Box {
                origin: $origin,
                size: $size,
                uv: $uv,
                inflate: 0.0,
                mirror: false,
            }],
            children: &[],
        }
    };
}

/// `ModelBoat`'s five parts (`ModelBoat.java`:20-34), a 64x64 sheet.
pub static MODEL_BOAT: Model = Model {
    parts: &[
        part!(
            [0.0, 4.0, 0.0],
            [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
            [-12.0, -8.0, -3.0],
            [24.0, 16.0, 4.0],
            [0.0, 8.0],
        ),
        part!(
            [-11.0, 4.0, 0.0],
            [0.0, 3.0 * std::f32::consts::FRAC_PI_2, 0.0],
            [-10.0, -7.0, -1.0],
            [20.0, 6.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [11.0, 4.0, 0.0],
            [0.0, std::f32::consts::FRAC_PI_2, 0.0],
            [-10.0, -7.0, -1.0],
            [20.0, 6.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [0.0, 4.0, -9.0],
            [0.0, std::f32::consts::PI, 0.0],
            [-10.0, -7.0, -1.0],
            [20.0, 6.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [0.0, 4.0, 9.0],
            [0.0, 0.0, 0.0],
            [-10.0, -7.0, -1.0],
            [20.0, 6.0, 2.0],
            [0.0, 0.0],
        ),
    ],
};

/// `ModelMinecart`'s six drawn parts (`ModelMinecart.java`:22-38), a 64x64 sheet; the
/// table's seventh entry is the source's own unused null.
pub static MODEL_MINECART: Model = Model {
    parts: &[
        part!(
            [0.0, 4.0, 0.0],
            [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
            [-10.0, -8.0, -1.0],
            [20.0, 16.0, 2.0],
            [0.0, 10.0],
        ),
        part!(
            [-9.0, 4.0, 0.0],
            [0.0, 3.0 * std::f32::consts::FRAC_PI_2, 0.0],
            [-8.0, -7.0, -1.0],
            [16.0, 8.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [9.0, 4.0, 0.0],
            [0.0, std::f32::consts::FRAC_PI_2, 0.0],
            [-8.0, -7.0, -1.0],
            [16.0, 8.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [0.0, 4.0, -7.0],
            [0.0, std::f32::consts::PI, 0.0],
            [-8.0, -7.0, -1.0],
            [16.0, 8.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [0.0, 4.0, 7.0],
            [0.0, 0.0, 0.0],
            [-8.0, -7.0, -1.0],
            [16.0, 8.0, 2.0],
            [0.0, 0.0],
        ),
        part!(
            [0.0, 4.1, 0.0],
            [-std::f32::consts::FRAC_PI_2, 0.0, 0.0],
            [-9.0, -7.0, -1.0],
            [18.0, 14.0, 1.0],
            [44.0, 10.0],
        ),
    ],
};

/// The cart body a wire sub-type names: the byte `0..4` the store's kinds carry, in the
/// order `Plain`, `Chest`, `Furnace`, `Tnt`, `Hopper`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinecartBody {
    /// The bare cart, no cargo (`EntityMinecart.getDefaultDisplayTile`:1050-1053).
    Plain,
    /// A chest cargo, facing north, offset 8 (`EntityMinecartChest.java`:50-58).
    Chest,
    /// A furnace cargo, facing north, offset 6 (`EntityMinecartFurnace`:199-202).
    Furnace,
    /// A TNT cargo, offset 6 (`EntityMinecartTNT.java`:35-38).
    Tnt,
    /// A hopper cargo, offset 1 (`EntityMinecartHopper.java`:41-49).
    Hopper,
}

/// The body a stored byte names; out-of-range bytes read as plain rather than panic.
pub fn minecart_body(byte: u8) -> MinecartBody {
    match byte {
        1 => MinecartBody::Chest,
        2 => MinecartBody::Furnace,
        3 => MinecartBody::Tnt,
        4 => MinecartBody::Hopper,
        _ => MinecartBody::Plain,
    }
}

/// One cart's default cargo: the block state and the display-tile offset the class's
/// defaults name (`EntityMinecart.getDefaultDisplayTile`:1050-1053 and its overrides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cargo {
    /// The block id.
    pub block: u16,
    /// The metadata: `north`, the facing each class's default state holds, folded through
    /// the client's own state table.
    pub meta: u8,
    /// The display tile's offset; the tile rides `(offset - 8) / 16` blocks above the
    /// cart's base (`RenderMinecart.java`:54-64).
    pub offset: i32,
}

/// The cargo a body draws, when it has one.
pub fn minecart_cargo(body: MinecartBody) -> Option<Cargo> {
    match body {
        MinecartBody::Plain => None,
        MinecartBody::Chest => Some(Cargo {
            block: 54,
            meta: 2,
            offset: 8,
        }),
        MinecartBody::Furnace => Some(Cargo {
            block: 61,
            meta: 2,
            offset: 6,
        }),
        MinecartBody::Tnt => Some(Cargo {
            block: 46,
            meta: 0,
            offset: 6,
        }),
        MinecartBody::Hopper => Some(Cargo {
            block: 154,
            meta: 0,
            offset: 1,
        }),
    }
}

/// The cart's id jitter, in blocks (`RenderMinecart.java`:34-39).
pub fn minecart_jitter(id: i32) -> [f32; 3] {
    let mut i = i64::from(id).wrapping_mul(493_286_711);
    i = i
        .wrapping_mul(i)
        .wrapping_mul(4_392_167_121)
        .wrapping_add(i.wrapping_mul(98_761));
    let mut out = [0.0_f32; 3];
    for (axis, shift) in [16_u32, 20, 24].iter().enumerate() {
        let bits = (i >> shift) & 7;
        out[axis] = ((bits as f32 + 0.5) / 8.0 - 0.5) * 0.004;
    }
    out
}

/// The orb's texture key (`RenderXPOrb.java`:14).
pub const ORB_TEXTURE: &str = "entity/experience_orb.png";

/// The orb's billboard scale (`RenderXPOrb.java`:51-52).
pub const ORB_SCALE: f32 = 0.3;

/// The orb's icon index for an XP value (`EntityXPOrb.getTextureByXP`:259-262).
pub fn orb_icon(value: i16) -> u8 {
    let value = i32::from(value);
    if value >= 2477 {
        10
    } else if value >= 1237 {
        9
    } else if value >= 617 {
        8
    } else if value >= 307 {
        7
    } else if value >= 149 {
        6
    } else if value >= 73 {
        5
    } else if value >= 37 {
        4
    } else if value >= 17 {
        3
    } else if value >= 7 {
        2
    } else if value >= 3 {
        1
    } else {
        0
    }
}

/// The orb's uvs for an icon index: the 16-pixel cell's corners in the 64x64 sheet, in
/// the source's corner order (`RenderXPOrb.java`:32-35, :56-59).
pub fn orb_uv(index: u8) -> [[f32; 2]; 4] {
    let index = u32::from(index) % 11;
    let u0 = (index % 4 * 16) as f32 / 64.0;
    let u1 = (index % 4 * 16 + 16) as f32 / 64.0;
    let v0 = (index / 4 * 16) as f32 / 64.0;
    let v1 = (index / 4 * 16 + 16) as f32 / 64.0;
    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
}

/// The orb's quad corners (`RenderXPOrb.java`:56-59; the same unit quad the fireball
/// draws).
pub fn orb_corners() -> [[f32; 3]; 4] {
    [
        [-0.5, -0.25, 0.0],
        [0.5, -0.25, 0.0],
        [0.5, 0.75, 0.0],
        [-0.5, 0.75, 0.0],
    ]
}

/// The orb's pulse colour for `(xpColor + partialTicks) / 2` (`RenderXPOrb.java`:45-48),
/// in the 0..1 units the vertex colour takes; the alpha is the source's `128 / 255`.
pub fn orb_colour(pulse: f32) -> [f32; 4] {
    let red = (pulse.sin() + 1.0) * 0.5;
    let blue = (pulse + 4.188_790_3).sin().mul_add(0.1, 0.1);
    [red, 1.0, blue, 128.0 / 255.0]
}

/// The item entity's bob offset, in blocks: `sin((age + partial) / 10 + hoverStart) * 0.1
/// + 0.1` (`RenderEntityItem.java`:41).
pub fn item_bob(age: f32, hover_start: f32) -> f32 {
    (age / 10.0 + hover_start).sin().mul_add(0.1, 0.1)
}

/// The item entity's spin, in degrees: `((age + partial) / 20 + hoverStart) * (180 / pi)`
/// (`RenderEntityItem.java`:47).
pub fn item_spin_degrees(age: f32, hover_start: f32) -> f32 {
    (age / 20.0 + hover_start) * (180.0 / std::f32::consts::PI)
}

/// The copy count a stack size draws (`RenderEntityItem.func_177078_a`:64-86).
pub fn item_copies(count: u8) -> u8 {
    if count > 48 {
        5
    } else if count > 32 {
        4
    } else if count > 16 {
        3
    } else if count > 1 {
        2
    } else {
        1
    }
}

/// The item drops' vertical lift: the source adds `0.25 * ground.scale.y`
/// (`RenderEntityItem.java`:42-43), and an item's ground transform is the identity — only
/// a model with an explicit `ground` display block would move it.
pub const ITEM_GROUND_LIFT: f32 = 0.25;

/// The item copy stack's step along z, in blocks (`RenderEntityItem.java`:138-139).
pub const ITEM_COPY_STEP: f32 = 0.046875;

/// The pre-step the flat copies' stack takes back to sit centred
/// (`RenderEntityItem.java`:56-59`): `-0.046875 * (copies - 1) * 0.5`.
pub fn item_copy_centre(copies: u8) -> f32 {
    -ITEM_COPY_STEP * (f32::from(copies) - 1.0) * 0.5
}

/// The two billboard shapes the projectile classes draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Billboard {
    /// `RenderSnowball`'s: the generated item, rotated `(-playerViewY, +playerViewX)`
    /// inside the class's `0.5` scale (`RenderSnowball.java`:28-35`).
    Snowball,
    /// `RenderFireball`'s: the particle sprite's quad, rotated
    /// `(180 - playerViewY, -playerViewX)` inside the class's own scale
    /// (`RenderFireball.java`:33-51`).
    Fireball,
}

/// The billboard's turn pair in degrees for the camera's angles
/// (`RenderSnowball.java`:31-32, `RenderFireball.java`:44-45; `playerViewY` is the
/// camera's yaw plus 180, `playerViewX` its pitch — `RenderManager.java`:260-266).
pub fn billboard_angles(view_y: f32, view_x: f32, kind: Billboard) -> [f32; 2] {
    match kind {
        Billboard::Snowball => [-view_y, view_x],
        Billboard::Fireball => [180.0 - view_y, -view_x],
    }
}

/// The pre-transform the item draw path applies: the flat (generated) models double, the
/// 3D block models not (`RenderItem.preTransform`:245-259, reached from
/// `renderItemModelTransform`:316-320 for every transform type).
pub fn item_pretransform(gui3d: bool) -> f32 {
    if gui3d { 1.0 } else { 2.0 }
}

/// The scale the item draw path itself applies (`RenderItem.renderItem`:140-145).
pub const ITEM_RENDER_SCALE: f32 = 0.5;

/// The centring translate the item draw path applies before the mesh
/// (`RenderItem.renderItem`:157).
pub const ITEM_CENTRE: [f32; 3] = [-0.5, -0.5, -0.5];

/// The dropped item's own loop scale: only the 3D models take it
/// (`RenderEntityItem.java`:113-129).
pub fn dropped_loop_scale(gui3d: bool) -> f32 {
    if gui3d { ITEM_RENDER_SCALE } else { 1.0 }
}

/// The dropped item draw's net scale: the loop's own scale (3D only) times the render
/// path's chain — the pre-transform's flat doubling and `renderItem`'s `0.5`
/// (`RenderEntityItem.java`:113-129` then `RenderItem.renderItem`:140-157`).
pub fn dropped_item_scale(gui3d: bool) -> f32 {
    dropped_loop_scale(gui3d) * item_pretransform(gui3d) * ITEM_RENDER_SCALE
}

/// The projectile's net scale: the caller's own scale times the pre-transform times the
/// render path's (`RenderSnowball.java`:29` then `RenderItem.renderItemModelTransform`).
pub fn projectile_item_scale(caller: f32, gui3d: bool) -> f32 {
    caller * item_pretransform(gui3d) * ITEM_RENDER_SCALE
}

/// The generated item shape of a sprite: the item model generator's body box and its
/// sprite-derived edge spans (`ItemModelGenerator.java`:17-234).
///
/// `rgba` is the sprite's own pixels, `width * height * 4` bytes, row-major from the top
/// left, in the sheet's own orientation. The returned quads are in 1/16 model units with
/// uvs in the sprite's 0..1 space, ready for the client's atlas mapping; an empty or
/// undersized image answers `None`.
pub fn generated_item(width: u32, height: u32, rgba: &[u8]) -> Option<Vertices> {
    if width == 0 || height == 0 {
        return None;
    }
    let pixels = (width as usize).checked_mul(height as usize)?;
    if rgba.len() < pixels.checked_mul(4)? {
        return None;
    }
    let w = width as i32;
    let h = height as i32;
    let mut out = Vertices::default();
    // The body: the generator's own element, drawn north then south.
    push_baked_quad(
        &mut out,
        2,
        [0.0, 0.0, 7.5],
        [16.0, 16.0, 8.5],
        [16.0, 0.0, 0.0, 16.0],
    );
    push_baked_quad(
        &mut out,
        3,
        [0.0, 0.0, 7.5],
        [16.0, 16.0, 8.5],
        [0.0, 0.0, 16.0, 16.0],
    );
    // The alpha scan: row-major, the four span facings per opaque pixel
    // (`ItemModelGenerator.func_178393_a`:169-193, `func_178396_a`:195-203).
    // A neighbour outside the sprite counts as opaque, the source's own bounds chain
    // (`func_178391_a`:236-239): only a real zero-alpha pixel is transparent.
    let transparent = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= w || y >= h {
            return false;
        }
        rgba[((y * w + x) * 4 + 3) as usize] == 0
    };
    let mut spans: Vec<(u8, i32, i32, i32)> = Vec::new();
    for row in 0..h {
        for col in 0..w {
            let opaque = !transparent(col, row);
            if !opaque {
                continue;
            }
            for (facing, dx, dy) in [(0_u8, 0, -1), (1, 0, 1), (2, -1, 0), (3, 1, 0)] {
                if transparent(col + dx, row + dy) {
                    add_span(&mut spans, facing, col, row);
                }
            }
        }
    }
    for &(facing, b, c, d) in &spans {
        let (from, to, uv) = span_box(facing, b, c, d, width as f32, height as f32);
        let face = match facing {
            0 => 1, // UP
            1 => 0, // DOWN
            2 => 5, // LEFT: the source pairs it with EAST
            _ => 4, // RIGHT: the source pairs it with WEST
        };
        push_baked_quad(&mut out, face, from, to, uv);
    }
    Some(out)
}

/// Merges one span into the list, the source's `func_178395_a`: keyed by the facing and
/// the fixed coordinate, extended along the free one (`ItemModelGenerator.java`:205-234).
fn add_span(spans: &mut Vec<(u8, i32, i32, i32)>, facing: u8, x: i32, y: i32) {
    // The up/down facings key on the row, the left/right facings on the column
    // (`ItemModelGenerator.func_178369_d`:322-325).
    let keyed_by_row = facing <= 1;
    let key = if keyed_by_row { y } else { x };
    let value = if keyed_by_row { x } else { y };
    for span in spans.iter_mut() {
        if span.0 == facing && span.3 == key {
            if value < span.1 {
                span.1 = value;
            } else if value > span.2 {
                span.2 = value;
            }
            return;
        }
    }
    spans.push((facing, value, value, key));
}

/// One span's element box and uv rect, the source's `func_178397_a` math
/// (`ItemModelGenerator.java`:59-167`): the box's thin axis is the free coordinate's
/// edge, and the uv rect runs the span's length with the source's stretched `16 / (size
/// - 1)` scale on the thin axis.
fn span_box(facing: u8, b: i32, c: i32, d: i32, w: f32, h: f32) -> ([f32; 3], [f32; 3], [f32; 4]) {
    let (b, c, d) = (b as f32, c as f32, d as f32);
    // Each facing picks the box's thin axis, the free pair and the uv rect the source's
    // `func_178397_a` switch computes, before the shared scales below.
    let (mut f2, mut f3, mut f4, mut f5, mut f6, mut f7, mut f8, mut f9, f10, f11) = match facing {
        0 => (
            // UP: the strip at the texel row's top edge.
            b,
            d,
            c + 1.0,
            d,
            b,
            c + 1.0,
            d,
            d,
            16.0 / w,
            16.0 / (h - 1.0).max(1.0),
        ),
        1 => (
            // DOWN: the bottom edge.
            b,
            d + 1.0,
            c + 1.0,
            d + 1.0,
            b,
            c + 1.0,
            d,
            d,
            16.0 / w,
            16.0 / (h - 1.0).max(1.0),
        ),
        2 => (
            // LEFT: the strip at the texel column's edge, paired with EAST.
            d,
            b,
            d,
            c + 1.0,
            d,
            d,
            c + 1.0,
            b,
            16.0 / (w - 1.0).max(1.0),
            16.0 / h,
        ),
        _ => (
            // RIGHT: the other edge, paired with WEST.
            d + 1.0,
            b,
            d + 1.0,
            c + 1.0,
            d,
            d,
            c + 1.0,
            b,
            16.0 / (w - 1.0).max(1.0),
            16.0 / h,
        ),
    };
    let f15 = 16.0 / w;
    let f16 = 16.0 / h;
    f2 *= f15;
    f4 *= f15;
    f3 *= f16;
    f5 *= f16;
    f3 = 16.0 - f3;
    f5 = 16.0 - f5;
    f6 *= f10;
    f7 *= f10;
    f8 *= f11;
    f9 *= f11;
    let (from, to) = match facing {
        0 => ([f2, f3, 7.5], [f4, f3, 8.5]),
        1 => ([f2, f5, 7.5], [f4, f5, 8.5]),
        2 => ([f2, f3, 7.5], [f2, f5, 8.5]),
        _ => ([f4, f3, 7.5], [f4, f5, 8.5]),
    };
    (from, to, [f6, f8, f7, f9])
}

/// The client's `EnumFaceDirection` vertex order, in slots
/// `[from.y, to.y, from.z, to.z, from.x, to.x]` — the same table the model baker uses.
const FACE_VERTICES: [[[u8; 3]; 4]; 6] = [
    [[4, 0, 3], [4, 0, 2], [5, 0, 2], [5, 0, 3]],
    [[4, 1, 2], [4, 1, 3], [5, 1, 3], [5, 1, 2]],
    [[5, 1, 2], [5, 0, 2], [4, 0, 2], [4, 1, 2]],
    [[4, 1, 3], [4, 0, 3], [5, 0, 3], [5, 1, 3]],
    [[4, 1, 2], [4, 0, 2], [4, 0, 3], [4, 1, 3]],
    [[5, 1, 3], [5, 0, 3], [5, 0, 2], [5, 1, 2]],
];

/// The face order the table above is indexed by, the client's `EnumFacing.values()`:
/// down, up, north, south, west, east.
fn face_vector(face: usize) -> [f32; 3] {
    match face {
        0 => [0.0, -1.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        2 => [0.0, 0.0, -1.0],
        3 => [0.0, 0.0, 1.0],
        4 => [-1.0, 0.0, 0.0],
        _ => [1.0, 0.0, 0.0],
    }
}

/// Pushes one face of a `from`/`to` box with an explicit uv rect, in 1/16 units and the
/// sprite's 0..1 uv space — the client's baker rules for a face without its own rotation.
fn push_baked_quad(out: &mut Vertices, face: usize, from: [f32; 3], to: [f32; 3], uv: [f32; 4]) {
    let slots = [from[1], to[1], from[2], to[2], from[0], to[0]];
    for (vertex, indices) in FACE_VERTICES[face].iter().enumerate() {
        out.positions.push([
            slots[indices[0] as usize],
            slots[indices[1] as usize],
            slots[indices[2] as usize],
        ]);
        let step = vertex % 4;
        let u = if step == 2 || step == 3 { uv[2] } else { uv[0] };
        let v = if step == 1 || step == 2 { uv[3] } else { uv[1] };
        out.uvs.push([u / 16.0, v / 16.0]);
        out.normals.push(face_vector(face));
    }
}

/// The fallback block cube the mesher's own missing path draws: a unit cube of the
/// missing sprite, six faces with the whole sprite on each.
pub fn missing_block() -> Vertices {
    let mut out = Vertices::default();
    for face in 0..6 {
        push_baked_quad(&mut out, face, [0.0; 3], [16.0; 3], [0.0, 0.0, 16.0, 16.0]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every art's title, size and offset, in ordinal order
    /// (`EntityPainting.java`:144-169).
    #[test]
    fn the_art_table_is_the_sources_26_entries() {
        let expected: [(&str, [u16; 2], [u16; 2]); 26] = [
            ("Kebab", [16, 16], [0, 0]),
            ("Aztec", [16, 16], [16, 0]),
            ("Alban", [16, 16], [32, 0]),
            ("Aztec2", [16, 16], [48, 0]),
            ("Bomb", [16, 16], [64, 0]),
            ("Plant", [16, 16], [80, 0]),
            ("Wasteland", [16, 16], [96, 0]),
            ("Pool", [32, 16], [0, 32]),
            ("Courbet", [32, 16], [32, 32]),
            ("Sea", [32, 16], [64, 32]),
            ("Sunset", [32, 16], [96, 32]),
            ("Creebet", [32, 16], [128, 32]),
            ("Wanderer", [16, 32], [0, 64]),
            ("Graham", [16, 32], [16, 64]),
            ("Match", [32, 32], [0, 128]),
            ("Bust", [32, 32], [32, 128]),
            ("Stage", [32, 32], [64, 128]),
            ("Void", [32, 32], [96, 128]),
            ("SkullAndRoses", [32, 32], [128, 128]),
            ("Wither", [32, 32], [160, 128]),
            ("Fighters", [64, 32], [0, 96]),
            ("Pointer", [64, 64], [0, 192]),
            ("Pigscene", [64, 64], [64, 192]),
            ("BurningSkull", [64, 64], [128, 192]),
            ("Skeleton", [64, 48], [192, 64]),
            ("DonkeyKong", [64, 48], [192, 112]),
        ];
        for (index, (title, size, offset)) in expected.iter().enumerate() {
            let art = &ART[index];
            assert_eq!(art.title, *title, "art {index}'s title");
            assert_eq!(art.size, *size, "art {index}'s size");
            assert_eq!(art.offset, *offset, "art {index}'s offset");
        }
        assert_eq!(ART.len(), 26);
    }

    /// The title lookup: the exact match, and the unknown title's `Kebab` fallback.
    #[test]
    fn an_unknown_title_falls_back_to_kebab() {
        assert_eq!(art_index("Kebab"), 0);
        assert_eq!(art_index("DonkeyKong"), 25);
        assert_eq!(art_index("Wither"), 19);
        assert_eq!(art_index("kebab"), KEBAB_FALLBACK);
        assert_eq!(art_index(""), KEBAB_FALLBACK);
        assert_eq!(art_for_title("SkullAndRoses").offset, [128, 128]);
    }

    /// An index off the table reads the first art rather than panicking.
    #[test]
    fn an_off_table_index_reads_the_first_art() {
        assert_eq!(art(0).title, "Kebab");
        assert_eq!(art(25).title, "DonkeyKong");
        assert_eq!(art(200).title, "Kebab");
    }

    /// The hanging position: `Pool` (32x16) on a north wall at (4, 60, 7).
    ///
    /// North's front offset is `(0, -1)`, so the centre shifts 0.46875 back along +Z;
    /// the width is two whole blocks, so the rotateYCCW half lands on X (west); the
    /// height is one block, so no vertical half.
    #[test]
    fn a_painting_hangs_off_its_cell_centre() {
        let pool = art_for_title("Pool");
        let position = painting_position([4.0, 60.0, 7.0], 2, pool);
        assert_eq!(position, [4.0, 60.5, 7.968_75]);
        // A one-block art on the east wall: the front offset alone moves it.
        let kebab = art_for_title("Kebab");
        let position = painting_position([4.0, 60.0, 7.0], 3, kebab);
        assert_eq!(position, [4.031_25, 60.5, 7.5]);
        // A two-block-tall art lifts half a block: `Wanderer` (16x32).
        let wanderer = art_for_title("Wanderer");
        let position = painting_position([4.0, 60.0, 7.0], 2, wanderer);
        assert_eq!(position, [4.5, 61.0, 7.968_75]);
    }

    /// A 16x16 art draws six quads; `Pool` (two cells) twelve; `Pointer` (16 cells)
    /// ninety-six (`RenderPainting.renderPainting`:66-68's cell loops).
    #[test]
    fn a_painting_draws_six_quads_per_cell() {
        assert_eq!(
            painting_vertices(art_for_title("Kebab")).positions.len(),
            24
        );
        assert_eq!(painting_vertices(art_for_title("Pool")).positions.len(), 48);
        assert_eq!(
            painting_vertices(art_for_title("Pointer")).positions.len(),
            384
        );
    }

    /// The art face's first cell for `Kebab`: the face quad at z = -0.5 with its corners
    /// and mirrored uvs exactly as `renderPainting` writes them
    /// (`RenderPainting.java`:77-87).
    #[test]
    fn the_art_face_carries_the_sources_mirrored_uvs() {
        let kebab = art_for_title("Kebab");
        let vertices = painting_vertices(kebab);
        assert_eq!(vertices.positions[0], [8.0, -8.0, -0.5]);
        assert_eq!(vertices.positions[1], [-8.0, -8.0, -0.5]);
        assert_eq!(vertices.positions[2], [-8.0, 8.0, -0.5]);
        assert_eq!(vertices.positions[3], [8.0, 8.0, -0.5]);
        // f19 = (0 + 16 - 0) / 256 = 1/16, f20 = (0 + 16 - 16) / 256 = 0, both v's the
        // same pair.
        assert_eq!(vertices.uvs[0], [0.0, 1.0 / 16.0]);
        assert_eq!(vertices.uvs[1], [1.0 / 16.0, 1.0 / 16.0]);
        assert_eq!(vertices.uvs[2], [1.0 / 16.0, 0.0]);
        assert_eq!(vertices.uvs[3], [0.0, 0.0]);
        assert_eq!(vertices.normals[0], [0.0, 0.0, -1.0]);
        // The back panel follows, then the four frame strips.
        assert_eq!(vertices.normals[4], [0.0, 0.0, 1.0]);
        assert_eq!(vertices.positions[4], [8.0, 8.0, 0.5]);
        assert_eq!(vertices.positions[8], [8.0, 8.0, -0.5]);
        assert_eq!(vertices.normals[8], [0.0, 1.0, 0.0]);
        assert_eq!(vertices.normals[20], [1.0, 0.0, 0.0]);
    }

    /// The arrow: six quads, the tail's two at x = -7 and the shaft's four, each shaft
    /// quad carried into its own rotated frame (`RenderArrow.java`:56-81).
    #[test]
    fn the_arrow_is_six_quads_in_the_sources_order() {
        let vertices = arrow_vertices();
        assert_eq!(vertices.positions.len(), 24);
        assert_eq!(vertices.positions[0], [-7.0, -2.0, -2.0]);
        assert_eq!(vertices.positions[1], [-7.0, -2.0, 2.0]);
        assert_eq!(vertices.positions[2], [-7.0, 2.0, 2.0]);
        assert_eq!(vertices.positions[3], [-7.0, 2.0, -2.0]);
        let v6 = 5.0 / 32.0;
        let v7 = 10.0 / 32.0;
        assert_eq!(vertices.uvs[0], [0.0, v6]);
        assert_eq!(vertices.uvs[1], [0.156_25, v6]);
        assert_eq!(vertices.uvs[2], [0.156_25, v7]);
        assert_eq!(vertices.uvs[3], [0.0, v7]);
        assert_eq!(vertices.normals[0], [1.0, 0.0, 0.0]);
        // The tail quad B, reversed winding.
        assert_eq!(vertices.positions[4], [-7.0, 2.0, -2.0]);
        assert_eq!(vertices.positions[5], [-7.0, 2.0, 2.0]);
        assert_eq!(vertices.positions[6], [-7.0, -2.0, 2.0]);
        assert_eq!(vertices.positions[7], [-7.0, -2.0, -2.0]);
        assert_eq!(vertices.normals[4], [-1.0, 0.0, 0.0]);
        // The first shaft quad, in the frame its own rotate(90, x) leaves it: the
        // (-8, -2, 0) corner turns to (-8, 0, -2), up to the trigonometry's own
        // last-bit error.
        let near_zero = |value: f32| value.abs() < 1.0e-5;
        assert_eq!(vertices.positions[8][0], -8.0);
        assert!(near_zero(vertices.positions[8][1]));
        assert_eq!(vertices.positions[8][2], -2.0);
        assert_eq!(vertices.positions[9][0], 8.0);
        assert!(near_zero(vertices.positions[9][1]));
        assert_eq!(vertices.positions[9][2], -2.0);
        assert_eq!(vertices.positions[10][0], 8.0);
        assert!(near_zero(vertices.positions[10][1]));
        assert_eq!(vertices.positions[10][2], 2.0);
        assert_eq!(vertices.positions[11][0], -8.0);
        assert!(near_zero(vertices.positions[11][1]));
        assert_eq!(vertices.positions[11][2], 2.0);
        assert_eq!(vertices.uvs[8], [0.0, 0.0]);
        assert_eq!(vertices.uvs[9], [0.5, 0.0]);
        assert_eq!(vertices.uvs[10], [0.5, v6]);
        assert_eq!(vertices.uvs[11], [0.0, v6]);
        assert_eq!(vertices.normals[8][0], 0.0);
        assert_eq!(vertices.normals[8][1], -1.0);
        assert!(near_zero(vertices.normals[8][2]));
        // The later quads land in their own frames: the third (270 degrees) in the
        // opposite plane's sign, the fourth (360 degrees) back in the identity frame,
        // both up to the trigonometry's last-bit error.
        assert!(vertices.positions[16][1].abs() < 1.0e-5);
        assert!((vertices.positions[16][2] - 2.0).abs() < 1.0e-5);
        assert_eq!(vertices.positions[20][0], -8.0);
        assert_eq!(vertices.positions[20][1], -2.0);
        assert!(near_zero(vertices.positions[20][2]));
        assert_eq!(vertices.positions[21][0], 8.0);
        assert_eq!(vertices.positions[21][1], -2.0);
        assert!(near_zero(vertices.positions[21][2]));
        assert_eq!(vertices.positions[22][0], 8.0);
        assert_eq!(vertices.positions[22][1], 2.0);
        assert!(near_zero(vertices.positions[22][2]));
        assert_eq!(vertices.positions[23][0], -8.0);
        assert_eq!(vertices.positions[23][1], 2.0);
        assert!(near_zero(vertices.positions[23][2]));
        assert!(near_zero(vertices.normals[20][0]));
        assert!(near_zero(vertices.normals[20][1]));
        assert_eq!(vertices.normals[20][2], 1.0);
    }

    /// The boat's five parts and the minecart's six, with the source's pivots, boxes and
    /// rest rotations (`ModelBoat.java`:20-34, `ModelMinecart.java`:22-38).
    #[test]
    fn the_boat_and_cart_models_are_the_sources_parts() {
        let boat = &MODEL_BOAT;
        assert_eq!(boat.parts.len(), 5);
        assert_eq!(boat.parts[0].point, [0.0, 4.0, 0.0]);
        assert_eq!(boat.parts[0].rest, [std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
        assert_eq!(boat.parts[0].boxes.len(), 1);
        assert_eq!(boat.parts[0].boxes[0].origin, [-12.0, -8.0, -3.0]);
        assert_eq!(boat.parts[0].boxes[0].size, [24.0, 16.0, 4.0]);
        assert_eq!(boat.parts[0].boxes[0].uv, [0.0, 8.0]);
        assert_eq!(boat.parts[1].point, [-11.0, 4.0, 0.0]);
        assert_eq!(
            boat.parts[1].rest,
            [0.0, 3.0 * std::f32::consts::FRAC_PI_2, 0.0]
        );
        assert_eq!(boat.parts[1].boxes[0].origin, [-10.0, -7.0, -1.0]);
        assert_eq!(boat.parts[1].boxes[0].size, [20.0, 6.0, 2.0]);
        assert_eq!(boat.parts[2].point, [11.0, 4.0, 0.0]);
        assert_eq!(boat.parts[3].point, [0.0, 4.0, -9.0]);
        assert_eq!(boat.parts[4].point, [0.0, 4.0, 9.0]);
        assert_eq!(boat.parts[4].rest, [0.0, 0.0, 0.0]);

        let cart = &MODEL_MINECART;
        assert_eq!(cart.parts.len(), 6);
        assert_eq!(cart.parts[0].boxes[0].origin, [-10.0, -8.0, -1.0]);
        assert_eq!(cart.parts[0].boxes[0].size, [20.0, 16.0, 2.0]);
        assert_eq!(cart.parts[0].boxes[0].uv, [0.0, 10.0]);
        assert_eq!(cart.parts[0].rest, [std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
        assert_eq!(cart.parts[1].point, [-9.0, 4.0, 0.0]);
        assert_eq!(
            cart.parts[1].rest,
            [0.0, 3.0 * std::f32::consts::FRAC_PI_2, 0.0]
        );
        assert_eq!(cart.parts[1].boxes[0].origin, [-8.0, -7.0, -1.0]);
        assert_eq!(cart.parts[1].boxes[0].size, [16.0, 8.0, 2.0]);
        assert_eq!(cart.parts[5].boxes[0].uv, [44.0, 10.0]);
        assert_eq!(cart.parts[5].boxes[0].origin, [-9.0, -7.0, -1.0]);
        assert_eq!(cart.parts[5].boxes[0].size, [18.0, 14.0, 1.0]);
        assert_eq!(cart.parts[5].point, [0.0, 4.1, 0.0]);
        assert_eq!(cart.parts[5].rest, [-std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
    }

    /// The four cargo bodies and their offsets, and the bare cart's none
    /// (`EntityMinecartChest.java`:50-58, `EntityMinecartHopper.java`:41-49,
    /// `EntityMinecartFurnace`:199-202, `EntityMinecartTNT.java`:35-38).
    #[test]
    fn the_cart_cargoes_are_the_sources_default_tiles() {
        assert_eq!(minecart_cargo(MinecartBody::Plain), None);
        assert_eq!(
            minecart_cargo(MinecartBody::Chest),
            Some(Cargo {
                block: 54,
                meta: 2,
                offset: 8
            })
        );
        assert_eq!(
            minecart_cargo(MinecartBody::Furnace),
            Some(Cargo {
                block: 61,
                meta: 2,
                offset: 6
            })
        );
        assert_eq!(
            minecart_cargo(MinecartBody::Tnt),
            Some(Cargo {
                block: 46,
                meta: 0,
                offset: 6
            })
        );
        assert_eq!(
            minecart_cargo(MinecartBody::Hopper),
            Some(Cargo {
                block: 154,
                meta: 0,
                offset: 1
            })
        );
        assert_eq!(minecart_body(0), MinecartBody::Plain);
        assert_eq!(minecart_body(1), MinecartBody::Chest);
        assert_eq!(minecart_body(4), MinecartBody::Hopper);
        assert_eq!(minecart_body(5), MinecartBody::Plain);
        assert_eq!(minecart_body(255), MinecartBody::Plain);
    }

    /// The id jitter is exact integer math (`RenderMinecart.java`:34-39): id zero pins
    /// the offset's home, id one the three seven-bit windows.
    #[test]
    fn the_cart_jitter_is_the_sources_integer_math() {
        assert_eq!(minecart_jitter(0), [-0.001_75; 3]);
        let i = 1_i64.wrapping_mul(493_286_711);
        let i = i
            .wrapping_mul(i)
            .wrapping_mul(4_392_167_121)
            .wrapping_add(i.wrapping_mul(98_761));
        let jitter = minecart_jitter(1);
        for (axis, shift) in [16_u32, 20, 24].iter().enumerate() {
            let expected = ((((i >> shift) & 7) as f32 + 0.5) / 8.0 - 0.5) * 0.004;
            assert_eq!(jitter[axis], expected, "axis {axis}");
        }
    }

    /// The orb's icon table, both ends and one boundary each way
    /// (`EntityXPOrb.getTextureByXP`:259-262).
    #[test]
    fn the_orb_icon_table_is_the_sources_thresholds() {
        assert_eq!(orb_icon(0), 0);
        assert_eq!(orb_icon(2), 0);
        assert_eq!(orb_icon(3), 1);
        assert_eq!(orb_icon(6), 1);
        assert_eq!(orb_icon(7), 2);
        assert_eq!(orb_icon(16), 2);
        assert_eq!(orb_icon(17), 3);
        assert_eq!(orb_icon(36), 3);
        assert_eq!(orb_icon(37), 4);
        assert_eq!(orb_icon(72), 4);
        assert_eq!(orb_icon(73), 5);
        assert_eq!(orb_icon(148), 5);
        assert_eq!(orb_icon(149), 6);
        assert_eq!(orb_icon(306), 6);
        assert_eq!(orb_icon(307), 7);
        assert_eq!(orb_icon(616), 7);
        assert_eq!(orb_icon(617), 8);
        assert_eq!(orb_icon(1236), 8);
        assert_eq!(orb_icon(1237), 9);
        assert_eq!(orb_icon(2476), 9);
        assert_eq!(orb_icon(2477), 10);
        assert_eq!(orb_icon(i16::MAX), 10);
    }

    /// The orb's cell uvs: index 0's top-left cell, index 5's and index 10's
    /// (`RenderXPOrb.java`:32-35, :56-59).
    #[test]
    fn the_orb_uvs_are_sixteenth_cells_of_the_sheet() {
        assert_eq!(
            orb_uv(0),
            [[0.0, 0.25], [0.25, 0.25], [0.25, 0.0], [0.0, 0.0]]
        );
        assert_eq!(
            orb_uv(5),
            [[0.25, 0.5], [0.5, 0.5], [0.5, 0.25], [0.25, 0.25]]
        );
        assert_eq!(
            orb_uv(10),
            [[0.5, 0.75], [0.75, 0.75], [0.75, 0.5], [0.5, 0.5]]
        );
        // An index past the table wraps rather than panicking.
        assert_eq!(orb_uv(11), orb_uv(0));
    }

    /// The orb's quad: the source's corners (`RenderXPOrb.java`:56-59).
    #[test]
    fn the_orb_quad_is_the_sources_box() {
        let corners = orb_corners();
        assert_eq!(corners[0], [-0.5, -0.25, 0.0]);
        assert_eq!(corners[1], [0.5, -0.25, 0.0]);
        assert_eq!(corners[2], [0.5, 0.75, 0.0]);
        assert_eq!(corners[3], [-0.5, 0.75, 0.0]);
    }

    /// The pulse at two phases (`RenderXPOrb.java`:45-48): the sine pair at zero, the
    /// green channel pinned at one throughout.
    #[test]
    fn the_orb_colour_is_the_sources_pulse() {
        let colour = orb_colour(0.0);
        assert!((colour[0] - 0.5).abs() < 1.0e-6);
        assert_eq!(colour[1], 1.0);
        assert!((colour[2] - (4.188_790_3_f32.sin() + 1.0) * 0.1).abs() < 1.0e-6);
        assert!((colour[3] - 128.0 / 255.0).abs() < 1.0e-7);
        let colour = orb_colour(std::f32::consts::FRAC_PI_2);
        assert!((colour[0] - 1.0).abs() < 1.0e-6);
        let expected = (std::f32::consts::FRAC_PI_2 + 4.188_790_3_f32)
            .sin()
            .mul_add(0.1, 0.1);
        assert!((colour[2] - expected).abs() < 1.0e-6);
    }

    /// The item bob and spin at pinned ages (`RenderEntityItem.java`:41, :47).
    #[test]
    fn the_item_bob_and_spin_are_the_sources_formulas() {
        assert!((item_bob(0.0, 0.0) - 0.1).abs() < 1.0e-6);
        assert!(item_spin_degrees(0.0, 0.0).abs() < 1.0e-6);
        let spin = item_spin_degrees(20.0, 0.0);
        assert!((spin - 180.0 / std::f32::consts::PI).abs() < 1.0e-5);
        let rate = item_spin_degrees(1.0, 0.0) - item_spin_degrees(0.0, 0.0);
        let per_tick = 180.0 / std::f32::consts::PI / 20.0;
        assert!(
            (rate - per_tick).abs() < 1.0e-5,
            "rate {rate} against {per_tick}"
        );
        // A full turn takes 20 * pi ticks.
        let full = item_spin_degrees(20.0 * std::f32::consts::PI, 0.0);
        assert!((full - 180.0).abs() < 1.0e-3, "one revolution by {full}");
        // The bob's phase: a half period on, the sine's trough (0.1 again).
        let bob = item_bob(10.0 * std::f32::consts::PI, 0.0);
        assert!((bob - 0.1).abs() < 1.0e-4);
        // hoverStart shifts both: a quarter period of the bob's own 10-tick divisor.
        let shifted = item_bob(0.0, std::f32::consts::FRAC_PI_2);
        assert!((shifted - 0.2).abs() < 1.0e-6);
    }

    /// The dropped item's ground lift: the source adds `0.25 * ground.scale.y`
    /// (`RenderEntityItem.java`:42-43), and an item's ground transform is the identity —
    /// only a model with an explicit `ground` display block would move it.
    #[test]
    fn the_item_ground_lift_is_the_sources_quarter_block() {
        assert_eq!(ITEM_GROUND_LIFT, 0.25);
    }

    /// The copy count table (`RenderEntityItem.func_177078_a`:64-86).
    #[test]
    fn the_item_copy_count_is_the_sources_thresholds() {
        assert_eq!(item_copies(1), 1);
        assert_eq!(item_copies(2), 2);
        assert_eq!(item_copies(16), 2);
        assert_eq!(item_copies(17), 3);
        assert_eq!(item_copies(32), 3);
        assert_eq!(item_copies(33), 4);
        assert_eq!(item_copies(48), 4);
        assert_eq!(item_copies(49), 5);
        assert_eq!(item_copies(64), 5);
    }

    /// The billboard turns: the snowball family's negated yaw and positive pitch; the
    /// fireballs' `180 - yaw` with the negated pitch (`RenderSnowball.java`:31-32,
    /// `RenderFireball.java`:44-45`). `playerViewY` is the camera's yaw plus 180.
    #[test]
    fn the_billboards_turn_to_the_camera_by_the_sources_signs() {
        let view_y = 90.0 + 180.0;
        let view_x = 15.0;
        assert_eq!(
            billboard_angles(view_y, view_x, Billboard::Snowball),
            [-270.0, 15.0]
        );
        assert_eq!(
            billboard_angles(view_y, view_x, Billboard::Fireball),
            [-90.0, -15.0]
        );
    }

    /// The item draws' net scale: the dropped item path takes the render path's `0.5`
    /// with the pre-transform's flat doubling, and the loop's own `0.5` on top for the
    /// 3D models; the projectile path the pre-transform as well
    /// (`RenderItem.preTransform`:254-257, `RenderItem.renderItem`:140-145,
    /// `RenderEntityItem.java`:113-129).
    #[test]
    fn the_item_draw_scales_are_the_sources_chain() {
        assert_eq!(dropped_item_scale(true), 0.25);
        assert_eq!(dropped_item_scale(false), 1.0);
        assert_eq!(dropped_loop_scale(true), 0.5);
        assert_eq!(dropped_loop_scale(false), 1.0);
        assert_eq!(projectile_item_scale(0.5, false), 0.5);
        assert_eq!(item_pretransform(false), 2.0);
        assert_eq!(item_pretransform(true), 1.0);
        assert_eq!(ITEM_RENDER_SCALE, 0.5);
        assert_eq!(ITEM_CENTRE, [-0.5, -0.5, -0.5]);
    }

    /// The frame content's rotation states (`RenderItemFrame.java`:103-110).
    #[test]
    fn the_frame_rotation_steps_in_eighth_turns() {
        assert_eq!(frame_rotation_degrees(0), 0.0);
        assert_eq!(frame_rotation_degrees(1), 45.0);
        assert_eq!(frame_rotation_degrees(7), 315.0);
        assert_eq!(frame_rotation_degrees(8), 0.0);
        assert_eq!(frame_rotation_degrees(9), 45.0);
    }

    /// The painting and frame yaw fold for all four wire facings
    /// (`EntityHanging.updateFacingWithBoundingBox`:46).
    #[test]
    fn the_hanging_yaw_is_the_horizontal_index_times_ninety() {
        for facing in 0..4u8 {
            assert_eq!(painting_yaw(facing), f32::from(facing) * 90.0);
            assert_eq!(frame_yaw(facing), painting_yaw(facing));
        }
        assert_eq!(painting_yaw(5), 90.0);
        assert_eq!(painting_yaw(255), 270.0);
    }

    /// The item copy stack's centring step (`RenderEntityItem.java`:56-59).
    #[test]
    fn the_copy_stack_centres_by_its_own_step() {
        assert_eq!(item_copy_centre(1), 0.0);
        assert_eq!(item_copy_centre(2), -0.023_437_5);
        assert_eq!(item_copy_centre(5), -0.093_75);
    }

    /// The generated shape of a full-alpha sprite: the body's two quads alone
    /// (`ItemModelGenerator.java`:48-56, the alpha test `func_178391_a`:236-239).
    #[test]
    fn a_full_alpha_sprite_is_the_body_only() {
        let rgba = vec![255u8; 16 * 16 * 4];
        let vertices = generated_item(16, 16, &rgba).expect("a 16x16 sprite bakes");
        assert_eq!(vertices.positions.len(), 8, "two quads");
        // The body box: (0,0,7.5) to (16,16,8.5) in 1/16 units. Its north face comes
        // first, with the generator's [16,0,0,16] rect.
        assert_eq!(vertices.positions[0], [16.0, 16.0, 7.5]);
        assert_eq!(vertices.positions[1], [16.0, 0.0, 7.5]);
        assert_eq!(vertices.positions[2], [0.0, 0.0, 7.5]);
        assert_eq!(vertices.positions[3], [0.0, 16.0, 7.5]);
        assert_eq!(vertices.uvs[0], [1.0, 0.0]);
        assert_eq!(vertices.uvs[1], [1.0, 1.0]);
        assert_eq!(vertices.uvs[2], [0.0, 1.0]);
        assert_eq!(vertices.uvs[3], [0.0, 0.0]);
        assert_eq!(vertices.normals[0], [0.0, 0.0, -1.0]);
        // The south face follows, with the identity rect.
        assert_eq!(vertices.positions[4], [0.0, 16.0, 8.5]);
        assert_eq!(vertices.positions[5], [0.0, 0.0, 8.5]);
        assert_eq!(vertices.positions[6], [16.0, 0.0, 8.5]);
        assert_eq!(vertices.uvs[4], [0.0, 0.0]);
        assert_eq!(vertices.normals[4], [0.0, 0.0, 1.0]);
    }

    /// The generated shape of a sprite with one opaque texel: the body plus that texel's
    /// four edge strips (`ItemModelGenerator.java`:169-193's neighbour scan).
    #[test]
    fn an_alpha_cut_sprite_grows_edge_strips() {
        // A 16x16 sprite, transparent but for (8, 8), the centre.
        let mut rgba = vec![0u8; 16 * 16 * 4];
        let at = (8 * 16 + 8) * 4;
        rgba[at + 3] = 255;
        let vertices = generated_item(16, 16, &rgba).expect("a 16x16 sprite bakes");
        assert_eq!(
            vertices.positions.len(),
            8 + 4 * 4,
            "the body and four span quads"
        );
        // The first strip is the UP span: a zero-height plane at the texel row's top
        // edge, y = 16 - 8 = 8, x from 8 to 9; its v line is the source's stretched
        // 8 * 16 / 15.
        assert_eq!(vertices.positions[8], [8.0, 8.0, 7.5]);
        assert_eq!(vertices.positions[9], [8.0, 8.0, 8.5]);
        assert_eq!(vertices.positions[10], [9.0, 8.0, 8.5]);
        assert_eq!(vertices.positions[11], [9.0, 8.0, 7.5]);
        assert_eq!(vertices.uvs[8], [0.5, 8.0 / 15.0]);
        assert_eq!(vertices.normals[8], [0.0, 1.0, 0.0]);
        // Then the DOWN span at the row's bottom edge, y = 7.
        assert_eq!(vertices.positions[12], [8.0, 7.0, 8.5]);
        assert_eq!(vertices.normals[12], [0.0, -1.0, 0.0]);
        // The LEFT span sits at the column's left edge and the RIGHT span opposite; both
        // carry the source's line u on the stretched column.
        assert_eq!(vertices.positions[16], [8.0, 7.0, 8.5]);
        // The left strip's uv rect is the source's own line pair: u on the stretched
        // column, v from the free pair's `c + 1` slot first.
        assert_eq!(vertices.uvs[16], [8.0 / 15.0, 9.0 / 16.0]);
        assert_eq!(vertices.normals[16], [1.0, 0.0, 0.0]);
        assert_eq!(vertices.positions[20], [9.0, 7.0, 7.5]);
        assert_eq!(vertices.normals[20], [-1.0, 0.0, 0.0]);
    }

    /// An empty or undersized image answers `None` rather than panicking.
    #[test]
    fn a_bad_sprite_answers_none() {
        assert_eq!(generated_item(0, 16, &[]), None);
        assert_eq!(generated_item(16, 0, &[]), None);
        assert_eq!(generated_item(16, 16, &[0; 8]), None);
        assert_eq!(generated_item(16, 16, &[]), None);
    }

    /// The missing block cube: six quads, the whole sprite on each.
    #[test]
    fn the_missing_cube_is_six_faces() {
        let vertices = missing_block();
        assert_eq!(vertices.positions.len(), 24, "six quads");
        assert_eq!(vertices.positions[0], [0.0, 0.0, 16.0]);
        assert_eq!(vertices.normals[0], [0.0, -1.0, 0.0]);
        assert_eq!(vertices.uvs[0], [0.0, 0.0]);
        assert_eq!(vertices.uvs[2], [1.0, 1.0]);
        assert_eq!(vertices.normals[23], [1.0, 0.0, 0.0]);
    }
}
