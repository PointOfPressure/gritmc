### COLD START — branch `corpus-coverage-audit`, off `main` (`1b2994a6`), 2026-08-10

**The item was to grow the gameplay corpus, because it had stopped being able
to find anything. The audit is done and five checks are written; their
verdicts, their stability and the re-measured floor are IN PROGRESS.**
Worktree `~/grit/work-coverage`, remote dir `~/rustmc-coverage`. Not merged.
Full account in `docs/PARITY-ROADMAP.md` §20.

**WHAT IS MEASURED AND WILL NOT NEED REDOING.**

* **The corpus's denominator, which nobody had.** `ci/corpus-surface.py`, new,
  reads the pinned jar's own `GameProtocols` registration. Clientbound: 141 in
  the protocol, 57 named in `rustmc-proto`, 12 housekeeping, 38 legible, 91
  opaque, 30 observed by a check, **36 emitted by vanilla during a corpus run
  and 28 of those observed**. Serverbound: 21 of 69 can be sent at all.
  Behaviour: `use_without_item` 11 of 39 implementations, `set_placed_by` 4 of
  7. 17 vanilla recordings, sayer-desktop, 2026-08-10.
* **Eight packet types vanilla emits inside windows the corpus already records
  have no check looking at them** — `WAYPOINT` 7484, `BUNDLE` 544,
  `ROTATE_HEAD` 346, `SET_CURSOR_ITEM` 221, `MOVE_ENTITY_ROT` 87,
  `CONTAINER_SET_DATA` 68, `UPDATE_ATTRIBUTES` 68, `SET_PLAYER_INVENTORY` 17.
  Five are opaque, which means *more* strictly compared, not less.
* **Stability of a candidate projection is now free.** `--recordings` screens
  it against recordings that already exist. `0x60`, `0x00`, `0x13`, `0x6C` and
  `update_attributes` are stable at n = 17.
* **`damage_event` and `set_health` occur 0 times in 17 recordings** and are
  projected by two checks each. Creative players are invulnerable; `550`/`551`
  name four kinds and are decided by one.
* **The brief's `7 of 88` and `docs/PARITY.md` §4.1's `9 of 88` are both
  correct, on different ensembles** — `yrot6-20260809` and
  `twin-homog-20260809`. Neither supersedes the other. §4.1 now carries a ⚠
  pointer.

**NULLS AND CORRECTIONS — DO NOT RE-RUN OR RE-DERIVE THESE.**

* **Do not score `update_shape.json`'s `class` column.** It is an integer
  shape-group index, not a class name; 139 of its blocks map to more than one
  and `piston_head` to twelve. A first cut reported "22 of 118 classes" from 15
  covered blocks, a ratio with no referent.
* **Do not count runtime classes in the behaviour tables.** Inheritance
  collapses eight of them: `TrappedChestBlock`, `CopperChestBlock` and
  `WeatheringCopperChestBlock` all inherit `ChestBlock.useWithoutItem`, which
  `090` already covers. 11 of 39, not 12 of 47.
* **`rotate_head` is not a safe projection and the pre-screen proved it.**
  Stable in three rigs, present in 9/17, 2/17 and 16/17 runs of three others.
  It is a property of how much the rig moves. Do not write a check that
  observes it without pinning the rig first.
* **`ci/corpus-surface.py --selftest` asserted only exit codes and passed with
  a guard deleted.** Fixed to assert which guard fired; all three of
  `parse_protocol`'s were falsified against it. It is in `ci/t0.sh` now.
* **The corpus README's `collapse_repeats` rule was wrong** — it said the kind
  must be in `observe`; the loader accepts `DECODED_KINDS` too and eleven
  checks rely on it for the *strict* projection. Corrected.
* **`ci/check-figures.py` cannot catch a stale corpus denominator.** Its
  `corpus-floor` kind requires `scenarios` to be present, not to match the
  tree. `ci/corpus-floor-report.py` prints `STALE DENOMINATOR` and is
  warn-only by design.

**WHAT IS IN FLIGHT.** Five checks are written and committed —
`700`/`701` fence gate (a pair: `701` pins the `FACING` rewrite `700` cannot
see), `710` daylight detector, `720` container-click cursor (`packet:0x60`),
`790` flower pot. Corpus is **93**. Still to do, in order:

1. `ci/t2-gameplay-subset.sh` for the five — verdicts. **Queued behind a merge
   gate in `~/rustmc-batch` for 17 min as of writing.**
2. Any that come back `match` must be shown able to go red. `720` is the
   likely one: `SET_CURSOR_ITEM` **is** implemented, at
   `crates/rustmc-server/src/world_send.rs`'s `Reply::SetCursor` arm, which is
   a single send and therefore a clean thing to break and revert.
3. `ci/subset-stability.sh 6 <the five>` — new script, records N cold vanillas
   and runs `ci/corpus-floor.py` over them.
4. **`ci/checks/gameplay-baseline.toml` needs one `[expect]` line per new
   check or the full gate reds** — `compare.rs` refuses a name that is not
   there. Values may only be `match`, `diverge` or `absent`, and may only be
   written from a full-corpus run, not a subset.
5. Re-measure the corpus floor on 93 and register it with its ensemble, N and
   `p_hide`. **Do not quote it beside the 9 or the 7** — different corpus,
   different ensemble. §20.6.

**`790` MUST STAY THE HIGHEST-NUMBERED CHECK.** `Player.addItem` writes the
actor's inventory and the actor plays the whole corpus, so both the window-0
`incrementStateId` chain and the free-slot layout are inherited by anything
after it.

### COLD START — branch `corpus-coverage-audit`, rebased onto `main` (`1f8f3813`), 2026-08-11

**Entry written mid-item, before the corpus run returned, so that the
denominator work is not held only in one agent's context.** Worktree
`~/grit/work-cov3`, remote dir `~/rustmc-cov3`.

**THE DENOMINATOR IS 92, NOT 93, AND THE BRIEF SAID 93.** Counted from the
tree rather than carried: `ls ci/checks/gameplay/*.toml` is 92 here and 87 on
`main`. The branch adds five (`700`, `701`, `710`, `720`, `790`); the
`88 -> 93` this branch was written against predates `water-spreads-cascade`,
which took `main` `88 -> 87` by parking `160-water-spreads.toml` in
`ci/checks/gameplay/open/`. `diff` of the two check directories confirms
`160-water-spreads.toml` is the *only* difference between `~/rustmc-cov2`'s 93
and this tree's 92. `ci/checks/gameplay-baseline.toml` carries 87 `[expect]`
entries and needs five more.

**STEP 3 WAS DONE AND ITS ARTEFACTS SURVIVED — DO NOT RE-RUN IT.**
`ci/subset-stability.sh 6` over the five, ensemble `coverage93-n6-20260810`,
`~/rustmc-cov2/oracle-work/subset-stability/floor/floor.json`: floor **1 of
5**, N = 6, 15 pairs, one recorder (`tree-now:04362178408d96ab`), p_hide
0.393. The one is `a-daylight-detector-inverts-when-right-clicked`, class
`reference` — vanilla's own, 5/15 pairs, 2 distinct projections. The other
four are `stable` at 0/15. Subset denominator; not a corpus figure. That
figure is denominator-independent, so parking `water-spreads` does not touch
it.

**A FULL-CORPUS N = 6 SELFCHECK ALSO SURVIVED**, at
`~/rustmc-cov2/oracle-work/gameplay-selfcheck/r{1..6}.json` with
`~/rustmc-cov2/selfcheck.log`: declared **85/93** reproduced (8 unstable),
strict **81/93** (12 unstable). `water-spreads` is one of the 12, at 5/5
comparisons. Six full-corpus recordings from one recorder — this is the
ensemble the re-stamped floor is measured over, re-projected through this
tree's 92 checks and this tree's binary rather than re-recorded.

**TWO OF THE FIVE NEW CHECKS ARE REFERENCE-UNSTABLE**, which is what makes the
`[expect]` question a real one rather than bookkeeping: `790`
(`a-flower-pot-empties-when-right-clicked`) is strict-unstable at 3/5
comparisons and the selfcheck flagged it `NEWLY UNSTABLE`; `710` is the
subset's 5/15 flapper and the corpus selfcheck reports it `UNRESOLVED AT THIS
N` (it could not have seen a 5/15 rate in 5 comparisons at alpha 0.05, so its
absence there is not evidence).

**SECTION NUMBER: this branch is `docs/PARITY-ROADMAP.md` §20, claimed.** It
opened as §17, renumbered itself to §18 in a merge that the rebase drops, and
both are taken in `main` now (`step-look-timing` §17,
`gameplay-recipe-book-add` §18, `gameplay-gap-rederive` §19).

### COLD START — branch `corpus-coverage-audit`, closed, 2026-08-11

**The item is closed.** Steps 1-5 of the original cold start are all done, the
floor is re-stamped and quotable, and the branch is gate-clean on `main`
`1f8f3813`. Worktree `~/grit/work-cov3`, remote dir `~/rustmc-cov3`. Not
merged. Full account in `docs/PARITY-ROADMAP.md` §20, and §20.8/§20.9 are the
two sections this entry summarises.

**THE HEADLINE.** Corpus **87 -> 92**. `CORPUS declared 71/92 match` (16
diverge, 5 absent, **0 vacuous, 0 inert, 0 error**), `CORPUS strict 67/92`.
**Floor 7 of 92**, ensemble `coverage92-selfcheck-n6-20260811`, N = 6, 15
pairs, one recorder, p_hide 39.3%, projection `proj:b385528330fee646` — the
same comparator as the run, so **the floor is quotable beside the score** and
both `STALE PROJECTION` and `STALE DENOMINATOR` are gone from the gate output.

**WHAT WAS KILLED AS A NULL — DO NOT RE-RUN OR RE-DERIVE.**

* **Do not re-run `ci/subset-stability.sh` for the five.** It was run on
  2026-08-10 and its artefacts are intact in
  `~/rustmc-cov2/oracle-work/subset-stability/` (ensemble
  `coverage93-n6-20260810`, floor 1 of 5).
* **Do not re-run the N = 6 full-corpus selfcheck.** `~/rustmc-cov2`'s set
  (`oracle-work/gameplay-selfcheck/r{1..6}.json`, ~2.5 h of cold boots) is
  intact and is the floor's ensemble. A floor at a new corpus size does **not**
  need new recordings: `ci/corpus-floor.sh --checks` decides which checks are
  projected, so an older recording set yields exactly this tree's scenario set.
  `PROJECTED 92/92` on all six, `scenarios_dropped` empty.
* **Do not declare `a-daylight-detector-inverts-when-right-clicked` unstable.**
  A draft entry existed, written from the subset's 5/15. The corpus ensemble
  measures 0/15, 1 distinct projection, class `stable`, **with the power to
  have seen 5/15** — `(1 - 5/15)^15 = 0.0023 < alpha`. The entry would go
  stale on arrival and `ci/corpus-floor.py adjudicate` would red it. The
  reason is written into `ci/checks/gameplay-unstable.toml` beside where the
  entry would have gone.
* **Do not carry the "720 diverges on window-id drift" explanation forward.**
  An uncommitted full-corpus reading had `720` diverge, blamed on the crafting
  table opening at `window = 17` against our `18`. On this tree `720` has no
  first difference under either lens and the crafting table's diverge is
  `window = 0` on both sides. `609` went `diverge -> match` in `main` between
  the two runs and is the suspect, named as a suspect and not measured.
* **The denominator is 92.** 93 is wrong wherever it appears, including in
  this file's first entry, in §20.6, and in the brief. `87 + 5`, and the
  harness prints `recording 92 scenario(s)`.

**TWO INSTRUMENT DEFECTS FOUND, BOTH OF THE HOUSE KIND.**

1. **`ci/figures.toml`'s `corpus-floor-n6-gpfloors` could not fail.** Its
   `artefact_claim` was the string `floor = 7` against the **generated**
   `ci/checks/gameplay-floor.toml`. Re-stamping that file from a different
   ensemble, over a different corpus, under a different comparator left those
   four characters in place, and `ci/check-figures.py` reported `FIGURES
   GREEN` across the whole replacement. Repointed at §20.9's prose, which
   names the ensemble and the projection; the successor's claim string is its
   **ensemble id**, which moves on every re-measure. **A one-token
   `artefact_claim` on a generated artefact is the shape to distrust.**
2. **Five `BETTER` advisories had been printing unread on every corpus run.**
   `main` had moved five scenarios `diverge -> match` without re-running the
   corpus, so the gate had been saying "update the baseline to lock these in"
   to nobody. All five could have silently regressed. Locked in, after
   checking all five stable at 0/15 on the floor ensemble.

**THE FLOOR IS A DIFFERENT SEVEN AND THE EQUAL DIGIT HIDES A SWAP.**
`water-spreads` leaves (parked — `NOT IN THIS RUN`, so not measured stable,
just not measured) and `a-flower-pot-empties-when-right-clicked` arrives
`NEWLY UNSTABLE` at 9/15 pairs, class `reference` — one of this branch's own
five. Three surviving names re-measure at different rates on the new ensemble
(8->5, 11->9, 11->9 flaps). **Do not quote `7 -> 7` as no movement**, and do
not quote either beside the old `9 of 88`.

**WHAT `[expect]` IS, SETTLED.** It is a claim about the **declared** verdict:
`compare::Baseline::check` reads `Outcome::verdict` and never `Outcome::strict`.
So the eleven strict-unstable names get ordinary baseline lines like everything
else and **nothing is exempted from the ratchet**. The strict reading is
printed beside the declared one every run, and where the instability is
vanilla's own it is registered with its ensemble, N and rate. The distinction
from an exemption list is mechanised, not argued: the declared projection is
one comparator over all 92 with an identity that withdraws the floor's
quotability when it changes, whereas an exemption is per-name and leaves
nothing able to tell an exempted scenario from a passing one.

**THE RULE THIS BRANCH ADDS.** *A subset is not the corpus* was already true
of verdicts — `ci/checks/gameplay-baseline.toml` has said since 2026-08-03
that a baseline entry may only be written from the full gate. It is now also
written down for **stability**, which is where it had never been said and
where it cost a draft entry that would have red the adjudicator.

**`608` IS NOT OURS AND WAS NOT TOUCHED.** It reads `match` on this run and
0/15 pairs on this single-recorder ensemble. That does not disturb the
recorder-suspect diagnosis, it corroborates it: the split that bucketed it was
5/15 overall against 0/10 inside the homogeneous subset, and a homogeneous 15
reading 0 is the same finding at higher N.

**THE SINGLE NEXT ITEM.** `701` is the structural one and is ranked in §20.7:
`use_without_item` is a `(table, state) -> Used` map and **cannot express a
player-dependent rewrite**, so `FenceGateBlock`'s "opened from behind turns to
face you" is unrepresentable rather than unported. The result that closes it
is `701` reading `match` on a full-corpus run without any other verdict
moving. `710` and `790` are ordinary ports behind it
(`DaylightDetectorBlock.useWithoutItem`'s `INVERTED` cycle, and
`FlowerPotBlock.useWithoutItem`'s `Player.addItem` — note `790` will still
carry its `[[unstable]]` entry after the port, because the instability is in
`container_set_slot`'s cumulative state id and not in the mechanic).

---

### ONE LINE SUPERSEDED — 2026-08-11, by branch `corpus-stateid-decoder`

Appended rather than edited, per the append-only convention for these files.

This entry says `790` **"will still carry its `[[unstable]]` entry after the
port, because the instability is in `container_set_slot`'s cumulative state id
and not in the mechanic"**. The first half is no longer true: the state id is
now rebased onto the scenario by `compare::rebase_session_counters` and `790`'s
entry is retired, along with the two others on that field. The *reasoning* was
right and is why the entry could be retired without touching the port — the
instability really was in a different field from the mechanic, and `790` is
still baselined `diverge` for the unported
`FlowerPotBlock.useWithoutItem`.

This entry's **`790` MUST STAY THE HIGHEST-NUMBERED CHECK** rule stands, but on
one leg instead of two: the window-0 `incrementStateId` chain is no longer
inherited *in the comparison*, and the free-slot layout is, which is enough on
its own. Do not renumber it.

The floor figures in this entry (`7/92`, quotable) are stale for the same
reason — see `ci/figures.toml`'s `corpus-floor-n6-coverage92`, now
`quotable = false` pending the queued re-stamp.
