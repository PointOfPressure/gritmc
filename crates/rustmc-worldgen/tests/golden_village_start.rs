//! The village oracle: the piece list vanilla itself saved.
//!
//! A village has no draw-level harness. `DecorateGolden` runs *features*, and
//! `JigsawPlacement` is not a feature, so the only thing that can say whether
//! this port's assembler made the same decisions as vanilla's is the piece
//! list vanilla wrote into its own chunk NBT —
//! `PoolElementStructurePiece.addAdditionalSaveData`: `pool_element`,
//! `rotation`, `PosX/PosY/PosZ`, `ground_level_delta` and `junctions`.
//!
//! That list pins each decision **separately**, which is why it is worth more
//! than a block diff:
//!
//!   * which element the pool's shuffle drew,
//!   * which of the four shuffled rotations survived `canAttach` and the free
//!     space test,
//!   * the y the piece was projected or carried to,
//!   * and how many junctions the piece ended up with, which is the shape of
//!     the walk itself.
//!
//! A block diff answers none of those on its own: one wrong element early
//! moves everything after it and the diff reports a single large number.
//!
//! # The vector is committed, and why that is safe here
//!
//! `golden/village_31415926.json` was produced by
//! `worldgen-dump jigsaw-starts oracle-work/van-village-31415926
//! village_savanna --json` from the world
//! `ci/gen-vanilla-region.sh 31415926 7 ... 8 17` generated. Committing it
//! means the assembler can be gated with no vanilla world present at all.
//!
//! Two independently generated vanilla worlds of one seed disagree by
//! hundreds of *blocks* — measured, 232 on the 20260722 village window — so a
//! committed block reference would be quoting noise. A piece list is not a
//! sampled world: it is the output of a deterministic decision sequence off a
//! seeded `WorldgenRandom`, and the second half of this test proves the point
//! by re-reading the world and requiring it to agree exactly.

use std::path::Path;

use serde_json::Value;

const VECTOR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/village_31415926.json");

const WORLD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/van-village-31415926"
);

const TEMPLATES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/structure"
);

fn vector() -> Value {
    serde_json::from_str(&std::fs::read_to_string(VECTOR).expect("the committed village vector"))
        .expect("the vector is JSON")
}

/// The shape of the reference, as a ratchet.
///
/// Not decoration: every number here is one the assembler has to reproduce,
/// and a vector that quietly changed shape — a re-dump against a different
/// world, a parser that started dropping a field — would otherwise turn the
/// assembler's gate green against something else.
#[test]
fn the_reference_village_is_the_one_the_assembler_is_built_against() {
    let doc = vector();
    assert_eq!(doc["structure"], "minecraft:village_savanna");
    assert_eq!(doc["chunk"][0], 8);
    assert_eq!(doc["chunk"][1], 17);
    let pieces = doc["pieces"].as_array().expect("pieces");
    assert_eq!(pieces.len(), 181, "the reference village is 181 pieces");

    // The start piece is the one `StructureTemplatePool.getRandomTemplate`
    // drew from `village/savanna/town_centers` on the very first draw of the
    // structure's random, before anything else can have moved the stream. If
    // a port gets this one wrong, nothing after it can be right, so it is
    // called out separately from the rest.
    let first = &pieces[0];
    assert_eq!(
        first["element"],
        "minecraft:village/savanna/zombie/town_centers/savanna_meeting_point_4"
    );
    assert_eq!(first["rotation"], "CLOCKWISE_180");
    assert_eq!(first["pos"], serde_json::json!([128, 83, 272]));
    assert_eq!(first["projection"], "rigid");

    // A `rigid` piece takes its connection's y; a `terrain_matching` one is
    // re-projected onto the heightmap. Getting the split backwards is a
    // village on stilts, so the census is asserted rather than assumed.
    let rigid = pieces.iter().filter(|p| p["projection"] == "rigid").count();
    let matching = pieces
        .iter()
        .filter(|p| p["projection"] == "terrain_matching")
        .count();
    assert_eq!(
        rigid + matching,
        pieces.len(),
        "every piece has a projection"
    );
    assert_eq!((rigid, matching), (27, 154));

    // Six pieces of this village carry no template: they are
    // `feature_pool_element`s, which place a configured feature rather than a
    // structure template. The assembler has to know they exist — a walk that
    // assumed every element is a template would drop them and every junction
    // they own, and the six here are the village's trees and its cats.
    let featureless = pieces.iter().filter(|p| p["element"].is_null()).count();
    let features = pieces
        .iter()
        .filter(|p| p["type"] == "minecraft:feature_pool_element")
        .count();
    assert_eq!((featureless, features), (6, 6));

    // Every remaining element must name a template the extract carries. This
    // is the cross-file claim: a pool that points at a missing `.nbt` would
    // otherwise only fail once the assembler reached it.
    let dir = Path::new(TEMPLATES);
    if !rustmc_data::fixture::available(dir, "structure templates", "ci/vanilla-server.sh data") {
        return;
    }
    for piece in pieces {
        let Some(name) = piece["element"].as_str() else {
            continue;
        };
        let rel = name
            .strip_prefix("minecraft:")
            .expect("a namespaced element");
        let path = dir.join(format!("{rel}.nbt"));
        assert!(
            path.exists(),
            "{name} names no template at {}",
            path.display()
        );
    }
}

/// The committed vector is still what the world says.
///
/// Runs only where the reference world exists. It is the guard against the
/// vector drifting away from the world it claims to describe — the failure
/// mode a committed oracle has and a live one does not.
#[test]
fn the_vector_still_matches_the_world_it_came_from() {
    let dir = Path::new(WORLD);
    // **NOT converted to `fixture::available`, and that is the finding.**
    // `oracle-work/van-village-31415926` does not exist in any tree on this
    // box — not in the shared `~/rustmc/oracle-work`, not in any gate
    // directory — so this test has never once compared the committed vector
    // against the live world it claims to re-derive. Converting it would make
    // T1 red for every lane over a fixture nobody can produce without a
    // vanilla boot, so the hole is *declared* instead, in
    // `ci/checks/fixture-skips.toml`, where it is counted and named rather
    // than silent. Convert this the day the world is generated.
    if !dir.exists() {
        println!("skipping: no vanilla village world");
        return;
    }
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("read the world")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .collect();
    entries.sort();
    let mut found = None;
    for path in entries {
        for chunk in rustmc_world::anvil::read_region(&path).expect("read a region") {
            for start in chunk.structure_starts {
                if start.id == "minecraft:village_savanna" {
                    found = Some(start);
                }
            }
        }
    }
    let start = found.expect("the savanna village is in this world");
    let doc = vector();
    let pieces = doc["pieces"].as_array().expect("pieces");
    assert_eq!(start.pieces.len(), pieces.len());
    for (i, (live, saved)) in start.pieces.iter().zip(pieces).enumerate() {
        assert_eq!(
            live.pool_element.as_deref(),
            saved["element"].as_str(),
            "piece {i} element"
        );
        assert_eq!(
            live.rotation.as_deref(),
            saved["rotation"].as_str(),
            "piece {i} rotation"
        );
        let pos = live.pos.expect("a jigsaw piece has a position");
        assert_eq!(
            serde_json::json!([pos[0], pos[1], pos[2]]),
            saved["pos"],
            "piece {i} position"
        );
        assert_eq!(
            live.junctions.len(),
            saved["junctions"].as_u64().expect("junctions") as usize,
            "piece {i} junction count"
        );
    }
}
