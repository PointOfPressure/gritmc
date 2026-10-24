# Task: Standalone JVM golden dumper v0 (oracle self-containment, PHASE0 P0-11)

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

- docs/ORACLE.md — top AMENDED block + §5 (RNG exactness rules)
- crates/rustmc-worldgen/golden/README.md, crates/rustmc-data/golden/
  README.md, crates/rustmc-behavior/golden/README.md (what the 19 existing
  vectors are and how they were originally produced)
- tools/goldengen/ (current stub)

## The task

Make golden-vector regeneration fully self-contained: `tools/goldengen`
becomes a small standalone Java program (single Main.java + a build script
or minimal gradle, whichever is less machinery) that regenerates every
existing golden JSON WITHOUT any Minecom checkout:

1. RNG streams: Xoroshiro128++ and legacy LCG (java.util.Random), first
   10k outputs for the 5 committed seeds, both families, plus the derived/
   fork paths the existing files carry — implement against the JDK +
   ported vanilla seeding logic, citing the reference classes (rule 4).
2. Data-table vectors (map colors, difficulty/enchant-cost/beacon/conduit,
   sulfur archetypes): regenerate from the extracted data files in
   data/vanilla/ (present locally, gitignored) + cited reference logic.
3. `tools/goldengen/regen.sh` runs the whole thing and rewrites
   crates/*/golden/*.json in place.
4. Gate: regen output is byte-identical to the committed vectors
   (`git diff --exit-code crates/*/golden/` after regen) — proving the
   standalone dumper reproduces what the Minecom-derived originals said.
   If any file cannot be reproduced without Minecom internals, do NOT
   fudge it: escalate with the specific file and the missing mechanism.
5. Update the three golden READMEs: regen command is now the in-repo one.
