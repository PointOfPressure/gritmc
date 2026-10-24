# Cold start — branch `world-systems-2`

Every `### COLD START` entry ever written for this branch, appended in order.

---

### COLD START — branch `world-systems-2` off `main` (`1a31e0a56`), 2026-09-05

**The second world-systems lane: wiring what the first one transcribed into
the play path, so a client actually receives it.** Worktree
`~/grit/work-misc`, remote dir `~/rustmc-misc2` on the **Threadripper**. Not
merged — the orchestrator gates.

**It is rebased onto `main`, not onto `world-systems`.** The brief said "on
top of `world-systems`", and `world-systems` is off `58f1fa5c6`, which is 14
commits behind main and does **not** contain the `commands` branch — no
`cmdreader.rs`, no argument nodes, no `Parser::parse`. Every command this lane
registers takes arguments, so the base had to be main. The rebase replayed
`world-systems`' 15 commits with two conflicts (`docs/FINDINGS.md`, both sides
kept; `ci/checks/test-targets.toml`, two `[[bucket]]`s that had merged into
one).

#### WHAT A CLIENT NOW RECEIVES

| packet | when | checked against |
| --- | --- | --- |
| `initialize_border` | **on join**, between `login` and the `LEVEL_CHUNKS_LOAD_START` game event | the 39 bytes in `golden/world_border.json`'s `fresh` state, vanilla's own `STREAM_CODEC` |
| `set_border_size` | `/worldborder set <d>` | `golden/world_border.json`, and scenario `113` |
| `set_border_lerp_size` | `/worldborder set <d> <time>` | same golden |
| `set_border_center` | `/worldborder center <x> <z>` | same golden |
| `set_border_warning_delay` / `_distance` | `/worldborder warning time\|distance` | same golden, and scenario `114` |
| `set_objective` | **`setdisplay`, not `add`** | `golden/scoreboard.json`, and scenario `115` |
| `set_display_objective` | `/scoreboard objectives setdisplay` | same golden, and `115` |

Plus nineteen reply components — ten `/worldborder`, six `/scoreboard`, and
`chat.square_brackets` twice inside them — every one byte-exact against
`ci/commands-golden.sh`'s output from vanilla's own
`ComponentSerialization` stream codec, first run.

Position in the join sequence is vanilla's, from bytecode:
`PlayerList.placeNewPlayer` calls `sendLevelInfo` once, and `sendLevelInfo`
sends the border **first** — offset 9, ahead of the clock full-sync at 31, the
default spawn at 41, the weather events at 66..118 and the game event at 128.
This server sends none of the four in between at join, so the ordering fixes
the position exactly.

#### THE INSTRUMENT PROBLEM, AND WHAT IT COST TO SOLVE

**The corpus could not reach any permission-level-2 command.** `Step::Command`
is level 0: the harness's players are `oracle` and `oracle_eye`, neither
server has an `ops.json`, and op-ing them was considered and rejected long ago
because `sendPlayerPermissionLevel` changes the `entity_event` byte **and**
the whole `commands` packet at join for every check in the corpus.

`record::Console`'s header says there is no console on our side. **That has
not been true since `ci/t2-gameplay.sh` started passing `--console` on both
record passes** — `$VAN_DIR/console.fifo` and `$OUR_DIR/console.fifo` — and
`crate::console` is the reading end. So `Step::Console` is symmetric, and a
check built on one is a real comparison rather than a reference running a
mechanism the subject is not.

A console step with no `--console` **refuses** rather than skipping: a skipped
line leaves a window holding only what the step did not cause, on both
servers, agreeing perfectly.

`crate::console` also applies `Commands.trimOptionalPrefix` now — one leading
slash removed, nothing else — without which a line typed with the slash a
player would use runs on vanilla and not here.

#### THE BRIEF WAS WRONG ABOUT ONE THING AND IT IS LOAD-BEARING

**`WorldBorder.tick()` reaches no listener.** It is three instructions,
`extent = extent.update()`, and every `BorderChangeListener` call site in the
class is in a setter. There is no per-tick border broadcast; the whole shrink
animation is the client interpolating the single `set_border_lerp_size` it was
sent. Ticking is still required — `getSize()` and a later joiner's
`initialize_border` both read the extent — and it runs as `ServerLevel.tick`'s
first step (offsets 32..48), ahead of weather and ahead of the world guard.

#### THE SIX THINGS THAT WOULD HAVE BEEN WRONG

1. **`PlayerList$1` has seven listener methods and only five send anything.**
   `onSetDamagePerBlock` and `onSetSafeZone` have empty bodies, so
   `/worldborder damage amount|buffer` change state, answer the caller and put
   **no packet on the wire**.
2. **`ServerScoreboard.onObjectiveAdded` broadcasts nothing.** Every
   scoreboard packet goes through `trackedObjectives`, a field on the server
   subclass: `setDisplayObjective` on an untracked objective calls
   `startTrackingObjective`, and *that* sends `set_objective` METHOD_ADD then
   the display packet. The first packet a client ever sees for an objective is
   the one `setdisplay` emits.
3. **Java's `%f` is not Rust's `{:.N}`.** HALF_UP on the shortest round-trip
   repr against half-to-even on the exact value. `%.1f` of `0.15` is `0.2` and
   `0.1`; `%.2f` of `0.125` is `0.13` and `0.12`; `%.0f` of `2.5` is `3` and
   `2`. All three are typeable at a `/worldborder`.
4. **`Vec2Argument.vec2()` adds half a block.** `centerCorrect` is `true`, so
   `WorldCoordinate.parseDouble` adds `0.5` when the text held no `.` and the
   coordinate was not relative: `/worldborder center 10 20` centres at
   (10.5, 20.5). And `Vec2Argument.parse` has **no `^` branch at all**, where
   `Vec3Argument.parse` opens with one.
5. **`ObjectiveCriteria.byName` throws an unchecked exception** on a `:` name
   that is not a valid identifier — `IdentifierException` from
   `assertValidPath`, escaping the parser entirely.
6. **`Objective.getFormattedDisplayName()` is not the display name.** It is
   `wrapInSquareBrackets(displayName.copy().withStyle(hover(name)))`, and it is
   the argument of three of the six scoreboard replies.

#### TWO DEFECTS IN THE TOOLING, BOTH FOUND BY LANDING ON THEM

* **`ci/command-parse-golden.sh` recorded a non-deterministic field.**
  `String.valueOf(e.getType())` is an identity hash for any exception type
  with no `toString`, which `DynamicCommandExceptionType` is. The dumper runs
  twice and compares, and the two runs disagreed on every row raised by one —
  the moment `minecraft:objective_criteria` landed, because no earlier parser
  had reached that class. It records the type's class name now.
* **Its input corpus held no criterion and no display-slot name**, so both new
  parsers would have accepted nothing and been refused by the dumper's own
  "a parser that accepts nothing is indistinguishable from one that always
  fails" check. Seven inputs added, one per arm of the two lookups.

#### WHAT IS REFUSED, AND WHY — SO NOBODY RE-RUNS IT

* **Dimension travel and the nether.** `crates/rustmc-proto/src/ids.rs` has no
  `respawn` packet id at all — `grep -i respawn` finds one serverbound
  `client_command` comment and nothing else. So `changeDimension` has no
  packet to send and a dimension port would be a state change no client could
  observe. Adding the id belongs to the packets lane; this is the same
  argument the `commands` lane used to refuse `/difficulty` over
  `change_difficulty`. The library (`crate::dimension`, `crate::portal`, the
  clamps and the 144 portal-frame fields) is complete and gated and is waiting
  on that one id.
* **A villager trade through `merchant_offers`.** No villager can stand:
  `entities::KINDS` is `[PLAYER, ITEM]`, and
  `ci/checks/gameplay-baseline.toml` baselines `spawn-egg-makes-a-mob` at
  **`absent`** — this server spawns no mob at all. The brief's "mob-in-world
  serves pig and bat" is true of an unmerged branch, not of `main`. The
  152-byte packet is byte-identical and has nothing to open.
* **The online-mode handshake behind a config flag.** The flag would have
  nothing to turn on. `generate_key_pair` is refused, so there is no RSA
  keypair to offer in `ClientboundHello`; and the step between "the client
  proved it holds the shared secret" and "this player is who they say" is
  `hasJoinedServer`, refused because it is a live HTTPS call to a third
  party's database. Wiring the cipher onto the socket with both ends refused
  would be either dead code or a server that admits anyone who completes a
  handshake against a published key — which
  `tests/online_login.rs::the_refusals_are_refusals` exists to prevent.
* **`/scoreboard objectives modify` and every `players` branch**, and
  `/worldborder`'s executor-refusal MESSAGES. The first two take
  `minecraft:component` or `minecraft:score_holder`, neither of which this
  server can read, and they are **not registered** rather than registered and
  silent. The third is `Commands.executeCommand`'s catch, a different path
  from the `finishParsing` one `crate::commands::failure_components` renders;
  `BorderError`/`ScoreboardError` carry which message is owed.
* **`/team`.** Not started. `crate::scoreboard::PlayerTeam` and
  `set_player_team` are transcribed and gated; the command needs
  `minecraft:team`, `minecraft:team_color` and `minecraft:score_holder`, and
  the last of those is the same wall `players` hit.

#### NUMBERS

    725 lib tests, 116 oracle-runner tests, 0 failed
    command-parse: 3,267 agreed / 0 disagreed / 264 unread (ceiling 264) /
                   3 vanilla-threw, of 3,534 rows
                   — 31 parser configurations x 114 inputs, up from
                   2,752 / 244 / 2,996 at 28 x 107
    19 reply components byte-exact against vanilla's own stream codec,
       all on the first run
    2 command roots added (11th and 12th), 15 executable paths

Threadripper, `rustmc-misc2`, 2026-09-05.

#### THE SINGLE NEXT ITEM

**`/team`, and then the `respawn` packet id.** `/team` is the last
wire-visible system in this area whose library is complete and whose command
is absent, and it needs exactly three parsers —
`minecraft:team` is `readUnquotedString` like `minecraft:objective`,
`minecraft:team_color` is a 16-name enum already in
`scoreboard::TEAM_COLORS`, and `minecraft:score_holder` is the one that also
unblocks `/scoreboard players`. The result that closes it: a
`ci/checks/gameplay/*.toml` for `/team add` + `/team join` observing
`set_player_team`, swept and registered at tier 2.

After that, `respawn` in `crates/rustmc-proto/src/ids.rs` turns
`crate::dimension` from a gated library into a thing a client can see, and it
is one constant.
