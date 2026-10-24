//! Which chunks a structure set may start in.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `StructurePlacement`, `RandomSpreadStructurePlacement`,
//! `RandomSpreadType`, `StructurePlacement.{FrequencyReductionMethod,
//! ExclusionZone}`, `ChunkGeneratorStructureState#hasStructureChunkInRange`.
//!
//! Structures are the largest single item left in the whole-world diff:
//! roughly 2,400 blocks of the 5,374 that remain are one mineshaft, its
//! corridors carved to `cave_air` and its supports never built (BENCH.md).
//! This is the floor of that work and it is worth having on its own — a
//! generator that builds the right pieces in the wrong chunks is not closer
//! than one that builds nothing.
//!
//! # It is a pure function of the seed and the chunk
//!
//! Nothing here reads the world. Every placement is decided by a
//! `WorldgenRandom` seeded from `(levelSeed, region, salt)`, so this whole
//! module can be gated against vanilla without generating a single block —
//! which is why it is the first structure code written rather than the last.
//!
//! # Three things that look like one
//!
//! `isStructureChunk` is a conjunction of three independent tests, and 19 of
//! the game's 20 structure sets use only the first:
//!
//! ```text
//! isPlacementChunk(x, z)                 // the spread grid
//!   && applyAdditionalChunkRestrictions  // the frequency reducer
//!   && applyInteractionsWithOtherStructures  // the exclusion zone
//! ```
//!
//! The **grid** picks one chunk per `spacing x spacing` region, jittered
//! within `spacing - separation` by two draws. A chunk is a placement chunk
//! only if the jittered pick lands on it — so `separation` does not reserve a
//! border, it shrinks the jitter and makes the picks in adjacent regions
//! further apart.
//!
//! The **frequency reducer** is a second, independent coin, and there are
//! four of them that are not interchangeable. `mineshafts` has
//! `spacing: 1, separation: 0`, so its grid accepts *every* chunk and the
//! reducer does all the work: a mineshaft is `nextDouble() < 0.004` and
//! nothing else.
//!
//! The **exclusion zone** is the only part that is not local: it asks whether
//! *another* structure set has a chunk within a radius, which means running
//! that set's whole placement over a square of chunks. One set uses it
//! (`pillager_outposts`, against `villages`), and it is a `hasStructureChunk`
//! call — the *other* set's full three-part test, not just its grid.

use crate::rng::legacy::LegacyRandomSource;
use crate::rng::worldgen_random::WorldgenRandom;
use crate::rng::RandomSource;

/// `RandomSpreadType` — how the jitter inside a region is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpreadType {
    /// One `nextInt(bound)`.
    #[default]
    Linear,
    /// **Two** `nextInt(bound)` averaged, which both changes the
    /// distribution and doubles the draws.
    Triangular,
}

impl SpreadType {
    fn evaluate<R: RandomSource + ?Sized>(self, random: &mut R, bound: i32) -> i32 {
        match self {
            Self::Linear => random.next_i32_bound(bound),
            Self::Triangular => {
                let first = random.next_i32_bound(bound);
                let second = random.next_i32_bound(bound);
                (first + second) / 2
            }
        }
    }

    /// # Errors
    /// If the name is not one of the two.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "minecraft:linear" | "linear" => Ok(Self::Linear),
            "minecraft:triangular" | "triangular" => Ok(Self::Triangular),
            other => Err(format!("unknown spread type `{other}`")),
        }
    }
}

/// `StructurePlacement.FrequencyReductionMethod`.
///
/// Four coins that all answer "does this chunk survive the frequency roll",
/// and no two of them draw the same way. The enum order is the reference's,
/// because the codec is `fromEnum` and the names below are its serialised
/// forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FrequencyReduction {
    /// `probabilityReducer` — salted, `nextFloat()`.
    #[default]
    Default,
    /// `legacyPillagerOutpostReducer` — the odd one out. It seeds from a
    /// **region** coordinate (`>> 4`), throws away the first `nextInt()`, and
    /// then asks `nextInt((int)(1 / frequency)) == 0` rather than comparing
    /// against the frequency directly.
    LegacyType1,
    /// `legacyArbitrarySaltProbabilityReducer` — salted with a hardcoded
    /// 10387320 that has nothing to do with the set's own salt.
    LegacyType2,
    /// `legacyProbabilityReducerWithDouble` — **ignores the salt entirely**
    /// and compares a `nextDouble()` against the frequency widened to double.
    LegacyType3,
}

impl FrequencyReduction {
    /// # Errors
    /// If the name is not one of the four.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name.strip_prefix("minecraft:").unwrap_or(name) {
            "default" => Ok(Self::Default),
            "legacy_type_1" => Ok(Self::LegacyType1),
            "legacy_type_2" => Ok(Self::LegacyType2),
            "legacy_type_3" => Ok(Self::LegacyType3),
            other => Err(format!("unknown frequency reduction method `{other}`")),
        }
    }

    fn should_generate(self, seed: i64, salt: i32, x: i32, z: i32, frequency: f32) -> bool {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        match self {
            Self::Default => {
                random.set_large_feature_with_salt(seed, x, z, salt);
                random.next_f32() < frequency
            }
            Self::LegacyType1 => {
                // The region, not the chunk — and the seed is mixed by hand
                // rather than through `setLargeFeatureSeed`.
                let region_x = x >> 4;
                let region_z = z >> 4;
                random.set_seed(i64::from(region_x ^ (region_z << 4)) ^ seed);
                // One value discarded. Removing it is the single easiest way
                // to write a placement that is right about everything except
                // where the outposts are.
                let _ = random.next_i32();
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "`(int)(1.0F / frequency)` — the truncation is the reference's"
                )]
                let bound = (1.0f32 / frequency) as i32;
                random.next_i32_bound(bound) == 0
            }
            Self::LegacyType2 => {
                // A hardcoded salt, not the placement's own.
                random.set_large_feature_with_salt(seed, x, z, 10_387_320);
                random.next_f32() < frequency
            }
            Self::LegacyType3 => {
                // No salt at all, and a double against a widened float.
                random.set_large_feature_seed(seed, x, z);
                random.next_f64() < f64::from(frequency)
            }
        }
    }
}

/// `StructurePlacement.ExclusionZone`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExclusionZone {
    /// The set whose chunks forbid this one.
    pub other_set: String,
    /// The radius, in chunks, searched in both axes inclusive.
    pub chunk_count: i32,
}

/// The spread shape. Only `random_spread` is here; `concentric_rings` is one
/// set (`strongholds`) and needs a biome search, so it is named rather than
/// guessed at.
#[derive(Clone, Debug, PartialEq)]
pub enum Spread {
    RandomSpread {
        spacing: i32,
        separation: i32,
        spread_type: SpreadType,
    },
    /// `concentric_rings` — parsed so a set that uses it can be *named* in a
    /// refusal instead of silently answering "no structure here", which is
    /// what a missing arm would do.
    ConcentricRings,
}

/// One structure set's placement.
#[derive(Clone, Debug, PartialEq)]
pub struct StructurePlacement {
    pub spread: Spread,
    pub frequency: f32,
    pub frequency_reduction: FrequencyReduction,
    pub salt: i32,
    pub exclusion_zone: Option<ExclusionZone>,
}

/// This port cannot answer for that placement, named.
#[derive(Debug, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl StructurePlacement {
    /// Parse one `worldgen/structure_set/*.json`'s `placement`.
    ///
    /// # Errors
    /// If a field is present and malformed. Absent optional fields take the
    /// codec's defaults — `frequency` is 1.0 and the reduction method is
    /// `default`, which together mean "no second coin at all", and 16 of the
    /// 20 sets rely on exactly that.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "placement has no `type`".to_string())?;
        let int = |name: &str, default: i32| -> Result<i32, String> {
            match value.get(name) {
                None => Ok(default),
                Some(v) => v
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("placement `{name}` is not an integer")),
            }
        };
        let spread = match kind.strip_prefix("minecraft:").unwrap_or(kind) {
            "random_spread" => Spread::RandomSpread {
                spacing: int("spacing", 1)?,
                separation: int("separation", 0)?,
                spread_type: match value.get("spread_type") {
                    None => SpreadType::default(),
                    Some(v) => SpreadType::parse(
                        v.as_str()
                            .ok_or_else(|| "spread_type is not a name".to_string())?,
                    )?,
                },
            },
            "concentric_rings" => Spread::ConcentricRings,
            other => return Err(format!("unknown placement type `{other}`")),
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the codec's field is a float, compared against a float"
        )]
        let frequency = value
            .get("frequency")
            .and_then(serde_json::Value::as_f64)
            .map_or(1.0f32, |v| v as f32);
        let exclusion_zone = match value.get("exclusion_zone") {
            None => None,
            Some(zone) => Some(ExclusionZone {
                other_set: zone
                    .get("other_set")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "exclusion_zone has no `other_set`".to_string())?
                    .to_string(),
                chunk_count: zone
                    .get("chunk_count")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| "exclusion_zone has no `chunk_count`".to_string())?,
            }),
        };
        Ok(Self {
            spread,
            frequency,
            frequency_reduction: match value.get("frequency_reduction_method") {
                None => FrequencyReduction::default(),
                Some(v) => FrequencyReduction::parse(
                    v.as_str()
                        .ok_or_else(|| "frequency_reduction_method is not a name".to_string())?,
                )?,
            },
            salt: int("salt", 0)?,
            exclusion_zone,
        })
    }

    /// `RandomSpreadStructurePlacement#getPotentialStructureChunk` — the one
    /// chunk this set may start in, within the region containing `(x, z)`.
    ///
    /// # Errors
    /// If this is a `concentric_rings` placement.
    pub fn potential_structure_chunk(
        &self,
        seed: i64,
        x: i32,
        z: i32,
    ) -> Result<(i32, i32), Unsupported> {
        let Spread::RandomSpread {
            spacing,
            separation,
            spread_type,
        } = self.spread
        else {
            return Err(Unsupported("concentric_rings placement".to_string()));
        };
        let region_x = x.div_euclid(spacing);
        let region_z = z.div_euclid(spacing);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_with_salt(seed, region_x, region_z, self.salt);
        let bound = spacing - separation;
        // Two evaluations, x then z — and with a triangular spread that is
        // four draws, not two.
        let offset_x = spread_type.evaluate(&mut random, bound);
        let offset_z = spread_type.evaluate(&mut random, bound);
        Ok((region_x * spacing + offset_x, region_z * spacing + offset_z))
    }

    /// `RandomSpreadStructurePlacement#isPlacementChunk` — the grid alone.
    ///
    /// # Errors
    /// If this is a `concentric_rings` placement.
    pub fn is_placement_chunk(&self, seed: i64, x: i32, z: i32) -> Result<bool, Unsupported> {
        Ok(self.potential_structure_chunk(seed, x, z)? == (x, z))
    }

    /// `StructurePlacement#applyAdditionalChunkRestrictions` — the frequency
    /// coin, which is skipped entirely when the frequency is 1.
    #[must_use]
    pub fn passes_frequency(&self, seed: i64, x: i32, z: i32) -> bool {
        if self.frequency >= 1.0 {
            return true;
        }
        self.frequency_reduction
            .should_generate(seed, self.salt, x, z, self.frequency)
    }

    /// `StructurePlacement#isStructureChunk`, without the exclusion zone.
    ///
    /// Separate from [`Self::is_structure_chunk`] because the exclusion zone
    /// needs every *other* set's placement, and 19 of 20 sets do not have
    /// one — a caller holding a single set should not have to supply a
    /// registry it will never read.
    ///
    /// # Errors
    /// If this is a `concentric_rings` placement.
    pub fn is_structure_chunk_ignoring_exclusion(
        &self,
        seed: i64,
        x: i32,
        z: i32,
    ) -> Result<bool, Unsupported> {
        Ok(self.is_placement_chunk(seed, x, z)? && self.passes_frequency(seed, x, z))
    }

    /// `StructurePlacement#isStructureChunk`, in full.
    ///
    /// `other` resolves the exclusion zone's set by name. Returning `None`
    /// from it is an error rather than "no exclusion": a zone naming a set
    /// the caller cannot find is a missing registry, and treating it as
    /// permission would put outposts inside villages.
    ///
    /// # Errors
    /// If this or the excluded set is a `concentric_rings` placement, or the
    /// excluded set cannot be resolved.
    pub fn is_structure_chunk(
        &self,
        seed: i64,
        x: i32,
        z: i32,
        other: &dyn Fn(&str) -> Option<Self>,
    ) -> Result<bool, Unsupported> {
        if !self.is_structure_chunk_ignoring_exclusion(seed, x, z)? {
            return Ok(false);
        }
        let Some(zone) = &self.exclusion_zone else {
            return Ok(true);
        };
        let excluded = other(&zone.other_set)
            .ok_or_else(|| Unsupported(format!("exclusion zone names {}", zone.other_set)))?;
        // `hasStructureChunkInRange` — the other set's **full** test over an
        // inclusive square, short-circuiting on the first hit. Recursive in
        // principle; in practice no exclusion zone points at a set that has
        // one, and a cycle would show up here as unbounded recursion rather
        // than a wrong answer.
        for other_x in (x - zone.chunk_count)..=(x + zone.chunk_count) {
            for other_z in (z - zone.chunk_count)..=(z + zone.chunk_count) {
                if excluded.is_structure_chunk(seed, other_x, other_z, other)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> StructurePlacement {
        StructurePlacement::parse(&serde_json::from_str(json).expect("json")).expect("parse")
    }

    /// `mineshafts`: spacing 1, so the grid accepts every chunk and the
    /// frequency coin is the whole feature.
    #[test]
    fn a_spacing_of_one_makes_every_chunk_a_placement_chunk() {
        let mineshafts = parse(
            r#"{"type":"minecraft:random_spread","frequency":0.004,
                "frequency_reduction_method":"legacy_type_3","salt":0,
                "separation":0,"spacing":1}"#,
        );
        for (x, z) in [(0, 0), (5, -7), (-100, 100), (1234, -5678)] {
            assert!(
                mineshafts.is_placement_chunk(20_260_722, x, z).expect("ok"),
                "({x}, {z}) is not a placement chunk"
            );
        }
    }

    /// ...and at 0.4% the reducer accepts a small fraction of them. A
    /// distribution check rather than a value check — the values are gated
    /// against vanilla in `golden_structure_placement`; this is here so the
    /// two coins cannot both be wired to "always true" and still pass.
    #[test]
    fn the_mineshaft_frequency_coin_is_the_whole_feature() {
        let mineshafts = parse(
            r#"{"type":"minecraft:random_spread","frequency":0.004,
                "frequency_reduction_method":"legacy_type_3","salt":0,
                "separation":0,"spacing":1}"#,
        );
        let mut hits = 0;
        for x in 0..200 {
            for z in 0..200 {
                if mineshafts
                    .is_structure_chunk_ignoring_exclusion(20_260_722, x, z)
                    .expect("ok")
                {
                    hits += 1;
                }
            }
        }
        // 40,000 chunks at 0.4% is 160 expected.
        assert!(
            (80..260).contains(&hits),
            "{hits} mineshafts in 40,000 chunks — expected about 160"
        );
    }

    /// A grid with spacing 34 picks exactly one chunk per 34x34 region, and
    /// that chunk is inside the region.
    #[test]
    fn the_grid_picks_one_chunk_per_region() {
        let villages = parse(
            r#"{"type":"minecraft:random_spread","salt":10387312,
                "separation":8,"spacing":34}"#,
        );
        let seed = 20_260_722;
        let mut found = Vec::new();
        for x in 0..34 {
            for z in 0..34 {
                if villages.is_placement_chunk(seed, x, z).expect("ok") {
                    found.push((x, z));
                }
            }
        }
        assert_eq!(found.len(), 1, "{found:?}");
        let (x, z) = found[0];
        // The jitter is bounded by spacing - separation, not by spacing.
        assert!((0..26).contains(&x) && (0..26).contains(&z), "({x}, {z})");
    }

    /// Negative coordinates go through `floorDiv`, not a truncating divide.
    /// Getting this wrong makes the region containing chunk -1 the same as
    /// the one containing chunk 0, which duplicates a structure across the
    /// origin and is invisible in any test that stays positive.
    #[test]
    fn regions_are_floor_divided_so_the_origin_is_not_special() {
        let villages = parse(
            r#"{"type":"minecraft:random_spread","salt":10387312,
                "separation":8,"spacing":34}"#,
        );
        let seed = 7;
        let near_origin = villages
            .potential_structure_chunk(seed, -1, -1)
            .expect("ok");
        let in_region_zero = villages.potential_structure_chunk(seed, 0, 0).expect("ok");
        assert_ne!(
            near_origin, in_region_zero,
            "chunk -1 and chunk 0 are in different regions"
        );
        assert!(near_origin.0 < 0 && near_origin.1 < 0, "{near_origin:?}");
    }

    /// A triangular spread draws **twice** per axis. Two placements
    /// identical but for the spread type must disagree somewhere, or the
    /// second `nextInt` is not being taken.
    #[test]
    fn a_triangular_spread_is_four_draws_not_two() {
        let linear = parse(
            r#"{"type":"minecraft:random_spread","salt":10387313,
                "separation":11,"spacing":20}"#,
        );
        let triangular = parse(
            r#"{"type":"minecraft:random_spread","salt":10387313,
                "separation":11,"spacing":20,"spread_type":"triangular"}"#,
        );
        let differing = (0..40)
            .filter(|&i| {
                linear.potential_structure_chunk(99, i * 20, 0)
                    != triangular.potential_structure_chunk(99, i * 20, 0)
            })
            .count();
        assert!(differing > 10, "only {differing} of 40 regions differ");
    }

    /// The four reducers are four different functions. A port that wired
    /// them all to the same coin would pass every distribution check.
    #[test]
    fn the_four_reducers_disagree() {
        use FrequencyReduction::{Default, LegacyType1, LegacyType2, LegacyType3};
        let methods = [Default, LegacyType1, LegacyType2, LegacyType3];
        let mut signatures = Vec::new();
        for method in methods {
            let accepted: Vec<(i32, i32)> = (0..60)
                .flat_map(|x| (0..60).map(move |z| (x, z)))
                .filter(|&(x, z)| method.should_generate(4242, 165_745_296, x, z, 0.2))
                .collect();
            assert!(!accepted.is_empty(), "{method:?} accepted nothing at all");
            signatures.push(accepted);
        }
        for i in 0..signatures.len() {
            for j in (i + 1)..signatures.len() {
                assert_ne!(
                    signatures[i], signatures[j],
                    "{:?} and {:?} accept the same chunks",
                    methods[i], methods[j]
                );
            }
        }
    }

    /// `legacy_type_3` ignores the salt. Asserted because it is the one the
    /// mineshafts use, and "pass the salt through everywhere" is the obvious
    /// way to write these four.
    #[test]
    fn legacy_type_3_ignores_the_salt() {
        for (x, z) in [(0, 0), (13, -4), (-9, 21)] {
            assert_eq!(
                FrequencyReduction::LegacyType3.should_generate(1, 0, x, z, 0.5),
                FrequencyReduction::LegacyType3.should_generate(1, 999_999, x, z, 0.5),
            );
        }
    }

    /// ...and `default` does not, which is what makes the previous test a
    /// statement about `legacy_type_3` rather than about the seeding.
    #[test]
    fn the_default_reducer_does_not_ignore_the_salt() {
        let differing = (0..200)
            .filter(|&x| {
                FrequencyReduction::Default.should_generate(1, 0, x, 0, 0.5)
                    != FrequencyReduction::Default.should_generate(1, 999_999, x, 0, 0.5)
            })
            .count();
        assert!(differing > 10, "only {differing} of 200 differ");
    }

    /// An exclusion zone consults the *other* set's full test, and refuses
    /// rather than guessing when that set cannot be resolved.
    #[test]
    fn an_unresolvable_exclusion_zone_refuses() {
        let outposts = parse(
            r#"{"type":"minecraft:random_spread","salt":165745296,
                "separation":8,"spacing":32,"frequency":0.2,
                "frequency_reduction_method":"legacy_type_1",
                "exclusion_zone":{"chunk_count":10,"other_set":"minecraft:villages"}}"#,
        );
        assert_eq!(
            outposts.exclusion_zone.as_ref().map(|z| z.chunk_count),
            Some(10)
        );
        // A chunk the grid and the frequency both accept, so the exclusion
        // zone is actually reached. 256x256 chunks is 64 regions of 32, and
        // at a frequency of 0.2 that is about a dozen outposts — 64x64 is
        // four regions and expects less than one, which is how this test
        // first passed by proving nothing.
        let seed = 20_260_722;
        let mut reached = false;
        for x in 0..256 {
            for z in 0..256 {
                if outposts
                    .is_structure_chunk_ignoring_exclusion(seed, x, z)
                    .expect("ok")
                {
                    reached = true;
                    assert_eq!(
                        outposts.is_structure_chunk(seed, x, z, &|_| None),
                        Err(Unsupported(
                            "exclusion zone names minecraft:villages".to_string()
                        ))
                    );
                }
            }
        }
        assert!(
            reached,
            "no outpost chunk in 256x256 — the test proved nothing"
        );
    }

    /// `concentric_rings` is named, not silently answered.
    #[test]
    fn concentric_rings_refuses_by_name() {
        // `r##`, not `r#`: the JSON contains `"#minecraft:...` for the biome
        // tag, and `"#` closes an `r#` string in the middle of a tag name.
        let strongholds = parse(
            r##"{"type":"minecraft:concentric_rings","count":128,"distance":32,
                "preferred_biomes":"#minecraft:stronghold_biased_to","salt":0,"spread":3}"##,
        );
        assert_eq!(
            strongholds.is_placement_chunk(1, 0, 0),
            Err(Unsupported("concentric_rings placement".to_string()))
        );
    }

    /// Every structure set in the datapack parses, and the survey of what
    /// they use is asserted rather than remembered — a future version adding
    /// a fifth reducer should fail here, not in a parity number.
    #[test]
    fn every_structure_set_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/structure_set"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let (mut total, mut random_spread, mut with_frequency, mut with_exclusion) = (0, 0, 0, 0);
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            let placement = StructurePlacement::parse(&json["placement"])
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            total += 1;
            if matches!(placement.spread, Spread::RandomSpread { .. }) {
                random_spread += 1;
            }
            if placement.frequency < 1.0 {
                with_frequency += 1;
            }
            if placement.exclusion_zone.is_some() {
                with_exclusion += 1;
            }
        }
        assert_eq!(total, 20, "the structure set registry changed size");
        assert_eq!(random_spread, 19, "only strongholds is concentric_rings");
        assert_eq!(with_frequency, 3, "buried_treasure, mineshafts, outposts");
        assert_eq!(with_exclusion, 1, "only pillager_outposts");
        println!(
            "STRUCTURE SETS: {total} parsed, {random_spread} random_spread, \
             {with_frequency} with a frequency coin, {with_exclusion} with an exclusion zone"
        );
    }
}
