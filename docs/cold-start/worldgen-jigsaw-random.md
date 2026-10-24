# COLD START — branch `worldgen-jigsaw-random`

Append-only. A new entry per handoff; never edit an old one.

### COLD START — branch `worldgen-jigsaw-random`, 2026-08-17

**What this branch is.** The item §43 left behind: `place_piece` is called with
`random: None` at four of its five call sites, so the `nextLong`
`RandomizableContainer.setBlockEntityLootTable` spends is never spent, and 89 of
the census window's 101 unreproduced block entities are trial-chamber containers
waiting on it. §43 predicted that threading the stream would move the block-state
residual and asked which way. **This branch's answer is that the premise behind
that prediction is refutable from the pinned jar, and the branch is built so the
refutation had to survive a measurement rather than an argument.**

**THE ANSWER, so nobody re-runs it.** Threading the stream costs **0 block
states of 107,053,056** on the quotable seed-12345 r=16 window — all three arms
pairwise, one binary, ours against ours so no floor applies. The positive control
is the half that makes the zero mean anything: the `off` arm traced 0 lines and 0
draws, the `stream` arm traced 52 and spent **121**, every group
`trial_chambers step=3 index=4`. Vanilla first, all three arms identical at
charge 3,602 against a twin floor of 17,779 masked positions at N=2 — a delta
baseline, not a level.

**AND WHAT IT BUYS IS 60, NOT 89 — §43's closing criterion is refuted on both
halves.** `bediff`, vanilla first, recording on: `missing` 104 -> 44, `exact`
69 -> 121, and **`payload` 0 -> 8**. The 89 over-attributed: what is still
missing is 13 `vault` and 7 `brushable_block` and the remainder, and none of
those is a `RandomizableContainer`, so no threading can ever produce a draw for
them. The 8 were `decorated_pot.sherds` — the template's second payload key,
which this branch now carries — and the finding worth more than the eight is
that `EmitStats::incomplete` was **0 in both arms**: it guards seedless loot,
not partial payloads, because it asks whether the placer could supply what it
*drew*.

**The village path is exercised too, on the window that can.** Seed 20260722
r=16: 89 `village_savanna` groups, 3 container draws, 6 refused feature pool
elements — and `mismatches 0 of 107,053,056` on all three arms. §44.9.

**THIS BRANCH IS SAFE TO MERGE AND IS NOT A MEASUREMENT BRANCH.**
`RUSTMC_JIGSAW_STREAM` is unset by default, so it changes no output at all; and
the measurement says the change it gates would not change block states either.
Those are two independent reasons and the first one holds regardless of the
second. What is **not** merged is the decision to flip the default — that is the
owner's, and §44.7 is the recommendation with the number it rests on.

**Read `docs/DETERMINISM.md` §44 first.** It carries the bytecode offsets, the
blast radius as a count, and the numbers. What follows is only what a next agent
needs that is not in there.

**The one-line mechanism.** There are two streams. `setLargeFeatureSeed` decides
which pieces exist and is finished before a block is written. The one
`place_piece` takes is `applyBiomeDecoration`'s decoration `WorldgenRandom`,
`setFeatureSeed(populationSeed, index, step)` at `ChunkGenerator` offset 293 —
seeded **once per structure**, shared by every start and every piece — and inside
`StructureTemplate.placeInWorld` it has **exactly one consumer**: `nextLong` at
offset 494, one per placed `RandomizableContainer`, inline in the block loop.
**So the container tag is drawn at placement, not at save.**

**The blast radius is one call site, not four.**
`tools/worldgen-assembly/src/generate.rs`'s `place_village_in_chunk` is misnamed:
trial chambers go through it too, which is why 89 positions are reachable by
threading one function. The other three `None` call sites are `nether_fossil`,
`end_city` and `shipwreck`, and **two of them do not want this stream at all** —
`setBlockEntityLootTable` has exactly four callers in the jar and
`ShipwreckPieces$ShipwreckPiece` and `EndCityPieces$EndCityPiece` take it in
`handleDataMarker`, from a `minecraft:structure_block` marker. `place_piece` does
not run `handleDataMarker`. Budget those two as a **data-marker** item; only the
jigsaw call site is a stream item.

**How to run the measurement.** `ci/jigsaw-stream-cost.sh <label>`, seed 12345,
centre chunk (64,64), r=16, bbox `48 48 80 80`, dump order read off the
references' own `HARVEST-ORDER` stamps. It takes three dumps off **one** binary —
`RUSTMC_JIGSAW_STREAM` unset, `=1`, `=2` — and its primary figure is `off`
against `stream`, ours against ours, which needs no floor. Everything scored
against vanilla is vanilla-first and prints the twin floor beside it, never
subtracted.

**`RUSTMC_JIGSAW_STREAM` is unset by default and this branch does not change
that.** Off is the code that shipped. `1` threads the stream and moves nothing
else. `2` adds the two orders vanilla imposes on the starts — structures in
ascending step index, starts of one structure in `LongOpenHashSet` order — and
`2` is the arm that could genuinely move a block, by changing which of two
overlapping starts writes last. They are separate settings so the draws and the
orders are two numbers rather than one.

**Killed as a null — do not re-derive these.**

* **"Threading the stream necessarily moves block states."** §43's own words were
  "Expect the residual to MOVE". The stream is observable only through a later
  consumer and all three in the jigsaw chain are answered from the jar:
  `Structure.afterPlace` is overridden by exactly `WoodlandMansionStructure` and
  `DesertPyramidStructure`, neither of them jigsaw;
  `StructurePoolElement.handleDataMarker` is a single `return` and
  `SinglePoolElement` does not override it; `FeaturePoolElement.place` does
  consume it and is **the only path** by which it can move a block — and this
  port refuses feature pool elements out loud, so it consumes nothing.
* **"The processors read the piece stream."** They do not.
  `StructurePlaceSettings.getRandom(pos)` falls back to
  `RandomSource.create(Mth.getSeed(pos))` and `SinglePoolElement.getSettings`
  never sets the field. `CappedProcessor` — the one processor whose
  `evaluatesEntirePieceState` is true, and trial chambers do reach it — seeds
  itself from `ServerLevel.getSeed()` at offset 73. The palette comes from
  `getRandomPalette`, not from this stream.
* **"§43's `RuinedPortalPiece` example generalises."** It does not: a ruined
  portal is a `TemplateStructurePiece`, not a jigsaw structure, and
  `ruined_portal.rs` is the one call site that has **always** threaded its
  stream. The knob does not reach it and its `spreadNetherrack` radius does not
  move.
* **Deriving the step index by hand.** `structure_step_indices()` reads it off
  the datapack in `ResourceManagerRegistryLoadTask` order:
  `trial_chambers` step 3 index 4, `village_desert` 21, `plains` 22, `savanna`
  23, `snowy` 24, `taiga` 25, all step 4. The savanna 23 corroborates the figure
  a comment in `generate.rs` already carried.

**The gap this branch does NOT close, stated so nobody reads the emit count as
complete.** A village pool can hold a `feature` element and 13 of vanilla's 188
template pools do — every one of them a village `decor` or `trees` pool.
`FeaturePoolElement.place` spends a whole placed feature's draws on the same
stream, and this port refuses it, so **once the stream is threaded a village's
containers after the first feature element read a stream vanilla has moved
further along.** `trial_chambers` holds no feature element, which is why the 89
are the affordable ones and the villages are not. The refusal message says so
now.

**AND THE CHEAPEST FINDING WAS NOT THE MEASUREMENT.** `suspicious_sand` and
`suspicious_gravel` were in `is_randomizable_container` and are not
`RandomizableContainer`s at all — `BrushableBlockEntity extends BlockEntity`,
with its own private loot fields and a `setLootTable(key, seed)` that takes the
seed from its caller, so it looks like one from every angle except the
`instanceof` test `placeInWorld` actually runs. The list had been derived from
the classes that *mention* the interface rather than the ones whose declaration
names it. Chambers hold suspicious gravel, so two spurious draws per block would
have put every later container on the wrong `LootTableSeed` **while every
block-state instrument in this repo reported zero**, because a spurious
`nextLong` writes no block. Falsifier:
`a_suspicious_block_is_not_a_randomizable_container_and_spends_no_draw`.

**THE SINGLE NEXT ITEM.** Flip `RUSTMC_JIGSAW_STREAM`'s default to on for the
**trial chamber** group only, leave the villages behind the knob, and re-take
`region-differ bediff` on the r=16 window.

**It closes when:** `bediff` reports `missing=44` or better with **`payload=0
id=0`** restored — the 8 `decorated_pot.sherds` mismatches this branch measured
should already be gone, since `sherds` is now carried, but that has NOT been
re-measured and is the first thing to check — on the same window and the same
reference, vanilla first; and the `off`-against-`stream` block-state figure from
`ci/jigsaw-stream-cost.sh` is still `0 of 107,053,056`, re-taken on the tree that
flips the default rather than carried from this one.

**Do NOT expect `missing` to reach 0.** 13 `vault` and 7 `brushable_block` are
not containers and are a different item entirely. And **do not flip `=2` with
`=1`** — `groups-with-2+-starts=0` on both windows means the start-order arm has
never been exercised and its zeros prove nothing about it.

### COLD START — branch `worldgen-jigsaw-random`, 2026-08-18 (the recommendation completed, not flipped)

**The default is NOT flipped and this entry does not flip it.** The branch is
still safe to merge as it stands: `RUSTMC_JIGSAW_STREAM` unset changes no
output, and every number below was taken with the knob driven explicitly.
What changed is that §44.7's recommendation is now complete enough to decide
on, which it was not.

**Rebased onto `b94f3ebf` (current main) and green.** Six tree-only checks
pass. Two things the rebase surfaced, both fixed here rather than worked
around:

* `Cargo.lock` still named `grit-plugin-api 0.2.1` against main's `0.3.0`, so
  every build in the worktree dirtied the tree.
* This worktree's `data/vanilla` predated the `randomlyTicking` column that
  current main's `block_props.rs` requires, so **every dump died on the first
  call** and the branch's own `ci/jigsaw-stream-bediff.sh` reported it as
  "(no emit line found)", ran `bediff` against directories that did not
  exist, printed `FAIL: No such file or directory` twice and **exited 0**.
  Its header says it refuses without its positive control; it only printed
  it. Now it refuses, on four conditions (dump rc, missing emit line, `off`
  spending a draw, `stream` spending none).

**What was measured, and it closes two open items rather than opening one:**

* **`payload` is back to 0.** §44.8's `payload=8` was fixed by `0f92e0e9` and
  never re-measured — the branch documented a live regression that no longer
  existed. Same window, same reference, vanilla first: `missing=44 spurious=3
  id=0 payload=0 exact=129` against §44.8's `exact=121`. Block states across
  the recording arms `mismatches 0 of 107,053,056`.
* **The villages' desync is ONE draw of 292.** §44.7 reserved the villages on
  a mechanism and §44.9 counted refusals and draws without relating them.
  `StreamAudit` (new, `RUSTMC_JIGSAW_TRACE=1` only, spends no draw) records
  the running draw count at the first refusal in a stream: `village_savanna`
  3 draws / 6 refusals / **1 downstream**, `trial_chambers` 289 draws / 0
  refusals / 0 downstream. The chambers' zero is structural — they hold no
  feature pool element — not a small sample.

**The thing that makes it a decision and not a preference:**
`RUSTMC_JIGSAW_STREAM=1` is **global**. There is no trial-chambers-only
setting, so §44.7's "the recommendation is trial chambers only" was not an
action anyone could take. The choice on the table is: flip and buy 60
positions at 0 block states of 107,053,056 on both windows and **one wrong
`LootTableSeed`**, or leave it unset. That is the owner's call and §44.10
states it in those terms.

**Killed as a null — do not re-run:**

* **Do not re-run the `sherds` bediff to see whether the fix took.** It took;
  `payload` is 0 and the eight positions are the eight decorated pots.
* **Do not try to scope the stream to trial chambers by keying on the
  structure id.** The desync is not a property of villages as a structure, it
  is a property of a pool holding a `feature_pool_element`; 13 of vanilla's
  188 pools do and every one is a village `decor`/`trees` pool. Keying on the
  id reproduces today's answer by coincidence and breaks the moment a
  non-village pool gains one.
* **Do not read `groups-with-2+-starts=0` as evidence `faithful` is safe.**
  Still 0 in every arm of every run on both windows; the reorder has never had
  two starts to swap.

**The single next item, and what closes it:** the owner's decision on the
default. Nothing else on this branch is open. If the answer is "flip", the
closing measurement is the r=16 block-state residual re-taken with the knob
on by default — expected to be identical, since three arms on two windows
already say `mismatches 0`, but the default changing is what makes it a
published figure rather than an arm.
