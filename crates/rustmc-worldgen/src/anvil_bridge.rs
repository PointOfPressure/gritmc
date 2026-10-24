//! Generated chunk to on-disk chunk.
//!
//! Turns a [`FilledChunk`] — the density layer's small closed vocabulary of
//! substances — into the palette-and-indices form
//! [`rustmc_world::anvil`] writes. This is the seam where the generator stops
//! speaking in "solid / water / vein" and starts speaking in block ids.
//!
//! # Why the mapping lives here and not in a registry
//!
//! `rustmc-data`'s block registry does not exist yet. Until it does, this
//! table is the only place a `Substance` becomes a name, and it is
//! deliberately exhaustive rather than defaulted: adding a substance without
//! deciding its block should be a compile error, not a silently-stone block
//! somewhere in the world.
//!
//! `Substance::Solid` maps to the *overworld's* default block. Other
//! dimensions use a different one (the nether uses netherrack), which is why
//! it is a parameter and not a constant.

use crate::chunk_gen::FilledChunk;
use crate::density::fill::Substance;
use crate::density::ore_vein::{VeinBlock, VeinType};
use rustmc_world::anvil::{ChunkColumn, Section};

/// The data version this writes into chunk NBT.
///
/// Vanilla refuses to load a chunk from a *newer* version and runs DataFixerUpper
/// on an older one, so a wrong value here is the difference between a world
/// that opens and one that is silently upgraded.
///
/// **This was 4325 and 26.2's value is 4903**, so every chunk this repo ever
/// wrote claimed to be 578 versions old and would have been run through
/// DataFixerUpper on load. Nothing noticed, because until
/// `ci/t2-vanilla-loads-ours.sh` nothing had ever handed a region we wrote to
/// vanilla — our own reader takes the field at face value, and the differ
/// compares our output against vanilla's *blocks*, never its version. The
/// doc comment above stated the consequence correctly the whole time.
///
/// Two independent sources for 4903, agreeing:
///
/// * `SharedConstants.WORLD_VERSION = 4903`, read from the constant pool of
///   the pinned jar (`javap -p -c -constants net/minecraft/SharedConstants`).
/// * the `DataVersion` field of a chunk vanilla itself wrote, decoded out of
///   `oracle-work/t2-regiondiff/a/r.0.0.mca` without going through our
///   reader.
///
/// [`tests::the_data_version_matches_the_one_vanilla_writes`] pins it against
/// the second of those, so drifting from the jar is a test failure rather
/// than a discovery made years later.
pub const DATA_VERSION_26_2: i32 = 4903;

/// Vanilla's chunk status after the noise stage and nothing more.
///
/// Honest labelling matters: marking a chunk `minecraft:full` when it has no
/// surfaces, carvers or features tells the server not to finish it, and the
/// world is permanently missing everything after this stage.
pub const STATUS_NOISE: &str = "minecraft:noise";

/// The block name for each substance.
///
/// # Why this borrows rather than allocating
///
/// Every name it can return is either `'static` or the caller's own
/// `default_block`, so there was never anything to own. Returning a `String`
/// cost one heap allocation **per block of the world** — 98,304 a chunk,
/// 134 million over the r=16 square the perf harness generates — and the
/// noise stage threw most of them away immediately, because it allocates
/// `"minecraft:air"` only to compare it and skip. Borrowing is the same
/// bytes with none of that.
#[must_use]
pub fn block_name(substance: Substance, default_block: &str) -> &str {
    match substance {
        Substance::Solid => default_block,
        Substance::Air => "minecraft:air",
        Substance::Water => "minecraft:water[level=0]",
        Substance::Lava => "minecraft:lava[level=0]",
        Substance::Vein(vein) => vein_block_name(vein),
    }
}

const fn vein_block_name(vein: VeinBlock) -> &'static str {
    match vein {
        VeinBlock::Ore(VeinType::Copper) => "minecraft:copper_ore",
        VeinBlock::RawOreBlock(VeinType::Copper) => "minecraft:raw_copper_block",
        VeinBlock::Filler(VeinType::Copper) => "minecraft:granite",
        VeinBlock::Ore(VeinType::Iron) => "minecraft:deepslate_iron_ore",
        VeinBlock::RawOreBlock(VeinType::Iron) => "minecraft:raw_iron_block",
        VeinBlock::Filler(VeinType::Iron) => "minecraft:tuff",
    }
}

/// Convert a generated chunk into the on-disk form.
///
/// `biomes` supplies the biome name for each 4x4x4 cell, indexed
/// `(cell_y * 4 + cell_z) * 4 + cell_x` within a section — vanilla's own
/// order. Pass `None` to omit biome containers entirely, which is valid on
/// disk and is what a chunk that has not reached the biome stage looks like.
#[must_use]
pub fn to_chunk_column(
    chunk: &FilledChunk,
    default_block: &str,
    biomes: Option<&dyn Fn(i32, i32, i32) -> String>,
) -> ChunkColumn {
    let section_count = chunk.height / 16;
    let first_section = chunk.min_y >> 4;

    let sections: Vec<Section> = (0..section_count)
        .map(|s| {
            let section_y = first_section + s;
            let base_y = section_y * 16;

            // Palette entries are discovered in the order blocks are visited,
            // which is vanilla's y-then-z-then-x order within a section. The
            // *set* is what matters for correctness; the order only decides
            // which index a block gets, and both sides read it back through
            // the same palette.
            let mut palette: Vec<String> = Vec::new();
            let mut blocks = Vec::with_capacity(4096);
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let name = block_name(chunk.get(x, base_y + y, z), default_block);
                        let index = palette.iter().position(|p| p == name).unwrap_or_else(|| {
                            palette.push(name.to_string());
                            palette.len() - 1
                        });
                        blocks.push(index as u16);
                    }
                }
            }

            let (biome_palette, biome_indices) = match biomes {
                None => (Vec::new(), Vec::new()),
                Some(lookup) => {
                    let mut biome_palette: Vec<String> = Vec::new();
                    let mut biome_indices = Vec::with_capacity(64);
                    for cell_y in 0..4 {
                        for cell_z in 0..4 {
                            for cell_x in 0..4 {
                                let name = lookup(cell_x, (base_y >> 2) + cell_y, cell_z);
                                let index = biome_palette
                                    .iter()
                                    .position(|p| *p == name)
                                    .unwrap_or_else(|| {
                                        biome_palette.push(name);
                                        biome_palette.len() - 1
                                    });
                                biome_indices.push(index as u8);
                            }
                        }
                    }
                    (biome_palette, biome_indices)
                }
            };

            Section {
                y: section_y as i8,
                palette,
                blocks,
                biome_palette,
                biomes: biome_indices,
                // No light arrays: this crate computes no light. A section
                // read from disk carries its `BlockLight`/`SkyLight` here and
                // a generated one has none, which is what vanilla writes for
                // a chunk whose light has not been lit.
                extra: std::collections::BTreeMap::new(),
            }
        })
        .collect();

    ChunkColumn {
        // Derived from the FINISHED block states, which is what vanilla's
        // own answer is a function of: `LevelChunk.setBlockState` creates a
        // block entity for any `EntityBlock` state it stores and drops it
        // when that position is overwritten, so at save time the list is a
        // statement about the final states and nothing else.
        //
        // Running it here rather than inside a feature is what makes it
        // unable to perturb generation: the canvas is already final, so no
        // draw is taken and no block state can move. Only the types
        // `crate::block_entity` can reproduce exactly are emitted, and its
        // header says which those are and why the rest are absent — the
        // absence means `not yet reproduced`, never `needs none`.
        // No channel here, and that is a fact about the input rather than a
        // gap. A `FilledChunk` is the noise stage's output: no feature has run
        // over it, so nothing has placed a container and there is nothing for
        // a placer to have recorded. The one class this derives — a block
        // entity that is a function of the finished state — is all a
        // pre-decoration column can carry.
        block_entities: crate::block_entity::from_sections(&sections, chunk.chunk_x, chunk.chunk_z),
        // Filled by the caller through `heightmaps_from_canvas`: computing
        // them needs the block-property table, and this crate deliberately
        // depends on nothing that would carry one.
        heightmaps: std::collections::BTreeMap::new(),
        // `LastUpdate`, `InhabitedTime`, `isLightOn`, the two tick queues,
        // `PostProcessing` and `structures`, at the values a freshly
        // generated chunk has. Named through one constructor so that this
        // bridge and the save path cannot disagree about what a fresh chunk
        // carries.
        extra: rustmc_world::anvil::generated_extra(),
        // This generator does not place structures yet, so a column it
        // builds records none. Named rather than defaulted: the day
        // structures land, every writer has to decide what goes here.
        structure_starts: Vec::new(),
        // Nor references: writing a reference set that nothing computed
        // would make the diff read as agreement where there is none.
        structure_references: std::collections::BTreeMap::new(),
        x: chunk.chunk_x,
        z: chunk.chunk_z,
        status: STATUS_NOISE.to_string(),
        data_version: DATA_VERSION_26_2,
        sections,
        is_light_on: false,
        light: std::collections::BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_gen::{FillInputs, FillSettings, NoiseFill};

    /// [`DATA_VERSION_26_2`], against a chunk vanilla itself wrote.
    ///
    /// The constant was wrong by 578 for as long as it has existed and no
    /// test could see it, because every consumer either wrote the field and
    /// never read it back or read it back with the same reader that wrote it.
    /// The fixture breaks that loop: its `DataVersion` was chosen by the
    /// pinned server jar, not by us.
    ///
    /// Deliberately asserted against the **fixture** rather than a literal
    /// repeated from the jar. A literal here would be a second copy of the
    /// number in the same repo, so a version bump that updated one and not
    /// the other would still pass — which is the shape of the bug this is
    /// here to stop.
    ///
    /// A missing fixture **fails** rather than skips, matching
    /// [`rustmc_world::anvil`]'s own vanilla-fixture test — which skipped
    /// invisibly in every gate on the box for its whole life, because a passing
    /// test's stderr never leaves `cargo`. `RUSTMC_FIXTURES=optional` is the
    /// opt-out for a clean clone; see `rustmc_data::fixture`. The region is
    /// Mojang-derived, gitignored, and produced by `ci/t2-regiondiff.sh`.
    #[test]
    fn the_data_version_matches_the_one_vanilla_writes() {
        let fixture = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/t2-regiondiff/a/r.0.0.mca"
        ));
        if !rustmc_data::fixture::available(fixture, "vanilla region", "ci/t2-regiondiff.sh") {
            return;
        }
        let chunks = rustmc_world::anvil::read_region(fixture).expect("read the vanilla region");
        assert!(!chunks.is_empty(), "the fixture region has no chunks");

        // Every chunk, not the first: a region whose chunks disagree with each
        // other about their version is a fixture that cannot pin anything, and
        // asserting on `chunks[0]` alone would not say so.
        let versions: std::collections::BTreeSet<i32> =
            chunks.iter().map(|c| c.data_version).collect();
        assert_eq!(
            versions.len(),
            1,
            "the fixture's chunks carry several data versions {versions:?}"
        );
        let vanilla = *versions.iter().next().expect("one version");
        assert_eq!(
            DATA_VERSION_26_2, vanilla,
            "we write DataVersion {DATA_VERSION_26_2} into chunk NBT and vanilla \
             writes {vanilla}; a world we saved would be run through \
             DataFixerUpper on load"
        );
    }

    use crate::density::aquifer::AquiferNoises;
    use crate::density::build;
    use crate::rng::XoroshiroRandomSource;

    fn flat(density: f64) -> FilledChunk {
        NoiseFill::new(
            FillInputs {
                final_density: build::constant(density),
                aquifer_noises: AquiferNoises {
                    barrier: build::constant(0.0),
                    fluid_level_floodedness: build::constant(1.0),
                    fluid_level_spread: build::constant(0.0),
                    lava: build::constant(0.0),
                    erosion: build::constant(0.0),
                    depth: build::constant(0.0),
                },
                preliminary_surface_level: build::constant(64.0),
                aquifer_random: XoroshiroRandomSource::from_seed(1).fork_positional(),
                ore_veinifier: None,
                // No beard: this fixture predates structures entirely.
                beardifier: None,
            },
            FillSettings {
                ore_veins_enabled: false,
                ..FillSettings::overworld()
            },
            0,
            0,
        )
        .fill()
    }

    #[test]
    fn the_section_grid_covers_the_whole_column() {
        let column = to_chunk_column(&flat(1.0), "minecraft:stone", None);
        // -64..320 is 24 sections, the lowest indexed -4.
        assert_eq!(column.sections.len(), 24);
        assert_eq!(column.sections[0].y, -4);
        assert_eq!(column.sections[23].y, 19);
        for section in &column.sections {
            assert_eq!(section.blocks.len(), 4096);
        }
    }

    #[test]
    fn a_uniform_section_collapses_to_one_palette_entry() {
        // Which is what makes an all-stone or all-air section nearly free on
        // disk — the writer then emits no packed data at all.
        let column = to_chunk_column(&flat(1.0), "minecraft:stone", None);
        for section in &column.sections {
            assert_eq!(section.palette, vec!["minecraft:stone".to_string()]);
        }
    }

    #[test]
    fn fluids_carry_their_level_property() {
        // `minecraft:water` and `minecraft:water[level=0]` are different
        // palette entries on disk; vanilla writes the second. Dropping the
        // property makes every ocean a distinct block state from vanilla's.
        let column = to_chunk_column(&flat(-1.0), "minecraft:stone", None);
        let names: std::collections::BTreeSet<&str> = column
            .sections
            .iter()
            .flat_map(|s| s.palette.iter().map(String::as_str))
            .collect();
        assert!(names.contains("minecraft:water[level=0]"), "{names:?}");
        assert!(names.contains("minecraft:lava[level=0]"), "{names:?}");
    }

    #[test]
    fn the_status_says_what_the_chunk_actually_is() {
        // Not `minecraft:full`. Claiming a finished chunk that has no
        // surfaces or features tells the server not to finish it, and the
        // world is permanently missing every later stage.
        let column = to_chunk_column(&flat(1.0), "minecraft:stone", None);
        assert_eq!(column.status, "minecraft:noise");
    }

    #[test]
    fn biomes_are_written_at_quart_resolution() {
        let lookup = |x: i32, y: i32, z: i32| {
            if y < 0 {
                "minecraft:deep_dark".to_string()
            } else {
                format!("minecraft:plains_{x}_{z}")
            }
        };
        let column = to_chunk_column(&flat(1.0), "minecraft:stone", Some(&lookup));
        for section in &column.sections {
            assert_eq!(section.biomes.len(), 64, "4x4x4 cells per section");
            assert!(!section.biome_palette.is_empty());
        }
        // The lowest section is entirely below y=0, so it is all deep dark.
        assert_eq!(
            column.sections[0].biome_palette,
            vec!["minecraft:deep_dark".to_string()]
        );
    }

    #[test]
    fn a_generated_chunk_round_trips_through_the_region_writer() {
        // The whole path in one assertion: generate, convert, write, read.
        let column = to_chunk_column(&flat(1.0), "minecraft:stone", None);
        let dir = std::env::temp_dir().join(format!("rustmc-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        rustmc_world::anvil::write_region(&dir, std::slice::from_ref(&column)).expect("write");
        let back = rustmc_world::anvil::read_region(&dir.join("r.0.0.mca")).expect("read");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(back.len(), 1);
        assert_eq!(back[0].sections.len(), column.sections.len());
        assert_eq!(back[0].status, column.status);
        assert_eq!(back[0].data_version, column.data_version);
    }
}

/// Convert a canvas chunk to the on-disk form.
///
/// The counterpart of [`to_chunk_column`], which reads a noise-stage
/// `FilledChunk` of `Substance`. A canvas has been through every stage and so
/// carries block *states* — `minecraft:oak_leaves[distance=1,...]` — and the
/// two must never be used for the same chunk, because the first knows nothing
/// about anything a surface rule or a feature wrote.
///
/// Sections are rebuilt with their own palettes rather than sharing the
/// chunk's: a chunk's palette spans 384 blocks of world and a section's spans
/// sixteen, and writing the wide one into every section would inflate the
/// region file several-fold for no gain. Vanilla's own sections are
/// per-section too.
///
/// # Panics
/// If the canvas has no such chunk.
#[must_use]
pub fn column_from_canvas(
    canvas: &crate::canvas::Canvas,
    chunk_x: i32,
    chunk_z: i32,
    status: &str,
    data_version: i32,
) -> ChunkColumn {
    let (palette, ids) = canvas
        .chunk_data(chunk_x, chunk_z)
        .expect("the canvas holds this chunk");
    let (biome_palette, biome_ids) = canvas
        .chunk_biomes(chunk_x, chunk_z)
        .expect("the canvas holds this chunk");

    let min_y = canvas.min_y();
    let section_count = canvas.height() / 16;
    let first_section = min_y >> 4;

    let sections: Vec<Section> = (0..section_count)
        .map(|s| {
            let base = usize::try_from(s * 16 * 256).expect("a positive index");
            let mut section_palette: Vec<String> = Vec::new();
            let mut blocks = Vec::with_capacity(4096);
            for i in 0..4096usize {
                let name = &palette[ids[base + i] as usize];
                let index = section_palette
                    .iter()
                    .position(|p| p == name)
                    .unwrap_or_else(|| {
                        section_palette.push(name.clone());
                        section_palette.len() - 1
                    });
                blocks.push(u16::try_from(index).expect("a section palette is small"));
            }

            // Four 4x4x4 layers per section, sixteen cells each.
            let biome_base = usize::try_from(s * 4 * 16).expect("a positive index");
            let mut section_biomes: Vec<String> = Vec::new();
            let mut biomes = Vec::with_capacity(64);
            for i in 0..64usize {
                let name = &biome_palette[biome_ids[biome_base + i] as usize];
                let index = section_biomes
                    .iter()
                    .position(|p| p == name)
                    .unwrap_or_else(|| {
                        section_biomes.push(name.clone());
                        section_biomes.len() - 1
                    });
                biomes.push(u8::try_from(index).expect("a biome palette is small"));
            }

            Section {
                y: i8::try_from(first_section + s).expect("a section index fits in i8"),
                palette: section_palette,
                blocks,
                biome_palette: section_biomes,
                biomes,
                extra: std::collections::BTreeMap::new(),
            }
        })
        .collect();

    ChunkColumn {
        // Derived from the FINISHED block states, which is what vanilla's
        // own answer is a function of: `LevelChunk.setBlockState` creates a
        // block entity for any `EntityBlock` state it stores and drops it
        // when that position is overwritten, so at save time the list is a
        // statement about the final states and nothing else.
        //
        // Running it here rather than inside a feature is what makes it
        // unable to perturb generation: the canvas is already final, so no
        // draw is taken and no block state can move. Only the types
        // `crate::block_entity` can reproduce exactly are emitted, and its
        // header says which those are and why the rest are absent — the
        // absence means `not yet reproduced`, never `needs none`.
        // The channel is read here, off the canvas that generated the chunk.
        // A canvas that was not recording hands back an empty slice, so this
        // column carries exactly what it carried before the channel existed —
        // the classes the finished block states answer for.
        block_entities: crate::block_entity::from_sections_recorded(
            &sections,
            chunk_x,
            chunk_z,
            canvas.block_entity_records(chunk_x, chunk_z),
        )
        .0,
        // Filled by the caller through `heightmaps_from_canvas`: computing
        // them needs the block-property table, and this crate deliberately
        // depends on nothing that would carry one.
        heightmaps: std::collections::BTreeMap::new(),
        // `LastUpdate`, `InhabitedTime`, `isLightOn`, the two tick queues,
        // `PostProcessing` and `structures`, at the values a freshly
        // generated chunk has. Named through one constructor so that this
        // bridge and the save path cannot disagree about what a fresh chunk
        // carries.
        extra: rustmc_world::anvil::generated_extra(),
        // This generator does not place structures yet, so a column it
        // builds records none. Named rather than defaulted: the day
        // structures land, every writer has to decide what goes here.
        structure_starts: Vec::new(),
        // Nor references: writing a reference set that nothing computed
        // would make the diff read as agreement where there is none.
        structure_references: std::collections::BTreeMap::new(),
        x: chunk_x,
        z: chunk_z,
        status: status.to_string(),
        data_version,
        sections,
        is_light_on: false,
        light: std::collections::BTreeMap::new(),
    }
}

#[cfg(test)]
mod canvas_tests {
    use super::*;
    use crate::canvas::Canvas;

    #[test]
    fn a_canvas_chunk_round_trips_into_sections() {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(0, 0, "minecraft:air");
        canvas.set_block(0, -64, 0, "minecraft:bedrock");
        canvas.set_block(15, 100, 15, "minecraft:oak_leaves[distance=1]");
        canvas.set_biome(0, -64, 0, "minecraft:desert");

        let column = column_from_canvas(&canvas, 0, 0, "minecraft:full", 4000);
        assert_eq!(column.sections.len(), 24, "384 / 16");
        assert_eq!(column.sections[0].y, -4, "the bottom section is -64 >> 4");

        // The bottom section holds the bedrock and the desert cell.
        let bottom = &column.sections[0];
        assert!(bottom.palette.contains(&"minecraft:bedrock".to_string()));
        assert!(bottom
            .biome_palette
            .contains(&"minecraft:desert".to_string()));
        assert_eq!(bottom.blocks.len(), 4096);
        assert_eq!(bottom.biomes.len(), 64);

        // A section far from either keeps a palette of one.
        let mid = column
            .sections
            .iter()
            .find(|s| s.y == 0)
            .expect("section zero");
        assert_eq!(mid.palette, ["minecraft:air"]);
    }

    /// Section palettes are per section, not the chunk's — otherwise every
    /// section carries every state in the column.
    #[test]
    fn section_palettes_are_narrow() {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(0, 0, "minecraft:air");
        for y in -64..-48 {
            canvas.set_block(0, y, 0, "minecraft:deepslate");
        }
        for y in 100..116 {
            canvas.set_block(0, y, 0, "minecraft:sandstone");
        }
        let column = column_from_canvas(&canvas, 0, 0, "minecraft:full", 4000);
        let bottom = &column.sections[0];
        assert!(!bottom.palette.contains(&"minecraft:sandstone".to_string()));
    }
}
