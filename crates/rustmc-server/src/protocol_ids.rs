//! The numeric ids of built-in registries, for the packets that carry them.
//!
//! # Why this is a table and not a constant
//!
//! Four registries reach the wire as bare varints and none is ever sent to
//! a client, so none is in `data/vanilla/registries.bin`:
//!
//! * `minecraft:menu` — `ClientboundOpenScreenPacket` writes its `MenuType`
//!   through `ByteBufCodecs.registry(Registries.MENU)`;
//! * `minecraft:block` — `ClientboundBlockEventPacket` writes the **block**,
//!   through `ByteBufCodecs.registry(Registries.BLOCK)`;
//! * `minecraft:block_entity_type` — `ClientboundBlockEntityDataPacket`
//!   writes its type through
//!   `ByteBufCodecs.registry(Registries.BLOCK_ENTITY_TYPE)`;
//! * `minecraft:sound_event` — `ClientboundSoundPacket` writes the event as a
//!   `Holder`, a varint of `registry id + 1` with 0 reserved for an inline
//!   event.
//!
//! That is the same situation `crate::entities`' kind ids were in, and it has
//! the same answer: read them out of the generator's own report, via
//! `ci/dump-protocol-ids.sh`. See that script's header for the cross-check
//! that the block half was verified against before it was trusted.
//!
//! # The trap this module exists to keep in one place
//!
//! **A block's registry id is not its state id and the two are both small
//! integers.** Everything else this server puts on the wire — `block_update`,
//! `section_blocks_update`, a chunk's palette — carries the *global state* id
//! out of `block_props.json`, where `minecraft:chest` occupies a run of
//! five-figure numbers. `block_event` carries the *block* id, which for a
//! chest is 201. Passing one where the other belongs produces a packet that
//! decodes cleanly and names a different block, and the client silently drops
//! it: there is no error to see, only a chest whose lid never moves.
//!
//! **The block-entity type is a third such number over the same object and it
//! is the smallest of the three**, which is what makes it the easiest to get
//! away with: `minecraft:sign` is block-entity type 7, `minecraft:oak_sign`
//! is block 84, and `minecraft:oak_sign[rotation=0,waterlogged=false]` is a
//! five-figure state id. Every one of them decodes.
//!
//! Sound events are a fourth space over names that only *look* related to the
//! others — `minecraft:block.chest.open` is sound 349 and `minecraft:chest` is
//! block 201 — and the packet carries `id + 1` rather than `id`, which is one
//! more way for a number that decodes to name the wrong thing.

use std::collections::HashMap;
use std::io;
use std::path::Path;

/// `minecraft:menu`, `minecraft:block`, `minecraft:block_entity_type` and
/// `minecraft:sound_event`, by name.
#[derive(Debug, Default)]
pub struct ProtocolIds {
    menus: HashMap<String, i32>,
    blocks: HashMap<String, i32>,
    block_entity_types: HashMap<String, i32>,
    sounds: HashMap<String, i32>,
    attributes: HashMap<String, i32>,
}

impl ProtocolIds {
    /// Load the extract.
    ///
    /// # Errors
    /// If the file is unreadable, is not JSON, or is not the shape
    /// `ci/dump-protocol-ids.sh` writes.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        let section = |key: &str| -> io::Result<HashMap<String, i32>> {
            let map = doc
                .get(key)
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| io::Error::other(format!("protocol_ids.json has no `{key}` map")))?;
            Ok(map
                .iter()
                .filter_map(|(k, v)| {
                    let id = v.as_i64()?;
                    i32::try_from(id).ok().map(|id| (k.clone(), id))
                })
                .collect())
        };
        // **A missing section is an error and not an empty map**, which is
        // what makes a stale `protocol_ids.json` loud. The merge queue lost an
        // afternoon on 2026-08-06 to the other shape of this: a table that was
        // present, well-formed and missing the column a branch had just added,
        // where the only thing that caught it was a loader that refused rather
        // than defaulted. `section` already returns `Err` for an absent key;
        // this line is where that matters for the newest of the four.
        Ok(Self {
            menus: section("menus")?,
            blocks: section("blocks")?,
            block_entity_types: section("block_entity_types")?,
            sounds: section("sounds")?,
            attributes: section("attributes")?,
        })
    }

    /// The process-wide table, loaded once.
    ///
    /// # Errors
    /// The load error, as a string, so a caller can report it rather than
    /// resolve every id to `None` and look like a server with no containers.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<ProtocolIds, String>> = std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// A menu type's `Registries.MENU` id.
    #[must_use]
    pub fn menu(&self, name: &str) -> Option<i32> {
        self.menus.get(name).copied()
    }

    /// A block's `Registries.BLOCK` id — **not** a block *state* id.
    #[must_use]
    pub fn block(&self, name: &str) -> Option<i32> {
        self.blocks.get(name).copied()
    }

    /// A block entity type's `Registries.BLOCK_ENTITY_TYPE` id — **not** the
    /// id of the block it hangs off, and not a state id either.
    #[must_use]
    pub fn block_entity_type(&self, name: &str) -> Option<i32> {
        self.block_entity_types.get(name).copied()
    }

    /// A sound event's `Registries.SOUND_EVENT` id — **the registry id, not
    /// the number on the wire.** `ClientboundSoundPacket` writes the event as
    /// a `Holder`, so the varint it carries is this **plus one**, with 0
    /// reserved for an inline event. `crate::sounds::sound_body` does the
    /// `+ 1`; this returns the registry's own number so that it can be
    /// compared against vanilla's report without an off-by-one in the middle.
    #[must_use]
    pub fn sound(&self, name: &str) -> Option<i32> {
        self.sounds.get(name).copied()
    }

    /// An attribute's `Registries.ATTRIBUTE` id — what
    /// `ClientboundUpdateAttributesPacket` writes for each snapshot, as a
    /// plain varint (`holderRegistry`, not the `+ 1` form `sound` uses).
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<i32> {
        self.attributes.get(name).copied()
    }
}

/// The table at its canonical path, or `None` with a line on stderr.
///
/// The line is not decoration. When the entity kind ids were missing the
/// tracker was fully written, sent nothing, and looked exactly like a server
/// with no entity tracking at all; the failure only became findable when the
/// resolution printed instead of defaulting.
///
/// # Path resolution, and why there are two of them
///
/// Tried first **relative to the process's working directory** —
/// `data/vanilla/protocol_ids.json`, unchanged from before this function grew
/// a fallback. That is what production wants: the server is booted from the
/// repo root and this must keep resolving exactly as it does today, with no
/// dependence on where the binary happened to be built.
///
/// `cargo test` does not run with that cwd. A package's test binaries run
/// with `CARGO_MANIFEST_DIR` (`crates/rustmc-server`) as the working
/// directory, not the repo root, so the lookup above failed on **every**
/// `cargo test` invocation regardless of what was on disk, and this table
/// read as absent whether the real one was missing, stale, or correct.
/// `World::chunk`'s block-entity list and
/// `broadcast::broadcast_block_entity_if_needed` both depend on this table
/// and could not be exercised by any unit test as a result: right and wrong
/// produced the identical empty answer. The second attempt below —
/// compile-time-anchored via `env!("CARGO_MANIFEST_DIR")`, tried **only**
/// when the first one finds no file — is what makes them reachable.
///
/// # The risk this carries, checked rather than assumed
///
/// `env!("CARGO_MANIFEST_DIR")` is the build machine's own path, embedded in
/// the binary at compile time — the risk named for this fix is that a
/// shipped binary would start depending on the source tree's layout at
/// runtime. Checked by construction rather than by feel: the fallback is
/// reached only when [`Path::exists`] on the cwd-relative candidate answers
/// `false`, so it cannot change production's answer on any checkout where
/// the working-directory lookup already succeeds — which is every real boot,
/// today exactly as before. What remains is narrower than "the shipped
/// binary depends on the source tree": a release binary launched from the
/// **wrong** working directory, on the **same machine it was built on**,
/// would now silently read that build tree's table instead of refusing with
/// the line below — a regression only in that specific misconfiguration, and
/// one this project's own history (`AGENT-BRIEF.md`'s `data/vanilla`
/// symlink incident) says is worth naming rather than waving off. On any
/// other machine, or once the build tree is gone, the fallback path is
/// simply absent and this refuses exactly as it does today.
#[must_use]
pub fn shared() -> Option<&'static ProtocolIds> {
    const REL: &str = "data/vanilla/protocol_ids.json";
    let cwd_relative = Path::new(REL);
    let path: std::path::PathBuf = if cwd_relative.exists() {
        cwd_relative.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(REL)
    };
    match ProtocolIds::shared(&path) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("protocol ids unavailable ({e}) — no container or block event can be sent");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Four id spaces over overlapping names, and the fixture makes them
    /// collide on purpose.** `minecraft:chest` is a block *and* a block
    /// entity type and the two numbers differ; a lookup that fell through
    /// from one map to the other would answer 201 where 1 belongs and the
    /// packet would still decode. The distinct values here are what makes
    /// that failure visible rather than plausible.
    #[test]
    fn the_four_registries_load_and_none_falls_through_to_another() {
        let dir = std::env::temp_dir().join(format!("rustmc-protocol-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let path = dir.join("protocol_ids.json");
        std::fs::write(
            &path,
            r#"{"menus":{"minecraft:crafting":12},
                "blocks":{"minecraft:chest":201,"minecraft:oak_sign":84},
                "block_entity_types":{"minecraft:chest":1,"minecraft:sign":7},
                "sounds":{"minecraft:block.chest.open":349},
                "attributes":{"minecraft:movement_speed":26}}"#,
        )
        .expect("write");
        let t = ProtocolIds::load(&path).expect("load");
        assert_eq!(t.menu("minecraft:crafting"), Some(12));
        assert_eq!(t.block("minecraft:chest"), Some(201));
        assert_eq!(t.block_entity_type("minecraft:sign"), Some(7));
        assert_eq!(t.sound("minecraft:block.chest.open"), Some(349));
        // The maps are separate namespaces and a lookup must not fall
        // through from one to the other: `menu("minecraft:chest")` is not 201.
        assert_eq!(t.menu("minecraft:chest"), None);
        assert_eq!(t.block("minecraft:crafting"), None);
        assert_eq!(t.block_entity_type("minecraft:oak_sign"), None);
        // A sound name is in a fourth space and shares no key with the rest.
        assert_eq!(t.sound("minecraft:chest"), None);
        // The one name in two of the maps answers each map's own number.
        assert_eq!(t.block("minecraft:chest"), Some(201));
        assert_eq!(t.block_entity_type("minecraft:chest"), Some(1));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A table written before `block_entity_types` existed is refused rather
    /// than loaded with an empty map. An empty map would resolve every sign
    /// to no type id, send no `block_entity_data` at all, and look exactly
    /// like a server with no sign block entities — the failure mode
    /// `crate::entities`' kind ids already paid for once.
    #[test]
    fn a_table_missing_a_section_is_an_error_not_an_empty_map() {
        let dir = std::env::temp_dir().join(format!("rustmc-protocol-old-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let path = dir.join("protocol_ids.json");
        std::fs::write(&path, r#"{"menus":{},"blocks":{}}"#).expect("write");
        assert!(ProtocolIds::load(&path).is_err());
        // And a table written before `attributes` existed, which is the live
        // version of this: every other section is present and well-formed.
        std::fs::write(
            &path,
            r#"{"menus":{},"blocks":{},"block_entity_types":{},"sounds":{}}"#,
        )
        .expect("write");
        assert!(
            ProtocolIds::load(&path).is_err(),
            "a stale table is refused, not loaded with a silently empty attribute map"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
