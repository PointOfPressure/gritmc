# Task: Noise router + chunk sampling — wave 3

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

## ✅ Unblocked — the wave-2 dependency is landed

This task originally waited on wave 2, because
`NoiseRouterData.splineWithBlending` is

```java
flatCache(cache2d(lerp(blendAlpha(), blendingTarget, spline)))
```

and `lerp` falls to its `cacheOnce` branch whenever `blendingTarget` is not
a `Constant` — which `blendOffset()` is not.

Those operators are now in, verified against vanilla:
`build::{blend_alpha, blend_offset, marker, interpolated, flat_cache,
cache_2d, cache_once, cache_all_in_cell, blend_density, lerp,
spline_with_blending}`.

Markers are tagged passthroughs at this layer — that is vanilla's own
behavior, not a stub. Real caching is **your Group F job** (`NoiseChunk`).

## What is already done (do not redo)

The whole stack below the router is landed and proved bit-exact against
real vanilla. Read these as worked examples, do not modify them:

- `rng/` — all random sources, the seeding ladder, positional factories.
- `noise/` — Improved/Perlin/Normal/Blended, plus `noise/registry.rs`
  (world seed -> named noise, the `from_hash_of("minecraft:<name>")` chain).
- `density/mod.rs` — the framework and 16 operators, `build` smart
  constructors, and the composites you will need:
  `build::lerp_constant`, `build::noise_gradient_density`,
  `build::offset_to_depth`.
- `density/spline.rs` — cubic splines (all `f32`).
- `density/terrain.rs` — `overworld_offset` / `overworld_factor` /
  `overworld_jaggedness`, amplified included.

## Your slice

**Group D — the router — ✅ DONE (orchestrator).**
`density/router.rs` has the overworld path: shift_x/shift_z, the
continents/erosion/ridges 2D fields, ridges_folded, the shaper wiring into
offset/factor/jaggedness, depth, initial_density and sloped_cheese.
Verified against vanilla's own router (112 samples, 2 seeds x normal and
amplified). Take Group E or F.

> Three traps it hit, in case you touch it: `constant(-0.50375F)` is a
> **float** literal (~2.6e-8, propagates into every terrain height);
> TerrainProvider's `weirdness` argument is the **unfolded** RIDGES field
> while its `ridges` is RIDGES_FOLDED; and the 3D BlendedNoise is
> **re-seeded** by `RandomState.wrapNew` from
> `fromHashOf("minecraft:terrain")`, not seeded from the world seed.

**Group E — cave carving density — ✅ DONE (orchestrator).**
`spaghettiRoughnessFunction`, `entrances`, `spaghetti2D`, `noodle`,
`pillars`, `underground` and the `final_density` assembly are in
`density/router.rs`, verified against vanilla's own `router.finalDensity()`.
Take Group F.

**Group F — cell sampling (`NoiseChunk`).** The last piece before the chunk
generator, and the only one left with real design in it.

**Already landed for you** in `density/cell.rs`, both verified:
- `CellInterpolator` — eight corners staged through Y-then-X-then-Z, with a
  test proving the staged path (update_y per y, update_x per x, update_z per
  block) equals a direct sample bit-for-bit.
- `CellGeometry` — the lattice arithmetic, floor-divided throughout.

**What you build: the chunk-scoped evaluation.** Here is the crux, and it is
easy to get subtly wrong:

> `final_density` contains **five separate `interpolated(...)` nodes** — one
> inside `postProcess`, four inside `noodle`. Vanilla gives **each** its own
> interpolator, fills each on the lattice, and only then evaluates the
> surrounding operators per block. So "sample final_density on the lattice
> and interpolate the result" is **wrong**: it interpolates across `min`,
> `abs` and `rangeChoice` boundaries that vanilla evaluates per block.

The shape that works:

1. Walk the tree once, assign an index to every `Marker { Interpolated }`.
2. For each index, fill its lattice slices by evaluating that marker's
   *wrapped* function at cell corners.
3. Evaluate the tree per block with a context that carries the current
   per-index interpolated values; `Interpolated` reads its slot instead of
   recursing.

That needs `FunctionContext` to carry chunk state. `FunctionContext` is
**not** a frozen interface (the frozen list is in CRATES.md and is about
`Canvas`/`ChunkAccess`/`EntityView`/`BlockBehavior`/`Event`/`Scheduler`/the
region message enum) — so extending it is your call, not an escalation.
Keep `compute` cheap: this is the hottest path in the server.

Also yours, and welcome once the above is green: the batched per-operator
`fill_array` overrides vanilla has. Only behind the existing
`fill_array_agrees_with_compute` invariant in `tests/golden_density.rs` —
extend it, never weaken it.

## Definition of done

1. The router builds and evaluates for a given world seed without panicking
   at any y in `-64..=320`.
2. **Golden vectors from vanilla.** `routerVectors()` in
   `tools/goldengen/vanilla/GoldenDump.java` dumps depth / continents /
   erosion / ridges / **final_density** against vanilla's own router, plus
   every noise parameter in the registry. It is green — 140 samples across
   2 seeds x normal and amplified. Extend it for whatever you add; never
   weaken it.
3. `ci/t0.sh` and `ci/t1.sh` green.

## Why this matters more than it looks

Everything up to here has been provable in isolation. The router is the
first place where a wrong answer looks *plausible* — terrain that generates,
loads, and looks like Minecraft while being a different world. The only
defence is the vanilla vector, so write it first and implement against it.

The rung after this one is the chunk generator writing real Anvil regions,
at which point `ci/t2-regiondiff.sh` starts producing an actual parity
percentage against vanilla instead of testing the harness on flat worlds.
That number is the project's headline claim. Do not let it start life on a
shortcut.

## Traps carried forward

- Java's `Math.min`/`max` are not Rust's — `math::java_min` / `java_max`
  (and the `_f32` pair for spline-adjacent code).
- Float literals with an `F` suffix are `f32` widened to `f64`.
- No algebraic simplification. `lerp` is expanded exactly as vanilla
  expands it; see `build::lerp_constant`.
- Bounds feed evaluation-time short-circuits in `Min`/`Max`. A sloppy
  `min_value` makes a *different* node skip a branch.
- Construction-time rewrites are observable (`build::two_argument` folding
  constants into `MulOrAdd` drops `Mul`'s zero short-circuit).
- Splines are `f32` end to end; the router hands them `f64` coordinates and
  takes `f64` back, and the narrowing happens at those two boundaries only.
