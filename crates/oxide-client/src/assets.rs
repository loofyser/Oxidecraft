//! The client's asset bootstrap: the store's extraction tree, loaded once at startup.
//!
//! The chain is the launcher's own resolution rule — the store root is the argument, else
//! `OXIDECRAFT_STORE`, else `<data dir>/oxidecraft` (`oxide-launcher/src/main.rs`) — then
//! [`Store::open`], the `1.8.9` extraction root [`Store::open`]'s version writes to, and from
//! that root the whole texture tree, the block models, the stitched atlas, the two colour maps
//! and the ascii font.
//!
//! The `1.8.9` extraction root must already exist; a client pointed at an empty store fails
//! with the `oxide-launcher fetch` command in the message rather than drawing the asset-less
//! fallback. Everything else the tree holds is read through the modules that own those rules:
//! a malformed PNG, blockstate or model is their error, never a silent default.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use oxide_assets::atlas::{Atlas, AtlasError, build_atlas};
use oxide_assets::extract::Extractor;
use oxide_assets::font::{Font, FontError};
use oxide_assets::model::{BakedModel, BakedQuad, ModelError, ModelSource, Transform};
use oxide_assets::resources::{GUI_SHEETS, ResourceError, TextureSet};
use oxide_assets::store::{Store, StoreError};
use oxide_assets::texture::Texture;
use oxide_client::items::{self, ItemModel};
use oxide_game::chat::LanguageTable;
use oxide_game::mesher::{BlockModelSet, ModelChoice};
use oxide_game::session::MeshAssets;
use oxide_render::entity_models::{Vertices, objects};
use oxide_render::entity_pass::{ItemMesh, ItemMeshSource};
use oxide_render::gui_item::{IconShape, ItemIconMesh, ItemIconSource};
use oxide_render::sky::SkyTextures;
use oxide_world::biome::{ColorMap, ColorMapError, TintMaps};

/// The version whose extraction tree the client loads.
const VERSION: &str = "1.8.9";

/// The environment variable that names the store root, when no argument is given: the
/// launcher's own override.
pub const STORE_VAR: &str = "OXIDECRAFT_STORE";

/// The mip count the atlas is stitched with: the client's default setting of 4.
const ATLAS_MIP_LEVELS: u32 = 4;

/// The grass colour map's texture path.
const GRASS_COLORMAP: &str = "colormap/grass";
/// The foliage colour map's texture path.
const FOLIAGE_COLORMAP: &str = "colormap/foliage";
/// The ascii font sheet's texture path.
const FONT_SHEET: &str = "font/ascii";
/// The language file's path below the extraction root: the client's own language
/// table, which the chat path resolves translation components against.
const LANG_FILE: &str = "assets/minecraft/lang/en_US.lang";
/// The sun quad's texture path.
const SUN: &str = "environment/sun";
/// The moon phase sheet's texture path.
const MOON_PHASES: &str = "environment/moon_phases";
/// The cloud layer's texture path.
const CLOUDS: &str = "environment/clouds";

/// The entity textures the window uploads once at startup: the entity pass's static
/// set, keyed by its own names.
///
/// The set is the shadow sprite, the two default skins and the mob sheets the kinds of
/// this milestone draw — the biped family (with the zombie pigman's sheet,
/// `RenderPigZombie.java`:11), the core quadrupeds, the crawler families and the exotic
/// families, each key the class renderer's own resource location and the layers'
/// sheets beside theirs.
///
/// The keys are the renderers' own, which are not always the obvious names: the giant
/// binds `textures/entity/zombie/zombie.png`, the zombie's own sheet
/// (`RenderGiantZombie.java`:13), the golem binds `textures/entity/iron_golem.png`, no
/// subdirectory (`RenderIronGolem.java`:11), the wool layer binds
/// `textures/entity/sheep/sheep_fur.png` (`LayerSheepWool.java`:12), and the eyes
/// layers bind sheets of their own, `textures/entity/spider_eyes.png` and
/// `textures/entity/enderman/enderman_eyes.png` (`LayerSpiderEyes.java`:11,
/// `LayerEndermanEyes.java`:11).
pub const ENTITY_TEXTURES: [&str; 77] = [
    "misc/shadow.png",
    "entity/steve.png",
    "entity/alex.png",
    "entity/zombie/zombie.png",
    "entity/zombie_pigman.png",
    "entity/zombie/zombie_villager.png",
    "entity/skeleton/skeleton.png",
    "entity/villager/villager.png",
    "entity/villager/farmer.png",
    "entity/villager/librarian.png",
    "entity/villager/priest.png",
    "entity/villager/smith.png",
    "entity/villager/butcher.png",
    "entity/witch.png",
    "entity/snowman.png",
    "entity/iron_golem.png",
    "entity/pig/pig.png",
    "entity/pig/pig_saddle.png",
    "entity/cow/cow.png",
    "entity/cow/mooshroom.png",
    "entity/sheep/sheep.png",
    "entity/sheep/sheep_fur.png",
    "entity/creeper/creeper.png",
    "entity/spider/spider.png",
    "entity/spider_eyes.png",
    "entity/spider/cave_spider.png",
    "entity/enderman/enderman.png",
    "entity/enderman/enderman_eyes.png",
    "entity/chicken.png",
    "entity/squid.png",
    "entity/slime/slime.png",
    "entity/slime/magmacube.png",
    "entity/bat.png",
    "entity/silverfish.png",
    "entity/endermite.png",
    // The exotic families: the horse's type, colour, marking and armour tables
    // (`RenderHorse.getEntityTexture`:51-78, `EntityHorse.java`:53-58), the wolf's
    // states and the collar sheet (`RenderWolf.getEntityTexture`:46-49,
    // `LayerWolfCollar.java`:12), the cat coats (`RenderOcelot.getEntityTexture`:23-40),
    // the rabbit coats with the toast and killer entries
    // (`RenderRabbit.getEntityTexture`:27-62), the ghast's shooting sheet
    // (`RenderGhast.getEntityTexture`:21-24), the blaze's, the guardian pair
    // (`RenderGuardian.getEntityTexture`:177-180), the dragon's
    // (`RenderDragon.getEntityTexture`:150-153) and the wither's spawn-shield pair
    // (`RenderWither.getEntityTexture`:33-37).
    "entity/horse/horse_white.png",
    "entity/horse/horse_creamy.png",
    "entity/horse/horse_chestnut.png",
    "entity/horse/horse_brown.png",
    "entity/horse/horse_black.png",
    "entity/horse/horse_gray.png",
    "entity/horse/horse_darkbrown.png",
    "entity/horse/donkey.png",
    "entity/horse/mule.png",
    "entity/horse/horse_zombie.png",
    "entity/horse/horse_skeleton.png",
    "entity/horse/horse_markings_white.png",
    "entity/horse/horse_markings_whitefield.png",
    "entity/horse/horse_markings_whitedots.png",
    "entity/horse/horse_markings_blackdots.png",
    "entity/horse/armor/horse_armor_iron.png",
    "entity/horse/armor/horse_armor_gold.png",
    "entity/horse/armor/horse_armor_diamond.png",
    "entity/wolf/wolf.png",
    "entity/wolf/wolf_tame.png",
    "entity/wolf/wolf_angry.png",
    "entity/wolf/wolf_collar.png",
    "entity/cat/ocelot.png",
    "entity/cat/black.png",
    "entity/cat/red.png",
    "entity/cat/siamese.png",
    "entity/rabbit/brown.png",
    "entity/rabbit/white.png",
    "entity/rabbit/black.png",
    "entity/rabbit/white_splotched.png",
    "entity/rabbit/gold.png",
    "entity/rabbit/salt.png",
    "entity/rabbit/toast.png",
    "entity/rabbit/caerbannog.png",
    "entity/ghast/ghast.png",
    "entity/ghast/ghast_shooting.png",
    "entity/blaze.png",
    "entity/guardian.png",
    "entity/guardian_elder.png",
    "entity/enderdragon/dragon.png",
    "entity/wither/wither.png",
    "entity/wither/wither_invulnerable.png",
];

/// The wide default skin's key, an entry of [`ENTITY_TEXTURES`].
pub const DEFAULT_SKIN_WIDE: &str = "entity/steve.png";

/// The slim default skin's key, an entry of [`ENTITY_TEXTURES`].
pub const DEFAULT_SKIN_SLIM: &str = "entity/alex.png";

/// The hud icon sheet's key, the extraction tree's `gui/icons.png`: the tab list's
/// latency bars and heart glyphs sample it under the name every draw of it carries.
pub const HUD_ICONS: &str = "gui/icons";

/// The enchanted glint's sheet, the extraction tree's `misc/enchanted_item_glint.png`:
/// the glint passes of an enchanted icon sample it, the source's own
/// `RenderItem.RES_ITEM_GLINT` file (`RenderItem.java`:63).
pub const GLINT_SHEET: &str = "misc/enchanted_item_glint";

/// The item frame's own wood: the blockstate file and the variant key the frame's
/// model resource names (`RenderItemFrame.java`:37's `("item_frame", "normal")`).
const ITEM_FRAME_STATE: (&str, &str) = ("item_frame", "normal");

/// The blocks texture the baked-model item meshes sample: the client's own name for
/// the stitched atlas (`TextureMap.java`:30's `textures/atlas/blocks.png`).
pub const BLOCKS_ATLAS_TEXTURE: &str = "textures/atlas/blocks.png";

/// The object set's own sheets: every key the object draws, the generated item shapes
/// and the icon quads sample.
///
/// The keys are the names the draws and the shapes register and resolve under: the
/// object geometries' own files (`objects::ARROW_TEXTURE` and its siblings), and the
/// item sheets under the item table's asset paths (`items.rs`'s sprite entries), which
/// the generated shape's key and the icon quad's key carry unchanged.
pub const OBJECT_TEXTURES: [&str; 23] = [
    "entity/arrow.png",
    "entity/boat.png",
    "entity/experience_orb.png",
    "entity/minecart.png",
    "painting/paintings_kristoffer_zetterstrand.png",
    "items/apple",
    "items/arrow",
    "items/bow_standby",
    "items/coal",
    "items/diamond",
    "items/diamond_sword",
    "items/egg",
    "items/ender_eye",
    "items/ender_pearl",
    "items/experience_bottle",
    "items/fireball",
    "items/fireworks",
    "items/gold_ingot",
    "items/iron_ingot",
    "items/iron_sword",
    "items/potion_bottle_drinkable",
    "items/snowball",
    "items/stick",
];

/// Errors from loading the client's assets.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    /// No store root could be resolved: no argument, no environment variable, and no data
    /// directory on this platform.
    #[error(
        "no store root was given, {STORE_VAR} is unset, and this platform has no data directory"
    )]
    NoStoreRoot,
    /// The extraction root the version names does not exist.
    #[error(
        "the extraction tree {root} does not exist; run `oxide-launcher fetch` first to \
         download and extract the assets"
    )]
    MissingTree {
        /// The extraction root that is missing.
        root: PathBuf,
    },
    /// A texture the bootstrap needs is not in the tree.
    #[error("the texture {key} is not in the extraction tree")]
    MissingTexture {
        /// The texture's resource path.
        key: &'static str,
    },
    /// The store could not be opened.
    #[error("the store could not be opened: {0}")]
    Store(#[from] StoreError),
    /// The texture tree could not be loaded.
    #[error("the texture tree could not be loaded: {0}")]
    Resources(#[from] ResourceError),
    /// The block models could not be loaded.
    #[error("the block models could not be loaded: {0}")]
    Model(#[from] ModelError),
    /// The atlas could not be stitched.
    #[error("the atlas could not be built: {0}")]
    Atlas(#[from] AtlasError),
    /// A colour map's bytes are not a 256x256 RGBA image.
    #[error("the colour map {key} could not be decoded: {source}")]
    ColourMap {
        /// The colour map's resource path.
        key: &'static str,
        /// The decoding error.
        source: ColorMapError,
    },
    /// The ascii font sheet could not be measured.
    #[error("the font sheet could not be measured: {0}")]
    Font(#[from] FontError),
    /// The language file could not be read.
    #[error("the language file {path} could not be read: {source}")]
    Lang {
        /// The file's path.
        path: PathBuf,
        /// The read error.
        source: std::io::Error,
    },
}

/// The assets one client run loads: what the session meshes with, the overlay's font,
/// the sky's textures, and the entity set.
#[derive(Debug)]
pub struct ClientAssets {
    /// The atlas, the baked block models and the tint maps.
    ///
    /// They travel in one `Arc` because `BlockModelSet` is not `Clone`, and the session and
    /// the renderer both read them.
    pub mesh: Arc<MeshAssets>,
    /// The ascii font measured from the sheet.
    pub font: Font,
    /// The language table, from the extraction tree's `en_US.lang`: the chat path
    /// resolves translation components against it.
    pub lang: LanguageTable,
    /// The ascii font sheet itself, for the overlay's GPU upload.
    pub sheet: Texture,
    /// The sun, the moon phase sheet and the cloud layer.
    pub sky_textures: SkyTextures,
    /// The entity textures, keyed by the pass's names.
    pub entity_textures: Vec<(&'static str, Texture)>,
    /// The object set's own sheets, keyed as the object draws name them.
    pub object_textures: Vec<(&'static str, Texture)>,
    /// The blocks atlas's level-0 image, under [`BLOCKS_ATLAS_TEXTURE`]: the sheet the
    /// block-item meshes sample.
    pub blocks_atlas: Texture,
    /// The wide default skin, from the entity set.
    pub skin_wide: Texture,
    /// The slim default skin, from the entity set.
    pub skin_slim: Texture,
    /// The hud's icon sheet, under [`HUD_ICONS`]: the tab list's latency bars and
    /// heart glyphs sample it.
    pub hud_icons: Texture,
    /// The enchanted glint's sheet, under [`GLINT_SHEET`]: the glint passes of an
    /// enchanted icon sample it.
    pub glint_sheet: Texture,
    /// The GUI sheets, under their own store keys ([`GUI_SHEETS`]): the widgets
    /// sheet, the container family, the two book sheets, the SGA glyph sheet and
    /// the chest trio's icon sheets — the keys the screens and the chest item
    /// model name them.
    pub gui_sheets: Vec<(&'static str, Texture)>,
    /// The object draws' item mesh source: the baked block models, the sheets and the
    /// frame's wood.
    pub item_meshes: ClientItemMeshes,
    /// The hud's item icon source: every item row's baked icon.
    pub item_icons: ClientItemIcons,
}

/// The item registry's atlas sprite paths: every generated item's own layer sheets,
/// deduplicated and sorted — the item set the atlas stitches beside the block model
/// set (the survey's §1.4 decision: one atlas holds blocks and items).
///
/// The chest trio's icon sheets resolve through [`ItemModel::Builtin`] and are
/// registered as their own textures ([`GUI_SHEETS`]), not stitched.
fn item_sprite_paths() -> BTreeSet<String> {
    items::registry()
        .iter()
        .filter_map(|entry| match entry.resolution {
            ItemModel::Generated(layers) => Some(layers),
            _ => None,
        })
        .flatten()
        .map(|layer| (*layer).to_string())
        .collect()
}

impl ClientAssets {
    /// Loads the whole chain from the store's extraction tree.
    ///
    /// The root is the argument when given, else [`STORE_VAR`], else `<data dir>/oxidecraft`
    /// — the launcher's own rule. `<root>/extracted/1.8.9` must exist; a missing tree is
    /// [`AssetError::MissingTree`] and names the fetch command.
    pub fn load(store_root: Option<PathBuf>) -> Result<ClientAssets, AssetError> {
        let root = match store_root {
            Some(root) => root,
            None => default_store_root()?,
        };
        let store = Store::open(root)?;
        let tree = Extractor::new(&store, VERSION).root();
        if !tree.is_dir() {
            return Err(AssetError::MissingTree { root: tree });
        }

        let textures = TextureSet::load(&tree)?;
        let models = ModelSource::open(&tree)?;
        // The item registry's sprite list, beside the block model set: one atlas
        // holds the blocks and the items (the survey's §1.4 decision).
        let item_sprites = item_sprite_paths();
        let atlas = build_atlas(
            &textures,
            &models.texture_paths(),
            &item_sprites,
            ATLAS_MIP_LEVELS,
        )?;
        let block_models = BlockModelSet::load(&models);
        let tint_maps = TintMaps {
            grass: colormap(&textures, GRASS_COLORMAP)?,
            foliage: colormap(&textures, FOLIAGE_COLORMAP)?,
        };
        let sheet = texture(&textures, FONT_SHEET)?.clone();
        let font = Font::load(&sheet, None)?;
        let lang = LanguageTable::from_lang(
            &std::fs::read_to_string(tree.join(LANG_FILE)).map_err(|source| AssetError::Lang {
                path: tree.join(LANG_FILE),
                source,
            })?,
        );
        let sky_textures = SkyTextures {
            sun: texture(&textures, SUN)?.clone(),
            moon_phases: texture(&textures, MOON_PHASES)?.clone(),
            clouds: texture(&textures, CLOUDS)?.clone(),
        };
        // The entity set: the static list's keys, and the two default skins among
        // them, each refused (by name) rather than defaulted when the tree lacks it.
        let mut entity_textures = Vec::with_capacity(ENTITY_TEXTURES.len());
        for key in ENTITY_TEXTURES {
            entity_textures.push((key, texture(&textures, key)?.clone()));
        }
        let skin_wide = texture(&textures, DEFAULT_SKIN_WIDE)?.clone();
        let skin_slim = texture(&textures, DEFAULT_SKIN_SLIM)?.clone();

        // The object set: the sheets the object draws sample — the geometries' files
        // and the item table's sprite paths — loaded under the keys the draws and the
        // shapes name.
        let mut object_textures = Vec::with_capacity(OBJECT_TEXTURES.len());
        for key in OBJECT_TEXTURES {
            object_textures.push((key, texture(&textures, key)?.clone()));
        }
        // The hud's icon sheet: the tab list's latency bars and heart glyphs sample
        // it under the name every draw of it carries.
        let hud_icons = texture(&textures, HUD_ICONS)?.clone();
        // The enchanted glint's sheet: the glint passes of an enchanted icon sample
        // it, under the name the hud's glint registration carries.
        let glint_sheet = texture(&textures, GLINT_SHEET)?.clone();
        // The GUI sheets: the widgets, the container family, the book sheets, the
        // SGA glyph sheet and the chest trio's icon sheets, under the keys the
        // screens and the chest item model name them.
        let mut gui_sheets = Vec::with_capacity(GUI_SHEETS.len());
        for key in GUI_SHEETS {
            gui_sheets.push((key, texture(&textures, key)?.clone()));
        }
        // The blocks atlas's level-0 image, under the name the block-item meshes
        // sample; and the item frame's wood, baked from the tree's own model
        // (`RenderItemFrame.java`:37).
        let blocks_atlas = Texture {
            width: atlas.width,
            height: atlas.height,
            rgba: atlas.levels[0].rgba.clone(),
        };
        let frame = bake_item_frame(&models, &atlas);
        let mesh = Arc::new(MeshAssets {
            models: block_models,
            atlas,
            tint_maps,
        });
        let item_meshes = ClientItemMeshes {
            mesh: Arc::clone(&mesh),
            sheets: object_textures.clone(),
            frame,
        };
        // The hud's item icons: every item row's icon, baked once against the same
        // models and atlas the object set reads.
        let item_icons = ClientItemIcons::load(&models, &mesh);

        tracing::info!(
            atlas_width = mesh.atlas.width,
            atlas_height = mesh.atlas.height,
            atlas_levels = mesh.atlas.level_count,
            atlas_sprites = mesh.atlas.sprites.len(),
            item_sprites = item_sprites.len(),
            gui_sheets = gui_sheets.len(),
            sheet_width = sheet.width,
            sheet_height = sheet.height,
            "the client assets were loaded"
        );
        Ok(ClientAssets {
            mesh,
            font,
            lang,
            sheet,
            sky_textures,
            entity_textures,
            object_textures,
            blocks_atlas,
            skin_wide,
            skin_slim,
            hud_icons,
            glint_sheet,
            gui_sheets,
            item_meshes,
            item_icons,
        })
    }
}

/// The item frame's own wood model, baked once: the frame's blockstate's `normal`
/// variant through the tree's own baker (`RenderItemFrame.java`:37, `:61-75`).
fn bake_item_frame(models: &ModelSource, atlas: &Atlas) -> Option<ItemMesh> {
    let states = models.blockstates(ITEM_FRAME_STATE.0).ok()?;
    let variant = states.variants.get(ITEM_FRAME_STATE.1)?.first()?;
    let baked = models.bake_variant(variant).ok()?;
    Some(ItemMesh {
        vertices: Arc::new(model_vertices(&baked, atlas)),
        texture: BLOCKS_ATLAS_TEXTURE,
    })
}

/// The object draws' mesh source: the client's baked block models, its stitched atlas
/// and the sheets the generated shapes read, behind the pass's own
/// [`ItemMeshSource`].
///
/// The pass names none of this: it asks for a sprite key, a block state, the frame's
/// wood or an icon sprite, and this resolves each against the same assets the terrain
/// and the entity set are built from.
#[derive(Debug, Clone)]
pub struct ClientItemMeshes {
    /// The session's mesh assets: the block models the terrain bakes, and the atlas.
    mesh: Arc<MeshAssets>,
    /// The item sheets, keyed as the draws name them.
    sheets: Vec<(&'static str, Texture)>,
    /// The item frame's wood, baked at load.
    frame: Option<ItemMesh>,
}

impl ItemMeshSource for ClientItemMeshes {
    /// The generated item shape of a sprite: its pixels through the item model
    /// generator's own scan (`ItemModelGenerator.java`:17-234), the quads in the
    /// sheet's own 0..1 space.
    fn generated(&self, key: &str) -> Option<ItemMesh> {
        let sheet = self.sheets.iter().find(|entry| entry.0 == key)?;
        let vertices = objects::generated_item(sheet.1.width, sheet.1.height, &sheet.1.rgba)?;
        Some(ItemMesh {
            vertices: Arc::new(vertices),
            texture: sheet.0,
        })
    }

    /// The baked mesh of a block state through the terrain's own model set. The origin
    /// stands in for a position — a dropped item has no cell of its own — and an
    /// unresolved state answers the mesher's own fallback cube.
    fn block_item(&self, block: u16, meta: u8) -> Option<ItemMesh> {
        match self.mesh.models.model(block, meta, 0, 0, 0) {
            ModelChoice::Model(model) => Some(ItemMesh {
                vertices: Arc::new(model_vertices(model, &self.mesh.atlas)),
                texture: BLOCKS_ATLAS_TEXTURE,
            }),
            ModelChoice::Missing => Some(ItemMesh {
                vertices: Arc::new(missing_cube(&self.mesh.atlas)),
                texture: BLOCKS_ATLAS_TEXTURE,
            }),
        }
    }

    /// The frame's own wood, baked at load.
    fn frame_wood(&self) -> Option<ItemMesh> {
        self.frame.clone()
    }

    /// The icon quad of a sprite: the source's own one-quad draw, the sprite as the
    /// whole sheet, the normal up (`RenderFireball.doRender`:46-51).
    fn icon_quad(&self, key: &str) -> Option<ItemMesh> {
        let sheet = self.sheets.iter().find(|entry| entry.0 == key)?;
        let mut vertices = Vertices::default();
        for ([x, y], [u, v]) in [
            ([-8.0, -4.0], [0.0, 1.0]),
            ([8.0, -4.0], [1.0, 1.0]),
            ([8.0, 12.0], [1.0, 0.0]),
            ([-8.0, 12.0], [0.0, 0.0]),
        ] {
            vertices.positions.push([x, y, 0.0]);
            vertices.uvs.push([u, v]);
            vertices.normals.push([0.0, 1.0, 0.0]);
        }
        Some(ItemMesh {
            vertices: Arc::new(vertices),
            texture: sheet.0,
        })
    }
}

/// The hud's item icon source: every item row's icon, baked once at load, behind the
/// pass's own [`ItemIconSource`].
///
/// Mirrors [`ClientItemMeshes`]'s shape: the pass asks for an id and a damage and this
/// resolves them against the item table (Task 8) and the item bake (Task 7) — a block
/// row's chain through [`ModelSource::bake_item`], a generated row's layer through the
/// atlas's stitched sprite, a folded chest trio row through [`objects::chest_item`].
/// The meshes are the entity pass's own 1/16-unit shapes, so the icon draw samples the
/// atlas the world draws sample; a row nothing resolves for answers `None` and the
/// draw falls back to [`ItemIconSource::missing_icon`].
#[derive(Debug, Clone)]
pub struct ClientItemIcons {
    /// One icon per item id, `None` for a row nothing resolves for.
    icons: Vec<Option<ItemIconMesh>>,
    /// The missing icon every unresolved stack draws: the missing model's own cube
    /// (`ModelBakery`'s `builtin/missing` elements, every face the whole sprite,
    /// `ModelBakery.java`:716) under the 3D branch — the missing model's own `gui3d`.
    missing: Option<ItemIconMesh>,
}

impl ClientItemIcons {
    /// Bakes every item row's icon against the loaded tree.
    ///
    /// The atlas and the item table are the two inputs; nothing here reads the store,
    /// so the build is pure over the loaded assets and fails soft: a row the bake
    /// refuses answers the missing icon rather than failing the client's load.
    fn load(models: &ModelSource, mesh: &Arc<MeshAssets>) -> ClientItemIcons {
        let max_id = items::registry()
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0);
        let mut icons = vec![None; max_id as usize + 1];
        for entry in items::registry() {
            icons[entry.id as usize] = match entry.resolution {
                ItemModel::Block(name) => {
                    // The item's own chain: the source's bake starts at the item
                    // file (`models/item/<name>.json`, `ModelBakery.getItemLocation`)
                    // and walks down into the block model, and the display slots
                    // live on the item file — the chain's transform lookup walks up
                    // from the item's own model (`ModelBlock.getTransform`:177-179).
                    // The table carries the chain's block member, so the bake starts
                    // at the item file above it; a member no item file parents
                    // bakes as the block model itself.
                    let item_file = models.item_model_above(name);
                    let baked = item_file
                        .as_deref()
                        .and_then(|resource| models.bake_item(resource).ok())
                        .or_else(|| models.bake_item(name).ok());
                    baked.map(|baked| ItemIconMesh {
                        mesh: ItemMesh {
                            vertices: Arc::new(quad_vertices(&baked.quads, &mesh.atlas)),
                            texture: BLOCKS_ATLAS_TEXTURE,
                        },
                        transform: baked.display.gui,
                        // The block item's own file states its first-person slot
                        // (`item/torch.json`'s own `display.firstperson`), the chain's
                        // completed lookup.
                        first_person: baked.display.first_person,
                        shape: IconShape::Gui3d,
                    })
                }
                ItemModel::Generated(layers) => layers.last().and_then(|layer| {
                    generated_icon_vertices(layer, &mesh.atlas).map(|vertices| ItemIconMesh {
                        mesh: ItemMesh {
                            vertices: Arc::new(vertices),
                            texture: BLOCKS_ATLAS_TEXTURE,
                        },
                        // No generated item model in the tree states a gui slot — the
                        // 26 that state one are block items or builtin/entity ids — so
                        // the chain's completed gui slot is the source's default (the
                        // store pass pins the representative chains).
                        transform: Transform::DEFAULT,
                        // The first-person slot is a different story: the generated
                        // items' own files state one (`item/diamond_sword.json`'s
                        // `display.firstperson`, the 45-degree item pose), and it is
                        // read the generator's way — the item's file sits over the
                        // sprites the row's layer list names (`item/wooden_sword`
                        // over `items/wood_sword`), so the file is resolved by
                        // matching that full layer list against the store's `item/`
                        // tree (`ModelSource::generated_item_file`; the layer0
                        // basename alone misses 86 of the 208 rows — 85 resolve no
                        // file at all). A row that resolves no file leaves the
                        // source's default, which `applyTransform` no-ops
                        // (`ItemCameraTransforms.java`:59).
                        first_person: models
                            .generated_item_file(layers)
                            .and_then(|file| models.bake_item(&file).ok())
                            .map(|baked| baked.display.first_person)
                            .unwrap_or(Transform::DEFAULT),
                        shape: IconShape::Flat,
                    })
                }),
                ItemModel::Builtin(item) => {
                    models
                        .bake_item(item.model_name())
                        .ok()
                        .map(|baked| ItemIconMesh {
                            mesh: ItemMesh {
                                vertices: Arc::new(objects::chest_item()),
                                texture: item.icon_sheet(),
                            },
                            transform: baked.display.gui,
                            // The folded chest trio states no display at all (its
                            // `item/chest.json` files carry none), so the slot is the
                            // source's default.
                            first_person: baked.display.first_person,
                            shape: IconShape::Builtin,
                        })
                }
                ItemModel::Missing => None,
            };
        }
        let missing = Some(ItemIconMesh {
            mesh: ItemMesh {
                vertices: Arc::new(missing_cube(&mesh.atlas)),
                texture: BLOCKS_ATLAS_TEXTURE,
            },
            transform: Transform::DEFAULT,
            first_person: Transform::DEFAULT,
            shape: IconShape::Gui3d,
        });
        ClientItemIcons { icons, missing }
    }
}

impl ItemIconSource for ClientItemIcons {
    /// The icon the row resolves.
    ///
    /// The damage is not consulted: the table's rows are per id, and the source's
    /// per-subtype model registrations (`RenderItem.registerItems`) are not folded
    /// (recorded, matching the entity pass's own `items::resolve`).
    fn icon(&self, id: i16, _damage: i16) -> Option<ItemIconMesh> {
        let index = usize::try_from(id).ok()?;
        self.icons.get(index).and_then(|icon| icon.clone())
    }

    /// The atlas's own missing sprite as the missing model draws it: the cube the
    /// fallback bakes, under the 3D branch.
    fn missing_icon(&self) -> Option<ItemIconMesh> {
        self.missing.clone()
    }
}

/// The generated item shape over one atlas sprite: the sprite's own pixels through the
/// item model generator's scan, its uvs mapped into the sprite's atlas rect — the icon
/// draw samples the atlas, so the shape lives in atlas space.
///
/// The sprite is the path the item table's layer names; a path the atlas never stitched
/// draws the missing sprite, the source's own fallback
/// (`TextureMap.getAtlasSprite`'s `missingno`).
fn generated_icon_vertices(key: &str, atlas: &Atlas) -> Option<Vertices> {
    let sprite = atlas.drawn(key);
    let rect = sprite.content;
    let (width, height) = (rect.w as usize, rect.h as usize);
    let stride = atlas.levels[0].width as usize * 4;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = (rect.y as usize + row) * stride + rect.x as usize * 4;
        rgba.extend_from_slice(atlas.levels[0].rgba.get(start..start + width * 4)?);
    }
    let mut mesh = objects::generated_item(rect.w, rect.h, &rgba)?;
    let [min, max] = atlas.uv(sprite);
    for uv in &mut mesh.uvs {
        uv[0] = min[0] + uv[0] * (max[0] - min[0]);
        uv[1] = min[1] + uv[1] * (max[1] - min[1]);
    }
    Some(mesh)
}

/// The mesher's own fallback for a state the model set did not resolve: the missing
/// sprite's cube (`objects::missing_block`), its uvs mapped onto the atlas's missing
/// sprite.
fn missing_cube(atlas: &Atlas) -> Vertices {
    let mut mesh = objects::missing_block();
    let [min, max] = atlas.uv(&atlas.missing);
    for uv in &mut mesh.uvs {
        uv[0] = min[0] + uv[0] * (max[0] - min[0]);
        uv[1] = min[1] + uv[1] * (max[1] - min[1]);
    }
    mesh
}

/// A baked model's quads as one entity vertex set: positions in 1/16 units, uvs mapped
/// into the atlas's drawn rects, each quad's own normal.
fn model_vertices(model: &BakedModel, atlas: &Atlas) -> Vertices {
    quad_vertices(&model.quads, atlas)
}

/// A quad list as one entity vertex set, the same mapping [`model_vertices`] applies.
fn quad_vertices(quads: &[BakedQuad], atlas: &Atlas) -> Vertices {
    let mut out = Vertices::default();
    for quad in quads {
        push_model_quad(&mut out, quad, atlas);
    }
    out
}

/// Pushes one baked quad: the mesher's own sprite mapping (`oxide-game`'s `atlas_uv`
/// rule), the corners scaled from the model's 0..1 block-local units.
fn push_model_quad(out: &mut Vertices, quad: &BakedQuad, atlas: &Atlas) {
    let [min, max] = atlas.uv(atlas.drawn(&quad.texture));
    let normal = quad_normal(&quad.corners);
    for index in 0..4 {
        let corner = quad.corners[index];
        out.positions
            .push([corner[0] * 16.0, corner[1] * 16.0, corner[2] * 16.0]);
        let uv = quad.uv[index];
        out.uvs.push([
            min[0] + uv[0] * (max[0] - min[0]),
            min[1] + uv[1] * (max[1] - min[1]),
        ]);
        out.normals.push(normal);
    }
}

/// The unit normal of a quad's first three corners; the up axis for a degenerate quad.
fn quad_normal(corners: &[[f32; 3]; 4]) -> [f32; 3] {
    let edge = |from: [f32; 3], to: [f32; 3]| [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let a = edge(corners[0], corners[1]);
    let b = edge(corners[1], corners[2]);
    let [x, y, z] = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = (x * x + y * y + z * z).sqrt();
    if length > 0.0 {
        [x / length, y / length, z / length]
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// The store root when no argument is given: [`STORE_VAR`] when it is set and not empty, else
/// the launcher's `<data dir>/oxidecraft`.
pub fn default_store_root() -> Result<PathBuf, AssetError> {
    if let Some(root) = std::env::var_os(STORE_VAR).filter(|root| !root.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    let base = dirs::data_dir().ok_or(AssetError::NoStoreRoot)?;
    Ok(base.join("oxidecraft"))
}

/// The extraction tree's key for a sheet the pass names by its file name.
///
/// The pass registers its sheets under the file names the models name them by
/// (`entity/steve.png`), while [`TextureSet`] keys the tree by the path below
/// `textures/` without the extension (`entity/steve`). The suffix is stripped here,
/// at the store's edge.
fn store_key(sheet: &str) -> &str {
    sheet.strip_suffix(".png").unwrap_or(sheet)
}

/// The texture a sheet's name reads, refused (by name) rather than defaulted.
fn texture<'a>(textures: &'a TextureSet, key: &'static str) -> Result<&'a Texture, AssetError> {
    let tree_key = store_key(key);
    textures
        .get(tree_key)
        .ok_or(AssetError::MissingTexture { key: tree_key })
}

/// The colour map a resource path names, decoded from its texture's RGBA bytes.
///
/// The bytes are row-major with the first row at the top, exactly the orientation
/// [`ColorMap::from_rgba`] and the biome tint lookup read.
fn colormap(textures: &TextureSet, key: &'static str) -> Result<ColorMap, AssetError> {
    let map = texture(textures, key)?;
    ColorMap::from_rgba(&map.rgba).map_err(|source| AssetError::ColourMap { key, source })
}

#[cfg(test)]
mod tests {
    //! The entity set's registry: the static list's own shape here, and the store's
    //! tree behind `-- --ignored`.

    use super::*;
    use oxide_render::entity_pass::ModelRef;

    #[test]
    fn the_entity_set_names_the_shadow_the_skins_and_the_mob_sheets() {
        // The pass's static set: the shadow sprite first, then the two defaults, under
        // the keys the resolver falls back to, then the mob sheets of this milestone's
        // kinds.
        assert_eq!(
            ENTITY_TEXTURES,
            [
                "misc/shadow.png",
                "entity/steve.png",
                "entity/alex.png",
                "entity/zombie/zombie.png",
                "entity/zombie_pigman.png",
                "entity/zombie/zombie_villager.png",
                "entity/skeleton/skeleton.png",
                "entity/villager/villager.png",
                "entity/villager/farmer.png",
                "entity/villager/librarian.png",
                "entity/villager/priest.png",
                "entity/villager/smith.png",
                "entity/villager/butcher.png",
                "entity/witch.png",
                "entity/snowman.png",
                "entity/iron_golem.png",
                "entity/pig/pig.png",
                "entity/pig/pig_saddle.png",
                "entity/cow/cow.png",
                "entity/cow/mooshroom.png",
                "entity/sheep/sheep.png",
                "entity/sheep/sheep_fur.png",
                "entity/creeper/creeper.png",
                "entity/spider/spider.png",
                "entity/spider_eyes.png",
                "entity/spider/cave_spider.png",
                "entity/enderman/enderman.png",
                "entity/enderman/enderman_eyes.png",
                "entity/chicken.png",
                "entity/squid.png",
                "entity/slime/slime.png",
                "entity/slime/magmacube.png",
                "entity/bat.png",
                "entity/silverfish.png",
                "entity/endermite.png",
                "entity/horse/horse_white.png",
                "entity/horse/horse_creamy.png",
                "entity/horse/horse_chestnut.png",
                "entity/horse/horse_brown.png",
                "entity/horse/horse_black.png",
                "entity/horse/horse_gray.png",
                "entity/horse/horse_darkbrown.png",
                "entity/horse/donkey.png",
                "entity/horse/mule.png",
                "entity/horse/horse_zombie.png",
                "entity/horse/horse_skeleton.png",
                "entity/horse/horse_markings_white.png",
                "entity/horse/horse_markings_whitefield.png",
                "entity/horse/horse_markings_whitedots.png",
                "entity/horse/horse_markings_blackdots.png",
                "entity/horse/armor/horse_armor_iron.png",
                "entity/horse/armor/horse_armor_gold.png",
                "entity/horse/armor/horse_armor_diamond.png",
                "entity/wolf/wolf.png",
                "entity/wolf/wolf_tame.png",
                "entity/wolf/wolf_angry.png",
                "entity/wolf/wolf_collar.png",
                "entity/cat/ocelot.png",
                "entity/cat/black.png",
                "entity/cat/red.png",
                "entity/cat/siamese.png",
                "entity/rabbit/brown.png",
                "entity/rabbit/white.png",
                "entity/rabbit/black.png",
                "entity/rabbit/white_splotched.png",
                "entity/rabbit/gold.png",
                "entity/rabbit/salt.png",
                "entity/rabbit/toast.png",
                "entity/rabbit/caerbannog.png",
                "entity/ghast/ghast.png",
                "entity/ghast/ghast_shooting.png",
                "entity/blaze.png",
                "entity/guardian.png",
                "entity/guardian_elder.png",
                "entity/enderdragon/dragon.png",
                "entity/wither/wither.png",
                "entity/wither/wither_invulnerable.png",
            ]
        );
        assert!(ENTITY_TEXTURES.contains(&DEFAULT_SKIN_WIDE));
        assert!(ENTITY_TEXTURES.contains(&DEFAULT_SKIN_SLIM));
    }

    #[test]
    fn every_kinds_sheets_are_in_the_static_entity_set() {
        // Every kind this milestone draws resolves through the pass's named keys: each of
        // the model's own sheets, layers included, must be in the window's static set.
        let kinds = [
            ModelRef::Zombie,
            ModelRef::ZombieVillager,
            ModelRef::Skeleton,
            ModelRef::Witch,
            ModelRef::Giant,
            ModelRef::SnowGolem,
            ModelRef::IronGolem,
            ModelRef::Cow,
            ModelRef::Mooshroom,
            ModelRef::Pig { saddle: true },
            ModelRef::Sheep {
                wool: 14,
                sheared: false,
            },
            ModelRef::Creeper,
            ModelRef::Spider,
            ModelRef::CaveSpider,
            ModelRef::Enderman,
            ModelRef::Chicken { child: false },
            ModelRef::Squid,
            ModelRef::Slime { size: 1 },
            ModelRef::MagmaCube { size: 1 },
            ModelRef::Bat { hanging: false },
            ModelRef::Silverfish,
            ModelRef::EnderMite,
            ModelRef::Horse {
                variant: 0,
                colour: 0,
                markings: 1,
                saddle: true,
                armour: 2,
            },
            ModelRef::Wolf {
                tamed: true,
                collar: 12,
                angry: false,
            },
            ModelRef::Ocelot {
                variant: 0,
                child: false,
                tamed: false,
            },
            ModelRef::Rabbit {
                variant: 0,
                child: false,
            },
            ModelRef::Ghast { shooting: true },
            ModelRef::Blaze,
            ModelRef::Guardian { elder: true },
            ModelRef::EnderDragon,
            ModelRef::Wither { invul_time: 0 },
        ];
        for reference in kinds {
            for key in oxide_render::entity_models::textures(reference) {
                assert!(
                    ENTITY_TEXTURES.contains(key),
                    "{reference:?} draws {key}, which the static entity set is missing"
                );
            }
        }
        // The villager's five profession sheets too.
        for profession in 0..5 {
            for key in oxide_render::entity_models::textures(ModelRef::Villager {
                profession,
                child: false,
            }) {
                assert!(
                    ENTITY_TEXTURES.contains(key),
                    "the villager's profession {profession} draws {key}"
                );
            }
        }
    }

    /// Every key the static entity set names exists in the store's extraction tree.
    ///
    /// The store root is required: `OXIDECRAFT_STORE` must name it, as the other
    /// real-tree tests require. Run with `-- --ignored`.
    #[test]
    #[ignore = "reads the asset store; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_entity_set_exists_in_the_store() {
        let root = std::env::var_os(STORE_VAR)
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let store = Store::open(PathBuf::from(root)).expect("the store opens");
        let tree = Extractor::new(&store, VERSION).root();
        let textures = TextureSet::load(&tree).expect("the texture tree loads");
        for key in ENTITY_TEXTURES {
            assert!(
                textures.get(store_key(key)).is_some(),
                "the entity texture {key} is in the store"
            );
        }
    }

    /// The object set's registry: every key the object draws, the generated item
    /// shapes and the icon quads sample, both key forms pinned — the geometries' own
    /// files and the item table's asset paths.
    #[test]
    fn the_object_set_names_the_sheets_the_object_draws_sample() {
        assert_eq!(
            OBJECT_TEXTURES,
            [
                "entity/arrow.png",
                "entity/boat.png",
                "entity/experience_orb.png",
                "entity/minecart.png",
                "painting/paintings_kristoffer_zetterstrand.png",
                "items/apple",
                "items/arrow",
                "items/bow_standby",
                "items/coal",
                "items/diamond",
                "items/diamond_sword",
                "items/egg",
                "items/ender_eye",
                "items/ender_pearl",
                "items/experience_bottle",
                "items/fireball",
                "items/fireworks",
                "items/gold_ingot",
                "items/iron_ingot",
                "items/iron_sword",
                "items/potion_bottle_drinkable",
                "items/snowball",
                "items/stick",
            ]
        );
        // The object geometries' own keys, as `objects.rs` names them.
        for key in [
            objects::ARROW_TEXTURE,
            objects::BOAT_TEXTURE,
            objects::MINECART_TEXTURE,
            objects::ORB_TEXTURE,
            objects::PAINTING_TEXTURE,
        ] {
            assert!(OBJECT_TEXTURES.contains(&key), "the object sheet {key}");
        }
        // And every sprite an item shape or an icon quad can name.
        for key in [
            "items/stick",
            "items/apple",
            "items/snowball",
            "items/egg",
            "items/ender_pearl",
            "items/ender_eye",
            "items/potion_bottle_drinkable",
            "items/experience_bottle",
            "items/fireworks",
            "items/fireball",
        ] {
            assert!(OBJECT_TEXTURES.contains(&key), "the item sheet {key}");
        }
    }

    /// Every key the object set names exists in the store's extraction tree, and the
    /// frame's blockstate the wood bakes from is there too.
    #[test]
    #[ignore = "reads the asset store; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_object_set_exists_in_the_store() {
        let root = std::env::var_os(STORE_VAR)
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let store = Store::open(PathBuf::from(root)).expect("the store opens");
        let tree = Extractor::new(&store, VERSION).root();
        let textures = TextureSet::load(&tree).expect("the texture tree loads");
        for key in OBJECT_TEXTURES {
            assert!(
                textures.get(store_key(key)).is_some(),
                "the object texture {key} is in the store"
            );
        }
        let models = ModelSource::open(&tree).expect("the block models load");
        assert!(
            models.blockstates(ITEM_FRAME_STATE.0).is_ok(),
            "the frame's blockstate file is in the store"
        );
    }

    /// The item sprite set is the registry's own generated layers: every generated
    /// item's sheet paths, deduplicated and sorted — the `items/` sheets and the
    /// block sheets the generated items sample (the torch, the rails, the plants)
    /// — and nothing else: the block model set's own sheets stay out, and the
    /// chest trio's icon sheets resolve through their own class and are registered
    /// as textures, not stitched.
    #[test]
    fn the_item_sprite_set_is_the_registrys_generated_layers() {
        let sprites = item_sprite_paths();
        // A generated item's own top sheet is in it.
        assert!(sprites.contains("items/apple"));
        assert!(sprites.contains("items/diamond_sword"));
        assert!(sprites.contains("items/potion_bottle_drinkable"));
        // The block sheets the generated items sample travel in the set too: the
        // torch, the rails and the plants are block textures.
        assert!(sprites.contains("blocks/torch_on"));
        assert!(sprites.contains("blocks/rail_golden"));
        assert!(sprites.contains("blocks/flower_rose"));
        // The block model set's own sheets are not the item set's, and the chest
        // trio's icon sheets are not stitched.
        assert!(!sprites.contains("blocks/stone"));
        assert!(!sprites.contains("entity/chest/normal"));
        // Every entry is a sheet of the tree, sorted by the set.
        assert_eq!(sprites.len(), 214, "the registry's distinct layer sheets");
        let mut sorted: Vec<&String> = sprites.iter().collect();
        sorted.sort();
        assert_eq!(sorted, sprites.iter().collect::<Vec<_>>());
    }

    /// The item sprites and the GUI sheets load from the store's own tree: the
    /// atlas carries the registry's sprites beside the block set, and every GUI
    /// sheet loads under its own key — one real container sheet among them.
    #[test]
    #[ignore = "reads the asset store; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_item_sprites_and_gui_sheets_load_from_the_store() {
        let root = std::env::var_os(STORE_VAR)
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let assets =
            ClientAssets::load(Some(PathBuf::from(&root))).expect("the client assets load");
        let store = Store::open(PathBuf::from(&root)).expect("the store opens");
        let tree = Extractor::new(&store, VERSION).root();
        let models = ModelSource::open(&tree).expect("the block models load");
        let requested = models.texture_paths();

        // A real item sprite is stitched into the atlas beside the block set, at
        // its own size; the item set shares the block sheets the generated items
        // sample (the torch, the rails, the plants), so the count grows by the
        // item set's own new sprites plus the fallback.
        let item_sprites = item_sprite_paths();
        let apple = assets
            .mesh
            .atlas
            .sprites
            .get("items/apple")
            .expect("the apple's sheet is stitched");
        assert_eq!(
            (apple.content.w, apple.content.h),
            (16, 16),
            "the apple's own 16x16 pixels"
        );
        let shared = item_sprites
            .iter()
            .filter(|path| requested.contains(*path))
            .count();
        assert_eq!(
            shared, 22,
            "the generated items' block sheets are the block model set's own"
        );
        assert_eq!(
            assets.mesh.atlas.sprites.len(),
            requested.len() + item_sprites.len() - shared + 1,
            "the block set, the item set's new sprites and the fallback"
        );

        // Every GUI sheet loads under its own key, in the list's own order: the
        // widgets sheet, the container family, the two book sheets, the SGA glyph
        // sheet and the chest trio's icon sheets.
        assert_eq!(assets.gui_sheets.len(), GUI_SHEETS.len());
        for ((key, texture), expected) in assets.gui_sheets.iter().zip(GUI_SHEETS) {
            assert_eq!(*key, expected, "the sheets load in the list's own order");
            assert!(
                texture.width > 0 && texture.height > 0,
                "{key} is loaded with its own pixels"
            );
        }
        let generic = assets
            .gui_sheets
            .iter()
            .find(|(key, _)| *key == "gui/container/generic_54")
            .expect("the generic container frame is loaded");
        assert_eq!(
            (generic.1.width, generic.1.height),
            (256, 256),
            "the container frame's own canvas"
        );
        let widgets = assets
            .gui_sheets
            .iter()
            .find(|(key, _)| *key == "gui/widgets")
            .expect("the widgets sheet is loaded");
        assert_eq!((widgets.1.width, widgets.1.height), (256, 256));

        // The enchanted glint's sheet loads with its own pixels: the source's own
        // 64x64 `misc/enchanted_item_glint.png` (`RenderItem.java`:63).
        assert_eq!(
            (assets.glint_sheet.width, assets.glint_sheet.height),
            (64, 64),
            "the glint sheet's own canvas"
        );
    }

    /// The two atlas names the icon path couples: the resolver marks an atlas-mapped
    /// mesh with the client's registry name ([`BLOCKS_ATLAS_TEXTURE`]) and the hud
    /// pass's batch split compares it against its own
    /// [`oxide_render::gui_item::ATLAS_TEXTURE`] — the same registry entry
    /// (`TextureMap.java`:30); if the two ever diverge, an icon's quads batch under a
    /// named texture nothing registered instead of the atlas binding.
    #[test]
    fn the_atlas_names_are_one_key() {
        assert_eq!(BLOCKS_ATLAS_TEXTURE, oxide_render::gui_item::ATLAS_TEXTURE);
    }

    /// The live item icons: every class's icon builds from the store's own tree — the
    /// block bake, the generated scan over the atlas's stitched sprite, the chest
    /// trio's boxes — and the rows nothing resolves for stay `None`.
    #[test]
    #[ignore = "reads the asset store; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_item_icons_build_from_the_store() {
        let root = std::env::var_os(STORE_VAR)
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let assets =
            ClientAssets::load(Some(PathBuf::from(&root))).expect("the client assets load");
        let icons = &assets.item_icons;

        // A block item's icon: the chain's baked quads sampling the atlas, the GUI slot
        // the file states (item/stone.json states third-person only, so the source's
        // default applies), the 3D shape.
        let stone = icons.icon(1, 0).expect("stone's icon builds");
        assert_eq!(stone.mesh.texture, BLOCKS_ATLAS_TEXTURE);
        assert_eq!(stone.shape, IconShape::Gui3d);
        assert_eq!(stone.transform, Transform::DEFAULT);
        assert!(!stone.mesh.vertices.positions.is_empty());

        // A stated GUI slot rides through the chain: item/oak_stairs.json states
        // rotation (0, 180, 0) in its gui slot.
        let stairs = icons.icon(53, 0).expect("the stairs' icon builds");
        assert_eq!(stairs.shape, IconShape::Gui3d);
        assert_eq!(stairs.transform.rotation, [0.0, 180.0, 0.0]);

        // A generated item: the atlas sprite's own scan, the flat shape, the source's
        // default GUI slot (no generated item model states one). The shape is the same
        // scan the entity pass's own generated draw bakes — the same sprite pixels.
        let apple = icons.icon(260, 0).expect("the apple's icon builds");
        assert_eq!(apple.shape, IconShape::Flat);
        assert_eq!(apple.transform, Transform::DEFAULT);
        assert_eq!(apple.mesh.texture, BLOCKS_ATLAS_TEXTURE);
        let world = assets
            .item_meshes
            .generated("items/apple")
            .expect("the world's apple bakes");
        assert_eq!(
            apple.mesh.vertices.positions, world.vertices.positions,
            "the icon and the world's generated shape scan the same pixels"
        );

        // The chest trio: the chest model's boxes on the trio's own sheet, under the
        // builtin shape (the 3D branch plus the block-entity tail).
        let chest = icons.icon(54, 0).expect("the chest's icon builds");
        assert_eq!(chest.shape, IconShape::Builtin);
        assert_eq!(chest.mesh.texture, "entity/chest/normal");
        assert_eq!(
            chest.mesh.vertices.positions.len(),
            72,
            "three boxes, six faces each"
        );
        assert_eq!(chest.transform, Transform::DEFAULT);

        // The missing icon: the missing model's cube under the 3D branch, sampling the
        // atlas's own missing sprite.
        let missing = icons.missing_icon().expect("the missing icon builds");
        assert_eq!(missing.shape, IconShape::Gui3d);
        assert_eq!(missing.mesh.texture, BLOCKS_ATLAS_TEXTURE);
        assert_eq!(
            missing.mesh.vertices.positions.len(),
            24,
            "the cube's six faces"
        );

        // An id nothing resolves for answers `None`: below the item range and the
        // missing marker's own rows.
        assert!(icons.icon(0, 0).is_none());
        let skull = items::registry()
            .iter()
            .find(|entry| matches!(entry.resolution, ItemModel::Missing))
            .map(|entry| entry.id)
            .expect("a missing row is in the table");
        assert!(icons.icon(skull, 0).is_none());

        // The representative chains the generated and builtin defaults rest on: none
        // states a gui slot, so the source's default is the completed one.
        let store = Store::open(PathBuf::from(&root)).expect("the store opens");
        let tree = Extractor::new(&store, VERSION).root();
        let models = ModelSource::open(&tree).expect("the block models load");
        for name in [
            "apple",
            "diamond_sword",
            "item/chest",
            "item/trapped_chest",
            "item/ender_chest",
        ] {
            let baked = models.bake_item(name).expect("the model bakes");
            assert_eq!(
                baked.display.gui,
                Transform::DEFAULT,
                "{name} states no gui slot"
            );
        }
        // The first-person slots are the other half of the story: the item files state
        // one — the 45-degree item pose — while the chest trio states none.
        let pose = Transform {
            rotation: [0.0, -135.0, 25.0],
            translation: [0.0, 4.0, 2.0],
            scale: [1.7, 1.7, 1.7],
        };
        for name in ["apple", "diamond_sword"] {
            let baked = models.bake_item(name).expect("the model bakes");
            assert_eq!(
                baked.display.first_person, pose,
                "{name} states the item first-person pose"
            );
        }
        for name in ["item/chest", "item/trapped_chest", "item/ender_chest"] {
            let baked = models.bake_item(name).expect("the model bakes");
            assert_eq!(
                baked.display.first_person,
                Transform::DEFAULT,
                "{name} states no first-person slot"
            );
        }
        // The icon the running client builds carries the slot through: the diamond
        // sword's own icon (`276`, the table's own id) holds the stated pose, and the
        // chest's holds the default.
        assert_eq!(
            icons
                .icon(276, 0)
                .expect("the sword's icon builds")
                .first_person,
            pose,
            "the sword's icon carries its first-person slot"
        );
        assert_eq!(
            icons
                .icon(54, 0)
                .expect("the chest's icon builds")
                .first_person,
            Transform::DEFAULT,
            "the chest's icon carries the default"
        );

        // The wooden sword (`268`): its layer0 is `items/wood_sword` while its file
        // is `item/wooden_sword`, so a layer0-basename rule misses it — the row read
        // the default before the full-list rule.
        assert_eq!(
            icons
                .icon(268, 0)
                .expect("the wooden sword's icon builds")
                .first_person,
            pose,
            "the wooden sword carries its file's stated pose"
        );
        // The potion overlay (`373`): its layer0 alone ties the drinkable and splash
        // files; the full layer list names the drinkable one, whose pose the icon
        // carries.
        assert_eq!(
            icons
                .icon(373, 0)
                .expect("the potion's icon builds")
                .first_person,
            pose,
            "the potion overlay carries the drinkable file's stated pose"
        );
        assert_eq!(
            models
                .generated_item_file(&["items/potion_overlay", "items/potion_bottle_drinkable"])
                .as_deref(),
            Some("item/bottle_drinkable"),
            "the full layer list disambiguates the potion overlay's tie"
        );
        // The generated rows' own first-person slots, resolved by the full layer
        // list: the sweep pins the whole class — every one of the table's 208
        // generated rows carries its file's stated pose.
        let mut rows = 0;
        let mut resolved = 0;
        let mut misses: Vec<(i16, &'static str)> = Vec::new();
        for entry in items::registry() {
            if let ItemModel::Generated(_) = entry.resolution {
                rows += 1;
                if icons
                    .icon(entry.id, 0)
                    .expect("a generated row's icon builds")
                    .first_person
                    != Transform::DEFAULT
                {
                    resolved += 1;
                } else {
                    misses.push((entry.id, entry.name));
                }
            }
        }
        assert_eq!(
            resolved, rows,
            "every generated row resolves its stated pose; misses: {misses:?}"
        );
    }

    /// The live item mesh source: the baked block models, the generated item shapes,
    /// the icon quads and the frame's wood all build from the store's own tree — the
    /// path the running client wires into the pass.
    #[test]
    #[ignore = "reads the asset store; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_object_meshes_build_from_the_store() {
        let root = std::env::var_os(STORE_VAR)
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let assets = ClientAssets::load(Some(PathBuf::from(root))).expect("the client assets load");
        let source = &assets.item_meshes;

        // The sheets the startup uploads are all present.
        assert_eq!(source.sheets.len(), OBJECT_TEXTURES.len());

        // A block item's state bakes through the terrain's own model set, sampling the
        // atlas under the blocks texture's name.
        let stone = source.block_item(1, 0).expect("stone's state bakes");
        assert_eq!(stone.texture, BLOCKS_ATLAS_TEXTURE);
        assert_eq!(stone.vertices.positions.len() % 4, 0);
        assert!(
            stone.vertices.positions.len() >= 24,
            "a cube's six quads, got {}",
            stone.vertices.positions.len()
        );

        // The generated item shape reads the sprite's own pixels.
        let apple = source
            .generated("items/apple")
            .expect("the apple's shape bakes");
        assert_eq!(apple.texture, "items/apple");
        assert!(!apple.vertices.positions.is_empty());
        assert!(source.generated("items/not_a_sheet").is_none());

        // The frame's own wood, baked at load.
        let wood = source.frame_wood().expect("the frame's wood bakes");
        assert!(!wood.vertices.positions.is_empty());

        // The icon quad: the source's own four corners.
        let icon = source
            .icon_quad("items/fireball")
            .expect("the fireball's icon builds");
        assert_eq!(icon.vertices.positions.len(), 4);
        assert_eq!(icon.texture, "items/fireball");

        // The atlas image the startup registers under the blocks name is the level-0
        // stitch itself.
        assert_eq!(assets.blocks_atlas.width, assets.mesh.atlas.width);
        assert_eq!(assets.blocks_atlas.height, assets.mesh.atlas.height);
        assert_eq!(
            assets.blocks_atlas.rgba.len(),
            (assets.mesh.atlas.width as usize) * (assets.mesh.atlas.height as usize) * 4
        );
    }
}
