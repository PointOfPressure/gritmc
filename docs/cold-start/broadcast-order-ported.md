# The per-tick broadcast order, ported

Branch `bcast-order`, 2026-08-19, based on `entid-alloc-order` (`65ecaeeb0`).
Roadmap §103. **Read `docs/cold-start/entity-id-and-order-keyed.md` first** —
this is the port of what that lane derived, and it does not repeat the
derivation.

Read this before touching the entity list, the tracker, the entity tick, or
anything that iterates entities.

## THE NUMBER

```text
id sets whose walk differs from vanilla's broadcast order
    before   21 of 41
    after     0 of 41
```

And the half that is not a caveat: **nothing observable moved.** 0 of 108
gameplay checks can grade a per-tick entity order, before or after. §101.6 and
§101.6.1 established that from both directions and neither number changed.

## WHAT IS PORTED, AND WHERE IT LIVES

`crates/rustmc-server/src/openhash.rs` — `IntOpenHashMap<V>`, the walk order of
`it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap`, transcribed from the
bytecode of the fastutil the pinned 26.2 jar ships against.

`crates/rustmc-server/src/entities.rs` — `Entities` holds `by_id`
(`IntOpenHashMap`, = `ChunkMap.entityMap`) and `tick_order` (`Vec<i32>`, =
`ServerLevel.entityTickList.active`) under one lock.

`crates/rustmc-server/src/tracker.rs` — `Tracker.tracked` is an
`IntOpenHashMap<TrackedEntity>`, which is the same `ChunkMap.entityMap` holding
the other half of the record. `ChunkMap.tick` walks that map **twice** and both
of ours now do too.

## THE TEN INPUTS — do not re-read the jar for these

`ci/fastutil-open-hash-walk.py` reads each from the bytecode and pins it.
`openhash.rs` names each at the code that reproduces it.

| # | pin key | the fact |
|---|---|---|
| 1 | `1_mix` | `h = k * 0x9E3779B9; return h ^ (h >>> 16)` |
| 2 | `2_next_power_of_two` | `1L << (64 - Long.numberOfLeadingZeros(x - 1))` |
| 3 | `3_array_size` | `max(2, nextPowerOfTwo(ceil(expected / f)))`, ceiling 2^30 |
| 4 | `4_max_fill` | `min(ceil(n * f), n - 1)` |
| 5 | `5_default_ctor` | `(16, 0.75f)`, so `n = 32`, `mask = 31`, `maxFill = 24`, `minN = 32` |
| 6 | `6_probe` | first slot `mix(k) & mask`, then linear `+1 & mask`; key 0 lives at slot `n` |
| 7 | `7_growth` | `if (size++ >= maxFill) rehash(arraySize(size + 1, f))` |
| 8 | `8_shrink` | `if (n > minN && size < maxFill / 4 && n > 16) rehash(n / 2)` |
| 9 | `9_shift_keys` | Knuth algorithm R, wrap-aware, no tombstones |
| 10 | `10_iterator` | `pos = n`; zero key first from slot `n`; then `--pos` **descending** |

Five of these are things a plausible reimplementation gets wrong:

1. **`>>>` is Java's logical shift.** An arithmetic shift agrees for every key
   whose product is non-negative and disagrees for about half of all keys.
   Right for {1,2}, wrong somewhere past it.
2. **`size++` is read on two different sides of the increment.** The growth
   *test* sees the size before, the *target* sees it after. Getting both from
   one side moves the table size at exactly one boundary, which changes the
   walk only for maps of a particular size.
3. **`arraySize(16, 0.75)` is 32, not 16.** `ceil(16/0.75) = 22`, and the next
   power of two is 32. A map that starts at 16 walks a different order for
   every key set from the first one.
4. **The iterator goes DOWN.** Nothing else in the algorithm implies it, and it
   is the reason {1,2} walks `2 1`: 2 takes a higher slot than 1 under `mix`,
   and higher slots come out first.
5. **`shiftKeys`' break condition is not symmetric.** Written from the source's
   one-line ternary it reads as though it were; the `last <= pos` arm is an
   `or` and the `last > pos` arm is an `and`. Transcribe it from the two
   bytecode branches.

## THE CORRECTION TO §101.5 — the walk is NOT a pure function of the key set

It is a pure function of the key set **at a fixed capacity**, and the capacity
is a function of the whole insert-and-remove history. Input 8 is why:
`removeEntry` halves the table when a map that has grown past its constructed
size empties out.

Every set §101 measured agrees with the stronger claim, because every one of
them was insert-only. So the golden here is not key sets:

```text
ci/probes/WalkHistory.java     takes `+k` / `-k` histories
ci/checks/fastutil-walk-history-golden.txt
    948 histories, five families
    870 walking a real permutation
    460 containing a remove
     64 growing past the first table boundary and draining back
```

`openhash::agrees_with_the_measured_vanilla_walks` replays all 948 and requires
the identical walk. It also asserts the corpus's own floors, so it cannot be
made vacuous by handing it a shorter file.

What survives of §101.5's sentence is the weaker true one, and it is what makes
{5,4,3,2,1} and {1,2,3,4,5} one row: **for a fixed table size, linear probing
puts the same keys in the same slots whatever order they arrived in**, and
algorithm R preserves that across removals.

## TWO CONTAINERS, BECAUSE VANILLA HAS TWO AND THEY DISAGREE

This is the trap this lane nearly walked into, and the one most likely to be
walked into again.

```text
ChunkMap.entityMap             Int2ObjectOpenHashMap        KEYHASH
ServerLevel.entityTickList     Int2ObjectLinkedOpenHashMap  INSERTION
```

Same entities, two orders. While `Entities` was a single `BTreeMap`, the entity
tick walked ascending id — and **for a monotonic allocator inserting in
allocation order, ascending id IS insertion order**. So the tick agreed with
vanilla, by coincidence.

Porting the broadcast walk into that one container would have moved the tick to
hash order: a divergence closed and another opened, with nothing to say so,
because `update_swing_time` touches one `i32` per entity and no ordering of it
is observable today.

`tick_order` is a `Vec<i32>` rather than a second ported map. A linked open hash
map's walk *is* insertion order, a re-put of a live key does not move it, and a
remove takes it out of the chain — all three of which a `Vec` with a
push-if-absent and a `retain` reproduce exactly.

`the_tick_order_and_the_broadcast_order_are_not_the_same_order` pins both on one
entity set: {1,2,3,4,5} ticks `1 2 3 4 5` and broadcasts `2 4 1 3 5`.

## THE CORRECTION TO §101.5 (2) — PlayerMap reaches chunk tracking

`ChunkMap.tick`'s call sequence, derived from its bytecode and pinned under
`ChunkMap.tick call sequence`:

```text
PlayerMap.getAllPlayers -> updateChunkTracking -> ServerLevel.players
  -> Int2ObjectMap.values -> TrackedEntity.updatePlayers
  -> ServerEntity.sendChanges -> Int2ObjectMap.values
  -> TrackedEntity.updatePlayers
```

So `getAllPlayers` — the `Object2BooleanOpenHashMap<ServerPlayer>` keyed on the
entity ids, because `Entity.hashCode()` returns `getId()` — feeds
`updateChunkTracking` and nothing else. The list `updatePlayers` walks is
`ServerLevel.players()`, a plain `java.util.List` field: INSERTION order, which
ours already is because `Players::connections` is a `Vec` in join order. The
order one entity's packets reach several players is `TrackedEntity.seenBy`, a
`Sets.newIdentityHashSet`, which §101.4 established is unreproducible.

**§101.5 concluded one fix closes two of the four wire order classes. It closes
one.** The second KEYHASH site is real; its consumer is one this server does not
have. Our only per-tick player walk builds a set of wanted chunks and is
order-blind, so porting it today would be a check over a function nothing calls.

## THE ASSUMPTION THE WALK RESTS ON, AND ITS TRIPWIRE

`ChunkMap.entityMap` holds only **tracked** entities and `ChunkMap.tick` walks
that. Ours walks `Entities::all`, which is every **live** entity, and skips the
ones the tracker has no record of.

Those are the same walk exactly while the two key sets are the same set. If one
diverges the result is not "close": an open hash map's walk is a function of the
whole key set, so **one untracked entity in the live map moves the slots of the
tracked ones** and reorders packets that have nothing to do with it.

One thing can make them differ. `Tracker::add` returns without recording
anything when `clientTrackingRange() * 16 == 0` — vanilla's own test in
`ChunkMap.addEntity`. No `EntityKind` here has a zero range, so today the sets
coincide.

`the_broadcast_walk_assumes_every_live_entity_is_tracked` is the tripwire, over
`entities::KINDS` rather than over a call-site literal so it cannot pass by not
looking. **Whoever adds a zero-range kind has to move `Tracker::tick`'s main
loop onto `self.tracked`, which is the real `entityMap`.** The failure it guards
is silent: the untracked entity would simply never appear, everything else would
still be broadcast, and only the order would be wrong.

## AND ChunkMap.tick WALKS THAT MAP TWICE

The pinned call sequence shows both: `entityMap.values -> updatePlayers ->
sendChanges` is the broadcast, and the second `entityMap.values ->
updatePlayers` is the `movedPlayers` sweep. Ours splits that one vanilla map
across `Entities.by_id` (the entity records) and `Tracker.tracked` (the
`TrackedEntity` records), so the sweep walks the second container. Both are
ported, and `ci/entity-broadcast-order.py` reads both — a sweep left on an
ordered map would be a divergence the check could not otherwise see, because it
grades the ids `Entities::all` returns and the sweep emits from somewhere else.

## THE CHECK RUNS OUR WALK NOW, AND WHY IT HAD TO

`ci/entity-broadcast-order.py` computed our column as `sorted(set(ids))` and
justified it from a source read — "a BTreeMap walks ascending, which is the
container's contract, not an observation about this tree". True while the
container was the language's.

The container is now ours. "It walks fastutil's order" is a claim about code in
this repository, and a check that reads our source and then states our walk
**asserts the thing it exists to grade**.

So it runs it: `entities::tests::dump_broadcast_walks` builds a real `Entities`
for each pinned id set and prints what `all()` returns, and the check computes
the divergence itself.

> **The dump asserts nothing, and must never start.** A wrong walk has to reach
> the check as a RED. If the dump asserted, a wrong walk would fail the test,
> the check would get an empty stdout, and it would REFUSE — which is a
> different verdict, and the one arm 5 of the falsifier depends on not getting.

## BOTH RATCHETS, AND WHY ONE TURNED ROUND

`ci/order-blind.py --check` refused the day a check became CAN-GRADE-ORDER
while the divergence was open. The divergence is closed, so that condition can
never fire again. **An arm that cannot fire is a comment.**

The rot did not go away, it changed direction. With 0 of 108 checks able to see
this order, a walk that drifts back off vanilla's reddens nothing.
`entity-broadcast-order.py --check` catches a drift against its pin, and that
pin has a `--measure` that rewrites it — the one way a closed divergence could
quietly reopen.

So `ci/checks/order-blind.toml` now carries `broadcast_order_closed = true` and
`--check` refuses if the pin ever says otherwise. Monotone, and pointed at the
escape hatch. Removing the ledger line disarms it, deliberately: that must be a
visible edit and not a side effect of running a tool.

The same treatment for `entity-broadcast-order-falsify.sh` arm 5, which
demanded a red when ours changed **to** vanilla's order so a fix could not land
unnoticed. The fix landed. Inverted rather than deleted — ours moves **away**,
same red — with a new arm 5b covering the hole the inversion opens, because a
column measured by running a dump can be wrong by the dump lying about it.

## THE INSTRUMENTS

| script | gates | falsifier arms |
|---|---|---|
| `ci/fastutil-open-hash-walk.py` | the ten inputs, the two consumers, the tick call sequence | 13 |
| `ci/entity-broadcast-order.py` | the walk, vanilla's measured against ours **run** | 10 |
| `ci/order-blind.py` | 0 of 108, and the closure floor | 9 |
| `openhash::agrees_with_the_measured_vanilla_walks` | 948 histories, in `cargo test --lib` | — |

Three refusals worth knowing by name:

* **the walk check refuses if cargo cannot run.** It does not fall back to a
  source read. A check that cannot execute our walk has nothing to say about
  it, and `sorted(ids)` in that gap is how it would come to report a divergence
  that no longer exists;
* **the input check refuses on a truncated golden**, on one with every remove
  deleted, and on one whose walks are their own keys ascending. Each is a way a
  948-row replay passes with no assertion firing;
* **the input check refuses if our port stops naming a pin key.** A parameter
  that moves in the jar must have somewhere in our source to be wrong.

And one method note, because it is the only reason any of this is trustworthy:
arm 9's first draft renamed `8_shrink` to `8_shrink_renamed`, the substring
survived the `in` test, and the check stayed green on a real mutation. It was
caught by **running** the falsifier, not by reading it.

## WHAT THIS LANE DELIBERATELY DID NOT DO

* **No scenario.** §101.8's item 1 is unchanged and is still the only thing
  that would make this a graded result rather than a replayed one.
* **No `PlayerMap` walk.** The container is ported; the consumer does not
  exist here. §103.4.
* **No change to `EntityView::entities_near`.** Still frozen, still a note.
* **No claim that anything on the corpus moved.** It did not. The within-tick
  proxy gives 62 of 89 against 53 and still explains nothing — 62 of 87 in
  §101, and the two groups three recordings added since went to neither — and
  69.7% is no more support for the fix than 71.3% was for the divergence.
