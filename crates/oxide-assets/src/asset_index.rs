//! The hashed asset index: logical paths to content-addressed objects.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::Error as _;

/// The 1.8 asset index.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    /// Logical path to object metadata.
    pub objects: BTreeMap<String, AssetObject>,
}

/// One hashed object.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    /// SHA-1 hash, lowercase hex.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
}

impl AssetObject {
    /// The download URL for this object.
    ///
    /// Delegates to the store's derivation, the one owner of the resources
    /// base URL and the shard rule. A hash shorter than two characters shards
    /// on itself, the way the store's paths do, so this never panics.
    #[must_use]
    pub fn url(&self) -> String {
        crate::store::object_url(&self.hash)
    }
}

impl AssetIndex {
    /// Parses an index document.
    ///
    /// Every object hash is checked while the document is read: a hash that is
    /// not at least two ASCII hex characters is rejected, because no object URL
    /// can be derived from it. The store checks the full forty-character form
    /// again when an object is fetched.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        let index: Self = serde_json::from_str(json)?;
        for (path, object) in &index.objects {
            if !is_shardable_hash(&object.hash) {
                return Err(serde_json::Error::custom(format!(
                    "malformed object hash {hash:?} for {path:?}",
                    hash = object.hash
                )));
            }
        }
        Ok(index)
    }

    /// Total size of every object in the index.
    #[must_use]
    pub fn total_size(&self) -> u64 {
        self.objects.values().map(|object| object.size).sum()
    }
}

/// True when `hash` can be sharded into a URL path: two or more ASCII hex
/// characters.
fn is_shardable_hash(hash: &str) -> bool {
    hash.len() >= 2 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}
