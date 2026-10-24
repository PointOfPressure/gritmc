//! A whole-process occupancy trace: how many cores were busy, when.
//!
//! # Why the stage table is not enough
//!
//! `RUSTMC_STAGE_CPU` prints one `cores` figure per stage, which is a *mean*
//! over that stage. A mean cannot separate the two shapes that produce it:
//! twelve cores for half the stage and one for the other half reads the same
//! as six and a half cores throughout, and only the first has an idle gap to
//! find. It also stops at the last `times.mark`, so everything the dump does
//! **after** the generator returns — building 10,201 chunk columns and
//! compressing them into region files — is outside every stage and outside
//! every number the table prints.
//!
//! This samples the thread group's own CPU counter at a fixed interval and
//! reports instantaneous occupancy, so a gap is *located in time* rather than
//! averaged away, and the trace runs for as long as the process does.
//!
//! # What it costs, and why that is not a measurement hazard
//!
//! One thread, asleep except for a `read` of `/proc/self/stat` every
//! interval. At the 100 ms default over a 100 s run that is a thousand reads
//! of a ~350-byte pseudo-file: measured below the run-to-run noise of the
//! thing it measures. It is off unless `RUSTMC_OCCUPANCY_MS` is set, so a
//! benchmark that does not ask for it pays nothing at all, not even the
//! thread.
//!
//! # The check that can turn this red
//!
//! An occupancy trace is exactly the kind of instrument this project's
//! standing rule is about: it reports a number nobody can independently see,
//! and a broken sampler reports *plausible* numbers rather than obviously
//! wrong ones. Two things make it falsifiable:
//!
//! * the trace prints its own **total CPU-seconds and wall**, which
//!   `/usr/bin/time -v` reports independently for the same process. If the
//!   two disagree the sampler is wrong, and the comparison costs nothing;
//! * `occupancy_tells_a_known_load_apart_from_none` in this module spins
//!   four threads until each has been on a CPU for 150 ms by its own
//!   per-thread clock, and asserts the sampler's delta is at least their sum.
//!   `the_known_load_check_refuses_a_blind_sampler` runs the same check on a
//!   calling-thread-only sampler and a constant one and asserts both are
//!   refused.

/// A running occupancy trace. Printing happens on drop, so a caller cannot
/// forget to stop it and a trace covers the whole scope it was created in.
pub struct Occupancy {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<Vec<(f64, f64)>>>,
    label: &'static str,
}

impl Occupancy {
    /// Start sampling if `RUSTMC_OCCUPANCY_MS` names an interval, otherwise
    /// return a trace that does nothing and prints nothing.
    ///
    /// `RUSTMC_OCCUPANCY_MS=0` is treated as unset rather than as a
    /// zero-interval spin: a typo that pinned a core would be a measurement
    /// hazard in an instrument whose whole job is to measure core occupancy.
    #[must_use]
    pub fn start(label: &'static str) -> Self {
        let interval = std::env::var("RUSTMC_OCCUPANCY_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|ms| *ms > 0);
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handle = interval.map(|ms| {
            let stop = std::sync::Arc::clone(&stop);
            let period = std::time::Duration::from_millis(ms);
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let mut samples: Vec<(f64, f64)> = Vec::new();
                if let Some(cpu) = super::generate::process_cpu_seconds() {
                    samples.push((0.0, cpu));
                }
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(period);
                    if let Some(cpu) = super::generate::process_cpu_seconds() {
                        samples.push((started.elapsed().as_secs_f64(), cpu));
                    }
                }
                if let Some(cpu) = super::generate::process_cpu_seconds() {
                    samples.push((started.elapsed().as_secs_f64(), cpu));
                }
                samples
            })
        });
        Self {
            stop,
            handle,
            label,
        }
    }
}

/// Turn cumulative `(wall, cpu)` samples into instantaneous occupancy.
///
/// A free function so the report can be tested without a thread: the
/// arithmetic is the part that can be wrong.
#[must_use]
pub fn occupancy_series(samples: &[(f64, f64)]) -> Vec<(f64, f64)> {
    samples
        .windows(2)
        .filter_map(|w| {
            let (t0, c0) = w[0];
            let (t1, c1) = w[1];
            let dt = t1 - t0;
            (dt > 0.0).then(|| (t1, (c1 - c0) / dt))
        })
        .collect()
}

/// How much wall time was spent at each occupancy band, and the total.
///
/// The bands are the answer to "where did the machine idle", which a mean
/// cannot give. Bounds are chosen against this box rather than as round
/// numbers: `<1.5` is a stage that is sequential in all but name, `1.5..4`
/// is one whose parallelism is not paying, and `>=8` is one using the
/// machine.
#[must_use]
pub fn bands(series: &[(f64, f64)], step: f64) -> Vec<(&'static str, f64)> {
    let mut out = vec![
        ("< 1.5", 0.0),
        ("1.5 - 4", 0.0),
        ("4 - 8", 0.0),
        (">= 8", 0.0),
    ];
    for (_, cores) in series {
        let slot = if *cores < 1.5 {
            0
        } else if *cores < 4.0 {
            1
        } else if *cores < 8.0 {
            2
        } else {
            3
        };
        out[slot].1 += step;
    }
    out
}

impl Drop for Occupancy {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let Some(handle) = self.handle.take() else {
            return;
        };
        let Ok(samples) = handle.join() else {
            eprintln!("occupancy: the sampler thread died, so there is no trace");
            return;
        };
        if samples.len() < 2 {
            eprintln!("occupancy: fewer than two samples, so there is nothing to report");
            return;
        }
        let series = occupancy_series(&samples);
        let wall = samples.last().map_or(0.0, |(t, _)| *t);
        let cpu = samples.last().map_or(0.0, |(_, c)| *c) - samples[0].1;
        let step = if series.is_empty() {
            0.0
        } else {
            wall / series.len() as f64
        };
        eprintln!(
            "occupancy [{}] {:.2} s wall, {:.2} CPU-s, {:.2} avg cores, {} samples",
            self.label,
            wall,
            cpu,
            if wall > 0.0 { cpu / wall } else { 0.0 },
            series.len(),
        );
        eprintln!("occupancy  band       wall s   share");
        for (name, secs) in bands(&series, step) {
            eprintln!(
                "occupancy  {name:<9} {secs:8.2}  {:5.1}%",
                if wall > 0.0 { secs / wall * 100.0 } else { 0.0 },
            );
        }
        // The timeline itself, coarsened to one line a second, because the
        // shape is the finding and a thousand rows is not readable. A caller
        // that wants the raw samples asks for a 1000 ms interval and gets
        // them one to a bucket.
        eprintln!("occupancy  timeline (one bucket a second, cores busy)");
        let mut bucket = 0usize;
        let mut acc = 0.0f64;
        let mut n = 0usize;
        for (t, cores) in &series {
            let this = *t as usize;
            if this != bucket && n > 0 {
                eprintln!(
                    "occupancy  t={bucket:>5}s  {:5.2}  {}",
                    acc / n as f64,
                    "#".repeat(((acc / n as f64) + 0.5) as usize),
                );
                acc = 0.0;
                n = 0;
            }
            bucket = this;
            acc += cores;
            n += 1;
        }
        if n > 0 {
            eprintln!(
                "occupancy  t={bucket:>5}s  {:5.2}  {}",
                acc / n as f64,
                "#".repeat(((acc / n as f64) + 0.5) as usize),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arithmetic, against a series whose answer is known by hand: two
    /// seconds of wall in which four CPU-seconds were spent is two cores.
    #[test]
    fn occupancy_is_cpu_seconds_per_wall_second() {
        let series = occupancy_series(&[(0.0, 0.0), (1.0, 1.0), (2.0, 5.0)]);
        assert_eq!(series.len(), 2);
        assert!((series[0].1 - 1.0).abs() < 1e-9, "{series:?}");
        assert!((series[1].1 - 4.0).abs() < 1e-9, "{series:?}");
    }

    /// A sample pair with no time between them is dropped rather than
    /// dividing by zero — which would print `inf` cores and read as a wildly
    /// parallel stage.
    #[test]
    fn a_zero_length_interval_is_dropped_rather_than_divided_by() {
        let series = occupancy_series(&[(1.0, 1.0), (1.0, 2.0)]);
        assert!(series.is_empty(), "{series:?}");
    }

    /// The bands add up to the wall time they were given, so a share cannot
    /// silently go missing.
    #[test]
    fn the_bands_partition_the_wall_time() {
        let series = vec![(1.0, 0.5), (2.0, 2.0), (3.0, 6.0), (4.0, 12.0)];
        let total: f64 = bands(&series, 0.25).iter().map(|(_, s)| s).sum();
        assert!((total - 1.0).abs() < 1e-9, "{total}");
        let banded = bands(&series, 0.25);
        assert!((banded[0].1 - 0.25).abs() < 1e-9, "{banded:?}");
        assert!((banded[3].1 - 0.25).abs() < 1e-9, "{banded:?}");
    }

    /// On-CPU nanoseconds of the calling thread, from the kernel's own
    /// scheduler accounting (`/proc/thread-self/schedstat`, first field).
    fn thread_cpu_seconds() -> f64 {
        let s = std::fs::read_to_string("/proc/thread-self/schedstat")
            .expect("/proc/thread-self/schedstat (CONFIG_SCHED_INFO)");
        let ns: u64 = s
            .split_whitespace()
            .next()
            .and_then(|f| f.parse().ok())
            .expect("schedstat's first field is on-CPU ns");
        ns as f64 / 1e9
    }

    /// Spin four threads until each has been **on a CPU** for `PER_THREAD`
    /// by its own clock, and return what `sampler` saw across that span
    /// alongside the sum of the four threads' own clocks.
    ///
    /// Each spinner's CPU time is part of the thread group's, so a sampler
    /// that reads the thread group sees *at least* their sum. Whatever else
    /// the process did meanwhile (sibling tests, `map_indexed`'s workers)
    /// only adds to it, and a loaded box only stretches the wall time, never
    /// the CPU time the spinners are required to accumulate.
    fn spin_known_load(sampler: impl Fn() -> f64) -> (f64, f64) {
        const PER_THREAD: f64 = 0.15;
        const WALL_CAP: std::time::Duration = std::time::Duration::from_secs(30);
        let before = sampler();
        let spun: f64 = std::thread::scope(|scope| {
            let spinners: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let start = thread_cpu_seconds();
                        let cap = std::time::Instant::now() + WALL_CAP;
                        let mut x = 0u64;
                        while thread_cpu_seconds() - start < PER_THREAD
                            && std::time::Instant::now() < cap
                        {
                            for _ in 0..10_000 {
                                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                            }
                        }
                        std::hint::black_box(x);
                        thread_cpu_seconds() - start
                    })
                })
                .collect();
            spinners.into_iter().map(|h| h.join().unwrap()).sum()
        });
        (sampler() - before, spun)
    }

    /// The sampler's delta against the spinners' own clocks. `/proc/self/stat`
    /// counts in 10 ms ticks, two reads, so 0.05 s covers its granularity.
    fn sees_the_thread_group(seen: f64, spun: f64) -> Result<(), String> {
        const TOLERANCE: f64 = 0.05;
        if spun < 0.4 {
            return Err(format!(
                "the spinners only ran {spun:.3} CPU-s; no load was applied"
            ));
        }
        if seen + TOLERANCE < spun {
            return Err(format!(
                "four spinners ran {spun:.3} CPU-s by their own clocks and the \
                 sampler saw {seen:.3} — it is not seeing the thread group"
            ));
        }
        Ok(())
    }

    /// **The falsification.** A known load, and the trace has to see it.
    ///
    /// The naive form — spin four threads, assert about four cores — fails
    /// in `cargo test`, because the counter is the **thread group's** (the
    /// property that makes it right for a worker pool) and sibling tests run
    /// in the same process. The form after that — busy window minus an equal
    /// idle window, > 1.5 CPU-s — went red on a loaded box (gate
    /// `gate-instruments` 2026-09-24, 3.43 busy against 2.46 idle): wall
    /// windows let a loaded box starve the spinners and let siblings inflate
    /// "idle". So the load is now defined in CPU time by the spinners' own
    /// per-thread clocks, and the assertion is one-sided: the sampler's delta
    /// must be at least their sum. Contamination can only raise it.
    #[test]
    fn occupancy_tells_a_known_load_apart_from_none() {
        let (seen, spun) =
            spin_known_load(|| crate::generate::process_cpu_seconds().expect("a Linux box"));
        sees_the_thread_group(seen, spun).unwrap();
    }

    /// The same check refuses samplers that are wrong: one reading only the
    /// calling thread (the defect a thread-group instrument exists to avoid),
    /// and one returning a constant.
    #[test]
    fn the_known_load_check_refuses_a_blind_sampler() {
        let (seen, spun) = spin_known_load(thread_cpu_seconds);
        assert!(
            sees_the_thread_group(seen, spun).is_err(),
            "a calling-thread-only sampler saw {seen:.3} of {spun:.3} and was accepted"
        );
        let (seen, spun) = spin_known_load(|| 7.0);
        assert!(
            sees_the_thread_group(seen, spun).is_err(),
            "a constant sampler was accepted"
        );
    }
}
