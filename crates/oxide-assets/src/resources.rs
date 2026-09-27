//! The texture set: every texture under an extraction root, keyed by its
//! resource path.
//!
//! [`TextureSet::load`] reads the tree the extractor wrote —
//! `<extraction root>/assets/minecraft/textures/` — and holds every `.png`
//! under it as a [`Texture`], with each `.png.mcmeta` sidecar parsed and
//! attached to its texture. The root is what
//! [`Extractor::root`](crate::extract::Extractor::root) returns; resolving a
//! store root to that path is the caller's business, not this module's.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::mcmeta::{AnimationMeta, McmetaError};
use crate::texture::{Texture, TextureError};

/// The texture tree inside an extraction root, with the case the jar uses.
const TEXTURES_DIR: &str = "assets/minecraft/textures";

/// The extension a texture file carries, with the case the jar uses.
const PNG_EXTENSION: &str = "png";

/// The suffix an animation sidecar's name carries, after its texture's name.
const MCMETA_SUFFIX: &str = ".png.mcmeta";

/// The key prefix of the unicode glyph pages the loader skips.
const UNICODE_PAGE_PREFIX: &str = "font/unicode_page_";

/// Errors from loading a texture tree.
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    /// A filesystem failure while walking or reading the tree.
    #[error("filesystem error at {path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A texture that does not decode.
    #[error("could not load the texture {path}: {source}")]
    Texture {
        /// The texture file.
        path: PathBuf,
        /// Underlying error.
        source: TextureError,
    },
    /// An animation sidecar that does not parse.
    #[error("could not parse the animation sidecar {path}: {source}")]
    Mcmeta {
        /// The sidecar file.
        path: PathBuf,
        /// Underlying error.
        source: McmetaError,
    },
    /// A sidecar whose texture is not in the tree.
    #[error("the animation sidecar {path} has no texture beside it")]
    OrphanSidecar {
        /// The sidecar file.
        path: PathBuf,
    },
    /// A path below the texture tree that cannot become a key.
    #[error("the texture path {path} is not a usable UTF-8 path below the texture tree")]
    NonUtf8Path {
        /// The offending path.
        path: PathBuf,
    },
}

/// Every texture an extraction tree holds, with its animation sidecars.
#[derive(Debug)]
pub struct TextureSet {
    /// The textures, keyed by their path below `textures/` without the
    /// extension.
    textures: BTreeMap<String, Texture>,
    /// The sidecars that parsed, keyed the same way as `textures`.
    animations: BTreeMap<String, AnimationMeta>,
}

impl TextureSet {
    /// Loads every texture under `extraction_root`'s texture tree.
    ///
    /// `extraction_root` is what
    /// [`Extractor::root`](crate::extract::Extractor::root) returns —
    /// `<store root>/extracted/<version>` — and the textures live under its
    /// `assets/minecraft/textures/`, with the case the jar uses. Keys are the
    /// paths below that directory without the `.png` extension:
    /// `blocks/stone`, `font/ascii`, `colormap/grass`.
    ///
    /// `font/unicode_page_*.png` is skipped: 222 files in the 1.8.9 tree, the
    /// unicode glyph pages a later milestone's text path takes on. The pages'
    /// neighbours under `font/` (`ascii.png`, `ascii_sga.png`) load normally,
    /// and a page's own sidecar, were a tree to carry one, is skipped with
    /// it.
    ///
    /// The walk is sorted, so the same tree fails on the same file on every
    /// host. The set is whole or it is an error: a texture that does not
    /// decode, a sidecar that does not parse, and a sidecar whose texture is
    /// absent each fail the load, naming the file and the reason.
    pub fn load(extraction_root: &Path) -> Result<Self, ResourceError> {
        let root = extraction_root.join(TEXTURES_DIR);
        let files = collect_files(&root)?;

        // Classify first, in sorted path order: the sidecars by the key they
        // belong to, the textures as (key, path) pairs. A skipped page drops
        // out here, its sidecar with it.
        let mut textures: Vec<(String, PathBuf)> = Vec::new();
        let mut sidecars: BTreeMap<String, PathBuf> = BTreeMap::new();
        for relative in files {
            let path = root.join(&relative);
            let Some(name) = relative.file_name().and_then(OsStr::to_str) else {
                return Err(ResourceError::NonUtf8Path { path });
            };
            let sidecar = name
                .strip_suffix(MCMETA_SUFFIX)
                .filter(|png_name| !png_name.is_empty());
            if let Some(png_name) = sidecar {
                let key = key_for(&relative.with_file_name(png_name))?;
                if !is_unicode_page(&key) {
                    sidecars.insert(key, path);
                }
            } else if relative.extension().and_then(OsStr::to_str) == Some(PNG_EXTENSION) {
                let key = key_for(&relative)?;
                if !is_unicode_page(&key) {
                    textures.push((key, path));
                }
            }
        }

        // Load the textures, attaching each sidecar as its texture lands so
        // the first failure is the first file in sorted order.
        let mut set = Self {
            textures: BTreeMap::new(),
            animations: BTreeMap::new(),
        };
        for (key, path) in &textures {
            let bytes = fs::read(path).map_err(|source| ResourceError::Io {
                path: path.clone(),
                source,
            })?;
            let texture = Texture::from_png(&bytes).map_err(|source| ResourceError::Texture {
                path: path.clone(),
                source,
            })?;
            set.textures.insert(key.clone(), texture);

            if let Some(sidecar) = sidecars.remove(key) {
                let json = fs::read_to_string(&sidecar).map_err(|source| ResourceError::Io {
                    path: sidecar.clone(),
                    source,
                })?;
                let meta = AnimationMeta::parse(&json).map_err(|source| ResourceError::Mcmeta {
                    path: sidecar,
                    source,
                })?;
                set.animations.insert(key.clone(), meta);
            }
        }

        // Whatever is left over is a sidecar whose texture never loaded; the
        // map's order makes the first one the one reported.
        if let Some((_, path)) = sidecars.into_iter().next() {
            return Err(ResourceError::OrphanSidecar { path });
        }
        Ok(set)
    }

    /// The texture a resource path names, when the set holds it.
    ///
    /// The path is the key [`TextureSet::load`] built: `blocks/stone`,
    /// `items/stick`, `colormap/grass`, `font/ascii`, `environment/sun`.
    pub fn get(&self, path: &str) -> Option<&Texture> {
        self.textures.get(path)
    }

    /// The animation a sidecar beside the texture states, when there is one.
    ///
    /// A sidecar that holds no `animation` object — the 1.8 tree's four
    /// `misc/` sidecars state a `texture` section — parses to the default
    /// meta (frametime 1, no frame list) and is recorded like any other.
    pub fn animation(&self, path: &str) -> Option<&AnimationMeta> {
        self.animations.get(path)
    }
}

/// True for the unicode glyph pages the loader skips.
///
/// The 1.8.9 tree holds 222 of them; they belong to a later milestone's
/// unicode text, and the pages' neighbours under `font/` load normally.
fn is_unicode_page(key: &str) -> bool {
    key.starts_with(UNICODE_PAGE_PREFIX)
}

/// The key for a texture file's path, relative to the texture tree: the path
/// with its extension dropped.
fn key_for(relative: &Path) -> Result<String, ResourceError> {
    let stem = relative.with_extension("");
    let Some(key) = stem.to_str() else {
        return Err(ResourceError::NonUtf8Path {
            path: relative.to_path_buf(),
        });
    };
    Ok(key.to_string())
}

/// Every file under `dir`, recursively, sorted, as paths relative to `dir`.
///
/// The directory entries are visited in the order the filesystem lists them;
/// the result is sorted afterwards, so the walk order cannot leak into which
/// file a failure reports.
fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, ResourceError> {
    let mut files = Vec::new();
    collect_into(dir, Path::new(""), &mut files)?;
    files.sort();
    Ok(files)
}

/// The recursive half of [`collect_files`]: pushes every file under `dir`,
/// naming each by its path below the walk's starting directory.
fn collect_into(dir: &Path, prefix: &Path, files: &mut Vec<PathBuf>) -> Result<(), ResourceError> {
    let entries = fs::read_dir(dir).map_err(|source| ResourceError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ResourceError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let relative = prefix.join(entry.file_name());
        let file_type = entry.file_type().map_err(|source| ResourceError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect_into(&path, &relative, files)?;
        } else {
            files.push(relative);
        }
    }
    Ok(())
}
