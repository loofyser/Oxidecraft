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

use std::path::PathBuf;
use std::sync::Arc;

use oxide_assets::atlas::{AtlasError, build_atlas};
use oxide_assets::extract::Extractor;
use oxide_assets::font::{Font, FontError};
use oxide_assets::model::{ModelError, ModelSource};
use oxide_assets::resources::{ResourceError, TextureSet};
use oxide_assets::store::{Store, StoreError};
use oxide_assets::texture::Texture;
use oxide_game::mesher::BlockModelSet;
use oxide_game::session::MeshAssets;
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
/// this milestone draw — the biped family, the core quadrupeds and the crawler
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
pub const ENTITY_TEXTURES: [&str; 34] = [
    "misc/shadow.png",
    "entity/steve.png",
    "entity/alex.png",
    "entity/zombie/zombie.png",
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
];

/// The wide default skin's key, an entry of [`ENTITY_TEXTURES`].
pub const DEFAULT_SKIN_WIDE: &str = "entity/steve.png";

/// The slim default skin's key, an entry of [`ENTITY_TEXTURES`].
pub const DEFAULT_SKIN_SLIM: &str = "entity/alex.png";

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
    /// The ascii font sheet itself, for the overlay's GPU upload.
    pub sheet: Texture,
    /// The sun, the moon phase sheet and the cloud layer.
    pub sky_textures: SkyTextures,
    /// The entity textures, keyed by the pass's names.
    pub entity_textures: Vec<(&'static str, Texture)>,
    /// The wide default skin, from the entity set.
    pub skin_wide: Texture,
    /// The slim default skin, from the entity set.
    pub skin_slim: Texture,
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
        let atlas = build_atlas(&textures, &models.texture_paths(), ATLAS_MIP_LEVELS)?;
        let block_models = BlockModelSet::load(&models);
        let tint_maps = TintMaps {
            grass: colormap(&textures, GRASS_COLORMAP)?,
            foliage: colormap(&textures, FOLIAGE_COLORMAP)?,
        };
        let sheet = texture(&textures, FONT_SHEET)?.clone();
        let font = Font::load(&sheet, None)?;
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

        tracing::info!(
            atlas_width = atlas.width,
            atlas_height = atlas.height,
            atlas_levels = atlas.level_count,
            atlas_sprites = atlas.sprites.len(),
            sheet_width = sheet.width,
            sheet_height = sheet.height,
            "the client assets were loaded"
        );
        Ok(ClientAssets {
            mesh: Arc::new(MeshAssets {
                models: block_models,
                atlas,
                tint_maps,
            }),
            font,
            sheet,
            sky_textures,
            entity_textures,
            skin_wide,
            skin_slim,
        })
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
}
