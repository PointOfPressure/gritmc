// UseItemOnGolden — extract `BlockState.useItemOn` per state and held item.
//
// # What this answers, and why it is a second table rather than a column
//
// `ServerPlayerGameMode.useItemOn` is one straight line and the block gets
// **two** offers, not one. From the bytecode of the pinned jar:
//
//     126      if (secondary && holdingSomething) goto 223     // sneak + holding
//     145      r  = state.useItemOn(stack, level, player, hand, hit)
//     152-157  if r.consumesAction()                    -> r
//     179-187  if r instanceof TryEmptyHandInteraction && hand == MAIN_HAND:
//     196          r2 = state.useWithoutItem(level, player, hit)
//     203-208      if r2.consumesAction()               -> r2
//     223      if stack.isEmpty() || onCooldown         -> PASS
//     245      ctx = new UseOnContext(player, hand, hit)
//     275/292  r3 = stack.useOn(ctx)
//
// `UseGolden` extracts offset 196. This extracts offset 145 — the one that
// runs **first**, and the one `use_without_item.json` therefore describes the
// aftermath of rather than the whole of.
//
// Two facts make that gap load-bearing rather than tidy:
//
//   * **`TRY_WITH_EMPTY_HAND` is not the only pass-like answer, and the others
//     are not equivalent.** Only `TryEmptyHandInteraction` reaches
//     `useWithoutItem` at all (offset 179). A `useItemOn` that returns `PASS`
//     or `FAIL` *skips* it, so a table that records only offset 196 cannot say
//     whether offset 196 ran. That is why `interesting()` here turns on the
//     result being anything other than `TRY_WITH_EMPTY_HAND`, where
//     `UseGolden`'s turns on `consumesAction()`.
//   * **`FlowerPotBlock` is the worked example.** `useWithoutItem`'s offset 0
//     is `if (isEmpty()) return CONSUME`, so porting it converts nothing for a
//     scenario whose *potting* is a `useItemOn`: the pot is empty at click
//     time and the flower goes in at offset 145. 39 blocks share that
//     implementation and none of them are described by the other table.
//
// # The two axes, and which one is new
//
// `useWithoutItem` is handed the level, the player and the hit. `useItemOn` is
// handed **the stack as well**, and dispatches on it: `stack.getItem()`,
// `stack.is(TagKey)`, `stack.isEmpty()`, `stack.getCount()`. So the question
// is per (state, item) and the item axis is the **whole item registry**, 1,537
// entries, minted as `new ItemStack(item, 1)`. No hand-written list of
// "relevant" items: a list is exactly the thing that silently loses a member,
// and this project has already paid for one (`copper_hoe`, new in 26.2, is the
// seventh hoe in a place six were expected).
//
// Recording 1,537 cells per state would be absurd and almost all of them
// identical, so each state is emitted as **one `default` cell plus the
// exceptions**, grouped by identical answer. That is a compression of the
// measurement, not a sampling of it: every item is probed on every state.
//
// # What is recorded
//
// Per (state, item): every `setBlock` the call makes — offset from the clicked
// position, the state written and the flags — the `InteractionResult` by its
// **constant name** (`SUCCESS`, `SUCCESS_SERVER`, `CONSUME`, `FAIL`, `PASS`,
// `TRY_WITH_EMPTY_HAND`, and `Success(...)` for one built on the spot), any
// scheduled tick, whether a sound was played, and **what happened to the
// stack**. The last is not decoration: `FlowerPotBlock` ends with
// `stack.consume(1, player)` and `DecoratedPotBlock` with `consumeAndReturn`,
// and a consumer that plants the flower without taking it has duplicated an
// item.
//
// A block that reads something this harness does not model **throws and is
// recorded as a refusal naming the method**, exactly as in `UseGolden`.
// Signs, lecterns, jukeboxes, chiseled bookshelves and shelves are all
// expected to refuse: they want a block entity.
//
// # Which blocks are probed, and how that set is checked
//
// `BlockBehaviour.useItemOn` is `return TRY_WITH_EMPTY_HAND` at offset 0, so a
// block whose class does not override it cannot answer anything else. The set
// that does override is found by walking each block's superclass chain for a
// declared `useItemOn` — which is precisely what the JVM dispatches on, not an
// approximation of it — and comes to **25 classes over 163 blocks and 4,537
// states** on the pinned jar.
//
// That is still a decision about what not to measure, so it is falsified
// rather than asserted: every one of the remaining 1,033 blocks is probed with
// the **entire item registry** on its first state, and any interesting answer
// is a hard failure. One state is the whole of the answer *if* the dispatch is
// what this claims — which is the claim being checked, and the check costs 1.6
// million probes and about a second.
//
// # Grep any diagnostic output unanchored
//
// `Bootstrap.bootStrap()` redirects both streams through log4j, which re-emits
// each line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so
// `grep -E "^SOMETHING"` matches nothing and exits 1 — indistinguishable from
// a clean run that found nothing. That prefix is also why this writes its JSON
// to stdout and is captured with `>` rather than through a pipe.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

public final class UseItemOnGolden {

    /// The faces a right-click carries. Two, for the same reason `UseGolden`
    /// uses two: a block whose answer depends on the face is caught by the
    /// second disagreeing with the first. `useItemOn` reads the face far more
    /// than `useWithoutItem` does — `NoteBlock` gates its whole rule on
    /// `hit.getDirection() == UP`, and `ChiseledBookShelfBlock` and
    /// `ShelfBlock` turn the cursor position into a slot index — so this axis
    /// is expected to be live here where it was nearly dead there.
    private static final Direction[] FACES = {Direction.UP, Direction.NORTH};

    /// Both hands. `ShelfBlock.useItemOn` opens with
    /// `if (hand.equals(InteractionHand.OFF_HAND)) return PASS`, and
    /// `ServerPlayerGameMode.useItemOn` is reached with either — so a table
    /// taken in one hand states a fact about that hand.
    private static final InteractionHand[] HANDS =
        {InteractionHand.MAIN_HAND, InteractionHand.OFF_HAND};

    /// The signature the JVM dispatches `BlockState.useItemOn` through.
    private static Class<?>[] signature() {
        return new Class<?>[] {
            ItemStack.class, BlockState.class, Level.class, BlockPos.class,
            Player.class, InteractionHand.class, BlockHitResult.class,
        };
    }

    /// The class that actually runs for `block`, or null if none does.
    ///
    /// Reflection over the pinned jar, and exact rather than approximate: this
    /// is the same walk virtual dispatch performs. `BlockBehaviour` declaring
    /// it means the block has no rule of its own.
    private static Class<?> declarer(Class<?> c, Class<?>[] sig) {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                k.getDeclaredMethod("useItemOn", sig);
                return k;
            } catch (NoSuchMethodException ignored) {
                continue;
            }
        }
        return null;
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        UseGolden.armVanillasOwnAlarm();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        // **Both of these are preconditions of this table existing at all**,
        // not tidying. Without the first there is no `ItemStack` to hold;
        // without the second `ItemStack.is(TagKey)` is false for every item in
        // the game and five of the twenty-five classes below answer "does
        // nothing" to everything, silently. See `UseGolden` for both.
        UseGolden.bindItemComponents(lookup);
        UseGolden.bindItemTags();
        // The `instanceof ServerLevel` gates, derived from the bytecode by
        // `ci/server-level-gates.py --emit` because nothing inside a run can
        // observe them. Loaded before the first probe, and throwing rather
        // than defaulting: without it 2,784 sign cells and one pumpkin cell
        // go back to saying they were measured.
        UseGolden.ServerLevelGate.load("useItemOn");
        Holder<DimensionType> overworld = lookup
            .lookupOrThrow(Registries.DIMENSION_TYPE)
            .getOrThrow(BuiltinDimensionTypes.OVERWORLD);

        UseGolden.TableLevel level = new UseGolden.TableLevel(UseGolden.accessOf(lookup), overworld);
        UseGolden.HarnessPlayer player = new UseGolden.HarnessPlayer(level);
        UseGolden.pinHome(player);
        UseGolden.mintFiller();
        if (UseGolden.FILLER == null) {
            throw new IllegalStateException(
                "no ItemStack could be minted after binding components: " + UseGolden.FILLER_WHY);
        }

        // The item axis, in registry order, built once. `minecraft:air` is in
        // the registry and mints the empty stack, which is the stack a player
        // with nothing in hand actually carries — so the empty-handed case is
        // measured here rather than assumed to be `useWithoutItem`'s.
        List<Item> items = new ArrayList<>();
        List<String> itemNames = new ArrayList<>();
        List<ItemStack> stacks = new ArrayList<>();
        for (Item item : BuiltInRegistries.ITEM) {
            items.add(item);
            itemNames.add(BuiltInRegistries.ITEM.getKey(item).toString());
            stacks.add(new ItemStack(item, 1));
        }

        Class<?>[] sig = signature();
        List<Block> overriding = new ArrayList<>();
        List<Block> plain = new ArrayList<>();
        Map<String, Integer> classStates = new TreeMap<>();
        int overridingStates = 0;
        for (Block block : BuiltInRegistries.BLOCK) {
            Class<?> d = declarer(block.getClass(), sig);
            if (d == null || d == BlockBehaviour.class) {
                plain.add(block);
                continue;
            }
            overriding.add(block);
            int n = block.getStateDefinition().getPossibleStates().size();
            overridingStates += n;
            classStates.merge(d.getSimpleName(), n, Integer::sum);
        }

        List<String> rows = new ArrayList<>();
        int blocksWithBehaviour = 0;
        int statesProbed = 0;
        int statesRecorded = 0;
        int cells = 0;
        int refusedStates = 0;
        int partlyRefusedStates = 0;
        int blocksAllRefused = 0;
        int randomBlocks = 0;
        int probeDependentBlocks = 0;
        int probeOnlyBlocks = 0;
        int faceSensitiveBlocks = 0;
        int handSensitiveBlocks = 0;
        long gridProbes = 0L;
        int gatedBlocks = 0;
        int gatedCells = 0;
        Map<String, Integer> refusedBy = new TreeMap<>();

        for (Block block : overriding) {
            Identifier id = BuiltInRegistries.BLOCK.getKey(block);
            String cls = block.getClass().getSimpleName();
            List<String> stateJson = new ArrayList<>();
            boolean any = false;
            boolean faceSensitive = false;
            boolean handSensitive = false;
            boolean random = false;
            int stateRefused = 0;
            Set<String> probeAxes = new TreeSet<>();
            Set<String> probeUnknown = new TreeSet<>();

            // Resolved once per block by walking its own class chain, not
            // from a transcribed list of ids: `SignBlock` covers 48 of the 56
            // and a list of them here would be a second copy of what the
            // registry already knows and the ledger already derives.
            currentGate = UseGolden.ServerLevelGate.of(block);
            if (currentGate != null) {
                gatedBlocks++;
            }
            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                statesProbed++;
                // ---- the grid, base stance -------------------------------
                //
                // State-major and item-inner: the stance is applied once per
                // (state, stance) rather than once per probe, which is what
                // makes 84 million probes affordable. Every scalar axis is
                // still set on every application — see `reapply` — and the
                // inventory, the one axis a probe can dirty, is restored the
                // moment a probe does anything at all.
                Shot[] base = new Shot[stacks.size()];
                reapply(UseGolden.STANCES[0], player);
                for (int i = 0; i < stacks.size(); i++) {
                    base[i] = shoot(level, player, state, FACES[0], HANDS[0],
                        stacks.get(i), UseGolden.SEEDS[0]);
                    gridProbes++;
                    if (base[i].interesting()) {
                        reapply(UseGolden.STANCES[0], player);
                    }
                }

                // ---- the sweeps, on the axes that can move ---------------
                for (int st = 1; st < UseGolden.STANCES.length; st++) {
                    if (!UseGolden.STANCES[st].available()) {
                        continue;
                    }
                    reapply(UseGolden.STANCES[st], player);
                    for (int i = 0; i < stacks.size(); i++) {
                        Shot q = shoot(level, player, state, FACES[0], HANDS[0],
                            stacks.get(i), UseGolden.SEEDS[0]);
                        gridProbes++;
                        if (q.interesting()) {
                            reapply(UseGolden.STANCES[st], player);
                        }
                        if (q.same(base[i])) {
                            continue;
                        }
                        // "measured to move" and "could not be measured" are
                        // different findings and are kept apart, exactly as
                        // `UseGolden` keeps them apart: a stance that refused
                        // where `base` did not is charged to this harness's
                        // level model, not to the block.
                        if (base[i].refusal == null && q.refusal != null) {
                            probeUnknown.add(UseGolden.STANCES[st].name);
                        } else {
                            probeAxes.add(UseGolden.STANCES[st].name);
                        }
                    }
                }

                reapply(UseGolden.STANCES[0], player);
                for (int i = 0; i < stacks.size(); i++) {
                    if (!base[i].interesting()) {
                        continue;
                    }
                    // The second face and the off hand, on the interesting
                    // set only. A pair that does nothing under the base face
                    // in the main hand and something under another is not
                    // reachable: `useItemOn` is dispatched on the block and
                    // the stack, and both are fixed here — the face and the
                    // hand can only ever narrow what a rule does, never
                    // create one. `NoteBlock` (`== UP`) and `ShelfBlock`
                    // (`== OFF_HAND`) are both that shape.
                    Shot other = shoot(level, player, state, FACES[1], HANDS[0],
                        stacks.get(i), UseGolden.SEEDS[0]);
                    gridProbes++;
                    reapply(UseGolden.STANCES[0], player);
                    if (!other.same(base[i])) {
                        faceSensitive = true;
                    }
                    Shot off = shoot(level, player, state, FACES[0], HANDS[1],
                        stacks.get(i), UseGolden.SEEDS[0]);
                    gridProbes++;
                    reapply(UseGolden.STANCES[0], player);
                    if (!off.same(base[i])) {
                        handSensitive = true;
                    }
                    // The seed sweep, on the pairs that drew from the level
                    // RNG and no others. `TableLevel.getRandom` sets
                    // `usedRandom`, so this is exact rather than a screen: a
                    // rule that never asked for a random number cannot answer
                    // differently under another seed.
                    if (base[i].usedRandom) {
                        for (int s = 1; s < UseGolden.SEEDS.length; s++) {
                            Shot r = shoot(level, player, state, FACES[0], HANDS[0],
                                stacks.get(i), UseGolden.SEEDS[s]);
                            gridProbes++;
                            reapply(UseGolden.STANCES[0], player);
                            if (!r.same(base[i])) {
                                random = true;
                            }
                        }
                    }
                }

                // ---- emit -------------------------------------------------
                //
                // One `default` cell and the exceptions. Grouped by identical
                // answer so a block that says the same thing to 1,500 items
                // costs one line and the fifteen that differ cost fifteen.
                Map<String, List<Integer>> byAnswer = new LinkedHashMap<>();
                for (int i = 0; i < base.length; i++) {
                    byAnswer.computeIfAbsent(base[i].cellJson(), k -> new ArrayList<>()).add(i);
                }
                String defaultAnswer = null;
                int biggest = -1;
                for (Map.Entry<String, List<Integer>> e : byAnswer.entrySet()) {
                    if (e.getValue().size() > biggest) {
                        biggest = e.getValue().size();
                        defaultAnswer = e.getKey();
                    }
                }
                boolean stateInteresting = false;
                for (Shot s : base) {
                    if (s.interesting()) {
                        stateInteresting = true;
                        break;
                    }
                }
                if (!stateInteresting) {
                    continue;
                }
                any = true;
                statesRecorded++;
                StringBuilder sb = new StringBuilder();
                sb.append("{\"from\": ").append(UseGolden.quote(BlockStateParser.serialize(state)))
                    .append(", \"default\": ").append(defaultAnswer)
                    .append(", \"except\": [");
                int written = 0;
                for (Map.Entry<String, List<Integer>> e : byAnswer.entrySet()) {
                    if (e.getKey().equals(defaultAnswer)) {
                        continue;
                    }
                    List<String> names = new ArrayList<>();
                    for (int i : e.getValue()) {
                        names.add(itemNames.get(i));
                    }
                    cells += names.size();
                    if (written++ > 0) {
                        sb.append(", ");
                    }
                    sb.append("{\"items\": [").append(UseGolden.quotedList(names))
                        .append("], \"cell\": ").append(e.getKey()).append("}");
                }
                sb.append("]}");
                stateJson.add(sb.toString());
                // **Counted over recorded answers, one per default and one
                // per group**, which is the definition `ci/server-level-
                // gates.py` uses and deliberately not this file's own `cells`
                // — that counts (state, item) grid points differing from
                // their default, 44,078 of them, and the two are not
                // interchangeable. The cross-check in `--check` compares this
                // number against a walk of the jar, so the two sides have to
                // be counting the same objects.
                if (defaultAnswer.contains("\"gated\":")) {
                    gatedCells++;
                }
                for (Map.Entry<String, List<Integer>> e : byAnswer.entrySet()) {
                    if (!e.getKey().equals(defaultAnswer)
                            && e.getKey().contains("\"gated\":")) {
                        gatedCells++;
                    }
                }
                // **Refusals are counted per STATE, not per cell.** Ten of
                // the twenty-five classes refuse for every item on every
                // state, so a per-cell count is the item registry multiplied
                // by the state count — 3.96 million on the pinned jar — and a
                // number that large in a `refusedBy` histogram is measuring
                // the width of the item axis rather than the coverage of the
                // table. A state is refused when *every* item was refused on
                // it, and partly refused when some were; the two are
                // different findings and are counted apart.
                int refusedCells = 0;
                int totalCells = 0;
                String firstRefusal = null;
                for (Shot s : base) {
                    totalCells++;
                    if (s.refusal != null) {
                        refusedCells++;
                        if (firstRefusal == null) {
                            firstRefusal = s.refusal;
                        }
                    }
                }
                if (refusedCells == totalCells) {
                    refusedStates++;
                    stateRefused++;
                    refusedBy.merge(firstRefusal, 1, Integer::sum);
                } else if (refusedCells > 0) {
                    partlyRefusedStates++;
                    refusedBy.merge(firstRefusal, 1, Integer::sum);
                }
            }

            String probeCols = ", \"probeDependent\": " + !probeAxes.isEmpty()
                + ", \"probeAxes\": [" + UseGolden.quotedList(probeAxes) + "]"
                + ", \"probeUnknown\": [" + UseGolden.quotedList(probeUnknown) + "]";
            if (!any && probeAxes.isEmpty() && probeUnknown.isEmpty()) {
                continue;
            }
            if (any) {
                blocksWithBehaviour++;
                if (stateRefused == stateJson.size() && stateRefused > 0) {
                    blocksAllRefused++;
                }
            } else {
                // A block the base probe cannot see at all — every (state,
                // item) was uninteresting under `STANCES[0]` and some stance
                // made one interesting. Emitted with no states rather than
                // left out: an absent row is indistinguishable from a block
                // with no rule.
                probeOnlyBlocks++;
            }
            if (!probeAxes.isEmpty()) {
                probeDependentBlocks++;
            }
            if (random) {
                randomBlocks++;
            }
            if (faceSensitive) {
                faceSensitiveBlocks++;
            }
            if (handSensitive) {
                handSensitiveBlocks++;
            }
            rows.add("    {\"block\": " + UseGolden.quote(id.toString())
                + ", \"class\": " + UseGolden.quote(cls)
                + (currentGate == null ? "" : currentGate.json())
                + ", \"random\": " + random
                + ", \"faceSensitive\": " + faceSensitive
                + ", \"handSensitive\": " + handSensitive
                + probeCols
                + (random
                    // No cells, deliberately, and for `UseGolden`'s reason: a
                    // consumer that read one sample would reproduce a state
                    // vanilla only sometimes writes.
                    ? ", \"states\": []}"
                    : ", \"states\": [\n      " + String.join(",\n      ", stateJson) + "\n    ]}"));
        }

        // ---- the control ------------------------------------------------
        //
        // Every block this run decided not to probe, against the entire item
        // registry, on its first state. `BlockBehaviour.useItemOn` ignores
        // the state — `return TRY_WITH_EMPTY_HAND` at offset 0 — so one state
        // is the whole of the answer if the dispatch is what the reflective
        // walk says it is, which is exactly the claim being tested.
        //
        // A hit here is not a warning. It means a block with a real use rule
        // was skipped, and the table would be silently missing it.
        List<String> unexpected = new ArrayList<>();
        long controlProbes = 0L;
        // **Cleared, or the control inherits the last probed block's gate.**
        // `interesting()` counts a stamp, so a stale `currentGate` would make
        // every plain block under `SignBlock`'s chain look like a rule the
        // reflective walk missed — an alarm firing on this file's own state.
        currentGate = null;
        reapply(UseGolden.STANCES[0], player);
        for (Block block : plain) {
            BlockState first = block.getStateDefinition().getPossibleStates().get(0);
            for (int i = 0; i < stacks.size(); i++) {
                Shot s = shoot(level, player, first, FACES[0], HANDS[0],
                    stacks.get(i), UseGolden.SEEDS[0]);
                controlProbes++;
                if (s.interesting()) {
                    unexpected.add(BuiltInRegistries.BLOCK.getKey(block) + " with "
                        + itemNames.get(i));
                    reapply(UseGolden.STANCES[0], player);
                }
            }
        }
        if (!unexpected.isEmpty()) {
            throw new IllegalStateException(
                "the reflective override set is not the whole answer: " + unexpected.size()
                + " (block, item) pairs answered something other than TRY_WITH_EMPTY_HAND on a "
                + "block whose class does not declare useItemOn, e.g. "
                + unexpected.subList(0, Math.min(5, unexpected.size())));
        }

        // **The alarm against itself.** Every piece of the gate column can
        // be present and inert: the file loads, the class chain matches
        // nothing, `reaches` is never called, and every row says exactly what
        // it said before. That is the `blockEntityAsked` shape — set on every
        // ask and read by nobody from March to August — and the only defence
        // is a counter that has to move.
        if (UseGolden.ServerLevelGate.resolved() == 0 || gatedCells == 0) {
            throw new IllegalStateException(
                "the ServerLevel gate column resolved "
                + UseGolden.ServerLevelGate.resolved() + " block(s) and stamped "
                + gatedCells + " cell(s) — the pinned jar has seven gated "
                + "useItemOn rules over 56 registry blocks, so a zero here is "
                + "a broken pipeline and not a clean world");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"what\": \"BlockState.useItemOn, per state and held item\",\n");
        out.append("  \"itemsProbed\": ").append(stacks.size()).append(",\n");
        out.append("  \"blocksProbed\": ").append(overriding.size()).append(",\n");
        out.append("  \"blocksSkipped\": ").append(plain.size()).append(",\n");
        out.append("  \"statesProbed\": ").append(statesProbed).append(",\n");
        out.append("  \"statesRecorded\": ").append(statesRecorded).append(",\n");
        out.append("  \"cells\": ").append(cells).append(",\n");
        out.append("  \"gridProbes\": ").append(gridProbes).append(",\n");
        out.append("  \"controlProbes\": ").append(controlProbes).append(",\n");
        out.append("  \"blocksWithBehaviour\": ").append(blocksWithBehaviour).append(",\n");
        out.append("  \"refusedStates\": ").append(refusedStates).append(",\n");
        out.append("  \"partlyRefusedStates\": ").append(partlyRefusedStates).append(",\n");
        out.append("  \"blocksAllRefused\": ").append(blocksAllRefused).append(",\n");
        out.append("  \"randomBlocks\": ").append(randomBlocks).append(",\n");
        out.append("  \"probeDependentBlocks\": ").append(probeDependentBlocks).append(",\n");
        out.append("  \"probeOnlyBlocks\": ").append(probeOnlyBlocks).append(",\n");
        out.append("  \"faceSensitiveBlocks\": ").append(faceSensitiveBlocks).append(",\n");
        out.append("  \"handSensitiveBlocks\": ").append(handSensitiveBlocks).append(",\n");
        out.append("  \"serverLevelGatedBlocks\": ").append(gatedBlocks).append(",\n");
        out.append("  \"serverLevelGatedCells\": ").append(gatedCells).append(",\n");
        out.append("  \"overridingClasses\": {");
        int n = 0;
        for (Map.Entry<String, Integer> e : classStates.entrySet()) {
            if (n++ > 0) {
                out.append(", ");
            }
            out.append(UseGolden.quote(e.getKey())).append(": ").append(e.getValue());
        }
        out.append("},\n");
        // The stance every recorded cell was taken under, written into the
        // file — see `UseGolden`. A reader that wants to know whether a row is
        // a fact about the block or a fact about one player can read what that
        // player was.
        out.append("  \"probe\": ").append(UseGolden.STANCES[0].json()).append(",\n");
        out.append("  \"probeStances\": [\n");
        for (int i = 1; i < UseGolden.STANCES.length; i++) {
            out.append("    ").append(UseGolden.STANCES[i].json())
                .append(i < UseGolden.STANCES.length - 1 ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"probeStancesUnavailable\": {");
        List<String> gone = new ArrayList<>();
        for (UseGolden.Stance s : UseGolden.STANCES) {
            if (!s.available()) {
                gone.add(UseGolden.quote(s.name) + ": "
                    + UseGolden.quote(String.valueOf(UseGolden.FILLER_WHY)));
            }
        }
        out.append(String.join(", ", gone)).append("},\n");
        out.append("  \"refusedBy\": {\n");
        int written = 0;
        for (Map.Entry<String, Integer> e : refusedBy.entrySet()) {
            written++;
            out.append("    ").append(UseGolden.quote(e.getKey())).append(": ").append(e.getValue())
                .append(written < refusedBy.size() ? ",\n" : "\n");
        }
        out.append("  },\n  \"table\": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }

    /// Put the player back into the stance, every axis of it.
    ///
    /// `UseGolden.probe` does this once per probe. This dumper cannot: at 84
    /// million probes, `Stance.apply`'s inventory clear-and-refill is the
    /// whole of the run time. So it is called at every **stance change** and
    /// immediately after **any probe that did anything at all**, which is what
    /// keeps the player an input rather than an accumulator.
    ///
    /// That is sound rather than merely cheap, and the reason is worth stating
    /// because the shortcut looks like the kind this project keeps finding: a
    /// probe that is not `interesting()` wrote no block, scheduled no tick,
    /// granted no item and returned `TRY_WITH_EMPTY_HAND` with the stack it
    /// was given. `HarnessPlayer.granted` is recorded at the accessor, not
    /// inferred, so "granted nothing" is a measurement — and a rule that
    /// mutated the player some fourth way would have to do it without
    /// touching `addItem`, `drop`, `setItemInHand` or `getInventory`.
    private static void reapply(UseGolden.Stance stance, UseGolden.HarnessPlayer player) {
        stance.apply(player);
    }

    /// One right-click with something in hand, and what it did.
    private static final class Shot {
        InteractionResult result;
        Map<BlockPos, UseGolden.Write> writes = new LinkedHashMap<>();
        boolean scheduled;
        long tickDelay = -1L;
        String tickPriority;
        BlockPos tickAt;
        String tickKind;
        boolean sounded;
        /// The `LevelEvent` type raised, or -1. A packet, not a nicety —
        /// `ComposterBlock` raises one of two from the same rule.
        int levelEvent = -1;
        boolean usedRandom;
        /// The held stack after the call, when it is not what went in.
        String stackAfter;
        /// The stack a `Success` says the held one turned into, when any.
        String transformed;
        /// What the rule handed the player — see `HarnessPlayer.granted`.
        String granted;
        /// Whether the rule read the player's inventory at all. Not part of
        /// `interesting()`: reading is not doing. Carried because a consumer
        /// that sees it knows the recorded answer is a function of an
        /// inventory this harness chose.
        boolean readsInventory;
        /// What this harness answered out of its own emptiness — see
        /// `UseGolden.Probe.reads`, whose vocabulary this shares.
        final java.util.TreeSet<String> reads = new java.util.TreeSet<>();
        String refusal;

        /// Whether the row says anything a consumer needs.
        ///
        /// **Deliberately not `UseGolden.Probe.interesting()`.** That one turns
        /// on `consumesAction()`, which is right for offset 196 — the last
        /// call in the chain, whose only job is to answer yes or no. This is
        /// offset 145, and the difference between `TRY_WITH_EMPTY_HAND` and
        /// `PASS` decides whether offset 196 runs at all. A table that called
        /// both "not interesting" could not describe that.
        boolean interesting() {
            return refusal != null
                || !writes.isEmpty()
                || scheduled
                || granted != null
                || stackAfter != null
                || !gated.isEmpty()
                || !reads.isEmpty()
                || !(result instanceof InteractionResult.TryEmptyHandInteraction);
        }

        /// Columns of **this cell** that the `instanceof ServerLevel` branch
        /// decided rather than the block. Derived outside the run by
        /// `ci/server-level-gates.py` and evaluated against this cell's real
        /// state and stack by [`UseGolden.ServerLevelGate.reaches`].
        ///
        /// It is in `cellJson` and therefore in `same`, deliberately. Every
        /// shot of one `(state, item)` pair carries the identical stamp — the
        /// gate does not depend on face, hand, stance or seed — so it can
        /// never make an axis look sensitive. What it *does* do is split a
        /// gated cell out of the default it would otherwise have merged into,
        /// which is the whole of the `BeehiveBlock` finding: shears at
        /// `honey_level=5` answer exactly what the default answers, because a
        /// rule that ran nothing and a rule that does nothing are the same
        /// answer, and only the stamp tells them apart.
        final java.util.TreeSet<String> gated = new java.util.TreeSet<>();

        boolean same(Shot other) {
            return cellJson().equals(other.cellJson());
        }

        private String cached;

        String cellJson() {
            if (cached != null) {
                return cached;
            }
            StringBuilder b = new StringBuilder("{");
            if (refusal != null) {
                b.append("\"refused\": ").append(UseGolden.quote(refusal)).append("}");
                return cached = b.toString();
            }
            b.append("\"result\": ").append(UseGolden.quote(resultName(result)));
            b.append(", \"consumes\": ").append(result != null && result.consumesAction());
            b.append(", \"schedulesTick\": ").append(scheduled);
            if (scheduled) {
                b.append(", \"tickDelay\": ").append(tickDelay);
                b.append(", \"tickPriority\": ").append(UseGolden.quote(String.valueOf(tickPriority)));
                b.append(", \"tickAt\": [").append(tickAt == null ? "" : tickAt.getX()
                    + ", " + tickAt.getY() + ", " + tickAt.getZ()).append("]");
                b.append(", \"tickKind\": ").append(UseGolden.quote(String.valueOf(tickKind)));
            }
            b.append(", \"sound\": ").append(sounded);
            if (!reads.isEmpty()) {
                b.append(", \"reads\": [").append(UseGolden.quotedList(reads)).append("]");
            }
            if (levelEvent >= 0) {
                b.append(", \"levelEvent\": ").append(levelEvent);
            }
            if (stackAfter != null) {
                b.append(", \"stackAfter\": ").append(UseGolden.quote(stackAfter));
            }
            if (transformed != null) {
                b.append(", \"transformed\": ").append(UseGolden.quote(transformed));
            }
            if (granted != null) {
                b.append(", \"granted\": ").append(UseGolden.quote(granted));
            }
            if (readsInventory) {
                b.append(", \"readsInventory\": true");
            }
            if (!gated.isEmpty()) {
                b.append(", \"gated\": [").append(UseGolden.quotedList(gated)).append("]");
            }
            b.append(", \"writes\": [");
            int n = 0;
            for (Map.Entry<BlockPos, UseGolden.Write> e : writes.entrySet()) {
                BlockPos d = e.getKey();
                if (n++ > 0) {
                    b.append(", ");
                }
                b.append("{\"at\": [").append(d.getX()).append(", ").append(d.getY())
                    .append(", ").append(d.getZ()).append("], \"state\": ")
                    .append(UseGolden.quote(BlockStateParser.serialize(e.getValue().state())))
                    .append(", \"flags\": ").append(e.getValue().flags()).append("}");
            }
            return cached = b.append("]}").toString();
        }
    }

    /// The `InteractionResult` by the name of the constant it *is*.
    ///
    /// `getClass().getSimpleName()` is not enough here and the difference is
    /// behavioural, not cosmetic: `SUCCESS`, `SUCCESS_SERVER` and `CONSUME`
    /// are three distinct instances of the same record `Success`, separated
    /// only by their `swingSource`, and the swing is a packet. Identity is the
    /// comparison because these are singletons in `InteractionResult`'s own
    /// `<clinit>`; anything built on the spot — `heldItemTransformedTo` makes
    /// one — falls through to a spelled-out form rather than being rounded to
    /// the nearest constant.
    static String resultName(InteractionResult r) {
        if (r == null) {
            return "null";
        }
        if (r == InteractionResult.SUCCESS) {
            return "SUCCESS";
        }
        if (r == InteractionResult.SUCCESS_SERVER) {
            return "SUCCESS_SERVER";
        }
        if (r == InteractionResult.CONSUME) {
            return "CONSUME";
        }
        if (r == InteractionResult.FAIL) {
            return "FAIL";
        }
        if (r == InteractionResult.PASS) {
            return "PASS";
        }
        if (r == InteractionResult.TRY_WITH_EMPTY_HAND) {
            return "TRY_WITH_EMPTY_HAND";
        }
        if (r instanceof InteractionResult.Success s) {
            return "Success(swing=" + s.swingSource() + ", item=" + s.itemContext() + ")";
        }
        return r.getClass().getSimpleName();
    }

    private static String describe(ItemStack stack) {
        if (stack == null || stack.isEmpty()) {
            return "empty";
        }
        return stack.getCount() + " " + BuiltInRegistries.ITEM.getKey(stack.getItem());
    }

    /// The gate over the block currently being probed, or `null`.
    ///
    /// A field rather than a parameter because `shoot` is called from five
    /// places — the base grid, the stance sweep, the second face, the off
    /// hand and the seed sweep — and every one of them must stamp identically
    /// or `same()` would read the stamp as an axis moving.
    private static UseGolden.ServerLevelGate currentGate;

    private static Shot shoot(
        UseGolden.TableLevel level, UseGolden.HarnessPlayer player, BlockState state,
        Direction face, InteractionHand hand, ItemStack proto, long seed
    ) {
        Shot p = new Shot();
        level.reset();
        level.seedRandom(seed);
        level.put(UseGolden.CLICKED, state);
        // A fresh copy every probe. `FlowerPotBlock` ends with
        // `stack.consume(1, player)` and `DecoratedPotBlock` with `grow`, so a
        // shared stack would carry one probe's consumption into the next and
        // the item axis would stop being 1,537 independent questions.
        ItemStack stack = proto.copy();
        player.setItemInHand(hand, stack);
        player.setItemInHand(hand == InteractionHand.MAIN_HAND
            ? InteractionHand.OFF_HAND : InteractionHand.MAIN_HAND, ItemStack.EMPTY);
        level.recording = p.writes;
        BlockHitResult hit = new BlockHitResult(
            Vec3.atCenterOf(UseGolden.CLICKED).relative(face, 0.5), face, UseGolden.CLICKED, false);
        int blockBefore = level.blockTicks.scheduled;
        int fluidBefore = level.fluidTicks.scheduled;
        // Armed here and not at the top: setting up the hand goes through
        // `Player.setItemInHand` itself, so a recorder armed any earlier
        // would record this dumper's own setup as a grant.
        player.granted.clear();
        player.inventoryRead = false;
        player.recordGrants = true;
        player.clearScreens();
        UseGolden.vanillaPaused = null;
        try {
            p.result = state.useItemOn(stack, level, player, hand, hit);
        } catch (Throwable t) {
            String m = t.getMessage();
            p.refusal = t.getClass().getSimpleName()
                + (m == null ? "" : ": " + m.substring(0, Math.min(m.length(), 90)));
        } finally {
            level.recording = null;
            player.recordGrants = false;
        }
        p.sounded = level.sounded;
        p.usedRandom = level.usedRandom;
        p.levelEvent = level.leveledEvent;
        p.readsInventory = player.inventoryRead;
        if (p.refusal == null && UseGolden.vanillaPaused != null) {
            p.refusal = "VanillaPaused: " + UseGolden.vanillaPaused.substring(
                0, Math.min(UseGolden.vanillaPaused.length(), 90));
        }
        if (p.refusal == null) {
            if (level.readUnheldPosition) {
                p.reads.add("air-world");
            }
            if (level.sweptEntities) {
                p.reads.add("no-entities");
            }
        }
        if (!player.granted.isEmpty()) {
            p.granted = String.join("; ", player.granted);
        }
        // **Stamped here rather than at emit, and never on a refusal.** A
        // refusal already says "not measured" and says it louder; stamping it
        // as well would put two different reasons for the same silence on one
        // cell. `CampfireBlock` is the whole of that case — 64 states refused
        // on `recipeAccess`, and its guard says `item-unknown` so the two
        // reasons agree rather than one covering for the other.
        if (p.refusal == null && currentGate != null
                && currentGate.reaches(state, stack)) {
            p.gated.addAll(currentGate.columns());
        }
        // **The block entity used to be a refusal here and is now an answer.**
        //
        // Ten of the twenty-five classes open with
        // `if (!(level.getBlockEntity(pos) instanceof XBlockEntity)) return PASS`
        // — signs, hanging signs, lecterns, jukeboxes, chiseled bookshelves,
        // shelves, decorated pots, beehives, campfires and vaults. The level
        // held no block entities, so that branch was taken every time and the
        // recorded answer was `PASS` for every item on every state. Recording
        // that `PASS` would have been this project's signature defect with no
        // disguise at all, so **this dumper refused on the ask**: 2,593
        // refusals over 2,576 of 4,537 states, one cause and no others.
        //
        // That refusal was right for a level with nothing to answer with, and
        // this level builds the real `BlockEntity` now
        // (`UseGolden.TableLevel.getBlockEntity`) — `EntityBlock.newBlockEntity`
        // is what vanilla's own `LevelChunk` calls. Keeping the blanket refusal
        // would make this table say "not covered" about rules that ran to the
        // end, which is the same fault pointed the other way: a table that
        // understates itself cannot be contradicted either.
        //
        // What stays is the case the level cannot answer, and it stays as the
        // level's own throw rather than as a flag read here — a block that
        // declares `EntityBlock` and whose `newBlockEntity` returns `null`,
        // which registry-wide is `MovingPistonBlock` alone. And an ask for a
        // position this world holds nothing at is recorded on the row as
        // `no-block-entity` rather than refused, because `null` is the right
        // answer for an empty position and the wrong thing to be silent about.
        if (p.refusal == null && level.askedBlockEntityElsewhere) {
            p.reads.add("no-block-entity");
        }
        if (p.refusal == null) {
            // `isSameItemSameComponents` rather than a count comparison: an
            // item can come back the same item in the same number and a
            // different thing — `hurtAndBreak` moves a damage component and
            // `consumeAndReturn` a container one.
            if (stack.getCount() != proto.getCount()
                || !ItemStack.isSameItemSameComponents(stack, proto)) {
                p.stackAfter = describe(stack);
            }
            // `heldItemTransformedTo()` is **nullable**, and null is the
            // common case: `InteractionResult.SUCCESS` and `CONSUME` are
            // singletons built with a null `itemContext`, and only
            // `Success.heldItemTransformedTo(stack)` fills one in. Reading it
            // as an empty stack is an NPE, not a false negative, which is the
            // lucky direction.
            if (p.result instanceof InteractionResult.Success s) {
                ItemStack into = s.heldItemTransformedTo();
                if (into != null && !into.isEmpty()) {
                    p.transformed = describe(into);
                }
            }
        }
        boolean blockMoved = level.blockTicks.scheduled > blockBefore;
        boolean fluidMoved = level.fluidTicks.scheduled > fluidBefore;
        p.scheduled = blockMoved || fluidMoved;
        if (p.scheduled) {
            UseGolden.TickSink<?> sink = blockMoved ? level.blockTicks : level.fluidTicks;
            p.tickKind = blockMoved ? "block" : "fluid";
            p.tickDelay = sink.lastTriggerTick;
            p.tickPriority = sink.lastPriority;
            p.tickAt = sink.lastPos == null ? null : sink.lastPos.subtract(UseGolden.CLICKED);
        }
        return p;
    }

    private UseItemOnGolden() {
    }
}
