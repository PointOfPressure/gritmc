### COLD START — branch `be-furnace-brewing`, off `main` (`d48904cb5`), 2026-10-01

**The furnace family and the brewing stand tick, and an open menu over
either is swept every tick.** Worktree `~/grit/work-befurn`, remote dir
`~/rustmc-befurn`. Not merged — the orchestrator gates.

**WHAT WAS WRONG ON MAIN** (lines are main's; `docs/FINDINGS.md` has them):

* a furnace never smelted — menu writes bypassed
  `AbstractFurnaceBlockEntity.setItem`, so `cookingTotalTime` stayed 0 and
  the `if_icmpne` completion test never fired;
* the brewing stand's ticker arm was the catch-all `_ => {}`;
* no per-tick `broadcastChanges`: a ticker's slot writes and every data slot
  stayed frozen on the client (797's missing `container_set_slot`);
* the blast furnace, smoker and brewing stand opened no menu (744-746);
* furnace/brewing counters saved as constructor zeros; a lava bucket was
  deleted instead of leaving `BUCKET`.

**WHAT WAS PORTED.** `crate::menuslots` (the six slot subclasses),
`AbstractFurnaceMenu.quickMoveStack`, `BrewingStandMenu.quickMoveStack`, the
data-slot phase of `broadcastChanges`/`broadcastFullState`,
`click::sweep_container` + `world_send::sweep_open_menus` (shared
`Arc<Mutex<OpenMenu>>` on the `Connection`), `brewing::VanillaBrewing`,
`doBrew`'s remainder, `consumeFuel`'s remainder, live save/load of the
counters, and `container_set_data` decoding in `oracle-runner`.

**NULLS — do not re-open.**

* "brewing needs data components" is false for every stack this server can
  hold: a component-less potion has `PotionContents.EMPTY` and `mix` returns
  it unchanged. Vanilla's own recording of 840 confirms: 400 ticks, `1035`,
  gunpowder consumed, bottle untouched.
* per-tick `container_set_data` streams cannot be graded exactly — two per
  tick while lit, and the count in a settle window is harness timing. Every
  data check here grades a value that moves once.

**NOT DONE.** Smelting XP (`FurnaceResultSlot.onTake` ->
`awardUsedRecipesAndPopExperience`) needs an experience-orb entity this
server does not have; `RecipesUsed` is therefore not tracked. `doBrew`'s
remainder drop lands on `TickEffects::drops`, which no caller spawns.

**NEXT ITEM.** The experience orb: an entity kind, its `add_entity` and
value metadata, then `awardUsedRecipesAndPopExperience` (`createExperience`'s
`Math.random` fraction makes only integer-XP recipes gradeable — raw gold's
1.0 is the scenario).

**STATE AT HALT (2026-10-01, owner's usage stop).** Full corpus run: 162/192 declared; all 17 of this branch's checks match; 2 REGRESSED are vanilla-side flaps (FINDINGS). T0 last run RED on 4 stages: projection-drops counts (fixed, committed), `FIELD_UNIVERSE` 129 -> 132 pin in `ci/corpus-falsifiable.py` (NOT yet changed — needs the re-sweep's number), `cargo fmt --check` (passes now), `t1-par-falsify` case 6 timing flake.
Unfinished: job `befurn-sub3` (subset 741+842, splice into `$SP/fullpair`, re-sweep, grades) writes ledgers into `~/rustmc-befurn/ci/checks/`; copy `gameplay-falsifiable.toml` and `gameplay-grades.toml` back, bump `FIELD_UNIVERSE`, add 842's baseline row, re-run T0. Next item after that: `Level.updateNeighbourForOutputSignal` is called by nothing, so comparators never see a furnace's or chest's contents change.

### COLD START — branch `be-furnace-brewing`, rebased onto main `3a74284a5`, 2026-10-01 (second session)

**Closed the halt's leftovers and the next slice: a container's contents now
reach a comparator.** Worktree `~/grit/work-befurn2`, gate tree
`~/grit/work-befurn-gate`, remote dirs `~/rustmc-befurn` (recordings, sweeps,
T0) and `~/rustmc-befurn-cmp` (compile/unit only).

**PORTED.** `BlockEntity.setChanged` -> `Level.updateNeighbourForOutputSignal`
from the block-entity walk (per ticker, before the next one runs) and from
`handle_container_click`; `getRedstoneSignalFromBlockEntity` for barrel,
hopper, dispenser/dropper, shulker box, brewing stand; a ticker's
`setBlock` now carries its flags (furnace 3, brewing stand and jukebox 2)
through `shapes::set_block`, so an observer watching a furnace pulses.
Dropped `b60cb51a9` on the rebase: main already decodes `container_set_data`
with the same field names.

**MEASURED** (`ci/t2-gameplay-subset.sh`, `~/rustmc-befurn`): 24 of this
branch's checks plus 741/744-746/797/456 at dbe18e394 — the furnace and
brewing checks all match, 741 absent as baselined; 847-849/854 re-recorded
at 5d80e3b27, declared 4/4 match. Ledgers swept over `~/rustmc-stack`'s
16:33 pair with those recordings spliced in.

**NULLS — do not re-open.**

* "the branch moves `FIELD_UNIVERSE` to 132": it moves it by nothing. Main's
  own pair swept alone reads 137, dead 2 (`move_entity_rot.entity/.rest`),
  unaskable 15 (`waypoint.chunk_x/.chunk_z`); the branch's splice reads the
  same. The pins in `ci/corpus-falsifiable.py` say so.
* A scenario that clicks into a hopper menu cannot be recorded here: this
  server opens none (741 absent), the subject errors and the reconnect costs
  the next scenarios their walks. 842/847/848 load hoppers from a chest.

**OVERLAP.** Unmerged `origin/be-hopper-dispense` ports the analog outputs
(`2210cbf24`), a post-pass `setChanged` notification (`72ad51dfa`) and a
hopper menu plus its own sweep (`3595d42dd`). The second to land keeps one of
each; `docs/FINDINGS.md` has the line.

**NEXT ITEM.** Unchanged from the halt: the experience orb, then
`awardUsedRecipesAndPopExperience` (raw gold's integer 1.0 XP is the
gradeable scenario). Also open: `doBrew`'s remainder lands on
`TickEffects::drops`, which no caller spawns.
