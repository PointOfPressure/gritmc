//! The entities generation puts in a structure — a woodland mansion's
//! evokers, vindicators and allays, an ocean monument's elder guardians, an
//! end city's shulkers and its ship's elytra frame, a mineshaft's chest
//! minecarts, a swamp hut's witch and black cat, an ocean ruin's drowned —
//! taken off the generated
//! canvas and added to the level, the way `WorldGenRegion.addFreshEntity`
//! hands them to the chunk.
//!
//! The compounds are worldgen's (`rustmc_worldgen::structure::spawned`),
//! checked against vanilla's saved `entities/` files: every mansion and
//! monument mob key for key but `UUID`, and every shulker but its unseeded
//! yaw. This module turns one into a level entity: its kind, position,
//! rotation, UUID, and the synched data a client is sent — a mob's health,
//! the frame's facing and its elytra.
//!
//! # What these entities do not do
//!
//! **They have no AI and no mob slot.** This server ticks behaviour for the
//! kinds in `crate::mob::KINDS`, and an evoker, vindicator, allay, elder
//! guardian or shulker is not one of them. They are tracked, sent to every
//! client in range with the right type, position and data, and stand where
//! vanilla put them; they do not move, attack, take damage or despawn. The
//! frame holds its elytra and cannot be emptied; a chest minecart keeps its
//! loot table and seed in the saved compound but cannot be opened.

use rustmc_world::nbt::Nbt;

use crate::entities::{Entities, Entity, EntityKind};

/// The level kind for a generated entity's id, when it is one of ours.
fn kind_of(id: &str) -> Option<EntityKind> {
    Some(match id {
        "minecraft:evoker" => crate::entities::EVOKER,
        "minecraft:vindicator" => crate::entities::VINDICATOR,
        "minecraft:allay" => crate::entities::ALLAY,
        "minecraft:elder_guardian" => crate::entities::ELDER_GUARDIAN,
        "minecraft:shulker" => crate::entities::SHULKER,
        "minecraft:item_frame" => crate::entities::ITEM_FRAME,
        "minecraft:chest_minecart" => crate::entities::CHEST_MINECART,
        "minecraft:witch" => crate::entities::WITCH,
        "minecraft:cat" => crate::entities::CAT,
        "minecraft:drowned" => crate::entities::DROWNED,
        _ => return None,
    })
}

fn doubles(n: &Nbt, key: &str) -> Option<Vec<f64>> {
    match n.get(key)? {
        Nbt::List(l) => l
            .iter()
            .map(|v| match v {
                Nbt::Double(d) => Some(*d),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn floats(n: &Nbt, key: &str) -> Option<Vec<f32>> {
    match n.get(key)? {
        Nbt::List(l) => l
            .iter()
            .map(|v| match v {
                Nbt::Float(f) => Some(*f),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// The elytra a ship's frame holds, as `DATA_ITEM` carries it.
fn elytra_bytes() -> Option<Vec<u8>> {
    static ITEMS: std::sync::OnceLock<Option<crate::player::ItemIds>> = std::sync::OnceLock::new();
    let items = ITEMS
        .get_or_init(|| {
            for path in [
                std::path::PathBuf::from("data/vanilla/items.json"),
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../data/vanilla/items.json"),
            ] {
                if let Ok(ids) = crate::player::ItemIds::load(&path) {
                    return Some(ids);
                }
            }
            None
        })
        .as_ref()?;
    let id = items.id_of("minecraft:elytra")?;
    Some(crate::menu::item_stack_bytes(Some(
        crate::menu::WireStack::plain(id, 1),
    )))
}

/// One generated compound as a level entity, with a fresh network id.
#[must_use]
pub fn entity_from(compound: &Nbt, network_id: i32) -> Option<Entity> {
    let Nbt::String(id) = compound.get("id")? else {
        return None;
    };
    let kind = kind_of(id)?;
    let pos = doubles(compound, "Pos")?;
    let rotation = floats(compound, "Rotation").unwrap_or_else(|| vec![0.0, 0.0]);
    let uuid = match compound.get("UUID")? {
        Nbt::IntArray(v) if v.len() == 4 => v
            .iter()
            .fold(0u128, |acc, &w| (acc << 32) | u128::from(w.cast_unsigned())),
        _ => return None,
    };
    let (data, synched) = if kind == crate::entities::ITEM_FRAME {
        let facing = match compound.get("Facing") {
            Some(Nbt::Byte(f)) => u8::try_from(*f).unwrap_or(3),
            _ => 3,
        };
        let item = match compound.get("Item") {
            Some(_) => elytra_bytes(),
            None => None,
        };
        (
            i32::from(facing),
            crate::synched::Synched::ItemFrame {
                direction: facing,
                item,
            },
        )
    } else if kind == crate::entities::CHEST_MINECART {
        // Every `AbstractMinecart` accessor is at its definition for a cart
        // nothing has hurt or shaken.
        (0, crate::synched::Synched::EntityOnly)
    } else {
        let health = match compound.get("Health") {
            Some(Nbt::Float(h)) => *h,
            _ => 1.0,
        };
        (0, crate::synched::Synched::Living { health })
    };
    Some(Entity {
        id: network_id,
        uuid,
        kind,
        x: *pos.first()?,
        y: *pos.get(1)?,
        z: *pos.get(2)?,
        y_rot: *rotation.first()?,
        x_rot: *rotation.get(1)?,
        y_head_rot: *rotation.first()?,
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched,
    })
}

/// Add every generated structure entity in `chunks` to the level and to the
/// tracker. An entity whose UUID is already in the level is skipped, so a
/// tile generated a second time does not double its garrison.
///
/// Returns how many were added.
pub fn adopt_generated(
    canvas: &rustmc_worldgen::canvas::Canvas,
    chunks: impl Iterator<Item = (i32, i32)>,
    entities: &Entities,
) -> usize {
    let present: std::collections::BTreeSet<u128> = entities.all().iter().map(|e| e.uuid).collect();
    let mut added = 0;
    for (cx, cz) in chunks {
        for compound in canvas.entity_records(cx, cz) {
            let Some(e) = entity_from(compound, 0) else {
                continue;
            };
            if present.contains(&e.uuid) {
                continue;
            }
            let e = Entity {
                id: entities.next_id(),
                ..e
            };
            // Pairing sends the non-default synched values with the
            // `add_entity`; nothing is dirty for a fresh entity.
            entities.insert(e.clone());
            if let Some(kinds) = crate::entities::shared_kind_ids() {
                crate::tracker::Tracker::shared().add(&e, &[], kinds);
            }
            added += 1;
        }
    }
    added
}
