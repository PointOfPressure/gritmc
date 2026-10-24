// FireOddsGolden — `FireBlock`'s two flammability maps, off the pinned jar.
//
// `FireBlock` keeps two `Object2IntMap<Block>` fields, `igniteOdds` and
// `burnOdds`, both filled by `setFlammable(block, igniteOdds, burnOdds)` from
// `FireBlock.bootStrap()`. Every fire decision in the game reads one of them:
//
//   canBurn(state)          = getIgniteOdds(state) > 0
//   isValidFireLocation     = any of the six neighbours canBurn
//   getStateForPlacement    = the north/east/south/west/up connection walk
//   FireBlock.tick          = the six-face burn-out pass and the 3x3x6 spread
//
// **It is data, not a rule.** `bootStrap()` is 175 literal `setFlammable`
// calls plus two `ColorCollection.forEach` sweeps — the sixteen wools at
// (30, 60) through `lambda$bootStrap$0` and the sixteen carpets at (60, 20)
// through `lambda$bootStrap$1`. There is no naming rule connecting a block to
// its odds: `oak_planks` is (5, 20) and `oak_log` is (5, 5), `vine` is
// (15, 100) and `hay_block` is (60, 20). A hand-written table is a list that
// rots at the next version, and a table derived from block names is wrong on
// its first exception.
//
// **The count on this file's reader was 175 and 175 is the number of literal
// call sites, not the number of blocks.** `crates/rustmc-server/src/itemuse.rs`
// named "a 175-entry map built in `FireBlock.bootStrap()`" while declining to
// extract it; the two `ColorCollection.forEach` sweeps add thirty-two more.
// `ci/fire-odds-golden.sh` re-derives the expected count from the bytecode
// on every run rather than trusting either number.
//
// The maps are read by reflection because `getBurnOdds`/`getIgniteOdds` are
// private and take a `BlockState`, and because a `BlockState`-shaped read
// would fold in the `WATERLOGGED` short-circuit those two methods apply
// before the lookup. That short-circuit belongs in the port, not in the table:
// it is a rule about a state, and this file is about a block.
//
// Converted to JSON by `ci/fire-odds-golden.sh`.

import it.unimi.dsi.fastutil.objects.Object2IntMap;
import java.lang.reflect.Field;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.FireBlock;

public final class FireOddsGolden {

    @SuppressWarnings("unchecked")
    private static Object2IntMap<Block> mapOf(FireBlock fire, String name) throws Exception {
        Field f = FireBlock.class.getDeclaredField(name);
        f.setAccessible(true);
        return (Object2IntMap<Block>) f.get(fire);
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        // `Bootstrap.bootStrap()` calls `FireBlock.bootStrap()` at offset 41 of
        // its own body, so the maps are filled by the time this returns. It is
        // not called directly here: calling it twice would double-put every
        // key, which a map hides, and this program must not be able to hide it.
        Bootstrap.bootStrap();

        FireBlock fire = (FireBlock) Blocks.FIRE;
        Object2IntMap<Block> ignite = mapOf(fire, "igniteOdds");
        Object2IntMap<Block> burn = mapOf(fire, "burnOdds");

        int blocks = 0;
        int rows = 0;
        StringBuilder out = new StringBuilder();

        // Walked in registry order rather than map order: an `Object2IntMap`
        // iterates in its own hash order, which is not a property of vanilla
        // and would make the table's byte content depend on the JVM. The
        // registry is a stable, declared order, and the shell sorts anyway.
        for (Block block : BuiltInRegistries.BLOCK) {
            blocks++;
            boolean inIgnite = ignite.containsKey(block);
            boolean inBurn = burn.containsKey(block);
            if (!inIgnite && !inBurn) {
                continue;
            }
            Identifier id = BuiltInRegistries.BLOCK.getKey(block);
            if (id == null) {
                continue;
            }
            rows++;
            // Both columns on one row, and `PRESENT` per map, so a block that
            // reached only one of the two maps — which `setFlammable` cannot
            // produce and a future refactor could — is visible rather than
            // silently defaulted to 0.
            out.append("ROW\t")
                    .append(id)
                    .append('\t')
                    .append(inIgnite ? ignite.getInt(block) : -1)
                    .append('\t')
                    .append(inBurn ? burn.getInt(block) : -1)
                    .append('\t')
                    .append(inIgnite ? 1 : 0)
                    .append(inBurn ? 1 : 0)
                    .append('\n');
        }

        System.out.print(out);
        // `ignite.size()` and `burn.size()` are printed rather than derived from
        // the rows: a block in a map but not in the registry would otherwise be
        // invisible, and the two figures disagreeing is the finding.
        System.out.println("COUNT\t" + blocks + "\t" + rows + "\t" + ignite.size() + "\t" + burn.size());
    }
}
