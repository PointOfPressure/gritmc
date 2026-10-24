# COLD START — branch `useblock-port-the-17`

### COLD START — branch `useblock-port-the-17`, 2026-08-12

**The item: port the 17 `use_without_item.json` rows `golden-probe-audit`
proved were readings of the probe's own player, not of the block.** Of the 17,
13 (12 `FenceGateBlock` + `DaylightDetectorBlock`'s `facing`/refusal rows) are
already hand-ported on `gameplay-block-interactions` (`6ef6e0a9`, merged to
`main` ahead of this branch's base) — confirmed by reading that code directly
rather than assuming from the brief. **This branch closes the remaining
`mayBuild`/`game_master` axis: `ComparatorBlock`, `DaylightDetectorBlock`,
`RedStoneWireBlock`, `RepeaterBlock` (all `mayBuild`) and `LightBlock`
(`canUseGameMasterBlocks`) — 5 of the 17 rows, across 4 classes' worth of new
code plus one existing class's existing hand port gaining the same gate.**

### What was ported, and the one-mechanism claim

`javap -p -c -constants` on the pinned jar's `ComparatorBlock.useWithoutItem`,
`RedStoneWireBlock.useWithoutItem` and `RepeaterBlock.useWithoutItem` shows the
**identical** leading five instructions in all three: `aload player;
invokevirtual Player.getAbilities; getfield Abilities.mayBuild; ifne <rest>;
getstatic PASS; areturn`. `DaylightDetectorBlock.useWithoutItem` opens with
`Player.mayBuild()` instead — itself only `return this.abilities.mayBuild;` —
and its failure branch is `invokespecial BaseEntityBlock.useWithoutItem`,
which resolves to `BlockBehaviour.useWithoutItem` (`BaseEntityBlock` does not
override it) — `return PASS` verbatim. **All four therefore answer the same
observable thing, checked rather than assumed, and one gate
(`crate::useblock::use_gate`) covers all four.**

`LightBlock.useWithoutItem` reads a different bit and its *false* branch
answers differently: `canUseGameMasterBlocks() == false` returns `CONSUME`
with no write, not `PASS`. The golden table's base stance is never a game
master, so all 32 `minecraft:light` rows already record `consumes: true,
writes: []` — that branch needs no gate. Only the *true* branch —
`state.cycle(LEVEL)`, `setBlock(pos, next, 2)` — has no table cell, so
`use_gate` supplies only that one arm and defers to the table otherwise.

New code: `crate::useblock::use_gate(classes, state, may_build, game_master)
-> Option<Gate>`, consulted **ahead of** `hand_ported` and the table both in
`world_send::handle_use_item_on` (mirrors vanilla's own order — the gate is
offset 0, before any state is read, in all five classes). `may_build=true,
game_master=false` are **hardcoded at the one call site**, not read off a
connected player — this server has no per-player ability model, the same
stated limit `DaylightDetectorBlock`'s existing hand port already carried
("every player this server serves may build"). So in production `use_gate`
answers `None` on every live call today: the port is **live-behaviour-
identical**, checked by construction (the reachable branches are untouched;
only the previously-unreachable `mayBuild=false`/`game_master=true` arms are
new), ready for a real ability bit to replace the constant.

6 new tests in `crates/rustmc-server/src/useblock.rs`. **Witnessed failing**:
temporarily prefixed both of `use_gate`'s gate conditions with `false &&`,
resynced to the box, reran — exactly 3 of 19 failed
(`a_comparator_click_from_a_player_who_may_not_build_passes`,
`the_may_build_gate_is_the_same_on_all_four_classes`,
`a_game_masters_light_click_cycles_the_level`), each `left: None, right:
Some(...)`. Reverted, reran, 19/19 green. `ci/t0-remote.sh` GREEN, 0 skips,
`rustmc-p17` on the Threadripper, 2026-08-12.

### What was killed as a null — do not re-run this

**The toml's own "delete the entry, `STALE` fires" claim is refuted —
verified by doing it, not by reading it.**
`ci/checks/use-probe-dependent.toml`'s header (as `golden-probe-audit` wrote
it) says porting a block by hand lets its entry be deleted, "at which point
the `STALE` rule fires and tells you to". `ci/check-use-probe.py`'s `STALE`
branch fires only for a toml entry whose block is **absent** from a freshly
re-probed table's `probeDependent`/`probeUnknown` set, and that set is a
property of the **pinned jar**, re-derived by `UseGolden.java` with zero
knowledge of anything in `crates/rustmc-server`. Hand-porting a class cannot
make vanilla stop reading its player.

Checked directly against the real generated table
(`data/vanilla/use_without_item.json`, regenerated in this tree via
`ci/gen-gameplay-tables.sh`, and separately against the table already
generated on `golden-probe-audit`'s own worktree, `~/grit/work-probe`, which
carries the full 12-stance sweep): deleting `ci/checks/use-probe-dependent.toml`'s
`comparator` entry and re-running `ci/check-use-probe.py` against either
table produces

```
NEW  minecraft:comparator (ComparatorBlock) is probe-dependent on ['no_build']
and is not in ci/checks/use-probe-dependent.toml. ...
```

— exit 1, `ci/use-golden.sh`'s hard refusal, which `ci/gen-gameplay-tables.sh`
runs for every tree on the box. The same holds for the already-hand-ported
`FenceGateBlock`/`DaylightDetectorBlock` rows — deleting all 13 of those
produces the identical `NEW`, not `STALE`, so this is not specific to the
`mayBuild` axis; it is a property of the checker as built. **So the toml is
left unchanged by this branch — correctly, not by omission — and every one of
the 17 rows stays in `ci/checks/use-probe-dependent.toml` regardless of what
gets hand-ported.** What would actually let an entry be removed — plausibly a
`hand_ported`-aware exemption added to `UseGolden.java` or
`ci/check-use-probe.py` — is unbuilt, and building it is generator work this
item did not authorise (flagged to the orchestrator rather than done
unilaterally, given a second, concurrent branch — `golden-useitemon-axis` —
is extending the same refusal machinery for a different table).

**Do not re-attempt "delete the entry, expect `STALE`" for any of the 17.**
The axis these rows sit on is a permanent fact about the pinned jar.

### The single next item

Whoever wants an entry actually removed from `ci/checks/use-probe-dependent.toml`
needs to build the generator-side exemption described above first — that is
new, unscoped tooling work, not a port. Absent that, the remaining 12
`FenceGateBlock` and 1 `DaylightDetectorBlock`-refusal rows are already fully
hand-ported (by `gameplay-block-interactions`) and the 5 `mayBuild`/
`game_master` rows are fully hand-ported by this branch — **all 17 are ported
in the sense the audit asked for (the server's answer no longer depends on
the table's one recorded player), and 0 of the 17 toml entries can currently
be deleted under the checker as built.**

### COLD START — branch `useblock-port-the-17`, 2026-08-12 (the measurement)

**The entry above was written from a reading of the code and had never been
run against the corpus.** This one is the measurement it was missing (ORACLE
rule 9). Rebased `bd1f6429..` onto `main` at `84f789db` — nine commits,
including `golden-probe-audit`'s allowlist, `golden-useitemon-axis`'s second
table and `itemuse`'s `ItemStack.useOn` dispatch. **The rebase was clean:
`main` never touched `crates/rustmc-server/src/useblock.rs`, `placedby.rs` or
`placement.rs`, and the post-rebase `useblock.rs` is byte-identical to the
pre-rebase one**, so the falsification witnessed in `61a710c0` (both gate
conditions prefixed `false &&` -> exactly 3 of 19 tests fail) still describes
this code and was not re-burned on a box running another tree's corpus.

**The port was not overtaken.** `main`'s golden generator emits probe-dependent
rows with a `probeDependent` column and refuses to *write the table* when one
of them is unallowlisted; it does not drop the rows, and it did not port them.
`ci/checks/use-probe-dependent.toml` still carries all 17 entries on `main`.

#### The audit's own check, re-run on this tree — 17, unchanged

```
probe sweep: 17 of 184 rows move with the player,
             all 17 allowed by ci/checks/use-probe-dependent.toml
rows total 184 | probeDependentBlocks 17 | probeUnknown 0
stances swept 12 | unavailable {}
```

**The answer to the question `golden-probe-audit` left open: gating on
`mayBuild` does neither of the two things the brief offered. It does not make
the five rows probe-independent, and it does not change which probe they
depend on.** The five are still `probeDependent` on exactly their recorded
axes — `no_build` for `ComparatorBlock`, `DaylightDetectorBlock`,
`RedStoneWireBlock`, `RepeaterBlock`; `game_master` for `LightBlock` — and the
other 12 still on the four `FenceGateBlock` axes. **17 -> 17 is the
deliverable, and it is a structural result rather than a disappointing one:**
the sweep re-probes the pinned jar and knows nothing whatever about
`crates/rustmc-server`, so no port can move this number. It is measured here
rather than argued because the branch previously argued it.

Re-verified against the *post-rebase* checker, on this tree's freshly stamped
table, using temp allowlist copies so nothing shared was edited: deleting
`minecraft:comparator` yields `NEW`, not `STALE`; deleting all 13
already-hand-ported `FenceGateBlock`/`DaylightDetectorBlock` rows yields 13
identical `NEW`s. `ci/check-use-probe-falsify.sh` is **10 passed, 0 failed**,
its `real-table` case reading this tree's own 17-of-184.

#### Why no corpus verdict could have moved, established before the run

Both hardcodes at the one call site are correct for every player the corpus
can produce, and this is now read off the instruments rather than assumed:

* `may_build = true` — `ci/t2-gameplay.sh` boots both servers
  `gamemode=creative`, so `Abilities.mayBuild` is true and the `Gate::Pass`
  arm is unreachable.
* `game_master = false` — `Player.canUseGameMasterBlocks()` is
  `abilities.instabuild && permissions().hasPermission(COMMANDS_GAMEMASTER)`
  (`javap -p -c -constants`, offsets 0-30, pinned `server-26.2.jar`), and
  `ci/t2-gameplay.sh`'s own header states its players are not operators — the
  same fact that makes it push `gamerule` through the console fifo instead of
  `Step::Command`. So the second conjunct is false and the `Gate::Write` arm
  is unreachable too.

`use_gate` therefore answers `None` on every call the corpus makes, and the
corpus is the wrong instrument for this port by construction. **The five rows'
corpus coverage is also thinner than the count suggests**: `redstone_wire` and
`light` are named by **no** scenario in `ci/checks/gameplay/` at all, so two of
the five are unexercised — "a gate a window cannot exercise proves nothing".
The three that are exercised are covered by four scenarios.

#### The corpus run — the verdict the branch was missing

Run in `rustmc-p17` on the Threadripper, 2026-08-12, **against base
`84f789db`** (the first rebase; see the caveat at the end of this section).
Queued 18m behind `rustmc-stack`'s merge gate, ran 40m, `T2-GAMEPLAY GREEN`.
What the run itself printed:

```
CORPUS  declared  78/93 match   (12 diverge, 3 absent, 0 vacuous, 0 mute, 0 inert, 0 error)
CORPUS  strict    74/93 match   (18 diverge, 1 absent, 0 vacuous, 0 mute, 0 inert, 0 error)
CORPUS  floor     4/92 scenarios cannot match — 4 vanilla does not reproduce
                  against itself, 0 this harness cannot sample
CORPUS  floor     ensemble coverage92-selfcheck-n6-20260811, N = 6, 15 pairwise
CORPUS  floor     STALE PROJECTION: measured under proj:7cefde0fb56d9f3e, this
                  run compares under proj:f7fbb29926074663
CORPUS  floor     STALE DENOMINATOR: measured over 92 scenarios, this run has 93
CORPUS  floor     LOWER BOUND: at this N a scenario flapping < 39.3% of the time
                  still shows 6 identical runs (alpha 0.05)
```

**Corpus size 93 before and 93 after — this branch adds no scenario.** The
floor is `4/92` and **the run withdrew its own quotability on two independent
grounds**, so it is recorded here beside the score and must not be read as an
error bar on it: it was measured over 92 scenarios against this run's 93, and
under a different comparator. Note the projection this run actually compared
under — **`proj:f7fbb29926074663`** — which is neither the `7cefde0fb56d9f3e`
the floor file carries nor the `67604e5eb262728f` that `trace-itemstack-legible`
is expected to move it to. Never subtracted from the score, per the floor
file's own rule.

#### The five rows' verdicts, from this run

| row | corpus scenario | baseline | this run |
|---|---|---|---|
| `minecraft:repeater` | `repeater-faces-away-from-the-player` | `match` | **`match`** |
| `minecraft:repeater` | `a-repeater-delays-the-signal-it-passes-on` | `diverge` | **`diverge`** |
| `minecraft:comparator` | `a-comparator-switches-to-subtract-mode` | `match` | **`match`** |
| `minecraft:daylight_detector` | `a-daylight-detector-inverts-when-right-clicked` | `diverge` | **`diverge`** |
| `minecraft:redstone_wire` | *none — no scenario names it* | — | **unexercised** |
| `minecraft:light` | *none — no scenario names it* | — | **unexercised** |

**Every exercised row landed exactly on its baseline: nothing moved, nothing
regressed.** That is the predicted result, and it was predicted *before* the
run from the harness's own configuration rather than read off it afterwards,
which is what makes it evidence rather than a shrug. Two of the five rows have
no corpus scenario at all, so for those the corpus is silent — not clean.

**The six `BETTER` lines this run printed are not this branch's and were not
locked in.** `hoe-turns-grass-into-farmland` and `flint-and-steel-lights-a-fire`
are `itemuse`'s `ItemStack.useOn` port, `a-fence-gate-opened-from-behind-turns-to-face-you`
is `gameplay-block-interactions`', and `swapping-to-the-offhand-shows-on-both-channels`,
`attacking-another-player` and `attacking-from-out-of-range` belong to the
actions work — all already on `main` and merely not yet baselined.
`ci/checks/gameplay-baseline.toml` is **untouched by this branch**: a branch
whose own finding is a documented negative must not ratchet the corpus on six
improvements it did not cause.

**Caveat on the base, stated because it is the one thing that could move the
table above.** These verdicts were taken at `84f789db`. This branch was then
rebased again onto `3ddb3ca8`, which brings in `gameplay-redstone-group`'s
`crate::ticks::drain_requests` call in `world_send::record_updates` — 8 lines,
downstream of this port and unable to reach `use_gate`, but squarely aimed at
the scheduled-tick deferral that `a-repeater-delays-the-signal-it-passes-on`
diverges on. **If that scenario is `match` on current `main`, that is the
redstone group's result and not this branch's.** The corpus was not re-run for
it, because re-running would measure their change, and this port is provably
unable to move any of these four either way.

#### What is NOT ported, stated plainly

The gate's two new branches are reachable only from a per-player ability model
this server does not have. Their whole coverage is the 6 unit tests in
`useblock.rs`. This branch buys correctness-in-waiting and a tested mechanism;
it buys **no** measured behaviour change today, and nothing in it should be
quoted as one.

#### Killed as a null — do not re-run

* **"Hand-port the block, then delete its allowlist entry and `STALE` fires."**
  Refuted twice now, and the second time against the post-rebase checker. The
  probe-dependent set is a property of the pinned jar.
* **Re-witnessing `61a710c0`'s `false &&` falsification after the rebase.** The
  file is byte-identical; there is nothing new to falsify.
* **Expecting the corpus to move on this branch.** Established unreachable
  above *before* the run, and then confirmed by the run.

#### The single next item

Unchanged from the entry above and now with a measured reason: an entry can
only leave `ci/checks/use-probe-dependent.toml` once a `hand_ported`-aware
exemption exists in `UseGolden.java` or `ci/check-use-probe.py`. That is
generator work. The item that would make *this* port observable is different
and larger: a per-player ability/permission model, at which point
`may_build`/`game_master` stop being constants at the `world_send` call site
and `LightBlock` needs a corpus scenario, which it does not have.
