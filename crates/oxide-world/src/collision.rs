//! The collision box the movement model resolves against.
//!
//! One [`CollisionBox`] is one axis-aligned box of world space. The source
//! calls this an `AxisAlignedBB` (`refs/_src/MCP-919/src/minecraft/net/minecraft/util/AxisAlignedBB.java`);
//! a block hands out its box through `Block.getCollisionBoundingBox`, and
//! `World.getCollidingBoundingBoxes` (`World.java:1262-1307`) collects the
//! boxes of every block a query box touches. The behaviour table's rows carry
//! the boxes each block class declares; the movement model (and the
//! interaction raycast with it) resolves against the boxes, not the blocks.
//!
//! Every box is stored as the source stores it — absolute world coordinates,
//! `min` on the low side and `max` on the high side — so `full()` at block
//! `(x, y, z)` is the block's own unit cube and the block classes' own
//! partial boxes are built with [`CollisionBox::of`] and placed with
//! [`CollisionBox::offset`].
//!
//! Unlike the source's `AxisAlignedBB`, which carries a growth vector and a
//! re-centring rule for entity boxes, this is a plain pair of corners: the
//! movement model's own arithmetic (`addCoord`, `expand`, `intersectsWith`)
//! lives beside the rules that use it.

/// An axis-aligned collision box in world coordinates.
///
/// `min[i] <= max[i]` for the three axes; the source never normalises a box,
/// so neither does this type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionBox {
    /// The low corner.
    pub min: [f64; 3],
    /// The high corner.
    pub max: [f64; 3],
}

impl CollisionBox {
    /// The full-block box, `[0, 1]` on every axis.
    ///
    /// The box a default full cube reports: `Block.getCollisionBoundingBox`
    /// returns the block's `minX..maxZ` bounds, which a full cube leaves at
    /// their defaults (`block/Block.java:499-502`).
    pub const fn full() -> CollisionBox {
        CollisionBox {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
        }
    }

    /// The box from two explicit corners, in block-local or world space.
    ///
    /// The partial boxes the block classes declare: a bottom slab is
    /// `of([0, 0, 0], [1, 0.5, 1])` (`BlockSlab.java:34`'s
    /// `setBlockBounds(0.0F, 0.0F, 0.0F, 1.0F, 0.5F, 1.0F)`), a ladder a thin
    /// plate on its attached face (`BlockLadder.setBlockBoundsBasedOnState`).
    pub const fn of(min: [f64; 3], max: [f64; 3]) -> CollisionBox {
        CollisionBox { min, max }
    }

    /// The box moved by the offset — the source's `AxisAlignedBB.offset`
    /// (`AxisAlignedBB.java:117-125`).
    pub const fn offset(self, x: f64, y: f64, z: f64) -> CollisionBox {
        CollisionBox {
            min: [self.min[0] + x, self.min[1] + y, self.min[2] + z],
            max: [self.max[0] + x, self.max[1] + y, self.max[2] + z],
        }
    }
}

#[cfg(test)]
mod tests {
    //! The pinned shapes the behaviour table and the movement model consume.

    use super::CollisionBox;

    #[test]
    fn the_full_box_is_the_sources_unit_cube() {
        let full = CollisionBox::full();
        assert_eq!(full.min, [0.0, 0.0, 0.0]);
        assert_eq!(full.max, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn of_keeps_the_corners_it_is_given() {
        let slab = CollisionBox::of([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);
        assert_eq!(slab.min, [0.0, 0.0, 0.0]);
        assert_eq!(slab.max, [1.0, 0.5, 1.0]);
    }

    #[test]
    fn offset_moves_both_corners() {
        let at_origin = CollisionBox::full().offset(2.0, -1.0, 4.0);
        assert_eq!(at_origin.min, [2.0, -1.0, 4.0]);
        assert_eq!(at_origin.max, [3.0, 0.0, 5.0]);
    }
}
