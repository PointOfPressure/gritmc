# COLD START — branch `gameplay-useitemon-unit`

### COLD START — branch `gameplay-useitemon-unit`, 2026-08-13

**The item: §39 put `useItemOn` at 25 declaring classes and 89 distinct
behaviours — a 3.6x fan-out — and left the second axis unnamed. Find it, and
recompute the coverage figure in a unit where covering one member predicts the
others. `docs/PARITY-ROADMAP.md` §43.**

Remote dir `~/rustmc-uio`. Rebased mid-item from `0679f236` onto `a106314d`
after `gameplay-block-entities` merged, and **that rebase is the most useful
thing that happened to this branch** — see "the claim survived a 3x move".

### The answer

The briefed hypothesis was that the fan-out *is* the item dimension, because
`useItemOn` is handed the `ItemStack` and `useWithoutItem` is not. **It is
not.** The item is a real axis that the 89 never counted at all, and the 3.6x
is something else entirely.

```
  25 -> 89 decomposes as 68 + 21
     68  blocks in 8 classes measured at NOTHING (NoBlockEntity), charged 1 each
     21  behaviour groups over the 17 classes that were measured

  fan-out among MEASURED classes  1.24x     (useWithoutItem: 1.03x)
```

So **76% of the 89 is the harness's blind spot, not vanilla's dispatch**, and
§39's headline — "class membership does not predict behaviour" — does not
survive the decomposition. The class predicts `useItemOn` behaviour about as
well as it predicts `useWithoutItem` behaviour. §39's *own next sentence* is
what survives, that the figure is a statement about the refusal hole; §43 is
that sentence measured.

The item axis, from the table (it was already there — `UseItemOnGolden` probes
all 1,537 items on all 4,537 states, 92,101,730 probes):

```
  21 measured behaviour groups -> 109 distinct answers        STRICT
                               ->  45 once payload is merged  CONTROL
  20 of the 21 depend on the held item at all.
```

**The unit is (behaviour group, item control-class): 113 = 68 + 45.** After the
payload merge the item axis is 2 wide for 16 of the 20 item-dependent groups
and never wider than 3.

The merge is read off the jar, not chosen: six classes branch on the item
through a map or predicate returning a *payload*, not a code path.
`FlowerPotBlock`'s `POTTED_BY_CONTENT.getOrDefault` serves all 38 potted
flowers through one branch — strict 39 answers, 2 control units. `CakeBlock`
18 -> 2. The four cauldrons 7/5/5/4 -> 3/2/3/2.

### The class figure is not a figure with a floor — it IS the floor

Every state's cell table is one `default` answer plus exceptions, and the
default is what nearly all 1,537 items get. So a corpus holding **one arbitrary
item** covers every reachable block's default class for free, and the floor of
an item-aware figure is the count of reachable behaviour groups — exactly what
a class-keyed figure counts.

| | `0679f236` | `a106314d` |
|---|---|---|
| by declaring class | 3 of 25, 12.00% | 9 of 25, 36.00% |
| by interaction, CONTROL | 4 of 113, 3.54% | 11 of 113, 9.73% |
| of which floor | 3 | 9 |
| above the floor | +1 | +2 |

**The claim survived a 3x move.** The class numerator equals the CONTROL floor
on both corpora, before and after a merge that took the corpus from 92
scenarios to 105 and held items from 43 to 55. One measurement is a
coincidence; two through a 3x change in the input is an identity.

**The class figure overstates by 3.4x.** §39's 3-of-89 is nearly right by
accident: it carries the same 68 refusal charges, and its inflated denominator
almost exactly cancels the axis it never counted. Quote it and you are close;
reason from it about where the work *is* and you will send somebody to write
one check where two branches live.

### The corpus check that came out of it

`890-a-skeleton-skull-goes-on-top-of-a-note-block-instead-of-playing-it`.
`NoteBlock.useItemOn` returns `PASS` instead of `TRY_WITH_EMPTY_HAND` for the
seven `NOTE_BLOCK_TOP_INSTRUMENTS` items **on the UP face only**, so
`useWithoutItem` never runs, the note does not advance, and the skull is placed
above instead. `430` is its control and is deliberately not duplicated — same
block, position, verb and face, one item different.

It exists because **the instrument that found the gap cannot close it.**
`ci/useitemon-unit.py` already scores that cell reached, from `766` (holds a
skeleton skull) and `430` (places a note block) — two scenarios that have never
met. Reach is a conjunction evaluated over the whole corpus at once, so a bound
can be satisfied by two checks that never touch. This is the first place in the
corpus where a bound and a record differ by something a server can fail.

### What was killed as a null — do not re-run these

* **"The fan-out is the item dimension."** The briefed hypothesis. It is not:
  68 of the 89 are refused blocks and the item axis is a *third* axis, absent
  from that number in both directions.
* **"§39 was wrong that class membership does not predict `useItemOn`."** Do
  not re-open it as an error — §39 states the refusal caveat itself, in the
  same paragraph. What is new is the decomposition, not a correction.
* **A `890`/`891` pair, note block with a stick and note block with a skull.**
  The stick half is `430`, already in the corpus and identical step-for-step
  down to the face. Written, then deleted on discovering it. Check `430` before
  writing any note-block rig.
* **Grouping the table's rows by their `class` column.** That column is
  `getClass().getSimpleName()` — the **runtime** class, 28 values. The header's
  `overridingClasses` is the declaring set, 25. Grouping by the stored column
  measures inheritance and reports it as dispatch.
* **Trusting `cp -rn` for `use_item_on.json`.** `ci/gen-gameplay-tables.sh`
  does not derive it — no server source mentions the path yet — so setup
  leaves a **foreign** copy behind and nothing regenerates it. Run
  `ci/use-item-on-golden.sh` by hand before measuring. (The copy happened to be
  byte-identical this time. That is luck, not a property.)

### The stated blind spots

* **Reach bounds, not records.** Unchanged from §39, and now with a named
  instance: the `NoteBlock` + skull cell is scored reached by two scenarios
  that never meet. `ci/useitemon-unit.py` narrows the item side — it scores the
  item in hand *at a `place`*, the only verb that sends `UseItemOn` — but on
  today's corpus that changes nothing (55 held, 53 clicked, 11 units either
  way).
* **Six of the nine covered classes were measured at nothing.** The
  block-entity merge tripled the numerator largely by making refused classes
  reachable: progress on the corpus, none on the evidence.
* **The 68 are charged, not measured.** If the golden harness ever holds a
  block entity, the 1.24x above says they should collapse hard —
  `SignBlock`'s 24 blocks are one implementation — and this denominator moves.

### The single next item

**Give `UseItemOnGolden` a block entity and re-take the table.** Every figure
in §43 is dominated by the 68, all refused with one reason
(`NoBlockEntity: the rule asked for a block entity and this harness holds
none`), and 2,593 of the table's refusals are that single line. `UseGolden`'s
`TableLevel` returns null from `getBlockEntity`; the eight classes need only
enough of one to get past the ask. Closing result: the 68 replaced by a
measured group count, and the first `useItemOn` coverage denominator that is
not three-quarters guess.
