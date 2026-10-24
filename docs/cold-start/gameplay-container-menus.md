# COLD START — branch `gameplay-container-menus`

### COLD START — branch `gameplay-container-menus`, 2026-08-12

**The item: the corpus had opened 3 of vanilla's 25 `minecraft:menu` types
(`docs/PARITY-ROADMAP.md` §21, ranked fifth in its work order). Find where the
honest limit is and move the number to it. `docs/PARITY-ROADMAP.md` §45.**

Seventeen scenarios, `740`-`756`, one per unreached menu type that a block
can open. No server code was touched: `crates/` is untouched by this branch on
purpose, and the seventeen land in the baseline at the verdict the run
measured rather than at one somebody hoped for.

### The limit, which is the deliverable and is lower than 25

* **Four of the 25 have no opener anywhere in the jar.** `GENERIC_9x1`,
  `9x2`, `9x4` and `9x5` are named by exactly one class each — `MenuType`,
  the registration site — and so are the `ChestMenu.oneRow`/`twoRows`/
  `fourRows`/`fiveRows` factories that build them. The control is in the same
  grep: `threeRows` has five callers and `sixRows` one. They are client-side
  screen constructors for a packet an unmodified server never sends.
* **`merchant` needs a mob**, therefore mob spawning, mob AI and the trading
  subsystem. `MerchantMenu` is reached only from `Merchant.openTradingScreen`
  and `handleSelectTrade`.
* So the ceiling for a block-and-item harness on this substrate is **20 of
  25**, and that is where this branch leaves it.

### The ranked list of what stops *this server* answering the seventeen

Full version with the class names in §45.2. Cheapest first:

1. **six menus need only a `MenuSpec` row** — anvil, grindstone, loom,
   smithing, cartography_table, stonecutter. The provider is a
   `SimpleMenuProvider` built in the block class with no block entity
   anywhere on the path, which is the path `480` already proves works. What
   is *not* free about them: six different slot layouts, three of which are
   `ItemCombinerMenu` subclasses that disagree about their input count.
2. **ten menus need one new `blockentity::Kind` each** — generic_3x3,
   crafter_3x3, hopper, brewing_stand, beacon, shulker_box, lectern,
   blast_furnace, smoker, enchantment. `Kind` has three members today.
   `blast_furnace` and `smoker` are one mechanism with two registry ids and
   two titles; `enchantment` is a Tier 1 menu behind a Tier 2 door, because
   `EnchantingTableBlock.getMenuProvider` reads `getDisplayName()` off the
   block entity and returns `null` without it (offsets 46-47).
3. **`generic_9x6` needs one method** — `ChestBlock.getMenuProvider`'s
   two-half combiner. The block entity is already here and `240` already
   builds the pair; nothing opens it.

### Nulls — do not re-run these

* **A stability column that compares every observation kind is not a
  stability column.** `ci/menu-probe.sh`'s first version did, and
  `move_entity_heartbeat` — a server timer — put thirteen scenarios in a
  `FLAPS` column including `470`, which the corpus has held at `match` since
  2026-08-04. `ci/corpus-floor.sh` over the identical two recordings said
  0/18. The script now projects the two kinds the checks declare. Do not
  read a flap off anything that does not project the way the check does.
* **`minecraft:smithing` is a menu id, not an item id.** The recorder wrote
  `error: no item called minecraft:smithing` into the trace and carried on,
  and the probe printed it in its `NO MENU` column beside genuine negatives —
  a harness fault reported as a substrate finding. The probe now exits 2 on a
  recorder error.
* **A `T0 RED` from `ci/check-use-probe-falsify.sh`'s `real-table` case after
  a rebase is a stale generated table, not a defect.** Seen here: the tree's
  `data/vanilla/use_without_item.json` had been generated on `d049ef5a`, the
  rebase brought in `golden-probe-audit`'s twelve-stance `UseGolden`, and the
  check refused with *"UseGolden swept 1 stances,
  `ci/checks/use-probe-dependent.toml` pins 12"* plus two `STALE
  ...fence_gate` lines that look like somebody's dirty allowance list.
  `ci/gen-gameplay-tables.sh` and then `ci/t0-remote.sh` again is the whole
  fix — `T0 GREEN`, 0 skips. This is the AGENT-BRIEF's rsync-generate-T0 order
  biting after a rebase rather than after a first setup, which is the case
  `ci/handoff.sh` covers and hand-rolling does not.
* **`finish_dig` and the two `setPlacedBy` second-block classes are still
  substrate items, not corpus items**, and nothing here changes that. See
  `ci/checks/gameplay/README.md`.

### The floor, and the one loose end

`ci/corpus-floor.sh` over two cold vanilla boots of the eighteen-check subset
(`ci/menu-probe.sh`), `proj:7cefde0fb56d9f3e`, ensemble `menuprobe-n2`:
**0/18 do not reproduce, 17 reproduced, 0 dead, 0 reference, 0 mixed, 0
boundary, 0 silent**, resolving power 77.6% at alpha 0.05. Nothing went into
`ci/checks/gameplay-unstable.toml` because nothing was observed to flap, which
is not the same claim as nothing flaps — see §45.5.

**`752` is `unmeasured` in that ensemble** and this is the loose end to pick
up: it was recorded while the file still held the `minecraft:smithing` typo.
It has one observation of `MenuType.SMITHING`, from the corpus run, and one
observation is not a pair. An N = 3 measurement over
`oracle-work/gameplay/menu-van-r1.json` (kept for this) plus two fresh probe
recordings was queued on the box's corpus lock and had waited 62 minutes
behind the gate chain when this was written; it writes
`~/rustmc-menus/oracle-work/menu-floor/floor.json` and `~/menu-floor.log`.
Re-run `ci/menu-probe.sh` if those are gone.

### The next item, and the result that closes it

**Port the six Tier 1 menus into `menu_for_state`** — one `MenuSpec` row each
plus a content packet over an empty container, which is the only new code
path any of them needs. It closes when `748`, `750`, `751`, `752`, `753` and
`754` read `match` in a corpus run, and the thing that will make them read
`diverge` instead is the slot count: three of the six are `ItemCombinerMenu`
subclasses and the superclass does not decide how many inputs they have.
Read each menu's own constructor, not `ItemCombinerMenu`'s.
