// ItemSurviveGolden — `BlockState.canSurvive` for every block a BlockItem can place.
//
// # The gap this closes
//
// `BlockItem.canPlace` is
// `(!mustSurvive() || state.canSurvive(level, pos)) && level.isUnobstructed(...)`
// — offsets 5-51 of `BlockItem.canPlace`. This server's placement arm has
// never asked the first half, so a torch placed against air is placed. Every
// one of the 1,054 `BlockItem`s in 26.2 goes through that gate.
//
// `data/vanilla/survival.json` cannot answer it. Its subjects are read out of
// the **datapack** — every state a `would_survive` placement modifier names —
// which is 51 blocks, all of them vegetation, and none of them a torch, a
// sign, a rail, a door or a carpet.
//
// # Why a table and not a reading
//
// Same argument as `SurvivalGolden`, and the same shape of answer: `canSurvive`
// reads the world around a position, so there is no per-state fact to extract.
// What extracts is *the answer over a controlled neighbourhood*, and this
// dumper builds one world per (direction, ground) pair — exactly one non-air
// block, at a named offset from the subject, everything else air — and records
// vanilla's own answer.
//
// A rule that reads one neighbour is then pinned exactly. A rule that reads
// several is still pinned *for these worlds*, which is weaker but is not a
// guess: the world is fully specified, so vanilla's answer in it is a fact
// about the rule even when the harness cannot separate which read produced it.
//
// # The census is the first half of the finding
//
// The declaring class of `canSurvive` is read off the jar by reflection rather
// than written down, because a hand-picked subject list is how the named-noise
// gate came to prove six of sixty-three (ORACLE.md 5b rule 5) and how the
// first `SurvivalGolden` missed `minecraft:bush`. 785 of the 1,054 blocks do
// not override it at all — their `canSurvive` is `BlockBehaviour`'s
// unconditional `true` — and those are emitted as one row each with no sweep,
// so the file says which blocks have no rule rather than leaving that to be
// inferred from an absence.
//
// Converted to JSON by `ci/item-survive-golden.sh`.

// **Default package, like `ItemPropsGolden` and unlike `SurvivalGolden`.**
// The recursive block-tag binder lives on `ItemPropsGolden`, and a class in a
// named package cannot import one from the default package — so being here is
// what makes there be one binder rather than two.

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.StandingAndWallBlockItem;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.tags.BlockTags;
import net.minecraft.tags.FluidTags;

public final class ItemSurviveGolden {

    /// The class every block inherits `canSurvive` from when it has no rule.
    private static final String NO_RULE =
        "net.minecraft.world.level.block.state.BlockBehaviour";

    /// The six offsets a neighbourhood can put its one block at, in
    /// `Direction`'s own declaration order — DOWN, UP, NORTH, SOUTH, WEST,
    /// EAST. Written as the enum rather than as vectors so the order is
    /// vanilla's and not this file's opinion of it.
    private static final Direction[] DIRECTIONS = Direction.values();

    /// What the one non-air block is.
    ///
    /// Chosen to straddle the boundaries these rules actually test, one
    /// entry per boundary and no more:
    ///
    /// * `stone` — a full sturdy cube, the yes-case for almost everything;
    /// * `grass_block` — `#supports_vegetation`, and `dirt` beside it as the
    ///   member that is not the block a sapling is usually on;
    /// * `farmland` — `#supports_vegetation` **and** the crop rule's own
    ///   requirement, and the block whose top face is not a full cube;
    /// * `sand` — `#supports_dry_vegetation` and the cactus rule;
    /// * `soul_sand` — soul fire's, and not a `#supports_vegetation` member;
    /// * `water` — the fluid tests: lily pad, seagrass, kelp, and the
    ///   `getFluidState` half of every rule that has one;
    /// * `glass` — a full cube that is **not** `#solid` for the rules that
    ///   ask about the material rather than the shape;
    /// * `oak_slab` — bottom slab: sturdy on the bottom face and on the top
    ///   of the half that is there, which is the case a `isFaceSturdy` port
    ///   gets wrong if it reads the block instead of the shape;
    /// * `oak_fence` — a post: `canSupportCenter` yes, `isFaceSturdy(UP)` no,
    ///   and that pair is the whole difference between `BaseTorchBlock` and
    ///   `CarpetBlock`;
    /// * `hopper` — sturdy on the sides, not on top;
    /// * `ice` — a full cube nothing may be placed on in several rules;
    /// * `netherrack` — the nether ground, and `#soul_fire_base_blocks`'s
    ///   negative control.
    private static final String[] GROUNDS = {
        "minecraft:stone", "minecraft:grass_block", "minecraft:dirt", "minecraft:farmland",
        "minecraft:sand", "minecraft:soul_sand", "minecraft:water", "minecraft:glass",
        "minecraft:oak_slab", "minecraft:oak_fence", "minecraft:hopper", "minecraft:ice",
        "minecraft:netherrack",
    };

    /// One answer, encoded as one character.
    ///
    /// A row is a string of these rather than a list of objects because the
    /// sweep is 6,340 states by 79 neighbourhoods and a JSON object per
    /// answer would be a 40 MB file nobody would regenerate. The column order
    /// is stated once, in the header, and the reader indexes it.
    /// The two light levels every neighbourhood is swept at.
    ///
    /// `getRawBrightness` is not a property of the block below and this
    /// harness has no light engine, so it is answered by a constant — and one
    /// constant would have made the table wrong about a whole family in
    /// silence. `CropBlock.canSurvive` needs `getRawBrightness(pos, 0) >= 8`
    /// and `MushroomBlock.canSurvive` needs `<= 12`, so at 15 no mushroom
    /// survives anywhere and at 0 no crop does. Both are swept and both are
    /// named in the file's header, so a reader can tell an answer that
    /// depends on light from one that does not by comparing the two strings.
    ///
    /// `canSeeSky` is the other half of the crop rule and is answered
    /// `false` at both levels: a world with one block in it has no sky model,
    /// and inventing one would be the harness deciding a rule's outcome. That
    /// makes the dark column the crop rule's genuine no.
    private static final int[] LIGHT_LEVELS = {15, 0};

    private static final char YES = 'Y';
    private static final char NO = 'N';
    /// The rule read something this harness cannot answer, and the exception
    /// is recorded once in `refusals` rather than per cell.
    private static final char REFUSED = '?';

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // **Without these every tag-reading rule silently answers no**, which
        // is finding 3 of `docs/cold-start/items.md` in a different file: 16
        // of 17 tool `HolderSet`s resolved to zero blocks and the table said
        // every tool mines everything. Here it would say no sapling may be
        // placed on grass. `ItemPropsGolden.bindBlockTags` is the recursive
        // one; `SurfaceChunkGolden.bindBlockTags` is flat and would miss the
        // nested tags.
        ItemPropsGolden.bindBlockTags();
        ItemPropsGolden.bindFluidTags();
        assertTagsAreBound();

        Map<String, Block> subjects = subjects();
        Map<String, Integer> refusals = new TreeMap<>();
        Map<String, Integer> census = new TreeMap<>();

        List<String> rows = new ArrayList<>();
        int overriding = 0;
        int sweptStates = 0;
        for (Map.Entry<String, Block> entry : subjects.entrySet()) {
            Block block = entry.getValue();
            String declaring = declaringClassOfCanSurvive(block);
            census.merge(declaring, 1, Integer::sum);
            // **A second key, because one is not enough.**
            // `VegetationBlock.canSurvive` is `mayPlaceOn(below)` and three
            // of its 48 subclasses override `mayPlaceOn` without overriding
            // `canSurvive` — `LilyPadBlock`, `DeadBushBlock` (via
            // `DryVegetationBlock`) and `CactusFlowerBlock`. A port keyed on
            // the `canSurvive` class alone answers all three with
            // `#supports_vegetation` and is wrong on 144 of this table's
            // cells, which is how this column came to exist.
            String mayPlaceOn = declaringClassOf(block, "mayPlaceOn");
            if (declaring.equals(NO_RULE)) {
                // No rule: one row, no sweep, and the file says so rather
                // than leaving the block out and making its absence mean two
                // things at once.
                rows.add("{\"block\": " + quote(entry.getKey())
                    + ", \"rule\": null, \"mayPlaceOn\": null, \"states\": []}");
                continue;
            }
            overriding++;
            List<String> states = new ArrayList<>();
            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                sweptStates++;
                StringBuilder lit = new StringBuilder();
                StringBuilder dark = new StringBuilder();
                for (String column : columns()) {
                    lit.append(answer(state, column, LIGHT_LEVELS[0], refusals));
                    dark.append(answer(state, column, LIGHT_LEVELS[1], refusals));
                }
                states.add("{\"state\": " + quote(BlockStateParser.serialize(state))
                    + ", \"lit\": " + quote(lit.toString())
                    + ", \"dark\": " + quote(dark.toString()) + "}");
            }
            rows.add("{\"block\": " + quote(entry.getKey())
                + ", \"rule\": " + quote(declaring)
                + ", \"mayPlaceOn\": "
                + (mayPlaceOn == null ? "null" : quote(mayPlaceOn))
                + ", \"states\": [" + String.join(", ", states) + "]}");
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"item-survive\",\n");
        out.append("  \"blocks\": ").append(subjects.size()).append(",\n");
        out.append("  \"overriding\": ").append(overriding).append(",\n");
        out.append("  \"swept_states\": ").append(sweptStates).append(",\n");
        out.append("  \"lit_brightness\": ").append(LIGHT_LEVELS[0]).append(",\n");
        out.append("  \"dark_brightness\": ").append(LIGHT_LEVELS[1]).append(",\n");
        out.append("  \"can_see_sky\": false,\n");
        out.append("  \"columns\": [");
        List<String> quoted = new ArrayList<>();
        for (String column : columns()) {
            quoted.add(quote(column));
        }
        out.append(String.join(", ", quoted)).append("],\n");
        out.append("  \"census\": {\n");
        List<String> censusRows = new ArrayList<>();
        for (Map.Entry<String, Integer> e : census.entrySet()) {
            censusRows.add("    " + quote(e.getKey()) + ": " + e.getValue());
        }
        out.append(String.join(",\n", censusRows)).append("\n  },\n");
        out.append("  \"refusals\": {\n");
        List<String> refusalRows = new ArrayList<>();
        for (Map.Entry<String, Integer> e : refusals.entrySet()) {
            refusalRows.add("    " + quote(e.getKey()) + ": " + e.getValue());
        }
        out.append(String.join(",\n", refusalRows)).append("\n  },\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i)).append(i + 1 < rows.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// `"air"`, then `"<direction>=<ground>"` for every pair, in
    /// `Direction.values()` order outside and `GROUNDS` order inside.
    private static List<String> columns() {
        List<String> columns = new ArrayList<>();
        columns.add("air");
        for (Direction direction : DIRECTIONS) {
            for (String ground : GROUNDS) {
                columns.add(direction.getName() + "=" + ground);
            }
        }
        return columns;
    }

    private static char answer(
        BlockState state, String column, int brightness, Map<String, Integer> refusals
    ) {
        Map<BlockPos, BlockState> world = new LinkedHashMap<>();
        if (!column.equals("air")) {
            String[] halves = column.split("=", 2);
            Direction direction = Direction.byName(halves[0]);
            BlockState ground = BuiltInRegistries.BLOCK
                .getValue(Identifier.parse(halves[1]))
                .defaultBlockState();
            world.put(BlockPos.ZERO.relative(direction), ground);
        }
        try {
            return state.canSurvive(oneBlock(world, brightness), BlockPos.ZERO) ? YES : NO;
        } catch (RuntimeException | StackOverflowError e) {
            String reason = e.getClass().getSimpleName()
                + ": " + (e.getMessage() == null ? "?" : e.getMessage());
            refusals.merge(reason, 1, Integer::sum);
            return REFUSED;
        }
    }

    /// Refuse a run whose tags did not bind, rather than emit a table of
    /// noes.
    ///
    /// An unbound tag is not an exception in this codebase — `HolderSet`
    /// resolves to the empty set and `state.is(tag)` answers `false` — so the
    /// only thing that separates a bound run from an unbound one is a fact
    /// that is true under binding and false without it. Two, one per registry,
    /// picked because they are what the sweep's own rules read:
    /// `#minecraft:supports_vegetation` is what `VegetationBlock.mayPlaceOn`
    /// reads and `#minecraft:water` is what every `isWaterAt` reduces to.
    ///
    /// **The first version of this control asserted `#minecraft:dirt`, and it
    /// fired on a correctly bound run**: in 26.2 that tag is
    /// `{dirt, coarse_dirt, rooted_dirt}` and does not contain
    /// `grass_block`. A control has to be a fact, and a plausible one is not
    /// the same thing — the reading that felt obvious was of an older
    /// version's tag. Read out of
    /// `generated-data/data/minecraft/tags/block/` before trusting either.
    private static void assertTagsAreBound() {
        if (!Blocks.GRASS_BLOCK.defaultBlockState().is(BlockTags.SUPPORTS_VEGETATION)) {
            throw new IllegalStateException(
                "block tags did not bind: grass_block is not in #minecraft:supports_vegetation");
        }
        if (!Blocks.WATER.defaultBlockState().getFluidState().is(FluidTags.WATER)) {
            throw new IllegalStateException(
                "fluid tags did not bind: water is not in #minecraft:water");
        }
    }

    /// Every block a `BlockItem` names, plus the wall block of every
    /// `StandingAndWallBlockItem`.
    ///
    /// **The wall halves are not reachable from `getBlock()`** — a torch item
    /// names `minecraft:torch` and nothing else — and they are exactly the
    /// blocks whose rule decides whether the 48 wall-placeable items may go
    /// where the player aimed. `wallBlock` is `protected final` on
    /// `StandingAndWallBlockItem`, so it is read by reflection here rather
    /// than by re-deriving the pairing from names, which would silently miss
    /// any pair vanilla does not spell that way.
    private static Map<String, Block> subjects() {
        Set<Block> blocks = new LinkedHashSet<>();
        for (Item item : BuiltInRegistries.ITEM) {
            if (!(item instanceof BlockItem blockItem)) {
                continue;
            }
            blocks.add(blockItem.getBlock());
            if (item instanceof StandingAndWallBlockItem) {
                blocks.add(wallBlockOf((StandingAndWallBlockItem) item));
            }
        }
        Map<String, Block> byName = new TreeMap<>();
        for (Block block : blocks) {
            byName.put(BuiltInRegistries.BLOCK.getKey(block).toString(), block);
        }
        return byName;
    }

    private static Block wallBlockOf(StandingAndWallBlockItem item) {
        for (java.lang.reflect.Field field : StandingAndWallBlockItem.class.getDeclaredFields()) {
            if (!Block.class.isAssignableFrom(field.getType())) {
                continue;
            }
            field.setAccessible(true);
            try {
                return (Block) field.get(item);
            } catch (IllegalAccessException e) {
                throw new IllegalStateException("cannot read wallBlock", e);
            }
        }
        throw new IllegalStateException("StandingAndWallBlockItem has no Block field");
    }

    /// Which class in the block's hierarchy declares `canSurvive`.
    ///
    /// `BlockBehaviour`'s is `return true`, so a block that reaches it has no
    /// rule at all. Read by reflection because the alternative is a list of
    /// fifty class names in this file that goes stale at the next version.
    private static String declaringClassOfCanSurvive(Block block) {
        String declaring = declaringClassOf(block, "canSurvive");
        if (declaring == null) {
            throw new IllegalStateException("no canSurvive anywhere above " + block.getClass());
        }
        return declaring;
    }

    /// Which class in the block's hierarchy declares a
    /// `(BlockState, BlockGetter-or-LevelReader, BlockPos)` method of this
    /// name, or `null` if none does.
    ///
    /// Matched by name and arity rather than by exact parameter types,
    /// because `canSurvive` takes a `LevelReader` and `mayPlaceOn` a
    /// `BlockGetter` and pinning both signatures here would be two more
    /// things to keep in step with the jar.
    private static String declaringClassOf(Block block, String method) {
        Class<?> c = block.getClass();
        while (c != null) {
            for (java.lang.reflect.Method candidate : c.getDeclaredMethods()) {
                if (candidate.getName().equals(method) && candidate.getParameterCount() == 3) {
                    return c.getName();
                }
            }
            c = c.getSuperclass();
        }
        return null;
    }

    /// A `LevelReader` over a fully specified world: the map, and air
    /// everywhere else.
    ///
    /// Unlike `SurvivalGolden.oneBlockBelow` this does **not** throw for a
    /// read outside one position. The world here is a real world — one block
    /// and air — so every position has a true answer and refusing to give it
    /// would turn a multi-read rule into a hole rather than into a fact. What
    /// still throws is a `LevelReader` method with no answer at all, and the
    /// caller records that as a refusal.
    private static LevelReader oneBlock(Map<BlockPos, BlockState> world, int brightness) {
        return (LevelReader) java.lang.reflect.Proxy.newProxyInstance(
            ItemSurviveGolden.class.getClassLoader(),
            new Class<?>[]{LevelReader.class},
            (proxy, method, args) -> {
                String name = method.getName();
                if (name.equals("getBlockState") && method.getParameterCount() == 1) {
                    return world.getOrDefault(args[0], Blocks.AIR.defaultBlockState());
                }
                if (name.equals("getFluidState") && method.getParameterCount() == 1) {
                    return world.getOrDefault(args[0], Blocks.AIR.defaultBlockState())
                        .getFluidState();
                }
                if (name.equals("getBlockEntity")) {
                    return null;
                }
                // Three `LevelReader` **default** methods that a proxy
                // intercepts along with the abstract ones, and which the
                // first run of this dumper refused 4,278 cells over. Each is
                // its own interface default, transcribed rather than guessed:
                // `isEmptyBlock` is `getBlockState(pos).isAir()`, `isWaterAt`
                // is `getFluidState(pos).is(FluidTags.WATER)`, and
                // `getRawBrightness` is the constant this sweep declares.
                if (name.equals("isEmptyBlock") && method.getParameterCount() == 1) {
                    return world.getOrDefault(args[0], Blocks.AIR.defaultBlockState()).isAir();
                }
                if (name.equals("isWaterAt") && method.getParameterCount() == 1) {
                    FluidState fluid = world
                        .getOrDefault(args[0], Blocks.AIR.defaultBlockState())
                        .getFluidState();
                    return fluid.is(FluidTags.WATER);
                }
                if (name.equals("getRawBrightness") && method.getParameterCount() == 2) {
                    return brightness;
                }
                if (name.equals("canSeeSky") && method.getParameterCount() == 1) {
                    return false;
                }
                if (name.equals("getMinY")) {
                    return -64;
                }
                if (name.equals("getHeight") && method.getParameterCount() == 0) {
                    return 384;
                }
                if (name.equals("getMaxLightLevel")) {
                    return 15;
                }
                if (name.equals("isClientSide")) {
                    return false;
                }
                if (name.equals("toString")) {
                    return "oneBlock" + world + "@" + brightness;
                }
                if (name.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (name.equals("equals")) {
                    return proxy == args[0];
                }
                throw new UnsupportedOperationException(
                    "canSurvive read LevelReader." + name + "/" + method.getParameterCount());
            });
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private ItemSurviveGolden() {}
}
