//! Which ore feature wins where two stone-variant blobs overlap.
//!
//! The window at (64,47) shows 235 mismatches that are **variant against
//! variant** and, decisively, **zero** where one side is a variant and the
//! other is not. The set of stone-variant cells is identical between this port
//! and the server; only 235 of the labels differ.
//!
//! That rules out the reading this was filed under. Nothing is misplaced —
//! every blob covers exactly the cells vanilla's does. The question is only
//! which feature owns an overlapping cell.
//!
//! `stone_ore_replaceables` is `[stone, granite, diorite, andesite]`, so a
//! later ore genuinely overwrites an earlier variant and the **last** feature
//! wins. The datapack order at step 6 is granite_upper, granite_lower,
//! diorite_upper, diorite_lower, andesite_upper, andesite_lower, tuff — which
//! predicts andesite beating both, and that is what the server does:
//!
//! ```text
//!   91  vanilla andesite   ours granite
//!   77  vanilla andesite   ours diorite
//!   62  vanilla granite    ours diorite
//! ```
//!
//! This checks the one thing that would explain it from the port's side: that
//! the tag is actually loaded with all four values at runtime. A tag holding
//! only `stone` turns "last wins" into "first wins" and inverts exactly these
//! pairs.

#[test]
fn the_replaceable_tag_carries_every_stone_variant() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/tags/block");
    if !rustmc_data::fixture::available(&dir, "datagen extract", "ci/vanilla-server.sh data") {
        return;
    }
    let tags = rustmc_worldgen::tags::BlockTags::load(&dir).expect("load block tags");

    let set = tags
        .get("minecraft:stone_ore_replaceables")
        .expect("the ore replaceable tag must exist");
    let mut names: Vec<&str> = set.iter().map(String::as_str).collect();
    names.sort_unstable();
    println!("stone_ore_replaceables = {names:?}");

    for want in [
        "minecraft:stone",
        "minecraft:granite",
        "minecraft:diorite",
        "minecraft:andesite",
    ] {
        assert!(
            set.contains(want),
            "{want} missing from stone_ore_replaceables; without it a later ore \
             cannot overwrite an earlier variant, which turns vanilla's \
             last-wins overlap into first-wins and mislabels every overlapping \
             cell while leaving the cell *set* exactly right"
        );
    }
}
