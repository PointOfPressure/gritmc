# Task: Density-function operators — the wave-2 long tail

## Standing rules (stable prefix — do not edit per-task)

You are one of 3 worker agents building rustmc, a Minecraft-compatible
server in Rust. Non-negotiable rules:

1. You work ONLY in this worktree, on the current branch. Never touch
   `main`, other worktrees, `~/minecom`, or `vanilla-src/`.
2. Before every commit: `ci/t0.sh` and `ci/t1.sh` must pass. Never commit red.
3. Frozen interfaces (marked FROZEN in crate docs) are read-only. If your
   task cannot be done without changing one, STOP and write an escalation
   (see report format) — a half-done interface edit costs more than a stall.
4. Port behavior from the cited reference classes; never copy code. Cite the
   class in a comment at the porting site, Minecom-style.
5. No `static mut`, no ambient singletons, no HashMap iteration at
   behavior-visible sites (ordered containers there), no tolerance-widening
   in tests, no fast-math.
6. Assert invariants, never instantaneous cross-thread state.
7. If a test fails twice for the same reason you don't understand, stop and
   escalate with the evidence. Do not attempt a third blind fix.
8. Deliverables must be cherry-pickable: docs/log entries committed alone;
   code in separate commits.

## Report format (final message — this is all the orchestrator reads)

- LANDED: commits + one-line what/why each
- EVIDENCE: gate output tails proving green (T0/T1 + task-specific)
- ESCALATED: blocked items, each with mechanism/evidence/blocked-on
- NOT TOUCHED: confirm main/other-trees/minecom untouched

## ⚠️ The decompile cache lies — check the jar

`~/minecom/vanilla-src/` mixes versions. `NoiseRouterData.java` there is a
**pre-26.2** decompile: it calls `DensityFunctions.weirdScaledSampler(...)`
and `WeirdScaledSampler.RarityValueMapper`, **neither of which exists in
26.2** (`unzip -l` the server jar and grep — there is no
`DensityFunctions$WeirdScaledSampler` class). 26.2 replaced it with
`NoiseRouterData.QuantizedSpaghettiRarity.wrapRarity2d/3d`, which are
`intervalSelect` over rarity bands, and separately added a `flatCache`
inside `spaghetti2D`.

**Before porting any class, confirm it exists in the jar and re-decompile
if in doubt:**

```
unzip -l ~/mc-26.2/versions/26.2/server-26.2.jar | grep <ClassName>
# extract to a TEMP dir (never write into ~/minecom):
java -jar ~/minecom/vanilla-src/tools/vineflower.jar <classdir> <outdir>
```

Porting from the stale copy would have implemented an operator that no
longer exists and missed a real behavioral change. This is the failure mode
that produces confident, wrong code.

## Read these sections (nothing else)

- `crates/rustmc-worldgen/src/density/mod.rs` — **the whole file.** The
  module docs explain why bounds are semantics; the `build` module is the
  only supported way to construct a node. Read the existing operators as
  worked examples before writing yours.
- `crates/rustmc-worldgen/tests/golden_density.rs` — the harness you extend.
- `tools/goldengen/vanilla/GoldenDump.java`, `densityVectors()` — where you
  add the vanilla side.
- `~/minecom/vanilla-src/net/minecraft/world/level/levelgen/DensityFunctions.java`
  — the reference. Your operator is in here.

Do **not** read the rest of the worldgen crate. The RNG and noise layers
below you are already proved bit-exact against vanilla; treat them as
correct and do not modify them.

## Why this task exists

The framework and 16 operators are done and green. What remains is a
long tail of independent operators. They are independent — each is a new
enum variant, one `compute` arm, one bounds rule, one builder, one golden
case — which is exactly why three agents can grind them in parallel without
colliding.

**The oracle already exists.** You are not guessing at correctness: you add
your operator to `GoldenDump.java`, regenerate, and the test tells you
bit-for-bit whether you are right. If your operator does not have a golden
case, it is not done.

## Your slice

Take **one** group. Announce which in your first commit message so the other
two agents don't duplicate it.

**Group A — selection**
- `FindTopSurface` — iterative downward scan; note it walks by `cellHeight`.

  > `IntervalSelect` is **done** (orchestrator) — 26.2's cave rarity is
  > built on it, so it moved onto the critical path.

  > `Spline` was in this group and is now **done** (orchestrator, commit
  > follows this file) because it gated `NoiseRouterData` and workers were
  > two days out. Read `density/spline.rs` before starting — it is the
  > worked example closest to what you are about to write, and it is the one
  > place in worldgen that evaluates in `f32`.

**Group B — caching markers — ✅ DONE (orchestrator)**
- `Marker` (all six kinds) landed as tagged passthroughs, which is what
  vanilla does at this layer: `Marker.compute` delegates straight to the
  wrapped function, and the caching the names promise materializes only
  when `NoiseChunk` wraps them for cell sampling.
- The one bounds quirk is ported: `BlendDensity` reports **unbounded**
  min/max rather than its wrapped function's.
- The real caching work moved to **w3 Group F** (`NoiseChunk`), where it
  belongs. Take another group.

**Group C — dimension-specific — nearly closed**
- `BeardifierMarker` — trivial (returns 0.0) until structure beardification
  is wired. Port the trivial form, cite it, name what it becomes later. Do
  **not** invent the beardification system. This is all that is left in
  Group C; consider taking Group A or a wave-4 slice instead.

  > `SimplexNoise` and `EndIslandDensityFunction` are **done**
  > (orchestrator), both bit-exact vs vanilla.

  > `BlendAlpha` / `BlendOffset` / `blendDensity` are **done**
  > (orchestrator) — they gated the wave-3 router. Note `BlendAlpha` and
  > `BlendOffset` are their own variants and deliberately NOT `Constant`
  > nodes: `build::lerp` branches on `Constant`, so collapsing them would
  > silently take the wrong expansion.

## Definition of done (per operator)

1. A variant on `DensityFunction`, an arm in `compute`, arms in `min_value`
   and `max_value`, and a constructor in `build` that computes any bounds
   the evaluation path reads.
2. A case in `GoldenDump.java`'s `densityVectors()` under a new key, built
   with vanilla's own public factory where one exists.
3. The matching arm in `golden_density.rs`'s `tree()`. **Never** add a key
   to the skip path — the test panics on unknown keys by design.
4. `cargo run -p goldengen` regenerated, and the diff reviewed: your new
   case should be the *only* thing that changed. If unrelated vectors moved,
   stop and escalate — that means you changed a lower layer.
5. `ci/t0.sh` and `ci/t1.sh` green.

## Traps, from the layers below

These cost real time on the first three rungs. They will bite you too.

- **Java's `Math.min`/`max` are not Rust's.** Use `math::java_min` /
  `java_max`. Rust returns the non-NaN operand; Java propagates NaN.
- **Float literals in double context.** `1.0E-7F` is an `f32` widened to
  `f64`, which is *not* the `f64` nearest that decimal. If the reference
  has an `F` suffix, port it as `<literal>_f32 as f64`.
- **No algebraic simplification, ever.** `p0 + a*(p1-p0)` and
  `(1-a)*p0 + a*p1` differ in the last bits. Copy the reference's
  association order exactly, including which operand is subtracted.
- **`Mth.floor` returns an `int`**, so `floor(x) * y` is int*double. Widen
  where the reference widens, not before.
- **Bounds feed short-circuits.** If you give an operator a sloppy
  `min_value`, a `Min` node above it will skip evaluating a branch it
  should have evaluated. Bounds are not documentation.
- **Construction-time rewrites are observable.** See how `build::two_argument`
  turns constant-folded `Add`/`Mul` into `MulOrAdd` and loses the zero
  short-circuit. If your operator has a smart constructor in vanilla,
  it is there for a reason — port it.

## Escalate rather than guess

- Your operator needs `NoiseChunk` / cell-based interpolation state → that
  scaffolding is not built yet. Implement uncached semantics, flag it, move
  on.
- Your operator needs a frozen interface changed → stop, write the
  escalation, take the next operator in your group.
- Vanilla's golden value disagrees with your port twice and you don't know
  why → stop. Include the failing key, both bit patterns, and the reference
  lines you ported from. Do not widen a tolerance; the previous three rungs
  landed bit-exact and so can this one.
