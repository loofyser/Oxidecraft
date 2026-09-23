//! piston-meta version metadata: the version manifest, the version JSON, and
//! the download descriptors Oxidecraft reads.

use serde::Deserialize;

/// Where the version manifest lives.
pub const VERSION_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

/// SHA-1 of the 1.8.9 client jar, taken from a known-good local copy of that jar
/// and used as a constant guard.
pub const CLIENT_1_8_9_SHA1: &str = "3870888a6c3d349d3771a3e9d16c9bf5e076b908";

/// Size of the 1.8.9 client jar in bytes.
pub const CLIENT_1_8_9_SIZE: u64 = 8_461_484;

/// The version manifest, trimmed to the fields Oxidecraft reads.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    /// The newest versions, by channel.
    pub latest: LatestVersions,
    /// Every published version, newest first.
    pub versions: Vec<VersionEntry>,
}

/// The newest version ids, by channel.
#[derive(Debug, Clone, Deserialize)]
pub struct LatestVersions {
    /// The newest release version, for example `1.8.9`.
    pub release: String,
    /// The newest snapshot version.
    pub snapshot: String,
}

/// One entry in the version manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionEntry {
    /// Version id, for example `1.8.9`.
    pub id: String,
    /// URL of this version's JSON document.
    pub url: String,
    /// SHA-1 of the version JSON document.
    pub sha1: String,
}

/// A version JSON document, trimmed to the fields Oxidecraft reads.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionJson {
    /// Version id.
    pub id: String,
    /// Asset index name, `1.8` for the whole 1.8 line.
    pub assets: String,
    /// Asset index descriptor.
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexInfo,
    /// Downloadable artifacts. The document lists more than the client; only
    /// the client jar is read.
    pub downloads: Downloads,
}

/// Descriptor for the asset index.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexInfo {
    /// Index id, `1.8`.
    pub id: String,
    /// URL of the index document.
    pub url: String,
    /// SHA-1 of the index document.
    pub sha1: String,
    /// Size of the index document in bytes.
    pub size: u64,
    /// Total size of every object the index lists, in bytes.
    #[serde(rename = "totalSize")]
    pub total_size: u64,
}

/// Downloadable artifacts for a version.
#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    /// The client jar.
    pub client: DownloadInfo,
}

/// One downloadable file.
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfo {
    /// URL.
    pub url: String,
    /// SHA-1.
    pub sha1: String,
    /// Size in bytes.
    pub size: u64,
}

/// Parses a version manifest.
pub fn parse_manifest(json: &str) -> Result<VersionManifest, serde_json::Error> {
    serde_json::from_str(json)
}

/// Parses a version JSON document.
pub fn parse_version_json(json: &str) -> Result<VersionJson, serde_json::Error> {
    serde_json::from_str(json)
}

/// Finds the version entry with the given id.
pub fn find_version<'a>(manifest: &'a VersionManifest, id: &str) -> Option<&'a VersionEntry> {
    manifest.versions.iter().find(|entry| entry.id == id)
}
