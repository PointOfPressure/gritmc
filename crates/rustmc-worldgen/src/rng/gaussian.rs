//! Marsaglia polar Gaussian.
//!
//! Reference: `net.minecraft.world.level.levelgen.MarsagliaPolarGaussian`.
//!
//! # Parity status: the structure is proved, the last bit is characterised
//!
//! `golden_gaussian` now exists and pins everything that decides how many
//! numbers come off the stream: the rejection loop, the cached second half of
//! each pair, `setSeed` discarding it, and the stream position after a run of
//! 256. All of that is **exact** — those are the properties worldgen parity
//! actually depends on.
//!
//! What is not exact is the last bit of the value, and the reason is a
//! measured fact about the JVM rather than a gap in this port. `Math.log` is
//! a HotSpot intrinsic: deterministic within a run, within 1 ulp of the true
//! logarithm, and **not correctly rounded**. It disagrees with
//! `StrictMath.log` on 7.3% of inputs in `(0, 1)`, so "port fdlibm" is not
//! the answer either — the two are different functions and neither is the
//! true one.
//!
//! [`crate::precise_log::ln`] is used here instead: correctly rounded, which
//! is a target that does not depend on which JVM or architecture produced a
//! golden file. It agrees with `Math.log` on 299,996 of 300,000 measured
//! inputs, against `f64::ln`'s 299,769, and on the four it does not, an
//! 80-digit reference says the JVM is the one that is wrong. The residual is
//! counted in the gate, not hidden in a tolerance. See ORACLE.md §5c.
//!
//! The residual cannot desynchronise anything: the rejection loop draws only
//! `nextDouble` and never touches the logarithm, so a 1-ulp difference
//! changes a value and never a draw count.
//!
//! `sqrt` is safe — IEEE-754 specifies it exactly.

/// Implements `RandomSource::next_gaussian` for a type carrying
/// `next_next_gaussian: f64` and `have_next_gaussian: bool` fields.
///
/// A macro rather than a shared function because the cached state lives *on*
/// the source and the rejection loop needs `&mut self` for `next_f64` at the
/// same time — splitting it would mean either a borrow dance or a trait
/// object on the hottest path in the crate.
///
/// The polar method yields two normal deviates per accepted sample; vanilla
/// returns one and banks the other. That makes the *number of underlying
/// draws* depend on call parity, so clearing the cache in `set_seed` (which
/// every implementor must do) is observable behavior, not hygiene.
macro_rules! marsaglia_next_gaussian {
    () => {
        fn next_gaussian(&mut self) -> f64 {
            // One call, however many `next_f64` pairs the rejection loop
            // below spends: vanilla's `Counting` wrapper never sees them
            // because its gaussian holds the inner source.
            let _composite = $crate::rng::draws::composite();
            if self.have_next_gaussian {
                self.have_next_gaussian = false;
                $crate::rng::draws::log_gaussian(self.next_next_gaussian);
                return self.next_next_gaussian;
            }

            // Reject outside the unit disc, and at the origin (log(0)).
            let (x, y, radius_squared) = loop {
                let x = 2.0 * self.next_f64() - 1.0;
                let y = 2.0 * self.next_f64() - 1.0;
                let radius_squared = x * x + y * y;
                if radius_squared < 1.0 && radius_squared != 0.0 {
                    break (x, y, radius_squared);
                }
            };

            // `crate::precise_log::ln`, not `f64::ln`: see the module docs
            // for what the JVM actually computes here and why being *more*
            // accurate is the closest reachable target.
            let multiplier =
                (-2.0 * $crate::precise_log::ln(radius_squared) / radius_squared).sqrt();
            self.next_next_gaussian = y * multiplier;
            self.have_next_gaussian = true;
            $crate::rng::draws::log_gaussian(x * multiplier);
            x * multiplier
        }
    };
}

pub(crate) use marsaglia_next_gaussian;
