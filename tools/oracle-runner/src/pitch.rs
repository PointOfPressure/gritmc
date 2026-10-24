//! What a `sound` packet's pitch is allowed to be, per sound event.
//!
//! # The hole this closes
//!
//! `569323e` read the pitch off `ClientboundSoundPacket` and threw it away,
//! beside the `long seed`, and the reason was good: `ChestBlockEntity.playSound`
//! draws its pitch from `level.getRandom().nextFloat() * 0.1f + 0.9f`, and the
//! corpus run of 2026-08-07 02:19 recorded **five different pitches for that one
//! event** — 0.9120, 0.9268, 0.9339, 0.9552, 0.9952. A reference that disagrees
//! with itself run to run cannot be matched by any implementation.
//!
//! What it cost was stated at the time and never bought back in the corpus: the
//! sounds whose pitch **is** behaviour stopped being compared on it. The lever's
//! `powered ? 0.6f : 0.5f` and the comparator's `SUBTRACT ? 0.55f : 0.5f` are
//! the whole difference between the two directions of those events — same event
//! id, same source, same position, same volume — so with the pitch gone a
//! `sound` observation of a lever carries no evidence about **which way the
//! lever went**.
//!
//! # Why this is not a revert
//!
//! The original reason still holds, so the fix is not to compare the float
//! again. It is to compare **what vanilla's call site actually determines**,
//! which is different per event and is read here from the pinned jar:
//!
//! * a pitch that is a literal at the call site is compared **exactly**;
//! * a pitch vanilla **draws** is compared as the half-open **range it is drawn
//!   from**, which is the part of it that is behaviour;
//! * an event this table does not classify keeps `569323e`'s behaviour and is
//!   dropped — so nothing that is green today can go red for want of an entry.
//!
//! Exactness is safe for the first class and that is a property of the
//! instrument, not an assumption: both sides' pitch arrives as the **same four
//! wire bytes** of an `f32`, decoded by the same `read_f32`, so a literal
//! `0.6f` on each side compares equal without any tolerance. Nothing here
//! computes a float and hopes two languages round it the same way — see the
//! `deliberately unclassified` block at the foot of [`classify`] for the one
//! event where vanilla *does* compute one, and which is left ungraded for it.
//!
//! # The ranges are not one range, which is why this is a table
//!
//! Measured off the corpus's own recordings and then read out of the jar: the
//! sound events the corpus emits draw their pitches from **three different
//! formulas with three different ranges**. Any rule keyed on the *value* —
//! "treat anything in `[0.9, 1.0)` as drawn" — is therefore wrong by
//! construction, because a drawn flint-and-steel at 0.95 and a drawn chest at
//! 0.95 would be indistinguishable from each other and from a lever that is
//! simply broken.
//!
//! ```text
//! ChestBlockEntity.playSound        116-127  nextFloat() * 0.1f + 0.9f   [0.9, 1.0)
//! DoorBlock.playSound                33-46   nextFloat() * 0.1f + 0.9f   [0.9, 1.0)
//! FenceGateBlock.useWithoutItem     151-162  nextFloat() * 0.1f + 0.9f   [0.9, 1.0)
//! FlintAndSteelItem.useOn            63-74   nextFloat() * 0.4f + 0.8f   [0.8, 1.2)
//! LivingEntity.getVoicePitch         39-59   (nextFloat() - nextFloat())
//!                                              * 0.2f + 1.0f             (0.8, 1.2)
//! ```
//!
//! Read with `ci/sound-pitch-callsites.sh <class> <method>`, which is the only
//! route by which an entry may be added to this file.

/// What vanilla's call site determines about one event's pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Class {
    /// The pitch is a literal at the call site. Compare it exactly.
    Constant,
    /// Vanilla draws it. Compare the range, which is the part that is
    /// behaviour; `label` is what goes in the observation in place of the
    /// number, so two runs that drew differently still read as equal.
    Drawn {
        lo: f32,
        hi: f32,
        label: &'static str,
    },
}

/// `nextFloat() * 0.1f + 0.9f` — the door/trapdoor/fence-gate/chest family.
///
/// Named for the formula rather than for any one block because five call sites
/// write it character for character. `crates/rustmc-server/src/sounds.rs`'s
/// `nine_tenths_pitch` is the same constant from the other side.
const NINE_TENTHS: Class = Class::Drawn {
    lo: 0.9,
    hi: 1.0,
    label: "drawn:0.9..1.0",
};

/// `nextFloat() * 0.4f + 0.8f` — `FlintAndSteelItem.useOn`, offsets 63-74.
const FLINT: Class = Class::Drawn {
    lo: 0.8,
    hi: 1.2,
    label: "drawn:0.8..1.2",
};

/// `(nextFloat() - nextFloat()) * 0.2f + 1.0f` — `LivingEntity.getVoicePitch`,
/// offsets 39-59, the adult branch.
///
/// The baby branch two offsets up adds `1.5f` instead of `1.0f` and so draws
/// from `(1.3, 1.7)`. **Only the adult range is declared**, because a union of
/// the two would accept a pitch neither branch can produce; a baby mob in the
/// corpus is expected to fall out of range and be compared exactly, which is a
/// visible refusal rather than a silent widening.
const VOICE: Class = Class::Drawn {
    lo: 0.8,
    hi: 1.2,
    label: "drawn:0.8..1.2",
};

/// `nextFloat() * 0.25f + 0.6f` — `PistonBaseBlock`, offsets 131-142, the
/// `PISTON_EXTEND` arm.
///
/// `docs/PARITY-ROADMAP.md`'s gap table had already measured this event at
/// **0.765 and 0.647** and inferred the formula from the two readings; the
/// offsets above are the confirmation from the jar. That row also said the two
/// piston scenarios were "comparable **only** because `569323e` dropped
/// `sound.pitch`", which stops being true here: they are comparable because
/// the range is compared instead of the draw.
const PISTON_EXTEND: Class = Class::Drawn {
    lo: 0.6,
    hi: 0.85,
    label: "drawn:0.6..0.85",
};

/// `nextFloat() * 0.15f + 0.6f` — the same method, offsets 544-556, the
/// `PISTON_CONTRACT` arm.
///
/// **A different range from the extend two arms up**, which is the fifth
/// distinct range in this file and the reason none of it can be keyed on the
/// value: 0.8 is a legitimate extend and an impossible contract.
const PISTON_CONTRACT: Class = Class::Drawn {
    lo: 0.6,
    hi: 0.75,
    label: "drawn:0.6..0.75",
};

/// Classify one sound event by its registry name.
///
/// `None` means unclassified, which is `569323e`'s behaviour: the pitch is read
/// and dropped. That is the safe default and it is deliberately the default,
/// so that adding an event to this table is the only way to change a verdict.
#[must_use]
pub fn classify(name: &str) -> Option<Class> {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);

    // ---- drawn, and each range read from its own call site -----------------
    if matches!(
        n,
        "block.chest.open"
            | "block.chest.close"
            | "block.ender_chest.open"
            | "block.ender_chest.close"
            | "block.barrel.open"
            | "block.barrel.close"
            | "block.shulker_box.open"
            | "block.shulker_box.close"
            | "block.fence_gate.open"
            | "block.fence_gate.close"
    ) || (n.starts_with("block.")
        && (n.ends_with("_door.open")
            || n.ends_with("_door.close")
            || n.ends_with("_trapdoor.open")
            || n.ends_with("_trapdoor.close")
            || n.ends_with("_fence_gate.open")
            || n.ends_with("_fence_gate.close")))
    {
        return Some(NINE_TENTHS);
    }
    if n == "item.flintandsteel.use" {
        return Some(FLINT);
    }
    if n == "block.piston.extend" {
        return Some(PISTON_EXTEND);
    }
    if n == "block.piston.contract" {
        return Some(PISTON_CONTRACT);
    }
    // Mob voices. `LivingEntity.getVoicePitch` is the only route to these and
    // it always draws, so the family is classified rather than enumerated.
    if n.starts_with("entity.")
        && (n.ends_with(".ambient") || n.ends_with(".hurt") || n.ends_with(".death"))
    {
        return Some(VOICE);
    }

    // ---- constant ----------------------------------------------------------
    //
    // `BlockItem.place` offsets 234-240 passes `SoundType.getPitch() * 0.8f`
    // and `Entity.playStepSound` offsets 14-22 passes `SoundType.getPitch()`.
    // Neither touches a `RandomSource`, and a `SoundType`'s pitch is a literal
    // in its constructor, so every placement and every footstep in the corpus
    // has a pitch that is a function of the block alone.
    if n.starts_with("block.") && (n.ends_with(".place") || n.ends_with(".step")) {
        return Some(Class::Constant);
    }
    // The click family, which is the whole point of this file.
    //
    //   LeverBlock.useWithoutItem       state.getValue(POWERED) ? 0.6f : 0.5f
    //   ComparatorBlock.useWithoutItem  MODE == SUBTRACT       ? 0.55f : 0.5f
    //   ButtonBlock.playSound           the four-argument LevelAccessor
    //                                   overload, which is 1.0f and 1.0f
    if n == "block.lever.click" || n == "block.comparator.click" {
        return Some(Class::Constant);
    }
    if n.starts_with("block.")
        && (n.ends_with("button.click_on") || n.ends_with("button.click_off"))
    {
        return Some(Class::Constant);
    }
    // The workstation take sounds, all `Level.playSound(null, pos, event,
    // BLOCKS, 1.0f, 1.0f)` — `fconst_1; fconst_1` in `StonecutterMenu$2`'s and
    // `LoomMenu$6`'s `lambda$onTake$0`.
    if n == "ui.stonecutter.take_result" || n == "ui.loom.take_result" {
        return Some(Class::Constant);
    }
    // `EnchantmentMenu.lambda$clickMenuButton$0` offsets 222-241:
    // `level.getRandom().nextFloat() * 0.1f + 0.9f` — the chest's range.
    if n == "block.enchantment_table.use" {
        return Some(NINE_TENTHS);
    }

    // ---- deliberately unclassified ----------------------------------------
    //
    // `block.note_block.*`. `NoteBlock.playNote` reaches
    // `Math.pow(2.0d, (note - 12) / 12.0d)` and narrows the result to a float.
    // That is deterministic, so it is *not* a draw and the range treatment
    // would be wrong — but `Math.pow` is specified to within 1 ulp rather than
    // correctly rounded, so an exact comparison would be asserting that two
    // languages' libm agree bit for bit. Neither arm of this file is honest
    // about it, so it gets neither; `docs/PARITY-ROADMAP.md` carries it as the
    // one sound this instrument still cannot grade.
    None
}

/// The observation value for a pitch that arrived on the wire.
///
/// `None` keeps the field out of the observation entirely, which is what an
/// unclassified event does and what every event did before this module.
#[must_use]
pub fn observed(name: &str, pitch: f32) -> Option<serde_json::Value> {
    match classify(name)? {
        Class::Constant => Some(serde_json::json!(f64::from(pitch))),
        // Out of range is **not** folded into the label. A server that answers
        // a chest-open with a constant 1.0 has to be able to fail here, and it
        // can only fail if the number survives.
        Class::Drawn { lo, hi, label } => Some(if pitch >= lo && pitch < hi {
            serde_json::json!(label)
        } else {
            serde_json::json!(f64::from(pitch))
        }),
    }
}

/// The `minecraft:sound_event` registry, id to name, installed once.
///
/// # Why this is a global and not another parameter
///
/// `trace::decode` already carries the block-state and item registries as
/// arguments, and a third would have to be threaded through `observe`, `pump`
/// and both of `pump`'s callers to reach the one arm that wants it. This
/// registry is a property of the **pinned version**, fixed for the life of the
/// process and identical for every caller, so a `OnceLock` states that
/// correctly where a parameter would only state it repeatedly.
///
/// **Not installing it is the safe direction.** `by_id` then answers `None`
/// for every event, which is exactly `569323e`'s behaviour, so a harness that
/// somehow ran without the registry loses this coverage rather than inventing
/// a verdict.
static REGISTRY: std::sync::OnceLock<std::collections::BTreeMap<i32, String>> =
    std::sync::OnceLock::new();

/// Install the sound registry. Later calls are ignored; the first wins.
pub fn install_registry(map: std::collections::BTreeMap<i32, String>) {
    let _ = REGISTRY.set(map);
}

/// How many of the installed registry's events this table classifies.
///
/// Reported rather than asserted: the uncovered remainder is the honest
/// statement of what a `sound` observation still cannot grade, and it can only
/// be driven down by adding call sites read out of the jar.
#[must_use]
pub fn coverage() -> (usize, usize) {
    REGISTRY.get().map_or((0, 0), |m| {
        (
            m.values().filter(|n| classify(n).is_some()).count(),
            m.len(),
        )
    })
}

/// The observation value for a pitch, resolving the event through the
/// installed registry. `None` whenever the event cannot be named or is not
/// classified — in both cases the field is left out, as it was before.
#[must_use]
pub fn observed_by_id(id: i32, pitch: f32) -> Option<serde_json::Value> {
    observed(REGISTRY.get()?.get(&id)?, pitch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The lever's two directions differ in nothing but the pitch, and this
    /// is the assertion that they are now distinguishable.**
    ///
    /// `LeverBlock.useWithoutItem` emits one event id at one position with one
    /// volume whichever way the lever went. Before this module the whole
    /// observation was identical for both, so a `match` on a lever scenario
    /// was compatible with a server that always turned it off.
    #[test]
    fn the_levers_two_directions_are_different_observations() {
        let on = observed("minecraft:block.lever.click", 0.6).unwrap();
        let off = observed("minecraft:block.lever.click", 0.5).unwrap();
        assert_ne!(on, off, "0.6 and 0.5 must not collapse to one value");
    }

    /// The same for the comparator, whose 0.55/0.5 is what claim 420 is named
    /// for.
    #[test]
    fn the_comparators_two_modes_are_different_observations() {
        let subtract = observed("minecraft:block.comparator.click", 0.55).unwrap();
        let compare = observed("minecraft:block.comparator.click", 0.5).unwrap();
        assert_ne!(subtract, compare);
        // And it is not the lever's pair, which is the other half of 420's
        // claim: the two blocks are keyed on different properties and use
        // different constants.
        assert_ne!(
            subtract,
            observed("minecraft:block.lever.click", 0.6).unwrap()
        );
    }

    /// **Two chest opens that drew different pitches are the same
    /// observation** — the property `569323e` dropped the field to get, kept
    /// here without dropping it.
    #[test]
    fn a_drawn_pitch_does_not_make_two_runs_disagree() {
        // The five pitches the 2026-08-07 corpus run actually recorded.
        let drawn = [0.9120, 0.9268, 0.9339, 0.9552, 0.9952];
        let first = observed("minecraft:block.chest.open", drawn[0]).unwrap();
        for p in drawn {
            assert_eq!(
                observed("minecraft:block.chest.open", p).unwrap(),
                first,
                "{p} is inside [0.9, 1.0) and must read as the same observation"
            );
        }
    }

    /// **And the range is still a check.** This is the half a blanket drop
    /// cannot give you: a server that answers a chest-open with a constant
    /// outside the draw is caught, because the number survives.
    #[test]
    fn a_constant_outside_the_draw_is_still_caught() {
        let real = observed("minecraft:block.chest.open", 0.94).unwrap();
        for wrong in [1.0_f32, 0.8, 0.5, 1.5] {
            assert_ne!(
                observed("minecraft:block.chest.open", wrong).unwrap(),
                real,
                "{wrong} is outside [0.9, 1.0) and must not read as the draw"
            );
        }
    }

    /// The draw families have **five** different ranges, and conflating them
    /// is the error this table exists to prevent. A flint-and-steel at 1.05 is
    /// a legitimate draw; a chest at 1.05 is not. A piston at 0.8 is a
    /// legitimate extend and an impossible contract, and those two are the
    /// same block.
    #[test]
    fn the_draw_ranges_are_not_interchangeable() {
        assert_eq!(
            observed("minecraft:item.flintandsteel.use", 1.05).unwrap(),
            serde_json::json!("drawn:0.8..1.2")
        );
        assert_eq!(
            observed("minecraft:block.chest.open", 1.05).unwrap(),
            serde_json::json!(f64::from(1.05_f32)),
            "1.05 is outside the chest's draw and must survive as a number"
        );
        // 0.765 and 0.647 are the two readings PARITY-ROADMAP's gap table
        // recorded for `block.piston.extend` before the formula was read out
        // of the jar. Both must now read as its draw.
        for p in [0.765_f32, 0.647] {
            assert_eq!(
                observed("minecraft:block.piston.extend", p).unwrap(),
                serde_json::json!("drawn:0.6..0.85"),
                "{p} was measured off a real corpus run"
            );
        }
        assert_eq!(
            observed("minecraft:block.piston.contract", 0.8).unwrap(),
            serde_json::json!(f64::from(0.8_f32)),
            "0.8 is outside the contract's [0.6, 0.75) even though the extend \
             at the same block can produce it"
        );
    }

    /// An unclassified event keeps `569323e`'s behaviour exactly. This is what
    /// makes the change unable to redden a scenario for want of a table entry.
    #[test]
    fn an_unclassified_event_still_drops_its_pitch() {
        assert_eq!(classify("minecraft:block.note_block.harp"), None);
        assert_eq!(observed("minecraft:block.note_block.harp", 1.0), None);
        assert_eq!(observed("minecraft:entity.generic.explode", 1.0), None);
    }

    /// Placement and footstep pitches are constants of the block, so they are
    /// compared. These are the corpus's two commonest sounds by a wide margin
    /// — `block.grass.step` alone appears 2,082 times across the recordings on
    /// this box — so classifying them is most of the coverage this buys.
    #[test]
    fn placement_and_step_pitches_are_constants() {
        assert_eq!(
            classify("minecraft:block.stone.place"),
            Some(Class::Constant)
        );
        assert_eq!(
            classify("minecraft:block.grass.step"),
            Some(Class::Constant)
        );
        assert_eq!(
            classify("minecraft:block.wood.place"),
            Some(Class::Constant)
        );
    }

    /// The door family draws, and it is reached by suffix rather than by
    /// enumeration because every wood type has its own event.
    #[test]
    fn every_wooden_door_draws_from_the_same_range() {
        for n in [
            "minecraft:block.wooden_door.open",
            "minecraft:block.cherry_wood_door.close",
            "minecraft:block.copper_trapdoor.open",
            "minecraft:block.fence_gate.open",
        ] {
            assert_eq!(classify(n), Some(NINE_TENTHS), "{n}");
        }
    }
}
