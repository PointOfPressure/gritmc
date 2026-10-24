# Task: rustmc-data — real registry/block-state loading (wave 1)

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

- crates/rustmc-data/src/lib.rs (BlockStateId/BiomeId doc comments carry
  the id-order rule — THE correctness constraint of this task)
- data/README.md (what's in data/vanilla/, how to regenerate)
- oracle-work/vanilla-26.2/generated/reports/ — blocks.json +
  registries.json are Mojang's own ground truth for id assignment
  (regenerate with `ci/vanilla-server.sh reports` if absent)

## The task

Make `Registries::load(dir)` real:

1. Parse the Mojang blocks report (blocks.json): every block, every state,
   properties → **global BlockStateId assignment identical to vanilla's**
   (the report carries explicit state ids — use them, never re-derive).
   API: state id ⇄ canonical string (`name[k=v,...]`, sorted props —
   match rustmc-world's anvil::canonical_block_state spelling exactly),
   default state per block.
2. Registry id maps (registries.json): biomes, entity types, items →
   BiomeId/EntityKindId/(ItemId newtype if needed internally).
3. Loading is fallible with real errors (missing file, bad JSON), fast
   (< 2s laptop), and allocates once (no per-lookup allocation;
   regionization rule: `Arc<Registries>` is shared read-only).

Gate `ci/t1` (unit tests in the crate): total block-state count matches the
report; 10 spot vectors across the id space (first, last, air, stone, a
multi-property stair state) assert exact ids + canonical strings; biome
count + minecraft:plains id match registries.json. Cite the report file
as the source in test comments. These vectors are the foundation the
Anvil writer and worldgen both stand on — precision here is everything.
