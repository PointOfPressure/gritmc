//! Section light data: the part of `ClientboundLevelChunkWithLightPacket`
//! nothing here has ever compared.
//!
//! # What this module is for
//!
//! Every chunk packet carries light for `getLightSectionCount()` sections —
//! **26 in the overworld** — and until this module existed we classified
//! **none** of them: [`crate::chunk::LightData::default`] is four empty
//! bitsets and two empty array lists, which tells a client "no section of
//! this chunk has any light information at all". A client then lights the
//! chunk itself, which is why the server stayed playable and why nobody
//! noticed.
//!
//! The classification is not a formatting detail. Vanilla puts every one of
//! those 26 sections, for each of the two layers, into exactly one of three
//! states, and which state it lands in is observable on the wire:
//!
//! | state | sky/block mask | empty mask | array sent |
//! |---|---|---|---|
//! | **absent** — the storage has no layer for it | no | no | no |
//! | **empty** — a layer exists and is all-default-zero | no | **yes** | no |
//! | **data** — a layer exists with content | **yes** | no | **2048 bytes** |
//!
//! # The rule, from the bytecode
//!
//! `ClientboundLightUpdatePacketData.<init>` loops `i` over
//! `0..LevelLightEngine.getLightSectionCount()` and calls `prepareSectionData`
//! once per layer per `i` (guarded by the caller's optional changed-masks,
//! which are `null` for a whole-chunk send). `prepareSectionData` is:
//!
//! ```text
//!   DataLayer d = engine.getLayerListener(layer)
//!       .getDataLayerData(SectionPos.of(pos, engine.getMinLightSection() + i));
//!   if (d != null) {
//!       if (d.isEmpty()) emptyMask.set(i);
//!       else { mask.set(i); updates.add(d.copy().getData()); }
//!   }
//! ```
//!
//! Note the index: the **mask bit is `i`**, a dense 0-based index over the
//! light sections, while the **lookup is at `getMinLightSection() + i`**, a
//! signed section-Y. Conflating the two puts every bit five places out in the
//! overworld and is invisible in any dimension whose `minSectionY` is 0.
//!
//! # `isEmpty` is about allocation, not content — and this is the trap
//!
//! `DataLayer.isEmpty()` is `data == null && defaultValue == 0`. It does
//! **not** scan the nibbles. A layer that was materialised and then had every
//! nibble written back to zero reports `isEmpty() == false`, goes into the
//! **data** mask, and puts 2048 zero bytes on the wire — where a layer that
//! was never materialised reports empty and puts nothing there.
//!
//! So a port that answers "is this section all zero?" by looking at the
//! values agrees with vanilla on every section except exactly the ones whose
//! light was computed and came out dark, which is the commonest section in
//! any world with a roof over it. [`DataLayer`] therefore keeps vanilla's
//! representation — a homogeneous default with no backing array, promoted to
//! a real array on first write — rather than a plain `[u8; 2048]`, because
//! the distinction is *load-bearing on the wire* and a flat array cannot
//! represent it.
//!
//! # Sky light lives in this module too
//!
//! The rest of this header is `sky-light-model`'s, unchanged. The two
//! halves share a file because they share a name and nothing else: the
//! section classification above is what the chunk packet carries, and
//! `sky_light` below is the one position at which this server can answer
//! `getBrightness(SKY, pos)` exactly.
//!
//! Sky light, exactly where this server can answer exactly — and refused
//! everywhere else.
//!
//! # The one rule this module implements, and why it stops there
//!
//! `LevelReader.getEffectiveSkyBrightness(pos)` is
//! `getBrightness(SKY, pos) - getSkyDarken()`, and the second half is
//! [`crate::sky`]. The first half is vanilla's light engine: a breadth-first
//! propagation that seeds 15 at the top of every column, loses nothing going
//! straight down through states that propagate sky light, and loses one per
//! step in every other direction.
//!
//! **Only the seed is ported here.** A position whose whole column above it
//! propagates sky light has `getBrightness(SKY, pos) == 15`, and that is not
//! an approximation — it is the engine's initial condition, before any
//! propagation runs, and no propagation can lower it. Every other position
//! needs the breadth-first half, which this server does not have, so
//! [`sky_light`] answers `None` there.
//!
//! `None` is not a zero. A daylight detector handed a zero would read
//! `power = 0` under a roof and `power = 15` inverted, which are both
//! plausible numbers and neither is measured; handed a `None` it declines and
//! writes nothing, which is what this server did before this module existed
//! and is visible as a missing packet rather than a wrong one. The difference
//! matters because `docs/AGENT-BRIEF.md`'s one-way-door rule is about exactly
//! this choice.
//!
//! # `propagatesSkylightDown` is the property, and it is none of its
//! neighbours
//!
//! `BlockBehaviour$BlockStateBase.propagatesSkylightDown()` is a cached
//! boolean, added to `data/vanilla/block_props.json` for this module. It is
//! **not** `!canOcclude` and **not** `!isSolidRender`: water occludes
//! nothing, renders non-solid, and propagates no sky light at all. Reading
//! either of the other two would put a detector at the bottom of a one-block
//! puddle on 15.
//!
//! # Block light is not here
//!
//! Nothing this server implements reads block light yet, and a block-light
//! engine with no consumer is an unfalsifiable claim of exactly the kind
//! `ci/unfalsifiable-falsify.py` exists to catch. `lightEmission` and
//! `lightDampening` are extracted alongside `propagatesSkylightDown` because
//! they come from the same probe and the same table, and they are read by
//! nothing — said here rather than left to be discovered.

use crate::chunk::LightData;

/// `DataLayer.SIZE` — 4096 nibbles, one per block in a section.
pub const DATA_LAYER_SIZE: usize = 2048;

/// `LevelLightEngine.LIGHT_SECTION_PADDING` — one section below the world and
/// one above, which is why the count is `getSectionsCount() + 2`.
pub const LIGHT_SECTION_PADDING: i32 = 1;

/// Which of the two light layers a value belongs to.
///
/// The order matters: it is the order `ClientboundLightUpdatePacketData`
/// writes its masks in, and `LightLayer`'s own ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightLayer {
    /// `LightLayer.SKY`.
    Sky,
    /// `LightLayer.BLOCK`.
    Block,
}

/// Vanilla's `DataLayer`: 4096 nibbles, stored either as a homogeneous
/// default with **no array at all** or as a materialised 2048-byte array.
///
/// The two are not interchangeable — see the module docs. `isEmpty` reads the
/// representation, not the contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLayer {
    /// `DataLayer.data` — `None` until something writes through
    /// [`DataLayer::data_mut`] or [`DataLayer::materialised`].
    data: Option<Vec<u8>>,
    /// `DataLayer.defaultValue`, read only while `data` is `None`.
    default_value: u8,
}

impl Default for DataLayer {
    /// `new DataLayer()` — `this(0)`.
    fn default() -> Self {
        Self::filled(0)
    }
}

impl DataLayer {
    /// `new DataLayer(int)` — homogeneous, no backing array.
    #[must_use]
    pub const fn filled(default_value: u8) -> Self {
        Self {
            data: None,
            default_value,
        }
    }

    /// `new DataLayer(byte[])` — materialised from an existing array.
    ///
    /// # Errors
    /// If `data` is not exactly [`DATA_LAYER_SIZE`] bytes, which is vanilla's
    /// own `IllegalArgumentException`.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        if data.len() != DATA_LAYER_SIZE {
            return Err(format!(
                "DataLayer should be {DATA_LAYER_SIZE} bytes not {}",
                data.len()
            ));
        }
        Ok(Self {
            data: Some(data),
            default_value: 0,
        })
    }

    /// `DataLayer.getIndex` — `y << 8 | z << 4 | x`, each coordinate 0..16.
    #[must_use]
    pub const fn index(x: u8, y: u8, z: u8) -> usize {
        ((y as usize) << 8) | ((z as usize) << 4) | (x as usize)
    }

    /// `DataLayer.get(int, int, int)`.
    #[must_use]
    pub fn get(&self, x: u8, y: u8, z: u8) -> u8 {
        match &self.data {
            None => self.default_value,
            Some(d) => {
                let i = Self::index(x, y, z);
                (d[i >> 1] >> (4 * (i & 1))) & 15
            }
        }
    }

    /// `DataLayer.set(int, int, int, int)` — materialises the array.
    pub fn set(&mut self, x: u8, y: u8, z: u8, value: u8) {
        let i = Self::index(x, y, z);
        let d = self.data_mut();
        let shift = 4 * (i & 1);
        d[i >> 1] = (d[i >> 1] & !(15 << shift)) | ((value & 15) << shift);
    }

    /// `DataLayer.fill(int)` — drops any array and goes homogeneous again.
    ///
    /// This is the operation that can make a materialised layer report empty
    /// once more, and it is the only one that does.
    pub fn fill(&mut self, value: u8) {
        self.default_value = value;
        self.data = None;
    }

    /// `DataLayer.packFilled` — a nibble broadcast into both halves of a byte.
    #[must_use]
    pub const fn pack_filled(value: u8) -> u8 {
        (value & 15) | ((value & 15) << 4)
    }

    /// `DataLayer.getData()` — **materialising**, exactly as vanilla's is.
    ///
    /// Calling this is what turns a homogeneous layer into one that will
    /// never again report [`DataLayer::is_empty`], so it is deliberately
    /// `&mut self` rather than hidden behind a `&self` accessor.
    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        if self.data.is_none() {
            let byte = if self.default_value == 0 {
                0
            } else {
                Self::pack_filled(self.default_value)
            };
            self.data = Some(vec![byte; DATA_LAYER_SIZE]);
        }
        self.data.as_mut().expect("just materialised")
    }

    /// The 2048 bytes this layer puts on the wire, materialising a copy the
    /// way `d.copy().getData()` does — without materialising `self`.
    #[must_use]
    pub fn materialised(&self) -> Vec<u8> {
        match &self.data {
            Some(d) => d.clone(),
            None => vec![
                if self.default_value == 0 {
                    0
                } else {
                    Self::pack_filled(self.default_value)
                };
                DATA_LAYER_SIZE
            ],
        }
    }

    /// `DataLayer.isEmpty()` — `data == null && defaultValue == 0`.
    ///
    /// Read the module docs before "simplifying" this into a scan.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_none() && self.default_value == 0
    }

    /// `DataLayer.isDefinitelyHomogenous()` — `data == null`.
    #[must_use]
    pub const fn is_definitely_homogenous(&self) -> bool {
        self.data.is_none()
    }

    /// `DataLayer.isDefinitelyFilledWith(int)`.
    #[must_use]
    pub const fn is_definitely_filled_with(&self, value: u8) -> bool {
        self.data.is_none() && self.default_value == value
    }
}

/// The geometry `ClientboundLightUpdatePacketData` walks.
///
/// `LevelLightEngine.getLightSectionCount()` is `getSectionsCount() + 2` and
/// `getMinLightSection()` is `getMinSectionY() - 1`; both are reproduced here
/// rather than assumed, because a dimension whose `minSectionY` is 0 hides
/// every off-by-one between the mask index and the section Y.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightGeometry {
    /// `LevelHeightAccessor.getMinSectionY()`.
    pub min_section_y: i32,
    /// `LevelHeightAccessor.getSectionsCount()`.
    pub sections_count: i32,
}

impl LightGeometry {
    /// The overworld: y -64..320, so 24 sections starting at section -4.
    pub const OVERWORLD: Self = Self {
        min_section_y: -4,
        sections_count: 24,
    };

    /// `LevelLightEngine.getLightSectionCount()`.
    #[must_use]
    pub const fn light_section_count(&self) -> i32 {
        self.sections_count + 2 * LIGHT_SECTION_PADDING
    }

    /// `LevelLightEngine.getMinLightSection()`.
    #[must_use]
    pub const fn min_light_section(&self) -> i32 {
        self.min_section_y - LIGHT_SECTION_PADDING
    }

    /// `LevelLightEngine.getMaxLightSection()`.
    #[must_use]
    pub const fn max_light_section(&self) -> i32 {
        self.min_light_section() + self.light_section_count()
    }

    /// The section Y that mask bit `i` refers to.
    #[must_use]
    pub const fn section_y_of_bit(&self, i: i32) -> i32 {
        self.min_light_section() + i
    }
}

/// What a light engine has to answer for the packet to be built: for a given
/// layer and **section Y**, the stored layer, or `None` when the storage has
/// none — which is the "absent" state and is not the same as an empty one.
pub trait SectionLightSource {
    /// `LayerLightEventListener.getDataLayerData(SectionPos)`.
    fn data_layer(&self, layer: LightLayer, section_y: i32) -> Option<&DataLayer>;
}

/// `java.util.BitSet.toLongArray()` — the trailing all-zero words are
/// dropped, which is why an all-clear mask is written as a zero-length array
/// and not as one of zeroes.
fn to_long_array(bits: &[bool]) -> Vec<i64> {
    let mut words = vec![0i64; bits.len().div_ceil(64)];
    for (i, set) in bits.iter().enumerate() {
        if *set {
            words[i / 64] |= 1i64 << (i % 64);
        }
    }
    while words.last() == Some(&0) {
        words.pop();
    }
    words
}

/// Build the packet's light section exactly as
/// `ClientboundLightUpdatePacketData.<init>(pos, engine, null, null)` does.
///
/// The two `null` arguments are the changed-section masks, and they are null
/// for every whole-chunk send: `ClientboundLevelChunkWithLightPacket`'s
/// constructor passes `null, null`. A partial `LIGHT_UPDATE` would pass real
/// ones; we send no such packet (`sendChunk` sends only the chunk packet),
/// so the loop here is unconditional.
#[must_use]
pub fn classify(geometry: LightGeometry, source: &impl SectionLightSource) -> LightData {
    classify_changed(geometry, source, None, None)
}

/// The same, with the two changed-section masks a **partial**
/// `light_update` carries.
///
/// `ClientboundLightUpdatePacketData`'s constructor is one loop over
/// `getLightSectionCount()` with a per-bit filter on each layer:
///
/// ```text
///   if (skyChanged   == null || skyChanged.get(i))   prepareSectionData(SKY,   i, ...);
///   if (blockChanged == null || blockChanged.get(i)) prepareSectionData(BLOCK, i, ...);
/// ```
///
/// So a partial update is the whole-chunk classification **restricted** to
/// the bits the mask sets, and a section the mask does not name appears in
/// neither the data mask nor the empty mask — it is absent, and the client
/// keeps what it had. `None` is the null the whole-chunk send passes.
///
/// The masks are `bool` slices rather than packed words because the caller
/// builds them from a set of section Ys and the packing is
/// [`bitset_words`]'s job, once, at the end.
#[must_use]
pub fn classify_changed(
    geometry: LightGeometry,
    source: &impl SectionLightSource,
    sky_changed: Option<&[bool]>,
    block_changed: Option<&[bool]>,
) -> LightData {
    let n = usize::try_from(geometry.light_section_count()).unwrap_or(0);
    let mut sky_bits = vec![false; n];
    let mut block_bits = vec![false; n];
    let mut empty_sky_bits = vec![false; n];
    let mut empty_block_bits = vec![false; n];
    let mut sky = Vec::new();
    let mut block = Vec::new();

    for i in 0..n {
        let section_y = geometry.section_y_of_bit(i32::try_from(i).unwrap_or(0));
        for (layer, mask, empty, out, changed) in [
            (
                LightLayer::Sky,
                &mut sky_bits,
                &mut empty_sky_bits,
                &mut sky,
                sky_changed,
            ),
            (
                LightLayer::Block,
                &mut block_bits,
                &mut empty_block_bits,
                &mut block,
                block_changed,
            ),
        ] {
            // The per-bit filter. `null` means every section, which is what
            // `ClientboundLevelChunkWithLightPacket` passes; a real mask
            // means this section is skipped entirely and appears in NEITHER
            // the data nor the empty mask.
            if changed.is_some_and(|c| !c.get(i).copied().unwrap_or(false)) {
                continue;
            }
            // `prepareSectionData`, verbatim: a missing layer sets no bit in
            // either mask, an empty one sets only the empty bit, and only a
            // non-empty one contributes an array.
            if let Some(d) = source.data_layer(layer, section_y) {
                if d.is_empty() {
                    empty[i] = true;
                } else {
                    mask[i] = true;
                    out.push(d.materialised());
                }
            }
        }
    }

    LightData {
        sky_mask: to_long_array(&sky_bits),
        block_mask: to_long_array(&block_bits),
        empty_sky_mask: to_long_array(&empty_sky_bits),
        empty_block_mask: to_long_array(&empty_block_bits),
        sky,
        block,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fixed(HashMap<(usize, i32), DataLayer>);

    impl Fixed {
        fn put(&mut self, layer: LightLayer, section_y: i32, d: DataLayer) {
            self.0.insert((layer as usize, section_y), d);
        }
    }

    impl SectionLightSource for Fixed {
        fn data_layer(&self, layer: LightLayer, section_y: i32) -> Option<&DataLayer> {
            self.0.get(&(layer as usize, section_y))
        }
    }

    #[test]
    fn overworld_has_twenty_six_light_sections() {
        // The figure the whole item is named for: `getSectionsCount() + 2`.
        assert_eq!(LightGeometry::OVERWORLD.light_section_count(), 26);
        assert_eq!(LightGeometry::OVERWORLD.min_light_section(), -5);
        assert_eq!(LightGeometry::OVERWORLD.max_light_section(), 21);
    }

    #[test]
    fn mask_bit_zero_is_section_minus_five_not_section_zero() {
        // The off-by-five that no dimension with minSectionY == 0 can show.
        let g = LightGeometry::OVERWORLD;
        assert_eq!(g.section_y_of_bit(0), -5);
        assert_eq!(g.section_y_of_bit(25), 20);
    }

    #[test]
    fn absent_empty_and_data_are_three_distinct_wire_states() {
        let g = LightGeometry::OVERWORLD;
        let mut f = Fixed::default();
        // section -5 absent: nothing inserted at all.
        // section -4 empty: homogeneous zero.
        f.put(LightLayer::Sky, -4, DataLayer::filled(0));
        // section -3 data: homogeneous fifteen is NOT empty.
        f.put(LightLayer::Sky, -3, DataLayer::filled(15));
        let out = classify(g, &f);

        // bit 0 = section -5, bit 1 = -4, bit 2 = -3.
        assert_eq!(out.sky_mask, vec![0b100]);
        assert_eq!(out.empty_sky_mask, vec![0b010]);
        assert_eq!(out.sky.len(), 1);
        assert!(out.sky[0].iter().all(|&b| b == 0xFF));
        // The block layer was never populated: every one of its 26 sections
        // is absent, so both of its masks are zero-length arrays.
        assert!(out.block_mask.is_empty());
        assert!(out.empty_block_mask.is_empty());
        assert!(out.block.is_empty());
    }

    #[test]
    fn a_zeroed_but_materialised_layer_is_not_empty() {
        // The trap the module docs are about. This layer's 4096 nibbles are
        // all zero, and vanilla still sends 2048 zero bytes for it.
        let mut d = DataLayer::default();
        assert!(d.is_empty());
        d.set(3, 4, 5, 7);
        d.set(3, 4, 5, 0);
        assert!(!d.is_empty(), "materialising is one-way short of fill()");
        assert!(d.materialised().iter().all(|&b| b == 0));

        let mut f = Fixed::default();
        f.put(LightLayer::Block, 0, d);
        let out = classify(LightGeometry::OVERWORLD, &f);
        // section 0 is bit 5.
        assert_eq!(out.block_mask, vec![1 << 5]);
        assert!(out.empty_block_mask.is_empty());
        assert_eq!(out.block.len(), 1);
        assert!(out.block[0].iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_takes_a_materialised_layer_back_to_empty() {
        let mut d = DataLayer::default();
        d.set(0, 0, 0, 15);
        assert!(!d.is_empty());
        d.fill(0);
        assert!(
            d.is_empty(),
            "fill() drops the array, which is what makes it empty again"
        );
    }

    #[test]
    fn nibble_addressing_matches_the_java_index() {
        let mut d = DataLayer::default();
        d.set(1, 2, 3, 9);
        assert_eq!(d.get(1, 2, 3), 9);
        // y << 8 | z << 4 | x = 512 + 48 + 1 = 561, so byte 280, high nibble.
        assert_eq!(DataLayer::index(1, 2, 3), 561);
        assert_eq!(d.materialised()[280], 0x90);
        // Nothing else moved.
        assert_eq!(d.materialised().iter().filter(|&&b| b != 0).count(), 1);
    }

    #[test]
    fn homogeneous_fifteen_materialises_to_packed_nibbles() {
        assert_eq!(DataLayer::pack_filled(15), 0xFF);
        assert_eq!(DataLayer::pack_filled(7), 0x77);
        let d = DataLayer::filled(15);
        assert!(d.is_definitely_filled_with(15));
        assert!(!d.is_empty());
        assert_eq!(d.materialised().len(), DATA_LAYER_SIZE);
    }

    #[test]
    fn a_full_mask_is_twenty_six_bits_in_one_word() {
        let g = LightGeometry::OVERWORLD;
        let mut f = Fixed::default();
        for y in g.min_light_section()..g.max_light_section() {
            f.put(LightLayer::Sky, y, DataLayer::filled(15));
            f.put(LightLayer::Block, y, DataLayer::filled(1));
        }
        let out = classify(g, &f);
        assert_eq!(out.sky_mask, vec![(1i64 << 26) - 1]);
        assert_eq!(out.block_mask, vec![(1i64 << 26) - 1]);
        assert_eq!(out.sky.len(), 26);
        assert_eq!(out.block.len(), 26);
        assert!(out.empty_sky_mask.is_empty());
    }

    #[test]
    fn an_all_clear_mask_writes_no_words_at_all() {
        // BitSet.toLongArray() trims trailing zero words, so "nothing set" is
        // a zero-length array on the wire and not a word of zeroes. This is
        // what we send today for all four masks.
        assert!(to_long_array(&[false; 26]).is_empty());
        assert_eq!(to_long_array(&[false; 26]), Vec::<i64>::new());
    }

    /// A partial `light_update` carries the changed-section masks, and a
    /// section the mask does not name appears in NEITHER the data mask nor
    /// the empty one — it is absent, and the client keeps what it had.
    ///
    /// This is the difference between a partial update and a whole-chunk
    /// send, and a port that ignored the masks would send the client a
    /// complete chunk's worth of light on every torch placement while
    /// looking correct in every whole-chunk test.
    #[test]
    fn a_changed_mask_restricts_the_packet_to_the_sections_it_names() {
        let g = LightGeometry::OVERWORLD;
        let mut f = Fixed::default();
        // Three sections with real data and one that is empty.
        f.put(LightLayer::Block, 0, DataLayer::filled(7));
        f.put(LightLayer::Block, 1, DataLayer::filled(9));
        f.put(LightLayer::Block, 2, DataLayer::default());
        f.put(LightLayer::Sky, 0, DataLayer::filled(15));

        let whole = classify(g, &f);
        assert_eq!(whole.block.len(), 2, "sections 0 and 1 carry data");
        assert_eq!(whole.sky.len(), 1);

        // Bit for section Y 1 is `1 - min_light_section` = 6.
        let mut changed = [false; 26];
        changed[6] = true;
        let partial = classify_changed(g, &f, Some(&changed), Some(&changed));
        assert_eq!(
            partial.block.len(),
            1,
            "only section 1's block layer survives the mask"
        );
        assert!(partial.sky.is_empty(), "section 1 has no sky layer");
        assert!(
            partial.empty_block_mask.is_empty(),
            "section 2 is empty but unnamed by the mask, so it is absent, not empty"
        );
        // And the whole-chunk send is exactly the null-mask case.
        assert_eq!(classify_changed(g, &f, None, None), whole);
    }

    #[test]
    fn classifying_an_empty_source_reproduces_todays_default() {
        // Proof that this port subsumes the current behaviour rather than
        // changing it by accident: a source that has nothing produces exactly
        // `LightData::default()`, which is what every chunk we send carries.
        let f = Fixed::default();
        assert_eq!(classify(LightGeometry::OVERWORLD, &f), LightData::default());
    }
}

use crate::interact::BlockPos;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

/// Which states let sky light straight down, from
/// `data/vanilla/block_props.json`.
static ACTIVE: OnceLock<rustmc_behavior::block_props::BlockPropsTable> = OnceLock::new();

/// Load the block table this module reads.
///
/// # Errors
/// If the table is missing or malformed. A caller that cannot load it must
/// say so: see the module docs on why a silent zero is worse than a refusal.
pub fn select(path: &Path) -> io::Result<()> {
    let table =
        rustmc_behavior::block_props::BlockPropsTable::load(path).map_err(io::Error::other)?;
    let _ = ACTIVE.set(table);
    Ok(())
}

/// Whether a state propagates sky light straight down, or `None` if no table
/// was loaded or the state is not in it.
#[must_use]
pub fn propagates_skylight_down(state: &str) -> Option<bool> {
    ACTIVE.get()?.get(state).map(|p| p.propagates_skylight_down)
}

/// `getBrightness(LightLayer.SKY, pos)`, for the positions with a provable
/// answer.
///
/// `Some(15)` when every state from `pos.y + 1` to `top_y` propagates sky
/// light down — the light engine's seed, which nothing can lower.
/// `None` otherwise, including when the block table is absent.
///
/// `top_y` is the dimension's `min_y + height`, exclusive.
///
/// `state_at` is a closure rather than `&dyn Blocks` deliberately: this needs
/// exactly one of that trait's fourteen methods, and taking the whole thing
/// would let a later edit of this function start writing blocks or reading
/// neighbours without anything saying so.
#[must_use]
pub fn sky_light(state_at: impl Fn(BlockPos) -> String, pos: BlockPos, top_y: i32) -> Option<i32> {
    ACTIVE.get()?;
    let (x, y0, z) = pos;
    let mut y = y0.checked_add(1)?;
    while y < top_y {
        let state = state_at((x, y, z));
        if !propagates_skylight_down(&state)? {
            return None;
        }
        y = y.checked_add(1)?;
    }
    Some(15)
}

#[cfg(test)]
mod sky_tests {
    use super::{propagates_skylight_down, select, sky_light};
    use crate::interact::BlockPos;

    fn table() -> bool {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/block_props.json");
        if select(&p).is_err() && super::ACTIVE.get().is_none() {
            // The absence decides here rather than in a `println` cargo
            // swallows: `available` panics in the default mode and names the
            // command that makes the extract.
            return rustmc_data::fixture::available(
                &p,
                "vanilla block_props extract",
                "ci/block-props-golden.sh",
            );
        }
        true
    }

    /// The three properties this module could have been written against, and
    /// the states that tell them apart. Water is the important row: it
    /// occludes nothing and renders non-solid, so `!canOcclude` and
    /// `!isSolidRender` would both put light through it, and it propagates
    /// none.
    #[test]
    fn water_propagates_no_skylight_though_it_occludes_nothing() {
        if !table() {
            return;
        }
        assert_eq!(propagates_skylight_down("minecraft:air"), Some(true));
        assert_eq!(
            propagates_skylight_down("minecraft:water[level=0]"),
            Some(false)
        );
        assert_eq!(propagates_skylight_down("minecraft:stone"), Some(false));
        assert_eq!(propagates_skylight_down("minecraft:glass"), Some(true));
    }

    /// A column that is air everywhere except the y values it names.
    fn column(blocks: &'static [(i32, &'static str)]) -> impl Fn(BlockPos) -> String {
        move |pos| {
            blocks
                .iter()
                .find(|(y, _)| *y == pos.1)
                .map_or("minecraft:air", |(_, s)| *s)
                .to_owned()
        }
    }

    #[test]
    fn an_open_column_seeds_fifteen_and_a_covered_one_refuses() {
        if !table() {
            return;
        }
        let open = column(&[]);
        assert_eq!(sky_light(open, (0, -60, 0), 320), Some(15));
        let covered = column(&[(0, "minecraft:stone")]);
        assert_eq!(sky_light(covered, (0, -60, 0), 320), None);
        // Glass overhead still seeds 15 — it propagates.
        let glazed = column(&[(0, "minecraft:glass")]);
        assert_eq!(sky_light(glazed, (0, -60, 0), 320), Some(15));
    }
}
