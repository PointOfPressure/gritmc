# Cold start — branch `corpus-third-verdict`

Every `### COLD START` entry written for this branch, newest last. One file per
branch, per the doc-collision-class restructure; see `docs/AGENT-BRIEF.md`.

---

### COLD START — branch `corpus-third-verdict`, off `doc-collision-class` (`1f8f3813`), 2026-08-10

**A corpus scenario that could not fail now says so.** Worktree
`~/grit/work-thirdverdict` (was `~/grit/work-verdict` before the resume), remote
dir `~/rustmc-verdict`. Not merged — the
orchestrator gates. Full write-up: `docs/DETERMINISM.md` §27 (§22-§26 were taken
by the stack that merged under this branch).

**THE FINDING.** `607-container-click-pickup-all-gathers-matching-stacks` was
`declared match` in every run it ever appeared in, with `reference_obs: 7` and
`subject_obs: 7`, and it **passed by comparing two silences**. All seven
observations on each side came from steps 0..5 — the rig. Its step-6
`pickup_all` produced one packet on vanilla's side, `packet:0x60`
(`ClientboundSetCursorItemPacket`, id 96), which `607` did not `observe`, and
that packet's digest is **byte-identical to step 5's** (`67414a774c80560f`, on
both the 2026-08-08 and 2026-08-10 recordings). Vanilla resynced a cursor it
did not change.

**THE SWEEP OF THE CLASS, reproduced on two recordings eight days apart:**
0 scenarios pass on zero observations over the whole scenario (`vacuous`
already covers that, and is at 0); **5 pass where the last `[[step]]`
contributes zero declared observations on both sides — 561, 571, 581, 607,
608.** Four are negative checks by design and correct. 607 was the only
positive assertion in the set.

**JOB 1 — `compare::Verdict::Mute`.** The measured step, the last `[[step]]`,
contributed zero declared observations **on both sides**. Non-baselineable,
counted with `vacuous`/`inert`/`error`. Only a `Match` may be relabelled and
it reads both sides — both constraints carried from `inert`'s 2026-08-03
correction, and both keep it from swallowing a real `Diverge`. Counted after
`inert`, so no existing `inert` number moves.

The four stay green via a declared field, `measured_step = "silent"`. **The
distinction cannot be derived** — 608's measured click and 607's are the same
`container_click` on the same rig — and the polarity is the safety argument:
absent means "must speak", so a check that forgets it goes red, never green.
The declaration suppresses `mute` and nothing else; the silence is still
compared, so a subject that speaks there still `Diverge`s.

**POISONED. `ci/mute-falsify.py`, 16 cases, all green**, against the
`rustmc-cov2` pair (2026-08-10 13:16), seconds, no server booted: baseline has
`mute` = {607} and nothing else; silencing a victim's last step in both traces
makes **that** victim mute (3/3) and moves **no neighbour** (3/3); stripping
the declaration from each of the four makes each mute (4/4); adding it to a
poisoned victim restores its baseline verdict (3/3); silencing step 0 instead
of the last produces no new mute anywhere (3/3). **Case 4 is the load-bearing
one** — without it, "the four stay green" is indistinguishable from "the
verdict never fires".

**JOB 2 — 607 rewritten.** `AbstractContainerMenu.doClick`'s `PICKUP_ALL` arm
(`javap -p -c -constants`, offsets 1611 on) enters the gather at 1667 on three
tests: 1641 bails on an empty cursor; **1649 `slot.hasItem()` jumps straight
to the sweep when the clicked slot is EMPTY**; 1657-1664 bail when it holds a
takeable item. The old file clicked a slot still holding a matching stone, so
1664 jumped out of the arm — the sequence was one no client sends, because a
real double-click sends `PICKUP` first and `PICKUP_ALL` against the slot it
has just emptied. Now: three stone into three chest slots, a plain `PICKUP` on
slot 0, then `pickup_all` on the now-empty slot 0. Three donors, because one
cannot distinguish a gather from an ordinary pickup. It observes `packet:0x60`
— one component, `ItemStack contents`, no state id, nothing to flap.

**NULLS — do not re-run these.**

* **A list of names is not available as the discriminator** and neither is any
  inference from the recording: the four negative checks and 607 are
  action-identical at the measured step. This was looked for before the field
  was added.
* **Counting the measured step after `collapse_repeats` does not work.** That
  filter blanks `step` on the kinds a check names; using it would report a
  silence its own disclaimer had created. Counted before it, with a unit test
  pinning the order.
* **Poisoning a one-step scenario tests the wrong verdict.** Silencing its
  only step empties the whole reference projection, which is `vacuous`,
  decided in `verdict_for` before `Mute` is reached. The victim must keep
  speaking outside its measured step.
* **A victim's shape must be read off the comparator's own counts, not the raw
  trace.** `a-bed-facing-north-puts-its-head-the-other-way` is loud on the
  wire before its last step and silent under its own `observe` list.

**NEXT ITEM, and it is `click.rs`'s and not this branch's:** `pickup_all` is
still unimplemented. 607 can now report on it; whether it currently converts,
diverges or is absent is in the corpus verdict below.


---

### COLD START — branch `corpus-third-verdict`, off `doc-collision-class` (`1f8f3813`), 2026-08-11

**The entry above was written before this branch had a corpus verdict of its
own. This is the run, and the deliverable is closed.** Worktree
`~/grit/work-thirdverdict`, remote dir `~/rustmc-verdict`. Not merged.

**Rebased onto the stack, not onto main.** `doc-collision-class` (`1f8f3813`)
= `main` (`31e4615f`) + 23 commits, not yet landed. The only conflict was
`docs/WORKLIST.md`: the write-up moved out to this file and WORKLIST keeps one
`COLD START INDEX` row. §25 had been taken by the stack, so the section is
**§27**, registered in `docs/claims/DETERMINISM-27-third-verdict-mute.toml`.
`sulfur-cinnabar-r19` later took §30; that does not touch §27.

**GATE.** T0 GREEN on `~/rustmc-verdict`, `skipping: no vanilla` = **0**, 19
generated tables current and stamped. `ci/check-doc-claims.py`: 55 claims,
0 collisions, 0 drift.

**JOB 1 — the verdict declares what it should.** `ci/t2-gameplay.sh`, corpus
**87**, Threadripper, 2026-08-11, ports :26584/:26585:
`declared 67/87 (15 diverge, 5 absent, 0 vacuous, 0 mute, 0 inert, 0 error)`,
`strict 63/87`. `561`, `571`, `581` stay `match`. `608` does not, and it is
**not** `mute` — the census says `0 mute`; it diverges on `container_set_content`
at step 0, its rig step. See §27 and the paragraph below.

**THE POISON — `ci/mute-falsify.py`, GREEN**, against this branch's own
recordings (`~/rustmc-verdict/oracle-work/gameplay/{vanilla,rustmc}.json`,
corpus 87, 2026-08-11), run against the **committed** tree and re-run after the
one fix below. 15 cases pass, 1 UNRUNNABLE and named. Poisoning a victim's
measured step makes **that** victim `mute` (3/3) and moves **no neighbour**
(3/3); silencing step 0 instead produces no new mute anywhere (3/3);
undeclaring a by-design silent check makes it `mute` (3/3); re-declaring a
poisoned victim restores its baseline verdict (3/3).

**JOB 2 — `607` CONVERTS.** `match` -> `diverge`, and the diverge is the
gather: vanilla empties donor slot 1 into the cursor at step 8 while we answer
with `packet:0x60` carrying digest `67414a774c80560f` — the same unchanged-
cursor resync the old file compared against itself. `pickup_all` is still
unimplemented in `crates/rustmc-server/src/click.rs`. `607`'s baseline row is
ratcheted **down** to `diverge`, the second deliberate downward move in that
file after `602`.

**THE CONTROL THAT MATTERS.** This branch changes **no server code**
(`git diff 1f8f3813..HEAD -- crates/` is empty) and does not touch
`compare::project`. The stack-tip comparator, built at `1f8f3813` and carrying
no `mute` verdict at all, scores **the same two recordings** to the identical
census — 67/87, 15 diverge, 5 absent. **The third verdict moved zero verdicts
on this corpus.**

**NULLS — do not re-run these.**

* **RETRACTED 2026-08-11 — the bullet below is wrong; see the entry after
  this one.** `608` IS this branch's. Left standing because the way it was
  wrong is the finding.
* **`608` is not this branch's and must not be fixed here.** Same recordings,
  stack-tip comparator, same `diverge`. `ci/corpus-floor.sh` over six vanilla
  recordings puts it in the **recorder-suspect** bucket — 5/15 pairs disagree,
  **0/10 inside the homogeneous subset**, every disagreeing pair containing
  this run, which is the only one taken on the stack tip. It belongs to the
  stack's gate. This branch owns `ci/checks/gameplay/607` and nothing else in
  that directory.
* **Do not re-derive the floor from this ensemble.** `ci/checks/gameplay-floor.toml`
  is stale — `scenarios` 88 should be **87**, `floor` 7 measures **8** — but
  the ensemble is heterogeneous (two recorders) and two of the eight members
  are `607` and `608`. `607` in particular was *rewritten by this branch*, so
  this run's `607` and the other five runs' `607` are different experiments
  sharing a name. A re-stamp needs a homogeneous ensemble on the merged tip.
  Whoever merges the stack stamps it, not a branch.
* **`STALE PROJECTION` on the floor is not this branch's doing either.** The
  floor declares `proj:58380c9e4cbeae81`; the stack tip is already
  `proj:b385528330fee646` and this branch is `proj:1e154ef8765b7525`. The id is
  a digest over `compare.rs`, `scenario.rs` and `trace.rs`, so **any** branch
  editing those moves it whether or not the projection's semantics change —
  and this one's did not.
* **The floor line never printed in the corpus log.** `ci/t2-gameplay.sh` ends
  at `FAIL: 2 scenario(s) unacceptable` and the floor report sits after it.
  That is the "a gate line after a suite that exits non-zero never runs" hazard
  showing up in reporting rather than in gating; the numbers above came from
  running `ci/corpus-floor.sh` by hand.
* **A case-4 subject that is not `match` at baseline cannot exercise case 4**,
  because `Mute` only relabels a `Match`. `ci/mute-falsify.py` reported that as
  a failure once; it now reports UNRUNNABLE with the reason and refuses if
  fewer than two subjects remain. Do not re-argue this as a weakened control —
  the three surviving subjects are what carries the claim.

**NEXT ITEM, and it is `click.rs`'s, not this branch's:** implement
`PICKUP_ALL` in `crates/rustmc-server/src/click.rs`. The closing result is
`607` going `diverge` -> `match` on a corpus run, with its baseline row
ratcheted back **up**. `608`'s step-0 divergence is a second, separate item
and belongs to whoever gates the five-branch stack.

---

### COLD START — branch `corpus-third-verdict`, off `doc-collision-class` (`1f8f3813`), 2026-08-11 (correction)

**`608` IS this branch's, and the entry above says the opposite. Read this
one.** T0 GREEN, `skipping: no vanilla` 0; T2 RED on `608` and honestly so.

**WHAT I GOT WRONG, AND HOW.** I ran a control that held the recordings fixed
and swapped the comparator — stack-tip binary over my traces, identical census
67/87 — and concluded the third verdict was innocent. It is; that control is
sound for what it tests. But my change also lands in the **recordings**, and
that control is blind to it by construction. The stack tip scores `608` `match`
on *its own* traces. I varied the wrong thing, and `ci/corpus-floor.sh`'s
"recorder-suspect" label was telling me so while I read it as exoneration.

**THE BYTES — every gameplay recording on the box, 39 pairs, 2026-08-11.**
`rest` is baked into the trace by the recorder, not computed at compare time,
so no projection could have moved it:

* vanilla's `608` step-0 `rest` is `8aac16faed2ec626` in **37 of 37**
  recordings made with the old `607`, across corpus sizes 86/87/88/92/93, and
  `b4b699a3a20338fc` in exactly the **2** made with the rewritten one;
* **ours is `8aac16faed2ec626` in all 39** and never moves.

The rewritten `607` moved vanilla and left us where we were.

**THE MECHANISM.** `607` sorts immediately before `608` and the whole run
shares one connection. Vanilla's `PICKUP_ALL` gathers three stone onto the
cursor; we no-op it. The per-scenario reset closes the menu, and vanilla's
`doCloseContainer` drops what is carried while we do not — which is `611`,
already baselined `diverge` and failing with the identical signature. `608`
then opens a chest and its `container_set_content` carries an inventory that
differs by three stone. `608` is reading `607`'s defect through a reset that
depends on the server under test being correct.

**WHAT CLOSES IT:** a server-independent inventory reset in `record.rs`,
beside the position and sneak resets already there — the same ruling
`552`/`553` already forced, whose comment says a scenario-local fix "works
until somebody writes the next scenario, so it is fixed here". Not done on
this branch because it changes every recording and validating it costs a full
corpus run, and the box is on the stack's gate.

**NULLS — do not re-run these.**

* **Do not look for a scenario-local fix to `607`.** Renumbering it past `608`
  moves the leak to whatever sorts next; and no rig ending in `PICKUP_ALL` can
  leave both sides carrying the same thing, because carrying different things
  is the measurement. `record.rs`'s own comment already rejected this class.
* **Do not baseline `608` to `diverge`.** It would publish `607`'s defect under
  `608`'s name and flip back the day either `PICKUP_ALL` or drop-on-close
  lands. Left `match`, and left red.
* **The comparator and the projection are exonerated, and that part stands.**
  `compare::project` untouched; `Mute` only relabels a `Match`; stack-tip
  comparator gives the identical 67/87 on the same traces.
* **Not recorder instability.** Two independent readings of the tip are
  byte-identical (`~/rustmc-stack` `428ce367` :26782/:26783, and this run).

**THE DECISION THAT IS NOT MINE:** either spend one corpus run validating the
`record.rs` reset — which is the fix — or land the stack with `608` red and
the reason recorded. `607`'s own conversion (`match` -> `diverge`) is real
either way and its baseline ratchet stands.
