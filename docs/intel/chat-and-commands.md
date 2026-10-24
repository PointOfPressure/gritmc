# Chat and the command registry — what the pinned jar actually does

Read from `oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar` with
`javap -p -c -constants`, 2026-08-06. Every offset below is from that dump.
Packet ids are from Mojang's own report,
`oracle-work/vanilla-26.2/generated/reports/packets.json`, never from memory.

**Written before the port, so it cannot be written backwards from whatever the
port turned out to do.**

---

## 0. Ids, from the report

| direction | name | id |
|---|---|---|
| S->C | `minecraft:chat_ack` | 6 |
| S->C | `minecraft:chat_command` | 7 |
| S->C | `minecraft:chat_command_signed` | 8 |
| S->C | `minecraft:chat` | **9** |
| S->C | `minecraft:chat_session_update` | 10 |
| S->C | `minecraft:command_suggestion` | 15 |
| C->S | `minecraft:commands` | **16** |
| C->S | `minecraft:command_suggestions` | 15 |
| C->S | `minecraft:delete_chat` | 31 |
| C->S | `minecraft:disguised_chat` | 33 |
| C->S | `minecraft:player_chat` | **65** |
| C->S | `minecraft:system_chat` | 121 |

`ids.rs` already had `chat_command` 0x07, `disguised_chat` 0x21, `player_chat`
0x41 and `system_chat` 0x79, and all four agree with the report. The
serverbound `chat` id 9 was simply absent — there was no constant for it,
which is the mechanical form of "chat is unrepresentable".

---

## 1. `ServerboundChatPacket` — the wire

`ServerboundChatPacket(FriendlyByteBuf)`, offsets 0..39:

```text
   2: sipush 256      readUtf(256)                    message
   9: readInstant()   -> readLong + Instant.ofEpochMilli   timeStamp
  13: readLong()                                       salt
  22: readNullable(MessageSignature::read)             signature
  33: new LastSeenMessages$Update(buf)                 lastSeenMessages
```

so:

1. string, max **256** chars
2. i64 epoch-millis timestamp
3. i64 salt
4. bool present, then **256 raw bytes** if present
   (`MessageSignature.BYTES = 256`, `read` does `readBytes(new byte[256])`
   with no length prefix)
5. `LastSeenMessages$Update`: varint `offset`, `readFixedBitSet(20)`, i8
   `checksum`

`readFixedBitSet(n)` is `Mth.positiveCeilDiv(n, 8)` bytes, so 20 bits is
**3 bytes**, little-endian in `BitSet.valueOf` order. The trailing `checksum`
byte is easy to miss and is the difference between a frame that ends where the
next one begins and one that does not.

---

## 2. The crypto path is not required, and this is a bytecode fact, not a
## scoping convenience

This is the question the brief asked to be answered explicitly rather than
half-done, so here is the chain, every link checked.

`ServerGamePacketListenerImpl`'s constructor, offsets 127..146:

```text
 127: player.getUUID()
 138: invokedynamic  BSM#0 -> REF_invokeVirtual MinecraftServer.enforceSecureProfile:()Z
 143: SignedMessageChain$Decoder.unsigned(UUID, BooleanSupplier)
 146: putfield signedMessageDecoder
```

`Decoder.unsigned`'s lambda (`SignedMessageChain$Decoder.lambda$unsigned$0`):

```text
   1: BooleanSupplier.getAsBoolean()
   6: ifeq 20
   9..19: throw new DecodeException(MISSING_PROFILE_KEY)
  20: PlayerChatMessage.unsigned(uuid, body.content())
```

— i.e. **when the supplier is false the signature argument is discarded
entirely.** And `DedicatedServer.enforceSecureProfile()`:

```text
   6: properties.enforceSecureProfile   ifeq -> 33 (false)
  13: properties.onlineMode             ifeq -> 33 (false)
  23: services.canValidateProfileKeys() ifeq -> 33 (false)
  29: iconst_1
```

`online-mode=false` alone forces it false. **rustmc is an offline-mode server
— `JoinInfo` sends `online_mode: false` and `enforces_secure_chat: false` —
so vanilla itself never runs a signature check on our configuration.** There
is no partial credit being claimed here and no crypto being stubbed: the
unsigned branch *is* vanilla's behaviour for this server, and porting the
signed branch would port a branch vanilla cannot reach here.

**What that costs, stated plainly.** An online-mode rustmc would need:
`ProfilePublicKey` (Mojang's RSA-2048 Yggdrasil key, its expiry, and the
signature over it), `SignedMessageChain` link chaining, `SignedMessageLink`
index/prev-signature, `MessageSignatureCache` (a 128-slot ring used to shrink
`LastSeenMessages` on the wire), and `LastSeenMessagesValidator`'s pending
tracking. That is a sub-project and it is **not** in this branch. It is
recorded here so nobody reads the port as complete for online mode.

### The two pieces of machinery that go inert with it, and why

`sendPlayerChatMessage`, offsets 60..69:

```text
  60: PlayerChatMessage.signature()
  64: astore_3
  65: aload_3
  66: ifnonnull 70
  69: return          <-- null signature: send the packet, then stop
```

Everything after offset 70 — `MessageSignatureCache.push` and
`LastSeenMessagesValidator.addPending` — is **skipped for every unsigned
message**. So on an offline server:

* `LastSeenMessagesValidator.trackedMessages` never gains an entry, so
  `lastSeenCount` stays 0, so `applyUpdate` accepts any update whose
  `acknowledged.length() <= 0` (offset 23..60 of `applyUpdate`) — which is
  every update a client can send when it has been told about no signed
  messages. The validator is constructed at width **20**
  (ctor offset 34: `bipush 20`).
* `MessageSignatureCache` stays empty, so `LastSeenMessages.pack` produces an
  empty list.

Neither is being skipped because it is hard. Each is *provably* a no-op on
this configuration, and each becomes live the moment online mode does.

---

## 3. `handleChat` — the server path, in order

```text
handleChat:
   5: unpackAndApplyLastSeen(packet.lastSeenMessages())
  13: if empty -> return          (validator threw; the player was disconnected)
  31: tryHandleChat(packet.message(), false, <lambda>)

tryHandleChat:
   1: isChatMessageIllegal(text)  -> disconnect "multiplayer.disconnect.illegal_characters"
  18: if !overlay && player.getChatVisibility() == HIDDEN
        -> send system_chat "chat.disabled.options" RED, return
  64: player.resetLastActionTime()
  72: server.execute(lambda)      <-- hops to the main thread

lambda$handleChat$0:
   9: getSignedMessage(packet, lastSeen)   -> the unsigned decoder above
  16: catch DecodeException -> handleMessageDecodeFailure
  30: filterTextPacket(signedContent)      -> TextFilter.DUMMY on a dedicated server
  50: server.getChatDecorator().decorate(player, msg.decoratedContent())
  72: chatMessageChain.append(...)

lambda$handleChat$1:
   2: msg.withUnsignedContent(decorated)
   9: .filter(mask)
  17: broadcastChatMessage(msg)
```

`isChatMessageIllegal` is a per-char `StringUtil.isAllowedChatCharacter`.

**`ChatDecorator.PLAIN` returns its argument unchanged** (`lambda$static$0` is
`aload_1; areturn`), and `MinecraftServer.getChatDecorator()` returns
`PLAIN` unconditionally. So `decorated == Component.literal(content)`, and
`withUnsignedContent` (offsets 0..21) compares its argument against
`Component.literal(signedContent())` and stores **null** when they are equal.
**`unsignedContent` on the wire is therefore null** — one nullable-absent
byte, not a component. Guessing the other way would add a whole NBT component
to every chat packet.

---

## 4. `SignedMessageBody.unsigned` rewrites the timestamp and salt

```text
SignedMessageBody.unsigned(String):
   5: Instant.now()        <-- NOT the client's timeStamp
   8: lconst_0             <-- salt 0, NOT the client's salt
   9: LastSeenMessages.EMPTY
```

**The client's `timeStamp` and `salt` are read off the wire and thrown
away.** The outgoing packet carries the *server's* wall clock and a zero salt.

This has a direct consequence for any check: **the clientbound chat packet
contains a wall-clock i64 and can never be byte-stable across runs.** A golden
over these bytes has to mask that field or it is measuring `Instant.now()`.
Said here because a digest-the-whole-body projection — which is what
`chat` uses in `tools/oracle-runner` — would otherwise look like a permanent
divergence with no cause.

---

## 5. Which clientbound packet vanilla actually sends

The intuitive answer is wrong, so it is worth stating.

`OutgoingChatMessage.create(msg)` branches on `msg.isSystem()`:

```text
   0: PlayerChatMessage.isSystem()
   4: ifeq 19
   7..18: new OutgoingChatMessage$Disguised(decoratedContent)   -> disguised_chat
  19..27: new OutgoingChatMessage$Player(msg)                   -> player_chat
```

and `isSystem()` is `sender().equals(SYSTEM_SENDER)`. For an unsigned player
message, `PlayerChatMessage.unsigned` builds
`SignedMessageLink.unsigned(uuid)` = `root(uuid, Util.NIL_UUID)` =
`SignedMessageLink(0, uuid, NIL_UUID)` — the **sender is the player**, not the
system sender.

**So an offline server sends `player_chat` (65), with a null signature — not
`disguised_chat`.** `disguised_chat` is for messages whose sender *is* the
system, which is a different feature (`/say`-style output) and is not ported
here.

Two further gates on the recipient side:

* `PlayerList.broadcastChatMessage` iterates `players` **in list order** and
  sends to every player including the speaker; the `Predicate` only decides
  the `filtered` boolean, not whether to send.
* `ServerPlayer.sendChatMessage` sends **only if
  `acceptsChatMessages()`**, which is `chatVisibility == FULL` and nothing
  else (`ChatVisiblity`: FULL=0, SYSTEM=1, HIDDEN=2). A recipient on SYSTEM or
  HIDDEN receives **no packet at all** — not a filtered one.
  `tools/oracle-runner`'s client sends visibility 0, so the corpus only ever
  exercises FULL; the gate is implemented anyway because it is one comparison
  and its absence is invisible until a real client changes the setting.

---

## 6. `ClientboundPlayerChatPacket` — the wire

`write(RegistryFriendlyByteBuf)`, offsets 0..80:

```text
   5: writeVarInt(globalIndex)
  14: writeUUID(sender)                                   16 bytes
  23: writeVarInt(index)
  37: writeNullable(signature, MessageSignature::write)   bool + 256 raw
  45: SignedMessageBody$Packed.write(buf)
  56: writeNullable(unsignedContent, TRUSTED_STREAM_CODEC)
  64: FilterMask.write(buf, filterMask)
  75: ChatType$Bound.STREAM_CODEC.encode(buf, chatType)
```

`SignedMessageBody$Packed.write`:

```text
   8: writeUtf(content, 256)
  17: writeInstant(timeStamp)      i64 millis
  25: writeLong(salt)
  34: LastSeenMessages$Packed.write -> writeCollection(entries, ...)
```

`FilterMask.write` is `writeEnum(type)` — a varint ordinal — plus a bitset
**only** for `PARTIALLY_FILTERED`. Ordinals: `PASS_THROUGH`=0,
`FULLY_FILTERED`=1, `PARTIALLY_FILTERED`=2. Ours is always `PASS_THROUGH`, so
one zero byte.

`ChatType$Bound.STREAM_CODEC` is a 3-field composite:

```text
  ChatType.STREAM_CODEC                              chatType (Holder)
  ComponentSerialization.TRUSTED_STREAM_CODEC        name
  ComponentSerialization.TRUSTED_OPTIONAL_STREAM_CODEC  targetName
```

`ChatType.STREAM_CODEC = ByteBufCodecs.holder(Registries.CHAT_TYPE, ...)`, and
`ByteBufCodecs$30.encode` writes **`id + 1`** for a registry reference and `0`
followed by an inline value for a direct one. `minecraft:chat` is entry **0**
of `minecraft:chat_type` in `data/vanilla/registries.bin`, so the holder is
the single byte **`0x01`**.

`ChatType.bind(CHAT, entity)` sets `name = entity.getDisplayName()` and leaves
`targetName` empty. For a player on no team that display name is the same
styled component `crate::commands` already builds for `/list`.

So the whole packet, for our configuration:

```text
  varint  globalIndex      per-connection counter, from 0, ++ per message sent
  16 B    sender uuid      the speaker's
  varint  index            0 (SignedMessageLink.root)
  0x00    signature        absent
  string  content
  i64     Instant.now()    the SERVER's clock — not stable across runs
  i64     0                salt
  varint  0                lastSeen entries
  0x00    unsignedContent  absent
  0x00    filterMask       PASS_THROUGH
  0x01    chatType holder  minecraft:chat, id 0, +1
  <nbt>   name             sender display name
  0x00    targetName       absent
```

`globalIndex` is `nextChatIndex++` on the **recipient's** connection
(`sendPlayerChatMessage` offsets 5..15), so it counts messages that
connection has been sent, not messages the speaker has sent.

---

## 7. What is NOT covered by the chat half

Named, because a call site reading `handle_chat` would otherwise assume the
opposite of each:

* **signed chat, in every part** — see §2. Online mode is out of scope.
* **`chat_ack` (6)** — the client's periodic acknowledgement. Ignored. It only
  feeds `LastSeenMessagesValidator`, which §2 shows is inert here.
* **`chat_session_update` (10)** — a client with no profile key never sends
  it; accepting one would mean accepting a public key we cannot validate.
* **`chat_command_signed` (8)** — the signed-argument command path.
* **`delete_chat` (31)** and the signature cache it indexes into.
* **chat spam throttling** — `TickThrottler(20, 20 * chatSpamThresholdSeconds)`
  and `detectChatRateSpam`. Not ported; a real deployment wants it and it is
  not a parity-visible behaviour under the corpus.
* **`TextFilter`** — `DUMMY` on a dedicated server anyway, so the filter mask
  is always `PASS_THROUGH` and `isFullyFiltered` is always false.
* **`/say`, `/msg`, `/tell`** and every other chat-type binding except
  `minecraft:chat`.

---

# Part 2 — the command registry

## 8. What was there before

One `if text != "list"` on the whole command text, in `crate::commands`. No
tree, no `commands` packet, no permission level anywhere in the server. The
module said so in its own header, honestly, and listed `list uuids` as a thing
it would answer wrongly if it ever matched by prefix.

## 9. 26.2 replaced the integer op level

Any memory of `hasPermission(int)` is a memory of an older version.
`Commands`' static initialiser:

```text
  16: PermissionCheck$AlwaysPass.INSTANCE          -> LEVEL_ALL
  22: new Require(Permissions.COMMANDS_MODERATOR)  -> LEVEL_MODERATORS
  35: new Require(Permissions.COMMANDS_GAMEMASTER) -> LEVEL_GAMEMASTERS
  48: new Require(Permissions.COMMANDS_ADMIN)      -> LEVEL_ADMINS
  61: new Require(Permissions.COMMANDS_OWNER)      -> LEVEL_OWNERS
```

and each of those four is `new Permission$HasCommandLevel(PermissionLevel.X)`.
`PermissionLevel` is `ALL`=0, `MODERATORS`=1, `GAMEMASTERS`=2, `ADMINS`=3,
`OWNERS`=4. So the five observable levels survive.

**The indirection is load-bearing in exactly one place.** `LEVEL_ALL` is
`AlwaysPass`, *not* `Require(level >= 0)`. `COMMAND_NODE_INSPECTOR.isRestricted`
tests a node's requirement against a `PermissionSet.NO_PERMISSIONS` source, and
`NO_PERMISSIONS` holds nothing at all — so `AlwaysPass` passes and every
`Require` fails. That is why level 0 is the only unrestricted level, and it is
not derivable from thinking of the levels as integers.

`PlayerList.sendPlayerPermissionLevel` offsets 66..86 push 24, 25, 26, 27, 28
in ordinal order into a `ClientboundEntityEventPacket`, which is `writeInt`
(a plain big-endian i32, **not** a varint) then `writeByte`. It then calls
`Commands.sendCommands` — and note the `ifnull 105` guards only the entity
event, so `sendCommands` runs either way.

### The four chat permissions are dead

26.2 also declares `chat/send_messages`, `chat/send_commands`,
`chat/receive_player_messages`, `chat/receive_system_messages`, and
`CHAT_PERMISSIONS` as the set of all four. **No class outside `Permissions`
itself references any of them.**

Swept over all 7,434 `net/minecraft` classes extracted from the pinned jar,
2026-08-06. This is recorded with its instrument history because the
instrument lied twice before it worked:

1. the first sweep used the `ugrep`-aliased `grep`, which **skips binary files
   by default**, and returned zero hits for *every* pattern — including the
   declaring class. Caught by a positive control, not by inspection;
2. the extraction directory turned out to hold a second, stale class tree, so
   counts double-counted until it was wiped and re-extracted from the pinned
   jar (whose top-level entries are `assets`, `com`, `data`, `META-INF`, `net`,
   `version.json` — there is no `core/`).

With `--binary-files=text` on a clean extract, the positive control
(`COMMANDS_ADMIN`) hits `Permissions`, `Player` and `Commands`, and all four
chat permissions hit `Permissions` alone. **Chat is not permission-gated in
26.2.**

## 10. `ClientboundCommandsPacket` — the wire

`write`: `writeCollection(entries, Entry::write)` then `writeVarInt(rootIndex)`.

`Entry.write`:

```text
   5: writeByte(flags)
  14: writeVarIntArray(children)        count, then each index
  22: if (flags & 8) writeVarInt(redirect)
  49: stub.write(buf)                   if the stub is non-null
```

`LiteralNodeStub.write` is `writeUtf(id)`. `ArgumentNodeStub.write` is
`writeUtf(id)`, then `serializeCap`, then an optional `writeIdentifier`.

Flags, from the class's own constants: `MASK_TYPE` 3, `FLAG_EXECUTABLE` 4,
`FLAG_REDIRECT` 8, `FLAG_CUSTOM_SUGGESTIONS` 16, **`FLAG_RESTRICTED` 32**,
`TYPE_ROOT` 0, `TYPE_LITERAL` 1, `TYPE_ARGUMENT` 2.

`createEntry` sets them: redirect -> 8, `isExecutable` -> 4, `isRestricted` ->
32, and then the type by a pattern switch on the node class.

### Numbering

`enumerateNodes` is breadth-first over an `ArrayDeque`: add root, poll, skip if
already numbered, else assign `map.size()`, enqueue `getChildren()` whole, then
the redirect. Brigadier's `CommandNode.children` is a **`LinkedHashMap`**
(`brigadier-1.3.10.jar`, ctor offsets 5..12), so children come out in
*registration* order.

`createEntries` writes each entry at its own index, so the emitted list is in
index order regardless of the `Object2IntOpenHashMap` it was collected from.
Root index is therefore always 0.

### Filtering

`sendCommands` does **not** send the dispatcher's tree. It builds a fresh
`RootCommandNode`, runs `fillUsableCommands(dispatcherRoot, fresh,
player.createCommandSourceStack(), map)`, and builds the packet from the copy.
`fillUsableCommands` recurses into a child's children **only after** `canUse`
accepted that child (offsets 33..39 gate the whole body), so a usable node
under an unusable parent is unreachable and never sent.

So there are two independent mechanisms and a port needs both: the *filter*
decides what is present, and `isRestricted` decides a *flag* on what survived.
They disagree on purpose — a node an owner may use is still flagged restricted,
because the flag is computed against a fixed no-permission source.

## 11. Why the golden is built, not captured

The obvious gate — boot vanilla, record the `commands` packet, require ours to
match — **cannot exist**. Vanilla's dispatcher holds every command in the game;
ours holds three nodes. Because `enumerateNodes` numbers breadth-first over the
whole tree, every index in vanilla's packet depends on commands we do not
implement, and not one of our entries is a byte substring of vanilla's.

Nor can vanilla's capture be parsed our way round: reaching entry *k* means
walking entries 0..*k*-1, and vanilla's tree is dense with argument nodes whose
payload comes from one of ~50 `ArgumentTypeInfo` serialisers. Skipping one
requires implementing it.

`ci/commands-golden.sh` therefore builds a dispatcher holding exactly our
commands — using vanilla's own `ListPlayersCommand.register` and
`SeedCommand.register` — and encodes it with vanilla's own
`ClientboundCommandsPacket` constructor, vanilla's own (private, reflected)
`COMMAND_NODE_INSPECTOR` and vanilla's own `STREAM_CODEC`, after vanilla's own
(private, reflected) `fillUsableCommands`. Nothing in the golden path is
reimplemented, which is the property that makes it a gate rather than a second
opinion.

Measured on the Threadripper, 2026-08-06:

```text
  ALL          ordinal=0 entityEvent=24 len=21  usable=[list, list uuids]
  MODERATORS   ordinal=1 entityEvent=25 len=21  usable=[list, list uuids]
  GAMEMASTERS  ordinal=2 entityEvent=26 len=29  usable=[list, list uuids, seed]
  ADMINS       ordinal=3 entityEvent=27 len=29  usable=[list, list uuids, seed]
  OWNERS       ordinal=4 entityEvent=28 len=29  usable=[list, list uuids, seed]
```

decoded by hand for the two distinct packets:

```text
ALL          03 | 00 01 01 | 05 01 02 04"list" | 05 00 05"uuids" | 00
GAMEMASTERS  04 | 00 02 01 02 | 05 01 03 04"list" | 25 00 04"seed"
                | 05 00 05"uuids" | 00
```

`0x25` is `TYPE_LITERAL | FLAG_EXECUTABLE | FLAG_RESTRICTED`, and `seed` takes
index 2 while `uuids` takes 3 — which is the breadth-first order, and the
single clearest confirmation of it.

**The script refuses a golden whose rows are all equal.** `SeedCommand.register`
takes a `boolean` that, passed wrongly, makes `/seed` level 0 and every row
identical; the dump would still be vanilla's bytes and the Rust test would
still pass, while gating nothing.

## 12. `/seed`'s reply, and the guess the golden killed

`SeedCommand`'s command is `commands.seed.success` around
`ComponentUtils.copyOnClickText(String.valueOf(seed))`, which is a green
literal carrying a `copy_to_clipboard` click, a `chat.copy.click` hover and an
insertion, **wrapped by `wrapInSquareBrackets`** — which is
`Component.translatable("chat.square_brackets", inner)`, a translation and not
two literal characters.

`/list uuids` is `commands.list.players` again, with each element
`commands.list.nameAndId(player.getName(), Component.translationArg(uuid))`.

**Both arguments of `nameAndId` are bare `TAG_String`s on the wire.**
`Player.getName()` is `Component.literal(profile.name())` and
`translationArg(UUID)` is `Component.literal(uuid.toString())`, and an unstyled
literal component serialises as a plain string, not as `{text: "..."}`. The
uuid is therefore the **dashed** `UUID.toString()` form — a different encoding
of the same number from the four-int `TAG_Int_Array` `/list` puts in its hover
event. Both were written the other way first, from a reading of
`ListPlayersCommand`, and the golden corrected them.

## 13. What the command half does NOT cover

* **Argument nodes**, and so the whole `ArgumentTypeInfo` registry. `Node` has
  no argument variant, so this fails to compile rather than mis-serialising.
* **Redirects** and **custom suggestions**, for the same reason.
* **The unknown-command reply.** Vanilla sends *two* `system_chat` messages
  (`Commands.finishParsing` offsets 28..220): Brigadier's own exception message,
  then a context component with the input, a `CommonComponents.ELLIPSIS` when
  the cursor is past 10, the offending remainder in red-underline, and
  `command.context.here` in red-italic — all wrapped by `sendFailure` in
  `Component.empty().append(x).withStyle(RED)`. Not ported; silence is kept,
  and `Refusal` records which of the two reasons applied so the day it is
  ported the caller knows which message it owes.
* **`/op`, `/deop`, `op-permission-level`, reloading, and uuid-keyed op
  matching.** `crate::ops` reads `ops.json` and matches on name, which is exact
  on an offline server and wrong on an online one.
* **Command spam throttling**, the console source, `/execute`, command blocks,
  and `command_suggestion`/`command_suggestions`.
