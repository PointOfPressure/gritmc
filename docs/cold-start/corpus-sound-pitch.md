# COLD START — branch `corpus-sound-pitch`

### COLD START — branch `corpus-sound-pitch`, 2026-08-17, area: making `sound.pitch` gradeable

**What the branch is.** `569323e` read `ClientboundSoundPacket`'s pitch and
threw it away. This branch compares it again — not as a float, which would
reintroduce the problem `569323e` fixed, but **per call site**, read out of the
pinned jar. `docs/PARITY-ROADMAP.md` §60 is the full account; this file is the
short version and the list of what is left.

**The mechanism, in one paragraph.** `tools/oracle-runner/src/pitch.rs`
classifies each sound event by name: `Constant` (compare the float exactly),
`Drawn { lo, hi, label }` (compare the range it is drawn from), or unclassified
(drop the field, which is `569323e`'s behaviour and is the **default**).
`trace::decode` writes the classified value; `client::Tables::load` installs the
`minecraft:sound_event` registry from `data/vanilla/protocol_ids.json` into a
`OnceLock`. Exactness needs no tolerance because both sides' pitch arrives as
the same four wire bytes of an `f32` through one `read_f32`.

**Why unclassified-is-dropped is the important line.** It is what makes this
change unable to redden anything by omission. Only a table entry can move a
verdict, and every entry is justified by a call site read with
`ci/sound-pitch-callsites.sh`.

## What I measured, including the bit that refuted my own brief

**The brief said `lever-flips-when-right-clicked` (290) reporting `match`
cannot tell a lever that turned on from one that turned off. That is false and
I checked it before building anything.** 290's `observe` list does not name
`sound` at all, and its recorded `block_update` is
`state_id 6773`, `state "minecraft:lever[face=floor,facing=south,powered=true]"`.
The direction is in two fields, and `ci/checks/gameplay-falsifiable.toml`
already lists `perturb:state_id` and `perturb:state` among 290's kills.

**The true statement is narrower, and it lands on 420 rather than 290.** With
the pitch gone, a lever's *sound* observation was
`{at, rest, sound: 935, source: 4, volume: 0.3}` — identical whichever way it
went. For 290 that costs coverage of the sound rule but not the check. For
`a-comparator-switches-to-subtract-mode` (420) it costs the check: 420's step is
a `place`, so `compare::informative` subtracts a `block_update` at the clicked
position and the one above it — which is exactly the `block_update` carrying
`mode=subtract` — leaving **the sound as 420's single informative
observation**, and that sound read identically for both modes. Claim 420 is
named for `0.55f` against `0.5f` and could not see either.

**A value-keyed rule ("anything in [0.9, 1.0) is drawn") is wrong and this is
the null I killed before writing code.** The ~90 sound ids the corpus emits
were extracted from every `vanilla.json` on the box, resolved through
`protocol_ids.json`, and their call sites read. Three formulas, three ranges:
chest/door/fence-gate `nextFloat()*0.1f+0.9f` → `[0.9, 1.0)`;
`FlintAndSteelItem.useOn` `nextFloat()*0.4f+0.8f` → `[0.8, 1.2)`;
`LivingEntity.getVoicePitch` `(nextFloat()-nextFloat())*0.2f+1.0f` → `(0.8, 1.2)`.
Both non-chest families are live in the corpus (`item.flintandsteel.use` in
`flint-and-steel-lights-a-fire`, `entity.pig.ambient` in
`spawn-egg-makes-a-mob`), so a value rule would have reddened real scenarios
while claiming to strengthen them. **Do not re-open this.**

## Nulls — do not re-run these

* **A tolerance on the pitch float.** Not needed and not wanted. The hazard
  `569323e` hit was randomness, not precision: identical wire bytes decode
  identically, so a tolerance would only blur a constant into a neighbouring
  constant (the comparator's `0.55f` and the lever's `0.6f` are 0.05 apart).
* **Quantising the pitch to a fixed grid.** Same objection as the value rule —
  a grid coarse enough to swallow a draw swallows `0.55` against `0.5`.
* **Reproducing vanilla's draw.** It comes off `ServerLevel`'s `RandomSource`,
  whose sequence depends on everything the level has consumed since load. Out
  of reach and explicitly not the goal; `sounds::nine_tenths_pitch` deliberately
  does *not* use a reproducible source, so that nothing can be written against
  it.

## What still cannot be graded, named rather than left to be found

* **`block.note_block.*`** — `Math.pow(2.0d, (note-12)/12.0d)`, deterministic
  so the range treatment is wrong, but specified to 1 ulp rather than correctly
  rounded so exactness would assert two libms agree bit for bit. Classified as
  neither; pitch still dropped. `a-note-block-retunes-when-right-clicked`
  grades its retune on block state and `block_event`, not on the audible pitch.
* **Baby mob voices** — `getVoicePitch`'s baby branch draws from `(1.3, 1.7)`.
  Only the adult range is declared; a union would accept a pitch neither branch
  can produce. A baby in the corpus falls out of range and compares exactly,
  which is a visible refusal rather than a silent widening.
* **A constant *inside* a declared draw range.** A server hard-coding 0.95 for
  `block.chest.open` still matches. Strictly better than the blanket drop,
  which was blind in both directions; strictly worse than comparing the draw,
  which nothing can do.
* **The unclassified remainder of the registry.** `pitch::coverage()` reports
  classified-of-registry so this is a number rather than an impression.

## The single next item

**Drive `pitch::coverage()` down by reading more call sites.** The table covers
the families the corpus exercises, not the registry. The result that closes it
is a coverage figure quoted with its denominator and every new entry carrying
the class and offsets it was read from, via `ci/sound-pitch-callsites.sh`. The
one entry that must NOT be added without new argument is `block.note_block.*` —
see above.
