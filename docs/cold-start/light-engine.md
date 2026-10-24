# COLD START — branch `light-engine`

### COLD START — branch `light-engine`, 2026-09-05

**The area: `LightEngine`'s queues, `BlockLightEngine`, `SkyLightEngine`, and
the live update path.** Built on `origin/stack-worldgen` (main + 390, zero
behind), which folds `sky-light-model` and `light-section-data`. That is a
departure from "build on main" and it is deliberate: `main` has **no light
files at all**, so a branch based there would have had to write a second light
engine beside the one that is landing. `stack-worldgen` is a fast-forward of
main, so this branch is main + the stack + this work.

### The headline

Three figures, three different instruments, all vanilla-first.

    incremental section light        6084/6084 sections agree with vanilla
    post-edit section light          2340/2340 over 5 edited worlds
    harvested nibbles, 3 dimensions  5609 of 16060416 differ
      overworld  0 of 6787072   289 chunks   r=8
      nether     0 of 7409664   289 chunks   r=8
      the End    5609 of 1863680  81 chunks  r=4  — all sky, all in section -1

No percentage is published here on purpose. `ci/check-figures.py` requires
every percentage a reader could act on to carry an artefact, a window and a
floor in `ci/figures.toml`, and this one has no floor: there is no
innocent-vanilla ensemble for saved light the way there is for worldgen. The
counts are the primary form and the test prints the ratio at the point it is
derived, beside the command that derives it.

The third is the one the `light-section-data` cold start asked for and did not
have: *"Closes when something compares our chunk packet's light against
vanilla's on a generated world. That instrument does not exist and is the
single most valuable next thing here."* It exists.

    RUSTMC_LIGHT_HARVEST_RADIUS=8 cargo test --release -p rustmc-server \
        --test light_harvest -- --nocapture

`light_harvest` is a **T1** target, on `session_joint`'s own criterion — it
needs the extract and minutes. Decoding the three harvests is ~5,500 chunks and
costs 112 s in T0's debug build, against 1.7 s for the two T0 light targets
together; the engine is gated on every branch by those two and this one
re-measures a published number.

The default radius is **2**, not 8, for the same reason, and the radius is not
a tolerance — the comparison is exact at every radius that fits, so a smaller
window costs coverage and not correctness. Every printed figure names the
radius it was taken at.

Three harvests, three dimensions, and the last two are there for one reason:
**`minSectionY == 0`**, which `docs/AGENT-BRIEF.md` names as the case that
hides every off-by-one between a mask index and a section Y. An
overworld-only comparison cannot see that class at all.

`oracle-work/vanilla-r7` holds 362 chunks at `minecraft:full`, every one with
`isLightOn: 1` and light saved; `van-nether-20260722-c6464-open-a` and
`van-end-20260722-c0_0-open-a` hold 437 and 249. **All three had been on the
box unread since the harvests were taken.** 289 have all eight neighbours
loaded and only those are scored, because `LightEngine.getState` returns
**bedrock** for a position in an unloaded chunk and an edge chunk would be lit
against a wall vanilla did not have. Both sides read the blocks out of the same
file, so no worldgen residual can leak into the number — which no other parity
figure in this project can say.

**And the figure's blind spot is measured, not waved at.** A region file
carries a layer only when it was materialised, so 13,371 of the window's
15,028 section/layer pairs carry no saved array and are unscorable. A "we put
light there" count over them would be meaningless — above the terrain we
*should* put 15 everywhere and vanilla saved nothing. What is meaningful is
whether **our** layer is uniform in exactly the pairs vanilla declined to save,
because uniformity is what made vanilla decline. It is uniform in all 13,371,
and the test asserts it: a non-uniform layer where vanilla saved nothing is a
place the residual could hide that the nibble count cannot see.

**The geometry was an inference and the inference was wrong on its first run.**
`min_section_y` was derived as `min(section Y) + 1`, assuming a saved file
carries the two light-only padding sections. This harvest carries sections
-4..19 — exactly `getSectionsCount()` = 24 — because vanilla writes the padding
only when the engine has data there. The derived `min_light_section` was
therefore -4 instead of -5, clipping section -5 out of the stored set and out
of the scoring in the same step: a clean-looking figure over a window one
section short at each end. Both extents are handled explicitly now and
anything else panics. **The `light_section_count() == 26` cross-check caught
it, on the run it was added**, which is the whole argument for turning an
inference into an assertion.

### The End's 5,609, attributed

Every one of them is in **section -1** — the light-only padding section below
the End's world bottom, which exists because `getMinLightSection()` is
`getMinSectionY() - 1` and the End's `min_y` is 0. Every one is sky, none is
block, and vanilla is uniformly **brighter** than us there: `vanilla 1 ours 0`
384 times, `vanilla 15 ours 9` 104 times, and so on down a gradient. Outside
that one section the End is exact, and the overworld and the nether are exact
everywhere.

The census is printed by the test itself (`section -1: vanilla V ours O — N
nibbles`), so whoever picks this up starts from an attribution rather than a
number. The obvious suspect is `SkyLightSectionStorage.createDataLayer`'s
`repeatFirstLayer` branch, which is the only thing in the engine that can make
a section below the data start bright rather than dark — see the refusal below
recording it as ported and, on every driver in this tree, unreached.

### The dimension geometry is read out of vanilla, not written down

`the_end.json` in 26.2 says `"has_skylight": true`, which contradicts what
that dimension did in every older version. A hand-written `false` in the test's
dimension table scored **1,079,016** of the End's sky nibbles wrong and looked
exactly like a port defect. `min_y`, `height` and `has_skylight` now come out
of `generated-data/data/minecraft/dimension_type/<name>.json`, and the check
the declaration earns is that every section the harvest carries lies inside the
light range that geometry describes.

### What the 3744/3744 figure could not see, measured rather than argued

`light_golden.rs`'s green was real and it was narrower than it looked. Three
injections into the incremental engine were applied to the tree and **none of
them turned it red**:

| injection | `light_incremental_golden` | `light_incremental_mutation` |
|---|---|---|
| decreases after increases | pass | **red** |
| `checkNode` never carries the old level | pass | **red** |
| `removeSourcesBelow` does nothing | pass | **red** |
| emptiness from values, not history | pass | **red** |
| stale sky sources after an edit | pass | **red** |

Every static case reaches the engine only through `propagateLightSources`,
which enqueues nothing but increases, so `propagateDecreases` drains an empty
queue. **A port with no decrease queue at all passes every static case.**
`ci/light-incremental-falsify.sh` asserts *both* verdicts per injection, so it
is simultaneously the falsifier for the new checks and the standing evidence
for that claim.

### Two defects in the oracle, found by the new check going red

Both were in `tools/goldengen/vanilla/LightGolden.java`, not in our engine, and
neither was visible before something drove vanilla through an edit.

1. **The mutator did `chunk.setBlockState` + `engine.checkBlock` and nothing
   else.** `ProtoChunk.setBlockState` does the rest itself — but only when its
   status `isOrAfter(INITIALIZE_LIGHT)` **and** its `lightEngine` is non-null,
   and the golden's grid is built with a null engine at the default status, so
   the whole light branch is skipped. Two things were therefore never done:
   `updateSectionStatus` for a section that stopped being all-air, and
   `ChunkSkyLightSources.update` for the column. Without the second,
   `SkyLightEngine.checkNode` reads the **pre-edit** `getLowestSourceY`,
   decides the sealed position is still a sky source, and vanilla's recorded
   answer is that an 8x8 stone lid darkens nothing. 86 disagreeing sections
   became 50, then 9.
2. **`light_golden.rs` lights the dumped world from scratch and the dumped
   world is the world after the edit.** For `roof_opened` and `sky_sealed` a
   from-scratch relight and vanilla's incremental repair are genuinely
   different answers and neither is defective; those are skipped there, named
   in the output, and graded by the mutation test instead.

The last 9 were ours, and they are open item 3 of
`docs/cold-start/light-section-data.md`: **materialisation across edits**.
`DataLayer.isEmpty()` reads the representation, and `light_engine::light` has
to reconstruct it from the values because a fixpoint has no history. The
incremental engine has the history, so `LightStorage` tracks which sections
have been written through `DataLayer.set`. A section lit and then darkened
rides in the data mask with 2048 zero bytes, exactly as vanilla's does.

### The one place this is deliberately not vanilla

`blockNodesToCheck` is a `LongOpenHashSet` and vanilla iterates it in hash
order; ours is a `BTreeSet`. `ci/light-golden.sh` runs vanilla's engine twice
over the same world and `cmp`s, and the results are identical — including now
for the five mutated cases — so the converged answer does not depend on that
order. `Engine::check_order_is_total` is the property that would have to give
if a case ever showed otherwise.

### Refusals — what was not done, and why nothing writable would have been verifiable

* **No `light_update` packet is sent and none is graded on the wire.** The
  packet body is ported: `LightStorage` tracks
  `sectionsAffectedByLightUpdates` through `SectionPos.aroundAndAtBlockPos`
  (one section for a write inside one, up to eight at a corner — never a fixed
  27), and `light::classify_changed` carries
  `ClientboundLightUpdatePacketData`'s per-bit filter, with the unit test that
  a masked section is **absent** from both masks rather than empty in one.
  What is not gradeable is the *derivation of the mask a real server sends*,
  which lives in `ChunkHolder`'s `sectionLightChanged` bitset and
  `ThreadedLevelLightEngine`'s `onLightUpdate` callback — server plumbing the
  chunk-send lane owns. And there is no captured vanilla `light_update` to
  compare against: `tools/oracle-runner/src/trace.rs:247` classifies
  `cb::LIGHT_UPDATE` as housekeeping and drops it, so the corpus has never
  recorded one. Taking a capture is a corpus run plus a scenario plus a
  falsifiable sweep, and it grades nothing until the server sends the packet.
  **Closes when the server sends a partial `light_update` after a block edit
  and a scenario records vanilla's.**
* **`repeatFirstLayer` is transcribed and unreached.** The sky column-fill
  shortcut only fires for a section created *below* existing data, and both
  the golden's driver and `light_region` declare sections bottom-up, so every
  created section is the new top and takes the `isAboveData` branch instead.
  Injecting a dark layer in its place changes no output in any case in the
  tree. It is kept because it is what vanilla does and a chunk loaded under an
  existing column will reach it; it is recorded here as **ported and
  ungraded** rather than counted as covered.
* **The union-of-two-partials occlusion case is still open** — inherited from
  `light-section-data` open item 2, untouched here.

### Positive controls, all of them

1. `ci/light-incremental-falsify.sh` — 8 cases, case 1 a positive control on
   the unmodified tree, 7 injections each required to redden the mutation
   check, every injection verified to change its file first.
   `LIGHT_INCREMENTAL_FALSIFY_DRYRUN=1` is what T0 gates.
2. `light_harvest`'s own control: making `minecraft:stone` emit light moves
   **936,327** of the same nibbles, and the test asserts the denominator did
   not move so the two figures are comparable.
3. `the_edit_actually_changes_the_light` — for every edited case, vanilla's
   post-edit light must differ from the pre-edit world's, computed on
   vanilla's side alone. An edit that stopped mattering is refused, not
   passed.
4. `every_mutated_case_records_its_edits` — `mutation_drain_iterations > 0`
   iff edits were recorded, so a mutator that forgot to record silently
   becomes a from-scratch case and this says so.
5. `the_fixpoint_and_the_queues_agree_on_every_golden_world` — two different
   algorithms over the same input, 117 chunks. A disagreement is a finding
   about one of them.
6. `light_harvest` refuses a window under a million nibbles rather than
   reporting a clean number over nothing.
7. `a_neighbour_count_of_27_is_refused_not_wrapped` — `SectionState`'s range
   check panics as vanilla's `IllegalArgumentException` does, rather than
   wrapping into the `hasData` bit.

### Nulls — do not re-run

* **Do not grade the incremental engine on the static goldens alone.** The
  table above is the measurement; three of the five injections are invisible
  there.
* **Do not "fix" `light_golden.rs` to grade the mutated cases.** It lights
  from scratch; the mutated cases' input does not contain their history.
* **Do not add a light check to the gameplay corpus** — inherited null from
  `light-section-data`, and `trace.rs`'s housekeeping filter is the second
  reason.
