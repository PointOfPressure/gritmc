//! `PoolAliasBinding` — the table a jigsaw structure resolves pool names
//! through before the walk ever opens one.
//!
//! Reference (26.2 bytecode, `javap -p -c -constants`, never transcribed):
//! `net.minecraft.world.level.levelgen.structure.pools.alias.{PoolAliasLookup,
//! DirectPoolAlias, RandomPoolAlias, RandomGroupPoolAlias}` and
//! `net.minecraft.util.random.WeightedList`.
//!
//! # Why this exists
//!
//! `trial_chambers` is the only structure in 26.2 that uses aliases, and it
//! cannot generate without them: its spawner pools are named
//! `minecraft:trial_chambers/spawner/contents/{ranged,slow_ranged,melee,
//! small_melee}` and **none of those four files exists on disk**. Only
//! `contents/breeze.json` does. The names are virtual and the alias table is
//! the only thing that turns them into a pool that can be loaded. A walk that
//! skips them silently — which is what a missing pool does — builds a chamber
//! with holes where every spawner room should be.
//!
//! # The stream it draws from is not the structure's
//!
//! `JigsawStructure.findGenerationPoint` calls
//! `PoolAliasLookup.create(poolAliases, blockpos, context.seed())`, and
//! `create` is
//!
//! ```text
//!   RandomSource.create(worldSeed).forkPositional().at(blockPos)
//! ```
//!
//! — a **fresh** `LegacyRandomSource` off the world seed, forked positionally
//! and taken at the structure's own start position. It is not the
//! `setLargeFeatureSeed` stream the piece walk runs on, so resolving aliases
//! cannot shift a single draw of the walk. That is worth stating because it is
//! the property that makes this module safe to add: get the alias choice wrong
//! and you get the wrong *spawner variant*; you do not get a different chamber.
//!
//! # The draw
//!
//! `WeightedList.getRandomOrThrow` is `selector.get(random.nextInt(
//! totalWeight))`, and both selector implementations — `Flat`, which expands
//! the entries into a `totalWeight`-long array, and `Compact`, which walks
//! subtracting weights until the running value goes negative — pick the same
//! element for the same draw. `Flat` is used below a total weight of 64 and
//! `Compact` at or above it; the choice is a memory trade and never a
//! behavioural one, so one implementation reproduces both.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::rng::legacy::{LegacyPositionalFactory, LegacyRandomSource};
use crate::rng::{PositionalRandomFactory, RandomSource};
use crate::structure::template_pool::Malformed;

/// One entry of a structure's `pool_aliases` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    /// `minecraft:direct` — a rename with no draw.
    Direct { alias: String, target: String },
    /// `minecraft:random` — one weighted draw picks this alias's target.
    Random {
        alias: String,
        targets: Vec<(String, i32)>,
    },
    /// `minecraft:random_group` — one weighted draw picks a *group* of
    /// bindings, and every binding in it is then resolved against the same
    /// stream. The groups are how vanilla keeps two aliases consistent with
    /// each other: trial chambers picks skeleton, stray or poison_skeleton
    /// **once**, and both the `ranged` and the `slow_ranged` spawner take that
    /// same answer.
    RandomGroup { groups: Vec<(Vec<Binding>, i32)> },
}

/// A `{ "data": .., "weight": n }` entry of a `WeightedList`.
///
/// `Weighted.weight` has no default in the codec, but every entry this port
/// reads writes one, and a missing weight is refused rather than assumed to be
/// 1 — a silently-1 weight would change a draw rather than fail.
fn weighted<'a>(value: &'a Value, what: &str) -> Result<(&'a Value, i32), Malformed> {
    let data = value
        .get("data")
        .ok_or_else(|| Malformed(format!("{what} entry has no `data`")))?;
    let weight = value
        .get("weight")
        .and_then(Value::as_i64)
        .ok_or_else(|| Malformed(format!("{what} entry has no `weight`")))?;
    let weight = i32::try_from(weight)
        .map_err(|_| Malformed(format!("{what} weight {weight} is not an i32")))?;
    Ok((data, weight))
}

impl Binding {
    /// # Errors
    /// If the binding's type is unknown or a field is missing.
    pub fn parse(value: &Value) -> Result<Self, Malformed> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| Malformed("pool alias has no `type`".to_string()))?;
        let string = |key: &str| -> Result<String, Malformed> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| Malformed(format!("pool alias has no `{key}`")))
        };
        match kind {
            "minecraft:direct" => Ok(Self::Direct {
                alias: string("alias")?,
                target: string("target")?,
            }),
            "minecraft:random" => {
                let alias = string("alias")?;
                let list = value
                    .get("targets")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Malformed("`random` alias has no `targets`".to_string()))?;
                let mut targets = Vec::with_capacity(list.len());
                for entry in list {
                    let (data, weight) = weighted(entry, "`random` target")?;
                    let target = data.as_str().ok_or_else(|| {
                        Malformed("`random` target `data` is not a string".to_string())
                    })?;
                    targets.push((target.to_string(), weight));
                }
                Ok(Self::Random { alias, targets })
            }
            "minecraft:random_group" => {
                let list = value
                    .get("groups")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Malformed("`random_group` alias has no `groups`".to_string()))?;
                let mut groups = Vec::with_capacity(list.len());
                for entry in list {
                    let (data, weight) = weighted(entry, "`random_group` group")?;
                    let inner = data.as_array().ok_or_else(|| {
                        Malformed("`random_group` group `data` is not a list".to_string())
                    })?;
                    let bindings = inner
                        .iter()
                        .map(Self::parse)
                        .collect::<Result<Vec<_>, _>>()?;
                    groups.push((bindings, weight));
                }
                Ok(Self::RandomGroup { groups })
            }
            other => Err(Malformed(format!("unknown pool alias type `{other}`"))),
        }
    }

    /// `PoolAliasBinding.forEachResolved`.
    fn for_each_resolved<R: RandomSource>(
        &self,
        random: &mut R,
        out: &mut BTreeMap<String, String>,
    ) {
        match self {
            Self::Direct { alias, target } => {
                out.insert(alias.clone(), target.clone());
            }
            Self::Random { alias, targets } => {
                if let Some(target) = pick(targets, random) {
                    out.insert(alias.clone(), target.clone());
                }
            }
            Self::RandomGroup { groups } => {
                // The group is drawn first, then every binding inside it is
                // resolved against the same stream — `list.forEach(binding ->
                // binding.forEachResolved(random, consumer))`.
                if let Some(group) = pick(groups, random) {
                    for binding in &group.clone() {
                        binding.for_each_resolved(random, out);
                    }
                }
            }
        }
    }
}

/// `WeightedList.getRandomOrThrow` — `selector.get(random.nextInt(total))`.
///
/// `None` is vanilla's throw: a list whose total weight is zero has a null
/// selector. Nothing in 26.2's data reaches it, and returning `None` rather
/// than panicking keeps a malformed pack from taking the generator down.
fn pick<'a, T, R: RandomSource>(items: &'a [(T, i32)], random: &mut R) -> Option<&'a T> {
    let total: i32 = items.iter().map(|(_, w)| *w).sum();
    if total <= 0 {
        return None;
    }
    let mut roll = random.next_i32_bound(total);
    for (item, weight) in items {
        roll -= *weight;
        if roll < 0 {
            return Some(item);
        }
    }
    None
}

/// `PoolAliasLookup.create(bindings, pos, seed)`.
///
/// The returned map is `alias -> target`; a name that is not a key resolves to
/// itself, which is `PoolAliasLookup.lookup`'s `getOrDefault(key, key)`.
///
/// An empty binding list short-circuits to `PoolAliasLookup.EMPTY` **without
/// constructing the random**, exactly as vanilla does — so a structure with no
/// aliases consumes nothing and is bit-identical to one that never had this
/// code.
#[must_use]
pub fn lookup_table(
    bindings: &[Binding],
    pos: (i32, i32, i32),
    seed: i64,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if bindings.is_empty() {
        return out;
    }
    let factory: LegacyPositionalFactory = LegacyRandomSource::new(seed).fork_positional();
    let mut random = factory.at(pos.0, pos.1, pos.2);
    for binding in bindings {
        binding.for_each_resolved(&mut random, &mut out);
    }
    out
}

/// Parse a structure definition's `pool_aliases` list.
///
/// Absent means empty, which is every structure but `trial_chambers`.
///
/// # Errors
/// If the field is present but is not a list of well-formed bindings.
pub fn parse_list(value: Option<&Value>) -> Result<Vec<Binding>, Malformed> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(list)) => list.iter().map(Binding::parse).collect(),
        Some(other) => Err(Malformed(format!("`pool_aliases` is not a list: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).expect("test json")
    }

    #[test]
    fn a_direct_binding_draws_nothing_and_renames() {
        let bindings = parse_list(Some(&json(
            r#"[{"type":"minecraft:direct","alias":"a","target":"b"}]"#,
        )))
        .expect("parses");
        let table = lookup_table(&bindings, (0, 0, 0), 1);
        assert_eq!(table.get("a").map(String::as_str), Some("b"));
        assert_eq!(table.len(), 1, "a direct binding binds exactly one name");
    }

    #[test]
    fn an_empty_list_builds_no_random_and_binds_nothing() {
        assert!(lookup_table(&[], (1, 2, 3), 99).is_empty());
    }

    /// The property the whole module rests on: `pick` must agree with
    /// `Compact.get`, which subtracts weights until the running value goes
    /// negative. Exercised on unequal weights, which trial chambers does not
    /// have — every one of its entries is weight 1, so a bug in the cumulative
    /// arithmetic would be invisible on the real data.
    #[test]
    fn pick_matches_the_cumulative_selector_on_unequal_weights() {
        let items = vec![("a", 2), ("b", 3), ("c", 1)];
        // total = 6; rolls 0,1 -> a; 2,3,4 -> b; 5 -> c.
        let expected = ["a", "a", "b", "b", "b", "c"];
        for (roll, want) in expected.iter().enumerate() {
            let mut fixed = FixedRoll {
                value: i32::try_from(roll).expect("small"),
            };
            assert_eq!(
                pick(&items, &mut fixed).copied(),
                Some(*want),
                "roll {roll}"
            );
        }
    }

    #[test]
    fn a_zero_total_weight_picks_nothing_rather_than_panicking() {
        let items: Vec<(&str, i32)> = vec![("a", 0)];
        let mut fixed = FixedRoll { value: 0 };
        assert_eq!(pick(&items, &mut fixed), None);
    }

    /// A `RandomSource` that returns a fixed bounded draw, so the selector's
    /// arithmetic can be tested without asserting anything about the LCG.
    struct FixedRoll {
        value: i32,
    }

    impl RandomSource for FixedRoll {
        fn set_seed(&mut self, _seed: i64) {}
        fn next_i32(&mut self) -> i32 {
            self.value
        }
        fn next_i32_bound(&mut self, _bound: i32) -> i32 {
            self.value
        }
        fn next_i64(&mut self) -> i64 {
            i64::from(self.value)
        }
        fn next_bool(&mut self) -> bool {
            self.value != 0
        }
        fn next_f32(&mut self) -> f32 {
            0.0
        }
        fn next_f64(&mut self) -> f64 {
            0.0
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }
}
