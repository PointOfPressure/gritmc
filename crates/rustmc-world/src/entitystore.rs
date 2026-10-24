//! `dimensions/<dim>/entities/r.X.Z.mca` — entities, which have not lived in
//! the block chunk since 1.17.
//!
//! # The thing that is easy to get wrong
//!
//! `SerializableChunkData`'s field list still holds an `entities` key, and it
//! is a **legacy** one: `ChunkStorage.upgradeChunkTag` moves it out on load
//! and nothing writes it back. A save path that puts entities there produces
//! a file vanilla loads and shows an empty world in, because
//! `EntityStorage.loadEntities` reads the entity region and never looks at
//! the block chunk. The two are separate `RegionFileStorage`s over separate
//! directories.
//!
//! # The file
//!
//! Byte-for-byte the same container as a block region — 1,024 offsets, 1,024
//! timestamps, 4 KiB sectors, `[u32 len][u8 scheme][payload]` — which is why
//! this module reuses [`crate::anvil`]'s framing rather than restating it. The
//! compound inside carries three keys and `EntityStorage`'s own constant pool
//! names two of them:
//!
//! ```text
//! Position     IntArray[2]      the chunk's x and z
//! Entities     List<Compound>   one per entity, each carrying its own `id`
//! DataVersion  Int              added by the storage layer, not by EntityStorage
//! ```
//!
//! Read off a real vanilla `entities/r.0.0.mca` from
//! `oracle-work/recipe-dump`, which `ci/gen-gameplay-tables.sh` leaves behind
//! and which the golden below compares against.

use crate::anvil::{
    decode_region_record, encode_region_record, lay_out_region, read_region_records, region_index,
};
use crate::nbt::Nbt;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

/// One chunk's worth of entities.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub data_version: i32,
    /// Each entity's full compound, verbatim. Raw NBT for the same reason
    /// `ChunkColumn::block_entities` is: this crate is the save format, and
    /// what an entity's fields mean is the server's business.
    pub entities: Vec<Nbt>,
}

impl EntityChunk {
    #[must_use]
    pub fn to_nbt(&self) -> Nbt {
        let mut m = BTreeMap::new();
        m.insert(
            "Position".into(),
            Nbt::IntArray(vec![self.chunk_x, self.chunk_z]),
        );
        m.insert("DataVersion".into(), Nbt::Int(self.data_version));
        m.insert("Entities".into(), Nbt::List(self.entities.clone()));
        Nbt::Compound(m)
    }

    /// # Errors
    /// If `Position` is missing or is not a two-element int array.
    pub fn from_nbt(root: &Nbt) -> io::Result<Self> {
        let Some(Nbt::IntArray(pos)) = root.get("Position") else {
            return Err(io::Error::other("entity chunk has no int-array `Position`"));
        };
        if pos.len() != 2 {
            return Err(io::Error::other(format!(
                "entity chunk `Position` has {} entries, not 2",
                pos.len()
            )));
        }
        Ok(Self {
            chunk_x: pos[0],
            chunk_z: pos[1],
            data_version: match root.get("DataVersion") {
                Some(Nbt::Int(v)) => *v,
                _ => 0,
            },
            // Absent is legal and means none — a chunk that has been visited
            // and holds nothing still gets a record written for it.
            entities: root
                .get("Entities")
                .and_then(Nbt::as_list)
                .map(<[Nbt]>::to_vec)
                .unwrap_or_default(),
        })
    }
}

/// Read every entity chunk in one `entities/r.X.Z.mca`.
///
/// # Errors
/// If the file exists and is truncated or malformed. A missing file is an
/// empty vector: a region nothing has walked into has no file.
pub fn read_region(path: &Path) -> io::Result<Vec<EntityChunk>> {
    let mut out = Vec::new();
    for record in read_region_records(path)?.values() {
        out.push(EntityChunk::from_nbt(&decode_region_record(record)?)?);
    }
    Ok(out)
}

/// Write entity chunks into `dir`, preserving every chunk already on disk that
/// is not among them.
///
/// Merging for the same reason [`crate::anvil::write_region_merging`] is: a
/// region holds 1,024 chunks and a save usually touches one, and a
/// non-merging writer would delete the other 1,023 — here that is every mob
/// in a 512-block square.
///
/// **An empty entity list is written, not skipped.** A chunk whose last mob
/// has just died has to record that it now holds none; leaving its old record
/// in place would resurrect them on the next load.
///
/// # Errors
/// If the directory cannot be created or a region file cannot be written.
pub fn write_region_merging(dir: &Path, chunks: &[EntityChunk]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut by_region: BTreeMap<(i32, i32), Vec<&EntityChunk>> = BTreeMap::new();
    for chunk in chunks {
        by_region
            .entry((chunk.chunk_x >> 5, chunk.chunk_z >> 5))
            .or_default()
            .push(chunk);
    }
    for ((rx, rz), group) in by_region {
        let path = dir.join(format!("r.{rx}.{rz}.mca"));
        let mut records = read_region_records(&path)?;
        for chunk in group {
            records.insert(
                region_index(chunk.chunk_x, chunk.chunk_z),
                encode_region_record(&chunk.to_nbt())?,
            );
        }
        std::fs::write(&path, lay_out_region(rx, rz, &records)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vanilla_entity_regions() -> Vec<std::path::PathBuf> {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(root) else {
            return out;
        };
        for entry in entries.flatten() {
            let dir = entry
                .path()
                .join("world/dimensions/minecraft/overworld/entities");
            let Ok(files) = std::fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                // A zero-length `.mca` is what vanilla leaves for a region it
                // opened and never wrote to; reading one is not a test of
                // anything.
                if path.extension().is_some_and(|e| e == "mca")
                    && std::fs::metadata(&path).is_ok_and(|m| m.len() > 8192)
                {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    /// **A real vanilla entity region, read and rewritten, comes back with the
    /// same entities.**
    ///
    /// Read-then-rewrite through our own pair is the weaker half of this and
    /// it is here for what it *can* see: a `Position` written as a two-int
    /// array in the wrong order, an `Entities` list dropped when it is empty,
    /// a chunk lost from the offset table. What it cannot see is a byte
    /// vanilla would reject, which is what `ci/t2-vanilla-loads-ours.sh` is
    /// for on the block-chunk side.
    ///
    /// The stronger half is the second assertion: the re-encoded **payload of
    /// each record** is compared to vanilla's own, byte for byte, which is a
    /// claim about the compound rather than about our reader.
    #[test]
    fn a_real_vanilla_entity_region_round_trips() {
        let probe = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/recipe-dump/world/dimensions/minecraft/overworld/entities"
        ));
        if !rustmc_data::fixture::available(
            probe,
            "a vanilla entity region",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let regions = vanilla_entity_regions();
        assert!(!regions.is_empty(), "no non-empty entity regions found");
        let mut entities_seen = 0;
        for path in &regions {
            let original = read_region(path).expect("read");
            for record in read_region_records(path).expect("records").values() {
                let root = decode_region_record(record).expect("decode");
                let chunk = EntityChunk::from_nbt(&root).expect("parse");
                entities_seen += chunk.entities.len();
                let want = crate::nbt::encode_named_root("", &root).expect("re-encode vanilla's");
                let ours = crate::nbt::encode_named_root("", &chunk.to_nbt()).expect("ours");
                assert_eq!(ours, want, "{}", path.display());
            }

            let dir = std::env::temp_dir().join(format!(
                "rustmc-entities-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            write_region_merging(&dir, &original).expect("write");
            let name = path.file_name().expect("name");
            let mut back = read_region(&dir.join(name)).expect("read back");
            let _ = std::fs::remove_dir_all(&dir);
            let mut original = original;
            original.sort_by_key(|c| (c.chunk_x, c.chunk_z));
            back.sort_by_key(|c| (c.chunk_x, c.chunk_z));
            assert_eq!(back, original, "{}", path.display());
        }
        // Without this the test passes over regions holding nothing but empty
        // `Entities` lists, which is the shape a save path that never wrote an
        // entity also produces.
        assert!(
            entities_seen > 0,
            "the fixture regions hold no entities at all; \
             nothing about the entity list was exercised"
        );
    }

    /// Merging leaves a neighbouring chunk alone. The failure this catches is
    /// the one `write_region` (non-merging) has and that
    /// `ci/t2-vanilla-loads-ours.sh` found on the block side: a save of one
    /// chunk deleting the other 1,023.
    #[test]
    fn writing_one_chunk_leaves_its_neighbour_alone() {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-entities-merge-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let pig = |x: f64| {
            let mut m = BTreeMap::new();
            m.insert("id".to_string(), Nbt::String("minecraft:pig".into()));
            m.insert(
                "Pos".to_string(),
                Nbt::List(vec![Nbt::Double(x), Nbt::Double(64.0), Nbt::Double(0.0)]),
            );
            Nbt::Compound(m)
        };
        let first = EntityChunk {
            chunk_x: 0,
            chunk_z: 0,
            data_version: 4903,
            entities: vec![pig(0.5)],
        };
        let second = EntityChunk {
            chunk_x: 1,
            chunk_z: 0,
            data_version: 4903,
            entities: vec![pig(16.5)],
        };
        write_region_merging(&dir, std::slice::from_ref(&first)).expect("first");
        write_region_merging(&dir, std::slice::from_ref(&second)).expect("second");
        let mut back = read_region(&dir.join("r.0.0.mca")).expect("read");
        back.sort_by_key(|c| (c.chunk_x, c.chunk_z));
        assert_eq!(back, vec![first, second]);

        // And an emptied chunk stays emptied rather than reverting.
        let emptied = EntityChunk {
            chunk_x: 0,
            chunk_z: 0,
            data_version: 4903,
            entities: Vec::new(),
        };
        write_region_merging(&dir, std::slice::from_ref(&emptied)).expect("third");
        let back = read_region(&dir.join("r.0.0.mca")).expect("read");
        assert_eq!(
            back.iter().find(|c| c.chunk_x == 0).expect("still there"),
            &emptied,
            "an empty entity list must overwrite, not be skipped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A chunk outside a region's 32x32 goes to the right file. `>> 5` on a
    /// negative coordinate is the arithmetic shift, so chunk -1 is in region
    /// -1 and not region 0.
    #[test]
    fn negative_chunks_land_in_the_negative_region() {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-entities-neg-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let chunk = EntityChunk {
            chunk_x: -1,
            chunk_z: -33,
            data_version: 4903,
            entities: Vec::new(),
        };
        write_region_merging(&dir, std::slice::from_ref(&chunk)).expect("write");
        assert!(dir.join("r.-1.-2.mca").is_file(), "region (-1, -2)");
        assert_eq!(
            read_region(&dir.join("r.-1.-2.mca")).expect("read"),
            vec![chunk]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
