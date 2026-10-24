//! Surfaces — what covers the terrain the noise stage produced.
//!
//! Reference: `net.minecraft.world.level.levelgen.SurfaceSystem` and
//! `SurfaceRules`.

pub mod bands;
pub mod conditions;
pub mod context;
pub mod eval;
pub mod iceberg;
pub mod rule;
pub mod system;
pub mod world;

/// A biome, interned to an integer for the surface stage's inner loop.
///
/// # Why the rules do not carry the name
///
/// Vanilla's `SurfaceRules$Context.getBiome` memoises a `Holder<Biome>` per
/// y-update and every `isBiome` condition compares **holder identity**. Ours
/// memoised the same answer at the same moment — the call counts match one
/// for one — but carried it as a `String`, so each of the ~28,864 answers a
/// chunk needed a 25-byte heap allocation, a `BTreeMap<String, _>` walk to
/// find its climate, and a byte-for-byte comparison against each name a
/// condition lists. Every one of those names begins `minecraft:`, so a
/// comparison reads ten equal bytes before it can decide anything.
///
/// This is the same defect `block_name` had (134 million allocations) and it
/// is fixed the same way: resolve the identity once, carry an integer.
///
/// # Why this cannot move a block
///
/// The id is an index into a palette built from the datapack's biome
/// directory — the complete registry, in `BTreeMap` order, fixed before the
/// stage starts and never appended to. Interning is injective, so
/// `id(a) == id(b)` exactly when `a == b`, and every comparison the surface
/// rules make is an equality. That is why the change from names to ids is
/// output-identical, and it was proved by byte-comparing region files
/// against a pristine control at one worker and at twelve.
///
/// # Nothing depends on WHICH integer a biome gets — and this said the
/// # opposite for three days
///
/// A falsification sweep on 2026-08-06 rebuilt this palette from the same
/// registry in reverse, expecting no change, and reported **4 of 4 region
/// files changed**. It named no consumer. The honest response then was to
/// stop claiming the opposite and warn, so this block said the order was
/// load-bearing and that some unidentified consumer made it so.
///
/// **Re-measured 2026-08-09 and the 4-of-4 does not reproduce.** The new
/// instrument differs from the old one in a way that matters:
/// `RUSTMC_BIOME_PALETTE_ORDER` picks the order at **run time**, so the
/// control and the injection are the same executable and no build difference
/// can enter. It also splits the question the old injection conflated —
/// `reverse` moves an id's *value* and its *order* at once, `shift N` moves
/// only the value — and it carries an arm that must fire.
///
/// Seed 20260722, r=16, centre (64,64), `RUSTMC_WORKERS=1`, Threadripper —
/// the same window the 2026-08-06 claim was made on:
///
/// ```text
///   forward vs forward     0 of 4    determinism control, same binary
///   reverse                0 of 4    values AND order moved
///   shift 1                0 of 4    values moved, order preserved
///   shift 7                0 of 4
///   poison (every id 0)    4 of 4    POSITIVE CONTROL -- must not be 0
/// ```
///
/// **`poison` is why the zeros mean something.** A knob that silently did
/// nothing would report every injection as undetectable, which is rule 7's
/// "you measured the instrument". It answers every biome query with id 0 and
/// changes 4 of 4, so the palette this perturbs is the live one and this
/// window does resolve a wrong biome id. The three palette digests differ
/// (`4462c36f462d1ad9`, `75604bcabe2ee445`, `ad374a1f42e1a584`), which is a
/// second, independent way for a dead knob to be caught. Also 0 of 4 at r=7
/// on the same centre, and at r=7 centred on (-6000,-6000), which is the
/// centre `docs/BENCH.md` names as the one that reaches the climate path at
/// all.
///
/// # Why it is undetectable, from vanilla's side
///
/// Vanilla has **no numeric biome identifier on the generation path**, so
/// there is nothing there for an order to be load-bearing *for*.
/// `SurfaceRules$BiomeConditionSource$1BiomeCondition.compute` is
/// `HolderSet.contains(Holder)` — `Set.contains` on holder objects for a
/// direct set, `Holder.is(TagKey)` for a named one. `BiomeManager.getBiome`
/// returns a `Holder<Biome>` throughout and its fuzz tie-break,
/// `getFiddledDistance(J,I,I,D,D,D)`, is seeded from **position** only. Ours
/// is the exact analogue: `Condition::BiomeIds` is `Vec::contains`, an
/// equality. The integer is an implementation device with no vanilla
/// counterpart, and our behaviour matches vanilla's precisely because both
/// sides can only ask "is it this biome".
///
/// # What is still true, and what is no longer
///
/// * Building from an ordered source (`BTreeMap` keys, which
///   `SurfaceBiomes::new` uses) stays. It is **cheap insurance, not a
///   correctness requirement**: it costs nothing and it keeps ids
///   reproducible run to run, which is what makes the digest above worth
///   printing.
/// * A change to the build order is **no longer** a change to worldgen output
///   needing a byte-compare. That claim is retracted.
/// * The guard is now mechanical rather than advisory:
///   `ci/biome-palette-order.sh` is the dump-level gate, and
///   `eval::relabelling_is_undetectable` is the T0 unit-test form that runs on
///   every gate in microseconds. Either goes red if a `<`, a `min`/`max`, a
///   `binary_search`, a hash of an id, or an id indexing a table built outside
///   the palette's own loop ever appears.
pub const ORDER_IS_NOT_OBSERVABLE: () = ();
///
/// `u16` rather than `u32` because the registry is ~64 biomes and the id
/// travels inside `SurfaceContext`, which is cloned per column.
pub type BiomeId = u16;

/// The id no biome has, used by the stub oracles that never answer a biome
/// condition. A real palette never issues it, so a rule that reaches one of
/// those stubs compares against nothing and is `false` — which is what the
/// `String` stubs did by returning a name no condition lists.
pub const NO_BIOME: BiomeId = BiomeId::MAX;

/// Names to [`BiomeId`]s and back.
///
/// # Build it eagerly on the generating path
///
/// The surface stage's interior runs on every core against a shared,
/// immutable `SurfaceShared`. A palette that grew as it was asked would need
/// a lock on the hottest call in the stage, which is how a previous
/// optimisation here turned into a contention tax — an `Arc` clone per block
/// written cost more than the allocation it removed. So the generating path
/// builds this once from the datapack's complete biome directory, in
/// `BTreeMap` order, before the stage starts, and never touches it again.
///
/// [`Self::intern`] exists for tests and other single-threaded callers that
/// do not have the registry to hand. It is not used on the generating path.
#[derive(Clone, Debug, Default)]
pub struct BiomePalette {
    names: Vec<String>,
    index: std::collections::BTreeMap<String, BiomeId>,
}

impl BiomePalette {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a complete name list. Ids follow the iteration order.
    ///
    /// Pass something ordered — a `BTreeMap`'s keys — so a seed reproduces
    /// its ids run to run. That is reproducibility housekeeping and **not** a
    /// correctness requirement: relabelling is measured undetectable in the
    /// generated world, see [`ORDER_IS_NOT_OBSERVABLE`].
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut palette = Self::new();
        for name in names {
            palette.intern(&name.into());
        }
        palette
    }

    /// The id for a name, adding it if it is new.
    pub fn intern(&mut self, name: &str) -> BiomeId {
        if let Some(id) = self.index.get(name) {
            return *id;
        }
        let id = BiomeId::try_from(self.names.len())
            .expect("a biome registry with more than 65,535 entries");
        assert!(id != NO_BIOME, "biome palette exhausted");
        self.names.push(name.to_string());
        self.index.insert(name.to_string(), id);
        id
    }

    #[must_use]
    pub fn id_of(&self, name: &str) -> Option<BiomeId> {
        self.index.get(name).copied()
    }

    /// The name behind an id. Tracing and diagnostics only.
    #[must_use]
    pub fn name(&self, id: BiomeId) -> &str {
        self.names
            .get(id as usize)
            .map_or("<unknown>", |s| s.as_str())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod palette_tests {
    use super::BiomePalette;

    /// The whole safety argument for carrying ids instead of names rests on
    /// interning being injective: every comparison the surface rules make is
    /// an equality, so `id(a) == id(b)` must hold exactly when `a == b`.
    #[test]
    fn interning_is_injective_and_stable() {
        let mut p = BiomePalette::new();
        let plains = p.intern("minecraft:plains");
        let desert = p.intern("minecraft:desert");
        assert_ne!(plains, desert, "two names must not share an id");
        assert_eq!(plains, p.intern("minecraft:plains"), "ids must be stable");
        assert_eq!(p.len(), 2, "re-interning must not grow the palette");
    }

    #[test]
    fn ids_follow_the_build_order_and_round_trip() {
        let p = BiomePalette::from_names(["a", "b", "c"]);
        assert_eq!(p.id_of("a"), Some(0));
        assert_eq!(p.id_of("b"), Some(1));
        assert_eq!(p.id_of("c"), Some(2));
        for name in &["a", "b", "c"] {
            let id = p.id_of(name).expect("just built");
            assert_eq!(p.name(id), *name, "name(id_of(n)) must be n");
        }
    }

    /// A miss must be reportable, not guessable. `Rule::intern_biomes`
    /// collects these and the caller asserts the list is empty — a name that
    /// silently resolved to some default would leave a surface condition
    /// that never fires, with nothing red anywhere.
    #[test]
    fn an_unknown_name_has_no_id() {
        let p = BiomePalette::from_names(["minecraft:plains"]);
        assert_eq!(p.id_of("minecraft:nope"), None);
    }

    #[test]
    fn a_fresh_palette_is_empty() {
        let p = BiomePalette::new();
        assert!(p.is_empty());
        assert_eq!(p.id_of("anything"), None);
    }
}
