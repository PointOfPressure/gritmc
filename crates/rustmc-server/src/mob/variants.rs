//! The datapack variants a cow and a chicken carry, the same machinery the
//! pig's are drawn with.
//!
//! Reference (26.2): `Cow.defineSynchedData` / `Chicken.defineSynchedData`
//! (`VariantUtils.getDefaultOrAny(registryAccess(), TEMPERATE)`, then
//! `registry.get(CLASSIC).or(registry::getAny).orElseThrow()`),
//! `Cow.finalizeSpawn` / `Chicken.finalizeSpawn` (offsets 0-37:
//! `VariantUtils.selectVariantToSpawn(SpawnContext.create(level,
//! blockPosition()), <KIND>_VARIANT).ifPresent(this::setVariant)`, then
//! `setSoundVariant(<Kind>SoundVariants.pickRandomSoundVariant(
//! registryAccess(), level.getRandom()))`), `CowSoundVariants
//! .pickRandomSoundVariant` / `ChickenSoundVariants.pickRandomSoundVariant`
//! (`Registry.getRandom(random).orElseThrow()` — `nextInt(size)` over `byId`),
//! `Cow.getSoundSet` (`getSoundVariant().value()`), `Chicken.getSoundSet`
//! (`isBaby() ? babySounds() : adultSounds()`), and the two
//! `getBreedOffspring`s (offsets 31-56: one `nextBoolean` on the parent's
//! random, `getVariant()` or the partner's).
//!
//! The selection walk is `PriorityProvider.select`/`pick`, documented on
//! [`super::pig::select_variant_to_spawn`], and it is one function here for
//! all three registries.
use super::host::ServerMob;
use super::{MobVariants, SoundSet, VariantSelector};
use rustmc_entity::host::MobHost;
use rustmc_worldgen::rng::RandomSource;
use std::collections::BTreeMap;

/// One kind's pair of variant registries and the accessors they ride.
#[derive(Debug)]
pub struct Family {
    /// `Registries.<KIND>_VARIANT`.
    pub variant_registry: &'static str,
    /// `Registries.<KIND>_SOUND_VARIANT`.
    pub sound_registry: &'static str,
    /// `DATA_VARIANT_ID`.
    pub variant_accessor: u8,
    /// `DATA_SOUND_VARIANT_ID`.
    pub sound_accessor: u8,
    selectors: fn(&MobVariants) -> &BTreeMap<String, Vec<VariantSelector>>,
    sounds: fn(&MobVariants) -> &BTreeMap<String, SoundSet>,
}

/// `Cow`.
pub static COW: Family = Family {
    variant_registry: "minecraft:cow_variant",
    sound_registry: "minecraft:cow_sound_variant",
    variant_accessor: super::data::id::COW_VARIANT,
    sound_accessor: super::data::id::COW_SOUND_VARIANT,
    selectors: |t| &t.cow_variant,
    sounds: |t| &t.cow_sound_variant,
};

/// `Chicken`.
pub static CHICKEN: Family = Family {
    variant_registry: "minecraft:chicken_variant",
    sound_registry: "minecraft:chicken_sound_variant",
    variant_accessor: super::data::id::CHICKEN_VARIANT,
    sound_accessor: super::data::id::CHICKEN_SOUND_VARIANT,
    selectors: |t| &t.chicken_variant,
    sounds: |t| &t.chicken_sound_variant,
};

/// `(TEMPERATE, CLASSIC)` as registry ids — `defineSynchedData`'s two
/// initial values. `None` when the registries were not captured, which
/// refuses the spawn exactly as the pig's does.
#[must_use]
pub fn defaults(f: &Family) -> Option<(i32, i32)> {
    let v = super::captured_registry(f.variant_registry)
        .iter()
        .position(|n| n == "minecraft:temperate")?;
    let s = super::captured_registry(f.sound_registry)
        .iter()
        .position(|n| n == "minecraft:classic")?;
    Some((i32::try_from(v).ok()?, i32::try_from(s).ok()?))
}

/// `PriorityProvider.select` then `pick` over `ids` (registry order) with
/// `table`'s selectors, at `biome`: the survivors' `nextInt(size)` on the
/// level random — **a draw even when one survives**. `None` when nothing
/// survives.
pub fn select_from(
    ids: &[String],
    table: &BTreeMap<String, Vec<VariantSelector>>,
    biome: &str,
) -> Option<i32> {
    let mut entries: Vec<(usize, i32, Option<&Vec<String>>)> = Vec::new();
    for (idx, name) in ids.iter().enumerate() {
        let Some(sels) = table.get(name) else {
            continue;
        };
        for s in sels {
            entries.push((idx, s.priority, s.biomes.as_ref()));
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.1));
    let mut running = i32::MIN;
    let mut survivors: Vec<usize> = Vec::new();
    for (idx, priority, biomes) in entries {
        if priority < running {
            continue;
        }
        if biomes.is_none_or(|b| b.iter().any(|x| x == biome)) {
            running = priority;
            survivors.push(idx);
        }
    }
    if survivors.is_empty() {
        return None;
    }
    let n = i32::try_from(survivors.len()).ok()?;
    let i = super::level_random(|r| r.next_i32_bound(n));
    i32::try_from(survivors[usize::try_from(i).ok()?]).ok()
}

/// `VariantUtils.selectVariantToSpawn(SpawnContext.create(level,
/// blockPosition()), <KIND>_VARIANT)`.
pub fn select_variant_to_spawn(m: &ServerMob, f: &Family) -> Option<i32> {
    let table = super::mob_variants()?;
    let biome = m.level.biome_at(m.block_position());
    select_from(
        super::captured_registry(f.variant_registry),
        (f.selectors)(table),
        &biome,
    )
}

/// `<Kind>SoundVariants.pickRandomSoundVariant(registryAccess(),
/// level.getRandom())`: `nextInt(size)` over the registry's `byId`.
pub fn pick_random_sound_variant(f: &Family) -> Option<i32> {
    let n = i32::try_from(super::captured_registry(f.sound_registry).len()).ok()?;
    if n == 0 {
        return None;
    }
    Some(super::level_random(|r| r.next_i32_bound(n)))
}

/// The kind's `finalizeSpawn` head: the variant, then the sound variant.
pub fn finalize_spawn(m: &mut ServerMob, f: &Family) {
    if let Some(v) = select_variant_to_spawn(m, f) {
        m.data.set_varint(f.variant_accessor, v);
    }
    if let Some(s) = pick_random_sound_variant(f) {
        m.data.set_varint(f.sound_accessor, s);
    }
}

/// `getSoundSet()` for a cow (`getSoundVariant().value()`) or a chicken
/// (adult or baby half of it).
#[must_use]
pub fn sound_set(m: &ServerMob, f: &Family) -> Option<SoundSet> {
    let id = m.data.get_varint(f.sound_accessor);
    let names = super::captured_registry(f.sound_registry);
    let name = names.get(usize::try_from(id).ok()?)?;
    (f.sounds)(super::mob_variants()?)
        .get(name.as_str())
        .copied()
}

/// The ambient sound: the set's, baby or adult.
#[must_use]
pub fn ambient_sound(m: &ServerMob, f: &Family) -> Option<&'static str> {
    let s = sound_set(m, f)?;
    Some(if m.is_baby() {
        s.baby_ambient
    } else {
        s.ambient
    })
}

/// `playStepSound`'s event.
#[must_use]
pub fn step_sound(m: &ServerMob, f: &Family) -> Option<&'static str> {
    let s = sound_set(m, f)?;
    Some(if m.is_baby() { s.baby_step } else { s.step })
}

/// `getBreedOffspring` 31-56: `random.nextBoolean() ? getVariant() :
/// partner.getVariant()`, on the parent's random.
pub fn inherit_variant(
    parent: &mut ServerMob,
    baby: &mut ServerMob,
    f: &Family,
    partner_variant: Option<i32>,
) {
    let mine = parent.data.get_varint(f.variant_accessor);
    let pick = if parent.random.next_bool() {
        mine
    } else {
        partner_variant.unwrap_or(mine)
    };
    baby.data.set_varint(f.variant_accessor, pick);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(priority: i32, biomes: Option<&[&str]>) -> VariantSelector {
        VariantSelector {
            priority,
            biomes: biomes.map(|b| b.iter().map(|s| (*s).to_string()).collect()),
        }
    }

    /// Plains: only the unconditional `temperate` survives, and the pick is
    /// still made — one level-random draw. A warm biome lifts the running
    /// priority to 1 and drops `temperate` behind it.
    #[test]
    fn the_priority_walk_keeps_the_highest_passing_selectors() {
        let ids: Vec<String> = ["minecraft:cold", "minecraft:temperate", "minecraft:warm"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let mut t = BTreeMap::new();
        t.insert(
            "minecraft:cold".to_string(),
            vec![sel(1, Some(&["minecraft:snowy_plains"]))],
        );
        t.insert("minecraft:temperate".to_string(), vec![sel(0, None)]);
        t.insert(
            "minecraft:warm".to_string(),
            vec![sel(1, Some(&["minecraft:desert"]))],
        );
        assert_eq!(select_from(&ids, &t, "minecraft:plains"), Some(1));
        assert_eq!(select_from(&ids, &t, "minecraft:desert"), Some(2));
        assert_eq!(select_from(&ids, &t, "minecraft:snowy_plains"), Some(0));
        assert_eq!(
            select_from(&ids, &BTreeMap::new(), "minecraft:plains"),
            None
        );
    }
}
