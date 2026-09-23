//! Extraction tests against a synthetic jar built inside the test: the
//! include and skip rules, the atomic manifest, the up-to-date check and the
//! re-extraction cases.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use oxide_assets::extract::{
    EXTRACTOR_SCHEMA_VERSION, ExtractError, ExtractionManifest, Extractor,
};
use oxide_assets::store::Store;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// The version the tests extract for.
const VERSION: &str = "1.8.9";

/// The manifest's file name, as the store layout names it.
const MANIFEST_FILE_NAME: &str = ".manifest.json";

/// The refused class entry's payload. Every refused payload is poisoned
/// before the jar is handed to the extractor, so a reader that took the
/// entry's bytes would fail on the checksum.
const CLASS_PAYLOAD: &[u8] = b"synthetic class payload, never to be read (0001)";

/// The refused META-INF entry's payload, poisoned the same way.
const META_INF_PAYLOAD: &[u8] = b"synthetic manifest payload, never to be read (0002)";

/// The bytes of an included texture, fake but recognizable.
const STONE: &[u8] = b"not really a png";

/// The bytes of the texture's animation sidecar.
const STONE_MCMETA: &[u8] = b"{\"animation\": {\"frametime\": 2}}";

/// The bytes of a language file.
const LANG: &[u8] = b"item.stone=Stone\ntile.stone=Stone\n";

/// The bytes of the resource pack descriptor.
const PACK_MCMETA: &[u8] = b"{\"pack\": {\"pack_format\": 1, \"description\": \"fixture\"}}";

/// The bytes of a root sound index.
const SOUNDS_JSON: &[u8] = b"{\"block.stone.break\": {\"category\": \"block\"}}";

/// Opens a store under `dir`.
fn store_at(dir: &Path) -> Store {
    Store::open(dir.to_path_buf()).expect("open the store")
}

/// Stores `bytes` as the test version's client jar.
fn write_jar(store: &Store, bytes: &[u8]) {
    let path = store.client_jar_path(VERSION).expect("the client jar path");
    std::fs::create_dir_all(path.parent().expect("the version directory"))
        .expect("create the version directory");
    std::fs::write(&path, bytes).expect("write the jar");
}

/// The extraction root, computed the way the store layout spells it rather
/// than through the extractor, so a wrong path in the code cannot hide.
fn extraction_root(store: &Store) -> PathBuf {
    store.root().join("extracted").join(VERSION)
}

/// The manifest path, computed the way the store layout spells it.
fn manifest_path(store: &Store) -> PathBuf {
    extraction_root(store).join(MANIFEST_FILE_NAME)
}

/// Reads and parses the manifest.
fn read_manifest(store: &Store) -> ExtractionManifest {
    let bytes = std::fs::read(manifest_path(store)).expect("read the manifest");
    serde_json::from_slice(&bytes).expect("parse the manifest")
}

/// Writes `manifest` in the manifest file's place.
fn write_manifest(store: &Store, manifest: &ExtractionManifest) {
    let bytes = serde_json::to_vec_pretty(manifest).expect("serialize the manifest");
    std::fs::write(manifest_path(store), bytes).expect("write the manifest");
}

/// The lowercase hex SHA-1 of `bytes`.
fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};

    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Every file below `root`, with its bytes.
fn extracted_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("read the extraction tree") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("read an extracted file");
                files.push((path, bytes));
            }
        }
    }
    files.sort();
    files
}

/// Every path below `root`, directories included.
fn all_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("read the extraction tree") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

/// Every extracted file with its modification time, for no-op assertions.
fn stamps(root: &Path) -> Vec<(PathBuf, SystemTime)> {
    extracted_files(root)
        .into_iter()
        .map(|(path, _)| {
            let modified = std::fs::metadata(&path)
                .expect("metadata")
                .modified()
                .expect("a modification time");
            (path, modified)
        })
        .collect()
}

/// Flips the last byte of `payload`'s stored copy, so the entry's stored
/// checksum can no longer pass and a reader that took the entry's bytes would
/// fail.
fn poison(archive: &mut [u8], payload: &[u8]) {
    let start = archive
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("the payload is stored verbatim");
    archive[start + payload.len() - 1] ^= 0xff;
}

/// Builds a jar in memory. The `deflated` entries are written compressed, the
/// `stored` ones verbatim and then poisoned, and `directories` become
/// directory entries.
///
/// Every stored payload must be unique in the archive, which is what makes
/// the poisoning target unambiguous.
fn build_jar(
    deflated: &[(&str, &[u8])],
    stored: &[(&str, &[u8])],
    directories: &[&str],
) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated_options =
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored_options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for directory in directories {
        writer
            .add_directory(*directory, deflated_options)
            .expect("add a directory entry");
    }
    for (name, bytes) in deflated {
        writer
            .start_file(*name, deflated_options)
            .expect("start an included entry");
        writer.write_all(bytes).expect("write the entry");
    }
    for (name, payload) in stored {
        writer
            .start_file(*name, stored_options)
            .expect("start a stored entry");
        writer.write_all(payload).expect("write the entry");
    }

    let mut archive = writer.finish().expect("finish the jar").into_inner();
    for (_, payload) in stored {
        poison(&mut archive, payload);
    }
    archive
}

/// The synthetic jar: the four includable entries, a class file and a
/// META-INF entry, the latter two refused.
fn synthetic_jar() -> Vec<u8> {
    build_jar(
        &[
            ("assets/minecraft/textures/blocks/stone.png", STONE),
            (
                "assets/minecraft/textures/blocks/stone.png.mcmeta",
                STONE_MCMETA,
            ),
            ("assets/minecraft/lang/en_US.lang", LANG),
            ("pack.mcmeta", PACK_MCMETA),
        ],
        &[
            ("net/minecraft/client/Minecraft.class", CLASS_PAYLOAD),
            ("META-INF/MANIFEST.MF", META_INF_PAYLOAD),
        ],
        &[],
    )
}

#[test]
fn extracts_resources_and_refuses_class_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, VERSION);

    // The plan lists the includable entries and neither refused one.
    let plan = extractor.plan().expect("plan");
    assert_eq!(
        plan,
        vec![
            "assets/minecraft/lang/en_US.lang".to_string(),
            "assets/minecraft/textures/blocks/stone.png".to_string(),
            "assets/minecraft/textures/blocks/stone.png.mcmeta".to_string(),
            "pack.mcmeta".to_string(),
        ],
        "the plan must hold exactly the includable entries"
    );
    assert!(!plan.iter().any(|path| path.contains(".class")));

    // The run writes exactly those four files, with the jar's bytes.
    let report = extractor.run().expect("run");
    assert_eq!(report.entries_read, 6, "four included, two refused");
    assert_eq!(report.extracted, 4);
    assert_eq!(report.skipped, 2);
    assert_eq!(
        report.bytes,
        (STONE.len() + STONE_MCMETA.len() + LANG.len() + PACK_MCMETA.len()) as u64
    );
    assert!(!report.up_to_date);

    let root = extraction_root(&store);
    let expected: [(&str, &[u8]); 4] = [
        ("assets/minecraft/textures/blocks/stone.png", STONE),
        (
            "assets/minecraft/textures/blocks/stone.png.mcmeta",
            STONE_MCMETA,
        ),
        ("assets/minecraft/lang/en_US.lang", LANG),
        ("pack.mcmeta", PACK_MCMETA),
    ];
    for (relative, bytes) in expected {
        let path = root.join(relative);
        assert!(path.is_file(), "{} must be extracted", path.display());
        assert_eq!(std::fs::read(&path).expect("read it"), bytes);
    }

    // No refused path exists anywhere under the extraction root, and no file
    // carries the refused payloads: the class and META-INF entries were never
    // read and never written. The payloads are poisoned, so a reader that
    // took them could not have succeeded either.
    let files = extracted_files(&root);
    for path in all_paths(&root) {
        assert!(
            !path.to_string_lossy().contains(".class"),
            "a class path exists: {}",
            path.display()
        );
        assert!(
            !path.to_string_lossy().contains("META-INF"),
            "a META-INF path exists: {}",
            path.display()
        );
    }
    for (payload, what) in [(CLASS_PAYLOAD, "class"), (META_INF_PAYLOAD, "META-INF")] {
        let prefix = &payload[..payload.len() - 1];
        for (path, bytes) in &files {
            assert!(
                !bytes.windows(prefix.len()).any(|window| window == prefix),
                "the {what} entry's payload was written to {}",
                path.display()
            );
        }
    }

    // A finished extraction is up to date; a different jar is not.
    assert!(extractor.is_up_to_date());
    let changed = build_jar(
        &[
            ("assets/minecraft/textures/blocks/stone.png", STONE),
            ("assets/minecraft/lang/en_US.lang", LANG),
            ("sounds.json", SOUNDS_JSON),
        ],
        &[],
        &[],
    );
    write_jar(&store, &changed);
    assert!(
        !extractor.is_up_to_date(),
        "a changed jar must not be considered extracted"
    );
}

#[test]
fn manifest_records_jar_hash_and_extractor_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    let jar = synthetic_jar();
    write_jar(&store, &jar);
    let extractor = Extractor::new(&store, VERSION);
    extractor.run().expect("run");

    assert_eq!(
        extractor.manifest_path(),
        manifest_path(&store),
        "the manifest's path must be the store layout's"
    );
    assert!(manifest_path(&store).is_file());

    let manifest = read_manifest(&store);
    assert_eq!(
        manifest.jar_sha1,
        sha1_hex(&jar),
        "the manifest records the source jar's SHA-1"
    );
    assert_eq!(manifest.schema_version, EXTRACTOR_SCHEMA_VERSION);
    assert_eq!(
        manifest.entries,
        BTreeMap::from([
            (
                "assets/minecraft/lang/en_US.lang".to_string(),
                LANG.len() as u64
            ),
            (
                "assets/minecraft/textures/blocks/stone.png".to_string(),
                STONE.len() as u64
            ),
            (
                "assets/minecraft/textures/blocks/stone.png.mcmeta".to_string(),
                STONE_MCMETA.len() as u64
            ),
            ("pack.mcmeta".to_string(), PACK_MCMETA.len() as u64),
        ]),
        "the entries map holds every extracted path with its size"
    );
    assert!(
        !manifest
            .entries
            .contains_key("net/minecraft/client/Minecraft.class"),
        "no class path may be recorded"
    );

    // The entries are serialized in sorted order.
    let text = std::fs::read_to_string(manifest_path(&store)).expect("read the manifest text");
    let positions: Vec<usize> = manifest
        .entries
        .keys()
        .map(|key| text.find(key.as_str()).expect("a recorded key"))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the entries must be sorted: {text}"
    );
}

#[test]
fn a_second_run_is_a_no_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, VERSION);

    extractor.run().expect("first run");
    let before = stamps(&extraction_root(&store));
    let manifest_before = std::fs::read(manifest_path(&store)).expect("read the manifest");

    let report = extractor.run().expect("second run");
    assert!(report.up_to_date, "the second run must be skipped");
    assert_eq!(report.entries_read, 0);
    assert_eq!(report.extracted, 0);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.bytes, 0);

    assert_eq!(
        stamps(&extraction_root(&store)),
        before,
        "no extracted file may be rewritten by a skipped run"
    );
    assert_eq!(
        std::fs::read(manifest_path(&store)).expect("read the manifest"),
        manifest_before,
        "the manifest may not be rewritten by a skipped run"
    );
}

#[test]
fn a_damaged_extraction_is_repaired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, VERSION);
    extractor.run().expect("first run");
    let root = extraction_root(&store);

    // A missing file means the extraction is no longer current.
    let stone = root.join("assets/minecraft/textures/blocks/stone.png");
    std::fs::remove_file(&stone).expect("remove the texture");
    assert!(!extractor.is_up_to_date(), "a missing file must not pass");
    let report = extractor.run().expect("re-extract the missing file");
    assert!(!report.up_to_date);
    assert_eq!(std::fs::read(&stone).expect("read it"), STONE);

    // A file of the wrong size means the same, even though it exists.
    let lang = root.join("assets/minecraft/lang/en_US.lang");
    std::fs::write(&lang, b"truncated").expect("damage the language file");
    assert!(
        !extractor.is_up_to_date(),
        "a size mismatch must not pass the cheap check"
    );
    extractor.run().expect("re-extract the resized file");
    assert_eq!(std::fs::read(&lang).expect("read it"), LANG);
    assert!(extractor.is_up_to_date());
}

#[test]
fn a_stale_schema_version_forces_re_extraction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, VERSION);
    extractor.run().expect("run");

    // A manifest from another extractor schema must be redone even though the
    // jar and every file are unchanged.
    let mut manifest = read_manifest(&store);
    manifest.schema_version = EXTRACTOR_SCHEMA_VERSION + 1;
    write_manifest(&store, &manifest);
    assert!(
        !extractor.is_up_to_date(),
        "another schema version must not pass"
    );

    let report = extractor.run().expect("re-extract the whole tree");
    assert!(!report.up_to_date);
    assert_eq!(report.extracted, 4);
    assert_eq!(
        read_manifest(&store).schema_version,
        EXTRACTOR_SCHEMA_VERSION,
        "the fresh manifest carries the current schema"
    );
    assert!(extractor.is_up_to_date());
}

#[test]
fn an_unreadable_manifest_is_re_extracted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    let jar = synthetic_jar();
    write_jar(&store, &jar);
    let extractor = Extractor::new(&store, VERSION);
    extractor.run().expect("run");

    std::fs::write(manifest_path(&store), b"{ this is not a manifest")
        .expect("damage the manifest");
    assert!(
        !extractor.is_up_to_date(),
        "an unreadable manifest must not pass"
    );
    assert_eq!(
        extractor.plan().expect("plan").len(),
        4,
        "the jar is still planned from"
    );

    extractor
        .run()
        .expect("re-extract over the damaged manifest");
    let manifest = read_manifest(&store);
    assert_eq!(manifest.jar_sha1, sha1_hex(&jar));
    assert_eq!(manifest.schema_version, EXTRACTOR_SCHEMA_VERSION);
    assert!(extractor.is_up_to_date());
}

#[test]
fn the_include_and_skip_rules_cover_root_files_and_refuse_the_rest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    let jar = build_jar(
        &[
            ("assets/minecraft/lang/de_DE.lang", LANG),
            ("pack.mcmeta", PACK_MCMETA),
            ("sounds.json", SOUNDS_JSON),
        ],
        &[
            (
                "log4j2.xml",
                b"a root file that is not on the include list (0003)",
            ),
            (
                "net/minecraft/Other.txt",
                b"a non-asset file that is not a class (0004)",
            ),
            (
                "assets/minecraft/textures/gui/PACK.SF",
                b"a signature file under assets (0005)",
            ),
        ],
        &["assets/", "assets/minecraft/"],
    );
    write_jar(&store, &jar);
    let extractor = Extractor::new(&store, VERSION);

    let plan = extractor.plan().expect("plan");
    assert_eq!(
        plan,
        vec![
            "assets/minecraft/lang/de_DE.lang".to_string(),
            "pack.mcmeta".to_string(),
            "sounds.json".to_string(),
        ],
        "directory entries, the signature file and the unlisted files stay out"
    );

    let report = extractor.run().expect("run");
    assert_eq!(
        report.entries_read, 8,
        "three included, three refused and two directory entries"
    );
    assert_eq!(report.extracted, 3);
    assert_eq!(report.skipped, 5);
    let root = extraction_root(&store);
    assert!(root.join("sounds.json").is_file());
    assert!(root.join("pack.mcmeta").is_file());
    assert!(!root.join("log4j2.xml").exists());
    assert!(!root.join("net/minecraft/Other.txt").exists());
    assert!(!root.join("assets/minecraft/textures/gui/PACK.SF").exists());
}

#[test]
fn a_changed_jar_is_extracted_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, VERSION);
    extractor.run().expect("first run");

    // One more resource: a different jar, a new manifest, and the new file.
    let changed = build_jar(
        &[
            ("assets/minecraft/textures/blocks/stone.png", STONE),
            ("assets/minecraft/lang/en_US.lang", LANG),
            ("sounds.json", SOUNDS_JSON),
        ],
        &[],
        &[],
    );
    write_jar(&store, &changed);
    assert!(!extractor.is_up_to_date());

    let report = extractor.run().expect("re-extract the changed jar");
    assert!(!report.up_to_date);
    assert_eq!(report.extracted, 3);
    assert_eq!(read_manifest(&store).jar_sha1, sha1_hex(&changed));
    assert!(extraction_root(&store).join("sounds.json").is_file());
    assert!(extractor.is_up_to_date());
}

#[test]
fn an_entry_that_would_escape_the_extraction_root_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    let jar = build_jar(
        &[("assets/minecraft/lang/en_US.lang", LANG)],
        &[(
            "assets/../../escape.txt",
            b"an entry that would climb out of the tree (0006)",
        )],
        &[],
    );
    write_jar(&store, &jar);
    let extractor = Extractor::new(&store, VERSION);

    let plan = extractor.plan().expect("plan");
    assert_eq!(
        plan,
        vec!["assets/minecraft/lang/en_US.lang".to_string()],
        "an escaping entry is not part of the plan"
    );
    let report = extractor.run().expect("run");
    assert_eq!(report.extracted, 1);
    assert_eq!(report.skipped, 1);

    // The escaping entry stayed out: nothing exists beside the extraction
    // root, neither under the store root nor above the store.
    assert!(!dir.path().join("escape.txt").exists());
    assert!(!store.root().join("escape.txt").exists());
    assert!(!store.root().join("extracted/escape.txt").exists());
}

#[test]
fn a_version_id_that_could_escape_the_store_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, "../1.8.9");

    assert!(
        matches!(extractor.plan(), Err(ExtractError::BadVersion { .. })),
        "a version id that is not a path segment must not be planned with"
    );
    assert!(
        matches!(extractor.run(), Err(ExtractError::BadVersion { .. })),
        "a version id that is not a path segment must not be extracted with"
    );
    assert!(!extractor.is_up_to_date());
    assert!(
        !store.root().join("1.8.9").exists(),
        "nothing may be written outside the extraction directory"
    );
}

#[test]
fn a_failed_extraction_leaves_no_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_at(dir.path());
    write_jar(&store, &synthetic_jar());
    let extractor = Extractor::new(&store, VERSION);
    extractor.run().expect("first run");
    assert!(manifest_path(&store).is_file());

    // A jar whose includable entry cannot be read: the run must fail, and the
    // old manifest must not survive to describe a tree that is not current.
    let broken = build_jar(
        &[
            ("assets/minecraft/lang/en_US.lang", LANG),
            ("assets/minecraft/textures/blocks/stone.png", STONE),
        ],
        &[(
            "assets/minecraft/textures/blocks/broken.png",
            b"an included entry a reader cannot take (0007)",
        )],
        &[],
    );
    write_jar(&store, &broken);
    let error = extractor
        .run()
        .expect_err("a poisoned included entry must fail the run");
    assert!(matches!(error, ExtractError::Entry { .. }), "got {error:?}");
    assert!(
        !manifest_path(&store).exists(),
        "a failed extraction may not leave a manifest behind"
    );
    assert!(!extractor.is_up_to_date());

    // The next run over a jar it can read completes and restores the manifest.
    write_jar(&store, &synthetic_jar());
    extractor.run().expect("recover");
    assert!(manifest_path(&store).is_file());
    assert!(extractor.is_up_to_date());
}
