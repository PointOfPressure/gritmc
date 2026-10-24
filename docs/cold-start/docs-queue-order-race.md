# COLD START — `docs-queue-order-race`

### COLD START — branch `docs-queue-order-race`, 2026-08-18, area "gate throughput"

Two measured facts about the merge route, recorded in `docs/CI.md` because the
instruments that produce them live **outside this repo** and a correction that
lives only in the file it corrected does not travel.

**1. The queue ORDER is state, and it was being lost silently.** The chain is
serial at ~80 min a gate, so queue position is worth roughly `position × 80 min`
to a branch. `preflight.sh` truncated `preflight.tsv` in place and appended
across its whole ~2 min sweep; `chain-keeper.sh` orders the queue from that file
under `[ -s preflight.tsv ]`; `supervisor.sh` re-runs preflight on the same tick
the keeper rebuilds the queue. Both are conditioned on "no chain is running", so
they race by construction.

    07:15:24  supervisor: preflight was 426m stale — re-running
    07:15:25  keeper: relaunching over 46 branch(es)      <- read the empty file
    preflight.tsv mtime 07:17, queue7.txt mtime 07:15

`gate-stage-timing` — **first** in the keeper's `PRIORITY` list, GREEN in
preflight, one commit — came out at **position 7 of 46**. Repaired twice over,
in the untracked scripts: `preflight.sh` publishes by atomic rename and keeps
the previous file rather than publishing an empty sweep, and the `PRIORITY` pin
is hoisted out of the preflight conditional because it is a hand-written branch
list that needs nothing preflight produces. Evidence: `preflight-falsify.sh`
case 6, which reconstructs the shipped defect by patch and asserts it destroys
the file while the repair keeps it. `chain-falsify.sh` 33/33 after the keeper
change.

**2. The first measured stage budget for a merge gate**, assembled from three
independent sources rather than one:

    1 prepare              1 s              STAGE-TIME markers, N=1
    2 rsync                2 s              STAGE-TIME markers, N=1
    3 tables               3 s              STAGE-TIME markers, N=1
    4 T0                 149 s   ( 4%)      STAGE-TIME markers, N=1
    5 T1                 ~19 min ( 30%)     residual, corroborated
    6 T2 corpus, in-lock 43.3 min ( 66%)    refcache probe, wall − lock-wait
      total              65.2 min           mean of 127 GREEN gate logs

Stages 1-4 come from the **only** gate log in existence carrying `STAGE-TIME`
lines: `gate-stage-timing` gating itself before the 00:15 power-off killed it.
The 65.2 min total is measured independently over 127 green logs. Stage 5 is
the residual **and** is corroborated: the five `T1-GATE RED` gates reach their
verdict at 21.3 min, less 2.6 min of stages 1-4, leaving 18.7 min of T1 before
failure — a lower bound on a full run. Two routes, ~19 min.

Corrected a stale target in the table it sits under: T1's `~11 min` was
understated by ~8 min.

**The consequence worth acting on: T2 is two thirds of every gate and the
reference cache halves it** (21.6 cached vs 43.3 uncached), so the mean gate
goes **65.2 → 43.5 min — 1.50x on every gate, from one line.** That is larger
than any other throughput change currently proposed. See
`docs/REFERENCE-CACHE-FLAG.md`.

**WHAT I KILLED AS A NULL.** Do not reconstruct gate cost by differencing other
files again — that is how every previous figure here was made, and it is why
`~11 min` survived. Stages 1-4 are N=1 and are quoted as such.

**The single next item.** Land `gate-stage-timing` and re-take this table from
per-stage timing on *every* gate, which is the only way to say where inside T1
and T2 the time goes, and how the shares move for a worldgen-only branch.
