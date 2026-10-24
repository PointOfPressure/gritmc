# COLD START — branch `corpus-reach-ceilings`

### COLD START — branch `corpus-reach-ceilings`, 2026-08-12

**The item: make `ci/vanilla-reach-census.py` report a reachable CEILING beside
every observed count and every registry size. Done.
`docs/PARITY-ROADMAP.md` §29.**

§21 obeyed its own rule — no denominator from our tree — and that rule held.
What went wrong is that a **registry size** is an upper bound on what the corpus
could ever observe and was printed as though it *were* that quantity. §25 then
measured the block-entity row at **20 of 49, not 49**, and §27 found 1,712 of
1,939 unheard sound events unreachable by any scenario. `1 of 49` reads as
catastrophic where the honest reading is `1 of 20`.

**No vanilla was booted, nothing was built, and the box-wide corpus lock was
never taken.** The whole item is static: the pinned jar, `generated/reports/`,
`javap`, and the six recordings already in `~/rustmc-cov2`. The census runs in
about five seconds.

### What moved — every row's before / after

```text
  UNIT 1  clientbound packets   31 / 141        -> 31 / 129 / 141
  UNIT 2  tick sections         41 / 120 dead   -> 41 / 120 dead, 41 of the 41 OURS
  UNIT 3  sound_event           29 / 1968       -> 29 / 256  / 1968     ours (cited)
          block                 43 / 1196       -> 43 /  ?   / 1196     unknown
          entity_type            4 / 158        ->  4 /  ?   / 158      unknown, 91..158
          block_entity_type      1 / 49         ->  1 /  20  / 49       jar
          menu                   3 / 25         ->  3 /  20  / 25       jar 21, ours 20
          block_state           87 / 32366      -> 87 /  ?   / 32366    unknown
  sensitivity spread            55x registry    -> 13x against ceilings
  work order                    block entities 48 -> 19; menus 22 -> 17
```

### The four rules, and the two that had to be corrected before they worked

* **block entities** — does the registered type override
  `BlockEntity.getUpdatePacket()`? 20 of 49 do. Ids are paired with their
  constructor by **bytecode offset inside `BlockEntityTypes.<clinit>`**, not
  positionally across two class files. Two guards: the base body must still be
  `aconst_null; areturn` or the rule is void, and every override must construct
  a `ClientboundBlockEntityDataPacket`.
* **menus** — is the registered factory called from any class but `MenuType`?
  Four have no caller anywhere in the jar; `merchant` has only an entity's.
  Vanilla 21, this harness 20.
* **clientbound packets** — is the class constructed anywhere in the jar?
  **The rule fires on nothing.** Vanilla sets no ceiling below 141 and the whole
  reduction to 129 is `trace::is_housekeeping`.
* **sound events** — CITED at 256 from §27.1, labelled `ours`, because it is a
  ceiling on **our emitter**. Vanilla's own is unknown.

**Two guards fired while this was written and both were the guard working, not
a bug in the jar.** The block-entity corroboration refused eighteen of the
nineteen overriding classes: they declare `getUpdatePacket` twice, covariantly
and as a synthetic bridge, and the bridge — whose descriptor matches the base —
constructs nothing. And the menu rule scored **all twenty-five** unreachable
while matching factories on the exact descriptor, because the registration site
uses the two-argument overload and every real opener uses a wider one. Both
corrections are comments in `ci/vanilla-reach-census.py`.

### Nulls — killed with numbers, do not re-run these

* **"a block has a `BlockItem`" as a block ceiling.** It is a floor, not a
  ceiling: fire, farmland, dirt_path and water all arrive without one.
* **`net/minecraft/references/BlockItemIds` as a block↔item authority.** A
  787-name datafixer list; 758 of its names are blocks against a registry of
  1,196. Not 26.2's answer to anything.
* **"no class but `SoundEvents` reads the field" as a sound ceiling.** Excludes
  131 of 1,968 and is **unsound**: sound ids also reach `playSound` by name
  from data (jukebox songs, `sound_types.json`), and 94 registry entries have
  no `SoundEvents` field at all. Measured, refused, left out.
* **A `new`-opcode-only scan for unbuildable packets.** Called ten types dead
  including `SET_TITLE_TEXT`, which `/title` plainly sends: a record built
  through a `::new` method reference carries no `new` at the call site. Union
  the `<init>` call targets and the exclusion set is empty.

### Which ceilings are ours to move — the whole point of the split

```text
  OURS     41 tick sections   ci/t2-gameplay.sh's three game rules + spawn config
           12 packet types    trace::is_housekeeping drops them
           29 block entities  vanilla DOES send them, inside LEVEL_CHUNK_WITH_LIGHT;
                              one decoder arm moves the ceiling 20 -> 49
            1 menu type       merchant needs a villager
          154 entity types    the same spawn settings; >= 91 are one hotbar slot away
         1712 sound events    OUR emitter, not vanilla's
  VANILLA  29 block entities  the same 29, on the block_entity_data channel
            4 menu types      generic_9x1/_9x2/_9x4/_9x5 have no opener in the jar
            0 packet types, 0 tick sections
```

### The single next item, and the result that closes it

**Decode the `BlockEntityInfo` list inside `LEVEL_CHUNK_WITH_LIGHT` in
`trace`.** It moves the block-entity ceiling from 20 to 49 without un-dropping
the packet, and it is the only change in this census that buys 29 registry
entries for one decoder arm where a scenario buys one. It closes when
`ci/vanilla-reach-census.py` prints a `block_entity_type` ceiling of 49 and the
`jar` owner on that row becomes `ours` — see §25.2 for the payload shape.

Two smaller ones are named in §29.6 and belong to other branches: the 347
break/hit/fall sound events already sitting in `sound_types.json` (§27.1), and
an `ops.json` for the corpus player, which unblocks four block entity types
(§25.2, where the cost of op-ing every scenario's join traffic is also
measured and is not free).

**And the honest gap this branch did not close: `block`, `block_state` and
`entity_type` still have no ceiling**, and they are rows 1 and 3 of the census's
own work order. Whoever finds an honest jar rule for either moves the two
largest numbers in `docs/PARITY-ROADMAP.md`.

### COLD START — branch `corpus-reach-ceilings`, 2026-08-16 — the ceiling was published with the wrong owner, and nothing could have said so

**Measured.** `block_entity_type` reads `1 / 20 / 49`, and the 20 is right. Its
**owner** was `jar` and is `ours`. That is not a nuance: §29.3 of
`docs/PARITY-ROADMAP.md` states that a `jar` ceiling is a fact nobody can spend
money on and an `ours` ceiling is work, so the label was telling every reader to
write off twenty-nine block entity types that one decoder arm buys.

**Why.** `BlockEntity.getUpdatePacket` filters a **route**, not a type. Read off
the pinned jar: `ClientboundLevelChunkPacketData.<init>(LevelChunk)` walks
`LevelChunk.getBlockEntities().entrySet()` and hands each value to
`BlockEntityInfo.create`, which reads `getUpdateTag`, `getBlockPos` and
`getType` and never `getUpdatePacket`. The chunk payload filters nothing. So
vanilla's ceiling on the unit is **49**; the census reaches **20**; the gap is
`trace::is_housekeeping` dropping `LEVEL_CHUNK_WITH_LIGHT`, which is ours.

**The defect class, and it is the repo's central one.** The rule *stated its own
premise in its first paragraph* — "`block_entity_data` is the census's only
channel, because `trace::is_housekeeping` drops `LEVEL_CHUNK_WITH_LIGHT`" — and
nothing in the tree read that list. Both of its guards read
`BlockEntity.getUpdatePacket`, which does not move. `rank()` filtered rows on
the **registry** gap while ranking them on the **ceiling** gap, so a finished
row would still have printed as the work order's fourth-largest system. Branch
`trace-chunk-block-entities` deletes exactly that one line. On the day it lands
the honest ceiling is 49, `ci/t0.sh` stays green, and §29 publishes 5.00%
against a real 2.04%.

**Fixed, in `ci/vanilla-reach-census.py`:** `ceiling_block_entities` returns one
set per channel and `live_ceiling` folds them — a channel counts only if
`trace.rs` neither drops its packet nor leaves it undecoded, and the owner is
`jar` only when the live union equals the union over every channel;
`ceiling_premise_housekeeping` is the guard, pinning the twelve ids in
`HOUSEKEEPING_PINNED` and naming in `HOUSEKEEPING_SPENT_BY` which drops a
ceiling spends and what un-dropping each costs;
`chunk_payload_carries_every_block_entity` refuses if a future jar grows the
filter this channel is claimed not to have; `rank()` filters on the ceiling gap.

**Red-when-broken, because that is the bar.** `--selftest` (run by `ci/t0.sh`)
fires the guard three ways and the first case is `trace-chunk-block-entities`'s
edit verbatim. Reproduced end to end: delete the line from `trace.rs` and the
census exits 2 naming the row and its price, `--selftest` exits 1. Neuter either
refusal and three self-test cases go red. `live_ceiling` is tested on the fold
itself — chunk dropped 20/`ours`, un-dropped **and** decoded 49/`jar`,
un-dropped but still undecoded 20/`ours` — so the guard's refusal rests on a
number that would really have differed.

**KILLED AS A NULL — do not re-run.** *"`trace-chunk-block-entities` has landed
on `origin/main`."* It has not, as of 2026-08-16. `origin/main`'s
`is_housekeeping` still lists `LEVEL_CHUNK_WITH_LIGHT`,
`git branch -r --contains origin/trace-chunk-block-entities` names only that
branch and `world-send-chunk-block-entities`, and no `787`/`730` check exists on
main. **So the published 20 was never numerically wrong** — only its owner was,
and the guard is what stops the number going wrong the day the branch lands.
Do not go looking for a wrong 20 in the history; there isn't one.

**Green:** `ci/check-figures.py`, `ci/check-doc-claims.py`,
`ci/vanilla-reach-census.py --selftest`, `ci/corpus-surface.py --selftest`. No
`.rs`, `.sh` or `Cargo` file is touched by this branch, so nothing in the Rust
half of T0 can have moved. Static: no vanilla booted, no build ran, the corpus
lock was never taken.

**Next item, and the result that closes it:** `block` and `entity_type` still
print `?`. Unchanged from the entry above and still the two largest numbers in
the document.

### COLD START — branch `corpus-reach-ceilings`, 2026-08-16 (second entry) — rebased onto `911c2e04`

The first entry below stands. This records the rebase, one figure that moved
underneath it, and the three-way reconciliation of
`ci/vanilla-reach-census.py`.

**The branch was four commits on `bd1f6429`, not on `main`.** It read
REBASE-CONFLICT in gate preflight, which means SKIP, which means the guard
would never have run. Rebased onto `911c2e04`; four conflicts, all of them
append-at-the-same-anchor except the first:

* `ci/vanilla-reach-census.py` — `rank()`'s `rows` list. `main` had
  `46bdab5d` (4-tuples, derived notes); this branch has 5-tuples with the
  ceiling column. **Took this branch's tuples and carried `main`'s comment
  forward**, because that comment is a lesson — the note read "only `sign` has
  ever been observed" until it went stale — and taking a side would have
  dropped it;
* `docs/PARITY-ROADMAP.md`, `docs/WORKLIST.md`, `ci/figures.toml` — keep
  `main`'s tail, then append this branch's. §29 is still free on `main` (which
  holds 20-28, 33, 34, 37), so the claim number did not have to move.
  `ci/figures.toml` has no merge driver, so that one is always by hand.

**A FIGURE MOVED ON THE REBASE AND IT IS NOT THIS BRANCH'S.** §26 landed in
between and turns `random_tick_speed` back on for check `780`, so `randomTick`
stopped being provably dead: **41 -> 40 sections, game-rule share 8 -> 7,
harness ceiling 79/120 -> 80/120, 65.83% -> 66.67%.** Republished in §29.2 and
in `corpus-reach-ceiling-tick-ownership-20260812`. §26.5 already publishes
`with 780: 7 / 80 / 40 of 120 = 33.3%` from its own run, so this is the same
number reached twice by different routes. **§21's `41/120` is NOT corrected** —
it was taken under its own stamp against a 92-check corpus and is still true of
it. That is exactly what `corpus=` in the four-input stamp exists to separate,
and it worked: `92 -> 106` moved the stamp, which is what forced the re-take.
Recordings, ensemble and N did not move (six runs, `a9dff28db35c`,
`coverage92-selfcheck-n6-20260811`, 93 scenarios).

**The three-way reconciliation, measured rather than asserted.** §29.8 has the
detail. Short version: run this branch's `live_ceiling` against
`origin/world-send-chunk-block-entities`'s own `trace.rs` and it returns
**49 of 49, owner `jar`, 0 dark channels** — which is precisely the question
their commit wrote down as *deliberately not asserted here*. The guard refuses
until `HOUSEKEEPING_PINNED` is bumped from twelve ids to eleven, and that is
the one line whichever branch merges second has to carry. **The published
figure does not depend on merge order; only the pin bump does.**

**KILLED AS A NULL — do not re-run.** Do not "fix" the guard so it tolerates
the other branches' housekeeping list. The refusal is the deliverable. A guard
that accepts both worlds silently is the defect this branch exists to remove,
and `live_ceiling` already gives the right number in both — 20/`ours` here,
49/`jar` there — without any tolerance.

**Verified on the rebased tree**, worktree `/home/sayer/grit/work-ceil3`,
sayer-desktop, static only — no vanilla booted, no build, corpus lock never
taken: `ci/check-figures.py` GREEN (90 registered); `ci/check-doc-claims.py`
72 claims, 0 collisions, 0 drift; `ci/corpus-falsifiable.py --check`
**106/106** carry red-when-broken evidence; `ci/liveness-proofs.py --check`
5/5, 0 pinned unsound; `ci/corpus-surface.py --selftest` OK;
`ci/vanilla-reach-census.py --selftest` GREEN. Red-when-broken re-proved after
the rebase, both arms: delete the `LEVEL_CHUNK_WITH_LIGHT` line from `trace.rs`
and the census exits **2** naming the row while `--selftest` exits **1**;
neuter either refusal in `ceiling_premise_housekeeping` and **three** self-test
cases go red. `trace.rs` restored and clean in both cases.
