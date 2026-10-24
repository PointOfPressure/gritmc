//! `net.minecraft.world.entity.schedule.Activity` and
//! `net.minecraft.world.entity.ai.ActivityData` from the 26.2 jar.
//!
//! # `Activity.hashCode()` is `name.hashCode()`, and the brain keys a `HashMap` on it
//!
//! `Activity` (offsets 0..17 of `<init>`) stores `hashCode = name.hashCode()`
//! — a `String` hash, which is a pure function of the name and therefore
//! portable. That matters because `Brain.availableBehaviorsByPriority` is a
//! `TreeMap<Integer, HashMap<Activity, LinkedHashSet<BehaviorControl>>>`
//! (`Brain.<init>` offsets 18..22 and `lambda$addActivity$0/$1`): priority
//! ascending, then **`HashMap` order over the activities sharing that
//! priority**, then add order within an activity. When two active activities
//! — `CORE` and whatever the schedule picked, usually — both have behaviours
//! at one priority, which starts and ticks first is decided by
//! [`java_hash_map_order`], reproduced here from `HashMap.hash` (spread
//! `h ^ (h >>> 16)`), the default table of 16 doubling past 12, 24, …, and
//! insertion order within a bucket (`resize` splits chains preserving it).
//!
//! The 26 constants are `Activity.<clinit>`'s `register` calls in order.

use crate::brain::behavior::BehaviorControl;
use crate::brain::memory::{MemoryModuleType, MemoryStatus};
use crate::host::MobHost;

macro_rules! activities {
    ($( $name:ident = $s:literal ),* $(,)?) => {
        /// `Activity`, every registered instance in `<clinit>` order.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Activity {
            $( $name, )*
        }

        impl Activity {
            /// Every activity, in registration order.
            pub const ALL: &'static [Activity] = &[ $( Activity::$name, )* ];

            /// `getName()`.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $( Activity::$name => $s, )*
                }
            }
        }
    };
}

activities! {
    Core = "core",
    Idle = "idle",
    Work = "work",
    Play = "play",
    Rest = "rest",
    Meet = "meet",
    Panic = "panic",
    Raid = "raid",
    PreRaid = "pre_raid",
    Hide = "hide",
    Fight = "fight",
    Celebrate = "celebrate",
    AdmireItem = "admire_item",
    Avoid = "avoid",
    Ride = "ride",
    PlayDead = "play_dead",
    LongJump = "long_jump",
    Ram = "ram",
    Tongue = "tongue",
    Swim = "swim",
    LaySpawn = "lay_spawn",
    Sniff = "sniff",
    Investigate = "investigate",
    Roar = "roar",
    Emerge = "emerge",
    Dig = "dig",
}

impl Activity {
    /// `hashCode()` — `String.hashCode()` of the name: `s[0]*31^(n-1) + …`
    /// in wrapping `int` arithmetic.
    #[must_use]
    pub fn java_hash(self) -> i32 {
        java_string_hash(self.name())
    }
}

/// `java.lang.String.hashCode()` for an ASCII/UTF-16 string.
#[must_use]
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(u));
    }
    h
}

/// The iteration order of a `java.util.HashMap` whose keys were inserted
/// in the given order with the given `hashCode()`s: bucket
/// `(h ^ (h >>> 16)) & (n - 1)` on a table of 16 that doubles whenever the
/// size exceeds `0.75 * n`, insertion order within a bucket. Returns the
/// indices into `hashes` in iteration order. Tree bins (eight collisions in
/// one bucket on a table of at least 64) are not modelled and cannot occur
/// for the 26 activities.
#[must_use]
pub fn java_hash_map_order(hashes: &[i32]) -> Vec<usize> {
    let mut n: usize = 16;
    while hashes.len() > n * 3 / 4 {
        n *= 2;
    }
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, h) in hashes.iter().enumerate() {
        #[allow(clippy::cast_sign_loss)]
        let spread = (*h as u32) ^ ((*h as u32) >> 16);
        buckets[(spread as usize) & (n - 1)].push(i);
    }
    buckets.into_iter().flatten().collect()
}

/// `ActivityData<E>`: one `addActivity` call's worth of arguments.
pub struct ActivityData<M: MobHost> {
    /// `activityType`.
    pub activity: Activity,
    /// `behaviorPriorityPairs`, in list order.
    pub behaviors: Vec<(i32, Box<dyn BehaviorControl<M>>)>,
    /// `conditions` (a `Set<Pair<MemoryModuleType, MemoryStatus>>`; only
    /// ever all-tested, so order is immaterial).
    pub conditions: Vec<(MemoryModuleType, MemoryStatus)>,
    /// `memoriesToEraseWhenStopped`.
    pub memories_to_erase_when_stopped: Vec<MemoryModuleType>,
}

impl<M: MobHost> ActivityData<M> {
    /// `ActivityData.create(activity, ImmutableList<Pair<Integer, BehaviorControl>>)`
    /// — no conditions, nothing erased.
    #[must_use]
    pub fn new(activity: Activity, behaviors: Vec<(i32, Box<dyn BehaviorControl<M>>)>) -> Self {
        ActivityData {
            activity,
            behaviors,
            conditions: Vec::new(),
            memories_to_erase_when_stopped: Vec::new(),
        }
    }

    /// `ActivityData.create(activity, int, ImmutableList<BehaviorControl>)`:
    /// `createPriorityPairs(startPriority, list)` numbers the behaviours
    /// `start, start+1, …` in list order.
    #[must_use]
    pub fn with_priority(
        activity: Activity,
        start_priority: i32,
        behaviors: Vec<Box<dyn BehaviorControl<M>>>,
    ) -> Self {
        Self::new(activity, create_priority_pairs(start_priority, behaviors))
    }

    /// `ActivityData.create(activity, int, list, MemoryModuleType)`: the
    /// condition is `(memory, VALUE_PRESENT)` and the memory is erased when
    /// the activity stops.
    #[must_use]
    pub fn with_priority_and_memory(
        activity: Activity,
        start_priority: i32,
        behaviors: Vec<Box<dyn BehaviorControl<M>>>,
        memory: MemoryModuleType,
    ) -> Self {
        ActivityData {
            activity,
            behaviors: create_priority_pairs(start_priority, behaviors),
            conditions: vec![(memory, MemoryStatus::ValuePresent)],
            memories_to_erase_when_stopped: vec![memory],
        }
    }

    /// `ActivityData.create(activity, int, list, Set<Pair<..>>)` — conditions
    /// only, nothing erased.
    #[must_use]
    pub fn with_priority_and_conditions(
        activity: Activity,
        start_priority: i32,
        behaviors: Vec<Box<dyn BehaviorControl<M>>>,
        conditions: Vec<(MemoryModuleType, MemoryStatus)>,
    ) -> Self {
        ActivityData {
            activity,
            behaviors: create_priority_pairs(start_priority, behaviors),
            conditions,
            memories_to_erase_when_stopped: Vec::new(),
        }
    }

    /// `ActivityData.create(activity, pairs, conditions, memoriesToErase)`.
    #[must_use]
    pub fn full(
        activity: Activity,
        behaviors: Vec<(i32, Box<dyn BehaviorControl<M>>)>,
        conditions: Vec<(MemoryModuleType, MemoryStatus)>,
        memories_to_erase_when_stopped: Vec<MemoryModuleType>,
    ) -> Self {
        ActivityData {
            activity,
            behaviors,
            conditions,
            memories_to_erase_when_stopped,
        }
    }
}

/// `ActivityData.createPriorityPairs(int, ImmutableList)`.
#[must_use]
pub fn create_priority_pairs<M: MobHost>(
    start: i32,
    behaviors: Vec<Box<dyn BehaviorControl<M>>>,
) -> Vec<(i32, Box<dyn BehaviorControl<M>>)> {
    let mut p = start;
    behaviors
        .into_iter()
        .map(|b| {
            let pair = (p, b);
            p += 1;
            pair
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_hash_matches_java() {
        // "core".hashCode() and "idle".hashCode(), computed by hand from
        // s[0]*31^3 + s[1]*31^2 + s[2]*31 + s[3].
        assert_eq!(java_string_hash("core"), 3059615);
        assert_eq!(java_string_hash("idle"), 3227604);
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("hello world"), 1_794_106_052);
    }

    #[test]
    fn twenty_six_activities_in_registration_order() {
        assert_eq!(Activity::ALL.len(), 26);
        assert_eq!(Activity::ALL[0], Activity::Core);
        assert_eq!(Activity::ALL[25].name(), "dig");
        assert_eq!(Activity::PreRaid.name(), "pre_raid");
    }

    #[test]
    fn hash_map_order_is_by_bucket_then_insertion() {
        // Two keys in the same bucket keep insertion order; a key in a
        // lower bucket comes first regardless of when it was added.
        let order = java_hash_map_order(&[17, 3, 33]);
        assert_eq!(order, vec![0, 2, 1]);
        // core (0x2EAF9F) and idle (0x313FD4) on a table of 16: buckets 1
        // and 5, so core walks first whichever was added first.
        let core = Activity::Core.java_hash();
        let idle = Activity::Idle.java_hash();
        assert_eq!((core ^ (core >> 16)) & 15, 1);
        assert_eq!((idle ^ (idle >> 16)) & 15, 5);
        assert_eq!(java_hash_map_order(&[idle, core]), vec![1, 0]);
    }

    #[test]
    fn table_doubles_past_twelve() {
        // 13 keys all in bucket 0 on 16 spread over 32 once the table
        // doubles; keys 0 and 16 share bucket 0 on 16 but not on 32.
        let keys: Vec<i32> = (0..13).map(|i| i * 16).collect();
        let order = java_hash_map_order(&keys);
        // On 32: key 16*i lands in bucket (16*i) & 31 = 0 for even i, 16 for odd i.
        assert_eq!(order, vec![0, 2, 4, 6, 8, 10, 12, 1, 3, 5, 7, 9, 11]);
    }
}
