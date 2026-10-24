//! `BlockBehaviour.affectNeighborsAfterRemoval` — what a redstone component
//! does to its neighbours on the way *out*.
//!
//! # Where vanilla calls it, read from `LevelChunk.setBlockState`
//!
//! ```text
//!   331-342  bl  = !oldState.is(block)          the block changed
//!   345-356  bl2 = (flags & 64) != 0            UPDATE_MOVE_BY_PISTON
//!   359-371  bl3 = (flags & 256) == 0           not SKIP_BLOCK_ENTITY_SIDEEFFECTS
//!   375-435  block-entity removal, gated on bl && oldState.hasBlockEntity()
//!   440-490  if (bl || block instanceof BaseRailBlock)
//!              && level instanceof ServerLevel
//!              && ((flags & 1) != 0 || bl2)
//!                oldState.affectNeighborsAfterRemoval(level, pos, bl2)
//!   514-542  if (!isClientSide && (flags & 512) == 0)
//!                state.onPlace(level, pos, oldState, bl2)
//! ```
//!
//! So it runs **inside** the chunk write, after the block entity has gone and
//! *before* `onPlace`, and before `Level.setBlock`'s own `updateNeighborsAt`.
//! Until this module existed [`crate::shapes::set_block`] went straight from
//! the write to `onPlace`, so digging out a powered lever told the lever's six
//! neighbours (the flag-1 half) and never the six neighbours of the block it
//! was attached to — which is where the thing it was powering usually is.
//!
//! # The per-block bodies, from the jar
//!
//! ```text
//! RedStoneWireBlock       if (moved) return;
//!                         for d in Direction.values(): updateNeighborsAt(pos.relative(d), this)
//!                         updatePowerStrength(level, pos, state, null, false)
//!                         updateNeighborsOfNeighboringWires(level, pos)
//! RedstoneTorchBlock      if (!moved) notifyNeighbors      (= the six-direction loop onPlace uses)
//! LeverBlock              if (!moved && POWERED) updateNeighbours
//! ButtonBlock             if (!moved && POWERED) updateNeighbours
//!   updateNeighbours:     updateNeighborsAt(pos, this);
//!                         updateNeighborsAt(pos.relative(getConnectedDirection(state).getOpposite()), this)
//! ObserverBlock           if (POWERED && getBlockTicks().hasScheduledTick(pos, this))
//!                             updateNeighborsInFront(level, pos, state.setValue(POWERED, false))
//! DiodeBlock              if (!moved) updateNeighborsInFront(level, pos, state)
//! BasePressurePlateBlock  if (!moved && getSignalForState(state) > 0) updateNeighbours
//!   updateNeighbours:     updateNeighborsAt(pos, this); updateNeighborsAt(pos.below(), this)
//! PistonHeadBlock         if (isFittingBase(state, getBlockState(pos.relative(FACING.getOpposite()))))
//!                             destroyBlock(that, true)
//! ```
//!
//! Note the two that ignore `movedByPiston`: the observer and the piston
//! head. Everything else stays quiet when a piston carries it, because the
//! piston's own `moveBlocks` calls `affectNeighborsAfterRemoval(..., false)`
//! on the vacated positions itself.
//!
//! `BaseRailBlock` has a body too and is not here: rails belong to
//! [`crate::rails`], and the redstone lane leaves that file alone. Its
//! absence is a named gap, not an oversight.

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::placedby::PlacedByTable;
use crate::power::{update_neighbors_at, SignalTable, DIRECTIONS};
use crate::shapes::{Effects, ShapeTable};

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection`. The
/// `$SwitchMap` indices are source order, not ordinal: case 1 is `CEILING`
/// (DOWN), case 2 is `FLOOR` (UP), default is `WALL` (the `facing`).
#[must_use]
fn connected_direction_of(state: &str) -> Option<Face> {
    match crate::placement::property_of(state, "face")? {
        "ceiling" => Some(Face::Down),
        "floor" => Some(Face::Up),
        _ => crate::placement::property_of(state, "facing").and_then(Face::from_name),
    }
}

/// `LeverBlock.updateNeighbours` / `ButtonBlock.updateNeighbours` — the same
/// two calls, byte for byte.
fn face_attached_update_neighbours(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    let block = block_of(state);
    update_neighbors_at(blocks, signals, shapes, pos, block, updates);
    if let Some(back) = connected_direction_of(state).map(Face::opposite) {
        update_neighbors_at(blocks, signals, shapes, back.relative(pos), block, updates);
    }
}

/// `BasePressurePlateBlock.getSignalForState`: `POWERED ? 15 : 0` for a
/// `PressurePlateBlock`, the `POWER` property for a weighted one.
#[must_use]
fn plate_signal(state: &str) -> i32 {
    if let Some(p) = crate::placement::property_of(state, "power") {
        return p.parse().unwrap_or(0);
    }
    if crate::placement::property_of(state, "powered") == Some("true") {
        15
    } else {
        0
    }
}

/// `PistonHeadBlock.isFittingBase(head, base)`.
#[must_use]
pub fn is_fitting_base(head: &str, base: &str) -> bool {
    let wanted = if crate::placement::property_of(head, "type") == Some("sticky") {
        "minecraft:sticky_piston"
    } else {
        "minecraft:piston"
    };
    block_of(base) == wanted
        && crate::placement::property_of(base, "extended") == Some("true")
        && crate::placement::property_of(base, "facing")
            == crate::placement::property_of(head, "facing")
}

/// The dispatch. `old` is the state that was just overwritten at `pos`;
/// `moved_by_piston` is `(flags & 64) != 0`. The caller has already checked
/// the block changed and that `(flags & 1) != 0 || moved_by_piston`.
#[expect(
    clippy::too_many_arguments,
    reason = "one entry point per vanilla hook, with the tables it needs"
)]
pub fn affect_neighbors_after_removal(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    classes: Option<&PlacedByTable>,
    pos: BlockPos,
    old: &str,
    moved_by_piston: bool,
    updates: &mut Effects,
) {
    let block = block_of(old);
    if crate::wire::is_wire(old) {
        if moved_by_piston {
            return;
        }
        let Some(signals) = signals else { return };
        for d in DIRECTIONS {
            update_neighbors_at(
                blocks,
                Some(signals),
                shapes,
                d.relative(pos),
                block,
                updates,
            );
        }
        crate::power::wire_after_removal(blocks, signals, shapes, pos, old, updates);
        return;
    }
    match block {
        "minecraft:redstone_torch" | "minecraft:redstone_wall_torch" => {
            if !moved_by_piston {
                if let Some(signals) = signals {
                    crate::power::torch_on_place(blocks, signals, shapes, pos, updates);
                }
            }
        }
        "minecraft:lever" => {
            if !moved_by_piston && crate::placement::property_of(old, "powered") == Some("true") {
                face_attached_update_neighbours(blocks, signals, shapes, pos, old, updates);
            }
        }
        "minecraft:observer" => {
            if crate::placement::property_of(old, "powered") == Some("true")
                && crate::ticks::ScheduledTicks::shared().has_scheduled_tick(pos, block)
            {
                if let Some(signals) = signals {
                    let unpowered = crate::placement::with_property(old, "powered", "false");
                    crate::power::observer_update_neighbors_in_front(
                        blocks, signals, shapes, pos, &unpowered, updates,
                    );
                }
            }
        }
        "minecraft:repeater" | "minecraft:comparator" => {
            if !moved_by_piston {
                if let Some(signals) = signals {
                    crate::power::diode_update_neighbors_in_front(
                        blocks, signals, shapes, pos, old, updates,
                    );
                }
            }
        }
        "minecraft:piston_head" => {
            if let Some(facing) =
                crate::placement::property_of(old, "facing").and_then(Face::from_name)
            {
                let base_pos = facing.opposite().relative(pos);
                if is_fitting_base(old, &blocks.state_at(base_pos)) {
                    crate::shapes::destroy_block(
                        blocks,
                        shapes,
                        base_pos,
                        true,
                        crate::shapes::RECURSION_LIMIT,
                        updates,
                    );
                }
            }
        }
        _ => {
            let Some(classes) = classes else { return };
            if classes.is_a(block, crate::ticks::BUTTON_BLOCK) {
                if !moved_by_piston && crate::placement::property_of(old, "powered") == Some("true")
                {
                    face_attached_update_neighbours(blocks, signals, shapes, pos, old, updates);
                }
            } else if classes.is_a(block, "BasePressurePlateBlock")
                && !moved_by_piston
                && plate_signal(old) > 0
            {
                update_neighbors_at(blocks, signals, shapes, pos, block, updates);
                update_neighbors_at(
                    blocks,
                    signals,
                    shapes,
                    (pos.0, pos.1 - 1, pos.2),
                    block,
                    updates,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct World {
        states: Mutex<BTreeMap<BlockPos, String>>,
    }

    impl World {
        fn new(initial: &[(BlockPos, &str)]) -> Self {
            Self {
                states: Mutex::new(
                    initial
                        .iter()
                        .map(|(p, s)| (*p, (*s).to_string()))
                        .collect(),
                ),
            }
        }
        fn at(&self, pos: BlockPos) -> String {
            self.states
                .lock()
                .unwrap()
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            self.at(pos)
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            let mut map = self.states.lock().unwrap();
            if map.get(&pos).map(String::as_str) == Some(state) {
                return None;
            }
            map.insert(pos, state.to_string());
            Some(1)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _state: &str) -> bool {
            true
        }
        fn state_id(&self, _state: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.at(pos))
        }
        fn face_sturdy(&self, pos: BlockPos, _face: Face) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }
        fn redstone_conductor(&self, pos: BlockPos) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }
        fn signal_source(&self, pos: BlockPos) -> bool {
            matches!(
                block_of(&self.at(pos)),
                "minecraft:lever" | "minecraft:redstone_torch" | "minecraft:redstone_block"
            )
        }
        fn water_at(&self, _pos: BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _pos: BlockPos) -> String {
            "minecraft:air".to_string()
        }
    }

    fn table() -> SignalTable {
        let json = r#"{
          "rng": "signal", "states": 4, "quiet": 1, "sources": 3, "refusedStates": 0,
          "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
          "refused": [], "unanswerable": [],
          "table": [
            {"state": "minecraft:redstone_block", "id": 1, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,0,0,0,0,0]},
            {"state": "minecraft:lever[face=floor,facing=north,powered=true]", "id": 6, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,15,0,0,0,0]},
            {"state": "minecraft:redstone_torch[lit=true]", "id": 2, "source": true,
             "signal": [15,0,15,15,15,15], "direct": [15,0,0,0,0,0]}
          ]
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "rustmc-removal-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("signal.json");
        std::fs::write(&path, json).unwrap();
        let out = SignalTable::load(&path).expect("fixture loads");
        let _ = std::fs::remove_file(&path);
        out
    }

    /// The rig `a-lever-dug-out-darkens-the-lamp-through-its-block` uses: a
    /// floor lever on a stone, a lit lamp beside the stone. The lamp is not
    /// a neighbour of the lever, so `Level.setBlock`'s own flag-1 sweep
    /// never reaches it; `LeverBlock.updateNeighbours`' second call — the
    /// six neighbours of the block the lever was attached to — does.
    #[test]
    fn digging_a_powered_floor_lever_reaches_the_lamp_beside_its_support() {
        let t = table();
        let lever = "minecraft:lever[face=floor,facing=north,powered=true]";
        let w = World::new(&[
            ((1, 0, 0), "minecraft:stone"),
            ((2, 0, 0), "minecraft:redstone_lamp[lit=true]"),
        ]);
        let mut fx = Effects::new();
        affect_neighbors_after_removal(&w, Some(&t), None, None, (1, 1, 0), lever, false, &mut fx);
        let lamp: Vec<_> = fx.ticks.iter().filter(|r| r.pos == (2, 0, 0)).collect();
        assert_eq!(
            lamp.len(),
            1,
            "the lamp books exactly one tick: {:?}",
            fx.ticks
        );
        assert_eq!(lamp[0].block, "minecraft:redstone_lamp");
        assert_eq!(
            lamp[0].delay, 4,
            "RedstoneLampBlock.neighborChanged's scheduleTick(4)"
        );

        let mut quiet = Effects::new();
        let unpowered = "minecraft:lever[face=floor,facing=north,powered=false]";
        affect_neighbors_after_removal(
            &w,
            Some(&t),
            None,
            None,
            (1, 1, 0),
            unpowered,
            false,
            &mut quiet,
        );
        assert!(
            quiet.ticks.is_empty(),
            "an unpowered lever leaving says nothing"
        );

        let mut carried = Effects::new();
        affect_neighbors_after_removal(
            &w,
            Some(&t),
            None,
            None,
            (1, 1, 0),
            lever,
            true,
            &mut carried,
        );
        assert!(carried.ticks.is_empty(), "movedByPiston silences the lever");
    }

    /// `RedstoneTorchBlock.affectNeighborsAfterRemoval` is `notifyNeighbors`:
    /// the six neighbours' neighbours. A lamp on top of the block the torch
    /// powers from below is two positions from the torch and hears it.
    #[test]
    fn digging_a_torch_reaches_the_lamp_on_the_block_it_powered() {
        let t = table();
        let w = World::new(&[
            ((0, 1, 0), "minecraft:stone"),
            ((0, 2, 0), "minecraft:redstone_lamp[lit=true]"),
        ]);
        let mut fx = Effects::new();
        affect_neighbors_after_removal(
            &w,
            Some(&t),
            None,
            None,
            (0, 0, 0),
            "minecraft:redstone_torch[lit=true]",
            false,
            &mut fx,
        );
        assert!(
            fx.ticks.iter().any(|r| r.pos == (0, 2, 0) && r.delay == 4),
            "{:?}",
            fx.ticks
        );
    }

    /// The piston head's arm ignores `movedByPiston` and destroys a fitting
    /// base, which is a `destroyBlock` — a level event and an air write.
    #[test]
    fn digging_a_piston_head_destroys_its_extended_base() {
        let w = World::new(&[((1, 0, 0), "minecraft:piston[extended=true,facing=north]")]);
        let mut fx = Effects::new();
        affect_neighbors_after_removal(
            &w,
            None,
            None,
            None,
            (1, 0, -1),
            "minecraft:piston_head[facing=north,short=false,type=normal]",
            true,
            &mut fx,
        );
        assert_eq!(w.at((1, 0, 0)), "minecraft:air");
        assert_eq!(fx.level_events.len(), 1, "one LEVEL_EVENT_DESTROY");

        let w = World::new(&[((1, 0, 0), "minecraft:piston[extended=false,facing=north]")]);
        let mut fx = Effects::new();
        affect_neighbors_after_removal(
            &w,
            None,
            None,
            None,
            (1, 0, -1),
            "minecraft:piston_head[facing=north,short=false,type=normal]",
            false,
            &mut fx,
        );
        assert_eq!(
            w.at((1, 0, 0)),
            "minecraft:piston[extended=false,facing=north]"
        );
    }

    #[test]
    fn a_floor_lever_is_connected_upwards_and_a_wall_lever_along_its_facing() {
        assert_eq!(
            connected_direction_of("minecraft:lever[face=floor,facing=north,powered=true]"),
            Some(Face::Up)
        );
        assert_eq!(
            connected_direction_of("minecraft:lever[face=ceiling,facing=north,powered=true]"),
            Some(Face::Down)
        );
        assert_eq!(
            connected_direction_of("minecraft:stone_button[face=wall,facing=east,powered=true]"),
            Some(Face::East)
        );
    }

    #[test]
    fn a_head_fits_only_an_extended_base_of_its_own_type_and_facing() {
        let head = "minecraft:piston_head[facing=north,short=false,type=normal]";
        assert!(is_fitting_base(
            head,
            "minecraft:piston[extended=true,facing=north]"
        ));
        assert!(!is_fitting_base(
            head,
            "minecraft:piston[extended=false,facing=north]"
        ));
        assert!(!is_fitting_base(
            head,
            "minecraft:piston[extended=true,facing=south]"
        ));
        assert!(!is_fitting_base(
            head,
            "minecraft:sticky_piston[extended=true,facing=north]"
        ));
        let sticky = "minecraft:piston_head[facing=north,short=false,type=sticky]";
        assert!(is_fitting_base(
            sticky,
            "minecraft:sticky_piston[extended=true,facing=north]"
        ));
    }

    #[test]
    fn a_plates_signal_is_fifteen_or_its_power_property() {
        assert_eq!(
            plate_signal("minecraft:stone_pressure_plate[powered=true]"),
            15
        );
        assert_eq!(
            plate_signal("minecraft:stone_pressure_plate[powered=false]"),
            0
        );
        assert_eq!(
            plate_signal("minecraft:light_weighted_pressure_plate[power=7]"),
            7
        );
    }
}
