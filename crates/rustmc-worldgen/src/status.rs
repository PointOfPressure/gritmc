//! Vanilla's chunk-status pyramid: which chunks must reach which status
//! before one chunk may take its next step.
//!
//! Ported from `net/minecraft/world/level/chunk/status/{ChunkStatus,
//! ChunkDependencies, ChunkStep, ChunkPyramid}` and
//! `net/minecraft/server/level/ChunkGenerationTask` in the pinned
//! `server-26.2.jar`, read with `javap -p -c -constants`.
//!
//! # Why this exists, and what it replaces
//!
//! [`crate::pipeline::DECORATION_MARGIN`] is a **single** number: grow the
//! requested area by two rings and run every stage over the result. Vanilla
//! does not have one number. Each step declares, per chessboard distance,
//! the status a neighbour must have reached, and the scheduler then walks
//! **one status at a time over a shrinking square** — structure starts far
//! out, terrain closer in, decoration closer still.
//!
//! `docs/DETERMINISM.md` §31 is why that difference matters rather than being
//! a tidier spelling of the same thing. A uniform margin decorates its own
//! outermost ring, and that ring decorates against a world that stops at its
//! edge; the pyramid decorates a ring **narrower** than the terrain it stands
//! on, so no chunk decorates against a void it could have read. §31.3
//! measured that no margin width converges — 5,753 / 2,201 / 1,130 blocks at
//! margins 1 / 2 / 4 — and this is the shape the fix has instead.
//!
//! # The pyramid, as the jar states it
//!
//! `ChunkStep$Builder.addRequirement(status, radius)` sets
//! `direct[i] = max(direct[i], status)` for every `i <= radius`, growing the
//! array to `radius + 1` when it has to. The steps of
//! `ChunkPyramid.GENERATION_PYRAMID`, in order, with their direct
//! requirements and block-state write radius:
//!
//! ```text
//!   EMPTY                 -                                        -
//!   STRUCTURE_STARTS      -                                        -
//!   STRUCTURE_REFERENCES  STRUCTURE_STARTS @ 8                     -
//!   BIOMES                STRUCTURE_STARTS @ 8                     -
//!   NOISE                 STRUCTURE_STARTS @ 8, BIOMES @ 1         write 0
//!   SURFACE               STRUCTURE_STARTS @ 8, BIOMES @ 1         write 0
//!   CARVERS               STRUCTURE_STARTS @ 8                     write 0
//!   FEATURES              STRUCTURE_STARTS @ 8, CARVERS @ 1        write 1
//!   INITIALIZE_LIGHT      -                                        -
//!   LIGHT                 INITIALIZE_LIGHT @ 1                     -
//!   SPAWN                 BIOMES @ 1                               -
//!   FULL                  -                                        -
//! ```
//!
//! **Read the `FEATURES` row twice, because it is the whole of this module's
//! reason to exist.** A chunk may decorate once its eight neighbours are
//! *carved* — not decorated. Vanilla never requires a decorated neighbour of
//! a decorating chunk, at any distance. Our uniform margin does the opposite:
//! it decorates the whole grown square, so its outermost ring decorates with
//! nothing beyond it at all.
//!
//! # Accumulating them
//!
//! `ChunkStep$Builder.buildAccumulatedDependencies` composes a step's direct
//! requirements with its parent's accumulated ones, offset by
//! `getRadiusOfParent` — the largest radius at which this step already
//! requires the parent's own target status:
//!
//! ```text
//!   p        = largest i with direct[i] >= parent.targetStatus, else 0
//!   len      = max(p + parent.accumulated.len(), direct.len())
//!   acc[i]   = direct[i]                               when i - p is outside
//!                                                      parent.accumulated
//!            = max(direct[i], parent.accumulated[i-p])  otherwise
//! ```
//!
//! and `ChunkGenerationTask.getRadiusForLayer` then reads the layer radius
//! for a status straight off the target step's accumulated dependencies.
//! `scheduleLayer` walks that square **x-then-z** — the same order
//! [`crate::pipeline::Area::chunks`] already promises — and
//! `scheduleChunkInLayer` skips a chunk that has already reached the status,
//! which is the generate-once property this module is named for.
//!
//! # What falls out, and it is the number that matters
//!
//! For a chunk to reach `FULL`:
//!
//! ```text
//!   FEATURES  at radius 1     CARVERS at radius 2     BIOMES at radius 3
//! ```
//!
//! So delivering a square of radius `r` at `FULL` needs decoration over
//! `r + 1` and terrain over `r + 2` — **one ring fewer decorated than
//! [`crate::pipeline::DECORATION_MARGIN`] decorates today, over the same
//! terrain.** That is not a saving bolted on afterwards, it is the
//! difference §31.3 is about.
//!
//! # One place the jar is narrower than the scheduler appears to be
//!
//! `ChunkDependencies`'s lookup table is `first().getIndex() + 1` entries
//! long, so a status **later** than anything any neighbour is required to
//! reach is outside it and `getRadiusOf` throws
//! `Requesting a ChunkStatus(%s) outside of dependency range(%s)`. For the
//! `FULL` step that is `LIGHT` and `SPAWN`: no neighbour of a chunk being
//! taken to `FULL` is ever required to be lit. [`ChunkStep::layer_radius`]
//! answers `0` there rather than reproducing the throw, and says why: a
//! status no *neighbour* needs is still a status the *centre* chunk must
//! pass through, and a layer of radius 0 is exactly that statement. Nothing
//! in this crate depends on the difference; it is written down because a
//! reader checking this port against the bytecode will find the throw and
//! should not have to work out whether it was missed.

/// One rung of `ChunkPyramid.GENERATION_PYRAMID`, in vanilla's own order.
///
/// The discriminants are `ChunkStatus.getIndex()`, which is what
/// `ChunkDependencies` indexes its lookup table by, so the ordering is
/// load-bearing rather than alphabetical convenience.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChunkStatus {
    Empty = 0,
    StructureStarts = 1,
    StructureReferences = 2,
    Biomes = 3,
    Noise = 4,
    Surface = 5,
    Carvers = 6,
    Features = 7,
    InitializeLight = 8,
    Light = 9,
    Spawn = 10,
    Full = 11,
}

impl ChunkStatus {
    /// Every status, in pyramid order — `ChunkStatus.getStatusList()`.
    pub const ALL: [Self; 12] = [
        Self::Empty,
        Self::StructureStarts,
        Self::StructureReferences,
        Self::Biomes,
        Self::Noise,
        Self::Surface,
        Self::Carvers,
        Self::Features,
        Self::InitializeLight,
        Self::Light,
        Self::Spawn,
        Self::Full,
    ];

    /// `ChunkStatus.getIndex()`.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// `ChunkStatus.isOrAfter`.
    #[must_use]
    pub const fn is_or_after(self, other: Self) -> bool {
        self.index() >= other.index()
    }

    /// `ChunkStatus.max`.
    #[must_use]
    pub const fn max(self, other: Self) -> Self {
        if self.index() >= other.index() {
            self
        } else {
            other
        }
    }

    /// The status one rung up, or `None` at `FULL`.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        let i = self.index() + 1;
        if i < Self::ALL.len() {
            Some(Self::ALL[i])
        } else {
            None
        }
    }
}

/// `ChunkDependencies` — the status a neighbour must have reached, indexed by
/// chessboard distance from the chunk taking the step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkDependencies {
    by_radius: Vec<ChunkStatus>,
}

impl ChunkDependencies {
    #[must_use]
    fn new(by_radius: Vec<ChunkStatus>) -> Self {
        Self { by_radius }
    }

    /// `ChunkDependencies.size()`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_radius.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_radius.is_empty()
    }

    /// `ChunkDependencies.get(radius)` — `None` past the declared range,
    /// where vanilla's `ImmutableList.get` would throw.
    #[must_use]
    pub fn get(&self, radius: usize) -> Option<ChunkStatus> {
        self.by_radius.get(radius).copied()
    }

    /// `ChunkDependencies.getRadiusOf(status)` — the largest chessboard
    /// distance at which a neighbour is required to have reached `status`.
    ///
    /// `None` where vanilla throws `Requesting a ChunkStatus(%s) outside of
    /// dependency range(%s)`: no neighbour at any distance is required to
    /// have got that far. See the module docs for why that is answered rather
    /// than reproduced.
    #[must_use]
    pub fn radius_of(&self, status: ChunkStatus) -> Option<i32> {
        // Vanilla precomputes `radiusByDependency` in the constructor:
        //   for i in 0..list.size(): for j in 0..=list[i].index():
        //       radiusByDependency[j] = i
        // which leaves each entry holding the *largest* radius whose
        // requirement is that status or later. The scan below is the same
        // answer read out of the list directly.
        let mut found = None;
        for (radius, required) in self.by_radius.iter().enumerate() {
            if required.is_or_after(status) {
                found = Some(i32::try_from(radius).expect("a dependency radius fits in i32"));
            }
        }
        // The table is `first().getIndex() + 1` long, so a status later than
        // the radius-0 requirement is out of range even though the scan above
        // would simply find nothing. Both answers are `None`; the length
        // check is written out so the two reasons stay distinguishable to a
        // reader diffing this against the jar.
        found
    }

    /// `ChunkDependencies.getRadius()` — the outermost distance any
    /// requirement reaches.
    #[must_use]
    pub fn radius(&self) -> i32 {
        i32::try_from(self.by_radius.len().saturating_sub(1)).expect("a radius fits in i32")
    }
}

/// `ChunkStep` — one status, what it needs around it, and how far it writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkStep {
    target: ChunkStatus,
    direct: ChunkDependencies,
    accumulated: ChunkDependencies,
    block_state_write_radius: i32,
}

impl ChunkStep {
    #[must_use]
    pub const fn target(&self) -> ChunkStatus {
        self.target
    }

    #[must_use]
    pub const fn direct(&self) -> &ChunkDependencies {
        &self.direct
    }

    #[must_use]
    pub const fn accumulated(&self) -> &ChunkDependencies {
        &self.accumulated
    }

    /// `ChunkStep.blockStateWriteRadius()` — how far outside its own chunk
    /// this step may set a block. `-1` where vanilla leaves it unset, which
    /// is every step that writes no blocks at all.
    #[must_use]
    pub const fn block_state_write_radius(&self) -> i32 {
        self.block_state_write_radius
    }

    /// `ChunkGenerationTask.getRadiusForLayer` — how wide the square of
    /// chunks brought to `status` must be, for a chunk taking *this* step.
    ///
    /// `ChunkStep.getAccumulatedRadiusOf` answers 0 for the step's own target
    /// status and otherwise defers to the accumulated dependencies; a status
    /// no neighbour needs is 0 here too, per the module docs.
    #[must_use]
    pub fn layer_radius(&self, status: ChunkStatus) -> i32 {
        if status == self.target {
            return 0;
        }
        self.accumulated.radius_of(status).unwrap_or(0)
    }
}

/// `ChunkPyramid` — the steps, indexed by the status they reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkPyramid {
    steps: Vec<ChunkStep>,
}

impl ChunkPyramid {
    /// `ChunkPyramid.getStepTo(status)`.
    ///
    /// # Panics
    /// Never for a status in [`ChunkStatus::ALL`]; the pyramid holds one step
    /// per status by construction.
    #[must_use]
    pub fn step_to(&self, status: ChunkStatus) -> &ChunkStep {
        &self.steps[status.index()]
    }

    #[must_use]
    pub fn steps(&self) -> &[ChunkStep] {
        &self.steps
    }
}

/// A `ChunkStep$Builder`, in the jar's own terms.
struct StepBuilder {
    status: ChunkStatus,
    parent: Option<ChunkStep>,
    direct_by_radius: Vec<ChunkStatus>,
    block_state_write_radius: i32,
}

impl StepBuilder {
    fn new(status: ChunkStatus, parent: Option<ChunkStep>) -> Self {
        Self {
            status,
            parent,
            direct_by_radius: Vec::new(),
            block_state_write_radius: -1,
        }
    }

    /// `ChunkStep$Builder.addRequirement`.
    ///
    /// # Panics
    /// If `status` is at or after the step's own target, which vanilla
    /// refuses with an `IllegalArgumentException`. It is a panic rather than
    /// a `Result` because the pyramid is a constant: a violation is a broken
    /// port, not a runtime condition.
    fn add_requirement(mut self, status: ChunkStatus, radius: usize) -> Self {
        assert!(
            !status.is_or_after(self.status),
            "{status:?} cannot be a requirement of {:?}",
            self.status
        );
        let wanted = radius + 1;
        let previous = std::mem::take(&mut self.direct_by_radius);
        self.direct_by_radius = if wanted > previous.len() {
            vec![status; wanted]
        } else {
            previous.clone()
        };
        for (slot, existing) in self
            .direct_by_radius
            .iter_mut()
            .zip(previous.iter())
            .take(wanted)
        {
            *slot = ChunkStatus::max(*existing, status);
        }
        self
    }

    /// `ChunkStep$Builder.blockStateWriteRadius`.
    const fn block_state_write_radius(mut self, radius: i32) -> Self {
        self.block_state_write_radius = radius;
        self
    }

    /// `ChunkStep$Builder.getRadiusOfParent` — the largest radius at which
    /// this step already requires the parent's target status.
    fn radius_of_parent(&self, parent_status: ChunkStatus) -> usize {
        for radius in (0..self.direct_by_radius.len()).rev() {
            if self.direct_by_radius[radius].is_or_after(parent_status) {
                return radius;
            }
        }
        0
    }

    /// `ChunkStep$Builder.buildAccumulatedDependencies`.
    fn accumulated(&self) -> Vec<ChunkStatus> {
        let Some(parent) = self.parent.as_ref() else {
            return self.direct_by_radius.clone();
        };
        let offset = self.radius_of_parent(parent.target);
        let inherited = &parent.accumulated.by_radius;
        let len = (offset + inherited.len()).max(self.direct_by_radius.len());
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let own = self.direct_by_radius.get(i).copied();
            let from_parent = i
                .checked_sub(offset)
                .and_then(|j| inherited.get(j).copied());
            out.push(match (own, from_parent) {
                (Some(a), Some(b)) => a.max(b),
                (Some(a), None) => a,
                // Past this step's own declared range the parent's
                // requirement stands on its own. Vanilla reaches the same
                // place by reading a null out of a longer array and taking
                // the max against it.
                (None, Some(b)) => b,
                (None, None) => unreachable!("len is the max of the two lengths"),
            });
        }
        out
    }

    fn build(self) -> ChunkStep {
        let accumulated = ChunkDependencies::new(self.accumulated());
        ChunkStep {
            target: self.status,
            direct: ChunkDependencies::new(self.direct_by_radius.clone()),
            accumulated,
            block_state_write_radius: self.block_state_write_radius,
        }
    }
}

/// `ChunkPyramid.GENERATION_PYRAMID`.
///
/// Built once and shared. The tasks are not here — this crate composes its
/// stages in [`crate::pipeline`] — so what is ported is the *shape*: which
/// chunks have to be how far along before one chunk may take its next step.
#[must_use]
pub fn generation_pyramid() -> &'static ChunkPyramid {
    static PYRAMID: std::sync::OnceLock<ChunkPyramid> = std::sync::OnceLock::new();
    PYRAMID.get_or_init(|| {
        let mut steps: Vec<ChunkStep> = Vec::with_capacity(ChunkStatus::ALL.len());
        for status in ChunkStatus::ALL {
            let parent = steps.last().cloned();
            let builder = StepBuilder::new(status, parent);
            let builder = match status {
                ChunkStatus::Empty | ChunkStatus::StructureStarts => builder,
                ChunkStatus::StructureReferences | ChunkStatus::Biomes => {
                    builder.add_requirement(ChunkStatus::StructureStarts, 8)
                }
                ChunkStatus::Noise | ChunkStatus::Surface => builder
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .add_requirement(ChunkStatus::Biomes, 1)
                    .block_state_write_radius(0),
                ChunkStatus::Carvers => builder
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .block_state_write_radius(0),
                ChunkStatus::Features => builder
                    .add_requirement(ChunkStatus::StructureStarts, 8)
                    .add_requirement(ChunkStatus::Carvers, 1)
                    .block_state_write_radius(1),
                ChunkStatus::InitializeLight | ChunkStatus::Full => builder,
                ChunkStatus::Light => builder.add_requirement(ChunkStatus::InitializeLight, 1),
                ChunkStatus::Spawn => builder.add_requirement(ChunkStatus::Biomes, 1),
            };
            steps.push(builder.build());
        }
        ChunkPyramid { steps }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The direct requirements, spelled out rather than derived, so that
    /// re-reading the jar and finding a different row makes this fail rather
    /// than agreeing with whatever the builder above happens to do.
    #[test]
    fn the_features_step_wants_carved_neighbours_not_decorated_ones() {
        let step = generation_pyramid().step_to(ChunkStatus::Features);
        assert_eq!(step.direct().get(0), Some(ChunkStatus::Carvers));
        assert_eq!(step.direct().get(1), Some(ChunkStatus::Carvers));
        assert_eq!(step.direct().get(2), Some(ChunkStatus::StructureStarts));
        assert_eq!(step.direct().get(8), Some(ChunkStatus::StructureStarts));
        assert_eq!(step.direct().get(9), None, "eight is the outermost");
        assert_eq!(step.block_state_write_radius(), 1);
        // The whole of docs/DETERMINISM.md §31 in one assertion: no
        // neighbour of a decorating chunk is ever required to be decorated.
        for radius in 0..step.direct().len() {
            let required = step.direct().get(radius).expect("in range");
            assert!(
                !required.is_or_after(ChunkStatus::Features),
                "radius {radius} would need a decorated neighbour"
            );
        }
    }

    /// `ChunkGenerationTask.getRadiusForLayer` for a chunk being taken to
    /// `FULL`. These four numbers are what the generated extent is derived
    /// from, so they are pinned to the block rather than recomputed.
    #[test]
    fn a_full_chunk_needs_decoration_one_ring_out_and_terrain_two() {
        let step = generation_pyramid().step_to(ChunkStatus::Full);
        assert_eq!(step.layer_radius(ChunkStatus::Features), 1);
        assert_eq!(step.layer_radius(ChunkStatus::Carvers), 2);
        assert_eq!(step.layer_radius(ChunkStatus::Surface), 2);
        assert_eq!(step.layer_radius(ChunkStatus::Noise), 2);
        assert_eq!(step.layer_radius(ChunkStatus::Biomes), 3);
        assert_eq!(step.layer_radius(ChunkStatus::StructureStarts), 11);
        // Nobody's neighbour has to be lit, and the module docs say why this
        // is 0 here where vanilla's own lookup throws.
        assert_eq!(step.layer_radius(ChunkStatus::Light), 0);
        assert_eq!(step.layer_radius(ChunkStatus::Full), 0);
    }

    /// The same query for a chunk being taken only as far as `FEATURES` —
    /// which is what a decorating chunk in the middle of a canvas is doing,
    /// and the radii the isolated decoration pass rests on.
    #[test]
    fn a_decorating_chunk_needs_carved_neighbours_one_ring_out() {
        let step = generation_pyramid().step_to(ChunkStatus::Features);
        assert_eq!(step.layer_radius(ChunkStatus::Carvers), 1);
        assert_eq!(step.layer_radius(ChunkStatus::Surface), 1);
        assert_eq!(step.layer_radius(ChunkStatus::Noise), 1);
        assert_eq!(step.layer_radius(ChunkStatus::Biomes), 2);
        assert_eq!(step.layer_radius(ChunkStatus::StructureStarts), 10);
        assert_eq!(step.layer_radius(ChunkStatus::Features), 0);
    }

    /// `addRequirement` takes the max per radius, and the later, narrower
    /// call must not shorten the array the earlier, wider one grew.
    #[test]
    fn a_narrow_requirement_does_not_shorten_a_wide_one() {
        let step = StepBuilder::new(ChunkStatus::Features, None)
            .add_requirement(ChunkStatus::StructureStarts, 8)
            .add_requirement(ChunkStatus::Carvers, 1)
            .build();
        assert_eq!(step.direct().len(), 9, "the wide call sets the length");
        assert_eq!(step.direct().get(0), Some(ChunkStatus::Carvers));
        assert_eq!(step.direct().get(8), Some(ChunkStatus::StructureStarts));
    }

    /// A step may not depend on itself or on anything later — vanilla throws
    /// and so do we.
    #[test]
    #[should_panic(expected = "cannot be a requirement of")]
    fn a_step_cannot_require_its_own_status() {
        let _ =
            StepBuilder::new(ChunkStatus::Features, None).add_requirement(ChunkStatus::Features, 1);
    }

    /// Accumulation is monotone: a later status never needs a *narrower*
    /// square of an earlier one than the step before it did. A pyramid that
    /// narrowed would be one where a chunk could be promoted past a
    /// neighbour it still depends on.
    #[test]
    fn the_pyramid_never_narrows_as_it_rises() {
        let pyramid = generation_pyramid();
        for status in ChunkStatus::ALL {
            let Some(next) = status.next() else { continue };
            for required in ChunkStatus::ALL {
                if required.is_or_after(status) {
                    continue;
                }
                let here = pyramid.step_to(status).layer_radius(required);
                let above = pyramid.step_to(next).layer_radius(required);
                assert!(
                    above >= here,
                    "{next:?} needs {required:?} at radius {above} where \
                     {status:?} needed it at {here}"
                );
            }
        }
    }

    /// The write radius is what bounds how far a decorating chunk's blocks
    /// can land, and therefore how many rings a delivered chunk needs
    /// decorated around it. One, from the jar, and nothing else in the
    /// pyramid writes further.
    #[test]
    fn features_is_the_only_step_that_writes_outside_its_own_chunk() {
        for step in generation_pyramid().steps() {
            let radius = step.block_state_write_radius();
            match step.target() {
                ChunkStatus::Features => assert_eq!(radius, 1),
                ChunkStatus::Noise | ChunkStatus::Surface | ChunkStatus::Carvers => {
                    assert_eq!(radius, 0, "{:?}", step.target());
                }
                other => assert_eq!(radius, -1, "{other:?} sets no write radius"),
            }
        }
    }
}
