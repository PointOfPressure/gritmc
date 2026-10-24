//! `FuelValues` — how long each item burns, from the jar's own map.
//!
//! Read by `AbstractFurnaceBlockEntity.serverTick` (offsets 207-217, where
//! the answer becomes both `litTimeRemaining` and `litTotalTime`) and by its
//! `canPlaceItem` (offsets 24-35, which decides what the fuel slot and a
//! hopper feeding it will accept).
//!
//! The table is `data/vanilla/fuel_values.json`, produced by
//! `ci/fuel-values-golden.sh` from `FuelValues.vanillaBurnTimes`. It is not
//! transcribed because there is nothing to transcribe: `FuelValues.Builder`
//! is fed from item **tags** as well as from individual items, so writing the
//! builder out by hand would mean expanding `ItemTags.PLANKS` by hand, and an
//! item missing from a hand list is not a wrong burn time — it is a furnace
//! that refuses fuel.
//!
//! # An absent item and an item mapped to zero are different
//!
//! `Object2IntSortedMap.getInt` answers the map's default, `0`, for a key it
//! does not hold. So `burnDuration` answers 0 for a stone and would answer 0
//! for a fuel explicitly mapped to zero, and only `isFuel` — a bare
//! `containsKey` — can tell them apart. [`FuelValues::burn_duration`] and
//! [`FuelValues::is_fuel`] keep that distinction for the same reason, even
//! though the shipped table has **no** zero-duration entries (the golden
//! counts them and says so).
//!
//! `isFuel` has **no** emptiness test, which is worth knowing rather than
//! rounding off: `ItemStack.EMPTY.getItem()` is `Items.AIR` and the answer
//! comes out false because air is not in the map, not because the stack was
//! empty. `burnDuration` does have one, at its offsets 0-8.

use crate::item::ItemStack;
use std::collections::BTreeMap;
use std::path::Path;

/// Where the golden writes this table.
///
/// The literal lives here rather than at the call site because
/// `ci/gen-gameplay-tables.sh` **derives** the set of tables a tree needs by
/// grepping `crates/` and `tools/` for exactly this string shape. A table
/// named only by a `format!` or by joining a directory to a basename is
/// invisible to that grep, and the failure is not a missing file: the gate
/// stamps and regenerates the tables it can see, so an unseen one keeps
/// whichever tree's copy the setup's `cp -rn` left behind.
pub const DEFAULT_PATH: &str = "data/vanilla/fuel_values.json";

/// The vanilla fuel map.
#[derive(Debug, Default)]
pub struct FuelValues {
    values: BTreeMap<String, i32>,
}

/// The one table [`FuelValues::shared`] loads and [`FuelValues::loaded`]
/// reads.
static SHARED: std::sync::OnceLock<Result<FuelValues, String>> = std::sync::OnceLock::new();

impl FuelValues {
    /// Parse `data/vanilla/fuel_values.json`.
    ///
    /// # Errors
    /// If the file is missing, is not JSON, or carries no `table`.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let doc: serde_json::Value =
            serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))
                .map_err(std::io::Error::other)?;
        Self::parse(&doc)
    }

    /// Parse the golden's document.
    ///
    /// # Errors
    /// If `table` is absent or empty. An empty map is refused rather than
    /// accepted: every `burn_duration` would answer 0 and every `is_fuel`
    /// false, which is a furnace that never lights and a table that cannot
    /// be told from a missing one.
    pub fn parse(doc: &serde_json::Value) -> std::io::Result<Self> {
        let mut values = BTreeMap::new();
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| std::io::Error::other("fuel_values.json has no table"))?;
        for row in rows {
            let (Some(item), Some(d)) = (
                row.get("item").and_then(serde_json::Value::as_str),
                row.get("burnDuration").and_then(serde_json::Value::as_i64),
            ) else {
                continue;
            };
            values.insert(item.to_string(), i32::try_from(d).unwrap_or(i32::MAX));
        }
        if values.is_empty() {
            return Err(std::io::Error::other(
                "fuel_values.json carries no fuels — a furnace with this table never lights",
            ));
        }
        Ok(Self { values })
    }

    /// The one table this process serves, loaded once.
    ///
    /// # Errors
    /// Whatever [`Self::load`] returned the first time, repeated.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        SHARED
            .get_or_init(|| Self::load(path).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The table [`Self::shared`] loaded, if anything has loaded it — and
    /// **never a load of its own**.
    ///
    /// For the readers that have no path to hand and must not pick one: the
    /// furnace menu's `FurnaceFuelSlot.mayPlace` and `quickMoveStack` ask
    /// `Level.fuelValues()`, which is one table per server, and the clock
    /// thread loads exactly that table at startup for the furnace ticker. A
    /// click reading a second copy could disagree with the ticker about what
    /// burns; reading this one cannot. `None` before the load and in a unit
    /// test, which a fuel slot answers by accepting nothing but a bucket —
    /// the refusal direction.
    #[must_use]
    pub fn loaded() -> Option<&'static Self> {
        SHARED.get().and_then(|r| r.as_ref().ok())
    }

    /// `FuelValues.burnDuration(ItemStack)` — offsets 0-22.
    ///
    /// The `isEmpty` short-circuit is first and returns a literal 0, so an
    /// empty stack never reaches the map.
    #[must_use]
    pub fn burn_duration(&self, stack: &ItemStack) -> i32 {
        if stack.is_empty() {
            return 0;
        }
        self.values.get(stack.item()).copied().unwrap_or(0)
    }

    /// `FuelValues.isFuel(ItemStack)` — offsets 0-13, a bare `containsKey`.
    #[must_use]
    pub fn is_fuel(&self, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            // Vanilla asks the map for `Items.AIR`, which is not in it. The
            // early return is the same answer by a shorter road and is
            // written out so nobody reads its absence as a bug.
            return false;
        }
        self.values.contains_key(stack.item())
    }

    /// How many items the map holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the map is empty. Always false for a table [`Self::parse`]
    /// accepted; present because clippy asks for it beside `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl crate::hopper::Rules for FuelValues {
    fn is_fuel(&self, stack: &ItemStack) -> bool {
        Self::is_fuel(self, stack)
    }

    /// **Not answered here.** `PotionBrewing.isIngredient` is a registry of
    /// its own and no table for it has been taken, so a hopper over a brewing
    /// stand will not fill the ingredient slot. False is the refusal; see
    /// [`crate::hopper::NoRules`].
    fn is_brewing_ingredient(&self, _stack: &ItemStack) -> bool {
        false
    }

    /// **Not answered here** — the `brewing_fuel` item tag is in
    /// `data/vanilla/item_tags.json` and nothing routes it to this trait yet.
    fn is_brewing_fuel(&self, _stack: &ItemStack) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hopper::{ContainerView, NoRules, Worldly};

    fn table() -> FuelValues {
        FuelValues::parse(&serde_json::json!({
            "table": [
                {"item": "minecraft:coal", "burnDuration": 1600},
                {"item": "minecraft:stick", "burnDuration": 100},
                {"item": "minecraft:zero_fuel", "burnDuration": 0},
            ]
        }))
        .expect("three rows parse")
    }

    /// The distinction the whole module exists for: a zero-duration fuel is
    /// still a fuel, and an item absent from the map is not.
    #[test]
    fn a_zero_duration_fuel_is_a_fuel_and_an_absent_item_is_not() {
        let f = table();
        let zero = ItemStack::one("minecraft:zero_fuel");
        assert_eq!(f.burn_duration(&zero), 0);
        assert!(f.is_fuel(&zero), "a mapped zero is still a fuel");
        let stone = ItemStack::one("minecraft:stone");
        assert_eq!(f.burn_duration(&stone), 0);
        assert!(!f.is_fuel(&stone), "an absent item is not a fuel");
        // Positive control: something in the map with a real duration, so a
        // table that had failed to load would not pass the two asserts above
        // by answering 0/false to everything.
        assert_eq!(f.burn_duration(&ItemStack::one("minecraft:coal")), 1600);
        assert!(f.is_fuel(&ItemStack::one("minecraft:coal")));
    }

    /// An empty stack is 0 and not a fuel, by two different routes in vanilla
    /// and both of them false here.
    #[test]
    fn an_empty_stack_burns_for_nothing_and_is_not_a_fuel() {
        let f = table();
        assert_eq!(f.burn_duration(&ItemStack::empty()), 0);
        assert!(!f.is_fuel(&ItemStack::empty()));
    }

    /// An empty table is refused rather than served. A `FuelValues` that
    /// answered 0/false to everything is indistinguishable from a furnace
    /// with no fuel table at all, which is the shape of a green that cannot
    /// go red.
    #[test]
    fn a_table_with_no_fuels_is_refused() {
        assert!(FuelValues::parse(&serde_json::json!({"table": []})).is_err());
        assert!(FuelValues::parse(&serde_json::json!({})).is_err());
    }

    /// With a real table the hopper's furnace rule stops refusing coal — the
    /// same call that [`NoRules`] declines. This is the join, and it is what
    /// makes `NoRules` a refusal rather than a claim.
    #[test]
    fn a_loaded_table_lets_a_hopper_fuel_a_furnace_where_norules_will_not() {
        let mut furnace = ContainerView::empty(3);
        furnace.worldly = Worldly::Furnace;
        let coal = ItemStack::one("minecraft:coal");
        assert!(!furnace.can_place_item(1, &coal, &NoRules));
        assert!(furnace.can_place_item(1, &coal, &table()));
        // And the result slot stays shut whatever the table says.
        assert!(!furnace.can_place_item(2, &coal, &table()));
    }
}
