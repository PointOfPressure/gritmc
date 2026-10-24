//! `region-differ topdown` — a top-down PNG of a region directory.
//!
//! # Why a picture, when every other number here is a block count
//!
//! Because the owner's acceptance criterion is not a percentage. It is "if I
//! load into the game and it looks really weird, I won't be happy", and a
//! block-diff cannot see that. 99.96% agreement over 107 million blocks
//! leaves 43,041 blocks free, and 43,041 blocks is an entire missing village
//! twice over — a thing a player would notice from fifty blocks away and that
//! moves the percentage by 0.013%. A number that only goes up is not evidence
//! that the world is right; this renders the world so a human can say.
//!
//! It draws the highest non-air block in every column, coloured by block and
//! shaded by height, so the picture carries terrain shape *and* material. Two
//! runs of it — vanilla and ours over the same square — are directly
//! comparable by eye, and `--mark` overlays a position list (the output of
//! `diff --positions`) so the disagreements can be seen in place rather than
//! counted.
//!
//! PNG rather than PPM because the build machine has no image converter
//! installed; the encoder below is the minimum that produces a valid file and
//! uses `flate2`, which is already a workspace dependency.

use rustmc_world::anvil::ChunkColumn;
use std::collections::BTreeMap;
use std::io::{self, Write};

/// Blocks that are not "ground" for the purpose of the highest visible block.
fn is_transparent(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// A colour per block name.
///
/// The named entries are the families that carry the eye — terrain, foliage,
/// water, and the structure palettes this project is porting — because those
/// are the ones a reader is judging. Everything else gets a stable colour
/// derived from its name, which is enough to make "these two pictures differ
/// here" visible without pretending to be a texture atlas.
fn colour_of(name: &str) -> (u8, u8, u8) {
    match name {
        "minecraft:water" => (48, 92, 190),
        "minecraft:lava" => (222, 108, 30),
        "minecraft:grass_block" => (94, 148, 68),
        "minecraft:dirt" | "minecraft:coarse_dirt" | "minecraft:rooted_dirt" => (122, 88, 60),
        "minecraft:podzol" => (86, 62, 30),
        "minecraft:sand" => (216, 204, 152),
        "minecraft:red_sand" => (176, 96, 42),
        "minecraft:sandstone" | "minecraft:smooth_sandstone" => (204, 190, 140),
        "minecraft:gravel" => (136, 132, 130),
        "minecraft:stone" | "minecraft:andesite" | "minecraft:diorite" => (128, 128, 128),
        "minecraft:granite" => (150, 104, 86),
        "minecraft:deepslate" => (72, 72, 78),
        "minecraft:calcite" => (222, 224, 216),
        "minecraft:tuff" => (108, 110, 100),
        "minecraft:snow" | "minecraft:snow_block" | "minecraft:powder_snow" => (238, 244, 250),
        "minecraft:ice" | "minecraft:packed_ice" | "minecraft:blue_ice" => (160, 200, 240),
        "minecraft:terracotta" => (152, 94, 68),
        "minecraft:netherrack" => (110, 44, 44),
        "minecraft:basalt" | "minecraft:smooth_basalt" => (72, 70, 80),
        "minecraft:blackstone" => (44, 40, 46),
        "minecraft:soul_sand" | "minecraft:soul_soil" => (84, 64, 50),
        "minecraft:nether_wart_block" => (124, 20, 20),
        "minecraft:warped_wart_block" => (20, 110, 116),
        "minecraft:crimson_nylium" => (142, 44, 40),
        "minecraft:warped_nylium" => (30, 116, 110),
        "minecraft:end_stone" => (220, 222, 160),
        "minecraft:obsidian" => (24, 20, 34),
        "minecraft:purpur_block" | "minecraft:purpur_pillar" => (170, 120, 170),
        "minecraft:chorus_plant" | "minecraft:chorus_flower" => (120, 80, 130),
        "minecraft:bedrock" => (20, 20, 20),
        "minecraft:oak_log" | "minecraft:spruce_log" | "minecraft:birch_log" => (94, 72, 44),
        "minecraft:oak_leaves"
        | "minecraft:spruce_leaves"
        | "minecraft:birch_leaves"
        | "minecraft:jungle_leaves"
        | "minecraft:acacia_leaves"
        | "minecraft:dark_oak_leaves" => (54, 110, 44),
        "minecraft:cobblestone" | "minecraft:mossy_cobblestone" => (110, 110, 110),
        "minecraft:tuff_bricks" | "minecraft:chiseled_tuff_bricks" => (118, 120, 108),
        "minecraft:copper_block" | "minecraft:cut_copper" => (190, 116, 78),
        _ => {
            // Stable per-name colour. FNV-1a, then forced into a mid band so
            // nothing unnamed can pass for water or for the void.
            let mut h: u32 = 0x811c_9dc5;
            for b in name.bytes() {
                h ^= u32::from(b);
                h = h.wrapping_mul(0x0100_0193);
            }
            let r = 90 + u8::try_from((h >> 16) & 0x5f).unwrap_or(0);
            let g = 90 + u8::try_from((h >> 8) & 0x5f).unwrap_or(0);
            let b = 90 + u8::try_from(h & 0x5f).unwrap_or(0);
            (r, g, b)
        }
    }
}

/// The highest non-air block in each column, as `(name, y)`.
fn surface_map(
    chunks: &BTreeMap<(i32, i32), ChunkColumn>,
    ybox: Option<(i32, i32)>,
) -> BTreeMap<(i32, i32), (String, i32)> {
    let mut out: BTreeMap<(i32, i32), (String, i32)> = BTreeMap::new();
    for (&(chunk_x, chunk_z), chunk) in chunks {
        for section in &chunk.sections {
            let base_y = i32::from(section.y) * 16;
            if ybox.is_some_and(|(lo, hi)| base_y + 15 < lo || base_y > hi) {
                continue;
            }
            for i in 0..4096usize {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "an index bounded by 4096"
                )]
                let (dx, dy, dz) = ((i % 16) as i32, (i / 256) as i32, ((i / 16) % 16) as i32);
                let world_y = base_y + dy;
                if ybox.is_some_and(|(lo, hi)| world_y < lo || world_y > hi) {
                    continue;
                }
                let state = &section.palette[section.blocks.get(i).map_or(0, |&v| v as usize)];
                let name = state.split('[').next().unwrap_or(state);
                if is_transparent(name) {
                    continue;
                }
                let key = (chunk_x * 16 + dx, chunk_z * 16 + dz);
                let better = out.get(&key).is_none_or(|&(_, y)| world_y > y);
                if better {
                    out.insert(key, (name.to_string(), world_y));
                }
            }
        }
    }
    out
}

/// Render and write a PNG. Returns the pixel dimensions.
pub fn render(
    chunks: &BTreeMap<(i32, i32), ChunkColumn>,
    ybox: Option<(i32, i32)>,
    marks: &[(i32, i32)],
    scale: usize,
    out_path: &str,
) -> io::Result<(usize, usize)> {
    let surf = surface_map(chunks, ybox);
    if surf.is_empty() {
        return Err(io::Error::other(
            "no non-air blocks in range: nothing to draw",
        ));
    }
    let (mut x0, mut x1, mut z0, mut z1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    let (mut ylo, mut yhi) = (i32::MAX, i32::MIN);
    for (&(x, z), (_, y)) in &surf {
        x0 = x0.min(x);
        x1 = x1.max(x);
        z0 = z0.min(z);
        z1 = z1.max(z);
        ylo = ylo.min(*y);
        yhi = yhi.max(*y);
    }
    let w = usize::try_from(x1 - x0 + 1).unwrap_or(0) * scale;
    let h = usize::try_from(z1 - z0 + 1).unwrap_or(0) * scale;
    let span = f64::from((yhi - ylo).max(1));

    let mut rgb = vec![0u8; w * h * 3];
    for (&(x, z), (name, y)) in &surf {
        let (r, g, b) = colour_of(name);
        // Shade by height so terrain shape reads even where the material is
        // uniform. 0.55..1.0 keeps the darkest ground distinguishable from
        // the black background.
        let t = 0.55 + 0.45 * (f64::from(*y - ylo) / span);
        let px = usize::try_from(x - x0).unwrap_or(0) * scale;
        let pz = usize::try_from(z - z0).unwrap_or(0) * scale;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "t is in 0.55..=1.0 and the operands are u8"
        )]
        let shaded = [
            (f64::from(r) * t) as u8,
            (f64::from(g) * t) as u8,
            (f64::from(b) * t) as u8,
        ];
        for sz in 0..scale {
            for sx in 0..scale {
                let idx = ((pz + sz) * w + px + sx) * 3;
                if let Some(slot) = rgb.get_mut(idx..idx + 3) {
                    slot.copy_from_slice(&shaded);
                }
            }
        }
    }
    // Marks last so nothing paints over them. Magenta is chosen because no
    // block colour above is anywhere near it.
    for &(x, z) in marks {
        if x < x0 || x > x1 || z < z0 || z > z1 {
            continue;
        }
        let px = usize::try_from(x - x0).unwrap_or(0) * scale;
        let pz = usize::try_from(z - z0).unwrap_or(0) * scale;
        for sz in 0..scale {
            for sx in 0..scale {
                let idx = ((pz + sz) * w + px + sx) * 3;
                if let Some(slot) = rgb.get_mut(idx..idx + 3) {
                    slot.copy_from_slice(&[255, 0, 220]);
                }
            }
        }
    }
    write_png(&rgb, w, h, out_path)?;
    Ok((w, h))
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    let len = u32::try_from(body.len()).unwrap_or(0);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(body);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

fn write_png(rgb: &[u8], w: usize, h: usize, path: &str) -> io::Result<()> {
    let mut png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(w).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(h).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, truecolour RGB
    png_chunk(&mut png, b"IHDR", &ihdr);

    // Every scanline carries filter byte 0 (None). Filtering would compress
    // better and this file is written once and looked at once.
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for row in 0..h {
        raw.push(0);
        let start = row * w * 3;
        raw.extend_from_slice(rgb.get(start..start + w * 3).unwrap_or(&[]));
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&raw)?;
    png_chunk(&mut png, b"IDAT", &enc.finish()?);
    png_chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png)
}

#[cfg(test)]
mod tests {
    use super::{colour_of, is_transparent};

    /// The three air spellings vanilla uses. A renderer that misses
    /// `cave_air` draws the roof of every cave as the ground.
    #[test]
    fn every_air_spelling_is_transparent_and_solid_ground_is_not() {
        for air in ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"] {
            assert!(is_transparent(air), "{air} must not read as ground");
        }
        for solid in [
            "minecraft:stone",
            "minecraft:water",
            "minecraft:grass_block",
        ] {
            assert!(!is_transparent(solid), "{solid} must read as ground");
        }
    }

    /// "Everything else gets a stable colour derived from its name" — the
    /// doc comment's claim, which nothing checked. A colour that moved
    /// between two renders of the same world would make the two pictures
    /// differ everywhere and say nothing.
    #[test]
    fn an_unnamed_block_gets_the_same_colour_every_time() {
        let a = colour_of("minecraft:some_block_this_tool_has_never_heard_of");
        let b = colour_of("minecraft:some_block_this_tool_has_never_heard_of");
        assert_eq!(a, b);
        assert_ne!(a, colour_of("minecraft:a_different_unnamed_block"));
    }

    /// The named families have to be told apart by eye, which is the whole
    /// point of naming them; two that collide are worse than two derived ones.
    #[test]
    fn the_named_terrain_families_are_distinct_colours() {
        let names = [
            "minecraft:water",
            "minecraft:lava",
            "minecraft:grass_block",
            "minecraft:sand",
            "minecraft:stone",
            "minecraft:deepslate",
        ];
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(colour_of(a), colour_of(b), "{a} and {b} render identically");
            }
        }
    }
}
