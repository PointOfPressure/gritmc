//! The framing against the codec, and the three payloads against theirs.

use super::*;

fn props() -> Option<crate::itemprops::ItemProps> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push(crate::itemprops::TABLE_PATH);
    crate::itemprops::ItemProps::load(&path).ok()
}

/// The ids the wire uses, and they are not derivable from the names: the
/// registry's order is registration order, and `custom_name` is 6 while the
/// alphabetically-sorted name list this table also carries puts it 15th.
#[test]
fn the_component_type_ids_are_the_registrys_own() {
    let Some(p) = props() else { return };
    assert_eq!(
        p.component_type_count(),
        111,
        "every type, not the 59 with a default"
    );
    assert_eq!(p.component_type_id("minecraft:max_damage"), Some(2));
    assert_eq!(p.component_type_id("minecraft:damage"), Some(3));
    assert_eq!(p.component_type_id("minecraft:custom_name"), Some(6));
    assert_eq!(p.component_type_id("minecraft:enchantments"), Some(13));
    assert_eq!(p.component_type_id("minecraft:bundle_contents"), Some(50));
    assert_eq!(p.component_type_id("minecraft:not_a_component"), None);
}

/// `0, 0` — and what it means is not "no components".
#[test]
fn the_empty_patch_is_two_zeroes_and_means_the_prototype() {
    let mut out = Vec::new();
    Patch::EMPTY.write(&mut out, &|_| Some(6)).expect("writes");
    assert_eq!(out, vec![0, 0]);
    assert_eq!(Patch::EMPTY.counts(), (0, 0));
}

/// A damage component: one added, id 3, then the value as a varint.
#[test]
fn a_damaged_stack_is_one_added_component() {
    let Some(p) = props() else { return };
    let patch = Patch::default().with(Component::Damage(7));
    assert_eq!(patch.counts(), (1, 0));
    let mut out = Vec::new();
    patch
        .write(&mut out, &|n| p.component_type_id(n))
        .expect("writes");
    assert_eq!(out, vec![1, 0, 3, 7], "added=1, removed=0, type 3, value 7");
}

/// A removal is a type id and nothing else, and it goes in the SECOND loop —
/// after every added component, not interleaved.
#[test]
fn removals_are_ids_alone_and_come_after_every_addition() {
    let Some(p) = props() else { return };
    let patch = Patch::default()
        .without("minecraft:max_damage")
        .with(Component::Damage(1))
        .without("minecraft:custom_name");
    assert_eq!(patch.counts(), (1, 2));
    let mut out = Vec::new();
    patch
        .write(&mut out, &|n| p.component_type_id(n))
        .expect("writes");
    assert_eq!(
        out,
        vec![1, 2, 3, 1, 2, 6],
        "added=1 removed=2, then damage(3)=1, then the two removals 2 and 6 — \
         the removals last even though one was added to the patch first"
    );
}

/// **A bundle's stacks use the non-optional codec.** The optional one would
/// add a leading `1` per stack and the client would read the next stack's
/// count as an item id.
#[test]
fn bundle_contents_is_a_count_then_bare_stacks() {
    let Some(p) = props() else { return };
    let patch = Patch::default().with(Component::BundleContents(vec![
        crate::menu::WireStack::plain(12, 3),
        crate::menu::WireStack::plain(5, 64),
    ]));
    let mut out = Vec::new();
    patch
        .write(&mut out, &|n| p.component_type_id(n))
        .expect("writes");
    assert_eq!(
        out,
        vec![1, 0, 50, 2, 3, 12, 0, 0, 64, 5, 0, 0],
        "added=1 removed=0, type 50, list length 2, then (count,id,0,0) twice — \
         no zero-means-absent varint in front of either"
    );
}

/// The custom name is network NBT — a tag byte, then the payload, with no
/// root name. A named root would put two length bytes in front and the client
/// would read the string length as a tag type.
#[test]
fn a_custom_name_is_network_nbt_with_no_root_name() {
    let Some(p) = props() else { return };
    let name = Nbt::Compound(
        [("text".to_string(), Nbt::String("Hi".to_string()))]
            .into_iter()
            .collect(),
    );
    let patch = Patch::default().with(Component::CustomName(name));
    let mut out = Vec::new();
    patch
        .write(&mut out, &|n| p.component_type_id(n))
        .expect("writes");
    assert_eq!(
        &out[..3],
        &[1, 0, 6],
        "added=1, removed=0, custom_name is 6"
    );
    assert_eq!(out[3], 10, "TAG_Compound, and no name length after it");
    assert_eq!(
        out[4], 8,
        "TAG_String — the first field, straight after the tag"
    );
}

/// A type name the table does not know is an error and not a silent skip.
/// Skipping would leave the header's count promising an entry that is not
/// there, and the client would read the rest of the packet as that
/// component's payload.
#[test]
fn an_unknown_component_type_is_refused_rather_than_skipped() {
    let patch = Patch::default().with(Component::Damage(1));
    let mut out = Vec::new();
    let err = patch.write(&mut out, &|_| None).expect_err("refuses");
    assert!(err.to_string().contains("minecraft:damage"), "{err}");
}

/// Ids for the payload tests below, chosen so a wrong registry or a missing
/// `+1` lands on a different byte: every registry answers a distinct range.
fn fake_types(n: &str) -> Option<i32> {
    Some(match n {
        "minecraft:enchantments" => 13,
        "minecraft:stored_enchantments" => 34,
        "minecraft:potion_contents" => 42,
        "minecraft:suspicious_stew_effects" => 44,
        "minecraft:instrument" => 59,
        "minecraft:ominous_bottle_amplifier" => 60,
        "minecraft:damage" => 3,
        _ => return None,
    })
}

fn fake_holders(r: &str, n: &str) -> Option<i32> {
    Some(match (r, n) {
        ("minecraft:enchantment", "minecraft:sharpness") => 5,
        ("minecraft:enchantment", "minecraft:mending") => 21,
        ("minecraft:potion", "minecraft:water_breathing") => 19,
        ("minecraft:mob_effect", "minecraft:night_vision") => 15,
        ("minecraft:instrument", "minecraft:seek_goat_horn") => 1,
        _ => return None,
    })
}

fn bytes(p: &Patch) -> Vec<u8> {
    let mut out = Vec::new();
    p.write_with(&mut out, &fake_types, &fake_holders)
        .expect("writes");
    out
}

/// `ItemEnchantments.STREAM_CODEC`: `ByteBufCodecs.map` — a count, then
/// `holderRegistry` ids (no `+1`) each followed by a varint level. A book's
/// is the same codec under `stored_enchantments`.
#[test]
fn enchantments_are_a_count_then_plain_id_level_pairs() {
    let p = Patch::default().with(Component::StoredEnchantments(vec![(
        "minecraft:mending".into(),
        1,
    )]));
    assert_eq!(bytes(&p), vec![1, 0, 34, 1, 21, 1]);
    let p = Patch::default()
        .with(Component::Damage(200))
        .with(Component::Enchantments(vec![(
            "minecraft:sharpness".into(),
            3,
        )]));
    // damage first because it was set first: 200 is the two-byte varint
    // 0xC8 0x01.
    assert_eq!(bytes(&p), vec![2, 0, 3, 0xC8, 0x01, 13, 1, 5, 3]);
}

/// `PotionContents.STREAM_CODEC`: optional potion (bool + plain id), then an
/// absent colour, an empty effect list and an absent name.
#[test]
fn a_set_potion_is_present_potion_then_three_empty_fields() {
    let p = Patch::default().with(Component::PotionContents(Some(
        "minecraft:water_breathing".into(),
    )));
    assert_eq!(bytes(&p), vec![1, 0, 42, 1, 19, 0, 0, 0]);
}

/// `SuspiciousStewEffects`: a list of `(mob_effect id, duration)`.
#[test]
fn stew_effects_are_a_list_of_effect_duration_pairs() {
    let p = Patch::default().with(Component::SuspiciousStewEffects(vec![(
        "minecraft:night_vision".into(),
        100,
    )]));
    assert_eq!(bytes(&p), vec![1, 0, 44, 1, 15, 100]);
}

/// `ByteBufCodecs.holder`: a registry holder is written `id + 1`, which is
/// what separates it from every other holder in this file.
#[test]
fn an_instrument_is_its_id_plus_one() {
    let p = Patch::default().with(Component::Instrument("minecraft:seek_goat_horn".into()));
    assert_eq!(bytes(&p), vec![1, 0, 59, 2]);
}

#[test]
fn an_ominous_amplifier_is_one_varint() {
    let p = Patch::default().with(Component::OminousBottleAmplifier(3));
    assert_eq!(bytes(&p), vec![1, 0, 60, 3]);
}

/// A holder the tables do not name refuses the patch: no id is invented.
#[test]
fn an_unknown_holder_is_refused() {
    let p = Patch::default().with(Component::Enchantments(vec![(
        "minecraft:not_an_enchantment".into(),
        1,
    )]));
    let mut out = Vec::new();
    assert!(p.write_with(&mut out, &fake_types, &fake_holders).is_err());
    // And `write`, which has no holder resolver, refuses every holder.
    let p = Patch::default().with(Component::Instrument("minecraft:seek_goat_horn".into()));
    assert!(p.write(&mut Vec::new(), &fake_types).is_err());
}

/// The real tables: the synchronised registries come out of the capture the
/// client is sent, the built-in ones out of the registry report.
#[test]
fn the_holder_ids_are_the_clients_own() {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root.pop();
    if !rustmc_data::fixture::available(
        &root.join("data/vanilla/protocol_ids.json"),
        "protocol ids with potions and mob effects",
        "ci/dump-protocol-ids.sh",
    ) {
        return;
    }
    let h = HolderIds::load(&root).expect("loads");
    // `MobEffects` registers speed first; `Potions` registers water first.
    assert_eq!(h.id("minecraft:mob_effect", "minecraft:speed"), Some(0));
    assert_eq!(h.id("minecraft:potion", "minecraft:water"), Some(0));
    assert_eq!(
        h.id("minecraft:potion", "water"),
        Some(0),
        "bare names resolve"
    );
    // The enchantment registry is synchronised: its ids are the capture's
    // order, which must be the order `crate::enchantment` numbers
    // `enchantClue` by too, or one of the two is wrong on the client.
    let table = crate::enchantment::Registry::load(&root.join(crate::enchantment::TABLE_PATH))
        .expect("enchantment table");
    assert_eq!(table.len(), 43);
    for name in table.order() {
        assert_eq!(
            h.id("minecraft:enchantment", name),
            table.numeric_id(name),
            "{name}"
        );
    }
    assert!(h
        .id("minecraft:instrument", "minecraft:ponder_goat_horn")
        .is_some());
    assert_eq!(h.id("minecraft:enchantment", "minecraft:nope"), None);
}
