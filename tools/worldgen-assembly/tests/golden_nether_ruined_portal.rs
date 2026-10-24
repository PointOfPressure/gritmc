//! `ruined_portal_nether`, gated against vanilla's own saved starts.
//!
//! The six r=16 nether references (`gen-wide-20260722-r16-nether-{a..f}`)
//! save two, identically: chunk (48, 64), `portal_3` at (768, 63, 1024), and
//! chunk (92, 81), `portal_8` at (1472, 28, 1296), the second in the region
//! files' border ring rather than in the measured square. They are the only
//! `in_nether` portals in any reference on the box, so they are the only
//! measurement of that placement's branch of
//! `RuinedPortalStructure.findSuitableY` — the random height
//! (`randomBetweenInclusive(32, 100)` with an air pocket, else a coin and one
//! of two ranges) walked down until three of the box's four corners are
//! opaque in the base column. The y is the sharp field: it is drawn and then
//! searched, and a wrong draw or a wrong opacity test moves it. (28 is the
//! interesting one: the `27..=29` range only a missing air pocket reaches.)
//!
//! Until 2026-10-02 the nether never placed this structure at all, and the
//! r=16 window charged its blocks to nothing.

use std::path::Path;

use rustmc_worldgen::pipeline::Area;

type Saved = ((i32, i32), Option<String>, Option<[i32; 3]>, [i32; 6]);

fn saved_portals(dir: &Path) -> Vec<Saved> {
    assert!(
        rustmc_data::fixture::available(dir, "r=16 nether reference", "ci/gen-vanilla-region.sh"),
        "missing {}",
        dir.display()
    );
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("reference directory") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mca") {
            continue;
        }
        for column in rustmc_world::anvil::read_region(&path).expect("region parses") {
            for start in &column.structure_starts {
                if start.id == "minecraft:ruined_portal_nether" {
                    let p = &start.pieces[0];
                    found.push((
                        (start.chunk_x, start.chunk_z),
                        p.template.clone(),
                        p.template_pos,
                        p.bounding_box,
                    ));
                }
            }
        }
    }
    found.sort();
    assert!(
        !found.is_empty(),
        "{} saved no nether portal",
        dir.display()
    );
    found
}

#[test]
fn the_nether_portals_match_vanillas_saved_starts() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
    let saved: Vec<Vec<Saved>> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|t| saved_portals(&root.join(format!("gen-wide-20260722-r16-nether-{t}"))))
        .collect();
    for other in &saved[1..] {
        assert_eq!(
            &saved[0], other,
            "the six references disagree about the portals"
        );
    }
    assert_eq!(saved[0].len(), 3, "the references' portal count moved");
    for (chunk, template, tp, bb) in saved[0].clone() {
        let ours = worldgen_assembly::generate::nether_ruined_portal_report(
            20_260_722,
            Area::square(chunk.0, chunk.1, 0),
        )
        .expect("lays out");
        let mine = ours
            .iter()
            .find(|p| p.start_chunk == chunk)
            .unwrap_or_else(|| panic!("no nether portal laid out at {chunk:?}"));
        assert_eq!(mine.structure, "ruined_portal_nether");
        let ob = mine.piece.bounding_box(&mine.template);
        let tpos = mine.piece.template_position;
        let got: Saved = (
            chunk,
            Some(format!("minecraft:{}", mine.template_name)),
            Some([tpos.0, tpos.1, tpos.2]),
            [ob.min_x, ob.min_y, ob.min_z, ob.max_x, ob.max_y, ob.max_z],
        );
        assert_eq!(
            (chunk, template, tp, bb),
            got,
            "nether portal at {chunk:?}: vanilla (template, TP, BB) against ours"
        );
        println!("NETHER PORTAL {got:?} equal to vanilla's");
    }
}
