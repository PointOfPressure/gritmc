//! `Block.setPlacedBy` — the blocks a placement puts down that the placement
//! state never mentioned.
//!
//! # Why a placement table is not the whole placement
//!
//! [`crate::placement`] answers `getStateForPlacement`, and for 1,040 of
//! 1,054 block items that is the entire story. It is not for a door or a
//! bed. From `BlockItem.place`, offsets 156-173:
//!
//! ```text
//!   state = level.getBlockState(clickedPos)          // re-read after the write
//!   if state.is(placementState.getBlock()):
//!       state = updateBlockStateFromTag(...)
//!       updateCustomBlockEntityTag(...)
//!       state.getBlock().setPlacedBy(level, clickedPos, state, player, stack)
//! ```
//!
//! and `setPlacedBy` is free to call `setBlock` again, anywhere. Two blocks
//! in the corpus do:
//!
//! ```text
//!   DoorBlock.setPlacedBy   setBlockAndUpdate(pos.above(),
//!                                             state.setValue(HALF, UPPER))
//!   BedBlock.setPlacedBy    setBlockAndUpdate(pos.relative(FACING),
//!                                             state.setValue(PART, HEAD))
//! ```
//!
//! Without this, `door-places-both-halves` leaves a doorway with no top and
//! `bed-places-two-blocks` a bed with no head — and two further checks fail
//! downstream of that, because there is no upper half for an interaction or
//! a dig to propagate to.
//!
//! # The offset is not a constant, and that is the whole trap
//!
//! A door's second block is always `above`. A bed's is
//! `pos.relative(FACING)`, which is four different offsets depending on
//! which way the player was looking. **A consumer that hard-codes `[0, 1, 0]`
//! passes every door test that will ever be written** and is wrong about
//! every bed. So the offset is carried per state, keyed by the full state
//! string, and `ci/placed-golden.sh` asserts that the four bed facings
//! produce four distinct offsets rather than merely that the table loads.
//!
//! # What the table says, measured rather than assumed
//!
//! `ci/placed-golden.sh` and `tools/goldengen/vanilla/PlacedGolden.java`:
//! **59 blocks with behaviour, 2,046 of 32,366 states doing something**,
//! 52 refusals, 0 random, identical across two runs. It read 50, 1,684 and 12
//! until 2026-08-18, then 56 and 2,010 the same day; see §80, §87 and the
//! bullet on absences below.
//!
//!   * **Flags are carried per write** and not assumed. `setBlockAndUpdate`
//!     is `UPDATE_ALL` (3), but `SmallDripleafBlock` writes with its own
//!     flags, and `Level.setBlock` gates the shape cascade on
//!     `UPDATE_KNOWN_SHAPE` being clear. One hard-coded number would be
//!     right about some of the family and silently wrong about the rest.
//!   * **52 states refuse** on `getChunkSource` — 12 are both pistons and 40
//!     are the two wither skulls, which reached it for the first time on
//!     2026-08-18 because the probe's level started supplying a real
//!     `SkullBlockEntity` and `checkSpawn` could finally run. They are absent
//!     from the table and so do nothing here, which is the honest answer
//!     rather than a guess.
//!   * **Four classes produce nothing, and one of those is the harness.** It
//!     was nine and seven until 2026-08-18. A refusal is loud; an absence is
//!     not, and this table's absences outnumbered its refusals.
//!     `minecraft:repeater` and `minecraft:comparator` had **no row at all** —
//!     `DiodeBlock.setPlacedBy` is `if (shouldTurnOn(...)) level.scheduleTick
//!     (pos, this, 1)` and `shouldTurnOn` reads the input side of a world the
//!     probe fills with air, so a repeater placed already powered never wakes
//!     and the table and this module held the same absent input and agreed
//!     about it.
//!
//!     **They have 64 and 16 cells now and the world is still air.** The fix
//!     was not to make the level throw but to put the read on the row: each
//!     cell says `reads: ["air-world"]` and is emitted *because* it does, so
//!     "wrote nothing" and "wrote nothing, in a world with nothing to read"
//!     stopped being the same row. What is left is a structure block that
//!     writes only block entity data, a jukebox that needs a placement input
//!     this table does not sweep, and `PitcherCropBlock`, which is
//!     `0: return`. **Three overrides, three blocks, and none of them the
//!     probe's environment** — that count reached 0 on 2026-08-18.
//!
//!     The three command blocks were the last of it. `setPlacedBy` is
//!     *entirely* inside `instanceof ServerLevel` and returns `void`, so
//!     unlike the `useWithoutItem` rules there was no `consumes` to be right
//!     by coincidence about and the silence was total. They carry 3 rows and
//!     36 cells now, each stamped `gated: ["blockEntityWritten", "writes"]`,
//!     which is the same shape as a `reads` row: no measurement, and the row
//!     saying so. **Nothing was ported** — the rule is as unreachable as it
//!     was, and it is `needs-subsystem` in
//!     `ci/checks/server-level-gates.toml`. This module is unaffected: the
//!     cells carry an empty `writes` and no tick, so `writes_for` answers the
//!     same empty slice it answered when there was no row at all.
//!     `ci/probe-absences.py` derives the override set from the **jar** and
//!     carries the bytecode read behind each remaining absence in
//!     `ci/checks/probe-absences.toml`.
//!   * **The 24 tick rows are all `crafter`** — `crafting=true,
//!     triggered=true`, delay 4, `NORMAL`, at the block's own position. Not
//!     because the crafter is the only block that schedules a tick on
//!     placement, but because it is the only one whose condition an empty
//!     world can satisfy.
//!   * **`minecraft:small_dripleaf` is `worldSensitive`** and its rows are
//!     still carried. `DoublePlantBlock.copyWaterloggedFrom` reads the fluid
//!     at the position it is about to write, so the extract's answer —
//!     `waterlogged=false` — is right for a placement into air and wrong for
//!     one into water. Every corpus placement is into air. Named here so
//!     that when it matters somebody finds this line rather than the bug.
//!
//! # It also carries the class map, and that is the wider job
//!
//! `classes` and `superclasses` cover the **whole registry**, and
//! [`PlacedByTable::is_a`] is this project's only answer to "what kind of
//! block is this". Two rules key on it — `ButtonBlock` in [`crate::ticks`],
//! `DoorBlock` in [`crate::shapes`] — and neither may key on a block *id*.
//!
//! Two failures are already paid for and are why it is shaped this way:
//!
//!   * a map built from the rows rather than the registry answers "not that
//!     class" for every block it omits, and a rule also asks what class a
//!     block's *neighbour* is, which can be anything;
//!   * comparing simple names is not the test the bytecode performs.
//!     `DoorBlock.updateShape` asks `instanceof`, and 21 blocks answer yes
//!     while only 17 are called `DoorBlock` — the four unwaxed copper doors
//!     are `WeatheringCopperDoorBlock`. Every door check in the corpus places
//!     an *oak* door, so nothing would have caught it.
//!
//! # Scheduled ticks are loaded and deliberately not wired
//!
//! `DiodeBlock.setPlacedBy` and `CrafterBlock.setPlacedBy` schedule a tick,
//! and the extract carries the delay and the priority. Nothing here consumes
//! them: a repeater that started scheduling ticks the moment it was placed
//! would change behaviour this branch has not measured, and new behaviour
//! lands opt-in. [`PlacedByTable::scheduled_tick`] exposes it so the next
//! item starts from data rather than from another extraction.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::interact::BlockPos;

/// One `setBlock` a `setPlacedBy` makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// Offset from the placed position. **Not always `[0, 1, 0]`** — see the
    /// module note.
    pub at: BlockPos,
    pub state: String,
    /// The flags vanilla passed. Carried because the shape cascade is gated
    /// on them.
    pub flags: i32,
}

/// A tick a placement asked for. Loaded, not yet wired — see the module note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub delay: i64,
    /// `TickPriority`'s ordinal. `crate::ticks` sorts its drain order on it.
    pub priority: u8,
    /// Whether it went into the **block** queue. `false` is a fluid tick.
    ///
    /// **The two are not interchangeable and this server only has the block
    /// one.** `TrapDoorBlock.toggle` ends with
    /// `if (WATERLOGGED) scheduleTick(pos, Fluids.WATER, ...)` and
    /// `TrapDoorBlock` has no `tick` override at all, so all 640 trapdoor rows
    /// in the use table are *fluid* ticks. A consumer that read the delay
    /// without the queue would put a block tick for `oak_trapdoor` in the
    /// queue, where it fires and finds no rule — and, worse, where
    /// `UNIQUE_TICK_HASH` then makes it block a real block tick at that
    /// position until it drains.
    pub is_block: bool,
}

/// `Block.setPlacedBy` per block state, from `ci/placed-golden.sh`.
#[derive(Debug, Default)]
pub struct PlacedByTable {
    by_state: HashMap<String, Vec<Write>>,
    ticks: HashMap<String, Tick>,
    /// Block id to vanilla's own class name, for **every** block in the
    /// registry rather than only the ones with a row here.
    ///
    /// This is the project's single answer to "what kind of block is this",
    /// and it exists so that no rule is ever keyed on a block *name*.
    /// "The id ends in `_button`" holds until one block breaks it and the
    /// break is silent. Consumers also ask about a block's *neighbour*,
    /// which can be anything, so a map covering only the 50 blocks with
    /// behaviour would answer "not a door" for most of the game.
    classes: HashMap<String, String>,
    /// Class name to its superclass chain, up to but not including
    /// `java.lang.Object`.
    ///
    /// **Because the bytecode tests `instanceof`, not equality.**
    /// `DoorBlock.updateShape` asks `nb.getBlock() instanceof DoorBlock`, and
    /// the four unwaxed copper doors answer yes while being a
    /// `WeatheringCopperDoorBlock`. Comparing simple names gives those four no
    /// rule and the corpus only ever places an oak door, so nothing catches
    /// it.
    supers: HashMap<String, Vec<String>>,
}

impl PlacedByTable {
    /// Load the extract.
    ///
    /// # Errors
    /// If the file is unreadable, is not JSON, or is not the shape
    /// `PlacedGolden` writes.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        let table = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("set_placed_by.json has no `table` array"))?;

        let mut supers: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(map) = doc
            .get("superclasses")
            .and_then(serde_json::Value::as_object)
        {
            for (class, chain) in map {
                let chain = chain
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                supers.insert(class.clone(), chain);
            }
        }
        let mut classes = HashMap::new();
        if let Some(map) = doc.get("classes").and_then(serde_json::Value::as_object) {
            for (block, class) in map {
                if let Some(class) = class.as_str() {
                    classes.insert(block.clone(), class.to_string());
                }
            }
        }
        let mut by_state: HashMap<String, Vec<Write>> = HashMap::new();
        let mut ticks = HashMap::new();
        for row in table {
            // A random block carries no cells by construction, so this finds
            // none and nothing special is needed — the point of the
            // extraction dropping them rather than writing one sample.
            let Some(states) = row.get("states").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for cell in states {
                if cell.get("refused").is_some() {
                    continue;
                }
                let Some(from) = cell.get("from").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if let (Some(delay), Some(priority)) = (
                    cell.get("tickDelay").and_then(serde_json::Value::as_i64),
                    cell.get("tickPriority").and_then(serde_json::Value::as_str),
                ) {
                    ticks.insert(
                        from.to_string(),
                        Tick {
                            delay,
                            priority: priority_ordinal(priority).ok_or_else(|| {
                                io::Error::other(format!("{from}: unknown TickPriority {priority}"))
                            })?,
                            is_block: cell.get("tickKind").and_then(serde_json::Value::as_str)
                                != Some("fluid"),
                        },
                    );
                }
                let writes = cell
                    .get("writes")
                    .and_then(serde_json::Value::as_array)
                    .map_or(&[][..], Vec::as_slice);
                let mut out = Vec::with_capacity(writes.len());
                for w in writes {
                    let at = w
                        .get("at")
                        .and_then(serde_json::Value::as_array)
                        .filter(|a| a.len() == 3)
                        .ok_or_else(|| io::Error::other(format!("{from}: a write has no `at`")))?;
                    let coord = |i: usize| -> io::Result<i32> {
                        at[i]
                            .as_i64()
                            .and_then(|v| i32::try_from(v).ok())
                            .ok_or_else(|| {
                                io::Error::other(format!("{from}: `at` is not integral"))
                            })
                    };
                    let state = w
                        .get("state")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| io::Error::other(format!("{from}: a write has no state")))?;
                    // Flags are required, not defaulted. A table dumped
                    // before they were recorded would otherwise load and
                    // silently place every second half with whatever number
                    // seemed reasonable here.
                    let flags = w
                        .get("flags")
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(|| {
                            io::Error::other(format!(
                                "{from}: a write has no `flags` — regenerate with \
                                 ci/placed-golden.sh"
                            ))
                        })?;
                    out.push(Write {
                        at: (coord(0)?, coord(1)?, coord(2)?),
                        state: state.to_string(),
                        flags,
                    });
                }
                if !out.is_empty() {
                    by_state.insert(from.to_string(), out);
                }
            }
        }
        if by_state.is_empty() && classes.is_empty() {
            return Err(io::Error::other(
                "set_placed_by.json carries neither writes nor classes",
            ));
        }
        Ok(Self {
            by_state,
            ticks,
            classes,
            supers,
        })
    }

    /// The one table this process serves, loaded once it loads.
    ///
    /// **A failed load is not cached.** It used to be: the first caller's
    /// result was kept whatever it was, so under `cargo test` — cwd
    /// `crates/rustmc-server` — a caller passing the repo-relative
    /// `data/vanilla/set_placed_by.json` cached a not-found `Err`, and every
    /// later caller in the process got it even with a path that resolves.
    /// T0 on `redstone` 43d63a5f1 (3611) failed five `fallable::tests` that
    /// way: `set_placed_by extract is present and did not load`, with the
    /// extract present. A success is still loaded once and kept.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned for `path`, when no earlier call
    /// has loaded the table.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<PlacedByTable> = std::sync::OnceLock::new();
        if let Some(t) = TABLE.get() {
            return Ok(t);
        }
        let t = Self::load(path).map_err(|e| e.to_string())?;
        Ok(TABLE.get_or_init(|| t))
    }

    /// Vanilla's class name for a block id.
    ///
    /// `None` means the extract does not name it, which for a rule keyed on
    /// a class must be read as "not that class" — never as "assume the
    /// usual". See the field note.
    #[must_use]
    pub fn class_of(&self, block: &str) -> Option<&str> {
        self.classes.get(block).map(String::as_str)
    }

    /// A class's superclasses, nearest first, as the extract records them.
    /// Empty for a class it does not name.
    #[must_use]
    pub fn supers_of(&self, class: &str) -> &[String] {
        self.supers.get(class).map_or(&[], Vec::as_slice)
    }

    /// Whether `block` is an instance of `class`, **including subclasses**.
    ///
    /// This is the `instanceof` the bytecode actually performs. Using
    /// [`Self::class_of`] and comparing strings is the same test only when
    /// nothing extends the class, and something usually does:
    /// `WeatheringCopperDoorBlock` extends `DoorBlock`, so four doors would
    /// silently get no door rule.
    #[must_use]
    pub fn is_a(&self, block: &str, class: &str) -> bool {
        let Some(own) = self.class_of(block) else {
            return false;
        };
        own == class
            || self
                .supers
                .get(own)
                .is_some_and(|chain| chain.iter().any(|c| c == class))
    }

    /// The class map as sorted pairs, for a consumer that wants to own a
    /// copy — `crate::shapes` does, so its typed rules can ask what class a
    /// neighbour is without reaching back through a second table.
    #[must_use]
    pub fn class_pairs(&self) -> std::collections::BTreeMap<String, String> {
        self.classes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// How many blocks the class map covers. **1,196 in 26.2** — the whole
    /// registry. A number much smaller than that means the extract predates
    /// the map and every class rule is silently off.
    #[must_use]
    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    /// What this placement writes besides itself, in the order vanilla wrote
    /// them. Empty for the ~30,700 states that place one block and stop.
    #[must_use]
    pub fn writes_for(&self, state: &str) -> &[Write] {
        self.by_state.get(state).map_or(&[], Vec::as_slice)
    }

    /// The tick this placement asked for, if any. **Nothing consumes this
    /// yet** — see the module note.
    #[must_use]
    pub fn scheduled_tick(&self, state: &str) -> Option<Tick> {
        self.ticks.get(state).copied()
    }

    /// How many states write a second block.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_state.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_state.is_empty()
    }

    /// How many states ask for a tick.
    #[must_use]
    pub fn tick_count(&self) -> usize {
        self.ticks.len()
    }
}

/// `TickPriority`'s ordinal, which is what `crate::ticks` sorts on.
///
/// Read from the enum's declaration order in the jar rather than from the
/// numbers in the names: the constants are `EXTREMELY_HIGH(-3)` through
/// `EXTREMELY_LOW(3)`, and it is the **ordinal**, 0..6, that
/// `ScheduledTick.DRAIN_ORDER` compares.
pub(crate) fn priority_ordinal(name: &str) -> Option<u8> {
    Some(match name {
        "EXTREMELY_HIGH" => 0,
        "VERY_HIGH" => 1,
        "HIGH" => 2,
        "NORMAL" => 3,
        "LOW" => 4,
        "VERY_LOW" => 5,
        "EXTREMELY_LOW" => 6,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{PlacedByTable, Write};

    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn table_from(json: &str) -> (PlacedByTable, Temp) {
        let p = std::env::temp_dir().join(format!(
            "rustmc-placedby-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&p, json).expect("write");
        let t = PlacedByTable::load(&p).expect("load");
        (t, Temp(p))
    }

    /// The door: one write, one block up, `half` flipped.
    #[test]
    fn a_door_writes_its_upper_half_one_block_up() {
        let (t, _g) = table_from(
            r#"{"table": [{"block": "minecraft:oak_door", "random": false, "states": [
                {"from": "d[half=lower]", "schedulesTick": false,
                 "writes": [{"at": [0, 1, 0], "state": "d[half=upper]", "flags": 3}]}
            ]}]}"#,
        );
        assert_eq!(
            t.writes_for("d[half=lower]"),
            &[Write {
                at: (0, 1, 0),
                state: "d[half=upper]".into(),
                flags: 3
            }]
        );
        assert!(t.writes_for("something else").is_empty());
    }

    /// **The test the module exists for.** Two states differing in exactly
    /// one property — the bed's `facing` — must produce two *different*
    /// offsets. A consumer that hard-coded the door's `[0, 1, 0]` passes
    /// every door assertion above and fails here, which is the point.
    #[test]
    fn a_beds_head_offset_follows_its_facing_and_nothing_else() {
        let (t, _g) = table_from(
            r#"{"table": [{"block": "minecraft:red_bed", "random": false, "states": [
                {"from": "b[facing=south,part=foot]", "schedulesTick": false,
                 "writes": [{"at": [0, 0, 1], "state": "b[facing=south,part=head]", "flags": 3}]},
                {"from": "b[facing=north,part=foot]", "schedulesTick": false,
                 "writes": [{"at": [0, 0, -1], "state": "b[facing=north,part=head]", "flags": 3}]}
            ]}]}"#,
        );
        let south = t.writes_for("b[facing=south,part=foot]");
        let north = t.writes_for("b[facing=north,part=foot]");
        assert_eq!(south[0].at, (0, 0, 1));
        assert_eq!(north[0].at, (0, 0, -1));
        assert_ne!(
            south[0].at, north[0].at,
            "two states differing only in facing must not share an offset"
        );
    }

    /// A write with no `flags` is refused rather than defaulted. A table
    /// dumped before flags were recorded would otherwise load and place
    /// every second half with whatever number looked reasonable.
    #[test]
    fn a_write_without_flags_is_refused() {
        let p = std::env::temp_dir().join(format!(
            "rustmc-placedby-noflags-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &p,
            r#"{"table": [{"block": "b", "random": false, "states": [
                {"from": "s", "writes": [{"at": [0, 1, 0], "state": "t"}]}]}]}"#,
        )
        .expect("write");
        let err = PlacedByTable::load(&p).expect_err("a flagless write is refused");
        assert!(err.to_string().contains("flags"), "{err}");
        let _ = std::fs::remove_file(&p);
    }

    /// Refused states carry no writes and must not become an empty answer
    /// that looks like "this block does nothing".
    #[test]
    fn a_refused_state_is_absent_rather_than_empty() {
        let (t, _g) = table_from(
            r#"{"table": [{"block": "minecraft:piston", "random": false, "states": [
                {"from": "p[extended=true]", "refused": "UnsupportedOperationException"},
                {"from": "p[extended=false]", "schedulesTick": false,
                 "writes": [{"at": [0, 1, 0], "state": "q", "flags": 3}]}
            ]}]}"#,
        );
        assert!(t.writes_for("p[extended=true]").is_empty());
        assert_eq!(t.len(), 1);
    }

    /// **`instanceof`, not string equality.** The four unwaxed copper doors
    /// are `WeatheringCopperDoorBlock`, which extends `DoorBlock`; a consumer
    /// comparing simple names gives them no door rule and the corpus, which
    /// only ever places an oak door, would never notice. The two blocks here
    /// differ in exactly that — one is the class, one is a subclass of it.
    #[test]
    fn is_a_walks_the_superclass_chain() {
        let (t, _g) = table_from(
            r#"{"classes": {"minecraft:oak_door": "DoorBlock",
                            "minecraft:copper_door": "WeatheringCopperDoorBlock",
                            "minecraft:stone": "Block"},
                "superclasses": {
                    "DoorBlock": ["Block", "BlockBehaviour"],
                    "WeatheringCopperDoorBlock": ["DoorBlock", "Block", "BlockBehaviour"],
                    "Block": ["BlockBehaviour"]},
                "table": []}"#,
        );
        assert!(t.is_a("minecraft:oak_door", "DoorBlock"));
        assert!(
            t.is_a("minecraft:copper_door", "DoorBlock"),
            "a subclass is an instance of its parent"
        );
        assert!(!t.is_a("minecraft:stone", "DoorBlock"));
        assert!(
            !t.is_a("minecraft:nothing", "DoorBlock"),
            "an unknown block is not"
        );
        // And the parent is not an instance of the child.
        assert!(!t.is_a("minecraft:oak_door", "WeatheringCopperDoorBlock"));
    }

    /// The tick columns load, with the ordinal `crate::ticks` sorts on.
    #[test]
    fn a_scheduled_tick_carries_its_delay_and_priority_ordinal() {
        let (t, _g) = table_from(
            r#"{"table": [{"block": "minecraft:crafter", "random": false, "states": [
                {"from": "c", "schedulesTick": true, "tickDelay": 4,
                 "tickPriority": "NORMAL", "tickAt": [0, 0, 0],
                 "writes": [{"at": [0, 1, 0], "state": "d", "flags": 3}]}
            ]}]}"#,
        );
        let tick = t.scheduled_tick("c").expect("a tick");
        assert_eq!(tick.delay, 4);
        assert_eq!(tick.priority, 3);
        assert!(tick.is_block, "no tickKind means the block queue");
        assert_eq!(t.tick_count(), 1);
    }
}
