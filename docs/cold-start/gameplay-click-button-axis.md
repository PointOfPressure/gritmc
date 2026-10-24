# Cold start: `gameplay-click-button-axis`

Item stacks and containers, approached through the one axis nobody had counted.

## What this branch is

`ContainerInput` has seven values and the corpus sends four, which reads like
four sevenths of the click surface. It is not the surface.
`AbstractContainerMenu.doClick` takes `(slotNum, button, ContainerInput,
Player)` and **the button means something different in every arm**. Counted
against the jar there are **28 behavioural `(input, button)` classes** and the
corpus asks **6**.

`ci/click-surface.py --report` prints the table. `--check` gates it.
`ci/click-surface-falsify.py` is the positive control: five doctored copies of
the real `javap` output that `--check` must refuse, and the real one it must
accept.

## The bug this found, and why nothing else could have

Offsets 561-581 of `doClick`. `PICKUP` and `QUICK_MOVE` enter **one arm**, and
`if (button != 0 && button != 1) return` sits inside it, above the re-test at
664 that separates them. This server dispatched `QUICK_MOVE` above the guard,
and `do_click_quick_move` does not take `button`, so nothing below caught it
either. Every out-of-range button shift-clicked a stack vanilla leaves alone.

**The corpus could not have found it.** Every `quick_move` step in
`ci/checks/gameplay` sends button 0 — four steps in three files — because the
corpus asks what `oracle-runner` sends and that is what `click.rs` names. That
loop is closed and its agreement is a fact about this repository. The jar is
downstream of neither side, which is the whole reason to ask it.

## Hazards, in the order they will cost you time

1. **`ci/unlanded.sh`, never `git rev-list --count`.** `container-quick-move`
   reads 9 commits behind main by `rev-list` and **0** by `git cherry`. It is
   fully landed. Sizing it any other way invents a backlog.
2. **A clean rebase is not a build, and a green background job is not a green
   gate.** Two of this branch's background runs reported exit 0 while the
   command inside had failed — once `cargo: command not found` (the background
   shell has no `~/.cargo/bin`), once `T0 RED` — because a trailing `tail`
   supplied the exit code. Put the real `$?` in the log, not in an `echo`
   after a pipeline.
3. **The gate directory needs seeding before its first T0.** `~/rustmc-items2`
   had no `oracle-work`, so T0 died in `tick-section-classify.py` on a missing
   jar and said nothing about the branch. `ci/handoff.sh` stage 1 does this;
   running `ci/t0-remote.sh` directly does not. Link every entry of
   `~/rustmc/oracle-work` except `gameplay`, `gameplay-selfcheck` and
   `server-failures`, then `cp -rn` `data/vanilla`.
4. **An untracked file in your worktree is rsynced into the gate.** A reserved
   claim `.toml` with no matching heading made T0 RED under
   `CLAIM-NO-HEADING` on a branch that had nothing to do with it.
5. **`about` is outside the ledger digest.** `semantic_digest` serialises the
   parsed document with `about` removed, so a prose fix in a scenario cannot
   invalidate its red-when-broken evidence. Fix wrong prose freely.

## What is measured here and what is only recorded

Measured, with N stated in §81:

* the 28/6/22 split, parsed from the jar rather than quoted;
* the `QUICK_MOVE` guard divergence, shown red before green by reverting the
  guard and re-running the unit test;
* `onTake`'s fan-out — eight paths inside one `doClick` call;
* 39 `container_click` steps across 15 files, which is what `720`'s "thirty-four
  checks" should have said in either unit;
* 64 frozen of 109 checks over 66 recordings, and which three are in this area;
* `killed_by` disagreeing across three recordings for 32 of 108 checks.

Recorded and **not** acted on:

* **the offhand `SWAP` arm.** `button == 40` is `Inventory.SLOT_OFFHAND` and
  vanilla routes it through `EQUIPMENT_SLOT_MAPPING` onto `EntityEquipment`.
  `do_click_swap` declines it for two stated reasons and both have expired —
  `Hotbar` models an offhand, and `swap_offhand` exists in the step vocabulary
  (`510` uses it), so `swap_offhand` then `container_click input = "swap"
  button = 40` is reachable today. Left declined because implementing it
  unmeasured is what the doc was right to refuse; what changed is the reason,
  not the verdict.
* **`610-container-click-quick-move-with-an-out-of-range-button-changes-nothing`**
  is written and is **not committed**. It is the corpus check that would have
  caught the guard bug: the same click at the same slot with button 2 then
  button 0, so an over-eager server answers the two steps in the opposite order
  to vanilla. It cannot land until a recording gives it a ledger row, and the
  recording budget for this branch was one. The file is the only thing standing
  between this fix and a gated check — pick it up first.
