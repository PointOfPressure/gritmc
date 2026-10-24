//! **The rest of `docs/PARITY-ROADMAP.md` §33's census.** §33 (branch
//! `server-protocol-ids-path`) found six more loaders sharing
//! `crate::protocol_ids::shared()`'s exact fault: a zero-argument
//! `shared()`/`load_default()` that resolves `data/vanilla/<table>.json` (or,
//! for [`rustmc_server::ops`], `ops.json` beside the server jar) relative to
//! the process's **working directory**, with no override. Every `cargo test`
//! binary in this workspace runs with its own package's `CARGO_MANIFEST_DIR`
//! as that directory, not the repo root, so every one of these five failed
//! the cwd-relative lookup on **every** test run regardless of what was on
//! disk — a correct table and a missing one produced the identical `None`.
//! `docs/PARITY-ROADMAP.md` §34 names the fix and the risk it carries, once,
//! for the set: this file is the witness that each of the five now resolves.
//!
//! The sixth, `collide::CollisionShapes::load_default`, is not here —
//! `docs/PARITY-ROADMAP.md` §34 explains why it was deleted rather than
//! fixed.
//!
//! # This is reachability, not behaviour
//!
//! Every assertion below cross-checks the zero-argument loader's answer
//! against an **independent** read of the same generated file (or, for
//! `ops`, an independently written one) — never against the loader agreeing
//! with itself — and none of them changes what any loader's data means.
//!
//! # Method: injected and witnessed
//!
//! Each of the five `shared()`/`load_default()` bodies was reverted in place
//! to its pre-fix, single-cwd-path form and this file re-run. What was
//! watched, per function, is in `docs/PARITY-ROADMAP.md` §34 and in the
//! commit that lands this file; the shape is the same one
//! `protocol_ids_reachable.rs` used: `No such file or directory (os error
//! 2)` from the loader's own error line, and the cross-check `.expect`
//! naming which one.
//!
//! Skipped without the relevant table on disk, like every other
//! golden-backed test in this crate — except [`ops_level_for_resolves_a_real_
//! file_beside_the_repo_root`], which manufactures its own fixture rather
//! than depending on the generated set, and refuses rather than clobbers if
//! a real `ops.json` is already sitting at the path it needs.

use rustmc_server::attributes::{Attributes, MAX_HEALTH};
use rustmc_server::entities::{self, ITEM};
use rustmc_server::movement_props::MovementProps;
use rustmc_server::power;
use rustmc_server::power::SignalTable;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The function this item exists to unlock, called exactly as its one real
/// caller (`world_send::spawn_and_announce_thrown_item`) calls it: no path,
/// no override.
#[test]
fn entities_shared_kind_ids_resolves_under_the_cargo_test_working_directory() {
    let path = repo("data/vanilla/entity_types.json");
    if !path.exists() {
        eprintln!("skipped: no data/vanilla/entity_types.json — run ci/gen-gameplay-tables.sh");
        return;
    }
    let ids = entities::shared_kind_ids().expect(
        "entity kind ids unreachable under cargo test's own working directory — \
         crate::entities::shared_kind_ids()'s cwd-relative lookup, and its \
         CARGO_MANIFEST_DIR fallback, both failed",
    );
    let text = std::fs::read_to_string(&path).expect("read");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
    let want = doc["entity_types"]["minecraft:item"]
        .as_i64()
        .expect("minecraft:item has an entry");
    assert_eq!(
        ids.get(ITEM),
        Some(i32::try_from(want).expect("fits in i32")),
        "a wrong id here decodes cleanly on the wire and announces a different entity"
    );
}

/// The function `crate::shapes::set_block`'s production `on_place` and
/// `update_neighbors_at` calls resolve on every write. Cross-checked against
/// `gaps()` — carried/refused/quiet counts — rather than a single lookup, so
/// a table that loaded but lost rows would still be caught.
#[test]
fn power_shared_resolves_under_the_cargo_test_working_directory() {
    let path = repo("data/vanilla/signal.json");
    if !path.exists() {
        eprintln!("skipped: no data/vanilla/signal.json — run ci/gen-gameplay-tables.sh");
        return;
    }
    let live = power::shared().expect(
        "signal table unreachable under cargo test's own working directory — \
         crate::power::shared()'s cwd-relative lookup, and its \
         CARGO_MANIFEST_DIR fallback, both failed",
    );
    let independent = SignalTable::load(&path).expect("an explicit-path load of the same file");
    assert_eq!(
        live.gaps(),
        independent.gaps(),
        "power::shared() answered a different table from the one on disk"
    );
    assert!(
        live.is_signal_source("minecraft:redstone_block"),
        "a redstone block that is not a signal source means nothing was really loaded"
    );
}

/// `status::joining_health`'s only table. Cross-checked against the base
/// `minecraft:max_health` a fresh player joins with, which is not a made-up
/// probe: it is the one value `joining_health()` itself reads.
#[test]
fn attributes_shared_resolves_under_the_cargo_test_working_directory() {
    let path = repo("data/vanilla/attributes.json");
    if !path.exists() {
        eprintln!("skipped: no data/vanilla/attributes.json — run ci/gen-gameplay-tables.sh");
        return;
    }
    let live = Attributes::shared().expect(
        "attributes table unreachable under cargo test's own working directory — \
         crate::attributes::Attributes::shared()'s cwd-relative lookup, and its \
         CARGO_MANIFEST_DIR fallback, both failed",
    );
    let independent = Attributes::load(&path).expect("an explicit-path load of the same file");
    assert_eq!(
        live.base("minecraft:player", MAX_HEALTH),
        independent.base("minecraft:player", MAX_HEALTH),
        "attributes::shared() answered a different base health from the one on disk"
    );
    assert!(
        live.base("minecraft:player", MAX_HEALTH).is_some(),
        "no base health at all means nothing was really loaded"
    );
}

/// Currently uncalled by anything in the crate (`docs/PARITY-ROADMAP.md`
/// §33) — witnessed anyway, on the same footing as the other four, because
/// the fault is in path resolution and does not care whether a caller exists
/// yet.
#[test]
fn movement_props_shared_resolves_under_the_cargo_test_working_directory() {
    let path = repo("data/vanilla/movement_props.json");
    if !path.exists() {
        eprintln!("skipped: no data/vanilla/movement_props.json — run ci/gen-gameplay-tables.sh");
        return;
    }
    let live = MovementProps::shared().expect(
        "movement props unreachable under cargo test's own working directory — \
         crate::movement_props::MovementProps::shared()'s cwd-relative lookup, \
         and its CARGO_MANIFEST_DIR fallback, both failed",
    );
    let independent = MovementProps::load(&path).expect("an explicit-path load of the same file");
    assert_eq!(
        live.counts(),
        independent.counts(),
        "movement_props::shared() answered a different table from the one on disk"
    );
    let ice = live
        .by_block("minecraft:ice")
        .expect("ice has a movement-props row");
    assert!(
        (ice.friction - 0.98).abs() < 0.001,
        "ice's friction ({}) is not the 0.98 vanilla gives it — wrong row or wrong file",
        ice.friction
    );
}

/// **`ops::shared()` has no generated table to fall back to** — `ops.json`
/// sits beside a real server's jar and is never checked in — so this is the
/// one test in the file that builds its own fixture rather than depending on
/// `ci/gen-gameplay-tables.sh`.
///
/// The cwd-relative branch (unchanged by this item) resolves to
/// `crates/rustmc-server/ops.json` under `cargo test`, which this test never
/// writes, so the loader can only find the fixture through the new
/// `CARGO_MANIFEST_DIR`-anchored fallback — the same divergence that made
/// `protocol_ids_reachable.rs` fail against the pre-fix body: the old,
/// single-path `shared()` never tries this second location at all.
///
/// **Refuses rather than clobbers.** If a real `ops.json` is already sitting
/// at the repo root — nothing in this tree should put one there, but a
/// human running a real server from a checkout might — the fixture is never
/// written and the test is skipped, on the same principle `ci/t2-gameplay.sh`
/// uses for a table it will not overwrite.
#[test]
fn ops_level_for_resolves_a_real_file_beside_the_repo_root() {
    let fixture_path = repo("ops.json");
    if fixture_path.exists() {
        eprintln!(
            "skipped: {} already exists — not overwriting a real ops list",
            fixture_path.display()
        );
        return;
    }

    struct CleanUp(std::path::PathBuf);
    impl Drop for CleanUp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    std::fs::write(
        &fixture_path,
        r#"[{"uuid":"00000000-0000-0000-0000-00000000002a",
             "name":"loader_paths_reachable_fixture","level":3,
             "bypassesPlayerLimit":false}]"#,
    )
    .expect("write the fixture ops.json beside the repo root");
    let _cleanup = CleanUp(fixture_path);

    assert_eq!(
        rustmc_server::ops::level_for("loader_paths_reachable_fixture"),
        rustmc_server::commandtree::Level::Admins,
        "ops::level_for() did not see the fixture at all under cargo test's \
         own working directory — the cwd-relative lookup (unchanged) cannot \
         reach a repo-root ops.json from crates/rustmc-server, and the \
         CARGO_MANIFEST_DIR fallback is what has to carry it"
    );
    // A name not in the fixture is level 0, same as vanilla's `PlayerList`
    // for anyone not on the list — proves the table was really read rather
    // than a hard-coded level answering everything.
    assert_eq!(
        rustmc_server::ops::level_for("nobody_in_the_fixture"),
        rustmc_server::commandtree::Level::All
    );
}
