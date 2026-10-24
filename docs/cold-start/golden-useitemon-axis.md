# COLD START — branch `golden-useitemon-axis`

### COLD START — branch `golden-useitemon-axis` off `golden-probe-audit`, 2026-08-12

**The item: build the `useItemOn` golden the way `use_without_item.json`
exists, and build it under the probe refusal from the start.
`docs/PARITY-ROADMAP.md` §23.**

The table's real path is `data/vanilla/use_item_on.json`. It is generated,
gitignored and branch-stamped, exactly like its sibling.

**Nothing was booted, nothing Rust was built by this branch's own work, and the
box-wide corpus lock was never taken.** Everything here is the pinned jar, two
JVM dumper runs and static Python. Remote dir `~/rustmc-uio`.

**This branch is based on `golden-probe-audit`, not on `main`.** The probe
machinery it had to generate under — `ci/probe-reads.py`,
`ci/check-use-probe.py`, `ci/checks/use-probe-dependent.toml`,
`UseGolden.STANCES` — exists only there. `golden-probe-audit` was rebased onto
`main` d049ef5a first; the one conflict was two `COLD START INDEX` rows and both
were kept. When `golden-probe-audit` merges, rebasing this branch onto `main`
replays only its own four commits.

### The numbers

| | |
|---|---|
| blocks whose class overrides `useItemOn` | **163**, 25 classes, 4,537 states |
| blocks skipped — and falsified, not assumed | **1,033** × the whole item registry |
| the item axis | **1,537** items, minted `new ItemStack(item, 1)` |
| the grid | 92,101,730 probes + 1,587,721 control probes |
| dumper run time | ~95 s, run twice and `cmp`-identical |
| states recorded | 4,319 of 4,537 |
| (state, item) cells differing from their state's default | **15,070** |
| blocks fully measured | **94** |
| blocks refused on every state | **68** — the block-entity classes |
| blocks `random`, keeping no cells | **1** — `minecraft:composter` |
| probe-dependent (measured) | **25** of 163: 8 on `game_master`, 17 on `no_build` |
| reads the player at all (static upper bound) | 140 of 163 |
| face-sensitive | 18 |
| hand-sensitive | 5 |

**How many of the 39 the table unblocks: all 39.** Every `FlowerPotBlock` block
is fully measured. `flower_pot` + each of 38 pottable items writes the matching
`potted_*` at `[0,0,0]` with flags 3, returns `SUCCESS` and leaves the stack
empty; every `potted_*` answers `CONSUME` and writes nothing. `790` needs no new
golden data.

**Which probe axes `useItemOn` reads**, measured rather than argued:

* **`game_master` (instabuild)** — 8 rows. `ItemStack.consume(n, player)` is
  `if (entity == null || !entity.hasInfiniteMaterials()) shrink(n)`, so every
  rule that takes the item off you reads the ability set. What moves is
  `stackAfter`, never the block write.
* **`no_build` (mayBuild)** — 17 rows, all `CandleBlock`.
* **face** — 18 blocks. `NoteBlock` gates its whole rule on
  `hit.getDirection() == UP`.
* **hand** — 5 blocks. `ShelfBlock` opens with
  `if (hand.equals(OFF_HAND)) return PASS`.

Face and hand are axes `use_without_item.json` has no column for, because
`useWithoutItem` is not handed the stack or the hand.

**Did the refusal machinery generalise?** The checker did, the allowlist could
not, and `ci/probe-reads.py` needed real work — **and its containment refusal
fired on the first attempt and was right.** The static walk called seven rows
clean that the sweep measures moving (cake, four cauldrons, composter, flower
pot), and it had two holes, neither visible from the table it was written
against: the walk stopped at the block packages, so
`ItemStack.consume(n, player)` in `world/item/` was fenced out; and it stopped
at `CauldronInteraction.interact`, a functional interface whose implementations
are lambdas no bytecode edge reaches from the call site. Both are fixed
(`world/item/` and `core/cauldron/` are inside the walk; an unresolvable
interface call fans out by exact descriptor, 21 methods for this one). Re-run
on `use_without_item.json` the corrected walk reads 123 of 184 rather than
§22's 122, and its decomposition moved much more than its total: 38 rows that
were "constant because every cell is a refusal" are now zero, and "fully
measured and constant" went 51 -> 90. Details in §23; the short version
is that `UseItemOnGolden` writes the same probe columns because it sweeps the
same `UseGolden.STANCES` over the same `UseGolden.HarnessPlayer` in the same
`UseGolden.TableLevel`, which is a property of the dumper and is therefore
checked (`ci/check-use-probe-falsify.sh` now runs a green control against both
real tables and refuses if the two allowlists disagree about the sweep width).
The refusal was demonstrated end to end rather than asserted: the first
generator run named all 25 probe-dependent blocks and would not write the file.

### What this branch also fixed, and why it had to

Neither is optional — the first is why this table could not have existed
before, and the second is why it would have been silently wrong.

* **`UseGolden.bindItemComponents`.** 141 pending registries apply, all 1,537
  items mint a stack. `use_without_item.json` goes from **199 refusals to 160**;
  the 39 `Components not bound yet` cells are now measured rows
  (`potted_dandelion` writes `flower_pot` flags 3; `composter[level=8]` writes
  `composter[level=0]`), and `full_inventory` is swept.
* **`UseGolden.bindItemTags`.** Recursive, because item tags nest
  (`tags/item/sulfur_cube_archetype/bouncy.json`).

### What was killed as a null — do not re-run these

* **"Binding item components turns the `full_inventory` axis."** It does not.
  `golden-probe-audit` named that as its single highest-value follow-up. Bound
  and swept: `use_without_item.json` still reports **17 of 184**, the same 17,
  and `ci/checks/use-probe-dependent.toml` needed nothing but its `unavailable`
  list emptied. The refusals were real; the axis behind them was not.
* **"38 blocks are refused on `Components not bound yet`."** 39, and the 39th is
  **`minecraft:composter`**, not a 39th pot. `flower_pot` itself never refused —
  its `useWithoutItem` returns `CONSUME` at offset 0 because the pot is empty.
* **A hand-written list of "relevant" items.** Not needed and not used. The
  whole registry is probed and the output is compressed by grouping identical
  answers, which costs 92 million probes and 95 seconds and cannot lose a
  member.
* **Counting refusals per cell.** Ten classes refuse for every item on every
  state, so a per-cell count is 3,959,358 and measures the width of the item
  axis. Refusals are counted per state.
* **A `sameAnswer`/`sameRow` split.** `UseGolden` needs one because `sound` is
  not part of `random`'s question. Here one comparison over the whole cell is
  correct and simpler; do not port the split across.

### The stated blind spots

* **17 states inside otherwise-measured blocks carry per-item refusals** —
  `jukebox[has_record=false]`, the eight book-less `lectern` states and the
  eight `honey_level=5` beehive/bee_nest states. Each is one item (the record,
  the book, the shears/bottle) reaching for a block entity on a block whose
  other items do not.
* **68 blocks are `NoBlockEntity` refusals** — signs, hanging signs, shelves,
  chiseled bookshelves, decorated pots, campfires, copper golem statues. This
  harness holds no block entities and every branch below the ask is a reading of
  that. Measuring them needs a level that does, which is a different item.
  Note the same limit applies to `use_without_item.json`, which records the
  null branch as a `PASS` rather than refusing; changing that is that table's
  business, not this one's.
* **One stack shape per item.** `new ItemStack(item, 1)`, default components.
  An enchanted book, a dyed leather boot or a stack of 2 is not probed.
* **`onlyMainHand` sweeps, not cooldowns.** `ServerPlayerGameMode`'s
  `stack.isEmpty() || onCooldown` gate at offset 223 is upstream of the item
  path, not of this one, and is not modelled.

### Provenance of the tip

`ci/t0-remote.sh` on the committed tree at `72e66016`, `~/rustmc-uio`,
sayer-desktop 2026-08-12: **T0 GREEN**, `skipping: no vanilla` = 0, zero
`test result: FAILED`, `check-use-probe-falsify: 11 passed, 0 failed` — eleven
rather than nine because both real tables exist in that tree and both are used
as green controls. No corpus run was queued: this branch touches no
`ci/checks/gameplay/` scenario and no `crates/rustmc-server/` source.

### The single next item

**Convert `790` from this table**, and it is a lookup rather than a port: the
scenario's potting is `flower_pot` + one item, the table gives the write, the
flags, the result and the consumption, and `loot_blocks.json` already recovers
`POTTED_BY_CONTENT` 38/38. The closing result is `790` going `diverge -> match`.

The second item, if that goes quickly: `ci/checks/use-item-on-probe-dependent.toml`'s
17 `CandleBlock` entries are a real debt and go away with one hand port of
`CandleBlock.useItemOn`'s `mayBuild` branch. The 8 `game_master` entries do
**not** go away by porting — they are the creative branch of item consumption
and the consumer has to read `instabuild` itself.
