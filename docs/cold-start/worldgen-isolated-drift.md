# COLD STARTS — branch `worldgen-isolated-drift`

Append a new `### COLD START` entry rather than editing an old one.

### COLD START — branch `worldgen-isolated-drift` off `main` (`d049ef5a`), with `worldgen-generate-once` (`dd6f029d`) merged in, 2026-08-12

**THE ONE LINE: §32.3's "~200 blocks across three builds" is not build
sensitivity. `RUSTMC_DECORATE_ISOLATED=1` was not a function of its inputs at
all — one binary, one seed, one window, nine runs, THREE different worlds —
and the mechanism is one shared flag whose own comment names the level
barriers the isolated driver deletes.**

Worktree `~/grit/work-drift`, remote dir `~/rustmc-drift`, logs in
`~/rp-logs/selfrepro-*.log` and `~/grit/private/drift-*.log`. Write-up:
`docs/DETERMINISM.md` §33; §32.3 corrected in place.

**What was measured, in order.**

1. **The cheap test first, and it settles it.** Determinism needs no
   reference: same binary, same seed, same window, twice, byte-identical.
   `ci/isolated-selfrepro.sh` runs N dumps at each of several
   `RUSTMC_WORKERS` and cross-diffs every pair. Seed 20260722, r=7 at
   (64,64), 225 chunks, 22,118,400 blocks, VANILLA NOT INVOLVED (an
   ours-vs-ours diff makes no parity claim).
2. **Control: the shipped `extent` arm is clean.** Nine runs at workers
   1/12/auto, **0 mismatches on all 36 pairs**. So every parallel stage
   upstream of decoration — noise, surface, carvers — is reproducible, and
   what follows belongs to the isolated decoration driver alone.
3. **The isolated arm produced three worlds from one binary.** Three
   sequential runs identical to each other; the six parallel runs in classes
   1 / 99 / 123 from sequential, 24 / 100 / 124 from each other. **Every
   parallel run differed from the sequential run.**
4. **What moved names the structure.** `--positions`: `cobweb`, `rail`,
   `oak_fence`, `wall_torch`, `oak_planks`, a `spawner` present in one run
   and absent in another, `deepslate`/`cave_air`/`air` — all inside mineshaft
   boxes in chunks (71,57) and (70,63). One structure's random stream one
   draw out of phase.
5. **The mechanism, and the source says it.** `spider_placed` carries
   `MineShaftCorridor.hasPlacedSpider` between the chunks a corridor reaches
   — correct, it is a piece field vanilla saves as `hps`. Its declaring
   comment: *"it is only safe because the two chunks that could race for one
   entry cannot run at the same time … the parallel driver below orders any
   two chunks within `2 * DECORATE_REACH`"*. That is the shared-canvas pass's
   **level barriers**, and retiring them is the one thing the isolated model
   buys. Two chunks of one corridor then run together, both read the flag
   false, both spend the `nextInt(3)` that picks the spawner's z, and every
   later draw in the losing chunk's mineshaft pass lands one place early.
6. **Fixed by the model, not by a lock.** `SpiderMemory::Pass` keeps
   vanilla's carry for the shared-canvas arms; `SpiderMemory::Chunk` is one
   memory per decorating chunk under isolation, because a flag another chunk
   set is another chunk's write reaching this one by a different name, and it
   was the only such reach left in the pass. Serialising it instead would
   make a chunk's writes depend on which other chunks ran first — the exact
   dependence that arm exists to remove.

**Killed as a null — do not re-run these.** Every one was eliminated by
measurement or by the compiler, not by reading:

* **A hash container's iteration order.** Would move the `extent` arm too;
  `extent` is 0 on 36 pairs. The one `HashMap` in the driver (`level_of`) is
  in the shared-canvas branch and is only ever `get`; the canvas is
  `BTreeMap` throughout.
* **`f32`/`f64` reordered by codegen.** Codegen cannot differ between two
  runs of one binary, and the arm moves between two runs of one binary.
* **An uninitialised or reused buffer.** Fresh `clone_region` canvas and
  fresh `Vec` write log per chunk.
* **A `thread_local` carried between tiles.** Bounded from above by the
  compiler: `thread::scope` requires captured shared state to be `Sync`, so
  nothing reachable across the isolated workers can be a `Cell`/`RefCell`.
  The `Mutex`es are three: the processor-list cache (pure), the refusal list
  (diagnostic), and `spider_placed`.
* **Rayon or a thread-count dependence in the scheduler.** No rayon; a
  hand-written `AtomicUsize` queue. The thread-count dependence *was* the
  defect and is now its own control — workers 1, 12 and auto agree.
* **Bisecting §32.3's three builds.** Do not try: the three readings are not
  attached to three commits. All of §32.3's numbers are already present in
  the tree at `81b6533d`, so the builds that produced them were intermediate
  working states that never landed, and bisection has nothing to walk.

**What §32 keeps.** The 4.8x stands: the isolated arm is ~42k against
**both** references against a reference-pair spread of 10,067, so rule 6
fires and a ±124-block wobble does not touch it. The End city window's
99.998861% stands — its 168 blocks are all `chorus_plant`/`chorus_flower`,

(Registered unquotable: it is **not the model this server runs**, which is the whole point of the isolated arm.)
none of them a mineshaft. What does not survive is the sentence "Both models
are byte-identical to themselves: two dumps of each", corrected in place.

**Gate state at hand-off.** See the `### GATE` block appended below.

**The single next item, and the result that closes it.** Point
`ci/isolated-selfrepro.sh` at the **nether** and **End** drivers. Both have
their own `IsolatedDecoration` call sites (`generate_nether`, `generate_end`)
and neither has a mineshaft, so a non-zero there is a *second* mechanism
rather than this one. **It closes when both come back 0 over nine runs at
three thread counts, or names what moved.** After that, §32.6's item — one
canvas per world, a status per chunk, world-anchored decoration order — is
still the open one, and this section is a reason to run the self-reproduction
sweep on it the day it exists rather than a month later.

### GATE — branch `worldgen-isolated-drift`, 2026-08-12

* **`ci/end-city-window.sh drift-shipped`: 100.000000% against BOTH twins**,
  seed 20260722, centre chunk (83,62), r=7, y 0..255, denominator 14,745,600,
  **0 mismatches**, VANILLA FIRST, against `van-end-20260722-c8362` and its
  `-b` twin. The standing must-not-regress figure holds. Run because the
  change touches a structure-placement call site, even though `generate_end`
  never constructs a `SpiderMemory` — rule 9.
* **The shipped model is byte-identical across the change.** Post-fix
  `extent` dump against a pre-fix `extent` dump, seed 20260722, r=7 at
  (64,64), whole window: **0 of 22,118,400**. Not an argument from the
  signature of the diff, a byte comparison.
* **`ci/isolated-selfrepro.sh` after the fix**: nine runs at
  `RUSTMC_WORKERS` 1 / 12 / auto, **0 mismatches on all 36 pairs**, both
  arms.
* **`ci/t0-remote.sh` on the committed tree: T0 GREEN**, `skipping: no vanilla` count 0.
* `cargo test --release -p worldgen-assembly --lib`: the two new tests pass,
  and the isolated one was witnessed **red** under the injected pre-fix
  behaviour.
* `cargo clippy --release -p worldgen-assembly --lib --bins -- -D warnings`:
  clean. `cargo fmt --check`: clean. `ci/check-doc-claims.py`: 61 claims, 0
  collisions, 0 drift. `ci/check-figures.py`: GREEN.
* **No corpus run.** This branch touches neither `ci/checks/` nor
  `crates/rustmc-server/`, so `t0-remote` is the whole gate condition. The
  box-wide corpus lock was held by `rustmc-stack` throughout with one waiter
  already queued; nothing here took it.

### COLD START — branch `worldgen-isolated-drift`, 2026-08-16 — rebased onto `911c2e04` and landed

The measurement above is unchanged and was not re-taken. What this entry
records is the **rebase resolution**, because it was the reason the branch sat
unpushed and because getting it wrong is expensive in a way that is invisible.

**Thirteen of the branch's commits were already on `main` under different
SHAs** — §31's margin work, §32's generate-once measurement, the pyramid
port, the parallel-pass keep. `git rebase origin/main` cannot see that (the
patch-ids differ) and tries to re-apply all of them, which is what produced
"three source conflicts" in `crates/rustmc-worldgen/src/pipeline.rs`,
`crates/rustmc-server/src/tiles.rs` and
`crates/rustmc-server/tests/tiled_world.rs`. **All three are doc comments and
two have an empty branch side**: main's text is a superset written later, and
the branch was trying to restore the older paragraph.

The resolution, reproduced here and verified: **rebase only the tail**,
`git rebase --onto origin/main dd6f029d`, nine commits. After it the branch
touches **none** of those three files. Its whole footprint against `main` is:

```
ci/figures.toml
ci/isolated-selfrepro.sh
docs/DETERMINISM.md
docs/WORKLIST.md
docs/claims/DETERMINISM-33-isolated-arm-was-not-a-function-of-its-inputs.toml
docs/cold-start/worldgen-isolated-drift.md
tools/worldgen-assembly/src/generate.rs
```

**`docs/DETERMINISM.md` is spliced, never unioned.** `ci/merge-appendonly.py`
refuses these commits by design — they are not pure appends — and a union
re-appends §31 and §32, which `main` already holds, and trips
`DUPLICATE-HEADING`. Take `main`'s file and `git apply` each commit's own hunks
onto it. The result carries **one** `## 33.` heading, `main`'s §30/§31/§32/§35
untouched, and §33 last. §32.3's two in-place corrections are kept: they exist
to stop `main` publishing "both models are byte-identical to themselves", which
§33 refutes, and dropping them would leave a false sentence standing.

**KILLED AS A NULL — do not re-run.** The three source files are not a
three-way merge problem and no amount of care in `pipeline.rs` is needed. Do
not hand-merge them; take `main` and cut the rebase base instead.

**Verified before pushing:** `ci/check-doc-claims.py` 72 claims, 0 collisions,
0 drift. `ci/check-figures.py` GREEN, 84 registered. `ci/liveness-proofs.py
--check` 5/5, 0 pinned unsound — `ci/isolated-selfrepro.sh` adds no unsound
wait loop. `cargo fmt --check` clean, `cargo clippy -p worldgen-assembly
--all-targets -D warnings` clean, and
`generate::tests::an_isolated_chunk_does_not_inherit_a_neighbours_spider_corridor`
passes on the rebased tree — which matters, because `main` restructured
`generate.rs` heavily (`generate_resuming`, `Resumed`) and `SpiderMemory` had
to land on top of that rather than beside it. Threadripper, `~/rustmc-drift2`.

**Next item, unchanged:** §33.6 — `Tiles` is still not rewired onto the canvas
model, and that is the memory half.
