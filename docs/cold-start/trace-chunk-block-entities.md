# COLD START — branch `trace-chunk-block-entities`

### COLD START — branch `trace-chunk-block-entities`, 2026-08-12

**The item: give `crate::trace` an arm that decodes the block-entity section
of `LEVEL_CHUNK_WITH_LIGHT`, because 29 of the 49 block entity types are on no
other channel. Done, plus the positive control it turned out to need.
`docs/PARITY-ROADMAP.md` §31.**

### What moved

```text
  proj:                     7cefde0fb56d9f3e -> d72fede6cee1c0d9   (floor is STALE)
  is_housekeeping drops     12 -> 11 packet types
  decoded kinds             33 -> 34   (+ chunk_block_entities)
  corpus                    92 -> 93 checks   (+787, the control)
  census block_entity_type  1/49 -> 3/49      one recording scored twice:
                            block_entity_data alone {sign}; with the chunk arm
                            {chest, daylight_detector, sign}. BOTH new types
                            came through the new channel.
  corpus verdict            77/93 declared, 13 diverge, 3 absent,
                            0 vacuous/mute/inert/error, NOTHING regressed
  787                       enters the baseline at `absent`
  787 stability             1 distinct signature over 6 cold vanilla boots
```

### The four things a reader should take from this branch

**1. The brief's premise about contents is refuted by the jar.** It was
written as "a chest's contents and a hopper's state become observable".
`BlockEntityInfo.create` stores `getUpdateTag(provider).isEmpty() ? null :
tag`; `BlockEntity.getUpdateTag` is `new CompoundTag(); areturn`; only
**seventeen** classes in the pinned jar override it and **chest, hopper,
barrel, lectern and furnace are not among them**. Confirmed on the wire, not
just read: the one chest this branch got vanilla to put in a chunk payload
carried tag digest `af72e84c8601b7df`, which is `trace::digest` of a single
`0x00` — a null tag. The channel reports a block entity's *existence, place
and type*. That is what the census counts and it is a real assertion; it is
not contents.

**2. The arm fired zero times until a scenario was written for it.** Measured
on five container scenarios (090, 240, 270, 470, 490) against cold vanilla: 0
observations — and, with a throwaway build that recorded *empty* chunks too,
**0 chunk packets of any kind inside any measured window.** `record::run_one`
walks to the workspace, runs setup into a discarded recording, and
`Session::settle` waits for quiet with chunk streaming deliberately not
counted as liveness. Every chunk is therefore sent before anything is
measured, and a block entity placed in setup goes into a chunk the client
already holds. An arm that cannot fire is a gate that cannot fail.

**3. Dropping empty chunks is load-bearing, not tidiness.** `787` receives
**132** chunk packets inside its window and `530` receives **176**; one of the
308 is non-empty. Recording the empties would have made two scenarios'
observation counts a function of which chunk batch landed inside a settle
timer, and both would have started flapping the day this landed.

**4. Leftovers arrive, and `near` is what stops them counting.** `787`'s
window carries **three** chunk observations, not one: the chest it placed at
`[1, 0, 0]`, the previous workspace's chest at `[33, 0, 0]` and a
daylight detector two workspaces east at `[65, 0, 0]`. Walking back re-sends
every chunk that left view, not only this scenario's. Each came as its own
packet holding exactly one block entity — workspaces are 32 blocks apart,
which is two chunks — so `compare::near` bounded the far two out at
`within = 16` and the verdict rests on the near one alone. The **census**
counts all three, because it reads the recording rather than the projection;
that is why its `daylight_detector` is honest even though no check looked
at it.

### What is deliberately dropped, and why

`consumed` is `body.len()`, so the heightmaps, the section blob and the whole
light payload reach neither a field nor the trailing `rest` digest. This is
the shape `sound` uses for its `long seed`, widened from one field to three
sections — **not** the `container_set_slot` rebase shape, because a rebase is
for a counter whose differences are behaviour and none of these is a counter.
Heightmaps and the section blob are restatements of the terrain and would put
worldgen into every gameplay scenario sent a chunk; the blob additionally
carries the palette *encoding*, which two servers can differ on while agreeing
about every block. Light is the output of an engine nothing here measures and
is computed asynchronously, so the same world produces different bytes
depending on when the chunk went out. The tag is kept as a digest because
`block_entity_data` already does exactly that with the same bytes.

### Nulls — killed with numbers, do not re-run these

* **"decode the arm and the census ceiling becomes 49."** It does not, on its
  own. A type reaches this channel only if a scenario places it *and* gets its
  chunk re-sent; the decoder removes the recorder's half of the blocker and
  leaves one scenario per type, which is the same price §25.2 quotes for the
  twenty already reachable.
* **"a chunk observation per packet, bounded by `near`."** Tried and rejected
  before it was written: 308 empty observations across two scenarios, all of
  them scheduling.
* **A hand-run vanilla boot whose `VANILLA_EXTRA_PROPS` went through a nested
  `ssh "..."`.** `\$'...'` at one level of quoting is ANSI-C quoting on the
  remote and at two levels is a literal `$`, so `difficulty=peaceful` never
  reached `server.properties` — and superflat is famously full of slimes.
  Six cold-boot runs came back with the scenario recording nothing at all and
  `oracle was slain by Slime` in the server log, which is a measurement that
  cleared its candidate for a reason having nothing to do with the candidate.
  Put the loop in a file and `scp` it; do not nest the quoting. The substrate
  paragraph in `ci/t2-gameplay.sh` says peaceful is substrate and it is right.
* **Reading `trace::digest` as FNV-1a.** The multiplier on that line is
  `0x1000_0000_01b3`, not the FNV prime `0x100000001b3`. Reproducing a tag
  digest with the real prime gives an answer agreeing in the bottom five bytes
  and differing in the top three, which reads like a decoder bug. The doc
  comment on `digest` already says this; it cost half an hour anyway.

### The single next item, and the result that closes it

**One scenario per block entity type that matters, on the `787` pattern.**
`787` buys `minecraft:chest`. The other 28 are one file each — place the
block, walk 100 blocks on +z, walk back — and each moves
`ci/vanilla-reach-census.py`'s `block_entity_type` numerator by one. The four
game-master types (`structure_block`, `jigsaw`, `test_block`,
`test_instance_block`) still need the `ops.json` §25.2 names, and `mob_spawner`,
`vault`, `trial_spawner` and `end_gateway` carry per-run state in their update
tag and belong in `ci/checks/gameplay-unstable.toml` or nowhere.

**And the floor is stale and is not this branch's to re-stamp.**
`ci/checks/gameplay-floor.toml` carries 4/92 at `proj:7cefde0fb56d9f3e`; the
projection is now `proj:d72fede6cee1c0d9` and the corpus is 93 checks — the
gate printed **both** `STALE PROJECTION` and `STALE DENOMINATOR`. Never
run `ci/corpus-floor.sh --update` from here — re-measuring is the queued item
after this one, so the corpus is stamped once rather than twice.
