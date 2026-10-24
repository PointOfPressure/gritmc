//! One configured feature, whatever kind it is.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `ConfiguredFeature`, `RandomSelectorFeature`,
//! `SimpleRandomSelectorFeature`, `RandomBooleanSelectorFeature`.
//!
//! Until now each feature had its own `place` and its own world trait, and
//! the caller matched on the type. That stops working at the *selector*
//! features, which choose among other configured features and place one —
//! so a feature has to be able to place a feature without knowing what it
//! is.
//!
//! # A selector's draw is the whole of its behaviour
//!
//! All three selectors draw and then delegate, and the delegate's own draws
//! follow on the same stream. Which means the *order* is: pick, then place.
//! A port that resolved its children eagerly — say, to check they were all
//! supported before committing — would take their draws in the wrong place.
//!
//! `random_selector` is the subtle one. It walks its `features` list, draws
//! **once per entry** until one passes, and falls through to `default` if
//! none does:
//!
//! ```text
//! for (WeightedPlacedFeature entry : features)
//!     if (random.nextFloat() < entry.chance())
//!         return entry.feature().place(...)
//! return defaultFeature.place(...)
//! ```
//!
//! So a two-entry selector costs one draw when the first entry wins and two
//! when it does not — and the entry that wins still pays for the entries
//! before it. Collapsing that into one draw against a cumulative weight
//! gives the same distribution and a different world.

use crate::rng::RandomSource;
use crate::tags::BlockTags;

use super::aquatic::{self, AquaticWorld, ProbabilityConfig};
use super::block_column::{self, BlockColumnConfig, ColumnWorld};
use super::block_pile::{self, BlockPileConfig};
use super::blue_ice::{self, BlueIceWorld};
use super::disk::{self, DiskConfig, DiskWorld};
use super::fallen_tree::{self, FallenTreeConfig};
use super::freeze_top_layer::{self, FreezeWorld};
use super::geode::{self, GeodeConfig, GeodeWorld};
use super::huge_mushroom::{self, HugeMushroomConfig};
use super::iceberg::{self, IcebergWorld};
use super::lake::{self, LakeConfig, LakeWorld};
use super::large_dripstone::{self, LargeDripstoneConfig};
use super::monster_room::{self, MonsterRoomWorld};
use super::multiface_growth::{self, MultifaceGrowthConfig, MultifaceWorld};
use super::ore::{self, OreConfig, OreWorld};
use super::simple_block::{self, SimpleBlockConfig, SimpleBlockWorld};
use super::speleothem::{self, SpeleothemConfig, SpeleothemWorld};
use super::speleothem_cluster::{self, SpeleothemClusterConfig};
use super::spring::{self, SpringConfig, SpringWorld};
use super::tree::{self, TreeConfig, TreeWorld};
use super::underwater_magma::{self, MagmaWorld, UnderwaterMagmaConfig};
use super::vegetation_patch::{self, PatchWorld, VegetationPatchConfig};
use super::vines::{self, VineWorld};

/// Everything a feature might need of the world.
///
/// One supertrait rather than a bound per feature, because a selector places
/// a child whose kind it does not know. The cost is that a caller must be
/// able to answer everything; the benefit is that features compose, which is
/// what vanilla's `Feature<FC>` gets from its own level interface.
/// Counts `attemptSpreadVein` calls when the vein spread is enabled, so a
/// runaway announces itself instead of just hanging.
static SCULK_VEIN_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Patches that passed `canSpreadFrom`, and those that did not.
static SCULK_PATCH_STARTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static SCULK_PATCH_REFUSED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether to run the vein spread at all. **On**, and `RUSTMC_SCULK_VEIN=0`
/// turns it off.
///
/// # The reason it was off had already been fixed
///
/// It shipped off on 2026-08-09 with "does not terminate usefully": a single
/// chunk took over 120 s where a 25-chunk window had taken seconds. Two
/// commits later `2ac46e12` found the cause — `vein_config()` was being
/// called inside the per-cursor closure and re-parsing the whole block-state
/// extract every time — hoisted it into the `OnceLock` below, and measured
/// the chunk at 23.22 s. The default was left off anyway, for two reasons
/// both stated at the time: 23 s was still five times the baseline, and the
/// sculk volume was 4% of vanilla's.
///
/// **Both of those reasons then expired and the comment did not.** The four
/// commits after it — `c78a5624`, `a0f17d7c`, `6ab8e40a`, `858a920e` — took
/// the volume from 4% to 83% and the veins to essentially exact, and the cost
/// is not what it was either. Re-measured 2026-08-18 on window A of
/// `ci/ow-new-windows.sh`, seed 20260722 r=7 centre (-1056,-672), 225 chunks:
///
/// ```text
///   vein spread OFF   18 s      sculk     10        sculk_vein   412
///   vein spread ON    23 s      sculk 25,278        sculk_vein 8,705
///   vanilla                     sculk 30,184        sculk_vein 9,053
/// ```
///
/// 1.3x the dump cost, not 5x, and `sculk_catalyst` is **exact** at 47 of 47.
/// Off, `deep_dark` is not generated at all; the census called it the largest
/// unported family in the overworld by blocks on the strength of that.
///
/// **The switch stays, inverted**, because the thing it guards is still the
/// most expensive feature in the dimension and a future regression wants a
/// one-variable bisect. What does not stay is a default chosen by a
/// measurement that four later commits had already overturned.
fn sculk_vein_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("RUSTMC_SCULK_VEIN").as_deref() != Ok("0"))
}

/// Whether to NARRATE the sculk spread — `RUSTMC_SCULK_TRACE` set to anything.
///
/// # Why this is a separate switch and was not
///
/// Every counter and `eprintln` below used to be gated on
/// `sculk_vein_enabled()`, which is the switch that decides whether the thing
/// being counted runs at all. That is the project's own central defect class
/// in miniature — **an instrument wired to its own subject** — and it had two
/// concrete costs. Measuring how many patches start and how many refuse was
/// impossible with the spread off, which is exactly the configuration whose
/// 10 sculk blocks needed explaining; and turning the spread on for a real
/// dump meant a `SCULK_BRANCH` line per patch, thousands per window, into the
/// same stderr the refusal tally is read from.
///
/// So the two questions are now asked separately: `RUSTMC_SCULK_VEIN`
/// decides what the world contains, `RUSTMC_SCULK_TRACE` decides what the log
/// contains, and neither answer is conditional on the other.
fn sculk_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUSTMC_SCULK_TRACE").is_some())
}

/// `Blocks.SCULK_VEIN`'s own spreader config, as `sculk_vein.json` declares it.
///
/// Built here rather than read from the datapack because `attemptSpreadVein`
/// reaches for the **block's** spreader, not for a configured feature a biome
/// selected — the vein spreads during a sculk patch whether or not
/// `sculk_vein` is in that biome's feature list.
fn vein_config() -> Option<&'static MultifaceGrowthConfig> {
    // Built ONCE. Building it per call was a 38x slowdown: `extract_or_skip`
    // loads and parses the whole block-state extract, and the closure that
    // calls this runs hundreds of times per patch and thousands of times per
    // chunk. One chunk went from 4.29 s to 162.95 s, which read as an
    // unbounded algorithm and was in fact a constant hoisted into a loop.
    static CONFIG: std::sync::OnceLock<Option<MultifaceGrowthConfig>> = std::sync::OnceLock::new();
    CONFIG.get_or_init(build_vein_config).as_ref()
}

fn build_vein_config() -> Option<MultifaceGrowthConfig> {
    let states = crate::feature::state_spec::extract_or_skip()?;
    let json: serde_json::Value = serde_json::from_str(
        r#"{"block":"minecraft:sculk_vein",
            "can_be_placed_on":["minecraft:stone","minecraft:andesite",
                "minecraft:diorite","minecraft:granite",
                "minecraft:dripstone_block","minecraft:calcite",
                "minecraft:tuff","minecraft:deepslate"],
            "can_place_on_ceiling":true,"can_place_on_floor":true,
            "can_place_on_wall":true,"chance_of_spreading":1.0,
            "search_range":20}"#,
    )
    .ok()?;
    MultifaceGrowthConfig::parse(&json, &states).ok()
}

/// Bridges a `FeatureWorld` to the narrow traits the sculk spreader wants.
///
/// The spreader's traits are deliberately smaller than `FeatureWorld` because
/// the same code runs for a sculk catalyst at runtime, where no feature world
/// exists. Adapting here keeps that true.
struct SculkAdapter<'a, W: ?Sized> {
    world: &'a mut W,
    tags: &'a BlockTags,
}

impl<W: FeatureWorld + ?Sized> super::sculk_spreader::SpreadWorld for SculkAdapter<'_, W> {
    fn is_face_sturdy(&mut self, pos: (i32, i32, i32), face: super::sculk_spreader::Face) -> bool {
        // `Face` is declared in `Direction.values()` order, which is also the
        // index order `PredicateWorld` uses.
        let index = match face {
            super::sculk_spreader::Face::Down => 0,
            super::sculk_spreader::Face::Up => 1,
            super::sculk_spreader::Face::North => 2,
            super::sculk_spreader::Face::South => 3,
            super::sculk_spreader::Face::West => 4,
            super::sculk_spreader::Face::East => 5,
        };
        // Fully qualified: several supertraits of `FeatureWorld` declare a
        // method of this name.
        <W as crate::feature::block_predicate::PredicateWorld>::face_is_sturdy(
            self.world, pos.0, pos.1, pos.2, index,
        )
        .unwrap_or(false)
    }
}

impl<W: FeatureWorld + ?Sized> super::sculk_worldgen::SculkLevel for SculkAdapter<'_, W> {
    fn block_at(&mut self, pos: (i32, i32, i32)) -> String {
        <W as crate::feature::block_predicate::PredicateWorld>::block_at(
            self.world, pos.0, pos.1, pos.2,
        )
    }
    fn set_block(&mut self, pos: (i32, i32, i32), state: &str) {
        <W as MultifaceWorld>::set_block(self.world, pos.0, pos.1, pos.2, state);
    }
    fn fluid_at(&mut self, pos: (i32, i32, i32)) -> String {
        <W as crate::feature::block_predicate::PredicateWorld>::fluid_at(
            self.world, pos.0, pos.1, pos.2,
        )
    }
    fn is_air(&mut self, pos: (i32, i32, i32)) -> bool {
        <W as crate::feature::block_predicate::PredicateWorld>::air_at(
            self.world, pos.0, pos.1, pos.2,
        )
        .unwrap_or(false)
    }
    /// `isCollisionShapeFullBlock`, as a full collision face on all six
    /// sides — the table has no whole-shape column, and for every block a
    /// deep dark is built of the two agree.
    fn is_collision_full_block(&mut self, pos: (i32, i32, i32)) -> bool {
        (0..6).all(|d| {
            <W as crate::feature::block_predicate::PredicateWorld>::face_is_collision_full(
                self.world, pos.0, pos.1, pos.2, d,
            )
            .unwrap_or(false)
        })
    }
    fn in_tag(&mut self, tag: &str, pos: (i32, i32, i32)) -> bool {
        let state = <W as crate::feature::block_predicate::PredicateWorld>::block_at(
            self.world, pos.0, pos.1, pos.2,
        );
        self.tags
            .contains(tag, state.split('[').next().unwrap_or(&state))
    }
    fn can_attach_to(&mut self, pos: (i32, i32, i32), face: super::sculk_spreader::Face) -> bool {
        let index = face_index(face);
        multiface_growth::can_attach_to(self.world, multiface_growth::offset_of(pos, index), index)
            .unwrap_or(false)
    }
    fn vein_spread_all(&mut self, state: &str, pos: (i32, i32, i32), same_space: bool) -> u64 {
        if !sculk_vein_enabled() {
            return 0;
        }
        let Some(vein) = vein_config() else {
            return 0;
        };
        let calls = SCULK_VEIN_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if sculk_trace() && calls.is_multiple_of(100_000) {
            eprintln!("SCULK_VEIN {calls} spread calls so far");
        }
        let types: &[multiface_growth::SpreadType] = if same_space {
            &[multiface_growth::SpreadType::SamePosition]
        } else {
            &[
                multiface_growth::SpreadType::SamePosition,
                multiface_growth::SpreadType::SamePlane,
                multiface_growth::SpreadType::WrapAround,
            ]
        };
        multiface_growth::spread_all_with(self.world, self.tags, vein, state, pos, true, types)
            .unwrap_or(0)
    }
}

/// `Direction.values()` index of a sculk [`super::sculk_spreader::Face`].
const fn face_index(face: super::sculk_spreader::Face) -> usize {
    match face {
        super::sculk_spreader::Face::Down => 0,
        super::sculk_spreader::Face::Up => 1,
        super::sculk_spreader::Face::North => 2,
        super::sculk_spreader::Face::South => 3,
        super::sculk_spreader::Face::West => 4,
        super::sculk_spreader::Face::East => 5,
    }
}

pub trait FeatureWorld:
    OreWorld
    + DiskWorld
    + MagmaWorld
    + SimpleBlockWorld
    + SpringWorld
    + FreezeWorld
    + ColumnWorld
    + MultifaceWorld
    + AquaticWorld
    + MonsterRoomWorld
    + BlueIceWorld
    + IcebergWorld
    + VineWorld
    + TreeWorld
    + LakeWorld
    + GeodeWorld
    + SpeleothemWorld
    + super::large_dripstone::LargeDripstoneWorld
    + super::block_pile::BlockPileWorld
    + super::huge_mushroom::HugeMushroomWorld
    + PatchWorld
    + super::nether::NetherWorld
    + super::end::EndWorld
    + super::placement::PlacementWorld
{
}

impl<
        T: OreWorld
            + DiskWorld
            + MagmaWorld
            + SimpleBlockWorld
            + SpringWorld
            + FreezeWorld
            + ColumnWorld
            + MultifaceWorld
            + AquaticWorld
            + MonsterRoomWorld
            + BlueIceWorld
            + IcebergWorld
            + VineWorld
            + TreeWorld
            + LakeWorld
            + GeodeWorld
            + SpeleothemWorld
            + super::large_dripstone::LargeDripstoneWorld
            + super::block_pile::BlockPileWorld
            + super::huge_mushroom::HugeMushroomWorld
            + PatchWorld
            + super::nether::NetherWorld
            + super::end::EndWorld
            + super::placement::PlacementWorld
            + ?Sized,
    > FeatureWorld for T
{
}

/// A feature this port cannot place, named.
#[derive(Debug, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unimplemented feature `{}`", self.0)
    }
}

impl std::error::Error for Unsupported {}

/// A feature nested inside a selector.
///
/// **A placed feature, not a configured one.** `WeightedPlacedFeature.place`
/// takes a `Holder<PlacedFeature>` and `RandomFeatureConfiguration.default`
/// is one too, so a selector's child carries its own placement chain and runs
/// it — the datapack writes them inline as
/// `{"feature": "minecraft:oak_bees_005", "placement": []}`.
///
/// Reading them as configured features would drop the chain. Most are empty
/// in practice, which is exactly what would make the mistake survive testing.
#[derive(Clone, Debug, PartialEq)]
pub struct NestedPlaced {
    pub placement: Vec<super::placement::PlacementModifier>,
    pub feature: Box<ConfiguredFeature>,
}

/// One entry of a `random_selector`.
#[derive(Clone, Debug, PartialEq)]
pub struct WeightedFeature {
    pub feature: NestedPlaced,
    pub chance: f32,
}

/// `ConfiguredFeature` — a feature plus its configuration.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfiguredFeature {
    Ore(OreConfig),
    Disk(DiskConfig),
    SimpleBlock(SimpleBlockConfig),
    UnderwaterMagma(UnderwaterMagmaConfig),
    Spring(SpringConfig),
    /// `freeze_top_layer` — no config at all.
    FreezeTopLayer,
    BlockColumn(BlockColumnConfig),
    MultifaceGrowth(MultifaceGrowthConfig),
    SculkPatch(super::sculk_patch::SculkPatchConfig),
    Bamboo(super::bamboo::BambooConfig),
    /// The config, plus the inline placed feature it grows on top of.
    VegetationPatch(Box<VegetationPatchConfig>, Box<NestedPlaced>),
    /// `sequence` — run each child in order and **stop at the first one that
    /// returns false**.
    ///
    /// Not "run them all": a sulfur pool is a lake followed by the things
    /// that decorate it, and if the lake did not fit, the decoration must not
    /// happen either. Each child is a *placed* feature, so it runs its own
    /// chain from the same origin on the same stream — a child that declines
    /// still spends its chain's draws.
    Sequence(Vec<NestedPlaced>),
    Lake(Box<LakeConfig>),
    Geode(Box<GeodeConfig>),
    Speleothem(Box<SpeleothemConfig>),
    SpeleothemCluster(Box<SpeleothemClusterConfig>),
    /// `large_dripstone` — the two big cones. Not a speleothem: it shares
    /// `SpeleothemUtils` and nothing else, and its config is its own record.
    LargeDripstone(Box<LargeDripstoneConfig>),
    /// `block_pile` — the savanna village's hay and melons. Reached only
    /// through a `feature_pool_element`, never through biome decoration.
    BlockPile(Box<BlockPileConfig>),
    /// The two huge mushrooms. One configuration and one algorithm apart
    /// from the cap, so the variant rides alongside rather than duplicating
    /// the record.
    HugeMushroom(Box<HugeMushroomConfig>, huge_mushroom::Variant),
    Seagrass(ProbabilityConfig),
    Kelp,
    /// `scattered_ore` — ancient debris. Shares `OreConfiguration` with
    /// [`Self::Ore`] and none of its geometry.
    ScatteredOre(OreConfig),
    /// The five nether-only features. Their configurations are unrelated to
    /// each other; only the world they read is shared.
    GlowstoneBlob,
    BasaltPillar,
    /// The two End-only features that write blocks on the central island.
    /// Neither has a configuration: `end_spike`'s is an empty `spikes` list
    /// (the layout comes from the world seed, not the datapack) and
    /// `end_platform` is `NoneFeatureConfiguration`.
    EndSpike,
    EndPlatform,
    /// `end_island` — the outer End's floating end-stone rubble, and
    /// `NoneFeatureConfiguration` as well.
    EndIsland,
    /// `end_gateway` — the return gateways of `end_highlands`.
    EndGateway(super::end::GatewayConfig),
    /// `chorus_plant` — `NoneFeatureConfiguration` too, and the only End
    /// feature whose blocks are stateful: six connection booleans per block,
    /// computed by reading the world back as it grows.
    ChorusPlant,
    ReplaceBlobs(super::nether::ReplaceSphereConfig),
    Delta(Box<super::nether::DeltaConfig>),
    BasaltColumns(super::nether::ColumnConfig),
    /// The forest biomes' four: `nether_forest_vegetation` (roots, fungi and
    /// sprouts), `huge_fungus`, and the two vine features.
    NetherForestVegetation(Box<super::nether_forest::VegetationConfig>),
    HugeFungus(Box<super::nether_forest::HugeFungusConfig>),
    WeepingVines,
    TwistingVines(super::nether_forest::TwistingVinesConfig),
    MonsterRoom,
    BlueIce,
    /// `minecraft:iceberg`, carrying its `BlockStateConfiguration` — the
    /// one block state the berg is built from. The extract wires this same
    /// feature up twice, as `iceberg_packed` and `iceberg_blue`.
    Iceberg(String),
    /// The vine feature, carrying the default state it writes.
    Vines(String),
    Tree(Box<TreeConfig>),
    /// `fallen_tree` — a stump and a log lying beside it. Shares the
    /// decorators and the trunk provider with [`Self::Tree`] and nothing
    /// else.
    FallenTree(Box<FallenTreeConfig>),
    /// `random_selector` — one draw per entry until one passes, then the
    /// default. See the module docs for why that is not one draw.
    RandomSelector {
        features: Vec<WeightedFeature>,
        default: NestedPlaced,
    },
    /// `simple_random_selector` — one `nextInt(size)` and place that one.
    SimpleRandomSelector(Vec<NestedPlaced>),
    /// `random_boolean_selector` — one `nextBoolean()`, two outcomes.
    RandomBooleanSelector {
        on_true: NestedPlaced,
        on_false: NestedPlaced,
    },
    /// Parsed but not placeable. Carries its type so a refusal names it.
    Unimplemented(String),
}

impl ConfiguredFeature {
    /// Parse one `worldgen/configured_feature/*.json`.
    ///
    /// `resolve` turns a referenced feature name into its document, because
    /// the selectors name their children by id rather than inlining them.
    ///
    /// # Errors
    /// If a supported type is malformed, or a referenced child is missing. An
    /// *unsupported* type is not an error — it becomes
    /// [`Self::Unimplemented`], so a selector wrapping one still parses and
    /// refuses only when it actually picks it.
    pub fn parse(
        value: &serde_json::Value,
        resolve: &dyn Fn(&str) -> Option<serde_json::Value>,
        resolve_configured: &dyn Fn(&str) -> Option<serde_json::Value>,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "configured feature has no `type`".to_string())?;
        // `freeze_top_layer` has no configuration at all, so `config` is
        // absent rather than empty — hence the default rather than an error.
        let empty = serde_json::Value::Object(serde_json::Map::new());
        let config = value.get("config").unwrap_or(&empty);

        // A child is a placed feature, written either as a name or inline.
        // A configured feature named from somewhere that is not a placed
        // feature's `feature` field. A tree decorator is the only such place:
        // `PaleMossDecorator` hardcodes `VegetationFeatures.PALE_MOSS_PATCH`
        // and reads it out of `registryAccess()` at place time rather than
        // taking it from its own config.
        //
        // Resolving it HERE, at parse time, is what keeps the registry out of
        // the place path entirely. Vanilla does the lookup inside `place`
        // because it has a live `RegistryAccess` on the level; this port does
        // not, and threading one down to a decorator would mean a registry
        // parameter on every world trait. The name is fixed, so the document
        // can be fetched once while the tree config is being read and stored
        // as a parsed `ConfiguredFeature` in the decorator.
        //
        // Recursion terminates because nothing reachable this way contains a
        // tree: `pale_moss_patch` is a `vegetation_patch` over a
        // `simple_block`. A datapack that made one cycle would recurse until
        // the stack ran out, which is the same thing `child` above already
        // does for a placed-feature cycle.
        let resolve_feature = |name: &str| -> Result<Self, String> {
            let json =
                resolve_configured(name).ok_or_else(|| format!("{name} is not in the extract"))?;
            Self::parse(&json, resolve, resolve_configured, states)
        };

        let child = |value: &serde_json::Value| -> Result<NestedPlaced, String> {
            let inline = match value.as_str() {
                Some(name) => {
                    resolve(name).ok_or_else(|| format!("{name} is not in the extract"))?
                }
                None => value.clone(),
            };
            // `feature` is either a registry name or a whole configured
            // feature written inline. Both spellings are legal in the same
            // file — `sulfur_pool` inlines a lake, `trees_plains` names one —
            // and a parser that only took the first refuses every sequence in
            // the datapack.
            let feature_value = inline
                .get("feature")
                .ok_or_else(|| "nested placed feature has no `feature`".to_string())?;
            let configured = match feature_value.as_str() {
                Some(name) => resolve_configured(name)
                    .ok_or_else(|| format!("{name} is not in the extract"))?,
                None => feature_value.clone(),
            };
            Ok(NestedPlaced {
                placement: inline
                    .get("placement")
                    .and_then(serde_json::Value::as_array)
                    .map(|list| {
                        list.iter()
                            .map(super::placement::PlacementModifier::parse)
                            .collect::<Result<Vec<_>, String>>()
                    })
                    .transpose()?
                    .unwrap_or_default(),
                feature: Box::new(Self::parse(
                    &configured,
                    resolve,
                    resolve_configured,
                    states,
                )?),
            })
        };

        match kind {
            "minecraft:ore" => Ok(Self::Ore(OreConfig::parse(config, states)?)),
            "minecraft:disk" => Ok(Self::Disk(DiskConfig::parse(config, states)?)),
            "minecraft:simple_block" => {
                Ok(Self::SimpleBlock(SimpleBlockConfig::parse(config, states)?))
            }
            "minecraft:underwater_magma" => {
                Ok(Self::UnderwaterMagma(UnderwaterMagmaConfig::parse(config)?))
            }
            "minecraft:spring_feature" => Ok(Self::Spring(SpringConfig::parse(config, states)?)),
            "minecraft:freeze_top_layer" => Ok(Self::FreezeTopLayer),
            "minecraft:block_column" => {
                Ok(Self::BlockColumn(BlockColumnConfig::parse(config, states)?))
            }
            "minecraft:vegetation_patch" | "minecraft:waterlogged_vegetation_patch" => {
                // The waterlogged variant differs only in the ground it
                // lays; parsed here so the refusal names the right type, and
                // refused below until that difference is ported.
                let inner =
                    child(config.get("vegetation_feature").ok_or_else(|| {
                        "vegetation_patch has no `vegetation_feature`".to_string()
                    })?)?;
                Ok(Self::VegetationPatch(
                    Box::new(VegetationPatchConfig::parse(config, states)?),
                    Box::new(inner),
                ))
            }
            "minecraft:multiface_growth" => Ok(Self::MultifaceGrowth(
                MultifaceGrowthConfig::parse(config, states)?,
            )),

            "minecraft:sculk_patch" => Ok(Self::SculkPatch(
                super::sculk_patch::SculkPatchConfig::parse(config)?,
            )),
            "minecraft:bamboo" => Ok(Self::Bamboo(super::bamboo::BambooConfig::parse(config)?)),
            "minecraft:sequence" => {
                let features = config
                    .get("features")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "sequence has no `features`".to_string())?
                    .iter()
                    .map(child)
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::Sequence(features))
            }
            "minecraft:seagrass" => Ok(Self::Seagrass(ProbabilityConfig::parse(config)?)),
            "minecraft:kelp" => Ok(Self::Kelp),
            "minecraft:scattered_ore" => Ok(Self::ScatteredOre(OreConfig::parse(config, states)?)),
            // `glowstone_blob` and `basalt_pillar` take `NoneFeatureConfiguration`,
            // so their `config` is `{}` and there is nothing to read.
            "minecraft:glowstone_blob" => Ok(Self::GlowstoneBlob),
            // `end_spike`'s only config field is `spikes`, and vanilla's own
            // `end_spike.json` leaves it empty — which is not a default, it is
            // the signal that `getSpikesForLevel` should derive the layout
            // from the world seed. A non-empty list only ever comes from the
            // dragon fight re-placing a spike at runtime, never from worldgen,
            // so refusing to parse one here would refuse nothing real.
            "minecraft:end_spike" => Ok(Self::EndSpike),
            "minecraft:end_platform" => Ok(Self::EndPlatform),
            "minecraft:end_island" => Ok(Self::EndIsland),
            "minecraft:end_gateway" => {
                Ok(Self::EndGateway(super::end::GatewayConfig::parse(config)?))
            }
            "minecraft:chorus_plant" => Ok(Self::ChorusPlant),
            // `minecraft:basalt_pillar` was unreachable from here on purpose
            // for two turns, because wiring it up moved the r=7 nether window
            // (seed 20260722, centre (64,64), 225 chunks, y 0..127) from
            // 24,245 mismatches to 72,684 — a regression of 48,439 blocks.
            //
            // That was `docs/ORACLE.md` §22 and nobody read it as such: **the
            // regression was a second defect, not an argument against the
            // change.** `place_basalt_pillar` had
            // `BasaltPillarFeature.place`'s entry test inverted, so a pillar
            // could start anywhere with two blocks of headroom instead of only
            // under a ceiling. With that fixed the same wiring moves the window
            // to **22,014**, and our basalt count lands on 24,433 against
            // vanilla's 24,333 — where the bug produced 70,057.
            // The registry name really is this long — the feature class is
            // `ReplaceBlobsFeature`, but it is registered as
            // `netherrack_replace_blobs`, and the datapack names the
            // registration.
            "minecraft:netherrack_replace_blobs" => Ok(Self::ReplaceBlobs(
                super::nether::ReplaceSphereConfig::parse(config, states)?,
            )),
            "minecraft:delta_feature" => Ok(Self::Delta(Box::new(
                super::nether::DeltaConfig::parse(config, states)?,
            ))),
            "minecraft:basalt_columns" => Ok(Self::BasaltColumns(
                super::nether::ColumnConfig::parse(config)?,
            )),
            "minecraft:basalt_pillar" => Ok(Self::BasaltPillar),
            "minecraft:nether_forest_vegetation" => Ok(Self::NetherForestVegetation(Box::new(
                super::nether_forest::VegetationConfig::parse(config, states)?,
            ))),
            "minecraft:huge_fungus" => Ok(Self::HugeFungus(Box::new(
                super::nether_forest::HugeFungusConfig::parse(config, states)?,
            ))),
            "minecraft:weeping_vines" => Ok(Self::WeepingVines),
            "minecraft:twisting_vines" => Ok(Self::TwistingVines(
                super::nether_forest::TwistingVinesConfig::parse(config)?,
            )),
            "minecraft:monster_room" => Ok(Self::MonsterRoom),
            "minecraft:blue_ice" => Ok(Self::BlueIce),
            "minecraft:iceberg" => Ok(Self::Iceberg(super::state_spec::canonical(
                config
                    .get("state")
                    .ok_or_else(|| "iceberg config has no `state`".to_string())?,
                states,
            )?)),
            "minecraft:lake" => Ok(Self::Lake(Box::new(LakeConfig::parse(config, states)?))),
            "minecraft:geode" => Ok(Self::Geode(Box::new(GeodeConfig::parse(config, states)?))),
            "minecraft:speleothem" => Ok(Self::Speleothem(Box::new(SpeleothemConfig::parse(
                config, states,
            )?))),
            "minecraft:block_pile" => Ok(Self::BlockPile(Box::new(BlockPileConfig::parse(
                config, states,
            )?))),
            "minecraft:speleothem_cluster" => Ok(Self::SpeleothemCluster(Box::new(
                SpeleothemClusterConfig::parse(config, states)?,
            ))),
            "minecraft:large_dripstone" => Ok(Self::LargeDripstone(Box::new(
                LargeDripstoneConfig::parse(config)?,
            ))),
            "minecraft:huge_brown_mushroom" => Ok(Self::HugeMushroom(
                Box::new(HugeMushroomConfig::parse(config, states)?),
                huge_mushroom::Variant::Brown,
            )),
            "minecraft:huge_red_mushroom" => Ok(Self::HugeMushroom(
                Box::new(HugeMushroomConfig::parse(config, states)?),
                huge_mushroom::Variant::Red,
            )),
            "minecraft:tree" => Ok(Self::Tree(Box::new(TreeConfig::parse(
                config,
                states,
                &resolve_feature,
            )?))),
            "minecraft:fallen_tree" => Ok(Self::FallenTree(Box::new(FallenTreeConfig::parse(
                config, states,
            )?))),
            "minecraft:vines" => Ok(Self::Vines(super::state_spec::canonical(
                &serde_json::json!({"Name": "minecraft:vine"}),
                states,
            )?)),
            "minecraft:random_selector" => {
                let features =
                    config
                        .get("features")
                        .and_then(serde_json::Value::as_array)
                        .ok_or_else(|| "random_selector has no `features`".to_string())?
                        .iter()
                        .map(|entry| {
                            #[allow(clippy::cast_possible_truncation)]
                            let chance = entry
                                .get("chance")
                                .and_then(serde_json::Value::as_f64)
                                .ok_or_else(|| "selector entry has no `chance`".to_string())?
                                as f32;
                            Ok(WeightedFeature {
                                feature: child(entry.get("feature").ok_or_else(|| {
                                    "selector entry has no `feature`".to_string()
                                })?)?,
                                chance,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::RandomSelector {
                    features,
                    default: child(
                        config
                            .get("default")
                            .ok_or_else(|| "random_selector has no `default`".to_string())?,
                    )?,
                })
            }
            "minecraft:simple_random_selector" => {
                let features = config
                    .get("features")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "simple_random_selector has no `features`".to_string())?
                    .iter()
                    .map(child)
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::SimpleRandomSelector(features))
            }
            "minecraft:random_boolean_selector" => {
                Ok(Self::RandomBooleanSelector {
                    on_true: child(config.get("feature_true").ok_or_else(|| {
                        "random_boolean_selector has no `feature_true`".to_string()
                    })?)?,
                    on_false: child(config.get("feature_false").ok_or_else(|| {
                        "random_boolean_selector has no `feature_false`".to_string()
                    })?)?,
                })
            }
            other => Ok(Self::Unimplemented(other.to_string())),
        }
    }

    /// Whether this feature — and every child it could reach — can be
    /// placed.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Ore(_) | Self::UnderwaterMagma(_) | Self::Spring(_) | Self::FreezeTopLayer => {
                true
            }
            Self::Disk(config) => config.is_supported(),
            Self::BlockColumn(config) => config.is_supported(),
            Self::MultifaceGrowth(config) => config.is_supported(),
            Self::SculkPatch(_) => true,
            Self::Bamboo(_) => true,
            Self::VegetationPatch(_, inner) => inner.feature.is_supported(),
            Self::Sequence(features) => features.iter().all(|f| f.feature.is_supported()),
            Self::Lake(config) => config.is_supported(),
            Self::Geode(config) => config.is_supported(),
            Self::Speleothem(config) => config.is_supported(),
            Self::SpeleothemCluster(config) => config.is_supported(),
            Self::LargeDripstone(config) => config.is_supported(),
            Self::BlockPile(config) => config.is_supported(),
            // Both providers and the placement predicate are the only things
            // that could be unimplemented, and both report for themselves.
            Self::HugeMushroom(config, _) => {
                config.cap_provider.is_supported() && config.stem_provider.is_supported()
            }
            Self::Seagrass(_)
            | Self::Kelp
            | Self::MonsterRoom
            | Self::BlueIce
            | Self::Iceberg(_) => true,
            Self::EndSpike
            | Self::EndPlatform
            | Self::EndIsland
            | Self::EndGateway(_)
            | Self::ChorusPlant => true,
            Self::ScatteredOre(_)
            | Self::GlowstoneBlob
            | Self::BasaltPillar
            | Self::ReplaceBlobs(_)
            | Self::Delta(_)
            | Self::BasaltColumns(_) => true,
            Self::NetherForestVegetation(config) => config.is_supported(),
            Self::HugeFungus(config) => config.is_supported(),
            Self::WeepingVines | Self::TwistingVines(_) => true,
            Self::Vines(_) => true,
            Self::Tree(config) => config.is_supported(),
            Self::FallenTree(config) => config.is_supported(),
            Self::SimpleBlock(config) => config.is_supported(),
            Self::RandomSelector { features, default } => {
                default.is_supported() && features.iter().all(|f| f.feature.is_supported())
            }
            Self::SimpleRandomSelector(features) => features.iter().all(NestedPlaced::is_supported),
            Self::RandomBooleanSelector { on_true, on_false } => {
                on_true.is_supported() && on_false.is_supported()
            }
            Self::Unimplemented(_) => false,
        }
    }

    /// `ConfiguredFeature#place`. Returns whether anything was placed.
    ///
    /// # Errors
    /// If this feature, or the child a selector picked, is not implemented.
    pub fn place<R: RandomSource>(
        &self,
        world: &mut (dyn FeatureWorld + '_),
        tags: &BlockTags,
        random: &mut R,
        origin: (i32, i32, i32),
    ) -> Result<bool, Unsupported> {
        match self {
            Self::Ore(config) => Ok(ore::place(world, tags, config, random, origin)),
            Self::ScatteredOre(config) => {
                Ok(ore::place_scattered(world, tags, config, random, origin))
            }
            Self::GlowstoneBlob => Ok(super::nether::place_glowstone(world, random, origin)),
            Self::EndSpike => Ok(super::end::place_spike(world, random, origin)),
            // No random: `EndPlatformFeature` does not draw.
            Self::EndPlatform => Ok(super::end::place_platform(world, origin)),
            Self::EndIsland => Ok(super::end::place_island(world, random, origin)),
            Self::EndGateway(config) => Ok(super::end::place_gateway(world, *config, origin)),
            Self::ChorusPlant => Ok(super::end::place_chorus_plant(world, tags, random, origin)),
            Self::BasaltPillar => Ok(super::nether::place_basalt_pillar(world, random, origin)),
            Self::ReplaceBlobs(config) => Ok(super::nether::place_replace_blobs(
                world, config, random, origin,
            )),
            Self::Delta(config) => Ok(super::nether::place_delta(world, config, random, origin)),
            Self::BasaltColumns(config) => Ok(super::nether::place_basalt_columns(
                world, config, random, origin,
            )),
            Self::NetherForestVegetation(config) => {
                super::nether_forest::place_vegetation(world, tags, config, random, origin)
            }
            Self::HugeFungus(config) => {
                super::nether_forest::place_huge_fungus(world, tags, config, random, origin)
            }
            Self::WeepingVines => Ok(super::nether_forest::place_weeping_vines(
                world, random, origin,
            )),
            Self::TwistingVines(config) => Ok(super::nether_forest::place_twisting_vines(
                world, *config, random, origin,
            )),
            Self::Disk(config) => {
                disk::place(world, tags, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::SimpleBlock(config) => simple_block::place(world, tags, config, random, origin)
                .map_err(|e| Unsupported(e.0)),
            Self::UnderwaterMagma(config) => underwater_magma::place(world, config, random, origin)
                .map_err(|_| Unsupported("underwater_magma: no face shapes".to_string())),

            // No random: `SpringFeature` does not draw. See its module docs.
            Self::Spring(config) => {
                spring::place(world, tags, config, origin).map_err(|e| Unsupported(e.0))
            }

            Self::BlockColumn(config) => block_column::place(world, tags, config, random, origin)
                .map_err(|e| Unsupported(e.0)),

            Self::MultifaceGrowth(config) => {
                multiface_growth::place(world, tags, config, random, origin)
                    .map_err(|e| Unsupported(e.0))
            }

            Self::Bamboo(config) => {
                super::bamboo::place(world, config, random, origin).map_err(|e| Unsupported(e.0))
            }

            Self::SculkPatch(config) => {
                // `SculkPatchFeature.place`, every write immediate: see
                // `feature::sculk_worldgen` for the port and for the eleven
                // places the previous one (which deferred its writes) left
                // the bytecode.
                let mut level = SculkAdapter { world, tags };
                let placed = super::sculk_worldgen::place(&mut level, config, random, origin);
                if placed.is_some() {
                    SCULK_PATCH_STARTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    SCULK_PATCH_REFUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if sculk_trace() {
                    let started = SCULK_PATCH_STARTED.load(std::sync::atomic::Ordering::Relaxed);
                    let refused = SCULK_PATCH_REFUSED.load(std::sync::atomic::Ordering::Relaxed);
                    if (started + refused).is_multiple_of(500) {
                        eprintln!("SCULK_PATCH started={started} refused={refused}");
                    }
                }
                Ok(placed.is_some())
            }

            Self::VegetationPatch(config, inner) => {
                // The inner feature runs through the ordinary nested-placed
                // path, so its own placement chain and draws are the same
                // ones any other child would take.
                // The rng comes in as a parameter rather than being captured:
                // `vegetation_patch::place` holds it for its own draws, and
                // the inner feature must draw from the *same* stream.
                let mut vegetation =
                    |world: &mut _, mut rng: &mut dyn RandomSource, pos: (i32, i32, i32)| {
                        // `&mut rng`, not `rng`: the generic parameter becomes
                        // `&mut dyn RandomSource`, which is sized and carries the
                        // delegating impl, so every other feature keeps its
                        // concrete rng type.
                        inner
                            .place(world, tags, &mut rng, pos)
                            .map(|_| true)
                            .map_err(|e| vegetation_patch::NeedsBehaviour(e.0))
                    };
                vegetation_patch::place(world, tags, config, random, origin, &mut vegetation)
                    .map_err(|e| Unsupported(e.0))
            }

            Self::Seagrass(config) => {
                aquatic::place_seagrass(world, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::Kelp => aquatic::place_kelp(world, random, origin).map_err(|e| Unsupported(e.0)),
            Self::MonsterRoom => {
                monster_room::place(world, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::BlueIce => blue_ice::place(world, random, origin).map_err(|e| Unsupported(e.0)),
            Self::Iceberg(state) => {
                iceberg::place(world, random, origin, state).map_err(|e| Unsupported(e.0))
            }
            Self::Lake(config) => {
                lake::place(world, tags, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::Geode(config) => {
                geode::place(world, tags, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::Speleothem(config) => {
                speleothem::place(world, tags, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::SpeleothemCluster(config) => {
                speleothem_cluster::place(world, tags, config, random, origin)
                    .map_err(|e| Unsupported(e.0))
            }
            Self::LargeDripstone(config) => {
                large_dripstone::place(world, tags, config, random, origin)
                    .map_err(|e| Unsupported(e.0))
            }
            Self::BlockPile(config) => {
                block_pile::place(world, tags, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::HugeMushroom(config, variant) => {
                huge_mushroom::place(world, tags, config, *variant, random, origin)
                    .map_err(|e| Unsupported(e.0))
            }
            Self::Vines(state) => vines::place(world, state, origin).map_err(|e| Unsupported(e.0)),
            Self::Tree(config) => {
                tree::place(world, tags, config, random, origin).map_err(|e| Unsupported(e.0))
            }
            Self::FallenTree(config) => fallen_tree::place(world, tags, config, random, origin)
                .map_err(|e| Unsupported(e.0)),

            // Also draws nothing, and runs over the whole chunk rather than
            // the position it was handed.
            Self::FreezeTopLayer => {
                freeze_top_layer::place(world, origin).map_err(|e| Unsupported(e.0))
            }

            // `RandomSelectorFeature#place`: a draw per entry until one
            // passes. The winner still pays for the entries before it.
            Self::RandomSelector { features, default } => {
                for entry in features {
                    if random.next_f32() < entry.chance {
                        return entry.feature.place(world, tags, random, origin);
                    }
                }
                default.place(world, tags, random, origin)
            }

            // `SequenceFeature#place`: each child in order, short-circuiting.
            Self::Sequence(features) => {
                for feature in features {
                    if !feature.place(world, tags, random, origin)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }

            // `SimpleRandomSelectorFeature#place`: one `nextInt(size)`.
            Self::SimpleRandomSelector(features) => {
                if features.is_empty() {
                    return Ok(false);
                }
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let index = random.next_i32_bound(features.len() as i32);
                features[usize::try_from(index).unwrap_or(0)].place(world, tags, random, origin)
            }

            // `RandomBooleanSelectorFeature#place`: one `nextBoolean()`.
            Self::RandomBooleanSelector { on_true, on_false } => {
                if random.next_bool() {
                    on_true.place(world, tags, random, origin)
                } else {
                    on_false.place(world, tags, random, origin)
                }
            }

            Self::Unimplemented(name) => Err(Unsupported(name.clone())),
        }
    }
}

impl NestedPlaced {
    /// Whether this child and everything under it can be placed.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.feature.is_supported()
            && self
                .placement
                .iter()
                .all(super::placement::PlacementModifier::is_supported)
    }

    /// `PlacedFeature#place` — the chain, then the feature at each position
    /// it produces, interleaved on one stream.
    ///
    /// Note this is `place`, not `placeWithBiomeCheck`: a selector's child is
    /// placed with `topFeature = Optional.empty()`, so a `biome` modifier in
    /// a nested chain has nothing to ask about. None of them have one.
    ///
    /// # Errors
    /// If the chain or the feature is not implemented.
    pub fn place<R: RandomSource>(
        &self,
        world: &mut (dyn FeatureWorld + '_),
        tags: &BlockTags,
        random: &mut R,
        origin: (i32, i32, i32),
    ) -> Result<bool, Unsupported> {
        let start = super::placement::Pos {
            x: origin.0,
            y: origin.1,
            z: origin.2,
        };
        let mut placed = false;
        let mut failure = None;
        super::placement::for_each_position(
            &self.placement,
            world,
            random,
            start,
            |world, random, pos| {
                match self
                    .feature
                    .place(world, tags, random, (pos.x, pos.y, pos.z))
                {
                    Ok(true) => placed = true,
                    Ok(false) => {}
                    Err(e) => failure = Some(e),
                }
                Ok(())
            },
        )
        .map_err(|e| Unsupported(e.0))?;
        match failure {
            Some(e) => Err(e),
            None => Ok(placed),
        }
    }
}
