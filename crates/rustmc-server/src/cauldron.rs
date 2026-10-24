//! A bucket against a cauldron — the four `CauldronInteraction.Dispatcher`s'
//! bucket entries, from `CauldronInteractions.bootStrap` and
//! `addDefaultInteractions` on the pinned 26.2 jar.
//!
//! # The map, as `bootStrap` builds it
//!
//! ```text
//!   addDefaultInteractions(d), applied to EMPTY, WATER, LAVA, POWDER_SNOW:
//!     LAVA_BUCKET        -> fillLavaInteraction
//!     WATER_BUCKET       -> fillWaterInteraction
//!     POWDER_SNOW_BUCKET -> fillPowderSnowInteraction
//!   WATER.put(BUCKET, lambda$1)        fillBucket(.., WATER_BUCKET, level == 3, BUCKET_FILL)
//!   LAVA.put(BUCKET, lambda$7)         fillBucket(.., LAVA_BUCKET, true, BUCKET_FILL_LAVA)
//!   POWDER_SNOW.put(BUCKET, lambda$8)  fillBucket(.., POWDER_SNOW_BUCKET, level == 3, ...)
//! ```
//!
//! `fillWaterInteraction` is `emptyBucket(.., WATER_CAULDRON[level=3],
//! BUCKET_EMPTY)`; the lava and powder-snow ones first return `CONSUME` with no
//! write when `isUnderWater` (`level.getFluidState(pos.above()).is(WATER)`),
//! then `emptyBucket(.., LAVA_CAULDRON, BUCKET_EMPTY_LAVA)` /
//! `(.., POWDER_SNOW_CAULDRON[level=3], BUCKET_EMPTY_POWDER_SNOW)`. A water
//! bucket on a water cauldron of any level refills it to 3 — the default
//! interactions are on every dispatcher, including the one already holding
//! water.
//!
//! `emptyBucket` and `fillBucket` both `setBlockAndUpdate` (flags 3) and
//! `level.playSound(null, pos, sound, BLOCKS, 1.0f, 1.0f)` — **`null`, so the
//! user hears it too**, unlike `BucketItem`'s own sounds. A `fillBucket` whose
//! predicate fails is `TRY_WITH_EMPTY_HAND` and writes nothing. Anything not
//! in a dispatcher's map is `CauldronInteraction.DEFAULT`, also
//! `TRY_WITH_EMPTY_HAND`.
//!
//! Only the bucket entries are here. Potions, bottles, dyed items, banners
//! and shulker boxes are other dispatcher entries with item-component
//! machinery this server does not carry.

/// What a cauldron does with the held bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interaction {
    /// The state written with flags 3, or `None` for a `CONSUME` that writes
    /// nothing (lava or powder snow under water).
    pub write: Option<&'static str>,
    /// The sound, at the cauldron, heard by everyone.
    pub sound: Option<&'static str>,
    /// What `createFilledResult` hands back to a player without infinite
    /// materials.
    pub hand_after: &'static str,
}

fn level_of(state: &str) -> Option<u8> {
    crate::placement::property_of(state, "level").and_then(|v| v.parse().ok())
}

/// `AbstractCauldronBlock.useItemOn` for the bucket entries: `None` is
/// `TRY_WITH_EMPTY_HAND`, which the caller passes on to `useWithoutItem`.
///
/// `water_above` is `isUnderWater` — the fluid one block up is in
/// `FluidTags.WATER`.
#[must_use]
pub fn interact(state: &str, item: &str, water_above: bool) -> Option<Interaction> {
    let block = state.split('[').next().unwrap_or(state);
    if !matches!(
        block,
        "minecraft:cauldron"
            | "minecraft:water_cauldron"
            | "minecraft:lava_cauldron"
            | "minecraft:powder_snow_cauldron"
    ) {
        return None;
    }
    let empty = |write, sound| {
        Some(Interaction {
            write: Some(write),
            sound: Some(sound),
            hand_after: "minecraft:bucket",
        })
    };
    let consume = || {
        Some(Interaction {
            write: None,
            sound: None,
            hand_after: "minecraft:bucket",
        })
    };
    match item {
        "minecraft:water_bucket" => empty(
            "minecraft:water_cauldron[level=3]",
            "minecraft:item.bucket.empty",
        ),
        "minecraft:lava_bucket" if water_above => consume(),
        "minecraft:lava_bucket" => empty(
            "minecraft:lava_cauldron",
            "minecraft:item.bucket.empty_lava",
        ),
        "minecraft:powder_snow_bucket" if water_above => consume(),
        "minecraft:powder_snow_bucket" => empty(
            "minecraft:powder_snow_cauldron[level=3]",
            "minecraft:item.bucket.empty_powder_snow",
        ),
        "minecraft:bucket" => {
            let (full, sound) = match block {
                "minecraft:water_cauldron" if level_of(state) == Some(3) => {
                    ("minecraft:water_bucket", "minecraft:item.bucket.fill")
                }
                "minecraft:lava_cauldron" => {
                    ("minecraft:lava_bucket", "minecraft:item.bucket.fill_lava")
                }
                "minecraft:powder_snow_cauldron" if level_of(state) == Some(3) => (
                    "minecraft:powder_snow_bucket",
                    "minecraft:item.bucket.fill_powder_snow",
                ),
                _ => return None,
            };
            Some(Interaction {
                write: Some("minecraft:cauldron"),
                sound: Some(sound),
                hand_after: full,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_water_bucket_fills_any_cauldron_to_three() {
        for from in [
            "minecraft:cauldron",
            "minecraft:water_cauldron[level=1]",
            "minecraft:lava_cauldron",
        ] {
            let i = interact(from, "minecraft:water_bucket", false).expect(from);
            assert_eq!(i.write, Some("minecraft:water_cauldron[level=3]"), "{from}");
            assert_eq!(i.sound, Some("minecraft:item.bucket.empty"));
            assert_eq!(i.hand_after, "minecraft:bucket");
        }
    }

    #[test]
    fn an_empty_bucket_takes_only_a_full_water_cauldron() {
        assert_eq!(
            interact(
                "minecraft:water_cauldron[level=2]",
                "minecraft:bucket",
                false
            ),
            None
        );
        let i = interact(
            "minecraft:water_cauldron[level=3]",
            "minecraft:bucket",
            false,
        )
        .expect("full");
        assert_eq!(i.write, Some("minecraft:cauldron"));
        assert_eq!(i.hand_after, "minecraft:water_bucket");
        assert_eq!(i.sound, Some("minecraft:item.bucket.fill"));
        // An empty cauldron has no BUCKET entry at all.
        assert_eq!(
            interact("minecraft:cauldron", "minecraft:bucket", false),
            None
        );
        // Lava's predicate is `true`.
        let lava = interact("minecraft:lava_cauldron", "minecraft:bucket", false).expect("lava");
        assert_eq!(lava.hand_after, "minecraft:lava_bucket");
    }

    #[test]
    fn lava_under_water_is_consumed_without_a_write() {
        let i = interact("minecraft:cauldron", "minecraft:lava_bucket", true).expect("consume");
        assert_eq!((i.write, i.sound), (None, None));
        // Water does not care what is above it.
        assert!(
            interact("minecraft:cauldron", "minecraft:water_bucket", true)
                .is_some_and(|i| i.write.is_some())
        );
    }

    #[test]
    fn anything_else_is_try_with_empty_hand() {
        assert_eq!(
            interact("minecraft:cauldron", "minecraft:stick", false),
            None
        );
        assert_eq!(
            interact("minecraft:stone", "minecraft:water_bucket", false),
            None
        );
    }
}
