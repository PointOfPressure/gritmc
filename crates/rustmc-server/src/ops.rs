//! The op list — who holds which command permission level.
//!
//! `PlayerList` keeps a `ServerOpList` loaded from `ops.json` beside the
//! server jar, and a player's command level is that entry's `level`, or
//! `PermissionLevel.ALL` for anyone not in it.
//!
//! # The file, from the jar rather than from memory
//!
//! `ServerOpListEntry`'s serialiser writes `level` and `bypassesPlayerLimit`,
//! and its `NameAndId` base writes `uuid` and `name`. So an entry is
//!
//! ```json
//! { "uuid": "...", "name": "Steve", "level": 4, "bypassesPlayerLimit": false }
//! ```
//!
//! and the file is an array of them.
//!
//! # What is covered and what is not
//!
//! **Covered:** reading the file, matching by name, and the level that comes
//! out of it. That is the whole of what the command registry needs.
//!
//! **Not covered, and named so nobody reads this as an op system:**
//!
//! * **Matching by uuid.** Vanilla keys the list by `NameAndId`, so a renamed
//!   account keeps its op. This server matches on the **name only**, because
//!   it is offline-mode: a name *is* the identity here, the uuid is derived
//!   from it (`UUIDUtil.createOfflinePlayerUUID`), and the two can never
//!   disagree. On an online-mode server they can, and this would then be
//!   wrong.
//! * **`/op` and `/deop`.** The file is read, never written. Both commands
//!   would need `sendPlayerPermissionLevel` re-sent to the affected player,
//!   which [`crate::commands::send_commands`] already does — the missing part
//!   is the mutation, not the notification.
//! * **`op-permission-level`** from `server.properties`, which is the level
//!   `/op` *grants*. With no `/op` there is nothing for it to grant.
//! * **`bypassesPlayerLimit`**, which belongs to the login path.
//! * **Reloading.** Read once, on first use. Vanilla re-reads on `/reload`.
//!
//! # Why this exists at all rather than a constant zero
//!
//! Because a permission model nobody can be on the far side of is a
//! permission model that has never run. With no op list, every player is
//! level 0, `/seed` is unreachable, and the whole filtered-tree path in
//! [`crate::commandtree`] would be dead code in the live server while passing
//! its unit tests — the shape of defect this project keeps paying for. One
//! `ops.json` and the machinery is exercised for real.

use crate::commandtree::Level;
use std::collections::HashMap;
use std::path::Path;

/// The parsed list: lowercased name -> level.
#[derive(Debug, Default)]
pub struct Ops {
    levels: HashMap<String, Level>,
}

impl Ops {
    /// Parse an `ops.json`.
    ///
    /// A malformed entry is skipped rather than failing the file: an operator
    /// with one bad line should lose that line, not every op on the server.
    /// A file that is not an array at all *is* an error, because that is a
    /// wrong file rather than a wrong entry.
    ///
    /// # Errors
    /// If the text is not JSON, or is not a JSON array.
    pub fn parse(text: &str) -> Result<Self, String> {
        let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let arr = doc
            .as_array()
            .ok_or_else(|| "ops.json is not an array".to_string())?;
        let mut levels = HashMap::new();
        for entry in arr {
            let Some(name) = entry.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            // `level` absent is not "level 0": vanilla's entry always carries
            // one, and an entry without it is malformed. Skipping is safer
            // than defaulting either way — defaulting high would op a broken
            // line, defaulting low would silently de-op a real one.
            let Some(level) = entry.get("level").and_then(serde_json::Value::as_i64) else {
                continue;
            };
            let level = i32::try_from(level).unwrap_or(0);
            levels.insert(name.to_lowercase(), Level::from_ordinal(level));
        }
        Ok(Self { levels })
    }

    /// This name's level, or [`Level::All`].
    #[must_use]
    pub fn level_for(&self, name: &str) -> Level {
        self.levels
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(Level::All)
    }

    /// How many ops were loaded, for the boot line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }
}

/// The process-wide list, read once from `ops.json`.
///
/// A missing file is an empty list and **is not an error** — an unopped
/// server is the normal case. A file that exists and does not parse prints,
/// because the alternative is an operator whose ops silently do not work.
///
/// # Path resolution
///
/// Tried first relative to the process's cwd, unchanged from before this
/// grew a fallback; only when that file is absent does it fall back to a
/// `CARGO_MANIFEST_DIR`-anchored path, which is what makes this reachable
/// under `cargo test` (whose cwd is the package directory, not the repo
/// root — the same place `ops.json` sits beside a real deployed jar). Same
/// shape and the same carried risk as `crate::protocol_ids::shared()`:
/// `docs/PARITY-ROADMAP.md` §34.
pub fn shared() -> &'static Ops {
    static OPS: std::sync::OnceLock<Ops> = std::sync::OnceLock::new();
    OPS.get_or_init(|| {
        const REL: &str = "ops.json";
        let cwd_relative = Path::new(REL);
        let path: std::path::PathBuf = if cwd_relative.exists() {
            cwd_relative.to_path_buf()
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(REL)
        };
        match std::fs::read_to_string(&path) {
            Err(_) => Ops::default(),
            Ok(text) => match Ops::parse(&text) {
                Ok(o) => {
                    eprintln!("ops.json: {} operator(s)", o.len());
                    o
                }
                Err(e) => {
                    eprintln!("ops.json is present but unreadable ({e}) — nobody is an operator");
                    Ops::default()
                }
            },
        }
    })
}

/// This name's command permission level, from the shared list.
#[must_use]
pub fn level_for(name: &str) -> Level {
    shared().level_for(name)
}

/// `PlayerList.isOp(nameAndId)`: whether the op list has an entry for this
/// player at all, whatever its level.
///
/// What `CommandSourceStack.broadcastToAdmins` asks, and not the same
/// question as [`level_for`]: an entry at level 0 is still an op.
#[must_use]
pub fn is_op(name: &str) -> bool {
    shared().levels.contains_key(&name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
        {"uuid":"00000000-0000-0000-0000-000000000001","name":"Steve","level":4,
         "bypassesPlayerLimit":false},
        {"uuid":"00000000-0000-0000-0000-000000000002","name":"Alex","level":2,
         "bypassesPlayerLimit":false}
    ]"#;

    #[test]
    fn levels_come_from_the_file_and_absence_is_level_zero() {
        let ops = Ops::parse(SAMPLE).expect("parse");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops.level_for("Steve"), Level::Owners);
        assert_eq!(ops.level_for("Alex"), Level::Gamemasters);
        assert_eq!(ops.level_for("Nobody"), Level::All);
    }

    /// Vanilla's own list is case-insensitive on names, and an operator who
    /// typed `steve` into the file should not find their op silently absent.
    #[test]
    fn names_match_case_insensitively() {
        let ops = Ops::parse(SAMPLE).expect("parse");
        assert_eq!(ops.level_for("steve"), Level::Owners);
        assert_eq!(ops.level_for("STEVE"), Level::Owners);
    }

    /// One malformed entry costs that entry and nothing else.
    #[test]
    fn a_bad_entry_is_skipped_not_fatal() {
        let ops = Ops::parse(r#"[{"name":"Steve","level":4},{"name":"NoLevel"},{"level":3}]"#)
            .expect("parse");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops.level_for("Steve"), Level::Owners);
        assert_eq!(ops.level_for("NoLevel"), Level::All);
    }

    /// A file of the wrong shape is refused rather than read as empty, so an
    /// operator sees a message instead of a server that ignores them.
    #[test]
    fn a_non_array_file_is_an_error() {
        assert!(Ops::parse(r#"{"name":"Steve"}"#).is_err());
        assert!(Ops::parse("not json at all").is_err());
        assert!(Ops::parse("[]").expect("empty array").is_empty());
    }

    /// Out-of-range levels clamp rather than panic — `Level::from_ordinal`'s
    /// contract, asserted here because this is the only caller that can be
    /// handed a number from a file a human edited.
    #[test]
    fn out_of_range_levels_clamp() {
        let ops =
            Ops::parse(r#"[{"name":"High","level":99},{"name":"Low","level":-3}]"#).expect("parse");
        assert_eq!(ops.level_for("High"), Level::Owners);
        assert_eq!(ops.level_for("Low"), Level::All);
    }
}
