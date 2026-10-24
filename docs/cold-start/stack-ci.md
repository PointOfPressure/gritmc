# COLD START — branch `stack-ci`

Append-only. Newest entry last.

### COLD START — branch `stack-ci`, 2026-09-05, landing it on main f4fa28ff8

Worktree `~/grit/work-stackci` (local branch `stackci-work`, pushed as
`origin/stack-ci`); remote dirs `~/rustmc-stackci` and `~/rustmc-stackci2`;
detached logs `~/.rustmc-detached/stackci-*.log`. The stack was already rebased
onto main `f4fa28ff8` by the chain at 2dc2e0776 (275 patches); this entry is
what it took to make that tip green.

## The three reds, and what each one actually was

**T1-CONTROLS, `cargo builds 84 test binaries, the ledger declares 80`.** The
real one. `mobs-goals` added four `rustmc-entity` test binaries and
`ci/checks/t1-positive-controls.toml` is the only thing in this tree coupled to
that count — which is the refusal working, not a defect. Measured with
`ci/t1-positive-control.py --only <name>` in `~/rustmc-stackci`:
`golden_mob_math` and `golden_mob_path` both reach `perturbed` (one integer
changed inside `golden/mob_math_vectors.json` / `mob_path_vectors.json` reddens
them), so the fixture axis covers two of the four. `ground_path` and
`navigation` read no fixture at all and come back `unforced`, `0 candidate
fixture(s)`, which is `ci/t1-sabotage.sh`'s case.

**`ci/check-handoff-terminals.py`, a terminal declared twice.** The stage-6
copy of the `RUSTMC_REFERENCE_CACHE` validation, which main moved to before
stage 1 and a fold put back. This is the THIRD application of the same one-hunk
repair — `af4a9a0fb`, then `e94dedfbe`, both local tags, neither pushed, so the
chain's rebase dropped both. Anything repaired only as a tag on this stack will
be dropped again; push it.

**`ci/check-use-probe-falsify.sh`, "an injection went green".** Not a branch
defect. The two `real-table` cases failed with
`data/vanilla/use_item_on.json: STALE — this tree did not generate that table`,
in a remote dir that had skipped `ci/gen-gameplay-tables.sh`. The stack does
change `ci/use-item-on-golden.sh`, so any gate directory carrying pre-change
tables reds here; `ci/handoff.sh` stage 3 regenerates and the real gate is fine.

## The null, so nobody re-runs it

`ground_path` and `navigation` needed a sabotage arm and **the obvious one does
not work.** `path-tiebreak` relaxed `BinaryHeap.upHeap`'s strict `<` to `<=` —
the defect that file's own header says "decides which of two equally good paths
a mob takes" — and it **fired and moved nothing**: `fired=1`, both binaries
green, 8 and 7 tests passing (`ci/t1-sabotage.sh --build` in
`~/rustmc-stackci2`, 2026-09-05 02:26). Every grid in those two test files is a
straight line or one detour whose optimal path is unique in shape, so no
equal-`f` choice reaches an assertion. `fired=1` with a green binary is the
mirror of `fired=0`: the code ran and the gate did not look.

What does reach them is `path-accuracy` — `PathFinder.findPath`'s `accuracy`
forced to 0, the shape of that loop before the argument was ported.
`ground_path::accuracy_stops_short` asserts the path ends at `(4, 64, 0)` for a
target at `(5, 64, 0)`, and every `move_to` in `navigation` paths at accuracy 1
and counts 5 nodes. It is a live parity claim rather than a synthetic one:
`docs/FINDINGS.md` records that `moveTo` "paths with accuracy 1, so a mob told
to walk to a block stops one node short of it".

## The red that only appeared once the cheap block was green

`ci/reference-cache-integration.sh`, `2 passed, 4 failed`, every failure on
`reference-cache: MISS — input unavailable: proto=MISSING`. `a55be66bc` added
`proto=` to `refcache_components` and updated the synthetic tree in
`ci/reference-cache-falsify.sh`, with the hazard stated in its own commit
message — without the stub sources "the harness computes `proto=MISSING` and
every lookup refuses, so the suite passes while testing nothing". **There are
two synthetic trees.** `reference-cache-integration.sh`'s `build_tree` was not
updated, so from 21:55 on 2026-09-04 the half of that script carrying its whole
claim — *on a hit, no vanilla boots* — was unreachable. Two files fix it and it
is `6 passed, 0 failed`. **A component added to `refcache_components` has to be
added to BOTH trees**; that is written beside the fix because the next one will
need it too.

This is also the argument for the paragraph below: it could not be seen until
T1-CONTROLS stopped aborting the cheap block.

## A finding about the gate rather than the branch

`crates/rustmc-data/src/sabotage.rs` had been fmt-dirty since `1b7cf9fd2` (5
hunks, the only fmt-dirty file in the tree) and **no gate had ever reached it**:
`cargo fmt --check` is `ci/t0.sh:1477` and the cheap block's one permitted exit
is `ci/t0.sh:1474`. A stack that has never been green above the cheap block has
never been checked below it, so expect more of these on any long-red branch and
run `cargo fmt --check` by hand before assuming otherwise.

## The reds behind the reds — budget for a chain, not for one

Every red above `ci/t0.sh:1474` conceals every check below it, so **the number
of outstanding reds on a long-red branch is unknowable until the first is
fixed.** This stack went: T1-CONTROLS, then `reference-cache-integration`, then
`check-test-targets` *and* `lockfile-falsify` together. Four box T0s to find
four reds, which is the expensive way to do it.

Two of them were main's, not this stack's:

* `cargo test -q -p rustmc-entity --test ground_path --test navigation` at
  `ci/t0.sh` arrived from main `f4fa28ff8` (`6abcb4d53`) **without `--locked`**.
  Nothing on main gates that; `ci/lockfile-falsify.sh` case 5 on this stack
  enumerates every cargo stage in the file, and case 1 shows what an unlocked
  stage costs — `rc=0, and git now sees a dirty tree`, mid-gate.
* the four `rustmc-entity` binaries are the T1-CONTROLS story above.

And two were this stack's own ledgers not covering their own tree:
`probe_cluster_boxes` and `probe_ruin_height` execute no assertion (both a
single `#[ignore]`d `#[test]`), were already named `unforceable` in
`ci/checks/t1-positive-controls.toml` and `max_ignored` in
`ci/checks/t1-known-red.toml`, and were missing the one declaration
`ci/check-test-targets.py` can adjudicate — `[[compile_only]]`. Neither is
retired: what is absent is the INPUT, and `probe_cluster_boxes` carries an
`assert!(visited > 0)` written so it "cannot go back to being a silent no-op
the day somebody removes the `#[ignore]`".

**The cheap order, learned here and worth keeping:** walk `ci/t0.sh` top to
bottom and run every check that needs no cargo locally first — about four
minutes for ~55 of them — and let the box run meet only the checks that need a
build. Every check in that set states its own status honestly; the only trap is
reading it through a pipe, since `x=$(check | tail)` takes `tail`'s status and
is green over a red gate.

## The next item

The four `rustmc-entity` binaries are `demonstrated` on the fixture and
sabotage axes. **Nothing yet covers the jar-golden half that
`walk_node_evaluator_golden_needs_a_running_server` and
`path_navigation_golden_needs_a_running_server` name as deferred**, and both
say why: their vectors need a `Mob` in a `Level`, which needs a running server.
That is the item, and what closes it is a server-backed golden for
`WalkNodeEvaluator`, not another arm.
