//! `BoatItem.use` and `AbstractBoat`'s server tick — a boat put down from the
//! hand and left to settle.
//!
//! Transcribed from the pinned 26.2 jar, `javap -p -c -constants` on
//! `BoatItem`, `Item.getPlayerPOVHitResult`, `BlockGetter.clip`,
//! `AbstractBoat` and `FlowingFluid`.
//!
//! # The verb
//!
//! `BoatItem` overrides `Item.use`, not `useOn`, so a boat arrives through
//! `use_item` and lands wherever the player's view ray meets the world:
//!
//! ```text
//!    9-31   hit = getPlayerPOVHitResult(level, player, ClipContext.Fluid.ANY)
//!           MISS -> PASS
//!   33-150  any pickable entity whose box (inflated by its pick radius)
//!           contains the eye, among those in the player's box stretched 5
//!           along the view and inflated 1 -> PASS
//!  155-161  hit.getType() == BLOCK, else PASS
//!  171      getBoat: EntityType.create(level, SPAWN_ITEM_USE); setInitialPos(hit)
//!  188-191  boat.setYRot(player.getYRot())
//!  194-208  !level.noCollision(boat, boat.getBoundingBox()) -> FAIL
//!  222      level.addFreshEntity(boat)
//! ```
//!
//! The ray is `Block.OUTLINE` against [`OUTLINE_PATH`], a table this module
//! is the first reader of (`ci/outline-shapes-golden.sh`), and `Fluid.ANY`
//! against the fluid's own shape. A traversed state whose outline is not a
//! function of the state (an offset flower, scaffolding, a shulker box) is
//! **refused**: no boat, rather than a boat where vanilla would not put one.
//!
//! # The fluid shape is cached per fluid state, process-wide, and so is this one
//!
//! `FlowingFluid.getShape` never takes its `Shapes.block()` arm — it tests
//! `getAmount() == 9` and no fluid state has more than 8 — and so always
//! returns `shapes.computeIfAbsent(state, s -> box(0, 0, 0, 1,
//! s.getHeight(level, pos), 1))`. The **first** position a fluid state's
//! shape is asked at decides it for the life of the server: a source with
//! water above is `1.0` high there and `8/9` where nothing is above, and
//! whichever was asked first is what every later clip sees. This port keeps
//! the same cache ([`fluid_shape_height`]) rather than recomputing, because
//! recomputing is the behaviour vanilla does not have.
//!
//! # The tick, for a boat with no passenger
//!
//! `AbstractBoat.tick`: `oldStatus = status; status = getStatus()`, the
//! out-of-control counter, the hurt-time and damage decay, `Entity.tick`,
//! then (`isLocalInstanceAuthoritative`, which a passenger-less boat on the
//! server is) `floatBoat()` and `move(SELF, getDeltaMovement())`.
//!
//! # Not ported, by name
//!
//! Passengers and paddling (`controlBoat`, `positionRider`, the paddle
//! sounds — every one is behind a passenger), the end-of-tick push that
//! seats a nearby mob (`startRiding`) or pushes a player, bubble columns,
//! `Entity.baseTick`'s fluid current push, hurting and breaking a boat, and
//! the chest boats' container. The chest boats and rafts are placed and
//! ticked exactly as the boats are, which is what `AbstractBoat` gives them.

use crate::collide::{Aabb, CollisionBlocks, CollisionShapes, NoShapeFor, Vec3};
use crate::edits::Blocks;
use crate::entities::{Entities, Entity, EntityKind};
use crate::physics::{Body, MoverType, Profile};
use crate::projectile::clip::{shape_clip, traverse_blocks, BlockHit};
use rustmc_data::BlockPos;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

/// The outline table. The literal is what `ci/gen-gameplay-tables.sh`
/// greps for, so it is spelled whole.
pub const OUTLINE_PATH: &str = "data/vanilla/outline_shapes.json";

/// The twenty `AbstractBoat` types, each placed by the item of the same
/// name (`BoatItem(entityType, …)` in `Items`); the constants are in
/// [`crate::entities`].
pub const KINDS: [EntityKind; 20] = {
    use crate::entities as e;
    [
        e::OAK_BOAT,
        e::OAK_CHEST_BOAT,
        e::SPRUCE_BOAT,
        e::SPRUCE_CHEST_BOAT,
        e::BIRCH_BOAT,
        e::BIRCH_CHEST_BOAT,
        e::JUNGLE_BOAT,
        e::JUNGLE_CHEST_BOAT,
        e::ACACIA_BOAT,
        e::ACACIA_CHEST_BOAT,
        e::CHERRY_BOAT,
        e::CHERRY_CHEST_BOAT,
        e::DARK_OAK_BOAT,
        e::DARK_OAK_CHEST_BOAT,
        e::PALE_OAK_BOAT,
        e::PALE_OAK_CHEST_BOAT,
        e::MANGROVE_BOAT,
        e::MANGROVE_CHEST_BOAT,
        e::BAMBOO_RAFT,
        e::BAMBOO_CHEST_RAFT,
    ]
};

/// The boat type a held item places, if it is a `BoatItem`.
#[must_use]
pub fn kind_for_item(item: &str) -> Option<EntityKind> {
    KINDS.iter().copied().find(|k| k.name == item)
}

#[must_use]
pub fn is_boat(kind: EntityKind) -> bool {
    KINDS.contains(&kind)
}

/// `AbstractBoat$Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    InWater,
    UnderWater,
    UnderFlowingWater,
    OnLand,
    InAir,
}

/// The fields `AbstractBoat` adds that its passenger-less tick reads.
#[derive(Debug, Clone)]
pub struct BoatState {
    pub status: Option<Status>,
    pub old_status: Option<Status>,
    pub land_friction: f32,
    pub water_level: f64,
    pub out_of_control_ticks: f32,
    pub delta_rotation: f32,
    pub last_yd: f64,
    pub body: Body,
}

fn boats() -> &'static Mutex<BTreeMap<i32, BoatState>> {
    static B: OnceLock<Mutex<BTreeMap<i32, BoatState>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// `BlockStateBase.getOffset(pos)`'s inputs for one state: whether the
/// offset has a Y part (`OffsetType.XYZ`), and the block's
/// `getMaxHorizontalOffset()` / `getMaxVerticalOffset()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetRule {
    pub xyz: bool,
    pub max_h: f32,
    pub max_v: f32,
}

/// The outline table: the shapes, and for an offset block the rule that
/// moves its (unshifted) shape to where a position puts it.
#[derive(Debug)]
pub struct Outline {
    pub shapes: CollisionShapes,
    pub offsets: HashMap<u32, OffsetRule>,
}

/// `Mth.getSeed(x, y, z)`: `(long)(x * 3129871) ^ (long)z * 116129781L ^
/// (long)y`, then `l * l * 42317861L + l * 11L`, `>> 16`. The first product
/// is in `int` and wraps before it is widened.
#[must_use]
pub fn mth_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l = i64::from(x.wrapping_mul(3_129_871))
        ^ i64::from(z).wrapping_mul(116_129_781)
        ^ i64::from(y);
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// `BlockBehaviour$Properties.lambda$offsetType$0/$1`, the offset of a state
/// at a position. `Mth.getSeed(x, 0, z)`; each nibble `/ 15.0F` in float,
/// widened, then the double arithmetic and `Mth.clamp(double, double,
/// double)` against the float maximum.
#[must_use]
pub fn offset_at(rule: OffsetRule, x: i32, z: i32) -> (f64, f64, f64) {
    let seed = mth_seed(x, 0, z);
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own l2f")]
    let nib = |shift: u32| ((seed >> shift) & 15) as f32 / 15.0_f32;
    let clamp = |v: f64, max: f32| {
        let lo = f64::from(-max);
        if v < lo {
            lo
        } else {
            v.min(f64::from(max))
        }
    };
    let ox = clamp((f64::from(nib(0)) - 0.5) * 0.5, rule.max_h);
    let oz = clamp((f64::from(nib(8)) - 0.5) * 0.5, rule.max_h);
    let oy = if rule.xyz {
        (f64::from(nib(4)) - 1.0) * f64::from(rule.max_v)
    } else {
        0.0
    };
    (ox, oy, oz)
}

/// The outline shapes, loaded once.
#[must_use]
pub fn outline_shapes() -> Option<&'static Outline> {
    static TABLE: OnceLock<Option<Outline>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let cwd = std::path::Path::new(OUTLINE_PATH);
            let path = if cwd.exists() {
                cwd.to_path_buf()
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(OUTLINE_PATH)
            };
            let load = || -> std::io::Result<Outline> {
                let text = std::fs::read_to_string(&path)?;
                let doc: serde_json::Value =
                    serde_json::from_str(&text).map_err(std::io::Error::other)?;
                let shapes = CollisionShapes::from_json_carrier(&doc)?;
                let mut offsets = HashMap::new();
                for row in doc
                    .get("offsetStates")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let v: Vec<i64> = row
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_i64)
                        .collect();
                    let [id, xyz, h, vv] = v[..] else {
                        return Err(std::io::Error::other(
                            "an offsetStates row is not four integers",
                        ));
                    };
                    let bits = |b: i64| {
                        u32::try_from(b)
                            .map(f32::from_bits)
                            .map_err(std::io::Error::other)
                    };
                    offsets.insert(
                        u32::try_from(id).map_err(std::io::Error::other)?,
                        OffsetRule {
                            xyz: xyz != 0,
                            max_h: bits(h)?,
                            max_v: bits(vv)?,
                        },
                    );
                }
                Ok(Outline { shapes, offsets })
            };
            load()
                .map_err(|e| {
                    eprintln!(
                        "outline shapes at {} will not load ({e}) — no boat can be placed; \
                         run ci/gen-gameplay-tables.sh",
                        path.display()
                    );
                })
                .ok()
        })
        .as_ref()
}

// ---- fluids ---------------------------------------------------------------

/// A `FluidState`, recovered from `getFluidState(pos).createLegacyBlock()`:
/// `LiquidBlock.LEVEL` is `0` for a source, `8 - amount` for a flowing state
/// and `+ 8` when it is falling, so the legacy block names the fluid state
/// one-to-one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fluid {
    /// `FluidTags.WATER` — water or flowing water.
    pub water: bool,
    /// `FluidState.getAmount()`.
    pub amount: i32,
    /// `FluidState.isSource()`.
    pub source: bool,
    /// The legacy block, the cache key.
    pub key: String,
}

/// The fluid state at a legacy fluid block, or `None` for air.
#[must_use]
pub fn fluid_of(legacy: &str) -> Option<Fluid> {
    let (block, props) = legacy.split_once('[').map_or((legacy, ""), |(b, p)| (b, p));
    let water = block == "minecraft:water";
    if !water && block != "minecraft:lava" {
        return None;
    }
    let level: i32 = props
        .trim_end_matches(']')
        .split(',')
        .find_map(|kv| kv.strip_prefix("level="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // `LiquidBlock.getFluidState`: `stateCache[min(level, 8)]` — 0 the
    // source, 1..7 `getFlowing(8 - level, false)`, 8 `getFlowing(8, true)`.
    let (amount, source) = match level.min(8) {
        0 => (8, true),
        8 => (8, false),
        l => (8 - l, false),
    };
    Some(Fluid {
        water,
        amount,
        source,
        key: legacy.to_string(),
    })
}

fn fluid_at(blocks: &dyn Blocks, pos: BlockPos) -> Option<Fluid> {
    fluid_of(&blocks.legacy_fluid_at((pos.x, pos.y, pos.z)))
}

/// `FlowingFluid.getHeight(state, level, pos)`: `1.0` when the same fluid is
/// above, else `getOwnHeight()` — `amount / 9.0F`.
fn fluid_height(blocks: &dyn Blocks, f: &Fluid, pos: BlockPos) -> f32 {
    let above = fluid_at(
        blocks,
        BlockPos {
            x: pos.x,
            y: pos.y + 1,
            z: pos.z,
        },
    );
    if above.is_some_and(|a| a.water == f.water) {
        1.0
    } else {
        #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
        let a = f.amount as f32;
        a / 9.0
    }
}

/// `FlowingFluid.getShape`'s `shapes.computeIfAbsent`: the height the first
/// asker saw. See the module docs.
fn fluid_shape_height(blocks: &dyn Blocks, f: &Fluid, pos: BlockPos) -> f32 {
    static CACHE: OnceLock<Mutex<HashMap<String, f32>>> = OnceLock::new();
    let mut c = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("fluid shapes");
    *c.entry(f.key.clone())
        .or_insert_with(|| fluid_height(blocks, f, pos))
}

// ---- the view ray ---------------------------------------------------------

/// Why no boat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `PASS`: the ray missed, or the eye is inside a pickable entity.
    Pass,
    /// `FAIL`: the boat's box collides.
    Fail,
    /// The ray crossed a state this server has no outline for.
    NoShape(NoShapeFor),
    /// The tables are absent.
    NoTables,
}

struct Ids<'a>(&'a dyn Blocks);

impl CollisionBlocks for Ids<'_> {
    fn state_id_at(&self, pos: BlockPos) -> u32 {
        let s = self.0.state_at((pos.x, pos.y, pos.z));
        self.0
            .state_id(&s)
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(0)
    }
}

/// `BlockGetter.clip(ClipContext(from, to, Block.OUTLINE, Fluid.ANY, player))`,
/// as `Item.getPlayerPOVHitResult` asks it. `Ok(None)` is a miss.
///
/// ```text
///   visit(pos): block = ctx.getBlockShape(state).clip...  (clipWithInteractionOverride:
///                                                          the location is the outline's)
///               fluid = ctx.getFluidShape(fluidState).clip(from, to, pos)
///               d = block == null ? MAX : from.distanceToSqr(block.location)
///               e = fluid == null ? MAX : from.distanceToSqr(fluid.location)
///               return d <= e ? block : fluid
/// ```
///
/// # Errors
/// [`NoShapeFor`] for a traversed state with no outline.
pub fn pov_clip(
    blocks: &dyn Blocks,
    outline: &Outline,
    from: Vec3,
    to: Vec3,
) -> Result<Option<BlockHit>, NoShapeFor> {
    let ids = Ids(blocks);
    let mut hit = None;
    let mut err = None;
    traverse_blocks(from, to, &mut |pos| {
        let state_id = ids.state_id_at(pos);
        let Some(shape) = outline.shapes.get(state_id) else {
            err = Some(NoShapeFor { pos, state_id });
            return false;
        };
        // An offset block's shape is carried unshifted; `getShape` moves it
        // by `getOffset(pos)` — `VoxelShape.move`, one addition per grid line.
        let block = match outline.offsets.get(&state_id) {
            Some(rule) => {
                let (dx, dy, dz) = offset_at(*rule, pos.x, pos.z);
                shape_clip(&shape.moved([dx, dy, dz]), pos, from, to)
            }
            None => shape_clip(shape, pos, from, to),
        };
        let fluid = fluid_at(blocks, pos).and_then(|f| {
            let h = f64::from(fluid_shape_height(blocks, &f, pos));
            let s = crate::collide::Shape::single_box([0.0, 0.0, 0.0], [1.0, h, 1.0]);
            shape_clip(&s, pos, from, to)
        });
        let dist = |h: &Option<BlockHit>| {
            h.as_ref().map_or(f64::MAX, |h| {
                let (dx, dy, dz) = (
                    h.location.x - from.x,
                    h.location.y - from.y,
                    h.location.z - from.z,
                );
                dx * dx + dy * dy + dz * dz
            })
        };
        let chosen = if dist(&block) <= dist(&fluid) {
            block
        } else {
            fluid
        };
        match chosen {
            Some(h) => {
                hit = Some(h);
                false
            }
            None => true,
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(hit)
}

/// The boat's `Profile`: `sized(1.375F, 0.5625F)`, no step, `getDefaultGravity`
/// `ldc2_w 0.04d`, `getAirDrag` `fconst_1`, server-authoritative, not living.
pub const PROFILE: Profile = Profile {
    width: 1.375,
    height: 0.5625,
    max_up_step: 0.0,
    gravity: 0.04,
    air_drag: 1.0,
    entity_bounciness: 0.0,
    client_authoritative: false,
    can_simulate_movement: true,
    living: false,
};

/// Where the player is and where they look.
#[derive(Debug, Clone, Copy)]
pub struct Viewer {
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub y_rot: f32,
    pub x_rot: f32,
    /// `blockInteractionRange()`: 4.5, and 5.0 for a creative player
    /// (`ServerPlayer.updatePlayerAttributes` adds
    /// `creative_mode_block_range`, `+0.5 ADD_VALUE`).
    pub creative: bool,
}

/// `BoatItem.use` up to `addFreshEntity`: the boat, built, or why not.
///
/// # Errors
/// [`Refusal`] for `PASS` and `FAIL` and for what cannot be answered.
pub fn use_item(
    blocks: &dyn Blocks,
    collision: &CollisionShapes,
    kind: EntityKind,
    viewer: Viewer,
    entities: &Entities,
) -> Result<Entity, Refusal> {
    let outline = outline_shapes().ok_or(Refusal::NoTables)?;
    let eye = Vec3::new(
        viewer.x,
        viewer.y + crate::dropitem::PLAYER_EYE_HEIGHT,
        viewer.z,
    );
    let view = rustmc_entity::mth::calculate_view_vector(viewer.x_rot, viewer.y_rot);
    let range = if viewer.creative { 5.0 } else { 4.5 };
    let end = Vec3::new(
        eye.x + view.x * range,
        eye.y + view.y * range,
        eye.z + view.z * range,
    );
    let hit = pov_clip(blocks, outline, eye, end)
        .map_err(Refusal::NoShape)?
        .ok_or(Refusal::Pass)?;
    // 33-150: `player.getBoundingBox().expandTowards(view.scale(5)).inflate(1)`,
    // pickable entities in it, and any whose box contains the eye.
    let pb = Aabb::new(
        viewer.x - 0.3,
        viewer.y,
        viewer.z - 0.3,
        viewer.x + 0.3,
        viewer.y + 1.8,
        viewer.z + 0.3,
    )
    .expand_towards(view.x * 5.0, view.y * 5.0, view.z * 5.0)
    .inflate(1.0);
    for e in entities.all() {
        if e.id == viewer.entity_id || !pickable(&e) {
            continue;
        }
        let b = entity_box(&e);
        if !b.intersects(&pb) {
            continue;
        }
        // `getPickRadius()` is `Entity`'s `0.0F` for every kind here.
        if b.min_x <= eye.x
            && eye.x < b.max_x
            && b.min_y <= eye.y
            && eye.y < b.max_y
            && b.min_z <= eye.z
            && eye.z < b.max_z
        {
            return Err(Refusal::Pass);
        }
    }
    // 171 → getBoat: `setInitialPos(hit.x, hit.y, hit.z)`.
    let loc = hit.location;
    let bb = PROFILE.bounding_box(loc);
    // 194-208: `noCollision(boat, bb)` — blocks, entities the boat cannot
    // pass (`canBeCollidedWith`: another boat), and the world border.
    let ids = Ids(blocks);
    let blocked = crate::collide::collect_block_collisions(collision, &ids, bb)
        .map_err(Refusal::NoShape)?
        .iter()
        .any(|_| true);
    let boat_in_the_way = entities
        .all()
        .iter()
        .any(|e| is_boat(e.kind) && entity_box(e).intersects(&bb));
    if blocked || boat_in_the_way {
        return Err(Refusal::Fail);
    }
    let id = entities.next_id();
    let mut random = crate::dropitem::unique_random();
    let uuid = crate::dropitem::insecure_uuid(&mut random);
    Ok(Entity {
        id,
        uuid,
        kind,
        x: loc.x,
        y: loc.y,
        z: loc.z,
        y_rot: viewer.y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    })
}

/// Register a boat just added to the world.
pub fn track(e: &Entity) {
    boats().lock().expect("boats").insert(
        e.id,
        BoatState {
            status: None,
            old_status: None,
            land_friction: 0.0,
            water_level: 0.0,
            out_of_control_ticks: 0.0,
            delta_rotation: 0.0,
            last_yd: 0.0,
            body: Body::at(Vec3::new(e.x, e.y, e.z)),
        },
    );
}

fn pickable(e: &Entity) -> bool {
    // Players, mobs, the armor stand and boats; not items or orbs.
    e.kind == crate::entities::PLAYER || crate::mob::is_mob(e.id) || is_boat(e.kind)
}

fn entity_box(e: &Entity) -> Aabb {
    if is_boat(e.kind) {
        return PROFILE.bounding_box(Vec3::new(e.x, e.y, e.z));
    }
    if let Some(f) = crate::mob::facts_of_entity(e.id) {
        let b = f.bb;
        return Aabb::new(b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z);
    }
    if e.kind == crate::entities::PLAYER {
        return Aabb::new(e.x - 0.3, e.y, e.z - 0.3, e.x + 0.3, e.y + 1.8, e.z + 0.3);
    }
    Aabb::new(
        e.x - 0.125,
        e.y,
        e.z - 0.125,
        e.x + 0.125,
        e.y + 0.25,
        e.z + 0.125,
    )
}

// ---- the tick -------------------------------------------------------------

fn floor(v: f64) -> i32 {
    rustmc_entity::mth::floor(v)
}

fn ceil(v: f64) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "Mth.ceil")]
    {
        v.ceil() as i32
    }
}

/// `AbstractBoat.isUnderwater()`.
fn is_underwater(blocks: &dyn Blocks, bb: &Aabb) -> Option<Status> {
    let d0 = bb.max_y + 0.001;
    let mut flag = false;
    for x in floor(bb.min_x)..ceil(bb.max_x) {
        for y in floor(bb.max_y)..ceil(d0) {
            for z in floor(bb.min_z)..ceil(bb.max_z) {
                let pos = BlockPos { x, y, z };
                if let Some(f) = fluid_at(blocks, pos).filter(|f| f.water) {
                    if d0 < f64::from(y) + f64::from(fluid_height(blocks, &f, pos)) {
                        if !f.source {
                            return Some(Status::UnderFlowingWater);
                        }
                        flag = true;
                    }
                }
            }
        }
    }
    flag.then_some(Status::UnderWater)
}

/// `AbstractBoat.checkInWater()`; writes `waterLevel`.
fn check_in_water(blocks: &dyn Blocks, bb: &Aabb, st: &mut BoatState) -> bool {
    let mut flag = false;
    st.water_level = -f64::MAX;
    for x in floor(bb.min_x)..ceil(bb.max_x) {
        for y in floor(bb.min_y)..ceil(bb.min_y + 0.001) {
            for z in floor(bb.min_z)..ceil(bb.max_z) {
                let pos = BlockPos { x, y, z };
                if let Some(f) = fluid_at(blocks, pos).filter(|f| f.water) {
                    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
                    let top = y as f32 + fluid_height(blocks, &f, pos);
                    st.water_level = st.water_level.max(f64::from(top));
                    flag |= bb.min_y < f64::from(top);
                }
            }
        }
    }
    flag
}

/// `AbstractBoat.getGroundFriction()`: the mean friction of the blocks whose
/// collision shape meets a 0.001-thick slab under the boat, corners of the
/// one-block margin excluded, lily pads skipped.
fn ground_friction(
    blocks: &dyn Blocks,
    collision: &CollisionShapes,
    props: &crate::movement_props::MovementProps,
    bb: &Aabb,
) -> Result<f32, NoShapeFor> {
    let slab = Aabb::new(
        bb.min_x,
        bb.min_y - 0.001,
        bb.min_z,
        bb.max_x,
        bb.min_y,
        bb.max_z,
    );
    let (i, j) = (floor(slab.min_x) - 1, ceil(slab.max_x) + 1);
    let (k, l) = (floor(slab.min_y) - 1, ceil(slab.max_y) + 1);
    let (i1, j1) = (floor(slab.min_z) - 1, ceil(slab.max_z) + 1);
    let ids = Ids(blocks);
    let mut f = 0.0_f32;
    let mut n = 0_i32;
    for x in i..j {
        for z in i1..j1 {
            let edges = i32::from(x == i || x == j - 1) + i32::from(z == i1 || z == j1 - 1);
            if edges == 2 {
                continue;
            }
            for y in k..l {
                if edges > 0 && (y == k || y == l - 1) {
                    continue;
                }
                let pos = BlockPos { x, y, z };
                let state = blocks.state_at((x, y, z));
                if state.split('[').next() == Some("minecraft:lily_pad") {
                    continue;
                }
                let id = ids.state_id_at(pos);
                let shape = collision.get(id).ok_or(NoShapeFor { pos, state_id: id })?;
                if shape.overlaps(slab, [f64::from(x), f64::from(y), f64::from(z)]) {
                    f += props.friction(id);
                    n += 1;
                }
            }
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    Ok(f / n as f32)
}

/// `AbstractBoat.getStatus()`.
fn status(
    blocks: &dyn Blocks,
    collision: &CollisionShapes,
    props: &crate::movement_props::MovementProps,
    bb: &Aabb,
    st: &mut BoatState,
) -> Result<Status, NoShapeFor> {
    if let Some(s) = is_underwater(blocks, bb) {
        st.water_level = bb.max_y;
        return Ok(s);
    }
    if check_in_water(blocks, bb, st) {
        return Ok(Status::InWater);
    }
    let f = ground_friction(blocks, collision, props, bb)?;
    if f > 0.0 {
        st.land_friction = f;
        return Ok(Status::OnLand);
    }
    Ok(Status::InAir)
}

/// `AbstractBoat.floatBoat()` for a boat with no controlling passenger.
fn float_boat(blocks: &dyn Blocks, collision: &CollisionShapes, st: &mut BoatState) {
    let gravity = -PROFILE.gravity;
    let mut d1 = 0.0_f64;
    let mut inv_friction = 0.05_f32;
    let mut gravity_used = gravity;
    if st.old_status == Some(Status::InAir)
        && st.status != Some(Status::InAir)
        && st.status != Some(Status::OnLand)
    {
        // 45-135: dropped into water from the air — snap to the surface when
        // the box fits there.
        let bb = PROFILE.bounding_box(st.body.pos);
        // 45-49: `waterLevel = getY(1.0)` — the top of the box.
        st.water_level = bb.max_y;
        let level_above = water_level_above(blocks, &bb, st.last_yd);
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
        let target = f64::from(level_above - PROFILE.height as f32) + 0.101;
        let moved = bb.shifted(0.0, target - st.body.pos.y, 0.0);
        let ids = Ids(blocks);
        let free = crate::collide::collect_block_collisions(collision, &ids, moved)
            .is_ok_and(|v| v.is_empty());
        if free {
            st.body.pos = Vec3::new(st.body.pos.x, target, st.body.pos.z);
            let m = st.body.movement;
            st.body.movement = Vec3::new(m.x, 0.0, m.z);
            st.last_yd = 0.0;
        }
        st.status = Some(Status::InWater);
        return;
    }
    match st.status {
        Some(Status::InWater) => {
            d1 = (st.water_level - st.body.pos.y) / PROFILE.height;
            inv_friction = 0.9;
        }
        Some(Status::UnderFlowingWater) => {
            gravity_used = -7.0E-4;
            inv_friction = 0.9;
        }
        Some(Status::UnderWater) => {
            d1 = 0.009_999_999_776_482_582;
            inv_friction = 0.45;
        }
        Some(Status::InAir) => inv_friction = 0.9,
        // No controlling player, so `landFriction` is not halved.
        Some(Status::OnLand) => inv_friction = st.land_friction,
        None => {}
    }
    let m = st.body.movement;
    st.body.movement = Vec3::new(
        m.x * f64::from(inv_friction),
        m.y + gravity_used,
        m.z * f64::from(inv_friction),
    );
    st.delta_rotation *= inv_friction;
    if d1 > 0.0 {
        let m = st.body.movement;
        st.body.movement = Vec3::new(m.x, (m.y + d1 * (PROFILE.gravity / 0.65)) * 0.75, m.z);
    }
}

/// `AbstractBoat.getWaterLevelAbove()`, offsets 1-207: from the row the
/// box's top is in up to `ceil(maxY - lastYd)`, the first row whose highest
/// water surface is below a full block answers `y + height`; a row with a
/// full block of water moves on; running out answers `l + 1`.
fn water_level_above(blocks: &dyn Blocks, bb: &Aabb, last_yd: f64) -> f32 {
    let (i, j) = (floor(bb.min_x), ceil(bb.max_x));
    let (k, l) = (floor(bb.max_y), ceil(bb.max_y - last_yd));
    let (i1, j1) = (floor(bb.min_z), ceil(bb.max_z));
    'rows: for y in k..l {
        let mut f = 0.0_f32;
        for x in i..j {
            for z in i1..j1 {
                let pos = BlockPos { x, y, z };
                if let Some(fl) = fluid_at(blocks, pos).filter(|f| f.water) {
                    f = f.max(fluid_height(blocks, &fl, pos));
                }
                if f >= 1.0 {
                    continue 'rows;
                }
            }
        }
        if f < 1.0 {
            #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
            return y as f32 + f;
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    {
        (l + 1) as f32
    }
}

/// One tick of every boat: `AbstractBoat.tick()`'s passenger-less server
/// path. Called in the level's entity tick phase.
pub fn tick_all(
    blocks: &dyn Blocks,
    collision: &CollisionShapes,
    props: &crate::movement_props::MovementProps,
    entities: &Entities,
) {
    let ids: Vec<i32> = boats().lock().expect("boats").keys().copied().collect();
    for id in ids {
        if entities.get(id).is_none() {
            boats().lock().expect("boats").remove(&id);
            continue;
        }
        let Some(mut st) = boats().lock().expect("boats").remove(&id) else {
            continue;
        };
        // 1-13.
        st.old_status = st.status;
        let bb = PROFILE.bounding_box(st.body.pos);
        let Ok(s) = status(blocks, collision, props, &bb, &mut st) else {
            // A floor this server cannot shape: the boat stays put.
            boats().lock().expect("boats").insert(id, st);
            continue;
        };
        st.status = Some(s);
        // 17-54.
        if matches!(s, Status::UnderWater | Status::UnderFlowingWater) {
            st.out_of_control_ticks += 1.0;
        } else {
            st.out_of_control_ticks = 0.0;
        }
        // 79-116: hurt time and damage decay — both zero, never set here.
        // 150-199: floatBoat, then move(SELF, deltaMovement).
        float_boat(blocks, collision, &mut st);
        let vec = st.body.movement;
        let ids_ = Ids(blocks);
        let _ = crate::physics::move_entity(
            collision,
            props,
            &ids_,
            &PROFILE,
            &mut st.body,
            MoverType::SelfMove,
            vec,
        );
        // `checkFallDamage` override: `lastYd = getDeltaMovement().y`.
        st.last_yd = st.body.movement.y;
        let b = st.body;
        entities.update(id, |e| {
            e.x = b.pos.x;
            e.y = b.pos.y;
            e.z = b.pos.z;
            e.movement = (b.movement.x, b.movement.y, b.movement.z);
            e.on_ground = b.on_ground;
        });
        boats().lock().expect("boats").insert(id, st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_fluid_blocks_name_their_fluid_states() {
        let s = fluid_of("minecraft:water[level=0]").expect("water");
        assert!(s.water && s.source && s.amount == 8);
        let f = fluid_of("minecraft:water[level=3]").expect("flowing");
        assert!(!f.source && f.amount == 5);
        let fall = fluid_of("minecraft:water[level=8]").expect("falling");
        assert!(!fall.source && fall.amount == 8);
        assert!(fluid_of("minecraft:air").is_none());
        assert!(!fluid_of("minecraft:lava[level=0]").expect("lava").water);
    }

    /// Vanilla's own offsets, dumped by `OutlineShapesGolden` at six
    /// positions per offset block, against [`offset_at`].
    #[test]
    fn the_offset_formula_reproduces_vanillas_offsets_bit_for_bit() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(OUTLINE_PATH);
        if !rustmc_data::fixture::available(
            &path,
            "outline shapes extract",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
        let table = outline_shapes().expect("loads");
        let golden = doc["offsetGolden"].as_array().expect("offsetGolden");
        assert!(golden.len() >= 60, "only {} golden offsets", golden.len());
        for g in golden {
            let v: Vec<i64> = g
                .as_array()
                .expect("row")
                .iter()
                .filter_map(serde_json::Value::as_i64)
                .collect();
            let rule = table.offsets[&u32::try_from(v[0]).expect("id")];
            let (x, y, z) = offset_at(
                rule,
                i32::try_from(v[1]).expect("x"),
                i32::try_from(v[2]).expect("z"),
            );
            let want = |b: i64| f64::from_bits(b.cast_unsigned());
            assert_eq!(
                (x.to_bits(), y.to_bits(), z.to_bits()),
                (
                    want(v[3]).to_bits(),
                    want(v[4]).to_bits(),
                    want(v[5]).to_bits()
                ),
                "state {} at ({}, {})",
                v[0],
                v[1],
                v[2]
            );
        }
    }

    #[test]
    fn every_boat_item_names_its_own_entity_type() {
        assert_eq!(
            kind_for_item("minecraft:oak_boat").map(|k| k.name),
            Some("minecraft:oak_boat")
        );
        assert!(kind_for_item("minecraft:bamboo_chest_raft").is_some());
        assert!(kind_for_item("minecraft:oak_planks").is_none());
    }
}
