//! `BlockState.useItemOn` — the half of a right-click that is handed the
//! stack, and the one that runs **before** [`crate::useblock`]'s.
//!
//! # Why this is a second table and not a column on the first one
//!
//! `ServerPlayerGameMode.useItemOn` asks the block twice, in this order:
//!
//! ```text
//!   145  r = state.useItemOn(stack, level, player, hand, hit)
//!        if r.consumesAction()                            -> r
//!   196  if r instanceof TryEmptyHandInteraction && hand == MAIN_HAND:
//!            r2 = state.useWithoutItem(level, player, hit)
//!            if r2.consumesAction()                       -> r2
//!   223  the placement path
//! ```
//!
//! `useWithoutItem` is not handed the stack, so `use_without_item.json` has no
//! item axis and cannot hold an answer that depends on what you are holding.
//! `data/vanilla/use_item_on.json` is the same probe swept over the whole item
//! registry — 1,537 items x 4,537 states — and it is the table this module
//! reads.
//!
//! # This module answers for one class, on purpose
//!
//! The table describes **163 blocks**. Applying all of them at once would
//! change the answer to a right-click on 163 blocks in one commit, none of
//! them measured, which is the opposite of the rule that new behaviour lands
//! opt-in until it is measured. [`PORTED_CLASSES`] is therefore an explicit
//! list and [`UseItemOnTable::load`] keeps only rows whose `class` is in it.
//! Widening it is a deliberate act with a corpus run behind it, not a
//! side effect of regenerating the table.
//!
//! Today it is two classes. `FlowerPotBlock` is **39 blocks**:
//! `minecraft:flower_pot` and the 38 `minecraft:potted_*`, one state each.
//! `SweetBerryBushBlock` is **one block and three states** — `age` 0..2; the
//! table omits `age=3`, where every item is the default
//! `TRY_WITH_EMPTY_HAND`, which is also what a missing row answers. All 42
//! are fully measured with no refusal, and [`UseItemOnTable::blocks`] is what
//! says so at run time rather than in a comment.
//!
//! # `SweetBerryBushBlock`, from the jar
//!
//! ```text
//!   useItemOn(stack, state, level, pos, player, hand, hit)
//!      0-26   boolean ripe = state.getValue(AGE) == 3
//!     28-46   if (!ripe && stack.is(Items.BONE_MEAL)) return PASS
//!     47-62   return super.useItemOn(...)          — TRY_WITH_EMPTY_HAND
//! ```
//!
//! The `PASS` is the whole reason it is here. Without the row, bone meal on a
//! bush at `age=2` fell through to `useWithoutItem`, which picks the berries
//! (`age > 1`) and writes `age=1`, and **then** bone meal ran on the picked
//! bush and wrote `age=2` — both in one tick, so the client saw one
//! `block_update` for `age=2` where vanilla sends `age=3` and a `1505`.
//! Scenario 975, the third use.
//!
//! # `FlowerPotBlock`, from the jar
//!
//! ```text
//!   useItemOn(stack, state, level, pos, player, hand, hit)
//!      0-19   Item item = stack.getItem(); if (item instanceof BlockItem bi)
//!     21-40       block = POTTED_BY_CONTENT.getOrDefault(bi.getBlock(), AIR)
//!        43   else block = AIR
//!        46   BlockState potted = block.defaultBlockState()
//!     51-62   if (potted.isAir())  return TRY_WITH_EMPTY_HAND
//!     63-73   if (!this.isEmpty()) return CONSUME
//!     74-83   level.setBlock(pos, potted, 3)
//!     84-95   level.gameEvent(player, BLOCK_CHANGE, pos)
//!    95-102   player.awardStat(Stats.POT_FLOWER)
//!   103-113   stack.consume(1, player);  return SUCCESS
//!
//!   useWithoutItem(state, level, pos, player, hit)
//!      0-10   if (this.isEmpty()) return CONSUME
//!     11-22   ItemStack out = new ItemStack(this.potted)
//!     24-42   if (!player.addItem(out)) player.drop(out, false)
//!     43-55   level.setBlock(pos, FLOWER_POT.defaultBlockState(), 3)
//!     56-65   level.gameEvent(player, BLOCK_CHANGE, pos)
//!        66   return SUCCESS
//! ```
//!
//! `isEmpty()` is `this.potted == Blocks.AIR`, so the two methods are one
//! mechanism seen from either side, and the **order of the two tests in
//! `useItemOn` decides the interesting case**: the `isAir` test at 51 comes
//! first, so clicking a *full* pot with something that is not pottable falls
//! out at `TRY_WITH_EMPTY_HAND` and reaches `useWithoutItem`, which empties
//! it. Clicking a full pot with something that *is* pottable returns `CONSUME`
//! at 73 and the pot keeps what it had. Both are in the table; neither is
//! guessable from the block's name.
//!
//! # The item a pot hands back, and where the map comes from
//!
//! `useWithoutItem` offset 11 is `new ItemStack(this.potted)` and `potted` is
//! a private field with no accessor, so no probe can read it directly. It is
//! recovered here from the **potting direction** instead:
//! `POTTED_BY_CONTENT.getOrDefault(bi.getBlock(), AIR)` is the same field's
//! map, and `minecraft:flower_pot`'s row records one `except` group per
//! pottable item saying which `potted_*` that item writes. Inverting those 38
//! groups gives potted state -> the item that fills it, and
//! `content.asItem()` is that item for every one of them because the map's key
//! is `BlockItem.getBlock()` of exactly that item.
//!
//! That is a **code map read out of the code's own behaviour**, which is why
//! it is preferred to the other candidate. `data/vanilla/loot_blocks.json`
//! also recovers the pairing — every `potted_*` drops `minecraft:flower_pot`
//! and its content — but a loot table is a datapack correspondence rather than
//! this field, so agreeing with it is evidence and not identity. The loader
//! refuses a table whose inversion is not injective. The loot route was
//! checked against this one and agrees on **38 of 38** (2026-08-12,
//! `~/rustmc-pot/data/vanilla/`), and it is recorded in
//! `docs/cold-start/gameplay-790-flower-pot.md` rather than as a test here,
//! because a test would name `loot_blocks.json` in `crates/` and so make a
//! second table required of every tree for a corroboration that is already
//! taken. The table is the source; the loot route is the witness.
//!
//! # What this module does not model
//!
//! * **`Stats.POT_FLOWER`.** No statistics anywhere in this server, and
//!   nothing on the wire until one asks for them.
//! * **`Level.gameEvent`.** Sculk has no listener here.
//! * **`stack.consume(1, player)`'s creative branch is modelled by the
//!   caller, not by the table.** `ItemStack.consume(n, entity)` is
//!   `if (entity == null || !entity.hasInfiniteMaterials()) shrink(n)`, so the
//!   probe's `stackAfter: "empty"` is a survival player's answer over a
//!   one-count stack. `minecraft:flower_pot` is the row `use_item_on.json`
//!   marks `probeDependent` on the `game_master` axis for exactly this reason,
//!   and the axis moves the stack and never the block write. This module
//!   therefore reports *that a consume happens* and leaves whether it shrinks
//!   to the caller, which is the only place that knows the player's abilities.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::Path;

/// `FlowerPotBlock` — 39 blocks, one state each.
pub const FLOWER_POT_BLOCK: &str = "FlowerPotBlock";

/// The classes this server answers `useItemOn` for.
///
/// See the module header: the table holds 163 and this list is what keeps a
/// regenerated table from turning 162 unmeasured blocks on by itself.
pub const PORTED_CLASSES: &[&str] = &[FLOWER_POT_BLOCK, SWEET_BERRY_BUSH_BLOCK];

/// `SweetBerryBushBlock` — one block, three states in the table.
pub const SWEET_BERRY_BUSH_BLOCK: &str = "SweetBerryBushBlock";

/// The one probe axis a kept row may depend on.
///
/// `game_master` is `Player.hasInfiniteMaterials()`, which decides whether
/// `ItemStack.consume` shrinks. The caller resolves it from the player's own
/// game mode, so a row that depends on it is answerable; a row that depends on
/// anything else is not, and the loader refuses rather than serving the
/// probe's stance as though it were a constant.
const ANSWERABLE_AXIS: &str = "game_master";

/// What the block did with the click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsedItem {
    /// `InteractionResult.TryEmptyHandInteraction` — the marker that sends the
    /// click on to `useWithoutItem`. Also what a state with no row here
    /// answers, which is the same fall-through this server had before the
    /// table existed.
    TryWithEmptyHand,
    /// `consumesAction()`. The placement path must not run.
    Consumed(Consumed),
    /// `PASS` or `FAIL`: the block declined without consuming, and the click
    /// goes to the placement path without `useWithoutItem` being asked.
    Pass,
}

/// The consuming branch's effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumed {
    /// The state written at the clicked position, and `Level.setBlock`'s third
    /// argument at that call site. `None` is a consume that writes nothing —
    /// a pottable item on a pot that is already full.
    pub write: Option<(String, i32)>,
    /// `stack.consume(1, player)` ran. Whether it shrinks is the caller's
    /// question; see the module header.
    pub consume_one: bool,
}

/// One state's answers: the default, and the items that differ from it.
#[derive(Debug, Clone)]
struct StateRow {
    default: Cell,
    except: HashMap<String, Cell>,
}

/// One `(state, item)` answer.
#[derive(Debug, Clone)]
struct Cell {
    /// `TRY_WITH_EMPTY_HAND`.
    try_with_empty_hand: bool,
    /// `InteractionResult.consumesAction()`.
    consumes: bool,
    write: Option<(String, i32)>,
    consume_one: bool,
}

/// `useItemOn` per state and held item, for [`PORTED_CLASSES`].
#[derive(Debug, Default)]
pub struct UseItemOnTable {
    by_state: HashMap<String, StateRow>,
    /// The inverse of `POTTED_BY_CONTENT`: a potted state -> the item
    /// `new ItemStack(this.potted)` hands back.
    ///
    /// Ordered, because it is iterated: a `HashMap` here would make the order
    /// of a test's 38 assertions a property of the allocator, and
    /// `clippy::iter_over_hash_type` is denied in this workspace for exactly
    /// that reason.
    content_of: BTreeMap<String, String>,
    blocks: usize,
}

impl UseItemOnTable {
    /// Load the extract, keeping only [`PORTED_CLASSES`].
    ///
    /// # Errors
    /// If the file is unreadable, is not JSON, is not the shape
    /// `UseItemOnGolden` writes, or holds a kept row this module cannot
    /// answer from — a refusal, a second write, a write away from the clicked
    /// block, a scheduled tick, a sound, a probe axis that is not
    /// [`ANSWERABLE_AXIS`], or a `stackAfter` that is not the one-count
    /// probe's `empty`.
    ///
    /// **Every one of those is a refusal rather than a skip.** A row that is
    /// silently dropped is a block that goes back to falling through to the
    /// placement path, which is the inversion `crate::useblock`'s header is
    /// about; a row that is silently truncated is a block that half works.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        let table = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("use_item_on.json has no `table` array"))?;

        let mut by_state = HashMap::new();
        let mut potted_by_content: BTreeMap<String, String> = BTreeMap::new();
        let mut blocks = 0;
        for row in table {
            let class = row.get("class").and_then(serde_json::Value::as_str);
            if !class.is_some_and(|c| PORTED_CLASSES.contains(&c)) {
                continue;
            }
            let block = row
                .get("block")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| io::Error::other("a row with a class and no block"))?;
            // A `random` block keeps no cells at all — the extract drops them
            // rather than writing one sample of a die roll — so a kept class
            // that is random would be silently empty here.
            for flag in ["random", "faceSensitive", "handSensitive"] {
                if row.get(flag) == Some(&serde_json::Value::Bool(true)) {
                    return Err(io::Error::other(format!(
                        "{block}: {flag}, which this module has no axis for"
                    )));
                }
            }
            for axis in row
                .get("probeAxes")
                .and_then(serde_json::Value::as_array)
                .map_or(&[][..], Vec::as_slice)
            {
                if axis.as_str() != Some(ANSWERABLE_AXIS) {
                    return Err(io::Error::other(format!(
                        "{block}: probe axis {axis}, which the caller cannot resolve"
                    )));
                }
            }
            if row
                .get("probeUnknown")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|a| !a.is_empty())
            {
                return Err(io::Error::other(format!(
                    "{block}: an unmeasured probe axis, so its rows are the probe's stance"
                )));
            }
            let states = row
                .get("states")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| io::Error::other(format!("{block}: no `states`")))?;
            blocks += 1;
            for state in states {
                let from = state
                    .get("from")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| io::Error::other(format!("{block}: a state with no `from`")))?;
                if state.get("refused").is_some() {
                    return Err(io::Error::other(format!(
                        "{from}: refused, and this module has no fallback that is not a guess"
                    )));
                }
                let default = state
                    .get("default")
                    .ok_or_else(|| io::Error::other(format!("{from}: no `default` cell")))?;
                let default = cell(from, default)?;
                let mut except = HashMap::new();
                for group in state
                    .get("except")
                    .and_then(serde_json::Value::as_array)
                    .map_or(&[][..], Vec::as_slice)
                {
                    let c = cell(
                        from,
                        group.get("cell").ok_or_else(|| {
                            io::Error::other(format!("{from}: a group with no cell"))
                        })?,
                    )?;
                    let items = group
                        .get("items")
                        .and_then(serde_json::Value::as_array)
                        .map_or(&[][..], Vec::as_slice);
                    // The potting direction, read off the empty pot's own row:
                    // one item, one write, and that write names the pot it
                    // fills. Groups with more than one item are the *refusing*
                    // direction — every pottable item gives one potted block
                    // the same `CONSUME` — and carry no write, so they are not
                    // a pairing and are not read as one.
                    if let (Some(w), [only]) = (c.write.as_ref(), items) {
                        if let Some(item) = only.as_str() {
                            if let Some(prev) = potted_by_content.insert(w.0.clone(), item.into()) {
                                return Err(io::Error::other(format!(
                                    "{from}: both {prev} and {item} write {} — the pairing is \
                                     not a bijection and cannot be inverted",
                                    w.0
                                )));
                            }
                        }
                    }
                    for item in items {
                        let item = item.as_str().ok_or_else(|| {
                            io::Error::other(format!("{from}: a non-string item name"))
                        })?;
                        except.insert(item.to_string(), c.clone());
                    }
                }
                by_state.insert(from.to_string(), StateRow { default, except });
            }
        }
        // The inversion is over the whole kept set rather than one row, so a
        // table that split the pairing across two blocks still inverts.
        Ok(Self {
            by_state,
            content_of: potted_by_content,
            blocks,
        })
    }

    /// The one table this process serves, loaded once. 1.7 MB of JSON, so
    /// shared for the reason [`crate::useblock::UseTable::shared`] is.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static TABLE: std::sync::OnceLock<Result<UseItemOnTable, String>> =
            std::sync::OnceLock::new();
        TABLE
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// How many blocks were kept. **39** for the classes ported today, and the
    /// number a claim of coverage has to quote.
    #[must_use]
    pub const fn blocks(&self) -> usize {
        self.blocks
    }

    /// How many states have an answer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_state.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_state.is_empty()
    }

    /// How many pairings were inverted. **38**: every `potted_*` but not the
    /// empty pot, which hands nothing back.
    #[must_use]
    pub fn pairings(&self) -> usize {
        self.content_of.len()
    }
}

/// One cell of the grid.
fn cell(from: &str, v: &serde_json::Value) -> io::Result<Cell> {
    if v.get("refused").is_some() {
        return Err(io::Error::other(format!(
            "{from}: a refused cell inside a kept row"
        )));
    }
    for flag in ["schedulesTick", "sound"] {
        if v.get(flag) == Some(&serde_json::Value::Bool(true)) {
            return Err(io::Error::other(format!(
                "{from}: {flag}, which this module would silently drop"
            )));
        }
    }
    let result = v
        .get("result")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| io::Error::other(format!("{from}: a cell with no result")))?;
    let consumes = v.get("consumes") == Some(&serde_json::Value::Bool(true));
    let writes = v
        .get("writes")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if writes.len() > 1 {
        return Err(io::Error::other(format!(
            "{from}: {} writes, and the ported classes write at most one",
            writes.len()
        )));
    }
    let mut write = None;
    if let Some(w) = writes.first() {
        let at = w.get("at").and_then(serde_json::Value::as_array);
        if !at.is_some_and(|a| a.len() == 3 && a.iter().all(|v| v.as_i64() == Some(0))) {
            return Err(io::Error::other(format!(
                "{from}: a write away from the clicked block"
            )));
        }
        let state = w
            .get("state")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| io::Error::other(format!("{from}: a write with no state")))?;
        let flags = w
            .get("flags")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| io::Error::other(format!("{from}: a write with no flags")))?;
        write = Some((
            state.to_string(),
            i32::try_from(flags).map_err(io::Error::other)?,
        ));
    }
    // `stackAfter` is present only where the probe's one-count stack ended
    // empty, which for these classes is `consume(1, player)` and nothing else.
    // Anything else would be a consumption this module cannot express.
    let consume_one = match v.get("stackAfter").and_then(serde_json::Value::as_str) {
        None => false,
        Some("empty") => true,
        Some(other) => {
            return Err(io::Error::other(format!(
                "{from}: stackAfter {other}, which is not a one-count consume"
            )))
        }
    };
    Ok(Cell {
        try_with_empty_hand: result == "TRY_WITH_EMPTY_HAND",
        consumes,
        write,
        consume_one,
    })
}

/// `BlockState.useItemOn`, at `ServerPlayerGameMode.useItemOn` offset 145.
///
/// `None` for the table, or a state with no row, is
/// [`UsedItem::TryWithEmptyHand`] — `BlockBehaviour`'s own default and the
/// fall-through that reaches `useWithoutItem`. That is the answer for the
/// 1,033 blocks the extract skipped as well as for the 162 classes this
/// module does not yet serve, and it is deliberately the same answer this
/// server gave before the table existed.
///
/// `item` is the **main hand** stack's item, or `None` for an empty hand. An
/// empty hand cannot be a `BlockItem`, so it takes the `isAir` branch at
/// offset 51 exactly as an unpottable item does.
#[must_use]
pub fn use_item_on(table: Option<&UseItemOnTable>, state: &str, item: Option<&str>) -> UsedItem {
    let Some(row) = table.and_then(|t| t.by_state.get(state)) else {
        return UsedItem::TryWithEmptyHand;
    };
    let cell = item.and_then(|i| row.except.get(i)).unwrap_or(&row.default);
    if cell.try_with_empty_hand {
        return UsedItem::TryWithEmptyHand;
    }
    if cell.consumes {
        return UsedItem::Consumed(Consumed {
            write: cell.write.clone(),
            consume_one: cell.consume_one,
        });
    }
    UsedItem::Pass
}

/// `new ItemStack(this.potted)` — what `FlowerPotBlock.useWithoutItem` hands
/// back at offset 11, for a state that has a pairing.
///
/// `None` for every other state, including the empty pot: `isEmpty()` returns
/// at offset 0 there and nothing is created.
///
/// This is the half of `useWithoutItem` `use_without_item.json` cannot hold —
/// that table records the block write and has no column for an item — so the
/// write comes from `crate::useblock` as it does for every other block and
/// only the hand-back comes from here.
#[must_use]
pub fn given_back<'a>(table: Option<&'a UseItemOnTable>, state: &str) -> Option<&'a str> {
    table
        .and_then(|t| t.content_of.get(state))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(rel: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel)
    }

    /// The real extract.
    ///
    /// **Panics rather than skipping**, and that is the point of this branch:
    /// `data/vanilla/use_item_on.json` was optional until this module named
    /// it, and is now one of the tables `ci/gen-gameplay-tables.sh` derives
    /// and every tree has. A test that quietly returned on a missing file
    /// would pass in exactly the tree where the port does nothing.
    fn table() -> UseItemOnTable {
        let p = repo("data/vanilla/use_item_on.json");
        UseItemOnTable::load(&p)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", p.display()))
    }

    /// The coverage claim, made by the loader rather than by a comment: two
    /// classes, 40 blocks, 42 states, no refusals anywhere in them, and 38
    /// pairings.
    #[test]
    fn the_ported_classes_are_forty_blocks_and_thirty_eight_pairings() {
        let t = table();
        assert_eq!(
            t.blocks(),
            40,
            "FlowerPotBlock's 39 and the sweet berry bush"
        );
        assert_eq!(t.len(), 42, "one state per pot, three bush ages");
        assert_eq!(t.pairings(), 38, "every potted block but the empty pot");
    }

    /// `SweetBerryBushBlock.useItemOn` offsets 28-46: bone meal on an unripe
    /// bush is `PASS`, so `useWithoutItem` — picking — is skipped and the
    /// item's own `useOn` runs. Anything else, and every item on a ripe bush,
    /// tries the empty hand. Red before this class was listed: every arm
    /// below answered `TryWithEmptyHand`.
    #[test]
    fn bone_meal_on_an_unripe_bush_passes_and_never_picks() {
        let t = table();
        for age in 0..=2 {
            let state = format!("minecraft:sweet_berry_bush[age={age}]");
            assert_eq!(
                use_item_on(Some(&t), &state, Some("minecraft:bone_meal")),
                UsedItem::Pass,
                "{state}"
            );
            assert_eq!(
                use_item_on(Some(&t), &state, Some("minecraft:stick")),
                UsedItem::TryWithEmptyHand,
                "{state}"
            );
        }
        assert_eq!(
            use_item_on(
                Some(&t),
                "minecraft:sweet_berry_bush[age=3]",
                Some("minecraft:bone_meal")
            ),
            UsedItem::TryWithEmptyHand
        );
    }

    /// The scenario `790` runs, both directions, off the real table.
    #[test]
    fn a_dandelion_pots_and_the_pot_hands_it_back() {
        let t = table();
        let potting = use_item_on(
            Some(&t),
            "minecraft:flower_pot",
            Some("minecraft:dandelion"),
        );
        assert_eq!(
            potting,
            UsedItem::Consumed(Consumed {
                write: Some(("minecraft:potted_dandelion".into(), 3)),
                consume_one: true,
            })
        );
        // A full pot clicked with something unpottable falls through to
        // `useWithoutItem`, which is the whole of `790`'s step.
        assert_eq!(
            use_item_on(
                Some(&t),
                "minecraft:potted_dandelion",
                Some("minecraft:stick")
            ),
            UsedItem::TryWithEmptyHand
        );
        assert_eq!(
            given_back(Some(&t), "minecraft:potted_dandelion"),
            Some("minecraft:dandelion")
        );
        assert_eq!(given_back(Some(&t), "minecraft:flower_pot"), None);
    }

    /// The branch a name-based guess gets wrong: a pottable item on a pot that
    /// is already full is `CONSUME` with **no** write, so the pot keeps what
    /// it had and no placement happens either.
    #[test]
    fn a_pottable_item_on_a_full_pot_consumes_and_writes_nothing() {
        let t = table();
        assert_eq!(
            use_item_on(
                Some(&t),
                "minecraft:potted_dandelion",
                Some("minecraft:poppy")
            ),
            UsedItem::Consumed(Consumed {
                write: None,
                consume_one: false,
            })
        );
    }

    /// An empty hand takes the `isAir` branch, not a fourth one.
    #[test]
    fn an_empty_hand_falls_through_to_the_empty_hand_method() {
        let t = table();
        assert_eq!(
            use_item_on(Some(&t), "minecraft:flower_pot", None),
            UsedItem::TryWithEmptyHand
        );
    }

    /// Every one of the 38 pairings is exercised, in both directions, so the
    /// claim is over 39 blocks and not over the one the corpus clicks.
    #[test]
    fn all_thirty_eight_pots_fill_and_empty() {
        let t = table();
        let mut seen = 0;
        for (potted, item) in &t.content_of {
            assert_eq!(
                use_item_on(Some(&t), "minecraft:flower_pot", Some(item)),
                UsedItem::Consumed(Consumed {
                    write: Some((potted.clone(), 3)),
                    consume_one: true,
                }),
                "{item} should pot"
            );
            assert_eq!(given_back(Some(&t), potted), Some(item.as_str()));
            seen += 1;
        }
        assert_eq!(seen, 38);
    }

    /// The other half of the pair, pinned against the table that supplies it:
    /// every potted state's `useWithoutItem` writes the empty pot. Without
    /// this the hand-back could be right while the block write silently was
    /// not, and the two come from different files.
    #[test]
    fn every_pairing_agrees_with_the_use_without_item_table() {
        let t = table();
        let p = repo("data/vanilla/use_without_item.json");
        let uses = crate::useblock::UseTable::load(&p)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", p.display()));
        for potted in t.content_of.keys() {
            assert_eq!(
                crate::useblock::use_without_item(Some(&uses), potted),
                crate::useblock::Used::Consumed(Some("minecraft:flower_pot".into())),
                "{potted} should empty to a flower pot"
            );
            assert_eq!(uses.write_flags(potted), Some(3));
        }
    }

    /// A refusal inside a kept row is refused rather than skipped: a skipped
    /// row is a block that goes back to falling through to placement, which is
    /// the inversion this module exists to stop.
    #[test]
    fn a_refused_cell_in_a_kept_row_refuses_the_load() {
        let dir = std::env::temp_dir().join(format!("useitemon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let p = dir.join("refused.json");
        std::fs::write(
            &p,
            r#"{"table":[{"block":"minecraft:flower_pot","class":"FlowerPotBlock",
                "states":[{"from":"minecraft:flower_pot","refused":"NoBlockEntity"}]}]}"#,
        )
        .expect("write");
        let err = UseItemOnTable::load(&p).expect_err("refused");
        assert!(err.to_string().contains("refused"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A class this module has not ported is not served, however complete its
    /// row is. The 163-block table must not turn 162 blocks on by itself.
    #[test]
    fn an_unported_class_is_not_served() {
        let t = table();
        assert_eq!(
            use_item_on(
                Some(&t),
                "minecraft:cake[bites=0]",
                Some("minecraft:candle")
            ),
            UsedItem::TryWithEmptyHand,
            "CakeBlock is measured and deliberately not ported"
        );
    }
}
