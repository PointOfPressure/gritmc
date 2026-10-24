//! `SculkPatchFeature.place` and the worldgen `SculkSpreader`, ported
//! method for method from the 26.2 bytecode with every write made
//! immediately, the way vanilla's `level.setBlock` makes it.
//!
//! Reference: `SculkPatchFeature.place`, `SculkSpreader.updateCursors`,
//! `SculkSpreader.addCursors`, `SculkSpreader$ChargeCursor` (`update`,
//! `getValidMovementPos`, `isMovementUnobstructed`, `isPosUnreasonable`), the
//! `SculkBehaviour` interface and its three implementations — `SculkBlock`,
//! `SculkVeinBlock`, and the anonymous `SculkBehaviour.DEFAULT` that every
//! other block falls to — and `MultifaceBlock.availableFaces`.
//!
//! # Why this replaced the earlier port
//!
//! The earlier one (`sculk_spreader::ChargeCursor::update_worldgen` and the
//! old `sculk_patch::place`) collected growths and conversions and wrote
//! them after the patch, and departed from the bytecode in eleven places, any
//! of which moves every later draw of the patch:
//!
//! 1. the catalyst went on `origin.below()`; `setBlock` takes `origin`
//!    (`aload_3` at offset 192) — only the full-block *test* is below;
//! 2. a cursor on a block that is neither sculk nor vein ran `SculkBlock`'s
//!    charge use; vanilla runs `SculkBehaviour.DEFAULT`, which spends no draw
//!    and keeps the charge while `decayDelay > 0`;
//! 3. `DEFAULT.updateDecayDelay` is `max(d - 1, 0)`; the port pinned it at 1;
//! 4. the worldgen cursor kill is `closerThan(new Vec3i(origin.x, pos.y,
//!    origin.z), 15.0)` — horizontal only, and strict (`distSqr < 225`); the
//!    port measured in 3D and killed only past 225;
//! 5. `SculkBlock`'s no-growth radius is `closerThan(origin, 1)`, strict, so
//!    only the origin itself; the port's `<=` took the six faces too;
//! 6. `sculk_catalyst` was treated as a `SculkBehaviour`; only `sculk` and
//!    `sculk_vein` implement it;
//! 7. `hasSubstrateAccess` reads `BlockTags.SCULK_REPLACEABLE`, not the
//!    worldgen tag;
//! 8. growths were written after the patch, so `canPlaceGrowth`'s crowding
//!    count never saw a sensor this patch had grown;
//! 9. veins were discharged after the patch from a list of conversions;
//!    vanilla discharges a vein when its cursor leaves it or dies on it, and
//!    a conversion's neighbours (all but the face it came through) as it
//!    happens;
//! 10. a cursor's `facings` — what `DEFAULT.attemptSpreadVein` regrows a
//!     vein from — did not exist;
//! 11. `DEFAULT.attemptSpreadVein` with no facings uses the vein's
//!     *same-space* spreader (`SpreadType.SAME_POSITION` only).
//!
//! # What this module does not model
//!
//! `levelEvent`, `playSound` and `pushEntitiesUp` — no world effect during
//! generation — and the catalyst's runtime merge branch, which
//! `isWorldGeneration` skips.

use super::sculk_patch::SculkPatchConfig;
use super::sculk_spreader::{
    can_place_growth, get_decay_penalty, has_face, is_movement_unobstructed,
    randomized_non_corner_neighbour_offsets, Face, SculkSpreader, SpreadWorld,
};
use crate::rng::RandomSource;

/// `Direction.values()`.
const DIRECTIONS: [Face; 6] = [
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// `Blocks.SCULK_VEIN.defaultBlockState()`.
pub const VEIN_DEFAULT: &str = "minecraft:sculk_vein[down=false,east=false,north=false,south=false,up=false,waterlogged=false,west=false]";

/// What the worldgen sculk path asks of the level. Every write is visible
/// to the next read.
pub trait SculkLevel: SpreadWorld {
    fn block_at(&mut self, pos: (i32, i32, i32)) -> String;
    fn set_block(&mut self, pos: (i32, i32, i32), state: &str);
    /// The fluid's registry name: `minecraft:empty`, `minecraft:water`
    /// (source), `minecraft:flowing_water`, ...
    fn fluid_at(&mut self, pos: (i32, i32, i32)) -> String;
    /// `BlockState#isAir`.
    fn is_air(&mut self, pos: (i32, i32, i32)) -> bool;
    /// `BlockState#isCollisionShapeFullBlock`.
    fn is_collision_full_block(&mut self, pos: (i32, i32, i32)) -> bool;
    /// `level.getBlockState(pos).is(tag)`.
    fn in_tag(&mut self, tag: &str, pos: (i32, i32, i32)) -> bool;
    /// `MultifaceBlock.canAttachTo(level, pos, direction)`.
    fn can_attach_to(&mut self, pos: (i32, i32, i32), face: Face) -> bool;
    /// `SculkVeinBlock.getSpreader()` (or `getSameSpaceSpreader()` when
    /// `same_space`) `.spreadAll(state, level, pos, true)`.
    fn vein_spread_all(&mut self, state: &str, pos: (i32, i32, i32), same_space: bool) -> u64;
}

fn plain(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

fn step(pos: (i32, i32, i32), face: Face) -> (i32, i32, i32) {
    let (dx, dy, dz) = face.offset();
    (pos.0 + dx, pos.1 + dy, pos.2 + dz)
}

/// Which `SculkBehaviour` a block state answers to: `getBlock() instanceof
/// SculkBehaviour ? block : DEFAULT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    Sculk,
    Vein,
    Default,
}

impl Behaviour {
    #[must_use]
    pub fn of(state: &str) -> Self {
        match plain(state) {
            "minecraft:sculk" => Self::Sculk,
            "minecraft:sculk_vein" => Self::Vein,
            _ => Self::Default,
        }
    }
}

/// `MultifaceBlock.availableFaces(state)`: the faces a vein has, in
/// `Direction` order (it is an `EnumSet`); empty for anything that is not a
/// multiface block.
#[must_use]
pub fn available_faces(state: &str) -> Vec<Face> {
    if plain(state) != "minecraft:sculk_vein" {
        return Vec::new();
    }
    DIRECTIONS
        .into_iter()
        .filter(|&f| has_face(state, f))
        .collect()
}

/// A `sculk_vein` state with the given faces and waterlogging, properties in
/// the canonical (alphabetical) order.
fn vein_state(faces: &[Face], waterlogged: bool) -> String {
    let on = |f: Face| faces.contains(&f);
    format!(
        "minecraft:sculk_vein[down={},east={},north={},south={},up={},waterlogged={waterlogged},west={}]",
        on(Face::Down),
        on(Face::East),
        on(Face::North),
        on(Face::South),
        on(Face::Up),
        on(Face::West)
    )
}

/// `SculkSpreader$ChargeCursor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub pos: (i32, i32, i32),
    pub charge: i32,
    pub decay_delay: i32,
    pub update_delay: i32,
    /// `facings`, `null` until the cursor first stands on a sculk behaviour.
    pub facings: Option<Vec<Face>>,
}

impl Cursor {
    /// `new ChargeCursor(pos, charge)` → `(pos, charge, 1, 0, Optional.empty())`.
    #[must_use]
    pub const fn new(pos: (i32, i32, i32), charge: i32) -> Self {
        Self {
            pos,
            charge,
            decay_delay: 1,
            update_delay: 0,
            facings: None,
        }
    }

    /// `isPosUnreasonable`: `pos.distChessboard(origin) > 1024`.
    #[must_use]
    pub fn is_pos_unreasonable(&self, origin: (i32, i32, i32)) -> bool {
        (self.pos.0 - origin.0)
            .abs()
            .max((self.pos.1 - origin.1).abs())
            .max((self.pos.2 - origin.2).abs())
            > 1024
    }

    /// `ChargeCursor.update(level, origin, random, spreader, shouldConvert)`.
    pub fn update<W, R>(
        &mut self,
        spreader: &SculkSpreader,
        world: &mut W,
        origin: (i32, i32, i32),
        random: &mut R,
        should_convert: bool,
    ) where
        W: SculkLevel + ?Sized,
        R: RandomSource + ?Sized,
    {
        // `shouldUpdate`: charge > 0, and during world generation that is all.
        if self.charge <= 0 || !spreader.is_world_generation {
            return;
        }
        if self.update_delay > 0 {
            self.update_delay -= 1;
            return;
        }
        let mut state = world.block_at(self.pos);
        let mut behaviour = Behaviour::of(&state);
        if should_convert
            && attempt_spread_vein(behaviour, world, self.pos, &state, self.facings.as_deref())
            && behaviour != Behaviour::Sculk
        {
            // `canChangeBlockStateOnSpread()`: false for `SculkBlock` only.
            state = world.block_at(self.pos);
            behaviour = Behaviour::of(&state);
        }
        self.charge = attempt_use_charge(
            behaviour,
            self,
            world,
            origin,
            random,
            spreader,
            should_convert,
        );
        if self.charge <= 0 {
            on_discharged(behaviour, world, &state, self.pos);
            return;
        }
        if let Some(next) = valid_movement_pos(world, self.pos, random) {
            on_discharged(behaviour, world, &state, self.pos);
            self.pos = next;
            let (dx, dz) = (next.0 - origin.0, next.2 - origin.2);
            if spreader.is_world_generation && dx * dx + dz * dz >= 225 {
                self.charge = 0;
                return;
            }
            state = world.block_at(next);
        }
        if Behaviour::of(&state) != Behaviour::Default {
            self.facings = Some(available_faces(&state));
        }
        // The behaviour read at the top, not the one at the new position.
        self.decay_delay = match behaviour {
            Behaviour::Default => (self.decay_delay - 1).max(0),
            _ => 1,
        };
        self.update_delay = 1;
    }
}

/// `SculkBehaviour.attemptSpreadVein`, by behaviour.
fn attempt_spread_vein<W: SculkLevel + ?Sized>(
    behaviour: Behaviour,
    world: &mut W,
    pos: (i32, i32, i32),
    state: &str,
    facings: Option<&[Face]>,
) -> bool {
    match (behaviour, facings) {
        // The interface default: the vein spreader, from this state.
        (Behaviour::Sculk | Behaviour::Vein, _) | (Behaviour::Default, Some([])) => {
            world.vein_spread_all(state, pos, false) > 0
        }
        // `DEFAULT`, no facings yet: the same-space spreader, from the block
        // read again.
        (Behaviour::Default, None) => {
            let here = world.block_at(pos);
            world.vein_spread_all(&here, pos, true) > 0
        }
        // `DEFAULT` with facings: regrow a vein, but only into air or water.
        (Behaviour::Default, Some(faces)) => {
            if world.is_air(pos) || world.fluid_at(pos) == "minecraft:water" {
                regrow(world, pos, faces)
            } else {
                false
            }
        }
    }
}

/// `SculkVeinBlock.regrow(level, pos, state, faces)`.
fn regrow<W: SculkLevel + ?Sized>(world: &mut W, pos: (i32, i32, i32), faces: &[Face]) -> bool {
    let attached: Vec<Face> = faces
        .iter()
        .copied()
        .filter(|&f| world.can_attach_to(pos, f))
        .collect();
    if attached.is_empty() {
        return false;
    }
    let waterlogged = world.fluid_at(pos) != "minecraft:empty";
    world.set_block(pos, &vein_state(&attached, waterlogged));
    true
}

/// `SculkBehaviour.attemptUseCharge`, by behaviour.
fn attempt_use_charge<W, R>(
    behaviour: Behaviour,
    cursor: &Cursor,
    world: &mut W,
    origin: (i32, i32, i32),
    random: &mut R,
    spreader: &SculkSpreader,
    should_convert: bool,
) -> i32
where
    W: SculkLevel + ?Sized,
    R: RandomSource + ?Sized,
{
    let charge = cursor.charge;
    match behaviour {
        Behaviour::Default => {
            if cursor.decay_delay > 0 {
                charge
            } else {
                0
            }
        }
        Behaviour::Vein => {
            if should_convert && attempt_place_sculk(world, cursor.pos, random) {
                return charge - 1;
            }
            if random.next_i32_bound(spreader.charge_decay_rate) == 0 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                {
                    (charge as f32 * 0.5).floor() as i32
                }
            } else {
                charge
            }
        }
        Behaviour::Sculk => {
            if charge == 0 || random.next_i32_bound(spreader.charge_decay_rate) != 0 {
                return charge;
            }
            let pos = cursor.pos;
            // `pos.closerThan(origin, noGrowthRadius)`: distSqr < r², strict.
            let d = (pos.0 - origin.0, pos.1 - origin.1, pos.2 - origin.2);
            let near = d.0 * d.0 + d.1 * d.1 + d.2 * d.2
                < spreader.no_growth_radius * spreader.no_growth_radius;
            if near || !growth_fits(world, pos) {
                if random.next_i32_bound(spreader.additional_decay_rate) != 0 {
                    return charge;
                }
                return charge
                    - if near {
                        1
                    } else {
                        get_decay_penalty(spreader.no_growth_radius, pos, origin, charge)
                    };
            }
            let cost = spreader.growth_spawn_cost;
            if random.next_i32_bound(cost) < charge {
                let above = (pos.0, pos.1 + 1, pos.2);
                // `getRandomGrowthState`: one draw; waterlogged if the
                // position above holds any fluid.
                let shrieker = random.next_i32_bound(11) == 0;
                let wet = world.fluid_at(above) != "minecraft:empty";
                let state = if shrieker {
                    format!(
                        "minecraft:sculk_shrieker[can_summon={},shrieking=false,waterlogged={wet}]",
                        spreader.is_world_generation
                    )
                } else {
                    format!("minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged={wet}]")
                };
                world.set_block(above, &state);
            }
            (charge - cost).max(0)
        }
    }
}

/// `SculkBlock.canPlaceGrowth`, through the shared implementation.
fn growth_fits<W: SculkLevel + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    struct View<'a, W: ?Sized>(&'a mut W);
    impl<W: SculkLevel + ?Sized> SpreadWorld for View<'_, W> {
        fn is_face_sturdy(&mut self, pos: (i32, i32, i32), face: Face) -> bool {
            self.0.is_face_sturdy(pos, face)
        }
    }
    impl<W: SculkLevel + ?Sized> super::sculk_spreader::SculkWorld for View<'_, W> {
        fn block_at(&mut self, pos: (i32, i32, i32)) -> String {
            self.0.block_at(pos)
        }
        fn is_sculk_replaceable(&mut self, pos: (i32, i32, i32)) -> bool {
            self.0.in_tag("minecraft:sculk_replaceable_world_gen", pos)
        }
        fn set_block(&mut self, pos: (i32, i32, i32), state: &str) {
            self.0.set_block(pos, state);
        }
    }
    can_place_growth(&mut View(world), pos)
}

/// `SculkVeinBlock.attemptPlaceSculk(spreader, level, pos, random)`.
fn attempt_place_sculk<W, R>(world: &mut W, pos: (i32, i32, i32), random: &mut R) -> bool
where
    W: SculkLevel + ?Sized,
    R: RandomSource + ?Sized,
{
    let state = world.block_at(pos);
    // `Direction.allShuffled(random)`: drawn whole, before any test.
    for index in crate::structure::processor::shuffled_order(random, 6) {
        let face = DIRECTIONS[index];
        if !has_face(&state, face) {
            continue;
        }
        let target = step(pos, face);
        if !world.in_tag("minecraft:sculk_replaceable_world_gen", target) {
            continue;
        }
        world.set_block(target, "minecraft:sculk");
        world.vein_spread_all("minecraft:sculk", target, false);
        let from = face.opposite();
        for direction in DIRECTIONS {
            if direction == from {
                continue;
            }
            let neighbour = step(target, direction);
            let there = world.block_at(neighbour);
            if plain(&there) == "minecraft:sculk_vein" {
                on_discharged(Behaviour::Vein, world, &there, neighbour);
            }
        }
        return true;
    }
    false
}

/// `SculkBehaviour.onDischarged`: `SculkVeinBlock`'s, or nothing.
///
/// The state is the one the caller holds, which may be stale: vanilla tests
/// `state.is(this)` and computes the new state from it, then writes it over
/// whatever is at `pos` — always, even when no face changed.
fn on_discharged<W: SculkLevel + ?Sized>(
    behaviour: Behaviour,
    world: &mut W,
    state: &str,
    pos: (i32, i32, i32),
) {
    if behaviour != Behaviour::Vein || plain(state) != "minecraft:sculk_vein" {
        return;
    }
    let mut faces = available_faces(state);
    faces.retain(|&f| plain(&world.block_at(step(pos, f))) != "minecraft:sculk");
    let next = if faces.is_empty() {
        if world.fluid_at(pos) == "minecraft:empty" {
            "minecraft:air".to_string()
        } else {
            "minecraft:water[level=0]".to_string()
        }
    } else {
        vein_state(&faces, state.contains("waterlogged=true"))
    };
    world.set_block(pos, &next);
}

/// `ChargeCursor.getValidMovementPos(level, pos, random)`.
fn valid_movement_pos<W, R>(
    world: &mut W,
    pos: (i32, i32, i32),
    random: &mut R,
) -> Option<(i32, i32, i32)>
where
    W: SculkLevel + ?Sized,
    R: RandomSource + ?Sized,
{
    let mut best = pos;
    for (dx, dy, dz) in randomized_non_corner_neighbour_offsets(random) {
        let candidate = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
        let state = world.block_at(candidate);
        if Behaviour::of(&state) == Behaviour::Default {
            continue;
        }
        if !is_movement_unobstructed(world, pos, candidate) {
            continue;
        }
        best = candidate;
        if has_substrate_access(world, &state, candidate) {
            break;
        }
    }
    (best != pos).then_some(best)
}

/// `SculkVeinBlock.hasSubstrateAccess(level, state, pos)` — against
/// `BlockTags.SCULK_REPLACEABLE`, the runtime tag.
fn has_substrate_access<W: SculkLevel + ?Sized>(
    world: &mut W,
    state: &str,
    pos: (i32, i32, i32),
) -> bool {
    if plain(state) != "minecraft:sculk_vein" {
        return false;
    }
    DIRECTIONS
        .into_iter()
        .any(|f| has_face(state, f) && world.in_tag("minecraft:sculk_replaceable", step(pos, f)))
}

/// `SculkPatchFeature.canSpreadFrom(level, pos)`.
fn can_spread_from<W: SculkLevel + ?Sized>(world: &mut W, pos: (i32, i32, i32)) -> bool {
    let state = world.block_at(pos);
    if Behaviour::of(&state) != Behaviour::Default {
        return true;
    }
    let source_water =
        plain(&state) == "minecraft:water" && world.fluid_at(pos) == "minecraft:water";
    if !world.is_air(pos) && !source_water {
        return false;
    }
    DIRECTIONS
        .into_iter()
        .any(|f| world.is_collision_full_block(step(pos, f)))
}

/// What one patch did, for counters and tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatchOutcome {
    pub catalyst: bool,
    pub rare_growths: u32,
}

/// `SculkPatchFeature.place(context)`. `None` when `canSpreadFrom` refuses,
/// which spends no draw.
pub fn place<W, R>(
    world: &mut W,
    config: &SculkPatchConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Option<PatchOutcome>
where
    W: SculkLevel + ?Sized,
    R: RandomSource + ?Sized,
{
    if !can_spread_from(world, origin) {
        return None;
    }
    let spreader = SculkSpreader::world_gen();
    let rounds = config.spread_rounds + config.growth_rounds;
    let mut cursors: Vec<Cursor> = Vec::new();
    for round in 0..rounds {
        for _ in 0..config.charge_count {
            // `addCursors(origin, amountPerCharge)`, and `addCursor` drops
            // past 32.
            let mut charge = config.amount_per_charge;
            while charge > 0 {
                let amount = charge.min(super::sculk_spreader::MAX_CHARGE);
                if cursors.len() < super::sculk_spreader::MAX_CURSORS {
                    cursors.push(Cursor::new(origin, amount));
                }
                charge -= amount;
            }
        }
        let convert = round < config.spread_rounds;
        for _ in 0..config.spread_attempts {
            // `updateCursors`, worldgen: no merging; a spent cursor is
            // dropped, an unreasonable one skipped and dropped.
            let mut next = Vec::with_capacity(cursors.len());
            for mut cursor in cursors.drain(..) {
                if cursor.is_pos_unreasonable(origin) {
                    continue;
                }
                cursor.update(&spreader, world, origin, random, convert);
                if cursor.charge > 0 {
                    next.push(cursor);
                }
            }
            cursors = next;
        }
        cursors.clear();
    }

    let mut outcome = PatchOutcome::default();
    let below = (origin.0, origin.1 - 1, origin.2);
    if random.next_f32() <= config.catalyst_chance && world.is_collision_full_block(below) {
        world.set_block(origin, "minecraft:sculk_catalyst[bloom=false]");
        outcome.catalyst = true;
    }
    let extra = config.extra_rare_growths.sample(random);
    for _ in 0..extra {
        let dx = random.next_i32_bound(5) - 2;
        let dz = random.next_i32_bound(5) - 2;
        let at = (origin.0 + dx, origin.1, origin.2 + dz);
        if world.is_air(at) && world.is_face_sturdy((at.0, at.1 - 1, at.2), Face::Up) {
            world.set_block(
                at,
                "minecraft:sculk_shrieker[can_summon=true,shrieking=false,waterlogged=false]",
            );
            outcome.rare_growths += 1;
        }
    }
    Some(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use std::collections::BTreeMap;

    /// A cave: `solid` cells are deepslate (full, sturdy, replaceable),
    /// everything else air unless written.
    struct Cave {
        blocks: BTreeMap<(i32, i32, i32), String>,
    }
    impl Cave {
        fn floor(y: i32, r: i32) -> Self {
            let mut blocks = BTreeMap::new();
            for x in -r..=r {
                for z in -r..=r {
                    blocks.insert((x, y, z), "minecraft:deepslate[axis=y]".to_string());
                }
            }
            Self { blocks }
        }
    }
    impl SpreadWorld for Cave {
        fn is_face_sturdy(&mut self, pos: (i32, i32, i32), _face: Face) -> bool {
            self.blocks
                .get(&pos)
                .is_some_and(|s| matches!(plain(s), "minecraft:deepslate" | "minecraft:sculk"))
        }
    }
    impl SculkLevel for Cave {
        fn block_at(&mut self, pos: (i32, i32, i32)) -> String {
            self.blocks
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".into())
        }
        fn set_block(&mut self, pos: (i32, i32, i32), state: &str) {
            self.blocks.insert(pos, state.to_string());
        }
        fn fluid_at(&mut self, _pos: (i32, i32, i32)) -> String {
            "minecraft:empty".into()
        }
        fn is_air(&mut self, pos: (i32, i32, i32)) -> bool {
            plain(&self.block_at(pos)) == "minecraft:air"
        }
        fn is_collision_full_block(&mut self, pos: (i32, i32, i32)) -> bool {
            self.is_face_sturdy(pos, Face::Up)
        }
        fn in_tag(&mut self, _tag: &str, pos: (i32, i32, i32)) -> bool {
            plain(&self.block_at(pos)) == "minecraft:deepslate"
        }
        fn can_attach_to(&mut self, pos: (i32, i32, i32), face: Face) -> bool {
            let backing = step(pos, face);
            self.is_face_sturdy(backing, face.opposite())
        }
        fn vein_spread_all(&mut self, _state: &str, _pos: (i32, i32, i32), _same: bool) -> u64 {
            0
        }
    }

    /// `SculkPatchFeature.place` puts the catalyst AT the origin
    /// (`setBlock(origin, ...)`, `aload_3` at offset 192); only the
    /// full-block test reads `origin.below()`. Measured in seed 12345's deep
    /// dark: 21 catalysts both vanilla runs agree on, none of ours at the same
    /// position, and half of ours exactly one block under theirs.
    #[test]
    fn the_catalyst_lands_on_the_origin_over_a_full_block() {
        let config = SculkPatchConfig::parse(&serde_json::json!({
            "amount_per_charge": 32, "catalyst_chance": 1.0, "charge_count": 0,
            "extra_rare_growths": 0, "growth_rounds": 0, "spread_attempts": 0, "spread_rounds": 0
        }))
        .expect("parses");
        let mut cave = Cave::floor(-1, 2);
        let mut r = LegacyRandomSource::new(1);
        let out = place(&mut cave, &config, &mut r, (0, 0, 0)).expect("starts");
        assert!(out.catalyst);
        assert_eq!(
            cave.block_at((0, 0, 0)),
            "minecraft:sculk_catalyst[bloom=false]"
        );
        assert_eq!(plain(&cave.block_at((0, -1, 0))), "minecraft:deepslate");
    }

    /// A cursor on a block that is neither sculk nor vein is
    /// `SculkBehaviour.DEFAULT`: it spends no draw, keeps its charge while
    /// `decayDelay > 0`, and `updateDecayDelay` counts that delay down.
    #[test]
    fn a_default_cursor_draws_nothing_and_decays_its_delay() {
        struct Counting(LegacyRandomSource, u32);
        impl RandomSource for Counting {
            fn next_i32(&mut self) -> i32 {
                self.1 += 1;
                self.0.next_i32()
            }
            fn next_i32_bound(&mut self, bound: i32) -> i32 {
                self.1 += 1;
                self.0.next_i32_bound(bound)
            }
            fn next_i64(&mut self) -> i64 {
                self.1 += 1;
                self.0.next_i64()
            }
            fn next_f32(&mut self) -> f32 {
                self.1 += 1;
                self.0.next_f32()
            }
            fn next_f64(&mut self) -> f64 {
                self.1 += 1;
                self.0.next_f64()
            }
            fn next_bool(&mut self) -> bool {
                self.1 += 1;
                self.0.next_bool()
            }
            fn next_gaussian(&mut self) -> f64 {
                self.1 += 1;
                self.0.next_gaussian()
            }
            fn set_seed(&mut self, seed: i64) {
                self.0.set_seed(seed);
            }
        }
        // Stone everywhere around: no sculk behaviour to move to.
        let mut cave = Cave {
            blocks: BTreeMap::new(),
        };
        cave.set_block((0, 0, 0), "minecraft:stone");
        let spreader = SculkSpreader::world_gen();
        let mut cursor = Cursor::new((0, 0, 0), 20);
        cursor.facings = Some(Vec::new());
        let mut r = Counting(LegacyRandomSource::new(7), 0);
        cursor.update(&spreader, &mut cave, (0, 0, 0), &mut r, false);
        // The movement search shuffles the 18 offsets: 17 draws, nothing else.
        assert_eq!(r.1, 17);
        assert_eq!(cursor.charge, 20, "decayDelay 1 keeps the charge");
        assert_eq!(cursor.decay_delay, 0, "max(1 - 1, 0)");
        assert_eq!(cursor.update_delay, 1);
        cursor.update_delay = 0;
        cursor.update(&spreader, &mut cave, (0, 0, 0), &mut r, false);
        assert_eq!(cursor.charge, 0, "decayDelay 0 spends it");
    }

    /// The worldgen kill is horizontal and strict: `closerThan(new
    /// Vec3i(origin.x, pos.y, origin.z), 15.0)`. A cursor 14 blocks out and
    /// 20 down survives; one at (9,_,12) — distSqr exactly 225 — dies.
    #[test]
    fn the_worldgen_cursor_leash_is_horizontal_and_strict() {
        let spreader = SculkSpreader::world_gen();
        for (pos, alive) in [((13, -20, 0), true), ((8, 0, 12), false)] {
            let mut cave = Cave {
                blocks: BTreeMap::new(),
            };
            // A lone sculk block next to the cursor, so it moves there.
            let to = (pos.0 + 1, pos.1, pos.2);
            cave.set_block(to, "minecraft:sculk");
            cave.set_block(pos, "minecraft:stone");
            let mut cursor = Cursor::new(pos, 20);
            cursor.facings = Some(Vec::new());
            let mut r = LegacyRandomSource::new(3);
            cursor.update(&spreader, &mut cave, (0, 0, 0), &mut r, false);
            assert_eq!(cursor.pos, to);
            assert_eq!(cursor.charge > 0, alive, "{pos:?} -> {to:?}");
        }
    }

    /// `hasSubstrateAccess` and the movement search accept sculk and vein
    /// only; a catalyst is `DEFAULT`.
    #[test]
    fn a_catalyst_is_not_a_sculk_behaviour() {
        assert_eq!(
            Behaviour::of("minecraft:sculk_catalyst[bloom=false]"),
            Behaviour::Default
        );
        assert_eq!(Behaviour::of("minecraft:sculk"), Behaviour::Sculk);
        assert_eq!(Behaviour::of(VEIN_DEFAULT), Behaviour::Vein);
        assert_eq!(
            available_faces("minecraft:sculk_vein[down=true,east=false,north=false,south=false,up=false,waterlogged=false,west=true]"),
            vec![Face::Down, Face::West]
        );
        assert!(available_faces("minecraft:sculk").is_empty());
    }

    /// The ancient city's patch draws `uniform 1..3` rare growths after the
    /// catalyst's float, each at the origin offset by `nextInt(5) - 2` on x
    /// then z, and places a `can_summon` shrieker only on air over a sturdy
    /// top face.
    #[test]
    fn rare_growths_land_on_air_over_a_sturdy_floor() {
        let config = SculkPatchConfig::parse(&serde_json::json!({
            "amount_per_charge": 32, "catalyst_chance": 0.0, "charge_count": 0,
            "extra_rare_growths": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 3},
            "growth_rounds": 0, "spread_attempts": 0, "spread_rounds": 0
        }))
        .expect("parses");
        let mut placed = 0;
        for seed in 0..50 {
            let mut cave = Cave::floor(-1, 2);
            let mut r = LegacyRandomSource::new(seed);
            let out = place(&mut cave, &config, &mut r, (0, 0, 0)).expect("starts");
            assert!((1..=3).contains(&out.rare_growths));
            let shriekers = cave
                .blocks
                .iter()
                .filter(|(p, s)| {
                    plain(s) == "minecraft:sculk_shrieker"
                        && p.1 == 0
                        && (-2..=2).contains(&p.0)
                        && (-2..=2).contains(&p.2)
                })
                .count();
            assert!(shriekers >= 1 && shriekers <= out.rare_growths as usize);
            placed += out.rare_growths;
        }
        assert!(placed > 50, "fifty patches placed only {placed}");
    }
}
