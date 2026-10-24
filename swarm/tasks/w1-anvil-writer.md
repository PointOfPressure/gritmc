# Task: Anvil writer — vanilla-loadable region output (wave 1)

## Standing rules (stable prefix — do not edit per-task)

You are one of 3 worker agents building rustmc, a Minecraft-compatible
server in Rust. Non-negotiable rules:

1. You work ONLY in this worktree, on the current branch. Never touch
   `main`, other worktrees, `~/minecom`, or `vanilla-src/`.
2. Before every commit: `ci/t0.sh` and `ci/t1.sh` must pass. Never commit red.
3. Frozen interfaces (marked FROZEN in crate docs) are read-only. If your
   task cannot be done without changing one, STOP and write an escalation
   (see report format) — a half-done interface edit costs more than a stall.
4. Port behavior from the cited reference classes; never copy code. Cite the
   class in a comment at the porting site, Minecom-style.
5. No `static mut`, no ambient singletons, no HashMap iteration at
   behavior-visible sites (ordered containers there), no tolerance-widening
   in tests, no fast-math.
6. Assert invariants, never instantaneous cross-thread state.
7. If a test fails twice for the same reason you don't understand, stop and
   escalate with the evidence. Do not attempt a third blind fix.
8. Deliverables must be cherry-pickable: docs/log entries committed alone;
   code in separate commits.

## Report format (final message — this is all the orchestrator reads)

- LANDED: commits + one-line what/why each
- EVIDENCE: gate output tails proving green (T0/T1 + task-specific)
- ESCALATED: blocked items, each with mechanism/evidence/blocked-on
- NOT TOUCHED: confirm main/other-trees/minecom untouched

## Read these sections (nothing else)

- crates/rustmc-world/src/nbt.rs + anvil.rs (the read side you're
  mirroring; ChunkAccess/Canvas in lib.rs are FROZEN)
- docs/ORACLE.md §3 (genregions contract + the 26.x layout note in the
  IN-REPO banner)
- ci/gen-vanilla-region.sh + ci/t2-regiondiff.sh (harness patterns)

## The task

1. NBT writer in rustmc-world::nbt (mirror of the reader; compound keys
   written in BTreeMap order — deterministic output is a requirement, not
   a nicety).
2. `anvil::write_region` for real: paletted sections (blocks min 4 bits,
   biomes min 1, non-spanning packing), zlib chunks, sector table. Output
   must be LOADABLE BY VANILLA, not byte-identical to vanilla's own files
   (compression details may differ; semantics may not).
3. In-memory chunk model: implement ChunkAccess (FROZEN — including
   biome/set_biome/light) for a ProtoChunk type here or reuse/extend an
   existing one; write_region consumes it.

Gate: new `ci/t2-anvilwrite.sh`, two provable properties:
  a. Round-trip: read a vanilla-generated region dir (produce with
     ci/gen-vanilla-region.sh), rebuild each chunk through the in-memory
     model, write to a fresh dir, re-read, region-differ diff original vs
     rewritten → EXACTNESS 100.000000%.
  b. Vanilla-loadable: place the rewritten region under a prepared world
     dir (copy level.dat from the source world), boot the REAL vanilla
     server on it (ci/vanilla-server.sh), assert clean "Done" boot and an
     oracle-runner join — vanilla accepting our bytes is the gate that
     matters for G3's future (rustmc --genregions output must satisfy the
     same reader).
