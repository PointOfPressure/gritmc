//! A world that grows: square tiles, generated as a player walks into them.
//!
//! # The problem, and why a bigger square is not the answer
//!
//! [`crate::world::World`] generates one square before the socket opens and
//! serves out of it. Past its edge there is nothing, and a player who walks
//! that far falls out of the world. Making the square bigger only moves the
//! edge — and pays for the whole of it up front, on the clock of the first
//! player to join.
//!
//! The reason it cannot simply be generated on demand, one chunk at a time,
//! is the one `pipeline` states: carvers read a 17x17 neighbourhood and
//! decoration writes one chunk outwards, so a chunk is only finished once
//! every chunk that can reach it has been. `DECORATION_MARGIN` is 2 and it
//! was measured, not chosen. Answering a chunk request immediately would hand
//! out a chunk whose trees had not been planted, and the world would depend
//! on the order players walked through it.
//!
//! # What this does instead, and the measurement that allowed it
//!
//! Tiles. Each tile is its own full generation of a square — margin and all —
//! and a chunk is served by whichever tile owns it. Tiles never overlap, so
//! every chunk has exactly one owner.
//!
//! That is only valid if the seam is invisible: the chunks either side of a
//! tile boundary must hold exactly what one generation covering both would
//! have produced. There is an argument that they must — the margin means a
//! delivered chunk had every reaching chunk decorated, and decoration is
//! position-seeded — but this project does not ship arguments.
//! `crates/rustmc-server/tests/tiled_world.rs` generates two adjacent 1x1
//! tiles and one pass over both and compares every block of both chunks:
//! **0 differences over 196,608 blocks**, measured 2026-07-28.
//!
//! # And it is only true at that seed, which is the finding
//!
//! That test runs at seed 20260722. Widening the context at **seed 0** — the
//! seed `rustmc-server run` actually uses — does not settle:
//! `tests/margin_probe.rs` regenerates chunk (0,0) inside areas of 2 to 7
//! rings and the column at (8,8) reads 74, 74, 62, 74, 62, 62, with 24 to 334
//! blocks moving between one context and the next. It **oscillates** rather
//! than converging, which is not the signature of a margin that is merely too
//! small.
//!
//! It was found from a client: `ci/dig-probe.py` spawned at y 75 with a tile
//! radius of 1 and y 63 with a radius of 2, same world, same column.
//!
//! So the claim this module is allowed to make is narrower than "tiling is
//! seamless": **a tile is exactly as finished as the single fixed square was,
//! and no more.** That square was also one area chosen at one radius, so
//! tiling does not make anything worse — it makes an existing
//! `DECORATION_MARGIN` problem visible in more places, and it is now measured
//! rather than assumed. Fixing it is worldgen's, not this module's: whatever
//! reaches a chunk from more than two rings away has to be found first.
//!
//! # 2026-08-12: "more than two rings away" has no answer, and that is settled
//!
//! The paragraph above ends by naming the thing to find. There is nothing to
//! find, because the depth at which contamination stops is not a constant.
//! `docs/DETERMINISM.md` §31 takes the ladder out to a 14-ring context: at
//! seed 12345 the delivered square is **still moving** at every one of twelve
//! widths, and injecting margins of 1, 2 and 4 into the strengthened
//! `tiled_world.rs` gate gives 5,753, 2,201 and 1,130 blocks — decreasing,
//! never zero.
//!
//! The reason is this module's own arithmetic. A tile of radius `r` decorates
//! out to `r + m` and delivers out to `r`, so **its rim sits at depth `m`
//! below the outermost decorated ring whatever `m` is.** The outermost ring
//! decorates against a world that stops at its own edge and writes that
//! inward. Raising `m` moves the rim outward with it and buys nothing at the
//! rim, which is where the tile boundary is. Two contaminated rings measured
//! at one window is a property of that window, not a bound (§31.7).
//!
//! Nothing above is retracted — seed 20260722 really is stable, re-checked to
//! a 10-ring context — and the cost note below is unchanged.
//!
//! # 2026-08-12, later the same day: the named fix was run, and it is priced
//!
//! The paragraph that stood here said the shape of the fix was generate-once
//! status-based generation over one canvas. It was built and measured
//! (`docs/DETERMINISM.md` §32) and the result is not the one it expected.
//!
//! * The **extent** half comes straight out of the jar and ships:
//!   `ChunkStep`'s `FEATURES` step wants `CARVERS` at radius 1, so decoration
//!   now stops one ring inside the terrain instead of running to the
//!   outermost generated ring. It moves 50 blocks on the seed-20260722 r=7
//!   window at (64,64) and leaves the End city window at 100.000000%.
//! * The **read** half — a chunk decorating against a world no other chunk
//!   has written into — does take the widening gate to **0 on all four
//!   cases**, and it is **measurably further from vanilla**: 8,712 to 42,873
//!   mismatches against `oracle-work/van-s1-c62` on that window, and the End
//!   city window's 100.000000% down to 99.998861%, 168 blocks of
//!   `chorus_plant`. It ships behind `RUSTMC_DECORATE_ISOLATED=1`.
//!
//! So **"a tile is exactly the slice of one pass it claims to be" is not a
//! property vanilla has**, and this module cannot acquire it without leaving
//! vanilla. Two unmodified vanilla runs of one seed differ by 32,127 blocks
//! in one chunk order and 146,169 across orders; a generator whose delivered
//! chunks are extent-independent cannot let a feature read a neighbour's
//! features, and vanilla's features read them.
//!
//! What replaces it is a **weaker and reachable** property, and it is the one
//! a player can actually feel: not "a tile equals one pass" but "a chunk,
//! once delivered, is never written again". §32.6 stated it.
//!
//! # 2026-08-12, later still: that property is measured, and it holds
//!
//! `docs/DETERMINISM.md` §33. [`crate::growing::GrowingWorld`] holds **one**
//! canvas for the world, a `ChunkStatus` per chunk raised at most once, and
//! hands a chunk out only once its own 3x3 has decorated — after which
//! `FEATURES`'s `blockStateWriteRadius(1)` means nothing left in the pyramid
//! can reach it. Over four seed/centre ladders taken from radius 0 to 1 to 2,
//! growing the canvas moves **0 blocks** in every already-delivered chunk on
//! all eight rungs, where regenerating the bigger square — what **this
//! module** does — moves **4,059** on the same eight rungs and the same
//! instrument. The same growth moves 50,378 blocks in the ring one chunk
//! further out, which is the set a world delivering one ring early would have
//! handed out, so the zero is the delivery rule rather than a gate that
//! stopped looking.
//!
//! It also carves each chunk once: 81 chunks to deliver radius 2, against the
//! 155 the regenerate arm carves. That is the "69% thrown away" note further
//! down, answered.
//!
//! **This module is not rewired onto it yet, and that is deliberate.** The
//! canvas property is closed; replacing the tile map changes retirement, the
//! memory bound `docs/BENCH.md` measures, the per-tile overlay and
//! [`MAX_CONCURRENT_GENERATIONS`], none of which §33 measured. §33.6.
//!
//! # What it costs, which is the seam in the design rather than in the world
//!
//! Each tile regenerates its own margin, so a tile of side *n* generates
//! `(n + 4)^2` chunks to deliver `n^2`. At the default side of 5 that is 81
//! generated for 25 delivered — **69% thrown away**, and the discard is
//! paid again by the tile next door for the chunks they share. A generator
//! that tracked a per-chunk *status* and promoted a chunk to decorated once
//! its neighbours had reached the previous stage would generate each chunk
//! once. That is the real fix and it is a larger piece: it changes
//! `pipeline`, which is also where the concurrency work for item 2 has to
//! go, so it is deliberately not started from here.
//!
//! **Built 2026-08-12 and measured — [`crate::growing::GrowingWorld`],
//! `docs/DETERMINISM.md` §33.4.** A growing world carves 81 chunks to deliver
//! a radius-2 square where regenerating each square in turn carves 155. What
//! is left is wiring *this* module onto it, which is the memory and retirement
//! half rather than the generator half.
//!
//! **This is the seam.** Tiling makes the world unbounded and correct;
//! status-based generation makes it cheap. They are independent, and this is
//! the one that a player can feel.
//!
//! # A tile is generated on a background thread, and the gap is visible
//!
//! Generation takes seconds — 6.4s for a 3x3 tile with its margin on the
//! Threadripper, and a client's read timeout is 20. So a tile the player is
//! walking into is generated on its own thread and the chunks arrive when
//! they are ready; until then the player sees the void that was always there,
//! for a few seconds rather than forever. The alternative — generating on the
//! connection thread — is the mistake that made the *first* version of this
//! server fail its join at exactly twenty seconds.
//!
//! Requests are deduplicated: two players walking into the same tile, or one
//! player whose view keeps asking, start one generation between them.
//!
//! # Tiles retire, and that is what bounds this server's memory
//!
//! Until 2026-08-05 they did not. A tile was `Box::leak`ed on the argument
//! that it lived for the process anyway, and the consequence was measured
//! rather than argued: **resident memory tracked the union of everywhere any
//! player had ever been and never fell.** One player at the shipped view
//! distance cost 2,957 MB, against vanilla's 1,097 MB for the same world, and
//! disconnecting returned none of it. `docs/BENCH.md` has the numbers.
//!
//! The leak comment was right about one thing and wrong about the other. It
//! was right that a reference has to be able to leave the lock — serving a
//! chunk encodes 24 sections and must not hold a reader while it does. It was
//! wrong that `'static` was the only way to get one: an [`Arc`] leaves the
//! lock just as freely, and it also **counts**. So a tile is dropped when the
//! map stops naming it *and* the last thread serving out of it has finished,
//! which is a property `Box::leak` cannot express at any price.
//!
//! What decides when the map stops naming a tile is [`Tiles::retain_around`]:
//! a tile is wanted while some connected player's view reaches into it, and
//! is retired once it has been unwanted continuously for
//! [`RETIRE_AFTER_SECS`]. The grace period is the hysteresis — a player
//! pacing across a tile boundary must not be able to make the server
//! regenerate the tile behind them on every step — and it is why no spatial
//! margin is needed on top.
//!
//! # Retiring a tile is not losing it, and the overlay is why
//!
//! A tile's canvas is a pure function of the seed and the square, so a
//! regenerated tile is the same tile. What is *not* reproducible is what a
//! player built on it. That lives in [`crate::edits::BlockEdits`], which is
//! held here behind an `Arc` — one per tile, for the life of the process,
//! costing one entry per changed block — and handed back to the tile when it
//! is regenerated ([`World::adopt_edits`]). Retirement therefore returns the
//! canvas, which is all of the memory, and keeps the build, which is none of
//! it.
//!
//! # Freeing is not returning, on glibc
//!
//! Dropping a tile makes its pages free to the *allocator*; whether the
//! process gives them back to the kernel is a separate question, and `VmRSS`
//! — which is what an operator's 1 GB limit is enforced against — answers the
//! second one. Generation runs on its own threads, each of which glibc gives
//! its own arena, and a freed arena is not trimmed on any schedule of its
//! own. So a retirement ends with an explicit [`return_freed_memory`], and
//! the measurement that this is not superstition is in `docs/BENCH.md`.

use crate::dimension::Dimension;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use rustmc_worldgen::pipeline::Area;

use crate::chunk::LevelChunkWithLight;
use crate::edits::{BlockEdits, Blocks};
use crate::interact::BlockPos;
use crate::terrain::StateIds;
use crate::world::{World, WorldTables};

/// How long a tile stays resident after the last player stopped being able to
/// see it.
///
/// Hysteresis, and the number is set by what it costs to be wrong in each
/// direction. Too short and a player walking a boundary makes the server
/// regenerate the tile behind them every few steps — seconds of CPU for a
/// tile they are about to want again. Too long and the memory the retirement
/// exists to return is held anyway. Thirty seconds is far longer than the
/// walk across a tile at any radius this server ships and far shorter than
/// the session it has to bound.
pub const RETIRE_AFTER_SECS: u64 = 30;

/// How many tiles may be generated at the same time.
///
/// # This is a memory bound, and it was measured before it was chosen
///
/// A tile is generated on its own thread, and `request` used to spawn one per
/// tile with no ceiling. A player joining at the shipped view distance of 10
/// with the shipped tile radius of 2 needs a 21x21 chunk view, which spans
/// **twenty-five** 5x5 tiles — so twenty-five pipelines ran at once, each
/// holding a full working set of density caches, aquifer state and samplers.
///
/// The radius sweep in `docs/BENCH.md` measured what that costs on the noise
/// world and found a step function rather than a curve: 1,991 MB at radius 2,
/// flat around 1,200 MB from radius 3 to 6, and then **201 MB at radius 10**
/// — the first radius at which one tile covers a view-distance-10 view and no
/// second generation is ever started. The memory was never really about the
/// radius. It was about how many generations were in flight, and the radius
/// only decided that number.
///
/// Two is chosen rather than one because the pipeline is already parallel
/// across cores — a second concurrent tile buys very little throughput and
/// multiplies the working set — while one lookahead keeps a walking player's
/// next tile started before they arrive. It bounds the peak at two working
/// sets whatever the radius and the view distance are, which is what lets
/// `status::WORLD_RADIUS` stay small enough to keep the boot-time result.
pub const MAX_CONCURRENT_GENERATIONS: usize = 2;

/// Ask the allocator to give free pages back to the kernel.
///
/// # Why this exists at all
///
/// `VmRSS` is the number `docs/PRODUCT.md`'s hardware floor is stated in and
/// the number a container's memory limit kills on. Dropping a tile returns
/// its pages to glibc's free lists, and glibc returns them to the kernel only
/// when it happens to notice — for the main arena, when the top of the heap
/// has enough contiguous free space to pass `M_TRIM_THRESHOLD`, and for a
/// thread arena, on no schedule that a caller can rely on. Tile generation
/// runs on spawned threads, so most of what a tile allocated is in a thread
/// arena, and "we dropped it" would otherwise not be visible as "the server
/// is using less memory".
///
/// `malloc_trim(0)` walks every arena and `madvise(MADV_DONTNEED)`s the free
/// pages in each. It is declared here rather than taken from a crate because
/// this workspace adds dependencies through the orchestrator and this is one
/// symbol with no preconditions.
///
/// On a non-glibc target this is a no-op and the tile is still dropped: the
/// correctness of retirement does not depend on it, only the visibility.
pub fn return_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        // SAFETY: `malloc_trim` takes an integer and returns one. It has no
        // preconditions, touches only the allocator's own bookkeeping, and is
        // safe to call from any thread at any time.
        unsafe {
            let _ = malloc_trim(0);
        }
    }
}

/// A growing world.
pub struct Tiles {
    /// Which dimension this grid is.
    ///
    /// Chooses the generator, the region subdirectory and the section count
    /// of every chunk packet served out of it. There is one `Tiles` per
    /// dimension and they share nothing but the block and biome tables.
    dimension: Dimension,
    seed: i64,
    /// Tile radius in chunks: a tile delivers `(2 * radius + 1)^2` chunks.
    radius: i32,
    /// The block and biome tables, loaded once and shared by every tile —
    /// see [`WorldTables`]. Held as the `Arc` that is handed to each `World`.
    tables: Arc<WorldTables>,
    /// Finished tiles, by tile index.
    ///
    /// `Arc`, not `&'static`. A clone leaves the lock exactly as a leaked
    /// reference did — which is the property the leak was there for, so that
    /// encoding 24 sections does not hold a reader — and unlike the leak it
    /// ends. A tile removed from this map by [`Tiles::retain_around`] is
    /// dropped as soon as the last thread serving out of it lets go, so a
    /// retirement can never race a chunk encode.
    ready: RwLock<BTreeMap<(i32, i32), Arc<World>>>,
    /// Tiles a thread is generating right now, so two askers start one
    /// generation.
    pending: Mutex<BTreeSet<(i32, i32)>>,
    /// Per-tile block overlays, kept across retirement.
    ///
    /// **The reason a retired tile is not a lost tile.** The canvas comes
    /// back from the seed; what a player built does not, so it lives here
    /// rather than only inside the `World`, and is handed to each generation
    /// of the tile through [`World::adopt_edits`]. One entry per changed
    /// block, so an untouched world costs nothing and a heavily built one
    /// costs what it built.
    overlays: Mutex<BTreeMap<(i32, i32), Arc<BlockEdits>>>,
    /// When each resident tile stopped being wanted by any player.
    ///
    /// Absent means "wanted now". Present means the clock is running towards
    /// [`RETIRE_AFTER_SECS`], and any player looking at it again removes the
    /// entry rather than merely pausing it.
    unwanted_since: Mutex<BTreeMap<(i32, i32), Instant>>,
    /// How many tiles have been retired, for the log line and for a test that
    /// wants to know retirement actually happened rather than that memory
    /// happened to be lower.
    retired: Mutex<u64>,
    /// Where this world saves, or `None` for "do not persist".
    ///
    /// See [`Tiles::region_dir`] for why this is a field rather than four
    /// reads of the `OnceLock`.
    region_dir: Option<std::path::PathBuf>,
}

impl Tiles {
    /// The one growing world this process serves.
    ///
    /// A singleton for the same reason [`World::shared`] is: two players have
    /// to be standing in the same world, and a per-connection copy is
    /// identical right up until somebody changes a block.
    ///
    /// # Errors
    /// If the extract needed to generate the first tile is missing.
    pub fn shared(seed: i64, radius: i32) -> io::Result<&'static Self> {
        Self::shared_in(Dimension::Overworld, seed, radius)
    }

    /// The one growing world this process serves **for one dimension**.
    ///
    /// One `OnceLock` per dimension, not one keyed map: a map behind a lock
    /// cannot hand out `&'static Self`, and `&'static` is what
    /// [`Tiles::request`] and [`Tiles::chunk`] need in order to spawn a
    /// generation thread that outlives the caller.
    ///
    /// The seed is the **same** in all three. `WorldOptions` holds one seed
    /// and every `LevelStem`'s generator gets it unchanged; nothing in
    /// vanilla derives a per-dimension seed. What makes the three worlds
    /// different is the noise router and the noise settings.
    ///
    /// # Errors
    /// If the extract needed to generate the first tile is missing.
    pub fn shared_in(dimension: Dimension, seed: i64, radius: i32) -> io::Result<&'static Self> {
        fn slot(d: Dimension) -> &'static std::sync::OnceLock<Result<Tiles, String>> {
            static OVERWORLD: std::sync::OnceLock<Result<Tiles, String>> =
                std::sync::OnceLock::new();
            static NETHER: std::sync::OnceLock<Result<Tiles, String>> = std::sync::OnceLock::new();
            static END: std::sync::OnceLock<Result<Tiles, String>> = std::sync::OnceLock::new();
            match d {
                Dimension::Overworld => &OVERWORLD,
                Dimension::Nether => &NETHER,
                Dimension::TheEnd => &END,
            }
        }
        slot(dimension)
            .get_or_init(|| Self::build(dimension, seed, radius).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| io::Error::other(e.clone()))
    }

    /// Which dimension this grid serves.
    #[must_use]
    pub const fn dimension(&self) -> Dimension {
        self.dimension
    }

    fn build(dimension: Dimension, seed: i64, radius: i32) -> io::Result<Self> {
        let ids = StateIds::load(std::path::Path::new("data/vanilla/block_props.json"))?;
        let registries = crate::configuration::CapturedRegistries::load(std::path::Path::new(
            "data/vanilla/registries.bin",
        ))?;
        let biomes = registries
            .entry_ids("minecraft:worldgen/biome")?
            .ok_or_else(|| {
                io::Error::other("the registry capture has no minecraft:worldgen/biome")
            })?;
        // Loaded once, here, instead of once per tile inside `World`. The
        // biome registry is consumed rather than stored: numbering it is the
        // only thing a tile ever wanted it for.
        let tables = Arc::new(WorldTables::load(&biomes, ids)?);
        Ok(Self {
            dimension,
            seed,
            radius,
            tables,
            ready: RwLock::new(BTreeMap::new()),
            pending: Mutex::new(BTreeSet::new()),
            overlays: Mutex::new(BTreeMap::new()),
            unwanted_since: Mutex::new(BTreeMap::new()),
            retired: Mutex::new(0),
            region_dir: Self::region_dir_for(dimension),
        })
    }

    /// Where this server saves, once an operator has named a world directory.
    ///
    /// A `OnceLock` rather than a field on `Tiles` because the choice has to
    /// be made before the first tile is generated — [`Tiles::generate`] loads
    /// a tile's saved chunks as it builds it — and `Tiles` itself is created
    /// lazily by whoever asks for a chunk first.
    ///
    /// Unset means "do not persist", and that is the default. A test harness
    /// that generated a world would otherwise write a region directory into
    /// whatever it happened to be run from, and two harnesses run in the same
    /// place would load each other's worlds.
    fn world_dir() -> &'static std::sync::OnceLock<std::path::PathBuf> {
        static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        &DIR
    }

    /// Name the world directory to save into and load from.
    ///
    /// Call before [`Tiles::prepare`]. The first caller wins, like every other
    /// choice on this singleton.
    pub fn persist_to(dir: std::path::PathBuf) {
        let _ = Self::world_dir().set(dir);
    }

    /// The operator's world directory, if persistence is on.
    ///
    /// `crate::plugins` roots plugin storage under this, so that a server told
    /// to persist nothing also persists nothing on a plugin's behalf — and
    /// therefore does not publish `grit:storage.dir`. One operator-facing
    /// choice, `--world`, decides both.
    #[must_use]
    pub fn configured_world_dir() -> Option<std::path::PathBuf> {
        Self::world_dir().get().cloned()
    }

    /// The region directory for the overworld, if persistence is on.
    ///
    /// `world/dimensions/minecraft/overworld/region` — 26.x moved it there
    /// from the classic `world/region`, and the overworld is the odd one out
    /// in the naming: it saves under `overworld` while the other two dimensions
    /// save under `the_nether` and `the_end`. `ci/gen-vanilla-region.sh`
    /// records the same layout, and a real vanilla world on disk is what both
    /// were read from.
    #[must_use]
    pub fn configured_region_dir() -> Option<std::path::PathBuf> {
        Self::region_dir_for(Dimension::Overworld)
    }

    /// The region directory for any dimension, if persistence is on.
    ///
    /// `<world>/<Dimension::world_subdir>/region`. There is no `DIM-1` and no
    /// `DIM1` in 26.2 and the overworld is not the world root: the path is
    /// `DimensionType.getStorageFolder`, which is
    /// `key.identifier().resolveAgainst(world.resolve("dimensions"))` with no
    /// special case for any of the three.
    #[must_use]
    pub fn region_dir_for(dimension: Dimension) -> Option<std::path::PathBuf> {
        Self::world_dir()
            .get()
            .map(|d| d.join(dimension.world_subdir()).join("region"))
    }

    /// Where **this** `Tiles` saves.
    ///
    /// Read off the `OnceLock` once, in [`Tiles::build`], and then held —
    /// rather than re-read at each of the four call sites. Identical in
    /// production, because `persist_to` is documented as happening before
    /// `prepare` and `main` does exactly that. What it buys is that the
    /// **retirement path is now testable**: block-entity eviction is allowed
    /// only where the tile was actually written to disk first, and a policy
    /// that can only be exercised by setting a process-wide static can only
    /// be tested in a test binary of its own.
    #[must_use]
    pub fn region_dir(&self) -> Option<&std::path::Path> {
        self.region_dir.as_deref()
    }

    /// Write every resident tile's edited chunks to disk. Returns how many
    /// chunks were written across all of them.
    ///
    /// # Why every tile and not just the changed ones
    ///
    /// A tile with no edits saves nothing — [`World::save_to`] walks the
    /// overlay — so "every tile" costs a lock and a lookup per tile and
    /// writes only what a player actually changed.
    ///
    /// # Errors
    /// If a region file cannot be written. A world directory that was never
    /// named is not an error: it means persistence is off, and this returns 0.
    pub fn save_all(&self) -> io::Result<usize> {
        let Some(region_dir) = self.region_dir() else {
            return Ok(0);
        };
        // Copied out of the lock before saving: a save writes files and must
        // not hold the map against every chunk request for the duration.
        // Cloning the `Arc`s is what makes that safe now — a tile retired
        // mid-save stays alive until this vector drops.
        let tiles: Vec<Arc<World>> = match self.ready.read() {
            Ok(ready) => ready.values().map(Arc::clone).collect(),
            Err(_) => return Err(io::Error::other("tile map lock poisoned")),
        };
        let mut written = 0;
        for world in tiles {
            written += world.save_to(region_dir)?;
        }
        Ok(written)
    }

    /// Generate the spawn tile, **before listening**.
    ///
    /// The first tile is the one a client waits on, so it is paid for on the
    /// server's clock rather than the player's — the same argument
    /// [`World::prepare`] makes, and the same twenty-second timeout it was
    /// learned from.
    ///
    /// # Errors
    /// If the extract is missing or generation fails.
    pub fn prepare(seed: i64, radius: i32) -> io::Result<&'static Self> {
        Self::prepare_in(Dimension::Overworld, seed, radius)
    }

    /// [`Tiles::prepare`] for one dimension.
    ///
    /// Only the overworld is prepared before the socket opens. The nether and
    /// the End are prepared the first time a player is sent to one — a portal
    /// is a several-second wait in vanilla too, and generating three spawn
    /// tiles at boot would triple "Preparing spawn area" for two dimensions
    /// most sessions never enter.
    ///
    /// # Errors
    /// If the extract is missing or generation fails.
    pub fn prepare_in(dimension: Dimension, seed: i64, radius: i32) -> io::Result<&'static Self> {
        let tiles = Self::shared_in(dimension, seed, radius)?;
        tiles.generate(tiles.tile_of(0, 0))?;
        Ok(tiles)
    }

    /// What a number taken from this world has to be reported alongside — the
    /// spawn tile's caveats, which are the generator's own for **this**
    /// dimension.
    ///
    /// Empty if the spawn tile is not generated yet, which is a different
    /// statement from "there are no caveats" and is why this returns a
    /// `Vec` rather than panicking: a caller printing it before `prepare` has
    /// run should get nothing, not the overworld's list.
    #[must_use]
    pub fn caveats(&self) -> Vec<&'static str> {
        self.tile(0, 0).map(|w| w.caveats()).unwrap_or_default()
    }

    /// How wide a tile is, in chunks.
    #[must_use]
    pub const fn side(&self) -> i32 {
        2 * self.radius + 1
    }

    /// Which tile owns a chunk.
    ///
    /// Euclidean division, not truncation: chunk −1 belongs to the tile west
    /// of the origin, and `/` would put it in the origin's and make every
    /// tile west or north of spawn overlap its neighbour by one row.
    #[must_use]
    pub const fn tile_of(&self, chunk_x: i32, chunk_z: i32) -> (i32, i32) {
        (
            (chunk_x + self.radius).div_euclid(self.side()),
            (chunk_z + self.radius).div_euclid(self.side()),
        )
    }

    /// The chunks a tile owns.
    #[must_use]
    pub const fn area_of(&self, (tx, tz): (i32, i32)) -> Area {
        Area::square(tx * self.side(), tz * self.side(), self.radius)
    }

    /// The spawn tile's area — what the join packet sends and what the
    /// client's view starts out holding.
    #[must_use]
    pub const fn spawn_area(&self) -> Area {
        self.area_of(self.tile_of(0, 0))
    }

    /// A finished tile, if it is finished.
    ///
    /// The `Arc` is cloned out of the read lock and the lock released, so a
    /// caller may hold the tile for as long as it likes — encoding a chunk,
    /// walking an overlay — without blocking either another reader or a
    /// retirement. A retirement that lands while this reference is held
    /// removes the tile from the map and the drop happens here, when the
    /// caller is done.
    #[must_use]
    pub fn tile(&self, chunk_x: i32, chunk_z: i32) -> Option<Arc<World>> {
        let key = self.tile_of(chunk_x, chunk_z);
        self.ready.read().ok()?.get(&key).map(Arc::clone)
    }

    /// The tile that owns this block position.
    fn tile_at(&self, (x, _, z): BlockPos) -> Option<Arc<World>> {
        self.tile(x >> 4, z >> 4)
    }

    /// Generate one tile now, on this thread, and publish it.
    ///
    /// # Errors
    /// Whatever the pipeline returned.
    pub fn generate(&self, key: (i32, i32)) -> io::Result<()> {
        if self
            .tile(key.0 * self.side(), key.1 * self.side())
            .is_some()
        {
            return Ok(());
        }
        // The one place the substrate is chosen. Everything downstream —
        // the overlay, the chunk encoder, the play loop — reads a `World`
        // and cannot tell which generator filled it, which is what makes a
        // gameplay check measured on superflat a check on *this* server.
        let world = match crate::flat::selected() {
            Some(preset) => {
                World::generate_flat(preset, self.area_of(key), Arc::clone(&self.tables))?
            }
            None => World::generate_in(
                self.dimension,
                self.seed,
                self.area_of(key),
                Arc::clone(&self.tables),
                rustmc_worldgen::pipeline::Through::Decoration,
            )?,
        };
        self.publish(key, world)
    }

    /// Attach the tile's overlay, restore anything saved, and make it live.
    ///
    /// Split out of [`Tiles::generate`] so that the part which is *not* about
    /// running the pipeline can be tested without running it. Everything in
    /// here is what makes a **regenerated** tile the same tile as the one
    /// that was retired, and none of it needs the vanilla extract — so
    /// keeping it in `generate` would have put the whole of "retiring is not
    /// losing" behind a T1 fixture.
    ///
    /// # Errors
    /// If a saved region file cannot be read.
    fn publish(&self, key: (i32, i32), mut world: World) -> io::Result<()> {
        // The overlay goes on **before** anything else touches the world, so
        // a tile that has been here before comes back carrying what a player
        // built on it. For a tile generated for the first time this is an
        // empty overlay and the line is free; for a regenerated one it is the
        // difference between retiring a tile and destroying it.
        world.adopt_edits(self.overlay_for(key));
        // Saved chunks go on **before** the tile is published, so a client
        // can never be sent the generated version of a chunk a player has
        // already built in. Publishing first and loading after would leave a
        // window in which the world briefly forgets the build — short, and
        // exactly long enough for a join to land in it.
        if let Some(region_dir) = self.region_dir() {
            let restored = world.load_from(region_dir)?;
            if restored > 0 {
                eprintln!(
                    "tile {key:?}: restored {restored} saved blocks from {}",
                    region_dir.display()
                );
            }
        }
        if let Ok(mut ready) = self.ready.write() {
            ready.insert(key, Arc::new(world));
        }
        // **Most of what generating a tile costs is not the tile.** The
        // pipeline builds and discards density-function caches, aquifer
        // state, noise samplers and a very large number of block-state
        // strings on the way to a canvas that is a fraction of the size. All
        // of that is freed by the time this line runs — and none of it has
        // gone back to the kernel, because this ran on a spawned thread and
        // glibc does not trim a thread arena on its own. `VmRSS` therefore
        // keeps the high-water mark of every generation the server has ever
        // run, which is a second reason resident memory only ever rose, and
        // it is independent of whether the tile itself is retired.
        return_freed_memory();
        // A tile that has just been generated is wanted by definition —
        // somebody asked for a chunk in it. Clearing the clock here rather
        // than waiting for the next sweep stops a tile generated during a
        // sweep's window from being retired before it has been served once.
        if let Ok(mut unwanted) = self.unwanted_since.lock() {
            unwanted.remove(&key);
        }
        Ok(())
    }

    /// The overlay for a tile, created empty the first time it is asked for.
    ///
    /// Keyed here and not on the `World` because it has to outlive the
    /// `World` — see the module docs.
    fn overlay_for(&self, key: (i32, i32)) -> Arc<BlockEdits> {
        self.overlays.lock().map_or_else(
            |_| Arc::new(BlockEdits::new()),
            |mut o| Arc::clone(o.entry(key).or_insert_with(|| Arc::new(BlockEdits::new()))),
        )
    }

    /// Start generating a tile if nothing is generating it already **and the
    /// server is not already generating as many as it allows at once**.
    ///
    /// Returns whether this call started one — which is what a caller logs,
    /// so "the world is loading" appears once per tile rather than once per
    /// chunk asked for.
    ///
    /// A request refused for being over the cap is not an error and is not
    /// remembered: the chunk stays unsent, `ChunkView::missing` stays true,
    /// and `keep_alive_loop` asks again. So the cap delays a tile rather than
    /// dropping it, and no caller needs to know it exists.
    pub fn request(&'static self, key: (i32, i32)) -> bool {
        {
            let Ok(ready) = self.ready.read() else {
                return false;
            };
            if ready.contains_key(&key) {
                return false;
            }
        }
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        if pending.len() >= MAX_CONCURRENT_GENERATIONS && !pending.contains(&key) {
            return false;
        }
        if !pending.insert(key) {
            return false;
        }
        drop(pending);
        std::thread::spawn(move || {
            if let Err(e) = self.generate(key) {
                eprintln!("tile {key:?} failed to generate: {e}");
            }
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&key);
            }
        });
        true
    }

    /// A chunk to send, or `None` — which means "not yet", and a generation
    /// has been started for it.
    ///
    /// `None` is deliberately the same answer for "outside everything" and
    /// "still generating", because to a client they are the same thing: a
    /// chunk it does not have. The difference shows up as one arriving a few
    /// seconds later.
    ///
    /// # Errors
    /// If encoding the chunk fails.
    pub fn chunk(
        &'static self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> io::Result<Option<LevelChunkWithLight>> {
        if let Some(world) = self.tile(chunk_x, chunk_z) {
            return world.chunk(chunk_x, chunk_z).map(Some);
        }
        let key = self.tile_of(chunk_x, chunk_z);
        if self.request(key) {
            eprintln!(
                "generating tile {key:?} — {} chunks",
                self.side() * self.side()
            );
        }
        Ok(None)
    }

    /// The ground height at a column of the spawn tile.
    ///
    /// # Panics
    /// If the spawn tile is not generated, which [`Self::prepare`] guarantees
    /// and any other order is a caller bug rather than a runtime condition.
    #[must_use]
    pub fn surface_at(&self, x: i32, z: i32) -> f64 {
        self.tile(x >> 4, z >> 4)
            .expect("the spawn tile is generated before listening")
            .surface_at(x, z)
    }

    /// Every resident tile's owed `light_update`s —
    /// `ChunkHolder.broadcastChanges`'s light half, over the whole world.
    ///
    /// One `ChunkHolder` per chunk in vanilla, so a union over tiles is the
    /// same set: a chunk belongs to exactly one tile and a tile's filters
    /// name only its own chunks. Order is the tile map's, which is
    /// `(x, z)`-ascending; vanilla's is `chunkHoldersToBroadcast`, a
    /// `HashSet`, so no order there is a fact to reproduce.
    ///
    /// Empty unless a block edit changed some section's light since the last
    /// call, which makes this cost one lock and a map walk on the ticks
    /// nobody built anything.
    #[must_use]
    pub fn light_updates(&self) -> Vec<crate::chunk::LightUpdate> {
        let Ok(ready) = self.ready.read() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for w in ready.values() {
            out.extend(w.light_updates());
        }
        out
    }

    /// How many tiles are finished — the loader's own proof that the world
    /// grew.
    #[must_use]
    pub fn ready_count(&self) -> usize {
        self.ready.read().map_or(0, |r| r.len())
    }

    /// Every chunk in every resident tile.
    ///
    /// For `crate::inhabited`, which is `ServerChunkCache.tickChunks`'
    /// habitation half and needs the loaded set to intersect against the
    /// players. A `Vec` rather than an iterator because the read lock must not
    /// be held across the caller's distance loop: that loop runs once per
    /// chunk per player per tick, and holding the tile map's lock through it
    /// would put the tick thread in the way of every `publish`.
    ///
    /// It is the tile's chunks and not its **grown** chunks: the margin is
    /// context the generator reads and not world this server is serving, and
    /// crediting habitation to it would credit chunks no player can be in.
    #[must_use]
    pub fn resident_chunks(&self) -> Vec<(i32, i32)> {
        let keys: Vec<(i32, i32)> = match self.ready.read() {
            Ok(ready) => ready.keys().copied().collect(),
            Err(_) => return Vec::new(),
        };
        keys.into_iter()
            .flat_map(|key| self.area_of(key).chunks())
            .collect()
    }

    /// How many tiles have been retired since the process started.
    ///
    /// A test asserting that memory fell needs to know it fell *for this
    /// reason*; a number that improves for an unrelated reason is the vacuous
    /// result `docs/AGENT-BRIEF.md` warns about.
    #[must_use]
    pub fn retired_count(&self) -> u64 {
        self.retired.lock().map_or(0, |r| *r)
    }

    /// Every tile a player standing at `chunk` with this view distance can
    /// see.
    ///
    /// The view is a square of `2 * view_distance + 1` chunks, so this is
    /// every tile the square touches — which is between one and four at any
    /// radius where a tile is wider than the view, and more when it is not.
    #[must_use]
    pub fn tiles_in_view(&self, (cx, cz): (i32, i32), view_distance: i32) -> BTreeSet<(i32, i32)> {
        let vd = view_distance.max(0);
        let (min, max) = (
            self.tile_of(cx - vd, cz - vd),
            self.tile_of(cx + vd, cz + vd),
        );
        let mut out = BTreeSet::new();
        for tx in min.0..=max.0 {
            for tz in min.1..=max.1 {
                out.insert((tx, tz));
            }
        }
        out
    }

    /// Retire every resident tile that no player has been able to see for
    /// [`RETIRE_AFTER_SECS`], and return how many were retired.
    ///
    /// # What "wanted" means, and what it deliberately does not
    ///
    /// `wanted` is the union of every connected player's view. It is **not**
    /// "every tile that has been generated", which is what the previous
    /// design effectively used and is why resident memory only ever rose. A
    /// player disconnecting removes their contribution from the union on the
    /// next sweep, which is what makes memory come back on a disconnect
    /// rather than at a restart.
    ///
    /// # Why the spawn tile is never retired
    ///
    /// Three reads on the `Blocks` impl — `default_state`, `has_state`,
    /// `state_id` — are questions about the block table rather than about the
    /// world, and answer them through tile (0, 0) because it is the one tile
    /// guaranteed to exist. [`Tiles::surface_at`] documents a panic on the
    /// same guarantee. Retiring it would turn all four into a race against
    /// whoever asked first, so it is pinned, and the cost of pinning it is
    /// one tile.
    pub fn retain_around(&self, wanted: &BTreeSet<(i32, i32)>) -> usize {
        let spawn = self.tile_of(0, 0);
        let now = Instant::now();
        let resident: Vec<(i32, i32)> = match self.ready.read() {
            Ok(ready) => ready.keys().copied().collect(),
            Err(_) => return 0,
        };
        let mut due = Vec::new();
        if let Ok(mut unwanted) = self.unwanted_since.lock() {
            // A tile no longer resident must not keep a clock entry, or a
            // tile regenerated later inherits a timer that started before it
            // existed and is retired the moment it appears.
            unwanted.retain(|key, _| resident.contains(key));
            for key in resident {
                if key == spawn || wanted.contains(&key) {
                    unwanted.remove(&key);
                    continue;
                }
                let since = unwanted.entry(key).or_insert(now);
                if now.duration_since(*since).as_secs() >= RETIRE_AFTER_SECS {
                    due.push(key);
                }
            }
            for key in &due {
                unwanted.remove(key);
            }
        }
        if due.is_empty() {
            return 0;
        }
        // Saved before the tile goes, when there is somewhere to save to. The
        // overlay is kept either way, so an operator running without
        // `--world` still keeps the build for the life of the process; the
        // save is what makes it survive the process.
        let region_dir = self.region_dir().map(std::path::Path::to_path_buf);
        let mut retired = 0;
        let mut evicted = 0;
        for key in due {
            let Ok(mut ready) = self.ready.write() else {
                break;
            };
            let Some(world) = ready.remove(&key) else {
                continue;
            };
            drop(ready);
            // **The block entities go only if they were written down, and
            // the order of these two statements is the whole item.** See
            // `BlockEntities::evict_chunks`: dropping a chest's contents
            // because nobody is looking at it is a data loss that every
            // existing test passes through, because every existing test
            // asks about the chest while the tile is still resident.
            let saved = match region_dir.as_deref() {
                Some(dir) => match world.save_to(dir) {
                    Ok(_) => true,
                    Err(e) => {
                        eprintln!("tile {key:?}: could not save before retiring: {e}");
                        false
                    }
                },
                None => false,
            };
            // **No generation of this tile can be in flight here**, which is
            // what stops the eviction deleting block entities a concurrent
            // `publish` has just loaded from the file this save wrote:
            // `Tiles::request` refuses a key that is in `ready`, and
            // `retain_around` only ever retires a key that is. The `remove`
            // above is what opens the door to a new generation, and it is
            // ordered before both the save and the eviction — so a generation
            // that starts from here loads what this save has written, not
            // what it is about to write.
            if saved {
                evicted += crate::blockentity::BlockEntities::shared()
                    .evict_chunks(self.area_of(key).chunks());
            }
            // The last reference this thread holds. Any chunk encode that
            // started before the `remove` above still holds its own, and the
            // tile outlives this drop until that one finishes.
            drop(world);
            retired += 1;
        }
        if retired > 0 {
            if let Ok(mut n) = self.retired.lock() {
                *n += retired as u64;
            }
            // Freed is not returned — see `return_freed_memory`. Called once
            // per sweep rather than once per tile: it walks every arena, and
            // the sweep is the natural batch.
            return_freed_memory();
            eprintln!(
                "retired {retired} tile(s) nothing had looked at for {RETIRE_AFTER_SECS}s; \
                 {} resident, {evicted} block entit{} released",
                self.ready_count(),
                if evicted == 1 { "y" } else { "ies" }
            );
        }
        retired
    }
}

/// Reads and writes route to the tile that owns the position.
///
/// A position in a tile that does not exist yet reads as air and refuses
/// every write. Refusing is the honest answer rather than a missing feature:
/// an edit there would be made against a world that has not been generated,
/// and the generation that followed would overwrite it — so the block a
/// player placed would vanish the moment the ground under it arrived.
impl Blocks for Tiles {
    fn state_at(&self, pos: BlockPos) -> String {
        self.tile_at(pos)
            .map_or_else(|| "minecraft:air".to_string(), |w| w.state_at(pos))
    }

    fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
        self.tile_at(pos)?.set_block(pos, state)
    }

    /// Delegated to the tile that owns the position, like every other read.
    /// A position in no tile has no block and therefore no sturdy face.
    fn face_sturdy(&self, pos: BlockPos, face: crate::interact::Face) -> bool {
        self.tile_at(pos).is_some_and(|w| w.face_sturdy(pos, face))
    }

    fn default_state(&self, block: &str) -> Option<String> {
        // Any finished tile answers this — it is a read of the block table,
        // not of the world — and the spawn tile always exists.
        self.tile(0, 0)?.default_state(block)
    }

    fn has_state(&self, state: &str) -> bool {
        self.tile(0, 0).is_some_and(|w| w.has_state(state))
    }

    /// Delegated like every other read. A position in no tile has no block,
    /// and air is neither a conductor nor a source — so `false` here is the
    /// same answer the owning tile would give, not a fallback.
    fn redstone_conductor(&self, pos: BlockPos) -> bool {
        self.tile_at(pos).is_some_and(|w| w.redstone_conductor(pos))
    }

    fn signal_source(&self, pos: BlockPos) -> bool {
        self.tile_at(pos).is_some_and(|w| w.signal_source(pos))
    }

    fn state_id(&self, state: &str) -> Option<i32> {
        // A read of the block table rather than of the world, so any
        // finished tile answers it and the spawn tile always exists.
        self.tile(0, 0)?.state_id(state)
    }

    fn replaceable(&self, pos: BlockPos, ctx: &crate::placement::Replacing<'_>) -> bool {
        self.tile_at(pos).is_some_and(|w| w.replaceable(pos, ctx))
    }

    fn water_at(&self, pos: BlockPos) -> bool {
        self.tile_at(pos).is_some_and(|w| w.water_at(pos))
    }

    /// A read of the block table rather than of the world, as `state_id` is.
    fn fluid_of_state(&self, state: &str) -> crate::fluid::FluidState {
        self.tile(0, 0)
            .map_or(crate::fluid::NOTHING, |w| w.fluid_of_state(state))
    }

    fn solid_of_state(&self, state: &str) -> bool {
        self.tile(0, 0).is_some_and(|w| w.solid_of_state(state))
    }

    fn fluid_at(&self, pos: BlockPos) -> crate::fluid::FluidState {
        self.tile_at(pos)
            .map_or(crate::fluid::NOTHING, |w| w.fluid_at(pos))
    }

    fn fast_lava(&self) -> bool {
        self.tile(0, 0).is_some_and(|w| w.fast_lava())
    }

    fn water_evaporates(&self) -> bool {
        self.tile(0, 0).is_some_and(|w| w.water_evaporates())
    }

    /// A position in no tile answers air, which is what it reads as. It also
    /// refuses every write, so nothing acts on the answer.
    fn legacy_fluid_at(&self, pos: BlockPos) -> String {
        self.tile_at(pos)
            .map_or_else(|| "minecraft:air".to_string(), |w| w.legacy_fluid_at(pos))
    }

    // ---- the four defaulted reads ------------------------------------------
    //
    // **Every method `Blocks` gives a default body has to be repeated here,
    // and forgetting one is silent.** The default is `None`, every caller
    // treats `None` as "the table cannot answer, so do nothing", and doing
    // nothing is exactly what a correct server looks like when the mechanism
    // simply has not fired. That is this repo's central defect class reached
    // from a new direction: not a check that cannot go red, but a *port* that
    // cannot run, behind a unit test suite that passes because its fakes
    // override the very method the real world drops.
    //
    // Both of the instances below shipped that way and neither was noticed
    // by T0:
    //
    //   * `level_min_y` — added with the gravity block. `ticks.rs` reads it
    //     as `if let Some(min_y) = blocks.level_min_y()`, so on the real
    //     server the whole `FallingBlock.tick` arm was skipped and sand did
    //     not fall, while `movement.rs`'s own fake answered `-64` and its
    //     tests passed.
    //   * `note_block_instrument` — added with the note block, same shape:
    //     the placement refinement leaves the state alone on `None`, so a
    //     note block would have gone on being placed with the golden
    //     harness's instrument.
    //
    // `ci/check-blocks-delegation.sh` refuses a fifth.

    /// A property of the level rather than of a position, so it is read off
    /// the spawn tile like `state_id` and `default_state` — `tile_at` would
    /// answer `None` for a position outside every tile and make the world's
    /// floor depend on where you asked from.
    fn level_min_y(&self) -> Option<i32> {
        self.tile(0, 0)?.level_min_y()
    }

    /// Positional: `NoteBlock.setInstrument` reads `pos.above()` and
    /// `pos.below()`, which share this position's x and z and so are always
    /// in the same tile.
    fn note_block_instrument(&self, pos: BlockPos) -> Option<String> {
        self.tile_at(pos)?.note_block_instrument(pos)
    }

    /// Not positional: every tile shares this server's one `WorldTables`, so
    /// the table is read here rather than through a tile, and it answers
    /// before any tile has finished. Same emptiness rule as
    /// [`World::place_props`](crate::edits::Blocks::place_props): an empty
    /// table is no table, and `None` is "no `canSurvive` gate".
    fn place_props(&self) -> Option<&rustmc_behavior::block_props::BlockPropsTable> {
        let props = self.tables.props();
        (!props.is_empty()).then_some(props)
    }

    /// Positional, by chunk: a chunk belongs to exactly one tile, so the
    /// owner answers for the whole column. A chunk in no tile has no
    /// sections, which is `None` — and `None` is "nothing here random
    /// ticks", the true answer for a chunk that has not been generated.
    fn random_ticking_sections(&self, chunk_x: i32, chunk_z: i32) -> Option<(i32, Vec<bool>)> {
        self.tile(chunk_x, chunk_z)?
            .random_ticking_sections(chunk_x, chunk_z)
    }

    /// Positional: one column is one chunk is one tile.
    fn column_top(&self, x: i32, z: i32) -> Option<i32> {
        self.tile(x >> 4, z >> 4)?.column_top(x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Tiles` with no world in it, for the arithmetic — which is the part
    /// that is wrong in a way no amount of generating would reveal.
    fn empty(radius: i32) -> Tiles {
        empty_saving_to(radius, None)
    }

    /// The same, with somewhere to save. **The region directory is a field
    /// rather than a `OnceLock` read exactly so this function can exist**:
    /// the retirement path's block-entity eviction is conditional on the tile
    /// having been written to disk, and a condition that can only be set by a
    /// process-wide static cannot be tested both ways in one binary.
    fn empty_saving_to(radius: i32, region_dir: Option<std::path::PathBuf>) -> Tiles {
        Tiles {
            dimension: Dimension::Overworld,
            region_dir,
            seed: 0,
            radius,
            tables: Arc::new(WorldTables::empty()),
            ready: RwLock::new(BTreeMap::new()),
            pending: Mutex::new(BTreeSet::new()),
            overlays: Mutex::new(BTreeMap::new()),
            unwanted_since: Mutex::new(BTreeMap::new()),
            retired: Mutex::new(0),
        }
    }

    /// Tiles tile: every chunk has exactly one owner, and the owner's area
    /// contains it.
    #[test]
    fn every_chunk_belongs_to_the_tile_that_claims_it() {
        for radius in [0, 1, 2, 5] {
            let tiles = empty(radius);
            for cx in -40..40 {
                for cz in -3..3 {
                    let key = tiles.tile_of(cx, cz);
                    assert!(
                        tiles.area_of(key).contains(cx, cz),
                        "radius {radius}: chunk ({cx},{cz}) claims tile {key:?}, \
                         whose area does not contain it"
                    );
                }
            }
        }
    }

    /// The negative side is the one a truncating division gets wrong, and it
    /// gets it wrong by *overlapping* — two tiles both claiming chunk −1,
    /// which would generate the same chunk twice and serve whichever won.
    #[test]
    fn tiles_do_not_overlap_across_the_origin() {
        let tiles = empty(2);
        // A range that is a whole number of tiles, derived from the tiles
        // rather than picked: a round number of chunks cuts the tiles at
        // each end in half and the assertion below fails on the scan instead
        // of on the arithmetic.
        let (first, last) = (-4, 3);
        let mut seen = std::collections::BTreeMap::new();
        for cx in tiles.area_of((first, 0)).min_chunk_x..=tiles.area_of((last, 0)).max_chunk_x {
            let key = tiles.tile_of(cx, 0);
            seen.entry(key).or_insert_with(Vec::new).push(cx);
        }
        assert_eq!(
            seen.len(),
            (last - first + 1) as usize,
            "every tile seen once"
        );
        for (key, chunks) in seen {
            let area = tiles.area_of(key);
            assert_eq!(
                chunks.len(),
                tiles.side() as usize,
                "tile {key:?} claims {} chunks, a tile is {} wide",
                chunks.len(),
                tiles.side()
            );
            assert_eq!(
                (
                    *chunks.first().expect("nonempty"),
                    *chunks.last().expect("nonempty")
                ),
                (area.min_chunk_x, area.max_chunk_x),
                "tile {key:?} claims a different run of chunks from the one it covers"
            );
        }
    }

    /// The spawn tile is the one containing the origin, whatever the radius.
    #[test]
    fn spawn_is_inside_the_spawn_tile() {
        for radius in [0, 1, 2, 5] {
            let area = empty(radius).spawn_area();
            assert!(
                area.contains(0, 0),
                "radius {radius}: spawn tile misses (0,0)"
            );
        }
    }

    /// A tile with a canvas in it, without the vanilla extract.
    ///
    /// The retirement policy is arithmetic over a map of keys and a clock; it
    /// is testable without generating anything, and a test that needed the
    /// extract would not run in T0 — which is where a policy this cheap to
    /// get wrong should be gated.
    fn publish(tiles: &Tiles, key: (i32, i32), unwanted_for: u64) {
        // A preset with no layers: every column is air, so nothing is looked
        // up in the block table and `WorldTables::empty()` is enough. That is
        // what lets this run in T0 rather than behind the vanilla extract.
        let preset = crate::flat::FlatPreset {
            biome: "minecraft:the_void".to_string(),
            features: false,
            lakes: false,
            layers: Vec::new(),
        };
        let world = World::generate_flat(&preset, tiles.area_of(key), Arc::clone(&tiles.tables))
            .expect("a preset with no layers needs no block table");
        // Through `publish`, not straight into the map: attaching the overlay
        // is the half of a regeneration that a test of retirement has to
        // exercise, and putting the world in by hand would skip it.
        tiles.publish(key, world).expect("nothing to restore");
        if unwanted_for > 0 {
            tiles.unwanted_since.lock().expect("fresh lock").insert(
                key,
                Instant::now() - std::time::Duration::from_secs(unwanted_for),
            );
        }
    }

    /// The whole point of the change: a tile nobody can see goes away, and
    /// takes its canvas with it.
    #[test]
    fn a_tile_no_player_can_see_is_retired_once_the_grace_period_is_up() {
        let tiles = empty(2);
        publish(&tiles, (5, 5), RETIRE_AFTER_SECS + 1);
        assert_eq!(tiles.ready_count(), 1);
        // Nobody is connected, so nothing is wanted — which is exactly the
        // state the server is in one second after the last player leaves.
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1);
        assert_eq!(tiles.ready_count(), 0, "the tile is gone");
        assert_eq!(tiles.retired_count(), 1, "and it went by being retired");
    }

    /// The grace period is not decoration: a tile that has only just stopped
    /// being watched stays, or a player pacing a boundary regenerates the
    /// world behind them on every step.
    #[test]
    fn a_tile_unwanted_for_less_than_the_grace_period_stays() {
        let tiles = empty(2);
        publish(&tiles, (5, 5), 0);
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 0);
        assert_eq!(tiles.ready_count(), 1);
        // And the second sweep does not either — the clock started on the
        // first one, it did not expire on it.
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 0);
        assert_eq!(tiles.ready_count(), 1);
    }

    /// A tile somebody is looking at is not retired however long it has been
    /// resident, and looking at it again resets the clock rather than pausing
    /// it.
    #[test]
    fn a_watched_tile_is_kept_and_looking_again_resets_the_clock() {
        let tiles = empty(2);
        publish(&tiles, (5, 5), RETIRE_AFTER_SECS + 1);
        let wanted: BTreeSet<(i32, i32)> = [(5, 5)].into_iter().collect();
        assert_eq!(tiles.retain_around(&wanted), 0, "wanted tiles are kept");
        assert_eq!(tiles.ready_count(), 1);
        // The clock was cleared, not paused: an immediately following sweep
        // with nobody watching must start counting from zero.
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 0);
        assert_eq!(tiles.ready_count(), 1);
    }

    /// The spawn tile answers three block-table questions and `surface_at`
    /// promises a panic if it is missing. It is pinned, and this is the test
    /// that says so.
    #[test]
    fn the_spawn_tile_is_never_retired() {
        let tiles = empty(2);
        let spawn = tiles.tile_of(0, 0);
        publish(&tiles, spawn, RETIRE_AFTER_SECS * 100);
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 0);
        assert_eq!(
            tiles.ready_count(),
            1,
            "the spawn tile survives an empty world"
        );
    }

    /// Retiring a tile must not lose what a player built on it. The overlay
    /// outlives the canvas and is handed back to the next generation of the
    /// same tile.
    #[test]
    fn a_retired_tile_keeps_the_overlay_it_had() {
        let tiles = empty(2);
        let key = (5, 5);
        publish(&tiles, key, RETIRE_AFTER_SECS + 1);
        // Write through the overlay the tile is holding, exactly as a player
        // placing a block does.
        let overlay = tiles.overlay_for(key);
        overlay.set((85, 64, 85), "minecraft:diamond_block");
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1);
        // The same handle comes back for the same tile, still carrying it.
        let after = tiles.overlay_for(key);
        assert_eq!(
            after.get((85, 64, 85)).as_deref(),
            Some("minecraft:diamond_block"),
            "a retired tile forgot what a player built on it"
        );
    }

    /// **The claim the whole change rests on: retiring a tile is not losing
    /// it.** Read back through the public `Blocks` impl, which is the path a
    /// client's chunk actually takes, rather than through the overlay handle
    /// — a test that checked only the handle would pass even if the
    /// regenerated tile never adopted it.
    #[test]
    fn a_block_built_before_a_retirement_is_served_again_afterwards() {
        let tiles = empty(2);
        let key = (5, 5);
        // Tile (5,5) at radius 2 owns chunks 23..=27, so the position has to
        // be in one of those and not in chunk 5. The assertion below is what
        // caught that the first time.
        let pos = (400, 64, 400);
        publish(&tiles, key, RETIRE_AFTER_SECS + 1);
        assert_eq!(
            tiles.tile_of(pos.0 >> 4, pos.2 >> 4),
            key,
            "the position has to be inside the tile under test"
        );
        // Written through the tile's own overlay, which is what
        // `World::set_block` does once a real block table can validate the
        // state. `WorldTables::empty()` cannot, so the write goes in at the
        // layer below and the read still comes out through `Blocks`.
        tiles.overlay_for(key).set(pos, "minecraft:diamond_block");
        assert_eq!(
            Blocks::state_at(&tiles, pos),
            "minecraft:diamond_block",
            "the tile did not serve a block that was just written to it"
        );

        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1, "the tile retired");
        assert_eq!(
            Blocks::state_at(&tiles, pos),
            "minecraft:air",
            "a retired tile still answered — nothing was actually released"
        );

        // Walk back into it: the tile is generated again over the same area,
        // and the build has to come back with it.
        publish(&tiles, key, 0);
        assert_eq!(
            Blocks::state_at(&tiles, pos),
            "minecraft:diamond_block",
            "the regenerated tile lost what a player had built on it"
        );
    }

    /// A scratch directory that goes away with the test.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-evict-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// Where the chest goes: inside tile (5,5) at radius 2, which owns chunks
    /// 23..=27 and therefore blocks 368..=447 on each axis. Far from every
    /// other position in this binary, because
    /// `BlockEntities::shared()` is one static shared by every test in it.
    const CHEST: BlockPos = (400, 64, 400);
    const SLOT: usize = 3;
    const COUNT: i32 = 7;

    /// **The test that had to exist before the eviction did.**
    ///
    /// Evicting a retired tile's block entities is a memory fix and a data
    /// loss one line apart. Every other test of the store asks about a chest
    /// while its tile is resident, so an eviction that dropped the contents
    /// instead of writing them down first would be green in all of them and
    /// would empty a player's chest the moment they walked away.
    ///
    /// The three assertions are three different failures:
    ///
    /// * **the chest is not in the store after the retirement** — red if
    ///   nothing was evicted, which is the leak this item is about;
    /// * **the item is back after the tile is regenerated** — red if the
    ///   eviction ran without the save, which is the loss;
    /// * **the count is 7 and the slot is 3** — red if the round trip went
    ///   through a path that defaulted either, which is `ItemStackWithSlot`'s
    ///   own `orElse(1)` and the reason `block_entity_round_trip.rs` picks
    ///   the same two odd numbers.
    #[test]
    fn a_chest_survives_the_retirement_that_evicted_it() {
        let dir = scratch("survives");
        let tiles = empty_saving_to(2, Some(dir.clone()));
        let key = (5, 5);
        assert_eq!(tiles.tile_of(CHEST.0 >> 4, CHEST.2 >> 4), key);

        publish(&tiles, key, RETIRE_AFTER_SECS + 1);
        // Straight into the store, which is what `World::load_from` does and
        // what a placement would do through `set_block` — unavailable here,
        // because `WorldTables::empty()` cannot validate a state.
        let store = crate::blockentity::BlockEntities::shared();
        let mut be = crate::blockentity::BlockEntity::new(crate::blockentity::Kind::Chest);
        be.set_item(SLOT, crate::item::ItemStack::new("minecraft:stone", COUNT));
        store.insert(CHEST, be);

        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1, "the tile retired");
        assert!(
            store.get(CHEST).is_none(),
            "the retired tile's block entities are still resident — nothing was evicted"
        );

        // Walk back in. The tile is generated again over the same area and
        // `publish` loads what was saved.
        publish(&tiles, key, 0);
        let back = store
            .get(CHEST)
            .expect("the chest was evicted without being written down first");
        assert_eq!(
            back.item(SLOT),
            crate::item::ItemStack::new("minecraft:stone", COUNT)
        );
        assert!(back.item(0).is_empty(), "the slot index did not survive");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// With nowhere to write it down, retirement keeps the block entities —
    /// the same choice the overlay already makes, and for the same reason.
    ///
    /// **This is not a smaller version of the test above, it is the other
    /// arm.** An implementation that evicted unconditionally passes that one
    /// whenever persistence happens to be on, and loses every chest on a
    /// server started without `--world`.
    #[test]
    fn retirement_without_a_world_directory_keeps_the_block_entities() {
        let tiles = empty(2);
        assert!(tiles.region_dir().is_none(), "persistence is off");
        let key = (5, 6);
        // A different chunk from the test above: the store is shared and the
        // two tests run on parallel threads.
        let pos = (400, 64, 480);
        assert_eq!(tiles.tile_of(pos.0 >> 4, pos.2 >> 4), key);
        publish(&tiles, key, RETIRE_AFTER_SECS + 1);
        let store = crate::blockentity::BlockEntities::shared();
        let mut be = crate::blockentity::BlockEntity::new(crate::blockentity::Kind::Chest);
        be.set_item(SLOT, crate::item::ItemStack::new("minecraft:stone", COUNT));
        store.insert(pos, be);

        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1, "the tile retired");
        assert_eq!(
            store.get(pos).map(|b| b.item(SLOT)),
            Some(crate::item::ItemStack::new("minecraft:stone", COUNT)),
            "a chest was dropped with nowhere to reload it from"
        );
        store.evict_chunks(tiles.area_of(key).chunks());
    }

    /// Eviction is bounded by the tile that retired. A chest in the tile next
    /// door is not in the retired tile's chunks and must stay — a `clear()`
    /// where an `evict_chunks` belongs passes both tests above.
    #[test]
    fn retiring_one_tile_does_not_evict_another_tiles_chests() {
        let dir = scratch("neighbour");
        let tiles = empty_saving_to(2, Some(dir.clone()));
        let (going, staying) = ((7, 7), (8, 8));
        let (gone_pos, kept_pos) = ((560, 64, 560), (640, 64, 640));
        assert_eq!(tiles.tile_of(gone_pos.0 >> 4, gone_pos.2 >> 4), going);
        assert_eq!(tiles.tile_of(kept_pos.0 >> 4, kept_pos.2 >> 4), staying);

        publish(&tiles, going, RETIRE_AFTER_SECS + 1);
        publish(&tiles, staying, 0);
        let store = crate::blockentity::BlockEntities::shared();
        for pos in [gone_pos, kept_pos] {
            store.insert(
                pos,
                crate::blockentity::BlockEntity::new(crate::blockentity::Kind::Chest),
            );
        }
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1, "one tile retired");
        assert!(store.get(gone_pos).is_none(), "the retired tile's chest");
        assert!(
            store.get(kept_pos).is_some(),
            "a resident tile's chest went with its neighbour's"
        );
        store.evict_chunks(tiles.area_of(staying).chunks());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `VmRSS` in kB, which is the number an operator's memory limit is
    /// enforced against and the one `docs/BENCH.md` quotes.
    ///
    /// `/proc/self/status` rather than `statm`, so it is the same field
    /// `ci/bench-boot.py` reads and the two are comparable.
    ///
    /// **`None` means unavailable, never zero.** Only Linux and Android have
    /// `/proc/self/status`; on Windows, macOS and FreeBSD there is no
    /// implementation here, and a Linux box whose read fails is reported the
    /// same way. The bench prints `null` for it, so an unmeasured arm cannot
    /// read as a process that used no memory.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn vm_rss_kb() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse().ok())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn vm_rss_kb() -> Option<u64> {
        None
    }

    /// The one-minute load average, as `/proc/loadavg` prints it. `None` is
    /// unavailable, for the same reasons and on the same platforms as
    /// [`vm_rss_kb`].
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn load_1m() -> Option<String> {
        let loadavg = std::fs::read_to_string("/proc/loadavg").ok()?;
        loadavg.split_whitespace().next().map(str::to_owned)
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn load_1m() -> Option<String> {
        None
    }

    /// A measured value as JSON, or `null` when the platform could not
    /// measure it.
    fn json_or_null<T: std::fmt::Display>(v: Option<T>) -> String {
        v.map_or_else(|| "null".to_owned(), |v| v.to_string())
    }

    /// **The measurement for the eviction, one arm per process.**
    ///
    /// `#[ignore]`d, so it is not in T0 and never competes for RSS with the
    /// three hundred tests it shares a binary with. `ci/bench-rss-blockentities.sh`
    /// runs it, alternating the arms, and takes the median across runs.
    ///
    /// # The two arms, and why the control is the point
    ///
    /// Both fill one tile with `RUSTMC_EVICT_BENCH_CHESTS` chests and retire
    /// it. They differ in one thing: whether the world has a directory to
    /// save into, which is exactly the condition eviction is gated on.
    ///
    /// * `on` — persistence on: the tile is written, its block entities are
    ///   evicted, `return_freed_memory` trims;
    /// * `off` — persistence off: the tile is written nowhere and its block
    ///   entities are kept, which is the deliberate choice
    ///   `retirement_without_a_world_directory_keeps_the_block_entities`
    ///   pins.
    ///
    /// **A one-armed version of this proves nothing**, which is the trap
    /// `docs/BENCH.md`'s reload control already paid for: RSS after a
    /// retirement can be lower for a dozen reasons that have nothing to do
    /// with block entities — the tile's own canvas going, an allocator that
    /// happened to trim, a different page-cache mood. The `off` arm holds
    /// every one of those constant and changes only the thing under test.
    ///
    /// It prints one JSON line and asserts nothing about the numbers: a
    /// threshold here would be a gate on the Threadripper's allocator, and
    /// `docs/BENCH.md` is where measurements live.
    #[test]
    #[ignore = "a measurement, not a gate — run it through ci/bench-rss-blockentities.sh"]
    fn bench_rss_after_retiring_a_tile_full_of_chests() {
        let chests: i32 = std::env::var("RUSTMC_EVICT_BENCH_CHESTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50_000);
        let arm = std::env::var("RUSTMC_EVICT_BENCH_ARM").unwrap_or_else(|_| "on".to_string());
        let dir = scratch("bench");
        let tiles = match arm.as_str() {
            "off" => empty(2),
            _ => empty_saving_to(2, Some(dir.clone())),
        };
        let key = (5, 5);
        publish(&tiles, key, RETIRE_AFTER_SECS + 1);

        // Spread over the tile's own chunks, so every one of them is inside
        // the area the retirement evicts. Four filled slots each: the empty
        // ones cost their `ItemStack` and no heap string, and a chest with
        // nothing in it would understate what a real one holds.
        let store = crate::blockentity::BlockEntities::shared();
        let area = tiles.area_of(key);
        let (x0, z0) = (area.min_chunk_x * 16, area.min_chunk_z * 16);
        let side = tiles.side() * 16;
        for i in 0..chests {
            let pos = (
                x0 + (i % side),
                40 + (i / (side * side)),
                z0 + ((i / side) % side),
            );
            let mut be = crate::blockentity::BlockEntity::new(crate::blockentity::Kind::Chest);
            for slot in 0..4 {
                be.set_item(
                    slot,
                    crate::item::ItemStack::new("minecraft:cobblestone", 64),
                );
            }
            store.insert(pos, be);
        }
        // Trimmed on both sides of the reading, so the `before` figure is
        // not carrying the allocator's opinion of the fill loop.
        return_freed_memory();
        let before = vm_rss_kb();
        let held = store.len();

        let retired = tiles.retain_around(&BTreeSet::new());
        let after = vm_rss_kb();
        let left = store.len();

        let delta = before.zip(after).map(|(b, a)| {
            i64::try_from(b).unwrap_or(i64::MAX) - i64::try_from(a).unwrap_or(i64::MAX)
        });
        let load = load_1m().map(|l| format!("\"{l}\""));
        println!(
            "{{\"arm\":\"{arm}\",\"chests\":{chests},\"retired\":{retired},\
             \"held\":{held},\"left\":{left},\
             \"rss_before_kb\":{},\"rss_after_kb\":{},\
             \"delta_kb\":{},\"load\":{}}}",
            json_or_null(before),
            json_or_null(after),
            json_or_null(delta),
            json_or_null(load)
        );
        assert_eq!(
            retired, 1,
            "the tile has to have retired for this to mean anything"
        );
        // Leaves the store as it found it, whichever arm ran: the process
        // exits straight after, but a bench that depended on that would be
        // one edit away from poisoning every test beside it.
        store.evict_chunks(area.chunks());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A tile that is retired and regenerated must not inherit the clock the
    /// previous generation was running, or it is retired the instant it
    /// appears and the player never sees it.
    #[test]
    fn a_regenerated_tile_does_not_inherit_the_old_clock() {
        let tiles = empty(2);
        let key = (5, 5);
        publish(&tiles, key, RETIRE_AFTER_SECS + 1);
        assert_eq!(tiles.retain_around(&BTreeSet::new()), 1);
        // Back again, with no clock of its own.
        publish(&tiles, key, 0);
        assert_eq!(
            tiles.retain_around(&BTreeSet::new()),
            0,
            "a regenerated tile was retired on the sweep that followed it"
        );
        assert_eq!(tiles.ready_count(), 1);
    }

    /// The view-to-tiles arithmetic, which decides what `wanted` holds. A
    /// view that fits inside one tile must not name four, and a view wider
    /// than a tile must name every tile it crosses.
    #[test]
    fn tiles_in_view_names_every_tile_the_square_touches() {
        let tiles = empty(2);
        // Side 5, so a view distance of 1 around the origin fits inside the
        // spawn tile.
        let one = tiles.tiles_in_view((0, 0), 1);
        assert_eq!(one.len(), 1, "a view inside one tile named {one:?}");
        // A view distance of 10 spans chunks -10..=10, which at side 5 is
        // tiles -2..=2 on each axis: 25 of them.
        let wide = tiles.tiles_in_view((0, 0), 10);
        assert_eq!(wide.len(), 25, "a 21-chunk view at side 5 spans 5x5 tiles");
        for c in [-10, -3, 0, 7, 10] {
            assert!(
                wide.contains(&tiles.tile_of(c, c)),
                "the tile owning chunk ({c},{c}) is in the view but not in the set"
            );
        }
    }

    /// A world with no tiles refuses every write rather than accepting one
    /// that the generation which follows would silently overwrite.
    #[test]
    fn an_ungenerated_position_reads_as_air_and_refuses_writes() {
        let tiles = empty(2);
        assert_eq!(tiles.state_at((0, 64, 0)), "minecraft:air");
        assert_eq!(tiles.set_block((0, 64, 0), "minecraft:stone"), None);
        // An ungenerated position has no tile, so the delegation never
        // reaches a rule and the context's contents cannot change the
        // answer. Holding air, which is the emptiest one there is.
        assert!(!tiles.replaceable(
            (0, 64, 0),
            &crate::placement::Replacing {
                item: None,
                face: crate::interact::Face::Up,
                cursor_y: 0.5,
                replacing_clicked: true,
                secondary_use: false,
                table: None,
            }
        ));
    }
}
