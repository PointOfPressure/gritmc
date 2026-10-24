//! Chunk tickets: what keeps a chunk loaded, and how far the effect reaches.
//!
//! Transcribed from 26.2's `net.minecraft.server.level.TicketType`,
//! `DistanceManager`, `DistanceManager$FixedPlayerDistanceChunkTracker`,
//! `DistanceManager$PlayerTicketTracker`, `LoadingChunkTracker`,
//! `SimulationChunkTracker` and `ChunkTracker`, under
//! `javap -p -c -constants`.
//!
//! # Two independent fields, not one
//!
//! A chunk has a **loading** level and a **simulation** level and they come
//! from different tickets:
//!
//! * `LoadingChunkTracker` sums the tickets whose type `doesLoad()`, and its
//!   answer decides whether the chunk exists at all
//!   ([`crate::chunklevel::is_loaded`]) and whether it is `FULL` and so
//!   sendable;
//! * `SimulationChunkTracker` sums the tickets whose type `doesSimulate()`,
//!   and its answer decides random ticks, block entities and entity ticking.
//!
//! A player adds one of each and they have **different radii**: the loading
//! ticket is spread over the *view* distance by
//! `DistanceManager$PlayerTicketTracker`, the simulation ticket is a single
//! ticket at the player's own chunk whose *level* is lowered by the
//! simulation distance. So the two regions are shaped alike but sized by two
//! different settings, and a server that drives both off `view-distance`
//! simulates chunks vanilla does not.
//!
//! # Levels propagate Chebyshev, one per step
//!
//! `ChunkTracker.computeLevelFromNeighbor` is `level + 1`, and
//! `checkNeighborsAfterUpdate` visits `dx, dz in -1..=1`. So a ticket of
//! level `L` at chunk `c` gives every chunk `d` a level of at most
//! `L + chebyshev(c, d)`, and the level of a chunk is the minimum over every
//! ticket. That is the whole propagation; there is no Euclidean anything in
//! it, which is what makes the ticket regions squares while the *tracking*
//! region ([`crate::trackingview`]) is a padded disc.

use crate::chunklevel::{self, FullChunkStatus, BLOCK_TICKING_LEVEL, ENTITY_TICKING_LEVEL};
use crate::trackingview::ChunkPos;

/// `TicketType.FLAG_PERSIST` — the ticket survives a save/load.
pub const FLAG_PERSIST: i32 = 1;
/// `TicketType.FLAG_LOADING` — the ticket feeds `LoadingChunkTracker`.
pub const FLAG_LOADING: i32 = 2;
/// `TicketType.FLAG_SIMULATION` — the ticket feeds `SimulationChunkTracker`.
pub const FLAG_SIMULATION: i32 = 4;
/// `TicketType.FLAG_KEEP_DIMENSION_ACTIVE`.
pub const FLAG_KEEP_DIMENSION_ACTIVE: i32 = 8;
/// `TicketType.FLAG_CAN_EXPIRE_IF_UNLOADED`.
pub const FLAG_CAN_EXPIRE_IF_UNLOADED: i32 = 16;

/// `TicketType.NO_TIMEOUT`.
pub const NO_TIMEOUT: i64 = 0;

/// A `TicketType` — vanilla's record of `(timeout, flags)`, registered into
/// `BuiltInRegistries.TICKET_TYPE` under an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketType {
    /// The registry id, without the `minecraft:` prefix — the string vanilla
    /// passes to `register`.
    pub id: &'static str,
    /// `TicketType.timeout`, in ticks. Zero is `NO_TIMEOUT`.
    pub timeout: i64,
    /// `TicketType.flags`.
    pub flags: i32,
}

impl TicketType {
    /// `TicketType.persist()`.
    #[must_use]
    pub const fn persist(&self) -> bool {
        self.flags & FLAG_PERSIST != 0
    }
    /// `TicketType.doesLoad()`.
    #[must_use]
    pub const fn does_load(&self) -> bool {
        self.flags & FLAG_LOADING != 0
    }
    /// `TicketType.doesSimulate()`.
    #[must_use]
    pub const fn does_simulate(&self) -> bool {
        self.flags & FLAG_SIMULATION != 0
    }
    /// `TicketType.shouldKeepDimensionActive()`.
    #[must_use]
    pub const fn should_keep_dimension_active(&self) -> bool {
        self.flags & FLAG_KEEP_DIMENSION_ACTIVE != 0
    }
    /// `TicketType.canExpireIfUnloaded()`.
    #[must_use]
    pub const fn can_expire_if_unloaded(&self) -> bool {
        self.flags & FLAG_CAN_EXPIRE_IF_UNLOADED != 0
    }
    /// `TicketType.hasTimeout()`.
    #[must_use]
    pub const fn has_timeout(&self) -> bool {
        self.timeout != NO_TIMEOUT
    }
}

/// `TicketType.PLAYER_SPAWN` — 20 ticks, LOADING.
pub const PLAYER_SPAWN: TicketType = TicketType {
    id: "player_spawn",
    timeout: 20,
    flags: FLAG_LOADING,
};
/// `TicketType.SPAWN_SEARCH` — 1 tick, LOADING.
pub const SPAWN_SEARCH: TicketType = TicketType {
    id: "spawn_search",
    timeout: 1,
    flags: FLAG_LOADING,
};
/// `TicketType.DRAGON` — no timeout, LOADING | SIMULATION.
pub const DRAGON: TicketType = TicketType {
    id: "dragon",
    timeout: NO_TIMEOUT,
    flags: FLAG_LOADING | FLAG_SIMULATION,
};
/// `TicketType.PLAYER_LOADING` — no timeout, LOADING.
///
/// The one `PlayerTicketTracker` spreads over the view distance.
pub const PLAYER_LOADING: TicketType = TicketType {
    id: "player_loading",
    timeout: NO_TIMEOUT,
    flags: FLAG_LOADING,
};
/// `TicketType.PLAYER_SIMULATION` — no timeout, SIMULATION |
/// KEEP_DIMENSION_ACTIVE.
///
/// **Not LOADING.** A player's simulation ticket does not load anything on
/// its own; the loading comes entirely from [`PLAYER_LOADING`].
pub const PLAYER_SIMULATION: TicketType = TicketType {
    id: "player_simulation",
    timeout: NO_TIMEOUT,
    flags: FLAG_SIMULATION | FLAG_KEEP_DIMENSION_ACTIVE,
};
/// `TicketType.FORCED` — no timeout, all of PERSIST | LOADING | SIMULATION |
/// KEEP_DIMENSION_ACTIVE. The `/forceload` ticket.
pub const FORCED: TicketType = TicketType {
    id: "forced",
    timeout: NO_TIMEOUT,
    flags: FLAG_PERSIST | FLAG_LOADING | FLAG_SIMULATION | FLAG_KEEP_DIMENSION_ACTIVE,
};
/// `TicketType.PORTAL` — 300 ticks, and the same four flags as `FORCED`.
pub const PORTAL: TicketType = TicketType {
    id: "portal",
    timeout: 300,
    flags: FLAG_PERSIST | FLAG_LOADING | FLAG_SIMULATION | FLAG_KEEP_DIMENSION_ACTIVE,
};
/// `TicketType.ENDER_PEARL` — 40 ticks, LOADING | SIMULATION |
/// KEEP_DIMENSION_ACTIVE. Not persisted.
pub const ENDER_PEARL: TicketType = TicketType {
    id: "ender_pearl",
    timeout: 40,
    flags: FLAG_LOADING | FLAG_SIMULATION | FLAG_KEEP_DIMENSION_ACTIVE,
};
/// `TicketType.UNKNOWN` — 1 tick, LOADING | CAN_EXPIRE_IF_UNLOADED.
pub const UNKNOWN: TicketType = TicketType {
    id: "unknown",
    timeout: 1,
    flags: FLAG_LOADING | FLAG_CAN_EXPIRE_IF_UNLOADED,
};

/// Every registered `TicketType`, in registration order — which is the
/// order `BuiltInRegistries.TICKET_TYPE` assigns ids in.
pub const ALL: [TicketType; 9] = [
    PLAYER_SPAWN,
    SPAWN_SEARCH,
    DRAGON,
    PLAYER_LOADING,
    PLAYER_SIMULATION,
    FORCED,
    PORTAL,
    ENDER_PEARL,
    UNKNOWN,
];

/// `DistanceManager.PLAYER_TICKET_LEVEL` — `ChunkLevel.byStatus(ENTITY_TICKING)`, 31.
pub const PLAYER_TICKET_LEVEL: i32 = chunklevel::by_status(FullChunkStatus::EntityTicking);

/// `LoadingChunkTracker.MAX_LEVEL` — `ChunkLevel.MAX_LEVEL + 1`, 45.
///
/// The level a chunk with no loading ticket reads at, and one past the last
/// level [`chunklevel::is_loaded`] accepts.
pub const LOADING_UNSET_LEVEL: i32 = chunklevel::MAX_LEVEL + 1;

/// `SimulationChunkTracker.MAX_LEVEL` — 33, and a *literal* there rather
/// than `ChunkLevel.MAX_LEVEL`.
///
/// A chunk with no simulation ticket reads 33, which is neither entity- nor
/// block-ticking. Note this is `FULL_CHUNK_LEVEL` numerically and not by
/// derivation: they are separate constants in separate classes and only one
/// of them moved when the pyramid was introduced.
pub const SIMULATION_UNSET_LEVEL: i32 = 33;

/// `DistanceManager$PlayerTicketTracker`'s maximum distance — the
/// constructor argument in `DistanceManager.<init>`, 32, which is also
/// `ChunkMap.MAX_VIEW_DISTANCE`.
pub const PLAYER_TICKET_TRACKER_MAX_DISTANCE: i32 = 32;

/// `DistanceManager.naturalSpawnChunkCounter`'s maximum distance, 8.
pub const NATURAL_SPAWN_TRACKER_MAX_DISTANCE: i32 = 8;

/// `ChunkMap.MIN_VIEW_DISTANCE`.
pub const MIN_VIEW_DISTANCE: i32 = 2;
/// `ChunkMap.MAX_VIEW_DISTANCE`.
pub const MAX_VIEW_DISTANCE: i32 = 32;

/// `DistanceManager.getPlayerTicketLevel()` — the level of the player's
/// `PLAYER_SIMULATION` ticket.
///
/// ```java
/// return Math.max(0, ChunkLevel.byStatus(ENTITY_TICKING) - this.simulationDistance);
/// ```
///
/// So at the default simulation distance of 10 the ticket is level 21, and
/// the chunk under the player reaches level 21, which is entity-ticking with
/// ten chunks of margin. The `max(0, ..)` binds only above simulation
/// distance 31.
#[must_use]
pub const fn player_ticket_level(simulation_distance: i32) -> i32 {
    let l = PLAYER_TICKET_LEVEL - simulation_distance;
    if l < 0 {
        0
    } else {
        l
    }
}

/// `ChunkMap.getPlayerViewDistance(ServerPlayer)` —
/// `Mth.clamp(player.requestedViewDistance(), 2, serverViewDistance)`.
///
/// **`Mth.clamp(int, int, int)` is `Math.min(Math.max(v, min), max)`** — read
/// off the bytecode, not assumed. The order matters when the server's own
/// ceiling is below the floor of two: the ceiling wins and the answer can be
/// 1 or 0. `ChunkMap.setServerViewDistance` clamps the server value into
/// `2..=32` first, so a running server never asks; written this way anyway,
/// because Rust's `clamp` panics when `min > max` where Java's quietly
/// returns `max`, and an operator setting `view-distance=1` should not take
/// the server down.
#[must_use]
pub const fn player_view_distance(requested: i32, server: i32) -> i32 {
    let floored = if requested > MIN_VIEW_DISTANCE {
        requested
    } else {
        MIN_VIEW_DISTANCE
    };
    if floored < server {
        floored
    } else {
        server
    }
}

/// `ChunkMap.setServerViewDistance`'s clamp — `Mth.clamp(v, 2, 32)`.
#[must_use]
pub const fn server_view_distance(requested: i32) -> i32 {
    if requested < MIN_VIEW_DISTANCE {
        MIN_VIEW_DISTANCE
    } else if requested > MAX_VIEW_DISTANCE {
        MAX_VIEW_DISTANCE
    } else {
        requested
    }
}

/// How far a player's `PLAYER_SIMULATION` ticket reaches at entity-ticking
/// level, in chunks, Chebyshev.
///
/// The ticket sits at the player's own chunk at level
/// `max(0, 31 - simulation_distance)` and levels rise by one per Chebyshev
/// step, so `ChunkLevel.isEntityTicking(level)` — `level <= 31` — holds out
/// to exactly `simulation_distance`. That is the set
/// `DistanceManager.forEachEntityTickingChunk` walks, which is what
/// `ServerChunkCache.tickChunks` random ticks over
/// (see [`crate::randomtick`]'s module doc, which established that the
/// method named `forEachBlockTickingChunk` admits on `isEntityTicking`).
///
/// The `max(0, ..)` floor makes this saturate: above simulation distance 31
/// the ticket is already at level 0 and cannot reach further, so the radius
/// is 31 and not the setting.
#[must_use]
pub const fn entity_ticking_radius(simulation_distance: i32) -> i32 {
    ENTITY_TICKING_LEVEL - player_ticket_level(simulation_distance)
}

/// How far the same ticket reaches at block-ticking level.
///
/// **One further, not the same.** `isBlockTicking` is `level <= 32`, so this
/// is `entity_ticking_radius + 1` for every simulation distance the floor
/// does not bind at. A port that used one number for both would tick a whole
/// ring wrong in one direction or the other, at the right rate everywhere
/// inside it — the error only a count at the boundary can see.
#[must_use]
pub const fn block_ticking_radius(simulation_distance: i32) -> i32 {
    BLOCK_TICKING_LEVEL - player_ticket_level(simulation_distance)
}

/// One placed ticket: a type, a level and a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    /// `Ticket.type`.
    pub ticket_type: TicketType,
    /// `Ticket.ticketLevel`.
    pub level: i32,
    /// Where it was added.
    pub chunk: ChunkPos,
}

/// `ChunkPos` Chebyshev distance — the metric `ChunkTracker` propagates in.
#[must_use]
pub const fn chebyshev(a: ChunkPos, b: ChunkPos) -> i32 {
    let dx = (a.0 - b.0).abs();
    let dz = (a.1 - b.1).abs();
    if dx > dz {
        dx
    } else {
        dz
    }
}

/// The level `LoadingChunkTracker` would settle on for one chunk.
///
/// `min` over every LOADING ticket of `ticket.level + chebyshev`, floored at
/// nothing and ceilinged at [`LOADING_UNSET_LEVEL`] — which is what a chunk
/// no ticket reaches reads as.
#[must_use]
pub fn loading_level(tickets: &[Ticket], chunk: ChunkPos) -> i32 {
    level_from(tickets, chunk, TicketType::does_load, LOADING_UNSET_LEVEL)
}

/// The level `SimulationChunkTracker` would settle on for one chunk.
#[must_use]
pub fn simulation_level(tickets: &[Ticket], chunk: ChunkPos) -> i32 {
    level_from(
        tickets,
        chunk,
        TicketType::does_simulate,
        SIMULATION_UNSET_LEVEL,
    )
}

fn level_from(
    tickets: &[Ticket],
    chunk: ChunkPos,
    admits: impl Fn(&TicketType) -> bool,
    unset: i32,
) -> i32 {
    let mut best = unset;
    for t in tickets {
        if !admits(&t.ticket_type) {
            continue;
        }
        let l = t.level.saturating_add(chebyshev(t.chunk, chunk));
        if l < best {
            best = l;
        }
    }
    best
}

/// The `PLAYER_LOADING` tickets one player produces.
///
/// `DistanceManager$PlayerTicketTracker` gives every chunk a level equal to
/// its Chebyshev distance from the nearest player chunk, and
/// `haveTicketFor(level)` is `level <= viewDistance`, so a `PLAYER_LOADING`
/// ticket at [`PLAYER_TICKET_LEVEL`] sits on **every chunk of the
/// `(2 * view_distance + 1)` square**, not just the centre. That is a square
/// and not the padded disc of [`crate::trackingview`]: the two regions are
/// different shapes and only one of them decides what the client is sent.
///
/// The `view_distance` here is the **server's**, not the player's clamped
/// one: `ChunkMap.setServerViewDistance` calls
/// `DistanceManager.updatePlayerTickets(this.serverViewDistance)`, and
/// nothing else ever sets it. A client that asks for less is sent less and
/// the server still loads the full square.
#[must_use]
pub fn player_loading_tickets(player_chunk: ChunkPos, view_distance: i32) -> Vec<Ticket> {
    let mut out = Vec::new();
    for dx in -view_distance..=view_distance {
        for dz in -view_distance..=view_distance {
            out.push(Ticket {
                ticket_type: PLAYER_LOADING,
                level: PLAYER_TICKET_LEVEL,
                chunk: (player_chunk.0 + dx, player_chunk.1 + dz),
            });
        }
    }
    out
}

/// The one `PLAYER_SIMULATION` ticket a player produces, at their own chunk.
#[must_use]
pub const fn player_simulation_ticket(player_chunk: ChunkPos, simulation_distance: i32) -> Ticket {
    Ticket {
        ticket_type: PLAYER_SIMULATION,
        level: player_ticket_level(simulation_distance),
        chunk: player_chunk,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        block_ticking_radius, chebyshev, entity_ticking_radius, loading_level,
        player_loading_tickets, player_simulation_ticket, player_ticket_level,
        player_view_distance, server_view_distance, simulation_level, ALL, DRAGON, ENDER_PEARL,
        FORCED, LOADING_UNSET_LEVEL, PLAYER_LOADING, PLAYER_SIMULATION, PLAYER_SPAWN,
        PLAYER_TICKET_LEVEL, PORTAL, SIMULATION_UNSET_LEVEL, SPAWN_SEARCH, UNKNOWN,
    };
    use crate::chunklevel::{is_block_ticking, is_entity_ticking, is_loaded};

    /// The registry, verbatim. Nine types, in registration order, each with
    /// the timeout and flag word `TicketType.<clinit>` passes to `register`.
    #[test]
    fn the_nine_ticket_types_match_the_registry() {
        let want: [(&str, i64, i32); 9] = [
            ("player_spawn", 20, 2),
            ("spawn_search", 1, 2),
            ("dragon", 0, 6),
            ("player_loading", 0, 2),
            ("player_simulation", 0, 12),
            ("forced", 0, 15),
            ("portal", 300, 15),
            ("ender_pearl", 40, 14),
            ("unknown", 1, 18),
        ];
        assert_eq!(ALL.len(), want.len());
        for (got, (id, timeout, flags)) in ALL.iter().zip(want) {
            assert_eq!(got.id, id);
            assert_eq!(got.timeout, timeout, "{id} timeout");
            assert_eq!(got.flags, flags, "{id} flags");
        }
    }

    /// The flag word decomposed, for the four types where getting it wrong
    /// changes behaviour rather than bookkeeping.
    #[test]
    fn a_players_simulation_ticket_does_not_load() {
        assert!(!PLAYER_SIMULATION.does_load(), "flags 12 has no bit 2");
        assert!(PLAYER_SIMULATION.does_simulate());
        assert!(PLAYER_LOADING.does_load());
        assert!(
            !PLAYER_LOADING.does_simulate(),
            "flags 2 has no bit 4 — the view distance loads, it does not simulate"
        );
        assert!(FORCED.persist());
        assert!(PORTAL.persist() && PORTAL.has_timeout() && PORTAL.timeout == 300);
        assert!(!ENDER_PEARL.persist() && ENDER_PEARL.timeout == 40);
        assert!(UNKNOWN.can_expire_if_unloaded());
        assert!(DRAGON.does_load() && DRAGON.does_simulate() && !DRAGON.has_timeout());
        assert!(PLAYER_SPAWN.has_timeout() && SPAWN_SEARCH.timeout == 1);
    }

    /// `getPlayerTicketLevel` at the settings a real server runs.
    #[test]
    fn the_simulation_ticket_level_is_thirty_one_minus_the_distance() {
        assert_eq!(PLAYER_TICKET_LEVEL, 31);
        assert_eq!(player_ticket_level(10), 21);
        assert_eq!(player_ticket_level(4), 27);
        assert_eq!(player_ticket_level(31), 0);
        assert_eq!(player_ticket_level(40), 0, "the max(0, ..) binds");
    }

    /// A single player at the origin, at vanilla's defaults: which chunks
    /// tick, which are merely loaded, and where the world stops.
    ///
    /// The three radii are different numbers and this is the test that says
    /// so — a port that used one distance for all three would pass every
    /// value test inside the smallest of them.
    #[test]
    fn one_player_produces_three_different_radii() {
        let view = 10;
        let sim = 10;
        let mut tickets = player_loading_tickets((0, 0), view);
        tickets.push(player_simulation_ticket((0, 0), sim));

        // Entity ticking: the simulation ticket is level 21 at (0,0) and
        // 31 is the ceiling, so it reaches Chebyshev 10 — the simulation
        // distance, exactly.
        assert!(is_entity_ticking(simulation_level(&tickets, (10, 0))));
        assert!(!is_entity_ticking(simulation_level(&tickets, (11, 0))));
        // Block ticking gets one more ring: level 32.
        assert!(is_block_ticking(simulation_level(&tickets, (11, 0))));
        assert!(!is_block_ticking(simulation_level(&tickets, (12, 0))));
        // Loading: the PLAYER_LOADING square is level 31 out to Chebyshev
        // 10, and `isLoaded` accepts up to 44, so the loaded region runs to
        // 10 + 13 = 23.
        assert_eq!(loading_level(&tickets, (10, 10)), 31);
        assert_eq!(loading_level(&tickets, (23, 0)), 44);
        assert!(is_loaded(loading_level(&tickets, (23, 0))));
        assert!(!is_loaded(loading_level(&tickets, (24, 0))));
        assert_eq!(loading_level(&tickets, (100, 100)), LOADING_UNSET_LEVEL);
        assert_eq!(
            simulation_level(&tickets, (100, 100)),
            SIMULATION_UNSET_LEVEL
        );
    }

    /// The loading square really is a square: the corner and the axis are at
    /// the same level, which a Euclidean propagation would not give.
    #[test]
    fn ticket_levels_propagate_chebyshev() {
        assert_eq!(chebyshev((0, 0), (3, 3)), 3);
        assert_eq!(chebyshev((0, 0), (3, 0)), 3);
        let t = vec![super::Ticket {
            ticket_type: PLAYER_LOADING,
            level: 31,
            chunk: (0, 0),
        }];
        assert_eq!(loading_level(&t, (5, 5)), 36);
        assert_eq!(loading_level(&t, (5, 0)), 36, "corner and axis agree");
    }

    /// The two simulation radii, derived rather than written down: they are
    /// `ENTITY_TICKING_LEVEL - ticketLevel` and
    /// `BLOCK_TICKING_LEVEL - ticketLevel`, and the second is always one
    /// larger until the `max(0, ..)` floor binds.
    #[test]
    fn the_two_simulation_radii_differ_by_exactly_one_until_the_floor_binds() {
        assert_eq!(entity_ticking_radius(10), 10);
        assert_eq!(block_ticking_radius(10), 11);
        assert_eq!(entity_ticking_radius(4), 4);
        assert_eq!(block_ticking_radius(4), 5);
        for sim in 0..=31 {
            assert_eq!(entity_ticking_radius(sim), sim, "sim {sim}");
            assert_eq!(block_ticking_radius(sim), sim + 1, "sim {sim}");
        }
        assert_eq!(
            entity_ticking_radius(40),
            31,
            "the ticket is already at level 0 and cannot reach further"
        );
        assert_eq!(block_ticking_radius(40), 32);
        // And the radii agree with the level arithmetic they are shorthand
        // for, which is the thing that would break if either constant moved.
        let t = vec![player_simulation_ticket((0, 0), 10)];
        for r in 0..14 {
            assert_eq!(
                is_entity_ticking(simulation_level(&t, (r, 0))),
                r <= entity_ticking_radius(10),
                "entity ticking at {r}"
            );
            assert_eq!(
                is_block_ticking(simulation_level(&t, (r, 0))),
                r <= block_ticking_radius(10),
                "block ticking at {r}"
            );
        }
    }

    /// The tracked square's size, so a change to either the loop bounds or
    /// the metric moves a number.
    #[test]
    fn the_loading_square_is_two_r_plus_one_squared() {
        assert_eq!(player_loading_tickets((0, 0), 10).len(), 441);
        assert_eq!(player_loading_tickets((0, 0), 2).len(), 25);
        assert_eq!(player_loading_tickets((0, 0), 0).len(), 1);
    }

    /// `Mth.clamp`'s asymmetry, kept deliberately: the server ceiling beats
    /// the floor of two. `setServerViewDistance` is what makes it
    /// unreachable, and it is tested here so that removing that clamp is
    /// visible rather than silent.
    #[test]
    fn the_view_distance_clamps_are_vanillas_own() {
        assert_eq!(player_view_distance(10, 4), 4);
        assert_eq!(player_view_distance(3, 10), 3);
        assert_eq!(player_view_distance(1, 10), 2);
        assert_eq!(player_view_distance(0, 10), 2);
        assert_eq!(player_view_distance(-1, 10), 2, "the byte is signed");
        assert_eq!(
            player_view_distance(10, 1),
            1,
            "Mth.clamp is min(max(v, 2), server), so the ceiling wins"
        );
        assert_eq!(
            player_view_distance(0, 1),
            1,
            "and it wins over the floor too: min(max(0, 2), 1) = 1"
        );
        assert_eq!(
            server_view_distance(1),
            2,
            "and the server value is clamped"
        );
        assert_eq!(server_view_distance(99), 32);
        assert_eq!(server_view_distance(10), 10);
    }
}
