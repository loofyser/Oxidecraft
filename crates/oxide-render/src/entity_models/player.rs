//! The player model: the source's `ModelPlayer` geometry and the biped's pose.
//!
//! The tables are `ModelPlayer`'s own — `ModelBiped`'s head, body and legs with
//! `ModelPlayer`'s arms (the wide pair and the 3-texel slim pair, each with its own UV cell)
//! and its overlay parts: the hat, the jacket, the sleeves and the pant legs, all inflated a
//! quarter texel and drawn only where the model-parts byte wears them. The cape is its own
//! part, built against the source's own 64x32 sheet (`ModelPlayer.java:23-25`).
//!
//! [`pose`] is `ModelBiped.setRotationAngles` with `ModelPlayer.setRotationAngles`' copies:
//! the head's angles, the limb-swing sways, the arm-swing term over the swing progress, the
//! sneak adjustments and offsets, the idle arm sway over the entity's age, and the copies that
//! hang each overlay on its base part. The parts byte gates the six wear parts and the cape
//! (`RenderPlayer.setModelVisibilities`, `LayerCape.doRenderLayer`); the base parts always
//! draw. Head and pitch angles are written as they arrive — `ModelBiped.setRotationAngles`
//! does not clamp them, and neither does this pose.
//!
//! [`cape_rotation`] is the cape layer's own wave (`LayerCape.doRenderLayer`): the chaser
//! lag, read here from the frame's own motion between its tick pair, turns the cape's box
//! behind the player, with the sneak lift the layer adds.

use std::f32::consts::PI;

use super::{Box, Model, Part, Pose, Rot};

/// The parts byte with every model part worn, the client's own default until the settings
/// screen can clear a bit (`EnumPlayerModelParts`' seven bits; the options file's default is
/// all of them on).
pub const PARTS_ALL: u8 = 0x7F;

/// The cape bit of the parts byte (`EnumPlayerModelParts.CAPE`, bit 0).
pub const PART_CAPE: u8 = 1 << 0;

/// The jacket bit (bit 1).
pub const PART_JACKET: u8 = 1 << 1;

/// The left sleeve bit (bit 2).
pub const PART_LEFT_SLEEVE: u8 = 1 << 2;

/// The right sleeve bit (bit 3).
pub const PART_RIGHT_SLEEVE: u8 = 1 << 3;

/// The left pant leg bit (bit 4).
pub const PART_LEFT_PANTS_LEG: u8 = 1 << 4;

/// The right pant leg bit (bit 5).
pub const PART_RIGHT_PANTS_LEG: u8 = 1 << 5;

/// The hat bit (bit 6).
pub const PART_HAT: u8 = 1 << 6;

/// The number of parts `pose` writes, in order.
pub const PART_COUNT: usize = 13;

/// The player sheet's size in texels: the skin, 64 by 64 (`ModelPlayer`'s constructor).
pub const PLAYER_TEXTURE_SIZE: [f32; 2] = [64.0, 64.0];

/// The cape sheet's size in texels: 64 by 32, the cape renderer's own set (`ModelPlayer`'s
/// constructor).
pub const CAPE_TEXTURE_SIZE: [f32; 2] = [64.0, 32.0];

/// The player model's own pose input: the cape layer's motion term.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CapeMotion {
    /// The entity's displacement over the tick pair the frame spans, in blocks — the
    /// frame's stand-in for the chasing-position delta the source reads; the window
    /// carries neither the smoothed chaser nor the camera-yaw sensor the source derives
    /// it from, so the wave reads this motion and the sneak lift alone.
    pub motion: [f32; 3],
}

/// The head's box: 8x8x8 at the pivot's back-top corner (`ModelBiped.java:55-57`).
static HEAD_BOXES: [Box; 1] = [Box {
    origin: [-4.0, -8.0, -4.0],
    size: [8.0, 8.0, 8.0],
    uv: [0.0, 0.0],
    inflate: 0.0,
    mirror: false,
}];

/// The hat: the head's box inflated half a texel on `ModelPlayer`'s overlay cell
/// (`ModelBiped.java:58-60`).
static HEADWEAR_BOXES: [Box; 1] = [Box {
    origin: [-4.0, -8.0, -4.0],
    size: [8.0, 8.0, 8.0],
    uv: [32.0, 0.0],
    inflate: 0.5,
    mirror: false,
}];

/// The body: 8x12x4 (`ModelBiped.java:61-63`).
static BODY_BOXES: [Box; 1] = [Box {
    origin: [-4.0, 0.0, -2.0],
    size: [8.0, 12.0, 4.0],
    uv: [16.0, 16.0],
    inflate: 0.0,
    mirror: false,
}];

/// The wide right arm: 4 texels wide (`ModelPlayer.java:32-34`).
static RIGHT_ARM_WIDE_BOXES: [Box; 1] = [Box {
    origin: [-3.0, -2.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [40.0, 16.0],
    inflate: 0.0,
    mirror: false,
}];

/// The slim right arm: 3 texels wide (`ModelPlayer.java:33-34`).
static RIGHT_ARM_SLIM_BOXES: [Box; 1] = [Box {
    origin: [-2.0, -2.0, -2.0],
    size: [3.0, 12.0, 4.0],
    uv: [40.0, 16.0],
    inflate: 0.0,
    mirror: false,
}];

/// The wide left arm: its own cell, 4 texels wide (`ModelPlayer.java:44-46`).
static LEFT_ARM_WIDE_BOXES: [Box; 1] = [Box {
    origin: [-1.0, -2.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [32.0, 48.0],
    inflate: 0.0,
    mirror: false,
}];

/// The slim left arm: its own cell, 3 texels wide (`ModelPlayer.java:29-31`).
static LEFT_ARM_SLIM_BOXES: [Box; 1] = [Box {
    origin: [-1.0, -2.0, -2.0],
    size: [3.0, 12.0, 4.0],
    uv: [32.0, 48.0],
    inflate: 0.0,
    mirror: false,
}];

/// The right leg (`ModelBiped.java:71-73`).
static RIGHT_LEG_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [0.0, 16.0],
    inflate: 0.0,
    mirror: false,
}];

/// The left leg: `ModelPlayer`'s own cell (`ModelPlayer.java:55-57`).
static LEFT_LEG_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [16.0, 48.0],
    inflate: 0.0,
    mirror: false,
}];

/// The left pant leg: a quarter-texel overlay (`ModelPlayer.java:58-60`).
static LEFT_LEGWEAR_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [0.0, 48.0],
    inflate: 0.25,
    mirror: false,
}];

/// The right pant leg (`ModelPlayer.java:61-63`).
static RIGHT_LEGWEAR_BOXES: [Box; 1] = [Box {
    origin: [-2.0, 0.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [0.0, 32.0],
    inflate: 0.25,
    mirror: false,
}];

/// The wide left sleeve (`ModelPlayer.java:47-49`).
static LEFT_ARMWEAR_WIDE_BOXES: [Box; 1] = [Box {
    origin: [-1.0, -2.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [48.0, 48.0],
    inflate: 0.25,
    mirror: false,
}];

/// The slim left sleeve (`ModelPlayer.java:35-37`).
static LEFT_ARMWEAR_SLIM_BOXES: [Box; 1] = [Box {
    origin: [-1.0, -2.0, -2.0],
    size: [3.0, 12.0, 4.0],
    uv: [48.0, 48.0],
    inflate: 0.25,
    mirror: false,
}];

/// The wide right sleeve (`ModelPlayer.java:50-52`).
static RIGHT_ARMWEAR_WIDE_BOXES: [Box; 1] = [Box {
    origin: [-3.0, -2.0, -2.0],
    size: [4.0, 12.0, 4.0],
    uv: [40.0, 32.0],
    inflate: 0.25,
    mirror: false,
}];

/// The slim right sleeve (`ModelPlayer.java:38-40`).
static RIGHT_ARMWEAR_SLIM_BOXES: [Box; 1] = [Box {
    origin: [-2.0, -2.0, -2.0],
    size: [3.0, 12.0, 4.0],
    uv: [40.0, 32.0],
    inflate: 0.25,
    mirror: false,
}];

/// The jacket (`ModelPlayer.java:64-66`).
static BODY_WEAR_BOXES: [Box; 1] = [Box {
    origin: [-4.0, 0.0, -2.0],
    size: [8.0, 12.0, 4.0],
    uv: [16.0, 32.0],
    inflate: 0.25,
    mirror: false,
}];

/// The cape: 10x16x1 on its own 64x32 sheet (`ModelPlayer.java:23-25`).
static CAPE_BOXES: [Box; 1] = [Box {
    origin: [-5.0, 0.0, -1.0],
    size: [10.0, 16.0, 1.0],
    uv: [0.0, 0.0],
    inflate: 0.0,
    mirror: false,
}];

/// The wide model's parts, in the order the source draws them: `ModelBiped.render`'s seven,
/// then `ModelPlayer.render`'s five overlays, then the cape the layer draws.
static WIDE_PARTS: [Part; PART_COUNT] = [
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &HEAD_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &BODY_BOXES,
        children: &[],
    },
    Part {
        point: [-5.0, 2.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_ARM_WIDE_BOXES,
        children: &[],
    },
    Part {
        point: [5.0, 2.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_ARM_WIDE_BOXES,
        children: &[],
    },
    Part {
        point: [-1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_LEG_BOXES,
        children: &[],
    },
    Part {
        point: [1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_LEG_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &HEADWEAR_BOXES,
        children: &[],
    },
    Part {
        point: [1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_LEGWEAR_BOXES,
        children: &[],
    },
    Part {
        point: [-1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_LEGWEAR_BOXES,
        children: &[],
    },
    Part {
        point: [5.0, 2.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_ARMWEAR_WIDE_BOXES,
        children: &[],
    },
    Part {
        point: [-5.0, 2.0, 10.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_ARMWEAR_WIDE_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &BODY_WEAR_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &CAPE_BOXES,
        children: &[],
    },
];

/// The slim model's parts: the same tables with the 3-texel arms and their sleeves, and the
/// arms' pivots half a texel lower (`ModelPlayer.java:27-41`).
static SLIM_PARTS: [Part; PART_COUNT] = [
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &HEAD_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &BODY_BOXES,
        children: &[],
    },
    Part {
        point: [-5.0, 2.5, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_ARM_SLIM_BOXES,
        children: &[],
    },
    Part {
        point: [5.0, 2.5, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_ARM_SLIM_BOXES,
        children: &[],
    },
    Part {
        point: [-1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_LEG_BOXES,
        children: &[],
    },
    Part {
        point: [1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_LEG_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &HEADWEAR_BOXES,
        children: &[],
    },
    Part {
        point: [1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_LEGWEAR_BOXES,
        children: &[],
    },
    Part {
        point: [-1.9, 12.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_LEGWEAR_BOXES,
        children: &[],
    },
    Part {
        point: [5.0, 2.5, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &LEFT_ARMWEAR_SLIM_BOXES,
        children: &[],
    },
    Part {
        point: [-5.0, 2.5, 10.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &RIGHT_ARMWEAR_SLIM_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &BODY_WEAR_BOXES,
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &CAPE_BOXES,
        children: &[],
    },
];

/// The cape in a model of its own: the layer's box, built against the 64x32 sheet its own
/// renderer carries.
static CAPE_PARTS: [Part; 1] = [Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &CAPE_BOXES,
    children: &[],
}];

/// The wide player model: the source's `ModelPlayer(0, false)`.
pub static MODEL_PLAYER_WIDE: Model = Model { parts: &WIDE_PARTS };

/// The slim player model: the source's `ModelPlayer(0, true)`.
pub static MODEL_PLAYER_SLIM: Model = Model { parts: &SLIM_PARTS };

/// The cape part alone, for the layer's own build through [`CAPE_TEXTURE_SIZE`].
pub static MODEL_PLAYER_CAPE: Model = Model { parts: &CAPE_PARTS };

/// The cape part's transform for a pose: its pivot's sneak lift and the layer's gate.
///
/// The box itself hangs at the origin with no rest rotation; the layer's wave turns it
/// through the draw's own matrix, and the sneak lift raises the pivot a tenth of a block
/// (`ModelPlayer.setRotationAngles`).
pub fn cape_rot(pose: &Pose, parts: u8) -> Rot {
    Rot {
        point: [0.0, if pose.sneak { 2.0 } else { 0.0 }, 0.0],
        angles: [0.0, 0.0, 0.0],
        visible: parts & PART_CAPE != 0,
    }
}

/// The cape layer's rotation for a pose, in degrees `(x, y, z)`.
///
/// `LayerCape.doRenderLayer`'s turns, composed in its order: the chaser lag drives the pitch
/// `6 + f2/2 + f1` and the yaw pair `180 - f3/2` and `f3/2`; the sneak takes `f1` up a
/// further 25 degrees. The lag reads the frame's motion between its tick pair in place of the
/// source's smoothed chaser delta; the camera-yaw and walk-distance term the window carries no
/// state for is left out.
pub fn cape_rotation(pose: &Pose, motion: [f32; 3]) -> [f32; 3] {
    let yaw = pose.body_yaw.to_radians();
    let d3 = yaw.sin();
    let d4 = -yaw.cos();
    let mut f1 = (motion[1] * 10.0).clamp(-6.0, 32.0);
    let mut f2 = (motion[0] * d3 + motion[2] * d4) * 100.0;
    let f3 = (motion[0] * d4 - motion[2] * d3) * 100.0;
    if f2 < 0.0 {
        f2 = 0.0;
    }
    if pose.sneak {
        f1 += 25.0;
    }
    [6.0 + f2 / 2.0 + f1, 180.0 - f3 / 2.0, f3 / 2.0]
}

/// Writes the biped pose over `out`, the model's rest transforms.
///
/// `out` holds one slot per part in the order [`MODEL_PLAYER_WIDE`] declares, seeded from
/// [`super::Model::rest`]; the call does nothing if it is shorter. The arm-swing term reads
/// [`Pose::swing_progress`] on the source's six-tick grid — the values never reach `1.0` —
/// and the walk sways read the limb-swing pair as handed in.
pub fn pose(pose: &Pose, parts: u8, out: &mut [Rot]) {
    if out.len() < PART_COUNT {
        return;
    }
    let head_yaw = pose.head_yaw.to_radians();
    let head_pitch = pose.head_pitch.to_radians();
    out[0].angles = [head_pitch, head_yaw, 0.0];

    // The walking sway (`ModelBiped.setRotationAngles`): the arms in opposite phase, the
    // legs in opposite phase again.
    let amount = pose.limb_swing_amount;
    let phase = pose.limb_swing * 0.6662;
    out[2].angles = [(phase + PI).cos() * 2.0 * amount * 0.5, 0.0, 0.0];
    out[3].angles = [phase.cos() * 2.0 * amount * 0.5, 0.0, 0.0];
    out[4].angles = [phase.cos() * 1.4 * amount, 0.0, 0.0];
    out[5].angles = [(phase + PI).cos() * 1.4 * amount, 0.0, 0.0];

    // The arm swing about the body, over the swing progress.
    let swing = pose.swing_progress;
    let body_yaw = (swing.sqrt() * PI * 2.0).sin() * 0.2;
    out[1].angles[1] = body_yaw;
    out[2].point[0] = -body_yaw.cos() * 5.0;
    out[2].point[2] = body_yaw.sin() * 5.0;
    out[3].point[0] = body_yaw.cos() * 5.0;
    out[3].point[2] = -body_yaw.sin() * 5.0;
    out[2].angles[1] += body_yaw;
    out[3].angles[1] += body_yaw;
    out[3].angles[0] += body_yaw;
    let mut eased = 1.0 - swing;
    eased = eased * eased;
    eased = eased * eased;
    eased = 1.0 - eased;
    let lift = (eased * PI).sin();
    let dip = (swing * PI).sin() * -(out[0].angles[0] - 0.7) * 0.75;
    out[2].angles[0] -= lift * 1.2 + dip;
    out[2].angles[1] += body_yaw * 2.0;
    out[2].angles[2] += (swing * PI).sin() * -0.4;

    // The sneak adjustments: the body's lean, the arms half a radian forward, the legs and
    // the head drawn in; at rest the legs' pivots sit a tenth back and their default height.
    if pose.sneak {
        out[1].angles[0] = 0.5;
        out[2].angles[0] += 0.4;
        out[3].angles[0] += 0.4;
        out[4].point[2] = 4.0;
        out[5].point[2] = 4.0;
        out[4].point[1] = 9.0;
        out[5].point[1] = 9.0;
        out[0].point[1] = 1.0;
    } else {
        out[1].angles[0] = 0.0;
        out[4].point[2] = 0.1;
        out[5].point[2] = 0.1;
        out[4].point[1] = 12.0;
        out[5].point[1] = 12.0;
        out[0].point[1] = 0.0;
    }

    // The idle sway over the entity's age.
    let roll = (pose.age * 0.09).cos() * 0.05 + 0.05;
    out[2].angles[2] += roll;
    out[3].angles[2] -= roll;
    let drift = (pose.age * 0.067).sin() * 0.05;
    out[2].angles[0] += drift;
    out[3].angles[0] -= drift;

    // `copyModelAngles`: each overlay hangs where its base part hangs. The visibility flags
    // are the parts byte's, written after, not copied.
    out[6].point = out[0].point;
    out[6].angles = out[0].angles;
    out[7].point = out[5].point;
    out[7].angles = out[5].angles;
    out[8].point = out[4].point;
    out[8].angles = out[4].angles;
    out[9].point = out[3].point;
    out[9].angles = out[3].angles;
    out[10].point = out[2].point;
    out[10].angles = out[2].angles;
    out[11].point = out[1].point;
    out[11].angles = out[1].angles;

    // The cape's pivot: `ModelPlayer.setRotationAngles` lifts it two texels while sneaking.
    out[12].point[1] = if pose.sneak { 2.0 } else { 0.0 };

    // The parts byte's gates (`RenderPlayer.setModelVisibilities`, `LayerCape`): the six
    // wear parts and the cape draw only where their bit is worn.
    out[6].visible = parts & PART_HAT != 0;
    out[11].visible = parts & PART_JACKET != 0;
    out[9].visible = parts & PART_LEFT_SLEEVE != 0;
    out[10].visible = parts & PART_RIGHT_SLEEVE != 0;
    out[7].visible = parts & PART_LEFT_PANTS_LEG != 0;
    out[8].visible = parts & PART_RIGHT_PANTS_LEG != 0;
    out[12].visible = parts & PART_CAPE != 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_models::{PoseExtra, Vertices, build_vertices};

    /// A pose with every field at rest: no swing, no walk, no age, facing forward.
    fn rest_pose() -> Pose {
        Pose {
            extra: PoseExtra::Player(CapeMotion::default()),
            ..Pose::default()
        }
    }

    /// Whether two angles agree to within a thousandth of a radian.
    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1.0e-3)
    }

    /// Builds one box through a model of its own, for the UV literals.
    fn box_vertices(geometry: &'static Box, sheet: [f32; 2]) -> Vertices {
        static BOXES: [Box; 1] = [Box {
            origin: [0.0, 0.0, 0.0],
            size: [0.0, 0.0, 0.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }];
        let boxes: &'static [Box] = std::boxed::Box::leak(vec![*geometry].into_boxed_slice());
        let parts: &'static [Part] = std::boxed::Box::leak(
            vec![Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes,
                children: &[],
            }]
            .into_boxed_slice(),
        );
        let _ = &BOXES;
        let model = Model { parts };
        let transforms = model.rest();
        build_vertices(&model, &transforms, sheet)
    }

    #[test]
    fn the_model_draws_thirteen_parts_of_one_box_each() {
        assert_eq!(MODEL_PLAYER_WIDE.parts.len(), PART_COUNT);
        assert_eq!(MODEL_PLAYER_SLIM.parts.len(), PART_COUNT);
        assert_eq!(MODEL_PLAYER_CAPE.parts.len(), 1);
        for model in [&MODEL_PLAYER_WIDE, &MODEL_PLAYER_SLIM] {
            let boxes: usize = model.parts.iter().map(|part| part.boxes.len()).sum();
            assert_eq!(boxes, PART_COUNT);
            let vertices = build_vertices(model, &model.rest(), PLAYER_TEXTURE_SIZE);
            assert_eq!(vertices.positions.len(), PART_COUNT * 24);
        }
    }

    #[test]
    fn the_head_body_arm_and_overlay_boxes_match_the_source() {
        let boxes = |model: &'static Model, part: usize| model.parts[part].boxes[0];
        let wide = &MODEL_PLAYER_WIDE;
        let slim = &MODEL_PLAYER_SLIM;
        // The head: 8x8x8 on the skin's top-left cell.
        assert_eq!(
            boxes(wide, 0),
            Box {
                origin: [-4.0, -8.0, -4.0],
                size: [8.0, 8.0, 8.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            }
        );
        // The hat: the head again, half a texel out and on the overlay cell.
        assert_eq!(
            boxes(wide, 6),
            Box {
                origin: [-4.0, -8.0, -4.0],
                size: [8.0, 8.0, 8.0],
                uv: [32.0, 0.0],
                inflate: 0.5,
                mirror: false,
            }
        );
        // The body: 8x12x4.
        assert_eq!(
            boxes(wide, 1),
            Box {
                origin: [-4.0, 0.0, -2.0],
                size: [8.0, 12.0, 4.0],
                uv: [16.0, 16.0],
                inflate: 0.0,
                mirror: false,
            }
        );
        // The wide right arm: 4 texels, pivot five left and two up.
        assert_eq!(
            boxes(wide, 2),
            Box {
                origin: [-3.0, -2.0, -2.0],
                size: [4.0, 12.0, 4.0],
                uv: [40.0, 16.0],
                inflate: 0.0,
                mirror: false,
            }
        );
        assert_eq!(wide.parts[2].point, [-5.0, 2.0, 0.0]);
        // The slim right arm: 3 texels on the same cell, its pivot half a texel lower.
        assert_eq!(
            boxes(slim, 2),
            Box {
                origin: [-2.0, -2.0, -2.0],
                size: [3.0, 12.0, 4.0],
                uv: [40.0, 16.0],
                inflate: 0.0,
                mirror: false,
            }
        );
        assert_eq!(slim.parts[2].point, [-5.0, 2.5, 0.0]);
        assert_eq!(slim.parts[3].point, [5.0, 2.5, 0.0]);
        // The jacket: the body inflated a quarter texel.
        assert_eq!(
            boxes(wide, 11),
            Box {
                origin: [-4.0, 0.0, -2.0],
                size: [8.0, 12.0, 4.0],
                uv: [16.0, 32.0],
                inflate: 0.25,
                mirror: false,
            }
        );
        // The pant legs and sleeves carry the same quarter-texel inflation.
        assert_eq!(boxes(wide, 7).inflate, 0.25);
        assert_eq!(boxes(wide, 8).inflate, 0.25);
        assert_eq!(boxes(wide, 9).inflate, 0.25);
        assert_eq!(boxes(wide, 10).inflate, 0.25);
    }

    #[test]
    fn the_slim_arms_shift_their_uvs() {
        // The right arm's south face starts where the sleeve table puts it: its U span is
        // the cell's origin shifted by the box's own depth, width and depth again, so the
        // slim box's three-texel width narrows the face and moves its start one texel left.
        let wide = box_vertices(&RIGHT_ARM_WIDE_BOXES[0], PLAYER_TEXTURE_SIZE);
        let slim = box_vertices(&RIGHT_ARM_SLIM_BOXES[0], PLAYER_TEXTURE_SIZE);
        assert_eq!(wide.uvs[20], [56.0 / 64.0, 20.0 / 64.0]);
        assert_eq!(wide.uvs[21], [52.0 / 64.0, 20.0 / 64.0]);
        assert_eq!(slim.uvs[20], [54.0 / 64.0, 20.0 / 64.0]);
        assert_eq!(slim.uvs[21], [51.0 / 64.0, 20.0 / 64.0]);
    }

    #[test]
    fn the_cape_builds_through_its_own_sheet() {
        let vertices = build_vertices(
            &MODEL_PLAYER_CAPE,
            &MODEL_PLAYER_CAPE.rest(),
            CAPE_TEXTURE_SIZE,
        );
        assert_eq!(vertices.positions.len(), 24);
        // The cape's south face reads the bottom row of its 64x32 sheet.
        assert_eq!(vertices.uvs[20], [22.0 / 64.0, 1.0 / 32.0]);
        assert_eq!(vertices.uvs[21], [12.0 / 64.0, 1.0 / 32.0]);
        assert_eq!(vertices.positions[0], [5.0, 0.0, 0.0]);
    }

    #[test]
    fn the_pose_follows_the_swing_grid() {
        let mut rots = MODEL_PLAYER_WIDE.rest();
        let pose = Pose {
            swing_progress: 1.0 / 6.0,
            extra: PoseExtra::Player(CapeMotion::default()),
            ..Pose::default()
        };
        super::pose(&pose, PARTS_ALL, &mut rots);
        // The body turns with the swing, and the arms' pivots follow the body.
        assert!((rots[1].angles[1] - 0.109_017_42).abs() < 1.0e-5);
        assert!((rots[2].point[0] - -4.970_317).abs() < 1.0e-5);
        assert!((rots[2].point[2] - 0.544_008).abs() < 1.0e-5);
        assert!((rots[3].point[0] - 4.970_317).abs() < 1.0e-5);
        assert!((rots[3].point[2] - -0.544_008).abs() < 1.0e-5);
        // The right arm carries the swing: the body's turn three times over, the eased
        // lift and the head dip on x, and the roll on z.
        assert!((rots[2].angles[1] - 0.327_052).abs() < 1.0e-5);
        assert!((rots[2].angles[0] - -1.460_635).abs() < 1.0e-5);
        assert!((rots[2].angles[2] - -0.1).abs() < 1.0e-5);
        // The left arm take the body's turn once, on y and on x.
        assert!((rots[3].angles[1] - 0.109_017).abs() < 1.0e-5);
        assert!((rots[3].angles[0] - 0.109_017).abs() < 1.0e-5);
        assert!((rots[3].angles[2] - -0.1).abs() < 1.0e-5);
    }

    #[test]
    fn the_pose_sways_with_the_walk_and_the_age() {
        let mut rots = MODEL_PLAYER_WIDE.rest();
        let pose = Pose {
            limb_swing: 1.0,
            limb_swing_amount: 1.0,
            age: 20.0,
            extra: PoseExtra::Player(CapeMotion::default()),
            ..Pose::default()
        };
        super::pose(&pose, PARTS_ALL, &mut rots);
        // The arms in opposite phase, the legs in opposite phase again, all slid by the
        // age's idle terms.
        assert!((rots[2].angles[0] - -0.737_502).abs() < 1.0e-4);
        assert!((rots[3].angles[0] - 0.737_502).abs() < 1.0e-4);
        assert!((rots[4].angles[0] - 1.100_646).abs() < 1.0e-4);
        assert!((rots[5].angles[0] - -1.100_646).abs() < 1.0e-4);
        assert!((rots[2].angles[2] - 0.038_640).abs() < 1.0e-4);
        assert!((rots[3].angles[2] - -0.038_640).abs() < 1.0e-4);
        // At rest the legs' pivots sit a tenth back at their standing height, the head's at
        // zero and the cape's hanging.
        assert_eq!(rots[4].point, [-1.9, 12.0, 0.1]);
        assert_eq!(rots[5].point, [1.9, 12.0, 0.1]);
        assert_eq!(rots[0].point, [0.0, 0.0, 0.0]);
        assert_eq!(rots[12].point, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_head_angles_are_written_as_they_arrive() {
        let mut rots = MODEL_PLAYER_WIDE.rest();
        let pose = Pose {
            head_yaw: 90.0,
            head_pitch: 45.0,
            extra: PoseExtra::Player(CapeMotion::default()),
            ..Pose::default()
        };
        super::pose(&pose, PARTS_ALL, &mut rots);
        // No clamp: the model takes the net head yaw and the pitch as the frame hands them
        // over (`ModelBiped.setRotationAngles`).
        assert!(close(rots[0].angles, [PI / 4.0, PI / 2.0, 0.0]));
    }

    #[test]
    fn the_sneak_pose_leans_and_lifts() {
        let mut rots = MODEL_PLAYER_WIDE.rest();
        let pose = Pose {
            sneak: true,
            limb_swing: 1.0,
            limb_swing_amount: 1.0,
            extra: PoseExtra::Player(CapeMotion::default()),
            ..Pose::default()
        };
        super::pose(&pose, PARTS_ALL, &mut rots);
        // The body leans half a radian, the legs draw in and up, the head rises a texel and
        // the cape's pivot takes its two-texel lift.
        assert_eq!(rots[1].angles[0], 0.5);
        assert!(
            (rots[2].angles[0] - (-0.786_175_7 + 0.4)).abs() < 1.0e-4,
            "right arm x: {:?}",
            rots[2].angles
        );
        assert!(
            (rots[3].angles[0] - (0.786_175_7 + 0.4)).abs() < 1.0e-4,
            "left arm x: {:?}",
            rots[3].angles
        );
        assert_eq!(rots[4].point[2], 4.0);
        assert_eq!(rots[5].point[2], 4.0);
        assert_eq!(rots[4].point[1], 9.0);
        assert_eq!(rots[5].point[1], 9.0);
        assert_eq!(rots[0].point[1], 1.0);
        assert_eq!(rots[12].point[1], 2.0);
        assert_eq!(cape_rot(&pose, PARTS_ALL).point[1], 2.0);
    }

    #[test]
    fn the_overlays_copy_their_base_parts_and_the_byte_gates_them() {
        let mut rots = MODEL_PLAYER_WIDE.rest();
        // The wide right sleeve's rest pivot carries the source's own ten-texel z, and the
        // pose washes it away with the copy from its arm.
        assert_eq!(rots[10].point, [-5.0, 2.0, 10.0]);
        let pose = rest_pose();
        super::pose(&pose, PARTS_ALL, &mut rots);
        assert_eq!(rots[6].angles, rots[0].angles);
        assert_eq!(rots[6].point, rots[0].point);
        assert_eq!(rots[9].angles, rots[3].angles);
        assert_eq!(rots[10].point, [-5.0, 2.0, 0.0]);
        assert_eq!(rots[11].angles, rots[1].angles);
        for slot in [0, 1, 2, 3, 4, 5] {
            assert!(rots[slot].visible);
        }
        for rot in &rots[6..PART_COUNT] {
            assert!(rot.visible);
        }
        // With the byte cleared only the overlays and the cape stop drawing.
        let mut bare = MODEL_PLAYER_WIDE.rest();
        super::pose(&pose, 0, &mut bare);
        for slot in [6, 7, 8, 9, 10, 11, 12] {
            assert!(!bare[slot].visible);
        }
        for slot in [0, 1, 2, 3, 4, 5] {
            assert!(bare[slot].visible);
        }
    }

    #[test]
    fn the_cape_rule_reads_two_frames() {
        let pose = rest_pose();
        // Hanging at rest: the base pitch, the layer's own yaw, no roll.
        assert_eq!(cape_rotation(&pose, [0.0, 0.0, 0.0]), [6.0, 180.0, 0.0]);
        // Struck sideways by a quarter block: the roll turns the yaw off its rest and tips
        // the box by half of it.
        assert_eq!(cape_rotation(&pose, [-0.25, 0.0, 0.0]), [6.0, 167.5, 12.5]);
        // Falling with the sneak lift: the pitch clamps its drop and takes the lift.
        let sneak = Pose {
            sneak: true,
            ..rest_pose()
        };
        assert_eq!(cape_rotation(&sneak, [0.0, -2.0, 0.0]), [25.0, 180.0, 0.0]);
    }
}
