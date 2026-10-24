# Pre-registered predictions for the chat/commands corpus run

**Committed before the run, and deliberately before the numbers exist.** The
brief for this branch requires it, and the reason is this project's own
history: a prediction written after a measurement is a description, and it
cannot be wrong.

Branch `chat-commands`, off `main` at `fbf0ae6` (1,451 commits). Remote dir
`~/rustmc-chatcmd` on the Threadripper.

## Baseline

`main`'s declared corpus result is **56/86**. This branch adds two scenarios,
so the denominator becomes **88** if both are recorded, and any number quoted
against 86 afterwards is a number compared with the wrong denominator.

The two additions:

* `111-server-answers-list-uuids.toml` — `/list uuids`, observed from the
  actor, kind `chat` (a `system_chat`).
* `112-a-player-says-something.toml` — a `say` step, observed from the
  **observer**, kind `player_chat`.

## Prediction 1 — `110-server-answers-a-command` stays exactly as it was

It passed on `main` and this branch rewrote the code path underneath it: the
whole-text `if text != "list"` became a tree walk. The reply bytes are
unchanged and are still gated by
`commands::tests::the_list_reply_is_byte_identical_to_vanillas` against 539
bytes of real vanilla wire.

**Predicted: match.** If this regresses, the tree walk resolves `list`
differently from the old string compare, and that is a defect in
`Tree::resolve`, not in the reply.

## Prediction 2 — `111-server-answers-list-uuids` matches

The reply is gated byte-for-byte against vanilla's own component codec
(`ci/commands-golden.sh`). Nothing in it is time-dependent, order-dependent or
seeded.

**Predicted: match**, first run, no caveats.

The one thing that could falsify it and would *not* be a defect in the reply:
the corpus connects two players and the golden was taken at exactly those two
names in that order. A run in which the two clients join in the other order
would produce a legitimately different component. That is worth stating now
because after the fact it would look like an excuse.

## Prediction 3 — `112-a-player-says-something` matches, and this is the one at risk

This is the prediction I would least like to be asked to defend, so it is the
one written in most detail.

The clientbound `player_chat` body carries `Instant.now()` — vanilla's own
wall clock, substituted by `SignedMessageBody.unsigned` for whatever the
client sent. Two runs of vanilla against itself produce different bytes there.
`tools/oracle-runner`'s `chat` arm consumes nothing, so `rest` digests the
whole body, and under that projection this check **could never pass, for
anybody, including vanilla against vanilla**.

So this branch adds a `player_chat` projection arm that consumes exactly as
far as the timestamp and stops, leaving salt, last-seen, unsigned content,
filter mask, chat-type holder and the sender's display-name component in
`rest`.

**Predicted: match.** But the honest confidence is lower than for 1 and 2, and
these are the ways it can fail, listed before they happen:

1. **`global_index` disagrees.** It is the *recipient's* `nextChatIndex++`. If
   vanilla sends any other `player_chat` to the observer earlier in the
   session — a join message, anything — its counter is ahead of ours. I
   believe it does not, because join/leave notices are `system_chat`, which
   does not touch `nextChatIndex`. **If this is what fails, the finding is
   that something else in vanilla's session emits a `player_chat`, and that is
   worth more than the check.**
2. **The display-name component differs.** It is the same
   `ServerPlayer.getDisplayName()` the `/list` reply embeds, and that one is
   gated against real wire — but `/list`'s is reached through
   `ComponentUtils.formatList` and this one through `ChatType.bind`, and only
   the first is proved by bytes today.
3. **The chat-type holder is not 1.** `minecraft:chat` is entry 0 of
   `minecraft:chat_type` in `data/vanilla/registries.bin` *as this server
   replays it*. If vanilla's live registry orders that registry differently
   from the capture, the holder differs.
4. **Vanilla refuses the message and we accept it, or the reverse.** The
   `say` step sends timestamp 0 and salt 0. Vanilla discards both, so this
   should be inert — but `LastSeenMessagesValidator.applyUpdate` runs before
   anything else and I am predicting it accepts an all-zero update on the
   grounds that `lastSeenCount` is 0. That is derived from bytecode, not
   measured.

**Predicted verdict if it diverges: `diverge`, not `vacuous` and not
`inert`.** The reference is definitely not silent — vanilla broadcasts chat —
so a `vacuous` verdict here would mean the projection is wrong rather than the
port.

## Prediction 4 — nothing else moves

No other scenario touches chat or commands. The changes outside those two
modules are: a new `seed` field on `Session` (read only by `/seed`), a
`chat_visibility` field read during configuration (previously the byte was
skipped), two new `Connection` counters, and the `send_commands` call on join.

**The join-time change is the one that could move an unrelated check.** Every
player now receives two extra packets immediately after joining — an
`entity_event` carrying the permission byte and a `commands` packet. If any
existing scenario's projection observes `entity_event`, its stream now has an
extra observation at the head.

**Predicted: no change to any other scenario.** If one moves, the first thing
to check is whether it observes `entity_event`, and the fix is the scenario's
`observe` list, not the join order — vanilla sends both packets there too.

## What would make me distrust the whole run

If **every** new check passes first time and no existing check moves, that is
the outcome I predicted, and it is also exactly what a run that measured
nothing looks like. The control is the falsification sweep in
`ci/chat-commands-falsify.sh`, which is what establishes that these tests can
go red at all; and for the corpus specifically, `110`'s history — it has
passed on `main` — means a *green* 110 here is only informative alongside a
sweep that shows the path is live.
