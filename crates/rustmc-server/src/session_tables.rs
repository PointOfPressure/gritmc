//! The eight optional tables a production [`crate::world_send::Session`] carries.
//!
//! # Why this module exists at all
//!
//! `Session` has eight optional tables — `placement`, `shapes`,
//! `sound_types`, `sound_events`, `ticks`, `placed_by`, `recipes`,
//! `fire_odds` — and when there were six of them, until 2026-08-10, it had
//! **six construction sites, of which five were tests that passed `None` to
//! all six**. The sixth was `status::listen`, which sets every one.
//! (`fire_odds` arrived later, with its own arm in `tests/session_joint.rs`
//! for the reason the last paragraph of this header gives.) So the wiring
//! that carries every one of those tables out of the boot path and into the
//! connection loop was witnessed by nothing in any gate: only by a box-wide
//! corpus run that takes the better part of an hour and a contended lock.
//!
//! That is not N untested mechanisms. Every callee has module tests —
//! `crate::placement`, `crate::shapes`, `crate::placedby`, `crate::ticks`,
//! `crate::sound_types`, `crate::sound_events` and `crate::recipes` are each
//! covered where they live. It is **one untested joint, once per table**: the
//! question no test
//! asked was whether setting the field does anything, and that question has
//! exactly one answer per field and no answer at all in aggregate.
//!
//! It is also the shape that left `IcebergFeature` green for a week. The
//! feature was fine; the gate simply never reached it.
//!
//! # What this module is, and what it deliberately is not
//!
//! It is **the one place they are loaded**. `status::listen` calls
//! [`SessionTables::load`] and reads its fields straight into the `Session`
//! literal; `tests/session_joint.rs` calls the same function with the repo
//! root and reads them into the same literal. Neither has a copy of the
//! loading logic, so a test cannot witness a fixture that has drifted from
//! production: there is no second construction to drift.
//!
//! That is the point of the `root` argument, and it is the only difference
//! between the two callers. Production runs with the repo as its working
//! directory and passes `.`; an integration test runs with the *crate* as
//! its working directory and passes `../..`. Everything downstream of the
//! join — which file, which loader, which fallback on a missing file, which
//! line on the console — is shared.
//!
//! It is **not** a place to put convenience. A helper that gave a test a
//! shorter path to a `Session` than production takes would reintroduce
//! exactly the divergence this closes.
//!
//! # `None` is a value here, not an error
//!
//! Every one of them is an `Option` because a checkout without the
//! gitignored vanilla extract must still boot and still serve a player. Each
//! absence has a *specific* visible consequence — a stair that will not face
//! the placer, a torch that floats, a world you cannot hear — and each is
//! announced on the console rather than defaulted, because "not implemented"
//! and "the extract is missing" are indistinguishable from a client. The
//! announcements live here with the loads for that reason; moving them to the
//! call site is how one of them goes quiet.

use std::path::Path;

/// The eight optional tables, loaded together.
///
/// Two of them are owned (`placement`, `recipes` parse per call) and five are
/// `&'static` borrows of a process-wide `OnceLock` (`shapes`, `placed_by`,
/// `sound_types`, `sound_events`, `ticks`). That asymmetry is inherited from the loaders and
/// is not smoothed over: `update_shape.json` is three megabytes and sits on
/// the per-connection path, so sharing it is the reason it is shared, and
/// hiding which of them are shared would hide the only thing about them
/// that differs.
pub struct SessionTables {
    /// `getStateForPlacement` per block. `None` means every placement takes
    /// the block's default state.
    pub placement: Option<crate::placement::PlacementTable>,
    /// The crafting recipes and the item tags their ingredients name.
    /// `None` means a crafting table opens and its result slot never fills.
    pub recipes: Option<crate::recipes::Recipes>,
    /// `updateShape` per state and direction. `None` means nothing cascades.
    pub shapes: Option<&'static crate::shapes::ShapeTable>,
    /// `setPlacedBy` per state. `None` means a door has no top.
    pub placed_by: Option<&'static crate::placedby::PlacedByTable>,
    /// `SoundType` per state. `None` means nothing you place is heard.
    pub sound_types: Option<&'static crate::sound_types::SoundTypes>,
    /// `SoundEvent.fixedRange()` per registry entry. `None` means every sound
    /// is broadcast at the variable-range radius, which is right for all but
    /// the handful of events that carry a fixed one — so unlike the other
    /// others, an absence here is a small wrong answer rather than silence, and
    /// its console line says so.
    pub sound_events: Option<&'static crate::sound_events::SoundEventTable>,
    /// The scheduled-tick queue. `None` means a block that asks to be woken
    /// later never is.
    pub ticks: Option<&'static crate::ticks::ScheduledTicks>,
    /// Every item's default data components. `None` means every item stacks
    /// to [`crate::item::DEFAULT_MAX_STACK_SIZE`], which is wrong for 295 of
    /// the 1,537 in the registry — see [`crate::itemprops`].
    pub item_props: Option<&'static crate::itemprops::ItemProps>,
    /// Which `canSurvive` rule each block a `BlockItem` names reaches.
    /// `None` means `BlockItem.canPlace`'s survive gate is not asked and a
    /// torch may be placed on air — see [`crate::itemsurvive`].
    pub survive: Option<&'static crate::itemsurvive::SurviveClasses>,
    /// Who `BoneMealItem.useOn` can grow. `None` means bone meal is inert.
    pub bonemeal: Option<&'static crate::bonemeal::BonemealTable>,
    /// `FireBlock`'s two flammability maps. `None` means a flint-and-steel
    /// used anywhere but on a block with a sturdy upward face writes nothing
    /// at all, because the connection walk cannot be answered.
    pub fire_odds: Option<&'static crate::fire::FireOdds>,
}

impl SessionTables {
    /// Load all eight, relative to `root`.
    ///
    /// `root` is the directory `data/vanilla` sits under: `.` for a server
    /// booted from the repo, `../..` for an integration test whose working
    /// directory is the crate. It is the **only** parameter, deliberately —
    /// see the module note.
    ///
    /// Never fails. Each table that cannot be read becomes `None` and says
    /// so on the console, which is the behaviour a checkout without the
    /// vanilla extract depends on.
    #[must_use]
    pub fn load(root: &Path) -> Self {
        // Not a field: the enchantment registry is process-wide state the
        // workstation menus reach through `Registry::shared`, loaded here so
        // that the boot log says whether it is there.
        match crate::enchantment::Registry::load_shared(&root.join(crate::enchantment::TABLE_PATH))
        {
            Some(r) => eprintln!("enchantments: {} in the registry", r.len()),
            None => eprintln!(
                "no enchantment registry — an anvil combines nothing and an enchanting \
                 table offers nothing"
            ),
        }
        Self {
            placement: load_placement(root),
            recipes: load_recipes(root),
            shapes: load_shapes(root),
            placed_by: load_placed_by(root),
            sound_types: load_sound_types(root),
            sound_events: load_sound_events(root),
            item_props: load_item_props(root),
            survive: load_survive(root),
            bonemeal: load_bonemeal(root),
            // Not read from a file: this is the live queue the clock thread
            // drains, and production always has one. It is in this struct
            // rather than beside it because it is one of the fields the
            // joint carries, and a set of six plus a special case is a set
            // somebody eventually forgets to check.
            ticks: Some(crate::ticks::ScheduledTicks::shared()),
            fire_odds: load_fire_odds(root),
        }
    }
}

/// `getStateForPlacement` per block, with the class map the typed rules need.
///
/// The class map is attached here rather than inside `PlacementTable::load`
/// because it is a different file with a different failure mode: without it
/// every table lookup is still right and only the stair corner goes quiet,
/// and that is worth being able to tell apart.
fn load_placement(root: &Path) -> Option<crate::placement::PlacementTable> {
    let placement =
        crate::placement::PlacementTable::load(&root.join("data/vanilla/placement.json"));
    match &placement {
        Ok(t) => eprintln!(
            "placement table: {} blocks, {} items that place one",
            t.len(),
            t.item_count()
        ),
        Err(e) => {
            eprintln!("no placement table ({e}) — every placement takes the block's default state");
        }
    }
    placement.ok().map(|t| {
        match crate::placedby::PlacedByTable::shared(&root.join("data/vanilla/set_placed_by.json"))
        {
            Ok(c) => t.with_classes(c),
            Err(e) => {
                eprintln!("placement table has no class map ({e}) — stairs will not corner");
                t
            }
        }
    })
}

/// The crafting recipes, plus the item tags their ingredients are written in
/// terms of.
///
/// **Both files or neither.** Ingredients are mostly tags — `oak_planks` is
/// `#minecraft:oak_logs` — so recipes without tags would load and then match
/// almost nothing, which looks like a broken recipe engine rather than a
/// missing file.
fn load_recipes(root: &Path) -> Option<crate::recipes::Recipes> {
    let recipes = crate::recipes::Recipes::load(
        &root.join("data/vanilla/recipes.json"),
        &root.join("data/vanilla/tags_item.json"),
    );
    match &recipes {
        Ok(r) => eprintln!(
            "recipe table: {} crafting recipes over {} item tags",
            r.len(),
            r.tags().len()
        ),
        Err(e) => eprintln!("no recipe table ({e}) — a crafting table will open and craft nothing"),
    }
    recipes.ok()
}

/// `updateShape` per state and direction.
///
/// Shared rather than loaded per join: three megabytes of JSON, and this is
/// on the per-connection path. The failure line is printed by
/// [`crate::shapes::shared_with_classes`] itself, inside the `OnceLock`, so
/// that it appears once rather than once per connection; the success line is
/// here because it is the one that carries the gap counts a reader of the
/// console wants beside it.
fn load_shapes(root: &Path) -> Option<&'static crate::shapes::ShapeTable> {
    let shapes = crate::shapes::shared_with_classes(root);
    match shapes {
        Some(t) => {
            let (states, connective, refused) = t.gaps();
            eprintln!(
                "shape table: {} pairs over {states} states \
                 ({connective} connective and {refused} unanswerable, both uncarried)",
                t.len()
            );
        }
        None => eprintln!("no shape table — nothing cascades; torches will float"),
    }
    shapes
}

/// `FireBlock.igniteOdds` and `burnOdds`. Shared rather than loaded per join.
///
/// The console line names the *consequence* rather than the file, like the
/// other six: "no fire odds" tells nobody anything, and the observable is a
/// flint-and-steel that clicks and does nothing.
fn load_fire_odds(root: &Path) -> Option<&'static crate::fire::FireOdds> {
    let odds = crate::fire::FireOdds::shared(&root.join(crate::fire::TABLE_PATH));
    match &odds {
        Ok(t) => eprintln!("fire odds table: {} flammable blocks", t.len()),
        Err(e) => eprintln!(
            "no fire odds table ({e}) — a flint-and-steel lights nothing \
             except on a block with a sturdy top, and fire never spreads"
        ),
    }
    odds.ok()
}

/// `setPlacedBy` per state. Shared rather than loaded per join.
fn load_placed_by(root: &Path) -> Option<&'static crate::placedby::PlacedByTable> {
    let placed_by =
        crate::placedby::PlacedByTable::shared(&root.join("data/vanilla/set_placed_by.json"));
    match &placed_by {
        Ok(t) => eprintln!(
            "setPlacedBy table: {} states write a second block, {} ask for a tick",
            t.len(),
            t.tick_count()
        ),
        Err(e) => eprintln!(
            "no setPlacedBy table ({e}) — a door will be placed with no top \
             and a bed with no head"
        ),
    }
    placed_by.ok()
}

/// `SoundType` per state.
///
/// Silence is the one failure that looks exactly like the feature not
/// existing, which is why the absence gets a line of its own.
fn load_item_props(root: &Path) -> Option<&'static crate::itemprops::ItemProps> {
    let props = crate::itemprops::ItemProps::shared(&root.join(crate::itemprops::TABLE_PATH));
    match &props {
        Ok(t) => eprintln!(
            "item props: {} items, {} of them not stacking to 64, {} damageable, {} tools",
            t.len(),
            t.not_sixty_four(),
            t.damageable(),
            t.tools()
        ),
        Err(e) => eprintln!(
            "no item-props table ({e}) — every item will stack to 64, so a bucket, \
             a sword and a filled bucket all stack like cobblestone"
        ),
    }
    props.ok()
}

fn load_survive(root: &Path) -> Option<&'static crate::itemsurvive::SurviveClasses> {
    let survive =
        crate::itemsurvive::SurviveClasses::shared(&root.join(crate::itemsurvive::TABLE_PATH));
    match &survive {
        Ok(t) => eprintln!(
            "canSurvive classes: {} blocks a BlockItem places, {} of them with a rule",
            t.len(),
            t.with_a_rule()
        ),
        Err(e) => eprintln!(
            "no canSurvive table ({e}) — BlockItem.canPlace's survive gate is not asked, \
             so a torch may be placed against air and a sign hung on nothing"
        ),
    }
    survive.ok()
}

fn load_bonemeal(root: &Path) -> Option<&'static crate::bonemeal::BonemealTable> {
    let bonemeal = crate::bonemeal::BonemealTable::shared(&root.join(crate::bonemeal::TABLE_PATH));
    match &bonemeal {
        // **Both numbers, always.** 63 alone reads as coverage this port does
        // not have and 5 alone hides how much is missing; the pair is the
        // honest statement and is why the boot line prints a ratio.
        Ok(t) => eprintln!(
            "bonemeal: {} blocks are BonemealableBlock, {} of them this server can grow",
            t.len(),
            t.ported()
        ),
        Err(e) => eprintln!(
            "no bonemeal table ({e}) — bone meal is inert; no crop will grow when it is used"
        ),
    }
    bonemeal.ok()
}

fn load_sound_types(root: &Path) -> Option<&'static crate::sound_types::SoundTypes> {
    let sound_types =
        crate::sound_types::SoundTypes::shared(&root.join(crate::sound_types::TABLE_PATH));
    match &sound_types {
        Ok(t) => eprintln!(
            "sound types: {} distinct over {} states",
            t.len(),
            t.states()
        ),
        Err(e) => eprintln!(
            "no sound-type table ({e}) — nothing you place will be heard \
             and nobody's footsteps will carry"
        ),
    }
    sound_types.ok()
}

/// `SoundEvent.fixedRange()` per registry entry.
///
/// The absence here is quieter than the others and deliberately not silent
/// in the log. Without this table every sound is broadcast at
/// `16 * max(volume, 1)` blocks — correct for every event this server
/// currently emits and wrong for whichever loud one it emits next — so the
/// failure is a radius that is too small rather than a world with no sound
/// in it, and nothing downstream would ever surface it.
fn load_sound_events(root: &Path) -> Option<&'static crate::sound_events::SoundEventTable> {
    let sound_events =
        crate::sound_events::SoundEventTable::shared(&root.join(crate::sound_events::TABLE_PATH));
    match &sound_events {
        Ok(t) => eprintln!(
            "sound events: {} registered, {} with a fixed range",
            t.len(),
            t.fixed_range_events()
        ),
        Err(e) => eprintln!(
            "no sound-event table ({e}) — every sound will carry the default \
             16-block radius, including the ones vanilla makes louder"
        ),
    }
    sound_events.ok()
}
