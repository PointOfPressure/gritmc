//! `play/level_chunk_with_light` (clientbound `0x2d`) — encode and decode.
//!
//! # Where the layout came from
//!
//! Not from a field list. Every field below was settled against **4.6 MB of
//! real bytes**: 117 `level_chunk_with_light` bodies captured from a vanilla
//! 26.2 server at seed 31415926 by `CHUNK_DUMP=... ci/regdump.py`, and the
//! test at the bottom re-encodes all of them and demands the original bytes
//! back. Two fields would have been wrong from reading alone:
//!
//! - **`fluidCount`.** `LevelChunkSection.write` writes *two* shorts, not
//!   one — `nonEmptyBlockCount` then `fluidCount` — and `getSerializedSize`
//!   opens with `iconst_4` to match. Missing it desynced every section and
//!   made the bottom section decode as 4096 blocks of air, which is
//!   self-contradictory but perfectly parseable for a while.
//! - **The long arrays carry no length.** `PalettedContainer$Data.write`
//!   ends in `writeFixedSizeLongArray`, which writes the longs and nothing
//!   else; the count is implied by `bitsPerEntry`. Reading a varint there
//!   ate the first byte of the data and still "worked" for one section.
//!
//! # Layout
//!
//! ```text
//!   i32   chunkX
//!   i32   chunkZ
//!   varint heightmapCount, then per heightmap:
//!       varint kind
//!       varint longCount, longCount * i64
//!   varint sectionBytes, then that many bytes of packed sections
//!   varint blockEntityCount, then per entity: u8 packedXZ, i16 y, varint type, NBT
//!   4 * (varint longCount, longCount * i64)      sky/block/emptySky/emptyBlock masks
//!   varint skyArrays,   per: varint len, len bytes
//!   varint blockArrays, per: varint len, len bytes
//! ```
//!
//! and each section, `SECTIONS` of them bottom-up:
//!
//! ```text
//!   i16 nonEmptyBlockCount
//!   i16 fluidCount
//!   PalettedContainer states   (4096 cells)
//!   PalettedContainer biomes   (  64 cells)
//! ```

use rustmc_proto::varint::{read_varint, write_varint};
use rustmc_proto::wire::{read_i32, read_i64, read_u8, write_i32, write_i64, write_u8};
use std::io::{self, Read, Write};

/// Overworld: `minY -64`, height 384, so 24 sections. The packet carries one
/// entry per section of the *dimension*, with no count of its own — a
/// decoder that assumes the wrong number silently reads into the block
/// entities, so this is a parameter rather than a constant at the use site.
pub const OVERWORLD_SECTIONS: usize = 24;

/// Which of the three palette regimes a container is in.
///
/// The regime is not on the wire: it is implied by `bitsPerEntry` together
/// with *which* container this is, and the two containers have different
/// thresholds. From `Strategy$1.getConfigurationForBitCount` (block states)
/// and `Strategy$2` (biomes), both `tableswitch`es:
///
/// ```text
///   states  0 -> single     1,2,3,4 -> linear(4)   5,6,7,8 -> hashmap   >8 -> global
///   biomes  0 -> single     1 -> linear(1)  2 -> linear(2)  3 -> linear(3)   >3 -> global
/// ```
///
/// Linear and hashmap palettes are identical on the wire (a varint count then
/// varint entries), so only three cases matter here. Note the states row: a
/// palette needing 1, 2 or 3 bits is still written with **`bitsPerEntry` 4**,
/// which is why a block container never appears below 4 bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Palette {
    /// `bitsPerEntry == 0`: one value for the whole container, no data.
    Single(i32),
    /// A varint count then that many varint registry ids.
    Indirect(Vec<i32>),
    /// `Configuration$Global`: no palette on the wire, values are registry
    /// ids packed directly.
    Direct,
}

/// The two kinds of container, which differ in cell count and in where the
/// direct threshold sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// 4096 cells, indirect up to 8 bits.
    BlockStates,
    /// 64 cells, indirect up to 3 bits.
    Biomes,
}

impl ContainerKind {
    const fn cells(self) -> usize {
        match self {
            Self::BlockStates => 4096,
            Self::Biomes => 64,
        }
    }
    const fn max_indirect_bits(self) -> u8 {
        match self {
            Self::BlockStates => 8,
            Self::Biomes => 3,
        }
    }
    /// The smallest `bitsPerEntry` vanilla will use for an indirect palette
    /// of this kind. Block states floor at four; biomes do not.
    const fn min_indirect_bits(self) -> u8 {
        match self {
            Self::BlockStates => 4,
            Self::Biomes => 1,
        }
    }
}

/// How many longs `bitsPerEntry` needs for `cells` entries.
///
/// Values do **not** straddle a long: `64 / bits` of them fit and the
/// remaining high bits are left zero. That is what makes the length
/// derivable, and it is why `writeFixedSizeLongArray` can omit it.
#[must_use]
pub fn long_count(bits: u8, cells: usize) -> usize {
    if bits == 0 {
        return 0;
    }
    let per_long = 64 / bits as usize;
    cells.div_ceil(per_long)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PalettedContainer {
    pub bits: u8,
    pub palette: Palette,
    /// Raw packed longs, exactly `long_count(bits, kind.cells())` of them.
    pub data: Vec<i64>,
}

impl PalettedContainer {
    /// Build the container vanilla would build for these cell values.
    ///
    /// Mirrors `Strategy.getConfigurationForPaletteSize`: count the distinct
    /// values, take `ceillog2` of that, and map it through the kind's
    /// threshold table. The palette order is **first appearance in cell
    /// order**, which is *not* what vanilla produces — vanilla's order is a
    /// function of the order blocks were set during generation, which the
    /// final grid does not record. So this is semantically equivalent, not
    /// byte-identical, and the round-trip test does not go through here.
    #[must_use]
    pub fn from_values(kind: ContainerKind, values: &[i32]) -> Self {
        assert_eq!(values.len(), kind.cells(), "wrong cell count for {kind:?}");
        let mut palette: Vec<i32> = Vec::new();
        for &v in values {
            if !palette.contains(&v) {
                palette.push(v);
            }
        }
        if palette.len() == 1 {
            return Self {
                bits: 0,
                palette: Palette::Single(palette[0]),
                data: Vec::new(),
            };
        }
        // ceillog2 of the palette size, floored at the kind's minimum.
        let needed = u8::try_from(palette.len().next_power_of_two().trailing_zeros())
            .expect("palette bits fit a byte");
        let mut bits = needed.max(kind.min_indirect_bits());
        // One bit narrower than the palette needs, so its top index aliases to
        // zero. The length field still agrees with `bits`, so the packet parses
        // and only a value comparison notices. `rustmc_data::sabotage`.
        if rustmc_data::sabotage::fires(rustmc_data::sabotage::Arm::Palette) && bits > 1 {
            bits -= 1;
        }
        if bits > kind.max_indirect_bits() {
            // Direct: values are registry ids, packed at the global width.
            // NOT EXERCISED by the capture — see the module test.
            let bits = direct_bits(kind, values);
            let data = pack(values, bits, kind.cells());
            return Self {
                bits,
                palette: Palette::Direct,
                data,
            };
        }
        let indices: Vec<i32> = values
            .iter()
            .map(|v| {
                i32::try_from(palette.iter().position(|p| p == v).expect("in palette"))
                    .expect("palette index fits i32")
            })
            .collect();
        let data = pack(&indices, bits, kind.cells());
        Self {
            bits,
            palette: Palette::Indirect(palette),
            data,
        }
    }

    /// Unpack back to one value per cell.
    #[must_use]
    pub fn values(&self, kind: ContainerKind) -> Vec<i32> {
        match &self.palette {
            Palette::Single(v) => vec![*v; kind.cells()],
            Palette::Indirect(p) => unpack(&self.data, self.bits, kind.cells())
                .into_iter()
                .map(|i| p.get(i as usize).copied().unwrap_or(0))
                .collect(),
            Palette::Direct => unpack(&self.data, self.bits, kind.cells()),
        }
    }

    fn write(&self, out: &mut impl Write, kind: ContainerKind) -> io::Result<()> {
        write_u8(out, self.bits)?;
        match &self.palette {
            Palette::Single(v) => write_varint(out, *v)?,
            Palette::Indirect(p) => {
                write_varint(out, i32::try_from(p.len()).map_err(io::Error::other)?)?;
                for &e in p {
                    write_varint(out, e)?;
                }
            }
            Palette::Direct => {}
        }
        // `writeFixedSizeLongArray`: the longs and nothing else.
        let want = long_count(self.bits, kind.cells());
        if self.data.len() != want {
            return Err(io::Error::other(format!(
                "container has {} longs, {} bits over {} cells needs {want}",
                self.data.len(),
                self.bits,
                kind.cells()
            )));
        }
        for &v in &self.data {
            write_i64(out, v)?;
        }
        Ok(())
    }

    fn read(r: &mut impl Read, kind: ContainerKind) -> io::Result<Self> {
        let bits = read_u8(r)?;
        let palette = if bits == 0 {
            Palette::Single(read_varint(r)?)
        } else if bits <= kind.max_indirect_bits() {
            let n = read_varint(r)?;
            if !(0..=1 << 16).contains(&n) {
                return Err(io::Error::other(format!("palette length {n}")));
            }
            let mut p = Vec::with_capacity(n as usize);
            for _ in 0..n {
                p.push(read_varint(r)?);
            }
            Palette::Indirect(p)
        } else {
            Palette::Direct
        };
        let mut data = Vec::with_capacity(long_count(bits, kind.cells()));
        for _ in 0..long_count(bits, kind.cells()) {
            data.push(read_i64(r)?);
        }
        Ok(Self {
            bits,
            palette,
            data,
        })
    }
}

/// The width a direct palette packs at: enough bits for the largest id
/// present. Vanilla uses the registry's full width
/// (`Configuration$Global(globalPaletteBitsInMemory, ...)`), which this
/// cannot know without the registry, so it is passed the values instead.
///
/// **Unverified against vanilla bytes**: no container in the 117-chunk
/// capture reached a direct palette, so this width is read from the bytecode
/// and never confirmed on the wire.
fn direct_bits(kind: ContainerKind, values: &[i32]) -> u8 {
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let needed = 32 - (max as u32).leading_zeros();
    u8::try_from(needed.max(u32::from(kind.max_indirect_bits()) + 1)).expect("fits")
}

/// Re-pack a container's own cell values at its own width, mapping back
/// through its palette.
///
/// The round-trip test carries the packed longs through untouched, so on its
/// own it would pass even if the bit layout were misunderstood. This goes
/// through the values and back, which is what makes the layout — seven 9-bit
/// values to a long with the top bit unused, and no straddling — actually
/// tested rather than merely copied.
#[must_use]
pub fn repack(values: &[i32], container: &PalettedContainer, kind: ContainerKind) -> Vec<i64> {
    match &container.palette {
        Palette::Single(_) => Vec::new(),
        Palette::Indirect(p) => {
            let indices: Vec<i32> = values
                .iter()
                .map(|v| i32::try_from(p.iter().position(|e| e == v).unwrap_or(0)).unwrap_or(0))
                .collect();
            pack(&indices, container.bits, kind.cells())
        }
        Palette::Direct => pack(values, container.bits, kind.cells()),
    }
}

fn pack(values: &[i32], bits: u8, cells: usize) -> Vec<i64> {
    let per_long = 64 / bits as usize;
    let mut out = vec![0i64; long_count(bits, cells)];
    for (i, &v) in values.iter().enumerate() {
        let (slot, off) = (i / per_long, (i % per_long) * bits as usize);
        out[slot] |= ((v as i64) & ((1i64 << bits) - 1)) << off;
    }
    out
}

fn unpack(data: &[i64], bits: u8, cells: usize) -> Vec<i32> {
    if bits == 0 {
        return vec![0; cells];
    }
    let per_long = 64 / bits as usize;
    let mask = (1u64 << bits) - 1;
    (0..cells)
        .map(|i| {
            let (slot, off) = (i / per_long, (i % per_long) * bits as usize);
            data.get(slot)
                .map_or(0, |&l| i32::try_from((l as u64 >> off) & mask).unwrap_or(0))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Blocks that are not air, per `LevelChunkSection.nonEmptyBlockCount`.
    pub non_empty_blocks: i16,
    /// `LevelChunkSection.fluidCount` — the field a field list does not show
    /// you and whose absence desyncs the whole buffer.
    pub fluid_count: i16,
    pub states: PalettedContainer,
    pub biomes: PalettedContainer,
}

impl Section {
    fn write(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(&self.non_empty_blocks.to_be_bytes())?;
        out.write_all(&self.fluid_count.to_be_bytes())?;
        self.states.write(out, ContainerKind::BlockStates)?;
        self.biomes.write(out, ContainerKind::Biomes)
    }
    fn read(r: &mut impl Read) -> io::Result<Self> {
        let mut b = [0u8; 2];
        r.read_exact(&mut b)?;
        let non_empty_blocks = i16::from_be_bytes(b);
        r.read_exact(&mut b)?;
        let fluid_count = i16::from_be_bytes(b);
        Ok(Self {
            non_empty_blocks,
            fluid_count,
            states: PalettedContainer::read(r, ContainerKind::BlockStates)?,
            biomes: PalettedContainer::read(r, ContainerKind::Biomes)?,
        })
    }
}

/// One heightmap. `kind` is the `Heightmap.Types` ordinal; the capture
/// carries 1, 4 and 5 — `WORLD_SURFACE`, `MOTION_BLOCKING` and
/// `MOTION_BLOCKING_NO_LEAVES`, the three with `sendToClient`.
///
/// Nine bits a column, seven to a long, 256 columns — 37 longs, which is what
/// every heightmap in the capture is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heightmap {
    pub kind: i32,
    pub data: Vec<i64>,
}

/// The light half of the packet. Kept as raw arrays: this encoder's job is
/// the framing, and inventing light values is a separate problem with its own
/// oracle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LightData {
    pub sky_mask: Vec<i64>,
    pub block_mask: Vec<i64>,
    pub empty_sky_mask: Vec<i64>,
    pub empty_block_mask: Vec<i64>,
    /// One 2048-byte nibble array per bit set in `sky_mask`.
    pub sky: Vec<Vec<u8>>,
    pub block: Vec<Vec<u8>>,
}

impl LightData {
    /// The light half of a packet: four bitsets, then the two array lists.
    ///
    /// `ClientboundLightUpdatePacketData.write` and the tail of
    /// `ClientboundLevelChunkWithLightPacket`'s writer are the **same six
    /// writes in the same order** — the chunk packet embeds this object — so
    /// this is one function and both callers use it. A second transcription
    /// is a second thing to get out of order.
    ///
    /// # Errors
    /// If a length does not fit an `i32`.
    pub fn write(&self, o: &mut Vec<u8>) -> io::Result<()> {
        for m in [
            &self.sky_mask,
            &self.block_mask,
            &self.empty_sky_mask,
            &self.empty_block_mask,
        ] {
            write_longs(o, m)?;
        }
        for arrays in [&self.sky, &self.block] {
            write_varint(o, i32::try_from(arrays.len()).map_err(io::Error::other)?)?;
            for a in arrays {
                write_varint(o, i32::try_from(a.len()).map_err(io::Error::other)?)?;
                o.write_all(a)?;
            }
        }
        Ok(())
    }

    /// The inverse.
    ///
    /// # Errors
    /// On a malformed body.
    pub fn read(r: &mut io::Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self {
            sky_mask: read_longs(r)?,
            block_mask: read_longs(r)?,
            empty_sky_mask: read_longs(r)?,
            empty_block_mask: read_longs(r)?,
            sky: read_arrays(r)?,
            block: read_arrays(r)?,
        })
    }
}

/// `play/light_update` (clientbound), `ClientboundLightUpdatePacket`.
///
/// From the bytecode, and it is short enough to state whole:
///
/// ```text
///   write(FriendlyByteBuf):
///     buf.writeVarInt(this.x)
///     buf.writeVarInt(this.z)
///     this.lightData.write(buf)
/// ```
///
/// So the coordinates are **varints** here where the chunk packet writes them
/// as `i32`s, and the payload after them is byte-for-byte the chunk packet's
/// light tail. The constructor is
/// `(ChunkPos, LevelLightEngine, BitSet skyChanged, BitSet blockChanged)` and
/// the two bitsets are the filters `ChunkHolder` accumulated; a section
/// neither names appears in **neither** the data mask nor the empty mask and
/// the client keeps what it had. See [`crate::light::classify_changed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightUpdate {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub light: LightData,
}

impl LightUpdate {
    /// The packet body, without the leading packet id.
    ///
    /// # Errors
    /// If a length does not fit an `i32`.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut o = Vec::new();
        write_varint(&mut o, self.chunk_x)?;
        write_varint(&mut o, self.chunk_z)?;
        self.light.write(&mut o)?;
        Ok(o)
    }

    /// Parse a body, refusing trailing bytes for the same reason
    /// [`LevelChunkWithLight::decode`] does.
    ///
    /// # Errors
    /// On a malformed body or one with bytes left over.
    pub fn decode(body: &[u8]) -> io::Result<Self> {
        let mut r = io::Cursor::new(body);
        let chunk_x = read_varint(&mut r)?;
        let chunk_z = read_varint(&mut r)?;
        let light = LightData::read(&mut r)?;
        let used = usize::try_from(r.position()).map_err(io::Error::other)?;
        if used != body.len() {
            return Err(io::Error::other(format!(
                "{} trailing bytes after the packet",
                body.len() - used
            )));
        }
        Ok(Self {
            chunk_x,
            chunk_z,
            light,
        })
    }
}

/// A block entity, kept as its raw NBT bytes — this packet only has to carry
/// them intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntity {
    /// `(x & 15) << 4 | (z & 15)`.
    pub packed_xz: u8,
    pub y: i16,
    pub kind: i32,
    pub nbt: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelChunkWithLight {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub heightmaps: Vec<Heightmap>,
    pub sections: Vec<Section>,
    pub block_entities: Vec<BlockEntity>,
    pub light: LightData,
}

fn write_longs(out: &mut impl Write, v: &[i64]) -> io::Result<()> {
    write_varint(out, i32::try_from(v.len()).map_err(io::Error::other)?)?;
    for &l in v {
        write_i64(out, l)?;
    }
    Ok(())
}

fn read_longs(r: &mut impl Read) -> io::Result<Vec<i64>> {
    let n = read_varint(r)?;
    if !(0..=1 << 20).contains(&n) {
        return Err(io::Error::other(format!("long array length {n}")));
    }
    (0..n).map(|_| read_i64(r)).collect()
}

impl LevelChunkWithLight {
    /// The packet body, without the leading packet id.
    ///
    /// # Errors
    /// If a container's long array does not match its `bitsPerEntry`, or a
    /// length does not fit an `i32`.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut o = Vec::new();
        write_i32(&mut o, self.chunk_x)?;
        write_i32(&mut o, self.chunk_z)?;
        write_varint(
            &mut o,
            i32::try_from(self.heightmaps.len()).map_err(io::Error::other)?,
        )?;
        for h in &self.heightmaps {
            write_varint(&mut o, h.kind)?;
            write_longs(&mut o, &h.data)?;
        }
        let mut sections = Vec::new();
        for s in &self.sections {
            s.write(&mut sections)?;
        }
        write_varint(
            &mut o,
            i32::try_from(sections.len()).map_err(io::Error::other)?,
        )?;
        o.extend_from_slice(&sections);

        write_varint(
            &mut o,
            i32::try_from(self.block_entities.len()).map_err(io::Error::other)?,
        )?;
        for be in &self.block_entities {
            write_u8(&mut o, be.packed_xz)?;
            o.write_all(&be.y.to_be_bytes())?;
            write_varint(&mut o, be.kind)?;
            o.write_all(&be.nbt)?;
        }

        self.light.write(&mut o)?;
        Ok(o)
    }

    /// Parse a body. `sections` is the dimension's section count — it is not
    /// on the wire.
    ///
    /// # Errors
    /// On a malformed body, or if `body` has trailing bytes: this refuses a
    /// partial parse rather than returning something plausible, because a
    /// packet that decodes with bytes left over is one whose layout is not
    /// understood.
    pub fn decode(body: &[u8], sections: usize) -> io::Result<Self> {
        let mut r = io::Cursor::new(body);
        let chunk_x = read_i32(&mut r)?;
        let chunk_z = read_i32(&mut r)?;
        let hn = read_varint(&mut r)?;
        if !(0..=64).contains(&hn) {
            return Err(io::Error::other(format!("heightmap count {hn}")));
        }
        let mut heightmaps = Vec::with_capacity(hn as usize);
        for _ in 0..hn {
            let kind = read_varint(&mut r)?;
            heightmaps.push(Heightmap {
                kind,
                data: read_longs(&mut r)?,
            });
        }
        let blen = read_varint(&mut r)?;
        let start = usize::try_from(r.position()).map_err(io::Error::other)?;
        let end = start
            .checked_add(usize::try_from(blen).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("section length overflow"))?;
        let buf = body
            .get(start..end)
            .ok_or_else(|| io::Error::other("section buffer past end of packet"))?;
        let mut br = io::Cursor::new(buf);
        let mut secs = Vec::with_capacity(sections);
        for _ in 0..sections {
            secs.push(Section::read(&mut br)?);
        }
        if usize::try_from(br.position()).map_err(io::Error::other)? != buf.len() {
            return Err(io::Error::other(format!(
                "section buffer: consumed {} of {}",
                br.position(),
                buf.len()
            )));
        }
        r.set_position(end as u64);

        let ben = read_varint(&mut r)?;
        if !(0..=1 << 16).contains(&ben) {
            return Err(io::Error::other(format!("block entity count {ben}")));
        }
        let mut block_entities = Vec::with_capacity(ben as usize);
        for _ in 0..ben {
            let packed_xz = read_u8(&mut r)?;
            let mut yb = [0u8; 2];
            r.read_exact(&mut yb)?;
            let kind = read_varint(&mut r)?;
            // The NBT is kept verbatim: this packet only has to carry it
            // intact, and re-serialising a parsed tree would risk changing
            // bytes the round-trip test is there to protect. Parsing is still
            // how the length is found — there is no length prefix.
            let at = usize::try_from(r.position()).map_err(io::Error::other)?;
            let mut probe = io::Cursor::new(&body[at..]);
            rustmc_world::nbt::read_network_tag(&mut probe)?;
            let used = usize::try_from(probe.position()).map_err(io::Error::other)?;
            let nbt = body[at..at + used].to_vec();
            r.set_position((at + used) as u64);
            block_entities.push(BlockEntity {
                packed_xz,
                y: i16::from_be_bytes(yb),
                kind,
                nbt,
            });
        }

        let light = LightData::read(&mut r)?;
        let used = usize::try_from(r.position()).map_err(io::Error::other)?;
        if used != body.len() {
            return Err(io::Error::other(format!(
                "{} trailing bytes after the packet",
                body.len() - used
            )));
        }
        Ok(Self {
            chunk_x,
            chunk_z,
            heightmaps,
            sections: secs,
            block_entities,
            light,
        })
    }
}

fn read_arrays(r: &mut io::Cursor<&[u8]>) -> io::Result<Vec<Vec<u8>>> {
    let n = read_varint(r)?;
    if !(0..=4096).contains(&n) {
        return Err(io::Error::other(format!("light array count {n}")));
    }
    (0..n)
        .map(|_| {
            let k = read_varint(r)?;
            if !(0..=1 << 16).contains(&k) {
                return Err(io::Error::other(format!("light array length {k}")));
            }
            let mut v = vec![0u8; k as usize];
            r.read_exact(&mut v)?;
            Ok(v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{LightData, LightUpdate};

    fn data() -> LightData {
        LightData {
            // 26 bits, so two words — the overworld's light section count,
            // which is the case a one-word packing would get wrong.
            sky_mask: vec![0b101, 0],
            block_mask: vec![0b10, 0],
            empty_sky_mask: vec![0b1000, 0],
            empty_block_mask: vec![],
            sky: vec![vec![0xab; 2048], vec![0xcd; 2048]],
            block: vec![vec![0x0f; 2048]],
        }
    }

    /// `ClientboundLightUpdatePacket.write` is two varints and the shared
    /// light body, and this is the check that it is written and read in that
    /// order. `decode` refuses trailing bytes, so a pass means every byte is
    /// accounted for by a named field.
    #[test]
    fn a_light_update_round_trips() {
        let p = LightUpdate {
            chunk_x: -3,
            chunk_z: 129,
            light: data(),
        };
        let body = p.encode().expect("encodes");
        assert_eq!(LightUpdate::decode(&body).expect("decodes"), p);
    }

    /// The coordinates are **varints** in this packet and `i32`s in the chunk
    /// packet, so a negative x must survive: a port that reused the chunk
    /// packet's writer would produce four bytes where vanilla writes a
    /// zig-zag-free varint and the client would read the masks out of the
    /// coordinate.
    #[test]
    fn the_coordinates_are_varints_not_fixed_width() {
        let p = LightUpdate {
            chunk_x: 1,
            chunk_z: 1,
            light: LightData::default(),
        };
        let body = p.encode().expect("encodes");
        assert_eq!(&body[..2], &[1, 1], "a small chunk coordinate is one byte");
        let far = LightUpdate {
            chunk_x: -1,
            chunk_z: 0,
            light: LightData::default(),
        };
        let body = far.encode().expect("encodes");
        assert_eq!(
            &body[..5],
            &[0xff, 0xff, 0xff, 0xff, 0x0f],
            "-1 is a five-byte varint, not a four-byte int"
        );
        assert_eq!(LightUpdate::decode(&body).expect("decodes"), far);
    }

    /// A body with anything left over is refused rather than accepted as a
    /// plausible parse.
    #[test]
    fn a_trailing_byte_is_refused() {
        let p = LightUpdate {
            chunk_x: 0,
            chunk_z: 0,
            light: data(),
        };
        let mut body = p.encode().expect("encodes");
        body.push(0);
        assert!(LightUpdate::decode(&body).is_err());
    }
}
