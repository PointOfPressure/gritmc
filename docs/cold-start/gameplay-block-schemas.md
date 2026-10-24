# COLD START — branch `gameplay-block-schemas`

### COLD START — branch `gameplay-block-schemas`, 2026-08-13

**The item: the 116 block-property schemas are a work queue. Rank them, say
which are behaviourally inert, and cover the non-inert head with checks that
discriminate.** Remote dir `~/rustmc-schema`, base `a106314d`.
`docs/PARITY-ROADMAP.md` §48.

Two things in the brief were wrong and the tree said so within the first
twenty minutes. Both are recorded here so the next agent does not re-derive
them.

* **The brief attributed the 116-schema decomposition to
  `gameplay-block-classes`.** It is `gameplay-block-placement`
  (`68dcc5dc`, `f773183e`), and that branch is **unmerged and not an ancestor
  of `a106314d`** — so the `block_schema` unit, scenarios `730`-`732` and the
  roadmap section the brief told me to quote are all absent from `main`. This
  branch therefore re-derives the identity in its own instrument rather than
  building on the census, and claims its own section number. If
  `gameplay-block-placement` lands later, §48 and its §41 are two
  presentations of the same identity and should be reconciled, not stacked.
* **The brief's figures (26.72% -> 28.45%, "the 116 base") are a different
  corpus.** They were taken on the placement branch's 96-scenario corpus.
  This branch's base corpus is 106. Do not carry them across; see §48.2 on
  why cross-corpus differencing measures the corpora.

### The instrument, and why it has two numerators

`ci/block-schema-queue.py`. The census's identity holds — for all 1,196
blocks `len(states) == product(domain sizes)`, zero exceptions, so 32,366 is a
Cartesian product — and the script asserts it rather than trusting it.

The part that is new is that **the obvious replacement numerator is pumpable
too**. Scoring a schema "reached" when any state of any member block is seen
means one copper grate marks the 33-block `waterlogged` schema without water
ever having existed in the corpus. So:

```text
  touched   >= 1 distinct state of the schema seen
  varied    >= 2 distinct states seen — the property actually moved
```

Six cold vanilla runs of one corpus (`~/rustmc-stackdec`, N=6, 2026-08-12):

```text
  touched   30 / 116   25.86%     identical in all six runs
  varied    19 / 116   16.38%     identical in all six runs
```

**Floor 0 at N=6.** Positive control: the six recordings are six *different*
files (distinct md5s) that each reach exactly 79 state ids. A one-schema
movement is real. This is unusually good resolving power for this project and
it is the only reason a `+1` here is quotable.

### The ranked queue — the head, by block count

```text
   #  blocks  states  reach     signature
   1     399     399  touched   (NO PROPERTIES)          <- empty, see below
   2      68     408  VARIED    type waterlogged
   3      64    5120  VARIED    facing half shape waterlogged
   4      58     174  touched   axis                     <- one value only
   5      44     176  touched   facing (4)               <- one value only
   6      39    1248  VARIED    east north south waterlogged west
   7      37     296  VARIED    facing waterlogged
   8      33      66  --        waterlogged              <- untouched
   9      32   10368  --        east north south up waterlogged west (walls)
  10      21    1344  VARIED    facing half hinge open powered
  11      21    1344  VARIED    facing half open powered waterlogged
  12      21      42  touched   lit                      <- one value only
  13      18     108  --        facing (6)               <- taken by 963
  14      17     272  --        candles lit waterlogged
  15      16     256  --        rotation (banners)
  16      16     256  VARIED    facing occupied part
  17      15     360  VARIED    face facing powered
  18      14      28  --        powered (pressure plates)
```

`./ci/block-schema-queue.py --recordings '<glob>'` regenerates it;
`--unvaried` prints the eleven single-state schemas with the one witnessing
state and the scenario that produced it.

### Which schemas are behaviourally inert — one, and the obvious test is wrong

**Exactly one of the 116 is inert, and it is the empty one.**

The largest signature by block count has **no properties at all**: 399 blocks,
34.4% of the registry. It is permanently `touched` (its witness in every
recording checked is `minecraft:air`, from `break-block`) and can never be
`varied`. Covering it is a check that cannot fail. It is worse than empty —
it collapses 399 blocks with 399 different behaviours into one satisfied bit.
Named as a remainder; `--exclude-empty` gives 29/115 = 25.22%, **lower** than
30/116, because the deleted entry was already scored.

**The brief's proposed criterion — strike out schemas whose properties only
drive the client model — does not hold, and this is the finding worth
carrying.** A jar-wide `javap` scan over `net/minecraft/**` finds seven
property *values* never read by any server code:

```text
  snowy   dusted   locked   crafting   has_bottle_0/1/2
```

Positive control for the scan: 126 read sites for `waterlogged`, 74 for
`powered`, 43 for `lit`. `snowy`'s single hit is a `hasProperty` schema probe,
not a value read. Three schemas consist *entirely* of write-only properties:
`snowy` (grass_block, mycelium, podzol), `dusted` (2 blocks),
`has_bottle_*` (brewing_stand).

**None of the three is inert.** All seven properties are *computed by server
logic and broadcast* — `SnowyDirtBlock.updateShape` and `getStateForPlacement`
both derive `snowy` via `isSnowySetting(above)` and `setValue` it; `locked`
out of `RepeaterBlock`; `dusted` out of brushing; the rest out of block-entity
ticks. The corpus reads `block_update`/`section_blocks_update`, which carry
the **whole state id**, so a property the server puts on the wire is
observable whether or not any code reads it back. *Write-only* and *inert* are
different properties and only the second would justify striking an entry out.

### What was actually wrong with the coverage, and it is not thinness

Eleven schemas are touched at **exactly one state**. Three of them are the
largest property-carrying signatures in the registry:

```text
  58 blocks  axis    only ever axis=x       030-place-log-axis clicks one face
  44 blocks  facing  only ever facing=east  one wall torch
  21 blocks  lit     only ever lit=true     nothing has watched it clear
```

**123 blocks whose schema is satisfied by a constant.** A server whose
`RotatedPillarBlock` returns `Axis.X` unconditionally, or that lights a lamp
and never un-lights it, passes the corpus as it stood. That is the
`230`/`235`, `330`/`335` shape — a check plus the partner that stops a
constant passing it — standing unpartnered over the three biggest schemas.

So the head of this queue was **not** "go and place 85 more blocks". Three of
the four checks added here touch no new schema at all; they make an existing
one move.

### What was added — `960`-`963`, and `790` -> `999`

```text
  960  a log clicked on the top and the side lies two ways
  961  an anvil and a pumpkin from one yaw are a quarter turn apart
  962  a redstone lamp goes out when the lever goes back
  963  an end rod placed on an end rod points the other way
```

Every rule below is from `javap -p -c -constants` on the pinned jar; the
offsets are in each file's header.

* **`960`** — `RotatedPillarBlock.getStateForPlacement` is one line:
  `axis <- clickedFace.getAxis()`, no other arm. `030` clicks `east` and gets
  `x`; this clicks `up` and `south` for `y` and `z` and closes the domain.
* **`961`** — `AnvilBlock` takes `getHorizontalDirection().getClockWise()`,
  `CarvedPumpkinBlock` takes `.getOpposite()`. From one unchanged `yaw = 0`
  (south) that is `west` and `north` — a **quarter** turn apart, inside one
  schema. `040` does the 180-degree version across *two* schemas and therefore
  cannot see a `getClockWise` implemented backwards; this can.
* **`962`** — `RedstoneLampBlock.neighborChanged` lights through `setBlock`
  and goes out through `scheduleTick`, landing in `tick()`. **The falling edge
  is a different method on the scheduled-tick path**, and nothing in the
  corpus has ever required a server to have it. Step 1 is the rising edge as a
  control, deliberately a step and not setup, so a dead rig cannot read as a
  pass.
* **`963`** — `EndRodBlock.getStateForPlacement` reads the *neighbour's* own
  `FACING` and inverts when it equals the clicked face. Largest untouched
  non-wall schema (18 blocks). The other 17 members are shulker boxes and are
  refused here on purpose: `open_screen` projects `window`, the per-player
  `containerCounter` with no reset.

`790-a-flower-pot-empties-when-right-clicked` was renamed to `999-`. The
corpus README requires the pot to run last and offers two arms — number below
it, or move it up and say so in its header — and `960`-`999` is this branch's
allocated range. Its `name` key is unchanged, so its baseline and unstable
rows are untouched and its verdict does not move; corpus order for every
pre-existing check is unchanged. The README gained the **second** reason for
the rule, which is stronger than the one it had: the pot is baselined
`diverge`, so after it runs the two servers' inventories already differ, and a
later check inherits a free-slot layout that differs *between servers* — which
is the side-effects-reach-another-scenario case the corpus deletes checks for.

### Red-when-broken, taken against OUR SERVER ALONE — no vanilla, no lock

`ci/schema-check-falsify.sh` (pins the server) and `ci/schema-check-falsify.py`
(flips a recording). **4/4 go red, each moving exactly one scenario.** The
agreement half still belongs to `ci/t2-gameplay.sh`; this is the half that
costs a minute instead of forty, and running it first found three harness
defects before a corpus slot was spent.

`data/vanilla/placement.json` is read from disk **per connection**
(`session_tables.rs` -> `placement.rs`, a plain `read_to_string`; no
`build.rs`, nothing baked into the binary), so a placement property is pinned
by editing data — no rebuild, which matters because a rebuild would also
rebuild the code under test. Each `table` row is `states` plus 144 `cells`
indexing into it, so pinning is one edit: make every entry of `states` the
same string.

```text
  960  table[2]    oak_log  [y,z,x]   -> all axis=x   match -> diverge -> match
  961  table[232]  anvil    [w,n,e,s] -> all north    match -> diverge -> match
```

The restore is **asserted, not assumed**: a red that does not go away when the
pin is lifted has not been attributed to the pin.

**`962` and `963` are not pinned, because this server is already the wrong
model both were written against.** From the intact recording:

```text
  962  redstone_lamp[lit=true] ... lit=true   the lever goes powered=true ->
                                              false; the lamp never changes
  963  end_rod[facing=up] x7                  both rods take the clicked face
```

`crates/rustmc-server/src/power.rs:2427` calls `lamp_wants_a_tick`, which logs
*"this server has no lamp tick — it will not go out"* and pushes no
`TickRequest`; `ticks.rs` dispatches only torches, observers and repeaters.
`placement.rs:971`'s `computed_with_neighbours` handles `minecraft:ladder` and
`minecraft:redstone_wire` and nothing else; there is no `EndRodBlock` port in
`crates/`. Pinning a constant that is already the answer is a no-op dressed as
an experiment, so that evidence is **printed rather than asserted** — a script
asserting our server lacks a feature would go red the day someone adds it.
Their red-when-broken half comes from the recording flip instead, which proves
the narrower thing still worth proving: the projection carries the field.

```text
  960  oak_log[axis=y] -> x           3 obs   match -> diverge
  961  anvil[facing=west] -> north    3 obs   match -> diverge
  962  redstone_lamp[lit=true] -> false 2 obs match -> diverge
  963  end_rod[facing=up] -> down     7 obs   match -> diverge
```

The value pairs are **unordered** and the direction is read off the recording:
vanilla puts `end_rod[facing=down]` on the wire and we put `facing=up`, so a
fixed direction finds nothing in one of the two and refuses — which reads as a
dead check rather than as a wrong flip.

#### The three harness defects it caught, all before the corpus slot

* **The control demanded every scenario be `match`.** A legitimate
  self-comparison has one `vacuous`: `random-ticks-turn-covered-grass-to-dirt`,
  whose gamerule `ci/t2-gameplay.sh` shuts for every other check. The control
  would have failed on a perfectly good recording.
* **Nothing asserted a scenario was non-`vacuous` INTACT.** A rig that emitted
  nothing at all would have scored as a pass, since breaking a server cannot
  make it emit less than nothing. Now a hard fail that says *fix the rig, not
  this script*.
* **`rustmc-server` prints `listening on` BEFORE it binds**, then prints
  `listen: Address already in use` and keeps running. The first run of this
  script recorded all four scenarios against a stray server left on the port
  by an earlier killed job, and the pinned-table experiment measured a table
  that had never been pinned. **Grepping the log for `listening on` is not
  proof that the server you started is the one on the port — and
  `ci/t2-gameplay.sh:503` does exactly that.** This script now refuses if the
  port is held before it starts, refuses if the log says the bind failed, and
  confirms the listener is its own pid. The same hole is still open in the
  corpus runners and is the cheapest thing on this list for someone to close.

#### A free confirmation of the write-only argument

The intact recording carries `minecraft:grass_block[snowy=false]` in two rigs,
unasked. `snowy`'s *value* is read by no server code anywhere in the jar — and
here our own server computes it and puts it on the wire, where a check can
fail on it. The §48.4 argument was made from bytecode; this is the same claim
measured.

### What was killed as a null — do not re-run these

* **Striking "model-only" schemas out of the denominator.** The criterion does
  not survive contact with how the corpus observes. See above; seven write-only
  properties exist and not one of them is inert.
* **Banners (`rotation`, 16 blocks) as the next cheap schema.** The 16-value
  `rotation` formula is already exercised by the sign checks through the
  12-block `rotation waterlogged` schema, which reads `VARIED`. A banner adds
  a second instance of a rule already covered, and it is a block entity.
* **Shulker boxes as the way into the 18-block `facing` schema.** 17 of the 18
  members, and every one of them projects `window`. `end_rod` is the 18th,
  is not a block entity, and carries the better rule.
* **Leaves (`distance persistent waterlogged`, 11 blocks) this turn.** Not a
  null — a deferral with a reason. `LeavesBlock.updateShape` schedules a tick
  rather than writing, so the distance change is observable inside
  `settle_ms` and the check is viable; it needs a log rig and it is 11 blocks,
  so it ranked below the three constant-satisfied schemas.
* **Differencing two recording ensembles for the before/after.** cov2 reads
  31/116, stackdec 30/116, twin 29/116 — three corpora, not three samples.
  `--exclude-scenario` takes the before out of the after's own recording and
  refuses if the named scenario is absent, because subtracting nothing
  silently reports the after as the before.

### What moved — corpus 106 -> 110, N=1, 2026-08-13

Both figures out of the **same recording**; the before is the after with the
four new scenarios subtracted (`--exclude-scenario`), not a second run:

```text
                    before      after
  touched          39/116      40/116      33.62% -> 34.48%    +1
  varied           19/116      23/116      16.38% -> 19.83%    +4
```

One `varied` per check, one `touched` in total. `960`, `961` and `962` add no
new schema — they make an already-touched signature's property move for the
first time. Only `963` reaches a schema nothing had reached.

```text
  960  a-log-clicked-on-the-top-and-the-side-lies-two-ways        match
  961  an-anvil-and-a-pumpkin-from-one-yaw-are-a-quarter-turn...  match
  962  a-redstone-lamp-goes-out-when-the-lever-goes-back          diverge
  963  an-end-rod-placed-on-an-end-rod-points-the-other-way       diverge
```

Both divergences were predicted from the jar *before* the run and both are
missing mechanisms, not new breakage. Neither is a regression: nothing had
ever looked.

**N=1, so no floor is stamped for this corpus** — §48.2's floor 0 was six cold
runs of a *different* corpus and does not transfer. The **delta** is exact
regardless, because before and after are two projections of one file. Note
`touched` read 30/116 on `~/rustmc-stackdec` and 39/116 here purely from other
branches landing blocks, while `varied` was 19 in both: that is the whole
argument for carrying two numerators, measured rather than asserted.

The flower pot came back `diverge` with no REGRESSED flag, which confirms the
`790 -> 999` rename left its baseline row intact — the rows are keyed by name.

### The stated blind spots

* **`963` does not separate every wrong flip model.** It kills
  `facing <- clickedFace`. It does *not* distinguish a flip gated on
  `is(this)` alone from one that also checks the neighbour's facing, nor a
  flip that reads the clicked block instead of the block behind the new
  position — at step 2 those are the same block. Both need a rod placed on the
  *side* of a vertical one. Said in the file rather than left implied.
* **`varied` is a floor on rigour, not a certificate.** Two states of a
  schema means the property moved once. It says nothing about the other
  domain values, and for `axis` and `facing` this branch closes the domain
  only because those domains are 3 and 4 wide.
* **The four checks were added; the schema figure moves by construction for
  `963` only.** `960`, `961` and `962` move `varied` and leave `touched`
  where it was. That is the intended shape and it is why both numbers are
  reported.

### The single next item

**The 33-block `waterlogged`-only schema (#8) is the largest *untouched*
signature after walls, and the corpus cannot reach it — the substrate is a
superflat with no water.** Every member (33 blocks: corals, copper grates,
`barrier`, `heavy_core`, `mangrove_roots`, `conduit`) needs either a water
source or a bucket, and `place` with a `water_bucket` has never been exercised
by this corpus. Establish first whether `hold water_bucket` + `place` reaches
`BucketItem.useOn` through the harness at all — that is a one-scenario probe,
not a port — because if it does, the same rig unlocks the `waterlogged` factor
on 8 more schemas above it in the ranking.

Closing result: one scenario in `964`-`969`, a baseline row, and either
`touched` at 31/116 with `waterlogged` named in the observed list, or a
written null saying the harness cannot place a fluid and what would be needed
to change that.
