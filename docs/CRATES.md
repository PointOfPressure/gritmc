# CRATES — workspace layout & interface freeze

Cargo workspace, one repo. Design goals in order: (1) crate boundaries =
swarm-agent boundaries (one agent per crate, no collisions), (2) regionized
parallelism is an architecture property from day 0, not a retrofit, (3) the
plugin API is a real public surface from the first commit.

## Workspace

```
rustmc/
├── crates/
│   ├── rustmc-proto        # wire protocol
│   ├── rustmc-world        # chunk/palette/Anvil storage
│   ├── rustmc-worldgen     # terrain + decoration
│   ├── rustmc-entity       # ECS entity storage + systems
│   ├── rustmc-behavior     # block/item/mob gameplay rules
│   ├── rustmc-server       # tick loop, regions, orchestration, CLI
│   ├── rustmc-api          # the public plugin API (the product surface)
│   └── rustmc-data         # extracted-vanilla-data loading (registries, tags, loot)
├── plugins/
│   ├── essentials-core     # homes/warps/kits/tpa/spawn/msg
│   ├── permissions         # groups, nodes, inheritance, contexts
│   └── worldedit-core      # selection, set/copy/paste, schematics
└── tools/
    ├── oracle-runner       # drives Minecom PlayTest-external + region-diff (ORACLE.md)
    └── goldengen           # regenerates golden vectors from the JVM side
```

## Crate responsibilities (one line each, enforced)

- **rustmc-proto** — packet codecs for the pinned protocol version, framing,
  compression, encryption; zero gameplay knowledge. (Serde-style derive on
  packet structs; the version bump surface lives here alone.)
- **rustmc-world** — chunk representation (paletted sections), heightmaps,
  lighting storage, Anvil read/write byte-compatible with vanilla (this is
  what region-diff G3 sees); no behavior.
- **rustmc-worldgen** — noise, terrain shaping, carving, surface rules,
  features/decoration on the shared-canvas defined-order model
  (ORACLE.md §6 contract is binding here); pure function of (seed, pos,
  data); depends on world + data only.
- **rustmc-entity** — ECS storage (archetype or sparse-set — implementer's
  call, benchmarked), spatial index, per-region entity ownership; systems are
  registered by behavior/server, not defined here.
- **rustmc-behavior** — the boring 80%: block interactions, item use, mob AI,
  redstone, combat, status effects; the crate the swarm grinds on longest;
  organized by vanilla's own taxonomy (one module per mechanic family, cites
  decompiled reference classes like Minecom does).
- **rustmc-server** — owns `main`, config, tick scheduler, region executor,
  login/admission, and the `genregions`/`--target-external`-compatible CLI
  contracts from ORACLE.md; the only crate allowed to wire everything
  together.
- **rustmc-api** — traits + event types plugins compile against; versioned
  semver from 0.1.0; NOTHING in plugins/ may reach around it (enforced: the
  three first-party plugins depend on rustmc-api *only*).
- **rustmc-data** — loads the same 1,486 extracted JSON files Minecom's
  extractor produces (registries, tags, loot tables, worldgen configs);
  regeneration stays a Python tool run against the vanilla jar. It also holds
  `java_hash_set`, **by layering rather than by category**, and the exception
  is named rather than hidden: it is a JDK primitive, not extracted data, but
  two unrelated things need `java.util.HashMap` iteration order from opposite
  sides of the graph — `rustmc-worldgen` because `TreeFeature.place` collects
  into `Sets.newHashSet()` and what comes out is behaviour, `rustmc-world`'s
  NBT writer because `CompoundTag` wraps a `HashMap` and vanilla writes
  `keySet()` order. It lived in `rustmc-worldgen` until 2026-08-05 and could
  not be called from `nbt.rs` without a cycle. This crate is the bottom, so
  the move adds no edge and needs none later.

## The frozen-interface rule (day-0 gate for the swarm)

**Before fan-out, the inter-crate traits are written, reviewed by the
orchestrator, and frozen** — `Canvas`, `ChunkAccess`, `EntityView`,
`BlockBehavior`, `Event`, `Scheduler`, the region message enum.

> **FROZEN 2026-07-22 (P0-6 review complete).** Orchestrator second pass
> against Minecom call-site reality + the perf ceiling (PRODUCT.md: 1GB/
> 1vCPU floor, regionized multicore) + the WASM plugin lens. Amendments
> landed pre-freeze: `ChunkAccess` gained `biome`/`set_biome`/`light`
> (worldgen writes biomes into Anvil+wire, spawn rules read biome+light —
> the #1 would-be day-3 escalation), `EntityView` gained `kind` (target
> selection/spawn caps/plugins), `rustmc-data` gained `BiomeId`/
> `EntityKindId`/`LightKind` (extracted-registry id order, same rule as
> `BlockStateId`). Everything else confirmed as drafted.

After freeze:

- **Additive defaulted methods are NOT an interface change.** Adding a
  method with a default body to a frozen trait (perf bulk ops: section
  fills, `for_each_near`-style allocation-free iteration) needs no
  escalation and no queue freeze — it breaks no implementor. Changing or
  removing anything, or adding a required method, remains a freeze-level
  escalation.
- **WASM-boundary rule (binding, from PRODUCT.md §3):** every public type
  crossing `rustmc-api` (event fields, method params/returns) stays plain
  serializable data — no borrowed server internals, no lifetimes in API
  types, closures only in the native-tier registration path. The WASM
  sandbox tier maps this same semantic surface onto an ABI in wave 4+;
  any API addition that couldn't cross an FFI boundary is rejected in
  review.

- An agent may change its crate's *internals* freely.
- An interface change is an **escalation to the orchestrator** (merge-train
  arbiter), never a unilateral edit — one interface edit invalidates N other
  agents' in-flight work; that's the swarm's most expensive failure mode.
- Interfaces ship with doc-tests showing intended use, so 20 agents read one
  canonical example instead of inventing 20 idioms.

## Regionization constraints (bind every crate from day 0)

The 5–20× multiplier is architecture. Rules that make it reachable without a
rewrite:

1. **No global mutable state.** No `static mut`, no ambient singletons; state
   lives in the region it belongs to or in explicitly-shared read-only data
   (`Arc<Registries>`). The compiler is the auditor — `Send`/`Sync` bounds on
   region state are part of the frozen interfaces.
2. **Cross-region effects are messages, not calls.** Anything that touches a
   block/entity outside the current region enqueues a typed message applied
   at the next tick boundary (teleports, portal travel, chunk-border redstone,
   explosion spill). The message enum is a frozen interface. Single-threaded
   execution of the same message model is the launch configuration —
   correctness first, the executor goes multi-core later without semantic
   change.
3. **Update-order-visible mechanics stay serial within a region** (redstone
   update order is observable behavior; parity forbids parallelizing it).
   Region boundaries are the parallelism seam, exactly like the message model
   implies.
4. **Worldgen is region-independent by the ORACLE §6 purity contract** — it
   parallelizes across chunks freely because output can't depend on order;
   decoration canonical order is enforced *within* the generation pipeline,
   not by the executor.
5. **The tick loop is deterministic given inputs**: fixed system order,
   ordered containers at behavior-visible iteration sites, seeded per-tick
   RNG streams. (This also makes future replay/rollback testing possible —
   free gift, don't design it away.)

## Plugin API stance (extraction, not speculation)

- The API grows **only** by demand from the three first-party plugins during
  the sprint, then freezes for launch as 0.x. Every method a plugin didn't
  need is a method we maintain forever for no one.
- Packet-level access is a first-class API citizen (kills the ProtocolLib
  class of workaround); economy/placeholder interfaces are API types from
  day 0 (kills Vault/PAPI); multi-instance worlds are core (kills
  Multiverse).
- API stability promise at launch: 0.x = "may break with a minor, changelog
  guaranteed"; 1.0 only after the API survives a real Mojang version bump
  untouched.
