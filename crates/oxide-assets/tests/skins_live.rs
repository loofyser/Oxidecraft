//! The live skin fetch: one real CDN URL through the cache.
//!
//! Ignored by default — it reaches the network and expects a public URL to
//! stay up, so it never runs in the gate. Run it with:
//!
//!     cargo test -p oxide-assets --test skins_live -- --ignored
//!
//! Everything it fetches lands in a temporary store under the system's temp
//! directory; no fetched byte is ever written into the repository.

use oxide_assets::skins::SkinCache;
use oxide_assets::store::Store;

/// The default skin the client falls back to when a profile names none: the
/// Steve texture, the same CDN path every client fetches skins from.
const DEFAULT_SKIN: &str = "https://textures.minecraft.net/texture/\
                             5c500205248f3af53ea628f862ebf756fe8e7c9ec8afa4bd963fed1497f46ee1";

#[test]
#[ignore = "fetches the live skin CDN; run explicitly with --ignored"]
fn the_cdn_serves_a_64x64_skin() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let store = Store::open(dir.path().to_path_buf()).expect("the store opens");
    let cache = SkinCache::new(&store);

    let texture = cache
        .fetch(DEFAULT_SKIN)
        .expect("the CDN serves the default skin");
    assert_eq!(
        (texture.width, texture.height),
        (64, 64),
        "the default skin is the 64x64 Steve texture"
    );

    // The bytes landed in the temporary store's own skins directory, not
    // anywhere a commit could reach.
    let path = cache
        .path_for(DEFAULT_SKIN)
        .expect("the url names a cache file");
    assert!(path.starts_with(dir.path().join("skins")));
    assert!(path.is_file());

    // The second fetch is the disk hit: the same texels, no second download.
    let again = cache.fetch(DEFAULT_SKIN).expect("the cached copy serves");
    assert_eq!(again.rgba, texture.rgba);
}
