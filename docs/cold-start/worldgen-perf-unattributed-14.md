# COLD START — branch `worldgen-perf-unattributed-14`

### COLD START — branch `worldgen-perf-unattributed-14`, 2026-08-17

**Analysis only. No timing was taken and nothing here is a speed
measurement.** The box sat at load 18.6-19.6 the whole time with a merge gate
and an end-to-end lane test running, which is the condition `docs/BENCH.md`
says makes a wall number meaningless. The instruction was to do the
attribution reasoning now and take timings only on a quiet box, so this branch
narrows the candidate set and specifies the run; it does not perform it.

**The item.** `docs/BENCH.md` attributes the stale-tenth drift entirely to the
trial chambers at +7.03% instructions/chunk (r=7, n=6), and the same metric
against `35b3bde5` reads +22.44%. About **+14% landed between `9b4afd4e` and
`35b3bde5` unattributed**.

**What this branch establishes, all of it from the tree:**

* The span is **247 commits, 18 of them touching `crates/rustmc-worldgen/src`**.
* **The release profile did not change in the span** — no `opt-level`, `lto`,
  `codegen-units`, `panic` or `debug` line in `git diff 9b4afd4e 35b3bde5 --
  Cargo.toml`. That is the one explanation that would produce the observed
  uniform shape for no source reason, and it is dead.
* The 18 group into **A surface biome-id (5, every chunk)**, **B ruined portal
  (4, where a portal lands)**, **C the apron (3, where the feature runs)**, and
  **D no-ops**.
* **Group B is demoted on its own evidence**: `parse_for_block` has exactly one
  production call site in the crate, `crates/rustmc-worldgen/src/structure/place.rs::block_age_with` (`:2118`). A cost paid
  only where a ruined portal lands cannot be uniform across three centres and
  cannot survive a 4.8x larger square — which is the same mechanism test that
  convicted the chambers of being a per-chunk scan.
* **Group A is the only one paid on every chunk**, and `6120623f`'s own subject
  line ends "not yet measured". It is the leading candidate on mechanism. That
  is not a verdict.

**The run that decides it** — four short arms, `RUSTMC_WORKERS=1`, INSTRUCTIONS
RETIRED, tip and `9b4afd4e` at `RUSTMC_THROUGH=noise` and `=surface`. Group A
is inside `surface`; B and C are outside it. Then bisect five commits at most.

**Nulls and traps, recorded so the quiet window is not spent on them:**

* **Do not use `RUSTMC_STAGE_TIMES`.** It is on the rule-7 list for forcing the
  surface stage sequential, and the surface stage is the subject. At
  `RUSTMC_WORKERS=1` it buys nothing anyway.
* **Do not subtract one `RUSTMC_THROUGH` arm from another.** Truncated runs
  produce beardless terrain, so the difference between arms is not the cost of
  a stage. Only tip-against-baseline at the *same* arm means anything.
* **Do not re-measure against today's main without re-taking the baseline.**
  36 further worldgen commits landed between `35b3bde5` and `b7ff97fc` and none
  of them is in the +22.44% figure; folding them in silently re-opens what the
  chambers sweep already closed.
* **Do not use CPU-seconds or wall here.** Every run in the original sweep
  shared the box with 7-13 competing processes; instructions retired at one
  worker is why those numbers survived that.

**The single next item, and what closes it:** the four arms above, on a box
with no gate and no lane test, load recorded beside every number. It closes
when the +14% is inside `surface` or demonstrably outside it — either answer
attributes it, and the second one puts groups B and C back with a lumpiness
prediction to test.

### UPDATE 2026-08-17 — the 18 was undercounted (it is 24), the four-arm design still holds, and the box is still not quiet

**Still analysis only.** Load at pickup: 26.66-30.75, `/run/ci-busy` set, a
merge gate holding the corpus lock in `rustmc-refcache-bw-A`, and
`TmForever.exe` at ~1235% CPU for 19+ hours per the brief. No timing taken.

* **The 18 only scanned `crates/rustmc-worldgen/src`.** `worldgen-dump`'s real
  dependency closure (named at the top of `docs/BENCH.md`) is four paths; over
  the same span it is **24, not 18** — six more in
  `tools/worldgen-assembly/src/generate.rs` and `crates/rustmc-world`
  (`2ec2377b`, `36f6d432`, `50fc4220`, `6f3a0abe`, `d6b6abd2`, `f87d23f4`),
  all village/ruined-portal work. `docs/BENCH.md`'s new "The '18' undercounted"
  section has the detail.
* **The four-arm design is unaffected.** Checked in `generate.rs`: every
  structure-start computation — villages, ruined portals, trial chambers,
  mineshafts, ocean ruins, shipwrecks, end cities — is gated
  `if through >= Through::Decoration`, and `Through` orders `Noise < Biomes <
  Surface < Carvers < Decoration` (`crates/rustmc-worldgen/src/pipeline.rs::Through` (`:158`)). None of the 24 minus
  group A's 5 can run under `RUSTMC_THROUGH=noise|surface`, so a positive
  result in `surface` still convicts group A cleanly.
* **But `50fc4220` reopens group B's mechanism, not just its count.** It is
  not the `parse_for_block` call group B was demoted on — it is
  `ruined_portal_starts`, the structure-set scan over the whole window (RNG
  draws, template selection, biome checks), the same *shape* as the chambers'
  jigsaw walk rather than `parse_for_block`'s single landed-portal call site.
  If the surface arm comes back flat, the decoration-side re-run must test
  this function (and the other five) against the lumpy-vs-uniform prediction,
  not treat B as fully demoted on the old count of four.
* **Group A's five commits were read in full.** Every change removes an
  allocation or a doubled map walk; nothing in the diff reads as slower than
  what it replaced, and the `BiomeIds` linear scan is the same O(n) the old
  code did. This does not clear group A — three of five commits say
  themselves they were never measured — but there is no visible smoking gun
  either. Reading settles nothing further here; only the run does.

Nothing else in this file changes. The four arms, the nulls and the traps
stand as written above.

### NOT QUOTABLE — a smoke test of `ci/bench-attrib-through.sh` itself, kept because of its direction, not its size

Built both binaries (`9b4afd4e` at `~/rustmc-benchB`, current `main` tip
`bba29ee2` at `~/rustmc-wgperf14-tip`) and ran the new script end to end at
**r=1, reps=1, load ~21, contend 6-9 processes per rep** — purely to prove
the script itself works, the same way `ci/bench-pair.sh`'s own warmup pass is
discarded rather than trusted. It is not the run this item needs and is not
entered as a figure anywhere. Full conditions, so nobody mistakes it for one:

```text
  through=noise    tip vs base   instructions   ~0 at all three centres
  through=surface  tip vs base   instructions   tip LOWER by 2.2-3.0% at all three centres
```

At r=1 (49 generated chunks), N=1, four times below this box's own stated
ceiling, this proves nothing — but the direction agrees with the read-through
of group A's diff above (allocation and map-walk removals, no added
complexity) rather than pointing at it as the source of a regression. Take it
as a reason to run the real four arms rather than skip straight to
bisecting group A; it is not a reason to skip the run.

### UPDATE 2026-08-17 (later) — the run is loaded, verified and ready; still not fired

**No timing taken this session either.** Load polled twice, ten minutes apart:
17.78/21.98/25.63 and 17.77/20.46/24.39 (1/5/15-min), `/run/ci-busy` held
throughout, and `ps` shows the live gate chain (`gate-chain-160259.snapshot.sh`,
37 branches queued, this branch among them) plus `TmForever.exe` still at
~1200% CPU. All of it is at least 4x `ci/bench-attrib-through.sh`'s own
ceiling of 4.0. Per `AGENT-BRIEF`, that is a refusal, not a number.

**What is now prepared, so the next quiet window costs one command, not a
build:**

* `~/rustmc-benchB` — `9b4afd4e`, pre-existing, source verified byte-identical
  to `git archive 9b4afd4e -- crates tools Cargo.toml` (diff -rq, zero output).
* `~/rustmc-wgperf14-35b3bde5` — **new this session**, `git archive 35b3bde5 |
  tar -x` then `cargo build --release -p worldgen-dump`, 55.66s, clean. This
  is the actual tip the +22.44% figure was measured against — not today's
  `main`, per this file's own "do not re-measure against today's main"
  warning above.
* Both directories are now stamped and verified by a new check,
  `ci/bench-stamp-sha.sh` (below), which `ci/bench-attrib-through.sh` now
  requires before it will run any arm.

**The check added, and its falsification.** Until this session,
`bench-attrib-through.sh`'s `<sha>` arguments were pure labels — nothing
confirmed a directory actually held that commit's source, which is this
project's central defect class by another door (see `AGENT-BRIEF`'s mtime
hazard: "every trace agrees with every other trace and all of them are
wrong"). `ci/bench-stamp-sha.sh <dir> <sha>` now diffs `crates/`, `tools/`
and `Cargo.toml` against `git archive <sha>` and only writes `<dir>/.bench-sha`
on a clean match; `bench-attrib-through.sh` refuses with `FATAL` if a dir has
no stamp or a stamp for a different sha. Proven both ways this session, not
just written:

```text
  wrong sha, real dir (rustmc-benchB vs 35b3bde5's sha)   -> REFUSED, exit 3, no stamp written
  right sha, real dir (rustmc-benchB vs 9b4afd4e's sha)   -> stamped, exit 0
  bench-attrib-through.sh, tip dir mislabelled deadbeef.. -> FATAL before any arm runs, exit 2
  bench-attrib-through.sh, both dirs correctly stamped    -> passes the stamp check, reaches the load gate
```

Cargo.lock is deliberately excluded from the stamp diff — checked directly,
`9b4afd4e`'s committed lock disagrees with the freshly-built `rustmc-benchB`
lock (an extra `anvil-probe` entry, one added `serde` edge) while every file
under `crates/`, `tools/` and `Cargo.toml` is byte-identical, so comparing it
would refuse a directory that is the right source for everything
`worldgen-dump` depends on. Full reasoning in the script's own header.

**The exact command for the next quiet window** (load ≤ 4, no
`ci/handoff.sh`, no other heavy build):

```
cd /home/sayer/grit/work-wgperf14 && \
./ci/bench-attrib-through.sh rustmc-benchB 9b4afd4ee56ff253c46987b237edc8a2a7b43003 \
    rustmc-wgperf14-35b3bde5 35b3bde541ff42c928ebc41067baea83dbd43872 6 7
```

reps=6, r=7 to match the chambers figures' own n=6 at r=7 above, so the two
attributions are directly comparable. Nothing else in this file changes.

### COLD START — branch `worldgen-perf-unattributed-14-decide`, 2026-08-18 (the run is armed and waiting; still no figure)

**No timing was taken this session either, and the reason is the same one as
every other session: the box never went quiet.** Load through the window this
session, 1-minute, recorded rather than remembered:

```text
  07:21  2.91   the reboot window — this is the quietest it got
  07:23  5.24   REFUSED by ci/bench-attrib-through.sh, correctly
  07:53  4.45   other agents' gate + corpus work
  08:08 11.98
  08:23 28.26
```

`ci/bench-attrib-through.sh`'s ceiling is 4.0 and it was over it within two
minutes of the first attempt. **One `worldgen-dump full 20260722 7` at
`RUSTMC_THROUGH=surface RUSTMC_WORKERS=1` is 11.1s on this box** (real 11.135,
user 10.969), so the four-arm design needs about **14 minutes** of quiet — more
than any window this item has ever been offered.

**What is now armed rather than merely prepared.** `/home/sayer/grit/wgperf14-watch.sh`
is running detached (deadline 12:28 today) and fires the exact command in this
file the moment the 1-minute load holds at **2.5 or below for 60 seconds**. The
threshold is below the script's own 4.0 ceiling on purpose: the run is one
CPU-bound worker and adds ~1.0 to the load over its own lifetime, and the gate
is re-checked before each arm, so firing at 3.9 buys the first arm and then
refuses the second on load the run itself created. **The 4.0 ceiling is not
loosened anywhere.**

It holds `logs/.wgperf14-bench.lock` across the whole run, and
`logs/heavy.sh <cmd>` takes the same lock — so no build this agent starts can
land in the middle of an arm.

**Where the answer will be, and how to read it:**

* `/home/sayer/grit/logs/wgperf14-watch.log` — every load poll, the fire, and
  on a clean run the paired summary and the `contend` column per arm.
* `/home/sayer/grit/logs/bench-attrib-through-<stamp>.summary` — ends with
  `# arms clean:` / `# arms NOT clean:` and the line "a figure may be quoted
  ONLY from an arm listed clean above". **That line is load-bearing now**: an
  arm is believed only if `ci/bench-pair.sh` exited 0 AND the TSV holds
  `reps*centres*2` rows. Both halves are falsified three ways in `docs/BENCH.md`.

**Two changes that make a short window worth more than it was:**

* **The `surface` arm runs FIRST.** Group A is inside it and is the only
  candidate paid on every chunk; `noise` is the control. The old order spent a
  short window on the arm that decides nothing.
* **An all-crash run can no longer report success.** That is how the 00:04
  window was lost, twice.

**What closes this item, unchanged:** the `surface` arm clean, tip against
`9b4afd4e`, instructions retired, `RUSTMC_WORKERS=1`, with the load recorded
beside it. Inside `surface` convicts group A; flat sends it to the
decoration-side re-run with the lumpy-vs-uniform prediction, and `50fc4220`
(`ruined_portal_starts`) is the candidate to test first, not the demoted
`parse_for_block`.
