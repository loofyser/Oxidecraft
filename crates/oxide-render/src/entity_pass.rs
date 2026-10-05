//! The entity pass: the box models drawn between the terrain's solid and translucent layers.
//!
//! The pass draws the frame's [`EntityDraw`] list through one vertex format and three
//! pipelines. The model pipeline renders a box model's vertices — the per-kind poses turned
//! into world space on the CPU, the texture, the entity's own brightness in the vertex colour
//! and the client's two standard item lights shading each face from its normal
//! (`RenderHelper.enableStandardItemLighting`'s fixed-function pair, expressed per fragment).
//! The hurt pipeline re-draws the same vertices through the source's damage combine: the
//! lightmap stage's interpolate, `0.7 * previous + 0.3 * (1, 0, 0)`
//! (`RendererLivingEntity.setBrightness`), gated by the same condition — `hurtTime > 0 ||
//! deathTime > 0`. The shadow pipeline draws the flat quad the source lays under an entity's
//! feet (`Render.renderShadow`), from the named shadow texture, blending its texture's alpha
//! by the fade the source computes.
//!
//! The draw order inside one entity is the source's: the model (with its hurt pass in place of
//! the plain one when the damage combine is in force), then the cape layer, then the shadow
//! (`RenderManager.renderEntity` draws the shadow after the entity, `RenderManager.java:388`).
//!
//! The transforms compose the transforms `RendererLivingEntity.doRender` and
//! `RenderPlayer.doRender` build: the position (with a sneaking player's own eighth-block
//! drop), `rotateCorpse`'s `180 - bodyYaw` about y with the death tilt about z, the model
//! flip `scale(-1, -1, 1)`, the player's own `0.9375` shrink (`RenderPlayer.preRenderCallback`)
//! and the `-1.5078125` the living renderer drops the model by, all before the model's own
//! 1/16 geometry. Everything is composed into `f32` world-space vertices on the CPU; the
//! shader applies the frame's view-projection and nothing else.
//!
//! The texture registry holds what draws sample: the named textures uploaded once (`Named`
//! keys; a key with nothing behind it resolves to a generated placeholder and logs), the two
//! default skins, and the per-uuid skins and capes the client uploads as fetches land.
//! [`SkinLookup`] is the resolver both the pass and the later tab list share.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3};
use oxide_assets::atlas::missing_pixels;
use oxide_assets::texture::Texture;

use crate::camera::{Camera, render_eye};
use crate::entity_models::{self, PoseExtra, build_vertices, player};
use crate::fog::FogParams;
use crate::terrain_pass::{color_target, depth_state, primitive_state};

/// The model a draw uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRef {
    /// A player: the slim arms and the model-parts byte (`EnumPlayerModelParts`; this
    /// milestone's draws pin every bit on).
    Player {
        /// Whether the arms draw slim.
        slim: bool,
        /// The model-parts byte: which of the skin's overlay parts the draw wears.
        parts: u8,
    },
    /// A zombie: `ModelZombie`'s raised arms over the biped table (`ModelZombie.java`:31-44).
    Zombie,
    /// A zombie villager: `ModelZombieVillager`'s own head over the biped limbs
    /// (`ModelZombieVillager.java`:25-28).
    ZombieVillager,
    /// A skeleton: the two-wide limbs of `ModelSkeleton` (`ModelSkeleton.java`:20-33).
    Skeleton,
    /// A villager: `ModelVillager`'s table, under the profession's own sheet
    /// (`RenderVillager.getEntityTexture`:32-54).
    Villager {
        /// The profession, `0..5` — the renderer's texture switch.
        profession: u8,
        /// Whether the draw is a child: the renderer's pre-render scale, halved
        /// (`RenderVillager.preRenderCallback`:64-67).
        child: bool,
    },
    /// A witch: the villager's table with the hat and nose (`ModelWitch.java`:9-39).
    Witch,
    /// A giant: the zombie's model drawn at `RenderGiantZombie`'s sixfold scale
    /// (`RenderManager.java`:162).
    Giant,
    /// A snow golem: `ModelSnowMan`'s three boxes and hands (`ModelSnowMan.java`:18-32).
    SnowGolem,
    /// An iron golem: `ModelIronGolem`'s table (`ModelIronGolem.java`:41-61).
    IronGolem,
    /// A pig: `ModelPig`'s quadruped (`ModelPig.java`), saddle or bare.
    Pig {
        /// Whether the draw is saddled, which its saddle layer reads.
        saddle: bool,
    },
    /// A cow: `ModelCow`'s quadruped (`ModelCow.java`:8-25).
    Cow,
    /// A sheep: `ModelSheep2`'s quadruped, fleece or sheared.
    Sheep {
        /// The wool colour, `0..16`.
        wool: u8,
        /// Whether the draw is sheared, which its wool layer reads.
        sheared: bool,
    },
    /// A mooshroom: `ModelCow` under the mooshroom's own sheet (`RenderMooshroom`:10).
    Mooshroom,
    /// A creeper: `ModelCreeper`'s table (`ModelCreeper.java`:16); the charge aura its
    /// renderer layers on is deferred (`RenderCreeper.java`:17).
    Creeper,
    /// A spider: `ModelSpider`'s head, neck, body and eight legs (`ModelSpider.java`:41),
    /// with the eyes layer (`RenderSpider.java`:15).
    Spider,
    /// A cave spider: the spider's table and eyes under the cave spider's own sheet and
    /// its renderer's seventh-tenths scale and shadow (`RenderCaveSpider.java`:9,`:14,`:23`).
    CaveSpider,
    /// An enderman: `ModelEnderman`'s outstretched biped (`ModelEnderman.java`:13), with
    /// the eyes layer (`RenderEnderman.java`:23).
    Enderman,
    /// A chicken: `ModelChicken`'s table (`ModelChicken.java`:18); a child draws the
    /// folded table its own render builds (`ModelChicken.java`:54-72).
    Chicken {
        /// Whether the draw is a child.
        child: bool,
    },
    /// A squid: `ModelSquid`'s body and eight tentacles (`ModelSquid.java`:13).
    Squid,
    /// A slime: `ModelSlime(16)`'s inner body under the gel layer (`RenderSlime.java`:16);
    /// the size rides the renderer's squash pair and the shadow (`:24`, `:32-38`).
    Slime {
        /// The slime size, `1..=4`.
        size: u8,
    },
    /// A magma cube: `ModelMagmaCube`'s core and eight segments (`ModelMagmaCube.java`:12);
    /// the size rides the renderer's squash pair (`RenderMagmaCube.java`:29-36`).
    MagmaCube {
        /// The cube size, `1..=4`.
        size: u8,
    },
    /// A bat: `ModelBat`'s nested ears and wings (`ModelBat.java`:26); hanging folds the
    /// wings and lifts the body (`RenderBat.rotateCorpse`:35-46).
    Bat {
        /// Whether the bat hangs.
        hanging: bool,
    },
    /// A silverfish: `ModelSilverfish`'s seven bodies and three wings
    /// (`ModelSilverfish.java`:21).
    Silverfish,
    /// An endermite: `ModelEnderMite`'s four segments (`ModelEnderMite.java`:13).
    EnderMite,
    /// A horse: `ModelHorse`'s table (`ModelHorse.java`:65-205) under the class's own
    /// colour, marking and armour sheets (`RenderHorse.getEntityTexture`:53-78).
    Horse {
        /// The horse type: `0` horse, `1` donkey, `2` mule, `3` zombie, `4` skeleton.
        variant: u8,
        /// The colour: the variant's low byte (`0..7`).
        colour: u8,
        /// The marking: the variant's high byte (`0..5`).
        markings: u8,
        /// Whether the draw is saddled — the saddle boxes' gate.
        saddle: bool,
        /// The armour's table index: `0` none, `1..3` the worn armour — the armour
        /// sheet's gate (`EntityHorse.java`:750-761, `RenderHorse.getEntityTexture`:76).
        armour: u8,
    },
    /// A wolf: `ModelWolf`'s table (`ModelWolf.java`:36-66) under the taming's own sheets
    /// (`RenderWolf.getEntityTexture`:47-49).
    Wolf {
        /// Whether the wolf is tamed.
        tamed: bool,
        /// The collar colour, `0..16` — the collar layer's palette byte.
        collar: u8,
        /// Whether the wolf is angry — its own sheet and tail.
        angry: bool,
    },
    /// An ocelot: `ModelOcelot`'s table (`ModelOcelot.java`:36-70`) under the cat type's
    /// sheet (`RenderOcelot.getEntityTexture`:23-40) and the taming's own scale
    /// (`RenderOcelot.preRenderCallback`:46-54).
    Ocelot {
        /// The cat type: `0` wild, `1..4` the tamed coats.
        variant: u8,
        /// Whether the draw is a child (`ModelOcelot.render`:79-98).
        child: bool,
        /// Whether the cat is tamed — the tamed cat folds to its renderer's eight
        /// tenths (`RenderOcelot.preRenderCallback`:46-54).
        tamed: bool,
    },
    /// A rabbit: `ModelRabbit`'s table (`ModelRabbit.java`:49-115`) under the variant's
    /// sheet (`RenderRabbit.getEntityTexture`:29-64).
    Rabbit {
        /// The rabbit type: `0..6` and `99`.
        variant: u8,
        /// Whether the draw is a child (`ModelRabbit.render`:131-153).
        child: bool,
    },
    /// A ghast: `ModelGhast`'s body and nine tentacles (`ModelGhast.java`:12-32`) under
    /// the sheet its attacking flag swaps (`RenderGhast.getEntityTexture`:21-24).
    Ghast {
        /// Whether the draw is attacking — the shooting sheet's gate
        /// (`EntityGhast.isAttacking`, watcher 16's byte).
        shooting: bool,
    },
    /// A blaze: `ModelBlaze`'s head and twelve rods (`ModelBlaze.java`:12-22).
    Blaze,
    /// A guardian: `ModelGuardian`'s body, twelve spines, eye and tail
    /// (`ModelGuardian.java`:16-49`).
    Guardian {
        /// Whether the draw is an elder: its own sheet and the renderer's `2.35` scale
        /// (`RenderGuardian.preRenderCallback`:168-170, `getEntityTexture`:177-180).
        elder: bool,
    },
    /// An ender dragon: `ModelDragon`'s head, body, wings, legs and spine chain
    /// (`ModelDragon.java`:47-124`).
    EnderDragon,
    /// A wither: `ModelWither`'s three ribs and three heads (`ModelWither.java`:13-38`)
    /// under the sheet its spawn invulnerability flickers (`RenderWither.getEntityTexture`:33-37`).
    Wither {
        /// The spawn invulnerability's timer, watcher 20's int (`EntityWither.getInvulTime`:623-626):
        /// the sheet flickers by it and the pre-render scale rises through it.
        invul_time: u16,
    },
    /// An arrow: `RenderArrow`'s six quads in the arrow's own scaled space
    /// (`RenderArrow.java`:56-81), under its `45`-degree roll and `0.05625` pre-scale
    /// (`:53-55`; no shadow).
    Arrow,
    /// A boat: `ModelBoat`'s five parts under the renderer's `+0.25` lift, `180 - yaw`
    /// turn, `(-1, -1, 1)` flip and `0.0625` draw (`RenderBoat.doRender`:29-49). The
    /// rock from the damage fields is unmodelled (the harness records neither).
    Boat,
    /// A minecart: the id jitter, the `+0.375` lift, the `180 - yaw` turn and the
    /// `0.0625` draw around `ModelMinecart`'s six parts (`RenderMinecart.doRender`:
    /// 34-39, :75-108) with the subclass's default cargo. The rail pose and the rolling
    /// rock are unmodelled: the cart draws flat on its networked yaw and pitch.
    Minecart {
        /// The subclass's sub-type byte, `0..4` for plain through hopper.
        body: u8,
    },
    /// A painting: the art's own quads under the `180 - yaw` turn and the `0.0625` draw
    /// (`RenderPainting.doRender`:29-36; no shadow). The yaw folds the extra's facing
    /// byte through the source's own `index * 90`.
    Painting {
        /// The art's index, `0..26` (`EntityPainting.EnumArt`'s ordinal).
        art: u8,
    },
    /// An item sprite the generated-item shape covers: the snowball family's billboards,
    /// a dropped sprite item, a frame's flat content. The client's source supplies the
    /// atlas-mapped shape; the extra names the draw's path.
    Sprite {
        /// The sprite's asset key, as the client's atlas holds it.
        key: &'static str,
    },
    /// A block item: the state's own baked model, fetched through the pass's per-state
    /// cache. The metadata folds out of the draw's item damage — its low four bits
    /// (`Block.getStateById`'s fold, `Block.java`:174-178) — or zero when the draw
    /// carries no item extras.
    BlockItem {
        /// The block's id.
        block: u16,
    },
    /// An experience orb: the sheet's icon cell for the orb's value, on the renderer's
    /// one quad (`RenderXPOrb.doRender`:26-65; the pulse reads the draw's pose age).
    Orb {
        /// The orb's XP value, watcher 18's short.
        value: i16,
    },
    /// An item frame: its own wood model with the content its variant names
    /// (`RenderItemFrame.doRender`:52-81, `renderItem`:103-170; no shadow).
    ItemFrame {
        /// The content the frame holds.
        content: FrameContent,
    },
}

/// The content an item frame holds: the source's own three cases
/// (`RenderItemFrame.renderItem`:103-170 draws the nested stack, or the empty frame's
/// bare wood).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameContent {
    /// The empty frame: the wood alone.
    Empty,
    /// A block stack, whose low four damage bits are its metadata.
    Block(u16),
    /// An item sprite the generated-item shape covers.
    Sprite(&'static str),
}

/// The texture a draw samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureRef {
    /// A texture the client uploaded once, by its key.
    Named(&'static str),
    /// A player's skin, resolved through the registry by profile.
    Skin {
        /// The profile's hyphenated uuid.
        uuid: String,
        /// Whether the draw wants the slim default when no skin is uploaded.
        slim: bool,
    },
}

/// The extras a draw carries beyond its model and pose — the per-kind state the kind's own
/// path reads. One variant per kind whose renderer or layers read state the model and pose
/// do not; the rest draw as [`DrawExtra::None`].
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DrawExtra {
    /// A draw whose model and poses carry everything.
    #[default]
    None,
    /// A zombie villager — the flag `RenderZombie` reads to swap the head's model
    /// (`RenderZombie.func_82427_a`).
    ZombieVillager,
    /// A villager: the profession picks the sheet, the child the renderer's scale.
    Villager {
        /// The profession, `0..5`.
        profession: u8,
        /// Whether the draw is a child.
        child: bool,
    },
    /// A sheep: the wool layer's gate and its palette byte.
    Sheep {
        /// The wool colour, `0..16`.
        wool: u8,
        /// Whether the draw is sheared.
        sheared: bool,
    },
    /// A pig: the saddle layer's gate.
    Pig {
        /// Whether the draw is saddled.
        saddle: bool,
    },
    /// A creeper: the kind's own marker — the charge flag lives on the entity and only
    /// the deferred aura layer will read it (`LayerCreeperCharge`).
    Creeper,
    /// A chicken: the child fold rides the model table (`ModelChicken.render`:54-72).
    Chicken {
        /// Whether the draw is a child.
        child: bool,
    },
    /// A slime: the size and the squash pair the renderer's scale folds
    /// (`RenderSlime.preRenderCallback`:32-38).
    Slime {
        /// The slime size, `1..=4`.
        size: u8,
        /// The frame's interpolated `squishFactor`, `0.0..1.0`.
        squish: f32,
    },
    /// A bat: the hang flag the model pose and the corpse shift read (`RenderBat.rotateCorpse`:35-46).
    Bat {
        /// Whether the bat hangs.
        hanging: bool,
    },
    /// A wolf: the collar layer's gate and its palette byte
    /// (`LayerWolfCollar.doRenderLayer`:22-28).
    Wolf {
        /// Whether the wolf is tamed — the collar layer's gate.
        tamed: bool,
        /// The collar colour, `0..16` — the wool table's index.
        collar: u8,
    },
    /// A horse: the marking and armour sheets the layered stack draws over it
    /// (`EntityHorse.getVariantTexturePaths`:774-782).
    Horse {
        /// The marking's table index, `0..5`.
        markings: u8,
        /// The armour's table index, `0..4`.
        armour: u8,
    },
    /// A dropped item: the stack the item path draws, with the bob and spin derived
    /// from the draw's pose age (`RenderEntityItem.doRender`:91-154; no `EntityItem`
    /// update method carries the rotation — `func_177077_a`:47-48` does).
    Item {
        /// The item's id.
        id: i16,
        /// The stack's count: the copy count the stack draws.
        count: u8,
        /// The stack's damage: a block item's low four bits are its metadata.
        damage: i16,
    },
    /// A thrown item's billboard: the snowball family's generated item in the class's own
    /// scale, or a fireball's plain icon quad (`RenderSnowball.java`:26-39,
    /// `RenderFireball.java`:27-55).
    Projectile {
        /// Which billboard shape the class draws.
        billboard: entity_models::objects::Billboard,
        /// The class's own scale: `0.5` for the snowball family, the fireball
        /// registrations' `2.0` and `0.5` (`RenderManager.java`:184-185).
        scale: f32,
    },
    /// An item frame's content: the frame's own rotation slot
    /// (`RenderItemFrame.renderItem`:103-110`.
    Frame {
        /// The rotation, `0..8` (`EntityItemFrame.getRotation`).
        rotation: u8,
    },
    /// A painting: the hanging's facing byte, folded by the source's own
    /// `horizontalIndex * 90` (`EntityHanging.updateFacingWithBoundingBox`:46).
    Painting {
        /// The facing byte, `0..4`.
        facing: u8,
    },
}

/// Whether a model belongs to the object set: its geometry takes its own path, not the
/// mob table's parts and poses.
pub fn is_object(model: ModelRef) -> bool {
    matches!(
        model,
        ModelRef::Arrow
            | ModelRef::Boat
            | ModelRef::Minecart { .. }
            | ModelRef::Painting { .. }
            | ModelRef::Sprite { .. }
            | ModelRef::BlockItem { .. }
            | ModelRef::Orb { .. }
            | ModelRef::ItemFrame { .. }
    )
}

/// The item meshes the object draws read: the atlas-mapped sprite shapes, the baked block
/// states and the frame's own wood.
///
/// The client implements this over its own sheet and model baker, so neither crate is
/// named here. Every mesh the trait hands back is in 1/16 model units with its uvs
/// already mapped into the texture its key names.
pub trait ItemMeshSource {
    /// The generated-item mesh of a sprite key: the item model generator's body and its
    /// alpha-derived edge strips (`ItemModelGenerator.java`:17-234).
    fn generated(&self, key: &str) -> Option<ItemMesh>;
    /// The baked mesh of a block item's state — the same models the terrain bakes.
    fn block_item(&self, block: u16, meta: u8) -> Option<ItemMesh>;
    /// The item frame's own wood model (`models/block/item_frame`, the frame's own
    /// block model).
    fn frame_wood(&self) -> Option<ItemMesh>;
    /// A sprite's own quad: the icon billboards' one quad over the sprite's region, its
    /// normal `(0, 1, 0)` (`RenderFireball.doRender`:46-51).
    fn icon_quad(&self, key: &str) -> Option<ItemMesh>;
}

/// One source-built item mesh: the vertices and the registry key their uvs sample.
#[derive(Debug, Clone)]
pub struct ItemMesh {
    /// The vertex set, in 1/16 model units.
    pub vertices: Arc<entity_models::Vertices>,
    /// The texture registry key the uvs sample.
    pub texture: &'static str,
}

/// The damage-to-metadata fold the cache keys on: a block stack's damage carries the
/// block's metadata in its low four bits (`Block.getStateById`'s fold, `Block.java`:174-178).
pub fn item_meta(damage: u16) -> u8 {
    (damage & 0x0F) as u8
}

/// The cap on distinct block states the cache keeps: distinct block items in view are
/// few, and a state past the cap draws uncached rather than evicting a kept one.
pub const BLOCK_ITEM_CACHE_CAP: usize = 256;

/// The per-state cache of block-item meshes: one built vertex set per distinct block
/// state, built once through the client-supplied source and kept.
#[derive(Default)]
pub struct BlockItemCache {
    /// The entries, keyed by block id and the folded metadata.
    entries: RefCell<BTreeMap<(u16, u8), Option<ItemMesh>>>,
}

impl BlockItemCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The mesh for a block state, built through the source once and kept; a cache at
    /// its cap answers without storing.
    pub fn mesh(&self, source: &dyn ItemMeshSource, block: u16, meta: u8) -> Option<ItemMesh> {
        let mut entries = self.entries.borrow_mut();
        if let Some(mesh) = entries.get(&(block, meta)) {
            return mesh.clone();
        }
        let mesh = source.block_item(block, meta);
        if entries.len() < BLOCK_ITEM_CACHE_CAP {
            entries.insert((block, meta), mesh.clone());
        }
        mesh
    }

    /// The number of states kept.
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Whether the cache keeps nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One entity's draw for a frame, as the window assembles it.
///
/// The positions and angles are already interpolated for the frame's fraction; `light` is the
/// brightness at the entity's feet (`EntityLivingBase.getBrightness`), the vertex colour the
/// draw bakes; `hurt` and `death` are the damage combines the pass gates on; `health` is the
/// boss kinds' pair, for the status the pass will raise.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityDraw {
    /// The box model the draw renders.
    pub model: ModelRef,
    /// The interpolated position, in blocks.
    pub position: [f64; 3],
    /// The interpolated body yaw in degrees (`renderYawOffset`).
    pub body_yaw: f32,
    /// The interpolated net head yaw in degrees.
    pub head_yaw: f32,
    /// The interpolated head pitch in degrees.
    pub head_pitch: f32,
    /// The model's pose input.
    pub pose: entity_models::Pose,
    /// The texture the draw samples.
    pub texture: TextureRef,
    /// The brightness at the entity's feet, `0.0..1.0`.
    pub light: f32,
    /// The hurt window's fraction: one while it is open, else zero.
    pub hurt: f32,
    /// The death ramp's fraction, one at its end.
    pub death: f32,
    /// The health pair for the kinds whose maximum is known, in hearts.
    pub health: Option<(f32, f32)>,
    /// The kind's own extras.
    pub extra: DrawExtra,
}

/// A registered entity texture: the GPU texture and the bind group the pass samples through.
pub struct RegisteredTexture {
    /// The texture itself, kept so its view stays valid.
    texture: wgpu::Texture,
    /// The view the bind group holds.
    view: wgpu::TextureView,
    /// The bind group the pass sets at group 1.
    bind_group: wgpu::BindGroup,
}

impl RegisteredTexture {
    /// The texture's view, for consumers outside the pipeline binding (the tab list's head
    /// draws).
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The texture's size in texels.
    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
}

/// The skin resolver the entity pass and the tab list share: the texture a profile draws
/// with.
pub trait SkinLookup {
    /// The registered texture for a profile's skin: the uploaded skin when one has arrived,
    /// else the default for `slim`.
    fn resolve(&self, uuid: &str, slim: bool) -> &RegisteredTexture;
}

/// One profile's uploaded textures.
struct SkinEntry {
    /// The skin, at the profile's own resolution; `None` until one arrives, so the resolver
    /// falls back to the default.
    skin: Option<RegisteredTexture>,
    /// The cape, when the profile named one and it arrived; absent clears it.
    cape: Option<RegisteredTexture>,
}

/// The entity texture registry: the named textures, the default skins, the per-profile skins
/// and the placeholder a missing key resolves to.
pub struct TextureRegistry {
    /// The layout every texture bind group is built against, shared with the pipeline.
    layout: wgpu::BindGroupLayout,
    /// The sampler every entity texture is sampled through: nearest, clamped — the pixel
    /// sheet's own look.
    sampler: wgpu::Sampler,
    /// The named textures, by key.
    named: BTreeMap<&'static str, RegisteredTexture>,
    /// The wide and slim default skins, in that order.
    defaults: [RegisteredTexture; 2],
    /// The placeholder a named key with nothing behind it resolves to: the generated
    /// magenta-and-black checkerboard the atlas's fallback sprite is built from.
    placeholder: RegisteredTexture,
    /// The per-profile entries, ascending uuid.
    skins: BTreeMap<String, SkinEntry>,
}

impl TextureRegistry {
    /// Builds the registry with its placeholder; no named texture and no default skin is
    /// uploaded yet.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = texture_bind_group_layout(device);
        let sampler = device.create_sampler(&texture_sampler_descriptor());
        let placeholder = upload_texture(
            device,
            queue,
            "oxide entity placeholder",
            &placeholder_image(),
            &layout,
            &sampler,
        );
        let defaults = [
            upload_texture(
                device,
                queue,
                "oxide entity default wide",
                &placeholder_image(),
                &layout,
                &sampler,
            ),
            upload_texture(
                device,
                queue,
                "oxide entity default slim",
                &placeholder_image(),
                &layout,
                &sampler,
            ),
        ];
        Self {
            layout,
            sampler,
            named: BTreeMap::new(),
            defaults,
            placeholder,
            skins: BTreeMap::new(),
        }
    }

    /// The bind group layout the entity pass's second bind group expects; a [`TextureRegistry`]
    /// built over it pairs with the pass.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Uploads a named texture, replacing whatever the key held.
    pub fn set_named(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: &'static str,
        image: &Texture,
    ) {
        let texture = upload_texture(device, queue, key, image, &self.layout, &self.sampler);
        self.named.insert(key, texture);
    }

    /// Uploads the two default skins: the wide fallback and the slim one.
    pub fn set_defaults(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wide: &Texture,
        slim: &Texture,
    ) {
        self.defaults[0] = upload_texture(
            device,
            queue,
            "oxide entity default wide",
            wide,
            &self.layout,
            &self.sampler,
        );
        self.defaults[1] = upload_texture(
            device,
            queue,
            "oxide entity default slim",
            slim,
            &self.layout,
            &self.sampler,
        );
    }

    /// Uploads one profile's skin and cape, replacing the profile's entry whole: an absent
    /// texture clears that half (a re-upload replaces, a missing cape is no cape).
    ///
    /// A profile whose skin has not arrived keeps the default skin but may still carry a
    /// cape; an entry with neither is dropped.
    pub fn set_skin(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uuid: &str,
        skin: Option<&Texture>,
        cape: Option<&Texture>,
    ) {
        let skin = skin.map(|skin| {
            upload_texture(
                device,
                queue,
                "oxide entity skin",
                skin,
                &self.layout,
                &self.sampler,
            )
        });
        let cape = cape.map(|cape| {
            upload_texture(
                device,
                queue,
                "oxide entity cape",
                cape,
                &self.layout,
                &self.sampler,
            )
        });
        if skin.is_none() && cape.is_none() {
            self.skins.remove(uuid);
            return;
        }
        self.skins
            .insert(uuid.to_string(), SkinEntry { skin, cape });
    }

    /// The cape a profile uploaded, when one is registered.
    pub fn cape(&self, uuid: &str) -> Option<&RegisteredTexture> {
        self.skins.get(uuid).and_then(|entry| entry.cape.as_ref())
    }

    /// The texture a draw's reference resolves to: a named key, or a profile's skin.
    ///
    /// A named key with nothing behind it resolves to the placeholder and logs at debug; a
    /// profile with no uploaded skin resolves to the default for `slim`.
    pub fn texture_for(&self, texture: &TextureRef) -> &RegisteredTexture {
        match texture {
            TextureRef::Named(key) => match self.named.get(*key) {
                Some(texture) => texture,
                None => {
                    tracing::debug!(
                        "the entity texture {key:?} has nothing behind it; drawing the placeholder"
                    );
                    &self.placeholder
                }
            },
            TextureRef::Skin { uuid, slim } => self.resolve(uuid, *slim),
        }
    }

    /// The default skin for a slim or wide draw.
    fn default_skin(&self, slim: bool) -> &RegisteredTexture {
        &self.defaults[usize::from(slim)]
    }
}

impl SkinLookup for TextureRegistry {
    fn resolve(&self, uuid: &str, slim: bool) -> &RegisteredTexture {
        match self.skins.get(uuid) {
            Some(entry) => match &entry.skin {
                Some(skin) => skin,
                None => self.default_skin(slim),
            },
            None => self.default_skin(slim),
        }
    }
}

/// The texture bind group layout: the texture at binding [`TEXTURE_BINDING`] and its sampler
/// next to it.
pub(crate) fn texture_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("oxide entity texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: TEXTURE_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: SAMPLER_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// The sampler every entity texture binds: nearest on both filters, clamped at the edges —
/// the pixel sheet's own look, and the clamp the source's shadow quads read off the sprite's
/// edge.
fn texture_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("oxide entity sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    }
}

/// The generated placeholder image: the atlas's own fallback checkerboard, so the pass and
/// the terrain show the same missing texture.
fn placeholder_image() -> Texture {
    let side = 16;
    Texture {
        width: side,
        height: side,
        rgba: missing_pixels(),
    }
}

/// Uploads one RGBA image into a registered texture.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    image: &Texture,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> RegisteredTexture {
    let size = wgpu::Extent3d {
        width: image.width,
        height: image.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &image.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width * 4),
            rows_per_image: Some(image.height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: TEXTURE_BINDING,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: SAMPLER_BINDING,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    RegisteredTexture {
        texture,
        view,
        bind_group,
    }
}

/// The entity pass: its three pipelines, the frame uniforms they read, and the vertex buffer
/// the frame's geometry is uploaded into.
pub struct EntityPass {
    /// The queue the pass uploads through, a handle of the renderer's own.
    queue: wgpu::Queue,
    /// The model pipeline: the lit fragment.
    model_pipeline: wgpu::RenderPipeline,
    /// The hurt pipeline: the damage combine's fragment.
    hurt_pipeline: wgpu::RenderPipeline,
    /// The shadow pipeline: the flat quad, depth writes off.
    shadow_pipeline: wgpu::RenderPipeline,
    /// The eyes pipeline: the model fragment with the source's additive pair
    /// (`LayerSpiderEyes.java`:24, `LayerEndermanEyes.java`:24).
    eyes_pipeline: wgpu::RenderPipeline,
    /// The gel pipeline: the model fragment with the source's straight-alpha pair
    /// (`LayerSlimeGel.java`:26).
    gel_pipeline: wgpu::RenderPipeline,
    /// The uniform buffer holding the frame's matrix, eye, fog and lights.
    frame_buffer: wgpu::Buffer,
    /// The bind group the frame uniforms are read through.
    frame_bind_group: wgpu::BindGroup,
    /// The vertex buffer, grown to hold the largest frame so far.
    vertex_buffer: wgpu::Buffer,
    /// The vertex buffer's capacity in bytes.
    vertex_capacity: usize,
    /// The frame's uniform values, kept so either setter can rewrite them whole.
    frame: FrameUniform,
    /// Whether a camera has been set: without one nothing draws.
    camera_set: bool,
    /// The object draws' mesh source: the client's atlas and model baker, once set.
    item_source: Option<Arc<dyn ItemMeshSource>>,
    /// The block-item meshes: one cached vertex set per distinct block state.
    block_cache: BlockItemCache,
    /// The camera's yaw in degrees, the source's `playerViewY` minus its own `180`.
    view_yaw: f32,
    /// The camera's pitch in degrees, the source's `playerViewX`.
    view_pitch: f32,
}

impl EntityPass {
    /// Builds the pass's pipelines built for colour attachments in `format`.
    ///
    /// The vertex layout is [`vertex_bytes`]' stream; every pipeline tests the depth buffer
    /// in [`DEPTH_FORMAT`]. The model and hurt pipelines cull nothing, as the living
    /// renderer disables culling for the model and its layers and re-enables it only after
    /// them (`RendererLivingEntity.java:92`, `:192`), while the shadow pipeline culls back
    /// faces: the shadow draws after that re-enable, under the world pass's own culling
    /// (`EntityRenderer.java:1328`), and its up-facing quad is wound like the source's, so
    /// the cull only drops it from below. Group 0 is the frame's uniform; group 1 is the
    /// draw's texture, created by the registry against the `layout` handed in, which is the
    /// registry's own.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oxide entity shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("oxide entity frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAME_BYTES as u64),
                },
                count: None,
            }],
        });
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide entity frame"),
            size: FRAME_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oxide entity frame bind group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oxide entity pipeline layout"),
            bind_group_layouts: &[&frame_layout, texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline =
            |label: &str, fragment: &str, plan: PipelinePlan, cull: Option<wgpu::Face>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(VS_ENTRY),
                        buffers: &[vertex_layout()],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fragment),
                        targets: &[color_target(format, plan.blend)],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: primitive_state(cull),
                    depth_stencil: Some(depth_state(plan.depth_write)),
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                })
            };
        let model_pipeline = pipeline(
            "oxide entity model pipeline",
            FRAGMENT_MODEL,
            PipelinePlan {
                blend: None,
                depth_write: true,
            },
            None,
        );
        let hurt_pipeline = pipeline(
            "oxide entity hurt pipeline",
            FRAGMENT_HURT,
            PipelinePlan {
                blend: None,
                depth_write: true,
            },
            None,
        );
        let shadow_pipeline = pipeline(
            "oxide entity shadow pipeline",
            FRAGMENT_SHADOW,
            PipelinePlan {
                blend: Some(shadow_blend()),
                depth_write: false,
            },
            Some(wgpu::Face::Back),
        );
        let eyes_pipeline = pipeline(
            "oxide entity eyes pipeline",
            FRAGMENT_MODEL,
            PipelinePlan {
                blend: Some(additive_blend()),
                depth_write: true,
            },
            None,
        );
        let gel_pipeline = pipeline(
            "oxide entity gel pipeline",
            FRAGMENT_MODEL,
            PipelinePlan {
                blend: Some(alpha_blend()),
                depth_write: true,
            },
            None,
        );
        let vertex_capacity = INITIAL_VERTEX_BYTES;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxide entity vertices"),
            size: vertex_capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            queue: queue.clone(),
            model_pipeline,
            hurt_pipeline,
            shadow_pipeline,
            eyes_pipeline,
            gel_pipeline,
            frame_buffer,
            frame_bind_group,
            vertex_buffer,
            vertex_capacity,
            frame: FrameUniform::default(),
            camera_set: false,
            item_source: None,
            block_cache: BlockItemCache::new(),
            view_yaw: 0.0,
            view_pitch: 0.0,
        }
    }

    /// Writes the frame's camera: its view-projection matrix, the eye and the two standard
    /// item lights in world space.
    ///
    /// The lights are the source's fixed-function pair — positions `(0.2, 1.0, -0.7)` and
    /// `(-0.2, 1.0, 0.7)` in eye space (`RenderHelper.enableStandardItemLighting`) — rotated
    /// into world space by the camera's own rotation, so a face's shading follows the camera
    /// the way the fixed-function pipeline lights it.
    pub fn set_camera(&mut self, camera: Camera, aspect: f32) {
        let eye = render_eye(&camera.pose);
        self.frame.view_projection = camera.view_projection(aspect);
        self.frame.eye = [eye.x, eye.y, eye.z, 0.0];
        let rotation = Mat3::from_mat4(camera.view());
        let lights = entity_lights(rotation);
        self.frame.light0 = [lights[0][0], lights[0][1], lights[0][2], 0.0];
        self.frame.light1 = [lights[1][0], lights[1][1], lights[1][2], 0.0];
        self.write_frame();
        self.camera_set = true;
        self.view_yaw = camera.pose.yaw;
        self.view_pitch = camera.pose.pitch;
    }

    /// Gives the pass the item meshes its object draws read: the client's own atlas and
    /// model tables, reached through [`ItemMeshSource`] so this crate names neither.
    pub fn set_item_source(&mut self, source: Arc<dyn ItemMeshSource>) {
        self.item_source = Some(source);
    }

    /// Writes the frame's fog for the frames that follow.
    pub fn set_fog(&mut self, params: FogParams) {
        self.frame.fog_colour = [params.colour[0], params.colour[1], params.colour[2], 0.0];
        self.frame.fog_params = [params.start, params.end, params.far_plane, 0.0];
        self.write_frame();
    }

    /// Writes the frame uniform back out.
    fn write_frame(&mut self) {
        self.queue
            .write_buffer(&self.frame_buffer, 0, &self.frame.to_bytes());
    }

    /// Draws the frame's entities.
    ///
    /// Every draw composes its model-space vertices into world space, uploads them in one
    /// write, then issues the source's own sequence: the model (or the hurt combine in its
    /// place when the damage window is open), the cape when the parts byte wears it and a cape
    /// texture is registered, and the shadow quad under the feet. Nothing draws until a camera
    /// has been set.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[EntityDraw],
        textures: &TextureRegistry,
    ) {
        if !self.camera_set || draws.is_empty() {
            return;
        }
        let mut vertices: Vec<EntityVertex> = Vec::new();
        let mut built = Vec::with_capacity(draws.len());
        for draw in draws {
            built.push(self.build(draw, textures, &mut vertices));
        }
        if vertices.is_empty() {
            return;
        }
        self.upload(device, &vertices);
        for (draw, geometry) in draws.iter().zip(&built) {
            let texture = textures.texture_for(&draw.texture);
            pass.set_pipeline(&self.model_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, &texture.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(geometry.body.clone(), 0..1);
            if geometry.hurt {
                pass.set_pipeline(&self.hurt_pipeline);
                pass.draw(geometry.body.clone(), 0..1);
            }
            // The layers draw after the body, each through its own sheet and blend
            // (`RenderLiving.renderModel`'s layer walk: the eyes additive, the gel
            // straight-alpha, the rest opaque).
            for (range, key, blend) in &geometry.layers {
                let texture = textures.texture_for(&TextureRef::Named(key));
                let pipeline = match blend {
                    entity_models::layers::Blend::Opaque => &self.model_pipeline,
                    entity_models::layers::Blend::Alpha => &self.gel_pipeline,
                    entity_models::layers::Blend::Additive => &self.eyes_pipeline,
                };
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, &texture.bind_group, &[]);
                pass.draw(range.clone(), 0..1);
            }
            if let Some(range) = &geometry.cape {
                if let TextureRef::Skin { uuid, .. } = &draw.texture {
                    if let Some(cape_texture) = textures.cape(uuid) {
                        pass.set_pipeline(&self.model_pipeline);
                        pass.set_bind_group(1, &cape_texture.bind_group, &[]);
                        pass.draw(range.clone(), 0..1);
                    }
                }
            }
            // The shadow draws last: `RenderManager.renderEntity` draws it after the entity
            // (`RenderManager.java:388`), through the shared shadow sprite.
            if let Some(range) = &geometry.shadow {
                let shadow = textures.texture_for(&TextureRef::Named(SHADOW_TEXTURE));
                pass.set_pipeline(&self.shadow_pipeline);
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &shadow.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(range.clone(), 0..1);
            }
        }
    }

    /// Builds one draw's geometry into `vertices`, returning the ranges to draw.
    fn build(
        &self,
        draw: &EntityDraw,
        textures: &TextureRegistry,
        vertices: &mut Vec<EntityVertex>,
    ) -> BuiltDraw {
        let model = entity_models::model_for(draw.model);
        let colour = [draw.light, draw.light, draw.light, 1.0];

        let mut built = BuiltDraw {
            shadow: None,
            body: 0..0,
            layers: Vec::new(),
            cape: None,
            hurt: draw.hurt > 0.0 || draw.death > 0.0,
        };

        // The shadow first in the buffer (drawn after the model, in the source's order).
        if let Some((corners, uvs, alpha)) = shadow_quad(
            draw,
            entity_models::shadow(draw.model),
            self.frame.eye_world(),
        ) {
            let start = vertices.len() as u32;
            let corners: Vec<EntityVertex> = corners
                .into_iter()
                .zip(uvs)
                .map(|(position, uv)| EntityVertex {
                    position,
                    uv,
                    normal: [0.0, 1.0, 0.0],
                    colour: [1.0, 1.0, 1.0, alpha],
                })
                .collect();
            // Four corners per quad: the quad's two triangles, front-loaded.
            for index in [0, 1, 2, 0, 2, 3] {
                vertices.push(corners[index]);
            }
            built.shadow = Some(start..vertices.len() as u32);
        }

        // The object models take their own geometry and chains: the mob table's
        // parts and poses do not apply, and the object set has no hurt combine.
        if is_object(draw.model) {
            self.build_object(draw, vertices, &mut built, colour);
            built.hurt = false;
            return built;
        }

        // The body: the model's parts posed by the model's own pose. The player's model
        // takes the parts byte with the cape's bit cleared — the cape is its own build
        // through its own sheet.
        let mut rots = model.rest();
        entity_models::pose(draw.model, &draw.pose, &mut rots);
        let body = build_vertices(model, &rots, entity_models::texture_size(draw.model));
        let chain = body_chain(draw);
        let start = vertices.len() as u32;
        push_vertices(vertices, &body, chain, colour);
        built.body = start..vertices.len() as u32;

        // The layers draw after the model, in the source's list order
        // (`RenderLiving.renderModel` walks its layer renderers after the main model), each
        // through its own sheet with its tint multiplied into the vertex colour — a
        // full-bright layer's colours skip the draw's light, as the source pins those
        // layers' lightmap to its constant (`LayerSpiderEyes.java`:35-38).
        for layer in entity_models::layers::draw_layers(draw.model, &draw.extra, &draw.pose) {
            let layer_vertices = build_vertices(layer.model, &layer.transforms, layer.texture_size);
            let light = if layer.full_bright { 1.0 } else { draw.light };
            let tint = [
                light * layer.tint[0],
                light * layer.tint[1],
                light * layer.tint[2],
                1.0,
            ];
            let start = vertices.len() as u32;
            push_vertices(vertices, &layer_vertices, chain, tint);
            built
                .layers
                .push((start..vertices.len() as u32, layer.texture, layer.blend));
        }

        // The cape: the layer's own box, wave and chain.
        if let ModelRef::Player { parts, .. } = draw.model {
            if parts & player::PART_CAPE != 0 {
                if let TextureRef::Skin { uuid, .. } = &draw.texture {
                    if textures.cape(uuid).is_some() {
                        let rot = player::cape_rot(&draw.pose, parts);
                        let cape = build_vertices(
                            &player::MODEL_PLAYER_CAPE,
                            std::slice::from_ref(&rot),
                            player::CAPE_TEXTURE_SIZE,
                        );
                        let motion = match draw.pose.extra {
                            PoseExtra::Player(cape) => cape.motion,
                            _ => [0.0; 3],
                        };
                        let angles = player::cape_rotation(&draw.pose, motion);
                        let start = vertices.len() as u32;
                        push_vertices(vertices, &cape, cape_chain(draw, angles), colour);
                        built.cape = Some(start..vertices.len() as u32);
                    }
                }
            }
        }
        built
    }
}

/// The item draw path's own tail: the pre-transform's flat doubling, `renderItem`'s
/// `0.5` scale and centring translate, then the mesh's 1/16 units
/// (`RenderItem.renderItemModelTransform`:316-320, `RenderItem.renderItem`:140-157).
fn item_tail(gui3d: bool) -> Mat4 {
    Mat4::from_scale(Vec3::splat(
        entity_models::objects::item_pretransform(gui3d)
            * entity_models::objects::ITEM_RENDER_SCALE,
    )) * Mat4::from_translation(Vec3::from(entity_models::objects::ITEM_CENTRE))
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// The dropped item's fields for a draw: whether its model is the 3D kind (the block
/// items; the loop's own scale and the pre-transform's flat doubling key on it,
/// `RenderItem.preTransform`:254-257), the age the bob and spin read (the draw's pose
/// age; `hoverStart` is zero on the wire path) and the copy count.
fn item_fields(model: ModelRef, extra: &DrawExtra, pose: &entity_models::Pose) -> (bool, f32, u8) {
    let count = match extra {
        DrawExtra::Item { count, .. } => *count,
        _ => 1,
    };
    (matches!(model, ModelRef::BlockItem { .. }), pose.age, count)
}

/// The boat's and the minecart's shared prefix: the world position (a cart's id jitter
/// already folded into it, the renderer's own translate order), the lift, the
/// `180 - yaw` turn and the pitch about z (`RenderBoat.doRender`:29-30,
/// `RenderMinecart.doRender`:34-39, :75-77).
fn vehicle_prefix(position: Vec3, body_yaw: f32, pitch: f32, lift: f32, jitter: [f32; 3]) -> Mat4 {
    Mat4::from_translation(Vec3::new(
        position.x + jitter[0],
        position.y + jitter[1],
        position.z + jitter[2],
    )) * Mat4::from_translation(Vec3::new(0.0, lift, 0.0))
        * Mat4::from_rotation_y((180.0 - body_yaw).to_radians())
        * Mat4::from_rotation_z((-pitch).to_radians())
}

/// Pushes one raw vertex set as an alpha group through `chain`.
fn push_group(
    vertices: &mut Vec<EntityVertex>,
    built: &mut BuiltDraw,
    mesh: &entity_models::Vertices,
    chain: Mat4,
    colour: [f32; 4],
    texture: &'static str,
) {
    let start = vertices.len() as u32;
    push_vertices(vertices, mesh, chain, colour);
    built.layers.push((
        start..vertices.len() as u32,
        texture,
        entity_models::layers::Blend::Alpha,
    ));
}

/// Pushes a source-built mesh, when the source had one, as an alpha group through `chain`.
fn push_source_group(
    vertices: &mut Vec<EntityVertex>,
    built: &mut BuiltDraw,
    mesh: Option<ItemMesh>,
    chain: Mat4,
    colour: [f32; 4],
) {
    if let Some(mesh) = mesh {
        push_group(vertices, built, &mesh.vertices, chain, colour, mesh.texture);
    }
}

/// The object set's build: every object model's mesh, chain and groups.
///
/// Each match arm composes the class's own renderer chain — the root always the draw's
/// world position, the leaves the mesh's 1/16 units — and lands its groups in the draw's
/// layer list, which the frame's draw walks through the alpha blend: the object renderers
/// disable culling and blend the sprites' own alpha.
impl EntityPass {
    fn build_object(
        &self,
        draw: &EntityDraw,
        vertices: &mut Vec<EntityVertex>,
        built: &mut BuiltDraw,
        colour: [f32; 4],
    ) {
        let Some(source) = self.item_source.as_deref() else {
            return;
        };
        let position = Vec3::new(
            draw.position[0] as f32,
            draw.position[1] as f32,
            draw.position[2] as f32,
        );
        // The source's `playerViewY` is the camera's yaw plus 180 (`RenderManager.java`:491).
        let view_y = self.view_yaw + 180.0;
        let view_x = self.view_pitch;
        match (draw.model, &draw.extra) {
            (ModelRef::Arrow, _) => {
                let chain = Mat4::from_translation(position)
                    * Mat4::from_rotation_y((draw.body_yaw - 90.0).to_radians())
                    * Mat4::from_rotation_z(draw.head_pitch.to_radians())
                    * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_4)
                    * Mat4::from_scale(Vec3::splat(entity_models::objects::ARROW_SCALE))
                    * Mat4::from_translation(Vec3::new(-4.0, 0.0, 0.0));
                push_group(
                    vertices,
                    built,
                    &entity_models::objects::arrow_vertices(),
                    chain,
                    colour,
                    entity_models::objects::ARROW_TEXTURE,
                );
            }
            (ModelRef::Boat, _) => {
                let model = &entity_models::objects::MODEL_BOAT;
                let mesh = build_vertices(model, &model.rest(), [64.0, 64.0]);
                let chain = vehicle_prefix(position, draw.body_yaw, 0.0, 0.25, [0.0; 3])
                    * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
                    * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                push_group(
                    vertices,
                    built,
                    &mesh,
                    chain,
                    colour,
                    entity_models::objects::BOAT_TEXTURE,
                );
            }
            (ModelRef::Minecart { body }, _) => {
                let model = &entity_models::objects::MODEL_MINECART;
                let mesh = build_vertices(model, &model.rest(), [64.0, 64.0]);
                let prefix =
                    vehicle_prefix(position, draw.body_yaw, draw.head_pitch, 0.375, [0.0; 3]);
                let flip = prefix
                    * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
                    * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                push_group(
                    vertices,
                    built,
                    &mesh,
                    flip,
                    colour,
                    entity_models::objects::MINECART_TEXTURE,
                );
                // The cargo: the subclass's default tile at the renderer's own `0.75`
                // scale and offset (`RenderMinecart.doRender`:91-100).
                if let Some(cargo) = entity_models::objects::minecart_cargo(
                    entity_models::objects::minecart_body(body),
                ) {
                    let mesh = self.block_cache.mesh(source, cargo.block, cargo.meta);
                    let chain = prefix
                        * Mat4::from_scale(Vec3::splat(0.75))
                        * Mat4::from_translation(Vec3::new(
                            -0.5,
                            (cargo.offset - 8) as f32 / 16.0,
                            0.5,
                        ))
                        * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                    push_source_group(vertices, built, mesh, chain, colour);
                }
            }
            (ModelRef::Painting { art }, extra) => {
                // The hanging's yaw is its facing's own `horizontalIndex * 90`
                // (`EntityHanging.updateFacingWithBoundingBox`:46); the art is the
                // draw's own table index.
                let facing = match extra {
                    DrawExtra::Painting { facing } => *facing,
                    _ => 0,
                };
                let art = entity_models::objects::art(art);
                let chain = Mat4::from_translation(position)
                    * Mat4::from_rotation_y(
                        (180.0 - entity_models::objects::painting_yaw(facing)).to_radians(),
                    )
                    * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                push_group(
                    vertices,
                    built,
                    &entity_models::objects::painting_vertices(art),
                    chain,
                    colour,
                    entity_models::objects::PAINTING_TEXTURE,
                );
            }
            (ModelRef::Orb { value }, _) => {
                let corners = entity_models::objects::orb_corners();
                let uvs = entity_models::objects::orb_uv(entity_models::objects::orb_icon(value));
                let mut quad = entity_models::Vertices::default();
                for index in 0..4 {
                    let corner = corners[index];
                    quad.positions
                        .push([corner[0] * 16.0, corner[1] * 16.0, corner[2] * 16.0]);
                    quad.uvs.push(uvs[index]);
                    quad.normals.push([0.0, 1.0, 0.0]);
                }
                let pair = entity_models::objects::billboard_angles(
                    view_y,
                    view_x,
                    entity_models::objects::Billboard::Fireball,
                );
                let chain = Mat4::from_translation(position)
                    * Mat4::from_rotation_y(pair[0].to_radians())
                    * Mat4::from_rotation_x(pair[1].to_radians())
                    * Mat4::from_scale(Vec3::splat(entity_models::objects::ORB_SCALE))
                    * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                // The pulse rides the draw's age in place of the orb's own tick counter.
                let pulse = entity_models::objects::orb_colour(draw.pose.age / 2.0);
                let tint = [
                    colour[0] * pulse[0],
                    colour[1] * pulse[1],
                    colour[2] * pulse[2],
                    pulse[3],
                ];
                push_group(
                    vertices,
                    built,
                    &quad,
                    chain,
                    tint,
                    entity_models::objects::ORB_TEXTURE,
                );
            }
            (ModelRef::Sprite { key }, extra) => match extra {
                DrawExtra::Projectile { billboard, scale } => {
                    let pair = entity_models::objects::billboard_angles(view_y, view_x, *billboard);
                    let chain = Mat4::from_translation(position)
                        * Mat4::from_scale(Vec3::splat(*scale))
                        * Mat4::from_rotation_y(pair[0].to_radians())
                        * Mat4::from_rotation_x(pair[1].to_radians());
                    match billboard {
                        entity_models::objects::Billboard::Snowball => {
                            let chain = chain * item_tail(false);
                            push_source_group(
                                vertices,
                                built,
                                source.generated(key),
                                chain,
                                colour,
                            );
                        }
                        entity_models::objects::Billboard::Fireball => {
                            let chain = chain * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                            push_source_group(
                                vertices,
                                built,
                                source.icon_quad(key),
                                chain,
                                colour,
                            );
                        }
                    }
                    // The throwable renderers leave their shadow sizes at zero.
                    built.shadow = None;
                }
                _ => {
                    let (gui3d, age, count) = item_fields(draw.model, extra, &draw.pose);
                    let hover = 0.0_f32;
                    let bob = entity_models::objects::item_bob(age, hover)
                        + entity_models::objects::ITEM_GROUND_LIFT;
                    let spin = entity_models::objects::item_spin_degrees(age, hover);
                    let copies = entity_models::objects::item_copies(count);
                    let centre = entity_models::objects::item_copy_centre(copies);
                    let prefix = Mat4::from_translation(position)
                        * Mat4::from_translation(Vec3::new(0.0, bob, 0.0))
                        * Mat4::from_rotation_y(spin.to_radians())
                        * Mat4::from_translation(Vec3::new(0.0, 0.0, centre));
                    let mesh = source.generated(key);
                    for copy in 0..copies.max(1) {
                        let chain = prefix
                            * Mat4::from_translation(Vec3::new(
                                0.0,
                                0.0,
                                entity_models::objects::ITEM_COPY_STEP * f32::from(copy),
                            ))
                            * item_tail(gui3d);
                        push_source_group(vertices, built, mesh.clone(), chain, colour);
                    }
                }
            },
            (ModelRef::BlockItem { block }, extra) => {
                // The stack's damage carries the block's metadata in its low four bits
                // (`Block.getStateById`'s fold, `Block.java`:174-178); draws without an
                // item extra take the state's own default.
                let meta = match extra {
                    DrawExtra::Item { damage, .. } => item_meta((*damage).max(0) as u16),
                    _ => 0,
                };
                let mesh = self.block_cache.mesh(source, block, meta);
                match extra {
                    DrawExtra::Projectile { billboard, scale } => {
                        let pair =
                            entity_models::objects::billboard_angles(view_y, view_x, *billboard);
                        let chain = Mat4::from_translation(position)
                            * Mat4::from_scale(Vec3::splat(*scale))
                            * Mat4::from_rotation_y(pair[0].to_radians())
                            * Mat4::from_rotation_x(pair[1].to_radians());
                        match billboard {
                            entity_models::objects::Billboard::Snowball => {
                                let chain = chain * item_tail(true);
                                push_source_group(vertices, built, mesh, chain, colour);
                            }
                            entity_models::objects::Billboard::Fireball => {
                                let chain = chain * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                                push_source_group(vertices, built, mesh, chain, colour);
                            }
                        }
                        built.shadow = None;
                    }
                    _ => {
                        let (gui3d, age, count) = item_fields(draw.model, extra, &draw.pose);
                        let hover = 0.0_f32;
                        let bob = entity_models::objects::item_bob(age, hover)
                            + entity_models::objects::ITEM_GROUND_LIFT;
                        let spin = entity_models::objects::item_spin_degrees(age, hover);
                        let copies = entity_models::objects::item_copies(count);
                        let centre = entity_models::objects::item_copy_centre(copies);
                        let prefix = Mat4::from_translation(position)
                            * Mat4::from_translation(Vec3::new(0.0, bob, 0.0))
                            * Mat4::from_rotation_y(spin.to_radians())
                            * Mat4::from_translation(Vec3::new(0.0, 0.0, centre));
                        for copy in 0..copies.max(1) {
                            let chain = prefix
                                * Mat4::from_translation(Vec3::new(
                                    0.0,
                                    0.0,
                                    entity_models::objects::ITEM_COPY_STEP * f32::from(copy),
                                ))
                                * item_tail(gui3d);
                            push_source_group(vertices, built, mesh.clone(), chain, colour);
                        }
                    }
                }
            }
            (ModelRef::ItemFrame { content }, extra) => {
                let prefix = Mat4::from_translation(position)
                    * Mat4::from_rotation_y((180.0 - draw.body_yaw).to_radians());
                // The wood: the frame's own model, centred in its cell
                // (`RenderItemFrame.doRender`:70-79).
                let chain = prefix
                    * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                    * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                let wood = source.frame_wood();
                push_source_group(vertices, built, wood, chain, colour);
                // The content: the frame's stack at its own `0.4375` depth, under its
                // rotation slot and half scale (`RenderItemFrame.doRender`:77; `renderItem`:103-110, `:153`). The block contents
                // draw as small blocks, the item contents as the generated shape.
                let gui3d = matches!(content, FrameContent::Block(_));
                let mesh = match content {
                    FrameContent::Empty => None,
                    FrameContent::Block(block) => self.block_cache.mesh(source, block, 0),
                    FrameContent::Sprite(key) => source.generated(key),
                };
                let rotation = match extra {
                    DrawExtra::Frame { rotation } => *rotation,
                    _ => 0,
                };
                let mut chain = prefix
                    * Mat4::from_translation(Vec3::new(0.0, 0.0, 0.4375))
                    * Mat4::from_rotation_z(
                        entity_models::objects::frame_rotation_degrees(rotation).to_radians(),
                    );
                if !gui3d {
                    // A flat content turns back to face the frame's own front
                    // (`RenderItemFrame.renderItem`:141-146).
                    chain *= Mat4::from_rotation_y(std::f32::consts::PI);
                }
                chain *= Mat4::from_scale(Vec3::splat(0.5)) * item_tail(gui3d);
                push_source_group(vertices, built, mesh, chain, colour);
            }
            _ => {}
        }
    }
}

/// The body chain for a draw: the source's `renderLivingAt` composition — the interpolated
/// position, the class's own rotate-corpse shift (the bat's bob and the squid's translate
/// pair, `RenderBat.rotateCorpse`:39, `RenderSquid.rotateCorpse`:29-33),
/// the `180 - body_yaw` turn — the dragon's own override replaces it with the movement
/// ring's pinned turn and its block-back step (`RenderDragon.rotateCorpse`:33-39) — the
/// death tilt at the renderer's own largest angle with the class's extra roll, the
/// `(-1, -1, 1)` flip, the class's pre-render scale (the cubes' squash pair riding it
/// per-axis, `RenderSlime.preRenderCallback`:34-37), the `-1.5078125` model drop and the
/// model's own sneak lift, then the model's 1/16 units (`Render.doRender`, the renderer's
/// pre-render callback, `RendererLivingEntity.doRender`).
fn body_chain(draw: &EntityDraw) -> Mat4 {
    let [_, lift] = entity_models::sneak_terms(draw.model);
    let position = Vec3::new(
        draw.position[0] as f32,
        draw.position[1] as f32 - sneak_drop(draw),
        draw.position[2] as f32,
    );
    let tilt = draw.death * entity_models::death_rotation(draw.model)
        + entity_models::corpse_roll(draw.model, &draw.pose);
    let shift = entity_models::corpse_shift(draw.model, &draw.pose);
    let lift = if draw.pose.sneak { lift } else { 0.0 };
    let scale = match entity_models::cube_scale(draw.model, squish_of(draw)) {
        Some([x, y, z]) => Vec3::new(x, y, z),
        None => Vec3::splat(entity_models::render_scale(draw.model)),
    };
    // The class's own rotate-corpse turn: the base `180 - body_yaw` turn every class
    // takes, save the dragon — its override replaces the base turn outright
    // (`RenderDragon.rotateCorpse`:33-39) — the movement ring's yaw turn and its pitch
    // lean (`f1 * 10`) both pin (no frame input carries the ring), leaving the
    // override's block-back step in the turn's place, in front of the death tilt.
    let turn = match draw.model {
        ModelRef::EnderDragon => Mat4::from_translation(Vec3::new(0.0, 0.0, 1.0)),
        _ => Mat4::from_rotation_y((180.0 - draw.body_yaw).to_radians()),
    };
    // The model's own level transform in front of its parts (`ModelDragon.render`:144-147):
    // the dragon's flight — the whole model's translate, then its pitch, on the flap wave —
    // composes here, ahead of every part and its children. The ghast's per-part spread
    // cannot carry this one: the dragon's parts nest (head, wings and legs hold children),
    // so every part's own copy would land the flight twice, and the source composes both
    // terms as the one model-level matrix ahead of the parts.
    let (flight, pitch) = match draw.model {
        ModelRef::EnderDragon => entity_models::exotics::dragon_flight(&draw.pose),
        _ => ([0.0; 3], 0.0),
    };
    Mat4::from_translation(position)
        * Mat4::from_translation(Vec3::new(0.0, shift, 0.0))
        * turn
        * Mat4::from_rotation_z(tilt.to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(scale)
        * Mat4::from_translation(Vec3::new(0.0, MODEL_DROP, 0.0))
        * Mat4::from_translation(Vec3::new(0.0, lift, 0.0))
        * Mat4::from_translation(Vec3::from(flight))
        * Mat4::from_rotation_x(pitch.to_radians())
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// The squash factor a cube draw carries this frame: the slime's rides the draw's extras,
/// the magma cube's its pose's own variant — the interpolated `squishFactor` both their
/// renderers' scale and their models' segments read (`RenderSlime.preRenderCallback`:34-35,
/// `ModelMagmaCube.setLivingAnimations`:42-56).
fn squish_of(draw: &EntityDraw) -> f32 {
    match (&draw.extra, draw.pose.extra) {
        (DrawExtra::Slime { squish, .. }, _) => *squish,
        (_, PoseExtra::MagmaCube { squish }) => squish,
        _ => 0.0,
    }
}

/// The cape layer's chain: the body's, without the model's sneak lift, with the layer's own
/// `+0.125` z offset and its three turns (`LayerCape.doRenderLayer`).
fn cape_chain(draw: &EntityDraw, angles: [f32; 3]) -> Mat4 {
    let position = Vec3::new(
        draw.position[0] as f32,
        draw.position[1] as f32 - sneak_drop(draw),
        draw.position[2] as f32,
    );
    let death = (draw.death * entity_models::death_rotation(draw.model)).to_radians();
    Mat4::from_translation(position)
        * Mat4::from_rotation_y((180.0 - draw.body_yaw).to_radians())
        * Mat4::from_rotation_z(death)
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(entity_models::render_scale(draw.model)))
        * Mat4::from_translation(Vec3::new(0.0, MODEL_DROP, 0.0))
        * Mat4::from_translation(Vec3::new(0.0, 0.0, CAPE_OFFSET))
        * Mat4::from_rotation_x(angles[0].to_radians())
        * Mat4::from_rotation_z(angles[2].to_radians())
        * Mat4::from_rotation_y(angles[1].to_radians())
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

impl EntityPass {
    /// Grows the vertex buffer if the frame needs it and writes the frame's vertices.
    fn upload(&mut self, device: &wgpu::Device, vertices: &[EntityVertex]) {
        let bytes = vertices.len() * VERTEX_BYTES;
        if bytes > self.vertex_capacity {
            let capacity = bytes.next_power_of_two();
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("oxide entity vertices"),
                size: capacity as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = capacity;
        }
        self.queue
            .write_buffer(&self.vertex_buffer, 0, &vertex_bytes(vertices));
    }
}

/// One draw's built ranges into the frame's vertex buffer.
struct BuiltDraw {
    /// The shadow quad's range, when the fade leaves it visible.
    shadow: Option<Range<u32>>,
    /// The model's range.
    body: Range<u32>,
    /// The layers' ranges with their sheets' registry keys and blends, in the model's layer
    /// order.
    layers: Vec<(Range<u32>, &'static str, entity_models::layers::Blend)>,
    /// The cape's range, when both the bit and a texture are present.
    cape: Option<Range<u32>>,
    /// Whether the hurt combine draws over the body.
    hurt: bool,
}

/// The shadow quad's geometry: its four corners, their uvs and the vertex alpha.
type ShadowQuad = ([[f32; 3]; 4], [[f32; 2]; 4], f32);

/// The shadow quad for a draw: its four corners and their uvs, and the vertex alpha.
///
/// The quad is the block the entity stands in, centred on the entity and a whit above the
/// feet, the sprite sampled from corner to corner with the source's own mapping
/// (`Render.renderShadowBlock`: `(x - minX) / 2f + 0.5` reads one at the low corner, so the
/// sprite runs backwards). The alpha is the source's own: the camera's squared-distance fade
/// `(1 - d² / 256) * shadowOpaque`, with `d²` the sum of the squared axis offsets
/// (`Render.doRenderShadowAndFire`:305-306, `RenderManager.getDistanceToCamera`:478-484),
/// halved for the block under the entity and scaled by the feet's light
/// (`Render.renderShadowBlock`'s `d0` for the block the entity stands in). The fade reaches
/// zero at sixteen blocks, so the quad falls away there.
fn shadow_quad(draw: &EntityDraw, shadow: [f32; 2], eye: [f32; 3]) -> Option<ShadowQuad> {
    let [size, opacity] = shadow;
    let position = Vec3::new(
        draw.position[0] as f32,
        draw.position[1] as f32,
        draw.position[2] as f32,
    );
    let squared_distance = position.distance_squared(Vec3::from(eye));
    let fade = (1.0 - squared_distance / SHADOW_FADE_DISTANCE) * opacity;
    let alpha = fade * 0.5 * draw.light;
    if alpha <= 0.0 {
        return None;
    }
    let half = size;
    let y = position.y + SHADOW_LIFT;
    let corners = [
        [position.x - half, y, position.z - half],
        [position.x - half, y, position.z + half],
        [position.x + half, y, position.z + half],
        [position.x + half, y, position.z - half],
    ];
    let uvs = [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
    Some((corners, uvs, alpha))
}

/// The two standard item lights in world space, from the eye-space pair the source sets.
fn entity_lights(rotation: Mat3) -> [[f32; 3]; 2] {
    let eye_to_world = rotation.transpose();
    let l0 = eye_to_world * Vec3::new(0.2, 1.0, -0.7).normalize();
    let l1 = eye_to_world * Vec3::new(-0.2, 1.0, 0.7).normalize();
    [l0.normalize().into(), l1.normalize().into()]
}

/// The position drop a sneaking draw takes, in blocks: the player's own renderer drops it a
/// sneak's eighth (`RenderPlayer.doRender`); the mob renderers do not.
fn sneak_drop(draw: &EntityDraw) -> f32 {
    if draw.pose.sneak {
        entity_models::sneak_terms(draw.model)[0]
    } else {
        0.0
    }
}

/// Pushes a built vertex set through `chain` with the draw's colour.
///
/// The build emits four corners per quad; the stream carries the quad's two triangles — the
/// pipelines index nothing — so each quad lands front-loaded as `(0, 1, 2)` and `(0, 2, 3)`.
fn push_vertices(
    out: &mut Vec<EntityVertex>,
    vertices: &entity_models::Vertices,
    chain: Mat4,
    colour: [f32; 4],
) {
    let corner = |index: usize| EntityVertex {
        position: chain
            .transform_point3(Vec3::from(vertices.positions[index]))
            .into(),
        uv: vertices.uvs[index],
        normal: chain
            .transform_vector3(Vec3::from(vertices.normals[index]))
            .normalize_or_zero()
            .into(),
        colour,
    };
    let quads = vertices.positions.len() & !3;
    for quad in (0..quads).step_by(4) {
        for index in [quad, quad + 1, quad + 2, quad, quad + 2, quad + 3] {
            out.push(corner(index));
        }
    }
}

/// One vertex the entity pipelines take.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
struct EntityVertex {
    /// World-space position.
    position: [f32; 3],
    /// The texture uv.
    uv: [f32; 2],
    /// The face's normal.
    normal: [f32; 3],
    /// The vertex colour: the entity's brightness in the rgb channels.
    colour: [f32; 4],
}

/// The vertex stream's stride in bytes: a `Float32x3` position, a `Float32x2` uv, a
/// `Float32x3` normal and a `Float32x4` colour.
const VERTEX_BYTES: usize = 12 + 8 + 12 + 16;

/// The vertex attributes [`vertex_bytes`] lays out.
static ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Float32x3,
    3 => Float32x4
];

/// The vertex layout the pipelines declare.
fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_BYTES as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

/// Packs the frame's vertices into little-endian bytes.
fn vertex_bytes(vertices: &[EntityVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_BYTES);
    for vertex in vertices {
        for value in vertex.position {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.uv {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.normal {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in vertex.colour {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// The frame's uniform values, laid out as the shader's `Frame` struct.
#[derive(Debug, Clone, Copy)]
struct FrameUniform {
    /// The camera's view-projection matrix.
    view_projection: Mat4,
    /// The eye's world position, the fourth component unused.
    eye: [f32; 4],
    /// The fog colour, the alpha unused.
    fog_colour: [f32; 4],
    /// The fog's start, end and far plane.
    fog_params: [f32; 4],
    /// The first standard item light's world direction.
    light0: [f32; 4],
    /// The second standard item light's world direction.
    light1: [f32; 4],
}

impl Default for FrameUniform {
    fn default() -> Self {
        Self {
            view_projection: Mat4::IDENTITY,
            eye: [0.0; 4],
            fog_colour: [0.0; 4],
            fog_params: [0.0; 4],
            light0: [0.0; 4],
            light1: [0.0; 4],
        }
    }
}

/// The size of the frame uniform in bytes: a matrix and five `vec4`s.
const FRAME_BYTES: usize = 64 + 5 * 16;

impl FrameUniform {
    /// The eye the frame was set with, in world space; zero before a camera is set.
    fn eye_world(&self) -> [f32; 3] {
        [self.eye[0], self.eye[1], self.eye[2]]
    }

    /// Packs the uniform into little-endian bytes.
    fn to_bytes(self) -> [u8; FRAME_BYTES] {
        let mut bytes = [0u8; FRAME_BYTES];
        let mut at = 0;
        push_values(&mut bytes, &mut at, &self.view_projection.to_cols_array());
        push_values(&mut bytes, &mut at, &self.eye);
        push_values(&mut bytes, &mut at, &self.fog_colour);
        push_values(&mut bytes, &mut at, &self.fog_params);
        push_values(&mut bytes, &mut at, &self.light0);
        push_values(&mut bytes, &mut at, &self.light1);
        bytes
    }
}

/// Appends one run of `f32`s to a byte buffer at a cursor.
fn push_values(bytes: &mut [u8], at: &mut usize, values: &[f32]) {
    for value in values {
        bytes[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
        *at += 4;
    }
}

/// One pipeline's state choices, kept as a value so the tests can pin them without a device.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PipelinePlan {
    /// The colour blend; the model and hurt pipelines draw the source's unblended state.
    blend: Option<wgpu::BlendState>,
    /// Whether the pipeline writes the depth buffer.
    depth_write: bool,
}

/// The shadow's blend: source alpha over one-minus-source alpha, the alpha channel kept
/// (`Render.renderShadow`: `blendFunc(770, 771)`).
fn shadow_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The eyes layers' blend: the source's `blendFunc(1, 1)` — the layer's colours added onto
/// the frame (`LayerSpiderEyes.java`:24, `LayerEndermanEyes.java`:24).
fn additive_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The gel layer's blend: the source's `blendFunc(770, 771)` — source alpha over
/// one-minus-source alpha (`LayerSlimeGel.java`:26).
fn alpha_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

/// The texture binding in the texture group.
const TEXTURE_BINDING: u32 = 0;

/// The sampler binding in the texture group.
const SAMPLER_BINDING: u32 = 1;

/// The vertex entry point.
const VS_ENTRY: &str = "vs_main";

/// The model fragment: the lit sheet.
const FRAGMENT_MODEL: &str = "fs_model";

/// The hurt fragment: the damage combine.
const FRAGMENT_HURT: &str = "fs_hurt";

/// The shadow fragment.
const FRAGMENT_SHADOW: &str = "fs_shadow";

/// The alpha below which the model and hurt fragments discard: the client's own tenth.
const CUTOUT_ALPHA: f32 = 0.1;

/// The drop the living renderer puts under every model, in blocks
/// (`RendererLivingEntity.doRender`: `translate(0, -1.5078125, 0)`).
const MODEL_DROP: f32 = -1.5078125;

/// The cape layer's own forward offset, in blocks (`LayerCape.doRenderLayer`).
const CAPE_OFFSET: f32 = 0.125;

/// The shadow sprite's key: the shared sheet every shadow quad samples
/// (`Render.renderShadow`'s `misc/shadow.png`).
const SHADOW_TEXTURE: &str = "misc/shadow.png";

/// The squared camera distance at which the shadow's fade reaches zero
/// (`Render.doRenderShadowAndFire`:305-306 — the alpha `(1 - d² / 256) * shadowOpaque`, with
/// `d²` the squared distance `RenderManager.getDistanceToCamera`:478-484 measures): zero at
/// sixteen blocks, nothing past it.
const SHADOW_FADE_DISTANCE: f32 = 256.0;

/// How far above the feet the shadow quad lies, in blocks (`Render.renderShadowBlock`:
/// `pos.getY() + blockBounds + dy + 0.015625`).
const SHADOW_LIFT: f32 = 0.015625;

/// The first vertex buffer's capacity in bytes.
const INITIAL_VERTEX_BYTES: usize = 1024 * 48;

/// The shader source: the entity shader.
fn shader_source() -> String {
    format!(
        r#"
struct Frame {{
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    fog_colour: vec4<f32>,
    fog_params: vec4<f32>,
    light0: vec4<f32>,
    light1: vec4<f32>,
}};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var entity: texture_2d<f32>;
@group(1) @binding(1) var entity_sampler: sampler;

// The alpha below which the client's alpha test discards a fragment.
const CUTOUT_ALPHA: f32 = {CUTOUT_ALPHA};

struct VertexInput {{
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) colour: vec4<f32>,
}};

struct VertexOutput {{
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
    @location(2) colour: vec4<f32>,
    @location(3) distance: f32,
}};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {{
    var output: VertexOutput;
    output.clip_position = frame.view_projection * vec4<f32>(input.position, 1.0);
    output.distance = length(input.position - frame.eye.xyz);
    // The two standard item lights: the global ambient plus each light's own diffuse term,
    // clamped once the way the fixed-function pipeline clamps the lit colour.
    let diffuse = max(dot(input.normal, frame.light0.xyz), 0.0)
        + max(dot(input.normal, frame.light1.xyz), 0.0);
    output.shade = min(0.4 + 0.6 * diffuse, 1.0);
    output.uv = input.uv;
    output.colour = input.colour;
    return output;
}}

// The linear fog, the terrain pass's own rule: the factor is one at the fade's start and zero
// at its end; a range that does not run forwards leaves the colour alone.
fn fogged(colour: vec4<f32>, distance: f32) -> vec4<f32> {{
    let span = frame.fog_params.y - frame.fog_params.x;
    if (span <= 0.0) {{
        return colour;
    }}
    let factor = clamp((frame.fog_params.y - distance) / span, 0.0, 1.0);
    return vec4<f32>(mix(frame.fog_colour.rgb, colour.rgb, factor), colour.a);
}}

// The model's fragment: the texel times the entity's brightness and the face's shade, then
// the fog; alpha comes from the texel alone, and the fragment below the client's tenth is
// discarded.
@fragment
fn fs_model(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(entity, entity_sampler, input.uv);
    let colour = vec4<f32>(texel.rgb * input.colour.rgb * input.shade, texel.a);
    if (colour.a < CUTOUT_ALPHA) {{
        discard;
    }}
    return fogged(colour, input.distance);
}}

// The hurt combine: the lightmap stage's interpolate — 0.7 of the lit texel with 0.3 of the
// damage red added (`RendererLivingEntity.setBrightness`'s (1, 0, 0, 0.3) constant).
@fragment
fn fs_hurt(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(entity, entity_sampler, input.uv);
    let lit = texel.rgb * input.colour.rgb * input.shade;
    let colour = vec4<f32>(lit * 0.7 + vec3<f32>(0.3, 0.0, 0.0), texel.a);
    if (colour.a < CUTOUT_ALPHA) {{
        discard;
    }}
    return fogged(colour, input.distance);
}}

// The shadow's fragment: the sprite's texel times the vertex colour, no lighting — the
// source's quad carries no normals.
@fragment
fn fs_shadow(input: VertexOutput) -> @location(0) vec4<f32> {{
    let texel = textureSample(entity, entity_sampler, input.uv);
    let colour = vec4<f32>(texel.rgb * input.colour.rgb, texel.a * input.colour.a);
    return fogged(colour, input.distance);
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraPose, DEFAULT_FOV, NEAR_PLANE, NO_VIEW_EFFECT};
    use glam::Vec3;

    /// A camera at the origin facing north (down -z), where the view rotation is the
    /// identity.
    fn north_camera() -> Camera {
        Camera {
            pose: CameraPose {
                position: [0.0, 0.0, 0.0],
                yaw: 180.0,
                pitch: 0.0,
            },
            fov_degrees: DEFAULT_FOV,
            near: NEAR_PLANE,
            far_chunks: 8.0,
            view_effect: NO_VIEW_EFFECT,
        }
    }

    /// A player draw standing at the origin, lit at half brightness.
    fn player_draw() -> EntityDraw {
        EntityDraw {
            model: ModelRef::Player {
                slim: false,
                parts: entity_models::player::PARTS_ALL,
            },
            position: [0.0, 0.0, 0.0],
            body_yaw: 0.0,
            head_yaw: 0.0,
            head_pitch: 0.0,
            pose: entity_models::Pose::default(),
            texture: TextureRef::Named("entity/steve.png"),
            light: 0.5,
            hurt: 0.0,
            death: 0.0,
            health: None,
            extra: DrawExtra::None,
        }
    }

    #[test]
    fn the_shadow_blend_is_the_sources_alpha_over() {
        let blend = shadow_blend();
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.alpha.src_factor, wgpu::BlendFactor::One);
        assert_eq!(blend.alpha.dst_factor, wgpu::BlendFactor::Zero);
    }

    #[test]
    fn a_plug_keeps_the_frame_uniform_144_bytes() {
        assert_eq!(FRAME_BYTES, 144);
        let uniform = FrameUniform::default();
        assert_eq!(uniform.to_bytes().len(), 144);
    }

    #[test]
    fn the_item_lights_stand_in_eye_space_and_rotate_with_the_camera() {
        let lights = entity_lights(Mat3::IDENTITY);
        let expected0 = Vec3::new(0.2, 1.0, -0.7).normalize();
        let expected1 = Vec3::new(-0.2, 1.0, 0.7).normalize();
        assert!((Vec3::from(lights[0]) - expected0).length() < 1.0e-6);
        assert!((Vec3::from(lights[1]) - expected1).length() < 1.0e-6);
        assert!((Vec3::from(lights[0]).length() - 1.0).abs() < 1.0e-6);
        // The north camera's rotation is the identity, so its lights stand as they are;
        // turning the camera a quarter turn swings them with it.
        let camera = north_camera();
        let north = entity_lights(Mat3::from_mat4(camera.view()));
        assert!((Vec3::from(north[0]) - expected0).length() < 1.0e-5);
        let turned = Camera {
            pose: CameraPose {
                yaw: 90.0,
                ..camera.pose
            },
            ..camera
        };
        let swung = entity_lights(Mat3::from_mat4(turned.view()));
        assert!((Vec3::from(swung[0]) - Vec3::from(north[0])).length() > 0.5);
    }

    #[test]
    fn the_shadow_quad_spans_the_block_and_fades_with_distance() {
        let draw = player_draw();
        let shadow = entity_models::shadow(draw.model);
        // The camera's squared distance drives the fade (`Render.doRenderShadowAndFire`:305-306
        // over `RenderManager.getDistanceToCamera`:478-484): fifteen sixteenths of the
        // opacity at four blocks (1 - 16/256) and three quarters at eight (1 - 64/256).
        for (distance, fade) in [(4.0_f32, 0.9375_f32), (8.0, 0.75)] {
            let (corners, uvs, alpha) = shadow_quad(&draw, shadow, [0.0, 0.0, distance]).unwrap();
            // The quad spans twice the class's shadow size around the entity, just above the
            // feet.
            assert_eq!(corners[0], [-0.5, SHADOW_LIFT, -0.5]);
            assert_eq!(corners[2], [0.5, SHADOW_LIFT, 0.5]);
            // The sprite runs backwards: the low corner reads one (`(x - minX) / 2f + 0.5`).
            assert_eq!(uvs[0], [1.0, 1.0]);
            assert_eq!(uvs[2], [0.0, 0.0]);
            // The alpha: the squared-distance fade times the opacity, halved, times the
            // feet's light.
            let expected = fade * shadow[1] * 0.5 * draw.light;
            assert!(
                (alpha - expected).abs() < 1.0e-6,
                "at {distance} blocks the alpha reads {alpha} against {expected}"
            );
        }
        // The fade reaches zero at sixteen blocks and the source draws nothing past it
        // (`Render.doRenderShadowAndFire`:308's guard).
        assert!(shadow_quad(&draw, shadow, [0.0, 0.0, 16.0]).is_none());
        assert!(shadow_quad(&draw, shadow, [0.0, 0.0, 17.0]).is_none());
    }

    #[test]
    fn the_placeholder_is_the_atlases_checkerboard() {
        // The placeholder a missing named key resolves to is the assets crate's own missing
        // sprite bytes, so both passes show the same fallback.
        let placeholder = placeholder_image();
        assert_eq!(placeholder.width, 16);
        assert_eq!(placeholder.height, 16);
        assert_eq!(placeholder.rgba.len(), 16 * 16 * 4);
        assert_eq!(placeholder.rgba, missing_pixels());
    }

    /// Whether two corners agree to within a ten-thousandth of a block.
    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1.0e-4)
    }

    #[test]
    fn the_body_chain_composes_the_sources_turns_and_scales() {
        // The right arm's outer top corner, (-3, -2, -2) in model units: the 1/16 scale and
        // the drop land it at 1.53 blocks up, the (-1, -1, 1) flip turns the model's down
        // axis into the world's up one, and `180 - body_yaw` turns it.
        let draw = player_draw();
        let corner = body_chain(&draw).transform_point3(Vec3::new(-3.0, -2.0, -2.0));
        assert!(close(
            corner.into(),
            [-0.175_781_25, 1.530_761_7, 0.117_187_5]
        ));
        // A quarter turn of the body: the same corner swings a quarter turn about the
        // entity's own axis.
        let turned = EntityDraw {
            body_yaw: 90.0,
            ..player_draw()
        };
        let corner = body_chain(&turned).transform_point3(Vec3::new(-3.0, -2.0, -2.0));
        assert!(close(
            corner.into(),
            [-0.117_187_5, 1.530_761_7, -0.175_781_25]
        ));
    }

    #[test]
    fn the_death_tilt_quarters_the_model_at_a_full_ramp() {
        // `RendererLivingEntity.rotateCorpse` turns three quarters of the ramp's 90 degrees
        // about Z after the yaw turn; at one the model lies on its side.
        let draw = EntityDraw {
            death: 1.0,
            ..player_draw()
        };
        let top = body_chain(&draw).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        assert!(close(top.into(), [1.882_324_2, 0.0, 0.0]));
        // The ramp's quarter point: a quarter of the turn, the head a quarter over.
        let quarter = EntityDraw {
            death: 0.25,
            ..player_draw()
        };
        let top = body_chain(&quarter).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        let angle = (22.5_f32).to_radians();
        assert!(close(
            top.into(),
            [angle.sin() * 1.882_324_2, angle.cos() * 1.882_324_2, 0.0]
        ));
    }

    #[test]
    fn the_squid_corpse_transform_nets_seven_tenths_down() {
        // The squid's corpse transform at zero squid pitch and yaw collapses to its
        // translate pair (`RenderSquid.rotateCorpse`:29-33): half a block up, then one and
        // a fifth down, so the model's origin lands seven tenths below the drop's own
        // height (1.5078125 − 0.7 = 0.8078125).
        let squid = body_chain(&mob_draw(ModelRef::Squid)).transform_point3(Vec3::ZERO);
        assert!(
            close(squid.into(), [0.0, 0.807_812_5, 0.0]),
            "the squid's origin at {squid:?} against [0.0, 0.8078125, 0.0]"
        );
    }

    #[test]
    fn the_dying_squid_keeps_the_corpse_transform_untilted() {
        // `RenderSquid.rotateCorpse` replaces the base's corpse turn outright
        // (`RenderSquid.rotateCorpse`:25-34) and carries no death block, so the death ramp
        // turns nothing: a full-ramp squid's origin still lands where the translate pair
        // leaves it, seven tenths below the drop's own height (1.5078125 − 0.7).
        let squid = EntityDraw {
            death: 1.0,
            ..mob_draw(ModelRef::Squid)
        };
        let origin = body_chain(&squid).transform_point3(Vec3::ZERO);
        assert!(
            close(origin.into(), [0.0, 0.807_812_5, 0.0]),
            "the dying squid's origin at {origin:?} against [0.0, 0.8078125, 0.0]"
        );
    }

    #[test]
    fn the_dragon_corpse_drops_the_base_turn_and_steps_a_block_back() {
        // `RenderDragon.rotateCorpse`:33-39 replaces the base's `180 - body_yaw` turn
        // outright: the movement ring's turn through its rest — no frame input carries the
        // ring, so its yaw turn and its pitch lean both pin — and the block-back step.
        // Only the base's death tilt survives the override.
        let dragon = EntityDraw {
            pose: entity_models::Pose {
                extra: PoseExtra::Dragon { anim_time: 0.0 },
                ..entity_models::Pose::default()
            },
            ..mob_draw(ModelRef::EnderDragon)
        };
        // The model's origin: the flip and the drop land it three and a half blocks up
        // (1.5078125 + 2 - 0.0171094) and the flight and the step leave it two blocks back.
        let origin = body_chain(&dragon).transform_point3(Vec3::ZERO);
        assert!(
            close(origin.into(), [0.0, 3.490_703, -2.0]),
            "the dragon's origin at {origin:?} against [0.0, 3.490703, -2.0]"
        );
        // A point on the model's +x: the base turn would swing it to the other side; the
        // ring's pinned turn leaves it, so only the flip's mirror shows.
        let side = body_chain(&dragon).transform_point3(Vec3::new(1.0, 0.0, 0.0));
        assert!(
            close(side.into(), [-0.062_5, 3.490_703, -2.0]),
            "the dragon's side at {side:?} against [-0.0625, 3.490703, -2.0]"
        );
        // The death tilt survives: a dying dragon's top point (0, -8, 0) lies down the
        // flank under the ring's step and the flight's own frame.
        let dying = EntityDraw {
            death: 1.0,
            ..dragon
        };
        let top = body_chain(&dying).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        assert!(
            close(top.into(), [-3.990_703, 0.0, -2.000_298_6]),
            "the dying dragon's top at {top:?} against [-3.990703, 0.0, -2.0002986]"
        );
    }

    #[test]
    fn the_arthropod_death_tilt_lies_at_a_half_turn() {
        // The spiders, the silverfish and the endermite turn a half turn over their death
        // ramp (`RenderSpider.java`:18-21, `RenderSilverfish.java`:16-19,
        // `RenderEndermite.java`:16-19); the classes without the override keep the base
        // quarter turn (`RendererLivingEntity.getDeathMaxRotation`:473-476). At a full ramp
        // the flipped model's top point (0, -8, 0) lands under the feet at the half turn,
        // and along the flank at the quarter.
        let spider = EntityDraw {
            death: 1.0,
            ..mob_draw(ModelRef::Spider)
        };
        let top = body_chain(&spider).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        assert!(
            close(top.into(), [0.0, -2.007_812_5, 0.0]),
            "the dying spider's top at {top:?} against [0.0, -2.0078125, 0.0]"
        );
        let creeper = EntityDraw {
            death: 1.0,
            ..mob_draw(ModelRef::Creeper)
        };
        let top = body_chain(&creeper).transform_point3(Vec3::new(0.0, -8.0, 0.0));
        assert!(
            close(top.into(), [2.007_812_5, 0.0, 0.0]),
            "the dying creeper's top at {top:?} against [2.0078125, 0.0, 0.0]"
        );
    }

    #[test]
    fn the_sneak_drop_and_lift_move_the_body_down() {
        let sketch = entity_models::Pose {
            sneak: true,
            ..entity_models::Pose::default()
        };
        let draw = EntityDraw {
            pose: sketch,
            ..player_draw()
        };
        // The drop comes off the position and the model's own lift pushes down again — the
        // model's local +y is the world's down after the flip.
        let feet = body_chain(&draw).transform_point3(Vec3::new(0.0, 24.0, 0.0));
        let plain = body_chain(&player_draw()).transform_point3(Vec3::new(0.0, 24.0, 0.0));
        assert!((feet[1] - (plain[1] - 0.125 - 0.1875)).abs() < 1.0e-4);
    }

    /// A mob draw: the player template's inputs with the kind's model.
    fn mob_draw(model: ModelRef) -> EntityDraw {
        EntityDraw {
            model,
            ..player_draw()
        }
    }

    #[test]
    fn the_giant_scales_sixfold_where_the_zombie_stands_unscaled() {
        // The chain's pre-render scale is the class's own (`RenderGiantZombie.preRenderCallback`:44);
        // the same model point lands at six times the zombie's height.
        let zombie = body_chain(&mob_draw(ModelRef::Zombie));
        let giant = body_chain(&mob_draw(ModelRef::Giant));
        let sample = Vec3::new(0.0, -8.0, 0.0);
        let zombie_y = zombie.transform_point3(sample).y;
        let giant_y = giant.transform_point3(sample).y;
        assert!(
            (giant_y - 6.0 * zombie_y).abs() < 1.0e-4,
            "the giant stands sixfold: {giant_y} against the zombie's {zombie_y}"
        );
        // The villager's pre-render scale shrinks it a sixteenth short of the block
        // (`RenderVillager.preRenderCallback`:62-74), and its child's half of that.
        let villager = body_chain(&mob_draw(ModelRef::Villager {
            profession: 0,
            child: false,
        }));
        let child = body_chain(&mob_draw(ModelRef::Villager {
            profession: 0,
            child: true,
        }));
        let villager_y = villager.transform_point3(sample).y;
        let child_y = child.transform_point3(sample).y;
        assert!(
            (villager_y - 0.9375 * zombie_y).abs() < 1.0e-4,
            "the villager scales 0.9375: {villager_y} against the zombie's {zombie_y}"
        );
        assert!(
            (child_y - 0.5 * villager_y).abs() < 1.0e-4,
            "the child villager halves its size: {child_y} against {villager_y}"
        );
    }

    #[test]
    fn the_mobs_take_the_models_sneak_lift_without_the_players_drop() {
        // A mob's position takes no drop — the player's own renderer takes that eighth
        // (`RenderPlayer.doRender`) and drops nothing else — while the biped models' own
        // render still lifts the model a fifth (`ModelBiped.render`); the models off
        // `ModelBase` take neither.
        let mut sneak_zombie = mob_draw(ModelRef::Zombie);
        sneak_zombie.pose.sneak = true;
        let walk_zombie = body_chain(&mob_draw(ModelRef::Zombie));
        let sneak = body_chain(&sneak_zombie);
        let sample = Vec3::new(0.0, -8.0, 0.0);
        let sneak_y = sneak.transform_point3(sample).y;
        let walk_y = walk_zombie.transform_point3(sample).y;
        assert!(
            (sneak_y - (walk_y - 0.2)).abs() < 1.0e-4,
            "the zombie's model lifts a fifth on a sneak: {sneak_y} against {walk_y}"
        );
        // The chain's whole shift is the model's own lift for the mob (-0.2) and the lift
        // plus the player's eighth for the player (-0.3125).
        let mob_shift = sneak.w_axis.y - walk_zombie.w_axis.y;
        assert!(
            (mob_shift - (-0.2)).abs() < 1.0e-4,
            "the mob's chain shifts by the lift alone: {mob_shift}"
        );
        // The pig's model carries no sneak term at all.
        let mut sneak_pig = mob_draw(ModelRef::Pig { saddle: false });
        sneak_pig.pose.sneak = true;
        let walk_pig = body_chain(&mob_draw(ModelRef::Pig { saddle: false }));
        let pig_y = body_chain(&sneak_pig).transform_point3(sample).y;
        let pig_walk_y = walk_pig.transform_point3(sample).y;
        assert!(
            (pig_y - pig_walk_y).abs() < 1.0e-6,
            "the quadruped's model takes no sneak term: {pig_y} against {pig_walk_y}"
        );
        // And the player's own drop still lands on its chain, an eighth below the mob's
        // own shift.
        let mut sneak_player = player_draw();
        sneak_player.pose.sneak = true;
        let sneak_chain = body_chain(&sneak_player);
        let walk_chain = body_chain(&player_draw());
        let player_shift = sneak_chain.w_axis.y - walk_chain.w_axis.y;
        assert!(
            (player_shift - (-0.3125)).abs() < 1.0e-4,
            "the player's chain shifts by the lift and the drop: {player_shift}"
        );
        assert!(
            (player_shift - -(0.2 * 0.9375 + 0.125)).abs() < 1.0e-4,
            "the player's shift is its scaled lift plus the eighth: {player_shift}"
        );
    }

    #[test]
    fn the_cube_kinds_scale_by_their_squash_pair() {
        // At rest the pair collapses to the size: a four-cube's chain holds fourfold on
        // every axis (`RenderSlime.preRenderCallback`:34-37).
        let rest = body_chain(&mob_draw(ModelRef::Slime { size: 4 }));
        assert!((rest.x_axis.truncate().length() - 4.0 / 16.0).abs() < 1.0e-5);
        assert!((rest.y_axis.truncate().length() - 4.0 / 16.0).abs() < 1.0e-5);
        assert!((rest.z_axis.truncate().length() - 4.0 / 16.0).abs() < 1.0e-5);
        // Fully squashed, the slime's y stretches and x/z flatten by the renderer's own
        // fold, off the squish its draw carries.
        let squished = EntityDraw {
            extra: DrawExtra::Slime {
                size: 1,
                squish: 1.0,
            },
            ..mob_draw(ModelRef::Slime { size: 1 })
        };
        let chain = body_chain(&squished);
        let f1 = 1.0 / (1.0 * 0.5 + 1.0);
        let f2 = 1.0 / (f1 + 1.0);
        assert!((chain.x_axis.truncate().length() - f2 * 1.0 / 16.0).abs() < 1.0e-5);
        assert!((chain.y_axis.truncate().length() - (1.0 / f2) * 1.0 / 16.0).abs() < 1.0e-5);
        // The magma cube's squish rides its pose's own variant, over the same fold
        // (`RenderMagmaCube.preRenderCallback`:31-35).
        let magma = EntityDraw {
            pose: entity_models::Pose {
                extra: PoseExtra::MagmaCube { squish: 1.0 },
                ..entity_models::Pose::default()
            },
            ..mob_draw(ModelRef::MagmaCube { size: 3 })
        };
        let chain = body_chain(&magma);
        let g1 = 1.0 / (3.0 * 0.5 + 1.0);
        let g2 = 1.0 / (g1 + 1.0);
        assert!((chain.x_axis.truncate().length() - g2 * 3.0 / 16.0).abs() < 1.0e-5);
        assert!((chain.y_axis.truncate().length() - (1.0 / g2) * 3.0 / 16.0).abs() < 1.0e-5);
    }

    #[test]
    fn the_bat_bob_and_hang_shift_the_chain() {
        // The flying bat's chain sits on the age wave (`RenderBat.rotateCorpse`:39): between
        // two ages of the same draw, the difference is the wave.
        let bat_at = |age: f32, hanging: bool| EntityDraw {
            pose: entity_models::Pose {
                age,
                ..entity_models::Pose::default()
            },
            ..mob_draw(ModelRef::Bat { hanging })
        };
        let step =
            body_chain(&bat_at(60.0, false)).w_axis.y - body_chain(&bat_at(61.0, false)).w_axis.y;
        let wave = ((60.0_f32 * 0.3).cos() - (61.0_f32 * 0.3).cos()) * 0.1;
        assert!(
            (step - wave).abs() < 1.0e-5,
            "the flying bob is the age wave: {step} against {wave}"
        );
        // Hanging sits an eighth of a block low (`:43`) and ignores the age: two ages agree,
        // and the hanging chain is the flying one less the wave and the eighth.
        let hang_60 = body_chain(&bat_at(60.0, true)).w_axis.y;
        let hang_61 = body_chain(&bat_at(61.0, true)).w_axis.y;
        assert!(
            (hang_60 - hang_61).abs() < 1.0e-6,
            "the hang ignores the age: {hang_60} against {hang_61}"
        );
        let flying_60 = body_chain(&bat_at(60.0, false)).w_axis.y;
        let between = hang_60 - flying_60;
        assert!(
            (between - (-0.1 - (60.0_f32 * 0.3).cos() * 0.1)).abs() < 1.0e-5,
            "the hang shift against the flying draw: {between}"
        );
    }
}
