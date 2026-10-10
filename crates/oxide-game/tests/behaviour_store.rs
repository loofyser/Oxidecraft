//! The covered ids against the jar's own blockstate files. Ignored by default:
//! it needs the extraction tree of a store, named by `OXIDECRAFT_STORE`.
//!
//! For every covered id and every metadata value `0..16`, the production state
//! mapper — [`blockstate_target`], the join `BlockModelSet::load` performs —
//! must name a blockstate file the tree holds and a variant key that file
//! carries. The mapper is `BlockModelShapes.registerAllBlocks`
//! (`refs/_src/MCP-919/src/minecraft/net/minecraft/client/renderer/BlockModelShapes.java`)
//! read as data: a `StateMap` names the file after one property's value plus a
//! suffix and drops named properties from the key, and the remaining blocks use
//! the registry name with the whole property string (`DefaultStateMapper`).
//!
//! Eight covered ids are the client's built-in blocks, which have no blockstate
//! file at all: `BlockModelShapes.registerBuiltInBlocks` names `flowing_water`,
//! `water`, `flowing_lava`, `lava`, `chest`, `standing_sign`, `wall_sign` and
//! `barrier` (with the other chests, skulls and banners). Their resolution is
//! asserted as the file's absence instead; the mesher routes the four liquids
//! away from the model path (`RenderKind::Liquid`), draws nothing for the
//! barrier (`RenderKind::Invisible`) and synthesises the chest's closed model
//! and the two sign boards from the block-entity renderers (fix3b).
//!
//! A variant whose only difference from the answer is `uvlock` still satisfies
//! the check: the test compares keys, not the variants' own rotation, weight or
//! lock values.

use std::collections::BTreeMap;
use std::path::Path;

use oxide_assets::model::ModelSource;
use oxide_game::mesher::blockstate_target;
use oxide_world::behaviour::{RenderKind, behaviour, covered_ids, variant_key};

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
    let mut jar_gap = 0usize;
    let mut files = BTreeMap::<String, usize>::new();
    for &id in covered_ids() {
        let block = behaviour(id).expect("the id is covered");
        for meta in 0..16u8 {
            match blockstate_target(block, meta) {
                Some((file, key)) => {
                    // The hopper's jar gap: `facing=up` has no variant in
                    // hopper.json because the source cannot construct that
                    // state at all (`BlockHopper.FACING` excludes UP, so
                    // `getStateFromMeta(1|7|9|15)` violates the property). The
                    // load keeps the missing choice there and the mesher
                    // draws the fallback, the missing model's port.
                    if id == 154 && (meta & 7 == 1 || meta & 7 == 7) {
                        assert_eq!(key, "facing=up", "the hopper's unconstructible facing");
                        let states = source
                            .blockstates(&file)
                            .unwrap_or_else(|error| panic!("id {id} meta {meta}: {error:?}"));
                        assert!(
                            !states.variants.contains_key(&key),
                            "id {id} meta {meta}: the jar grew a `facing=up` variant"
                        );
                        jar_gap += 1;
                        continue;
                    }
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
                    // No file. The eight built-in ids are the only covered states
                    // that may answer so: a state the mapper cannot read — a key
                    // without the property its file is named after — would land
                    // here too, and the exact counts below would catch it.
                    assert!(
                        matches!(id, 8 | 9 | 10 | 11 | 54 | 63 | 68 | 166),
                        "id {id} ({}) meta {meta} resolves to no file, but it is not built in; \
                         its key is `{}`",
                        block.name,
                        variant_key(block, meta)
                    );
                    // A built-in block has no blockstate file in the jar at all.
                    let absent = source.blockstates(block.name);
                    assert!(
                        absent.is_err(),
                        "id {id} ({}) is a built-in block, so the tree has no {}.json",
                        block.name,
                        block.name
                    );
                    if id == 54 || id == 63 || id == 68 {
                        assert_eq!(
                            block.render,
                            RenderKind::Model,
                            "id {id} has no blockstate file and meshes its synthesised model"
                        );
                    } else if id == 166 {
                        assert_eq!(
                            block.render,
                            RenderKind::Invisible,
                            "the barrier is a built-in block with no renderer: it draws nothing"
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
        resolved + built_in + jar_gap,
        covered_ids().len() * 16,
        "every covered state is accounted for"
    );
    assert_eq!(
        resolved,
        76 * 16 - 4,
        "the 76 covered ids with a blockstate file, minus the hopper's jar gap"
    );
    assert_eq!(built_in, 8 * 16, "the client's eight built-in covered ids");
    assert_eq!(
        jar_gap, 4,
        "hopper metas 1, 7, 9 and 15: the unconstructible facing=up"
    );
    println!(
        "behaviour store: {resolved} of {} (id, meta) pairs resolved as variant keys in \
         {} blockstate files; {built_in} states are the client's built-in blocks; \
         {jar_gap} are the hopper's jar gap",
        covered_ids().len() * 16,
        files.len()
    );
}
