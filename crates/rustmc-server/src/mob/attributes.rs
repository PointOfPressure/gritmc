//! `AttributeMap` and `AttributeInstance` as a living entity carries them,
//! and `ClientboundUpdateAttributesPacket`.
//!
//! Reference (26.2): `AttributeMap.<init>`, `getInstance`, `getValue`,
//! `onAttributeModified`, `getSyncableAttributes`, `getAttributesToSync`,
//! `getAttributesToUpdate`; `AttributeSupplier.createInstance`;
//! `AttributeInstance.<init>`, `replaceFrom`, `setDirty`, `addModifier`,
//! `addPermanentModifier`, `hasModifier`, `getValue`, `calculateValue`;
//! `ClientboundUpdateAttributesPacket.<init>(int, Collection)` and its
//! `STREAM_CODEC`; `ServerEntity.sendPairingData` / `sendDirtyEntityData`.
//!
//! # An instance exists only once something asked for it
//!
//! `AttributeMap.attributes` starts **empty**. `getValue(holder)` reads the
//! supplier's base when there is no instance and creates nothing;
//! `getInstance(holder)` (`LivingEntity.getAttribute`) is the one call that
//! creates one, through `computeIfAbsent` → `supplier.createInstance` →
//! `new AttributeInstance(...)` + `replaceFrom(template)` — and `replaceFrom`
//! ends in `setDirty()`, which reports the instance to the map's
//! `attributesToUpdate` and, if the attribute `isClientSyncable()`, to
//! `attributesToSync`.
//!
//! **That is why a freshly spawned pig's pairing bundle carries no
//! `update_attributes`, and its second tick does.** `LivingEntity.<init>`
//! reads `getMaxHealth()` through `getValue` (no instance);
//! `Mob.finalizeSpawn` creates `FOLLOW_RANGE` (not syncable); the first
//! `aiStep` reaches `removeFrost()` → `getAttribute(MOVEMENT_SPEED)`, which
//! creates a syncable instance, and the next `sendChanges` →
//! `sendDirtyEntityData` sends it. `ci/checks/gameplay/081-*` grades that
//! packet.
//!
//! # Order
//!
//! `attributes` is an `Object2ObjectOpenHashMap<Holder<Attribute>, …>` keyed
//! by a `Holder.Reference`, whose `hashCode` is `Object`'s identity hash, so
//! the walk order of `getSyncableAttributes` is a property of the JVM run.
//! `attributesToSync` is an `ObjectOpenHashSet` with the same key. The
//! recordings agree with each other 120/120 on this packet because a fresh
//! mob has **one** syncable instance when it goes out; a mob with two would
//! be order-unstable in vanilla itself. Here instances are kept in creation
//! order, which is a deterministic member of the set of orders vanilla can
//! produce.
use crate::attributes::{Attributes, Modifier, Operation};
use rustmc_proto::varint::write_varint;

/// `AttributeInstance`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeInstance {
    /// The attribute's registry id.
    pub attribute: &'static str,
    /// `baseValue`.
    pub base: f64,
    /// `modifierById`, in insertion order (`Object2ObjectArrayMap`).
    pub modifiers: Vec<Modifier>,
    /// `permanentModifiers`' ids.
    pub permanent: Vec<String>,
}

impl AttributeInstance {
    /// `hasModifier(Identifier)`.
    #[must_use]
    pub fn has_modifier(&self, id: &str) -> bool {
        self.modifiers.iter().any(|m| m.id == id)
    }

    /// `getModifiers()`: an `ImmutableSet.copyOf(modifierById.values())`,
    /// which keeps the array map's insertion order.
    #[must_use]
    pub fn modifier_values(&self) -> Vec<&Modifier> {
        self.modifiers.iter().collect()
    }
}

/// `AttributeMap`, plus the supplier it was built over.
#[derive(Debug, Clone)]
pub struct MobAttributes {
    entity_type: &'static str,
    table: &'static Attributes,
    /// `attributes`, in creation order.
    instances: Vec<AttributeInstance>,
    /// `attributesToSync`, as indices into `instances`, in insertion order.
    to_sync: Vec<usize>,
    /// `attributesToUpdate`, likewise.
    to_update: Vec<usize>,
}

impl MobAttributes {
    /// `new AttributeMap(DefaultAttributes.getSupplier(type))`, or `None`
    /// when the type has no supplier — `DefaultAttributes.getSupplier` throws
    /// for those.
    #[must_use]
    pub fn new(table: &'static Attributes, entity_type: &'static str) -> Option<Self> {
        if !table.has_supplier(entity_type) {
            return None;
        }
        Some(Self {
            entity_type,
            table,
            instances: Vec::new(),
            to_sync: Vec::new(),
            to_update: Vec::new(),
        })
    }

    fn index_of(&self, attribute: &str) -> Option<usize> {
        self.instances.iter().position(|i| i.attribute == attribute)
    }

    /// `getValue(holder)`: the instance's value when one exists, else
    /// `supplier.getValue(holder)`. `None` is the supplier's
    /// `IllegalArgumentException` for an attribute the type lacks.
    #[must_use]
    pub fn value(&self, attribute: &str) -> Option<f64> {
        match self.index_of(attribute) {
            Some(i) => {
                let inst = &self.instances[i];
                Some(self.value_of(inst.base, &inst.modifiers, attribute))
            }
            None => self.table.value(self.entity_type, attribute, &[]),
        }
    }

    /// `getBaseValue(holder)`.
    #[must_use]
    pub fn base_value(&self, attribute: &str) -> Option<f64> {
        match self.index_of(attribute) {
            Some(i) => Some(self.instances[i].base),
            None => self.table.base(self.entity_type, attribute),
        }
    }

    /// `AttributeInstance.calculateValue` + `sanitizeValue`, over a base and
    /// a modifier list — the table's arithmetic, on this instance's base.
    fn value_of(&self, base: f64, modifiers: &[Modifier], attribute: &str) -> f64 {
        let Some(spec) = self.table.spec(attribute) else {
            return base;
        };
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
        spec.sanitize(t)
    }

    /// `getInstance(holder)` — `LivingEntity.getAttribute`: the instance,
    /// created from the supplier's template on first use. Creation runs
    /// `replaceFrom`, whose last statement is `setDirty()`, so a freshly
    /// created instance is already in `attributesToUpdate` and (when
    /// syncable) `attributesToSync`. `None` when the type lacks it.
    pub fn instance(&mut self, attribute: &'static str) -> Option<&mut AttributeInstance> {
        if let Some(i) = self.index_of(attribute) {
            return self.instances.get_mut(i);
        }
        let base = self.table.base(self.entity_type, attribute)?;
        self.instances.push(AttributeInstance {
            attribute,
            base,
            modifiers: Vec::new(),
            permanent: Vec::new(),
        });
        let i = self.instances.len() - 1;
        self.on_dirty(i);
        self.instances.get_mut(i)
    }

    /// `AttributeInstance.setDirty` → `onDirty.accept(this)` →
    /// `AttributeMap.onAttributeModified`.
    fn on_dirty(&mut self, i: usize) {
        if !self.to_update.contains(&i) {
            self.to_update.push(i);
        }
        let syncable = self
            .table
            .spec(self.instances[i].attribute)
            .is_some_and(|s| s.syncable);
        if syncable && !self.to_sync.contains(&i) {
            self.to_sync.push(i);
        }
    }

    /// `getAttribute(holder).addPermanentModifier(modifier)`:
    /// `addModifier` (refusing a duplicate id, then `setDirty`) and the
    /// permanent map.
    ///
    /// # Panics
    /// On a modifier id already applied — vanilla's `IllegalArgumentException`.
    pub fn add_permanent_modifier(&mut self, attribute: &'static str, modifier: Modifier) {
        let Some(i) = self
            .instance(attribute)
            .map(|_| ())
            .and(self.index_of(attribute))
        else {
            return;
        };
        assert!(
            !self.instances[i].has_modifier(&modifier.id),
            "Modifier is already applied on this attribute!"
        );
        let id = modifier.id.clone();
        self.instances[i].modifiers.push(modifier);
        self.on_dirty(i);
        self.instances[i].permanent.push(id);
    }

    /// `getAttributesToSync()` drained: the instances a `sendDirtyEntityData`
    /// puts in one `update_attributes`, in the set's order, then
    /// `Set.clear()`.
    pub fn take_to_sync(&mut self) -> Vec<AttributeInstance> {
        let out = self
            .to_sync
            .iter()
            .map(|&i| self.instances[i].clone())
            .collect();
        self.to_sync.clear();
        out
    }

    /// `getAttributesToUpdate()` drained, for `LivingEntity.refreshDirtyAttributes`.
    pub fn take_to_update(&mut self) -> Vec<&'static str> {
        let out = self
            .to_update
            .iter()
            .map(|&i| self.instances[i].attribute)
            .collect();
        self.to_update.clear();
        out
    }

    /// `getSyncableAttributes()`: every instance whose attribute is
    /// syncable, for the pairing bundle.
    #[must_use]
    pub fn syncable(&self) -> Vec<AttributeInstance> {
        self.instances
            .iter()
            .filter(|i| self.table.spec(i.attribute).is_some_and(|s| s.syncable))
            .cloned()
            .collect()
    }

    /// The instances so far, in creation order.
    #[must_use]
    pub fn instances(&self) -> &[AttributeInstance] {
        &self.instances
    }
}

/// `ClientboundUpdateAttributesPacket`'s body:
///
/// ```text
///   varint  entity id
///   varint  count
///   per attribute:
///     varint  Registries.ATTRIBUTE id           (holderRegistry, no +1)
///     double  base
///     varint  modifier count
///     per modifier:
///       string  id (Identifier)
///       double  amount
///       varint  operation ordinal
/// ```
///
/// `AttributeSnapshot.STREAM_CODEC` is `composite(Attribute.STREAM_CODEC,
/// DOUBLE, MODIFIER_STREAM_CODEC.apply(collection))`, and
/// `MODIFIER_STREAM_CODEC` is `composite(Identifier.STREAM_CODEC, DOUBLE,
/// Operation.STREAM_CODEC)`; the packet is `composite(VAR_INT,
/// AttributeSnapshot.STREAM_CODEC.apply(list))`.
///
/// # Errors
/// If an attribute has no registry id in `ids`, which names a stale
/// `protocol_ids.json` rather than a wire fault.
pub fn update_attributes_body(
    ids: &crate::protocol_ids::ProtocolIds,
    entity_id: i32,
    attributes: &[AttributeInstance],
) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(32);
    write_varint(&mut b, entity_id)?;
    write_varint(
        &mut b,
        i32::try_from(attributes.len()).map_err(std::io::Error::other)?,
    )?;
    for a in attributes {
        let id = ids.attribute(a.attribute).ok_or_else(|| {
            std::io::Error::other(format!("no protocol id for attribute {}", a.attribute))
        })?;
        write_varint(&mut b, id)?;
        b.extend_from_slice(&a.base.to_be_bytes());
        write_varint(
            &mut b,
            i32::try_from(a.modifiers.len()).map_err(std::io::Error::other)?,
        )?;
        for m in &a.modifiers {
            rustmc_proto::wire::write_string(&mut b, &m.id)?;
            b.extend_from_slice(&m.amount.to_be_bytes());
            write_varint(&mut b, m.op.ordinal())?;
        }
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> &'static Attributes {
        let doc = serde_json::json!({
            "table": [
                {"id": "minecraft:max_health", "ranged": true, "syncable": true,
                 "defaultBits": 20.0_f64.to_bits() as i64,
                 "minBits": 1.0_f64.to_bits() as i64,
                 "maxBits": 1024.0_f64.to_bits() as i64},
                {"id": "minecraft:movement_speed", "ranged": true, "syncable": true,
                 "defaultBits": 0.7_f64.to_bits() as i64,
                 "minBits": 0.0_f64.to_bits() as i64,
                 "maxBits": 1024.0_f64.to_bits() as i64},
                {"id": "minecraft:follow_range", "ranged": true, "syncable": false,
                 "defaultBits": 32.0_f64.to_bits() as i64,
                 "minBits": 0.0_f64.to_bits() as i64,
                 "maxBits": 2048.0_f64.to_bits() as i64}
            ],
            "suppliers": [
                {"type": "minecraft:pig", "base": [
                    {"id": "minecraft:max_health", "baseBits": 10.0_f64.to_bits() as i64},
                    {"id": "minecraft:movement_speed", "baseBits": 0.25_f64.to_bits() as i64},
                    {"id": "minecraft:follow_range", "baseBits": 16.0_f64.to_bits() as i64}
                ]}
            ]
        });
        Box::leak(Box::new(Attributes::from_json(&doc).expect("fixture")))
    }

    #[test]
    fn a_value_read_creates_no_instance_and_an_instance_read_does() {
        let mut m = MobAttributes::new(table(), "minecraft:pig").expect("pig has a supplier");
        assert_eq!(m.value("minecraft:max_health"), Some(10.0));
        assert!(m.instances().is_empty());
        assert!(m.syncable().is_empty());
        m.instance("minecraft:follow_range")
            .expect("pig has follow range");
        assert_eq!(m.instances().len(), 1);
        // Created dirty, reported to update, not to sync: not syncable.
        assert_eq!(m.take_to_update(), vec!["minecraft:follow_range"]);
        assert!(m.take_to_sync().is_empty());
        m.instance("minecraft:movement_speed")
            .expect("pig has speed");
        let sync = m.take_to_sync();
        assert_eq!(sync.len(), 1);
        assert_eq!(sync[0].attribute, "minecraft:movement_speed");
        assert_eq!(sync[0].base, 0.25);
        // A second creation of the same instance is the map hit, no report.
        m.instance("minecraft:movement_speed");
        assert!(m.take_to_sync().is_empty());
        assert_eq!(m.syncable().len(), 1);
    }

    #[test]
    fn a_permanent_modifier_changes_the_value_and_refuses_a_duplicate() {
        let mut m = MobAttributes::new(table(), "minecraft:pig").expect("pig");
        m.add_permanent_modifier(
            "minecraft:follow_range",
            Modifier {
                id: "minecraft:random_spawn_bonus".to_string(),
                amount: 0.5,
                op: Operation::AddMultipliedBase,
            },
        );
        assert_eq!(m.value("minecraft:follow_range"), Some(24.0));
        assert!(m.instances()[0].has_modifier("minecraft:random_spawn_bonus"));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut m2 = m.clone();
            m2.add_permanent_modifier(
                "minecraft:follow_range",
                Modifier {
                    id: "minecraft:random_spawn_bonus".to_string(),
                    amount: 0.1,
                    op: Operation::AddValue,
                },
            );
        }));
        assert!(r.is_err(), "a duplicate modifier id is refused");
    }

    #[test]
    fn the_packet_is_id_count_then_attribute_base_and_modifiers() {
        let dir = std::env::temp_dir().join(format!("rustmc-mob-attr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let path = dir.join("protocol_ids.json");
        std::fs::write(
            &path,
            r#"{"menus":{},"blocks":{},"block_entity_types":{},"sounds":{},
                "attributes":{"minecraft:movement_speed":26}}"#,
        )
        .expect("write");
        let ids = crate::protocol_ids::ProtocolIds::load(&path).expect("load");
        let inst = AttributeInstance {
            attribute: "minecraft:movement_speed",
            base: 0.25,
            modifiers: Vec::new(),
            permanent: Vec::new(),
        };
        let body = update_attributes_body(&ids, 7, &[inst]).expect("body");
        let mut want = vec![7, 1, 26];
        want.extend_from_slice(&0.25_f64.to_be_bytes());
        want.push(0);
        assert_eq!(body, want);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
