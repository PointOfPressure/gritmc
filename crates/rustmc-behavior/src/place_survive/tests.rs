//! Unit vectors for the parts the golden cannot separate.

use super::*;

/// `Direction.getOpposite()` is the ordinal XOR 1, and it is worth a vector
/// because the whole file depends on the enum grouping the axes Y, Z, X — the
/// same grouping that once made an `oracle-runner` test assert the DOWN
/// cursor lay on the x face.
#[test]
fn opposites_are_the_enums_own_pairing() {
    assert_eq!(opposite(DOWN), UP);
    assert_eq!(opposite(UP), DOWN);
    assert_eq!(opposite(NORTH), SOUTH);
    assert_eq!(opposite(SOUTH), NORTH);
    assert_eq!(opposite(WEST), EAST);
    assert_eq!(opposite(EAST), WEST);
}

#[test]
fn the_horizontal_turn_is_north_east_south_west() {
    assert_eq!(clockwise(NORTH), Some(EAST));
    assert_eq!(clockwise(EAST), Some(SOUTH));
    assert_eq!(clockwise(SOUTH), Some(WEST));
    assert_eq!(clockwise(WEST), Some(NORTH));
    assert_eq!(clockwise(UP), None);
    assert_eq!(counter_clockwise(NORTH), Some(WEST));
}

/// A property read must not be fooled by a prefix. `half=upper` is a
/// substring of `other_half=upper` and a `contains` here would be a silent
/// wrong answer for the door family.
#[test]
fn a_property_is_a_whole_field() {
    let state = "minecraft:oak_door[facing=north,half=upper,hinge=left]";
    assert_eq!(property(state, "half"), Some("upper"));
    assert_eq!(property(state, "alf"), None);
    assert_eq!(property(state, "facing"), Some("north"));
    assert_eq!(property("minecraft:torch", "half"), None);
    assert_eq!(block_of(state), "minecraft:oak_door");
    assert_eq!(block_of("minecraft:torch"), "minecraft:torch");
}

#[test]
fn a_direction_property_is_read_as_an_ordinal() {
    assert_eq!(direction_property("a[facing=down]", "facing"), Some(DOWN));
    assert_eq!(direction_property("a[facing=east]", "facing"), Some(EAST));
    assert_eq!(direction_property("a[facing=nope]", "facing"), None);
}

/// A class this port has never seen is `Unported`, not a guess off a
/// similar-looking name.
#[test]
fn an_unknown_class_is_unported_and_not_guessed() {
    assert!(family_of("net.minecraft.world.level.block.TorchBlock", None).is_none());
    assert!(family_of("net.minecraft.world.level.block.BaseTorchBlock", None).is_some());
    assert_eq!(
        unported_classes(
            [
                ("a.b.VineBlock", None),
                ("a.b.LadderBlock", None),
                ("a.b.VineBlock", None)
            ]
            .into_iter()
        ),
        vec!["a.b.VineBlock".to_string()]
    );
}

/// A `VegetationBlock` is dispatched on its `mayPlaceOn`, and an unported one
/// is named by **that** class rather than by `VegetationBlock` — otherwise
/// six blocks reading an instance-field tag would report the whole 48-block
/// family as unported.
#[test]
fn a_vegetation_block_is_keyed_on_its_may_place_on() {
    let veg = "net.minecraft.world.level.block.VegetationBlock";
    assert!(family_of(veg, Some(veg)).is_some());
    assert!(family_of(veg, Some("a.b.LilyPadBlock")).is_some());
    assert!(family_of(veg, Some("a.b.StemBlock")).is_none());
    assert!(family_of(veg, None).is_none());
    assert_eq!(
        unported_classes([(veg, Some("a.b.StemBlock"))].into_iter()),
        vec!["a.b.StemBlock".to_string()]
    );
}

/// Looking straight down puts DOWN first and UP last, and the array is a
/// permutation of all six — which is what `makeDirectionArray`'s
/// `{p, q, r, ~r, ~q, ~p}` guarantees and is the cheapest way to catch a
/// transcription that dropped an arm.
#[test]
fn the_look_order_is_always_a_permutation_of_the_six() {
    for x_rot in [-90.0, -45.0, -1.0, 0.0, 1.0, 45.0, 89.9] {
        for y_rot in [-180.0, -135.0, -90.0, -45.0, 0.0, 45.0, 90.0, 135.0, 179.9] {
            let order = ordered_by_nearest(x_rot, y_rot);
            let mut seen = order;
            seen.sort_unstable();
            assert_eq!(
                seen,
                [0, 1, 2, 3, 4, 5],
                "x_rot {x_rot} y_rot {y_rot} gave {order:?}"
            );
            // `{p, q, r, ~r, ~q, ~p}` — the second half is the first
            // reversed and opposited.
            for i in 0..3 {
                assert_eq!(order[5 - i], opposite(order[i]), "at {i} for {order:?}");
            }
        }
    }
}

/// **`up` is `sinX < 0`, so looking DOWN is a positive pitch.** The inverted
/// y term is the one thing in `orderedByNearest` a transcription gets
/// backwards, and it is invisible in a permutation check.
#[test]
fn looking_down_puts_down_first() {
    assert_eq!(ordered_by_nearest(90.0, 0.0)[0], DOWN, "pitch +90 is down");
    assert_eq!(ordered_by_nearest(90.0, 0.0)[5], UP);
    assert_eq!(ordered_by_nearest(-90.0, 0.0)[0], UP, "pitch -90 is up");
    assert_eq!(ordered_by_nearest(-90.0, 0.0)[5], DOWN);
}

/// Yaw 0 faces SOUTH in Minecraft's frame, and the horizontals follow from
/// `east = sin(-yaw) > 0` and `south = cos(-yaw) > 0`.
#[test]
fn a_level_look_orders_the_horizontals_by_yaw() {
    assert_eq!(ordered_by_nearest(0.0, 0.0)[0], SOUTH, "yaw 0 faces south");
    assert_eq!(ordered_by_nearest(0.0, 180.0)[0], NORTH);
    assert_eq!(ordered_by_nearest(0.0, -90.0)[0], EAST);
    assert_eq!(ordered_by_nearest(0.0, 90.0)[0], WEST);
}

/// The clicked face's OPPOSITE goes to the front — the direction pointing
/// back at the block that was clicked, which is the one an attachment would
/// use.
#[test]
fn the_clicked_faces_opposite_is_moved_to_the_front() {
    // Looking level and south, clicking the UP face of the floor: the
    // opposite is DOWN and it leads.
    let order = nearest_looking_directions(0.0, 0.0, UP, false);
    assert_eq!(order[0], DOWN);
    let mut seen = order;
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2, 3, 4, 5], "still a permutation");

    // `replaceClicked` skips the reorder entirely.
    let untouched = nearest_looking_directions(0.0, 0.0, UP, true);
    assert_eq!(untouched, ordered_by_nearest(0.0, 0.0));
}

/// The walk itself: the attachment's opposite is skipped, the attachment
/// itself gives the standing block, everything else the wall block, and a
/// world that holds neither refuses.
#[test]
fn the_standing_and_wall_walk_takes_the_first_that_survives() {
    // A sign: attachment DOWN, so UP is skipped.
    let all = [UP, DOWN, NORTH, SOUTH, WEST, EAST];
    assert_eq!(
        standing_or_wall(all, DOWN, &|_| true),
        Some(Placement::Standing),
        "UP is skipped, DOWN is next and is the attachment"
    );
    assert_eq!(
        standing_or_wall(all, DOWN, &|p| p == Placement::Wall),
        Some(Placement::Wall),
        "with no floor under it the walk carries on to a horizontal"
    );
    assert_eq!(
        standing_or_wall(all, DOWN, &|_| false),
        None,
        "and refuses outright when neither survives anywhere"
    );
    // A hanging sign: attachment UP, so DOWN is the skipped one.
    assert_eq!(
        standing_or_wall([DOWN, UP, NORTH, SOUTH, WEST, EAST], UP, &|_| true),
        Some(Placement::Standing),
        "DOWN skipped, UP is the attachment"
    );
}
