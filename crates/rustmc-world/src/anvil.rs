//! Anvil region file IO. Both directions are live: the read side is the
//! differ's foundation, and the write side is what `worldgen-dump` and the
//! server's save path put on disk.
//!
//! **The header used to say "the writer is wave-1 work", and that was stale
//! for long enough to mislead a survey of this repo into reporting chunk
//! support as read-only.** It is corrected here rather than deleted, because
//! the same sentence appeared in `nbt.rs` and both were believed.
//!
//! Format reference: `net.minecraft.world.level.chunk.storage.RegionFile` —
//! an 8 KiB header (1024 offsets then 1024 timestamps, both big-endian u32),
//! then per-chunk `[u32 len][u8 scheme][payload]` on 4 KiB sector
//! boundaries. The offset entry packs `sector << 8 | count`.
//!
//! Scheme ids come from `RegionFileVersion`'s class initialiser: 1 `gzip`,
//! 2 `deflate`, 3 `none`, 4 `lz4`, 127 `custom`, with `DEFAULT` bound to
//! `VERSION_DEFLATE`. We read 1, 2 and 3 and write 2.
//!
//! **Byte-compatible with vanilla is a claim about what vanilla accepts, and
//! only vanilla can settle it.** A round trip through our own reader cannot:
//! a reader and a writer can agree on the same wrong thing, and this pair
//! was written together. `ci/t2-vanilla-loads-ours.sh` is the test that
//! actually closes it — it hands a region this module wrote to a real
//! vanilla server and asks vanilla what is in it.

use crate::nbt::{self, Nbt};
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;

/// One parsed chunk column, reduced to exactly what block-exactness diffing
/// needs: palette strings + indices. Full ChunkAccess-backed loading arrives
/// with wave 1; keeping the differ's model this small keeps it obviously
/// correct.
#[derive(Debug)]
pub struct ChunkColumn {
    pub x: i32,
    pub z: i32,
    pub status: String,
    pub data_version: i32,
    /// Sorted ascending by section y.
    pub sections: Vec<Section>,
    /// The chunk's `block_entities` list, each entry exactly as it appears in
    /// the file: a compound carrying at least `id`, `x`, `y` and `z`, plus
    /// whatever the type stores.
    ///
    /// Kept as raw NBT rather than a typed enum because this crate is the
    /// Anvil *format* and has no business knowing what a chest is; the server
    /// crate interprets these. Order is the file's own, which is vanilla's
    /// insertion order over the chunk's block-entity map, and is preserved
    /// rather than sorted so a round trip is byte-stable.
    pub block_entities: Vec<crate::nbt::Nbt>,
    /// The four heightmaps, by vanilla's saved names (`WORLD_SURFACE`,
    /// `OCEAN_FLOOR`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`), each
    /// 256 world-y values in `z * 16 + x` order.
    ///
    /// Empty means "this column has none", and then nothing is written —
    /// which is what every reader here did before, and is still what a
    /// column built from a partial pipeline should say. Vanilla writes them
    /// for real, so a chunk that carries them is directly diffable against
    /// a vanilla region: the heightmaps remember stage history the blocks
    /// have forgotten, which is exactly how a mis-sited disk was traced to
    /// a pre-carve terrain difference.
    pub heightmaps: BTreeMap<String, Vec<i32>>,
    /// The structures that **start** in this chunk, sorted by id.
    ///
    /// Vanilla records these in the chunk itself, which makes a generated
    /// region a free oracle for the structure stage: which chunks a
    /// structure starts in, and the bounding box of every piece, without
    /// running a JVM harness. A chunk that merely *overlaps* a structure
    /// carries a reference to the start's chunk instead, not a start.
    /// `isLightOn` — vanilla writes `1` for a chunk whose light is converged
    /// and saved, and the client (and any comparison) may only trust the
    /// nibble arrays when it is set.
    pub is_light_on: bool,
    /// The saved light nibbles, per section Y, for **every** section the file
    /// carries — including the ones with no `block_states`, which
    /// [`Section`] skips because they hold nothing but light.
    ///
    /// 2048 bytes each, two nibbles per byte, low nibble first, indexed
    /// `y << 8 | z << 4 | x` — `DataLayer`'s own order.
    pub light: BTreeMap<i8, SectionLight>,
    pub structure_starts: Vec<StructureStart>,
    /// `structures.References` — for each structure id, the packed
    /// `ChunkPos.asLong` of every start whose **union** box reaches this
    /// chunk.
    ///
    /// This is the second free oracle in the same NBT, and a sharper one
    /// than the starts: `StructureManager#startsForStructure` walks exactly
    /// this set and hands each start to `placeInChunk` in iteration order,
    /// sharing one `RandomSource`. So the set's *membership* decides which
    /// pieces a chunk writes and, through
    /// [`rustmc_worldgen::fastutil_long_set`], the order they are written
    /// in. Vanilla saves it; nothing was comparing it.
    ///
    /// Order here is the file's, which is the `LongOpenHashSet`'s own table
    /// order — not insertion order and not sorted. It is kept as written
    /// rather than sorted for that reason.
    pub structure_references: BTreeMap<String, Vec<i64>>,
    /// Every top-level key this struct does not model, **verbatim**.
    ///
    /// A `minecraft:full` chunk vanilla writes carries fifteen keys and this
    /// struct models eight of them; the other seven — `LastUpdate`,
    /// `InhabitedTime`, `isLightOn`, `block_ticks`, `fluid_ticks`,
    /// `PostProcessing` and `structures` — used to be **written back as
    /// constants**, so a real vanilla chunk read in here and saved again came
    /// out with its tick queues emptied, its light flag cleared and its
    /// structure references deleted. Nothing could see it: the only test of
    /// the writer against vanilla data compared palettes and block indices.
    ///
    /// Keeping them raw is the same decision [`Self::block_entities`] made and
    /// [`crate::playerdat::PlayerData::extra`] makes: the format crate carries
    /// what it does not interpret rather than dropping it. `structures` is in
    /// here rather than rebuilt from [`Self::structure_starts`] because those
    /// are a *reading* of the compound for the worldgen oracle and model only
    /// the fields that oracle needs — re-emitting from them would lose the
    /// rest.
    ///
    /// A column built by the generator rather than read from disk starts from
    /// [`generated_extra`].
    pub extra: BTreeMap<String, Nbt>,
}

/// The non-modelled top-level keys a freshly generated `minecraft:full` chunk
/// carries, at the values vanilla writes for a chunk nothing has happened in.
///
/// Read off a real 26.2 chunk rather than assumed. `PostProcessing` is
/// twenty-four empty lists, one per section, and **not** an empty outer list:
/// `ProtoChunk#postProcessGeneration` applies the marks and clears them, so a
/// chunk saved at `minecraft:full` always has this shape and never any
/// content.
///
/// `isLightOn` is absent rather than 0, and the reason is worth reading before
/// adding it back — see the comment where it used to be.
#[must_use]
pub fn generated_extra() -> BTreeMap<String, Nbt> {
    let mut m = BTreeMap::new();
    m.insert("LastUpdate".into(), Nbt::Long(0));
    m.insert("InhabitedTime".into(), Nbt::Long(0));
    // `isLightOn` IS DELIBERATELY ABSENT, and it used to be `Byte(0)`.
    //
    // `SerializableChunkData.copyOf` writes the key **only when
    // `lightCorrect` is true**:
    //
    // ```text
    //   337: getfield  lightCorrect:Z
    //   340: ifeq      350            <- skips the write entirely
    //   344: ldc       "isLightOn"
    // ```
    //
    // and the read side is `getBooleanOr("isLightOn", false)`, so an absent
    // key and an explicit 0 load identically. They are not identical on
    // DISK, and this crate's whole claim is about the bytes: a chunk this
    // server generated has light this server did not compute, which is
    // `lightCorrect == false`, and vanilla writes nothing for that.
    //
    // The 754-chunk byte-identity golden could never have caught this and
    // still cannot, by construction: every chunk it compares comes off a real
    // vanilla disk with `lightCorrect` true, so `extra` carries `isLightOn: 1`
    // through the passthrough and this function's value is never reached.
    // `generated_extra_omits_the_light_flag_vanilla_omits` is what covers it.
    m.insert("block_ticks".into(), Nbt::List(Vec::new()));
    m.insert("fluid_ticks".into(), Nbt::List(Vec::new()));
    m.insert(
        "PostProcessing".into(),
        Nbt::List((0..24).map(|_| Nbt::List(Vec::new())).collect()),
    );
    m.insert("structures".into(), {
        let mut s = BTreeMap::new();
        s.insert("References".into(), Nbt::Compound(BTreeMap::new()));
        s.insert("starts".into(), Nbt::Compound(BTreeMap::new()));
        Nbt::Compound(s)
    });
    m
}

/// Top-level keys [`ChunkColumn`] has a field for; everything else in a chunk
/// root goes to [`ChunkColumn::extra`].
const MODELLED_CHUNK_KEYS: [&str; 8] = [
    "DataVersion",
    "xPos",
    "zPos",
    "yPos",
    "Status",
    "sections",
    "block_entities",
    "Heightmaps",
];

impl ChunkColumn {
    /// The section holding world y, and the index of `(x, y, z)` within it.
    ///
    /// `x` and `z` are world coordinates and are masked to the column; `y` is
    /// a world y and must fall inside a section this column actually carries,
    /// which is why this returns an `Option` rather than clamping. A column
    /// generated by a partial pipeline legitimately has fewer sections than
    /// the dimension has, and silently writing into the wrong one would be a
    /// block that moves rather than a block that fails to save.
    fn locate(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize)> {
        // Floor division: y = -1 is in section -1, not section 0.
        let section_y = i8::try_from(y.div_euclid(16)).ok()?;
        let slot = self.sections.iter().position(|s| s.y == section_y)?;
        // `4096 palette indices (y·256 + z·16 + x)`, with y taken within the
        // section — `rem_euclid` so negative world y lands at 0..16, not
        // -16..0.
        let index = (y.rem_euclid(16) * 256 + (z & 15) * 16 + (x & 15)) as usize;
        Some((slot, index))
    }

    /// The canonical block-state string at a world position, or `None` if the
    /// column has no section covering that y.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<&str> {
        let (slot, index) = self.locate(x, y, z)?;
        let section = &self.sections[slot];
        // An empty `blocks` means every cell is palette entry 0 — the
        // single-state section a full-air or full-stone chunk compresses to.
        let entry = if section.blocks.is_empty() {
            0
        } else {
            *section.blocks.get(index)? as usize
        };
        section.palette.get(entry).map(String::as_str)
    }

    /// Set the canonical block state at a world position.
    ///
    /// Returns whether the column changed. Extends the section's palette if
    /// the state is new, and materialises the implicit all-zero `blocks`
    /// array the moment a section stops being uniform.
    pub fn set_block_at(&mut self, x: i32, y: i32, z: i32, state: &str) -> bool {
        let Some((slot, index)) = self.locate(x, y, z) else {
            return false;
        };
        let section = &mut self.sections[slot];
        let entry = match section.palette.iter().position(|p| p == state) {
            Some(at) => at,
            None => {
                section.palette.push(state.to_string());
                section.palette.len() - 1
            }
        };
        if section.blocks.is_empty() {
            // Was uniform. If it is still uniform after this write there is
            // nothing to materialise; otherwise every other cell is entry 0.
            if entry == 0 {
                return false;
            }
            section.blocks = vec![0u16; 4096];
        }
        let entry = entry as u16;
        if section.blocks[index] == entry {
            return false;
        }
        section.blocks[index] = entry;
        true
    }
}

/// One `structures.starts` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureStart {
    /// `minecraft:mineshaft` and the like.
    pub id: String,
    /// The chunk the start belongs to, from the entry itself rather than
    /// the containing chunk — they agree, and reading the entry means a
    /// mismatch shows up as one.
    pub chunk_x: i32,
    pub chunk_z: i32,
    /// Every piece, in the order vanilla wrote them. The order is
    /// behaviour: pieces are generated by a recursive walk and later ones
    /// overwrite earlier ones where they overlap.
    pub pieces: Vec<StructurePiece>,
}

/// One `Children` entry: what it is and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructurePiece {
    /// The piece type, e.g. `MSCorridor`. Absent in a few structures that
    /// write no `id`, hence the empty string rather than an `Option`.
    pub id: String,
    /// `BB` — inclusive min/max, `[minX, minY, minZ, maxX, maxY, maxZ]`.
    pub bounding_box: [i32; 6],
    /// `hr`, `sc`, `Num` — a mineshaft corridor's own fields, which vanilla
    /// writes into the piece and nothing else does.
    ///
    /// They are a free oracle for the three values that decide a corridor's
    /// entire draw sequence: whether it lays rails, whether it is a spider
    /// corridor (which costs a whole `generateMaybeBox` of draws), and how
    /// many five-block sections it runs for. A bounding box that matches
    /// proves the piece is in the right place and says nothing about any of
    /// these.
    pub has_rails: Option<bool>,
    pub spider: Option<bool>,
    pub sections: Option<i32>,
    /// `hps` — `MineShaftCorridor.hasPlacedSpider`, and the reason it is in
    /// this struct at all.
    ///
    /// It is **not** layout: it is written by `postProcess`, and vanilla
    /// saves it because the field has to survive between the chunk passes
    /// that place one corridor. That is the whole point. The flag is checked
    /// *before* the `nextInt(3)` that picks the spawner's z, so a corridor
    /// that already placed its spawner spends no value on it in the next
    /// chunk it reaches — a port that treats it as call-local redraws, and
    /// every draw after it in that chunk's pass lands one place late.
    pub spider_placed: Option<bool>,
    /// `O` — the piece's orientation as a `Direction` index, or `-1` when
    /// the piece did not record one.
    pub orientation: i32,
    /// `GD` — the recursion depth this piece was generated at. Load-bearing
    /// for mineshafts: the walk stops at a depth limit, so a port that gets
    /// the depth wrong builds a different-sized mine.
    pub depth: i32,
    /// `MST` — `MineshaftStructure.Type.ordinal()`: 0 normal, 1 mesa.
    ///
    /// The type carries exactly three blocks — `(OAK_LOG, OAK_PLANKS,
    /// OAK_FENCE)` against `(DARK_OAK_LOG, DARK_OAK_PLANKS,
    /// DARK_OAK_FENCE)` — and every reference to the field inside the piece
    /// classes is one of `getWoodState`, `getPlanksState`, `getFenceState`
    /// or this `ordinal()`. No piece compares it against `MESA`, so within a
    /// piece it really is only a palette. The one behavioural branch is in
    /// `MineshaftStructure#generatePiecesAndAdjust`, which places a mesa mine
    /// against the surface height instead of below sea level.
    pub mineshaft_type: Option<i32>,
    /// `Seed` — a fortress `BridgeEndFiller`'s `selfSeed`, the bare
    /// `nextInt()` its constructor draws. One integer per filler, read off the
    /// walk's own stream at the moment the filler was made, so it pins the
    /// stream position at every dead end of the walk.
    pub seed: Option<i32>,
    /// `Chest` — a fortress corridor turn's `isNeedingChest`, `nextInt(3) == 0`
    /// at construction.
    pub chest: Option<bool>,
    /// `Mob` — a fortress `MonsterThrone`'s `hasPlacedSpawner`.
    pub mob: Option<bool>,
    /// `Entrances` — `MineShaftRoom.childEntranceBoxes`, one 6-int box per
    /// child that broke through the room's wall.
    ///
    /// The room's `postProcess` hollows exactly these out, on top of its own
    /// box and dome, and it draws **no** random values while doing so. That
    /// makes this the one mineshaft oracle the draw-count work could not
    /// reach: a wrong entrance is wrong *blocks* and costs nothing in the
    /// stream, so every offset downstream still agrees and the ceiling tool
    /// sees nothing.
    ///
    /// Encoded by `BoundingBox.CODEC.listOf()`, and `BoundingBox.CODEC` is
    /// `Codec.INT_STREAM.comapFlatMap(...)`, so this is an NBT list of
    /// 6-element int arrays in the same `[minX, minY, minZ, maxX, maxY,
    /// maxZ]` order as `BB`.
    pub entrances: Vec<[i32; 6]>,
    /// `pool_element.location` — the template a jigsaw piece placed, e.g.
    /// `minecraft:village/savanna/houses/savanna_small_house_1`.
    ///
    /// This is the whole reason the jigsaw fields are read at all. A village
    /// has no draw-level harness — `DecorateGolden` runs *features* — so the
    /// only oracle for `JigsawPlacement.addPieces` is the piece list vanilla
    /// itself saved into the chunk. `PoolElementStructurePiece.
    /// addAdditionalSaveData` writes `PosX/PosY/PosZ`, `ground_level_delta`,
    /// `pool_element` and `rotation`, which together pin every decision the
    /// assembler makes: which element the shuffle drew, which of the four
    /// rotations survived, and where the piece landed. Comparing blocks
    /// answers none of those separately; comparing this list answers all of
    /// them at once.
    pub pool_element: Option<String>,
    /// `pool_element.element_type` — `minecraft:single_pool_element`,
    /// `legacy_single_pool_element`, `feature_pool_element`, `list_pool_element`.
    pub pool_element_type: Option<String>,
    /// `pool_element.feature` — a `feature_pool_element`'s placed feature.
    ///
    /// A feature element has **no `location`**, so [`Self::pool_element`] is
    /// `None` on every one of them, and a comparison that only looks there
    /// reads a perfectly reproduced feature element as a divergence. Fourteen
    /// of the 98 pieces of the savanna village at seed 20260722 chunk (46,77)
    /// are feature elements, and all fourteen were reported as mismatches
    /// against a field vanilla does not write for them.
    ///
    /// `FeaturePoolElement`'s codec is `PlacedFeature.CODEC.fieldOf("feature")`
    /// and `PlacedFeature.CODEC` is a `RegistryFileCodec`, so a registry
    /// reference serialises as the plain id string. An inline placed feature
    /// would serialise as a compound instead and read as `None` here — which
    /// is why the oracle treats a `None` on a feature element as
    /// *unadjudicated* and says so, rather than as a pass.
    pub pool_element_feature: Option<String>,
    /// `pool_element.projection` — `rigid` or `terrain_matching`. Saved per
    /// *piece* because `StructurePoolElement.setProjection` rewrites it from
    /// the pool's own projection, so the file records what was used rather
    /// than what the element file says.
    pub projection: Option<String>,
    /// `rotation` — `none`, `clockwise_90`, `clockwise_180`,
    /// `counterclockwise_90`.
    pub rotation: Option<String>,
    /// `PosX`/`PosY`/`PosZ` — the piece's placement position, which is
    /// **not** the minimum corner of `BB` under a rotation.
    pub pos: Option<[i32; 3]>,
    /// `ground_level_delta`.
    pub ground_level_delta: Option<i32>,
    /// `junctions` — one `(source_x, source_ground_y, source_z, delta_y,
    /// dest_proj)` per connection recorded on this piece.
    pub junctions: Vec<[i32; 4]>,
    /// `Template` — `TemplateStructurePiece.templateName`, the NBT the piece
    /// stamped, e.g. `base_floor`.
    ///
    /// A `TemplateStructurePiece` is **not** a jigsaw piece and writes none
    /// of the `pool_element` keys above. The End city is the case that forced
    /// this field: it is a hand-written recursive template walk, not a
    /// jigsaw, so `pool_element` is absent on every one of its pieces and the
    /// jigsaw oracle reports the world as holding no starts at all. These
    /// three fields are the same oracle for that family — which template, at
    /// which rotation, at which position, in the order the walk emitted them.
    pub template: Option<String>,
    /// `TPX`/`TPY`/`TPZ` — `TemplateStructurePiece.templatePosition`, the
    /// origin the template was stamped from. Distinct from `BB`'s minimum
    /// corner under a rotation, exactly as `PosX/Y/Z` is for a jigsaw piece.
    pub template_pos: Option<[i32; 3]>,
    /// `Rot` — the rotation a `TemplateStructurePiece` subclass saved
    /// alongside its template. Read as a string first and as an ordinal
    /// second, because `Rotation.LEGACY_CODEC` has been both.
    pub template_rotation: Option<String>,
    /// `VerticalPlacement` — a ruined portal's own saved field, and the one
    /// that decides where its apron sits: a surface portal decays at the
    /// ground, a buried one at its own floor.
    pub vertical_placement: Option<String>,
    /// `Properties.cold` and `Properties.overgrown` — the two saved portal
    /// properties that change the **draw sequence** rather than the output.
    ///
    /// `cold` short-circuits `placeNetherrackOrMagma` before its `nextFloat`,
    /// and `overgrown` adds an unconditional `maybeAddLeavesAbove` draw after
    /// every apron block. The other four (`mossiness`, `air_pocket`, `vines`,
    /// `replace_with_blackstone`) spend nothing on the piece's own stream —
    /// mossiness and air_pocket are read by processors that seed per position
    /// from `Mth.getSeed` — so they are not read here.
    pub portal_cold: Option<bool>,
    pub portal_overgrown: Option<bool>,
}

/// One section's saved light. Either array may be absent: vanilla writes
/// `BlockLight` only when the layer is materialised and `SkyLight` only in a
/// dimension that has one, so an absent array is a fact about the section and
/// not a gap in the reader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionLight {
    pub block: Option<Vec<u8>>,
    pub sky: Option<Vec<u8>>,
}

impl SectionLight {
    /// The nibble at a section-relative position, or `None` if that layer is
    /// absent. Absent is NOT zero — a chunk that never saved block light is
    /// not a chunk whose block light is zero — so this returns an option and
    /// every caller has to say which it meant.
    #[must_use]
    pub fn nibble(&self, layer_sky: bool, x: u8, y: u8, z: u8) -> Option<u8> {
        let a = if layer_sky { &self.sky } else { &self.block };
        let i = (usize::from(y) << 8) | (usize::from(z) << 4) | usize::from(x);
        a.as_ref().map(|d| (d[i >> 1] >> (4 * (i & 1))) & 15)
    }
}

#[derive(Debug)]
pub struct Section {
    pub y: i8,
    /// Canonical block-state strings: `name[k=v,k=v]`, properties sorted
    /// (BTreeMap order — deterministic by construction).
    pub palette: Vec<String>,
    /// 4096 palette indices (y·256 + z·16 + x order), empty ⇒ all index 0.
    pub blocks: Vec<u16>,
    pub biome_palette: Vec<String>,
    /// 64 palette indices (4×4×4 cells), empty ⇒ all index 0.
    pub biomes: Vec<u8>,
    /// `BlockLight` and `SkyLight`, and anything else a section carries,
    /// verbatim. 2,048-byte nibble arrays; this crate does not compute light
    /// and dropping them on a rewrite would darken a saved world.
    pub extra: BTreeMap<String, Nbt>,
}

/// Section keys [`Section`] models; everything else goes to
/// [`Section::extra`].
const MODELLED_SECTION_KEYS: [&str; 3] = ["Y", "block_states", "biomes"];

/// Read every present chunk in a region file. Chunk world coords come from
/// the chunk NBT itself (xPos/zPos) — trusted over filename math.
pub fn read_region(path: &Path) -> io::Result<Vec<ChunkColumn>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 8192 {
        return Err(io::Error::other(format!(
            "{}: truncated region header",
            path.display()
        )));
    }
    let mut chunks = Vec::new();
    for i in 0..1024 {
        let entry = u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let sector = (entry >> 8) as usize;
        let count = (entry & 0xFF) as usize;
        if sector == 0 && count == 0 {
            continue; // chunk absent
        }
        let at = sector * 4096;
        if at + 5 > bytes.len() {
            return Err(io::Error::other("chunk offset past EOF"));
        }
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if len < 1 || at + 4 + len > bytes.len() {
            return Err(io::Error::other("chunk length past EOF"));
        }
        let compression = bytes[at + 4];
        let payload = &bytes[at + 5..at + 4 + len];
        let nbt_bytes: Vec<u8> = match compression {
            1 => {
                let mut out = Vec::new();
                GzDecoder::new(payload).read_to_end(&mut out)?;
                out
            }
            2 => {
                let mut out = Vec::new();
                ZlibDecoder::new(payload).read_to_end(&mut out)?;
                out
            }
            3 => payload.to_vec(),
            c => {
                return Err(io::Error::other(format!(
                    "compression scheme {c} unsupported"
                )))
            }
        };
        let (_, root) = nbt::read_named_root(&mut nbt_bytes.as_slice())?;
        chunks.push(decode_chunk(&root)?);
    }
    Ok(chunks)
}

fn decode_chunk(root: &Nbt) -> io::Result<ChunkColumn> {
    let get_i32 = |k: &str| {
        root.get(k)
            .and_then(Nbt::as_i32)
            .ok_or_else(|| io::Error::other(format!("chunk missing {k}")))
    };
    let x = get_i32("xPos")?;
    let z = get_i32("zPos")?;
    let data_version = get_i32("DataVersion")?;
    let status = root
        .get("Status")
        .and_then(Nbt::as_str)
        .unwrap_or("?")
        .to_string();

    let is_light_on = root
        .get("isLightOn")
        .and_then(Nbt::as_i32)
        .is_some_and(|v| v != 0);

    let mut light: BTreeMap<i8, SectionLight> = BTreeMap::new();
    let mut sections = Vec::new();
    for sec in root.get("sections").and_then(Nbt::as_list).unwrap_or(&[]) {
        let y = sec
            .get("Y")
            .and_then(Nbt::as_i32)
            .ok_or_else(|| io::Error::other("section missing Y"))? as i8;

        // Read the light BEFORE the `continue` below: a section with no
        // `block_states` is exactly the padding section above or below the
        // terrain, and those are the ones that carry nothing BUT light. A
        // reader that skipped them would silently drop the brightest sections
        // in the file.
        let nibbles = |k: &str| -> io::Result<Option<Vec<u8>>> {
            match sec.get(k) {
                None => Ok(None),
                Some(v) => {
                    let d = v.as_byte_array().ok_or_else(|| {
                        io::Error::other(format!("section {k} is not a byte array"))
                    })?;
                    if d.len() != 2048 {
                        return Err(io::Error::other(format!(
                            "section {k} is {} bytes, not 2048",
                            d.len()
                        )));
                    }
                    #[allow(clippy::cast_sign_loss)]
                    Ok(Some(d.iter().map(|b| *b as u8).collect()))
                }
            }
        };
        let sl = SectionLight {
            block: nibbles("BlockLight")?,
            sky: nibbles("SkyLight")?,
        };
        if sl.block.is_some() || sl.sky.is_some() {
            light.insert(y, sl);
        }

        let (palette, blocks) = match sec.get("block_states") {
            Some(bs) => decode_container(bs, 4096, 4, canonical_block_state)?,
            None => continue, // no block data (above/below build limit padding)
        };
        let (biome_palette, biomes_u16) = match sec.get("biomes") {
            Some(bio) => decode_container(bio, 64, 0, |e| {
                e.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| io::Error::other("biome palette entry not a string"))
            })?,
            None => (Vec::new(), Vec::new()),
        };
        let mut section_extra = sec.as_compound().cloned().unwrap_or_default();
        for key in MODELLED_SECTION_KEYS {
            section_extra.remove(key);
        }
        sections.push(Section {
            y,
            palette,
            blocks,
            biome_palette,
            biomes: biomes_u16.into_iter().map(|v| v as u8).collect(),
            extra: section_extra,
        });
    }
    sections.sort_by_key(|s| s.y);

    let mut structure_starts = Vec::new();
    if let Some(starts) = root.get("structures").and_then(|s| s.get("starts")) {
        for (id, start) in starts.as_compound().into_iter().flatten() {
            // An empty compound is written for a structure that was
            // considered and did not start; only entries carrying `Children`
            // are real starts.
            let Some(children) = start.get("Children").and_then(Nbt::as_list) else {
                continue;
            };
            let mut pieces = Vec::new();
            for child in children {
                let bb = child
                    .get("BB")
                    .and_then(Nbt::as_i32_array)
                    .ok_or_else(|| io::Error::other(format!("{id}: piece has no BB")))?;
                if bb.len() != 6 {
                    return Err(io::Error::other(format!(
                        "{id}: piece BB has {} entries, want 6",
                        bb.len()
                    )));
                }
                // `Entrances` is absent on every piece but a mineshaft room,
                // which is why a missing key is empty rather than an error —
                // but a key that is present and the wrong shape is a parser
                // bug or a format change, and silently dropping the box would
                // turn the gate below into one that passes on nothing.
                let mut entrances = Vec::new();
                for entrance in child.get("Entrances").and_then(Nbt::as_list).unwrap_or(&[]) {
                    let e = entrance.as_i32_array().ok_or_else(|| {
                        io::Error::other(format!("{id}: Entrances entry is not an int array"))
                    })?;
                    if e.len() != 6 {
                        return Err(io::Error::other(format!(
                            "{id}: Entrances box has {} entries, want 6",
                            e.len()
                        )));
                    }
                    entrances.push([e[0], e[1], e[2], e[3], e[4], e[5]]);
                }
                // The jigsaw half. Every one of these keys is absent on a
                // non-jigsaw piece, so they are options rather than errors —
                // but a `pool_element` that is present and shaped wrong would
                // be a format change, and reading it as `None` would quietly
                // turn the village oracle into one that compares nothing.
                let element = child.get("pool_element");
                let mut junctions = Vec::new();
                for j in child.get("junctions").and_then(Nbt::as_list).unwrap_or(&[]) {
                    let field = |name: &str| j.get(name).and_then(Nbt::as_i32);
                    let (Some(sx), Some(sy), Some(sz), Some(dy)) = (
                        field("source_x"),
                        field("source_ground_y"),
                        field("source_z"),
                        field("delta_y"),
                    ) else {
                        return Err(io::Error::other(format!(
                            "{id}: a junction is missing one of its four ints"
                        )));
                    };
                    junctions.push([sx, sy, sz, dy]);
                }
                pieces.push(StructurePiece {
                    id: child
                        .get("id")
                        .and_then(Nbt::as_str)
                        .unwrap_or("")
                        .to_string(),
                    pool_element: element
                        .and_then(|e| e.get("location"))
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    pool_element_type: element
                        .and_then(|e| e.get("element_type"))
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    pool_element_feature: element
                        .and_then(|e| e.get("feature"))
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    projection: element
                        .and_then(|e| e.get("projection"))
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    rotation: child
                        .get("rotation")
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    pos: match (
                        child.get("PosX").and_then(Nbt::as_i32),
                        child.get("PosY").and_then(Nbt::as_i32),
                        child.get("PosZ").and_then(Nbt::as_i32),
                    ) {
                        (Some(px), Some(py), Some(pz)) => Some([px, py, pz]),
                        _ => None,
                    },
                    ground_level_delta: child.get("ground_level_delta").and_then(Nbt::as_i32),
                    junctions,
                    template: child
                        .get("Template")
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    template_pos: match (
                        child.get("TPX").and_then(Nbt::as_i32),
                        child.get("TPY").and_then(Nbt::as_i32),
                        child.get("TPZ").and_then(Nbt::as_i32),
                    ) {
                        (Some(px), Some(py), Some(pz)) => Some([px, py, pz]),
                        _ => None,
                    },
                    template_rotation: child.get("Rot").and_then(|r| {
                        r.as_str()
                            .map(str::to_string)
                            .or_else(|| r.as_i32().map(|v| v.to_string()))
                    }),
                    vertical_placement: child
                        .get("VerticalPlacement")
                        .and_then(Nbt::as_str)
                        .map(str::to_string),
                    portal_cold: child
                        .get("Properties")
                        .and_then(|p| p.get("cold"))
                        .and_then(Nbt::as_i32)
                        .map(|v| v != 0),
                    portal_overgrown: child
                        .get("Properties")
                        .and_then(|p| p.get("overgrown"))
                        .and_then(Nbt::as_i32)
                        .map(|v| v != 0),
                    bounding_box: [bb[0], bb[1], bb[2], bb[3], bb[4], bb[5]],
                    has_rails: child.get("hr").and_then(Nbt::as_i32).map(|v| v != 0),
                    spider: child.get("sc").and_then(Nbt::as_i32).map(|v| v != 0),
                    sections: child.get("Num").and_then(Nbt::as_i32),
                    spider_placed: child.get("hps").and_then(Nbt::as_i32).map(|v| v != 0),
                    orientation: child.get("O").and_then(Nbt::as_i32).unwrap_or(-1),
                    depth: child.get("GD").and_then(Nbt::as_i32).unwrap_or(0),
                    mineshaft_type: child.get("MST").and_then(Nbt::as_i32),
                    seed: child.get("Seed").and_then(Nbt::as_i32),
                    chest: child.get("Chest").and_then(Nbt::as_i32).map(|v| v != 0),
                    mob: child.get("Mob").and_then(Nbt::as_i32).map(|v| v != 0),
                    entrances,
                });
            }
            structure_starts.push(StructureStart {
                id: id.to_string(),
                chunk_x: start.get("ChunkX").and_then(Nbt::as_i32).unwrap_or(x),
                chunk_z: start.get("ChunkZ").and_then(Nbt::as_i32).unwrap_or(z),
                pieces,
            });
        }
    }
    structure_starts.sort_by(|a, b| a.id.cmp(&b.id));

    let structure_references = root
        .get("structures")
        .and_then(|s| s.get("References"))
        .and_then(Nbt::as_compound)
        .map(|refs| {
            refs.iter()
                .filter_map(|(id, packed)| Some((id.clone(), packed.as_long_array()?.to_vec())))
                .collect()
        })
        .unwrap_or_default();

    // `block_entities` — confirmed against `SerializableChunkData`'s own
    // string constants, not guessed from the pre-1.18 `TileEntities` name.
    // Absent on a chunk that never held one, which is not an error: a chunk
    // built from a partial pipeline, or one at a status before entities are
    // ever placed, legitimately has none.
    let block_entities = root
        .get("block_entities")
        .and_then(Nbt::as_list)
        .unwrap_or(&[])
        .to_vec();

    // **The stored value is relative to the bottom of the world, and this
    // used to read it as an absolute y.** `Heightmap.setHeight` stores
    // `y - chunk.getMinY()` and `getFirstAvailable` adds it back, so a raw
    // read is 64 short of the world y this field is documented to hold — and
    // `pack_heightmap`, which subtracts `min_y` again on the way out, is
    // right. The pair therefore raised every heightmap by 64 on a
    // decode-then-encode of a real vanilla chunk, and nothing could see it:
    // the one test that rewrote vanilla chunks compared palettes and block
    // indices and never looked at `Heightmaps`.
    let min_y = root
        .get("yPos")
        .and_then(Nbt::as_i32)
        .or_else(|| sections.first().map(|s| i32::from(s.y)))
        .unwrap_or(-4)
        * 16;
    let heightmaps = root
        .get("Heightmaps")
        .and_then(Nbt::as_compound)
        .map(|maps| {
            maps.iter()
                .filter_map(|(name, packed)| {
                    let longs = packed.as_long_array()?;
                    let mut out = Vec::with_capacity(256);
                    for word in longs {
                        for k in 0..7 {
                            if out.len() >= 256 {
                                break;
                            }
                            out.push((((word >> (k * 9)) & 0x1FF) as i32) + min_y);
                        }
                    }
                    (out.len() == 256).then_some((name.clone(), out))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut extra = root.as_compound().cloned().unwrap_or_default();
    for key in MODELLED_CHUNK_KEYS {
        extra.remove(key);
    }

    Ok(ChunkColumn {
        x,
        z,
        status,
        data_version,
        sections,
        is_light_on,
        light,
        block_entities,
        heightmaps,
        structure_starts,
        structure_references,
        extra,
    })
}

/// Decode a paletted container (block_states or biomes): palette list +
/// bit-packed LongArray, non-spanning packing (data version ≥ 1.16).
/// `min_bits` is 4 for blocks, 0 for biomes — reference:
/// net.minecraft.world.level.chunk.PalettedContainer.Strategy.
fn decode_container(
    tag: &Nbt,
    entries: usize,
    min_bits: u32,
    entry_to_string: impl Fn(&Nbt) -> io::Result<String>,
) -> io::Result<(Vec<String>, Vec<u16>)> {
    let palette_tags = tag
        .get("palette")
        .and_then(Nbt::as_list)
        .ok_or_else(|| io::Error::other("container missing palette"))?;
    let palette: Vec<String> = palette_tags
        .iter()
        .map(&entry_to_string)
        .collect::<io::Result<_>>()?;
    if palette.is_empty() {
        return Err(io::Error::other("empty palette"));
    }

    let data = tag.get("data").and_then(Nbt::as_long_array);
    if palette.len() == 1 || data.is_none() {
        // Single-entry container: all indices 0, data legitimately absent.
        return Ok((palette, Vec::new()));
    }
    let data = data.unwrap();

    let needed = usize::BITS - (palette.len() - 1).leading_zeros();
    let bits = needed.max(min_bits).max(1);
    let per_long = 64 / bits as usize;
    let expect_longs = entries.div_ceil(per_long);
    if data.len() != expect_longs {
        return Err(io::Error::other(format!(
            "container data length {} != expected {expect_longs} (bits={bits})",
            data.len()
        )));
    }
    let mask = (1u64 << bits) - 1;
    let mut out = Vec::with_capacity(entries);
    for i in 0..entries {
        let long = data[i / per_long] as u64;
        let idx = ((long >> ((i % per_long) as u32 * bits)) & mask) as usize;
        if idx >= palette.len() {
            return Err(io::Error::other(format!(
                "palette index {idx} out of range ({} entries)",
                palette.len()
            )));
        }
        out.push(idx as u16);
    }
    Ok((palette, out))
}

/// `minecraft:oak_stairs[facing=east,half=bottom,...]` — properties sorted
/// (compound is a BTreeMap), exactly one canonical spelling per state.
fn canonical_block_state(entry: &Nbt) -> io::Result<String> {
    let name = entry
        .get("Name")
        .and_then(Nbt::as_str)
        .ok_or_else(|| io::Error::other("palette entry missing Name"))?;
    match entry.get("Properties").and_then(Nbt::as_compound) {
        None => Ok(name.to_string()),
        Some(props) => {
            let mut s = String::with_capacity(name.len() + 16);
            s.push_str(name);
            s.push('[');
            for (i, (k, v)) in props.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(k);
                s.push('=');
                s.push_str(v.as_str().unwrap_or("?"));
            }
            s.push(']');
            Ok(s)
        }
    }
}

/// Encode one chunk column back into vanilla's chunk NBT.
///
/// The inverse of [`decode_chunk`], and deliberately built on the same
/// reduced model: palette strings plus indices. That keeps the writer
/// testable by round-trip before a block registry exists, and it is the
/// model the differ already trusts.
///
/// Not byte-identical to vanilla's own output, and cannot be: NBT compounds
/// are unordered, we sort keys, and vanilla emits insertion order. The
/// *content* is identical, which is what a loader and the differ read.
fn encode_chunk(chunk: &ChunkColumn) -> Nbt {
    // **Starts from the passthrough, not from an empty map.** Everything this
    // struct does not model — the tick queues, `isLightOn`, `InhabitedTime`,
    // `structures` — is written back exactly as it was read. A column the
    // generator built carries `generated_extra()` here, which is the same set
    // at the values a fresh chunk has, so a worldgen dump is unchanged.
    let mut root = chunk.extra.clone();
    root.insert("DataVersion".into(), Nbt::Int(chunk.data_version));
    root.insert("xPos".into(), Nbt::Int(chunk.x));
    root.insert("zPos".into(), Nbt::Int(chunk.z));
    // yPos is the lowest section index, which vanilla needs to place the
    // sections list; -4 is the overworld's (y = -64 >> 4).
    root.insert(
        "yPos".into(),
        Nbt::Int(chunk.sections.first().map_or(-4, |s| i32::from(s.y))),
    );
    root.insert("Status".into(), Nbt::String(chunk.status.clone()));
    let sections = chunk
        .sections
        .iter()
        .map(|section| {
            let mut m = section.extra.clone();
            m.insert("Y".into(), Nbt::Byte(section.y));
            m.insert(
                "block_states".into(),
                encode_container(&section.palette, &section.blocks, 4096, 4, block_state_tag),
            );
            if !section.biome_palette.is_empty() {
                let biomes: Vec<u16> = section.biomes.iter().map(|b| u16::from(*b)).collect();
                m.insert(
                    "biomes".into(),
                    encode_container(&section.biome_palette, &biomes, 64, 0, |s| {
                        Nbt::String(s.to_string())
                    }),
                );
            }
            Nbt::Compound(m)
        })
        .collect();
    root.insert("sections".into(), Nbt::List(sections));

    // **Always written, including empty.** This used to be omitted when the
    // list was empty, on the reasoning that "a vanilla chunk that never had a
    // chest" carries no key. It does: a real 26.2 chunk at
    // `minecraft:structure_starts` and one at `minecraft:full` both carry
    // `block_entities` as an empty list, checked on the harvest. Omitting it
    // is legal NBT that vanilla reads identically, and it was the one thing
    // standing between a decoded vanilla chunk and a byte-identical rewrite.
    root.insert(
        "block_entities".into(),
        Nbt::List(chunk.block_entities.clone()),
    );

    // Same as `block_entities`: vanilla writes the key either way, as an
    // empty compound on a chunk whose heightmaps have not been primed.
    let min_y = chunk.sections.first().map_or(-4, |s| i32::from(s.y)) * 16;
    let mut maps = BTreeMap::new();
    for (name, values) in &chunk.heightmaps {
        maps.insert(name.clone(), Nbt::LongArray(pack_heightmap(values, min_y)));
    }
    root.insert("Heightmaps".into(), Nbt::Compound(maps));

    Nbt::Compound(root)
}

/// Vanilla stores a heightmap as 256 nine-bit entries, seven to a long, and
/// **does not let an entry span two longs** — the top bit of each long is
/// simply unused. Getting that wrong produces a file that loads and shows
/// the wrong terrain height, which is why this is its own function with its
/// own test rather than a line inside `encode_chunk`.
///
/// The stored value is relative to the bottom of the world, so a heightmap
/// of `min_y` means "nothing here" and reads back as zero.
fn pack_heightmap(values: &[i32], min_y: i32) -> Vec<i64> {
    assert_eq!(values.len(), 256, "a heightmap is one entry per column");
    let mut longs = vec![0i64; 37];
    for (i, value) in values.iter().enumerate() {
        let stored = (value - min_y).clamp(0, 511) as i64;
        longs[i / 7] |= stored << ((i % 7) * 9);
    }
    longs
}

/// `minecraft:oak_stairs[facing=east,half=bottom]` back into a palette entry.
fn block_state_tag(canonical: &str) -> Nbt {
    let mut entry = BTreeMap::new();
    match canonical.split_once('[') {
        None => {
            entry.insert("Name".into(), Nbt::String(canonical.to_string()));
        }
        Some((name, rest)) => {
            entry.insert("Name".into(), Nbt::String(name.to_string()));
            let inner = rest.strip_suffix(']').unwrap_or(rest);
            let mut props = BTreeMap::new();
            for pair in inner.split(',') {
                if let Some((k, v)) = pair.split_once('=') {
                    props.insert(k.to_string(), Nbt::String(v.to_string()));
                }
            }
            entry.insert("Properties".into(), Nbt::Compound(props));
        }
    }
    Nbt::Compound(entry)
}

/// The inverse of [`decode_container`]: palette list plus a bit-packed
/// `LongArray`, non-spanning.
///
/// "Non-spanning" is the whole subtlety. Entries are packed
/// `64 / bits` per long and the leftover high bits of each long are **left
/// zero** rather than carrying the next entry across the boundary. Packing
/// them densely produces a file that loads and is wrong everywhere the
/// boundary falls.
fn encode_container(
    palette: &[String],
    indices: &[u16],
    entries: usize,
    min_bits: u32,
    entry_to_tag: impl Fn(&str) -> Nbt,
) -> Nbt {
    let mut m = BTreeMap::new();
    m.insert(
        "palette".into(),
        Nbt::List(palette.iter().map(|p| entry_to_tag(p)).collect()),
    );

    // A single-entry palette carries no data at all, which is how vanilla
    // stores an all-air or all-stone section in a few bytes.
    if palette.len() > 1 {
        let needed = usize::BITS - (palette.len() - 1).leading_zeros();
        let bits = needed.max(min_bits).max(1);
        let per_long = 64 / bits as usize;
        let mut data = vec![0i64; entries.div_ceil(per_long)];
        for i in 0..entries {
            let index = u64::from(*indices.get(i).unwrap_or(&0));
            let shift = (i % per_long) as u32 * bits;
            data[i / per_long] |= (index << shift) as i64;
        }
        m.insert("data".into(), Nbt::LongArray(data));
    }
    Nbt::Compound(m)
}

/// One chunk's record exactly as it sits in a region file: the compression
/// scheme byte and the payload, still compressed and still unparsed.
///
/// Carrying a chunk in this form is what lets [`write_region_merging`] leave
/// a chunk it was not asked to touch **byte-identical**. Round-tripping it
/// through [`ChunkColumn`] instead would not: that model is deliberately
/// reduced to palettes, indices and (since block entities gained a field)
/// raw `block_entities` compounds, so a decode/encode pass still silently
/// drops `entities`, `block_ticks`, `fluid_ticks`, `structures`, the light
/// arrays and `isLightOn` — see [`encode_chunk`], which writes all of those
/// empty. For the worldgen oracle that loss is harmless, because the chunks
/// it writes never had any. For a save path it is data loss, and it would be
/// invisible: the chunk still loads, it has just forgotten every entity
/// standing in it — item frames, minecarts, dropped items — which `entities`
/// alone carries and `block_entities` does not.
pub struct RawRecord {
    pub scheme: u8,
    pub payload: Vec<u8>,
}

/// Read every present chunk's record without decompressing or parsing it.
///
/// Keyed by the header's own offset-table index, `(x & 31) + (z & 31) * 32`,
/// so it round-trips through [`lay_out_region`] without consulting the chunk
/// NBT at all. A missing file is an empty map rather than an error: writing
/// the first chunk of a region is the normal case, not a failure.
pub fn read_region_records(path: &Path) -> io::Result<BTreeMap<usize, RawRecord>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e),
    };
    if bytes.len() < 8192 {
        return Err(io::Error::other(format!(
            "{}: truncated region header",
            path.display()
        )));
    }
    let mut records = BTreeMap::new();
    for i in 0..1024 {
        let entry = u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        if entry == 0 {
            continue; // CHUNK_NOT_PRESENT
        }
        let sector = (entry >> 8) as usize;
        let at = sector * 4096;
        if at + 5 > bytes.len() {
            return Err(io::Error::other("chunk offset past EOF"));
        }
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if len < 1 || at + 4 + len > bytes.len() {
            return Err(io::Error::other("chunk length past EOF"));
        }
        records.insert(
            i,
            RawRecord {
                scheme: bytes[at + 4],
                payload: bytes[at + 5..at + 4 + len].to_vec(),
            },
        );
    }
    Ok(records)
}

/// Compress and frame **any** root compound into the record a region file
/// holds.
///
/// Public and generic over the tag because the region-file container is not
/// specific to block chunks: `entities/r.X.Z.mca` and `poi/r.X.Z.mca` use the
/// identical sector table, timestamp table, length prefix and compression
/// byte, and differ only in what the compound inside says. Sharing the
/// framing is what stops a second implementation of the 4 KiB alignment and
/// the 255-sector limit drifting from this one.
///
/// # Errors
/// If the tag cannot be encoded or the deflate stream cannot be flushed.
pub fn encode_region_record(root: &Nbt) -> io::Result<RawRecord> {
    let mut nbt_bytes = Vec::new();
    nbt::write_named_root(&mut nbt_bytes, "", root)?;
    let mut payload = Vec::new();
    let mut encoder = ZlibEncoder::new(&mut payload, Compression::default());
    encoder.write_all(&nbt_bytes)?;
    encoder.finish()?;
    Ok(RawRecord { scheme: 2, payload })
}

/// Decompress one record back into its root compound.
///
/// # Errors
/// On an unsupported compression scheme or malformed NBT.
pub fn decode_region_record(record: &RawRecord) -> io::Result<Nbt> {
    let bytes = match record.scheme {
        1 => {
            let mut out = Vec::new();
            GzDecoder::new(record.payload.as_slice()).read_to_end(&mut out)?;
            out
        }
        2 => {
            let mut out = Vec::new();
            ZlibDecoder::new(record.payload.as_slice()).read_to_end(&mut out)?;
            out
        }
        3 => record.payload.clone(),
        c => {
            return Err(io::Error::other(format!(
                "compression scheme {c} unsupported"
            )))
        }
    };
    Ok(nbt::read_named_root(&mut bytes.as_slice())?.1)
}

/// Compress and frame one column into the record a region file holds.
fn encode_record(chunk: &ChunkColumn) -> io::Result<RawRecord> {
    let mut nbt_bytes = Vec::new();
    nbt::write_named_root(&mut nbt_bytes, "", &encode_chunk(chunk))?;
    let mut payload = Vec::new();
    // `finish()` rather than a drop: `flate2`'s `Drop` calls `try_finish` and
    // **discards the result**, so a failure to flush the tail of the deflate
    // stream would leave a truncated payload and no error. Vanilla reports
    // that as `Failed to check chunk` and regenerates the chunk, which looks
    // exactly like "the save never happened".
    let mut encoder = ZlibEncoder::new(&mut payload, Compression::default());
    encoder.write_all(&nbt_bytes)?;
    encoder.finish()?;
    // Scheme 2 is `RegionFileVersion.VERSION_DEFLATE`, which is also
    // `RegionFileVersion.DEFAULT` — the id vanilla itself writes. Confirmed
    // from the class initialiser: id 1 `gzip`, 2 `deflate`, 3 `none`, 4 `lz4`,
    // 127 `custom`, and `DEFAULT = VERSION_DEFLATE`. Deflate here is the
    // zlib-wrapped form (`DeflaterOutputStream`/`InflaterInputStream` with the
    // default constructors), which is what `ZlibEncoder` emits.
    Ok(RawRecord { scheme: 2, payload })
}

/// Lay a full set of records out into region-file bytes: the 8 KiB header
/// then every payload on a 4 KiB sector boundary.
///
/// Reference: `net.minecraft.world.level.chunk.storage.RegionFile`. The
/// constructor allocates an 8192-byte header and calls
/// `usedSectors.force(0, 2)`, so sectors 0 and 1 are the two tables and
/// payloads start at 2. Its own validation loop is the specification for what
/// this must produce, and it names each way of getting it wrong:
///
/// * `"has invalid sector at index: {}; sector {} overlaps with header"` —
///   fires when `getSectorNumber(offset) < 2`.
/// * `"has an invalid sector at index: {}; size has to be > 0"` —
///   `getNumSectors(offset) == 0`.
/// * `"has an invalid sector at index: {}; sector {} is out of bounds"` —
///   the allocation runs past the file's length, which is why every record is
///   padded up to a whole sector before the next one starts.
///
/// The offset entry packs `sector << 8 | count`, so a chunk may occupy at
/// most 255 sectors inline; at 256 vanilla spills to an external `.mcc` file
/// (`EXTERNAL_CHUNK_THRESHOLD = 256`, `EXTERNAL_STREAM_FLAG = 128`). We refuse
/// instead of spilling, so the failure is loud rather than a chunk vanilla
/// cannot find.
pub fn lay_out_region(
    rx: i32,
    rz: i32,
    records: &BTreeMap<usize, RawRecord>,
) -> io::Result<Vec<u8>> {
    let mut offsets = [0u32; 1024];
    let mut timestamps = [0u32; 1024];
    let mut body: Vec<u8> = Vec::new();
    let mut next_sector = 2u32;

    for (&index, rec) in records {
        // [u32 length][u8 scheme][payload]; the length covers the scheme
        // byte, which is why it is payload.len() + 1 and not + 5.
        let mut record = Vec::with_capacity(rec.payload.len() + 5);
        record.extend_from_slice(&(rec.payload.len() as u32 + 1).to_be_bytes());
        record.push(rec.scheme);
        record.extend_from_slice(&rec.payload);
        let sectors = record.len().div_ceil(4096);
        if sectors > 255 {
            return Err(io::Error::other(format!(
                "chunk ({}, {}) needs {sectors} sectors; the offset table \
                 packs the count into 8 bits",
                (rx << 5) + (index % 32) as i32,
                (rz << 5) + (index / 32) as i32
            )));
        }
        record.resize(sectors * 4096, 0);

        offsets[index] = (next_sector << 8) | sectors as u32;
        // Vanilla stores `Util.getEpochMillis() / 1000` here. A fixed value
        // keeps output reproducible, and nothing reads it back: the whole
        // read path — `getChunkDataInputStream`, the constructor's validation
        // loop, `doesChunkExist`, `hasChunk` — consults `offsets` only, and
        // `timestamps` is written by `writeHeader` and never read for
        // correctness anywhere in `RegionFile`.
        timestamps[index] = 0;
        next_sector += sectors as u32;
        body.extend_from_slice(&record);
    }

    let mut out = Vec::with_capacity(8192 + body.len());
    for entry in offsets {
        out.extend_from_slice(&entry.to_be_bytes());
    }
    for entry in timestamps {
        out.extend_from_slice(&entry.to_be_bytes());
    }
    out.extend_from_slice(&body);
    Ok(out)
}

#[must_use]
pub const fn region_index(x: i32, z: i32) -> usize {
    ((x & 31) + (z & 31) * 32) as usize
}

/// Group columns by the region file they belong in.
fn by_region(chunks: &[ChunkColumn]) -> BTreeMap<(i32, i32), Vec<&ChunkColumn>> {
    let mut map: BTreeMap<(i32, i32), Vec<&ChunkColumn>> = BTreeMap::new();
    for chunk in chunks {
        // Arithmetic shift: region -1 holds chunks -32..-1.
        map.entry((chunk.x >> 5, chunk.z >> 5))
            .or_default()
            .push(chunk);
    }
    map
}

/// The non-modelled top-level keys of a chunk already on disk, if it is there.
///
/// # What this is for, and the data loss it closes
///
/// The save path does not round-trip a loaded chunk: it rebuilds the column
/// from the generated canvas and lays the player's overlay on top, because a
/// saved chunk is not expressed relative to anything and the canvas is what
/// the server has. That is right for the blocks and **wrong for everything
/// this crate does not model**. A server pointed at a world vanilla made — a
/// world with villages in it — would, the first time a player broke a block
/// in a chunk, write that chunk back with
/// [`generated_extra`]'s empty `structures`, and the village's own start and
/// reference set would be gone. The blocks would still be there; vanilla
/// would simply no longer know a village was.
///
/// So a save reads this first and seeds the column's [`ChunkColumn::extra`]
/// from it. A chunk that is not on disk yet returns `None` and the generated
/// defaults stand.
///
/// Cheap: it decompresses one record and parses its NBT, and does not touch
/// the other 1,023 slots.
///
/// # Errors
/// If the region file exists and is truncated, or the record will not parse.
pub fn read_chunk_extra(
    region_dir: &Path,
    chunk_x: i32,
    chunk_z: i32,
) -> io::Result<Option<BTreeMap<String, Nbt>>> {
    let path = region_dir.join(format!("r.{}.{}.mca", chunk_x >> 5, chunk_z >> 5));
    let records = read_region_records(&path)?;
    let Some(record) = records.get(&region_index(chunk_x, chunk_z)) else {
        return Ok(None);
    };
    let root = decode_region_record(record)?;
    let mut extra = root.as_compound().cloned().unwrap_or_default();
    for key in MODELLED_CHUNK_KEYS {
        extra.remove(key);
    }
    Ok(Some(extra))
}

/// Write chunks to `region/r.<rx>.<rz>.mca` under `region_dir`, **replacing**
/// each region file it touches.
///
/// Chunks are grouped by the region they fall in, so a caller can hand over
/// a whole generated area without doing the 32x32 arithmetic itself.
///
/// Replace, not merge, and that is the right default *for the worldgen
/// oracle*: a region file left over from an earlier dump that silently
/// survived into this one would contaminate a parity measurement with chunks
/// nothing in this run generated. A save path wants the opposite and must use
/// [`write_region_merging`].
pub fn write_region(region_dir: &Path, chunks: &[ChunkColumn]) -> io::Result<()> {
    std::fs::create_dir_all(region_dir)?;
    for ((rx, rz), region_chunks) in by_region(chunks) {
        let mut records = BTreeMap::new();
        for chunk in region_chunks {
            records.insert(region_index(chunk.x, chunk.z), encode_record(chunk)?);
        }
        let out = lay_out_region(rx, rz, &records)?;
        std::fs::write(region_dir.join(format!("r.{rx}.{rz}.mca")), out)?;
    }
    Ok(())
}

/// Write chunks into `region_dir`, **preserving** every chunk already on disk
/// that is not among them.
///
/// This is the save path's writer. A server that has one modified chunk to
/// flush must not rewrite the other 1,023 slots of its region file, and
/// [`write_region`] would: it lays out only what it is handed and truncates
/// the rest away. A player who built two bases in one region would lose the
/// one they were not standing in.
///
/// Untouched chunks are copied across as [`RawRecord`]s — compressed payload
/// and scheme byte, verbatim — so they survive with their block entities,
/// entities, tick queues and structure references intact. Only the chunks
/// actually handed in are re-encoded through [`encode_chunk`].
///
/// The whole file is still laid out afresh, which compacts it. Vanilla's
/// `RegionFile.write` instead frees and reallocates sectors in place via
/// `RegionBitmap`; the difference is not observable through the read path,
/// which resolves every chunk through the offset table it is handed.
pub fn write_region_merging(region_dir: &Path, chunks: &[ChunkColumn]) -> io::Result<()> {
    std::fs::create_dir_all(region_dir)?;
    for ((rx, rz), region_chunks) in by_region(chunks) {
        let path = region_dir.join(format!("r.{rx}.{rz}.mca"));
        let mut records = read_region_records(&path)?;
        for chunk in region_chunks {
            records.insert(region_index(chunk.x, chunk.z), encode_record(chunk)?);
        }
        let out = lay_out_region(rx, rz, &records)?;
        std::fs::write(&path, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn state(name: &str, props: &[(&str, &str)]) -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("Name".into(), Nbt::String(name.into()));
        if !props.is_empty() {
            let mut p = BTreeMap::new();
            for (k, v) in props {
                p.insert((*k).into(), Nbt::String((*v).into()));
            }
            m.insert("Properties".into(), Nbt::Compound(p));
        }
        Nbt::Compound(m)
    }

    /// Nine bits per entry, seven per long, and **no entry spanning two
    /// longs** — the top bit of each long goes unused. A packer that filled
    /// all 64 bits would produce a file that loads cleanly and reports the
    /// wrong terrain height for five columns in every seven.
    #[test]
    fn a_heightmap_packs_seven_entries_per_long() {
        let mut values = vec![-64; 256];
        // Distinct low values so a misalignment shifts them visibly.
        for (i, v) in values.iter_mut().enumerate().take(8) {
            *v = -64 + i as i32;
        }
        let packed = pack_heightmap(&values, -64);
        assert_eq!(packed.len(), 37, "256 entries at 7 per long, rounded up");
        for i in 0..7 {
            assert_eq!(
                (packed[0] >> (i * 9)) & 0x1FF,
                i as i64,
                "entry {i} of the first long"
            );
        }
        assert_eq!(packed[0] >> 63, 0, "the 64th bit is never used");
        assert_eq!(packed[1] & 0x1FF, 7, "the eighth entry starts a new long");
    }

    /// The stored value is relative to the bottom of the world, and a
    /// column with nothing in it stores zero rather than a negative.
    #[test]
    fn a_heightmap_is_stored_relative_to_the_world_bottom() {
        let mut values = vec![-64; 256];
        values[0] = 54;
        let packed = pack_heightmap(&values, -64);
        assert_eq!(packed[0] & 0x1FF, 118, "54 - (-64)");
        assert_eq!((packed[0] >> 9) & 0x1FF, 0, "an empty column stores zero");
    }

    #[test]
    fn canonical_state_sorts_properties() {
        let s = state(
            "minecraft:oak_stairs",
            &[("half", "top"), ("facing", "east")],
        );
        assert_eq!(
            canonical_block_state(&s).unwrap(),
            "minecraft:oak_stairs[facing=east,half=top]"
        );
    }

    #[test]
    fn container_unpacks_two_entry_palette() {
        // 4096 entries, palette len 2 → bits=max(4,1)=4, 16 per long, 256 longs.
        // Alternate 0,1,0,1… within each long: nibble pattern 0x1010…
        let mut m = BTreeMap::new();
        m.insert(
            "palette".into(),
            Nbt::List(vec![
                state("minecraft:air", &[]),
                state("minecraft:stone", &[]),
            ]),
        );
        m.insert(
            "data".into(),
            Nbt::LongArray(vec![0x1010_1010_1010_1010u64 as i64; 256]),
        );
        let (palette, idx) =
            decode_container(&Nbt::Compound(m), 4096, 4, canonical_block_state).unwrap();
        assert_eq!(palette, vec!["minecraft:air", "minecraft:stone"]);
        assert_eq!(idx.len(), 4096);
        assert_eq!(&idx[..4], &[0, 1, 0, 1]);
    }

    #[test]
    fn single_entry_palette_needs_no_data() {
        let mut m = BTreeMap::new();
        m.insert(
            "palette".into(),
            Nbt::List(vec![state("minecraft:air", &[])]),
        );
        let (palette, idx) =
            decode_container(&Nbt::Compound(m), 4096, 4, canonical_block_state).unwrap();
        assert_eq!(palette.len(), 1);
        assert!(idx.is_empty());
    }

    /// A chunk with a two-entry block palette, alternating, plus biomes —
    /// enough that every encoder branch is exercised.
    fn sample_chunk(x: i32, z: i32) -> ChunkColumn {
        let sections = (-4..0i8)
            .map(|y| Section {
                y,
                palette: vec!["minecraft:air".into(), "minecraft:stone".into()],
                blocks: (0..4096).map(|i| u16::from(i % 3 == 0)).collect(),
                biome_palette: vec!["minecraft:plains".into(), "minecraft:desert".into()],
                biomes: (0..64).map(|i| u8::from(i % 5 == 0)).collect(),
                extra: BTreeMap::new(),
            })
            .collect();
        ChunkColumn {
            x,
            z,
            status: "minecraft:noise".into(),
            data_version: 4325,
            sections,
            is_light_on: false,
            light: BTreeMap::new(),
            block_entities: Vec::new(),
            heightmaps: BTreeMap::new(),
            extra: generated_extra(),
            structure_starts: Vec::new(),
            structure_references: BTreeMap::new(),
        }
    }

    fn round_trip(chunks: &[ChunkColumn]) -> Vec<ChunkColumn> {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-anvil-{}-{}",
            std::process::id(),
            chunks.first().map_or(0, |c| c.x)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        write_region(&dir, chunks).expect("write");
        let mut out = Vec::new();
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        for file in files {
            out.extend(read_region(&file).expect("read"));
        }
        let _ = std::fs::remove_dir_all(&dir);
        out.sort_by_key(|c| (c.x, c.z));
        out
    }

    #[test]
    fn a_chunk_survives_a_write_read_round_trip() {
        let original = sample_chunk(1, 2);
        let back = round_trip(std::slice::from_ref(&original));
        assert_eq!(back.len(), 1);
        let got = &back[0];
        assert_eq!((got.x, got.z), (original.x, original.z));
        assert_eq!(got.status, original.status);
        assert_eq!(got.data_version, original.data_version);
        assert_eq!(got.sections.len(), original.sections.len());
        for (a, b) in got.sections.iter().zip(&original.sections) {
            assert_eq!(a.y, b.y);
            assert_eq!(a.palette, b.palette);
            assert_eq!(a.blocks, b.blocks);
            assert_eq!(a.biome_palette, b.biome_palette);
            assert_eq!(a.biomes, b.biomes);
        }
    }

    /// This crate does not know what a chest is, so the proof it does not
    /// need to is a compound that round-trips exactly as it went in —
    /// nested `Items` list included. That is also why the compound below
    /// deliberately does not use any of the fields [`decode_chunk`] itself
    /// reads (`Y`, `sections`, `xPos`...): a bug that only preserved keys
    /// this module already knows about would pass a weaker version of this
    /// test.
    #[test]
    fn a_block_entity_survives_a_write_read_round_trip() {
        let mut chunk = sample_chunk(5, -6);

        let mut item = BTreeMap::new();
        item.insert("Slot".into(), Nbt::Byte(0));
        item.insert("id".into(), Nbt::String("minecraft:stone".into()));
        item.insert("count".into(), Nbt::Int(1));

        let mut chest = BTreeMap::new();
        chest.insert("id".into(), Nbt::String("minecraft:chest".into()));
        chest.insert("x".into(), Nbt::Int(chunk.x * 16 + 3));
        chest.insert("y".into(), Nbt::Int(-60));
        chest.insert("z".into(), Nbt::Int(chunk.z * 16 + 12));
        chest.insert("Items".into(), Nbt::List(vec![Nbt::Compound(item)]));
        chunk.block_entities = vec![Nbt::Compound(chest)];

        let back = round_trip(std::slice::from_ref(&chunk));
        assert_eq!(back.len(), 1);
        let got = &back[0];

        assert_eq!(
            got.block_entities, chunk.block_entities,
            "the block entity compound did not survive the round trip"
        );

        // The equality above already covers this, but pinned separately so
        // a future refactor that broke only the nested list — a much easier
        // mistake than losing the whole compound — fails with a message
        // that says so rather than a generic structural diff.
        let items = got.block_entities[0]
            .get("Items")
            .and_then(Nbt::as_list)
            .expect("the chest kept its Items list");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].get("id").and_then(Nbt::as_str),
            Some("minecraft:stone")
        );
        assert_eq!(items[0].get("count").and_then(Nbt::as_i32), Some(1));
    }

    #[test]
    fn chunks_are_split_into_the_right_region_files() {
        // Region coordinates are chunk >> 5, an arithmetic shift, so chunk
        // -1 belongs to region -1 and not region 0. Truncating division here
        // would put a whole quadrant of the world in the wrong file.
        let chunks = vec![
            sample_chunk(0, 0),
            sample_chunk(-1, -1),
            sample_chunk(33, 2),
        ];
        let back = round_trip(&chunks);
        assert_eq!(back.len(), 3, "a chunk went missing across region files");
        let positions: Vec<(i32, i32)> = back.iter().map(|c| (c.x, c.z)).collect();
        assert_eq!(positions, vec![(-1, -1), (0, 0), (33, 2)]);
    }

    #[test]
    fn the_header_is_two_full_sectors_and_payloads_are_aligned() {
        let dir = std::env::temp_dir().join(format!("rustmc-anvil-hdr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_region(&dir, &[sample_chunk(0, 0)]).expect("write");
        let bytes = std::fs::read(dir.join("r.0.0.mca")).expect("read back");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(bytes.len() >= 8192, "header tables are 4 KiB each");
        assert_eq!(bytes.len() % 4096, 0, "file is not sector-padded");

        // Chunk (0,0) is index 0; every other slot must be absent.
        let entry = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
        assert_eq!(entry >> 8, 2, "payloads start after the two header sectors");
        assert!(entry & 0xFF > 0, "sector count must be non-zero");
        for i in 1..1024 {
            let other = u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
            assert_eq!(other, 0, "slot {i} should be empty");
        }
    }

    #[test]
    fn a_single_entry_palette_writes_no_data_array() {
        // An all-air section is the common case at the top of every chunk,
        // and vanilla stores it as a bare palette. Emitting 256 zero longs
        // instead would be legal but would inflate every region file.
        let container = encode_container(
            &["minecraft:air".to_string()],
            &[],
            4096,
            4,
            block_state_tag,
        );
        assert!(container.get("palette").is_some());
        assert!(
            container.get("data").is_none(),
            "single-entry palettes carry no packed data"
        );
    }

    #[test]
    fn packing_does_not_span_long_boundaries() {
        // 4096 entries with a 3-entry palette needs 4 bits (the minimum for
        // blocks), so 16 per long and 256 longs — not the 192 a dense
        // bit-stream would use. Getting this wrong loads fine and is wrong
        // wherever a boundary falls.
        let palette: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_string()).collect();
        let indices: Vec<u16> = (0..4096).map(|i| (i % 3) as u16).collect();
        let container = encode_container(&palette, &indices, 4096, 4, block_state_tag);
        let data = container
            .get("data")
            .and_then(Nbt::as_long_array)
            .expect("data");
        assert_eq!(data.len(), 256);

        // And it must read back as what went in.
        let (back_palette, back) =
            decode_container(&container, 4096, 4, canonical_block_state).expect("decode");
        assert_eq!(back_palette, palette);
        assert_eq!(back, indices);
    }

    #[test]
    fn a_state_with_properties_round_trips_through_the_palette() {
        let canonical = "minecraft:oak_stairs[facing=east,half=top]";
        let tag = block_state_tag(canonical);
        assert_eq!(canonical_block_state(&tag).unwrap(), canonical);
    }

    /// **A structure start survives a rewrite that rebuilds the column.**
    ///
    /// The shape of the loss this closes: take a chunk vanilla wrote with a
    /// `structures.starts` entry in it, rebuild the column the way the save
    /// path does (from the generator, so `extra` is `generated_extra()` and
    /// `structures` is empty), and write it back. Without
    /// [`read_chunk_extra`] the entry is gone; with it the entry is there and
    /// the *blocks* are the rebuilt ones, which is exactly the split the save
    /// path wants.
    ///
    /// The first half is the control: the naive rebuild really does lose it,
    /// so the second is not passing for free.
    #[test]
    fn a_saved_chunks_structures_survive_a_rebuild() {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-anvil-extra-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        // A chunk as vanilla would have left it: a village starts here.
        let mut original = sample_chunk(4, 4);
        let mut starts = BTreeMap::new();
        let mut village = BTreeMap::new();
        village.insert("id".to_string(), Nbt::String("minecraft:village".into()));
        village.insert("ChunkX".to_string(), Nbt::Int(4));
        village.insert("ChunkZ".to_string(), Nbt::Int(4));
        village.insert("Children".to_string(), Nbt::List(Vec::new()));
        starts.insert("minecraft:village".to_string(), Nbt::Compound(village));
        let mut structures = BTreeMap::new();
        structures.insert("starts".to_string(), Nbt::Compound(starts));
        structures.insert("References".to_string(), Nbt::Compound(BTreeMap::new()));
        original
            .extra
            .insert("structures".to_string(), Nbt::Compound(structures));
        original
            .extra
            .insert("InhabitedTime".to_string(), Nbt::Long(9_001));
        write_region_merging(&dir, std::slice::from_ref(&original)).expect("write the original");

        let has_village = |column: &ChunkColumn| {
            column
                .extra
                .get("structures")
                .and_then(|s| s.get("starts"))
                .and_then(Nbt::as_compound)
                .is_some_and(|m| m.contains_key("minecraft:village"))
        };

        // The control: a naive rebuild loses it.
        let naive = sample_chunk(4, 4);
        assert!(!has_village(&naive), "the rebuilt column has no village");
        write_region_merging(&dir, std::slice::from_ref(&naive)).expect("naive rewrite");
        let after_naive = read_region(&dir.join("r.0.0.mca")).expect("read");
        assert!(
            !after_naive.iter().any(has_village),
            "control: the naive rewrite was supposed to lose the village"
        );

        // And the repair.
        let _ = std::fs::remove_dir_all(&dir);
        write_region_merging(&dir, std::slice::from_ref(&original)).expect("write the original");
        let mut rebuilt = sample_chunk(4, 4);
        rebuilt.extra = read_chunk_extra(&dir, 4, 4)
            .expect("read the saved chunk")
            .expect("it is on disk");
        write_region_merging(&dir, std::slice::from_ref(&rebuilt)).expect("repaired rewrite");
        let after = read_region(&dir.join("r.0.0.mca")).expect("read");
        let column = after
            .iter()
            .find(|c| (c.x, c.z) == (4, 4))
            .expect("present");
        assert!(has_village(column), "the village survived the rebuild");
        assert_eq!(
            column.extra.get("InhabitedTime"),
            Some(&Nbt::Long(9_001)),
            "and so did everything else the rebuild does not model"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A chunk that is not on disk is `None`, not an error — that is a first
    /// save and it is the common case.
    #[test]
    fn read_chunk_extra_on_a_missing_chunk_is_none() {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-anvil-extra-missing-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_chunk_extra(&dir, 0, 0).expect("no directory"), None);
        std::fs::create_dir_all(&dir).expect("mkdir");
        write_region_merging(&dir, &[sample_chunk(0, 0)]).expect("write");
        assert!(read_chunk_extra(&dir, 0, 0).expect("present").is_some());
        assert_eq!(
            read_chunk_extra(&dir, 1, 0).expect("absent slot"),
            None,
            "a slot in an existing region file that holds no chunk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A real vanilla region, decoded and re-encoded, is byte-identical to
    /// vanilla's own chunk payloads.**
    ///
    /// This is the golden the brief for this work named and the one the
    /// rewrite test above is not: `real_vanilla_chunks_survive_a_rewrite`
    /// compares palettes and block indices through *our* reader, so a field
    /// this struct drops on the way in and writes as a constant on the way
    /// out is invisible to it. Comparing the encoded NBT payload against
    /// vanilla's own bytes is not: every top-level key, every section key,
    /// every tag width, the bit packing, the palette order, the heightmap
    /// offset and the `java.util.HashMap` key order are all in it.
    ///
    /// Three defects were found by turning it on, and all three had been
    /// shipping:
    ///
    /// * every `Heightmaps` entry came back **64 too high**, because the
    ///   decoder read the stored value and `pack_heightmap` subtracts `min_y`
    ///   on the way out while `Heightmap.setHeight` had already done so;
    /// * `block_entities` was omitted when empty, where vanilla writes `[]`;
    /// * `LastUpdate`, `InhabitedTime`, `isLightOn`, `block_ticks`,
    ///   `fluid_ticks`, `PostProcessing` and `structures` were re-written as
    ///   constants, so a saved world lost its tick queues and its structure
    ///   references on the first rewrite.
    ///
    /// It runs over every chunk of every harvested region rather than one,
    /// because the three chunk statuses in a harvest carry different key sets
    /// and a single sample would exercise one of them.
    #[test]
    fn a_real_vanilla_region_re_encodes_byte_for_byte() {
        let harvest = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work"));
        if !rustmc_data::fixture::available(
            harvest,
            "the vanilla world harvest",
            "ci/gen-vanilla-region.sh",
        ) {
            return;
        }
        let mut regions = Vec::new();
        for entry in std::fs::read_dir(harvest)
            .expect("read oracle-work")
            .flatten()
        {
            let dir = entry
                .path()
                .join("world/dimensions/minecraft/overworld/region");
            let Ok(files) = std::fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e == "mca")
                    && std::fs::metadata(&path).is_ok_and(|m| m.len() > 8192)
                {
                    regions.push(path);
                }
            }
        }
        regions.sort();
        assert!(!regions.is_empty(), "no vanilla regions under {harvest:?}");

        // **The first cut of this bounded the walk to two regions and passed
        // over 161 chunks, every one of them `minecraft:structure_starts`.**
        // Those are the harvest's margin chunks: empty heightmaps, air-only
        // single-entry palettes, no `isLightOn`. So it exercised neither the
        // bit packing nor the heightmap offset nor the light flag — a gate the
        // window could not fail. The walk now runs until it has seen a
        // `minecraft:full` chunk and a quorum of chunks that actually carry
        // heightmaps, and asserts both at the end.
        let mut chunks = 0;
        let mut with_heightmaps = 0;
        let mut statuses = std::collections::BTreeSet::new();
        for path in &regions {
            if chunks >= 300 && with_heightmaps >= 20 && statuses.contains("minecraft:full") {
                break;
            }
            for record in read_region_records(path).expect("records").values() {
                let root = decode_region_record(record).expect("decode");
                let column = decode_chunk(&root).expect("parse");
                statuses.insert(column.status.clone());
                if !column.heightmaps.is_empty() {
                    with_heightmaps += 1;
                }
                let want = nbt::encode_named_root("", &root).expect("vanilla's own, re-encoded");
                let ours = nbt::encode_named_root("", &encode_chunk(&column)).expect("ours");
                assert_eq!(
                    ours.len(),
                    want.len(),
                    "{} chunk ({}, {}) [{}]: vanilla {} bytes, ours {}",
                    path.display(),
                    column.x,
                    column.z,
                    column.status,
                    want.len(),
                    ours.len()
                );
                assert_eq!(
                    ours,
                    want,
                    "{} chunk ({}, {}) [{}]",
                    path.display(),
                    column.x,
                    column.z,
                    column.status
                );
                chunks += 1;
            }
        }
        assert!(chunks >= 100, "only {chunks} chunks compared");
        assert!(
            statuses.contains("minecraft:full"),
            "no `minecraft:full` chunk was compared; statuses seen {statuses:?}. \
             A structure_starts chunk has empty heightmaps and single-entry \
             palettes, so a run that saw only those tested neither."
        );
        assert!(
            with_heightmaps >= 20,
            "only {with_heightmaps} chunks carried heightmaps; the +min_y offset \
             is untested below that"
        );
        eprintln!(
            "{chunks} vanilla chunks re-encoded identically ({with_heightmaps} with \
             heightmaps); statuses {statuses:?}"
        );
    }
    /// # The writer against a real vanilla region
    ///
    /// The round-trip tests above prove the writer is consistent with our own
    /// reader, which is weaker than it sounds — a lenient reader could accept
    /// bytes vanilla would not. This one takes a region **vanilla actually
    /// wrote**, reads it, writes it back out, and reads it again, asserting
    /// every chunk survives unchanged.
    ///
    /// That covers the cases synthetic fixtures miss: real palette sizes
    /// (1, 2, and everything up to a full section's worth), sections that
    /// legitimately have no biome container, chunks large enough to need
    /// several sectors, and whatever mix of statuses a generated region
    /// carries.
    ///
    /// **This test skipped in every gate run on the Threadripper for its whole
    /// life**, and the gate could not tell. `oracle-work/t2-regiondiff/` did
    /// not exist on that box, the early return below made the test *pass*, and
    /// `cargo` captures a passing test's stderr — so `ci/handoff.sh`'s
    /// `skipping: no vanilla` guard never saw a byte of it. The only check this
    /// project had against real vanilla writer output was decorative for its
    /// entire existence, hidden inside the mechanism that certifies everything
    /// else. See `rustmc_data::fixture` for the whole account.
    ///
    /// A missing fixture is now a **failure** by default;
    /// `RUSTMC_FIXTURES=optional` is the explicit opt-out for a clean clone,
    /// and under it `ci/t0.sh` prints a sentinel no gate greps for. The region
    /// itself is Mojang-derived and gitignored, produced by
    /// `ci/t2-regiondiff.sh`, and now lives in the shared reference directory
    /// `ci/handoff.sh` stage 1 links every gate directory from.
    #[test]
    fn real_vanilla_chunks_survive_a_rewrite() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/t2-regiondiff/a/r.0.0.mca"
        ));
        if !rustmc_data::fixture::available(fixture, "vanilla region", "ci/t2-regiondiff.sh") {
            return;
        }

        let original = read_region(fixture).expect("read the vanilla region");
        assert!(!original.is_empty(), "the fixture region has no chunks");

        let dir = std::env::temp_dir().join(format!("rustmc-anvil-vanilla-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_region(&dir, &original).expect("write");
        let mut rewritten = read_region(&dir.join("r.0.0.mca")).expect("read back");
        let _ = std::fs::remove_dir_all(&dir);

        let mut original = original;
        original.sort_by_key(|c| (c.x, c.z));
        rewritten.sort_by_key(|c| (c.x, c.z));
        assert_eq!(
            rewritten.len(),
            original.len(),
            "chunk count changed across the rewrite"
        );

        let mut palette_sizes = std::collections::BTreeSet::new();
        for (before, after) in original.iter().zip(&rewritten) {
            assert_eq!((after.x, after.z), (before.x, before.z));
            assert_eq!(after.data_version, before.data_version);
            assert_eq!(after.status, before.status);
            assert_eq!(
                after.sections.len(),
                before.sections.len(),
                "section count changed in chunk ({}, {})",
                before.x,
                before.z
            );
            for (a, b) in after.sections.iter().zip(&before.sections) {
                palette_sizes.insert(b.palette.len());
                assert_eq!(a.y, b.y);
                assert_eq!(a.palette, b.palette, "palette changed at section {}", b.y);
                assert_eq!(a.blocks, b.blocks, "blocks changed at section {}", b.y);
                assert_eq!(a.biome_palette, b.biome_palette);
                assert_eq!(a.biomes, b.biomes);
            }
        }

        // The fixture has to be varied enough for the assertions to mean
        // something — a region of nothing but single-entry palettes would
        // pass without touching the bit-packing at all.
        assert!(
            palette_sizes.iter().any(|n| *n > 2),
            "the fixture only contains trivial palettes {palette_sizes:?}; \
             the packed path was never exercised"
        );
        eprintln!(
            "rewrote {} real vanilla chunks; palette sizes seen: {:?}",
            original.len(),
            palette_sizes
        );
    }
    /// **`generated_extra` writes no `isLightOn`, because vanilla writes none
    /// for a chunk whose light it has not computed.**
    ///
    /// `SerializableChunkData.copyOf` guards the write with `ifeq` on
    /// `lightCorrect`; the read is `getBooleanOr("isLightOn", false)`. So an
    /// absent key and an explicit `Byte(0)` are the same chunk to vanilla and
    /// two different files on disk, and this crate's claim is about the file.
    ///
    /// It is a test rather than a comment because the byte-identity golden
    /// cannot reach it: every chunk that golden compares comes off a vanilla
    /// disk with the light computed, so `extra` carries `isLightOn: 1` and
    /// `generated_extra`'s value is never exercised. This is the one
    /// assertion in the crate that covers a generated chunk's light flag.
    ///
    /// The second half is the part that keeps it honest. Asserting only the
    /// absence passes on a `generated_extra` that returns an empty map, which
    /// would silently drop `structures` and delete every village reference in
    /// the world.
    #[test]
    fn generated_extra_omits_the_light_flag_vanilla_omits() {
        let extra = generated_extra();
        assert!(
            !extra.contains_key("isLightOn"),
            "a generated chunk has lightCorrect = false, and vanilla writes \
             no isLightOn at all for one"
        );
        for key in [
            "LastUpdate",
            "InhabitedTime",
            "block_ticks",
            "fluid_ticks",
            "PostProcessing",
            "structures",
        ] {
            assert!(extra.contains_key(key), "{key} went missing too");
        }
    }
}
