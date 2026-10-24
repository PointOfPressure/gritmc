//! `BlockItem.place` and the overrides that change what it writes.
//!
//! # Why a module rather than more lines in `world_send`
//!
//! `BlockItem` is **938 of the 1,537 items** in 26.2 and its family —
//! `DoubleHighBlockItem` 28, `StandingAndWallBlockItem` 20, `BedItem` 16,
//! `BannerItem` 16, `SignItem` 12, `HangingSignItem` 12,
//! `GameMasterBlockItem` 7, `PlaceOnWaterBlockItem` 2, `ScaffoldingBlockItem`
//! 1, `SolidBucketItem` 1 — is 1,051 (class column of
//! `data/vanilla/item_props.json`). Most of what those subclasses do is
//! override one step of `place`, and the server's placement path is a single
//! 300-line arm of `handle_use_item_on`. Adding each override inline is how
//! that arm stops being readable and how each override stops being testable:
//! reaching one from a test means standing up a `Session`, a world, a
//! placement table and a connection.
//!
//! So each override lands here as a **pure function of what vanilla reads**,
//! the way `crate::itemuse` holds `ItemStack.useOn` and `crate::placement`
//! holds `getStateForPlacement`. The caller stays one `if let`.
//!
//! # What is here
//!
//! [`clear_above`] — `DoubleHighBlockItem.placeBlock`, the whole of it.
//! [`places_from_use_on`] — the two subclasses that refuse to place at all.
//! [`place_flags`] — `BedItem.placeBlock`'s 26 against everyone else's 11.
//!
//! Nothing else yet. `StandingAndWallBlockItem`, `BedItem` and `BannerItem`
//! are named above so the next agent has the counts rather than a search.

use crate::interact::BlockPos;

/// `bipush 27` at `DoubleHighBlockItem.placeBlock` offset 45.
///
/// `UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_IMMEDIATE | UPDATE_KNOWN_SHAPE`
/// — carried as the number the jar has rather than reassembled from names,
/// the rule `crate::itemuse::SET_BLOCK_FLAGS` follows for the same reason.
///
/// **`UPDATE_KNOWN_SHAPE` is set and the placement's own `UPDATE_ALL` does
/// not have it**, so this write starts no shape cascade. That is not
/// incidental: the thing being cleared is usually a plant, and a cascade from
/// it would ask its neighbours to update before the door is down.
pub const CLEAR_FLAGS: i32 = 27;

/// What `DoubleHighBlockItem.placeBlock` writes before the lower half.
///
/// ```text
///     5-12   above = ctx.getClickedPos().above()
///    14-38   fill = level.isWaterAt(above) ? Blocks.WATER.defaultBlockState()
///                                          : Blocks.AIR.defaultBlockState()
///    40-50   level.setBlock(above, fill, 27)
///    51-57   return super.placeBlock(ctx, state)
/// ```
///
/// That is the entire override. 28 items reach it — every door and every tall
/// flower — and it is a **class** dispatch: no data component says an item is
/// a `DoubleHighBlockItem`, which is why
/// [`crate::itemprops::ItemProps::is_double_high`] reads the class column
/// this branch's golden added.
///
/// # The world ends the same and the wire does not
///
/// `BlockItem.canPlace` has already required the position above to be
/// replaceable, and `setPlacedBy` then writes the upper half over whatever is
/// there. So omitting this write loses no block — it loses the
/// `block_update` to air that vanilla sends **first**. The gameplay corpus
/// compares packets, so that is a divergence rather than a cosmetic detail,
/// and it is invisible to any test that only reads the finished world.
///
/// # Returns the block, not the state
///
/// `Blocks.WATER.defaultBlockState()` and `Blocks.AIR.defaultBlockState()`,
/// so the caller resolves the default through its own block table rather than
/// this function spelling a state string that `block_props.json` would have
/// to agree with. Note it is the **default** water state and not
/// `createLegacyBlock()` of what was there: a flowing level is replaced by a
/// source.
///
/// `None` means the item is not a `DoubleHighBlockItem` and `place` runs
/// unchanged.
#[must_use]
pub fn clear_above(
    props: Option<&crate::itemprops::ItemProps>,
    item: &str,
    target: BlockPos,
    water_above: bool,
) -> Option<(BlockPos, &'static str)> {
    if !props?.is_double_high(item) {
        return None;
    }
    let block = if water_above {
        "minecraft:water"
    } else {
        "minecraft:air"
    };
    Some(((target.0, target.1 + 1, target.2), block))
}

/// Does this item place its block from `ItemStack.useOn`?
///
/// This server inlines `BlockItem.useOn`'s placement arm into the
/// `use_item_on` handler and runs it for **every** item that names a block.
/// Vanilla dispatches it virtually, and two subclasses answer differently:
///
/// * **`PlaceOnWaterBlockItem.useOn` is `getstatic PASS; areturn`** — the
///   whole method, offsets 0-3. A lily pad or frogspawn clicked on a block
///   places **nothing**; the item reaches its block through `use` instead
///   (`getPlayerPOVHitResult` with `ClipContext.Fluid.SOURCE_ONLY`, then
///   `BlockItem.useOn` on the position *above* the water it hit). This server
///   decodes no `minecraft:use_item`, so the honest answer is that neither
///   item places anything — and that is nearer vanilla than placing a lily
///   pad against a wall, which is what this server did.
/// * **`GameMasterBlockItem.getPlacementState` returns `null`** — vanilla's
///   refusal — unless `player.canUseGameMasterBlocks()`, offsets 5-24. Seven
///   items: the three command blocks, the structure block, the jigsaw and the
///   two test blocks.
///
/// `game_master` is the caller's answer to `canUseGameMasterBlocks()`, which
/// is `Abilities.instabuild && permission >= 2`. It is hardcoded `false` at
/// the one call site, exactly as it already is for
/// `crate::useblock::use_gate` two hundred lines below — this server has no
/// per-player ability model, and the two places that need the same answer now
/// give the same one.
///
/// Without a table this is `true` for everything, which is what the server
/// did before.
#[must_use]
pub fn places_from_use_on(
    props: Option<&crate::itemprops::ItemProps>,
    item: &str,
    game_master: bool,
) -> bool {
    let Some(class) = props.and_then(|p| p.class_of(item)) else {
        return true;
    };
    match class {
        "net.minecraft.world.item.PlaceOnWaterBlockItem" => false,
        "net.minecraft.world.item.GameMasterBlockItem" => game_master,
        _ => true,
    }
}

/// `BedItem.placeBlock`'s flags — `bipush 26`, offsets 0-14.
///
/// **`UPDATE_NEIGHBORS` is clear and `UPDATE_KNOWN_SHAPE` is set**, which is
/// the opposite of both on `BlockItem.placeBlock`'s 11. So placing a bed's
/// foot runs **neither** the neighbour update nor the shape cascade: 26 is
/// `UPDATE_CLIENTS | UPDATE_IMMEDIATE | UPDATE_KNOWN_SHAPE`.
///
/// That is not an optimisation to be reproduced approximately. A bed is two
/// blocks and the head is written afterwards by `setPlacedBy`; running the
/// cascade from the foot first would ask the position the head is about to
/// occupy to update against a foot with no head, and every one of those
/// updates is a packet.
pub const BED_PLACE_FLAGS: i32 = 26;

/// The flags `BlockItem.placeBlock` writes the placed block with.
///
/// `bipush 11` for every `BlockItem` — `UPDATE_NEIGHBORS | UPDATE_CLIENTS |
/// UPDATE_IMMEDIATE` — and [`BED_PLACE_FLAGS`] for the sixteen `BedItem`s.
///
/// The general answer is given as [`crate::shapes::UPDATE_ALL`] (3) rather
/// than as the jar's 11, and the difference is exactly `UPDATE_IMMEDIATE`
/// (8), which `Level.setBlock` never reads: it is a client-side re-render
/// hint. That is the number this server has always used for a placement and
/// changing it would be a no-op with a diff. The bed's 26 is a different
/// matter — two of its bits are ones `crate::shapes::set_block` acts on.
#[must_use]
pub fn place_flags(props: Option<&crate::itemprops::ItemProps>, item: &str) -> i32 {
    if props.and_then(|p| p.class_of(item)) == Some("net.minecraft.world.item.BedItem") {
        return BED_PLACE_FLAGS;
    }
    crate::shapes::UPDATE_ALL
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real extract, so the 28 is vanilla's and not a list kept here.
    fn props() -> Option<&'static crate::itemprops::ItemProps> {
        let path = std::path::Path::new("../..").join(crate::itemprops::TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props table", "ci/item-props-golden.sh") {
            return None;
        }
        Some(Box::leak(Box::new(
            crate::itemprops::ItemProps::load(&path).expect("load"),
        )))
    }

    /// A door clears the block above; the fill follows `isWaterAt`.
    #[test]
    fn a_door_clears_the_block_above_and_water_stays_water() {
        let Some(p) = props() else { return };
        assert_eq!(
            clear_above(Some(p), "minecraft:oak_door", (4, 70, -2), false),
            Some(((4, 71, -2), "minecraft:air")),
            "the position is above the TARGET, not the clicked block"
        );
        assert_eq!(
            clear_above(Some(p), "minecraft:oak_door", (4, 70, -2), true),
            Some(((4, 71, -2), "minecraft:water")),
            "offsets 23-29: a flooded doorway keeps its water"
        );
    }

    /// **The dispatch is by class and the count is vanilla's.** 28 items, and
    /// they are not all doors: `tall_grass`, `sunflower` and the rest of the
    /// tall flowers are `DoubleHighBlockItem` too. A hand-written list of
    /// doors would pass every door test and miss twelve plants.
    #[test]
    fn twenty_eight_items_are_double_high_and_they_are_not_all_doors() {
        let Some(p) = props() else { return };
        assert_eq!(
            p.class_count("net.minecraft.world.item.DoubleHighBlockItem"),
            28,
            "the class column's own count, not a list kept here"
        );
        let doubles = ["minecraft:oak_door", "minecraft:iron_door"];
        let plants = ["minecraft:tall_grass", "minecraft:sunflower"];
        for item in doubles.iter().chain(plants.iter()) {
            assert!(p.is_double_high(item), "{item}");
        }
        // The negative control, one per neighbouring shape: a plain
        // `BlockItem`, a `BedItem` (which also writes a second block, through
        // `setPlacedBy` rather than through this override) and a non-block
        // item.
        for item in [
            "minecraft:stone",
            "minecraft:red_bed",
            "minecraft:diamond_sword",
        ] {
            assert!(!p.is_double_high(item), "{item}");
            assert_eq!(clear_above(Some(p), item, (0, 0, 0), false), None);
        }
    }

    /// No table means no clear, and that must be `None` rather than a guess:
    /// a checkout without the vanilla extract places a door exactly as it did
    /// before this function existed.
    #[test]
    fn no_table_means_no_clear() {
        assert_eq!(
            clear_above(None, "minecraft:oak_door", (0, 0, 0), false),
            None
        );
    }

    /// The two refusing subclasses, their counts, and a positive control.
    ///
    /// The control is what makes this a test of the dispatch rather than of a
    /// list: `minecraft:stone` is a plain `BlockItem` and must still place,
    /// and so must `minecraft:oak_door`, which is a *different* subclass that
    /// overrides a different step.
    #[test]
    fn two_subclasses_refuse_to_place_from_use_on() {
        let Some(p) = props() else { return };
        assert_eq!(
            p.class_count("net.minecraft.world.item.PlaceOnWaterBlockItem"),
            2
        );
        assert_eq!(
            p.class_count("net.minecraft.world.item.GameMasterBlockItem"),
            7
        );
        for item in ["minecraft:lily_pad", "minecraft:frogspawn"] {
            assert!(
                !places_from_use_on(Some(p), item, false),
                "{item}: PlaceOnWaterBlockItem.useOn is an unconditional PASS"
            );
            assert!(
                !places_from_use_on(Some(p), item, true),
                "{item}: and game-master status has nothing to do with it"
            );
        }
        for item in ["minecraft:command_block", "minecraft:jigsaw"] {
            assert!(
                !places_from_use_on(Some(p), item, false),
                "{item} for anyone"
            );
            assert!(
                places_from_use_on(Some(p), item, true),
                "{item} for a player who canUseGameMasterBlocks"
            );
        }
        for item in ["minecraft:stone", "minecraft:oak_door", "minecraft:torch"] {
            assert!(places_from_use_on(Some(p), item, false), "{item}");
        }
        assert!(
            places_from_use_on(None, "minecraft:lily_pad", false),
            "no table is the old behaviour exactly"
        );
    }

    /// A bed places with 26 and everything else with `UPDATE_ALL`.
    ///
    /// The assertions are on the **bits**, not on the number, because the
    /// claim is behavioural: no neighbour update and no shape cascade from a
    /// bed's foot, both of which a plain block does run.
    #[test]
    fn a_bed_places_without_a_neighbour_update_or_a_shape_cascade() {
        let Some(p) = props() else { return };
        assert_eq!(p.class_count("net.minecraft.world.item.BedItem"), 16);
        let bed = place_flags(Some(p), "minecraft:red_bed");
        assert_eq!(bed, BED_PLACE_FLAGS);
        assert_eq!(
            bed & crate::shapes::UPDATE_NEIGHBORS,
            0,
            "26 has no UPDATE_NEIGHBORS: the foot updates nothing"
        );
        assert_ne!(
            bed & crate::shapes::UPDATE_KNOWN_SHAPE,
            0,
            "26 sets UPDATE_KNOWN_SHAPE: no cascade from the foot"
        );
        assert_ne!(
            bed & crate::shapes::UPDATE_CLIENTS,
            0,
            "the players are still told"
        );

        for item in ["minecraft:stone", "minecraft:oak_door", "minecraft:torch"] {
            let f = place_flags(Some(p), item);
            assert_eq!(f, crate::shapes::UPDATE_ALL, "{item}");
            assert_ne!(f & crate::shapes::UPDATE_NEIGHBORS, 0, "{item}");
            assert_eq!(f & crate::shapes::UPDATE_KNOWN_SHAPE, 0, "{item}");
        }
        assert_eq!(
            place_flags(None, "minecraft:red_bed"),
            crate::shapes::UPDATE_ALL,
            "no table is the old behaviour exactly"
        );
    }
}

/// `BlockItem.updateBlockStateFromTag`'s flag for the second write: `2`,
/// `UPDATE_CLIENTS` alone (offsets 46-47).
pub const BLOCK_STATE_FLAGS: i32 = 2;

/// `BlockItemStateProperties.apply(state)`: each `(property, value)` the
/// stack's `minecraft:block_state` names, set on `state` through
/// `updateState` — a property the block does not have, or a value that
/// property cannot take, leaves the state as it was and moves on. The
/// properties are independent, so their order (a `HashMap`'s) cannot matter.
///
/// `has_state` is the block table's own knowledge of which states exist,
/// which is exactly the set `Property.getValue` would accept.
#[must_use]
pub fn apply_block_state_props(
    state: &str,
    props: &[(String, String)],
    has_state: &dyn Fn(&str) -> bool,
) -> String {
    let mut out = state.to_string();
    for (key, value) in props {
        let Some((name, rest)) = out.split_once('[') else {
            return out;
        };
        let Some(inner) = rest.strip_suffix(']') else {
            return out;
        };
        let mut found = false;
        let parts: Vec<String> = inner
            .split(',')
            .map(|kv| match kv.split_once('=') {
                Some((k, _)) if k == key => {
                    found = true;
                    format!("{k}={value}")
                }
                _ => kv.to_string(),
            })
            .collect();
        if !found {
            continue;
        }
        let candidate = format!("{name}[{}]", parts.join(","));
        if has_state(&candidate) {
            out = candidate;
        }
    }
    out
}

#[cfg(test)]
mod block_state_tests {
    use super::apply_block_state_props;

    /// Each pair is set when the block has the property and the value is one
    /// it can take, and skipped otherwise — `updateState`'s `orElse(state)`.
    #[test]
    fn block_state_props_set_what_exists_and_skip_the_rest() {
        let known = |s: &str| {
            ["y", "x", "z"]
                .iter()
                .any(|a| s == format!("minecraft:oak_log[axis={a}]"))
        };
        let p = |k: &str, v: &str| (k.to_string(), v.to_string());
        assert_eq!(
            apply_block_state_props("minecraft:oak_log[axis=y]", &[p("axis", "z")], &known),
            "minecraft:oak_log[axis=z]"
        );
        assert_eq!(
            apply_block_state_props(
                "minecraft:oak_log[axis=y]",
                &[p("axis", "q"), p("facing", "north")],
                &known
            ),
            "minecraft:oak_log[axis=y]",
            "a bad value and a missing property both leave it alone"
        );
        assert_eq!(
            apply_block_state_props("minecraft:stone", &[p("axis", "z")], &|_| true),
            "minecraft:stone",
            "a block with no properties"
        );
    }
}
