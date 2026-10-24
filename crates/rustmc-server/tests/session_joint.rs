//! **The joint between the seven optional tables and the connection loop.**
//!
//! # The defect this closes
//!
//! `Session` has six optional tables — `placement`, `shapes`, `sound_types`,
//! `ticks`, `placed_by`, `recipes` — and six construction sites. Five were
//! tests and passed `None` to all six; the sixth was `status::listen`, which
//! sets all six. So the wiring that carries every one of those tables from the
//! boot path into `keep_alive_loop` was witnessed by **nothing in any gate**,
//! only by a box-wide corpus run behind a contended lock.
//!
//! That is not six untested mechanisms. `crate::placement`,
//! `crate::placedby`, `crate::shapes`, `crate::ticks`,
//! `crate::sound_types` and `crate::recipes` each have module tests, and
//! every one of them passed throughout. It is **one untested joint, six times
//! over**: nothing anywhere asked whether setting the field does anything.
//!
//! It is the shape that left `IcebergFeature` green for a week. The feature
//! was fine; the gate never reached it. A mechanism can be ported, convert
//! its scenario, and still have its wiring unwitnessed — `sound_types` gates
//! a roadmap row already marked PORTED.
//!
//! # How this avoids witnessing its own fixture
//!
//! The rule the brief states and this file is built around: *if the test path
//! diverges from how production builds a `Session`, the test witnesses the
//! fixture rather than the joint.* Three things enforce it.
//!
//! 1. **There is no second loader.** The six tables come from
//!    [`rustmc_server::session_tables::SessionTables::load`], which is the
//!    same function `status::listen` calls and the only place in the tree
//!    that loads any of them. The two call sites differ in one argument —
//!    the root `data/vanilla` sits under, `.` for a server booted from the
//!    repo and `../..` for a test whose working directory is the crate — and
//!    in nothing else. There is no duplicated loading logic to drift.
//! 2. **The tables are the real extract**, parsed off `data/vanilla/*.json`
//!    by the production loaders. Not a hand-written fixture table: a
//!    three-entry `placement.json` would witness three entries.
//! 3. **Everything is driven through `keep_alive_loop`**, as bytes on a
//!    socket, never by calling a callee directly. The joint *is* the loop
//!    dispatch, so a test that reached past it would measure the half that
//!    was never in doubt.
//!
//! The world is the one remaining fixture, and it answers `has_state`,
//! `default_state`, `state_id`, `replaceable` and `face_sturdy` out of the
//! same `block_props.json` and `StateIds` that `world::World` does — its
//! `Blocks` impl is `world::World`'s with the canvas replaced by air.
//!
//! # Why one scenario and six assertions rather than six scenarios
//!
//! Every test here runs the **same** packet stream: place a stair, a door, a
//! coral block, a torch and a crafting table; dig the torch's support; open
//! the table and craft. One run, six independent observables.
//!
//! That is what makes the controls sharp. [`Cut`] removes exactly one table
//! from the `Session` — the field goes `None`, which is precisely the state
//! all five old test sites were in and precisely what a broken `status.rs`
//! would produce — and each `cut_*` test then asserts **its own arm dead and
//! the other five still alive**. So "break the wiring for one table and
//! exactly that table's assertion goes red, at its own assertion, not a
//! neighbour's" is not a claim made in a commit message and re-checked by
//! hand: it is six tests that fail if it stops being true.
//!
//! A control that cannot fire is not a control, so note what the cuts are
//! *not*: they are not `#[ignore]`d, they are not behind an env var, and they
//! do not assert "the field is `None`". They assert on packets and on blocks.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use rustmc_proto::varint::write_varint;
use rustmc_proto::{frame, ids, wire};
use rustmc_server::edits::{BlockEdits, Blocks};
use rustmc_server::interact::{BlockPos, Face};
use rustmc_server::session_tables::SessionTables;
use rustmc_server::terrain::StateIds;

/// A path inside the repo, from the crate this test lives in.
///
/// The same `../..` the `root` argument to `SessionTables::load` gets, and
/// for the same reason: `cargo test` runs an integration test with the crate
/// as its working directory, and every vanilla table is addressed from the
/// repo root.
fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The seven extract files this whole file depends on.
///
/// Checked by hand rather than through `worldgen_assembly::have_extract`,
/// which asks about the *worldgen* datagen extract and would answer `true`
/// for a checkout that has it and none of these.
const NEEDED: [&str; 9] = [
    "data/vanilla/placement.json",
    "data/vanilla/use_item_on.json",
    "data/vanilla/set_placed_by.json",
    "data/vanilla/update_shape.json",
    "data/vanilla/sound_types.json",
    "data/vanilla/recipes.json",
    "data/vanilla/block_props.json",
    "data/vanilla/items.json",
    "data/vanilla/fire_odds.json",
];

/// Everything loaded once for the whole binary.
///
/// `cargo test` runs these on parallel threads in one process and the tables
/// are megabytes of JSON, so they are parsed once. This is also the only
/// arrangement that is honest about `shapes`, `placed_by` and `sound_types`:
/// those are `OnceLock`-backed in production too, and a per-test load would
/// silently be the first load's result anyway.
struct Fixture {
    tables: SessionTables,
    uses: &'static rustmc_server::useblock::UseTable,
    /// `useItemOn` per state and held item. Production loads it beside
    /// `uses`, and it runs **in front of** it, so a fixture without it
    /// witnesses only half the right-click path.
    item_uses: &'static rustmc_server::useitemon::UseItemOnTable,
    protocol_ids: &'static rustmc_server::protocol_ids::ProtocolIds,
    props: rustmc_behavior::block_props::BlockPropsTable,
    ids: StateIds,
    /// The item registry every reply's stack is numbered against.
    ///
    /// **Not optional here, because it is not optional in production.**
    /// `status::listen` sets it and `send_reply` resolves every stack through
    /// it; a `Session` built with `items: None` sends an empty stack for a
    /// perfectly good crafting result, so a recipe assertion over such a
    /// session would be red for a reason that has nothing to do with the
    /// recipe table. That is the same class of mistake this whole file
    /// exists to close, found inside the fixture for it on the first run.
    items: rustmc_server::player::ItemIds,
}

/// The fixture, or `None` on a checkout without the gitignored extract.
///
/// **The paragraph that stood here said the skip prints `skipping: no vanilla
/// extract`, "which is the marker every gate greps for: `grep -c "skipping: no
/// vanilla" <log>` must be `0` on a real run".** It is always `0`, on every
/// run, whether this fixture is present or absent — `cargo test` never prints
/// a passing test's stdout or stderr, and a test that returns early passes. The
/// sentence ended "A test whose skip is invisible is how a gate reads green for
/// a week without reaching the code it names", which is exactly what this file
/// was doing while saying it could not.
///
/// It matters more here than almost anywhere. The six `cutting_*` tests below
/// are this project's own proof that its assertions can fail — each cuts one
/// table and requires that arm to die *and the other five to live*. Behind a
/// silent skip, the falsifiers were as green on a tree with no tables as on a
/// correct one, which makes them a demonstration that proves nothing.
///
/// `fixture::available` panics in the default `required` mode instead, naming
/// the first missing table and the command that makes it. See
/// `crates/rustmc-data/src/fixture.rs` and `docs/PARITY-ROADMAP.md` §73.
fn fixture() -> Option<&'static Fixture> {
    static F: OnceLock<Option<Fixture>> = OnceLock::new();
    F.get_or_init(|| {
        // Every entry, not the first miss: a run that is missing four tables
        // should say four rather than send somebody to generate one and come
        // back. `all()` rather than `find()` for that, and the short-circuit is
        // deliberately given up.
        let mut ok = true;
        for p in NEEDED {
            ok &= rustmc_data::fixture::available(&repo(p), p, "ci/gen-gameplay-tables.sh");
        }
        if !ok {
            return None;
        }
        let root = repo("");
        Some(Fixture {
            tables: SessionTables::load(&root),
            uses: rustmc_server::useblock::UseTable::shared(&repo(
                "data/vanilla/use_without_item.json",
            ))
            .expect("the use table, checked present above"),
            item_uses: rustmc_server::useitemon::UseItemOnTable::shared(&repo(
                "data/vanilla/use_item_on.json",
            ))
            .expect("the useItemOn table, checked present above"),
            protocol_ids: rustmc_server::protocol_ids::ProtocolIds::shared(&repo(
                "data/vanilla/protocol_ids.json",
            ))
            .expect("the protocol id table"),
            props: rustmc_behavior::block_props::BlockPropsTable::load(&repo(
                "data/vanilla/block_props.json",
            ))
            .expect("the block property table"),
            ids: StateIds::load(&repo("data/vanilla/block_props.json")).expect("the state ids"),
            items: rustmc_server::player::ItemIds::load(&repo("data/vanilla/items.json"))
                .expect("the item registry"),
        })
    })
    .as_ref()
}

// ---------------------------------------------------------------------------
// The world
// ---------------------------------------------------------------------------

/// A world of air with an overlay, answering every property question out of
/// the real extract.
///
/// The `Blocks` impl below is `crate::world::World`'s with the canvas
/// replaced by air. That is deliberate and is the third of the three
/// anti-divergence measures in the module note: a bespoke `has_state` that
/// accepted two strings would make every placement in this file a test of
/// that bespoke function.
struct AirWorld {
    edits: BlockEdits,
    props: &'static rustmc_behavior::block_props::BlockPropsTable,
    ids: &'static StateIds,
}

impl AirWorld {
    fn new(f: &'static Fixture) -> Self {
        Self {
            edits: BlockEdits::new(),
            props: &f.props,
            ids: &f.ids,
        }
    }
    fn at(&self, pos: BlockPos) -> String {
        self.edits
            .get(pos)
            .unwrap_or_else(|| "minecraft:air".to_owned())
    }
}

impl Blocks for AirWorld {
    fn state_at(&self, pos: BlockPos) -> String {
        self.at(pos)
    }
    fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
        let id = self.ids.get(state).ok()?;
        if self.at(pos) == state {
            return None;
        }
        self.edits.set(pos, state);
        Some(id)
    }
    fn default_state(&self, block: &str) -> Option<String> {
        self.props.default_state(block).map(ToString::to_string)
    }
    fn has_state(&self, state: &str) -> bool {
        self.props.get(state).is_some()
    }
    fn state_id(&self, state: &str) -> Option<i32> {
        self.ids.get(state).ok()
    }
    /// **The props table's `replaceable` is the no-argument
    /// `canBeReplaced()`, which is an input to the rule and not the answer.**
    ///
    /// Byte for byte what `crate::world::World` does, because the module note
    /// above says this impl *is* `World`'s with the canvas replaced by air,
    /// and that is the third of this file's three anti-divergence measures.
    /// Taking `ctx` and ignoring it would satisfy the compiler and break that
    /// promise: five of the six arms here run with the **real**
    /// `placement.json` in `Session::placement`, so the context reaching this
    /// method carries a live table, and a double that dropped it would answer
    /// the bare flag where production answers `SlabBlock`, `SnowLayerBlock`
    /// and `replaceable && !item.is(asItem())`. Every placement in this file
    /// would then agree with whatever the item-aware path decided, which is
    /// the failure this world has already had twice — it called a chest
    /// replaceable once, and its flag omitted `tall_grass` once, and both
    /// were caught only by poison.
    ///
    /// It happens to move no outcome in the six arms as they stand: this
    /// canvas is air, air's flag is true and air has no item, so the rule
    /// reduces to the old answer at all five placement targets. **That is a
    /// property of today's fixture, not a reason to drop the argument** — and
    /// `the_world_double_answers_the_item_aware_rule_not_the_bare_flag` below
    /// is the assertion that keeps it honest, because nothing else here can
    /// tell the two implementations apart.
    fn replaceable(&self, pos: BlockPos, ctx: &rustmc_server::placement::Replacing<'_>) -> bool {
        let state = self.at(pos);
        let flag = self.props.get(&state).is_some_and(|p| p.replaceable);
        rustmc_server::placement::can_be_replaced(&state, flag, ctx)
    }
    fn face_sturdy(&self, pos: BlockPos, face: Face) -> bool {
        self.props
            .get(&self.at(pos))
            .is_some_and(|p| p.sturdy_faces[face.to_3d() as usize])
    }
    fn redstone_conductor(&self, pos: BlockPos) -> bool {
        self.props
            .get(&self.at(pos))
            .is_some_and(|p| p.redstone_conductor)
    }
    fn signal_source(&self, pos: BlockPos) -> bool {
        self.props
            .get(&self.at(pos))
            .is_some_and(|p| p.signal_source)
    }
    fn water_at(&self, pos: BlockPos) -> bool {
        matches!(
            self.props
                .get(&self.at(pos))
                .and_then(|p| p.fluid.as_deref()),
            Some("minecraft:water" | "minecraft:flowing_water")
        )
    }
    fn legacy_fluid_at(&self, _: BlockPos) -> String {
        "minecraft:air".to_owned()
    }
}

/// A socket that keeps what was written to it.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);
impl Write for Recorder {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("recorder").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Recorder {
    /// The frames written, as `(id, body)`.
    fn frames(&self) -> Vec<(i32, Vec<u8>)> {
        let bytes = self.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut out = Vec::new();
        while let Ok(f) = frame::read_frame(&mut cur, false) {
            out.push(f);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Packets
// ---------------------------------------------------------------------------

fn pack_pos(pos: BlockPos) -> i64 {
    (i64::from(pos.0) & 0x3FF_FFFF) << 38
        | (i64::from(pos.2) & 0x3FF_FFFF) << 12
        | (i64::from(pos.1) & 0xFFF)
}

/// `use_item_on`: hand, then a `BlockHitResult`, then the sequence.
fn place(pos: BlockPos, face: i32, sequence: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, 0).expect("buf");
    wire::write_i64(&mut body, pack_pos(pos)).expect("buf");
    write_varint(&mut body, face).expect("buf");
    for v in [0.5f32, 1.0, 0.5] {
        wire::write_f32(&mut body, v).expect("buf");
    }
    wire::write_bool(&mut body, false).expect("buf");
    wire::write_bool(&mut body, false).expect("buf");
    write_varint(&mut body, sequence).expect("buf");
    framed(ids::play::sb::USE_ITEM_ON, &body)
}

/// `player_action`: creative destroy at `pos`.
fn dig(pos: BlockPos, sequence: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, 0).expect("buf");
    wire::write_i64(&mut body, pack_pos(pos)).expect("buf");
    wire::write_u8(&mut body, 1).expect("buf");
    write_varint(&mut body, sequence).expect("buf");
    framed(ids::play::sb::PLAYER_ACTION, &body)
}

/// `set_carried_item`: a **short**, not a varint.
fn select(slot: u16) -> Vec<u8> {
    let mut body = Vec::new();
    wire::write_u16(&mut body, slot).expect("buf");
    framed(rustmc_server::player::ids::SET_CARRIED_ITEM, &body)
}

/// `container_click`, in `STREAM_CODEC`'s composite order.
///
/// `containerId` is a varint and `slotNum` a two-byte big-endian short; both
/// are called out in `click::decode_click` as the two that are easy to get
/// wrong. No changed slots and an absent carried claim, which is what a
/// client sends when its cursor is empty.
fn container_click(container_id: i32, state_id: i32, slot: i16, button: i8, input: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, container_id).expect("buf");
    write_varint(&mut body, state_id).expect("buf");
    #[expect(
        clippy::cast_sign_loss,
        reason = "writeShort is signed; the bytes are the same"
    )]
    wire::write_u16(&mut body, slot as u16).expect("buf");
    #[expect(
        clippy::cast_sign_loss,
        reason = "writeByte is signed; the bytes are the same"
    )]
    wire::write_u8(&mut body, button as u8).expect("buf");
    write_varint(&mut body, input).expect("buf");
    write_varint(&mut body, 0).expect("buf");
    wire::write_u8(&mut body, 0).expect("buf");
    framed(ids::play::sb::CONTAINER_CLICK, &body)
}

fn framed(id: i32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    frame::write_frame(&mut out, None, id, body).expect("buf");
    out
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// Which single table the `Session` is built **without**.
///
/// The cut is applied at the `Session` field and nowhere else. That is the
/// joint: the field going `None` is exactly the state all five old test sites
/// were in, and exactly what a `status.rs` that stopped passing a table would
/// produce. Poisoning a table's *contents* instead would test the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cut {
    Nothing,
    Placement,
    PlacedBy,
    Shapes,
    SoundTypes,
    Ticks,
    Recipes,
    FireOdds,
}

/// Where each of the five placements goes. The player stands at (8, 64, 8).
const STAIR: BlockPos = (10, 65, 10);
const DOOR: BlockPos = (12, 65, 10);
const CORAL: BlockPos = (14, 65, 10);
const TORCH: BlockPos = (16, 65, 10);
const TABLE: BlockPos = (18, 65, 10);
/// The torch's support, dug to make the shape cascade run.
const TORCH_SUPPORT: BlockPos = (16, 64, 10);

/// The wool the flint-and-steel is aimed at, and the position the fire
/// therefore lands in — its **west** neighbour, because the click is on the
/// wool's west face.
///
/// This pair is the only arm here that reaches `FireBlock`'s connection walk.
/// [`FIRE`] has air below it and air below is neither burnable nor
/// sturdy-faced-up, so `getStateForPlacement`'s short-circuit at offsets
/// 20-36 does **not** fire and the five-face walk at 41-131 runs — which is
/// the branch that needs `igniteOdds` and the reason the fire arrives with
/// `east=true` rather than as a bare default state. Sitting the fire on top
/// of something burnable would place fire just as well and would take the
/// short-circuit, so it would not distinguish the walk from its absence.
const WOOL: BlockPos = (20, 65, 10);
const FIRE: BlockPos = (19, 65, 10);

/// Hotbar layout. Slot 0 holds the log because menu slot 37 is inventory
/// slot 0 — see `click::CRAFTING_MENU_HOTBAR_0`.
const HOTBAR: [(usize, &str); 7] = [
    (0, "minecraft:oak_log"),
    (1, "minecraft:oak_stairs"),
    (2, "minecraft:oak_door"),
    (3, "minecraft:tube_coral_block"),
    (4, "minecraft:torch"),
    (5, "minecraft:crafting_table"),
    (6, "minecraft:flint_and_steel"),
];

/// What one run of the packet stream produced.
struct Outcome {
    /// The state at each of the five placement targets, and the door's head.
    stair: String,
    door_head: String,
    coral: String,
    torch: String,
    /// Scheduled ticks pending after the run.
    pending_ticks: usize,
    /// `SOUND` frames on the **observer's** stream. A place sound excludes
    /// the placer, so the actor's stream is the wrong place to look and a
    /// version of this test with one connection would assert on a silence it
    /// caused itself.
    sounds: Vec<(i32, BlockPos)>,
    /// `container_set_slot` frames on the actor's stream, as
    /// `(slot, item-ish bytes non-empty)`.
    result_slot_filled: bool,
    /// `open_screen` frames on the actor's stream — the guard that says the
    /// crafting arm got as far as a menu at all.
    opened_menus: usize,
    /// The state the flint-and-steel left at [`FIRE`].
    fire: String,
}

/// Drive the whole stream through `keep_alive_loop` once.
#[expect(
    clippy::too_many_lines,
    reason = "one Session literal, spelled out; splitting it would hide the joint this file exists to witness"
)]
fn run(cut: Cut) -> Outcome {
    let f = fixture().expect("the caller checked");
    let world = AirWorld::new(f);
    // Supports for the five placements, and the torch's.
    for x in [10, 12, 14, 16, 18] {
        world.edits.set((x, 64, 10), "minecraft:stone");
    }
    // The fire arm's rig: wool at head height on its own stone support, with
    // **nothing under `FIRE`**. See `WOOL`.
    world.edits.set((20, 64, 10), "minecraft:stone");
    world.edits.set(WOOL, "minecraft:white_wool");

    let actor_rec = Recorder::default();
    let observer_rec = Recorder::default();
    let players = rustmc_server::players::Players::new();
    let conn = Arc::new(rustmc_server::players::Connection::with_entity(
        players.next_id(),
        "Actor",
        Box::new(actor_rec.clone()),
        None,
        0,
        2,
        (0, 0),
    ));
    let observer = Arc::new(rustmc_server::players::Connection::with_entity(
        players.next_id(),
        "Observer",
        Box::new(observer_rec.clone()),
        None,
        0,
        2,
        (0, 0),
    ));
    players.join(&conn);
    players.join(&observer);

    // The join's own two steps, which is what puts this connection into the
    // recipient list of `ChunkHolder.broadcastChanges`. `enter_world` writes
    // the spawn blob straight onto the socket and tells the view about it
    // (`mark_sent`); `ChunkMap.updatePlayerStatus(player, true)` then runs
    // `updateChunkTracking`. Without both, `contains` is false or
    // `isPending` is true, and vanilla withholds every block update from a
    // client in that state — which is the behaviour under test, so the setup
    // has to reproduce the join rather than assume it.
    conn.with_chunk_view(|v| {
        for dx in -(2 + 2)..=(2 + 2) {
            for dz in -(2 + 2)..=(2 + 2) {
                v.mark_sent(((0, 0).0 + dx, (0, 0).1 + dz));
            }
        }
        v.update_tracking((0, 0));
    });

    // The join's own two steps, which is what puts this connection into the
    // recipient list of `ChunkHolder.broadcastChanges`. `enter_world` writes
    // the spawn blob straight onto the socket and tells the view about it
    // (`mark_sent`); `ChunkMap.updatePlayerStatus(player, true)` then runs
    // `updateChunkTracking`. Without both, `contains` is false or
    // `isPending` is true, and vanilla withholds every block update from a
    // client in that state — which is the behaviour under test, so the setup
    // has to reproduce the join rather than assume it.
    observer.with_chunk_view(|v| {
        for dx in -(2 + 2)..=(2 + 2) {
            for dz in -(2 + 2)..=(2 + 2) {
                v.mark_sent(((0, 0).0 + dx, (0, 0).1 + dz));
            }
        }
        v.update_tracking((0, 0));
    });

    let clock = rustmc_server::tick::GameClock::new();
    let changed = rustmc_server::broadcast::ChangedBlocks::new();
    let block_events = rustmc_server::containers::BlockEvents::new();
    let openers = rustmc_server::containers::Openers::new();
    // A private queue per run, not `ScheduledTicks::shared()`: `cargo test`
    // runs these on parallel threads in one process and a shared queue would
    // let one run's coral be counted by another's assertion.
    let ticks = rustmc_server::ticks::ScheduledTicks::new();
    let waypoints = rustmc_server::waypoints::Waypoints::new();
    // Within reach of TORCH_SUPPORT at (16, 64, 10): `handleBlockBreakAction`
    // refuses a dig outside `isWithinBlockInteractionRange` before it reads
    // the action, and (8, 64, 8) is 8 blocks short in x. Nothing placed here
    // reads the player's position — stairs and doors face by `yaw`.
    let mut player = rustmc_server::player::PlayerState::at(13.0, 64.0, 8.0);
    player.yaw = 0.0;
    let mut hotbar = rustmc_server::player::Hotbar::new();
    for (slot, item) in HOTBAR {
        hotbar.set(slot, Some(item));
    }
    let mut menu = rustmc_server::menu::MenuState::new();
    let mut known_recipes = std::collections::BTreeSet::new();
    let chunk_at = |_: i32, _: i32| Ok(None);

    let mut session = rustmc_server::world_send::Session {
        conn: &conn,
        player: &mut player,
        clock: &clock,
        clock_ids: &[],
        players: &players,
        blocks: Some(&world),
        hotbar: &mut hotbar,
        menu: &mut menu,
        items: Some(&f.items),
        known_recipes: &mut known_recipes,
        advancements: None,
        adv_engine: None,
        recipe_unlocks: None,
        uses: Some(f.uses),
        item_uses: Some(f.item_uses),
        survive: None,
        bonemeal: None,
        protocol_ids: Some(f.protocol_ids),
        waypoints: &waypoints,
        changed: &changed,
        block_events: &block_events,
        openers: &openers,
        chunk_at: &chunk_at,
        plugins: None,
        creative: true,
        seed: 0,
        // The six. Read off `SessionTables` exactly as `status::listen` reads
        // them, with one nulled when this run is a control.
        placement: (cut != Cut::Placement)
            .then_some(f.tables.placement.as_ref())
            .flatten(),
        placed_by: (cut != Cut::PlacedBy)
            .then_some(f.tables.placed_by)
            .flatten(),
        shapes: (cut != Cut::Shapes).then_some(f.tables.shapes).flatten(),
        sound_types: (cut != Cut::SoundTypes)
            .then_some(f.tables.sound_types)
            .flatten(),
        // NO CUT, AND THE REASON IS THE POINT OF THIS FILE.
        //
        // Every other table here has a cut because nulling it has a
        // consequence this fixture can see: a stair that will not face, a
        // door with no top, a world you cannot hear. Nulling `sound_events`
        // has none — its only effect is the *radius* a sound carries, and
        // every player and every block in this fixture is inside sixteen
        // blocks of every other, where the table-driven and the default
        // radius give the same answer. A `Cut::SoundEvents` here would be a
        // control whose green means nothing, which is the exact defect this
        // whole test exists to refuse. The radius itself is covered where it
        // can go red: `crate::sounds`' own tests place a listener at 20
        // blocks.
        sound_events: f.tables.sound_events,
        ticks: (cut != Cut::Ticks).then_some(&ticks),
        fire_odds: (cut != Cut::FireOdds)
            .then_some(f.tables.fire_odds)
            .flatten(),
        recipes: (cut != Cut::Recipes)
            .then_some(f.tables.recipes.as_ref())
            .flatten(),
    };

    let mut input = Vec::new();
    let mut seq = 0;
    let mut next = || {
        seq += 1;
        seq
    };
    // Five placements, each from its own hotbar slot.
    for (slot, support) in [(1u16, 10), (2, 12), (3, 14), (4, 16), (5, 18)] {
        input.extend(select(slot));
        input.extend(place((support, 64, 10), 1, next()));
    }
    // The fire arm. Face 4 is `Direction.from3DDataValue(4)` = WEST, so
    // `clickedPos.relative(face)` is the wool's west neighbour.
    input.extend(select(6));
    input.extend(place(WOOL, 4, next()));
    // The shape cascade: take the torch's support away and the torch must go
    // with it. `UPDATE_ALL` on the dig is what carries it.
    input.extend(dig(TORCH_SUPPORT, next()));
    // The crafting arm. The table is already down; right-clicking it opens
    // the menu because `useWithoutItem` gets first refusal — the item in hand
    // never reaches the placement path.
    input.extend(place(TABLE, 1, next()));
    // Menu counter: `next_container_counter` from 0. Read back below rather
    // than assumed, so a change to the counter does not silently make these
    // two clicks land on no menu at all.
    input.extend(container_click(1, 1, 37, 0, 0));
    input.extend(container_click(1, 2, 1, 0, 0));

    let mut cursor = std::io::Cursor::new(input);
    rustmc_server::world_send::keep_alive_loop(&mut cursor, &mut session).expect("ends on EOF");
    changed.flush(&players, &world);
    players.flush_acks();

    // Sounds, off the observer's stream, decoded to (event id, position).
    let mut sounds = Vec::new();
    for (id, body) in observer_rec.frames() {
        if id != ids::play::cb::SOUND {
            continue;
        }
        let mut cur = &body[..];
        let Ok(event) = rustmc_proto::varint::read_varint(&mut cur) else {
            continue;
        };
        let _ = rustmc_proto::varint::read_varint(&mut cur);
        let (Ok(x), Ok(y), Ok(z)) = (
            wire::read_i32(&mut cur),
            wire::read_i32(&mut cur),
            wire::read_i32(&mut cur),
        ) else {
            continue;
        };
        // `Eighths::block` is `8 * coord + 4`.
        sounds.push((event - 1, ((x - 4) / 8, (y - 4) / 8, (z - 4) / 8)));
    }

    let actor = actor_rec.frames();
    let opened_menus = actor
        .iter()
        .filter(|(id, _)| *id == ids::play::cb::OPEN_SCREEN)
        .count();
    // The crafting result is menu slot 0. A `container_set_slot` naming slot
    // 0 with a non-empty stack is the recipe having run; the slot is sent as
    // a short after the container id and state id, both varints.
    let result_slot_filled = actor
        .iter()
        .filter(|(id, _)| *id == ids::play::cb::CONTAINER_SET_SLOT)
        .any(|(_, body)| {
            let mut cur = &body[..];
            let _ = rustmc_proto::varint::read_varint(&mut cur);
            let _ = rustmc_proto::varint::read_varint(&mut cur);
            let Ok(slot) = wire::read_u16(&mut cur) else {
                return false;
            };
            // The stack that follows starts with its count as a varint; zero
            // is the empty stack.
            let count = rustmc_proto::varint::read_varint(&mut cur).unwrap_or(0);
            slot == 0 && count > 0
        });

    Outcome {
        stair: world.at(STAIR),
        door_head: world.at((DOOR.0, DOOR.1 + 1, DOOR.2)),
        coral: world.at(CORAL),
        torch: world.at(TORCH),
        pending_ticks: ticks.len(),
        sounds,
        result_slot_filled,
        opened_menus,
        fire: world.at(FIRE),
    }
}

// ---------------------------------------------------------------------------
// The seven arms, wired
// ---------------------------------------------------------------------------

/// The seven observables, each named for the table that produces it.
///
/// Written as one function per arm rather than inline so the `cut_*` tests
/// can assert six of them and deny the seventh without repeating a line.
mod arm {
    use super::{Outcome, CORAL, DOOR, STAIR, TORCH};

    /// `getStateForPlacement`: the stair faces the placer instead of taking
    /// the block's default state.
    pub fn placement(o: &Outcome) -> bool {
        o.stair.starts_with("minecraft:oak_stairs") && o.stair.contains("facing=south")
    }
    /// `setPlacedBy`: the door's upper half exists. Nothing in
    /// `getStateForPlacement` describes it.
    pub fn placed_by(o: &Outcome) -> bool {
        o.door_head.starts_with("minecraft:oak_door") && o.door_head.contains("half=upper")
    }
    /// `updateShape`: the torch went when its support did.
    pub fn shapes(o: &Outcome) -> bool {
        o.torch == "minecraft:air"
    }
    /// `SoundType`: the observer heard the placements.
    pub fn sound_types(o: &Outcome) -> bool {
        let at = |p| o.sounds.iter().any(|(_, q)| *q == p);
        at(STAIR) && at(DOOR) && at(CORAL) && at(TORCH)
    }
    /// The scheduled-tick queue: a coral put down out of water asked to be
    /// woken, and something recorded the request.
    pub fn ticks(o: &Outcome) -> bool {
        o.pending_ticks > 0 && o.coral == "minecraft:tube_coral_block"
    }
    /// The recipes: a log in the grid filled the result slot.
    pub fn recipes(o: &Outcome) -> bool {
        o.result_slot_filled
    }
    /// `FireBlock.igniteOdds`: the flint-and-steel put fire beside the wool
    /// **and the connection walk ran**.
    ///
    /// `east=true` is the assertion that matters. Without the odds this arm
    /// is `minecraft:air` — `can_be_placed_at` refuses rather than guessing —
    /// and a port that placed a bare `defaultBlockState()` here would give
    /// `east=false`, which is a fire in the right place with the wrong five
    /// booleans and is what the refusal existed to prevent.
    pub fn fire_odds(o: &Outcome) -> bool {
        o.fire.starts_with("minecraft:fire") && o.fire.contains("east=true")
    }
}

/// Every arm alive at once, which is the configuration production runs in and
/// the one nothing in this tree had ever built.
#[test]
fn all_seven_tables_reach_the_connection_loop() {
    let Some(_) = fixture() else { return };
    let o = run(Cut::Nothing);
    assert!(
        o.opened_menus > 0,
        "the crafting table never opened, so the recipe arm below would be \
         asserting on a menu that does not exist: {} open_screen frames",
        o.opened_menus
    );
    assert!(arm::placement(&o), "placement: stair is {}", o.stair);
    assert!(
        arm::placed_by(&o),
        "placed_by: door head is {}",
        o.door_head
    );
    assert!(arm::shapes(&o), "shapes: torch is {}", o.torch);
    assert!(
        arm::sound_types(&o),
        "sound_types: observer heard {:?}",
        o.sounds
    );
    assert!(
        arm::ticks(&o),
        "ticks: {} pending, coral is {}",
        o.pending_ticks,
        o.coral
    );
    assert!(arm::recipes(&o), "recipes: result slot never filled");
    assert!(arm::fire_odds(&o), "fire_odds: fire is {}", o.fire);
}

// ---------------------------------------------------------------------------
// The seven controls
// ---------------------------------------------------------------------------

/// Assert that cutting `cut` kills exactly `dead` and leaves the rest alive.
///
/// The second half is the half that matters. "This assertion goes red when I
/// break this table" is cheap; "and no other assertion moves" is what says the
/// six arms are independent and that each one is measuring its own table.
fn only(cut: Cut, dead: fn(&Outcome) -> bool, name: &str) {
    let Some(_) = fixture() else { return };
    let o = run(cut);
    assert!(
        !dead(&o),
        "{name}: cutting {cut:?} left its own arm alive — the assertion is not \
         measuring that table"
    );
    for (other, live) in [
        ("placement", arm::placement as fn(&Outcome) -> bool),
        ("placed_by", arm::placed_by),
        ("shapes", arm::shapes),
        ("sound_types", arm::sound_types),
        ("ticks", arm::ticks),
        ("recipes", arm::recipes),
        ("fire_odds", arm::fire_odds),
    ] {
        if other == name {
            continue;
        }
        assert!(
            live(&o),
            "{name}: cutting {cut:?} also killed {other} — the two arms are not \
             independent, so neither assertion attributes a failure correctly"
        );
    }
}

#[test]
fn cutting_placement_kills_only_the_stairs_facing() {
    only(Cut::Placement, arm::placement, "placement");
}

#[test]
fn cutting_placed_by_kills_only_the_doors_upper_half() {
    only(Cut::PlacedBy, arm::placed_by, "placed_by");
}

#[test]
fn cutting_shapes_kills_only_the_cascade() {
    only(Cut::Shapes, arm::shapes, "shapes");
}

#[test]
fn cutting_sound_types_kills_only_the_sound() {
    only(Cut::SoundTypes, arm::sound_types, "sound_types");
}

#[test]
fn cutting_ticks_kills_only_the_scheduled_tick() {
    only(Cut::Ticks, arm::ticks, "ticks");
}

#[test]
fn cutting_recipes_kills_only_the_crafting_result() {
    only(Cut::Recipes, arm::recipes, "recipes");
}

#[test]
fn cutting_fire_odds_kills_only_the_fire() {
    only(Cut::FireOdds, arm::fire_odds, "fire_odds");
}

/// **The world double answers the item-aware `canBeReplaced`, not the bare
/// props flag — and this is the only thing in this file that can tell the
/// difference.**
///
/// `Blocks::replaceable` grew a `BlockPlaceContext` when `canBeReplaced`
/// became item-aware. Every other assertion in this file places into **air**,
/// and air's flag is `true` with no item to collide with, so all six arms
/// give the same answer whether the double reads the context or throws it
/// away. An impl written as `let _ = ctx; flag` would pass every one of them.
/// That is exactly the shape of the two defects this world has already had —
/// a chest called replaceable, a flag missing `tall_grass` — and both were
/// found by poison rather than by a test, because no test could see them.
///
/// So the discrimination is asserted directly, on the double, against the
/// **real** `placement.json`:
///
/// ```text
///   holding stone      over tall grass  ->  replaceable   (the flag stands)
///   holding tall grass over tall grass  ->  NOT           (!item.is(asItem()))
/// ```
///
/// Nothing here is hardcoded that the extract could contradict. The block's
/// item is looked up *through* the real table rather than spelled out, and
/// the props flag is asserted as a precondition — so a rename in 26.2 makes
/// this fail loudly instead of passing on a lookup that missed, which is the
/// distinction §3 S2's step-0 control is built on.
#[test]
fn the_world_double_answers_the_item_aware_rule_not_the_bare_flag() {
    let Some(f) = fixture() else { return };
    let world = AirWorld::new(f);
    let table = f.tables.placement.as_ref();
    assert!(
        table.is_some(),
        "the real placement.json is a precondition of this file"
    );

    let pos: BlockPos = (8, 70, 8);
    let state = "minecraft:tall_grass[half=lower]";
    world.edits.set(pos, state);
    assert_eq!(world.state_at(pos), state, "the overlay took the write");
    assert!(
        f.props.get(state).is_some_and(|p| p.replaceable),
        "precondition: the no-argument canBeReplaced() is true for {state}, so \
         anything below that answers `false` did so because of the context"
    );

    // `Block.asItem()` out of the extract, never typed: an item name that had
    // drifted would otherwise turn the sharp case into a lookup that missed
    // and passed.
    let its_own_item = table
        .expect("checked above")
        .item_for_block("minecraft:tall_grass")
        .expect("tall grass has an item in the real placement extract");

    let ctx = |held: &'static str| rustmc_server::placement::Replacing {
        item: Some(held),
        face: Face::Up,
        cursor_y: 1.0,
        replacing_clicked: true,
        secondary_use: false,
        table,
    };

    assert!(
        world.replaceable(pos, &ctx("minecraft:stone")),
        "holding something else, the props flag stands and the grass is \
         replaced in place"
    );
    assert!(
        !world.replaceable(pos, &ctx(its_own_item)),
        "holding the block itself, `replaceable && !item.is(asItem())` is \
         false — this is the assertion that fails if the double ignores its \
         context, and the only one in this file that would"
    );
}
