//! Against the extract, and against the arithmetic the extract cannot state.

use super::*;

fn table() -> Option<BonemealTable> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push(TABLE_PATH);
    BonemealTable::load(&path).ok()
}

/// The census, asserted rather than described: the pair 63 / 5 is the honest
/// statement of this port and either number alone reads as coverage it does
/// not have.
#[test]
fn sixty_three_blocks_are_bonemealable_and_five_of_them_grow() {
    let Some(t) = table() else { return };
    assert_eq!(t.len(), 63, "blocks implementing BonemealableBlock");
    assert_eq!(t.ported(), 5, "of which this module can grow");
    assert_eq!(t.level_event(), 1505);
    assert_eq!(t.level_event_data(), 15);
    assert_eq!(t.set_block_flags(), 2, "UPDATE_CLIENTS alone");
    assert_eq!(t.shrink(), 1);
}

/// `Mth.nextInt(random, 2, 5)` is `nextInt(4) + 2` — bound `hi - lo + 1`, so
/// the four rolls are 2, 3, 4 and 5. A port reading the bound as `hi - lo`
/// would never grow a crop by five.
#[test]
fn a_crops_increase_is_two_to_five_over_one_draw_of_bound_four() {
    assert_eq!(AgeIncrease::Crop.draw_bound(), Some(4));
    let got: Vec<i32> = (0..4).map(|r| AgeIncrease::Crop.increase(r)).collect();
    assert_eq!(got, vec![2, 3, 4, 5]);
}

/// **A torchflower draws nothing.** Its `getBonemealAgeIncrease` is
/// `iconst_1; ireturn`, so routing it through `CropBlock`'s version would
/// take one draw too many and desynchronise every later draw on that level's
/// random source — a divergence with no local symptom.
#[test]
fn a_torchflower_takes_no_draw_at_all() {
    assert_eq!(AgeIncrease::One.draw_bound(), None);
    assert_eq!(AgeIncrease::One.increase(0), 1);
    assert_eq!(AgeIncrease::One.increase(3), 1);
}

/// **A beetroot fails one bone meal in four and still consumes it.** Its
/// increase is `super / 3` on the same single draw, so a roll of 0 gives
/// `(0 + 2) / 3 == 0`.
#[test]
fn a_beetroot_grows_by_zero_on_one_roll_of_four() {
    assert_eq!(AgeIncrease::CropOverThree.draw_bound(), Some(4));
    let got: Vec<i32> = (0..4)
        .map(|r| AgeIncrease::CropOverThree.increase(r))
        .collect();
    assert_eq!(got, vec![0, 1, 1, 1]);
}

/// The zero-increase beetroot still writes, still shrinks and still sends the
/// event — so it is a `Crop` whose state equals the one that was there, not a
/// `Pass`. Collapsing it would lose a packet and an item.
#[test]
fn a_beetroot_that_grows_by_zero_is_still_a_write() {
    let Some(t) = table() else { return };
    assert_eq!(
        grow_crop(Some(&t), "minecraft:beetroots[age=1]", 0),
        Grow::Crop {
            state: "minecraft:beetroots[age=1]".to_string()
        }
    );
}

/// `Math.min(getMaxAge(), ...)`, with vanilla's own per-block max age: 7 for
/// wheat, 3 for beetroot, 2 for torchflower. A port that baked in
/// `CropBlock`'s 7 would grow a beetroot into a state that does not exist.
#[test]
fn growth_is_clamped_at_each_crops_own_max_age() {
    let Some(t) = table() else { return };
    assert_eq!(
        grow_crop(Some(&t), "minecraft:wheat[age=0]", 3),
        Grow::Crop {
            state: "minecraft:wheat[age=5]".to_string()
        }
    );
    assert_eq!(
        grow_crop(Some(&t), "minecraft:wheat[age=5]", 3),
        Grow::Crop {
            state: "minecraft:wheat[age=7]".to_string()
        }
    );
    assert_eq!(
        grow_crop(Some(&t), "minecraft:beetroots[age=0]", 3),
        Grow::Crop {
            state: "minecraft:beetroots[age=1]".to_string()
        }
    );
    assert_eq!(
        grow_crop(Some(&t), "minecraft:torchflower_crop[age=0]", 3),
        Grow::Crop {
            state: "minecraft:torchflower_crop[age=1]".to_string()
        }
    );
}

/// `TorchflowerCropBlock.getStateForAge(2)` is the torchflower itself. The
/// crop's `age` runs 0..1, so `torchflower_crop[age=2]` does not exist and a
/// write of it is refused — which is what this port did until 2026-10-01.
#[test]
fn a_torchflower_crop_at_its_last_age_becomes_the_flower() {
    let Some(t) = table() else { return };
    assert_eq!(
        grow_crop(Some(&t), "minecraft:torchflower_crop[age=1]", 0),
        Grow::Crop {
            state: "minecraft:torchflower".to_string()
        }
    );
}

/// A fully grown crop is not a valid target, so nothing is written and the
/// bone meal is not consumed. This is the half that distinguishes a working
/// gate from one that always grows.
#[test]
fn a_full_grown_crop_is_passed_over() {
    let Some(t) = table() else { return };
    assert_eq!(t.is_valid_target("minecraft:wheat[age=7]"), Some(false));
    assert_eq!(grow_crop(Some(&t), "minecraft:wheat[age=7]", 3), Grow::Pass);
    assert_eq!(t.is_valid_target("minecraft:wheat[age=6]"), Some(true));
}

/// Three answers, not two, and they are three different facts.
///
/// * `stone` is not a `BonemealableBlock` at all — `growCrop` offset 17.
/// * `oak_sapling` is one, and vanilla says **no** to it in a world of air
///   (`SaplingBlock.isValidBonemealTarget` reads what is under it). That is a
///   `Pass` because vanilla passes, not because this port cannot answer.
/// * `pumpkin_stem` is one, vanilla says **yes**, and this port cannot grow
///   it. That is `Unported`, and keeping it distinct from the two above is
///   the whole reason the enum has three arms: a gap that reports as a pass
///   is a gap nobody counts.
#[test]
fn the_three_answers_are_kept_apart() {
    let Some(t) = table() else { return };
    assert_eq!(t.is_valid_target("minecraft:stone"), None);
    assert_eq!(grow_crop(Some(&t), "minecraft:stone", 0), Grow::Pass);

    assert_eq!(
        t.is_valid_target("minecraft:oak_sapling[stage=0]"),
        Some(false),
        "vanilla itself refuses a sapling standing on air"
    );
    assert_eq!(
        grow_crop(Some(&t), "minecraft:oak_sapling[stage=0]", 0),
        Grow::Pass
    );

    assert_eq!(
        t.is_valid_target("minecraft:pumpkin_stem[age=0]"),
        Some(true)
    );
    assert_eq!(
        grow_crop(Some(&t), "minecraft:pumpkin_stem[age=0]", 0),
        Grow::Unported,
        "a stem is a valid target vanilla grows and this port does not"
    );
}

/// No table at all is `Pass` everywhere — the same fail-open shape every
/// other table on this path has.
#[test]
fn without_the_table_nothing_grows() {
    assert_eq!(grow_crop(None, "minecraft:wheat[age=0]", 3), Grow::Pass);
    assert_eq!(draw_bound(None, "minecraft:wheat[age=0]"), None);
}

/// The draw is taken only for the families that take one.
#[test]
fn the_draw_is_taken_only_where_vanilla_takes_one() {
    let Some(t) = table() else { return };
    assert_eq!(draw_bound(Some(&t), "minecraft:wheat[age=0]"), Some(4));
    assert_eq!(draw_bound(Some(&t), "minecraft:beetroots[age=0]"), Some(4));
    assert_eq!(
        draw_bound(Some(&t), "minecraft:torchflower_crop[age=0]"),
        None
    );
    assert_eq!(draw_bound(Some(&t), "minecraft:stone"), None);
}
