//! `BoneMealItem.useOn` — the crop arm, and a named refusal for the rest.
//!
//! # The shape of the method
//!
//! ```text
//!   BoneMealItem.useOn
//!     26-33   growCrop(stack, level, clickedPos)
//!     43-52     causeUseVibration(player, ITEM_INTERACT_FINISH)
//!     55-62     level.levelEvent(1505, clickedPos, 15)
//!        65     return SUCCESS_SERVER
//!    73-110   else: state.isFaceSturdy(clickedFace) && growWaterPlant(..., above)
//!   132-140     level.levelEvent(1505, above, 15)
//!       147   else PASS
//!
//!   growCrop
//!     12-17   block instanceof BonemealableBlock
//!     27-37   isValidBonemealTarget(level, pos, state)
//!     53-67   isBonemealSuccess(level, level.getRandom(), pos, state)
//!     70-80     performBonemeal(level, level.getRandom(), pos, state)
//!        86   stack.shrink(1)          <- INSIDE the success branch
//!        90   return true
//! ```
//!
//! Two orderings that a reading gets wrong and a trace shows:
//!
//! * **`shrink` is inside the `isValidBonemealTarget` branch, not inside
//!   `isBonemealSuccess`.** A bone meal used on a block that *could* grow but
//!   whose random roll failed is still consumed — offsets 67 and 85 both jump
//!   past `performBonemeal` to the same `shrink`. Only a target that was
//!   never valid costs nothing.
//! * **the block write happens before the `level_event`**, because
//!   `performBonemeal` is inside `growCrop` and the event is two calls above
//!   it in `useOn`.
//!
//! # What is ported and what is refused
//!
//! `data/vanilla/bonemeal.json` is the census: **63 blocks over 45 classes**
//! implement `BonemealableBlock`, and five of them are `CropBlock`. This
//! module answers the five and refuses the other 58 **by name**, because a
//! port that grew wheat and did nothing for a sapling would look, in every
//! trace, exactly like one that had not been written.
//!
//! The 58 are refused rather than approximated for a reason that is not
//! effort: most of their `performBonemeal`s place a **configured feature** —
//! a sapling grows a tree, nylium grows a nether forest patch — which is
//! worldgen reached through an item, and every one of them draws from
//! `Level.getRandom()` in an order this lane has not derived. A wrong tree is
//! a worse answer than no tree.

use rustmc_behavior::place_survive::{block_of, property};
use std::collections::BTreeMap;
use std::path::Path;

/// The one item that reaches this module.
///
/// `BoneMealItem` is a single registered item in 26.2 — the class has one
/// instance and no subclasses — so this is a constant rather than a table
/// column, and the fact that it is exactly one is what makes that safe.
pub const BONE_MEAL: &str = "minecraft:bone_meal";

/// Where `ci/bonemeal-golden.sh` writes it.
pub const TABLE_PATH: &str = "data/vanilla/bonemeal.json";

/// A `CropBlock`'s three overridable parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crop {
    /// `getMaxAge()`. **Not 7.** Beetroot is 3 and torchflower 2, and both
    /// override it, so a port that baked in `CropBlock`'s own value grows a
    /// beetroot to age 7 — a state that does not exist.
    pub max_age: i32,
    /// `getAgeProperty().getName()` — `age` for all five in 26.2, carried
    /// because `getAgeProperty` is an override too and a version that renamed
    /// it would otherwise move a state string this port spells by hand.
    pub age_property: String,
    /// Which class declares `getBonemealAgeIncrease`. See [`AgeIncrease`].
    pub age_increase: String,
}

/// How much one bone meal adds to a crop's age, and the RNG it costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgeIncrease {
    /// `CropBlock.getBonemealAgeIncrease` — `Mth.nextInt(random, 2, 5)`,
    /// which is `random.nextInt(4) + 2`. **One draw of bound 4**, and
    /// `Mth.nextInt(rs, lo, hi)` is `hi - lo + 1` as the bound, so 4 and not
    /// 3: the result is 2, 3, 4 or 5.
    Crop,
    /// `TorchflowerCropBlock.getBonemealAgeIncrease` is `iconst_1; ireturn` —
    /// **it draws nothing at all**. A port that routed every crop through the
    /// `CropBlock` version would take one draw too many and desynchronise
    /// every later draw on that level's random source, which is a divergence
    /// with no local symptom.
    One,
    /// `BeetrootBlock.getBonemealAgeIncrease` is
    /// `super.getBonemealAgeIncrease(level) / 3` — **integer division of the
    /// same single draw**, so it is 0 for a roll of 2 and 1 for 3, 4 or 5.
    /// A beetroot therefore fails to grow one bone meal in four while still
    /// consuming it, because `growCrop`'s `shrink` is above this call.
    CropOverThree,
}

impl AgeIncrease {
    /// Which one a declaring class is, or `None` for a class this port has
    /// not read.
    #[must_use]
    pub fn of(class: &str) -> Option<Self> {
        Some(match class.rsplit('.').next().unwrap_or(class) {
            "CropBlock" => Self::Crop,
            "TorchflowerCropBlock" => Self::One,
            "BeetrootBlock" => Self::CropOverThree,
            _ => return None,
        })
    }

    /// The increase, given the one `nextInt(4)` this family draws.
    ///
    /// The caller draws — rather than this function taking a random source —
    /// because [`Self::One`] must draw **nothing**, and a signature that
    /// handed a source in would make it far too easy to draw first and
    /// discard. `None` means take no draw at all.
    #[must_use]
    pub const fn draw_bound(self) -> Option<i32> {
        match self {
            Self::One => None,
            Self::Crop | Self::CropOverThree => Some(4),
        }
    }

    /// `Mth.nextInt(random, 2, 5)` applied to `roll`, then this family's own
    /// arithmetic. `roll` is what `nextInt(4)` returned, or ignored entirely
    /// for [`Self::One`].
    #[must_use]
    pub const fn increase(self, roll: i32) -> i32 {
        match self {
            Self::One => 1,
            Self::Crop => roll + 2,
            Self::CropOverThree => (roll + 2) / 3,
        }
    }
}

/// One block's row.
#[derive(Clone, Debug)]
pub struct Row {
    /// The block's Java class.
    pub class: String,
    /// `Some` when it is a `CropBlock`.
    pub crop: Option<Crop>,
    /// `isValidBonemealTarget` per state, as vanilla answered it in a world
    /// of nothing but that state at raw brightness 15.
    valid: BTreeMap<String, bool>,
}

/// `data/vanilla/bonemeal.json`.
#[derive(Clone, Debug, Default)]
pub struct BonemealTable {
    by_block: BTreeMap<String, Row>,
    level_event: i32,
    level_event_data: i32,
    set_block_flags: i32,
    shrink: i32,
}

impl BonemealTable {
    /// # Errors
    /// If the file is missing or malformed.
    pub fn shared(path: &Path) -> Result<&'static Self, String> {
        static ONCE: std::sync::OnceLock<Result<BonemealTable, String>> =
            std::sync::OnceLock::new();
        match ONCE.get_or_init(|| Self::load(path)) {
            Ok(t) => Ok(t),
            Err(e) => Err(e.clone()),
        }
    }

    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&json)
    }

    /// # Errors
    /// If the document has no `table` or is missing a constant.
    pub fn parse(json: &serde_json::Value) -> Result<Self, String> {
        let number = |key: &str| -> Result<i32, String> {
            json[key]
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("bonemeal.json has no `{key}`"))
        };
        let rows = json["table"]
            .as_array()
            .ok_or_else(|| "bonemeal.json has no `table`".to_string())?;
        let mut by_block = BTreeMap::new();
        for row in rows {
            let block = row["block"]
                .as_str()
                .ok_or_else(|| "a row has no `block`".to_string())?;
            let mut valid = BTreeMap::new();
            for entry in row["states"]
                .as_array()
                .ok_or_else(|| format!("{block} has no `states`"))?
            {
                let state = entry["state"]
                    .as_str()
                    .ok_or_else(|| format!("{block} has a state with no name"))?;
                // A `null` is a state whose rule the dumper could not answer.
                // Absent from the map rather than stored as `false`, so
                // "vanilla says no" and "nobody asked" stay different.
                if let Some(v) = entry["valid"].as_bool() {
                    valid.insert(state.to_string(), v);
                }
            }
            let crop = row["crop"].as_object().map(|c| Crop {
                max_age: c["maxAge"].as_i64().unwrap_or(0) as i32,
                age_property: c["ageProperty"].as_str().unwrap_or("age").to_string(),
                age_increase: c["ageIncrease"].as_str().unwrap_or_default().to_string(),
            });
            by_block.insert(
                block.to_string(),
                Row {
                    class: row["class"].as_str().unwrap_or_default().to_string(),
                    crop,
                    valid,
                },
            );
        }
        Ok(Self {
            by_block,
            level_event: number("levelEvent")?,
            level_event_data: number("levelEventData")?,
            set_block_flags: number("setBlockFlags")?,
            shrink: number("shrink")?,
        })
    }

    /// `1505`, off the table rather than spelled here — a version that
    /// renumbers the event moves the file and not this module.
    #[must_use]
    pub const fn level_event(&self) -> i32 {
        self.level_event
    }

    #[must_use]
    pub const fn level_event_data(&self) -> i32 {
        self.level_event_data
    }

    /// `2` — `UPDATE_CLIENTS` alone. A crop that grows runs no neighbour
    /// update and starts no shape cascade.
    #[must_use]
    pub const fn set_block_flags(&self) -> i32 {
        self.set_block_flags
    }

    /// `1` — how much `growCrop` shrinks the stack by on any valid target,
    /// successful roll or not.
    #[must_use]
    pub const fn shrink(&self) -> i32 {
        self.shrink
    }

    #[must_use]
    pub fn row(&self, state: &str) -> Option<&Row> {
        self.by_block.get(block_of(state))
    }

    /// `BonemealableBlock.isValidBonemealTarget`, or `None` when this block
    /// is not bonemealable at all or the extract could not answer the state.
    #[must_use]
    pub fn is_valid_target(&self, state: &str) -> Option<bool> {
        self.row(state)?.valid.get(state).copied()
    }

    /// How many blocks implement the interface — 63 in 26.2.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_block.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_block.is_empty()
    }

    /// How many of them this module can actually grow — the five crops.
    ///
    /// Printed on the boot line beside [`Self::len`] deliberately: the pair
    /// **63 / 5** is the honest statement of the port, where either number
    /// alone reads as coverage it does not have.
    #[must_use]
    pub fn ported(&self) -> usize {
        self.by_block
            .values()
            .filter(|r| {
                r.crop
                    .as_ref()
                    .is_some_and(|c| AgeIncrease::of(&c.age_increase).is_some())
            })
            .count()
    }
}

/// What a bone meal click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grow {
    /// `growCrop` returned false: not bonemealable, or not a valid target.
    /// `useOn` falls through to the water-plant arm and then to `PASS`, and
    /// the stack is **not** shrunk.
    Pass,
    /// A valid target this port cannot grow. The stack is not shrunk and
    /// nothing is written — which is not what vanilla does, and is why the
    /// count of blocks in this arm is on the boot line.
    Unported,
    /// A valid crop. Write `state` with [`BonemealTable::set_block_flags`],
    /// shrink the stack, then send the `level_event`.
    ///
    /// **`state` may equal the state that was there.** A beetroot whose
    /// `nextInt(4)` returned 0 gets an increase of `(0 + 2) / 3 == 0`, so the
    /// write is a no-op — and vanilla still writes it, still shrinks the
    /// stack and still sends the event. Collapsing that case into `Pass`
    /// would lose a packet and an item.
    Crop { state: String },
}

/// `growCrop`, for a crop, given the roll this family's `getBonemealAgeIncrease`
/// takes.
///
/// `roll` is what `random.nextInt(4)` returned; it is ignored for
/// `TorchflowerCropBlock`, which draws nothing — see [`AgeIncrease::One`].
/// The caller takes the draw, and takes it **only** when
/// [`AgeIncrease::draw_bound`] is `Some`, so the level's random source is
/// advanced exactly as often as vanilla advances it.
#[must_use]
pub fn grow_crop(table: Option<&BonemealTable>, state: &str, roll: i32) -> Grow {
    let Some(table) = table else {
        return Grow::Pass;
    };
    let Some(row) = table.row(state) else {
        // Not a `BonemealableBlock` at all — `growCrop` offset 17.
        return Grow::Pass;
    };
    if table.is_valid_target(state) != Some(true) {
        return Grow::Pass;
    }
    let Some(crop) = &row.crop else {
        return Grow::Unported;
    };
    let Some(increase) = AgeIncrease::of(&crop.age_increase) else {
        return Grow::Unported;
    };
    let Some(age) = property(state, &crop.age_property).and_then(|v| v.parse::<i32>().ok()) else {
        return Grow::Unported;
    };
    // `CropBlock.growCrops` offsets 0-18:
    // `Math.min(getMaxAge(), getAge(state) + getBonemealAgeIncrease(level))`.
    // `min` and not a clamp at both ends: the increase is never negative.
    let next = crop.max_age.min(age + increase.increase(roll));
    // `growCrops` writes `getStateForAge(next)`, and
    // `TorchflowerCropBlock.getStateForAge` is overridden: at 2 it returns
    // `Blocks.TORCHFLOWER.defaultBlockState()`. The crop's own `age`
    // property only runs 0..1, so the spelled-out `torchflower_crop[age=2]`
    // is a state nobody has, the write was refused, and a bone meal on a
    // nearly-grown torchflower was spent for nothing.
    if block_of(state) == "minecraft:torchflower_crop" && next == 2 {
        return Grow::Crop {
            state: "minecraft:torchflower".to_string(),
        };
    }
    Grow::Crop {
        state: format!("{}[{}={next}]", block_of(state), crop.age_property),
    }
}

/// Which draw, if any, this state's crop family takes. `None` means take no
/// draw — the caller must not advance the random source.
#[must_use]
pub fn draw_bound(table: Option<&BonemealTable>, state: &str) -> Option<i32> {
    let row = table?.row(state)?;
    let crop = row.crop.as_ref()?;
    AgeIncrease::of(&crop.age_increase)?.draw_bound()
}

#[cfg(test)]
mod tests;

/// `ClientboundLevelEventPacket`'s body — `int type`, `BlockPos pos`,
/// `int data`, `boolean globalEvent`.
///
/// Read off the packet's own reader and writer (offsets 0-36 of each), not
/// off a wiki: the position is a **packed long** via `writeBlockPos` and the
/// two ints are plain big-endian 32-bit, not varints, which is the one thing
/// a reader of this packet gets wrong.
///
/// `global` is `iconst_0` at `ServerLevel.levelEvent` offset 56 — the
/// three-argument `Level.levelEvent(type, pos, data)` every bone meal call
/// site uses can never set it.
#[must_use]
pub fn level_event_body(event: i32, pos: crate::interact::BlockPos, data: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(17);
    body.extend_from_slice(&event.to_be_bytes());
    body.extend_from_slice(&crate::interact::pack_pos(pos).to_be_bytes());
    body.extend_from_slice(&data.to_be_bytes());
    body.push(0);
    body
}
