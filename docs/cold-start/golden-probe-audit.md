# COLD START — branch `golden-probe-audit`

### COLD START — branch `golden-probe-audit`, 2026-08-11

**The item: for how many blocks in `use_without_item.json` is the recorded
behaviour a function of the probe's own state rather than of the block?
Answered twice, with two instruments. `docs/PARITY-ROADMAP.md` §22.**

The table's real path is `data/vanilla/use_without_item.json`, not
`ci/checks/...`. It is generated, gitignored, and branch-stamped.

**Nothing was booted, nothing was built, and the box-wide corpus lock was never
taken.** The whole item is static: the pinned jar, the table, and two 14-second
JVM runs of the dumper itself. Four other agents were live throughout.

### The numbers

| | |
|---|---|
| rows in the table (denominator) | **184** blocks, 47 classes |
| rows whose `useWithoutItem` reads the player at all | **122** (34 classes) — `ci/probe-reads.py`, static |
| rows that actually **move** when the player does | **17** (6 classes) — `UseGolden.STANCES`, measured |
| probe-dependent **and constant** | **105** |
| — of which every cell is a refusal | 38, all `FlowerPotBlock`: constant because *unmeasured* — **now 1, `DaylightDetectorBlock`; see below** |
| — of which some cells are refusals | 16, `BedBlock` |
| — fully measured and constant | 51 (48 of them containers; `openMenu`'s `OptionalInt` is `pop`ped at 25 of its 26 call sites in the jar) |
| rows that never read the player | **62** |

The 17: 12 `FenceGateBlock` (facing), 4 on `no_build` (`comparator`,
`daylight_detector`, `redstone_wire`, `repeater`), 1 on `game_master` (`light`).
Named individually in `ci/probe-audit-20260811.txt` and in
`ci/checks/use-probe-dependent.toml`.

Every one of the 17 is inside the 122. That containment is the only consistency
check the two instruments have, and `ci/probe-reads.py` prints a refusal if it
ever stops holding.

### What was killed as a null — do not re-run these

* **"Most of the table is probe-dependent."** It is not. The first run of the
  static walk said **179 of 184** and every extra row was instrument fault, in
  two specific ways that are now fixed and commented in `ci/probe-reads.py`:
  `ci/jarcallgraph.py` devirtualises `invokespecial`, so
  `DaylightDetectorBlock`'s single `super.useWithoutItem` call fanned out to
  every override in the jar; and walking out of the block packages turns
  `Level.playSound`'s **excluded** player into `PlayerList.broadcast` reading
  `getX/getY/getZ` off every *other* player, which swept in every button, door,
  trapdoor and lever. **Do not quote 179.**
* **Position is not an axis this table moves on.** `adjacent_south` and
  `adjacent_above` move nothing that facing does not already move; the
  `adjacent_south` hits on the fence gates are its `yRot=180`, not its position.
  `BedBlock`'s 16 rows read `startSleepInBed` and stay refused under every
  stance.
* **Sneaking, `looking_down` and `looking_up` move nothing** — measured, not
  argued. No row in the table changed under any of the three. Note the bound:
  that is a statement about `useWithoutItem`, and `isSecondaryUseActive` is read
  by `ServerPlayerGameMode.useItemOn` *before* it, so a sneak-sensitive
  interaction can still exist upstream of everything this table describes.
* **`Player.openMenu` is not a defect surface.** Its `OptionalInt` is `pop`ped
  at 25 of its 26 call sites in the jar — the one exception,
  `Merchant.openTradingScreen`, `astore`s it and is on no block's use path — which
  is why 48 container rows read the player and are still constant. Do not port a
  container on the strength of the 122.
* **A corpus run was never needed and was never asked for.** Everything here is
  the jar and the table.

### The finding nobody was looking for

`LivingEntity`'s constructor is `setYRot(random.nextFloat() * 6.2831855f)` — a
uniform [0,1) times **2 pi**, into a field measured in **degrees**. The probe's
facing has therefore been a die roll since the dumper was written: two runs on
2026-08-11 gave 0.522 and 5.360.

Every such value lands in `Direction.fromYRot`'s southern bucket, [-45, 45), so
`getDirection()` has always answered `SOUTH` and the table has always been
stable. **A random input invisible because its range was narrower than its only
consumer's resolution.** `base` now pins `yRot` to 0 and *throws* if a
constructed player ever faces anything but `SOUTH`. Caught by the two-run `cmp`
already in `ci/use-golden.sh`, the moment the dumper started writing the number
into the file.

### What is enforced now

`ci/use-golden.sh` refuses to write the table at all when
`ci/checks/use-probe-dependent.toml` does not already know what the sweep found:
a new probe-dependent block, a listed block whose axes changed (equality, not
membership), a listed block that stopped being probe-dependent, a stance count
that shrank, or a stance that became unavailable.

`ci/check-use-probe-falsify.sh` — six injections against a synthetic fixture,
green controls either side, one against the real table, 9 of 9, under a second,
no jar and no box. Gated in `ci/t0.sh`, because a refusal that lives inside a
generator most trees never invoke is a refusal nobody would notice had stopped
working.

### The stated blind spot — CLOSED, and the follow-up it named is done

`full_inventory` is one of the twelve stances and it **could not be set up**:
`new ItemStack` reads `Holder.Reference.components()` and `Bootstrap.bootStrap()`
does not bind them — the same limit that already puts 39 `Components not bound
yet` refusals in this table. It is reported in `probeStancesUnavailable` and
pinned in the allowlist rather than degraded into a silent copy of `base`, which
would have reported `FlowerPotBlock`'s `Player.addItem` axis as clean on no
evidence. **Binding item components in `TableLevel` would turn that axis and
would also retire the 38 refusals, and is the single highest-value follow-up
here.**

> **BOTH HALVES OF THAT ARE CLOSED.** Re-derived 2026-08-18 against a fresh
> regeneration of `use_without_item.json` off the pinned jar:
>
> * `probeStancesUnavailable` is **`{}`** — no stance is unavailable, so the
>   blind spot this section is about no longer exists;
> * `FlowerPotBlock` has **39 rows and zero refused cells**. The whole
>   `Components not bound yet` cause is gone from the table: it refuses 160
>   states in two causes now, 128 `environmentAttributes` (the 16 beds) and 32
>   `getChunkSource` (`daylight_detector`).
>
> So the "single highest-value follow-up here" has been made, and the row in
> the table above that reads "38, all `FlowerPotBlock`" is **1, all
> `DaylightDetectorBlock`** — `minecraft:dragon_egg` is `random: true` with no
> cells at all and is not an all-refusal row despite counting as one under a
> naive `refused == len(cells)` test. The "16, `BedBlock`" row still holds.
>
> Counts here are gated: `ci/refusal-ledger.py --cite` ties the live sentences
> that quote this table to the table itself, so the next figure to go stale
> reddens on the commit that breaks it rather than a week later. This block was
> written after two other documents were found naming *this same closed cause*
> as the cheapest work left in the module.

### The single next item

**Port the 17 by hand from their bytecode and delete their entries from
`ci/checks/use-probe-dependent.toml`.** The `STALE` rule then fires and tells
whoever does it that the job is finished — that is the closing condition, and it
is mechanical.

`FenceGateBlock` and `DaylightDetectorBlock` are already done on
`gameplay-block-interactions`, which is not in this branch's base; when both
land, those 13 entries go and the list is down to 4.

**Do not bulk-convert on suspicion.** The 122 is an upper bound, registered
unquotable as a defect count for exactly that reason.
