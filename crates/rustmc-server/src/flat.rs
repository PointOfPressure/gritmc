//! Superflat: the world the gameplay oracle measures gameplay in.
//!
//! # Why a server that generates real terrain needs a world with none
//!
//! The gameplay oracle runs a scenario against real vanilla and against this
//! server and diffs what each one said on the wire. That only means anything
//! if the two started from the same world. On the noise world they do not:
//! the overworld's order-invariant core is zero on two seeds, which is a
//! very strong statement about terrain and still not "identical" — and a
//! gameplay check that fails because a sapling grew in one world and not the
//! other has measured nothing about gameplay.
//!
//! Superflat is vanilla's own answer to that. Four layers, one biome, no
//! features, no lakes; every column is the same column. A scenario written
//! relative to where the player is standing therefore means the same thing
//! on both servers *by construction* rather than by an argument about
//! terrain parity — which is exactly the property a control should have.
//!
//! It is also cheap: no noise, no carvers, no decoration, so a target boots
//! in milliseconds instead of the tens of seconds the spawn tile costs. A
//! corpus that boots a server per scenario needs that to be true.
//!
//! # The layers are extracted, not written down here
//!
//! `ci/dump-flat-preset.sh` pulls them out of Mojang's own data generator
//! into `data/vanilla/flat_preset.json`, the same category as registries,
//! items and block properties. Without the file, `--level-type flat` fails
//! by name: a substrate that a published parity number is measured on must
//! never be four block names somebody remembered.

use rustmc_worldgen::canvas::{Canvas, HeightmapPredicates, LiveHeightmap};
use rustmc_worldgen::pipeline::Area;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

/// The overworld's vertical extent — `minecraft:overworld`'s dimension type,
/// the same pair every other `Canvas` in this repo is built with.
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;

/// One entry of the preset's `layers` list.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Layer {
    pub block: String,
    pub height: i32,
}

/// Vanilla's superflat settings, as its data generator emits them.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct FlatPreset {
    pub biome: String,
    /// Carried even though the shipped preset says false for both: a
    /// substrate that quietly gained decoration would otherwise read as a
    /// gameplay divergence in every scenario at once.
    #[serde(default)]
    pub features: bool,
    #[serde(default)]
    pub lakes: bool,
    /// Bottom layer first, starting at the dimension's minimum y.
    pub layers: Vec<Layer>,
}

impl FlatPreset {
    /// Read the extracted preset.
    ///
    /// # Errors
    /// If the file is missing or is not the shape the generator emits. Both
    /// are reported rather than defaulted — see the module docs.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{}: {e} — run ci/dump-flat-preset.sh", path.display()),
            )
        })?;
        let preset: Self = serde_json::from_str(&text).map_err(io::Error::other)?;
        if preset.layers.is_empty() {
            return Err(io::Error::other(format!(
                "{}: the preset has no layers",
                path.display()
            )));
        }
        if preset.features || preset.lakes {
            return Err(io::Error::other(format!(
                "{}: this preset generates features or lakes, which is not a \
                 substrate a differential check can use",
                path.display()
            )));
        }
        Ok(preset)
    }

    /// The block at a height, or `None` above the stack.
    ///
    /// Layers are laid bottom-up from the dimension's minimum y, which is
    /// what makes the shipped preset put bedrock at −64 and grass at −61.
    #[must_use]
    pub fn block_at(&self, y: i32) -> Option<&str> {
        let mut base = MIN_Y;
        for layer in &self.layers {
            if y >= base && y < base + layer.height {
                return Some(&layer.block);
            }
            base += layer.height;
        }
        None
    }

    /// One past the top solid layer — where a player stands.
    #[must_use]
    pub fn surface_y(&self) -> i32 {
        MIN_Y + self.layers.iter().map(|l| l.height).sum::<i32>()
    }

    /// Each layer's block resolved to its **default state**.
    ///
    /// The preset names blocks; the canvas, the chunk encoder and the wire
    /// all speak block *states*. `minecraft:grass_block` is not a state —
    /// `minecraft:grass_block[snowy=false]` is — and a canvas full of bare
    /// names encodes to nothing: the server refused the very first join with
    /// "no network id for block state minecraft:grass_block", which is the
    /// right error in the wrong place. Resolving here means a preset naming
    /// a block that does not exist fails when the world is built, by name.
    ///
    /// # Errors
    /// If a layer names a block the extract does not have.
    pub fn states(
        &self,
        props: &rustmc_behavior::block_props::BlockPropsTable,
    ) -> io::Result<Vec<(String, i32)>> {
        self.layers
            .iter()
            .map(|l| {
                props
                    .default_state(&l.block)
                    .map(|s| (s.to_string(), l.height))
                    .ok_or_else(|| {
                        io::Error::other(format!(
                            "the flat preset names {}, which the block extract does not have",
                            l.block
                        ))
                    })
            })
            .collect()
    }

    /// Fill `area` — plus nothing, because a flat world needs no margin.
    ///
    /// The margin the noise pipeline generates exists because carvers read a
    /// 17x17 neighbourhood and decoration writes one chunk outwards. Neither
    /// runs here, so a chunk is finished the moment it is filled, and this
    /// is the one generator in the repo whose output does not depend on what
    /// its neighbours did.
    #[must_use]
    pub fn canvas(&self, area: Area, states: &[(String, i32)]) -> Canvas {
        let mut canvas = Canvas::new(MIN_Y, HEIGHT);
        for (cx, cz) in area.chunks() {
            canvas.ensure_chunk(cx, cz, "minecraft:air");
            let mut y = MIN_Y;
            for (state, height) in states {
                for _ in 0..*height {
                    for dz in 0..16 {
                        for dx in 0..16 {
                            canvas.set_block(cx * 16 + dx, y, cz * 16 + dz, state);
                        }
                    }
                    y += 1;
                }
            }
            for dz in 0..16 {
                for dx in 0..16 {
                    for y in (MIN_Y..MIN_Y + HEIGHT).step_by(4) {
                        canvas.set_biome(cx * 16 + dx, y, cz * 16 + dz, &self.biome);
                    }
                }
            }
        }
        canvas
    }
}

/// The heightmap predicate, over the real block-property table.
///
/// Not a shortcut like "anything that is not air": the flat stack happens to
/// make those agree, and the moment a scenario places water or a slab the
/// shortcut would be wrong in a way that only shows up as a client rendering
/// the ground in the wrong place.
pub struct Predicates<'a>(pub &'a rustmc_behavior::block_props::BlockPropsTable);

impl HeightmapPredicates for Predicates<'_> {
    fn matches(&self, state: &str, map: LiveHeightmap) -> bool {
        use rustmc_behavior::block_props::Heightmap as P;
        let prop = match map {
            LiveHeightmap::WorldSurface => P::WorldSurface,
            LiveHeightmap::OceanFloor => P::OceanFloor,
            LiveHeightmap::MotionBlocking => P::MotionBlocking,
            LiveHeightmap::MotionBlockingNoLeaves => P::MotionBlockingNoLeaves,
        };
        self.0.get(state).is_some_and(|facts| facts.matches(prop))
    }
}

/// The preset this process serves, if `--level-type flat` was asked for.
static SELECTED: OnceLock<Option<FlatPreset>> = OnceLock::new();

/// Choose superflat for this process, loading the extracted preset.
///
/// # Errors
/// If the extract is missing or malformed.
pub fn select(path: &Path) -> io::Result<()> {
    let preset = FlatPreset::load(path)?;
    let _ = SELECTED.set(Some(preset));
    Ok(())
}

/// The active flat preset, or `None` on the noise world.
#[must_use]
pub fn selected() -> Option<&'static FlatPreset> {
    SELECTED.get_or_init(|| None).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> FlatPreset {
        // The shipped preset's own shape, used to check the *stacking* rule
        // rather than to assert the contents — the contents come from the
        // extract and asserting them here would be the transcription this
        // module exists to avoid.
        FlatPreset {
            biome: "minecraft:plains".into(),
            features: false,
            lakes: false,
            layers: vec![
                Layer {
                    block: "minecraft:bedrock".into(),
                    height: 1,
                },
                Layer {
                    block: "minecraft:dirt".into(),
                    height: 2,
                },
                Layer {
                    block: "minecraft:grass_block".into(),
                    height: 1,
                },
            ],
        }
    }

    #[test]
    fn layers_stack_upward_from_the_world_bottom() {
        let p = shipped();
        assert_eq!(p.block_at(MIN_Y), Some("minecraft:bedrock"));
        assert_eq!(p.block_at(MIN_Y + 1), Some("minecraft:dirt"));
        assert_eq!(p.block_at(MIN_Y + 2), Some("minecraft:dirt"));
        assert_eq!(p.block_at(MIN_Y + 3), Some("minecraft:grass_block"));
        assert_eq!(p.block_at(MIN_Y + 4), None);
        assert_eq!(p.block_at(MIN_Y - 1), None);
        assert_eq!(p.surface_y(), MIN_Y + 4);
    }

    #[test]
    fn every_column_is_the_same_column() {
        // The one property the whole substrate argument rests on.
        let p = shipped();
        let states: Vec<(String, i32)> = p
            .layers
            .iter()
            .map(|l| (format!("{}[test=1]", l.block), l.height))
            .collect();
        let canvas = p.canvas(Area::square(0, 0, 1), &states);
        for y in [MIN_Y, MIN_Y + 3, MIN_Y + 4] {
            let want = canvas.block_at(0, y, 0).to_string();
            for (x, z) in [(1, 0), (15, 15), (-1, -1), (-16, 20)] {
                assert_eq!(canvas.block_at(x, y, z), want, "at ({x},{y},{z})");
            }
        }
    }
}
