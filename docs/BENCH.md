# Worldgen benchmarks

Measurements, with their method and their caveats. Numbers without both are
not measurements.

> **One gate is deliberately red: `golden_surface_chunk`.**
>
> It went red when it was *widened* to a third seed and found a whole surface
> branch that has presumably never worked — `packed_ice` in `frozen_peaks`,
> 4,110 blocks in nine chunks. It has not been narrowed back, because a green
> gate that does not cover this is worth less than a red one that does.
>
> Everything else is green: 550 library tests, and every other golden gate
> including `golden_ocean_floor_wg`, `golden_structure_starts`,
> `golden_carver_chunk`, `golden_climate` and `golden_assembly_chunk`.
>
> The cause is isolated to `stoneDepthAbove` — see "the fence rows" section
> and the eleven eliminations recorded with it.

## The surface stage resolves its biome as an integer — 17% off the stage, byte-identical (2026-08-06)

Branch `perf-surface-biome-id`. `SurfaceOracle::biome` answered with a
`String` for each of the ~28,864 biome answers a chunk needs, and each answer
also cost a `BTreeMap<(i32,i32)>` walk over up to 4,761 entries to find the
container, a `BTreeMap<String, BiomeEntry>` walk to find that biome's
climate, and a byte comparison against every name the condition lists — all
of which begin `minecraft:`. Vanilla memoises a `Holder<Biome>` per y-update
and compares holder identity; **our call counts already matched vanilla's,
only the cost per call did not.**

What landed: a `BiomePalette` interning the datapack's whole biome directory
to a `u16` once, before the stage starts, so it never grows under the worker
pool and needs no lock (an `Arc` per answer was the alternative and this file
already records that shape as a contention tax); each chunk container remapped
to global ids once; a `Neighbourhood` resolving the 3x3 a chunk's fuzz can
reach once per chunk; `Condition::Biome` rewritten to `Condition::BiomeIds`
before the stage runs; and `vertical_gradient`'s two `BTreeMap` lookups on
the hit path reduced to one.

### Output-identical, proved

Against a pristine control built from the parent commit, **both trees on
byte-identical `data/vanilla`**, all three dimensions, at `RUSTMC_WORKERS=1`
**and** 12:

```text
  overworld r=16 (1,089 chunks)  w=1   0 of 4 region files differ
  overworld r=16                 w=12  0 of 4
  nether    r=8                  w=1   0 of 4
  nether    r=8                  w=12  0 of 4
  End       r=8                  w=1   0 of 4
  End       r=8                  w=12  0 of 4
```

Every parity number in this file is therefore unchanged **by construction** —
they are all functions of exactly these bytes. Checked rather than assumed
that this was not two copies of the same tree agreeing with each other: the
changed tree contains the new symbols and the control contains none of them,
and the two binaries have different digests.

**Re-run at the rebased base on 2026-08-07 and still byte-identical.** An
identity proof is a statement about *two named trees*, so it does not survive
a rebase on its own authority — the control moved. Control rebuilt from `main`
at `add7297a` (1,501 commits, 34 ahead of the base this branch was written
on), subject rebuilt from the rebased tip, both from scratch, same six rows,
same `0 of 4`. The same anti-tautology check was re-applied and passed: both
binaries were compiled in this run, they differ in size and digest, and
`BiomePalette` appears in the subject's symbols and not in the control's.

That the answer did not move is not luck and can be stated in advance: of the
34 commits `main` gained, **not one line falls in `crates/rustmc-worldgen`,
`crates/rustmc-world`, `crates/rustmc-data` or `tools/worldgen-assembly`** —
`worldgen-dump`'s entire dependency closure — every one of them landing in
`crates/rustmc-server` instead. The re-run is what turns that argument into a
measurement.

### The cost, alternated arms

```text
  worldgen-dump, r=16, stage CPU-s        ctl      new     ratio
    12 workers  total                    70.49    68.04   1.0360x   (n=3)
                surface                  16.40    13.61   1.2053x
     1 worker   total                    63.12    60.74   1.0392x   (n=4)
                surface                  14.97    12.51   1.1962x
```

**The surface stage falls ~17%; the whole pipeline falls ~3.6%.** The absolute
saving is ~2.4 CPU-s at *both* widths, which is what removing
width-independent work has to look like.

**The predicted third did not arrive, and the profile that predicted it says
why.** The closing condition asked for a third on the strength of "44% of the
stage is resolving a biome id", but 13.5 of those points are
`quart_for_block` — the fuzz vanilla computes too, which the same profile
calls "not a defect", and which is still computed on every query because it
must be. The removable share was ~30%, not 44%.

### The falsification sweep — and the control that failed

Eight deliberate bugs, one at a time, each built and dumped and compared
against the same pristine control the real proof uses. **First, the check
that makes any of this mean anything:** the same binary run three times
produces byte-identical output, so the compare reports "equal" only when
things are equal.

```text
  cell-index-xz-swapped          CAUGHT   4 of 4 region files differ
  neighbourhood-slot-reflected   CAUGHT   4 of 4
  vertical-clamp-dropped         CAUGHT   3 of 4
  climate-always-first           NOT CAUGHT  -- see below
  id-of-always-zero              CAUGHT   4 of 4
  biome-condition-inverted       CAUGHT   4 of 4
  gradient-name-hardcoded        CAUGHT   4 of 4
  palette-renumbered             CAUGHT   4 of 4  -- THIS WAS THE CONTROL
                                                  -- and RETRACTED 2026-08-09,
                                                     it does not reproduce
```

**`climate-always-first` was not caught because the window cannot reach the
code, and that was chased rather than excused.** Giving every biome the first
biome's climate feeds only `cold_enough_to_snow` and
`height_adjusted_temperature` — frozen and cold ocean business — and seed
20260722 at (64,64) has none. So the byte-compare at that centre says nothing
about the climate table. Seven more centres were tried and **(-6000,-6000)
reaches it** (the injection changes 1 of 1 files there); the real change is
**byte-identical on that window at `RUSTMC_WORKERS=1` and 12**. The gap is
closed, but only because the sweep exposed it: "a gate a window cannot
exercise proves nothing".

**`palette-renumbered` was the control and it should have been undetectable.**
Rebuilding the palette from the same registry in reverse keeps interning
injective, keeps the climate table in step (the in-loop assertion would have
panicked and did not), and on the argument that every comparison the rules
make is an equality it cannot change a block. **It changed 4 of 4 region
files.** The dumper is deterministic, so this is real.

> **RETRACTED 2026-08-09 — it does not reproduce.** The row above stands as
> the record of what was observed; the conclusion drawn from it does not.
> There is no order-sensitive consumer, and the search for one is closed.
> `docs/DETERMINISM.md` §13 is the measurement, `ci/biome-palette-order.sh`
> is the gate, and `surface::ORDER_IS_NOT_OBSERVABLE` is the corrected note.

**What the re-measurement changed about the method, which is the part worth
carrying to the next sweep.** This sweep built a separate binary per injection
and compared each against a pristine control. That establishes determinism
across three runs of **one** binary and then compares **differently built**
ones — two different claims, with no arm able to tell "the injection moved
output" from "the build moved output". The re-run puts the choice behind
`RUSTMC_BIOME_PALETTE_ORDER`, read at run time, so **control and injection are
one executable**. It also splits what `reverse` conflates: `reverse` moves an
id's value *and* its order together, `shift N` moves only the value, so the
pair can say *which kind* of consumer exists rather than only that one does.

Same window as the claim — seed 20260722, r=16, centre (64,64),
`RUSTMC_WORKERS=1`:

```text
  forward vs forward     0 of 4    determinism control, same binary
  reverse                0 of 4    values AND order moved
  shift 1                0 of 4    values moved, order preserved
  shift 7                0 of 4
  poison (every id 0)    4 of 4    POSITIVE CONTROL -- must not be 0
```

**The `poison` row is what makes the zeros admissible.** A knob that never
reached the generator would report every injection as undetectable, and that
is rule 7's "you measured the instrument" — the failure this file has now paid
for fourteen times. It answers every biome query with id 0 and moves 4 of 4,
so the palette being perturbed is the live one and this window does resolve a
wrong biome id. The three palette digests differ as a second, independent
check. Repeated at r=7 on the same centre, and at r=7 centred on
**(-6000,-6000)** — the centre this very section identifies as the only one
that reaches the climate path — where it is **0 of 1** with poison at 1 of 1.

**And the arm this sweep lacked, now taken:** two `cargo build --release` runs
of one source into different target directories produce a **bit-identical**
`worldgen-dump` here, so a same-source rebuild control is *vacuous* rather
than passing. Forced apart with `RUSTFLAGS='-C codegen-units=1'` — a genuinely
different binary from identical source — the r=16 dump is unchanged. So
"a rebuild moves output" is not an available explanation either.

**The End's own window, checked separately.** The 100.000000% result is
measured at seed 20260722, centre (83,62), r=7 — not the (64,64) the main
verification used, and byte-identity on one window does not establish it on
another. That window is **0 of 2 region files differing at both worker
counts**.

### RULE 7 PAYOUT: an unpaired before/after cannot resolve this, and said the opposite

Run through `ci/bench-servers.sh` as two runs half an hour apart, the change
measured **+5.6%** at r=7 and **+0.8%** at r=32 — the wrong sign — on a box
with zero other harnesses witnessed before and after each run. Alternating
the same two trees through the same harness, C A C A:

```text
  pair      ctl     after    paired
    1     21.470   21.030    -2.05%
    2     21.570   20.240    -6.17%
    3     20.817   19.867    -4.56%
    4     20.727   19.983    -3.59%
                             -4.09% mean, every pair negative
```

**And the control that was missing all along:** the four `ctl` means are
21.47, 21.57, 20.82, 20.73 — a **4.0% range on one unchanged tree**, with a
downward drift across rounds. So `bench-servers`' run-to-run variance for a
single three-square run is the same size as the effect, and an unpaired
comparison cannot see it. The "CPU-seconds reproduce, rustmc -1.3%" recorded
elsewhere in this file was **six** squares pooled within one session and does
not license a three-square unpaired difference.

> **Do not quote an unpaired `bench-servers` before/after for anything
> smaller than ~8%.** Alternate the arms, as the tile-retirement memory work
> did.

Where that leaves the absolutes, applying the paired -4.09%:

```text
  r=32   0.060695 -> 0.058212   MEETS <=0.075  (the square the target is stated over)
  r=7    0.091481 -> 0.087738   NO VERDICT — the target has none at this radius
```

The r=7 row deliberately carries no verdict. The efficiency target is stated
over the r=32 square, and this file's own correction beside the four-server
table says the honest state at r=7 is **not comparable, therefore unresolved
— not failing**. Section 2 below is the measurement of *why*; it does not
license reading the target at r=7 either.

The -4.09% is a paired **r=7** result and applying it to the r=32 absolute is
a projection, not a measurement — the r=32 arm was not re-run after the
change. It is the conservative direction, which is why it is the one used: on
the affine model in section 2, a saving that lives entirely in the
per-generated-chunk coefficient costs 0.86 CPU-s over 361 generated chunks at
r=7, which is 0.00238 per chunk, which over 4,761 generated chunks at r=32 is
11.3 of 256.4 CPU-s — **-4.42%**, larger than the -4.09% applied here.

## The efficiency target is a function of the RADIUS it is measured at, and this file's figures are stale by a tenth (2026-08-06; drift ATTRIBUTED 2026-08-09, see §3)

**Three results. The second is the one that changes what this file should
say; the third is the one somebody has to act on.**

Measured on branch `perf-surface-biome-id` against **unmodified `main` at
`9b4afd4`**, `ci/bench-servers.sh` with `BENCH_FLAVOURS=rustmc`, seven radii,
three centres each, seed 20260722, Threadripper 1920X. **Zero other harnesses
were live before and after every single run**, checked by sampling each
candidate process's CPU rather than by matching names or reading the load
average — an abandoned vanilla server on that box matches `java` and burns
exactly nothing, and two contending corpus runs once sat at load 0.02 while
throughput collapsed tenfold.

### 1. The metric carries the measurement window, and the target was stated over r=32

`ci/bench-servers.sh` says so in its own header: the denominator is the square
**asked for**, `(2r+1)^2`, "because that is the work the caller receives",
while the numerator is the whole process — which also generated the
`DECORATION_MARGIN` ring around it, `(2r+5)^2`. Those ring chunks are not
partial work: `decorate_order_for(pipeline.generated())` walks the whole
canvas, so they run every stage, decoration included. They are simply
excluded from the divisor.

```text
  r=32   4,761 generated / 4,225 delivered = 1.127   (+12.7%)
  r=7      361 generated /   225 delivered = 1.604   (+60.4%)
  r=4      169 generated /    81 delivered = 2.086  (+108.6%)
```

The target was set as "1.5x better than Paper's 0.0614" and is stated in this
file **over the r=32 square**. The 0.082364 in the four-server table **below**
is an **r=7** number. It was once published in this file as "the efficiency
target is still not met at 0.0824" — in the **same sentence** that went on to
say "the two radii are not comparable and must not be quoted as a trend".
That verdict has since been withdrawn and the correction now stands beside the
table. This section is the
measurement of *why* the two radii cannot be compared, and it does not
reinstate a verdict at r=7 — there is none to reinstate.

### 2. Total CPU is affine in the GENERATED area — seven radii, two parameters

The model was written down, with predictions, in `docs/WORKLIST.md` **before**
this sweep ran, fitted on the two published points alone.

```text
  r  delivered generated margin   CPU_s  pred_s  resid%   CPU/chunk
   4        81       169  2.086   10.88   11.28   -3.67    0.134362
   7       225       361  1.604   20.58   21.54   -4.67    0.091481
   9       361       529  1.465   30.53   30.52   +0.03    0.084580
  12       625       841  1.346   48.70   47.20   +3.08    0.077920
  16      1089      1369  1.257   75.34   75.42   -0.11    0.069180
  24      2401      2809  1.170  152.61  152.39   +0.14    0.063560
  32      4225      4761  1.127  256.44  256.72   -0.11    0.060695
```

> **`CPU_total(r) = 2.2493 + 0.053449 * (2r+5)^2`** — the four largest radii
> fit to within **0.14%**. The two smallest come in *below* the line by 3.7%
> and 4.7%, so at tiny radii the generator is slightly cheaper than a uniform
> per-generated-chunk cost. Do not quote r=4 as if the model held there.

**So the fitted curve crosses `<=0.075` at radius 12.25.** One unchanged
generator sits above that threshold at every smaller radius and below it at
every larger one, which is the point: the number that gets compared to the
target is a function of the radius it was taken at, so the target yields a
verdict only over the r=32 square it was stated for. Reading it anywhere else
is not a fail — it is not a measurement of the target at all.

At r=32, the radius it *is* stated over, we are at **0.0607, met with 19%
margin**. The generator's own cost is **c = 0.0534 CPU-s per *generated*
chunk** — a different denominator from the target's, so it is not a second,
roomier verdict; the two converge only as the margin ring amortises. The r=7
figure is that same `c` carrying a 1.604x margin ring plus **2.25 CPU-s of
fixed process cost** divided over a 225-chunk *delivered* denominator.
Neither of those two terms is the generator, and both amortise away at a real
working radius.

**What this does NOT license.** It does not turn "the efficiency target is
met" into an unqualified claim. It says the number needs its radius attached,
the way every parity number here carries its window and its twin floor. The
honest form is "0.0607 CPU-s per delivered chunk at r=32, 0.0534 per chunk
generated" — and the wall target, which is the binding one, is untouched by
any of this.

### 3. The figures in this file were stale by about a tenth — ATTRIBUTED (measured 2026-08-09, written up 2026-08-17)

```text
                    published    measured at 9b4afd4    drift
  r=7  CPU-s/chunk   0.082364          0.091481         +11.1%
  r=32 CPU-s/chunk   0.055041          0.060695         +10.3%
```

Near-uniform across the curve, which is what a per-chunk cost increase looks
like rather than a fixed-cost one. **And this run is the more favourable
configuration**: the published r=7 came from the four-flavour harness, which
this file records as charging rustmc ~7% extra for running last after forty
minutes of JVM load, while this was rustmc alone. So the drift is at least
this large.

**Attributed by `ci/bench-attrib-run.sh`** (branch `bench-regression-attrib`),
Threadripper 1920X, seed 20260722, three centres, primary metric
INSTRUCTIONS RETIRED at `RUSTMC_WORKERS=1` — contention-immune, unlike
CPU-s, which matters here because **every one of these runs shared the box**:
7-13 competing processes recorded per run (the harness's own `contend=`
column), 2026-08-09 20:18-21:23. This is the prior sweep's data, written up
now; it was not re-taken this session because a merge gate holds the corpus
lock as this is written.

```text
                                r=7 instructions/chunk           r=16
  chambers alone (C->D)   +7.03% [+3.02%,+11.19%] n=6    +4.08% [+2.51%,+5.67%] n=6
  whole span    (A->B)    +7.15% [+3.13%,+11.32%] n=6         (not run at r=16)
```

`C->D` is exactly the two trial-chamber commits (`5b0d5aed` -> `d2f7c6e5`,
the jigsaw walk and the r=16 write-up) and nothing else. `A->B` is
`ed0a29ec` -> `9b4afd4e`, the full published-to-remeasured span — i.e. it is
the same span as the `measured at 9b4afd4` column above. **The two ratios are
the same number inside noise: the intervals overlap almost completely.** The
beardifier, `FlowingFluid` post-processing, `nether_fossil`, the column
evaluator and the multiface/lake/disk work are all also inside that span and
none of them move it beyond what the chambers alone already account for — so
**the trial chambers are the whole of the stale-tenth drift**, not one
contributor among several. `ci/bench-pair-summary.py`'s `cpu12`
(12-worker, corpus-locked) arm corroborates on `C->D`: +7.06% [+4.69%,
+9.48%] n=12; its `A->B` run did not finish (queue was jumped by a merge
gate mid-run, contend up to 13) so only the `ins1` arm is quoted for that
pair.

**It is a per-chunk placement cost, not the price of a chamber landing.**
Two things predicted in `docs/WORKLIST.md` before the sweep ran and both
came out the way that rules out "chamber generation" as the mechanism: an
r=7 square expects only 0.31-0.94 chamber starts, so a generation cost would
be lumpy (one centre large, two near 1.0) — instead all three centres moved
together, 1.04-1.12x. And a fixed per-process cost (loading the 692-piece
template pool once) would dilute toward zero as the square grows — instead
+4.08% survives at r=16, a 4.8x larger square. What is left is a per-chunk
jigsaw/placement scan paid on every chunk whether or not a chamber starts
there — an optimisation target, not the price of coverage.

**Not attributed by this: the file is now stale by more than a tenth.** The
same instructions-retired metric against today's tip at measurement time
(`35b3bde5`) reads +22.44% [+16.03%,+29.19%] n=12 (cpu12) / +22.22%
[+11.49%,+33.97%] n=6 (ins1) against the published baseline — roughly double
the chambers-only figure, meaning **another ~14% landed on top of the
chambers cost between `9b4afd4e` and `35b3bde5` and nobody has attributed
that increment.** That, not this, is the open item now; see
`docs/WORKLIST.md`'s performance track.

**Caveat that must travel with every number above:** the spread *between the
three centres* is 7-17% at every radius. That is terrain, not noise. It does
not weaken the shape fit — the same three centres are used at every radius,
so the centre effect is common to all of them — but a three-square mean is
not repeatable to four significant figures and should not be quoted as if it
were.

Raw log: `~/rustmc-perf-biomeid-logs/baseline-servers.log` on the box.
Attribution sweep logs: `/home/sayer/grit/logs/bench-attrib-*.log` on the box.

**Registered in the ledger 2026-08-18**, four entries with their windows —
`bench-attrib-chambers-r7-20260809`, `-r16-`, `-span-r7-` and `-tip-r7-` — and
`ci/bench-attrib-20260809.txt`, which is generated from the logs' PRIMARY
blocks. That file exists because the same logs carry both metrics and the
secondary CPU-s block reads **+18.11%** where the primary reads +7.03%: citing
the log rather than the block is a live way to quote the contaminated half. The
`+22.44%` row is registered as the open item, because an open item with no
figure is one nobody can close.

**And this section is why `ci/figures.toml` now requires a commit in
`provenance`.** The attribution could run at all only because arm A was written
down here as `ed0a29ec`; the ledger recorded the published r=7 table's
provenance as the bare date `2026-08-03`, which identifies no tree. Both ends
now name the same sha.

## Boot time and resident memory, measured for the first time (2026-08-05) — 15.0x on boot, and the memory result goes AGAINST us

> **Read the memory sections before quoting a memory number from here.**
> The 6.3x resident-memory advantage in the first table is the memory of a
> server **nobody has joined**. With one player in the world it reversed:
> 1,077 MB against vanilla's 835 MB at view-distance 4, and **2,957 MB
> against 1,097 MB — 2.70x worse — at the view distance both servers
> actually ship.** `docs/PRODUCT.md` commits to a 1 GB hardware floor for
> ten players; we were at three times it with one, on superflat.
>
> **FIXED 2026-08-05, same day, and the diagnosis in this section was
> partly wrong.** Every memory table below now carries an `after` row. At
> **view-distance 10 and tile `--radius 2`** the figure went from 2,957 MB to
> **52 MB** with a player's view loaded and **32 MB** at rest (the 90-second
> mark, `ci/bench-rss-retire.sh`; `docs/WORKLIST.md`'s summary of the same run
> says 30 MB and one of the two is a transcription slip — the table below is
> the instrument's own output and is the figure of record), with the same
> 441 chunks delivered. The before figures reproduce exactly — 1,077 and
> 2,957 to the megabyte, alternated against the fix — so the comparison is
> like-for-like and the baseline is not a straw man.
>
> **That pair is NOT the shipped configuration, and calling it one was a
> window mismatch. Corrected 2026-08-07.** This box previously read "the
> shipped configuration went from 2,957 MB to 52 MB". View-distance 10 is
> shipped; **tile `--radius` 2 is not** — `status::WORLD_RADIUS` became **10**
> on 2026-08-06. The before/after pair is pinned at radius 2 on purpose, so
> that a before/after does not also change the tile size, and the boxed note
> under *THE RESULT THAT REVERSES THE MEMORY HEADLINE* already says so. The
> shipped-configuration numbers are **141 MB** (one player, vd=10,
> `WORLD_RADIUS`=10, superflat, n=3) and **218 MB** (`ci/t2-memory.sh`, ten
> co-located players, vd=10, superflat, against a 700 MB cgroup ceiling).
> **Quote 141 or 218 for what this server costs; 52 is the like-for-like
> number for what the fix did.**
>
> The section below attributes the cost to `crate::tiles` never retiring a
> tile. That is real and it is fixed, but **it was the smaller term.** The
> larger one was that `World::generate` called
> `worldgen_assembly::block_props()`, which re-reads and re-parses a
> **15.8 MB JSON file, once per tile**, on the tile's own spawned thread —
> where glibc never returns the parse arena to the kernel. Twenty-five tiles
> at view-distance 10 meant twenty-five of those. See *What the leak
> diagnosis got right and what it got wrong* below.

**Neither of these had ever been measured.** Every claim this project has
made about being fast rests on the tables further down this file, and all of
them time *chunk generation*, through `worldgen-dump`, with no server
running. `ci/bench-servers.sh` never boots `rustmc-server` at all. So the
second pillar's two most quotable numbers — how long it takes to come up and
how much memory it sits in — were assumptions.

Instrument: `ci/bench-boot.py`, driven by `ci/bench-boot-run.sh`, whole sweep
in `ci/bench-boot-sweep.sh`. Raw per-run records are JSON, one object per
boot; `ci/bench-boot-report.py` reduces them and does no measuring, so the
reduction can be checked against the evidence rather than trusted.

### The headline, superflat, n=12 each

Superflat first because it takes the world out of the number: every column
is identical, generation is nearly free, and what is left is the cost of
*starting a server*. Threadripper 1920X, 24 threads, 62 GB, seed 0,
view-distance 4 on both sides, vanilla at `-Xmx1G` with no `-Xms`,
rustmc at its shipped `--radius 2`. Twelve boots each, **alternated**
rustmc/vanilla/rustmc/vanilla rather than run in blocks.

```text
  flavour   t_join median   IQR    min    max   t_tcp median   RSS at join   RSS IQR
  vanilla        9.47 s    0.18   9.35  10.08      7.63 s        831.7 MB     82.7
  rustmc         0.63 s    0.01   0.61   0.64      0.46 s        132.0 MB      4.4
                 15.0x                                            6.3x (*)
```

`(*)` **This column does not survive a player joining** — see the reversal
section below. It is the resident set of a server that has generated one
25-chunk tile and served nobody, and it is quoted here only because it is
the number the boot run produces. Do not lift it out of this file on its own.

**The box was not quiet and the table says so.** 1-minute load average
before each run ranged 1.13–4.93; another agent's `ci/t2-gameplay.sh` — a
vanilla JVM plus an `oracle-runner` — was on the machine for part of the
sweep, and the radius sweep below saw load spike to 10.65. That is the
reason for alternating: a block design charges whatever else was running
entirely to whichever flavour ran during it. Alternating cannot make a busy
box idle, it makes the *comparison* survive one, which is a smaller claim.
The spreads are the evidence that it worked — rustmc's boot IQR is 10 ms on
a box under contention.

### Why there are two marks and not one

A single "time to accepting connections" would have been wrong on one side
or the other, and which side depends on how you measure it:

* **vanilla opens its listening socket before the world can be served.** Its
  TCP port accepts at 7.63 s but a client cannot complete a login until
  9.47 s;
* **rustmc generates its spawn tile before it calls `listen()`** (see
  `main.rs` — the comment there is explicit that this is deliberate, so a
  join cannot fail on a 20-second client read timeout). Timing it to first
  accept charges it for work vanilla has not begun.

So both marks are taken from one `t0`, and `t_join` — `oracle-runner`
completing the login→play transition and receiving a spawn position, the
same protocol event on both servers — is the honest headline. `t_tcp` is
printed beside it so the gap is visible instead of hidden.

### Vanilla's own boot timer is not its boot time, and it is off by 18x

Vanilla's log says `Done (0.522s)!` — median over twelve runs — and this is
the number that gets quoted. Measured from process start, the same boot
takes **9.47 s**. The log is not lying; its timer starts very late:

```text
  process start                                    0.000 s
  first timestamped line in vanilla's own log      5.520 s   <- JVM, bundler,
                                                                class loading,
                                                                registries
  TCP socket accepts                               7.634 s
  "Preparing spawn area" completes ("Time elapsed") 0.380 s of that
  "Done (0.522s)!" printed                        ~7.7   s
  a real client is in the world                    9.467 s
```

**5.52 seconds elapse before the server writes its first log line at all**,
and `Done (Xs)` measures none of it. Anyone comparing a remembered "vanilla
boots in half a second" against a measured rustmc figure is comparing two
unrelated quantities. This is why `t_first_log_line_s` is recorded on every
vanilla run: the gap is a measurement, not an inference.

The same instrument, pointed at rustmc, finds nothing equivalent to hide —
its own `spawn tile: 25 chunks (81 with the margin) in 0.5s` line accounts
for 0.45 s of a 0.46 s time-to-socket, so **fixed startup overhead is about
10 ms and essentially all of its boot is world generation.** The server's
line prints to one decimal place, which is the limit on that attribution.

### What the two servers are not doing equally, stated rather than smoothed

* **Vanilla delivers more chunks on join.** Within a 3-second probe after
  login, vanilla sent 117 chunks in 10 batches; rustmc sent 81 in 2. 81 is
  exactly `(2*4+1)^2`, the view distance; vanilla ships roughly a ring
  beyond it. So vanilla's join does about 1.4x the chunk work, and none of
  that is inside `t_join`, which ends at the login→play transition on both.
  A "time to a full view distance" metric would be a different measurement
  and this harness does not take it — listed as unmeasured below.
* **rustmc pre-generates 25 chunks, vanilla ~380 ms worth of spawn area.**
  Both are each server's own shipped default rather than a matched
  workload. The matched-workload version is the radius sweep below.
* **`-Xmx1G` and no `-Xms` is a choice that favours vanilla's RSS column.**
  `-Xms4G` would have made it report the flag instead of the need. A JVM's
  resident set is substantially a function of how much heap it was told it
  may have, and rustmc has no equivalent knob — which cuts both ways and is
  why the growth measurement below exists.

### On the noise world the lead is 4.2x, not 15x — and that is the honest operator number

Same instrument, same box, `--level normal`, **seed 0 on both sides**, n=8
each, alternated. Seed 0 is not a free choice: `rustmc-server run` calls
`Tiles::prepare(0, radius)` and has **no seed flag at all**, so its world is
always seed 0 and vanilla has to be pinned to match or the two are
generating different worlds.

```text
  flavour   t_join median   IQR    min    max   t_tcp   RSS at join   own spawn prep
  vanilla       14.02 s    0.24  13.83  14.24   7.61 s     941.1 MB      2,415 ms
  rustmc         3.32 s    0.07   3.24   3.38   3.12 s     159.5 MB      3,100 ms
                  4.2x                                        5.9x
```

Load 1.72–10.65 across the sweep, which is the noisiest window in this
section; the IQRs stayed at 0.07 and 0.24 s anyway.

**The advantage is a boot-overhead advantage, and terrain eats most of it.**
Superflat isolates startup and shows 15.0x. Put a real world under it and
the constant that vanilla pays — 5.5 s before it logs, 7.6 s to a socket —
stops being the dominant term for either side, and the ratio falls by more
than three times. Quoting 15x as "rustmc's boot time" would be quoting the
most favourable substrate we measured.

**And the sub-number underneath it is the one to worry about**: vanilla
prepared its spawn area in 2,415 ms; rustmc took 3,100 ms to produce 25
chunks (81 including the decoration margin). Our spawn tile is *slower in
wall-clock than vanilla's spawn preparation* even though rustmc wins the
overall boot. The two are not the same workload — see the radius sweep for
the matched one — but nothing here supports a claim that we generate a
spawn area faster than vanilla does.

### Cold page cache costs 0.02 s and 0.19 s — a null, recorded so nobody re-runs it

`sync` then `echo 3 > /proc/sys/vm/drop_caches` before **every** boot on both
sides, n=5 each, alternated, superflat. `drop_caches_ok` is asserted in all
11 records, so a run that silently stayed warm cannot be reported as cold.

```text
  flavour   warm t_join   cold t_join   delta   cold RSS at join
  vanilla      9.47 s        9.66 s     +0.19      904.6 MB
  rustmc       0.63 s        0.65 s     +0.02      127.0 MB
```

**Boot time here is not I/O.** It is class loading, registry construction and
world generation — all CPU. This kills the most obvious explanation for the
gap ("the JVM reads more off disk") and it means "cold vs warm" needs saying
carefully in this project: the meaningful cold/warm distinction on the JVM
side is *JIT*, which is per-process and cannot be warmed across a restart,
not the page cache. Every boot of a JVM is a cold JIT by construction, and
that is a real and permanent cost of the architecture rather than a
measurement artefact.

Caveat on transfer: this box has 62 GB of RAM and the artefacts are small.
On a memory-starved host with a slow disk the null may not hold.

### CPU-seconds, which is the column wall clock hides — 9.1x to boot, 36x at idle

`docs/AGENT-BRIEF.md` requires CPU beside wall for perf work, because wall
alone on a 24-thread box says more about thread count than about cost.
`utime + stime` from `/proc/<pid>/stat`, superflat, n=3 rustmc / n=2 vanilla:

```text
  flavour   CPU-s to boot and accept one player   CPU-s per 180 s of idle
  vanilla            57.3   (median)                    3.61  (2.0% of a core)
  rustmc              6.27  (median)                    0.10  (0.06% of a core)
                      9.1x less                         36x less
```

**The boot figure is a real like-for-like win**: 6.27 CPU-seconds against
57.3 for the same observable outcome, one player standing in a superflat
world. Note it also means rustmc's 0.63 s wall boot is spending about 9.5
cores — the speed is bought with parallelism as well as efficiency, and on a
1-vCPU box (the stated hardware floor) that wall figure will not hold.
**Nothing here was measured on one core, and the floor is specified in
one.** Listed as unmeasured.

**The idle figure is NOT like-for-like and must not be quoted as efficiency.**
Vanilla is running a full game loop over a live world; rustmc is advancing a
counter. 36x less idle CPU is a restatement of the tick gap, in the units of
the thing we do not yet do. It will shrink as mechanics land.

### THE RESULT THAT REVERSES THE MEMORY HEADLINE: one player costs us more than vanilla

> **The 1.29x gap in the table below is the `(before)` arm and is fixed.**
> Read the whole table: the `(after)` row two lines down is the same
> alternated measurement, same box, same workload, and it is 21x LESS than
> vanilla, not more. The mechanism (a per-tile JSON parse, not the chunk
> canvases) is under "The mechanism, from our own source" and "What the leak
> diagnosis got right and what it got wrong" below. The shipped default
> (radius 10, not the radius 2 pinned here) is **141 MB one player / 218 MB
> ten players**, both still well under vanilla's 1,097-1,144 MB for the same
> job — see the boxed note under "The cost is per TILE, not per chunk". If
> you came here from a brief calling this an open gap: it is not, as of
> 2026-08-05, and the open unmeasured question is players in DIFFERENT
> places / who move — see "What else is unmeasured" — not this number.

**132 MB against 832 MB is the memory of an empty server.** It is taken at
the login→play transition, before a single chunk has been streamed to
anybody. Let one client actually load its surroundings and the ordering
swaps. Superflat, view-distance 4, one probe client in the world for 3
seconds, then 180 seconds of idle; **n=3 each**, load 2.49–4.02:

```text
  flavour          RSS at join      after one client loads a view   growth over 180 s
  vanilla           822 MB (765-932)     835 MB (830-940)              +0.6 MB
  rustmc (before)   127 MB (127-128)   1,077 MB (1,077-1,077)          -0.1 MB
                                          1.29x MORE THAN VANILLA
  rustmc (after)     46 MB (46-46)        40 MB (40-40)      31 MB after 90 s idle
                                          21x LESS THAN VANILLA
```

Medians with the full range beside them, because vanilla's spread is wide
and rustmc's is not: vanilla's three readings differ by 110 MB (GC timing —
it is running against a 1 GB ceiling and collects when it chooses), while
rustmc returned **1,077 MB three times out of three**. A number that
reproduces to the megabyte is a structural quantity, not noise — and the
`after` row reproduces the same way, 40 MB three times out of three.

**We used 1.29x more resident memory than vanilla as soon as one player
existed**, on the cheapest world that can be generated. Neither server was
drifting afterwards — the 180-second idle moved rustmc by −0.1 MB and
vanilla by +0.6 MB — so it was a **step, not a leak**: paid the moment a
player looked at the world, and then held.

**The `after` row was taken by `ci/bench-boot.py` in two checkouts driven
alternately**, one at `d198028` and one carrying the fix, so whatever else
the box was doing is charged to both arms. `probe_chunks` is 81 on both, so
the client received the same world in both; a fall with a smaller chunk
count would be a server that had stopped working rather than one that had
got cheaper. The after row falls further at rest — 40 MB to 31 MB — because
tiles now retire, which is the row above it that never moved.

> **Both rows are at `--radius 2`, and that is no longer the default.** It is
> pinned on both arms because a before/after that also changed the tile size
> would be measuring two things at once. The default became 10 in the same
> branch, and a bigger spawn tile is more resident memory, so **the shipped
> configuration is not the 40 MB in this table**. Measured separately,
> superflat, vd=10, n=3: **141 MB with one player at radius 10** against
> 50 MB at radius 2 — and against vanilla's 1,097 MB for the same job.
> `ci/t2-memory.sh` measures the shipped configuration with ten players at
> **218 MB**. Quote 141 or 218 for "what this server costs"; 40 is the
> like-for-like number for "what the fix did".

### The mechanism, from our own source, and it is deliberate

Not an inference. `crates/rustmc-server/src/tiles.rs` holds finished tiles as
`RwLock<BTreeMap<(i32, i32), &'static World>>` and its own comment says why:

> The `World`s are leaked on purpose. They live for the process either way —
> **nothing retires a tile yet** — and leaking is what lets a reference out
> of the lock […]

So resident memory is a function of **how much world any player has ever
caused to exist**, and it never comes back down. The server's own log for
one of these runs shows it happening — one stationary client, view-distance
4, spawn tile already generated:

```text
  bench joined at (8.0, -60.0, 8.0), 25 chunks
  generating tile (-1, -1) — 25 chunks
  generating tile (0, -1) — 25 chunks
  generating tile (1, -1) — 25 chunks
  generating tile (-1, 0) — 25 chunks
  generating tile (1, 0) — 25 chunks
  generating tile (-1, 1) — 25 chunks
  generating tile (0, 1) — 25 chunks
  … and (1, 1)
```

**Eight tiles beyond the spawn tile: 225 chunks generated and permanently
retained so that one player could stand still.** The arithmetic across the
step is ~950 MB for those 200 extra chunks, about **4.7 MB per chunk held
for the lifetime of the process** — the same order as the "≈3 MB per chunk"
already recorded in this file for the r=32 generator run, and for the same
structural reason.

A second per-player cost sits beside it, also from the source rather than
guessed: `serve_connection` (`status.rs`) calls `ItemIds::load`,
`PlacementTable::load` and the shape/use/`setPlacedBy` loads **inside the
per-connection thread**, not behind a `OnceLock`. The server prints those
table sizes once per joining client, which is how it was noticed — a
12,716-pair shape table over 32,366 states, parsed again for every player.

### What the leak diagnosis got right and what it got wrong (2026-08-05, later the same day)

The section above is a reading of our own source, and rule 9 says a reading
of code moves nothing on its own. It moved a suspect, and the suspect was
only partly guilty. Retiring tiles was implemented — it was the brief — and
**it accounts for about 9 MB of a 1,037 MB fall at view-distance 4.** The
attribution matters more than the win does, because the same reasoning will
be applied to the next memory number.

Three things were wrong with the arithmetic above:

* **"4.7 MB per chunk held for the lifetime of the process" is not what a
  chunk costs.** A superflat `ChunkBlocks` allocates `vec![0; 384 * 256]` of
  `u16` — 192 KB — through `alloc_zeroed`, which for an allocation that size
  is fresh `mmap` pages. Superflat writes four layers, about 2 KB of it. The
  other 190 KB is **never touched and therefore never resident**: it is not
  in `VmRSS` and never was. The 225 retained chunks could not have cost
  950 MB, because on superflat they cost single-digit MB in total.
* **The real per-tile cost was a JSON parse.** `World::generate` and
  `World::generate_flat` both called `worldgen_assembly::block_props()`,
  which does `std::fs::read_to_string` + `serde_json::from_str` on
  `data/vanilla/block_props.json` — **15.8 MB** — and it is called *per
  tile*, not once. Nine tiles at view-distance 4 is nine parses; twenty-five
  at view-distance 10 is twenty-five. A `serde_json::Value` tree over
  15.8 MB of text peaks at many times that, and each of those parses happens
  on the tile's **own spawned thread**, so it lands in a per-thread glibc
  arena that is never trimmed on any schedule of its own.
* **Freed is not returned, and `VmRSS` only answers the second question.**
  That is why the fix has three parts and not one, and why the third part is
  a `malloc_trim(0)` after every generation rather than only after a
  retirement.

So the sequence that produced 2,957 MB was: twenty-five tile threads, each
parsing 15.8 MB of JSON into a temporary tree, each freeing it to an arena
the process then kept. The tiles themselves — the thing the leak comment was
about — were a small fraction of it *on superflat*. **On noise terrain every
column is written, so retained canvases are real and retirement is the term
that matters**; see the radius curve, which is taken on `--level normal` for
exactly this reason.

**The lesson worth keeping is the one the arithmetic would have caught.**
The published figure implied 4.7 MB per superflat chunk. A superflat chunk
contains four distinct block states. Nothing about it can cost 4.7 MB, and
noticing that before writing "leaked on purpose" into the finding would have
pointed at the parse a day earlier. **A per-unit cost that is absurd on its
face is a measurement of something other than the unit.**

The fix, in `crates/rustmc-server/src/{tiles,world}.rs`:

* `WorldTables` — `biome_ids`, `StateIds` and `BlockPropsTable` — is loaded
  **once**, in `Tiles::build`, and shared by every tile behind an `Arc`. It
  is a parse of a fixed file, so this has no behavioural term at all;
* `ready: BTreeMap<(i32, i32), Arc<World>>` in place of `&'static World`. An
  `Arc` leaves the lock as freely as a leaked reference — which is what the
  leak was for — and it also counts, so a tile is dropped when the map stops
  naming it and the last thread serving out of it has finished;
* `Tiles::retain_around`, swept once a second from the tick thread: a tile
  unwanted by every connected player's view for `RETIRE_AFTER_SECS` = 30 is
  retired. `BlockEdits` moved behind an `Arc` held by `Tiles` rather than by
  `World`, so a retired tile keeps what a player built on it and gets it
  back when it regenerates;
* `return_freed_memory()` — `malloc_trim(0)` — after every generation and
  after any sweep that retired something.

### The block entities were left behind, and that was a second leak of the same shape (2026-08-06)

The list above returns a retired tile's **canvas**. It does not touch
`crate::blockentity::BlockEntities`, which is a process-wide map with no
eviction of any kind — so a chest stayed resident for the life of the
process however far away its tile was. It is worse than a placement leak,
because `World::load_from` inserts **every** block entity in every saved
chunk of every tile it publishes: the store grew at the rate a player could
walk across a saved world, and never shrank.

`BlockEntities::evict_chunks`, called from `retain_around` **after** the
tile has been written to disk. `ci/bench-rss-blockentities.sh`, 5 runs per
arm, arms alternated, one process per reading, Threadripper at load
0.72-0.75, 50,000 chests of four filled slots in one radius-2 tile:

```text
  arm    n    held    left  rss_before_mb  rss_after_mb  freed_mb  spread_mb
  on     5   50000       0           57.4           5.6      51.8        0.2
  off    5   50000   50000           57.4          57.1       0.3        0.1
```

**The `off` arm is the control and it is the reason the number means
anything.** Both arms fill the same tile with the same chests and retire it;
they differ only in whether the world has a directory to save into, which is
the exact condition the eviction is gated on. RSS falls at a retirement for
reasons that have nothing to do with block entities — starting with the
canvas the list above was already returning — and a one-armed measurement
could not tell those apart from this one. Same fill, same retirement,
nothing evicted: 0.3 MB.

`held`/`left` are the store's own count either side of the sweep, so the
memory reading is corroborated by a deterministic one rather than resting on
`VmRSS` alone. That is the same discipline `probe_chunks` enforces in
`ci/bench-rss-retire.sh`: a reading with no liveness evidence beside it
cannot be told apart from a run that allocated nothing.

**Eviction is refused when there is nowhere to save**, which is the same
choice `crate::edits`' overlay already makes. A server started without
`--world` keeps its block entities for the life of the process, because the
alternative is losing a player's chest to a memory optimisation.

### The cost is per TILE, not per chunk — and the shipped default is the worst setting

The radius sweep was meant to produce an RSS-against-chunks curve. It
produced something more useful. Noise world, seed 0, view-distance 4, one
probe client, n=3 per radius:

```text
  rustmc --radius   tile size    tiles a vd=4 player forces   RSS after the client
        2            25 chunks             9                   1,144 MB  (n=3)
        5           121 chunks             1                     217 MB  (n=3)
        8           289 chunks             1                     269 MB  (n=3)
       12           625 chunks             1                     339 MB  (n=1)
```

**625 chunks held in one tile cost 339 MB. 225 chunks held across nine tiles
cost 1,144 MB.** 2.8x more world, in 3.4x less memory. Once the tile is big
enough to contain the player's view the curve is nearly flat in chunks —
217, 269, 339 MB across a 5x range in chunk count, about 0.24 MB per
additional chunk — and the whole of the r=2 figure is nine copies of a large
per-tile fixed cost.

**Same player, same view distance, same world, 5.3x less memory.** At
`--radius 5` one tile is 11x11 chunks, which already contains the 9x9 a
view-distance-4 player needs, so no further tile is ever generated. At
`--radius 2` the same player straddles nine 5x5 tiles and every one of them
is allocated and then never released.

So resident memory here is dominated by **how many tiles exist**, not by how
many chunks they hold — a 121-chunk tile costs 217 MB while nine 25-chunk
tiles holding 225 chunks between them cost 1,144 MB. There is a large fixed
cost per tile and the shipped default multiplies it by nine.

`status::WORLD_RADIUS` is **2**, and `main.rs` already says the trade is the
operator's because "the cost is quadratic" — that comment is about startup
time and tile-crossing pauses, and the memory term appears not to have been
priced. Three readings of 217 MB with zero variance say this is structural
rather than noise.

**This is a measurement, not a change.** Nothing was tuned. What a bigger
tile costs is in the same sweep and is not free — the spawn tile is
generated before the socket opens, so it is paid as boot time, and it is
also the "a tile a player walks into takes seconds to generate; until then
it is void" pause the server's own caveat line warns about:

```text
  rustmc --radius   chunks   spawn tile generated in   chunks/s
        2             25            3.30 s               7.6
        5            121            5.80 s              20.9
        8            289            9.00 s              32.1
       12            625           15.50 s              40.3
```

Noise world, seed 0, from the server's own `spawn tile: … in Ns` line, n=3
per radius except r=12 (n=1). **Throughput improves with tile size** — 7.6
chunks/s at the shipped radius against 40.3 at r=12 — so the small tile is
losing on generation efficiency as well as on memory; it buys only a shorter
boot. The trade is real and it is quantified here, which is what was missing.

#### Re-taken across the whole range (2026-08-06), and the memory half of this does not survive

`ci/bench-radius-sweep.sh`, 2..12 rather than four points, noise world, seed
0, **view-distance 10** — the shipped one, where the sweep above used
`ci/t2-gameplay.sh`'s 4 — one probe client, n=3 per radius, with tile
generation capped at `tiles::MAX_CONCURRENT_GENERATIONS` = 2.

```text
  r   side  delivers  generates  wasted   boot     chunks/s  view at 6 s   RSS
  2     5      25         81      69%    3.33 s      8.1      75 / 441    350 MB
  3     7      49        121      60%    4.07 s     12.9     147 / 441    392 MB
  4     9      81        169      52%    4.84 s     17.6      81 / 441    142 MB
  5    11     121        225      46%    5.82 s     22.0     121 / 441    464 MB
  6    13     169        289      41%    6.83 s     26.4     169 / 441    465 MB
  8    17     289        441      34%    9.33 s     32.8     289 / 441    527 MB
 10    21     441        625      29%   12.38 s     38.0     441 / 441    201 MB
```

**Boot and throughput are monotone. The RSS column is not, and it should not
be read as a curve at all** — 350, 392, 142, 464, 465, 527, 201 does not
describe a property of the radius. It describes how many tile generations
happened to be in flight when the sample was taken, which depends on how far
the probe got. `r=4` reads lowest and delivered only its own spawn tile;
`r=8` reads highest and delivered 289. **A column that is not monotone in the
thing it is plotted against is usually measuring the sampling instant**, and
this one is.

That kills the memory argument in the section above. The "large fixed cost
per tile" was the 15.8 MB `block_props.json` parse, and the nine-tiles-at-
once figure was twenty-five unbounded generation threads; with the parse
shared and the concurrency capped, **resident memory is no longer a function
of the tile radius**. What the cap is worth, at the shipped radius, measured
the same way: **1,991 MB uncapped against 350 MB capped**, and the capped run
delivered *more* chunks in the same six seconds (75 against 25) because
twenty-five pipelines competing for the same cores finish nothing.

So the radius trade is throughput against boot, and the discard is what
decides it. **A tile of side *n* generates `(n + 4)^2` chunks through every
stage to deliver `n^2`.** At radius 2 that is 81 for 25 — 69% of all
generation work thrown away, on every tile, for the life of the server. At
radius 10 it is 29%. Boot is paid once per start; the discard is paid per
tile forever, so `status::WORLD_RADIUS` **is now 10** (was 2).

Ten is also the first radius whose tile contains a view-distance-10 player's
whole view, so a player who joins and stands still forces no background
generation — the `view at 6 s` column is measured, and every smaller radius
leaves the view incomplete.

**The price, stated rather than buried:** noise boot goes 3.33 s → 12.38 s
against vanilla's 14.02 s, so *that* comparison narrows from 4.2x to 1.13x.
On superflat it goes 0.64 s → 1.30 s against vanilla's 9.47 s, so the
headline substrate stays at 7.3x. The superflat numbers by radius, same
harness, vd=10, n=3:

```text
  r    boot     RSS     view at 6 s
  2    0.64 s    50 MB    279 / 441
  6    0.85 s    85 MB    441 / 441
 10    1.30 s   141 MB    441 / 441
```

`ci/t2-gameplay.sh` passes `--radius 2` explicitly, so the corpus substrate
does not move with this default.

#### One measurement from this sweep is disqualified, and it is ours

The same harness drove **vanilla** to each square with `forceload` and timed
it. Those numbers are **not published** and should not be quoted from the
raw file. Rule 7: the vanilla column came out at 138.6 chunks/s at r=12
against rustmc's 40.3, which contradicts this file's own four-server table
(rustmc 1.76x faster on wall and 2.30x cheaper on CPU than vanilla at r=7),
so the measurement is the suspect. Two things convict it:

* **the same work took wildly different times** — 25 chunks measured at
  25.03 s, 10.52 s and 10.26 s across three runs;
* **289 and 625 chunks both came out at ~4.50 s**, which is the
  quiet-detection floor rather than a generation time. The probe declares
  completion after the process stays under an eighth of a core for 3 s, and
  vanilla generates forceloaded chunks asynchronously across ticks with
  lulls in it, so the detector fires in a gap.

`ci/bench-servers.sh` already solves this properly and its table stands as
the throughput comparison. **rustmc's own figures above are safe** because
they come from the server's internal timer around a synchronous operation,
not from an external idle detector — but they are therefore not directly
comparable with a vanilla number taken any other way, and no such
comparison is made here.

### One player costs 945 MB and none of it comes back when they leave

`ci/bench-rss-players.sh`, superflat, view-distance 4, `--radius 2`, clients
held in the world for 12 seconds, RSS read with them connected and again
three seconds after the last one disconnects. n=3 each:

```text
  flavour   players   before   with them   after they leave   never returned
  vanilla      1      752 MB     776 MB        822 MB               —
  rustmc       1      132 MB   1,077 MB      1,077 MB            945 MB
  vanilla      5      732 MB     756 MB        886 MB               —
  rustmc       5      127 MB   1,111 MB      1,111 MB            984 MB
```

(n=3 throughout.)

**A single player costs this server 945 MB, and disconnecting returns
exactly none of it.** Vanilla's same player costs 24 MB while connected, and
vanilla's total is bounded by `-Xmx` whatever happens next; ours is bounded
by nothing.

**This is the row the fix was aimed at, and it is the one to re-read.** The
reading three seconds after a disconnect is now the *wrong* reading to take:
tiles retire after `tiles::RETIRE_AFTER_SECS` = 30 seconds of nobody being
able to see them, so a three-second window observes a server that has
correctly not yet done anything. `ci/bench-rss-retire.sh` exists for this and
idles 90 seconds. Measured with it, superflat, `--radius 2`, one probe
client, n=3:

```text
  view distance   with the player   ~3 s after they leave   90 s after they leave
        4              40 MB               40 MB                    31 MB
       10              52 MB               52 MB                    32 MB
```

Against the same instrument on `d198028`: 1,077 MB and 2,957 MB at all three
marks, unchanged to the megabyte, because nothing retired. **The answer to
"does resident memory come back when a player leaves" was no and is now
yes** — and the shape of the fall is the grace period, not noise. A harness
that idled for 15 seconds would have reported no change for both servers,
which is why the script that takes this reading is a new one.

**At the shipped tile radius the demonstration needs a player who leaves the
spawn tile, and here is why.** Radius 10 was chosen so that one tile contains
a view-distance-10 view — and that one tile is the *spawn* tile, which is
pinned and never retired (three `Blocks` reads and `Tiles::surface_at` answer
through it). So a player who joins and stands still causes nothing to be
retired, correctly, and RSS holds flat at 141 MB. Forcing tiles beyond it
with `--view-distance 16`, superflat, shipped radius, n=3:

```text
  at join   with the client's 1,089-chunk view   100 s after they leave
   127 MB               179 MB                          111 MB
```

and the server's own log for those runs says what happened, which is the
liveness proof this table would otherwise be missing:

```text
  generating tile (-1, 1) — 441 chunks
  generating tile (0, 1) — 441 chunks
  generating tile (1, 1) — 441 chunks
  retired 8 tile(s) nothing had looked at for 30s; 1 resident
```

Eight tiles generated, eight retired, one resident — the pinned spawn tile —
and the resident set ends **below where it started**, because the trim also
returns what generating them cost. `probe_chunks` is 1,089 on all three runs,
which is the whole of a 33x33 view, so nothing was saved by failing to serve
the player.

**The five-player row separates the two costs, and the answer is that only
one of them matters.** Going from one player to five adds just 34 MB — about
**8.5 MB per additional connection**, which is the per-connection table
parsing — because all five clients stand on the same spawn and therefore
share the same tiles. The 945 MB is paid once, by whoever arrives first.

**That is the best case and must not be read as the general one.** Five
players standing on one another is the cheapest arrangement possible. Five
players in five different places would each pull in their own tiles, and
nothing is ever released, so the resident set tracks **the union of
everywhere anyone has been** for the life of the process. That scenario was
not measured — see the unmeasured list — but it is the one a real server
lives in, and the mechanism guarantees it is worse than this table.

At five co-located players: **1,111 MB against vanilla's 886 MB, 1.25x.**

The 1,077 MB figure has now reproduced in **three independent experiments**
— the 180-second idle runs, the r=2 radius runs and this one — to the
megabyte each time. It is a structural constant of the current design, not a
reading.

### At the SHIPPED view distance the gap is 2.7x, and one player costs us 2.96 GB

Every table above pins view-distance 4, because that is what
`ci/t2-gameplay.sh` uses. **Neither server ships that.**
`status::DEFAULT_VIEW_DISTANCE` is 10 and so is vanilla's
`server.properties` default. `ci/bench-rss-viewdistance.sh`, superflat,
`--radius 2`, one client, n=3 each:

```text
  view distance   flavour            RSS at join   after one client   chunks delivered
        4         vanilla                866 MB          879 MB             117
        4         rustmc (before)        127 MB        1,077 MB              81
        4         rustmc (after)          46 MB           40 MB              81
       10         vanilla                784 MB        1,097 MB             473
       10         rustmc (before)        135 MB        2,957 MB             441
                                                        2.70x MORE
       10         rustmc (after)          46 MB           52 MB             441
                                                        21x LESS
```

**At the configuration both servers actually ship, one player cost this
server 2.96 GB against vanilla's 1.10 GB. It now costs 52 MB.** The chunk
counts confirm the sides are doing comparable work — 441 against 473,
vanilla still shipping its extra ring — and, more to the point here, the
`before` and `after` rows deliver **the same 441**. A memory number that
improves while the chunk count falls is a server that stopped working, and
that is the check this table was already carrying for a different reason.

Going from view-distance 4 to 10 cost rustmc 1,880 MB and vanilla 218 MB. It
now costs rustmc 12 MB, because what grew with tile count was never the
tiles: it was one 15.8 MB JSON parse per tile, on a thread whose arena was
never returned. The sub-linearity noted here — 5.4x the chunks for 2.7x the
memory — was tile count, and it was tile count for the wrong reason.

**This measurement was nearly wrong, and the way it failed is worth keeping.**
The first attempt at this table asked vanilla for view-distance 10 and
measured a server still running at 4, because `--view-distance` was threaded
into rustmc's command line but written as a literal `4` into vanilla's
`server.properties`. It would have compared our 441-chunk client against a
117-chunk one and reported a much larger gap. It was caught by
`probe_chunks` — recorded on every run for exactly this reason — and the
vanilla half was discarded and re-run. The `vd10` (broken) and `vd10fixed`
rows are both still in `rss-vd.jsonl`; only `vd10fixed` is quoted.
**A setting that must match on both servers has to be threaded through, not
written twice**, and a run that does not assert its own configuration took
effect cannot tell you it did not.

### This breaks the project's own stated hardware floor, with one player

`docs/PRODUCT.md` item 8, a definition-of-complete item:

> **Hardware floor: 10-player survival runs WELL on 1 vCPU / 1GB VPS.** This
> is the hosting-density headline and a standing G5 bench config (not just a
> marketing line — **CI-enforced like parity**).

Measured here: **1,077 MB resident with one player, on superflat, at
view-distance 4** — the cheapest world, a quarter of the shipped view
distance, and a tenth of the stated player count. The floor is already
exceeded before the scenario the floor describes begins.

**At the shipped view distance of 10 it is 2,957 MB — very nearly three
times the floor, for one player, on superflat.** Vanilla does the same job
in 1,097 MB, which is also over the floor but by 10% rather than by 196%,
and vanilla can be told to stay under it with `-Xmx` while this server
cannot be told anything.

**And it is not CI-enforced.** No gate in `ci/` sets a memory limit, a
`ulimit`, or a cgroup; nothing measures server RSS at all before this
commit. The parenthetical claiming parity-grade enforcement describes
something that does not exist. That is a documentation defect independent of
the measurement, and it is the more dangerous of the two, because a floor
nobody checks reads as a floor somebody checks.

#### It is enforced now, at 700 MB, and the gate proves it can fail

`ci/t2-memory.sh`. Ten concurrent clients at view-distance 10 on superflat,
run **inside a cgroup v2 scope with `MemoryMax` set to the ceiling**, so a
breach is a kernel kill and not a number the script chose to compare. The
measured peak `VmRSS` is printed and compared as well, but that is the
diagnostic; the cgroup is the enforcement.

The ceiling is **700 MB** and it sits under the 1 GB floor rather than at
it, because the floor is the whole VPS — kernel, sshd and everything else
share it — and a server allowed to use all of it is a server something else
OOM-kills. The same scenario measures far below that today, and the gap is
deliberate: every mechanic still to land adds resident state, and a gate
that has to be edited on every feature is a gate that gets edited without
being read. What it catches is a return to the old behaviour, and the figure
it replaces was 1,077 MB for **one** player, so any regression of that shape
breaches it on the first run.

**The gate self-tests, and that is not decoration.** After the real run it
re-runs the same scenario under an impossible 32 MB and **fails if that
passes**. `docs/AGENT-BRIEF.md` lists twenty checks in this project that
could not fail; a memory guard is an easy twenty-first, because a working
limit and an unapplied one look identical from outside — the happy path
prints the same line either way. Without the self-test, a `systemd-run` that
silently ignored `MemoryMax` would have printed GREEN forever.

Two limitations are in the script's own header rather than left to be
assumed. **All ten players stand on spawn**, which is the cheapest
ten-player arrangement there is, because tiles are shared — the five-player
row above measured the fifth player costing 8.5 MB for exactly this reason.
And **superflat is the cheapest world**: its canvases are mostly untouched
zero pages that never enter `VmRSS`. Passing this gate is necessary for the
stated floor and is not sufficient for it. `BENCH_LEVEL=normal` runs it on
terrain and is the number to watch as worldgen features land.

Same caveat as everywhere else in this section, stated rather than buried:
`docs/PRODUCT.md` also calls startup and RAM "vanity metrics" and points the
speed claim at load instead. That is a defensible product position. It is
not a reason for the numbers to be unknown, and the one that was unknown
turns out to be the one that breaks a stated floor.

### What this measurement cannot tell you

* **Retained live data or allocator retention?** `VmRSS` cannot separate
  "still referenced" from "freed but not returned to the OS". The source
  says the tiles are genuinely retained, which makes the first the leading
  explanation, but a heap profile is what would settle it and none was
  taken. For an operator on a 1 GB box the distinction does not change the
  outcome.
* **Superflat is the floor, not the ceiling.** Noise terrain holds more per
  chunk; the noise-world boot already sits 27 MB higher at join.
* **Vanilla's number is bounded and ours is not.** Vanilla was capped at
  `-Xmx1G` and will collect rather than exceed it. rustmc has no equivalent
  knob, so its figure is what it wanted rather than what it was allowed.

### The two controls: the boot gap is not GC, and our reload control came back VACUOUS

Rule 7 says a measurement that clears every candidate is the suspect, so the
boot table got two controls. `ci/bench-boot-controls.sh`.

**Heap.** Vanilla was measured at `-Xmx1G` sitting at 832 MB resident —
inside 20% of its own ceiling, which is where a JVM collects constantly. If
the 9.47 s were partly GC against a starved heap it would be an artefact of
a flag we chose. Re-run at `-Xmx4G`, n=5:

```text
  vanilla heap    t_join median   range          RSS at join
  -Xmx1G  (n=12)      9.47 s      9.35-10.08       831.7 MB
  -Xmx4G  (n=5)      10.04 s      9.67-10.31       854.6 MB
```

**Four times the heap made it slightly slower, not faster, and moved
resident memory by 23 MB.** So the boot figure is not heap starvation and
the memory figure is not an artefact of the cap either — at this workload
vanilla genuinely wants ~830–855 MB and would want it whatever it was
allowed. Both headline comparisons survive their own control.

**Reload — and this one did not survive.** Every boot in the tables above
generates a fresh world; an operator restarting a server loads one. The
control asked both servers to keep their world and boot again:

```text
  flavour   t_join (reload)   region files on disk   autosave chunks written
  vanilla       9.43 s              12                        n/a
  rustmc        0.65 s               0                          0
```

**rustmc wrote nothing, so its "reload" was a regeneration every time and
the number is vacuous.** It was given `--world`, `--autosave-seconds 2` and
six seconds of life — the autosave timer fired and had nothing to write,
because nothing was dirty. **This server does not persist a world it merely
generated; only changes are saved, so a restart regenerates from scratch.**
That is reported as a *property found*, not as a fast reload. It is exactly
the failure `docs/AGENT-BRIEF.md` warns about — a negative result is only
evidence if the mechanism was live — and it was caught only because
`world_region_files` and `autosave_writes` are asserted in every record
rather than assumed. Without that assertion this table would have published
"rustmc restarts 14x faster than vanilla".

The vanilla half is real and is worth its own line: **9.43 s to reload
against 9.47 s to generate — reloading an existing superflat world saves
vanilla nothing**, because its boot cost is JVM startup and class loading,
not terrain. Its resident memory was lower on the reload, 668.8 MB against
831.7 MB, so loading chunks is cheaper for it than making them.

### Everything that was pinned, so the run is repeatable

Reproduce, in this order — they are sequential because two of them running
at once measure each other:

```text
  ci/bench-boot-sweep.sh          boot, cold, idle and the radius sweep
  ci/bench-boot-controls.sh       the heap and reload controls
  ci/bench-rss-players.sh         1 and 5 concurrent players
  ci/bench-rss-viewdistance.sh    view-distance 4 against 10
  ci/bench-rss-retire.sh          does memory come back? 90 s idle, both ends
  ci/bench-radius-sweep.sh        the radius curve, 2..12, at the shipped vd
  ci/t2-memory.sh                 the gate: 10 players inside a cgroup ceiling
  ci/bench-boot-report.py *.jsonl the reduction, which does no measuring
```

`ci/bench-boot.py` is the instrument; every setting lives in
`ci/bench-boot-run.sh` in one block rather than being passed at call sites.

```text
  box                Threadripper 1920X, 24 threads, 62 GB, Linux
  tree               ~/rustmc-bench-boot0805, branch off origin/main d198028
  version            26.2 (crates/rustmc-proto GAME_VERSION)
  seed               0          forced: rustmc-server calls
                                Tiles::prepare(0, radius) and has NO seed flag
  view-distance      4          both sides, explicitly, never defaulted,
                                EXCEPT the view-distance sweep which also
                                runs 10, the shipped default on both
  simulation-distance =vd       vanilla only; rustmc has no equivalent
  rustmc --radius    2          what status::WORLD_RADIUS was when this
                                section was taken; it became 10 on
                                2026-08-06, so EVERY figure above that says
                                "the shipped default" means radius 2 and no
                                longer means the shipped default. The
                                before/after rows and the radius curve pass
                                --radius explicitly for this reason.
                                Radius sweep: 2, 5, 8, 12 (original),
                                2..12 (2026-08-06 re-take)
  vanilla heap       -Xmx1G, no -Xms
  vanilla props      online-mode=false, spawn-protection=0,
                     enforce-secure-profile=false, sync-chunk-writes=false,
                     max-tick-time=-1
  flat substrate     ci/t2-gameplay.sh's: gamemode=creative,
                     generate-structures=false, spawn-monsters=false,
                     difficulty=peaceful, spawn-radius=0
  normal substrate   structures ON (vanilla's own default)
  n                  12 flat, 8 normal, 5 cold, 3 idle, 3 per radius,
                     5 heap control, 4-5 reload control, 3 per player arm,
                     3 per view-distance arm
  order              alternated rustmc/vanilla, never blocked
  boot mark          oracle-runner login->play complete, from process start
  RSS                /proc/<pid>/status VmRSS, sampled every 100 ms
  load               /proc/loadavg[0] recorded before AND after every run
```

**What was NOT pinned, and could not be:** the rest of the box. Other agents
were building and gating throughout. Load before each run is in every record
and in every table above; the widest window was 1.72–10.65 during the noise
sweep. Any figure here can be re-filtered on that field by a later reader.

### Tick timing under load: NOT MEASURED, and it would be dishonest to publish one

This is the fourth thing a speed claim is normally made about and it is
**deliberately absent**. It is not a gap in the harness, it is a gap in the
server, and a number here would be a lie by construction.

`crates/rustmc-server/src/tick.rs` says so in its own module docs: *"This is
the clock and nothing else. It advances a counter at 20 Hz and tells
connected clients what it reads. No block updates, no entities, no scheduled
ticks, no random ticks."* `docs/SERVER.md` agrees and refines it — five
blocks (all corals) schedule a tick, block updates go out at a tick
boundary, and players are tracked — but **no entity moves under its own
power, no mob AI runs, and there are no random ticks at all**, because
`Block.tick` and the mob path both take a `ServerLevel`, which does not
exist here yet.

So an MSPT or TPS comparison against vanilla would time **an empty loop
against a full one** and would return whatever ratio we cared to make it
return. `ci/bench-servers.sh` reached the same conclusion in its header and
declined for the same reason; that judgement is re-confirmed here rather
than inherited, because the tick module has changed since it was written.

The honest position is: **rustmc has no measurable tick load because it does
almost no per-tick work, and that is a missing feature, not a performance
result.** When mob AI, random ticks and entity movement land, this is the
first number to take, and it is the one most likely to move against us —
every one of those is work we currently do not do and vanilla does.

What *could* be driven reproducibly, and is listed as future work rather
than claimed: concurrent joins. `oracle-runner` connects real clients that
both servers accept, so N simultaneous logins is a load test neither server
can no-op its way through. It was not run for this pass.

### What else is unmeasured, listed rather than estimated

* **Time to a full view distance.** `t_join` ends at the login→play
  transition. Chunk delivery continues after it — 117 chunks on vanilla, 81
  on rustmc, within a 3-second probe — and no mark was taken for "the client
  has everything". Vanilla ships about 1.4x the chunks, so this would move
  against the headline, by an unknown amount. **Partly closed 2026-08-06**:
  the radius curve records chunks delivered inside a 6-second probe at every
  radius, which is a lower bound on this and is what chose the new default —
  radius 10 delivers all 441, radius 2 delivered 75. It is still not a
  *time*, and no vanilla arm was taken, so the comparison is unmeasured.
* **Players in DIFFERENT places, and players who move.** Still the most
  important unmeasured quantity here, and the fix changes its shape rather
  than removing it: tiles now retire, so resident memory tracks the union of
  everywhere anyone is *currently* looking plus a 30-second tail, instead of
  everywhere anyone has ever been. A spread-out population is still strictly
  worse than the co-located one every table above measures, and it is still
  the normal case for a real server. `ci/t2-memory.sh` gates the co-located
  case and says in its own header that passing it is necessary and not
  sufficient. **The old wording follows, because the reasoning is still the
  reasoning for the unspread case.** Every player in
  these tables stands on the spawn point and shares one set of tiles. Since
  tiles were never retired, resident memory tracked the union of everywhere
  anyone has walked, so a spread-out or moving population is strictly worse
  and possibly by a large factor. This is the single most important
  unmeasured quantity in this section and it is the normal case for a real
  server.
* **Anything on one core.** `docs/PRODUCT.md`'s hardware floor specifies
  1 vCPU. Every measurement here was taken on 24 threads, and rustmc's boot
  spends about 9.5 cores, so its wall-clock boot figure will not transfer to
  the box the floor describes. Nothing here says what either server does
  under `taskset -c 0`. **`ci/bench-boot.py --taskset-cpus 0` exists now
  (2026-08-17) and pins both flavours identically before launch** — this is
  the capability, not the number: it has not been run, because writing it
  needed no CPU and running it does, and the box was under a merge chain
  when it was added. `rec["taskset_cpus"]` records the pin (or `null`) in
  every output row, so a run through this flag is distinguishable from every
  existing 24-thread figure without re-reading a log's provenance by hand.
* **RSS on a memory-constrained host.** 62 GB of RAM, and the cold-cache
  null above may not transfer to a small box.
* **Any server other than vanilla.** Out of scope for this pass by
  instruction and unavailable to it. The Paper and Folia columns elsewhere
  in this file are from earlier runs and were not re-taken.
* **Boot with a large existing world.** The reload control uses a 25-chunk
  superflat world. A restart onto a world with thousands of saved chunks is
  a different measurement and probably the one an operator cares most about.
* **Long-run memory.** The steady-state figure below is 180 seconds. A leak
  with an hour-scale time constant would not show up in it.

## The four-server table at r=7 (2026-08-03) — 2.30x vanilla, 1.39x Paper and 1.59x Folia per CPU-second

Measured on **one machine, back to back, twice**: `ci/bench-servers.sh` on an
idle Threadripper 1920X, seed 20260722, `BENCH_RADIUS=7`, `-Xms4G -Xmx4G` on
all three JVMs, three squares of 225 chunks — centres (64,64), (256,-192),
(-320,128). Run 1 is `~/rustmc-private/bench-2026-08-03.log`, run 2 is
`bench2-2026-08-03.log`. Pooled over all **six** squares per flavour:

```text
  flavour   wall s   CPU s   CPU-s/chunk   peak cores   avg cores   vs rustmc CPU
  vanilla    11.52   42.67     0.189644       13.30        3.70         2.30x
  folia      19.32   29.38     0.130592        9.73        1.52         1.59x
  paper       4.61   25.78     0.114578        9.30        5.59         1.39x
  rustmc      6.55   18.53     0.082364       11.99        2.83         1.00x
```

**A finished chunk costs less CPU here than on any of the three**: 2.30x
against vanilla, 1.59x against Folia, 1.39x against Paper.

> **The efficiency target is NOT comparable to this row, and the sentence
> that used to stand here compared them anyway. Corrected 2026-08-07.**
>
> It read: *"The efficiency target (`<=0.075` CPU-s/chunk) is still not met
> at 0.0824."* The target `<=0.075` is stated over **r=32** — it was derived
> against Paper's 0.0614 on a 4,225-chunk square (see *What the goal actually
> requires, in numbers*), and it was last **MET at r=32, at 0.055041**. The
> 0.082364 above is **r=7**. The paragraph immediately following declares
> those two radii not comparable, and the verdict was drawn across exactly
> that gap.
>
> `ci/bench-servers.sh` divides by the square it was *asked* for, `(2r+1)²`,
> while paying for a `DECORATION_MARGIN`=2 ring, `(2r+5)²` — an overhead of
> **1.604x at r=7 against 1.127x at r=32**. A 225-chunk square pays fixed
> costs a 4,225-chunk one amortises, and CPU-s/chunk is exactly the quantity
> that inflates.
>
> **Honest state of the efficiency target at r=7: not comparable, therefore
> unresolved — not failing.** The target has a verdict only at r=32. Do not
> restate 0.0824 against 0.075 anywhere.

**Wall is still the binding target and Paper still wins it**: 6.55 s against
4.61, so Paper is 1.42x faster on the clock. That is an improvement on the
r=32 table's 1.78x, and for the same reason as above it should not be read as
one. Against the other two, rustmc's wall is 1.76x faster than vanilla and
2.95x faster than Folia.

### Which columns reproduce between the two runs, and which do not

Pooling is only legitimate for the columns that reproduce, so here are both
runs side by side. CPU-seconds, the headline column, is stable to within 1%
on three flavours of four:

```text
              CPU s          wall s          peak cores
  flavour   run1   run2    run1    run2     run1    run2
  vanilla  42.88  42.46   11.35   11.69    13.16   13.43
  paper    25.69  25.87    4.54    4.68     9.19    9.41
  folia    28.27  30.49   13.55   25.09     9.90    9.56
  rustmc   18.65  18.41    6.67    6.43     (n/a)  11.99
```

* **CPU-seconds reproduce.** vanilla -1.0%, paper +0.7%, rustmc -1.3%. The
  three CPU ratios move by less than the rounding: 2.30/1.38/1.52 in run 1
  against 2.31/1.41/1.66 in run 2, 2.30/1.39/1.59 pooled. Quote the pooled
  ones.
* **Folia's wall does not reproduce and its number is not usable.** 13.55 s
  against 25.09 s, an 85% swing, on the same box, same seed, same squares,
  minutes apart; its CPU also moved +7.9%, the largest of the four. This is
  consistent with the caveat already recorded below — Folia rejected one of
  the nine quiescing commands, so it is the only flavour still live-ticking
  the forceloaded square while being measured, and how much ticking it does
  is not controlled. **The "2.95x faster than Folia on wall" figure above is
  therefore the weakest number in the table** and should be quoted as
  "faster, magnitude not established" rather than as 2.95x. Its CPU column
  is the more defensible half and is still only an upper bound against it.
* **rustmc's wall is the tightest of the four** — 6.67 against 6.43, 3.6% —
  which is what a fixed worker pool with no GC and no JIT warm-up should
  look like.

### The `peak_cores` column for rustmc was a hardcoded literal — now measured

`ci/bench-servers.sh` printed `1.00` in rustmc's peak-cores column for run 1
and every run before it. Not a measurement: the string `1.00` in a `printf`,
left over from when the comment at the top of that file — "rustmc's pipeline
is single-threaded, there is no `rayon` in the tree" — was true. Parallel
decoration landed 2026-07-29 and neither the literal nor the comment moved.

Run 1's own numbers already refuted it: 17.35 CPU-seconds inside 6.1 wall
seconds is 2.84 cores on average, so the peak could not be one. Any claim of
the form "one core against their nine to thirteen" rested on that literal and
was withdrawn before it was ever published.

Both are fixed, and run 2 is the first run to measure the column.
`watch_cores` samples `/proc/<pid>/stat` fields 14 and 15 over a 0.2 s wall
interval — the same two fields, the same interval, the same way as
`ci/bench-probe.py` already samples the JVMs, so the four numbers are
directly comparable.

**rustmc peaks at 11.99 cores, the highest of the four after vanilla**, on a
box with 12 physical cores and 24 hardware threads. That is not a coincidence
and it is the check that the sampler is measuring real work:
`pipeline::worker_threads()` defaults to **one thread per physical core**
(not per hardware thread — see its doc comment), so the pool is exactly 12
wide here and 11.99 means it is saturated at peak, not partially used.

The gap between peak (11.99) and average (2.83) is the finding this column
was worth: the pipeline is **bursty**, not broadly parallel. It saturates 12
cores during the chunk-local stages and spends the rest of its wall clock
close to serial. Every flavour shows the same shape to a lesser degree
(vanilla 13.30 peak against 3.70 average, Paper 9.30 against 5.59), so it is
not unique to us — but ours is the widest ratio, 4.2x, against Paper's 1.7x.

### The next perf question, named: our serial fraction, not our parallel one

**Paper wins wall while peaking on fewer cores than we do.** That single
sentence is the useful result of this table, and it is worth more than any
of the raw numbers in it, because it says where to look.

```text
             wall s   CPU s   peak cores   avg cores   peak/avg
  paper        4.61   25.78         9.30        5.59       1.7x
  rustmc       6.55   18.53        11.99        2.83       4.2x
```

We do **less total work** than Paper — 18.53 CPU-seconds against 25.78, a
1.39x advantage per finished chunk — and we still lose the clock by 1.42x.
Both facts at once can only mean the work is badly *spread*: Paper keeps
about 5.6 cores busy on average, we keep 2.8, and we do it while proving we
can reach 12 when a stage allows it.

So the wall gap is **not** a throughput problem and there is nothing to be
won by making the parallel stages faster — they already run at pool width.
It is Amdahl's law with a large serial term, and the question that follows
is the one to take into the next perf turn:

> **Which parts of the pipeline are still serial, and what fraction of wall
> time do they hold?**

`RUSTMC_STAGE_CPU=1` and `ci/stage-scale.sh` already answer the second half
per stage; what has never been measured is stage *occupancy* — wall time
during which fewer than `worker_threads()` threads are runnable. A 4.2x
peak-to-average ratio bounds it: if every stage ran at 12 wide we would see
an average near 12, so roughly three quarters of our wall clock is spent
somewhere that is not using the pool.

Two candidates worth pricing before anything is changed, named rather than
assumed: the stages `Canvas::chunk_writers` does **not** mark chunk-local
(so they cannot be divided at all), and the sequential region write, which
`ci/bench-servers.sh` already notes the JVMs defer to shutdown and so do not
pay inside their measured window. The second is a measurement artefact
rather than a real gap and must be subtracted before the first is believed.

**Do not chase this against the r=32 table.** A 225-chunk square pays fixed
costs a 4,225-chunk one amortises, the two radii are not comparable, and
the serial fraction is exactly the quantity that fixed costs inflate.

### Parity at r=7, and why the number is an upper bound

```text
  vanilla vs rustmc   225 chunks   22,118,400 blocks   5,822 mismatches  (run 1)
                                                       5,893 mismatches  (run 2)
                                                       99.9737% / 99.9734%
                      biomes 345,600 cells             0 mismatches (both runs)
```

**The two runs disagree by 71 blocks, and all 71 are vanilla's.** rustmc is
deterministic at a fixed seed and generated the same 225 chunks both times,
so every position that moved between the two diffs moved on vanilla's side.
That makes 71 a *lower* bound on how far the two vanilla worlds are apart
here — a position where both vanilla runs differ from us but also from each
other cancels in the count and is invisible to this subtraction. It is an
independent corroboration of the first limitation below, taken on this exact
window rather than a comparable one.

Four limitations, none of which the number survives without:

* **It is not twin-masked, so it is an upper bound on our defect and not a
  measurement of it.** ORACLE rule 6: a residual counts against us only if
  two independently generated vanilla worlds of that seed agree on it. They
  frequently do not — on a comparable r7 window at this seed (156 chunks)
  two vanilla runs differ from *each other* by **3,006 blocks**, about 19 per
  chunk, and the twin mask drops **4,105** positions on overworld window 1.
  At 19 blocks per chunk, 225 chunks would expect roughly 4,300 from
  vanilla's own nondeterminism alone. What is left after that is not
  computed here and must not be inferred from these two numbers.
* **The differ compared the overlap, not the square**: `a=1445 b=225`.
  Vanilla dumped 1,445 chunks — its forceloaded square plus everything its
  margin pulled in — and rustmc 225. The 225 common chunks were compared and
  the other 1,220 were not looked at.
* **One seed, three centres, r=7, worldgen only.** No ticking, no entities,
  no players, no saves. This is the only workload the four servers currently
  share.
* **Folia rejected one of the nine quiescing commands** (vanilla, Paper and
  rustmc rejected none), so its CPU column carries some live ticking of the
  forceloaded square and its parity column carries whatever that ticking
  mutated. Both are over-estimates against it and neither can be removed
  from outside.

Zero biome mismatches, and at this radius Paper and Folia have zero too —
unlike the r=32 window, where each differs from vanilla on 95 biome cells. A
225-chunk square is not enough to separate them on biomes and should not be
cited as if it were.

Raw logs, outside the repo: `~/rustmc-private/bench-2026-08-03.log` (run 1)
and `~/rustmc-private/bench2-2026-08-03.log` (run 2).

## The surface stage: the same question, asked again, paid 5.3% not 10x (2026-07-29)

The biomes stage closed at 10.3x by asking *how many times do we evaluate
something vanilla evaluates once*. Asked of the surface stage the same question
found a real 2x — and it was worth **5.3% of the stage**, not a factor. Both
halves of that are the finding; the second half is the one that redirects the
next turn.

### What vanilla does, from the bytecode

`SurfaceRules$Context` (javap) carries two `IdentityHashMap`s keyed on the
noise's `ResourceKey` — `noiseSamplers2d` and `noiseSamplers3d` — and
`getNoiseSampler(key, is3d)` `computeIfAbsent`s a `DoubleSupplier` into one of
them. `Context$1` (the 2D one) recomputes only when `lastUpdateXZ` moves;
`Context$2` (3D) only when `lastUpdateY` moves. `NoiseThresholdConditionSource`
fetches that supplier **once, when the rule tree is instantiated for the
chunk**, so every rule site naming one noise shares one memo and vanilla
performs no map lookup at all during the walk.

The overworld rule (`noise_settings/overworld.json`) has **42
`noise_threshold` sites over 8 distinct noises** — `minecraft:surface` alone
appears at 19 of them, and 7 of the 8 are 2D. Our oracle sampled a fresh
multi-octave `NormalNoise` at every site, with a `BTreeMap<String, _>` lookup
in front of it, and said so in a comment: "Vanilla memoises each sampler on
the context's XZ or Y counter. Not reproduced."

### The counter, and what it said

`RUSTMC_SURFACE_NOISE=1` counts asks against computes. Seed 20260722, r=8 at
(64,64), one worker:

```text
  surface named noise:     asked 13,803/chunk   computed 6,924/chunk   1.99x
  surface getSurfaceDepth: asked    256/chunk   computed   256/chunk   1.00x
```

So the resampling was real but it was **2x, not 61x**. After the memo the
count is vanilla's by construction: 2D collapses to at most one evaluation per
column per noise (7 x 256 = 1,792) and the 3D `sulfur_cave_gradient` to one
per y-update, which is what `Context$2` does.

### The cost it bought — Threadripper 1920X, r=32, centre (64,64)

`RUSTMC_STAGE_CPU=1`, against a pristine `git archive` control of the parent
commit `48d1efb`, on an idle box:

```text
                    1 worker            12 workers
  stage         before   after       before   after
  surface        48.90   46.30        54.04   51.18   CPU-s
  TOTAL         193.03  193.80       211.90  213.16   CPU-s
```

**Surface -5.3% on CPU at both widths.** The pipeline total moved +0.4%, which
is inside the box's 1.5-2.5% run-to-run spread and should be read as flat.
Byte-identical: 0 of 9 region files differ at `RUSTMC_WORKERS=1` and 0 of 9 at
12, both `cmp`d against the control generated sequentially.

The arithmetic is self-consistent, which is the check that the counter was
measuring the right thing: 6,879 evaluations removed per chunk for 2.60 CPU-s
over 4,761 chunks is **79 ns per `NormalNoise::get_value`** — the right order
for a 3-octave sample. It also bounds the whole line item: the 6,924 that
remain cost ~0.55 ms of the stage's 9.7 ms per chunk, so **named-noise
sampling was only ~11% of this stage and is now ~6%.**

### Two nulls, recorded so nobody re-runs them

1. **`hole` never resamples in practice.** `HoleCondition` in vanilla is a
   `LazyXZCondition` reading the `surfaceDepth` field `updateXZ` already
   filled; ours recomputed a `NormalNoise` *and* seeded a positional random at
   every `hole` site, of which the rule has three. It looked like a clean N-vs-1.
   The counter says `getSurfaceDepth` is asked **exactly 256 times a chunk**,
   which is `build_chunk`'s own once-per-column call and nothing else — no
   `hole` condition was reached anywhere on the r=8 square. The memo is kept
   because it is correct and free, but it is worth zero. *A reading of the code
   named it; a measurement killed it.*
2. **`FlatCaches::none()` in the surface stage is not a marker falling
   through.** Vanilla's `NoiseChunk.preliminarySurfaceLevel` field *is* the
   wrapped router (`mapAll(this::wrapNew)`, javap offsets 388-413) evaluated at
   a `SinglePointContext`, so its `flat_cache` markers are live — but
   `Context.getMinSurfaceLevel` samples the **four corners of a 16-block
   cell**, one cell is one chunk, and our `SurfaceLevels` memo is per worker
   and outlives the chunk. That is ~1 new corner evaluation per chunk.
   Wiring the caches in would be worth ~0.1 s of 193.

### Where the other 89% of the stage is — MEASURED, and it is the biome id

Taken after the change, which is the wrong order and is why the change was
worth 5%: `perf record -F 999 -e cycles`, one worker, r=16 at (64,64), twice —
`RUSTMC_THROUGH=biomes` and `RUSTMC_THROUGH=surface` — and differenced. The
scale factor comes from three symbols that only the noise stage executes
(`ChunkSampler::evaluate` 18.89 -> 12.07%, `compute_substance` 8.13 -> 5.14%,
`NoiseFill::fill_retaining` 3.94 -> 2.39%): the surface run is **1.57x** the
biomes run, so the surface stage is 36% of it. Self time, rescaled to *shares
of the surface stage*:

```text
  biome::manager::quart_for_block                   14.1%   the fuzz itself
  the biome_at / cold / melt closures (2 symbols)   12.3%
  surface::eval::test                               11.2%
  malloc + cfree + two libc string/memory stubs     10.2%
  surface::eval::apply                               8.0%
  SurfaceShared::stored_biome_at                     7.7%
  WorldSurfaceOracle::vertical_gradient              6.4%
  Canvas/ChunkWriter block_at + set_block            ~7%
  named noise (ImprovedNoise + PerlinNoise)          4.7%
  climate Node::search (the perimeter fallback)      0.7%
```

**Roughly 44% of the surface stage is resolving a biome id** — the fuzz, the
closures that wrap it, the `BTreeMap<(i32,i32)>` lookup into the snapshot, and
the `String` allocated and freed for each of the ~28,864 answers a chunk.
Vanilla's `SurfaceRules$Context.getBiome` memoises a `Holder<Biome>` per
y-update, exactly as `SurfaceContext::biome` does — **so this is not a
call-count gap, it is a cost-per-call gap.** Vanilla indexes
`WorldGenRegion`'s chunk array and copies a reference; we walk a B-tree of up
to 4,761 entries and then heap-allocate 25 bytes to compare against string
literals.

That makes the next item a representation change rather than a cache: an
interned biome id (`u16` into the stage's palette) carried through
`SurfaceOracle::biome` and `Condition::Biome`, plus a 3x3 array of the
neighbourhood's containers resolved once per chunk instead of a B-tree lookup
per query. Both are order-preserving by construction — no value moves, only
its type — so the byte-compare is the whole proof.

`vertical_gradient` at 6.4% is the other surprise, and it is nearly all
lookup: the bedrock-floor gradient sits at the top of the overworld rule
sequence and is therefore tested at **every y of every column**, ~34k times a
chunk, and each test resolves its `random_name` through a `BTreeMap<String,
_>`. This commit removed the `String` *allocation* that was on that path but
left two map lookups on the hit path where one would do (`get(..).copied()` —
`AnyPositionalFactory` is `Copy`). Unmeasured, cheap, and the profile says it
is worth ~3% of the stage.

## The four-server table, re-taken after the biomes work (2026-07-29c) — 1.78x Paper on wall, and BELOW Paper on CPU

The row above was stale by the whole of the biomes win. Re-taken with the same
harness on the same box, four flavours back to back in one session:

```text
  BENCH_RADIUS=32 ci/bench-servers.sh
```

Threadripper 1920X (12c/24t), seed 20260722, `-Xms4G -Xmx4G` on all three
JVMs, three squares of 4,225 chunks asked for — centres (64,64), (256,-192),
(-320,128). Means over the three, at commit `48d1efb`:

```text
  flavour   wall s   CPU s   CPU-s/chunk   vs vanilla CPU   vs paper CPU   vs paper wall
  paper      32.97  247.66     0.058618        0.48x            1.00x          1.00x
  folia     119.40  224.78     0.053190        0.43x            0.91x          3.62x
  rustmc     58.83  232.55     0.055041        0.45x            0.94x          1.78x
  vanilla   106.70  518.21     0.122653        1.00x            1.71x          3.24x
```

* **Wall against Paper: 2.12x -> 1.78x.** The wall target (1.5-2x *faster*
  than Paper) is still not met and this is still the binding one.
* **CPU-s/chunk: 0.069129 -> 0.055041, and it is now BELOW Paper's 0.058618.**
  The efficiency target `<=0.075` was already met; what is new is that a
  finished chunk costs less CPU here than on Paper — 0.94x — where the
  previous reading was 1.21x. Against vanilla it is 0.45x.
* Folia's mean wall is dragged by one outlier square, 172.78 s at (256,-192)
  against 90.31 and 95.12 at the other two. Its CPU column is steady.
* Per centre, rustmc: gen 56.8 / 61.9 / 57.8 s, CPU 224.34 / 245.09 / 228.22,
  peak RSS 1.38-1.41 GB.
* Raw rows and log: `oracle-work/bench-archive/results-r32-2026-07-29c.tsv`.
* **The wall column for rustmc is the tool's own `gen` figure**, which is what
  the previous two tables in this file quoted; the harness also prints
  `wall_with_write` (66.08 s mean here) and the two must never be mixed into
  one ratio.
* Block parity over the (64,64) square is unchanged by this run: 715,034
  mismatches of 415,334,400 = 99.827826%, with **zero** biome mismatches,
  still the only one of the three servers with none (Paper and Folia each
  differ from vanilla on 95 biome cells).

### rustmc per stage at r=32, re-taken at `48d1efb`

`RUSTMC_STAGE_CPU=1`, seed 20260722, centre (64,64), over the 4,761-chunk
canvas, on an idle box:

```text
                1 worker                    12 workers
  stage       wall s   cpu s        wall s    cpu s   cores
  noise        73.47   73.47          6.91    82.77   11.98
  biomes        5.51    5.50          4.27     6.40    1.50
  surface      48.91   48.90          8.38    54.04    6.45
  carvers      17.50   17.50         17.65    17.64    1.00
  structures    1.06    1.07          1.08     1.09    1.01
  decoration   46.60   46.59         18.16    49.96    2.75
  TOTAL       193.05  193.03         56.45   211.90    3.75
```

The single-worker total is 193.03 against the 198.34 recorded for `5e72c82`,
which is a 2.7% difference on a box whose run-to-run spread is 1.5-2.5% and
should be read as the same number. **Surface is 48.90 CPU-s, a quarter of the
single-thread pipeline and the largest stage after noise.**

## The biomes stage was evaluating the router 61x more often than vanilla (2026-07-29) — 57.17 -> 5.56 CPU-s at r=32

**CLOSED.** The stage this file recorded at 49% parallel efficiency "for
reasons nobody measured" was not a scaling problem at all. It was doing sixty
times vanilla's arithmetic on one thread, and no amount of thread count would
have found that.

`NoiseBasedChunkGenerator#doCreateBiomes` fills a chunk's biomes through
`NoiseChunk#cachedClimateSampler`, and that method maps **each of the six
climate functions** through `NoiseChunk::wrapNew` before handing them to
`Climate.Sampler` (javap, `NoiseChunk.class`, offsets 389-98). `wrapNew` is
what instantiates a `flat_cache` marker into a filled 5x5 quart table for the
chunk. This port sampled climate through a bare `FunctionContext`, where every
marker falls through and computes, so `continents`, `erosion`, `ridges` and
the terrain `offset` behind `depth` were evaluated **once per cell — 1,536
times a chunk — against vanilla's 25**. `offset` carries the whole terrain
spline, which is why it was the expensive one.

Threadripper 1920X, seed 20260722, centre (64,64), `RUSTMC_STAGE_CPU=1`, one
worker. Three commits, each byte-identical against a pristine `git archive`
control of its own parent at `RUSTMC_WORKERS=1` **and** at 12:

```text
  r=16, 1,369 chunks            biomes cpu-s   pipeline cpu-s
  before                            15.82           69.89
  + the chunk's flat caches           2.44          56.76     6.5x
  + a marker for temperature/veg      1.82          55.38     8.7x
  + the search's pruning shortcut     1.58          56.88    10.0x
```

At r=32, 4,761 chunks on the canvas, against the table below:

```text
                1 worker                    12 workers
  stage       wall s   cpu s        wall s    cpu s   cores
  noise        76.29   76.29          7.08    84.80   11.98
  biomes        5.57    5.56          4.19     6.45    1.54
  surface      50.25   50.26          8.54    54.99    6.44
  carvers      17.84   17.83         17.69    17.69    1.00
  structures    1.07    1.07          1.07     1.07    1.00
  decoration   47.33   47.33         18.12    51.73    2.86
  TOTAL       198.36  198.34         56.69   216.73    3.82
```

* **Biomes: 57.17 -> 5.56 CPU-seconds, 10.3x**, and 2.8% of the single-thread
  pipeline where it was 23%.
* **The whole pipeline's single-thread cost falls 247.41 -> 198.34 CPU-s,
  -19.8%**, and 46.9 ms of CPU per asked chunk where it was 58.6.
* **That moves the wall target.** A perfectly parallel pipeline is now
  198.34/12 = 16.5 s against Paper's 32.44 s, i.e. **1.96x**, where the same
  arithmetic on the old figure gave 1.57x and only just cleared the goal. The
  binding constraint is still single-thread cost, but there is now margin in
  it.
* **The stage is no longer worth parallelising.** 1.54 cores at 12 workers is
  a bad number and it does not matter: 4.19 s of wall for 5.56 CPU-s of work
  means the batch barrier is most of what is left, and the whole stage is 7.4%
  of the twelve-worker wall.

### How the remainder was attributed, and what is left

`RUSTMC_THROUGH=biomes` was added so `perf record` over noise+biomes could be
differenced against a noise-only profile — the stages run in sequence and a
flat profile cannot otherwise separate them. Absolute CPU-seconds, r=16, one
worker:

```text
  symbol                            before   after caches   noise stage alone
  ImprovedNoise::noise_with_fudge    15.90        6.26            5.66
  biome::climate::Node::search        1.08        1.08             -
  ClimateSampler::sample_in              -        0.06             -
  density::flat_cache::FlatCaches::get   -        0.05             -
```

Read the first row across: 15.90 CPU-seconds of noise evaluation over
noise+biomes, of which 5.66 is the noise stage's own — so **10.2 of the
15.82-second biomes stage was `ImprovedNoise`**, and afterwards 0.6 is.
`Node::search` does not move, because nothing about it changed.

**The climate search was 44% of the stage after the first fix and 59% after
the second** — the R-tree is only the hot structure once the router is not.
That is the opposite of the order it was guessed in, and it is why the
profile was taken twice.

The pruning shortcut is exact, not an approximation: every use of a child's
distance is guarded by `min_distance > distance`, the distance is a sum of
squares and therefore monotone, so a partial sum that reaches the bound
settles the comparison. `the_bounded_distance_agrees_with_the_exact_one`
checks the value and the verdict at six bounds around each exact distance,
including *at* it, which is where the strict `>` tie-break lives.

**What is left in the stage, ~1.6 CPU-s at r=16:** roughly 0.9 of search, 0.1
of sampling, and ~0.6 of the `String` clone per cell plus `Canvas::set_biome`
plus the per-chunk `FlatCaches::fill`. The next lever on the search is data
layout — `Node` is an enum with `Vec<Node>` children, so every child visit is
a pointer chase; flattening the tree into contiguous node spaces preserves
the traversal order and the tie-break exactly, because the order is the child
order and nothing else.

### The two null results this did *not* need

* **The resample fallback in the decoration biome lookup was not the
  problem.** It fires 4,188 times on the r=16 square; the biome stage makes
  2.1 million climate queries. It is 0.2% of the stage by count and does not
  appear in a profile at all.
* **The parameter search was not the top of the stage either**, until the
  router work made it so. Profiling first is what stopped a week going into
  an R-tree that was 7% of the cost.

## The four-server table, re-taken after parallel decoration (2026-07-29) — 2.12x Paper on wall, down from 6.19x

The r=32 row in this file was taken before parallel decoration landed and was
explicitly flagged as a floor. Re-taken with the same command, same box, same
session:

```text
  BENCH_RADIUS=32 BENCH_WARMUP_RADIUS=7 BENCH_PROBE_TIMEOUT=1800 ci/bench-servers.sh
```

Threadripper 1920X (12c/24t), JDK 25.0.3, seed 20260722, `-Xms4G -Xmx4G` on
all three JVMs, three squares of 4,225 chunks asked for — centres (64,64),
(256,-192), (-320,128). Means over the three:

```text
  flavour   wall s   CPU s   CPU-s/chunk   vs vanilla CPU   vs paper CPU   vs paper wall
  paper      32.44  241.66     0.057198        0.54x            1.00x          1.00x
  folia     127.00  249.85     0.059136        0.56x            1.03x          3.91x
  rustmc     68.77  292.07     0.069129        0.66x            1.21x          2.12x
  vanilla   104.83  444.86     0.105293        1.00x            1.84x          3.23x
```

Against the 2026-07-29 pre-decoration row (rustmc 199.07 s wall, 283.98 CPU-s,
0.067215):

* **Wall against Paper went from 6.19x to 2.12x** — 2.89x on our own wall for
  the same square, which is more than the 1.30x the r=16 pipeline measured,
  because the r=32 square dilutes the sequential surface perimeter.
* **CPU-s/chunk went the wrong way, 0.0672 -> 0.0691, +2.8%.** The efficiency
  target `<=0.075` is still MET and we still spend less CPU per chunk than
  vanilla (0.66x), but parallel decoration is not free and this is the price.
* Per centre, rustmc: wall 73.7 / 69.1 / 63.5 s, CPU 289.4 / 308.63 / 278.19,
  peak RSS 1.37-1.41 GB.

**The harness's own rustmc row is 7% worse than the same square measured in
isolation** — 68.77 s / 292.07 CPU-s here against 64.13 s / 273.68 CPU-s
measured alone earlier in the session. `ci/bench-servers.sh` runs rustmc last,
after forty minutes of sustained all-core JVM load, and it charges the Anvil
write to the CPU column where the stage instrument does not. Quote the harness
number for cross-server comparisons and the isolated one for our own
before/after; **do not mix them into one ratio.**

### rustmc per stage at r=32, one worker and twelve

`RUSTMC_STAGE_CPU=1`, seed 20260722, centre (64,64), over the 69x69 = 4,761
chunk canvas that delivers the 4,225 asked for. This is the first per-stage
decomposition taken at r=32 rather than r=16:

```text
                1 worker                    12 workers
  stage       wall s   cpu s        wall s    cpu s   cores
  noise        73.56   73.55          6.99    83.68   11.97
  biomes       57.18   57.17          9.90    62.99    6.36
  surface      48.84   48.84          8.50    54.12    6.37
  carvers      17.99   17.99         18.09    18.09    1.00
  structures    1.07    1.06          1.07     1.07    1.00
  decoration   48.80   48.80         19.58    53.73    2.74
  TOTAL       247.43  247.41         64.13   273.68    4.27
```

* **3.86x of 12 on the whole pipeline, for +10.6% CPU.**
* **Carvers are now the largest fully-sequential stage** — 18.09 s of wall at
  1.00 cores, 28% of the twelve-worker wall against 7% of the one-worker wall.
* **Decoration reaches 2.74 cores at r=32**, below the 1.98x speedup measured
  at r=16, because a bigger square has more levels, not fewer.
* Single-thread cost is **58.56 ms of CPU per asked chunk**; biomes, noise and
  decoration are 180 of those 247 CPU-seconds.

### Parity at r=32 after the same change

`region-differ diff`, vanilla first every time, 4,225 chunks /
415,334,400 blocks:

```text
  vanilla vs paper     131,470 blocks   99.968347%   biomes 95/6,489,600
  vanilla vs folia     391,840          99.905658%   biomes 95/6,489,600
  vanilla vs rustmc    722,664          99.826041%   biomes  0/6,489,600
```

Unchanged in character from the pre-decoration run (717,655): parallel
decoration is byte-identical to the sequential control, and the 5k difference
is the harness's own forceload ordering, not our change. rustmc is still the
only one of the three with **zero** biome mismatches, and its worst chunks are
still interior — (40,32) at 13,580, (51,72) at 11,254 — where Paper's and
Folia's are on the square boundary.

### The census says we never place 111 of vanilla's 253 block names

`region-differ census` over the same square, ours against vanilla's:

```text
  vanilla   253 block names
  rustmc    153 block names, 111 of vanilla's absent, 11 of ours vanilla lacks
  smallest set of blocks that must change identity to reconcile: 293,154
```

The absent names are not scattered: `tuff_bricks` (168,190 blocks),
`polished_tuff` (18,432), `chiseled_stone_bricks`, `waxed_copper_block`
(16,601), `waxed_oxidized_copper` (12,694), `grindstone`, `loom`, beds, doors,
`sculk_catalyst`. **That is trial chambers and ancient cities — whole
structures we do not build** — and it is a cleaner statement of the missing
structure work than any block-diff percentage, because a census sees a block
never placed while a diff cannot distinguish it from one placed wrongly.

Note the direction this implies and do not misread it: the census floor
(99.929%) sits **above** the block-for-block number (99.826%). A composition
match is an upper bound on parity, never a measurement of it.

## Decoration runs on every core, and it is byte-identical (2026-07-29)

Decoration was the last big sequential stage: 12.14 s of a 25.76 s wall at
twelve workers, 47% of the pipeline. It now runs on the pool. Threadripper
1920X, seed 20260722, r=16 at (64,64), 1,369 chunks generated,
`ci/stage-scale.sh` at widths 1 and 12:

```text
  stage        12wk wall        12wk cpu-s        speedup
  decoration   12.14 -> 6.25    12.14 -> 15.13    1.02x -> 1.98x
  TOTAL        25.76 -> 19.83   75.61 -> 78.10    2.74x -> 3.52x
```

**1.30x on the whole pipeline for +3.3% CPU-seconds.** The 1-worker path is
untouched and measured the same: 69.81 s wall, 69.81 CPU-s, decoration 12.36 s.

### The scheme is not the 5x5 colouring, and the difference is the point

The NOW block sketched a `(2M+1)x(2M+1)` colouring: decorate all chunks of one
colour together, since chunks five apart cannot touch each other's write
footprints. That is correct about the footprints and it still **changes the
world**. A colouring is *a different decoration order*, and a different order
writes different blocks wherever two chunks' features overlap — measured, not
assumed: two vanilla worlds of one seed generated at different centres differ
by 637 blocks, all of it ore veins. So a coloured pass can never be `cmp`'d
against the sequential control. It can only be argued to be as good.

This keeps the sequential order's **answer** instead. The observation is that
a total order over the chunks says far more than correctness needs. Two chunks
whose footprints cannot overlap have no order between them at all, so only
chunks close enough to touch have to keep their relative order. Give each
chunk a level — one more than the highest level of any *earlier* chunk within
`2 * DECORATE_REACH` of it — and every chunk in a level is more than
`2 * DECORATE_REACH` from every other chunk in it, because if two were closer
the later one would have inherited a higher level. Their `(2R+1)^2`
neighbourhoods are therefore disjoint, so each can be moved out into a canvas
of its own (`Canvas::detach`), decorated, and moved back (`Canvas::merge`).
No unsafe, no aliasing argument: the borrow checker sees one owner per chunk,
and `merge` panics if two groups ever claim one.

Two consequences worth stating separately:

* **Every decoration order parallelises**, including the eight the sweep uses.
  The level assignment is computed from whatever order `decorate_order`
  produced. Nothing in the scheme knows what a raster is.
* **The nine-order gate becomes a reproduction rather than a re-measurement.**
  If the parallel pass reproduces the sequential bytes for an order, every
  parity number ever taken from that order is unchanged, exactly, without
  re-running a differ.

### Output-identical, proved — and then proved eight more times

Control: a pristine `git archive` checkout of the parent commit `fc4469d`,
built separately, generated **sequentially**. `cmp` on all 4 of 4 region files:

* new build at `RUSTMC_WORKERS=1` — identical, 4 of 4;
* new build at 12 workers — identical, 4 of 4;
* `ci/decorate-order-sweep.sh`, all **nine** `RUSTMC_DECORATE_ORDER` values
  (`raster reverse zx zx_rev zx_zrev diag spiral spiral_in spawn`), pristine
  control sequential against the new build at 12 workers — identical, every
  order.

### The reach is not a bound, and the instrument that says so

`DECORATE_REACH` is 2 — `DECORATION_MARGIN`, the measured radius of a chunk's
influence, and one step more conservative than vanilla's own rule, which
refuses a *write* more than one chunk from the centre. This port enforces that
rule too: `Decoration::write` drops a write outside `write_radius`.

**Nothing bounds a read**, and the assumption that features read as far as they
write was tested rather than believed. `RUSTMC_READ_REACH=1` records the
largest Chebyshev chunk distance from the decorating chunk that any canvas
access uses. On the whole-canvas sequential run of this square it is **39** —
wider than the 37-chunk canvas. Some call site inside decoration reads right
across the world and past its edge. **Which one is unknown and is the next
thing to find**; the instrument is in the tree and the question is one run
away.

So the safety of a finite reach rests on evidence, not on a structural
argument, and the evidence was widened until it stopped moving (ORACLE rule
14). At 12 workers, `DECORATE_REACH` of **1, 2, 3, 4, 6 and 8** all produce
byte-identical output to the pristine sequential control — 4 of 4 region files
at every one of them. Reads that far out exist and demonstrably decide
nothing here.

A second instrument backs it: `RUSTMC_OUTSIDE_READS=1` counts every access
naming a chunk the canvas does not hold, which is what a group too small to
cover a feature's reach produces. The whole-canvas run counts 418,142 — all of
them decoration, none before it, i.e. the true edge of the generated area. The
grouped runs count more, and **more as the reach grows**: 429,361 at reach 1,
433,589 at 2, 442,021 at 4, 458,789 at 8, about +4,220 per step. That is the
opposite of the naive model, in which a bigger group can only turn misses into
hits — so the reading is that a group holding more real chunks lets a scanning
read keep going, and the extra reads land further out again. It is not a
number to quote as a safety margin. It is a number that says the reach is
open-ended, which is why the byte comparison across six reach values is the
evidence and this is only the flag that raised the question.

### One real nondeterminism found on the way, and closed

`Biomes::biome_at` falls back to resampling the climate function when a fuzzed
quart cell lands outside the snapshot — `getUncachedNoiseBiome`'s case. It was
assumed rare. `RUSTMC_STAGE_TIMES` says it runs **4,188 times** on the r=16
square, and it went through `ParameterList::find_value`, whose `lastResult`
hint carries from call to call and decides exact ties by the sequence of calls
before it. Under a worker pool that sequence is thread scheduling.

Vanilla has the same property — the hint is a `ThreadLocal` and its worldgen
runs on a pool — but this project's claim is that a seed gives one world, so
the decoration fallback now calls `find_value_stateless`. **Byte-identical
either way** (4 of 4 region files against the pristine control, at 1 worker and
at 12), so the hint decided none of the 4,188; the change is that it cannot
decide the next one differently on a different run.

Note how it was found: not by reading the code — the `RefCell` had been read
several times while converting it to a `Mutex` — but by turning on a counter
that had never been looked at. Rule 9 again.

### What it costs, and what is left

Decoration's CPU rises 12.14 -> 15.13 s, **+22.3%**, which is well past the
"a few points" the NOW block allowed. Two candidates, and the measurement
separates them:

```text
  reach   levels   mean/level   decoration wall   decoration cpu-s
    1       145        9.4           5.39              14.73  (+19.1%)
    2       217        6.3           6.25              15.13  (+22.4%)
    3       361        3.8           7.13              15.86  (+28.3%)
```

CPU rises with the level count at about 436 microseconds per level per worker,
which is what a thread create-and-join costs. **The driver opens a fresh
`std::thread::scope` per level** — 217 levels x 12 = 2,604 spawns at reach 2 —
so most of the tax is the pool being rebuilt at every barrier. The rest is
locality: a worker now walks a 5x5 group of chunks rather than the canvas in
order.

That also caps the speedup. Mean level size at reach 2 is 6.3 chunks, so a
12-worker pool is half idle at every barrier, and 1.98x against a critical-path
bound of 1369/217 = 6.3x is mostly *barrier stalls, not contention*. The fix is
one persistent pool over the whole pass with a ready-queue instead of levels —
a chunk becomes runnable when its last blocking predecessor finishes, no
barrier at all. Same correctness argument, same levels implied, no rebuild.
**That is the next item, and it is worth more than the change it follows.**

Reach 1 is already faster and cheaper than reach 2 (5.39 s against 6.25, CPU
14.73 against 15.13) and byte-identical. It is **not** the default, because the
read probe says neither value is a structural bound and 2 is the number
`DECORATION_MARGIN` was raised to on evidence. Lowering it is a decision for a
turn that can say *why* a read goes 39 chunks out.

## The contention tax was an `Arc` clone per block written (2026-07-29)

**CLOSED.** The +28.2% CPU-second tax that three sessions chased — NUMA,
DRAM capacity, per-worker noise tables, hyperthreading, cross-CCX placement,
all of them nulls or retractions — was one line: `ChunkWriter::update_wg`
opened with `self.wg.clone()`, an `Arc<dyn Fn>` refcount bump, **once per
`set_block` call**, on a single cache line shared by every worker. Two atomic
read-modify-writes per block written. At one worker the line stays in that
core's L1; at twelve it ping-pongs across four L3 slices.

At 12 workers on the r=16 square the tax is now **+7.1% CPU-seconds, and the
cycle count is flat to +0.5%** — the residual is the measured all-core clock
drop and is silicon. `78f8035`.

### The instrument that found it, and why the old one could not

The NOW block required `worldgen-dump scale`'s equivalent for the surface and
biomes stages before any further fix. That is `RUSTMC_STAGE_CPU=1` plus
`ci/stage-scale.sh` (`f80cb4c`), and it found the answer on its first run.

Two things made it work where `RUSTMC_STAGE_TIMES` could not:

* **It is a different switch.** `RUSTMC_STAGE_TIMES` also turns on the surface
  stage's `Cell` probes, which force that stage down the **sequential** path.
  A table read from it is not the parallel pipeline's shape and can never
  answer how a stage scales. This is written down in WORKLIST's housekeeping
  and was still an obstacle, because the only per-stage instrument in the tree
  disabled the thing being measured.
* **It reports CPU seconds, not wall.** `/proc/self/stat`'s `utime`+`stime`
  are the *thread group's* totals, so a worker pool created and joined inside
  a stage still shows up after its threads have exited. No `libc` dependency.

Threadripper 1920X, r=16 at (64,64), seed 20260722, widths 1/2/3/6/12.
**Before** the fix, CPU-seconds against the 1-worker baseline:

```text
  stage        1wk cpu-s   12wk cpu-s     tax    12wk speedup   12wk eff
  noise           20.78       23.58     +13.5%      10.50x         88%
  biomes          15.74       17.35     +10.2%       5.77x         48%
  surface         15.05       29.65     +97.0%       3.36x         28%
  carvers          5.00        4.99      -0.2%       1.00x          8%
  structures       0.38        0.40      +5.3%       0.97x          8%
  decoration      12.03       12.11      +0.7%       0.99x          8%
  TOTAL           68.98       88.08     +27.7%       2.58x         22%
```

**The surface stage is 14.60 of the 19.10 extra CPU-seconds — 76% of the whole
tax** — and the worst-scaling parallel stage by a distance. Noise and biomes
sit at or near the ~8.6-point all-core clock floor and were never the problem.
The sequential stages, as expected, cost the same CPU at any width.

### Extra work, or stalls? — the discriminating run

`perf stat`, `RUSTMC_THROUGH=surface` so the 17 s of carvers and decoration
stop diluting the signal:

```text
             instructions      cycles      IPC    CPU-s    GHz
  1 worker     535.05 G       215.55 G     2.48   54.86   3.929
  12 workers   536.51 G       265.78 G     2.02   72.07   3.688
```

Instructions **+0.27%**, cycles **+23.3%**, IPC 2.48 -> 2.02. Constant work,
23% more cycles to do it: stalls, not recomputation. That kills the reading
that per-worker memos (each surface worker holds its own `SurfaceLevels`) were
recomputing anything — recomputation would show in the instruction count, and
it does not.

### Which symbol — `perf record`, absolute percentages, both widths

```text
  symbol                            1 worker   12 workers
  ImprovedNoise::noise_with_fudge     32.18%      26.30%
  ChunkWriter::set_block               1.33%      20.31%
  DensityFunction::compute             9.79%       7.81%
  PerlinNoise::get_value_with_fudge    9.03%       7.09%
  ChunkSampler::evaluate               8.63%       7.01%
```

Against each run's own cycle total that is **2.9 G cycles -> 54.0 G**, a
delta of +51.1 G against a whole-run delta of +50.2 G. **One symbol is the
entire tax.** `update_wg` is inlined into `set_block`, which is why the clone
does not appear under its own name.

The noise stage never paid it: it runs while the `_WG` heightmaps are fossils,
so `wg` is `None` and the clone never happens. The surface stage is the first
**parallel** stage that runs with them live — which is exactly the stage the
curve indicted, by a mechanism nobody had guessed.

### The fix, and what it is not

`ChunkWriter.wg` is now `Option<&'a WgPredicates>` — `Copy`, borrowed once for
the whole writer set out of a field disjoint from `chunks`. No writer touches
a refcount. The same closure is called with the same arguments in the same
order, so no behaviour change is possible; the `cmp` below proves it rather
than asserting it.

**After**, same commands, same box, same square:

```text
             instructions      cycles      IPC    CPU-s    GHz
  1 worker     534.62 G       217.24 G     2.46   55.69   3.901
  12 workers   536.03 G       218.32 G     2.46   60.16   3.629
```

**Cycles 1 -> 12 workers: +23.3% before, +0.5% after. IPC no longer drops at
all.** The +8.0% of CPU-seconds that remains is the clock (3.901 -> 3.629 GHz,
-7.0%), which is the silicon floor this file measured twice already.

Whole pipeline, `ci/stage-scale.sh`, same square:

```text
  stage        12wk cpu-s          12wk speedup       12wk efficiency
  noise        23.58 -> 23.71     10.50x -> 10.69x      88% -> 89%
  biomes       17.35 -> 17.50      5.77x ->  5.82x      48% -> 49%
  surface      29.65 -> 16.80      3.36x ->  4.52x      28% -> 38%
  TOTAL        88.08 -> 75.61      2.58x ->  2.74x      22% -> 23%
  wall                             26.69 s -> 25.76 s
  tax vs 1 worker                  +27.7% -> +7.1% CPU-seconds
```

**0.0809 -> 0.0694 CPU-s/chunk** at 12 workers over the 1,089-chunk inner
square. The 1-worker baseline moved 68.98 -> 70.62 CPU-s between the two
sweeps, which is run-to-run drift, not a regression: the instruction count at
one worker fell 535.05 G -> 534.62 G, so the work is if anything very slightly
less.

### Output-identical, proved

Control generated at the parent commit `73ca21f` from a **pristine
`git archive` checkout** — not the working tree with the change reverted —
sequential, seed 20260722, r=16, centre (64,64). `cmp` on all **4 of 4**
region files against the fixed build at `RUSTMC_WORKERS=1` and at 12:
identical, 0 differing, both widths.

### What this leaves

* **Surface still scales worst of the three parallel stages** (4.52x at 12
  workers, 38% efficiency) and that is now *Amdahl, not contention*: its
  perimeter ring runs sequentially, and at r=16 the ring is 144 of 1,369
  chunks. Bigger squares dilute it; r=32 should scale better than r=16 does.
* **Biomes at 49% efficiency** is the 256-chunk batch barrier, still
  unmeasured on its own.
* The remaining `+7.1%` is clock. There is nothing left in it for software.
* `Canvas::update_wg` — the sequential twin — cloned the same `Arc` per write
  and no longer does (`32d9d21`). **It was a null**: 716,503,072,862 ->
  716,547,198,624 instructions over the whole r=16 pipeline at one worker,
  +0.006%. `wg_live` is cleared before decoration, so the only stage that
  reached it is the carvers, and an *uncontended* atomic pair costs nothing at
  5 s of volume. Which is the useful half of the result: **the tax was never
  the atomic, it was the sharing.** Kept anyway because carvers are next on
  the parallelisation list and this is what becomes the contended form.

**The process lesson.** Five mechanisms were proposed for this tax and four
were measured dead; the fifth was published and retracted the same day. What
found it was not a better hypothesis, it was a *per-stage* instrument that
narrowed the search from a 70-second pipeline to a 15-second stage, followed
by a profile that named a symbol. Rule 9 again: measurements move suspects.
The whole-run `perf stat` that had said "instructions are flat, therefore the
allocator and false-sharing suspects are cleared" was **right about the
instructions and wrong about the conclusion** — false sharing is *exactly* an
effect that leaves instruction counts flat.

## RETRACTION: the contention tax is NOT cross-CCX (2026-07-29, same day)

**`9e2fa48` and `970c716` concluded the tax is cross-CCX placement. That is
wrong, and the run that killed it is the one those commits named as the next
step.** Keep the section below for the method; do not quote its conclusion.

**What killed it.** The noise stage alone, `worldgen-dump scale 20260722 1536
3`, three threads, same shared `&OverworldRouter` across all of them — which
is precisely the read-only-sharing configuration the cross-CCX story blamed:

```text
  threads  ONE CCX (taskset 0-2)        THREE CCX (taskset 0,3,6)
     1     19.735 s   77.8 ch/s         19.763 s   77.7 ch/s
     2     10.747 s  142.9   1.84x  92%  9.954 s  154.3   1.99x  99%
     3      7.258 s  211.6   2.72x  91%  6.674 s  230.2   2.96x  99%
```

**Spreading across three CCXes is 8% FASTER and scales at 99%, not slower.**
Packing three workers onto one CCX is what costs 9%, because three cores then
share one 8 MiB L3 and one boost domain. The sign is the opposite of the
claim.

**And the original table was contaminated by exactly the correction it had
already made once.** Those rows compared **cycles**. A memory stall costs a
fixed number of *nanoseconds*, so it costs *more cycles* at a higher clock —
and the spread placements ran at a higher clock, because three cores on three
CCXes boost further than three cores packed on one. Re-read in **task-clock**,
which is CPU-seconds and is what the efficiency target is denominated in:

```text
  workers  cpus         CCX dies   instr      cycles    GHz    CPU-s   vs 1wk
     1     (free)        1   1   715.041 G  289.756 G  4.088   70.874     --
     3     0-2           1   1   716.342 G  290.727 G  3.800   76.498   +7.94%
     3     0,1,3         2   1   716.742 G  297.987 G  3.854   77.321   +9.10%
     3     0,1,6         2   2   716.499 G  301.990 G  3.985   75.786   +6.93%
     3     0,3,6         3   2   716.322 G  316.770 G  4.039   78.418  +10.65%
     6     0-5           2   1   716.458 G  303.063 G  3.785   80.061  +12.96%
     6     0-2,6-8       2   2   716.811 G  305.945 G  3.790   80.714  +13.88%
     6     0,1,3,4,6,9   4   2   716.526 G  323.300 G  3.866   83.628  +17.99%
    12     (free)        4   2   716.736 G  342.014 G  3.764   90.861  +28.20%
```

> **In CPU-seconds the placement gradient is not monotonic in CCX count and
> barely exists.** Three workers on **two dies** (0,1,6) is *cheaper* than
> three packed on **one CCX** (0-2) — 75.786 s against 76.498 s, the opposite
> of what the retracted conclusion predicts. What the tax actually tracks is
> **worker count**: ~+7-11% at three, ~+13-18% at six, +28% at twelve, with
> only ~3 points of spread attributable to placement at a fixed count.

**What survives, and it is worth keeping.**

* Instruction counts are flat to **0.03%** across every width and placement —
  now confirmed a fourth time. Parallelism does no extra work.
* 1 -> 12 workers costs **+28.2% CPU-seconds** for the same instructions, of
  which **~8.6 points is measured all-core clock drop** (4.088 -> 3.764 GHz)
  and is not recoverable in software.
* **The noise stage is not where the rest lives**: it scales 2.96x on three
  threads at 99% efficiency, sharing one router.
* Per-worker `SurfaceNoises` is a null (-0.19%). Read-only sharing was never
  a good mechanism anyway: clean lines replicate into every L3 for free.

**So the open question is narrower and better posed than it was this
morning: the surface and biomes stages are the parallel work whose scaling has
never been measured on its own.** The noise stage has `worldgen-dump scale`;
they have nothing equivalent. Build the same instrument for them before
writing any fix — a per-stage speedup curve at 1, 2, 3, 6, 12 workers on a
fixed square. Candidate mechanisms to test with it, in order: the ~43 M
`String` allocations `biome_at` makes per run (allocator metadata is the one
thing here that is genuinely written and shared), the 256-chunk batch barrier
in the biomes stage, and the `Mutex` queue handing out chunks.

**Process note, and it is the point.** Two commits published a conclusion that
one cheap run reversed. The failure was not the hypothesis, it was quoting
**cycles** for a comparison across different clock domains after having
already written down, in the same file, that cycles and CPU-seconds diverge
because of boost. Rule 7 — when a measurement clears every candidate, suspect
the measurement — applies to the measurement that *confirms* a candidate too.

## The contention tax is cross-CCX, and it is not bandwidth (2026-07-29)

**Measured, not argued, and every cheaper explanation is excluded by a
measurement rather than by reading code.** The question: parallelism costs
+18% cycles for the same instruction count, and that is now most of the
shortfall against the wall target. Where does it go?

**First, two suspects killed outright.**

* `numactl --hardware` says **one NUMA node**, 24 CPUs, 64 GB. So "half the
  workers are reading across the Infinity Fabric to another memory
  controller" is wrong, and per-node table copies are not the fix.
* At 12 workers, LLC misses rise 113.3 M -> 153.0 M (+35%) — but 39.7 M extra
  misses at ~200 cycles each is **~8 G cycles of the 52 G extra**. DRAM
  traffic is under a sixth of it. Cache *references* barely move (3.887 G ->
  3.816 G).

**Then the discriminator, which is the whole finding.** `lscpu` says **L3 is
32 MiB in FOUR instances** — this is Zen 1, four CCXes of three cores each,
and the twelve workers do not share a last-level cache at all.
`/sys/devices/system/cpu/cpuN/cache/index3/shared_cpu_list` gives the map:
cpus 0-2, 3-5, 6-8, 9-11. So hold the worker count fixed and vary how many
L3 slices they sit on. `worldgen-dump full 20260722 16 <out> 64 64`, r=16
square at (64,64), 1,089 delivered / 1,369 generated chunks:

```text
  workers  L3 slices  cpus          instructions    cycles     vs 1wk   IPC   wall
     1         1      (free)         715.041 G   289.756 G      --      2.47  69.2 s
     3         1      0-2            716.342 G   290.727 G    +0.34%    2.46  39.1 s
     3         3      0,3,6          716.322 G   316.770 G    +9.30%    2.26  39.7 s
     6         2      0-5            716.458 G   303.063 G    +4.59%    2.36  30.9 s
     6         4      0,1,3,4,6,9    716.526 G   323.300 G   +11.57%    2.22  33.3 s
    12         4      (free)         716.736 G   342.014 G   +18.03%    2.10  27.2 s
```

> **Three workers sharing one 8 MiB L3 slice cost 0.34% more cycles than one
> worker — nothing. The same three workers on three slices cost 9.3%.** Six
> workers on two slices cost 4.6%; the same six on four cost 11.6%. Instruction
> counts agree to 0.03% across every row. **The tax tracks the number of L3
> slices in use, not the number of workers.**

So it is cross-CCX coherence traffic on **shared read-only data** — the noise
permutation tables, the density-function graph, the biome parameter list, the
surface rule tree — which every CCX has to pull into its own victim L3
independently. It is not capacity: LLC misses went *down* across three slices
(137.7 M -> 129.1 M) because there is three times as much L3.

**The confound that was nearly published as fact.** The table above varies
L3 slices, but on a 1920X the CCX count and the **die** count move together:
`/sys/.../topology/die_id` says cpus 0-5 are die 0 and 6-11 are die 1, two
CCXes each, and with one NUMA node memory is interleaved across both dies'
controllers. So "3 workers on cpus 0,3,6" spans three CCXes **and** two dies,
and the previous commit's conclusion had not separated them. Three more runs,
holding one variable at a time:

```text
  workers  cpus          CCX  dies      cycles     vs 1wk
     1     (free)         1     1    289.756 G      --
     3     0-2            1     1    290.727 G    +0.34%
     3     0,1,3          2     1    297.987 G    +2.84%
     3     0,1,6          2     2    301.990 G    +4.22%
     3     0,3,6          3     2    316.770 G    +9.30%
     6     0-5            2     1    303.063 G    +4.59%
     6     0-2,6-8        2     2    305.945 G    +5.59%
     6     0,1,3,4,6,9    4     2    323.300 G   +11.57%
    12     (free)         4     2    342.014 G   +18.03%
```

* **Crossing dies at a fixed CCX count costs ~1%:** 3 workers on 2 CCXes,
  297.99 G on one die against 301.99 G on two (+1.34%); 6 workers on 2 CCXes,
  303.06 G against 305.94 G (+0.95%).
* **Spreading CCXes at a fixed die count costs several times that:** 3 workers
  on one die, 290.73 G on one CCX against 297.99 G on two (+2.50%).
* And at a **constant three workers per CCX** — same chunks in flight per
  slice — the cost still climbs with slice count: +0.34% (1), +4.59% (2),
  +18.03% (4).

> **So the CCX reading survives and the die effect is a real but minor term.**
> The cleanest row to quote is the constant-worker-count one: **three workers,
> three chunks in flight, identical working set, and only the placement
> changing — +0.34% on one slice, +2.84% on two, +9.30% on three.**
> `9e2fa48` published the CCX conclusion before this control existed; it
> happened to survive, which is not the same as having been checked.

**The mechanism is still unnamed, and one candidate is already dead.**
Read-only lines replicate freely into every L3 in a MESI-style protocol, so
"every CCX pulls the same tables" should not by itself cost anything — and
the per-worker `SurfaceNoises` null below says it does not. What is left to
test, cheapest first: whether the noise stage **alone** shows the same
gradient. It allocates almost nothing per chunk, where the surface stage
allocates a `String` per biome query — 31,719 a chunk, ~43 M a run. If the
gradient survives with the allocation gone, the allocator is cleared too.

**A null result that narrows it, taken immediately.** The obvious first
suspect was the surface stage's `SurfaceNoises` — every worker reads the same
`Arc`, and `SurfaceNoises::new` is a pure function of `(seed, parameters)`, so
a per-worker copy is bit-for-bit the shared one and costs one construction per
worker at pool start. Built it behind a switch and A/B'd it in one binary,
12 workers, r=16 at (64,64):

```text
  shared Arc      716.758 G instructions   339.820 G cycles   26.9 s   IPC 2.11
  per worker      716.948 G instructions   339.184 G cycles   27.2 s   IPC 2.11
```

**-0.19% cycles. Nothing.** So the surface noise tables are not the shared
lines that cost the 18%, and the change was reverted rather than landed. What
that leaves, in order of size:

* **The density-function graph.** `DensityFunction::compute` is 6.88% of the
  profile *on top of* the 32% of noise math it dispatches into, and it is what
  the *noise* stage — the most parallel stage, 28.6% — hammers. Every worker
  reaches it through one shared `Arc` (`5733982`). This is the same experiment
  with a much hotter target and is the next one to run.
* **The canvas itself.** 1,369 chunks x 98,304 blocks x `u16` is **263 MB** of
  block ids written by the noise stage and read back by every later stage.
  Nothing shares a cache line, but twelve concurrent streams over 263 MB is a
  different memory profile from one, and it is the one thing whose size scales
  with the square. Interning states to `u16` does not shrink this; it is
  already `u16`. Bit-packing the palette would.
* The 4 MB biome snapshot, which every surface worker reads across.

**Two things follow, and one of them is free.**

1. **Packing workers onto fewer CCXes is cheaper in cycles *and* faster in
   wall at the same worker count.** Six workers on two slices: 30.9 s and
   303.1 G cycles. Six on four slices: 33.3 s and 323.3 G. And twelve workers
   across all four buy only 27.2 s — 12% of wall over the six packed ones, for
   13% more cycles. There is a real Pareto choice here that the current
   "one worker per physical core, wherever the scheduler puts it" default is
   not making.
2. **The fix with headroom is per-CCX copies of the hot read-only tables.**
   Not per-chunk — `5733982` correctly removed that — and not one global
   `Arc`, which is what produces this. A small fixed number of copies, one per
   L3 domain, with workers pinned to a domain. Untried; the number to beat is
   the 0.34% that three workers on one slice pay.

**Method notes.** `perf stat -e instructions,cycles,task-clock` in **one** run
per row: an earlier draft divided `perf`'s cycles by a *different* run's
`/usr/bin/time` CPU-s and got an implied 4.069 GHz against a 3.5 GHz nameplate,
which should have been the tell. Taken properly, the clock is 4.088 GHz at one
worker and 3.764 at twelve — **-7.9%, all-core boost, and unrecoverable**. That
is why the CPU-seconds tax (+28.2%) is larger than the cycle tax (+18.0%), and
why only the latter is a software problem.

## Closing r=32 row for 2026-07-29 — wall 1.98x, and past vanilla for the first time

Same harness as `run_rustmc`, same three centres, 4,225 chunks asked for per
square, seed 20260722, measured at `ae9c218`:

```text
  centre       gen wall s   CPU s      (the same run's bench-day baseline)
  64,64            95.8     321.84      192.0 / 274.30
  256,-192        106.4     349.25      213.4 / 303.42
  -320,128         99.5     319.92      191.8 / 274.23
  mean            100.57    330.34      199.07 / 283.98
  CPU-s/chunk               0.078186    0.067215
  peak RSS      1.45-1.49 GB
```

```text
  flavour   wall s   CPU s   CPU-s/chunk   vs paper wall   vs vanilla wall
  paper      32.17  239.32     0.056643        1.00x           0.31x
  vanilla   105.03  445.04     0.105335        3.26x           1.00x
  rustmc    100.57  330.34     0.078186        3.13x           0.96x
  folia     116.45  246.76     0.058406        3.62x           1.11x
```

* **Wall 199.07 -> 100.57 s a square in one turn, 1.98x**, from 6.19x Paper to
  **3.13x**, and past vanilla on wall (0.96x) for the first time. rustmc is
  now second of the four on wall and third on CPU.
* **CPU-s/chunk 0.0672 -> 0.0782**, so the `<=0.075` efficiency target is
  missed by 4.2%. The *work* fell over the same turn (-4.6% instructions
  single-threaded across the four commits); what rose is the price of running
  it on twelve cores. **And that price is two different things, which must not
  be conflated** — at `ae9c218`, r=16 at (64,64), 1 worker against 12:

```text
    instructions   715.051 G -> 716.798 G     +0.24%
    cycles         291.241 G -> 342.507 G    +17.6%
    CPU-s              71.58 ->   91.76      +28.2%
    implied clock   4.069 GHz -> 3.733 GHz    -8.3%
```

  The 1920X boosts a single core to ~4.0 GHz and holds ~3.7 all-core, so
  **8.3 of the 28.2 points is clock, not contention** and no amount of
  cache-line work will get it back. The recoverable tax is the **+17.6% in
  cycles**. A plan aimed at 28% would have been aimed at a number that is
  partly a property of the silicon.
* Re-derivation: serial-equivalent time is `330.34 / 1.2819 = 257.7` CPU-s a
  square (1.2819 is this HEAD's 12-worker CPU over its 1-worker CPU, r=16 at
  (64,64), 91.76 / 71.58). 1.5x faster than Paper is 21.45 s, needing a
  whole-pipeline speedup of **12.01x**; 2x is 16.09 s, needing 16.02x. The box
  measures 8.26x at 12 threads and 9.39x at 24 on the noise stage, which is an
  upper bound. **Still short by 1.28x at 24 threads** — and the shortfall is
  now almost exactly the contention tax.

## The biomes stage as two passes — exact by construction, not by validation (2026-07-29)

The biomes stage fills every 4x4x4 cell through `find_value`, so there is no
snapshot to hit and the surface stage's split does not transfer. But a cell is
two halves that separate cleanly:

* `ClimateSampler::sample` — six density functions, a **pure function of
  `(qx, qy, qz)`**, and the expensive half. BENCH.md's surface measurement
  puts the pair at **4.84 s of sampling against 0.19 s of searching**.
* `ParameterList::find_value` — mutates a search hint and resolves exact ties
  by the sequence of calls before it, so the *order* of these calls is
  behaviour. 0.195% of cells are ties (`golden_climate_ties.rs`).

So: **pass 1 samples every cell on a worker pool; pass 2 walks the chunks and
their cells in exactly the order they are walked today and calls `find_value`
on the stored points.** The list sees literally the same calls, with the same
inputs, in the same order. This is not speculate-validate-apply — there is
nothing to speculate and nothing to validate.

Both passes walk `stage::biome::cells`, which is now the single spelling of
the `fillBiomesFromNoise` order (sections bottom to top, then x, then y within
the section, then z) so the two cannot drift apart; a `debug_assert` compares
the coordinate and a release-build assertion compares the count.

Batched at 256 chunks so the points in flight do not scale with the radius:
256 x 1,536 cells x 48 bytes is ~19 MB at any square, against 351 MB for a
whole r=32 square.

**r=16 square at (64,64), seed 20260722, 1,089 delivered chunks**, against the
parent commit `adae9fd`:

```text
                     gen wall  proc wall   CPU-s   instructions     cycles   IPC
  adae9fd,  1 wk      68.9 s       --        --     714.714 G  287.870 G  2.48
  HEAD,     1 wk      70.9 s     71.59 s   71.58    715.051 G  291.241 G  2.46
  adae9fd, 12 wk      39.8 s     41.51 s   88.44    716.365 G  338.657 G  2.12
  HEAD,    12 wk      27.0 s     29.94 s   91.76    716.798 G  342.507 G  2.09
```

* **Whole pipeline 41.51 -> 29.94 s, 1.39x**, and 2.39x against its own
  single-threaded run.
* Instructions move **+0.06%**: the work is the same work, moved.
* Single-threaded it is **2.9% slower in wall and +1.17% in cycles** —
  materialising 1,536 `TargetPoint`s a chunk costs something when there is no
  parallelism to spend it on. That is the honest price of the split and it is
  why `RUSTMC_WORKERS=1` is not free.
* CPU-s at 12 workers 88.44 -> 91.76, **+3.8%**, the same contention tax the
  surface split pays.

BYTE-IDENTICAL: `cmp` on **4 of 4 region files, 0 differ**, at 12 workers and
at `RUSTMC_WORKERS=1`, both against dumps of the parent commit.

**Where the pipeline now is.** Parallel: noise (28.6%), surface (30.0%) and
~96% of biomes (19.7%) — about **77.5%**. Sequential: decoration (15.5%),
carvers (6.1%), structures (0.2%), the surface perimeter (144 of 1,369
chunks) and the biome `find_value` pass. Amdahl at 12 workers on 77.5% is
3.45x; the measured 2.39x is the gap contention is taking.

## The surface stage on every core — 1.76x, byte-identical, and the two targets now pull apart (2026-07-29)

With the clamp below, a chunk with all nine neighbours on the canvas
**cannot** reach the shared `ParameterList`, so the interior runs on a worker
pool and the perimeter runs sequentially afterwards in `generated().chunks()`
order. Because the interior makes no calls to the list at all, dropping it out
of that sequence leaves the sequence the list actually sees identical — this
is order-**preserving**, not order-invariant, so it needs no owner ruling and
no speculate-validate-apply pass.

**Byte-identical.** `cmp` on **4 of 4 region files, 0 differ**, at 12 workers
*and* at `RUSTMC_WORKERS=1`, both against a dump from the parent commit
`e28beb0` — r=16 square at (64,64), seed 20260722, 1,089 delivered chunks.
The guard is still live: `RUSTMC_SURFACE_TIES=1` says **0** interior
fallbacks and 144 of 1,369 chunks reaching the list, and 144 is exactly the
perimeter of the 37x37 generated square.

**r=16 square at (64,64), seed 20260722, 1,089 delivered / 1,369 generated:**

```text
                      wall     CPU-s   CPU-s/chunk  instructions    cycles    IPC
  e28beb0, 12 wk     54.2 s      --         --           --           --       --
  HEAD,    12 wk    41.38 s    88.68     0.08144     727.727 G   342.764 G   2.12
  HEAD,     1 wk    72.80 s    72.78     0.06683     725.735 G   292.439 G   2.48
```

* Whole pipeline **1.76x** on 12 cores (72.80 -> 41.38 s), and 1.35x against
  the parent's own 12-worker wall.
* **-3.13% instructions** against the 749.19 G that `9bc925f` recorded
  single-threaded: the per-chunk 3x3 biome snapshot became one snapshot for
  the whole stage (9 `Vec` clones per chunk -> 1), and 10,814 resamples went
  away with the clamp.
* Parallelism costs **+0.27% instructions and +17.2% cycles**. When only the
  noise stage was parallel that figure was +0.8%. Running 58.6% of the
  pipeline on 12 cores of a 1920X is where the memory system starts to
  charge for it.

**r=32, three centres, 4,225 chunks asked for per square** — the harness item
2 is defined against, `worldgen-dump full` exactly as `run_rustmc` invokes it:

```text
  centre        gen wall s        CPU s
                before  after   before  after
  64,64         192.0   141.7   274.30  314.58
  256,-192      213.4   162.4   303.42  345.35
  -320,128      191.8   143.4   274.23  313.19
  mean          199.07  149.17  283.98  324.37
  CPU-s/chunk                   0.06722 0.07677
```

> **The two targets now pull against each other, and this is the first
> increment where that shows.** Wall improved **1.33x** (199.07 -> 149.17 s a
> square, 6.19x -> 4.64x Paper) and CPU-s/chunk got **14.2% worse**, crossing
> the `<=0.075` efficiency target from 0.0672 to **0.0768**. Nothing regressed:
> the *work* per chunk fell 3.13% by instruction count. What rose is
> contention, and the four-server harness measures total CPU, which includes
> it.
>
> So there are two honest efficiency numbers and they must be labelled:
> **work** is 0.0668 CPU-s/chunk single-threaded (r=16 at (64,64)), and
> **what the four-server harness reads** is 0.0768 at 12 workers (r=32).
> Whichever the owner means, the wall target still needs the whole pipeline
> parallel *and* roughly another 1.3x of single-thread efficiency, and
> reducing the +17.2% cycle tax is now itself worth ~0.010 CPU-s/chunk.

## The surface stage's shared-list traffic was a missing clamp (2026-07-29)

**Measured, then read out of the bytecode, then fixed — and it moves zero
blocks.** `RUSTMC_SURFACE_TIES=1`, r=16 square at (64,64), seed 20260722,
1,369 generated chunks. The old probe reported "1362 of 1369 chunks reached
the shared `ParameterList`", which killed the interior/perimeter split for
the surface stage. Splitting the fallback by **cause** killed that instead:

```text
  before   fallbacks         123,797   the hint changed 1 answer
           of which: no chunk in the 3x3 snapshot   112,983
                     quart y off the container       10,814
                       every one of them at block y = -68
                       9,716 of them from a chunk with all nine neighbours
           chunks reaching the shared list   1362 of 1369  (99.5%)
```

`LevelReader#getNoiseBiome` (javap, 26.2 server jar) asks the region for the
chunk at `(toSection(qx), toSection(qz))` and **only if that is null** falls
through to `getUncachedNoiseBiome` and resamples the climate. When the chunk
is there it calls `ChunkAccess#getNoiseBiome`, which **clamps**:
`Mth.clamp(y, fromBlock(minY), fromBlock(minY) + fromBlock(height) - 1)`.
This port returned `None` for an out-of-range quart y and the caller
resampled — a query vanilla never makes, answered by a computation vanilla
never runs, and one that also perturbs the search hint for every later call.

y = -68 is `(y - 2) >> 2 = -17` for y in -64..=-63: the one quart below the
world floor, reached by the rule walk at the bottom of a column.

```text
  after    fallbacks         112,983   the hint changed 0 answers
           interior fallbacks      0
           chunks reaching the shared list    144 of 1369  (10.5%)
```

**144 is exactly the perimeter** of the 37x37 generated square (4*37 - 4).
Every chunk with all nine neighbours on the canvas is now provably clean —
not measured to be clean, *provably*: the biome fuzz reaches at most one
chunk out, and the only remaining miss is "the snapshot has no such chunk".

**Output-identical, over the r=16 square at (64,64), 1,089 delivered
chunks, 107,053,056 blocks:** `region-differ diff` 0 mismatches,
100.000000%, and `cmp` on **4 of 4 region files, 0 differ**. So the defect
was latent on this window; the clamp is worth no parity blocks here and
everything to the parallel split.

## The four-server benchmark, re-run at r=32 (2026-07-29) — the efficiency target is MET

**This is the number item 2 is defined against.** Every previous claim in
this file about where rustmc stands came from `worldgen-dump` at r=16, and
the two harnesses' absolutes must never be divided into each other. This is
`ci/bench-servers.sh`, four servers, one box, back to back:

```text
  BENCH_RADIUS=32 BENCH_WARMUP_RADIUS=7 BENCH_PROBE_TIMEOUT=1800 ci/bench-servers.sh
```

Threadripper 1920X (12c/24t), JDK 25.0.3, seed 20260722, `-Xms4G -Xmx4G` on
all three JVMs, Paper and Folia fetched from `fill.papermc.io` and
sha256-verified. **Three squares of 4,225 chunks asked for** — centres
(64,64), (256,-192), (-320,128) — every server asked for the same three,
one boot each, warmed on a 225-chunk throwaway square first. Raw rows in
`oracle-work/bench-archive/results-r32-2026-07-29.tsv`, full log beside it.

```text
  flavour   wall s   CPU s   CPU-s/chunk   vs vanilla CPU   vs paper CPU   vs paper wall
  paper      32.17  239.32     0.056643        0.54x           1.00x          1.00x
  folia     116.45  246.76     0.058406        0.55x           1.03x          3.62x
  rustmc    199.07  283.98     0.067215        0.64x           1.19x          6.19x
  vanilla   105.03  445.04     0.105335        1.00x           1.86x          3.26x
```

Per centre, rustmc: wall 192.0 / 213.4 / 191.8 s, CPU 274.30 / 303.42 /
274.23 s. Peak RSS **1.37-1.41 GB**, against 14.4 GB for the same square in
the 2026-07-28 run — `5733982` (Arc the immutable noise) took the retained
per-chunk state from ~3.0 MB to ~0.29 MB. The three JVMs did it in 4 GB.

**Where this leaves the two targets, both stated over the r=32 square,
4,225 chunks asked for, three centres, seed 20260722:**

* **Efficiency `<=0.075 CPU-s/chunk`: MET. 0.0672.** rustmc now spends
  **less CPU per chunk than vanilla** (0.64x) for the first time, and is
  1.19x Paper. The stretch target `<=0.053` is not met.
* **Wall `1.5-2x faster than Paper`: not met, and the gap is 6.19x.**
  Paper is 32.17 s a square, so 1.5x faster is 21.45 s and 2x is 16.09 s.

**The re-derivation the old arithmetic needs.** The 2026-07-28 version said
the wall goal needed 22.0x and the box gives 9.6x, "so the stretch
efficiency target is a precondition". Redone on these numbers:

```text
  rustmc is single-threaded except the noise stage, so its serial-equivalent
  time is its CPU-s: 283.98 s a square.
  wall at a whole-pipeline speedup S is 283.98 / S.
  21.45 s  =>  S >= 13.24        1.5x faster than Paper
  16.09 s  =>  S >= 17.65        2x
  measured on this box: 8.26x at 12 threads, 9.39x at 24 (worldgen-dump
  scale, the noise stage — the most parallel stage there is, so an upper
  bound for the pipeline).
```

> **So multicore alone still does not reach it, but the shortfall is now
> 1.41x rather than 2.3x.** At 9.39x the wall goal needs **0.0477
> CPU-s/chunk**; the stretch target of 0.053 lands at 23.8 s, which is 1.35x
> faster than Paper — close, and short. The stretch target is no longer
> *sufficient*, and it is also no longer the binding constraint: the binding
> constraint is that **71.4% of the pipeline still runs on one core.**
> At r=32 the process sits at 100.0% CPU for most of its 192 s.

**Parity at r=32**, 4,225 chunks / 415,334,400 blocks, `region-differ diff`
with **vanilla first** every time:

```text
  vanilla vs paper     145,016 blocks   99.965086%   biomes 95/6,489,600
  vanilla vs folia     386,183 blocks   99.907012%   biomes 95/6,489,600
  vanilla vs rustmc    717,655 blocks   99.827242%   biomes  0/6,489,600
```

rustmc is the only one of the three with **zero** biome mismatches. Its
worst chunks are interior — (40,32) at 13,580 and (51,72) at 11,254 — where
Paper's and Folia's are all on the square boundary. That interior residual
is item 1's, and it moved from 747,518 on 2026-07-28 to 717,655 here with no
work aimed at it (the village track). Do not read the three rows as a
ranking: Paper and Folia are being compared to vanilla's own chunk-promotion
order, and rustmc to its raster order.

## The server benchmark: vanilla, Paper, Folia, rustmc (2026-07-28)

**Item 2 of the definition of complete — "fastest vanilla-parity server" —
had never been measured. It is now, and it is not met: rustmc is the slowest
of the four.**

```
ci/bench-servers.sh                       # all four, method in the header
BENCH_FLAVOURS="vanilla rustmc" ci/bench-servers.sh
```

Threadripper 1920X (12c/24t), JDK 25.0.3, seed 20260722, Paper build 84,
Folia build 1, both fetched from `fill.papermc.io` and sha256-verified.
**The workload is producing a finished 15x15 chunk square** — 225 chunks
asked for — through noise, surfaces, carvers, features and structures. Three
separated squares, every server asked for the same three, one boot each, JVM
warmed on a fourth throwaway square first.

| flavour | centre | wall s | CPU s | peak cores |
|---|---|---|---|---|
| vanilla | 64,64 | 17.82 | 47.56 | 8.44 |
| vanilla | 256,-192 | 10.82 | 42.44 | 13.61 |
| vanilla | -320,128 | 10.02 | 39.69 | 10.80 |
| paper | 64,64 | 5.81 | 28.70 | 10.14 |
| paper | 256,-192 | 3.61 | 25.44 | 8.94 |
| paper | -320,128 | 3.41 | 21.71 | 8.09 |
| folia | 64,64 | 18.63 | 32.06 | 11.09 |
| folia | 256,-192 | 14.62 | 28.53 | 9.24 |
| folia | -320,128 | 7.01 | 21.52 | 7.34 |
| **rustmc** | 64,64 | 53.90 | 54.29 | 1.00 |
| **rustmc** | 256,-192 | 60.00 | 60.46 | 1.00 |
| **rustmc** | -320,128 | 63.20 | 63.69 | 1.00 |

Means over the three squares, and CPU per delivered chunk (denominator 225,
the same square on every row):

| flavour | wall s | CPU s | CPU s / chunk | vs vanilla CPU | vs vanilla wall |
|---|---|---|---|---|---|
| paper | 4.28 | 25.28 | 0.112 | **0.58x** | **0.33x** |
| folia | 13.42 | 27.37 | 0.122 | 0.63x | 1.04x |
| vanilla | 12.89 | 43.23 | 0.192 | 1.00x | 1.00x |
| **rustmc** | **59.03** | **59.48** | **0.264** | **1.38x** | **4.58x** |

Per centre rather than per mean, because the terrain spread is large:
rustmc costs **1.14x / 1.43x / 1.60x** vanilla's CPU and **3.02x / 5.55x /
6.31x** its wall. Against Paper, which is the one to beat, **2.35x the CPU
and 13.8x the wall**.

Vanilla's 0.192 CPU-s per chunk here independently reproduces the 0.211
figure this file has been carrying, measured a different way.

### The wall gap and the CPU gap are different problems

The wall column is almost entirely thread count: rustmc's pipeline is
single-threaded — there is no `rayon` in the tree — and its peak is 1.00
cores against 8-14 for the JVMs. That is a known architectural gap, not a
generator one.

The CPU column is the real reading, and 1.38x of it is not all generator
either. rustmc generates a `DECORATION_MARGIN`-2 skirt: to deliver 15x15 it
generates 19x19 = 361 chunks and throws 136 of them away, **37.7% of the
work**. Per chunk *generated* it is at 0.165 CPU-s, below vanilla's 0.192
per chunk delivered. That is not a like-for-like win, because vanilla and
Paper pay for a skirt too and it cannot be separated from outside the
process — but it does mean r=7 is the size that flatters us least, and the
margin fraction falls to 11% at r=32.

**Measured there, and the table below supersedes this one.** The r=7 reading
turned out to flatter rustmc rather than the other way round: vanilla's own
skirt is about three times ours, so it amortises harder, and at r=32 the gap
widens from 1.38x to 1.71x. The "it cannot be separated from outside the
process" in the paragraph above is also wrong, and the next section says how.

### Parity is not free for the forks either

Same square, same seed, same driver, vanilla first. 225 chunks,
22,118,400 blocks:

| pair | mismatched blocks | exactness |
|---|---|---|
| vanilla vs **rustmc** | **4,395** | **99.980130%** |
| vanilla vs paper | 7,436 | 99.966380% |
| vanilla vs folia | 29,922 | 99.864719% |

**Do not quote this as "rustmc is more vanilla-accurate than Paper."** It is
one square, one driver and **one** vanilla reference, and this file's own
rule is that a residual is only a defect when two independently generated
vanilla worlds agree on it. Cross-chunk decoration order alone is worth
4,885 blocks between two vanilla extents (see "vanilla disagrees with itself
more than we disagree with it"), Paper runs Moonrise's chunk system and
Folia a regionised scheduler, so both promote chunks to `full` in a
different order from vanilla and most of their residual is probably that.
What the table does establish is that **"vanilla parity" is a live variable
for the forks too**, and that a benchmark which assumes the three Java
servers produce the same world is assuming something false.

Folia's row is worse than it should be for a second reason, below.

### Two things this measurement got wrong first, and what fixed them

**The probe.** The first version froze ticks and polled `save-all flush` +
`region-differ scan` until the square was on disk. **Folia implements
neither `/save-all` nor `/tick`.** Every command was rejected, and what the
run actually measured was Folia's five-minute autosave interval: 303 s of
wall against 33 s of CPU, silently, in a table that looked fine. The probe
is now CPU only (`ci/bench-probe.py`, `/proc/<pid>/stat`, busy-then-idle
edge detection), nothing is sent to a server inside the timed window, and
the identical code runs against all three.

**The gamerules.** `gamerule randomTickSpeed 0` and its four companions are
rejected by **vanilla 26.2 itself** — the ids are snake_case now, and
several are not renamings: `doMobSpawning` is `spawn_mobs`, `doWeatherCycle`
is `advance_weather`, `doFireTick` has become the integer
`fire_spread_radius_around_player`. Read out of the constant pool of
`net/minecraft/world/level/gamerules/GameRules.class`. `ci/gen-vanilla-region.sh`
— which generates every vanilla reference world this project measures
against — has been sending the old names since it was written, so its
"belt-and-braces" gamerules have never once taken effect and `tick freeze`
has been carrying it alone, exactly as its own comment says it does. Both
scripts now send 26.2's ids. Nothing generated before this is invalidated:
freeze stops the tick loop, and the gamerules only ever mattered if freeze
failed.

Folia rejected `tick freeze` as well as the gamerules, so its square ticked
live while it generated. Its CPU column is therefore an over-estimate and
its parity row carries whatever that ticking mutated. **With 26.2's real
rule ids Folia does take all seven gamerules** — checked after the fix, 1 of
9 commands rejected against 6 of 7 before — so only `/tick` is still missing
there and the next run's Folia row will be cleaner than this one's. The r=7
table above was taken with the broken names on all three servers, which is
at least uniform.

### Row-batched, and the skirt is visible on vanilla too

`forceload add` refuses more than 256 chunks, so r=7 was the largest square
the harness could ask for in one command. It now issues one command per
chunk row past that cap, verified at r=8 (289 chunks, delivered by both
vanilla and Folia). Timing is unaffected; the parity column is not, because
the promotion order changes, so the two radii's parity rows are not
comparable to each other.

The first thing that came back is worth having: vanilla at centre (64,64)
costs **0.211 CPU-s/chunk at r=7 and 0.185 at r=8** — 47.56 s / 225 against
53.41 s / 289. Its skirt amortises exactly the way rustmc's should, which
means the r=32 reading has to be taken on both sides before rustmc's 1.38x
can be attributed to the generator. (The r=8 run also had working gamerules,
so it is a corroboration of the direction, not a fourth row for the table.)

## The same benchmark at r=32 — the margin was hiding a bigger gap (2026-07-28)

```
BENCH_RADIUS=32 BENCH_WARMUP_RADIUS=7 BENCH_PROBE_TIMEOUT=3600 \
    BENCH_CENTRES="64,64 256,-192 -320,128" ci/bench-servers.sh
```

**4,225 chunks asked for per square** — 18.8x the r=7 square — same three
centres, same seed 20260722, same `-Xms4G -Xmx4G`, same machine, one boot per
server. Row-batched forceload (65 commands, one per chunk row) and every
server delivered `full=4225` on all three squares. The JVMs were warmed on a
225-chunk square, i.e. on exactly what warmed them for the r=7 table.

| flavour | centre | wall s | CPU s | peak cores |
|---|---|---|---|---|
| vanilla | 64,64 | 104.37 | 369.40 | 14.43 |
| vanilla | 256,-192 | 114.78 | 464.12 | 13.63 |
| vanilla | -320,128 | 95.76 | 474.23 | 15.13 |
| paper | 64,64 | 34.25 | 240.68 | 11.94 |
| paper | 256,-192 | 34.24 | 260.57 | 13.33 |
| paper | -320,128 | 35.25 | 277.05 | 12.93 |
| folia | 64,64 | 99.93 | 243.60 | 12.88 |
| folia | 256,-192 | 157.74 | 249.49 | 13.97 |
| folia | -320,128 | 100.32 | 242.67 | 15.53 |
| **rustmc** | 64,64 | 692.10 | 699.56 | 1.00 |
| **rustmc** | 256,-192 | 771.90 | 779.91 | 1.00 |
| **rustmc** | -320,128 | 752.40 | 760.29 | 1.00 |

Means, CPU per delivered chunk over 4,225:

| flavour | wall s | CPU s | CPU s / chunk | vs vanilla CPU | vs vanilla wall | vs paper CPU |
|---|---|---|---|---|---|---|
| folia | 119.33 | 245.25 | **0.0580** | 0.56x | 1.14x | 0.95x |
| paper | 34.58 | 259.43 | 0.0614 | 0.60x | 0.33x | 1.00x |
| vanilla | 104.97 | 435.92 | 0.1032 | 1.00x | 1.00x | 1.68x |
| **rustmc** | **738.80** | **746.59** | **0.1767** | **1.71x** | **7.04x** | **2.88x** |

**The r=7 table flattered rustmc, not the JVMs.** Going from 225 chunks to
4,225 the deficit widens from 1.38x to **1.71x** vanilla's CPU and from 2.35x
to **2.88x** Paper's; on wall it goes 4.58x -> 7.04x against vanilla and
13.8x -> 21.4x against Paper. Per centre, rustmc is 1.89x / 1.68x / 1.60x
vanilla's CPU and 6.63x / 6.73x / 7.86x its wall. Folia also overtakes Paper
on CPU at this size (0.0580 against 0.0614), which it did not at r=7.

The reason is the thing the r=7 section guessed wrong. Both sides amortise a
skirt, but vanilla's is about three times ours, so a bigger square helps
vanilla more than it helps us.

### Vanilla's skirt *can* be measured from outside the process

The r=7 section says it cannot. It can — by measuring the same centre at
several radii and fitting, because a skirt of `m` chunks makes the cost per
*delivered* chunk `c * ((2r+1+2m) / (2r+1))^2` and only one `(m, c)` pair
fits four radii at once. Vanilla at centre (64,64), all four points measured
by this harness:

| radius | delivered | CPU s | CPU s / delivered | implied c at m=6.5 |
|---|---|---|---|---|
| 7 | 225 | 47.56 | 0.21138 | 0.06066 |
| 8 | 289 | 53.41 | 0.18481 | 0.05934 |
| 16 | 1089 | 127.04 | 0.11666 | 0.06004 |
| 32 | 4225 | 369.40 | 0.08743 | 0.06072 |

> **vanilla generates an effective ~6.5-chunk skirt, at 0.0602 CPU-s per
> chunk generated** — a 2.3% spread in `c` across a 19x range in delivered
> chunks. No other `m` comes close: m=2 spreads 51%, m=4 26%, m=5 15%, m=8
> 14%. m=6 gives 4.6% and m=7 6.3%.

**The control that makes this a skirt and not terrain:** rustmc's skirt is
known exactly (`DECORATION_MARGIN` = 2), so its cost per *generated* chunk is
the same quantity at every radius, and any drift in it is the terrain the
bigger window pulled in. At the same centre (64,64) that drift is **-2.3%**
from r=7 to r=32 — 0.15039 to 0.14694 CPU-s per generated chunk — against
vanilla's -58.6% fall in cost per delivered chunk over the same two radii.
Terrain is a rounding error here; the fall is skirt.

Two-point solves for the forks, same centre, agree without being told to:
**Paper m=5.7 at c=0.0412, Folia m=6.7 at c=0.0397.** Two points and two
unknowns is an exact solve with no goodness of fit, so these are corroboration
rather than measurements — but three independent JVM servers landing between
5.7 and 6.7 is what you would expect if the number is the chunk-status
dependency radius they share, and is not what you would expect from noise.

So the like-for-like generator comparison, skirt removed on both sides:

> **rustmc costs 0.147 CPU-s per chunk generated against vanilla's 0.060 —
> 2.44x.** Against Paper's 0.041, 3.6x.

That reproduces, from a completely different measurement, the **2.25x** this
file has carried since 2026-07-26 from differencing two `rustmc-server`
radii against two `gen-vanilla-region.sh` radii. Two methods, one answer.

### rustmc needed 14.4 GB of RAM to do what the JVMs did in a 4 GB heap

`worldgen-dump full` at r=32 was observed resident at **14.4 GB** — 4,761
generated chunks at about **3.0 MB each** — because `Pipeline` holds the
whole square's `Canvas` *and* every chunk's retained aquifer, flat caches and
surface levels alive until the carver stage runs. The three JVMs produced the
same square inside `-Xmx4G`. The harness now records `%M` so this is a column
rather than someone's memory of a `ps` output.

This is not a micro-optimisation note. It is the same fact as the wall
column: the pipeline is a batch job over a fixed rectangle, and a server
streams. Nothing about `DECORATION_MARGIN` requires holding chunk `(32,32)`'s
aquifer while chunk `(96,96)` is being filled.

### Parity at r=32, and it is not the r=7 story

4,225 chunks, 415,334,400 blocks, vanilla first, first centre only:

| pair | mismatched blocks | exactness |
|---|---|---|
| vanilla vs paper | 156,351 | 99.962355% |
| vanilla vs folia | 395,968 | 99.904663% |
| vanilla vs **rustmc** | **747,518** | **99.820020%** |

At r=7 rustmc had the *best* of these three rows. At r=32 it has the worst.
Do not read either as a ranking — the r=7 caveats all still apply, the two
radii's parity rows are not comparable to each other because row-batching
changes promotion order, and Paper's and Folia's worst chunks here are all on
the square's boundary — (95,89), (96,88), (94,91) — where rustmc's are
interior: (40,32) with 13,598 blocks, (51,72) with 11,203. Interior residual
at that size is item 1's, and 4,225 chunks is a much larger sample than item
1 has been working on.

### Re-measured after the two perf commits — 1.71x vanilla becomes 1.19x

Same harness, same three centres, same 4,225-chunk squares, `BENCH_FLAVOURS=rustmc`.
The JVM rows are unchanged and are the ones above: **rustmc's output is
byte-identical before and after**, so the parity rows are unchanged too, which
is the whole point of the output-identical rule.

| flavour | wall s | CPU s | CPU s / chunk | vs vanilla CPU | vs paper CPU | vs paper wall |
|---|---|---|---|---|---|---|
| folia | 119.33 | 245.25 | 0.0580 | 0.56x | 0.95x | 3.45x |
| paper | 34.58 | 259.43 | 0.0614 | 0.60x | 1.00x | 1.00x |
| vanilla | 104.97 | 435.92 | 0.1032 | 1.00x | 1.68x | 3.04x |
| rustmc *before* | 738.80 | 746.59 | 0.1767 | 1.71x | 2.88x | 21.36x |
| **rustmc *after*** | **506.90** | **517.03** | **0.1224** | **1.19x** | **1.99x** | **14.66x** |

Per centre against vanilla's CPU: **1.31x / 1.23x / 1.05x**, from 1.89x /
1.68x / 1.60x. Per chunk *generated*, 0.1568 -> **0.1086** against vanilla's
fitted 0.0602 and Paper's 0.0412. Peak RSS 13.72-13.73 GB across the three
squares, now recorded by the harness rather than read off `ps`.

### What the goal actually requires, in numbers

The targets are **<=0.075 CPU-s/chunk** (1.5x better than Paper's 0.0614),
stretch **<=0.056** (2x), and **1.5-2x Paper on wall**. Paper's wall on this
square is 34.58 s, so 1.5x faster is **23.05 s**.

| | now | needed | gap |
|---|---|---|---|
| CPU s/chunk | 0.1224 | <=0.075 | **1.63x** |
| CPU s/chunk (stretch) | 0.1224 | <=0.056 | 2.19x |
| wall, one square | 506.90 s | <=23.05 s | **22.0x** |

**Multicore alone does not reach the wall goal, and this is the load-bearing
arithmetic on the whole item.** `worldgen-dump scale` measures this box at
**8.26x on 12 threads and 9.39x on 24** for the noise stage, and that is the
optimistic end because it is the most parallel stage there is. Take 9.6x:
506.90 / 9.6 = **52.8 s**, which is **2.3x short** of 23.05 s.

So the wall goal needs multicore **and** a further ~2.3x of single-core
efficiency, i.e. **0.1224 / 2.29 = 0.053 CPU-s per chunk**. That is the
stretch CPU target (0.056) almost exactly. The two goals are not independent
asks that can be traded off against each other:

> **Hitting the stretch efficiency target is a precondition for the wall
> target, not an alternative to it.** 0.053 CPU-s/chunk plus 9.6x of scaling
> is 22.1x, against a 22.0x requirement. There is no headroom in that, so
> anything that pushes efficiency past the stretch target buys margin on
> both goals at once.

Two things that make it less tight than it reads. The **margin** is still
11.3% of rustmc's work at r=32 and vanilla's fitted skirt is 6.5 chunks
against our 2 — at a server's real working radius ours amortises to
essentially nothing, and the `chunks/s` a player experiences is a streaming
number rather than a square-completion one. And the profile has not been
retaken since these two commits landed; the 1.79x they bought came entirely
from three symbols nobody had costed.

### Two things the harness needed before r=32 could be trusted

- **The probe's deadline was 1,200 s**, chosen when the slowest square was
  18.6 s of wall. At 18.8x the chunks that is no longer obviously generous,
  so it is `BENCH_PROBE_TIMEOUT` now and the r=32 run used 3,600.
- **The gap between squares was a `sleep 3`.** `forceload remove all` drops
  the tickets on the square just timed and the server then unloads and
  *writes* it; at r=7 that is 225 chunks and invisible, at r=32 it is 4,225
  and anything still running is charged to the next square. It is now
  `settle()` — the same CPU probe with no commands sent — and it prints what
  it cost, so the next run quantifies what this one could only bound. The
  r=32 numbers above were taken before that landed, and the bound comes from
  rustmc: it runs a **fresh process per square**, so it cannot carry anything
  between them, and its three squares came back 700 / 780 / 760 CPU-s. The
  three that can carry something ordered themselves the same way — vanilla
  369 / 464 / 474, Paper 241 / 261 / 277 — cheapest square first on both
  sides. A contaminated run would put the *later* squares up relative to a
  process that cannot be contaminated, and they are not. Folia is the one
  that does not follow (244 / 249 / 243), and Folia is also the one still
  ticking live because it has no `/tick`.

## The 2026-07-28/29 single-core pass — 1.43x, and the `getenv` census

Everything here is measured with **one** harness, and every ratio in it is
over **one** region, so that a later reader does not have to guess which:

```
  ./target/release/worldgen-dump full 20260722 16 <out> 64 64
  1089 chunks in the square, 1369 generated once the 2-ring decoration margin
  is included; the CPU-per-chunk figures divide by the 1089.
```

This is **not** the same harness as the four-server table above, which drives
a real server through `forceload` and reported 0.1224 CPU-s/chunk at r=32.
Do not compare a number from one against a number from the other. The
*ratios* below are within-harness and carry; the absolutes do not.

```text
  commit    what                              wall     CPU-s/chunk  maxrss
  8ca9123   before this pass                 116.39 s    0.1069     4.23 GB
  5b3fca2   three getenv sites                102.19 s    0.0952     4.23 GB
  5733982   Arc the immutable noise            81.62 s    0.0750     0.36 GB
            cumulative                                    1.426x     11.9x less
```

Every step is output-identical: 4 of 4 region files byte-identical by `cmp`
against the previous step's dump of the same square, `diff -rq` over the
whole directory empty.

### The profile that drove it, and the one that did not

The pass began by **re-recording**, because the standing profile had named
noise at 54% — a figure imported from a sibling project — and this tree's own
`perf` said 17%. Folding 24,097 stacks of the r=16 run:

```text
  ImprovedNoise (any frame)          16.93%   real work
  allocator (malloc/free family)     11.15%   waste
  std::env::__var (any frame)         8.03%   waste
```

Neither piece of waste was guessable from the self-time list — `getenv` sat
at 3.82% self, and `malloc` at 2.05%. **A symbol at 3% self is worth folding
the stacks for**; both of this pass's wins were found that way and neither
would have been chosen from the top of a `perf report`.

### The `getenv` census — all 28 sites, classified once

Two separate rounds of hot-path `getenv` had been found and fixed one profile
at a time (`0163397`, then `5b3fca2`). That is a pattern in the codebase, not
two accidents, so the whole tree was enumerated rather than sampled:

```
  grep -rn 'env::var' crates/*/src tools/*/src --include=*.rs
```

**28 in `crates/`** (27 worldgen, 1 server) and **25 in `tools/`**. The
classification:

| class | count | disposition |
| --- | --- | --- |
| already cached (`OnceLock`/`LazyLock`/`thread_local!`) | 8 | left alone |
| the `trace_switch` implementation and its tests | 4 | left alone |
| genuinely cold — CLI parse, one-shot setup, once-per-run report | 21 | left alone |
| hot by construction — per block, per column, per attempt, per chunk | 12 | converted |
| another track's file (`rustmc-server/src/ticks.rs`) | 1 | not mine |

The twelve converted, with what "hot" means for each:

```text
  surface::eval::test                RUSTMC_TRACE_BLOCK        per condition
  aquifer::compute_substance         RUSTMC_TRACE_AQUIFER      per open block
  chunk_gen::NoiseFill::material     RUSTMC_DUMP_FILL_DENSITY  per block
  feature::geode::place              RUSTMC_TRACE_GEODE        per geode
  feature::geode (block loop)        RUSTMC_TRACE_GEODE        per block of one
  speleothem_cluster (column)        RUSTMC_TRACE_SPELEO_COLUMNS  per column
  speleothem_cluster (column)        RUSTMC_TRACE_SPELEO       per column
  feature::lake::place (x2)          RUSTMC_TRACE_LAKE         per lake
  carver::driver::carve_chunk        RUSTMC_TRACE_CARVER       per chunk
  structure::writer                  RUSTMC_TRACE_MINESHAFT    per piece
  structure::mineshaft (x3)          RUSTMC_TRACE_MINESHAFT[_PHASES]  per section
  feature::monster_room::trace_chest RUSTMC_TRACE_ROOM         per chest attempt
  worldgen-assembly::generate (x4)   TRACE_CARVER/DUMP_CHUNK/TRACE_MINESHAFT  per chunk
```

**The null result, which is the point of writing this down.** The first three
were worth **1.123x** on the r=16 square. The other nine, swept afterwards,
are worth **nothing measurable**: after the first three landed, `env::__var`
appears in **7 of 17,594** folded stacks — 0.04% — and three timed runs of
the swept build (85.13, 82.62, 81.99 s) straddle the unswept build's 81.62 s,
which is the box's own noise with two other agents on it. They are per-block
loops inside features that are *rare* in this window; the sweep removes the
pattern and buys no time here. **Do not re-profile looking for them.**

The semantic change is deliberate and was checked, not assumed: a cached
switch cannot be toggled part-way through a process. `grep -rn 'set_var'`
over `crates/` and `tools/` matches **nothing but a doc comment**, so no test
and no harness flips one at runtime. Each converted switch was also re-run
after the change and still prints — `RUSTMC_TRACE_CARVER` 67 lines,
`RUSTMC_TRACE_LAKE` 365, `RUSTMC_TRACE_MINESHAFT` 540.

### Why the allocator was 11% — and why the memory number matters more

`stage::noise::inputs` clones the router's whole density graph **once per
chunk**, because `ChunkSampler::new` numbers the markers in place and needs a
graph of its own. A `NormalNoise` is two `PerlinNoise` stacks and every
octave carries a 256-byte permutation table, so each chunk was copying and
then freeing every noise table in the router. Four types are built once and
never mutated — `NormalNoise`, `BlendedNoise`, `EndIslandNoise`,
`Multipoint`, none of which has a single `&mut self` method — and moving them
from `Box`/by-value to `Arc` makes their share of that clone a refcount bump.
The graph *shape* is still cloned per chunk; the markers still need their own
`slot`.

Peak RSS went from **4.23 GB to 0.36 GB**. That is the number that matters
for what comes next: a worker pool needs a router per thread, and at 4.23 GB
for one it could not have had many.

### The profile after the pass, same square

```text
  self time                         inclusive (folded stacks)
  ImprovedNoise::noise_with_fudge  23.86%     ImprovedNoise      24.17%
  DensityFunction::compute          6.90%     decoration          8.08%
  PerlinNoise::get_value_with_fudge 6.48%     biome               7.83%
  ChunkSampler::evaluate            5.49%     allocator           6.27%
  Canvas::set_block                 4.78%     Canvas::set_block   4.88%
  biome::manager::quart_for_block   4.71%     quart_for_block     4.83%
  Canvas::block_at                  3.40%     Canvas::block_at    3.53%
  stage::decorate::form_bubble_columns 3.16%  form_bubble_columns 3.10%
```

And the stage times for the same run (`RUSTMC_STAGE_TIMES=1`, 82.71 s over
1369 generated chunks):

```text
  noise       23.67 s  28.6%   17.3 ms/chunk
  biomes      16.25 s  19.7%   11.9 ms/chunk
  surface     24.83 s  30.0%   18.1 ms/chunk
  carvers      5.02 s   6.1%    3.7 ms/chunk
  structures   0.14 s   0.2%    0.1 ms/chunk
  decoration  12.80 s  15.5%    9.3 ms/chunk
```

The surface stage asks the biome source **28,864 times per chunk**
(39,515,420 over 1369), which is 5.09 s of the 24.83 — 4.91 s of climate
sampling and 0.19 s of parameter search. That, `Canvas::set_block` and
`quart_for_block` are the next single-core candidates; noise itself is real
work and SIMD over it changes float association, which is forbidden.

## The noise stage on every core — 1.37x wall, and 22% more CPU (2026-07-29)

Same harness and same square as the section above: `worldgen-dump full
20260722 16 <out> 64 64`, **1089 chunks in the square**, 1369 generated with
the 2-ring decoration margin.

```text
  build                              wall     CPU-s   CPU-s/chunk
  pristine HEAD, sequential         80.92 s   80.50     0.0739
  this change, RUSTMC_WORKERS=1     80.49 s   80.02     0.0735
  this change, 24 workers           58.98 s   98.05     0.0900
                                    1.372x             +21.8% CPU
```

**All three are byte-identical** — 4 of 4 region files by `cmp`, pristine
sequential against one worker against twenty-four.

**Take the pristine control, not just the one-worker run.** The tree is
shared, and another track landed `fallen_tree` between two of these dumps. It
produced a one-region `DIFF` that looked like a parallelism bug and was not.
A `cmp` against a build of the same HEAD is the only comparison that means
anything here.

### Why it cannot change a block

`Canvas` keeps its chunks in a `BTreeMap<(i32,i32), ChunkBlocks>`, so
`values_mut()` hands out every chunk's `&mut` at once and the borrows are
disjoint by construction. `Canvas::chunk_writers()` wraps each in a
`ChunkWriter` whose `set_block` takes **absolute** coordinates and drops
writes outside its own chunk — the same thing the canvas already does for a
chunk it does not hold. It asserts `wg_live.is_none()`, because `update_wg`
is a read-modify-write of a **neighbour's** column when a write lands on a
border, so a stage running under `set_wg_live` is not chunk-local by
definition.

Three facts make the output independent of how the work was divided:

* each chunk's **palette is per chunk** and interned in that chunk's own
  write order, so two chunks on two threads cannot reorder either one's;
* the per-chunk `Retained` (aquifer, flat caches, surface levels) is a pure
  function of position;
* the results go back into the same `BTreeMap`, so the reassembled order is
  the canvas's order whatever order the workers finished in.

Work is pulled **one chunk at a time off a `Mutex<vec::IntoIter>`** rather
than sliced up front. Chunk cost varies by terrain and a static split
finishes on its slowest slice; the lock is taken once per chunk against ~17
ms of work, which is nothing.

### The 22% is the finding — **WRONG, superseded below**

*Kept because the reasoning below it is the reasoning that got corrected, and
because the correction is the point. Everything in this subsection about
allocators and memory buses is false: `perf stat` says the instruction count
is the same at 1, 12, 16 and 24 threads. Read
["The 22% was hyperthreading"](#the-22-was-hyperthreading-2026-07-29) first.*

The efficiency target is CPU-s per chunk; the wall target is seconds per
square. **This lever trades one for the other**, and the arithmetic that says
the stretch efficiency target is a *precondition* for the wall target gets
worse rather than better as cores are added.

It is not lock contention. Amdahl over the measured 28.6% noise share and the
observed 1.372x whole-pipeline speedup puts the stage's own scaling at ~19x
on 24 threads:

```text
  1 / (0.714 + 0.286/k) = 1.372  ->  k ~ 19.2
```

so the stage is scaling nearly linearly and the extra CPU is 24 threads
sharing an allocator and a memory bus. **Anything that shrinks the per-chunk
working set now pays twice** — once on single-core efficiency and once on the
bus every worker shares. That, not more threads, is the shape of the next win.

`RUSTMC_WORKERS=<n>` overrides the thread count; `1` forces the sequential
path, which is what the byte comparison is taken against. Unset means every
core the machine reports.

## The 22% was hyperthreading (2026-07-29)

The paragraph above says the extra CPU "is 24 threads sharing an allocator
and a memory bus". It was never measured — it was inferred from Amdahl and
from the allocator being 11% of a profile. `perf stat -e instructions,cycles`
over the same square, same binary, four widths, settles it:

```text
  threads  pin    wall     CPU-s   instructions   cycles     IPC    GHz
   1        -     78.99 s  78.54   808.786 G      322.42 G   2.51   4.08
  12       0-11   59.24 s  81.97   810.179 G      325.00 G   2.49   3.94
  12       0-11   58.92 s  81.64   810.111 G      324.92 G   2.49   3.95
  16        -     59.54 s  88.43   810.282 G      348.88 G   2.32   3.92
  24        -     59.72 s  98.96   810.414 G      388.89 G   2.08   3.91
  24        -     58.42 s  97.30   810.413 G      382.88 G   2.12   3.91
```

Rule 3: every row is the r=16 square at (64,64), seed 20260722, **1089
delivered chunks** (1369 generated), commit `2512eaf`, `worldgen-dump`.

**The instruction count is identical to within 0.2% at every width.** Twelve
or twenty-four threads do not execute more instructions than one; there is no
extra *work* for an arena, a per-worker buffer pool or a false-sharing fix to
remove. The three cheapest-first candidates the owner named are cleared by
one measurement, which is rule 7 in the direction it is usually needed.

What differs is **cycles**. The bench box is a Threadripper 1920X: twelve
cores, twenty-four *hardware threads*. `available_parallelism` counts threads,
so the pool was filling both SMT siblings of every core. Two threads on one
core's execution units each retire fewer instructions per cycle — IPC 2.51 at
one thread, 2.08 at twenty-four — and the kernel bills both threads' seconds:

```text
  12 threads on 12 distinct cores   +0.8% cycles    (325.0 vs 322.4 G)
  24 threads on 12 cores           +19.7% cycles    (385.9 vs 322.4 G)
  clock, 1 thread -> all-core       -4%             (4.08 -> 3.91 GHz)
```

`0.8% + 19.7%` of cycles plus 4% of clock is the whole of the "22%". The
cost of *parallelism* is 0.8%. The rest is a hardware thread being counted as
if it were a core.

### And SMT buys 0.6% of wall for 17 CPU-seconds

`RUSTMC_STAGE_TIMES=1`, the noise stage's own wall time, same square:

```text
  threads   noise stage   speedup   whole run
   1          22.69 s       1.0x     80.26 s
  12           2.13 s      10.7x     59.35 s     unpinned
  12           2.13 s      10.7x     59.86 s     pinned 0-11
  24           1.74 s      13.0x     59.40 s
```

SMT is worth 0.39 s of a 59 s run. It costs ~17 CPU-s. On a target defined in
CPU-s per chunk that is a straight loss, so the default is now **one worker
per physical core**, read from `/sys/devices/system/cpu/cpu*/topology/
thread_siblings_list` and clamped to `available_parallelism` so a cpuset still
wins. `RUSTMC_WORKERS` overrides it, and 24 is one env var away the day the
wall matters more than the CPU.

**Pinning is not required**, and that was worth checking rather than
assuming: 12 threads pinned to cores 0-11 and 12 threads left to the
scheduler both take 2.13 s in the stage. A 12-thread default that needed
`taskset` to beat a 24-thread one would have been a regression everywhere it
is not applied.

### The exchange rate, for when the parallel fraction grows

SMT's wall value here is small **because the parallel fraction is small** —
one stage of six, and it is already down to 2 s of a 59 s run. On the stage
itself SMT is worth 22% (2.13 s -> 1.74 s) for 19.7% more cycles. So the
honest statement of the trade is:

```text
  today, 29% of the pipeline parallel:  SMT = +0.6% wall for +19% CPU-s
  hypothetically, 100% parallel:        SMT = ~+18% wall for +19% CPU-s
```

Which is roughly break-even, and a decision to re-take when more stages are
parallel — not a decision to inherit from this paragraph.

### `block_name` was 134 million allocations

Found while looking for the allocator cost that turned out not to exist.
`anvil_bridge::block_name` returned a `String`; every name it can return is
`'static` or the caller's own `default_block`. The noise stage calls it
98,304 times a chunk and discards most of the results immediately, because it
allocates `"minecraft:air"` only to compare it and skip.

```text
  build                 wall     CPU-s   instructions   cycles
  2512eaf, 1 worker    78.99 s   78.54   808.786 G      322.42 G
  borrowed, 1 worker   77.85 s   77.44   784.930 G      316.91 G
  borrowed, 1 worker   78.91 s   78.49   784.942 G      317.94 G
                                         -2.95%         -1.6%
```

Wall and CPU-s on this box vary ~1.5% run to run, which is the size of the
win, so they cannot carry it; the instruction count is stable to five digits
and says 23.9 G instructions are gone. Cycles fall less than instructions
because those allocations were tcache-warm — this is 3% of *work* removed,
not 3% of time.

### Where the two commits leave the number

```text
                                wall      CPU-s   CPU-s/chunk   region files
  2512eaf, 24 threads          59.72 s    98.96     0.0909      4/4 identical
  HEAD, default (12 physical)  59.06 s    80.09     0.0735      4/4 identical
                                          -19.1%    -19.1%
```

1089 delivered chunks over the r=16 square at (64,64), seed 20260722. Every
dump `cmp`-identical to a pristine-`2512eaf` **sequential** dump of the same
square, at N = 1, 12, 16 and 24 workers and with the `block_name` change in
and out. **The efficiency target of <=0.075 CPU-s/chunk is met on this
harness** — and note rule 3 again: this is the `worldgen-dump` harness, not
`ci/bench-servers.sh`, whose 0.1224 is a different measurement of a different
thing and must not be divided into these.

### The surface walk read every block through a `String` (2026-07-29)

`BlockColumn::get` returned an owned `BlockState(String)` and `build_column`
calls it for every block of every column plus a lookahead scan per stone run,
only to compare against three literals. Measured against a **pristine
`7faf942` control built in the same session**, because another track's merge
moved the control by 4.5% between two of this track's dumps:

```text
  build                    wall     CPU-s   instructions   cycles     IPC
  7faf942, 1 worker       78.87 s   78.43   784.840 G      320.20 G   2.45
  b9491f2, 1 worker       77.42 s   76.98   764.225 G      311.30 G   2.45
                                    -1.85%  -2.63%         -2.78%

  7faf942, default (12)   60.56 s   83.91   785.118 G      330.90 G   2.37
  b9491f2, default (12)   61.34 s   82.21   764.540 G      319.28 G   2.39
                                    -2.03%  -2.62%         -3.51%
```

The same commit replaced `split_once('[')` in the two fluid checks with a
byte scan. `CharSearcher::next_match` — the UTF-8 machinery behind a `char`
pattern — was **1.49% of the whole pipeline**, spent finding a `[` in an
ASCII block name.

4 of 4 region files `cmp`-identical to the pristine `7faf942` sequential
dump, at 1 worker and at 12.

### The surface stage's parallel blocker, measured (2026-07-29)

`RUSTMC_SURFACE_TIES=1` reports what the surface stage does to the shared
`ParameterList`, whose `find_value` mutates a search hint and resolves exact
ties by call order. Two seeds, r=16 square, 1369 generated chunks:

```text
  seed 20260722   fallbacks 123797 of 39515420 queries (0.3133%)
                  the hint changed    1 answer
                  9716 fallbacks from a chunk with all nine neighbours
                  1362 of 1369 chunks reached the shared list  (99.5%)
  seed 31415926   fallbacks 118921 of 38794216 queries (0.3065%)
                  the hint changed    4 answers
                  9782 interior fallbacks
                  1361 of 1369 chunks reached the shared list  (99.4%)
```

Three plans die in that order: the fallback is not zero, so the list is live;
it is not confined to the perimeter, so a geometric split does not isolate
it; and the hint is not inert, so reordering the calls can change a block.
The scheme that survives is speculate-validate-apply, written out in
WORKLIST's `## NOW`.

### The biome fuzz reuses its cell's corners, and one memo that did not pay

`quart_for_block` runs once per biome query — 39,515,420 times over the r=16
square, 4.88% of the profile — and evaluates eight corners of nine `lcg_next`
rounds each. Those rounds are a pure function of `(seed, corner)`; `dx`, `dy`
and `dz` enter only in the distance arithmetic afterwards. All 64 blocks of a
4x4x4 cell therefore want the same eight perturbations.

```text
  build                          instructions   cycles      note
  b9491f2, 1 worker              764.225 G      311.298 G   control
  corner memo, 2048 slots        761.155 G      311.443 G   -0.40% / -0.06%
  cell memo, one entry           749.185 G      305.562 G   -1.97% / -1.84%
```

**The first shape is the finding.** A corner-keyed direct-mapped table did
nothing: eight thread-local accesses, eight tag checks and a multiplicative
hash per query cost as much as the nine multiplies they skipped. Memoising
the *cell's* eight corners instead — one lookup per query rather than eight,
one entry deep, because a column walk asks four consecutive `y` from the same
cell — is the same idea at a granularity where the lookup is amortised. At
this scale a memo only pays when its lookup is amortised over the whole batch
it serves.

4 of 4 region files `cmp`-identical to the pristine `7faf942` sequential dump,
at 1 worker (twice) and at 12.

### The per-stage parallel-safety table, as it stands

```text
  stage       share   writes                       parallel?
  noise       28.6%   own chunk, before wg_live    DONE
  surface     30.0%   own chunk, AFTER wg_live     needs the wg question
                      + find_value fallback at       answered first
                      the area edge is order-dep
  biomes      19.7%   own chunk, but find_value    NOT SAFE: mutates a
                      mutates a shared search hint   shared hint, and the
                                                     standing 3-block red
                                                     IS a climate tie
  decoration  15.5%   writes across borders        only under the nine-order
                                                     RUSTMC_DECORATE_ORDER
                                                     sweep (WORKLIST, owner
                                                     decision 2026-07-29)
  carvers      6.1%   writes across borders        no
```

## The noise-stage benchmark, and how to reproduce it

```
cargo build --release -p worldgen-dump
./target/release/worldgen-dump bench <seed> <chunks>     # rustmc
./ci/bench-vanilla.sh <seed> <chunks> <warmup>           # vanilla 26.2
```

Both report the **noise stage only**: density, cell interpolation, aquifer,
ore veins, block fill. No surfaces, carvers, features or structures, and no
disk IO on either side. Comparing this against vanilla's *full* chunk
generation would flatter us by a large factor, so don't.

Two method points that change the numbers:

- The Java side warms up before timing (32 chunks by default). Without it the
  first chunks run interpreted and vanilla looks roughly twice as slow.
- `VanillaBench` is declared in `net.minecraft.world.level.levelgen` rather
  than using reflection, because a reflective call per block times the
  reflection. The JVM refuses to mix an unsigned class into a signed package,
  so `ci/bench-vanilla.sh` runs it against a signature-stripped copy of the
  jar. Same bytecode; the signature only certifies provenance.

## Results

Seed 20260725, 64 chunks, single-threaded, all after the flat-cache work.

| machine | vanilla | rustmc | ratio |
|---|---|---|---|
| Threadripper 1920X (12c/24t) | 16.64 ms | 24.39 ms | **1.47x slower** |
| i7-5500U (2c/4t, 15 W) | 48.25 ms | ~40 ms | ~0.8x (faster) |

The Threadripper row is the mean of **three pairs measured back to back in
one session** (2026-07-25 10:38), rather than the single pair it used to be:

| rep | vanilla | rustmc |
|---|---|---|
| 1 | 16.74 | 24.11 |
| 2 | 16.91 | 24.71 |
| 3 | 16.28 | 24.36 |

That row previously read `17.80 / 23.95 = 1.35x`. Both numbers moved, and the
one that moved further is vanilla's: **the JVM this file was measured against
was not always the same one.** The box's `java` alternative was on JDK 21
today and the harness would not even compile against the 26.2 jar, which
needs 25; it is pinned to 25 now. Whether the earlier 17.80 came from a
different JDK or from ordinary run-to-run spread is not established, and the
honest reading is that a single unpaired measurement was never worth a
two-decimal-place ratio.

So: **still slower than vanilla on the fast machine, and by slightly more
than previously claimed.** The laptop row is unchanged and still the other
way round, which is the more interesting fact of the two — see the caveat on
it below before leaning on it.

### Progress

| change | laptop | threadripper |
|---|---|---|
| first measurement | 100.24 ms | 63.98 ms |
| live flat caches, flat lattice, cell reuse | 63.49 ms | 33.56 ms |
| cacheOnce with construction-time slots | ~61 ms | 32.75 ms |
| lazy ore-vein sampling | ~49 ms | 27.68 ms |
| roundsd instead of libm floor; stored operand bounds | ~40 ms | 23.95 ms |

Threadripper ratio against vanilla: **3.73x -> 1.47x**, i.e. **2.7x faster**
in absolute terms, with parity unchanged at every step. (The absolute
speedup is the trustworthy half of that sentence: it is rustmc measured
against rustmc on one machine. The ratio depends on which JVM vanilla ran
under, and has moved by 0.12 on that account alone.)

Parity was 100.000000% over 1,769,472 blocks before and after every entry in
this table. That is the point of doing it in this order: the gate makes an
optimisation either obviously safe or obviously wrong, immediately.

### A note on `cacheOnce`, since it nearly did nothing

The first attempt at `cacheOnce` assigned cache slots with a tree walk, the
same way interpolators are numbered, and measured **no change at all**.

The reason is worth keeping. In vanilla `slopedCheese` is one object
referenced three times, and `NoiseChunk.wrap` maps it through an
identity-keyed map to one cache instance — so all three reads share it. Our
graph is a tree, so those three references are three *cloned subtrees*.
Numbering them afterwards gives each its own slot, every read misses, and
the memoization is pure overhead.

Slots are therefore fixed at construction, before the clones happen. The
lesson generalises: any optimisation that depends on two references being
"the same function" has to be expressed where the sharing still exists.

### Where worldgen parity stands (2026-07-25)

| gate | scope | result |
|---|---|---|
| `golden_noise_chunk` | 2,064,384 blocks, 21 chunks, 3 seeds | 100.000000% |
| `golden_surface_chunk` | 2,555,904 blocks, full **block states** | 100.000000% |
| `golden_carver_chunk` | 2,555,904 blocks, full **block states** | 100.000000% |
| `golden_feature_order` | 168 features in 11 steps, 55 biomes | 100.000000% |
| `golden_ore` | 1,980 cases, 94,541 blocks, 30 configs | 100.000000% |
| `golden_decorate` | **all 11 steps**, 169 attempts, 17,454 blocks | 100.000000% |
| `golden_named_noise` | 63 noises x 3 seeds | 100.000000% |
| `golden_mth_sin` | 65,536 table entries + 15 call sites | 100.000000% |
| `golden_temperature` | 804 noise samples, 31,548 lookups, 66 biomes | 100.000000% |
| `golden_climate` | 3,375 ordered lookups | 100.000000% |
| `golden_biome_zoom` | 512 positions | 100.000000% |

Two coverage numbers sit alongside them, because a percentage says how often
the port agrees on what it was asked and not how much it was asked:

- **rule output coverage 26/26.** The surface gate's chunks exercise every
  block the overworld rule can place. Ratcheted, so it fails if it drops.

  It started at 10/26 with three chunks near spawn, which measure whatever
  those three chunks happen to contain. Each addition since was located by a
  search — a badlands for the terracotta stripes and red sand, a mushroom
  island for mycelium, a snowy peak for snow and ice, a mangrove swamp for
  mud, a stony peak for calcite, a grove for powder snow, an old-growth pine
  taiga for podzol, a wooded badlands for coarse dirt, and one chunk for
  `red_sandstone`, whose guard wants a block that is the top *and* the bottom
  of a stone run at once — a one-block ledge with air above and below.

  **Every one of those blocks was bit-exact on first measurement.** The port
  has not needed a fix for anything a new chunk introduced, across sixteen
  chunks added. That is the outcome an oracle-chosen chunk set exists to
  establish rather than assume.

  Two searches, not one, was the lesson. `--find` generates a chunk per
  candidate and answers "does this chunk contain block X"; `locate_biome`
  reads only the biome source and answers "where is biome Y". Asking them as
  one question cost fifty minutes generating chunks of
  `old_growth_birch_forest`, which a substring matched and which places no
  podzol at all.

- **ore branch coverage: 804 veins ran, 1,176 returned early, 161 drew for the
  air-exposure discard, 84 short-circuited it.** All four ratcheted. The last
  pair is the one worth having: seven of the thirty ore configs carry a
  discard chance, but two carry `1.0`, and `shouldSkipAirCheck` returns true
  at `>= 1` **without drawing**. Those eighty-four cases assert that the
  short-circuit happens — a port that drew there would place the same ore and
  desynchronise every feature after it.

  Like the surface set, **every one of the thirty configs was bit-exact on
  first measurement.**

  The gate was then mutation-tested, which is where it earned the rest of its
  case set. Dropping the swallowed-sphere deletion pass failed 2 cases;
  dropping the tested-bitset failed 20. Substituting a recomputed
  `OCEAN_FLOOR_WG` for the shipped one failed **none** — the probe that reads
  it is a loose inequality, and every origin sat far enough from the terrain
  that a 25-block error in the input moved no output. Origins at the terrain's
  own height were added until that mutation failed too; it now fails 68 of
  1,980. That is where 540 of the cases came from. See ORACLE.md 5b rule 9.

- **decoration coverage: computed 165 of 169 attempts (97.6%)** across all
  eleven steps, from 88 at the start of 2026-07-24 and 124 at its end. Every
  attempt is the port's own output except four: three `freeze_top_layer`,
  which is implemented and deliberately switched off until the ten steps
  beneath it compute rather than replay, and one tree selector, whose children
  need the unported tree feature.

  Twenty-two of the 165 are chains that produced **no position**, so the
  feature was never called and its being unported cost nothing. They are
  counted as computed — the port produced vanilla's result — and reported
  apart, so the headline cannot quietly borrow against work not done.

  Three features port the *survey* and refuse if it passes: `monster_room`,
  `blue_ice` and the carve phases they share a shape with need block entities
  and loot tables. A refusal is loud rather than silent — the gate compares
  both blocks and the draw count, and every unported body writes hundreds of
  the first and tens of the second.

## Whole-world parity (2026-07-26)

**The headline number is the wide one, and it now holds at two seeds.**

    seed 20260722   EXACTNESS 99.982785%   2,860 mismatches
    seed 31415926   EXACTNESS 99.993228%   1,125 mismatches

    where this stood earlier today:
      start          99.971047% / 4,810      99.975815% / 4,018
      mineshaft order 99.975050% / 4,145     99.986836% / 2,187
      magma nesting   99.979950% / 3,331     99.989256% / 1,785
      mineshaft blocks 99.982785% / 2,860    99.993228% / 1,125

Two fixes, 2,479 blocks between them, and both were the same kind of
mistake: a container walked in the wrong order. Mineshaft starts came out of
a fastutil `LongOpenHashSet` and we walked our own scan order;
`underwater_magma`'s box came out of `BlockPos.betweenClosedStream` and we
nested it inside out. Neither changed *how much* of anything was placed —
only where — which is why both hid behind block counts that looked roughly
right.

    each: 169 chunks, 16,613,376 blocks, biomes 259,584 / 0 mismatches

## What "parity with vanilla" can mean at all

The oracle world is generated by forceloading a square and letting the
server's chunk system decide the order chunks reach `FEATURES`. Changing
*only* that — same seed, same centre, same radius, same jar, same settings,
one `forceload add` over the square against fifteen issued a row at a time:

```text
  DIFF chunks=169  blocks=16,613,376  mismatches=16,315
  EXACTNESS 99.901796%
```

**16,315 blocks, from nothing but the order the chunks were asked for.** That
is five times our entire gap to the reference world.

So vanilla's output is not a function of the seed. It is a function of the
seed *and the order chunks are generated in*, because a feature writes one
chunk out and whichever of two neighbours goes first wins the overlap. Two
vanilla servers given the same seed and asked for chunks differently produce
measurably different worlds.

### The consequences, in order of how much they matter

1. **A whole-world parity figure is only defined relative to a generation
   order.** "99.98% against vanilla" means "against *this* vanilla world".
   Quoted without that, it is not wrong so much as underspecified.
2. **The ceiling is ~99.90% at this seed** for any implementation whose chunk
   order differs from the reference's, however perfect its features. We are
   at 99.98% against the square-forceload world, which is *better* than the
   spread between two vanilla orderings — meaning our raster order is very
   close to what the square forceload happens to produce, and the remaining
   gap is smaller than the ambiguity in the target.
3. **The order-sensitive share is measurable and small.** 16,315 of 16.6M is
   0.098%. Everything else — terrain, biomes, carvers, surfaces, the bulk of
   decoration — is order-independent and can be exactly right.

### What this does not excuse

It is not a licence to stop. The order-sensitive blocks are a *bound on the
measurement*, not on the port: within a fixed order, every one of them still
has a right answer, and this project has fixed several today. What changes is
which numbers deserve trust:

- Stage gates (noise, surface, carvers) are unaffected — they compare one
  chunk generated one way, with no cross-chunk ordering involved. Those are
  still the numbers to hold to 100%.
- Whole-world figures should be quoted with the floor beside them, and a
  change worth less than that floor at a given seed cannot be told from a
  change of generation order.

### Tried: driving the order does not make the oracle stable

`GEN_ORDER=rows` forceloads one chunk row at a time instead of the square in
one command, on the theory that a driven order would make the oracle
reproducible across extents and collapse the floor. Both worlds generated on
the same machine so the scheduler is not a variable. Seed 20260722, over the
window both share:

```text
  rows(64,64)    vs  rows(62,62)      7,397 mismatches
  square(64,64)  vs  square(62,62)    4,885
```

**It made the oracle less stable, not more.** Forceloading a row still leaves
the chunk system free to order the work within and across rows, and where the
row boundaries fall relative to the square changes with the centre.

So the floor is not an artifact of *how* we ask for the chunks — it is
intrinsic to how the server schedules generation, and it cannot be driven out
from the client side. The square forceload stays the reference.

One more number from the same run, which is the useful one:

```text
  ours  vs  square(64,64)    3,076 mismatches    99.9741%
  ours  vs  rows(64,64)      7,085
  square(64,64) vs square(62,62)   4,885         99.9589%
```

**We are closer to the reference world than a second vanilla world is.** Our
raster decoration order resembles what the square forceload produces closely
enough to beat vanilla's own cross-extent reproducibility, and the gap to the
rows world (7,085) is roughly the gap between the two orderings themselves —
which is what it should be if our order matches one of them and not the other.

That is the honest ceiling statement for this seed: the port is inside the
noise of the oracle, and further whole-world work here is measuring the
harness rather than the generator.

## Corrected: the artifact split was computed on four fifths of the data

The split below was read from `region-differ`'s JSON `by_block` array, which
was **capped at thirty pairs**. Over a whole-window diff those thirty
routinely account for about 80% of the mismatches, so roughly a fifth of
every such analysis in this file was invisible — and the totals not adding up
was there to be noticed and was not.

That is how the mineshaft got called order-independent. Two vanilla worlds of
seed 20260722 differing only in generation order disagree on **52
mineshaft-only blocks**; the "scores zero between the two vanilla worlds"
claim came from a table where the tail had been cut off.

The JSON now emits **every** pair. The text output stays capped, because a
human is reading that and a script is not. Re-taken over the complete list:

```text
  in pairs vanilla also disagrees with itself on:  2,536
  in pairs it does not:                              137
```

The conclusion holds and is slightly stronger than the truncated version
suggested — but it was luck that the direction survived, not method.

## Mineshaft blocks: from ~380 to 1

The largest real cluster in this file is closed. Seed 31415926, inner-169
window:

```text
  deepslate <-> cave_air   87/86  ->  0/0
  cobweb    <-> cave_air   68/61  ->  0/1
  stone     <-> cave_air   41/39  ->  0/0
```

Counted exhaustively rather than off the top-N table, 19 blocks remain in
that area and **16 are `monster_room`**, not the mineshaft: nine chest facings
where vanilla `reorient`s and that feature hardcodes `facing=north`, and six
where a dungeon's cobblestone overwrote a support ours kept. **Three blocks
are actually this stage**, against a measured floor of 52.

The eight corridors this file called "unexplainable" were refusals, and are
now measured from two independent instruments rather than assumed: vanilla's
own blocks (an all-rock ceiling means the 0.8 pass never ran — probability
0.2^45, about 3e-32) and our code path. Eight refused, ours refused all eight,
zero unexplainable.

A new `Entrances` gate covers something no draw-count instrument could see:
the room hollows its child-entrance boxes taking **zero** draws, so a wrong
one is invisible to every existing check. 100.000000% over 5 worlds, 18 rooms,
52 boxes.

## A third window, chosen on purpose: 99.41%

Both r7 windows were picked for terrain and are ocean-ish. Vanilla's own
`/locate` was used to pick a third deliberately — a plains village at seed
20260722, chunk (-53, -33) — and the number there is not the headline
number:

```text
  seed 20260722, village window, inner 169 chunks
  DIFF blocks=16,613,376  mismatches=97,905
  EXACTNESS 99.410686%
```

Where that window went during the same session, and the last row is the one
worth reading:

```text
  as found                      99.410686%   97,905
  + canSurvive(snow)            99.525364%   78,853
  + fancy tree placers          99.542610%   75,988
  + a refused tree writes nothing  99.496207%   83,697
  + HashMap treeification          99.573175%   70,910
```

**The last row is what the row above it was owed.** All 188 refusals were
spruces — "straight trunk, spruce foliage" — refused because
`java_hash_set` declined a bucket of eight rather than treeify it, and a
spruce's foliage set is the one that reaches eight where oak and birch do
not. Porting `java.util.HashMap`'s red-black path gave 12,787 blocks back and
took the refusal count to zero.

Three things the bytecode said that the analysis of it had not: the trigger
is the ninth insert, not the eighth, because `binCount` starts at zero on the
head and the test is `>= 7`; below `MIN_TREEIFY_CAPACITY` `treeifyBin`
resizes and returns, so one insert can resize twice; and the iteration order
after a treeify is "root first, then the old order" *only for the treeify
itself* — `putTreeVal` splices each later entry in after its tree parent and
moves the root again, so the full insert path is required and identifying the
root is not enough.

**The row before that made the number worse and was kept.** A refused tree used
to leave its logs and leaves behind; a half-built spruce overlaps a real one,
so those blocks scored against vanilla's tree while being a shape that exists
in no vanilla world. Removing the fabrication removed the coincidental credit
with it.

So `99.5426%` was **overstated by about 7,709 blocks**, and 99.4962% is the
real position. Keeping the better-scoring variant would have been fitting the
mechanism to the metric — the same error as reading a stage's health off a
thirty-row table, made twice in one day and caught the second time.

**Against 99.98% on the two windows this project has been quoting.** Same
seed, same generator, same day. The difference is entirely what is in range.

### It would not generate at all until an hour ago

The first attempt died: `java_hash_set` refuses when a bucket reaches eight
entries, because `java.util.HashMap` would treeify it and this port cannot
reproduce the resulting order — a designed limit, and one this project counts
rather than hides. Three sites in the tree code had turned it into a
`.expect`, so the refusal was a **panic**.

That was invisible for as long as every window was ocean. The measurement
this project could take was partly a function of where its generator does not
crash, which is worth knowing on its own.

### What the window actually exposes

The refusal counter, now that it counts:

```text
  199x  a bucket reached 8 entries — HashMap would treeify it
  132x  canSurvive(snow)
   92x  fancy_trunk_placer + fancy_foliage_placer trees
   12x  fallen_tree
    3x  canSurvive(peony)
```

`fancy_trunk_placer` is the large branching oak. Ninety-two refusals of it,
plus 199 treeify refusals concentrated in dense foliage, is most of 97,905
blocks — and none of it appears in an ocean.

### What this means for the headline

The launch bar is 99.9%+ whole-world parity. **Two windows clear it and a
third does not**, and the third was chosen deliberately rather than found by
luck. The honest statement of where this project is:

- Terrain, biomes, carvers, surfaces and the ore/aquatic/mineshaft features
  are at 99.98% and inside the oracle's own noise on the windows measured.
- Land vegetation — the fancy tree in particular — is **unmeasured and
  substantially wrong**, because no window that contains it was ever run.
- The headline needs a third seed quoted beside the other two, permanently.
  A number that improves when you stop looking at hard terrain is not a
  parity number.

### Snow first, because the breakdown said so

The confusion table for that window, rather than the refusal counter:

```text
  14,513  snow[layers=1]         -> air
   8,053  grass_block[snowy=true] -> [snowy=false]
   4,921  oak_leaves[distance=3]  -> air        (and both directions, many rows)
   2,627  spruce_leaves[distance=2] -> air
```

**Snow was 23% of the window** and the trees were the rest. `canSurvive` for
`SnowLayerBlock` is two tag tests then a collision-shape test, all of it
read from the bytecode, and the order is load-bearing: a block in both tags
is refused, so testing the overriding tag first would place snow on it.

```text
  village window, seed 20260722
    before   99.410686%   97,905 mismatches
    after    99.525364%   78,853
```

**19,052 blocks**, and the snow refusals are gone. What is left there is
vegetation: the leaves rows above, and the `fancy_trunk_placer` /
`fancy_foliage_placer` trees that are still 92 refusals.

That diagnosis has since been confirmed from a completely different
direction. The chunk-packet work decoded **vanilla's own network packet** for
chunk (0, 0) to state ids and diffed it against our generated chunk:
**99.564616%**, 428 of 98,304 cells, and *every one of them vegetation* —
oak and birch leaves, oak logs, peony. No terrain, no structure. Two
independent instruments, a region diff and a network capture, agree on both
the size and the shape of what is left.



## The missing structures, measured rather than listed

This port generates **one** of the twenty structure sets in the datapack:
mineshafts. That sounds like nineteen gaps. Measured against what vanilla
actually placed in the two r7 windows, it is three instances:

```text
  seed 20260722   3x mineshaft, 1x shipwreck
  seed 31415926   4x mineshaft, 1x buried_treasure, 1x ocean_ruin_warm
```

And attributing every mismatch position to the box it falls inside — which
`region-differ --positions` exists for — gives the cost of each:

```text
  shipwreck        (20260722)   374 of 4,204 mismatches   8.9%
  ocean_ruin_warm  (31415926)    33 of 2,460             1.3%
  buried_treasure  (31415926)     0                       one block, outside the window
```

**The shipwreck is the largest identifiable cluster left at seed 20260722**,
and the whole rest of the nineteen sets is worth 33 blocks across both
windows. That is the opposite of what a list of nineteen missing features
suggests, and it is why this is measured rather than assumed: villages,
strongholds and ocean monuments are enormous features that simply are not in
range of either sample.

Two consequences. **The next structure to build is the shipwreck**, not
whichever is most famous. And **the r7 windows say almost nothing about
structure parity in general** — a third seed chosen for a village or a
stronghold would measure something these two cannot.

### RETRACTED: there is no mesa mineshaft, and the dark oak is a shipwreck

The section below is wrong and is kept because the *way* it was wrong is the
lesson. The filter that produced "76 mineshaft blocks are dark oak" matched
`oak_planks` and `oak_fence` — which are **substrings** of `dark_oak_planks`
and `dark_oak_fence`. A shipwreck was counted as a mineshaft, and a task was
set and worked on off the back of it.

`worldgen-dump mineshaft-mesa` reads the structure starts vanilla actually
saved. Across all five oracle worlds there are **zero** mesa mineshafts, and
**47 of 47** sampled dark-oak positions lie inside one shipwreck's bounding
box:

```text
  vanilla mesa mineshafts in oracle-work/vanilla-r7: 0
  structure starts saved in this world, by id:
        3x minecraft:mineshaft
        1x minecraft:shipwreck
    minecraft:shipwreck start (57, 65) union 912,54,1040..920,62,1063
```

Corrected residue, whole-name matching, shipwreck excluded:

```text
  seed 31415926   19 mineshaft-only ->  3 after removing monster_room
  seed 20260722   30 (not 106)      -> ~18 after removing monster_room
  order-only floor between two vanilla worlds:            52
```

**Both seeds are far under the floor. The mineshaft is done.** A substring
match is how a finished stage looked like it had 106 blocks outstanding.

The dark oak belongs to a real gap of a different shape: **shipwrecks are one
of nineteen structure sets this port does not generate at all**, exactly as
`full`'s own caveats say. That is a missing feature list, not a wrong
implementation, and it is the honest next target.

### Superseded: "a real gap it found: no mesa mineshafts"

`mineshaft_starts` only opens the structure set with `MineshaftType::Normal`,
and `generate` refuses `Mesa` by name. **76 of seed 20260722's 106
mineshaft-attributable blocks** are `dark_oak_planks` and `dark_oak_fence`
that vanilla places and we leave as bare terrain. That is now the largest
mineshaft cluster anywhere in the diff — a missing feature rather than a
wrong one, diagnosed from the block signature and the code path but **not**
from generating one.

## Seed 20260722: vanilla disagrees with itself more than we disagree with it

A second vanilla world of seed 20260722 was generated at centre (62, 62)
instead of (64, 64), and all three compared over the 121-chunk window both
have fully decorated (58..68):

```text
  vanilla(64,64)  vs  vanilla(62,62)    4,885 mismatches
  ours            vs  vanilla(64,64)    3,076
  ours            vs  vanilla(62,62)    4,811
```

**The two vanilla worlds are further apart than we are from either.** We are
closer to the reference world than vanilla's own second run is.

Splitting our disagreements by whether vanilla also disagrees with itself on
that block pair:

```text
  pair                                    ours   van-vs-van
  cinnabar   -> sulfur                     305          143
  sulfur     -> cinnabar                   204          263
  cave_air   -> sulfur                     179          491
  diorite    -> andesite                   136          461
  water      -> kelp_plant                 118          147
  water      -> seagrass                   109           93
  ...
  in pairs vanilla also disagrees with itself on:  2,306
  in pairs it does not:                              120
```

At seed 31415926 that split was 450 against 2,223 — a small floor. Here it is
the other way round.

### What this does and does not say

It does **not** say those 2,306 blocks are all artifacts. Sharing a category
with vanilla's self-disagreement is not the same as being the same blocks.
What it says is that essentially every category we still lose at this seed is
one vanilla's own two runs also lose, at comparable or larger volume — so the
*expected value* of more feature work there is low until the categories
themselves are made order-stable.

It also explains why the two seeds diverged so sharply. Seed 20260722's
window is dominated by the sulfur caves, and sulfur lakes are the most
order-sensitive thing in the generator: Worker C showed a lake dug by one
chunk changes which origins the next chunk's `block_predicate_filter`
accepts. Change the generation extent and the whole complex reshuffles. Seed
31415926 has no such structure in range, and its floor is 637.

And it independently corroborates both workers' verdicts. Worker B measured
seagrass and kelp exact against the harness and 92.45% against a server, and
concluded they amplify upstream differences rather than causing them; Worker
C measured the sulfur pool correct at the two chunks under investigation and
found the harness wrong at a third. Both said "do not fix toward the diff".
This table is why they were right.

### The honest consequence for the headline

`99.979950%` at seed 20260722 is a real measurement against a real vanilla
world, and it is **not** a measurement of how much of the generator is wrong.
A second vanilla world scores 99.958%, against the same reference, by the
same method. Quote both seeds, quote the floor beside them, and treat any
change at this seed worth less than a few hundred blocks as unmeasurable
until the oracle is made order-stable.

### The window is comparable, checked

A worker reported that `vanilla-r7` "only has chunks 55..64 fully generated",
which if true would mean a third of the quoted window is not comparable and
the headline is wrong. **It does not hold for this world**, and it is written
down because an unchecked claim of that shape would silently invalidate every
number in this file.

`gen-vanilla-region.sh` forceloads `(CX - R)` to `(CX + R)`, so centre 64 at
r7 covers chunks **57 to 71**. Scanning the world back:

```text
  SCAN full=169  bbox=58,58 -> 70,70      every chunk in the window is full
  SCAN full=1    bbox=71,71 -> 71,71      the outer ring exists
```

So all 169 chunks quoted have their eight neighbours present and decorated,
which is exactly why the figure is quoted over 169 of the 225 and not more.
The worker was describing a world generated at a different centre.

The block figures are quoted over 169 chunks rather than the 225 generated,
because decoration reaches one chunk out: the outer ring is missing whatever
its ungenerated neighbours would have written into it, and scoring it would
flatter us. **Biomes have no such dependency**, so those are quoted over the
full 225: 345,600 cells, 0 wrong, at both seeds.

One seed is one place, and every number in this file rested on one until now.
Two seeds a thousandth of a percentage point apart is much harder to explain
by luck than a single figure is.

The second seed earned its cost on the first run: it is the only one with
**lush caves** in range, and `vegetation_patch` was unported — 850 blocks of
`moss_block` against `deepslate` and 33 refusals, none of which seed 20260722
can see at all. Porting it, and then the four `canSurvive` rules for the plants that grow on
it, took that seed from 99.969242% to **99.974551%** — 882 blocks — while
seed 20260722 did not move by a single block, because it has no lush caves to
be right or wrong about.

Seed 31415926 is now down to **one** refusal, and it is an honest one: a
`java.util.HashMap` bucket reaching eight entries, which vanilla treeifies
and `java_hash_set` does not reproduce.

That is the argument for a second seed in one line: a whole feature was
missing, the first sample had no way to say so, and fixing it was invisible
there too.

Seed 20260722, centre chunk (64, 64); rustmc generated through every stage,
the vanilla side from `ci/gen-vanilla-region.sh` at the same seed and centre.

The r2 (9-chunk) figure this project worked to for a long time was
**99.979203%**, and it was optimistic — not because it was wrong, but
because nine chunks is one place. Widening to 169 chunks cost 0.017
percentage points and exposed five faults the small sample could not
express, three of which are now fixed:

| found only at r7 | status |
|---|---|
| 336 biome cells wrong (r2 said zero) | **fixed** — the fill visited cells in the wrong order on all three axes |
| 277 blocks of missing acacia leaves | **fixed** — forking trunk + acacia foliage placers ported |
| 63 potent_sulfur blocks entirely absent | **fixed** — the block has no `canSurvive` override, so the answer is an unconditional yes |
| 672 cinnabar/sulfur, 195 sand/dirt, 433 carver | open |

The comparison this invites: the inherited Minecom figure is **99.381792%
at r18**. r7 is not r18, but it is the same order of denominator, so
99.9660% against 99.3818% is a real gap rather than a sampling artefact.
r18 needs row-batching in `gen-vanilla-region.sh`, which caps at radius 7 on
vanilla's 256-chunk forceload limit, and is the number that settles it.

### Vanilla reproduces itself here

Two independent vanilla generations of the same region agree on all 259,584
biome cells and were used to prove the biome fault was ours rather than an
oracle fault. Worth keeping in mind before blaming the reference: at a
climate tie — measured at 0.195% of cells — the answer *is* order-dependent
in vanilla too, it simply happens to be stable for a forceload.

### What the remaining diff is, ordered by measured count

| count | pair | notes |
|---:|---|---|
| 551 | bubble_column ↔ water | known caveat: formed by a sweep, not scheduled ticks |
| 543 | cinnabar ↔ sulfur | speleothem base-layer extent; totals +0.03%, fine-grained |
| 433 | cave_air ↔ deepslate | carver |
| 262/214/207 | tall_seagrass, seagrass, kelp_plant ↔ water | aquatic — positional, not count |
| 244 | cave_air ↔ sulfur | as the sulfur row |
| 225 | air ↔ sulfur_spike | as the sulfur row |
| 202 | air ↔ cave_air | carver |
| 201 | cave_air ↔ cobweb | mineshaft, 103 under / 98 over — near-balanced |
| 136 | andesite ↔ diorite | ore blob |

Two clusters are bounded and known to be fine-grained rather than missing
features — worth recording so neither is mistaken for one:

| block | vanilla | rustmc | delta |
|---|---:|---:|---:|
| sulfur | 798,736 | 798,983 | +247 (0.03%) |
| cinnabar | 689,043 | 688,908 | −135 |
| seagrass | 5,251 | 5,272 | +21 |
| tall_seagrass | 4,518 | 4,510 | −8 |
| kelp_plant | 6,541 | 6,571 | +30 |

Each of those places very nearly the right number of blocks; what differs is
where a small fraction land. Both aquatic chains anchor to `OCEAN_FLOOR_WG`
and then the feature re-reads the **live** `OCEAN_FLOOR` at a jittered
offset, so they are sensitive to both maps at once.

### Heightmaps are written now, and they are a second oracle

Vanilla saves four heightmaps into every chunk. We wrote none, which meant a
real client would be served none *and* that the most direct check on stage
order was unavailable. Both fixed:

| heightmap | wrong / 43,264 | exact |
|---|---:|---:|
| MOTION_BLOCKING_NO_LEAVES | 0 | 100.0000% |
| MOTION_BLOCKING | 12 | 99.9723% |
| WORLD_SURFACE | 14 | 99.9676% |
| OCEAN_FLOOR | 16 | 99.9630% |

These are the maps the canvas **maintained**, not a recomputation from the
finished blocks. The distinction is the whole point: a recomputation is
correct by construction and gates nothing — it would agree with vanilla even
if every `Heightmap#update` here were wrong, because it never reads them.
Features anchor to these maps *during* decoration, long before anything could
recompute, so the maintained values are what actually decide where things go.

Both were tried, and they produce identical files, which says our maintenance
and a from-scratch recompute agree on all 43,264 columns. The 42 that differ
from vanilla are genuine block differences underneath, not bookkeeping.

### The fence rows were a missing pass — now run

144 blocks at seed 31415926 were the same block on both sides with different
properties: vanilla's fence had **no** connections, ours had one. The
opposite of the usual shape — not failing to set something, failing to unset
it, which is a much better clue than the counts are.

`MineShaftCorridor.placeSupport` sets `WEST` and `EAST` explicitly in 26.2,
exactly as this port does; checked in the bytecode rather than assumed,
because "we set a property vanilla doesn't" was the obvious first guess and
it was wrong. What was missing came after: `StructurePiece#placeBlock` marks
the position when the block is in `SHAPE_CHECK_BLOCKS`, and a later pass runs
`Block.updateFromNeighbourShapes` to re-derive it from its six neighbours. A
fence set to `west=true` against a bare cave wall loses it there.

    seed 20260722:  4,941 -> 4,810   99.970259% -> 99.971047%
    seed 31415926:  4,228 -> 4,018   99.974551% -> 99.975815%

Same-block state differences are now **zero at both seeds**. Note this one
moved *both*, unlike the last three fixes — mineshafts are everywhere, and
seed 20260722's share was spread thinly enough across distinct state pairs to
stay out of its top fifteen entirely. It was only visible at all because the
differ compares full states rather than block names.

`PostProcessing` in the region file is **not** a fourth oracle, which is
worth writing down because it looks like one.
`ProtoChunk#postProcessGeneration` applies the marks and then clears them, so
a chunk saved at `minecraft:full` always carries empty lists — checked across
all 169 chunks of seed 31415926 before building anything on it. We write
twenty-four empty lists to match vanilla's shape exactly, and our own marked
set stays in memory where it belongs.

Only fences have a shape rule. `SHAPE_CHECK_BLOCKS` also holds `cave_air`,
`torch`, `wall_torch`, `ladder` and `iron_bars`; those positions are marked
but left exactly as written, because none of them appears in either seed's
diff and a shape rule with no number attached is one nobody can check.

### The mineshaft rows are not the layout

Seed 31415926's largest cluster is mineshaft blocks: 1,031 of them across 40
chunks, concentrated in one region — cobweb 417, rail 255, oak_planks 124,
and part of the 387 `cave_air`/`stone` row. Vanilla has five mines nearby and
two of them overlap the disagreement, so the obvious question is whether we
lay them out in the same place.

We do. `golden_structure_starts.rs` now runs over **three** vanilla worlds
rather than one:

    MINESHAFT LAYOUT 100.000000% over 4,036 pieces (0 wrong)
                     in 28 mineshafts, 0 with the wrong count

That is a six-fold increase on the 659 pieces it checked before, and it
includes seed 31415926's mines specifically. Every piece is the right type,
in the right box, with the right orientation and depth.

So the fault is inside the pieces — which block goes where within a corridor
that is otherwise correct — and not in `generate`, `findCorridorSize`, or the
placement.

And the four block types have one cause, but not the one this file claimed
first. Counting which chunks each disagrees in:

```text
  cobweb      39 chunks        oak_fence    3 chunks
  rail        33 chunks        chest        1 chunk
  oak_planks  32 chunks
  wall_torch  21 chunks
```

Every one of the 33 rail-affected chunks is also cobweb-affected, which was
read here as a draw-stream cascade — one divergence early in
`MineShaftCorridor#postProcess` moving every draw after it. **That reading
was wrong**, and the overlap never supported it: cobwebs and rails live in
the same corridors, so they share chunks under any explanation. Another rule
12 measurement that could not tell two stories apart.

What settles it is reading the predicate instead of what it displaced. Only
two world questions can change the corridor's draw *count*: `isInterior`,
tested before `maybePlaceCobWeb` draws, and `isSupportingBox`, tested before
`placeSupport` draws its `nextInt(4)`. `isSupportingBox` reads the three
blocks at local `(0..2, 3, z)` in each section, and those can be pulled out
of both worlds directly using vanilla's saved `BB` and `O`:

    isSupportingBox inputs compared:  2,798
    isAir disagrees at:                   8   (0.29%)

Eight, and every one of them is a `rail` or an `iron_chain` — mineshaft
output, not carver output. So they are consequences of the disagreement, not
its cause, and the inputs to the draw-count decisions are otherwise
identical.

Which means **the draw stream does not diverge at all**. The cobwebs and
rails differ one position at a time, through the gates that run *after* the
draw — `hasSturdyNeighbours` for a cobweb, air-over-solid for a rail — and
those read the cave walls. So the mineshaft rows are still downstream of the
terrain, but as ~1,000 independent local decisions rather than one cascade,
and they should largely fall out when the carver rows do.

### The carver rows: jitter, and five dead hypotheses

The carver itself is exact where gated. `golden_carver_chunk.rs` runs the
real driver against vanilla: **100.000000% over 2,555,904 blocks**, 16,906 of
them carved.

The disagreement is **boundary jitter**. Excluding mineshaft-placed blocks,
at seed 31415926, 1,458 blocks in:

```text
  870 connected groups, mean 1.7 blocks
  largest group 10; groups of 20 or more: none
  720 of the 870 are one or two blocks
```

The named-pair totals are symmetric to within one block — `cave_air` against
`deepslate` 251/251, against `stone` 194/193 — and this file previously read
that as one displaced cave. **It is not.** Equal counts either way is exactly
what jitter on a surface produces, so the symmetry never distinguished the
two, and the group sizes settle it: a displaced cave is one large group, and
the largest here is ten blocks.

Five candidates measured and dead:

- *Climate ties swap a carver list.* 0 of 961 source chunks at seed
  20260722, 1 at 31415926 — and that one is `river` against `plains`, which
  carry **identical** carver lists, so no index and no seed change.
- *A biome is simply wrong at a source chunk.* Biomes are exact over all
  345,600 cells of the generated area.
- *`OCEAN_FLOOR_WG`.* Gated at 100.000000% over 6,912 columns.
- *The mineshaft rows cascade from it.* `isSupportingBox` has 2,790 of 2,798
  identical inputs; the eight that differ are mineshaft output.
- *One displaced cave.* Refuted above by the group sizes.

**Why the gate disagrees with the diff, and it is structural.** The
whole-world rate is roughly 0.4% of carved blocks; at that rate the gate's
16,906 carved blocks should show around 60 wrong, and it shows none.

The reason is in the gate's own design, and it is not a flaw: it starts from
**vanilla's** post-surface blocks and runs our carver over them, precisely so
that a failure there cannot be a terrain or surface difference in disguise.
The whole-world run starts from *our* blocks. So the gate proves the carver
is correct **given identical input**, and says nothing about whether the
input is identical.

Which relocates the question. Our noise stage is bit-exact and the surface
gate is at 100% — but both of those are measured at seed 20260725, on the
gate's own 3x3 world. There is no equivalent for seeds 20260722 or 31415926,
and a pre-carve terrain difference at those seeds would be invisible in the
final diff wherever a cave later removed the evidence.

That measurement has now been made. `SurfaceChunkGolden` gained seed
31415926 and three chunks from inside the carve disagreement — (60, 68),
(63, 65) and (62, 67), the three with the most differing blocks.

**The three r7 chunks pass.** Their surface stage is exact before any carver
runs, so the terrain the carver is handed there is right and the jitter is
genuinely the carver or the aquifer. The sixth possibility — that the carver
was never involved — is closed.

The same run turned the gate **red** for an unrelated and real reason, which
is the whole point of widening coverage:

```text
  SURFACE CHUNK EXACTNESS 99.912898% over 4,718,592 blocks (4,110 wrong)
    minecraft:packed_ice -> minecraft:water[level=0]   2,375
    minecraft:packed_ice -> minecraft:air              1,718
    minecraft:snow_block -> minecraft:air                 17
```

Every one is at chunk (0, −135), which is a badlands under seed 0 and
something frozen under 31415926 — a biome this gate had never surfaced.
The full condition chain for `packed_ice` is

```text
  above_preliminary_surface AND stone_depth AND water
    AND biome=frozen_peaks AND (steep OR noise(minecraft:packed_ice 0.0..0.2))
```

and three of those are already cleared. We emit `water[level=0]`, which is
the terminal fallback of that same `water` sequence, so the first three
conditions pass. The **biome grid matches at 73,728 quart cells** in the same
run, so `frozen_peaks` resolves correctly. `steep` is a faithful port of
`Steep#test`, checked against the reference. And the `minecraft:packed_ice`
noise is present in the parameter set (`firstOctave` −7), so nothing is
panicking on a missing entry.

The noise is now cleared too. Sampled at the failing chunk's 256 columns,
**209 of 256 fall inside `[-0.5, 0.2]`** — so the band would fire, and the
value is not the problem (`probe_packed_ice.rs`).

Which means the condition is **never reached**, and the fault is in the rule
tree rather than in any leaf. The `water[level=0]` we emit therefore comes
from a *different* branch that also ends in water, not from the fallback of
this one — an easy thing to misread, and it is what made the enclosing
conditions look like they passed. The remaining suspects were the three that guard the branch, with their
datapack parameters:

```text
  above_preliminary_surface
  stone_depth   floor, offset 0, add_surface_depth false, secondary 0
  water         offset -1, surface_depth_multiplier 0, add_stone_depth false
```

`water` is now cleared as well: `WaterCondition#compute` is
`waterHeight == Integer.MIN_VALUE || blockY + (addStoneDepth ? stoneDepthAbove
: 0) >= waterHeight + offset + surfaceDepth * multiplier`, and ours is that
expression exactly. Two left — `above_preliminary_surface`, which is
`blockY >= minSurfaceLevel` over a bilinear 16-block cell, and `stone_depth`,
which with these parameters reduces to `stoneDepthAbove <= 1`.

`above_preliminary_surface` looked strongest — the gate feeds it **our**
`router.preliminary_surface_level`, not vanilla's — so it is now gated
directly. `SurfaceChunkGolden` dumps `NoiseChunk#preliminarySurfaceLevel` per
column, and:

    preliminary surface: 0/12,288 wrong
    surface depth:       0/12,288 wrong

Cleared. `stone_depth` reads correctly too: it gates `add_surface_depth`
exactly as the datapack asks.

**So every condition in the chain is individually right and the branch still
does not fire.** Nine links checked — three enclosing guards, the biome, the
noise parameters, the noise value, the thresholds, `steep`, and now the
preliminary surface. That points away from the leaves entirely and at how the
rule *tree* is composed: a `sequence` that returns early, a `condition` whose
`then_run` is attached to the wrong parent, or a branch dropped at parse
time. Nothing in the current gates would show any of those, because each
produces a legal rule that simply answers differently.

One more thing ruled out: the `frozen_peaks` branch is at **index 0** of its
twelve-element sequence, so no earlier sibling can shadow it by returning
first. Our evaluation is therefore not reaching that sequence at all, and the
fault is *above* it in the tree rather than beside it. The path is

```text
  /sequence[1]/then_run/sequence[2]/then_run/then_run/sequence[1]
```

and the three `then_run` hops on the way in are exactly the three guards
already named — `above_preliminary_surface`, `stone_depth`, `water`. No
hidden outer condition. The parser and evaluator are also right: `sequence`
is `find_map` over its children, `condition` recurses on pass and yields
`None` on failure, which is vanilla's shape exactly.

So the branch is unreachable because one of those three is false, and two of
them are now proved: `minSurfaceLevel` is exact over 12,288 columns and
`WaterCondition#compute` is our expression line for line.

That leaves `stone_depth`, and the interesting part is *which half* of it.
The condition logic reads correctly — it gates `add_surface_depth` as the
datapack asks. But with these parameters it reduces to `stoneDepthAbove <= 1`,
and **`stoneDepthAbove` is context state, not condition logic**: a running
count the caller maintains as it walks a column, which places no block of its
own and which nothing measures.

That is the third value of exactly that shape in this document, after
`minSurfaceLevel` and the aquifer's `computeSubstance`.

**But the reading of `stone_depth` above is wrong**, and vanilla's own dump
says so. Decoding the packed_ice in that chunk: **2,291 of 2,657 have ice
directly above them.** They are stacked in columns, not sitting one per stone
run — so a guard that reduces to `stoneDepthAbove <= 1` cannot be what admits
them, whatever `stoneDepthAbove` counts.

Which means either there is a further `packed_ice` rule this walk has not
found — the two chains printed earlier differ in their `min_threshold`
(`0.0` versus `-0.5`) and only one had its guards printed — or
`stoneDepthAbove` does not mean what is assumed here.

Resolved, and the answer is that there are **four** `packed_ice` chains, not
two. The two examined above are the shallow pair; the two never looked at
carry different guards:

| chain | `water` | `stone_depth` |
|---|---|---|
| 0, 1 | offset −1, no stone depth, multiplier 0 | offset 0, **no** surface depth |
| 2, 3 | offset −6, **add_stone_depth**, multiplier **−1** | offset 0, **add_surface_depth** |

`add_surface_depth: true` turns the allowance from `1` into `1 + surfaceDepth`,
which is precisely what admits a stack — and the stack is 2,291 of the 2,657
blocks. So chains 2 and 3 do the work, and every condition verified so far
was on the path that does not.

That moves the suspects to the two quantities those guards introduce and the
shallow pair never used: **`surfaceDepth`** and **`stoneDepthAbove`**. Both
are context state maintained during the column walk; both place no block of
their own; neither is measured.

### Found: our surface walk never visits the failing y

`RUSTMC_TRACE_BLOCK=x,y,z` prints every condition asked at one block and its
answer, plus the numbers behind it. Two wrong turns before it paid off, both
worth recording:

1. The trace filters on **position only**, and chunk (0, −135) is generated
   at all three dumped seeds. The `AbovePreliminarySurface false` it showed
   was seed 20260725's column, not the failing one.
2. Its "corners" print used `oracle.min_surface_level`, which the gate stubs
   to `|_, _| 0`. The four zeroes it reported meant nothing.

With the seed announced alongside, the picture inverts:

```text
  [seed 20260725 column (4, -2160) depth 2 sampled [56, 56, 48, 56] blended 50]
  [seed 31415926 column (4, -2160) depth 3 sampled [24, 24, 24, 24] blended 19]
```

At the failing seed the blend is **19**, so `44 >= 19` is true and
`above_preliminary_surface` passes. It was never the guard.

And the failing column emits **no condition trace at y44 at all** — not a
`false`, nothing. The rules are never asked there. So our surface walk does
not visit y44 in that column, the noise stage's water survives untouched, and
every condition in the chain is irrelevant because none of them runs.

Not the bounds either: the walk runs `top 63` down to `bottom -64` at that
column, so y44 is inside it. It reaches y44 and skips it.

`build_column` skips a block when it is fluid, and applies the rule only when
the block is the default (stone). **Vanilla does exactly the same** —
`SurfaceSystem#buildSurface` records `waterHeight` for a fluid and moves on,
and guards `tryApply` with `blockState == defaultBlock`. Ours matches both.

So at y44 our block is **water** and vanilla's must be **stone**, because
vanilla's rule fires there and ours cannot. The surface rules are innocent
end to end — every condition, the tree, the walk, the bounds — and the
divergence is at the **noise stage**, upstream of all of it.

Vanilla's own post-surface column confirms it:

```text
  y 50..44  packed_ice        y 43  gravel        y 42..  stone
```

Stone at y44 in its noise stage, dressed to `packed_ice` by the rule. Ours is
water there, so nothing can dress it. `worldgen-dump column 31415926 4 -2160
40 52` shows it stage by stage:

```text
  y 52..44   noise: water[level=0]        (vanilla: stone)
  y 43       noise: stone -> surface: gravel   (vanilla: gravel)
  y 42..40   noise: stone                 (vanilla: stone)
```

Our gravel at y43 matches vanilla's exactly — the surface rule correctly
dresses the top of our stone run. The run is simply nine blocks too short,
and `OCEAN_FLOOR_WG` follows it down to 44 where vanilla's is ~51.

It is the **aquifer**, and this was checked rather than argued. The density
gate covered seeds 0 and 20260708 only, so seed 31415926 and this column were
added to `GoldenDump` — a dense y sweep at (4, y, −2160) alongside the
existing ones. It **passes**: `finalDensity` matches vanilla at every y
including 44.

So density agrees and the fill still diverges, which leaves
`Aquifer#computeSubstance`. With a negative density the aquifer chooses the
block, and it can legitimately return the default block rather than a fluid —
vanilla's returns stone at y44..50, ours returns water.

That puts both open faults back on the same suspect: this column and the
carve jitter's `computeSubstance` at carve positions.

Narrowed further inside it. `computeSubstance` short-circuits when
`pos_y > skip_sampling_above_y`, returning the **global** fluid without
sampling the aquifer at all — and below sea level the global fluid is water.
Our preliminary surface at that column is 24, so a `skip_sampling_above_y`
derived from it sits well under 44 and takes exactly that path.

Vanilla has the same field, derived the same way (`fromGridY(..., 11) - 1`),
so the difference is in its *input*. Both candidates now checked against the
bytecode and both match: `max_over` uses vanilla's loop exactly (z outer, x
inner, step 4, sampling `(x, z)`), and `adjust_surface_level` is `+8` in
both.

The bounds match too: vanilla calls `maxPreliminarySurfaceLevel(fromGridX(
minGridX, 0), fromGridZ(minGridZ, 0), fromGridX(maxGridX, 9), fromGridZ(
maxGridZ, 9))` and ours is that call argument for argument.

So every link in the `skip_sampling_above_y` derivation matches, and the
remaining candidate is the one thing no gate covers: **`preliminary_surface_level`
outside the chunk being dumped.** The padded grid reaches well past the chunk
edge, and `surface_chunk_vectors.json` carries 256 values for the columns
*inside* chunk (0, −135) and nothing beyond it. If a peak just outside is read
too low, the maximum is too low, the threshold falls under y44, and
`computeSubstance` short-circuits to water.

The dump now carries the eight chunks around (0, −135), and
`preliminary surface` is **0 of 18,432** across all of them. So every input to
`skip_sampling_above_y` is exact and the short-circuit is not the path taken.
The fault is deeper in `computeSubstance` — the barrier/fluid-level sampling
proper.

Widening also showed the fault is not one chunk: the failure count went from
4,110 to **7,984** as the neighbours came into scope. Nine chunks, one cause,
still open.

Checked inside `compute_fluid` since: the neighbour search (`gridX(x−5)`,
`gridY(y+1)`, `gridZ(z−5)`, the 2×3×2 walk and its four-way insertion sort
with `>=` ties), `adjust_surface_level` (+8 both), `max_over`'s loop and
bounds, and the head of `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS` — all match.
`computeFluid`'s context was switched to a bare `SinglePointContext` to match
the bytecode; no measured change.

`compute_fluid` is now checked end to end and matches: all 13 offsets,
`Y_SPACING` 12, the `bottom_of_cell > adjusted` early return, the
`min` taken on the **raw** preliminary rather than the adjusted one, the
`clampedMap(depth, 0, 64, 1, 0)` factor gated on
`surface_at_centre_is_under_global_fluid`, the clamp to [−1, 1], and both
thresholds (`map(f, 1, 0, −0.3, 0.8)` then `map(f, 1, 0, −0.8, 0.4)`) with
the fully-flooded test first.

`randomized_fluid_surface_level`'s head matches too: 16-wide by 40-tall
cells, `floorDiv` on all three axes, spread noise at *cell* coordinates.

**Resume at**: the barrier logic in `compute_substance` after the neighbour
sort, and `computeFluidType`, which is implemented and matches: lava noise sampled
at cell coordinates on a 64/40/64 grid, lava when `|noise| > 0.3`.

The barrier logic in `compute_substance` is fully implemented — the
`sim12 <= 0` early exit, the water-on-global-lava case, the three pressure
pairs, the once-per-call barrier noise, and the four-way `may_flow` fan-out.

### Found: the density handed to the aquifer is −0.010136

`RUSTMC_TRACE_AQUIFER=4,44,-2160` prints which branch of `compute_substance`
answered, and it answers the question reading could not:

```text
  aquifer(4,44,-2160) density=-0.010136:
      centres d1=30 d2=77 sim12=-0.880000 fluid_here=Water status1.level=63
```

The aquifer is **correct on that input**. Its nearest centre has a fluid
level of 63, `sim12` is well below zero so no barrier is contested, and water
at y44 is the right answer for a density of −0.010136. Vanilla returns stone,
so vanilla's density there must be marginally *positive*.

**−0.010136 is a hair below zero**, and that is the whole fault. It is also
why the density gate passes at that exact point: the gate compares
`finalDensity.compute(x, y, z)` at a sampled position, while the fill hands
the aquifer an **interpolated** value from the surrounding cell corners. Two
different numbers, and only one of them is measured.

It is not the interpolation either. The interpolated-density gate covered
seed 31415926 but sampled at (0, y, 0); adding the column (4, y, −2160) —
stride 3 in y, so y44 is hit — it **passes**. Vanilla is handed the same
−0.010136.

So both sides see the same density and disagree anyway. The only paths that
keep stone are `density > 0` and the barrier's `density + pressure > 0`, and
ours never reaches the barrier: `sim12 = similarity(30, 77) = 1 − 47/25 =
−0.88`, which is ≤ 0 and takes the early exit to water. `similarity` matches
the reference, so **vanilla's `d1`/`d2` must differ** — its two nearest
aquifer centres are not ours.

Those come from `positionalRandomFactory.at(gx, gy, gz)` and three
`nextInt` draws (bounds 10, 9, 10) per grid cell. That is the next target,
and it is a small one: a per-cell centre is trivially dumpable.

The cell mapping is not the problem — `gridX`/`gridZ` are `>>4` and `gridY`
is `floorDiv(y, 12)` in both, and `>>4` agrees with `floorDiv` on negatives.
`similarity`'s missing `Math.abs` is a no-op too, since `d2 >= d1` always
holds after the sort.

Nor is the factory: vanilla builds it as
`random.fromHashOf("minecraft:aquifer").forkPositional()` and ours is
`seeding.source_for("minecraft:aquifer").fork_positional()`, the same call.

Which undermines the inference that the centres differ, since every input to
them now matches. That possibility is now closed too: the fill is
`aquifer.computeSubstance(ctx, f.compute(ctx))` where `ctx` is the cell
interpolation context, so the density it passes **is** the interpolated value
`golden_interpolated` measures.

**Resolved: the centres match and the density does not.** Vanilla's aquifer
centres for the twelve cells around (4, 44, −2160) are now dumped
(`aquifer_centre_vectors.json`). Squared distances from the block:

```text
  cell [0, 4, -135] -> centre (3, 49, -2158)   d = 30
  cell [0, 3, -135] -> centre (2, 36, -2157)   d = 77
```

30 and 77 — exactly the `d1` and `d2` the trace reported. So our centres are
vanilla's, `sim12` is −0.88 on both sides, and vanilla takes the *same* early
exit to water. Since vanilla's block is stone, its `computeSubstance` must
have returned `null` before that — which only happens when **density > 0**.

Dumping the density from **inside** vanilla's fill loop settles that too:

```text
  seed 31415926, block (4, 44, -2160):  vanilla -0.005194,  ours -0.010136
```

Both negative, so vanilla's aquifer is consulted as well, gets the same
centres, the same `sim12 = −0.88`, and takes the same early exit. (The two
densities differ in the third decimal, which is its own small divergence and
not enough to change any branch here.)

The early exit returns `status1.at(pos_y)`. Our trace shows
`status1.level = 63`, the global sea level, so `at(44)` is water. Vanilla
must have a **lower** level for that same centre — low enough that `at(44)`
falls through to the default block, which is the stone its surface rule then
dresses to `packed_ice`.

But the centre column itself says something simpler. Vanilla at
(3, y, −2158):

```text
  y 52  packed_ice     y 51  water      y 50..46  packed_ice
  y 45..43  water
```

Mixed — stone dressed to ice at 46–50, water above and below. Ours is water
at every one of 43..52. And the two **agree at y44**.

That is not a fluid-level difference, which would move a whole column at
once. It is the density being marginally too low everywhere: vanilla
−0.005194 against our −0.010136 at the one block measured, a gap of about
0.005. Every block where vanilla's density lands in (0, 0.005) becomes
negative for us and turns to water, and every block where vanilla is already
negative agrees — which is exactly the pattern of that column.

**So the fault is a small, roughly constant density offset**, and the
aquifer, the centres, the fluid levels and the surface rules were all
downstream of it.

And the offset is not a missing interpolation: our fill interpolates too,
loading eight cell corners and blending on `factor_x/y/z`, and both
`golden_interpolator_corners` and `golden_interpolated` pass at this seed and
column (the latter samples y at stride 3 from −64, so y44 is hit).

Which means the two numbers differ **inside vanilla** — or so it looked. The
harness samples through an API; the fill drives `advanceCellX` /
`selectCellYZ` / `updateForY` / `updateForX` / `updateForZ` and reads the
interpolator's own state.

### RESOLVED — it was the harness, and our fill was right all along

The dump on **vanilla's** side computed `state.router().finalDensity()`.
`state.router()` is the **unwrapped** router: its `interpolated` nodes are
plain `DensityFunctions.Marker`, whose `compute` delegates straight to the
wrapped function. Computing that against `noiseChunk` yields the **raw**
per-block noise, not the interpolated value the fill uses.

The fill reads `fullNoiseDensity`, built in `NoiseChunk`'s constructor and
read out of the bytecode (the class is not in `vanilla-src/`):

```java
this.fullNoiseDensity = DensityFunctions.cacheAllInCell(
    DensityFunctions.add(router.finalDensity(), BeardifierMarker.INSTANCE)
).mapAll(this::wrapNew);
```

`mapAll(this::wrapNew)` replaces every marker with a live `NoiseInterpolator`.
`NoiseChunk#getInterpolatedDensity()` computes exactly that field, is
`protected`, and the dumper is in the same package — so it can simply be
asked.

With the dumper corrected, on the column that had 137 of 384 wrong:

```text
  384 positions compared, 0 differ, max delta 0
```

Exact, and the dump repeats twice per run with identical values (ORACLE.md
rule 10). **The fill density was never wrong.** Every hypothesis built on
that divergence — the missing cell cache, the period-8 signature, the
"linear where vanilla is not" reading — was explaining an artifact.

The tell was there in the data and was misread: **all 48 cell boundaries
agreed exactly and every interior differed, in both directions.** Raw and
interpolated coincide precisely on cell corners and nowhere else. A wrong
lattice cannot produce that, because it would move the corners too. A
difference that is exact on a lattice and wrong between its points is a sign
the two sides are evaluating different *functions*, not the same function
badly (ORACLE.md rule 11: compare the most specific thing both sides
express).

### Retracted: "the overworld has zero `cache_all_in_cell` nodes"

That count was of the **datapack**, and it is correct — `noise_settings/
overworld.json` has `interpolated` 4, `cache_2d` 4, `cache_once` 3,
`cache_all_in_cell` 0, `flat_cache` 0. But `NoiseChunk` wraps one around the
whole final density **in code**, as the constructor above shows. So the
marker is live in the fill, and the earlier retraction of the w10 diagnosis
was itself wrong on that point.

It changes no value here, though, and that is worth stating plainly rather
than leaving as an implied fix. Under `fillingCell`,
`NoiseInterpolator#compute` does **not** return the raw value — it returns
`Mth.lerp3(inCellX / cellWidth, inCellY / cellHeight, inCellZ / cellWidth,
noise000 ...)`, the same trilinear blend our `Interpolation::CellFilled`
already computes. The cell cache changes when the work happens, not what it
produces. Our implementation of the marker stays (correct, and now known to
be reachable), and the parity numbers do not move for it.

One convention difference, checked and harmless: vanilla indexes the cell
cache `((cellHeight - 1 - inCellY) * cellWidth + inCellX) * cellWidth +
inCellZ`, top-down in y; ours counts y upward. The fill and the read use the
same convention on each side, so the mapping is a bijection and the values
are identical. It would matter only if the index itself were observable.

### What this closes and what it does not

Closed: the fill density, and with it the claim that `golden_interpolated`
was gating the wrong number. Both sides of the fill now agree exactly on a
384-position column, and the gate was measuring an honest value.

## `packed_ice` — closed, and it was never a surface rule

`SurfaceSystem#buildSurface` applies a rule only where the column holds
neither air nor fluid: the walk resets on air and merely records
`waterHeight` on fluid. An iceberg is packed ice standing **in the water
column**, above the sea floor, so no rule the datapack can express is ever
asked about those positions. Vanilla places them from a second pass —
`SurfaceSystem#frozenOceanExtension` — that runs after the walk and writes
straight into the column.

That is why eleven conditions were eliminated and none of them was the
cause. The biome grid, the preliminary surface, the surface depth, the
`packed_ice` noise bands, the `steep` condition were all measured exact
because **none of them was ever consulted**. The tell was in the confusion
table from the first run and went unread for a long time:

```text
  minecraft:packed_ice -> minecraft:air:            2976
  minecraft:packed_ice -> minecraft:water[level=0]: 4991
  minecraft:snow_block -> minecraft:air:              17
```

No overworld surface rule places snow blocks over open ocean. The third row
said the fault was outside the rule tree, and it was there the whole time.

Ported from the mapped jar's bytecode — `SurfaceSystem` is not in
`vanilla-src/`. Two draw-count traps are preserved: the air branch draws
before it knows whether it will place, and a losing draw falls through to the
water test rather than advancing to the next block; and the method returns
before creating its random source when the column is too thin, so a column
that grows no berg consumes nothing.

    golden_surface_chunk   99.887198%  ->  99.999958%
    frozen-ocean pass wrote 7,984 blocks

The block count is reported by the gate on purpose: a pass that runs and
changes nothing is indistinguishable from one that never ran.

### And it moved the whole-world number by nothing at all

Re-running the r7 diff after the fix:

```text
  seed 20260722   EXACTNESS 99.971047%   4,810 mismatches   (unchanged)
  seed 31415926   EXACTNESS 99.975815%   4,018 mismatches   (unchanged)
```

Identical to six decimal places, with identical mismatch counts — so the
change touched **zero** blocks in either 225-chunk square. The code did not
exist before, so "we were already right there" is not available as an
explanation: neither r7 square contains a frozen ocean.

Worth keeping in view, because it cuts both ways. A whole feature was
missing, it is worth 7,984 blocks where it applies, and the headline number
this project quotes could not see it — the same argument the second seed
earned, one level up. A 225-chunk square is also one place. It also means the
r7 figure understates how much of the generator is unproven rather than
proven: the parts it never exercises are scored neither right nor wrong.

## Next by measured count: 450 blocks of stone variant

With positions now in the differ output, the r7 diff at seed 31415926 reads:

```text
  251  minecraft:granite   -> minecraft:diorite   y -2..4   at (930,0,977) (931,0,977) (930,1,978) (945,2,1071)
  199  minecraft:andesite  -> minecraft:diorite   y  4..9   at (1038,4,936) (1038,4,937) (1037,4,938) (1038,4,938)
```

450 blocks, **no reverse pair** — our diorite is a superset, not a permutation.

What is established:

- The three `_lower` placed features are identical apart from the block:
  `count 2`, `in_square`, `height_range uniform 0..60`, `biome`. They differ
  only by their index in the step list, and therefore only by their stream.
- Our feature order and indices are right. The attempt dump for chunk
  (64, 58) gives indices 2..7 in vanilla's order, with `ore_granite_lower`
  at (1037, 54, 936) and (1034, 2, 931), `ore_diorite_lower` at
  (1028, 59, 940) and (1025, 30, 935), `ore_andesite_lower` at
  (1037, 10, 938) and (1031, 50, 932).
- **None of our `ore_diorite` blobs is near the failing positions.** Sweeping
  the 3x3 chunk neighbourhood, the closest diorite centres to (1038, 4, 937)
  are (1037, 29, 927) and (1040, 14, 916) — 25 and 21 blocks away in y, where
  a size-64 vein reaches a handful. So the diorite we write there did not
  come from an `ore_diorite` feature.
- The loader is **not** collapsing the three configs, which was the first
  hypothesis and is wrong. `ore_granite`, `ore_diorite` and `ore_andesite`
  parse to distinct target states, and vanilla's granite across 169 chunks is
  on the order of 10^4 blocks against 251 disagreements — if the mapping were
  collapsed, every one of them would differ.

So this is localised, not systematic. The open question — what writes diorite
at (1038, 4, 937)? — is now answered, by a new `RUSTMC_TRACE_WRITE=x,y,z`
that prints every write to one block with whoever made it:

```text
  WRITE (1038,4,937) <- minecraft:stone               by ?
  WRITE (1038,4,937) <- minecraft:deepslate[axis=y]   by ?
  WRITE (1038,4,937) <- minecraft:andesite   by ore_andesite from chunk (64, 58)
  WRITE (1038,4,937) <- minecraft:diorite    by ore_diorite  from chunk (64, 59)
```

**Our andesite is right.** It writes the block vanilla ends up with. What is
wrong is that a diorite vein decorated from the *next chunk over* comes
afterwards and paints over it. Within a chunk diorite runs before andesite,
so this could only ever have been a cross-chunk write, and the attempt dump —
which reports where a feature was invoked, not what it wrote — could not have
shown it.

The offending vein is `ore_diorite_lower`'s second position in chunk
(64, 59), at (1038, 6, 945). Reaching z = 937 from there is eight blocks,
which is exactly a size-64 vein's half-length (`size / 8.0`), so the reach is
legal and the vein is not malformed. Either its position differs from
vanilla's or its axis draw does.

That oracle turned out to exist already. `DecorateGolden` dumps, per feature
per chunk, the positions its placement chain produced *and every block it
wrote*; it was pinned to one seed and three centres, and now takes
`-Ddecorate.seed` and `-Ddecorate.centers` so it can be asked about anywhere.

Asked about both chunks, it settles the question completely:

```text
  chunk (64, 58)  ore_andesite_lower  at (1037,10,938) (1031,50,932)
      writes andesite at (1038, 4..11, 937)
  chunk (64, 59)  ore_diorite_lower   at (1035,43,954) (1038, 6,945)
      writes diorite  at (1038, 4.. 7, 937)
```

**Every position is identical to ours, and both veins write the same block we
write.** Nothing about the placement chain, the stream, the feature index or
the vein geometry differs. What differs is which of the two lands last:
vanilla's world holds andesite, so vanilla applied chunk (64, 58)'s ores
**after** chunk (64, 59)'s. Ours decorates (64, 58) first, so diorite wins.

### This is a chunk decoration *order* fault, and it is a class

A feature writes up to one chunk outside its own, so wherever two adjacent
chunks' features touch the same block, the answer is decided by which chunk
is decorated first. Our order and vanilla's disagree, and every overlap in
the r7 window pays for it.

### How much of the diff it actually is: 450 of 2,673

Guessing which faults are ordering is unnecessary now that two vanilla worlds
exist. Take the pairs where **vanilla disagrees with vanilla**, and check
which of our own pairs appear in that set. Over the shared 121-chunk window:

```text
  pair                            ours   van-vs-van   verdict
  granite  -> diorite              251          251   ordering
  andesite -> diorite              199          199   ordering
  deepslate -> cave_air            194            0   real
  cave_air  -> deepslate           191            0   real
  cobweb    -> cave_air            181            0   real
  stone     -> cave_air            168            0   real
  cave_air  -> stone               166            0   real
  cave_air  -> cobweb              156            0   real
  moss_block -> deepslate          141            0   real
  water     -> bubble_column       109            0   real
  stone     -> coal_ore             96            0   real
  ...
  450 blocks in pairs vanilla also disagrees with itself on
  2,223 blocks in pairs it does not
```

The two ordering pairs match **exactly** — 251 against 251, 199 against 199.
Those are the same blocks: we agree with the (62, 62) world and disagree with
the (64, 64) one, block for block. Nothing else overlaps at all.

**This retracts the claim, made one commit earlier, that the symmetric pairs
are the ordering signature.** It was a reasonable read — neither side missing
anything, resolving opposite ways — and it is wrong. `deepslate <-> cave_air`,
`cobweb <-> cave_air`, `stone <-> cave_air` and `water <-> bubble_column` all
score **zero** between the two vanilla worlds. They are real faults, and the
symmetry means something else: a carver displaced by a block or two takes the
mineshaft contents with it, so the same block appears on both sides of the
swap.

So the ordering floor is real but small — 450 of 2,673 at this window, 17% —
and the work list is the other 2,223.

> **Later correction (w11).** "All score **zero** between the two vanilla
> worlds" is too strong, twice over.
>
> First, it was read off `--json`'s `by_block`, which is **truncated to the
> thirty largest pairs** — about four fifths of an r7 diff. Mineshaft rows
> live in the tail, so "absent from the table" was being read as "absent from
> the diff". `RUSTMC_DIFF_PAIRS` now lifts the cap.
>
> Second, the two worlds compared there differ by **centre**, both generated
> as a square. Comparing two worlds that differ by generation **order**
> instead — `vanilla-r7` against `van-rows-c64`, seed 20260722, the same 225
> chunks — gives 10,040 mismatches across 263 pairs, of which **52 are blocks
> only a mineshaft places** (cobweb 26, oak_planks 11, rail 5, oak_fence 5,
> wall_torch 3, chest 1, spawner 1).
>
> Mineshaft blocks are therefore *not* order-independent, and 52 is the floor
> for that stage. The reasoning that led to zero — `StructurePiece#getBlock`
> answers air outside the clip — is sound about a piece's own writes and says
> nothing about `isInInvalidLocation`, which reads liquid inside the clip and
> so sees whatever the carvers left there.
>
> This does not disturb the mineshaft fixes recorded below: those were
> measured in **draw offsets** via `worldgen-dump mineshaft-ceiling`, which is
> order-free. See `swarm/tasks/w11-mineshaft-blocks.md`.

## The largest real fault is the mineshaft, not the carvers

The `cave_air` pairs above are the biggest cluster by far — roughly 1,370
blocks once `deepslate`, `stone`, `cobweb`, `rail`, `oak_planks` and the
stone variants are added up. This file has called them **carve jitter** for a
long time. They are not.

Two measurements settle it.

First, `golden_carver_chunk` **already covers three of the disagreeing
chunks** — (60, 68), (63, 65) and (62, 67) at seed 31415926, added precisely
because the r7 diff pointed at them — and it passes at 99.999958%, its only
failures being the three climate-tie blocks. Meanwhile the r7 diff calls
(62, 67) its **worst chunk, 213 blocks**. Same chunk, same seed, exact
through carving and wrong in the world. So the fault is downstream of the
carvers.

Second, `RUSTMC_TRACE_WRITE` names it outright:

```text
  WRITE (928,-12,1045) <- minecraft:deepslate[axis=y]  by ? (no feature running)
  WRITE (928,-12,1045) <- minecraft:cave_air   by mineshaft into chunk (58, 65)
```

It is the **mineshaft**. Corridors replace stone with cave air and carry the
cobwebs, rails and planks with them, which is why one displaced corridor
shows up as six different block pairs in both directions and looks so much
like scattered carver noise.

### Found: the order two mineshafts reach one chunk in

`StructureManager#startsForStructure` walks the chunk's **reference set** and
hands each start to `placeInChunk` in iteration order, and every start shares
one `RandomSource`. The set is `it.unimi.dsi.fastutil.longs.LongOpenHashSet`
(confirmed in `ChunkAccess#addReferenceForStructure`'s bytecode), whose order
is neither insertion nor sorted.

Two distinct mineshafts reach chunk (58, 65) — starts at chunk (57, 70) and
(61, 65). We walked them in our own scan order. Measured over the r7 window
at seed 31415926:

```text
  our scan order (x then z)      4,018 mismatches   99.975815%
  sorted by ChunkPos.asLong      3,545              99.978662%
  reversed scan order            2,674              99.983905%
  fastutil iteration order       2,187              99.986836%
```

**1,831 blocks.** The reversed and sorted rows are in the table on purpose:
both beat the baseline, and neither is right. Reversing our scan happened to
approximate the real order in the chunks first looked at, and shipping it
would have been fitting the rule to the diff. The last row is the rule.

Reproduced in [`crate::fastutil_long_set`]: open addressing with linear
probing, fastutil's `mix`, and its iterator, which walks the table
**downwards** and returns the zero key first — `ChunkPos.asLong(0, 0)` is
zero, so chunk (0, 0) really does take that branch. Growth is refused rather
than guessed, the same way [`crate::java_hash_set`] refuses a treeified
bucket.

One detail that is easy to get wrong and matters: membership is by the
start's **union** bounding box, which is what `createReferences` intersects —
not by any individual piece. A start whose union reaches a chunk but whose
pieces do not draws nothing, yet it still occupies a slot in the set and so
moves every other start's position in it.

### What that says about the gates

Mineshaft **layout** is gated and exact: 4,036 pieces across three worlds,
100.000000%, including the corridor `hr`/`sc`/`Num` fields. Mineshaft
**blocks** are gated by nothing at all. The layout gate compares bounding
boxes and piece types; how a piece fills its box has never been measured
against vanilla.

That is the gap, and it is the same shape as the one `frozenOceanExtension`
hid in: a stage whose inputs are all proven and whose output nobody checks.
The next instrument is a mineshaft block gate.

### Both halves are gated now, and neither needed a JVM

**References.** `structures.References` is a `LongArray` per structure in
every chunk, and `LongOpenHashSet#toLongArray` fills it *through the
iterator* — so the file records the set's iteration order, not just its
membership. `startsForStructure` walks exactly that set and every start
shares one `RandomSource`, so this is the whole of "what enters a chunk's
pass". `worldgen-dump mineshaft-refs` compares it:

```text
  vanilla-s2-r7   362 chunks, 375 references   0 wrong membership, 0 wrong order
  van-s2-c62      362 chunks, 339 references   0 wrong
  vanilla-r7      362 chunks, 215 references   0 wrong
  vanilla-r7-b    362 chunks, 215 references   0 wrong
  van-c6458        82 chunks,  26 references   0 wrong
```

**Draws.** A corridor's first drawing step is a `generateMaybeBox` at 0.8
that reads nothing: one `nextFloat` per position, taken before every test.
So the ceiling row is a thresholded printout of `3 * (length + 1)`
consecutive stream values, and vanilla saved it. A ceiling position holding
natural rock proves the draw at its index failed; sliding the stream against
the rock recovers the offset the corridor entered on.
`worldgen-dump mineshaft-ceiling` does that for vanilla and, given a second
region directory, for us — and subtracts. That is the draw counter this
investigation was blocked on, and it found four faults in `postProcess`:
`createChest` returning before every chest in the game on an `isAir` written
as a name comparison, the `nextLong` a chest minecart's loot table spends,
`hasPlacedSpider` treated as call state when it is piece state that vanilla
saves as `hps`, and `isSolidRender`/`isFaceSturdy(UP)` answered from block
names rather than the props table. Details in
`swarm/tasks/w11-mineshaft-blocks.md`.

```text
                          all 225 chunks            inner 169 chunks
  before                  3,389   99.984678%        1,785   99.989256%
  clip + props table      3,346   99.984872%        1,770   99.989346%
  createChest             2,721   99.987698%        1,386   99.991657%
  hasPlacedSpider         2,460   99.988878%        1,125   99.993228%
```

**929 blocks over the wide window, 660 over the inner one — 27% and 37% of
everything that was still wrong.** And in the units the tool measures:

```text
  before        88 of 102 offsets agreed; deltas -9, -7, -6, -5, -3 x5, -1, +1 x4
  createChest   101 of 105 agreed; deltas +1 x4
  spider        106 of 106 agreed
```

Every corridor whose ceiling row is readable now enters on exactly the draw
vanilla entered on. Over the 36-chunk window the mineshaft dominates, the
diff fell from 804 blocks to 254, and `cobweb`, `cave_air`, `rail` and
`oak_planks` are no longer in it at all — what is left there is 184 blocks of
`diorite <-> granite` and the ore rows.

It also pins `MEASURED_MINESHAFT_INDEX`. The first corridor of a chunk's pass
must enter at offset 0, and only one index makes that true:

```text
  index   first corridors pinned   at offset 0   unexplainable
  0       3                        0             118
  1       27                       25            8
  2       3                        0             122
  3       6                        0             115
  4       2                        0             120
```

That is evidence at a completely different level from the block diff the
number was originally read off, and it agrees with it.

### A trap in the trace, fixed

The first attribution this trace produced was wrong. It named
`minecraft:freeze_top_layer` for a cave-air write at y = −12, which that
feature cannot do — the label was **stale**, left set by the last feature to
run, and inherited by a structure that sets none. The label is now cleared
after every feature, unclaimed writes read `? (no feature running)`, and the
mineshaft stage sets its own. A label that outlives its writer is worse than
no label.

So the next question is not any individual feature. It is: in what order does
vanilla apply chunk decoration, and what drives it?

### Vanilla disagrees with vanilla

Generating the **same seed** twice with real vanilla, once as the r7 world
centred on chunk (64, 64) and once as a small region centred on (64, 58),
and diffing the two vanilla worlds against each other over chunk (64, 58):

```text
  DIFF chunks=1  blocks=98304  mismatches=134
       134  minecraft:andesite -> minecraft:diorite   y 4..9
  EXACTNESS 99.863688%
```

Two vanilla worlds, one seed, one chunk, 134 blocks apart — and it is exactly
this cluster. The (64, 58)-centred vanilla world holds **diorite** there,
which is what we generate.

The control rules out flakiness. Generating the (64, 58) centre a second time
on a different port and diffing the two:

```text
  DIFF chunks=9  blocks=884736  mismatches=0
  EXACTNESS 100.000000%
```

So vanilla is deterministic **given the generation extent**, and different
extents produce different worlds wherever two chunks' features write the same
block. The seed does not determine the block; the seed and the order the
region was generated in do.

### What that costs this project

The r7 figure is measured against **one** vanilla world, generated at one
centre. Any disagreement caused by cross-chunk feature overlap is scored
against us whether or not our writes are right — and here our writes are
demonstrably identical to vanilla's, position for position and block for
block.

Three consequences, in order of how much they change:

1. **A share of the remaining diff is not a port fault**, and it has now been
   measured. A second full vanilla world of seed 31415926 was generated at
   centre (62, 62) instead of (64, 64), and all three worlds compared over
   the identical 121-chunk window they share:

   ```text
     vanilla(64,64)  vs  vanilla(62,62)     637 mismatches
     ours            vs  vanilla(64,64)   3,258 mismatches
     ours            vs  vanilla(62,62)   2,853 mismatches
   ```

   Same 11,894,784 blocks, same biomes (0 wrong against both). **Our score
   moves by 405 blocks — 12% — depending on which vanilla world grades it**,
   and the two graders disagree with each other by 637.

   So roughly a fifth of the gap at this window is reference-dependent rather
   than ours, and we are closer to the (62, 62) world than to the one the
   headline is quoted against. That is not licence to quote the friendlier
   number: it is a reason to quote both, and to treat 637 as the resolution
   of the instrument. A change worth less than that cannot be told from a
   change of centre.
2. **ORACLE.md rule 10 is too weak.** "Regenerate twice and `cmp`" passed
   throughout, because both runs used the same centre. Regenerating twice at
   the *same* extent tests the RNG; it does not test whether the harness
   itself pins the answer. The rule needs a second extent.
3. **Feature accuracy work is unmeasurable at these positions until this is
   settled**, and worse than unmeasurable: a change could move the number in
   either direction for reasons unconnected to the change.

None of this touches the stage gates, which compare one chunk generated one
way against vanilla generating that same chunk — no cross-chunk ordering is
involved. `golden_surface_chunk` at 99.999958% and the noise stage's
100.000000% are unaffected.

## `moss_block` — our features match the harness, and the harness is wrong

170 blocks of `moss_block -> deepslate` at seed 31415926, asymmetric, so a
genuine miss rather than the ordering floor. It resisted for a while and the
reason is worth writing down, because it invalidates a gate.

The failing block is (1068, −15, 1007) in chunk (66, 62). Everything checks
out except the answer:

- **Our terrain is identical there.** Diffing that chunk alone shows 21
  `moss_block` and a handful of `cave_vines`, and **no `cave_air` or
  `deepslate` disagreement at all** — the caves match block for block, so
  `environment_scan` is reading the same world we do.
- **Our feature indices match vanilla's**: 27, 29, 30 for
  `lush_caves_ceiling_vegetation`, `lush_caves_clay`,
  `lush_caves_vegetation`. Feature streams are seeded per index by
  `setFeatureSeed(populationSeed, index, step)`, so they are independent of
  each other — a wrong earlier feature cannot shift this one.
- **Our positions match vanilla's exactly** wherever both were asked:
  chunk (67, 61) gives `[[1079, −16, 990]]` on both sides, and chunk
  (67, 62) gives `[[1072, −13, 994]]` and `[[1077, −14, 997]]` on both.
- `moss_patch`'s `xz_radius` is uniform 4..7, so **none of those patches can
  reach** (1068, 1007). The nearest is 9 blocks away in x and 10 in z.

So something places a patch centred around (1069, −13, 1004) — inside chunk
(66, 62) — and for chunk (66, 62) **both our port and `DecorateGolden`
produce zero positions from all 125 attempts.**

### The gate is measuring a different world

`DecorateGolden` builds its own 3x3 world per centre. `lush_caves_vegetation`
and its ceiling twin are placed through `environment_scan`, which walks the
world looking for air and then a surface — so their positions are a function
of the terrain that exists **at the moment decoration runs**. A 3x3 world is
not the world a server generates: chunks outside it never contribute, and
each run's world differs from the last because it is centred somewhere else.

`golden_decorate` passes. It passes because it compares our features against
that harness, and our features agree with it *exactly*. Both are wrong
against the real world.

That is a sharper version of the lesson from the ore veins. There the two
oracles were two real vanilla worlds and differed by generation extent; here
one oracle is a real world and the other is a harness, and the harness is the
one that cannot see the terrain.

### The same check on moss says something different

`RUSTMC_TRACE_FEATURE` applied to the moss patch our port *does* place, at
(1079, −16, 990) in chunk (67, 61), seed 31415926 — note the label is the
**configured** feature (`moss_patch`), not the placed one:

```text
  vanilla 118 writes, ours 118
  identical (position and block)   118
  only vanilla 0        only ours 0
  same order   no
```

Every block matches. So unlike `sulfur_pool`, our `moss_patch` is a faithful
port of what it writes — and the 170-block moss fault is therefore a patch we
never **place**, not a patch we place wrongly. That is consistent with
everything measured: the patches we do place match vanilla's coordinate for
coordinate, and none of them can reach the failing block.

The order difference is worth keeping in view even though the block set is
identical. It cannot change this feature's own output — the positions are
distinct, so last-write-wins never fires — but `moss_patch` places a
*vegetation feature* per column through a callback, and if that callback
draws, a different visiting order gives different vegetation. The same chunk
shows `cave_vines` disagreements alongside the moss, which is exactly what
that would look like.

### Sharpening that: the gate is sound, it is *blind*

"`golden_decorate` is unsound" was too strong, and the `sulfur_pool` result
shows why. The gate compares our features against the harness **inside the
harness's world** — both sides see the same blocks, so it is a fair test of
the feature *code*, and it does compare every block each feature writes, not
just positions. It would have caught `sulfur_pool`'s 382-against-325
immediately.

It did not, for a duller reason: it has never covered a sulfur-cave chunk.
Until `GenLevel` gained `isWaterAt` today, `DecorateGolden` **crashed** on
any chunk containing one, so no such chunk could be in the vector set at all.

So the accurate statement is two claims, not one:

1. `golden_decorate` is a sound test of feature code, and its coverage is the
   limit — three centres at one seed, none of them a sulfur cave, none a lush
   cave that places what the r7 world places.
2. It is **blind** to divergence that only appears in a server-generated
   world, because it never sees one. `moss_patch` is the demonstration: our
   writes match the harness's 118 for 118, our positions match, and the r7
   world still has moss we never place.

The fix for (1) is coverage — the harness now takes `-Ddecorate.seed` and
`-Ddecorate.centers`, so a sulfur chunk can be added to the vector set. The
fix for (2) is not more gating; it is understanding why a server's world
differs, which is still open.

### `moss_block`: the complete elimination list

This is the project's deepest open question and it has now consumed a long
careful search, so the eliminations are written down in full — the point of
the list is that **nobody repeats it**.

The fault: the r7 world has 170 blocks of `moss_block` we do not place, the
clearest instance being a ceiling patch around (1068, −15, 1007) in chunk
(66, 62), seed 31415926.

| candidate | how it was eliminated |
|---|---|
| our `moss_patch` writes the wrong blocks | **no** — 118 writes against vanilla's 118, every position and block identical |
| our placement positions differ | **no** — identical wherever both were asked: (67, 61) gives `[[1079,−16,990]]`, (67, 62) gives `[[1072,−13,994]]` and `[[1077,−14,997]]` |
| our feature indices differ | **no** — 27, 29, 30 on both sides |
| the pre-decoration world differs | **no** — the column at (1068, \*, 1007) is deepslate to −20, air −19..−16, deepslate from −15, on both sides |
| the terrain in that chunk differs | **no** — diffing chunk (66, 62) alone shows moss and cave vines and **no** `cave_air`/`deepslate` disagreement at all |
| a patch we place could reach it | **no** — every `moss_patch` variant's `xz_radius` is at most 7 (`moss_patch` and `moss_patch_ceiling` uniform 4..7, `moss_patch_bonemeal` 1..2); the nearest patch is 13 blocks away in z |
| a *different* feature places it | **no** — searched every feature's writes in all four chunks that could reach; the only writes at that x/z are ore, tuff and kelp, at other y |
| the surface rule places moss | **no** — `moss_block`, `moss_carpet` and `azalea` appear nowhere in the overworld surface rule |
| chunk decoration order | **no** — forward raster beats four alternatives by 3,000 to 48,000 blocks |
| write clipping past one chunk | **no** — `write_radius: 1` matches `WorldGenRegion.ensureCanWrite` and the harness's reported `writeRadius: 1` |
| the harness's biome lookup is unfuzzed | **no** — `GenLevel.getBiome` goes through a real `BiomeManager`, and records every lookup it answers |
| post-processing at FULL status | **no** — it runs after decoration and cannot *add* a patch that was never placed |

So: identical draws, identical world, identical positions, identical writes —
and a server produces a patch that neither this port nor vanilla's own
harness produces. Every input that has been compared matches. Whatever is
left is something no measurement here has yet been pointed at.

The one thing not yet tried, and the next thing to try, is to stop comparing
against the harness and instrument **the server**: generate the r7 world with
a Java agent or a mixin that logs every `place` call for the lush features in
chunk (66, 62). That is a different kind of work from everything above, which
is why it has not been done — but every cheaper route is now closed.

## Seed 20260722's biggest cluster is `sulfur_pool`, not the surface rules

With seed 31415926 down to 2,187, seed 20260722 at 4,145 is now the worse of
the two and had never been broken down. Its dominant cluster is sulfur:

```text
  335  cinnabar -> sulfur      252  sulfur -> cinnabar
  179  cave_air -> sulfur       97  sulfur -> deepslate
```

Roughly 860 blocks, all in one region around chunk (58, 63). The obvious
suspect is the surface rule, which picks between the two on
`sulfur_cave_gradient` bands — cinnabar in `[-0.4, -0.1]`, sulfur in
`[0.0, 0.4]`, cinnabar again above `0.4`. It is not the suspect:
`golden_surface_chunk` covers a sulfur-cave chunk and is exact there.

`RUSTMC_TRACE_WRITE` settles it in one line:

```text
  WRITE (937,-20,1009) <- minecraft:cinnabar   by ? (no feature running)
  WRITE (937,-20,1009) <- minecraft:sulfur     by minecraft:sulfur_pool from chunk (58, 63)
```

**Our surface rule writes cinnabar there, which is what vanilla ends up
with.** The `sulfur_pool` feature then paints over it. So the fault is the
feature's placement or its pool shape, and the surface stage is innocent —
the same shape of answer as the mineshaft, found the same way.

`sulfur_pool`'s chain ends with a `block_predicate_filter` requiring the
target block to already be `minecraft:sulfur`, so it reads the world twice
(that filter and an `environment_scan`). Which means `golden_decorate` cannot
gate it either, for the reason in the `moss_block` section above.

### A dead end worth recording

`cave_air -> air` (91) looked like a clean systematic bug — our carver
writing plain air where vanilla writes cave air. It is not: `block_name`
maps `Substance::Air` to `minecraft:air` unconditionally, so if that were
wrong every carved block in the world would differ, not 91. Vanilla's carved
space is `air` too, and those 91 are mineshaft cave-air that our corridors
put somewhere slightly different — part of the mineshaft residue, already
counted.

### The harness was blocked, and the block was informative

Asking `DecorateGolden` for seed 20260722 at centre (58, 63) died with
`Exception in thread "main"` and no visible message — because the harness
writes its own stack traces to **stdout**, which the script sends to the
output file. Read properly it is exact:

```text
  UnsupportedOperationException: the subject reached WorldGenLevel.isWaterAt/1,
  which the javap analysis behind this harness says it does not.
      at SpeleothemUtils.growSpeleothem
      at SulfurSpikeFeature...
```

`GenLevel`, the harness's `WorldGenLevel` proxy, had no `isWaterAt`, and the
sulfur spikes reach it through `SpeleothemUtils`. So **no chunk containing a
sulfur cave could be dumped at all** — which is why this cluster was never
gated. Added, from `LevelReader#isWaterAt`'s own bytecode:
`getFluidState(pos).is(FluidTags.WATER)` — the fluid *tag*, so waterlogged
and flowing water both count.

### RETRACTED AGAIN — and this time the retraction is of the retraction

The section immediately below claims our `sulfur_pool` writes 382 blocks
against vanilla's 325 and is therefore an ordinary wrong port. **That is
wrong, and the error is in the measurement, not the port.**

`DecorateGolden`'s `changes` array is a **diff**: it records a cell only when
its state id changed, updating its snapshot as it goes, so a write that lays
a block over an identical block is invisible and a repeat write collapses.
`RUSTMC_TRACE_FEATURE` records every `setBlock`. The two count different
things and could not have come out equal however correct the port was.

Decomposed properly:

- **All 83 "only ours" write the block that was already there** in the
  harness's own pre-decoration world. Zero exceptions. It is the barrier pass
  laying `sulfur` over `sulfur`, which a diff cannot show.
- **Of the 26 "only vanilla", our full run and the r7 server agree at 25** —
  the harness wrote `sulfur` where both real worlds have water or cave air.
- "Same order: no" compared against a sort. `changes` is reported in grid
  scan order, not write order, and exactly one position of 382 is written
  twice.

So I made the mistake I had just finished quoting ORACLE.md 11 about:
**compare the most specific thing *both* sides express.** A diff and a write
log are not that. Two retractions of the same claim in one day, in opposite
directions, and both from comparing things that were not the same thing.

### The mechanism, finally: the harness decorates only its centre chunk

The 25-of-26 result is not noise, it is the answer to the question this file
has been circling all session. At those positions chunk (58, 62) digs its own
lake at (943, −18, 1006) **before** (58, 63) runs. A real server does that.
This port does that. A `DecorateGolden` 3x3 that decorates only its centre
never does.

That is why the harness and a server disagree about features placed through
`environment_scan` and `block_predicate_filter`: those chains read blocks
that a *neighbouring chunk's* features put there, and in the harness nobody
put them there. It explains `sulfur_pool`, and it is the strongest candidate
for `moss_block` — where the missing patch's origin would likewise have to
sit in a neighbour's output.

Measured per chunk over the r7 world, our writes against the harness's
changes:

```text
  chunk      vanilla changes   our writes   identical   only-van   only-ours
  (58,62)              459          567         459           0         108
  (58,63)              325          382         299          26          83
  (59,62)              445            0           0         445           0
  (59,63)              345          302           0         345         302
```

The two chunks investigated all session are the two that are **right**. At
(59, 62) the harness places four lakes and we place none — because two of its
four origins sit inside the box of the lake (58, 62) digs, above its
waterline, so `cave_air` rather than `sulfur`. **There the harness is the
oracle that is wrong.** (59, 63) is what still costs blocks: the r7 world has
a pool at about (951, −5, 1016) that we do not.

So the fix is to the oracle, not the port: **`DecorateGolden` must decorate a
whole square, not a 3x3 centre.** That single change is what both this
cluster and the lush-caves cluster have been waiting on.

### Superseded: "our port does not match the harness"

The section below was written from four sampled positions and is wrong. Its
conclusion — that we reproduce the harness faithfully and the harness is at
odds with a real server — sent this investigation looking for an oracle
mystery when there is an ordinary bug.

Comparing the **whole** write set of `sulfur_pool` for chunk (58, 63), seed
20260722, using a new `RUSTMC_TRACE_FEATURE` that prints every write a named
feature makes:

```text
  vanilla   325 writes
  ours      382 writes
  identical (position and block)   299
  only vanilla  26        only ours  83
  same order    no
```

Vanilla's extras cluster at x = 937 and ours at x = 941 — a lateral offset,
with a different write order. That is the same signature as the two order
bugs fixed today, and it means the harness is a **sound** oracle here:
positions match, the pre-decoration world matches, only the writes differ.

The lesson is the cheaper one to have learned twice: **four agreeing samples
are not a match.** ORACLE.md 11 says compare the most specific thing both
sides express, and a feature's most specific expression is every block it
writes, in order — which is exactly what the harness already dumps and what
this file did not bother to read.

What survives from the section below: the sulfur cluster's cause is the
feature and not the surface rule, the positions and the pre-decoration world
really do match, and `GenLevel` really did need `isWaterAt`.

### The retraction is itself withdrawn: 325 is a diff, 382 is a write trace

The two numbers above are not the same kind of object, so the comparison
between them could not have come out equal however right the port was.

`DecorateGolden.diff` walks the whole 3x3 grid after each feature and records
a cell only when its **state id changed**, updating the snapshot as it goes.
So a write that puts back the block already there is invisible on the vanilla
side, and two writes to one position collapse to one row. `RUSTMC_TRACE_FEATURE`
records every `setBlock`, no-ops included. Reproduced exactly, and then
decomposed:

```text
  vanilla changes 325   our writes 382
    identical 299   only-vanilla 26   only-ours 83
  of the 83 only-ours: 83 write the block that was ALREADY there in the
                       harness's own pre-decoration world — 0 exceptions
  positions our lake wrote twice: 1
```

**Every single one** of the 83. That is not a port writing extra blocks; it
is a barrier pass laying `sulfur` over `sulfur`, which `LakeFeature` does at
every solid shell cell and which a diff cannot show.

The 26 "only vanilla" are 21 in chunk (58, 62) and 5 in (58, 63). Checked
one by one against both finished worlds: **our full run and the r7 server
agree at 25 of the 26**, and it is the harness that differs at all of them.

```text
  position         harness wrote   r7 (server)   ours (full run)
  (937,-20,1006)   sulfur          water         water
  (937,-18,1006)   sulfur          cave_air      cave_air
  (939,-20,1009)   sulfur          water         water
  (940,-21,1007)   sulfur          water         water
  ... 25 of 26 like this; the 26th is a sulfur_spike tip vs water
```

Every one is a shell cell the harness lined with barrier and both real
worlds have a pool in — because chunk (58, 62) digs its own lake at
(943, -18, 1006) *before* (58, 63) runs, which the harness's 3x3 never does.

The harness's 3x3 decorates only its centre chunk, so its (58, 62) is
undecorated when (58, 63)'s lake reads it; in a real server and in our full
run that chunk has already run all eleven of its own steps. Reconstructing
the harness's finished centre chunk and diffing it against r7 measures how
far that goes: **1,384 blocks**, dominated by ore-vein spill from neighbours
the harness never decorates. So the harness is a sound oracle for *a feature
given a world*, and not for a finished chunk.

### The direct test: `golden_decorate` on the sulfur chunks, and it is green

`RUSTMC_DECORATE_GOLDEN` points the gate at a dump of any seed and centre.
Pointed at seed 20260722 centre (58, 63):

```text
  DECORATE 11 STEPS: 53 attempts, 5308 blocks
  COVERAGE: computed 52, replayed 1 — 98.1% of attempts run by the port
```

Green — which is the attempt order, the positions, the blocks compared row by
row in the harness's own order, and the RNG state after each feature, for
`sulfur_pool`'s 325 changes among them.

Widened to the five chunks the cluster covers, `sulfur_pool` agrees at every
one of them:

```text
  centre    lakes placed                                            changes
  (58,63)   (942,-18,1010) (942,-19,1012)                               325
  (59,63)   (951,-5,1016) (944,-18,1008) (958,-5,1017) (957,-6,1016)    345
  (59,62)   (947,-17,1001) (946,-17,1006) (951,-18,1007) (955,-14,1004) 445
  (58,62)   (943,-18,1006)                                              459
  (57,63)   none                                                          0
```

That run is red, on five disagreements — `amethyst_geode` at (58, 62) and
three ores downstream of the world it left. **No sulfur feature is among
them**, and the geode runs at step 2 while `sulfur_pool` runs at step 1, so
it cannot be shielding one.

On write **order**: the harness cannot express it. `diff` reports in grid
scan order, not the order the feature wrote, so "same order: no" was a
comparison against a sort. What order can still hide is a position written
twice within one feature with two different values — measured here as one
position out of 382 — and everything else about the draws is already pinned
by the RNG state after each feature, which this gate checks. Making
`DecorateGolden` dump the raw `setBlock` sequence alongside `changes` is a
real oracle upgrade and is worth doing; it is not what is wrong here.

`LakeFeature`, `SpeleothemFeature`, `SpeleothemUtils`, `SpeleothemClusterFeature`
and `Column.scan` were also read out of the jar's bytecode against the port,
loop nesting by loop nesting. All four passes of the lake iterate `x` outer,
`z`, then `y` inner as explicit nested loops with `blockPos.offset(i, k, j)` —
there is no `BlockPos.betweenClosedStream` anywhere in it, so the
`underwater_magma` fastest-axis fault has no analogue here. `Direction.Plane.HORIZONTAL`
was confirmed from the class initialiser as NORTH, EAST, SOUTH, WEST.

### Where the sulfur cluster actually is: chunks (59, 62) and (59, 63)

`RUSTMC_TRACE_FEATURE` over the whole r7 world, our `sulfur_pool` writes
against the harness's changes, per chunk:

```text
  chunk     vanilla changes   our writes   identical   only-van   only-ours
  (58,62)             459          567         459           0         108
  (58,63)             325          382         299          26          83
  (59,62)             445            0           0         445           0
  (59,63)             345          302           0         345         302
```

The two chunks the retraction looked at are the two that are right. (58, 62)
is **exact** — all 459 of vanilla's changes, plus 108 no-op writes. (58, 63)
is the 299/26/83 above, all of it accounted for. The cluster lives one chunk
east: **(59, 62) places no lake at all where vanilla places four, and
(59, 63) shares not one write with vanilla's.**

The mechanism is visible in the origin columns. `sulfur_pool`'s chain ends
`environment_scan(up, 32, air) -> random_offset(y -1) -> block_predicate_filter(matching_blocks sulfur)`,
so an origin has to be a `sulfur` block directly under air. At two of the
four origins vanilla's harness used in (59, 62):

```text
  (947,-17,1001)   harness-pre sulfur    r7 cave_air    ours cave_air
  (946,-17,1006)   harness-pre sulfur    r7 spike tip   ours cave_air
```

Both are inside the box of the lake **chunk (58, 62) digs at
(943, -18, 1006)** — mask y 5, above the waterline, so `cave_air`. We place
that lake exactly, and so does the server; the harness's 3x3 does not place
it at all, because it decorates only its centre. `cave_air` is not `sulfur`,
the filter rejects, and no lake follows. So at (59, 62) the harness is the
oracle that is wrong, and our zero may well be the server's zero.

(59, 63) is the one that still costs blocks: the r7 world has a pool at
roughly (951, -5, 1016) — `cave_air -> sulfur` 126 and `water -> sulfur` 47,
bounded x 944..952, y -6..-2, z 1007..1018 — which is exactly the box of
vanilla's first (59, 63) lake, and we have solid sulfur there. Once one
attempt out of 256 is decided differently the lake that follows eats a
different slice of the stream, and every position after it moves; **zero of
302 writes in common** is what that looks like, not a wrong `LakeFeature`.

So the target is no longer the feature. It is what chunks (59, 62) and
(59, 63) see at step 1, and neither oracle in the building can arbitrate it:
`DecorateGolden` decorates one chunk of nine, and the r7 world only shows
the finished result. **The next oracle this needs is a vanilla dump that
decorates a whole square** — the same shape of gap that left the lush-caves
`moss_block` cluster open.

### What the audit did find: the ice pass read a block vanilla never reads

One real deviation, in `LakeFeature`'s fourth pass. Vanilla:

```text
  if (blockState.getFluidState().is(FluidTags.WATER))     // the *fluid provider's* state
```

Ours also asked the **world** at the box's minimum corner, and matched the
fluid by string prefix:

```text
  world.fluid_at(origin) == water || fluid.starts_with("minecraft:water")
```

Neither half is in the reference. The world read makes a lava lake freeze
when it happens to be dug beside water, and the prefix matches
`water_cauldron` while missing `flowing_water`. Fixed to test the provider's
own state, with a loud refusal for a fluid whose fluid state is not decidable
from its name rather than a guess. Inert for `sulfur_pool`, whose fluid is
water either way — recorded because an audit that only reports the faults
that pay is not an audit.

Measured rather than assumed, seed 20260722 over the r7 window:

```text
                      whole window   chunk (58,63)   (58,63) writes vs vanilla
  before                     5,010             149     299 / 26 only-van / 83 only-ours
  after the ice-pass fix     5,010             149     299 / 26 only-van / 83 only-ours
```

**Zero.** No lava lake in this window is dug beside water, so the fault it
removes does not fire here. It is still a fault, and it would have fired the
first time a lava lake landed next to an aquifer.

### Superseded: "our port matches the harness exactly"

All four sulfur features, chunk (58, 63), seed 20260722:

```text
  rooted_sulfur_spring   idx 2   0 positions      identical
  sulfur_pool            idx 3   2 positions      identical
  sulfur_spike_cluster   idx 2   4 positions      identical
  sulfur_spike           idx 3  78 positions      identical
```

Every index, every count, every coordinate. And the blocks agree too — at
(942, −18, 1010) the harness writes `cave_air` and so do we.

**The r7 world has `sulfur` there.** So our port reproduces vanilla's harness
faithfully, and the harness disagrees with a server-generated world — the
third time this has happened, after `moss_block` and this cluster's
`cinnabar`/`sulfur` swap.

That is now the thing to explain, and it is worth more than any of the three
faults it is blocking. Both oracles are vanilla; they differ on features that
read the world during decoration, and roughly a thousand blocks across the
two seeds sit behind the question.

### Decoration order is worth 50,000 blocks, and ours is already right

The obvious hypothesis was chunk decoration order — a feature writes one
chunk out, so two chunks touching the same block resolve by whoever goes
first. `RUSTMC_DECORATE_ORDER=reverse` tests it directly. Seed 20260722,
r7 window:

```text
  forward (x ascending, then z)    4,145 mismatches   99.975050%
  reversed                        52,576 mismatches   99.683532%
```

**Order is worth 48,431 blocks** — an order of magnitude more than every
remaining fault combined, and far more than this file had any reason to
assume. It also settles the question it was asked: our forward raster order
is within 4,145 blocks of a real server's, so it is essentially *the* order.
Order is therefore **not** the explanation for the cave-feature clusters.

That is worth keeping in view for its own sake. Decoration order is the
single most load-bearing choice in the whole feature stage, it was never
gated or even measured, and a plausible-looking change to it would have cost
half a percent of parity silently.

### So the harness-versus-server question narrows

With order eliminated, our r7 run and a server both decorate chunk (58, 63)
after its (57, \*) and (58, 62) neighbours, and features do write across
chunk borders on our side — the ore and mineshaft traces show it. So our
world at decoration time should be the server's, and our `sulfur_pool`
positions should be the server's. They are the harness's instead.

Something else about `DecorateGolden`'s 3x3 differs, so the next step was to
measure the world rather than the feature. `DecorateGolden` records its
pre-decoration chunks run-length encoded, which makes that a direct
comparison. Column (942, \*, 1010), chunk (58, 63), seed 20260722:

```text
  y      harness (pre-decoration)     ours (post-carver)
  -25..-23  deepslate                 deepslate
  -22..-18  sulfur                    sulfur
  -17..-15  air                       air (carved)
```

**Identical.** The two worlds agree block for block going into decoration.

So the terrain is not the answer, and neither is order. What is left is
narrow and strange: the server's final world keeps `sulfur` at
(942, −18, 1010), which is what the *pre-decoration* world already holds
there. So the server's `sulfur_pool` never wrote at that block, while ours
and the harness's both did — from identical positions, identical draws and an
identical world.

Three vanilla-derived answers, two of which agree with us and one of which
does not, from the same seed and the same inputs. The remaining candidate is
that a real server's chunk decoration order is not the raster our
`RUSTMC_DECORATE_ORDER` test compares against — forward beats reversed by
48,431 blocks, which proves it is *close*, not that it is right. A spiral or
a chunk-system order would also beat reversed by a similar margin.

Measured, four orders, seed 20260722 over the r7 window:

```text
  forward   (x outer, z inner)    4,145   99.975050%
  zx        (z outer, x inner)    7,170   99.956842%
  spiral_in (rings, inwards)     18,772   99.887007%
  spiral    (rings, outwards)    26,091   99.842952%
  reversed                       52,576   99.683532%
```

Forward raster wins by a wide margin over every alternative, including both
ring orders — which is what a ticket-driven chunk system would look like. So
**decoration order is settled**: ours is the right one, and the cave-feature
clusters are not explained by it.

### Where that leaves the three clusters

Every general explanation has now been eliminated by measurement rather than
argument:

| candidate | verdict |
|---|---|
| chunk decoration order | forward raster beats four alternatives by 3,000 to 48,000 blocks |
| the world going into decoration | identical to the harness's, block for block, at the failing column |
| feature index or population seed | positions match vanilla's exactly wherever both were asked |
| the surface stage | `golden_surface_chunk` is exact on a sulfur-cave chunk |
| carvers | `golden_carver_chunk` is exact on the disagreeing chunks |
| write clipping | vanilla's `WorldGenRegion.ensureCanWrite` drops a feature's writes more than one chunk from the one being decorated; `Decoration::can_write` does the same, with `write_radius: 1` against the harness's reported `writeRadius: 1` |

What remains is specific rather than structural: the server's `sulfur_pool`
did not write at a block where the harness's did, from the same seed, the
same index, the same draws and the same world. Something about a real
server's decoration that neither `DecorateGolden` nor this port reproduces —
and it is now a small target, because everything around it has been ruled
out.

The three clusters stay open together; one answer still covers all of them.

## `underwater_magma` overplaces — a real fault, and the water cluster's root

The `water <-> bubble_column` pair (222 blocks at seed 31415926, 551 at
20260722) is not its own fault: bubble columns form **above magma**, and the
diff carries `stone -> magma_block` 59 with `magma_block -> stone` 53
alongside it. The magma is the root and the columns follow it.

Compared against the harness for chunk (59, 65), seed 31415926:

```text
  positions   vanilla 17, ours 17 — identical, coordinate for coordinate
  writes      vanilla 3 magma blocks
              ours    more, including (951, 29, 1049) and (953, 30, 1048),
                      which vanilla leaves as stone
```

**Seventeen positions, three blocks.** Almost every position vanilla visits
places nothing, and we place at some of them. This one is gateable: unlike
the lush-caves set, the positions match, so the harness and our port are
looking at the same thing.

`place` and `isValidPlacement` were both read from the bytecode and our port
matches them:

```text
  water or air at pos                          -> false
  isVisibleFromOutside(pos.below(), UP)        -> false
  any horizontal neighbour visible from that
      side's opposite                          -> false
  otherwise                                    -> true
```

### Found: the box was iterated inside out

The counts gave it away. 59 magma blocks too many and 53 too few is not
overplacement — it is the same amount of magma in the wrong places, which is
what a draw landing on the wrong position looks like. And the `nextFloat()`
is drawn for **every** position in the box before any question is asked about
the world, so the nesting decides which position gets which draw.

`BlockPos.betweenClosedStream` walks a flat index. `BlockPos$4.computeNext`'s
first act after loading it is `getfield val$width; irem` — so
`i = n % width`, and **x is the fastest axis**, then y, then z. This port
nested it the other way round: x outer, z inner.

```text
  seed 31415926, r7 window
    x outer (was)     2,187 mismatches
    x fastest (now)   1,785 mismatches
```

**402 blocks** at that seed and **814** at the other, where the bubble columns
that form above magma follow it — 1,216 in total, from three lines of loop
order. The occlusion-shape extract was the other suspect and is innocent:
`BlockPropsGolden` computes
`Block.isShapeFullBlock(state.getFaceOcclusionShape(direction))`, which is
exactly what `isVisibleFromOutside` asks.

### Audited the rest of the box walks

Two order bugs in one day is a class, not a coincidence, so every feature
that walks a box was checked against `betweenClosed`'s decomposition rather
than waiting for the third:

| file | order | verdict |
|---|---|---|
| `geode.rs` | z, y, x | correct, and already commented |
| `underwater_magma.rs` | z, y, x | correct as of this fix |
| `disk.rs` | x, z | **wrong order, currently unobservable** |
| `lake.rs`, `ore.rs` | x, z, y / x, y, z | their own loops, not `betweenClosed` |

`disk.rs` had it backwards *and* said so in a comment. It changes nothing
today: a disk only draws through its state provider, and every disk in the
26.2 datapack — `disk_sand`, `disk_grass`, `disk_clay`, `disk_gravel` —
resolves to `simple_state_provider`, including through
`rule_based_state_provider`'s rules and fallback. So no draw, so no
observable order. Fixed anyway, because that is a property of this version's
data rather than of the code, and the next datapack to use a weighted
provider would have turned it into a silent fault.

### The standing hypothesis: post-processing at FULL status

One difference between the two oracles has not been tested and fits the
shape of the evidence. `WorldCarver#carveBlock` ends with

```java
    if (aquifer.shouldScheduleFluidUpdate() && !state.getFluidState().isEmpty())
        chunk.markPosForPostProcessing(pos);
```

so every carved position holding a fluid is **marked**. Those marks are
applied by `postProcessGeneration()`, which runs when a chunk reaches
**FULL** status — block updates, `updateFromNeighbourShapes`, fluid ticks.

`ci/gen-vanilla-region.sh` forceloads its square, so every chunk in the r7
world reaches FULL and gets that pass. `DecorateGolden` stops at decoration
and never does, and neither does this port beyond the structure fence rule in
`stage/post_process.rs`.

That predicts exactly the pattern seen: the harness and this port agree with
each other and both differ from the server, on features that touch water and
carved space — which is `sulfur_pool` (110 of its 325 writes are water and
154 are cave air), and the standing `water <-> bubble_column` pair, 222
blocks at one seed and 551 at the other, already filed as "bubble columns by
sweep not ticks".

**It cannot explain `sulfur_pool`, and that is worth stating rather than
leaving to be discovered.** Post-processing runs *after* decoration, so it
cannot restore the `sulfur` the pre-decoration world held at
(942, −18, 1010) once a pool has written `cave_air` over it. Block updates do
not put sulfur back. So the hypothesis is already falsified for that cluster,
and it does not obviously explain `moss_block` either. Test it before assuming either way: the cheapest version is to run
the r7 comparison against a vanilla world generated **without** reaching FULL
status, if the harness can be made to stop earlier — if the sulfur and water
clusters vanish, this is the whole of it.

## The last 3 blocks are a climate tie, not a rule

What is left is 3 blocks of `dirt -> sand` at seed 20260725, chunk
(80, 153). Sand at that position can only come from the
`warm_ocean/beach/snowy_beach` or `desert` branch; dirt is the chain's
fallback. So it is the biome the rule saw, and the trace says ours saw
`minecraft:beach`.

Vanilla was then asked directly, through a new `-Ddump.biome=x,y,z` on the
golden dumper that prints `BiomeManager#getBiome` — the fuzzed lookup the
rules are handed, which is **not** the quart grid the biome gate compares.
One run, one seed, one position:

```text
  20260725 1285,54,2458 = minecraft:beach
  20260725 1285,54,2458 = minecraft:river
```

The same position answers differently twice in the same run. That is
`Climate.RTree.search`: strict `>`, seeded from the previous result held in a
`ThreadLocal`, so a point equidistant between two entries resolves by **query
order**. Measured tie rate across the world is 0.195%.

This was already recorded as harmless, on the grounds that the tied pairs
seen then (`river`/`plains`) carry identical carver lists. That conclusion
was too narrow: `beach` and `river` differ in the surface rule, so a tie is
behaviour-visible here, and 3 blocks is what it costs at this seed.

It also makes the golden vectors for that chunk order-dependent on vanilla's
side, so this is not a difference our port can close by computing something
more accurately — it needs our query order into the climate tree to match
vanilla's. That is the next piece of work, and it is a different kind of
problem from everything above.


## Thread scaling

`worldgen-dump scale <seed> <chunks> [max-threads]`. Chunks are independent
— nothing in a `NoiseFill` outlives it and the router is immutable — so this
is close to the cleanest parallelism a server ever gets.

Threadripper 1920X (12 physical cores, 24 threads), 512 chunks:

| threads | chunks/s | speedup | efficiency |
|---|---|---|---|
| 1 | 44.0 | 1.00x | 100% |
| 2 | 85.6 | 1.95x | 97% |
| 4 | 165.6 | 3.77x | 94% |
| 8 | 272.9 | 6.20x | 78% |
| 12 | 362.5 | 8.26x | 69% |
| 24 | 413.1 | 9.39x | 39% |

**413 chunks/s** at 24 threads. The 24-thread efficiency figure is not the
interesting one — 12 of those are SMT siblings, so 39% is roughly what any
compute-bound workload gets there.

The interesting one is that efficiency is already down to **78% at 8
threads**, well before SMT. A 1920X is four CCXs of four cores, each sharing
8 MB of L3, so eight threads is the first configuration to span more than
one CCX pair — and each chunk touches roughly 300 KB of freshly allocated
memory (a 78 KB lattice, a ~196 KB block vector, the flat-cache tables). At
400 chunks/s that is enough allocation churn to keep nothing resident.

So the ceiling here is memory rather than compute. Which *kind* of memory
problem was then tested directly, because "allocator contention" and
"bandwidth" want opposite fixes — a faster allocator against per-worker
buffer reuse.

Swapping the global allocator for mimalloc moved nothing:

| threads | system | mimalloc |
|---|---|---|
| 1 | 44.0 | 42.4 |
| 8 | 272.9 | 273.8 |
| 24 | 413.1 | 407.4 |

**Allocator contention is ruled out.** The dependency was removed again
rather than kept "just in case" — it costs a C build and earned nothing.

That leaves cache footprint and memory bandwidth. Per-worker buffer reuse
may still help, but by keeping a worker's working set resident rather than
by avoiding lock contention, which is a different argument and needs its
own measurement before anyone spends a refactor on it.

Speedups are against one thread on the same machine, never against vanilla:
vanilla parallelises chunk generation too, so a single-threaded vanilla
number beside a 24-thread rustmc one would be meaningless.

### What a profile found that ablation could not

Ablation splits a program by *feature*. It cannot see a cost that is spread
evenly across all of them. Running `perf` on the Threadripper named two in
one report, neither of which was on the list:

- **`f64::floor` was a call into libm.** `Mth.floor` is
  `(int) Math.floor(v)`, which HotSpot compiles to a single `roundsd`
  because the JIT targets the CPU it is running on. Rust's default target
  is baseline x86-64, which has no rounding instruction. Worldgen calls
  `floor` three times per noise sample, and `floor` plus unattributed
  `libc.so.6` frames came to roughly 10% of total generation time. Fixed by
  `target-cpu=x86-64-v2`; the value cannot change, since `roundsd` in floor
  mode is exactly what libm's `floor` computes.
- **`Min`/`Max` recomputed their operand's bounds per block** — 1.9% — by
  asking `min_value()`, which recurses through markers and range choices.
  Those are constants of the graph and are now stored on the node.

The general lesson: ablation finds *features* that cost too much; a profile
finds *mechanisms*. The build-flag one in particular could not have been
found any other way, because it was not in the source at all.

### On measuring the fill by ablation

The fill cannot be timed from inside its own block loop: an
`Instant::now()` costs ~25 ns against a ~140 ns block, so the instrument
would be a third of the measurement. Turning a feature off and taking the
difference measures what it costs *in place*, including work it causes
elsewhere, which is the number worth having.

Both ablation switches are real vanilla settings — `oreVeinsEnabled` and
`isAquifersEnabled` — that several dimensions ship with off, not benchmark
scaffolding.

It immediately paid: ore veins measured **10.47 ms**, the largest item in
the fill, which was not the veins being expensive but us evaluating them
for blocks that cannot have one. Vanilla pulls the three inputs lazily and
the vein band is -60..=50 out of a 384-block column, so three quarters of
every column was paying for two lattice reads it discarded on the next
line. Fixed, and it fell to 0.82 ms.

### Caveat on the laptop

It is a 15 W part that throttles. In one session it measured 53.07 ms/chunk
over 32 chunks and 63.49 ms/chunk over 64. The *ratio* against vanilla in the
same session is more trustworthy than either absolute; prefer the
Threadripper for anything load-bearing.

## The whole pipeline, measured 2026-07-26 — the surface stage is 79% of it

Everything above this section is about the **noise stage**, which is where
the parity work was and where the profiling therefore went. Serving a
decorated world to a real client meant timing the other four stages for the
first time, and the answer is not where anyone was looking.

`rustmc-server run --radius r` prepares a square through every stage before
it listens. Wall time on the Threadripper, uncontended:

| radius | chunks offered | chunks generated | wall |
|---|---|---|---|
| 0 | 1 | 9 | 5.2s |
| 2 | 25 | 49 | 24.0s |
| 3 | 49 | 81 | 39.3s |

Differencing the ends removes the fixed cost: **0.474 s per chunk** through
the whole pipeline, with about **0.9s** of one-off extract loading. That is
~2.1 chunks/s single-threaded, against the noise stage's own few
milliseconds — so better than 95% of a real chunk is spent after the fill.

`RUSTMC_STAGE_TIMES=1` splits it, at radius 2 over 49 chunks:

| stage | wall | share | per chunk |
|---|---|---|---|
| noise | 1.58s | 6.2% | 32.2 ms |
| biomes | 0.88s | 3.5% | 18.0 ms |
| **surface** | **19.89s** | **78.8%** | **405.8 ms** |
| carvers | 0.26s | 1.0% | 5.3 ms |
| structures | 0.13s | 0.5% | 2.6 ms |
| decoration | 2.49s | 9.9% | 50.9 ms |

**The surface rules are four fifths of the cost**, at 1.6 ms for each of a
chunk's 256 columns. Decoration — the stage with eleven steps, hundreds of
features and every tree in the world — is an eighth of that.

This is worth stating plainly because it is the opposite of the intuition
that would have driven the work: decoration *looks* expensive and the surface
rules look like a decision table. The order in the list below was written
before this measurement existed and is about the noise stage, which is now
6% of a chunk. Nothing in it is wrong; all of it is smaller than the row
above.

Two caveats on the number, both mattering for what to conclude:

- **Single-threaded.** Vanilla generates chunks across its worldgen pool, so
  this is not yet a comparison — it is our own side of one.
- **A cold canvas.** These chunks are generated once into an empty canvas
  with no reuse between them, which is the honest shape for a server's first
  load and not for steady-state streaming.

### Vanilla's side of it, and why it is not yet a verdict

Same machine, `ci/gen-vanilla-region.sh` at two radii so the server boot,
gamerules and settle window cancel:

| radius | chunks | wall |
|---|---|---|
| 3 | 49 | 24s |
| 7 | 225 | 32s |

176 chunks in 8s — about **22 chunks/s**, against our 2.1. A ten-fold
wall-clock deficit.

That number should not be quoted without the two things wrong with it:

1. **The poll is 2s granular** and runs a `save-all flush` each round, so
   those 8s are four poll iterations and the true figure is somewhere in
   6–10s: 18–29 chunks/s. The deficit is 9x–14x, not a precise 10x.
2. **Vanilla is using the whole machine and we are using one core.** If it
   really is spreading across its worldgen pool, its *per-core* rate is
   under one chunk a second and ours is ahead of it. That is a completely
   different conclusion from the wall-clock one and neither is established,
   because vanilla's CPU time here was never measured — only its wall time.

Measured, same day, by summing the vanilla server's `utime + stime` from
`/proc/<pid>/stat` and differencing the two radii the same way:

| radius | chunks | CPU |
|---|---|---|
| 3 | 49 | 148.0s |
| 7 | 225 | 185.1s |

37.1 CPU-seconds for 176 chunks — **0.211 CPU-s a chunk**, against our
0.474. So the honest comparison is not ten-fold:

> **We cost about 2.25x vanilla's CPU per generated chunk.** The wall-clock
> gap is ten-fold because vanilla spends its cost across the machine and we
> spend ours on one core.

That 2.25x is a **lower bound on our deficit**, not an estimate: the
differenced CPU still contains whatever vanilla's background threads did
during the eight extra seconds of the longer run, so vanilla's true
generation cost is at or below 0.211 and the ratio is at or above 2.25.

Which makes the target concrete. Parallelising alone would close the
wall-clock gap and leave us burning 2.25x the CPU to do it; the stage table
above says where the 2.25x is.

## The surface stage: one hypothesis tested, and refuted

The obvious suspect was construction. `WorldSurfaceOracle::new` instantiated
every named noise's octaves and redrew the 192 clay bands **per chunk**, all
pure functions of the world seed and never written — where vanilla builds its
`SurfaceSystem` once per chunk generator.

Hoisted into a shared `SurfaceNoises`. The result:

| | before | after |
|---|---|---|
| surface | 405.8 ms/chunk | 378.9 ms/chunk |
| whole pipeline | 0.474 s/chunk | 0.442 s/chunk |

**6.6%.** Worth keeping — it is free, and it matches what vanilla does — but
it is not the answer, and the surface stage is still 78% of a chunk.

So the cost is inside the rule walk, not around it. The next thing to test,
and the reason it is a suspicion rather than a finding: the `biome_at`
closure the walk is given re-runs `ClimateSampler::sample` — six density
functions — and then a linear search over the biome parameter list, **per
query**, returning a fresh `String`. Vanilla does not re-sample climate
during `buildSurface`; the biomes are already in the chunk's container by
then and it reads them out.

That was where the time was.

### Memoising the climate sample: 2x, and byte-identical

Only the sample, never `find_value` — see the comment in
`tools/worldgen-assembly/src/generate.rs` for why the second one cannot be
cached without moving which biome an exact tie resolves to.

Proved the way this kind of change should be proved: **generate the world
both ways and `cmp` the region files**, not diff the blocks. That covers
palettes, heightmaps, biomes and everything `region-differ` does not look at.

| seed | centre | before | after | |
|---|---|---|---|---|
| 20260722 | (64, 64) | 28.4s | 18.0s | 1.58x |
| 31415926 | (-53, -33) | 22.5s | 11.1s | 2.03x |

Identical on all six region files. The second window is land and so asks
more biome questions, which is where the 2x comes from.

The stage table afterwards, same laptop, 49 chunks:

| stage | share | per chunk |
|---|---|---|
| noise | 21.6% | 47.3 ms |
| biomes | 6.8% | 14.9 ms |
| surface | 44.4% | 97.2 ms |
| carvers | 2.0% | 4.4 ms |
| structures | 1.4% | 3.0 ms |
| decoration | 23.7% | 51.9 ms |

The surface stage fell from 79% to 44% and the profile is flat now — there is
no single row left to attack. Within the surface stage the biome query is
still the largest term, but it has inverted: the climate sample is 0.53s and
`find_value` is 2.50s, where before the cache it was 7.11s and 1.95s.

### A null result on `find_value`, and a warning about this laptop

`Climate.RTree.search` returned an owned node, cloning a leaf at every level
of every query, where vanilla holds a reference in its ThreadLocal. Fixed —
byte-identical on the same two-window gate.

**It bought nothing measurable.** The search went 2.50s to 2.38s, and the
*same workload* swung 10.71s to 13.82s between two runs. **The laptop's noise
floor is ~29%**, which a 5% claim cannot survive. The cost is the
seven-axis distance computation across the tree, not the copying.

That is two wrong hypotheses about this stage in one session — the other was
`SurfaceNoises` construction, which bought 6.6%. Both were plausible, both
were about *allocation and setup*, and the thing that was actually expensive
was neither. The measurement that ended it was not cleverer, it was just
narrower: count the calls and time the two halves separately.

**Nothing under about 30% should be believed from a number taken on this
laptop.** Re-measure on the Threadripper.

## Re-measured on the Threadripper — decoration is half the pipeline (2026-07-28)

```
RUSTMC_STAGE_TIMES=1 ./target/release/worldgen-dump full 20260722 16 <out> 64 64
```

Every stage table above this line was taken on the laptop over 49 chunks.
Here it is on the Threadripper at two radii, over 441 and 1,369 **generated**
chunks — the same centre and seed the server benchmark uses:

| stage | r=8, 441 chunks | | r=16, 1,369 chunks | |
|---|---|---|---|---|
| | share | ms/chunk | share | ms/chunk |
| noise | 22.7% | 35.6 | 23.6% | 36.2 |
| biomes | 6.8% | 10.8 | 7.8% | 12.0 |
| surface | 17.0% | 26.7 | 15.8% | 24.3 |
| carvers | 2.2% | 3.5 | 2.5% | 3.9 |
| structures | 0.2% | 0.3 | 0.1% | 0.1 |
| **decoration** | **51.1%** | **80.3** | **50.1%** | **76.8** |
| TOTAL | 69.29 s | 157.1 | 209.85 s | 153.3 |

**The order has inverted since the laptop table.** Decoration is 50%, not
24%; the surface stage is 16%, not 44%. That is not a regression and it is
not machine noise — it is the stored-biome-container change. The surface
rules used to re-derive the biome for every one of ~28,000 queries a chunk,
and the profiler line now reads `5.03s = 4.84s climate sample + 0.19s
parameter search` out of a 209.85 s pipeline: **2.4% of the total**. The two
sections above spent a whole session inside a term that is now a rounding
error, and their conclusion — "the profile is flat, there is no single row
left to attack" — is no longer true. There is: it is decoration.

The two radii agree to within 3% on every row, and TOTAL/chunk of 153.3 ms
independently reproduces the server benchmark's 0.147 CPU-s per **generated**
chunk at the same centre. The stage split and the four-server table are
measuring the same pipeline.

What this rules in and out for the wall-clock gap, which is the bigger one:

- Everything except decoration is per-chunk independent given a per-thread
  router — `worldgen-dump scale` already parallelises the noise stage at 8.26x
  on 12 threads. But that is only **49.9%** of the pipeline, so Amdahl caps a
  noise+biomes+surface+carvers-only parallel pipeline at **1.8x wall**.
- Decoration is the half that resists it, for the reason `DECORATION_MARGIN`
  exists: features write across chunk borders and the order is load-bearing.
  A parallel decoration stage is a parity risk, not just a refactor.

So the honest next question for item 2 is not "add threads", it is **why is
decoration 77 ms a chunk**.

## The first real profile — and it killed the hypothesis it was taken to test

```
sudo sysctl -w kernel.perf_event_paranoid=1      # it ships at 4 on this box
perf record -F 199 -g -o /tmp/rustmc.perf -- \
    ./target/release/worldgen-dump full 20260722 6 <out> 64 64
perf report -i /tmp/rustmc.perf --no-children --percent-limit 1.0 --stdio
```

Threadripper, seed 20260722, r=6 at (64,64), 289 generated chunks. Flat
profile, everything at or above 1%:

| % | symbol |
|---|---|
| 8.58 | `ImprovedNoise::noise_with_fudge` |
| 6.40 | `malloc` |
| 4.81 | `cfree` |
| 4.30 | `Pipeline::prime_heightmaps` |
| 3.79 | `DensityFunction::compute` |
| 3.42 | libc, unresolved |
| **3.40** | **`drop_in_place<BTreeMap<String, BTreeSet<String>>>`** |
| **3.24** | **`BTreeMap::clone::clone_subtree`** |
| 2.60 | `PerlinNoise::get_value_with_fudge` |
| 2.20 | `ChunkSampler::evaluate` |
| **2.17** | **`getenv`** |
| 2.11 | libc, unresolved |
| 1.74 | `biome::manager::quart_for_block` |
| **1.67** | **`std::env::__var`** |
| 1.64 | `Canvas::set_block` |
| 1.33 | `surface::eval::test` |
| 1.10 | `NoiseBasedAquifer::compute_substance` |

> **Noise is ~17% here, not 54%.** `ImprovedNoise` + `PerlinNoise` +
> `DensityFunction::compute` + `ChunkSampler::evaluate` = 17.17%. The
> sibling project's "noise sampling is 54% of the profile" does **not**
> transfer to this tree. SIMD over the noise lattice — the obvious big idea,
> and the one this profile was taken to justify — is worth about **13% of a
> chunk even at a 4x speedup on the whole of it**, and it is the change most
> likely to break bit-exactness, because reassociating float adds changes
> results. It is not the lever. Do not spend a session on it.

What the profile named instead was two things nobody had looked at, both
pure overhead, both fixed the same afternoon:

- **`getenv` 2.17% + `std::env::__var` 1.67% = 3.8%** spent asking whether
  tracing is switched on, with every switch off. Every `RUSTMC_TRACE_*` gate
  in this crate is read at its point of use — inside placement chains,
  inside feature inner loops. `vegetation_patch.rs` reads one **ten times**.
  `getenv` is a linear scan of `environ` with a `strncmp` per entry.
- **`BTreeMap<String, BTreeSet<String>>` clone 3.24% + drop 3.40% = 6.6%**,
  which is `BlockTags` — every block tag in the game, deep-copied *per
  placement attempt*. `PlacementModifier::EnvironmentScan` and
  `BlockPredicateFilter` both do `world.block_tags().clone()`, not because
  they want a copy but because `predicate.test` needs `&mut world` and
  `&tags` at once and the tags come out of the world. A borrow-checker
  workaround that nobody re-costed.

### The fixes, and what they were worth

`BlockTags` now holds its resolved map behind an `Arc`, so the same
`.clone()` is a refcount bump — **no call site changed**, because the map was
already never written after `from_entries` returned. And `crate::trace_switch`
reads each switch once into a `OnceLock` (a `OnceLock` rather than the
`thread_local!` `canvas.rs` uses, because a worker pool would pay a
`thread_local` init check per thread).

**Output-identical, and proved rather than assumed.** Four windows, two
centres, regenerated with the new binary and `cmp`ed against the dumps the
old one wrote: **16 of 16 region files byte-identical, 0 differ.** `cmp`
rather than `region-differ diff` deliberately — it also covers palettes,
heightmaps and biomes, which a block differ does not look at and which is
exactly what a palette-adjacent change could move.

| window | before | after | |
|---|---|---|---|
| r=16 at (64,64), 1,369 generated chunks | 209.85 s | **138.41 s** | **1.52x** |
| r=8 at (64,64), 441 chunks | 69.29 s | 41.94 s | 1.65x |
| r=4 at (0,0), 169 chunks | 17.76 s | 15.09 s | 1.18x |

Per stage, at r=16:

| stage | before | after | |
|---|---|---|---|
| noise | 36.2 ms/chunk | 36.8 | unchanged, as it should be |
| **decoration** | **76.8 ms/chunk** | **23.0** | **3.34x** |
| whole pipeline | 153.3 ms/chunk | **101.1** | 1.52x |

**A 10.4% profile share bought 34% of the runtime.** That is worth
understanding rather than celebrating: `perf`'s resolved `getenv` symbol is
only part of the cost — several of the unresolved libc frames above (3.42,
2.11, 1.56, 1.25, 1.19 = 9.5%) sit in the same address range and are almost
certainly its internals. The lesson for the next round is that the flat
profile **under**-attributed the winner, so a symbol at 3% is worth probing
rather than dismissing.

**Decoration is no longer half the pipeline.** It was 50.1% and is now
22.7%; noise is the largest single row at 36.4%. The stage table in the
section above is superseded for the after-case, and any plan built on
"decoration is where the time goes" needs re-reading.

### And a second lead: the noise stage costs 53% more inside the pipeline

`worldgen-dump bench 20260722 64` on the same machine reports **23.67
ms/chunk** for the noise stage — 1.72 graph clone + 7.92 setup + 14.03 fill.
The pipeline's `noise` row for the same seed on the same machine is **36.2
ms/chunk**. Same stage, same code, **+12.5 ms**.

The two differ in exactly one thing: where the blocks go. `bench` fills a
dense chunk; the pipeline fills a `Canvas`, and `Canvas::set_block` is, per
block:

```rust
let Some(chunk) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { ... };   // BTreeMap lookup
let id = match chunk.palette.iter().position(|p| p == state) { ... };    // linear scan of Vec<String>
```

A chunk is 98,304 blocks, so filling one is 98,304 `BTreeMap` lookups on a
key that does not change for the whole chunk, and 98,304 linear string
compares against a palette that is a handful of entries long but is compared
by `str` content every time. Both are per-block costs that carry no
per-block information. **Not yet measured** — `bench` and the pipeline also
sample different terrain (origin against (64,64)), which is worth some of
the 12.5 ms and cannot be worth most of it. That is the measurement to take
next, and the fix, if it is the fix, has to be proved the way the climate
cache was: regenerate and `cmp` the region files, not diff the blocks.

## Where the diff is now, on the (64,64) window

Measured 2026-07-27 against `oracle-work/t2-regiondiff/a`, the 25-chunk
square at centre (64,64), seed 20260722:

```text
  DIFF chunks=25  blocks=2,457,600  mismatches=498  biomes=38,400/0
  EXACTNESS 99.979736%
```

**About 91% of it is one thing** — sulfur against cinnabar, 320 direct swaps
plus ~130 sulfur-spike presence differences. Nothing else reaches double
figures except a 10-block andesite/diorite patch.

And it is attributed, which it was not before. See
`crates/rustmc-worldgen/tests/probe_sulfur_gradient.rs` for the working; the
short version is that everything upstream of the feature was cleared against
vanilla rather than by argument — noise parameters identical to the datapack,
`Context$2` samples `(blockX, blockY, blockZ)` as we do,
`NoiseThresholdCondition` disassembles to `>= min && <= max` so the shared
0.4 band edge is not it, and the window has **zero** biome mismatches. Then
`worldgen-dump column` at the dump's own centre named the stage outright:

```text
  (993,-3,1003)  noise stone -> surface CINNABAR -> carvers - -> decorated SULFUR
```

Our decoration paints sulfur over cinnabar our own surface rule placed
correctly.

### ...and then most of it turned out to be the margin, not the feature

Chasing that cluster to its origin landed on chunk (61,65) — a **margin**
chunk. Which raised a better question than the one I was asking: is the
generator wrong, or is the harness?

The harness. `DECORATION_MARGIN` was 1, and one ring is not enough — see
ORACLE.md rule 14 for why the write-distance argument that justified it is
the wrong argument. Regenerating the same window at increasing width:

| context around the window | sulfur | cinnabar | vs vanilla |
|---|---|---|---|
| vanilla | 222,219 | 168,964 | — |
| 1 chunk | 222,404 (+185) | 168,778 (−186) | 498 |
| 2 chunks | 222,282 (**+63**) | 168,878 (−86) | 557 |
| 3 chunks | 222,282 (+63) | 168,878 (−86) | 557 |

Two and three agree **exactly** — `region-differ diff` between them is
100.000000% over the window — so two is converged and one was wrong. Our own
one-ring output differs from the converged answer by **604 blocks**, which is
more than the 557 it disagrees with vanilla about.

So the sulfur/cinnabar excess is a third of what it looked like, the
remainder is a real residual worth chasing, and **the 99.979736% above is not
the number** — the converged one is 99.977336%, slightly worse, because the
thin margin had been wrong in a flattering direction.

The generator was never the thing to read first here. Two of the three most
promising leads tonight — the `root_system` refusal and this cluster — came
apart on the same point: what looked like a fault was a property of how the
world was being generated for measurement.

### The converged diff, and its new top item: a cave we do not carve

At the converged margin the window is **557 mismatches / 2,457,600 =
99.977336%**, and the shape of it is different from what margin-1 showed:

| | blocks | |
|---|---|---|
| `cinnabar -> sulfur` | 135 | was 253 |
| `sulfur -> deepslate` | 51 | new |
| `sulfur -> cinnabar` | 49 | was 67 |
| `cave_air -> air` | 43 | new |
| `cave_air -> sulfur` | 35 | new |
| `cave_air -> deepslate` | 33 | new |
| `water -> deepslate` | 29 | new |

Chunk (62,62) alone holds 315 of the 557, and **229 of those sit in five y
levels**, −37 to −33, with this profile:

```text
  y -37   40
  y -36   65
  y -35   69
  y -34   53
  y -33    2
```

That is a lens, which is a cave in cross-section — not the scatter a feature
makes. `worldgen-dump column` at (1000, 992) says the rest:

```text
  y -37   noise stone -> surface deepslate      vanilla: sulfur
  y -36   noise stone -> surface deepslate      vanilla: water
  y -35   noise deepslate_iron_ore              vanilla: same
```

Our carvers column is **blank** through the band — we never cut there.
Vanilla did: it has `cave_air` through the middle of the lens, water at the
bottom where the cut went below the local water table, and sulfur on the
floor the cut exposed. Everything we disagree about in that chunk follows
from the cave not existing: no cut, so no exposed floor for the surface rule
to make sulfur of, so the deepslate band answers instead.

That reading was wrong, and the way it was wrong is the useful part.

**It is not a carver.** `RUSTMC_TRACE_CARVER` (added for this) reports what
the sweep did for one chunk, and chunk (62,62) is healthy: 867 carvers
offered, 64 passed `isStartChunk` — identical structure to its neighbour —
and the aquifer was asked 613 times and declined **none**, answering
`minecraft:air` 543 times and water 70.

So the carves ran and wrote. They just do not write `cave_air`: our
`carve_state` returns the aquifer's substance verbatim, and so does
vanilla's. Reading the jar settles it — `WorldCarver.CAVE_AIR` is assigned in
the static initialiser and **never read**, `Aquifer$NoiseBasedAquifer`
references only `AIR`, `LAVA` and `WATER`, and **no class under
`levelgen/` or `chunk/` references `CAVE_AIR` at all**. Carvers on both sides
carve to plain air.

Which means the 119 `cave_air` vanilla has in that chunk are **structural**,
and there are only three things in this port that write it: mineshafts,
monster rooms, and lakes. A census rules out the first two — no
`oak_planks`, `rail`, `cobweb`, `mossy_cobblestone`, `spawner` or `chest` on
either side. So it is a lake, and the geometry agrees: `LakeFeature`'s box is
eight tall anchored at `origin - (8, 4, 8)`, fluid in the lower half and
`cave_air` above, which puts fluid at −40..−37 and air at −36..−33 — the
band observed. Vanilla also has 45 more water blocks there than we do.

**One `sulfur_pool` lake that vanilla places and we do not**, and that
accounts for the rest of the chunk too: no lake means no sulfur barrier, so
`sulfur -> deepslate`; no lake means no hollow, so `cave_air -> deepslate`
and `water -> deepslate`.

Which also retracts something written two sections above. Equal
`potent_sulfur` counts (9 and 9) were taken there as exonerating
`sulfur_pool`. They do not: `potent_sulfur` is placed by the pool's *second*
sub-feature, only where an environment scan succeeds, so matching counts say
nothing about whether the same lakes were placed. The lens shape in a y
histogram was the evidence that mattered, and it was sitting in the same
data.

#### Where it stands: the pool never runs there

`RUSTMC_TRACE_FEATURE=sulfur_pool` over the same 25-chunk dump writes
**12,778 blocks**, and **zero** of them come from any of the nine chunks that
could reach (62,62) — a lake's box is 16 wide anchored at `origin - (8,4,8)`,
so (61..63, 61..63) is the complete set of possible sources. Elsewhere in the
same run it fires freely: 1,733 writes from (65,66), 1,079 from (65,62), 832
from (60,67).

So this is not a lake landing in the wrong place. It is a lake that never
happens in that neighbourhood at all, and there are exactly two ways that can
be true:

1. the placement chain rejects all 256 attempts per chunk — its later links
   are position-dependent (`block_predicate_filter` for solid, an
   `environment_scan` 32 up for `#minecraft:air`, then a filter for a block
   that is already `minecraft:sulfur`); or
2. the chain yields a position and `LakeFeature`'s **validity pass** aborts,
   which it does if any cell just outside the mask fails — and it writes
   nothing when it does.

The trace could not separate them, because it records writes and both produce
none. So `RUSTMC_TRACE_ATTEMPT=<substring>` was added — it reports, per
invocation, how many attempts survived the chain and which modifier index
turned the rest away.

The answer is **(2), and it points back at the spikes**:

```text
  ATTEMPT sulfur_pool chunk (62,62) reached-feature=8   rejected_at=[0,0,0,159,46,0,47]
  ATTEMPT sulfur_pool chunk (65,62) reached-feature=30  rejected_at=[1,0,0,166,51,0,25]
```

The chain is **not** the problem: eight attempts reached the feature in
(62,62). The lake ran eight times and wrote nothing, so `LakeFeature`'s
validity pass aborted every one — and that pass is gated on the pool's own
`can_place_feature`, which is `not(matching_blocks minecraft:sulfur_spike)`.
A spike just outside the lake's mask abandons the whole lake.

Then the census on that chunk:

```text
  minecraft:sulfur_spike   vanilla 42   ours 57
```

**Fifteen extra spikes** — and the obvious reading, that the spikes veto the
lake, is **wrong**. `biome_features.json` puts them in different decoration
steps:

```text
  step 1 (LAKES)                   minecraft:sulfur_pool
  step 7 (UNDERGROUND_DECORATION)  minecraft:sulfur_spike_cluster, sulfur_spike
```

The pool runs **six steps before** any spike exists. So
`can_place_feature: not(matching_blocks minecraft:sulfur_spike)` is
trivially satisfied when the lake is attempted — there is nothing for it to
match — and it cannot be what aborts those eight attempts.

The arrow runs the other way: **the missing lake causes the extra spikes.**
Vanilla places a lake at step 1, which hollows rock and lays water and
sulfur; by step 7 its spikes are placed against that terrain and find 42
homes. We place no lake, so step 7 sees unbroken rock and finds 57. The
spike surplus is a *consequence*, and the window-wide numbers agree with
that reading — spikes are only +10 across 25 chunks while (62,62) alone is
+15, so there is no systematic over-placement to explain.

Which leaves the original question sharper and unanswered: **why do all
eight lake attempts abort at step 1?** `LakeFeature`'s validity pass
abandons the whole lake if a single cell just outside the mask fails, and at
step 1 the terrain it is judging is noise + surface + carvers, which this
port matches vanilla on to 3 blocks over the carver gate's own window. So
either that pass reads something we build differently, or it is stricter
than vanilla's.

`RUSTMC_TRACE_LAKE=1` names the cell and the reason. Across the 25-chunk
dump, 261 lakes abandon:

```text
  211  wall-not-solid
   41  liquid-above-waterline
    9  can_place_feature
```

and every abandon inside chunk (62,62) is the first kind, on the same block:

```text
  LAKE abandon at (1006,-36,994) reason=wall-not-solid block=minecraft:air
  LAKE abandon at (1001,-3,1004)  reason=wall-not-solid block=minecraft:air
  LAKE abandon at (1007,-34,999)  reason=wall-not-solid block=minecraft:air
```

The wall test is `y < 4 && !solid && state != fluid`, so this says our step-1
terrain has **air** where the lake needs something solid to hold water. That
is upstream of every feature: at step 1 the world is noise + surface +
carvers and nothing else.

Which puts the investigation back above the feature layer for the fifth time,
now with a sharp shape: **we have air at (1006,-36,994) and vanilla, to have
placed its lake, must have had solid.** Except that it does not need an oracle, because the diff already answers it:
**none of those three cells is a mismatch.** Vanilla has air at
(1006,−36,994) too. So vanilla's lake, attempted at that origin, would have
abandoned for exactly the same reason.

Which relocates the fault one more time, and this is the useful conclusion:
the terrain is not the difference and the lake logic is not the difference —
**the difference is which origins get attempted.** Our chain yields 8 valid
origins in that chunk out of ~256 attempts, rejecting 159 at modifier 3, 46
at 4 and 47 at 6. Vanilla evidently yields at least one origin we do not,
and that one is placeable.

That is a much narrower target than "the lake aborts", and it is reachable
without an intermediate-status capture: the chain's modifiers are
`block_predicate_filter` (solid), `environment_scan` (32 up for
`#minecraft:air`), `random_offset`, and `block_predicate_filter` (already
sulfur), all of which read only the step-1 world.

### And there it is: the surface rule disagrees with its own noise

`RUSTMC_TRACE_ATTEMPT_POS=1` records **where** each rejection happened, not
just which modifier. Of the 47 attempts the already-sulfur gate turns away in
(62,62), one lands on a cell where we differ from vanilla:

```text
  (1003,-36,993)   vanilla sulfur   ours deepslate
```

`worldgen-dump column` shows that deepslate came from our **surface stage**
and nothing touched it afterwards, so the gate rejected correctly given the
block. The question is whether the block is right — and our own noise answers
it:

```text
  (1003,-36,993)  sulfur_cave_gradient = +0.004351   -> band [0.0, 0.4] -> sulfur
  (1000,-37,992)                        = +0.006258   -> sulfur
  (1001,-37,992)                        = +0.010375   -> sulfur
```

19 of the 52 `sulfur -> deepslate` cells in that chunk have a gradient value
inside the sulfur band and we wrote deepslate anyway — which looked like this
port disagreeing with itself.

**It is not, and the probe was the thing that was wrong.** `RUSTMC_TRACE_BLOCK`
asks the rule tree directly at that cell:

```text
  false  AbovePreliminarySurface  (blockY -36 vs minSurfaceLevel 25)
  false  Biome(["minecraft:sulfur_caves"])  (biome minecraft:lukewarm_ocean)
```

The cell is not in `sulfur_caves`. The three noise bands live **inside** that
biome branch, behind `above_preliminary_surface`, a `stone_depth` floor test
and a `water` test — four gates, and the noise is consulted only if all four
hold. `probe_sulfur_gradient` applies the bands unconditionally, so "our
noise implies sulfur" is a statement about a rule that was never reached.
The gradient is a real number and the band arithmetic is right; the inference
drawn from it was not.

Since the biome containers match vanilla with **zero** mismatches, vanilla
sees `lukewarm_ocean` there too and skips the same branch. So vanilla's
sulfur at (1003,−36,993) is almost certainly the `sulfur_pool` **barrier** —
the lake's own output — which puts the question back where it was rather
than answering it, and removes a surface-rule bug that was never there.

The lesson is the probe's, and it is the same one this file has recorded
twice already in different clothes: a helper that reproduces part of a rule
answers questions about that part, not about the rule. `band()` models three
`noise_threshold` conditions and none of the four gates in front of them.

### The lake is confirmed, and the gate that loses it is named

A full census of chunk (62,62) lists exactly two blocks vanilla has and this
port has none of:

```text
  119  minecraft:cave_air
    1  minecraft:potent_sulfur
```

`potent_sulfur` is written by **one thing in the whole datapack** — the
second sub-feature of `sulfur_pool`. So vanilla placed a pool in that chunk
and we did not, with no inference required. (It also retires the equal 9-and-9
window totals used earlier to exonerate the pool: per chunk it is 1 against
0, and the totals matched by coincidence across chunks.)

Vanilla's lake occupies x997..1005, y−35..−34, z992..1002. Our placement
chain rejects **10 candidate origins inside that footprint**, and seven of
them at the *already-sulfur* gate, at exactly the depths a lake origin needs:

```text
  #6 already-sulfur  (1007,-37,994) (995,-37,994) (1007,-36,997)
                     (1006,-36,996) (1003,-36,993) (992,-34,993) ...
  #3 solid           (1007,-35,997) (999,-32,995) (998,-29,996)
```

So vanilla had `minecraft:sulfur` at one of those cells **at step 1** and we
did not. At step 1 the only sulfur in the world is the surface rule's, and
`RUSTMC_TRACE_BLOCK` says why ours declines at (1003,−36,993): the rule sees
`lukewarm_ocean` and `above_preliminary_surface` is false with
`minSurfaceLevel 25`.

That looked answerable by checking the surface rule's biome lookup against
the biome container. It is not the question: `SurfaceSystem.buildSurface`
takes a `BiomeManager` and its getter is `biomeManager::getBiome`, the
**fuzzed** path — so this port using the fuzz is right, and disagreeing with
the container is expected in vanilla too.

### 250 of 252 rejections happen where the two worlds agree

Every position our chain rejects in that chunk, checked against the diff:

```text
  252 rejections
    2 at cells where we differ from vanilla
      #6 (1003,-36,993)  vanilla sulfur    ours deepslate
      #6 (993,-15,999)   vanilla cinnabar  ours sulfur
```

and both of those are *consequences* rather than causes. The first sits
directly beneath the lake body at exactly where a floor barrier goes, so
vanilla's sulfur there is the lake's own output. The second is our step-7
spike base written over cinnabar, which only exists because the lake is
missing.

So our chain rejects the same candidates vanilla's would, the terrain agrees
at every cell either side examines, the mask draws match the bytecode
(`nextInt(4)+4` rounds, six draws each, sizes before centres), and the
validity pass matches — and vanilla still places a lake where we do not.

**The contradiction resolves on a property of the walk that had not been
stated: it is stateful.** `walk` is depth-first and the modifiers *draw*. A
candidate rejected at modifier 3 never reaches modifiers 4, 5 and 6, so it
never spends their draws. Two runs that reject at different points therefore
diverge in the stream from that moment, and **every later candidate differs**
— not just the one that disagreed.

That makes the whole comparison above weaker than it looks. It confirms our
252 rejections are individually defensible; it cannot confirm the *set* is
right, because a cell where vanilla rejects and we accept leaves no
rejection record on our side and is invisible from here. One such cell early
in the walk would reshuffle everything after it while every rejection we do
record still checks out.

So the next instrument is not another check of our own rejections. It is a
way to compare **candidate sequences** — and part of that turns out to be
recoverable after all.

### We produce vanilla's origin, and the lake there still abandons

A lake's box is anchored at `origin - (8,4,8)`, its mask lives in local
1..14 x/z and 1..6 y, and its `cave_air` half is at local y >= 4. Vanilla's
lake body — `cave_air` at x997..1005, y−35..−34, z992..1002 — therefore pins
its origin to `x 999..1004, y -37..-35, z 996..999`.

Logging the origins that survive our chain gives four, and one of them is:

```text
  REACHED at (1003,-35,997)
```

inside that box on all three axes. Its own box spans x995..1010, y−39..−32,
z989..1004, which contains vanilla's `cave_air` exactly. **That is vanilla's
lake origin, and our chain produced it.**

So the placement chain is not losing the lake. The feature ran at the right
place and abandoned — at (1006,−36,994), a shell cell three blocks up from
the box floor, so below the waterline, where the wall must be solid and the
block is `minecraft:air`. And that cell **agrees with vanilla**.

Same origin, same terrain at the failing cell, and opposite outcomes. The
only remaining input is the **mask**: which cells are shell at all. It is
built from `nextInt(4)+4` rounds of six draws, so it is a pure function of
the stream position when `place` is entered — and that position is set by
how many draws every earlier candidate spent, which is the stateful walk
again.

That is now a much sharper statement than "the candidate sets might differ":
the sets agree at least at the origin that matters, and the divergence is in
the *stream offset* by the time it is reached.

**And the offset does not come from the modifiers, which corrects the
"stateful walk" reading above.** Counting `sulfur_pool`'s chain by hand:

```text
  count 256              constant IntProvider   0 draws
  in_square                                     2 draws
  height_range uniform                          1 draw
  block_predicate_filter (solid)                0    <- rejects
  environment_scan                              0    <- rejects
  random_offset  xz 0, y -1  both constant      0
  block_predicate_filter (sulfur)               0    <- rejects
  biome                                         0    <- rejects
```

Every drawing modifier comes **before** every rejecting one, and none of the
rejecting ones draws. So each of the 256 candidates spends exactly three
draws whether it survives or not, and the stream offset at candidate *k* is
`3k` regardless of what was rejected. Rejections cannot desynchronise this
chain.

What *can* is the feature itself. `LakeFeature` builds its mask before the
validity pass — `nextInt(4)+4` rounds of six draws — so a candidate that
**reaches** the feature spends those draws and one that was rejected spends
none, and an abandoned lake spends them too, because the mask is built first.

So the offset at our origin is `3k + (mask draws of every survivor before
it)`, and the divergence is a difference in the **number of survivors ahead
of it**, not in the rejections themselves.

### The root cause: a neighbour's step 7 lands before this chunk's step 1

Logging the origins that survive gives four for chunk (62,62), and vanilla's
origin `(1003,-35,997)` is the **second**. So exactly one survivor precedes
it — and that one is a cell where the two worlds disagree:

```text
  REACHED (992,-15,1005)      vanilla cinnabar   ours sulfur
  REACHED (1003,-35,997)   <- vanilla's lake origin
```

Our already-sulfur gate accepts it because our block there is sulfur.
Vanilla's rejects it, because vanilla's is cinnabar. `RUSTMC_TRACE_WRITE`
says where ours came from:

```text
  WRITE (992,-15,1005) <- minecraft:cinnabar  by ? (no feature running)
  WRITE (992,-15,1005) <- minecraft:sulfur    by minecraft:sulfur_spike from chunk (61, 62)
```

**A neighbour's `sulfur_spike`, which is a step 7 feature, wrote it — before
chunk (62,62) ran its step 1.** This pipeline decorates chunk by chunk in
x-then-z order, so (61,62) finishes all eleven of its steps before (62,62)
begins, and (62,62)'s pool reads a world that already contains its
neighbour's spikes.

The whole chain then follows mechanically: our gate passes, a doomed lake
runs and spends `1 + 6*rounds` mask draws, the stream is offset by the time
it reaches the real origin, that origin gets a different mask, its shell
includes a cell holding air, and the validity pass abandons — where vanilla,
whose stream was never offset, succeeds.

**This is cross-chunk decoration ordering, and it is systemic.** Nothing
about it is specific to sulfur: any feature that reads a block a
later-numbered step can write is exposed to it, and the exposure is decided
by which chunks were decorated first. It is the same family as the margin
bug fixed earlier today — an ordering property of the harness masquerading as
a generator fault — and it belongs with ORACLE.md §6.

#### Confirmed by experiment, and it does not have a fix by reordering

`RUSTMC_DECORATE_ORDER` already offers four alternatives to the default
raster. Running all five against the same window:

| order | window mismatches | (62,62) `potent_sulfur` | `cave_air` |
|---|---|---|---|
| raster (default) | **557** | 0 | 0 |
| reverse | 7,819 | **1** | **119** |
| zx | 1,872 | 0 | 0 |
| spiral | 2,983 | **1** | **119** |
| spiral_in | 3,926 | 0 | 0 |
| *vanilla* | — | *1* | *119* |

**Two independent orders reproduce vanilla's lake exactly** — the same single
`potent_sulfur` and the same 119 `cave_air`. The mechanism is therefore not
in doubt: whether that lake exists is decided by the order chunks are
decorated in, and nothing else.

**And every order that recovers it is far worse overall** — 3x to 14x the
mismatches. Raster is the best global fit by a wide margin and is the one
that loses this lake.

So there is no reordering that fixes this. Either vanilla's order is none of
these five, or — more likely, given that raster wins so decisively on the
total — the remaining residual is **order noise rather than port error**, and
this lake is one draw of it. That is the same floor already measured from the
other side: two *vanilla* worlds generated at different centres differ by
4,885 blocks, and driving the forceload order made the agreement worse, not
better.

The strategic conclusion matters more than the lake. Chasing an individual
order-sensitive disagreement is chasing the floor, not a bug, and the 557
should be read as containing an unknown amount of it. Separating the two is a measurement nobody had taken, and it is takeable:
diff **every** order against vanilla, keep the positions, and intersect.

```text
  raster (default)      557 mismatched positions
  reverse             7,819
  zx                  1,872
  spiral              2,983
  spiral_in           3,926

  wrong under ALL five      202     <- order-invariant
  wrong under ANY         9,564
```

**202 blocks are wrong whatever order is used.** That is the port-error core
for this window: 202 / 2,457,600 = **99.991782%**. And **355 of raster's 557
— 64% — are order-sensitive**, correct under some other equally defensible
order.

Two things that follow, and the second is the one that changes how the
project's headline number should be read:

- The residual worth engineering against is **202, not 557**. Everything in
  the 355 is a coin-flip on a visiting order, and today's five-layer chase
  ended inside exactly that set.
- **Any single-order parity figure carries an unmeasured order component.**
  Here it is nearly two thirds. There is no reason to think the r7 and
  village-window numbers are cleaner, and they have never been decomposed
  this way.

Stated at its real strength, because the intersection is not a proof: a block
wrong under all five orders could still be order noise if vanilla's order is
a sixth one, and a block right under raster is not thereby correct. So 202 is
a **lower bound on port error** and 355 an **upper bound on what reordering
could recover** — but the gap between 202 and 557 is real either way, and it
is the first time this project has been able to say how much of a parity
number is not about the port.

#### Repeated on a 49-chunk window, and the rate is stable

Chunks 61..67 — every chunk whose full neighbourhood the local vanilla
reference contains — is 4,816,896 blocks, nearly double:

```text
  raster              2,037    99.957711%
  order-invariant       397    99.991758%
  union              16,605
  order-sensitive in raster: 1,640  (81% of it)
```

Put beside the smaller window:

| window | raster | order-invariant |
|---|---|---|
| 25 chunks | 99.977336% | **99.991782%** |
| 49 chunks | 99.957711% | **99.991758%** |

**Correction to the 49-chunk row: it is contaminated by the reference's own
edge.** The local vanilla dump covers chunks 60..68 and nothing outside, so
its chunks 60 and 68 decorated without their outward neighbours and spill
wrongly into 61 and 67. Measured: the order-invariant spike mismatches run
7.8 per chunk in the outer ring (61, 67) against 5.3 in the interior.

Restricting the same five-order intersection to chunks 62..66 gives **202**,
which is the number the independent 25-chunk run produced — 99.991781%
against 99.991782%, differing only in rounding. Two separate dump sets, one
answer. The usable window against this reference is 62..66, and 397 should
not be quoted.

**The single-order figure moves by 0.02 percentage points between windows.
The order-invariant figure agrees to six decimal places.** That is the
signature of two different quantities: one is a property of the port and is
stable, the other is a property of which chunks were looked at and how they
were visited, and is not.

It also means the honest parity claim is materially better than the one this
project has been quoting. **~99.9918% order-invariant**, against a launch bar
of 99.9%. The bar is met on the measure that is about the port — and the
number that has been reported, and chased, is the one containing up to 81%
noise.

#### Widened to seven orders: still 202, and two of them are the same world

Two more orders were added — `zx_rev` (z-major, x descending) and `diag` (an
anti-diagonal sweep) — to test whether the "order-invariant" core is really
invariant or just invariant across the five orders that happened to exist.

```text
  zx_rev   4,312 mismatches
  diag       557 mismatches
  seven-order intersection: 202     (unchanged from five)
```

Adding them removed nothing. The core is stable at 202 across seven orders,
which is the strongest evidence yet that it is port error rather than noise.

**And `diag` produced a world byte-identical to raster** — zero differing
positions, not merely the same count. That is not coincidence and it is worth
keeping: a feature reaches at most one chunk, so the only thing an order can
change is **the relative order of the eight neighbours**. Raster (x-major)
and an anti-diagonal sweep induce the *same* relative order on all eight, so
they are the same world. The space of meaningfully distinct decoration orders
is far smaller than the space of orders, and enumerating orders is not the
same as enumerating worlds.

The caveat stands and should travel with the figure: seven orders is not all
orders, so 397 remains a lower bound on port error. But two independent
windows agreeing to six decimals is strong evidence that the bound is a tight
one, and far stronger evidence than any single-order number has ever carried
here.

#### What the 397 actually are — one family

Breaking the order-invariant core down by pair:

```text
  109  cinnabar -> sulfur
   97  sulfur -> cinnabar          <- 206 together, 52%
  ~140  sulfur_spike present/absent, both directions, mostly `tip`
```

and `RUSTMC_TRACE_BLOCK` at an order-invariant swap cell, (990,−25,986),
shows the two are the same thing:

```text
   true  Biome(["minecraft:sulfur_caves"])   (biome minecraft:sulfur_caves)
   true  NoiseThreshold  -0.4 .. -0.1        -> cinnabar
```

```text
  y -27   decorated sulfur_spike[thickness=base,vertical_direction=down]
  y -26   surface cinnabar -> decorated sulfur
  y -25   surface cinnabar -> decorated sulfur
```

Our surface rule writes cinnabar **correctly** — the biome and band both
hold — and then a speleothem's base column overwrites it with its
`base_block`, which for `sulfur_spike` is `minecraft:sulfur`. So the
cinnabar/sulfur swaps *are* the spikes' base columns and the present/absent
rows are their tips. **The whole order-invariant core is one bug family:
speleothem placement.**

Census over the same 49 chunks, by thickness — which decomposes the height
distribution, since `buildBaseToTipColumn` emits `base` only at height 3+,
`frustum` at 2+, `tip` at 1+:

| thickness | vanilla | ours | delta |
|---|---|---|---|
| base | 203 | 211 | +8 |
| middle | 56 | 58 | +2 |
| frustum | 521 | 533 | +12 |
| tip | 1,601 | 1,632 | +31 |
| **total** | **2,449** | **2,507** | **+58 (2.4%)** |

heights: vanilla `h1=1080 h2=318 h3+=203`, ours `1099 / 322 / 211`.

**A roughly uniform 2% surplus across every height**, which *refutes* the
short-speleothem reading taken earlier from chunk (62,62) — that chunk was
inside the order-sensitive 81% and its skew was noise. And ~140 positional
mismatches against only 58 net surplus means the fault is mostly
**displacement**, not over-production: a speleothem in the wrong place costs
two mismatches, one at each end.

That is the work list, and it is one item.

Note where these values sit: +0.004, +0.006, +0.010, all a hair above the
band's lower edge of 0.0. The remaining 33 fall in the gap `(-0.1, 0.0)`
where the rule legitimately produces neither sulfur nor cinnabar. So this is
a **boundary** effect at zero, not a wholesale failure of the branch.

The chain therefore runs: surface rule writes deepslate where its own noise
says sulfur -> the pool's already-sulfur gate rejects those origins -> no
lake in (62,62) -> no `cave_air`, no water -> step 7 finds unbroken rock and
places 15 extra spikes. One fault at the surface, four layers of consequence.

What it is not: the noise value, the band edges, or the comparison — all
three were read against the jar earlier and match. What is left is *why the
branch did not fire* at a position whose noise condition it satisfies, and
the sulfur_caves branch has other conditions in front of the noise one. Those
are local questions.

Worth noting what is *not* in doubt: the biome. The window has zero biome
mismatches, and the surface rule that paints sulfur and cinnabar in (62,62)
is gated on `sulfur_caves`, so that chunk is in the same biome as its
pool-placing neighbours. Whatever rejects the pool is positional, not a
missing entry in a biome's feature list.

## Known remaining work, roughly in expected-value order

1. **Split the fill measurement** before optimising it. It is 56% now and
   nothing in it has been touched.
2. **`cache2d`.** Same shape as `cacheOnce`, smaller win, attacks setup.
3. **Stop deep-cloning the density graph per chunk.** Measured at 4%, so
   worth doing but not first. Requires numbering the graph once per world
   instead of once per chunk, which is a real refactor for a small return.
4. **Flatten the density enum into an instruction array.** Documented in
   `density/mod.rs` as the post-parity plan: an enum tree can be walked
   iteratively without changing semantics, and the tree walk is currently
   pointer-chasing.
5. **Per-worker buffer reuse.** Parallel scaling falls to 78% efficiency by
   8 threads, and each chunk allocates and touches ~300 KB. Reusing buffers
   per worker is the obvious next move, and would also remove the 7%
   per-chunk graph clone if the graph is numbered once per world.

Items 1-3 are recovering work vanilla already avoids. Items 4-5 are where a
Rust implementation should be able to go past it rather than draw level.

## The (64,64) window after the shared-per-chunk RNG — 99.999268%

Measured 2026-07-27 against `oracle-work/t2-regiondiff/a`, same 25-chunk
square, same seed, immediately after `run_feature` stopped allocating its own
`WorldgenRandom`:

```text
  DIFF chunks=25  blocks=2,457,600  mismatches=18  biomes=38,400/0
  EXACTNESS 99.999268%
```

Against **498** mismatches and 99.979736% before it. The sulfur-against-
cinnabar cluster that was ~91% of the diff — 320 direct swaps plus ~130
sulfur-spike presence differences — is gone entirely, and so is the andesite
patch's neighbourhood noise.

The cause was not in any feature. `ChunkGenerator.applyBiomeDecoration`
allocates one `WorldgenRandom` per chunk and calls `setFeatureSeed` on it per
feature; this port allocated one per feature. That reproduces every integer
draw, because `setFeatureSeed` fully determines the bit stream — which is
exactly why it survived every gate for so long — but not the gaussian cache,
which `WorldgenRandom.setSeed` never resets. A feature drawing an odd number
of gaussians leaves half a pair behind for the next feature to consume as its
first gaussian, and a per-feature source always starts empty.

### What the remaining 18 are

| n | difference | note |
|---|---|---|
| 10 | `diorite` -> `andesite`, y 18–21 | the long-standing patch, now the largest single item |
| 3 | `chest[facing=north]` -> `chest[facing=south]` | mineshaft chest orientation |
| 2 | `sulfur_spike[tip]` -> `water` | spike tips in water |
| 1 | `cobweb` -> `sulfur` | |
| 1 | `air` -> `water` | |
| 1 | `deepslate_lapis_ore` -> `deepslate[axis=y]` | |

Nothing here is a cluster. Each is now small enough to be chased as an
individual case rather than as a distribution, which the sulfur diff never
was.

## Order-invariant on the (64,64) window: 4 mismatches, 99.999837%

Measured 2026-07-27, seven decoration orders, same 25-chunk square and the
same `oracle-work/t2-regiondiff/a` reference:

```text
  raster     18      zx        1322      spiral      3267
  diag       18      zx_rev    4088      spiral_in   3424
  reverse  7648
```

Intersection over all seven: **4** positions.

```text
  (1020,   5, 1071)  air                 -> water
  (1038,  -6, 1005)  cobweb              -> sulfur
  (1050, -45,  999)  chest[facing=north] -> chest[facing=south]
  (1059, -38,  999)  deepslate_lapis_ore -> deepslate[axis=y]
```

`4 / 2,457,600` = **99.999837%**, against a definition-of-complete bar of
99.99%. Down from **202** on the same methodology before the per-chunk RNG fix.

**Why the non-raster orders look so much worse, and why that is not a
regression.** The vanilla reference was generated in vanilla's own chunk
order. Any order of ours that visits neighbours differently disagrees with it
wherever two chunks' features touch the same block — that is the mechanism
this knob exists to expose, not a fault. The inference is the established one:
a block that matches vanilla under *some* order is a block this generator can
produce correctly, so the disagreement is attributable to chunk ordering; a
block that is wrong under *every* order is port error. Only the intersection
is a parity claim.

`raster` and `diag` agree at 18 because `diag` yields a byte-identical world —
a feature reaches at most one chunk, so only the relative order of the eight
neighbours matters, and those two orders induce the same one. Six distinct
worlds, not seven.

### What the 4 are, and what they are not

The 10-block `diorite` -> `andesite` patch that dominated the raster diff is
**not** among them, which is consistent with the direct check: replaying
chunk (67,63) against its own oracle is exact — 110 attempts, 6,760 blocks,
zero mismatches — and both the diorite and andesite blob origins match vanilla
to the block in both chunks, with matching draws. It is cross-chunk decoration
order, not a port defect.

Two of the three raster chest mismatches also fall away; one survives.

## The 121-chunk window, against two vanilla worlds — 104 blocks, 99.999126%

Six decoration orders on chunks 59..69 (centre 64,64, seed 20260722) against
`oracle-work/vanilla-r7`:

```text
  raster    1,533     zx        4,383     spiral      16,665
  reverse  38,320     zx_rev   25,296     spiral_in   13,161
```

Order-invariant core: **226**. Then each core block was checked against a
*second* vanilla world, `van-rows-c64` — same seed, same 225 chunks, generated
in a different order:

```text
  our value matches the other vanilla world exactly   122
  differs from BOTH vanilla worlds                    104
```

`104 / 11,894,784` = **99.999126%**. Zero biome mismatches over the window.

The 122 are not defects to fix. Vanilla disagrees with itself at 21,556 blocks
across those chunks, and at every one of the 122 our answer *is* one of
vanilla's two answers. The whole kelp/seagrass family — which dominated the
raw core — lands here.

### What the 104 actually are

| n | pair |
|---|---|
| 36 | `sand` -> `gravel` |
| 15 | `gravel` -> `sand` |
| 10 | `air` -> `water[level=0]` |
| 9 | `air` -> `water[level=1]` |
| 8 | `deepslate_lapis_ore` -> `tuff` |
| 7 | `deepslate_lapis_ore` -> `deepslate[axis=y]` |
| 5 | `deepslate_diamond_ore` -> `deepslate[axis=y]` |
| 5 | `deepslate_gold_ore` -> `deepslate[axis=y]` |
| 3 | `deepslate_gold_ore` -> `tuff` |
| 3 | `sulfur_spike[thickness=tip_merge]` -> `[thickness=tip]` |

Three families, not ten: **sand/gravel disks (51)**, **deepslate ore veins
(28)**, and **water at y 16 (19)**.

## The unattributed ~14%: the candidate set narrowed to one group, and the one run that decides it (2026-08-17)

**No timing was taken for this section and none of it is a measurement of
speed.** The box was at load 18.6-19.6 throughout with a merge gate and an
end-to-end lane test both running, which is exactly the condition this file
says makes a wall figure meaningless; the standing instruction was to do the
analysis and leave the stopwatch alone. What follows is the candidate set, the
mechanism test each candidate has to pass, and the measurement that separates
them — so that the next quiet window is spent once rather than six times.

### What is open

The section above closes `C->D` at **+7.03% instructions/chunk** at r=7 and
attributes the whole of the stale-tenth drift to the trial chambers. Against
today's tip at that time (`35b3bde5`) the same metric reads **+22.44%**, so
roughly another **+14%** landed between `9b4afd4e` and `35b3bde5` and nobody
has attributed it.

### The span, and the first thing ruled out

`9b4afd4e..35b3bde5` is **247 commits, of which 18 touch
`crates/rustmc-worldgen/src`**. The obvious non-code explanation is checked
and dead: **the release profile did not change in the span** — `git diff
9b4afd4e 35b3bde5 -- Cargo.toml` contains no `opt-level`, `lto`,
`codegen-units`, `panic` or `debug` line. A profile change would have produced
exactly the observed shape (uniform, everywhere, surviving at every radius)
for no source reason at all, so it is worth ruling out before reading any
diff. It is ruled out.

### The 18, grouped by the stage they are paid in

| group | commits | mechanism | paid |
|---|---|---|---|
| **A — surface biome id** | `6120623f`, `e96f9ac1`, `48e63734`, `01578f5d`, `badb8d3d` | resolve the biome id once as an integer; build the palette and the climate table in lockstep | the **surface** stage, every chunk |
| **B — ruined portal** | `f79ae495`, `01b33fe3`, `679c0c09`, `cafbfae5` | `RuinedPortalPiece.postProcess`, its processors, and `final_state` resolved through `parse_for_block` | structure placement, only where a portal lands |
| **C — the apron** | `78b9efc9`, `f6553930`, `b7c820fb` | a new feature and its draw counter | decoration, where the feature runs |
| **D — rotation and no-ops** | `cd992dba`, `31d2c15d`, `f12f3ed3`, `df303349`, `cafbfae5` | one rotation rule, two clippy edits, a 37-block fix | negligible or nowhere |

### The mechanism test, which is the same one that convicted the chambers

The chambers were convicted as a **per-chunk placement scan** and not as the
price of a chamber landing, by two predictions made before the sweep: an r=7
square expects 0.31-0.94 chamber starts, so a per-generation cost would be
**lumpy** — one centre large, two near 1.0 — and all three centres moved
together instead; and a fixed per-process cost would **dilute** as the square
grows, where +4.08% survived at r=16. Any candidate for the +14% has to clear
the same bar.

**That bar demotes group B on its own evidence.** `parse_for_block` has
exactly **one production call site** in the whole crate —
`crates/rustmc-worldgen/src/structure/place.rs::block_age_with` (`:2118`), inside ruined-portal `final_state` resolution — and
a ruined portal is rarer per square than a trial chamber. A cost paid there
cannot be uniform across three centres and cannot survive a 4.8x larger
square. Group C is the same shape for the same reason: a feature's cost is
paid where the feature runs.

**Group A is the only one whose cost is paid on every chunk**, and it is also
the one commit in the span that says so itself: `6120623f`'s subject line ends
**"not yet measured"**. It was written as an optimisation — resolve the biome
id once, as an integer — and an optimisation that was never measured is as
likely to have gone the other way as not. It is the leading candidate on
mechanism, and nothing here says it is guilty.

### The one run that decides it, and the instrument that must not be used

The stages are already separable and the separator needs no new code.
`RUSTMC_THROUGH=noise|surface|carvers` truncates the pipeline, so **group A's
cost is inside `surface` and groups B and C are entirely outside it**. Four
short runs settle it, all at `RUSTMC_WORKERS=1` with INSTRUCTIONS RETIRED as
the metric — contention-immune, which is why this file uses it — two on the
tip and two on `9b4afd4e`:

```text
  RUSTMC_THROUGH=noise     tip vs 9b4afd4e     expect ~0
  RUSTMC_THROUGH=surface   tip vs 9b4afd4e     group A lands here, or it does not
```

If the surface arm carries the increment, bisect the five commits of group A —
three measurements at most. If it does not, the increment is in decoration or
structure placement and groups B and C are back, with the lumpiness their
mechanism predicts to look for.

**Compare like with like and never across arms.** `RUSTMC_THROUGH` produces
**beardless terrain** (`chunk_gen.rs`'s own header says so), so a truncated
run is not a full run with a stage removed and the difference between the two
arms is not "the cost of the surface stage". Only tip-against-baseline **at
the same `RUSTMC_THROUGH`** means anything here.

**Do not reach for `RUSTMC_STAGE_TIMES`.** It is on this project's rule-7
list because it **forced the surface stage sequential** — the instrument
disabled the thing it was measuring — and the surface stage is precisely what
this run is about. At `RUSTMC_WORKERS=1` the sequentialisation is moot anyway,
which removes the only reason anyone would have wanted it.

### What would make this section wrong

The tip has moved: **36 further commits touch `crates/rustmc-worldgen/src`
between `35b3bde5` and `b7ff97fc`**, none of them in the +22.44% measurement.
So the increment measured against today's main is not the increment this
section is about, and re-measuring against today's tip without re-taking the
baseline would fold the two together. Take the baseline arm at `9b4afd4e` in
the same session, on the same box, and record the load with every number.

### The "18" undercounted — it is 24, and the six extra fall the same way B and C did (2026-08-17)

**Read, not measured.** The 18 above only scanned
`crates/rustmc-worldgen/src`, but the byte-identity proof at the top of this
file names `worldgen-dump`'s real dependency closure as four paths:
`crates/rustmc-worldgen`, `crates/rustmc-world`, `crates/rustmc-data` and
`tools/worldgen-assembly`. Scanned the same way over the same span:

```text
  crates/rustmc-worldgen/src      18
  crates/rustmc-world               2
  crates/rustmc-data                 0
  tools/worldgen-assembly          13
  union (dedup)                    24
```

The six not already in the 18 are `2ec2377b`, `36f6d432`, `50fc4220`,
`6f3a0abe`, `d6b6abd2`, `f87d23f4` — all 2026-08-06/09, all village or ruined-
portal work in `tools/worldgen-assembly/src/generate.rs`, which is not a
table-builder but the actual pipeline `worldgen-dump` calls
(`worldgen_assembly::generate::generate`, confirmed at
the `generate` call in `tools/worldgen-dump/src/main.rs::column` (`:1436`) and five more call sites).

**They do not reopen the noise/surface test.** `villages`, `ruined_portals`,
`trial_chambers`, `mineshafts`, `ocean_ruins`, `shipwrecks` and `end_cities`
are every one of them gated `if through >= Through::Decoration` in
`generate.rs` (lines 2294, 2338, 2388, 2459, 2483, 3790, 1424), and
`pipeline::Through` orders `Noise < Biomes < Surface < Carvers < Decoration`
(`crates/rustmc-worldgen/src/pipeline.rs::Through` (`:158`)). None of the six can execute
under `RUSTMC_THROUGH=noise` or `=surface`, so the run below still isolates
group A cleanly if it comes back negative.

**But they widen what "group B" means if it comes back negative.** `50fc4220`
is not the `parse_for_block` call group B was demoted on — it is
`ruined_portal_starts`, the structure-set scan itself (RNG draws, template
selection, biome checks), fixing which stub position vanilla's own bytecode
uses. That function runs once per window over the whole spacing grid, which
is the same shape as the trial chambers' jigsaw walk, not the same shape as
`parse_for_block`'s single landed-portal call site. **Read alone, this commit
is a correctness fix (swaps which coordinate feeds `stub`) with no obvious
added loop or allocation** — the same reading group A's five commits get
below — so static reading does not convict or clear it either. If the
noise/surface run shows no increment in group A, the decoration-side rerun
needs to test these six alongside B and C, with the same lumpy-vs-uniform
prediction the chambers were convicted on, not just B's four.

**Group A's own diff, read rather than assumed.** All five commits were read
end to end for this section. Every change removes an allocation or a second
map walk (`context.rs`'s `biome()` now returns `Copy` `BiomeId` instead of an
owned `String`; `world.rs`'s gradient lookup drops a `contains_key` +
index double-walk to one `get`), and `Condition::BiomeIds(wanted).contains()`
is the same O(n) linear scan the old `wanted.iter().any()` was — no
complexity change. Nothing in the diff reads as slower than what it replaced.
**This does not clear group A** — three of its five commits say "not yet
measured" in their own subject or body, and a hand-read diff is exactly the
instrument rule 9 says cannot close a question like this on its own — but the
leading candidate does not have a visible smoking gun either. The run below
remains the only thing that decides it.

### The quiet window opened, closed nine minutes later, and no arm was taken clean (2026-08-18)

**No attribution figure for the +14% exists after this session either.** The
box rebooted at 00:04, load was 1.7-2.9 through 00:04-00:11, then the
supervisor-driven gate chain relaunched and load was back over the 4.0
ceiling by 00:11:39 and never returned. Three real attempts were made in the
window; none produced usable data, for three different reasons, recorded
here so the next window is not spent on the same three:

```text
  00:09:28  load 3.01  both arms "completed" in 19s total   -> FAKE: rustmc-wgperf14-35b3bde5
                                                                 (git-archive tree) had no
                                                                 oracle-work/, tip crashed on
                                                                 rep 0, script still exited 0
  00:10:32  load 3.00  both arms "completed" in 19s total   -> FAKE: same tree also had no
                                                                 data/vanilla/ (block_props.json,
                                                                 17.7 MB, also gitignored),
                                                                 different crash, same result
  00:11:58  load 3.99  killed by the operator's own `| head -5`
                        on the launch command, SIGPIPE before
                        either arm produced a row              -> LOST, not the box's fault
  00:12:17  load 9.49  refused, correctly                      -> window closed
```

**Two real gaps, now fixed for good:** `ci/bench-stamp-sha.sh` verifies
`crates/`, `tools/` and `Cargo.toml` but a `git archive` tree has neither
`oracle-work/` nor `data/vanilla/` — both are `/oracle-work` and
`/data/vanilla` in `.gitignore`, so a freshly-archived bench dir builds clean
and then crashes on the first `worldgen-dump full` call, and
**`bench-attrib-through.sh`'s own `for through in noise surface` loop does not
check either arm's exit code**, so both false starts above printed a full
"uptime after" / "data:" footer and `exit 0` while every row in the TSV was
the discarded warmup rep — a script that reports success on an all-crash run
is this project's central defect by the same door as the mtime hazard.
`rustmc-wgperf14-35b3bde5` now has both symlinked to the same data
`rustmc-benchB` already used (`oracle-work/vanilla-26.2` and `data/vanilla`
are vanilla-26.2 extracts, sha-independent of the two commits being compared,
so pointing both bench dirs at the same copy does not contaminate the
comparison). **Not yet fixed:** `bench-attrib-through.sh` should check
`bench-pair.sh`'s exit code per arm and refuse to write a summary footer on a
run where every rep failed; left for whoever gets the next window, since
fixing it blind under load with no way to re-run it would be the same
unfalsifiable-check mistake by another door.

**The falsification this session did do**, on `ci/bench-pair.sh` itself
(the layer `bench-attrib-through.sh` calls), because a benchmark that cannot
show a regression is the defect above, and this file's own item requires it
of anything the session touches:

```text
  base vs base (identical binary, twice)     PAIRED RATIO 1.0132 (+1.32%)
                                              95% CI 0.70 .. 1.46, n=3
                                              VERDICT: UNRESOLVED (straddles 1.0)

  base vs base+injected busy-loop            PAIRED RATIO 1.3422 (+34.22%)
    (30M-iteration awk spin prepended,       95% CI 1.22 .. 1.47, n=3
     exec'd into the real binary,            VERDICT: SLOWER by 34.22%
     same PID, same perf-stat counters)        (CI excludes 1.0)
```

r=1, reps=1, load 6.7-9.0 throughout (the gate chain was already back) —
admissible for this because it is a mechanism proof of the script, not an
attribution figure, same standing as the `NOT QUOTABLE` smoke test above.
Injection created and destroyed in `~/rustmc-fakeslow` this session; nothing
persists.

**Status: unchanged.** The +14% is still attributed only to "group A on
mechanism, not yet measured" per the section above. `9b4afd4ee56ff253c46987b237edc8a2a7b43003`
(`rustmc-benchB`) and `35b3bde541ff42c928ebc41067baea83dbd43872`
(`rustmc-wgperf14-35b3bde5`) are both stamped, both now have the data
directories the binaries actually need, and the four-arm command is unchanged
from the cold-start doc. The next quiet window costs one command.

### The arm check that would have caught both fakes, proven three ways (2026-08-18)

The section above closes with an admission: `ci/bench-attrib-through.sh` was
still able to report a full "uptime after" / `data:` footer and `exit 0` on a
run where every rep crashed, and fixing it was left for whoever got the next
window. It is fixed now, and it is falsified rather than merely written —
which matters more here than usual, because the thing being fixed *is* a check
that could not fail.

Two independent reasons an arm is now disbelieved, and it must pass both:

```text
  rc == 0                 taken from PIPESTATUS[0], not $?. The runner's
                          output goes through `| tee`, so a bare $? was
                          always tee's status and was always 0. This alone
                          is what let two all-crash arms print success.
  rows == reps*centres*2  the ABBA design writes exactly one row per
                          (rep, centre, arm) and deletes the warmup. centres
                          is read back out of the header ci/bench-pair.sh
                          itself wrote, so the count is not a constant
                          duplicated across two scripts.
```

The second condition is not redundant with the first. `bench-pair.sh` exits 1
on a failed `run_one`, so a total crash is caught by rc alone; a run that dies
*after* its last successful rep, or one whose final rep succeeds while an
earlier one was lost, exits 0 with a short table. Only the row count sees that.

The falsification, run this session — `BENCH_ATTRIB_THROUGH_FAKE_ARM_FAIL` is
read only when set, and takes either `1` (substitute `/bin/false`) or a path
to a stand-in runner, so both halves can be shown red without a crashing
binary or a busy box:

```text
  runner exits 1, no TSV written        noise+surface ARM FAILED  rc=1 rows=0/0    exit 1
  runner exits 0, writes 35 of 36 rows  noise+surface ARM FAILED  rc=0 rows=35/36  exit 1
  runner exits 0, writes 36 of 36 rows  noise+surface ARM CLEAN   rc=0 rows=36/36  exit 0
```

The middle row is the one worth keeping: it is a runner that succeeded, wrote
a well-formed header and a table one row short, and the old code would have
summarised it without comment. The summary now ends with an explicit
`# arms clean:` / `# arms NOT clean:` pair and the line "a figure may be
quoted ONLY from an arm listed clean above", so a partial result is labelled
at the point of use rather than in someone's memory of how the run went.

**Also recorded, because it sizes every future window:** one
`worldgen-dump full 20260722 7` at `RUSTMC_THROUGH=surface RUSTMC_WORKERS=1`
takes **11.1s** on this box (real 11.135s, user 10.969s). The full four-arm
design is 2 warmup + 36 measured runs per through-value, so the run needs
roughly **14 minutes of quiet**, not the nine minutes the 2026-08-18 window
offered. That is why the window is now waited for rather than hoped for.

### The decisive arm now runs FIRST, because every window this item has had was short

`ci/bench-attrib-through.sh` ran `noise` then `surface`, and the gate is
re-checked before each arm so a window that closes mid-run stops the run.
That order spends a short window on the arm that decides nothing.

Group A — the only candidate paid on every chunk, and the one this item is
down to — is **inside** the surface stage. Groups B and C are gated
`>= Through::Decoration` and cannot run under either arm. So `surface` is the
arm that attributes the +14%, and `noise` is its control: it says whether
anything moved *below* the surface stage, which is what makes a surface-arm
difference attributable rather than whole-pipeline drift.

The windows this item has actually been offered, all of them refused or lost:

```text
  2026-08-17  load 17.8-30.8, gate chain + TmForever.exe   refused, correctly
  2026-08-18 00:04-00:11  ~7 min at load 1.7-3.0           2 fake passes, 1 SIGPIPE
  2026-08-18 07:15-07:23  ~8 min at load 2.9-3.2           lost to a load spike at 5.2
```

**Not one of them was 14 minutes.** With `surface` first, a half-length window
yields the arm that decides the item plus an honest `# arms NOT clean: noise`,
which is worth more than a control with nothing left to control. With the old
order the same window yielded the control and nothing to apply it to.

This changes no measurement and no threshold — the 4.0 ceiling is untouched
and the per-arm re-check is untouched. It changes which arm is attempted
first, and that is a scheduling decision the file should record because the
next agent will otherwise read the arm order as arbitrary.
