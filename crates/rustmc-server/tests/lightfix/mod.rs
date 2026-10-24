//! The golden loader, shared by the two light comparisons.
//!
//! Lifted verbatim out of `tests/light_golden.rs` when a second comparison
//! against the same vectors arrived. It is a module rather than a copy
//! precisely because ORACLE.md 5b's rule about a second copy of the world
//! builders drifting from the first applies to the *reader* too: two decoders
//! of one golden that disagree would make one of the two comparisons measure
//! its own parser.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use rustmc_server::light_engine::{LightProps, LitChunk};

/// Load `data/vanilla/block_props.json` into the four columns the engine
/// needs, plus the state-string index the golden's palettes are written in.
/// Cargo runs an integration test with the *package* as its working
/// directory, not the workspace root, so every repo-relative path in a test
/// goes through here. Copied from `tests/entity_move.rs` rather than invented.
pub fn repo(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

pub fn load_props() -> (LightProps, BTreeMap<String, u16>) {
    let path = repo("data/vanilla/block_props.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — run ci/gen-gameplay-tables.sh in this tree",
            path.display()
        )
    });
    let v: serde_json::Value = serde_json::from_str(&raw).expect("block_props.json is not json");
    let table = v["table"]
        .as_array()
        .expect("block_props.json has no table");

    let n = table.len();
    let mut props = LightProps {
        emission: vec![0; n],
        dampening: vec![0; n],
        shape_occluding: vec![false; n],
        occlusion_faces: vec![0; n],
    };
    let mut ids = BTreeMap::new();
    for row in table {
        let id = usize::try_from(row["id"].as_u64().expect("row has no id")).expect("id fits");
        // A table without the light columns is a table generated before
        // 2026-08-18. Say so, rather than lighting a world in which every
        // block is transparent and emits nothing — which would look like a
        // port defect and is a stale table.
        let emission = row["lightEmission"].as_u64().unwrap_or_else(|| {
            panic!(
                "block_props.json has no lightEmission column — it predates the \
                 light port. Re-run ci/block-props-golden.sh."
            )
        });
        props.emission[id] = u8::try_from(emission).expect("emission 0..15");
        props.dampening[id] =
            u8::try_from(row["lightDampening"].as_u64().expect("lightDampening")).expect("0..15");
        props.shape_occluding[id] = row["shapeLightOcclusion"].as_bool().expect("flag");
        let faces = row["lightOcclusionFaces"]
            .as_str()
            .expect("lightOcclusionFaces");
        let mut bits = 0u8;
        for (d, c) in faces.chars().take(6).enumerate() {
            if c == '1' {
                bits |= 1 << d;
            }
        }
        props.occlusion_faces[id] = bits;
        ids.insert(
            row["state"].as_str().expect("state").to_owned(),
            u16::try_from(id).expect("state id fits u16"),
        );
    }
    (props, ids)
}

/// `BitSet.toLongArray()` as the golden writes it — decimal strings, because
/// a JSON number cannot hold the range.
pub fn mask(v: &serde_json::Value) -> Vec<i64> {
    v.as_array()
        .expect("mask is an array")
        .iter()
        .map(|w| {
            w.as_str()
                .expect("mask word is a string")
                .parse()
                .expect("i64")
        })
        .collect()
}

/// The golden writes each `DataLayer` as 4096 lowercase hex characters.
pub fn arrays(v: &serde_json::Value) -> Vec<Vec<u8>> {
    v.as_array()
        .expect("arrays")
        .iter()
        .map(|a| {
            let hex = a.as_str().expect("hex string");
            assert_eq!(
                hex.len(),
                4096,
                "a DataLayer is 2048 bytes, got {}",
                hex.len() / 2
            );
            hex.as_bytes()
                .chunks(2)
                .map(|p| {
                    u8::from_str_radix(std::str::from_utf8(p).expect("ascii"), 16)
                        .expect("hex byte")
                })
                .collect()
        })
        .collect()
}

/// Rebuild one chunk from the golden's palette + run-length world dump.
pub fn chunk_from(world: &serde_json::Value, ids: &BTreeMap<String, u16>, air: u16) -> LitChunk {
    let mut out = LitChunk::default();
    for sec in world.as_array().expect("world is an array") {
        let section_y = i32::try_from(sec["section_y"].as_i64().expect("section_y")).expect("i32");
        let palette: Vec<u16> = sec["palette"]
            .as_array()
            .expect("palette")
            .iter()
            .map(|s| {
                let name = s.as_str().expect("palette entry");
                *ids.get(name).unwrap_or_else(|| {
                    panic!("block_props.json has no state {name}, which the golden's world uses")
                })
            })
            .collect();
        let mut blocks = Vec::with_capacity(4096);
        for run in sec["runs"].as_array().expect("runs") {
            let r = run.as_array().expect("run pair");
            let idx = usize::try_from(r[0].as_u64().expect("run index")).expect("usize");
            let len = usize::try_from(r[1].as_u64().expect("run length")).expect("usize");
            for _ in 0..len {
                blocks.push(palette[idx]);
            }
        }
        assert_eq!(
            blocks.len(),
            4096,
            "section {section_y} run-length decodes to {} blocks, not 4096",
            blocks.len()
        );
        let _ = air;
        out.sections.insert(section_y, blocks);
    }
    out
}
