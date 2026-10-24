# Is the unported surface extractable, or must it be hand-written?

*Lane `extract-classify`, 2026-08-19, against `main` at e2fc07600 (2,324
commits). Roadmap claim 105. Everything below is measured on this box today from
the pinned 26.2 jar; nothing is recalled and nothing is read from the shared
`~/rustmc/data/vanilla/`.*

---

## 0. The answer, first

**No. Three years is wrong, and it is wrong in a way that is fixable rather than
merely optimistic.**

`docs/ASSESSMENT-2026-08-19.md` measured velocity honestly — one entity type a
week, zero command roots a fortnight — and extrapolated. The extrapolation is
where it breaks, because it prices every remaining surface at the rate of the
surface that was measured. The measured surface was ported by hand. Some of what
remains cannot be ported by hand at a sane cost, and **does not have to be.**

The single number that separates the two:

> **leverage = corpus size / vocabulary size** — how many items of a surface one
> unit of implementation work buys.

| surface | corpus | vocabulary | **leverage** | bucket |
|---|---|---|---|---|
| loot tables | 1,355 tables | 42 opcodes | **32.3** | EXTRACTABLE — *built, measured* |
| command tree shape | 2,085 nodes, 90 roots | 55 argument parsers | **37.9** | EXTRACTABLE — *built, measured* |
| `update_shape` | 79,111 refused of 103,318 pairs | 3-position extract | **large** | EXTRACTABLE (already known) |
| serverbound packet **decode** | 138 wire fields | 46 field types | **3.0** | TRANSCRIBABLE, mild leverage |
| command executors | 1,281 executors | 1,281 | **1.0** | TRANSCRIBABLE, mostly blocked |
| block entities | 38 NBT schemas | 38 classes, 8 sharing a container base | **1.0** | TRANSCRIBABLE |
| serverbound packet **handling** | 58 packets | 58 handlers | **1.0** | TRANSCRIBABLE |
| entity AI, goal-based | 46 mobs, 2,179 methods | 53 goal classes | **0.87** | TRANSCRIBABLE |
| entity AI, brain-based | 21 mob classes | 119 behavior classes | **0.18** | TRANSCRIBABLE → IRREDUCIBLE |

**Leverage above ~10 means the surface is a table and the cost stops growing
with it. Leverage near 1 means the vocabulary is the corpus, and the only way
through is to write it.** That is the whole classification, and it is
computable rather than argued.

The assessment's three years is what you get from applying leverage-1 rates to
leverage-32 work. It is right about the hand-written surfaces and wrong about the
tabular ones, and the tabular ones are large.

---

## 1. The loot prototype, and what it actually cost

Loot was the strongest available test: the largest untouched surface (**1,355 of
1,355 unported, 0 run**) and datapack JSON rather than bytecode.

### 1.1 The result

```
cases        18003       every family vanilla could evaluate
MATCH        17943       (99.67%)
MISMATCH         0
REFUSED         60       an opcode or a context we do not have
ORACLE-BLIND    12       a nested reference the harness cannot resolve; all 12 diverged
tables fully reproduced  1162 of 1174

   1109 of 1111  blocks        40 of 48  chests       4 of 5  spawners
      3 of    3  dispensers     2 of  2  gameplay     2 of 3  harvest
      1 of    1  carve          1 of  1  pots
```

**Re-taken 2026-10-01 (branch `loot`, Threadripper) with the oracle loading the
whole datapack through `WorldLoader`:** 18,055 cases, 17,993 MATCH, 0 MISMATCH,
62 REFUSED, **0 ORACLE-BLIND**, 1,163 of 1,181 tables fully reproduced
(`cargo test --release -p rustmc-loot --test golden_loot -- --nocapture`). The
twelve nested-reference cases that "all diverged" above were the old harness
rolling an unresolvable `loot_table` entry as empty; resolved, every one matches
the interpreter.

`crates/rustmc-loot` is roughly 550 lines. It knows nothing about any particular
loot table — it knows what 23 opcodes mean. Every drop it produces is checked
against a vector vanilla produced from its own table.

**The opcode set was derived from the block corpus and generalised to the others
without a single wrong answer.** That was the real test of the vocabulary claim
and it was not guaranteed: chests are a different corpus with different pools,
weights and multi-entry draws, and 40 of 48 of them reproduce exactly. The
remaining refusals name themselves — `set_potion` (11), `set_damage` (6),
`enchant_randomly` (3), `set_stew_effect` (3), `set_enchantments` (1) — and all
five are component-level functions that change neither the item id nor the count
the gate compares. **Implementing them as no-ops would take the coverage to
~100% and would be a coverage claim this has not earned**; closing them honestly
means teaching the golden to dump components, which is a day's work and is not
done.

### 1.2 The measured rate, which is the number the owner asked for

Branch `extract-classify` was created at **18:33**. The commits:

```
18:40  the census instrument and its falsifier
18:46  the golden dumper: vanilla rolls its own 1,355 tables
18:55  the interpreter: 1,109 of 1,111 block tables exact
18:57  the interpreter's falsifier: five faults, four turn it red
```

**Twenty-four minutes, one lane, one context, cold start to a gated port of the
largest unported surface in the project.** Call it half a lane-day with the
write-up and the review it deserves.

The comparison that matters is not to another port. It is to the same surface
priced the other way: 1,355 tables at the assessment's hand-written rate is
somewhere between one and three lane-years. **The ratio is about three orders of
magnitude, and that is the finding.**

Two honest deductions from that number. It is a *prototype* rate, on the surface
chosen because it was the most favourable. And it delivers 1,109 tables that
**nothing calls yet** — see §1.4.

### 1.3 Why the vocabulary is small

`ci/loot-census.py`, reading the jar and nothing else:

```
loot tables  1355
opcodes        42
```

829 of the 1,355 tables use **exactly three** opcodes. Three opcodes run 820
tables; nine run 1,144; forty-two run all 1,355. Twenty-three cover every block
table that can be evaluated without a live server.

The trap that hides this: the pinned `server.jar` is a **bundler**.
`unzip -l server.jar | grep loot_table` returns **zero hits**, and zero looks
like an answer. The datapack is inside `META-INF/versions/26.2/server-26.2.jar`.

### 1.4 What this extraction cannot see — read before quoting §1.1

The `use_item_on` win came with a warning that generalises: `result` was a gate
column the bytecode walk could never derive, because it mapped *calls* to fields
and a differing return value makes no call. **Extraction that misses a dimension
is worse than none, because it looks like coverage.** For loot:

1. **What an opcode means.** `explosion_decay` is one token in JSON and a method
   in bytecode. The 42 opcodes are the *names* of the work. **The opcode bodies
   are TRANSCRIBABLE, not extractable** — 23 of them cost about a day, which is
   the honest reading of §1.2.
2. **Draw order.** The JSON says a pool has `rolls` and an entry has a
   `set_count`. It never says which is drawn first. Three facts from `javap -c`
   decide every multi-entry table and none is visible in any table:
   * `addRandomItem` draws `nextInt(total)` **only when the pool expanded to more
     than one live entry** — 1,043 of 1,111 tables therefore consume no
     randomness at all;
   * `bonusRolls.getFloat` is evaluated **before** the luck multiply;
   * a short-circuiting condition does not draw.
   An implementation can get all 42 opcodes individually right and be wrong on
   every multi-entry table. This is loot's `result` column, and the reason the
   gate is against vanilla's vectors rather than against a reading of the JSON.
3. **Whether a table is ever invoked.** A loot table is inert until a call site
   reaches it. **Half of this is now closed and half is not.** `Loot::block_drops`
   is the dispatch, driven by the extracted mapping, and over all 1,196 blocks:

   ```
   dropped something        1015
   rolled, dropped nothing   155     leaves bare-handed, and so on
   no table by design         22     vanilla's noLootTable
   refused                     4     2 entity_properties, 2 location_check
   unknown block               0
   table missing               0
   ```

   It returns `Err` rather than an empty vector whenever the answer is not known,
   and `NoDrops` keeps `NoTableByDesign` apart from `UnknownBlock`,
   `MissingTable` and `Refused` — collapsing those into one empty result is the
   likeliest way a loot port reads as working while silently deleting every drop
   it does not understand.

   **What remains is one call from `rustmc-server`'s block-breaking path, and
   entity death and chest fill, which need the entity and structure subsystems.**
   That is the honest remaining cost and it is deliberately not claimed here.
4. ~~**Which block maps to which table.**~~ **Closed, and it needed a second
   extraction rather than a guess.** The mapping is a field on `Block`, set with
   `dropsLike(...)` or suppressed with `noLootTable()`, and it leaves no trace in
   any JSON — but `BlockBehaviour.getLootTable()` is public and answers it
   directly. `ci/loot-golden.sh` now emits `data/vanilla/block_loot_map.json`:

   ```
   1,196 blocks
   1,113 follow `minecraft:blocks/<name>` exactly
      61 point at ANOTHER block's table
      22 have no table at all
   1,113 distinct tables referenced — the whole block corpus, exactly
   ```

   The 83-block gap this document previously could not explain is 61 + 22.
   Assuming the naming convention would have given 61 blocks the wrong drops and
   22 blocks drops they should not have, **and every one would have looked like a
   correct port**, because the table it rolled really exists and really produces a
   plausible item. This is the same failure mode as the amethyst cluster in §1.5,
   found the same way: by refusing to accept a convention as evidence.
5. **Anything past the item id and the count.** The gate compares those two
   fields. `copy_components` and `copy_state` are documented no-ops in the
   interpreter — not a claim they are done — and the five refusing
   component-level functions above are refused for the same reason.

6. **The oracle's own blind spots, of which there are now two.** Both were found
   the same way, by an independently-derived implementation disagreeing with the
   vectors and the disagreement being investigated rather than conceded:
   * item tags are not bound in a bare harness, so a tag-valued `match_tool`
     silently answered false (§1.5);
   * the LOOT_TABLE registry is not populated either, so a nested `loot_table`
     entry silently contributes nothing. Twelve cases are excluded as
     ORACLE-BLIND, **and the test requires at least one of them to have actually
     diverged** — all twelve did. An exclusion that never fires is a way of
     hiding disagreements, so it is itself gated.

### 1.5 The gate caught a fault in its own oracle, and that is the part to keep

The dumper reported `blocks/amethyst_cluster` dropping **2** shards to an iron
pickaxe. The interpreter said **4**. The interpreter was right.

Item tags arrive with the datapack at reload, so in a bare harness
`#minecraft:cluster_max_harvestables` resolved to an empty holder set and
`ItemPredicate.test` returned false **without complaining**. The vector file was
completely plausible and completely a harness artefact.

**Had the port been written to match the golden rather than argued against it,
that bug would have been transcribed into the port and gated there forever.**
Tags are now bound with `prepareTagReload().apply()` — `bindTags` throws on a
frozen registry. This is the strongest argument in this document for keeping an
independently-derived implementation next to an oracle rather than fitting one to
the other.

### 1.6 The falsifiers, and the third verdict the data forced

`ci/loot-census-falsify.py`: five cases, each of which must pass on the real
walk **and fail on a deliberately broken one**. It caught a vacuous case of mine
during development — a monotonicity check that passed on both, because `curve()`
is monotone by construction.

`crates/rustmc-loot/tests/falsify_loot.rs`: five plausible mistakes injected into
the interpreter, each of which must turn the 17,744-case gate red.

```
RED            146  draws the weighted choice for a single-entry pool
UNREACHABLE      0  skips bonus_rolls when luck is zero
RED            101  filters zero-count stacks instead of emitting air/0
RED              4  rolls survives_explosion with no explosion in context
RED              4  treats item tags as matching nothing
```

The fifth forced a third verdict. `bonusRolls.getFloat` really is called before
the luck multiply, so skipping it really would desynchronise the stream — and
**zero of the 1,355 shipped tables declare `bonus_rolls` at all**. Calling that
VACUOUS would blame the gate for the corpus; calling it RED would be a lie. It
reports UNREACHABLE, reachability is recomputed from the corpus every run, and
if a datapack ever ships such a pool the case flips to VACUOUS and starts
failing.

---

## 2. The other surfaces, classified with the same measurement

### 2.1 Commands — EXTRACTABLE in shape, blocked in substance

The assessment's worst row was command roots at **180 weeks**, from zero new
roots in a fortnight. `ci/command-tree-golden.sh` measures what that prices.

Vanilla's entire dispatcher builds with no world and no server
(`CommandBuildContext.simple(lookup, FeatureFlags.VANILLA_SET)`), and Brigadier's
tree is walkable:

```
90 roots   2,084 nodes   1,084 argument nodes   112 redirects
55 distinct argument parsers   1,281 executable nodes
```

* **The shape is extractable**: one dumper, already written. That is the whole
  declare-commands tree, which is what makes a client autocomplete and what stops
  an unknown command being answered with silence.
* **The parsers are transcribable and bounded**: 55, and the top five
  (`Integer`, `Entity`, `NbtPath`, `Bool`, `BlockPos`) are 598 of the 1,084
  argument nodes.
* **The 1,281 executors must be written.** Nothing here can see inside a lambda;
  `executable: true` says an executor exists, not what it does. **A tree with
  every node placed and no executors autocompletes perfectly and does nothing** —
  the `result`-column mistake in a new costume.

**And the 180-week figure prices the wrong bottleneck.** Classifying each root by
the argument types in its subtree:

```
36 of 90 roots take an EntityArgument somewhere      this server has 2 entity types, 0 mobs
12 need registry lookups     5 players    4 blocks    4 nbt    4 scoreboard
38 of 90 use no subsystem-bearing argument anywhere  <- unblocked today
```

Zero commands landed in a fortnight not because commands are slow but because
**commands are downstream**.

**RE-MEASURED 2026-09-05, and only two of the three numbers reproduce.** The
36 and the registry groups reproduce exactly — 5 players, 4 blocks, 4 nbt, 4
scoreboard, over the same subtrees. The 38 does not. Sweeping each root's
subtree for any parser that names a subsystem this server does not have gives
**35**, and **30** once redirects are followed: `tp`, `tm`, `w` and `xp` are
pure redirects into entity-bearing subtrees and are free only if the walk stops
at the redirect edge. Which of those two is the right question depends on
whether a redirect root can be registered without its target, and it cannot —
`FLAG_REDIRECT` carries a node index. **So the honest count is 30, and this
section's 38 was the optimistic reading.** The method and the exact command are
in `docs/cold-start/stack-extract.md`.

Three of the roots this section named as unblocked are not: `tell` is a
redirect into `msg`, which takes an `EntityArgument`, and so are `tm` and `w`.
The list that survives is `gamerule`, `difficulty`, `weather`, `worldborder`,
`seed`, `say`, `reload`, `save-all`, `setworldspawn`, `defaultgamemode`,
`forceload`, `tick`, `banlist`, `ban-ip`, `pardon-ip`, `datapack`, `help`,
`jfr`, `list`, `me`, `perf`, `random`, `return`, `save-off`, `save-on`,
`setidletimeout`, `stop`, `stopwatch`, `teammsg`, `version`.

**And the tree half of this cost is now spent.** `crates/rustmc-server/src/commandtree.rs`
carries argument nodes, redirects and custom suggestions, and
`crates/rustmc-server/tests/command_tree_vanilla.rs` reproduces vanilla's own
`ClientboundCommandsPacket` for the **whole** 2,085-node dispatcher, byte for
byte, at all five permission levels. What that gate does not touch is the
executors, which is the bullet above and remains the real cost.

**Cost: the tree is done. The 30 unblocked roots are ~1 lane-week of executors
and replies, and the first of them (`/gamerule`, 233 nodes) is landed. The
remaining 60 roots are not command work at all — they are the entity,
scoreboard and NBT subsystems, and they should never be scheduled as a command
lane.**

### 2.1.1 What the `commands` branch measured against that, 2026-09-05

Ten roots ported, and the count is the least interesting number in this
paragraph. Three things this section got wrong:

**"The 1,281 executors must be written" prices the wrong unit twice over.**
The first correction is that a root does not need its executor written from
nothing: `/list`, `/seed`, `/gamerule`, `/version`, `/say`, `/me`, `/msg`,
`/tell`, `/w` and `/help` are ten roots and four distinct *reply mechanisms* —
a `system_chat` component, a `disguised_chat` broadcast, a ten-packet dump and
a usage walk. The second is larger and is below.

**The `arg`/`redirect`/`literals` split stopped being a gate and this document
did not say so.** §2.1 was written when `crate::commandtree` refused argument
nodes and redirects by type; both landed on `stack-extract`, and `/msg`,
`/tell` and `/w` are an `arg` root and two `redirect` roots this server now
ships with byte-exact trees at all five permission levels.
`docs/generated/command-surface.tsv`'s own header carried the same stale claim
and is corrected in `ci/command-surface.py`.

**The highest-leverage item on this surface is not a root at all.** It is
`Commands.finishParsing`'s two `system_chat` messages — the answer for every
typo, every root that is not ported, and every command a player lacks
permission for. One reply shape, gated over 80 rows at five permission levels,
72 rendered byte-exact and 0 unrendered. Before it, every one of those was
answered with silence, which on the wire is indistinguishable from a command
that ran and printed nothing. A surface census that counts executors cannot
see an item like that, and this section's method is a census.

What the branch adds as instruments: `ci/command-parse-golden.sh` (2,889 rows
of `(parser, input) -> (accepted, cursor)` from vanilla's own
`ArgumentType.parse`, with a ratcheted count of what this server cannot read),
and the `failures` block in `ci/commands-golden.sh` (vanilla's own
`finishParsing` output through a recording `CommandSource`). Both are in T0.

### 2.2 Entity types — TRANSCRIBABLE, and the leverage says so

Same measurement, opposite answer. Of 158 entity types, 55 classes declare
`registerGoals` and 46 build at least one AI goal:

```
46 mobs built from 53 distinct goal classes      leverage 0.87
median 6 goals per mob      2,179 declared methods across the 55 classes
```

The coverage curve is flat where loot's was a cliff: 12 goal classes cover 41% of
goal-using mobs, and reaching 100% needs 53 — **nearly one new goal class per
mob**. There is no dispatch that buys 32 mobs at a time, because there is no
shared vocabulary to dispatch on.

The other ~68 mobs do not use goals at all. They use the brain/behavior system:
**119 behavior classes, 21 classes declaring `makeBrain`** — leverage 0.18, worse
still.

What *is* extractable on entities is the registry data — attributes, dimensions,
categories, spawn eggs, sound events — and `AttributesGolden.java` already does
part of it. That is real and worth taking, and it is not the behaviour.

**Cost: ~2 lane-weeks for the extractable registry half. The behaviour half is
53 goal classes + 46 wirings + 119 behavior classes, and at the observed rate
that is the multi-month item the assessment correctly identified.** Its estimate
of one entity type a week is not pessimistic for this surface. It is accurate.

### 2.3 `update_shape` — EXTRACTABLE, and already half-built

**RE-MEASURED 2026-09-05, and the headline figure was wrong.** This section
said "48,405 pairs of real debt" and marked itself "not re-measured by this
lane". Read off the extract this tree actually generates
(`data/vanilla/update_shape.json`, `ci/shape-golden.sh`):

```
103,318 sensitive pairs      a (state, direction, neighbour) triple whose answer moves
 12,716 answered             the table, one row each
 11,491 connective           counted and refused, no single answer to give
 79,111 refused              over ELEVEN reasons
```

The dominant reason is **48,993** pairs, not 48,405 — `updateShape` reading the
block **above** rather than its own neighbour. `docs/SERVER.md` and
`docs/WORKLIST.md` already carried 48,993; this section was the stale copy, and
it is the one an incoming lane reads first.

The refusal census, which is the actual specification of the remaining work:

| pairs | blocks | `updateShape` reads |
|---|---|---|
| 48,993 | 81 | `0,1,0` — the block above |
| 6,976 | 98 | `0,0,-1` |
| 6,629 | 122 | `0,-1,0` |
| 5,412 | 79 | `1,0,0` |
| 5,386 | 78 | `0,0,1` |
| 5,381 | 78 | `-1,0,0` |
| 212 | 8 | `BlockAndLightGetter.getLightEngine` |
| 96 | 1 | `LevelReader.environmentAttributes` |
| 16 | 1 | `0,-1,-1` |
| 8 | 2 | `LevelReader.registryAccess` |
| 2 | 1 | `0,-2,0` |

**258 distinct blocks**, and that number is new for a reason worth carrying:
`ShapeGolden` capped each reason's block list at eight, so six of the eleven
rows above used to read "8 blocks" and the union used to read 37. The cap made
every block figure derived from that file a truncation, by 7x. Pair counts were
never affected. Fixed in the same commit as this paragraph; `blockCount` is now
emitted beside the list.

So the shape of the remaining work is sharper than "one lane-week": **the six
positional rows are 78,777 of the 79,111, they are one mechanism, and closing
them is a second sweep position in `ShapeGolden` rather than 258 hand-written
arms.** The other five rows are 334 pairs over 13 blocks and are genuinely
per-block. **Cost: unchanged at ~1 lane-week, now with a spec.**

### 2.4 Block entities — TRANSCRIBABLE, leverage 1.0

Of the block-entity classes in the jar, **38 declare their own
`loadAdditional`/`saveAdditional`**, which is the definition of having an NBT
schema that must be written. The shared vocabulary is thin:

```
28 of 38 extend BlockEntity directly            no shared behaviour to dispatch on
 8 of 38 share a container base                 RandomizableContainerBlockEntity / BaseContainerBlockEntity
```

There is a real container cluster worth taking as one piece of work, and after
that every remaining block entity is its own schema and its own tick. **Leverage
1.0. Cost: proportional to 38, minus one shared container implementation.**

### 2.5 Serverbound packets — the metric retrodicts the project's own history

58 serverbound game packet classes, **138 wire fields, 46 distinct field types**.
The two halves separate cleanly and they have different leverage:

* **decode** — 138 fields over 46 types is **leverage 3.0**, and the top six
  types (`int`, `boolean`, `BlockPos`, `String`, `float`, `InteractionHand`) are
  85 of the 138 fields. Mild leverage: a codec table buys something, but not much.
* **handling** — 58 packets, 58 handlers, **leverage 1.0**. Every handler is its
  own behaviour.

**This is the strongest available check on the leverage metric itself, because it
retrodicts a fact nobody chose.** The census records 21 of 69 packets sendable
and 48 unhandled: the decode side of this surface really did move about three
times further than the handling side, before anyone was measuring leverage and
without anyone deciding it should. A metric that only ever agreed with the
surface it was invented on would be worth little.

---

## 3. So: three years, or not?

**Not three years — but not because the work is smaller. Because a third of it
is the wrong shape to be counted in lane-weeks at all.**

Restated in the buckets:

* **EXTRACTABLE** — loot (done to 1,109 tables in 24 minutes), the command tree,
  `update_shape`. These collapse. The assessment's extrapolation does not apply
  to them and its own loot row, *"weighted by vanilla's own item counts, where
  1,688 advancements and 1,355 loot tables dominate: 2 to 4%"*, is the row most
  affected: the denominator it uses is the one that just moved cheapest.
* **TRANSCRIBABLE** — entity AI (0.87 and 0.18), command executors (1.0), block
  entities (1.0), packet handlers (1.0), packet decode (3.0). Here the assessment
  is **right**, and one entity type a week is a fair rate. This is the bulk and it
  does not collapse. Every one of these is now measured rather than assumed.
* **IRREDUCIBLE** — the call sites. This is the part neither the assessment nor
  this lane has priced, and it is the reason a collapsed loot surface is not yet
  a visible improvement: 1,109 correct tables that nothing invokes.

The honest correction to the headline is therefore narrower than "the number is
too pessimistic" and more useful:

> The three-year figure is a **sequencing** error more than an arithmetic one.
> It measures a project doing leverage-1 work on leverage-32 surfaces. Moving the
> extractable surfaces to generators does not make the transcribable ones
> faster — it stops them competing for the same lanes.

**What I would change tomorrow, in one line each:**

1. **Never schedule a lane against a surface without measuring its leverage
   first.** It takes an hour and it decided everything in this document.
2. **Call `Loot::block_drops` from the block-breaking path.** The dispatch is
   built and tested over all 1,196 blocks; what is missing is one call from
   `rustmc-server`, deliberately left to the lane that owns that file. Entity
   death and chest fill are behind the entity and structure subsystems and are
   not loot work.
3. **Stop scheduling command lanes.** 52 of 90 roots are entity and scoreboard
   work wearing a command's name, and a lane that takes them will report zero
   again.
4. **Take the 38 unblocked command roots and the command tree together**, once,
   as one lane. That is the whole of what commands can be today.

### 3.1 Where I think this document is most likely to be wrong

The 24-minute figure is the number most likely to be misused, including by me.
It is a prototype rate on the most favourable surface in the project, it produced
something with no call sites, and it compares against a hand-porting rate that
included gates, evidence and review this prototype has only partly paid for. **If
one number from this lane survives, it should be the leverage table in §0, not
the 24 minutes.**

---

## 4. Instruments added by this lane

| file | question it answers | falsifier |
|---|---|---|
| `ci/loot-census.py` | with N opcodes implemented, how many of 1,355 tables run end to end | `ci/loot-census-falsify.py` — 5 cases, each must pass real and fail broken |
| `ci/loot-golden.sh` + `tools/goldengen/vanilla/LootGolden.java` | what does vanilla actually drop, at a fixed seed | refuses a vector file where fewer than a quarter of successes dropped anything |
| `crates/rustmc-loot` | can a 23-opcode interpreter reproduce vanilla's own vectors | `tests/falsify_loot.rs` — 5 injected faults, RED / VACUOUS / UNREACHABLE |
| `crates/rustmc-loot` `Loot::block_drops` | is the right table reached for the right block | `tests/block_dispatch.rs` — all 1,196 blocks, zero unknown, zero missing, 22 by design asserted |
| `ci/command-tree-golden.sh` + `CommandTreeGolden.java` | how much of the command surface is shape rather than code | refuses a tree under 80 roots or 1,500 nodes |

```
python3 ci/loot-census.py --curve
LOOT_JAR=<pinned jar> python3 ci/loot-census-falsify.py
ci/loot-golden.sh                      # ~40s, no server
ci/command-tree-golden.sh              # ~10s, no server
cargo test -p rustmc-loot -- --nocapture   # NOT `cargo build -p`, which skips the tests
```
