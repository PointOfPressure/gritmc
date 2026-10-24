# Entity id allocation, and the four order-keyed surfaces behind it

Branch `entid-alloc-order`, 2026-08-19, based on `mobs-tick-surface`
(`16d010f2d`). Roadmap §101. **Nothing was ported. The derivation and four
checks are the deliverable.**

Read this before touching entity ids, the entity tick, the tracker, or
anything that iterates entities.

## THE ALLOCATOR, TRANSCRIBED — do not re-read the jar for this

`Entity.ENTITY_COUNTER` **does not exist in 26.2.** Every note in this tree
that named it was wrong, including `entities.rs`'s own. The allocator is:

```java
// Entity.<init>(EntityType, Level), offsets 227-232
this.id = level.getNextEntityId();

// Level.getNextEntityId()          -- the base, for the client and any
0: iconst_0                         -- non-server level
1: ireturn

// ServerLevel.getNextEntityId()
int i = 0;
while (i == 0 || this.chunkSource.hasEntityWithId(i))
    i = ENTITY_COUNTER.incrementAndGet();
return i;

// ServerLevel: private static final AtomicInteger ENTITY_COUNTER
//              = new AtomicInteger();          -- no-arg, so it starts at 0

// Entity.getId()
if (this.id == 0) throw new IllegalStateException(
    "Tried to access entity ID before ID assignment");
```

`ServerChunkCache.hasEntityWithId` delegates to
`ChunkMap.entityMap.containsKey`.

Six consequences, each of which is a thing a porter can get wrong:

1. **the first id in a session is 1**, because the loop head tests `i == 0`
   before anything else, so the first call always increments;
2. **0 is never allocated**, and it is a tripwire rather than merely unused;
3. **the counter is static**, so it is shared by every dimension in the
   process. Ids are unique across the whole server, not per level;
4. **the liveness retry is per level.** `this.chunkSource` is the level the
   entity is being constructed in. Under a monotonic counter the loop body is
   unreachable until the counter wraps at 2^31;
5. **the draw happens in the constructor**, so an entity vanilla builds and
   then discards still burns a number;
6. **`PlayerList.respawn` is exactly such a site.** It constructs a second
   `ServerPlayer`, which draws an id, then calls
   `serverplayer1.setId(serverplayer.getId())` and abandons the drawn one.
   `BaseSpawner` has the other server-side `Entity.setId`, stamping `-1` on
   its display entity.

Three classes in the jar reference the allocator and exactly one references
the counter, so that is the whole surface.

## WHY IT IS A PARITY SUBJECT, AND HOW NARROW THE REQUIREMENT IS

`Mob.serverAiStep` re-evaluates its goal lists only when
`(tickCount + getId()) % 2 == 0`, or on the mob's first two ticks. So the goal
phase is a function of the entity id and nothing about the entity's own state
distinguishes the two phases.

**`% 2` reads one bit.** The requirement is not "allocate the same ids", it is
"hand each entity the same parity", which for a dense counter from 1 means
the same number of draws happened first. An allocator that skips a number, or
draws one for an object vanilla never constructs — or fails to draw one where
vanilla does, which is the respawn case — flips that bit for every entity
after it.

## THE FOUR ORDER CLASSES, AND THIRTEEN SITES

`ci/order-keyed-census.py` derives the concrete container from the `new` (or
the guava factory) in front of each `putfield`, never from the declared type,
because twelve of the thirteen fields are declared as interfaces. The
thirteenth, `SynchedEntityData.itemsById`, is a flat array `packDirty` walks
by index — the one case where the declaration settles the order.

| class | what the order is a function of | portable by |
|---|---|---|
| SORTED | the keys, and the keys are state | writing the comparator |
| INSERTION | the order things were added | adding them in that order |
| KEYHASH | the KEY VALUES, through the container's hash | reproducing the walk |
| IDENTITY | `Object.hashCode()` on a type that does not override it | see below |

Eleven of the thirteen reach an observer. On the wire: IDENTITY 3,
INSERTION 4, KEYHASH 2, SORTED 2.

Vanilla's two deliberate order-safeties are worth knowing, because they are
what the three IDENTITY sites are the exception to. `AttributeInstance` holds
its modifiers in an `EnumMap` and two `Object2ObjectArrayMap`s, and
`calculateValue` names the three operations itself rather than iterating the
map — **the attribute code is carefully order-safe inside the instance and
order-blind at the two sets that reach the wire.** And `set_entity_data`, the
biggest entity packet there is, is ordered by the accessor ids and nothing
else.

**`ContainerOpenersCounter` is not a site.** It is order-sensitive, but
`getEntitiesWithContainerOpen` calls `Level.getEntities(Entity, AABB,
Predicate)` and takes that list's order, so it inherits
`EntitySectionStorage`'s sorted section keys and `ClassInstanceMultiMap`'s
insertion order. Fixing those fixes it.

**`Entity.hashCode()` returns `getId()`, and `equals` is id equality.** So
every hash container keyed by an entity is keyed on the id values.
`PlayerMap.players` is the instance that matters — the player set
`ChunkMap.tick` walks for every broadcast is ordered by the players' entity
ids, which is why KEYHASH is 2 and not 1.

## THE IDENTITY-HASH CORRECTION — this project had it backwards

`ci/idhash-determinism.sh`, five cold JVMs per arm:

* the same allocation sequence produces the **same** identity hashes on every
  run. HotSpot's per-thread xorshift is seeded from a fixed constant;
* the hash is drawn from that sequence when `hashCode()` is **first called**,
  so it belongs to the draw order and not to the object. Hash the same eight
  objects in a different order and every one gets a different number.

Which gives the rule the three IDENTITY rows are read under:

> an identity-hash iteration order is run-stable exactly when the order of
> first-hash calls into that container is run-stable.

So `broadcastChangedChunks` measuring 4.24% self-agreement against
1/4! = 4.17% is **not** evidence that identity hashing is random per run. It
is evidence that which chunk is first marked dirty varies per run. The
nondeterminism is upstream and the `ReferenceOpenHashSet` only exposes it.
A container whose insertions are one thread doing the same thing every boot
has no such upstream and is stable forever — which is a worse problem, not a
better one, because it looks matchable and is not.

## THE REACHABLE ONE: BROADCAST ORDER IS A HASH OF THE IDS

`ChunkMap.entityMap` is a plain `Int2ObjectOpenHashMap`. `ChunkMap` has
exactly four map fields and the other three are
`Long2ObjectLinkedOpenHashMap`; this one alone is not. `ChunkMap.tick()`
walks its `values()` into `TrackedEntity.updatePlayers` and
`ServerEntity.sendChanges`.

So **the order every per-entity packet leaves the server in, within one tick,
is a hash of the entity id values.** That is the same dependency §94 found in
the goal phase, on a path this server reaches today: a player and a dropped
item are two entities holding ids 1 and 2.

Measured against the fastutil the pinned jar ships with — not reimplemented,
because reimplementing a hash from memory is the plausible-but-wrong step this
project does not take:

```text
ids {1,2}     vanilla  2 1              ours  1 2      DIFFERS
ids {1,2,3}   vanilla  2 1 3            ours  1 2 3    DIFFERS
ids {1,3}     vanilla  1 3              ours  1 3      agrees by luck
ids {5,4,3,2,1} inserted in that order  ->  2 4 1 3 5
```

The last line is the shape of the thing: the open map **ignores insertion
order**, while the linked map beside it — `EntityTickList` — does not, and
ticks {5,4,3,2,1} in that order. 21 of the 41 pinned id sets differ.

**This is a requirement rather than a distribution.** The walk is a pure
function of the key set, so vanilla agrees with itself about it forever. There
is no spread to argue over and no run count to quote.

## AND YET THERE IS NO CORPUS VERDICT, WHICH IS THE HONEST PART

`ci/order-selfconsistency.py`, N = 21 distinct recordings (20 when this was
written; three scenarios entered the corpus on 2026-08-19 and branch
`bcast-order` re-pinned it, with no verdict flipped — roadmap §101.6 carries
the new rows):

| site | verdict |
|---|---|
| `AttributeMap.*` on `spawn-egg-makes-a-mob` | SELF-CONSISTENT, 20 runs 1 distinct |
| `AttributeMap.*` on an iron golem, a copper golem, an armor stand | SELF-CONSISTENT, 2 runs 1 distinct each |
| `AttributeMap.*` on `walking-away-drops-a-standing-player-from-view` | NOT-SELF-CONSISTENT, 20 runs 5 distinct |
| `AttributeMap.*` on `a-chest-is-in-its-chunks-payload...` | NOT-SELF-CONSISTENT, 2 runs 2 distinct |
| `ChunkMap.entityMap` | **UNMEASURABLE** |
| `EntityTickList` + `EntityLookup` + `ClassInstanceMultiMap` | **UNMEASURABLE** |
| `ServerChunkCache.chunkHoldersToBroadcast` | NOT-SELF-CONSISTENT (4.24%, measured elsewhere) |

The obvious proxy for the broadcast order — two entities named in one socket
at one `at_ms` — gives **62 of 89 for the hash walk against 53 for ascending
id** (62 of 87 when this was written). Neither explains the data, and the reason is structural rather than a
shortage of runs: a group mixes packets emitted *during* the entity tick,
which leave in `EntityTickList` INSERTION order, with `ChunkMap.tick`'s own
broadcast, which leaves in hash order. **Reporting 71.3% as support would
have been reaching for a number that does not mean what it looks like** — and
it is 62 of 89, 69.7%, since the corpus grew, because the two groups the new
recordings added went to neither candidate order.

A mob's attribute payload being byte-identical across twenty cold boots
while a **player's** is not is worth sitting with. It is the reverse of the
usual shape on this project, where the mob is the unstable thing.

## THE AABB SECTION ORDER IS NOT THE NESTED LOOP YOU WOULD WRITE

`EntitySectionStorage.forEachAccessibleNonEmptySection` loops section **x**
ascending, and for each x walks
`sectionIds.subSet(asLong(x,0,0), asLong(x,-1,-1)+1)` — a `LongAVLTreeSet`, so
ascending **packed long**. `SectionPos.asLong` packs

```text
    x & 0x3FFFFF  << 42        bits 42..63, and bit 63 is the long's sign
    z & 0x3FFFFF  << 20        bits 20..41
    y & 0x000FFFFF << 0        bits  0..19
```

x's field carries the sign bit, so **x sorts numerically**. z's and y's do not,
so their masked fields compare as **unsigned**. Verified by sorting the packed
values:

```text
x = 0, ascending packed long, as (z, y):
  (0,0) (0,1) (0,-1) (1,0) (1,1) (1,-1) (2,0) (2,1) (2,-1)
  (-2,0) (-2,1) (-2,-1) (-1,0) (-1,1) (-1,-1)

x, ascending packed long:  -2 -1 0 1 2
```

So the walk is **x numerically ascending, then z with the non-negative
sections before the negative ones, then y the same way.** A natural
`for z in min..=max` nested loop is correct only for a box that does not
straddle z = 0 or y = 0, and silently wrong for one that does — which near
world spawn is most of them.

Ours (`crates/rustmc-entity/src/store.rs`) walks chunk x then chunk z with no
y dimension at all and then **sorts the whole result by `EntityId`**, which
discards the traversal order entirely. Two divergences, not one: the section
order above, and the within-section insertion order that an id sort also
destroys.

**It is unreachable today.** `entities_near` has no caller outside its own
crate, so nothing observes either. That is why it is written down here rather
than gated: a check over a function nothing calls is `710`.

## AND NOTHING IN THE CORPUS CAN GRADE IT, FROM THE OTHER DIRECTION

`ci/order-blind.py` files all 108 gameplay checks on two derived tests — does
`observe` name a kind `ServerEntity.sendChanges` builds, and do two entities
ever emit such a kind in one tick:

```text
BLIND-BY-PROJECTION  100
BLIND-BY-POPULATION    8
CAN-GRADE-ORDER        0
```

Same answer as the self-consistency measurement, reached from the checks
rather than from the recordings.

**It is a ratchet.** `--check` refuses the day that zero moves while the
divergence is still open, so the fix lands as the thing to do rather than as a
regression from whichever scenario happened to see it.

**Do not widen the population test.** Counting distinct entities anywhere in a
check's window reports `a-standing-player-is-rebroadcast-on-a-timer` as able
to grade the order, at three entities, and its own file says why that is
false: they are a dropped item leaking in from scenario `500` that
`compare::place` bounds out of the projection. An order exists only where two
entities emit in the same tick. Falsifier arm 5 restores the wide reading and
requires the resulting red.

## WHAT THE NEXT LANE NEEDS, IN ORDER

1. **A scenario with two entities producing packets in one tick, observed.**
   Everything above is unobservable until one exists. It is a scenario-design
   problem before it is a porting one, and it is the same item §94.8 asks for
   from the other direction.
2. **`Entities::all` walking the hash order**, once (1) can grade it. The
   golden table is already measured and pinned; what is missing is the walk,
   and it must not land unverified — flipping `ours_matches_vanilla` in
   `ci/checks/entity-broadcast-order.json` with no scenario that can see it
   buys a green over a dead path, which is §94.4's `710`.
3. **`entities_near`'s two orders**, if the frozen contract is ever reopened:
   sorted on the packed section key, major; insertion within a section, minor.
   Neither half is by id.
4. **Burn an id in respawn**, whenever respawn is written. There is nothing to
   do today and it is a one-line requirement that is invisible until a mob
   exists to have its goal phase flipped by it.

## THE INSTRUMENTS, AND WHAT EACH ONE REFUSES

| script | gates | falsifier arms |
|---|---|---|
| `ci/entity-id-allocator.py` | vanilla's allocator against ours, 12 properties, 4 required | 8 |
| `ci/order-keyed-census.py` | 13 sites, container derived from the bytecode | 7 |
| `ci/entity-broadcast-order.py` | the walk, against a golden measured from the shipped fastutil | 9 |
| `ci/order-selfconsistency.py` | is vanilla self-consistent, per order, N = 20 | 7 |
| `ci/order-blind.py` | the ratchet: 0 of 108 checks can grade a per-tick order | 7 |

38 arms, counted by running them. Every falsifier carries at least two
negative controls, because a detector that refuses on any input at all passes
every positive arm for free.

Three refusals are worth knowing by name:

* **the allocator check refuses if a mob `EntityKind` lands.** It declines to
  grade `Mob.serverAiStep`'s goal phase on the ground that no mob can exist
  here; that refusal is honest only while it is true, and it fires before
  anything is printed so a stale refusal cannot be pinned either;
* **the broadcast check re-measures its golden table from a cold probe run**
  and requires it to agree figure for figure. A cached table is how a dead
  probe keeps answering;
* **the self-consistency check refuses at N = 1**, and at two identical files
  that de-duplicate to N = 1. One run always agrees with itself, and reporting
  that as SELF-CONSISTENT is the most seductive wrong answer available here.

`ci/idhash-determinism.sh` is deliberately outside T0: it compiles and runs
five cold JVMs and its answer is a property of HotSpot, not of this tree.

## WHAT THIS LANE DELIBERATELY DID NOT DO

* **No reimplementation of fastutil's hash.** The golden table is measured
  from the shipped jar and pinned with its sha256. A hash written from memory
  that is right for {1,2} and wrong for {1,2,7} is worse than no hash.
* **No change to `EntityView::entities_near`.** It is frozen; the divergence
  is a note.
* **No claim that we are red on the corpus.** We are not gradeable on it.
