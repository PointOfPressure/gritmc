//! What order this port actually runs step 6 in.
//!
//! `golden_feature_order` passes and the golden's own step-6 list is
//!
//! ```text
//!   ore_granite_upper, ore_granite_lower, ore_diorite_upper,
//!   ore_diorite_lower, ore_andesite_upper, ore_andesite_lower, ore_tuff
//! ```
//!
//! But a trace of a single cell says this port writes andesite there and then
//! overwrites it with granite:
//!
//! ```text
//!   ORE (1007,32,767) reached, minecraft:stone    -> minecraft:andesite
//!   ORE (1007,32,767) reached, minecraft:andesite -> minecraft:granite
//! ```
//!
//! Those cannot both be true. Either `build_features_per_step` returns a
//! different order from the golden it was checked against, or the pipeline
//! walks that order backwards. This prints what the pipeline is actually
//! handed, so the disagreement lands on one of the two.

#[test]
fn the_order_the_pipeline_is_handed() {
    // `feature_order_vectors.json` is committed, so an `Err` here is not the
    // gitignored-extract case at all — it is a golden this code cannot read,
    // and returning a pass for that is how a committed vector stops being one.
    let per_step = worldgen_assembly::features_per_step()
        .unwrap_or_else(|e| panic!("the feature-order golden did not load: {e}"));
    let step = &per_step[6];
    let names: Vec<&str> = step
        .features
        .iter()
        .map(String::as_str)
        .filter(|n| {
            ["granite", "diorite", "andesite", "tuff"]
                .iter()
                .any(|k| n.contains(k))
        })
        .collect();
    println!("step 6, the ore run, in the order the pipeline receives it:");
    for (i, n) in names.iter().enumerate() {
        println!("  {i}  {n}");
    }

    let position = |needle: &str| names.iter().position(|n| n.contains(needle));
    let (g, d, a) = (
        position("granite").expect("granite"),
        position("diorite").expect("diorite"),
        position("andesite").expect("andesite"),
    );
    assert!(
        g < d && d < a,
        "vanilla runs granite, then diorite, then andesite — and the last one \
         wins an overlap because stone_ore_replaceables contains all three. \
         Got granite at {g}, diorite at {d}, andesite at {a}"
    );
}
