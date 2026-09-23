//! Metadata chain tests: the version manifest, the version JSON, and the asset
//! index, parsed from small hand-written fixtures shaped like the piston-meta
//! documents. Nothing here touches the network; the live checks at the end of
//! the file are ignored by default.

use oxide_assets::asset_index::{AssetIndex, AssetObject};
use oxide_assets::http::{HttpClient, UreqClient};
use oxide_assets::version::{
    CLIENT_1_8_9_SHA1, CLIENT_1_8_9_SIZE, VERSION_MANIFEST_URL, VersionManifest, find_version,
    parse_manifest, parse_version_json,
};

const MANIFEST: &str = include_str!("fixtures/version_manifest_v2.json");
const VERSION: &str = include_str!("fixtures/1.8.9.json");

#[test]
fn finds_the_1_8_9_entry_and_its_url() {
    let manifest = parse_manifest(MANIFEST).expect("manifest");
    let entry = find_version(&manifest, "1.8.9").expect("1.8.9 present");
    assert!(entry.url.ends_with("1.8.9.json"));
}

#[test]
fn the_manifest_names_its_latest_release() {
    let manifest = parse_manifest(MANIFEST).expect("manifest");
    assert_eq!(manifest.latest.release, "1.8.9");
    assert_eq!(manifest.latest.snapshot, "1.8.9");
}

#[test]
fn an_absent_version_is_not_found() {
    let manifest = parse_manifest(MANIFEST).expect("manifest");
    assert!(find_version(&manifest, "1.99").is_none());
}

#[test]
fn version_json_exposes_asset_index_and_client_jar() {
    let version = parse_version_json(VERSION).expect("version json");
    assert_eq!(version.id, "1.8.9");
    assert_eq!(version.assets, "1.8");
    assert_eq!(version.asset_index.id, "1.8");
    assert!(
        version.asset_index.url.ends_with("1.8.json"),
        "the index URL is read from the version JSON, never hardcoded"
    );
    assert_eq!(version.asset_index.total_size, 114_885_064);
    assert_eq!(version.downloads.client.size, 8_461_484);
    assert_eq!(version.downloads.client.size, CLIENT_1_8_9_SIZE);
    assert_eq!(version.downloads.client.sha1, CLIENT_1_8_9_SHA1);
    assert!(version.downloads.client.url.ends_with("client.jar"));
}

#[test]
fn asset_index_parses_objects_and_builds_urls() {
    let index_json =
        r#"{"objects":{"minecraft/sounds/ambient/cave/cave1.ogg":{"hash":"abc123","size":42}}}"#;
    let index = AssetIndex::parse(index_json).expect("index");
    let object = index
        .objects
        .get("minecraft/sounds/ambient/cave/cave1.ogg")
        .expect("object");
    assert_eq!(object.size, 42);
    assert_eq!(
        object.url(),
        "https://resources.download.minecraft.net/ab/abc123"
    );
}

#[test]
fn object_url_derivation_holds_for_full_and_short_hashes() {
    let full = AssetObject {
        hash: "00112233445566778899aabbccddeeff00112233".to_string(),
        size: 7,
    };
    assert_eq!(
        full.url(),
        "https://resources.download.minecraft.net/00/00112233445566778899aabbccddeeff00112233"
    );

    // A hash too short to shard falls back to itself instead of panicking;
    // parsing rejects such a hash before it can reach this point.
    let short = AssetObject {
        hash: "a".to_string(),
        size: 7,
    };
    assert_eq!(short.url(), "https://resources.download.minecraft.net/a/a");
}

#[test]
fn the_index_total_is_the_sum_of_its_objects() {
    let index_json = r#"{"objects":{"a":{"hash":"aa11","size":10},"b":{"hash":"bb22","size":32}}}"#;
    let index = AssetIndex::parse(index_json).expect("index");
    assert_eq!(index.total_size(), 42);
}

#[test]
fn a_malformed_object_entry_is_rejected_not_panicked_on() {
    for (shape, json) in [
        ("empty", r#"{"objects":{"bad":{"hash":"","size":1}}}"#),
        (
            "one character",
            r#"{"objects":{"bad":{"hash":"a","size":1}}}"#,
        ),
        ("not hex", r#"{"objects":{"bad":{"hash":"zzzz","size":1}}}"#),
        ("no hash", r#"{"objects":{"bad":{"size":1}}}"#),
        (
            "size as text",
            r#"{"objects":{"bad":{"hash":"aa11","size":"1"}}}"#,
        ),
    ] {
        assert!(
            AssetIndex::parse(json).is_err(),
            "a hash with shape {shape} must be rejected"
        );
    }

    let good = r#"{"objects":{"good":{"hash":"ab","size":1}}}"#;
    assert!(AssetIndex::parse(good).is_ok());
}

/// Fetches and parses the live version manifest.
fn fetch_manifest(http: &dyn HttpClient) -> VersionManifest {
    let body = http
        .get(VERSION_MANIFEST_URL)
        .expect("fetch the live manifest");
    let json = std::str::from_utf8(&body).expect("the manifest is UTF-8");
    parse_manifest(json).expect("parse the live manifest")
}

// The two tests below reach the real Mojang endpoints and are skipped by
// default. Run them with:
//   cargo test -p oxide-assets --test metadata -- --ignored

#[test]
#[ignore = "network test against the live piston-meta version manifest"]
fn live_manifest_names_a_release_and_still_lists_1_8_9() {
    let http = UreqClient::new();
    let manifest = fetch_manifest(&http);
    assert!(
        !manifest.latest.release.is_empty(),
        "the live manifest names the current release"
    );
    assert!(!manifest.latest.snapshot.is_empty());
    let entry = find_version(&manifest, "1.8.9").expect("1.8.9 is still listed");
    assert!(entry.url.ends_with("1.8.9.json"));
}

#[test]
#[ignore = "network test against the live version JSON for 1.8.9"]
fn live_version_json_matches_the_client_jar_constants() {
    let http = UreqClient::new();
    let manifest = fetch_manifest(&http);
    let entry = find_version(&manifest, "1.8.9").expect("1.8.9 is still listed");
    let body = http.get(&entry.url).expect("fetch the 1.8.9 version JSON");
    let version =
        parse_version_json(std::str::from_utf8(&body).expect("UTF-8")).expect("version json");

    assert_eq!(version.asset_index.id, "1.8");
    assert_eq!(version.downloads.client.sha1, CLIENT_1_8_9_SHA1);
    assert_eq!(version.downloads.client.size, CLIENT_1_8_9_SIZE);
}
