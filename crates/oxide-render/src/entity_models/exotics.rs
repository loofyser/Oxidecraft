//! The large and the exotic mob families: the horse, the wolf, the ocelot, the rabbit, the
//! ghast, the blaze, the guardian, the ender dragon and the wither — their tables, their own
//! poses and their variant sheets.
//!
//! Every table is its model class's constructor in the constructor's own order
//! (`ModelHorse.java`:65-205, `ModelWolf.java`:36-66, `ModelOcelot.java`:36-70,
//! `ModelRabbit.java`:49-115, `ModelGhast.java`:13-32, `ModelBlaze.java`:12-22,
//! `ModelGuardian.java`:16-49, `ModelDragon.java`:47-124, `ModelWither.java`:13-38), with the
//! class's own rest rotations where the constructor sets them. The horse carries both ear
//! pairs and its render picks one per type (`ModelHorse.java`:142-149,`:294-303); the
//! rabbit's whole model mirrors (`ModelRabbit.java`:55-114, every box); the dragon carries
//! one wing, one front leg and one rear leg per side, its second side the mirror its renderer
//! draws with `scale(-1, 1, 1)` (`ModelDragon.java`:206).
//!
//! The poses are the classes' own arithmetic. The horse walks its own leg pair with the swing
//! against its neighbours (`ModelHorse.setLivingAnimations`:353-572, and it writes its whole
//! pose there — the class overrides no `setRotationAngles`); the wolf's pose steps through
//! its sitting branch (`ModelWolf.setLivingAnimations`:112-163), its tail turning by the
//! entity's health through the renderer's own rotation float (`RenderWolf.handleRotationFloat`:24-27,
//! `EntityWolf.getTailRotation`:493-496); the ocelot's states turn its body and legs
//! (`ModelOcelot.setRotationAngles`:117-151, `setLivingAnimations`:157-218); the rabbit's
//! legs ride the hop (`ModelRabbit.setRotationAngles`:176-188); the ghast's tentacles sway on
//! the age (`ModelGhast.setRotationAngles`:39-45); the blaze's rods orbit its head by
//! rotation point, never an angle (`ModelBlaze.setRotationAngles`:43-77); the guardian's
//! spines and tail ride their own counters (`ModelGuardian.setRotationAngles`:70-135); the
//! dragon's wings flap on the interpolated `animTime` — advanced from the frame's age at
//! the at-rest rate — its whole model riding the flight's own translate and pitch; its
//! neck and tail a chain of ten-unit steps (`ModelDragon.render`:138-241); the wither's
//! rib cage swings its middle rib with the age and drops its third from the second
//! (`ModelWither.setRotationAngles`:64-72).
//!
//! The frames carry less than the classes read, and each pose pins the rest where a frame
//! input is missing: the horse's eating, rearing and mouth fractions pin off (the head keeps
//! its table lean); the wolf's feeding interest and wet-shake angles pin off; the ocelot's
//! sprint state bounds to sitting or resting; the rabbit's hop progress is carried; the
//! guardian's spine extension and tail phase are carried; the dragon's movement ring holds
//! at zero, so its chain is the resting one and its corpse lean zero — its corpse path the
//! ring's own turn through that rest, the yaw turn and the pitch lean both pinned (no
//! `180 - body_yaw`; `RenderDragon.rotateCorpse`:33-39 replaces it), with the block-back
//! step — its flight translate and pitch riding the model's own level, ahead of every part
//! (`ModelDragon.render`:144-147), and its flight clock advancing the frame's age at the
//! at-rest rate, a fifth of a tick (`EntityDragon.onLivingUpdate`:158-167; the slowed
//! flag's halving, the motion scale and the AI-disabled `0.5` lock are not carried); the
//! wither's side heads pin upright.

use super::{Box, Model, Part, Pose, PoseExtra, Rot};
use std::f32::consts::{FRAC_PI_6, PI};

/// The horse's part slots, in the depth-first order of the tables below: the head with its
/// two muzzle children, the neck, the four ears, the body, the three tail parts, the mane,
/// the twelve leg parts, the two mule chests, the face ropes, the two face metals, the two
/// reins and the seven saddle parts (`ModelHorse.java`:65-205).
pub mod horse {
    /// The head; its two muzzle boxes are children and take the slots after it.
    pub const HEAD: usize = 0;
    /// The upper muzzle box (`ModelHorse.java`:121-125).
    pub const MUZZLE_UPPER: usize = 1;
    /// The lower muzzle box (`ModelHorse.java`:126-130).
    pub const MUZZLE_LOWER: usize = 2;
    /// The neck (`ModelHorse.java`:150-153).
    pub const NECK: usize = 3;
    /// The horse's left ear (`ModelHorse.java`:134-137).
    pub const HORSE_LEFT_EAR: usize = 4;
    /// The horse's right ear (`ModelHorse.java`:138-141).
    pub const HORSE_RIGHT_EAR: usize = 5;
    /// The mule's left ear (`ModelHorse.java`:142-145).
    pub const MULE_LEFT_EAR: usize = 6;
    /// The mule's right ear (`ModelHorse.java`:146-149).
    pub const MULE_RIGHT_EAR: usize = 7;
    /// The body (`ModelHorse.java`:69-71).
    pub const BODY: usize = 8;
    /// The tail's base (`ModelHorse.java`:72-75).
    pub const TAIL_BASE: usize = 9;
    /// The tail's middle (`ModelHorse.java`:76-79).
    pub const TAIL_MIDDLE: usize = 10;
    /// The tail's tip (`ModelHorse.java`:80-83).
    pub const TAIL_TIP: usize = 11;
    /// The mane (`ModelHorse.java`:197-200).
    pub const MANE: usize = 12;
    /// The back left leg and its shin and hoof children (`ModelHorse.java`:84-100).
    pub const BACK_LEFT_LEG: usize = 13;
    /// The back left shin.
    pub const BACK_LEFT_SHIN: usize = 14;
    /// The back left hoof.
    pub const BACK_LEFT_HOOF: usize = 15;
    /// The back right leg and its children.
    pub const BACK_RIGHT_LEG: usize = 16;
    /// The back right shin.
    pub const BACK_RIGHT_SHIN: usize = 17;
    /// The back right hoof.
    pub const BACK_RIGHT_HOOF: usize = 18;
    /// The front left leg and its children (`ModelHorse.java`:101-109).
    pub const FRONT_LEFT_LEG: usize = 19;
    /// The front left shin.
    pub const FRONT_LEFT_SHIN: usize = 20;
    /// The front left hoof.
    pub const FRONT_LEFT_HOOF: usize = 21;
    /// The front right leg and its children.
    pub const FRONT_RIGHT_LEG: usize = 22;
    /// The front right shin.
    pub const FRONT_RIGHT_SHIN: usize = 23;
    /// The front right hoof.
    pub const FRONT_RIGHT_HOOF: usize = 24;
    /// The mule's left chest (`ModelHorse.java`:154-157).
    pub const MULE_LEFT_CHEST: usize = 25;
    /// The mule's right chest (`ModelHorse.java`:158-161).
    pub const MULE_RIGHT_CHEST: usize = 26;
    /// The face ropes (`ModelHorse.java`:201-204).
    pub const FACE_ROPES: usize = 27;
    /// The left face metal (`ModelHorse.java`:183-186).
    pub const LEFT_FACE_METAL: usize = 28;
    /// The right face metal (`ModelHorse.java`:187-190).
    pub const RIGHT_FACE_METAL: usize = 29;
    /// The left rein (`ModelHorse.java`:191-193).
    pub const LEFT_REIN: usize = 30;
    /// The right rein (`ModelHorse.java`:194-196).
    pub const RIGHT_REIN: usize = 31;
    /// The saddle's bottom (`ModelHorse.java`:162-164).
    pub const SADDLE_BOTTOM: usize = 32;
    /// The saddle's front (`ModelHorse.java`:165-167).
    pub const SADDLE_FRONT: usize = 33;
    /// The saddle's back (`ModelHorse.java`:168-170).
    pub const SADDLE_BACK: usize = 34;
    /// The left saddle metal (`ModelHorse.java`:171-173).
    pub const LEFT_SADDLE_METAL: usize = 35;
    /// The left saddle rope (`ModelHorse.java`:174-176).
    pub const LEFT_SADDLE_ROPE: usize = 36;
    /// The right saddle metal (`ModelHorse.java`:177-179).
    pub const RIGHT_SADDLE_METAL: usize = 37;
    /// The right saddle rope (`ModelHorse.java`:180-182).
    pub const RIGHT_SADDLE_ROPE: usize = 38;
}

/// The wolf's part slots (`ModelWolf.java`:36-66).
pub mod wolf {
    /// The head, carrying its ear and muzzle boxes (`ModelWolf.java`:40-41,`:63-65`).
    pub const HEAD: usize = 0;
    /// The body (`ModelWolf.java`:43-44).
    pub const BODY: usize = 1;
    /// The mane (`ModelWolf.java`:46-47).
    pub const MANE: usize = 2;
    /// The first leg.
    pub const LEG1: usize = 3;
    /// The second leg.
    pub const LEG2: usize = 4;
    /// The third leg.
    pub const LEG3: usize = 5;
    /// The fourth leg.
    pub const LEG4: usize = 6;
    /// The tail (`ModelWolf.java`:61-62).
    pub const TAIL: usize = 7;
}

/// The ocelot's part slots, in the render's own order (`ModelOcelot.render`:101-108).
pub mod ocelot {
    /// The head, carrying its nose and two ear boxes (`ModelOcelot.java`:42-47).
    pub const HEAD: usize = 0;
    /// The body (`ModelOcelot.java`:48-50).
    pub const BODY: usize = 1;
    /// The tail (`ModelOcelot.java`:51-54).
    pub const TAIL: usize = 2;
    /// The tail's second part (`ModelOcelot.java`:55-57).
    pub const TAIL2: usize = 3;
    /// The back left leg (`ModelOcelot.java`:58-60).
    pub const BACK_LEFT_LEG: usize = 4;
    /// The back right leg (`ModelOcelot.java`:61-63).
    pub const BACK_RIGHT_LEG: usize = 5;
    /// The front left leg (`ModelOcelot.java`:64-66).
    pub const FRONT_LEFT_LEG: usize = 6;
    /// The front right leg (`ModelOcelot.java`:67-69).
    pub const FRONT_RIGHT_LEG: usize = 7;
}

/// The rabbit's part slots, in the render's own order (`ModelRabbit.render`:156-167).
pub mod rabbit {
    /// The left foot (`ModelRabbit.java`:55-59).
    pub const LEFT_FOOT: usize = 0;
    /// The right foot (`ModelRabbit.java`:60-64).
    pub const RIGHT_FOOT: usize = 1;
    /// The left thigh (`ModelRabbit.java`:65-69).
    pub const LEFT_THIGH: usize = 2;
    /// The right thigh (`ModelRabbit.java`:70-74).
    pub const RIGHT_THIGH: usize = 3;
    /// The body (`ModelRabbit.java`:75-79).
    pub const BODY: usize = 4;
    /// The left arm (`ModelRabbit.java`:80-84).
    pub const LEFT_ARM: usize = 5;
    /// The right arm (`ModelRabbit.java`:85-89).
    pub const RIGHT_ARM: usize = 6;
    /// The head (`ModelRabbit.java`:90-94).
    pub const HEAD: usize = 7;
    /// The right ear (`ModelRabbit.java`:95-99).
    pub const RIGHT_EAR: usize = 8;
    /// The left ear (`ModelRabbit.java`:100-104).
    pub const LEFT_EAR: usize = 9;
    /// The tail (`ModelRabbit.java`:105-109).
    pub const TAIL: usize = 10;
    /// The nose (`ModelRabbit.java`:110-114).
    pub const NOSE: usize = 11;
}

/// The ghast's part slots: the body first, then the nine tentacles
/// (`ModelGhast.java`:13-32, the render's own order, `ModelGhast.render`:50-63).
pub mod ghast {
    /// The body.
    pub const BODY: usize = 0;
    /// The tentacles' first slot; the nine follow in order.
    pub const TENTACLE_0: usize = 1;
}

/// The blaze's part slots: the head first, then the twelve rods
/// (`ModelBlaze.java`:12-22, the render's own order, `ModelBlaze.render`:27-36).
pub mod blaze {
    /// The head.
    pub const HEAD: usize = 0;
    /// The rods' first slot; the twelve follow in order.
    pub const ROD_0: usize = 1;
}

/// The guardian's part slots: one root, the body, with the twelve spines, the eye and the
/// tail's three-link chain hanging under it (`ModelGuardian.java`:16-49).
pub mod guardian {
    /// The body, the table's one root part.
    pub const BODY: usize = 0;
    /// The spines' first slot; the twelve follow in order.
    pub const SPINE_0: usize = 1;
    /// The eye (`ModelGuardian.java`:35-37).
    pub const EYE: usize = 13;
    /// The tail's first part, a child of the body.
    pub const TAIL_0: usize = 14;
    /// The tail's second part, a child of the first.
    pub const TAIL_1: usize = 15;
    /// The tail's third part, a child of the second.
    pub const TAIL_2: usize = 16;
}

/// The dragon's part slots, in the render's own order (`ModelDragon.render`:159-238).
pub mod dragon {
    /// The head; the jaw is its child and takes the slot after it
    /// (`ModelDragon.java`:71-83).
    pub const HEAD: usize = 0;
    /// The jaw (`ModelDragon.java`:80-82).
    pub const JAW: usize = 1;
    /// The five neck spines; the first slot, the rest following
    /// (`ModelDragon.java`:84-86, drawn five times at `:159-173`).
    pub const NECK_0: usize = 2;
    /// The body (`ModelDragon.java`:87-92).
    pub const BODY: usize = 7;
    /// The left wing, its tip a child (`ModelDragon.java`:93-101).
    pub const WING_LEFT: usize = 8;
    /// The left wing's tip (`ModelDragon.java`:97-101).
    pub const WING_LEFT_TIP: usize = 9;
    /// The right wing, the left's own mirror (`ModelDragon.java`:206).
    pub const WING_RIGHT: usize = 10;
    /// The right wing's tip.
    pub const WING_RIGHT_TIP: usize = 11;
    /// The left front leg, its tip and foot children (`ModelDragon.java`:102-112).
    pub const FRONT_LEFT_LEG: usize = 12;
    /// The left front leg's tip.
    pub const FRONT_LEFT_TIP: usize = 13;
    /// The left front leg's foot.
    pub const FRONT_LEFT_FOOT: usize = 14;
    /// The right front leg.
    pub const FRONT_RIGHT_LEG: usize = 15;
    /// The right front leg's tip.
    pub const FRONT_RIGHT_TIP: usize = 16;
    /// The right front leg's foot.
    pub const FRONT_RIGHT_FOOT: usize = 17;
    /// The left rear leg, its tip and foot children (`ModelDragon.java`:113-123).
    pub const REAR_LEFT_LEG: usize = 18;
    /// The left rear leg's tip.
    pub const REAR_LEFT_TIP: usize = 19;
    /// The left rear leg's foot.
    pub const REAR_LEFT_FOOT: usize = 20;
    /// The right rear leg.
    pub const REAR_RIGHT_LEG: usize = 21;
    /// The right rear leg's tip.
    pub const REAR_RIGHT_TIP: usize = 22;
    /// The right rear leg's foot.
    pub const REAR_RIGHT_FOOT: usize = 23;
    /// The twelve tail spines; the first slot, the rest following
    /// (`ModelDragon.java`:84-86, drawn twelve times at `:224-238`).
    pub const TAIL_0: usize = 24;
}

/// The wither's part slots, in the render's own order (`ModelWither.render`:44-57: the three
/// heads, then the three rib cage parts).
pub mod wither {
    /// The centre head (`ModelWither.java`:29-30).
    pub const HEAD_CENTRE: usize = 0;
    /// The left head (`ModelWither.java`:31-34).
    pub const HEAD_LEFT: usize = 1;
    /// The right head (`ModelWither.java`:35-38).
    pub const HEAD_RIGHT: usize = 2;
    /// The rib cage's first part (`ModelWither.java`:18-19).
    pub const RIB_0: usize = 3;
    /// The rib cage's middle (`ModelWither.java`:20-25).
    pub const RIB_1: usize = 4;
    /// The rib cage's last part (`ModelWither.java`:26-27).
    pub const RIB_2: usize = 5;
}

/// A part drawing nothing, the stand-in the tables are filled from.
#[cfg(test)]
const ZERO: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[],
    children: &[],
};

// The horse's table: the head with its two muzzle children, the neck, four ears, the body,
// three tail parts, the mane, twelve leg parts, two mule chests, the face ropes, two face
// metals, two reins and the seven saddle parts — thirty-nine boxes (`ModelHorse.java`:65-205).
static HORSE_PARTS: [Part; 29] = [
    // The head, its face, muzzle and jaw boxes merged onto the part the render turns
    // (`ModelHorse.java`:119-131).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-2.5, -10.0, -1.5],
                size: [5.0, 5.0, 7.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-2.0, -10.0, -7.0],
                size: [4.0, 3.0, 6.0],
                uv: [24.0, 18.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-2.0, -7.0, -6.5],
                size: [4.0, 2.0, 5.0],
                uv: [24.0, 27.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[
            // The upper muzzle (`ModelHorse.java`:121-125), an empty part the pose slides.
            Part {
                point: [0.0, 3.95, -10.0],
                rest: [FRAC_PI_6, 0.0, 0.0],
                boxes: &[],
                children: &[],
            },
            // The lower muzzle (`ModelHorse.java`:126-130).
            Part {
                point: [0.0, 4.0, -10.0],
                rest: [FRAC_PI_6, 0.0, 0.0],
                boxes: &[],
                children: &[],
            },
        ],
    },
    // The neck (`ModelHorse.java`:150-153).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.05, -9.8, -2.0],
            size: [4.0, 14.0, 8.0],
            uv: [0.0, 12.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The horse's left ear (`ModelHorse.java`:134-137).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.45, -12.0, 4.0],
            size: [2.0, 3.0, 1.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The horse's right ear (`ModelHorse.java`:138-141).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.45, -12.0, 4.0],
            size: [2.0, 3.0, 1.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The mule's left ear (`ModelHorse.java`:142-145).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.261_799_4],
        boxes: &[Box {
            origin: [-2.0, -16.0, 4.0],
            size: [2.0, 7.0, 1.0],
            uv: [0.0, 12.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The mule's right ear (`ModelHorse.java`:146-149).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, -0.261_799_4],
        boxes: &[Box {
            origin: [0.0, -16.0, 4.0],
            size: [2.0, 7.0, 1.0],
            uv: [0.0, 12.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The body (`ModelHorse.java`:69-71).
    Part {
        point: [0.0, 11.0, 9.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-5.0, -8.0, -19.0],
            size: [10.0, 10.0, 24.0],
            uv: [0.0, 34.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The tail's base (`ModelHorse.java`:72-75).
    Part {
        point: [0.0, 3.0, 14.0],
        rest: [-1.134_464, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, -1.0, 0.0],
            size: [2.0, 2.0, 3.0],
            uv: [44.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The tail's middle (`ModelHorse.java`:76-79).
    Part {
        point: [0.0, 3.0, 14.0],
        rest: [-1.134_464, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, -2.0, 3.0],
            size: [3.0, 4.0, 7.0],
            uv: [38.0, 7.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The tail's tip (`ModelHorse.java`:80-83).
    Part {
        point: [0.0, 3.0, 14.0],
        rest: [-1.402_15, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, -4.5, 9.0],
            size: [3.0, 4.0, 7.0],
            uv: [24.0, 3.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The mane (`ModelHorse.java`:197-200).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, -11.5, 5.0],
            size: [2.0, 16.0, 4.0],
            uv: [58.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The back left leg with its shin and hoof (`ModelHorse.java`:84-91).
    Part {
        point: [4.0, 9.0, 11.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.5, -2.0, -2.5],
            size: [4.0, 9.0, 5.0],
            uv: [78.0, 29.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [4.0, 16.0, 11.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-2.0, 0.0, -1.5],
                size: [3.0, 5.0, 3.0],
                uv: [78.0, 43.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [4.0, 16.0, 11.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-2.5, 5.1, -2.0],
                    size: [4.0, 3.0, 4.0],
                    uv: [78.0, 51.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The back right leg with its shin and hoof (`ModelHorse.java`:92-100).
    Part {
        point: [-4.0, 9.0, 11.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, -2.0, -2.5],
            size: [4.0, 9.0, 5.0],
            uv: [96.0, 29.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [-4.0, 16.0, 11.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-1.0, 0.0, -1.5],
                size: [3.0, 5.0, 3.0],
                uv: [96.0, 43.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [-4.0, 16.0, 11.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.5, 5.1, -2.0],
                    size: [4.0, 3.0, 4.0],
                    uv: [96.0, 51.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The front left leg with its shin and hoof (`ModelHorse.java`:101-109).
    Part {
        point: [4.0, 9.0, -8.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.9, -1.0, -2.1],
            size: [3.0, 8.0, 4.0],
            uv: [44.0, 29.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [4.0, 16.0, -8.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-1.9, 0.0, -1.6],
                size: [3.0, 5.0, 3.0],
                uv: [44.0, 41.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [4.0, 16.0, -8.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-2.4, 5.1, -2.1],
                    size: [4.0, 3.0, 4.0],
                    uv: [44.0, 51.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The front right leg with its shin and hoof (`ModelHorse.java`:110-118).
    Part {
        point: [-4.0, 9.0, -8.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.1, -1.0, -2.1],
            size: [3.0, 8.0, 4.0],
            uv: [60.0, 29.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [-4.0, 16.0, -8.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-1.1, 0.0, -1.6],
                size: [3.0, 5.0, 3.0],
                uv: [60.0, 41.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [-4.0, 16.0, -8.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.6, 5.1, -2.1],
                    size: [4.0, 3.0, 4.0],
                    uv: [60.0, 51.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The mule's left chest (`ModelHorse.java`:154-157).
    Part {
        point: [-7.5, 3.0, 10.0],
        rest: [0.0, 1.570_796_4, 0.0],
        boxes: &[Box {
            origin: [-3.0, 0.0, 0.0],
            size: [8.0, 8.0, 3.0],
            uv: [0.0, 34.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The mule's right chest (`ModelHorse.java`:158-161).
    Part {
        point: [4.5, 3.0, 10.0],
        rest: [0.0, 1.570_796_4, 0.0],
        boxes: &[Box {
            origin: [-3.0, 0.0, 0.0],
            size: [8.0, 8.0, 3.0],
            uv: [0.0, 47.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The face ropes (`ModelHorse.java`:201-204), inflated a fifth of a texel.
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.5, -10.1, -7.0],
            size: [5.0, 5.0, 12.0],
            uv: [80.0, 12.0],
            inflate: 0.2,
            mirror: false,
        }],
        children: &[],
    },
    // The left face metal (`ModelHorse.java`:183-186).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [1.5, -8.0, -4.0],
            size: [1.0, 2.0, 2.0],
            uv: [74.0, 13.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The right face metal (`ModelHorse.java`:187-190).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [FRAC_PI_6, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.5, -8.0, -4.0],
            size: [1.0, 2.0, 2.0],
            uv: [74.0, 13.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The left rein (`ModelHorse.java`:191-193`), a zero-width strip.
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [2.6, -6.0, -6.0],
            size: [0.0, 3.0, 16.0],
            uv: [44.0, 10.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The right rein (`ModelHorse.java`:194-196`).
    Part {
        point: [0.0, 4.0, -10.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.6, -6.0, -6.0],
            size: [0.0, 3.0, 16.0],
            uv: [44.0, 5.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The saddle's bottom (`ModelHorse.java`:162-164).
    Part {
        point: [0.0, 2.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-5.0, 0.0, -3.0],
            size: [10.0, 1.0, 8.0],
            uv: [80.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The saddle's front (`ModelHorse.java`:165-167).
    Part {
        point: [0.0, 2.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, -1.0, -3.0],
            size: [3.0, 1.0, 2.0],
            uv: [106.0, 9.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The saddle's back (`ModelHorse.java`:168-170).
    Part {
        point: [0.0, 2.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -1.0, 3.0],
            size: [8.0, 1.0, 2.0],
            uv: [80.0, 9.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The left saddle metal (`ModelHorse.java`:171-173).
    Part {
        point: [5.0, 3.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 6.0, -1.0],
            size: [1.0, 2.0, 2.0],
            uv: [74.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The left saddle rope (`ModelHorse.java`:174-176).
    Part {
        point: [5.0, 3.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -0.5],
            size: [1.0, 6.0, 1.0],
            uv: [70.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The right saddle metal (`ModelHorse.java`:177-179).
    Part {
        point: [-5.0, 3.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 6.0, -1.0],
            size: [1.0, 2.0, 2.0],
            uv: [74.0, 4.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The right saddle rope (`ModelHorse.java`:180-182).
    Part {
        point: [-5.0, 3.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -0.5],
            size: [1.0, 6.0, 1.0],
            uv: [80.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The horse's model (`ModelHorse`), texture `128` by `128` (`ModelHorse.java`:67-68).
pub static MODEL_HORSE: Model = Model {
    parts: &HORSE_PARTS,
};

/// The wolf's model: `ModelWolf`, texture `64` by `32` (`ModelBase`'s default).
static WOLF_PARTS: [Part; 8] = [
    // The head with its ears and muzzle (`ModelWolf.java`:40-41,`:63-65`).
    Part {
        point: [-1.0, 13.5, -7.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-3.0, -3.0, -2.0],
                size: [6.0, 6.0, 4.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-3.0, -5.0, 0.0],
                size: [2.0, 2.0, 1.0],
                uv: [16.0, 14.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [1.0, -5.0, 0.0],
                size: [2.0, 2.0, 1.0],
                uv: [16.0, 14.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.5, 0.0, -5.0],
                size: [3.0, 3.0, 4.0],
                uv: [0.0, 10.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    // The body (`ModelWolf.java`:43-44`), six wide off its pivot.
    Part {
        point: [0.0, 14.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -2.0, -3.0],
            size: [6.0, 9.0, 6.0],
            uv: [18.0, 14.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The mane (`ModelWolf.java`:46-47`).
    Part {
        point: [-1.0, 14.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -3.0, -3.0],
            size: [8.0, 6.0, 7.0],
            uv: [21.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The four legs (`ModelWolf.java`:48-59`).
    Part {
        point: [-2.5, 16.0, 7.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.5, 16.0, 7.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [-2.5, 16.0, -4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.5, 16.0, -4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The tail (`ModelWolf.java`:61-62`).
    Part {
        point: [-1.0, 12.0, 8.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 8.0, 2.0],
            uv: [9.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The wolf's model.
pub static MODEL_WOLF: Model = Model { parts: &WOLF_PARTS };

/// The ocelot's model: `ModelOcelot` (`ModelOcelot.java`:36-70).
static OCELOT_PARTS: [Part; 8] = [
    // The head with its nose and two ears (`ModelOcelot.java`:42-47`).
    Part {
        point: [0.0, 15.0, -9.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-2.5, -2.0, -3.0],
                size: [5.0, 4.0, 5.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.5, 0.0, -4.0],
                size: [3.0, 2.0, 2.0],
                uv: [0.0, 24.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-2.0, -3.0, 0.0],
                size: [1.0, 1.0, 2.0],
                uv: [0.0, 10.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [1.0, -3.0, 0.0],
                size: [1.0, 1.0, 2.0],
                uv: [6.0, 10.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    // The body (`ModelOcelot.java`:48-50`), hanging below its pivot.
    Part {
        point: [0.0, 12.0, -10.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.0, 3.0, -8.0],
            size: [4.0, 16.0, 6.0],
            uv: [20.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The tail (`ModelOcelot.java`:51-54`).
    Part {
        point: [0.0, 15.0, 8.0],
        rest: [0.9, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, 0.0],
            size: [1.0, 8.0, 1.0],
            uv: [0.0, 15.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The tail's second part (`ModelOcelot.java`:55-57`).
    Part {
        point: [0.0, 20.0, 14.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, 0.0],
            size: [1.0, 8.0, 1.0],
            uv: [4.0, 15.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The back left leg (`ModelOcelot.java`:58-60`).
    Part {
        point: [1.1, 18.0, 5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, 1.0],
            size: [2.0, 6.0, 2.0],
            uv: [8.0, 13.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The back right leg (`ModelOcelot.java`:61-63`).
    Part {
        point: [-1.1, 18.0, 5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, 1.0],
            size: [2.0, 6.0, 2.0],
            uv: [8.0, 13.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The front left leg (`ModelOcelot.java`:64-66`).
    Part {
        point: [1.2, 13.8, -5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, 0.0],
            size: [2.0, 10.0, 2.0],
            uv: [40.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The front right leg (`ModelOcelot.java`:67-69`).
    Part {
        point: [-1.2, 13.8, -5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, 0.0],
            size: [2.0, 10.0, 2.0],
            uv: [40.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The ocelot's model.
pub static MODEL_OCELOT: Model = Model {
    parts: &OCELOT_PARTS,
};

/// The rabbit's model: `ModelRabbit` (`ModelRabbit.java`:49-115`), every box mirrored.
static RABBIT_PARTS: [Part; 12] = [
    // The left foot (`ModelRabbit.java`:55-59`).
    Part {
        point: [3.0, 17.5, 3.7],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 5.5, -3.7],
            size: [2.0, 1.0, 7.0],
            uv: [26.0, 24.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The right foot (`ModelRabbit.java`:60-64`).
    Part {
        point: [-3.0, 17.5, 3.7],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 5.5, -3.7],
            size: [2.0, 1.0, 7.0],
            uv: [8.0, 24.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The left thigh (`ModelRabbit.java`:65-69`).
    Part {
        point: [3.0, 17.5, 3.7],
        rest: [-0.349_065_84, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, 0.0],
            size: [2.0, 4.0, 5.0],
            uv: [30.0, 15.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The right thigh (`ModelRabbit.java`:70-74`).
    Part {
        point: [-3.0, 17.5, 3.7],
        rest: [-0.349_065_84, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, 0.0],
            size: [2.0, 4.0, 5.0],
            uv: [16.0, 15.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The body (`ModelRabbit.java`:75-79`).
    Part {
        point: [0.0, 19.0, 8.0],
        rest: [-0.349_065_84, 0.0, 0.0],
        boxes: &[Box {
            origin: [-3.0, -2.0, -10.0],
            size: [6.0, 5.0, 10.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The left arm (`ModelRabbit.java`:80-84`).
    Part {
        point: [3.0, 17.0, -1.0],
        rest: [-0.174_532_92, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 7.0, 2.0],
            uv: [8.0, 15.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The right arm (`ModelRabbit.java`:85-89`).
    Part {
        point: [-3.0, 17.0, -1.0],
        rest: [-0.174_532_92, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 7.0, 2.0],
            uv: [0.0, 15.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The head (`ModelRabbit.java`:90-94`).
    Part {
        point: [0.0, 16.0, -1.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.5, -4.0, -5.0],
            size: [5.0, 4.0, 5.0],
            uv: [32.0, 0.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The right ear (`ModelRabbit.java`:95-99`).
    Part {
        point: [0.0, 16.0, -1.0],
        rest: [0.0, -0.261_799_4, 0.0],
        boxes: &[Box {
            origin: [-2.5, -9.0, -1.0],
            size: [2.0, 5.0, 1.0],
            uv: [52.0, 0.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The left ear (`ModelRabbit.java`:100-104`).
    Part {
        point: [0.0, 16.0, -1.0],
        rest: [0.0, 0.261_799_4, 0.0],
        boxes: &[Box {
            origin: [0.5, -9.0, -1.0],
            size: [2.0, 5.0, 1.0],
            uv: [58.0, 0.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The tail (`ModelRabbit.java`:105-109`).
    Part {
        point: [0.0, 20.0, 7.0],
        rest: [-0.349_065_9, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, -1.5, 0.0],
            size: [3.0, 3.0, 2.0],
            uv: [52.0, 6.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
    // The nose (`ModelRabbit.java`:110-114`).
    Part {
        point: [0.0, 16.0, -1.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, -2.5, -5.5],
            size: [1.0, 1.0, 1.0],
            uv: [32.0, 9.0],
            inflate: 0.0,
            mirror: true,
        }],
        children: &[],
    },
];

/// The rabbit's model.
pub static MODEL_RABBIT: Model = Model {
    parts: &RABBIT_PARTS,
};

/// The ghast's model: `ModelGhast` (`ModelGhast.java`:13-32), its tentacle lengths the
/// class's own seeded draws (`new Random(1660L)`'s first nine `nextInt(7) + 8`).
static GHAST_PARTS: [Part; 10] = [
    // The body, sixteen under the tentacle ring (`ModelGhast.java`:15-18`).
    Part {
        point: [0.0, 8.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-8.0, -8.0, -8.0],
            size: [16.0, 16.0, 16.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The nine tentacles: the ring spans two units and a half a row, five deep
    // (`ModelGhast.java`:19-30); the lengths are the seeded draws 8, 13, 9, 11, 11, 10, 12, 9, 12.
    Part {
        point: [-3.75, 15.0, -5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [1.25, 15.0, -5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 13.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [6.25, 15.0, -5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 9.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [-6.25, 15.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 11.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [-1.25, 15.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 11.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [3.75, 15.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 10.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [-3.75, 15.0, 5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 12.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [1.25, 15.0, 5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 9.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [6.25, 15.0, 5.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 12.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The ghast's model.
pub static MODEL_GHAST: Model = Model {
    parts: &GHAST_PARTS,
};

/// The blaze's model: `ModelBlaze` (`ModelBlaze.java`:12-22`).
static BLAZE_PARTS: [Part; 13] = [
    // The head (`ModelBlaze.java`:20-21`).
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -4.0, -4.0],
            size: [8.0, 8.0, 8.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The twelve rods, one box and one cell each (`ModelBlaze.java`:14-18`).
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [2.0, 8.0, 2.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The blaze's model.
pub static MODEL_BLAZE: Model = Model {
    parts: &BLAZE_PARTS,
};

/// The guardian's model: `ModelGuardian` (`ModelGuardian.java`:16-49), one root part.
static GUARDIAN_PARTS: [Part; 1] = [
    // The body with its five boxes, the right fin mirrored (`ModelGuardian.java`:22-26`), and
    // the twelve spines, the eye and the tail's chain under it.
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-6.0, 10.0, -8.0],
                size: [12.0, 12.0, 16.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-8.0, 10.0, -6.0],
                size: [2.0, 12.0, 12.0],
                uv: [0.0, 28.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [6.0, 10.0, -6.0],
                size: [2.0, 12.0, 12.0],
                uv: [0.0, 28.0],
                inflate: 0.0,
                mirror: true,
            },
            Box {
                origin: [-6.0, 8.0, -6.0],
                size: [12.0, 2.0, 12.0],
                uv: [16.0, 40.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-6.0, 22.0, -6.0],
                size: [12.0, 2.0, 12.0],
                uv: [16.0, 40.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[
            // The twelve spines, one box each (`ModelGuardian.java`:28-33`).
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, -4.5, -1.0],
                    size: [2.0, 9.0, 2.0],
                    uv: [0.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            // The eye (`ModelGuardian.java`:35-37`).
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-1.0, 15.0, 0.0],
                    size: [2.0, 2.0, 1.0],
                    uv: [8.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            },
            // The tail's first part (`ModelGuardian.java`:38-40`), its links under it.
            Part {
                point: [0.0, 0.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-2.0, 14.0, 7.0],
                    size: [4.0, 4.0, 8.0],
                    uv: [40.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[Part {
                    point: [0.0, 0.0, 0.0],
                    rest: [0.0, 0.0, 0.0],
                    boxes: &[Box {
                        origin: [0.0, 14.0, 0.0],
                        size: [3.0, 3.0, 7.0],
                        uv: [0.0, 54.0],
                        inflate: 0.0,
                        mirror: false,
                    }],
                    children: &[Part {
                        point: [0.0, 0.0, 0.0],
                        rest: [0.0, 0.0, 0.0],
                        boxes: &[
                            Box {
                                origin: [0.0, 14.0, 0.0],
                                size: [2.0, 2.0, 6.0],
                                uv: [41.0, 32.0],
                                inflate: 0.0,
                                mirror: false,
                            },
                            Box {
                                origin: [1.0, 10.5, 3.0],
                                size: [1.0, 9.0, 9.0],
                                uv: [25.0, 19.0],
                                inflate: 0.0,
                                mirror: false,
                            },
                        ],
                        children: &[],
                    }],
                }],
            },
        ],
    },
];

/// The guardian's model.
pub static MODEL_GUARDIAN: Model = Model {
    parts: &GUARDIAN_PARTS,
};

/// The dragon's model: `ModelDragon` (`ModelDragon.java`:47-124). The spine's two boxes
/// draw seventeen times — five under the head and twelve under the body — and each side
/// keeps its own wing and legs, the right side the mirror the renderer draws with
/// `scale(-1, 1, 1)` (`ModelDragon.java`:206`).
static DRAGON_PARTS: [Part; 25] = [
    // The head with its six boxes, the left pair of scales and nostrils mirrored
    // (`ModelDragon.java`:71-79`), the jaw under it.
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-6.0, -1.0, -24.0],
                size: [12.0, 5.0, 16.0],
                uv: [176.0, 44.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-8.0, -8.0, -10.0],
                size: [16.0, 16.0, 16.0],
                uv: [112.0, 30.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-5.0, -12.0, -4.0],
                size: [2.0, 4.0, 6.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: true,
            },
            Box {
                origin: [-5.0, -3.0, -22.0],
                size: [2.0, 2.0, 4.0],
                uv: [112.0, 0.0],
                inflate: 0.0,
                mirror: true,
            },
            Box {
                origin: [3.0, -12.0, -4.0],
                size: [2.0, 4.0, 6.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [3.0, -3.0, -22.0],
                size: [2.0, 2.0, 4.0],
                uv: [112.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[Part {
            point: [0.0, 4.0, -8.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-6.0, 0.0, -16.0],
                size: [12.0, 4.0, 16.0],
                uv: [176.0, 65.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[],
        }],
    },
    // The five neck spines (`ModelDragon.java`:84-86`, drawn five times at `:159-173`); the
    // pose writes the live chain, the seed the chain's head.
    Part {
        point: [0.0, 20.0, -12.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, -12.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, -12.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, -12.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, -12.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    // The body with its three dorsal scales (`ModelDragon.java`:87-92`).
    Part {
        point: [0.0, 4.0, 8.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-12.0, 0.0, -16.0],
                size: [24.0, 24.0, 64.0],
                uv: [0.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -6.0, -10.0],
                size: [2.0, 6.0, 12.0],
                uv: [220.0, 53.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -6.0, 10.0],
                size: [2.0, 6.0, 12.0],
                uv: [220.0, 53.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -6.0, 30.0],
                size: [2.0, 6.0, 12.0],
                uv: [220.0, 53.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    // The left wing, its tip under it (`ModelDragon.java`:93-101`).
    Part {
        point: [-12.0, 5.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-56.0, -4.0, -4.0],
                size: [56.0, 8.0, 8.0],
                uv: [112.0, 88.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-56.0, 0.0, 2.0],
                size: [56.0, 0.0, 56.0],
                uv: [-56.0, 88.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[Part {
            point: [-56.0, 0.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[
                Box {
                    origin: [-56.0, -2.0, -2.0],
                    size: [56.0, 4.0, 4.0],
                    uv: [112.0, 136.0],
                    inflate: 0.0,
                    mirror: false,
                },
                Box {
                    origin: [-56.0, 0.0, 2.0],
                    size: [56.0, 0.0, 56.0],
                    uv: [-56.0, 144.0],
                    inflate: 0.0,
                    mirror: false,
                },
            ],
            children: &[],
        }],
    },
    // The right wing, its own side of the mirror (`ModelDragon.java`:206`).
    Part {
        point: [12.0, 5.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-56.0, -4.0, -4.0],
                size: [56.0, 8.0, 8.0],
                uv: [112.0, 88.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-56.0, 0.0, 2.0],
                size: [56.0, 0.0, 56.0],
                uv: [-56.0, 88.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[Part {
            point: [-56.0, 0.0, 0.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[
                Box {
                    origin: [-56.0, -2.0, -2.0],
                    size: [56.0, 4.0, 4.0],
                    uv: [112.0, 136.0],
                    inflate: 0.0,
                    mirror: false,
                },
                Box {
                    origin: [-56.0, 0.0, 2.0],
                    size: [56.0, 0.0, 56.0],
                    uv: [-56.0, 144.0],
                    inflate: 0.0,
                    mirror: false,
                },
            ],
            children: &[],
        }],
    },
    // The left front leg with its tip and foot (`ModelDragon.java`:102-112`).
    Part {
        point: [-12.0, 20.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -4.0, -4.0],
            size: [8.0, 24.0, 8.0],
            uv: [112.0, 104.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [0.0, 20.0, -1.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-3.0, -1.0, -3.0],
                size: [6.0, 24.0, 6.0],
                uv: [226.0, 138.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [0.0, 23.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-4.0, 0.0, -12.0],
                    size: [8.0, 4.0, 16.0],
                    uv: [144.0, 104.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The right front leg (`ModelDragon.java`:206`).
    Part {
        point: [12.0, 20.0, 2.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -4.0, -4.0],
            size: [8.0, 24.0, 8.0],
            uv: [112.0, 104.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [0.0, 20.0, -1.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-3.0, -1.0, -3.0],
                size: [6.0, 24.0, 6.0],
                uv: [226.0, 138.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [0.0, 23.0, 0.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-4.0, 0.0, -12.0],
                    size: [8.0, 4.0, 16.0],
                    uv: [144.0, 104.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The left rear leg with its tip and foot (`ModelDragon.java`:113-123`).
    Part {
        point: [-16.0, 16.0, 42.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-8.0, -4.0, -8.0],
            size: [16.0, 32.0, 16.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [0.0, 32.0, -4.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-6.0, -2.0, 0.0],
                size: [12.0, 32.0, 12.0],
                uv: [196.0, 0.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [0.0, 31.0, 4.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-9.0, 0.0, -20.0],
                    size: [18.0, 6.0, 24.0],
                    uv: [112.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The right rear leg (`ModelDragon.java`:206`).
    Part {
        point: [16.0, 16.0, 42.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-8.0, -4.0, -8.0],
            size: [16.0, 32.0, 16.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[Part {
            point: [0.0, 32.0, -4.0],
            rest: [0.0, 0.0, 0.0],
            boxes: &[Box {
                origin: [-6.0, -2.0, 0.0],
                size: [12.0, 32.0, 12.0],
                uv: [196.0, 0.0],
                inflate: 0.0,
                mirror: false,
            }],
            children: &[Part {
                point: [0.0, 31.0, 4.0],
                rest: [0.0, 0.0, 0.0],
                boxes: &[Box {
                    origin: [-9.0, 0.0, -20.0],
                    size: [18.0, 6.0, 24.0],
                    uv: [112.0, 0.0],
                    inflate: 0.0,
                    mirror: false,
                }],
                children: &[],
            }],
        }],
    },
    // The twelve tail spines, the neck's boxes drawn twelve more times (`:224-238`).
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    Part {
        point: [0.0, 10.0, 60.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [-5.0, -5.0, -5.0],
                size: [10.0, 10.0, 10.0],
                uv: [192.0, 104.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-1.0, -9.0, -3.0],
                size: [2.0, 4.0, 6.0],
                uv: [48.0, 0.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
];

/// The dragon's model.
pub static MODEL_DRAGON: Model = Model {
    parts: &DRAGON_PARTS,
};

/// The wither's model: `ModelWither` (`ModelWither.java`:13-38`), three heads and the rib
/// cage's three parts.
static WITHER_PARTS: [Part; 6] = [
    // The centre head (`ModelWither.java`:29-30`).
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -4.0, -4.0],
            size: [8.0, 8.0, 8.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The left head (`ModelWither.java`:31-34`).
    Part {
        point: [-8.0, 4.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -4.0, -4.0],
            size: [6.0, 6.0, 6.0],
            uv: [32.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The right head (`ModelWither.java`:35-38`).
    Part {
        point: [10.0, 4.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-4.0, -4.0, -4.0],
            size: [6.0, 6.0, 6.0],
            uv: [32.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The rib cage's first part (`ModelWither.java`:18-19`).
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-10.0, 3.9, -0.5],
            size: [20.0, 3.0, 3.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The rib cage's middle, its three slats (`ModelWither.java`:20-25`).
    Part {
        point: [-2.0, 6.9, -0.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[
            Box {
                origin: [0.0, 0.0, 0.0],
                size: [3.0, 10.0, 3.0],
                uv: [0.0, 22.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-4.0, 1.5, 0.5],
                size: [11.0, 2.0, 2.0],
                uv: [24.0, 22.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-4.0, 4.0, 0.5],
                size: [11.0, 2.0, 2.0],
                uv: [24.0, 22.0],
                inflate: 0.0,
                mirror: false,
            },
            Box {
                origin: [-4.0, 6.5, 0.5],
                size: [11.0, 2.0, 2.0],
                uv: [24.0, 22.0],
                inflate: 0.0,
                mirror: false,
            },
        ],
        children: &[],
    },
    // The rib cage's last part (`ModelWither.java`:26-27`).
    Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, 0.0],
            size: [3.0, 6.0, 3.0],
            uv: [12.0, 22.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The wither's model.
pub static MODEL_WITHER: Model = Model {
    parts: &WITHER_PARTS,
};

/// The horse's colour sheet for its type and colour: the four type sheets the renderer holds
/// directly, the seven colour sheets of the class's own table; empty for a variant off the
/// table, which the source resolves to no sheet at all
/// (`RenderHorse.getEntityTexture`:51-78, `EntityHorse.setHorseTexturePaths`:722-742).
pub fn horse_sheet(variant: u8, colour: u8) -> &'static str {
    const COLOURS: [&str; 7] = [
        "entity/horse/horse_white.png",
        "entity/horse/horse_creamy.png",
        "entity/horse/horse_chestnut.png",
        "entity/horse/horse_brown.png",
        "entity/horse/horse_black.png",
        "entity/horse/horse_gray.png",
        "entity/horse/horse_darkbrown.png",
    ];
    const ABBR: [&str; 5] = [
        "",
        "entity/horse/donkey.png",
        "entity/horse/mule.png",
        "entity/horse/horse_zombie.png",
        "entity/horse/horse_skeleton.png",
    ];
    if variant == 0 {
        COLOURS.get(colour as usize).copied().unwrap_or("")
    } else {
        ABBR.get(variant as usize).copied().unwrap_or("")
    }
}

/// The horse's marking sheet: the class's five-entry table; empty for no marking
/// (`EntityHorse.setHorseTexturePaths`:742, table at `EntityHorse.java`:58).
pub fn horse_marking(markings: u8) -> &'static str {
    const SHEETS: [&str; 5] = [
        "",
        "entity/horse/horse_markings_white.png",
        "entity/horse/horse_markings_whitefield.png",
        "entity/horse/horse_markings_whitedots.png",
        "entity/horse/horse_markings_blackdots.png",
    ];
    SHEETS.get(markings as usize).copied().unwrap_or("")
}

/// The horse's armour sheet: the class's four-entry table; empty for no armour
/// (`EntityHorse.setHorseTexturePaths`:759, table at `EntityHorse.java`:53).
pub fn horse_armour(armour: u8) -> &'static str {
    const SHEETS: [&str; 4] = [
        "",
        "entity/horse/armor/horse_armor_iron.png",
        "entity/horse/armor/horse_armor_gold.png",
        "entity/horse/armor/horse_armor_diamond.png",
    ];
    SHEETS.get(armour as usize).copied().unwrap_or("")
}

/// The ocelot's sheet: the wild cat's coat for type zero, the tamed coats for one through
/// three, the wild coat again off the table (`RenderOcelot.getEntityTexture`:23-40).
pub fn ocelot_sheet(variant: u8) -> &'static str {
    const SHEETS: [&str; 4] = [
        "entity/cat/ocelot.png",
        "entity/cat/black.png",
        "entity/cat/red.png",
        "entity/cat/siamese.png",
    ];
    SHEETS
        .get(variant as usize)
        .copied()
        .unwrap_or("entity/cat/ocelot.png")
}

/// The rabbit's sheet: the six coats, the killer rabbit's, and the brown default off the
/// table (`RenderRabbit.getEntityTexture`:41-62).
pub fn rabbit_sheet(variant: u8) -> &'static str {
    const SHEETS: [&str; 6] = [
        "entity/rabbit/brown.png",
        "entity/rabbit/white.png",
        "entity/rabbit/black.png",
        "entity/rabbit/white_splotched.png",
        "entity/rabbit/gold.png",
        "entity/rabbit/salt.png",
    ];
    if variant == 99 {
        "entity/rabbit/caerbannog.png"
    } else {
        SHEETS
            .get(variant as usize)
            .copied()
            .unwrap_or("entity/rabbit/brown.png")
    }
}

/// The wolf's sheet: the tamed coat first, then the angry one, then the wild
/// (`RenderWolf.getEntityTexture`:46-49).
pub fn wolf_sheet(tamed: bool, angry: bool) -> &'static str {
    if tamed {
        "entity/wolf/wolf_tame.png"
    } else if angry {
        "entity/wolf/wolf_angry.png"
    } else {
        "entity/wolf/wolf.png"
    }
}

/// The wolf's tail rotation — the renderer's rotation float, which the model's tail angle
/// reads: the angry tail's own angle, the tamed droop scaled by the entity's health, the
/// wild tail's fifth of a turn (`EntityWolf.getTailRotation`:493-496).
pub fn wolf_tail_rotation(tamed: bool, angry: bool, health: f32) -> f32 {
    if angry {
        1.5393804
    } else if tamed {
        (0.55 - (20.0 - health) * 0.02) * PI
    } else {
        PI / 5.0
    }
}

/// The horse's pose: the class's own living animations, its whole pose
/// (`ModelHorse.setLivingAnimations`:353-572).
pub fn pose_horse(pose: &Pose, out: &mut [Rot]) {
    let (saddle, chested, adult, variant) = match pose.extra {
        PoseExtra::Horse {
            saddle,
            chested,
            adult,
            variant,
        } => (saddle, chested, adult, variant),
        _ => (false, false, true, 0),
    };
    // The fractions the class reads off the entity but the wire does not carry — the mouth
    // gap, the grass bite and the rear — pin off, and no rider ever sits
    // (`f5 = f6 = f8 = 0`).
    let swing = pose.limb_swing;
    let amount = pose.limb_swing_amount;
    let f3 = pose.head_yaw.clamp(-20.0, 20.0);
    let mut f4 = pose.head_pitch * (PI / 180.0);
    if amount > 0.2 {
        f4 += (swing * 0.4).cos() * 0.15 * amount;
    }
    let f10 = (swing * 0.6662 + PI).cos();
    let f11 = f10 * 0.8 * amount;

    // The head and the neck, ears and mane that ride its stride (`:395-400`, `:406-437`).
    let head_point = [0.0, 4.0, -10.0];
    let head_angles = [FRAC_PI_6 + f4, f3 * (PI / 180.0), 0.0];
    out[horse::HEAD].point = head_point;
    out[horse::HEAD].angles = head_angles;
    for slot in [
        horse::NECK,
        horse::HORSE_LEFT_EAR,
        horse::HORSE_RIGHT_EAR,
        horse::MULE_LEFT_EAR,
        horse::MULE_RIGHT_EAR,
        horse::MANE,
    ] {
        out[slot].point = head_point;
        out[slot].angles = head_angles;
    }
    // The two mouths (`:411-428`).
    out[horse::MUZZLE_UPPER].point = [0.0, 0.02, 0.02];
    out[horse::MUZZLE_UPPER].angles = [0.0, 0.0, 0.0];
    out[horse::MUZZLE_LOWER].point = [0.0, 0.0, 0.0];
    out[horse::MUZZLE_LOWER].angles = [0.0, 0.0, 0.0];
    // The tail (`:546-551`, `:569-571`): the mounting droop, clamped at level.
    let f12 = {
        let droop = -1.3089 + amount * 1.5;
        if droop > 0.0 { 0.0 } else { droop }
    };
    out[horse::TAIL_BASE].point = [0.0, 3.0, 14.0];
    out[horse::TAIL_MIDDLE].point = [0.0, 3.0, 14.0];
    out[horse::TAIL_TIP].point = [0.0, 3.0, 14.0];
    out[horse::TAIL_BASE].angles = [f12, 0.0, 0.0];
    out[horse::TAIL_MIDDLE].angles = [f12, 0.0, 0.0];
    out[horse::TAIL_TIP].angles = [-0.2618 + f12, 0.0, 0.0];
    // The legs (`:445-478`): the rear pair half the swing, the front pair the whole, hooves
    // hanging from their shins and the shins' pivots following.
    let bl_swing = -f10 * 0.5 * amount;
    let br_swing = f10 * 0.5 * amount;
    out[horse::BACK_LEFT_LEG].angles[0] = bl_swing;
    out[horse::BACK_LEFT_SHIN].angles[0] = bl_swing - (f10 * 0.5 * amount).max(0.0);
    out[horse::BACK_LEFT_HOOF].angles[0] = out[horse::BACK_LEFT_SHIN].angles[0];
    out[horse::BACK_RIGHT_LEG].angles[0] = br_swing;
    out[horse::BACK_RIGHT_SHIN].angles[0] = br_swing - (-f10 * 0.5 * amount).max(0.0);
    out[horse::BACK_RIGHT_HOOF].angles[0] = out[horse::BACK_RIGHT_SHIN].angles[0];
    out[horse::FRONT_LEFT_LEG].point = [4.0, 9.0, -8.0];
    out[horse::FRONT_LEFT_LEG].angles[0] = f11;
    out[horse::FRONT_LEFT_SHIN].angles[0] = f11 + (f10 * 0.5 * amount).max(0.0);
    out[horse::FRONT_LEFT_HOOF].angles[0] = out[horse::FRONT_LEFT_SHIN].angles[0];
    out[horse::FRONT_RIGHT_LEG].point = [-4.0, 9.0, -8.0];
    out[horse::FRONT_RIGHT_LEG].angles[0] = -f11;
    out[horse::FRONT_RIGHT_SHIN].angles[0] = -f11 + (-f10 * 0.5 * amount).max(0.0);
    out[horse::FRONT_RIGHT_HOOF].angles[0] = out[horse::FRONT_RIGHT_SHIN].angles[0];
    let bl_y = 9.0 + (PI / 2.0 + bl_swing).sin() * 7.0;
    let bl_z = 11.0 + (3.0 * PI / 2.0 + bl_swing).cos() * 7.0;
    out[horse::BACK_LEFT_SHIN].point = [4.0, bl_y, bl_z];
    out[horse::BACK_LEFT_HOOF].point = [4.0, bl_y, bl_z];
    let br_y = 9.0 + (PI / 2.0 + br_swing).sin() * 7.0;
    let br_z = 11.0 + (3.0 * PI / 2.0 + br_swing).cos() * 7.0;
    out[horse::BACK_RIGHT_SHIN].point = [-4.0, br_y, br_z];
    out[horse::BACK_RIGHT_HOOF].point = [-4.0, br_y, br_z];
    let fl_y = 9.0 + (PI / 2.0 + f11).sin() * 7.0;
    let fl_z = -8.0 + (3.0 * PI / 2.0 + f11).cos() * 7.0;
    out[horse::FRONT_LEFT_SHIN].point = [4.0, fl_y, fl_z];
    out[horse::FRONT_LEFT_HOOF].point = [4.0, fl_y, fl_z];
    let fr_y = 9.0 + (PI / 2.0 - f11).sin() * 7.0;
    let fr_z = -8.0 + (3.0 * PI / 2.0 - f11).cos() * 7.0;
    out[horse::FRONT_RIGHT_SHIN].point = [-4.0, fr_y, fr_z];
    out[horse::FRONT_RIGHT_HOOF].point = [-4.0, fr_y, fr_z];
    // The mule's chests and the ridden gear (`:438-439`, `:480-542`): the chests pull
    // inward with the swing, the saddle's ropes follow the gait.
    out[horse::MULE_LEFT_CHEST].angles[0] = f11 / 5.0;
    out[horse::MULE_RIGHT_CHEST].angles[0] = -f11 / 5.0;
    if saddle {
        out[horse::SADDLE_BOTTOM].point = [0.0, 2.0, 2.0];
        out[horse::SADDLE_FRONT].point = [0.0, 2.0, 2.0];
        out[horse::SADDLE_BACK].point = [0.0, 2.0, 2.0];
        out[horse::MULE_LEFT_CHEST].point = [-7.5, 2.0, 2.0];
        out[horse::MULE_RIGHT_CHEST].point = [4.5, 2.0, 2.0];
        let rope_x = f11 / 3.0;
        let rope_z = f11 / 5.0;
        for slot in [horse::LEFT_SADDLE_METAL, horse::LEFT_SADDLE_ROPE] {
            out[slot].point = [5.0, 2.0, 2.0];
            out[slot].angles = [rope_x, 0.0, rope_z];
        }
        for slot in [horse::RIGHT_SADDLE_METAL, horse::RIGHT_SADDLE_ROPE] {
            out[slot].point = [-5.0, 2.0, 2.0];
            out[slot].angles = [rope_x, 0.0, -rope_z];
        }
    }
    // The face gear rides the head (`:501-520`).
    for slot in [
        horse::FACE_ROPES,
        horse::LEFT_FACE_METAL,
        horse::RIGHT_FACE_METAL,
        horse::LEFT_REIN,
        horse::RIGHT_REIN,
    ] {
        out[slot].point = head_point;
        out[slot].angles = head_angles;
    }
    out[horse::LEFT_REIN].angles[0] = f4;
    out[horse::RIGHT_REIN].angles[0] = f4;
    // The ear pick and the hidden gear (`ModelHorse.render`:215-317`): the mule's ears for
    // the mule kinds, the horse's for the rest; the gear rides the saddle, the chests the
    // chest flag, and the reins wait on a rider the frame never carries.
    let mule = variant == 1 || variant == 2;
    out[horse::HORSE_LEFT_EAR].visible = !mule;
    out[horse::HORSE_RIGHT_EAR].visible = !mule;
    out[horse::MULE_LEFT_EAR].visible = mule;
    out[horse::MULE_RIGHT_EAR].visible = mule;
    let gear = saddle && adult;
    for slot in [
        horse::SADDLE_BOTTOM,
        horse::SADDLE_FRONT,
        horse::SADDLE_BACK,
        horse::LEFT_SADDLE_METAL,
        horse::LEFT_SADDLE_ROPE,
        horse::RIGHT_SADDLE_METAL,
        horse::RIGHT_SADDLE_ROPE,
        horse::FACE_ROPES,
        horse::LEFT_FACE_METAL,
        horse::RIGHT_FACE_METAL,
    ] {
        out[slot].visible = gear;
    }
    out[horse::LEFT_REIN].visible = false;
    out[horse::RIGHT_REIN].visible = false;
    out[horse::MULE_LEFT_CHEST].visible = chested && adult;
    out[horse::MULE_RIGHT_CHEST].visible = chested && adult;
}

/// The wolf's pose (`ModelWolf.setLivingAnimations`:112-163,
/// `setRotationAngles`:170-176).
pub fn pose_wolf(pose: &Pose, out: &mut [Rot]) {
    let (tamed, angry, sitting, health) = match pose.extra {
        PoseExtra::Wolf {
            tamed,
            angry,
            sitting,
            health,
        } => (tamed, angry, sitting, health),
        _ => (false, false, false, 20.0),
    };
    let swing = pose.limb_swing;
    let amount = pose.limb_swing_amount;
    // The head reads the frame directly (`:170-176`); the lean and the head-shake are not
    // carried and pin off.
    out[wolf::HEAD].angles = [
        pose.head_pitch * (PI / 180.0),
        pose.head_yaw * (PI / 180.0),
        0.0,
    ];
    // The tail's yaw is the walk's swing, silent while angry (`ModelWolf.setLivingAnimations`:116-123); its pitch the
    // renderer's rotation float (`ModelWolf.setRotationAngles`:175).
    out[wolf::TAIL].angles[1] = if angry {
        0.0
    } else {
        (swing * 0.6662).cos() * 1.4 * amount
    };
    out[wolf::TAIL].angles[0] = wolf_tail_rotation(tamed, angry, health);
    if sitting {
        // The sit (`ModelWolf.setLivingAnimations`:125-141): the haunches down, the front legs forward.
        out[wolf::BODY].point = [0.0, 18.0, 0.0];
        out[wolf::BODY].angles[0] = PI / 4.0;
        out[wolf::MANE].point = [-1.0, 16.0, -3.0];
        out[wolf::MANE].angles[0] = PI * 2.0 / 5.0;
        out[wolf::TAIL].point = [-1.0, 21.0, 6.0];
        out[wolf::LEG1].point = [-2.5, 22.0, 2.0];
        out[wolf::LEG1].angles[0] = 3.0 * PI / 2.0;
        out[wolf::LEG2].point = [0.5, 22.0, 2.0];
        out[wolf::LEG2].angles[0] = 3.0 * PI / 2.0;
        out[wolf::LEG3].angles[0] = 5.811947;
        out[wolf::LEG3].point = [-2.49, 17.0, -4.0];
        out[wolf::LEG4].angles[0] = 5.811947;
        out[wolf::LEG4].point = [0.51, 17.0, -4.0];
    } else {
        // The standing pose (`:132-163`).
        out[wolf::BODY].point = [0.0, 14.0, 2.0];
        out[wolf::BODY].angles[0] = PI / 2.0;
        out[wolf::MANE].point = [-1.0, 14.0, -3.0];
        out[wolf::MANE].angles[0] = PI / 2.0;
        out[wolf::TAIL].point = [-1.0, 12.0, 8.0];
        out[wolf::LEG1].point = [-2.5, 16.0, 7.0];
        out[wolf::LEG1].angles[0] = (swing * 0.6662).cos() * 1.4 * amount;
        out[wolf::LEG2].point = [0.5, 16.0, 7.0];
        out[wolf::LEG2].angles[0] = (swing * 0.6662 + PI).cos() * 1.4 * amount;
        out[wolf::LEG3].point = [-2.5, 16.0, -4.0];
        out[wolf::LEG3].angles[0] = (swing * 0.6662 + PI).cos() * 1.4 * amount;
        out[wolf::LEG4].point = [0.5, 16.0, -4.0];
        out[wolf::LEG4].angles[0] = (swing * 0.6662).cos() * 1.4 * amount;
    }
}

/// The ocelot's pose (`ModelOcelot.setRotationAngles`:117-151,
/// `setLivingAnimations`:157-218).
pub fn pose_ocelot(pose: &Pose, out: &mut [Rot]) {
    let sitting = match pose.extra {
        PoseExtra::Ocelot { sitting } => sitting,
        _ => false,
    };
    let swing = pose.limb_swing;
    let amount = pose.limb_swing_amount;
    // The rest shape (`:157-218`) — the sprint state off the table (the wire does not carry
    // it), the sneak read from the pose.
    out[ocelot::BODY].point = [0.0, 12.0, -10.0];
    out[ocelot::HEAD].point = [0.0, 15.0, -9.0];
    out[ocelot::TAIL].point = [0.0, 15.0, 8.0];
    out[ocelot::TAIL].angles[0] = 0.9;
    out[ocelot::TAIL2].point = [0.0, 20.0, 14.0];
    out[ocelot::FRONT_LEFT_LEG].point = [1.2, 13.8, -5.0];
    out[ocelot::FRONT_RIGHT_LEG].point = [-1.2, 13.8, -5.0];
    out[ocelot::BACK_LEFT_LEG].point = [1.1, 18.0, 5.0];
    out[ocelot::BACK_RIGHT_LEG].point = [-1.1, 18.0, 5.0];
    let state = if pose.sneak {
        // The stalk (`:165-179`).
        out[ocelot::BODY].point[1] += 1.0;
        out[ocelot::HEAD].point[1] += 2.0;
        out[ocelot::TAIL].point[1] += 1.0;
        out[ocelot::TAIL2].point[1] -= 4.0;
        out[ocelot::TAIL2].point[2] += 2.0;
        out[ocelot::TAIL].angles[0] = PI / 2.0;
        out[ocelot::TAIL2].angles[0] = PI / 2.0;
        0
    } else if sitting {
        // The sit (`:181-216`): the haunches down, the head low, the front legs braced.
        out[ocelot::BODY].angles[0] = PI / 4.0;
        out[ocelot::BODY].point[1] -= 4.0;
        out[ocelot::BODY].point[2] += 5.0;
        out[ocelot::HEAD].point[1] -= 3.3;
        out[ocelot::HEAD].point[2] += 1.0;
        out[ocelot::TAIL].point[1] += 8.0;
        out[ocelot::TAIL].point[2] -= 2.0;
        out[ocelot::TAIL2].point[1] += 2.0;
        out[ocelot::TAIL2].point[2] -= 0.8;
        out[ocelot::TAIL].angles[0] = 1.7278761;
        out[ocelot::TAIL2].angles[0] = 2.670354;
        out[ocelot::FRONT_LEFT_LEG].angles[0] = -0.15707964;
        out[ocelot::FRONT_RIGHT_LEG].angles[0] = -0.15707964;
        out[ocelot::FRONT_LEFT_LEG].point[1] = 15.8;
        out[ocelot::FRONT_RIGHT_LEG].point[1] = 15.8;
        out[ocelot::FRONT_LEFT_LEG].point[2] = -7.0;
        out[ocelot::FRONT_RIGHT_LEG].point[2] = -7.0;
        out[ocelot::BACK_LEFT_LEG].angles[0] = -PI / 2.0;
        out[ocelot::BACK_RIGHT_LEG].angles[0] = -PI / 2.0;
        out[ocelot::BACK_LEFT_LEG].point[1] = 21.0;
        out[ocelot::BACK_RIGHT_LEG].point[1] = 21.0;
        out[ocelot::BACK_LEFT_LEG].point[2] = 1.0;
        out[ocelot::BACK_RIGHT_LEG].point[2] = 1.0;
        3
    } else {
        1
    };
    // The walk (`ModelOcelot.setRotationAngles`:117-151`): the body flattens level and the legs swing.
    out[ocelot::HEAD].angles = [
        pose.head_pitch * (PI / 180.0),
        pose.head_yaw * (PI / 180.0),
        0.0,
    ];
    if state != 3 {
        out[ocelot::BODY].angles[0] = PI / 2.0;
        out[ocelot::BACK_LEFT_LEG].angles[0] = (swing * 0.6662).cos() * amount;
        out[ocelot::BACK_RIGHT_LEG].angles[0] = (swing * 0.6662 + PI).cos() * amount;
        out[ocelot::FRONT_LEFT_LEG].angles[0] = (swing * 0.6662 + PI).cos() * amount;
        out[ocelot::FRONT_RIGHT_LEG].angles[0] = (swing * 0.6662).cos() * amount;
        let rate = if state == 1 { PI / 4.0 } else { 0.47123894 };
        out[ocelot::TAIL2].angles[0] = 1.7278761 + rate * swing.cos() * amount;
    }
}

/// The rabbit's pose (`ModelRabbit.setRotationAngles`:176-188).
pub fn pose_rabbit(pose: &Pose, out: &mut [Rot]) {
    let hop = match pose.extra {
        PoseExtra::Rabbit { hop } => hop,
        _ => 0.0,
    };
    // The hop progress's sine drives all six limbs (`:176-188`).
    let m = (hop * PI).sin();
    let head = [
        pose.head_pitch * 0.017453292,
        pose.head_yaw * 0.017453292,
        0.0,
    ];
    out[rabbit::HEAD].angles = head;
    out[rabbit::NOSE].angles = head;
    out[rabbit::RIGHT_EAR].angles[0] = head[0];
    out[rabbit::LEFT_EAR].angles[0] = head[0];
    out[rabbit::RIGHT_EAR].angles[1] = head[1] - 0.2617994;
    out[rabbit::LEFT_EAR].angles[1] = head[1] + 0.2617994;
    out[rabbit::LEFT_THIGH].angles[0] = (m * 50.0 - 21.0) * 0.017453292;
    out[rabbit::RIGHT_THIGH].angles[0] = (m * 50.0 - 21.0) * 0.017453292;
    out[rabbit::LEFT_FOOT].angles[0] = m * 50.0 * 0.017453292;
    out[rabbit::RIGHT_FOOT].angles[0] = m * 50.0 * 0.017453292;
    out[rabbit::LEFT_ARM].angles[0] = (m * -40.0 - 11.0) * 0.017453292;
    out[rabbit::RIGHT_ARM].angles[0] = (m * -40.0 - 11.0) * 0.017453292;
}

/// The ghast's pose (`ModelGhast.setRotationAngles`:39-45).
pub fn pose_ghast(pose: &Pose, out: &mut [Rot]) {
    // The tentacles drift on the clock, one behind the next; the whole creature floats on
    // the renderer's own shift (`RenderGhast.preRenderCallback`:33-36).
    for i in 0..9 {
        out[ghast::TENTACLE_0 + i].angles[0] = 0.2 * (pose.age * 0.3 + i as f32).sin() + 0.4;
    }
    for rot in out.iter_mut() {
        rot.offset = [0.0, 0.6, 0.0];
    }
}

/// The blaze's pose (`ModelBlaze.setRotationAngles`:43-77).
pub fn pose_blaze(pose: &Pose, out: &mut [Rot]) {
    let age = pose.age;
    // The four inner rods, then the four outer, then the four lowest — each ring turning
    // at its own rate (`:43-77`).
    let mut f = age * PI * -0.1;
    for i in 0..4 {
        let slot = blaze::ROD_0 + i;
        out[slot].point = [
            f.cos() * 9.0,
            -2.0 + ((i as f32 * 2.0 + age) * 0.25).cos(),
            f.sin() * 9.0,
        ];
        f += 1.0;
    }
    let mut f = PI / 4.0 + age * PI * 0.03;
    for j in 4..8 {
        let slot = blaze::ROD_0 + j;
        out[slot].point = [
            f.cos() * 7.0,
            2.0 + ((j as f32 * 2.0 + age) * 0.25).cos(),
            f.sin() * 7.0,
        ];
        f += 1.0;
    }
    let mut f = 0.47123894 + age * PI * -0.05;
    for k in 8..12 {
        let slot = blaze::ROD_0 + k;
        out[slot].point = [
            f.cos() * 5.0,
            11.0 + ((k as f32 * 1.5 + age) * 0.5).cos(),
            f.sin() * 5.0,
        ];
        f += 1.0;
    }
    out[blaze::HEAD].angles = [
        pose.head_pitch * (PI / 180.0),
        pose.head_yaw * (PI / 180.0),
        0.0,
    ];
}

/// The guardian's pose (`ModelGuardian.setRotationAngles`:70-135).
pub fn pose_guardian(pose: &Pose, out: &mut [Rot]) {
    let (spikes, tail_phase) = match pose.extra {
        PoseExtra::Guardian { spikes, tail_phase } => (spikes, tail_phase),
        _ => (1.0, 0.0),
    };
    out[guardian::BODY].angles = [
        pose.head_pitch * (PI / 180.0),
        pose.head_yaw * (PI / 180.0),
        0.0,
    ];
    // The twelve spines' table: each triple of tilt fractions and each row of reach
    // (`:75-97`); the extension squeezes them toward the shell when it dips.
    let a0 = [
        1.75, 0.25, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 1.25, 0.75, 0.0, 0.0,
    ];
    let a1 = [
        0.0, 0.0, 0.0, 0.0, 0.25, 1.75, 1.25, 0.75, 0.0, 0.0, 0.0, 0.0,
    ];
    let a2 = [
        0.0, 0.0, 0.25, 1.75, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.75, 1.25,
    ];
    let a3 = [
        0.0, 0.0, 8.0, -8.0, -8.0, 8.0, 8.0, -8.0, 0.0, 0.0, 8.0, -8.0,
    ];
    let a4 = [
        -8.0, -8.0, -8.0, -8.0, 0.0, 0.0, 0.0, 0.0, 8.0, 8.0, 8.0, 8.0,
    ];
    let a5 = [
        8.0, -8.0, 0.0, 0.0, -8.0, -8.0, 8.0, 8.0, 8.0, -8.0, 0.0, 0.0,
    ];
    let shrink = (1.0 - spikes) * 0.55;
    for i in 0..12 {
        let slot = guardian::SPINE_0 + i;
        out[slot].angles = [PI * a0[i], PI * a1[i], PI * a2[i]];
        let breath = 1.0 + (pose.age * 1.5 + i as f32).cos() * 0.01 - shrink;
        out[slot].point = [a3[i] * breath, 16.0 + a4[i] * breath, a5[i] * breath];
    }
    // The eye looks straight ahead while no target is carried (`:99-108`).
    out[guardian::EYE].point = [0.0, 15.0, -8.25];
    // The tail's chain (`:110-135`), its swing the carried phase.
    out[guardian::TAIL_0].angles[1] = tail_phase.sin() * PI * 0.05;
    out[guardian::TAIL_1].angles[1] = tail_phase.sin() * PI * 0.1;
    out[guardian::TAIL_1].point = [-1.5, 0.5, 14.0];
    out[guardian::TAIL_2].angles[1] = tail_phase.sin() * PI * 0.15;
    out[guardian::TAIL_2].point = [0.5, 0.5, 6.0];
}

/// The flap wave `f1` the dragon's flight reads off the interpolated clock
/// (`ModelDragon.render`:141-142): one plus the sine a radian late, squared, twice
/// itself and a twentieth.
fn dragon_wave(anim_time: f32) -> f32 {
    let raw = (anim_time * PI * 2.0 - 1.0).sin() + 1.0;
    (raw * raw + raw * 2.0) * 0.05
}

/// The dragon's model-level flight transform (`ModelDragon.render`:144-147): the whole
/// model's translate `(0, f1 - 2, -3)` in blocks and its pitch `f1 * 2` in degrees,
/// composed by the draw chain ahead of every part — the source writes both once at the
/// model's own level, never on a part.
pub fn dragon_flight(pose: &Pose) -> ([f32; 3], f32) {
    let f1 = dragon_wave(match pose.extra {
        PoseExtra::Dragon { anim_time } => anim_time,
        _ => 0.0,
    });
    ([0.0, f1 - 2.0, -3.0], f1 * 2.0)
}

/// The dragon's pose (`ModelDragon.render`:138-241). The flight's model-level translate
/// and pitch (`:144-147`) are no part's own — the draw chain carries them ahead of every
/// part ([`dragon_flight`]).
pub fn pose_dragon(pose: &Pose, out: &mut [Rot]) {
    let f = match pose.extra {
        PoseExtra::Dragon { anim_time } => anim_time,
        _ => 0.0,
    };
    // The flight clock's sine drives everything (`:140-146`); the movement offsets of the
    // neck and tail chains are not carried and pin at zero.
    let f8 = f * PI * 2.0;
    out[dragon::JAW].angles[0] = (f8.sin() + 1.0) * 0.2;
    let f1 = dragon_wave(f);
    // The wings (`:196-207`): the beat and the fold, the right side the mirror.
    let w = [0.125 - f8.cos() * 0.2, 0.25, (f8.sin() + 0.125) * 0.8];
    out[dragon::WING_LEFT].angles = w;
    out[dragon::WING_LEFT_TIP].angles[2] = -((f8 + 2.0).sin() + 0.5) * 0.75;
    out[dragon::WING_RIGHT].angles = [w[0], -w[1], -w[2]];
    out[dragon::WING_RIGHT_TIP].angles[2] = ((f8 + 2.0).sin() + 0.5) * 0.75;
    // The legs (`:209-222`): the rear strides, the front reaches.
    let rear = 1.0 + f1 * 0.1;
    let rear_tip = 0.5 + f1 * 0.1;
    let foot = 0.75 + f1 * 0.1;
    let front = 1.3 + f1 * 0.1;
    let front_tip = -0.5 - f1 * 0.1;
    for slot in [dragon::REAR_LEFT_LEG, dragon::REAR_RIGHT_LEG] {
        out[slot].angles[0] = rear;
    }
    for slot in [dragon::REAR_LEFT_TIP, dragon::REAR_RIGHT_TIP] {
        out[slot].angles[0] = rear_tip;
    }
    for slot in [dragon::REAR_LEFT_FOOT, dragon::REAR_RIGHT_FOOT] {
        out[slot].angles[0] = foot;
    }
    for slot in [dragon::FRONT_LEFT_LEG, dragon::FRONT_RIGHT_LEG] {
        out[slot].angles[0] = front;
    }
    for slot in [dragon::FRONT_LEFT_TIP, dragon::FRONT_RIGHT_TIP] {
        out[slot].angles[0] = front_tip;
    }
    for slot in [dragon::FRONT_LEFT_FOOT, dragon::FRONT_RIGHT_FOOT] {
        out[slot].angles[0] = foot;
    }
    // The neck chain (`:159-173`): each spine steps ten units along the one before.
    let (mut y, mut z, x) = (20.0_f32, -12.0_f32, 0.0_f32);
    for i in 0..5 {
        let ang = (i as f32 * 0.45 + f8).cos() * 0.15;
        let slot = dragon::NECK_0 + i;
        out[slot].angles = [ang, 0.0, 0.0];
        out[slot].point = [x, y, z];
        y += ang.sin() * 10.0;
        z -= ang.cos() * 10.0;
    }
    out[dragon::HEAD].point = [x, y, z];
    // The tail chain (`:224-238`), its turn held level.
    let (mut y, mut z, x) = (10.0_f32, 60.0_f32, 0.0_f32);
    let mut f10 = 0.0_f32;
    for k in 0..12 {
        f10 += (k as f32 * 0.45 + f8).sin() * 0.05;
        let slot = dragon::TAIL_0 + k;
        out[slot].angles = [f10, PI, 0.0];
        out[slot].point = [x, y, z];
        y += f10.sin() * 10.0;
        z += f10.cos() * 10.0;
    }
}

/// The wither's pose (`ModelWither.setRotationAngles`:64-72).
pub fn pose_wither(pose: &Pose, out: &mut [Rot]) {
    let breath = (pose.age * 0.1).cos();
    out[wither::RIB_1].angles[0] = (0.065 + 0.05 * breath) * PI;
    let swing = out[wither::RIB_1].angles[0];
    out[wither::RIB_2].point = [-2.0, 6.9 + swing.cos() * 10.0, -0.5 + swing.sin() * 10.0];
    out[wither::RIB_2].angles[0] = (0.265 + 0.1 * breath) * PI;
    out[wither::HEAD_CENTRE].angles = [
        pose.head_pitch * (PI / 180.0),
        pose.head_yaw * (PI / 180.0),
        0.0,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_models::layers::WOOL_COLOURS;
    use crate::entity_models::{corpse_roll, corpse_shift, death_rotation, layers};
    use crate::entity_pass::{DrawExtra, ModelRef};
    use std::f32::consts::PI;

    /// One box, spelled as one `addBox` call spells it.
    fn b(origin: [f32; 3], size: [f32; 3], uv: [f32; 2], inflate: f32, mirror: bool) -> Box {
        Box {
            origin,
            size,
            uv,
            inflate,
            mirror,
        }
    }

    /// The part at one flat index of a model's depth-first walk — the slot numbering the
    /// transforms use. A part out of the walk's reach reads the all-zero stand-in.
    fn flat(model: &Model, index: usize) -> &Part {
        fn walk<'a>(parts: &'a [Part], cursor: &mut usize, index: usize) -> Option<&'a Part> {
            for part in parts {
                if *cursor == index {
                    return Some(part);
                }
                *cursor += 1;
                if let Some(found) = walk(part.children, cursor, index) {
                    return Some(found);
                }
            }
            None
        }
        let mut cursor = 0;
        walk(model.parts, &mut cursor, index).unwrap_or(&ZERO)
    }

    /// The number of parts of a table, children included.
    fn part_count(parts: &[Part]) -> usize {
        parts.iter().map(|part| 1 + part_count(part.children)).sum()
    }

    /// The number of boxes a table draws, children included.
    fn box_count(parts: &[Part]) -> usize {
        parts
            .iter()
            .map(|part| part.boxes.len() + box_count(part.children))
            .sum()
    }

    /// Whether two angles agree to a ten-thousandth.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn the_horse_geometry_matches_its_class() {
        assert_eq!(part_count(&HORSE_PARTS), 39, "the horse's parts");
        assert_eq!(box_count(&HORSE_PARTS), 39, "the horse's boxes");
        assert_eq!(
            flat(&MODEL_HORSE, horse::HEAD).boxes,
            [
                b([-2.5, -10.0, -1.5], [5.0, 5.0, 7.0], [0.0, 0.0], 0.0, false),
                b(
                    [-2.0, -10.0, -7.0],
                    [4.0, 3.0, 6.0],
                    [24.0, 18.0],
                    0.0,
                    false
                ),
                b(
                    [-2.0, -7.0, -6.5],
                    [4.0, 2.0, 5.0],
                    [24.0, 27.0],
                    0.0,
                    false
                ),
            ],
            "the head's three boxes (`ModelHorse.java`:121-131)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::BODY).boxes,
            [b(
                [-5.0, -8.0, -19.0],
                [10.0, 10.0, 24.0],
                [0.0, 34.0],
                0.0,
                false
            )],
            "the body (`ModelHorse.java`:69-70)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::TAIL_TIP).rest,
            [-1.40215, 0.0, 0.0],
            "the tail tip's steeper rest (`ModelHorse.java`:83)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::NECK).boxes,
            [b(
                [-2.05, -9.8, -2.0],
                [4.0, 14.0, 8.0],
                [0.0, 12.0],
                0.0,
                false
            )],
            "the neck (`ModelHorse.java`:150-151)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::HORSE_LEFT_EAR).boxes,
            [b(
                [0.45, -12.0, 4.0],
                [2.0, 3.0, 1.0],
                [0.0, 0.0],
                0.0,
                false
            )],
            "the horse's left ear (`ModelHorse.java`:134-135)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::MULE_LEFT_EAR).boxes,
            [b(
                [-2.0, -16.0, 4.0],
                [2.0, 7.0, 1.0],
                [0.0, 12.0],
                0.0,
                false
            )],
            "the mule's left ear (`ModelHorse.java`:142-143)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::MULE_RIGHT_EAR).rest,
            [FRAC_PI_6, 0.0, -0.2617994],
            "the mule's right ear splays the other way (`ModelHorse.java`:148)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::FRONT_LEFT_LEG).boxes,
            [b(
                [-1.9, -1.0, -2.1],
                [3.0, 8.0, 4.0],
                [44.0, 29.0],
                0.0,
                false
            )],
            "the front left leg's own corners (`ModelHorse.java`:102-103)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::FRONT_LEFT_SHIN).point,
            [4.0, 16.0, -8.0],
            "the front left shin's pivot (`ModelHorse.java`:105)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::BACK_RIGHT_HOOF).boxes,
            [b(
                [-1.5, 5.1, -2.0],
                [4.0, 3.0, 4.0],
                [96.0, 51.0],
                0.0,
                false
            )],
            "the back right hoof's own corners (`ModelHorse.java`:99-100)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::MULE_LEFT_CHEST).rest,
            [0.0, PI / 2.0, 0.0],
            "the mule's left chest faces across (`ModelHorse.java`:156)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::MANE).boxes,
            [b(
                [-1.0, -11.5, 5.0],
                [2.0, 16.0, 4.0],
                [58.0, 0.0],
                0.0,
                false
            )],
            "the mane (`ModelHorse.java`:197-198)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::FACE_ROPES).boxes,
            [b(
                [-2.5, -10.1, -7.0],
                [5.0, 5.0, 12.0],
                [80.0, 12.0],
                0.2,
                false
            )],
            "the face ropes' fifth-texel inflation (`ModelHorse.java`:201-202)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::LEFT_FACE_METAL).boxes,
            [b(
                [1.5, -8.0, -4.0],
                [1.0, 2.0, 2.0],
                [74.0, 13.0],
                0.0,
                false
            )],
            "the left face metal (`ModelHorse.java`:183-184)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::LEFT_REIN).boxes,
            [b(
                [2.6, -6.0, -6.0],
                [0.0, 3.0, 16.0],
                [44.0, 10.0],
                0.0,
                false
            )],
            "the left rein's zero width (`ModelHorse.java`:191-192)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::SADDLE_BOTTOM).boxes,
            [b(
                [-5.0, 0.0, -3.0],
                [10.0, 1.0, 8.0],
                [80.0, 0.0],
                0.0,
                false
            )],
            "the saddle's seat (`ModelHorse.java`:162-163)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::SADDLE_FRONT).boxes,
            [b(
                [-1.5, -1.0, -3.0],
                [3.0, 1.0, 2.0],
                [106.0, 9.0],
                0.0,
                false
            )],
            "the saddle's front (`ModelHorse.java`:165-166)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::SADDLE_BACK).boxes,
            [b(
                [-4.0, -1.0, 3.0],
                [8.0, 1.0, 2.0],
                [80.0, 9.0],
                0.0,
                false
            )],
            "the saddle's back (`ModelHorse.java`:168-169)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::LEFT_SADDLE_ROPE).point,
            [5.0, 3.0, 2.0],
            "the left saddle rope's pivot (`ModelHorse.java`:175)"
        );
        assert_eq!(
            flat(&MODEL_HORSE, horse::RIGHT_SADDLE_ROPE).boxes,
            [b(
                [-0.5, 0.0, -0.5],
                [1.0, 6.0, 1.0],
                [80.0, 0.0],
                0.0,
                false
            )],
            "the right saddle rope's own cell (`ModelHorse.java`:180-181)"
        );
    }

    #[test]
    fn the_wolf_geometry_matches_its_class() {
        assert_eq!(part_count(&WOLF_PARTS), 8, "the wolf's parts");
        assert_eq!(box_count(&WOLF_PARTS), 11, "the wolf's boxes");
        assert_eq!(
            flat(&MODEL_WOLF, wolf::HEAD).boxes,
            [
                b([-3.0, -3.0, -2.0], [6.0, 6.0, 4.0], [0.0, 0.0], 0.0, false),
                b([-3.0, -5.0, 0.0], [2.0, 2.0, 1.0], [16.0, 14.0], 0.0, false),
                b([1.0, -5.0, 0.0], [2.0, 2.0, 1.0], [16.0, 14.0], 0.0, false),
                b([-1.5, 0.0, -5.0], [3.0, 3.0, 4.0], [0.0, 10.0], 0.0, false),
            ],
            "the head's four boxes (`ModelWolf.java`:40,`:63-65`)"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::HEAD).point,
            [-1.0, 13.5, -7.0],
            "the head's pivot (`ModelWolf.java`:41)"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::BODY).boxes,
            [b(
                [-4.0, -2.0, -3.0],
                [6.0, 9.0, 6.0],
                [18.0, 14.0],
                0.0,
                false
            )],
            "the body (`ModelWolf.java`:43) — six wide off its pivot"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::MANE).boxes,
            [b(
                [-4.0, -3.0, -3.0],
                [8.0, 6.0, 7.0],
                [21.0, 0.0],
                0.0,
                false
            )],
            "the mane (`ModelWolf.java`:46)"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::LEG1).point,
            [-2.5, 16.0, 7.0],
            "the first leg's pivot (`ModelWolf.java`:50)"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::LEG4).point,
            [0.5, 16.0, -4.0],
            "the fourth leg's pivot (`ModelWolf.java`:59)"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::TAIL).boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [2.0, 8.0, 2.0],
                [9.0, 18.0],
                0.0,
                false
            )],
            "the tail (`ModelWolf.java`:61)"
        );
        assert_eq!(
            flat(&MODEL_WOLF, wolf::TAIL).point,
            [-1.0, 12.0, 8.0],
            "the tail's pivot (`ModelWolf.java`:62)"
        );
    }

    #[test]
    fn the_ocelot_geometry_matches_its_class() {
        assert_eq!(part_count(&OCELOT_PARTS), 8, "the ocelot's parts");
        assert_eq!(box_count(&OCELOT_PARTS), 11, "the ocelot's boxes");
        assert_eq!(
            flat(&MODEL_OCELOT, ocelot::HEAD).boxes,
            [
                b([-2.5, -2.0, -3.0], [5.0, 4.0, 5.0], [0.0, 0.0], 0.0, false),
                b([-1.5, 0.0, -4.0], [3.0, 2.0, 2.0], [0.0, 24.0], 0.0, false),
                b([-2.0, -3.0, 0.0], [1.0, 1.0, 2.0], [0.0, 10.0], 0.0, false),
                b([1.0, -3.0, 0.0], [1.0, 1.0, 2.0], [6.0, 10.0], 0.0, false),
            ],
            "the head's four boxes (`ModelOcelot.java`:43-46)"
        );
        assert_eq!(
            flat(&MODEL_OCELOT, ocelot::BODY).boxes,
            [b(
                [-2.0, 3.0, -8.0],
                [4.0, 16.0, 6.0],
                [20.0, 0.0],
                0.0,
                false
            )],
            "the body (`ModelOcelot.java`:49) — the box hangs below the pivot"
        );
        assert_eq!(
            flat(&MODEL_OCELOT, ocelot::TAIL).rest,
            [0.9, 0.0, 0.0],
            "the tail's table rest (`ModelOcelot.java`:53)"
        );
        assert_eq!(
            flat(&MODEL_OCELOT, ocelot::TAIL2).point,
            [0.0, 20.0, 14.0],
            "the second tail's pivot (`ModelOcelot.java`:57)"
        );
        assert_eq!(
            flat(&MODEL_OCELOT, ocelot::FRONT_LEFT_LEG).point,
            [1.2, 13.8, -5.0],
            "the front left leg's pivot (`ModelOcelot.java`:66)"
        );
    }

    #[test]
    fn the_rabbit_geometry_matches_its_class() {
        assert_eq!(part_count(&RABBIT_PARTS), 12, "the rabbit's parts");
        assert_eq!(box_count(&RABBIT_PARTS), 12, "the rabbit's boxes");
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::LEFT_FOOT).boxes,
            [b(
                [-1.0, 5.5, -3.7],
                [2.0, 1.0, 7.0],
                [26.0, 24.0],
                0.0,
                true
            )],
            "the left foot, seven long and mirrored (`ModelRabbit.java`:55-58)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::RIGHT_FOOT).boxes,
            [b(
                [-1.0, 5.5, -3.7],
                [2.0, 1.0, 7.0],
                [8.0, 24.0],
                0.0,
                true
            )],
            "the right foot's own cell (`ModelRabbit.java`:60-63)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::BODY).boxes,
            [b(
                [-3.0, -2.0, -10.0],
                [6.0, 5.0, 10.0],
                [0.0, 0.0],
                0.0,
                true
            )],
            "the body (`ModelRabbit.java`:75-78)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::BODY).rest,
            [-0.34906584, 0.0, 0.0],
            "the body's rest a third of a turn down (`ModelRabbit.java`:79)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::RIGHT_EAR).rest,
            [0.0, -0.2617994, 0.0],
            "the right ear splays (`ModelRabbit.java`:99)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::LEFT_EAR).rest,
            [0.0, 0.2617994, 0.0],
            "the left ear splays (`ModelRabbit.java`:104)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::NOSE).boxes,
            [b(
                [-0.5, -2.5, -5.5],
                [1.0, 1.0, 1.0],
                [32.0, 9.0],
                0.0,
                true
            )],
            "the nose (`ModelRabbit.java`:110-113)"
        );
        assert_eq!(
            flat(&MODEL_RABBIT, rabbit::TAIL).boxes,
            [b(
                [-1.5, -1.5, 0.0],
                [3.0, 3.0, 2.0],
                [52.0, 6.0],
                0.0,
                true
            )],
            "the tail (`ModelRabbit.java`:105-108)"
        );
    }

    #[test]
    fn the_ghast_geometry_carries_its_seeded_tentacles() {
        assert_eq!(part_count(&GHAST_PARTS), 10, "the ghast's parts");
        assert_eq!(box_count(&GHAST_PARTS), 10, "the ghast's boxes");
        assert_eq!(
            flat(&MODEL_GHAST, ghast::BODY).boxes,
            [b(
                [-8.0, -8.0, -8.0],
                [16.0, 16.0, 16.0],
                [0.0, 0.0],
                0.0,
                false
            )],
            "the body (`ModelGhast.java`:17)"
        );
        assert_eq!(
            flat(&MODEL_GHAST, ghast::BODY).point,
            [0.0, 8.0, 0.0],
            "the body's pivot — sixteen under the tentacle ring (`ModelGhast.java`:18)"
        );
        // The tentacle lengths are the class's own seeded generator's first nine draws
        // (`ModelGhast.java`:19-27): `new Random(1660L)`'s `nextInt(7) + 8`.
        let lengths = [
            flat(&MODEL_GHAST, ghast::TENTACLE_0).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 1).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 2).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 3).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 4).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 5).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 6).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 7).boxes[0].size[1],
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 8).boxes[0].size[1],
        ];
        assert_eq!(lengths, [8.0, 13.0, 9.0, 11.0, 11.0, 10.0, 12.0, 9.0, 12.0]);
        assert_eq!(
            flat(&MODEL_GHAST, ghast::TENTACLE_0).boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [2.0, 8.0, 2.0],
                [0.0, 0.0],
                0.0,
                false
            )],
            "the first tentacle's own box and the first length (`ModelGhast.java`:27)"
        );
        assert_eq!(
            flat(&MODEL_GHAST, ghast::TENTACLE_0).point,
            [-3.75, 15.0, -5.0],
            "the first tentacle's pivot (`ModelGhast.java`:24-30)"
        );
        assert_eq!(
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 5).point,
            [3.75, 15.0, 0.0],
            "the sixth tentacle's pivot"
        );
        assert_eq!(
            flat(&MODEL_GHAST, ghast::TENTACLE_0 + 8).point,
            [6.25, 15.0, 5.0],
            "the ninth tentacle's pivot"
        );
    }

    #[test]
    fn the_blaze_geometry_matches_its_class() {
        assert_eq!(part_count(&BLAZE_PARTS), 13, "the blaze's parts");
        assert_eq!(box_count(&BLAZE_PARTS), 13, "the blaze's boxes");
        assert_eq!(
            flat(&MODEL_BLAZE, blaze::HEAD).boxes,
            [b(
                [-4.0, -4.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )],
            "the head (`ModelBlaze.java`:20-21)"
        );
        assert_eq!(
            flat(&MODEL_BLAZE, blaze::ROD_0).boxes,
            [b([0.0, 0.0, 0.0], [2.0, 8.0, 2.0], [0.0, 16.0], 0.0, false)],
            "a rod — the rods share one corner and cell (`ModelBlaze.java`:16-17)"
        );
        assert_eq!(
            flat(&MODEL_BLAZE, blaze::ROD_0 + 11).boxes,
            flat(&MODEL_BLAZE, blaze::ROD_0).boxes,
            "every rod is the same box"
        );
    }

    #[test]
    fn the_guardian_geometry_matches_its_class() {
        assert_eq!(part_count(&GUARDIAN_PARTS), 17, "the guardian's parts");
        assert_eq!(box_count(&GUARDIAN_PARTS), 22, "the guardian's boxes");
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::BODY).boxes,
            [
                b(
                    [-6.0, 10.0, -8.0],
                    [12.0, 12.0, 16.0],
                    [0.0, 0.0],
                    0.0,
                    false
                ),
                b(
                    [-8.0, 10.0, -6.0],
                    [2.0, 12.0, 12.0],
                    [0.0, 28.0],
                    0.0,
                    false
                ),
                b([6.0, 10.0, -6.0], [2.0, 12.0, 12.0], [0.0, 28.0], 0.0, true),
                b(
                    [-6.0, 8.0, -6.0],
                    [12.0, 2.0, 12.0],
                    [16.0, 40.0],
                    0.0,
                    false
                ),
                b(
                    [-6.0, 22.0, -6.0],
                    [12.0, 2.0, 12.0],
                    [16.0, 40.0],
                    0.0,
                    false
                ),
            ],
            "the body's five boxes — the right fin mirrors (`ModelGuardian.java`:22-26)"
        );
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::SPINE_0).boxes,
            [b(
                [-1.0, -4.5, -1.0],
                [2.0, 9.0, 2.0],
                [0.0, 0.0],
                0.0,
                false
            )],
            "a spine (`ModelGuardian.java`:31)"
        );
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::SPINE_0 + 11).boxes,
            flat(&MODEL_GUARDIAN, guardian::SPINE_0).boxes,
            "every spine is the same box"
        );
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::EYE).boxes,
            [b(
                [-1.0, 15.0, 0.0],
                [2.0, 2.0, 1.0],
                [8.0, 0.0],
                0.0,
                false
            )],
            "the eye (`ModelGuardian.java`:36)"
        );
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::TAIL_0).boxes,
            [b(
                [-2.0, 14.0, 7.0],
                [4.0, 4.0, 8.0],
                [40.0, 0.0],
                0.0,
                false
            )],
            "the tail's first part (`ModelGuardian.java`:40)"
        );
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::TAIL_1).boxes,
            [b(
                [0.0, 14.0, 0.0],
                [3.0, 3.0, 7.0],
                [0.0, 54.0],
                0.0,
                false
            )],
            "the tail's second part (`ModelGuardian.java`:42)"
        );
        assert_eq!(
            flat(&MODEL_GUARDIAN, guardian::TAIL_2).boxes,
            [
                b([0.0, 14.0, 0.0], [2.0, 2.0, 6.0], [41.0, 32.0], 0.0, false),
                b([1.0, 10.5, 3.0], [1.0, 9.0, 9.0], [25.0, 19.0], 0.0, false),
            ],
            "the tail's last part and its fin (`ModelGuardian.java`:44-45)"
        );
    }

    #[test]
    fn the_dragon_geometry_matches_its_class() {
        assert_eq!(part_count(&DRAGON_PARTS), 36, "the dragon's parts");
        // The source's twenty-three constructor boxes, the spine's two drawn seventeen
        // times (`ModelDragon.java`:84-86,`:159-173`,`:224-238`).
        assert_eq!(box_count(&DRAGON_PARTS), 65, "the dragon's drawn boxes");
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::HEAD).boxes,
            [
                b(
                    [-6.0, -1.0, -24.0],
                    [12.0, 5.0, 16.0],
                    [176.0, 44.0],
                    0.0,
                    false
                ),
                b(
                    [-8.0, -8.0, -10.0],
                    [16.0, 16.0, 16.0],
                    [112.0, 30.0],
                    0.0,
                    false
                ),
                b([-5.0, -12.0, -4.0], [2.0, 4.0, 6.0], [0.0, 0.0], 0.0, true),
                b(
                    [-5.0, -3.0, -22.0],
                    [2.0, 2.0, 4.0],
                    [112.0, 0.0],
                    0.0,
                    true
                ),
                b([3.0, -12.0, -4.0], [2.0, 4.0, 6.0], [0.0, 0.0], 0.0, false),
                b(
                    [3.0, -3.0, -22.0],
                    [2.0, 2.0, 4.0],
                    [112.0, 0.0],
                    0.0,
                    false
                ),
            ],
            "the head's six boxes — the left pair mirrors (`ModelDragon.java`:72-79)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::JAW).point,
            [0.0, 4.0, -8.0],
            "the jaw's pivot, eight forward of the head's origin (`ModelDragon.java`:81)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::JAW).boxes,
            [b(
                [-6.0, 0.0, -16.0],
                [12.0, 4.0, 16.0],
                [176.0, 65.0],
                0.0,
                false
            )],
            "the jaw (`ModelDragon.java`:82)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::BODY).boxes,
            [
                b(
                    [-12.0, 0.0, -16.0],
                    [24.0, 24.0, 64.0],
                    [0.0, 0.0],
                    0.0,
                    false
                ),
                b(
                    [-1.0, -6.0, -10.0],
                    [2.0, 6.0, 12.0],
                    [220.0, 53.0],
                    0.0,
                    false
                ),
                b(
                    [-1.0, -6.0, 10.0],
                    [2.0, 6.0, 12.0],
                    [220.0, 53.0],
                    0.0,
                    false
                ),
                b(
                    [-1.0, -6.0, 30.0],
                    [2.0, 6.0, 12.0],
                    [220.0, 53.0],
                    0.0,
                    false
                ),
            ],
            "the body and its three dorsal scales (`ModelDragon.java`:89-92)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::BODY).point,
            [0.0, 4.0, 8.0],
            "the body's pivot (`ModelDragon.java`:88)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::NECK_0).boxes,
            [
                b(
                    [-5.0, -5.0, -5.0],
                    [10.0, 10.0, 10.0],
                    [192.0, 104.0],
                    0.0,
                    false
                ),
                b([-1.0, -9.0, -3.0], [2.0, 4.0, 6.0], [48.0, 0.0], 0.0, false),
            ],
            "a spine, its box and scale (`ModelDragon.java`:85-86)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::NECK_0 + 4).boxes,
            flat(&MODEL_DRAGON, dragon::NECK_0).boxes,
            "the neck boxes draw seventeen times"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::WING_LEFT).boxes,
            [
                b(
                    [-56.0, -4.0, -4.0],
                    [56.0, 8.0, 8.0],
                    [112.0, 88.0],
                    0.0,
                    false
                ),
                b(
                    [-56.0, 0.0, 2.0],
                    [56.0, 0.0, 56.0],
                    [-56.0, 88.0],
                    0.0,
                    false
                ),
            ],
            "the wing's bone and its flat skin (`ModelDragon.java`:95-96)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::WING_LEFT).point,
            [-12.0, 5.0, 2.0],
            "the wing's pivot (`ModelDragon.java`:94)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::WING_LEFT_TIP).point,
            [-56.0, 0.0, 0.0],
            "the wing tip's pivot at the bone's end (`ModelDragon.java`:98)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::WING_LEFT_TIP).boxes,
            [
                b(
                    [-56.0, -2.0, -2.0],
                    [56.0, 4.0, 4.0],
                    [112.0, 136.0],
                    0.0,
                    false
                ),
                b(
                    [-56.0, 0.0, 2.0],
                    [56.0, 0.0, 56.0],
                    [-56.0, 144.0],
                    0.0,
                    false
                ),
            ],
            "the wing tip's bone and skin (`ModelDragon.java`:99-100)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::WING_RIGHT).boxes,
            [
                b(
                    [-56.0, -4.0, -4.0],
                    [56.0, 8.0, 8.0],
                    [112.0, 88.0],
                    0.0,
                    false
                ),
                b(
                    [-56.0, 0.0, 2.0],
                    [56.0, 0.0, 56.0],
                    [-56.0, 88.0],
                    0.0,
                    false
                ),
            ],
            "the right wing spans its own side; the renderer draws it mirrored"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::WING_RIGHT).point,
            [12.0, 5.0, 2.0],
            "the right wing's pivot mirrors the left's (`ModelDragon.java`:206)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::FRONT_LEFT_LEG).boxes,
            [b(
                [-4.0, -4.0, -4.0],
                [8.0, 24.0, 8.0],
                [112.0, 104.0],
                0.0,
                false
            )],
            "the front leg (`ModelDragon.java`:104)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::FRONT_LEFT_TIP).point,
            [0.0, 20.0, -1.0],
            "the front leg tip's pivot (`ModelDragon.java`:106)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::FRONT_LEFT_FOOT).boxes,
            [b(
                [-4.0, 0.0, -12.0],
                [8.0, 4.0, 16.0],
                [144.0, 104.0],
                0.0,
                false
            )],
            "the front foot, sixteen long (`ModelDragon.java`:111)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::REAR_LEFT_TIP).boxes,
            [b(
                [-6.0, -2.0, 0.0],
                [12.0, 32.0, 12.0],
                [196.0, 0.0],
                0.0,
                false
            )],
            "the rear leg tip (`ModelDragon.java`:118)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::REAR_LEFT_FOOT).point,
            [0.0, 31.0, 4.0],
            "the rear foot's pivot (`ModelDragon.java`:121)"
        );
        assert_eq!(
            flat(&MODEL_DRAGON, dragon::TAIL_0 + 11).boxes,
            flat(&MODEL_DRAGON, dragon::TAIL_0).boxes,
            "the tail's spines share the neck's boxes"
        );
    }

    #[test]
    fn the_wither_geometry_matches_its_class() {
        assert_eq!(part_count(&WITHER_PARTS), 6, "the wither's parts");
        assert_eq!(box_count(&WITHER_PARTS), 9, "the wither's boxes");
        assert_eq!(
            flat(&MODEL_WITHER, wither::HEAD_CENTRE).boxes,
            [b(
                [-4.0, -4.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )],
            "the centre head (`ModelWither.java`:30)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::HEAD_LEFT).boxes,
            [b(
                [-4.0, -4.0, -4.0],
                [6.0, 6.0, 6.0],
                [32.0, 0.0],
                0.0,
                false
            )],
            "the left head (`ModelWither.java`:32)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::HEAD_LEFT).point,
            [-8.0, 4.0, 0.0],
            "the left head's pivot (`ModelWither.java`:33-34)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::HEAD_RIGHT).point,
            [10.0, 4.0, 0.0],
            "the right head's pivot (`ModelWither.java`:37-38)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::RIB_0).boxes,
            [b(
                [-10.0, 3.9, -0.5],
                [20.0, 3.0, 3.0],
                [0.0, 16.0],
                0.0,
                false
            )],
            "the rib cage's first part (`ModelWither.java`:19)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::RIB_1).boxes,
            [
                b([0.0, 0.0, 0.0], [3.0, 10.0, 3.0], [0.0, 22.0], 0.0, false),
                b([-4.0, 1.5, 0.5], [11.0, 2.0, 2.0], [24.0, 22.0], 0.0, false),
                b([-4.0, 4.0, 0.5], [11.0, 2.0, 2.0], [24.0, 22.0], 0.0, false),
                b([-4.0, 6.5, 0.5], [11.0, 2.0, 2.0], [24.0, 22.0], 0.0, false),
            ],
            "the middle rib and its three slats (`ModelWither.java`:22-25)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::RIB_1).point,
            [-2.0, 6.9, -0.5],
            "the middle rib's pivot (`ModelWither.java`:21)"
        );
        assert_eq!(
            flat(&MODEL_WITHER, wither::RIB_2).boxes,
            [b(
                [0.0, 0.0, 0.0],
                [3.0, 6.0, 3.0],
                [12.0, 22.0],
                0.0,
                false
            )],
            "the rib cage's last part (`ModelWither.java`:27)"
        );
    }

    #[test]
    fn the_horse_walk_swings_its_legs_and_tail() {
        let pose = Pose {
            limb_swing: 1.0,
            limb_swing_amount: 1.0,
            head_yaw: 10.0,
            head_pitch: 5.0,
            extra: PoseExtra::Horse {
                saddle: true,
                chested: false,
                adult: true,
                variant: 0,
            },
            ..Pose::default()
        };
        let mut out = MODEL_HORSE.rest();
        pose_horse(&pose, &mut out);
        // The head takes the frame's clamped turn, its table lean and the walk's nod
        // (`ModelHorse.java`:372-375,`:154-157` over `:133`).
        assert!(
            close(out[horse::HEAD].angles[0], 0.749_024),
            "the head's pitch"
        );
        assert!(
            close(out[horse::HEAD].angles[1], 0.174_533),
            "the head's yaw"
        );
        // The muzzles follow the head's live pivots (`:405-409`).
        assert_eq!(out[horse::MUZZLE_UPPER].point, [0.0, 0.02, 0.02]);
        assert_eq!(out[horse::MUZZLE_LOWER].point, [0.0, 0.0, 0.0]);
        assert_eq!(out[horse::NECK].point, out[horse::HEAD].point);
        // The front left leg runs on the walk's own pair, the right against it, and the
        // right shin takes the max term (`:467-478`).
        assert!(close(out[horse::FRONT_LEFT_LEG].angles[0], -0.628_941));
        assert!(close(out[horse::FRONT_RIGHT_LEG].angles[0], 0.628_941));
        assert!(close(out[horse::FRONT_RIGHT_SHIN].angles[0], 1.022_028));
        // The rear legs swing opposite one another and their shins fold (`:459-464`).
        assert!(close(out[horse::BACK_LEFT_LEG].angles[0], 0.393_088));
        assert!(close(out[horse::BACK_RIGHT_LEG].angles[0], -0.393_088));
        assert!(close(out[horse::BACK_RIGHT_SHIN].angles[0], -0.786_176));
        // The tail's base decays to the walk's clamp and the tip hangs on (`:546-571`).
        assert!(close(out[horse::TAIL_BASE].angles[0], 0.0));
        assert!(close(out[horse::TAIL_TIP].angles[0], -0.261_8));
        // The saddle parts draw when the draw is saddled; the reins wait on a rider and
        // the mule's chests on the chest flag (`ModelHorse.render`:222-240,`:309-313`).
        assert!(out[horse::SADDLE_BOTTOM].visible);
        assert!(out[horse::FACE_ROPES].visible);
        assert!(!out[horse::LEFT_REIN].visible);
        assert!(
            !out[horse::MULE_LEFT_CHEST].visible,
            "no chest flag draws none"
        );
        // The ears pick by type (`:294-303`): a horse keeps the short pair.
        assert!(out[horse::HORSE_LEFT_EAR].visible);
        assert!(!out[horse::MULE_LEFT_EAR].visible);
        // Bare draws hide the saddle group and leave the rest standing.
        let bare = Pose {
            extra: PoseExtra::Horse {
                saddle: false,
                chested: false,
                adult: true,
                variant: 1,
            },
            ..pose
        };
        let mut out = MODEL_HORSE.rest();
        pose_horse(&bare, &mut out);
        assert!(!out[horse::SADDLE_BOTTOM].visible);
        assert!(!out[horse::FACE_ROPES].visible);
        assert!(
            out[horse::MULE_LEFT_EAR].visible,
            "a donkey takes the long ears"
        );
        assert!(!out[horse::HORSE_LEFT_EAR].visible);
    }

    #[test]
    fn the_wolf_pose_reads_its_sitting_branch() {
        let pose = Pose {
            limb_swing: 1.0,
            limb_swing_amount: 1.0,
            extra: PoseExtra::Wolf {
                tamed: true,
                angry: false,
                sitting: true,
                health: 20.0,
            },
            ..Pose::default()
        };
        let mut out = MODEL_WOLF.rest();
        pose_wolf(&pose, &mut out);
        // Sitting (`ModelWolf.setLivingAnimations`:127-140).
        assert!(close(out[wolf::BODY].angles[0], PI / 4.0));
        assert_eq!(out[wolf::BODY].point, [0.0, 18.0, 0.0]);
        assert_eq!(out[wolf::MANE].point, [-1.0, 16.0, -3.0]);
        assert!(close(out[wolf::MANE].angles[0], PI * 2.0 / 5.0));
        assert_eq!(out[wolf::TAIL].point, [-1.0, 21.0, 6.0]);
        assert!(close(out[wolf::LEG1].angles[0], PI * 3.0 / 2.0));
        assert_eq!(out[wolf::LEG1].point, [-2.5, 22.0, 2.0]);
        assert!(close(out[wolf::LEG3].angles[0], 5.811_947));
        assert_eq!(out[wolf::LEG3].point, [-2.49, 17.0, -4.0]);
        // The tail always turns by the entity's health through the renderer's rotation
        // float (`RenderWolf.handleRotationFloat`:24-27, `EntityWolf.getTailRotation`:493-496):
        // a full-health tamed wolf keeps the resting droop.
        assert!(close(
            out[wolf::TAIL].angles[0],
            wolf_tail_rotation(true, false, 20.0)
        ));
        assert!(close(wolf_tail_rotation(true, false, 20.0), 0.55 * PI));
        // Standing, the legs take the swing in the source's own phase pair and the tail
        // keeps its yaw rule (`ModelWolf.setLivingAnimations`:144-156,`:116-123`).
        let standing = Pose {
            extra: PoseExtra::Wolf {
                tamed: false,
                angry: true,
                sitting: false,
                health: 8.0,
            },
            ..pose
        };
        let mut out = MODEL_WOLF.rest();
        pose_wolf(&standing, &mut out);
        assert!(close(out[wolf::LEG1].angles[0], 1.100_646));
        assert!(close(out[wolf::LEG2].angles[0], -1.100_646));
        assert!(
            close(out[wolf::TAIL].angles[0], 1.539_380_4),
            "an angry wolf"
        );
        assert!(
            out[wolf::TAIL].angles[1].abs() < 1.0e-6,
            "the angry tail holds its yaw"
        );
        assert_eq!(out[wolf::BODY].point, [0.0, 14.0, 2.0]);
        assert_eq!(out[wolf::MANE].point, [-1.0, 14.0, -3.0]);
        let walking = Pose {
            extra: PoseExtra::Wolf {
                tamed: false,
                angry: false,
                sitting: false,
                health: 8.0,
            },
            ..pose
        };
        let mut out = MODEL_WOLF.rest();
        pose_wolf(&walking, &mut out);
        assert!(
            close(out[wolf::TAIL].angles[1], 1.100_646),
            "the wild tail swings on the same swing (`ModelWolf.setLivingAnimations`:122`)"
        );
    }

    #[test]
    fn the_ocelot_states_shift_its_legs() {
        let sitting = Pose {
            extra: PoseExtra::Ocelot { sitting: true },
            ..Pose::default()
        };
        let mut out = MODEL_OCELOT.rest();
        pose_ocelot(&sitting, &mut out);
        // The sit branch (`ModelOcelot.setLivingAnimations`:193-212).
        assert!(close(out[ocelot::BODY].angles[0], PI / 4.0));
        assert_eq!(out[ocelot::BODY].point, [0.0, 8.0, -5.0]);
        assert_eq!(out[ocelot::HEAD].point, [0.0, 11.7, -8.0]);
        assert_eq!(out[ocelot::TAIL].point, [0.0, 23.0, 6.0]);
        assert!(close(out[ocelot::TAIL].angles[0], 1.727_876_1));
        assert!(close(out[ocelot::TAIL2].angles[0], 2.670_354));
        assert!(close(out[ocelot::FRONT_LEFT_LEG].angles[0], -0.157_079_64));
        assert_eq!(out[ocelot::FRONT_LEFT_LEG].point, [1.2, 15.8, -7.0]);
        assert!(close(out[ocelot::BACK_LEFT_LEG].angles[0], -PI / 2.0));
        assert_eq!(out[ocelot::BACK_LEFT_LEG].point, [1.1, 21.0, 1.0]);
        // The sneak state turns the body a quarter and folds both tails (`:174-183`), the
        // walk the body's quarter with the leg swing and the tail's idle sway (`ModelOcelot.setRotationAngles`:122-149`).
        let sneaking = Pose {
            sneak: true,
            limb_swing: 1.0,
            limb_swing_amount: 1.0,
            ..Pose::default()
        };
        let mut out = MODEL_OCELOT.rest();
        pose_ocelot(&sneaking, &mut out);
        assert!(close(out[ocelot::BODY].angles[0], PI / 2.0));
        assert_eq!(out[ocelot::BODY].point, [0.0, 13.0, -10.0]);
        assert!(close(out[ocelot::TAIL].angles[0], PI / 2.0));
        assert!(close(out[ocelot::BACK_LEFT_LEG].angles[0], 0.786_176));
        assert!(close(out[ocelot::BACK_RIGHT_LEG].angles[0], -0.786_176));
        assert!(
            close(
                out[ocelot::TAIL2].angles[0],
                1.727_876_1 + 0.471_238_94 * 1.0_f32.cos()
            ),
            "the tail2 sway the later angle write leaves (`:147`)"
        );
    }

    #[test]
    fn the_rabbit_hop_tilts_its_legs() {
        let hopping = Pose {
            head_yaw: 20.0,
            head_pitch: 10.0,
            extra: PoseExtra::Rabbit { hop: 0.5 },
            ..Pose::default()
        };
        let mut out = MODEL_RABBIT.rest();
        pose_rabbit(&hopping, &mut out);
        // At the hop's crest the thighs throw forward, the feet with them, the arms back
        // (`ModelRabbit.setRotationAngles`:184-187).
        assert!(close(
            out[rabbit::LEFT_THIGH].angles[0],
            29.0 * 0.017_453_292
        ));
        assert!(close(
            out[rabbit::LEFT_FOOT].angles[0],
            50.0 * 0.017_453_292
        ));
        assert!(close(
            out[rabbit::LEFT_ARM].angles[0],
            -51.0 * 0.017_453_292
        ));
        assert!(close(out[rabbit::HEAD].angles[0], 10.0 * 0.017_453_292));
        assert!(close(out[rabbit::HEAD].angles[1], 20.0 * 0.017_453_292));
        assert!(close(
            out[rabbit::RIGHT_EAR].angles[1],
            20.0 * 0.017_453_292 - 0.261_799_4
        ));
        assert!(close(
            out[rabbit::LEFT_EAR].angles[1],
            20.0 * 0.017_453_292 + 0.261_799_4
        ));
        // At rest the legs settle to the class's own standing angles (`:185-187` at zero).
        let resting = Pose {
            extra: PoseExtra::Rabbit { hop: 0.0 },
            ..Pose::default()
        };
        let mut out = MODEL_RABBIT.rest();
        pose_rabbit(&resting, &mut out);
        assert!(close(
            out[rabbit::LEFT_THIGH].angles[0],
            -21.0 * 0.017_453_292
        ));
        assert!(close(out[rabbit::LEFT_FOOT].angles[0], 0.0));
        assert!(close(
            out[rabbit::LEFT_ARM].angles[0],
            -11.0 * 0.017_453_292
        ));
    }

    #[test]
    fn the_ghast_tentacles_sway_on_the_age() {
        let pose = Pose {
            age: 1.0,
            ..Pose::default()
        };
        let mut out = MODEL_GHAST.rest();
        pose_ghast(&pose, &mut out);
        // `0.2 * sin(age * 0.3 + i) + 0.4` (`ModelGhast.setRotationAngles`:43).
        assert!(close(
            out[ghast::TENTACLE_0].angles[0],
            0.2 * (0.3_f32).sin() + 0.4
        ));
        assert!(close(
            out[ghast::TENTACLE_0 + 1].angles[0],
            0.2 * (1.3_f32).sin() + 0.4
        ));
        assert!(close(
            out[ghast::TENTACLE_0 + 8].angles[0],
            0.2 * (1.0 * 0.3 + 8.0_f32).sin() + 0.4
        ));
        // The model's whole float rides the render's own shift (`ModelGhast.render`:52-60):
        // six tenths of a block up, on every part.
        for rot in &out {
            assert_eq!(rot.offset, [0.0, 0.6, 0.0]);
        }
    }

    #[test]
    fn the_blaze_rods_orbit_the_head() {
        let pose = Pose {
            age: 2.0,
            head_yaw: 30.0,
            head_pitch: -10.0,
            ..Pose::default()
        };
        let mut out = MODEL_BLAZE.rest();
        pose_blaze(&pose, &mut out);
        // The rod orbits: `f = age * PI * -0.1`, per rod a unit of phase on; the first
        // ring at radius nine and two units under, one height step a rod
        // (`ModelBlaze.setRotationAngles`:45-53).
        let f = 2.0 * PI * -0.1;
        assert!(close(out[blaze::ROD_0].point[0], f.cos() * 9.0));
        assert!(close(out[blaze::ROD_0].point[2], f.sin() * 9.0));
        assert!(close(out[blaze::ROD_0].point[1], -2.0 + (0.5_f32).cos()));
        assert!(close(out[blaze::ROD_0 + 1].point[0], (f + 1.0).cos() * 9.0));
        assert!(
            close(out[blaze::ROD_0 + 1].point[1], -2.0 + (1.0_f32).cos()),
            "the second rod's height steps two a rod (`:49`)"
        );
        // The second ring at radius seven from a quarter turn (`:55-63`), the third at
        // radius five, eleven units up (`:65-73`).
        let g = PI / 4.0 + 2.0 * PI * 0.03;
        assert!(close(out[blaze::ROD_0 + 4].point[0], g.cos() * 7.0));
        assert!(close(out[blaze::ROD_0 + 4].point[2], g.sin() * 7.0));
        assert!(close(out[blaze::ROD_0 + 4].point[1], 2.0 + (2.5_f32).cos()));
        let h = 0.471_238_94 + 2.0 * PI * -0.05;
        assert!(close(out[blaze::ROD_0 + 8].point[0], h.cos() * 5.0));
        assert!(close(
            out[blaze::ROD_0 + 8].point[1],
            11.0 + (7.0_f32).cos()
        ));
        // The head takes the frame's angles (`:75-76`) and the rods never tilt: their
        // whole motion is the point orbit.
        assert!(close(out[blaze::HEAD].angles[0], -10.0_f32.to_radians()));
        assert!(close(out[blaze::HEAD].angles[1], 30.0_f32.to_radians()));
        for rot in &out[blaze::ROD_0..blaze::ROD_0 + 12] {
            assert_eq!(rot.angles, [0.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn the_guardian_spines_ride_their_rings() {
        let pose = Pose {
            age: 0.0,
            head_yaw: 15.0,
            head_pitch: 5.0,
            extra: PoseExtra::Guardian {
                spikes: 1.0,
                tail_phase: 0.0,
            },
            ..Pose::default()
        };
        let mut out = MODEL_GUARDIAN.rest();
        pose_guardian(&pose, &mut out);
        // The body takes the frame's angles (`ModelGuardian.setRotationAngles`:74-75).
        assert!(close(out[guardian::BODY].angles[0], 5.0_f32.to_radians()));
        assert!(close(out[guardian::BODY].angles[1], 15.0_f32.to_radians()));
        // The spines sit on their twelve-seat rings, their reach breathing a hundredth of
        // the extension (`:82-92`): the first is a long ring of its own.
        assert!(close(out[guardian::SPINE_0].angles[0], 1.75 * PI));
        assert!(close(out[guardian::SPINE_0].angles[1], 0.0));
        assert!(close(out[guardian::SPINE_0].point[1], 16.0 - 8.0 * 1.01));
        assert!(close(out[guardian::SPINE_0].point[2], 8.0 * 1.01));
        assert!(close(out[guardian::SPINE_0 + 2].angles[2], 0.25 * PI));
        assert!(close(
            out[guardian::SPINE_0 + 2].point[0],
            8.0 * (1.0 + (2.0_f32).cos() * 0.01)
        ));
        // The eye rides its own fixed depth and the constructor's height, the branch the
        // source takes with no target in view (`:94-124`).
        assert_eq!(out[guardian::EYE].point, [0.0, 15.0, -8.25]);
        // The tail's chain hangs at rest with the phase pinned (`:126-134`).
        assert!(close(out[guardian::TAIL_0].angles[1], 0.0));
        assert_eq!(out[guardian::TAIL_1].point, [-1.5, 0.5, 14.0]);
        assert_eq!(out[guardian::TAIL_2].point, [0.5, 0.5, 6.0]);
    }

    #[test]
    fn the_dragon_wing_flap_reads_the_interpolated_anim_time() {
        let pose_at = |anim_time: f32| Pose {
            extra: PoseExtra::Dragon { anim_time },
            ..Pose::default()
        };
        let at = |anim_time: f32| {
            let mut out = MODEL_DRAGON.rest();
            pose_dragon(&pose_at(anim_time), &mut out);
            out
        };
        // A quarter of the flap's turn: the wings level, the tips at their extremes and
        // the legs a tenth of the bob on (`ModelDragon.render`:193-202).
        let quarter = at(0.25);
        assert!(close(quarter[dragon::WING_LEFT].angles[0], 0.125));
        assert!(close(quarter[dragon::WING_LEFT].angles[1], 0.25));
        assert!(close(quarter[dragon::WING_LEFT].angles[2], 0.9));
        assert!(close(
            quarter[dragon::WING_LEFT_TIP].angles[2],
            -((f32::sin(PI / 2.0 + 2.0) + 0.5) * 0.75)
        ));
        assert!(close(quarter[dragon::REAR_LEFT_LEG].angles[0], 1.027_266));
        assert!(close(quarter[dragon::FRONT_LEFT_LEG].angles[0], 1.327_266));
        assert!(close(quarter[dragon::FRONT_LEFT_TIP].angles[0], -0.527_266));
        // The right side mirrors the left: the same angles' y and z negate and the
        // renderer draws its boxes flipped (`ModelDragon.java`:206).
        assert!(close(
            quarter[dragon::WING_RIGHT].angles[0],
            quarter[dragon::WING_LEFT].angles[0]
        ));
        assert!(close(
            quarter[dragon::WING_RIGHT].angles[1],
            -quarter[dragon::WING_LEFT].angles[1]
        ));
        assert!(close(
            quarter[dragon::WING_RIGHT].angles[2],
            -quarter[dragon::WING_LEFT].angles[2]
        ));
        // The jaw opens with the same wave (`:143`).
        assert!(close(
            quarter[dragon::JAW].angles[0],
            ((PI / 2.0_f32).sin() + 1.0) * 0.2
        ));
        // At the wave's start the wings fold to their own extremes, the jaw half opens and
        // the flight's own model-level translate and pitch read the same wave (`:144-146`,
        // `:193-196`): the flight is no part's own offset — it rides the draw chain ahead
        // of every part — and its values are the model's own.
        let zero = at(0.0);
        assert!(close(zero[dragon::WING_LEFT].angles[0], -0.075));
        assert!(close(zero[dragon::WING_LEFT].angles[2], 0.1));
        assert!(close(
            zero[dragon::WING_LEFT_TIP].angles[2],
            -((f32::sin(2.0) + 0.5) * 0.75)
        ));
        assert!(close(zero[dragon::JAW].angles[0], 0.2));
        for (slot, rot) in zero.iter().enumerate() {
            assert_eq!(
                rot.offset, [0.0; 3],
                "part {slot} carries no offset of its own: the flight is model-level (`ModelDragon.render`:144-147)"
            );
        }
        // The flight's own model values at the wave's start and a quarter on: the
        // translate the chain composes ahead of every part and the pitch it turns by
        // (`:144-147`).
        let start = dragon_flight(&pose_at(0.0));
        assert!(close(start.0[0], 0.0));
        assert!(close(start.0[1], 0.017_109_5 - 2.0));
        assert!(close(start.0[2], -3.0));
        assert!(close(start.1, 0.034_218_95));
        let quarter_flight = dragon_flight(&pose_at(0.25));
        assert!(close(quarter_flight.0[1], 0.272_656_8 - 2.0));
        assert!(close(quarter_flight.1, 0.545_313_6));
        // The neck chain steps ten units a spine (`:166-172`): the first pair land on the
        // source's own chain values.
        assert_eq!(zero[dragon::NECK_0].point, [0.0, 20.0, -12.0]);
        assert!(close(zero[dragon::NECK_0].angles[0], 0.15));
        assert!(close(
            zero[dragon::NECK_0 + 1].angles[0],
            f32::cos(0.45) * 0.15
        ));
        assert!(close(
            zero[dragon::NECK_0 + 4].angles[0],
            f32::cos(1.8) * 0.15
        ));
        // The tail's spines trail a half turn a segment at rest (`:228`).
        assert!(close(zero[dragon::TAIL_0].angles[1], PI));
        assert!(close(zero[dragon::TAIL_0 + 1].angles[1], PI));
        assert_eq!(zero[dragon::TAIL_0].point, [0.0, 10.0, 60.0]);
    }

    #[test]
    fn the_wither_cage_bobs_on_the_age() {
        let pose = Pose {
            age: 0.0,
            head_yaw: 10.0,
            head_pitch: 5.0,
            ..Pose::default()
        };
        let mut out = MODEL_WITHER.rest();
        pose_wither(&pose, &mut out);
        // The middle rib swings by a sixteenth of a turn a second (its cosine at age zero)
        // and the third hangs ten units off it (`ModelWither.setRotationAngles`:66-69);
        // the heads turn with the frame (`:70-71`).
        assert!(close(out[wither::RIB_1].angles[0], (0.065 + 0.05) * PI));
        let swing = out[wither::RIB_1].angles[0];
        assert_eq!(
            out[wither::RIB_2].point,
            [-2.0, 6.9 + swing.cos() * 10.0, -0.5 + swing.sin() * 10.0]
        );
        assert!(close(out[wither::RIB_2].angles[0], (0.265 + 0.1) * PI));
        assert!(close(
            out[wither::HEAD_CENTRE].angles[0],
            5.0_f32.to_radians()
        ));
        assert!(close(
            out[wither::HEAD_CENTRE].angles[1],
            10.0_f32.to_radians()
        ));
    }

    #[test]
    fn the_wolf_sheets_follow_the_taming() {
        // The tamed coat draws first, the angry one only for a wild wolf that is angry
        // (`RenderWolf.getEntityTexture`:46-49).
        assert_eq!(wolf_sheet(true, false), "entity/wolf/wolf_tame.png");
        assert_eq!(wolf_sheet(true, true), "entity/wolf/wolf_tame.png");
        assert_eq!(wolf_sheet(false, true), "entity/wolf/wolf_angry.png");
        assert_eq!(wolf_sheet(false, false), "entity/wolf/wolf.png");
        // The tail's own angles (`EntityWolf.getTailRotation`:493-496).
        assert!(close(wolf_tail_rotation(false, true, 20.0), 1.539_380_4));
        assert!(close(wolf_tail_rotation(true, false, 20.0), 0.55 * PI));
        assert!(close(wolf_tail_rotation(true, false, 10.0), 0.35 * PI));
        assert!(close(wolf_tail_rotation(false, false, 20.0), PI / 5.0));
    }

    #[test]
    fn the_wolf_collar_tints_by_the_wool_palette() {
        // The collar layer draws only for a tamed wolf (`LayerWolfCollar.doRenderLayer`:22)
        // and tints from the wool table through the dye-damage decode
        // (`EntityWolf.getCollarColor`:540-543): the stored byte is the dye's damage value,
        // which counts down against the table's metadata.
        let layers = layers::layers_for(ModelRef::Wolf {
            tamed: true,
            collar: 14,
            angry: false,
        });
        assert_eq!(layers.len(), 1, "the collar layer");
        let collar = &layers[0];
        assert_eq!(collar.texture, "entity/wolf/wolf_collar.png");
        assert_eq!(collar.blend, layers::Blend::Opaque);
        assert_eq!(collar.texture_size, [64.0, 32.0]);
        let extra = DrawExtra::Wolf {
            tamed: true,
            collar: 14,
        };
        assert!((collar.active)(&extra), "a tamed wolf's collar draws");
        // A freshly spawned wolf stores the red dye's damage byte, 14, which decodes to
        // the orange seat of the table — the source's own default (MC-71674), kept.
        assert_eq!(collar.tint.rgb(&extra), WOOL_COLOURS[1]);
        assert_eq!(
            collar.tint.rgb(&extra),
            [0.85, 0.5, 0.2],
            "the default orange"
        );
        let red = DrawExtra::Wolf {
            tamed: true,
            collar: 1,
        };
        assert_eq!(collar.tint.rgb(&red), WOOL_COLOURS[14]);
        let untamed = DrawExtra::Wolf {
            tamed: false,
            collar: 4,
        };
        assert!(
            !(collar.active)(&untamed),
            "a wild wolf's collar never draws"
        );
        // The byte folds to its nibble before the decode, as the source's own read does.
        let odd = DrawExtra::Wolf {
            tamed: true,
            collar: 0xff,
        };
        assert_eq!(collar.tint.rgb(&odd), WOOL_COLOURS[0]);
    }

    #[test]
    fn the_variant_sheets_follow_the_sources_tables() {
        // The horse's colours (`EntityHorse.setHorseTexturePaths`:722-733 over the table at
        // `:56`), the type sheets first (`RenderHorse.getEntityTexture`:53-78).
        assert_eq!(horse_sheet(0, 0), "entity/horse/horse_white.png");
        assert_eq!(horse_sheet(0, 1), "entity/horse/horse_creamy.png");
        assert_eq!(horse_sheet(0, 2), "entity/horse/horse_chestnut.png");
        assert_eq!(horse_sheet(0, 3), "entity/horse/horse_brown.png");
        assert_eq!(horse_sheet(0, 4), "entity/horse/horse_black.png");
        assert_eq!(horse_sheet(0, 5), "entity/horse/horse_gray.png");
        assert_eq!(horse_sheet(0, 6), "entity/horse/horse_darkbrown.png");
        assert_eq!(horse_sheet(1, 0), "entity/horse/donkey.png");
        assert_eq!(horse_sheet(2, 0), "entity/horse/mule.png");
        assert_eq!(horse_sheet(3, 0), "entity/horse/horse_zombie.png");
        assert_eq!(horse_sheet(4, 0), "entity/horse/horse_skeleton.png");
        // Off the table the source resolves no sheet at all (`EntityHorse.java`:707-710, `RenderHorse.java`:84-87`).
        assert_eq!(horse_sheet(0, 7), "");
        assert_eq!(horse_sheet(5, 0), "");
        // The markings and armour, empty where the class's table holds no file.
        assert_eq!(horse_marking(0), "");
        assert_eq!(horse_marking(1), "entity/horse/horse_markings_white.png");
        assert_eq!(
            horse_marking(2),
            "entity/horse/horse_markings_whitefield.png"
        );
        assert_eq!(
            horse_marking(3),
            "entity/horse/horse_markings_whitedots.png"
        );
        assert_eq!(
            horse_marking(4),
            "entity/horse/horse_markings_blackdots.png"
        );
        assert_eq!(horse_marking(5), "");
        assert_eq!(horse_armour(0), "");
        assert_eq!(horse_armour(1), "entity/horse/armor/horse_armor_iron.png");
        assert_eq!(horse_armour(2), "entity/horse/armor/horse_armor_gold.png");
        assert_eq!(
            horse_armour(3),
            "entity/horse/armor/horse_armor_diamond.png"
        );
        assert_eq!(horse_armour(4), "");
        // The rabbit's coats and the killer rabbit (`RenderRabbit.getEntityTexture`:41-62).
        assert_eq!(rabbit_sheet(0), "entity/rabbit/brown.png");
        assert_eq!(rabbit_sheet(1), "entity/rabbit/white.png");
        assert_eq!(rabbit_sheet(2), "entity/rabbit/black.png");
        assert_eq!(rabbit_sheet(3), "entity/rabbit/white_splotched.png");
        assert_eq!(rabbit_sheet(4), "entity/rabbit/gold.png");
        assert_eq!(rabbit_sheet(5), "entity/rabbit/salt.png");
        assert_eq!(rabbit_sheet(99), "entity/rabbit/caerbannog.png");
        assert_eq!(rabbit_sheet(7), "entity/rabbit/brown.png", "the default");
        // The ocelot's wild coat and its tamed skins (`RenderOcelot.getEntityTexture`:23-40).
        assert_eq!(ocelot_sheet(0), "entity/cat/ocelot.png");
        assert_eq!(ocelot_sheet(1), "entity/cat/black.png");
        assert_eq!(ocelot_sheet(2), "entity/cat/red.png");
        assert_eq!(ocelot_sheet(3), "entity/cat/siamese.png");
        assert_eq!(ocelot_sheet(9), "entity/cat/ocelot.png", "the default");
    }

    #[test]
    fn the_death_tilts_follow_the_renderers() {
        // None of the nine renderers overrides `rotateCorpse` or `getDeathMaxRotation`
        // except the dragon, whose own corpse path still tilts by the base renderer's
        // ninety (`RenderDragon.rotateCorpse`:41-52 over `RendererLivingEntity.getDeathMaxRotation`:473-476),
        // and none rolls (`RenderIronGolem`'s lean is its own). Every one of the nine
        // resolves through the base pair.
        let kinds = [
            ModelRef::Horse {
                variant: 0,
                colour: 0,
                markings: 0,
                saddle: true,
                armour: 0,
            },
            ModelRef::Wolf {
                tamed: true,
                collar: 14,
                angry: false,
            },
            ModelRef::Ocelot {
                variant: 1,
                child: false,
                tamed: false,
            },
            ModelRef::Rabbit {
                variant: 0,
                child: false,
            },
            ModelRef::Ghast { shooting: false },
            ModelRef::Blaze,
            ModelRef::Guardian { elder: false },
            ModelRef::EnderDragon,
            ModelRef::Wither { invul_time: 0 },
        ];
        let pose = Pose::default();
        for kind in kinds {
            assert_eq!(death_rotation(kind), 90.0, "{kind:?} tilts the base ninety");
            assert_eq!(corpse_roll(kind, &pose), 0.0, "{kind:?} adds no roll");
            assert_eq!(corpse_shift(kind, &pose), 0.0, "{kind:?} shifts none");
        }
    }
}
