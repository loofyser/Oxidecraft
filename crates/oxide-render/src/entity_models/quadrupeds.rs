//! The core quadrupeds: `ModelQuadruped`'s table and walk (`ModelQuadruped.java`:18-37,
//! `setRotationAngles`:79-89) as the pig, the cow, the mooshroom, the sheep and the two
//! layer-only models the sheep's wool and the pig's saddle draw with.
//!
//! Every quadruped's head, body and four legs come off the base table: the head at
//! `18 - legHeight` looking down the walk, the body a quarter turn about x over the legs'
//! pivots at `24 - legHeight`, the legs swinging in the source's own order — leg1 and leg4 in
//! phase, leg2 and leg3 against them. The subclasses then replace what they need:
//! `ModelPig` adds the snout and drops the legs to six (`ModelPig.java`:12-13), `ModelCow` its
//! shorter head, horns and udder, widens the front legs a unit and lengthens the back ones
//! (`ModelCow.java`:8-25), `ModelSheep2` the sheep's head and fleece-sized body
//! (`ModelSheep2.java`:14-19), and `ModelSheep1` the wool layer's inflated copy of them with
//! its own short shanks (`ModelSheep1.java`:14-32).
//!
//! The sheep models' `setLivingAnimations` nod the head through the sheep's own eating timer
//! (`ModelSheep2.setLivingAnimations`:29-30 over `EntitySheep.getHeadRotationPointY`:160-163
//! and `getHeadRotationAngleX`:165-175): no frame input carries that timer, so the pose pins
//! the class's resting state — the head at its table pivot, six units up, and the head's
//! pitch the frame's own. The quadrupeds' child branch (`ModelQuadruped.render`:46-62, the
//! head higher and the body at half scale) reads a child flag no frame input carries either;
//! a quadruped draw is the grown one.

use super::{Box, Model, Part, Pose, Rot};

/// The quadruped table's four legs, slotted in the source's own order: leg1 and leg2 at the
/// front pair, leg3 and leg4 at the back (`ModelQuadruped.java`:25-36).
mod slot {
    /// The head.
    pub const HEAD: usize = 0;
    /// The body.
    pub const BODY: usize = 1;
    /// The first leg, the front left.
    pub const LEG1: usize = 2;
    /// The second leg, the front right.
    pub const LEG2: usize = 3;
    /// The third leg, the back left.
    pub const LEG3: usize = 4;
    /// The fourth leg, the back right.
    pub const LEG4: usize = 5;
}

use slot::*;

/// The six-high shank of the pig's legs (`ModelQuadruped.java`:26), bare.
static LEG6_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 6.0, 4.0],
    uv: [0.0, 16.0],
    inflate: 0.0,
    mirror: false,
}];

/// The wool layer's six-high shank, half a texel out (`ModelSheep1.java`:22).
static LEG6_GROWN_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 6.0, 4.0],
    uv: [0.0, 16.0],
    inflate: 0.5,
    mirror: false,
}];

/// The twelve-high shank of the cow's and the sheep's legs.
static LEG12_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [0.0, 16.0],
    inflate: 0.0,
    mirror: false,
}];

/// One leg: its shank's boxes at the pivot the class sets with `setRotationPoint` — the base
/// table's `24 - legHeight`, or the subclass's own number (`ModelSheep1.java`:23).
const fn leg(point: [f32; 3], boxes: &'static [Box]) -> Part {
    Part {
        point,
        rest: [0.0, 0.0, 0.0],
        boxes,
        children: &[],
    }
}

/// The pig's head: the base cube with the snout on it (`ModelPig.java`:13).
static PIG_HEAD: Part = Part {
    point: [0.0, 12.0, -6.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-4.0, -4.0, -8.0],
            size: [8.0, 8.0, 8.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-2.0, 0.0, -9.0],
            size: [4.0, 3.0, 1.0],
            uv: [16.0, 16.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The pig's body (`ModelQuadruped.java`:22-24 at the six-high legs).
static PIG_BODY: Part = Part {
    point: [0.0, 11.0, 2.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-5.0, -10.0, -7.0],
        size: [10.0, 16.0, 8.0],
        uv: [28.0, 8.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The pig's parts, in draw order (`ModelQuadruped.render`:65-70).
static PIG_PARTS: [Part; 6] = [
    PIG_HEAD,
    PIG_BODY,
    leg([-3.0, 18.0, 7.0], &LEG6_BOXES),
    leg([3.0, 18.0, 7.0], &LEG6_BOXES),
    leg([-3.0, 18.0, -5.0], &LEG6_BOXES),
    leg([3.0, 18.0, -5.0], &LEG6_BOXES),
];

/// The pig's saddle layer's head: the same boxes grown half a texel (`ModelPig(0.5F)`).
static PIG_SADDLE_HEAD: Part = Part {
    point: [0.0, 12.0, -6.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-4.0, -4.0, -8.0],
            size: [8.0, 8.0, 8.0],
            uv: [0.0, 0.0],
            inflate: 0.5,
            mirror: false,
        },
        Box {
            origin: [-2.0, 0.0, -9.0],
            size: [4.0, 3.0, 1.0],
            uv: [16.0, 16.0],
            inflate: 0.5,
            mirror: false,
        },
    ],
    children: &[],
};

/// The pig's saddle layer's body, grown half a texel.
static PIG_SADDLE_BODY: Part = Part {
    point: [0.0, 11.0, 2.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-5.0, -10.0, -7.0],
        size: [10.0, 16.0, 8.0],
        uv: [28.0, 8.0],
        inflate: 0.5,
        mirror: false,
    }],
    children: &[],
};

/// The saddle layer's parts: `ModelPig(0.5F)` all through.
static PIG_SADDLE_PARTS: [Part; 6] = [
    PIG_SADDLE_HEAD,
    PIG_SADDLE_BODY,
    leg([-3.0, 18.0, 7.0], &LEG6_GROWN_BOXES),
    leg([3.0, 18.0, 7.0], &LEG6_GROWN_BOXES),
    leg([-3.0, 18.0, -5.0], &LEG6_GROWN_BOXES),
    leg([3.0, 18.0, -5.0], &LEG6_GROWN_BOXES),
];

/// The cow's head: the shorter cube and its two horns (`ModelCow.java`:8-12).
static COW_HEAD: Part = Part {
    point: [0.0, 4.0, -8.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-4.0, -4.0, -6.0],
            size: [8.0, 8.0, 6.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-5.0, -5.0, -4.0],
            size: [1.0, 3.0, 1.0],
            uv: [22.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [4.0, -5.0, -4.0],
            size: [1.0, 3.0, 1.0],
            uv: [22.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The cow's body and udder (`ModelCow.java`:13-16).
static COW_BODY: Part = Part {
    point: [0.0, 5.0, 2.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-6.0, -10.0, -7.0],
            size: [12.0, 18.0, 10.0],
            uv: [18.0, 4.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-2.0, 2.0, -8.0],
            size: [4.0, 6.0, 1.0],
            uv: [52.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The cow's parts: the twelve-high legs moved out a unit at the front and back a unit at
/// the rear (`ModelCow.java`:17-24).
static COW_PARTS: [Part; 6] = [
    COW_HEAD,
    COW_BODY,
    leg([-4.0, 12.0, 7.0], &LEG12_BOXES),
    leg([4.0, 12.0, 7.0], &LEG12_BOXES),
    leg([-4.0, 12.0, -6.0], &LEG12_BOXES),
    leg([4.0, 12.0, -6.0], &LEG12_BOXES),
];

/// The sheep's head (`ModelSheep2.java`:14-16).
static SHEEP_HEAD: Part = Part {
    point: [0.0, 6.0, -8.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, -4.0, -6.0],
        size: [6.0, 6.0, 8.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The sheep's body (`ModelSheep2.java`:17-19).
static SHEEP_BODY: Part = Part {
    point: [0.0, 5.0, 2.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -10.0, -7.0],
        size: [8.0, 16.0, 6.0],
        uv: [28.0, 8.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The sheep's parts (`ModelSheep2` keeps the base's twelve-high legs).
static SHEEP_PARTS: [Part; 6] = [
    SHEEP_HEAD,
    SHEEP_BODY,
    leg([-3.0, 12.0, 7.0], &LEG12_BOXES),
    leg([3.0, 12.0, 7.0], &LEG12_BOXES),
    leg([-3.0, 12.0, -5.0], &LEG12_BOXES),
    leg([3.0, 12.0, -5.0], &LEG12_BOXES),
];

/// The wool layer's head: the sheep's grown six-fifths of a texel out
/// (`ModelSheep1.java`:14-16).
static WOOL_HEAD: Part = Part {
    point: [0.0, 6.0, -8.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, -4.0, -4.0],
        size: [6.0, 6.0, 6.0],
        uv: [0.0, 0.0],
        inflate: 0.6,
        mirror: false,
    }],
    children: &[],
};

/// The wool layer's body, grown a texel and three quarters (`ModelSheep1.java`:17-19).
static WOOL_BODY: Part = Part {
    point: [0.0, 5.0, 2.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -10.0, -7.0],
        size: [8.0, 16.0, 6.0],
        uv: [28.0, 8.0],
        inflate: 1.75,
        mirror: false,
    }],
    children: &[],
};

/// The wool layer's parts: the sheep's table with the short shanks
/// (`ModelSheep1.java`:21-32), so the wool rides the upper leg.
static SHEEP_WOOL_PARTS: [Part; 6] = [
    WOOL_HEAD,
    WOOL_BODY,
    leg([-3.0, 12.0, 7.0], &LEG6_GROWN_BOXES),
    leg([3.0, 12.0, 7.0], &LEG6_GROWN_BOXES),
    leg([-3.0, 12.0, -5.0], &LEG6_GROWN_BOXES),
    leg([3.0, 12.0, -5.0], &LEG6_GROWN_BOXES),
];

/// The pig's model: `ModelQuadruped` with `legHeight` six plus the snout, texture `64` by
/// `32` (`ModelBase`'s own default; the quadruped classes set none).
pub static MODEL_PIG: Model = Model { parts: &PIG_PARTS };

/// The pig's saddle layer: `ModelPig(0.5F)` (`LayerSaddle.java`:12).
pub static MODEL_PIG_SADDLE: Model = Model {
    parts: &PIG_SADDLE_PARTS,
};

/// The cow's model: `ModelQuadruped` with `legHeight` twelve and `ModelCow`'s own head,
/// body and leg spread.
pub static MODEL_COW: Model = Model { parts: &COW_PARTS };

/// The sheep's model: `ModelSheep2`'s head and body over the twelve-high legs.
pub static MODEL_SHEEP: Model = Model {
    parts: &SHEEP_PARTS,
};

/// The sheep's wool layer: `ModelSheep1` (`LayerSheepWool`'s own model).
pub static MODEL_SHEEP_WOOL: Model = Model {
    parts: &SHEEP_WOOL_PARTS,
};

/// The shared quadruped walk (`ModelQuadruped.setRotationAngles`:79-89): the head on the
/// frame's angles, the body a quarter turn about x, and the four legs in the source's order —
/// leg1 and leg4 on one phase, leg2 and leg3 on the other.
pub fn pose_quadruped(pose: &Pose, out: &mut [Rot]) {
    out[HEAD].angles = [
        pose.head_pitch.to_radians(),
        pose.head_yaw.to_radians(),
        0.0,
    ];
    out[BODY].angles[0] = std::f32::consts::FRAC_PI_2;
    let step = pose.limb_swing * 0.6662;
    let swing = 1.4 * pose.limb_swing_amount;
    out[LEG1].angles[0] = step.cos() * swing;
    out[LEG2].angles[0] = (step + std::f32::consts::PI).cos() * swing;
    out[LEG3].angles[0] = (step + std::f32::consts::PI).cos() * swing;
    out[LEG4].angles[0] = step.cos() * swing;
}

/// The pig's pose: the quadruped walk untouched — `ModelPig` adds no terms of its own.
pub fn pose_pig(pose: &Pose, out: &mut [Rot]) {
    pose_quadruped(pose, out);
}

/// The sheep's pose: the quadruped walk with the class's own head pitch over it
/// (`ModelSheep2.setRotationAngles`:41).
///
/// At rest that pitch is the frame's own head pitch — the source's
/// `getHeadRotationAngleX` reads the entity's raw `rotationPitch` when the eating timer is
/// down (`EntitySheep.getHeadRotationAngleX`:174), the same angle the base walk writes from
/// its interpolated head pitch — so the two models' resting poses agree, which the pin
/// records rather than an omission.
pub fn pose_sheep(pose: &Pose, out: &mut [Rot]) {
    pose_quadruped(pose, out);
    out[HEAD].angles[0] = pose.head_pitch.to_radians();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `addBox` call, spelled as the source spells it.
    fn b(origin: [f32; 3], size: [f32; 3], uv: [f32; 2], inflate: f32, mirror: bool) -> Box {
        Box {
            origin,
            size,
            uv,
            inflate,
            mirror,
        }
    }

    #[test]
    fn the_pig_geometry_matches_its_class() {
        assert_eq!(MODEL_PIG.parts.len(), 6, "a quadruped's six parts draw");
        assert_eq!(
            MODEL_PIG.parts[HEAD].boxes,
            [
                b([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 0.0], 0.0, false),
                b([-2.0, 0.0, -9.0], [4.0, 3.0, 1.0], [16.0, 16.0], 0.0, false),
            ]
        );
        assert_eq!(
            MODEL_PIG.parts[BODY].boxes,
            [b(
                [-5.0, -10.0, -7.0],
                [10.0, 16.0, 8.0],
                [28.0, 8.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_PIG.parts[LEG1].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 6.0, 4.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        // The pivots: the head twelve up for the six-high legs, the body eleven, the legs at
        // eighteen (`24 - 6`) in the source's front pair (z 7) and back pair (z -5).
        assert_eq!(MODEL_PIG.parts[HEAD].point, [0.0, 12.0, -6.0]);
        assert_eq!(MODEL_PIG.parts[BODY].point, [0.0, 11.0, 2.0]);
        assert_eq!(MODEL_PIG.parts[LEG1].point, [-3.0, 18.0, 7.0]);
        assert_eq!(MODEL_PIG.parts[LEG2].point, [3.0, 18.0, 7.0]);
        assert_eq!(MODEL_PIG.parts[LEG3].point, [-3.0, 18.0, -5.0]);
        assert_eq!(MODEL_PIG.parts[LEG4].point, [3.0, 18.0, -5.0]);
    }

    #[test]
    fn the_saddle_layer_is_the_pig_grown_half_a_texel() {
        assert_eq!(MODEL_PIG_SADDLE.parts.len(), 6);
        for (part, plain) in MODEL_PIG_SADDLE.parts.iter().zip(MODEL_PIG.parts) {
            assert_eq!(
                part.point, plain.point,
                "the saddle layer keeps the pig's pivots"
            );
            assert_eq!(part.boxes.len(), plain.boxes.len());
            for (grown, bare) in part.boxes.iter().zip(plain.boxes) {
                assert_eq!(
                    grown.origin, bare.origin,
                    "the saddle keeps the pig's boxes"
                );
                assert_eq!(grown.size, bare.size);
                assert_eq!(grown.uv, bare.uv);
                assert_eq!(
                    grown.inflate,
                    bare.inflate + 0.5,
                    "every saddle box grows half a texel: {} grew {}",
                    grown.inflate,
                    bare.inflate + 0.5
                );
            }
        }
    }

    #[test]
    fn the_cow_geometry_matches_its_class() {
        assert_eq!(
            MODEL_COW.parts[HEAD].boxes,
            [
                b([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0.0, 0.0], 0.0, false),
                b([-5.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22.0, 0.0], 0.0, false),
                b([4.0, -5.0, -4.0], [1.0, 3.0, 1.0], [22.0, 0.0], 0.0, false),
            ]
        );
        assert_eq!(
            MODEL_COW.parts[BODY].boxes,
            [
                b(
                    [-6.0, -10.0, -7.0],
                    [12.0, 18.0, 10.0],
                    [18.0, 4.0],
                    0.0,
                    false
                ),
                b([-2.0, 2.0, -8.0], [4.0, 6.0, 1.0], [52.0, 0.0], 0.0, false),
            ]
        );
        assert_eq!(
            MODEL_COW.parts[LEG1].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        // The pivots: the head down at four, the body at five, and the wider, longer spread
        // (`ModelCow.java`:17-24).
        assert_eq!(MODEL_COW.parts[HEAD].point, [0.0, 4.0, -8.0]);
        assert_eq!(MODEL_COW.parts[BODY].point, [0.0, 5.0, 2.0]);
        assert_eq!(MODEL_COW.parts[LEG1].point, [-4.0, 12.0, 7.0]);
        assert_eq!(MODEL_COW.parts[LEG2].point, [4.0, 12.0, 7.0]);
        assert_eq!(MODEL_COW.parts[LEG3].point, [-4.0, 12.0, -6.0]);
        assert_eq!(MODEL_COW.parts[LEG4].point, [4.0, 12.0, -6.0]);
    }

    #[test]
    fn the_sheep_geometry_matches_its_class() {
        assert_eq!(
            MODEL_SHEEP.parts[HEAD].boxes,
            [b(
                [-3.0, -4.0, -6.0],
                [6.0, 6.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SHEEP.parts[BODY].boxes,
            [b(
                [-4.0, -10.0, -7.0],
                [8.0, 16.0, 6.0],
                [28.0, 8.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SHEEP.parts[LEG1].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(MODEL_SHEEP.parts[HEAD].point, [0.0, 6.0, -8.0]);
        assert_eq!(MODEL_SHEEP.parts[BODY].point, [0.0, 5.0, 2.0]);
        assert_eq!(MODEL_SHEEP.parts[LEG1].point, [-3.0, 12.0, 7.0]);
        assert_eq!(MODEL_SHEEP.parts[LEG4].point, [3.0, 12.0, -5.0]);
    }

    #[test]
    fn the_wool_layer_is_the_sheep_grown_and_cropped() {
        assert_eq!(MODEL_SHEEP_WOOL.parts.len(), 6);
        // The head: six-fifths of a texel out (`ModelSheep1.java`:15).
        assert_eq!(
            MODEL_SHEEP_WOOL.parts[HEAD].boxes,
            [b(
                [-3.0, -4.0, -4.0],
                [6.0, 6.0, 6.0],
                [0.0, 0.0],
                0.6,
                false
            )]
        );
        // The body: the sheep's, grown a texel and three quarters (`:18`).
        assert_eq!(
            MODEL_SHEEP_WOOL.parts[BODY].boxes,
            [b(
                [-4.0, -10.0, -7.0],
                [8.0, 16.0, 6.0],
                [28.0, 8.0],
                1.75,
                false
            )]
        );
        // The legs: six high where the sheep's stand twelve, half a texel out, at the same
        // pivots (`:21-32`).
        assert_eq!(
            MODEL_SHEEP_WOOL.parts[LEG1].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 6.0, 4.0],
                [0.0, 16.0],
                0.5,
                false
            )]
        );
        assert_eq!(MODEL_SHEEP_WOOL.parts[LEG1].point, [-3.0, 12.0, 7.0]);
        assert_eq!(MODEL_SHEEP_WOOL.parts[LEG4].point, [3.0, 12.0, -5.0]);
        // The head's pivot matches the sheep's, six up (`:16`).
        assert_eq!(MODEL_SHEEP_WOOL.parts[HEAD].point, [0.0, 6.0, -8.0]);
    }

    #[test]
    fn the_quadruped_legs_swing_in_the_sources_order() {
        // At a whole quarter of the two-pi walk the pairs stand against each other:
        // leg1 and leg4 take the cosine, leg2 and leg3 the cosine a half turn over
        // (`ModelQuadruped.setRotationAngles`:85-88).
        let sketch = Pose {
            limb_swing: 0.0,
            limb_swing_amount: 1.0,
            ..Pose::default()
        };
        let mut out = MODEL_COW.rest();
        pose_quadruped(&sketch, &mut out);
        assert!(
            (out[LEG1].angles[0] - 1.4).abs() < 1.0e-6,
            "leg1: {}",
            out[LEG1].angles[0]
        );
        assert!(
            (out[LEG2].angles[0] + 1.4).abs() < 1.0e-6,
            "leg2: {}",
            out[LEG2].angles[0]
        );
        assert!(
            (out[LEG3].angles[0] + 1.4).abs() < 1.0e-6,
            "leg3: {}",
            out[LEG3].angles[0]
        );
        assert!(
            (out[LEG4].angles[0] - 1.4).abs() < 1.0e-6,
            "leg4: {}",
            out[LEG4].angles[0]
        );
        // The body lies a quarter turn over and the head takes the frame's angles.
        assert_eq!(out[BODY].angles[0], std::f32::consts::FRAC_PI_2);
        let sketch = Pose {
            head_yaw: 20.0,
            head_pitch: -5.0,
            ..Pose::default()
        };
        let mut out = MODEL_PIG.rest();
        pose_pig(&sketch, &mut out);
        assert!((out[HEAD].angles[0] - (-5.0_f32).to_radians()).abs() < 1.0e-6);
        assert!((out[HEAD].angles[1] - 20.0_f32.to_radians()).abs() < 1.0e-6);
        // The sheep's own override lands the head where the base walk already had it, the
        // frame's interpolated pitch (`ModelSheep2.setRotationAngles`:41, the resting branch
        // of `EntitySheep.getHeadRotationAngleX`).
        let mut sheep = MODEL_SHEEP.rest();
        pose_sheep(&sketch, &mut sheep);
        assert_eq!(
            sheep[HEAD].angles, out[HEAD].angles,
            "the sheep's resting head stands where the walk's does"
        );
        assert_eq!(sheep[LEG1].angles, out[LEG1].angles);
    }
}
