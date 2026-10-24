# Cold start — branch `chat-commands`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (2 entries). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `chat-commands`, rebased onto `main` (`0544dfec`), 2026-08-09

**The one red that gated this branch was not this branch's and was not a port
defect. `040-stairs-and-furnace-face-apart` is a coin flip on step
attribution, and it has been one since the scenario was written.** Worktree
`~/grit/work-cicounts`, remote dir `~/rustmc-integrate`. Not merged — the
orchestrator gates. The branch's own account of the chat/command port is the
`chat-commands` block further down this file and is unchanged.

**THE NUMBERS, first.** `ci/t2-gameplay.sh`, corpus **88** — this branch adds
`111` and `112`, so nothing here may be compared against `main`'s 86 — vanilla
26.2 protocol 776, superflat, seed 20260803, Threadripper.

```
gating run   (03:30, pre-rebase, unfixed):  declared 61/88, strict 59/88   REGRESSED 040
before       (04:34, rebased,   unfixed):   declared 62/88, strict 60/88   GREEN, 040 match
after        (05:13, rebased,   fixed):     declared 62/88, strict 60/88   GREEN, 040 match
```

**Read the middle line before anything else.** The same branch, the same
harness, the same corpus, 28 minutes apart: one red, one green. The `+1` on
both denominators between the first two lines is 040 alone. **A scenario whose
verdict changes between two runs of one tree is not measuring the server**,
and the honest "before" for this item is a distribution, not a number.

Gate: `ci/t0-remote.sh` exit **0**, `grep -c "skipping: no vanilla"` = **0**,
`ci/t2-gameplay.sh` **T2-GAMEPLAY GREEN with no `REGRESSED` line**.

**What actually caused the step shift.** Nothing on this branch.
`git log main..HEAD -- ci/checks/gameplay/040-*.toml` is empty and
`recipe_book.rs` is untouched. `Step::Hold` in
`tools/oracle-runner/src/client.rs` returned a **60 ms** window, shorter than
one 50 ms vanilla tick plus a round trip, so anything vanilla says in answer to
a hold was a coin flip between that step and the next. Six full-corpus
recordings were already on the box — `~/rustmc`, `~/rustmc-droppickup`,
`~/rustmc-integrate`, `~/rustmc-pairing`, `~/rustmc-placesound`,
`~/rustmc-recipebook`, four branches and two bases — and all six hold 040 with
`rest = 7a679a94549f118e`:

```
vanilla  322, 333, 335, 345, 381, 383 ms   ->  step 2,2,2,2,2,3
ours     317, 329, 329, 330 ms             ->  step 2,2,2,2
```

The hold was issued at t = 321 ms so the boundary sat at **381 ms**, inside
vanilla's own 1..62 ms spread. `rustmc-pairing` at 381 ms landed in step 2 and
`rustmc-integrate` at 383 ms landed in step 3. **No server-side change could
have fixed it**: vanilla's tick phase is neither observable to nor controlled
by the client, so even a subject that quantised its own answers to 50 ms would
race the reference. `Step::Hold` now returns 250 ms — what `place`, `dig` and
`use` already return. After the fix vanilla is at 538 ms and we are at 516 ms
against a boundary at ~760 ms: the margin went from **-2 ms** to **+220 ms**.
Full account: `docs/PARITY-ROADMAP.md` §12.

**Blast radius: zero, checked row by row.** Before and after per-scenario
verdicts, 88 names each, both denominators: **0 rows moved**. That includes
`490`, `520`, `560`, `561` and `601`-`608`, the scenarios §2 R1 says a
free-running timer decides and which carry two to four holds each — they were
the standing risk of widening a window and they did not notice.

**What I killed as a null — do not re-run these.**

1. **The briefed hypothesis, "the command registry changes the login/join
   packet sequence and shifts a later packet's attribution by one".** Refuted
   without a new run, from the six recordings above: the late arrival appears
   on branches with no command registry and the early one appears on this
   branch's own predecessor. It tracks vanilla's tick phase and nothing else.
2. **Looking for the cause in our source.** `recipe_book.rs`, `world_send.rs`'s
   `apply_hand` arm and the generated `recipe_unlocks.json` are untouched by
   this branch and were never wrong — the digest agreed on both sides from the
   first line of the failure. Rule 9 cuts both ways: a reading of our own code
   that "explained" the shift would have explained a defect that is not there.
3. **Widening 040's projection.** Considered and refused; it is §3 S1's named
   trap. The fix changes no projection — same packet, same step, same digest,
   both sides — and 040's step-0 negative control (`oak_stairs` unlocks
   nothing) is untouched.

**Known, unfixed and named: `Step::Look` also returns 60 ms and has the
identical defect.** Left alone only because no scenario has been *measured*
failing on it and this change was gated on one corpus run, where two
simultaneous window changes could not be told apart. The next
step-attribution divergence whose previous step is a `look` is this, not a
server bug — change the 60 to 250 before opening an investigation.

**Still outstanding and not mine:** the five `BETTER container-click-*` lines
are `main`'s S5 conversions and are still not locked into
`ci/checks/gameplay-baseline.toml`. This branch converts nothing of its own
and has no business ratcheting them.


### COLD START — branch `chat-commands` off `main` (`fbf0ae6`, 1,451 commits), 2026-08-06

**The two plugin-API gaps that were not under-built but *unrepresentable* are
now ported.** Remote dir `~/rustmc-chatcmd` on the Threadripper, working tree
`~/grit/work-chatcmd`, logs under `~/grit/logs/chatcmd/`. Not merged — the
orchestrator gates. Expect a rebase.

Everything is in `docs/intel/chat-and-commands.md` (both halves, with bytecode
offsets) and `docs/intel/chat-commands-corpus-prediction.md` (written before
the run).

#### The one thing to read if you read nothing else

**The chat crypto path is out of scope by measurement, not by choice, and the
proof is three links long.** `ServerGamePacketListenerImpl`'s ctor installs
`SignedMessageChain.Decoder.unsigned(uuid, server::enforceSecureProfile)`;
`Decoder.unsigned` **discards the signature argument** when the supplier is
false; and `DedicatedServer.enforceSecureProfile()` is false unless
`enforce-secure-profile` AND `online-mode` AND `canValidateProfileKeys`. We
send `online_mode: false`. So the unsigned branch *is* vanilla's behaviour
here and the signed branch is unreachable — porting it would port dead code.

Two things go **provably inert** with it rather than being skipped:
`sendPlayerChatMessage` returns at offset 66 on a null signature, so
`MessageSignatureCache.push` and `LastSeenMessagesValidator.addPending` never
run and `chat_ack` has nothing to acknowledge. What an online-mode server
would still owe is listed in the intel doc §2 — it is a sub-project.

#### What was ported

* **Chat.** `ids::play::sb::CHAT` = 9 did not exist before; the packet was not
  unhandled, it was *unnameable*. Serverbound read whole (including the
  20-bit-bitset-plus-checksum tail that desynchronises a frame reader if
  dropped), broadcast as **`player_chat` (65), not `disguised_chat`** —
  `OutgoingChatMessage.create` branches on `isSystem()` and
  `SignedMessageLink.unsigned` leaves the sender as the player.
* **The command registry.** The hardcoded whole-text `list` match is **gone,
  not wrapped**. Literal node tree, per-node permission levels,
  `fillUsableCommands` filtering, `FLAG_RESTRICTED` (32, new in 26.2),
  breadth-first `enumerateNodes` numbering, the `commands` packet (16), and
  the `24 + level` entity event before it. `list` (0), `list uuids` (0),
  `seed` (2). Argument nodes are refused **by the type** — `Node` has no
  argument variant — so an argument command fails to compile rather than
  mis-serialising one of ~50 `ArgumentTypeInfo` payloads.
* **`crate::ops`** reads `ops.json`, so the permission model is reachable on a
  live server rather than being unit-tested dead code.

#### Two bugs this work found in existing code, neither reported

* `configuration.rs` never read chat visibility, so a client asking for
  silence would have been sent chat anyway.
  `ServerPlayer.acceptsChatMessages()` is `chatVisibility == FULL` and nothing
  else — SYSTEM and HIDDEN receive **no packet**, not a filtered one.
* The configuration unit test's `client_information` fixture stopped at the
  view distance. It was a truncated packet that passed only because nothing
  read further; the first field added after it turned the test red.

#### The gate, and why it is built rather than captured

`ci/commands-golden.sh` + `tools/goldengen/vanilla/CommandsGolden.java` emit
**vanilla's own** `ClientboundCommandsPacket` bytes for *our* command set at
all five levels, using vanilla's constructor, vanilla's private
`COMMAND_NODE_INSPECTOR`, vanilla's `fillUsableCommands` and vanilla's stream
codec. Nothing in that path is reimplemented.

A capture from a live server **cannot** gate this: vanilla's dispatcher holds
every command in the game, `enumerateNodes` numbers breadth-first over the
whole tree, so every index depends on commands we do not implement and not one
of our entries is a byte substring of vanilla's packet. Parsing vanilla's
capture the other way needs ~50 argument-type serialisers to skip past
argument nodes.

Measured: 21 bytes at ALL/MODERATORS, 29 at GAMEMASTERS and above, `/seed`
absent below GAMEMASTERS and flagged `0x25` above. **The script refuses a
golden whose rows are all equal**, because `SeedCommand.register`'s boolean,
passed wrongly, makes every row identical while the dump is still vanilla's
bytes.

The same dumper emits vanilla's own component bytes for the `/list uuids` and
`/seed` replies, and **it corrected a wrong guess before a line was written**:
an unstyled literal component encodes as a bare `TAG_String`, not
`{text: "..."}`, so `commands.list.nameAndId`'s arguments are plain strings
and its uuid is the dashed `UUID.toString()` form — a different encoding from
the four-int array `/list` puts in its hover event, for the same uuid, in two
replies to the same command.

#### Nulls — do not re-run these

* **Chat is NOT permission-gated in 26.2.** `Permissions` declares
  `chat/send_messages`, `chat/send_commands`, `chat/receive_player_messages`
  and `chat/receive_system_messages`, and **no other class in the jar
  references any of them** — swept over all 7,434 `net/minecraft` classes with
  a positive control on `COMMANDS_ADMIN`. Declared and unused.
* **A `player_chat` check can never be digested whole.**
  `SignedMessageBody.unsigned` substitutes `Instant.now()`, so the body
  carries the sending server's wall clock and vanilla-against-vanilla would
  diverge. The new `player_chat` projection consumes exactly to the timestamp
  and stops.
* **The instrument lied twice before the permission sweep measured anything**:
  the aliased `grep` is `ugrep`, which skips binary files by default and
  returned zero for every pattern including the declaring class; and the
  extraction dir held a stale second class tree. Use `--binary-files=text` on
  a freshly extracted jar, and keep a positive control.
* **A lib test's cwd is the package dir, not the workspace root.** Three
  golden-backed tests skipped silently on their first run and reported green.
  `crate::power::tests::repo()` is the helper; copy it, and `expect` rather
  than skip.
* **A sweep launched under `setsid nohup` from a non-interactive ssh has no
  `cargo`.** `$HOME/.cargo/bin` is not on that PATH, every `cargo test` exits
  127, every injection therefore exits non-zero, and the sweep reports a
  **perfect score for a run in which nothing compiled**. It reported 24/24.
  Only the two controls saw it. `ci/chat-commands-falsify.sh` now exports the
  path itself and runs the unmutated tree first, refusing to start on a red
  baseline — which catches this class in one minute rather than forty.

#### The single next item

**Argument nodes, and with them the `minecraft:command_argument_type`
registry.** It is the one thing standing between this registry and a command
that takes a value, and it is deliberately a compile error today rather than a
wrong byte: `Node` has no argument variant. Closing condition: `/gamemode
<mode>` or `/time set <value>` declared in the `commands` packet with its
`ArgumentTypeInfo` payload byte-identical to vanilla's, gated by extending
`tools/goldengen/vanilla/CommandsGolden.java` — which already builds the tree
with vanilla's own `register` methods and will serialise argument nodes with
vanilla's own encoder the moment one is registered, with no change to the
harness.

Second, and independent: **the unknown-command reply**. Vanilla sends two
`system_chat` messages from `Commands.finishParsing` (offsets 28..220) and
this server sends silence. It is the most visible remaining gap to an actual
player, and unlike the arguments it needs no registry — only the Brigadier
exception message and the red-underline/`command.context.here` context
component.

#### Where this stands

* **T0 green**, exit 0, `skipping: no vanilla` count **0**. 435 tests in
  `rustmc-server` (was 431 on `main` + 4 written for the sweep misses), 709 in
  `rustmc-worldgen`, 18 in `tools/oracle-runner`.
* **Workspace tests green**, `--lib --bins`, exit 0 — which is what covers the
  `oracle-runner` changes, that crate being a **binary** crate whose tests a
  `--lib`-only selector does not reach.
* **Falsification 25/28**, three misses, all three fixed and re-confirmed
  caught. Both controls survived. Detail below.
* **Corpus: `declared 58/88 match`** (24 diverge, 6 absent, **0 vacuous, 0
  inert, 0 error**); strict 36/88. One FULL pass, box confirmed clear of
  harnesses immediately before *and* after. `main`'s baseline is **56/86** —
  the denominator is 88 because this branch adds two scenarios, and **the +2
  is exactly those two**: nothing that already existed moved.

  All three chat/command scenarios **match**, and all four pre-registered
  predictions held:

  | scenario | verdict |
  |---|---|
  | `server-answers-a-command` | match — unchanged under the rewrite |
  | `server-answers-list-uuids` | match, first run |
  | `a-player-says-something` | match — **the one written as at risk** |

  Prediction 3 named four ways the chat check could fail (a `globalIndex`
  ahead of ours, a display name reached through `ChatType.bind` rather than
  `formatList`, a chat-type holder that is not 1, a last-seen validator
  refusing an all-zero update). None happened. Prediction 4 — that the
  join-time `entity_event` and `commands` packet move nothing, including the
  two scenarios that observe `entity_event` — held **structurally**: the
  harness settles to quiet before opening its measured window, so join-time
  packets are discarded before recording starts.

  **Both new checks now have a RED-BEFORE, taken 2026-08-07 at the rebase and
  not on the first pass.** The prediction doc named
  `ci/chat-commands-falsify.sh` as the control that establishes these can go
  red; that sweep mutates source and runs `cargo test`, so it controls the
  **unit tests**. A corpus check's ability to fail lives in its projection,
  its `observe` list, its settle window and the runner's step, and the sweep
  touches none of them — so a green first run of a new corpus check was still
  an unfalsified check. `ci/t2-gameplay-subset.sh` over exactly these two,
  against a server built without either behaviour (the `uuids` subnode
  deleted; the serverbound CHAT packet recognised and consumed but not
  handled, so the frame reader cannot desync):

  | arm | 111 | 112 |
  |---|---|---|
  | unmutated | match | match |
  | not implemented | **absent** | **absent** |

  `absent` requires a non-empty reference and an empty subject, so this also
  rules out the vacuous reading: vanilla spoke in both windows and it was our
  silence that scored. Rule for the next branch: **the falsification sweep is
  not a control for a corpus check.**

  The run first reported `FAIL: 2 unacceptable`, which is the ratchet working:
  a new check must state where it starts. Both are declared `match` in
  `ci/checks/gameplay-baseline.toml` and re-adjudication is **GAMEPLAY GREEN,
  exit 0**.

* **The killed first attempt**, for the record — the coordinator stopped it —
  three gameplay corpora were recording at once (this tree,
  `~/rustmc-storagedir`, and the merge gate) and the contention is **wall
  clock, not CPU**: each harness waits out the others' settle timers, so a
  three-way collision runs at about one scenario per ten minutes and none
  finishes. The three together sat at load **~0.02**, which is exactly why the
  rule is *count harnesses, never read the load average*. Vanilla's half had
  reached 57/88 when it was stopped; nothing but the recording is lost.
  `~/grit/logs/chatcmd/corpus.sh` now **waits** for `pgrep -af
  "oracle-runner|t2-gameplay|handoff"` to come back empty, claims
  `/run/ci-busy` — the marker this file already documents and which nothing
  currently checks — and takes one **full** pass. A real lock is being added
  and will supersede the marker.

**The denominator is 88, not `main`'s 86**, because this branch adds
`111-server-answers-list-uuids` and `112-a-player-says-something`. Any number
quoted against 86 is compared with the wrong denominator. Predictions are
pre-registered in `docs/intel/chat-commands-corpus-prediction.md`, committed
before the first run started.

One thing the killed run did establish, and it is not nothing: **vanilla
recorded all three chat/command scenarios** (12, 13 and 14 of 88) without
kicking the client, so the serverbound `chat` body this branch writes —
including the 20-bit bitset and the trailing checksum byte — is accepted by
real vanilla.

#### THE TRAP WORTH CARRYING TO EVERY TREE: rsync -a + cargo = a stale binary

**Found 2026-08-07, and it silently produced a wrong gate result before it was
found.** This is not specific to this branch and the orchestrator should decide
whether `ci/t0-remote.sh` wants the fix.

`rsync -az` implies `-t`, so a shipped file keeps the **laptop's** mtime.
Cargo fingerprints by mtime. If the remote `target/` was last built at a later
wall clock than the local file was last *edited*, cargo decides the source is
older than the artefact, **skips the rebuild**, and the test binary that runs
is the one built from whatever the tree held at that later moment.

The inner loop in `docs/AGENT-BRIEF.md` is exactly
`rsync -az … && ssh … cargo test`, and it is normally safe only because a
local edit is usually newer than the remote build. **A falsification sweep
breaks that invariant by construction**: it builds the tree from *mutated*
sources over and over, right up to the moment it ends. Every rsync-then-test
afterwards ships correct source, skips the rebuild, and measures the last
injected bug.

Here: `players.rs` shipped at mtime `00:47:34` against artefacts from
`00:52:40`; the workspace run and T0 both came back **exit 101** with
`the_chat_index_post_increments_from_zero` failing `left: 1, right: 0` — which
is precisely the pre-increment the sweep had injected minutes earlier. Local
and remote sources were byte-identical to the commit; the binary was not. A
bare `touch` on the remote file made it pass with no source change at all,
which is the whole proof.

**It failed loudly this time and that was luck.** Had the sweep's last
injection been one this branch's tests do not cover, the same stale binary
would have returned *green*.

`~/grit/logs/chatcmd/verify.sh` now touches every shipped `.rs` after the
rsync and takes the full rebuild. For the shared script a cheaper fix exists —
touch only the paths `rsync -i` reports as transferred — and that is a
decision about everyone's inner loop, not this branch's.

Related and separate: `pgrep -f verify.sh` **self-matched** during the cleanup
and reported the script still running after it had exited. Same payout as the
one already in this file's Rule 7 list.

#### Falsification: 25/28, and what the three misses were

Run on the hardened script, green baseline, both controls surviving. All three
misses are fixed and each was re-confirmed caught by the test written for it
(`~/grit/logs/chatcmd/recheck.sh`). Tests 431 -> 435.

| injection | now caught by |
|---|---|
| breadth-first becomes depth-first | `commandtree::tests::numbering_is_breadth_first_not_depth_first` |
| filter stops recursing | `commandtree::tests::a_usable_parent_does_not_carry_an_unusable_child` |
| globalIndex pre-increments | `players::tests::the_chat_index_post_increments_from_zero` |

**Two distinct causes, and the first is the one worth carrying forward.** The
first two misses were fixtures shaped like the production tree — and this
server's command tree is degenerate for both properties: one branching parent,
so BFS and DFS agree; and no usable parent with an unusable child, so the
recursive half of the filter is never asked a question it can answer wrongly.
**The vanilla golden inherits that blindness exactly**, because it gates the
bytes of *our* tree. A property is gated by it only if our three nodes happen
to distinguish it; everything else needs a synthetic fixture, and a fixture
that mirrors production is the one shape guaranteed not to falsify anything.
Written into `commandtree.rs`'s header so the next reader meets it before
adding a test.

The third was simpler and has no lesson beyond itself: `next_chat_index()` had
no caller in any test, because `crate::chat`'s tests pass `global_index` in
explicitly and so gate the wire field thoroughly and the counter not at all.
