# fix-flappers — cold start (handoff; kept current, last update 2026-10-04 after the reset)

Branch `fix-flappers`, rebased onto main 6828dcb7b (main had landed the recorder
fix itself, so that commit dropped out), pinned first in the gate chain.
**NOT T0-green yet; the coordinator holds it out of the queue until it is.**
Work tree: `/home/sayer/grit/work-flappers`; box dir `~/rustmc-flappers`
(`RUSTMC_REMOTE_DIR=rustmc-flappers`); scripts and run evidence `~/grit/flappers/`.
Push after every commit: `git push origin HEAD:refs/heads/fix-flappers`.

## State (2026-10-04 ~15:00)

Gated 14:14-14:36 and went RED at T0 on one stage: `UNFALSIFIABLE-FALSIFY REFUSED — sweep
wrote no ledger` (gate-fix-flappers-20261004-141458.log ~765). Cause: T0 ran
`ci/unfalsifiable-falsify.py` with the PREVIOUS gate's `target/release/oracle-runner`,
which does not know 086's `left_handed` egg field. Fixed in 844fae8b2 (T0 now builds
this tree's release oracle-runner before the falsifier when a recording pair exists).
Local T0 had skipped that stage (no pair in `~/rustmc-flappers`), which is why it was green.

Evidence: `~/grit/flappers/ufz/{stale,mine}.log` — the falsifier over a copy of the
stack's pair: stale binary rc=2 with the gate's refusal; this branch's binary rc=0
UNFALSIFIABLE-FALSIFY GREEN. The branch is ready to gate again at its tip.

## What is on the branch

| flapper | cause | fix |
|---|---|---|
| 136 console-rotate | a rotation on the 60-tick heartbeat tick goes out as a zero-delta PosRot, filed as a heartbeat and dropped | recorder splits it (on main as 6828dcb7b) |
| 083 tempted pig in a pit | 082's free bat flies into 083's tracking range (ref 95ef64ea: 73.6 blocks, 15 foreign moves) | 082 hatches the bat in a sealed cell under glass  |
| 086 / 087 bats | `Mob.finalizeSpawn` left-handed roll, 1 in 20, on both servers | egg carries `LeftHanded:1b` (harness + server; 0b was not enough, see FINDINGS) |
| 129 hollow fill | 28.5 MB `block_props.json` parsed lazily by the first /fill, on the console thread; missed the window at a shard head | table loaded in `status::listen` before readiness |
| 459 sticky piston | chunk-order of two block_updates | already fixed on main (bc413a1fb), no red since |
| tick test | wall-clock assertion, 1.321 s under load | fake-clock `TimeSource` tests, falsified by two mutants |
| gate: 3/8 vanilla shards "never confirmed gamerule" | `sleep 1` then one grep, while a loaded vanilla was still in its post-`Done` chunk save | `console_confirm` polls for confirmation or refusal up to 60 s, registered in `liveness-proofs.toml` (5b846e7bb) |

`compare::scenario_starts` (origin/ranged d40c72df0) explains none of these; not cherry-picked.
087's baseline is back to `match`.

## Evidence taken (subset runs, shard order, `~/grit/flappers/runs/`)

* shard 0 prefix 010..087 with the final rig (LeftHanded:1b, sealed bat), s0b-1..3:
  083, 086, 087 match 3/3. Non-matches only `spawn-egg-makes-a-mob` (3/3, baselined diverge)
  and `082` (2/3, baselined diverge for its vanilla pig flap, 4 of 13 references).
* shard 1 prefix 126..136, s1-1..3: 11/11 match each (129 at index 3, 136 at index 10).
  These ran on the binary BEFORE the block-table warm.
* 129 first-in-session: the ranged lane's 4 runs matched before the fix (rgc-G1/G2), at
  564-793 ms; the post-fix runs (s129w, expect ~400 ms) were NOT taken — the halt stopped them.

## Next steps, in order

1. Read `~/.rustmc-detached/flap-chain6.log`. If T0 lists no red stage, tell the
   coordinator the branch is green with the sha. Otherwise fix the reds named in
   `~/grit/flappers/t0.log` (search `T0 KEEP-GOING`), commit, push, re-run T0.
   (How the re-sweep works: splice `runs/s0b-1` into `~/grit/flappers/base` with
   `splice.py`, sweep into a temp copy of the ledger, copy the three rows back with
   `rows.py`, `--check`.)
2. (done) 129 proof: `runs/s129w-1..3`, ours at 400 ms like vanilla.
4. Not done, worth an area of its own: 082's pig re-pairing lands either side of its 3,000 ms
   settle in vanilla; it is baselined diverge, which the flapper rule says it should not be.
