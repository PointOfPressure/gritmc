# COLD START — branch `corpus-item-registry`

### COLD START — branch `corpus-item-registry`, 2026-08-13

**The item.** Interrogate the 1,537-entry item registry the way
`gameplay-block-classes` interrogated 32,366 block states, and decide what
`item 12 / 1537 0.78%` and `stacks opaque 0 / 539` (§38) actually measure.
Full write-up in `docs/PARITY-ROADMAP.md` §44; only what a next agent needs is
repeated here.

**Base.** This branch is `main` **plus `origin/trace-itemstack-legible`
merged in**, and it cannot be anything else: the `ItemStack` decoder is on that
branch and `main` has no item numerator at all. Rebased onto `a106314d`.
Sequence the two.

## The three answers

**1,537 is not a work queue, by about a factor of four on structure alone.**

```text
  1177 + 15x16 + 15x8 == 1537     IDENTITY HOLDS, zero exceptions
```

30 of `Items`' 1,207 static fields are `ColorCollection<Item>` or
`WeatheringCopperCollection<Item>` — **records in vanilla's own source** — and
they hold 360 of the ids. That is the item-side twin of §41's
`len(states) == prod|domain|`, and it has the same value: not a grouping
somebody chose, but vanilla declining to name those 360 separately. Arities are
counted off the record classes, so a new dye colour moves the number, not the
script. `ci/item-registry-decompose.py`, static, ten seconds, exits non-zero if
the identity breaks.

Removing the block half leaves **363 ids with a mechanism of their own** across
47 classes and 87 call shapes; plus dye/bundle/harness, about **411 of 1,537**
carry item behaviour at all. The other ~1,126 are block placement — already 116
schemas in §41 — and 88 spawn eggs, one code path with an `EntityType` in it.

**What the census counts as observed for an item:** a decoded `ItemStack` (a
node with both `item_id` and `item`, at any depth) in a recording of **vanilla**,
under one of five packet kinds. Our own emission counts for nothing. But
`hold` — `set_creative_mode_slot` — is the **only** route an item has into a
harness player's inventory (`Step::Command` cannot run `/give`, permission
level 2, and the harness's players are not operators), so **the items scenarios
name are a hard ceiling on the numerator**. This tree names 56. `12/1537` glues
a registry to a vocabulary; the census now prints the ceiling beside the ratio,
labelled as counted from the check files and not from a recording.

**`0 of 539 opaque` was guaranteed by the harness, not measured.** `Step::Hold`
wrote a literal `0` added components, so every stack in every recording was
componentless by construction and the decoder's opaque arm was unreachable.
Same shape as the `gamerule randomTickSpeed` vanilla refused for a week. The
harness can ask now: `ci/dump-items.sh` emits the 111-entry
`data_component_type` registry, `Step::Hold` takes
`components = [{ type, varint }]`, and exactly one stream codec is written
(`ByteBufCodecs.VAR_INT`, off `DataComponents.lambda$static$3`) with every
other type refused **by name at encode time** — wrong bytes throw inside
vanilla's packet read and arrive as a dead reference, not as a divergence.

## Nulls — killed, do not re-run

* **Do not re-derive how many items place a block.** `ci/block-items-golden.sh`
  on `gameplay-block-classes` measured it at runtime — **1,054, 14 of them
  under a different id**. A static walk of `Items.<clinit>` cannot see the 30
  collections at all and reaches 726. Quote theirs.
* **Do not present that 726 as a cross-check against 1,054.** The two routes
  are not independent readings of one thing; one of them cannot see a third of
  the registry, and the -328 "gap" is fabricated disagreement. This branch
  published it for one commit before catching it.
* **Do not push the static read on to a behaviour count.** §39 measured that a
  class unit is honest for `useWithoutItem` (39 classes, 40 behaviours) and
  dishonest for `useItemOn` (25 classes, 89 units) because refusals are not
  agreement. The item-side golden sweep does not exist; without it, quote the
  decomposition and refuse a coverage percentage over it.
* **Do not write speculative component codecs** — `trace-itemstack-legible`'s
  null, still standing, and it now has an encoding-side twin: a codec nothing
  exercises is untested wire format that only fires when something else is
  already wrong.
* **`LambdaMetafactory.metafactory` is the bootstrap method, not the target.**
  It is the first `REF_invokeStatic` in every `BootstrapMethods` entry, so a
  first-match-wins reader resolves all 275 lambda sites to it and reports **7**
  concrete item classes instead of 54. The wrong answer is tidier than the
  right one.

## What landed in the harness

* `ci/item-registry-decompose.py` — new, static, gates the identity.
* `ci/vanilla-reach-census.py` — the item row prints the vocabulary ceiling.
* `ci/dump-items.sh` + `Tables` — the `data_component_type` registry.
* `Step::Hold { components }` + `write_component_patch` — refusal validates
  before writing, so a rejected patch leaves the caller's buffer untouched
  (found by the test, not by inspection).
* `ci/t2-gameplay.sh` — `RUSTMC_CHECKS` runs a subset, printing
  `T2-GAMEPLAY SUBSET OK` and deliberately **not** the `GREEN` that handoff and
  every waiter greps for.
* `ci/checks/gameplay/781,783,782` (numbered 860,861,862 until the gate found
  the ordering bug below) — the experiment. 781 and 783 differ in one
  line (the component); 781 and 782 are different new items. So the pair
  isolates the opaque count and the trio isolates the per-item rule.

### COLD START — branch `corpus-item-registry`, 2026-08-17

**The gate found a real regression: 860/862 (renumbered 781/782 here) went
`match -> diverge`.** `container_set_content` step 1: vanilla's hotbar has
three stacks (the new item, a stick, and a `minecraft:dandelion`), ours has
only the first two — `slot 56 {"count":0}` where vanilla has the dandelion,
identically in both scenarios. Full log:
`gate-corpus-item-registry-20260817-063820.log:724-726`.

**Not a code regression — `crates/` is byte-identical between the commit that
recorded `match` (37ba5b0b) and the gated tip.** Traced instead to
`ci/checks/gameplay/790-a-flower-pot-empties-when-right-clicked.toml`'s own
documented rule: `Player.addItem` puts the potted flower back in "the first
free slot", so 790 writes state every check after it inherits, and its own
comment says **"A new check must not be numbered above 790 without moving this
one."** 860/861/862 landed above it anyway. `790` is itself a known, accepted,
already-documented gap — `useWithoutItem` is unimplemented on our server
(`docs/PARITY-ROADMAP.md:4373`), a pre-existing item on the roadmap, not this
branch's to close — so ours never gives the dandelion back at all, while
vanilla does. That difference was invisible while 860/861/862 were developed
and baselined under `RUSTMC_CHECKS` (a **subset** run, so 790 never ran first)
and only became observable once the full, correctly-ordered 108-scenario
`ci/t2-gameplay.sh` ran them for real, immediately after 790.

**Fix: renumber the new scenarios below 790 instead of moving 790.** 790 has
decades of PARITY-ROADMAP/WORKLIST prose citing it by number and needs no
change itself — its own rule was violated by the newer files, not by itself.
860 -> 781, 862 -> 782, 861 (blocked) -> 783; free slots between the existing
`780` and `790`. Internal self-references inside the three files updated to
match (checked with a whole-word grep, none left). This restores 790 as the
true highest-numbered file, so nothing inherits its state again.

**Null, so nobody re-tries it:** do not "fix" this by implementing
`FlowerPotBlock.useWithoutItem` — that is a real, separately-tracked, larger
item (see PARITY-ROADMAP's own `790` port note) and is not what broke these
two scenarios; implementing it would still leave the ordering rule violated
for the next check numbered above whatever 790 becomes.
