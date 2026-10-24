//! Light levels at a position, for the rules that read them: the random-tick
//! bodies in [`crate::growth`].
//!
//! # What vanilla asks and what this answers
//!
//! Four calls, all off the converged light engine:
//!
//! ```text
//!   LayerLightEventListener.getLightValue(pos)            getBrightness(layer, pos)
//!   LevelLightEngine.getRawBrightness(pos, amount)        max(sky - amount, block)
//!   LevelReader.getMaxLocalRawBrightness(pos, amount)     15 outside +-30,000,000,
//!                                                         else getRawBrightness
//!   LevelReader.getMaxLocalRawBrightness(pos)             (pos, getSkyDarken())
//! ```
//!
//! `CropBlock.randomTick` reads `getRawBrightness(pos, 0)`; a sapling, grass
//! and mycelium read `getMaxLocalRawBrightness(pos.above())`, which subtracts
//! the sky darkening; ice and a snow layer read the BLOCK layer alone.
//!
//! # How the value is computed, and why not from a stored light array
//!
//! This server keeps no live light: `crate::world`'s section light is built
//! once and never repaired (its own doc says so). So each query is answered
//! from the blocks, as a **reverse search** from the asked position out to
//! every source that could reach it:
//!
//! * a **sky source** is a position at or above its column's
//!   `ChunkSkyLightSources.findLowestSourceY`, the same column walk
//!   `crate::light_engine::lowest_source_y` transcribes — level 15;
//! * a **block source** is a state with `getLightEmission() > 0`.
//!
//! A source at `s` lights `pos` at `L(s) - sum(opacity(c))` over the cells
//! `c` entered on the way, `opacity` being `LightEngine.getOpacity` —
//! `max(1, getLightDampening())` — and a step is refused outright when the
//! two occlusion shapes close the face it crosses (`getLightDampeningInto`'s
//! 16). That is `LightEngine.propagateIncrease`'s arithmetic run backwards,
//! and the converged value of a max-plus flood is exactly the maximum over
//! paths, so the two agree wherever the flood converges — which
//! `ci/light-golden.sh` measured vanilla's own engine to do.
//!
//! The search stops at cost 15, past which no source can contribute, and
//! earlier when both layers already hold a level no further source could
//! beat. In the open — every rig in the gameplay corpus — it is one column
//! walk and no search at all.
//!
//! # What it is not
//!
//! * **Not stored-section-aware.** Vanilla's flood does not enter a section
//!   the storage does not hold (`crate::light_engine` rule 2). Every section
//!   within one of a non-air one is held, so the difference is a position
//!   more than sixteen blocks from any block, which no random-tick rule asks
//!   about.
//! * **Not a model of light that has not propagated yet.** Vanilla runs its
//!   light updates after the block writes of a tick, so a rule reading light
//!   in the same tick as the write that changed it reads the old value. This
//!   reads the new one. The column walk is cached per [`Light`], and a
//!   [`Light`] lives for one random-tick pass, which is the nearer of the two.
//! * **The union-of-two-partial-shapes residual** that
//!   `crate::light_engine::shape_occludes` documents, inherited unchanged.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::edits::Blocks;
use crate::interact::BlockPos;
use crate::light_engine::LightProps;

/// `LightEngine.MAX_LEVEL`.
const MAX: i32 = 15;

/// `Direction.values()` order, as `crate::light_engine` indexes faces.
const DIRS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// The four light columns of `block_props.json`, loaded once.
#[must_use]
pub fn props() -> Option<&'static LightProps> {
    static ONCE: std::sync::OnceLock<Option<LightProps>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let relative = std::path::Path::new("data/vanilla/block_props.json");
        let path = if relative.exists() {
            relative.to_path_buf()
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/vanilla/block_props.json")
        };
        LightProps::load(&path)
            .map_err(|e| eprintln!("brightness: no light columns ({e}) — every light read is 0"))
            .ok()
    })
    .as_ref()
}

/// What a caller needs out of a search, which decides when it may stop.
#[derive(Clone, Copy)]
enum Want {
    Both,
    Block,
    /// `max(block, sky - amount)`.
    Raw(i32),
}

/// One pass's light reader. Caches each column's lowest sky source and each
/// state's id, so a random-tick pass that reads the same columns thousands of
/// times walks each once.
pub struct Light<'a> {
    world: &'a dyn Blocks,
    props: Option<&'static LightProps>,
    min_y: i32,
    top_y: i32,
    has_sky: bool,
    sky_darken: i32,
    sources: RefCell<HashMap<(i32, i32), i32>>,
    ids: RefCell<HashMap<String, u16>>,
}

impl<'a> Light<'a> {
    /// A reader over `world`. `top_y` is the dimension's `min_y + height`,
    /// exclusive; `sky_darken` is `Level.getSkyDarken()` for this tick.
    #[must_use]
    pub fn new(world: &'a dyn Blocks, top_y: i32, has_sky: bool, sky_darken: i32) -> Self {
        Self::with_props(world, props(), top_y, has_sky, sky_darken)
    }

    /// [`Self::new`] with the table supplied, for tests.
    #[must_use]
    pub fn with_props(
        world: &'a dyn Blocks,
        props: Option<&'static LightProps>,
        top_y: i32,
        has_sky: bool,
        sky_darken: i32,
    ) -> Self {
        Self {
            world,
            props,
            min_y: world.level_min_y().unwrap_or(-64),
            top_y,
            has_sky,
            sky_darken,
            sources: RefCell::new(HashMap::new()),
            ids: RefCell::new(HashMap::new()),
        }
    }

    fn id(&self, state: &str) -> u16 {
        if let Some(&id) = self.ids.borrow().get(state) {
            return id;
        }
        let id = self
            .world
            .state_id(state)
            .and_then(|i| u16::try_from(i).ok())
            .unwrap_or(0);
        self.ids.borrow_mut().insert(state.to_string(), id);
        id
    }

    fn id_at(&self, pos: BlockPos) -> u16 {
        self.id(&self.world.state_at(pos))
    }

    /// `ChunkSkyLightSources.findLowestSourceY` for one column: walking down
    /// from the top, the first position whose edge with the one above it is
    /// occluded — non-zero dampening, or the two occlusion shapes closing the
    /// face — and one above it is the lowest source.
    fn lowest_source(&self, x: i32, z: i32) -> i32 {
        if let Some(&y) = self.sources.borrow().get(&(x, z)) {
            return y;
        }
        let y = self.walk_column(x, z);
        self.sources.borrow_mut().insert((x, z), y);
        y
    }

    fn walk_column(&self, x: i32, z: i32) -> i32 {
        let Some(props) = self.props else {
            return self.min_y;
        };
        let top = self
            .world
            .column_top(x, z)
            .unwrap_or(self.top_y)
            .min(self.top_y);
        let mut above = self.id("minecraft:air");
        let mut y = top - 1;
        while y >= self.min_y {
            let here = self.id_at((x, y, z));
            if crate::light_engine::edge_occluded(props, above, here) {
                return y + 1;
            }
            above = here;
            y -= 1;
        }
        self.min_y
    }

    /// `getBrightness(SKY, pos)` and `getBrightness(BLOCK, pos)`, together,
    /// because one search answers both.
    #[must_use]
    pub fn levels(&self, pos: BlockPos) -> (i32, i32) {
        self.search(pos, Want::Both)
    }

    /// The search, stopped as soon as nothing further out could change the
    /// answer the caller wants. Without the stop every query walked the whole
    /// radius-15 diamond looking for a torch, which is the cost of a grass
    /// block's random tick in the open: measured on a 5x5-chunk superflat at
    /// speed 4096, the pass spent 80% of its time there.
    fn search(&self, pos: BlockPos, want: Want) -> (i32, i32) {
        let Some(props) = self.props else {
            return (0, 0);
        };
        if pos.1 >= self.top_y {
            return (if self.has_sky { MAX } else { 0 }, 0);
        }
        if pos.1 < self.min_y {
            return (0, 0);
        }
        let wants_sky = !matches!(want, Want::Block);
        let is_sky_source =
            |p: BlockPos| wants_sky && self.has_sky && p.1 >= self.lowest_source(p.0, p.2);
        let done = |potential: i32, sky: i32, block: i32| match want {
            Want::Both => potential <= sky.min(block),
            Want::Block => potential <= block,
            Want::Raw(amount) => potential <= block.max(sky - amount),
        };
        let mut sky = if is_sky_source(pos) { MAX } else { 0 };
        let here_state = self.id_at(pos);
        let mut block = i32::from(props.emission(here_state));
        if done(MAX, sky, block) {
            return (sky, block);
        }
        // A bucket queue on cost: costs are small integers and a source can
        // only help while cost < 15.
        let mut best: HashMap<BlockPos, i32> = HashMap::new();
        let mut states: HashMap<BlockPos, u16> = HashMap::new();
        states.insert(pos, here_state);
        best.insert(pos, 0);
        let mut buckets: Vec<Vec<BlockPos>> = vec![Vec::new(); 16];
        buckets[0].push(pos);
        for cost in 0..MAX {
            let Some(frontier) = buckets.get_mut(usize::try_from(cost).unwrap_or(0)) else {
                break;
            };
            let frontier = std::mem::take(frontier);
            // Nothing reachable from here on can beat what the caller holds:
            // every source still to be found arrives at `MAX - cost - 1` at best.
            if done(MAX - cost - 1, sky, block) {
                break;
            }
            for x in frontier {
                if best.get(&x).copied() != Some(cost) {
                    continue;
                }
                let x_state = states[&x];
                // Entering `x` on the way to `pos` costs its opacity.
                let step = cost + i32::from(props.opacity(x_state));
                if step >= MAX {
                    continue;
                }
                for (di, d) in DIRS.iter().enumerate() {
                    let y = (x.0 + d.0, x.1 + d.1, x.2 + d.2);
                    if y.1 < self.min_y || y.1 >= self.top_y {
                        continue;
                    }
                    if best.get(&y).is_some_and(|&c| c <= step) {
                        continue;
                    }
                    let y_state = match states.get(&y) {
                        Some(&s) => s,
                        None => {
                            let s = self.id_at(y);
                            states.insert(y, s);
                            s
                        }
                    };
                    // Light travels y -> x, so the face crossed is y's face
                    // toward x: the opposite of `d`, which is `di ^ 1`.
                    if crate::light_engine::shape_occludes(props, y_state, x_state, di ^ 1) {
                        continue;
                    }
                    best.insert(y, step);
                    if is_sky_source(y) {
                        sky = sky.max(MAX - step);
                    }
                    let e = i32::from(props.emission(y_state));
                    if e > 0 {
                        block = block.max(e - step);
                    }
                    if let Some(b) = buckets.get_mut(usize::try_from(step).unwrap_or(15)) {
                        b.push(y);
                    }
                }
            }
        }
        (sky.max(0), block.max(0))
    }

    /// Whether the position is a sky source — the sky reaches it straight
    /// down, which is what `Level.canSeeSky` asks in a world whose light is
    /// converged.
    #[must_use]
    pub fn sky_open(&self, pos: BlockPos) -> bool {
        self.has_sky && (pos.1 >= self.top_y || pos.1 >= self.lowest_source(pos.0, pos.2))
    }

    /// `getBrightness(LightLayer.BLOCK, pos)`.
    #[must_use]
    pub fn block(&self, pos: BlockPos) -> i32 {
        self.search(pos, Want::Block).1
    }

    /// `LevelLightEngine.getRawBrightness(pos, amount)`:
    /// `max(block, sky - amount)`.
    #[must_use]
    pub fn raw(&self, pos: BlockPos, amount: i32) -> i32 {
        let (sky, block) = self.search(pos, Want::Raw(amount));
        block.max(sky - amount)
    }

    /// `LevelReader.getMaxLocalRawBrightness(pos)` — the sky darkened by the
    /// time of day, and 15 outside the world border's hard limit.
    #[must_use]
    pub fn max_local_raw(&self, pos: BlockPos) -> i32 {
        if pos.0 < -30_000_000 || pos.2 < -30_000_000 || pos.0 >= 30_000_000 || pos.2 >= 30_000_000
        {
            return MAX;
        }
        self.raw(pos, self.sky_darken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A world of explicit blocks over air, with the real state ids.
    struct World {
        blocks: Mutex<BTreeMap<BlockPos, String>>,
        ids: HashMap<String, i32>,
    }

    fn ids() -> Option<HashMap<String, i32>> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/vanilla/block_props.json");
        if !rustmc_data::fixture::available(
            &p,
            "vanilla block_props extract",
            "ci/block-props-golden.sh",
        ) {
            return None;
        }
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
        Some(
            doc["table"]
                .as_array()?
                .iter()
                .filter_map(|r| Some((r["state"].as_str()?.to_string(), r["id"].as_i64()? as i32)))
                .collect(),
        )
    }

    impl World {
        fn with(blocks: &[(BlockPos, &str)]) -> Option<Self> {
            Some(Self {
                blocks: Mutex::new(blocks.iter().map(|(p, s)| (*p, (*s).to_string())).collect()),
                ids: ids()?,
            })
        }
    }

    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            self.blocks
                .lock()
                .unwrap()
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".into())
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            self.blocks.lock().unwrap().insert(pos, state.into());
            Some(0)
        }
        fn default_state(&self, b: &str) -> Option<String> {
            Some(b.into())
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn state_id(&self, s: &str) -> Option<i32> {
            self.ids.get(s).copied()
        }
        fn replaceable(&self, _: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            false
        }
        fn face_sturdy(&self, _: BlockPos, _: crate::interact::Face) -> bool {
            false
        }
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _: BlockPos) -> bool {
            false
        }
        fn water_at(&self, _: BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:air".into()
        }
        fn level_min_y(&self) -> Option<i32> {
            Some(-64)
        }
        fn column_top(&self, x: i32, z: i32) -> Option<i32> {
            let b = self.blocks.lock().unwrap();
            Some(
                b.keys()
                    .filter(|p| p.0 == x && p.2 == z)
                    .map(|p| p.1 + 1)
                    .max()
                    .unwrap_or(-64),
            )
        }
    }

    fn light(w: &World) -> Light<'_> {
        Light::new(w, 320, true, 0)
    }

    #[test]
    fn open_sky_is_fifteen_and_inside_a_full_block_is_zero() {
        let Some(w) = World::with(&[((0, -61, 0), "minecraft:grass_block[snowy=false]")]) else {
            return;
        };
        let l = light(&w);
        assert_eq!(l.levels((0, -60, 0)), (15, 0));
        // The grass itself: dampening 15, so its own cell is no source, and
        // every neighbour that could feed it costs 15 to enter.
        assert_eq!(l.levels((0, -61, 0)).0, 0);
    }

    #[test]
    fn a_roof_lets_sky_in_from_the_side_one_level_per_step() {
        // A stone roof over (0,-60,0) only: the cell under it is lit from its
        // open neighbours at 15 - 1.
        let Some(w) = World::with(&[((0, -59, 0), "minecraft:stone")]) else {
            return;
        };
        assert_eq!(light(&w).levels((0, -60, 0)).0, 14);
    }

    #[test]
    fn a_glowstone_lights_its_neighbour_at_fourteen_and_fades_one_per_block() {
        let Some(w) = World::with(&[((0, -60, 0), "minecraft:glowstone")]) else {
            return;
        };
        let l = light(&w);
        assert_eq!(l.block((1, -60, 0)), 14);
        assert_eq!(l.block((3, -60, 0)), 12);
        assert_eq!(l.block((0, -60, 15)), 0, "15 blocks away is dark");
    }

    #[test]
    fn the_sky_darkening_is_taken_off_the_sky_and_not_the_block_layer() {
        let Some(w) = World::with(&[((0, -60, 0), "minecraft:glowstone")]) else {
            return;
        };
        let l = Light::new(&w, 320, true, 11);
        assert_eq!(
            l.max_local_raw((5, -60, 0)),
            10,
            "glowstone's 10 beats 15 - 11"
        );
        assert_eq!(l.max_local_raw((50, -60, 0)), 4, "the sky alone, darkened");
    }
}
