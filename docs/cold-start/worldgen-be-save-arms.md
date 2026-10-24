# COLD START — branch `worldgen-be-save-arms`

### COLD START — branch `worldgen-be-save-arms`, 2026-08-18 (the nine arms, and the one that was never a class)

**Based on `worldgen-jigsaw-random`, not on `main`.** The arms need
`structure::place::place_piece` to record what a template held, and that call
site exists only on that branch. `worldgen-block-entities` has the channel but
no recording in `place.rs`, so building here would have duplicated it and
collided at the merge.

**The claim §42.1 made was "the other nine classes are one item rather than
nine ports", and it holds in a stronger form than it was written in:** nine of
the ten are served by **four** arms, not nine.

* **The four containers share one save.** `Chest`, `Barrel`, `Dispenser` and
  `HopperBlockEntity.saveAdditional` are the same three instructions in the
  26.2 jar; the hopper adds `putInt("TransferCooldown", ...)` **after** the
  if/else and outside it, so a loot-bearing hopper writes it too.
* **The spawner's seven constant shorts** — already landed, §43.
* **The vault's config normalisation** — below.
* **The brushable's tag-carried seed** — written, unexercised, see the nulls.

**The finding that was not a class at all.** The recording added in §43 lived
INSIDE the guard on the container draw, and the two are different kinds of
thing: the draw is `placeInWorld`'s and is parity-relevant, the record is data
the piece already holds. Nesting them meant **`vault`, `trial_spawner` and
every other non-container class could never be recorded** — not for want of a
port but because they are not `RandomizableContainer`s — and meant **no class
at all was recorded with `RUSTMC_JIGSAW_STREAM` unset**, which is the default.

**The measurement**, r=16 seed-12345, vanilla first, `payload=0 id=0`
throughout:

```text
                       ours_be  missing  payload  exact
  §43   channel           71      105       0       68
  §44.10 stream          132       44       0      129
  45.6  arm off          102       74       0       99
  45.6  arm stream       162       14       0      159
```

The `off` arm moving 105 -> 74 **with no draw spent anywhere** is the part that
matters for the owner's pending decision: `vault` and `trial_spawner` carry no
loot and never needed a stream, so those positions land at the default setting
and are independent of §44.7.

**Killed as a null — do not re-run these:**

* **Do not look for a ninth or tenth `complete` arm.** Four arms serve nine
  classes and the tenth is derived. What is still missing from the window is a
  placement path and a draw, neither of which is an arm.
* **Do not carry the template tag through unfiltered.** It is not the saved
  tag: the block entity loads it and re-encodes, dropping what it does not
  understand and every codec field equal to its default.
  `saved_template_keys` is the one place that decides, and a class with no
  entry keeps nothing so the default is an absence.
* **Do not read `config:compound=13` as "nine vaults have an empty config".**
  That is this branch's own first answer and it was wrong in the opposite
  direction. All thirteen templates carry a config; `key_item` is a plain
  `fieldOf` and survives, `loot_table` is a `lenientOptionalFieldOf` and is
  dropped at its default. Nine save `{key_item}`, four ominous ones save both.
* **Do not synthesise `Items:[]` for a `chest` or a `dispenser`, or let their
  template `Items` through.** Measured: one chest scores a payload mismatch,
  because its table arrives from a data marker after the template is placed.
  All thirty chests and all nineteen dispensers on the window save a
  `LootTable`, so a bare one is outside what this window can speak for.
* **`brushable_block` is not a missing arm.** All seven belong to
  `minecraft:ocean_ruin_cold`, which does not go through `place_piece` at all
  — `EmitStats` reports `orphaned 0 incomplete 0`, so there is no record to
  refuse. Its arm is written and is **unexercised**; do not quote it as
  measured.

**Two scripts were repaired, and in both cases the control is what found it:**

* `ci/jigsaw-stream-bediff.sh`'s header says it refuses without its positive
  control and the body only printed it — a run whose dumps both died on a
  stale `data/vanilla` printed "(no emit line found)", ran `bediff` against
  directories that did not exist and **exited 0**. Four conditions now refuse.
* `ci/be-channel-falsify.sh` could not stage a workspace with an out-of-tree
  member (`grit-plugin-api = { path = "../plugins/..." }` resolves to
  `/tmp/plugins`), so every arm came back REFUSED. Its clean-tree control said
  the reds were unattributable rather than counting them as catches, which is
  the only reason this is a fixed bug and not a published green. **Verified
  end to end after the fix, not just past the manifest error**: injection 2
  caught by `monster_room`'s draw tests and confirmed blind to the identity
  gate, injection 1 caught by the comparison at `7 block state(s) moved`, and
  the clean tree green — `GREEN both defects were caught by the instrument
  that owns them`.

Both jigsaw-stream controls now also carry a `--selftest` and **both are gated
in `ci/t0.sh`**: each drives its conditions with crafted logs and no dump, and
two of the bediff's eight cases and three of the villages' eight must stay
SOUND, so a control that refuses everything fails as loudly as one that
refuses nothing.

**The single next item, and what closes it:** `decorated_pot` (31) and
`dispenser` (19) in the `off` arm are §43.5's draw, not a class — they close
when the owner decides the `RUSTMC_JIGSAW_STREAM` default (§44.7, §44.10).
Independently of that, routing ocean ruins through `place_piece` would exercise
the brushable arm; it closes when the seven `brushable_block` positions leave
`missing` with `payload=0 id=0` still holding.
