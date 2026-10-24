//! The world border — `net.minecraft.world.level.border.WorldBorder` and the
//! six clientbound packets that describe it, transcribed from 26.2 bytecode.
//!
//! # The shape vanilla gives it, and why it is copied rather than simplified
//!
//! A border is not "a size that changes over time". It is a *border* holding
//! an **extent**, and the extent is one of two objects:
//!
//! * `StaticBorderExtent` precomputes `minX/minZ/maxX/maxZ` in its
//!   constructor and returns itself from `update()` forever;
//! * `MovingBorderExtent` recomputes a size every tick and, on the tick its
//!   countdown reaches zero, **returns a `StaticBorderExtent` instead of
//!   itself** — which is how `WorldBorder.tick()` (a one-line
//!   `extent = extent.update()`) swaps the object out.
//!
//! Collapsing that into a size and an "is lerping" flag would be wrong in a
//! way the golden catches: on the last tick of a shrink the status flips to
//! `STATIONARY` *and* the size becomes exactly `to`, because the new static
//! extent is built from `to` rather than from one more step of the lerp.
//! `world_border.json`'s `shrink-t20` row is that tick.
//!
//! # The lerp is a countdown, not a clock
//!
//! `MovingBorderExtent` stores `lerpProgress` (initialised to the duration)
//! and decrements it in `update()`. `calculateSize()` is
//!
//! ```text
//!   d = (lerpDuration - lerpProgress) / lerpDuration
//!   d < 1.0 ? Mth.lerp(d, from, to) : to
//! ```
//!
//! so it never reads a world clock. `lerpBegin`/`lerpEnd` exist only for
//! `getLerpSpeed()`, which is `abs(from - to) / (lerpEnd - lerpBegin)` — a
//! per-tick rate, and the one place the begin time is used at all.
//!
//! # Two sizes, and the packets use both
//!
//! The moving extent keeps `size` and `previousSize` so a client can
//! interpolate within a tick: `getMinX(partialTick)` is
//! `clamp(centerX - lerp(partialTick, previousSize, size) / 2, -absMax,
//! absMax)`. The server-side accessors pass `0.0f`, so they read
//! `previousSize` — which is *last* tick's size, not this one's. That is not a
//! bug to fix on the way past; it is why `shrink-t10`'s `minX` does not equal
//! `centerX - size / 2`.
//!
//! # Evidence
//!
//! `crates/rustmc-server/golden/world_border.json` is vanilla's own output
//! over a scripted sequence of operations — thirteen states, and for each one
//! the bytes vanilla's own `STREAM_CODEC` produces for all six packets.
//! [`tests`] replays the script and demands both the scalars and the bytes.
//! `ci/border-golden.sh` regenerates it and refuses a capture that never left
//! the default state.

use std::io::{self, Read, Write};

use rustmc_proto::{
    varint::{read_varint, read_varlong, write_varint, write_varlong},
    wire,
};

/// `WorldBorder.MAX_SIZE`.
pub const MAX_SIZE: f64 = 5.9999968E7;

/// The one border this process serves, behind the lock every writer takes.
///
/// # Why process-wide, and why a `Mutex` rather than the atomics
/// [`crate::gamerules`] uses
///
/// Vanilla keeps a `WorldBorder` on `MinecraftServer` and gives every
/// `ServerLevel` a **view** of it: `MinecraftServer.<init>` builds one
/// `worldBorder` and `ServerLevel.getWorldBorder` returns it, so the overworld
/// and the nether share a single object and the nether's `WorldBorderExtent`
/// only rescales its coordinates. One cell here is therefore vanilla's shape
/// and not a simplification of it — see [`crate::dimension`] for the scale.
///
/// A `Mutex` rather than a field-per-atomic because a border mutation is not
/// one field. `set_size` replaces the whole [`Extent`], and
/// [`WorldBorder::tick`] reads the extent, decides whether it has expired and
/// writes a *different variant* back. A reader that caught that half-done
/// would see a moving extent whose `lerp_progress` had already reached zero,
/// which is a state vanilla never has.
pub fn shared() -> &'static std::sync::Mutex<WorldBorder> {
    static BORDER: std::sync::OnceLock<std::sync::Mutex<WorldBorder>> = std::sync::OnceLock::new();
    BORDER.get_or_init(|| std::sync::Mutex::new(WorldBorder::default()))
}

/// `WorldBorder.MAX_CENTER_COORDINATE`.
pub const MAX_CENTER_COORDINATE: f64 = 2.9999984E7;

/// The `absoluteMaxSize` a fresh `WorldBorder` starts with — `29999984`, an
/// `int`, and *not* `MAX_CENTER_COORDINATE` despite being numerically equal
/// to it. It is negated and widened to a double in every clamp, so the
/// integer type is what the arithmetic sees.
pub const DEFAULT_ABSOLUTE_MAX_SIZE: i32 = 29_999_984;

/// `Mth.clamp(double, double, double)`.
///
/// Transcribed rather than replaced by `f64::clamp`, which panics when
/// `min > max` and orders NaN differently. Vanilla's is
/// `v < min ? min : Math.min(v, max)`, and its comparison is `dcmpg`, so a NaN
/// `v` takes the `< min` branch and returns `min`.
#[must_use]
pub fn clamp(v: f64, min: f64, max: f64) -> f64 {
    if v < min || v.is_nan() {
        min
    } else if max.is_nan() {
        // `Math.min` propagates NaN; `f64::min` does not.
        f64::NAN
    } else {
        v.min(max)
    }
}

/// `Mth.lerp(double delta, double from, double to)` — `from + delta * (to -
/// from)`, in that association. Written out because `from * (1 - d) + to * d`
/// is a different double.
#[must_use]
pub fn lerp(delta: f64, from: f64, to: f64) -> f64 {
    from + delta * (to - from)
}

/// `net.minecraft.world.level.border.BorderStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStatus {
    Growing,
    Shrinking,
    Stationary,
}

impl BorderStatus {
    /// The name vanilla's enum constant carries, for the golden comparison.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Growing => "GROWING",
            Self::Shrinking => "SHRINKING",
            Self::Stationary => "STATIONARY",
        }
    }
}

/// `WorldBorder.BorderExtent`, as its two implementations.
#[derive(Debug, Clone, Copy)]
enum Extent {
    /// `StaticBorderExtent`. The box is precomputed in `updateBox()`, which is
    /// why the centre and the absolute max are captured here rather than read
    /// live: vanilla re-runs `updateBox()` from `onCenterChange()` and
    /// `onAbsoluteMaxSizeChange()`, and a static extent whose box was never
    /// rebuilt keeps the old numbers.
    Static {
        size: f64,
        min_x: f64,
        min_z: f64,
        max_x: f64,
        max_z: f64,
    },
    /// `MovingBorderExtent`.
    Moving {
        from: f64,
        to: f64,
        lerp_begin: i64,
        lerp_end: i64,
        lerp_duration: f64,
        lerp_progress: i64,
        size: f64,
        previous_size: f64,
    },
}

/// `net.minecraft.world.level.border.WorldBorder`.
#[derive(Debug, Clone)]
pub struct WorldBorder {
    center_x: f64,
    center_z: f64,
    damage_per_block: f64,
    safe_zone: f64,
    warning_time: i32,
    warning_blocks: i32,
    absolute_max_size: i32,
    extent: Extent,
}

impl Default for WorldBorder {
    /// `new WorldBorder()` — the field initialisers, **not**
    /// `Settings.DEFAULT`. The two disagree and it matters: `Settings.DEFAULT`
    /// carries `warningTime = 300`, the constructor writes `15`, and nothing
    /// applies the settings until `applyInitialSettings` runs. A fresh border
    /// on the wire therefore says 15.
    fn default() -> Self {
        let mut b = Self {
            center_x: 0.0,
            center_z: 0.0,
            damage_per_block: 0.2,
            safe_zone: 5.0,
            warning_time: 15,
            warning_blocks: 5,
            absolute_max_size: DEFAULT_ABSOLUTE_MAX_SIZE,
            // Placeholder; replaced immediately, because `StaticBorderExtent`'s
            // constructor calls `updateBox()`, which reads the border.
            extent: Extent::Static {
                size: MAX_SIZE,
                min_x: 0.0,
                min_z: 0.0,
                max_x: 0.0,
                max_z: 0.0,
            },
        };
        b.extent = b.static_extent(MAX_SIZE);
        b
    }
}

impl WorldBorder {
    /// `new StaticBorderExtent(size)` including its `updateBox()`.
    fn static_extent(&self, size: f64) -> Extent {
        let amax = f64::from(self.absolute_max_size);
        Extent::Static {
            size,
            min_x: clamp(self.center_x - size / 2.0, -amax, amax),
            min_z: clamp(self.center_z - size / 2.0, -amax, amax),
            max_x: clamp(self.center_x + size / 2.0, -amax, amax),
            max_z: clamp(self.center_z + size / 2.0, -amax, amax),
        }
    }

    /// `MovingBorderExtent.calculateSize()`.
    fn moving_size(lerp_duration: f64, lerp_progress: i64, from: f64, to: f64) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let d = (lerp_duration - lerp_progress as f64) / lerp_duration;
        if d < 1.0 {
            lerp(d, from, to)
        } else {
            to
        }
    }

    #[must_use]
    pub fn center_x(&self) -> f64 {
        self.center_x
    }
    #[must_use]
    pub fn center_z(&self) -> f64 {
        self.center_z
    }
    #[must_use]
    pub fn damage_per_block(&self) -> f64 {
        self.damage_per_block
    }
    #[must_use]
    pub fn safe_zone(&self) -> f64 {
        self.safe_zone
    }
    #[must_use]
    pub fn warning_time(&self) -> i32 {
        self.warning_time
    }
    #[must_use]
    pub fn warning_blocks(&self) -> i32 {
        self.warning_blocks
    }
    #[must_use]
    pub fn absolute_max_size(&self) -> i32 {
        self.absolute_max_size
    }

    /// `getSize()`.
    #[must_use]
    pub fn size(&self) -> f64 {
        match self.extent {
            Extent::Static { size, .. } | Extent::Moving { size, .. } => size,
        }
    }

    /// `getLerpTarget()` — the static extent answers with its own size.
    #[must_use]
    pub fn lerp_target(&self) -> f64 {
        match self.extent {
            Extent::Static { size, .. } => size,
            Extent::Moving { to, .. } => to,
        }
    }

    /// `getLerpTime()` — the remaining countdown, zero when static.
    #[must_use]
    pub fn lerp_time(&self) -> i64 {
        match self.extent {
            Extent::Static { .. } => 0,
            Extent::Moving { lerp_progress, .. } => lerp_progress,
        }
    }

    /// `getLerpSpeed()` — blocks per tick, zero when static.
    #[must_use]
    pub fn lerp_speed(&self) -> f64 {
        match self.extent {
            Extent::Static { .. } => 0.0,
            Extent::Moving {
                from,
                to,
                lerp_begin,
                lerp_end,
                ..
            } => {
                #[allow(clippy::cast_precision_loss)]
                let span = (lerp_end - lerp_begin) as f64;
                (from - to).abs() / span
            }
        }
    }

    /// `getStatus()`.
    #[must_use]
    pub fn status(&self) -> BorderStatus {
        match self.extent {
            Extent::Static { .. } => BorderStatus::Stationary,
            // `to < from` under `dcmpg`, so equal sizes are GROWING — though
            // `lerpSizeBetween` never builds a moving extent for equal sizes,
            // so that arm is unreachable from the public API.
            Extent::Moving { from, to, .. } => {
                if to < from {
                    BorderStatus::Shrinking
                } else {
                    BorderStatus::Growing
                }
            }
        }
    }

    /// The interpolated width a moving extent's four edge accessors share:
    /// `Mth.lerp(partialTick, previousSize, size)`, once per call.
    fn interpolated_size(&self, partial_tick: f32) -> f64 {
        match self.extent {
            Extent::Static { size, .. } => size,
            Extent::Moving {
                size,
                previous_size,
                ..
            } => lerp(f64::from(partial_tick), previous_size, size),
        }
    }

    /// `getMinX(partialTick)`. The no-argument server accessor passes `0.0`.
    #[must_use]
    pub fn min_x(&self, partial_tick: f32) -> f64 {
        match self.extent {
            Extent::Static { min_x, .. } => min_x,
            Extent::Moving { .. } => {
                let amax = f64::from(self.absolute_max_size);
                clamp(
                    self.center_x - self.interpolated_size(partial_tick) / 2.0,
                    -amax,
                    amax,
                )
            }
        }
    }

    /// `getMinZ(partialTick)`.
    #[must_use]
    pub fn min_z(&self, partial_tick: f32) -> f64 {
        match self.extent {
            Extent::Static { min_z, .. } => min_z,
            Extent::Moving { .. } => {
                let amax = f64::from(self.absolute_max_size);
                clamp(
                    self.center_z - self.interpolated_size(partial_tick) / 2.0,
                    -amax,
                    amax,
                )
            }
        }
    }

    /// `getMaxX(partialTick)`.
    #[must_use]
    pub fn max_x(&self, partial_tick: f32) -> f64 {
        match self.extent {
            Extent::Static { max_x, .. } => max_x,
            Extent::Moving { .. } => {
                let amax = f64::from(self.absolute_max_size);
                clamp(
                    self.center_x + self.interpolated_size(partial_tick) / 2.0,
                    -amax,
                    amax,
                )
            }
        }
    }

    /// `getMaxZ(partialTick)`.
    #[must_use]
    pub fn max_z(&self, partial_tick: f32) -> f64 {
        match self.extent {
            Extent::Static { max_z, .. } => max_z,
            Extent::Moving { .. } => {
                let amax = f64::from(self.absolute_max_size);
                clamp(
                    self.center_z + self.interpolated_size(partial_tick) / 2.0,
                    -amax,
                    amax,
                )
            }
        }
    }

    /// `getDistanceToBorder(double, double)`. Negative outside.
    #[must_use]
    pub fn distance_to_border(&self, x: f64, z: f64) -> f64 {
        let d_min_z = z - self.min_z(0.0);
        let d_max_z = self.max_z(0.0) - z;
        let d_min_x = x - self.min_x(0.0);
        let d_max_x = self.max_x(0.0) - x;
        // The exact fold vanilla uses: min(minX, maxX), then minZ, then maxZ.
        let d = d_min_x.min(d_max_x);
        let d = d.min(d_min_z);
        d.min(d_max_z)
    }

    /// `isWithinBounds(double, double)` — the two-argument form, which is
    /// `isWithinBounds(x, z, 0.0)`.
    #[must_use]
    pub fn is_within_bounds(&self, x: f64, z: f64) -> bool {
        self.is_within_bounds_slack(x, z, 0.0)
    }

    /// `isWithinBounds(double, double, double)`.
    #[must_use]
    pub fn is_within_bounds_slack(&self, x: f64, z: f64, slack: f64) -> bool {
        x >= self.min_x(0.0) - slack
            && x < self.max_x(0.0) + slack
            && z >= self.min_z(0.0) - slack
            && z < self.max_z(0.0) + slack
    }

    /// `setCenter(double, double)` — clamped to `MAX_CENTER_COORDINATE` and
    /// followed by `extent.onCenterChange()`, which rebuilds a static
    /// extent's box and does nothing to a moving one.
    pub fn set_center(&mut self, x: f64, z: f64) {
        self.center_x = clamp(x, -MAX_CENTER_COORDINATE, MAX_CENTER_COORDINATE);
        self.center_z = clamp(z, -MAX_CENTER_COORDINATE, MAX_CENTER_COORDINATE);
        self.on_center_change();
    }

    fn on_center_change(&mut self) {
        if let Extent::Static { size, .. } = self.extent {
            self.extent = self.static_extent(size);
        }
    }

    /// `setSize(double)`.
    pub fn set_size(&mut self, size: f64) {
        self.extent = self.static_extent(size);
    }

    /// `setAbsoluteMaxSize(int)` — followed by
    /// `extent.onAbsoluteMaxSizeChange()`, same asymmetry as the centre.
    pub fn set_absolute_max_size(&mut self, v: i32) {
        self.absolute_max_size = v;
        if let Extent::Static { size, .. } = self.extent {
            self.extent = self.static_extent(size);
        }
    }

    pub fn set_warning_blocks(&mut self, v: i32) {
        self.warning_blocks = v;
    }
    pub fn set_warning_time(&mut self, v: i32) {
        self.warning_time = v;
    }
    pub fn set_damage_per_block(&mut self, v: f64) {
        self.damage_per_block = v;
    }
    pub fn set_safe_zone(&mut self, v: f64) {
        self.safe_zone = v;
    }

    /// `lerpSizeBetween(from, to, duration, begin)`.
    ///
    /// Equal endpoints install a **static** extent, not a degenerate moving
    /// one — so the status is `STATIONARY` and no tick is needed to settle.
    pub fn lerp_size_between(&mut self, from: f64, to: f64, duration: i64, begin: i64) {
        self.extent = if from == to {
            self.static_extent(to)
        } else {
            #[allow(clippy::cast_precision_loss)]
            let lerp_duration = duration as f64;
            let size = Self::moving_size(lerp_duration, duration, from, to);
            Extent::Moving {
                from,
                to,
                lerp_begin: begin,
                lerp_end: begin + duration,
                lerp_duration,
                lerp_progress: duration,
                size,
                previous_size: size,
            }
        };
    }

    /// `tick()` — `extent = extent.update()`.
    pub fn tick(&mut self) {
        if let Extent::Moving {
            from,
            to,
            lerp_duration,
            ref mut lerp_progress,
            ref mut size,
            ref mut previous_size,
            ..
        } = self.extent
        {
            *lerp_progress -= 1;
            *previous_size = *size;
            *size = Self::moving_size(lerp_duration, *lerp_progress, from, to);
            if *lerp_progress <= 0 {
                self.extent = self.static_extent(to);
            }
        }
    }
}

/// The six clientbound packets, each named for its registry id.
///
/// Every one of them is written by the packet class itself rather than by a
/// composite codec, so the field widths below are `FriendlyByteBuf` calls read
/// straight off `write(FriendlyByteBuf)`.
pub mod packets {
    use super::{
        io, read_varint, read_varlong, wire, write_varint, write_varlong, Read, WorldBorder, Write,
    };

    /// `minecraft:initialize_border` — 43.
    ///
    /// Eight fields: four doubles, a **varlong**, then three varints. The
    /// varlong is the one that bites — `lerpTime` is a `long` written with
    /// `writeVarLong`, and a 32-bit varint there decodes every long lerp as
    /// the wrong number without ever failing to parse.
    #[derive(Debug, Clone, PartialEq)]
    pub struct InitializeBorder {
        pub new_center_x: f64,
        pub new_center_z: f64,
        pub old_size: f64,
        pub new_size: f64,
        pub lerp_time: i64,
        pub new_absolute_max_size: i32,
        pub warning_blocks: i32,
        pub warning_time: i32,
    }

    impl InitializeBorder {
        /// `new ClientboundInitializeBorderPacket(WorldBorder)`.
        #[must_use]
        pub fn of(b: &WorldBorder) -> Self {
            Self {
                new_center_x: b.center_x(),
                new_center_z: b.center_z(),
                old_size: b.size(),
                new_size: b.lerp_target(),
                lerp_time: b.lerp_time(),
                new_absolute_max_size: b.absolute_max_size(),
                warning_blocks: b.warning_blocks(),
                warning_time: b.warning_time(),
            }
        }

        /// # Errors
        /// If the sink refuses a write.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            wire::write_f64(out, self.new_center_x)?;
            wire::write_f64(out, self.new_center_z)?;
            wire::write_f64(out, self.old_size)?;
            wire::write_f64(out, self.new_size)?;
            write_varlong(out, self.lerp_time)?;
            write_varint(out, self.new_absolute_max_size)?;
            write_varint(out, self.warning_blocks)?;
            write_varint(out, self.warning_time)
        }

        /// # Errors
        /// If the stream ends early.
        pub fn read(r: &mut impl Read) -> io::Result<Self> {
            Ok(Self {
                new_center_x: wire::read_f64(r)?,
                new_center_z: wire::read_f64(r)?,
                old_size: wire::read_f64(r)?,
                new_size: wire::read_f64(r)?,
                lerp_time: read_varlong(r)?,
                new_absolute_max_size: read_varint(r)?,
                warning_blocks: read_varint(r)?,
                warning_time: read_varint(r)?,
            })
        }
    }

    /// `minecraft:set_border_center` — 88.
    #[derive(Debug, Clone, PartialEq)]
    pub struct SetBorderCenter {
        pub new_center_x: f64,
        pub new_center_z: f64,
    }

    impl SetBorderCenter {
        #[must_use]
        pub fn of(b: &WorldBorder) -> Self {
            Self {
                new_center_x: b.center_x(),
                new_center_z: b.center_z(),
            }
        }
        /// # Errors
        /// If the sink refuses a write.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            wire::write_f64(out, self.new_center_x)?;
            wire::write_f64(out, self.new_center_z)
        }
        /// # Errors
        /// If the stream ends early.
        pub fn read(r: &mut impl Read) -> io::Result<Self> {
            Ok(Self {
                new_center_x: wire::read_f64(r)?,
                new_center_z: wire::read_f64(r)?,
            })
        }
    }

    /// `minecraft:set_border_lerp_size` — 89.
    #[derive(Debug, Clone, PartialEq)]
    pub struct SetBorderLerpSize {
        pub old_size: f64,
        pub new_size: f64,
        pub lerp_time: i64,
    }

    impl SetBorderLerpSize {
        #[must_use]
        pub fn of(b: &WorldBorder) -> Self {
            Self {
                old_size: b.size(),
                new_size: b.lerp_target(),
                lerp_time: b.lerp_time(),
            }
        }
        /// # Errors
        /// If the sink refuses a write.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            wire::write_f64(out, self.old_size)?;
            wire::write_f64(out, self.new_size)?;
            write_varlong(out, self.lerp_time)
        }
        /// # Errors
        /// If the stream ends early.
        pub fn read(r: &mut impl Read) -> io::Result<Self> {
            Ok(Self {
                old_size: wire::read_f64(r)?,
                new_size: wire::read_f64(r)?,
                lerp_time: read_varlong(r)?,
            })
        }
    }

    /// `minecraft:set_border_size` — 90.
    ///
    /// Built from `getLerpTarget()`, not `getSize()`. On a static border those
    /// are the same number, which is exactly why a capture that never lerps
    /// cannot tell the two apart.
    #[derive(Debug, Clone, PartialEq)]
    pub struct SetBorderSize {
        pub size: f64,
    }

    impl SetBorderSize {
        #[must_use]
        pub fn of(b: &WorldBorder) -> Self {
            Self {
                size: b.lerp_target(),
            }
        }
        /// # Errors
        /// If the sink refuses a write.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            wire::write_f64(out, self.size)
        }
        /// # Errors
        /// If the stream ends early.
        pub fn read(r: &mut impl Read) -> io::Result<Self> {
            Ok(Self {
                size: wire::read_f64(r)?,
            })
        }
    }

    /// `minecraft:set_border_warning_delay` — 91. Carries `warningTime`.
    #[derive(Debug, Clone, PartialEq)]
    pub struct SetBorderWarningDelay {
        pub warning_delay: i32,
    }

    impl SetBorderWarningDelay {
        #[must_use]
        pub fn of(b: &WorldBorder) -> Self {
            Self {
                warning_delay: b.warning_time(),
            }
        }
        /// # Errors
        /// If the sink refuses a write.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            write_varint(out, self.warning_delay)
        }
        /// # Errors
        /// If the stream ends early.
        pub fn read(r: &mut impl Read) -> io::Result<Self> {
            Ok(Self {
                warning_delay: read_varint(r)?,
            })
        }
    }

    /// `minecraft:set_border_warning_distance` — 92. Carries `warningBlocks`.
    #[derive(Debug, Clone, PartialEq)]
    pub struct SetBorderWarningDistance {
        pub warning_blocks: i32,
    }

    impl SetBorderWarningDistance {
        #[must_use]
        pub fn of(b: &WorldBorder) -> Self {
            Self {
                warning_blocks: b.warning_blocks(),
            }
        }
        /// # Errors
        /// If the sink refuses a write.
        pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
            write_varint(out, self.warning_blocks)
        }
        /// # Errors
        /// If the stream ends early.
        pub fn read(r: &mut impl Read) -> io::Result<Self> {
            Ok(Self {
                warning_blocks: read_varint(r)?,
            })
        }
    }
}
