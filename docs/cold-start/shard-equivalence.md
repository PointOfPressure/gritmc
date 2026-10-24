### COLD START — branch `shard-equivalence`, off `main` (`606551b54`), 2026-10-01

**Area: gate throughput — make a sharded corpus run measure what a serial one does.**
Worktree `~/grit/work-shard-eq`; box trees `~/rustmc-sheq` (all proof runs),
`~/rustmc-sheq-dev` (builds, T0). Results and every run's stamped `t2.log`:
`~/grit/shard-eq-results/{base,fix1}/` (main / isolated). Ledger:
`ci/checks/corpus-shard-equivalence.toml` (six runs added).

**MEASURED.** Full corpus, 179 scenarios, lock hold: serial 4798-5031 s,
8 shards 486-499 s (10.1x), 4 shards 724-745 s (6.7x). On main every shard
effect is a predecessor effect (19 at 8 shards, none in the first shard): the
player's inventory/held item/facing leaked between scenarios (607, 781/782,
530, 788), `container_set_data`'s menu id sat undecoded in `rest` (every menu
check), the waypoint's absolute position sat undecoded in `rest` (540), and
710's detector update landed by tick phase. All four fixed; at 0ec801e1f the
remaining effects (4 at 8 shards, 2 at 4) are ALL in the first shard — same
predecessors, coordinates and fresh session as the serial run — and all in the
080-089 mob cluster, whose verdicts also differ between serial runs (083, 084
across four). `corpus-shard-equivalence.py` now prints that attribution.

**ALSO LANDED.** `ci/corpus-lock.sh hold -- CMD` (one lock across several
harnesses; inner acquires are served only to a process inside the hold, checked
by anchor liveness, the kernel's fd and ancestry) and `ci/handoff.sh` running
stages 6+7 under one hold via `ci/gate-corpus-stages.sh` — the 8.2-min stage-7
requeue on train-a's gate. Falsifier: 29 cases / 24 injections, GREEN.

**NULLS.** Absolute position and connection age (a hypothesis for 530/540/788)
stopped mattering: in the isolated runs those scenarios also differ between the
two SERIAL rustmc runs, so the control covers them. The layout knobs written to
test it (`ORACLE_INDEX_BASE`, `ORACLE_FRESH_SESSION_EACH`) were reverted
unmeasured. 459's flap was vanilla's identity-hash chunk broadcast order, fixed
on `fix-459-rig`. 083's extra pig is leftover world state (080-082's pigs),
not the player reset.

**NEXT.** The proof's floor, not the harness: an N=2 serial control cannot
bound ten random mob scenarios. Either a third serial control (`--control`
taking several, union of their disagreements) or the mob cluster's flaps
measured into `gameplay-floor.toml` by `ci/corpus-floor.sh`; then re-take one
isolated 8-shard run. Entity cleanup between scenarios (rustmc's console has no
`kill`) is what would make 083 deterministic serially.

### COLD START — branch `shard-equivalence`, 2026-10-01 (closing)

**RESULT: RED at 4 and 8 shards on one check; landing at shards = 1.** With a
third serial control (the proof now takes `--control` repeatedly and uses the
union) every isolated sharded run leaves exactly one effect, in the first
shard: `084`, whose strict verdict follows a pig ambient sound landing in its
window on either side (vanilla 3, ours 2 of ten full recordings) — chance, by
`Mob.baseTick`'s per-tick roll from 0 at spawn, not history and not load. It
is still a verdict a sharded gate could read differently, so nothing is
proven. Lock hold, full
corpus: serial 4976-5031 s, 8 shards 499-521 s, 4 shards 745 s.

**NULL.** A vanilla floor over the three isolated serial runs
(`ci/corpus-floor.sh`, N=3) does not list 084, so `gameplay-floor.toml` could
not excuse it and was not rewritten (it would also have demanded four new
`[[unstable]]` entries at p_hide 63%).

**NEXT, if anyone takes sharding up again:** no window placement can exclude
084's ambient roll (javap: `nextInt(1000) < ambientSoundTime++`, field 0 at
spawn, reset to -80 only after a sound). Spawn the pig silent (spawn egg
`entity_data` with `Silent`, once rustmc honours it), then
re-take one isolated 8-shard run against the three serial runs in
`~/grit/shard-eq-results/fix1` — the recorder is unchanged since 0ec801e1f, so
they remain valid controls until the recorder moves.

### STATE — 2026-10-01, halted at the owner's usage stop
Shard proof EQUIVALENT at 8 (`full-shard8c-silent084-3serial-20261001`); default `RUSTMC_CORPUS_SHARDS=8`; 084's pig is silent via spawn-egg `entity_data`, honoured by the server.
T0 keep-going GREEN at 7647d32fb (126 stages). `lock-pool-lanes` (db357ee4a) is separate: falsifier 61/61; T0's only red is the documented t1-par case-6 load flake.
Pin order: land this branch and watch one 8-shard gate before `lock-pool-lanes` (3 runs of 8 pairs each would be 24 pairs).
