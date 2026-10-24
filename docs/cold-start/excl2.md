# COLD START — branch `excl2`

### COLD START — branch `excl2`, stacked on `lockwhy`, 2026-08-18 — the two things §15 did not license: integrity at the full corpus, and the load band nobody had entered

Worktree `~/grit/work-excl2`, scratch trees `~/rustmc-lockwhy-a`, `-b`, `-c`
(reused in place: their tables are stamped with their own absolute paths, so
copying them to fresh names would fail `prov_check` and regenerating three sets
costs about fifty CPU-minutes of exactly the load the load-band section is
trying to control). Extends `docs/CORPUS-LOCK-HOLD.md` (§§17-20). Stacked on
`lockwhy` because §§15-16 are the result being extended.

**The lock is untouched. No tracked file in this branch changes its behaviour**,
and `ci/corpus-lock.sh` is not edited at all, not even in comments.

---

## What §15 and §16 said they had not measured, which is the whole area

> 3. **Treat bounded concurrency as a live option with a known price**, not as
>    settled. The rate evidence for exclusivity is refuted; what is not yet
>    measured at any useful N is *integrity* under concurrency at the full
>    108-scenario corpus, and the 50-110 load band was never reached.

Both are answered here, and the third thing §15 asked for, a number for the
falsifier rework instead of a shrug, is §18.

## Instruments this branch ships

| file | what it is | falsifier |
|---|---|---|
| `ci/corpus-integrity-108.sh` | the arms, over the whole corpus, driving `ci/t2-gameplay-selfcheck.sh` | `ci/corpus-integrity-108-falsify.sh`, 16 cases + 8 injections |
| `ci/corpus-integrity-arms.sh` | reads the arms by scenario NAME, not by floor total | (its discriminator is the result; see §19) |
| `ci/corpus-holder-rate.sh` | what background load is doing to the PRODUCTION lock holder, from `schedstat` runqueue delay | `ci/corpus-holder-rate-falsify.sh`, 9 cases + 4 injections |
| `ci/corpus-load-band.sh` | the load generator, niced, pinned, budget inside each worker | `ci/corpus-load-band-falsify.sh`, 15 cases + 8 injections |
| `ci/corpus-load-band-experiment.sh` | calibrate, refuse if the holder is harmed, then run §15's arms in the band | — |
| `ci/corpus-exclusion-cost.sh` | the falsifier rework, as a table checked against both suites | self-checking: refuses to tally if a case is unclassified |

## The load axis is not blocked on the lock, and that was a mistake to make

The integrity question needs the recordings and genuinely waits for them. **The
50-110 load band does not.** It needs load, and load is the half that can be
made rather than waited for. This branch initially chained the whole band
experiment behind the 108-scenario run, which was the wrong shape: the loader
can be built, characterised and pre-registered while the recordings are still
being taken, and only its *arms* need the trees back.

What is measurable without the lock, and was being left on the floor:

* what the box's own afternoon reaches, passively — **15 to 38**, so the free
  part of the axis stops well short of the band;
* how much more a concurrent recording waits for a CPU than a solo production
  holder at the same box load — the concurrency term itself, visible for the
  first time;
* whether the `nice 19` construction actually delivers contention to the arms
  (§20's prediction L4), which is the assumption the whole band result rests on
  and which needs no corpus run to test.

## Traps this branch paid for

* **`local a="$1" b="$W/x-$a"` fails under `set -u`.** Bash declares every name
  in one `local` before assigning any, so the second reference reads an unset
  variable. Cost three separate debugging rounds in this branch. Split it.
* **`$(( $(wc -l < "$f") - 1 ))` is a bash syntax error.** A command
  substitution containing a redirect inside an arithmetic expansion does not
  parse, and the error takes the whole enclosing `if` block with it, so later
  cases run with unset variables and report failures about code that is fine.
* **`setsid cmd &` forks**, so `$!` is the setsid that exits immediately, not
  the process you want.
* **A command substitution holds the pipe open until every writer exits**, so
  `PID="$(start_background_thing)"` both blocks and returns the wrong pid.
* **`perl -0pi -e 's/x/y/'` without `/g` patches the first occurrence**, which
  in a well-commented file is the header comment. The injection then reports
  COMMENT-ONLY for a patch that never reached the code.
* **A sleeping stub accumulates neither CPU nor runqueue delay**, so
  `delay/(cpu+delay)` and `delay/cpu` both return the same sentinel and a case
  written to tell them apart cannot. Stubs that stand in for a busy process
  have to be busy.
* **Editing a shell script while an instance of it is running corrupts that
  instance.** Bash reads a script incrementally by byte offset, so an edit
  shifts everything after it and the running copy starts executing fragments.
  It surfaces as a syntax error at a line number that does not match the file
  you are looking at, and it is easy to spend twenty minutes debugging the
  file instead of the process. `CLAUDE.md` says this about the supervisor
  loops; it is true of every script. Check `ps` before editing, and never
  relaunch a falsifier while the previous run is still in its sweep.
* **A shared `pkill -f` pattern lets a falsifier's own sub-invocations reap
  each other's stubs.** The injection sweep re-runs the file, each copy's
  cleanup killed the next copy's fake recorder, every sample row came back as
  the `-1` sentinel, and `-1` agrees with every formula, so the
  `share-is-delay-over-cpu` injection read as MISSED twice against code that
  was correct. Markers carry `$$` now, and the case fails outright if every row
  is a sentinel rather than passing vacuously.
* **`cargo` is not on a detached run's `PATH`.** The first smoke run of
  `ci/corpus-integrity-108.sh` returned 127 from every invocation, finished in
  eleven seconds, and printed a complete-looking block structure with empty
  arms under it. That is why the driver now refuses a barren first invocation.

## What is still running, and how to finish it

Two things were in flight when this was written, both detached and both able to
finish without supervision.

**1. `ci/corpus-integrity-108.sh`, blocks 2 and 3.** Log
`~/grit/work-excl2-int108.log`, output `~/grit/work-excl2-int108/`. Block 1 is
in and is §24; blocks 2 and 3 take the serial arm to n=6 and the concurrent arm
to n=18, which is what §17.1 pre-registered. When the log holds
`INTEGRITY-108-DONE`:

    ./ci/corpus-integrity-arms.sh ~/grit/work-excl2-int108

That prints the per-arm floor, the concurrent-only set, and the matched-n
placement. **Read §19 before reading its output**: the arms have unequal n by
design and their floor counts are not directly comparable. §24 is the worked
example of the false positive that comes from forgetting it.

**2. The band arms**, chained behind the above by
`~/grit/work-excl2-chain2.sh` (untracked), log `~/grit/work-excl2-band.log`.
Two reps of `ci/corpus-exclusion-experiment.sh` under 40 workers, reusing the
completed calibration at `~/grit/work-excl2-calib-40.tsv`. It answers the one
thing §22.3 names as unmeasured: whether concurrency costs *more* inside the
band than outside it. The rig drives the same eight checks every run, so unlike
§22's numbers it carries no scenario-mix confound, and its output is directly
comparable to §15's 12.070 and 12.109 s/scenario at low load.

It prints `LOAD-BAND-EXPERIMENT-COMPLETE` or `-INCOMPLETE`. **Do not quote a
ratio from an INCOMPLETE run**: it means the arms outlasted the load and part of
them ran on an unloaded box, which is indistinguishable from a band measurement
in the numbers alone.

**If the chain died**, everything it needs is committed; re-run it with the
environment it sets, and note that `LOADBAND_ACCEPT_HARM=1` is a deliberate,
priced decision explained in §22.5 and not a way round the guard.

## Two operational notes for whoever runs the analysis

**`Cargo.lock` will come back dirty and must not be committed.**
`ci/corpus-integrity-arms.sh` calls `ci/corpus-floor.sh`, which builds
`oracle-runner`, and on this branch's base that rewrites `Cargo.lock`
(`grit-plugin-api 0.2.1 -> 0.3.0`). `main` fixed that at 2183 and this branch
is based on `lockwhy`, which predates it. So after running the analysis:

    git checkout -- Cargo.lock

This branch's whole merge argument is that it modifies no tracked executable, so
letting a build artefact ride along would undo it for no reason.

**The `--console` difference is confirmed constant, not merely argued.** §17.2
predicted that the one check of the 108 carrying a `[[gamerule]]` block is
driven differently by the selfcheck than by the merge gate. The recorder says so
itself, in every invocation of both arms:

    !! random-ticks-turn-covered-grass-to-dirt: [[gamerule]] random_tick_speed
       = 1000 NOT APPLIED — no --console for 127.0.0.1:27601. This recording is
       on the harness's substrate, not the scenario's.

Exactly **two occurrences per invocation** (one per run, two runs each) in all
eight invocations, always that one scenario, and it lands in the floor as
`dead` in the external serial ensemble too (§19.1). Constant across arms,
therefore incapable of discriminating between them.

## What would still be worth doing after that

* **The top of the band.** L2 was never run: loads up to **62.8** were reached
  and 110 was not. A single `--calibrate 90 2` costs about thirteen minutes and
  closes it. It should be run against a *measured* baseline of the delay share
  rather than §20's invented +0.20 line — `ci/corpus-delay-curve.py` produces
  that baseline now and §22.5 explains why the invented one was useless.
* **Integrity at 108 scenarios AND high load together.** This branch covers each
  axis once and their intersection only accidentally, through the twelve loaded
  minutes inside block 1's concurrent arm (§20.5). A deliberate run would be one
  more block of `ci/corpus-integrity-108.sh` with the load held throughout.
* ~~`ci/corpus-holder-rate.sh` has no falsifier case for `SAMPLER-END`.~~ Closed:
  `ci/corpus-holder-rate-falsify.sh` case 10 asserts the reported count against
  the rows actually in the file, with a `no-row-count-reported` injection.
