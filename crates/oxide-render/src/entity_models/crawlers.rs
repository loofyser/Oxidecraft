//! The crawler, cube and arthropod families: the creeper, the spiders and the enderman, the
//! chicken, the squid, the slime and the magma cube, the bat, the silverfish and the
//! endermite.
//!
//! The tables are the classes' own (`ModelCreeper`, `ModelSpider`, `ModelEnderman`,
//! `ModelChicken`, `ModelSquid`, `ModelSlime`, `ModelMagmaCube`, `ModelBat`,
//! `ModelSilverfish`, `ModelEnderMite`), the poses their `setRotationAngles` and living
//! animations, and the scales the renderers' pre-render callbacks. The enderman's model is
//! the biped table outstretched — its limbs are thirty units long (`ModelEnderman.java`:23-36)
//! — whose pose runs the shared `ModelBiped` terms first and then halves, clamps and re-hangs
//! them (`ModelEnderman.setRotationAngles`:46-127). The bat's ears and wings are children of
//! its head and body (`ModelBat.java`:32-57), so its part table nests.
//!
//! The slot maps name every part the tables build; the few indices no pose reads are
//! flagged `#[allow(dead_code)]` — the module's own geometry tests address parts through
//! them.
//!
//! The chicken's wings and the squid's tentacles draw at rest: their flap and tentacle floats
//! are the entities' own client-side tick fields (`RenderChicken.handleRotationFloat`:28-33,
//! `RenderSquid.handleRotationFloat`:39-42), which the frame does not carry. The chicken's
//! grounded `destPos` is zero, and the squid's own field is zero in water past the half
//! rotation (`EntitySquid.java`:185) — out of water the same field runs
//! `|sin(squidRotation)| * π/4` (`EntitySquid.java`:205) — so both draw at the zero this
//! module pins. The cube family's
//! squash pair reaches the renderer through the pre-render callbacks (`RenderSlime`:32-37,
//! `RenderMagmaCube`:29-35) and the magma cube's segments through its living animation
//! (`ModelMagmaCube.setLivingAnimations`:42-56); the scale formulas and the segment offsets
//! are implemented, and the squash input itself is a recorded simplification — its source
//! counter is the entity's update loop's (`EntitySlime.onUpdate`), which the frame does not
//! carry.

use super::{Box, Model, Part, Pose, PoseExtra, Rot};

/// The creeper's part order: the head, the body and the four legs, the order
/// `ModelCreeper` builds them in (`ModelCreeper.java`:23-44).
mod creeper_slot {
    /// The head.
    pub const HEAD: usize = 0;
    /// The body.
    #[allow(dead_code)]
    pub const BODY: usize = 1;
    /// The first leg.
    pub const LEG1: usize = 2;
    /// The second leg.
    pub const LEG2: usize = 3;
    /// The third leg.
    pub const LEG3: usize = 4;
    /// The fourth leg.
    pub const LEG4: usize = 5;
}

/// The creeper's head (`ModelCreeper.java`:24-26).
static CREEPER_HEAD: Part = Part {
    point: [0.0, 6.0, 0.0],
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

/// The creeper's body (`ModelCreeper.java`:30-32).
static CREEPER_BODY: Part = Part {
    point: [0.0, 6.0, 0.0],
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

/// One creeper leg: four by six by four, off the sheet's sixteen-offset leg cell
/// (`ModelCreeper.java`:33-44). The four legs share the box; only their pivots differ.
static CREEPER_LEG: Part = Part {
    point: [0.0, 18.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, 0.0, -2.0],
        size: [4.0, 6.0, 4.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The creeper's legs, in the order the class hangs them: the first pair first, then the
/// second (`ModelCreeper.java`:33-44).
static CREEPER_PARTS: [Part; 6] = [
    CREEPER_HEAD,
    CREEPER_BODY,
    // The first and second legs.
    Part {
        point: [-2.0, 18.0, 4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: CREEPER_LEG.boxes,
        children: &[],
    },
    Part {
        point: [2.0, 18.0, 4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: CREEPER_LEG.boxes,
        children: &[],
    },
    // The third and fourth legs.
    Part {
        point: [-2.0, 18.0, -4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: CREEPER_LEG.boxes,
        children: &[],
    },
    Part {
        point: [2.0, 18.0, -4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: CREEPER_LEG.boxes,
        children: &[],
    },
];

/// The creeper's table: the head, the body and the four legs, in the render list's own order
/// (`ModelCreeper.java`:50-58). The class's sixth constructor box — the armour shell — is
/// never drawn; the render list leaves it out.
pub static MODEL_CREEPER: Model = Model {
    parts: &CREEPER_PARTS,
};

/// The spider's part order: the head, the neck, the body and the eight legs, the order
/// `ModelSpider` builds them in (`ModelSpider.java`:43-77).
mod spider_slot {
    /// The head.
    pub const HEAD: usize = 0;
    /// The neck.
    #[allow(dead_code)]
    pub const NECK: usize = 1;
    /// The body.
    #[allow(dead_code)]
    pub const BODY: usize = 2;
    /// The first leg, off the left flank.
    pub const LEG1: usize = 3;
    /// The second leg, off the right flank.
    pub const LEG2: usize = 4;
    /// The third leg.
    pub const LEG3: usize = 5;
    /// The fourth leg.
    pub const LEG4: usize = 6;
    /// The fifth leg.
    pub const LEG5: usize = 7;
    /// The sixth leg.
    pub const LEG6: usize = 8;
    /// The seventh leg.
    pub const LEG7: usize = 9;
    /// The eighth leg.
    pub const LEG8: usize = 10;
}

/// The spider's head, eight cubed off its own cell (`ModelSpider.java`:45-47).
static SPIDER_HEAD: Part = Part {
    point: [0.0, 15.0, -3.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -4.0, -8.0],
        size: [8.0, 8.0, 8.0],
        uv: [32.0, 4.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The spider's neck (`ModelSpider.java`:48-50).
static SPIDER_NECK: Part = Part {
    point: [0.0, 15.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, -3.0, -3.0],
        size: [6.0, 6.0, 6.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The spider's body, hung nine back of the neck (`ModelSpider.java`:51-53).
static SPIDER_BODY: Part = Part {
    point: [0.0, 15.0, 9.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-5.0, -4.0, -6.0],
        size: [10.0, 8.0, 12.0],
        uv: [0.0, 12.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// One left spider leg: sixteen long, reaching out from the left flank
/// (`ModelSpider.java`:54-59, the odd-index branch).
static SPIDER_LEFT_LEG: Part = Part {
    point: [-4.0, 15.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-15.0, -1.0, -1.0],
        size: [16.0, 2.0, 2.0],
        uv: [18.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// One right spider leg: the same reach from the right flank (`ModelSpider.java`:60-65, the
/// even-index branch). The class mirrors nothing; the box's own origin carries the side.
static SPIDER_RIGHT_LEG: Part = Part {
    point: [4.0, 15.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -1.0, -1.0],
        size: [16.0, 2.0, 2.0],
        uv: [18.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The spider's table: the head, the neck, the body, then the legs from the first to the
/// eighth (`ModelSpider.render`:86-96). The eight legs' pivots walk the body's z in pairs:
/// plus two, plus one, zero and minus one (`ModelSpider.java`:56-77).
static SPIDER_PARTS: [Part; 11] = [
    SPIDER_HEAD,
    SPIDER_NECK,
    SPIDER_BODY,
    Part {
        point: [-4.0, 15.0, 2.0],
        ..SPIDER_LEFT_LEG
    },
    Part {
        point: [4.0, 15.0, 2.0],
        ..SPIDER_RIGHT_LEG
    },
    Part {
        point: [-4.0, 15.0, 1.0],
        ..SPIDER_LEFT_LEG
    },
    Part {
        point: [4.0, 15.0, 1.0],
        ..SPIDER_RIGHT_LEG
    },
    Part {
        point: [-4.0, 15.0, 0.0],
        ..SPIDER_LEFT_LEG
    },
    Part {
        point: [4.0, 15.0, 0.0],
        ..SPIDER_RIGHT_LEG
    },
    Part {
        point: [-4.0, 15.0, -1.0],
        ..SPIDER_LEFT_LEG
    },
    Part {
        point: [4.0, 15.0, -1.0],
        ..SPIDER_RIGHT_LEG
    },
];

/// The spider's table (`ModelSpider.java`:43-77).
pub static MODEL_SPIDER: Model = Model {
    parts: &SPIDER_PARTS,
};

/// The enderman's part order: the biped order, its headwear last
/// (`ModelEnderman.java`:17-36, `ModelBiped`'s field order).
mod enderman_slot {
    /// The head.
    pub const HEAD: usize = 0;
    /// The body.
    pub const BODY: usize = 1;
    /// The right arm.
    pub const RIGHT_ARM: usize = 2;
    /// The left arm.
    pub const LEFT_ARM: usize = 3;
    /// The right leg.
    pub const RIGHT_LEG: usize = 4;
    /// The left leg.
    pub const LEFT_LEG: usize = 5;
    /// The headwear.
    pub const HEADWEAR: usize = 6;
}

/// The enderman's head (`ModelBiped.java`:55-57).
static ENDERMAN_HEAD: Part = Part {
    point: [0.0, -14.0, 0.0],
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

/// The enderman's body (`ModelEnderman.java`:20-22).
static ENDERMAN_BODY: Part = Part {
    point: [0.0, -14.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, 0.0, -2.0],
        size: [8.0, 12.0, 4.0],
        uv: [32.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The enderman's right arm: two by thirty, the outstretched limb
/// (`ModelEnderman.java`:23-25).
static ENDERMAN_RIGHT_ARM: Part = Part {
    point: [-3.0, -12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -2.0, -1.0],
        size: [2.0, 30.0, 2.0],
        uv: [56.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The enderman's left arm (`ModelEnderman.java`:26-29).
static ENDERMAN_LEFT_ARM: Part = Part {
    point: [5.0, -12.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -2.0, -1.0],
        size: [2.0, 30.0, 2.0],
        uv: [56.0, 0.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The enderman's right leg, thirty long (`ModelEnderman.java`:30-32).
static ENDERMAN_RIGHT_LEG: Part = Part {
    point: [-2.0, -2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [2.0, 30.0, 2.0],
        uv: [56.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The enderman's left leg (`ModelEnderman.java`:33-36).
static ENDERMAN_LEFT_LEG: Part = Part {
    point: [2.0, -2.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [2.0, 30.0, 2.0],
        uv: [56.0, 0.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The enderman's headwear: the head's box at the enderman's own quarter inflation —
/// `p_i46305_1_ - 0.5F` over the head's zero (`ModelEnderman.java`:17-19).
static ENDERMAN_HEADWEAR: Part = Part {
    point: [0.0, -14.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -8.0, -4.0],
        size: [8.0, 8.0, 8.0],
        uv: [0.0, 16.0],
        inflate: -0.5,
        mirror: false,
    }],
    children: &[],
};

/// The enderman's table (`ModelEnderman.java`:17-36).
static ENDERMAN_PARTS: [Part; 7] = [
    ENDERMAN_HEAD,
    ENDERMAN_BODY,
    ENDERMAN_RIGHT_ARM,
    ENDERMAN_LEFT_ARM,
    ENDERMAN_RIGHT_LEG,
    ENDERMAN_LEFT_LEG,
    ENDERMAN_HEADWEAR,
];

/// The enderman's table (`ModelEnderman.java`:17-36).
pub static MODEL_ENDERMAN: Model = Model {
    parts: &ENDERMAN_PARTS,
};

/// The chicken's part order: the head group, the body and legs, and the wings
/// (`ModelChicken.java`:20-44).
mod chicken_slot {
    /// The head.
    pub const HEAD: usize = 0;
    /// The bill.
    pub const BILL: usize = 1;
    /// The chin.
    pub const CHIN: usize = 2;
    /// The body.
    pub const BODY: usize = 3;
    /// The right leg.
    pub const RIGHT_LEG: usize = 4;
    /// The left leg.
    pub const LEFT_LEG: usize = 5;
    /// The right wing.
    pub const RIGHT_WING: usize = 6;
    /// The left wing.
    pub const LEFT_WING: usize = 7;
}

/// The chicken's head (`ModelChicken.java`:21-23).
static CHICKEN_HEAD: Part = Part {
    point: [0.0, 15.0, -4.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, -6.0, -2.0],
        size: [4.0, 6.0, 3.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The chicken's bill, hung at the head's pivot (`ModelChicken.java`:24-26).
static CHICKEN_BILL: Part = Part {
    point: [0.0, 15.0, -4.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, -4.0, -4.0],
        size: [4.0, 2.0, 2.0],
        uv: [14.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The chicken's chin, the bill's small sibling (`ModelChicken.java`:27-29).
static CHICKEN_CHIN: Part = Part {
    point: [0.0, 15.0, -4.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, -2.0, -3.0],
        size: [2.0, 2.0, 2.0],
        uv: [14.0, 4.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The chicken's body (`ModelChicken.java`:30-32).
static CHICKEN_BODY: Part = Part {
    point: [0.0, 16.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, -4.0, -3.0],
        size: [6.0, 8.0, 6.0],
        uv: [0.0, 9.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// One chicken leg: three by five by three (`ModelChicken.java`:33-35). The two legs share
/// the box; only their pivots differ.
static CHICKEN_LEG: Part = Part {
    point: [0.0, 19.0, 1.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -3.0],
        size: [3.0, 5.0, 3.0],
        uv: [26.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The chicken's right wing, one thick (`ModelChicken.java`:39-41).
static CHICKEN_RIGHT_WING: Part = Part {
    point: [-4.0, 13.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [0.0, 0.0, -3.0],
        size: [1.0, 4.0, 6.0],
        uv: [24.0, 13.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The chicken's left wing, the right's mirror (`ModelChicken.java`:42-44).
static CHICKEN_LEFT_WING: Part = Part {
    point: [4.0, 13.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -3.0],
        size: [1.0, 4.0, 6.0],
        uv: [24.0, 13.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The chicken's table (`ModelChicken.java`:20-44).
static CHICKEN_PARTS: [Part; 8] = [
    CHICKEN_HEAD,
    CHICKEN_BILL,
    CHICKEN_CHIN,
    CHICKEN_BODY,
    Part {
        point: [-2.0, 19.0, 1.0],
        ..CHICKEN_LEG
    },
    Part {
        point: [1.0, 19.0, 1.0],
        ..CHICKEN_LEG
    },
    CHICKEN_RIGHT_WING,
    CHICKEN_LEFT_WING,
];

/// The chicken's table (`ModelChicken.java`:20-44).
pub static MODEL_CHICKEN: Model = Model {
    parts: &CHICKEN_PARTS,
};

/// The chick's table: the renderer's own fold of the chicken (`ModelChicken.render`:54-72).
/// The head group moves down five and out two whole — the head, bill and chin together —
/// and the body group's boxes and pivots halve about a twelve-unit drop.
static CHICKEN_CHILD_PARTS: [Part; 8] = [
    Part {
        point: [0.0, 20.0, -2.0],
        ..CHICKEN_HEAD
    },
    Part {
        point: [0.0, 20.0, -2.0],
        ..CHICKEN_BILL
    },
    Part {
        point: [0.0, 20.0, -2.0],
        ..CHICKEN_CHIN
    },
    Part {
        point: [0.0, 20.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, -2.0, -1.5],
            size: [3.0, 4.0, 3.0],
            uv: [0.0, 9.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [-1.0, 21.5, 0.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -1.5],
            size: [1.5, 2.5, 1.5],
            uv: [26.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.5, 21.5, 0.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -1.5],
            size: [1.5, 2.5, 1.5],
            uv: [26.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [-2.0, 18.5, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [0.0, 0.0, -1.5],
            size: [0.5, 2.0, 3.0],
            uv: [24.0, 13.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [2.0, 18.5, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -1.5],
            size: [0.5, 2.0, 3.0],
            uv: [24.0, 13.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The chick's table (`ModelChicken.render`:54-72).
pub static MODEL_CHICKEN_CHILD: Model = Model {
    parts: &CHICKEN_CHILD_PARTS,
};

/// The squid's part order: the body, then the eight tentacles
/// (`ModelSquid.java`:10-34).
mod squid_slot {
    /// The body.
    #[allow(dead_code)]
    pub const BODY: usize = 0;
    /// The first tentacle.
    pub const TENTACLE0: usize = 1;
    /// The second tentacle.
    pub const TENTACLE1: usize = 2;
    /// The third tentacle.
    pub const TENTACLE2: usize = 3;
    /// The fourth tentacle.
    pub const TENTACLE3: usize = 4;
    /// The fifth tentacle.
    pub const TENTACLE4: usize = 5;
    /// The sixth tentacle.
    pub const TENTACLE5: usize = 6;
    /// The seventh tentacle.
    pub const TENTACLE6: usize = 7;
    /// The eighth tentacle.
    pub const TENTACLE7: usize = 8;
}

/// The squid's body (`ModelSquid.java`:13-15).
static SQUID_BODY: Part = Part {
    point: [0.0, 8.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-6.0, -8.0, -6.0],
        size: [12.0, 16.0, 12.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// One squid tentacle: two by eighteen, off the fan's cell (`ModelSquid.java`:29-32`), hung
/// at its ring point with its rest yaw the class's eighth-turn walk.
static SQUID_TENTACLE: Part = Part {
    point: [0.0, 15.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-1.0, 0.0, -1.0],
        size: [2.0, 18.0, 2.0],
        uv: [48.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The squid's table: the body and the eight tentacles, the j'th at the ring's j-eighth
/// point, `(cos(j/8 turn) * 5, 15, sin(j/8 turn) * 5)`, its rest yaw `-j * pi/4 + pi/2`
/// (`ModelSquid.java`:17-32). The source computes the ring in doubles and casts; the tiny
/// cosine whiskers at the quarter turns are carried as the casts leave them.
static SQUID_PARTS: [Part; 9] = [
    SQUID_BODY,
    Part {
        point: [5.0, 15.0, 0.0],
        rest: [0.0, 1.5707964, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [3.535_534, 15.0, 3.535_534],
        rest: [0.0, std::f32::consts::FRAC_PI_4, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [3.061617e-16, 15.0, 5.0],
        rest: [0.0, 0.0, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [-3.535_534, 15.0, 3.535_534],
        rest: [0.0, -std::f32::consts::FRAC_PI_4, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [-5.0, 15.0, 0.0],
        rest: [0.0, -1.5707964, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [-3.535_534, 15.0, -3.535_534],
        rest: [0.0, -2.3561945, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [-9.184851e-16, 15.0, -5.0],
        rest: [0.0, -std::f32::consts::PI, 0.0],
        ..SQUID_TENTACLE
    },
    Part {
        point: [3.535_534, 15.0, -3.535_534],
        rest: [0.0, -3.9269908, 0.0],
        ..SQUID_TENTACLE
    },
];

/// The squid's table (`ModelSquid.java`:10-34).
pub static MODEL_SQUID: Model = Model {
    parts: &SQUID_PARTS,
};

/// The slime's part order: the body, two eyes and the mouth (`ModelSlime.java`:7-17).
mod slime_slot {
    /// The body.
    #[allow(dead_code)]
    pub const BODIES: usize = 0;
    /// The right eye.
    #[allow(dead_code)]
    pub const RIGHT_EYE: usize = 1;
    /// The left eye.
    #[allow(dead_code)]
    pub const LEFT_EYE: usize = 2;
    /// The mouth.
    #[allow(dead_code)]
    pub const MOUTH: usize = 3;
}

/// The slime's mouth and eyes unit: the boxes whose size the renderer's lift scales
/// (`RenderSlime.preRenderCallback`:32-37 draws the eyes and mouth lifted by the squash).
static SLIME_BODY: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, 17.0, -3.0],
        size: [6.0, 6.0, 6.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The slime's right eye (`ModelSlime.java`:10-11).
static SLIME_RIGHT_EYE: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.25, 18.0, -3.5],
        size: [2.0, 2.0, 2.0],
        uv: [32.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The slime's left eye (`ModelSlime.java`:13-14).
static SLIME_LEFT_EYE: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [1.25, 18.0, -3.5],
        size: [2.0, 2.0, 2.0],
        uv: [32.0, 4.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The slime's mouth (`ModelSlime.java`:16-17).
static SLIME_MOUTH: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [0.0, 21.0, -3.5],
        size: [1.0, 1.0, 1.0],
        uv: [32.0, 8.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The slime's table (`ModelSlime.java`:21-33).
static SLIME_PARTS: [Part; 4] = [SLIME_BODY, SLIME_RIGHT_EYE, SLIME_LEFT_EYE, SLIME_MOUTH];

/// The slime's table (`ModelSlime.java`:21-33).
pub static MODEL_SLIME: Model = Model {
    parts: &SLIME_PARTS,
};

/// The gel layer's table: the zero-size slime's outer body alone
/// (`new ModelSlime(0)`, `LayerSlimeGel.java`:12), off the sheet's top-left cell.
static SLIME_GEL_PARTS: [Part; 1] = [Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, 16.0, -4.0],
        size: [8.0, 8.0, 8.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
}];

/// The gel layer's table (`LayerSlimeGel.java`:12).
pub static MODEL_SLIME_GEL: Model = Model {
    parts: &SLIME_GEL_PARTS,
};

/// The magma cube's part order: the core, then the eight segments
/// (`ModelMagmaCube.java`:14-35).
mod magma_slot {
    /// The core.
    #[allow(dead_code)]
    pub const CORE: usize = 0;
    /// The first segment.
    pub const SEGMENT0: usize = 1;
    /// The second segment.
    pub const SEGMENT1: usize = 2;
    /// The third segment.
    pub const SEGMENT2: usize = 3;
    /// The fourth segment.
    pub const SEGMENT3: usize = 4;
    /// The fifth segment.
    pub const SEGMENT4: usize = 5;
    /// The sixth segment.
    pub const SEGMENT5: usize = 6;
    /// The seventh segment.
    pub const SEGMENT6: usize = 7;
    /// The eighth segment.
    pub const SEGMENT7: usize = 8;
}

/// The magma cube's core (`ModelMagmaCube.java`:34-35).
static MAGMA_CORE: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-2.0, 18.0, -2.0],
        size: [4.0, 4.0, 4.0],
        uv: [0.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// One magma cube segment: eight wide and deep, one tall each, riding up one unit per
/// segment (`ModelMagmaCube.java`:14-32`).
static MAGMA_SEGMENT: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, 16.0, -4.0],
        size: [8.0, 1.0, 8.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The magma cube's table: the core and the eight segments, the second and third off their
/// own cells (`ModelMagmaCube.java`:14-35); the core draws first (`:63-69`).
static MAGMA_PARTS: [Part; 9] = [
    MAGMA_CORE,
    Part {
        boxes: &[Box {
            origin: [-4.0, 16.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 17.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [0.0, 1.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 18.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [24.0, 10.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 19.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [24.0, 19.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 20.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [0.0, 4.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 21.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [0.0, 5.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 22.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [0.0, 6.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
    Part {
        boxes: &[Box {
            origin: [-4.0, 23.0, -4.0],
            size: [8.0, 1.0, 8.0],
            uv: [0.0, 7.0],
            inflate: 0.0,
            mirror: false,
        }],
        ..MAGMA_SEGMENT
    },
];

/// The magma cube's table (`ModelMagmaCube.java`:14-35).
pub static MODEL_MAGMA_CUBE: Model = Model {
    parts: &MAGMA_PARTS,
};

/// The bat's part order: depth first, the head with its two ear children and the body with
/// each wing and its outer child (`ModelBat.java`:32-57, `ModelBat.java`:54-57).
mod bat_slot {
    /// The head.
    pub const HEAD: usize = 0;
    /// The right ear.
    #[allow(dead_code)]
    pub const RIGHT_EAR: usize = 1;
    /// The left ear.
    #[allow(dead_code)]
    pub const LEFT_EAR: usize = 2;
    /// The body.
    pub const BODY: usize = 3;
    /// The right wing.
    pub const RIGHT_WING: usize = 4;
    /// The right wing's outer plate.
    pub const OUTER_RIGHT_WING: usize = 5;
    /// The left wing.
    pub const LEFT_WING: usize = 6;
    /// The left wing's outer plate.
    pub const OUTER_LEFT_WING: usize = 7;
}

/// The bat's head (`ModelBat.java`:30-31).
static BAT_HEAD: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-3.0, -3.0, -3.0],
        size: [6.0, 6.0, 6.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The bat's right ear (`ModelBat.java`:32-34).
static BAT_RIGHT_EAR: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-4.0, -6.0, -2.0],
        size: [3.0, 4.0, 1.0],
        uv: [24.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The bat's left ear, the right's mirror (`ModelBat.java`:35-38).
static BAT_LEFT_EAR: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [1.0, -6.0, -2.0],
        size: [3.0, 4.0, 1.0],
        uv: [24.0, 0.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The bat's body: the torso and its chest plate (`ModelBat.java`:39-41).
static BAT_BODY: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[
        Box {
            origin: [-3.0, 4.0, -3.0],
            size: [6.0, 12.0, 6.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        },
        Box {
            origin: [-5.0, 16.0, 0.0],
            size: [10.0, 6.0, 1.0],
            uv: [0.0, 34.0],
            inflate: 0.0,
            mirror: false,
        },
    ],
    children: &[],
};

/// The bat's right wing (`ModelBat.java`:42-43).
static BAT_RIGHT_WING: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-12.0, 1.0, 1.5],
        size: [10.0, 16.0, 1.0],
        uv: [42.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[BAT_OUTER_RIGHT_WING],
};

/// The right wing's outer plate, hung at the wing's own pivot (`ModelBat.java`:44-46).
static BAT_OUTER_RIGHT_WING: Part = Part {
    point: [-12.0, 1.0, 1.5],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [-8.0, 1.0, 0.0],
        size: [8.0, 12.0, 1.0],
        uv: [24.0, 16.0],
        inflate: 0.0,
        mirror: false,
    }],
    children: &[],
};

/// The bat's left wing, the right's mirror (`ModelBat.java`:47-49).
static BAT_LEFT_WING: Part = Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [2.0, 1.0, 1.5],
        size: [10.0, 16.0, 1.0],
        uv: [42.0, 0.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[BAT_OUTER_LEFT_WING],
};

/// The left wing's outer plate (`ModelBat.java`:50-53).
static BAT_OUTER_LEFT_WING: Part = Part {
    point: [12.0, 1.0, 1.5],
    rest: [0.0, 0.0, 0.0],
    boxes: &[Box {
        origin: [0.0, 1.0, 0.0],
        size: [8.0, 12.0, 1.0],
        uv: [24.0, 16.0],
        inflate: 0.0,
        mirror: true,
    }],
    children: &[],
};

/// The bat's ears, head's children (`ModelBat.java`:32-38).
static BAT_EARS: [Part; 2] = [BAT_RIGHT_EAR, BAT_LEFT_EAR];

/// The bat's wings, body's children (`ModelBat.java`:41-57).
static BAT_WINGS: [Part; 2] = [BAT_RIGHT_WING, BAT_LEFT_WING];

/// The bat's root parts: the head and the body, each with its children nested
/// (`ModelBat.java`:32-57).
static BAT_PARTS: [Part; 2] = [
    Part {
        children: &BAT_EARS,
        ..BAT_HEAD
    },
    Part {
        children: &BAT_WINGS,
        ..BAT_BODY
    },
];

/// The bat's table (`ModelBat.java`:32-57). The wing plates nest inside their wings: the
/// left wing's outer plate at the wing's pivot, the right at the wing's own
/// (`ModelBat.java`:54-57).
pub static MODEL_BAT: Model = Model { parts: &BAT_PARTS };

/// The silverfish's part order: the seven bodies, then the three wings
/// (`ModelSilverfish.java`:25-47`).
mod silverfish_slot {
    /// The first body.
    #[allow(dead_code)]
    pub const BODY0: usize = 0;
    /// The second body.
    pub const BODY1: usize = 1;
    /// The third body.
    pub const BODY2: usize = 2;
    /// The fourth body.
    #[allow(dead_code)]
    pub const BODY3: usize = 3;
    /// The fifth body.
    pub const BODY4: usize = 4;
    /// The sixth body.
    #[allow(dead_code)]
    pub const BODY5: usize = 5;
    /// The seventh body.
    #[allow(dead_code)]
    pub const BODY6: usize = 6;
    /// The first wing.
    pub const WING0: usize = 7;
    /// The second wing.
    pub const WING1: usize = 8;
    /// The third wing.
    pub const WING2: usize = 9;
}

/// The silverfish's bodies: width, height and length off the class's table, each hung one
/// reach further back than the last (`ModelSilverfish.java`:16-37`).
static SILVERFISH_PARTS: [Part; 10] = [
    Part {
        point: [0.0, 22.0, -3.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, 0.0, -1.0],
            size: [3.0, 2.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 21.0, -1.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.0, 0.0, -1.0],
            size: [4.0, 3.0, 2.0],
            uv: [0.0, 4.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, 1.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-3.0, 0.0, -1.5],
            size: [6.0, 4.0, 3.0],
            uv: [0.0, 9.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 21.0, 4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, 0.0, -1.5],
            size: [3.0, 3.0, 3.0],
            uv: [0.0, 16.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 22.0, 7.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.5],
            size: [2.0, 2.0, 3.0],
            uv: [0.0, 22.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 23.0, 9.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.0, 0.0, -1.0],
            size: [2.0, 1.0, 2.0],
            uv: [11.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 23.0, 11.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -1.0],
            size: [1.0, 1.0, 2.0],
            uv: [13.0, 4.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    // The wings: the second, third and fifth bodies' pivots, with their cells' plates
    // (`ModelSilverfish.java`:39-47`).
    Part {
        point: [0.0, 16.0, 1.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-5.0, 0.0, -1.5],
            size: [10.0, 8.0, 3.0],
            uv: [20.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, 7.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-3.0, 0.0, -1.5],
            size: [6.0, 4.0, 3.0],
            uv: [20.0, 11.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 19.0, -1.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-3.0, 0.0, -1.5],
            size: [6.0, 5.0, 2.0],
            uv: [20.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The silverfish's table (`ModelSilverfish.java`:25-47`).
pub static MODEL_SILVERFISH: Model = Model {
    parts: &SILVERFISH_PARTS,
};

/// The endermite's table: four segments, shrinking nose to tail
/// (`ModelEnderMite.java`:18-28`).
static ENDERMITE_PARTS: [Part; 4] = [
    Part {
        point: [0.0, 21.0, -3.5],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-2.0, 0.0, -1.0],
            size: [4.0, 3.0, 2.0],
            uv: [0.0, 0.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 20.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-3.0, 0.0, -2.5],
            size: [6.0, 4.0, 5.0],
            uv: [0.0, 5.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 21.0, 3.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-1.5, 0.0, -0.5],
            size: [3.0, 3.0, 1.0],
            uv: [0.0, 14.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
    Part {
        point: [0.0, 22.0, 4.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &[Box {
            origin: [-0.5, 0.0, -0.5],
            size: [1.0, 2.0, 1.0],
            uv: [0.0, 18.0],
            inflate: 0.0,
            mirror: false,
        }],
        children: &[],
    },
];

/// The endermite's table (`ModelEnderMite.java`:18-28`).
pub static MODEL_ENDERMITE: Model = Model {
    parts: &ENDERMITE_PARTS,
};

/// The source's own degree-to-radian division, spelled as every model spells it
/// (`netHeadYaw / (180F / (float)Math.PI)`).
fn degrees(value: f32) -> f32 {
    value / (180.0 / std::f32::consts::PI)
}

/// The creeper's pose (`ModelCreeper.setRotationAngles`:66): the head takes the frame's
/// angles, the four legs swing in the class's own phase order.
pub fn pose_creeper(pose: &Pose, out: &mut [Rot]) {
    out[creeper_slot::HEAD].angles[1] = degrees(pose.head_yaw);
    out[creeper_slot::HEAD].angles[0] = degrees(pose.head_pitch);
    let phase = pose.limb_swing * 0.6662;
    out[creeper_slot::LEG1].angles[0] = phase.cos() * 1.4 * pose.limb_swing_amount;
    out[creeper_slot::LEG2].angles[0] =
        (phase + std::f32::consts::PI).cos() * 1.4 * pose.limb_swing_amount;
    out[creeper_slot::LEG3].angles[0] =
        (phase + std::f32::consts::PI).cos() * 1.4 * pose.limb_swing_amount;
    out[creeper_slot::LEG4].angles[0] = phase.cos() * 1.4 * pose.limb_swing_amount;
}

/// The spider's pose (`ModelSpider.setRotationAngles`:104): the head's angles, then the
/// eight legs' rest fan swung and lifted by the class's own phase pairs — the swings on the
/// doubled stride phase, the lifts on the single one (`ModelSpider.java`:127-134).
pub fn pose_spider(pose: &Pose, out: &mut [Rot]) {
    out[spider_slot::HEAD].angles[1] = degrees(pose.head_yaw);
    out[spider_slot::HEAD].angles[0] = degrees(pose.head_pitch);
    let swing_phase = pose.limb_swing * 0.6662 * 2.0;
    let lift_phase = pose.limb_swing * 0.6662;
    let swing = |offset: f32| -((swing_phase + offset).cos() * 0.4) * pose.limb_swing_amount;
    let lift = |offset: f32| (lift_phase + offset).sin().abs() * 0.4 * pose.limb_swing_amount;
    let quarter = std::f32::consts::FRAC_PI_2;
    let three_quarters = std::f32::consts::PI * 3.0 / 2.0;
    out[spider_slot::LEG1].angles[1] += swing(0.0);
    out[spider_slot::LEG2].angles[1] += -swing(0.0);
    out[spider_slot::LEG3].angles[1] += swing(std::f32::consts::PI);
    out[spider_slot::LEG4].angles[1] += -swing(std::f32::consts::PI);
    out[spider_slot::LEG5].angles[1] += swing(quarter);
    out[spider_slot::LEG6].angles[1] += -swing(quarter);
    out[spider_slot::LEG7].angles[1] += swing(three_quarters);
    out[spider_slot::LEG8].angles[1] += -swing(three_quarters);
    out[spider_slot::LEG1].angles[2] += lift(0.0);
    out[spider_slot::LEG2].angles[2] += -lift(0.0);
    out[spider_slot::LEG3].angles[2] += lift(std::f32::consts::PI);
    out[spider_slot::LEG4].angles[2] += -lift(std::f32::consts::PI);
    out[spider_slot::LEG5].angles[2] += lift(quarter);
    out[spider_slot::LEG6].angles[2] += -lift(quarter);
    out[spider_slot::LEG7].angles[2] += lift(three_quarters);
    out[spider_slot::LEG8].angles[2] += -lift(three_quarters);
}

/// The enderman's pose (`ModelEnderman.setRotationAngles`:44, over `ModelBiped`'s walk):
/// the gathered limbs halved and clamped, the long body and head hung low.
pub fn pose_enderman(pose: &Pose, out: &mut [Rot]) {
    // The base walk's own swings (`ModelBiped.setRotationAngles`:133-138).
    let phase = pose.limb_swing * 0.6662;
    let amount = pose.limb_swing_amount;
    let right_arm = (phase + std::f32::consts::PI).cos() * 2.0 * amount * 0.5;
    let left_arm = phase.cos() * 2.0 * amount * 0.5;
    let right_leg = phase.cos() * 1.4 * amount;
    let left_leg = (phase + std::f32::consts::PI).cos() * 1.4 * amount;
    // Then the class's own fold: halved, then clamped to ±0.4.
    out[enderman_slot::RIGHT_ARM].angles[0] = (right_arm * 0.5).clamp(-0.4, 0.4);
    out[enderman_slot::LEFT_ARM].angles[0] = (left_arm * 0.5).clamp(-0.4, 0.4);
    out[enderman_slot::RIGHT_LEG].angles[0] = (right_leg * 0.5).clamp(-0.4, 0.4);
    out[enderman_slot::LEFT_LEG].angles[0] = (left_leg * 0.5).clamp(-0.4, 0.4);
    out[enderman_slot::RIGHT_ARM].point[2] = 0.0;
    out[enderman_slot::LEFT_ARM].point[2] = 0.0;
    out[enderman_slot::RIGHT_LEG].point[2] = 0.0;
    out[enderman_slot::LEFT_LEG].point[2] = 0.0;
    out[enderman_slot::RIGHT_LEG].point[1] = -5.0;
    out[enderman_slot::LEFT_LEG].point[1] = -5.0;
    // The body and head hang at the class's own offsets; the headwear rides the head.
    out[enderman_slot::BODY].angles[0] = 0.0;
    out[enderman_slot::BODY].point[1] = -14.0;
    out[enderman_slot::BODY].point[2] = 0.0;
    out[enderman_slot::HEAD].angles[1] = degrees(pose.head_yaw);
    out[enderman_slot::HEAD].angles[0] = degrees(pose.head_pitch);
    out[enderman_slot::HEAD].point[1] = -13.0;
    out[enderman_slot::HEAD].point[2] = 0.0;
    out[enderman_slot::HEADWEAR].point = out[enderman_slot::HEAD].point;
    out[enderman_slot::HEADWEAR].angles = out[enderman_slot::HEAD].angles;
    if let PoseExtra::Enderman { attacking: true } = pose.extra {
        out[enderman_slot::HEAD].point[1] -= 5.0;
        out[enderman_slot::HEADWEAR].point = out[enderman_slot::HEAD].point;
    }
    // The carried-block arm fold awaits the held-item class.
}

/// The chicken's pose (`ModelChicken.setRotationAngles`:91): the head's angles over the
/// bill and chin, the body flat, the legs' step and the wings' flap.
pub fn pose_chicken(pose: &Pose, out: &mut [Rot]) {
    out[chicken_slot::HEAD].angles[1] = degrees(pose.head_yaw);
    out[chicken_slot::HEAD].angles[0] = degrees(pose.head_pitch);
    out[chicken_slot::BILL].angles[0] = out[chicken_slot::HEAD].angles[0];
    out[chicken_slot::BILL].angles[1] = out[chicken_slot::HEAD].angles[1];
    out[chicken_slot::CHIN].angles[0] = out[chicken_slot::HEAD].angles[0];
    out[chicken_slot::CHIN].angles[1] = out[chicken_slot::HEAD].angles[1];
    out[chicken_slot::BODY].angles[0] = std::f32::consts::FRAC_PI_2;
    let phase = pose.limb_swing * 0.6662;
    out[chicken_slot::RIGHT_LEG].angles[0] = phase.cos() * 1.4 * pose.limb_swing_amount;
    out[chicken_slot::LEFT_LEG].angles[0] =
        (phase + std::f32::consts::PI).cos() * 1.4 * pose.limb_swing_amount;
    let flap = match pose.extra {
        PoseExtra::Chicken { flap } => flap,
        _ => 0.0,
    };
    out[chicken_slot::RIGHT_WING].angles[2] = flap;
    out[chicken_slot::LEFT_WING].angles[2] = -flap;
}

/// The squid's pose (`ModelSquid.setRotationAngles`:40): every tentacle turns about x by the
/// one interpolated tentacle angle.
pub fn pose_squid(pose: &Pose, out: &mut [Rot]) {
    let angle = match pose.extra {
        PoseExtra::Squid { tentacle_angle } => tentacle_angle,
        _ => 0.0,
    };
    let tentacles = [
        squid_slot::TENTACLE0,
        squid_slot::TENTACLE1,
        squid_slot::TENTACLE2,
        squid_slot::TENTACLE3,
        squid_slot::TENTACLE4,
        squid_slot::TENTACLE5,
        squid_slot::TENTACLE6,
        squid_slot::TENTACLE7,
    ];
    for slot in tentacles {
        out[slot].angles[0] = angle;
    }
}

/// The slime's pose: the class's model overrides none (`ModelSlime` has no
/// `setRotationAngles`), so every frame draws the rest table.
pub fn pose_slime(_pose: &Pose, _out: &mut [Rot]) {}

/// The magma cube's pose (`ModelMagmaCube.setLivingAnimations`:42): the squash slides every
/// segment down its own share of the drop.
pub fn pose_magma_cube(pose: &Pose, out: &mut [Rot]) {
    let squish = match pose.extra {
        PoseExtra::MagmaCube { squish } => squish.clamp(0.0, 1.0),
        _ => 0.0,
    };
    let segments = [
        magma_slot::SEGMENT0,
        magma_slot::SEGMENT1,
        magma_slot::SEGMENT2,
        magma_slot::SEGMENT3,
        magma_slot::SEGMENT4,
        magma_slot::SEGMENT5,
        magma_slot::SEGMENT6,
        magma_slot::SEGMENT7,
    ];
    for (index, slot) in segments.into_iter().enumerate() {
        out[slot].point[1] = -(4.0 - index as f32) * 1.7 * squish;
    }
}

/// The bat's pose (`ModelBat.setRotationAngles`:75): hanging, the fold; flying, the body and
/// wings beating on the age.
pub fn pose_bat(pose: &Pose, out: &mut [Rot]) {
    let hanging = match pose.extra {
        PoseExtra::Bat { hanging } => hanging,
        _ => false,
    };
    if hanging {
        out[bat_slot::HEAD].angles[0] = degrees(pose.head_pitch);
        out[bat_slot::HEAD].angles[1] = std::f32::consts::PI - degrees(pose.head_yaw);
        out[bat_slot::HEAD].angles[2] = std::f32::consts::PI;
        out[bat_slot::HEAD].point = [0.0, -2.0, 0.0];
        out[bat_slot::RIGHT_WING].point = [-3.0, 0.0, 3.0];
        out[bat_slot::LEFT_WING].point = [3.0, 0.0, 3.0];
        out[bat_slot::BODY].angles[0] = std::f32::consts::PI;
        out[bat_slot::RIGHT_WING].angles[0] = -0.15707964;
        out[bat_slot::RIGHT_WING].angles[1] = -1.2566371;
        out[bat_slot::OUTER_RIGHT_WING].angles[1] = -1.7278761;
        out[bat_slot::LEFT_WING].angles[0] = out[bat_slot::RIGHT_WING].angles[0];
        out[bat_slot::LEFT_WING].angles[1] = -out[bat_slot::RIGHT_WING].angles[1];
        out[bat_slot::OUTER_LEFT_WING].angles[1] = -out[bat_slot::OUTER_RIGHT_WING].angles[1];
    } else {
        out[bat_slot::HEAD].angles[0] = degrees(pose.head_pitch);
        out[bat_slot::HEAD].angles[1] = degrees(pose.head_yaw);
        out[bat_slot::HEAD].angles[2] = 0.0;
        out[bat_slot::HEAD].point = [0.0, 0.0, 0.0];
        out[bat_slot::RIGHT_WING].point = [0.0, 0.0, 0.0];
        out[bat_slot::LEFT_WING].point = [0.0, 0.0, 0.0];
        out[bat_slot::BODY].angles[0] = std::f32::consts::FRAC_PI_4 + (pose.age * 0.1).cos() * 0.15;
        out[bat_slot::BODY].angles[1] = 0.0;
        let sweep = (pose.age * 1.3).cos() * std::f32::consts::PI * 0.25;
        out[bat_slot::RIGHT_WING].angles[1] = sweep;
        out[bat_slot::LEFT_WING].angles[1] = -sweep;
        out[bat_slot::OUTER_RIGHT_WING].angles[1] = sweep * 0.5;
        out[bat_slot::OUTER_LEFT_WING].angles[1] = -sweep * 0.5;
    }
}

/// The silverfish's pose (`ModelSilverfish.setRotationAngles`:73): the seven bodies sway
/// along the age's wave, the wings copying theirs.
pub fn pose_silverfish(pose: &Pose, out: &mut [Rot]) {
    for (index, rot) in out.iter_mut().enumerate().take(7) {
        let phase = pose.age * 0.9 + index as f32 * 0.15 * std::f32::consts::PI;
        let spread = (index as f32 - 2.0).abs();
        rot.angles[1] = phase.cos() * std::f32::consts::PI * 0.05 * (1.0 + spread);
        rot.point[0] = phase.sin() * std::f32::consts::PI * 0.2 * spread;
    }
    out[silverfish_slot::WING0].angles[1] = out[silverfish_slot::BODY2].angles[1];
    out[silverfish_slot::WING1].angles[1] = out[silverfish_slot::BODY4].angles[1];
    out[silverfish_slot::WING1].point[0] = out[silverfish_slot::BODY4].point[0];
    out[silverfish_slot::WING2].angles[1] = out[silverfish_slot::BODY1].angles[1];
    out[silverfish_slot::WING2].point[0] = out[silverfish_slot::BODY1].point[0];
}

/// The endermite's pose (`ModelEnderMite.setRotationAngles`:49): the silverfish's own sway
/// rule, a fifth of the reach.
pub fn pose_endermite(pose: &Pose, out: &mut [Rot]) {
    for (index, rot) in out.iter_mut().enumerate().take(4) {
        let phase = pose.age * 0.9 + index as f32 * 0.15 * std::f32::consts::PI;
        let spread = (index as f32 - 2.0).abs();
        rot.angles[1] = phase.cos() * std::f32::consts::PI * 0.01 * (1.0 + spread);
        rot.point[0] = phase.sin() * std::f32::consts::PI * 0.1 * spread;
    }
}

/// The cube family's render scale: the squash term divided by half the size plus one, then
/// the pair folded into the size (`RenderSlime.preRenderCallback`:32 and
/// `RenderMagmaCube.preRenderCallback`:29).
pub fn cube_scale(size: u8, squish: f32) -> [f32; 3] {
    let f = f32::from(size);
    let f1 = squish / (f * 0.5 + 1.0);
    let f2 = 1.0 / (f1 + 1.0);
    let wide = f2 * f;
    let tall = (1.0 / f2) * f;
    [wide, tall, wide]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_models::{Box, Pose, PoseExtra};

    /// The slot-th part in the model's draw order, depth first, the order `Model::rest`
    /// flattens the nested parts in.
    fn part_at(model: &Model, slot: usize) -> &'static Part {
        fn walk(part: &'static Part, cursor: &mut usize, want: usize) -> Option<&'static Part> {
            if *cursor == want {
                return Some(part);
            }
            *cursor += 1;
            for child in part.children {
                if let Some(found) = walk(child, cursor, want) {
                    return Some(found);
                }
            }
            None
        }
        let mut cursor = 0;
        for part in model.parts {
            if let Some(found) = walk(part, &mut cursor, slot) {
                return found;
            }
        }
        panic!("no part at slot {slot}");
    }

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
    fn the_creeper_is_the_sources_six_boxes() {
        assert_eq!(MODEL_CREEPER.parts.len(), 6, "the creeper's six boxes draw");
        let head = &MODEL_CREEPER.parts[creeper_slot::HEAD];
        assert_eq!(head.point, [0.0, 6.0, 0.0]);
        assert_eq!(
            head.boxes,
            [b(
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        let body = &MODEL_CREEPER.parts[creeper_slot::BODY];
        assert_eq!(body.point, [0.0, 6.0, 0.0]);
        assert_eq!(
            body.boxes,
            [b(
                [-4.0, 0.0, -2.0],
                [8.0, 12.0, 4.0],
                [16.0, 16.0],
                0.0,
                false
            )]
        );
        // The four legs at the body's corners, all off the one sixteen-offset cell.
        let legs = [
            (creeper_slot::LEG1, [-2.0, 18.0, 4.0]),
            (creeper_slot::LEG2, [2.0, 18.0, 4.0]),
            (creeper_slot::LEG3, [-2.0, 18.0, -4.0]),
            (creeper_slot::LEG4, [2.0, 18.0, -4.0]),
        ];
        for (slot, point) in legs {
            let leg = &MODEL_CREEPER.parts[slot];
            assert_eq!(leg.point, point);
            assert_eq!(
                leg.boxes,
                [b(
                    [-2.0, 0.0, -2.0],
                    [4.0, 6.0, 4.0],
                    [0.0, 16.0],
                    0.0,
                    false
                )]
            );
        }
    }

    #[test]
    fn the_spider_stands_on_eight_legs() {
        assert_eq!(
            MODEL_SPIDER.parts.len(),
            11,
            "the head, neck, body and eight legs"
        );
        assert_eq!(
            MODEL_SPIDER.parts[spider_slot::HEAD].point,
            [0.0, 15.0, -3.0]
        );
        assert_eq!(
            MODEL_SPIDER.parts[spider_slot::HEAD].boxes,
            [b(
                [-4.0, -4.0, -8.0],
                [8.0, 8.0, 8.0],
                [32.0, 4.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SPIDER.parts[spider_slot::NECK].point,
            [0.0, 15.0, 0.0]
        );
        assert_eq!(
            MODEL_SPIDER.parts[spider_slot::NECK].boxes,
            [b(
                [-3.0, -3.0, -3.0],
                [6.0, 6.0, 6.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SPIDER.parts[spider_slot::BODY].point,
            [0.0, 15.0, 9.0]
        );
        assert_eq!(
            MODEL_SPIDER.parts[spider_slot::BODY].boxes,
            [b(
                [-5.0, -4.0, -6.0],
                [10.0, 8.0, 12.0],
                [0.0, 12.0],
                0.0,
                false
            )]
        );
        // The legs alternate sides: the odd ones reach out from the left flank, the even
        // ones from the right, both sixteen long, and their pivots walk the body's z from
        // two to minus one in pairs (`ModelSpider.java`:54-77).
        let legs = [
            (spider_slot::LEG1, 2.0),
            (spider_slot::LEG2, 2.0),
            (spider_slot::LEG3, 1.0),
            (spider_slot::LEG4, 1.0),
            (spider_slot::LEG5, 0.0),
            (spider_slot::LEG6, 0.0),
            (spider_slot::LEG7, -1.0),
            (spider_slot::LEG8, -1.0),
        ];
        for (index, (slot, z)) in legs.into_iter().enumerate() {
            let leg = &MODEL_SPIDER.parts[slot];
            if index % 2 == 0 {
                assert_eq!(leg.point, [-4.0, 15.0, z]);
                assert_eq!(
                    leg.boxes,
                    [b(
                        [-15.0, -1.0, -1.0],
                        [16.0, 2.0, 2.0],
                        [18.0, 0.0],
                        0.0,
                        false
                    )]
                );
            } else {
                assert_eq!(leg.point, [4.0, 15.0, z]);
                assert_eq!(
                    leg.boxes,
                    [b(
                        [-1.0, -1.0, -1.0],
                        [16.0, 2.0, 2.0],
                        [18.0, 0.0],
                        0.0,
                        false
                    )]
                );
            }
        }
    }

    #[test]
    fn the_enderman_is_the_biped_table_outstretched() {
        assert_eq!(
            MODEL_ENDERMAN.parts.len(),
            7,
            "the enderman's seven parts draw"
        );
        let head = &MODEL_ENDERMAN.parts[enderman_slot::HEAD];
        assert_eq!(head.point, [0.0, -14.0, 0.0]);
        assert_eq!(
            head.boxes,
            [b(
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::BODY].point,
            [0.0, -14.0, 0.0]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::BODY].boxes,
            [b(
                [-4.0, 0.0, -2.0],
                [8.0, 12.0, 4.0],
                [32.0, 16.0],
                0.0,
                false
            )]
        );
        // The arms are two by thirty, hung at the shoulders; the left mirrors.
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::RIGHT_ARM].boxes,
            [b(
                [-1.0, -2.0, -1.0],
                [2.0, 30.0, 2.0],
                [56.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::RIGHT_ARM].point,
            [-3.0, -12.0, 0.0]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::LEFT_ARM].boxes,
            [b(
                [-1.0, -2.0, -1.0],
                [2.0, 30.0, 2.0],
                [56.0, 0.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::LEFT_ARM].point,
            [5.0, -12.0, 0.0]
        );
        // The legs are thirty long too, hung from the hips.
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::RIGHT_LEG].boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [2.0, 30.0, 2.0],
                [56.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::RIGHT_LEG].point,
            [-2.0, -2.0, 0.0]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::LEFT_LEG].boxes,
            [b(
                [-1.0, 0.0, -1.0],
                [2.0, 30.0, 2.0],
                [56.0, 0.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::LEFT_LEG].point,
            [2.0, -2.0, 0.0]
        );
        // The headwear is the head's own box at the enderman's quarter inflation
        // (`p_i46305_1_ - 0.5F` over the head's zero).
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::HEADWEAR].boxes,
            [b(
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 16.0],
                -0.5,
                false
            )]
        );
        assert_eq!(
            MODEL_ENDERMAN.parts[enderman_slot::HEADWEAR].point,
            [0.0, -14.0, 0.0]
        );
    }

    #[test]
    fn the_chicken_is_eight_boxes_and_its_child_is_the_renderers_fold() {
        assert_eq!(MODEL_CHICKEN.parts.len(), 8, "the chicken's eight parts");
        let parts = [
            (chicken_slot::HEAD, [0.0, 15.0, -4.0]),
            (chicken_slot::BILL, [0.0, 15.0, -4.0]),
            (chicken_slot::CHIN, [0.0, 15.0, -4.0]),
            (chicken_slot::BODY, [0.0, 16.0, 0.0]),
            (chicken_slot::RIGHT_LEG, [-2.0, 19.0, 1.0]),
            (chicken_slot::LEFT_LEG, [1.0, 19.0, 1.0]),
            (chicken_slot::RIGHT_WING, [-4.0, 13.0, 0.0]),
            (chicken_slot::LEFT_WING, [4.0, 13.0, 0.0]),
        ];
        for (slot, point) in parts {
            assert_eq!(MODEL_CHICKEN.parts[slot].point, point);
        }
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::HEAD].boxes,
            [b(
                [-2.0, -6.0, -2.0],
                [4.0, 6.0, 3.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::BILL].boxes,
            [b(
                [-2.0, -4.0, -4.0],
                [4.0, 2.0, 2.0],
                [14.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::CHIN].boxes,
            [b(
                [-1.0, -2.0, -3.0],
                [2.0, 2.0, 2.0],
                [14.0, 4.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::BODY].boxes,
            [b(
                [-3.0, -4.0, -3.0],
                [6.0, 8.0, 6.0],
                [0.0, 9.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::RIGHT_LEG].boxes,
            [b(
                [-1.0, 0.0, -3.0],
                [3.0, 5.0, 3.0],
                [26.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::LEFT_LEG].boxes,
            [b(
                [-1.0, 0.0, -3.0],
                [3.0, 5.0, 3.0],
                [26.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::RIGHT_WING].boxes,
            [b(
                [0.0, 0.0, -3.0],
                [1.0, 4.0, 6.0],
                [24.0, 13.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_CHICKEN.parts[chicken_slot::LEFT_WING].boxes,
            [b(
                [-1.0, 0.0, -3.0],
                [1.0, 4.0, 6.0],
                [24.0, 13.0],
                0.0,
                false
            )]
        );

        // The chick: the renderer's own fold (`ModelChicken.render`:54-72) translates the
        // head group down five and out two — the head, bill and chin in place — and halves
        // the body group's boxes and pivots about a twelve-unit drop.
        assert_eq!(MODEL_CHICKEN_CHILD.parts.len(), 8);
        let child = &MODEL_CHICKEN_CHILD;
        assert_eq!(child.parts[chicken_slot::HEAD].point, [0.0, 20.0, -2.0]);
        assert_eq!(
            child.parts[chicken_slot::HEAD].boxes,
            [b(
                [-2.0, -6.0, -2.0],
                [4.0, 6.0, 3.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(child.parts[chicken_slot::BILL].point, [0.0, 20.0, -2.0]);
        assert_eq!(child.parts[chicken_slot::CHIN].point, [0.0, 20.0, -2.0]);
        assert_eq!(child.parts[chicken_slot::BODY].point, [0.0, 20.0, 0.0]);
        assert_eq!(
            child.parts[chicken_slot::BODY].boxes,
            [b(
                [-1.5, -2.0, -1.5],
                [3.0, 4.0, 3.0],
                [0.0, 9.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            child.parts[chicken_slot::RIGHT_LEG].point,
            [-1.0, 21.5, 0.5]
        );
        assert_eq!(
            child.parts[chicken_slot::RIGHT_LEG].boxes,
            [b(
                [-0.5, 0.0, -1.5],
                [1.5, 2.5, 1.5],
                [26.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(child.parts[chicken_slot::LEFT_LEG].point, [0.5, 21.5, 0.5]);
        assert_eq!(
            child.parts[chicken_slot::RIGHT_WING].point,
            [-2.0, 18.5, 0.0]
        );
        assert_eq!(
            child.parts[chicken_slot::RIGHT_WING].boxes,
            [b(
                [0.0, 0.0, -1.5],
                [0.5, 2.0, 3.0],
                [24.0, 13.0],
                0.0,
                false
            )]
        );
        assert_eq!(child.parts[chicken_slot::LEFT_WING].point, [2.0, 18.5, 0.0]);
        assert_eq!(
            child.parts[chicken_slot::LEFT_WING].boxes,
            [b(
                [-0.5, 0.0, -1.5],
                [0.5, 2.0, 3.0],
                [24.0, 13.0],
                0.0,
                false
            )]
        );
    }

    #[test]
    fn the_squid_hangs_eight_tentacles_from_its_fan() {
        assert_eq!(MODEL_SQUID.parts.len(), 9, "the body and eight tentacles");
        assert_eq!(MODEL_SQUID.parts[squid_slot::BODY].point, [0.0, 8.0, 0.0]);
        assert_eq!(
            MODEL_SQUID.parts[squid_slot::BODY].boxes,
            [b(
                [-6.0, -8.0, -6.0],
                [12.0, 16.0, 12.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        // The fan: the j'th tentacle stands at the eighth-turn point of the ring, five units
        // out, hanging from fifteen, its rest yaw an eighth-turn walk from the half turn
        // (`ModelSquid.java`:20-32).
        let tentacles = [
            (squid_slot::TENTACLE0, [5.0, 15.0, 0.0], 1.5707964),
            (
                squid_slot::TENTACLE1,
                [3.535_534, 15.0, 3.535_534],
                std::f32::consts::FRAC_PI_4,
            ),
            (squid_slot::TENTACLE2, [0.0, 15.0, 5.0], 0.0),
            (
                squid_slot::TENTACLE3,
                [-3.535_534, 15.0, 3.535_534],
                -std::f32::consts::FRAC_PI_4,
            ),
            (squid_slot::TENTACLE4, [-5.0, 15.0, 0.0], -1.5707964),
            (
                squid_slot::TENTACLE5,
                [-3.535_534, 15.0, -3.535_534],
                -2.3561945,
            ),
            (
                squid_slot::TENTACLE6,
                [0.0, 15.0, -5.0],
                -std::f32::consts::PI,
            ),
            (
                squid_slot::TENTACLE7,
                [3.535_534, 15.0, -3.535_534],
                -3.9269908,
            ),
        ];
        for (slot, point, rest_yaw) in tentacles {
            let tentacle = &MODEL_SQUID.parts[slot];
            assert!(
                (tentacle.point[0] - point[0]).abs() < 1.0e-5
                    && tentacle.point[1] == point[1]
                    && (tentacle.point[2] - point[2]).abs() < 1.0e-5,
                "the tentacle at the ring's point {point:?}, got {:?}",
                tentacle.point
            );
            assert!(
                (tentacle.rest[1] - rest_yaw).abs() < 1.0e-5,
                "the tentacle's rest yaw {rest_yaw}, got {}",
                tentacle.rest[1]
            );
            assert_eq!(tentacle.rest[0], 0.0);
            assert_eq!(tentacle.rest[2], 0.0);
            assert_eq!(
                tentacle.boxes,
                [b(
                    [-1.0, 0.0, -1.0],
                    [2.0, 18.0, 2.0],
                    [48.0, 0.0],
                    0.0,
                    false
                )]
            );
        }
    }

    #[test]
    fn the_slime_and_its_gel_are_the_two_bodies() {
        assert_eq!(
            MODEL_SLIME.parts.len(),
            4,
            "the body, two eyes and the mouth"
        );
        assert_eq!(MODEL_SLIME.parts[slime_slot::BODIES].point, [0.0, 0.0, 0.0]);
        assert_eq!(
            MODEL_SLIME.parts[slime_slot::BODIES].boxes,
            [b(
                [-3.0, 17.0, -3.0],
                [6.0, 6.0, 6.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SLIME.parts[slime_slot::RIGHT_EYE].boxes,
            [b(
                [-3.25, 18.0, -3.5],
                [2.0, 2.0, 2.0],
                [32.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SLIME.parts[slime_slot::LEFT_EYE].boxes,
            [b(
                [1.25, 18.0, -3.5],
                [2.0, 2.0, 2.0],
                [32.0, 4.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SLIME.parts[slime_slot::MOUTH].boxes,
            [b(
                [0.0, 21.0, -3.5],
                [1.0, 1.0, 1.0],
                [32.0, 8.0],
                0.0,
                false
            )]
        );
        // The gel: the zero-size slime's outer body alone (`new ModelSlime(0)`,
        // `LayerSlimeGel.java`:12), off the sheet's top-left cell.
        assert_eq!(MODEL_SLIME_GEL.parts.len(), 1);
        assert_eq!(
            MODEL_SLIME_GEL.parts[0].boxes,
            [b(
                [-4.0, 16.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
    }

    #[test]
    fn the_magmacube_stacks_eight_segments_over_its_core() {
        assert_eq!(
            MODEL_MAGMA_CUBE.parts.len(),
            9,
            "the core and eight segments"
        );
        assert_eq!(
            MODEL_MAGMA_CUBE.parts[magma_slot::CORE].boxes,
            [b(
                [-2.0, 18.0, -2.0],
                [4.0, 4.0, 4.0],
                [0.0, 16.0],
                0.0,
                false
            )]
        );
        // The segments are drawn after the core (`ModelMagmaCube.render`:63-69), one unit
        // tall each, the second and third off their own cells (`ModelMagmaCube.java`:14-32).
        let segments = [
            (magma_slot::SEGMENT0, 16.0, [0.0, 0.0]),
            (magma_slot::SEGMENT1, 17.0, [0.0, 1.0]),
            (magma_slot::SEGMENT2, 18.0, [24.0, 10.0]),
            (magma_slot::SEGMENT3, 19.0, [24.0, 19.0]),
            (magma_slot::SEGMENT4, 20.0, [0.0, 4.0]),
            (magma_slot::SEGMENT5, 21.0, [0.0, 5.0]),
            (magma_slot::SEGMENT6, 22.0, [0.0, 6.0]),
            (magma_slot::SEGMENT7, 23.0, [0.0, 7.0]),
        ];
        for (slot, bottom, uv) in segments {
            assert_eq!(
                MODEL_MAGMA_CUBE.parts[slot].boxes,
                [b([-4.0, bottom, -4.0], [8.0, 1.0, 8.0], uv, 0.0, false)]
            );
        }
    }

    #[test]
    fn the_bat_nests_its_ears_and_wings() {
        // Depth first, as `ModelBat.render`: the head with its two ear children, then the
        // body with each wing and its outer child (`ModelBat.java`:54-57).
        assert_eq!(
            MODEL_BAT.rest().len(),
            8,
            "the bat's eight parts, children included"
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::HEAD).boxes,
            [b(
                [-3.0, -3.0, -3.0],
                [6.0, 6.0, 6.0],
                [0.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::RIGHT_EAR).boxes,
            [b(
                [-4.0, -6.0, -2.0],
                [3.0, 4.0, 1.0],
                [24.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::LEFT_EAR).boxes,
            [b(
                [1.0, -6.0, -2.0],
                [3.0, 4.0, 1.0],
                [24.0, 0.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::BODY).boxes,
            [
                b([-3.0, 4.0, -3.0], [6.0, 12.0, 6.0], [0.0, 16.0], 0.0, false),
                b([-5.0, 16.0, 0.0], [10.0, 6.0, 1.0], [0.0, 34.0], 0.0, false)
            ]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::RIGHT_WING).boxes,
            [b(
                [-12.0, 1.0, 1.5],
                [10.0, 16.0, 1.0],
                [42.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::OUTER_RIGHT_WING).point,
            [-12.0, 1.0, 1.5]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::OUTER_RIGHT_WING).boxes,
            [b(
                [-8.0, 1.0, 0.0],
                [8.0, 12.0, 1.0],
                [24.0, 16.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::LEFT_WING).boxes,
            [b(
                [2.0, 1.0, 1.5],
                [10.0, 16.0, 1.0],
                [42.0, 0.0],
                0.0,
                true
            )]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::OUTER_LEFT_WING).point,
            [12.0, 1.0, 1.5]
        );
        assert_eq!(
            part_at(&MODEL_BAT, bat_slot::OUTER_LEFT_WING).boxes,
            [b(
                [0.0, 1.0, 0.0],
                [8.0, 12.0, 1.0],
                [24.0, 16.0],
                0.0,
                true
            )]
        );
    }

    #[test]
    fn the_silverfish_segments_advance_along_the_body() {
        assert_eq!(
            MODEL_SILVERFISH.parts.len(),
            10,
            "seven bodies and three wings"
        );
        // The bodies: widths, heights and lengths off the class's table, each hung one
        // reach further back than the last, and the wings copy the second, third and fifth
        // bodies' cells and pivots (`ModelSilverfish.java`:16-47, `:39-47`).
        let bodies = [
            (
                silverfish_slot::BODY0,
                [0.0, 22.0, -3.5],
                [-1.5, 0.0, -1.0],
                [3.0, 2.0, 2.0],
                [0.0, 0.0],
            ),
            (
                silverfish_slot::BODY1,
                [0.0, 21.0, -1.5],
                [-2.0, 0.0, -1.0],
                [4.0, 3.0, 2.0],
                [0.0, 4.0],
            ),
            (
                silverfish_slot::BODY2,
                [0.0, 20.0, 1.0],
                [-3.0, 0.0, -1.5],
                [6.0, 4.0, 3.0],
                [0.0, 9.0],
            ),
            (
                silverfish_slot::BODY3,
                [0.0, 21.0, 4.0],
                [-1.5, 0.0, -1.5],
                [3.0, 3.0, 3.0],
                [0.0, 16.0],
            ),
            (
                silverfish_slot::BODY4,
                [0.0, 22.0, 7.0],
                [-1.0, 0.0, -1.5],
                [2.0, 2.0, 3.0],
                [0.0, 22.0],
            ),
            (
                silverfish_slot::BODY5,
                [0.0, 23.0, 9.5],
                [-1.0, 0.0, -1.0],
                [2.0, 1.0, 2.0],
                [11.0, 0.0],
            ),
            (
                silverfish_slot::BODY6,
                [0.0, 23.0, 11.5],
                [-0.5, 0.0, -1.0],
                [1.0, 1.0, 2.0],
                [13.0, 4.0],
            ),
        ];
        for (slot, point, origin, size, uv) in bodies {
            let body = &MODEL_SILVERFISH.parts[slot];
            assert_eq!(body.point, point);
            assert_eq!(body.boxes, [b(origin, size, uv, 0.0, false)]);
        }
        assert_eq!(
            MODEL_SILVERFISH.parts[silverfish_slot::WING0].point,
            [0.0, 16.0, 1.0]
        );
        assert_eq!(
            MODEL_SILVERFISH.parts[silverfish_slot::WING0].boxes,
            [b(
                [-5.0, 0.0, -1.5],
                [10.0, 8.0, 3.0],
                [20.0, 0.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SILVERFISH.parts[silverfish_slot::WING1].point,
            [0.0, 20.0, 7.0]
        );
        assert_eq!(
            MODEL_SILVERFISH.parts[silverfish_slot::WING1].boxes,
            [b(
                [-3.0, 0.0, -1.5],
                [6.0, 4.0, 3.0],
                [20.0, 11.0],
                0.0,
                false
            )]
        );
        assert_eq!(
            MODEL_SILVERFISH.parts[silverfish_slot::WING2].point,
            [0.0, 19.0, -1.5]
        );
        assert_eq!(
            MODEL_SILVERFISH.parts[silverfish_slot::WING2].boxes,
            [b(
                [-3.0, 0.0, -1.5],
                [6.0, 5.0, 2.0],
                [20.0, 18.0],
                0.0,
                false
            )]
        );
    }

    #[test]
    fn the_endermite_is_four_segments() {
        assert_eq!(MODEL_ENDERMITE.parts.len(), 4);
        let segments = [
            (
                0,
                [0.0, 21.0, -3.5],
                [-2.0, 0.0, -1.0],
                [4.0, 3.0, 2.0],
                [0.0, 0.0],
            ),
            (
                1,
                [0.0, 20.0, 0.0],
                [-3.0, 0.0, -2.5],
                [6.0, 4.0, 5.0],
                [0.0, 5.0],
            ),
            (
                2,
                [0.0, 21.0, 3.0],
                [-1.5, 0.0, -0.5],
                [3.0, 3.0, 1.0],
                [0.0, 14.0],
            ),
            (
                3,
                [0.0, 22.0, 4.0],
                [-0.5, 0.0, -0.5],
                [1.0, 2.0, 1.0],
                [0.0, 18.0],
            ),
        ];
        for (slot, point, origin, size, uv) in segments {
            let segment = &MODEL_ENDERMITE.parts[slot];
            assert_eq!(segment.point, point);
            assert_eq!(segment.boxes, [b(origin, size, uv, 0.0, false)]);
        }
    }

    /// A pose at a full stride with the swing phase at zero, so the cosine terms land on
    /// their extremes, plus the named head angles.
    fn stride(head_yaw: f32, head_pitch: f32) -> Pose {
        Pose {
            limb_swing: 0.0,
            limb_swing_amount: 1.0,
            head_yaw,
            head_pitch,
            ..Pose::default()
        }
    }

    #[test]
    fn the_creeper_turns_its_head_and_swings_its_legs() {
        let mut rots = MODEL_CREEPER.rest();
        pose_creeper(&stride(90.0, 45.0), &mut rots);
        assert_eq!(
            rots[creeper_slot::HEAD].angles[1],
            degrees(90.0),
            "the head's yaw rides through"
        );
        assert_eq!(
            rots[creeper_slot::HEAD].angles[0],
            degrees(45.0),
            "and its pitch"
        );
        // The four legs: one and four in phase, two and three against them
        // (`ModelCreeper.setRotationAngles`:70-73).
        let same = (0.0_f32).cos() * 1.4 * 1.0;
        let opposite = std::f32::consts::PI.cos() * 1.4 * 1.0;
        assert_eq!(rots[creeper_slot::LEG1].angles[0], same);
        assert_eq!(rots[creeper_slot::LEG2].angles[0], opposite);
        assert_eq!(rots[creeper_slot::LEG3].angles[0], opposite);
        assert_eq!(rots[creeper_slot::LEG4].angles[0], same);
    }

    #[test]
    fn the_spider_fans_its_swing_across_the_eight_legs() {
        // Two strides whose phases pull the swing and the lift apart. At 1.0 the leg-1 lift
        // reads |sin(0.6662)| * 0.4 = 0.2472 where the swing's doubled phase would give
        // |sin(1.3324)| * 0.4 = 0.3887; at 2.4 the pair reads 0.3998 against 0.0225. The
        // swing runs `limbSwing * 0.6662F * 2.0F` and the lift `limbSwing * 0.6662F`
        // (`ModelSpider.setRotationAngles`:127-134), so a lift riding the doubled phase
        // fails here.
        for limb_swing in [1.0_f32, 2.4] {
            let mut rots = MODEL_SPIDER.rest();
            let before = MODEL_SPIDER.rest();
            let pose = Pose {
                limb_swing,
                limb_swing_amount: 1.0,
                head_yaw: 90.0,
                head_pitch: 45.0,
                ..Pose::default()
            };
            pose_spider(&pose, &mut rots);
            assert_eq!(rots[spider_slot::HEAD].angles[1], degrees(90.0));
            assert_eq!(rots[spider_slot::HEAD].angles[0], degrees(45.0));
            // The swing pair and the lift pair, in the source's own phase order
            // (`ModelSpider.setRotationAngles`:135-150).
            let swing_phase = limb_swing * 0.6662 * 2.0;
            let lift_phase = limb_swing * 0.6662;
            let swing = |offset: f32| -((swing_phase + offset).cos() * 0.4) * 1.0;
            let lift = |offset: f32| (lift_phase + offset).sin().abs() * 0.4 * 1.0;
            let half = std::f32::consts::FRAC_PI_2;
            let three_halves = std::f32::consts::PI * 3.0 / 2.0;
            let pairs = [
                (spider_slot::LEG1, swing(0.0), lift(0.0)),
                (spider_slot::LEG2, -swing(0.0), -lift(0.0)),
                (
                    spider_slot::LEG3,
                    swing(std::f32::consts::PI),
                    lift(std::f32::consts::PI),
                ),
                (
                    spider_slot::LEG4,
                    -swing(std::f32::consts::PI),
                    -lift(std::f32::consts::PI),
                ),
                (spider_slot::LEG5, swing(half), lift(half)),
                (spider_slot::LEG6, -swing(half), -lift(half)),
                (spider_slot::LEG7, swing(three_halves), lift(three_halves)),
                (spider_slot::LEG8, -swing(three_halves), -lift(three_halves)),
            ];
            for (slot, y, z) in pairs {
                assert_eq!(rots[slot].angles[1], before[slot].angles[1] + y);
                assert_eq!(rots[slot].angles[2], before[slot].angles[2] + z);
            }
        }
    }

    #[test]
    fn the_enderman_halves_clamps_and_gathers_its_limbs() {
        let mut rots = MODEL_ENDERMAN.rest();
        pose_enderman(&stride(0.0, 0.0), &mut rots);
        // The body hangs at the class's own −14 and the head one up from it
        // (`ModelEnderman.setRotationAngles`:48-51, `:114-115`).
        assert_eq!(rots[enderman_slot::BODY].point, [0.0, -14.0, 0.0]);
        assert_eq!(rots[enderman_slot::BODY].angles, [0.0, 0.0, 0.0]);
        assert_eq!(rots[enderman_slot::HEAD].point, [0.0, -13.0, 0.0]);
        assert_eq!(
            rots[enderman_slot::HEADWEAR].point,
            rots[enderman_slot::HEAD].point
        );
        assert_eq!(
            rots[enderman_slot::HEADWEAR].angles,
            rots[enderman_slot::HEAD].angles
        );
        // The limbs: the base swing halved, then clamped to the class's own ±0.4
        // (`ModelEnderman.setRotationAngles`:54-98).
        let base_leg = (0.0_f32 * 0.6662).cos() * 1.4 * 1.0;
        let right_leg = (base_leg * 0.5).clamp(-0.4, 0.4);
        assert_eq!(rots[enderman_slot::RIGHT_LEG].angles[0], right_leg);
        let left_leg = (std::f32::consts::PI.cos() * 1.4 * 1.0 * 0.5).clamp(-0.4, 0.4);
        assert_eq!(rots[enderman_slot::LEFT_LEG].angles[0], left_leg);
        let right_arm = ((std::f32::consts::PI.cos() * 2.0 * 1.0 * 0.5) * 0.5).clamp(-0.4, 0.4);
        assert_eq!(rots[enderman_slot::RIGHT_ARM].angles[0], right_arm);
        let left_arm = ((0.0_f32).cos() * 2.0 * 1.0 * 0.5 * 0.5).clamp(-0.4, 0.4);
        assert_eq!(rots[enderman_slot::LEFT_ARM].angles[0], left_arm);
        assert_eq!(rots[enderman_slot::RIGHT_LEG].point, [-2.0, -5.0, 0.0]);
        assert_eq!(rots[enderman_slot::LEFT_LEG].point, [2.0, -5.0, 0.0]);
        // The scream drops the head five more (`:123-127`).
        let mut attacking = stride(0.0, 0.0);
        attacking.extra = PoseExtra::Enderman { attacking: true };
        pose_enderman(&attacking, &mut rots);
        assert_eq!(rots[enderman_slot::HEAD].point, [0.0, -18.0, 0.0]);
    }

    #[test]
    fn the_chicken_bobs_its_head_and_folds_its_wings() {
        let mut rots = MODEL_CHICKEN.rest();
        pose_chicken(&stride(90.0, 45.0), &mut rots);
        // The head, and the bill and chin riding it (`ModelChicken.setRotationAngles`:93-98).
        assert_eq!(
            rots[chicken_slot::HEAD].angles,
            [degrees(45.0), degrees(90.0), 0.0]
        );
        assert_eq!(rots[chicken_slot::BILL].angles[0], degrees(45.0));
        assert_eq!(rots[chicken_slot::BILL].angles[1], degrees(90.0));
        assert_eq!(rots[chicken_slot::CHIN].angles[0], degrees(45.0));
        assert_eq!(rots[chicken_slot::CHIN].angles[1], degrees(90.0));
        // The body pitches over flat (`:99`).
        assert_eq!(
            rots[chicken_slot::BODY].angles[0],
            std::f32::consts::FRAC_PI_2
        );
        // The legs step in opposition (`:100-101`).
        assert_eq!(
            rots[chicken_slot::RIGHT_LEG].angles[0],
            (0.0_f32).cos() * 1.4 * 1.0
        );
        assert_eq!(
            rots[chicken_slot::LEFT_LEG].angles[0],
            std::f32::consts::PI.cos() * 1.4 * 1.0
        );
        // The wings ride the flap, pinned at the frame's own zero (`:102-103`).
        assert_eq!(rots[chicken_slot::RIGHT_WING].angles[2], 0.0);
        assert_eq!(rots[chicken_slot::LEFT_WING].angles[2], 0.0);
        let mut flapping = stride(0.0, 0.0);
        flapping.extra = PoseExtra::Chicken { flap: 0.6 };
        pose_chicken(&flapping, &mut rots);
        assert_eq!(rots[chicken_slot::RIGHT_WING].angles[2], 0.6);
        assert_eq!(rots[chicken_slot::LEFT_WING].angles[2], -0.6);
    }

    #[test]
    fn the_squid_turns_its_tentacles_with_the_angle() {
        let mut rots = MODEL_SQUID.rest();
        let tentacles = [
            squid_slot::TENTACLE0,
            squid_slot::TENTACLE1,
            squid_slot::TENTACLE2,
            squid_slot::TENTACLE3,
            squid_slot::TENTACLE4,
            squid_slot::TENTACLE5,
            squid_slot::TENTACLE6,
            squid_slot::TENTACLE7,
        ];
        pose_squid(&stride(0.0, 0.0), &mut rots);
        for slot in tentacles {
            assert_eq!(
                rots[slot].angles[0], 0.0,
                "no carried angle: the tentacles rest"
            );
        }
        let mut swimming = stride(0.0, 0.0);
        swimming.extra = PoseExtra::Squid {
            tentacle_angle: 0.6,
        };
        pose_squid(&swimming, &mut rots);
        for slot in tentacles {
            assert_eq!(rots[slot].angles[0], 0.6);
        }
    }

    #[test]
    fn the_magma_cube_squashes_its_segments() {
        let mut rots = MODEL_MAGMA_CUBE.rest();
        pose_magma_cube(&stride(0.0, 0.0), &mut rots);
        for slot in [
            magma_slot::SEGMENT0,
            magma_slot::SEGMENT4,
            magma_slot::SEGMENT7,
        ] {
            assert_eq!(rots[slot].point[1], 0.0, "no squash: the segments rest");
        }
        // Two squash values off the class's own rule (`ModelMagmaCube.setLivingAnimations`:42-56).
        let mut squeezed = stride(0.0, 0.0);
        squeezed.extra = PoseExtra::MagmaCube { squish: 0.5 };
        pose_magma_cube(&squeezed, &mut rots);
        assert_eq!(
            rots[magma_slot::SEGMENT0].point[1],
            -(4.0_f32 - 0.0) * 1.7 * 0.5
        );
        assert_eq!(
            rots[magma_slot::SEGMENT4].point[1],
            -(4.0_f32 - 4.0) * 1.7 * 0.5
        );
        assert_eq!(
            rots[magma_slot::SEGMENT7].point[1],
            -(4.0_f32 - 7.0) * 1.7 * 0.5
        );
        squeezed.extra = PoseExtra::MagmaCube { squish: 1.0 };
        pose_magma_cube(&squeezed, &mut rots);
        assert_eq!(
            rots[magma_slot::SEGMENT0].point[1],
            -(4.0_f32 - 0.0) * 1.7 * 1.0
        );
        assert_eq!(
            rots[magma_slot::SEGMENT7].point[1],
            -(4.0_f32 - 7.0) * 1.7 * 1.0
        );
    }

    #[test]
    fn the_cube_scale_is_the_renderers_squash_pair() {
        // At rest the squash term is zero, so the scale is the size alone
        // (`RenderSlime.preRenderCallback`:32, `RenderMagmaCube.preRenderCallback`:29).
        assert_eq!(cube_scale(1, 0.0), [1.0, 1.0, 1.0]);
        assert_eq!(cube_scale(3, 0.0), [3.0, 3.0, 3.0]);
        // Fully squashed: the pair widens the cube and flattens it.
        let f1 = 1.0 / (1.0 * 0.5 + 1.0);
        let f2 = 1.0 / (f1 + 1.0);
        assert_eq!(cube_scale(1, 1.0), [f2 * 1.0, (1.0 / f2) * 1.0, f2 * 1.0]);
        let f1 = 1.0 / (3.0 * 0.5 + 1.0);
        let f2 = 1.0 / (f1 + 1.0);
        assert_eq!(cube_scale(3, 1.0), [f2 * 3.0, (1.0 / f2) * 3.0, f2 * 3.0]);
    }

    #[test]
    fn the_bat_folds_hanging_and_beats_flying() {
        let mut rots = MODEL_BAT.rest();
        let mut hanging = stride(0.0, 0.0);
        hanging.extra = PoseExtra::Bat { hanging: true };
        pose_bat(&hanging, &mut rots);
        // The fold: the head upside down at −2, the wings swept back
        // (`ModelBat.setRotationAngles`:77-93).
        assert_eq!(
            rots[bat_slot::HEAD].angles,
            [0.0, std::f32::consts::PI, std::f32::consts::PI]
        );
        assert_eq!(rots[bat_slot::HEAD].point, [0.0, -2.0, 0.0]);
        assert_eq!(rots[bat_slot::BODY].angles[0], std::f32::consts::PI);
        assert_eq!(rots[bat_slot::RIGHT_WING].point, [-3.0, 0.0, 3.0]);
        assert_eq!(rots[bat_slot::LEFT_WING].point, [3.0, 0.0, 3.0]);
        assert_eq!(rots[bat_slot::RIGHT_WING].angles[0], -0.15707964);
        assert_eq!(rots[bat_slot::RIGHT_WING].angles[1], -1.2566371);
        assert_eq!(rots[bat_slot::LEFT_WING].angles[1], 1.2566371);
        assert_eq!(rots[bat_slot::OUTER_RIGHT_WING].angles[1], -1.7278761);
        assert_eq!(rots[bat_slot::OUTER_LEFT_WING].angles[1], 1.7278761);
        // The beat: the body and wings ride the age (`:103-108`).
        let mut flying = stride(0.0, 0.0);
        flying.extra = PoseExtra::Bat { hanging: false };
        pose_bat(&flying, &mut rots);
        assert_eq!(rots[bat_slot::HEAD].angles[1], degrees(0.0));
        assert_eq!(rots[bat_slot::HEAD].point, [0.0, 0.0, 0.0]);
        assert_eq!(rots[bat_slot::RIGHT_WING].point, [0.0, 0.0, 0.0]);
        let beat = std::f32::consts::FRAC_PI_4 + (0.0_f32 * 0.1).cos() * 0.15;
        assert_eq!(rots[bat_slot::BODY].angles[0], beat);
        let sweep = (0.0_f32 * 1.3).cos() * std::f32::consts::PI * 0.25;
        assert_eq!(rots[bat_slot::RIGHT_WING].angles[1], sweep);
        assert_eq!(
            rots[bat_slot::OUTER_RIGHT_WING].angles[1],
            rots[bat_slot::RIGHT_WING].angles[1] * 0.5
        );
        assert_eq!(
            rots[bat_slot::LEFT_WING].angles[1],
            -rots[bat_slot::RIGHT_WING].angles[1]
        );
    }

    #[test]
    fn the_slime_pose_leaves_the_rest() {
        let mut rots = MODEL_SLIME.rest();
        pose_slime(&stride(90.0, 45.0), &mut rots);
        assert_eq!(rots, MODEL_SLIME.rest(), "the slime model carries no pose");
    }

    #[test]
    fn the_silverfish_sways_segment_by_segment() {
        let mut rots = MODEL_SILVERFISH.rest();
        pose_silverfish(&stride(0.0, 0.0), &mut rots);
        // Segment zero: the full reach, its pivot still (`ModelSilverfish.setRotationAngles`:77-78).
        let reach = (0.0_f32 * 0.9 + 0.0 * 0.15 * std::f32::consts::PI).cos()
            * std::f32::consts::PI
            * 0.05
            * 3.0;
        assert_eq!(rots[silverfish_slot::BODY0].angles[1], reach);
        assert_eq!(rots[silverfish_slot::BODY0].point[0], 0.0);
        // Segment two: the still middle, no sway of the pivot at all.
        assert_eq!(rots[silverfish_slot::BODY2].point[0], 0.0);
        // The wings copy the second, third and fifth bodies (`:81-85`).
        assert_eq!(
            rots[silverfish_slot::WING0].angles[1],
            rots[silverfish_slot::BODY2].angles[1]
        );
        assert_eq!(
            rots[silverfish_slot::WING1].angles[1],
            rots[silverfish_slot::BODY4].angles[1]
        );
        assert_eq!(
            rots[silverfish_slot::WING1].point[0],
            rots[silverfish_slot::BODY4].point[0]
        );
        assert_eq!(
            rots[silverfish_slot::WING2].angles[1],
            rots[silverfish_slot::BODY1].angles[1]
        );
        assert_eq!(
            rots[silverfish_slot::WING2].point[0],
            rots[silverfish_slot::BODY1].point[0]
        );
    }

    #[test]
    fn the_endermite_sways_segment_by_segment() {
        let mut rots = MODEL_ENDERMITE.rest();
        pose_endermite(&stride(0.0, 0.0), &mut rots);
        // The same sway rule as the silverfish, a fifth of the reach
        // (`ModelEnderMite.setRotationAngles`:53-54).
        let reach = (0.0_f32 * 0.9 + 0.0 * 0.15 * std::f32::consts::PI).cos()
            * std::f32::consts::PI
            * 0.01
            * 3.0;
        assert_eq!(rots[0].angles[1], reach);
        assert_eq!(rots[0].point[0], 0.0);
        assert_eq!(
            rots[2].point[0], 0.0,
            "the middle segment's pivot does not sway"
        );
    }
}
