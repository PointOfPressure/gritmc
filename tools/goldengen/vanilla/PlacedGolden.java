// PlacedGolden — `Block.setPlacedBy`, per block state, as a table.
//
// # The question
//
// `getStateForPlacement` describes **one** block, and `placement.json`
// carries it. But `BlockItem.place` does not stop there. Read from the jar,
// `BlockItem.place` offsets 156-173:
//
//     state8.getBlock().setPlacedBy(level, clickedPos, state8, player, stack)
//
// and four blocks in 26.2 use that call to write a *second* block that the
// placement state never mentioned. A server driven by `placement.json` alone
// puts down a doorway with no top and a bed with no head, which is exactly
// what `ci/checks/gameplay/130-door-places-both-halves.toml` and
// `230-bed-places-two-blocks.toml` were written to say.
//
// # What is recorded
//
// The same thing `UseGolden` records, for the same reason and against the
// same level: **every `setBlock` the call makes**, as an offset from the
// placed position and the state written there, plus whether it scheduled a
// tick and with what delay. The question "what `setBlock` calls does this
// call make" is the one both dumpers ask, so they share `TableLevel` rather
// than growing two levels that drift apart.
//
// A call that reads something the harness does not model **throws, and is
// recorded as a refusal naming the method**. That is `PlacementGolden`'s and
// `UseGolden`'s rule and it is the whole reason a table like this can be
// trusted: the alternative is a stub that invents a plausible answer and a
// row that is quietly wrong.
//
// # What the family looks like, counted rather than guessed
//
// **16 classes override `setPlacedBy`, and the glob that used to say so could
// not see one of them.** This paragraph derived the family from `javap -p`
// over `net/minecraft/world/level/block/*.class`, which is not recursive.
// Recursively the jar declares seventeen: sixteen overrides plus the base
// `Block`, which declares the method and overrides nothing. So the old census
// counted `Block` and missed `net.minecraft.world.level.block.piston`
// `.PistonBaseBlock` — two errors that cancel in the total and leave the
// *set* wrong, and the class it could not see is the only class in this table
// that produces a refusal at all. `ci/probe-absences.py` derives the set the
// recursive way and refuses if any member of it has no row and no written
// reason.
//
// Four write a second block from a fixed offset — `DoorBlock` (above,
// `HALF=UPPER`), `BedBlock` (`relative(FACING)`, `PART=HEAD`),
// `DoublePlantBlock` and `SmallDripleafBlock` (both above, `HALF=UPPER`).
// `CrafterBlock` schedules a tick. `PistonBaseBlock` refuses on
// `getChunkSource`. **The remaining ten produce nothing at all**, and nine of
// those ten are this harness rather than the jar — see
// `ci/checks/probe-absences.toml`, which carries the bytecode read for each.
// Only `PitcherCropBlock` is a genuine no-op (`0: return`, cancelling
// `DoublePlantBlock`'s upper half because a pitcher crop grows its second
// block later).
//
// # Twice, and the pair must agree
//
// `MossyCarpetBlock.setPlacedBy` calls `level.getRandom()`, so a block whose
// two probes disagree is emitted with **no cells** and `"random": true` rather
// than one sample of a die roll — that is how a golden file churns and how a
// consumer reproduces a state vanilla only sometimes writes. `UseGolden` makes
// the same call and the dragon egg is what it caught.
//
// **`MossyCarpetBlock` is not what it catches here, and this header claimed it
// was.** Measured 2026-08-18: `randomBlocks` is 0 and there is no mossy carpet
// row. `createTopperWithSideChance` builds the topper from whichever sides
// already carry mossy carpet; an empty world has none, so the `BooleanSupplier`
// over `getRandom` is never called, the topper is air, `isAir()` is true and
// the `setBlock` is skipped. The RNG is reached and never drawn. The check is
// still right to exist — it is the only thing that would catch a real drawer —
// but this family currently has none, and a check with nothing to catch is
// worth saying so about rather than leaving as an implied example.
//
// # Grep any diagnostic output unanchored
//
// `Bootstrap.bootStrap()` redirects both streams through log4j, which
// re-emits each line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so
// `grep -E "^SOMETHING"` matches nothing and exits 1 — indistinguishable
// from a clean run that found nothing. That prefix is also why this writes
// its JSON to stdout and is captured with `>` rather than through a pipe.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;

public final class PlacedGolden {

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        UseGolden.armVanillasOwnAlarm();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        // **The stack this harness holds is now the stack `BlockItem.place`
        // holds.** See the comment on `stack` below for the four years of
        // `ItemStack.EMPTY` this replaces.
        UseGolden.bindItemComponents(lookup);
        // `CommandBlock.setPlacedBy` is the one gated rule on this side, and
        // it is the whole body: `if (level instanceof ServerLevel)` wraps
        // everything, so the probe found nothing to record and three blocks had
        // no row at all. Unlike `useWithoutItem` there is no `consumes` to be
        // right by coincidence about — `setPlacedBy` returns `void` — so the
        // silence here was total.
        UseGolden.ServerLevelGate.load("setPlacedBy");
        Holder<DimensionType> overworld = lookup
            .lookupOrThrow(Registries.DIMENSION_TYPE)
            .getOrThrow(BuiltinDimensionTypes.OVERWORLD);

        UseGolden.TableLevel level =
            new UseGolden.TableLevel(UseGolden.accessOf(lookup), overworld);
        UseGolden.HarnessPlayer player = new UseGolden.HarnessPlayer(level);

        List<String> rows = new ArrayList<>();
        // **Every block in the registry, not only the ones with a row.**
        //
        // Consumers need to ask "is this a `DoorBlock`" about blocks that do
        // nothing interesting here, and about a block's *neighbour*, which
        // can be anything at all. A map built only from the rows would answer
        // "not a door" for every block it happened not to carry — a false
        // negative that reads exactly like a correct answer.
        //
        // It exists so that no rule in this project is ever keyed on a block
        // *name*. "The id ends in `_button`" holds until one block breaks it,
        // and the break is silent.
        Map<String, String> classes = new LinkedHashMap<>();
        // **The superclass chain, because the bytecode tests `instanceof`.**
        //
        // `DoorBlock.updateShape` asks `nb.getBlock() instanceof DoorBlock`,
        // and four blocks answer yes without being a `DoorBlock` by name: the
        // unwaxed copper doors are `WeatheringCopperDoorBlock`. A consumer
        // comparing simple names would give those four no rule at all — their
        // upper halves would neither follow their lower ones nor go when they
        // go — and the corpus only ever places an *oak* door, so nothing
        // would notice. Keyed by class rather than by block: ~200 chains
        // instead of 1,196.
        Map<String, List<String>> supers = new LinkedHashMap<>();
        int blocksWithBehaviour = 0;
        int statesProbed = 0;
        int statesWriting = 0;
        int refusals = 0;
        int gatedBlocks = 0;
        int gatedCells = 0;
        int randomBlocks = 0;
        int worldSensitiveBlocks = 0;
        Map<String, Integer> refusedBy = new LinkedHashMap<>();

        for (Block block : BuiltInRegistries.BLOCK) {
            Identifier id = BuiltInRegistries.BLOCK.getKey(block);
            // **The stack `BlockItem.place` actually holds**, which for four
            // months was `ItemStack.EMPTY`.
            //
            // The reason it was empty is written out in the history and was
            // true when it was written: `Item.getDefaultInstance` reaches
            // `Holder.Reference.components`, which throws `Components not bound
            // yet` on a JVM that has only run `Bootstrap.bootStrap()`. What
            // that comment could not know is that the fix landed on
            // 2026-08-12, in the *other* dumper —
            // `UseGolden.bindItemComponents` runs the initializers a real
            // server runs, 141 pending registries apply and all 1,537 items
            // mint a stack — and nothing carried it across, because the two
            // dumpers share a level and not a bootstrap.
            //
            // `JukeboxBlock` is the one block in the family that reads the
            // stack, off `DataComponents.BLOCK_ENTITY_DATA`, and the old
            // comment claimed it refused here. It did not: `ItemStack.EMPTY
            // .get(...)` answers `null` without throwing, the `HAS_RECORD`
            // write was skipped, and there was no jukebox row and no jukebox
            // refusal anywhere in the table.
            //
            // **With the real stack the answer is the same and it now means
            // something.** A plain jukebox item carries no block entity data —
            // only one middle-clicked off a jukebox that already holds a record
            // does — so vanilla writes nothing here either, and the absence is
            // a fact about the mechanic rather than about the harness. The
            // stack that would move it is a *placement input this table does
            // not sweep*, which is a different item from a level this harness
            // cannot build, and the ledger says so now.
            //
            // Nothing else in the family looks at the stack at all, which was
            // read from the bytecode of all sixteen overrides rather than
            // assumed.
            ItemStack stack = defaultStackFor(block);
            classes.put(id.toString(), block.getClass().getSimpleName());
            supers.computeIfAbsent(block.getClass().getSimpleName(), k -> {
                List<String> chain = new ArrayList<>();
                for (Class<?> c = block.getClass().getSuperclass();
                     c != null && !c.equals(Object.class);
                     c = c.getSuperclass()) {
                    chain.add(c.getSimpleName());
                }
                return chain;
            });
            UseGolden.ServerLevelGate gate = UseGolden.ServerLevelGate.of(block);
            List<String> cells = new ArrayList<>();
            boolean any = false;
            boolean random = false;
            boolean worldSensitive = false;

            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                statesProbed++;
                // Every seed, and they must all agree — see
                // `UseGolden.TableLevel.harnessRandom` for why the seeds are
                // fixed and why "the rule touched the RNG" is the wrong test.
                // `MossyCarpetBlock.setPlacedBy` is this family's drawer.
                Probe p = probe(level, player, state, stack, false, UseGolden.SEEDS[0]);
                for (int s = 1; s < UseGolden.SEEDS.length; s++) {
                    if (!p.sameAnswer(
                        probe(level, player, state, stack, false, UseGolden.SEEDS[s]))) {
                        random = true;
                    }
                }
                // **The same probe in a world made of water**, and if the
                // answer moves the row is conditioned on a world this
                // harness does not have. `SmallDripleafBlock.setPlacedBy`
                // calls `DoublePlantBlock.copyWaterloggedFrom`, which reads
                // the fluid at the position it is about to write — so in air
                // it writes `waterlogged=false` and that is the harness's
                // answer, not the mechanic's.
                //
                // Detected rather than assumed, and flagged rather than
                // dropped: the row is still right for a dry placement, which
                // is every placement the corpus makes, and a consumer that
                // needs the wet one now knows the row cannot give it.
                if (!p.sameAnswer(
                    probe(level, player, state, stack, true, UseGolden.SEEDS[0]))) {
                    worldSensitive = true;
                }
                // After the seed and world sweeps, so the gate is not on both
                // sides of a comparison it cannot move.
                if (p.refusal == null && gate != null && gate.reaches(state)) {
                    p.gated.addAll(gate.columns());
                }
                if (p.refusal != null) {
                    refusals++;
                    refusedBy.merge(p.refusal, 1, Integer::sum);
                }
                if (p.interesting()) {
                    any = true;
                    statesWriting++;
                    if (!p.gated.isEmpty()) {
                        gatedCells++;
                    }
                    cells.add(p.json(state));
                }
            }

            String gateCol = gate == null ? "" : gate.json();
            if (gate != null) {
                gatedBlocks++;
            }
            if (any) {
                blocksWithBehaviour++;
                if (random) {
                    randomBlocks++;
                    rows.add("    {\"block\": " + UseGolden.quote(id.toString())
                        + ", \"class\": " + UseGolden.quote(block.getClass().getSimpleName())
                        + ", \"random\": true, \"worldSensitive\": " + worldSensitive
                        + gateCol
                        + ", \"states\": []}");
                } else {
                    if (worldSensitive) {
                        worldSensitiveBlocks++;
                    }
                    rows.add("    {\"block\": " + UseGolden.quote(id.toString())
                        + ", \"class\": " + UseGolden.quote(block.getClass().getSimpleName())
                        + ", \"random\": false"
                        + ", \"worldSensitive\": " + worldSensitive
                        + gateCol
                        + ", \"states\": [\n      "
                        + String.join(",\n      ", cells) + "\n    ]}");
                }
            }
        }

        if (UseGolden.ServerLevelGate.resolved() == 0 || gatedCells == 0) {
            throw new IllegalStateException(
                "the ServerLevel gate column resolved "
                + UseGolden.ServerLevelGate.resolved() + " block(s) and stamped "
                + gatedCells + " cell(s) — inert, and an inert one puts the "
                + "command blocks back to having no row at all");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"what\": \"Block.setPlacedBy, per state\",\n");
        out.append("  \"blocksWithBehaviour\": ").append(blocksWithBehaviour).append(",\n");
        out.append("  \"statesProbed\": ").append(statesProbed).append(",\n");
        out.append("  \"statesWriting\": ").append(statesWriting).append(",\n");
        out.append("  \"refusals\": ").append(refusals).append(",\n");
        out.append("  \"randomBlocks\": ").append(randomBlocks).append(",\n");
        out.append("  \"worldSensitiveBlocks\": ").append(worldSensitiveBlocks).append(",\n");
        out.append("  \"serverLevelGatedBlocks\": ").append(gatedBlocks).append(",\n");
        out.append("  \"serverLevelGatedCells\": ").append(gatedCells).append(",\n");
        out.append("  \"refusedBy\": {\n");
        int written = 0;
        for (Map.Entry<String, Integer> e : refusedBy.entrySet()) {
            written++;
            out.append("    ").append(UseGolden.quote(e.getKey())).append(": ")
                .append(e.getValue()).append(written < refusedBy.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"blocks\": ").append(classes.size()).append(",\n");
        out.append("  \"classes\": {\n");
        written = 0;
        for (Map.Entry<String, String> e : classes.entrySet()) {
            written++;
            out.append("    ").append(UseGolden.quote(e.getKey())).append(": ")
                .append(UseGolden.quote(e.getValue()))
                .append(written < classes.size() ? ",\n" : "\n");
        }
        out.append("  },\n  \"superclasses\": {\n");
        written = 0;
        for (Map.Entry<String, List<String>> e : supers.entrySet()) {
            written++;
            StringBuilder chain = new StringBuilder();
            for (int i = 0; i < e.getValue().size(); i++) {
                chain.append(i > 0 ? ", " : "").append(UseGolden.quote(e.getValue().get(i)));
            }
            out.append("    ").append(UseGolden.quote(e.getKey())).append(": [")
                .append(chain).append("]")
                .append(written < supers.size() ? ",\n" : "\n");
        }
        out.append("  },\n  \"table\": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }

    /// One `setPlacedBy`, and what it did.
    private static final class Probe {
        /// Offset from the placed position -> the state written there.
        Map<BlockPos, UseGolden.Write> writes = new LinkedHashMap<>();
        boolean scheduled;
        boolean usedRandom;
        long tickDelay = -1L;
        String tickPriority;
        BlockPos tickAt;
        /// `"block"` or `"fluid"` — see `UseGolden.Probe.tickKind`.
        String tickKind;
        /// Non-null when the call read something this harness does not model.
        String refusal;
        /// The screens the rule asked the player to open, or null.
        String screens;
        /// Whether the rule played a sound.
        ///
        /// `use_without_item.json` has carried this since it was created and
        /// this table never did, which cost exactly one block:
        /// `DriedGhastBlock.setPlacedBy` calls the base no-op and then plays
        /// one of two sounds depending on `WATERLOGGED`, writes nothing and
        /// schedules nothing, so `interesting()` was false and there was no
        /// row. The ledger carried it as `out-of-scope` — correct for a table
        /// with no sound column, and the reason to add the column rather than
        /// to keep the entry.
        boolean sounded;
        /// Whether the rule changed a block entity's own data.
        boolean beWritten;
        /// **What this harness answered out of its own emptiness.** Same
        /// column, same closed vocabulary and same argument as
        /// `UseGolden.Probe.reads` — read its comment, not this one.
        ///
        /// This is the table where it does the most work. `use_without_item.json`
        /// needed it for one class; here it is the *only* thing separating
        /// three blocks from a block with no rule, and `DiodeBlock` is the
        /// worked example the whole ledger is built on: the entire method is
        /// `if (shouldTurnOn(level, pos, state)) level.scheduleTick(pos, this, 1)`,
        /// `shouldTurnOn` asks the world how much signal arrives at the back,
        /// this world is air, so no tick is scheduled and `interesting()` is
        /// false. A consumer reading "no row, no tick" ships a repeater that
        /// never wakes when it is placed already powered — and the table and
        /// the server hold the same absent input, so they agree, and agreeing
        /// is all any check between them could ever measure.
        final java.util.TreeSet<String> reads = new java.util.TreeSet<>();
        /// The columns of this cell the `instanceof ServerLevel` branch
        /// decided. See `UseGolden.Probe.gated`; this side has one rule under
        /// it and that rule is the whole body, so `gated` is the only reason
        /// these three blocks have a row at all.
        final java.util.TreeSet<String> gated = new java.util.TreeSet<>();

        boolean interesting() {
            return refusal != null || !writes.isEmpty() || scheduled
                || !reads.isEmpty() || !gated.isEmpty()
                || screens != null || beWritten || sounded;
        }

        boolean sameAnswer(Probe other) {
            return String.valueOf(refusal).equals(String.valueOf(other.refusal))
                && writes.equals(other.writes)
                && scheduled == other.scheduled
                && tickDelay == other.tickDelay
                && String.valueOf(tickPriority).equals(String.valueOf(other.tickPriority))
                && String.valueOf(tickAt).equals(String.valueOf(other.tickAt))
                && String.valueOf(tickKind).equals(String.valueOf(other.tickKind))
                && String.valueOf(screens).equals(String.valueOf(other.screens))
                && beWritten == other.beWritten
                && sounded == other.sounded
                && reads.equals(other.reads);
        }

        String json(BlockState from) {
            StringBuilder b = new StringBuilder();
            b.append("{\"from\": ").append(UseGolden.quote(BlockStateParser.serialize(from)));
            if (refusal != null) {
                b.append(", \"refused\": ").append(UseGolden.quote(refusal));
                return b.append("}").toString();
            }
            if (screens != null) {
                b.append(", \"opens\": ").append(UseGolden.quote(screens));
            }
            if (beWritten) {
                b.append(", \"blockEntityWritten\": true");
            }
            if (!reads.isEmpty()) {
                b.append(", \"reads\": [").append(UseGolden.quotedList(reads)).append("]");
            }
            if (!gated.isEmpty()) {
                b.append(", \"gated\": [").append(UseGolden.quotedList(gated)).append("]");
            }
            b.append(", \"sound\": ").append(sounded);
            b.append(", \"schedulesTick\": ").append(scheduled);
            if (scheduled) {
                b.append(", \"tickDelay\": ").append(tickDelay);
                b.append(", \"tickPriority\": ")
                    .append(UseGolden.quote(String.valueOf(tickPriority)));
                b.append(", \"tickAt\": [").append(tickAt == null ? "" : tickAt.getX()
                    + ", " + tickAt.getY() + ", " + tickAt.getZ()).append("]");
                b.append(", \"tickKind\": ")
                    .append(UseGolden.quote(String.valueOf(tickKind)));
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
                    .append(", \"flags\": ").append(e.getValue().flags())
                    .append("}");
            }
            return b.append("]}").toString();
        }
    }

    /// `block.asItem().getDefaultInstance()`, or `ItemStack.EMPTY` for a
    /// block with no item.
    ///
    /// **Not every block has one.** `minecraft:water`, `minecraft:fire`,
    /// `minecraft:moving_piston` and the potted plants have no item at all and
    /// `asItem()` answers `Items.AIR`, whose default instance is empty — which
    /// is the truthful stack for a block nothing can hold, and not the same
    /// statement as the blanket `EMPTY` this replaces.
    private static ItemStack defaultStackFor(Block block) {
        net.minecraft.world.item.Item item = block.asItem();
        if (item == net.minecraft.world.item.Items.AIR) {
            return ItemStack.EMPTY;
        }
        return item.getDefaultInstance();
    }

    private static Probe probe(
        UseGolden.TableLevel level, UseGolden.HarnessPlayer player,
        BlockState state, ItemStack stack, boolean wet, long seed
    ) {
        Probe p = new Probe();
        level.reset();
        level.seedRandom(seed);
        if (wet) {
            // Two things, because water is asked about two ways and only one
            // of them is a block. `getBlockState` sees the cube; `isWaterAt`
            // is a **tag** lookup that no bootstrap-only JVM can answer, so
            // it is forced — see `TableLevel.pretendWater`. Filling the cube
            // alone flagged nothing at all, which is what sent me looking.
            for (int dx = -1; dx <= 1; dx++) {
                for (int dy = -1; dy <= 1; dy++) {
                    for (int dz = -1; dz <= 1; dz++) {
                        level.put(UseGolden.CLICKED.offset(dx, dy, dz),
                            net.minecraft.world.level.block.Blocks.WATER.defaultBlockState());
                    }
                }
            }
            level.pretendWater = true;
        }
        // The world vanilla's call sees: the block already written at the
        // clicked position, because `BlockItem.place` calls `placeBlock`
        // first and re-reads the level before calling `setPlacedBy`. A door
        // that could not read itself back would take a different branch.
        // Written after the water, so the subject wins its own position.
        level.put(UseGolden.CLICKED, state);
        level.recording = p.writes;
        int blockBefore = level.blockTicks.scheduled;
        int fluidBefore = level.fluidTicks.scheduled;
        player.clearScreens();
        UseGolden.vanillaPaused = null;
        try {
            state.getBlock().setPlacedBy(level, UseGolden.CLICKED, state, player, stack);
        } catch (Throwable t) {
            String m = t.getMessage();
            p.refusal = t.getClass().getSimpleName()
                + (m == null ? "" : ": " + m.substring(0, Math.min(m.length(), 90)));
        } finally {
            level.recording = null;
        }
        if (p.refusal == null && UseGolden.vanillaPaused != null) {
            p.refusal = "VanillaPaused: " + UseGolden.vanillaPaused.substring(
                0, Math.min(UseGolden.vanillaPaused.length(), 90));
        }
        p.usedRandom = level.usedRandom;
        p.sounded = level.sounded;
        p.beWritten = level.blockEntityWritten;
        if (!player.screens.isEmpty()) {
            p.screens = String.join("; ", player.screens);
        }
        if (p.refusal == null) {
            if (level.readUnheldPosition) {
                p.reads.add("air-world");
            }
            if (level.sweptEntities) {
                p.reads.add("no-entities");
            }
            if (level.askedBlockEntityElsewhere) {
                p.reads.add("no-block-entity");
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
        // A write at offset [0,0,0] is the block overwriting *itself* — the
        // jukebox does it — and the placed state has to be dropped from the
        // recording, not the write. Nothing is filtered here; the consumer
        // applies the writes in order and the last one at a position wins,
        // which is what `Level.setBlock` does.
        return p;
    }

    private PlacedGolden() {
    }
}
