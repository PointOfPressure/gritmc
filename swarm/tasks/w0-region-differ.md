# Task: In-repo Anvil region differ v0 (oracle self-containment, PHASE0 P0-11)

> **DONE — built by the orchestrator pre-drop (2026-07-22), kept as spec
> reference.** Delivered: rustmc-world nbt+anvil (read), tools/region-differ
> scan/diff (--bbox/--json), ci/gen-vanilla-region.sh (≤r7),
> ci/t2-regiondiff.sh. 26.x layout: overworld regions live under
> world/dimensions/minecraft/overworld/region. Do not run this task.
> Still open for a worker later: r18 row-batching, nether dimension arg.

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
8. Deliverables must be cherry-pickable: docs/log commits separate from code
   commits.

## Report format (final message — this is all the orchestrator reads)

- LANDED: commits + one-line what/why each
- EVIDENCE: gate output tails proving green (T0/T1 + task-specific)
- ESCALATED: blocked items, each with mechanism/evidence/blocked-on
- NOT TOUCHED: confirm main/other-trees/minecom untouched

## Read these sections (nothing else)

- docs/ORACLE.md — top AMENDED block + §3 (region-diff contract, the
  genregions argv shape and the ratchet numbers)
- docs/CRATES.md — rustmc-world responsibility line
- crates/rustmc-world/src/lib.rs (whole file — note the anvil module stub)

## The task

Build `tools/region-differ` v0: a Rust CLI that bit-compares two Anvil
region directories, replacing the minecom-side Python script for rustmc.

1. `region-differ diff <dir-a> <dir-b>` — for every r.X.Z.mca present in
   either dir: parse chunks, compare every block's FULL block state, plus
   biome cells. Output: total blocks, mismatches, exactness %, per-chunk
   worst offenders (top 10), machine-readable JSON with --json.
2. `region-differ gen-vanilla <seed> <radius> <cx> <cz> <dim> <out-dir>` —
   drive a real vanilla 26.2 server (reuse ci/vanilla-server.sh from the
   wire-harness task if it exists; else write it) to generate the square
   and copy its region files to out-dir. This produces the reference side.
3. The Anvil PARSER goes in `crates/rustmc-world` (anvil module — reading
   is not the frozen part; the ChunkAccess trait is read-only to you).
   The differ tool consumes it. Parser gets unit tests against a tiny
   committed fixture region (generate one, strip to <1MB, it is
   rustmc-generated data not Mojang assets — verify licensing stance in
   ORACLE.md top block before committing; if in doubt, fixture stays in
   /oracle-work/ and the test is gated on its presence).
4. Gate: `ci/t2-regiondiff.sh` — gen-vanilla twice with the same seed into
   two dirs, diff them, assert 100.000000% (vanilla is deterministic;
   this proves the parser+differ end-to-end before rustmc has worldgen).

Numbers context (do NOT chase these yet, wave 2 does): the inherited
ratchet starts at overworld r18 = 99.381792%, nether = 99.419259%.
