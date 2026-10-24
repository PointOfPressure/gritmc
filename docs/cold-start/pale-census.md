# Cold start — branch `pale-census`

---

### COLD START — branch `pale-census` off `feat-undoworld` `63e548a92`, 2026-08-19

The pale garden census §85 could not take, and the three defects it found
underneath. Worktree `~/grit/work-pale`. `docs/PARITY-ROADMAP.md` §89.

## The thing to know before anything else

**§85 was right to refuse to claim the 9,129, and right that the answer might
be a second defect. It was both at once, three layers deep.**

```text
  before  308x  pale oak tree refusals            9,129 blocks missing
  moss    169x  canSurvive(pale_moss_carpet)      §85 lands, 3,821 closed
  carpet  101x  MossyCarpetBlock.placeAt          a survival rule
  partial  85x  the same, undelegated             a placement method
  levelrng  0x                                    WorldGenRegion.getRandom()
```

`pale_garden`'s **9,129 is 806**, and both remaining rows are *overshoots*.
The family's absolute error is 1,248 against a floor of 828, with four of nine
rows inside their own floor. Window A's total decline count across all
features is 441 → 23.

**A carpet refusal inside `PaleMossDecorator` rolls the whole tree back.** That
is §85's design working as intended, and it is why one block's survival rule
was worth 169 pale oaks. Generalise it before costing anything in this area:

> An unported leaf under a decorator is not worth its own blocks. It is worth
> the whole feature's.

## The two facts that unlock this area

**`WorldGenRegion.getRandom()` is positional and therefore portable.**
`simple_block`'s header called it "a different stream, positional per chunk"
and refused the block over it; the second half of that sentence is the answer
to the first.

```text
  this.random = level.getChunkSource().randomState()
      .getOrCreateRandomFactory(minecraft:worldgen_region_random)
      .at(center.getPos().getWorldPosition())
```

One stream per generating chunk, at `(minBlockX, 0, minBlockZ)` — the y is a
literal zero. `getOrCreateRandomFactory` hashes `Identifier.toString()`, so the
name carries the `minecraft:` prefix. `Decoration::with_level_random` builds
it; `SimpleBlockWorld::level_random_next_boolean` lends it.

**Nothing else in worldgen consumes it.** All 405 classes under
`net.minecraft.world.level.levelgen` were disassembled and grepped for
`getRandom:()` against the four level interfaces. `SimpleBlockFeature` is the
only hit, so within the FEATURES step this stream has exactly one consumer and
cannot be desynchronised by something unported.

## The trap that cost a whole generation run

**A defaulted method on a delegated trait is inherited, not forwarded, and
`UndoWorld` will not tell you.** Wiring the level random took window A's
declines from 101 to 85 — the other 85 were inside a `PaleMossDecorator`,
where the wrapper answered the `None` default instead of asking `inner`.

§85.2 already states the rule. This is the case that shows it is not a style
preference: the default is a **refusal**, so a feature refuses inside a tree
and places outside one, and the world generates fine and looks right. It was
found by generating 225 chunks and reading a decline tally.

`the_wrapper_forwards_the_level_random_rather_than_inheriting_its_default`
refuses it now, and `tests/nested_feature_rollback.rs` is the end-to-end net
§85 asked for: a real datapack pale oak, a real `Decoration`, a patch made to
refuse, and the **whole** block map asserted equal. Falsified by pointing
`UndoWorld`'s `PatchWorld::set_block` at `inner`, where it names the pale moss
blocks left behind.

## Measuring in this area

* **Window A adjudicates this port through `birch_forest`, not through
  `pale_garden`.** `pale_garden` is 0.53x its floor before and 0.67x after and
  is never a measurement; `birch_forest` is above its floor throughout and
  improved 944 blocks, because a pale oak near a boundary writes leaves over
  positions vanilla calls `birch_forest`. `meadow` corroborates at 2.28x.
* **Seven of the square's twelve biomes did not move by a single block across
  all six runs.** That is the control, and it is inside one square.
* **`pale_garden` cannot adjudicate its own port because its trees are the
  least positionally reproducible thing in the square.** Vanilla's own two runs
  disagree about **54%** of the pale oak log positions here — 1,292 of 2,397 —
  and 50% of the dark oak ones. `pale_moss_block`, placed by the same trees, is
  21%. The *count* floor for the same kind is 28. **Counts are two orders of
  magnitude more stable than positions in this biome**, which is the structural
  reason §85.6's "it must be a block census" is right. §89.11.
* `ci/family-census.py` is the block census: N region dirs, vanilla pair
  first, twin floor printed per row and never subtracted.
* Window A is seed 20260722, r=7, centre (-1056,-672), **dump order `zx`**,
  read off `HARVEST-ORDER` beside the reference and never assumed.
* **A block census says how much; dividing by a structural unit says how
  many.** `ci/family-census.py --columns` counts distinct trunk columns and
  prints the twin's beside ours. It separated "too many trees" from "trees too
  big": 552 vanilla against 621 ours for pale oak, floor 30 — a real excess —
  while logs per column is 4.42 against vanilla's own **4.34 and 4.54**, which
  is inside the floor and is nothing at all.
* **A derived ratio needs a floor as much as the count it came from, and it is
  easier to forget because it looks like an analysis.** §89.8's first version
  read the per-column excess as a shared placer bug. The floor was one line
  down in the same table.

## Nulls — do not re-do these

* **Do not read a mid-port wrong-way number as a regression.** `dark_oak_log`
  reached 2,415, `leaf_litter` 3,153 and `grove` crossed above its floor to
  2,988 while the family was partly ported. All three came back — 2,157, 3,025
  and 2,596 — once the last refusal went. §58.6's mechanism: a refused feature
  spends no randomness and vanilla spends the whole feature's. **Publish them,
  and say which they are evidence about.**
* **Do not re-cost `MossyCarpetBlock.placeAt` as needing an unmodellable
  stream.** See above.
* **`dark_oak_planks` in window A is 8 blocks at y=-46 in two runs of four.**
  It is a structure, not a decoration feature, and does not belong to this
  family.
* **`family_of`'s "sixteen unported" list was stale by fourteen.** It is
  derived now, and a test derives it. `fire` is the only survival subject with
  no rule at all.
* **Do not aim `ci/decorate-once.sh` at a pale garden chunk.** It throws.
  `PaleMossDecorator` reaches `WorldGenLevel.registryAccess/0` — now handled —
  and then `getLevel/0`, which returns a `ServerLevel`: a concrete class
  needing a `MinecraftServer`, so no proxy can stand in and no extension of
  the handler will reach it. The draw oracle is structurally blind to this
  biome. §89.9. Chunk (-1050,-668) still dumps, so the harness is intact.
* **Chunk (-1050,-668) is closed and was handed off twice as open.** §79.8's
  "vanilla 3,477 draws against our 3,374" for `pointed_dripstone` is green now
  and this branch did nothing to that path — every feature's stream is seeded
  per (chunk, step, index), so no other port can move it. §85 restated the
  figure without re-running it. §89.10.
* **A `javap`-derived "what the subject reaches" analysis decays when the port
  stops refusing something.** This was the first case: until both pale oaks
  could place, no dump ever ran that decorator, so the analysis was right when
  it was written.

## Environment

`cargo` is at `~/.cargo/bin` and not on the non-login PATH, and a piped
`cargo build | tail` reports exit 0 when cargo is missing entirely — check
that real compile output appeared. A fresh worktree's `data/vanilla` must be
copied from a live worktree, not from `~/grit/repo`. `oracle-work/*` can be
symlinked.

**Editing any Rust source invalidates `ci/binary-is-current.sh`**, which the
generation scripts run before they dump. A doc-comment edit is enough. Build
and generate in one detached chain, and do not edit while it runs.

`ci/unlanded.sh`, `ci/run-detached.sh` and `ci/claim-number.py` are not on
`main`; copy them untracked.

## THE SINGLE NEXT ITEM

**Find why `pale_garden` produces seventeen pale oaks too many and
`dark_forest` three too few.** The number to beat is 552 pale oak trunk columns
in window A against our 621.

Three explanations are already eliminated and should not be re-derived. The
two biomes' placement chains are byte-identical in the datapack; both
`_checked` placements are one `would_survive` filter on their own sapling, and
both saplings agree with vanilla on all twenty-three grounds; and
`pale_garden`'s `random_selector` picks between two *pale oaks*, so its draws
cannot change the total. **What is left is the stream inside the chunk, which
makes this §79.5's open item with the sign flipped** — that section said
"whatever places two extra dark oaks per chunk is downstream of draw 814" and
could not name it.

The size question is **closed**, not deferred: logs per column is 4.42 against
vanilla's own 4.34 and 4.54. There is no placer bug to find.
