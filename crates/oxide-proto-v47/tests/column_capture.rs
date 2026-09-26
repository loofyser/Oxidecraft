//! Decodes the real column payloads captured from the rig server (Task 2) and
//! asserts the invariants the capture's manifest records.

use serde_json::Value;

const MANIFEST: &str = include_str!("fixtures/m1-capture/manifest.json");

/// The absolute path of a committed fixture file.
///
/// Resolved from the crate root, not the test process's working directory, so
/// the test reads the committed bytes wherever it is run from.
fn fixture_path(file: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m1-capture")
        .join(file)
}

#[test]
fn the_manifest_parses_and_every_column_decodes() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("manifest parses");
    let columns = manifest["columns"].as_array().expect("columns array");
    assert!(
        !columns.is_empty(),
        "the capture produced at least one column"
    );
    for column in columns {
        let file = column["file"].as_str().expect("file name");
        let bytes = std::fs::read(fixture_path(file)).expect("fixture file");
        let mask = u16::from_str_radix(
            column["mask"]
                .as_str()
                .expect("mask")
                .trim_start_matches("0x"),
            16,
        )
        .expect("mask parses");
        let sky = column["sky"].as_bool().expect("sky flag");
        let ground_up = column["ground_up"].as_bool().expect("ground-up flag");
        assert_eq!(bytes.len(), column["size"].as_u64().expect("size") as usize);
        let decoded = oxide_proto_v47::column::parse_column(&bytes, mask, sky, ground_up)
            .unwrap_or_else(|error| panic!("{file} does not decode: {error}"));
        assert_eq!(decoded.mask, mask);
        // Every set mask bit has a section and every clear bit has none.
        let present: Vec<bool> = decoded.sections.iter().map(Option::is_some).collect();
        let expected: Vec<bool> = (0..16).map(|bit| mask >> bit & 1 == 1).collect();
        assert_eq!(present, expected, "{file}: sections follow the mask");
        // The biome array is carried exactly when the packet is ground-up.
        assert_eq!(
            decoded.biomes.is_some(),
            ground_up,
            "{file}: biomes follow the ground-up flag"
        );
        // A vanilla world has bedrock at the bottom of the spawn column.
        if let Some(section) = decoded.sections[0].as_ref() {
            let bedrock = column["block_counts"]["7"].as_u64().unwrap_or(0);
            if bedrock > 0 {
                assert_eq!(
                    section.blocks[0] >> 4,
                    7,
                    "{file}: the first block is bedrock in a generated world"
                );
                // The bottom layer is solid bedrock, and the capture's bedrock
                // count over the whole column must match the decode.
                assert!(
                    section.blocks[..256].iter().all(|value| value >> 4 == 7),
                    "{file}: the bottom layer is bedrock"
                );
                let decoded_bedrock = decoded
                    .sections
                    .iter()
                    .flatten()
                    .flat_map(|section| section.blocks.iter())
                    .filter(|value| *value >> 4 == 7)
                    .count() as u64;
                assert_eq!(
                    decoded_bedrock, bedrock,
                    "{file}: the decoded bedrock count matches the capture"
                );
            }
        }
        // The capture's recorded table carries one count for every block slot
        // of every section the mask declares.
        let recorded = column["block_counts"].as_object().expect("block counts");
        let total: u64 = recorded
            .values()
            .map(|count| count.as_u64().expect("count"))
            .sum();
        assert_eq!(
            total,
            4096 * u64::from(mask.count_ones()),
            "{file}: the recorded counts cover every section slot"
        );
    }
}

#[test]
fn the_manifest_records_the_capture_provenance() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("manifest parses");
    assert_eq!(manifest["server"]["version"], "1.8.9");
    assert_eq!(
        manifest["server"]["jar_sha1"],
        "b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd"
    );
    assert!(manifest["packets"]["0x21"].as_u64().is_some());
}
