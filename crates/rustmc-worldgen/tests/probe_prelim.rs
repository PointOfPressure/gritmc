//! Our `preliminary_surface_level` at the four cell corners the
//! `above_preliminary_surface` blend reads, for the failing packed_ice
//! column. Vanilla's value at (0, -2160) is 24.

mod common;
use common::{noise_parameters, overworld_noises};
use rustmc_worldgen::density::flat_cache::FlatCaches;
use rustmc_worldgen::density::router::OverworldRouter;
use rustmc_worldgen::density::surface_level::SurfaceLevels;

#[test]
fn corner_values_for_the_failing_column() {
    let noises = overworld_noises(&noise_parameters());
    let router = OverworldRouter::new(31_415_926, &noises, false);
    let mut levels = SurfaceLevels::new(router.preliminary_surface_level.clone());
    let flat = FlatCaches::none();
    for (x, z) in [(0, -2160), (16, -2160), (0, -2144), (16, -2144), (4, -2160)] {
        println!("  ({x}, {z}) -> {}", levels.at(&flat, x, z));
    }
}
