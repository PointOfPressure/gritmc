//! `ServerExplosion` — vanilla 26.2's explosion, on the server.
//!
//! Read from `net/minecraft/world/level/ServerExplosion.class`,
//! `Explosion.class`, `ExplosionDamageCalculator.class`,
//! `Level$ExplosionInteraction.class` and `ServerLevel.explode`, with
//! `javap -p -c -constants` under JDK 25. **The 1.20 shape is gone**: in 26.2
//! `Explosion` is an *interface* with eight methods, `ServerExplosion` is the
//! only server implementation, and `BlockInteraction` has gained a fourth
//! value, `TRIGGER_BLOCK`.
//!
//! # The call graph, in the order `ServerExplosion.explode()` runs it
//!
//! ```text
//! ServerLevel.explode(..., ExplosionInteraction, particle, emitter,
//!                     blockParticles, sound)              offsets 0..279
//!   switch on the interaction        -> BlockInteraction  offsets 0..118
//!   new ServerExplosion(...)                              offsets 135..154
//!   ServerExplosion.explode()                             offset  158
//!     level.gameEvent(source, EXPLODE, center)            offset    0
//!     calculateExplodedPositions()                        offset   18
//!     hurtEntities()                                      offset   23
//!     if blockInteraction != KEEP: interactWithBlocks(l)  offsets  27..52
//!     if fire:                     createFire(l)          offsets  58..67
//!     return l.size()                                     offsets  70..76
//!   ClientboundExplodePacket to every player within 4096  offsets 180..279
//! ```
//!
//! The return value is the *position count*, and it is the packet's
//! `blockCount` field. It is counted **before** `interactWithBlocks` removes
//! anything, so it is the size of the candidate set and not the number of
//! blocks that actually broke.
//!
//! # The draw order, which is the whole of the RNG contract
//!
//! One `level.random` stream, three consumers, in this order and no other:
//!
//! 1. `calculateExplodedPositions` — **exactly one `nextFloat()` per ray**,
//!    and a ray is drawn only for a cell on the shell of the 16x16x16 grid.
//!    That is `16^3 - 14^3 = 1352` draws, every time, whatever the radius and
//!    whatever the world contains. The draw happens *after* the shell test
//!    (`ServerExplosion.calculateExplodedPositions`, the `continue` at offset
//!    73 precedes the `nextFloat` at offset 170) and *before* anything reads
//!    a block, so it cannot be skipped by an early exit.
//! 2. `Util.shuffle(list, level.random)` at the head of `interactWithBlocks`
//!    — `size` down to 2, one `nextInt(i)` each. Skipped entirely when the
//!    interaction is `KEEP`, because `interactWithBlocks` is not called.
//! 3. `createFire` — one `nextInt(3)` per candidate position, always drawn
//!    even when the position is not air. Only when `fire` is set.
//!
//! # What this tree cannot run, and it is most of the mechanic
//!
//! There is no entity that ticks in this server — `crate::tick` says so — so
//! `hurtEntities` has nothing to iterate, there is no `PrimedTnt` to prime,
//! no `/summon` to prime it with, and no creeper. The pieces of the entity
//! pass that are pure arithmetic are ported and goldened here; the pieces
//! that need an entity in a world are refused by name in the `refusals`
//! module below.
//!
//! # The table
//!
//! `data/vanilla/block_explosion.json`, from `ci/block-explosion-golden.sh`.
//! Keyed by **block**, because the call is
//! `state.getBlock().getExplosionResistance()` and no property of the state
//! can move it — a claim the dumper checks over all 32,366 states rather than
//! asserting.

use std::collections::HashMap;
use std::io;
use std::sync::OnceLock;

/// Where the extract lives, relative to the server's working directory.
///
/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping the repo for
/// `"data/vanilla/<name>.json"`, so this literal must stay spelled inside one
/// pair of quotes. `ci/check-generator-arms.sh` reds if the arm exists and no
/// reader names it.
pub const TABLE_PATH: &str = "data/vanilla/block_explosion.json";

/// One distinct `(resistance, dropFromExplosion)` pair. 35 of them cover all
/// 1,196 blocks in 26.2.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// `Block.getExplosionResistance()`, the virtual call.
    pub resistance: f32,
    /// `Block.dropFromExplosion(Explosion)`. `true` for every block but
    /// `minecraft:tnt`, whose `TntBlock.dropFromExplosion` is `iconst_0`.
    pub drop_from_explosion: bool,
}

/// The table: distinct rows, one index per block name, and the fluids.
#[derive(Debug, Clone)]
pub struct BlockExplosion {
    rows: Vec<Row>,
    block_rows: HashMap<String, u16>,
    fluid_resistance: HashMap<String, f32>,
    states: usize,
}

fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("block_explosion.json: {what}"),
    )
}

/// A float read from its raw bits, never from its decimal rendering. `3600000.8`
/// and `3600000.0` differ in three bits and in nothing a reader would notice.
fn f32_bits(v: Option<&serde_json::Value>) -> Option<f32> {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    v.and_then(serde_json::Value::as_i64)
        .map(|b| f32::from_bits(b as u32))
}

impl BlockExplosion {
    /// Load from an explicit path.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)
            .map_err(|e| bad(&format!("not json: {e}")))?;
        Self::from_json(&doc)
    }

    /// Parse the dumper's output.
    ///
    /// # Errors
    /// If a row or a block entry is malformed, or if the table is empty. There
    /// is deliberately no default: a block with no resistance would be a block
    /// an explosion walks straight through, and that is indistinguishable from
    /// this server not having the table at all.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let mut rows = Vec::new();
        for r in doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `table` array"))?
        {
            rows.push(Row {
                resistance: f32_bits(r.get("resistanceBits"))
                    .ok_or_else(|| bad("a row with no `resistanceBits`"))?,
                drop_from_explosion: r
                    .get("dropFromExplosion")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| bad("a row with no `dropFromExplosion`"))?,
            });
        }

        let mut block_rows = HashMap::new();
        for e in doc
            .get("blockRows")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `blockRows` array"))?
        {
            let id = e
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a blockRows entry with no `id`"))?;
            let i = e
                .get("row")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| bad(&format!("{id} has no `row`")))?;
            let i = usize::try_from(i).map_err(|_| bad(&format!("{id} maps to row {i}")))?;
            if i >= rows.len() {
                return Err(bad(&format!("{id} maps to row {i} of {}", rows.len())));
            }
            #[allow(clippy::cast_possible_truncation)]
            block_rows.insert(id.to_string(), i as u16);
        }

        let mut fluid_resistance = HashMap::new();
        for e in doc
            .get("fluids")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `fluids` array"))?
        {
            let id = e
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a fluids entry with no `id`"))?;
            let r = f32_bits(e.get("resistanceBits"))
                .ok_or_else(|| bad(&format!("fluid {id} has no `resistanceBits`")))?;
            fluid_resistance.insert(id.to_string(), r);
        }

        let states = doc
            .get("states")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| bad("no `states` count"))?;

        if rows.is_empty() || block_rows.is_empty() || fluid_resistance.is_empty() {
            return Err(bad("empty — every block would be transparent to a blast"));
        }
        Ok(Self {
            rows,
            block_rows,
            fluid_resistance,
            states,
        })
    }

    /// The process-wide table, loaded once.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn shared(path: &std::path::Path) -> io::Result<&'static Self> {
        static ONCE: OnceLock<io::Result<BlockExplosion>> = OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        }
    }

    /// The row for a block name, with or without a `[...]` property suffix.
    ///
    /// `None` only for a name outside the registry, which is a name from
    /// another version rather than a block with no resistance.
    #[must_use]
    pub fn row(&self, state: &str) -> Option<&Row> {
        let name = state.split_once('[').map_or(state, |(n, _)| n);
        let i = *self.block_rows.get(name)?;
        self.rows.get(usize::from(i))
    }

    /// `FluidState.getExplosionResistance()` — `getType().getExplosionResistance()`,
    /// so a fluid's resistance is a property of its type and the flowing and
    /// source forms share it.
    ///
    /// `None` for an unknown fluid name. `minecraft:empty` is a real entry at
    /// 0.0 and is not the same answer as "no such fluid".
    #[must_use]
    pub fn fluid(&self, fluid: &str) -> Option<f32> {
        self.fluid_resistance.get(fluid).copied()
    }

    /// How many distinct rows the table holds, for the boot log.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table holds nothing. Never true for a loaded table —
    /// [`Self::from_json`] refuses an empty one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many blocks the table covers.
    #[must_use]
    pub fn blocks(&self) -> usize {
        self.block_rows.len()
    }

    /// How many block states the dumper saw. Not indexable here — the table is
    /// keyed by block — and carried so the boot log can quote the denominator
    /// the cross-check ran against.
    #[must_use]
    pub fn states(&self) -> usize {
        self.states
    }
}

/// `ExplosionDamageCalculator.getBlockExplosionResistance`, offsets 0-42.
///
/// ```text
///  0: if (state.isAir() && fluid.isEmpty()) return Optional.empty();
/// 20: return Optional.of(max(state.getBlock().getExplosionResistance(),
///                            fluid.getExplosionResistance()));
/// ```
///
/// `None` is `Optional.empty()` and is **not** zero: the caller
/// (`calculateExplodedPositions`, offsets 288-316) skips the whole
/// `f -= (res + 0.3f) * 0.3f` subtraction on an empty, so an empty costs the
/// ray nothing while a `Some(0.0)` costs it `0.09`. Over a 3.0-radius blast
/// that is the difference between reaching the far wall and not.
///
/// `fluid` is the fluid registry name at the position, or `None` for
/// `FluidState.isEmpty()`. `minecraft:empty` is accepted and treated as empty,
/// because that is the name the empty fluid actually carries in the registry.
///
/// # Errors
/// Never; the `Result` shape is not used. Returns `None` for the empty case
/// and `Some` otherwise.
#[must_use]
pub fn block_explosion_resistance(
    table: &BlockExplosion,
    state: &str,
    is_air: bool,
    fluid: Option<&str>,
) -> Option<f32> {
    let fluid_res = match fluid {
        None | Some("minecraft:empty") => None,
        Some(f) => table.fluid(f),
    };
    if is_air && fluid_res.is_none() {
        return None;
    }
    let block_res = table.row(state).map_or(0.0, |r| r.resistance);
    Some(block_res.max(fluid_res.unwrap_or(0.0)))
}

// ---------------------------------------------------------------------------
// The entity pass — the arithmetic, without the entity
// ---------------------------------------------------------------------------

/// `ExplosionDamageCalculator.getEntityDamageAmount(Explosion, Entity, float)`,
/// offsets 0-65, whole.
///
/// ```text
///  0: float f = explosion.radius() * 2.0F;
/// 10: double d0 = sqrt(entity.distanceToSqr(explosion.center())) / (double) f;
/// 33: double d1 = (1.0 - d0) * (double) seenPercent;
/// 42: return (float) ((d1 * d1 + d1) / 2.0 * 7.0 * (double) f + 1.0);
/// ```
///
/// Two terms and both matter: the quadratic `d1 * d1` and the linear `d1`,
/// halved, times seven, times the doubled radius, plus one. The `+ 1.0` is why
/// a blast at the very edge still does a point of damage rather than none, and
/// the `* 2.0F` at the top is applied **twice** — once to normalise the
/// distance and once to scale the result — which is why doubling a TNT's power
/// more than doubles its damage.
///
/// `distance` is the *linear* distance from the explosion centre to
/// `entity.position()`, already square-rooted. `seen_percent` is
/// [`seen_percent_samples`]' fraction; passing `1.0` gives the unobstructed
/// maximum.
///
/// Note the mixed precision, which is the part a rewrite gets wrong: the
/// radius doubling is `float`, everything after it is `double`, and only the
/// return is narrowed.
#[must_use]
pub fn entity_damage_amount(radius: f32, distance: f64, seen_percent: f32) -> f32 {
    let f = radius * 2.0;
    let d0 = distance / f64::from(f);
    let d1 = (1.0 - d0) * f64::from(seen_percent);
    #[allow(clippy::cast_possible_truncation)]
    {
        ((d1 * d1 + d1) / 2.0 * 7.0 * f64::from(f) + 1.0) as f32
    }
}

/// The knockback magnitude, `ServerExplosion.hurtEntities` offsets 371-388.
///
/// ```text
/// 371: double d3 = (1.0 - d0) * (double) seenPercent
///                             * (double) knockbackMultiplier
///                             * (1.0 - knockbackResistance);
/// 390: Vec3 push = direction.scale(d3);
/// 399: entity.push(push);
/// ```
///
/// `d0` is the same normalised distance [`entity_damage_amount`] uses, so this
/// takes it rather than recomputing it. `direction` is
/// `(eyePosition - center).normalize()` — except for a `PrimedTnt`, which uses
/// `position()` instead of `getEyePosition()` (offsets 224-245), and that is
/// the whole of the difference that makes TNT cannoning work.
///
/// `knockback_multiplier` is `ExplosionDamageCalculator.getKnockbackMultiplier`,
/// `1.0f` for the default calculator and `0.0f` for a flying creative player
/// under `SimpleExplosionDamageCalculator`. `knockback_resistance` is
/// `Attributes.EXPLOSION_KNOCKBACK_RESISTANCE`, `0.0` for anything that is not
/// a `LivingEntity` (offsets 342-369).
///
/// There is **no** protection-enchantment term here in 26.2. See
/// [`mod@refusals`].
#[must_use]
pub fn knockback_scale(
    normalised_distance: f64,
    seen_percent: f32,
    knockback_multiplier: f32,
    knockback_resistance: f64,
) -> f64 {
    (1.0 - normalised_distance)
        * f64::from(seen_percent)
        * f64::from(knockback_multiplier)
        * (1.0 - knockback_resistance)
}

/// `ServerExplosion.getSeenPercent(Vec3, Entity)`'s sampling grid, offsets
/// 0-294 — the ray **origins**, in the order the three loops emit them.
///
/// ```text
///   5: double dx = 1.0 / ((aabb.maxX - aabb.minX) * 2.0 + 1.0);
///  23: double dy = 1.0 / ((aabb.maxY - aabb.minY) * 2.0 + 1.0);
///  42: double dz = 1.0 / ((aabb.maxZ - aabb.minZ) * 2.0 + 1.0);
///  61: double ox = (1.0 - floor(1.0 / dx) * dx) / 2.0;
///  77: double oz = (1.0 - floor(1.0 / dz) * dz) / 2.0;
///  95: if (dx < 0.0 || dy < 0.0 || dz < 0.0) return 0.0F;
/// 126: for (double fx = 0.0; fx <= 1.0; fx += dx)
/// 136:   for (double fy = 0.0; fy <= 1.0; fy += dy)
/// 146:     for (double fz = 0.0; fz <= 1.0; fz += dz)
/// 153:       Vec3 from = new Vec3(lerp(fx, minX, maxX) + ox,
///                                 lerp(fy, minY, maxY),
///                                 lerp(fz, minZ, maxZ) + oz);
/// 219:       if (level.clip(new ClipContext(from, center, COLLIDER, NONE,
///                                           entity)).getType() == MISS) hits++;
/// 255:       total++;
/// 287: return (float) hits / (float) total;
/// ```
///
/// Three details a rewrite loses. **There is no `oy`** — the x and z origins
/// are nudged to the middle of their cell and the y one is not, and the
/// bytecode has only two `floor` calls (offsets 65 and 82). **The loop bound
/// is `<= 1.0` on an accumulating `double`**, so the sample count depends on
/// floating-point accumulation rather than on a computed count; a
/// `(1.0/dx).round()` loop gives a different number of samples for some box
/// sizes. And the **`dy < 0.0`** guard is a strict `<`, so a zero-height box
/// samples rather than returning 0.
///
/// The predicate itself is refused — there is no raycast in this workspace —
/// so this returns the origins and the caller counts the misses. See
/// [`mod@refusals`].
#[must_use]
pub fn seen_percent_samples(min: (f64, f64, f64), max: (f64, f64, f64)) -> Vec<(f64, f64, f64)> {
    let dx = 1.0 / ((max.0 - min.0) * 2.0 + 1.0);
    let dy = 1.0 / ((max.1 - min.1) * 2.0 + 1.0);
    let dz = 1.0 / ((max.2 - min.2) * 2.0 + 1.0);
    let ox = (1.0 - (1.0 / dx).floor() * dx) / 2.0;
    let oz = (1.0 - (1.0 / dz).floor() * dz) / 2.0;
    if dx < 0.0 || dy < 0.0 || dz < 0.0 {
        return Vec::new();
    }
    // `Mth.lerp(delta, start, end)` — `start + delta * (end - start)`, and
    // the tree's port of it. NOT a fused multiply-add: `dmul` then `dadd`
    // round twice and `mul_add` rounds once, which is a different number.
    let lerp = rustmc_worldgen::math::lerp;
    let mut out = Vec::new();
    let mut fx = 0.0f64;
    while fx <= 1.0 {
        let mut fy = 0.0f64;
        while fy <= 1.0 {
            let mut fz = 0.0f64;
            while fz <= 1.0 {
                out.push((
                    lerp(fx, min.0, max.0) + ox,
                    lerp(fy, min.1, max.1),
                    lerp(fz, min.2, max.2) + oz,
                ));
                fz += dz;
            }
            fy += dy;
        }
        fx += dx;
    }
    out
}

// ---------------------------------------------------------------------------
// The aftermath: drop collection
// ---------------------------------------------------------------------------

/// `ServerExplosion.MAX_DROPS_PER_COMBINED_STACK`.
pub const MAX_DROPS_PER_COMBINED_STACK: i32 = 16;

/// `ServerExplosion.addOrAppendStack` (offsets 0-60) and
/// `StackCollector.tryMerge`, over the whole per-position drop stream.
///
/// ```text
/// addOrAppendStack(list, stack, pos):
///   for (StackCollector c : list) { c.tryMerge(stack); if (stack.isEmpty()) return; }
///   list.add(new StackCollector(pos, stack));
///
/// StackCollector.tryMerge(other):
///   if (ItemEntity.areMergable(this.stack, other))
///       this.stack = ItemEntity.merge(this.stack, other, 16);
/// ```
///
/// Two asymmetries that a plausible rewrite inverts. `areMergable(a, b)` tests
/// `b.getCount() + a.getCount() > b.getMaxStackSize()` — the **incoming**
/// stack's max size, not the collector's — and `merge(a, b, 16)` moves
/// `min(min(a.getMaxStackSize(), 16) - a.getCount(), b.getCount())`, so a
/// collector that has reached sixteen is mergable with a further item and
/// moves nothing from it, which is exactly what forces the next collector to
/// be created. Without the 16 cap one explosion's cobblestone would arrive as
/// stacks of 64.
///
/// The position recorded is the position of the **first** stack that opened
/// the collector, not of the one that filled it — so a merged stack pops out
/// of the first block that dropped it, several blocks from where the last one
/// came from.
///
/// This is where the port stops. Vanilla's next line is
/// `Block.popResource(level, collector.pos, collector.stack)`, and there is no
/// item entity for it to make. See [`mod@refusals`].
#[must_use]
pub fn merge_drop_stacks(
    drops: &[((i32, i32, i32), crate::item::ItemStack)],
) -> Vec<((i32, i32, i32), crate::item::ItemStack)> {
    let mut out: Vec<((i32, i32, i32), crate::item::ItemStack)> = Vec::new();
    for (pos, stack) in drops {
        let mut stack = stack.clone();
        let mut consumed = false;
        for (_, held) in &mut out {
            // `ItemEntity.areMergable(this.stack, other)`, offsets 0-23.
            let mergable = stack.count() + held.count() <= stack.max_stack_size()
                && crate::item::ItemStack::is_same_item_same_components(held, &stack);
            if mergable {
                // `ItemEntity.merge(a, b, 16)`, offsets 0-40.
                let room = held.max_stack_size().min(MAX_DROPS_PER_COMBINED_STACK) - held.count();
                let moved = room.min(stack.count());
                *held = held.copy_with_count(held.count() + moved);
                stack.shrink(moved);
            }
            if stack.is_empty() {
                consumed = true;
                break;
            }
        }
        if !consumed {
            out.push((*pos, stack));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// What is NOT ported, and why
// ---------------------------------------------------------------------------

/// The parts of vanilla's explosion this tree cannot carry, named rather than
/// approximated.
///
/// Each of these is a refusal with a reason, not an omission. An explosion
/// that silently did four of the six things vanilla does would be worse than
/// one that does two and says so, because the missing two would then be
/// invisible to every gate that measures what the server emits.
///
/// # `ServerExplosion.hurtEntities()`, offsets 0-515 — REFUSED, no entities
///
/// There is no entity that ticks in this server. `crate::tick` states it:
/// nothing ever changes an entity's position except the owning client's own
/// movement packet, there is no gravity, no drag, no collision and no
/// `Entity.move`. `crate::entities::KINDS` holds two kinds, `PLAYER` and
/// `ITEM`; `crate::entities::Entity` carries no health and no bounding box;
/// and `crate::attack::Outcome::Hurt` is documented unreachable. So the
/// AABB query at offsets 118-152, `Entity.ignoreExplosion`, `Entity.hurtServer`
/// and `Entity.push` have nothing to iterate over.
///
/// What this would join to: `origin/mob-in-world` at `44027b933`, which is
/// unmerged. The arithmetic that does not need an entity —
/// [`entity_damage_amount`], [`knockback_scale`] and [`seen_percent_samples`]
/// — is ported and tested here, so that when the entity pass lands it is
/// wiring rather than transcription.
///
/// # `PrimedTnt` — REFUSED, and it is the reason nothing can set an explosion off
///
/// `TntBlock.wasExploded` spawns a `PrimedTnt` at
/// `(x + 0.5, y, z + 0.5)` when `GameRules.TNT_EXPLODES` is set, and
/// `PrimedTnt.tick` is `handlePortal, applyGravity, move(SELF, deltaMovement),
/// applyEffectsFromBlocks, scale by airDrag, the 0.7/-0.5/0.7 ground bounce,
/// then fuse--` — every line of which is entity motion this server does not
/// have. `DEFAULT_FUSE_TIME` is 80 ticks and `DEFAULT_EXPLOSION_POWER` is
/// 4.0f. Same branch, `origin/mob-in-world`.
///
/// # The creeper — REFUSED, it belongs to the mob branches
///
/// `ExplosionInteraction::Mob` and the `mobGriefing` arm of
/// [`block_interaction_for`] are ported because they are `ServerLevel`'s
/// switch and not the mob's; the swell, the ignition and
/// `Creeper.explodeCreeper` are the mob branches' and are not touched here.
///
/// # Enchantment-based damage reduction — REFUSED, and it is not in 26.2 anyway
///
/// Worth stating because a reader of an older version will look for it:
/// `ServerExplosion.hurtEntities` has **no** protection-enchantment term. The
/// damage is `getEntityDamageAmount` and the knockback is scaled only by
/// `getKnockbackMultiplier` and `Attributes.EXPLOSION_KNOCKBACK_RESISTANCE`
/// (offsets 342-388). Any reduction happens later, inside the damage pipeline,
/// which this tree does not have either.
///
/// # `Level.clip` — REFUSED, there is no raycast in this tree
///
/// [`seen_percent_samples`] emits the sample *origins*
/// `ServerExplosion.getSeenPercent` builds, which is the half of it that is
/// arithmetic. The other half is
/// `level.clip(new ClipContext(from, center, COLLIDER, NONE, entity))`, and
/// `crate::collide` has an AABB sweep and no ray: there is no `ClipContext`,
/// no `BlockHitResult` and no `traverseBlocks` anywhere in this workspace.
///
/// # Item entities from block drops — REFUSED, the last link is missing
///
/// `crate::dropitem` spawns a thrown item for the player's own drop and
/// nothing else; `crate::shapes` says at its `destroyBlock` that "no item
/// entities exist, so `dropResources` has nowhere to put them".
/// [`merge_drop_stacks`] does the part that is explosion-specific — the
/// `StackCollector` merge at a cap of 16 — and stops at the point where an
/// `ItemEntity` would be created.
pub mod refusals {}

// ---------------------------------------------------------------------------
// The interaction enums, and `ServerLevel.explode`'s switch
// ---------------------------------------------------------------------------

/// `Explosion$BlockInteraction` — **four** values in 26.2, in declaration
/// order, with the boolean each was constructed with.
///
/// ```text
/// Explosion$BlockInteraction.<clinit>
///    0: KEEP               ("KEEP", 0, false)
///   14: DESTROY            ("DESTROY", 1, true)
///   28: DESTROY_WITH_DECAY ("DESTROY_WITH_DECAY", 2, true)
///   42: TRIGGER_BLOCK      ("TRIGGER_BLOCK", 3, false)
/// ```
///
/// `TRIGGER_BLOCK` is the one a reader of the 1.20 shape would not expect. It
/// is what a wind charge uses: `BlockBehaviour.onExplosionHit` returns
/// immediately on it (offsets 7-20), so nothing breaks and nothing drops, and
/// `ServerExplosion.interactsWithBlocks` is still true for it, so the shuffle
/// and the per-position walk still run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockInteraction {
    /// Nothing happens to blocks. `interactWithBlocks` is not called at all,
    /// so the `Util.shuffle` draws are not spent either.
    Keep,
    /// Blocks break and drop their full loot.
    Destroy,
    /// Blocks break and each drop rolls `explosion_decay` / `survives_explosion`
    /// against the radius.
    DestroyWithDecay,
    /// Blocks are visited but not broken. New in 26.x.
    TriggerBlock,
}

impl BlockInteraction {
    /// `Explosion$BlockInteraction.shouldAffectBlocklikeEntities()` — the
    /// boolean field each constant carries.
    #[must_use]
    pub fn should_affect_blocklike_entities(self) -> bool {
        matches!(self, Self::Destroy | Self::DestroyWithDecay)
    }

    /// `ServerExplosion.interactsWithBlocks()`, offsets 0-15 —
    /// `blockInteraction != KEEP`. Note what this is *not*: it is true for
    /// `TRIGGER_BLOCK`, which breaks nothing.
    #[must_use]
    pub fn interacts_with_blocks(self) -> bool {
        self != Self::Keep
    }
}

/// `Level$ExplosionInteraction` — five values, with the serialized names the
/// enum's own `id` field carries.
///
/// ```text
/// Level$ExplosionInteraction.<clinit>
///    0: NONE    ("NONE", 0, "none")
///   15: BLOCK   ("BLOCK", 1, "block")
///   30: MOB     ("MOB", 2, "mob")
///   45: TNT     ("TNT", 3, "tnt")
///   60: TRIGGER ("TRIGGER", 4, "trigger")
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionInteraction {
    /// A `/summon`-free explosion that leaves the terrain alone.
    None,
    /// A block-sourced blast: a bed in the nether, a respawn anchor.
    Block,
    /// A creeper, a ghast fireball, a wither skull.
    Mob,
    /// Primed TNT, and an end crystal.
    Tnt,
    /// A wind charge.
    Trigger,
}

impl ExplosionInteraction {
    /// The `getSerializedName()` string.
    #[must_use]
    pub fn serialized_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Block => "block",
            Self::Mob => "mob",
            Self::Tnt => "tnt",
            Self::Trigger => "trigger",
        }
    }
}

/// The four game rules `ServerLevel.explode`'s switch reads, and no others.
///
/// Held as a struct rather than read from [`crate::gamerules::GameRules`] at
/// the point of use so that the switch is a pure function and every one of its
/// five arms is reachable from a test. [`Self::from_shared`] is the live route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropDecayRules {
    /// `GameRules.MOB_GRIEFING`. Default `true`.
    pub mob_griefing: bool,
    /// `GameRules.BLOCK_EXPLOSION_DROP_DECAY`. Default `true`.
    pub block_explosion_drop_decay: bool,
    /// `GameRules.MOB_EXPLOSION_DROP_DECAY`. Default `true`.
    pub mob_explosion_drop_decay: bool,
    /// `GameRules.TNT_EXPLOSION_DROP_DECAY`. Default **`false`** — the one of
    /// the three decay rules that is off, which is why TNT drops everything
    /// and a creeper does not.
    pub tnt_explosion_drop_decay: bool,
}

impl DropDecayRules {
    /// Read the four rules out of the process-wide table.
    ///
    /// A rule that is missing or is not a boolean falls back to vanilla's
    /// default for it, because the alternative is refusing to explode at all
    /// over a table defect that `crate::gamerules` would already have caught.
    #[must_use]
    pub fn from_shared() -> Self {
        let rules = crate::gamerules::GameRules::shared();
        let b = |name: &str, default: bool| match rules.get(name) {
            Some(crate::gamerules::GameRuleValue::Bool(v)) => v,
            _ => default,
        };
        Self {
            mob_griefing: b("mob_griefing", true),
            block_explosion_drop_decay: b("block_explosion_drop_decay", true),
            mob_explosion_drop_decay: b("mob_explosion_drop_decay", true),
            tnt_explosion_drop_decay: b("tnt_explosion_drop_decay", false),
        }
    }

    /// `ServerLevel.getDestroyType(rule)`, offsets 0-26 —
    /// `rule ? DESTROY_WITH_DECAY : DESTROY`.
    #[must_use]
    fn destroy_type(decay: bool) -> BlockInteraction {
        if decay {
            BlockInteraction::DestroyWithDecay
        } else {
            BlockInteraction::Destroy
        }
    }
}

/// `ServerLevel.explode`'s `switch` on the interaction, offsets 0-118.
///
/// ```text
///  54: NONE    -> KEEP
///  60: BLOCK   -> getDestroyType(BLOCK_EXPLOSION_DROP_DECAY)
///  70: MOB     -> mobGriefing ? getDestroyType(MOB_EXPLOSION_DROP_DECAY) : KEEP
/// 105: TNT     -> getDestroyType(TNT_EXPLOSION_DROP_DECAY)
/// 115: TRIGGER -> TRIGGER_BLOCK
/// ```
///
/// `MOB` is the only arm that consults `mobGriefing`, and it consults it
/// *before* the decay rule rather than after — so `mobGriefing false` makes a
/// creeper harmless to terrain whatever `mobExplosionDropDecay` says.
#[must_use]
pub fn block_interaction_for(
    interaction: ExplosionInteraction,
    rules: DropDecayRules,
) -> BlockInteraction {
    match interaction {
        ExplosionInteraction::None => BlockInteraction::Keep,
        ExplosionInteraction::Block => {
            DropDecayRules::destroy_type(rules.block_explosion_drop_decay)
        }
        ExplosionInteraction::Mob => {
            if rules.mob_griefing {
                DropDecayRules::destroy_type(rules.mob_explosion_drop_decay)
            } else {
                BlockInteraction::Keep
            }
        }
        ExplosionInteraction::Tnt => DropDecayRules::destroy_type(rules.tnt_explosion_drop_decay),
        ExplosionInteraction::Trigger => BlockInteraction::TriggerBlock,
    }
}

// ---------------------------------------------------------------------------
// The block-collection pass
// ---------------------------------------------------------------------------

/// One ray step's worth of world, as `calculateExplodedPositions` reads it.
///
/// The three fields are the three calls the loop makes at each step
/// (`ServerExplosion.calculateExplodedPositions`, offsets 231-343), and they
/// are bundled so that the whole world dependency of the ray walk is one
/// closure. `resistance` and `should_explode` are only read when `in_bounds`
/// is `true`, so a caller may leave them at their defaults for a position
/// outside the world rather than computing them — which is also what vanilla
/// does, since its bounds check (offset 262) precedes its
/// `getBlockExplosionResistance` (offset 283).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Probe {
    /// `ServerLevel.isInWorldBounds(pos)` —
    /// `minY <= y <= maxY && -30_000_000 <= x, z < 30_000_000`. The y bound is
    /// **inclusive at the top**: `LevelHeightAccessor.isInsideBuildHeight(int)`
    /// is `y >= getMinY() && y <= getMaxY()`.
    pub in_bounds: bool,
    /// `damageCalculator.getBlockExplosionResistance(...)`. `None` is
    /// `Optional.empty()`, which costs the ray nothing at all — not zero,
    /// which costs it `(0.0 + 0.3) * 0.3 = 0.09`.
    pub resistance: Option<f32>,
    /// `damageCalculator.shouldBlockExplode(...)`. `true` for the default
    /// `ExplosionDamageCalculator` (`iconst_1`), which is what every
    /// source-less explosion uses.
    pub should_explode: bool,
}

/// How many of the 4,096 grid cells are on the shell, and so how many rays —
/// and how many `nextFloat()` draws — every explosion spends.
///
/// `16^3 - 14^3`. It does not depend on the radius, on the world, or on
/// anything the explosion hits: the shell test at offsets 39-73 precedes the
/// draw at offset 170, and no path out of the ray loop skips a later ray.
pub const RAYS: usize = 16 * 16 * 16 - 14 * 14 * 14;

/// The step length, `(double) 0.3f`.
///
/// `ldc2_w #234 // double 0.30000001192092896d` at offsets 359, 370 and 381 —
/// the `double` widening of the `float` literal `0.3f`, not the `double`
/// literal `0.3`. They differ in the 30th bit and the difference accumulates
/// over the ~30 steps of a radius-4 ray.
pub const STEP: f64 = 0.300_000_011_920_928_96;

/// The per-step attenuation, `0.22500001f`.
///
/// `ldc #236 // float 0.22500001f` at offset 390. Not `0.225f`: the two differ
/// by one ulp and a ray takes about thirty steps.
pub const STEP_DECAY: f32 = 0.225_000_01;

/// `ServerExplosion.calculateExplodedPositions()`, offsets 0-424, whole.
///
/// Returns the positions in the order `new ObjectArrayList<>(HashSet)` yields
/// them — that is, in `java.util.HashSet` iteration order over the insertion
/// sequence — because that order is what `Util.shuffle` then permutes and what
/// `interactWithBlocks` walks. A `Vec` built in insertion order would give a
/// different shuffle from the same RNG draws.
///
/// The RNG contract: exactly [`RAYS`] `nextFloat()` draws, one per shell cell,
/// in `j`-major then `k` then `l` order, spent whether or not the ray goes
/// anywhere.
///
/// # Errors
/// [`Untracked`] when the position set would need a red-black-tree bin whose
/// order `java.util.HashMap` breaks with `System.identityHashCode` — a tie the
/// port refuses rather than guesses. It has never been reached: a bin needs
/// nine `BlockPos` hashes colliding after the spread in a table of at least
/// 64, and the positions of one blast are a contiguous shell.
///
/// # Panics
/// Never. The `sqrt` divisor cannot be zero: it is zero only when all three of
/// `j`, `k`, `l` map to `0.0`, which needs `j == 7.5`.
pub fn calculate_exploded_positions<R, F>(
    center: (f64, f64, f64),
    radius: f32,
    random: &mut R,
    mut probe: F,
) -> Result<Vec<(i32, i32, i32)>, rustmc_data::java_hash_set::Untracked>
where
    R: rustmc_worldgen::rng::RandomSource,
    F: FnMut((i32, i32, i32)) -> Probe,
{
    let mut set: rustmc_data::java_hash_set::JavaHashSet<(i32, i32, i32)> =
        rustmc_data::java_hash_set::JavaHashSet::new();

    for j in 0..16i32 {
        for k in 0..16i32 {
            for l in 0..16i32 {
                // `if (j != 0 && j != 15 && k != 0 && k != 15 && l != 0 && l != 15) continue;`
                // — the shell, offsets 39-73. The `continue` is *before* the
                // draw, so an interior cell costs nothing at all.
                if j != 0 && j != 15 && k != 0 && k != 15 && l != 0 && l != 15 {
                    continue;
                }

                // `(double)((float)j / 15.0F * 2.0F - 1.0F)` — the arithmetic
                // is `float` and only the result is widened. Offsets 76-112.
                #[allow(clippy::cast_precision_loss)]
                let mut d0 = f64::from((j as f32) / 15.0 * 2.0 - 1.0);
                #[allow(clippy::cast_precision_loss)]
                let mut d1 = f64::from((k as f32) / 15.0 * 2.0 - 1.0);
                #[allow(clippy::cast_precision_loss)]
                let mut d2 = f64::from((l as f32) / 15.0 * 2.0 - 1.0);
                // Normalised in `double`, offsets 114-155. Note this is not
                // `Vec3.normalize`, which has a 1e-4 guard; it is a bare
                // divide by `Math.sqrt`.
                let d3 = (d0 * d0 + d1 * d1 + d2 * d2).sqrt();
                d0 /= d3;
                d1 /= d3;
                d2 /= d3;

                // `this.radius * (0.7F + this.level.random.nextFloat() * 0.6F)`,
                // offsets 157-180. One draw per ray, in `float`.
                let mut f = radius * (0.7 + random.next_f32() * 0.6);

                let (mut x, mut y, mut z) = center;
                // `while (f > 0.0F)`, offset 213.
                while f > 0.0 {
                    // `BlockPos.containing(d4, d5, d6)` — `Mth.floor` each.
                    let pos = (
                        rustmc_worldgen::math::floor(x),
                        rustmc_worldgen::math::floor(y),
                        rustmc_worldgen::math::floor(z),
                    );
                    let p = probe(pos);
                    // `if (!this.level.isInWorldBounds(pos)) break;`, offsets
                    // 253-265. A `break` out of the ray, not a `continue`.
                    if !p.in_bounds {
                        break;
                    }
                    // `if (optional.isPresent()) f -= (optional.get() + 0.3F) * 0.3F;`
                    // offsets 288-316.
                    if let Some(r) = p.resistance {
                        f -= (r + 0.3) * 0.3;
                    }
                    // `if (f > 0.0F && shouldBlockExplode(...)) set.add(pos);`
                    // offsets 318-354. The re-test of `f` is not redundant:
                    // the subtraction above may have taken it negative.
                    if f > 0.0 && p.should_explode {
                        set.insert(pos)?;
                    }
                    x += d0 * STEP;
                    y += d1 * STEP;
                    z += d2 * STEP;
                    f -= STEP_DECAY;
                }
            }
        }
    }

    Ok(set.to_vec())
}

/// `Util.shuffle(List<T>, RandomSource)`, offsets 0-56.
///
/// ```text
///  9: for (int i = list.size(); i > 1; i--)
/// 14:     int j = random.nextInt(i);
/// 23:     list.set(i - 1, list.set(j, list.get(i - 1)));
/// ```
///
/// A backwards Fisher-Yates, `size` down to 2, one draw per iteration. It is
/// `Collections.shuffle`'s loop with vanilla's own `RandomSource` rather than
/// `java.util.Random`, and the difference is which stream the draws come from
/// — `interactWithBlocks` passes `level.random`, so the shuffle continues the
/// same stream the ray walk was drawing from.
///
/// A list of 0 or 1 elements spends no draws at all.
pub fn shuffle<T, R: rustmc_worldgen::rng::RandomSource>(list: &mut [T], random: &mut R) {
    let mut i = i32::try_from(list.len()).unwrap_or(i32::MAX);
    while i > 1 {
        let j = random.next_i32_bound(i);
        #[allow(clippy::cast_sign_loss)]
        list.swap((i - 1) as usize, j as usize);
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{block_explosion_resistance, BlockExplosion, TABLE_PATH};
    use std::path::{Path, PathBuf};

    /// The whole path in one literal, so `ci/gen-gameplay-tables.sh`'s grep
    /// sees it. `crates/rustmc-behavior/src/block_props.rs` uses a `concat!`
    /// here and is invisible to that grep; this does not.
    fn table() -> Option<BlockExplosion> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let path = root.join(TABLE_PATH);
        if !rustmc_data::fixture::available(
            Path::new(&path),
            "block explosion resistance extract",
            "ci/block-explosion-golden.sh",
        ) {
            return None;
        }
        Some(BlockExplosion::load(&path).expect("loads"))
    }

    #[test]
    fn the_table_covers_every_block_the_dumper_saw() {
        let Some(t) = table() else { return };
        assert_eq!(t.blocks(), 1196, "block count moved");
        assert_eq!(t.states(), 32366, "state count moved");
        assert_eq!(
            t.len(),
            35,
            "distinct (resistance, dropFromExplosion) rows moved"
        );
    }

    /// The values a reader would otherwise guess. Every one of these is a
    /// number no rule produces: obsidian and crying obsidian are both 1200,
    /// glowstone is 0.3 and glass is 0.3, and the ancient debris that is
    /// harder than netherite is 1200 while netherite block is 1200 too.
    #[test]
    fn named_resistances_are_the_jars() {
        let Some(t) = table() else { return };
        for (block, want) in [
            ("minecraft:obsidian", 1200.0_f32),
            ("minecraft:crying_obsidian", 1200.0),
            ("minecraft:bedrock", 3_600_000.0),
            ("minecraft:end_portal_frame", 3_600_000.0),
            ("minecraft:stone", 6.0),
            ("minecraft:dirt", 0.5),
            ("minecraft:glass", 0.3),
            ("minecraft:glowstone", 0.3),
            ("minecraft:tnt", 0.0),
            ("minecraft:water", 100.0),
            ("minecraft:lava", 100.0),
            ("minecraft:oak_planks", 3.0),
        ] {
            let row = t.row(block).unwrap_or_else(|| panic!("{block} missing"));
            assert!(
                (row.resistance - want).abs() < f32::EPSILON,
                "{block}: {} not {want}",
                row.resistance
            );
        }
    }

    /// `minecraft:tnt` is the only block in 26.2 whose
    /// `dropFromExplosion` is false, and the reason is
    /// `TntBlock.wasExploded` — it re-primes rather than dropping. A second
    /// entry here would mean the dumper had started passing a live
    /// `Explosion` that some override reads.
    #[test]
    fn tnt_is_the_only_block_that_does_not_drop_from_an_explosion() {
        let Some(t) = table() else { return };
        assert!(!t.row("minecraft:tnt").expect("tnt").drop_from_explosion);
        for b in [
            "minecraft:stone",
            "minecraft:obsidian",
            "minecraft:oak_planks",
            "minecraft:sand",
        ] {
            assert!(
                t.row(b).expect(b).drop_from_explosion,
                "{b} stopped dropping"
            );
        }
    }

    /// The property suffix a state string carries must not reach the lookup.
    #[test]
    fn a_state_string_resolves_to_its_block() {
        let Some(t) = table() else { return };
        let bare = t.row("minecraft:oak_slab").expect("bare");
        let with_props = t
            .row("minecraft:oak_slab[type=top,waterlogged=false]")
            .expect("with props");
        assert_eq!(bare, with_props);
    }

    /// `Optional.empty()` is not zero, and the distinction is the one thing a
    /// reader of this function can get wrong. Air with no fluid is empty; air
    /// with water is 100.0, because a waterlogged or flooded position stops a
    /// ray dead.
    #[test]
    fn air_with_no_fluid_is_empty_and_air_with_water_is_not() {
        let Some(t) = table() else { return };
        assert_eq!(
            block_explosion_resistance(&t, "minecraft:air", true, None),
            None
        );
        assert_eq!(
            block_explosion_resistance(&t, "minecraft:air", true, Some("minecraft:empty")),
            None
        );
        assert_eq!(
            block_explosion_resistance(&t, "minecraft:air", true, Some("minecraft:water")),
            Some(100.0)
        );
    }

    /// `Math.max` of the two, not the block's alone. A waterlogged oak slab is
    /// 3.0 as a block and 100.0 with the water in it.
    #[test]
    fn the_fluid_wins_when_it_is_the_harder_of_the_two() {
        let Some(t) = table() else { return };
        assert_eq!(
            block_explosion_resistance(&t, "minecraft:oak_slab", false, None),
            Some(3.0)
        );
        assert_eq!(
            block_explosion_resistance(
                &t,
                "minecraft:oak_slab[type=top,waterlogged=true]",
                false,
                Some("minecraft:water")
            ),
            Some(100.0)
        );
        // And the other way: obsidian is harder than the lava beside it.
        assert_eq!(
            block_explosion_resistance(&t, "minecraft:obsidian", false, Some("minecraft:lava")),
            Some(1200.0)
        );
    }

    // -- the interaction switch ---------------------------------------------

    use super::{
        block_interaction_for, calculate_exploded_positions, entity_damage_amount, knockback_scale,
        merge_drop_stacks, seen_percent_samples, shuffle, BlockInteraction, DropDecayRules,
        ExplosionInteraction, Probe, MAX_DROPS_PER_COMBINED_STACK, RAYS, STEP, STEP_DECAY,
    };
    use rustmc_worldgen::rng::{legacy::LegacyRandomSource, RandomSource};

    /// 26.2's defaults, from `crate::gamerules::DEFAULTS`. `tnt` is the odd one
    /// out and the odd one out is load-bearing.
    const VANILLA: DropDecayRules = DropDecayRules {
        mob_griefing: true,
        block_explosion_drop_decay: true,
        mob_explosion_drop_decay: true,
        tnt_explosion_drop_decay: false,
    };

    #[test]
    fn all_five_interaction_arms_and_the_default_rules() {
        use BlockInteraction as B;
        use ExplosionInteraction as E;
        assert_eq!(block_interaction_for(E::None, VANILLA), B::Keep);
        assert_eq!(
            block_interaction_for(E::Block, VANILLA),
            B::DestroyWithDecay
        );
        assert_eq!(block_interaction_for(E::Mob, VANILLA), B::DestroyWithDecay);
        // The one that differs, and the reason TNT gives back everything it
        // broke while a creeper does not.
        assert_eq!(block_interaction_for(E::Tnt, VANILLA), B::Destroy);
        assert_eq!(block_interaction_for(E::Trigger, VANILLA), B::TriggerBlock);
    }

    /// `mobGriefing` is consulted **before** the decay rule, so turning it off
    /// makes a creeper harmless whatever the decay rule says — and it does not
    /// touch the block, tnt or trigger arms.
    #[test]
    fn mob_griefing_gates_only_the_mob_arm() {
        use BlockInteraction as B;
        use ExplosionInteraction as E;
        let no_grief = DropDecayRules {
            mob_griefing: false,
            ..VANILLA
        };
        assert_eq!(block_interaction_for(E::Mob, no_grief), B::Keep);
        assert_eq!(
            block_interaction_for(E::Block, no_grief),
            B::DestroyWithDecay
        );
        assert_eq!(block_interaction_for(E::Tnt, no_grief), B::Destroy);
        assert_eq!(block_interaction_for(E::Trigger, no_grief), B::TriggerBlock);
        // And with griefing on but that arm's decay off, MOB is plain DESTROY.
        let no_decay = DropDecayRules {
            mob_explosion_drop_decay: false,
            ..VANILLA
        };
        assert_eq!(block_interaction_for(E::Mob, no_decay), B::Destroy);
    }

    /// `interactsWithBlocks` is `!= KEEP`, so it is **true** for
    /// `TRIGGER_BLOCK` — which breaks nothing. The two questions are different
    /// and the enum answers both.
    #[test]
    fn trigger_block_interacts_but_affects_no_blocklike_entity() {
        assert!(BlockInteraction::TriggerBlock.interacts_with_blocks());
        assert!(!BlockInteraction::TriggerBlock.should_affect_blocklike_entities());
        assert!(!BlockInteraction::Keep.interacts_with_blocks());
        assert!(!BlockInteraction::Keep.should_affect_blocklike_entities());
        assert!(BlockInteraction::Destroy.should_affect_blocklike_entities());
        assert!(BlockInteraction::DestroyWithDecay.should_affect_blocklike_entities());
    }

    #[test]
    fn the_serialized_names_are_the_jars() {
        use ExplosionInteraction as E;
        assert_eq!(E::None.serialized_name(), "none");
        assert_eq!(E::Block.serialized_name(), "block");
        assert_eq!(E::Mob.serialized_name(), "mob");
        assert_eq!(E::Tnt.serialized_name(), "tnt");
        assert_eq!(E::Trigger.serialized_name(), "trigger");
    }

    // -- the ray walk -------------------------------------------------------

    /// An `RandomSource` that counts what is drawn from it, so a claim about
    /// the draw order is a measurement rather than a reading.
    struct Counting {
        inner: LegacyRandomSource,
        floats: usize,
        ints: usize,
    }

    impl Counting {
        fn new(seed: i64) -> Self {
            Self {
                inner: LegacyRandomSource::new(seed),
                floats: 0,
                ints: 0,
            }
        }
    }

    impl RandomSource for Counting {
        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }
        fn next_i32(&mut self) -> i32 {
            self.ints += 1;
            self.inner.next_i32()
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.ints += 1;
            self.inner.next_i32_bound(bound)
        }
        fn next_i64(&mut self) -> i64 {
            self.inner.next_i64()
        }
        fn next_bool(&mut self) -> bool {
            self.inner.next_bool()
        }
        fn next_f32(&mut self) -> f32 {
            self.floats += 1;
            self.inner.next_f32()
        }
        fn next_f64(&mut self) -> f64 {
            self.inner.next_f64()
        }
        fn next_gaussian(&mut self) -> f64 {
            self.inner.next_gaussian()
        }
    }

    /// Air everywhere, in bounds everywhere. `resistance` is `None` — the
    /// `Optional.empty()` case — so a ray is stopped only by its own decay.
    fn open_air(_pos: (i32, i32, i32)) -> Probe {
        Probe {
            in_bounds: true,
            resistance: None,
            should_explode: true,
        }
    }

    /// The shell of a 16-cube is `16^3 - 14^3`, and every one of those cells
    /// spends exactly one `nextFloat()` whatever the world is. This is the
    /// whole RNG contract of the block pass and it is measured here rather
    /// than read off the bytecode a second time.
    #[test]
    fn every_explosion_spends_exactly_one_float_per_shell_cell() {
        assert_eq!(RAYS, 1352);
        for (radius, world) in [
            (4.0f32, open_air as fn((i32, i32, i32)) -> Probe),
            // A world that is entirely out of bounds: every ray breaks on its
            // first step, and the draw has already happened.
            (4.0, |_| Probe::default()),
            // A radius of zero: `f` starts at 0.0 and the `while` never runs.
            (0.0, open_air),
        ] {
            let mut r = Counting::new(1234);
            let out = calculate_exploded_positions((0.5, 65.0, 0.5), radius, &mut r, world);
            assert!(out.is_ok());
            assert_eq!(r.floats, RAYS, "radius {radius} drew {} floats", r.floats);
            assert_eq!(r.ints, 0, "the ray walk drew an int");
        }
    }

    /// The reach is set by the two constants and nothing else: a ray of
    /// strength `f` takes `ceil(f / 0.22500001)` steps of `0.3` each, and `f`
    /// is at most `radius * 1.3`. This is a bound derived from the constants
    /// rather than a recorded number, so it fails if either constant moves.
    #[test]
    fn an_unobstructed_blast_reaches_exactly_as_far_as_the_two_constants_allow() {
        let mut r = LegacyRandomSource::new(20_260_906);
        let centre = (0.5f64, 65.5, 0.5);
        let radius = 4.0f32;
        let out = calculate_exploded_positions(centre, radius, &mut r, open_air).expect("no tie");
        assert!(!out.is_empty(), "an open-air blast found nothing");

        let max_strength = f64::from(radius) * 1.3;
        let steps = (max_strength / f64::from(STEP_DECAY)).ceil();
        // The last position is floored from a point at most `steps * STEP`
        // from the centre, so allow the one block the floor can add.
        let reach = steps * STEP + 1.0;
        for p in &out {
            let d = ((f64::from(p.0) - centre.0).powi(2)
                + (f64::from(p.1) - centre.1).powi(2)
                + (f64::from(p.2) - centre.2).powi(2))
            .sqrt();
            assert!(d <= reach, "{p:?} is {d} from the centre, past {reach}");
        }
        // And it does reach out that far: a bound nothing approaches would
        // pass for a walk that never left the origin.
        let far = out
            .iter()
            .map(|p| {
                ((f64::from(p.0) - centre.0).powi(2)
                    + (f64::from(p.1) - centre.1).powi(2)
                    + (f64::from(p.2) - centre.2).powi(2))
                .sqrt()
            })
            .fold(0.0f64, f64::max);
        assert!(far > reach * 0.5, "the blast only reached {far} of {reach}");
    }

    /// `Optional.empty()` is not `Some(0.0)`, and the difference is
    /// measurable: an empty costs the ray nothing, a zero costs it
    /// `(0.0 + 0.3) * 0.3 = 0.09` per step, which is 40% on top of the
    /// `0.22500001` decay. This is the positive control for that distinction.
    #[test]
    fn an_empty_resistance_and_a_zero_resistance_give_different_blasts() {
        let air = |_: (i32, i32, i32)| Probe {
            in_bounds: true,
            resistance: None,
            should_explode: true,
        };
        let zero = |_: (i32, i32, i32)| Probe {
            in_bounds: true,
            resistance: Some(0.0),
            should_explode: true,
        };
        let mut r1 = LegacyRandomSource::new(7);
        let a = calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r1, air).expect("air");
        let mut r2 = LegacyRandomSource::new(7);
        let b = calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r2, zero).expect("zero");
        assert!(
            b.len() < a.len(),
            "a zero resistance ({}) did not cost the ray anything against an empty ({})",
            b.len(),
            a.len()
        );
    }

    /// Obsidian is 1200, and `(1200 + 0.3) * 0.3 = 360.09` takes any TNT-sized
    /// `f` negative on its first step — so a blast inside a solid obsidian
    /// world collects nothing at all. The control beside it is the same world
    /// made of dirt, which it does break.
    #[test]
    fn obsidian_stops_a_ray_on_its_first_step_and_dirt_does_not() {
        let solid = |res: f32| {
            move |_: (i32, i32, i32)| Probe {
                in_bounds: true,
                resistance: Some(res),
                should_explode: true,
            }
        };
        let mut r = LegacyRandomSource::new(99);
        let obsidian =
            calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r, solid(1200.0)).expect("ok");
        assert!(
            obsidian.is_empty(),
            "obsidian gave up {} blocks",
            obsidian.len()
        );

        let mut r = LegacyRandomSource::new(99);
        let dirt =
            calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r, solid(0.5)).expect("ok");
        assert!(!dirt.is_empty(), "dirt stopped a TNT blast dead");
    }

    /// `shouldBlockExplode` false collects nothing while still spending every
    /// draw and every step — the calculator can veto membership without
    /// changing the geometry.
    #[test]
    fn should_block_explode_false_collects_nothing_and_still_draws() {
        let mut r = Counting::new(5);
        let out = calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r, |_| Probe {
            in_bounds: true,
            resistance: None,
            should_explode: false,
        })
        .expect("ok");
        assert!(out.is_empty());
        assert_eq!(r.floats, RAYS);
    }

    /// A different seed is a different blast. Without this the tests above
    /// would all pass for a walk that ignored the RNG entirely.
    #[test]
    fn the_stream_actually_reaches_the_geometry() {
        let a = {
            let mut r = LegacyRandomSource::new(1);
            calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r, open_air).expect("ok")
        };
        let b = {
            let mut r = LegacyRandomSource::new(2);
            calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r, open_air).expect("ok")
        };
        assert_ne!(a, b, "two seeds gave the same blast");
        let a2 = {
            let mut r = LegacyRandomSource::new(1);
            calculate_exploded_positions((0.5, 65.5, 0.5), 4.0, &mut r, open_air).expect("ok")
        };
        assert_eq!(a, a2, "the same seed gave two blasts");
    }

    // -- Util.shuffle -------------------------------------------------------

    /// `size` down to 2, one `nextInt(i)` each — so `n - 1` draws, and none at
    /// all below two elements.
    #[test]
    fn shuffle_spends_one_draw_per_element_after_the_first() {
        for n in [0usize, 1, 2, 5, 40] {
            let mut list: Vec<usize> = (0..n).collect();
            let mut r = Counting::new(3);
            shuffle(&mut list, &mut r);
            assert_eq!(r.ints, n.saturating_sub(1), "n = {n}");
            assert_eq!(r.floats, 0);
            let mut sorted = list.clone();
            sorted.sort_unstable();
            assert_eq!(
                sorted,
                (0..n).collect::<Vec<_>>(),
                "n = {n} lost an element"
            );
        }
    }

    /// It permutes, and the permutation follows the stream. A shuffle that
    /// drew correctly and swapped nothing would pass every count above.
    #[test]
    fn shuffle_moves_things_and_the_seed_decides_where() {
        let mut a: Vec<i32> = (0..40).collect();
        let mut r = LegacyRandomSource::new(11);
        shuffle(&mut a, &mut r);
        assert_ne!(
            a,
            (0..40).collect::<Vec<_>>(),
            "shuffle left the list alone"
        );

        let mut b: Vec<i32> = (0..40).collect();
        let mut r = LegacyRandomSource::new(11);
        shuffle(&mut b, &mut r);
        assert_eq!(a, b, "the same seed gave two permutations");

        let mut c: Vec<i32> = (0..40).collect();
        let mut r = LegacyRandomSource::new(12);
        shuffle(&mut c, &mut r);
        assert_ne!(a, c, "two seeds gave one permutation");
    }

    // -- the entity arithmetic ----------------------------------------------

    /// The two terms and the `+ 1.0`. At the centre with full exposure the
    /// damage is `(1 + 1) / 2 * 7 * 2r + 1`, which for TNT's `r = 4` is
    /// `7 * 8 + 1 = 57` — half a heart short of a full-health player in
    /// hard difficulty, which is the number the mechanic is famous for.
    #[test]
    fn damage_at_the_centre_is_the_closed_form() {
        let d = entity_damage_amount(4.0, 0.0, 1.0);
        assert!((d - 57.0).abs() < 1e-4, "{d} not 57");
        // And it falls to exactly the `+ 1.0` at the edge, where `d0 == 1`.
        let edge = entity_damage_amount(4.0, 8.0, 1.0);
        assert!((edge - 1.0).abs() < 1e-6, "{edge} not 1");
    }

    /// `radius * 2.0F` is applied twice — once normalising the distance and
    /// once scaling the result — so doubling the power more than doubles the
    /// damage at a fixed distance. A port that applied it once would give a
    /// ratio of exactly 2.
    #[test]
    fn the_doubled_radius_is_applied_twice() {
        let near_r4 = entity_damage_amount(4.0, 2.0, 1.0);
        let near_r8 = entity_damage_amount(8.0, 2.0, 1.0);
        assert!(
            near_r8 / near_r4 > 2.0,
            "{near_r8} / {near_r4} is not more than twice"
        );
    }

    /// Zero exposure is one point of damage, not zero — the `+ 1.0` again, and
    /// it is why a player behind a wall still takes a hit.
    #[test]
    fn zero_seen_percent_still_does_the_plus_one() {
        assert!((entity_damage_amount(4.0, 1.0, 0.0) - 1.0).abs() < 1e-6);
    }

    /// Knockback resistance of 1.0 cancels the push exactly, and a knockback
    /// multiplier of 0.0 does too — the two independent gates
    /// `hurtEntities` applies.
    #[test]
    fn knockback_has_two_independent_gates() {
        assert!((knockback_scale(0.0, 1.0, 1.0, 0.0) - 1.0).abs() < 1e-12);
        assert!(knockback_scale(0.0, 1.0, 1.0, 1.0).abs() < 1e-12);
        assert!(knockback_scale(0.0, 1.0, 0.0, 0.0).abs() < 1e-12);
        assert!(knockback_scale(0.0, 0.0, 1.0, 0.0).abs() < 1e-12);
        // Halfway out, half resisted.
        assert!((knockback_scale(0.5, 1.0, 1.0, 0.5) - 0.25).abs() < 1e-12);
    }

    // -- getSeenPercent's grid ----------------------------------------------

    /// A player box is 0.6 x 1.8 x 0.6, which gives `dx = dz = 1/2.2` and
    /// `dy = 1/4.6`. The `<= 1.0` accumulation then yields 3 x 5 x 3 = 45
    /// samples. The count is what a computed `(1/d).round() + 1` loop gets
    /// wrong for some boxes, so it is asserted rather than derived.
    #[test]
    fn a_player_box_samples_forty_five_points() {
        let s = seen_percent_samples((-0.3, 0.0, -0.3), (0.3, 1.8, 0.3));
        assert_eq!(s.len(), 45);
    }

    /// There is no `oy`. The x and z origins are nudged to the middle of their
    /// cell and the y one is not — two `floor` calls in the bytecode, not
    /// three — so the first sample's y is exactly the box's `minY` while its x
    /// and z are not the box's `minX`/`minZ`.
    #[test]
    fn the_grid_offsets_x_and_z_but_never_y() {
        let s = seen_percent_samples((-0.3, 64.0, -0.3), (0.3, 65.8, 0.3));
        let first = s[0];
        assert!(
            (first.1 - 64.0).abs() < 1e-12,
            "y was offset: {} not 64.0",
            first.1
        );
        assert!(
            (first.0 - (-0.3)).abs() > 1e-9,
            "x was not offset: {}",
            first.0
        );
        assert!(
            (first.2 - (-0.3)).abs() > 1e-9,
            "z was not offset: {}",
            first.2
        );
    }

    /// `fz` is the innermost loop, so the samples come out z-fastest. The
    /// order is what the miss counter walks, and while the counter itself does
    /// not care, a port that swapped the loops would sample a different set of
    /// points once the offsets are applied.
    #[test]
    fn the_samples_come_out_z_fastest() {
        let s = seen_percent_samples((0.0, 0.0, 0.0), (1.0, 1.0, 1.0));
        assert!(s.len() > 3);
        assert!((s[0].0 - s[1].0).abs() < 1e-12, "x moved on the inner step");
        assert!((s[0].1 - s[1].1).abs() < 1e-12, "y moved on the inner step");
        assert!((s[0].2 - s[1].2).abs() > 1e-9, "z did not move");
    }

    /// A zero-height box still samples: the guard is `dy < 0.0`, strictly, and
    /// `dy` for a flat box is `1.0`, not negative.
    #[test]
    fn a_flat_box_still_samples() {
        let s = seen_percent_samples((0.0, 64.0, 0.0), (1.0, 64.0, 1.0));
        assert!(!s.is_empty());
    }

    // -- the drop merge -----------------------------------------------------

    /// Sixteen, not sixty-four. The cap is `MAX_DROPS_PER_COMBINED_STACK` and
    /// it is why an explosion's cobblestone arrives in many small stacks.
    #[test]
    fn the_merge_caps_at_sixteen_and_opens_a_new_collector() {
        let drops: Vec<((i32, i32, i32), crate::item::ItemStack)> = (0..20)
            .map(|i| {
                (
                    (i, 64, 0),
                    crate::item::ItemStack::one("minecraft:cobblestone"),
                )
            })
            .collect();
        let out = merge_drop_stacks(&drops);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].1.count(), MAX_DROPS_PER_COMBINED_STACK);
        assert_eq!(out[1].1.count(), 4);
        // The position is the FIRST stack's, not the last one merged into it.
        assert_eq!(out[0].0, (0, 64, 0));
        assert_eq!(out[1].0, (16, 64, 0));
    }

    /// Different items never merge, however many collectors are open.
    #[test]
    fn unlike_items_never_merge() {
        let drops = vec![
            (
                (0, 64, 0),
                crate::item::ItemStack::one("minecraft:cobblestone"),
            ),
            ((1, 64, 0), crate::item::ItemStack::one("minecraft:dirt")),
            (
                (2, 64, 0),
                crate::item::ItemStack::one("minecraft:cobblestone"),
            ),
        ];
        let out = merge_drop_stacks(&drops);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1.item(), "minecraft:cobblestone");
        assert_eq!(out[0].1.count(), 2);
        assert_eq!(out[1].1.item(), "minecraft:dirt");
        assert_eq!(out[1].1.count(), 1);
    }

    /// Nothing in, nothing out — and the empty case is not a panic.
    #[test]
    fn no_drops_merge_to_no_stacks() {
        assert!(merge_drop_stacks(&[]).is_empty());
    }

    // -- explode(), end to end ----------------------------------------------

    use super::{explode, ExplosionWorld, Outcome};
    use std::collections::BTreeSet;

    /// A world of stone with a floor: solid everywhere below y = 64, air at
    /// and above it, and every stone block drops one cobblestone. Mutates, so
    /// `createFire`'s `isAir` sees what `interactWithBlocks` did.
    #[derive(Default)]
    struct Fake {
        air: BTreeSet<(i32, i32, i32)>,
        fires: Vec<(i32, i32, i32)>,
        drop_calls: Vec<((i32, i32, i32), Option<f32>)>,
    }

    impl Fake {
        fn solid(&self, pos: (i32, i32, i32)) -> bool {
            pos.1 < 64 && !self.air.contains(&pos)
        }
    }

    impl ExplosionWorld for Fake {
        fn is_air(&self, pos: (i32, i32, i32)) -> bool {
            !self.solid(pos)
        }
        fn drops_from_explosion(&self, _pos: (i32, i32, i32)) -> bool {
            true
        }
        fn drops(
            &mut self,
            pos: (i32, i32, i32),
            explosion_radius: Option<f32>,
        ) -> Vec<crate::item::ItemStack> {
            self.drop_calls.push((pos, explosion_radius));
            vec![crate::item::ItemStack::one("minecraft:cobblestone")]
        }
        fn set_air(&mut self, pos: (i32, i32, i32)) {
            self.air.insert(pos);
        }
        fn solid_render_below(&self, pos: (i32, i32, i32)) -> bool {
            self.solid((pos.0, pos.1 - 1, pos.2))
        }
        fn set_fire(&mut self, pos: (i32, i32, i32)) {
            self.fires.push(pos);
        }
    }

    /// The floor probe the golden's `floor` world uses, so these tests and
    /// `tests/golden_explosion_ray.rs` are looking at the same geometry.
    fn floor_probe(table: &BlockExplosion) -> impl FnMut((i32, i32, i32)) -> Probe + '_ {
        move |pos| Probe {
            in_bounds: true,
            resistance: if pos.1 >= 64 {
                super::block_explosion_resistance(table, "minecraft:air", true, None)
            } else {
                super::block_explosion_resistance(table, "minecraft:stone", false, None)
            },
            should_explode: true,
        }
    }

    /// `KEEP` calls `interactWithBlocks` not at all, so the shuffle's draws are
    /// never spent and the visit order is the raw `HashSet` order. The
    /// draw-count difference is the measurement; a port that shuffled anyway
    /// and then discarded the result would leave the stream in the wrong place
    /// for `createFire`.
    #[test]
    fn keep_spends_no_shuffle_draws_and_removes_nothing() {
        let Some(table) = table() else { return };
        let mut r = Counting::new(42);
        let mut w = Fake::default();
        let out = explode(
            (0.5, 65.5, 0.5),
            4.0,
            false,
            BlockInteraction::Keep,
            &mut r,
            floor_probe(&table),
            &mut w,
        )
        .expect("ok");
        assert_eq!(r.floats, RAYS);
        assert_eq!(r.ints, 0, "KEEP spent shuffle draws");
        assert_eq!(out.visited, out.positions, "KEEP shuffled the list");
        assert!(out.removed.is_empty());
        assert!(out.drops.is_empty());
        assert!(w.drop_calls.is_empty());
    }

    /// `DESTROY` and `DESTROY_WITH_DECAY` differ in exactly one thing: whether
    /// `EXPLOSION_RADIUS` reaches the loot context. Passing `Some` for a plain
    /// `DESTROY` would turn on `survives_explosion` and delete most of the
    /// drops, which is why this is asserted on the parameter rather than on
    /// the outcome.
    #[test]
    fn only_destroy_with_decay_passes_the_explosion_radius() {
        let Some(table) = table() else { return };
        for (interaction, want) in [
            (BlockInteraction::Destroy, None),
            (BlockInteraction::DestroyWithDecay, Some(4.0f32)),
        ] {
            let mut r = LegacyRandomSource::new(42);
            let mut w = Fake::default();
            let out = explode(
                (0.5, 65.5, 0.5),
                4.0,
                false,
                interaction,
                &mut r,
                floor_probe(&table),
                &mut w,
            )
            .expect("ok");
            assert!(!out.removed.is_empty(), "{interaction:?} removed nothing");
            assert!(!w.drop_calls.is_empty());
            for (_, radius) in &w.drop_calls {
                assert_eq!(*radius, want, "{interaction:?}");
            }
        }
    }

    /// `TRIGGER_BLOCK` shuffles — `interactsWithBlocks()` is `!= KEEP` — and
    /// then breaks nothing, because `BlockBehaviour.onExplosionHit` returns on
    /// it at offset 20. Two questions, two answers, and this is the pair that
    /// separates them.
    #[test]
    fn trigger_block_shuffles_and_breaks_nothing() {
        let Some(table) = table() else { return };
        let mut r = Counting::new(42);
        let mut w = Fake::default();
        let out = explode(
            (0.5, 65.5, 0.5),
            4.0,
            false,
            BlockInteraction::TriggerBlock,
            &mut r,
            floor_probe(&table),
            &mut w,
        )
        .expect("ok");
        assert_eq!(
            r.ints,
            out.positions.len() - 1,
            "TRIGGER_BLOCK did not shuffle"
        );
        assert_ne!(
            out.visited, out.positions,
            "the shuffle left the list alone"
        );
        assert!(out.removed.is_empty(), "TRIGGER_BLOCK broke blocks");
    }

    /// `createFire` draws once per position **before** either block test, so
    /// the int draws are the position count and not the fire count.
    #[test]
    fn create_fire_draws_once_per_position_whatever_it_finds() {
        let Some(table) = table() else { return };
        let mut r = Counting::new(42);
        let mut w = Fake::default();
        let out = explode(
            (0.5, 65.5, 0.5),
            4.0,
            true,
            BlockInteraction::Keep,
            &mut r,
            floor_probe(&table),
            &mut w,
        )
        .expect("ok");
        // KEEP spends no shuffle draws, so every int is createFire's.
        assert_eq!(r.ints, out.positions.len());
        assert!(!out.fires.is_empty(), "nothing caught on a floor world");
        assert_eq!(out.fires, w.fires);
        // Fire only where the position is air over something solid.
        for f in &out.fires {
            assert!(f.1 >= 64, "fire at {f:?} is inside the floor");
        }
    }

    /// **`createFire` walks the list `interactWithBlocks` shuffled in place.**
    /// So the same seed and the same world give different fire positions
    /// depending on whether blocks were broken — the one list object is shared
    /// and `Util.shuffle` mutates it. A port that copied the list before
    /// shuffling would break the right blocks and burn the wrong ones, and
    /// this is the test that would catch it.
    #[test]
    fn the_fire_pass_inherits_the_shuffle() {
        let Some(table) = table() else { return };
        let run = |interaction| {
            let mut r = LegacyRandomSource::new(42);
            let mut w = Fake::default();
            let out = explode(
                (0.5, 65.5, 0.5),
                4.0,
                true,
                interaction,
                &mut r,
                floor_probe(&table),
                &mut w,
            )
            .expect("ok");
            out
        };
        let keep = run(BlockInteraction::Keep);
        let trigger = run(BlockInteraction::TriggerBlock);
        // Same seed, same world, same blocks broken (neither breaks any) —
        // and different fires, because one shuffled the list first.
        assert!(keep.removed.is_empty() && trigger.removed.is_empty());
        assert_ne!(
            keep.fires, trigger.fires,
            "the fire pass did not inherit the shuffle"
        );
    }

    /// `block_count` is the candidate count, taken before anything was
    /// removed, so it is larger than the number of blocks that actually broke.
    #[test]
    fn block_count_is_the_candidate_count_not_the_broken_count() {
        let Some(table) = table() else { return };
        let mut r = LegacyRandomSource::new(42);
        let mut w = Fake::default();
        let out = explode(
            (0.5, 65.5, 0.5),
            4.0,
            false,
            BlockInteraction::DestroyWithDecay,
            &mut r,
            floor_probe(&table),
            &mut w,
        )
        .expect("ok");
        assert_eq!(
            out.block_count(),
            i32::try_from(out.positions.len()).unwrap()
        );
        assert!(
            out.removed.len() < out.positions.len(),
            "every candidate was air-free, so this world has no air in it"
        );
        assert_eq!(out, Outcome { ..out.clone() });
    }

    /// `is_small` is the packet's particle choice and nothing else: under
    /// `2.0f`, or any radius at all under `KEEP`.
    #[test]
    fn is_small_is_the_radius_or_the_interaction() {
        use super::is_small;
        assert!(is_small(1.99, BlockInteraction::Destroy));
        assert!(!is_small(2.0, BlockInteraction::Destroy));
        assert!(is_small(100.0, BlockInteraction::Keep));
        assert!(!is_small(4.0, BlockInteraction::TriggerBlock));
    }
}

// ---------------------------------------------------------------------------
// The wire
// ---------------------------------------------------------------------------

/// One entry of the `explode` packet's `blockParticles` weighted list.
///
/// `ExplosionParticleInfo(ParticleOptions particle, float scaling, float speed)`,
/// paired with a `Weighted`'s `int` weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockParticle {
    /// The particle type's registry id, as a varint on the wire.
    pub particle: i32,
    /// `ExplosionParticleInfo.scaling()`.
    pub scaling: f32,
    /// `ExplosionParticleInfo.speed()`.
    pub speed: f32,
    /// `Weighted.weight()`. `WeightedList.Builder.add(E)` is `add(E, 1)`.
    pub weight: i32,
}

/// `Level.DEFAULT_EXPLOSION_BLOCK_PARTICLES`, from `Level.<clinit>` offsets
/// 54-92 — `POOF` at scaling 0.5f speed 1.0f then `SMOKE` at 1.0f/1.0f, both
/// at the implicit weight of 1.
///
/// The two registry ids are `minecraft:poof` and `minecraft:smoke`; they are
/// arguments rather than constants here because this workspace has no
/// particle-type id table, and `data/vanilla/explosion_wire.json` carries them
/// beside the recorded bytes.
#[must_use]
pub fn default_block_particles(poof: i32, smoke: i32) -> [BlockParticle; 2] {
    [
        BlockParticle {
            particle: poof,
            scaling: 0.5,
            speed: 1.0,
            weight: 1,
        },
        BlockParticle {
            particle: smoke,
            scaling: 1.0,
            speed: 1.0,
            weight: 1,
        },
    ]
}

/// `ServerExplosion.isSmall()`, offsets 0-21 —
/// `radius < 2.0f || !interactsWithBlocks()`.
///
/// `ServerLevel.explode` reads it at offsets 163-178 to choose between
/// `ParticleTypes.EXPLOSION` and `ParticleTypes.EXPLOSION_EMITTER`; that is
/// the only thing it decides. `LARGE_EXPLOSION_RADIUS` is the `2.0f`.
#[must_use]
pub fn is_small(radius: f32, block_interaction: BlockInteraction) -> bool {
    radius < 2.0 || !block_interaction.interacts_with_blocks()
}

/// `ClientboundExplodePacket`'s body — `rustmc_proto::ids::play::cb::EXPLODE`.
///
/// The record's seven fields, in `STREAM_CODEC.composite`'s order
/// (`ClientboundExplodePacket.<clinit>` offsets 0-77):
///
/// ```text
/// center            Vec3.STREAM_CODEC        3 x writeDouble
/// radius            ByteBufCodecs.FLOAT      writeFloat
/// blockCount        ByteBufCodecs.INT        writeInt — FOUR FIXED BYTES
/// playerKnockback   optional(Vec3)           bool, then 3 x writeDouble
/// explosionParticle ParticleTypes.STREAM_CODEC  varint type id, then the
///                                              type's own data — nothing for
///                                              a SimpleParticleType, which
///                                              both explosion particles are
/// explosionSound    SoundEvent.STREAM_CODEC  varint (registry id + 1)
/// blockParticles    WeightedList.streamCodec varint count, then per entry
///                                              the info then a varint weight
/// ```
///
/// **`blockCount` is `ByteBufCodecs.INT`, not `VAR_INT`.** `ByteBufCodecs$5`
/// is `ByteBuf.writeInt`. This is the same trap `crate::interact`'s
/// `level_event` carries, and it is the one a rewrite makes: a varint 1120 is
/// two bytes and a fixed int 1120 is four, so the client resynchronises on the
/// wrong byte and every later field is garbage. The recorded goldens include
/// two cases differing only in `blockCount` and assert they are the same
/// length.
///
/// `sound` is the registry id, **not** already incremented: the `+ 1` for the
/// `Holder`'s inline-event slot is applied here, exactly as
/// [`crate::sounds::sound_body`] does.
///
/// There is no separate `level_particles` packet for an explosion. The
/// particles ride on this one (`ServerLevel.explode` offsets 252-273), so a
/// server that also sent `minecraft:level_particles` would draw them twice.
///
/// # Errors
/// Only if the `Vec` write fails, which it does not.
pub fn explode_body(
    center: (f64, f64, f64),
    radius: f32,
    block_count: i32,
    player_knockback: Option<(f64, f64, f64)>,
    particle: i32,
    sound: i32,
    block_particles: &[BlockParticle],
) -> io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    use rustmc_proto::wire::{write_bool, write_f32, write_f64, write_i32};

    let mut out = Vec::new();
    write_f64(&mut out, center.0)?;
    write_f64(&mut out, center.1)?;
    write_f64(&mut out, center.2)?;
    write_f32(&mut out, radius)?;
    // Fixed, not varint. See the note above.
    write_i32(&mut out, block_count)?;
    match player_knockback {
        None => write_bool(&mut out, false)?,
        Some((x, y, z)) => {
            write_bool(&mut out, true)?;
            write_f64(&mut out, x)?;
            write_f64(&mut out, y)?;
            write_f64(&mut out, z)?;
        }
    }
    write_varint(&mut out, particle)?;
    write_varint(&mut out, sound + 1)?;
    write_varint(
        &mut out,
        i32::try_from(block_particles.len()).unwrap_or(i32::MAX),
    )?;
    for p in block_particles {
        write_varint(&mut out, p.particle)?;
        write_f32(&mut out, p.scaling)?;
        write_f32(&mut out, p.speed)?;
        write_varint(&mut out, p.weight)?;
    }
    Ok(out)
}

/// `ServerExplosion.getSeenPercent`'s counter, offsets 219-294 —
/// `(float) hits / (float) total` over [`seen_percent_samples`]'s grid.
///
/// `visible` stands in for
/// `level.clip(new ClipContext(from, center, COLLIDER, NONE, entity)).getType()
/// == MISS`, which this workspace cannot answer: `crate::collide` is an AABB
/// sweep and there is no `ClipContext`, no `BlockHitResult` and no
/// `traverseBlocks` anywhere in it. Injecting the predicate is what lets the
/// grid — which IS portable — be goldened against vanilla's own while the
/// raycast stays refused. See [`mod@refusals`].
///
/// The division is in `float`, over two `int` counters widened with `i2f`, and
/// not in `double`: `43.0f / 45.0f` is not `(float)(43.0 / 45.0)` for every
/// pair.
///
/// Returns `0.0` for an empty grid rather than dividing by zero. Vanilla
/// cannot reach that: the `dx < 0.0 || dy < 0.0 || dz < 0.0` guard returns
/// early and any box that passes it emits at least one sample.
#[must_use]
pub fn seen_percent(
    samples: &[(f64, f64, f64)],
    mut visible: impl FnMut((f64, f64, f64)) -> bool,
) -> f32 {
    let mut hits = 0u32;
    let mut total = 0u32;
    for &s in samples {
        if visible(s) {
            hits += 1;
        }
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        hits as f32 / total as f32
    }
}

// ---------------------------------------------------------------------------
// The aftermath, and the whole of `ServerExplosion.explode()`
// ---------------------------------------------------------------------------

/// What the aftermath needs of a world, one method per call
/// `interactWithBlocks` and `createFire` make.
///
/// A trait rather than a closure because there are six questions and two of
/// them mutate; a tuple of six closures would not be readable and the borrow
/// checker would make it worse. The live implementation is the one thing this
/// port is missing and it is missing for a reason — see [`mod@refusals`].
pub trait ExplosionWorld {
    /// `state.isAir()`. `BlockBehaviour.onExplosionHit` returns immediately on
    /// air (offsets 0-20), so an air position is visited and does nothing.
    fn is_air(&self, pos: (i32, i32, i32)) -> bool;

    /// `state.getBlock().dropFromExplosion(explosion)`. `false` for
    /// `minecraft:tnt` and for nothing else in 26.2; the table's
    /// `drop_from_explosion` column is the answer.
    fn drops_from_explosion(&self, pos: (i32, i32, i32)) -> bool;

    /// `state.getDrops(builder)` with `LootContextParams.ORIGIN` at the block
    /// centre, `TOOL` empty, and `EXPLOSION_RADIUS` present **only** when the
    /// interaction is `DESTROY_WITH_DECAY` (offsets 117-148). That parameter
    /// is what turns on `survives_explosion` and `explosion_decay` in
    /// `rustmc_loot`, so passing `Some` for a plain `DESTROY` would silently
    /// delete most of the drops.
    fn drops(
        &mut self,
        pos: (i32, i32, i32),
        explosion_radius: Option<f32>,
    ) -> Vec<crate::item::ItemStack>;

    /// `level.setBlock(pos, Blocks.AIR.defaultBlockState(), 3)` — flags
    /// `UPDATE_NEIGHBORS | UPDATE_CLIENTS`. Called for every non-air position
    /// whether or not it dropped anything (offsets 179-191).
    fn set_air(&mut self, pos: (i32, i32, i32));

    /// `level.getBlockState(pos.below()).isSolidRender()`, which `createFire`
    /// asks after it has already drawn.
    fn solid_render_below(&self, pos: (i32, i32, i32)) -> bool;

    /// `level.setBlockAndUpdate(pos, BaseFireBlock.getState(level, pos))` —
    /// the state is `soul_fire` over a soul-fire base and `fire` otherwise,
    /// which is `BaseFireBlock.getState`'s whole content.
    fn set_fire(&mut self, pos: (i32, i32, i32));
}

/// What one explosion did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// The candidate positions in `java.util.HashSet` order, before the
    /// shuffle. `positions.len()` is what `explode()` returns and what the
    /// packet's `blockCount` carries.
    pub positions: Vec<(i32, i32, i32)>,
    /// The same positions after `Util.shuffle`, which is the order
    /// `interactWithBlocks` and then `createFire` walk. Equal to
    /// [`Self::positions`] when the interaction is `KEEP`, because
    /// `interactWithBlocks` — and so the shuffle — is not called at all.
    pub visited: Vec<(i32, i32, i32)>,
    /// The positions set to air, in visit order.
    pub removed: Vec<(i32, i32, i32)>,
    /// The merged drop stacks, in `StackCollector` order. Vanilla's next step
    /// is `Block.popResource` for each; see [`mod@refusals`].
    pub drops: Vec<((i32, i32, i32), crate::item::ItemStack)>,
    /// The positions fire was placed at, in visit order.
    pub fires: Vec<(i32, i32, i32)>,
}

impl Outcome {
    /// `ServerExplosion.explode()`'s return value and the packet's
    /// `blockCount` — the size of the candidate list, counted **before**
    /// anything was removed.
    #[must_use]
    pub fn block_count(&self) -> i32 {
        i32::try_from(self.positions.len()).unwrap_or(i32::MAX)
    }
}

/// `ServerExplosion.explode()`, offsets 0-76, with the world it needs.
///
/// ```text
///  0: level.gameEvent(source, GameEvent.EXPLODE, center)   — no RNG, not modelled
/// 18: List<BlockPos> list = calculateExplodedPositions()   — 1352 nextFloat()
/// 23: hurtEntities()                                       — REFUSED, see below
/// 27: if (interactsWithBlocks()) interactWithBlocks(list)  — shuffle, then walk
/// 58: if (this.fire) createFire(list)                      — one nextInt(3) each
/// 70: return list.size()
/// ```
///
/// **`createFire` walks the SHUFFLED list.** `explode()` holds one `List`
/// object; `Util.shuffle` permutes it in place at the head of
/// `interactWithBlocks`, and `createFire` is handed that same object
/// afterwards. So which positions catch fire depends on whether the
/// interaction was `KEEP` — under `KEEP` neither the shuffle draws nor the
/// permutation happen, and the `nextInt(3)` draws then land on a differently
/// ordered list. A port that copied the list before shuffling would get the
/// right blocks broken and the wrong ones burning.
///
/// **`hurtEntities` is skipped and that does not disturb the stream.** It
/// spends no `level.random` draw on any path: `getSeenPercent` raycasts,
/// `getEntityDamageAmount` is arithmetic, `Entity.push` is arithmetic, and
/// anything `Entity.hurtServer` draws comes from the entity's own
/// `RandomSource` and not the level's. So the ray walk's 1,352 floats are
/// followed immediately by the shuffle's draws, exactly as they would be with
/// entities present. That is an assumption about a method this tree cannot
/// run, and it is stated here rather than left implicit.
///
/// # Errors
/// [`Untracked`](rustmc_data::java_hash_set::Untracked), from
/// [`calculate_exploded_positions`].
pub fn explode<R, F, W>(
    center: (f64, f64, f64),
    radius: f32,
    fire: bool,
    block_interaction: BlockInteraction,
    random: &mut R,
    probe: F,
    world: &mut W,
) -> Result<Outcome, rustmc_data::java_hash_set::Untracked>
where
    R: rustmc_worldgen::rng::RandomSource,
    F: FnMut((i32, i32, i32)) -> Probe,
    W: ExplosionWorld + ?Sized,
{
    let positions = calculate_exploded_positions(center, radius, random, probe)?;
    let mut out = Outcome {
        visited: positions.clone(),
        positions,
        ..Outcome::default()
    };

    if block_interaction.interacts_with_blocks() {
        // `Util.shuffle(list, this.level.random)` — in place, and the same
        // list `createFire` is handed below.
        shuffle(&mut out.visited, random);

        // `BlockBehaviour.onExplosionHit`, offsets 0-201, for each position in
        // turn, collecting drops into one `StackCollector` list.
        let mut raw: Vec<((i32, i32, i32), crate::item::ItemStack)> = Vec::new();
        for &pos in &out.visited {
            // `if (state.isAir() || explosion.getBlockInteraction() == TRIGGER_BLOCK) return;`
            if world.is_air(pos) || block_interaction == BlockInteraction::TriggerBlock {
                continue;
            }
            if world.drops_from_explosion(pos) {
                // `EXPLOSION_RADIUS` is added only under DESTROY_WITH_DECAY.
                let radius_param = if block_interaction == BlockInteraction::DestroyWithDecay {
                    Some(radius)
                } else {
                    None
                };
                for stack in world.drops(pos, radius_param) {
                    raw.push((pos, stack));
                }
            }
            // Unconditional: a block that dropped nothing is still removed.
            world.set_air(pos);
            out.removed.push(pos);
            // `block.wasExploded(level, pos, explosion)` — a no-op for every
            // block but `minecraft:tnt`, whose override primes a `PrimedTnt`.
            // Refused; see `refusals`.
        }
        out.drops = merge_drop_stacks(&raw);
    }

    if fire {
        out.fires = create_fire(&out.visited, random, world);
    }

    Ok(out)
}

/// `ServerExplosion.createFire(List<BlockPos>)`, offsets 0-93.
///
/// ```text
///  26: if (this.level.random.nextInt(3) != 0) continue;
///  42: if (!this.level.getBlockState(pos).isAir()) continue;
///  56: if (!this.level.getBlockState(pos.below()).isSolidRender()) continue;
///  73: this.level.setBlockAndUpdate(pos, BaseFireBlock.getState(this.level, pos));
/// ```
///
/// The draw comes **first**, before either block test, so the draw count is
/// the position count whatever the world looks like — a world of solid stone
/// spends exactly as many `nextInt(3)`s as an empty one.
///
/// Separate from [`explode`] so that a golden can drive it over a chosen order
/// and a chosen stream state. `explode` passes it `Outcome::visited`, which is
/// the shuffled list when the interaction was anything but `KEEP` and the raw
/// `HashSet` order when it was — see [`explode`]'s note.
pub fn create_fire<R, W>(
    positions: &[(i32, i32, i32)],
    random: &mut R,
    world: &mut W,
) -> Vec<(i32, i32, i32)>
where
    R: rustmc_worldgen::rng::RandomSource,
    W: ExplosionWorld + ?Sized,
{
    let mut out = Vec::new();
    for &pos in positions {
        if random.next_i32_bound(3) != 0 {
            continue;
        }
        if !world.is_air(pos) {
            continue;
        }
        if !world.solid_render_below(pos) {
            continue;
        }
        world.set_fire(pos);
        out.push(pos);
    }
    out
}
