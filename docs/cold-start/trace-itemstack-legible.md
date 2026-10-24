# COLD START — branch `trace-itemstack-legible`

### COLD START — branch `trace-itemstack-legible`, 2026-08-12

**The item.** `docs/PARITY-ROADMAP.md` §21.4: an `ItemStack` lived inside the
undecoded tail `trace::observe` digests into `rest`, so the 1,537-entry item
registry had no numerator from any recording, and twenty-three checks that turn
on what is *in* a slot were checking the shape of a packet whose payload the
recorder had already thrown into a hash. Full write-up in
`docs/PARITY-ROADMAP.md` §38; only what a next agent needs is repeated here.

## What was made legible, and what was deliberately left digested

Decoded, from `javap` on the pinned jar alone (`ItemStack$1.decode`,
`DataComponentPatch$3.decode`, `ByteBufCodecs$29.decode`):

* `count`, the item's registry id and its name;
* the two data-component patch counts;
* the removed component type ids, when the patch adds nothing — removals are
  self-delimiting and so the whole stack is legible;
* arms for `SET_CURSOR_ITEM` (96) and `SET_PLAYER_INVENTORY` (108), which had
  no decoder at all and were recorded as `packet:0x60` and `packet:0x6C`.
  Checks 607 and 720 are renamed off those literals; **720's own header said
  the fix was to write the arm**, so this is that instruction carried out.

**Left digested, and this is the decision to argue with rather than the code:**
every *added* component's payload. An added component carries **no length** —
it is written by that component type's own stream codec, so it can only be
skipped by running that codec. The decoder therefore reads the head, the two
counts and the first added type id (which names the component a codec would
have to be written for) and **stops**, leaving every remaining byte in `rest`.
One opaque stack stops its whole packet; the trace says `truncated` rather
than leaving a short list to be misread as a short inventory.

Nothing leaves the comparison either way: the projection gains fields and loses
no bytes. The one strictness spent is VarInt non-canonicality (`0x01` vs
`0x81 0x00`), stated in the decoder's doc comment.

## The proof the decoder can fail

Three refusals, each a place vanilla's own decoder throws: a short read
(`readVarInt` on an exhausted buffer), an item id the registry does not hold
(`IdMap.byIdOrThrow`), and a negative patch count. `count <= 0` is **not** one
of them — it is vanilla's encoding of empty, recorded as `{"count": 0}` — so a
genuine empty stack and a refusal can never read alike, which was the whole
risk. A refusal records `decode = failed` with the whole body digested.

A fourth test feeds two stacks differing in nothing but an opaque component
payload and asserts their `rest` digests differ, so the stop is not swallowing
bytes.

**And the wire form is pinned against vanilla's own recording, with nothing
booted.** Check 350's header recorded `rest = d2fbb5fb1fa81421` on 2026-08-03,
when the arm consumed only `window`/`state`/`slot` — so that tail *is* the
stack. Exactly one `(count, item id)` preimage exists under the transcribed
form: count 2, item 350 = `minecraft:torch`, which is what check 350's own
`[[setup]]` holds. `the_stack_form_reproduces_a_digest_recorded_by_vanilla`.

## The projection moved — the SOURCE, not a final id

**Touched: `trace.rs`**, one of the three `PROJECTION_SOURCES` (`compare.rs`,
`scenario.rs`, `trace.rs`). That is the durable fact.

**Do not quote a final id from this branch.** `trace-housekeeping-blindness`
moves the same identity by a different route (it moves the twelve-packet
housekeeping exclusion out of `trace.rs` record time and into
`compare::project`, touching two of the three sources), so two branches are
moving one identity and **neither re-stamps** — the orchestrator sequences both
and re-stamps once, afterwards. `ci/figures.toml` is deliberately untouched
here for that reason.

The measurements below were taken under `proj:67604e5eb262728f`, which is this
branch's tree in isolation off `origin/main` `84f789db`. That is the condition
of the run, not a prediction of the tree.

**Every corpus floor in the tree stamped `proj:7cefde0fb56d9f3e` is stale**,
including the standing `floor 4/92` at ensemble
`coverage92-selfcheck-n6-20260811` (`ci/figures.toml`,
`corpus-floor-n6-coverage92`) — twenty-eight places carry that stamp. Adding a
*test* does not move the identity: `projection_body` strips comments and the
test module, and that was checked rather than assumed, twice.

## Re-measured — the floor did not move, and the numerator is 12/1,537

One corpus-lock hold, `ci/stackdec-ensemble.sh`. Ensemble `20260812T2037Z`,
93 scenarios, N = 6, 15 pairs, one recorder (`8ac93eb6def2e0fb`).

```text
  CORPUS FLOOR  4/93  under proj:67604e5eb262728f, p_hide 39.3%
                same four scenario names as the old 4/92

  item   12 / 1537  0.78%   per-run 11..12, pooled 12
         539 stacks decoded over 521 stack-bearing observations
         0 stopped at a data component with no codec
```

**The floor did not move and the zero is the load-bearing number.** Decoding a
field that varies between two innocent vanilla runs would have manufactured a
floor; none of the four flapping scenarios is new and none is an item field.
Not one stack in 539 carried a component this decoder could not walk, so the
deliberate stop never fired and the numerator is not an underestimate hiding
behind an opaque tail.

The twelve: `stone` (297), `stick` (102), `oak_log` (48), `chest` (18),
`torch` (14), `furnace`, `oak_planks`, `dirt` (12 each), `oak_stairs`,
`redstone_block`, `crafting_table`, `dandelion` (6 each).

**Two non-green exits in the ensemble, both pre-existing.** `SELFCHECK
FAILED — NEWLY UNSTABLE random-ticks-turn-covered-grass-to-dirt (vacuous,
5/5)`: `780` observes `block_update`/`section_blocks_update` only, so no item
arm can reach it — it is vacuous because `ci/t2-gameplay-selfcheck.sh` does not
apply a scenario's `[[gamerule]]`, which §26 records against itself. And
`CORPUS FLOOR FAILED: 2 declared/measured disagreements` — `water-spreads` with
no usable pair, plus that dead `780`.

**This is a different ensemble from §21's**, so the other registry rows moved
with it (`sound_event` 29 → 26, `block` 43 → 42, `block_state` 87 → 79) and
those movements are not this branch's doing and are not a comparison. Only the
item row is new information.

## Nulls — killed, do not re-run

* **Do not write speculative codecs for data component types.** The corpus has
  to produce one first; when it does, its type id is in the trace under
  `opaque_component` and that is the moment to write that one codec. A codec
  for a component nothing exercises is a decoder that cannot fail, in the part
  of a stack most likely to carry per-run variance.
* **`set_entity_data` is not a route to items and was not attempted.** An
  `ItemEntity`'s stack rides entity metadata, which is a typed accessor list
  with no decoder here; it is a separate item, not a corner of this one.
* **Our own server cannot produce a component patch today** — `crate::menu`'s
  writer emits `0, 0` and the creative slot decoder reads none
  (`crates/rustmc-server/src/item.rs`, `is_same_item_same_components`). So the
  opaque path can only fire on vanilla's side in this corpus.
* **The old six-recording ensemble cannot be re-projected into an item
  numerator.** The item fields do not exist in a recording taken before the
  decoder; `ci/vanilla-reach-census.py` now refuses such an ensemble by name
  rather than reporting its zero.

## Against `trace.rs`'s own recording rule

The module's rule is *record at full fidelity; be lenient only when comparing*.
This passes its actual test — no byte is discarded, everything unnamed is still
in `rest` — but three record-time decisions are worth separating, and §38.7
argues them:

1. **VarInt canonicality** is a genuine fidelity loss and **cannot** be moved
   to the projection; the decision to consume a byte is the recorder's by
   construction. Spent, small, documented in the decoder.
2. **The stop at an added component is not leniency**, and the rule's remedy is
   unavailable for it: `compare::project` reads observations, not bytes, so it
   can drop what the recorder produced but cannot parse what it did not. The
   only fix is a per-type codec. Measured empty in this corpus (0 of 539).
3. **The one move toward leniency went to the right layer.** In `rest` the
   stack was all-or-nothing; as fields, a projection *could* drop the item.
   None does, and any that ever did would have to say so in `compare::project`
   where it is visible.

## The single next item

**Not the opaque count — that is measured and it is zero.** The next item is
the first corpus scenario carrying an item with a data component (enchanted,
damaged, or renamed). Nothing in the corpus produces one today, which is why
the stop has never fired; the moment one exists, its `opaque_component` type id
is in the trace and that is the single codec worth writing. What closes it: an
item numerator taken over a corpus that contains such a scenario, still with
`0 stopped at a data component with no codec`.
