### COLD START — branch `corpus-parallel`, off `main` (`1049a12d3`), 2026-09-24

**Area: gate throughput — corpus runs concurrently instead of one at a time.**
Worktree `~/grit/work-corpuspar`; box trees `~/rustmc-cpar-a`, `~/rustmc-cpar-b`.

**THE COLLIDING RESOURCE.** Not the box. Recorders sharing ONE vanilla: every
recorder logs in as `oracle`/`oracle_eye`, so a second one is kicked with
`multiplayer.disconnect.duplicate_login` (3.6 s), and a vanilla world that
already hosted a recording errors half the next one (8-scenario set: solo
97.4 s / 0 errors; second recording on the same world 203.1 s / 4 errors,
player saved at x=-191.5, walk east through old rigs refused). The 2026-08-06
runs shared one hardcoded port pair; the 08-07 "2 client joins for 7 minutes"
is what a healthy run shows too. `docs/FINDINGS.md` 2026-09-24.
Residual shared resources found and fixed: fixed default ports in four lock
takers (`ci/corpus-ports.sh`), same-tree `oracle-work/gameplay` (per-tree lock
under a pool). Also found: `t2-gameplay-subset.sh` failed every 26.2 run on a
dead `Set the time to` grep.

**WHAT LANDED ON THE BRANCH.** `RUSTMC_CORPUS_SHARDS=N` in `ci/t2-gameplay.sh`
(contiguous shards, N isolated pairs, merged in corpus order, refcache keyed on
N); `ci/corpus-shard-equivalence.py` (serial vs sharded vs serial control, both
projection directions per side + verdicts; selftest 10/10 in T0; `--check`
refuses an unproven default); `ci/checks/corpus-shard-equivalence.toml` (the
evidence — see its commit for the numbers); `ci/corpus-lock.sh` bounded pool
(`RUSTMC_CORPUS_LOCK_POOL`, slot 0 = `run.lock`, gate still first, one run per
tree), with 4 new falsify cases + 3 injections and exclusion-falsify cases 11-12.

**NULLS.** Load average and CPU are not the constraint (a recording is wall
clock: settle, QUIET, 10 blocks/s walk). Do not re-open "concurrent runs wait
out each other's timers" — no such mechanism exists; it was the shared server.

**NEXT.** Balance shards by cost rather than by count if the longest shard
dominates (per-shard walls are in each run's `shard-i/*.log`); re-take the
equivalence ledger whenever `record.rs`'s workspace layout changes.
