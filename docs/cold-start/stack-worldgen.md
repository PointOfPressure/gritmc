# COLD START — branch `stack-worldgen`

Append-only. Newest entry last.

### COLD START — branch `stack-worldgen`, 2026-09-05

**What this branch is.** One gated stack built from `main` f4fa28ff8 out of the
worldgen, structure, census and light branches that had been unlanded since
26 August. 386 distinct patch-ids existed across 26 branches; the stack carries
**370 commits**, and the difference is worldgen-parallelism (13, left out) plus
the duplicate copies four branches carried of each other's work.

**What folded, and what contains what.** Deduplicated by
`git patch-id --stable` before anything was rebased, which is the only measure
that survives these branches: four of them are absorbed WHOLE by
`worldgen-land` and add nothing at all —

    worldgen-be-save-arms      60 patches, 56 shared with worldgen-land
    worldgen-jigsaw-random     50 patches, 47 shared
    worldgen-block-entities    23 patches, 21 shared
    worldgen-pipeline-parallel  5 patches,  5 shared

The four commits `worldgen-be-save-arms` holds that `worldgen-land` does not
are two superseded doc/renumber commits and two commits `worldgen-land` carries
in a corrected form under the same subject line. **Landing `worldgen-land`
lands all four branches. Do not queue them separately.**

Folded, largest first: `worldgen-land` (112), `wall-carried-connective` (60,
contains `shape-three-position-extract` 42), `pale-census` (43, contains
`feat-undoworld` 27), `bastion-nether` (38, contains `structures-surface` 19
and `village-pool-elements` 12), `light-section-data` (33), `feat-undoworld`,
`floors-n-campaign` (24), `sky-light-model` (22), `end-biome-census` (20),
`clamped-floor-probe` (12), `worldgen-perf-unattributed-14-decide` (11),
`worldgen-isolated-drift` (10), `feat-decor` (8), `worldgen-aggregate` (6),
`overworld-biome-census` (6), `regiondiff-live-fixture` (4),
`worldgen-sulfur-r18` (3), `worldgen-perf-unattributed-14` (1).

## THE ONE BRANCH LEFT OUT, AND WHY IT CANNOT BE UNIONED

**`worldgen-parallelism` (13 commits) is out.** It and `worldgen-land`
independently implement the same thing — dividing the carver stage over the
chunk pool — under two different trait names: `CarveBlocks` on
`worldgen-parallelism`, `Blocks` on `worldgen-land` (`4f9aa7437`, "carve on the
pool"). The designs are the same shape and the code is not, so a hunk-level
union of `generate_resuming` produces **two complete carve drivers in one
function**, one holding atomic counters and one holding plain `u64` shadows of
them, and the tail reads `.load(Ordering::Relaxed)` off whichever survived.
That is not a merge conflict to resolve; it is a decision, and the decision
needs a measurement this lane may not take: the branch's §104.7-104.11 numbers
are properties of ITS implementation and cannot be restated over
`worldgen-land`'s without re-running `ci/par-identity.sh` on the merged tree.

So: land `worldgen-land`'s carve, then rebase `worldgen-parallelism`'s
**docs and `ci/par-identity.sh`** onto it and re-take §104 there. Its two code
commits (`the carver sweep asked the same question 140 times` and `divide the
carve`) are the ones to drop.

## THE THREE INTEGRATION DEFECTS ONLY THE COMBINATION HAS

None of these is a defect of any branch alone. Each is why a stack is worth
building rather than landing branches one at a time.

1. **`UndoWorld` satisfies neither `BlockEntityRecorder` nor `BlockPileWorld`.**
   `worldgen-land` makes `BlockEntityRecorder` a supertrait of
   `MonsterRoomWorld`; `pale-census`/`bastion-nether` add `BlockPileWorld` to
   `FeatureWorld`. Measured on all six branches: **0 impls each**, because
   every branch alone has only one of the two supertraits and `dyn
   FeatureWorld` is still implementable for the wrapper. Same hole in the
   test-only `TreeOnly`.

   The recorder impl **buffers rather than delegating**, and that is a
   correctness finding rather than a convenience: `Canvas::record_block_entity`
   appends to a per-chunk list with no remove, so `rollback` cannot reach a
   record. A tree that recorded a block entity and then refused would restore
   the block and leave the entity. Records are flushed by
   `commit_block_entities` on the Ok arm of `tree::place`.

2. **`place_piece`'s three container-record tests** pass a `(i32, i32, i32)`
   where `PieceOrigin` now goes. The tests are `worldgen-land`'s, the signature
   is `bastion-nether`'s.

3. **The nether bastion hook** threaded a `WorldgenRandom` unconditionally
   through `bastion-nether`'s `VillageCtx`, where the stack routes every jigsaw
   start through `place_jigsaw_starts` and keeps the piece stream behind
   `RUSTMC_JIGSAW_STREAM`. Threading it in the nether only would have made the
   knob mean one thing in the overworld and nothing in the nether.

## THE CONTRADICTION THAT WAS DECIDED, AND THE DOC LINE THAT DID NOT FOLLOW

`bastion-nether` and `village-pool-elements` PLACE village
`feature_pool_element`s; `worldgen-land` refuses them, deliberately —
`5fa6d01ec`, *"village-pool-elements lands its ports and its census, NOT its
placement path — the jigsaw branch already owns that code"*. The stack follows
`worldgen-land`.

**But `origin/worldgen-land`'s own tip publishes the opposite.** Its
`docs/WORKLIST.md` row says *"All 99 are now placed and nothing else moved"*
while its `generate.rs` has `features.get(feature` at **0 occurrences**
(`origin/bastion-nether` has 1). That row is a doc claim its own branch's code
does not implement, it is still on this stack, and **it is the next thing to
fix here** — either by landing the placement path with its stream cost
measured, or by rewriting the row to say the elements are refused and why.

`RUSTMC_VILLAGE_FEATURE_SKIP` is left registered with no consumer, which is the
state `worldgen-land`'s own WORKLIST line already describes
("NO CONSUMER TODAY").

## WHAT WAS REPAIRED TO MAKE T0 GREEN, BY CHECK

* **`check-conflict-markers`** — `docs/DETERMINISM.md:151` carried a bare
  `||||||| base` and the base text twice, with no `=======` and no `>>>>>>>`,
  on both `floors-n-campaign` and `end-biome-census`. A `<<<<<<<` grep does not
  find that shape.
* **`check-doc-claims`** — four DETERMINISM sections were carried twice, each
  by two branches that had written the same finding (33/46 the worldgen
  aggregate, 36 and 37 the regiondiff floors, 34 the r=18 sulfur adjudication).
  The aggregate section is renumbered **46 -> 55**, because both numbers it had
  claimed are taken on main.
* **`check-figures`** — 40 failing rules -> 0. Fourteen entries were registered
  twice (kept copy is always the post-rebase one: full `provenance`,
  `compare_keys` at twelve of twelve, `min_cites = 38`); fourteen had
  `provenance` naming no commit and are recovered with
  `ci/figure-provenance.py`, each recording its witness count because a
  single-witness answer is an upper bound; six `claim_count`s move up because
  the fold genuinely added mentions; the End per-biome section of
  `docs/PARITY.md` was restored from `end-biome-census`'s tip, which is why
  `end-per-biome-n6` had drifted the wrong way (5 mentions where the ledger
  said 14). Both ratchets were repaired rather than raised: ECHO-UNQUALIFIED
  26 -> 13 by printing the disqualifier beside each number, SELF-ARTEFACT 9 ->
  7 by pointing the two aggregate composites at `ci/figures.toml` and the
  component id that dominates them.
* **`check-fixture-skips`** — 20 raw sites against a declared 1. Twelve were
  dead text (`load_tags`, `state_spec::extract_or_skip`, `pale_oak_config`,
  `worldgen_assembly::have_extract` had all adopted `fixture::available` and
  their callers kept the old `eprintln!`); seven were real and are converted.

## NULLS — DO NOT RE-RUN THESE

* **Do not try to union `block_pile.rs`, `huge_mushroom.rs`,
  `tree/decorator.rs` or `generate.rs` hunk by hunk.** Every one of those
  conflicts has an EMPTY merge base with both sides non-empty, and a keep-both
  resolution there emits the region twice. Eleven such duplications were found
  by `cargo check`, not by reading: a duplicated `pub mod`, a duplicated `use`,
  a duplicated match arm at the pre-vtable arity, a duplicated field init, a
  duplicated `#[expect]`, a closure defined twice, and `status.rs`'s
  random-tick block losing its two closing braces to a block inserted inside
  it. **Take one side whole and re-apply the other side's own patch.**
* **`ci/figure-provenance.py --check` is not this stack's fault.** It compares
  an abbreviated sha with `in`, so it breaks whenever git's `%h` width changes;
  369 commits above main the width is 9 and every 8-character provenance stops
  matching — 56 of 70. It is already the queue's tier-0 item on `boxcopy`
  (`ci/queue-order.py`, `MAIN_IS_BROKEN`), and the gate never sees it because
  a gate directory has no `.git` and the check skips there.
* **No recording on this box holds both `711-a-daylight-detector-recomputes-its-power-from-the-sky`
  and `788-a-chest-is-in-its-chunks-payload-when-the-chunk-returns`.** Swept
  every `~/rustmc*/oracle-work/gameplay/vanilla.json`: `~/rustmc-rec` has 711
  and not 788, `~/rustmc-chunksend` and `~/rustmc-light` have 788 and not 711.
  A whole-ledger `--sweep` against any one of them therefore trades one
  UNREGISTERED for another.

## THE SINGLE NEXT ITEM

**Land the village `feature_pool_element` placement path with its piece-stream
cost measured, or leave it refused and keep the docs saying so.**

The doc half is done on this stack: `docs/WORKLIST.md`'s
`village-pool-elements` block used to open *"Closed, repaired, gated … All 99
are now placed and nothing else moved"* while `generate.rs` refused every one
of them, and it now opens by saying the placement path is not in this tree and
citing `415fc50fb` (`5fa6d01ec` before the rebase) as the decision. **The code
half is still open**, and it is a measurement rather than a merge:
`FeaturePoolElement.place` spends the placed feature's draws on the same
`RandomSource` the containers take their `LootTableSeed` from, so threading it
moves every later container in that start. The closing result is
`features.get(feature` reaching 1 in `tools/worldgen-assembly/src/generate.rs`
with the stream cost on the r=16 window stated beside the -99 blocks it buys.

### COLD START — branch `stack-worldgen`, 2026-09-05, the two reds after the ledger

Worktree `~/grit/work-stackwg` (local branch `stackwg-work`); remote dir
`~/rustmc-stackwg`; logs `~/.rustmc-detached/stackwg-*.log`. Picked up after
this stack's own lane stopped, with T0 red on the grades ledger.

**The ledger drift.** 110 scenario files against 108 rows —
`711-a-daylight-detector-recomputes-its-power-from-the-sky` and
`788-a-chest-is-in-its-chunks-payload-when-the-chunk-returns` had no row.
Re-taken the way the check's own message says, no lock and no server:
`ci/corpus-history.py` over every recording on the box against
`~/rustmc-stack/oracle-work/gameplay/vanilla.json` (copied aside first, so a
concurrent gate could not move it mid-run), then `ci/corpus-grades.py --write`.
**110 checks, 42 moved / 6 strict-only / 60 frozen**, inside
`HISTORY_MOVED_FLOOR = 41` and `HISTORY_FROZEN_CEILING = 61` — which the
ledger it replaced sat exactly on, so there was no headroom in either
direction and the numbers are quoted rather than argued.

Note for anyone comparing this to `stack-gameplay`'s ledger: **the two
denominators are not comparable.** This tree's `oracle-runner` reads all 65
recordings on the box; `stack-gameplay`'s refuses 38 of them with
`unknown variant 'probe'`, and takes its reading over the other 27.

**Two clippy lints, and neither wanted the code changed.**
`if_same_then_else` at `crates/rustmc-worldgen/src/feature/huge_mushroom.rs::tree_radius_for_height` (`:138`) is correct that the two
arms are identical and wrong about the repair: vanilla's two arms are
identical too. This module's header quotes the jar body — two separate tests
both returning `radius`, with `height` bound to the literal `-1` at the only
call site — so merging them into `y >= -4 && y <= -1` would be arithmetically
equal and would erase which of vanilla's tests a `y` fell through. `#[allow]`
with the bytecode reason. The unfulfilled
`#[expect(clippy::cast_precision_loss)]` at `crates/rustmc-worldgen/src/feature/large_dripstone.rs::height_at_radius` (`:278`)
is the opposite: the comment two lines above it already says why the
expectation cannot be met (`f64::from` on both widenings is exact), so the
expectation was a claim about the code that was not true. Dropped.

**Do not try to gate this locally.** This worktree's `../plugins` sibling puts
`Cargo.lock` out of date, so `cargo clippy --locked` refuses outright and an
unlocked run silently measures a different dependency set — it reported two
`type_complexity` errors in `worldgen-assembly` that the box's own run does
not have. Everything that needs cargo belongs in `~/rustmc-stackwg` via
`ci/t0-remote.sh`; everything that does not runs here in about four minutes,
which is the cheaper order and is how these two were found before paying for a
box run.
