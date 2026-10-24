//! The order in `data/vanilla/enchantments.json` against a real server's wire.
//!
//! # The defect this exists to catch, which was live for one commit
//!
//! `EnchantmentMenu.slotsChanged` writes
//! `registryAccess.lookupOrThrow(ENCHANTMENT).asHolderIdMap().getId(holder)`
//! into `enchantClue[i]`, and that array is a `DataSlot` — it goes to the
//! client as a `container_set_data`. So the numeric id of every enchantment is
//! observable, and getting the order wrong puts a wrong id on the wire for all
//! 43 at once.
//!
//! `tools/goldengen/vanilla/EnchantmentGolden.java`'s first answer was
//! `VanillaRegistries.createLookup().lookupOrThrow(ENCHANTMENT).listElements()`
//! — the *bootstrap's* registration order, which begins
//! `thorns, unbreaking, swift_sneak, fire_protection`. A live 26.2 server's
//! `registry_data` packet begins `aqua_affinity, bane_of_arthropods,
//! binding_curse`: a datapack registry is populated by `RegistryDataLoader`
//! walking the resource manager's listing, and that listing is a sorted map.
//!
//! Two plausible orders, one right, and no other check in this tree looks at
//! either. So this test compares the table against
//! `data/vanilla/registries.bin` — bytes captured from a real vanilla server
//! by `ci/dump-registries.sh`, on a route that shares no code with the
//! dumper — and fails naming the first position that differs.
//!
//! # It can go red, and here is what turns it red
//!
//! Deleting the `Collections.sort(order)` in the dumper. Reordering any two
//! rows in the table by hand. A registry capture from a version whose
//! enchantment set changed. All three are position-level differences and all
//! three are named rather than counted.

use rustmc_server::configuration::CapturedRegistries;
use rustmc_server::enchantment::{Registry, TABLE_PATH};

fn repo(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

#[test]
fn the_table_order_is_the_order_a_real_server_sent() {
    let table = repo(TABLE_PATH);
    let caps_path = repo("data/vanilla/registries.bin");
    let mut ok = rustmc_data::fixture::available(
        &table,
        "data/vanilla/enchantments.json",
        "ci/enchantments-golden.sh",
    );
    ok &= rustmc_data::fixture::available(
        &caps_path,
        "data/vanilla/registries.bin",
        "ci/dump-registries.sh",
    );
    if !ok {
        return;
    }

    let table = Registry::load(&table).expect("enchantments.json");
    let caps = CapturedRegistries::load(&caps_path).expect("registries.bin");
    let wire = caps
        .entry_ids("minecraft:enchantment")
        .expect("registry capture")
        .expect("the capture holds minecraft:enchantment");

    assert_eq!(
        wire.len(),
        table.len(),
        "the capture holds {} enchantments, the table {}",
        wire.len(),
        table.len()
    );

    // Named, not counted: "3 rows differ" sends the reader back to the file to
    // find out which, and the first differing position is the whole diagnosis
    // when a sort has been dropped.
    for (i, (want, got)) in wire.iter().zip(table.order()).enumerate() {
        assert_eq!(
            want, got,
            "enchantment id {i}: a real server sent `{want}`, the table has `{got}`"
        );
    }

    // And the id lookup the menu actually calls, on both ends of the range —
    // an off-by-one in `numeric_id` would pass the loop above.
    assert_eq!(table.numeric_id(&wire[0]), Some(0));
    let last = i32::try_from(wire.len() - 1).expect("43 fits");
    assert_eq!(table.numeric_id(&wire[wire.len() - 1]), Some(last));
    assert_eq!(table.numeric_id("minecraft:not_an_enchantment"), None);
}

/// The positive control for the test above: a *wrong* order must fail the
/// same comparison. Without this, a `zip` over an empty table would pass the
/// loop silently and the test would be measuring nothing.
#[test]
fn a_reordered_table_would_fail_that_comparison() {
    let wire = ["a", "b", "c"];
    let good = ["a", "b", "c"];
    let bad = ["a", "c", "b"];
    assert!(wire.iter().zip(good).all(|(w, g)| *w == g));
    assert!(!wire.iter().zip(bad).all(|(w, g)| *w == g));
    // And the shape that made the loop vacuous: a shorter list.
    let short: [&str; 0] = [];
    assert!(wire.iter().zip(short).all(|(w, g)| *w == g));
    assert_ne!(
        wire.len(),
        short.len(),
        "which is why the length is asserted first"
    );
}
