// UseGolden — extract `BlockState.useWithoutItem` per block state.
//
// # What this answers
//
// `ServerPlayerGameMode.useItemOn` offers a right-click to the **block**
// before it offers it to the item, and only reaches the placement path if
// the block declined. The order, from the bytecode:
//
//     if !state.getBlock().isEnabled(level.enabledFeatures())     -> FAIL
//     if gameMode == SPECTATOR: openMenu or PASS
//     holdingSomething = !mainHand.isEmpty() || !offhand.isEmpty()
//     if isSecondaryUseActive() && holdingSomething: goto PLACEMENT
//     r = state.useItemOn(stack, level, player, hand, hit)
//     if r.consumesAction()                                       -> r
//     if r instanceof TryEmptyHandInteraction && hand == MAIN_HAND:
//         r2 = state.useWithoutItem(level, player, hit)
//         if r2.consumesAction()                                  -> r2
//     PLACEMENT: ...
//
// **51 classes override `useWithoutItem` in 26.2** — counted with `javap -p`
// over `net/minecraft/world/level/block/*.class`, not guessed — and there is
// nothing in a table of block *states* that says `LeverBlock` cycles
// `POWERED` while `RepeaterBlock` cycles `DELAY` and `ComparatorBlock`
// cycles `MODE`. So this asks each state instead, exactly as
// `PlacementGolden` asks each block what `getStateForPlacement` returns.
//
// # Why an extraction and not fifty hand-written cases
//
// `crate::useblock` shipped with one block, `LeverBlock`, ported by hand
// from its own bytecode. That closed `lever-flips-when-right-clicked`
// (`diverge -> match`) and proved the path end to end — and the commit that
// landed it said adding a second block by hand was the wrong next move,
// because the behaviours are not guessable from names and fifty of them
// hand-written is fifty chances to be quietly wrong. This is that argument
// carried out.
//
// # What is recorded, and what "refused" means
//
// For every state of every block: put the state down, right-click it, and
// write down **every `setBlock` the call makes** — offset from the clicked
// position, and the state written — plus the `InteractionResult` and whether
// a tick was scheduled. Blocks that write nothing and return a pass-like
// result fall out on their own, which is the answer for the ~1,000 that need
// nothing.
//
// A block that reads something this harness does not model **throws, and is
// recorded as a refusal naming the method**. That is `PlacementGolden`'s
// discipline and `GenLevel`'s before it: a stub that invents a plausible
// empty answer produces a table that is quietly wrong, and this project has
// chased four parity failures to exactly that. Chests, furnaces, lecterns
// and cakes are all expected to refuse — they want a block entity, a menu or
// a food stat — and a refusal is the honest record that they are not covered
// rather than a false "does nothing".
//
// Two deliberate deviations from "throw for everything", each one because
// the alternative refuses a rule this is trying to capture:
//
//   * **scheduled ticks are accepted and recorded**, not refused.
//     `ButtonBlock.useWithoutItem` schedules its own un-press, and refusing
//     `getBlockTicks` would refuse the button — whose corpus check is one of
//     the two this extraction exists to close. `ShapeGolden` makes the same
//     call for the same reason.
//   * **sounds are accepted and recorded; game events are accepted and
//     dropped.** `Level.playSound` and `Level.gameEvent` are side effects
//     with no `setBlock` in them; refusing them would refuse the comparator
//     and the note block over something this table does not carry. The game
//     event still reaches no client and is still dropped.
//
//     **The sound was dropped too until 2026-08-17, and that was the whole
//     of the block-sound gap.** The column was a boolean, so the table said
//     *that* 184 blocks sound and never *which* sound, and a consumer cannot
//     build a packet out of a boolean: `world_send::use_sound` answered for
//     the four families whose classes had been transcribed by hand and 180
//     blocks stayed silent. The overrides were always handed the event, the
//     category, the volume and the pitch — see `recordSound`. What is
//     recorded is the first four; the **pitch is recorded only when it is not
//     a draw**, because `DoorBlock.playSound` takes it from
//     `level.getRandom().nextFloat() * 0.1F + 0.9F` and this harness fixes
//     that seed, so a drawn pitch in this table would be a constant of
//     `RandomSource.create(0L)` wearing a golden value's clothes.
//
// # Grep any diagnostic output unanchored
//
// `Bootstrap.bootStrap()` redirects both streams through log4j, which
// re-emits each line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so
// `grep -E "^SOMETHING"` matches nothing and exits 1 — indistinguishable
// from a clean run that found nothing. That prefix is also why this writes
// its JSON to stdout and is captured with `>` rather than through a pipe.

package net.minecraft.world.level.levelgen;

import com.mojang.authlib.GameProfile;
import com.mojang.serialization.Lifecycle;
import java.util.Collection;
import net.minecraft.SharedConstants;
import net.minecraft.world.Difficulty;
import net.minecraft.world.level.storage.LevelData;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeSet;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistrationInfo;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.ResourceKey;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.permissions.PermissionSet;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.storage.WritableLevelData;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

public final class UseGolden {

    /// The clicked block. Y is a plain in-range value, and not 0, so that a
    /// sign bug in a consumer's own packing shows up as a wrong number
    /// rather than as zero. Same choice as `PlacementGolden`.
    /// Package-private, not private: `PlacedGolden` runs the same level over
    /// the same origin and its offsets have to mean the same thing.
    static final BlockPos CLICKED = new BlockPos(0, 64, 0);

    /// The face and cursor a right-click carries. `useWithoutItem` reads the
    /// hit result for very little — a cake reads nothing, a lectern reads the
    /// page — so one face is enough to see whether anything is written at
    /// all, and a block whose answer depends on the face is caught by the
    /// second face below disagreeing with the first.
    private static final Direction[] FACES = {Direction.UP, Direction.NORTH};

    /// The RNG seeds every state is probed under. Eight, fixed, and **not
    /// 0..7**.
    ///
    /// Fixed so the classification is reproducible: the same questions every
    /// run, so the table cannot churn.
    ///
    /// Widely separated because consecutive small seeds are not independent.
    /// `RandomSource.create(long)` is `java.util.Random`'s algorithm, whose
    /// scramble leaves seeds 0..5 differing in a handful of low bits, and
    /// their leading outputs correlate. Measured: under `{0,1,2,3,4,5}` the
    /// dragon egg's teleport failed under **all six**, so its row came back
    /// "not random" with an empty write list — the classification looked
    /// stable and was simply blind. These are the Blowfish P-array
    /// constants, which is only a convenient source of well-spread bits.
    static final long[] SEEDS = {
        0x243F6A8885A308D3L, 0x13198A2E03707344L, 0xA4093822299F31D0L,
        0x082EFA98EC4E6C89L, 0x452821E638D01377L, 0xBE5466CF34E90C6CL,
        0xC0AC29B7C97C50DDL, 0x3F84D5B5B5470917L,
    };

    /// One configuration of the probe's **own player**, and the axis this
    /// dumper sweeps that nothing else did.
    ///
    /// # Why this exists
    ///
    /// `useWithoutItem` is handed the player, and a great many blocks read it.
    /// `FenceGateBlock` compares `player.getDirection()` against its own
    /// `FACING` and flips the gate to face the player when they disagree — so
    /// with one player, this dumper wrote 384 fence-gate rows of which exactly
    /// 48 change `facing`, all of them `north -> south`, **because the player
    /// this harness constructs has `yRot = 0` and `Direction.fromYRot(0)` is
    /// `SOUTH`.** The table was right for a player facing south and wrong for
    /// the other three quarters of the circle, and nothing could contradict it
    /// because no probe in another orientation existed. `DaylightDetectorBlock`
    /// is the same shape through `player.mayBuild()`, `LightBlock` through
    /// `player.canUseGameMasterBlocks()`.
    ///
    /// This is the project's signature defect — a golden table that cannot fail
    /// — and the fix is the one already used for the RNG and for the clicked
    /// face: **ask the question more than once and record whether the answer
    /// moved.** `SEEDS` sweeps the die roll and sets `random`; `FACES` sweeps
    /// the hit result and sets `faceSensitive`; `STANCES` sweeps the player and
    /// sets `probeDependent`. A consumer that reads a `probeDependent` row as a
    /// fact about the block is now doing so knowingly.
    ///
    /// # The first entry is not free to change
    ///
    /// `STANCES[0]` is the player exactly as the harness has always built it,
    /// and every recorded cell is still taken under it — that is what keeps
    /// this an added column rather than a rewritten table. Its position and
    /// rotation are read off the freshly constructed player rather than typed
    /// in (see `Stance.HOME`), so it stays the identity stance even if
    /// `Player`'s constructor changes where it puts one.
    ///
    /// Each further entry moves **one** thing, because a stance that moved
    /// three would say a row was probe-dependent without saying on what.
    static final class Stance {
        /// Filled once from the constructed player, so `base` is the identity.
        static Vec3 HOME;
        static float HOME_Y_ROT;
        static float HOME_X_ROT;
        /// The food level a freshly constructed `Player` carries, read rather
        /// than typed in for the same reason as `HOME` — see [`pinHome`].
        static int HOME_FOOD;

        final String name;
        /// Absolute position, or null for "wherever the constructor left it".
        final Vec3 pos;
        final Float yRot;
        final Float xRot;
        final boolean sneaking;
        final boolean mayBuild;
        final boolean instabuild;
        final boolean gameMaster;
        final boolean fullInventory;
        /// Food points, or null for "whatever the constructor left".
        final Integer food;
        /// `Abilities.invulnerable`, which `Player.canEat` reads *first*.
        final boolean invulnerable;

        private Stance(String name, Vec3 pos, Float yRot, Float xRot, boolean sneaking,
            boolean mayBuild, boolean instabuild, boolean gameMaster, boolean fullInventory,
            Integer food, boolean invulnerable) {
            this.name = name;
            this.pos = pos;
            this.yRot = yRot;
            this.xRot = xRot;
            this.sneaking = sneaking;
            this.mayBuild = mayBuild;
            this.instabuild = instabuild;
            this.gameMaster = gameMaster;
            this.fullInventory = fullInventory;
            this.food = food;
            this.invulnerable = invulnerable;
        }

        static Stance base() {
            return new Stance("base", null, null, null, false, true, false, false, false,
                null, false);
        }

        Stance with(String name) {
            return new Stance(name, pos, yRot, xRot, sneaking,
                mayBuild, instabuild, gameMaster, fullInventory, food, invulnerable);
        }

        Stance at(Vec3 p) {
            return new Stance(name, p, yRot, xRot, sneaking,
                mayBuild, instabuild, gameMaster, fullInventory, food, invulnerable);
        }

        Stance looking(float y, float x) {
            return new Stance(name, pos, y, x, sneaking,
                mayBuild, instabuild, gameMaster, fullInventory, food, invulnerable);
        }

        Stance sneak() {
            return new Stance(name, pos, yRot, xRot, true,
                mayBuild, instabuild, gameMaster, fullInventory, food, invulnerable);
        }

        Stance abilities(boolean build, boolean insta, boolean gm) {
            return new Stance(name, pos, yRot, xRot, sneaking, build, insta, gm, fullInventory,
                food, invulnerable);
        }

        Stance packed() {
            return new Stance(name, pos, yRot, xRot, sneaking,
                mayBuild, instabuild, gameMaster, true, food, invulnerable);
        }

        /// Hungry enough that `FoodData.needsFood()` is true.
        Stance hungry() {
            return new Stance(name, pos, yRot, xRot, sneaking,
                mayBuild, instabuild, gameMaster, fullInventory, 6, invulnerable);
        }

        /// `Abilities.invulnerable`, which a real creative player has and this
        /// harness's player has never had.
        Stance invulnerable() {
            return new Stance(name, pos, yRot, xRot, sneaking,
                mayBuild, instabuild, gameMaster, fullInventory, food, true);
        }

        /// Every axis is set on every application, never left over from the
        /// stance before. One reused mutable player and a partial reset is how
        /// a sweep silently stops sweeping.
        void apply(HarnessPlayer player) {
            Vec3 p = pos == null ? HOME : pos;
            player.setPos(p.x, p.y, p.z);
            player.setYRot(yRot == null ? HOME_Y_ROT : yRot);
            player.setXRot(xRot == null ? HOME_X_ROT : xRot);
            player.setShiftKeyDown(sneaking);
            player.getAbilities().mayBuild = mayBuild;
            player.getAbilities().instabuild = instabuild;
            player.gameMaster = gameMaster;
            player.getAbilities().invulnerable = invulnerable;
            player.getFoodData().setFoodLevel(food == null ? HOME_FOOD : food);
            player.getInventory().clearContent();
            if (fullInventory && FILLER != null) {
                for (int i = 0; i < player.getInventory().getContainerSize(); i++) {
                    player.getInventory().setItem(i, FILLER.copy());
                }
            }
        }

        /// Whether this stance can be set up at all in this harness.
        boolean available() {
            return !fullInventory || FILLER != null;
        }

        String json() {
            StringBuilder b = new StringBuilder("{\"name\": ").append(quote(name));
            Vec3 p = pos == null ? HOME : pos;
            b.append(", \"pos\": [").append(p.x).append(", ").append(p.y)
                .append(", ").append(p.z).append("]");
            b.append(", \"yRot\": ").append(yRot == null ? HOME_Y_ROT : yRot);
            b.append(", \"facing\": ").append(quote(
                Direction.fromYRot(yRot == null ? HOME_Y_ROT : yRot).getName()));
            b.append(", \"xRot\": ").append(xRot == null ? HOME_X_ROT : xRot);
            b.append(", \"sneaking\": ").append(sneaking);
            b.append(", \"mayBuild\": ").append(mayBuild);
            b.append(", \"instabuild\": ").append(instabuild);
            b.append(", \"gameMaster\": ").append(gameMaster);
            b.append(", \"fullInventory\": ").append(fullInventory);
            b.append(", \"food\": ").append(food == null ? HOME_FOOD : food);
            b.append(", \"invulnerable\": ").append(invulnerable);
            return b.append("}").toString();
        }
    }

    /// The stack `full_inventory` packs the player with, or null when this
    /// harness cannot mint one.
    ///
    /// **Null is the expected answer today and it is reported, not swallowed.**
    /// `new ItemStack(Items.BEDROCK)` reads `Holder.Reference.components()`,
    /// which `Bootstrap.bootStrap()` alone does not bind — the same limit that
    /// already puts 39 `Components not bound yet` refusals in this table. A
    /// stance that quietly degraded to a copy of `base` would make the sweep
    /// report "constant across all probe states" for an axis it never turned,
    /// which is this project's signature defect wearing the costume of its own
    /// fix. So `available()` is false, the stance is skipped, and
    /// `probeStancesUnavailable` in the output names it and says why.
    static ItemStack FILLER;
    static String FILLER_WHY;

    /// The sweep. Twelve, one axis apart each, and `base` first.
    ///
    /// `adjacent_*` exist because position is probe state as much as facing is:
    /// `SignBlockEntity.isFacingFrontText` reads `player.getX()`/`getZ()` and
    /// `BedBlock` hands the player's position straight to `startSleepInBed`.
    /// The constructed player stands at the origin, 64 blocks below the block
    /// it is clicking, which no real player ever does.
    static final Stance[] STANCES = {
        Stance.base(),
        Stance.base().with("facing_west").looking(90.0f, 0.0f),
        Stance.base().with("facing_north").looking(180.0f, 0.0f),
        Stance.base().with("facing_east").looking(270.0f, 0.0f),
        Stance.base().with("looking_down").looking(0.0f, 90.0f),
        Stance.base().with("looking_up").looking(0.0f, -90.0f),
        Stance.base().with("adjacent_south")
            .at(new Vec3(CLICKED.getX() + 0.5, CLICKED.getY(), CLICKED.getZ() + 2.5))
            .looking(180.0f, 0.0f),
        Stance.base().with("adjacent_above")
            .at(new Vec3(CLICKED.getX() + 0.5, CLICKED.getY() + 1, CLICKED.getZ() + 0.5)),
        Stance.base().with("sneaking").sneak(),
        Stance.base().with("no_build").abilities(false, false, false),
        Stance.base().with("game_master").abilities(true, true, true),
        Stance.base().with("full_inventory").packed(),
        Stance.base().with("hungry").hungry(),
        Stance.base().with("invulnerable").invulnerable(),
    };

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        armVanillasOwnAlarm();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        bindItemComponents(lookup);
        bindItemTags();
        ServerLevelGate.load("useWithoutItem");
        Holder<DimensionType> overworld = lookup
            .lookupOrThrow(Registries.DIMENSION_TYPE)
            .getOrThrow(BuiltinDimensionTypes.OVERWORLD);

        TableLevel level = new TableLevel(accessOf(lookup), overworld);
        HarnessPlayer player = new HarnessPlayer(level);
        pinHome(player);
        mintFiller();

        List<String> rows = new ArrayList<>();
        int blocksWithBehaviour = 0;
        int statesProbed = 0;
        int statesWriting = 0;
        int refusals = 0;
        int randomBlocks = 0;
        int probeDependentBlocks = 0;
        int probeOnlyBlocks = 0;
        int faceOnlyBlocks = 0;
        // Counted here as well as derived by `ci/server-level-gates.py` from
        // the bytecode and the table. Two independent routes to one number is
        // the point: the check compares them and a stamping mechanism that
        // silently stops firing cannot agree with a bytecode walk that has not.
        int gatedBlocks = 0;
        int gatedCells = 0;
        Map<String, Integer> refusedBy = new LinkedHashMap<>();

        for (Block block : BuiltInRegistries.BLOCK) {
            Identifier id = BuiltInRegistries.BLOCK.getKey(block);
            ServerLevelGate gate = ServerLevelGate.of(block);
            List<String> cells = new ArrayList<>();
            boolean any = false;
            boolean faceSensitive = false;
            // Stances whose recorded row differed from `base`, and stances that
            // could not be read because the harness refused where `base` did
            // not. Kept apart on purpose: "measured to move" and "could not be
            // measured" are different findings, and a sweep that merged them
            // would charge a block for this harness's missing level model.
            java.util.Set<String> probeAxes = new TreeSet<>();
            java.util.Set<String> probeUnknown = new TreeSet<>();

            boolean random = false;

            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                statesProbed++;
                // **Twice per face, and the pair must agree.** A block can
                // answer a right-click with a die roll — `DragonEggBlock`
                // teleports itself to a random position — and writing one
                // sample of that into a golden file is how a file churns.
                // `PlacementGolden` makes the same call for the same reason.
                // This was not designed in: `ci/use-golden.sh` runs the whole
                // dumper twice and compares, and the dragon egg is what that
                // check caught on its first green run.
                Probe first = null;
                for (Direction face : FACES) {
                    // Every seed in `SEEDS`, and they must all agree. A rule
                    // whose carried answer moves with the seed is a die roll
                    // and its cells are dropped. Fixed seeds, so this is the
                    // same question every run — see `TableLevel.harnessRandom`.
                    Probe p = probe(level, player, STANCES[0], state, face, SEEDS[0]);
                    // **The pitch is measured across the same sweep, not
                    // inferred from `usedRandom`.** This column first shipped
                    // as a snapshot of "did the rule touch the RNG", which is
                    // precisely the detector `TableLevel.harnessRandom`'s own
                    // doc comment records as tried and wrong: touching the
                    // RNG is not the same as the answer depending on it. It
                    // over-reported by 21 note-block instruments — 1,050
                    // states whose pitch is `2^((note-12)/12)` and does not
                    // move — and each one costs a hand-port that the table
                    // could have driven.
                    //
                    // So it joins the family the paragraph above names:
                    // `SEEDS` sets `random`, `FACES` sets `faceSensitive`,
                    // `STANCES` sets `probeDependent`, and now `SEEDS` also
                    // sets `soundPitchDrawn`. Ask the question eight times
                    // and record whether the answer moved.
                    boolean pitchMoved = false;
                    for (int s = 1; s < SEEDS.length; s++) {
                        Probe alt = probe(level, player, STANCES[0], state, face, SEEDS[s]);
                        if (!p.sameAnswer(alt)) {
                            random = true;
                        }
                        if (p.sounded && alt.sounded
                                && Float.compare(p.soundPitch, alt.soundPitch) != 0) {
                            pitchMoved = true;
                        }
                    }
                    p.soundPitchDrawn = pitchMoved;
                    // The stance sweep, against this face's own `base` probe.
                    // One seed, because `SEEDS` has already answered the
                    // question a second seed would: a rule whose answer moves
                    // with the die is `random` and keeps no cells at all.
                    for (int st = 1; st < STANCES.length; st++) {
                        if (!STANCES[st].available()) {
                            continue;
                        }
                        Probe q = probe(level, player, STANCES[st], state, face, SEEDS[0]);
                        if (q.sameRow(p)) {
                            continue;
                        }
                        if (p.refusal == null && q.refusal != null) {
                            probeUnknown.add(STANCES[st].name);
                        } else {
                            probeAxes.add(STANCES[st].name);
                        }
                    }
                    if (first == null) {
                        first = p;
                    } else if (!p.sameAnswer(first)) {
                        faceSensitive = true;
                    }
                }
                // **Stamped after the face and stance sweeps, never before.**
                // The gate is a property of `(block, state)` and not of the
                // player or the face, so stamping earlier would put it on both
                // sides of every `sameRow` comparison and change nothing — but
                // it would also make `probeAxes` depend on a column no stance
                // can move, which is a different claim than the one being made.
                //
                // And never on a refusal: `SignBlock` is behind this gate and
                // vanilla announces it, so those cells are already refusals and
                // a refusal is the louder record.
                if (first.refusal == null && gate != null && gate.reaches(state)) {
                    first.gated.addAll(gate.columns());
                }
                if (first.refusal != null) {
                    refusals++;
                    refusedBy.merge(first.refusal, 1, Integer::sum);
                }
                if (first.interesting()) {
                    any = true;
                    statesWriting++;
                    if (!first.gated.isEmpty()) {
                        gatedCells++;
                    }
                    cells.add(first.json(state));
                }
            }

            String gateCol = gate == null ? "" : gate.json();
            if (gate != null) {
                gatedBlocks++;
            }
            String probeCols = ", \"probeDependent\": " + !probeAxes.isEmpty()
                + ", \"probeAxes\": [" + quotedList(probeAxes) + "]"
                + ", \"probeUnknown\": [" + quotedList(probeUnknown) + "]";
            if (any) {
                blocksWithBehaviour++;
                if (!probeAxes.isEmpty()) {
                    probeDependentBlocks++;
                }
                if (random) {
                    // No cells, deliberately. A consumer that read one sample
                    // would reproduce a state vanilla only sometimes writes.
                    randomBlocks++;
                    rows.add("    {\"block\": " + quote(id.toString())
                        + ", \"class\": " + quote(block.getClass().getSimpleName())
                        + ", \"random\": true, \"faceSensitive\": false"
                        + probeCols + gateCol
                        + ", \"states\": []}");
                } else {
                    rows.add("    {\"block\": " + quote(id.toString())
                        + ", \"class\": " + quote(block.getClass().getSimpleName())
                        + ", \"random\": false"
                        + ", \"faceSensitive\": " + faceSensitive
                        + probeCols + gateCol
                        + ", \"states\": [\n      "
                        + String.join(",\n      ", cells) + "\n    ]}");
                }
            } else if (!probeAxes.isEmpty() || faceSensitive) {
                // **A block the base probe cannot see at all.** Every state was
                // uninteresting under `STANCES[0]` and at least one stance made
                // one interesting, so this block has a use rule and the table
                // has always been silent about it. Emitted with no cells rather
                // than left out: an absent row is indistinguishable from a
                // block with no rule, which is the exact confusion this column
                // exists to end.
                //
                // **`faceSensitive` is on the same footing and was not, which
                // hid two more blocks.** Every recorded cell is the `FACES[0]`
                // probe, and for `BellBlock` and `ChiseledBookShelfBlock` that
                // face is the one where nothing happens: a bell rings only on a
                // hit whose face and height `isProperHit` accepts, and a
                // chiseled bookshelf's `getHitSlot` answers empty for any face
                // that is not its `FACING`. Both answer under `NORTH`, both
                // were uninteresting under `UP`, and both therefore had no row
                // at all — not a refusal, not an empty row, nothing. The face
                // sweep already knew; only the emit did not ask it.
                if (probeAxes.isEmpty()) {
                    faceOnlyBlocks++;
                } else {
                    probeOnlyBlocks++;
                    probeDependentBlocks++;
                }
                rows.add("    {\"block\": " + quote(id.toString())
                    + ", \"class\": " + quote(block.getClass().getSimpleName())
                    + ", \"random\": " + random
                    + ", \"faceSensitive\": " + faceSensitive
                    + probeCols + gateCol
                    + ", \"states\": []}");
            }
        }

        // **The alarm against itself.** Every piece of the gate column can be
        // present and inert: the file loads, `of()` walks a chain that matches
        // nothing, `reaches` is never called, and every row goes back to saying
        // what it said before. That is exactly the shape of the `blockEntityAsked`
        // field that was set on every ask and read by nobody from March to
        // August. So the dumper refuses to write a table it did not stamp.
        if (ServerLevelGate.resolved() == 0 || gatedCells == 0) {
            throw new IllegalStateException(
                "the ServerLevel gate column resolved " + ServerLevelGate.resolved()
                + " block(s) and stamped " + gatedCells + " cell(s) — the "
                + "mechanism is inert, and an inert one produces exactly the "
                + "table this column was added to replace");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"what\": \"BlockState.useWithoutItem, per state\",\n");
        out.append("  \"blocksWithBehaviour\": ").append(blocksWithBehaviour).append(",\n");
        out.append("  \"statesProbed\": ").append(statesProbed).append(",\n");
        out.append("  \"statesWriting\": ").append(statesWriting).append(",\n");
        out.append("  \"refusals\": ").append(refusals).append(",\n");
        out.append("  \"randomBlocks\": ").append(randomBlocks).append(",\n");
        out.append("  \"probeDependentBlocks\": ").append(probeDependentBlocks).append(",\n");
        out.append("  \"probeOnlyBlocks\": ").append(probeOnlyBlocks).append(",\n");
        out.append("  \"faceOnlyBlocks\": ").append(faceOnlyBlocks).append(",\n");
        out.append("  \"serverLevelGatedBlocks\": ").append(gatedBlocks).append(",\n");
        out.append("  \"serverLevelGatedCells\": ").append(gatedCells).append(",\n");
        // **The stance every recorded cell was taken under, written into the
        // file.** This is the half of the fix that does not need a consumer to
        // change: a reader that wants to know whether a row is a fact about the
        // block or a fact about one player can now read what that player was.
        out.append("  \"probe\": ").append(STANCES[0].json()).append(",\n");
        out.append("  \"probeStances\": [\n");
        for (int i = 1; i < STANCES.length; i++) {
            out.append("    ").append(STANCES[i].json())
                .append(i < STANCES.length - 1 ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"probeStancesUnavailable\": {");
        List<String> gone = new ArrayList<>();
        for (Stance s : STANCES) {
            if (!s.available()) {
                gone.add(quote(s.name) + ": " + quote(String.valueOf(FILLER_WHY)));
            }
        }
        out.append(String.join(", ", gone)).append("},\n");
        out.append("  \"refusedBy\": {\n");
        int written = 0;
        for (Map.Entry<String, Integer> e : refusedBy.entrySet()) {
            written++;
            out.append("    ").append(quote(e.getKey())).append(": ").append(e.getValue())
                .append(written < refusedBy.size() ? ",\n" : "\n");
        }
        out.append("  },\n  \"table\": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }

    // ----------------------------------------------------------------------
    // Shared harness setup.
    //
    // Extracted from `main` rather than left inline because
    // `UseItemOnGolden` — the `useItemOn` half of the same question — has to
    // build the *same* player in the *same* world or its table cannot be read
    // beside this one. Two copies of "construct the probe" is how the two
    // tables would end up describing two different players while both called
    // it `base`.
    // ----------------------------------------------------------------------

    /// Vanilla's own "you are calling me in a context I do not support"
    /// alarm, which this harness had been muting.
    ///
    /// # The thing that was silent
    ///
    /// `SignBlock.useWithoutItem` opens with
    ///
    /// ```java
    /// if (!(level instanceof ServerLevel serverLevel)) {
    ///     Util.pauseInIde(new IllegalStateException("Expected to only call this on server"));
    ///     return InteractionResult.CONSUME;
    /// }
    /// ```
    ///
    /// `TableLevel` is not a `ServerLevel` and cannot be — `ServerLevel`'s
    /// constructor wants a whole server — so that branch is taken for every
    /// sign in the game. The recorded row then says `consumes: true, writes:
    /// []`, which is a plausible answer, is right by coincidence, and was
    /// reached without running a line of the rule. **This is the same fault as
    /// the null block entity in a costume this harness had no way to see
    /// through**: an early return, taken because of what the harness is, stated
    /// as a fact about the block.
    ///
    /// # Why the fix is one line of vanilla's own code
    ///
    /// Vanilla *announces* it. `Util.pauseInIde` is handed an exception whose
    /// message says exactly what went wrong, and then does nothing at all
    /// unless `SharedConstants.IS_RUNNING_IN_IDE` — which is `public static
    /// boolean`, not final. `Util.setPause(Consumer<String>)` is public too.
    /// So the alarm is armed rather than reconstructed, and every site in the
    /// jar that raises it is covered, not only the ones somebody thought to
    /// look for. A harness that invented its own list of "contexts vanilla
    /// dislikes" would be a list that goes stale; this one cannot.
    ///
    /// The message is kept and becomes the refusal, so the row names vanilla's
    /// complaint rather than this dumper's guess at it.
    static String vanillaPaused;

    static void armVanillasOwnAlarm() {
        // **Turn the alarm's own siren off first.** `pauseInIde` logs the
        // exception with a stack trace through `Util`'s logger before it
        // reaches the pauser, once per occurrence, and log4j's console
        // appender writes to the same stdout this dumper's JSON goes to. Armed
        // and left noisy, the signs alone print tens of thousands of stack
        // traces into the middle of the table. One logger is silenced, not the
        // root, so a real failure anywhere else still says so — and
        // `ci/use-golden.sh` refuses any output whose first byte is not `{`,
        // which is what fires if this ever stops working.
        org.apache.logging.log4j.core.config.Configurator.setLevel(
            "net.minecraft.util.Util", org.apache.logging.log4j.Level.OFF);
        SharedConstants.IS_RUNNING_IN_IDE = true;
        net.minecraft.util.Util.setPause(message -> vanillaPaused = message);
        // The positive control, and it is not ceremony: `setPause` and the
        // flag are two independent switches and either one left off makes this
        // whole mechanism a silent no-op — which is the exact failure this
        // dumper exists to refuse. Raise vanilla's alarm here and require it
        // to come back.
        vanillaPaused = null;
        net.minecraft.util.Util.pauseInIde(
            new IllegalStateException("UseGolden arming its refusal on vanilla's own alarm"));
        if (vanillaPaused == null) {
            throw new IllegalStateException(
                "Util.pauseInIde did not reach the pauser — every rule that bails out with "
                + "\"Expected to only call this on server\" would be recorded as a fact "
                + "about the block instead of a refusal");
        }
        vanillaPaused = null;
    }

    /// Read the freshly constructed player into `Stance.HOME`, and pin its
    /// facing.
    ///
    /// Read once, before any stance has touched it, so `STANCES[0]` is the
    /// identity and the recorded cells are the ones this dumper has always
    /// recorded. Typing these in would make `base` a *thirteenth* stance that
    /// merely resembles the old player.
    ///
    /// **The constructed player's yRot is a die roll, and has been all
    /// along.** `LivingEntity`'s constructor is
    /// `setYRot(random.nextFloat() * 6.2831855f)` — a uniform [0,1) times
    /// **2 pi**, into a field measured in **degrees**. So a fresh player faces
    /// somewhere in [0, 6.283) degrees, one run's was 0.522 and the next
    /// run's 5.360, and the two-run `cmp` in `ci/use-golden.sh` caught it the
    /// moment this dumper started writing the number down.
    ///
    /// Every one of those values is inside `Direction.fromYRot`'s southern
    /// bucket, [-45, 45), so `player.getDirection()` has always answered
    /// `SOUTH` and the table has always been stable. That is the whole shape
    /// of this project's signature defect: a random input that could not be
    /// seen because its range was narrower than the only consumer's
    /// resolution. Pin it, so `base` is a stated stance rather than a lucky
    /// one — and check the pin is a no-op rather than assuming it, because if
    /// that constant is ever 360.0f this becomes a table that churns.
    static void pinHome(HarnessPlayer player) {
        Stance.HOME = player.position();
        Stance.HOME_X_ROT = player.getXRot();
        Stance.HOME_FOOD = player.getFoodData().getFoodLevel();
        float constructed = player.getYRot();
        if (Direction.fromYRot(constructed) != Direction.SOUTH) {
            throw new IllegalStateException("a freshly constructed player faces "
                + Direction.fromYRot(constructed) + " (yRot " + constructed + "), not SOUTH — "
                + "`base` is no longer the player every recorded cell was taken under, "
                + "and this table's history is not comparable to what it is about to write");
        }
        Stance.HOME_Y_ROT = 0.0f;
    }

    /// Mint the stack `full_inventory` packs the player with.
    ///
    /// Still guarded, and the guard is not vestigial: `bindItemComponents`
    /// runs first and is what makes this succeed, so a failure here now means
    /// the binding did not take. Recording it as an unavailable stance rather
    /// than throwing keeps that a stated blind spot instead of a dead dumper.
    static void mintFiller() {
        try {
            FILLER = new ItemStack(Items.BEDROCK, 1);
        } catch (Throwable t) {
            String m = t.getMessage();
            FILLER_WHY = t.getClass().getSimpleName() + (m == null ? "" : ": " + m);
        }
    }

    /// Bind every registry's data components, so an `ItemStack` can be built.
    ///
    /// **Until 2026-08-12 nothing here did this, and it cost two things at
    /// once.** `new ItemStack(item)` reads `Holder.Reference.components()`,
    /// which throws `Components not bound yet` on a JVM that has only run
    /// `Bootstrap.bootStrap()`. So `use_without_item.json` carried **39**
    /// `NullPointerException: Components not bound yet` refusals — 38
    /// `FlowerPotBlock` rows, whose `useWithoutItem` hands the pot's content
    /// back through `new ItemStack(content)`, and `minecraft:composter` — and
    /// the `full_inventory` stance could not be set up at all, so
    /// `FlowerPotBlock`'s `Player.addItem` axis was reported unswept.
    ///
    /// `Item.<init>` registers an initializer into
    /// `BuiltInRegistries.DATA_COMPONENT_INITIALIZERS`; a real server runs
    /// them from `ReloadableServerResources.updateComponentsAndStaticRegistryTags`,
    /// and Mojang's own `RegistryComponentsReport` runs the same two calls
    /// this does. Measured on the pinned jar 2026-08-12: 141 pending
    /// registries apply, after which **all 1,537 items mint a stack** and none
    /// fail.
    ///
    /// Deliberately not silent about doing nothing. A binding that stopped
    /// working would put the 39 refusals back and read as a block-behaviour
    /// change; the count is asserted instead.
    static void bindItemComponents(HolderLookup.Provider lookup) {
        int applied = 0;
        for (net.minecraft.core.component.DataComponentInitializers.PendingComponents<?> pending
                : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(lookup)) {
            pending.apply();
            applied++;
        }
        if (applied == 0) {
            throw new IllegalStateException(
                "DATA_COMPONENT_INITIALIZERS produced nothing to apply — every ItemStack "
                + "this harness builds would throw `Components not bound yet`, and every "
                + "row that needs one would be recorded as a refusal");
        }
        // The positive control. `Items.BEDROCK` carries no interesting
        // components, which is the point: if the plainest item in the game
        // cannot be minted, nothing downstream is measuring what it says.
        new ItemStack(Items.BEDROCK, 1);
    }

    /// Bind `#minecraft:...` item tags into `BuiltInRegistries.ITEM`.
    ///
    /// **An unbound tag does not throw, it says no** — the same fault
    /// `SurfaceChunkGolden.bindBlockTags` exists for, and the same one
    /// `TableLevel.isWaterAt` steps around for `FluidTags.WATER`. Without
    /// this, `ItemStack.is(TagKey)` is false for every item in the game, and
    /// the `useItemOn` half of the use path is read *entirely* through that
    /// call by five block classes: `NoteBlock`
    /// (`ItemTags.NOTE_BLOCK_TOP_INSTRUMENTS`), `ChiseledBookShelfBlock`
    /// (`BOOKSHELF_BOOKS`), `CakeBlock` (`CANDLES`), `LecternBlock`
    /// (`LECTERN_BOOKS`) and the two copper golem statues (`AXES`). Their
    /// rows would come back "does nothing" for every item, uniformly and
    /// without an error anywhere.
    ///
    /// Mojang's own datagen output is the source, read out of
    /// `-Drustmc.data`, exactly as the block and fluid tags are.
    static void bindItemTags() {
        String data = System.getProperty("rustmc.data");
        if (data == null) {
            throw new IllegalStateException("set -Drustmc.data to the generated-data directory");
        }
        java.nio.file.Path dir =
            java.nio.file.Path.of(data, "data", "minecraft", "tags", "item");
        Map<String, List<String>> raw = new LinkedHashMap<>();
        // **Recursive, and that is not a refinement.** Item tags nest —
        // `tags/item/sulfur_cube_archetype/bouncy.json` is
        // `#minecraft:sulfur_cube_archetype/bouncy` — and a flat
        // `Files.list` misses every one of them. It does not miss them
        // quietly: the first tag that *includes* a nested one dies in
        // `flattenTag` with "does not exist", which is how this was found.
        try (var files = java.nio.file.Files.walk(dir)) {
            for (java.nio.file.Path file : files.sorted().toList()) {
                String name = file.getFileName().toString();
                if (!name.endsWith(".json") || !java.nio.file.Files.isRegularFile(file)) {
                    continue;
                }
                com.google.gson.JsonObject json = com.google.gson.JsonParser
                    .parseString(java.nio.file.Files.readString(file)).getAsJsonObject();
                List<String> entries = new ArrayList<>();
                for (com.google.gson.JsonElement element : json.getAsJsonArray("values")) {
                    entries.add(element.isJsonObject()
                        ? element.getAsJsonObject().get("id").getAsString()
                        : element.getAsString());
                }
                String rel = dir.relativize(file).toString().replace(java.io.File.separatorChar, '/');
                raw.put("minecraft:" + rel.substring(0, rel.length() - 5), entries);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read item tags from " + dir, e);
        }
        Map<net.minecraft.tags.TagKey<net.minecraft.world.item.Item>,
            List<Holder<net.minecraft.world.item.Item>>> bound = new LinkedHashMap<>();
        for (String name : raw.keySet()) {
            java.util.Set<String> flat = new java.util.LinkedHashSet<>();
            flattenTag(name, raw, flat, new java.util.LinkedHashSet<>());
            List<Holder<net.minecraft.world.item.Item>> holders = new ArrayList<>();
            for (String item : flat) {
                BuiltInRegistries.ITEM.get(Identifier.parse(item)).ifPresent(holders::add);
            }
            bound.put(
                net.minecraft.tags.TagKey.create(Registries.ITEM, Identifier.parse(name)),
                holders);
        }
        BuiltInRegistries.ITEM
            .prepareTagReload(
                new net.minecraft.tags.TagLoader.LoadResult<>(Registries.ITEM, bound))
            .apply();
        // The positive control, for the reason the whole method exists: a tag
        // that binds to nothing is indistinguishable from a tag that was never
        // bound, and both answer `false` to every question.
        if (!new ItemStack(Items.STONE_AXE, 1).is(net.minecraft.tags.ItemTags.AXES)) {
            throw new IllegalStateException(
                "item tags did not bind — `stone_axe` is not in `#minecraft:axes`. Every "
                + "`ItemStack.is(TagKey)` on the use path would answer false and every "
                + "row that turns on one would read as `does nothing`");
        }
    }

    /// `#other_tag` entries, resolved. Shared by every tag folder this
    /// harness binds.
    private static void flattenTag(
        String name, Map<String, List<String>> raw,
        java.util.Set<String> out, java.util.Set<String> visiting
    ) {
        if (!visiting.add(name)) {
            throw new IllegalStateException("item tag cycle at " + name);
        }
        List<String> entries = raw.get(name);
        if (entries == null) {
            throw new IllegalStateException("item tag " + name + " does not exist");
        }
        for (String entry : entries) {
            if (entry.startsWith("#")) {
                flattenTag(entry.substring(1), raw, out, visiting);
            } else {
                out.add(entry);
            }
        }
        visiting.remove(name);
    }

    /// One right-click, and what it did.
    /// # THE GATE THE HARNESS CANNOT SEE ITSELF FAIL
    ///
    /// `if (level instanceof ServerLevel serverLevel)`. `TableLevel` is not one
    /// and cannot be — `ServerLevel`'s constructor wants a whole server — so
    /// the branch is taken for every state of every block under one of these
    /// rules, and the rule returns a plausible value without running.
    ///
    /// **`instanceof` is answered by the JVM against the object's class.** A
    /// `Level` subclass cannot observe the question, cannot refuse it and
    /// cannot log it. There is no hook, which is why this fact is derived
    /// *outside* the run — `ci/server-level-gates.py` walks the bytecode — and
    /// handed in through `-Drustmc.gates`. [`SignBlock`] is the sole exception
    /// in the registry and only because vanilla itself announces the branch
    /// through `Util.pauseInIde`; see [`armVanillasOwnAlarm`].
    ///
    /// What this class adds is the half only the dumper can do: the real
    /// `BlockState`. `ci/server-level-gates.py` can say *what* the gate hides
    /// and *which column* would have carried it, but "which cells reach the
    /// gate at all" needs the state, and for two of the eight rules it is a
    /// property test — `age > 1` on the berry bush, `charges > 0` on the
    /// respawn anchor.
    record ServerLevelGate(
        String cls, String verdict, String guard,
        List<String> hides, List<String> columns) {

        /// Every gate, by the simple class name the bytecode named, for one
        /// probe method. Loaded once.
        private static Map<String, ServerLevelGate> byClass;
        /// Blocks a gate was resolved for. **A counter, and it is checked.**
        /// A stamping mechanism that resolves nothing is the
        /// `blockEntityAsked` field that sat unread for four months: the code
        /// is all present, every grep passes, and no row says anything.
        private static int resolved;

        static void load(String probe) {
            String path = System.getProperty("rustmc.gates");
            if (path == null) {
                throw new IllegalStateException(
                    "set -Drustmc.gates to data/vanilla/server_level_gates.json "
                    + "(ci/server-level-gates.py --emit writes it) — without it "
                    + "every row behind an `instanceof ServerLevel` goes back to "
                    + "saying nothing, and nothing else in the tree would notice");
            }
            Map<String, ServerLevelGate> found = new LinkedHashMap<>();
            try {
                com.google.gson.JsonObject doc = com.google.gson.JsonParser
                    .parseString(java.nio.file.Files.readString(
                        java.nio.file.Path.of(path))).getAsJsonObject();
                for (com.google.gson.JsonElement e : doc.getAsJsonArray("gates")) {
                    com.google.gson.JsonObject g = e.getAsJsonObject();
                    if (!probe.equals(g.get("probe").getAsString())) {
                        continue;
                    }
                    found.put(g.get("class").getAsString(), new ServerLevelGate(
                        g.get("class").getAsString(),
                        g.get("verdict").getAsString(),
                        g.get("guard").getAsString(),
                        strings(g.getAsJsonArray("hides")),
                        strings(g.getAsJsonArray("columns"))));
                }
            } catch (java.io.IOException ex) {
                throw new IllegalStateException("cannot read " + path, ex);
            }
            if (found.isEmpty()) {
                throw new IllegalStateException(
                    "no `" + probe + "` gates in " + path + " — the file is "
                    + "generated and this dumper's whole gate column depends on "
                    + "it, so an empty one is a broken pipeline and not a clean "
                    + "world");
            }
            byClass = found;
            resolved = 0;
        }

        private static List<String> strings(com.google.gson.JsonArray a) {
            List<String> out = new ArrayList<>();
            for (com.google.gson.JsonElement e : a) {
                out.add(e.getAsString());
            }
            return out;
        }

        /// The gate over this block, found by walking its **own** class chain.
        ///
        /// Matched on the runtime class rather than on a transcribed list of
        /// block ids: `ChestBlock` covers ten blocks and `ShulkerBoxBlock`
        /// seventeen, and a list of twenty-seven ids in this file would be a
        /// second copy of something the registry already knows and the ledger
        /// already derives.
        static int resolved() {
            return resolved;
        }

        static ServerLevelGate of(Block block) {
            for (Class<?> c = block.getClass(); c != null; c = c.getSuperclass()) {
                ServerLevelGate g = byClass.get(c.getSimpleName());
                if (g != null) {
                    resolved++;
                    return g;
                }
            }
            return null;
        }

        /// Whether this state reaches the gate.
        ///
        /// `no-branch` — the gate is the body's first branch, so every state
        /// does. `all-states` — something branches first but nothing that
        /// depends on the state in this harness, and the ledger owes a
        /// `guard_why` saying which emptiness makes it so. Otherwise a
        /// predicate on an integer property, evaluated against the real state
        /// because that is the one thing the bytecode walk cannot do.
        boolean reaches(BlockState state) {
            return reaches(state, null);
        }

        /// Whether the cell `(state, stack)` reaches the gate.
        ///
        /// `stack` is `null` for a table with no item axis, and an `item-`
        /// term against one **throws** rather than defaulting either way: a
        /// guard that names an axis the table does not have is a ledger error
        /// and quietly stamping every cell or none is how it would survive.
        ///
        /// The grammar is a `&&` conjunction — `ci/server-level-gates.py`
        /// holds the whole of it and evaluates the same terms against the
        /// serialised strings, which is the second of the two routes
        /// `--check` compares.
        boolean reaches(BlockState state, ItemStack stack) {
            for (String raw : guard.split("&&")) {
                String t = raw.trim();
                if (t.equals("no-branch") || t.equals("all-states")) {
                    continue;
                }
                if (t.equals("item-unknown")) {
                    // The branch turns on something this harness cannot
                    // evaluate. Stamping nothing is the honest floor and it
                    // is not the same as stamping everything.
                    return false;
                }
                if (t.startsWith("item")) {
                    if (stack == null) {
                        throw new IllegalStateException(
                            quote(t) + " on " + cls + ", whose table has no "
                            + "item axis — a guard cannot name one");
                    }
                    if (t.equals("item-any")) {
                        if (stack.isEmpty()) {
                            return false;
                        }
                        continue;
                    }
                    String id = net.minecraft.core.registries.BuiltInRegistries
                        .ITEM.getKey(stack.getItem()).toString();
                    boolean hit = false;
                    for (String want : t.substring(t.indexOf('=') + 1).split(",")) {
                        hit |= want.trim().equals(id);
                    }
                    if (!hit) {
                        return false;
                    }
                    continue;
                }
                if (!propertyTerm(t, state)) {
                    return false;
                }
            }
            return true;
        }

        /// `<property><op><value>` against the real state. `<` and `>` are
        /// integer orderings; `=` compares the property's own printed value,
        /// so `vault_state=active` works on an enum without this having to
        /// know the enum.
        private boolean propertyTerm(String t, BlockState state) {
            java.util.regex.Matcher m = java.util.regex.Pattern
                .compile("^([a-z_]+)([<>=])([a-z0-9_]+)$").matcher(t);
            if (!m.matches()) {
                throw new IllegalStateException(
                    "cannot read guard term " + quote(t) + " for " + cls
                    + " — the vocabulary is `no-branch`, `all-states`, "
                    + "`item-any`, `item-unknown`, `item=<ids>` or "
                    + "`<property><op><value>`, and an unreadable one must "
                    + "not quietly stamp every cell");
            }
            String want = m.group(1);
            for (net.minecraft.world.level.block.state.properties.Property<?> prop
                    : state.getProperties()) {
                if (!prop.getName().equals(want)) {
                    continue;
                }
                if (m.group(2).equals("=")) {
                    return printed(state, prop).equals(m.group(3));
                }
                if (!(prop instanceof net.minecraft.world.level.block.state
                        .properties.IntegerProperty ip)) {
                    throw new IllegalStateException(
                        want + " on " + cls + " is not an integer property, "
                        + "so " + quote(t) + " has no ordering to test");
                }
                int have = state.getValue(ip);
                int n = Integer.parseInt(m.group(3));
                return m.group(2).equals(">") ? have > n : have < n;
            }
            throw new IllegalStateException(
                cls + " has no property " + quote(want) + " — the guard in "
                + "ci/checks/server-level-gates.toml names one the block does "
                + "not have, so no cell would ever be stamped");
        }

        @SuppressWarnings("unchecked")
        private static <T extends Comparable<T>> String printed(
                BlockState state,
                net.minecraft.world.level.block.state.properties.Property<?> prop) {
            var p = (net.minecraft.world.level.block.state.properties.Property<T>) prop;
            return p.getName(state.getValue(p));
        }

        String json() {
            return ", \"serverLevelGate\": {\"verdict\": " + quote(verdict)
                + ", \"guard\": " + quote(guard)
                + ", \"hides\": [" + quotedList(hides) + "]"
                + ", \"columns\": [" + quotedList(columns) + "]}";
        }
    }

    private static final class Probe {
        InteractionResult result;
        /// Offset from the clicked position -> the state written there.
        Map<BlockPos, Write> writes = new LinkedHashMap<>();
        boolean scheduled;
        /// Whether the rule drew from the level's RNG. See
        /// [`TableLevel.usedRandom`].
        boolean usedRandom;
        /// Ticks from now until the scheduled tick fires, or -1 for none.
        long tickDelay = -1L;
        /// `TickPriority`'s own name, or null. Carried rather than assumed
        /// `NORMAL`: `crate::ticks`'s drain order sorts on it.
        String tickPriority;
        /// Offset from the clicked position of what was scheduled.
        BlockPos tickAt;
        /// **Which queue it went into**, `"block"` or `"fluid"`.
        ///
        /// Not cosmetic. `TrapDoorBlock.toggle` ends with
        /// `if (state.getValue(WATERLOGGED)) level.scheduleTick(pos,
        /// Fluids.WATER, Fluids.WATER.getTickDelay(level))` — a **fluid**
        /// tick, and `TrapDoorBlock` has no `tick` override at all. All 640
        /// trapdoor rows here are that. A consumer reading `schedulesTick`
        /// without the queue puts a *block* tick for `oak_trapdoor` in its
        /// queue, which fires and finds no rule, and which the
        /// `UNIQUE_TICK_HASH` guard then makes block a real block tick at
        /// that position.
        String tickKind;
        boolean sounded;
        /// Which sound, and whether its pitch is quotable. See the fields of
        /// the same names on the level for why each one is here.
        String soundEvent;
        String soundSource;
        float soundVolume;
        float soundPitch;
        int soundCount;
        boolean soundPitchDrawn;
        /// The screens the rule asked the player to open, or null. See
        /// [`HarnessPlayer.screens`].
        String screens;
        /// Whether the rule changed a block entity's own data.
        boolean beWritten;
        /// **What this harness answered out of its own emptiness, and the
        /// rule then branched on.**
        ///
        /// A refusal says "not measured". A row with no reads says "measured,
        /// and here is what the block does". This says the third thing, which
        /// had no way to be said before: *measured, and the answer turns on
        /// something this world is empty of.* A consumer that reads such a row
        /// as a fact about the block is now doing so knowingly, exactly as
        /// `probeDependent` did for the player.
        ///
        /// The vocabulary is closed and each entry names the emptiness:
        ///
        ///   `air-world`         read a position this world holds nothing at
        ///   `no-entities`       swept the entity index, which is empty
        ///   `no-block-entity`   asked for a block entity away from a block
        ///                       this world holds
        ///
        /// `block-entity` is deliberately **not** on that list: since this
        /// level builds the real one, consulting it is an answer and not an
        /// emptiness.
        final TreeSet<String> reads = new TreeSet<>();
        /// **The columns of this very cell that the `instanceof ServerLevel`
        /// gate decided, rather than the block.**
        ///
        /// `reads` says "measured, and the answer turns on something this world
        /// is empty of". This says the fourth thing: *not measured at all, and
        /// here is which of the fields below to disbelieve.* The vocabulary is
        /// the column names of this table — `opens`, `writes`, `sound`,
        /// `random`, `schedulesTick`, `blockEntityWritten` — because the only
        /// question a consumer has is which field to stop trusting.
        ///
        /// A refused cell is never stamped: a refusal already says "not
        /// measured" and says it louder. That is why all 1,344 sign cells stay
        /// pure refusals and `SignBlock`'s `live` count in the ledger is 0.
        final TreeSet<String> gated = new TreeSet<>();
        boolean soundExcludesActor;
        /// Non-null when the call read something this harness does not model.
        String refusal;

        /// Whether the row says anything a consumer needs. A block that
        /// wrote nothing and declined is the answer for most of the
        /// registry and is not worth a row.
        /// Whether the row says anything a consumer needs.
        ///
        /// `!reads.isEmpty()` is the clause this branch added and it is the
        /// whole point of the branch: a cell that wrote nothing *because this
        /// world is empty* is not the same as a cell that wrote nothing, and
        /// dropping it is what made 100 registry blocks vanish out of two
        /// tables with no refusal to mark the hole. `screens` is on the same
        /// footing — a right-click that opens a chest is not inert.
        boolean interesting() {
            return refusal != null || !writes.isEmpty() || !reads.isEmpty()
                || !gated.isEmpty()
                || screens != null || beWritten
                || (result != null && result.consumesAction());
        }

        /// Everything the row records, `sound` included.
        ///
        /// Not `sameAnswer`, which is deliberately left alone: that one drives
        /// `random` and `faceSensitive` and re-deciding those is a different
        /// branch's business. It also does not compare `sounded`, so a
        /// difference that moved only the `sound` column would be invisible to
        /// it — and `sound` is a column a consumer reads.
        /// **`soundPitchDrawn` is deliberately NOT compared, and leaving it
        /// in cost a whole re-extraction.** It is measured *across* the seed
        /// sweep and written onto one probe afterwards, so the stance probes
        /// it is compared against always carry the default `false`. Every
        /// drawn-pitch block therefore compared true-against-false and came
        /// back probe-dependent on all eleven axes — every door, trapdoor and
        /// fence gate, `full_inventory` and `game_master` included, which is
        /// a sound rule depending on the player's inventory and is absurd on
        /// its face. It belongs with `random` and `faceSensitive`, which are
        /// properties of a sweep rather than of a probe and are not compared
        /// here either.
        ///
        /// **Every other sound column is compared, not just `sounded`.** The
        /// paragraph above records that `sameAnswer` omitting `sounded` made
        /// a difference in that column invisible; adding four more columns
        /// and comparing only the boolean would rebuild exactly that hole one
        /// level down, where two states that both sound but sound
        /// *differently* would collapse into one row.
        boolean sameRow(Probe other) {
            return sameAnswer(other)
                && sounded == other.sounded
                && soundCount == other.soundCount
                && soundExcludesActor == other.soundExcludesActor
                && String.valueOf(soundEvent).equals(String.valueOf(other.soundEvent))
                && String.valueOf(soundSource).equals(String.valueOf(other.soundSource))
                && Float.compare(soundVolume, other.soundVolume) == 0
                && Float.compare(soundPitch, other.soundPitch) == 0
                && String.valueOf(screens).equals(String.valueOf(other.screens))
                && beWritten == other.beWritten
                && reads.equals(other.reads)
                && gated.equals(other.gated);
        }

        boolean sameAnswer(Probe other) {
            return String.valueOf(refusal).equals(String.valueOf(other.refusal))
                && writes.equals(other.writes)
                && scheduled == other.scheduled
                && tickDelay == other.tickDelay
                && String.valueOf(tickPriority).equals(String.valueOf(other.tickPriority))
                && String.valueOf(tickAt).equals(String.valueOf(other.tickAt))
                && String.valueOf(tickKind).equals(String.valueOf(other.tickKind))
                && String.valueOf(result).equals(String.valueOf(other.result));
        }

        String json(BlockState from) {
            StringBuilder b = new StringBuilder();
            b.append("{\"from\": ").append(quote(BlockStateParser.serialize(from)));
            if (refusal != null) {
                b.append(", \"refused\": ").append(quote(refusal));
                return b.append("}").toString();
            }
            b.append(", \"result\": ").append(quote(result == null ? "null"
                : result.getClass().getSimpleName()));
            b.append(", \"consumes\": ").append(result != null && result.consumesAction());
            b.append(", \"schedulesTick\": ").append(scheduled);
            if (scheduled) {
                b.append(", \"tickDelay\": ").append(tickDelay);
                b.append(", \"tickPriority\": ").append(quote(String.valueOf(tickPriority)));
                b.append(", \"tickAt\": [").append(tickAt == null ? "" : tickAt.getX()
                    + ", " + tickAt.getY() + ", " + tickAt.getZ()).append("]");
                b.append(", \"tickKind\": ").append(quote(String.valueOf(tickKind)));
            }
            b.append(", \"sound\": ").append(sounded);
            if (sounded) {
                // Kept beside the boolean rather than replacing it: every
                // existing consumer reads `sound`, and a table that renamed
                // the column would make this a flag day for all of them.
                b.append(", \"soundEvent\": ").append(soundEvent == null
                    ? "null" : quote(soundEvent));
                b.append(", \"soundSource\": ").append(soundSource == null
                    ? "null" : quote(soundSource));
                b.append(", \"soundVolume\": ").append(soundVolume);
                b.append(", \"soundCount\": ").append(soundCount);
                b.append(", \"soundPitchDrawn\": ").append(soundPitchDrawn);
                b.append(", \"soundExcludesActor\": ").append(soundExcludesActor);
                // **Emitted only when it is vanilla's.** A drawn pitch is
                // this harness's fixed seed talking, and writing it out as a
                // golden constant would publish an artefact of
                // `RandomSource.create(0L)`. The consumer gets the event and
                // the volume and is told, in the column above, that the pitch
                // is not in the table.
                if (!soundPitchDrawn) {
                    b.append(", \"soundPitch\": ").append(soundPitch);
                }
            }
            if (screens != null) {
                b.append(", \"opens\": ").append(quote(screens));
            }
            if (beWritten) {
                b.append(", \"blockEntityWritten\": true");
            }
            if (!reads.isEmpty()) {
                b.append(", \"reads\": [").append(quotedList(reads)).append("]");
            }
            if (!gated.isEmpty()) {
                b.append(", \"gated\": [").append(quotedList(gated)).append("]");
            }
            b.append(", \"writes\": [");
            int n = 0;
            for (Map.Entry<BlockPos, Write> e : writes.entrySet()) {
                BlockPos d = e.getKey();
                if (n++ > 0) {
                    b.append(", ");
                }
                b.append("{\"at\": [").append(d.getX()).append(", ").append(d.getY())
                    .append(", ").append(d.getZ()).append("], \"state\": ")
                    .append(quote(BlockStateParser.serialize(e.getValue().state())))
                    .append(", \"flags\": ").append(e.getValue().flags()).append("}");
            }
            return b.append("]}").toString();
        }
    }

    private static Probe probe(
        TableLevel level, HarnessPlayer player, Stance stance,
        BlockState state, Direction face, long seed
    ) {
        Probe p = new Probe();
        stance.apply(player);
        level.reset();
        level.seedRandom(seed);
        level.put(CLICKED, state);
        level.recording = p.writes;
        BlockHitResult hit = new BlockHitResult(
            Vec3.atCenterOf(CLICKED).relative(face, 0.5), face, CLICKED, false);
        // Scheduled ticks are counted by the inherited `TickSink`, not by
        // the probe, so they are read as a delta around the call. This was
        // wired the other way first and every `schedulesTick` came out false
        // -- including the button's, whose whole behaviour is that it
        // schedules its own un-press.
        // The two sinks are read **separately**, not as one sum. Summed, the
        // "which one moved" test below cannot be written: comparing one
        // sink's count against the pair's total is true for reasons that have
        // nothing to do with that sink.
        int blockBefore = level.blockTicks.scheduled;
        int fluidBefore = level.fluidTicks.scheduled;
        // Armed here, not in `Stance.apply`: `apply` fills the inventory for
        // the `full_inventory` stance, and a recorder armed any earlier would
        // record this dumper's own setup. Same reason `UseItemOnGolden` arms
        // `recordGrants` at this point.
        player.clearScreens();
        vanillaPaused = null;
        try {
            p.result = state.useWithoutItem(level, player, hit);
        } catch (Throwable t) {
            // The message names the method when it came from `no(...)`, and
            // the class name otherwise. Either way the row says what stopped
            // it rather than claiming the block does nothing.
            String m = t.getMessage();
            p.refusal = t.getClass().getSimpleName()
                + (m == null ? "" : ": " + m.substring(0, Math.min(m.length(), 90)));
        } finally {
            level.recording = null;
        }
        // **Before anything else is read off the level.** A rule that raised
        // vanilla's own alarm took a branch it would not take on a server, so
        // whatever it did afterwards is a reading of this harness — including
        // the result, which for a sign is a perfectly plausible `CONSUME`.
        if (p.refusal == null && vanillaPaused != null) {
            p.refusal = "VanillaPaused: " + vanillaPaused.substring(
                0, Math.min(vanillaPaused.length(), 90));
        }
        p.sounded = level.sounded;
        p.soundEvent = level.soundEvent;
        p.soundSource = level.soundSource;
        p.soundVolume = level.soundVolume;
        p.soundPitch = level.soundPitch;
        p.soundCount = level.soundCount;
        p.soundPitchDrawn = level.soundPitchDrawn;
        p.soundExcludesActor = level.soundExcludesActor;
        p.usedRandom = level.usedRandom;
        p.beWritten = level.blockEntityWritten;
        if (!player.screens.isEmpty()) {
            p.screens = String.join("; ", player.screens);
        }
        // **Only on a row that was actually recorded.** A refusal already says
        // "not measured" and is the stronger statement; hanging a `reads` list
        // off it would invite a reader to treat the two as alternatives when
        // one supersedes the other.
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
            // A use rule can schedule a fluid tick as easily as a block one,
            // and reading only `blockTicks` would print a delay of -1 beside
            // a `schedulesTick: true`. Two columns that can disagree is the
            // shape of fault this pair of reads exists to prevent.
            UseGolden.TickSink<?> sink = blockMoved ? level.blockTicks : level.fluidTicks;
            p.tickKind = blockMoved ? "block" : "fluid";
            p.tickDelay = sink.lastTriggerTick;
            p.tickPriority = sink.lastPriority;
            p.tickAt = sink.lastPos == null ? null : sink.lastPos.subtract(CLICKED);
        }
        return p;
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

    private static <T> Registry<T> rebuild(HolderLookup.RegistryLookup<T> lookup) {
        MappedRegistry<T> out = new MappedRegistry<>(
            (ResourceKey<? extends Registry<T>>) lookup.key(), Lifecycle.stable());
        lookup.listElements()
            .forEach(h -> out.register(h.key(), h.value(), RegistrationInfo.BUILT_IN));
        return out.freeze();
    }

    static String quotedList(java.util.Collection<String> names) {
        List<String> q = new ArrayList<>();
        for (String n : names) {
            q.add(quote(n));
        }
        return String.join(", ", q);
    }

    static String quote(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '"' || c == '\\') {
                b.append('\\').append(c);
            } else if (c < 0x20) {
                b.append(String.format("\\u%04x", (int) c));
            } else {
                b.append(c);
            }
        }
        return b.append('"').toString();
    }

    /// One `setBlock`: the state, and **the flags it was written with**.
    ///
    /// The flags are not decoration and were not carried at first. Vanilla's
    /// `Level.setBlock` gates `updateNeighbourShapes` on `UPDATE_KNOWN_SHAPE`
    /// (16) being *clear* and `updateNeighborsAt` on `UPDATE_NEIGHBORS` (1)
    /// being set, so the same write with different flags is a different
    /// event: one cascades and one does not. `DoorBlock.useWithoutItem`
    /// writes with **10** — `UPDATE_CLIENTS | UPDATE_IMMEDIATE`, neither
    /// `UPDATE_NEIGHBORS` nor `UPDATE_KNOWN_SHAPE` — while
    /// `DoorBlock.setPlacedBy` goes through `setBlockAndUpdate`, which is 3.
    /// A consumer that hard-codes one number is right about one of them.
    record Write(BlockState state, int flags) {
    }

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

        /// The world every probe is measured in: air everywhere. The
        /// subject state is put at `CLICKED` by `probe`, and nothing else
        /// exists — a block that needs a neighbour will read air, and
        /// [`readUnheldPosition`] puts that on the row, so "wrote nothing" is
        /// never mistaken for "does nothing".
        void reset() {
            blocks.clear();
            blockEntities.clear();
            sounded = false;
            soundEvent = null;
            soundSource = null;
            soundVolume = 0f;
            soundPitch = 0f;
            soundCount = 0;
            soundPitchDrawn = false;
            soundExcludesActor = false;
            pretendWater = false;
            usedRandom = false;
            blockEntityAsked = false;
            blockEntityWritten = false;
            readUnheldPosition = false;
            askedBlockEntityElsewhere = false;
            sweptEntities = false;
            leveledEvent = -1;
        }

        /// Makes [`isWaterAt`] answer true everywhere, for one probe.
        ///
        /// **Not a convenience — a blind detector was found here.**
        /// `PlacedGolden` probes each rule twice, once in air and once in
        /// water, and flags any whose answer moves. Filling the map with
        /// `Blocks.WATER` did not move a single one, and the reason is that
        /// the only path that asks is `LevelReader.isWaterAt`, which is
        /// `getFluidState(pos).is(FluidTags.WATER)` — a **tag** lookup, and
        /// `Bootstrap.bootStrap()` binds no tags. So it returned false in
        /// water exactly as it did in air, the sweep reported zero
        /// world-sensitive blocks, and that number was a fact about the JVM.
        ///
        /// Overriding the method steps around the tag entirely. The oracle
        /// in `ci/placed-golden.sh` then requires `small_dripleaf` to come
        /// back sensitive, so a detector that goes blind again fails the
        /// extraction instead of reporting a clean zero.
        boolean pretendWater;

        @Override
        public boolean isWaterAt(BlockPos pos) {
            return pretendWater || super.isWaterAt(pos);
        }

        /// Whether the rule reached for the level's RNG since the last
        /// [`reset`]. Reported, **not** used to drop a block's cells — see
        /// below for why that distinction cost a round trip.
        boolean usedRandom;

        /// The RNG every rule draws from, replaced per probe with a **fixed**
        /// seed. `Level.random` is `protected final` and cannot be swapped,
        /// but `getRandom()` is not final and everything goes through it.
        ///
        /// # Why this exists
        ///
        /// Randomness is inferred by probing a state several times and asking
        /// whether the *carried* answer moved. Left to a naturally advancing
        /// RNG that is a coin flip when a rule has few outcomes, and
        /// `DragonEggBlock` is exactly that case: this harness's world is air
        /// everywhere, `teleport` needs a target whose support is not air,
        /// and the only such square in the world is the one directly above
        /// the egg. So its search either finds that single square or exhausts
        /// its 1,000 attempts, and two probes agree whenever both succeed or
        /// both fail. Measured over six dumper runs on 2026-08-03: **twice
        /// out of six the egg came back "not random"**, which is a table that
        /// churns and a two-run `cmp` that fails with nothing changed.
        ///
        /// Fixed seeds make the classification a *reproducible* fact. Every
        /// dumper run asks exactly the same questions and gets exactly the
        /// same answers, so the file cannot churn; whether the answer is
        /// right is then checked once, by the oracle in `ci/use-golden.sh`,
        /// which requires the egg to come back random.
        ///
        /// # What was tried and is wrong
        ///
        /// Flagging any rule that *touched* the RNG. It is exact and cannot
        /// flake, and it is far too coarse: `DoorBlock.playSound` draws a
        /// **sound pitch**, which this table does not carry, so doors,
        /// trapdoors, fence gates and the note block all came back "random"
        /// and lost every cell — including the door row that
        /// `door-opens-when-right-clicked` is waiting on. Touching the RNG is
        /// not the same as the answer depending on it.
        net.minecraft.util.RandomSource harnessRandom =
            net.minecraft.util.RandomSource.create(0L);

        void seedRandom(long seed) {
            harnessRandom = net.minecraft.util.RandomSource.create(seed);
        }

        @Override
        public net.minecraft.util.RandomSource getRandom() {
            usedRandom = true;
            return harnessRandom;
        }

        /// Set for the duration of one probe; every write lands in it, keyed
        /// by offset from [`CLICKED`].
        ///
        /// A bare map rather than a `Probe`, so that a second dumper in this
        /// package can borrow the level without borrowing this one's row
        /// shape. `PlacedGolden` does.
        Map<BlockPos, Write> recording;

        /// Whether anything played a sound since the last [`reset`]. On the
        /// level rather than on the sink, because the sink is a map of
        /// writes and a sound is not a write.
        boolean sounded;

        /// **Which** sound, which is the whole point of the four fields
        /// below and the reason `sounded` alone was not enough.
        ///
        /// `sounded` is a boolean, so the table it feeds says *that* 184
        /// blocks sound and never *which* sound. A consumer cannot emit a
        /// packet from a boolean, so `world_send::use_sound` answers only for
        /// the families whose classes have been transcribed by hand and 180
        /// blocks stay silent. The arguments were always here — this override
        /// receives the event, the category, the volume and the pitch and
        /// dropped all four on the floor.
        String soundEvent;
        String soundSource;
        float soundVolume;
        float soundPitch;

        /// How many sounds this rule played, because "the last one" is a
        /// silent lie when a rule plays two.
        ///
        /// A consumer that reads [`soundEvent`] off a row with `soundCount`
        /// of 2 is reading whichever call happened to be last. Recorded so
        /// that case is visible in the table rather than discovered by a
        /// client hearing the wrong one.
        int soundCount;

        /// Whether the clicking player is the one who does **not** hear it.
        ///
        /// `Level.playSound`'s first argument is the excluded entity, not the
        /// emitter: `ButtonBlock.press` passes the player and the presser
        /// hears nothing, `LeverBlock` passes `aconst_null` and the flipper
        /// hears it. That distinction is currently hand-transcribed per class
        /// in `world_send::use_sound`, and it is the one part of a sound that
        /// changes *who gets a packet* rather than what is in it — so a table
        /// without it cannot drive the emitter even knowing the event id.
        boolean soundExcludesActor;

        /// Whether the RNG had been touched when the sound was played — so
        /// whether [`soundPitch`] is vanilla's value or this harness's.
        ///
        /// `DoorBlock.playSound` is `level.getRandom().nextFloat() * 0.1F +
        /// 0.9F` (offsets 30-46), and [`getRandom`] is the method this level
        /// already overrides. So a fixed seed makes that pitch *reproducible*
        /// without making it *vanilla's*: writing 0.9427 into the table would
        /// publish an artefact of `RandomSource.create(0L)` as a golden
        /// constant, which is this project's own defect class — a number
        /// measuring its own input.
        ///
        /// **Measured across `SEEDS`, not inferred from `usedRandom`.** The
        /// probe itself only ever sees one seed, so it cannot answer this;
        /// the state loop probes all eight and overwrites this field with
        /// whether the pitch actually moved. What is left here is the
        /// per-probe default of `false`, which is why nothing reads it
        /// between `recordSound` and that overwrite.
        boolean soundPitchDrawn;

        void put(BlockPos pos, BlockState state) {
            BlockPos at = pos.immutable();
            blocks.put(at, state);
            // A block entity this world already built for `at` survives the
            // write **only if vanilla's own rule says it does**.
            // `LevelChunk.setBlockState` keeps the existing block entity when
            // `isValidBlockState` accepts the new state and drops it
            // otherwise, and a use rule that writes its own position and then
            // re-reads the block entity — the jukebox does exactly that — sees
            // whichever answer that rule gives. Rebuilding unconditionally
            // would hand it a fresh, empty one and lose the record it had just
            // written; keeping it unconditionally would hand a candle cake the
            // cake's block entity.
            net.minecraft.world.level.block.entity.BlockEntity held = blockEntities.get(at);
            if (held == null) {
                return;
            }
            if (held.isValidBlockState(state)) {
                held.setBlockState(state);
            } else {
                blockEntities.remove(at);
            }
        }

        /// Whether the rule read a position this world holds nothing at.
        ///
        /// **The answer is still air, and that is deliberate.** Making this
        /// throw was measured first: over `use_without_item.json` it moves
        /// 1,416 cells across four classes — `NoteBlock`, `BedBlock`,
        /// `RedStoneWireBlock` and `ComparatorBlock` — from a recorded row to
        /// a refusal, and it reveals **no block that was not already in the
        /// table**. That is a table 1,416 cells smaller for no coverage at
        /// all, so the air world stays and the *row* carries the fact instead:
        /// see `Probe.reads`. `set_placed_by.json` is the opposite shape — the
        /// same flag is the only thing that distinguishes the repeater, the
        /// comparator and the mossy carpet from a block with no rule — and it
        /// is the same one column that answers both.
        boolean readUnheldPosition;

        @Override
        public BlockState getBlockState(BlockPos pos) {
            BlockState here = blocks.get(pos);
            if (here == null) {
                readUnheldPosition = true;
                return Blocks.AIR.defaultBlockState();
            }
            return here;
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return getBlockState(pos).getFluidState();
        }

        /// The block entities this world holds, built from the states it was
        /// given and **not** invented.
        ///
        /// Lazily, on the first ask. Eagerly would be simpler and is wrong:
        /// `put` is called from outside the probe's `try`, so a constructor
        /// that threw would take the dumper down instead of becoming a row
        /// that says which block could not be built.
        private final Map<BlockPos, net.minecraft.world.level.block.entity.BlockEntity>
            blockEntities = new HashMap<>();

        /// Set the moment a rule asks for a block entity.
        ///
        /// Kept, and it no longer means what it did. It used to mean "this
        /// answer is a reading of a `null` this harness invented", because
        /// this level held none; `UseItemOnGolden` reads it that way still and
        /// turns the ask itself into a refusal. Here the ask is now *answered*,
        /// so the flag means only that a block entity was consulted — which is
        /// worth a column and is not worth a refusal.
        boolean blockEntityAsked;

        /// Whether a rule asked for the block entity of a position this world
        /// holds nothing at.
        ///
        /// Separate from [`blockEntityAsked`] because the two are different
        /// facts. Asking about the clicked block is answered from the state
        /// that was put there. Asking about a *neighbour* is answered `null`
        /// because this world is empty there, and in a real world that
        /// neighbour may hold a chest — so the answer is a reading of this
        /// harness's world and the row has to say so.
        boolean askedBlockEntityElsewhere;

        /// The block entity for `pos`, built from the state this world holds
        /// there.
        ///
        /// # Why this is a supply and not a refusal
        ///
        /// `UseItemOnGolden` refuses on the ask, and refusing was the right
        /// call for a dumper that had nothing to answer with. It costs the
        /// whole rule: 2,593 refusals over 2,576 states, one cause and no
        /// others. Here the answer is available for nothing — `EntityBlock`
        /// declares `newBlockEntity(pos, state)` and vanilla's own
        /// `LevelChunk` calls exactly that — so the harness builds the real
        /// one and the rule runs to the end.
        ///
        /// # The four cases, and why only one of them throws
        ///
        ///   * **held, and the block declares a block entity** — build it,
        ///     attach this level, cache it, answer it. A real
        ///     `SignBlockEntity`, not a stand-in.
        ///   * **held, and the block declares none** — `null`, which is
        ///     vanilla's own answer for such a position and a fact about the
        ///     block rather than about this harness.
        ///   * **not held** — `null`, flagged. This world is air there; a real
        ///     one need not be.
        ///   * **held, declares one, and `newBlockEntity` answers `null`** —
        ///     **throw**. The block says it has a block entity and this harness
        ///     could not produce it, which is precisely the case where a
        ///     plausible empty answer would put a false `PASS` in the table.
        @Override
        public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) {
            blockEntityAsked = true;
            BlockPos at = pos.immutable();
            net.minecraft.world.level.block.entity.BlockEntity held = blockEntities.get(at);
            if (held != null) {
                return held;
            }
            BlockState state = blocks.get(at);
            if (state == null) {
                askedBlockEntityElsewhere = true;
                return null;
            }
            if (!(state.getBlock() instanceof net.minecraft.world.level.block.EntityBlock eb)) {
                return null;
            }
            net.minecraft.world.level.block.entity.BlockEntity made = eb.newBlockEntity(at, state);
            if (made == null) {
                // `MovingPistonBlock.newBlockEntity` is `aconst_null; areturn`
                // — the only one in the registry — because a moving piston's
                // `PistonMovingBlockEntity` is installed by
                // `MovingPistonBlock.newMovingBlockEntity` at the moment the
                // piston fires, and never by the standard path. So a real
                // moving piston has one and this harness has no way to build
                // it, which is a refusal and not a null.
                return no("a block entity for "
                    + BuiltInRegistries.BLOCK.getKey(state.getBlock())
                    + ": the block declares EntityBlock and its newBlockEntity "
                    + "answers null, so its block entity is installed by some other path");
            }
            made.setLevel(this);
            blockEntities.put(at, made);
            return made;
        }

        /// A block entity's own data changed.
        ///
        /// **Accepted and recorded, not refused**, and it is the fourth of this
        /// level's deliberate deviations from "throw for everything". Vanilla's
        /// `Level.blockEntityChanged` marks the chunk unsaved, which needs a
        /// `ChunkSource` this harness does not have — so the inherited method
        /// would refuse every rule that edits a block entity, which is most of
        /// the rules the block entity was supplied for. What it means is real
        /// and this table's `writes` column cannot express it (no block state
        /// moved), so it becomes its own column instead of a refusal.
        @Override
        public void blockEntityChanged(BlockPos pos) {
            blockEntityWritten = true;
        }

        /// Whether a rule changed a block entity's own data. See
        /// [`blockEntityChanged`].
        boolean blockEntityWritten;

        @Override
        public boolean isLoaded(BlockPos pos) {
            return true;
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            put(pos, state);
            if (recording != null) {
                recording.put(pos.subtract(CLICKED), new Write(state, flags));
            }
            return true;
        }

        private static <T> T no(String what) {
            throw new UnsupportedOperationException(
                "UseGolden's level was asked for " + what
                    + " — a use rule reads something this harness does not model");
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
            // **A real value, not a stub.** `PlacementGolden`'s level throws
            // here because nothing on the placement path reads it; the use
            // path does — `ServerPlayerGameMode.useItemOn`'s very first gate
            // is `state.getBlock().isEnabled(level.enabledFeatures())`, and
            // `Level.playSound` reaches it too, which is how every single
            // lever state came back refused on the first green run.
            //
            // `DEFAULT_FLAGS` is what a vanilla server runs with, so this is
            // the answer rather than a plausible empty one. The distinction
            // matters: the harness's discipline is that a method it did not
            // foresee must throw, not that every method must throw.
            return net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public net.minecraft.world.level.block.entity.FuelValues fuelValues() {
            return no("fuelValues");
        }

        @Override
        public Holder<net.minecraft.world.level.biome.Biome> getUncachedNoiseBiome(int x, int y, int z) {
            return no("getUncachedNoiseBiome");
        }

        /// Whether a rule swept the entity index. See [`getEntities`].
        boolean sweptEntities;

        /// No entities, which is the truth about this world — and the row now
        /// says which rules that truth decided.
        ///
        /// `DirtPathBlock.getStateForPlacement` calls `Block.pushEntitiesUp`,
        /// which sweeps the box above the placement for anything standing
        /// in it. There is nothing standing there — the harness places into
        /// open air with the player nowhere near — so an empty getter is the
        /// correct answer and not a stub.
        ///
        /// **`FenceBlock` is the case that says the argument is not enough.**
        /// Its `useWithoutItem` is `LeadItem.bindPlayerMobs(player, level,
        /// pos)`, which sweeps this index for leashed mobs and answers `PASS`
        /// when it finds none. Every fence in the game — 416 cells over 13
        /// blocks — was recorded as declining a right-click, no refusal
        /// anywhere, and vanilla ties a lead to a fence whenever a leashed mob
        /// is in range. The getter is still empty and still honest; what was
        /// missing is that the *row* never said the answer came out of an
        /// empty index. [`sweptEntities`] is that, and it is the only thing
        /// separating those 416 cells from a block with no rule.
        @Override
        protected net.minecraft.world.level.entity.LevelEntityGetter<Entity> getEntities() {
            sweptEntities = true;
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
            // A sculk signal. It reaches no client as a packet, so there is
            // nothing here for this table to carry.
        }

        /// The last `levelEvent` type raised, or -1.
        ///
        /// **Accepted and recorded rather than refused**, which is the third
        /// of this level's deliberate deviations from "throw for everything"
        /// and is there for the same reason as the other two: it has no
        /// `setBlock` in it, and refusing it refuses a rule this is trying to
        /// capture. `ComposterBlock.useItemOn` raises
        /// `LevelEvent.COMPOSTER_FILL` *after* it has written the new
        /// composter level, so a refusal here discards the write and records
        /// the composter — 115 compostable items over 7 levels, 805 cells —
        /// as measured-and-unknown when it had in fact already been measured.
        ///
        /// Recorded rather than dropped, unlike the sound: it is one integer,
        /// it is a packet, and `ComposterBlock` raises two different ones
        /// (fill and the ready-to-empty flash) from the same rule.
        int leveledEvent = -1;

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {
            leveledEvent = type;
        }

        @Override
        public void playSeededSound(
            Entity source, Entity target, Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch, long seed
        ) {
            recordSound(source, sound, category, volume, pitch);
        }

        @Override
        public void playSeededSound(
            Entity source, double x, double y, double z,
            Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch, long seed
        ) {
            recordSound(source, sound, category, volume, pitch);
        }

        /// The one place a played sound is written down, so the two
        /// `playSeededSound` overloads above cannot drift apart.
        ///
        /// **The id comes out of the registry key, never `toString`.**
        /// A `Holder` renders as `Reference{ResourceKey[...]}` and an unbound
        /// one renders differently again; `unwrapKey` either yields the
        /// namespaced id or it does not, and a sound this harness cannot name
        /// is written as `null` rather than as a string that looks like an id
        /// and is not.
        void recordSound(
            Entity excluded,
            Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch
        ) {
            sounded = true;
            soundExcludesActor = excluded != null;
            soundCount++;
            // `ResourceKey.identifier()`, not `location()`, and the type it
            // returns is `Identifier`, not `ResourceLocation` — both were
            // renamed in 26.2. Writing this from memory of an older version
            // is a compile error here, which is the lucky direction; the same
            // memory applied to a *field* name would not be.
            soundEvent = sound == null ? null : sound.unwrapKey()
                .<String>map(k -> k.identifier().toString()).orElse(null);
            soundSource = category == null ? null : category.getName();
            soundVolume = volume;
            soundPitch = pitch;
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

    /// A real `Player`, for the same reason `PlacementGolden` uses one: a
    /// stub that answers plausibly is exactly the class of instrument fault
    /// this project keeps finding, and vanilla's own code runs against a
    /// real one. Creative, because that is the mode this project's server
    /// puts a joining client in and the mode the gameplay corpus runs in.
    static final class HarnessPlayer extends Player {

        /// Set by [`Stance.apply`]. `Player.canUseGameMasterBlocks` is
        /// `abilities.instabuild && permissions().hasPermission(GAMEMASTER)`,
        /// and the second half is not reachable through any setter — a
        /// harness player has no permission set at all — so the whole
        /// `CommandBlock`/`StructureBlock`/`LightBlock` branch would have been
        /// unsweepable and would have looked constant.
        boolean gameMaster;

        /// What the rule under probe handed this player, and whether it read
        /// the inventory at all.
        ///
        /// **`setBlock` is not the whole of what a use rule does.**
        /// `BeehiveBlock` turns a glass bottle into a honey bottle with
        /// `Player.setItemInHand` and pushes the overflow out through
        /// `Player.drop`; `FlowerPotBlock.useWithoutItem` hands the flower
        /// back with `Player.addItem`. None of that is a block change and
        /// `UseGolden`'s `recording` map cannot see any of it, so a table
        /// built on writes alone says "the pot emptied" and not "and you got
        /// the flower".
        ///
        /// Recorded at the three public entry points rather than by counting
        /// occupied slots before and after: `UseItemOnGolden` probes tens of
        /// millions of (state, item) pairs and a 41-slot scan twice per probe
        /// is that dumper's whole run time. `inventoryRead` is the check on
        /// that shortcut — it is set by the accessor itself, so a rule that
        /// reaches the inventory by some fourth route still leaves a mark,
        /// and a cell that reads the inventory and grants nothing is visible
        /// as such rather than looking inert.
        final List<String> granted = new ArrayList<>();
        boolean recordGrants;
        boolean inventoryRead;

        private void grant(String how, ItemStack stack) {
            if (recordGrants && stack != null && !stack.isEmpty()) {
                granted.add(how + " " + stack.getCount() + " "
                    + BuiltInRegistries.ITEM.getKey(stack.getItem()));
            }
        }

        @Override
        public net.minecraft.world.entity.player.Inventory getInventory() {
            inventoryRead = true;
            return super.getInventory();
        }

        @Override
        public boolean addItem(ItemStack stack) {
            grant("addItem", stack);
            return super.addItem(stack);
        }

        @Override
        public net.minecraft.world.entity.item.ItemEntity drop(ItemStack stack, boolean dropAll) {
            grant("drop", stack);
            return super.drop(stack, dropAll);
        }

        @Override
        public void setItemInHand(net.minecraft.world.InteractionHand hand, ItemStack stack) {
            grant("setItemInHand", stack);
            super.setItemInHand(hand, stack);
        }

        HarnessPlayer(Level level) {
            super(level, new GameProfile(java.util.UUID.nameUUIDFromBytes(
                "UseGolden".getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                "UseGolden"));
        }

        /// Every screen this player was asked to open since [`clearScreens`],
        /// in the order they were asked for.
        ///
        /// # A no-op that is not a no-op
        ///
        /// `Player.openMenu`, `openTextEdit`, `openCommandBlock`,
        /// `openStructureBlock`, `openJigsawBlock`, `openTestBlock`,
        /// `openTestInstanceBlock`, `openDialog` and `openItemGui` are all
        /// **empty bodies on `Player`**; `ServerPlayer` overrides every one of
        /// them and sends a packet. So a harness built on `Player` silently
        /// drops the entire observable outcome of a right-click on a chest, a
        /// furnace, a sign, a lectern or a command block, and the row that
        /// comes back says `consumes: true, writes: []` — which reads as "the
        /// click was eaten and nothing happened".
        ///
        /// That is the same fault as the null block entity wearing different
        /// clothes: an inherited empty branch standing in for a thing the
        /// harness does not model, recorded as a fact about the block.
        /// `ci/probe-reads.py` names it exactly — "`Player.openMenu`'s
        /// `OptionalInt` is dropped on the floor by every caller in the jar" —
        /// which is why the *return* could never have carried this and the
        /// call has to.
        ///
        /// **Recorded rather than refused**, on the same argument that keeps
        /// the sound and the level event: a screen has no `setBlock` in it, and
        /// refusing it would refuse every container in the game over something
        /// this table can simply carry as a column. `crate::menus` is the
        /// consumer that has been waiting for it.
        final List<String> screens = new ArrayList<>();

        void clearScreens() {
            screens.clear();
        }

        private void screen(String what) {
            screens.add(what);
        }

        @Override
        public java.util.OptionalInt openMenu(net.minecraft.world.MenuProvider provider) {
            screen("menu");
            return java.util.OptionalInt.empty();
        }

        @Override
        public void openTextEdit(
            net.minecraft.world.level.block.entity.SignBlockEntity sign, boolean front
        ) {
            screen("sign_editor:" + (front ? "front" : "back"));
        }

        @Override
        public void openCommandBlock(
            net.minecraft.world.level.block.entity.CommandBlockEntity be
        ) {
            screen("command_block");
        }

        @Override
        public void openStructureBlock(
            net.minecraft.world.level.block.entity.StructureBlockEntity be
        ) {
            screen("structure_block");
        }

        @Override
        public void openJigsawBlock(
            net.minecraft.world.level.block.entity.JigsawBlockEntity be
        ) {
            screen("jigsaw_block");
        }

        @Override
        public void openTestBlock(
            net.minecraft.world.level.block.entity.TestBlockEntity be
        ) {
            screen("test_block");
        }

        @Override
        public void openTestInstanceBlock(
            net.minecraft.world.level.block.entity.TestInstanceBlockEntity be
        ) {
            screen("test_instance_block");
        }

        @Override
        public void openDialog(net.minecraft.core.Holder<net.minecraft.server.dialog.Dialog> d) {
            screen("dialog");
        }

        @Override
        public void openItemGui(ItemStack stack, net.minecraft.world.InteractionHand hand) {
            screen("item_gui");
        }

        @Override
        public GameType gameMode() {
            return GameType.CREATIVE;
        }

        @Override
        public PermissionSet permissions() {
            return gameMaster ? PermissionSet.ALL_PERMISSIONS : PermissionSet.NO_PERMISSIONS;
        }
    }


    /// Counts scheduled ticks and drops them — see `TableLevel.getBlockTicks`.
    ///
    /// It also **keeps the last one's delay and priority**, and that is not
    /// decoration. The first version of this counted only, so the use table
    /// said `schedulesTick: true` for every button and could not say *how
    /// long* — and the button's whole observable behaviour is that it
    /// un-presses a fixed number of ticks later. A column that says a tick
    /// exists but not when it fires cannot drive a tick queue.
    ///
    /// `triggerTick` is an absolute game time and this harness's
    /// `WritableLevelData.getGameTime()` is `0L`, so it **is** the delay. If
    /// that stub ever grows a real clock this has to subtract it.
    static final class TickSink<T> implements net.minecraft.world.ticks.LevelTickAccess<T> {
        int scheduled;
        long lastTriggerTick = -1L;
        String lastPriority;
        BlockPos lastPos;

        @Override
        public void schedule(net.minecraft.world.ticks.ScheduledTick<T> tick) {
            scheduled++;
            lastTriggerTick = tick.triggerTick();
            lastPriority = tick.priority().name();
            lastPos = tick.pos();
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

    private UseGolden() {
    }
}
