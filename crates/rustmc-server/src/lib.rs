//! rustmc-server — owns main, config, tick scheduler, region executor,
//! login/admission, and the oracle-facing CLI contracts (ORACLE.md).
//! The only crate allowed to wire everything together.
//!
//! # An example, and why this crate has one
//!
//! This crate's doctest target ran **zero** examples on every gate this
//! project has ever run (`ci/checks/test-targets.toml`, 2026-08-16), and it
//! is the crate that wires everything together. The example is the player-edit
//! overlay, because it is where a coordinate is turned into a chunk and that
//! conversion has one classic way of being wrong:
//!
//! ```
//! use rustmc_server::edits::BlockEdits;
//!
//! let edits = BlockEdits::new();
//! edits.set((-1, 64, 16), "minecraft:stone");
//!
//! assert_eq!(edits.get((-1, 64, 16)).as_deref(), Some("minecraft:stone"));
//! assert_eq!(edits.get((0, 64, 16)), None);
//!
//! // `-1 >> 4` is -1, not 0. A truncating divide would file this edit under
//! // chunk (0, 1), and a reload would look for it in a chunk that never had
//! // it — one block of error at every negative chunk border.
//! assert!(edits.touched(-1, 1));
//! assert!(!edits.touched(0, 1));
//! assert_eq!(edits.edited_chunks(), vec![(-1, 1)]);
//! ```

pub mod advancements;
pub mod anvil;
pub mod attack;
pub mod attributes;
pub mod autosave;
pub mod beacon;
pub mod behaviour;
pub mod bell;
pub mod blockarg;
pub mod blockentity;
pub mod blockitem;
pub mod blocktags;
pub mod boat;
pub mod bonemeal;
pub mod bookshelf;
pub mod bordercmd;
pub mod bossbar;
pub mod breath;
pub mod brewing;
pub mod brightness;
pub mod broadcast;
pub mod bucket;
pub mod campfire;
pub mod cauldron;
pub mod chat;
pub mod chunk;
pub mod chunklevel;
pub mod chunksender;
pub mod click;
pub mod cmdexec;
pub mod cmdreader;
pub mod collide;
pub mod combat_rules;
pub mod combat_tracker;
pub mod commands;
pub mod commandtree;
pub mod comparator;
pub mod componentarg;
pub mod conduit;
pub mod configuration;
pub mod console;
pub mod containers;
pub mod cooking;
pub mod crafter;
pub mod damage_types;
pub mod damagecmd;
pub mod death;
pub mod decoratedpot;
pub mod dig;
pub mod dimension;
pub mod dispenser;
pub mod dropitem;
pub mod durability;
pub mod edits;
pub mod effects;
pub mod enchanting;
pub mod enchantment;
pub mod entities;
pub mod experience;
pub mod explosion;
pub mod fall;
pub mod fallable;
pub mod falldamage;
pub mod fire;
pub mod flat;
pub mod fluid;
pub mod food;
pub mod fuel;
pub mod gamerules;
pub mod givecmd;
pub mod golem;
pub mod growing;
pub mod growth;
pub mod health;
pub mod hopper;
pub mod hurt;
pub mod inhabited;
pub mod interact;
pub mod item;
pub mod item_damage;
pub mod itementity;
pub mod itemprops;
pub mod itemsurvive;
pub mod itemtags;
pub mod itemuse;
pub mod javamath;
pub mod jukebox;
pub mod lectern;
pub mod level_events;
pub mod levelstore;
pub mod light;
pub mod light_engine;
pub mod light_incremental;
pub mod light_queue;
pub mod login;
pub mod loot;
pub mod lootroll;
pub mod menu;
pub mod menuslots;
pub mod mining;
pub mod mob;
pub mod move_player;
pub mod movement;
pub mod movement_props;
pub mod movestats;
pub mod noteblock;
pub mod obstruct;
pub mod online;
pub mod openhash;
pub mod ops;
pub mod particles;
pub mod patch;
pub mod path_facts;
pub mod peaceful;
pub mod physics;
pub mod pickup;
pub mod piston;
pub mod pistonmove;
pub mod placedby;
pub mod placement;
pub mod play;
pub mod player;
pub mod playerdeath;
pub mod playerlist;
pub mod playermove;
pub mod players;
pub mod playersync;
pub mod playertick;
pub mod plugins;
pub mod portal;
pub mod potions;
pub mod power;
pub mod projectile;
pub mod properties;
pub mod protocol_ids;
pub mod pvp;
pub mod rails;
pub mod randomtick;
pub mod recipe_book;
pub mod recipes;
pub mod removal;
pub mod respawn;
pub mod sabotage;
pub mod scoreboard;
pub mod scoreboardcmd;
pub mod sculk;
pub mod session_tables;
pub mod shapes;
pub mod shortset;
pub mod shutdown;
pub mod sign;
pub mod sky;
pub mod sleep;
pub mod sound_events;
pub mod sound_types;
pub mod sounds;
pub mod spawner;
pub mod status;
pub mod step;
pub mod structure_entities;
pub mod synched;
pub mod terrain;
pub mod tick;
pub mod tickets;
pub mod ticks;
pub mod tiles;
pub mod tracker;
pub mod trackingview;
pub mod useblock;
pub mod useitemon;
pub mod villager;
pub mod wall;
pub mod waypoints;
pub mod weather;
pub mod wire;
pub mod workstation;
pub mod world;
pub mod world_send;
pub mod worldborder;
pub mod xporb;

use rustmc_data::{BlockPos, Dimension};
use rustmc_entity::EntityId;

/// FROZEN INTERFACE: the region message enum — regionization rule 2
/// (CRATES.md): anything that touches a block/entity outside the current
/// region enqueues one of these, applied at the next tick boundary.
///
/// Launch configuration executes this same model single-threaded
/// (correctness first); the multi-core executor changes scheduling, never
/// semantics. Extending this enum is an interface change → orchestrator
/// escalation, queue freeze.
#[derive(Debug)]
#[non_exhaustive]
pub enum RegionMessage {
    /// Entity crosses a region/dimension boundary (walk, teleport, portal).
    EntityTransfer {
        entity: EntityId,
        to_dimension: Dimension,
        to_pos: (f64, f64, f64),
    },
    /// Block update spilling across a region border (chunk-border redstone).
    BlockUpdateSpill { pos: BlockPos, from: BlockPos },
    /// Explosion damage crossing a border.
    ExplosionSpill { center: (f64, f64, f64), power: f32 },
}

/// FROZEN INTERFACE: tick scheduling as behavior code sees it. Fixed system
/// order; deterministic given inputs (ORACLE.md §6 / CRATES.md rule 5).
pub trait Scheduler {
    /// Current game tick (20/s nominal).
    fn tick(&self) -> u64;

    /// Run `f` after `delay_ticks`, in deterministic registration order
    /// within the target tick.
    fn schedule(&mut self, delay_ticks: u32, f: Box<dyn FnOnce() + Send>);

    /// Enqueue a cross-region message for the next tick boundary.
    fn send(&mut self, msg: RegionMessage);
}

pub mod cli {
    //! CLI contracts pinned by ORACLE.md — these exact shapes are what the
    //! oracle harness drives; changing them breaks the gates.
    //!
    //! `rustmc --genregions <seed> <radius> <cx> <cz> <overworld|nether>`
    //! (positional; cwd = work dir — the exact argv worldgen_region_diff.py
    //! appends via --command, verified 2026-07-21) writes standard Anvil
    //! region/*.mca in the vanilla layout (G3).
    //!
    //! `rustmc run --port <p> ...` serves the pinned protocol version (G2,
    //! PlayTest-external drives bots against it).
}
