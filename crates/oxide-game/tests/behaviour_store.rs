//! The covered ids against the jar's own blockstate files. Ignored by default:
//! it needs the extraction tree of a store, named by `OXIDECRAFT_STORE`.
//!
//! For every covered id and every metadata value `0..16`, the behaviour table's
//! `variant_key` must resolve to a variant key that exists in the blockstate file
//! the client's own state mapper selects for that state. The mapper is mirrored
//! here from `BlockModelShapes.registerAllBlocks`
//! (`refs/_src/MCP-919/src/minecraft/net/minecraft/client/renderer/BlockModelShapes.java`):
//! a `StateMap` names the file after one property's value plus a suffix and drops
//! named properties from the key, and the remaining blocks use the registry name
//! with the whole property string (`DefaultStateMapper`). Task 8's `BlockModelSet`
//! is where the real join lands; this test is the join's first consumer.
//!
//! Five covered ids are the client's built-in blocks, which have no blockstate
//! file at all: `BlockModelShapes.registerBuiltInBlocks` names `flowing_water`,
//! `water`, `flowing_lava`, `lava` and `chest` (with the other chests, signs and
//! skulls). Their resolution is asserted as the file's absence instead; the
//! mesher routes the four liquids away from the model path (`RenderKind::Liquid`)
//! and the chest falls back to the magenta sprite until a block-entity renderer
//! exists (Decision 8).
//!
//! A variant whose only difference from the answer is `uvlock` still satisfies
//! the check: the test compares keys, not the variants' own rotation, weight or
//! lock values.

use std::collections::BTreeMap;
use std::path::Path;

use oxide_assets::model::ModelSource;
use oxide_world::behaviour::{RenderKind, behaviour, covered_ids, variant_key};

/// How the client's state mapper names the blockstate file and the variant key.
enum Mapper {
    /// The registry name and the full property string.
    Plain,
    /// The value of `prop` (plus `suffix`) names the file; `ignored` leaves the key.
    Name {
        /// The property whose value names the file.
        prop: &'static str,
        /// The mapper's suffix between the value and `.json`.
        suffix: &'static str,
        /// Properties the mapper removes from the variant key.
        ignored: &'static [&'static str],
    },
    /// The registry name, with properties removed from the key.
    Ignore {
        /// Properties the mapper removes from the variant key.
        ignored: &'static [&'static str],
    },
    /// Dirt: the variant value names the file, and `snowy` survives only for podzol.
    Dirt,
    /// The double stone slab: `<variant>_double_slab`, key `normal` or `all`.
    DoubleSlab,
    /// Quartz: the block, the chiseled block, and the pillar's own axis keys.
    Quartz,
    /// The dead bush: always `dead_bush` with the whole property string.
    DeadBush,
    /// A built-in block with no blockstate file at all.
    BuiltIn,
}

/// The mapper `BlockModelShapes` registers for a covered id.
fn mapper(id: u16) -> Mapper {
    match id {
        1 | 12 | 98 => Mapper::Name {
            prop: "variant",
            suffix: "",
            ignored: &[],
        },
        5 => Mapper::Name {
            prop: "variant",
            suffix: "_planks",
            ignored: &[],
        },
        17 | 162 => Mapper::Name {
            prop: "variant",
            suffix: "_log",
            ignored: &[],
        },
        18 | 161 => Mapper::Name {
            prop: "variant",
            suffix: "_leaves",
            ignored: &["check_decay", "decayable"],
        },
        24 | 31 | 37 | 38 => Mapper::Name {
            prop: "type",
            suffix: "",
            ignored: &[],
        },
        35 => Mapper::Name {
            prop: "color",
            suffix: "_wool",
            ignored: &[],
        },
        175 => Mapper::Name {
            prop: "variant",
            suffix: "",
            ignored: &["facing"],
        },
        46 => Mapper::Ignore {
            ignored: &["explode"],
        },
        64 => Mapper::Ignore {
            ignored: &["powered"],
        },
        81 | 83 => Mapper::Ignore { ignored: &["age"] },
        3 => Mapper::Dirt,
        43 => Mapper::DoubleSlab,
        155 => Mapper::Quartz,
        32 => Mapper::DeadBush,
        8 | 9 | 10 | 11 | 54 => Mapper::BuiltIn,
        _ => Mapper::Plain,
    }
}

/// One `variant_key` string's `name=value` pairs.
fn pairs(id: u16, meta: u8, key: &str) -> BTreeMap<&str, &str> {
    key.split(',')
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.split_once('=')
                .unwrap_or_else(|| panic!("id {id} meta {meta}: `{part}` is not a name=value pair"))
        })
        .collect()
}

/// The key a state's remaining properties spell: the pairs in name order, or the
/// client's `normal` when no property remains (`StateMapperBase.getPropertyString`).
fn key_of(pairs: &BTreeMap<&str, &str>, dropped: &[&str]) -> String {
    let mut body = String::new();
    for (name, value) in pairs {
        if dropped.contains(name) {
            continue;
        }
        if !body.is_empty() {
            body.push(',');
        }
        body.push_str(name);
        body.push('=');
        body.push_str(value);
    }
    if body.is_empty() {
        "normal".to_string()
    } else {
        body
    }
}

/// The `(file, key)` a covered `(id, meta)` state resolves to, or `None` for the
/// client's built-in blocks, which have no blockstate file.
fn resolve(id: u16, meta: u8) -> Option<(String, String)> {
    let block = behaviour(id).unwrap_or_else(|| panic!("id {id} is covered"));
    let key = variant_key(block, meta);
    let pairs = pairs(id, meta, &key);
    match mapper(id) {
        Mapper::BuiltIn => None,
        Mapper::Plain => {
            let key = if key.is_empty() {
                "normal".to_string()
            } else {
                key
            };
            Some((block.name.to_string(), key))
        }
        Mapper::Name {
            prop,
            suffix,
            ignored,
        } => {
            let value = pairs.get(prop).unwrap_or_else(|| {
                panic!("id {id} meta {meta}: the key `{key}` has no `{prop}` property")
            });
            let mut dropped = ignored.to_vec();
            dropped.push(prop);
            Some((format!("{value}{suffix}"), key_of(&pairs, &dropped)))
        }
        Mapper::Ignore { ignored } => Some((block.name.to_string(), key_of(&pairs, ignored))),
        Mapper::Dirt => {
            let value = pairs.get("variant").unwrap_or_else(|| {
                panic!("id {id} meta {meta}: the key `{key}` has no `variant` property")
            });
            let dropped: &[&str] = if *value == "podzol" {
                &["variant"]
            } else {
                &["variant", "snowy"]
            };
            Some((value.to_string(), key_of(&pairs, dropped)))
        }
        Mapper::DoubleSlab => {
            let value = pairs.get("variant").unwrap_or_else(|| {
                panic!("id {id} meta {meta}: the key `{key}` has no `variant` property")
            });
            let seamless = pairs
                .get("seamless")
                .map(|value| *value == "true")
                .unwrap_or(false);
            Some((
                format!("{value}_double_slab"),
                if seamless { "all" } else { "normal" }.to_string(),
            ))
        }
        Mapper::Quartz => match pairs.get("variant").copied() {
            Some("default") => Some(("quartz_block".to_string(), "normal".to_string())),
            Some("chiseled") => Some(("chiseled_quartz_block".to_string(), "normal".to_string())),
            Some("lines_y") => Some(("quartz_column".to_string(), "axis=y".to_string())),
            Some("lines_x") => Some(("quartz_column".to_string(), "axis=x".to_string())),
            Some("lines_z") => Some(("quartz_column".to_string(), "axis=z".to_string())),
            other => panic!("id {id} meta {meta}: unexpected quartz variant {other:?}"),
        },
        Mapper::DeadBush => {
            let key = if key.is_empty() {
                "normal".to_string()
            } else {
                key
            };
            Some(("dead_bush".to_string(), key))
        }
    }
}

#[test]
#[ignore = "needs a store: set OXIDECRAFT_STORE to the store root (for example ~/.local/share/oxidecraft)"]
fn every_covered_state_resolves_against_the_jar_blockstates() {
    let store = std::env::var("OXIDECRAFT_STORE").expect(
        "OXIDECRAFT_STORE must name the store root that holds extracted/ (for example \
         ~/.local/share/oxidecraft); this test does not pass without a store",
    );
    let root = Path::new(&store).join("extracted").join("1.8.9");
    let source = ModelSource::open(&root).expect("the real tree opens");

    let mut resolved = 0usize;
    let mut built_in = 0usize;
    let mut files = BTreeMap::<String, usize>::new();
    for &id in covered_ids() {
        let block = behaviour(id).expect("the id is covered");
        for meta in 0..16u8 {
            match resolve(id, meta) {
                Some((file, key)) => {
                    let states = source
                        .blockstates(&file)
                        .unwrap_or_else(|error| panic!("id {id} meta {meta}: {error:?}"));
                    assert!(
                        states.variants.contains_key(&key),
                        "id {id} ({}) meta {meta}: {file}.json has no variant `{key}`; \
                         the table's key is `{}`",
                        block.name,
                        variant_key(block, meta)
                    );
                    *files.entry(file).or_default() += 1;
                    resolved += 1;
                }
                None => {
                    // A built-in block has no blockstate file in the jar at all.
                    let absent = source.blockstates(block.name);
                    assert!(
                        absent.is_err(),
                        "id {id} ({}) is a built-in block, so the tree has no {}.json",
                        block.name,
                        block.name
                    );
                    if id == 54 {
                        assert_eq!(
                            block.render,
                            RenderKind::Model,
                            "the chest has no blockstate file and its block-entity renderer is not M2's"
                        );
                    } else {
                        assert_eq!(
                            block.render,
                            RenderKind::Liquid,
                            "id {id} is a liquid and takes the liquid path"
                        );
                    }
                    built_in += 1;
                }
            }
        }
    }

    assert_eq!(
        resolved + built_in,
        covered_ids().len() * 16,
        "every covered state is accounted for"
    );
    assert_eq!(
        resolved,
        68 * 16,
        "the 68 covered ids with a blockstate file"
    );
    assert_eq!(built_in, 5 * 16, "the client's five built-in covered ids");
    println!(
        "behaviour store: {resolved} of {} (id, meta) pairs resolved as variant keys in \
         {} blockstate files; {built_in} states are the client's built-in blocks",
        covered_ids().len() * 16,
        files.len()
    );
}
