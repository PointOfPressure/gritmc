# fix-flappers-2 — cold start (handoff; kept current)

Branch `fix-flappers-2` off main a002baf78 (fix-flappers merged). Work tree
`/home/sayer/grit/work-flappers`; box dir `~/rustmc-flappers`
(`RUSTMC_REMOTE_DIR=rustmc-flappers`); scripts and evidence `~/grit/flappers/`.
Push after every commit: `git push origin HEAD:refs/heads/fix-flappers-2`.

## On the branch

1. `ci/handoff-merge-tail-falsify.sh` case [4] (bcb463ebe): the kill waited for `main` to
   contain the branch, which `git merge --ff-only` reports before it has finished the index,
   so under load the kill landed inside git (index.lock left, re-run REFUSED; structures-3 T0,
   `~/rustmc-sl2-logs/t0-t0tip2.log:430-431`). The hung-push shim now writes its pid to
   `$WORK/push-entered`; the case kills on that marker and that pid only (it used to
   `pkill -9 -f 'sleep 600'` box-wide). Proof: with a proof-only shim that holds index.lock
   3 s after moving the ref, main's version fails 2/2 with the structures-3 pair of FAILs,
   this version passes 37/37 2/2; a handoff.sh mutant that refuses instead of ALREADY-MERGED
   still reddens case [4].
2. Gate-dir pollution (a1ae77cd8): `gen-gameplay-tables.sh --drop-foreign SEED` puts every
   stamped table this tree does not require back to the seed (or removes it); handoff stage 3
   runs it before the usual check-and-regenerate. Case: ranged's T1 read nether-parity's
   `survival.json` (gate-ranged-20261004-183154.log).

## State

Pollution proof done as expected (flap-chain7): 5/5 on the seed table, 4 FAILED on
nether-parity's, still 4 FAILED after the old stage 3, 5/5 after `--drop-foreign`.
That chain's keep-going T0 on 1b2147cf3 had ONE red stage, `ci/shell-footguns.py`: the
`pkill -f 'sleep 600'` declaration this branch made stale; dropped in 01939172a.
`flap-chain8` (`~/grit/flappers/chain8.sh`): keep-going T0 on the tip, output
`~/grit/flappers/t0d.log`.

## Next

Read `~/.rustmc-detached/flap-chain8.log`; if T0 GREEN, tell the coordinator the sha.
