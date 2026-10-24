//! Entity attributes — the read path `LivingEntity.getAttributeValue` takes.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `AttributeMap.getValue`, `AttributeSupplier.getValue`,
//! `AttributeSupplier.getAttributeInstance`, `AttributeInstance.getValue`,
//! `AttributeInstance.calculateValue`, `Attribute.sanitizeValue`,
//! `RangedAttribute.sanitizeValue`, `Attributes.<clinit>`,
//! `LivingEntity.createLivingAttributes`, `Player.createAttributes`.
//!
//! # Why this exists, given that nothing here has a potion
//!
//! Because `Entity.maxUpStep()` is an attribute read, and
//! [`crate::collide::collide`] already takes it as an argument:
//!
//! ```text
//!   Entity.maxUpStep        -> fconst_0
//!   LivingEntity.maxUpStep  -> (float) getAttributeValue(Attributes.STEP_HEIGHT)
//! ```
//!
//! so the 0.6 a player auto-steps by is `STEP_HEIGHT`'s default, clamped to
//! `STEP_HEIGHT`'s range, and not a constant. `LivingEntity.getAirDrag()` and
//! `LivingEntity.getEntityBounciness()` are two more reads on the same path
//! — `AIR_DRAG_MODIFIER` and `BOUNCINESS`.
//!
//! **What is NOT here is gravity for the two kinds that fall.**
//! `ItemEntity.getDefaultGravity()` and `FallingBlockEntity.getDefaultGravity()`
//! are each `ldc2_w 0.04d` — a literal in the bytecode, on an entity type that
//! `DefaultAttributes` has no supplier for at all. Only `LivingEntity` reads
//! `Attributes.GRAVITY`, and this server has no living entity whose movement
//! it simulates (see `docs/intel/movement-tick-three-checks.md` for why a
//! `ServerPlayer` never reaches `travel`). So the gravity limb of this module
//! is **live but unexercised**, and that is written down rather than left to
//! be discovered: a test that reads a player's gravity through this path is in
//! this file, and no runtime caller is.
//!
//! # The numbers are data, not source
//!
//! `data/vanilla/attributes.json` — `ci/attributes-golden.sh` — carries every
//! attribute's default and range and every entity type's base map, read off the
//! jar. Nothing in this file is a transcribed number. In particular `0.08`
//! does not appear in it, and neither does `0.6`.
//!
//! Every double in that table is carried twice, readable and as raw IEEE-754
//! bits, and **the bits are what is read**. `Attributes.GRAVITY`'s default is
//! the `double` literal `0.08d`; a JSON round trip through a shortest-repr
//! printer is exact only if every stage is correctly rounded, and this way the
//! question does not arise. Same argument `collision_shapes.json` uses for its
//! coordinate lists.
//!
//! # An absent attribute is an error, not a default
//!
//! `AttributeSupplier.getAttributeInstance` **throws
//! `IllegalArgumentException`** for an attribute the type does not carry, and
//! `AttributeMap.getValue` reaches it whenever the instance map has no entry.
//! So "this type does not have this attribute" is a crash in vanilla, not a
//! fallback to the attribute's own default — which is exactly the distinction a
//! reimplementation gets wrong by writing `unwrap_or(spec.default)` and making
//! every type silently carry every attribute. [`Attributes::value`] returns
//! `None` there and the caller has to say what it means.

use std::collections::BTreeMap;
use std::io;

/// Where the extract lives, relative to the server's working directory.
///
/// **This literal is load-bearing outside Rust.** `ci/gen-gameplay-tables.sh`
/// derives the set of tables a tree needs by grepping
/// `crates/rustmc-server/src` for `"data/vanilla/<name>.json"`. Spelling it any
/// other way makes the gate stop regenerating it and it goes stale in every
/// tree at once.
pub const TABLE_PATH: &str = "data/vanilla/attributes.json";

/// `AttributeModifier$Operation`, in the order `calculateValue` applies them.
///
/// The order is the whole content of the enum: `ADD_VALUE` folds into the base
/// before either multiplier is read, `ADD_MULTIPLIED_BASE` multiplies **that
/// sum** rather than the running total, and `ADD_MULTIPLIED_TOTAL` compounds.
/// Two modifiers of different operations do not commute, so an implementation
/// that summed them in one pass would agree with vanilla only when at most one
/// was present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operation {
    /// `v += amount`
    AddValue,
    /// `total += base_sum * amount`
    AddMultipliedBase,
    /// `total *= 1 + amount`
    AddMultipliedTotal,
}

/// One `AttributeModifier`.
///
/// `id` is vanilla's `Identifier` key. It is carried because vanilla's
/// modifier map is keyed by it — a second modifier with the same id replaces
/// the first rather than stacking — and a `Vec` that ignored that would double
/// an effect that vanilla refreshes.
impl Operation {
    /// `AttributeModifier.Operation.id()`: the wire ordinal `Operation.STREAM_CODEC`
    /// writes — `ADD_VALUE` 0, `ADD_MULTIPLIED_BASE` 1, `ADD_MULTIPLIED_TOTAL` 2.
    #[must_use]
    pub const fn ordinal(self) -> i32 {
        match self {
            Self::AddValue => 0,
            Self::AddMultipliedBase => 1,
            Self::AddMultipliedTotal => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Modifier {
    pub id: String,
    pub amount: f64,
    pub op: Operation,
}

/// One attribute's registry entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// `Attribute.getDefaultValue()` — the base a supplier uses when it adds
    /// the attribute without naming a value.
    pub default: f64,
    /// `RangedAttribute`'s `[minValue, maxValue]`, or `None` for a plain
    /// `Attribute`, whose `sanitizeValue` is the identity.
    ///
    /// Carried as an `Option` rather than as `[-inf, inf]` because the two are
    /// not the same for NaN: `RangedAttribute.sanitizeValue` maps NaN to
    /// `minValue`, and the identity does not.
    pub range: Option<(f64, f64)>,
    /// `Attribute.isClientSyncable()`: whether an instance of it is sent
    /// in `ClientboundUpdateAttributesPacket`. Set by `setSyncable(true)`
    /// in `Attributes.<clinit>` for 32 of the 40; `FOLLOW_RANGE` and
    /// `TEMPT_RANGE` are among the eight that stay server-side.
    pub syncable: bool,
}

impl Spec {
    /// `Attribute.sanitizeValue` / `RangedAttribute.sanitizeValue`.
    ///
    /// ```text
    ///   RangedAttribute:  Double.isNaN(v) ? minValue : Mth.clamp(v, minValue, maxValue)
    ///   Attribute:        v
    /// ```
    ///
    /// `Mth.clamp(double, double, double)` is `v < min ? min : (v > max ? max : v)`,
    /// which is **not** `f64::clamp`: `f64::clamp` panics when `min > max` and
    /// propagates NaN, and vanilla's does neither. The NaN case is handled
    /// above it here for the same reason it is there.
    #[must_use]
    pub fn sanitize(&self, v: f64) -> f64 {
        let Some((min, max)) = self.range else {
            return v;
        };
        if v.is_nan() {
            return min;
        }
        if v < min {
            min
        } else if v > max {
            max
        } else {
            v
        }
    }
}

/// The attribute registry and the per-entity-type base maps.
#[derive(Debug, Clone)]
pub struct Attributes {
    specs: BTreeMap<String, Spec>,
    /// entity type id -> attribute id -> base value. A type absent from this
    /// map has no `AttributeSupplier` at all, which is a fact about the type
    /// and not a gap in the table: `DefaultAttributes.SUPPLIERS` is keyed by
    /// `EntityType<? extends LivingEntity>`, so an item, a falling block and
    /// an arrow are all legitimately absent.
    suppliers: BTreeMap<String, BTreeMap<String, f64>>,
}

/// `Attributes.STEP_HEIGHT`'s registry id, for the one caller that exists.
pub const STEP_HEIGHT: &str = "minecraft:step_height";
/// `Attributes.GRAVITY`'s registry id.
pub const GRAVITY: &str = "minecraft:gravity";
/// `Attributes.AIR_DRAG_MODIFIER`'s registry id.
pub const AIR_DRAG_MODIFIER: &str = "minecraft:air_drag_modifier";
/// `Attributes.BOUNCINESS`'s registry id.
pub const BOUNCINESS: &str = "minecraft:bounciness";
/// `Attributes.MAX_HEALTH`'s registry id.
///
/// Read rather than written down as `20.0` because that is a `float` a
/// synched accessor puts on the wire byte-for-byte:
/// `LivingEntity.<init>` ends with `setHealth(getMaxHealth())`, and
/// `getMaxHealth()` is `(float) getAttributeValue(MAX_HEALTH)`. A constant
/// here would be a second place for the number to live, and the table it
/// would be duplicating is already a required input to this server.
pub const MAX_HEALTH: &str = "minecraft:max_health";

fn bits(v: Option<&serde_json::Value>) -> Option<f64> {
    #[allow(clippy::cast_sign_loss)]
    v.and_then(serde_json::Value::as_i64)
        .map(|b| f64::from_bits(b as u64))
}

fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("attributes.json: {what}"),
    )
}

impl Attributes {
    /// Load from [`TABLE_PATH`], resolved relative to the process's cwd and,
    /// failing that, relative to `CARGO_MANIFEST_DIR` — which is what makes
    /// this reachable under `cargo test` (whose cwd is the package
    /// directory, not the repo root). Same shape and the same carried risk
    /// as `crate::protocol_ids::shared()`: `docs/PARITY-ROADMAP.md` §34.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn load_default() -> io::Result<Self> {
        let cwd_relative = std::path::Path::new(TABLE_PATH);
        let path: std::path::PathBuf = if cwd_relative.exists() {
            cwd_relative.to_path_buf()
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(TABLE_PATH)
        };
        Self::load(&path)
    }

    /// Load from an explicit path.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| bad(&format!("not json: {e}")))?;
        Self::from_json(&doc)
    }

    /// Parse the dumper's output.
    ///
    /// # Errors
    /// If a row is missing a field this reader needs.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let mut specs = BTreeMap::new();
        for row in doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `table` array"))?
        {
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a row with no `id`"))?;
            let default =
                bits(row.get("defaultBits")).ok_or_else(|| bad("a row with no `defaultBits`"))?;
            // `ranged` decides whether a range is expected, rather than the
            // presence of the fields deciding it. A row that says it is ranged
            // and carries no bounds is a broken dump and must not silently
            // become an unclamped attribute.
            let range = if row.get("ranged").and_then(serde_json::Value::as_bool) == Some(true) {
                let min = bits(row.get("minBits"))
                    .ok_or_else(|| bad(&format!("{id} is ranged with no `minBits`")))?;
                let max = bits(row.get("maxBits"))
                    .ok_or_else(|| bad(&format!("{id} is ranged with no `maxBits`")))?;
                Some((min, max))
            } else {
                None
            };
            let syncable = row
                .get("syncable")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| bad(&format!("{id} has no `syncable`")))?;
            specs.insert(
                id.to_string(),
                Spec {
                    default,
                    range,
                    syncable,
                },
            );
        }

        let mut suppliers: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        for row in doc
            .get("suppliers")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `suppliers` array"))?
        {
            let ty = row
                .get("type")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a supplier with no `type`"))?;
            let mut base = BTreeMap::new();
            for b in row
                .get("base")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| bad(&format!("{ty} has no `base` array")))?
            {
                let id = b
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| bad(&format!("{ty} has a base entry with no `id`")))?;
                let v = bits(b.get("baseBits"))
                    .ok_or_else(|| bad(&format!("{ty}/{id} has no `baseBits`")))?;
                base.insert(id.to_string(), v);
            }
            suppliers.insert(ty.to_string(), base);
        }

        if specs.is_empty() || suppliers.is_empty() {
            return Err(bad(
                "empty — a table with no rows would make every read None",
            ));
        }
        Ok(Self { specs, suppliers })
    }

    /// The registry entry for an attribute id.
    #[must_use]
    pub fn spec(&self, attribute: &str) -> Option<&Spec> {
        self.specs.get(attribute)
    }

    /// Whether an entity type has an `AttributeSupplier` at all.
    ///
    /// `false` is `DefaultAttributes.hasSupplier(type)` being false, which is
    /// every type that is not a `LivingEntity`. It is the honest answer to
    /// "what is this item's step height": the question has no answer, and
    /// vanilla would throw.
    #[must_use]
    pub fn has_supplier(&self, entity_type: &str) -> bool {
        self.suppliers.contains_key(entity_type)
    }

    /// `AttributeSupplier.getBaseValue` — the base before any modifier.
    #[must_use]
    pub fn base(&self, entity_type: &str, attribute: &str) -> Option<f64> {
        self.suppliers.get(entity_type)?.get(attribute).copied()
    }

    /// `LivingEntity.getAttributeValue(attribute)` for an entity of this type
    /// carrying these modifiers.
    ///
    /// `None` when the type has no supplier, or has one that does not carry the
    /// attribute — both of which are the `IllegalArgumentException` arm of
    /// `AttributeSupplier.getAttributeInstance`. **Not a default**; see the
    /// module docs.
    ///
    /// The arithmetic is `AttributeInstance.calculateValue`, offsets 0-165:
    ///
    /// ```text
    ///   v = baseValue
    ///   for m in ADD_VALUE:             v += m.amount
    ///   t = v
    ///   for m in ADD_MULTIPLIED_BASE:   t += v * m.amount
    ///   for m in ADD_MULTIPLIED_TOTAL:  t *= 1 + m.amount
    ///   return attribute.sanitizeValue(t)
    /// ```
    ///
    /// Note `t += v * m.amount` — **`v`, the post-`ADD_VALUE` base, not the
    /// running `t`.** Reading `t` there is the single easiest way to get this
    /// wrong and it agrees with vanilla for zero or one modifier of that
    /// operation, which is every case anyone tests by hand.
    #[must_use]
    pub fn value(&self, entity_type: &str, attribute: &str, modifiers: &[Modifier]) -> Option<f64> {
        let base = self.base(entity_type, attribute)?;
        let spec = self.specs.get(attribute)?;
        let mut v = base;
        for m in modifiers.iter().filter(|m| m.op == Operation::AddValue) {
            v += m.amount;
        }
        let mut t = v;
        for m in modifiers
            .iter()
            .filter(|m| m.op == Operation::AddMultipliedBase)
        {
            t += v * m.amount;
        }
        for m in modifiers
            .iter()
            .filter(|m| m.op == Operation::AddMultipliedTotal)
        {
            t *= 1.0 + m.amount;
        }
        Some(spec.sanitize(t))
    }

    /// How many attributes and how many suppliers, for the loader's own log
    /// line. A table that parsed to nothing reads exactly like one that parsed
    /// correctly unless somebody prints the counts.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.specs.len(), self.suppliers.len())
    }

    /// The process-wide table, loaded once.
    ///
    /// `None` when the extract is absent, which is a checkout with no vanilla
    /// data rather than an error worth killing the server for — the caller
    /// decides. Every caller in this crate treats `None` as "do not simulate",
    /// which is a refusal rather than a guess.
    pub fn shared() -> Option<&'static Self> {
        static TABLE: std::sync::OnceLock<Option<Attributes>> = std::sync::OnceLock::new();
        TABLE.get_or_init(|| Self::load_default().ok()).as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table small enough to reason about, in the dumper's own shape.
    fn table() -> Attributes {
        let doc = serde_json::json!({
            "table": [
                {"id": "minecraft:gravity", "ranged": true, "syncable": true,
                 "defaultBits": 0.08_f64.to_bits() as i64,
                 "minBits": (-1.0_f64).to_bits() as i64,
                 "maxBits": 1.0_f64.to_bits() as i64},
                {"id": "minecraft:step_height", "ranged": true, "syncable": true,
                 "defaultBits": 0.6_f64.to_bits() as i64,
                 "minBits": 0.0_f64.to_bits() as i64,
                 "maxBits": 10.0_f64.to_bits() as i64},
                {"id": "minecraft:unranged", "ranged": false, "syncable": false,
                 "defaultBits": 3.0_f64.to_bits() as i64}
            ],
            "suppliers": [
                {"type": "minecraft:player", "base": [
                    {"id": "minecraft:gravity", "baseBits": 0.08_f64.to_bits() as i64},
                    {"id": "minecraft:step_height", "baseBits": 0.6_f64.to_bits() as i64},
                    {"id": "minecraft:unranged", "baseBits": 3.0_f64.to_bits() as i64}
                ]}
            ]
        });
        Attributes::from_json(&doc).expect("the fixture parses")
    }

    #[test]
    fn an_unmodified_attribute_is_its_base() {
        let a = table();
        assert_eq!(a.value("minecraft:player", GRAVITY, &[]), Some(0.08));
        assert_eq!(a.value("minecraft:player", STEP_HEIGHT, &[]), Some(0.6));
    }

    /// The bits path, stated as a test rather than as a comment: `0.08` read
    /// through raw bits is the same double the bytecode holds, and a reader
    /// that went via a decimal string would have to be shown not to move it.
    #[test]
    fn the_default_is_the_double_the_bytecode_holds() {
        let a = table();
        assert_eq!(
            a.spec(GRAVITY).expect("gravity").default.to_bits(),
            0.08_f64.to_bits()
        );
    }

    /// A type with no supplier has no value — vanilla throws here, and the
    /// tempting `unwrap_or(default)` would give a falling block a step height.
    #[test]
    fn a_type_without_a_supplier_has_no_attributes() {
        let a = table();
        assert!(!a.has_supplier("minecraft:item"));
        assert_eq!(a.value("minecraft:item", GRAVITY, &[]), None);
        assert_eq!(a.value("minecraft:falling_block", STEP_HEIGHT, &[]), None);
    }

    /// An attribute the supplier does not carry is the same arm.
    #[test]
    fn an_attribute_the_type_does_not_carry_has_no_value() {
        let a = table();
        assert_eq!(a.value("minecraft:player", "minecraft:luck", &[]), None);
    }

    #[test]
    fn add_value_modifiers_sum_into_the_base() {
        let a = table();
        let m = |amount| Modifier {
            id: "test".into(),
            amount,
            op: Operation::AddValue,
        };
        assert_eq!(
            a.value("minecraft:player", STEP_HEIGHT, &[m(0.5), m(0.25)]),
            Some(0.6 + 0.5 + 0.25)
        );
    }

    /// The line the module docs single out. Two `ADD_MULTIPLIED_BASE`
    /// modifiers must each multiply the **post-ADD_VALUE base**, not the
    /// running total, so 3 with two +1.0 modifiers is 3 + 3 + 3 = 9 and not
    /// 3 -> 6 -> 12.
    #[test]
    fn multiplied_base_multiplies_the_base_and_not_the_running_total() {
        let a = table();
        let m = |amount| Modifier {
            id: "test".into(),
            amount,
            op: Operation::AddMultipliedBase,
        };
        assert_eq!(
            a.value("minecraft:player", "minecraft:unranged", &[m(1.0), m(1.0)]),
            Some(9.0)
        );
    }

    /// `ADD_MULTIPLIED_TOTAL` compounds, which is the difference from the one
    /// above and the reason both exist.
    #[test]
    fn multiplied_total_compounds() {
        let a = table();
        let m = |amount| Modifier {
            id: "test".into(),
            amount,
            op: Operation::AddMultipliedTotal,
        };
        assert_eq!(
            a.value("minecraft:player", "minecraft:unranged", &[m(1.0), m(1.0)]),
            Some(12.0)
        );
    }

    /// The three operations are applied in vanilla's order and do not commute.
    /// `(3 + 1) + (3 + 1) * 1 = 8`, then `* 2 = 16`; any other order gives
    /// something else.
    #[test]
    fn the_three_operations_are_applied_in_vanillas_order() {
        let a = table();
        let ms = vec![
            Modifier {
                id: "c".into(),
                amount: 1.0,
                op: Operation::AddMultipliedTotal,
            },
            Modifier {
                id: "a".into(),
                amount: 1.0,
                op: Operation::AddValue,
            },
            Modifier {
                id: "b".into(),
                amount: 1.0,
                op: Operation::AddMultipliedBase,
            },
        ];
        assert_eq!(
            a.value("minecraft:player", "minecraft:unranged", &ms),
            Some(16.0)
        );
    }

    /// The clamp is the attribute's, not the caller's. Gravity's range is
    /// [-1, 1], so a modifier that would take it past 1 stops at 1.
    #[test]
    fn a_ranged_attribute_clamps_and_an_unranged_one_does_not() {
        let a = table();
        let m = |amount| Modifier {
            id: "test".into(),
            amount,
            op: Operation::AddValue,
        };
        assert_eq!(a.value("minecraft:player", GRAVITY, &[m(5.0)]), Some(1.0));
        assert_eq!(a.value("minecraft:player", GRAVITY, &[m(-5.0)]), Some(-1.0));
        assert_eq!(
            a.value("minecraft:player", "minecraft:unranged", &[m(5.0)]),
            Some(8.0)
        );
    }

    /// `RangedAttribute.sanitizeValue` maps NaN to **minValue**, which is the
    /// one case where the clamp is not a clamp. `f64::clamp` would propagate
    /// the NaN and `Mth.clamp`'s comparison chain would return the NaN too, so
    /// this is a branch vanilla writes before either of them.
    #[test]
    fn nan_sanitizes_to_the_minimum_and_not_to_nan() {
        let a = table();
        let m = |amount| Modifier {
            id: "test".into(),
            amount,
            op: Operation::AddValue,
        };
        assert_eq!(
            a.value("minecraft:player", GRAVITY, &[m(f64::NAN)]),
            Some(-1.0)
        );
        // Unranged: sanitizeValue is the identity, so the NaN survives.
        assert!(a
            .value("minecraft:player", "minecraft:unranged", &[m(f64::NAN)])
            .expect("a value")
            .is_nan());
    }

    /// A table that parsed to nothing must be an error rather than a table in
    /// which every read is `None` — the latter is indistinguishable at the
    /// call site from "this entity has no attributes", which is a real answer.
    #[test]
    fn an_empty_table_is_refused() {
        let doc = serde_json::json!({"table": [], "suppliers": []});
        assert!(Attributes::from_json(&doc).is_err());
    }

    /// A row claiming a range without carrying one is a broken dump. Accepting
    /// it would produce an unclamped attribute that behaves correctly until
    /// something exceeds the range nobody loaded.
    #[test]
    fn a_ranged_row_with_no_bounds_is_refused() {
        let doc = serde_json::json!({
            "table": [{"id": "a", "ranged": true, "defaultBits": 0}],
            "suppliers": [{"type": "t", "base": []}]
        });
        assert!(Attributes::from_json(&doc).is_err());
    }
}
