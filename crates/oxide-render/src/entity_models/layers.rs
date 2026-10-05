//! The layer framework: the extra geometry a model draws over itself.
//!
//! A layer is a model of its own — its boxes, its sheet and its pose — that a renderer draws
//! inside the same entity transform, after the base model, when the layer's condition holds
//! (`RenderLiving.renderModel` draws the model first and walks its layer list after it). The
//! framework's conditions are the renderer's own: a layer that draws only when a flag is set,
//! and a colour the layer's texels are multiplied by — either one colour for the whole sheet
//! or the draw's own colour byte indexing a 16-entry palette table.
//!
//! The palettes are the source's own. [`WOOL_COLOURS`] is `EntitySheep`'s static dye table
//! (`EntitySheep.java`:370-388), the table the wool layer (`LayerSheepWool.java`:35-42) and
//! the collar layer (`LayerWolfCollar.java`:26-27) both tint through. [`DYE_COLOURS`] is
//! `ItemDye.dyeColors` (`ItemDye.java`:21), the packed 0xRRGGBB palette the dye items carry;
//! no layer of this milestone reads it, and it is pinned here beside the wool table because
//! both are 16-entry colour tables indexed by `EnumDyeColor`'s metadata
//! (`EnumDyeColor.java`:9-24).
//!
//! The layers this milestone draws are the identity set the plan's decision lists: the
//! sheep's wool and the pig's saddle, and the crawler, cube and arthropod families' overlays
//! — the spider's and the enderman's eyes (`LayerSpiderEyes.java`:19-48,
//! `LayerEndermanEyes.java`:19-38: the body's own model on the eyes sheet, additive, pinned
//! to the source's constant full-bright lightmap) and the slime's gel
//! (`LayerSlimeGel.java`:19-32: the body's outer shell on the body's own sheet, alpha
//! blended). The creeper's charge aura (`LayerCreeperCharge.java`, registered at
//! `RenderCreeper.java`:17) defers with the render-type tinting; the magma cube draws no
//! layer. Three more identity layers are in the set but draw a
//! block through the block renderer — the snow golem's jack-o-lantern
//! (`LayerSnowmanHead.java`:25-30 draws `Blocks.pumpkin` through the item renderer), the
//! iron golem's rose (`LayerIronGolemFlower.java`:24-43 draws `Blocks.red_flower`), and the
//! mooshroom's mushrooms (`LayerMooshroomMushroom.java`:25-51 draws `Blocks.red_mushroom`)
//! — and the baked block models they re-use arrive with the object pass's block-item path,
//! so those three layers defer with it and are recorded here and in Task 9's report.

use super::{Model, Pose, Rot};
use crate::entity_pass::{DrawExtra, ModelRef};

/// The wool table: `EntitySheep`'s dye colours as floats, indexed by `EnumDyeColor`'s
/// metadata — white, orange, magenta, light blue, yellow, lime, pink, gray, silver, cyan,
/// purple, blue, brown, green, red, black (`EntitySheep.java`:372-387).
pub static WOOL_COLOURS: [[f32; 3]; 16] = [
    [1.0, 1.0, 1.0],
    [0.85, 0.5, 0.2],
    [0.7, 0.3, 0.85],
    [0.4, 0.6, 0.85],
    [0.9, 0.9, 0.2],
    [0.5, 0.8, 0.1],
    [0.95, 0.5, 0.65],
    [0.3, 0.3, 0.3],
    [0.6, 0.6, 0.6],
    [0.3, 0.5, 0.6],
    [0.5, 0.25, 0.7],
    [0.2, 0.3, 0.7],
    [0.4, 0.3, 0.2],
    [0.4, 0.5, 0.2],
    [0.6, 0.2, 0.2],
    [0.1, 0.1, 0.1],
];

/// The dye items' packed 0xRRGGBB palette, in `EnumDyeColor`'s metadata order
/// (`ItemDye.java`:21).
pub static DYE_COLOURS: [u32; 16] = [
    1973019, 11743532, 3887386, 5320730, 2437522, 8073150, 2651799, 11250603, 4408131, 14188952,
    4312372, 14602026, 6719955, 12801229, 15435844, 15790320,
];

/// The colour a layer multiplies its sheet's texels by.
#[derive(Debug, Clone, Copy)]
pub enum Tint {
    /// The sheet's own colours, untouched.
    Sheet,
    /// One colour over the whole layer, the way `GlStateManager.color` sets a flat colour
    /// before a layer draws (`LayerWolfCollar.java`:27).
    Flat([f32; 3]),
    /// A colour read from a 16-entry table by a byte the draw's own extras carry — the wool
    /// layer's fleece colour (`LayerSheepWool.java`:41-42).
    Palette {
        /// The table, indexed by the palette byte's low nibble.
        table: &'static [[f32; 3]; 16],
        /// The byte the draw carries, read from its extras.
        index: fn(&DrawExtra) -> u8,
    },
}

impl Tint {
    /// The colour this tint resolves to for a draw.
    pub fn rgb(&self, extra: &DrawExtra) -> [f32; 3] {
        match self {
            Tint::Sheet => [1.0, 1.0, 1.0],
            Tint::Flat(colour) => *colour,
            Tint::Palette { table, index } => table[usize::from(index(extra) & 0x0f)],
        }
    }
}

/// The blend a layer's colours land with: the source's own `blendFunc` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// The source's blend-disabled state: the wool and the saddle draw with no `blendFunc`
    /// at all (`LayerSheepWool.java`:35-42, `LayerSaddle.java`:16-20).
    Opaque,
    /// Straight alpha — the source over one minus its own alpha (`blendFunc(770, 771)`, the
    /// gel's pair, `LayerSlimeGel.java`:26).
    Alpha,
    /// Additive — the source's colours added onto the frame (`blendFunc(1, 1)`, the eyes
    /// overlays' pair, `LayerSpiderEyes.java`:24, `LayerEndermanEyes.java`:24).
    Additive,
}

/// One layer of a model class: the geometry it draws, the sheet it samples and its rules.
#[derive(Debug, Clone, Copy)]
pub struct Layer {
    /// The layer's geometry, its own model.
    pub model: &'static Model,
    /// The sheet the layer's texels come from, by the pass's registry key.
    pub texture: &'static str,
    /// The sheet's size in texels, which the layer's uvs divide through.
    pub texture_size: [f32; 2],
    /// Whether the layer draws for a draw's extras.
    pub active: fn(&DrawExtra) -> bool,
    /// The colour the layer's texels are multiplied by.
    pub tint: Tint,
    /// The pose the layer's geometry takes — the frames of its own model, in that model's
    /// part order.
    pub pose: fn(&Pose, &mut [Rot]),
    /// Whether the layer draws at full brightness, ignoring the entity's own light: the eyes
    /// overlays pin the lightmap to the source's constant pair of coordinates before they
    /// draw (`LayerSpiderEyes.java`:35-38, `LayerEndermanEyes.java`:27-30).
    pub full_bright: bool,
    /// The blend the layer's colours land with.
    pub blend: Blend,
}

/// One layer's resolved draw: its geometry with the frame's transforms, its sheet and the
/// colour its texels are multiplied by.
#[derive(Debug, Clone)]
pub struct LayerDraw {
    /// The layer's geometry.
    pub model: &'static Model,
    /// The transforms, one per part of `model`, in its own part order.
    pub transforms: Vec<Rot>,
    /// The sheet's registry key.
    pub texture: &'static str,
    /// The sheet's size in texels.
    pub texture_size: [f32; 2],
    /// The colour the texels are multiplied by.
    pub tint: [f32; 3],
    /// Whether the layer draws at full brightness.
    pub full_bright: bool,
    /// The blend the layer's colours land with.
    pub blend: Blend,
}

/// The wool byte a sheep draw carries.
pub fn wool_index(extra: &DrawExtra) -> u8 {
    match extra {
        DrawExtra::Sheep { wool, .. } => *wool,
        _ => 0,
    }
}

/// The sheep's wool layer (`LayerSheepWool.java`:21-42): `ModelSheep1` on the fur sheet,
/// tinted by the draw's fleece colour.
///
/// The source's gate is `!getSheared() && !isInvisible()`. An invisible draw never reaches
/// the pass — the client skips invisible entities before the draw list is built — so the
/// layer's condition is the shears alone. The layer draws its own sheet when the entity is
/// hurt (`shouldCombineTextures` true), and the wool's part in the hurt overlay is not
/// re-drawn: the overlay pass re-draws the base model only.
static WOOL_LAYER: Layer = Layer {
    model: &super::quadrupeds::MODEL_SHEEP_WOOL,
    texture: "entity/sheep/sheep_fur.png",
    texture_size: [64.0, 32.0],
    active: |extra| matches!(extra, DrawExtra::Sheep { sheared: false, .. }),
    tint: Tint::Palette {
        table: &WOOL_COLOURS,
        index: wool_index,
    },
    pose: super::quadrupeds::pose_sheep,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The sheep's layer table.
static SHEEP_LAYERS: [Layer; 1] = [WOOL_LAYER];

/// The pig's saddle layer (`LayerSaddle.java`:11-22): `ModelPig(0.5F)` on the saddle sheet,
/// untinted.
static SADDLE_LAYER: Layer = Layer {
    model: &super::quadrupeds::MODEL_PIG_SADDLE,
    texture: "entity/pig/pig_saddle.png",
    texture_size: [64.0, 32.0],
    active: |extra| matches!(extra, DrawExtra::Pig { saddle: true }),
    tint: Tint::Sheet,
    pose: super::quadrupeds::pose_pig,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The pig's layer table.
static PIG_LAYERS: [Layer; 1] = [SADDLE_LAYER];

/// The spider's eyes layer (`LayerSpiderEyes.java`:19-48): the spider's own model on the
/// eyes sheet — additive, at the source's constant full-bright lightmap, depth written (the
/// layer's invisible branch never fires: an invisible draw never reaches the pass). The cave
/// spider inherits it through `RenderSpider`'s constructor
/// (`RenderSpider.java`:15, reached by `RenderCaveSpider.java`:13).
static SPIDER_EYES_LAYER: Layer = Layer {
    model: &super::crawlers::MODEL_SPIDER,
    texture: "entity/spider_eyes.png",
    texture_size: [64.0, 32.0],
    active: |_| true,
    tint: Tint::Sheet,
    pose: super::crawlers::pose_spider,
    full_bright: true,
    blend: Blend::Additive,
};

/// The spider's layer table.
static SPIDER_LAYERS: [Layer; 1] = [SPIDER_EYES_LAYER];

/// The enderman's eyes layer (`LayerEndermanEyes.java`:19-38): the enderman's own model on
/// the eyes sheet — additive and full bright, as the spider's.
static ENDERMAN_EYES_LAYER: Layer = Layer {
    model: &super::crawlers::MODEL_ENDERMAN,
    texture: "entity/enderman/enderman_eyes.png",
    texture_size: [64.0, 32.0],
    active: |_| true,
    tint: Tint::Sheet,
    pose: super::crawlers::pose_enderman,
    full_bright: true,
    blend: Blend::Additive,
};

/// The enderman's layer table.
static ENDERMAN_LAYERS: [Layer; 1] = [ENDERMAN_EYES_LAYER];

/// The slime's gel layer (`LayerSlimeGel.java`:19-32): the body's outer shell over the inner
/// body, on the body's own sheet, alpha blended. The layer draws the sheet the base model
/// already bound — it binds none of its own — so it names the slime's own key
/// (`RenderSlime.java`:11), and it writes no frames: the shell's one part rests.
static SLIME_GEL_LAYER: Layer = Layer {
    model: &super::crawlers::MODEL_SLIME_GEL,
    texture: "entity/slime/slime.png",
    texture_size: [64.0, 32.0],
    active: |_| true,
    tint: Tint::Sheet,
    pose: super::crawlers::pose_slime,
    full_bright: false,
    blend: Blend::Alpha,
};

/// The slime's layer table.
static SLIME_LAYERS: [Layer; 1] = [SLIME_GEL_LAYER];

/// The wool-table index a wolf collar draw's byte decodes to: the byte is the dye's
/// damage value, which counts down against the table's metadata — the source's own
/// double fold, `byMetadata(byDyeDamage(byte & 15).getMetadata())`
/// (`LayerWolfCollar.java`:25, `EntityWolf.getCollarColor`:540-543). A fresh wolf's
/// byte is `14`, which reads the table's orange (the source's own default, MC-71674).
pub fn wolf_collar_index(extra: &DrawExtra) -> u8 {
    match extra {
        DrawExtra::Wolf { collar, .. } => 15 - (collar & 0x0f),
        _ => 0,
    }
}

/// The wolf's collar layer (`LayerWolfCollar.java`:20-30): the wolf's own model on the
/// collar sheet, tinted by the dye table through the collar byte.
static WOLF_COLLAR_LAYER: Layer = Layer {
    model: &super::exotics::MODEL_WOLF,
    texture: "entity/wolf/wolf_collar.png",
    texture_size: [64.0, 32.0],
    active: |extra| matches!(extra, DrawExtra::Wolf { tamed: true, .. }),
    tint: Tint::Palette {
        table: &WOOL_COLOURS,
        index: wolf_collar_index,
    },
    pose: super::exotics::pose_wolf,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The wolf's layer table.
static WOLF_LAYERS: [Layer; 1] = [WOLF_COLLAR_LAYER];

/// The horse's first marking layer, the class's own marking cells redrawn over the
/// horse. The source folds the stack into one layered texture
/// (`RenderHorse.getEntityTexture`:76-77, `EntityHorse.getVariantTexturePaths`:774-782);
/// the port draws the same stack as whole-model passes in the class's order.
static HORSE_MARKING_WHITE: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/horse_markings_white.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { markings: 1, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's second marking layer (`horse_markings_whitefield`, table index two).
static HORSE_MARKING_WHITE_FIELD: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/horse_markings_whitefield.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { markings: 2, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's third marking layer (`horse_markings_whitedots`, table index three).
static HORSE_MARKING_WHITE_DOTS: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/horse_markings_whitedots.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { markings: 3, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's fourth marking layer (`horse_markings_blackdots`, table index four).
static HORSE_MARKING_BLACK_DOTS: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/horse_markings_blackdots.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { markings: 4, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's iron armour layer: the class's own armour table's first file
/// (`EntityHorse.java`:53), the third layer of the source's stack. There are no armour
/// boxes anywhere in the client: the armour is this sheet alone (`EntityHorse.java`:52).
static HORSE_ARMOUR_IRON: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/armor/horse_armor_iron.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { armour: 1, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's golden armour layer (`horse_armor_gold`, table index two).
static HORSE_ARMOUR_GOLD: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/armor/horse_armor_gold.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { armour: 2, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's diamond armour layer (`horse_armor_diamond`, table index three).
static HORSE_ARMOUR_DIAMOND: Layer = Layer {
    model: &super::exotics::MODEL_HORSE,
    texture: "entity/horse/armor/horse_armor_diamond.png",
    texture_size: [128.0, 128.0],
    active: |extra| matches!(extra, DrawExtra::Horse { armour: 3, .. }),
    tint: Tint::Sheet,
    pose: super::exotics::pose_horse,
    full_bright: false,
    blend: Blend::Opaque,
};

/// The horse's layer table: the four markings first, then the three armours, the
/// source's own stack order (`EntityHorse.getVariantTexturePaths`:774-782).
static HORSE_LAYERS: [Layer; 7] = [
    HORSE_MARKING_WHITE,
    HORSE_MARKING_WHITE_FIELD,
    HORSE_MARKING_WHITE_DOTS,
    HORSE_MARKING_BLACK_DOTS,
    HORSE_ARMOUR_IRON,
    HORSE_ARMOUR_GOLD,
    HORSE_ARMOUR_DIAMOND,
];

/// The layers a model draws, in the source's order after its base model.
pub fn layers_for(model: ModelRef) -> &'static [Layer] {
    match model {
        ModelRef::Sheep { .. } => &SHEEP_LAYERS,
        ModelRef::Pig { .. } => &PIG_LAYERS,
        ModelRef::Spider | ModelRef::CaveSpider => &SPIDER_LAYERS,
        ModelRef::Enderman => &ENDERMAN_LAYERS,
        ModelRef::Slime { .. } => &SLIME_LAYERS,
        ModelRef::Wolf { .. } => &WOLF_LAYERS,
        ModelRef::Horse { .. } => &HORSE_LAYERS,
        // The creeper's charge aura (`RenderCreeper.java`:17) defers; the magma cube's
        // renderer registers no layer at all. The wither's invulnerable sheet and the
        // ghast's shooting sheet are their renderers' base-sheet selections, not layers
        // (`RenderWither.getEntityTexture`:33-37, `RenderGhast.getEntityTexture`:21-24).
        _ => &[],
    }
}

/// Resolves the layers a draw's model draws this frame: every active layer of the model, in
/// order, with its transforms built over the layer's own rest table.
pub fn draw_layers(model: ModelRef, extra: &DrawExtra, pose: &Pose) -> Vec<LayerDraw> {
    resolve(layers_for(model), extra, pose)
}

/// Resolves one layer table for a draw: the active layers' geometry in the table's order.
fn resolve(layers: &[Layer], extra: &DrawExtra, pose: &Pose) -> Vec<LayerDraw> {
    layers
        .iter()
        .filter(|layer| (layer.active)(extra))
        .map(|layer| {
            let mut transforms = layer.model.rest();
            (layer.pose)(pose, &mut transforms);
            LayerDraw {
                model: layer.model,
                transforms,
                texture: layer.texture,
                texture_size: layer.texture_size,
                tint: layer.tint.rgb(extra),
                full_bright: layer.full_bright,
                blend: layer.blend,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_models::{Box, Part};
    use crate::entity_pass::DrawExtra;
    use std::f32::consts::PI;

    /// A one-box model, for the ordering case's synthetic layers.
    static DOT_BOXES: [Box; 1] = [Box {
        origin: [0.0, 0.0, 0.0],
        size: [2.0, 2.0, 2.0],
        uv: [0.0, 0.0],
        inflate: 0.0,
        mirror: false,
    }];
    static DOT_PARTS: [Part; 1] = [Part {
        point: [0.0, 0.0, 0.0],
        rest: [0.0, 0.0, 0.0],
        boxes: &DOT_BOXES,
        children: &[],
    }];
    static DOT: Model = Model { parts: &DOT_PARTS };

    /// A pose function that writes nothing: the ordering case reads the transforms it seeds.
    fn still(_pose: &Pose, _out: &mut [Rot]) {}

    /// A pose function that turns the part a quarter about x.
    fn quarter(pose: &Pose, out: &mut [Rot]) {
        for rot in out.iter_mut() {
            rot.angles = [pose.head_pitch.to_radians(), 0.0, PI / 2.0];
        }
    }

    /// The two synthetic layers the ordering case drives: the first always active, the second
    /// only when the draw carries a saddle. The source's own ordering is the table's.
    static FIRST: Layer = Layer {
        model: &DOT,
        texture: "entity/first.png",
        texture_size: [16.0, 16.0],
        active: |_| true,
        tint: Tint::Sheet,
        pose: still,
        full_bright: false,
        blend: Blend::Opaque,
    };
    static SECOND: Layer = Layer {
        model: &DOT,
        texture: "entity/second.png",
        texture_size: [32.0, 32.0],
        active: |extra| matches!(extra, DrawExtra::Pig { saddle: true }),
        tint: Tint::Flat([0.25, 0.5, 0.75]),
        pose: quarter,
        full_bright: false,
        blend: Blend::Opaque,
    };
    static PAIR: [Layer; 2] = [FIRST, SECOND];

    /// The wool byte a draw carries.
    fn wool_index(extra: &DrawExtra) -> u8 {
        match extra {
            DrawExtra::Sheep { wool, .. } => *wool,
            _ => 0,
        }
    }

    #[test]
    fn the_wool_palette_is_the_sources_dye_table() {
        // `EntitySheep`'s static block, in `EnumDyeColor`'s metadata order.
        let wool = [
            (0, [1.0, 1.0, 1.0]),
            (1, [0.85, 0.5, 0.2]),
            (2, [0.7, 0.3, 0.85]),
            (3, [0.4, 0.6, 0.85]),
            (4, [0.9, 0.9, 0.2]),
            (5, [0.5, 0.8, 0.1]),
            (6, [0.95, 0.5, 0.65]),
            (7, [0.3, 0.3, 0.3]),
            (8, [0.6, 0.6, 0.6]),
            (9, [0.3, 0.5, 0.6]),
            (10, [0.5, 0.25, 0.7]),
            (11, [0.2, 0.3, 0.7]),
            (12, [0.4, 0.3, 0.2]),
            (13, [0.4, 0.5, 0.2]),
            (14, [0.6, 0.2, 0.2]),
            (15, [0.1, 0.1, 0.1]),
        ];
        for (index, colour) in wool {
            assert_eq!(
                WOOL_COLOURS[index], colour,
                "the wool palette at metadata {index}"
            );
        }
    }

    #[test]
    fn the_dye_palette_is_the_sources_packed_table() {
        // `ItemDye.dyeColors`, the packed 0xRRGGBB values in metadata order.
        let dye: [u32; 16] = [
            1973019, 11743532, 3887386, 5320730, 2437522, 8073150, 2651799, 11250603, 4408131,
            14188952, 4312372, 14602026, 6719955, 12801229, 15435844, 15790320,
        ];
        assert_eq!(DYE_COLOURS, dye);
    }

    #[test]
    fn the_palette_tint_reads_the_draws_byte_masked_to_its_nibble() {
        let palette = Tint::Palette {
            table: &WOOL_COLOURS,
            index: wool_index,
        };
        let red = DrawExtra::Sheep {
            wool: 14,
            sheared: false,
        };
        let white = DrawExtra::Sheep {
            wool: 0,
            sheared: false,
        };
        // The draw's own byte indexes the table: red is `[0.6, 0.2, 0.2]`, white `[1, 1, 1]`.
        assert_eq!(palette.rgb(&red), [0.6, 0.2, 0.2]);
        assert_eq!(palette.rgb(&white), [1.0, 1.0, 1.0]);
        // A byte off the wire keeps its low nibble — the source's own fold, `getFleeceColor`'s
        // `& 15` into `EnumDyeColor.byMetadata` (`EntitySheep.java`:262): 200 reads silver.
        let hostile = DrawExtra::Sheep {
            wool: 200,
            sheared: false,
        };
        assert_eq!(
            palette.rgb(&hostile),
            WOOL_COLOURS[8],
            "a hostile byte keeps its low nibble"
        );
        // The other two tint rules: the sheet untouched, and a flat colour.
        assert_eq!(Tint::Sheet.rgb(&white), [1.0, 1.0, 1.0]);
        assert_eq!(Tint::Flat([0.25, 0.5, 0.75]).rgb(&white), [0.25, 0.5, 0.75]);
    }

    #[test]
    fn the_layers_resolve_in_the_tables_order_and_skip_the_inactive_ones() {
        let pose = Pose::default();
        // Without the saddle only the first layer draws: its own sheet and leave of tint.
        let bare = DrawExtra::None;
        let draws = resolve(&PAIR, &bare, &pose);
        assert_eq!(draws.len(), 1, "the inactive layer is skipped");
        assert_eq!(draws[0].texture, "entity/first.png");
        assert_eq!(draws[0].tint, [1.0, 1.0, 1.0]);
        assert_eq!(draws[0].transforms.len(), 1);

        // With it, both draw, in the table's order, the second's transforms its own pose's.
        let saddled = DrawExtra::Pig { saddle: true };
        let draws = resolve(&PAIR, &saddled, &pose);
        assert_eq!(draws.len(), 2, "both layers draw");
        assert_eq!(draws[0].texture, "entity/first.png");
        assert_eq!(draws[1].texture, "entity/second.png");
        assert_eq!(draws[1].texture_size, [32.0, 32.0]);
        assert_eq!(draws[1].tint, [0.25, 0.5, 0.75]);
        assert_eq!(draws[1].transforms[0].angles[2], PI / 2.0);
    }

    #[test]
    fn the_eyes_and_gel_layers_are_the_sources_own() {
        let pose = Pose::default();
        // The spider: the body's own model on the eyes sheet, additive and full bright
        // (`LayerSpiderEyes.java`:21-24, :35-38).
        let spider = draw_layers(ModelRef::Spider, &DrawExtra::None, &pose);
        assert_eq!(spider.len(), 1);
        assert_eq!(spider[0].texture, "entity/spider_eyes.png");
        assert_eq!(spider[0].blend, Blend::Additive);
        assert!(spider[0].full_bright);
        assert_eq!(spider[0].model.parts.len(), 11, "the body's own model");
        // The cave spider inherits the same layer through `RenderSpider`'s constructor.
        let cave = draw_layers(ModelRef::CaveSpider, &DrawExtra::None, &pose);
        assert_eq!(cave.len(), 1);
        assert_eq!(cave[0].texture, "entity/spider_eyes.png");
        assert_eq!(cave[0].blend, Blend::Additive);
        // The enderman's own eyes (`LayerEndermanEyes.java`:21-24, :27-30).
        let enderman = draw_layers(ModelRef::Enderman, &DrawExtra::None, &pose);
        assert_eq!(enderman.len(), 1);
        assert_eq!(enderman[0].texture, "entity/enderman/enderman_eyes.png");
        assert_eq!(enderman[0].blend, Blend::Additive);
        assert!(enderman[0].full_bright);
        // The slime's gel: the one-part outer shell on the body's sheet, plain alpha
        // (`LayerSlimeGel.java`:26).
        let slime = draw_layers(ModelRef::Slime { size: 1 }, &DrawExtra::None, &pose);
        assert_eq!(slime.len(), 1);
        assert_eq!(slime[0].texture, "entity/slime/slime.png");
        assert_eq!(slime[0].blend, Blend::Alpha);
        assert!(!slime[0].full_bright);
        assert_eq!(slime[0].model.parts.len(), 1, "the gel is one body");
        // The creeper's aura defers; the magma cube and the rest of the families register no
        // layer at all.
        for bare in [
            ModelRef::Creeper,
            ModelRef::MagmaCube { size: 1 },
            ModelRef::Chicken { child: false },
            ModelRef::Squid,
            ModelRef::Bat { hanging: false },
            ModelRef::Silverfish,
            ModelRef::EnderMite,
        ] {
            assert!(
                draw_layers(bare, &DrawExtra::None, &pose).is_empty(),
                "{bare:?} draws no layer"
            );
        }
    }
}
