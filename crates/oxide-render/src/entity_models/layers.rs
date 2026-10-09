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

use super::{Box, Model, Part, Pose, Rot};
use crate::entity_pass::{DrawExtra, EquipmentDraw, ModelRef};
use crate::gui_item::display_matrix;
use glam::{Mat4, Vec3};
use oxide_assets::model::Transform;

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

// ---- the held item and the armour (the renderers' layers over the M4 poses) ----
//
// The renderers of the biped family carry three more layers this task lands: the held item
// (`LayerHeldItem`), the four armour slots (`LayerArmorBase`/`LayerBipedArmor`) and the
// owner-Q2 extras (`LayerCreeperCharge`, `LayerWitherAura`, `LayerDeadmau5Head`). The
// held-item and armour layers read the entity's own equipment; the extras read a flag and a
// name. The resolution functions here are the layer shapes' own values; the pass builds the
// geometry and the draws.

/// The held-item layer's arm-end scale: `postRenderArm(0.0625F)` (`LayerHeldItem.java`:41).
pub const HELD_ITEM_ARM_SCALE: f32 = 0.0625;

/// The layer's own mount translate (`LayerHeldItem.java`:42).
pub const HELD_ITEM_MOUNT: [f32; 3] = [-0.0625, 0.4375, 0.0625];

/// The child branch's lift, turn and scale (`LayerHeldItem.java`:35-38).
pub const HELD_ITEM_CHILD_LIFT: [f32; 3] = [0.0, 0.625, 0.0];
/// The child branch's `rotate(-20, -1, 0, 0)`: twenty degrees about +x.
pub const HELD_ITEM_CHILD_TURN: f32 = 20.0;
/// The child branch's scale.
pub const HELD_ITEM_CHILD_SCALE: f32 = 0.5;

/// The block branch's translate, turns and scale (`LayerHeldItem.java`:54-58) — the cross
/// family's own pose.
pub const HELD_ITEM_BLOCK_TRANSLATE: [f32; 3] = [0.0, 0.1875, -0.3125];
/// The block branch's twenty degrees about +x.
pub const HELD_ITEM_BLOCK_TURN_X: f32 = 20.0;
/// The block branch's forty-five degrees about +y.
pub const HELD_ITEM_BLOCK_TURN_Y: f32 = 45.0;
/// The block branch's negated x/y scale magnitude.
pub const HELD_ITEM_BLOCK_SCALE: f32 = 0.375;

/// The sneak branch's lift (`LayerHeldItem.java`:63).
pub const HELD_ITEM_SNEAK_LIFT: [f32; 3] = [0.0, 0.203125, 0.0];

/// The item's own third-person chain terms: the one class scale (`ItemRenderer.java`:67's
/// 3D class, `RenderItem.preTransform`'s generated class), the render's own scale and
/// centre, the builtin class's turn and tail, and the mesh's 1/16 fold
/// (`RenderItem.renderItem`:139-168).
pub const HELD_ITEM_CLASS_SCALE: f32 = 2.0;
/// The render's own scale (`RenderItem.renderItem`:142).
pub const HELD_ITEM_RENDER_SCALE: f32 = 0.5;
/// The render's own centre translate.
pub const HELD_ITEM_RENDER_CENTRE: f32 = -0.5;
/// The builtin class's half-turn (`RenderItem.renderItem`:146).
pub const HELD_ITEM_BUILTIN_TURN: f32 = 180.0;
/// The builtin class's chest tail's offset (`TileEntityChestRenderer.renderTileEntityAt`
/// through the port's folded chest trio).
pub const HELD_ITEM_BUILTIN_OFFSET: f32 = 1.0;
/// The mesh's own 1/16 fold: the port's item meshes carry 1/16 model units.
pub const HELD_ITEM_MESH_SCALE: f32 = 1.0 / 16.0;

/// The kinds whose renderers carry the held-item layer, from the source's own sites:
/// `RenderBiped`'s three-argument constructor adds it (`:19`, reached by the skeleton and
/// the giant), the zombie's four-argument super does not and its own `:31` does, and
/// `RenderPlayer`:36, `RenderGiantZombie`:22, `RenderPigZombie`:16 and
/// `ArmorStandRenderer`:31 add their own. The villager's renderer carries none
/// (`RenderVillager`:21's only layer is the custom head) and the witch's is its own
/// `LayerHeldItemWitch` (`RenderWitch`:16). Of those sites the port's roster holds the
/// player, the zombie (its villager form included — the same renderer), the skeleton and
/// the giant; the pig zombie and the armor stand are kinds the port does not draw. The
/// skeleton's duplicate instance (the inherited three-argument layer and its own `:18`)
/// draws the same chain twice in the source; the port draws it once — the duplicate is
/// pixel-identical and recorded as the ruling here.
pub fn holds_items(model: ModelRef) -> bool {
    matches!(
        model,
        ModelRef::Player { .. }
            | ModelRef::Zombie
            | ModelRef::ZombieVillager
            | ModelRef::Skeleton
            | ModelRef::Giant
    )
}

/// The kinds whose renderers carry a biped armour layer: `RenderPlayer`:35, `RenderZombie`:40
/// (its villager form through `LayerVillagerArmor`:50), `RenderSkeleton`:19,
/// `RenderGiantZombie`:23 and `RenderPigZombie`:17 — the same biped roster the held item
/// walks.
pub fn wears_armour(model: ModelRef) -> bool {
    holds_items(model)
}

/// The right arm's end for a pose, in the entity's frame (blocks): the arm's pivot translate
/// at the source's `0.0625` scale and its rotations z, y then x
/// (`ModelRenderer.postRender`:245-289), the transform the held item mounts on
/// (`LayerHeldItem.java`:41).
pub fn arm_end(arm: &Rot) -> Mat4 {
    let mut matrix = Mat4::from_translation(Vec3::from_array(arm.point) * HELD_ITEM_ARM_SCALE);
    matrix *= Mat4::from_rotation_z(arm.angles[2]);
    matrix *= Mat4::from_rotation_y(arm.angles[1]);
    matrix *= Mat4::from_rotation_x(arm.angles[0]);
    matrix
}

/// The held-item layer's mount chain (`LayerHeldItem.doRenderLayer`:31-64), in the entity's
/// frame (blocks): the child branch, the arm's end, the mount translate and the block and
/// sneak branches, in the source's own call order.
pub fn held_item_mount(arm: &Rot, child: bool, block: bool, sneak: bool) -> Mat4 {
    let mut matrix = Mat4::IDENTITY;
    if child {
        matrix *= Mat4::from_translation(Vec3::from_array(HELD_ITEM_CHILD_LIFT));
        matrix *= Mat4::from_rotation_x(HELD_ITEM_CHILD_TURN.to_radians());
        matrix *= Mat4::from_scale(Vec3::splat(HELD_ITEM_CHILD_SCALE));
    }
    matrix *= arm_end(arm);
    matrix *= Mat4::from_translation(Vec3::from_array(HELD_ITEM_MOUNT));
    if block {
        matrix *= Mat4::from_translation(Vec3::from_array(HELD_ITEM_BLOCK_TRANSLATE));
        matrix *= Mat4::from_rotation_x(HELD_ITEM_BLOCK_TURN_X.to_radians());
        matrix *= Mat4::from_rotation_y(HELD_ITEM_BLOCK_TURN_Y.to_radians());
        matrix *= Mat4::from_scale(Vec3::new(
            -HELD_ITEM_BLOCK_SCALE,
            -HELD_ITEM_BLOCK_SCALE,
            HELD_ITEM_BLOCK_SCALE,
        ));
    }
    if sneak {
        matrix *= Mat4::from_translation(Vec3::from_array(HELD_ITEM_SNEAK_LIFT));
    }
    matrix
}

/// The item's own third-person tail under the layer: one class scale, the display
/// transform, the render's own scale and centre, the builtin class's turn and tail, and the
/// mesh scale (`ItemRenderer.renderItem`:57-82 through `RenderItem.renderItemModelTransform`
/// and `RenderItem.renderItem`:139-168).
pub fn held_item_tail(transform: Transform, builtin: bool) -> Mat4 {
    let mut matrix = Mat4::from_scale(Vec3::splat(HELD_ITEM_CLASS_SCALE));
    matrix *= display_matrix(transform);
    matrix *= Mat4::from_scale(Vec3::splat(HELD_ITEM_RENDER_SCALE));
    if builtin {
        matrix *= Mat4::from_rotation_y(HELD_ITEM_BUILTIN_TURN.to_radians());
    }
    matrix *= Mat4::from_translation(Vec3::splat(HELD_ITEM_RENDER_CENTRE));
    if builtin {
        matrix *= Mat4::from_translation(Vec3::new(
            0.0,
            HELD_ITEM_BUILTIN_OFFSET,
            HELD_ITEM_BUILTIN_OFFSET,
        ));
        matrix *= Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0));
    }
    matrix *= Mat4::from_scale(Vec3::splat(HELD_ITEM_MESH_SCALE));
    matrix
}

/// The armour material a piece's sheet and dye rules read: `ItemArmor.ArmorMaterial`'s five
/// constants in the source's registration order (`ItemArmor.java`:46-50).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmourMaterial {
    /// `ArmorMaterial.LEATHER`.
    Leather,
    /// `ArmorMaterial.CHAIN`.
    Chainmail,
    /// `ArmorMaterial.IRON`.
    Iron,
    /// `ArmorMaterial.GOLD`.
    Gold,
    /// `ArmorMaterial.DIAMOND`.
    Diamond,
}

impl ArmourMaterial {
    /// The material's own name — the sheet path's middle term
    /// (`ArmorMaterial.getName`, `ItemArmor.java`:60).
    pub fn name(self) -> &'static str {
        match self {
            ArmourMaterial::Leather => "leather",
            ArmourMaterial::Chainmail => "chainmail",
            ArmourMaterial::Iron => "iron",
            ArmourMaterial::Gold => "gold",
            ArmourMaterial::Diamond => "diamond",
        }
    }
}

/// The four armour slots the layer draws, in the source's own numbering
/// (`LayerArmorBase.doRenderLayer`:34-37 walks 4, 3, 2, 1 — head, chest, legs, feet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmourSlot {
    /// The feet slot (`EntityEquipmentSlot.LEGS`'s neighbour: 1).
    Feet,
    /// The legs slot (2) — the leggings flag's own.
    Legs,
    /// The chest slot (3).
    Chest,
    /// The head slot (4).
    Head,
}

impl ArmourSlot {
    /// The source's own slot number.
    pub fn number(self) -> u8 {
        match self {
            ArmourSlot::Feet => 1,
            ArmourSlot::Legs => 2,
            ArmourSlot::Chest => 3,
            ArmourSlot::Head => 4,
        }
    }

    /// The store's slot index in the five-slot equipment array (`EntityLivingBase`'s
    /// `equipment` layout: held 0, feet 1, legs 2, chest 3, head 4).
    pub fn store_index(self) -> usize {
        usize::from(self.number())
    }

    /// The four slots in the source's own draw order (`LayerArmorBase.doRenderLayer`:34-37).
    pub fn walk() -> [ArmourSlot; 4] {
        [
            ArmourSlot::Head,
            ArmourSlot::Chest,
            ArmourSlot::Legs,
            ArmourSlot::Feet,
        ]
    }
}

/// The armour a stack's item id names: the piece's material and slot, from the source's own
/// registrations (`Item.registerItems`'s five four-piece runs, ids 298..=317: helmet, chest,
/// legs, boots per material, leather, chainmail, iron, diamond, gold).
pub fn armour_piece(id: i16) -> Option<(ArmourMaterial, ArmourSlot)> {
    let index = usize::try_from(id).ok()?.checked_sub(298)?;
    if index >= 20 {
        return None;
    }
    let material = match index / 4 {
        0 => ArmourMaterial::Leather,
        1 => ArmourMaterial::Chainmail,
        2 => ArmourMaterial::Iron,
        3 => ArmourMaterial::Diamond,
        _ => ArmourMaterial::Gold,
    };
    let slot = match index % 4 {
        0 => ArmourSlot::Head,
        1 => ArmourSlot::Chest,
        2 => ArmourSlot::Legs,
        _ => ArmourSlot::Feet,
    };
    Some((material, slot))
}

/// The armour sheets, by material and layer: `textures/models/armor/<material>_layer_<n>.png`
/// (`getArmorResource`:141-153). The keys are the extraction tree's own paths below
/// `textures/`; the layer index is 0 for layer 1, 1 for layer 2.
pub static ARMOUR_SHEETS: [[&str; 2]; 5] = [
    [
        "models/armor/leather_layer_1.png",
        "models/armor/leather_layer_2.png",
    ],
    [
        "models/armor/chainmail_layer_1.png",
        "models/armor/chainmail_layer_2.png",
    ],
    [
        "models/armor/iron_layer_1.png",
        "models/armor/iron_layer_2.png",
    ],
    [
        "models/armor/gold_layer_1.png",
        "models/armor/gold_layer_2.png",
    ],
    [
        "models/armor/diamond_layer_1.png",
        "models/armor/diamond_layer_2.png",
    ],
];

/// The leather overlays (`getArmorResource`'s third argument, `:68`), by layer.
pub static ARMOUR_OVERLAYS: [&str; 2] = [
    "models/armor/leather_layer_1_overlay.png",
    "models/armor/leather_layer_2_overlay.png",
];

/// The sheet one slot binds: the leggings flag picks layer 2, everything else layer 1
/// (`LayerArmorBase.renderLayer`:56-57).
pub fn armour_sheet(material: ArmourMaterial, slot: ArmourSlot) -> &'static str {
    let layer = usize::from(slot_for_leggings(slot));
    let material = match material {
        ArmourMaterial::Leather => 0,
        ArmourMaterial::Chainmail => 1,
        ArmourMaterial::Iron => 2,
        ArmourMaterial::Gold => 3,
        ArmourMaterial::Diamond => 4,
    };
    ARMOUR_SHEETS[material][layer]
}

/// The leather overlay one slot binds (`LayerArmorBase.renderLayer`:68's fall-through bind).
pub fn armour_overlay(slot: ArmourSlot) -> &'static str {
    ARMOUR_OVERLAYS[usize::from(slot_for_leggings(slot))]
}

/// Whether a slot draws the leggings model (`LayerArmorBase.isSlotForLeggings`:96-99): the
/// legs slot alone.
pub fn slot_for_leggings(slot: ArmourSlot) -> bool {
    slot == ArmourSlot::Legs
}

/// The construction inflation of a slot's model (`LayerBipedArmor.initArmor`:13-17): the
/// leggings model is `ModelBiped(0.5F)`, the armour model `ModelBiped(1.0F)`.
pub fn armour_inflation(slot: ArmourSlot) -> f32 {
    if slot_for_leggings(slot) { 0.5 } else { 1.0 }
}

/// The parts a slot's model draws, over the seven-part biped order (head, body, right arm,
/// left arm, right leg, left leg, headwear): `LayerBipedArmor.setModelPartVisible`:20-47
/// seeds every part hidden (`setModelVisible`'s `setInvisible(false)`) and shows the slot's
/// own.
pub fn armour_visible(slot: ArmourSlot) -> [bool; 7] {
    match slot {
        ArmourSlot::Feet => [false, false, false, false, true, true, false],
        ArmourSlot::Legs => [false, true, false, false, true, true, false],
        ArmourSlot::Chest => [false, true, true, true, false, false, false],
        ArmourSlot::Head => [true, false, false, false, false, false, true],
    }
}

/// The armour model's part table for a kind: the thick biped table every armour model
/// carries — `ModelSkeleton(_, true)` and `ModelZombie(_, true)` pass the flag that skips
/// the thin-limb swap, so the armour keeps the four-wide limbs — with the zombie villager's
/// own one-box head (`ModelZombieVillager`'s `p_i1165_3_` branch, `:17-23`).
pub fn armour_model(model: ModelRef) -> &'static Model {
    match model {
        ModelRef::ZombieVillager => &super::bipeds::MODEL_ARMOUR_VILLAGER,
        _ => &super::bipeds::MODEL_ARMOUR_BIPED,
    }
}

/// The default leather colour (`ItemArmor.getColor`:150's literal, 0xA06540).
pub const LEATHER_DEFAULT_COLOUR: i32 = 10_511_680;

/// The dye a stack's armour resolves to (`ItemArmor.getColor`:135-157): leather answers the
/// NBT's `display.color` when present and the default otherwise; every other material
/// answers `-1`.
pub fn armour_colour(material: ArmourMaterial, colour: Option<i32>) -> i32 {
    match material {
        ArmourMaterial::Leather => colour.unwrap_or(LEATHER_DEFAULT_COLOUR),
        _ => -1,
    }
}

/// The tint an armour draw multiplies its sheet's texels by
/// (`LayerArmorBase.renderLayer`:59-75): the leather branch's three channel folds of the
/// dye int; every other material's flat white.
pub fn armour_tint(material: ArmourMaterial, colour: i32) -> [f32; 3] {
    match material {
        ArmourMaterial::Leather => [
            f32::from((colour >> 16 & 255) as u8) / 255.0,
            f32::from((colour >> 8 & 255) as u8) / 255.0,
            f32::from((colour & 255) as u8) / 255.0,
        ],
        _ => [1.0, 1.0, 1.0],
    }
}

/// The glint's sheet (`LayerArmorBase.java`:15's `ENCHANTED_ITEM_GLINT_RES`).
pub const GLINT_SHEET: &str = "misc/enchanted_item_glint.png";

/// The glint's colour (`LayerArmorBase.renderGlint`:115-116): the source's `0.76` fold over
/// `(0.5, 0.25, 0.8)`.
pub const GLINT_COLOUR: [f32; 3] = [0.5 * 0.76, 0.25 * 0.76, 0.8 * 0.76];

/// The glint's per-pass turn about z, in degrees (`renderGlint`:121's `30 - i * 60`).
pub const GLINT_TURNS: [f32; 2] = [30.0, -30.0];

/// The glint's texture-matrix scale (`renderGlint`:119-120's `0.33333334`).
pub const GLINT_SCALE: f32 = 0.333_333_34;

/// The glint's per-pass scroll step inside the `f * (0.001 + i * 0.003) * 20` term
/// (`renderGlint`:122).
pub const GLINT_SCROLL: [f32; 2] = [0.001, 0.004];

/// One glint pass's uv under the source's texture matrix (`renderGlint`:117-122):
/// `scale(1/3) . rotate(30 - i*60) . translate(0, f * (0.001 + i*0.003) * 20)` over the
/// model's own uv.
pub fn glint_uv(pass: usize, age: f32, uv: [f32; 2]) -> [f32; 2] {
    let scroll = age * GLINT_SCROLL[pass] * 20.0;
    let angle = GLINT_TURNS[pass].to_radians();
    let (sin, cos) = angle.sin_cos();
    let u = uv[0];
    let v = uv[1] + scroll;
    [
        (u * cos - v * sin) * GLINT_SCALE,
        (u * sin + v * cos) * GLINT_SCALE,
    ]
}

/// The creeper's charge aura (`LayerCreeperCharge.java`:18-56): the creeper's own model at
/// `ModelCreeper(2.0F)`'s inflation, on the armour sheet, flat half-grey and additive at
/// full brightness, the texture matrix's diagonal scroll.
pub static CREEPER_AURA: Aura = Aura {
    texture: "entity/creeper/creeper_armor.png",
    inflate: 2.0,
    tint: [0.5, 0.5, 0.5],
    blend: Blend::Additive,
    full_bright: true,
    offset: |pose| [pose.age * 0.01, pose.age * 0.01],
};

/// The wither's aura (`LayerWitherAura.java`:18-61): `ModelWither(0.5F)`'s inflation, its
/// own armour sheet, the same flat grey and additive blend, the cosine-wave scroll pair.
pub static WITHER_AURA: Aura = Aura {
    texture: "entity/wither/wither_armor.png",
    inflate: 0.5,
    tint: [0.5, 0.5, 0.5],
    blend: Blend::Additive,
    full_bright: true,
    offset: |pose| [(pose.age * 0.02).cos() * 3.0, pose.age * 0.01],
};

/// The aura a layer redraws a model with: the sheet, the model's construction inflation, the
/// source's blend and brightness, and the texture matrix's offset.
#[derive(Debug, Clone, Copy)]
pub struct Aura {
    /// The sheet's registry key.
    pub texture: &'static str,
    /// The model's construction inflation, added to every box of the entity's own table.
    pub inflate: f32,
    /// The colour the texels are multiplied by.
    pub tint: [f32; 3],
    /// The blend the colours land with.
    pub blend: Blend,
    /// Whether the layer draws at full brightness (the source disables lighting).
    pub full_bright: bool,
    /// The texture matrix's uv offset for a pose.
    pub offset: fn(&Pose) -> [f32; 2],
}

/// One aura's resolved draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuraDraw {
    /// The sheet's registry key.
    pub texture: &'static str,
    /// The model's construction inflation.
    pub inflate: f32,
    /// The colour the texels are multiplied by.
    pub tint: [f32; 3],
    /// The blend the colours land with.
    pub blend: Blend,
    /// Whether the layer draws at full brightness.
    pub full_bright: bool,
    /// The uv offset the geometry's texture coordinates take.
    pub uv_offset: [f32; 2],
}

/// The aura a draw wears, when its kind has one and the draw's own flag is set: the
/// creeper's charge (`EntityCreeper.getPowered`:213-216) or the wither's armour — its
/// health at or below half (`EntityWither.isArmored`:653-656).
pub fn draw_charge_aura(model: ModelRef, extra: &DrawExtra, pose: &Pose) -> Option<AuraDraw> {
    let aura = match model {
        ModelRef::Creeper => match extra {
            DrawExtra::Creeper { powered: true } => &CREEPER_AURA,
            _ => return None,
        },
        ModelRef::Wither { .. } => match extra {
            DrawExtra::Wither { armored: true } => &WITHER_AURA,
            _ => return None,
        },
        _ => return None,
    };
    Some(AuraDraw {
        texture: aura.texture,
        inflate: aura.inflate,
        tint: aura.tint,
        blend: aura.blend,
        full_bright: aura.full_bright,
        uv_offset: (aura.offset)(pose),
    })
}

/// The name the deadmau5 ears hang on (`LayerDeadmau5Head.java`:24's
/// `getName().equals("deadmau5")`).
pub const DEADMAU5_NAME: &str = "deadmau5";

/// The ears' side offset and drop (`LayerDeadmau5Head.doRenderLayer`:35-36).
pub const EAR_SIDE: f32 = 0.375;
/// The ears' drop below the head's own frame.
pub const EAR_DROP: f32 = 0.375;
/// The ears' four-thirds scale (`:38`'s `1.3333334`).
pub const EAR_SCALE: f32 = 1.333_333_4;

/// The ear's one box: `ModelPlayer`'s own deadmau5 head, uv (24,0), six by six by one, no
/// inflation (`ModelPlayer.java`:21-22).
pub const EAR_BOX: Box = Box {
    origin: [-3.0, -6.0, -1.0],
    size: [6.0, 6.0, 1.0],
    uv: [24.0, 0.0],
    inflate: 0.0,
    mirror: false,
};

/// The ear's box table.
static EAR_BOXES: [Box; 1] = [EAR_BOX];

/// The ear's one-part model.
static EAR_PARTS: [Part; 1] = [Part {
    point: [0.0, 0.0, 0.0],
    rest: [0.0, 0.0, 0.0],
    boxes: &EAR_BOXES,
    children: &[],
}];

/// The deadmau5 ear's model: one box, drawn twice, once per side.
pub static MODEL_DEADMAU5_EAR: Model = Model { parts: &EAR_PARTS };

/// Whether a draw wears the ears: the name matches, the skin is present and the draw is
/// visible (`LayerDeadmau5Head.java`:24 — the port's player draws always carry a skin and
/// an invisible draw never reaches the pass).
pub fn deadmau5_ears(name: Option<&str>) -> bool {
    name == Some(DEADMAU5_NAME)
}

/// One ear's chain in the entity's frame (`LayerDeadmau5Head.doRenderLayer`:30-40): the
/// head-turn pair about the draw's own head yaw and pitch, the side and drop translates,
/// the pair undone, and the four-thirds scale. `side` is -1 for the first ear and 1 for the
/// second (`i * 2 - 1`).
pub fn ear_chain(head_yaw: f32, head_pitch: f32, side: f32) -> Mat4 {
    Mat4::from_rotation_y(head_yaw.to_radians())
        * Mat4::from_rotation_x(head_pitch.to_radians())
        * Mat4::from_translation(Vec3::new(EAR_SIDE * side, 0.0, 0.0))
        * Mat4::from_translation(Vec3::new(0.0, -EAR_DROP, 0.0))
        * Mat4::from_rotation_x(-head_pitch.to_radians())
        * Mat4::from_rotation_y(-head_yaw.to_radians())
        * Mat4::from_scale(Vec3::splat(EAR_SCALE))
}

/// The ear's own pose: the head's angles over the zeroed pivot
/// (`ModelPlayer.renderDeadmau5Head`:105-110's `copyModelAngles` over the zeroed point).
pub fn ear_rot(pose: &Pose) -> Rot {
    Rot {
        point: [0.0, 0.0, 0.0],
        angles: [
            pose.head_pitch.to_radians(),
            pose.head_yaw.to_radians(),
            0.0,
        ],
        offset: [0.0; 3],
        visible: true,
    }
}

/// The armour models' own texture size: every armour model the layers build is `64` by
/// `32` — `ModelBiped(modelSize)`'s pair, `ModelSkeleton(_, true)`'s and
/// `ModelZombieVillager(_, _, true)`'s, and `ModelZombie(_, true)`'s, whose `thinArms`
/// branch also answers `32` (`ModelZombie.java`:9-13).
pub const ARMOUR_TEXTURE: [f32; 2] = [64.0, 32.0];

/// The layers one kind's renderer registers, in the source's own list order — the order
/// `renderLayers` walks (`RendererLivingEntity.java`:459-470 over each renderer's own
/// `addLayer` sites).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderLayer {
    /// The armour walk (`LayerArmorBase`, four slots).
    Armour,
    /// The held item (`LayerHeldItem`).
    HeldItem,
    /// The deadmau5 ears (`LayerDeadmau5Head`).
    Ears,
    /// The charge aura (`LayerCreeperCharge` / `LayerWitherAura`).
    Aura,
}

/// The kind's own layer list: `RenderPlayer`:35-38 registers the armour, the held item,
/// the arrows (deferred — no arrow model) and the ears in that order; the zombie family's
/// `RenderZombie`:31/:40 and `RenderGiantZombie`:22-23 register the held item before the
/// armour; the skeleton's `RenderSkeleton`:18-19 registers its own held item (the 3-arg
/// `super` already added one at `RenderBiped`:19 — the duplicate draws the same chain, so
/// the port walks one) then the armour; the creeper's `RenderCreeper`:17 and the wither's
/// `RenderWither`:18 register the aura alone. Villagers carry no layer of these
/// (`RenderVillager`:21's only layer is the custom head), the witch's held item is its own
/// (`LayerHeldItemWitch`, `RenderWitch`:16) and stays deferred.
pub fn layer_order(model: ModelRef) -> &'static [RenderLayer] {
    match model {
        ModelRef::Player { .. } => &[
            RenderLayer::Armour,
            RenderLayer::HeldItem,
            RenderLayer::Ears,
        ],
        ModelRef::Zombie | ModelRef::ZombieVillager | ModelRef::Skeleton | ModelRef::Giant => {
            &[RenderLayer::HeldItem, RenderLayer::Armour]
        }
        ModelRef::Creeper | ModelRef::Wither { .. } => &[RenderLayer::Aura],
        _ => &[],
    }
}

/// The held item one draw wears — the brief's `draw_held_item` shape: `None` when the
/// kind's renderer carries no layer ([`holds_items`]) or the slot is empty; else the
/// layer's mount, composed from the draw's own right arm under the pose — the swing, the
/// held-item pose and the sneak sway all ride the pose the mount reads — and the stack the
/// item path draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldItemDraw {
    /// The layer's mount chain, in the entity's frame (blocks).
    pub mount: Mat4,
    /// The stack the layer draws.
    pub stack: EquipmentDraw,
}

/// Resolves a draw's held item: the mount for its right arm and the stack's own fields.
pub fn draw_held_item(
    model: ModelRef,
    pose: &Pose,
    stack: Option<&EquipmentDraw>,
) -> Option<HeldItemDraw> {
    if !holds_items(model) {
        return None;
    }
    let stack = *stack?;
    let mut rots = super::model_for(model).rest();
    super::pose(model, pose, &mut rots);
    let arm = rots[2];
    Some(HeldItemDraw {
        mount: held_item_mount(&arm, pose.child, stack.cross, pose.sneak),
        stack,
    })
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
        // The magma cube and the rest of the families register no layer at all.
        for bare in [
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

    // ---- the held item, the armour and the owner-Q2 extras (Task 13) ----

    use oxide_assets::model::Transform;

    /// The sword's third-person display transform, exactly as its model JSON states it
    /// (`models/item/diamond_sword.json`'s `display.thirdperson`).
    fn sword() -> Transform {
        Transform {
            rotation: [0.0, 90.0, -35.0],
            translation: [0.0, 1.25, -3.5],
            scale: [0.85, 0.85, 0.85],
        }
    }

    /// One matrix's columns, each pinned against its expectation.
    fn assert_cols(got: Mat4, want: [f32; 16], what: &str) {
        let cols = got.to_cols_array();
        for (index, (a, b)) in cols.iter().zip(want.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "{what}: column element {index} is {a}, want {b} (all: {cols:?})"
            );
        }
    }

    #[test]
    fn the_held_item_mount_is_the_layers_own_chain() {
        // The zombie's right arm at rest, straight from the pose machinery: the raised
        // arm of `pose_zombie` and the swing block's pivot.
        let mut rots = super::super::bipeds::MODEL_ZOMBIE.rest();
        super::super::bipeds::pose_zombie(&Pose::default(), &mut rots);
        let arm = rots[2];
        assert_eq!(
            arm.point,
            [-5.0, 2.0, 0.0],
            "the swing block's pivot at rest"
        );
        assert!(
            (arm.angles[0] + PI / 2.0).abs() < 1e-6,
            "the raised arm's x: {}",
            arm.angles[0]
        );
        assert!(
            (arm.angles[1] + 0.1).abs() < 1e-6,
            "its y: {}",
            arm.angles[1]
        );
        assert!(
            (arm.angles[2] - 0.1).abs() < 1e-6,
            "the idle roll: {}",
            arm.angles[2]
        );

        // The mount: `postRenderArm(0.0625)`'s pivot translate and rotations z, y, x, the
        // layer's own mount translate — pinned by literal (`LayerHeldItem.java`:41-42).
        let mount = held_item_mount(&arm, false, false, false);
        assert_cols(
            mount,
            [
                0.990_033_3,
                0.099_334_67,
                0.099_833_42,
                0.0,
                0.099_334_67,
                0.009_966_71,
                -0.995_004_2,
                0.0,
                -0.099_833_42,
                0.995_004_2,
                0.0,
                0.0,
                -0.337_157_75,
                0.185_339_78,
                -0.441_553_91,
                1.0,
            ],
            "the zombie's sword mount",
        );

        // The child branch lifts, turns and halves (`LayerHeldItem.java`:35-38).
        let child = held_item_mount(&arm, true, false, false);
        assert_cols(
            child,
            [
                0.495_016_66,
                0.029_599_51,
                0.063_893_59,
                0.0,
                0.049_667_34,
                0.174_838_53,
                -0.465_794_65,
                0.0,
                -0.049_916_71,
                0.467_499_05,
                0.170_155_7,
                0.0,
                -0.168_578_88,
                0.787_591_34,
                -0.175_767_53,
                1.0,
            ],
            "the child branch",
        );

        // The block branch's translate, 20/45-degree turns and negated x/y scale
        // (`LayerHeldItem.java`:54-58).
        let block = held_item_mount(&arm, false, true, false);
        assert_cols(
            block,
            [
                -0.296_406_84,
                0.220_684_62,
                0.063_765_64,
                0.0,
                -0.022_200_03,
                -0.131_128_53,
                0.350_623_54,
                0.0,
                0.228_637_6,
                0.273_365_15,
                0.116_710_71,
                0.0,
                -0.287_335_2,
                -0.123_730_06,
                -0.628_117_43,
                1.0,
            ],
            "the block branch",
        );

        // The sneak branch adds its lift on top, in the arm's own local frame — the
        // source's `translate` post-multiplies after the mount and the block branch
        // (`LayerHeldItem.java`:61-64), so the lift rides the arm's rotations.
        let sneak = held_item_mount(&arm, false, false, true);
        assert_cols(
            sneak,
            [
                0.990_033_3,
                0.099_334_67,
                0.099_833_42,
                0.0,
                0.099_334_67,
                0.009_966_71,
                -0.995_004_2,
                0.0,
                -0.099_833_42,
                0.995_004_2,
                0.0,
                0.0,
                -0.316_980_4,
                0.187_364_27,
                -0.643_664_1,
                1.0,
            ],
            "the sneak branch",
        );
    }

    #[test]
    fn the_held_item_tail_is_the_items_third_person_chain() {
        // The item's own tail under the layer: one `S(2)` (`ItemRenderer.java`:67), the
        // third-person display transform, the render's `S(0.5)`/`T(-0.5)` and the mesh
        // scale — pinned by literal for the sword's own `display.thirdperson`.
        let tail = held_item_tail(sword(), false);
        assert_cols(
            tail,
            [
                0.0,
                -0.030_471_25,
                -0.043_517_45,
                0.0,
                0.0,
                0.043_517_45,
                -0.030_471_25,
                0.0,
                0.053_125,
                0.0,
                0.0,
                0.0,
                -0.425,
                0.051_880_37,
                0.154_409_6,
                1.0,
            ],
            "the sword's third-person tail",
        );

        // The full chain a zombie's sword hangs on.
        let mut rots = super::super::bipeds::MODEL_ZOMBIE.rest();
        super::super::bipeds::pose_zombie(&Pose::default(), &mut rots);
        let chain = held_item_mount(&rots[2], false, false, false) * tail;
        assert_cols(
            chain,
            [
                0.001_317_64,
                -0.043_603_74,
                0.030_319_02,
                0.0,
                0.007_364_84,
                -0.029_885_29,
                -0.043_300_05,
                0.0,
                0.052_595_52,
                0.005_277_15,
                0.005_303_65,
                0.0,
                -0.768_183_62,
                0.297_277_8,
                -0.535_604_3,
                1.0,
            ],
            "the zombie's sword chain",
        );

        // The builtin class's own turn and tail (`RenderItem.renderItem`:140-150, the
        // chest trio's folded terms).
        let builtin = held_item_tail(sword(), true);
        assert!(
            builtin != held_item_tail(sword(), false),
            "the builtin tail is its own"
        );
    }

    #[test]
    fn the_armour_pieces_are_the_registrations_own_ids() {
        // `Item.registerItems`'s five four-piece runs, ids 298..=317: helmet, chest, legs,
        // boots per material, leather through gold.
        assert_eq!(
            armour_piece(298),
            Some((ArmourMaterial::Leather, ArmourSlot::Head))
        );
        assert_eq!(
            armour_piece(299),
            Some((ArmourMaterial::Leather, ArmourSlot::Chest))
        );
        assert_eq!(
            armour_piece(300),
            Some((ArmourMaterial::Leather, ArmourSlot::Legs))
        );
        assert_eq!(
            armour_piece(301),
            Some((ArmourMaterial::Leather, ArmourSlot::Feet))
        );
        assert_eq!(
            armour_piece(306),
            Some((ArmourMaterial::Iron, ArmourSlot::Head))
        );
        assert_eq!(
            armour_piece(312),
            Some((ArmourMaterial::Diamond, ArmourSlot::Legs))
        );
        assert_eq!(
            armour_piece(317),
            Some((ArmourMaterial::Gold, ArmourSlot::Feet))
        );
        // The neighbours are no armour: the range is closed at both ends.
        assert_eq!(armour_piece(297), None);
        assert_eq!(armour_piece(318), None);
        assert_eq!(armour_piece(0), None);
    }

    #[test]
    fn the_armour_sheets_and_the_leggings_flag_are_the_sources() {
        // The leggings flag: the legs slot alone (`isSlotForLeggings`:96-99), and the
        // sheet name it picks (`getArmorResource`:141-153).
        assert!(slot_for_leggings(ArmourSlot::Legs));
        for slot in [ArmourSlot::Feet, ArmourSlot::Chest, ArmourSlot::Head] {
            assert!(
                !slot_for_leggings(slot),
                "{slot:?} is not the leggings slot"
            );
            assert_eq!(
                armour_sheet(ArmourMaterial::Iron, slot),
                "models/armor/iron_layer_1.png",
                "{slot:?} binds layer 1"
            );
        }
        assert_eq!(
            armour_sheet(ArmourMaterial::Leather, ArmourSlot::Legs),
            "models/armor/leather_layer_2.png",
            "the leggings bind layer 2"
        );
        // Every material's pair, and the leather overlays the fall-through draws.
        assert_eq!(
            armour_sheet(ArmourMaterial::Chainmail, ArmourSlot::Head),
            "models/armor/chainmail_layer_1.png"
        );
        assert_eq!(
            armour_sheet(ArmourMaterial::Gold, ArmourSlot::Legs),
            "models/armor/gold_layer_2.png"
        );
        assert_eq!(
            armour_sheet(ArmourMaterial::Diamond, ArmourSlot::Chest),
            "models/armor/diamond_layer_1.png"
        );
        assert_eq!(
            armour_overlay(ArmourSlot::Head),
            "models/armor/leather_layer_1_overlay.png"
        );
        assert_eq!(
            armour_overlay(ArmourSlot::Legs),
            "models/armor/leather_layer_2_overlay.png"
        );
    }

    #[test]
    fn the_armour_model_pick_and_visibility_are_the_sources() {
        // The construction inflation pair (`LayerBipedArmor.initArmor`:13-17): the leggings
        // model at 0.5, the armour model at 1.0.
        assert_eq!(armour_inflation(ArmourSlot::Legs), 0.5);
        for slot in [ArmourSlot::Feet, ArmourSlot::Chest, ArmourSlot::Head] {
            assert_eq!(
                armour_inflation(slot),
                1.0,
                "{slot:?} draws the armour model"
            );
        }
        // The per-slot visibility over the seven-part order: head, body, right arm, left
        // arm, right leg, left leg, headwear (`LayerBipedArmor.setModelPartVisible`:20-47
        // over `ModelBiped.setInvisible`:110-119's all-hidden seed).
        assert_eq!(
            armour_visible(ArmourSlot::Feet),
            [false, false, false, false, true, true, false]
        );
        assert_eq!(
            armour_visible(ArmourSlot::Legs),
            [false, true, false, false, true, true, false]
        );
        assert_eq!(
            armour_visible(ArmourSlot::Chest),
            [false, true, true, true, false, false, false]
        );
        assert_eq!(
            armour_visible(ArmourSlot::Head),
            [true, false, false, false, false, false, true]
        );
        // The model tables: the thick biped for every kind, the zombie villager's own head
        // (`ModelZombieVillager`'s `p_i1165_3_` branch, :17-23).
        assert_eq!(armour_model(ModelRef::Zombie).parts.len(), 7);
        assert_eq!(armour_model(ModelRef::Skeleton).parts.len(), 7);
        assert_eq!(
            armour_model(ModelRef::Skeleton).parts[2].boxes[0].size,
            [4.0, 12.0, 4.0],
            "the skeleton's armour keeps the thick arm (ModelSkeleton's thinArms skips the swap)"
        );
        assert_eq!(
            armour_model(ModelRef::ZombieVillager).parts[0].boxes[0].size,
            [8.0, 8.0, 8.0],
            "the villager's armour head is the one-box 8x8x8"
        );
    }

    #[test]
    fn the_dye_tint_composes_through_get_colors_fold() {
        // `getColor`: non-leather answers -1 whatever the tag says; leather the NBT's own
        // `display.color` when present and the default 10511680 otherwise
        // (`ItemArmor.getColor`:135-157).
        assert_eq!(armour_colour(ArmourMaterial::Iron, Some(0x00FF00)), -1);
        assert_eq!(armour_colour(ArmourMaterial::Leather, None), 10_511_680);
        assert_eq!(
            armour_colour(ArmourMaterial::Leather, Some(0x00FF00)),
            0x00FF00
        );
        // The tint's channel fold, pinned through the rule: a pure red dye reads
        // `[1, 0, 0]`, the default leather brown its 0xA06540 channels, and every
        // non-leather material the white branch.
        assert_eq!(
            armour_tint(ArmourMaterial::Leather, 0xFF0000),
            [1.0, 0.0, 0.0]
        );
        let brown = armour_tint(ArmourMaterial::Leather, 10_511_680);
        assert!((brown[0] - 160.0 / 255.0).abs() < 1e-6, "the red channel");
        assert!((brown[1] - 101.0 / 255.0).abs() < 1e-6, "the green channel");
        assert!((brown[2] - 64.0 / 255.0).abs() < 1e-6, "the blue channel");
        assert_eq!(
            armour_tint(ArmourMaterial::Diamond, -1),
            [1.0, 1.0, 1.0],
            "the non-leather branch leaves the sheet's colours"
        );
    }

    #[test]
    fn the_auras_are_the_sources_sheets_and_blends() {
        // The creeper's charge (`LayerCreeperCharge.java`:18-56): the creeper's own model at
        // `ModelCreeper(2.0F)`, the armour sheet, the flat half-grey, additive at full
        // brightness, and the texture matrix's diagonal scroll.
        let pose = Pose {
            age: 50.0,
            ..Pose::default()
        };
        let charged = DrawExtra::Creeper { powered: true };
        let aura = draw_charge_aura(ModelRef::Creeper, &charged, &pose).expect("the aura draws");
        assert_eq!(aura.texture, "entity/creeper/creeper_armor.png");
        assert_eq!(aura.inflate, 2.0);
        assert_eq!(aura.tint, [0.5, 0.5, 0.5]);
        assert_eq!(aura.blend, Blend::Additive);
        assert!(aura.full_bright);
        assert_eq!(aura.uv_offset, [0.5, 0.5], "f * 0.01 at fifty ticks");

        // The wither's (`LayerWitherAura.java`:18-61): `ModelWither(0.5F)`, its own sheet,
        // the same grey and blend, the cosine scroll pair.
        let armored = DrawExtra::Wither { armored: true };
        let aura = draw_charge_aura(ModelRef::Wither { invul_time: 0 }, &armored, &pose)
            .expect("the aura draws");
        assert_eq!(aura.texture, "entity/wither/wither_armor.png");
        assert_eq!(aura.inflate, 0.5);
        assert_eq!(aura.tint, [0.5, 0.5, 0.5]);
        assert_eq!(aura.blend, Blend::Additive);
        assert!(aura.full_bright);
        let wave = (50.0_f32 * 0.02).cos() * 3.0;
        assert!((aura.uv_offset[0] - wave).abs() < 1e-6, "the cosine wave");
        assert_eq!(aura.uv_offset[1], 0.5, "f * 0.01");

        // The gates: an uncharged creeper and an unarmoured wither draw none, and no other
        // kind carries an aura.
        assert!(
            draw_charge_aura(
                ModelRef::Creeper,
                &DrawExtra::Creeper { powered: false },
                &pose
            )
            .is_none()
        );
        assert!(
            draw_charge_aura(
                ModelRef::Wither { invul_time: 0 },
                &DrawExtra::Wither { armored: false },
                &pose
            )
            .is_none()
        );
        assert!(draw_charge_aura(ModelRef::Zombie, &DrawExtra::None, &pose).is_none());
    }

    #[test]
    fn the_deadmau5_ears_are_the_sources_boxes() {
        // The name rule: `getName().equals("deadmau5")` (`LayerDeadmau5Head.java`:24).
        assert!(deadmau5_ears(Some("deadmau5")));
        assert!(!deadmau5_ears(Some("deadmau5x")));
        assert!(!deadmau5_ears(Some("Deadmau5")));
        assert!(!deadmau5_ears(None));
        assert_eq!(DEADMAU5_NAME, "deadmau5");

        // The box: `ModelPlayer`'s own ear, uv (24,0), six by six by one, no inflation
        // (`ModelPlayer.java`:21-22).
        assert_eq!(MODEL_DEADMAU5_EAR.parts.len(), 1);
        let ear_box = MODEL_DEADMAU5_EAR.parts[0].boxes[0];
        assert_eq!(ear_box.origin, [-3.0, -6.0, -1.0]);
        assert_eq!(ear_box.size, [6.0, 6.0, 1.0]);
        assert_eq!(ear_box.uv, [24.0, 0.0]);
        assert_eq!(ear_box.inflate, 0.0);

        // The ear's own pose: the head's angles over the zeroed pivot
        // (`renderDeadmau5Head`:107-110's copy over the zeroed point).
        let pose = Pose {
            head_yaw: 30.0,
            head_pitch: 10.0,
            ..Pose::default()
        };
        let rot = ear_rot(&pose);
        assert_eq!(rot.point, [0.0, 0.0, 0.0]);
        assert!((rot.angles[0] - 10.0_f32.to_radians()).abs() < 1e-6);
        assert!((rot.angles[1] - 30.0_f32.to_radians()).abs() < 1e-6);

        // The chain: the head-turn pair, the side and drop translates, the pair undone and
        // the four-thirds scale (`LayerDeadmau5Head.doRenderLayer`:30-40) — at rest the
        // side translate alone, the left ear at -0.375 and the right at +0.375.
        let left = ear_chain(0.0, 0.0, -1.0);
        let right = ear_chain(0.0, 0.0, 1.0);
        assert!(
            (left.to_cols_array()[12] + 0.375).abs() < 1e-6,
            "the left side: {:?}",
            left.to_cols_array()
        );
        assert!((right.to_cols_array()[12] - 0.375).abs() < 1e-6);
        assert!((left.to_cols_array()[13] + 0.375).abs() < 1e-6, "the drop");
        assert!(
            (left.to_cols_array()[0] - 4.0 / 3.0).abs() < 1e-6,
            "the scale"
        );
    }

    #[test]
    fn the_held_item_and_armour_rosters_are_the_source_sites() {
        // The held-item sites: `RenderBiped`'s 3-arg constructor (`:19`), `RenderZombie`:31,
        // `RenderPlayer`:36, `RenderGiantZombie`:22, `RenderPigZombie`:16 and
        // `ArmorStandRenderer`:31 — the port's roster is the player, the zombie (and its
        // villager form), the skeleton and the giant. The villager carries none
        // (`RenderVillager`:21) and the witch's is its own (`RenderWitch`:16).
        for holding in [
            ModelRef::Player {
                slim: false,
                parts: 0,
            },
            ModelRef::Zombie,
            ModelRef::ZombieVillager,
            ModelRef::Skeleton,
            ModelRef::Giant,
        ] {
            assert!(holds_items(holding), "{holding:?} holds items");
        }
        for bare in [
            ModelRef::Villager {
                profession: 0,
                child: false,
            },
            ModelRef::Witch,
            ModelRef::Creeper,
            ModelRef::SnowGolem,
            ModelRef::IronGolem,
            ModelRef::Pig { saddle: false },
        ] {
            assert!(!holds_items(bare), "{bare:?} holds nothing");
        }

        // The armour sites: `RenderPlayer`:35, `RenderZombie`:40 (its villager form through
        // `LayerVillagerArmor`:50), `RenderSkeleton`:19, `RenderGiantZombie`:23,
        // `RenderPigZombie`:17.
        for armoured in [
            ModelRef::Player {
                slim: false,
                parts: 0,
            },
            ModelRef::Zombie,
            ModelRef::ZombieVillager,
            ModelRef::Skeleton,
            ModelRef::Giant,
        ] {
            assert!(wears_armour(armoured), "{armoured:?} wears armour");
        }
        for bare in [
            ModelRef::Villager {
                profession: 0,
                child: false,
            },
            ModelRef::Witch,
            ModelRef::Creeper,
            ModelRef::Enderman,
        ] {
            assert!(!wears_armour(bare), "{bare:?} wears none");
        }
    }

    #[test]
    fn the_armour_pose_is_the_kinds_own_class_terms() {
        // The armour model is the renderer's main model class at its own construction
        // inflation: the plain biped for the player (the base terms with the held-item
        // pose), the zombie family's raised arms and the skeleton's own.
        let pose = Pose {
            limb_swing: 1.0,
            limb_swing_amount: 1.0,
            age: 3.0,
            ..Pose::default()
        };
        let mut armour = super::super::bipeds::MODEL_ARMOUR_BIPED.rest();
        super::super::bipeds::armour_pose(
            ModelRef::Player {
                slim: false,
                parts: 0,
            },
            &pose,
            true,
            &mut armour,
        );
        let mut zombie = super::super::bipeds::MODEL_ARMOUR_BIPED.rest();
        super::super::bipeds::armour_pose(ModelRef::Zombie, &pose, false, &mut zombie);
        let mut plain = super::super::bipeds::MODEL_ARMOUR_BIPED.rest();
        super::super::bipeds::armour_pose(
            ModelRef::Player {
                slim: false,
                parts: 0,
            },
            &pose,
            false,
            &mut plain,
        );

        // The zombie's armour arms are the raised pair, outright: x at -pi/2 (plus the
        // swing's own terms at this swing progress), y at its own -0.1 plus the idle roll.
        assert!(
            (zombie[2].angles[0] - (-PI / 2.0)).abs() > 1e-3,
            "the zombie's raised arm x is its own"
        );
        assert!(
            (zombie[2].angles[1] + 0.1).abs() < 1e-6,
            "its y is the zombie's own -0.1: {}",
            zombie[2].angles[1]
        );
        // The held-item term: the right arm's walk sway halves and drops by a tenth of pi
        // before the swing block (`ModelBiped.setRotationAngles`:160-174) — the two runs
        // differ, and the held one sits lower.
        assert!(
            (armour[2].angles[0] - plain[2].angles[0]).abs() > 1e-3,
            "the held term moves the right arm: {} vs {}",
            armour[2].angles[0],
            plain[2].angles[0]
        );
    }

    #[test]
    fn the_layer_rosters_are_the_source_sites() {
        // The player's list order: armour, held item, ears (`RenderPlayer`:35-38).
        assert_eq!(
            layer_order(ModelRef::Player {
                slim: false,
                parts: 0
            }),
            &[
                RenderLayer::Armour,
                RenderLayer::HeldItem,
                RenderLayer::Ears
            ]
        );
        // The zombie family and the skeleton: held item first, then armour.
        for model in [
            ModelRef::Zombie,
            ModelRef::ZombieVillager,
            ModelRef::Skeleton,
            ModelRef::Giant,
        ] {
            assert_eq!(
                layer_order(model),
                &[RenderLayer::HeldItem, RenderLayer::Armour],
                "{model:?}"
            );
        }
        // The auras alone.
        assert_eq!(layer_order(ModelRef::Creeper), &[RenderLayer::Aura]);
        assert_eq!(
            layer_order(ModelRef::Wither { invul_time: 0 }),
            &[RenderLayer::Aura]
        );
        // Villagers and the witch carry none of these.
        assert!(
            layer_order(ModelRef::Villager {
                child: false,
                profession: 0
            })
            .is_empty()
        );
        assert!(layer_order(ModelRef::Witch).is_empty());
        // And the resolver draws one only where the layer is registered.
        let stack = EquipmentDraw {
            id: 276,
            damage: 0,
            enchanted: false,
            colour: None,
            cross: false,
        };
        assert!(draw_held_item(ModelRef::Zombie, &Pose::default(), Some(&stack)).is_some());
        assert!(
            draw_held_item(
                ModelRef::Villager {
                    child: false,
                    profession: 0
                },
                &Pose::default(),
                Some(&stack)
            )
            .is_none()
        );
        assert!(draw_held_item(ModelRef::Zombie, &Pose::default(), None).is_none());
    }

    #[test]
    fn the_glint_scroll_and_uv_are_the_sources_matrix() {
        // The two passes' scroll terms, `f * (0.001 + i * 0.003) * 20` (`renderGlint`:122):
        // at age zero nothing moves, and the second pass scrolls four times the first.
        // At age zero the scroll term is zero, so the uv carries the pass's own
        // constant turn alone (`renderGlint`:121): 30 degrees about the sheet's origin.
        let turn = 30.0_f32.to_radians();
        let (sin, cos) = turn.sin_cos();
        let still = glint_uv(0, 0.0, [0.5, 0.5]);
        assert!(
            (still[0] - (0.5 * cos - 0.5 * sin) * GLINT_SCALE).abs() < 1e-6,
            "the first pass's still u: {}",
            still[0]
        );
        assert!((still[1] - (0.5 * sin + 0.5 * cos) * GLINT_SCALE).abs() < 1e-6);
        let first = glint_uv(0, 10.0, [0.0, 0.0]);
        let second = glint_uv(1, 10.0, [0.0, 0.0]);
        // Pass 0: v = 10 * 0.001 * 20 = 0.2, turned +30 and scaled by 1/3.
        let angle = 30.0_f32.to_radians();
        assert!(
            (first[0] - (-0.2 * angle.sin() * GLINT_SCALE)).abs() < 1e-5,
            "the first pass's u: {}",
            first[0]
        );
        assert!((first[1] - 0.2 * angle.cos() * GLINT_SCALE).abs() < 1e-5);
        // Pass 1: v = 10 * 0.004 * 20 = 0.8, turned -30.
        assert!(
            second[1].abs() > first[1].abs(),
            "the second pass scrolls further"
        );
        // The sheet and the colour are the source's (`LayerArmorBase.java`:15, :115-116).
        assert_eq!(GLINT_SHEET, "misc/enchanted_item_glint.png");
        assert!((GLINT_COLOUR[0] - 0.38).abs() < 1e-6);
        assert!((GLINT_COLOUR[1] - 0.19).abs() < 1e-6);
        assert!((GLINT_COLOUR[2] - 0.608).abs() < 1e-6);
        assert_eq!(GLINT_TURNS, [30.0, -30.0]);
        // The armour models' own texture size: 64 by 32 for every kind.
        assert_eq!(ARMOUR_TEXTURE, [64.0, 32.0]);
    }
}
