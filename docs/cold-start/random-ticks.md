# Cold start — branch `random-ticks`

### COLD START — branch `random-ticks` off `main` (`d48904cb5`), 2026-10-01 — HALTED at 91% weekly usage

State: ServerLevel.tickChunk ported whole (randomtick.rs), 21 randomTick bodies + bone meal arms in growth.rs, light from blocks in brightness.rs; unit tests green on the box (40 + flat-world end-to-end: 85 ticks at speed 4096, 25 chunks, 2.33 s). T0 NOT run. Scenarios 969-980 are a WIP commit with NO baseline/falsifiable rows yet — they must not be gated until a subset recording (job `rt-sub5`, keep dir `~/rustmc-random-ticks/oracle-work/keep-rt-sub5`) is read; 969 is calibration only and must be deleted before landing.
Open: our side recorded nothing on random-tick checks in `rt-sub2` because Light::levels searched radius 15 on every query (fixed in 32d3d2983, unverified on the wire); the third bone meal on a sweet berry bush did nothing on our server (cause unknown; `RUSTMC_DEBUG_USE` line in world_send.rs is uncommitted-debug, remove before landing); 971 rewritten as a probe because vanilla coalesces farmland->dirt->grass into one update.
Next: read rt-sub5, prove vanilla self-agreement across two subset vanilla recordings, add baseline + falsifiable rows (splice into the gate's latest pair per coordinator rule), T0 via ci/run-detached.sh in ~/rustmc-random-ticks.

### COLD START — branch `random-ticks`, rebased onto main `7df907ba5`, 2026-10-01 16:55 — PAUSED (usage pacing)

Measured (Threadripper, seed 20260803, `ci/t2-gameplay-subset.sh` derivative that records 2 cold vanilla passes + 1 rustmc pass under one corpus-lock hold, job `rt2-k1` at tree `9b7c66337`): **declared 12/12 match** on 780 + 970..980 (strict 7/12), and **vanilla against itself 12/12 declared** (strict 11/12). Recordings: `~/rustmc-rt2-keep/k1/{vanilla-1,vanilla-2,rustmc}.json` + `checks/`; a third vanilla (old tree's ours) in `~/rustmc-rt2-keep/rt-sub5`. Log `~/.rustmc-detached/rt2-k1.log`.

Fixed this session: `absent` on random-tick checks was the radius-15 light search (b5943f02c, now verified on the wire); 975's third bone meal (useItemOn PASS for `SweetBerryBushBlock`, 255d15639); 970/971/974 needed the dug block's own 2001 to observers (`spawn_destroy_particles`, d0084403a); debug lines and 969 removed. FINDINGS appended.

Gate pair snapshot for the splice: `~/rustmc-rt2-keep/gate/{vanilla,rustmc}.json` (rustmc-stack, checks digest 6313e364..., 15:13).

Early keep-going T0 (job `rt2-t0a`, dir `~/rustmc-rt2-t0`, log `/tmp/claude-1000/-/befd9b5b-5163-468a-9d93-210a7ac9ac95/scratchpad/rt2/t0-t0a.log`, may still be finishing): reds seen so far are the expected UNREGISTERED 970..980 (falsifiable + grades), plus `ci/projection-drops.py` ("removes a field nobody declared or ranked") and `ci/projection-drops-falsify.sh` (`historical-569323e-repaired exit 1`) — the last two NOT yet attributed; check whether main reds them too before touching.

Next, in order: (1) splice `k1/vanilla-1.json`+`k1/rustmc.json` into the gate snapshot (`scratchpad/rt2/` has `ledsplice.py`; ranged lane's `splice.py` pattern), run `ci/corpus-falsifiable.py --sweep --ledger <copy>` and `corpus-history.py` + `corpus-grades.py --write --out <copy>` in `~/rustmc-rt2`, splice only the 11 rows 970..980 into the committed ledgers (falsifiable sorted by `file`, grades by `name`); (2) add 11 baseline rows `= "match"` to `ci/checks/gameplay-baseline.toml` citing rt2-k1; (3) attribute the projection-drops reds; (4) T0 keep-going via `scratchpad/rt2/t0.sh rustmc-rt2 <tag>` under `ci/run-detached.sh`; (5) WORKLIST COLD START INDEX row.

### COLD START — branch `random-ticks`, landing repair, 2026-10-01 evening

State: gate RED at T0 (`gate-random-ticks-20261001-170706.log:322`) was ledger catch-up only: `projection-drops.toml`'s two `block_update` rows counted 107 observers and 970-980 made it 118 (the same stale count was the `projection-drops-falsify` `historical-569323e-repaired` red); on main 994e795b9, after be-furnace-brewing, it is 117 -> 128. Falsifiable + grades rows for 970-980 grafted from a sweep over a read-only copy of the `~/rustmc-stack` gate pair with `rt2-k1` spliced in (190/190 tier 2); baseline rows `match` for all eleven. No code changed.
Next: the chain's gate.
