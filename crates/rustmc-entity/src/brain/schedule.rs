//! The villager schedules of 26.2, which are no longer a `Schedule` class.
//!
//! # Where the schedule went
//!
//! `Brain.schedule` is an `EnvironmentAttribute<Activity>` — one of
//! `EnvironmentAttributes.VILLAGER_ACTIVITY` (`gameplay/villager_activity`)
//! and `BABY_VILLAGER_ACTIVITY` (`gameplay/baby_villager_activity`), both
//! registered with `defaultValue(Activity.IDLE)` and the `ACTIVITY`
//! attribute type (`AttributeType.ofNotInterpolated(...)`, whose
//! `keyframeLerp` is `LerpFunction.ofStep(1.0f)`: `t < 1 ? from : to`).
//! `Brain.updateActivityFromSchedule` reads it through
//! `EnvironmentAttributeSystem.getValue(attribute, mob.position())`, and the
//! value comes from the **timeline data**
//! `data/minecraft/timeline/villager_schedule.json` (tagged into every
//! dimension through `#minecraft:universal`), a `period_ticks = 24000`
//! track of keyframes on the `minecraft:overworld` clock.
//!
//! The keyframes, verbatim from the jar's data (the golden at
//! `golden/villager_schedule.json` is the file itself; the test below
//! refuses if the constants drift from it):
//!
//! ```text
//! villager_activity:        10 idle   2000 work   9000 meet  11000 idle  12000 rest
//! baby_villager_activity:   10 idle   3000 play   6000 idle  10000 play  12000 rest
//! ```
//!
//! # Sampling (`KeyframeTrackSampler`, from `net.minecraft.util`)
//!
//! `bakeSegments` for a periodic track of keyframes `k0..kn`: a wrap
//! segment `(kn.ticks - period) -> k0.ticks`, then each adjacent pair, then
//! `kn.ticks -> (k0.ticks + period)`. `sample(t)`: `t = floorMod(t, period)`;
//! the first segment with `t < toTicks`, else the last; `t <= fromTicks`
//! answers `fromValue`, `t >= toTicks` answers `toValue`, and between them
//! the step lerp answers `fromValue`. Net effect: **the value of the last
//! keyframe at or before `t mod 24000`, wrapping to the last keyframe of
//! the previous day before the first** — so a villager is `rest` from
//! 12000 through 24000 and for the first 10 ticks of the next day.
//!
//! The clock's `getTotalTicks` is the level's day time, which the host
//! answers through [`MobHost::day_time`]; the environment attribute system
//! also layers positional overrides, which no vanilla datapack adds to this
//! attribute, so the host's default resolves the track alone.

use crate::brain::activity::Activity;
use crate::host::MobHost;

/// `period_ticks` of `villager_schedule.json`.
pub const PERIOD_TICKS: i64 = 24000;

/// The two schedule attributes a brain can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Schedule {
    /// `EnvironmentAttributes.VILLAGER_ACTIVITY`.
    VillagerDefault,
    /// `EnvironmentAttributes.BABY_VILLAGER_ACTIVITY`.
    VillagerBaby,
}

/// `gameplay/villager_activity`'s keyframes.
pub const VILLAGER_ACTIVITY: &[(i64, Activity)] = &[
    (10, Activity::Idle),
    (2000, Activity::Work),
    (9000, Activity::Meet),
    (11000, Activity::Idle),
    (12000, Activity::Rest),
];

/// `gameplay/baby_villager_activity`'s keyframes.
pub const BABY_VILLAGER_ACTIVITY: &[(i64, Activity)] = &[
    (10, Activity::Idle),
    (3000, Activity::Play),
    (6000, Activity::Idle),
    (10000, Activity::Play),
    (12000, Activity::Rest),
];

impl Schedule {
    /// The attribute's registry name.
    #[must_use]
    pub fn attribute_name(self) -> &'static str {
        match self {
            Schedule::VillagerDefault => "gameplay/villager_activity",
            Schedule::VillagerBaby => "gameplay/baby_villager_activity",
        }
    }

    /// The track's keyframes.
    #[must_use]
    pub fn keyframes(self) -> &'static [(i64, Activity)] {
        match self {
            Schedule::VillagerDefault => VILLAGER_ACTIVITY,
            Schedule::VillagerBaby => BABY_VILLAGER_ACTIVITY,
        }
    }

    /// `KeyframeTrackSampler.sample(totalTicks)` on this track with the
    /// step lerp.
    #[must_use]
    pub fn sample(self, total_ticks: i64) -> Activity {
        sample_step(self.keyframes(), PERIOD_TICKS, total_ticks)
    }

    /// `EnvironmentAttributeSystem.getValue(attribute, pos)` as the host's
    /// default answers it: the track alone, sampled at `day_time()`.
    #[must_use]
    pub fn resolve<M: MobHost + ?Sized>(self, mob: &M) -> Activity {
        self.sample(mob.day_time())
    }
}

/// `KeyframeTrackSampler` for a periodic track under `LerpFunction.ofStep(1.0)`.
#[must_use]
pub fn sample_step(keyframes: &[(i64, Activity)], period: i64, total_ticks: i64) -> Activity {
    assert!(!keyframes.is_empty(), "KeyframeTrack refuses an empty list");
    if keyframes.len() == 1 {
        return keyframes[0].1;
    }
    let first = keyframes[0];
    let last = keyframes[keyframes.len() - 1];
    // bakeSegments: (from, fromTicks, to, toTicks)
    let mut segments: Vec<(Activity, i64, Activity, i64)> = Vec::new();
    segments.push((last.1, last.0 - period, first.1, first.0));
    for w in keyframes.windows(2) {
        segments.push((w[0].1, w[0].0, w[1].1, w[1].0));
    }
    segments.push((last.1, last.0, first.1, first.0 + period));
    // sample: loopTicks, getSegmentAt, then the three-way test.
    let t = total_ticks.rem_euclid(period);
    let seg = segments
        .iter()
        .find(|s| t < s.3)
        .unwrap_or(&segments[segments.len() - 1]);
    if t <= seg.1 {
        seg.0
    } else if t >= seg.3 {
        seg.2
    } else {
        // ofStep(1.0): frac < 1 -> from. The easing is linear on a periodic
        // track and cannot lift frac to 1 inside (fromTicks, toTicks).
        seg.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_villager_day() {
        let s = Schedule::VillagerDefault;
        assert_eq!(
            s.sample(0),
            Activity::Rest,
            "0..9 wraps to the last keyframe"
        );
        assert_eq!(s.sample(9), Activity::Rest);
        assert_eq!(s.sample(10), Activity::Idle);
        assert_eq!(s.sample(1999), Activity::Idle);
        assert_eq!(s.sample(2000), Activity::Work);
        assert_eq!(s.sample(8999), Activity::Work);
        assert_eq!(s.sample(9000), Activity::Meet);
        assert_eq!(s.sample(11000), Activity::Idle);
        assert_eq!(s.sample(12000), Activity::Rest);
        assert_eq!(s.sample(23999), Activity::Rest);
        assert_eq!(
            s.sample(24000 + 2000),
            Activity::Work,
            "floorMod over the period"
        );
        assert_eq!(
            s.sample(-1),
            Activity::Rest,
            "negative day time floorMods too"
        );
    }

    #[test]
    fn a_baby_villager_day() {
        let s = Schedule::VillagerBaby;
        assert_eq!(s.sample(3000), Activity::Play);
        assert_eq!(s.sample(6000), Activity::Idle);
        assert_eq!(s.sample(10000), Activity::Play);
        assert_eq!(s.sample(12000), Activity::Rest);
        assert_eq!(s.sample(5), Activity::Rest);
    }

    #[test]
    fn single_keyframe_is_constant() {
        assert_eq!(
            sample_step(&[(500, Activity::Work)], 24000, 0),
            Activity::Work
        );
    }

    /// The constants above against the jar's own data file, so a data
    /// change in a later jar reds this rather than drifting silently.
    #[test]
    fn constants_match_the_jars_villager_schedule_json() {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/villager_schedule.json");
        let text = std::fs::read_to_string(p).expect("golden/villager_schedule.json");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(v["clock"], "minecraft:overworld");
        assert_eq!(v["period_ticks"], PERIOD_TICKS);
        for s in [Schedule::VillagerDefault, Schedule::VillagerBaby] {
            let track = &v["tracks"][format!("minecraft:{}", s.attribute_name())]["keyframes"];
            let got: Vec<(i64, String)> = track
                .as_array()
                .expect("keyframes")
                .iter()
                .map(|k| {
                    (
                        k["ticks"].as_i64().unwrap(),
                        k["value"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            let want: Vec<(i64, String)> = s
                .keyframes()
                .iter()
                .map(|(t, a)| (*t, format!("minecraft:{}", a.name())))
                .collect();
            assert_eq!(got, want, "{}", s.attribute_name());
        }
    }
}
