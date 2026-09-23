//! Jar extraction: read the client jar once and write the resources the
//! client needs into the store, with an atomic manifest recording what was
//! extracted and from which jar.
//!
//! The include and skip rules are fixed. Every entry under `assets/` plus
//! the two root files `pack.mcmeta` and `sounds.json` are extracted; class
//! files, `META-INF/` entries, signature files, directory entries and entry
//! names that are not safe relative paths are skipped. A skipped entry is
//! never opened: the name alone decides, so no byte of a class entry is ever
//! read.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use zip::ZipArchive;

use crate::store::{Store, StoreError, is_safe_id, write_atomic};

/// Schema version of the extraction manifest; bump to force re-extraction.
pub const EXTRACTOR_SCHEMA_VERSION: u32 = 1;

/// The manifest's file name inside the extraction root.
const MANIFEST_FILE_NAME: &str = ".manifest.json";

/// The store subdirectory the extracted trees live under, relative to the
/// store root. It matches the directory [`Store::open`] creates.
const EXTRACTED_SUBDIR: &str = "extracted";

/// The prefix every extracted resource entry shares.
const ASSETS_PREFIX: &str = "assets/";

/// The root files that are extracted alongside the `assets/` tree.
const ROOT_FILES: [&str; 2] = ["pack.mcmeta", "sounds.json"];

/// The entry prefix that is never extracted.
const META_INF_PREFIX: &str = "META-INF/";

/// The file extensions of JAR signature files, which are never extracted.
const SIGNATURE_EXTENSIONS: [&str; 4] = ["sf", "rsa", "dsa", "ec"];

/// The most bytes a single entry read pre-allocates from the size the archive
/// records. It is a claim until the bytes have been read, so it is not
/// trusted with an unbounded reservation.
const MAX_ENTRY_PREALLOC: u64 = 1 << 20;

/// Errors from jar extraction.
///
/// Filesystem failures carry the store's shape whichever layer produced them:
/// the extractor's own reads and removals report [`StoreError::Io`] inside
/// [`ExtractError::Store`], exactly as the store's writes do, so one kind of
/// failure has one shape in logs and in callers' matches.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// A filesystem or store write failure.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The jar is not readable as a zip archive.
    #[error("malformed jar at {path}: {source}")]
    Zip {
        /// The jar.
        path: PathBuf,
        /// Underlying error.
        source: zip::result::ZipError,
    },
    /// An included entry could not be read out of the jar.
    #[error("could not read {entry} from the jar at {jar}: {source}")]
    Entry {
        /// The jar.
        jar: PathBuf,
        /// The entry's name.
        entry: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The manifest could not be serialized.
    #[error("could not serialize the extraction manifest: {source}")]
    Json {
        /// Underlying error.
        source: serde_json::Error,
    },
    /// A version id that is not usable as a path segment.
    #[error("version id {version:?} is not usable as a path segment")]
    BadVersion {
        /// The offending id.
        version: String,
    },
}

/// What an extraction run did.
///
/// `extracted` and `skipped` add up to `entries_read`; every count is zero on
/// a run that wrote nothing because the extraction was already up to date.
#[derive(Debug, Default)]
pub struct ExtractionReport {
    /// Entries the archive listed: the includable ones, the refused ones and
    /// directory entries alike.
    pub entries_read: usize,
    /// Entries extracted this run.
    pub extracted: usize,
    /// Entries refused by the include and skip rules.
    pub skipped: usize,
    /// Total bytes of the entries extracted this run.
    pub bytes: u64,
    /// True when the manifest was current and nothing was written.
    pub up_to_date: bool,
}

/// The extraction manifest: the source jar, the extractor schema and the
/// extracted files.
///
/// It is written atomically and only after every file has landed, so a reader
/// sees either the previous manifest or the complete new one, never a partial
/// list.
///
/// The up-to-date check is cheap by design: the jar's SHA-1 is the identity
/// check, and every recorded file is then checked for existence and its
/// recorded size rather than re-hashed. A missing or size-mismatched file
/// means the extraction is re-done; nothing outside the recorded set is
/// looked at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionManifest {
    /// SHA-1 of the source jar, lowercase hex.
    pub jar_sha1: String,
    /// The extractor schema version that wrote this manifest; the extraction
    /// is re-done when it differs from [`EXTRACTOR_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Every extracted file, relative path to byte size.
    pub entries: BTreeMap<String, u64>,
}

/// Extracts one version's client jar into the store, once.
pub struct Extractor<'a> {
    /// The store the jar and the extraction tree live in.
    store: &'a Store,
    /// The version being extracted.
    version: String,
}

impl<'a> Extractor<'a> {
    /// An extractor for `version`: it reads the client jar from
    /// `<store>/versions/<version>/client.jar` and writes under
    /// `<store>/extracted/<version>`.
    ///
    /// The version is used as a path segment; a version id that could escape
    /// the store is refused by [`Extractor::plan`], [`Extractor::run`] and
    /// [`Extractor::is_up_to_date`].
    pub fn new(store: &'a Store, version: &str) -> Self {
        Self {
            store,
            version: version.to_string(),
        }
    }

    /// The directory this extractor writes into.
    pub fn root(&self) -> PathBuf {
        self.store.root().join(EXTRACTED_SUBDIR).join(&self.version)
    }

    /// The manifest's path, inside [`Extractor::root`].
    pub fn manifest_path(&self) -> PathBuf {
        self.root().join(MANIFEST_FILE_NAME)
    }

    /// The paths one run would extract, sorted, without reading any entry.
    pub fn plan(&self) -> Result<Vec<String>, ExtractError> {
        self.check_version()?;
        let jar_bytes = self.read_jar()?;
        let archive = self.open_archive(&jar_bytes)?;
        let mut planned = Vec::new();
        for index in 0..archive.len() {
            let Some(name) = archive.name_for_index(index) else {
                continue;
            };
            if is_extractable(name) {
                planned.push(name.to_string());
            }
        }
        planned.sort();
        Ok(planned)
    }

    /// Extracts the jar unless the manifest shows the extraction is current.
    ///
    /// Every file is written atomically, the stale manifest is removed before
    /// the first write and the fresh one is written last, so an interrupted
    /// run never leaves a manifest describing a partial tree. A skipped run
    /// reports `up_to_date` and writes nothing.
    pub fn run(&self) -> Result<ExtractionReport, ExtractError> {
        self.check_version()?;
        let jar_bytes = self.read_jar()?;
        let jar_sha1 = sha1_hex(&jar_bytes);
        if self.manifest_matches(&jar_sha1) {
            return Ok(ExtractionReport {
                up_to_date: true,
                ..ExtractionReport::default()
            });
        }

        // The old manifest goes first: an interrupted re-extraction must not
        // leave behind a manifest that claims the tree is current.
        self.remove_manifest()?;

        let mut archive = self.open_archive(&jar_bytes)?;
        let mut report = ExtractionReport::default();
        let mut entries = BTreeMap::new();
        let jar_path = self.jar_path()?;
        for index in 0..archive.len() {
            let Some(name) = archive.name_for_index(index).map(str::to_string) else {
                continue;
            };
            report.entries_read += 1;
            if !is_extractable(&name) {
                report.skipped += 1;
                continue;
            }

            // Only now is the entry opened, and only entries this rule set
            // accepts ever are.
            let mut file = archive
                .by_index(index)
                .map_err(|source| ExtractError::Zip {
                    path: jar_path.clone(),
                    source,
                })?;
            let mut bytes = Vec::with_capacity(file.size().min(MAX_ENTRY_PREALLOC) as usize);
            file.read_to_end(&mut bytes)
                .map_err(|source| ExtractError::Entry {
                    jar: jar_path.clone(),
                    entry: name.clone(),
                    source,
                })?;
            write_atomic(&self.root().join(&name), &bytes)?;

            report.extracted += 1;
            report.bytes += bytes.len() as u64;
            entries.insert(name, bytes.len() as u64);
        }

        let manifest = ExtractionManifest {
            jar_sha1,
            schema_version: EXTRACTOR_SCHEMA_VERSION,
            entries,
        };
        let json =
            serde_json::to_vec_pretty(&manifest).map_err(|source| ExtractError::Json { source })?;
        write_atomic(&self.manifest_path(), &json)?;
        Ok(report)
    }

    /// True when the manifest describes this jar and every recorded file is
    /// present at its recorded size.
    ///
    /// The check hashes the jar but does not re-hash the extracted files:
    /// sizes are the cheap check, and a missing or size-mismatched file means
    /// the extraction is re-done.
    pub fn is_up_to_date(&self) -> bool {
        if self.check_version().is_err() {
            return false;
        }
        let Ok(jar_bytes) = self.read_jar() else {
            return false;
        };
        self.manifest_matches(&sha1_hex(&jar_bytes))
    }

    /// True when the manifest on disk names `jar_sha1`, carries the current
    /// schema version and every recorded file exists with its recorded size.
    fn manifest_matches(&self, jar_sha1: &str) -> bool {
        let Some(manifest) = self.read_manifest() else {
            return false;
        };
        if manifest.schema_version != EXTRACTOR_SCHEMA_VERSION || manifest.jar_sha1 != jar_sha1 {
            return false;
        }
        let root = self.root();
        manifest.entries.iter().all(|(relative, size)| {
            fs::metadata(root.join(relative))
                .map(|metadata| metadata.is_file() && metadata.len() == *size)
                .unwrap_or(false)
        })
    }

    /// The stored manifest, or `None` when it is absent or does not parse.
    fn read_manifest(&self) -> Option<ExtractionManifest> {
        let bytes = fs::read(self.manifest_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Removes a stale manifest, tolerating an absent one.
    fn remove_manifest(&self) -> Result<(), ExtractError> {
        let path = self.manifest_path();
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StoreError::Io { path, source }.into()),
        }
    }

    /// The path of the client jar this extractor reads.
    ///
    /// Every entry point checks the version before this path is built, and
    /// the store's builder checks it again.
    fn jar_path(&self) -> Result<PathBuf, ExtractError> {
        Ok(self.store.client_jar_path(&self.version)?)
    }

    /// Reads the jar; it is read whole, which a client jar's size allows.
    fn read_jar(&self) -> Result<Vec<u8>, ExtractError> {
        let path = self.jar_path()?;
        fs::read(&path).map_err(|source| StoreError::Io { path, source }.into())
    }

    /// Opens `bytes` as a zip archive.
    fn open_archive<'b>(
        &self,
        bytes: &'b [u8],
    ) -> Result<ZipArchive<Cursor<&'b [u8]>>, ExtractError> {
        let path = self.jar_path()?;
        ZipArchive::new(Cursor::new(bytes)).map_err(|source| ExtractError::Zip { path, source })
    }

    /// Refuses a version id that could escape the store as a path segment.
    fn check_version(&self) -> Result<(), ExtractError> {
        if is_safe_id(&self.version) {
            Ok(())
        } else {
            Err(ExtractError::BadVersion {
                version: self.version.clone(),
            })
        }
    }
}

/// True when `name` is one of the entries the extractor takes.
///
/// The refusal rules come first, so a class entry under `assets/` is refused
/// like any other: nothing reaches the reader without passing every refusal.
/// A path segment ending in `.class` is refused even when the entry's own
/// name does not end in it, so nothing sitting below a class segment is
/// taken either. Directory entries are recognised by their trailing slash,
/// the JAR format's convention.
fn is_extractable(name: &str) -> bool {
    if has_class_segment(name)
        || is_signature_file(name)
        || name.starts_with(META_INF_PREFIX)
        || name.ends_with('/')
    {
        return false;
    }
    if !is_safe_relative_path(name) {
        return false;
    }
    name.starts_with(ASSETS_PREFIX) || ROOT_FILES.contains(&name)
}

/// True when any of `name`'s path segments ends in `.class`, ignoring ASCII
/// case.
///
/// The entry's own name is covered by its final segment; a directory segment
/// carrying the extension is refused too, so a path like `foo.class/bar.png`
/// never reaches the reader.
fn has_class_segment(name: &str) -> bool {
    name.split('/')
        .any(|segment| ends_with_ignore_ascii_case(segment, ".class"))
}

/// True when the entry's final segment ends in a JAR signing extension.
fn is_signature_file(name: &str) -> bool {
    let final_segment = name.rsplit('/').next().unwrap_or(name);
    let Some((_, extension)) = final_segment.rsplit_once('.') else {
        return false;
    };
    SIGNATURE_EXTENSIONS
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
}

/// True when `name` ends with `suffix`, ignoring ASCII case. Unlike a plain
/// slice it does not panic on a name whose final bytes are not a character
/// boundary.
fn ends_with_ignore_ascii_case(name: &str, suffix: &str) -> bool {
    let Some(start) = name.len().checked_sub(suffix.len()) else {
        return false;
    };
    name.get(start..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
}

/// True when `name` is a relative path that stays inside the extraction root:
/// non-empty, without backslashes or NULs, and with normal path components
/// only, so no `..`, no root and no drive prefix.
fn is_safe_relative_path(name: &str) -> bool {
    if name.is_empty() || name.contains('\\') || name.contains('\0') {
        return false;
    }
    Path::new(name)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

/// The lowercase hex SHA-1 of `bytes`.
fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

#[cfg(test)]
mod tests {
    //! Unit tests for the include and skip rules and the relative-path guard.

    use super::{is_extractable, is_safe_relative_path};

    #[test]
    fn the_include_rule_takes_the_assets_tree_and_the_two_root_files() {
        assert!(is_extractable("assets/minecraft/lang/en_US.lang"));
        assert!(is_extractable("assets/minecraft/textures/blocks/stone.png"));
        assert!(
            is_extractable("assets/minecraft/textures/blocks/stone.png.mcmeta"),
            "animation sidecars are resources too"
        );
        assert!(is_extractable("pack.mcmeta"));
        assert!(is_extractable("sounds.json"));
        assert!(
            !is_extractable("log4j2.xml"),
            "a root file outside the include list stays out"
        );
        assert!(
            !is_extractable("net/minecraft/client/Minecraft.java"),
            "only the assets tree and the two root files are taken"
        );
        assert!(!is_extractable(""));
    }

    #[test]
    fn a_class_entry_is_never_extractable() {
        assert!(!is_extractable("net/minecraft/client/Minecraft.class"));
        assert!(
            !is_extractable("assets/minecraft/Foo.class"),
            "the class refusal wins under the assets tree too"
        );
        assert!(!is_extractable("assets/minecraft/Foo.CLASS"));
        assert!(
            !is_extractable("assets/minecraft/foo.class/bar.png"),
            "a path below a .class segment stays out"
        );
        assert!(
            !is_extractable("assets/minecraft/Foo.CLASS/bar.png"),
            "the segment refusal ignores case"
        );
        assert!(!is_extractable("META-INF/MANIFEST.MF"));
        assert!(!is_extractable("META-INF/MOJANG_C.SF"));
        assert!(!is_extractable("META-INF/versions/9/module-info.class"));
        assert!(
            !is_extractable("assets/minecraft/textures/gui/PACK.SF"),
            "a signature file stays out wherever it sits"
        );
        assert!(!is_extractable("assets/minecraft/textures/gui/pack.rsa"));
        assert!(
            !is_extractable("assets/minecraft/textures/"),
            "a directory entry stays out"
        );
    }

    #[test]
    fn a_path_that_would_escape_the_extraction_root_is_refused() {
        assert!(is_safe_relative_path("assets/minecraft/lang/en_US.lang"));
        assert!(is_safe_relative_path("pack.mcmeta"));
        assert!(!is_safe_relative_path(""));
        assert!(!is_safe_relative_path("."));
        assert!(!is_safe_relative_path(".."));
        assert!(!is_safe_relative_path("../escape.txt"));
        assert!(!is_safe_relative_path("assets/../../escape.txt"));
        assert!(!is_safe_relative_path("./assets/x.png"));
        assert!(!is_safe_relative_path("/etc/passwd"));
        assert!(!is_safe_relative_path("assets\\x.png"));
        assert!(!is_safe_relative_path("assets/x\0.png"));
    }
}
