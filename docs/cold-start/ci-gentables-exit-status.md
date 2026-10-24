# Cold start — branch `ci-gentables-exit-status`

---

### COLD START — branch `ci-gentables-exit-status` off `0679f23`, 2026-08-12

**The briefed defect does not exist, and the one underneath it is worse.** The
item was "`ci/gen-gameplay-tables.sh` ran nineteen generators, the thirteenth
died on `cargo: command not found`, and the run still exited 0". Reproduced on
the box, in a fresh `~/rustmc-gentab` set up from `docs/AGENT-BRIEF.md`'s
snippet verbatim (no `PATH` export), and the run does not exit 0:

    == ci/dump-recipe-unlocks.sh
    /home/sayer/rustmc-gentab/ci/dump-recipe-unlocks.sh: line 233: cargo: command not found
    gen-gameplay-tables: ci/dump-recipe-unlocks.sh FAILED
    GEN-EXIT=1

Three corrections in that log. It is the **twelfth** generator of nineteen, not
the thirteenth — the printed order is `sort -u` over the derived table paths and
under the box's `en_GB.UTF-8` collation `recipes.json` precedes
`recipe_unlocks.json`. The status is **1**, not 0, and it propagates through the
`ssh` unchanged. And the run **names the generator that died** on stderr. Every
clause of `ci/gen-gameplay-tables.sh:143`'s
`"$ROOT/$gen" || { …; exit 1; }` was already doing its job.

**NULL — do not re-open: there is no lost exit status in the generator path.**
All nineteen generators carry `set -euo pipefail`; the twelve
`javac … | grep -v '^Note' || true` sites all recover the status immediately
after; `trap … EXIT` does not mask a `set -e` exit (measured on bash 5.2.21 —
an EXIT handler preserves the pre-trap status unless it calls `exit` itself);
and `ci/handoff.sh` stage 3 captures `TABLES_RC` correctly and its `remote()`
has always exported `$HOME/.cargo/bin`. **The status was reported and not
read.**

**The `PATH` half was already fixed by somebody else.** `corpus-lock-fairness`
added `export PATH=\$HOME/.cargo/bin:\$PATH` to the `AGENT-BRIEF` setup snippet
on 2026-08-12 and documented it, naming the same generator. That is the only
call site that was missing it. Nothing to do; not duplicated here.

#### What was actually wrong

Two things, and neither is an exit status.

1. **A generator that returns 0 having written nothing gets its table
   laundered.** `:144`'s `[ -s "$t" ]` passes, because on every gate directory
   the `cp -rn` in the setup has already put *another tree's* copy at that
   path — non-empty, plausible, and not ours. `:145` then stamps it with this
   tree's provenance. That does not merely miss a defect: the stamp is the one
   instrument that can tell a table this tree made from one it did not, so it
   disarms `ci/t2-gameplay.sh`'s exit-4 refusal for that table permanently.
   **`ci/dump-flat-preset.sh:32-35` could reach exactly that state** — it
   `exit 0`'d with a "skipping: no vanilla datagen extract" message. A skip is
   right for the dozen `crates/rustmc-worldgen/tests/*.rs` that use that idiom
   and wrong for a generator, which is not asked to run but to produce a table.

2. **A half-written `data/vanilla` is indistinguishable from a complete one to
   everything that matters.** This is the whole of what the incident cost.
   After the failure the tree held nineteen tables: eleven this tree's, eight
   another branch's, all present and all non-empty. Nothing that *reads* a
   table reads its stamp — every golden test in T0 just opens the file. So T0
   came back with eight red golden-table tests and a red
   `check-use-probe-falsify`, and all nine read as defects in the branch under
   test. `ci/t2-gameplay.sh` would have refused (it does check stamps); T0 had
   no way to know.

#### The fix

* `ci/gen-gameplay-tables.sh` records the table's `stat -c '%y %s'` before
  running its generator and refuses if it did not move. Contents are the wrong
  comparison — several generators are deterministic and print "identical
  across two runs" — but a generator that writes always moves the mtime.
  Validated against all nineteen **real** generators with `--force` on the box,
  so it does not false-red on any of them.
* It writes `data/vanilla/.provenance/INCOMPLETE` around the generation loop,
  naming the tree, the reason and the pending tables, and removes it on
  success. It is a statement, not a latch: the repair for it is to finish the
  job, and a run that completes clears it. It lives with the tables, so it
  travels through the `cp -rn` that seeds the next gate directory — which is
  the right direction.
* `ci/t0.sh` **refuses** on that marker before running anything, naming the
  cause. That is the line that turns eight mystery reds into one sentence.
* `ci/dump-flat-preset.sh`'s skip is now `exit 1`.
* `ci/movement-falsify.sh:133` scored `cargo test` failing-for-any-reason as
  "injection caught". A box without cargo on its `PATH` — the same 127 this
  whole item started from — made every injection score caught and the sweep
  report a perfect falsification having compiled nothing. It now distinguishes
  101 (tests ran and failed) from anything else, and refuses.
* `ci/gen-tables-falsify.sh` is the new gate: six injections and two controls
  in a throwaway tree with stub generators, about a second, no cargo, no JVM,
  no jar, no box. In `ci/t0.sh`.

**The falsifier's own before/after is the demonstration.** Against
`origin/main`'s script it scores **19 passed, 3 failed** — and *which* three is
the diagnosis: main already refuses a generator that exits non-zero, already
names it, already stops, already catches an empty table. What it fails is
"leaves an INCOMPLETE marker" and, in the silent-success case, "run exits
non-zero" and "does NOT stamp the table it did not write". Against this branch,
**22 passed, 0 failed**.

Writing it cost one bug worth recording, because it is the same class as the
item: the harness passed its injections to the stub generators in `$MODE`, and
`ci/gen-gameplay-tables.sh` has a variable of its own called `MODE` which it
sets to `""` at line 22. Every injection was silently disarmed and every case
came back green-shaped. Renamed to `$STUB_INJECT`/`$STUB_MODE`. A harness whose
injections do not reach is a harness that always passes.

#### Other masked-exit sites in `ci/`, found and NOT fixed

Swept all 143 shell scripts. **66 have no top-level `set -e`; 43 of those end
on a zero-status command, so they always exit 0.** Worst first, all left alone
as out of scope:

* `ci/next-12345-measure.sh:193` — `run_build; run_dump; run_control;
  run_diffs; run_census` sequenced with `;` under `set -uo pipefail`, so a
  failed build still runs the census and only the census's status survives.
* `ci/end-city-window.sh:26`, `ci/canvas-model-price.sh:27`,
  `ci/portal-attribute.sh:33`, `ci/bench-arms-setup.sh:41` — unchecked
  `cargo build … | tail -3`, then measure with whatever binary is on disk.
* `ci/bench-servers.sh:140` — `if command -v cargo …; then build; fi` with **no
  else**: missing cargo silently benches a stale binary.
  `ci/gen-vanilla-region.sh:69-73` handles the same case correctly.
* `ci/floor-ensemble.sh:218`, `ci/order-level-retake.sh:152` — differ failures
  become `|| echo "?"` / `|| echo 0` and are then filtered out of the
  min/median/max, so a half-broken sweep reports a *tighter* spread.
* `ci/waterlog-poison-control.sh:121` — the `FIRES` branch prints and falls off
  the end, so its verdict is text only.
* The `ci/order-*.sh` family (10 scripts) plus `nether-ladder-refs.sh` —
  `set -uo pipefail`, no `-e`, last line a bare `echo "… DONE"`.

Verified clean: `ci/t0.sh`, `ci/handoff.sh`, `ci/gate-lock-falsify.sh`,
`ci/handoff-verdict-selftest.sh`, `ci/chat-commands-falsify.sh`.

#### Next

`ci/bench-servers.sh:140` is the one worth doing next and it is a two-line
change — it is the only site in the list where the false green is a *published
number* rather than a re-runnable sweep. Closes when a `PATH` without cargo
makes it refuse instead of benching the previous binary.
