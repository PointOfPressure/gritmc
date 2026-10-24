//! What data markers `warm_7` carries, and where.
//!
//! `TemplateStructurePiece.postProcess` scans the template for
//! `minecraft:structure_block` entries with metadata and hands each to
//! `handleDataMarker`. `OceanRuinPieces.OceanRuinPiece` understands two:
//!
//! ```java
//!   "chest"   -> setBlock(pos, CHEST.defaultBlockState()
//!                                   .setValue(WATERLOGGED, fluid.is(WATER)), 2)
//!                if (getBlockEntity(pos) instanceof ChestBlockEntity chest)
//!                    chest.setLootTable(OCEAN_RUIN_WARM, random.nextLong());
//!   "drowned" -> spawn a Drowned
//! ```
//!
//! Both matter for different reasons. The chest is a **block** the census
//! already saw missing from this port. The `nextLong` behind it is guarded by
//! `getBlockEntity(pos) instanceof ChestBlockEntity` — the same shape as the
//! two draws removed from `monster_room`, where a worldgen ProtoChunk has no
//! block entity and vanilla therefore spends nothing. If that holds here too,
//! placing this ruin costs the decoration stream nothing at all, which is what
//! makes wiring it in safe without first settling the structure-index question.
//!
//! `BlockIgnoreProcessor.STRUCTURE_AND_AIR` strips `structure_block` from the
//! *placed* blocks, so the marker never appears in the world — it only steers
//! `handleDataMarker`. A port that dropped it at the parse step would silently
//! lose the chest.

use rustmc_world::nbt::Nbt;

#[test]
fn what_markers_warm_7_carries() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("oracle-work/vanilla-26.2/generated-data/data/minecraft/structure/underwater_ruin");
    if !rustmc_data::fixture::available(
        &dir,
        "underwater_ruin templates",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }

    for name in ["warm_7", "warm_1", "big_warm_4"] {
        let path = dir.join(format!("{name}.nbt"));
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut reader = flate2::read::GzDecoder::new(file);
        let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).expect("gzipped NBT");

        let Nbt::Compound(map) = &root else {
            panic!("{name}: root is not a compound")
        };
        let Some(Nbt::List(blocks)) = map.get("blocks") else {
            panic!("{name}: no blocks list")
        };
        let Some(Nbt::List(palette)) = map.get("palette") else {
            panic!("{name}: no palette")
        };

        // Which palette indices are structure_block.
        let mut marker_states = Vec::new();
        for (i, entry) in palette.iter().enumerate() {
            if let Nbt::Compound(e) = entry {
                if let Some(Nbt::String(n)) = e.get("Name") {
                    if n == "minecraft:structure_block" {
                        marker_states.push(i as i32);
                    }
                }
            }
        }

        let mut found = Vec::new();
        for b in blocks {
            let Nbt::Compound(b) = b else { continue };
            let Some(Nbt::Int(state)) = b.get("state") else {
                continue;
            };
            if !marker_states.contains(state) {
                continue;
            }
            let pos = match b.get("pos") {
                Some(Nbt::List(p)) if p.len() == 3 => {
                    let g = |i: usize| match &p[i] {
                        Nbt::Int(v) => *v,
                        _ => -1,
                    };
                    (g(0), g(1), g(2))
                }
                _ => (-1, -1, -1),
            };
            // The marker's payload lives in the block's own nbt, under
            // `metadata` (older versions used `Metadata`).
            let mut meta = String::from("<none>");
            if let Some(Nbt::Compound(nbt)) = b.get("nbt") {
                for key in ["metadata", "Metadata"] {
                    if let Some(Nbt::String(v)) = nbt.get(key) {
                        meta = v.clone();
                    }
                }
                if meta == "<none>" {
                    meta = format!("{:?}", nbt.keys().collect::<Vec<_>>());
                }
            }
            found.push((pos, meta));
        }
        println!("{name}: {} marker(s) {found:?}", found.len());
    }
}
