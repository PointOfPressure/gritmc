//! Random ticks: which blocks the clock chooses, and how many of them.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `ServerChunkCache.tickChunks`, `ChunkMap.forEachBlockTickingChunk`,
//! `DistanceManager.forEachEntityTickingChunk`, `ServerLevel.tickChunk`,
//! `Level.getBlockRandomPos`, `LevelChunkSection.isRandomlyTicking` and
//! `BlockBehaviour$BlockStateBase.isRandomlyTicking`.
//!
//! # What this module is, and what it is not
//!
//! It is the **selection** half of random ticking: given a set of chunks and
//! a way to read the world, it produces exactly the sequence of positions
//! vanilla's loop would produce, in vanilla's order, at vanilla's rate.
//!
//! It is not any block's behaviour. Not one `randomTick` override is ported
//! yet, and that is deliberate rather than unfinished — see
//! "why this is not wired into the tick loop" below.
//!
//! # The rate, which is the part that has to be exact
//!
//! An effect at the wrong rate is not a smaller version of the right one: it
//! diverges on every measurement window, and it produces the right *kind* of
//! event throughout, so nothing that only asks "did anything happen" can see
//! it. The whole rate is three facts and they come from three different
//! methods:
//!
//! 1. **which chunks, and the method's name is wrong about it.**
//!    `ServerChunkCache.tickChunks` calls
//!    `chunkMap.forEachBlockTickingChunk(c -> level.tickChunk(c, speed))`.
//!    `forEachBlockTickingChunk` is one line and it delegates to
//!    `DistanceManager.forEachEntityTickingChunk`, which walks
//!    `simulationChunkTracker.chunks` and admits a chunk on
//!    **`ChunkLevel.isEntityTicking(level)`**.
//!
//!    That is not the same predicate as the name suggests, and `ChunkLevel`
//!    carries both so the difference is not hypothetical:
//!
//!    ```text
//!      ChunkLevel.isBlockTicking(l)   l <= 32   BLOCK_TICKING_LEVEL
//!      ChunkLevel.isEntityTicking(l)  l <= 31   ENTITY_TICKING_LEVEL
//!    ```
//!
//!    So random ticks run on the **entity**-ticking set, which is a strict
//!    subset of the block-ticking set — one chunk level tighter. A port that
//!    read the method's name and reached for `isBlockTicking` would random
//!    tick a whole ring of chunks vanilla leaves alone, at exactly the right
//!    rate everywhere inside it, which is the kind of error only a count at
//!    the boundary can see. And both are *simulation* distance, not view
//!    distance: a chunk a player can see but not simulate never random
//!    ticks;
//! 2. **how many per section.** `ServerLevel.tickChunk`'s block pass is
//!    `for (j = 0; j < speed; j++)` **inside** the per-section loop, so the
//!    count is `speed` picks per *randomly-ticking section* per chunk per
//!    tick — not `speed` per chunk, which is the reading that turns a 24-
//!    section column into a rate 24 times too low;
//! 3. **which sections are skipped.** `LevelChunkSection.isRandomlyTicking()`
//!    is `tickingBlockCount > 0 || tickingFluidCount > 0`. A section holding
//!    nothing that random ticks is passed over **before** any position is
//!    drawn, so it does not consume the generator either. Skipping it after
//!    the draw would give the right blocks and the wrong stream.
//!
//! The gamerule is `random_tick_speed` and it is an **integer**, not a
//! boolean — in 26.2 `doFireTick` became the integer
//! `fire_spread_radius_around_player` and this one kept its type but changed
//! its spelling. Vanilla's default is 3.
//!
//! # The ice-and-snow pass runs first and it is not free
//!
//! `tickChunk` opens with a second loop, before the block pass and outside
//! the `ifle` at offset 87 that guards it — though **not** outside `speed`:
//! its own bound is `i < speed`, so at speed 0 neither pass runs and neither
//! generator moves. That is why [`Chunk::block_pass`] returning nothing at
//! speed 0 is the whole of the tick and not half of it.
//!
//! ```text
//!   for (i = 0; i < speed; i++)
//!       if (random.nextInt(48) == 0)
//!           tickPrecipitation(getBlockRandomPos(minX, 0, minZ, 15));
//! ```
//!
//! It matters here for one reason that has nothing to do with ice: **every
//! success advances the same `randValue` the block pass draws from**, and
//! every iteration consumes a draw from `level.random` whether it succeeds or
//! not. So the two passes share both generators, and a port that implements
//! the block pass alone has the right positions in the wrong stream
//! positions from the first chunk that freezes. It is recorded here rather
//! than ported because `tickPrecipitation` needs a heightmap, a biome and
//! `Biome.shouldFreeze`, none of which this module has any business owning.
//!
//! [`Chunk::PRECIPITATION_ATTEMPTS_PER_TICK`] carries the count so the debt
//! is a named constant rather than a paragraph.
//!
//! **And as of 2026-09-06 the debt is measured and refused rather than
//! merely named.** `crate::weather` transcribes the same loop from the other
//! side and the arithmetic is in `docs/PARITY-ROADMAP.md` §107: ice
//! (`Biome.shouldFreeze`) and snow (`Biome.shouldSnow`) both reduce to
//! `getTemperature(pos, seaLevel) >= 0.15f` and this server's superflat
//! biome is `minecraft:plains` at 0.8, so both are false at every position
//! by construction — not slow, unreachable. The third effect,
//! `LayeredCauldronBlock.handlePrecipitation`, survives the biome and needs
//! 56.5 s of settle for 99% confidence at `random_tick_speed = 1000`. So
//! the stream-position hazard this section warns about is real and cannot
//! be observed here, which is worth knowing before anyone spends a turn
//! trying to.
//!
//! # It is the whole loop, since 2026-10-01
//!
//! From 2026-08-17 this module ticked one section per chunk — the flat
//! preset's grass layer — and ran one body, grass's covered arm. That is
//! gone. [`tick`] now walks every section of every chunk in range, skips the
//! quiet ones by `LevelChunkSection.isRandomlyTicking`
//! ([`crate::edits::Blocks::random_ticking_sections`]), and hands every
//! randomly-ticking state to its class's `randomTick` in [`crate::growth`],
//! which carries the bodies and the census of what is not ported.
//!
//! The corpus can observe it because a scenario may declare `[[gamerule]]`
//! (`oracle_runner::scenario::GameRule`, `docs/PARITY-ROADMAP.md` §22) or
//! set the rule from a `console` step, and `ci/t2-gameplay.sh` sends the
//! same traffic to both servers. The position function is gated against
//! vanilla's own by `crates/rustmc-server/golden/random_tick_pos.json`
//! (`ci/random-tick-golden.sh`, from `Level.getBlockRandomPos` directly).

/// `Level.getBlockRandomPos`'s addend — `ldc 1013904223`, written into the
/// method body rather than read from the `addend` field beside it.
const ADDEND: i32 = 1_013_904_223;

/// `Level.getBlockRandomPos`'s multiplier — `iconst_3`.
const MULTIPLIER: i32 = 3;

/// `Level.randValue`, and the position function that advances it.
///
/// # The transition, offsets 0-55, entire
///
/// ```text
///    0: randValue = randValue * 3 + 1013904223;
///   13: int r = randValue >> 2;
///   21: new BlockPos(x + (r       & 15),
///                    y + (r >> 16 & mask),
///                    z + (r >>  8 & 15));
/// ```
///
/// **`y` takes bits 16.. and `z` takes bits 8..**, and `y` is the only one of
/// the three that consults `mask`. That ordering is the reverse of the one a
/// plausible transcription reaches for, and getting it backwards is invisible
/// to every test that is not a golden vector: the positions stay uniform,
/// stay inside the section, and arrive at the same rate. It is pinned by
/// `tests/random_tick_pos.rs` against vanilla running the method.
///
/// `>> 2` is `ishr`, an **arithmetic** shift, and that is written because the
/// bytecode says so and **not** because anything can tell: an `iushr` port
/// produces byte-identical positions for every input. The two shifted values
/// differ only in bits 30-31 and this function reads bits 0-19. Measured over
/// 20,008 seeds, zero disagreements — see
/// `tests::the_shift_kind_cannot_be_observed_through_the_masked_fields`,
/// which exists because the first draft of this file claimed the opposite in
/// three places.
///
/// The multiply wraps at 32 bits (`wrapping_mul`); Java's `imul` has no other
/// behaviour, and **that** one is observable — it moves `randValue` itself,
/// which the golden compares as `endValue`. That is what the negative seeds
/// actually earn their place for.
///
/// # It is not reproducible across servers and nothing should pretend it is
///
/// `Level`'s constructor sets `randValue` from
/// `RandomSource.createThreadLocalInstance().nextInt()` — unseeded. Two
/// vanilla servers of this version, on the same world, at the same tick,
/// choose different random tick positions and always will. So a check that
/// compares *positions* against a live vanilla can never pass, vanilla
/// against vanilla included, and the only comparable things are the
/// transition (pinned here), the rate (pinned in [`Chunk`]) and a
/// distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRandomPos {
    rand_value: i32,
}

impl BlockRandomPos {
    /// A generator at a known `randValue`.
    ///
    /// There is no `new()` and no `Default`: the whole point of the type is
    /// that the starting value is unknowable on a real server and arbitrary
    /// everywhere else, so a caller has to state one.
    #[must_use]
    pub const fn at(rand_value: i32) -> Self {
        Self { rand_value }
    }

    /// The current `randValue`. For the golden, which compares the state the
    /// generator is left in as well as the positions it produced — an
    /// implementation can agree on every drawn position and still advance by
    /// the wrong amount, and that shows up on the next draw rather than on
    /// any of the compared ones.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.rand_value
    }

    /// `Level.getBlockRandomPos(x, y, z, mask)`.
    pub fn next(&mut self, x: i32, y: i32, z: i32, mask: i32) -> (i32, i32, i32) {
        self.rand_value = self
            .rand_value
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(ADDEND);
        let r = self.rand_value >> 2;
        (x + (r & 15), y + ((r >> 16) & mask), z + ((r >> 8) & 15))
    }
}

/// One chunk's worth of `ServerLevel.tickChunk`, block pass only.
///
/// A chunk is described by where its sections start and which of them are
/// randomly ticking, because those are the only two things the selection loop
/// asks — the states themselves are the caller's, and handing this a world
/// would make it a world-walker rather than a loop.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// `ChunkPos.getMinBlockX()`.
    pub min_block_x: i32,
    /// `ChunkPos.getMinBlockZ()`.
    pub min_block_z: i32,
    /// `chunk.getMinSectionY()`. Section index `i` covers block y
    /// `(min_section_y + i) << 4 ..= +15`, from
    /// `SectionPos.sectionToBlockCoord(getSectionYFromSectionIndex(i))` and
    /// `getSectionYFromSectionIndex(i) = i + getMinSectionY()`.
    pub min_section_y: i32,
    /// `LevelChunkSection.isRandomlyTicking()` per section, in
    /// `chunk.getSections()` order — index 0 is the **lowest** section.
    ///
    /// `isRandomlyTicking()` is `tickingBlockCount > 0 || tickingFluidCount >
    /// 0`, a pair of counters vanilla maintains as blocks are set. This
    /// server has no such counter, so whoever fills this answers the same
    /// question by whatever means it has — and the answer has to be "does the
    /// section contain a randomly-ticking **state or fluid**", not "a
    /// randomly-ticking block", because the two differ for every state of the
    /// twenty-five classes that override `isRandomlyTicking` per state.
    pub randomly_ticking: Vec<bool>,
}

impl Chunk {
    /// How many `level.random.nextInt(48)` draws the ice-and-snow pass makes
    /// per chunk per tick, as a multiple of the tick speed.
    ///
    /// One per iteration and the loop runs `speed` times, so it is exactly
    /// `speed`. Named because the pass is **not ported** and this is the size
    /// of that debt: the block pass below shares both `level.random` and
    /// `randValue` with it, so a stream this module produces is only
    /// vanilla's stream on a chunk where nothing froze and nothing snowed.
    pub const PRECIPITATION_ATTEMPTS_PER_TICK: i32 = 1;

    /// The positions `tickChunk`'s block pass would draw, in order.
    ///
    /// Advances `rng` exactly as vanilla advances `randValue`: once per pick,
    /// and **not at all** for a section that is not randomly ticking.
    ///
    /// Returns nothing when `speed <= 0`, which is `tickChunk`'s own
    /// `ifle` at offset 87 and not a convenience: at speed 0 vanilla draws no
    /// position for any section of any chunk, so the generator does not move
    /// either.
    #[must_use]
    pub fn block_pass(&self, rng: &mut BlockRandomPos, speed: i32) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        if speed <= 0 {
            return out;
        }
        for (index, ticking) in self.randomly_ticking.iter().enumerate() {
            if !*ticking {
                continue;
            }
            let section_y = self.min_section_y + i32::try_from(index).unwrap_or(i32::MAX);
            let min_y = section_y << 4;
            for _ in 0..speed {
                out.push(rng.next(self.min_block_x, min_y, self.min_block_z, 15));
            }
        }
        out
    }
}

/// How far around each connected player this server draws random ticks, in
/// chunks.
///
/// **A deliberate, documented deviation from vanilla's own geometry.**
/// Vanilla's real set is the entity-ticking chunk map — a per-player
/// simulation-distance disc this server does not track (see this module's
/// top doc, "which chunks, and the method's name is wrong about it"). A
/// small fixed radius stands in for it, covering every corpus rig, which
/// sits a few blocks from the acting player.
///
/// Public because `crate::behaviour::tick_block_entities` stands in for
/// vanilla's `shouldTickBlocksAt` with the same disc. Two passes each having
/// their own opinion about which chunks this server simulates is a
/// disagreement nothing would surface, and vanilla asks one question —
/// `DistanceManager.inBlockTickingRange` — of both.
pub const CHUNK_RADIUS: i32 = 2;

/// What this radius would be if it were derived rather than fixed: the
/// Chebyshev square of [`crate::tickets::entity_ticking_radius`] around each
/// player, which is the simulation distance exactly (chunk-streaming). Not
/// closed for the cost the module header prices; the test below keeps the
/// size of the deviation a number.
#[cfg(test)]
const fn faithful_chunk_radius(simulation_distance: i32) -> i32 {
    crate::tickets::entity_ticking_radius(simulation_distance)
}

/// One server-wide draw stream, matching `Level.randValue`: vanilla keeps
/// **one** generator per level, shared by every chunk's block pass in a
/// tick, advancing continuously tick to tick rather than being reseeded.
/// Seeded from the wall clock at first use — arbitrarily, and correctly so:
/// [`BlockRandomPos`]'s own doc establishes that two real vanilla servers of
/// this version disagree on this value too, always, so there is no "right"
/// seed to match.
fn stream() -> &'static std::sync::Mutex<BlockRandomPos> {
    static STREAM: std::sync::OnceLock<std::sync::Mutex<BlockRandomPos>> =
        std::sync::OnceLock::new();
    STREAM.get_or_init(|| {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as i32)
            .unwrap_or(0);
        std::sync::Mutex::new(BlockRandomPos::at(seed))
    })
}

/// What a pass did that the caller has to account for.
#[derive(Default)]
pub struct Pass {
    /// Every write and its cascade, in vanilla's order — the caller sends
    /// the positions at the flush and drains the tick requests.
    pub effects: crate::shapes::Effects,
    /// How many picks landed on a randomly-ticking state whose nearest
    /// `randomTick` declarer is not ported, by declaring block. Picks on a
    /// randomly-ticking fluid (lava) are counted under the fluid's block.
    pub unported: std::collections::BTreeMap<String, u32>,
}

/// One tick of `ServerChunkCache.tickChunks`' random-tick half:
/// `ServerLevel.tickChunk(chunk, speed)` for every chunk in range, in chunk
/// order.
///
/// Per chunk, exactly vanilla's two loops (offsets 32-267):
///
/// * the ice-and-snow pass — `speed` draws of `level.random.nextInt(48)`,
///   each success taking one position off `randValue` for
///   `tickPrecipitation`. The draws and the position are taken; the body is
///   not ported (`crate::weather` §107: on this server's plains it cannot
///   freeze or snow, and the cauldron arm is not this module's);
/// * the block pass — `speed` positions per randomly-ticking section
///   ([`crate::edits::Blocks::random_ticking_sections`]), each read once and
///   handed to its block's `randomTick` ([`crate::growth`]) when the state
///   random-ticks.
///
/// Vanilla re-reads `isRandomlyTicking` per section as its loop reaches the
/// section; this reads the column once, at the start of the chunk. A write
/// in one section that turns a later section of the **same chunk** on or
/// off in the same tick is the difference, and it moves only the position
/// stream, which is unseeded on both servers.
#[allow(clippy::too_many_arguments)]
pub fn tick(
    world: &dyn crate::edits::Blocks,
    shapes: Option<&crate::shapes::ShapeTable>,
    classes: Option<&crate::placedby::PlacedByTable>,
    players: &[crate::tracker::TrackedPlayer],
    speed: i32,
    game_time: i64,
) -> Pass {
    use rustmc_worldgen::rng::RandomSource;
    let mut pass = Pass::default();
    if speed <= 0 {
        return pass;
    }
    let (Some(classes), Some(props)) = (classes, world.place_props()) else {
        return pass;
    };
    let mut chunks: std::collections::BTreeSet<(i32, i32)> = std::collections::BTreeSet::new();
    for p in players {
        let (cx, cz) = p.chunk;
        for dx in -CHUNK_RADIUS..=CHUNK_RADIUS {
            for dz in -CHUNK_RADIUS..=CHUNK_RADIUS {
                chunks.insert((cx + dx, cz + dz));
            }
        }
    }
    let sky = crate::sky::active();
    let top_y = sky.map_or(320, crate::sky::SkyModel::top_y);
    let has_sky = sky.is_none_or(crate::sky::SkyModel::has_skylight);
    let darken = sky.map_or(0, |s| s.sky_darken(crate::sky::day_time(game_time)));
    let light = crate::brightness::Light::new(world, top_y, has_sky, darken);
    let raining = crate::weather::shared()
        .lock()
        .is_ok_and(|w| w.is_raining());
    let mut positions = stream()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut level = crate::weather::level_random()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut kinds: std::collections::HashMap<String, Option<crate::growth::Kind>> =
        std::collections::HashMap::new();
    for (cx, cz) in chunks {
        let (min_x, min_z) = (cx * 16, cz * 16);
        for _ in 0..speed {
            if level.next_i32_bound(48) == 0 {
                let _ = positions.next(min_x, 0, min_z, 15);
            }
        }
        let Some((min_section_y, randomly_ticking)) = world.random_ticking_sections(cx, cz) else {
            continue;
        };
        let chunk = Chunk {
            min_block_x: min_x,
            min_block_z: min_z,
            min_section_y,
            randomly_ticking,
        };
        for pos in chunk.block_pass(&mut positions, speed) {
            let state = world.state_at(pos);
            let Some(p) = props.get(&state) else {
                continue;
            };
            let block = rustmc_behavior::place_survive::block_of(&state);
            if p.randomly_ticking {
                let kind = *kinds
                    .entry(block.to_string())
                    .or_insert_with(|| crate::growth::Kind::of(classes, block));
                match kind {
                    Some(kind) => {
                        let mut cx = crate::growth::Ctx {
                            world,
                            shapes,
                            light: &light,
                            rng: &mut *level,
                            fx: &mut pass.effects,
                            raining,
                        };
                        crate::growth::random_tick(&mut cx, kind, pos, &state);
                    }
                    None => *pass.unported.entry(block.to_string()).or_default() += 1,
                }
            }
            // `FluidState.isRandomlyTicking` — lava's fire spread. Not ported.
            if p.fluid_randomly_ticking {
                *pass.unported.entry(block.to_string()).or_default() += 1;
            }
        }
    }
    pass
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chunk of `n` sections starting at `min_section_y`, all ticking.
    fn chunk(min_block_x: i32, min_block_z: i32, min_section_y: i32, n: usize) -> Chunk {
        Chunk {
            min_block_x,
            min_block_z,
            min_section_y,
            randomly_ticking: vec![true; n],
        }
    }

    /// **The rate, stated as a count.** `speed` picks per randomly-ticking
    /// section per chunk per tick — the reading that says "per chunk" gives 3
    /// here and is wrong by the number of sections.
    #[test]
    fn the_rate_is_speed_picks_per_randomly_ticking_section() {
        let c = chunk(0, 0, -4, 24);
        for speed in [1, 3, 7] {
            let mut rng = BlockRandomPos::at(0);
            assert_eq!(
                c.block_pass(&mut rng, speed).len(),
                24 * usize::try_from(speed).expect("positive"),
                "speed {speed} should draw {speed} per section over 24 sections"
            );
        }
    }

    /// A section that is not randomly ticking is skipped **before** the draw,
    /// so it costs no position and no generator step. Two chunks with the
    /// same ticking sections in different places must therefore produce the
    /// same *number* of picks and leave the generator in the same state.
    #[test]
    fn a_quiet_section_consumes_no_draw() {
        let mut all = chunk(0, 0, 0, 8);
        all.randomly_ticking = vec![false, true, false, false, true, false, false, false];
        let mut some = chunk(0, 0, 0, 8);
        some.randomly_ticking = vec![true, true, false, false, false, false, false, false];

        let mut a = BlockRandomPos::at(12345);
        let mut b = BlockRandomPos::at(12345);
        let pa = all.block_pass(&mut a, 3);
        let pb = some.block_pass(&mut b, 3);
        assert_eq!(pa.len(), 6);
        assert_eq!(pb.len(), 6);
        assert_eq!(
            a.value(),
            b.value(),
            "the generator advanced by the number of PICKS, not the number of sections"
        );
        // Same draws, different section bases: the x and z agree and the y
        // does not, which is what says the skip happened before the draw
        // rather than after it.
        assert_eq!(pa[0].0, pb[0].0);
        assert_eq!(pa[0].2, pb[0].2);
        assert_ne!(pa[0].1, pb[0].1);
    }

    /// `tickChunk`'s `ifle` at offset 87. At speed 0 nothing is drawn and the
    /// generator does not move — which is the difference between "the rate is
    /// zero" and "the rate is zero and the stream still advances".
    ///
    /// **What this cannot tell you, measured rather than assumed.** Deleting
    /// the `speed <= 0` early return leaves this test green: the inner
    /// `for _ in 0..speed` is already an empty range at zero and at every
    /// negative, so the guard changes no behaviour at all. The sweep of
    /// 2026-08-06 confirmed it — all sixteen tests green with the guard
    /// removed.
    ///
    /// The guard stays because vanilla has the branch and a port should not
    /// silently disagree with its source, and because it does change *cost*:
    /// vanilla's `ifle` skips `getSections()` and a per-section
    /// `isRandomlyTicking()` call for every chunk on every tick at speed 0,
    /// which is the setting this server ships. That is a real saving and it
    /// is not what this test measures.
    #[test]
    fn speed_zero_draws_nothing_and_moves_nothing() {
        let c = chunk(0, 0, 0, 24);
        let mut rng = BlockRandomPos::at(-7);
        assert!(c.block_pass(&mut rng, 0).is_empty());
        assert!(c.block_pass(&mut rng, -1).is_empty());
        assert_eq!(rng.value(), -7);
    }

    /// Every drawn position lands inside the section it was drawn for.
    ///
    /// **This test is weak on purpose to be measured, and it was: it catches
    /// almost nothing.** Written down here rather than left to be discovered,
    /// because a containment assertion reads like coverage.
    ///
    /// The falsification sweep of 2026-08-06 put two injected bugs through it
    /// and it stayed green for both — a `y`/`z` shift swap (both components
    /// still come from 4-bit masked fields, so both stay in range) and a
    /// section base left as a section index instead of a block coordinate
    /// (a 24-section chunk is tall enough to swallow it). The golden catches
    /// the first; `picks_are_grouped_by_section_in_order_and_land_in_that_sections_band`
    /// was written for the second and catches both.
    #[test]
    fn every_pick_is_inside_its_own_section() {
        let c = chunk(-32, 48, -4, 24);
        let mut rng = BlockRandomPos::at(1_577_290_342);
        for (x, y, z) in c.block_pass(&mut rng, 3) {
            assert!((x - c.min_block_x) as u32 <= 15, "x {x} out of section");
            assert!((z - c.min_block_z) as u32 <= 15, "z {z} out of section");
            let section = (y >> 4) - c.min_section_y;
            assert!(
                (0..24).contains(&section),
                "y {y} is in section {section}, outside the chunk"
            );
        }
    }

    /// **The test that the containment test above should have been.**
    ///
    /// `every_pick_is_inside_its_own_section` asks only that a pick lands
    /// somewhere in the chunk, and the falsification sweep of 2026-08-06
    /// showed how little that buys: it stays green with the section base
    /// left as a *section* index instead of a block coordinate (`min_y =
    /// section_y` rather than `section_y << 4`), because a 24-section chunk
    /// is tall enough to swallow the error. Two injected bugs passed it.
    ///
    /// This pins the actual structure of the walk instead:
    ///
    /// * picks come out in **ticking-section order, lowest first**;
    /// * there are exactly `speed` of them per ticking section, **contiguous**;
    /// * pick `k`'s y lies in that section's own 16-block band, which is
    ///   `(min_section_y + section_index) << 4 ..= + 15`.
    ///
    /// So the section base, the grouping and the ordering are all one
    /// assertion, and none of the three can be got wrong quietly.
    #[test]
    fn picks_are_grouped_by_section_in_order_and_land_in_that_sections_band() {
        let mut c = chunk(-32, 48, -4, 8);
        // A deliberately gappy chunk: leading gap, interior gap, trailing
        // gap. A walk that ignored `randomly_ticking` would have to guess
        // which three of the eight these are.
        c.randomly_ticking = vec![false, true, false, true, true, false, false, false];
        let ticking: Vec<usize> = c
            .randomly_ticking
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.then_some(i))
            .collect();
        assert_eq!(
            ticking,
            vec![1, 3, 4],
            "the fixture is the thing being read"
        );

        let speed = 3;
        let mut rng = BlockRandomPos::at(20_260_806);
        let picks = c.block_pass(&mut rng, speed);
        assert_eq!(
            picks.len(),
            ticking.len() * speed as usize,
            "one group of `speed` per ticking section and nothing for the rest"
        );

        for (k, (x, y, z)) in picks.iter().copied().enumerate() {
            let group = k / speed as usize;
            let section_index = ticking[group];
            let base = (c.min_section_y + section_index as i32) << 4;
            assert!(
                (base..=base + 15).contains(&y),
                "pick {k} is in group {group} (section index {section_index}, \
                 block y {base}..={}) but landed at y {y}",
                base + 15
            );
            assert!(
                (c.min_block_x..=c.min_block_x + 15).contains(&x),
                "pick {k} x {x} outside the chunk column"
            );
            assert!(
                (c.min_block_z..=c.min_block_z + 15).contains(&z),
                "pick {k} z {z} outside the chunk column"
            );
        }
    }

    /// The mask is an argument and only `y` reads it. `tickChunk` always
    /// passes 15, so a port that hard-codes 15 agrees with vanilla
    /// everywhere the loop calls it — this pins the parameter itself.
    #[test]
    fn the_mask_applies_to_y_alone() {
        let mut a = BlockRandomPos::at(999);
        let mut b = BlockRandomPos::at(999);
        let (xa, ya, za) = a.next(0, 0, 0, 15);
        let (xb, yb, zb) = b.next(0, 0, 0, 0);
        assert_eq!(xa, xb, "x does not read the mask");
        assert_eq!(za, zb, "z does not read the mask");
        assert_eq!(yb, 0, "mask 0 flattens y and nothing else");
        assert!(ya <= 15);
    }

    /// **`ishr` versus `iushr` is not observable through this function, and
    /// this test exists to stop anyone claiming otherwise a second time.**
    ///
    /// The first version of this file asserted the opposite in three places —
    /// that a `>>>` port "agrees with vanilla on every non-negative value and
    /// on none of the negatives", and that the golden's negative seeds were
    /// there to catch it. Measured on 2026-08-06 and **false**: across 20,008
    /// seeds the positions are identical under both shifts, without
    /// exception.
    ///
    /// The reason is arithmetic and total, not statistical. Shifting the same
    /// 32-bit pattern right by 2 arithmetically and logically gives values
    /// that differ **only in bits 30 and 31** (`xor == 0xC000_0000`), and
    /// `getBlockRandomPos` reads bits 0-3, 8-11 and 16-19. It never looks
    /// above bit 19, so the two top bits it disagrees about are discarded
    /// every time. `r` is a local, so nothing carries them forward either.
    ///
    /// `>>` is still what is written, because `ishr` is what the bytecode
    /// says and a port should not silently disagree with its source. But it
    /// is **untestable through the public surface**, so it is written down
    /// here instead of being defended by a check that cannot fail.
    #[test]
    fn the_shift_kind_cannot_be_observed_through_the_masked_fields() {
        let place = |r: i32| (r & 15, (r >> 16) & 15, (r >> 8) & 15);
        let mut disagreements = 0;
        // Every seed the golden uses, plus the whole low-order neighbourhood
        // of the sign boundary, which is where a difference would have to
        // live if there were one.
        let seeds = [0, 1, -1, ADDEND, -ADDEND, i32::MIN, i32::MAX, 12345];
        for seed in seeds.into_iter().chain(-4096..4096) {
            let rv = seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND);
            if place(rv >> 2) != place(((rv as u32) >> 2) as i32) {
                disagreements += 1;
            }
        }
        assert_eq!(
            disagreements, 0,
            "if this ever becomes non-zero the claim above is wrong and the \
             golden should grow a case for it"
        );

        // And the fields really are the low ones — otherwise the paragraph
        // above is an explanation of a coincidence.
        let differing = (i32::MIN >> 2) ^ ((i32::MIN as u32) >> 2) as i32;
        assert_eq!(differing as u32, 0xC000_0000, "only the top two bits");
    }

    use crate::edits::Blocks;

    /// The least `Blocks` that compiles: every read is air.
    struct Air;
    impl Blocks for Air {
        fn state_at(&self, _: crate::interact::BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn set_block(&self, _: crate::interact::BlockPos, _: &str) -> Option<i32> {
            None
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn face_sturdy(&self, _: crate::interact::BlockPos, _: crate::interact::Face) -> bool {
            false
        }
        fn redstone_conductor(&self, _: crate::interact::BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: crate::interact::BlockPos) -> bool {
            false
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(0)
        }
        fn replaceable(
            &self,
            _: crate::interact::BlockPos,
            _: &crate::placement::Replacing<'_>,
        ) -> bool {
            true
        }
        fn legacy_fluid_at(&self, _: crate::interact::BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn water_at(&self, _: crate::interact::BlockPos) -> bool {
            false
        }
    }

    /// The end-to-end path on a real superflat world: real tables, the real
    /// `World`, the real section census and the real dispatch. Covered grass
    /// must die within a handful of ticks at 4096 — the 780 shape — and a
    /// pass must actually land picks on randomly-ticking states.
    #[test]
    fn a_real_flat_world_kills_covered_grass_and_grows_wheat() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let props = root.join("data/vanilla/block_props.json");
        if !rustmc_data::fixture::available(
            &props,
            "vanilla block_props extract",
            "ci/block-props-golden.sh",
        ) {
            return;
        }
        let ids = crate::terrain::StateIds::load(&props).expect("ids");
        let registries = crate::configuration::CapturedRegistries::load(
            &root.join("data/vanilla/registries.bin"),
        )
        .expect("registries");
        let biomes = registries
            .entry_ids("minecraft:worldgen/biome")
            .expect("biomes")
            .expect("biome registry");
        let tables =
            std::sync::Arc::new(crate::world::WorldTables::load(&biomes, ids).expect("tables"));
        let preset = crate::flat::FlatPreset::load(&root.join("data/vanilla/flat_preset.json"))
            .expect("preset");
        let area = rustmc_worldgen::pipeline::Area {
            min_chunk_x: -2,
            min_chunk_z: -2,
            max_chunk_x: 2,
            max_chunk_z: 2,
        };
        let world = crate::world::World::generate_flat(&preset, area, tables).expect("world");
        let classes =
            crate::placedby::PlacedByTable::shared(&root.join("data/vanilla/set_placed_by.json"))
                .expect("classes");
        let grass_y = preset.surface_y() - 1;
        assert!(world
            .state_at((1, grass_y, 1))
            .starts_with("minecraft:grass_block"));
        assert!(world
            .set_block((1, grass_y + 1, 1), "minecraft:stone")
            .is_some());
        assert!(world
            .set_block((3, grass_y, 3), "minecraft:farmland[moisture=0]")
            .is_some());
        assert!(world
            .set_block((3, grass_y + 1, 3), "minecraft:wheat[age=0]")
            .is_some());
        let sections = world
            .random_ticking_sections(0, 0)
            .expect("chunk 0,0 is inner");
        assert!(
            sections.1.iter().any(|t| *t),
            "the grass layer's section ticks: {sections:?}"
        );
        let conn = std::sync::Arc::new(crate::players::Connection::new(
            0,
            "a",
            Box::new(Vec::new()),
            None,
        ));
        let players = [crate::tracker::TrackedPlayer {
            conn,
            entity_id: 1,
            pos: (0.0, 0.0, 0.0),
            chunk: (0, 0),
            view_distance: 4,
        }];
        let started = std::time::Instant::now();
        let mut ticks = 0;
        while ticks < 400 && world.state_at((3, grass_y + 1, 3)) != "minecraft:wheat[age=7]" {
            let _ = tick(&world, None, Some(classes), &players, 4096, 6000);
            ticks += 1;
        }
        eprintln!(
            "flat world: {ticks} ticks at speed 4096 over 25 chunks in {:?}",
            started.elapsed()
        );
        assert_eq!(world.state_at((1, grass_y, 1)), "minecraft:dirt");
        assert_eq!(
            world.state_at((3, grass_y + 1, 3)),
            "minecraft:wheat[age=7]"
        );
    }

    /// `speed <= 0` draws nothing and changes nothing — `tick`'s own guard,
    /// and the reason every scenario but 780 pays nothing for this module
    /// being wired in.
    #[test]
    fn speed_zero_changes_nothing() {
        let w = Air;
        let conn = std::sync::Arc::new(crate::players::Connection::new(
            0,
            "a",
            Box::new(Vec::new()),
            None,
        ));
        let players = [crate::tracker::TrackedPlayer {
            conn,
            entity_id: 1,
            pos: (0.0, 0.0, 0.0),
            chunk: (0, 0),
            view_distance: 4,
        }];
        let pass = tick(&w, None, None, &players, 0, 0);
        assert!(pass.effects.blocks.is_empty() && pass.unported.is_empty());
    }
    /// The size of the deviation, as a number rather than as prose.
    ///
    /// A dead constant behind a doc comment is this project's own named
    /// defect class, so `faithful_chunk_radius` is exercised here: it is the
    /// quantity that would replace `CHUNK_RADIUS`, and this test is what
    /// makes a change to either of them visible. If they ever agree, delete
    /// both this test and the deviation paragraph.
    #[test]
    fn the_random_tick_radius_is_two_where_vanillas_is_the_simulation_distance() {
        assert_eq!(super::CHUNK_RADIUS, 2);
        assert_eq!(super::faithful_chunk_radius(10), 10, "the default");
        assert_eq!(super::faithful_chunk_radius(4), 4, "ci/t2-gameplay.sh's");
        let ours = (2 * super::CHUNK_RADIUS + 1).pow(2);
        let theirs = (2 * super::faithful_chunk_radius(10) + 1).pow(2);
        assert_eq!((ours, theirs), (25, 441));
        assert!(
            super::CHUNK_RADIUS < super::faithful_chunk_radius(4),
            "the deviation is a narrowing in both configurations, never a widening"
        );
    }
}
