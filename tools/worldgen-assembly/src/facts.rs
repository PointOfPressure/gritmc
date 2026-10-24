//! The block-behaviour answers the decoration stage asks for.
//!
//! `rustmc-worldgen` defines the questions as traits and stays data-free;
//! these are the answers, read from the extracts. Every one of them can say
//! `None`, and `None` is never a polite "no" — a feature that cannot get an
//! answer refuses and is counted, while one that reads `false` from a missing
//! table places nothing and looks like a working generator with an empty
//! world. That distinction is the whole reason the decoration gate can report
//! "165 computed, 4 replayed" rather than "165 fine".

use rustmc_behavior::block_props::{BlockPropsTable, Heightmap};
use rustmc_worldgen::canvas::LiveHeightmap;
use rustmc_worldgen::stage::decorate::{BlockFacts, SurvivalRules, SurvivalView};
use rustmc_worldgen::tags::BlockTags;

/// [`BlockFacts`] over the per-state extract.
pub struct TableFacts(pub BlockPropsTable);

/// Vanilla's heightmap types, in this port's two spellings.
fn heightmap(map: LiveHeightmap) -> Heightmap {
    match map {
        LiveHeightmap::WorldSurface => Heightmap::WorldSurface,
        LiveHeightmap::OceanFloor => Heightmap::OceanFloor,
        LiveHeightmap::MotionBlocking => Heightmap::MotionBlocking,
        LiveHeightmap::MotionBlockingNoLeaves => Heightmap::MotionBlockingNoLeaves,
    }
}

impl BlockFacts for TableFacts {
    fn default_state(&self, block: &str) -> Option<String> {
        self.0.default_state(block).map(str::to_string)
    }

    fn is_air(&self, state: &str) -> Option<bool> {
        self.0.get(state).map(|p| p.air)
    }

    fn fluid(&self, state: &str) -> Option<String> {
        self.0.get(state).map(|p| {
            p.fluid
                .clone()
                .unwrap_or_else(|| "minecraft:empty".to_string())
        })
    }

    fn is_solid_render(&self, state: &str) -> Option<bool> {
        self.0.get(state).map(|p| p.solid_render)
    }

    fn is_solid(&self, state: &str) -> Option<bool> {
        self.0.get(state).map(|p| p.solid)
    }

    fn is_replaceable(&self, state: &str) -> Option<bool> {
        self.0.get(state).map(|p| p.replaceable)
    }

    fn face_is_sturdy(&self, state: &str, direction: usize) -> Option<bool> {
        self.0
            .get(state)
            .and_then(|p| p.sturdy_faces.get(direction).copied())
    }

    fn can_support_center(&self, state: &str, direction: usize) -> Option<bool> {
        self.0.get(state).map(|p| {
            // DOWN is index 0 in `Direction.values()`.
            if direction == 0 && p.unstable_bottom_center {
                return false;
            }
            p.center_faces.get(direction).copied().unwrap_or(false)
        })
    }

    fn face_is_full(&self, state: &str, direction: usize) -> Option<bool> {
        self.0
            .get(state)
            .and_then(|p| p.full_faces.get(direction).copied())
    }

    fn face_is_collision_full(&self, state: &str, direction: usize) -> Option<bool> {
        self.0
            .get(state)
            .and_then(|p| p.collision_full_faces.get(direction).copied())
    }

    fn matches_heightmap(&self, state: &str, map: LiveHeightmap) -> bool {
        // A state the table has never heard of counts towards nothing. That
        // is the one place a missing answer *is* treated as "no", and it is
        // safe because a heightmap is only ever raised: the worst case is a
        // column one block too low, and the unknown state is reported by
        // whatever asked for it first.
        self.0.get(state).is_some_and(|p| p.matches(heightmap(map)))
    }
}

/// [`SurvivalRules`] over `rustmc_behavior::survival`.
pub struct Survival {
    pub tags: BlockTags,
}

/// Adapts the view the decoration world lends out to the world the survival
/// rules want.
struct Lent<'a>(&'a SurvivalView<'a>);

impl rustmc_behavior::survival::SurvivalWorld for Lent<'_> {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        (self.0.block_at)(x, y, z)
    }

    fn face_is_sturdy_up(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        (self.0.face_is_sturdy_up)(x, y, z)
    }

    fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        (self.0.air_at)(x, y, z)
    }

    fn can_support_center(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        (self.0.can_support_center)(x, y, z, direction)
    }

    fn collision_face_full_up(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        (self.0.collision_face_full_up)(x, y, z)
    }

    /// Overrides the trait's name-shaped default with the block-property
    /// table's answer, which is the only one that sees a waterlogged block's
    /// water.
    fn fluid_at(&mut self, x: i32, y: i32, z: i32) -> String {
        (self.0.fluid_at)(x, y, z)
    }

    fn raw_brightness(&mut self, x: i32, y: i32, z: i32) -> Option<i32> {
        (self.0.raw_brightness)(x, y, z)
    }

    fn is_solid_render(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        (self.0.is_solid_render)(x, y, z)
    }
}

struct Tags<'a>(&'a BlockTags);

impl rustmc_behavior::survival::TagLookup for Tags<'_> {
    fn contains(&self, tag: &str, block: &str) -> bool {
        self.0.get(tag).is_some_and(|set| set.contains(block))
    }
}

impl SurvivalRules for Survival {
    fn would_survive(
        &self,
        view: &SurvivalView<'_>,
        state: &str,
        pos: (i32, i32, i32),
    ) -> Option<bool> {
        let tags = Tags(&self.tags);
        match rustmc_behavior::survival::can_survive(&mut Lent(view), &tags, state, pos) {
            rustmc_behavior::survival::Survival::Yes => Some(true),
            rustmc_behavior::survival::Survival::No => Some(false),
            rustmc_behavior::survival::Survival::Unknown => None,
        }
    }
}

/// Resolve a biome tag to the set of biomes it names.
///
/// Vanilla's biome tags nest: `has_structure/mineshaft_mesa` is exactly
/// `["#minecraft:is_badlands"]`, and `is_badlands` is three concrete biomes.
/// `has_structure/mineshaft` is eight nested tags plus a handful of literals.
/// So this has to recurse, and a resolver that treated a leading `#` as part
/// of a biome name would silently produce an empty set — which reads as "no
/// biome qualifies" and is indistinguishable from a correct refusal.
///
/// An entry may also be optional (`{"id": ..., "required": false}`), which is
/// accepted and treated as present; vanilla drops it only when the id does not
/// resolve, and every id in the extract does.
///
/// Cycles are impossible in vanilla's data but are guarded anyway, because the
/// failure mode is a hang rather than a wrong answer.
///
/// # Errors
/// If a tag file is missing or malformed.
pub fn resolve_biome_tag(tag: &str) -> Result<std::collections::BTreeSet<String>, String> {
    fn go(
        tag: &str,
        out: &mut std::collections::BTreeSet<String>,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        if !seen.insert(tag.to_string()) {
            return Ok(());
        }
        let bare = tag.strip_prefix('#').unwrap_or(tag);
        let bare = bare.strip_prefix("minecraft:").unwrap_or(bare);
        let path = format!("{}/tags/worldgen/biome/{bare}.json", crate::DATA);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let values = json["values"]
            .as_array()
            .ok_or_else(|| format!("{path}: no `values` array"))?;
        for value in values {
            // Either a string, or `{"id": "...", "required": false}`.
            let id = match value {
                serde_json::Value::String(s) => s.as_str(),
                other => other["id"]
                    .as_str()
                    .ok_or_else(|| format!("{path}: an entry is neither string nor id"))?,
            };
            if let Some(nested) = id.strip_prefix('#') {
                go(nested, out, seen)?;
            } else {
                out.insert(id.to_string());
            }
        }
        Ok(())
    }

    let mut out = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    go(tag, &mut out, &mut seen)?;
    Ok(out)
}

/// Resolve a structure definition's own `biomes` field to the biomes it names.
///
/// **Read the field. Do not rebuild it from the structure's id.** Both callers
/// of this used to derive `has_structure/<id>`, and that convention is not a
/// rule vanilla keeps: of the extract's 34 structures, five declare a tag with
/// a different name — `fortress` declares `has_structure/nether_fortress`,
/// `jungle_pyramid` declares `jungle_temple`, `mansion` declares
/// `woodland_mansion`, `monument` declares `ocean_monument`, and
/// `ruined_portal` declares **`has_structure/ruined_portal_standard`**, for
/// which the derived `has_structure/ruined_portal.json` does not exist at all.
///
/// The field is `Codec.either(TagKey.hashedCodec(registry),
/// homogenousList(elementCodec, …))` — `HolderSetCodec.create`, offsets 17..32
/// — and `homogenousList` wraps `elementCodec.listOf()` in
/// `ExtraCodecs.compactListCodec`, which accepts a bare element in a list's
/// place. So exactly three shapes decode:
///
/// * `"#minecraft:has_structure/foo"` — a tag. `TagKey.hashedCodec` is what
///   requires the `#`, so the `#` is the *only* thing that makes a string a
///   tag; [`resolve_biome_tag`] strips it and recurses.
/// * `"minecraft:plains"` — `compactListCodec`'s one-element form. A bare id
///   fails the tag branch and lands here as a single biome, not as a tag.
/// * `["minecraft:plains", …]` — a list of elements. Vanilla's element codec
///   decodes an `Identifier`, in which `#` is not a legal character, so a `#`
///   inside a list is an error there and is an error here.
///
/// **Measured on the 26.2 extract, 2026-08-09: all 34 structures use the tag
/// string form and none uses a list**, so the two element branches are dead
/// against this data and are written only so that an unfamiliar shape refuses
/// instead of being misread. Everything else refuses too: an absent field, a
/// non-string list entry, and a tag that resolves to nothing. That last one
/// matters most — an empty set reads as "no biome qualifies", which places no
/// structure anywhere and is indistinguishable from a correct refusal, and it
/// is the failure this whole resolver exists to make impossible.
///
/// # Errors
/// If `biomes` is absent, is not a string or a list of strings, contains a
/// tag reference inside a list, or resolves to no biomes at all.
pub fn resolve_structure_biomes(
    value: &serde_json::Value,
    path: &str,
) -> Result<std::collections::BTreeSet<String>, String> {
    let biomes = &value["biomes"];
    let mut out = std::collections::BTreeSet::new();
    match biomes {
        serde_json::Value::String(s) => {
            if s.starts_with('#') {
                out = resolve_biome_tag(s)?;
            } else {
                out.insert(s.clone());
            }
        }
        serde_json::Value::Array(entries) => {
            for entry in entries {
                let id = entry
                    .as_str()
                    .ok_or_else(|| format!("{path}: a `biomes` entry is not a string"))?;
                if id.starts_with('#') {
                    return Err(format!(
                        "{path}: `biomes` list holds a tag reference `{id}`, which vanilla's \
                         element codec cannot decode"
                    ));
                }
                out.insert(id.to_string());
            }
        }
        serde_json::Value::Null => return Err(format!("{path}: no `biomes`")),
        other => {
            return Err(format!(
                "{path}: `biomes` is neither a string nor a list ({other})"
            ))
        }
    }
    if out.is_empty() {
        return Err(format!("{path}: `biomes` resolved to no biomes at all"));
    }
    Ok(out)
}
