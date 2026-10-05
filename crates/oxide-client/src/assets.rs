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
/// This milestone's set is the shadow sprite and the two default skins; the mob
/// textures arrive with the models that draw them, each added here with the case that
/// needs it.
pub const ENTITY_TEXTURES: [&str; 3] = ["misc/shadow.png", "entity/steve.png", "entity/alex.png"];

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

/// The texture a resource path names, or the missing-texture error.
fn texture<'a>(textures: &'a TextureSet, key: &'static str) -> Result<&'a Texture, AssetError> {
    textures.get(key).ok_or(AssetError::MissingTexture { key })
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

    #[test]
    fn the_entity_set_names_the_shadow_and_the_two_default_skins() {
        // The pass's static set: the shadow sprite first, then the two defaults, under
        // the keys the resolver falls back to.
        assert_eq!(
            ENTITY_TEXTURES,
            ["misc/shadow.png", "entity/steve.png", "entity/alex.png"]
        );
        assert!(ENTITY_TEXTURES.contains(&DEFAULT_SKIN_WIDE));
        assert!(ENTITY_TEXTURES.contains(&DEFAULT_SKIN_SLIM));
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
                textures.get(key).is_some(),
                "the entity texture {key} is in the store"
            );
        }
    }
}
