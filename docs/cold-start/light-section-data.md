# COLD START — branch `light-section-data`

### COLD START — branch `light-section-data`, 2026-08-18

**The area: the section light data every chunk packet carries, of which we
classified none.** `docs/PARITY-ROADMAP.md` §75. Built on
`origin/chunk-send-path` (23 commits, T0 GREEN), which owns the send *order*
and the packet's other fields and explicitly left light here.

### The headline

**0 of 26 sections classified becomes 26 of 26.**

    section-light classification, vanilla first:
      void 468/468   superflat 468/468   roofed 468/468   emitters 468/468
      overhang 468/468   tower 468/468   sealed_dark 468/468
      slab_roof 468/468
      TOTAL 3744/3744 sections agree with vanilla

26 is `LevelLightEngine.getLightSectionCount()` = `getSectionsCount() + 2`,
verified from bytecode. 3744 is 8 worlds x 9 chunks x 2 layers x 26 sections
(the ninth, `relit`, is a history case and is graded separately).

### The observation point, and why the corpus is not it

Vanilla's own `LevelLightEngine`, in process, driven through
`ThreadedLevelLightEngine`'s exact sequence, dumping the packet's own light
section through `ClientboundLightUpdatePacketData(pos, engine, null, null)`.
`ci/light-golden.sh`, on the `ci/ore-golden.sh` pattern: compiled against the
unsigned jar, run twice, `cmp`'d.

Two structural reasons it is not a gameplay scenario, and **both are measured,
not argued**:

* `trace::observe` records a chunk packet only with a non-empty block-entity
  list — 132 in the window, 1 recorded (§70). Correct filter; it stays.
* `ci/t2-gameplay.sh` runs on superflat, and **superflat sky light takes
  exactly two distinct nibble values, {0, 15}**, across all 18 of its arrays.
  A value comparison there is `710`. By contrast `overhang` gives 10 distinct
  values and `emitters` block light gives all 16 — those two are what can
  grade values at all.

**`slab_roof` is the case that earns its place least obviously and mattered
most.** Counting the 6,762 shape-occluding states establishes how many a
dampening-only engine COULD be wrong about, never that it is wrong about one.
This case turned the count into a reading, and it failed until
`shape_occludes` existed.

### The radius threshold

`ci/light-radius-sweep.sh`, radius 0..4, ORACLE rule 14:

    void 0  superflat 0  roofed 0  relit 0  tower 0   emitters 1  overhang 1
    converged context radius over all cases: 1

**One, not the chunk-send lane's four.** `--view-distance 4` is ample for
light. What blinds the corpus is the block-entity filter, not the radius.

Five of seven cases converge at 0, and the sweep prints
`<- grades no cross-chunk propagation` against them itself — a suite of only
those would be green against a port with no horizontal propagation at all.

**The sweep measures the CENTRE chunk only.** `tower` shows the other
direction and a centre-only measure cannot: a chunk's stored-section set
depends on its *neighbours'* content.

### The three rules that decide the wire bytes

1. **A section is stored iff it, or any of the 26 sections in the `-1..=1`
   cube around it, is non-air.** Section space, so it crosses chunk borders.
   Measured by the `tower` case: a pillar to y=120 in the centre chunk alone
   makes the **diagonal** neighbour (1,1) store sections -5..8, where a
   per-column rule predicts -5..-3.
2. **Light does not enter an unstored section** (`propagateIncrease` skips a
   neighbour that is not `storingLightForSection`).
3. **Opacity is `max(1, getLightDampening())`.**
4. **`shapeOccludes` drops a neighbour outright** when the two occlusion
   shapes close the shared face. A bottom slab is `lightDampening = 0` with
   `lightOcclusionFaces = 100000`, so without this a slab roof lets light
   straight through and the room under it is lit where vanilla has it dark.

And two traps in the classification itself:

* the mask bit is a dense `i`, the storage lookup is at
  `getMinLightSection() + i` — 5 apart in the overworld, 0 apart in any
  dimension with `minSectionY == 0`;
* `DataLayer.isEmpty()` is `data == null && defaultValue == 0`. It does not
  scan nibbles.

### The finding

**Vanilla's section light for one and the same final world depends on how the
world was reached.** `relit` removes a glowstone from a lit sealed room;
`sealed_dark` builds the byte-identical room with it already absent:

    relit        block_mask .....1....   1 array of 2048 ZERO bytes
    sealed_dark  block_mask ..........   0 arrays, section 0 in the empty mask

Reachable on the live send path. Registered, not hidden: the test asserts we
reproduce the from-scratch control **and** that vanilla's two answers really
differ, so it cannot go vacuous.

### Positive controls — every one, and two of them found real defects

1. **`ci/light-golden.sh` runs twice and `cmp`s.** The engine drains a
   `LongOpenHashSet`-ordered queue; if any of that reached the result the
   vectors would churn and hide a regression. Identical.
2. **The classification is non-uniform across cases** — seven distinct mask
   layouts. A port that always answered "data", "empty" or "absent" fails at
   least three.
3. **`void` reproduces `LightData::default()` exactly** — our old output is
   right for a chunk with nothing in it and wrong for every other, which is
   also the unit test `classifying_an_empty_source_reproduces_todays_default`.
4. **Value variety is measured**, so the `710` trap is a number and not a
   worry: superflat sky 2 values, overhang sky 10, emitters block 16.
5. **`sealed_dark` is the control for `relit`** — same world, no mutation —
   and it is what attributes the divergence to history rather than to a port
   defect.
6. **The `relit` assertion is two-sided**: we match the control, AND vanilla's
   two answers must differ. If a future jar stops distinguishing them the test
   says so instead of passing.
7. **`ci/light-radius-sweep.sh` refuses a non-converged answer** rather than
   quoting the edge of its own sweep, and labels every case that grades no
   cross-chunk propagation.
8. **`ci/light-falsify.sh` — 8 cases, 8 passed, 0 failed** (isolated remote
   dir, 2026-08-18). Case 1 is a positive control on the unmodified tree; the
   other 7 are injections each required to turn the comparison red:
   mask-bit/section-Y conflation, the cube reduced to a column, light leaking
   into unstored sections, `getOpacity` without `max(1)`, `isEmpty` ignoring
   `defaultValue`, an untrimmed BitSet, and the history divergence losing its
   control. Every injection is verified to have changed its file first — a
   patch that stopped matching is a hole in the sweep, and T0 gates that check
   alone as `LIGHT_FALSIFY_DRYRUN=1`.

   **Case 6 earned its place by failing.** On the first clean run it came back
   `pass` where `red` was wanted: the engine materialised every `DataLayer`,
   so `default_value` was never non-zero and breaking `isEmpty()` to ignore it
   changed no output. A faithful port with dead state behind it is still dead
   state. Uniform sections are stored homogeneously now — no wire byte moved —
   and the case bites.

9. **`slab_roof` failed before `shape_occludes` existed.** Counting the 6,762
   shape-occluding states said only how many a dampening-only engine could be
   wrong about. This case is what proved it was wrong about them, and by how
   much: the whole room under the roof.
10. **`section_light_wired`** asserts the flag-off packet is byte-identical to
    what shipped before this branch, so the wiring cannot silently change the
    default.

11. **No unordered iteration in the engine at all.** `BTreeMap`/`BTreeSet`
    throughout, so a result that depended on iteration order is not
    expressible — the property `ci/light-golden.sh`'s two-run `cmp` checks on
    vanilla's side.

### Nulls — killed explicitly, do not re-run

* **Do not add a light check to the gameplay corpus.** Both halves are
  measured above. It is not a matter of writing a better scenario.
* **Do not "simplify" `DataLayer` to `[u8; 2048]`.** The homogeneous /
  materialised distinction is load-bearing on the wire and a flat array cannot
  express it. `ci/light-falsify.sh` case 6 is the guard.
* **Do not light per chunk.** The stored-section rule reads the eight
  neighbours; per-chunk lighting either does 9x the work or gives a different
  answer at every chunk border. It is computed once per `World`.
* **The `relit` divergence is not a port defect** and re-deriving that costs a
  golden regeneration. `sealed_dark` is the proof.

### What is still open, with the result that closes each

1. **`RUSTMC_SECTION_LIGHT` defaults off.** The path runs and is asserted
   end to end — 20 section/layer classifications on a real generated chunk,
   2 sky arrays, 5 block arrays, round-trips — but the *wiring* is unmeasured
   on a generated world, because no instrument can see a chunk packet.
   **Closes when something compares our chunk packet's light against
   vanilla's on a generated world.** That instrument does not exist and is the
   single most valuable next thing here.
2. **The union-of-two-partials occlusion case.** `shape_occludes` answers
   `faceShapeOccludes` from the per-state face column, which is exact whenever
   at most one of the two shapes is partial. Two stairs facing each other,
   neither covering the shared face alone but together covering it, is the
   realistic instance vanilla calls occluded and we do not.
   **Closes when a golden case with two facing stairs passes.**
3. **Materialisation across edits**, for the `relit` divergence.
4. **The incremental engine.** This is converged initial lighting only; there
   is no decrease queue, because a decrease only arises from a mutation.

### Coordination

The sibling sky-model lane owns block-light *semantics at the use site*
(daylight detectors, `environmentAttributes`, `useblock.rs`). This lane owns
the section light *data on the wire*. The one place they meet is
`lightEmission` / `lightDampening` in `block_props.json`, which this branch
adds — a use-site lane should read those columns rather than adding its own.
