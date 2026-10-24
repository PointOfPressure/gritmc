//! A decorated world, served to a client.
//!
//! [`crate::terrain`] sends the noise stage: stone, water and ore veins, with
//! no surface, no caves and no trees. This module closes that gap by running
//! the **whole pipeline** — noise, biomes, surface rules, carvers, decoration
//! — and serving chunks out of the finished canvas.
//!
//! # Why a pre-generated area rather than a chunk at a time
//!
//! Because a chunk at a time is *wrong*, and quietly. Carvers read a 17x17
//! neighbourhood and decoration writes one chunk outwards, so a chunk is only
//! finished once every chunk that can reach it has been. Generating on demand
//! and answering immediately would hand out chunks whose trees had not been
//! planted yet, and the world would depend on the order players walked
//! through it — ORACLE.md §6, a fault this project has already paid for once.
//!
//! `rustmc_worldgen::pipeline` states the rule: generate a margin, offer only
//! the inside. So the server generates a square up front and serves from it.
//! The honest limit of that is the square: walk off the edge and there is
//! nothing there. Streaming — generating a new ring as a player approaches
//! and retiring the one behind — is the next piece, and it is a scheduling
//! problem rather than a correctness one.
//!
//! # What a player will see, and what is still missing
//!
//! Real terrain: hills shaped by the surface rules, caves and canyons cut,
//! trees and grass and ores — **and structures.** These three lines said
//! "what is missing is ... structures, of which only mineshafts are ported,
//! and those are not run here. No villages, no strongholds, no shipwrecks"
//! until 2026-09-05, and it was never true of this function:
//! [`World::generate_through`] calls `worldgen_assembly::generate::generate`,
//! which is the dump's own entry point and lays out seven overworld structure
//! sets before the noise stage. `tests/served_structures.rs` decodes the
//! chunks this module hands the packet writer and finds a mineshaft, a trial
//! chamber, a ruined portal, a savanna village, an ocean ruin, a shipwreck
//! and a buried treasure in them.
//!
//! What is missing is every nether and End structure, because this server
//! serves the overworld only; all seventeen overworld sets are laid out.
//! `worldgen_assembly::generate::Generated::caveats` carries the full list.
//!
//! Block entities exist now ([`crate::blockentity`]) and the line that used to
//! sit here said they did not. The accurate version is narrower and still a
//! real gap: **the canvas stores block states and nothing else**, so a
//! generated block entity exists only where the side channel recorded a loot
//! table (the rest — spawners, beds, signs in a village — still have none),
//! while a chest a player *places* gets one through
//! [`Blocks::set_block`] — and, since `docs/PARITY-ROADMAP.md` §31, [`chunk`]
//! puts it on the wire, in the `LEVEL_CHUNK_WITH_LIGHT` block-entity section
//! every server sends whether or not the type has a contents packet. A
//! generated chest's loot table arrives through the worldgen side channel
//! and rolls on first open — see `crate::lootroll::adopt_generated`, called
//! from [`World::generate_in`].
//!
//! [`chunk`]: World::chunk
//!
//! Light is on by default since 2026-09-05. Every chunk packet carries the
//! incremental engine's own classification; `RUSTMC_SECTION_LIGHT=0` puts the
//! four empty bitsets back for a bisect. See [`World::section_light`] for the
//! measurement that bought the flip.

use crate::dimension::Dimension;
use std::io;
use std::sync::Arc;

use rustmc_behavior::block_props::{BlockPropsTable, Heightmap as PropHeightmap};
use rustmc_worldgen::canvas::{Canvas, LiveHeightmap};
use rustmc_worldgen::pipeline::{Area, Through};

use crate::chunk::{
    ContainerKind, Heightmap, LevelChunkWithLight, LightData, PalettedContainer, Section,
};
use crate::edits::{BlockEdits, Blocks};
use crate::interact::BlockPos;
use crate::terrain::StateIds;

/// The three heightmaps a client is sent.
///
/// `Heightmap.Types` carries an explicit `id` field rather than relying on
/// the ordinal, and `sendToClient()` is `usage == CLIENT`. Read from the
/// jar's static initialiser: `WORLD_SURFACE` is 1, `MOTION_BLOCKING` 4,
/// `MOTION_BLOCKING_NO_LEAVES` 5, and those three are the CLIENT ones.
const CLIENT_HEIGHTMAPS: [(i32, LiveHeightmap, PropHeightmap); 3] = [
    (1, LiveHeightmap::WorldSurface, PropHeightmap::WorldSurface),
    (
        4,
        LiveHeightmap::MotionBlocking,
        PropHeightmap::MotionBlocking,
    ),
    (
        5,
        LiveHeightmap::MotionBlockingNoLeaves,
        PropHeightmap::MotionBlockingNoLeaves,
    ),
];

/// The four heightmaps a chunk is **saved** with, by the names vanilla uses
/// on disk, paired with the predicate each is defined by.
///
/// Not the same set as [`CLIENT_HEIGHTMAPS`], and the difference is the point:
/// a client is sent three, because `Heightmap.Types.sendToClient()` is true
/// for exactly those, while a saved chunk carries four — `OCEAN_FLOOR` is
/// stored and never transmitted. Sharing one list between the two would have
/// silently dropped it from every save.
///
/// The names must match [`worldgen_assembly::heightmaps_from_canvas`]'s keys,
/// which is what a saved column's map is built from.
const SAVED_HEIGHTMAPS: [(&str, PropHeightmap); 4] = [
    ("WORLD_SURFACE", PropHeightmap::WorldSurface),
    ("OCEAN_FLOOR", PropHeightmap::OceanFloor),
    ("MOTION_BLOCKING", PropHeightmap::MotionBlocking),
    (
        "MOTION_BLOCKING_NO_LEAVES",
        PropHeightmap::MotionBlockingNoLeaves,
    ),
];

/// The three lookup tables a [`World`] needs that do not depend on the world.
///
/// # Why this is a type rather than three fields
///
/// All three are pure functions of `data/vanilla` and the registry capture:
/// the same bytes whichever square is being served, and immutable once
/// loaded. They were previously three fields on `World`, cloned into every
/// tile — and since [`crate::tiles`] keeps one `World` per tile and a player
/// walks through many, the server held one copy of each per tile.
///
/// `block_props.json` was additionally **re-parsed** for every tile, because
/// `worldgen_assembly::block_props()` reads the file each time it is called.
/// Loading it once and sharing the result is the same table by construction —
/// it is a parse of a fixed file — so this is a saving with no behavioural
/// term at all.
///
/// Grouping them means a tile is created from one `Arc` clone instead of
/// three deep copies, and it names the invariant: **these are the parts of a
/// world that are not about the world.**
pub struct WorldTables {
    /// Registry ids for the biome names the canvas uses.
    biome_ids: std::collections::BTreeMap<String, i32>,
    /// The four per-state light inputs, indexed by global state id. Loaded
    /// beside `ids` because they come out of the same file and a light engine
    /// with no emission table lights a world in which nothing emits.
    light: crate::light_engine::LightProps,
    /// State name to global palette id.
    ids: StateIds,
    /// The per-state facts a heightmap is defined in terms of. Needed only
    /// once a player can change a block: the canvas primes its own
    /// heightmaps during generation, and an edit invalidates the column.
    props: BlockPropsTable,
}

impl WorldTables {
    /// Load the block tables and number the biome registry.
    ///
    /// `biome_registry` is the client's own biome numbering — see
    /// [`crate::configuration::CapturedRegistries::entry_ids`].
    ///
    /// # Errors
    /// If `block_props.json` is missing or the registry is longer than `i32`.
    pub fn load(biome_registry: &[String], ids: StateIds) -> io::Result<Self> {
        let props = worldgen_assembly::block_props().map_err(io::Error::other)?;
        // The running server is launched from the repo root and finds the
        // table relatively; a `cargo test` is launched from the package
        // directory and does not. Both are tried, in that order, because the
        // alternative — a compile-time path only — bakes the build machine's
        // layout into the binary, and the alternative to that is a test that
        // cannot construct a `WorldTables` at all.
        let relative = std::path::Path::new("data/vanilla/block_props.json");
        let light = if relative.exists() {
            crate::light_engine::LightProps::load(relative)?
        } else {
            crate::light_engine::LightProps::load(std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../data/vanilla/block_props.json"
            )))?
        };
        let mut biome_ids = std::collections::BTreeMap::new();
        for (i, name) in biome_registry.iter().enumerate() {
            biome_ids.insert(name.clone(), i32::try_from(i).map_err(io::Error::other)?);
        }
        Ok(Self {
            biome_ids,
            light,
            ids,
            props,
        })
    }

    /// The block-property table, for a caller that needs it before it has a
    /// world — the superflat preset resolves its layer states through it.
    #[must_use]
    pub const fn props(&self) -> &BlockPropsTable {
        &self.props
    }

    /// No tables at all — for tests about the arithmetic around a world
    /// rather than about the world.
    ///
    /// Mirrors [`StateIds::empty`] and exists for the same reason: the tile
    /// grid is the part that is wrong in a way no amount of generating would
    /// reveal, and a test of it should not need the vanilla extract.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            biome_ids: std::collections::BTreeMap::new(),
            light: crate::light_engine::LightProps::default(),
            ids: StateIds::empty(),
            props: BlockPropsTable::default(),
        }
    }
}

/// A generated square of world, ready to serve.
///
/// The canvas is what the pipeline produced and is never written to again —
/// see [`crate::edits`] for why. Player writes live in `edits`, and every
/// read this module does is "the overlay, else the canvas".
pub struct World {
    canvas: Canvas,
    inner: Area,
    /// What the generator said this world is missing, in its own words.
    ///
    /// Taken from `Generated::caveats()` at generation and carried, rather
    /// than recomputed from a list in this file: the generator is the thing
    /// that knows what it did not run, and a second copy here is a caveat
    /// list that lags the code — which reads as authoritative and is worse
    /// than none.
    caveats: Vec<&'static str>,
    /// Which dimension this square belongs to.
    ///
    /// Carried rather than inferred from `canvas.height()`, for the same
    /// reason `worldgen_assembly::generate::Generated` carries it: the nether
    /// and the End are both 256 blocks tall, so height cannot tell them
    /// apart, and a caveat list or a `min_y` that named the wrong one would
    /// be silently wrong rather than absent.
    dimension: Dimension,
    /// The lookup tables, shared with every other tile — see [`WorldTables`].
    tables: Arc<WorldTables>,
    /// What players have changed, and **the one part of a `World` that
    /// outlives it.**
    ///
    /// Behind an `Arc` so [`crate::tiles`] can keep hold of the overlay after
    /// it has retired the canvas underneath it. A retired tile is regenerated
    /// from the same seed over the same area and is therefore identical; the
    /// overlay re-applied on top of it restores exactly what the player
    /// built. Without this, returning a tile's memory would mean discarding
    /// their work.
    edits: Arc<BlockEdits>,
    /// The live light for this whole area — built on first use and
    /// **repaired** on every edit after that.
    ///
    /// It lives on the `World` and not on a chunk because vanilla's stored
    /// section set is a **3x3x3 cube in section space** and therefore reads
    /// the eight neighbouring chunks (see [`crate::light_engine`]). Lighting
    /// per chunk would either do nine times the work or produce a different
    /// answer at every chunk border, and the second is worse: it would look
    /// like progress and be a fidelity regression.
    ///
    /// It was a `OnceLock` until 2026-09-05, which made every player edit
    /// after the first chunk send invisible to the light on the wire. See
    /// [`LiveLight`].
    light: std::sync::Mutex<Option<LiveLight>>,
}

impl World {
    /// Generate `area` for `seed`, all stages, and index it for serving.
    ///
    /// `tables` is shared with every other tile — see [`WorldTables`]. A
    /// biome the canvas produces that the registry does not list is an error
    /// rather than a fallback, for the same reason an unknown block state is:
    /// a world full of the wrong biome looks like a generator bug, and
    /// [`WorldTables::load`] is where that is caught.
    ///
    /// # Errors
    /// If the vanilla extract is missing or a stage cannot be built.
    pub fn generate(seed: i64, area: Area, tables: Arc<WorldTables>) -> io::Result<Self> {
        Self::generate_through(seed, area, tables, Through::Decoration)
    }

    /// Generate `area` for `seed` and stop after `through`.
    ///
    /// A server always wants [`Through::Decoration`] and this exists for the
    /// gate that has to tell a decorated world from an undecorated one:
    /// `tests/tiled_world.rs`'s widening comparison is four `Some(0)` cases
    /// now, and a generator that dropped every feature would satisfy all four
    /// (`docs/DETERMINISM.md` §32.5). Stopping at [`Through::Carvers`] and
    /// counting the difference is what says decoration happened at all.
    ///
    /// # Errors
    /// If the vanilla extract is missing or a stage cannot be built.
    pub fn generate_through(
        seed: i64,
        area: Area,
        tables: Arc<WorldTables>,
        through: Through,
    ) -> io::Result<Self> {
        Self::generate_in(Dimension::Overworld, seed, area, tables, through)
    }

    /// Generate `area` of `dimension` for `seed`, stopping after `through`.
    ///
    /// The one place a served dimension picks its generator, and the three
    /// arms are `worldgen_assembly`'s own three entry points — the same
    /// functions the whole-world dump calls, so a served chunk and a dumped
    /// one come out of the same code and a parity number taken on the dump
    /// transfers to the server. (The structures lane established that for the
    /// overworld through `generate`; this extends it to the other two by
    /// construction rather than by a second measurement.)
    ///
    /// **The per-dimension seed is the world seed, unchanged.** `WorldOptions`
    /// holds one `seed` and `ServerLevel`'s constructor passes it to every
    /// `LevelStem`'s chunk generator; nothing derives a second seed per
    /// dimension. What differs is the noise router and the noise settings,
    /// which is why the three dimensions of one seed are different worlds
    /// without any seed arithmetic.
    ///
    /// # Errors
    /// If the vanilla extract is missing or a stage cannot be built.
    pub fn generate_in(
        dimension: Dimension,
        seed: i64,
        area: Area,
        tables: Arc<WorldTables>,
        through: Through,
    ) -> io::Result<Self> {
        let generated = match dimension {
            Dimension::Overworld => worldgen_assembly::generate::generate(seed, area, through),
            Dimension::Nether => worldgen_assembly::generate::generate_nether(seed, area, through),
            Dimension::TheEnd => worldgen_assembly::generate::generate_end(seed, area, through),
        }
        .map_err(io::Error::other)?;
        // The generator's own answer, not ours: a canvas whose height
        // disagreed with the dimension type would encode a chunk packet the
        // client reads the wrong number of sections out of, and the failure
        // is a desync several fields later rather than a short packet.
        let want = dimension.height();
        let got = generated.canvas.height();
        if got != want {
            return Err(io::Error::other(format!(
                "{} generated a canvas {got} blocks tall, but its dimension type stores {want}",
                dimension.id()
            )));
        }
        let caveats = generated.caveats();
        // The generated containers' loot tables, into the block-entity store,
        // for the chunks this world serves. See `crate::lootroll::adopt_generated`.
        crate::lootroll::adopt_generated(
            &generated.canvas,
            area.chunks(),
            crate::blockentity::BlockEntities::shared(),
        );
        // And the entities its structures spawned — mansion illagers and
        // allays, monument elders, end city shulkers and the ship's frame.
        // See `crate::structure_entities`.
        crate::structure_entities::adopt_generated(
            &generated.canvas,
            area.chunks(),
            crate::entities::Entities::shared(),
        );
        Ok(Self {
            canvas: generated.canvas,
            inner: area,
            caveats,
            dimension,
            tables,
            edits: Arc::new(BlockEdits::new()),
            light: std::sync::Mutex::new(None),
        })
    }

    /// Which dimension this world is.
    #[must_use]
    pub const fn dimension(&self) -> Dimension {
        self.dimension
    }

    /// Fill `area` from vanilla's superflat preset and index it for serving.
    ///
    /// The substrate the gameplay oracle measures in — see [`crate::flat`]
    /// for why a server that generates real terrain needs a world with none.
    /// Same shape as [`World::generate`] and deliberately so: everything
    /// downstream of the canvas, from the overlay to the chunk encoder, must
    /// not be able to tell which generator produced it, or the harness would
    /// be exercising a second server rather than this one.
    ///
    /// # Errors
    /// If the preset names a block state the table does not hold.
    pub fn generate_flat(
        preset: &crate::flat::FlatPreset,
        area: Area,
        tables: Arc<WorldTables>,
    ) -> io::Result<Self> {
        let props = tables.props();
        let mut canvas = preset.canvas(area, &preset.states(props)?);
        for (cx, cz) in area.chunks() {
            canvas.prime_heightmaps(cx, cz, &crate::flat::Predicates(props));
        }
        Ok(Self {
            canvas,
            inner: area,
            caveats: vec!["superflat: this world was not generated at all"],
            // Vanilla's superflat preset is an overworld generator: a flat
            // world has the overworld's `min_y` and its 24 sections, whatever
            // the harness is measuring.
            dimension: Dimension::Overworld,
            tables: Arc::clone(&tables),
            edits: Arc::new(BlockEdits::new()),
            light: std::sync::Mutex::new(None),
        })
    }

    /// Hand this world the overlay it should serve reads from and record
    /// writes into, replacing the empty one it was built with.
    ///
    /// Called by [`crate::tiles`] between generation and publication, so a
    /// regenerated tile comes back carrying everything a player built on it
    /// before it was retired. It is the whole reason retiring a tile is not
    /// the same as losing one: the canvas is reproducible from the seed, and
    /// this is the part that is not.
    ///
    /// Must be called before the world is published — a `&mut self` receiver
    /// is what enforces that, since a published tile is behind an `Arc`.
    pub fn adopt_edits(&mut self, edits: Arc<BlockEdits>) {
        self.edits = edits;
    }

    /// The one world this process serves, generated on the first join.
    ///
    /// Not a cache. Two players have to be standing in the *same* world, and
    /// generating per connection gives each of them their own — identical
    /// today, because the seed is fixed and nothing can change a block, and
    /// silently divergent the moment either of those stops being true. The
    /// cost is the other half: the pipeline runs every stage over the square
    /// plus its margin — `(2r + 1 + 2 * DECORATION_MARGIN)^2`, so 81 chunks
    /// at radius 2 and 625 at the current `status::WORLD_RADIUS` of 10 — and
    /// a player should pay that once for the server rather than once each.
    /// (This said "49 chunks at radius two", which used a margin of 1;
    /// `DECORATION_MARGIN` is 2 and has been throughout.)
    ///
    /// The first caller's arguments win; later ones are ignored, which is
    /// correct precisely because there is only one world.
    ///
    /// # Errors
    /// Whatever [`World::generate`] returned on the first call, repeated —
    /// the error is kept rather than retried, so a missing extract is
    /// reported the same way to every player instead of costing each of them
    /// a fresh failed generation.
    pub fn shared(
        seed: i64,
        area: Area,
        biome_registry: &[String],
        ids: StateIds,
    ) -> Result<&'static Self, String> {
        static WORLD: std::sync::OnceLock<Result<World, String>> = std::sync::OnceLock::new();
        WORLD
            .get_or_init(|| {
                let tables = WorldTables::load(biome_registry, ids).map_err(|e| e.to_string())?;
                Self::generate(seed, area, Arc::new(tables)).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Load what the world needs from disk and generate it, once.
    ///
    /// Call this **before listening**, not on the first join. Generating 49
    /// chunks through every stage takes tens of seconds, and a client gives
    /// up long before that: the first version of this generated during login
    /// and the join failed at exactly twenty seconds, which is the client's
    /// read timeout and not a bug in anything it was waiting for. Vanilla
    /// does the same thing and says so — "Preparing spawn area".
    ///
    /// # Errors
    /// If the block table or the registry capture is missing, or generation
    /// fails.
    pub fn prepare(seed: i64, radius: i32) -> io::Result<&'static Self> {
        let ids = StateIds::load(std::path::Path::new("data/vanilla/block_props.json"))?;
        let registries = crate::configuration::CapturedRegistries::load(std::path::Path::new(
            "data/vanilla/registries.bin",
        ))?;
        let biomes = registries
            .entry_ids("minecraft:worldgen/biome")?
            .ok_or_else(|| {
                io::Error::other("the registry capture has no minecraft:worldgen/biome")
            })?;
        Self::shared(seed, Area::square(0, 0, radius), &biomes, ids).map_err(io::Error::other)
    }

    /// The area that is correct to serve — the requested square, not the
    /// margin generated around it.
    #[must_use]
    pub const fn area(&self) -> Area {
        self.inner
    }

    /// What this world is not — the generator's own caveats for the
    /// dimension it generated, plus the ones that belong to *serving* it.
    ///
    /// Carried rather than logged: a caller that reports terrain from here
    /// should not be able to do it without having had the caveats in hand.
    ///
    /// This was a `fn caveats()` with no receiver until the nether and the
    /// End became servable, and it printed the overworld's structure list
    /// over any world at all. `worldgen_assembly::generate::Generated`
    /// carries a `dimension` field for exactly that reason, and this now
    /// carries what that field produced.
    #[must_use]
    pub fn caveats(&self) -> Vec<&'static str> {
        let mut out = self.caveats.clone();
        out.extend(Self::serving_caveats());
        out
    }

    /// The caveats that are about **serving** a world rather than about
    /// generating one, so they are the same in all three dimensions.
    #[must_use]
    pub fn serving_caveats() -> Vec<&'static str> {
        vec![
            "a generated container gets its loot table and seed (they roll on \
             first open) and a structure's mobs, minecarts and frames are in \
             the level, but those entities have no AI: they do not move, \
             attack, take damage or despawn",
            "the outermost ring of the generated area is lit without its outside \
             neighbours and is wrong there, in the same way and for the same reason \
             its decoration is",
            "a tile a player walks into takes seconds to generate; until then it is void",
            "a chunk is only as finished as DECORATION_MARGIN makes it, and no margin \
             width converges: a tile's rim always sits DECORATION_MARGIN rings inside \
             the outermost chunk it decorated (docs/DETERMINISM.md 31). Seed 20260722 \
             is unaffected in practice; seeds 0 and 12345 are not",
        ]
    }

    /// What players have changed since the world was generated.
    #[must_use]
    pub fn edits(&self) -> &BlockEdits {
        &self.edits
    }

    /// The overlay itself, so a caller can keep it after this world is gone.
    ///
    /// [`crate::tiles`] holds one of these per tile for the life of the
    /// process — it is small, being one entry per block a player changed —
    /// and hands it back to the regenerated tile through
    /// [`World::adopt_edits`].
    #[must_use]
    pub fn edits_handle(&self) -> Arc<BlockEdits> {
        Arc::clone(&self.edits)
    }

    /// Whether the generated square covers this chunk.
    ///
    /// `Area`'s bounds are inclusive on both ends — `chunks()` iterates
    /// `min..=max` — and getting that wrong would drop the square's far edge
    /// on save and on load, which is the corner a player is most likely to
    /// have walked to.
    const fn holds_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        chunk_x >= self.inner.min_chunk_x
            && chunk_x <= self.inner.max_chunk_x
            && chunk_z >= self.inner.min_chunk_z
            && chunk_z <= self.inner.max_chunk_z
    }

    /// Write every chunk a player has changed into `region_dir`, in vanilla's
    /// Anvil format. Returns how many chunks were written.
    ///
    /// # What is saved, and what "a chunk" means here
    ///
    /// Only *edited* chunks. The rest of the world is a pure function of the
    /// seed and regenerating it is cheaper than storing it, so writing an
    /// untouched chunk would buy nothing and cost a megabyte. What lands on
    /// disk is the full column — canvas plus overlay, every section, the
    /// heightmaps — not a diff, because the file has to be a **vanilla
    /// chunk**, and a vanilla chunk is not expressed relative to anything.
    ///
    /// # Why `write_region_merging`
    ///
    /// A region file holds 1,024 chunks and a player usually edits one.
    /// [`rustmc_world::anvil::write_region`] lays out only the chunks it is
    /// handed and truncates the rest away, so using it here would delete
    /// every other chunk in the region on every save — including chunks saved
    /// a moment earlier by this same loop, leaving whichever chunk sorted
    /// last. `ci/t2-vanilla-loads-ours.sh` is the test that can see the
    /// difference.
    ///
    /// # Errors
    /// If the region directory cannot be created or written.
    pub fn save_to(&self, region_dir: &std::path::Path) -> io::Result<usize> {
        let mut columns = Vec::new();
        // **The union, not just the edited chunks.** A chest placed this
        // session is in both sets, so the difference is invisible until the
        // second run: a chest *loaded* from disk and then filled has no block
        // edit at all, and saving only `edited_chunks` would write its chunk
        // back without the items that were just put in it. That is the
        // difference between a container that survives a restart and one that
        // survives only the restart after the one that created it.
        //
        // **Filtered by `holds_chunk`, and that is not belt-and-braces.** The
        // block-entity store is process-wide while a `World` is one *tile* —
        // `crate::tiles` holds several and calls this on each in turn — so the
        // set includes every other tile's chests. The edited-chunk guard below
        // turns a chunk this world does not hold into an `Err`, which is right
        // for an edit (it should be impossible) and wrong for these (it is
        // routine). Without the filter the second tile a player visited would
        // make every save of the first one fail.
        let block_entities = crate::blockentity::BlockEntities::shared();
        // One reading for the whole save, so every chunk's `LastUpdate` and
        // every tick's delay are relative to the same instant. Taking it per
        // chunk would make two chunks of one save disagree about what time it
        // is, and a tick's `t` is a *difference* from it.
        let game_time = crate::levelstore::ticks();
        let mut chunks: std::collections::BTreeSet<(i32, i32)> =
            self.edits.edited_chunks().into_iter().collect();
        chunks.extend(
            block_entities
                .chunks()
                .into_iter()
                .filter(|(cx, cz)| self.holds_chunk(*cx, *cz)),
        );
        for (cx, cz) in chunks {
            // An edit outside the generated square has no canvas to build a
            // column from. It should not be reachable — a player can only
            // change a block in a chunk that was sent to them — but silently
            // dropping it would be a lost build, and `column_from_canvas`
            // panics rather than returning, so the guard is here.
            if !self.holds_chunk(cx, cz) {
                return Err(io::Error::other(format!(
                    "edit in chunk ({cx}, {cz}), which is outside this world's generated square"
                )));
            }
            let mut column = rustmc_worldgen::anvil_bridge::column_from_canvas(
                &self.canvas,
                cx,
                cz,
                // `minecraft:full` is honest here in a way it is not in the
                // noise-stage dump: this canvas has been through every stage
                // the pipeline has. What it has not been through is
                // structures, which `caveats` already says out loud.
                "minecraft:full",
                rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
            );
            column.heightmaps = worldgen_assembly::heightmaps_from_canvas(&self.canvas, cx, cz);
            // The overlay last, on top of the generated column.
            let edits = self.edits.in_chunk(cx, cz);
            for (pos, state) in &edits {
                column.set_block_at(pos.0, pos.1, pos.2, state);
            }

            // The heightmaps above came from the canvas, so for any column a
            // player changed they describe the world *before* the change.
            // Saving them stale is not cosmetic: vanilla trusts a saved
            // heightmap rather than recomputing it, so a tower built on a
            // reloaded world would have mobs spawning on the ground inside
            // it and rain falling through it.
            //
            // Recomputed only for the (x, z) columns that actually carry an
            // edit — usually a handful — because `edited_height` walks the
            // full height of the world for each one.
            let touched: std::collections::BTreeSet<(i32, i32)> =
                edits.iter().map(|(p, _)| (p.0, p.2)).collect();
            for (wx, wz) in touched {
                for (name, map) in SAVED_HEIGHTMAPS {
                    let Some(values) = column.heightmaps.get_mut(name) else {
                        continue;
                    };
                    // `z * 16 + x`, the order `ChunkColumn::heightmaps`
                    // documents and `pack_heightmap` writes in.
                    values[((wz & 15) * 16 + (wx & 15)) as usize] = self.edited_height(map, wx, wz);
                }
            }
            // The chunk's block entities, in position order. `in_chunk`
            // returns them sorted, which is what keeps a save reproducible —
            // vanilla's own order here is its chunk map's iteration order,
            // which is not something this server can reproduce and not
            // something any reader depends on.
            column.block_entities = block_entities
                .in_chunk(cx, cz)
                .into_iter()
                .map(|(pos, be)| be.to_nbt(pos))
                .collect();

            // `LastUpdate` and `block_ticks`, which were written as constants
            // until 2026-09-05 and were the two named holes in the save path.
            //
            // What is written here and what is NOT, stated rather than left
            // to be found:
            //
            // * `LastUpdate` — the game tick this save was taken at. Real.
            // * `block_ticks` — the queue's own pending entries for this
            //   chunk, undrained, as `SavedTick` compounds. Real, and it is
            //   what makes a scheduled water flow or a container recheck
            //   survive a restart instead of being silently cancelled.
            // * `fluid_ticks` — the fluid queue's pending entries for this
            //   chunk, in the same `SavedTick` shape: `LevelChunkTicks.save`
            //   is one method serving both `LevelTicks`, and `i` holds the
            //   fluid's registry id where a block tick holds the block's.
            //   Real since 2026-10-01, when `crate::ticks` grew the second
            //   queue; it was empty before because there was no queue to
            //   read it from.
            // * `InhabitedTime` — **real since 2026-09-05**, and written as
            //   the value already on disk PLUS what this process accumulated.
            //   `crate::inhabited` has the rule out of
            //   `ServerChunkCache.tickChunks`: once per tick per chunk, by
            //   the game-time delta, for a chunk within 128 blocks of a
            //   non-spectator player — per CHUNK and not per player, which is
            //   the thing every single-player test agrees with either way.
            //   The counter is taken after it is written or the next save
            //   would read its own answer back and add it again.
            // * `isLightOn` — **REFUSED BY NAME, and the refusal moved from
            //   `Byte(0)` to no key at all.** `SerializableChunkData.copyOf`
            //   guards the write with `ifeq` on `lightCorrect`, so vanilla
            //   writes nothing for a chunk whose light it has not computed;
            //   the read is `getBooleanOr("isLightOn", false)`, so the two
            //   load identically and differ on disk. See
            //   `rustmc_world::anvil::generated_extra`.
            //
            //   The refusal is by name because the engine exists and has not
            //   merged: `origin/light-engine-2` is 417 commits unlanded and
            //   its own cold start reports 245 of 2,572,288 nibbles against
            //   vanilla's captured chunk-packet light. **What the chunk NBT
            //   will want from it when it lands is not `isLightOn` alone** —
            //   `isLightOn` is a flag, and the payload is the per-section
            //   `BlockLight` and `SkyLight` byte arrays, each 2048 bytes,
            //   each written only when its `DataLayer` is non-null
            //   (`copyOf` offsets 249..268 for BlockLight). Writing the flag
            //   true without the arrays would tell vanilla the light is
            //   correct and give it none, which is worse than the 0 this
            //   replaced: vanilla would trust an empty layer instead of
            //   relighting.
            //
            //   So the value to write is: the arrays from the engine, per
            //   section, and the flag set only if every section that has
            //   light has been written. Refused until then.
            // **Seeded from the chunk already on disk, when there is one.**
            // This column was rebuilt from the canvas, so its `extra` is
            // `generated_extra()` — empty `structures`, empty tick queues,
            // `InhabitedTime` 0. Writing that over a chunk vanilla generated
            // would delete the village that starts in it: the blocks would
            // survive and vanilla would no longer know a structure was there.
            // See `rustmc_world::anvil::read_chunk_extra`.
            match rustmc_world::anvil::read_chunk_extra(region_dir, cx, cz) {
                Ok(Some(saved)) => column.extra = saved,
                Ok(None) => {}
                // A region file we cannot parse is not a reason to refuse the
                // save — the alternative is losing the player's build too —
                // but it is a reason to say so.
                Err(e) => eprintln!("chunk ({cx}, {cz}): could not read the saved chunk: {e}"),
            }
            column
                .extra
                .insert("LastUpdate".into(), rustmc_world::nbt::Nbt::Long(game_time));
            let ticks = crate::ticks::ScheduledTicks::shared();
            // `InhabitedTime`, folded onto whatever the chunk on disk
            // carried. The read has to come from `column.extra` and not from
            // the region file again: `read_chunk_extra` above has just seeded
            // it, and a second read would be a second answer to the same
            // question with a write in between.
            let saved_inhabited = match column.extra.get("InhabitedTime") {
                Some(rustmc_world::nbt::Nbt::Long(v)) => *v,
                _ => 0,
            };
            let inhabited = crate::inhabited::Inhabited::shared();
            column.extra.insert(
                "InhabitedTime".into(),
                rustmc_world::nbt::Nbt::Long(inhabited.saved_value(cx, cz, saved_inhabited)),
            );
            inhabited.take(cx, cz);
            column.extra.insert(
                "block_ticks".into(),
                rustmc_world::nbt::Nbt::List(
                    ticks
                        .pending_in_chunk(cx, cz)
                        .iter()
                        .map(|t| crate::ticks::saved_tick_nbt(t, game_time))
                        .collect(),
                ),
            );
            column.extra.insert(
                "fluid_ticks".into(),
                rustmc_world::nbt::Nbt::List(
                    ticks
                        .pending_fluid_in_chunk(cx, cz)
                        .iter()
                        .map(|t| crate::ticks::saved_tick_nbt(t, game_time))
                        .collect(),
                ),
            );
            columns.push(column);
        }
        let written = columns.len();
        rustmc_world::anvil::write_region_merging(region_dir, &columns)?;
        // The entities generation put in those chunks — a structure's
        // garrison — into the sibling `entities/` store, beside the block
        // chunk vanilla would otherwise load with nobody in it: a saved
        // chunk is a generated one to vanilla, which never spawns a
        // structure's mobs again. They never move or die on this server (see
        // `crate::structure_entities`), so the generated compound is still
        // their state, UUID included.
        if let Some(entity_dir) = region_dir.parent().map(|d| d.join("entities")) {
            let entity_chunks: Vec<rustmc_world::entitystore::EntityChunk> = columns
                .iter()
                .filter_map(|c| {
                    let generated = self.canvas.entity_records(c.x, c.z);
                    (!generated.is_empty()).then(|| rustmc_world::entitystore::EntityChunk {
                        chunk_x: c.x,
                        chunk_z: c.z,
                        data_version: rustmc_worldgen::anvil_bridge::DATA_VERSION_26_2,
                        entities: generated.to_vec(),
                    })
                })
                .collect();
            if !entity_chunks.is_empty() {
                rustmc_world::entitystore::write_region_merging(&entity_dir, &entity_chunks)?;
            }
        }
        Ok(written)
    }

    /// Load previously saved chunks from `region_dir` into the overlay.
    /// Returns how many blocks were restored.
    ///
    /// # Why this fills the overlay rather than replacing the canvas
    ///
    /// The canvas is what the pipeline produced and [`crate::edits`] explains
    /// at length why nothing may write into it: it is the object every parity
    /// measurement is taken against, and a world a player has dug in is no
    /// longer a generator's output. So loading a save asks the same question
    /// the overlay already answers — *how does this world differ from what
    /// the seed generates* — and the answer is exactly the set of positions
    /// where the saved chunk and the canvas disagree.
    ///
    /// That makes the round trip lossless for anything a player can do, and
    /// it makes a save of an *unedited* world a no-op rather than a
    /// world-sized overlay. It also means a saved world reloaded under a
    /// changed generator keeps the player's build and picks up the new
    /// terrain, which is the behaviour worth having while the generator is
    /// still moving.
    ///
    /// A chunk on disk that this world's square does not cover is skipped:
    /// the region file legitimately holds chunks from a larger session.
    ///
    /// # Errors
    /// If a region file exists but cannot be read or parsed. A region
    /// directory that does not exist yet is not an error — that is a first
    /// boot.
    pub fn load_from(&self, region_dir: &std::path::Path) -> io::Result<usize> {
        if !region_dir.is_dir() {
            return Ok(0);
        }
        let min_y = self.canvas.min_y();
        let max_y = min_y + self.canvas.height();
        let mut restored = 0usize;

        // Only the region files this world's square actually reaches, so a
        // directory holding a much larger world costs the square rather than
        // the world.
        let mut regions = std::collections::BTreeSet::new();
        for (cx, cz) in self.inner.chunks() {
            regions.insert((cx >> 5, cz >> 5));
        }
        for (rx, rz) in regions {
            let path = region_dir.join(format!("r.{rx}.{rz}.mca"));
            if !path.exists() {
                continue;
            }
            for column in rustmc_world::anvil::read_region(&path)? {
                if !self.holds_chunk(column.x, column.z) {
                    continue;
                }
                for y in min_y..max_y {
                    for z in 0..16 {
                        for x in 0..16 {
                            let (wx, wz) = (column.x * 16 + x, column.z * 16 + z);
                            let Some(saved) = column.block_at(wx, y, wz) else {
                                continue;
                            };
                            // Borrowed on both sides: this runs 98,304 times
                            // per saved chunk, and comparing two `String`s
                            // built for the purpose would allocate twice per
                            // block to answer a question that is almost
                            // always "no".
                            if saved != self.canvas.block_at(wx, y, wz) {
                                self.edits.set((wx, y, wz), saved);
                                restored += 1;
                            }
                        }
                    }
                }
                // The block entities, after the blocks.
                //
                // **The order matters and the reason is not obvious.**
                // `self.edits.set` above writes through `crate::edits`
                // directly rather than through `Blocks::set_block`, so it does
                // **not** run `on_block_changed` and does not create a block
                // entity — which is exactly what is wanted here, because
                // creating one would then be overwritten by the saved one and,
                // worse, would leave a fresh empty chest at any position the
                // save recorded a chest at but whose block entity this server
                // could not parse. Loading the saved ones afterwards is the
                // only step that puts anything in the store.
                //
                // A compound this server cannot parse is dropped by
                // `from_nbt` — a type it does not model, or a chunk written by
                // something else. That loses the block entity and keeps the
                // block, which is the same thing vanilla does with a block
                // entity whose type has been removed by a datapack.
                for nbt in &column.block_entities {
                    if let Some((pos, be)) = crate::blockentity::BlockEntity::from_nbt(nbt) {
                        crate::blockentity::BlockEntities::shared().insert(pos, be);
                    }
                }
            }
        }
        Ok(restored)
    }

    /// The state at a position: the player overlay if there is one, the
    /// generated canvas otherwise.
    #[must_use]
    pub fn block_at(&self, (x, y, z): BlockPos) -> String {
        self.edits
            .get((x, y, z))
            .unwrap_or_else(|| self.canvas.block_at(x, y, z).to_string())
    }

    /// The flat cell index the canvas stores this position at, mirroring
    /// `Canvas::index`: `(y - min_y) * 256 + (z & 15) * 16 + (x & 15)`.
    ///
    /// `None` above or below the world, which is the only bound a chunk's
    /// cell array has — x and z are masked into the chunk by construction.
    fn cell_index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if y < self.canvas.min_y() || y >= self.canvas.min_y() + self.canvas.height() {
            return None;
        }
        usize::try_from((y - self.canvas.min_y()) * 256 + (z & 15) * 16 + (x & 15)).ok()
    }

    /// `Heightmap#getFirstAvailable` over the *merged* view: one past the
    /// highest block matching the map's predicate.
    ///
    /// Only called for a column a player has touched. The canvas primes its
    /// own heightmaps during generation and they are right for every
    /// untouched column; the moment a block changes, that column's answer is
    /// whatever the merged view says and has to be walked for.
    fn edited_height(&self, map: PropHeightmap, x: i32, z: i32) -> i32 {
        let min_y = self.canvas.min_y();
        let mut y = min_y + self.canvas.height() - 1;
        while y >= min_y {
            let state = self.block_at((x, y, z));
            if self
                .tables
                .props
                .get(&state)
                .is_some_and(|p| p.matches(map))
            {
                return y + 1;
            }
            y -= 1;
        }
        min_y
    }

    /// A sensible place to stand: the motion-blocking surface at a column.
    ///
    /// `MOTION_BLOCKING` rather than `WORLD_SURFACE` because the latter
    /// counts leaves, and spawning on a canopy drops the player through the
    /// tree.
    #[must_use]
    pub fn surface_at(&self, x: i32, z: i32) -> f64 {
        if self.edits.touched(x >> 4, z >> 4) {
            return f64::from(self.edited_height(PropHeightmap::MotionBlocking, x, z));
        }
        f64::from(self.canvas.height_of(LiveHeightmap::MotionBlocking, x, z))
    }

    /// The section light for one chunk, as the packet carries it.
    ///
    /// # The default is ON as of 2026-09-05, and this is the reading that
    /// bought it
    ///
    /// It landed opt-in under the rule that new behaviour is opt-in **until
    /// it is measured**, and the measurement named as the one that closes it
    /// — a comparison against a real server's light on a *generated* world —
    /// now exists. `crates/rustmc-server/tests/light_harvest.rs` reads
    /// vanilla's own saved `BlockLight`/`SkyLight` nibbles out of three
    /// harvested region sets and lights the same blocks with this engine:
    ///
    /// ```text
    ///   overworld  0 of 6,787,072 nibbles differ   289 chunks, r=8
    ///   nether     0 of 7,409,664                  289 chunks, r=8
    ///   the End    5,609 of 1,863,680               81 chunks, r=4
    /// ```
    ///
    /// with positive controls of 1,611,341 / 4,686,430 / 63,837 nibbles moved
    /// by making each window's commonest state emit light. Both sides read
    /// their blocks out of the same file, so no worldgen residual leaks into
    /// it — which is what makes it a reading about the light engine rather
    /// than about the generator.
    ///
    /// `RUSTMC_SECTION_LIGHT=0` (or `off`/`false`/empty) restores the four
    /// empty bitsets, so a bisect against the pre-flip wire is one variable.
    ///
    /// The area's outermost ring is lit without its outside neighbours and is
    /// wrong there in the same way and for the same reason its decoration is
    /// (`DECORATION_MARGIN`, and `docs/DETERMINISM.md` 31).
    fn section_light(&self, chunk_x: i32, chunk_z: i32) -> LightData {
        if !section_light_enabled() {
            return LightData::default();
        }
        let mut g = self
            .light
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let live = g.get_or_insert_with(|| {
            LiveLight::build(&self.build_light_region(), &self.tables.light)
        });
        live.run_light_updates(&self.tables.light);
        crate::light::classify(self.light_geometry(), &live.light.chunk(chunk_x, chunk_z))
    }

    /// The partial `light_update`s owed to clients, and the filters cleared —
    /// `ChunkHolder.broadcastChanges`'s light half.
    ///
    /// Empty unless a block edit has changed some section's light since the
    /// last call, which is the condition `hasChangesToBroadcast` tests. The
    /// caller sends one packet per entry to the players tracking that chunk;
    /// deciding *who* is `ChunkHolder.PlayerProvider`'s job and not this
    /// one's.
    #[must_use]
    pub fn light_updates(&self) -> Vec<crate::chunk::LightUpdate> {
        if !section_light_enabled() {
            return Vec::new();
        }
        let mut g = self
            .light
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A world nothing has lit yet owes nothing: every chunk it later
        // sends carries the whole column, which is strictly more than a
        // partial update would have said.
        let Some(live) = g.as_mut() else {
            return Vec::new();
        };
        live.run_light_updates(&self.tables.light);
        live.broadcast_changes(self.light_geometry())
    }

    /// `LevelChunk.setBlockState`'s light half, called from
    /// [`Blocks::set_block`] after the overlay has taken the write.
    ///
    /// Two things happen in vanilla and both happen here: a section that has
    /// just stopped being all-air is declared through `updateSectionStatus`,
    /// and then `LevelLightEngine.checkBlock` is called for the position.
    /// The drain is **not** here — vanilla's runs on the light thread and is
    /// picked up by the chunk source's tick, and ours runs at the next read
    /// of the light, so a batch of edits costs one repair rather than one
    /// per block.
    ///
    /// A world whose light has not been built yet does nothing at all: the
    /// build reads `self.edits` and will see this write when it happens.
    fn light_set_block(&self, pos: crate::interact::BlockPos, state: &str) {
        if !section_light_enabled() {
            return;
        }
        let mut g = self
            .light
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(live) = g.as_mut() else { return };
        let id = u16::try_from(self.tables.ids.get(state).unwrap_or(0)).unwrap_or(0);
        live.set_block(pos.0, pos.1, pos.2, id, is_air(state));
    }

    /// One raw light level, as the engine holds it.
    ///
    /// This is the query `BlockState.canSurvive` and the mob-spawn rules ask
    /// and this server has never been able to answer — `randomtick`'s
    /// `canStayAlive` substitute says so in its own doc. Nothing reads it
    /// yet; it is here because a light engine whose only consumer is the
    /// packet encoder is one live block away from being a second
    /// implementation, and because the post-edit test grades nibbles rather
    /// than masks.
    #[must_use]
    pub fn light_level(&self, layer: crate::light::LightLayer, pos: BlockPos) -> u8 {
        if !section_light_enabled() {
            return 0;
        }
        let mut g = self
            .light
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let live = g.get_or_insert_with(|| {
            LiveLight::build(&self.build_light_region(), &self.tables.light)
        });
        live.run_light_updates(&self.tables.light);
        live.light.level_at(
            layer,
            crate::light_incremental::as_long(pos.0, pos.1, pos.2),
        )
    }

    /// The lit region: the generated canvas with every player write laid over
    /// it, exactly as [`World::chunk`] lays them over the packet.
    ///
    /// Public because it is the *input* to the light, and a test that wants
    /// to grade the live incremental path against
    /// [`crate::light_incremental::light_region_with_edits`] — the form that
    /// was graded 2,340 of 2,340 post-edit sections against vanilla — needs
    /// the same region both routes see. Building a second one from the
    /// packets would grade the reconstruction as much as the light.
    #[must_use]
    pub fn build_light_region(&self) -> crate::light_engine::Region {
        use crate::light_engine::{LitChunk, Region};
        let geometry = self.light_geometry();
        let air = self.tables.ids.get("minecraft:air").unwrap_or(0);
        let mut chunks = std::collections::BTreeMap::new();
        let (min_x, min_z, max_x, max_z) = (
            self.inner.min_chunk_x,
            self.inner.min_chunk_z,
            self.inner.max_chunk_x,
            self.inner.max_chunk_z,
        );
        for cz in min_z..=max_z {
            for cx in min_x..=max_x {
                let Some((palette, cells)) = self.canvas.chunk_data(cx, cz) else {
                    continue;
                };
                let ids: Vec<u16> = palette
                    .iter()
                    .map(|s| u16::try_from(self.tables.ids.get(s).unwrap_or(0)).unwrap_or(0))
                    .collect();
                let air_flags: Vec<bool> = palette.iter().map(|s| is_air(s)).collect();
                // Player writes, laid over the canvas exactly as `chunk`
                // does — a torch somebody placed has to light the room.
                let mut overrides: std::collections::BTreeMap<usize, (u16, bool)> =
                    std::collections::BTreeMap::new();
                for ((x, y, z), state) in self.edits.in_chunk(cx, cz) {
                    if let Some(index) = self.cell_index(x, y, z) {
                        let id = self.tables.ids.get(&state).unwrap_or(0);
                        overrides.insert(index, (u16::try_from(id).unwrap_or(0), is_air(&state)));
                    }
                }
                let mut chunk = LitChunk::default();
                for s in 0..geometry.sections_count {
                    let start = (s as usize) * 4096;
                    let Some(cell) = cells.get(start..start + 4096) else {
                        continue;
                    };
                    let mut values = Vec::with_capacity(4096);
                    let mut any = false;
                    for (offset, &c) in cell.iter().enumerate() {
                        let i = c as usize;
                        let (id, cell_air) = overrides
                            .get(&(start + offset))
                            .copied()
                            .unwrap_or((ids[i], air_flags[i]));
                        if !cell_air {
                            any = true;
                        }
                        values.push(id);
                    }
                    // `LevelChunkSection.hasOnlyAir()` — the predicate
                    // `initializeLight` tests, and the input to the stored
                    // section rule. An all-air section is absent from the map.
                    if any {
                        chunk.sections.insert(geometry.min_section_y + s, values);
                    }
                }
                chunks.insert((cx, cz), chunk);
            }
        }
        Region {
            geometry,
            chunks,
            air: u16::try_from(air).unwrap_or(0),
        }
    }

    /// The per-state light inputs this world lights with.
    ///
    /// Public for the same reason [`World::build_light_region`] is: a test
    /// grading the live path against `light_region_with_edits` has to hand
    /// that function the same table, or it grades the table too.
    #[must_use]
    pub fn light_props(&self) -> &crate::light_engine::LightProps {
        &self.tables.light
    }

    /// The light geometry this canvas describes.
    #[must_use]
    pub fn light_geometry(&self) -> crate::light::LightGeometry {
        crate::light::LightGeometry {
            min_section_y: self.canvas.min_y().div_euclid(16),
            sections_count: self.canvas.height() / 16,
        }
    }

    /// The biome at a position, as the canvas generated it.
    #[must_use]
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> String {
        self.canvas.biome_at(x, y, z).to_string()
    }

    /// `Level.getMinY()` — the canvas's, which is the dimension's.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.canvas.min_y()
    }

    /// `Level.getMaxY()` — the last buildable y, `minY + height - 1`.
    #[must_use]
    pub const fn max_y(&self) -> i32 {
        self.canvas.min_y() + self.canvas.height() - 1
    }

    /// `Heightmap.Types.WORLD_SURFACE` at a column, over the merged view:
    /// one past the highest non-air block. What a mob's `canSeeSky` reads
    /// in the absence of a light engine — see `crate::mob::level`.
    #[must_use]
    pub fn world_surface_at(&self, x: i32, z: i32) -> i32 {
        if self.edits.touched(x >> 4, z >> 4) {
            return self.edited_height(PropHeightmap::WorldSurface, x, z);
        }
        self.canvas.height_of(LiveHeightmap::WorldSurface, x, z)
    }

    /// Encode one chunk for the wire, with whatever players have changed in
    /// it laid over the generated canvas.
    ///
    /// The overlay is applied here rather than at generation because a chunk
    /// is encoded every time it is streamed and generated once: a player who
    /// walks away and back has to find their hole still dug, and a player who
    /// joins later has to see it too.
    ///
    /// # Errors
    /// If the chunk was not generated, or holds a block state or biome the
    /// registries do not know.
    pub fn chunk(&self, chunk_x: i32, chunk_z: i32) -> io::Result<LevelChunkWithLight> {
        let (palette, cells) = self.canvas.chunk_data(chunk_x, chunk_z).ok_or_else(|| {
            io::Error::other(format!("chunk {chunk_x},{chunk_z} was not generated"))
        })?;
        let (biome_palette, biome_cells) = self
            .canvas
            .chunk_biomes(chunk_x, chunk_z)
            .ok_or_else(|| io::Error::other(format!("chunk {chunk_x},{chunk_z} has no biomes")))?;

        // Resolve each palette once rather than per block: a chunk is 98,304
        // cells over a palette of a few dozen states.
        let block_ids: Vec<i32> = palette
            .iter()
            .map(|s| self.tables.ids.get(s))
            .collect::<io::Result<_>>()?;
        let air: Vec<bool> = palette.iter().map(|s| is_air(s)).collect();
        let fluid: Vec<bool> = palette.iter().map(|s| holds_fluid(s)).collect();
        let biome_reg: Vec<i32> = biome_palette
            .iter()
            .map(|b| {
                self.tables.biome_ids.get(b).copied().ok_or_else(|| {
                    io::Error::other(format!("biome {b} is not in the client's registry"))
                })
            })
            .collect::<io::Result<_>>()?;

        // Player writes, resolved once per chunk into the same flat cell
        // index the canvas uses, so the section loop below is a lookup rather
        // than a search. Empty for every chunk nobody has touched, which is
        // almost all of them.
        let mut overrides: std::collections::BTreeMap<usize, (i32, bool, bool)> =
            std::collections::BTreeMap::new();
        let mut edited_columns: std::collections::BTreeSet<(i32, i32)> =
            std::collections::BTreeSet::new();
        for ((x, y, z), state) in self.edits.in_chunk(chunk_x, chunk_z) {
            let Some(index) = self.cell_index(x, y, z) else {
                continue;
            };
            overrides.insert(
                index,
                (
                    self.tables.ids.get(&state)?,
                    is_air(&state),
                    holds_fluid(&state),
                ),
            );
            edited_columns.insert((x, z));
        }

        let sections = self.canvas.height() / 16;
        let mut out = Vec::with_capacity(sections as usize);
        for s in 0..sections {
            // The canvas indexes `(y - min_y) * 256 + z * 16 + x`, which is
            // y-then-z-then-x — the same order a paletted container reads
            // its cells in. So a section is a contiguous slice, not a
            // gather.
            let start = (s as usize) * 16 * 256;
            let cell = &cells[start..start + 4096];
            let mut values = Vec::with_capacity(4096);
            let mut non_empty = 0i16;
            let mut fluids = 0i16;
            for (offset, &c) in cell.iter().enumerate() {
                let i = c as usize;
                let (id, cell_air, cell_fluid) = overrides
                    .get(&(start + offset))
                    .copied()
                    .unwrap_or((block_ids[i], air[i], fluid[i]));
                values.push(id);
                if !cell_air {
                    non_empty += 1;
                }
                if cell_fluid {
                    fluids += 1;
                }
            }
            // Biome cells are 4x4x4 and stored the same way, so a section is
            // four cell-layers of sixteen: also contiguous.
            let bstart = (s as usize) * 4 * 16;
            let bcell = &biome_cells[bstart..bstart + 64];
            let biomes: Vec<i32> = bcell.iter().map(|&b| biome_reg[b as usize]).collect();

            out.push(Section {
                non_empty_blocks: non_empty,
                fluid_count: fluids,
                states: PalettedContainer::from_values(ContainerKind::BlockStates, &values),
                biomes: PalettedContainer::from_values(ContainerKind::Biomes, &biomes),
            });
        }

        let bits = bits_for(self.canvas.height());
        let heightmaps = CLIENT_HEIGHTMAPS
            .iter()
            .map(|&(kind, map, predicate)| {
                let mut column = [0i32; 256];
                for z in 0..16 {
                    for x in 0..16 {
                        let (bx, bz) = (chunk_x * 16 + x, chunk_z * 16 + z);
                        // The canvas primed these during generation, so its
                        // answer is right everywhere a player has not
                        // written. Where one has, the column is walked over
                        // the merged view instead — the canvas cannot know
                        // about an edit it never saw.
                        let y = if edited_columns.contains(&(bx, bz)) {
                            self.edited_height(predicate, bx, bz)
                        } else {
                            self.canvas.height_of(map, bx, bz)
                        };
                        // Stored as an offset from the bottom of the world,
                        // which is what makes nine bits enough.
                        column[(z * 16 + x) as usize] = y - self.canvas.min_y();
                    }
                }
                Heightmap {
                    kind,
                    data: pack_heightmap(&column, bits),
                }
            })
            .collect();

        // `LevelChunk.getBlockEntities()`, walked whole by
        // `ClientboundLevelChunkPacketData`'s constructor with no filter —
        // unlike `block_entity_data`, which only a sign ever reaches, every
        // block entity in the chunk gets an entry here. A missing protocol
        // id is the one thing that skips one: every other input this loop
        // needs already exists once a player has placed anything, and a
        // missing `protocol_ids.json` is reported once at boot rather than
        // failing chunk encoding silently on every chunk after.
        let block_entities = crate::protocol_ids::shared().map_or_else(Vec::new, |ids| {
            crate::blockentity::BlockEntities::shared()
                .in_chunk(chunk_x, chunk_z)
                .into_iter()
                .filter_map(|(pos, be)| {
                    let type_id = ids.block_entity_type(be.kind().id())?;
                    crate::blockentity::chunk_payload_entry(pos, &be, type_id).ok()
                })
                .collect()
        });

        Ok(LevelChunkWithLight {
            chunk_x,
            chunk_z,
            heightmaps,
            sections: out,
            block_entities,
            // 26 of 26 sections, unless `RUSTMC_SECTION_LIGHT=0` puts the
            // pre-2026-09-05 empty masks back. See `section_light` for the
            // three-dimension harvest that bought the default.
            light: self.section_light(chunk_x, chunk_z),
        })
    }
}

/// Whether the section light reaches the wire.
///
/// **On unless switched off**, which is the opposite of the polarity this
/// flag shipped with. The off values are the ones a shell makes easy to type
/// — `0`, `off`, `false`, and the empty string — and anything else, the
/// variable being absent included, is on. A flag whose only off switch is
/// "unset it" cannot be turned off from a service file that sets it, and a
/// flag whose off switch is "any value at all" turns itself off the moment
/// somebody writes `RUSTMC_SECTION_LIGHT=1`.
fn section_light_enabled() -> bool {
    match std::env::var("RUSTMC_SECTION_LIGHT") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | ""
        ),
        Err(_) => true,
    }
}

impl Blocks for World {
    /// [`World`] is the only implementation that answers this: it is the
    /// only one that has a real `block_props.json` behind it.
    fn place_props(&self) -> Option<&BlockPropsTable> {
        (!self.tables.props.is_empty()).then_some(&self.tables.props)
    }

    fn state_at(&self, pos: BlockPos) -> String {
        self.block_at(pos)
    }

    /// The canvas's own floor, which is the one the chunk encoder and the
    /// index arithmetic already use — so a gravity block and a block write
    /// cannot disagree about where the bottom of the world is.
    fn level_min_y(&self) -> Option<i32> {
        Some(self.canvas.min_y())
    }

    /// `LevelChunkSection.isRandomlyTicking()` per section, over the merged
    /// view: a section ticks when any of its 4096 states has
    /// `isRandomlyTicking` or its fluid has — `tickingBlockCount > 0 ||
    /// tickingFluidCount > 0`. Both columns are `block_props.json`'s, which
    /// read them off the live `BlockState`, so a crop at its last age and a
    /// persistent leaf are already `false` there.
    ///
    /// Only for a chunk inside the inner area. The margin is generated to
    /// finish the inside and is never sent; vanilla has no such chunk to tick.
    fn random_ticking_sections(&self, chunk_x: i32, chunk_z: i32) -> Option<(i32, Vec<bool>)> {
        if !self.inner.contains(chunk_x, chunk_z) {
            return None;
        }
        let (palette, cells) = self.canvas.chunk_data(chunk_x, chunk_z)?;
        let ticks = |state: &str| {
            self.tables
                .props
                .get(state)
                .is_some_and(|p| p.randomly_ticking || p.fluid_randomly_ticking)
        };
        let flags: Vec<bool> = palette.iter().map(|s| ticks(s)).collect();
        let sections = usize::try_from(self.canvas.height() / 16).ok()?;
        // Counts rather than flags, because an edit can take a ticking cell
        // away as well as add one, and a flag cannot be decremented.
        let mut counts = vec![0i32; sections];
        for (i, &c) in cells.iter().enumerate() {
            if flags.get(usize::from(c)).copied().unwrap_or(false) {
                if let Some(n) = counts.get_mut(i / 4096) {
                    *n += 1;
                }
            }
        }
        self.edits
            .for_each_in_chunk(chunk_x, chunk_z, |(x, y, z), state| {
                let Some(index) = self.cell_index(x, y, z) else {
                    return;
                };
                let under = cells
                    .get(index)
                    .and_then(|&c| flags.get(usize::from(c)))
                    .copied()
                    .unwrap_or(false);
                let over = ticks(state);
                if let Some(n) = counts.get_mut(index / 4096) {
                    *n += i32::from(over) - i32::from(under);
                }
            });
        Some((
            self.canvas.min_y().div_euclid(16),
            counts.into_iter().map(|n| n > 0).collect(),
        ))
    }

    /// The canvas's `WORLD_SURFACE` for the column, raised past the highest
    /// edit in it. Never lower than the truth: an edit that removed the
    /// surface block leaves the bound where it was.
    fn column_top(&self, x: i32, z: i32) -> Option<i32> {
        let mut top = self.canvas.height_of(LiveHeightmap::WorldSurface, x, z);
        self.edits
            .for_each_in_chunk(x >> 4, z >> 4, |(ex, ey, ez), state| {
                if ex == x && ez == z && !is_air(state) {
                    top = top.max(ey + 1);
                }
            });
        Some(top)
    }

    /// Write a block into the overlay.
    ///
    /// Refused outside the **inner** area rather than the generated one. The
    /// margin exists so the inside is finished; it is not itself finished,
    /// is never sent to a client, and a write there would be invisible and
    /// permanent.
    fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
        if !self.inner.contains(pos.0 >> 4, pos.2 >> 4) {
            return None;
        }
        self.cell_index(pos.0, pos.1, pos.2)?;
        // An unknown state is refused here rather than written and failed on
        // at encode time, where it would poison every future send of the
        // chunk instead of the one packet that caused it.
        let id = self.tables.ids.get(state).ok()?;
        // Read **before** the write, because the block-entity half below
        // needs `oldState.getBlock()` and there is nowhere else to get it
        // once the overlay has been written.
        let was = self.block_at(pos);
        if was == state {
            return None;
        }
        self.edits.set(pos, state);
        // `LevelChunk.setBlockState`'s block-entity half, and it is here
        // rather than at the placement and break call sites because vanilla
        // puts it here: **every** route that changes a block goes through
        // `setBlockState`, including the cascades — a chest whose support is
        // removed, a block replaced by a falling one — and a hook on the two
        // obvious paths would leak a block entity down every other one.
        crate::blockentity::BlockEntities::shared().on_block_changed(pos, &was, state);
        // `LevelChunk.setBlockState`'s light half, in the same place and for
        // the same reason as the block-entity half above: every route that
        // changes a block comes through here.
        self.light_set_block(pos, state);
        Some(id)
    }

    fn default_state(&self, block: &str) -> Option<String> {
        self.tables
            .props
            .default_state(block)
            .map(ToString::to_string)
    }

    fn has_state(&self, state: &str) -> bool {
        self.tables.props.get(state).is_some()
    }

    fn face_sturdy(&self, pos: BlockPos, face: crate::interact::Face) -> bool {
        self.tables
            .props
            .get(&self.block_at(pos))
            .is_some_and(|p| p.sturdy_faces[face.to_3d() as usize])
    }

    fn redstone_conductor(&self, pos: BlockPos) -> bool {
        self.tables
            .props
            .get(&self.block_at(pos))
            .is_some_and(|p| p.redstone_conductor)
    }

    fn signal_source(&self, pos: BlockPos) -> bool {
        self.tables
            .props
            .get(&self.block_at(pos))
            .is_some_and(|p| p.signal_source)
    }

    fn note_block_instrument(&self, pos: BlockPos) -> Option<String> {
        // `pos.above()` then `pos.below()`, in that order, with the below
        // branch inverted — all of which is
        // `BlockPropsTable::note_block_instrument`, ported and tested beside
        // the column it reads.
        self.tables
            .props
            .note_block_instrument(
                &self.block_at((pos.0, pos.1 + 1, pos.2)),
                &self.block_at((pos.0, pos.1 - 1, pos.2)),
            )
            .map(|i| i.name.clone())
    }

    fn state_id(&self, state: &str) -> Option<i32> {
        self.tables.ids.get(state).ok()
    }

    fn replaceable(&self, pos: BlockPos, ctx: &crate::placement::Replacing<'_>) -> bool {
        // The props table's `replaceable` is the **no-argument**
        // `canBeReplaced()`. It is an input to the rule, not the answer:
        // `oak_slab` is `replaceable = false` here and is nevertheless
        // replaceable by another oak slab.
        let state = self.block_at(pos);
        let flag = self.tables.props.get(&state).is_some_and(|p| p.replaceable);
        crate::placement::can_be_replaced(&state, flag, ctx)
    }

    fn water_at(&self, pos: BlockPos) -> bool {
        // `FluidTags.WATER` holds both `water` and `flowing_water`, and the
        // per-state `fluid` field already distinguishes them from lava.
        matches!(
            self.tables
                .props
                .get(&self.block_at(pos))
                .and_then(|p| p.fluid.as_deref()),
            Some("minecraft:water" | "minecraft:flowing_water")
        )
    }

    fn fluid_of_state(&self, state: &str) -> crate::fluid::FluidState {
        let fluid = self
            .tables
            .props
            .get(state)
            .and_then(|p| p.fluid.as_deref());
        crate::fluid::of_state(state, fluid)
    }

    fn solid_of_state(&self, state: &str) -> bool {
        self.tables.props.get(state).is_some_and(|p| p.solid)
    }

    fn fast_lava(&self) -> bool {
        self.dimension == Dimension::Nether
    }

    fn water_evaporates(&self) -> bool {
        self.dimension == Dimension::Nether
    }

    /// `FluidState.createLegacyBlock()`, from the fluid `block_props.json`
    /// already records per state.
    ///
    /// The three cases, and they are the only three a world can present:
    /// a fluid block answers **itself**, so destroying water writes the state
    /// that is already there and nothing changes; a waterlogged block answers
    /// a water source, because `waterlogged` is only ever a source; anything
    /// else answers air.
    fn legacy_fluid_at(&self, pos: BlockPos) -> String {
        let state = self.block_at(pos);
        let Some(fluid) = self
            .tables
            .props
            .get(&state)
            .and_then(|p| p.fluid.as_deref())
        else {
            return "minecraft:air".to_string();
        };
        match name_of(&state) {
            "minecraft:water" | "minecraft:lava" => state,
            _ if fluid == "minecraft:water" => crate::placement::WATER_SOURCE.to_string(),
            _ if fluid == "minecraft:lava" => "minecraft:lava[level=0]".to_string(),
            _ => "minecraft:air".to_string(),
        }
    }
}

/// `BlockState.isAir()` — the three air blocks, not just the one.
///
/// Carving replaces stone with `cave_air`, so counting only `minecraft:air`
/// would report a hollowed-out section as nearly full and the client would
/// render it as solid.
fn is_air(state: &str) -> bool {
    matches!(
        name_of(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// Whether a state carries a fluid, by name.
///
/// Derived from the block name rather than a fluid registry, which is the
/// honest description of what this is: the two fluids and the waterlogged
/// property. It feeds `fluidCount`, which the server uses for fluid ticking
/// and nothing here ticks yet — so a wrong count costs nothing today, and
/// saying how it is computed is worth more than pretending it is exact.
fn holds_fluid(state: &str) -> bool {
    matches!(name_of(state), "minecraft:water" | "minecraft:lava")
        || state.contains("waterlogged=true")
}

fn name_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// Bits per heightmap entry: enough for every y in a world of this height.
///
/// `Mth.ceillog2(height + 1)` — 385 values for a 384-tall world is nine bits.
const fn bits_for(height: i32) -> u32 {
    let mut bits = 1;
    while (1i32 << bits) < height + 1 {
        bits += 1;
    }
    bits
}

/// Pack 256 columns at `bits` each, `64 / bits` to a long, no straddling.
///
/// The same layout a paletted container uses: entries do not cross a long
/// boundary, so the top bits of each long go unused.
fn pack_heightmap(values: &[i32; 256], bits: u32) -> Vec<i64> {
    let per_long = 64 / bits as usize;
    let mask = (1i64 << bits) - 1;
    let mut out = Vec::with_capacity(values.len().div_ceil(per_long));
    for group in values.chunks(per_long) {
        let mut word = 0i64;
        for (slot, &v) in group.iter().enumerate() {
            word |= (i64::from(v) & mask) << (slot * bits as usize);
        }
        out.push(word);
    }
    out
}

/// The live section light for a `World`: the region it was lit from, the two
/// engines' state, the classified layers a chunk packet reads, and the
/// per-chunk changed-section filters a partial `light_update` is sent with.
///
/// # Why this is not a `OnceLock`
///
/// It was one until 2026-09-05, and that made every block a player changed
/// after the first chunk send invisible to the light on the wire: the overlay
/// the light read was whatever `edits` held the first time a chunk was
/// encoded. Inert while the flag defaulted off, because the classification
/// was `LightData::default()` either way; a live defect the moment it did
/// not.
///
/// # The shape is `ServerLevel`'s, not a rebuild
///
/// `LevelChunk.setBlockState` declares a section that has stopped being
/// all-air and calls `LevelLightEngine.checkBlock`;
/// `ThreadedLevelLightEngine` drains on its own thread;
/// `LayerLightSectionStorage.swapSectionMap` hands each affected section to
/// `LightChunkGetter.onLightUpdate`; `ServerChunkCache.onLightUpdate` turns
/// that into `ChunkHolder.sectionLightChanged`, which sets one bit in one of
/// two per-chunk filters; and `ChunkHolder.broadcastChanges` sends one
/// `ClientboundLightUpdatePacket` carrying both filters and clears them. Each
/// of those is a method below, named for it.
///
/// The one departure is *when* the drain runs: vanilla's is a task on the
/// light thread and ours is at the next read of the light. The converged
/// answer is the same — a drain is `hasLightWork` to fixpoint either way —
/// and a batch of edits still costs one repair rather than one per block,
/// which is the property `ThreadedLevelLightEngine`'s queueing exists for.
struct LiveLight {
    /// The world as the light currently sees it. Kept in step with
    /// `World::edits` by [`LiveLight::set_block`], because a `Region` is
    /// borrowed for an `Engine`'s whole life and an edit produces a different
    /// one.
    region: crate::light_engine::Region,
    /// `Option` only so a drain can take the state out and hand it back;
    /// `Some` at every point a caller can observe.
    block: Option<crate::light_incremental::EngineState>,
    sky: Option<crate::light_incremental::EngineState>,
    /// What `crate::light::classify` reads — vanilla's `visibleSectionData`.
    light: crate::light_incremental::IncrementalLight,
    /// `ChunkHolder.skyChangedLightSectionFilter` and
    /// `blockChangedLightSectionFilter`, per chunk. Index is
    /// `sectionY - getMinLightSection()`.
    sky_changed: std::collections::BTreeMap<(i32, i32), Vec<bool>>,
    block_changed: std::collections::BTreeMap<(i32, i32), Vec<bool>>,
}

impl LiveLight {
    /// Light the region from scratch, in `ThreadedLevelLightEngine`'s order,
    /// and keep both engines' state.
    fn build(
        region: &crate::light_engine::Region,
        props: &crate::light_engine::LightProps,
    ) -> Self {
        use crate::light::LightLayer;
        use crate::light_incremental::{drain, Engine, Layer};
        let mut light = crate::light_incremental::IncrementalLight::default();
        let mut states = Vec::new();
        for (layer, light_layer) in [
            (Layer::Block, LightLayer::Block),
            (Layer::Sky, LightLayer::Sky),
        ] {
            let mut e = Engine::new(layer, region, props);
            // The two loops are separate and that is load-bearing: a chunk's
            // light depends on its neighbours' sections having been
            // declared, so nothing may propagate until every declaration is
            // in.
            for (&(cx, cz), c) in &region.chunks {
                for &sy in c.sections.keys() {
                    e.update_section_status((cx, sy, cz), false);
                }
                e.set_light_enabled((cx, cz), true);
            }
            for &(cx, cz) in region.chunks.keys() {
                e.propagate_light_sources((cx, cz));
            }
            drain(&mut e);
            // The from-scratch build owes nobody a `light_update`: the first
            // packet any client gets for these chunks is the whole column.
            // So the affected set is harvested and dropped, not turned into
            // filter bits.
            e.swap_section_map(light_layer, &mut light);
            states.push(e.into_state());
        }
        let sky = states.pop();
        let block = states.pop();
        Self {
            region: region.clone(),
            block,
            sky,
            light,
            sky_changed: std::collections::BTreeMap::new(),
            block_changed: std::collections::BTreeMap::new(),
        }
    }

    /// `LevelChunk.setBlockState`'s light half for one write.
    ///
    /// The region cell is updated first — it is the level the engine reads —
    /// and a section that has just stopped being all-air is created and
    /// declared. A section that has just *become* all-air is deliberately
    /// **not** undeclared: vanilla leaves that to chunk unload, and
    /// undeclaring it here would change the stored-section set under a client
    /// that still has the chunk.
    fn set_block(&mut self, x: i32, y: i32, z: i32, id: u16, air: bool) {
        use crate::light::DataLayer;
        let (cx, cz) = (x >> 4, z >> 4);
        let sy = y.div_euclid(16);
        let Some(chunk) = self.region.chunks.get_mut(&(cx, cz)) else {
            return;
        };
        let g = self.region.geometry;
        if sy < g.min_section_y || sy >= g.min_section_y + g.sections_count {
            return;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = DataLayer::index(
            x.rem_euclid(16) as u8,
            y.rem_euclid(16) as u8,
            z.rem_euclid(16) as u8,
        );
        let created = !chunk.sections.contains_key(&sy);
        if created {
            if air {
                // Air into an all-air section changes nothing the engine can
                // see, and materialising the section would declare it
                // non-empty — which is a lie the stored-section rule reads.
                return;
            }
            chunk.sections.insert(sy, vec![self.region.air; 4096]);
        }
        let section = chunk.sections.get_mut(&sy).expect("just inserted");
        section[index] = id;
        let p = crate::light_incremental::as_long(x, y, z);
        for state in [self.block.as_mut(), self.sky.as_mut()]
            .into_iter()
            .flatten()
        {
            if created {
                state.storage.update_section_status((cx, sy, cz), false);
            }
            state.check_block(p);
        }
    }

    /// `ThreadedLevelLightEngine`'s drain, plus the `swapSectionMap` half
    /// that turns affected sections into `ChunkHolder` filter bits.
    ///
    /// A no-op when neither engine has work, which is every call that is not
    /// preceded by an edit.
    fn run_light_updates(&mut self, props: &crate::light_engine::LightProps) {
        use crate::light::LightLayer;
        use crate::light_incremental::{drain, Layer};
        for layer in [Layer::Block, Layer::Sky] {
            let slot = match layer {
                Layer::Block => &mut self.block,
                Layer::Sky => &mut self.sky,
            };
            let state = slot.take().expect("an engine state is always present");
            if !state.has_light_work() {
                *slot = Some(state);
                continue;
            }
            let light_layer = match layer {
                Layer::Block => LightLayer::Block,
                Layer::Sky => LightLayer::Sky,
            };
            let mut e = state.bind(&self.region, props);
            drain(&mut e);
            let affected = e.swap_section_map(light_layer, &mut self.light);
            let slot = match layer {
                Layer::Block => &mut self.block,
                Layer::Sky => &mut self.sky,
            };
            *slot = Some(e.into_state());
            for (cx, sy, cz) in affected {
                self.section_light_changed(light_layer, (cx, cz), sy);
            }
        }
    }

    /// `ChunkHolder.sectionLightChanged(LightLayer, int)`.
    ///
    /// ```text
    ///   int min = lightEngine.getMinLightSection();
    ///   int max = lightEngine.getMaxLightSection();
    ///   if (sectionY < min || sectionY > max) return false;
    ///   BitSet f = layer == SKY ? skyChanged : blockChanged;
    ///   int i = sectionY - min;
    ///   if (!f.get(i)) { f.set(i); return true; }
    ///   return false;
    /// ```
    ///
    /// `getMaxLightSection()` is `min + getLightSectionCount()`, so the
    /// accepted range is **inclusive of one section past the last bit the
    /// packet reads**: a write in section `max` sets bit
    /// `getLightSectionCount()`, and `ClientboundLightUpdatePacketData`'s
    /// loop runs `0..getLightSectionCount()` and never looks at it. That is
    /// vanilla's arithmetic, not a rounding of it, so the filter here is one
    /// entry longer than the packet window and the extra entry is never read.
    /// A port that clamped instead would differ from vanilla only in an
    /// unreachable place, which is exactly the kind of difference that stops
    /// being unreachable later.
    fn section_light_changed(
        &mut self,
        layer: crate::light::LightLayer,
        chunk: (i32, i32),
        section_y: i32,
    ) -> bool {
        let g = self.region.geometry;
        let (min, max) = (g.min_light_section(), g.max_light_section());
        if section_y < min || section_y > max {
            return false;
        }
        let width = usize::try_from(g.light_section_count() + 1).unwrap_or(0);
        let filters = match layer {
            crate::light::LightLayer::Sky => &mut self.sky_changed,
            crate::light::LightLayer::Block => &mut self.block_changed,
        };
        let f = filters.entry(chunk).or_insert_with(|| vec![false; width]);
        let Ok(i) = usize::try_from(section_y - min) else {
            return false;
        };
        if f.get(i).copied().unwrap_or(false) {
            return false;
        }
        f[i] = true;
        true
    }

    /// `ChunkHolder.broadcastChanges`'s light half: one packet per chunk with
    /// a non-empty filter, then both filters cleared.
    fn broadcast_changes(
        &mut self,
        geometry: crate::light::LightGeometry,
    ) -> Vec<crate::chunk::LightUpdate> {
        let n = usize::try_from(geometry.light_section_count()).unwrap_or(0);
        let mut chunks: std::collections::BTreeSet<(i32, i32)> =
            self.sky_changed.keys().copied().collect();
        chunks.extend(self.block_changed.keys().copied());
        let mut out = Vec::with_capacity(chunks.len());
        for (cx, cz) in chunks {
            let empty = vec![false; n];
            let sky = self
                .sky_changed
                .get(&(cx, cz))
                .cloned()
                .unwrap_or(empty.clone());
            let block = self.block_changed.get(&(cx, cz)).cloned().unwrap_or(empty);
            // `hasChangesToBroadcast` — nothing is sent for a chunk whose
            // filters are both empty, which a stale map entry can be.
            if !sky.iter().take(n).any(|b| *b) && !block.iter().take(n).any(|b| *b) {
                continue;
            }
            out.push(crate::chunk::LightUpdate {
                chunk_x: cx,
                chunk_z: cz,
                light: crate::light::classify_changed(
                    geometry,
                    &self.light.chunk(cx, cz),
                    Some(&sky[..n]),
                    Some(&block[..n]),
                ),
            });
        }
        self.sky_changed.clear();
        self.block_changed.clear();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_384_tall_world_packs_heightmaps_nine_bits_at_a_time() {
        assert_eq!(bits_for(384), 9, "385 values needs nine bits");
        assert_eq!(bits_for(256), 9, "257 values still needs nine");
        assert_eq!(bits_for(128), 8);

        let packed = pack_heightmap(&[100; 256], 9);
        assert_eq!(packed.len(), 37, "seven columns to a long, 256 columns");
        assert_eq!(packed[0] & 0x1ff, 100);
        assert_eq!((packed[0] >> 9) & 0x1ff, 100, "nine bits up, not eight");
        // Seven fit; the top single bit of the long is unused, not the start
        // of an eighth column.
        assert_eq!(packed[0] >> 63, 0);
    }

    /// Carved rock is `cave_air`, and counting only `minecraft:air` would
    /// report a hollow section as solid.
    #[test]
    fn all_three_air_blocks_count_as_air() {
        assert!(is_air("minecraft:air"));
        assert!(is_air("minecraft:cave_air"));
        assert!(is_air("minecraft:void_air"));
        assert!(!is_air("minecraft:stone"));
        // A state with properties still resolves by name.
        assert!(!is_air("minecraft:oak_log[axis=y]"));
    }

    #[test]
    fn fluids_are_the_two_liquids_and_anything_waterlogged() {
        assert!(holds_fluid("minecraft:water[level=0]"));
        assert!(holds_fluid("minecraft:lava[level=0]"));
        assert!(holds_fluid("minecraft:oak_stairs[waterlogged=true]"));
        assert!(!holds_fluid("minecraft:oak_stairs[waterlogged=false]"));
        assert!(!holds_fluid("minecraft:stone"));
    }
}
