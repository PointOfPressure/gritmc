//! `FoliagePlacer` — the leaves.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `FoliagePlacer`, `BlobFoliagePlacer`.
//!
//! # The corners cost a draw and the rest do not
//!
//! `BlobFoliagePlacer.shouldSkipLocation` is
//!
//! ```text
//! x == range && z == range && (nextInt(2) == 0 || y == 0)
//! ```
//!
//! so the draw happens **only at the four corners of each row**, and only
//! after both coordinate tests pass. A port that evaluated the whole
//! condition eagerly, or that drew once per position, would consume a
//! different number of values for every tree — and the corner rounding is the
//! visible half of what a blob looks like.
//!
//! # The offset is drawn once, by the base class, per attachment
//!
//! `FoliagePlacer.createFoliage` calls `offset(random)` — an `IntProvider`,
//! so at least one draw — and passes the result down to the subclass. It runs
//! per foliage attachment, which for a straight trunk is once.

use super::{valid_tree_pos, NeedsRules, Placed, Pos, TreeConfig, TreeWorld};
use crate::feature::int_provider::IntProvider;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// `FoliagePlacer.FoliageAttachment` — where a blob hangs, how much wider it
/// is than its radius, and whether the trunk under it is two blocks across.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoliageAttachment {
    pub pos: Pos,
    pub radius_offset: i32,
    pub double_trunk: bool,
}

/// One of vanilla's foliage placers.
#[derive(Clone, Debug, PartialEq)]
pub enum FoliagePlacer {
    Blob {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    /// `spruce_foliage_placer` — the tapering cone of a taiga spruce, whose
    /// radius *oscillates*: it widens by one per row until it reaches the
    /// current cap, then snaps back to the previous cap and the cap grows.
    /// That is what gives spruce its stepped silhouette.
    Spruce {
        radius: IntProvider,
        offset: IntProvider,
        trunk_height: IntProvider,
    },
    /// `pine_foliage_placer` — a narrower cone that widens to the radius and
    /// then holds, with one extra row at the top.
    Pine {
        radius: IntProvider,
        offset: IntProvider,
        height: IntProvider,
    },
    /// `bush_foliage_placer` — a blob whose radius shrinks with height and
    /// which has no `height` of its own.
    Bush {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    /// `acacia_foliage_placer` — the flat acacia canopy, which is not built
    /// from a stack of rows at all. It places exactly **three** rows at
    /// fixed offsets, and its `foliageHeight` is a constant 0 rather than a
    /// sampled value, so an acacia's height comes only from its trunk.
    Acacia {
        radius: IntProvider,
        offset: IntProvider,
    },
    /// `fancy_foliage_placer` — the canopy of the big branching oak.
    ///
    /// It extends `BlobFoliagePlacer` and overrides two things: the row
    /// radius (widened by one on every row *except* the top and bottom, so
    /// the cluster is a barrel rather than a cone) and `shouldSkipLocation`,
    /// which becomes a true circle test and — unlike a blob's — **takes no
    /// draw at all**. A blob rolls `nextInt(2)` at each corner; a fancy
    /// cluster rolls nothing, so getting this wrong shifts every draw after
    /// it in the chunk.
    Fancy {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    /// `jungle_foliage_placer` — `MegaJungleFoliagePlacer`, the canopy of the
    /// 2x2 jungle giant and of each of its branch ends.
    ///
    /// Two things make it unlike every placer above it.
    ///
    /// **Its row count depends on the attachment, not on the config.** A
    /// double-trunk attachment — the one `GiantTrunkPlacer` returns for the
    /// crown — uses the sampled `foliageHeight`, and a single-trunk one — the
    /// branch ends `MegaJungleTrunkPlacer` appends — uses
    /// `1 + nextInt(2)` instead. So the *same placer* draws on a branch and
    /// does not draw on the crown, and a port that always draws or never
    /// draws desynchronises one of the two.
    ///
    /// **Its rows widen downward rather than narrowing.** The radius is
    /// `foliageRadius + radiusOffset + 1 - y` with `y` counting *down* from
    /// the offset, so the lowest row is the widest. That is what makes the
    /// canopy an inverted cone hanging off the branch rather than a blob.
    MegaJungle {
        radius: IntProvider,
        offset: IntProvider,
        height: i32,
    },
    /// `dark_oak_foliage_placer` — the flat slab over a dark oak or a pale
    /// oak, and the placer with the most exceptions in it.
    ///
    /// **It places rows at fixed offsets and ignores `foliageHeight`
    /// entirely**, like the acacia and unlike every stacking placer:
    /// `foliageHeight` is a literal 4 that nothing here reads.
    ///
    /// **The crown and the stubs get different canopies.** A double-trunk
    /// attachment — the one over the leaning 2x2 — places three rows and then
    /// rolls `nextBoolean()` for a fourth; a single-trunk attachment — the
    /// twelve stubs — places two and rolls nothing. **That coin is the only
    /// draw in the whole placer**, and it fires once per dark oak rather than
    /// once per attachment, because only the crown is a double trunk.
    ///
    /// **It overrides `shouldSkipLocationSigned`, which no other placer here
    /// does.** Its extra rule runs on the *signed* coordinates, before the
    /// `abs`/nearest-column collapse, so it cannot be expressed in
    /// `should_skip` — see [`FoliagePlacer::should_skip_signed`].
    DarkOak {
        radius: IntProvider,
        offset: IntProvider,
    },
    /// Parsed but not placed. Carries its type name so a refusal says which.
    Unimplemented { name: String },
}

impl FoliagePlacer {
    /// # Errors
    /// If a supported placer is missing a field.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("foliage placer has no `type`: {value}"))?;
        let provider = |name: &str| -> Result<IntProvider, String> {
            IntProvider::parse(
                value
                    .get(name)
                    .ok_or_else(|| format!("{kind} has no `{name}`"))?,
            )
        };
        match kind {
            "minecraft:blob_foliage_placer" => Ok(Self::Blob {
                radius: IntProvider::parse(
                    value
                        .get("radius")
                        .ok_or_else(|| format!("{kind} has no `radius`"))?,
                )?,
                offset: IntProvider::parse(
                    value
                        .get("offset")
                        .ok_or_else(|| format!("{kind} has no `offset`"))?,
                )?,
                height: value
                    .get("height")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("{kind} has no `height`"))?,
            }),
            // `FancyFoliagePlacer` is a `BlobFoliagePlacer` subclass and
            // reuses `blobParts`, so the three fields are the same three.
            "minecraft:fancy_foliage_placer" => Ok(Self::Fancy {
                radius: provider("radius")?,
                offset: provider("offset")?,
                height: value
                    .get("height")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("{kind} has no `height`"))?,
            }),
            "minecraft:spruce_foliage_placer" => Ok(Self::Spruce {
                radius: provider("radius")?,
                offset: provider("offset")?,
                trunk_height: provider("trunk_height")?,
            }),
            "minecraft:pine_foliage_placer" => Ok(Self::Pine {
                radius: provider("radius")?,
                offset: provider("offset")?,
                height: provider("height")?,
            }),
            "minecraft:bush_foliage_placer" => Ok(Self::Bush {
                radius: provider("radius")?,
                offset: provider("offset")?,
                height: value
                    .get("height")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("{kind} has no `height`"))?,
            }),
            "minecraft:acacia_foliage_placer" => Ok(Self::Acacia {
                radius: provider("radius")?,
                offset: provider("offset")?,
            }),
            // `MegaJungleFoliagePlacer`'s registry name is `jungle_`, not
            // `mega_jungle_` — the class and the datapack disagree, and
            // matching on the class name here is a refusal that reads like an
            // unported placer.
            "minecraft:jungle_foliage_placer" => Ok(Self::MegaJungle {
                radius: provider("radius")?,
                offset: provider("offset")?,
                height: value
                    .get("height")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| format!("{kind} has no `height`"))?,
            }),
            "minecraft:dark_oak_foliage_placer" => Ok(Self::DarkOak {
                radius: provider("radius")?,
                offset: provider("offset")?,
            }),
            other => Ok(Self::Unimplemented {
                name: other.to_string(),
            }),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Blob { .. } => "blob",
            Self::Spruce { .. } => "spruce",
            Self::Pine { .. } => "pine",
            Self::Bush { .. } => "bush",
            Self::Acacia { .. } => "acacia",
            Self::Fancy { .. } => "fancy",
            Self::MegaJungle { .. } => "jungle",
            Self::DarkOak { .. } => "dark_oak",
            Self::Unimplemented { name } => name,
        }
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unimplemented { .. })
    }

    /// `FoliagePlacer#foliageHeight`.
    ///
    /// A blob's and a bush's are constant fields; spruce and pine sample an
    /// `IntProvider`, and spruce's is subtracted from the tree height rather
    /// than used directly.
    pub fn foliage_height(&self, random: &mut dyn RandomSource, tree_height: i32) -> i32 {
        match self {
            Self::Blob { height, .. } | Self::Bush { height, .. } | Self::Fancy { height, .. } => {
                *height
            }
            // `max(4, treeHeight - trunkHeight.sample(random))`.
            Self::Spruce { trunk_height, .. } => 4.max(tree_height - trunk_height.sample(random)),
            Self::Pine { height, .. } => height.sample(random),
            // `AcaciaFoliagePlacer#foliageHeight` returns a literal 0 and
            // takes no draw. It is not "height 0 by omission" — it is an
            // override, and the three rows below are placed relative to it.
            Self::Acacia { .. } => 0,
            // `MegaJungleFoliagePlacer#foliageHeight` returns the `height`
            // field and takes no draw, exactly like a blob's.
            Self::MegaJungle { height, .. } => *height,
            // `DarkOakFoliagePlacer#foliageHeight` returns a literal 4 and
            // takes no draw — and nothing in its `createFoliage` reads it.
            // It is here because `TreeFeature` subtracts it from the tree
            // height to get the trunk height, which decides how many rows the
            // TRUNK places.
            Self::DarkOak { .. } => 4,
            Self::Unimplemented { .. } => 0,
        }
    }

    /// `FoliagePlacer#foliageRadius` — an `IntProvider`, so whether it draws
    /// is the config's business.
    ///
    /// Pine overrides it and takes a **second** draw on top of the base
    /// class's: `super.foliageRadius(...) + nextInt(max(trunkHeight + 1, 1))`.
    pub fn foliage_radius(&self, random: &mut dyn RandomSource, trunk_height: i32) -> i32 {
        match self {
            Self::Blob { radius, .. }
            | Self::Spruce { radius, .. }
            | Self::Bush { radius, .. }
            | Self::Acacia { radius, .. }
            | Self::Fancy { radius, .. }
            | Self::MegaJungle { radius, .. }
            | Self::DarkOak { radius, .. } => radius.sample(random),
            Self::Pine { radius, .. } => {
                radius.sample(random) + random.next_i32_bound((trunk_height + 1).max(1))
            }
            Self::Unimplemented { .. } => 0,
        }
    }

    /// `FoliagePlacer#createFoliage`, which draws the offset and then hands
    /// off to the subclass.
    ///
    /// # Errors
    /// If the placer is unported or the world cannot answer.
    #[allow(
        clippy::too_many_arguments,
        reason = "\
        vanilla's own signature, and collapsing it into a struct would only
        move the list somewhere the reference no longer matches"
    )]
    pub fn create_foliage<W: TreeWorld + ?Sized>(
        &self,
        world: &mut W,
        tags: &BlockTags,
        config: &TreeConfig,
        random: &mut dyn RandomSource,
        attachment: &FoliageAttachment,
        foliage_height: i32,
        foliage_radius: i32,
        placed: &mut Placed,
    ) -> Result<(), NeedsRules> {
        match self {
            Self::Blob { offset, .. } => {
                let offset = offset.sample(random);
                // `for (int y = offset; y >= offset - foliageHeight; y--)`.
                let mut y = offset;
                while y >= offset - foliage_height {
                    // `max(foliageRadius + radiusOffset - 1 - y / 2, 0)`.
                    // Java's `/` truncates toward zero and so does Rust's, so
                    // a negative `y` rounds the same way in both.
                    let radius = (foliage_radius + attachment.radius_offset - 1 - y / 2).max(0);
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        attachment.pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )?;
                    y -= 1;
                }
                Ok(())
            }
            // `FancyFoliagePlacer#createFoliage` — the same descending rows
            // as a blob, but the radius rule is different: every row gets
            // `foliageRadius`, and the rows that are neither the top nor the
            // bottom get one more. That is what makes the cluster a barrel
            // with flat ends rather than a blob's taper.
            //
            // Note what is *absent*: the blob's `radiusOffset` term. Vanilla
            // adds `foliageRadius + (0 or 1)` and never consults the
            // attachment's offset here.
            Self::Fancy { offset, .. } => {
                let offset = offset.sample(random);
                let bottom = offset - foliage_height;
                let mut y = offset;
                while y >= bottom {
                    let radius = foliage_radius + i32::from(y != offset && y != bottom);
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        attachment.pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )?;
                    y -= 1;
                }
                Ok(())
            }
            // `SpruceFoliagePlacer#createFoliage`. The radius oscillates:
            // it grows by one per row until it hits the current cap, then
            // drops back to the previous cap and the cap itself widens.
            //
            // # The loop bound is `-foliage_height`, NOT `offset - foliage_height`
            //
            // Spruce is the odd one out and it does not look like it. Blob,
            // pine and bush all stop at `offset - foliageHeight`, so the
            // canopy is `foliageHeight + 1` rows wherever it starts; spruce
            // stops at plain `-foliageHeight`, so a nonzero offset makes the
            // canopy *taller* rather than moving it. In the 26.2 bytecode
            // that is the difference between
            //
            //   BlobFoliagePlacer   6: iload 9 (offset)  8: iload 7  10: isub
            //   SpruceFoliagePlacer 28: iload 7                      30: ineg
            //
            // — one instruction, and no `offset` load at all in the spruce
            // form. `spruce.json` samples `offset` from `uniform 0..2`, so
            // two thirds of every spruce tree in the world was short by one
            // or two rows here, and because `place_leaves_row` draws for the
            // corner cut, the missing rows also shifted the RNG stream for
            // every feature decorated after the tree in the same chunk.
            // Do not "regularise" this against its siblings.
            Self::Spruce { .. } => {
                let offset = self.offset_provider().sample(random);
                let mut radius = random.next_i32_bound(2);
                let mut cap = 1;
                let mut previous = 0;
                let mut y = offset;
                while y >= -foliage_height {
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        attachment.pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )?;
                    if radius >= cap {
                        radius = previous;
                        previous = 1;
                        cap = (cap + 1).min(foliage_radius + attachment.radius_offset);
                    } else {
                        radius += 1;
                    }
                    y -= 1;
                }
                Ok(())
            }

            // `PineFoliagePlacer#createFoliage`: widen to the radius, hold,
            // and narrow by one for the very top row.
            Self::Pine { .. } => {
                let offset = self.offset_provider().sample(random);
                let mut radius = 0;
                let mut y = offset;
                while y >= offset - foliage_height {
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        attachment.pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )?;
                    if radius >= 1 && y == offset - foliage_height + 1 {
                        radius -= 1;
                    } else if radius < foliage_radius + attachment.radius_offset {
                        radius += 1;
                    }
                    y -= 1;
                }
                Ok(())
            }

            // `BushFoliagePlacer#createFoliage`: the radius shrinks as the
            // rows rise, computed rather than stepped.
            Self::Bush { .. } => {
                let offset = self.offset_provider().sample(random);
                let mut y = offset;
                while y >= offset - foliage_height {
                    let radius = foliage_radius + attachment.radius_offset - 1 - y;
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        attachment.pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )?;
                    y -= 1;
                }
                Ok(())
            }

            // `AcaciaFoliagePlacer#createFoliage` — three rows, no loop.
            //
            // The middle row is the *widest* only when `radiusOffset` is 0;
            // with the forking trunk's main arm it is 1, so the top and
            // bottom rows step out past it. That asymmetry is the acacia
            // silhouette, and it is why the row radii are written out rather
            // than derived from one another.
            Self::Acacia { offset, .. } => {
                let offset = offset.sample(random);
                let pos = (
                    attachment.pos.0,
                    attachment.pos.1 + offset,
                    attachment.pos.2,
                );
                let row = |world: &mut W,
                           random: &mut dyn RandomSource,
                           placed: &mut Placed,
                           radius: i32,
                           y: i32|
                 -> Result<(), NeedsRules> {
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )
                };
                row(
                    world,
                    random,
                    placed,
                    foliage_radius + attachment.radius_offset,
                    -1 - foliage_height,
                )?;
                row(world, random, placed, foliage_radius - 1, -foliage_height)?;
                row(
                    world,
                    random,
                    placed,
                    foliage_radius + attachment.radius_offset - 1,
                    0,
                )?;
                Ok(())
            }
            // `MegaJungleFoliagePlacer#createFoliage`.
            //
            // ```java
            //   int n = attachment.doubleTrunk() ? foliageHeight
            //                                    : 1 + random.nextInt(2);
            //   for (int y = offset; y >= offset - n; y--) {
            //       int r = foliageRadius + attachment.radiusOffset() + 1 - y;
            //       placeLeavesRow(..., attachment.pos(), r, y,
            //                      attachment.doubleTrunk());
            //   }
            // ```
            //
            // **The `nextInt(2)` is conditional on the attachment**, and this
            // is the draw-order trap in the whole tree. One mega jungle tree
            // yields one double-trunk attachment (the crown, from
            // `GiantTrunkPlacer`) and one single-trunk attachment per branch
            // (from `MegaJungleTrunkPlacer`), so a single tree takes the
            // branch draw a variable number of times and the crown draw
            // never. Hoisting it out of the conditional would spend one extra
            // value per tree and displace every feature decorated after it in
            // the chunk.
            Self::MegaJungle { offset, .. } => {
                let offset = offset.sample(random);
                let rows = if attachment.double_trunk {
                    foliage_height
                } else {
                    1 + random.next_i32_bound(2)
                };
                let mut y = offset;
                while y >= offset - rows {
                    // Note the `+ 1 - y` rather than the blob's `- 1 - y / 2`:
                    // the row widens as `y` falls instead of narrowing, and it
                    // is not clamped at zero because it cannot go negative
                    // over the range walked here.
                    let radius = foliage_radius + attachment.radius_offset + 1 - y;
                    place_leaves_row(
                        world,
                        tags,
                        config,
                        random,
                        attachment.pos,
                        radius,
                        y,
                        attachment.double_trunk,
                        placed,
                    )?;
                    y -= 1;
                }
                Ok(())
            }
            // `DarkOakFoliagePlacer#createFoliage` — three fixed rows and a
            // coin for a fourth, or two fixed rows and no coin.
            //
            // `foliageHeight` is never read here. The rows are placed at
            // -1, 0, +1 (and maybe +2) relative to `pos.above(offset)`, which
            // is the whole vertical extent of the canopy.
            Self::DarkOak { offset, .. } => {
                let offset = offset.sample(random);
                let pos = (
                    attachment.pos.0,
                    attachment.pos.1 + offset,
                    attachment.pos.2,
                );
                let dbl = attachment.double_trunk;
                let row = |world: &mut W,
                           random: &mut dyn RandomSource,
                           range: i32,
                           y: i32,
                           placed: &mut Placed|
                 -> Result<(), NeedsRules> {
                    place_leaves_row(world, tags, config, random, pos, range, y, dbl, placed)
                };
                if dbl {
                    row(world, random, foliage_radius + 2, -1, placed)?;
                    row(world, random, foliage_radius + 3, 0, placed)?;
                    row(world, random, foliage_radius + 2, 1, placed)?;
                    // THE ONE DRAW IN THIS PLACER, and it is taken only by
                    // the crown. Note the top row's range is `foliageRadius`
                    // and not `foliageRadius + something`.
                    if random.next_bool() {
                        row(world, random, foliage_radius, 2, placed)?;
                    }
                } else {
                    row(world, random, foliage_radius + 2, -1, placed)?;
                    row(world, random, foliage_radius + 1, 0, placed)?;
                }
                Ok(())
            }
            Self::Unimplemented { name } => Err(NeedsRules(format!("{name} foliage placer"))),
        }
    }

    /// The `offset` `IntProvider` every implemented placer carries.
    fn offset_provider(&self) -> &IntProvider {
        match self {
            Self::Blob { offset, .. }
            | Self::Spruce { offset, .. }
            | Self::Pine { offset, .. }
            | Self::Bush { offset, .. }
            | Self::Acacia { offset, .. }
            | Self::Fancy { offset, .. }
            | Self::MegaJungle { offset, .. }
            | Self::DarkOak { offset, .. } => offset,
            Self::Unimplemented { .. } => unreachable!("refused before this point"),
        }
    }

    /// `FoliagePlacer#shouldSkipLocationSigned` — the collapse from signed
    /// coordinates to the distance from the nearer trunk column, and then the
    /// subclass's own rule.
    ///
    /// It is a separate step because **`DarkOakFoliagePlacer` overrides it**,
    /// and its extra rule reads the SIGNED coordinates: `x == -range` is not
    /// the same question as `x.abs() == range` once the collapse has happened,
    /// and folding it into [`Self::should_skip`] would test the wrong corner.
    /// No other placer here overrides it.
    fn should_skip_signed(
        &self,
        random: &mut dyn RandomSource,
        x: i32,
        y: i32,
        z: i32,
        range: i32,
        double_trunk: bool,
    ) -> bool {
        if matches!(self, Self::DarkOak { .. })
            && y == 0
            && double_trunk
            && (x == -range || x >= range)
            && (z == -range || z >= range)
        {
            return true;
        }
        // A double trunk is centred between two columns, so the distance is
        // to the nearer of them.
        let (px, pz) = if double_trunk {
            (x.abs().min((x - 1).abs()), z.abs().min((z - 1).abs()))
        } else {
            (x.abs(), z.abs())
        };
        self.should_skip(random, px, y, pz, range, double_trunk)
    }

    /// `BlobFoliagePlacer#shouldSkipLocation`, on already-absolute
    /// coordinates.
    fn should_skip(
        &self,
        random: &mut dyn RandomSource,
        x: i32,
        y: i32,
        z: i32,
        range: i32,
        double_trunk: bool,
    ) -> bool {
        match self {
            Self::Blob { .. } => {
                // Both coordinate tests come first: the draw happens only at
                // a corner.
                if x != range || z != range {
                    return false;
                }
                random.next_i32_bound(2) == 0 || y == 0
            }
            // Spruce and pine share one rule and it takes **no draw**: the
            // corner is skipped outright above y = 0 and kept at or below it.
            Self::Spruce { .. } | Self::Pine { .. } => x == range && z == range && range > 0,
            // Acacia has two rules, and which applies depends on the row.
            // On its own row (`y == 0`) it cuts the four corners *and* the
            // two axes, leaving a lattice; on every other row it behaves
            // like spruce. Neither takes a draw.
            Self::Acacia { .. } => {
                if y == 0 {
                    (x > 1 || z > 1) && x != 0 && z != 0
                } else {
                    x == range && z == range && range > 0
                }
            }
            // `FancyFoliagePlacer#shouldSkipLocation` — a genuine circle,
            // in `float`, and it **takes no draw**. The coordinates arrive
            // already absolute (`shouldSkipLocationSigned` applies `abs`
            // before delegating), so the `+ 0.5` offsets are applied to
            // non-negative numbers exactly as vanilla applies them.
            Self::Fancy { .. } => {
                #[expect(clippy::cast_precision_loss, reason = "vanilla computes this in f32")]
                let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "vanilla compares against a widened int"
                )]
                let limit = (range * range) as f32;
                fx * fx + fz * fz > limit
            }
            // A bush rounds its corners like a blob, but without the `y == 0`
            // reprieve — so the draw decides it alone.
            Self::Bush { .. } => {
                if x != range || z != range {
                    return false;
                }
                random.next_i32_bound(2) == 0
            }
            // `MegaJungleFoliagePlacer#shouldSkipLocation` — a circle in
            // INTEGERS, with a diagonal cut across it, and it **takes no
            // draw**:
            //
            // ```java
            //   if (x + z >= 7) return true;
            //   return x * x + z * z > range * range;
            // ```
            //
            // The `x + z >= 7` arm is not redundant with the circle: it is
            // an absolute cap that survives however large `range` grows, and
            // it is what flattens the canopy's corners at the widest rows.
            // `y` is unused, which is why a jungle row is the same shape all
            // the way down while its radius changes.
            Self::MegaJungle { .. } => x + z >= 7 || x * x + z * z > range * range,
            // `DarkOakFoliagePlacer#shouldSkipLocation` — two rows have a
            // rule and every other row has none, and neither takes a draw.
            // The `y == -1` rule applies only to a SINGLE trunk, which is why
            // this needs `double_trunk` even though the signed override
            // already consumed it once.
            Self::DarkOak { .. } => {
                if y == -1 && !double_trunk {
                    return x == range && z == range;
                }
                if y == 1 {
                    return x + z > range * 2 - 2;
                }
                false
            }
            Self::Unimplemented { .. } => false,
        }
    }
}

/// `FoliagePlacer#placeLeavesRow`.
#[allow(clippy::too_many_arguments, reason = "vanilla's own signature")]
fn place_leaves_row<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    pos: Pos,
    range: i32,
    y_offset: i32,
    double_trunk: bool,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    let extra = i32::from(double_trunk);
    for x in -range..=range + extra {
        for z in -range..=range + extra {
            if config
                .foliage_placer
                .should_skip_signed(random, x, y_offset, z, range, double_trunk)
            {
                continue;
            }
            let leaf = (pos.0 + x, pos.1 + y_offset, pos.2 + z);
            try_place_leaf(world, tags, config, random, leaf, placed)?;
        }
    }
    Ok(())
}

/// `FoliagePlacer#tryPlaceLeaf`.
///
/// The provider draws **after** the two rejection tests, so a leaf that
/// cannot go anywhere costs nothing.
fn try_place_leaf<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    pos: Pos,
    placed: &mut Placed,
) -> Result<bool, NeedsRules> {
    // `state.getValueOrElse(PERSISTENT, false)` — a leaf someone placed by
    // hand blocks the tree rather than being overwritten.
    let here = world.block_at(pos.0, pos.1, pos.2);
    if here.contains("persistent=true") {
        return Ok(false);
    }
    if !valid_tree_pos(world, tags, pos)? {
        return Ok(false);
    }
    let state = config
        .foliage_provider
        .state(world, tags, random, pos)
        .map_err(|e| NeedsRules(e.0))?;
    // `if (state.hasProperty(WATERLOGGED)) state.setValue(WATERLOGGED, ...)`.
    let state = if state.contains("waterlogged=") {
        let source = world.fluid_at(pos.0, pos.1, pos.2) == "minecraft:water";
        set_property(&state, "waterlogged", if source { "true" } else { "false" })
    } else {
        state
    };
    world.set_block(pos.0, pos.1, pos.2, &state);
    // A refusal here, not a crash. `java_hash_set` reproduces treeified
    // buckets now, so the only thing left that can refuse is a bucket
    // treeifying with two equal spread hashes in it — `java.util.HashMap`
    // would order that tree by `System.identityHashCode`, which nothing can
    // reproduce. It has never been observed on a measured window, because a
    // tree's positions are local and `(y + z * 31) * 31 + x` is injective
    // over a box that small. The project counts refusals so a thinner world
    // is reported rather than passed off as a complete one.
    //
    // It was an `.expect` here, which made a designed limit fatal, and the
    // effect was invisible for a long time because every measurement window
    // this project uses is ocean-ish: the first attempt to generate a
    // *forested* window died on it, and that is also why coverage looked the
    // way it did.
    placed.foliage.insert(pos).map_err(|e| NeedsRules(e.0))?;
    Ok(true)
}

/// `state.setValue(property, value)` on the canonical spelling.
fn set_property(state: &str, property: &str, value: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == property => format!("{property}={value}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    fn parse(json: &str) -> FoliagePlacer {
        FoliagePlacer::parse(&serde_json::from_str(json).expect("json")).expect("parse")
    }

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    fn mega_jungle() -> FoliagePlacer {
        parse(
            r#"{"type":"minecraft:jungle_foliage_placer",
                "radius":2,"offset":0,"height":2}"#,
        )
    }

    /// `MegaJungleFoliagePlacer` is registered as `jungle_foliage_placer`.
    /// The class name and the datapack name disagree, and matching the class
    /// name is a refusal that reads exactly like an unported placer.
    #[test]
    fn the_registry_name_is_jungle_not_mega_jungle() {
        assert!(mega_jungle().is_supported());
        assert_eq!(mega_jungle().name(), "jungle");
        assert!(!parse(r#"{"type":"minecraft:mega_jungle_foliage_placer"}"#).is_supported());
    }

    /// `x + z >= 7` is an ABSOLUTE cap and not a restatement of the circle:
    /// at range 5 the circle would keep (4,4) — 32 <= 25 is false, so the
    /// circle drops it too — but (3,4) is inside the circle at 25 <= 25 and
    /// the diagonal still cuts it. Losing that arm rounds the canopy's
    /// corners back on and places leaves vanilla does not.
    #[test]
    fn the_diagonal_cut_is_not_the_circle() {
        let p = mega_jungle();
        let mut r = random(1);
        // Inside the circle, cut by the diagonal.
        assert!(p.should_skip(&mut r, 3, 0, 4, 5, false));
        // Inside both.
        assert!(!p.should_skip(&mut r, 2, 0, 2, 5, false));
        // Outside the circle, under the diagonal.
        assert!(p.should_skip(&mut r, 3, 0, 0, 2, false));
    }

    /// `y` is not read at all, so a jungle row is the same shape at every
    /// height and only its radius changes.
    #[test]
    fn the_jungle_skip_ignores_y_and_takes_no_draw() {
        let p = mega_jungle();
        let mut r = random(7);
        let before = r.next_i32_bound(1_000_000);
        let mut r = random(7);
        for y in -4..4 {
            assert_eq!(
                p.should_skip(&mut r, 2, y, 2, 5, false),
                p.should_skip(&mut r, 2, 0, 2, 5, false)
            );
        }
        // No draw was spent by any of those calls.
        assert_eq!(r.next_i32_bound(1_000_000), before);
    }

    /// Pine's radius is the base class's **plus one of its own** — two draws
    /// where every other placer takes what its `IntProvider` costs.
    #[test]
    fn a_pine_radius_is_two_draws_not_one() {
        let pine = parse(
            r#"{"type":"minecraft:pine_foliage_placer",
                "radius":1,"offset":1,"height":{"type":"minecraft:uniform",
                "min_inclusive":3,"max_inclusive":4}}"#,
        );
        let mut r = random(5);
        let _ = pine.foliage_radius(&mut r, 6);

        let mut probe = random(5);
        // `radius` is a constant, so the base class draws nothing; the
        // override then draws `nextInt(max(trunkHeight + 1, 1))`.
        probe.next_i32_bound(7);
        assert_eq!(r.next_i64(), probe.next_i64());
    }

    /// A spruce's foliage height is `max(4, treeHeight - trunkHeight.sample)`,
    /// so unlike a blob's it costs a draw when the provider is not constant.
    #[test]
    fn a_spruce_foliage_height_draws_and_a_blobs_does_not() {
        let spruce = parse(
            r#"{"type":"minecraft:spruce_foliage_placer",
                "radius":{"type":"minecraft:uniform","min_inclusive":2,"max_inclusive":3},
                "offset":{"type":"minecraft:uniform","min_inclusive":0,"max_inclusive":1},
                "trunk_height":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":2}}"#,
        );
        let mut r = random(9);
        let height = spruce.foliage_height(&mut r, 10);
        assert!(height >= 4, "the floor of four applies: {height}");
        let mut probe = random(9);
        probe.next_i32_bound(2);
        assert_eq!(r.next_i64(), probe.next_i64());

        let blob =
            parse(r#"{"type":"minecraft:blob_foliage_placer","height":3,"offset":0,"radius":2}"#);
        let mut r = random(9);
        let mut probe = random(9);
        assert_eq!(blob.foliage_height(&mut r, 10), 3);
        assert_eq!(r.next_i64(), probe.next_i64(), "a blob's height is a field");
    }

    /// Spruce and pine round their corners **without a draw** — the rule is
    /// `range > 0`, not a coin. A port that reused the blob's rule would take
    /// one draw per corner per row and move everything downstream.
    #[test]
    fn a_conifer_corner_costs_no_draw() {
        for json in [
            r#"{"type":"minecraft:spruce_foliage_placer","radius":2,"offset":1,"trunk_height":1}"#,
            r#"{"type":"minecraft:pine_foliage_placer","radius":1,"offset":1,"height":3}"#,
        ] {
            let placer = parse(json);
            let mut r = random(2);
            let mut probe = random(2);
            // A corner: x == z == range, and range above zero.
            assert!(placer.should_skip(&mut r, 2, 0, 2, 2, false));
            assert_eq!(r.next_i64(), probe.next_i64(), "{json}");
        }
    }

    /// A bush's corner *is* a coin, and unlike a blob's there is no `y == 0`
    /// reprieve.
    #[test]
    fn a_bush_corner_is_decided_by_one_draw() {
        let bush =
            parse(r#"{"type":"minecraft:bush_foliage_placer","radius":2,"offset":0,"height":2}"#);
        let mut r = random(7);
        let mut probe = random(7);
        let _ = bush.should_skip(&mut r, 2, 0, 2, 2, false);
        probe.next_i32_bound(2);
        assert_eq!(r.next_i64(), probe.next_i64());
    }

    #[test]
    fn every_foliage_placer_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:tree" {
                continue;
            }
            FoliagePlacer::parse(&json["config"]["foliage_placer"])
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            seen += 1;
        }
        assert_eq!(seen, 39, "every tree's foliage placer must at least parse");
    }
}

#[cfg(test)]
mod dark_oak_tests {
    use super::*;
    use crate::rng::draws;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    fn dark_oak() -> FoliagePlacer {
        FoliagePlacer::parse(
            &serde_json::from_str(
                r#"{"type":"minecraft:dark_oak_foliage_placer","radius":0,"offset":0}"#,
            )
            .expect("json"),
        )
        .expect("parse")
    }

    #[test]
    fn the_registry_name_is_shared_by_dark_oak_and_pale_oak() {
        // `minecraft:dark_oak`, `dark_oak_leaf_litter`, `pale_oak`,
        // `pale_oak_bonemeal` and `pale_oak_creaking` all name this one
        // placer with the same two zeroes, so getting it wrong is five trees.
        assert!(dark_oak().is_supported());
        assert_eq!(dark_oak().name(), "dark_oak");
    }

    /// `foliageHeight` is a literal 4 and takes no draw — and nothing in
    /// `createFoliage` reads it. It matters anyway: `TreeFeature` subtracts it
    /// from the tree height to get the trunk height.
    #[test]
    fn the_foliage_height_is_a_drawless_four() {
        let mut r = WorldgenRandom::new(LegacyRandomSource::new(3));
        draws::arm();
        assert_eq!(dark_oak().foliage_height(&mut r, 12), 4);
        assert_eq!(dark_oak().foliage_height(&mut r, 30), 4);
        assert_eq!(draws::take(), 0);
    }

    /// The signed override cuts the four outer corners of the middle row of a
    /// DOUBLE trunk, and `x == -range` is not the same question as
    /// `x.abs() == range` once the row has been collapsed — which is why it
    /// cannot live in `should_skip`.
    #[test]
    fn the_signed_override_cuts_only_a_double_trunks_middle_row() {
        let p = dark_oak();
        let mut r = WorldgenRandom::new(LegacyRandomSource::new(1));
        // y == 0, double trunk, both coordinates at a boundary.
        assert!(p.should_skip_signed(&mut r, -3, 0, -3, 3, true));
        assert!(p.should_skip_signed(&mut r, 4, 0, -3, 3, true));
        // A single trunk never takes this arm.
        assert!(!p.should_skip_signed(&mut r, -3, 0, -3, 3, false));
        // Nor does any other row. `y = 2` and not `y = 1`: at `y = 1` the
        // unsigned diagonal rule cuts that corner anyway, so a test there
        // would pass whether the override checked `y == 0` or not.
        assert!(!p.should_skip_signed(&mut r, -3, 2, -3, 3, true));
        assert!(
            p.should_skip_signed(&mut r, -3, 1, -3, 3, true),
            "the diagonal"
        );
        // `x = 3` is `>= range` and cut; `x = -3` is cut by the equality; but
        // `x = 2` is neither, and after the collapse `|2| = 2 != 3` anyway.
        assert!(!p.should_skip_signed(&mut r, 2, 0, -3, 3, true));
    }

    /// The two unsigned rules, and the fact that neither takes a draw.
    #[test]
    fn the_bottom_row_rounds_only_a_single_trunk_and_the_top_row_is_a_diagonal() {
        let p = dark_oak();
        let mut r = WorldgenRandom::new(LegacyRandomSource::new(5));
        draws::arm();
        // y == -1 and single trunk: the far corner only.
        assert!(p.should_skip(&mut r, 2, -1, 2, 2, false));
        assert!(!p.should_skip(&mut r, 2, -1, 1, 2, false));
        // y == -1 and double trunk: no rule at all.
        assert!(!p.should_skip(&mut r, 2, -1, 2, 2, true));
        // y == 1: `x + z > range * 2 - 2`.
        assert!(p.should_skip(&mut r, 2, 1, 2, 2, false));
        assert!(!p.should_skip(&mut r, 1, 1, 1, 2, false));
        // Every other row keeps everything.
        assert!(!p.should_skip(&mut r, 9, 0, 9, 2, false));
        assert!(!p.should_skip(&mut r, 9, 2, 9, 2, false));
        assert_eq!(draws::take(), 0, "no rule in this placer draws");
    }
}
