//! Which properties the ruin templates' palettes actually carry.
//!
//! Bounds `BlockState.rotate` before it is written. A general per-block
//! dispatch is a lot of code; if these 48 templates use two or three property
//! kinds, the first implementation is a table and the rest waits for a
//! structure that needs it.
//!
//! Uses the loader that `golden_structure_template` already gates on all 48
//! files, rather than a hand-rolled parse — a second parser would be a second
//! thing to be wrong, which this week has enough of.

use rustmc_worldgen::structure::template::Template;
use std::collections::BTreeMap;

#[test]
fn what_the_ruin_palettes_carry() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(&dir, "templates extracted", "ci/vanilla-server.sh data") {
        return;
    }
    let mut props: BTreeMap<String, usize> = BTreeMap::new();
    let mut with_props: BTreeMap<String, usize> = BTreeMap::new();
    let mut states = 0usize;
    for entry in std::fs::read_dir(&dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("nbt") {
            continue;
        }
        let file = std::fs::File::open(&path).expect("open");
        let mut reader = flate2::read::GzDecoder::new(file);
        let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("nbt");
        let template = Template::from_nbt(&root).expect("template");
        for state in &template.palette {
            states += 1;
            let Some((name, rest)) = state.split_once('[') else {
                continue;
            };
            *with_props.entry(name.to_string()).or_default() += 1;
            for pair in rest.trim_end_matches(']').split(',') {
                if let Some((k, _)) = pair.split_once('=') {
                    *props.entry(k.to_string()).or_default() += 1;
                }
            }
        }
    }
    println!("{states} palette entries across the ruin templates");
    println!("properties present:");
    let mut rows: Vec<_> = props.iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in rows {
        println!("  {n:5}  {k}");
    }
    println!("blocks carrying properties:");
    let mut rows: Vec<_> = with_props.iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in rows.iter().take(12) {
        println!("  {n:5}  {k}");
    }
}
