//! The M1 block palette: one flat colour per block and face.
//!
//! These colours are the M1 stand-ins for the texture atlas and are replaced in
//! M2, when the mesher takes its colours from the loaded atlas instead. The ids
//! and metadata are the vanilla 1.8 block registry's, and an id the table has
//! no entry for renders as [`UNKNOWN_COLOR`], so a missing entry is
//! unmistakable rather than plausible.

/// The six faces of a block, named as vanilla names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// +Y.
    Top,
    /// -Y.
    Bottom,
    /// -Z.
    North,
    /// +Z.
    South,
    /// -X.
    West,
    /// +X.
    East,
}

impl Face {
    /// Every face, in a fixed order.
    pub const ALL: [Face; 6] = [
        Face::Top,
        Face::Bottom,
        Face::North,
        Face::South,
        Face::East,
        Face::West,
    ];

    /// The outward unit normal.
    pub fn normal(self) -> [f32; 3] {
        match self {
            Face::Top => [0.0, 1.0, 0.0],
            Face::Bottom => [0.0, -1.0, 0.0],
            Face::North => [0.0, 0.0, -1.0],
            Face::South => [0.0, 0.0, 1.0],
            Face::West => [-1.0, 0.0, 0.0],
            Face::East => [1.0, 0.0, 0.0],
        }
    }

    /// The neighbouring block this face looks at, as an offset.
    pub fn offset(self) -> (i32, i32, i32) {
        match self {
            Face::Top => (0, 1, 0),
            Face::Bottom => (0, -1, 0),
            Face::North => (0, 0, -1),
            Face::South => (0, 0, 1),
            Face::West => (-1, 0, 0),
            Face::East => (1, 0, 0),
        }
    }

    /// Vanilla's brightness for the face: top 1.0, bottom 0.5, north and south
    /// 0.8, east and west 0.6.
    pub fn brightness(self) -> f32 {
        match self {
            Face::Top => 1.0,
            Face::Bottom => 0.5,
            Face::North | Face::South => 0.8,
            Face::West | Face::East => 0.6,
        }
    }
}

/// What an id with no palette entry renders as: unmistakable magenta.
pub const UNKNOWN_COLOR: [f32; 3] = [1.0, 0.0, 1.0];

/// The colour for a block, before face brightness.
///
/// The id, the metadata and the face all take part: grass block draws a green
/// top, a log's sides depend on the variant's metadata, wool takes its dye
/// colour from the metadata, and every other entry is one colour for all six
/// faces. An id with no entry is [`UNKNOWN_COLOR`].
pub fn block_color(id: u16, meta: u8, face: Face) -> [f32; 3] {
    /// The dirt colour: dirt itself, and the grass block's sides and bottom.
    const DIRT: [f32; 3] = [0.48, 0.36, 0.25];
    /// The oak log's side colour, shared with the acacia and dark oak log.
    const LOG_SIDE: [f32; 3] = [0.42, 0.32, 0.19];

    match id {
        1 => [0.50, 0.50, 0.50],
        2 => match face {
            Face::Top => [0.35, 0.61, 0.26],
            _ => DIRT,
        },
        3 => DIRT,
        4 => [0.44, 0.44, 0.44],
        5 => [0.65, 0.53, 0.34],
        7 => [0.30, 0.30, 0.30],
        8 | 9 => [0.25, 0.40, 0.85],
        10 | 11 => [0.95, 0.51, 0.12],
        12 => [0.86, 0.81, 0.59],
        13 => [0.53, 0.51, 0.50],
        14 => [0.62, 0.58, 0.42],
        15 => [0.62, 0.58, 0.55],
        16 => [0.42, 0.42, 0.42],
        17 => match face {
            Face::Top | Face::Bottom => [0.66, 0.53, 0.35],
            _ => match meta {
                1 => [0.32, 0.23, 0.13],
                2 => [0.68, 0.62, 0.50],
                _ => LOG_SIDE,
            },
        },
        18 => match meta {
            1 => [0.28, 0.42, 0.30],
            2 => [0.45, 0.62, 0.30],
            _ => [0.32, 0.55, 0.24],
        },
        20 => [0.85, 0.92, 0.95],
        21 => [0.42, 0.45, 0.62],
        24 => [0.86, 0.82, 0.65],
        31 => [0.35, 0.65, 0.25],
        35 => match meta {
            1 => [0.95, 0.60, 0.20],
            4 => [0.92, 0.85, 0.20],
            11 => [0.25, 0.30, 0.85],
            14 => [0.65, 0.20, 0.20],
            15 => [0.10, 0.10, 0.10],
            _ => [0.93, 0.93, 0.93],
        },
        41 => [0.95, 0.80, 0.25],
        42 => [0.87, 0.87, 0.87],
        45 => [0.60, 0.36, 0.30],
        46 => [0.85, 0.30, 0.25],
        47 => [0.60, 0.50, 0.35],
        48 => [0.35, 0.45, 0.35],
        49 => [0.12, 0.10, 0.18],
        50 => [0.85, 0.70, 0.35],
        54 => [0.55, 0.40, 0.22],
        56 => [0.45, 0.70, 0.70],
        57 => [0.35, 0.85, 0.85],
        58 => [0.50, 0.36, 0.22],
        61 | 62 => [0.44, 0.44, 0.44],
        73 => [0.55, 0.35, 0.35],
        79 => [0.55, 0.70, 0.95],
        80 => [0.95, 0.97, 0.98],
        81 => [0.35, 0.55, 0.22],
        82 => [0.63, 0.65, 0.68],
        83 => [0.55, 0.72, 0.42],
        87 => [0.60, 0.25, 0.25],
        88 => [0.40, 0.33, 0.26],
        89 => [0.95, 0.85, 0.55],
        98 => [0.47, 0.47, 0.47],
        110 => [0.55, 0.50, 0.55],
        129 => [0.42, 0.65, 0.50],
        155 => [0.93, 0.92, 0.88],
        162 => LOG_SIDE,
        175 => [0.35, 0.65, 0.25],
        _ => UNKNOWN_COLOR,
    }
}
