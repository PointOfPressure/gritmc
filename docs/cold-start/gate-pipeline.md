### COLD START — branch `gate-pipeline`, off `main` (`606551b54`), 2026-09-30

**Gate throughput: native keep-going T0, measured stage timing, parallel T1,
prefetched T0 checks, and a merge-queue chain.** Box: sayer-desktop, remote dir
`~/rustmc-gpipe`. Every figure below has its log under
`/home/sayer/grit/gp-timing/`.

**Where the train-a gate's 146.8 min went** (`gate-train-a-20260930-082310.log`):
stages 1-4 16.1, T1 24.7, T2 build 1.0, T2 lock WAIT 53.2, T2 recording 39.9,
stage-7 lock WAIT 8.2, persistence ~3.2. Lock waits are 42%; stage 7 re-queues
for the lock stage 6 released 20 s earlier (a re-entrant acquire in
`ci/corpus-lock.sh` would fix it; not this lane's file).

**What landed on the branch**

* `ci/t0-stages.sh` (sourced by `ci/t0.sh`): `T0-STAGE line= wall_s= cpu_s=
  reds= label=` per stage, stages cut mechanically at each column-0 check
  paragraph; `T0_KEEP_GOING=1` (errexit stays on for every subshell; the main
  shell survives a red via ERR trap + DEBUG re-arm; `exit` shadowed; verdict
  guard before `T0 GREEN`); prefetch (`t0_spawn`/`t0_prefetched`, each job in
  its own rsync snapshot; `T0_PREFETCH=0` off). Evidence
  `ci/t0-keepgoing-falsify.sh` (in T0): 42 cases, 9 mutants rejected.
  Real tree: three injected reds -> keep-going listed all three (plus one real
  red of this branch) and exited 1; default stopped at the first.
* `ci/handoff.sh`: `GATE-STAGE stage= name= wall_s= cpu_s= lock_wait_s= rc=`
  for stages 2-7.
* `ci/t1-par.sh` + `ci/t1-par-runner.sh`: T1's test binaries run 12 at a time
  through a cargo runner, the log rebuilt in serial order. **T1 1445.1 s ->
  400.2 s**, same `T1-GATE GREEN — 158 test binaries, 3324 passed, 10 ignored,
  2 known red`. Evidence `ci/t1-par-falsify.sh` (in T0), 10/10.
* `ci/order-blind.py`, `ci/mob-projection-search.py`: `--check` reads a
  committed population/corpus instead of every lane's recordings; falsifier arms
  prove a foreign recording moves the box reading and not `--check`.

**Merge queue (local infra, outside the repo):** `/home/sayer/grit/mq-chain.sh`
(gate-chain.sh + batching: stack preflight-GREEN branches, one `ci/handoff.sh`
over the stack tip, land all on MERGED, bisect by halves on RED; never merges
ungated), `mq-falsify.sh` (8 cases, 7 mutants), keeper switch
`KEEPER_MERGE_QUEUE` / `state/merge-queue.off`. Live since 15:15:23.

**Nulls, so nobody re-runs them:** the javap `--check` readers are small
(2.4-7.6 s alone) — their T0 stages are big because of the `-falsify` script in
the same paragraph; `entity-broadcast-order.py --check` cannot be prefetched
(needs `target/`, refused loudly in a snapshot); `regiondiff-falsify` and
`be-emit-falsify` cannot either (target/, in-tree source edits + cargo).

**Next item:** the stage-7 lock re-queue (8.2 min on train-a) needs a
re-entrant `corpus_lock_acquire`; and T0's remaining serial giants
(server-level-gates 85 s, regiondiff 74 s, light_golden 54 s) are the next
prefetch candidates once they can run against a read-only `target/`.
