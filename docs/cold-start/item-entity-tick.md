# Cold start — branch `item-entity-tick`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `item-entity-tick`, off `main` (`35b3bde5`), 2026-08-09

**The item was `ItemEntity.tick`. It is retired by measurement and no server
code was written.** Worktree `~/grit/work-item`, remote dir `~/rustmc-item`.
Not merged — the orchestrator gates. Full account in `docs/PARITY-ROADMAP.md`
§16.

**WHAT CLOSED.**

* **`605`/`606` are NOT unstable, measured at N = 6 under the shipping
  comparator.** Both removed from `ci/checks/gameplay-unstable.toml`, which is
  now 7 unstable + 0 harness and whose declared set matches the measured floor
  name for name. The ⚠ that file carried — "the retirement's own evidence is
  weaker than the declaration's" — is answered in the strong direction.
* **§14's closing sentence is refuted on both halves.** `605`/`606`'s strict
  residual is *not* `ItemEntity.tick`, and it is not reachable at all: vanilla
  fails its own strict projection there on **15 of 15 pairs**, first divergence
  at index 19, kind `set_entity_motion`, every single pair. The velocity is
  four `nextFloat()` draws in `LivingEntity.createItemStackToDrop` (offsets
  190, 207, 254, 263) spent **before the entity ticks**, so every later
  `move_entity` inherits a random input. No repair to any tick can convert
  these two under strict.

**THE NUMBERS.** `ci/corpus-floor.sh --id yrot6-20260809`, six independent cold
vanilla boots that carry `y_rot` (`~/rustmc-throw`'s three selfcheck runs plus
the corpus `vanilla.json` of `~/rustmc-throw`, `~/rustmc-gate`,
`~/rustmc-look`), corpus 88, 88 common, 0 dropped, 15 pairs, Threadripper.

```
  declared  605/606  14/15 and 15/15 (twin-homog)  ->  0/15 and 0/15, 1 distinct
  strict    605/606  15/15 pairs differ, 6 distinct projections in 6 runs
  CORPUS FLOOR  9/88 -> 7/88, the two names being exactly the difference
```

Power, not assertion: at 14/15, five comparisons miss with probability
1.32e-06; at 15/15, with probability 0.

**NULLS — DO NOT RE-RUN THESE.**

* **Do not re-measure `605`/`606` over `twin-homog-20260809`.** Those
  recordings predate `y_rot` as a named field, so `exempt_item_spawn_yaw` is
  inert over them; it reproduces 14/15 and 15/15 and says nothing about the
  comparator that ships. The instruction previously left in
  `gameplay-unstable.toml` named that ensemble and was wrong to.
* **Do not hunt an `ItemEntity.tick` defect through `605`, `606`, `500` or
  `350`.** All four observe the drop velocity; vanilla disagrees with itself on
  it; the disagreement is upstream of the tick.
* **A strict floor needs no new `Lens`.** `project_trace` already writes a
  `strict` array into every ensemble record and `ci/corpus-floor.py` simply
  never reads it. Add `--projection strict` to `measure`, not a lens variant.
* **`ci/checks/gameplay-floor.toml` was deliberately NOT re-based.** It is
  stale under this comparator in four rows that are other branches'
  measurements (`540` 8 vs 5, crafting table 8 vs 9, offhand 8 vs 9,
  `water-spreads` 14 vs 15). That is §15.4's item and wants a homogeneously
  re-recorded ensemble.

**NEXT ITEM.** Mechanise the strict floor — `--projection strict` on
`ci/corpus-floor.py measure`, stamped so a strict floor can never be quoted as
a declared one — and then re-base `gameplay-floor.toml`. What closes it: a
tracked script prints the strict floor beside the declared one for the same
ensemble, and `ci/corpus-floor-falsify.sh` gains a case that reds if the two
are ever swapped. That is §15.4's item with this branch's finding folded in;
`ItemEntity.tick` itself needs an instrument that does not read a randomly
drawn velocity, and none is costed yet.
