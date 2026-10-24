// BlockItemGolden — which block each item places, from the pinned jar.
//
// **Block ids and item ids are two namespaces and most of the corpus's
// tooling has been comparing them with `==`.**
//
// `ci/corpus-surface.py` bounds what the gameplay corpus can *touch* by the
// set of items any check `hold`s, on the sound reasoning that the corpus has
// no verb that places a block it is not holding. It then asks whether a
// block in a golden table is covered by testing `block_id in held` — and that
// test is a string comparison between a block id and a set of item ids. It is
// right for `minecraft:chest`, whose item and block share a name, and it is
// wrong for every block whose item does not.
//
// Measured cost of the confusion on 2026-08-12, at base 84f789db: the corpus
// holds `minecraft:redstone` and eight checks place redstone dust with it, and
// `use_without_item.json`'s `minecraft:redstone_wire` row scored **uncovered**
// through all of them. §21.5's headline "16 of 184 blocks, 14 of 39 classes"
// was therefore one short in every unit it quotes.
//
// The mapping cannot come from Mojang's `generated/reports/` — there is no
// items report in the pinned jar's report set, and `registries.json`
// enumerates both registries without relating them. It also must not be
// hand-written: a hand-written map is a list that silently rots at the next
// version, which is the defect class this file exists to close, not to move.
//
// So it is read off `BlockItem.getBlock()` for every item in the registry.
// That is the same field vanilla itself dispatches placement through, so an
// item that stops placing a block, or starts placing a different one, moves
// this table on the next regeneration.
//
// Converted to JSON by `ci/block-items-golden.sh`.

import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.block.Block;

public final class BlockItemGolden {

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        int items = 0;
        int placing = 0;
        StringBuilder out = new StringBuilder();

        for (Item item : BuiltInRegistries.ITEM) {
            items++;
            if (!(item instanceof BlockItem)) {
                continue;
            }
            Block block = ((BlockItem) item).getBlock();
            Identifier itemId = BuiltInRegistries.ITEM.getKey(item);
            Identifier blockId = BuiltInRegistries.BLOCK.getKey(block);
            if (itemId == null || blockId == null) {
                continue;
            }
            placing++;
            out.append("ROW\t").append(itemId).append('\t').append(blockId).append('\n');
        }

        // Counts on their own line so the shell can assert the walk ran at all
        // rather than trusting an empty file. A dumper that bootstraps wrong
        // prints nothing and looks exactly like a registry with no block items.
        System.out.print(out);
        System.out.println("COUNT\t" + items + "\t" + placing);
    }
}
