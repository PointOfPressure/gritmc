//! `DataComponentPatch` on the wire — what makes a stack more than an id and
//! a count.
//!
//! # The framing, from the codec's own encoder
//!
//! `DataComponentPatch$3.encode` (offsets 0-19 for the empty case, 20-196 for
//! the rest):
//!
//! ```text
//!    0-19   isEmpty() -> writeVarInt(0); writeVarInt(0); return
//!   20-88   count the map: present Optionals into `added`, empty into `removed`
//!   89-101  writeVarInt(added); writeVarInt(removed)
//!  102-196  for each present entry: type id, then the component's own codec
//!           for each empty  entry: type id, and nothing else
//! ```
//!
//! Three things that framing settles, and the last is the one this server had
//! wrong:
//!
//! * **The two counts are the whole header.** There is no length prefix
//!   around a component's payload, so a reader that does not know a
//!   component's codec cannot skip it — which is why the added list must be
//!   written in a form the client can decode, not merely in *a* form.
//! * **A removal is a type id and nothing else.** It is how a stack says "my
//!   item's prototype has this component and I do not", which is the only way
//!   to express the absence of a default.
//! * **`0, 0` is not "no components".** It is "exactly the item's prototype
//!   components", which is what [`Patch::EMPTY`] means and what
//!   `crate::menu::write_item` has always written. Every stack this server
//!   has ever put on the wire has been the prototype; a damaged pickaxe, a
//!   renamed sign and a full bundle are all `0, 0` today and all wrong.
//!
//! # The type ids are a table, not a constant
//!
//! Each key goes through `DataComponentType.STREAM_CODEC`, which is the
//! `minecraft:data_component_type` registry id as a varint. Those ids are the
//! registry's **registration** order and cannot be derived from the names —
//! `item_props.json` carries all 111 of them in `componentTypeIds`, and all
//! 111 rather than the 59 that some item's prototype carries, because a patch
//! names components no prototype has. `minecraft:custom_name` is 6 and no
//! item has one by default.
//!
//! # What is here and what is refused
//!
//! The components with a payload this module can write are the ones this
//! server can put on a stack: the three it started with (`custom_name`,
//! `bundle_contents`, `damage`) and the six a loot table sets
//! (`enchantments`, `stored_enchantments`, `potion_contents`,
//! `suspicious_stew_effects`, `instrument`, `ominous_bottle_amplifier`).
//! Each is transcribed from its own stream codec. Everything else is
//! [`Component::Removed`] or nothing at all: a component whose codec this
//! port has not read **must not** be written, because there is no length
//! prefix to hide behind and a wrong payload desynchronises the rest of the
//! packet rather than corrupting one field.

use crate::menu::WireStack;
use rustmc_proto::varint::write_varint;
use rustmc_world::nbt::Nbt;
use std::io;

/// One entry of a patch.
#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    /// `minecraft:custom_name` — a `Component`, written as network NBT.
    ///
    /// `ComponentSerialization`'s stream codec is an NBT write with no root
    /// name, the same shape `crate::commands::write_network_nbt` produces for
    /// a chat line and `crate::menu` for a menu title.
    CustomName(Nbt),
    /// `minecraft:bundle_contents` — a `BundleContents`, whose stream codec
    /// is `ItemStack.STREAM_CODEC.apply(ByteBufCodecs.list())`: a varint
    /// count then that many stacks.
    ///
    /// **The non-optional `STREAM_CODEC`, not the `OPTIONAL_` one.** A
    /// bundle's contents cannot hold an empty stack, so each element is
    /// count, id, patch — with no leading zero-means-absent varint. Writing
    /// the optional form here adds a byte per item and the client reads the
    /// next stack's count as an item id.
    BundleContents(Vec<WireStack>),
    /// `minecraft:damage` — a plain varint. The component
    /// `crate::durability` writes, and the one a shovel losing durability
    /// puts in a `container_set_slot`.
    Damage(i32),
    /// `minecraft:enchantments` — `ItemEnchantments.STREAM_CODEC`:
    ///
    /// ```text
    ///   ByteBufCodecs.map(Object2IntOpenHashMap::new,
    ///                     Enchantment.STREAM_CODEC, ByteBufCodecs.VAR_INT)
    ///   ByteBufCodecs$25.encode: writeCount(size) ; map.forEach(key, value)
    ///   Enchantment.STREAM_CODEC = ByteBufCodecs.holderRegistry(ENCHANTMENT)
    /// ```
    ///
    /// A varint count, then `(registry id, level)` varint pairs. The id is the
    /// plain registry id (`holderRegistry`, not `holder`: no `+1`), and the
    /// enchantment registry is a synchronised one, so the id is its position
    /// in the `registry_data` the client was sent.
    ///
    /// **The pair order is `Object2IntOpenHashMap`'s walk over
    /// `Holder.Reference` keys, and `Holder$Reference` does not override
    /// `hashCode`** — the slot is an identity hash, which no table outside
    /// that JVM can reproduce. Written here in the order the caller gives;
    /// exact for one enchantment, and for more only where the two orders
    /// happen to agree.
    Enchantments(Vec<(String, i32)>),
    /// `minecraft:stored_enchantments` — the same `ItemEnchantments` codec,
    /// under an enchanted book's other component type.
    StoredEnchantments(Vec<(String, i32)>),
    /// `minecraft:potion_contents` — `PotionContents.STREAM_CODEC`, a
    /// four-field composite (static initialiser offsets 65..143):
    ///
    /// ```text
    ///   optional(Potion.STREAM_CODEC)      bool, then holderRegistry(POTION) id
    ///   optional(ByteBufCodecs.INT)        bool, then a 4-byte custom colour
    ///   list(MobEffectInstance.STREAM_CODEC) varint count, then effects
    ///   optional(STRING_UTF8)              bool, then a custom name
    /// ```
    ///
    /// Only the potion is modelled — `SetPotionFunction` sets nothing else —
    /// so the other three are written empty: `false`, `0`, `false`.
    PotionContents(Option<String>),
    /// `minecraft:suspicious_stew_effects` — `SuspiciousStewEffects
    /// .STREAM_CODEC` is `Entry.STREAM_CODEC.apply(list())`, and an `Entry` is
    /// `composite(MobEffect.STREAM_CODEC, VAR_INT)`: a varint count, then
    /// `(mob_effect id, duration)` varint pairs. `MobEffect.STREAM_CODEC` is
    /// `holderRegistry(MOB_EFFECT)`, a plain id.
    SuspiciousStewEffects(Vec<(String, i32)>),
    /// `minecraft:instrument` — `InstrumentComponent.STREAM_CODEC` maps
    /// `Instrument.STREAM_CODEC`, which is `ByteBufCodecs.holder(INSTRUMENT,
    /// DIRECT_STREAM_CODEC)`. `ByteBufCodecs$30.encode` writes a registry
    /// holder as **`id + 1`** (offsets 42..56) and reserves `0` for an inline
    /// value — the one holder in this enum that is not a plain id.
    Instrument(String),
    /// `minecraft:ominous_bottle_amplifier` — `composite(VAR_INT)`, one
    /// varint.
    OminousBottleAmplifier(i32),
    /// `minecraft:repair_cost` — `ByteBufCodecs.VAR_INT`, the anvil's
    /// running cost.
    RepairCost(i32),
    /// `minecraft:banner_patterns` — `BannerPatternLayers.STREAM_CODEC`, a
    /// list of `Layer(holder(BANNER_PATTERN), DyeColor)`: a varint count,
    /// then each layer's pattern as a **holder** (`id + 1`) and its dye
    /// colour's ordinal. (pattern id, dye ordinal), bottom layer first.
    BannerPatterns(Vec<(String, i32)>),
    /// `minecraft:trim` — `ArmorTrim.STREAM_CODEC`, `composite(
    /// TrimMaterial.STREAM_CODEC, TrimPattern.STREAM_CODEC)`, both
    /// `ByteBufCodecs.holder` and so `id + 1`. (material, pattern).
    Trim(String, String),
    /// A value already in its type's own stream codec, written verbatim —
    /// what a creative client's delimited patch holds once its length
    /// prefix is stripped (`crate::player`'s `read_untrusted_patch`). Named
    /// by the entry, like [`Component::Removed`].
    Raw(Vec<u8>),
    /// The item's prototype has this component and this stack does not.
    /// Written as its type id alone, in the removed list.
    Removed,
}

impl Component {
    /// The `minecraft:data_component_type` name this variant is.
    #[must_use]
    pub const fn type_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::CustomName(_) => "minecraft:custom_name",
            Self::BundleContents(_) => "minecraft:bundle_contents",
            Self::Damage(_) => "minecraft:damage",
            Self::Enchantments(_) => "minecraft:enchantments",
            Self::StoredEnchantments(_) => "minecraft:stored_enchantments",
            Self::PotionContents(_) => "minecraft:potion_contents",
            Self::SuspiciousStewEffects(_) => "minecraft:suspicious_stew_effects",
            Self::Instrument(_) => "minecraft:instrument",
            Self::OminousBottleAmplifier(_) => "minecraft:ominous_bottle_amplifier",
            Self::RepairCost(_) => "minecraft:repair_cost",
            Self::BannerPatterns(_) => "minecraft:banner_patterns",
            Self::Trim(..) => "minecraft:trim",
            // A removal and a raw value carry the name of their type, which
            // the caller holds; these variants do not know it.
            Self::Removed | Self::Raw(_) => return None,
        })
    }

    const fn is_removal(&self) -> bool {
        matches!(self, Self::Removed)
    }
}

/// A stack's patch: the components it has that its prototype does not, and
/// the ones its prototype has that it does not.
///
/// Ordered, and the order is the caller's. Vanilla's own is a
/// `Reference2ObjectArrayMap`'s insertion order, which is not reproducible
/// from outside the JVM — so this is a `Vec` and the caller decides. **That
/// matters for a byte-comparison and is said out loud**: two patches with the
/// same entries in different orders are both valid and are different bytes,
/// so a corpus check on a stack with more than one component compares against
/// whatever order vanilla happened to build, not against a canonical one.
#[derive(Debug, Clone, Default)]
pub struct Patch {
    entries: Vec<(String, Component)>,
}

impl Patch {
    /// The patch every stack this server has ever sent has had: `0, 0`.
    pub const EMPTY: Self = Self {
        entries: Vec::new(),
    };

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add a component under its own type name.
    #[must_use]
    pub fn with(mut self, component: Component) -> Self {
        if let Some(name) = component.type_name() {
            self.entries.push((name.to_string(), component));
        }
        self
    }

    /// Add a component under `type_name` whose value is already encoded.
    #[must_use]
    pub fn with_raw(mut self, type_name: &str, bytes: Vec<u8>) -> Self {
        self.entries
            .push((type_name.to_string(), Component::Raw(bytes)));
        self
    }

    /// Remove a component the item's prototype carries.
    #[must_use]
    pub fn without(mut self, type_name: &str) -> Self {
        self.entries
            .push((type_name.to_string(), Component::Removed));
        self
    }

    /// `added`, `removed` — the two counts the header carries.
    #[must_use]
    pub fn counts(&self) -> (i32, i32) {
        let removed = self.entries.iter().filter(|(_, c)| c.is_removal()).count();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "a patch is at most 111 entries — one per component type"
        )]
        ((self.entries.len() - removed) as i32, removed as i32)
    }

    /// Write the patch.
    ///
    /// `type_id` resolves a `minecraft:data_component_type` name to its
    /// registry id — [`crate::itemprops::ItemProps::component_type_id`].
    ///
    /// # Errors
    /// If a component's type name is not in the registry, which is a table
    /// mismatch and not a wire condition: writing the entry without its id
    /// would put a patch on the wire whose remaining bytes the client reads
    /// as the next field.
    pub fn write(
        &self,
        out: &mut Vec<u8>,
        type_id: &dyn Fn(&str) -> Option<i32>,
    ) -> io::Result<()> {
        self.write_with(out, type_id, &|_, _| None)
    }

    /// [`Patch::write`] with a resolver for the registry holders inside a
    /// component's payload — `holder_id(registry, name)`, for example
    /// `("minecraft:enchantment", "minecraft:sharpness")`. [`HolderIds`] is
    /// the server's one.
    ///
    /// # Errors
    /// As [`Patch::write`], and also when a holder inside a payload has no
    /// id: a component written without one would desynchronise the packet.
    pub fn write_with(
        &self,
        out: &mut Vec<u8>,
        type_id: &dyn Fn(&str) -> Option<i32>,
        holder_id: &dyn Fn(&str, &str) -> Option<i32>,
    ) -> io::Result<()> {
        let (added, removed) = self.counts();
        write_varint(out, added)?;
        write_varint(out, removed)?;
        // The present entries first, then the empty ones — the encoder's two
        // loops at offsets 102 and 148, in that order.
        for (name, component) in self.entries.iter().filter(|(_, c)| !c.is_removal()) {
            write_varint(out, id_of(name, type_id)?)?;
            write_component(out, component, holder_id)?;
        }
        for (name, _) in self.entries.iter().filter(|(_, c)| c.is_removal()) {
            write_varint(out, id_of(name, type_id)?)?;
        }
        Ok(())
    }
}

/// `Reference2ObjectArrayMap.equals` is `Map.equals`: the same keys with
/// equal values, **in any order** — the insertion order decides the bytes,
/// not the identity. An `ItemEnchantments` is an `Object2IntOpenHashMap`, so
/// two enchantment lists are equal as sets of pairs too.
impl PartialEq for Patch {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Component, b: &Component| match (a, b) {
            (Component::Enchantments(x), Component::Enchantments(y))
            | (Component::StoredEnchantments(x), Component::StoredEnchantments(y)) => {
                let (mut x, mut y) = (x.clone(), y.clone());
                x.sort();
                y.sort();
                x == y
            }
            _ => a == b,
        };
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .all(|(n, c)| other.get(n).is_some_and(|o| same(c, o)))
    }
}

/// Floats inside a `custom_name` make `PartialEq` hand-written; equality here
/// is "the same patch", which is what `ItemStack` needs it for.
impl Eq for Patch {}

impl Patch {
    /// The entries in the order they will be written.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Component)> {
        self.entries.iter().map(|(n, c)| (n.as_str(), c))
    }

    /// `Reference2ObjectArrayMap.put`: replace the entry for `type_name` in
    /// place if there is one, append it otherwise.
    pub fn put(&mut self, type_name: &str, component: Component) {
        if let Some(e) = self.entries.iter_mut().find(|(n, _)| n == type_name) {
            e.1 = component;
        } else {
            self.entries.push((type_name.to_string(), component));
        }
    }

    /// `Reference2ObjectArrayMap.remove`: drop the entry, closing the gap
    /// without reordering the rest.
    pub fn remove(&mut self, type_name: &str) {
        self.entries.retain(|(n, _)| n != type_name);
    }

    /// The entry for `type_name`, if the patch names it — the last one, as a
    /// map would keep it.
    #[must_use]
    pub fn get(&self, type_name: &str) -> Option<&Component> {
        self.entries
            .iter()
            .rev()
            .find(|(n, _)| n == type_name)
            .map(|(_, c)| c)
    }
}

/// The registry ids of the holders a component payload names.
///
/// Two kinds of registry, two sources, both the ones the client's own ids
/// come from:
///
/// * `minecraft:enchantment` and `minecraft:instrument` are **synchronised**
///   registries: the client numbers their entries in the order it received
///   them, so the id is the position in `data/vanilla/registries.bin`, the
///   capture this server replays at configuration
///   ([`crate::configuration::CapturedRegistries::entry_ids`]).
/// * `minecraft:potion` and `minecraft:mob_effect` are built-in: the id is
///   the `protocol_id` in vanilla's registry report, carried in
///   `data/vanilla/protocol_ids.json` (`ci/dump-protocol-ids.sh`).
#[derive(Debug, Default)]
pub struct HolderIds {
    by_registry: std::collections::BTreeMap<String, std::collections::BTreeMap<String, i32>>,
}

impl HolderIds {
    /// The synchronised registries read out of the capture.
    pub const SYNCED: [&'static str; 2] = ["minecraft:enchantment", "minecraft:instrument"];
    /// The built-in ones, as `protocol_ids.json` keys them.
    pub const BUILT_IN: [(&'static str, &'static str); 2] = [
        ("minecraft:potion", "potions"),
        ("minecraft:mob_effect", "mob_effects"),
    ];

    /// # Errors
    /// If either file is missing or not the expected shape, or a registry
    /// named above is absent from it.
    pub fn load(root: &std::path::Path) -> io::Result<Self> {
        let mut by_registry = std::collections::BTreeMap::new();
        let caps = crate::configuration::CapturedRegistries::load(
            &root.join("data/vanilla/registries.bin"),
        )?;
        for reg in Self::SYNCED {
            let names = caps.entry_ids(reg)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("registry capture has no {reg}"),
                )
            })?;
            let ids = names
                .into_iter()
                .enumerate()
                .filter_map(|(i, n)| i32::try_from(i).ok().map(|i| (n, i)))
                .collect();
            by_registry.insert(reg.to_string(), ids);
        }
        let text = std::fs::read_to_string(root.join("data/vanilla/protocol_ids.json"))?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        for (reg, key) in Self::BUILT_IN {
            let obj = doc
                .get(key)
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("protocol_ids.json has no `{key}` (regenerate it)"),
                    )
                })?;
            let ids = obj
                .iter()
                .filter_map(|(n, v)| {
                    v.as_i64()
                        .and_then(|v| i32::try_from(v).ok())
                        .map(|v| (n.clone(), v))
                })
                .collect();
            by_registry.insert(reg.to_string(), ids);
        }
        Ok(Self { by_registry })
    }

    /// The process-wide table, from `data/vanilla` under the working
    /// directory a server is booted from. `None`, once, said on stderr, when
    /// it cannot be loaded — and then every holder-bearing component is
    /// refused rather than written with a guessed id.
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        static ONCE: std::sync::OnceLock<Option<HolderIds>> = std::sync::OnceLock::new();
        ONCE.get_or_init(|| match Self::load(std::path::Path::new(".")) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("no holder id tables ({e}): enchanted, potion and horn stacks will be sent as their prototypes");
                None
            }
        })
        .as_ref()
    }

    /// `registry` id of `name`; names without a namespace are `minecraft:`.
    #[must_use]
    pub fn id(&self, registry: &str, name: &str) -> Option<i32> {
        let reg = self.by_registry.get(registry)?;
        match reg.get(name) {
            Some(i) => Some(*i),
            None if !name.contains(':') => reg.get(&format!("minecraft:{name}")).copied(),
            None => None,
        }
    }
}

fn id_of(name: &str, type_id: &dyn Fn(&str) -> Option<i32>) -> io::Result<i32> {
    type_id(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no data component type id for {name}"),
        )
    })
}

fn holder(
    holder_id: &dyn Fn(&str, &str) -> Option<i32>,
    registry: &str,
    name: &str,
) -> io::Result<i32> {
    holder_id(registry, name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no {registry} id for {name}"),
        )
    })
}

/// A varint count then `(holder id, varint)` pairs — the shape
/// `ItemEnchantments` and `SuspiciousStewEffects` share on the wire.
fn write_id_int_pairs(
    out: &mut Vec<u8>,
    pairs: &[(String, i32)],
    registry: &str,
    holder_id: &dyn Fn(&str, &str) -> Option<i32>,
) -> io::Result<()> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "at most one entry per registry member"
    )]
    write_varint(out, pairs.len() as i32)?;
    for (name, value) in pairs {
        write_varint(out, holder(holder_id, registry, name)?)?;
        write_varint(out, *value)?;
    }
    Ok(())
}

fn write_component(
    out: &mut Vec<u8>,
    component: &Component,
    holder_id: &dyn Fn(&str, &str) -> Option<i32>,
) -> io::Result<()> {
    match component {
        Component::CustomName(nbt) => crate::commands::write_network_nbt(out, nbt),
        Component::BundleContents(stacks) => {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "a bundle holds at most 64 stacks"
            )]
            write_varint(out, stacks.len() as i32)?;
            for stack in stacks {
                // `ItemStack.STREAM_CODEC` — count, id, patch. **No leading
                // zero-means-absent varint**: that is the `OPTIONAL_` codec
                // and a bundle's list does not use it.
                write_varint(out, stack.count)?;
                write_varint(out, stack.id)?;
                // A nested stack's own patch, already encoded by whoever made
                // the `WireStack`; none is the prototype.
                match &stack.patch {
                    Some(bytes) => out.extend_from_slice(bytes),
                    None => {
                        write_varint(out, 0)?;
                        write_varint(out, 0)?;
                    }
                }
            }
            Ok(())
        }
        Component::BannerPatterns(layers) => {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "a banner holds at most sixteen layers"
            )]
            write_varint(out, layers.len() as i32)?;
            for (pattern, dye) in layers {
                write_varint(
                    out,
                    holder(holder_id, "minecraft:banner_pattern", pattern)? + 1,
                )?;
                write_varint(out, *dye)?;
            }
            Ok(())
        }
        Component::Trim(material, pattern) => {
            write_varint(
                out,
                holder(holder_id, "minecraft:trim_material", material)? + 1,
            )?;
            write_varint(
                out,
                holder(holder_id, "minecraft:trim_pattern", pattern)? + 1,
            )
        }
        Component::Damage(value)
        | Component::OminousBottleAmplifier(value)
        | Component::RepairCost(value) => write_varint(out, *value),
        Component::Enchantments(pairs) | Component::StoredEnchantments(pairs) => {
            write_id_int_pairs(out, pairs, "minecraft:enchantment", holder_id)
        }
        Component::SuspiciousStewEffects(pairs) => {
            write_id_int_pairs(out, pairs, "minecraft:mob_effect", holder_id)
        }
        Component::PotionContents(potion) => {
            // `ByteBufCodecs$23.encode` — the optional codec: a boolean, then
            // the value only when present.
            match potion {
                Some(p) => {
                    out.push(1);
                    write_varint(out, holder(holder_id, "minecraft:potion", p)?)?;
                }
                None => out.push(0),
            }
            out.push(0); // customColor: Optional.empty
            write_varint(out, 0)?; // customEffects: List.of()
            out.push(0); // customName: Optional.empty
            Ok(())
        }
        Component::Instrument(name) => {
            // `ByteBufCodecs$30.encode`, Holder.Kind.REFERENCE: id + 1.
            write_varint(out, holder(holder_id, "minecraft:instrument", name)? + 1)
        }
        Component::Raw(bytes) => {
            out.extend_from_slice(bytes);
            Ok(())
        }
        Component::Removed => Ok(()),
    }
}

#[cfg(test)]
mod tests;
