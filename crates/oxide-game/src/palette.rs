//! The face table: the six faces of a block, their normals, their outward
//! offsets and vanilla's per-face brightness.
//!
//! M1 also carried a flat colour per block here; M2's mesher takes its colours
//! from the loaded atlas and the baked models, so only the face table remains.
//! Its values are the source's: the shading factors are `FaceBakery`'s own face
//! brightness table (top 1.0, bottom 0.5, north and south 0.8, east and west
//! 0.6), and the offsets are `EnumFacing`'s front offsets.

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
