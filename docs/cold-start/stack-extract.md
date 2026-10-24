# COLD START — `stack-extract`

### COLD START — branch `stack-extract`, off `main` (`f4fa28ff8`), 2026-09-05

The extractable-surface stack: `origin/extract-classify` and
`origin/commands-surface` folded onto main, both of their gate reds repaired,
the command-tree extractor run and gated, argument nodes implemented, and
`/gamerule` ported end to end. Worktree `~/grit/work-xtr`, remote dir
`~/rustmc-xtr`. Not merged — the orchestrator gates.

## WHAT CLOSED

**`origin/extract-classify` (12 patches).** Its only gate red was that
`crates/rustmc-loot`'s five cargo targets were unaccounted in
`ci/checks/test-targets.toml`. Accounting them found the reason the manifest
could not simply be extended, and it is the larger half:

> **Two of the four fixtures the crate reads were produced by no committed
> step.** `data/vanilla/loot_tables.json` (1,355 table documents) and
> `data/vanilla/item_tags.json` (224 flattened item tags) existed on this box
> in exactly one directory, `~/grit/work-extract/data/vanilla`, and
> `ci/loot-golden.sh` writes neither. From a clean checkout `golden_loot` and
> `block_dispatch` panic on a missing file, and the crate's headline figure
> (17,943 of 18,003 vectors) rested on documents nothing in the tree could
> reproduce.

`ci/loot-golden.sh` writes all four now — python rather than Java, because both
are restatements of files already unzipped out of the pinned jar and a JVM adds
a dependency and no information. Verified against the two hand-made files:
1,355 tables and 224 tags, key sets and values identical both ways, so every
vector taken against them still stands. The tests also built their fixture
paths by joining a directory to a bare filename, so
`ci/gen-gameplay-tables.sh`'s derived-table grep (which matches the literal
`"data/vanilla/<name>.json"`) saw none of them; they pass the whole path now
and `generator_for()` maps all four to `ci/loot-golden.sh`.

The three test targets are in **T0**, not T1: 1.26 s together (golden_loot
0.25, block_dispatch 0.13, falsify_loot 0.88; Threadripper, `cargo test
--release -p rustmc-loot`, 41.6 s wall including the build, 519 MB peak RSS).
Under `--lib --bins` cargo builds none of them, so a green T0 on that crate
measured the lib compiling.

**`origin/commands-surface` (26 patches).** Its 2026-09-05 T0 red was
`cargo fmt --check` and nothing else: seven rustfmt line wraps in
`crates/rustmc-server/src/commands.rs` and `tools/oracle-runner/src/trace.rs`,
with stages 5 and 6 skipped behind it. A clippy lint
(`unnecessary_get_then_check`, `-D warnings`) surfaced on the first gate of the
folded stack and is fixed too.

**The command-tree extractor.** `ci/command-tree-extract.sh` and
`tools/goldengen/vanilla/CommandTreeExtract.java` had merged to main untested.
Run against the pinned jar they work as drafted:

```
90 roots  2,085 nodes  1,084 argument  1,281 executable  112 redirects
126 custom-suggestion  57 parsers registered (55 used)
packets 265 / 265 / 29,279 / 30,099 / 30,248 bytes at ALL..OWNERS   32.1 s
```

**Argument nodes, redirects and custom suggestions**, which
`crates/rustmc-server/src/commandtree.rs` refused by type and said so in its
header. All three are implemented and the gate is
`crates/rustmc-server/tests/command_tree_vanilla.rs`: vanilla's **entire**
dispatcher rebuilt in our types from the semantic dump and encoded by the same
`encode` production calls, compared byte for byte against vanilla's own stream
codec at all five permission levels. 8 tests, 0.10 s.

There is **one** encoder. `Tree` stays the builder `crate::commands` registers
against and lowers to a `Graph`, which is vanilla's actual data structure
(2,084 child edges plus 112 redirect back edges). Two encoders would have left
this server's own packet ungated at scale.

**`/gamerule`, end to end.** 233 nodes under the root, 236 in the packet;
21 / 21 / 4,655 / 4,655 / 4,655 bytes at ALL..OWNERS, byte-exact against
vanilla's own encoder; query and set executors over the `crate::gamerules`
store that already existed; four reply vectors byte-exact against vanilla's
component codec. `docs/generated/command-surface.tsv`: roots 90, level0 10,
**ported 2 -> 3**.

## THE THREE THINGS THAT WERE HARD, AND WHERE THEY LIVE

**1. `enumerateNodes` deduplicates by Brigadier's STRUCTURAL equality, not by
identity, and nothing anywhere names it.** It collects into an
`Object2IntOpenHashMap`, which hashes by `equals`/`hashCode`, and
`CommandNode.equals` compares the children map and the `Command` object and
nothing else — not the requirement, not the redirect, not the suggestion
provider. So `if (map.containsKey(node)) continue` is not a cycle guard: it
merges every structurally identical subtree.

At `OWNERS`, where the permission filter drops nothing, vanilla's 2,085-node
dispatcher becomes a **2,017-entry** packet. All 68 collapsed nodes are in
`/execute store {result,success} {block,entity,storage} <path>
{byte,short,int,long,float,double} [scale]`: the six `scale` arguments are
non-executable `DoubleArgumentType`s with equal bounds, so they are equal, so
the six numeric literals above them are equal, and the shape is written once
and pointed at from everywhere. An encoder deduplicating by identity emits
2,085 entries and a packet **694 bytes too long**, which is what this one did
for the first hour. `Graph::class_of` has it.

Two corollaries: `ArgumentType.equals` is by value for only four of
Brigadier's six primitives (`integer`, `long`, `float`, `double`) and identity
for `bool`, `string` and every `net.minecraft.commands.arguments` type; and an
executable node never merges, because `Command` is a lambda. That second one
bites in the Java golden — a **non-capturing** `c -> 0` is one cached instance
per call site, so all 94 boolean `value` nodes would merge into one.

**2. Vanilla's own `ClientboundCommandsPacket` is not byte-stable for
`/gamerule`.** Sixteen consecutive JVM runs of the same class file against the
same jar: fifteen produced one document, one produced another, differing in
**160 of 2,085 nodes**, every one a `/gamerule` child, permuted, with packet
lengths identical at all five levels. `GameRuleCommand.register` walks
`new GameRules(ctx.enabledFeatures())` with `visitGameRuleTypes` and that
iteration is not stably ordered.

So there is no byte-exact golden of that subtree to take, and a port that
reproduced one run's order would be gated against a coin flip.
`ci/command-tree-compare.py` replaces the `cmp`: a difference confined to a
named root **and** which is a permutation of the same multiset is accepted and
recorded in the emitted document as `unstable_subtrees` / `unstable_nodes`;
anything else is refused. 13 assertions, 8 refusal cases, in T0. The allowance
is bounded three ways on purpose — a named constant, a permutation-only test,
and writing down what it forgave.

`/gamerule`'s order in our tree is therefore **ours**:
`crate::gamerules::DEFAULTS`' order, the jar's `<clinit>` order, stable and
already transcribed. `ci/commands-golden.sh` registers the same 58 rules
through vanilla's own builders in the same order, so a wrong bound, a wrong
type or a missing rule is still a red gate; only the order is ungated, and
`command_tree_vanilla::our_gamerule_subtree_is_vanillas_rules_with_vanillas_bounds`
closes that half order-insensitively against vanilla's own subtree.

**3. `ci/command-tree-extract.sh` reported that instability as "the dump is not
deterministic"** — a claim about vanilla — and its `run()` ran java with
`2>/dev/null` and checked only that the output file was non-empty, so a JVM
that died mid-document produced the identical message with its exit status and
its output thrown away. Both fixed.

## NUMBERS THAT DID NOT REPRODUCE — do not re-derive these

* **`docs/EXTRACTABILITY.md` §2.1's "38 of 90 roots unblocked" does not
  reproduce.** Its other two figures do, exactly: 36 of 90 roots take an
  `EntityArgument` somewhere, and the registry groups are 5 players / 4 blocks
  / 4 nbt / 4 scoreboard. The free count is **35** by the same sweep and
  **30** once redirects are followed — `tp`, `tm`, `w` and `xp` are pure
  redirects into entity-bearing subtrees and are free only if the walk stops
  at the redirect edge, which it cannot, since `FLAG_REDIRECT` carries a node
  index. The method is a DFS of each root's subtree collecting every
  `argument` node's `parser`, refusing the root if the set meets a
  subsystem-bearing parser; the exact script is in the commit message of the
  §2.1 correction. §2.1 now lists the surviving 30 by name.
* **§2.3's "48,405 pairs" does not reproduce.** The extract this tree
  generates says **48,993** for the dominant refusal, of **79,111** refused
  over eleven reasons, 12,716 answered, 11,491 connective, of 103,318
  sensitive. `docs/SERVER.md:808` and `docs/WORKLIST.md:1189` already carried
  48,993; §2.3 was the stale copy and it is the one an incoming lane reads
  first.
* **`update_shape.json`'s per-reason block counts were 7x low.**
  `ShapeGolden` capped each reason's block list at eight
  (`if (blocks.size() < 8)`), so six of the eleven reasons reported exactly 8
  blocks and the union read as 37 distinct blocks. Uncapped it is **258**.
  Pair counts were never affected. `blockCount` is emitted beside the list now.

## WHAT WAS DELIBERATELY NOT DONE, AND WHY

* **The other 29 unblocked roots are not registered.** Registering a tree
  whose executors do not exist puts nodes on the wire that answer with
  silence, and `crate::commands`' own header already refuses `/version` on
  exactly that ground: "a client offered a command that does nothing is worse
  on the wire than a client not offered it". `/gamerule` was portable because
  `crate::gamerules::GameRules` already existed with `get` and `set`. The rest
  need their subsystem first: `difficulty`, `weather` and `tick` have no state
  on this server to read or write, `say`/`me`/`teammsg` need the chat
  broadcast shape, `help` needs Brigadier's usage strings.
* **The `update_shape` port was not attempted.** §2.3 prices it at a lane-week
  and `crates/rustmc-server/src/shapes.rs` is 136 KB; the honest contribution
  here was the re-measurement and the refusal census, which turns "one
  lane-week" into a spec. See the next item.
* **`Tree::resolve` is a single-token approximation of Brigadier's parse, and
  the approximation is a refusal rather than a comment.** `Refusal::Unparsed`
  is a new arm for the 53 parsers this server cannot tokenise, kept apart from
  `Unknown` so the gap stays visible. Greedy arguments, quoted phrases and
  multi-token positions (`minecraft:vec3`) are each a different tokeniser and
  none is implemented.
* **`sendSuccess`'s operator broadcast is not ported.** `setRule` passes
  `true`, which in vanilla also logs the change to every other operator when
  `send_command_feedback` is on. This server answers the caller only.

## THE SINGLE NEXT ITEM

**Close the six positional refusals in `update_shape` with a second sweep
position in `ShapeGolden`.** They are **78,777 of the 79,111** refused pairs,
they are one mechanism (`updateShape` reading a fixed offset that is not the
neighbour that changed), and the census now names every block in each.
The other five reasons are 334 pairs over 13 blocks and are genuinely
per-block.

**What closes it:** `data/vanilla/update_shape.json`'s `refusedPairs` falls
from 79,111 to at most 334, with the added rows answered by the same
`ShapeTable` the server already loads and no new hand-written arm in
`crates/rustmc-server/src/shapes.rs`.
