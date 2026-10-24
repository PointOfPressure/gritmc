# Cold start — branch `worldgen-block-entities`

---

### COLD START — branch `worldgen-block-entities` off `main` 911c2e04, 2026-08-16

**The block-entity hole named in `docs/PARITY.md` §6 is measured, and the
first class out of it is written.** Threadripper. Two commits, both pushed:
`15aceab5` (census) and `a7ec5682` (implementation).

Window throughout: seed 12345, overworld, r=16, centre chunk (64,64), 1,089
chunks, bbox `48 48 80 80`, harvest `rows`, dump order `zx` matched to the
references' own `HARVEST-ORDER`. The same window that carries the quotable
block-state figure, chosen so the two can be quoted in one sentence.

    census   173 block entities in 1,089 chunks, 10 ids, 0 malformed
    floor    0..13 positions, median 5, n=120 pairs over 16 references
             — with id=0 and payload=0 in EVERY pair
    reach    160 of 173, 92.486%
    result   potent_sulfur: payload 28 of 28 byte-identical (floor 0)
                            presence 7 of a 35-position union (floor 0..12, BELOW FLOOR)

**No vanilla boot and no corpus lock were needed for any of it.** Block
entities are in the chunk NBT the existing harvest already wrote and
`ChunkColumn` has parsed them all along — `ci/corpus-floor.sh`'s trick on a
different tag. The only cost was two dumps of our own.

## The three things worth carrying forward

**1. The block-entity floor has no content in it, and that is the finding.**
Across all 120 unordered pairs of the sixteen c..r references, `id`
mismatches are 0 and `payload` mismatches are 0. Every position where two
independent vanilla runs both have a block entity, the NBT is byte-identical.
The whole floor is displacement. So **payload parity is exactly achievable**
— there is no irreducible content noise and nothing to subtract from a
payload claim — while **presence** parity is bounded by ordinary
decoration-order noise. Score the two axes separately or you will report a
class as 80% done when its content is 100% right and its presence is below
floor. `docs/DETERMINISM.md` §40.2, §41.2.

**2. Loot is NOT resolved at generation time.** `chest`, `dispenser`,
`decorated_pot`, `brushable_block` save `LootTable:string` +
`LootTableSeed:long` and **no `Items` list at all**. A real sample:
`{LootTable:"minecraft:chests/simple_dungeon", LootTableSeed:-548012762793186881L, components:{}, keepPacked:0b}`.
Reproducing a generated chest is one known string and one `nextLong` at the
right point in a draw sequence — a draw-order problem, which this repo is
good at — not a loot-table port. `barrel` (5 of 10) and `hopper` (all 5) are
the exception and carry resolved `Items`.

**3. The reachable share refutes the framing this item was written under.**
The premise was that sixteen unbuilt structure sets would put most chests and
spawners out of reach. On this window **trial chambers are fully reachable**
— all 89 of their block entities, `decorated_pot` 31/31, `vault` 13/13,
`trial_spawner` 7/7, `dispenser` 19/19, `barrel` 10/10, `hopper` 5/5 — and
the whole structural shortfall is `ocean_ruin_cold`'s 8 plus one
`buried_treasure` chest. `README.md` pairs *"sixteen of the twenty structure
sets are still unbuilt"* with *"and no block entities are written"* and
invites the inference that the first bounds the second. Here it does not.
Measured positionally with `bereach`, because two censuses can agree on
counts while every position differs.

## Nulls — do not re-run these

* **Do not read the reachability ceiling off two `census` runs.** It was
  tried and it is wrong in the way this repo keeps paying for: vanilla places
  32 `potent_sulfur` and we place 31, which reads as 31 reachable; the
  positional answer is 28. `bereach` exists for this.
* **Do not expect `region-differ starts` to tell you which structure sets we
  build.** Our dump writes no `structure_starts` at all — `anvil_bridge`
  says so and it is still true. Measure it from the block states that carry
  block entities instead, which is what `bereach`'s owner column does.
* **Do not try to match vanilla's within-chunk `block_entities` ORDER.** It is
  the block-entity map's hash order: the nineteen entries in chunk (66,79) of
  reference `c` come in an order that is neither by y, nor by position, nor
  by type. We emit a deterministic scan order and say so. Everything measured
  here is positional, so nothing depends on it — but a byte-identical region
  file needs that map's bucket layout first, and that is a real item nobody
  has opened.
* **Do not chase the `potent_sulfur` presence charge of 7.** It is below its
  own floor (0..12, median 4, n=120) and it is the same displacement already
  counted as 4 out-of-reach positions in the block-state residual. Closing it
  means moving blocks, not block entities.

## Two instrument findings that are not about block entities

**`ci/be-emit-falsify.sh` found a hole in itself on its first run.** The
injection table was tab-separated; tab is IFS whitespace, so the two rows
with an empty column had their fields shift left. They applied a nonsense
patch, matched no `case` branch, printed nothing — and the summary still read
`injections=10 holes=0` while showing eight results. **A sweep that loses a
case silently fails open.** Separator is now `@`, an unreadable row counts as
a hole, and the whole thing is in `ci/t0.sh` at 41 s.

**`ci/t2-regiondiff.sh` is green because its input stopped changing.** Its
header asserts *"If vanilla-vs-vanilla isn't 100%, the parser/differ/gen-harness
is broken"*. The frozen fixture reads `100.000000%`; a fresh harvest of the
identical seed, radius and 3x3 comparison reads **17 mismatches,
99.998079%**, in `sulfur_spike` and `air`. So it would go red on any fresh

(That reading is registered unquotable and is **not a level**: seventeen blocks is one draw from this window's own floor distribution.)
pair, and its premise is contradicted by every floor this repo has measured.
**Left as found** — it is in neither `ci/t0.sh` nor `ci/t1.sh` nor
`ci/handoff.sh` so it gates nothing, `oracle-work/t2-regiondiff/` survives as
the fixture for `real_vanilla_chunks_survive_a_rewrite`, and re-thresholding a
shared artefact needs its own item and its own measurement. Registered as
`t2-regiondiff-fresh-pair-floor`. `docs/DETERMINISM.md` §41.3.

One bookkeeping note: `end-r16-n6-exact`'s claim is the bare string
`100.000000%`, so §41.3's two legitimate uses of it moved that figure's
`claim_count` 7 -> 9. Its DOC-DRIFT protection is diluted by any unrelated
`100.000000%` in the same document. Pre-existing, not introduced here, but
worth knowing before someone reads a count bump as tampering.

## Worktree note

`oracle-work` in this worktree is a **real directory with the references
symlinked in individually**, not one symlink to the whole shared directory —
`docs/AGENT-BRIEF.md`'s rule, and it earned itself here. The spawn brief said
to symlink the lot; had I left it that way, `ci/t2-regiondiff.sh` would have
regenerated `oracle-work/t2-regiondiff/{a,b}` **inside the shared
`repo/oracle-work`** and overwritten fixture references two other trees read.

## CORRECTION 2026-08-17 — `mob_spawner` is NOT the cheapest one left

The next item below says `mob_spawner` is cheap because our code already makes
the draw. The draw is not the obstacle. **The architecture is**, and this was
found by reading it rather than by trying:

`block_entity::for_block(name, x, y, z)` is keyed on the **block name and
position alone**, and `from_sections` runs as a post-pass over the finished
canvas. `anvil_bridge.rs` says why in as many words — *"Running it here rather
than inside a feature is what makes it unable to perturb generation: the canvas
is already final, so no draw is taken and no block state can move."* That
property is the whole safety argument for emitting block entities at all, and
it is worth keeping.

A spawner's mob is not on the canvas. It is one `nextInt(4)` into
`{SKELETON, ZOMBIE, ZOMBIE, SPIDER}`, taken inside `monster_room.rs` and
currently discarded, and no function of `("minecraft:spawner", x, y, z)` can
recover it. So `mob_spawner` is **the first class that needs a generation-time
side channel** — a per-position payload recorded when the feature runs and read
by the emitter — rather than a new arm in `for_block`. Every class landed so
far (`potent_sulfur`) is a pure function of the block, which is why none of them
needed one.

That is a design item with a correctness question attached: the channel must
carry data *out* of generation without letting the emitter feed anything *back*
in, or the property quoted above stops holding. Budget it as such. **Do not
start it expecting a table entry.**

The genuinely cheapest remaining classes are the ones whose entire saved payload
is a constant or a function of the block state — check that first against the
census in §40 before picking the next one, because the census ranks by count and
not by whether the content is derivable.

## The single next item

> **SUPERSEDED 2026-08-17 — this whole section is withdrawn. See the entry
> at the end of this file and `docs/DETERMINISM.md` §42.** It is kept rather
> than deleted because it was published and because the shape of the error
> is the point: it priced an item by the draw it needed and not by the
> signature it had to cross.

**`mob_spawner`, and it is the cheapest one left.** 19 positions on this
window, **all 19 reachable** (13 dungeon, 6 mineshaft — both built). It
stores `SpawnData:{entity:{id:"minecraft:skeleton"}}` plus eight constant
shorts, `SpawnRange:4s`, and an empty `SpawnPotentials`.
`crates/rustmc-worldgen/src/feature/monster_room.rs` **already draws that
mob** — its `DRAW 66 nextInt(4)` comment names the draw — and simply does not
write it out, so the value exists in our code today.

It closes when `region-differ bediff --match mob_spawner` reports
**`payload=0` and `id=0`** against a per-class floor taken the same way
(`ci/be-census-12345.sh floor` with `--match`), with the presence charge
quoted beside its own floor rather than folded in. Payload is the axis that
resolves; presence will be below floor again and that is fine, provided it is
labelled.

Note the trap the census sets for it: the spawner's payload floor is 0 like
everything else, so **a wrong mob is fully visible** and there is no noise to
excuse it. Get the draw point wrong and the number will say so.

### COLD START — branch `worldgen-block-entities`, 2026-08-17 (second entry)

**The estimate in "The single next item" is withdrawn, and so is the framing
under it.** That section named `mob_spawner` as the next class and called it
the cheapest one left, on the grounds that `monster_room.rs` already takes its
`nextInt(4)`. The draw was never the obstacle, which the correction above
already records. What this entry adds is that the same is true of **every
remaining class**, so there is no cheapest-one-left to pick.

**Measured, per class, off `gen-wide-12345-r16-ow-c` and the pinned jar —
`docs/DETERMINISM.md` §42.** Of the 173 block entities on the window, **141
are placer-written**: a draw (`LootTableSeed` is a `nextLong` inside
`RandomizableContainer.setBlockEntityLootTable`; the spawner's mob), a
template's stored NBT (`trial_spawner`'s two configs, `decorated_pot`'s
sherds, `vault`'s config), or the bare fact that no loot table was set (a
plain `barrel` and a `hopper` are the same block in the same states as their
loot-bearing siblings). The emitter is a post-pass over final block states, so
none of that is reachable from `(name, x, y, z)`.

**So this is one architectural item and then nine cheap arms, not nine
items.** Ranking the classes by payload size was ranking the wrong axis.

**Two figures in §40 are refuted and both were mine:**

* **Nothing in this window carries resolved contents.** §40.3 called `barrel`
  and `hopper` "genuinely more expensive" for carrying an `Items` list. The
  lists are **empty**, and the barrel groups are disjoint: 5 × `{Items:[]}`
  with no loot table, 5 × `{LootTable,LootTableSeed}` with no `Items`. The
  class that looked most expensive is among the cheapest.
* **`vault` is not a function of its block state.** 13 of 13 correlate
  perfectly — 4 `ominous=true` against 4 ominous configs, 9 against 9 plain
  ones — and `VaultBlockEntity.<init>` sets `config` to `VaultConfig.DEFAULT`
  unconditionally and never reads `VaultBlock.OMINOUS`. The template places
  both. **A perfect correlation over every position in the window, and the
  bytecode says it is a coincidence of placement.** Keying the emitter on
  `ominous=` would have passed everything this window can run.

**Nulls — do not re-run these:**

* **Do not look for another derivable class.** All ten were read raw; the only
  one whose payload is a function of the finished state is `potent_sulfur`,
  which is landed. The census is in §42.1 and it is exhaustive for this window.
* **Do not emit a constant payload for `hopper` or a bare `barrel` without the
  channel**, however tempting the 5/5 uniformity looks. Both classes extend
  `RandomizableContainerBlockEntity` and can carry loot; emitting `{Items:[]}`
  for one that does would be a *wrong* payload, which this file's own rule
  ranks below a missing one.
* **Do not build the channel as a read/write map.** It is write-only from
  generation's side — features push, the emitter reads — and the moment a
  feature can branch on it, block-entity emission can perturb generation and
  every published block-state figure is back in question.

**The single next item, and what closes it:** build the channel, with its gate
in the same branch. `docs/DETERMINISM.md` §42.5 has the cost read off the
tree: two consumer call sites (`crates/rustmc-worldgen/src/anvil_bridge.rs::to_chunk_column` (`:99`) and `::column_from_canvas` (`:407`)), a write-only
`record_block_entity` on the per-feature world traits implemented once on the
canvas (ten files place an entity-bearing block, listed there), and
invalidation by intersecting the channel with the finished canvas so that an
overwritten position drops its payload exactly as `LevelChunk.setBlockState`
does. It closes when the window generates **byte-identically in its block
states** with recording on and off — that is the property that keeps the
post-pass sound, and it is a `cmp` of region files rather than an argument —
and then `region-differ bediff --match <class>` reports `payload=0 id=0` per
class against its own floor.

**What is NOT done here and needs a quiet box:** every number above is read
from vanilla references and the jar. Nothing in this entry required a dump of
ours, and nothing in it has been confirmed against one. The box was at load
19.6 with a gate and a lane test running throughout, so the window-wide
`bediff` that would confirm the classification against our own generator is
the next agent's first measurement, not a repeat of mine.

### COLD START — branch `worldgen-block-entities`, 2026-08-17 (the channel built and gated)

**The previous entry's next measurement is done and it says `payload=0 id=0`.**
The window-wide `bediff` that entry deferred for want of a quiet box has been
taken: `docs/DETERMINISM.md` §43, `ci/be-payload-r16-12345.sh`, seed 12345,
r=16, centre (64,64), bbox 48 48 80 80, dump order `zx` matched to the
reference's `HARVEST-ORDER`, vanilla first —

```
BEDIFF chunks=1089 vanilla_be=173 ours_be=71 union=176
  missing=105 spurious=3 id=0 payload=0 exact=68
```

**68 of 68 positions where both sides write a block entity agree key for key.**

**What was built.** The side channel §42 specified: `block_entity::Record` (a
payload map plus the one value a placer *draws*), `Canvas::block_entities` as an
`Option<Records>` so "not recording" and "recorded nothing" stay apart,
`BlockEntityRecorder` as a **required** method on the traits whose features
record, and the consumer at `crates/rustmc-worldgen/src/anvil_bridge.rs::column_from_canvas` (`:407`). Both isolation paths carry it
back — `detach`/`merge` and the isolated decoration pass's compose — in
decoration order, because the last record at a position is the one saved.

**What was landed on top of it.** Three classes, 72 positions of 173 with
`potent_sulfur`: the dungeon chest's `SIMPLE_DUNGEON` + the `nextLong`
`monster_room` already drew, the dungeon spawner's `nextInt(4)` mob, and the
mineshaft's constant `cave_spider`. The mineshaft one is the case that shows the
channel is load-bearing for a constant: same block, different mob, no block
state distinguishes them.

**The gate.** `ci/be-channel-identical.sh` — one binary, two dumps, recording
on and off, block states identical, with a positive control that refuses a
window where the recording arm wrote nothing. GREEN at (61,50) r=4. Its
falsifier `ci/be-channel-falsify.sh` injects two defects and **the pair is the
finding**: the identity gate catches a channel that writes into the world and
is BLIND to a producer that spends a draw, because that draw is spent in both
arms. The second half of the condition lives in `monster_room`'s draw-count
test, in T0.

**Killed as a null — do not re-run these.**

* **§42.4's six-variant payload vocabulary.** Unnecessary. Vanilla's templates
  store the block entity's tag beside the block and the save writes it back, so
  the channel needs a **map**, not an enum. Only the save-time completion is
  per class and it has two arms (`mob_spawner`, `vault`).
* **A `Bare` record kind.** Also unnecessary: a bare barrel's template tag is
  `{Items:[]}`, which is a payload rather than an absence.
* **Reading a payload value off a JSON rendering of the reference's NBT.** The
  seven `mob_spawner` counts went in as `TAG_Int` that way and scored all
  nineteen positions as a payload mismatch. `BaseSpawner.save` writes them with
  `putShort`. A rendering that prints a short and an int identically is an
  instrument that has thrown the answer away; only the bytecode distinguishes
  them, and `region-differ bediff` (typed NBT) is what caught it.
* **Keying the emitter on the block name for the type id.** `minecraft:spawner`
  saves as `minecraft:mob_spawner` and both suspicious blocks as
  `minecraft:brushable_block`, read off `BlockEntityTypes`' `validBlocks`
  arrays. `entity_id` is the table; `id=0` over the whole window is it working.
* **Keying `vault` on `ominous=`** — already killed in §42.3 and still killed.

**THE SINGLE NEXT ITEM, and it is not a block-entity item.** The other 101
positions are all one precondition: `structure::place::place_piece` is called
with `random: None` at four of its five call sites, so the `nextLong` that
`RandomizableContainer.setBlockEntityLootTable` spends is never spent, and a
`LootTable` with no seed is refused by the emitter and counted as
`EmitStats::incomplete`. Thread the piece's `RandomSource` into the jigsaw
driver (`tools/worldgen-assembly/src/generate.rs`, the `village.structure`
call site — it is the one trial chambers go through) and 89 of those 101
positions become emittable with no further work in `block_entity.rs`.

**It closes when:** the r=16 `bediff` reports `missing` down by 89 with
`payload=0 id=0` still holding, AND the block-state residual on the seed-12345
r=16 window is re-measured — because spending that draw moves every later draw
in the piece, which is a block-state change and the reason this was never a
line in the block-entity item. `place_piece`'s own header carries the worked
example (`RuinedPortalPiece.postProcess`, where the very next draw sets the
apron radius). Expect the residual to MOVE; the question the measurement
answers is whether it moves towards vanilla.
