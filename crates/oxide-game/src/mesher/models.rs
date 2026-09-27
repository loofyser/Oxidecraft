//! The baked-model set: block states joined to the models the mesher draws.
//!
//! [`BlockModelSet::load`] walks the behaviour table's covered ids and every
//! metadata value, resolves each state to its blockstate file and variant key —
//! the state mapper `BlockModelShapes.registerAllBlocks` registers, the same
//! table `tests/behaviour_store.rs` pins against the jar's own files — and bakes
//! the variant's model. The mesher then asks for a state's model by id,
//! metadata and position: the position because a weighted alternative's pick is
//! a function of it (`WeightedBakedModel.getAlternativeModel`).
//!
//! A state that does not resolve — the client's built-in blocks, which have no
//! blockstate file at all, a file the tree does not carry, a variant key the
//! file does not list, a model chain that ends at `builtin/missing` — keeps a
//! [`ModelChoice::Missing`] choice, and the load logs one warning naming every
//! block involved, never one line per block.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use oxide_assets::model::{BakedModel, ModelError, ModelSource};
use oxide_world::behaviour::{BlockBehaviour, behaviour, covered_ids, variant_key};
use tracing::warn;

/// The metadata values one block id's states span.
const METAS: u8 = 16;

/// The model a block state resolves to.
#[derive(Debug)]
pub enum ModelChoice<'a> {
    /// The state's own model.
    Model(&'a BakedModel),
    /// No model: the state is outside the set, its file or key is absent, or its
    /// chain ends at `builtin/missing`. The mesher draws the fallback cube.
    Missing,
}

/// One state's models: the resolution [`BlockModelSet::load`] performed.
#[derive(Debug)]
enum Slot {
    /// One baked model.
    Single(Arc<BakedModel>),
    /// Several weighted alternatives: the file's variant array.
    Alternatives(Alternatives),
    /// No model.
    Missing,
}

/// A variant array's alternatives, sorted and weighted for the pick.
#[derive(Debug)]
struct Alternatives {
    /// Cumulative weights, in the sorted order: entry `i`'s weight plus every
    /// earlier entry's. The last entry's value is the total.
    cumulative: Vec<(u32, Arc<BakedModel>)>,
}

impl Alternatives {
    /// The variant the position picks, exactly as the source does:
    /// `WeightedBakedModel.getAlternativeModel` takes
    /// `abs((int) hash >> 16) % total` over the cumulative weights and returns
    /// the first entry the running total exceeds.
    ///
    /// A list whose weights are all zero has no variant to pick, and the source
    /// would divide by zero; the sorted list's first entry is taken instead, and
    /// the sorted order is the source's own: descending weight, then ascending
    /// quad count, the file's order breaking ties.
    fn pick(&self, x: i32, y: i32, z: i32) -> &BakedModel {
        let (&(total, _), _) = match self.cumulative.split_last() {
            Some(last) => last,
            // A variant array is never empty: the loader refuses an empty one.
            None => unreachable!("an alternatives slot holds at least one model"),
        };
        if total == 0 {
            return &self.cumulative[0].1;
        }
        let index = alternative_index(coordinate_random(x, y, z), total);
        for (cumulative, model) in &self.cumulative {
            if *cumulative > index {
                return model;
            }
        }
        &self.cumulative[self.cumulative.len() - 1].1
    }
}

/// The index a position picks among `total` weights: the source's
/// `MathHelper.abs((int) hash >> 16) % total`.
///
/// The cast takes the hash's low 32 bits and the shift is the sign-preserving
/// one, so the shifted value is an `i32` in `-32768..=32767`: its absolute
/// value cannot overflow, and the shifted value is exactly the source's
/// `(int)hash >> 16`.
fn alternative_index(hash: i64, total: u32) -> u32 {
    let shifted = ((hash as u32) as i32) >> 16;
    (shifted.wrapping_abs() as u32) % total
}

/// `MathHelper.getCoordinateRandom`: the per-position hash the weighted pick
/// reads.
///
/// The x term multiplies in 32-bit arithmetic and widens, the z term in 64-bit;
/// the second line squares the first and adds eleven times it, both read before
/// the assignment, all wrapping.
fn coordinate_random(x: i32, y: i32, z: i32) -> i64 {
    let first = (x.wrapping_mul(3129871)) as i64;
    let mut hash = first ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
    let squared = hash.wrapping_mul(hash).wrapping_mul(42317861);
    let linear = hash.wrapping_mul(11);
    hash = squared.wrapping_add(linear);
    hash
}

/// Every covered state's models, keyed by the packed `(id << 4) | meta`.
#[derive(Debug)]
pub struct BlockModelSet {
    /// The resolved slots. A state outside this map — an id no covered row
    /// names — is [`ModelChoice::Missing`].
    states: BTreeMap<u16, Slot>,
}

impl BlockModelSet {
    /// Resolves every covered state against `models`.
    ///
    /// A state that does not resolve keeps the missing choice and is named in
    /// one warning: the load never fails on a state's behalf, because a missing
    /// blockstate file is the ordinary case for the client's built-in blocks
    /// and a tree the project does not control should degrade to the fallback
    /// rather than stop the client.
    pub fn load(models: &ModelSource) -> BlockModelSet {
        let mut states = BTreeMap::new();
        let mut unresolved: BTreeSet<String> = BTreeSet::new();
        let mut failed: Option<String> = None;
        let mut bakes: BTreeMap<(String, u16, u16, bool), Arc<BakedModel>> = BTreeMap::new();
        for &id in covered_ids() {
            let block = behaviour(id).expect("a covered id has a behaviour row");
            for meta in 0..METAS {
                let slot = match target(block, meta) {
                    Target::Builtin => Slot::Missing,
                    Target::Unreadable => {
                        unresolved.insert(block.name.to_string());
                        Slot::Missing
                    }
                    Target::File(file, key) => {
                        match resolve(models, &file, &key, &mut bakes, &mut failed) {
                            Some(slot) => slot,
                            None => {
                                unresolved.insert(file);
                                Slot::Missing
                            }
                        }
                    }
                };
                states.insert(state_key(id, meta), slot);
            }
        }
        if !unresolved.is_empty() {
            let names: Vec<&str> = unresolved.iter().map(String::as_str).collect();
            warn!(
                blocks = %names.join(", "),
                count = names.len(),
                first_failure = failed.as_deref().unwrap_or("none"),
                "no model resolved for these blocks: every one of their states meshes as the fallback sprite",
            );
        }
        BlockModelSet { states }
    }

    /// The set with no models at all: an asset-less session's set, where every
    /// state is [`ModelChoice::Missing`].
    pub fn empty() -> BlockModelSet {
        BlockModelSet {
            states: BTreeMap::new(),
        }
    }

    /// The model a block state's quad set comes from, chosen for the position.
    ///
    /// `meta` is the metadata half of the packed block value; an id with no
    /// covered row, or any state that did not resolve, answers
    /// [`ModelChoice::Missing`].
    pub fn model(&self, id: u16, meta: u8, x: i32, y: i32, z: i32) -> ModelChoice<'_> {
        match self.states.get(&state_key(id, meta)) {
            Some(Slot::Single(model)) => ModelChoice::Model(model),
            Some(Slot::Alternatives(alternatives)) => {
                ModelChoice::Model(alternatives.pick(x, y, z))
            }
            Some(Slot::Missing) | None => ModelChoice::Missing,
        }
    }

    /// The number of states the set resolved, one per covered id and metadata.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Whether the set holds no states at all.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

/// The packed key of a block state: the wire block value's id and metadata.
fn state_key(id: u16, meta: u8) -> u16 {
    (id << 4) | u16::from(meta)
}

/// Resolves one file and key to a slot, recording the file when it does not
/// resolve. `None` is the unresolved answer: an absent file, a key the file
/// does not list, a model chain that ends at `builtin/missing`, or a variant
/// the baker refused.
fn resolve(
    models: &ModelSource,
    file: &str,
    key: &str,
    bakes: &mut BTreeMap<(String, u16, u16, bool), Arc<BakedModel>>,
    failed: &mut Option<String>,
) -> Option<Slot> {
    let blockstates = match models.blockstates(file) {
        Ok(blockstates) => blockstates,
        Err(ModelError::MissingBlockState { .. }) => return None,
        Err(source) => {
            *failed = Some(format!("{file}: {source}"));
            return None;
        }
    };
    let variants = blockstates.variants.get(key)?;
    let mut baked: Vec<(u32, Arc<BakedModel>)> = Vec::with_capacity(variants.len());
    for variant in variants {
        let cache_key = (variant.model.clone(), variant.x, variant.y, variant.uvlock);
        let model = match bakes.get(&cache_key) {
            Some(model) => Arc::clone(model),
            None => {
                let baked = match models.bake_variant(variant) {
                    Ok(baked) => Arc::new(baked),
                    Err(source) => {
                        *failed = Some(format!("{}: {source}", variant.model));
                        return None;
                    }
                };
                if baked.missing {
                    // The chain ends at the client's missing model, which has no
                    // quads: the state draws the mesher's fallback cube instead.
                    return None;
                }
                bakes.insert(cache_key, Arc::clone(&baked));
                baked
            }
        };
        baked.push((variant.weight, model));
    }
    if baked.iter().all(|(weight, _)| *weight == 0) {
        // No variant can be picked; the sorted order's first entry is taken.
        baked.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.quads.len().cmp(&b.1.quads.len()))
        });
        let (_, model) = baked.remove(0);
        return Some(Slot::Single(model));
    }
    // Descending weight, then ascending quad count; the sort is stable, so the
    // file's own order breaks ties.
    baked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.quads.len().cmp(&b.1.quads.len()))
    });
    if baked.len() == 1 {
        let (_, model) = baked.remove(0);
        return Some(Slot::Single(model));
    }
    let mut cumulative = Vec::with_capacity(baked.len());
    let mut total = 0u32;
    for (weight, model) in baked {
        total = total.saturating_add(weight);
        cumulative.push((total, model));
    }
    Some(Slot::Alternatives(Alternatives { cumulative }))
}

/// What a state resolves to before the tree is read.
#[derive(Debug)]
enum Target {
    /// A blockstate file and the variant key inside it.
    File(String, String),
    /// A block the client builds in, with no blockstate file at all.
    Builtin,
    /// A state whose key does not carry the property the mapper names the file
    /// after. No covered state does this — `tests/behaviour_store.rs` asserts
    /// it against the jar — so the answer is a fallback rather than a panic.
    Unreadable,
}

/// The `(file, key)` a block state resolves to, or `None` for a state with no
/// blockstate file: the client's built-in blocks and a state the mapper cannot
/// read.
///
/// The pair is the join `BlockModelSet::load` performs, exposed on its own so
/// the mapping's literals can be asserted without a model tree.
pub fn blockstate_target(block: &BlockBehaviour, meta: u8) -> Option<(String, String)> {
    match target(block, meta) {
        Target::File(file, key) => Some((file, key)),
        Target::Builtin | Target::Unreadable => None,
    }
}

/// The state mapper `BlockModelShapes.registerAllBlocks` registers, read as the
/// file and key it names for a state.
fn target(block: &BlockBehaviour, meta: u8) -> Target {
    let key = variant_key(block, meta);
    let pairs = pairs(&key);
    match mapper(block.id) {
        Mapper::Builtin => Target::Builtin,
        Mapper::Plain => Target::File(block.name.to_string(), normalised(&key)),
        Mapper::Name {
            prop,
            suffix,
            ignored,
        } => {
            let Some(value) = pairs.get(prop) else {
                return Target::Unreadable;
            };
            let mut dropped = ignored.to_vec();
            dropped.push(prop);
            Target::File(format!("{value}{suffix}"), dropped_key(&pairs, &dropped))
        }
        Mapper::Ignore { ignored } => {
            Target::File(block.name.to_string(), dropped_key(&pairs, ignored))
        }
        Mapper::Dirt => {
            let Some(value) = pairs.get("variant") else {
                return Target::Unreadable;
            };
            let dropped: &[&str] = if *value == "podzol" {
                &["variant"]
            } else {
                &["variant", "snowy"]
            };
            Target::File(value.to_string(), dropped_key(&pairs, dropped))
        }
        Mapper::DoubleSlab => {
            let Some(value) = pairs.get("variant") else {
                return Target::Unreadable;
            };
            let seamless = pairs.get("seamless").is_some_and(|value| *value == "true");
            let key = if seamless { "all" } else { "normal" };
            Target::File(format!("{value}_double_slab"), key.to_string())
        }
        Mapper::Quartz => match pairs.get("variant").copied() {
            Some("default") => Target::File("quartz_block".to_string(), "normal".to_string()),
            Some("chiseled") => {
                Target::File("chiseled_quartz_block".to_string(), "normal".to_string())
            }
            Some(axis @ ("lines_x" | "lines_y" | "lines_z")) => Target::File(
                "quartz_column".to_string(),
                format!("axis={}", axis.trim_start_matches("lines_")),
            ),
            _ => Target::Unreadable,
        },
        Mapper::DeadBush => Target::File("dead_bush".to_string(), normalised(&key)),
    }
}

/// The mapper the client registers for a covered id.
///
/// The arms mirror `BlockModelShapes.registerAllBlocks`: a `StateMap` names the
/// file after one property's value plus a suffix and drops named properties from
/// the key (`BlockModelShapes.java:215-330`), and every other block uses the
/// registry name with the whole property string (`DefaultStateMapper`).
/// `registerBuiltInBlocks` names the five covered ids with no blockstate file.
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
        8 | 9 | 10 | 11 | 54 => Mapper::Builtin,
        _ => Mapper::Plain,
    }
}

/// How the client's state mapper names the blockstate file and the variant key.
#[derive(Debug, Clone, Copy)]
enum Mapper {
    /// The registry name and the full property string.
    Plain,
    /// The value of `prop` (plus `suffix`) names the file; `ignored` leaves the
    /// key.
    Name {
        /// The property whose value names the file.
        prop: &'static str,
        /// The mapper's suffix between the value and `.json`.
        suffix: &'static str,
        /// Properties the mapper removes from the key.
        ignored: &'static [&'static str],
    },
    /// The registry name, with properties removed from the key.
    Ignore {
        /// Properties the mapper removes from the key.
        ignored: &'static [&'static str],
    },
    /// Dirt: the variant value names the file, and `snowy` survives only for
    /// podzol.
    Dirt,
    /// The double stone slab: `<variant>_double_slab`, key `normal` or `all`.
    DoubleSlab,
    /// Quartz: the block, the chiseled block, and the pillar's own axis keys.
    Quartz,
    /// The dead bush: always `dead_bush` with the whole property string.
    DeadBush,
    /// A built-in block with no blockstate file at all.
    Builtin,
}

/// One `variant_key` string's `name=value` pairs, in name order.
fn pairs(key: &str) -> BTreeMap<&str, &str> {
    key.split(',')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.split_once('='))
        .collect()
}

/// The key a state's remaining properties spell: the pairs in name order, or
/// the client's `normal` when no property remains.
fn dropped_key(pairs: &BTreeMap<&str, &str>, dropped: &[&str]) -> String {
    let mut key = String::new();
    for (name, value) in pairs {
        if dropped.contains(name) {
            continue;
        }
        if !key.is_empty() {
            key.push(',');
        }
        key.push_str(name);
        key.push('=');
        key.push_str(value);
    }
    normalised(&key)
}

/// A property string as the blockstate files spell the empty one.
fn normalised(key: &str) -> String {
    if key.is_empty() {
        "normal".to_string()
    } else {
        key.to_string()
    }
}
