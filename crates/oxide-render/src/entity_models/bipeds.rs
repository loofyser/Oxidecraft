//! The biped family: the class of models whose parts hang off a head, a body and two limbs —
//! the zombie, the skeleton, the zombie villager, the villager and the witch, the snow golem
//! and the iron golem, and the giant, which is the zombie's own model drawn six times over
//! (`RenderGiantZombie`'s pre-render scale; `RenderManager.java`:162).
//!
//! The zombie, the skeleton and the zombie villager build on `ModelBiped`'s table and pose:
//! the head, the headwear over it, the body and the two four-wide limbs at
//! `ModelBiped.java`:55-77, with the arms and legs swinging on the walk
//! (`ModelBiped.setRotationAngles`:129-140). Over that base, `ModelZombie` lays the raised
//! arms — the arm angles written outright, the base's own arm swing overwritten
//! (`ModelZombie.setRotationAngles`:31-44), `ModelSkeleton` swaps the limbs for the two-wide
//! pair (`ModelSkeleton.java`:20-33) and reads the wither type into the aim flag
//! (`ModelSkeleton.setLivingAnimations`:43), and `ModelZombieVillager` swaps the head for the
//! two-box villager one (`ModelZombieVillager.java`:25-28).
//!
//! The villager and the witch build their own tables (`ModelVillager.java`:31-53) and pose
//! (`ModelVillager.setRotationAngles`:76-84 — the arms held across and down, the legs at half
//! the biped's swing); the witch hangs its wart under the nose and its four-piece hat over
//! the head (`ModelWitch.java`:9-39) and sways the nose on the entity's own id, dropping it
//! into the hold state while it holds an item (`ModelWitch.setRotationAngles`:50-61). The
//! witch's own renderer scales the villager table `0.9375` like the villager does
//! (`RenderWitch.preRenderCallback`:47-48).
//!
//! The snow golem and the iron golem are tables of their own: three snow boxes with two
//! outstretched hands whose spread follows the head's turn (`ModelSnowMan.java`:18-32,
//! `setRotationAngles`:43-55), and the golem's overlapping plates whose legs step on the
//! folded thirteen-tick wave and whose arms sway on it in the walk
//! (`ModelIronGolem.java`:41-61, `setRotationAngles`:85-90, `setLivingAnimations`:117-120).
//!
//! Model units are the source's: 1/16 metre, pivots in the same units, angles radians. The
//! held-item terms of `ModelBiped.setRotationAngles` (the `heldItemLeft`/`heldItemRight`
//! blocks) carry no input this milestone — a mob's equipment is not worn — and the zombie's
//! conversion lean (`RenderZombie.rotateCorpse`:87-95) reads a conversion state no frame
//! input carries; both are recorded limits of this milestone.

use super::{Box, Model, Part, Pose, PoseExtra, Rot};

/// The biped table's head: `ModelBiped.java`:55-57, model size zero.
static HEAD: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -8.0, -4.0],
        size: [8.0, 8.0, 8.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The biped table's headwear: the same cube grown half a texel closed, read from the
/// sheet's own hat cell (`ModelBiped.java`:58-60). The mob sheets leave the cell
/// transparent, so it rasterises nothing, exactly as the source draws it
/// (`ModelBiped.render`:118).
static HEADWEAR: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -8.0, -4.0],
        size: [8.0, 8.0, 8.0],
        uv: [32.0, 0.0],
        inflate: 0.5,
        mirror: false,
    }],
    children: &[],
};

/// The biped table's body (`ModelBiped.java`:61-63).
static BODY: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, 0.0, -2.0],
        size: [8.0, 12.0, 4.0],
        uv: [16.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The biped table's four-wide right arm (`ModelBiped.java`:64-66).
static RIGHT_ARM: Part = Part {
    point: [-5.0, 2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, -2.0, -2.0],
        size: [4.0, 12.0, 4.0],
        uv: [40.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The biped table's four-wide left arm, mirrored (`ModelBiped.java`:67-70).
static LEFT_ARM: Part = Part {
    point: [5.0, 2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -2.0, -2.0],
        size: [4.0, 12.0, 4.0],
        uv: [40.0, 16.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The biped table's right leg (`ModelBiped.java`:71-73).
static RIGHT_LEG: Part = Part {
    point: [-1.9, 12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, 0.0, -2.0],
        size: [4.0, 12.0, 4.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The biped table's left leg, mirrored (`ModelBiped.java`:74-77).
static LEFT_LEG: Part = Part {
    point: [1.9, 12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, 0.0, -2.0],
        size: [4.0, 12.0, 4.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The zombie's, the skeleton's and the giant's parts, in draw order
/// (`ModelBiped.render`:112-118).
static ZOMBIE_PARTS: [Part; 7] = [
    HEAD, BODY, RIGHT_ARM, LEFT_ARM, RIGHT_LEG, LEFT_LEG, HEADWEAR,
];

/// The skeleton's two-wide right arm (`ModelSkeleton.java`:20-22).
static THIN_RIGHT_ARM: Part = Part {
    point: [-5.0, 2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -2.0, -1.0],
        size: [2.0, 12.0, 2.0],
        uv: [40.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The skeleton's two-wide left arm, mirrored (`ModelSkeleton.java`:23-26).
static THIN_LEFT_ARM: Part = Part {
    point: [5.0, 2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -2.0, -1.0],
        size: [2.0, 12.0, 2.0],
        uv: [40.0, 16.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The skeleton's two-wide right leg (`ModelSkeleton.java`:27-29).
static THIN_RIGHT_LEG: Part = Part {
    point: [-2.0, 12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [2.0, 12.0, 2.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The skeleton's two-wide left leg, mirrored (`ModelSkeleton.java`:30-33).
static THIN_LEFT_LEG: Part = Part {
    point: [2.0, 12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [2.0, 12.0, 2.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The skeleton's parts: the biped table with the thin limbs in place.
static SKELETON_PARTS: [Part; 7] = [
    HEAD,
    BODY,
    THIN_RIGHT_ARM,
    THIN_LEFT_ARM,
    THIN_RIGHT_LEG,
    THIN_LEFT_LEG,
    HEADWEAR,
];

/// The zombie villager's two-box head: the tall bare skull and the nose
/// (`ModelZombieVillager.java`:25-28).
static ZOMBIE_VILLAGER_HEAD: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-4.0, -10.0, -4.0],
            size: [8.0, 10.0, 8.0],
            uv: [0.0, 32.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-1.0, -3.0, -6.0],
            size: [2.0, 4.0, 2.0],
            uv: [24.0, 32.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The zombie villager's parts: the biped table with its own head.
static ZOMBIE_VILLAGER_PARTS: [Part; 7] = [
    ZOMBIE_VILLAGER_HEAD,
    BODY,
    RIGHT_ARM,
    LEFT_ARM,
    RIGHT_LEG,
    LEFT_LEG,
    HEADWEAR,
];

/// The villager's nose, a child of the head (`ModelVillager.java`:34-37).
static VILLAGER_NOSE: Part = Part {
    point: [0.0, -2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -1.0, -6.0],
        size: [2.0, 4.0, 2.0],
        uv: [24.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The villager's head with the nose under it (`ModelVillager.java`:31-37).
static VILLAGER_HEAD: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -10.0, -4.0],
        size: [8.0, 10.0, 8.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[VILLAGER_NOSE],
};

/// The villager's body: the robe and its own outer layer (`ModelVillager.java`:38-41).
static VILLAGER_BODY: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-4.0, 0.0, -3.0],
            size: [8.0, 12.0, 6.0],
            uv: [16.0, 20.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-4.0, 0.0, -3.0],
            size: [8.0, 18.0, 6.0],
            uv: [0.0, 38.0],
            inflate: 0.5,
            mirror: false,
        },
    ],
    children: &[],
};

/// The villager's right leg (`ModelVillager.java`:47-49).
static VILLAGER_RIGHT_LEG: Part = Part {
    point: [-2.0, 12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, 0.0, -2.0],
        size: [4.0, 12.0, 4.0],
        uv: [0.0, 22.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The villager's left leg, mirrored (`ModelVillager.java`:50-53).
static VILLAGER_LEFT_LEG: Part = Part {
    point: [2.0, 12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, 0.0, -2.0],
        size: [4.0, 12.0, 4.0],
        uv: [0.0, 22.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The villager's arms: both sleeves and the crossed bar between them
/// (`ModelVillager.java`:42-46).
static VILLAGER_ARMS: Part = Part {
    point: [0.0, 2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-8.0, -2.0, -2.0],
            size: [4.0, 8.0, 4.0],
            uv: [44.0, 22.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [4.0, -2.0, -2.0],
            size: [4.0, 8.0, 4.0],
            uv: [44.0, 22.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-4.0, 2.0, -2.0],
            size: [8.0, 4.0, 4.0],
            uv: [40.0, 38.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The villager's parts, in draw order (`ModelVillager.render`:62-66).
static VILLAGER_PARTS: [Part; 5] = [
    VILLAGER_HEAD,
    VILLAGER_BODY,
    VILLAGER_RIGHT_LEG,
    VILLAGER_LEFT_LEG,
    VILLAGER_ARMS,
];

/// The witch's wart, a child of the nose (`ModelWitch.java`:15-17).
static WITCH_WART: Part = Part {
    point: [0.0, -2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [0.0, 3.0, -6.75],
        size: [1.0, 1.0, 1.0],
        uv: [0.0, 0.0],
        inflate: -0.25,
        mirror: false,
    }],
    children: &[],
};

/// The witch's nose with the wart under it (`ModelWitch.java`:15-17).
static WITCH_NOSE: Part = Part {
    point: [0.0, -2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -1.0, -6.0],
        size: [2.0, 4.0, 2.0],
        uv: [24.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[WITCH_WART],
};

/// The hat's tip, the innermost of the funnel's four pieces (`ModelWitch.java`:34-39).
static WITCH_TIP: Part = Part {
    point: [1.75, -2.0, 2.0],
    rest: [-0.20943952, 0.0, 0.10471976],
    boxes: &[Box {
        origin: [0.0, 0.0, 0.0],
        size: [1.0, 2.0, 1.0],
        uv: [0.0, 95.0],
        inflate: 0.25,
        mirror: false,
    }],
    children: &[],
};

/// The hat's third piece (`ModelWitch.java`:28-33).
static WITCH_HAT_THIRD: Part = Part {
    point: [1.75, -4.0, 2.0],
    rest: [-0.10471976, 0.0, 0.05235988],
    boxes: &[Box {
        origin: [0.0, 0.0, 0.0],
        size: [4.0, 4.0, 4.0],
        uv: [0.0, 87.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[WITCH_TIP],
};

/// The hat's second piece (`ModelWitch.java`:22-27).
static WITCH_HAT_SECOND: Part = Part {
    point: [1.75, -4.0, 2.0],
    rest: [-0.05235988, 0.0, 0.02617994],
    boxes: &[Box {
        origin: [0.0, 0.0, 0.0],
        size: [7.0, 4.0, 7.0],
        uv: [0.0, 76.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[WITCH_HAT_THIRD],
};

/// The hat's brim (`ModelWitch.java`:18-21).
static WITCH_HAT: Part = Part {
    point: [-5.0, -10.03125, -5.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [0.0, 0.0, 0.0],
        size: [10.0, 2.0, 10.0],
        uv: [0.0, 64.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[WITCH_HAT_SECOND],
};

/// The witch's head, its nose and the hat over it (`ModelWitch.java`:18-21).
static WITCH_HEAD: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -10.0, -4.0],
        size: [8.0, 10.0, 8.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[WITCH_NOSE, WITCH_HAT],
};

/// The witch's parts: the villager's table with the wart and the hat in it
/// (`ModelWitch.render` inherits the villager's draw order).
static WITCH_PARTS: [Part; 5] = [
    WITCH_HEAD,
    VILLAGER_BODY,
    VILLAGER_RIGHT_LEG,
    VILLAGER_LEFT_LEG,
    VILLAGER_ARMS,
];

/// The snow golem's head, a cube shrunk half a texel (`ModelSnowMan.java`:18-20).
static SNOW_HEAD: Part = Part {
    point: [0.0, 4.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -8.0, -4.0],
        size: [8.0, 8.0, 8.0],
        uv: [0.0, 0.0],
        inflate: -0.5,
        mirror: false,
    }],
    children: &[],
};

/// The snow golem's middle, `ModelSnowMan.java`:27-29.
static SNOW_BODY: Part = Part {
    point: [0.0, 13.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-5.0, -10.0, -5.0],
        size: [10.0, 10.0, 10.0],
        uv: [0.0, 16.0],
        inflate: -0.5,
        mirror: false,
    }],
    children: &[],
};

/// The snow golem's base, `ModelSnowMan.java`:30-32.
static SNOW_BOTTOM: Part = Part {
    point: [0.0, 24.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-6.0, -12.0, -6.0],
        size: [12.0, 12.0, 12.0],
        uv: [0.0, 36.0],
        inflate: -0.5,
        mirror: false,
    }],
    children: &[],
};

/// The snow golem's right hand, one long outstretched arm (`ModelSnowMan.java`:21-23).
static SNOW_RIGHT_HAND: Part = Part {
    point: [0.0, 6.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [12.0, 2.0, 2.0],
        uv: [32.0, 0.0],
        inflate: -0.5,
        mirror: false,
    }],
    children: &[],
};

/// The snow golem's left hand (`ModelSnowMan.java`:24-26).
static SNOW_LEFT_HAND: Part = Part {
    point: [0.0, 6.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [12.0, 2.0, 2.0],
        uv: [32.0, 0.0],
        inflate: -0.5,
        mirror: false,
    }],
    children: &[],
};

/// The snow golem's parts, in draw order (`ModelSnowMan.render`:64-68).
static SNOW_GOLEM_PARTS: [Part; 5] = [
    SNOW_BODY,
    SNOW_BOTTOM,
    SNOW_HEAD,
    SNOW_RIGHT_HAND,
    SNOW_LEFT_HAND,
];

/// The iron golem's head: the great cube and the snout (`ModelIronGolem.java`:41-44).
static GOLEM_HEAD: Part = Part {
    point: [0.0, -7.0, -2.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-4.0, -12.0, -5.5],
            size: [8.0, 10.0, 8.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-1.0, -5.0, -7.5],
            size: [2.0, 4.0, 2.0],
            uv: [24.0, 0.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The iron golem's torso and its shoulder plate (`ModelIronGolem.java`:45-48).
static GOLEM_BODY: Part = Part {
    point: [0.0, -7.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-9.0, -2.0, -6.0],
            size: [18.0, 12.0, 11.0],
            uv: [0.0, 40.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-4.5, 10.0, -3.0],
            size: [9.0, 5.0, 6.0],
            uv: [0.0, 70.0],
            inflate: 0.5,
            mirror: false,
        },
    ],
    children: &[],
};

/// The iron golem's left leg (`ModelIronGolem.java`:55-57).
static GOLEM_LEFT_LEG: Part = Part {
    point: [-4.0, 11.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.5, -3.0, -3.0],
        size: [6.0, 16.0, 5.0],
        uv: [37.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The iron golem's right leg, mirrored (`ModelIronGolem.java`:58-61).
static GOLEM_RIGHT_LEG: Part = Part {
    point: [5.0, 11.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.5, -3.0, -3.0],
        size: [6.0, 16.0, 5.0],
        uv: [60.0, 0.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The iron golem's right arm, the long plate (`ModelIronGolem.java`:49-51).
static GOLEM_RIGHT_ARM: Part = Part {
    point: [0.0, -7.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-13.0, -2.5, -3.0],
        size: [4.0, 30.0, 6.0],
        uv: [60.0, 21.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The iron golem's left arm (`ModelIronGolem.java`:52-54).
static GOLEM_LEFT_ARM: Part = Part {
    point: [0.0, -7.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [9.0, -2.5, -3.0],
        size: [4.0, 30.0, 6.0],
        uv: [60.0, 58.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The iron golem's parts, in draw order (`ModelIronGolem.render`:70-75).
static IRON_GOLEM_PARTS: [Part; 6] = [
    GOLEM_HEAD,
    GOLEM_BODY,
    GOLEM_LEFT_LEG,
    GOLEM_RIGHT_LEG,
    GOLEM_RIGHT_ARM,
    GOLEM_LEFT_ARM,
];

/// The zombie's model: `ModelZombie` over `ModelBiped`, texture `64` by `64`
/// (`ModelZombie.java`:20).
pub static MODEL_ZOMBIE: Model = Model {
    parts: &ZOMBIE_PARTS,
};

/// The skeleton's model: the two-wide limbs on the biped table
/// (`ModelSkeleton.java`:20-33), texture `64` by `32` (`ModelSkeleton.java`:16).
pub static MODEL_SKELETON: Model = Model {
    parts: &SKELETON_PARTS,
};

/// The zombie villager's model: its own head over the biped's limbs
/// (`ModelZombieVillager.java`:25-28), texture `64` by `64`.
pub static MODEL_ZOMBIE_VILLAGER: Model = Model {
    parts: &ZOMBIE_VILLAGER_PARTS,
};

/// The villager's model: `ModelVillager`'s table, texture `64` by `64`.
pub static MODEL_VILLAGER: Model = Model {
    parts: &VILLAGER_PARTS,
};

/// The witch's model: the villager's table with the wart and the hat
/// (`ModelWitch.java`:9-39), texture `64` by `128`.
pub static MODEL_WITCH: Model = Model {
    parts: &WITCH_PARTS,
};

/// The snow golem's model, texture `64` by `64` (`ModelSnowMan.java`:18-32).
pub static MODEL_SNOW_GOLEM: Model = Model {
    parts: &SNOW_GOLEM_PARTS,
};

/// The iron golem's model, texture `128` by `128` (`ModelIronGolem.java`:39-40).
pub static MODEL_IRON_GOLEM: Model = Model {
    parts: &IRON_GOLEM_PARTS,
};

/// The zombies' and the giant's part slots, in draw order.
mod slot {
    /// The head.
    pub const P_HEAD: usize = 0;
    /// The body.
    pub const P_BODY: usize = 1;
    /// The right arm.
    pub const P_RIGHT_ARM: usize = 2;
    /// The left arm.
    pub const P_LEFT_ARM: usize = 3;
    /// The right leg.
    pub const P_RIGHT_LEG: usize = 4;
    /// The left leg.
    pub const P_LEFT_LEG: usize = 5;
    /// The headwear.
    pub const P_HEADWEAR: usize = 6;
    /// The villager's and the witch's arms.
    pub const P_ARMS: usize = 4;
    /// The villager's and the witch's right leg.
    pub const P_RIGHT_VILLAGER_LEG: usize = 2;
    /// The villager's and the witch's left leg.
    pub const P_LEFT_VILLAGER_LEG: usize = 3;
    /// The witch's nose, the head's first child.
    pub const P_WITCH_NOSE: usize = 1;
    /// The snow golem's head and middle.
    pub const P_SNOW_BODY: usize = 0;
    /// The snow golem's base.
    pub const P_SNOW_HEAD: usize = 2;
    /// The snow golem's right hand.
    pub const P_SNOW_RIGHT_HAND: usize = 3;
    /// The snow golem's left hand.
    pub const P_SNOW_LEFT_HAND: usize = 4;
    /// The iron golem's legs and arms.
    pub const P_GOLEM_LEFT_LEG: usize = 2;
    /// The iron golem's right leg.
    pub const P_GOLEM_RIGHT_LEG: usize = 3;
    /// The iron golem's right arm.
    pub const P_GOLEM_RIGHT_ARM: usize = 4;
    /// The iron golem's left arm.
    pub const P_GOLEM_LEFT_ARM: usize = 5;
}

use slot::*;

/// The shared `ModelBiped.setRotationAngles` terms for the mob bipeds, in their part order
/// (`ModelBiped.java`:129-244), including the aim branch `aimed_bow` turns on.
///
/// The held-item terms carry no input — a mob's equipment is not worn — and the riding terms
/// need a riding state the frame does not carry; both stay at their zero branches, which is
/// what the source's own defaults produce (`heldItemLeft`/`heldItemRight` zero, `isRiding`
/// false). The term is the same one the player model's pose carries for its own table; the
/// two stand beside each other because the source shares them by inheritance and the tables
/// differ.
fn pose_biped_base(pose: &Pose, aimed_bow: bool, out: &mut [Rot]) {
    let yaw = pose.head_yaw.to_radians();
    let pitch = pose.head_pitch.to_radians();
    out[P_HEAD].angles = [pitch, yaw, 0.0];
    out[P_BODY].angles[0] = if pose.sneak { 0.5 } else { 0.0 };
    out[P_RIGHT_ARM].angles[0] = (pose.limb_swing * 0.6662 + std::f32::consts::PI).cos()
        * 2.0
        * pose.limb_swing_amount
        * 0.5;
    out[P_LEFT_ARM].angles[0] =
        (pose.limb_swing * 0.6662).cos() * 2.0 * pose.limb_swing_amount * 0.5;
    out[P_RIGHT_ARM].angles[2] = 0.0;
    out[P_LEFT_ARM].angles[2] = 0.0;
    out[P_RIGHT_LEG].angles[0] = (pose.limb_swing * 0.6662).cos() * 1.4 * pose.limb_swing_amount;
    out[P_LEFT_LEG].angles[0] =
        (pose.limb_swing * 0.6662 + std::f32::consts::PI).cos() * 1.4 * pose.limb_swing_amount;
    out[P_RIGHT_LEG].angles[1] = 0.0;
    out[P_LEFT_LEG].angles[1] = 0.0;
    out[P_RIGHT_ARM].angles[1] = 0.0;
    out[P_RIGHT_ARM].angles[2] = 0.0;
    out[P_LEFT_ARM].angles[1] = 0.0;
    out[P_LEFT_ARM].angles[2] = 0.0;

    // The swing block: the body opens with the punch and the arms follow it
    // (`ModelBiped.setRotationAngles`:178-198). `swingProgress` is the frame's own fraction,
    // so the block always runs.
    let swing = pose.swing_progress;
    let body_yaw = (swing.sqrt() * std::f32::consts::PI * 2.0).sin() * 0.2;
    out[P_BODY].angles[1] = body_yaw;
    out[P_RIGHT_ARM].point[2] = body_yaw.sin() * 5.0;
    out[P_RIGHT_ARM].point[0] = -body_yaw.cos() * 5.0;
    out[P_LEFT_ARM].point[2] = -body_yaw.sin() * 5.0;
    out[P_LEFT_ARM].point[0] = body_yaw.cos() * 5.0;
    out[P_RIGHT_ARM].angles[1] += body_yaw;
    out[P_LEFT_ARM].angles[1] += body_yaw;
    out[P_LEFT_ARM].angles[0] += body_yaw;
    let mut f = 1.0 - swing;
    f *= f;
    f *= f;
    f = 1.0 - f;
    let f1 = (f * std::f32::consts::PI).sin();
    let f2 = (swing * std::f32::consts::PI).sin() * -(out[P_HEAD].angles[0] - 0.7) * 0.75;
    out[P_RIGHT_ARM].angles[0] -= f1 * 1.2 + f2;
    out[P_RIGHT_ARM].angles[1] += body_yaw * 2.0;
    out[P_RIGHT_ARM].angles[2] += (swing * std::f32::consts::PI).sin() * -0.4;

    if pose.sneak {
        out[P_BODY].angles[0] = 0.5;
        out[P_RIGHT_ARM].angles[0] += 0.4;
        out[P_LEFT_ARM].angles[0] += 0.4;
        out[P_RIGHT_LEG].point[2] = 4.0;
        out[P_LEFT_LEG].point[2] = 4.0;
        out[P_RIGHT_LEG].point[1] = 9.0;
        out[P_LEFT_LEG].point[1] = 9.0;
        out[P_HEAD].point[1] = 1.0;
    } else {
        out[P_BODY].angles[0] = 0.0;
        out[P_RIGHT_LEG].point[2] = 0.1;
        out[P_LEFT_LEG].point[2] = 0.1;
        out[P_RIGHT_LEG].point[1] = 12.0;
        out[P_LEFT_LEG].point[1] = 12.0;
        out[P_HEAD].point[1] = 0.0;
    }

    let idle_z = (pose.age * 0.09).cos() * 0.05 + 0.05;
    let idle_x = (pose.age * 0.067).sin() * 0.05;
    out[P_RIGHT_ARM].angles[2] += idle_z;
    out[P_LEFT_ARM].angles[2] -= idle_z;
    out[P_RIGHT_ARM].angles[0] += idle_x;
    out[P_LEFT_ARM].angles[0] -= idle_x;

    if aimed_bow {
        out[P_RIGHT_ARM].angles[2] = 0.0;
        out[P_LEFT_ARM].angles[2] = 0.0;
        out[P_RIGHT_ARM].angles[1] = -0.1 + out[P_HEAD].angles[1];
        out[P_LEFT_ARM].angles[1] = 0.1 + out[P_HEAD].angles[1] + 0.4;
        out[P_RIGHT_ARM].angles[0] = -std::f32::consts::FRAC_PI_2 + out[P_HEAD].angles[0];
        out[P_LEFT_ARM].angles[0] = -std::f32::consts::FRAC_PI_2 + out[P_HEAD].angles[0];
        out[P_RIGHT_ARM].angles[2] += idle_z;
        out[P_LEFT_ARM].angles[2] -= idle_z;
        out[P_RIGHT_ARM].angles[0] += idle_x;
        out[P_LEFT_ARM].angles[0] -= idle_x;
    }

    out[P_HEADWEAR].angles = out[P_HEAD].angles;
}

/// `ModelZombie`'s raised arms over the base terms (`ModelZombie.setRotationAngles`:31-44).
///
/// Every arm angle is written outright — the base's own arm swing, its sneak offsets and its
/// aim branch are all overwritten — while the arm pivots the swing block set keep standing.
fn pose_zombie_arms(pose: &Pose, out: &mut [Rot]) {
    let swing = pose.swing_progress;
    let f = (swing * std::f32::consts::PI).sin();
    let f1 = ((1.0 - (1.0 - swing) * (1.0 - swing)) * std::f32::consts::PI).sin();
    out[P_RIGHT_ARM].angles[2] = 0.0;
    out[P_LEFT_ARM].angles[2] = 0.0;
    out[P_RIGHT_ARM].angles[1] = -(0.1 - f * 0.6);
    out[P_LEFT_ARM].angles[1] = 0.1 - f * 0.6;
    out[P_RIGHT_ARM].angles[0] = -std::f32::consts::FRAC_PI_2;
    out[P_LEFT_ARM].angles[0] = -std::f32::consts::FRAC_PI_2;
    out[P_RIGHT_ARM].angles[0] -= f * 1.2 - f1 * 0.4;
    out[P_LEFT_ARM].angles[0] -= f * 1.2 - f1 * 0.4;
    let idle_z = (pose.age * 0.09).cos() * 0.05 + 0.05;
    let idle_x = (pose.age * 0.067).sin() * 0.05;
    out[P_RIGHT_ARM].angles[2] += idle_z;
    out[P_LEFT_ARM].angles[2] -= idle_z;
    out[P_RIGHT_ARM].angles[0] += idle_x;
    out[P_LEFT_ARM].angles[0] -= idle_x;
}

/// The zombie's pose: the base terms with the raised arms over them — the arms held out at
/// the source's constant quarter-turn back.
pub fn pose_zombie(pose: &Pose, out: &mut [Rot]) {
    pose_biped_base(pose, false, out);
    pose_zombie_arms(pose, out);
}

/// The skeleton's pose: the base terms with the wither type's aim branch — the flag
/// `ModelSkeleton.setLivingAnimations`:43 reads into `aimedBow` — followed by the raised
/// arms, which replace every angle the aim branch wrote (`ModelZombie`'s override runs after
/// `ModelBiped`'s).
///
/// The two wither states therefore stand on the same arm angles; the aim state is carried so
/// the composition holds the source's own order, and its pin finds them equal.
pub fn pose_skeleton(pose: &Pose, out: &mut [Rot]) {
    let aimed_bow = matches!(pose.extra, PoseExtra::Skeleton { aimed_bow: true });
    pose_biped_base(pose, aimed_bow, out);
    pose_zombie_arms(pose, out);
}

/// The villager's pose (`ModelVillager.setRotationAngles`:76-84): the head on the frame's
/// angles, the arms carried across and down at the source's constant set, and the legs
/// swinging at half the biped's own amplitude.
pub fn pose_villager(pose: &Pose, out: &mut [Rot]) {
    out[P_HEAD].angles = [
        pose.head_pitch.to_radians(),
        pose.head_yaw.to_radians(),
        0.0,
    ];
    out[P_ARMS].point = [0.0, 3.0, -1.0];
    out[P_ARMS].angles = [-0.75, 0.0, 0.0];
    out[P_RIGHT_VILLAGER_LEG].angles[0] =
        (pose.limb_swing * 0.6662).cos() * 1.4 * pose.limb_swing_amount * 0.5;
    out[P_LEFT_VILLAGER_LEG].angles[0] = (pose.limb_swing * 0.6662 + std::f32::consts::PI).cos()
        * 1.4
        * pose.limb_swing_amount
        * 0.5;
    out[P_RIGHT_VILLAGER_LEG].angles[1] = 0.0;
    out[P_LEFT_VILLAGER_LEG].angles[1] = 0.0;
}

/// The witch's pose: the villager's terms with the nose's idle sway and hold state over them
/// (`ModelWitch.setRotationAngles`:50-61), reading [`PoseExtra::Witch`].
///
/// The sway's phase reads the entity's id and its whole ticks — the source multiplies
/// `ticksExisted`, an integer, so the frame's own fraction does not advance the sway — and
/// the hold state, which the renderer reads from the held item (`RenderWitch.doRender`:24),
/// drops the nose and slides it back and up.
pub fn pose_witch(pose: &Pose, out: &mut [Rot]) {
    pose_villager(pose, out);
    let (holding, entity_id) = match pose.extra {
        PoseExtra::Witch { holding, entity_id } => (holding, entity_id),
        _ => (false, 0),
    };
    let nose = &mut out[P_WITCH_NOSE];
    nose.offset = [0.0; 3];
    let f = 0.01 * (entity_id % 10) as f32;
    let ticks = pose.age.floor();
    nose.angles = [
        (ticks * f).sin() * 4.5 * std::f32::consts::PI / 180.0,
        0.0,
        (ticks * f).cos() * 2.5 * std::f32::consts::PI / 180.0,
    ];
    if holding {
        nose.angles[0] = -0.9;
        nose.offset[2] = -0.09375;
        nose.offset[1] = 0.1875;
    }
}

/// The snow golem's pose (`ModelSnowMan.setRotationAngles`:40-56): the head on the frame's
/// angles, the body turned a quarter of the head's own, and the two hands swung out to the
/// body's sides.
pub fn pose_snow_golem(pose: &Pose, out: &mut [Rot]) {
    let yaw = pose.head_yaw.to_radians();
    out[P_SNOW_HEAD].angles = [pose.head_pitch.to_radians(), yaw, 0.0];
    out[P_SNOW_BODY].angles[1] = yaw * 0.25;
    let body_yaw = out[P_SNOW_BODY].angles[1];
    let f = body_yaw.sin();
    let f1 = body_yaw.cos();
    out[P_SNOW_RIGHT_HAND].angles[2] = 1.0;
    out[P_SNOW_LEFT_HAND].angles[2] = -1.0;
    out[P_SNOW_RIGHT_HAND].angles[1] = body_yaw;
    out[P_SNOW_LEFT_HAND].angles[1] = std::f32::consts::PI + body_yaw;
    out[P_SNOW_RIGHT_HAND].point[0] = f1 * 5.0;
    out[P_SNOW_RIGHT_HAND].point[2] = -f * 5.0;
    out[P_SNOW_LEFT_HAND].point[0] = -f1 * 5.0;
    out[P_SNOW_LEFT_HAND].point[2] = f * 5.0;
}

/// The iron golem's pose: the head on the frame's angles, the legs stepping on the folded
/// thirteen-tick wave (`ModelIronGolem.setRotationAngles`:85-90) and the arms swaying on it
/// in the walk's branch (`ModelIronGolem.setLivingAnimations`:117-120).
///
/// The attack and hold branches above that one read the golem's attack timer and hold-rose
/// clock: both are server-side states its status packets set, and no frame input carries
/// them, so the walk's own branch is what a live draw takes.
pub fn pose_iron_golem(pose: &Pose, out: &mut [Rot]) {
    out[0].angles = [
        pose.head_pitch.to_radians(),
        pose.head_yaw.to_radians(),
        0.0,
    ];
    let wave = super::folded_wave(pose.limb_swing, 13.0);
    out[P_GOLEM_LEFT_LEG].angles[0] = -1.5 * wave * pose.limb_swing_amount;
    out[P_GOLEM_RIGHT_LEG].angles[0] = 1.5 * wave * pose.limb_swing_amount;
    out[P_GOLEM_LEFT_LEG].angles[1] = 0.0;
    out[P_GOLEM_RIGHT_LEG].angles[1] = 0.0;
    out[P_GOLEM_RIGHT_ARM].angles[0] = (-0.2 + 1.5 * wave) * pose.limb_swing_amount;
    out[P_GOLEM_LEFT_ARM].angles[0] = (-0.2 - 1.5 * wave) * pose.limb_swing_amount;
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

    /// A pose with every field at rest.
    fn rest_pose() -> Pose {
        Pose::default()
    }

    #[test]
    fn the_zombie_geometry_is_the_biped_table() {
        assert_eq!(MODEL_ZOMBIE.parts.len(), 7, "the biped's seven parts draw");
        assert_eq!(
            MODEL_ZOMBIE.parts[P_HEAD].boxes,
            [b(
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ZOMBIE.parts[P_BODY].boxes,
            [b(
                [-4.0, 0.0, -2.0],
                [8.0, 12.0, 4.0],
                [16.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ZOMBIE.parts[P_RIGHT_ARM].boxes,
            [b(
                [-3.0, -2.0, -2.0],
                [4.0, 12.0, 4.0],
                [40.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ZOMBIE.parts[P_LEFT_ARM].boxes,
            [b(
                [-1.0, -2.0, -2.0],
                [4.0, 12.0, 4.0],
                [40.0, 16.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            MODEL_ZOMBIE.parts[P_RIGHT_LEG].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ZOMBIE.parts[P_LEFT_LEG].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                [0.0, 16.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            MODEL_ZOMBIE.parts[P_HEADWEAR].boxes,
            [b(
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [32.0, 0.0],
                0.5,
                false
            )]
        );
        // The limb pivots, in the source's own units.
        assert_eq!(MODEL_ZOMBIE.parts[P_RIGHT_ARM].point, [-5.0, 2.0, 0.0]);
        assert_eq!(MODEL_ZOMBIE.parts[P_LEFT_ARM].point, [5.0, 2.0, 0.0]);
        assert_eq!(MODEL_ZOMBIE.parts[P_RIGHT_LEG].point, [-1.9, 12.0, 0.0]);
        assert_eq!(MODEL_ZOMBIE.parts[P_LEFT_LEG].point, [1.9, 12.0, 0.0]);
    }

    #[test]
    fn the_skeleton_swaps_in_the_two_wide_limbs() {
        assert_eq!(
            MODEL_SKELETON.parts[P_RIGHT_ARM].boxes,
            [b(
                [-1.0, -2.0, -1.0],
                [2.0, 12.0, 2.0],
                [40.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SKELETON.parts[P_LEFT_ARM].boxes,
            [b(
                [-1.0, -2.0, -1.0],
                [2.0, 12.0, 2.0],
                [40.0, 16.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            MODEL_SKELETON.parts[P_RIGHT_LEG].boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [2.0, 12.0, 2.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SKELETON.parts[P_LEFT_LEG].boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [2.0, 12.0, 2.0],
                [0.0, 16.0],
                0.0,
                true
            )]
        );
        assert_eq!(MODEL_SKELETON.parts[P_RIGHT_LEG].point, [-2.0, 12.0, 0.0]);
        assert_eq!(MODEL_SKELETON.parts[P_LEFT_LEG].point, [2.0, 12.0, 0.0]);
        // The head and body stay the biped's own.
        assert_eq!(
            MODEL_SKELETON.parts[P_HEAD].boxes,
            MODEL_ZOMBIE.parts[P_HEAD].boxes
        );
        assert_eq!(
            MODEL_SKELETON.parts[P_BODY].boxes,
            MODEL_ZOMBIE.parts[P_BODY].boxes
        );
    }

    #[test]
    fn the_zombie_villager_head_has_its_own_two_boxes() {
        assert_eq!(
            MODEL_ZOMBIE_VILLAGER.parts[P_HEAD].boxes,
            [
                b(
                    [-4.0, -10.0, -4.0],
                    [8.0, 10.0, 8.0],
                    [0.0, 32.0],
                    0.0,
                    false
                ),
                b(
                    [-1.0, -3.0, -6.0],
                    [2.0, 4.0, 2.0],
                    [24.0, 32.0],
                    0.0,
                    false
                ),
            ]
        );
        // Its limbs stay the biped's four-wide ones and the headwear still draws over the
        // head (`ModelBiped.render`:118 draws it for every biped).
        assert_eq!(
            MODEL_ZOMBIE_VILLAGER.parts[P_RIGHT_ARM].boxes,
            MODEL_ZOMBIE.parts[P_RIGHT_ARM].boxes
        );
        assert_eq!(
            MODEL_ZOMBIE_VILLAGER.parts[P_HEADWEAR].boxes,
            MODEL_ZOMBIE.parts[P_HEADWEAR].boxes
        );
    }

    #[test]
    fn the_villager_geometry_matches_its_class() {
        assert_eq!(
            MODEL_VILLAGER.parts.len(),
            5,
            "the villager's five parts draw"
        );
        assert_eq!(
            MODEL_VILLAGER.parts[P_HEAD].boxes,
            [b(
                [-4.0, -10.0, -4.0],
                [8.0, 10.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_VILLAGER.parts[P_BODY].boxes,
            [
                b(
                    [-4.0, 0.0, -3.0],
                    [8.0, 12.0, 6.0],
                    [16.0, 20.0],
                    0.0,
                    false
                ),
                b([-4.0, 0.0, -3.0], [8.0, 18.0, 6.0], [0.0, 38.0], 0.5, false),
            ]
        );
        assert_eq!(
            MODEL_VILLAGER.parts[P_ARMS].boxes,
            [
                b(
                    [-8.0, -2.0, -2.0],
                    [4.0, 8.0, 4.0],
                    [44.0, 22.0],
                    0.0,
                    false
                ),
                b([4.0, -2.0, -2.0], [4.0, 8.0, 4.0], [44.0, 22.0], 0.0, false),
                b([-4.0, 2.0, -2.0], [8.0, 4.0, 4.0], [40.0, 38.0], 0.0, false),
            ]
        );
        assert_eq!(
            MODEL_VILLAGER.parts[P_RIGHT_VILLAGER_LEG].boxes,
            [b(
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                [0.0, 22.0],
                0.0,
                false
            )]
        );
        // The nose is the head's child, and its own pivot hangs the source's two units down.
        let nose = &MODEL_VILLAGER.parts[P_HEAD].children[0];
        assert_eq!(nose.point, [0.0, -2.0, 0.0], "the nose's pivot");
        assert_eq!(
            nose.boxes,
            [b(
                [-1.0, -1.0, -6.0],
                [2.0, 4.0, 2.0],
                [24.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(MODEL_VILLAGER.parts[P_ARMS].point, [0.0, 2.0, 0.0]);
    }

    #[test]
    fn the_witch_hangs_its_wart_and_hat_in_the_villagers_table() {
        assert_eq!(
            MODEL_WITCH.parts.len(),
            5,
            "the witch's parts are the villager's"
        );
        // The head's children: the nose first, then the hat
        // (`ModelWitch` adds the hat after the base's nose).
        let head = &MODEL_WITCH.parts[P_HEAD];
        assert_eq!(
            head.children.len(),
            2,
            "the hat and the nose under the head"
        );
        let nose = &head.children[0];
        let hat = &head.children[1];
        // The wart hangs under the nose (`ModelWitch.java`:15-17).
        assert_eq!(nose.children.len(), 1, "the wart under the nose");
        assert_eq!(nose.children[0].point, [0.0, -2.0, 0.0]);
        assert_eq!(
            nose.children[0].boxes,
            [b(
                [0.0, 3.0, -6.75],
                [1.0, 1.0, 1.0],
                [0.0, 0.0],
                -0.25,
                false
            )]
        );
        // The hat's brim and its three stacked pieces
        // (`ModelWitch.java`:18-39).
        assert_eq!(hat.point, [-5.0, -10.03125, -5.0]);
        assert_eq!(
            hat.boxes,
            [b(
                [0.0, 0.0, 0.0],
                [10.0, 2.0, 10.0],
                [0.0, 64.0],
                0.0,
                false
            )]
        );
        let second = &hat.children[0];
        assert_eq!(second.point, [1.75, -4.0, 2.0]);
        assert_eq!(second.rest, [-0.05235988, 0.0, 0.02617994]);
        assert_eq!(
            second.boxes,
            [b([0.0, 0.0, 0.0], [7.0, 4.0, 7.0], [0.0, 76.0], 0.0, false)]
        );
        let third = &second.children[0];
        assert_eq!(third.point, [1.75, -4.0, 2.0]);
        assert_eq!(third.rest, [-0.10471976, 0.0, 0.05235988]);
        assert_eq!(
            third.boxes,
            [b([0.0, 0.0, 0.0], [4.0, 4.0, 4.0], [0.0, 87.0], 0.0, false)]
        );
        let tip = &third.children[0];
        assert_eq!(tip.point, [1.75, -2.0, 2.0]);
        assert_eq!(tip.rest, [-0.20943952, 0.0, 0.10471976]);
        assert_eq!(
            tip.boxes,
            [b(
                [0.0, 0.0, 0.0],
                [1.0, 2.0, 1.0],
                [0.0, 95.0],
                0.25,
                false
            )]
        );
        // The table under it all is the villager's own.
        assert_eq!(
            MODEL_WITCH.parts[P_ARMS].boxes,
            MODEL_VILLAGER.parts[P_ARMS].boxes
        );
        assert_eq!(
            MODEL_WITCH.parts[P_RIGHT_VILLAGER_LEG].boxes,
            MODEL_VILLAGER.parts[P_RIGHT_VILLAGER_LEG].boxes
        );
    }

    #[test]
    fn the_snow_golem_geometry_matches_its_class() {
        assert_eq!(MODEL_SNOW_GOLEM.parts.len(), 5, "three boxes and two hands");
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[0].boxes,
            [b(
                [-5.0, -10.0, -5.0],
                [10.0, 10.0, 10.0],
                [0.0, 16.0],
                -0.5,
                false
            )]
        );
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[1].boxes,
            [b(
                [-6.0, -12.0, -6.0],
                [12.0, 12.0, 12.0],
                [0.0, 36.0],
                -0.5,
                false
            )]
        );
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[2].boxes,
            [b(
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                -0.5,
                false
            )]
        );
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[P_SNOW_RIGHT_HAND].boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [12.0, 2.0, 2.0],
                [32.0, 0.0],
                -0.5,
                false
            )]
        );
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[P_SNOW_LEFT_HAND].boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [12.0, 2.0, 2.0],
                [32.0, 0.0],
                -0.5,
                false
            )]
        );
        // The pivots: the head four up, the hands at six, the body thirteen, the base 24.
        assert_eq!(MODEL_SNOW_GOLEM.parts[P_SNOW_HEAD].point, [0.0, 4.0, 0.0]);
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[P_SNOW_RIGHT_HAND].point,
            [0.0, 6.0, 0.0]
        );
        assert_eq!(
            MODEL_SNOW_GOLEM.parts[P_SNOW_LEFT_HAND].point,
            [0.0, 6.0, 0.0]
        );
        assert_eq!(MODEL_SNOW_GOLEM.parts[P_SNOW_BODY].point, [0.0, 13.0, 0.0]);
        // The base's pivot (`ModelSnowMan.java`:20).
        assert_eq!(MODEL_SNOW_GOLEM.parts[1].point, [0.0, 24.0, 0.0]);
    }

    #[test]
    fn the_iron_golem_geometry_matches_its_class() {
        assert_eq!(
            MODEL_IRON_GOLEM.parts.len(),
            6,
            "the golem's six parts draw"
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[0].boxes,
            [
                b(
                    [-4.0, -12.0, -5.5],
                    [8.0, 10.0, 8.0],
                    [0.0, 0.0],
                    0.0,
                    false
                ),
                b([-1.0, -5.0, -7.5], [2.0, 4.0, 2.0], [24.0, 0.0], 0.0, false),
            ]
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[1].boxes,
            [
                b(
                    [-9.0, -2.0, -6.0],
                    [18.0, 12.0, 11.0],
                    [0.0, 40.0],
                    0.0,
                    false
                ),
                b([-4.5, 10.0, -3.0], [9.0, 5.0, 6.0], [0.0, 70.0], 0.5, false),
            ]
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[P_GOLEM_LEFT_LEG].boxes,
            [b(
                [-3.5, -3.0, -3.0],
                [6.0, 16.0, 5.0],
                [37.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[P_GOLEM_RIGHT_ARM].boxes,
            [b(
                [-13.0, -2.5, -3.0],
                [4.0, 30.0, 6.0],
                [60.0, 21.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[P_GOLEM_LEFT_ARM].boxes,
            [b(
                [9.0, -2.5, -3.0],
                [4.0, 30.0, 6.0],
                [60.0, 58.0],
                0.0,
                false
            )]
        );
        // The pivots: the head and body seven down, the legs eleven, the arms with the body.
        assert_eq!(MODEL_IRON_GOLEM.parts[0].point, [0.0, -7.0, -2.0]);
        assert_eq!(MODEL_IRON_GOLEM.parts[1].point, [0.0, -7.0, 0.0]);
        assert_eq!(
            MODEL_IRON_GOLEM.parts[P_GOLEM_LEFT_LEG].point,
            [-4.0, 11.0, 0.0]
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[P_GOLEM_RIGHT_LEG].point,
            [5.0, 11.0, 0.0]
        );
        assert_eq!(
            MODEL_IRON_GOLEM.parts[P_GOLEM_RIGHT_ARM].point,
            [0.0, -7.0, 0.0]
        );
    }

    #[test]
    fn the_zombie_arms_stand_at_the_constant_quarter_turn() {
        // The source's own values with the walk and age at zero: the arms at minus a quarter
        // turn about x, the right pulled `-0.1` about y, the left `+0.1`
        // (`ModelZombie.setRotationAngles`:33-40).
        let mut out = MODEL_ZOMBIE.rest();
        pose_zombie(&rest_pose(), &mut out);
        let quarter = -std::f32::consts::FRAC_PI_2;
        assert_eq!(
            out[P_RIGHT_ARM].angles[0], quarter,
            "the right arm's raised angle"
        );
        assert_eq!(
            out[P_LEFT_ARM].angles[0], quarter,
            "the left arm's raised angle"
        );
        assert_eq!(out[P_RIGHT_ARM].angles[1], -0.1);
        assert_eq!(out[P_LEFT_ARM].angles[1], 0.1);
        // The age's idle terms: cosine `0.09` twice five per cent plus five, each side's own
        // sign, and the sine `0.067` term (`:41-44`).
        let mut out = MODEL_ZOMBIE.rest();
        let sketch = Pose {
            age: 10.0,
            ..rest_pose()
        };
        pose_zombie(&sketch, &mut out);
        let idle_z = (10.0_f32 * 0.09).cos() * 0.05 + 0.05;
        let idle_x = (10.0_f32 * 0.067).sin() * 0.05;
        assert!(
            (out[P_RIGHT_ARM].angles[2] - idle_z).abs() < 1.0e-6,
            "the right arm's idle z"
        );
        assert!(
            (out[P_LEFT_ARM].angles[2] + idle_z).abs() < 1.0e-6,
            "the left arm's idle z"
        );
        assert!((out[P_RIGHT_ARM].angles[0] - (quarter + idle_x)).abs() < 1.0e-6);
        assert!((out[P_LEFT_ARM].angles[0] - (quarter - idle_x)).abs() < 1.0e-6);
        // The swing's own terms at a half: the arms pick up the sin terms
        // (`:31-40`) while the walk's arm swing is gone — the override writes x outright.
        let mut out = MODEL_ZOMBIE.rest();
        let sketch = Pose {
            swing_progress: 0.5,
            limb_swing: 3.0,
            limb_swing_amount: 1.0,
            ..rest_pose()
        };
        pose_zombie(&sketch, &mut out);
        let f = (0.5_f32 * std::f32::consts::PI).sin();
        let f1 = ((1.0 - (1.0 - 0.5_f32) * (1.0 - 0.5)) * std::f32::consts::PI).sin();
        assert!((out[P_RIGHT_ARM].angles[0] - (quarter - f * 1.2 + f1 * 0.4)).abs() < 1.0e-6);
        assert!((out[P_RIGHT_ARM].angles[1] - (-(0.1 - f * 0.6))).abs() < 1.0e-6);
        // The legs still swing on the walk, the un-overwritten half of the base.
        assert!(
            (out[P_RIGHT_LEG].angles[0] - (3.0_f32 * 0.6662).cos() * 1.4).abs() < 1.0e-5,
            "the right leg keeps the walk's swing: left {}",
            out[P_RIGHT_LEG].angles[0]
        );
    }

    #[test]
    fn the_skeleton_aim_states_reduce_to_one_arm_pose() {
        // `ModelSkeleton` reads the wither type into `aimedBow` (`setLivingAnimations`:43)
        // over `ModelZombie`'s raised arms. The aim branch writes arm angles
        // (`ModelBiped.setRotationAngles`:226-242) and the raised-arms override then replaces
        // every one of them (`ModelZombie.setRotationAngles`:33-40) — so both states land on
        // the same arms. The pin records the reduction, not an oversight.
        let plain = Pose {
            head_yaw: 30.0,
            head_pitch: 12.0,
            limb_swing: 4.0,
            limb_swing_amount: 0.8,
            age: 7.0,
            ..rest_pose()
        };
        let aiming = Pose {
            extra: PoseExtra::Skeleton { aimed_bow: true },
            ..plain
        };
        let mut a = MODEL_SKELETON.rest();
        pose_skeleton(&plain, &mut a);
        let mut b = MODEL_SKELETON.rest();
        pose_skeleton(&aiming, &mut b);
        assert_eq!(
            a[P_RIGHT_ARM], b[P_RIGHT_ARM],
            "the aim state leaves the right arm where the raised arms put it"
        );
        assert_eq!(a[P_LEFT_ARM], b[P_LEFT_ARM], "and the left arm");
        // The shared head angles still follow the frame in both states.
        assert!((a[P_HEAD].angles[1] - 30.0_f32.to_radians()).abs() < 1.0e-6);
        assert!((b[P_HEAD].angles[0] - 12.0_f32.to_radians()).abs() < 1.0e-6);
    }

    #[test]
    fn the_villager_crosses_its_arms_and_swings_half_legs() {
        let mut out = MODEL_VILLAGER.rest();
        let sketch = Pose {
            limb_swing: 3.0,
            limb_swing_amount: 1.0,
            ..rest_pose()
        };
        pose_villager(&sketch, &mut out);
        // The arms carried across: the pivot three up and one back, a three-quarter turn
        // down (`ModelVillager.setRotationAngles`:78-80).
        assert_eq!(out[P_ARMS].point, [0.0, 3.0, -1.0]);
        assert_eq!(out[P_ARMS].angles, [-0.75, 0.0, 0.0]);
        // The legs at half the biped's swing (`:81-82`).
        let expected = (3.0_f32 * 0.6662).cos() * 1.4 * 0.5;
        assert!((out[P_RIGHT_VILLAGER_LEG].angles[0] - expected).abs() < 1.0e-6);
        assert!(
            (out[P_LEFT_VILLAGER_LEG].angles[0] + expected).abs() < 1.0e-6,
            "the left leg stands against the right: {}",
            out[P_LEFT_VILLAGER_LEG].angles[0]
        );
        assert_eq!(out[P_RIGHT_VILLAGER_LEG].angles[1], 0.0);
    }

    #[test]
    fn the_witch_nose_sways_on_the_id_and_holds_for_an_item() {
        // The sway: the id's modulo ten thousandths of a tick, four-and-a-half degrees of
        // sine about x and two-and-a-half of cosine about z (`ModelWitch.setRotationAngles`:51-54).
        let sketch = Pose {
            age: 12.4,
            extra: PoseExtra::Witch {
                holding: false,
                entity_id: 27,
            },
            ..rest_pose()
        };
        let mut out = MODEL_WITCH.rest();
        pose_witch(&sketch, &mut out);
        let f = 0.01 * 7.0;
        assert!(
            (out[P_WITCH_NOSE].angles[0]
                - (12.0_f32 * f).sin() * 4.5 * std::f32::consts::PI / 180.0)
                .abs()
                < 1.0e-6,
            "the nose's sway reads whole ticks: {}",
            out[P_WITCH_NOSE].angles[0]
        );
        assert!(
            (out[P_WITCH_NOSE].angles[2]
                - (12.0_f32 * f).cos() * 2.5 * std::f32::consts::PI / 180.0)
                .abs()
                < 1.0e-6
        );
        assert_eq!(
            out[P_WITCH_NOSE].offset,
            [0.0, 0.0, 0.0],
            "no offset while bare"
        );
        // The hold: the nose drops three-quarter ways and slides back and up
        // (`:56-61`).
        let sketch = Pose {
            extra: PoseExtra::Witch {
                holding: true,
                entity_id: 3,
            },
            ..rest_pose()
        };
        let mut out = MODEL_WITCH.rest();
        pose_witch(&sketch, &mut out);
        assert_eq!(out[P_WITCH_NOSE].angles[0], -0.9);
        assert_eq!(out[P_WITCH_NOSE].offset, [0.0, 0.1875, -0.09375]);
        // The villager's own terms still stand under it.
        assert_eq!(out[P_ARMS].point, [0.0, 3.0, -1.0]);
    }

    #[test]
    fn the_snow_golem_turns_its_hands_with_its_head() {
        let sketch = Pose {
            head_yaw: 90.0,
            ..rest_pose()
        };
        let mut out = MODEL_SNOW_GOLEM.rest();
        pose_snow_golem(&sketch, &mut out);
        // The body turns a quarter of the head's own yaw (`ModelSnowMan.setRotationAngles`:45).
        let body = 90.0_f32.to_radians() * 0.25;
        assert!((out[0].angles[1] - body).abs() < 1.0e-6);
        // The hands: right at +1 about z, left at -1, spread on the body's own sine pair
        // (`:46-55`).
        assert_eq!(out[P_SNOW_RIGHT_HAND].angles[2], 1.0);
        assert_eq!(out[P_SNOW_LEFT_HAND].angles[2], -1.0);
        assert!((out[P_SNOW_RIGHT_HAND].angles[1] - body).abs() < 1.0e-6);
        assert!((out[P_SNOW_LEFT_HAND].angles[1] - (std::f32::consts::PI + body)).abs() < 1.0e-6);
        assert!((out[P_SNOW_RIGHT_HAND].point[0] - body.cos() * 5.0).abs() < 1.0e-6);
        assert!((out[P_SNOW_RIGHT_HAND].point[2] + body.sin() * 5.0).abs() < 1.0e-6);
        assert!((out[P_SNOW_LEFT_HAND].point[0] + body.cos() * 5.0).abs() < 1.0e-6);
        assert!((out[P_SNOW_LEFT_HAND].point[2] - body.sin() * 5.0).abs() < 1.0e-6);
    }

    #[test]
    fn the_iron_golem_steps_and_sways_on_the_folded_wave() {
        // At a whole 6.5 of the thirteen-tick fold the wave is at its trough, minus one;
        // the legs step against each other on it (`ModelIronGolem.setRotationAngles`:87-90).
        let sketch = Pose {
            limb_swing: 6.5,
            limb_swing_amount: 1.0,
            ..rest_pose()
        };
        let mut out = MODEL_IRON_GOLEM.rest();
        pose_iron_golem(&sketch, &mut out);
        assert!(
            (out[P_GOLEM_LEFT_LEG].angles[0] - 1.5).abs() < 1.0e-5,
            "the left leg at the trough: {}",
            out[P_GOLEM_LEFT_LEG].angles[0]
        );
        assert!(
            (out[P_GOLEM_RIGHT_LEG].angles[0] + 1.5).abs() < 1.0e-5,
            "the right leg at the trough: {}",
            out[P_GOLEM_RIGHT_LEG].angles[0]
        );
        // The arms sway on the same wave in the walk's branch
        // (`ModelIronGolem.setLivingAnimations`:118-119): minus two tenths of a turn, the
        // left minus the right's term.
        assert!(
            (out[P_GOLEM_RIGHT_ARM].angles[0] + 1.7).abs() < 1.0e-5,
            "the right arm at the trough: {}",
            out[P_GOLEM_RIGHT_ARM].angles[0]
        );
        assert!(
            (out[P_GOLEM_LEFT_ARM].angles[0] - 1.3).abs() < 1.0e-5,
            "the left arm at the trough: {}",
            out[P_GOLEM_LEFT_ARM].angles[0]
        );
        // Standing still the arms hang at rest: the walk's branch scales both terms by the
        // amount, and the attack branch is a state no frame carries.
        let mut out = MODEL_IRON_GOLEM.rest();
        pose_iron_golem(&rest_pose(), &mut out);
        assert_eq!(out[P_GOLEM_RIGHT_ARM].angles[0], 0.0);
        assert_eq!(out[P_GOLEM_LEFT_ARM].angles[0], 0.0);
    }

    #[test]
    fn the_shared_base_keeps_the_walk_and_the_swing_on_the_limbs() {
        let sketch = Pose {
            limb_swing: 0.0,
            limb_swing_amount: 1.0,
            head_yaw: 45.0,
            head_pitch: 10.0,
            ..rest_pose()
        };
        let mut out = MODEL_ZOMBIE.rest();
        pose_zombie(&sketch, &mut out);
        // The legs: cosine at zero is one, so the right leg swings 1.4 and the left -1.4
        // (`ModelBiped.setRotationAngles`:137-138).
        assert!((out[P_RIGHT_LEG].angles[0] - 1.4).abs() < 1.0e-6);
        assert!((out[P_LEFT_LEG].angles[0] + 1.4).abs() < 1.0e-6);
        // The head carries the frame's own angles and the headwear copies them
        // (`:131-132`, `:244`).
        assert!((out[P_HEAD].angles[0] - 10.0_f32.to_radians()).abs() < 1.0e-6);
        assert!((out[P_HEAD].angles[1] - 45.0_f32.to_radians()).abs() < 1.0e-6);
        assert_eq!(out[P_HEADWEAR].angles, out[P_HEAD].angles);
        // The standing legs sit a tenth of a unit back and twelve up
        // (`ModelBiped.setRotationAngles`:214-218).
        assert_eq!(out[P_RIGHT_LEG].point[2], 0.1);
        assert_eq!(out[P_RIGHT_LEG].point[1], 12.0);
        assert_eq!(out[P_HEAD].point[1], 0.0);
        // A sneaking mob lifts the model and folds the legs forward (`:200-210`).
        let sketch = Pose {
            sneak: true,
            ..rest_pose()
        };
        let mut out = MODEL_ZOMBIE.rest();
        pose_zombie(&sketch, &mut out);
        assert_eq!(out[P_BODY].angles[0], 0.5);
        assert_eq!(out[P_RIGHT_LEG].point, [-1.9, 9.0, 4.0]);
        assert_eq!(out[P_HEAD].point[1], 1.0);
    }
}
