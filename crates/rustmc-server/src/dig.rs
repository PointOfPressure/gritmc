//! The session side of [`crate::mining`]: reading the digger and the world
//! out of a [`Session`], and turning the state machine's effects into
//! packets and world edits.
//!
//! `crate::mining` is a pure function of its fields so its tests need no
//! socket. This file is everything that does — and `crate::world_send` calls
//! nothing but [`block_break_action`] and [`tick`], so the hot file carries
//! two lines of this mechanism rather than two hundred.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::interact::BlockPos;
use crate::mining::{BlockMining, Dig, Digger, Effect, GameMode, World};
use crate::world_send::Session;
use rustmc_proto::ids;
use rustmc_worldgen::rng::RandomSource;

/// `Level.getMaxY()` for the overworld this server runs: `minY + height - 1`
/// = `-64 + 384 - 1`. `handleBlockBreakAction` offsets 23-29 compare
/// `pos.getY() > maxY`.
pub const MAX_Y: i32 = 319;

/// `Entity.updateFluidOnEyes`'s `0.11111111F`, widened.
const EYE_FLUID_OFFSET: f64 = 0.111_111_11_f32 as f64;

/// The process-wide item table, for the held stack's `tool` component.
fn item_props() -> Option<&'static crate::itemprops::ItemProps> {
    crate::itemprops::ItemProps::shared(std::path::Path::new(crate::itemprops::TABLE_PATH)).ok()
}

/// The enchantment registry, for its numeric ids.
fn enchantments() -> Option<&'static crate::enchantment::Registry> {
    static REG: OnceLock<Option<crate::enchantment::Registry>> = OnceLock::new();
    REG.get_or_init(|| {
        crate::enchantment::Registry::load(std::path::Path::new(crate::enchantment::TABLE_PATH))
            .ok()
    })
    .as_ref()
}

/// `Attributes.MINING_EFFICIENCY` for the held stack: Efficiency's
/// `minecraft:attributes` effect is `add_value` of `levels_squared + 1`
/// (`data/vanilla/enchantment.json`), applied while the stack is in the main
/// hand. The attribute's base is 0, so the value is `L*L + 1`, or 0.
fn mining_efficiency(stack: &crate::item::ItemStack) -> f64 {
    let level = stack.enchantment_level("efficiency");
    if level > 0 {
        f64::from(level) * f64::from(level) + 1.0
    } else {
        0.0
    }
}

/// What the digger is holding and standing in, for one block state.
fn digger(s: &Session<'_>, state: &str) -> Digger {
    let block = state.split('[').next().unwrap_or(state);
    let held = s.hotbar.held();
    let stack = s.hotbar.stack(s.hotbar.selected());
    let (tool_speed, item_correct_for_drops) = match (held, item_props()) {
        (Some(item), Some(p)) => (
            p.mining_speed(item, block),
            p.correct_tool_for_drops(item, block),
        ),
        // `Item.getDestroySpeed` with no `tool` component is `fconst_1`,
        // and `isCorrectToolForDrops` is `false` — bare hands.
        _ => (1.0, false),
    };
    Digger {
        tool_speed,
        item_correct_for_drops,
        mining_efficiency: mining_efficiency(&stack),
        on_ground: s.player.on_ground,
        eye_in_water: eye_in_water(s),
        ..Digger::default()
    }
}

/// `isEyeInFluid(FluidTags.WATER)` — `Entity.updateFluidOnEyes`:
///
/// ```text
///   d  = getEyeY() - 0.11111111F
///   p  = BlockPos.containing(getX(), d, getZ())
///   fs = level.getFluidState(p)
///   if (p.getY() + fs.getHeight(level, p) > d) fluidOnEyes += fs.tags
/// ```
///
/// `FluidState.getHeight` is 1.0 under the same fluid and `amount / 9`
/// otherwise; a source and a falling column are amount 8, a flowing level
/// `l` is `8 - l`.
fn eye_in_water(s: &Session<'_>) -> bool {
    let Some(blocks) = s.blocks else {
        return false;
    };
    let d = s.player.y + crate::dropitem::PLAYER_EYE_HEIGHT - EYE_FLUID_OFFSET;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a position inside the world's extent"
    )]
    let p = (
        s.player.x.floor() as i32,
        d.floor() as i32,
        s.player.z.floor() as i32,
    );
    if !blocks.water_at(p) {
        return false;
    }
    let height = if blocks.water_at((p.0, p.1 + 1, p.2)) {
        1.0
    } else {
        let legacy = blocks.legacy_fluid_at(p);
        let level: i32 = crate::placement::property_of(&legacy, "level")
            .and_then(|l| l.parse().ok())
            .unwrap_or(0);
        let amount = if level == 0 || level >= 8 {
            8
        } else {
            8 - level
        };
        f64::from(amount as f32 / 9.0)
    };
    f64::from(p.1) + height > d
}

/// The world as the state machine reads it, through one session.
struct SessionWorld<'a, 'b> {
    s: &'a Session<'b>,
    table: Option<&'static BlockMining>,
}

impl World for SessionWorld<'_, '_> {
    fn is_air(&self, pos: BlockPos) -> bool {
        let Some(blocks) = self.s.blocks else {
            return true;
        };
        is_air(&blocks.state_at(pos))
    }

    fn progress(&self, pos: BlockPos) -> f32 {
        let Some(blocks) = self.s.blocks else {
            return 0.0;
        };
        let state = blocks.state_at(pos);
        match self.table.and_then(|t| t.row(&state)) {
            Some(row) => crate::mining::destroy_progress(row, &digger(self.s, &state)),
            // No extract: every block breaks on the first hit, which is what
            // this server did in survival before the table existed.
            None => f32::INFINITY,
        }
    }
}

fn is_air(state: &str) -> bool {
    matches!(
        state.split('[').next().unwrap_or(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// `ServerPlayerGameMode.handleBlockBreakAction`, from the top: the range and
/// height refusals, then the state machine, then its effects in order.
///
/// The caller acknowledges the sequence afterwards, exactly as
/// `handlePlayerAction` does after this call returns.
pub fn block_break_action(s: &mut Session<'_>, gm: &mut GameMode, action: Dig, pos: BlockPos) {
    // 0-22: `isWithinBlockInteractionRange(pos, 1.0)`, and a refusal sends
    // nothing at all.
    let eye = (
        s.player.x,
        s.player.y + crate::dropitem::PLAYER_EYE_HEIGHT,
        s.player.z,
    );
    if !crate::mining::within_interaction_range(
        pos,
        eye,
        crate::containers::BLOCK_INTERACTION_RANGE,
    ) {
        return;
    }
    // 23-68: above the build limit, resend what is there.
    if pos.1 > MAX_Y {
        resend_block(s, pos);
        return;
    }
    let effects = {
        let world = SessionWorld {
            s,
            table: BlockMining::shared(),
        };
        gm.handle(action, pos, s.creative, &world)
    };
    apply(s, &effects);
}

/// `ServerPlayerGameMode.tick()`, once per game tick of this player.
pub fn tick(s: &mut Session<'_>, gm: &mut GameMode) {
    let effects = {
        let world = SessionWorld {
            s,
            table: BlockMining::shared(),
        };
        gm.tick(&world)
    };
    let destroyed = effects.iter().any(|e| matches!(e, Effect::Destroy(_)));
    apply(s, &effects);
    // A destroy from the tick happens inside `ServerPlayer.tick`, and that
    // same tick's `containerMenu.broadcastChanges()` follows it at once — so
    // the worn tool's slot goes out after the drop's pairing bundle and
    // BEFORE the block_update, which the chunk broadcasts on the next level
    // tick. Vanilla's 047 recorded exactly that order; a STOP-path destroy
    // (048, 050) leaves the slot for the end-of-tick sweep instead.
    if destroyed {
        s.conn.flush_menu_sweep();
    }
}

fn apply(s: &mut Session<'_>, effects: &[Effect]) {
    for e in effects {
        match *e {
            Effect::ResendBlock(pos) => resend_block(s, pos),
            Effect::Attack(pos) => attack(s, pos),
            Effect::Progress(pos, stage) => progress(s, pos, stage),
            Effect::DestroyAndAck(pos) => {
                if !destroy_block(s, pos) {
                    resend_block(s, pos);
                }
            }
            Effect::Destroy(pos) => {
                destroy_block(s, pos);
            }
        }
    }
}

/// `BlockState.attack` for the three blocks that override it.
///
/// * `NoteBlock.attack` offsets 0-25: `playNote` — the same block event a
///   right-click queues, without the note cycle.
/// * `RedStoneOreBlock.attack` -> `interact`: `setBlock(pos, lit=true, 3)`
///   when it is not lit; the particles are client-side.
/// * `DragonEggBlock.attack` -> `teleport`: see [`dragon_egg_teleport`].
fn attack(s: &Session<'_>, pos: BlockPos) {
    let Some(blocks) = s.blocks else { return };
    let state = blocks.state_at(pos);
    match block_of(&state) {
        "minecraft:note_block" => crate::world_send::queue_note_block_event(s, pos, &state),
        "minecraft:redstone_ore" | "minecraft:deepslate_redstone_ore"
            if crate::placement::property_of(&state, "lit") == Some("false") =>
        {
            let lit = state.replace("lit=false", "lit=true");
            let mut updates = crate::shapes::Effects::new();
            crate::shapes::set_block(
                blocks,
                s.shapes,
                pos,
                &lit,
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                &mut updates,
            );
            crate::world_send::record_updates(s, &updates);
        }
        "minecraft:dragon_egg" => dragon_egg_teleport(s, pos, &state),
        _ => {}
    }
}

/// `DragonEggBlock.teleport(state, level, pos)`, server side: find a spot
/// (`crate::mining::dragon_egg_target`), then offsets 333-345 —
/// `level.setBlock(target, state, 2)` and `level.removeBlock(pos, false)`.
/// The 128 portal particles are the client's (`isClientSide` arm).
///
/// `level.getRandom()` is drawn as every level-random draw in this server
/// is, from a fresh `crate::dropitem::unique_random`, so where the egg lands
/// differs run to run — as it does in vanilla, against a different stream.
/// The session's world is the overworld: build height `-64..=319`.
fn dragon_egg_teleport(s: &Session<'_>, pos: BlockPos, state: &str) {
    let Some(blocks) = s.blocks else { return };
    let overworld = crate::dimension::Dimension::Overworld;
    let (min_y, max_y) = (
        overworld.min_y(),
        overworld.min_y() + overworld.height() - 1,
    );
    let border = crate::worldborder::shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let target =
        crate::mining::dragon_egg_target(pos, &mut crate::dropitem::unique_random(), |t| {
            is_air(&blocks.state_at(t))
                && !is_air(&blocks.state_at((t.0, t.1 - 1, t.2)))
                // `WorldBorder.isWithinBounds(BlockPos)`: `(double) x`, `(double) z`.
                && border.is_within_bounds(f64::from(t.0), f64::from(t.2))
                // `isInsideBuildHeight`: `y >= getMinY() && y <= getMaxY()`.
                && (min_y..=max_y).contains(&t.1)
        });
    let Some(target) = target else { return };
    let mut updates = crate::shapes::Effects::new();
    crate::shapes::set_block(
        blocks,
        s.shapes,
        target,
        state,
        crate::shapes::UPDATE_CLIENTS,
        crate::shapes::RECURSION_LIMIT,
        &mut updates,
    );
    crate::shapes::remove_block(blocks, s.shapes, pos, &mut updates);
    crate::world_send::record_updates(s, &updates);
}

/// `connection.send(new ClientboundBlockUpdatePacket(pos, level.getBlockState(pos)))`
/// — straight to the digger, not through the chunk's change set.
fn resend_block(s: &Session<'_>, pos: BlockPos) {
    let Some(blocks) = s.blocks else { return };
    let state = blocks.state_at(pos);
    let Some(id) = blocks.state_id(&state) else {
        return;
    };
    if let Ok(body) = crate::interact::block_update_body(pos, id) {
        let _ = s.conn.send(ids::play::cb::BLOCK_UPDATE, &body);
    }
}

/// `ServerLevel.destroyBlockProgress(breakerId, pos, stage)`: every player
/// but the breaker, within `dx²+dy²+dz² < 1024` of the block's corner.
fn progress(s: &Session<'_>, pos: BlockPos, stage: i32) {
    let Ok(body) = crate::mining::block_destruction_body(s.conn.entity_id(), pos, stage) else {
        return;
    };
    crate::playerlist::broadcast_within(
        &s.players.connections(),
        &crate::playerlist::entity_locator(Some(crate::entities::Entities::shared())),
        Some(s.conn.id()),
        (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
        crate::mining::PROGRESS_RANGE_SQR.sqrt(),
        ids::play::cb::BLOCK_DESTRUCTION,
        &body,
    );
}

/// `ServerPlayerGameMode.destroyBlock(pos)`, offsets 0-273.
///
/// ```text
///     0-35   !mainHand.canDestroyBlock(state, level, pos, player) -> false
///    51-82   GameMasterBlock && !canUseGameMasterBlocks
///              -> sendBlockUpdated(pos, state, state, 3); false
///   104-119  state = block.playerWillDestroy(level, pos, state, player)
///   121-130  removed = level.removeBlock(pos, false)
///   173-186  removed -> block.destroy(level, pos, state)          (a no-op
///                                                                   for every
///                                                                   block here)
///   189-200  preventsBlockDrops()  -> true                        (creative)
///   201-241  mainHand.mineBlock(...)                              (durability)
///   244-269  removed && hasCorrectToolForDrops -> block.playerDestroy(...)
/// ```
///
/// Everything below `removeBlock` is the survival tail: durability, then the
/// drop.
pub fn destroy_block(s: &mut Session<'_>, pos: BlockPos) -> bool {
    let Some(blocks) = s.blocks else {
        return false;
    };
    let state = blocks.state_at(pos);
    if !can_destroy_block(s.hotbar.held(), s.creative) {
        return false;
    }
    let row = BlockMining::shared().and_then(|t| t.row(&state));
    // `canUseGameMasterBlocks` is `abilities.instabuild && permissionLevel >= 2`;
    // the harness's players are not operators, so a game-master block always
    // refuses here. `sendBlockUpdated(pos, state, state, 3)` marks the
    // position changed, which is a `block_update` to every tracker.
    if row.is_some_and(|r| r.game_master)
        && !(s.creative
            && crate::ops::level_for(s.conn.name()) >= crate::commandtree::Level::Gamemasters)
    {
        s.changed.record(pos);
        return false;
    }
    // **THE PLUGIN HOOK.** Before the world changes, and that position is
    // the whole of its value: a plugin asked afterwards could only log,
    // never refuse. `grit-plugins/docs/CONTRACT.md` §7 named this branch as
    // the one server-side change the plugin contract needed. A refusal is
    // `destroyBlock` returning false, so `destroyAndAck` resends the block
    // and the client's prediction that it is gone is corrected — what a
    // vanilla refusal does, and more than the old handler, which wrote
    // nothing and left the client showing a hole.
    if crate::plugins::block_break_denied(s, pos).is_some() {
        return false;
    }
    let Some(blocks) = s.blocks else {
        return false;
    };
    // `block.playerWillDestroy(level, pos, state, player)`, offsets 104-119,
    // in vanilla's own order: every override runs its body first and calls
    // `super` — `Block.playerWillDestroy`, whose `spawnDestroyParticles` is
    // the dug block's 2001 — last. So what an override writes or announces
    // leaves before that 2001, and both leave before `removeBlock`.
    let mut will = crate::shapes::Effects::new();
    // `DoublePlantBlock.playerWillDestroy`, offsets 0-40: a survival digger's
    // double plant drops HERE, before `super` announces it and before
    // `removeBlock` — and so before the other half's cascade, whose
    // `destroyBlock` is what a dug upper half's item comes from.
    if crate::mining::will_destroy_drops(row, s.creative) {
        let tool = s.hotbar.stack(s.hotbar.selected());
        drop_resources(s, pos, &state, &tool);
    }
    match crate::mining::will_destroy_class(row) {
        // `BaseFireBlock.playerWillDestroy` offsets 0-14:
        // `level.levelEvent(null, 1009, pos, 0)` — the extinguish sound, and
        // `null`, so the digger hears it too.
        Some("BaseFireBlock") => will.level_events.push((1009, pos, 0)),
        // `PistonHeadBlock.playerWillDestroy` offsets 0-55: a player whose
        // drops are prevented takes the base with the head —
        // `level.destroyBlock(base, false)` when `isFittingBase` — so the
        // removal's own `destroyBlock(base, true)` finds air and drops nothing.
        Some("piston.PistonHeadBlock") if s.creative => {
            if let Some(facing) = crate::placement::property_of(&state, "facing")
                .and_then(crate::interact::Face::from_name)
            {
                let base = facing.opposite().relative(pos);
                if crate::removal::is_fitting_base(&state, &blocks.state_at(base)) {
                    crate::shapes::destroy_block(
                        blocks,
                        s.shapes,
                        base,
                        false,
                        crate::shapes::RECURSION_LIMIT,
                        &mut will,
                    );
                }
            }
        }
        _ => {}
    }
    // `ServerPlayerGameMode.destroyBlock` is two calls in this order:
    // `playerWillDestroy` and *then* `Level.removeBlock`. The order is
    // load-bearing rather than tidy — `preventDropFromBottomPart` reads the
    // block it is about to clear and matches it against the one being
    // broken, so run after the removal it would be looking at air.
    if s.creative {
        // The bed and double-plant arms are `preventsBlockDrops()`-gated:
        // `BedBlock.playerWillDestroy` offsets 0-12 and
        // `DoublePlantBlock.playerWillDestroy`'s `preventDropFromBottomPart`
        // branch run only for a creative player. A survival player's other
        // half goes through the shape cascade instead.
        crate::shapes::player_will_destroy(blocks, s.shapes, pos, &mut will);
    }
    crate::world_send::record_updates(s, &will);
    // `super.playerWillDestroy` -> `spawnDestroyParticles`: `levelEvent(player,
    // 2001, pos, Block.getId(state))`, the four-argument overload, so
    // `PlayerList.broadcast` skips the digger. Every other listener within 64
    // blocks of the corner is told.
    if !is_air(&state) {
        if let Some(id) = blocks.state_id(&state) {
            if let Ok(body) = crate::interact::level_event_body(2001, pos, id, false) {
                crate::playerlist::broadcast_within(
                    &s.players.connections(),
                    &crate::playerlist::entity_locator(Some(crate::entities::Entities::shared())),
                    Some(s.conn.id()),
                    (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                    crate::playerlist::LEVEL_EVENT_RANGE,
                    ids::play::cb::LEVEL_EVENT,
                    &body,
                );
            }
        }
    }
    let mut updates = crate::shapes::Effects::new();
    // `Level.removeBlock`, not "write air": the block that goes in is
    // whatever fluid was there, and everything the removal knocks down comes
    // back in `updates` with it — see `crate::shapes`.
    let removed = crate::shapes::remove_block(blocks, s.shapes, pos, &mut updates);
    crate::world_send::record_updates(s, &updates);
    // 189-200: `preventsBlockDrops()` is `abilities.instabuild`.
    if s.creative {
        return true;
    }
    // 201-226: the held stack is copied, and `hasCorrectToolForDrops` is
    // asked, BEFORE `mineBlock` — so a tool that breaks on this very block
    // still drops it, and the loot roll sees the tool as it was.
    let tool = s.hotbar.stack(s.hotbar.selected());
    let correct = row.is_none_or(|r| {
        let item_correct = match (tool.is_empty(), item_props()) {
            (false, Some(p)) => p.correct_tool_for_drops(tool.item(), block_of(&state)),
            _ => false,
        };
        crate::mining::has_correct_tool_for_drops(r, item_correct)
    });
    // 228-241: `mainHand.mineBlock(level, state, pos, player)`.
    mine_block(s, row.map_or(0.0, |r| r.destroy_time));
    // 244-269: `removed && correct -> block.playerDestroy(...)`, whose default
    // body is `awardStat`, `causeFoodExhaustion(0.005F)` — neither puts a
    // packet on the wire here — and `dropResources`.
    if removed && correct {
        player_destroy(s, pos, &state, row, &tool);
    }
    true
}

/// `block.playerDestroy(level, player, pos, state, be, tool)` — the
/// default and the overrides `crate::mining::PlayerDestroy` names.
fn player_destroy(
    s: &mut Session<'_>,
    pos: BlockPos,
    state: &str,
    row: Option<&crate::mining::Row>,
    tool: &crate::item::ItemStack,
) {
    use crate::mining::PlayerDestroy as P;
    match crate::mining::player_destroy(row) {
        P::Default | P::Beehive => drop_resources(s, pos, state, tool),
        // The air table: nothing, and no draw.
        P::DoublePlant => {}
        P::Ice => {
            drop_resources(s, pos, state, tool);
            ice_melt(s, pos, tool);
        }
        P::TurtleEgg => {
            drop_resources(s, pos, state, tool);
            decrease_eggs(s, pos, state);
        }
    }
}

/// `EnchantmentHelper.hasTag(stack, tag)`: any enchantment on the stack is a
/// member of the tag, nested `#` references followed.
fn has_enchantment_tag(stack: &crate::item::ItemStack, tag: &str) -> bool {
    fn member(reg: &crate::enchantment::Registry, tag: &str, id: &str, depth: u8) -> bool {
        depth < 8
            && reg.tag(tag).is_some_and(|values| {
                values.iter().any(|v| match v.strip_prefix('#') {
                    Some(inner) => member(reg, inner, id, depth + 1),
                    None => v == id,
                })
            })
    }
    let Some(reg) = enchantments() else {
        return false;
    };
    // A stack carries its enchantment ids bare (`crate::item::ItemStack`);
    // the tag's members are namespaced.
    stack
        .enchantments()
        .iter()
        .any(|(id, level)| *level > 0 && member(reg, tag, &crate::item_damage::namespaced(id), 0))
}

/// `IceBlock.playerDestroy`, offsets 13-87, after its `super` drop.
///
/// `WATER_EVAPORATES` is the overworld's `false`: a session's `blocks` is
/// the overworld's block world — the only one this server digs in.
fn ice_melt(s: &mut Session<'_>, pos: BlockPos, tool: &crate::item::ItemStack) {
    let Some(blocks) = s.blocks else { return };
    let below = blocks.state_at((pos.0, pos.1 - 1, pos.2));
    let blocks_motion = crate::fallable::props()
        .and_then(|p| p.get(&below))
        .is_some_and(|p| p.blocks_motion);
    let after = crate::mining::ice_after_destroy(
        has_enchantment_tag(tool, "minecraft:prevents_ice_melting"),
        false,
        blocks_motion,
        crate::mining::is_liquid_block(block_of(&below)),
    );
    let mut updates = crate::shapes::Effects::new();
    match after {
        crate::mining::IceAfter::Nothing => return,
        crate::mining::IceAfter::Remove => {
            crate::shapes::remove_block(blocks, s.shapes, pos, &mut updates);
        }
        crate::mining::IceAfter::Water => {
            crate::shapes::set_block(
                blocks,
                s.shapes,
                pos,
                crate::placement::WATER_SOURCE,
                crate::shapes::UPDATE_ALL,
                crate::shapes::RECURSION_LIMIT,
                &mut updates,
            );
        }
    }
    crate::world_send::record_updates(s, &updates);
}

/// `TurtleEggBlock.decreaseEggs(level, pos, state)` with the dug state.
fn decrease_eggs(s: &mut Session<'_>, pos: BlockPos, state: &str) {
    let Some(blocks) = s.blocks else { return };
    // 0-26: `level.playSound(null, pos, TURTLE_EGG_BREAK, BLOCKS, 0.7F,
    // 0.9F + level.getRandom().nextFloat() * 0.2F)` — `null`, so the digger
    // hears it too.
    let pitch = 0.9 + crate::dropitem::unique_random().next_f32() * 0.2;
    let sound = crate::sounds::Sound {
        event: "minecraft:entity.turtle.egg_break".into(),
        source: crate::sounds::SOURCE_BLOCKS,
        at: crate::sounds::Eighths::block(pos),
        pos: crate::sounds::block_centre(pos),
        volume: crate::mining::TURTLE_EGG_BREAK_VOLUME,
        pitch,
        except: None,
    };
    crate::sounds::send(
        s.players,
        s.protocol_ids,
        s.sound_events,
        Some(crate::entities::Entities::shared()),
        &[sound],
    );
    let eggs = crate::placement::property_of(state, "eggs")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(1);
    let Some(left) = crate::mining::turtle_eggs_after_destroy(eggs) else {
        // `level.destroyBlock(pos, false)` on the air `removeBlock` left:
        // `isAir` returns false at its offset 12, before anything is sent.
        return;
    };
    let mut updates = crate::shapes::Effects::new();
    let fewer = state.replace(&format!("eggs={eggs}"), &format!("eggs={left}"));
    crate::shapes::set_block(
        blocks,
        s.shapes,
        pos,
        &fewer,
        crate::shapes::UPDATE_CLIENTS,
        crate::shapes::RECURSION_LIMIT,
        &mut updates,
    );
    // 97-105: `levelEvent(2001, pos, Block.getId(state))`, the three-argument
    // overload, with the dug state's id.
    if let Some(id) = blocks.state_id(state) {
        updates.level_events.push((2001, pos, id));
    }
    crate::world_send::record_updates(s, &updates);
}

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `ItemStack.mineBlock` -> `Item.mineBlock`, offsets 0-61: a stack with a
/// `tool` component takes `damagePerBlock` through `hurtAndBreak` unless the
/// block's destroy time is 0. See `crate::durability` for the arithmetic.
///
/// The break is `onEquippedItemBroken` -> `broadcastEntityEvent(this, 47)`,
/// which is `sendToTrackingPlayersAndSelf`: the digger is told too. The slot
/// is `broadcastChanges`'s one `container_set_slot`.
fn mine_block(s: &mut Session<'_>, destroy_time: f32) {
    let slot = s.hotbar.selected();
    let stack = s.hotbar.stack(slot);
    if stack.is_empty() {
        return;
    }
    let props = item_props();
    let Some(amount) = crate::durability::mine_block_damage(props, stack.item(), destroy_time)
    else {
        return;
    };
    // `processDurabilityChange`'s enchantment step — Unbreaking.
    let unbreaking = |a: i32| match (enchantments(), item_tags()) {
        (Some(reg), Some(tags)) => crate::item_damage::process_durability_change(
            reg,
            &stack.enchantments(),
            &crate::item_damage::Requirements {
                item: stack.item(),
                item_tags: tags,
            },
            a,
            &mut crate::dropitem::unique_random(),
        ),
        _ => a,
    };
    match crate::durability::hurt_and_break_with(
        props,
        stack.item(),
        stack.damage(),
        amount,
        s.creative,
        unbreaking,
    ) {
        crate::durability::Hurt::Nothing => {}
        crate::durability::Hurt::Damaged { damage } => {
            s.hotbar.set_stack(slot, stack.with_damage(damage));
            send_inventory_slot(s, slot);
        }
        crate::durability::Hurt::Broken {
            damage,
            entity_event,
        } => {
            let mut left = stack.with_damage(damage);
            left.shrink(1);
            s.hotbar.set_stack(slot, left);
            let id = s.conn.entity_id();
            let body = crate::durability::entity_event_body(id, entity_event);
            let _ = s.conn.send(ids::play::cb::ENTITY_EVENT, &body);
            let entities = crate::entities::Entities::shared();
            crate::tracker::Tracker::shared().send_to_tracking_players(
                id,
                &crate::status::tracked_players(s.players, entities),
                ids::play::cb::ENTITY_EVENT,
                &body,
            );
            send_inventory_slot(s, slot);
        }
    }
}

/// The loot interpreter over the three extracts `ci/loot-golden.sh` writes.
fn loot() -> Option<&'static rustmc_loot::Loot> {
    static LOOT: OnceLock<Option<rustmc_loot::Loot>> = OnceLock::new();
    LOOT.get_or_init(|| {
        let read = |p: &str| -> Option<serde_json::Value> {
            let text = std::fs::read_to_string(p)
                .map_err(|e| eprintln!("no {p} ({e}) — survival digs drop nothing"))
                .ok()?;
            serde_json::from_str(&text).ok()
        };
        let tables: BTreeMap<String, serde_json::Value> =
            serde_json::from_value(read("data/vanilla/loot_tables.json")?).ok()?;
        let tags = item_tags()?;
        let blocks: BTreeMap<String, Option<String>> =
            serde_json::from_value(read("data/vanilla/block_loot_map.json")?).ok()?;
        Some(rustmc_loot::Loot::new(
            std::sync::Arc::new(tables),
            tags,
            blocks,
        ))
    })
    .as_ref()
}

/// `data/vanilla/item_tags.json`, shared by the loot interpreter and the
/// enchantment requirements `crate::item_damage` evaluates.
type ItemTags = std::sync::Arc<BTreeMap<String, Vec<String>>>;

fn item_tags() -> Option<ItemTags> {
    static TAGS: OnceLock<Option<ItemTags>> = OnceLock::new();
    TAGS.get_or_init(|| {
        let text = std::fs::read_to_string("data/vanilla/item_tags.json")
            .map_err(|e| eprintln!("no data/vanilla/item_tags.json ({e})"))
            .ok()?;
        serde_json::from_str(&text).ok().map(std::sync::Arc::new)
    })
    .clone()
}

/// A state string as the loot interpreter's `BlockRef`.
fn block_ref(state: &str) -> rustmc_loot::BlockRef {
    let (block, props) = match state.split_once('[') {
        Some((b, rest)) => (b, rest.trim_end_matches(']')),
        None => (state, ""),
    };
    rustmc_loot::BlockRef {
        block: block.to_string(),
        props: props
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

/// `Block.dropResources(state, level, pos, be, entity, tool)`, offsets 0-47:
/// `getDrops(...).forEach(stack -> popResource(level, pos, stack))`, then
/// `spawnAfterBreak` — the experience of an ore, which this server has no
/// orbs for and does not send (`block_mining.json` names the 17
/// `DropExperienceBlock`s that would).
///
/// # One deviation, named
///
/// A block table carries a `random_sequence`, and `LootTable.getRandomItems`
/// draws from the level's `RandomSequences` stream for it — seeded from the
/// world seed and the sequence id, and advanced by every earlier roll of that
/// table in the world's life. This rolls from a fresh generator instead. For
/// the tables this corpus digs (grass, dirt, stone, torch, dead coral) the
/// pool has one live entry and `LootPool.addRandomItem` draws nothing, so
/// the answer is the same under any seed; a table that draws (gravel's
/// flint, a leaf's sapling) would differ in which run drops what.
fn drop_resources(s: &Session<'_>, pos: BlockPos, state: &str, tool: &crate::item::ItemStack) {
    roll_and_pop(s.players, s.items, pos, state, tool);
}

/// The item registry for a drop with no session to hand it one — a cascade
/// drained by the tick loop.
fn shared_items() -> Option<&'static crate::player::ItemIds> {
    static ITEMS: OnceLock<Option<crate::player::ItemIds>> = OnceLock::new();
    ITEMS
        .get_or_init(|| {
            crate::player::ItemIds::load(std::path::Path::new("data/vanilla/items.json")).ok()
        })
        .as_ref()
}

/// `Level.destroyBlock(pos, true, null, ...)`'s drop, offsets 49-81:
/// `Block.dropResources(state, level, pos, be, null, ItemStack.EMPTY)` — no
/// entity and no tool, so a `match_tool` condition fails and Silk Touch or
/// Fortune never apply. Drained wherever a cascade's level events are, in
/// their order: see `crate::shapes::each_event_and_drop`.
pub fn cascade_drop(players: &crate::players::Players, drop: &crate::shapes::DropRequest) {
    roll_and_pop(
        players,
        shared_items(),
        drop.pos,
        &drop.state,
        &crate::item::ItemStack::empty(),
    );
}

fn roll_and_pop(
    players: &crate::players::Players,
    items: Option<&crate::player::ItemIds>,
    pos: BlockPos,
    state: &str,
    tool: &crate::item::ItemStack,
) {
    let Some(loot) = loot() else { return };
    let seed = crate::dropitem::unique_random().next_i64();
    let held = (!tool.is_empty()).then(|| tool.item());
    let enchants: BTreeMap<String, i32> = tool.enchantments().iter().cloned().collect();
    match loot.block_drops_enchanted(&block_ref(state), held, enchants, seed) {
        Ok(stacks) => {
            for st in stacks {
                pop_resource(players, items, pos, &st.item, st.count);
            }
        }
        Err(rustmc_loot::NoDrops::NoTableByDesign) => {}
        Err(e) => eprintln!("dig: no drop for {state}: {e:?}"),
    }
}

/// `EntityType.ITEM.getHeight()` — `sized(0.25F, 0.25F)`.
const ITEM_HEIGHT: f64 = 0.25;

/// `Mth.nextDouble(random, min, max)`: `min >= max ? min : nextDouble() *
/// (max - min) + min`.
fn next_double(r: &mut impl RandomSource, min: f64, max: f64) -> f64 {
    if min >= max {
        min
    } else {
        r.next_f64() * (max - min) + min
    }
}

/// `Block.popResource(level, pos, stack)`, offsets 0-107, and the private
/// overload it calls (0-62): a position jittered by a quarter block on each
/// axis from `level.random`, lowered by half the item's height, then — only
/// when the `block_drops` game rule is on — an `ItemEntity` with the default
/// pickup delay, added to the level.
///
/// The entity's own draws are `crate::dropitem::spawn_thrown_item`'s, in the
/// same order: the uuid's two longs, `bobOffs`, the spawn yaw, then the
/// five-argument constructor's `nextDouble() * 0.2 - 0.1` for x and z with a
/// fixed 0.2 up (`ItemEntity(Level, x, y, z, stack)` offsets 23-61).
fn pop_resource(
    players: &crate::players::Players,
    items: Option<&crate::player::ItemIds>,
    pos: BlockPos,
    item: &str,
    count: i32,
) {
    if count <= 0 || !crate::gamerules::GameRules::shared().boolean("block_drops") {
        return;
    }
    let (Some(kinds), Some(item_id)) = (
        crate::entities::shared_kind_ids(),
        items.and_then(|t| t.id_of(item)),
    ) else {
        return;
    };
    let mut level = crate::dropitem::unique_random();
    let h = ITEM_HEIGHT / 2.0;
    let x = f64::from(pos.0) + 0.5 + next_double(&mut level, -0.25, 0.25);
    let y = f64::from(pos.1) + 0.5 + next_double(&mut level, -0.25, 0.25) - h;
    let z = f64::from(pos.2) + 0.5 + next_double(&mut level, -0.25, 0.25);
    let mut own = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut own);
    let _bob_offs = own.next_f32() * std::f32::consts::PI * 2.0;
    let y_rot = own.next_f32() * 360.0;
    let dx = own.next_f64() * 0.2 - 0.1;
    let dz = own.next_f64() * 0.2 - 0.1;
    let entities = crate::entities::Entities::shared();
    let e = crate::entities::Entity {
        id: entities.next_id(),
        uuid,
        kind: crate::entities::ITEM,
        x,
        y,
        z,
        y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (dx, 0.2, dz),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::item(item_id, count),
    };
    entities.insert(e.clone());
    crate::tracker::Tracker::shared().add(
        &e,
        &crate::status::tracked_players(players, entities),
        kinds,
    );
    crate::synched::mark_dirty(e.id);
}

/// `ItemStack.canDestroyBlock` -> `Item.canDestroyBlock`, offsets 0-58:
///
/// ```text
///   tool = stack.get(TOOL)
///   if (tool != null && !tool.canDestroyBlocksInCreative())
///       return !(player.getAbilities().instabuild)
///   return true
/// ```
///
/// So a sword — `can_destroy_blocks_in_creative = false` — breaks nothing in
/// creative and everything in survival. `DebugStickItem` also overrides it
/// (always false); nothing in this server holds one.
#[must_use]
pub fn can_destroy_block(held: Option<&str>, instabuild: bool) -> bool {
    can_destroy_block_with(item_props(), held, instabuild)
}

/// [`can_destroy_block`] against an explicit table.
#[must_use]
pub fn can_destroy_block_with(
    props: Option<&crate::itemprops::ItemProps>,
    held: Option<&str>,
    instabuild: bool,
) -> bool {
    let Some(item) = held else {
        return true;
    };
    if item == "minecraft:debug_stick" {
        return false;
    }
    match props.and_then(|p| p.tool(item)) {
        Some(tool) if !tool.can_destroy_blocks_in_creative => !instabuild,
        _ => true,
    }
}

/// `broadcastChanges` for one `Inventory` cell of the player's own menu:
/// `ClientboundContainerSetSlotPacket(0, incrementStateId(), slot, stack)`.
///
/// `index` is an `Inventory` index — hotbar 0..9, main 9..36 — and the menu
/// slot is `36 + index` for the hotbar and `index` for the rest
/// (`InventoryMenu`'s layout).
pub fn send_inventory_slot(s: &mut Session<'_>, index: usize) {
    let stack = s.hotbar.stack(index);
    let Ok(i) = i16::try_from(index) else { return };
    let slot = if index < crate::player::HOTBAR_SIZE {
        crate::player::HOTBAR_SLOT_START + i
    } else {
        i
    };
    s.menu.inventory_state_id = crate::menu::increment_state_id(s.menu.inventory_state_id);
    let mut body = Vec::new();
    let written = (|| -> std::io::Result<()> {
        rustmc_proto::varint::write_varint(&mut body, 0)?;
        rustmc_proto::varint::write_varint(&mut body, s.menu.inventory_state_id)?;
        body.extend_from_slice(&slot.to_be_bytes());
        // The stack's patch — the command's enchantments, then the damage
        // `setDamageValue` put after them, in insertion order — goes out
        // through the one encoder, `crate::menu::to_wire`.
        crate::menu::write_optional_stack(&mut body, &stack, s.items)
    })();
    // `broadcastChanges` runs in the player's tick, after the chunk's block
    // changes and the entity tracker: vanilla's 047/048/050 put the slot
    // after the block_update and the dropped item's second
    // `set_entity_data`, and before the ack. The menu sweep is flushed at
    // exactly that point of this server's tick.
    if written.is_ok() {
        s.conn
            .defer_to_menu_sweep(ids::play::cb::CONTAINER_SET_SLOT, body);
    }
}

/// Apply every console `give` and `clear` queued for this player since the
/// last wake, in the order they were typed: `Inventory.add` (or clearing
/// every cell), then a `container_set_slot` per cell that changed — which is
/// what `broadcastChanges` sends. See [`crate::givecmd`] for the half of
/// `giveItem` that is not here.
pub fn receive_gives(s: &mut Session<'_>) {
    for op in crate::givecmd::take(s.conn.name()) {
        match op {
            crate::givecmd::Op::Give(stack) => {
                let placed = s.hotbar.add_item(stack);
                for index in placed.touched {
                    send_inventory_slot(s, index);
                }
            }
            crate::givecmd::Op::Clear => {
                for index in 0..crate::player::INVENTORY_SIZE {
                    if !s.hotbar.stack(index).is_empty() {
                        s.hotbar.set_stack(index, crate::item::ItemStack::empty());
                        send_inventory_slot(s, index);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props() -> Option<crate::itemprops::ItemProps> {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.pop();
        path.push(crate::itemprops::TABLE_PATH);
        if !rustmc_data::fixture::available(&path, "item props table", "ci/item-props-golden.sh") {
            return None;
        }
        Some(crate::itemprops::ItemProps::load(&path).expect("item_props.json parses"))
    }

    /// `Item.canDestroyBlock`: a sword's `can_destroy_blocks_in_creative =
    /// false` refuses only a creative player; an item with no `tool`
    /// component never refuses; the debug stick always does.
    #[test]
    fn a_sword_refuses_only_in_creative() {
        let Some(p) = props() else { return };
        let p = Some(&p);
        assert!(!can_destroy_block_with(
            p,
            Some("minecraft:diamond_sword"),
            true
        ));
        assert!(can_destroy_block_with(
            p,
            Some("minecraft:diamond_sword"),
            false
        ));
        assert!(can_destroy_block_with(
            p,
            Some("minecraft:diamond_pickaxe"),
            true
        ));
        assert!(can_destroy_block_with(p, Some("minecraft:stone"), true));
        assert!(can_destroy_block_with(p, None, true));
        assert!(!can_destroy_block_with(
            p,
            Some("minecraft:debug_stick"),
            false
        ));
        assert!(
            !can_destroy_block_with(p, Some("minecraft:mace"), true),
            "the mace and the trident say so too"
        );
    }

    #[test]
    fn a_state_string_becomes_a_loot_block_ref() {
        let r = block_ref("minecraft:oak_door[facing=north,half=lower]");
        assert_eq!(r.block, "minecraft:oak_door");
        assert_eq!(r.props.get("half").map(String::as_str), Some("lower"));
        assert_eq!(r.props.len(), 2);
        assert!(block_ref("minecraft:dirt").props.is_empty());
    }

    /// `levels_squared + 1`, and nothing without the enchantment.
    #[test]
    fn efficiency_is_the_square_plus_one() {
        let plain = crate::item::ItemStack::one("minecraft:diamond_pickaxe");
        assert_eq!(mining_efficiency(&plain), 0.0);
        assert_eq!(
            mining_efficiency(&plain.clone().with_enchantment("minecraft:efficiency", 5)),
            26.0
        );
        assert_eq!(
            mining_efficiency(&plain.with_enchantment("efficiency", 1)),
            2.0
        );
    }
}
