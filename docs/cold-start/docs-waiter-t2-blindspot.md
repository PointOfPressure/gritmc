# COLD START — `docs-waiter-t2-blindspot`

### COLD START — branch `docs-waiter-t2-blindspot`, 2026-08-18, area "gate throughput"

**`docs/AGENT-BRIEF.md`'s recommended waiter broke the rule stated three lines
below it.** The block tells every agent: *"Match the failure signatures too, not
just the success one. A waiter that fires only on green is silent through a
crash, and silence is indistinguishable from still-running."* The example
directly above it matched `T2-GAMEPLAY (GREEN|REFUSED)`.

`ci/t2-gameplay.sh` has **no `T2-GAMEPLAY RED` string**. It prints GREEN on
success, REFUSED when it declines to measure, and on a genuine corpus
regression it exits non-zero saying neither. So the recommended waiter was
silent through the one failure the stage exists to catch — in the stage that is
66% of a gate.

**Measured against the real logs, not argued.** On the four gate logs carrying
a handoff RED summary — `chain-cascade-signs` twice,
`gate-skips-and-scenario-verbs`, `gate-be`, three of them at `t2=1/0` — the old
pattern matches **zero** lines and `^RED ` matches all four.

**Two fixes, and the second is the one that generalises.**

1. The pattern gains `^(RED|MERGED|REFUSED|INFRA) ` — the verbs
   `ci/handoff.sh` prints in its own summary, which is the one place a verdict
   is stated regardless of which stage produced it. It also gains
   `T2-GAMEPLAY RED`, the sentinel added on `ci-t2-emits-a-red-line`. **Both
   anchors are kept deliberately:** the sentinel is precise, the summary also
   covers a stage dying before T2 ever runs. If that branch does not land, the
   summary anchor alone still catches 4 of 4.
2. **A waiter on a DIRECT `ci/t2-gameplay.sh` run cannot be fixed with any log
   string** — there is no handoff summary in that log. The brief now shows
   writing your own sentinel and matching that:

   ```bash
   ( ./ci/t2-gameplay.sh > ~/t2.log 2>&1; echo "T2-EXIT=$?" >> ~/t2.log ) &
   until grep -q '^T2-EXIT=' ~/t2.log; do sleep 120; done
   ```

**The general rule it now states, which is the reason this is worth a branch:**
a waiter keyed on strings a script chooses to print can only see the outcomes
that script has a string for. Ask which outcomes have no string before arming
one.

**WHAT I KILLED AS A NULL.** Do not "fix" this by matching `T2-GAMEPLAY` more
loosely (e.g. dropping the verb). A pattern that matches the *progress* lines
fires the moment recording starts and is worse than the bug.

**The single next item.** None here; this branch is one file. The related work
is `ci-t2-emits-a-red-line` (the sentinel at source) and `ci-gate-batching`
(the classifier that read T2 as 143 GREEN / 0 RED and started all of this).
