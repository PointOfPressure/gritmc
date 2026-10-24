//! Packet ids for [`crate::GAME_VERSION`], transcribed from Mojang's own
//! data-generator report — the single authoritative source:
//!
//! ```text
//! ci/vanilla-server.sh reports
//! oracle-work/vanilla-<ver>/generated/reports/packets.json
//! ```
//!
//! On a version bump: regenerate the report, re-transcribe, done — this
//! module (and rustmc-data's extracted files) are the whole bump surface
//! (VISION.md 48h-SLA property). Only ids the codebase actually consumes
//! are listed; add on demand, always from the report, never from memory.

pub mod handshake {
    pub mod sb {
        pub const INTENTION: i32 = 0x00;
    }
}

pub mod status {
    pub mod sb {
        pub const STATUS_REQUEST: i32 = 0x00;
        pub const PING_REQUEST: i32 = 0x01;
    }
    pub mod cb {
        pub const STATUS_RESPONSE: i32 = 0x00;
        pub const PONG_RESPONSE: i32 = 0x01;
    }
}

pub mod login {
    pub mod sb {
        pub const HELLO: i32 = 0x00;
        /// `minecraft:key` — 1. The client's answer to the clientbound hello:
        /// the RSA-encrypted shared secret and the RSA-encrypted challenge.
        /// Only ever sent by a client an authenticating server challenged.
        pub const KEY: i32 = 0x01;
        pub const LOGIN_ACKNOWLEDGED: i32 = 0x03;
    }
    pub mod cb {
        pub const LOGIN_DISCONNECT: i32 = 0x00;
        /// `minecraft:hello` — **1, clientbound**, and it shares its name with
        /// the serverbound packet at id **0**. The two are different packets
        /// with different bodies and neither id is the other's.
        pub const HELLO: i32 = 0x01;
        pub const LOGIN_FINISHED: i32 = 0x02;
        pub const LOGIN_COMPRESSION: i32 = 0x03;
    }
}

pub mod config {
    pub mod sb {
        /// `minecraft:client_information` — 0. A real client sends this the
        /// instant it enters configuration. Vanilla tolerates its absence
        /// and rustmc does not, so a harness that omits it looks exactly
        /// like a server that never finishes configuring.
        pub const CLIENT_INFORMATION: i32 = 0x00;
        pub const FINISH_CONFIGURATION: i32 = 0x03;
        pub const KEEP_ALIVE: i32 = 0x04;
        pub const SELECT_KNOWN_PACKS: i32 = 0x07;
    }
    pub mod cb {
        pub const DISCONNECT: i32 = 0x02;
        pub const FINISH_CONFIGURATION: i32 = 0x03;
        pub const KEEP_ALIVE: i32 = 0x04;
        pub const SELECT_KNOWN_PACKS: i32 = 0x0E;
    }
}

pub mod play {
    pub mod sb {
        pub const ACCEPT_TELEPORTATION: i32 = 0x00;
        /// `minecraft:chat_ack` — 6. The client's running acknowledgement of
        /// signed messages it has been shown.
        ///
        /// Read and dropped, and that is a conclusion rather than a
        /// shortcut: it feeds only `LastSeenMessagesValidator`, and
        /// `sendPlayerChatMessage` returns at its offset 66 whenever the
        /// message signature is null, so on an offline-mode server nothing
        /// is ever added to the validator for this packet to acknowledge.
        /// See `docs/intel/chat-and-commands.md` §2.
        pub const CHAT_ACK: i32 = 6;
        pub const CHAT_COMMAND: i32 = 0x07;
        /// `minecraft:chat_command_signed` — 8. **Not handled.** The signed
        /// -argument command path; a client with no profile key never sends
        /// it.
        pub const CHAT_COMMAND_SIGNED: i32 = 8;
        /// `minecraft:chat` — 9. A player saying something.
        ///
        /// There was no constant for this at all before 2026-08-06, which is
        /// the mechanical form of "a chat plugin cannot exist": the packet
        /// was not merely unhandled, it was unnameable.
        pub const CHAT: i32 = 9;
        /// `minecraft:chat_session_update` — 10. **Not handled.** A client
        /// offering its Mojang-signed profile key, which an offline-mode
        /// server cannot validate.
        pub const CHAT_SESSION_UPDATE: i32 = 10;
        pub const KEEP_ALIVE: i32 = 0x1C;
        pub const PONG: i32 = 0x2D;
        /// `minecraft:chunk_batch_received` — 11 in the report. One `f32`,
        /// the client's desired chunks per tick.
        ///
        /// This is not optional politeness. `PlayerChunkSender` is
        /// constructed with `maxUnacknowledgedBatches = 1`, and
        /// `sendNextChunks` returns immediately while
        /// `unacknowledgedBatches >= maxUnacknowledgedBatches`. Only
        /// `onChunkBatchReceivedByClient` decrements that counter and raises
        /// the cap to 10. A client that never sends this therefore receives
        /// **one batch of chunks and then nothing, for the rest of the
        /// session**.
        pub const CHUNK_BATCH_RECEIVED: i32 = 11;
        /// `minecraft:player_action` — 41 in the report. Digging, dropping
        /// and the hand swap all arrive here.
        pub const PLAYER_ACTION: i32 = 41;
        /// `minecraft:use_item_on` — 66. Right-clicking a block face.
        pub const USE_ITEM_ON: i32 = 66;
        /// `minecraft:use_item` — 67. Right-clicking nothing in particular.
        pub const USE_ITEM: i32 = 67;
        /// `minecraft:set_creative_mode_slot` — 56. The slot number is an
        /// `InventoryMenu` index, **not** a hotbar index: hotbar slot 0
        /// arrives as 36 (`InventoryMenu.USE_ROW_SLOT_START`).
        pub const SET_CREATIVE_MODE_SLOT: i32 = 56;
        /// `minecraft:set_carried_item` — 53. Which hotbar slot is selected.
        pub const SET_CARRIED_ITEM: i32 = 53;
        pub const MOVE_PLAYER_POS: i32 = 30;
        pub const MOVE_PLAYER_POS_ROT: i32 = 31;
        /// `minecraft:move_player_rot` — 32. The only way the server learns
        /// which way a player faces; `use_item_on` does not carry it.
        pub const MOVE_PLAYER_ROT: i32 = 32;
        pub const MOVE_PLAYER_STATUS_ONLY: i32 = 33;
        /// `minecraft:swing` — 63. The arm animation, and the packet a
        /// left-click sends alongside `player_action`.
        pub const SWING: i32 = 63;
        /// `minecraft:attack` — **1**. Left-clicking an entity, and in 26.2 a
        /// packet of its own.
        ///
        /// # This is the version where attacking stopped being an `interact`
        ///
        /// Every account of this protocol older than 26.2 — and the brief this
        /// was ported under — says a client attacks by sending
        /// `ServerboundInteractPacket` with an `Action` discriminant of
        /// `ATTACK`, against `INTERACT` and `INTERACT_AT`. That is no longer
        /// the shape. `javap -p -c -constants` on the pinned jar:
        ///
        /// ```text
        ///   ServerboundAttackPacket   is a record of ONE field, `int entityId`
        ///     STREAM_CODEC = composite(ByteBufCodecs.VAR_INT, ...)
        ///     handle -> ServerGamePacketListener.handleAttack
        ///
        ///   ServerboundInteractPacket is a record of FOUR fields and has no
        ///     Action at all: entityId, hand, location, usingSecondaryAction
        ///     STREAM_CODEC = composite(VAR_INT, InteractionHand.STREAM_CODEC,
        ///                              Vec3.LP_STREAM_CODEC, BOOL, ...)
        ///     handle -> ServerGamePacketListener.handleInteract
        /// ```
        ///
        /// So the discriminant is gone in both directions: attack has its own
        /// packet, and the old `INTERACT`/`INTERACT_AT` split collapsed into
        /// one packet that always carries a location. A port written from the
        /// remembered shape would have sent a leading action varint into a
        /// codec that reads an entity id, and hit whichever entity happened to
        /// have id 0 or 1 — a wrong target is not a decode error, so nothing
        /// would have refused it.
        ///
        /// The number is vanilla's own report (`generated/reports/packets.json`,
        /// `minecraft:attack` -> `protocol_id` 1), not a count of the class
        /// list.
        pub const ATTACK: i32 = 1;
        /// `minecraft:interact` — 26. Right-clicking an entity. **Not**
        /// attacking one; see [`ATTACK`] for what changed in 26.2 and why the
        /// old comment here said otherwise.
        pub const INTERACT: i32 = 26;
        /// `minecraft:player_input` — 43. One byte of held-key flags, and
        /// where **sneak** lives in 26.2.
        ///
        /// Also derived against the brief rather than from it.
        /// `ServerboundPlayerCommandPacket$Action` is the traditional home of
        /// `PRESS_SHIFT_KEY`/`RELEASE_SHIFT_KEY`, and in 26.2 it has seven
        /// entries and neither of them is there — `<clinit>` puts them in this
        /// order: `STOP_SLEEPING`, `START_SPRINTING`, `STOP_SPRINTING`,
        /// `START_RIDING_JUMP`, `STOP_RIDING_JUMP`, `OPEN_INVENTORY`,
        /// `START_FALL_FLYING`.
        ///
        /// Sneak moved into the client's whole input state.
        /// `net.minecraft.world.entity.player.Input` is a seven-boolean record
        /// whose `STREAM_CODEC` (`Input$1.encode`) folds it into a **single
        /// byte**, and the flag constants are declared on `Input` itself:
        ///
        /// ```text
        ///   FLAG_FORWARD  = 1     FLAG_JUMP   = 16
        ///   FLAG_BACKWARD = 2     FLAG_SHIFT  = 32   <- sneak
        ///   FLAG_LEFT     = 4     FLAG_SPRINT = 64
        ///   FLAG_RIGHT    = 8
        /// ```
        ///
        /// `encode` ors them in that order and calls `writeByte` once;
        /// `decode` is `readByte` and seven `iand`s. So the packet body is one
        /// byte and nothing else, and jumping and sprinting are the same byte
        /// — which is why this constant is named for the packet rather than
        /// for sneak.
        pub const PLAYER_INPUT: i32 = 43;
        /// `minecraft:container_click` — 18. A click inside an open menu, and
        /// the only way anything is ever stored, retrieved or crafted.
        ///
        /// # The field that is not called `ClickType` any more
        ///
        /// Every account older than 26.2 gives this packet a
        /// `ClickType clickType`. `net.minecraft.world.inventory.ClickType`
        /// **is not in the pinned jar at all**; the field is
        /// `ContainerInput containerInput`, a seven-entry enum in the same
        /// package carrying an explicit `int id` and serialised through
        /// `ByteBufCodecs.idMapper(BY_ID, ContainerInput::id)` — a varint of
        /// `id()`, not of the ordinal. The seven ids happen to equal the
        /// ordinals, so this rename cannot be caught by a wrong number; it is
        /// written down because the *mechanism* differs and a comment claiming
        /// "the enum ordinal" would be a false statement about a field that
        /// still worked.
        ///
        /// The whole body, `STREAM_CODEC`'s `composite` in argument order:
        ///
        /// ```text
        ///   containerId     VarInt   (ByteBufCodecs.CONTAINER_ID, which is
        ///                             FriendlyByteBuf.writeContainerId, which
        ///                             is VarInt.write — not a byte)
        ///   stateId         VarInt
        ///   slotNum         i16 BE   (ByteBufCodecs.SHORT -> writeShort)
        ///   buttonNum       i8       (ByteBufCodecs.BYTE  -> writeByte)
        ///   containerInput  VarInt
        ///   changedSlots    VarInt n, then n * { i16 key, HashedStack }
        ///   carriedItem     HashedStack
        /// ```
        ///
        /// `HashedStack` is `ByteBufCodecs.optional(ActualItem.STREAM_CODEC)`,
        /// so empty is a single `false` byte and present is
        /// `true, VarInt item, VarInt count, HashedPatchMap`. It is a **hash**
        /// of the client's belief, not the stack itself: see
        /// `oracle_runner::client`'s `container_click` arm for why sending an
        /// empty belief is what makes the server answer at all.
        pub const CONTAINER_CLICK: i32 = 18;
        /// `minecraft:container_button_click` — 17 in
        /// `generated/reports/packets.json`. `ByteBufCodecs.CONTAINER_ID`
        /// (a varint) then `ByteBufCodecs.VAR_INT` for the button.
        /// `handleContainerButtonClick` drops it unless the id names the open
        /// menu, then calls `clickMenuButton` and broadcasts only on `true`.
        /// The enchanting table's three offers, the stonecutter's recipe list
        /// and the loom's pattern list all arrive here.
        pub const CONTAINER_BUTTON_CLICK: i32 = 17;
        /// `minecraft:rename_item` — 48 in `generated/reports/packets.json`.
        /// One `readUtf()`. `handleRenameItem` acts only when the open menu
        /// is an `AnvilMenu`, through `AnvilMenu.setItemName`.
        pub const RENAME_ITEM: i32 = 48;
        /// `minecraft:container_close` — 19. One field, `containerId`, as a
        /// varint through `writeContainerId`.
        ///
        /// `handleContainerClose` is four instructions and **checks nothing**:
        /// `ensureRunningOnSameThread` then `player.doCloseContainer()`,
        /// unconditionally, whatever id was sent and whether or not a menu is
        /// open. That is what makes it usable as a reset — see
        /// `oracle_runner::record`.
        ///
        /// # The number, twice, because a serverbound id table is easy to get
        /// silently wrong
        ///
        /// Derived first from the registration order in `GameProtocols`'s
        /// serverbound template, which assigns ids by position. **A grep for
        /// `GamePacketTypes.SERVERBOUND_*` alone gives the wrong answer**: nine
        /// of the sixty-nine entries are `CommonPacketTypes` — `client_information`,
        /// `cookie_response`, `custom_payload`, `keep_alive`, `ping_request`,
        /// `pong`, `resource_pack` among them — and omitting them shifts every
        /// later id by up to eight. That partial extraction puts
        /// `container_click` at 17 and this at 18, and 18 is a real packet, so
        /// nothing would have refused it.
        ///
        /// Cross-checked against vanilla's own `generated/reports/packets.json`,
        /// which is the primary source: `minecraft:container_close` ->
        /// `protocol_id` 19. Two derivations agreeing is the standard the
        /// block-id table in `ci/dump-protocol-ids.sh` is held to.
        pub const CONTAINER_CLOSE: i32 = 19;
        /// `minecraft:sign_update` — 61. The text a player typed into a sign.
        ///
        /// Wire form, from the private `FriendlyByteBuf` constructor in
        /// declaration order: `readBlockPos()` — one big-endian **long**, the
        /// packed `x/z/y` form — then `readBoolean()` for `isFrontText`, then
        /// **four** `readUtf(384)` strings. Always four; the array is
        /// `new String[4]` and the loop is unconditional, so a sign with one
        /// line of text still carries three empty strings.
        ///
        /// # Two gates make vanilla answer nothing, and both are silent
        ///
        /// This is the packet where "the green would have been the bug" is
        /// easiest to arrive at, because there are **two** independent ways to
        /// be ignored and neither produces a byte on the wire:
        ///
        ///   * `ServerGamePacketListenerImpl.updateSignText` returns at offset
        ///     56 unless `getBlockEntity(pos) instanceof SignBlockEntity`. A
        ///     server with no sign block entities — this one — is therefore
        ///     indistinguishable from vanilla at any position that is not a
        ///     sign;
        ///   * `SignBlockEntity.updateSignText` returns at offsets 28-42 if
        ///     the sign `isWaxed()`, **or if the sender is not the UUID in
        ///     `getPlayerWhoMayEdit()`**, or if the level is null. The only
        ///     effect is a `LOGGER.warn`; nothing is sent.
        ///
        /// `getPlayerWhoMayEdit` is set by `SignBlock.openTextEdit`, which
        /// `setPlacedBy` calls when a player places a sign — and the same
        /// method sends that player a `ClientboundOpenSignEditorPacket`. So an
        /// edit is only accepted from the placer, and only until the first one
        /// succeeds: `updateSignText` calls `setAllowedPlayerEditor(null)` at
        /// offsets 57-61 before it broadcasts. **A second identical packet is
        /// ignored**, which is what
        /// `ci/checks/gameplay/571` measures and the reason this verb has a
        /// check that can go `match` for a reason other than silence.
        ///
        /// On success the tail is `sendBlockUpdated(pos, state, state, 3)` —
        /// the same state both sides, so the observable is not a block change
        /// but the **`block_entity_data`** the changed position drags with it.
        ///
        /// Cross-checked against `generated/reports/packets.json`, the primary
        /// source: `minecraft:sign_update` -> `protocol_id` 61, agreeing with
        /// the brief.
        pub const SIGN_UPDATE: i32 = 61;
        /// `minecraft:client_command` — 12. Respawn and stats requests.
        pub const CLIENT_COMMAND: i32 = 12;
        /// `minecraft:player_loaded` — 44. An empty record, and the most
        /// consequential empty packet in the protocol.
        ///
        /// `ServerGamePacketListenerImpl` gates **eight** handlers on
        /// `hasClientLoaded()` — movement, block actions, item use among
        /// them — and each one `return`s immediately if the client has not
        /// sent this. There is no error, no acknowledgement and no log line:
        /// the server simply does nothing, which from the client is
        /// indistinguishable from a server that has no such feature.
        ///
        /// A `clientLoadedTimeoutTimer` eventually marks a silent client
        /// loaded anyway, so a harness that omits this does not fail — it
        /// fails *intermittently*, which is worse.
        pub const PLAYER_LOADED: i32 = 44;
    }
    pub mod cb {
        /// `minecraft:block_changed_ack` — 4. The sequence number a
        /// serverbound block action carried, echoed back; it is what ends
        /// the client's own optimistic prediction of that action.
        pub const BLOCK_CHANGED_ACK: i32 = 4;
        /// `minecraft:block_update` — 8. One position, one state.
        pub const BLOCK_UPDATE: i32 = 8;
        /// `minecraft:commands` — **16** in the report. The whole Brigadier
        /// node tree, sent once per player after the permission-level entity
        /// event; see `crate::commandtree`.
        pub const COMMANDS: i32 = 16;
        pub const CHUNK_BATCH_FINISHED: i32 = 0x0B;
        pub const CHUNK_BATCH_START: i32 = 0x0C;
        pub const DISCONNECT: i32 = 0x20;
        pub const DISGUISED_CHAT: i32 = 0x21;
        pub const GAME_EVENT: i32 = 0x26;
        pub const KEEP_ALIVE: i32 = 0x2C;
        pub const FORGET_LEVEL_CHUNK: i32 = 0x25;
        pub const LEVEL_CHUNK_WITH_LIGHT: i32 = 0x2D;
        pub const LOGIN: i32 = 0x31;
        pub const PING: i32 = 0x3D;
        pub const PLAYER_CHAT: i32 = 0x41;
        pub const PLAYER_POSITION: i32 = 0x48;
        pub const SET_CHUNK_CACHE_CENTER: i32 = 0x5E;
        pub const SET_TIME: i32 = 0x71;
        pub const SYSTEM_CHAT: i32 = 0x79;

        // Everything below is consumed by the gameplay oracle
        // (`tools/oracle-runner`) rather than by the server, and is here for
        // the same reason the rest is: a harness that decodes a packet has to
        // agree with the server about which packet it is, and there is one
        // authoritative list.
        /// `minecraft:bundle_delimiter` — 0. Sent before and after a group of
        /// packets a client must apply in one frame;
        /// `ClientboundBundlePacket` is exactly this pair around its
        /// contents, and `ServerEntity.addPairing` sends every entity's
        /// arrival that way.
        pub const BUNDLE_DELIMITER: i32 = 0;
        /// `minecraft:add_entity` — 1. Every entity a client is told about
        /// arrives here, mobs and dropped items alike.
        pub const ADD_ENTITY: i32 = 1;
        pub const ANIMATE: i32 = 2;
        /// `minecraft:block_destruction` — 5. The mining-progress overlay.
        pub const BLOCK_DESTRUCTION: i32 = 5;
        /// `minecraft:block_event` — 7. The note-block/piston/chest channel:
        /// a block asking the client to run an animation.
        pub const BLOCK_EVENT: i32 = 7;
        /// `minecraft:block_entity_data` — 6. A block position, the block
        /// entity type, and an NBT payload. How vanilla tells a client what
        /// is inside a chest, on a sign or in a furnace; this server sends
        /// none of them.
        pub const BLOCK_ENTITY_DATA: i32 = 6;
        pub const CONTAINER_SET_CONTENT: i32 = 18;
        /// `minecraft:container_set_data` — 19. One `(id, value)` pair of a
        /// menu's `dataSlots`, and the **only** container packet whose two
        /// payload fields are 16-bit rather than varints. A furnace has four
        /// of these and a chest has none.
        pub const CONTAINER_SET_DATA: i32 = 19;
        pub const CONTAINER_SET_SLOT: i32 = 20;
        /// `minecraft:set_cursor_item` — 96. The stack on the player's
        /// cursor, and **not** a `container_set_slot`.
        ///
        /// Older protocols carried the cursor as `container_set_slot` with
        /// `containerId = -1` and `slot = -1`; 26.2 gives it its own packet,
        /// and `ServerPlayer`'s `ContainerSynchronizer.sendCarriedChange`
        /// constructs that packet directly. The consequence is not cosmetic:
        /// `sendSlotChange` calls `incrementStateId` and this does **not**, so
        /// a server that answered the cursor on the old channel would run one
        /// state id ahead of vanilla for the rest of that menu's life.
        ///
        /// **The corpus no longer catches it by that offset, and still
        /// catches it — 2026-08-11.** `compare::rebase_session_counters`
        /// rebases `container_set_slot.state` onto the scenario, so a uniform
        /// offset inherited from earlier in the session is discarded by
        /// design. What survives is the thing that actually matters here: the
        /// wrong channel is an **extra `container_set_slot` that vanilla did
        /// not send**, and a packet present on one side and absent on the
        /// other still moves the projection. The gap between two packets on
        /// one menu is compared too. So write the cursor on its own packet
        /// because that is what vanilla does, not because of the counter.
        ///
        /// Id taken from Mojang's own `reports/packets.json`, never from
        /// memory: `container_set_content` 18, `container_set_data` 19,
        /// `container_set_slot` 20, `set_cursor_item` 96.
        pub const SET_CURSOR_ITEM: i32 = 96;
        /// `minecraft:set_player_inventory` — 108. One `Inventory` index and
        /// the stack now in it, on the player's own inventory and **not** on
        /// any menu.
        ///
        /// `Inventory.placeItemBackInInventory` sends one of these per slot it
        /// writes (offsets 95-105), through `createInventoryUpdatePacket`, so
        /// it is the first thing on the wire when a closing menu hands a stack
        /// back. It carries no state id — it is not a menu packet — which is
        /// why it neither reads nor advances the counter that the
        /// `container_set_slot` following it does.
        ///
        /// Id from Mojang's own `reports/packets.json`:
        /// `set_player_inventory` 108.
        pub const SET_PLAYER_INVENTORY: i32 = 108;
        pub const DAMAGE_EVENT: i32 = 25;
        pub const ENTITY_EVENT: i32 = 34;
        pub const ENTITY_POSITION_SYNC: i32 = 35;
        /// `minecraft:explode` — 36. Seven fields in 26.2, three of them new
        /// since 1.21: `radius`, `blockCount` and the `blockParticles`
        /// weighted list. `blockCount` is `ServerExplosion.explode()`'s return
        /// value, the size of the candidate position list **before**
        /// `interactWithBlocks` removes anything, and it goes through
        /// `ByteBufCodecs.INT` — four fixed big-endian bytes, not a varint.
        /// See `rustmc_server::explosion::explode_body`.
        pub const EXPLODE: i32 = 36;
        /// `ClientboundHurtAnimationPacket` — `ServerPlayer.indicateDamage` sends
        /// it to the hurt player alone. `hurt_animation` 42.
        pub const HURT_ANIMATION: i32 = 42;
        pub const LEVEL_EVENT: i32 = 46;
        /// `minecraft:level_particles` — 47. Named here because the explosion
        /// path's particles do **not** go through it: `ServerLevel.explode`
        /// carries them inside the `explode` packet itself (offsets 252-273),
        /// so a server sending both would double them.
        pub const LEVEL_PARTICLES: i32 = 47;
        pub const LIGHT_UPDATE: i32 = 48;
        pub const MOVE_ENTITY_POS: i32 = 53;
        pub const MOVE_ENTITY_POS_ROT: i32 = 54;
        /// `minecraft:move_entity_rot` — **56, not 55**. The three
        /// `ClientboundMoveEntityPacket` subclasses are not contiguous:
        /// `minecraft:move_minecart_along_track` sits between `pos_rot` and
        /// `rot`. Taken from Mojang's own `reports/packets.json`, which is the
        /// only place this ordering is stated.
        pub const MOVE_ENTITY_ROT: i32 = 56;
        /// `minecraft:rotate_head` — 83.
        pub const ROTATE_HEAD: i32 = 83;
        /// `minecraft:player_rotation` — 73. `ServerPlayer.forceSetRotation`
        /// sends it: a yaw, its relative flag, a pitch, its relative flag.
        /// Id from Mojang's own `reports/packets.json`.
        pub const PLAYER_ROTATION: i32 = 73;
        /// `minecraft:boss_event` — 9. `ServerBossEvent`'s add, remove and
        /// update operations. Id from Mojang's own `reports/packets.json`.
        pub const BOSS_EVENT: i32 = 9;
        /// `minecraft:clear_dialog` — 139, an empty body.
        /// `ServerPlayer.closeDialog`. Id from Mojang's own
        /// `reports/packets.json`.
        pub const CLEAR_DIALOG: i32 = 139;
        pub const OPEN_SCREEN: i32 = 59;
        /// `minecraft:open_sign_editor` — 60. Sent to the placing player only,
        /// with the sign's position and which face is being edited.
        pub const OPEN_SIGN_EDITOR: i32 = 60;
        pub const PLAYER_INFO_UPDATE: i32 = 70;
        /// `minecraft:recipe_book_add` — 74. `ServerRecipeBook.addRecipes`,
        /// offsets 115-137: one `ClientboundRecipeBookAddPacket(entries,
        /// false)`, sent only when `entries` is non-empty. Id from Mojang's
        /// own `reports/packets.json`, never from memory.
        pub const RECIPE_BOOK_ADD: i32 = 74;
        /// `minecraft:player_combat_kill` — **68**, and the name is the trap.
        ///
        /// Every informal account of this protocol calls the packet a player
        /// dies to `combat_death`, and there is no such registry entry in
        /// 26.2: `generated/reports/packets.json` has
        /// `minecraft:player_combat_kill` at 68, beside
        /// `minecraft:player_combat_end` (66) and
        /// `minecraft:player_combat_enter` (67), which are three different
        /// packets and only this one carries a death message. The id here is
        /// read out of that report, as every id in this project is.
        ///
        /// Body, from `ClientboundPlayerCombatKillPacket`'s `static {}`
        /// (`javap -p -c -constants`, pinned jar):
        ///
        /// ```text
        ///   0: ByteBufCodecs.VAR_INT                        playerId
        ///   8: ComponentSerialization.TRUSTED_STREAM_CODEC  message
        ///  21: StreamCodec.composite(...)
        /// ```
        ///
        /// So: a varint entity id and an NBT component. `TRUSTED_` matters —
        /// it is the codec that does **not** re-validate the component's
        /// depth, and it is the one the server writes with.
        ///
        /// `isSkippable()` returns `iconst_1`, which is why the client can
        /// drop it during a resource reload without the connection dying.
        pub const PLAYER_COMBAT_KILL: i32 = 68;
        /// `minecraft:player_combat_end` — `ServerPlayer.onLeaveCombat`.
        pub const PLAYER_COMBAT_END: i32 = 66;
        /// `minecraft:player_combat_enter` — `ServerPlayer.onEnterCombat`.
        pub const PLAYER_COMBAT_ENTER: i32 = 67;
        pub const REMOVE_ENTITIES: i32 = 77;
        /// `minecraft:respawn` — **82**. What a respawning player is sent,
        /// and it is not `login`.
        ///
        /// `PlayerList.respawn` builds a whole new `ServerPlayer`, swaps it
        /// onto the same connection, and then sends **this**, never
        /// [`LOGIN`]. Read from the bytecode rather than assumed, because the
        /// new-player-object shape makes a re-login look plausible:
        ///
        /// ```text
        ///   PlayerList.respawn, offsets 249..270
        ///     254: new ClientboundRespawnPacket
        ///     262: ServerPlayer.createCommonSpawnInfo(newLevel)
        ///     265: iload 8                      <- the dataToKeep byte
        ///     267: <init>:(CommonPlayerSpawnInfo;B)V
        ///     270: connection.send(...)
        /// ```
        ///
        /// The body is a `CommonPlayerSpawnInfo` — byte for byte the same
        /// ten-field block the join packet carries inline (`rustmc-server`'s
        /// `play::JoinInfo`) — followed by **one** `writeByte`. There
        /// is no length prefix and no second component; the whole difference
        /// from the tail of a join packet is that trailing byte.
        pub const RESPAWN: i32 = 82;
        /// `minecraft:section_blocks_update` — 84. A tick's worth of changes
        /// in one section, which is what vanilla sends where this server
        /// still sends one `block_update` each.
        pub const SECTION_BLOCKS_UPDATE: i32 = 84;
        /// `minecraft:initialize_border` — 43. The whole border in one
        /// packet; the other five each move one part of it.
        pub const INITIALIZE_BORDER: i32 = 43;
        /// `minecraft:merchant_offers` — 52.
        pub const MERCHANT_OFFERS: i32 = 52;
        /// `minecraft:set_border_center` — 88.
        pub const SET_BORDER_CENTER: i32 = 88;
        /// `minecraft:set_border_lerp_size` — 89.
        pub const SET_BORDER_LERP_SIZE: i32 = 89;
        /// `minecraft:set_border_size` — 90.
        pub const SET_BORDER_SIZE: i32 = 90;
        /// `minecraft:set_border_warning_delay` — 91. Carries `warningTime`,
        /// which is a *time* despite the packet's name, while
        /// `set_border_warning_distance` carries `warningBlocks`. The two are
        /// adjacent ids with swapped-sounding names and nothing on the wire
        /// distinguishes them but the id.
        pub const SET_BORDER_WARNING_DELAY: i32 = 91;
        /// `minecraft:set_border_warning_distance` — 92.
        pub const SET_BORDER_WARNING_DISTANCE: i32 = 92;
        /// `minecraft:reset_score` — 79.
        pub const RESET_SCORE: i32 = 79;
        pub const SET_DEFAULT_SPAWN_POSITION: i32 = 97;
        /// `minecraft:set_display_objective` — 98.
        pub const SET_DISPLAY_OBJECTIVE: i32 = 98;
        pub const SET_ENTITY_DATA: i32 = 99;
        /// `minecraft:set_entity_link` — 100. A lead attached or cut:
        /// `ClientboundSetEntityLinkPacket`, two big-endian ints.
        pub const SET_ENTITY_LINK: i32 = 100;
        /// `minecraft:set_objective` — 106.
        pub const SET_OBJECTIVE: i32 = 106;
        /// `minecraft:set_player_team` — 109.
        pub const SET_PLAYER_TEAM: i32 = 109;
        /// `minecraft:set_score` — 110.
        pub const SET_SCORE: i32 = 110;
        pub const SET_ENTITY_MOTION: i32 = 101;
        pub const SET_EQUIPMENT: i32 = 102;
        /// `minecraft:remove_mob_effect` — 78.
        ///
        /// `ClientboundRemoveMobEffectPacket` is a record whose `STREAM_CODEC`
        /// composes `ByteBufCodecs.VAR_INT` with `MobEffect.STREAM_CODEC`, so
        /// the wire is a varint entity id then a varint effect id and nothing
        /// else. `MobEffect.STREAM_CODEC` is
        /// `ByteBufCodecs.holderRegistry(Registries.MOB_EFFECT)`, which is
        /// `idMapper` — a **plain** `VarInt.write(rawId)`, with no `+1` and no
        /// inline-holder escape. That distinction is load-bearing: the
        /// optional-entity and optional-holder codecs elsewhere in this file
        /// do write `id + 1`, and a port that reaches for the familiar one
        /// shifts every effect by one.
        ///
        /// Id from Mojang's own `reports/packets.json`.
        pub const REMOVE_MOB_EFFECT: i32 = 78;
        pub const SET_EXPERIENCE: i32 = 103;
        pub const SET_HEALTH: i32 = 104;
        pub const SOUND_ENTITY: i32 = 116;
        pub const SOUND: i32 = 117;
        pub const TAB_LIST: i32 = 122;
        pub const TAKE_ITEM_ENTITY: i32 = 124;
        pub const TELEPORT_ENTITY: i32 = 125;
        pub const UPDATE_ADVANCEMENTS: i32 = 130;
        pub const UPDATE_ATTRIBUTES: i32 = 131;
        /// `minecraft:update_mob_effect` — 132.
        ///
        /// `ClientboundUpdateMobEffectPacket.write`, offsets 0..49: a varint
        /// entity id, `MobEffect.STREAM_CODEC` (a bare varint registry id —
        /// see [`REMOVE_MOB_EFFECT`]), a varint amplifier, a varint duration
        /// in ticks, and one **byte** of flags. The flags are built by the
        /// packet's own constructor at offsets 33..93 and are
        /// `AMBIENT = 1`, `VISIBLE = 2`, `SHOW_ICON = 4`, `BLEND = 8`; only
        /// the first three come off the `MobEffectInstance`, and `BLEND` is
        /// the constructor's third argument, which the caller supplies.
        ///
        /// Id from Mojang's own `reports/packets.json`.
        pub const UPDATE_MOB_EFFECT: i32 = 132;
        /// `CLIENTBOUND_WAYPOINT`, `0x8A`.
        ///
        /// Out of `GameProtocols`' own registration order — the clientbound
        /// list runs `… UPDATE_TAGS, PROJECTILE_POWER,
        /// CUSTOM_REPORT_DETAILS, SERVER_LINKS, WAYPOINT, …` — and
        /// corroborated by the recording: every `packet:0x8A` in the gameplay
        /// traces is a locator-bar waypoint, and there are 440 of them across
        /// five scenarios.
        pub const WAYPOINT: i32 = 138;
        /// `minecraft:change_difficulty` — 10. `MinecraftServer.sendDifficultyUpdate`:
        /// the difficulty's varint id and the lock flag. Id from Mojang's own
        /// `reports/packets.json`.
        pub const CHANGE_DIFFICULTY: i32 = 10;
        /// `minecraft:player_abilities` — 64. `ServerPlayer.onUpdateAbilities`:
        /// a flag byte, then the flying and walking speeds. Id from Mojang's
        /// own `reports/packets.json`.
        pub const PLAYER_ABILITIES: i32 = 64;
        /// `minecraft:clear_titles` — 14. One boolean: `resetTimes`.
        pub const CLEAR_TITLES: i32 = 14;
        /// `minecraft:set_titles_animation` — 115. Three big-endian ints:
        /// fade in, stay, fade out. Ids from Mojang's own `reports/packets.json`.
        pub const SET_TITLES_ANIMATION: i32 = 115;
        /// `minecraft:stop_sound` — 119. A flag byte (1: a source follows, 2:
        /// a sound id follows), then those. Id from `reports/packets.json`.
        pub const STOP_SOUND: i32 = 119;
        /// `minecraft:set_title_text` — 114, `set_subtitle_text` — 112,
        /// `set_action_bar_text` — 87: each one component, network form. Ids
        /// from `reports/packets.json`.
        pub const SET_TITLE_TEXT: i32 = 114;
        pub const SET_SUBTITLE_TEXT: i32 = 112;
        pub const SET_ACTION_BAR_TEXT: i32 = 87;
    }
}

pub mod configuration {
    pub mod sb {
        pub const CLIENT_INFORMATION: i32 = 0x00;
        pub const FINISH_CONFIGURATION: i32 = 0x03;
        pub const SELECT_KNOWN_PACKS: i32 = 0x07;
    }
    pub mod cb {
        pub const DISCONNECT: i32 = 0x02;
        pub const FINISH_CONFIGURATION: i32 = 0x03;
        pub const REGISTRY_DATA: i32 = 0x07;
        pub const UPDATE_ENABLED_FEATURES: i32 = 0x0c;
        pub const UPDATE_TAGS: i32 = 0x0d;
        pub const SELECT_KNOWN_PACKS: i32 = 0x0e;
    }
}
