# COLD START — `commands`

### COLD START — branch `commands`, off `origin/stack-extract` (`58f1fa5c6`), 2026-09-05

The command-executor lane: what the roots this server can reach actually
**do**, on top of `stack-extract`'s tree. Worktree `~/grit/work-cmdx`, remote
dir `~/rustmc-cmdx`. Not merged — the orchestrator gates.

## WHAT CLOSED

**Ten roots ported, from three to ten.** `docs/generated/command-surface.tsv`:
`roots 90  level0 10  ported 10`.

| root | shape | reply | gated against |
| --- | --- | --- | --- |
| `/version` | literal, L2 | 10 `system_chat` | `dumpVersion`'s own components |
| `/say` | `minecraft:message`, L2 | `disguised_chat` broadcast | the chat-type capture |
| `/me` | `minecraft:message`, L0 | `disguised_chat` broadcast | the chat-type capture |
| `/msg` | entity + message, L0 | 2 `disguised_chat` per target | tree bytes at 5 levels |
| `/tell`, `/w` | redirects into `msg` | as `/msg` | tree bytes at 5 levels |
| `/help` | literal + greedy string, L0 | one line per usable root | vanilla's `getSmartUsage` |

plus `/list`, `/list uuids`, `/seed` and `/gamerule` from before. The tree is
**byte-exact against vanilla's own encoder at all five permission levels** with
all of it registered: 106 bytes at ALL, 4,782 at GAMEMASTERS and above.

**And the item that is worth more than any of them: the answer to a command
that does not run.** `Commands.finishParsing`'s two `system_chat` messages, for
every typo, every root this server has not ported, and every command a player
lacks permission for. `crate::commands`' header called this "one job whose
reach is the whole unported surface"; before it, all of those were answered
with silence, which on the wire is indistinguishable from a command that ran
and printed nothing.

```
80 golden rows (16 failing inputs x 5 permission levels)
72 rendered byte-exact   8 silent where vanilla is silent   0 unrendered
0 disagreements
```

**Two instruments, both in T0.**

* `ci/command-parse-golden.sh` — **2,889 rows** of
  `(parser, input) -> (accepted, StringReader.getCursor())` from vanilla's own
  `ArgumentType.parse`, over 27 configurations of 15 parser names. The cursor
  is the recorded quantity: a parser one character over leaves the NEXT node
  reading the wrong input, and the answer is "unknown command" for a command
  that is present, correctly spelled and whose executor works.
  **1,674 -> 2,645 agreed, 0 disagreed, 1,209 -> 244 unread**, ratcheted.
* the `failures` block in `ci/commands-golden.sh` — vanilla's own
  `finishParsing` through a recording `CommandSource`, invoked by reflection.

Both carry a positive control that must catch a deliberately one-character-wrong
answer on **every** row that has one.

## THE FOUNDATION, WHICH WAS THE FIRST HALF OF THE WORK

`Tree::resolve` split on whitespace and gave each argument node one token. That
is a model of Brigadier, not a transcription, and it forecloses whole command
classes rather than approximating them: `minecraft:block_pos` is three tokens
in one node, `minecraft:message` is every remaining token in one node, and
`stone[facing=north]` is one node whose argument ends after `stone`. `/setblock`,
`/say`, `/me`, `/msg`, `/tellraw`, `/give` and `/help` were unreachable
independently of whether their executors existed.

`crates/rustmc-server/src/cmdreader.rs` is brigadier-1.3.10's `StringReader`,
transcribed. `Tree::resolve` is `CommandDispatcher.parseNodes`, including
`getRelevantNodes`' literal-map shortcut, a private reader copy per candidate,
the separator rule, `canRead(redirect == null ? 2 : 1)`, and the potentials
sort. Eight parsers are transcribed against the golden.

## NUMBERS AND CLAIMS THAT DID NOT SURVIVE

* **`resolution_matches_whole_tokens` asserted that `"  list   uuids  "`
  resolves**, with the comment "Brigadier tolerates it the same way". It does
  not. `parseNodes` skips exactly one separator character and
  `Commands.trimOptionalPrefix` removes a leading `/` and nothing else. A
  doubled, leading or trailing space is a syntax error on a real 26.2 server,
  and the old assertion was a property of the tokeniser under test written down
  as a property of vanilla.
* **The `minecraft:chat_type` registry the client receives is KEY-SORTED**, not
  in `ChatType.bootstrap`'s order. `say_command` is **4** and `emote_command`
  is **1**, not 1 and 6. Written in bootstrap order, refused by the capture on
  the first run.
* **`javap` strips trailing whitespace from a `Utf8` constant.**
  `CommandDispatcher.USAGE_REDIRECT` renders as `->` and `ARGUMENT_SEPARATOR`
  renders as *nothing*; `od -c` confirms it. Transcribed from the listing, a
  redirect usage is `tell ->msg`; vanilla produces `tell -> msg`. Under a
  clean-room rule the listing is the only other source, so **anything
  transcribed from a string constant that could end in a space needs a
  vector.**
* **`BrigadierExceptions` swaps the two arguments of every bound message.**
  `integerTooLow().createWithContext(reader, result, minimum)` becomes
  `translatable("argument.integer.low", minimum, result)`. Bound first.
* **`ListTag.write` wraps mismatched elements**, and three call sites had been
  applying that rule by hand without naming it. `/list`'s `{"": 2}` is an
  `IntTag` the WRITER wrapped because a Component sat beside it;
  `/version`'s `commands.version.data` is a bare `TAG_Int` because nothing
  disagrees with it. A port that always wraps gets one right and the other
  wrong.
* **`ci/command-surface.py::ours()` silently dropped roots three times in one
  day** — a builder with arguments, an unbounded builder-body search, and
  `Node::redirect_to`. All three are refusals now, plus a backstop: any
  `Node::<something>(` at depth one that the parser does not recognise stops
  the generator.
* **`ci/readiness-falsify.sh` carries a timing race that reds a clean branch.**
  1 of 5 runs on an unchanged tree. Not this branch's; it belongs to `ci/`.

The full list is in `docs/FINDINGS.md`, one line each.

## REFUSED, BY NAME AND WITH THE REASON

* **`/difficulty`** — no difficulty state, no `server.properties` reader, and
  **no `change_difficulty` in `crates/rustmc-proto/src/ids.rs` at all**. A cell
  nothing reads is the dead-field defect this project is built around, and the
  packet id belongs to the `packets` lane.
* **`/gamemode`, `/defaultgamemode`** — `crate::status`'s `GAME_TYPE` is a
  process-wide cell set before the listener opens, with no per-player field and
  no abilities model (`ci/checks/server-unported-inventory.json` carries
  `abilities-model` as a GAP). Changing it at runtime would be observable to
  nothing.
* **`/give`** — the non-overflow path is reachable (`Hotbar::add_item`,
  `container_set_slot_body`), but `GiveCommand` **drops an `ItemEntity` on
  overflow and plays a sound whose pitch is `player.getRandom().nextFloat()`**.
  An item entity and an RNG draw from a source this lane cannot see are two
  other lanes' work, and a `/give` that diverged only on overflow would be
  worse than none.
* **`/weather`, `/time`** — `origin/player-systems` carries `weather.rs` with
  `advance_weather_cycle` and both draws, unmerged. Registering a root that
  writes a cell that lane owns is an invitation to a conflict, and the state
  has no home on `main` yet.
* **`/kill`** — needs damage and death, which is `origin/player-systems`.
* **`/setblock`, `/fill`, `/clone`** — reachable in principle (`crate::edits`
  exists) and blocked on `minecraft:block_state`, which is `BlockStateParser`'s
  1,476 lines of javap plus the block registry. Priced, not attempted; see the
  next item.
* **`@p` and `@r` inside `/msg`** — `@p` is `ORDER_NEAREST` over the source's
  position and this server tracks no position for a connection other than the
  session's own; `@r` is a draw from an RNG we cannot see. Named in the
  executor rather than silently treated as `@s`.
* **A `/say` or `/me` carrying a resolvable entity selector, at GAMEMASTERS or
  above** — `Message.resolveComponent` would substitute the matched names.
  Below GAMEMASTERS vanilla does **not** resolve it (the permission gate is
  `COMMANDS_ENTITY_SELECTORS`, which `LevelBasedPermissionSet` answers as
  `level >= GAMEMASTERS`), so that case is exact.

## THE SINGLE NEXT ITEM, AND THE DESIGN QUESTION IN FRONT OF IT

**`minecraft:block_state` and `minecraft:item_stack` — but they cannot be
written the way the other eight were, and the golden says so.**

They are the last two parsers in `ci/command-parse-golden.sh`'s corpus this
server cannot read: 186 of the 244 unread rows. The pattern the other partials
used — transcribe the head, refuse the bracketed part, declare it in the
golden's `PARTIAL` set — **does not work here**, because for these two the
registry decides the ACCEPT, not just the value:

```
minecraft:block_state  'stone'          accepted@5    'hello'  rejected@0
minecraft:block_state  'diamond_sword'  rejected@0    <- an item, not a block
minecraft:item_stack   'diamond_sword'  accepted@13   <- the same string
```

The two parsers differ only by which registry they consult, and a port with no
registry would accept `hello` as a block. Even the property parse is
registry-dependent: `stone[facing=north]` is rejected because `stone` has no
`facing` property (`ERROR_UNKNOWN_PROPERTY`), not because the syntax is wrong.

`Parser` is a pure static description of a node — a registry name, its wire
properties and an optional suggestion provider — and `Parser::parse` is a
method on it with nothing else in hand. **Threading a block and item registry
into it is a design change, not a transcription**, and it is the decision this
lane stopped in front of rather than making badly. `data/vanilla/block_props.json`
and `data/vanilla/items.json` are both present and both loaded elsewhere in
the server, so the data is there; what is not settled is whether `parse` grows
a context argument, whether `Parser` grows a lifetime, or whether these two
parsers live outside `Parser::parse` entirely and `Tree::resolve` consults a
side table.

A second finding to carry into it: **`ItemArgument.item(ctx)` is built with
`allowNbt = false`.** `block_state` on `stone{Foo:1b}` ends at 13 — it consumed
the compound — and `item_stack` on the same string ends at **5**. In 26.2 item
components are `[...]` and the trailing `{...}` is the next node's problem.

**What closes it:** `command_parse_vanilla`'s unread count falls from 244 to at
most 58, and `/setblock` answers `commands.setblock.success` byte-exact through
`crate::edits::Blocks`. `/give` stays refused until the item-entity drop path
exists — see above.
