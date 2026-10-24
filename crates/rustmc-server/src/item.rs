//! `ItemStack` — an item and how many of it.
//!
//! # Why this exists now and did not before
//!
//! Everything this server could previously do with an item was decided by its
//! *name*: `use_item_on` places whatever the selected hotbar slot names, and
//! `set_creative_mode_slot` names it. `crate::player::apply_hand` even read
//! the count off the creative packet and threw it away, because nothing could
//! have used it.
//!
//! A container is the first thing that can. `AbstractContainerMenu.doClick`'s
//! PICKUP branch is almost entirely arithmetic on counts — `getCount()` picks
//! how much a primary click moves, `(count + 1) / 2` is what a secondary click
//! takes, `safeInsert` caps at the slot's maximum and returns the remainder —
//! so a container built on names alone would be a container that cannot
//! half-stack, cannot overflow and cannot merge. Those are not missing
//! features that could be added later; they are the branches of the one method
//! the whole click path goes through.
//!
//! # `EMPTY` is a value, not an absence
//!
//! Vanilla's `ItemStack.EMPTY` is a real singleton with `count = 0`, and
//! `isEmpty()` is a method on it rather than a null check. Every arm of
//! `doClick` calls `.isEmpty()` on stacks that are never null. Modelling that
//! as `Option<ItemStack>` would put a Rust-shaped seam through the middle of a
//! transcribed method — every `if (stack.isEmpty())` becoming a match on a
//! different type than vanilla has — and the first bug it would hide is the
//! one where an operation leaves a count of 0 behind in a slot instead of
//! clearing it. So [`ItemStack::EMPTY`] is a value of the same type, and
//! [`ItemStack::is_empty`] is the predicate, exactly as vanilla has them.

/// `Item.MAX_STACK_SIZE` — the default `DataComponents.MAX_STACK_SIZE`.
///
/// **A named gap, not a silently assumed constant.** Vanilla stores the
/// maximum per item as a data component, defaulted to 64 by
/// `Item.Properties`; items that override it — every tool, every piece of
/// armour, buckets, ender pearls at 16, snowballs at 16 — do so in the
/// `Items` class registration and not anywhere this tree extracts today.
/// `data/vanilla/items.json` is the item registry's *ids* and carries no such
/// column, because it comes from the generator's registry report and the
/// report does not have one.
///
/// Every item the gameplay corpus puts in a container — `minecraft:stone`,
/// `minecraft:oak_log`, `minecraft:oak_planks` — is 64, so nothing measured
/// today can tell the difference. Something that stacks to 16 or to 1 would
/// be wrong at exactly one place: [`ItemStack::max_stack_size`]. That is why
/// the constant is read through a method rather than used inline — when the
/// column lands there is one body to change and no call sites.
pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;

/// An item and a count, or nothing.
///
/// `item` is the registry name. It is meaningful only when the stack is
/// non-empty; [`ItemStack::EMPTY`] carries the empty string and a count of 0,
/// and no code may read the name of an empty stack.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemStack {
    /// `DataComponents.BLOCK_ENTITY_DATA`, the one data component this server
    /// reads off a creative client's stack: the block-entity type's registry
    /// id and the tag, kept as the network-NBT **bytes** the client sent.
    ///
    /// It is the route a creative client puts a `LootTable` on a chest by
    /// (`BlockItem.updateCustomBlockEntityTag` at `BlockItem.place` offset
    /// 143); `container_loot`, the component one would reach for, is
    /// `persistent` only and has no stream codec, so no client can send it.
    /// Bytes rather than a parsed tag so the stack stays `Eq`; the one reader
    /// parses them at placement. Boxed: every stack pays one pointer.
    ///
    /// **Read at placement and nowhere else.** `split`, `copy_with_count` and
    /// `is_same_item_same_components` do not carry or compare it yet, so a
    /// stack moved through a container loses it — vanilla's does not.
    block_entity_data: Option<Box<(i32, Vec<u8>)>>,
    /// The `DataComponentPatch` this stack differs from its item's prototype
    /// by — what a loot table's `enchant_randomly`, `set_damage` or
    /// `set_potion` leaves on a rolled stack. `None` is the empty patch.
    /// Carried by `split` and `copy_with_count`, compared by
    /// `is_same_item_same_components`, written by `crate::menu::to_wire`.
    patch: Option<Box<crate::patch::Patch>>,
    /// `minecraft:entity_data` decoded to what this server applies — see
    /// [`EntityData`]. Kept beside `patch`, which also holds the raw value
    /// when the stack came off the wire, the way `block_entity_data` is.
    entity_data: Option<EntityData>,
    item: String,
    count: i32,
}

/// `DataComponents.ENTITY_DATA`, a `TypedEntityData<EntityType<?>>`, reduced
/// to what this server applies: the entity type it is for and its `Silent`
/// key. A spawn egg applies it through `EntityType.updateCustomEntityTag`
/// (javap 17-27: nothing unless the type is the one being spawned; 77
/// `TypedEntityData.loadInto`), and `Entity.load` reads `Silent` with
/// `getBooleanOr("Silent", false)`. Every other key is not read, and a stack
/// carrying only those reads as carrying none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityData {
    /// The `minecraft:entity_type` name, e.g. `minecraft:pig`.
    pub entity: String,
    /// The compound's `Silent`, when it has one.
    pub silent: Option<bool>,
    /// The compound's `LeftHanded`, when it has one — `Mob.readAdditionalSaveData`
    /// (javap 111-120) reads it after `finalizeSpawn` has rolled its own.
    pub left_handed: Option<bool>,
}

/// A data component this server can hold on a stack, by its
/// `minecraft:data_component_type` name.
///
/// The seven the workstation menus read and write, as a typed view over the
/// stack's one store, its [`crate::patch::Patch`]. Every other component —
/// a loot roll's potion, a creative client's raw values — lives in the same
/// patch and is simply not one of these kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentKind {
    CustomName,
    Damage,
    RepairCost,
    Enchantments,
    StoredEnchantments,
    BannerPatterns,
    Trim,
}

impl ComponentKind {
    /// The registry name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CustomName => "minecraft:custom_name",
            Self::Damage => "minecraft:damage",
            Self::RepairCost => "minecraft:repair_cost",
            Self::Enchantments => "minecraft:enchantments",
            Self::StoredEnchantments => "minecraft:stored_enchantments",
            Self::BannerPatterns => "minecraft:banner_patterns",
            Self::Trim => "minecraft:trim",
        }
    }

    /// The inverse of [`Self::name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "minecraft:custom_name" => Self::CustomName,
            "minecraft:damage" => Self::Damage,
            "minecraft:repair_cost" => Self::RepairCost,
            "minecraft:enchantments" => Self::Enchantments,
            "minecraft:stored_enchantments" => Self::StoredEnchantments,
            "minecraft:banner_patterns" => Self::BannerPatterns,
            "minecraft:trim" => Self::Trim,
            _ => return None,
        })
    }
}

/// A component's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentValue {
    /// `DAMAGE` and `REPAIR_COST` — both `Integer`.
    Int(i32),
    /// `CUSTOM_NAME` as the literal text an anvil writes —
    /// `Component.literal(itemName)`.
    Text(String),
    /// `ENCHANTMENTS` or `STORED_ENCHANTMENTS` — `ItemEnchantments`, as
    /// (enchantment id, level) in the order the map was built.
    ///
    /// Vanilla's map is an `Object2IntOpenHashMap<Holder<Enchantment>>` and a
    /// `Holder.Reference` has identity hashing, so the order vanilla puts two
    /// or more enchantments on the wire is not reproducible even between two
    /// runs of vanilla. Equality here ignores order for that reason; the wire
    /// order is this list's.
    Enchantments(Vec<(String, i32)>),
    /// `BANNER_PATTERNS` — `BannerPatternLayers`, as (pattern id, `DyeColor`
    /// ordinal) per layer, bottom first. A list, so order is part of the
    /// value.
    Layers(Vec<(String, i32)>),
    /// `TRIM` — `ArmorTrim(material, pattern)`, both registry holders, as
    /// their ids.
    Trim { material: String, pattern: String },
}

impl ComponentValue {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Enchantments(a), Self::Enchantments(b)) => {
                let mut a = a.clone();
                let mut b = b.clone();
                a.sort();
                b.sort();
                a == b
            }
            _ => self == other,
        }
    }
}

/// The typed entries of a stack's patch, in patch order: (kind, `Optional`
/// value), `None` meaning "the prototype has this and the stack does not".
/// A snapshot — [`ItemStack::components`] builds it from the patch, which is
/// what the stack holds and what goes on the wire.
///
/// Insertion order is what `DataComponentPatch`'s stream codec writes, because
/// the map is a `Reference2ObjectArrayMap`: `put` on a new key appends, `put`
/// on an existing key replaces in place, and `remove` closes the gap without
/// reordering. So the bytes of a stack depend on the order its components were
/// set, and this keeps that order.
#[derive(Debug, Clone, Default)]
pub struct Components(Vec<(ComponentKind, Option<ComponentValue>)>);

impl PartialEq for Components {
    /// `Reference2ObjectArrayMap.equals` is `Map.equals` — order-insensitive.
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|(k, v)| {
                other.0.iter().any(|(k2, v2)| {
                    k == k2
                        && match (v, v2) {
                            (Some(a), Some(b)) => a.same(b),
                            (None, None) => true,
                            _ => false,
                        }
                })
            })
    }
}

impl Eq for Components {}

impl Components {
    /// `DataComponentPatch.isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The entries in wire order.
    #[must_use]
    pub fn entries(&self) -> &[(ComponentKind, Option<ComponentValue>)] {
        &self.0
    }
}

/// The value `kind` has in `item`'s **prototype** — what the stack has when
/// its patch says nothing about it.
///
/// Read from `DataComponents.COMMON_ITEM_COMPONENTS`, which every `Item`
/// starts from: it carries `REPAIR_COST` 0 and an empty `ENCHANTMENTS`, and
/// no `CUSTOM_NAME`. `DAMAGE` 0 is added by `Item.Properties.durability`,
/// which is what gives an item `MAX_DAMAGE`, so it is in the prototype of
/// exactly the damageable items — `damageable` says which. `Items.ENCHANTED_BOOK`
/// is the one item registered with an empty `STORED_ENCHANTMENTS`.
#[must_use]
pub fn prototype_value(
    item: &str,
    kind: ComponentKind,
    damageable: bool,
) -> Option<ComponentValue> {
    match kind {
        ComponentKind::CustomName => None,
        ComponentKind::Damage => damageable.then_some(ComponentValue::Int(0)),
        ComponentKind::RepairCost => Some(ComponentValue::Int(0)),
        ComponentKind::Enchantments => Some(ComponentValue::Enchantments(Vec::new())),
        ComponentKind::StoredEnchantments => {
            (item == "minecraft:enchanted_book").then(|| ComponentValue::Enchantments(Vec::new()))
        }
        // Every `BannerItem` is registered with an empty `BANNER_PATTERNS`.
        ComponentKind::BannerPatterns => item
            .ends_with("_banner")
            .then(|| ComponentValue::Layers(Vec::new())),
        // No item is registered with a `TRIM`; only a smithing table adds one.
        ComponentKind::Trim => None,
    }
}

impl ComponentValue {
    /// This value as the patch entry `kind` stores it.
    fn into_component(self, kind: ComponentKind) -> Option<crate::patch::Component> {
        use crate::patch::Component as C;
        Some(match (kind, self) {
            (ComponentKind::Damage, Self::Int(n)) => C::Damage(n),
            (ComponentKind::RepairCost, Self::Int(n)) => C::RepairCost(n),
            // `Component.literal(text)` with no style and no siblings encodes
            // as a bare string tag.
            (ComponentKind::CustomName, Self::Text(t)) => {
                C::CustomName(rustmc_world::nbt::Nbt::String(t))
            }
            (ComponentKind::Enchantments, Self::Enchantments(l)) => C::Enchantments(l),
            (ComponentKind::StoredEnchantments, Self::Enchantments(l)) => C::StoredEnchantments(l),
            (ComponentKind::BannerPatterns, Self::Layers(l)) => C::BannerPatterns(l),
            (ComponentKind::Trim, Self::Trim { material, pattern }) => C::Trim(material, pattern),
            _ => return None,
        })
    }

    /// A patch entry read as `kind`'s value. A raw varint `damage` or
    /// `repair_cost` off a creative client's patch reads as its number; any
    /// other raw value, or a `custom_name` that is not a plain string, is not
    /// a value this view models.
    fn from_component(kind: ComponentKind, c: &crate::patch::Component) -> Option<Option<Self>> {
        use crate::patch::Component as C;
        Some(Some(match c {
            C::Removed => return Some(None),
            C::Damage(n) | C::RepairCost(n) => Self::Int(*n),
            C::CustomName(rustmc_world::nbt::Nbt::String(t)) => Self::Text(t.clone()),
            C::Enchantments(l) | C::StoredEnchantments(l) => Self::Enchantments(l.clone()),
            C::BannerPatterns(l) => Self::Layers(l.clone()),
            C::Trim(m, p) => Self::Trim {
                material: m.clone(),
                pattern: p.clone(),
            },
            C::Raw(bytes) if matches!(kind, ComponentKind::Damage | ComponentKind::RepairCost) => {
                let mut r = bytes.as_slice();
                let n = rustmc_proto::varint::read_varint(&mut r).ok()?;
                if !r.is_empty() {
                    return None;
                }
                Self::Int(n)
            }
            _ => return None,
        }))
    }
}

impl ItemStack {
    /// `ItemStack.EMPTY`.
    ///
    /// A function rather than a `const` because the name is a `String`;
    /// vanilla's is a singleton and this is the nearest honest thing.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            block_entity_data: None,
            patch: None,
            entity_data: None,
            item: String::new(),
            count: 0,
        }
    }

    /// A stack of `count` of `item`.
    ///
    /// A count of zero or less produces [`ItemStack::empty`], which is
    /// `ItemStack`'s own constructor behaviour: `new ItemStack(item, 0)` is
    /// `isEmpty()`, and letting a zero-count named stack exist here would make
    /// `is_empty` and `item` disagree about whether there is anything there.
    #[must_use]
    pub fn new(item: &str, count: i32) -> Self {
        if count <= 0 || item.is_empty() || item == "minecraft:air" {
            return Self::empty();
        }
        Self {
            block_entity_data: None,
            patch: None,
            entity_data: None,
            item: item.to_string(),
            count,
        }
    }

    /// One of `item`.
    #[must_use]
    pub fn one(item: &str) -> Self {
        Self::new(item, 1)
    }

    /// `ItemStack.isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item.is_empty() || self.item == "minecraft:air"
    }

    /// The registry name. Empty for an empty stack.
    #[must_use]
    pub fn item(&self) -> &str {
        &self.item
    }

    /// `ItemStack.getCount()`.
    #[must_use]
    pub const fn count(&self) -> i32 {
        self.count
    }

    /// `ItemStack.setCount(int)`.
    ///
    /// Dropping to zero or below empties the stack rather than leaving a
    /// named zero behind, which is what `ItemStack.setCount` plus the
    /// `isEmpty()` check on every read amounts to.
    pub fn set_count(&mut self, count: i32) {
        self.count = count;
        if self.count <= 0 {
            *self = Self::empty();
        }
    }

    /// `ItemStack.grow(int)`.
    pub fn grow(&mut self, by: i32) {
        self.set_count(self.count + by);
    }

    /// `ItemStack.shrink(int)`.
    pub fn shrink(&mut self, by: i32) {
        self.set_count(self.count - by);
    }

    /// `ItemStack.getMaxStackSize()` with no item table: the stack's own
    /// `minecraft:max_stack_size` when its patch sets one, else
    /// [`DEFAULT_MAX_STACK_SIZE`] — see there for why the prototype half is a
    /// constant here and a table in `crate::itemprops`.
    #[must_use]
    pub fn max_stack_size(&self) -> i32 {
        self.patched_max_stack_size()
            .unwrap_or(DEFAULT_MAX_STACK_SIZE)
    }

    /// The `minecraft:max_stack_size` this stack's patch sets, if it sets one.
    ///
    /// `ItemInstance.getMaxStackSize()` is `getOrDefault(MAX_STACK_SIZE, 1)`
    /// over the *patched* map, so a patch entry wins over the item's
    /// prototype — which is how a creative client makes stone that stacks to
    /// one. The value is `ByteBufCodecs.VAR_INT` on the wire
    /// (`DataComponents.lambda$static$1`, offsets 10-13).
    #[must_use]
    pub fn patched_max_stack_size(&self) -> Option<i32> {
        if self.is_empty() {
            return None;
        }
        match self.patch().get("minecraft:max_stack_size")? {
            crate::patch::Component::Raw(bytes) => {
                let mut r = bytes.as_slice();
                let n = rustmc_proto::varint::read_varint(&mut r).ok()?;
                (r.is_empty() && n > 0).then_some(n)
            }
            _ => None,
        }
    }

    /// `ItemStack.isStackable()` — offsets 0-27.
    ///
    /// `getMaxStackSize() > 1 && (!isDamageableItem() || !isDamaged())`. The
    /// second conjunct is not modelled and cannot decide anything that
    /// reaches here: a damaged stack is damageable, every damageable item's
    /// prototype stacks to one, and a patch that makes one stack higher is
    /// refused by `ItemStack.validateComponents` ("cannot be both damageable
    /// and stackable") before vanilla would hold it.
    ///
    /// An **empty** stack is stackable, and that is vanilla rather than an
    /// oversight: `ItemStack.EMPTY.getMaxStackSize()` is `Items.AIR`'s 64. It
    /// matters because `moveItemStackTo` tests `isStackable` *once, outside*
    /// its merge loop (offsets 16-20) and tests `isEmpty` *inside* it (offset
    /// 23) — reading the two as one guard would skip the merge phase for a
    /// stack that is about to be filled.
    #[must_use]
    pub fn is_stackable(&self) -> bool {
        self.max_stack_size() > 1
    }

    /// `ItemStack.isSameItem(a, b)` — offsets 0-8, which is `a.is(b.getItem())`
    /// and therefore **the item alone**.
    ///
    /// Not interchangeable with [`Self::is_same_item_same_components`], and the
    /// distinction is load-bearing rather than pedantic: `doClick`'s
    /// `QUICK_MOVE` loop condition at offsets 716-726 uses *this* one while
    /// `moveItemStackTo`'s merge test at offsets 79-85 uses the other. A
    /// damaged and an undamaged pickaxe are `isSameItem` and are not
    /// `isSameItemSameComponents`, so on such a pair the loop re-enters and
    /// the merge declines — and the loop still terminates, because
    /// `quickMoveStack` returns `EMPTY` the moment `moveItemStackTo` moves
    /// nothing. Collapsing the two predicates into one would turn that into an
    /// infinite loop the day this server has a component.
    ///
    /// `getItem()` on an empty stack is `Items.AIR`, so two empties are the
    /// same item and an empty is never the same item as anything else.
    #[must_use]
    pub fn is_same_item(a: &Self, b: &Self) -> bool {
        fn name(s: &ItemStack) -> &str {
            if s.is_empty() {
                "minecraft:air"
            } else {
                &s.item
            }
        }
        name(a) == name(b)
    }

    /// `ItemStack.split(int)` — take up to `amount` off this stack and return
    /// it, leaving the remainder behind.
    #[must_use]
    pub fn split(&mut self, amount: i32) -> Self {
        let taken = amount.min(self.count);
        if taken <= 0 {
            return Self::empty();
        }
        let mut out = Self::new(&self.item, taken);
        out.patch.clone_from(&self.patch);
        out.entity_data.clone_from(&self.entity_data);
        self.shrink(taken);
        out
    }

    /// `ItemStack.copyWithCount(int)` — `copy()` keeps the patch.
    #[must_use]
    pub fn copy_with_count(&self, count: i32) -> Self {
        let mut out = Self::new(&self.item, count);
        if !out.is_empty() {
            out.patch.clone_from(&self.patch);
            out.entity_data.clone_from(&self.entity_data);
        }
        out
    }

    /// `ItemStack.isSameItemSameComponents(a, b)`.
    ///
    /// Vanilla compares the item and then the whole `DataComponentPatch`, so
    /// two stacks of the same item with different damage, enchantments or
    /// custom names do *not* merge. Here that is the patch plus the decoded
    /// `minecraft:entity_data`, which a stack built in code (a test, an egg)
    /// carries without a raw patch entry.
    #[must_use]
    pub fn is_same_item_same_components(a: &Self, b: &Self) -> bool {
        if a.is_empty() || b.is_empty() {
            // Vanilla's is `a.is(b.getItem()) && ...` on two real stacks; two
            // empties are equal and an empty is never the same item as a
            // non-empty, which falls out of comparing the names.
            return a.is_empty() && b.is_empty();
        }
        // `Objects.equals(a.components, b.components)` over the patch, which
        // is the only part of the map that can differ between two stacks of
        // one item. `block_entity_data` is still not compared — see its field.
        a.item == b.item && a.patch() == b.patch() && a.entity_data == b.entity_data
    }
}

/// The one data component a stack here carries, kept apart from the item and
/// count methods above so that a lane adding components there does not meet
/// this one line for line.
impl ItemStack {
    /// This stack carrying a `minecraft:block_entity_data` component: the
    /// block-entity type id and the tag's network-NBT bytes. A no-op on an
    /// empty stack, which can carry no components.
    #[must_use]
    pub fn with_block_entity_data(mut self, type_id: i32, tag: Vec<u8>) -> Self {
        if !self.is_empty() {
            self.block_entity_data = Some(Box::new((type_id, tag)));
        }
        self
    }

    /// The typed entries of this stack's patch, in patch order — see
    /// [`Components`]. Entries of kinds the view does not model are left out.
    #[must_use]
    pub fn components(&self) -> Components {
        Components(
            self.patch()
                .entries()
                .filter_map(|(name, c)| {
                    let kind = ComponentKind::from_name(name)?;
                    Some((kind, ComponentValue::from_component(kind, c)?))
                })
                .collect(),
        )
    }

    /// The patch's entry for `kind`: `None` when the patch says nothing,
    /// `Some(None)` when it removes the prototype's value.
    fn patch_entry(&self, kind: ComponentKind) -> Option<Option<ComponentValue>> {
        ComponentValue::from_component(kind, self.patch().get(kind.name())?)
    }

    /// The patch, made writable: `None` stores as no patch at all.
    fn patch_mut(&mut self, f: impl FnOnce(&mut crate::patch::Patch)) {
        let mut p = self
            .patch
            .take()
            .map_or_else(crate::patch::Patch::default, |b| *b);
        f(&mut p);
        self.patch = (!p.is_empty()).then(|| Box::new(p));
    }

    /// What `kind` reads as on this stack: the patch's value if it has one,
    /// otherwise the prototype's.
    #[must_use]
    pub fn get_component(&self, kind: ComponentKind, damageable: bool) -> Option<ComponentValue> {
        match self.patch_entry(kind) {
            Some(v) => v,
            None => prototype_value(&self.item, kind, damageable),
        }
    }

    /// `PatchedDataComponentMap.set(type, value)`, offsets 0-73: a value equal
    /// to the prototype's **removes** the patch entry rather than storing it,
    /// and anything else is `put` — in place if the key is already there,
    /// appended if it is not.
    pub fn set_component(&mut self, kind: ComponentKind, value: ComponentValue, damageable: bool) {
        if self.is_empty() {
            return;
        }
        match prototype_value(&self.item, kind, damageable) {
            Some(p) if p.same(&value) => self.patch_mut(|p| p.remove(kind.name())),
            _ => {
                if let Some(c) = value.into_component(kind) {
                    self.patch_mut(|p| p.put(kind.name(), c));
                }
            }
        }
    }

    /// `PatchedDataComponentMap.remove(type)`, offsets 0-64: a component the
    /// prototype has is recorded as removed; one it lacks is simply dropped
    /// from the patch.
    pub fn remove_component(&mut self, kind: ComponentKind, damageable: bool) {
        if self.is_empty() {
            return;
        }
        if prototype_value(&self.item, kind, damageable).is_some() {
            self.patch_mut(|p| p.put(kind.name(), crate::patch::Component::Removed));
        } else {
            self.patch_mut(|p| p.remove(kind.name()));
        }
    }

    /// Store a decoded patch entry verbatim — `applyPatch`'s per-entry form.
    pub fn apply_patch_entry(
        &mut self,
        kind: ComponentKind,
        value: Option<ComponentValue>,
        damageable: bool,
    ) {
        match value {
            Some(v) => self.set_component(kind, v, damageable),
            None => self.remove_component(kind, damageable),
        }
    }

    /// `ItemStack.getDamageValue()` without the clamp — the raw `DAMAGE`.
    #[must_use]
    pub fn damage_value(&self) -> i32 {
        match self.patch_entry(ComponentKind::Damage) {
            Some(Some(ComponentValue::Int(v))) => v,
            _ => 0,
        }
    }

    /// `getOrDefault(REPAIR_COST, 0)`.
    #[must_use]
    pub fn repair_cost(&self) -> i32 {
        match self.patch_entry(ComponentKind::RepairCost) {
            Some(Some(ComponentValue::Int(v))) => v,
            _ => 0,
        }
    }

    /// `get(CUSTOM_NAME)` as plain text: a bare string tag, or a compound's
    /// `text` — what an anvil compares its name field against.
    #[must_use]
    pub fn custom_name_text(&self) -> Option<String> {
        match self.custom_name()? {
            rustmc_world::nbt::Nbt::String(t) => Some(t),
            n => match n.get("text")? {
                rustmc_world::nbt::Nbt::String(t) => Some(t.clone()),
                _ => None,
            },
        }
    }

    /// `ENCHANTMENTS` or `STORED_ENCHANTMENTS`, empty when absent.
    #[must_use]
    pub fn enchantments_of(&self, kind: ComponentKind) -> Vec<(String, i32)> {
        match self.patch_entry(kind) {
            Some(Some(ComponentValue::Enchantments(e))) => e,
            _ => Vec::new(),
        }
    }

    /// `ItemStack.transmuteCopy(item, count)` — a stack of a different item
    /// carrying this one's patch, `applyPatch`ed onto the new prototype.
    #[must_use]
    pub fn transmute_copy(&self, item: &str, count: i32, damageable: bool) -> Self {
        let mut out = Self::new(item, count);
        if out.is_empty() {
            return out;
        }
        for (name, c) in self.patch().entries() {
            match ComponentKind::from_name(name) {
                Some(kind) => {
                    if let Some(v) = ComponentValue::from_component(kind, c) {
                        out.apply_patch_entry(kind, v, damageable);
                    }
                }
                None => out.patch_mut(|p| p.put(name, c.clone())),
            }
        }
        out.entity_data.clone_from(&self.entity_data);
        out
    }

    /// This stack with `minecraft:damage` set — `setDamageValue`'s
    /// `Math.clamp(0, maxDamage)` floor; 0 is the prototype's value and leaves
    /// no entry. A no-op on an empty stack.
    #[must_use]
    pub fn with_damage(mut self, damage: i32) -> Self {
        if !self.is_empty() {
            let d = damage.max(0);
            self.patch_mut(|p| {
                if d == 0 {
                    p.remove("minecraft:damage");
                } else {
                    p.put("minecraft:damage", crate::patch::Component::Damage(d));
                }
            });
        }
        self
    }

    /// `getDamageValue()` — [`Self::damage_value`] floored at 0.
    #[must_use]
    pub fn damage(&self) -> i32 {
        self.damage_value().max(0)
    }

    /// This stack with one more enchantment in `minecraft:enchantments` (a
    /// level already there for the same id is replaced in place). Ids are
    /// stored namespaced; a bare one gets `minecraft:`.
    #[must_use]
    pub fn with_enchantment(mut self, id: &str, level: i32) -> Self {
        if self.is_empty() {
            return self;
        }
        let bare = id.rsplit(':').next().unwrap_or(id);
        let full = format!("minecraft:{bare}");
        let mut list = self.enchantments_of(ComponentKind::Enchantments);
        match list
            .iter_mut()
            .find(|(e, _)| e.rsplit(':').next() == Some(bare))
        {
            Some(e) => e.1 = level,
            None => list.push((full, level)),
        }
        self.patch_mut(|p| {
            p.put(
                "minecraft:enchantments",
                crate::patch::Component::Enchantments(list),
            );
        });
        self
    }

    /// The stack's `minecraft:enchantments`, **bare** id to level, in patch
    /// order.
    #[must_use]
    pub fn enchantments(&self) -> Vec<(String, i32)> {
        self.enchantments_of(ComponentKind::Enchantments)
            .into_iter()
            .map(|(e, l)| (e.rsplit(':').next().unwrap_or(&e).to_string(), l))
            .collect()
    }

    /// `EnchantmentHelper.getItemEnchantmentLevel` for this stack's own
    /// `minecraft:enchantments`: 0 when absent.
    #[must_use]
    pub fn enchantment_level(&self, id: &str) -> i32 {
        let bare = id.rsplit(':').next().unwrap_or(id);
        self.enchantments()
            .iter()
            .find(|(e, _)| e == bare)
            .map_or(0, |(_, l)| *l)
    }

    /// This stack with `minecraft:entity_data` set. A no-op on an empty stack.
    #[must_use]
    pub fn with_entity_data(mut self, data: EntityData) -> Self {
        if !self.is_empty() {
            self.entity_data = Some(data);
        }
        self
    }

    /// `stack.get(DataComponents.CUSTOM_NAME)` as the NBT its stream codec
    /// carries — whether the patch was built here or kept raw off a creative
    /// client's delimited patch. `None` for a removal, or raw bytes that are
    /// not one NBT tag.
    #[must_use]
    pub fn custom_name(&self) -> Option<rustmc_world::nbt::Nbt> {
        match self.patch().get("minecraft:custom_name")? {
            crate::patch::Component::CustomName(n) => Some(n.clone()),
            crate::patch::Component::Raw(bytes) => {
                let mut r = bytes.as_slice();
                let n = rustmc_world::nbt::read_network_tag(&mut r).ok()??;
                r.is_empty().then_some(n)
            }
            _ => None,
        }
    }

    /// `stack.get(DataComponents.CONTAINER)`, the stacks it holds by slot —
    /// `ItemContainerContents.copyInto`'s `createStackFromSlot(i)` for each
    /// listed position. `None` when the stack has no raw `container` entry,
    /// or one this server cannot read in full; see [`read_container`].
    #[must_use]
    pub fn container_items(&self, ids: &crate::player::ItemIds) -> Option<Vec<Self>> {
        match self.patch().get("minecraft:container")? {
            crate::patch::Component::Raw(bytes) => read_container(bytes, ids),
            _ => None,
        }
    }

    /// `stack.get(DataComponents.POT_DECORATIONS)` as the item names its
    /// stream codec lists: `ByteBufCodecs.registry(ITEM)` under
    /// `ByteBufCodecs.list(4)` (`PotDecorations` static initialiser, offsets
    /// 54-69) — a varint count of at most four, then item registry ids.
    #[must_use]
    pub fn pot_decorations(&self, ids: &crate::player::ItemIds) -> Option<Vec<String>> {
        let crate::patch::Component::Raw(bytes) = self.patch().get("minecraft:pot_decorations")?
        else {
            return None;
        };
        let mut cur = bytes.as_slice();
        let n = rustmc_proto::varint::read_varint(&mut cur).ok()?;
        if !(0..=4).contains(&n) {
            return None;
        }
        let mut out = Vec::new();
        for _ in 0..n {
            let id = rustmc_proto::varint::read_varint(&mut cur).ok()?;
            out.push(ids.name(id)?.to_string());
        }
        cur.is_empty().then_some(out)
    }

    /// `stack.get(DataComponents.BLOCK_STATE)`, the `(property, value)`
    /// pairs of a `BlockItemStateProperties`. Its stream codec is
    /// `ByteBufCodecs.map(HashMap::new, STRING_UTF8, STRING_UTF8)`: a varint
    /// count, then each key and value as a varint byte length and UTF-8.
    /// `None` when the patch has no raw entry or it does not read whole.
    #[must_use]
    pub fn block_state_props(&self) -> Option<Vec<(String, String)>> {
        let crate::patch::Component::Raw(bytes) = self.patch().get("minecraft:block_state")? else {
            return None;
        };
        let mut cur = bytes.as_slice();
        let n = rustmc_proto::varint::read_varint(&mut cur).ok()?;
        let mut out = Vec::with_capacity(usize::try_from(n).ok()?.min(64));
        let string = |cur: &mut &[u8]| -> Option<String> {
            let len = usize::try_from(rustmc_proto::varint::read_varint(cur).ok()?).ok()?;
            if cur.len() < len {
                return None;
            }
            let (s, rest) = cur.split_at(len);
            *cur = rest;
            String::from_utf8(s.to_vec()).ok()
        };
        for _ in 0..n {
            let k = string(&mut cur)?;
            let v = string(&mut cur)?;
            out.push((k, v));
        }
        cur.is_empty().then_some(out)
    }

    /// The stack's `minecraft:entity_data`, if it carries one.
    #[must_use]
    pub fn entity_data(&self) -> Option<&EntityData> {
        self.entity_data.as_ref()
    }

    /// This stack with `patch` as its component patch. A no-op on an empty
    /// stack; an empty patch is stored as none.
    #[must_use]
    pub fn with_patch(mut self, patch: crate::patch::Patch) -> Self {
        if !self.is_empty() {
            self.patch = (!patch.is_empty()).then(|| Box::new(patch));
        }
        self
    }

    /// The component patch; [`crate::patch::Patch::EMPTY`] for a prototype.
    #[must_use]
    pub fn patch(&self) -> &crate::patch::Patch {
        static EMPTY: crate::patch::Patch = crate::patch::Patch::EMPTY;
        self.patch.as_deref().unwrap_or(&EMPTY)
    }

    /// `stack.get(DataComponents.BLOCK_ENTITY_DATA)`.
    #[must_use]
    pub fn block_entity_data(&self) -> Option<(i32, &[u8])> {
        self.block_entity_data
            .as_deref()
            .map(|(t, b)| (*t, b.as_slice()))
    }
}

/// `ItemContainerContents.STREAM_CODEC` (static initialiser offsets 42-81):
/// `ItemStackTemplate.STREAM_CODEC` through `ByteBufCodecs.optional`, then
/// `ByteBufCodecs.list(256)`. So a varint count of at most 256, then per
/// slot a presence boolean and, when present, an `ItemStackTemplate`:
/// `Item.STREAM_CODEC` (the item registry id) **before** the `VAR_INT` count
/// — the other way round from `ItemStack`'s — then a trusted
/// `DataComponentPatch`.
///
/// **A nested stack must carry the empty patch (`0, 0`).** The trusted patch
/// has no length prefix, so a component this server has no reader for
/// cannot be stepped over; rather than guess, the whole container is refused
/// (`None`) and the placed block keeps its empty inventory.
#[must_use]
pub fn read_container(bytes: &[u8], ids: &crate::player::ItemIds) -> Option<Vec<ItemStack>> {
    use rustmc_proto::varint::read_varint;
    let mut cur = bytes;
    let n = read_varint(&mut cur).ok()?;
    if !(0..=256).contains(&n) {
        return None;
    }
    let mut out = Vec::with_capacity(usize::try_from(n).ok()?);
    for _ in 0..n {
        let (&present, rest) = cur.split_first()?;
        cur = rest;
        match present {
            0 => out.push(ItemStack::empty()),
            1 => {
                let id = read_varint(&mut cur).ok()?;
                let count = read_varint(&mut cur).ok()?;
                let added = read_varint(&mut cur).ok()?;
                let removed = read_varint(&mut cur).ok()?;
                if added != 0 || removed != 0 || count <= 0 {
                    return None;
                }
                out.push(ItemStack::new(ids.name(id)?, count));
            }
            _ => return None,
        }
    }
    cur.is_empty().then_some(out)
}

#[cfg(test)]
mod tests {
    /// `ItemContainerContents`' stream form: count, then per slot a presence
    /// byte and item-before-count, with the empty trusted patch. Anything
    /// this cannot read whole — a nested patch, a bad presence byte, bytes
    /// left over, an unknown item — refuses the whole container.
    #[test]
    fn a_container_component_reads_slot_by_slot_or_not_at_all() {
        let ids = crate::player::ItemIds::parse(&serde_json::json!({
            "items": {"minecraft:stone": 1, "minecraft:stick": 974},
            "data_component_types": {}
        }))
        .expect("parsed");
        let ok = [3u8, 1, 1, 5, 0, 0, 0, 1, 0xce, 0x07, 1, 0, 0];
        assert_eq!(
            read_container(&ok, &ids),
            Some(vec![
                ItemStack::new("minecraft:stone", 5),
                ItemStack::empty(),
                ItemStack::one("minecraft:stick"),
            ])
        );
        for bad in [
            &[1u8, 1, 1, 5, 1, 0][..],    // a nested patch entry
            &[1u8, 2][..],                // presence is a boolean
            &[1u8, 1, 1, 5, 0, 0, 9][..], // trailing bytes
            &[1u8, 1, 2, 5, 0, 0][..],    // no item 2
            &[1u8, 1, 1, 0, 0, 0][..],    // a template's count is positive
        ] {
            assert_eq!(read_container(bad, &ids), None, "{bad:?}");
        }
    }

    use super::*;

    /// A zero count is empty however it is reached, and an empty stack never
    /// claims an item. Without this, a slot could hold `stone x0` and read as
    /// occupied.
    #[test]
    fn a_zero_count_is_empty_by_every_route() {
        assert!(ItemStack::empty().is_empty());
        assert!(ItemStack::new("minecraft:stone", 0).is_empty());
        assert!(ItemStack::new("minecraft:stone", -1).is_empty());
        assert!(ItemStack::new("minecraft:air", 5).is_empty());
        // **The constructor must normalise, not merely report.** `is_empty`
        // re-derives its answer from the count, so a constructor that stored
        // `stone x0` verbatim would still *say* empty and every assertion
        // above would pass. The name is what distinguishes the two, and the
        // mutation sweep found this assertion missing.
        assert_eq!(
            ItemStack::new("minecraft:stone", 0).item(),
            "",
            "a zero-count stack is normalised to EMPTY, not stored as itself"
        );
        let mut s = ItemStack::new("minecraft:stone", 1);
        assert!(!s.is_empty());
        s.shrink(1);
        assert!(s.is_empty(), "shrinking to zero empties the stack");
        assert_eq!(s.item(), "", "and it stops naming an item");
    }

    /// `split` is the secondary-click primitive and it must not over-take.
    #[test]
    fn split_takes_at_most_what_is_there() {
        let mut s = ItemStack::new("minecraft:stone", 3);
        let a = s.split(1);
        assert_eq!((a.item(), a.count()), ("minecraft:stone", 1));
        assert_eq!(s.count(), 2);
        // Asking for more than the stack holds takes the stack, not a debt.
        let b = s.split(99);
        assert_eq!(b.count(), 2);
        assert!(s.is_empty());
        // And splitting an empty stack yields an empty stack rather than
        // panicking or producing a negative count.
        assert!(s.split(1).is_empty());
    }

    /// The half a secondary click takes rounds **up**: vanilla's
    /// `(count + 1) / 2`, so 3 gives 2 and 1 gives 1.
    #[test]
    fn the_secondary_click_half_rounds_up() {
        for (count, want) in [(1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (64, 32)] {
            assert_eq!((count + 1) / 2, want, "half of {count}");
        }
    }

    #[test]
    fn sameness_is_by_item_and_two_empties_are_the_same() {
        let stone = ItemStack::one("minecraft:stone");
        let stone2 = ItemStack::new("minecraft:stone", 9);
        let log = ItemStack::one("minecraft:oak_log");
        assert!(ItemStack::is_same_item_same_components(&stone, &stone2));
        assert!(!ItemStack::is_same_item_same_components(&stone, &log));
        assert!(ItemStack::is_same_item_same_components(
            &ItemStack::empty(),
            &ItemStack::empty()
        ));
        assert!(!ItemStack::is_same_item_same_components(
            &stone,
            &ItemStack::empty()
        ));
    }

    /// `grow` past the maximum is *allowed* here — the cap belongs to the
    /// slot, not to the stack. `Slot.safeInsert` is what enforces it, and a
    /// stack that clamped itself would silently swallow the remainder that
    /// `safeInsert` is supposed to hand back.
    #[test]
    fn a_stack_does_not_cap_itself() {
        let mut s = ItemStack::new("minecraft:stone", 64);
        s.grow(10);
        assert_eq!(s.count(), 74, "the slot caps, the stack does not");
        assert_eq!(s.max_stack_size(), 64);
    }
}
