//! Shared setup for the whole-chunk parity gates.
//!
//! Every gate that generates a real chunk needs the same five things: the
//! noise parameters, the overworld noise set, the climate table, the biome
//! data and the surface rule. They were duplicated across
//! `golden_surface_chunk` and `golden_carver_chunk` for one round, which is
//! exactly how two gates drift into measuring subtly different worlds and
//! stop being comparable.
//!
//! What is *not* here is the generation flow itself. The gates differ in what
//! they instrument — one counts condition reaches, the other carries a live
//! aquifer into the carvers — and folding those into one function would make
//! it a switch statement pretending to be shared code.
//!
//! The loaders themselves now live in the `worldgen-assembly` crate, because
//! `worldgen-dump` needs the same ones and two copies of a datapack reader is
//! how two pipelines drift apart. This module re-exports them and keeps only
//! what is genuinely test-shaped: the golden-vector decoders and the column
//! adapter the surface gates instrument.
//!
//! Integration tests compile this module once per binary, so items only one
//! gate uses look dead to the others; hence the blanket allow.

#![allow(dead_code)]

use rustmc_worldgen::surface::rule::BlockState;
use rustmc_worldgen::surface::system::BlockColumn;
use serde_json::Value;

pub use worldgen_assembly::*;

pub fn plain_name(name: &str) -> &str {
    name.split_once('[').map_or(name, |(n, _)| n)
}

/// Expand a run-length encoded section of a golden case into palette indices.
#[must_use]
pub fn expand_runs(palette_len: usize, runs: &Value) -> Vec<usize> {
    let mut out = Vec::new();
    for run in runs.as_array().expect("runs") {
        let run = run.as_array().expect("[index, length]");
        let value = usize::try_from(run[0].as_u64().expect("index")).expect("usize");
        let length = usize::try_from(run[1].as_u64().expect("length")).expect("usize");
        assert!(value < palette_len, "run index {value} outside the palette");
        out.extend(std::iter::repeat_n(value, length));
    }
    out
}

#[must_use]
pub fn palette_of<'a>(case: &'a Value, field: &str) -> Vec<&'a str> {
    case[field]
        .as_array()
        .unwrap_or_else(|| panic!("case has no `{field}`"))
        .iter()
        .map(|v| v.as_str().expect("block name"))
        .collect()
}

/// A chunk's blocks as names, which is what surface rules and carvers speak
/// in.
///
/// `Substance` has no vocabulary for grass or deepslate, so the noise stage's
/// output is widened here rather than the enum being grown to cover every
/// block a datapack might name.
pub struct NameColumn {
    pub min_y: i32,
    pub height: i32,
    pub blocks: Vec<String>,
    pub x: i32,
    pub z: i32,
}

impl NameColumn {
    #[must_use]
    pub fn index(&self, y: i32) -> Option<usize> {
        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }
        usize::try_from(((y - self.min_y) * 256) + (self.z & 15) * 16 + (self.x & 15)).ok()
    }
}

impl BlockColumn for NameColumn {
    fn get(&self, y: i32) -> Option<&str> {
        self.index(y).map(|i| self.blocks[i].as_str())
    }
    fn set(&mut self, y: i32, state: BlockState) {
        if let Some(i) = self.index(y) {
            self.blocks[i] = state.0;
        }
    }
    fn is_air(&self, state: &str) -> bool {
        state == "minecraft:air"
    }
    fn is_fluid(&self, state: &str) -> bool {
        state.starts_with("minecraft:water") || state.starts_with("minecraft:lava")
    }
}
