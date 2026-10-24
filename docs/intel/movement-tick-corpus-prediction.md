# Pre-registered prediction — branch `movement-tick`, 2026-08-06

Written and committed **before** `ci/t2-gameplay.sh` was run on this branch.
The result is recorded underneath, unedited, whatever it says.

Baseline: `ci/checks/gameplay-baseline.toml`, 86 entries, **53 match, 27
diverge, 6 absent** — the figure `entity-move-collision` measured and this
branch's parent carries.

## The prediction

**Nothing moves. 53 / 27 / 6, identical to baseline, in both directions.**
**0 vacuous, 0 inert, 0 error.**

| check | baseline | predicted |
|---|---|---|
| `sand-falls-when-unsupported` | diverge | **diverge** |
| `dropping-an-item-throws-it-on-the-floor` | diverge | **diverge** |
| `a-player-that-leaves-the-ground-is-resynced` | diverge | **diverge** |

## Why nothing should move, stated so it can be wrong

`crate::attributes`, `crate::movement_props` and `crate::physics` are reachable
from no runtime path. They are `pub`, they are compiled, they are unit-tested,
and the only things that call them are their own `#[cfg(test)]` modules. There
is no entity tick, no caller in `status::serve`'s game loop, and no change to
any packet this server builds.

That is the same position `entity-move-collision` was in and it is stated the
same way, because the alternative — wiring physics under three checks before
the mechanism above it exists — is what would make unmeasured things look
measured. What this branch adds instead is the finding that two of those three
checks cannot be closed at all; see
`docs/intel/movement-tick-three-checks.md`.

## What this run is actually for

Not the count. **The count is predicted to be unchanged and a matched
prediction here is nearly arithmetic.** The run exists to close one specific
risk that no unit test can:

**This branch adds TWO new `data/vanilla` tables.** `ci/t2-gameplay.sh` exits 4
on a table that is absent, foreign, or older than the dumper that produces it,
and it does that per table over whatever `ci/gen-gameplay-tables.sh` derives
from the server crate's `open` calls. `entity-move-collision` established that
*one* new table goes through that mechanism untouched; two, added by two
different generators, is a different arm of the same question — in particular
because `movement_props.json` is the first extract in this tree to read the
**generated data reports** rather than the runtime registry, and the first to
depend on `-Drustmc.data` pointing somewhere real inside a gate rather than
inside a hand-run script.

`./ci/gen-gameplay-tables.sh --check` on the remote before this run reported
**15 tables required by crates/rustmc-server**, with `attributes.json` and
`movement_props.json` both discovered through the existing derived-path grep
and **no edit to the gate**. Both were then generated and stamped.

## What would falsify it, and what each would mean

1. **Any check moves at all.** This branch would be doing something at runtime
   that this note says it cannot, and the first thing to check is whether
   `physics`, `attributes` or `movement_props` acquired a caller.
2. **The run exits 4.** A tooling failure, not a movement, and it must not be
   reported as either. This is the falsifier the run is *for*; see above.
3. **A check moves and a re-run does not reproduce it.** Flap, not signal —
   and this branch has just measured that at least one check in this corpus
   flaps on the *reference* side, so the bar for believing a single-run
   movement here is higher than it was yesterday, not lower.
4. **Any vacuous / inert / error appears.** Those say nothing about the server
   and fail the gate regardless of the count.

## Result — the run did not finish inside this session

`ci/t2-gameplay.sh`, `~/rustmc-movetick` on the Threadripper, started
2026-08-06. The **vanilla arm completed, all 86 scenarios**. This server's arm
reached **74/86 and was then killed**: `/tmp/t2g.log` on the box ends

```text
  recorded 74/86  swinging-an-arm-shows-to-a-bystander
  Terminated
```

and no `rustmc-movetick` process survives. The run never reached its compare
step, so **there is no report and no verdict for any check** — not a partial
score, none at all.

**The cause is known and attributed.** The orchestrator ran
`pkill -f rustmc-movetick` on the box, deliberately, to stop this corpus
contending with the `redstone-power` **merge gate**, which was in its own
corpus at the time. The pattern matched this run's `t2-gameplay.sh` and its
`oracle-runner` by working directory. It was not known at the time that this
arm was at 74/86.

This session's own `pkill`, against `movement-falsify`, is exonerated: its
pattern cannot match `t2-gameplay.sh`, and the reasoning that said so was
correct.

Recorded as attributed rather than as "cause unknown" **because the wording
matters more than the fact**: a doc saying a run died for unknown reasons
sends the next agent hunting a phantom, and this project has paid for that
shape of caveat before.

**No corpus figure is quoted here and none exists to quote.** The prediction
stands unmeasured. That is the honest state and it is
better than a number taken from 41 of 86 scenarios, which is not what
`gameplay-baseline.toml` is a floor over.

What the run *did* close is falsifier 2, which was the reason for running it:

* `./ci/gen-gameplay-tables.sh --check` derived **15 tables required by
  crates/rustmc-server**, with `attributes.json` and `movement_props.json`
  both discovered through the existing derived-path grep and **no edit to the
  gate**;
* `./ci/gen-gameplay-tables.sh` then generated and stamped both, reporting
  **15 tables current**;
* `ci/t2-gameplay.sh`'s provenance preflight accepted the tree and proceeded
  to record. **It did not exit 4.** Two new extracts from two new generators,
  one of which is the first in this tree to read the generated data reports
  rather than the runtime registry, went through the mechanism untouched.

### Conditions, recorded because they would matter if anything had moved

The box was carrying another agent's corpus (`~/rustmc-gate-tables`) and a
worldgen server throughout, and this session's own falsification sweep for
part of it — niced to 19, but present. Load average sat between 1.5 and 3.
This server's arm ran at roughly **one scenario every two to ten minutes**
against the vanilla arm's two a minute — and it got *slower* as a second
corpus started up beside it.

**The contention is wall-clock, not CPU, and that is why throughput collapsed
rather than merely halving.** Load average was **0.02** with two corpora
running: the harness spends its time inside `settle_ms` waits and vanilla
boots, so two runs do not share a busy CPU, they each wait out the other's
timers. A `uptime` reading of an idle box is therefore *consistent with* two
runs starving each other, and reading it as "nothing is contending" is exactly
backwards. Anyone diagnosing a slow corpus here should count concurrent
harnesses, not look at load, which is chunk generation at a fresh
workspace on a contended box rather than anything about the branch.

**Several checks carry `Timing::Bucket` with 500 ms buckets.** A movement
measured under that load would need a re-run on a quiet box before being
believed, in either direction. Nothing on this branch has a runtime caller, so
the prediction is still "nothing moves" — but it is now an unmeasured
prediction, and the re-run is the next item.

### One methodological blemish, stated rather than buried

The working tree was `rsync`ed to the remote **while this run was in
progress**, three times, to run the falsification sweep against the newest
code. The scenario definitions did not change (the two probe files were
deleted before the run started), `data/vanilla` is excluded from the rsync,
and nothing that changed is on a runtime path — but our arm may have been
recorded against a server binary rebuilt mid-run from slightly newer source.
The re-run should be done on a tree nobody is touching.
