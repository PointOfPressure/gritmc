// PlacementGolden — what `Block.getStateForPlacement` answers, per block.
//
// # The problem this exists for
//
// `getStateForPlacement` is per block **class**, and everything this project
// extracts from the jar is per block **state**. `block_props.json` says
// `minecraft:oak_stairs` has a `facing` and so does `minecraft:furnace`, and
// stops there — but
//
//   StairBlock            FACING = context.getHorizontalDirection()
//   AbstractFurnaceBlock  FACING = context.getHorizontalDirection().getOpposite()
//
// are **opposites**, and nothing in a table of states can tell them apart. So
// a server that sets `facing` from the player's yaw with one generic rule is
// backwards for half the blocks that have it: wrong-and-confident, which is
// worse than uniformly unfinished. The only way to be right is to ask each
// block, which is what this does.
//
// # What it asks
//
// For every block in `BuiltInRegistries.BLOCK`, a grid of clicks:
//
//   6 clicked faces  x  4 yaws  x  3 pitches  x  2 cursor heights  =  144
//
// and for each, the state `getStateForPlacement` returns, serialized by
// `BlockStateParser.serialize` — the same spelling `block_props.json` uses,
// so the two tables join on it.
//
// The axes are chosen so that each one is a **derived vanilla quantity** a
// port can recompute exactly, not a raw angle:
//
//   * the four yaws are 0, 90, 180, 270, which `Direction.fromYRot` maps to
//     SOUTH, WEST, NORTH, EAST — every value `getHorizontalDirection()` can
//     take. `fromYRot` rounds to the nearest cardinal, so a port that
//     computes it from a real yaw lands in exactly one of these columns and
//     the column is exact for **every** yaw, not only the four sampled.
//   * the three pitches are -90, 0, +90, which `orderedByNearest` turns into
//     a nearest-looking direction of UP, the horizontal one, and DOWN. Those
//     are the only three cases a `getNearestLookingDirection()` block can be
//     in, at a cardinal yaw.
//   * the two cursor heights are 0.25 and 0.75 above the *placement* block's
//     corner, which is the comparison `StairBlock` and `SlabBlock` make:
//     `clickLocation.y - pos.getY() > 0.5`.
//
// # The world it clicks in
//
// One `minecraft:stone` at the clicked position and **air everywhere else**.
// Two things follow from that and both are deliberate:
//
//   * stone is not replaceable, so `BlockPlaceContext.replaceClicked` is
//     false and the placement goes one step along the clicked face — which is
//     what clicking a solid block does. An all-air level would make every
//     click replace the block it hit, `getClickedPos()` would return the hit
//     position, and `getNearestLookingDirections()` would skip its reordering
//     — a different code path from the one a player takes.
//   * a block that reads its neighbours — a fence, a wall, a pane, redstone —
//     sees air, so the table is *placed into open air*. That is a real
//     limit and it is named in `docs/SERVER.md`: this table cannot answer
//     what a fence connects to. The `neighbourSensitive` flag below is how a
//     consumer finds out which blocks it must not trust for that.
//
// # Why a real Player and not a stubbed context
//
// `getHorizontalDirection()` is `player.getDirection()`, `getRotation()` is
// `player.getYRot()`, and `getNearestLookingDirection()` is
// `Direction.orderedByNearest(player)[0]`. Overriding those in a
// `BlockPlaceContext` subclass — the shape `DirectionalPlaceContext` uses —
// would mean transcribing three vanilla methods into the oracle, and an
// oracle that reimplements the thing it is measuring is the fault this
// project keeps finding (ORACLE.md 5b). A real `Player` on a real
// `BlockPlaceContext` runs vanilla's own code and there is nothing to get
// wrong.
//
// The `Level` under it is a table: one `HashMap` of positions, and **all 29
// abstract methods throw**. Same argument as `GenLevel` — a stub that
// returns a plausible empty answer for a method nobody foresaw would produce
// a table that is quietly wrong; one that throws turns an incomplete analysis
// into a crash naming the method.
//
// # What comes out
//
//   {"block": "minecraft:oak_stairs",
//    "default": "minecraft:oak_stairs[facing=north,half=bottom,...]",
//    "states": [ ...distinct answers... ],
//    "cells":  [ ...144 indices into `states`, -1 for null... ],
//    "yawOnly": true, "trivial": false, ...}
//
// `cells` is indexed `((face * 4 + yaw) * 3 + pitch) * 2 + cursor`, with the
// faces in `Direction.values()` order — DOWN, UP, NORTH, SOUTH, WEST, EAST —
// because that is the order everything in vanilla iterates them and a table
// whose column order disagrees with the enum is a silent transposition.
//
// `-1` is a **null** return, which is vanilla saying "this block cannot be
// placed here" (a door with no room above, a crop on no farmland). It is not
// an error and a consumer must not place anything for it.
//
// The flags are the consumer's contract:
//
//   trivial            every cell is the default state — the block has
//                      nothing to compute and needs no table entry.
//   yawOnly            the three pitch rows agree everywhere. A consumer that
//                      knows only `Direction.fromYRot(yaw)` is **exact** for
//                      this block. Stairs, furnaces, slabs and trapdoors are
//                      all here.
//   pitchSensitive     the negation. These are the `getNearestLookingDirection`
//                      blocks — dispensers, observers, pistons, end rods —
//                      and a consumer that ignores pitch is wrong for them,
//                      so it must fall back to the default rather than guess.
//   random             the answer is a die roll and no table can hold it.
//                      `GrowingPlantHeadBlock.getStateForPlacement` is
//                      `AGE = level.getRandom().nextInt(25)` — weeping vines,
//                      twisting vines, cave vines. Those rows carry the flag
//                      and an **empty** `states`/`cells`, because writing one
//                      sample of a distribution into a golden file is how a
//                      file churns; the two-run check in the script is what
//                      found them.
//   schedulesTick      the placement asked the level to schedule a block or
//                      fluid tick before it returned a state — coral does,
//                      redstone does. Nothing in this server ticks yet, so
//                      this is where "nothing ticks" has to start looking.
//   neighbourSensitive detected by re-running one cell with a stone ring
//                      around the placement position: the answer moved, so
//                      this block reads the world and an into-open-air table
//                      does not describe it. Fences, walls, panes, redstone.
//
// # The item in hand is EMPTY, and that was forced
//
// `new ItemStack(item)` cannot be built after `Bootstrap.bootStrap()` alone:
// it reads the item holder's data components, and those are bound by
// `DataComponentInitializers` during a **datapack load**
// (`ReloadableServerResources`), not at bootstrap. The stack trace says
// "Components not bound yet" and there is no cheaper way in.
//
// So the context carries `ItemStack.EMPTY`. Read from the bytecode rather
// than assumed: of the 117 classes that mention `getStateForPlacement`,
// exactly **two** touch the stack inside it — `LecternBlock`, which asks for
// `BLOCK_ENTITY_DATA`, and `TestBlock`. Both treat an absent component as the
// ordinary case, which is what an empty stack gives, so the table is the
// answer a plain item would produce for every block in it.
//
// It also carries an `items` map — item name to the block its `BlockItem`
// places. `getStateForPlacement` never says *what* is being placed, and a
// server that guesses from the name is right for nearly every item and
// silently wrong for the rest (`minecraft:redstone` places `redstone_wire`).
// `BlockItem.getBlock()` is the real answer and is in no generated report.
//
// Converted to JSON by `ci/placement-golden.sh`, gitignored like the rest of
// `data/vanilla/`, regenerable from the jar in one command.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import com.mojang.authlib.GameProfile;
import com.mojang.serialization.Lifecycle;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.RegistrationInfo;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.Difficulty;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.context.BlockPlaceContext;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.WritableLevelData;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

public final class PlacementGolden {

    /// The clicked block. Y is a plain in-range value; nothing here depends
    /// on it, and it is not 0 so that a sign bug in a consumer's own packing
    /// shows up as a wrong number rather than as zero.
    private static final BlockPos CLICKED = new BlockPos(0, 64, 0);

    /// `Direction.fromYRot` maps these to SOUTH, WEST, NORTH, EAST — the four
    /// values `getHorizontalDirection()` can return, in `from2DDataValue`
    /// order.
    private static final float[] YAWS = {0.0F, 90.0F, 180.0F, 270.0F};

    /// Looking up, level, and down. `Direction.orderedByNearest` turns these
    /// into a nearest-looking direction of UP, the horizontal one, and DOWN —
    /// vanilla's pitch is negative upwards.
    private static final float[] PITCHES = {-90.0F, 0.0F, 90.0F};

    /// Below and above the middle of the block being placed into, which is
    /// the `> 0.5` comparison `StairBlock` and `SlabBlock` make.
    private static final double[] CURSORS = {0.25, 0.75};

    private static final int CELLS =
        Direction.values().length * YAWS.length * PITCHES.length * CURSORS.length;

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        Holder<DimensionType> overworld = lookup
            .lookupOrThrow(Registries.DIMENSION_TYPE)
            .getOrThrow(BuiltinDimensionTypes.OVERWORLD);

        TableLevel level = new TableLevel(accessOf(lookup), overworld);
        HarnessPlayer player = new HarnessPlayer(level);

        List<String> rows = new ArrayList<>();
        int trivial = 0;
        int yawOnly = 0;
        int pitchSensitive = 0;
        int neighbourSensitive = 0;
        int schedulesTick = 0;
        int randomBlocks = 0;
        int nulls = 0;

        // `BuiltInRegistries.BLOCK` iterates in registration order, which is
        // stable across runs — unlike `HolderLookup.listElements()`, whose
        // instability once churned the decorate vectors. `ci/placement-golden.sh`
        // runs this twice and demands the two agree, so a regression here is
        // caught rather than assumed away.
        for (Block block : BuiltInRegistries.BLOCK) {
            String name = BuiltInRegistries.BLOCK.getKey(block).toString();
            String def = BlockStateParser.serialize(block.defaultBlockState());
            ItemStack stack = ItemStack.EMPTY;

            level.blockTicks.scheduled = 0;
            level.fluidTicks.scheduled = 0;
            String[] answers = grid(level, player, block, stack);
            boolean ticks = level.blockTicks.scheduled + level.fluidTicks.scheduled > 0;

            // **Twice, per block.** Three placements are genuinely random —
            // `GrowingPlantHeadBlock.getStateForPlacement` is
            // `AGE = level.getRandom().nextInt(25)`, which is weeping vines,
            // twisting vines and cave vines — and a table cannot hold a die
            // roll. The two-run check in `ci/placement-golden.sh` found them
            // by refusing to agree with itself, which is what that check is
            // for; this turns "the file churns" into a column.
            boolean random = !java.util.Arrays.equals(answers, grid(level, player, block, stack));

            Map<String, Integer> index = new LinkedHashMap<>();
            int[] cells = new int[CELLS];
            boolean anyNull = false;
            for (int i = 0; i < CELLS; i++) {
                String answer = answers[i];
                if (answer == null) {
                    cells[i] = -1;
                    anyNull = true;
                } else {
                    cells[i] = index.computeIfAbsent(answer, k -> index.size());
                }
            }

            boolean isTrivial = !random && index.size() == 1 && index.containsKey(def) && !anyNull;
            // A random block's cells are one sample of a distribution, so
            // every flag derived from them is meaningless and is not
            // computed. `random` is the whole answer for those three.
            boolean isYawOnly = !random && pitchRowsAgree(cells);
            boolean neighbours = !random && readsNeighbours(level, player, block, stack);

            if (isTrivial) {
                trivial++;
            }
            if (random) {
                randomBlocks++;
            } else if (isYawOnly) {
                yawOnly++;
            } else {
                pitchSensitive++;
            }
            if (neighbours) {
                neighbourSensitive++;
            }
            if (ticks) {
                schedulesTick++;
            }
            if (anyNull) {
                nulls++;
            }

            // A block whose every answer is its own default carries no
            // information and is 90% of the registry, so it is summarised
            // rather than spelled out. The count is asserted in the header so
            // a consumer can tell "absent because trivial" from "absent
            // because the dump broke".
            if (isTrivial && !neighbours && !ticks) {
                continue;
            }

            // A random block emits no cells: writing one sample of a die roll
            // into a golden file is how a file churns, and a consumer that
            // read it would place a state vanilla only sometimes places.
            StringBuilder states = new StringBuilder("[");
            StringBuilder cellText = new StringBuilder("[");
            if (!random) {
                int n = 0;
                for (String st : index.keySet()) {
                    states.append(n++ == 0 ? "" : ", ").append(quote(st));
                }
                for (int i = 0; i < cells.length; i++) {
                    cellText.append(i == 0 ? "" : ",").append(cells[i]);
                }
            }
            states.append("]");
            cellText.append("]");

            rows.add("{\"block\": " + quote(name)
                + ", \"default\": " + quote(def)
                + ", \"trivial\": " + isTrivial
                + ", \"yawOnly\": " + isYawOnly
                + ", \"neighbourSensitive\": " + neighbours
                + ", \"schedulesTick\": " + ticks
                + ", \"random\": " + random
                + ", \"states\": " + states
                + ", \"cells\": " + cellText
                + "}");
        }

        // What each item places, which `getStateForPlacement` never says.
        //
        // The server maps an item to a block **by name**, which is right for
        // nearly every `BlockItem` — they are registered from their own block
        // — and silently wrong for the rest: `minecraft:redstone` places
        // `redstone_wire`, `minecraft:wheat_seeds` places `wheat`. The real
        // mapping is `BlockItem.getBlock()` and it is in no generated report,
        // so it is read off the class here. Items that are not `BlockItem`s
        // are absent, which is the correct answer for a sword.
        List<String> items = new ArrayList<>();
        List<String> wallItems = new ArrayList<>();
        int renamed = 0;
        for (net.minecraft.world.item.Item item : BuiltInRegistries.ITEM) {
            if (!(item instanceof net.minecraft.world.item.BlockItem placer)) {
                continue;
            }
            String itemName = BuiltInRegistries.ITEM.getKey(item).toString();
            String blockName = BuiltInRegistries.BLOCK.getKey(placer.getBlock()).toString();
            if (!itemName.equals(blockName)) {
                renamed++;
            }
            items.add(quote(itemName) + ": " + quote(blockName));

            // A torch, a sign, a banner and a head each place **two**
            // different blocks depending on which face was clicked, and
            // `getBlock()` returns only the standing one. The wall variant
            // lives in `StandingAndWallBlockItem.wallBlock`, a private field
            // that no generated report carries, so it is reflected out here
            // for the same reason `getBlock()` is: guessing it from the name
            // works for `torch` -> `wall_torch` and not for
            // `skeleton_skull` -> `skeleton_wall_skull`.
            if (placer instanceof net.minecraft.world.item.StandingAndWallBlockItem) {
                try {
                    java.lang.reflect.Field f = net.minecraft.world.item.StandingAndWallBlockItem.class
                            .getDeclaredField("wallBlock");
                    f.setAccessible(true);
                    net.minecraft.world.level.block.Block wall =
                            (net.minecraft.world.level.block.Block) f.get(placer);
                    wallItems.add(quote(itemName) + ": "
                            + quote(BuiltInRegistries.BLOCK.getKey(wall).toString()));
                } catch (ReflectiveOperationException e) {
                    throw new IllegalStateException(
                            "StandingAndWallBlockItem.wallBlock is not where it was: " + itemName, e);
                }
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"placement\",\n");
        out.append("  \"faceOrder\": [");
        for (int i = 0; i < Direction.values().length; i++) {
            out.append(i == 0 ? "" : ", ").append(quote(Direction.values()[i].name()));
        }
        out.append("],\n");
        out.append("  \"yaws\": [0.0, 90.0, 180.0, 270.0],\n");
        out.append("  \"yawDirections\": [");
        for (int i = 0; i < YAWS.length; i++) {
            out.append(i == 0 ? "" : ", ")
                .append(quote(Direction.fromYRot(YAWS[i]).name()));
        }
        out.append("],\n");
        out.append("  \"pitches\": [-90.0, 0.0, 90.0],\n");
        out.append("  \"cursors\": [0.25, 0.75],\n");
        out.append("  \"cellStride\": \"((face * 4 + yaw) * 3 + pitch) * 2 + cursor\",\n");
        out.append("  \"cells\": ").append(CELLS).append(",\n");
        out.append("  \"blocks\": ").append(BuiltInRegistries.BLOCK.size()).append(",\n");
        out.append("  \"trivialBlocks\": ").append(trivial).append(",\n");
        out.append("  \"yawOnlyBlocks\": ").append(yawOnly).append(",\n");
        out.append("  \"pitchSensitiveBlocks\": ").append(pitchSensitive).append(",\n");
        out.append("  \"neighbourSensitiveBlocks\": ").append(neighbourSensitive).append(",\n");
        out.append("  \"schedulesTickBlocks\": ").append(schedulesTick).append(",\n");
        out.append("  \"randomBlocks\": ").append(randomBlocks).append(",\n");
        out.append("  \"blocksWithARefusal\": ").append(nulls).append(",\n");
        out.append("  \"blockItems\": ").append(items.size()).append(",\n");
        out.append("  \"blockItemsUnderAnotherName\": ").append(renamed).append(",\n");
        out.append("  \"items\": {\n");
        for (int i = 0; i < items.size(); i++) {
            out.append("    ").append(items.get(i)).append(i + 1 < items.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"wallItems\": {\n");
        for (int i = 0; i < wallItems.size(); i++) {
            out.append("    ").append(wallItems.get(i))
                    .append(i + 1 < wallItems.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i)).append(i + 1 < rows.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// `VanillaRegistries.createLookup()` gives lookups; `Level`'s
    /// constructor wants a `RegistryAccess`, which is a different type
    /// carrying the same content. This rebuilds **every** registry the
    /// provider lists.
    ///
    /// Every one, rather than the one that happened to be missing: the first
    /// run of this harness died in `PalettedContainerFactory` wanting biomes,
    /// the second in `DamageSources` wanting damage types, and a table
    /// assembled by adding registries one crash at a time is exactly the
    /// oracle that reports success the moment it stops throwing.
    ///
    /// The holders it mints are **not** the provider's, and vanilla compares
    /// holders by identity (ORACLE.md 5b rule 1). Nothing on the placement
    /// path compares a biome or a damage type to a baked-in holder, so it is
    /// safe here — and it would not be in a worldgen harness, which is why
    /// this is not shared with one.
    static RegistryAccess accessOf(HolderLookup.Provider lookup) {
        List<Registry<?>> registries = new ArrayList<>();
        lookup.listRegistries().forEach(r -> registries.add(rebuild(r)));
        return new RegistryAccess.ImmutableRegistryAccess(registries);
    }

    @SuppressWarnings("unchecked")
    private static <T> Registry<T> rebuild(HolderLookup.RegistryLookup<T> lookup) {
        MappedRegistry<T> out = new MappedRegistry<>(
            (ResourceKey<? extends Registry<T>>) lookup.key(), Lifecycle.stable());
        lookup.listElements()
            .forEach(h -> out.register(h.key(), h.value(), RegistrationInfo.BUILT_IN));
        return out.freeze();
    }

    /// The whole 144-cell grid for one block, in cell order.
    static String[] grid(TableLevel level, HarnessPlayer player, Block block, ItemStack stack) {
        String[] out = new String[CELLS];
        int at = 0;
        for (Direction face : Direction.values()) {
            for (float yaw : YAWS) {
                for (float pitch : PITCHES) {
                    for (double cursor : CURSORS) {
                        out[at++] = ask(level, player, block, stack, face, yaw, pitch, cursor);
                    }
                }
            }
        }
        return out;
    }

    /// One click, and the state it produces. `null` is vanilla's own "cannot
    /// be placed here" and is carried through rather than smoothed away.
    private static String ask(
        TableLevel level,
        HarnessPlayer player,
        Block block,
        ItemStack stack,
        Direction face,
        float yaw,
        float pitch,
        double cursorY
    ) {
        player.aim(yaw, pitch);
        BlockPos placed = CLICKED.relative(face);

        // The click point: the centre of the clicked face, with the height
        // driven separately because that is the only component any
        // `getStateForPlacement` compares against a half-block.
        Vec3 hit = new Vec3(
            CLICKED.getX() + 0.5 + face.getStepX() * 0.5,
            placed.getY() + cursorY,
            CLICKED.getZ() + 0.5 + face.getStepZ() * 0.5);
        BlockHitResult result = new BlockHitResult(hit, face, CLICKED, false);
        BlockPlaceContext context =
            new BlockPlaceContext(player, InteractionHand.MAIN_HAND, stack, result);

        BlockState state;
        try {
            state = block.getStateForPlacement(context);
        } catch (RuntimeException e) {
            // Loud, not swallowed: the dump names the block and the throw and
            // stops, because a table with a silently missing row is exactly
            // the oracle fault this file's header is about.
            throw new IllegalStateException(
                "getStateForPlacement threw for "
                    + BuiltInRegistries.BLOCK.getKey(block)
                    + " face=" + face + " yaw=" + yaw + " pitch=" + pitch,
                e);
        }
        return state == null ? null : BlockStateParser.serialize(state);
    }

    /// True when the three pitch rows are identical everywhere — i.e. the
    /// block never asks which way the player is looking vertically, so
    /// `Direction.fromYRot(yaw)` alone determines its answer.
    private static boolean pitchRowsAgree(int[] cells) {
        for (Direction face : Direction.values()) {
            for (int yaw = 0; yaw < YAWS.length; yaw++) {
                for (int cursor = 0; cursor < CURSORS.length; cursor++) {
                    int first = cells[at(face.ordinal(), yaw, 0, cursor)];
                    for (int pitch = 1; pitch < PITCHES.length; pitch++) {
                        if (cells[at(face.ordinal(), yaw, pitch, cursor)] != first) {
                            return false;
                        }
                    }
                }
            }
        }
        return true;
    }

    /// `((face * 4 + yaw) * 3 + pitch) * 2 + cursor`, spelled once.
    private static int at(int face, int yaw, int pitch, int cursor) {
        return ((face * YAWS.length + yaw) * PITCHES.length + pitch) * CURSORS.length + cursor;
    }

    /// Does the answer move when the placement position gets neighbours?
    ///
    /// The whole grid is clicked into open air, so a fence's answer there is
    /// "connected to nothing" — true for that world and a lie about any
    /// other. This surrounds the placement position with stone on all six
    /// sides and re-asks one click; a block whose answer changes is one this
    /// table cannot describe, and says so.
    private static boolean readsNeighbours(
        TableLevel level, HarnessPlayer player, Block block, ItemStack stack
    ) {
        Direction face = Direction.UP;
        String open = ask(level, player, block, stack, face, 0.0F, 0.0F, 0.25);
        BlockPos placed = CLICKED.relative(face);
        try {
            for (Direction d : Direction.values()) {
                level.put(placed.relative(d), Blocks.STONE.defaultBlockState());
            }
            String walled = ask(level, player, block, stack, face, 0.0F, 0.0F, 0.25);
            return open == null ? walled != null : !open.equals(walled);
        } finally {
            level.reset();
        }
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private PlacementGolden() {}

    // ------------------------------------------------------------------
    // The level.
    // ------------------------------------------------------------------

    /// A `Level` that is a `HashMap`, and throws for everything else.
    ///
    /// Every one of `Level`'s 29 abstract methods raises rather than
    /// answering. That is the same argument `GenLevel` makes: a stub that
    /// invents a plausible empty answer for a method the analysis did not
    /// foresee produces a table that is quietly wrong, and this project has
    /// chased four parity failures to exactly that. A throw names the method.
    static final class TableLevel extends Level {

        private final Map<BlockPos, BlockState> blocks = new HashMap<>();

        TableLevel(RegistryAccess access, Holder<DimensionType> dimension) {
            super(
                DATA,
                Level.OVERWORLD,
                access,
                dimension,
                false,
                false,
                0L,
                0);
            reset();
        }

        /// The world every cell is measured in: one stone at the clicked
        /// position, air everywhere else.
        void reset() {
            blocks.clear();
            blocks.put(CLICKED, Blocks.STONE.defaultBlockState());
        }

        void put(BlockPos pos, BlockState state) {
            blocks.put(pos.immutable(), state);
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return blocks.getOrDefault(pos, Blocks.AIR.defaultBlockState());
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return getBlockState(pos).getFluidState();
        }

        @Override
        public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) {
            return null;
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            return true;
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            put(pos, state);
            return true;
        }

        private static <T> T no(String what) {
            throw new UnsupportedOperationException(
                "PlacementGolden's level was asked for " + what
                    + " — a placement rule reads something this harness does not model");
        }

        @Override
        public net.minecraft.world.level.chunk.ChunkSource getChunkSource() {
            return no("getChunkSource");
        }

        @Override
        public net.minecraft.world.clock.ClockManager clockManager() {
            return no("clockManager");
        }

        /// None — the same sweep as `getEntities`. `Level.getEntities`
        /// unions the entity index with the dragon's hitbox parts, so a
        /// world with no entities has no dragon parts either.
        @Override
        public Collection<net.minecraft.world.entity.boss.enderdragon.EnderDragonPart> dragonParts() {
            return List.of();
        }

        @Override
        public Entity getEntity(int id) {
            return no("getEntity");
        }

        @Override
        public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() {
            return no("environmentAttributes");
        }

        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return no("enabledFeatures");
        }

        @Override
        public net.minecraft.world.level.block.entity.FuelValues fuelValues() {
            return no("fuelValues");
        }

        @Override
        public Holder<net.minecraft.world.level.biome.Biome> getUncachedNoiseBiome(int x, int y, int z) {
            return no("getUncachedNoiseBiome");
        }

        /// No entities, which is the truth about this world rather than a
        /// convenient blank.
        ///
        /// `DirtPathBlock.getStateForPlacement` calls `Block.pushEntitiesUp`,
        /// which sweeps the box above the placement for anything standing
        /// in it. There is nothing standing there — the harness places into
        /// open air with the player nowhere near — so an empty getter is the
        /// correct answer and not a stub. A block that *counted* entities
        /// would still be measured in an empty world, and that limit is the
        /// same one `neighbourSensitive` describes for the block table.
        @Override
        protected net.minecraft.world.level.entity.LevelEntityGetter<Entity> getEntities() {
            return NO_ENTITIES;
        }

        /// Placement can **schedule a tick**, and that was a finding.
        ///
        /// `CoralBlock.getStateForPlacement` calls `scheduleTick` before it
        /// returns a state — coral checks whether it is still in water a
        /// moment later and dies if not. So does redstone, and so do the
        /// fluids. This server ticks nothing but the clock yet, so the sink
        /// counts them and drops them; the count becomes the
        /// `schedulesTick` column, which is where the "nothing ticks" item
        /// has to start.
        final TickSink<Block> blockTicks = new TickSink<>();
        final TickSink<net.minecraft.world.level.material.Fluid> fluidTicks = new TickSink<>();

        @Override
        public net.minecraft.world.ticks.LevelTickAccess<Block> getBlockTicks() {
            return blockTicks;
        }

        @Override
        public net.minecraft.world.ticks.LevelTickAccess<net.minecraft.world.level.material.Fluid> getFluidTicks() {
            return fluidTicks;
        }

        @Override
        public List<? extends Player> players() {
            return List.of();
        }

        @Override
        public net.minecraft.world.level.saveddata.maps.MapItemSavedData getMapData(
            net.minecraft.world.level.saveddata.maps.MapId id
        ) {
            return no("getMapData");
        }

        @Override
        public net.minecraft.world.item.alchemy.PotionBrewing potionBrewing() {
            return no("potionBrewing");
        }

        @Override
        public net.minecraft.world.item.crafting.RecipeAccess recipeAccess() {
            return no("recipeAccess");
        }

        @Override
        public LevelData.RespawnData getRespawnData() {
            return no("getRespawnData");
        }

        @Override
        public net.minecraft.world.scores.Scoreboard getScoreboard() {
            return no("getScoreboard");
        }

        @Override
        public String gatherChunkSourceStats() {
            return no("gatherChunkSourceStats");
        }

        @Override
        public net.minecraft.world.TickRateManager tickRateManager() {
            return no("tickRateManager");
        }

        /// A real one, at its defaults.
        ///
        /// `BedBlock.getStateForPlacement` asks whether the head position is
        /// inside the border before it will place a bed, and a throw there
        /// was the harness's first genuine finding rather than a bug: beds
        /// are the one placement rule that consults it. `new WorldBorder()`
        /// is vanilla's own fresh-world border — centred on (0,0), 59,999,968
        /// wide — so the answer is the one a new world gives, and the clicked
        /// position is nowhere near an edge.
        private final net.minecraft.world.level.border.WorldBorder border =
            new net.minecraft.world.level.border.WorldBorder();

        @Override
        public net.minecraft.world.level.border.WorldBorder getWorldBorder() {
            return border;
        }

        @Override
        public int getSeaLevel() {
            return no("getSeaLevel");
        }

        @Override
        public void destroyBlockProgress(int id, BlockPos pos, int progress) {
            no("destroyBlockProgress");
        }

        @Override
        public void explode(
            Entity source,
            net.minecraft.world.damagesource.DamageSource damage,
            net.minecraft.world.level.ExplosionDamageCalculator calculator,
            double x, double y, double z, float radius, boolean fire,
            Level.ExplosionInteraction interaction,
            net.minecraft.core.particles.ParticleOptions small,
            net.minecraft.core.particles.ParticleOptions large,
            net.minecraft.util.random.WeightedList<net.minecraft.core.particles.ExplosionParticleInfo> particles,
            Holder<net.minecraft.sounds.SoundEvent> sound
        ) {
            no("explode");
        }

        @Override
        public void gameEvent(
            Holder<net.minecraft.world.level.gameevent.GameEvent> event,
            Vec3 pos,
            net.minecraft.world.level.gameevent.GameEvent.Context context
        ) {
            no("gameEvent");
        }

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {
            no("levelEvent");
        }

        @Override
        public void playSeededSound(
            Entity source, Entity target, Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch, long seed
        ) {
            no("playSeededSound");
        }

        @Override
        public void playSeededSound(
            Entity source, double x, double y, double z,
            Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch, long seed
        ) {
            no("playSeededSound");
        }

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState was, BlockState now, int flags) {
            // Nothing observes it, and it is reached by `setBlock`, which
            // `getStateForPlacement` never calls. Silent rather than throwing
            // so a future caller that does place a block is not stopped by
            // its own notification.
        }

        @Override
        public void setRespawnData(LevelData.RespawnData data) {
            no("setRespawnData");
        }
    }

    /// Counts scheduled ticks and drops them — see `TableLevel.getBlockTicks`.
    static final class TickSink<T> implements net.minecraft.world.ticks.LevelTickAccess<T> {
        int scheduled;

        @Override
        public void schedule(net.minecraft.world.ticks.ScheduledTick<T> tick) {
            scheduled++;
        }

        @Override
        public boolean hasScheduledTick(BlockPos pos, T what) {
            return false;
        }

        @Override
        public boolean willTickThisTick(BlockPos pos, T what) {
            return false;
        }

        @Override
        public int count() {
            return scheduled;
        }
    }

    /// The empty entity index — see `TableLevel.getEntities`.
    private static final net.minecraft.world.level.entity.LevelEntityGetter<Entity> NO_ENTITIES =
        new net.minecraft.world.level.entity.LevelEntityGetter<Entity>() {
            @Override
            public Entity get(int id) {
                return null;
            }

            @Override
            public Entity get(java.util.UUID uuid) {
                return null;
            }

            @Override
            public Iterable<Entity> getAll() {
                return List.of();
            }

            @Override
            public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                net.minecraft.util.AbortableIterationConsumer<U> consumer
            ) {
            }

            @Override
            public void get(net.minecraft.world.phys.AABB box, java.util.function.Consumer<Entity> consumer) {
            }

            @Override
            public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                net.minecraft.world.phys.AABB box,
                net.minecraft.util.AbortableIterationConsumer<U> consumer
            ) {
            }
        };

    /// `WritableLevelData` for a world with no clock, no difficulty and no
    /// spawn. `Level`'s constructor stores it and nothing on the placement
    /// path reads it; each accessor throws so that is checked rather than
    /// assumed.
    private static final WritableLevelData DATA = new WritableLevelData() {
        @Override
        public LevelData.RespawnData getRespawnData() {
            throw new UnsupportedOperationException("levelData.getRespawnData");
        }

        @Override
        public void setSpawn(LevelData.RespawnData data) {
            throw new UnsupportedOperationException("levelData.setSpawn");
        }

        @Override
        public long getGameTime() {
            return 0L;
        }

        @Override
        public boolean isHardcore() {
            return false;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public boolean isDifficultyLocked() {
            return false;
        }
    };

    /// A real `Player`, so `getHorizontalDirection`, `getRotation` and
    /// `getNearestLookingDirection` are vanilla's own code rather than a
    /// transcription of it. Creative, because that is the mode this project's
    /// server puts a joining client in.
    static final class HarnessPlayer extends Player {

        HarnessPlayer(Level level) {
            super(level, new GameProfile(java.util.UUID.nameUUIDFromBytes(
                "PlacementGolden".getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                "PlacementGolden"));
        }

        /// Aim the player, and **the head as well as the body**.
        ///
        /// This is the one thing the harness got wrong first, and it was
        /// silent: `getHorizontalDirection()` is `player.getDirection()`,
        /// which reads `yRot`, but `Direction.orderedByNearest` reads
        /// `getViewYRot(1.0F)` — and `LivingEntity` **overrides** that to
        /// return `yHeadRot`. Setting only `yRot` left every
        /// `getNearestLookingDirection()` block answering as though the
        /// player faced south from every yaw, so dispensers and observers
        /// came out with one facing for all four columns while stairs and
        /// furnaces looked perfect. A real player's head and body are both
        /// driven from the move packet, so they agree; a synthetic one has to
        /// be told twice.
        void aim(float yaw, float pitch) {
            setYRot(yaw);
            setXRot(pitch);
            setYHeadRot(yaw);
            setYBodyRot(yaw);
            setOldPosAndRot();
        }

        @Override
        public GameType gameMode() {
            return GameType.CREATIVE;
        }
    }
}
