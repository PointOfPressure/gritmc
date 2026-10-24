# COLD START — branch `gameplay-block-classes`

### COLD START — branch `gameplay-block-classes`, 2026-08-12

**The item: §21.5 ranks block behaviour on two figures that disagree by 4x —
`use_without_item` at 16 of 184 blocks and 14 of 39 classes — and says the
honest ranking depends on which unit a reader means. Establish which unit the
project should quote, by measuring whether class membership actually predicts
behaviour. `docs/PARITY-ROADMAP.md` §39.**

**Nothing was booted, no corpus run was queued and the box-wide corpus lock was
never taken.** Everything here is the pinned jar, three JVM dumper runs and
static Python. Remote dir `~/rustmc-bclass`, base `84f789db`.

### The answer

| | `useWithoutItem` | `useItemOn` |
|---|---|---|
| by block | **17 of 184** (9.2%) | **3 of 163** (1.8%) |
| by declaring class | **15 of 39** (38.5%) | **3 of 25** (12.0%) |
| by distinct behaviour | **15 of 40** (37.5%) | **3 of 89** (3.4%) |
| does the class predict behaviour? | **yes** | **no** |

`useWithoutItem`: 39 classes hold 40 behaviours. Ten of the eleven testable
classes agree across every member and every state; the eleventh,
`FlowerPotBlock`, splits into 2 — its 38 potted variants are identical and the
empty pot is the other. **The class figure was honest all along and the block
figure is the one that understates.**

`useItemOn`: 25 classes hold 89 independent units, because **66 of 163 blocks
are in classes measured at nothing at all** — the block-entity classes, every
cell refused. Among *measured* classes the two units agree (19 → 23), so this
is a statement about the refusal hole, not about dispatch. **A class conclusion
from one entry point says nothing about the other, and that is now measured
rather than warned about.**

### Where the class decomposition really comes from

`ci/corpus-surface.py`, and **§21.5's attribution is right** — the suspicion
that it was wrong is what was wrong. The script has two axes; its docstring and
first 280 lines are protocol, and `behaviour_tables` (line 367) is the block
one. Two stages: `UseGolden.java` stores the **runtime** class
(`getClass().getSimpleName()`, 47 values), and `Hierarchy.declaring` `javap`s
the block package and walks the superclass chain to the **declaring** class,
collapsing 47 onto 39. The 39 needs the pinned jar; without it the script
prints 47 and tags it uncollapsed.

Do not confuse it with §23's "39", which is a refusal count, or with
`golden-probe-audit`'s "184 blocks, 47 classes", which is the uncollapsed
figure and correct for that document.

### The defect the measurement found

`behaviour_tables` tested `block_id in held` — **a string comparison between a
block id and a set of item ids.** The corpus holds `minecraft:redstone` and
places redstone dust with it; `minecraft:redstone_wire` scored uncovered
through every one of those checks. §21.5's headline was one short in both units.

Fixed with `ci/block-items-golden.sh` + `tools/goldengen/vanilla/BlockItemGolden.java`,
reading `BlockItem.getBlock()` off the jar for all 1,537 items. 1,054 place a
block, **14 under a different id** (seven crops, cocoa, tripwire from string,
powder snow from a bucket, cave vines from glow berries, sweet berries,
redstone). Not from `generated/reports/` — there is no items report in the
pinned jar — and not hand-written.

### Also fixed, and not optional

`ci/check-doc-claims.py`'s `find_headings` returned a `set[str]`, so **the one
collision the tool exists to prevent was the one collision it could not see.**
`main` at `84f789db` carried two `## 22.` headings in `docs/PARITY-ROADMAP.md`
— the renumber that made the dark-systems section 26 left the old line above the
new one — and the checker printed `0 collisions`. Orphan deleted,
`DUPLICATE-HEADING` red added with a poisoned fixture and a green control.

### What was killed as a null — do not re-run these

* **"§21.5 attributes the figure to the wrong script."** It does not. No other
  script in `ci/` emits a block-class figure.
* **"The class figure is a presentation choice."** For `useWithoutItem` it is
  nearly exactly right. Do not "fix" the ranking by switching to blocks.
* **"`FlowerPotBlock` is 39 implementations."** Two.
* **Sweet berry bush and cave vines as cheap scenarios.** The bush has cells
  only at `age=2`/`age=3` and reaching those needs random ticks, which
  `ci/t2-gameplay.sh` disables. Cave vines need a ceiling rig on a superflat
  that has no ceiling.
* **`ShulkerBoxBlock` as the cheapest big win.** Largest uncovered class at 17
  blocks, but `open_screen` projects `window` — `ServerPlayer.containerCounter`,
  a per-player counter with no reset shared down the corpus's one connection in
  corpus order. A fifth menu check worsens a coupling the baseline documents at
  four.
* **Using the stale `data/vanilla` copy that setup leaves behind.** It carries
  199 refusals against the current 160, and 39 of them are `FlowerPotBlock`
  cells — this instrument would have called that class predictive on refusals.
  Regenerate before measuring.

### The stated blind spots

* **These are bounds on reach, not records of touching.** `held` is what some
  check *holds*; adding a bare `hold` moves every figure above without
  measuring anything, and this instrument cannot tell the difference.
* **`BedBlock` agrees across 128 measured cells and 128 refused ones.** Its
  AGREE verdict rests on half its evidence; the refused fraction is printed
  beside every class for exactly this reason.
* **No scenario was added and the corpus is unchanged.** The movement above is
  entirely the namespace fix plus the new unit. `ci/checks/gameplay-baseline.toml`
  is untouched and `790` is still the highest-numbered scenario file.

### The single next item

**`ShulkerBoxBlock.useWithoutItem` returns `SUCCESS` on every path** — including
the one where `canOpen` is false because a solid block sits in the lid's swing
(`Shulker.getProgressDeltaAabb` deflated by 1e-6 against `Level.noCollision`).
So the golden table's result column cannot distinguish "opened" from "refused
to open", and only the corpus can. A `700`/`701`-shaped pair, in the free range
**781-786**, priced against the `window`-counter coupling. Closing result: two
new baseline rows, and the first corpus check whose subject is a branch the
golden tables provably cannot see.
