# COLD START — branch `lockwhy`

### COLD START — branch `lockwhy`, stacked on `corpus-lock-hold-span`, 2026-08-18 — the lock's founding evidence does not survive its own primary artefacts, and concurrency costs at most 1.3%

Worktree `~/grit/work-lockwhy`, scratch trees `~/rustmc-lockwhy-a` and
`~/rustmc-lockwhy-b`. Extends `docs/CORPUS-LOCK-HOLD.md` (§§11-15). Stacked
rather than branched off `main` because §8, both instruments and the document
itself are on `corpus-lock-hold-span`; rebase that first.

**The lock is untouched. Nothing in this branch disables, weakens or bypasses
it, and no tracked file changes its behaviour.** The one edit to
`ci/corpus-lock.sh` is a comment block; `ci/corpus-lock-falsify.sh` was run
after it. The experiment runs in scratch trees on ports outside
`pick_port_pair`'s range, each with its own private `RUSTMC_CORPUS_LOCK_DIR`,
which is the lock's own supported variable — so neither scratch run ever took a
slot in the real queue.

---

## The brief's premise was wrong, and so was my first correction of it

The brief said `92c47ea3` introduced the corpus lock **and** `pick_port_pair` in
one commit, making the founding incident confounded. That is `docs/CORPUS-LOCK-
HOLD.md` §8's claim and it is a dating error.

`pick_port_pair` entered in **`36b5f48f`, 2026-08-06 01:19:58**, thirty-one
hours before the lock, together with the boot-failure taxonomy that makes a port
collision return 3 and refuse by name. In `92c47ea3`'s diff the whole ports
block, including the sentence "Until 2026-08-06 this file hardcoded 25696 and
25697", arrives as **context**. A comment saying the hardcoding ended is written
by the commit that ended it.

I then wrote §11 concluding that the *second* incident, 2026-08-07, was
therefore unconfounded evidence **for** the lock. §13 withdraws that. Both
readings were reached the same way — by grepping for a string instead of asking
what the artefacts say — which is why `ci/corpus-lock-provenance.sh` exists and
why its own first run went red on exactly that mistake one layer down.

## What the founding evidence actually is

The primary artefacts of the incident night are still on the box, unmodified:

| file | mtime | what it is |
|---|---|---|
| `logs/chatcmd/gate-check-before.txt` | 01:44:17 | box checked for harnesses, `NONE — clear` |
| `logs/chatcmd/t2-gameplay.log` | 02:19:51 | the run's own stdout, 39 KB, every `recorded N/88` |
| `logs/chatcmd/gate-check-after.txt` | 02:19:51 | box checked again: **two other corpus recordings live** |
| `gatelock-logs/live-overlap-evidence.txt` | 02:10:43 | **the entire evidence for the stall** — four lines |

The last is one sample at one instant, recording `cputime=00:00:01` from `ps` at
one-second resolution, with no second sample to difference against. The header's
"1-2 CPU-seconds in 17 minutes" is a reading of it, not a measurement.

## None of the three instruments can tell a stalled run from a healthy one

`ci/corpus-healthy-baseline.sh` attaches to whichever run holds the corpus lock —
a run that by construction has the box to itself — and reads exactly those
numbers off it. **N=25 samples over 12.5 minutes** against the `rustmc-corpusgap`
holder on 2026-08-18, a run that was recording normally throughout:

| instrument | 08-07, called a stall | healthy solo holder, measured |
|---|---|---|
| `oracle-runner` cputime | `00:00:01` | `00:00:01` at 6m14s, **3.59 s at 956 s** |
| vanilla log | 2 joins, no writes 7+ min | 2 joins, **no writes for 959 s**, size fixed at 4297 |
| recorder state | `wait_woken` | `wait_woken` — it is what `pump_both` does |

The joins figure cannot be a throughput counter. `record` logs its party in
**once per run** and reuses it (`tools/oracle-runner/src/record.rs`, `party` is
filled on the first scenario and rebuilt only after a scenario *errors*), so a
successful 88-scenario recording writes exactly two `joined the game` lines. The
control read **2 joins while its own counter stood at 14/108**.

## The stated mechanism is not possible

"They each wait out the others' timers" needs a settle window another process
can extend. It is

    pump_both(actor, observer, Instant::now() + Duration::from_millis(settle_ms), ..)

against `while Instant::now() < until` — a fixed deadline computed once. Two
runs' settle windows elapse concurrently, not end to end. §8 said the claim
"requires a serialising resource nobody has named"; the code says there is none
in the settle path.

## The run called stalled finished a clean corpus, concurrently

The second line of the evidence file is `~/rustmc-chatcmd`'s recorder. That run
started 01:44:17 on a box checked clear, recorded **88/88 against vanilla on
:26694** and **88/88 against rustmc on :26695** — the exact pair the md5
derivation gives for that root, so `pick_port_pair` is confirmed against a
primary log — and finished 02:19:51 with `declared 58/88` and **0 vacuous, 0
inert, 0 error**, committed as `108c4ac6` with all four pre-registered
predictions holding.

At its finishing instant two other corpus recordings were live, on `:26503`
(`~/rustmc-gate`, already past its reference) and `:26666`
(`~/rustmc-gatelock-b`).

**2134 s for 176 scenario recordings including both boots = 12.1 s each**,
against the ~9 s solo figure the lock's header quotes. And the collapse is
arithmetically excluded: if the whole 1071 s overlap had run at the claimed 600
s/scenario it could have held 1.8 scenarios, leaving 174.2 for the remaining
1063 s — 6.1 s each, faster than solo.

## What is NOT settled, and why this branch recommends rather than flips

* **2026-08-06 stands.** That incident had all sixty trees on 25696/25697, real
  counter readings (74/86, 50/86, 70/86) and a real collapse. §8 is right that it
  is confounded by the port bug; nothing here says it did not happen.
* **Rate is not integrity.** One overlapped run coming out clean is one run.
* **The lock also orders the gate against runs**, which nothing here measures.

## Instruments this branch ships

| file | what it is | falsifier |
|---|---|---|
| `ci/corpus-lock-provenance.sh` | re-derives §11's dating/guard/port claims from the tree | self-checking, exits non-zero |
| `ci/corpus-healthy-baseline.sh` | the three instruments read off a run that is fine | `ci/corpus-healthy-baseline-falsify.sh`, 7 cases, case 3 is four waiters it must not pick |
| `ci/corpus-exclusion-experiment.sh` | §12's design, executable | — |
| `ci/corpus-exclusion-measure.py` | the reader, with the overlap discard rule | `ci/corpus-exclusion-measure-falsify.sh`, 9 cases |

**`ci/corpus-exclusion-measure-falsify.sh` case 6** plants a 60x collapse and
requires the reader to report it, so a null from this rig is a null that could
have been a positive. **Case 9** guards the timestamper: the rig's first version
used `stdbuf -oL awk`, which sets awk's *output* buffering and does nothing to
its input, so mawk processed a whole run in one burst and stamped every line
with one time. Shown red on the unrepaired stamper (1 distinct stamp, span 0 s)
and green on the repaired one (3 stamps, span 4 s) before it was trusted.

## Traps

* **`pkill -f` / `pgrep -f` match the caller's own command line.** Cost two
  killed shells in this branch, including one that killed its own setup chain.
  `CLAUDE.md` already says this; use `[b]racket` form every time.
* **`stdbuf -oL` does not make awk read unbuffered.** See case 9.
* **`gen-gameplay-tables.sh --force` is ~17 minutes per tree** and the
  provenance preflight will refuse a tree whose tables were copied from
  another, which is correct and is not a bug to route around.

---

## The measurement, and the bound it puts on the founding claim

`ci/corpus-exclusion-experiment.sh`, 8 reps, arms alternating so the treatment
is not confounded with the time of day, every concurrent pair's overlap
computed from timestamped logs and a non-overlapping pair discarded rather than
counted.

| arm | N | mean s/scen | sd | min | max |
|---|---:|---:|---:|---:|---:|
| serial | 16 | **12.070** | 0.177 | 11.75 | 12.38 |
| concurrent | 16 | **12.109** | 0.151 | 11.88 | 12.38 |

**95% CI on the ratio 0.994x .. 1.013x — the design EXCLUDES any concurrent
slowdown greater than 1.3%.** The founding claim is one scenario per ten minutes
against a ~12 s baseline, about **50x**, excluded by a factor of ~49. Lead with
the bound, not the point estimate: a ratio near 1 is what an underpowered
experiment produces when it measures nothing, and detecting a 50x collapse would
need N=1.

**Integrity, the other half:** 16 recordings per arm, **16 of 16 byte-distinct in
both** (vanilla's own non-determinism present, which is the null), floor **0/8
in both arms across 240 pairwise comparisons**. Resolving power: a scenario
flapping under **17.1%** of the time could still hide. No homogeneity control —
nothing here is a promotable floor, only an arm-to-arm comparison.

**K=3, the case both incidents involved** (third tree, rig generalised to N):
serial 12.219 mean, conc2 12.094 (**1.00x**), conc3 12.094 (**0.99x**), N=4 each,
CIs excluding slowdowns above 1.8% and 1.5%. `conc3` ran at a median load of
**14.9** against serial's **9.0** and was still 0.99x — 0.97x in the high-load
half. Integrity floor **0/8 in all three arms**, every recording byte-distinct.

**Load:** runs span **0.1 to 18.3**, and whole-run duration moved only 204-212 s
across that 180x range. Ratio 1.00x in the low band, 1.01x in the high band.
**The 50-110 band was NOT reached** — a real corpus recording held the lock for
most of the window and deliberately loading the box would have risked inflicting
§14's `settle`-cap contamination on somebody else's merge. That is a real
limitation, not a rounded one.

**Against the box's own history:** `ci/corpus-phase-mtimes.sh` at N=113 gives
12.46 s/scenario for the reference phase of real gates that each held the lock
alone. The concurrent arm here runs at **12.11**. Not a matched comparison —
different scenario mix, shorter run — but the bracket is on the wrong side for
the lock.

## The recommendation, in one line each

1. **`RUSTMC_REFERENCE_CACHE=1 ./ci/handoff.sh <branch>`** — no code change at
   all, `ci/handoff.sh` 764-769 already thread it, ~1.94x on the
   serialised section from §10's measured terms. Biggest lever, smallest change,
   independent of everything else here.
2. **Do not remove the lock**; this lane did not touch it (comment-only edit,
   `ci/corpus-lock-falsify.sh` **47/47, 0 injections uncaught** afterwards).
3. Bounded concurrency is a live option with a known price: the slot array is
   small, the **falsifier rework is not** — 47 cases assert exclusion directly
   and `ci/corpus-exclusion-falsify.sh`'s stated property inverts.
4. Retire "one scenario per ten minutes" as a justification.

## What the queue costs, measured

`./ci/corpus-lock.sh status` sampled every 2 minutes, 51 samples: **4.2 runs
waiting on average** (max 5), **431 agent-minutes of accumulated wait** present
at any instant, worst single wait **174 minutes**.
