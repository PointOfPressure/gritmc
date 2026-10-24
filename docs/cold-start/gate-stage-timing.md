# Cold start — branch `gate-stage-timing`

---

### COLD START — branch `gate-stage-timing` off `main` (`8d06bb06`), 2026-08-17

**The gate could not measure itself.** `ci/handoff.sh` prints six stage
markers and not one of them carries a timestamp — checked against all 248 logs
in `~/rustmc-private/gates`, the only wall-clock anywhere is the corpus-lock
waiter heartbeat. Every gate-cost figure this project has published (the
standing "~87 min, ~22 queueing / ~43 recording" split, the 38.4 min lock
figure) was reconstructed indirectly, by differencing chain-log timestamps or
recovering it from another script's output. Same defect class as a check that
cannot go red: an instrument that cannot report its own cost.

#### What this does, and does not, change

**Measurement only.** No stage's behaviour moved. `ci/handoff.sh` gained:

* `mark <label>` — one `STAGE-TIME <label> <epoch> <iso8601>` line into `$LOG`
  per stage boundary, called on the skip path exactly as on the run path, so a
  gate that dies at stage 1 still has a stage-1 duration on record.
* `stage_summary` — reads every `STAGE-TIME` line back out of `$LOG`, in the
  order the gate actually reached them, and prints `stage / wall_s / pct`, one
  row per interval between consecutive marks plus a TOTAL row. Called at every
  exit path: `infra`, RED, the REFUSED after `gate_lock_assert_anchored`,
  dry-run GREEN, and MERGED — not only the green path, because the fast-fail
  path is where slot waste hides.

Marks are placed at the START of a stage, not its end: the interval from
`mark 1-prepare` to `mark 2-rsync` **is** stage 1's wall time, whichever way
stage 1 went. That is also why the summary's parts sum to its TOTAL exactly —
integer subtraction over the same numbers written once, never a second
measurement that could disagree with the first.

`infra()` is the one exit path that can fire before the normal post-stage-6
"end" mark exists (a stage-1 or transport death never reaches stage 6 at all),
so it marks "end" itself, guarded against double-marking on the two call sites
that run after stage 6 has already closed it out.

#### The bug this caught in development, and why it is worth naming

The first implementation had `stage_summary`'s awk read `STAGE-TIME` lines
into two arrays keyed by an incrementing counter `n`, with no `BEGIN` block.
**An uninitialised awk scalar used as an array subscript is keyed by its
STRING value, and for an unset variable that is `""`, not `"0"`.** So the
very first `mark` (`start`) landed under `lbl[""]`/`ep[""]`, while the `END`
block's read loop starts at `lbl[0]`. Every later mark reads back correctly
because `n++` forces numeric context after the first assignment — only index 0
is wrong. The symptom was exact and ugly: `total = ep[n-1] - ep[0]` read
`ep[0]` as empty/0, so `TOTAL` printed a raw Unix timestamp (`1786976141`)
instead of a duration, and the first row printed with a blank label. Fixed
with `BEGIN { n = 0 }`. Reproduced and fixed via a hand-built fixture
(`/tmp/.../gatecheck`, not kept) before it reached the selftest, then
regressed in the selftest against the real code path, on the **printed
table**, not the function's input — a check against the input would not have
caught it, since the input (the STAGE-TIME lines) was already correct.

#### `ci/handoff-verdict-selftest.sh`: 22/22 -> 27/27

Two new sections, five new cases:

* **[7] stage timing on a green gate** — runs one more `GREEN` scenario, reads
  its own log back: at least 8 `STAGE-TIME` marks, epochs non-decreasing
  end-to-end, and the printed summary's non-TOTAL rows sum to its own TOTAL
  within 2 seconds (parsed from the same table a human reads, per the bug
  above).
* **[8] stage timing on the fast-fail path** — a stage-1 transport death
  (`GATE_T_STAGE1=transport`, the existing INFRA fixture) still has a
  `STAGE-TIME 1-prepare` mark and a `== gate timing summary ==` block in its
  log. Narrow on purpose: a stage-1 death should carry a mark for stage 1, not
  marks for stages that never ran.

All fake-ssh stages in this harness return instantly, so every measured
`wall_s` in the selftest's own fixture logs is 0 — the monotonic and
sum-to-total assertions hold at zero exactly as they would at any other value,
which is what makes them a real test of the arithmetic rather than of the
clock. Verified separately, by hand, against a fixture with `sleep 1/2/1/3`
spliced into the four remote stages: the summary attributed 1s/2s/1s/3s to
stages 1/3/5/6 respectively and TOTAL was exactly 7 — not re-included here,
since the selftest's job is regression, not a stopwatch.

#### What is NOT done here

* No stage's log OUTPUT changed beyond the added `STAGE-TIME` lines and the
  final summary block — `grep`s against `$LOG` elsewhere in this file
  (`T0_GREEN`, `T1_GREEN`, etc.) are unaffected, and the selftest's existing
  22 cases pass unmodified.
* The post-stage-6 bookkeeping (the compile census, `gate_lock_assert_anchored`)
  is deliberately NOT folded into stage 6's number — "end" marks right after
  stage 6's own exit code is captured, so TOTAL in the table is the sum of the
  six stages plus setup, not the whole script's wall-clock to its final echo.
* No throughput decision is made here. This is the instrument the three
  pending levers (reference cache, second lane, batching) need in order to be
  compared; reading it against a real gate is the next agent's job, not this
  branch's.
