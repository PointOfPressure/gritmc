package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.GameMasterBlock;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

/**
 * The per-block facts `ServerPlayerGameMode`'s dig pipeline reads, taken from
 * the live registry of the pinned jar rather than from any reading of it.
 *
 * <pre>
 *   BlockBehaviour.getDestroyProgress(state, player, level, pos)
 *      0-7    f = state.getDestroySpeed(level, pos)      -> destroy_time
 *     11-19   f == -1.0F  -> return 0
 *     20-35   player.hasCorrectToolForDrops(state) ? 30 : 100
 *              -> state.requiresCorrectToolForDrops()   -> requires_correct_tool
 *
 *   ServerPlayerGameMode.destroyBlock(pos)
 *     51-82   block instanceof GameMasterBlock && !canUseGameMasterBlocks
 *                                                        -> game_master
 *    104-116  block.playerWillDestroy(...)               -> will_destroy (the class
 *                                                           that declares it)
 *    254-269  block.playerDestroy(...)                   -> player_destroy
 * </pre>
 *
 * Both floats are read off EVERY state of the block and the generator refuses
 * a block whose states disagree, because the server keys the table by block
 * and a per-state difference would otherwise be silently averaged away.
 * `destroyTime` and `requiresCorrectToolForDrops` are copied from
 * `BlockBehaviour.Properties` into each `BlockStateBase` at construction, so
 * a disagreement would be a new mechanism, not noise.
 */
public final class MiningGolden {
    private MiningGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    /**
     * The most-derived class that declares `name(args)` for `block`, or null
     * when that is `Block` or `BlockBehaviour` — i.e. the block inherits the
     * default. Walks `getDeclaredMethod` up the hierarchy because the
     * protected overrides (`spawnAfterBreak`) are invisible to `getMethod`.
     */
    private static String declarer(Block block, String name, Class<?>... args) {
        for (Class<?> c = block.getClass(); c != null; c = c.getSuperclass()) {
            try {
                c.getDeclaredMethod(name, args);
            } catch (NoSuchMethodException e) {
                continue;
            }
            if (c == Block.class || c == net.minecraft.world.level.block.state.BlockBehaviour.class) {
                return null;
            }
            return c.getName();
        }
        throw new IllegalStateException("no " + name + " anywhere above " + block.getClass());
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        List<String> rows = new ArrayList<>();
        int unbreakable = 0;
        int instant = 0;
        int requires = 0;
        for (Block block : BuiltInRegistries.BLOCK) {
            BlockState def = block.defaultBlockState();
            float destroy = def.getDestroySpeed(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
            boolean needsTool = def.requiresCorrectToolForDrops();
            for (BlockState s : block.getStateDefinition().getPossibleStates()) {
                float d = s.getDestroySpeed(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
                if (Float.floatToIntBits(d) != Float.floatToIntBits(destroy)
                        || s.requiresCorrectToolForDrops() != needsTool) {
                    throw new IllegalStateException("states of "
                        + BuiltInRegistries.BLOCK.getKey(block) + " disagree: " + s);
                }
            }
            if (destroy == -1.0F) {
                unbreakable++;
            }
            if (destroy == 0.0F) {
                instant++;
            }
            if (needsTool) {
                requires++;
            }
            String willDestroy = declarer(block, "playerWillDestroy",
                Level.class, BlockPos.class, BlockState.class, Player.class);
            String playerDestroy = declarer(block, "playerDestroy",
                Level.class, Player.class, BlockPos.class, BlockState.class,
                BlockEntity.class, ItemStack.class);
            String afterBreak = declarer(block, "spawnAfterBreak",
                BlockState.class, ServerLevel.class, BlockPos.class, ItemStack.class,
                boolean.class);
            rows.add("  {"
                + q("block") + ": " + q(BuiltInRegistries.BLOCK.getKey(block).toString()) + ", "
                + q("destroy_time") + ": " + Float.toString(destroy) + ", "
                + q("requires_correct_tool") + ": " + needsTool + ", "
                + q("game_master") + ": " + (block instanceof GameMasterBlock) + ", "
                + q("will_destroy") + ": " + (willDestroy == null ? "null" : q(willDestroy)) + ", "
                + q("player_destroy") + ": " + (playerDestroy == null ? "null" : q(playerDestroy)) + ", "
                + q("spawn_after_break") + ": " + (afterBreak == null ? "null" : q(afterBreak))
                + "}");
        }
        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"rng\": \"none\",\n");
        out.append("  \"blocks\": ").append(rows.size()).append(",\n");
        out.append("  \"unbreakable\": ").append(unbreakable).append(",\n");
        out.append("  \"instant\": ").append(instant).append(",\n");
        out.append("  \"requires_correct_tool\": ").append(requires).append(",\n");
        out.append("  \"table\": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
