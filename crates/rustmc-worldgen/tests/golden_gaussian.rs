//! `nextGaussian`, against the JVM's own values.
//!
//! Every other RNG primitive this port uses consumes a fixed number of bits,
//! and `golden_rng` covers those. `nextGaussian` does not: Marsaglia polar
//! draws two `nextDouble`s per attempt, **retries** until the point lands
//! inside the unit circle, and caches the second value of the accepted pair
//! for the following call. The first gaussian on a stream therefore costs
//! two-or-more draws and the second costs none.
//!
//! It had no gate until now, and it reaches worldgen through `Mth.normal` ->
//! `ClampedNormalFloat` -> `SpeleothemClusterFeature.randomBetweenBiased`,
//! which runs once per speleothem in every cluster in the world. A port that
//! got the retry loop or the cache wrong would desynchronise the feature
//! stream from the first cluster onward while still producing a perfectly
//! plausible normal distribution — the kind of fault a statistical check
//! passes and a golden vector catches.
//!
//! # What is asserted exactly, and what is counted
//!
//! Everything that decides **how many numbers come off the stream** is
//! asserted bit-for-bit: the stream position after 256 gaussians, the
//! interleaved longs and doubles, and `setSeed` discarding the cached half of
//! a pair. Those are the properties worldgen parity depends on.
//!
//! The gaussian *values* are asserted to within 1 ulp and the exact matches
//! are counted, because `Math.log` is a HotSpot intrinsic that is within 1
//! ulp of the true logarithm and **not correctly rounded** — it disagrees
//! with `StrictMath.log` on 7.3% of inputs, so there is no "the JVM's log"
//! to port short of Intel's x86 assembly. `precise_log` is correctly rounded
//! instead, which agrees with the JVM more often than `f64::ln` does and is
//! a target that does not move with the JVM or the architecture.
//!
//! **This is not a widened tolerance.** The 1-ulp ceiling is an assertion
//! that fails on any structural fault; the exact-match count is a ratchet
//! with a named cause; and the numbers that matter — draw counts — are
//! compared with no latitude at all. See ORACLE.md §5c.
//!
//! # Three cases, because they fail independently
//!
//! - **sequence** — 256 consecutive values, plus the `nextLong` after them.
//!   The trailing long is the load-bearing part: a port whose retry loop ran
//!   a different number of times could match every gaussian by accident of
//!   the cache and still leave the stream in the wrong place.
//! - **interleaved** — a scripted mix of gaussian, long and double calls, so
//!   a plain draw follows a gaussian both when its cache is full and when it
//!   is empty.
//! - **reseeded** — `setSeed` must discard the cached value. A port that kept
//!   it returns a number belonging to the *previous* seed, which is related
//!   to the right answer in a way no distribution check would notice.

mod common;

use common::read;

/// One gaussian from a `WorldgenRandom`, then `set_seed`, then another.
///
/// Separate from [`Source`] because `WorldgenRandom` is generic over its
/// backing, so the two rngs are different types rather than one enum arm.
fn worldgen_reseed_pair(rng: &str, seed: i64) -> (u64, u64) {
    fn pair<B: rustmc_worldgen::rng::WorldgenBacking>(backing: B, seed: i64) -> (u64, u64) {
        let mut source = rustmc_worldgen::rng::WorldgenRandom::new(backing);
        let first = source.next_gaussian().to_bits();
        source.set_seed(seed);
        (first, source.next_gaussian().to_bits())
    }
    match rng {
        "legacy_lcg" => pair(LegacyRandomSource::new(seed), seed),
        "xoroshiro128pp" => pair(XoroshiroRandomSource::from_seed(seed), seed),
        other => panic!("unknown rng `{other}`"),
    }
}

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::xoroshiro::XoroshiroRandomSource;
use rustmc_worldgen::rng::RandomSource;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/gaussian_vectors.json");

/// One of the two sources, behind a single name so a case can pick by string.
enum Source {
    Legacy(LegacyRandomSource),
    Xoroshiro(XoroshiroRandomSource),
}

impl Source {
    fn new(rng: &str, seed: i64) -> Self {
        match rng {
            "legacy_lcg" => Self::Legacy(LegacyRandomSource::new(seed)),
            "xoroshiro128pp" => Self::Xoroshiro(XoroshiroRandomSource::from_seed(seed)),
            other => panic!("unknown rng `{other}`"),
        }
    }

    fn as_random(&mut self) -> &mut dyn RandomSource {
        match self {
            Self::Legacy(r) => r,
            Self::Xoroshiro(r) => r,
        }
    }
}

fn long_field(case: &serde_json::Value, field: &str) -> i64 {
    case[field]
        .as_str()
        .unwrap_or_else(|| panic!("case has no string `{field}`"))
        .parse()
        .expect("i64")
}

/// What a brand-new source of this kind gives, for the `setSeed` check.
fn first_of_a_fresh_source(rng: &str, seed: i64) -> u64 {
    Source::new(rng, seed).as_random().next_gaussian().to_bits()
}

fn long_list(case: &serde_json::Value, field: &str) -> Vec<i64> {
    case[field]
        .as_array()
        .unwrap_or_else(|| panic!("case has no `{field}`"))
        .iter()
        .map(|v| v.as_str().expect("string").parse().expect("i64"))
        .collect()
}

#[test]
fn the_gaussian_stream_matches_the_jvm() {
    let doc = read(GOLDEN);
    let cases = doc["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "the vector file has no cases");

    let mut sequences = 0usize;
    let mut interleaves = 0usize;
    let mut reseeds = 0usize;
    let mut values = 0usize;
    // Collected rather than asserted one at a time. A gate that dies on the
    // first disagreement reports "one value is wrong"; the useful question is
    // whether ten thousand are wrong or eleven, and by how much — a
    // last-bit disagreement and a structural one look identical at n=1.
    let mut wrong = 0usize;
    let mut worst_ulp = 0i64;
    let mut examples: Vec<String> = Vec::new();
    let mut ulp_histogram: std::collections::BTreeMap<i64, usize> =
        std::collections::BTreeMap::new();

    let mut long_runs = 0usize;
    let mut long_run_wrong = 0usize;
    let mut long_run_draws = 0usize;
    for case in cases {
        let kind = case["case"].as_str().expect("case");
        let rng = case["rng"].as_str().expect("rng");
        let seed = long_field(case, "seed");
        let mut source = Source::new(rng, seed);

        match kind {
            "sequence" => {
                for (i, want) in long_list(case, "output_bits").iter().enumerate() {
                    let got = source.as_random().next_gaussian().to_bits();
                    #[expect(clippy::cast_sign_loss, reason = "raw IEEE-754 bits through an i64")]
                    let want_bits = *want as u64;
                    values += 1;
                    if got != want_bits {
                        wrong += 1;
                        #[expect(
                            clippy::cast_possible_wrap,
                            reason = "an ulp distance between two finite doubles"
                        )]
                        let distance = (got as i64 - want_bits as i64).abs();
                        worst_ulp = worst_ulp.max(distance);
                        *ulp_histogram.entry(distance).or_insert(0) += 1;
                        // The ceiling, asserted. Anything past one ulp is not
                        // the logarithm's last bit and must not be counted as
                        // though it were.
                        assert!(
                            distance <= 1,
                            "{rng} seed {seed} gaussian {i}: {distance} ulp — too far to \
                             be `Math.log`'s rounding, so this is a real fault"
                        );
                        if examples.len() < 5 {
                            examples.push(format!(
                                "{rng} seed {seed} gaussian {i}: got {:.17e}, want {:.17e} \
                                 ({distance} ulp)",
                                f64::from_bits(got),
                                f64::from_bits(want_bits)
                            ));
                        }
                    }
                }
                // Where the stream ended up, which the values alone do not
                // pin down. See the module docs.
                assert_eq!(
                    source.as_random().next_i64(),
                    long_field(case, "next_long_after"),
                    "{rng} seed {seed}: the stream is in the wrong place — the \
                     retry loop drew the wrong number of times"
                );
                sequences += 1;
            }
            "interleaved" => {
                let kinds: Vec<&str> = case["kinds"]
                    .as_array()
                    .expect("kinds")
                    .iter()
                    .map(|v| v.as_str().expect("kind"))
                    .collect();
                let want = long_list(case, "output_bits");
                assert_eq!(kinds.len(), want.len(), "script and outputs disagree");
                for (i, (kind, want)) in kinds.iter().zip(&want).enumerate() {
                    #[expect(clippy::cast_sign_loss, reason = "raw bits through an i64")]
                    let want_bits = *want as u64;
                    let got = match *kind {
                        "g" => source.as_random().next_gaussian().to_bits(),
                        "d" => source.as_random().next_f64().to_bits(),
                        #[expect(clippy::cast_sign_loss, reason = "a raw 64-bit draw")]
                        "l" => source.as_random().next_i64() as u64,
                        other => panic!("unknown script step `{other}`"),
                    };
                    values += 1;
                    // A long or a double is a raw draw with no arithmetic on
                    // it, so it is exact and there is nothing to allow for.
                    // This is the assertion that catches a gaussian consuming
                    // or caching the wrong number of values, and it is the
                    // reason the script interleaves them at all.
                    if *kind != "g" {
                        assert_eq!(
                            got, want_bits,
                            "{rng} seed {seed}: step {i} (`{kind}`) diverged — the \
                             gaussian before it consumed or cached the wrong number \
                             of values"
                        );
                        continue;
                    }
                    if got != want_bits {
                        wrong += 1;
                        #[expect(clippy::cast_possible_wrap, reason = "an ulp distance")]
                        let distance = (got as i64 - want_bits as i64).abs();
                        assert!(
                            distance <= 1,
                            "{rng} seed {seed}: step {i} is {distance} ulp out"
                        );
                        worst_ulp = worst_ulp.max(distance);
                        *ulp_histogram.entry(distance).or_insert(0) += 1;
                    }
                }
                interleaves += 1;
            }
            // A million gaussians, every thousandth compared. This is the
            // stream-position check: a rejection loop or cache that consumes
            // the wrong number of draws desynchronises everything after it,
            // and the short sequences above are too small a window to catch a
            // bug that only bites on an unusual pair.
            "long_run" => {
                // Plain JSON numbers, not the longs-as-strings the bit
                // fields use — they are small and exact as doubles.
                let stride =
                    usize::try_from(case["stride"].as_u64().expect("stride")).expect("stride fits");
                let length =
                    usize::try_from(case["length"].as_u64().expect("length")).expect("length fits");
                let want = long_list(case, "sampled_bits");
                let mut wrong_here = 0usize;
                let mut sample = 0usize;
                for i in 0..length {
                    let got = source.as_random().next_gaussian().to_bits();
                    if (i + 1) % stride != 0 {
                        continue;
                    }
                    #[expect(clippy::cast_sign_loss, reason = "raw bits round-trip")]
                    let want_bits = want[sample] as u64;
                    sample += 1;
                    if got == want_bits {
                        continue;
                    }
                    // The same 1-ulp allowance the interleaved case makes,
                    // and for the same reason: `Math.log` is not correctly
                    // rounded. Anything larger is a desync, not a rounding
                    // difference, and the count is reported either way.
                    #[expect(clippy::cast_possible_wrap, reason = "an ulp distance")]
                    let distance = (got as i64 - want_bits as i64).abs();
                    assert!(
                        distance <= 1,
                        "{rng} seed {seed}: sample {sample} of the long run is {distance} \
                         ulp out — that is a desynchronised stream, not rounding"
                    );
                    wrong_here += 1;
                }
                assert_eq!(sample, want.len(), "sampled every stride-th value");
                long_run_wrong += wrong_here;
                long_run_draws += length;
                long_runs += 1;
            }
            // The mirror image of `reseeded`, and it must come out the
            // opposite way. `WorldgenRandom` overrides `setSeed` without
            // calling `super`, so the gaussian cache it inherits is never
            // reset and a cached half survives into the next feature. Port
            // the parent's reset here and every feature that draws an odd
            // number of gaussians desynchronises the one after it.
            "worldgen_reseeded" => {
                #[expect(clippy::cast_sign_loss, reason = "raw bits through an i64")]
                let want_after = long_field(case, "after_reseed_bits") as u64;
                #[expect(clippy::cast_sign_loss, reason = "raw bits through an i64")]
                let want_first = long_field(case, "first_bits") as u64;
                #[expect(clippy::cast_sign_loss, reason = "raw bits through an i64")]
                let want_scratch = long_field(case, "from_scratch_bits") as u64;

                let (first, after) = worldgen_reseed_pair(rng, seed);
                assert_ne!(
                    want_after, want_scratch,
                    "the vector should show the cache SURVIVING — if these are \
                     equal, vanilla changed and this gate is now backwards"
                );
                assert_eq!(
                    first, want_first,
                    "{rng} seed {seed}: first gaussian from a WorldgenRandom"
                );
                assert_eq!(
                    after, want_after,
                    "{rng} seed {seed}: after setSeed a WorldgenRandom must return \
                     the CACHED partner, not a fresh draw — its setSeed override \
                     never calls super, so the inherited gaussian source is not reset"
                );
            }
            "reseeded" => {
                let first = source.as_random().next_gaussian().to_bits();
                source.as_random().set_seed(seed);
                let after = source.as_random().next_gaussian().to_bits();

                // The load-bearing assertion, and it is exact: after
                // `setSeed` the source must be indistinguishable from a fresh
                // one. A port that kept the cached half returns `first`'s
                // partner here — a *different* number, not a differently
                // rounded one, so no ulp latitude applies.
                assert_eq!(
                    after,
                    first_of_a_fresh_source(rng, seed),
                    "{rng} seed {seed}: setSeed did not discard the cached half of \
                     the pair, so this value belongs to the previous seed"
                );

                #[expect(clippy::cast_sign_loss, reason = "raw bits through an i64")]
                let want_first = long_field(case, "first_bits") as u64;
                #[expect(clippy::cast_sign_loss, reason = "raw bits through an i64")]
                let want_after = long_field(case, "after_reseed_bits") as u64;
                for (got, want) in [(first, want_first), (after, want_after)] {
                    if got != want {
                        wrong += 1;
                        #[expect(clippy::cast_possible_wrap, reason = "an ulp distance")]
                        let distance = (got as i64 - want as i64).abs();
                        assert!(distance <= 1, "{rng} seed {seed}: {distance} ulp out");
                        worst_ulp = worst_ulp.max(distance);
                        *ulp_histogram.entry(distance).or_insert(0) += 1;
                    }
                }

                // The oracle's own consistency check: it recorded a fresh
                // source's first gaussian separately, and the two must agree.
                // Asserted so a broken *dumper* fails here rather than
                // quietly redefining what the gate is testing.
                assert_eq!(
                    long_field(case, "after_reseed_bits"),
                    long_field(case, "from_scratch_bits"),
                    "the oracle disagrees with itself about setSeed"
                );
                values += 2;
                reseeds += 1;
            }
            other => panic!("unknown case `{other}`"),
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a percentage over a value count"
    )]
    let exactness = 100.0 * (values - wrong) as f64 / values as f64;
    println!(
        "GAUSSIAN EXACTNESS {exactness:.6}% over {values} values ({wrong} wrong) — \
         {sequences} sequences, {interleaves} interleaved scripts, {reseeds} reseeds"
    );
    // The long runs are the stream-position evidence and are worth their own
    // line: they are three orders of magnitude more draws than everything
    // above, and a desync in any of them fails the assertion outright.
    println!(
        "  long runs: {long_runs} cases, {long_run_draws} draws sampled every stride, \
         {long_run_wrong} sampled values 1 ulp out"
    );
    assert!(long_runs > 0, "the vector carries no long_run case");
    if wrong > 0 {
        println!(
            "  worst distance: {worst_ulp} ulp — `Math.log` is a HotSpot intrinsic and \
             is not correctly rounded; see ORACLE.md 5c"
        );
        println!("  by distance:");
        for (distance, n) in &ulp_histogram {
            println!("    {distance:5} ulp  {n:7}");
        }
        for line in &examples {
            println!("  {line}");
        }
    }
    assert!(
        sequences >= 2 && interleaves >= 2 && reseeds >= 2,
        "thin coverage"
    );

    // Zero, and it was not zero an hour ago: with `f64::ln` this read two.
    // Both were one ulp, both were the same `Math.log` call, and
    // `precise_log` closed them.
    //
    // Left as an equality rather than a bound because that is what the
    // measurement supports. The 1-ulp ceiling asserted per value above is the
    // thing that says *what a future failure means* — a last-bit difference
    // is the JVM's non-correctly-rounded intrinsic and is worth
    // investigating; anything wider is a structural fault.
    assert_eq!(
        wrong, 0,
        "the logarithm moved away from the JVM — see the ulp histogram above"
    );
}
