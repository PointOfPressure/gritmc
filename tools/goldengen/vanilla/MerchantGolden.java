package net.minecraft.world.level.levelgen;

import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.nio.file.Path;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundMerchantOffersPacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.trading.ItemCost;
import net.minecraft.world.item.trading.MerchantOffer;
import net.minecraft.world.item.trading.MerchantOffers;

/**
 * `ClientboundMerchantOffersPacket` and the `MerchantOffer` list inside it, as
 * their own `STREAM_CODEC`s write them.
 *
 * <p>`MerchantOffer.writeToStream` is a hand-written lambda rather than a
 * composite, and its field list is not the record's: `costA`, then the
 * **result**, then the optional `costB`, then `outOfStock`, `uses`, `maxUses`,
 * `xp`, `specialPriceDiff`, `priceMultiplier`, `demand`. Six of those ten are
 * `writeInt` — fixed 32-bit, big-endian — where a varint is the obvious guess
 * and agrees on every value below 128.
 *
 * <p>Every stack here is component-free, so the `DataComponentPatch` and the
 * `DataComponentExactPredicate` both encode as their empty forms. That is a
 * deliberate limit and it is the reason this capture cannot stand in for the
 * item layer: a trade whose result carries an enchantment is not exercised
 * here and this file says so rather than implying coverage.
 *
 * <p>Run by {@code ci/merchant-golden.sh}. Writes to
 * {@code crates/rustmc-server/golden/merchant_offers.json}.
 */
public final class MerchantGolden {
    private MerchantGolden() {}

    public static void main(String[] args) throws Exception {
        // Written to a FILE rather than to stdout, unlike this project's other
        // goldens. `VanillaRegistries.createLookup()` installs a log4j
        // redirect that puts timestamped lines on stdout, and one of them
        // would land in the middle of the JSON and differ between runs — the
        // two-runs-must-agree check catches that, which is how this was
        // found, but a file is the fix rather than a filter.
        PrintStream out = new PrintStream(java.nio.file.Files.newOutputStream(Path.of(args[0])), true, "UTF-8");
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // `Bootstrap.bootStrap()` registers the items and leaves their DATA
        // COMPONENTS unbound, so `new ItemStack(Items.EMERALD)` dies with
        // "Components not bound yet" inside `Holder.Reference.components` —
        // a stack cannot be built at all, let alone encoded. The components
        // are baked from the registry lookup, the same step a real server
        // takes on load. Same class of trap as the unbound block tag in
        // PortalGolden: a bare bootstrap is not a loaded server.
        BuiltInRegistries.DATA_COMPONENT_INITIALIZERS
                .build(VanillaRegistries.createLookup())
                .forEach(net.minecraft.core.component.DataComponentInitializers.PendingComponents::apply);

        // Four offers, chosen so no field is constant across the capture:
        // one with a second cost and one without, one out of stock, a negative
        // specialPriceDiff, a non-zero demand and two different multipliers.
        MerchantOffers offers = new MerchantOffers();
        MerchantOffer wheat =
                new MerchantOffer(new ItemCost(Items.WHEAT, 20), new ItemStack(Items.EMERALD), 16, 2, 0.05f);
        MerchantOffer cooked = new MerchantOffer(
                new ItemCost(Items.COD, 6),
                Optional.of(new ItemCost(Items.EMERALD)),
                new ItemStack(Items.COOKED_COD, 6),
                16,
                5,
                0.05f);
        MerchantOffer used = new MerchantOffer(
                new ItemCost(Items.COAL, 15), new ItemStack(Items.EMERALD), 16, 1, 0.05f);
        for (int i = 0; i < 16; i++) {
            used.increaseUses();
        }
        MerchantOffer priced = new MerchantOffer(
                new ItemCost(Items.PAPER, 24), new ItemStack(Items.EMERALD), 16, 1, 0.2f);
        priced.setSpecialPriceDiff(-3);
        priced.addToSpecialPriceDiff(0);
        for (int i = 0; i < 5; i++) {
            priced.increaseUses();
        }
        priced.updateDemand();

        offers.add(wheat);
        offers.add(cooked);
        offers.add(used);
        offers.add(priced);

        out.println("{");
        out.println("  \"source\": \"ClientboundMerchantOffersPacket.STREAM_CODEC, 26.2\",");
        out.printf("  \"emeraldId\": %d,%n", BuiltInRegistries.ITEM.getId(Items.EMERALD));
        out.printf("  \"wheatId\": %d,%n", BuiltInRegistries.ITEM.getId(Items.WHEAT));
        out.printf("  \"codId\": %d,%n", BuiltInRegistries.ITEM.getId(Items.COD));
        out.printf("  \"cookedCodId\": %d,%n", BuiltInRegistries.ITEM.getId(Items.COOKED_COD));
        out.printf("  \"coalId\": %d,%n", BuiltInRegistries.ITEM.getId(Items.COAL));
        out.printf("  \"paperId\": %d,%n", BuiltInRegistries.ITEM.getId(Items.PAPER));
        out.println("  \"offers\": [");
        String[] names = {"wheat_emerald", "cod_and_emerald_cooked_cod", "coal_emerald_out_of_stock", "paper_emerald_priced"};
        MerchantOffer[] all = {wheat, cooked, used, priced};
        for (int i = 0; i < all.length; i++) {
            MerchantOffer o = all[i];
            out.printf(
                    "    {\"name\": \"%s\", \"outOfStock\": %b, \"uses\": %d, \"maxUses\": %d,"
                            + " \"xp\": %d, \"specialPriceDiff\": %d, \"priceMultiplier\": %s,"
                            + " \"demand\": %d, \"costACount\": %d, \"resultCount\": %d,"
                            + " \"hasCostB\": %b}%s%n",
                    names[i],
                    o.isOutOfStock(),
                    o.getUses(),
                    o.getMaxUses(),
                    o.getXp(),
                    o.getSpecialPriceDiff(),
                    Float.toString(o.getPriceMultiplier()),
                    o.getDemand(),
                    o.getItemCostA().count(),
                    o.getResult().getCount(),
                    o.getItemCostB().isPresent(),
                    i + 1 < all.length ? "," : "");
        }
        out.println("  ],");

        // The whole packet. `containerId` is deliberately above 127 so its
        // varint is two bytes, and `villagerXp` above 127 for the same reason:
        // a fixed-width read of either agrees on every small value.
        out.printf(
                "  \"packet\": \"%s\",%n",
                hex(encode(new ClientboundMerchantOffersPacket(200, offers, 3, 150, true, false))));
        out.printf("  \"containerId\": %d,%n", 200);
        out.printf("  \"villagerLevel\": %d,%n", 3);
        out.printf("  \"villagerXp\": %d,%n", 150);
        out.println("  \"showProgress\": true,");
        out.println("  \"canRestock\": false,");
        // And an empty offer list, which is the one that separates a varint
        // count from a fixed one at zero.
        out.printf(
                "  \"emptyPacket\": \"%s\"%n",
                hex(encode(new ClientboundMerchantOffersPacket(0, new MerchantOffers(), 1, 0, false, true))));
        out.println("}");
        out.close();
    }

    private static byte[] encode(ClientboundMerchantOffersPacket p) {
        RegistryFriendlyByteBuf b = new RegistryFriendlyByteBuf(
                Unpooled.buffer(),
                // NOT `RegistryAccess.EMPTY`: `ItemStack.STREAM_CODEC` writes
                // its item through `ByteBufCodecs.registry(Registries.ITEM)`,
                // which looks the registry up on the buffer and throws
                // "Missing registry: minecraft:item" on an empty access.
                // Nothing is constructed from this access here, so the caveat
                // in EntityMoveGolden — that the short spelling lacks the
                // datapack registries a Level needs — does not apply.
                RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY));
        ClientboundMerchantOffersPacket.STREAM_CODEC.encode(b, p);
        byte[] raw = new byte[b.readableBytes()];
        b.readBytes(raw);
        return raw;
    }

    private static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder(b.length * 2);
        for (byte x : b) {
            sb.append(String.format("%02x", x));
        }
        return sb.toString();
    }
}
