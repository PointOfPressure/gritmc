//! The two trading packets, from the 26.2 jar.
//!
//! # `merchant_offers`, clientbound, protocol id 52
//!
//! `ClientboundMerchantOffersPacket.write` (offsets 5..53) and its
//! buffer constructor read the same six fields in the same order:
//!
//! ```text
//! writeContainerId(containerId)                       VarInt
//! MerchantOffers.STREAM_CODEC                         VarInt count, then each offer
//! writeVarInt(villagerLevel)
//! writeVarInt(villagerXp)
//! writeBoolean(showProgress)
//! writeBoolean(canRestock)
//! ```
//!
//! `writeContainerId` is `VarInt.write` — it is **not** a byte and has not
//! been one since container ids became varints. `MerchantOffers.STREAM_CODEC`
//! is `MerchantOffer.STREAM_CODEC.apply(ByteBufCodecs.collection(...))`, so a
//! plain VarInt length prefix and then the elements.
//!
//! One offer, `MerchantOffer.writeToStream` (offsets 0..102):
//!
//! ```text
//! ItemCost.STREAM_CODEC          getItemCostA()     item id VarInt, count VarInt, components
//! ItemStack.STREAM_CODEC         getResult()
//! ItemCost.OPTIONAL_STREAM_CODEC getItemCostB()     ByteBufCodecs.optional: bool then value
//! writeBoolean(isOutOfStock())
//! writeInt(getUses())
//! writeInt(getMaxUses())
//! writeInt(getXp())
//! writeInt(getSpecialPriceDiff())
//! writeFloat(getPriceMultiplier())
//! writeInt(getDemand())
//! ```
//!
//! **Five of those six integers are `writeInt`, not `writeVarInt`** — fixed
//! 32-bit big-endian, four bytes each, negative values included. Only the
//! three fields on the *packet* are varints. Getting that wrong desynchronises
//! the whole rest of the packet rather than producing a wrong number, and
//! `specialPriceDiff` and `demand` are routinely negative, which a varint
//! would encode as five bytes.
//!
//! **`getItemCostA()` is the BASE cost, not the modified one.** The client
//! recomputes the displayed price itself from `specialPriceDiff`, `demand`
//! and `priceMultiplier`, all three of which are on the wire for exactly that
//! reason. Sending `getCostA()` here would apply the modifier twice.
//!
//! `createFromStream` reads the same order and then reconstructs: it builds
//! the offer with `uses`, `maxUses`, `xp`, `priceMultiplier`, `demand`, calls
//! `setToOutOfStock()` if the boolean was set, and `setSpecialPriceDiff`
//! last. Note it does **not** trust the boolean and the uses count to agree —
//! the boolean wins.
//!
//! # `select_trade`, serverbound, protocol id 51
//!
//! One `VarInt`, the index the client selected. It lands in
//! `MerchantOffers.getRecipeFor`'s third argument, where an index above zero
//! and in range does not fall back to the scan.
//!
//! # Where this joins the menu, which is not this lane's
//!
//! Nothing sends this packet from here. `Merchant.openTradingScreen`
//! (offsets 0..74) is `Player.openMenu(new SimpleMenuProvider(MerchantMenu::new,
//! title))`, and **only if that returns a container id and the offer list is
//! non-empty** does it call `sendMerchantOffers(id, offers, level, xp,
//! showProgressBar(), canRestock())`. So the join is `ServerPlayer.openMenu`
//! with the `merchant_offers` follow-up, and a merchant with no offers opens
//! a screen and sends no packet at all. `MerchantMenu`, `MerchantContainer`
//! and `MerchantResultSlot` — the slots and the click that executes a trade —
//! are the container-menu lane's.
//!
//! # No captured bytes exist to check this against
//!
//! Checked rather than assumed: `merchant_offers` appears in **0 of the 107**
//! `oracle-work/gameplay/` recording directories on this box
//! (`grep -l merchant_offers ~/rustmc*/oracle-work/gameplay/*.json`). So the
//! byte-level assertions below are against the bytecode's field order and
//! widths and nothing else, and no figure claiming wire parity for this
//! packet is registered anywhere. When a scenario drives a villager, the
//! recording is the check this file is missing.

use std::io::{self, Write};

use super::offer::{ItemCost, MerchantOffer, MerchantOffers, Stack};

/// The item payloads this crate cannot write: an item id is a registry index
/// and a stack carries data components, neither of which lives here.
///
/// It is a trait rather than three closures so that an implementation cannot
/// silently disagree with itself about, say, whether an empty stack is a
/// single zero byte.
pub trait StackWire {
    /// `ItemCost.STREAM_CODEC` — item holder id, `VarInt` count, then
    /// `DataComponentExactPredicate.STREAM_CODEC`.
    fn write_item_cost(&mut self, out: &mut dyn Write, cost: &ItemCost) -> io::Result<()>;
    /// `ItemStack.STREAM_CODEC`.
    fn write_stack(&mut self, out: &mut dyn Write, stack: &Stack) -> io::Result<()>;
}

/// `VarInt.write`.
pub fn write_var_int(out: &mut dyn Write, v: i32) -> io::Result<()> {
    // Java's `>>>` on the int: unsigned, so -1 is five bytes and not an
    // infinite loop.
    let mut u = v as u32;
    loop {
        if u & !0x7f == 0 {
            return out.write_all(&[u as u8]);
        }
        out.write_all(&[((u & 0x7f) | 0x80) as u8])?;
        u >>= 7;
    }
}

/// `MerchantOffer.writeToStream`.
pub fn write_offer<W: StackWire>(
    out: &mut dyn Write,
    w: &mut W,
    o: &MerchantOffer,
) -> io::Result<()> {
    w.write_item_cost(out, o.base_cost_a())?;
    w.write_stack(out, o.result())?;
    // `ByteBufCodecs.optional`: a presence boolean, then the value.
    match o.item_cost_b() {
        Some(b) => {
            out.write_all(&[1])?;
            w.write_item_cost(out, b)?;
        }
        None => out.write_all(&[0])?,
    }
    out.write_all(&[u8::from(o.is_out_of_stock())])?;
    out.write_all(&o.uses().to_be_bytes())?;
    out.write_all(&o.max_uses().to_be_bytes())?;
    out.write_all(&o.xp().to_be_bytes())?;
    out.write_all(&o.special_price_diff().to_be_bytes())?;
    out.write_all(&o.price_multiplier().to_be_bytes())?;
    out.write_all(&o.demand().to_be_bytes())?;
    Ok(())
}

/// `ClientboundMerchantOffersPacket.write`. The body only — the packet id and
/// the frame length are the caller's.
#[allow(clippy::too_many_arguments)]
pub fn write_merchant_offers<W: StackWire>(
    out: &mut dyn Write,
    w: &mut W,
    container_id: i32,
    offers: &MerchantOffers,
    villager_level: i32,
    villager_xp: i32,
    show_progress: bool,
    can_restock: bool,
) -> io::Result<()> {
    write_var_int(out, container_id)?;
    write_var_int(out, i32::try_from(offers.len()).unwrap_or(i32::MAX))?;
    for o in &offers.0 {
        write_offer(out, w, o)?;
    }
    write_var_int(out, villager_level)?;
    write_var_int(out, villager_xp)?;
    out.write_all(&[u8::from(show_progress)])?;
    out.write_all(&[u8::from(can_restock)])?;
    Ok(())
}

/// `ServerboundSelectTradePacket` — one `VarInt`. Returns the index and how
/// many bytes it consumed.
///
/// The value is **not** validated here and must not be: vanilla passes it
/// straight to `MerchantOffers.getRecipeFor`, which range-checks it and
/// answers nothing rather than throwing. A server that rejected an
/// out-of-range index would disconnect a client that vanilla tolerates.
#[must_use]
pub fn read_select_trade(buf: &[u8]) -> Option<(i32, usize)> {
    let mut v: i32 = 0;
    for (i, b) in buf.iter().take(5).enumerate() {
        v |= i32::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A writer that makes the frame readable without an item registry: an
    /// item cost is `C <len> <name>` and a stack is `S <len> <name>`, so the
    /// bytes below are the packet's own structure and nothing else.
    struct Named;

    impl StackWire for Named {
        fn write_item_cost(&mut self, out: &mut dyn Write, c: &ItemCost) -> io::Result<()> {
            out.write_all(b"C")?;
            out.write_all(&[c.item.len() as u8])?;
            out.write_all(c.item.as_bytes())?;
            out.write_all(&[c.count as u8])
        }
        fn write_stack(&mut self, out: &mut dyn Write, s: &Stack) -> io::Result<()> {
            out.write_all(b"S")?;
            out.write_all(&[s.item.len() as u8])?;
            out.write_all(s.item.as_bytes())?;
            out.write_all(&[s.count as u8])
        }
    }

    fn offer() -> MerchantOffer {
        MerchantOffer::new(
            ItemCost::new("emerald", 5, 64),
            None,
            Stack::new("book", 1),
            16,
            2,
            0.05,
        )
    }

    #[test]
    fn var_int_matches_vanillas_encoding() {
        let mut b = Vec::new();
        write_var_int(&mut b, 0).unwrap();
        assert_eq!(b, [0]);
        b.clear();
        write_var_int(&mut b, 127).unwrap();
        assert_eq!(b, [0x7f]);
        b.clear();
        write_var_int(&mut b, 128).unwrap();
        assert_eq!(b, [0x80, 0x01]);
        b.clear();
        write_var_int(&mut b, -1).unwrap();
        assert_eq!(b, [0xff, 0xff, 0xff, 0xff, 0x0f]);
    }

    /// The five trailing integers are fixed-width big-endian, so a fresh
    /// offer's tail is exactly 4+4+4+4+4+4 = 24 bytes after the two booleans.
    /// A varint encoding would make this 6 for the same values, which is the
    /// discrepancy that desynchronises the packet.
    #[test]
    fn an_offers_tail_is_fixed_width_big_endian() {
        let mut b = Vec::new();
        write_offer(&mut b, &mut Named, &offer()).unwrap();
        let head = b"C\x07emerald\x05S\x04book\x01\x00\x00";
        assert_eq!(
            &b[..head.len()],
            head,
            "cost A, result, no cost B, in stock"
        );
        let tail = &b[head.len()..];
        assert_eq!(tail.len(), 24);
        assert_eq!(&tail[0..4], &0i32.to_be_bytes(), "uses");
        assert_eq!(&tail[4..8], &16i32.to_be_bytes(), "maxUses");
        assert_eq!(&tail[8..12], &2i32.to_be_bytes(), "xp");
        assert_eq!(&tail[12..16], &0i32.to_be_bytes(), "specialPriceDiff");
        assert_eq!(&tail[16..20], &0.05f32.to_be_bytes(), "priceMultiplier");
        assert_eq!(&tail[20..24], &0i32.to_be_bytes(), "demand");
    }

    /// `specialPriceDiff` and `demand` are routinely negative and must come
    /// through as two's complement rather than as a five-byte varint.
    #[test]
    fn negative_price_fields_survive_the_wire() {
        let mut o = offer();
        o.add_to_special_price_diff(-7);
        o.update_demand(); // -16 on a fresh offer with maxUses 16
        let mut b = Vec::new();
        write_offer(&mut b, &mut Named, &o).unwrap();
        let tail = &b[b.len() - 24..];
        assert_eq!(&tail[12..16], &(-7i32).to_be_bytes());
        assert_eq!(&tail[20..24], &(-16i32).to_be_bytes());
    }

    /// The BASE cost goes on the wire, not the modified one. The client
    /// reapplies `specialPriceDiff` itself, so sending `getCostA()` would
    /// double every discount and surcharge.
    #[test]
    fn the_wire_carries_the_base_cost_not_the_modified_one() {
        let mut o = offer();
        o.add_to_special_price_diff(3);
        assert_eq!(o.cost_a().count, 8, "the modified price really moved");
        let mut b = Vec::new();
        write_offer(&mut b, &mut Named, &o).unwrap();
        assert_eq!(&b[..10], b"C\x07emerald\x05", "still the base 5");
    }

    /// The optional cost B is a presence boolean and then the value.
    #[test]
    fn cost_b_is_an_optional_not_an_empty_stack() {
        let with = MerchantOffer::new(
            ItemCost::new("emerald", 1, 64),
            Some(ItemCost::new("book", 1, 64)),
            Stack::new("enchanted_book", 1),
            12,
            10,
            0.2,
        );
        let mut b = Vec::new();
        write_offer(&mut b, &mut Named, &with).unwrap();
        let head = b"C\x07emerald\x01S\x0eenchanted_book\x01\x01C\x04book\x01";
        assert_eq!(&b[..head.len()], head);

        let mut n = Vec::new();
        write_offer(&mut n, &mut Named, &offer()).unwrap();
        assert_eq!(
            n[b"C\x07emerald\x05S\x04book\x01".len()],
            0,
            "absent is one 0"
        );
    }

    /// The packet's own three integers are varints and its two flags are
    /// bytes, in that order after the offer list.
    #[test]
    fn the_packet_frames_the_list_with_a_varint_count() {
        let mut offers = MerchantOffers::new();
        offers.push(offer());
        offers.push(offer());
        let mut b = Vec::new();
        write_merchant_offers(&mut b, &mut Named, 3, &offers, 5, 250, true, false).unwrap();
        assert_eq!(b[0], 3, "container id, varint");
        assert_eq!(b[1], 2, "offer count, varint");
        let tail = &b[b.len() - 5..];
        assert_eq!(tail[0], 5, "villager level, varint");
        assert_eq!(&tail[1..3], &[0xfa, 0x01], "250 as a varint");
        assert_eq!(tail[3], 1, "showProgress");
        assert_eq!(tail[4], 0, "canRestock");
    }

    /// An empty list is a legal packet body — but vanilla never sends one:
    /// `openTradingScreen` skips `sendMerchantOffers` entirely when the offer
    /// list is empty, so the menu opens and no packet follows.
    #[test]
    fn an_empty_list_is_two_bytes_and_the_flags() {
        let mut b = Vec::new();
        write_merchant_offers(
            &mut b,
            &mut Named,
            1,
            &MerchantOffers::new(),
            1,
            0,
            false,
            true,
        )
        .unwrap();
        assert_eq!(b, vec![1, 0, 1, 0, 0, 1]);
    }

    #[test]
    fn select_trade_is_one_var_int_and_is_not_validated() {
        assert_eq!(read_select_trade(&[0]), Some((0, 1)));
        assert_eq!(read_select_trade(&[0x03]), Some((3, 1)));
        assert_eq!(read_select_trade(&[0xfa, 0x01]), Some((250, 2)));
        // Out of range for any real merchant, and it must still decode: the
        // range check is `getRecipeFor`'s, and it answers nothing rather than
        // throwing.
        assert_eq!(
            read_select_trade(&[0xff, 0xff, 0xff, 0xff, 0x0f]),
            Some((-1, 5))
        );
        assert_eq!(read_select_trade(&[0x80]), None, "truncated");
    }
}
