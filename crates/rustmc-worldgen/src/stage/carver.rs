//! The carver stage — caves and canyons, onto the canvas.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `NoiseBasedChunkGenerator#applyCarvers`.
//!
//! [`crate::carver::driver::apply_carvers`] is the 17x17 sweep and it is
//! gated against carved chunks. This adapts a [`Canvas`] to the
//! [`CarveTarget`] a carver writes through.
//!
//! # A carver reads eight chunks away and writes only its own
//!
//! The sweep visits every source chunk within eight and lets each one's
//! carvers *reach* into the chunk being carved — so the neighbours must
//! already be noise-filled, which is why the pipeline is stage-major. What a
//! carver never does is write outside the chunk it was called for; the write
//! mask in `CarveBounds` is what enforces that, and the canvas would happily
//! accept the write if it did not.
//!
//! # `top_material` is the surface rules again, on an exposed face
//!
//! When a carver cuts into the ground it asks the surface system what the
//! newly exposed block should be — that is how a cave mouth in a desert gets
//! sand rather than a stone lip. It is supplied as a closure rather than run
//! here because it needs the rule tree and the oracle, which the caller
//! already built for the surface stage.

use crate::canvas::Canvas;
use crate::carver::carve::CarveTarget;

/// The three things a carve target does to blocks.
///
/// A trait so the same target works over the whole [`Canvas`] and over a
/// single [`crate::canvas::ChunkWriter`]. That is not a refactor for its own
/// sake: the carve loop clamps its `x` and `z` indices to `0..=15` of the
/// chunk named in [`crate::carver::carve::CarveBounds`], and the only read
/// outside that loop is `carve_block`'s look at the block one below — the
/// same column. So a carver touches nothing but its own chunk, and a writer
/// is a target that makes the compiler agree.
pub trait Blocks {
    fn get(&self, x: i32, y: i32, z: i32) -> String;
    fn set(&mut self, x: i32, y: i32, z: i32, state: &str);
    fn mark(&mut self, x: i32, y: i32, z: i32);
}

impl Blocks for Canvas {
    fn get(&self, x: i32, y: i32, z: i32) -> String {
        self.block_at(x, y, z).to_string()
    }
    fn set(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.set_block(x, y, z, state);
    }
    fn mark(&mut self, x: i32, y: i32, z: i32) {
        self.mark_post_processing(x, y, z);
    }
}

impl Blocks for crate::canvas::ChunkWriter<'_> {
    fn get(&self, x: i32, y: i32, z: i32) -> String {
        self.block_at(x, y, z).to_string()
    }
    fn set(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.set_block(x, y, z, state);
    }
    fn mark(&mut self, x: i32, y: i32, z: i32) {
        self.mark_post_processing(x, y, z);
    }
}

/// A canvas presented as something carvers can cut.
pub struct CanvasTarget<'a, B: Blocks = Canvas> {
    canvas: &'a mut B,
    /// `Aquifer#computeSubstance(pos, 0.0)`, which decides what fills a hole
    /// and can decline. The second half of the pair is the aquifer's
    /// `shouldScheduleFluidUpdate` flag as that call left it.
    aquifer: &'a mut dyn FnMut(i32, i32, i32) -> (Option<String>, bool),
    /// The flag from the most recent aquifer answer — or, before the first
    /// one, whatever the chunk's fill stage left on the same aquifer object.
    /// It is carried rather than re-derived because `getCarveState`'s lava
    /// branch reads it without refreshing it.
    should_schedule_fluid_update: bool,
    /// `CarvingContext#topMaterial`.
    top_material: &'a mut dyn FnMut(i32, i32, i32, bool) -> Option<String>,
}

impl<'a, B: Blocks> CanvasTarget<'a, B> {
    pub fn new(
        canvas: &'a mut B,
        aquifer: &'a mut dyn FnMut(i32, i32, i32) -> (Option<String>, bool),
        should_schedule_fluid_update: bool,
        top_material: &'a mut dyn FnMut(i32, i32, i32, bool) -> Option<String>,
    ) -> Self {
        Self {
            canvas,
            aquifer,
            should_schedule_fluid_update,
            top_material,
        }
    }
}

impl<B: Blocks> CarveTarget for CanvasTarget<'_, B> {
    fn block(&self, x: i32, y: i32, z: i32) -> String {
        self.canvas.get(x, y, z)
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        self.canvas.set(x, y, z, name);
    }

    fn aquifer_substance(&mut self, x: i32, y: i32, z: i32) -> Option<String> {
        let (substance, should_schedule) = (self.aquifer)(x, y, z);
        self.should_schedule_fluid_update = should_schedule;
        substance
    }

    fn top_material(&mut self, x: i32, y: i32, z: i32, under_fluid: bool) -> Option<String> {
        (self.top_material)(x, y, z, under_fluid)
    }

    fn should_schedule_fluid_update(&self) -> bool {
        self.should_schedule_fluid_update
    }

    fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
        self.canvas.mark(x, y, z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{Area, Pipeline};

    #[test]
    fn a_carver_reads_and_writes_through_the_canvas() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        pipeline.canvas_mut().set_block(4, 40, 4, "minecraft:stone");

        let mut aquifer = |_: i32, _: i32, _: i32| (Some("minecraft:cave_air".to_string()), false);
        let mut top = |_: i32, _: i32, _: i32, _: bool| None;
        {
            let mut target =
                CanvasTarget::new(pipeline.canvas_mut(), &mut aquifer, false, &mut top);
            assert_eq!(target.block(4, 40, 4), "minecraft:stone");
            let filler = target.aquifer_substance(4, 40, 4).expect("some");
            target.set_block(4, 40, 4, &filler);
        }
        assert_eq!(pipeline.canvas().block_at(4, 40, 4), "minecraft:cave_air");
    }

    /// A declining aquifer leaves the block alone — that is how caves stop
    /// below the local water table rather than flooding.
    #[test]
    fn a_declining_aquifer_is_not_an_error() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        pipeline.canvas_mut().set_block(0, 0, 0, "minecraft:stone");
        let mut aquifer = |_: i32, _: i32, _: i32| (None, false);
        let mut top = |_: i32, _: i32, _: i32, _: bool| None;
        let mut target = CanvasTarget::new(pipeline.canvas_mut(), &mut aquifer, false, &mut top);
        assert_eq!(target.aquifer_substance(0, 0, 0), None);
        assert_eq!(target.block(0, 0, 0), "minecraft:stone");
    }
}
