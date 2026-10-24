//! The per-mob brain providers and activity packages read from the jar by
//! `ci/gen-brain-tables.py` (`crates/rustmc-entity/golden/mob_brain_tables.json`,
//! checked against the jar by `ci/brain-tables.sh`).
//!
//! What can go red here: a behaviour class **any** of the twenty brain
//! packages names that this crate has neither transcribed nor listed as
//! pending (a jar bump that adds one fails it, and so does a lane forgetting
//! to move a class from `PENDING` to `TRANSCRIBED`), a `Pair.of(int,
//! behaviour)` whose first operand is not an integer literal (a generator
//! regression), a provider whose sensor list is not `getstatic SensorType.*`
//! tokens, and the table's shape — 20 providers, the villager's nine sensors
//! and the piglin's six memories and five sensors, in their `List.of` order.
//!
//! **The scope was villager + piglin until the fifth lane and is now all
//! twenty providers' packages.** A behaviour class is what the jar's own
//! descriptors say it is: a `new` of any `world/entity/` class outside
//! `ai/behavior/declarative` (the builder DSL, whose products are the
//! `Behavior`/`OneShot` infra rather than classes of their own), and a
//! `call` whose owner is under `ai/behavior/` or whose descriptor returns a
//! `BehaviorControl`, a `OneShot`, a `Behavior` or a `Trigger`. That rule is
//! what pulls in the mob-local behaviours the narrower reading missed —
//! `ai/behavior/warden/Roar`, `monster/breeze/Shoot`,
//! `monster/piglin/StartHuntingHoglin`, `animal/frog/ShootTongue`,
//! `animal/sniffer/SnifferAi$Digging` — every one of which is a
//! `BehaviorControl` the same way `RandomStroll` is.
//!
//! The positive control: every class the packages name must be in
//! `TRANSCRIBED` or in `PENDING`, the packages must name at least the
//! ratcheted floor of classes, and a class in both lists is refused, so the
//! ledger cannot be satisfied by copying one list into the other or by a
//! generator that names nothing.

use std::collections::BTreeSet;

use serde_json::Value;

const TABLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/mob_brain_tables.json");

/// Every `ai/behavior/*` class this crate transcribes, by its jar name.
const TRANSCRIBED: &[&str] = &[
    "TransportItemsBetweenContainers",
    "SpearRetreat",
    "ShootTongue",
    "GoAndGiveItemsToTarget",
    "ChargeAttack",
    "PrepareRamNearestTarget",
    "RamTarget",
    "CamelAi$CamelPanic",
    "CamelAi$RandomSitting",
    "CreakingAi$1",
    "ArmadilloAi$1",
    "ArmadilloAi$ArmadilloBallUp",
    "ArmadilloAi$ArmadilloPanic",
    "LongJump",
    "Shoot",
    "Slide",
    "BreezeAi$SlideToTargetSink",
    "ShootWhenStuck",
    "SnifferAi$Digging",
    "SnifferAi$FinishedDigging",
    "SnifferAi$Sniffing",
    "SnifferAi$Scenting",
    "SnifferAi$FeelingHappy",
    "SnifferAi$Searching",
    "SnifferAi$1",
    "SnifferAi$2",
    "SnifferAi$3",
    "PlayDead",
    "ValidatePlayDead",
    "BecomePassiveIfMemoryPresent",
    "EntityTracker",
    "RandomLookAround",
    "StayCloseToTarget",
    "Croak",
    "TryFindLand",
    "TryFindLandNearWater",
    "TryFindWater",
    "TryLaySpawnOnFluidNearLand",
    "Digging",
    "Emerging",
    "ForceUnmount",
    "Roar",
    "SetRoarTarget",
    "SetWardenLookTarget",
    "Sniffing",
    "SonicBoom",
    "TryToSniff",
    "RememberIfHoglinWasKilled",
    "StartAdmiringItemIfSeen",
    "StartHuntingHoglin",
    "StopAdmiringIfItemTooFarAway",
    "StopAdmiringIfTiredOfTryingToReachItem",
    "StopHoldingItemIfNoLongerAdmiring",
    "SpearAttack",
    "SpearApproach",
    "CrossbowAttack",
    "CelebrateVillagersSurvivedRaid",
    "GiveGiftToHero",
    "VillagerMakeLove",
    "TradeWithVillager",
    "ShowTradesToPlayer",
    "LookAndFollowTradingPlayerSink",
    "PlayTagWithOtherKids",
    "InteractWith",
    "SocializeAtBell",
    "JumpOnBed",
    "SleepInBed",
    "SetClosestHomeAsWalkTarget",
    "PoiCompetitorScan",
    "ValidateNearbyPoi",
    "YieldJobSite",
    "GoToPotentialJobSite",
    "LocateHidingPlace",
    "AssignProfessionFromJobSite",
    "MoveToSkySeeingSpot",
    "VillageBoundRandomStroll",
    "GoToClosestVillage",
    "InsideBrownianWalk",
    "VillagerPanicTrigger",
    "SetHiddenState",
    "AcquirePoi",
    "WakeUp",
    "VillagerCalmDown",
    "StartCelebratingIfTargetDead",
    "SetRaidStatus",
    "RingBell",
    "ResetRaidStatus",
    "ResetProfession",
    "ReactToBell",
    "Mount",
    "DismountOrSkipMounting",
    "CopyMemoryWithExpiry",
    "BackUpIfTooClose",
    "AnimalMakeLove",
    "AnimalPanic",
    "FollowTemptation",
    "SetWalkTargetFromBlockMemory",
    "StrollAroundPoi",
    "StrollToPoi",
    "BabyFollowAdult",
    "CountDownCooldownTicks",
    "EraseMemoryIf",
    "GoToTargetLocation",
    "GoToWantedItem",
    "SetLookAndInteract",
    "SetWalkTargetAwayFrom",
    "StopBeingAngryIfTargetDead",
    "BehaviorUtils",
    "DoNothing",
    "GateBehavior",
    "InteractWithDoor",
    "LongJumpMidJump",
    "LongJumpToPreferredBlock",
    "LongJumpToRandomPos",
    "LookAtTargetSink",
    "MeleeAttack",
    "MoveToTargetSink",
    "RandomStroll",
    "RunOne",
    "SetEntityLookTarget",
    "SetEntityLookTargetSometimes",
    "SetWalkTargetFromAttackTargetIfTargetOutOfReach",
    "SetWalkTargetFromLookTarget",
    "StartAttacking",
    "StopAttackingIfTargetInvalid",
    "Swim",
    "TriggerGate",
    "UpdateActivityFromSchedule",
    // `EntityTracker` is `PositionTracker::Entity` in `brain/memory.rs`,
    // transcribed by the first lane; the descriptor rule names it because
    // `AllayAi` constructs one, and it is a `PositionTracker` rather than
    // a behaviour.
    // Infra the descriptor rule now names, all three already in the crate:
    // `Behavior` and `BehaviorBuilder` are `brain/behavior.rs`, and the
    // `Ticker` is the inner class of `SetEntityLookTargetSometimes`.
    "Behavior",
    "BehaviorBuilder",
    "SetEntityLookTargetSometimes$Ticker",
];

/// Every behaviour class the twenty brain packages name that is not yet
/// transcribed. A class leaves this list when its file lands; the seventh
/// lane emptied it, so every behaviour class all twenty providers name is
/// now in `TRANSCRIBED`.
///
/// The positive control below still fires on an empty list: a class in
/// neither list is refused, so emptying this one cannot be a way of
/// satisfying the ledger.
const PENDING: &[&str] = &[];

/// Every `SensorType` a provider names that `brain::sensors::create` cannot
/// build yet. A name leaves this list when its sensor lands; the fifth
/// lane emptied it.
const PENDING_SENSORS: &[&str] = &[];

fn tables() -> Value {
    let text = std::fs::read_to_string(TABLES).expect("golden/mob_brain_tables.json");
    serde_json::from_str(&text).expect("json")
}

const ENTITY: &str = "net/minecraft/world/entity/";
const BEHAVIOR: &str = "net/minecraft/world/entity/ai/behavior/";
const DECLARATIVE: &str = "net/minecraft/world/entity/ai/behavior/declarative/";

/// The behaviour types a factory method returns. A `call` whose descriptor
/// ends in one of these is a behaviour factory wherever it lives, which is
/// how the mob-local classes are found without a hand-kept package list.
const BEHAVIOR_RETURNS: &[&str] = &[
    "Lnet/minecraft/world/entity/ai/behavior/BehaviorControl;",
    "Lnet/minecraft/world/entity/ai/behavior/OneShot;",
    "Lnet/minecraft/world/entity/ai/behavior/Behavior;",
    "Lnet/minecraft/world/entity/ai/behavior/Trigger;",
];

/// The class's simple name: the last `/`-separated segment, inner classes
/// kept (`SnifferAi$Digging`).
fn simple(class: &str) -> String {
    class.rsplit('/').next().unwrap_or(class).to_string()
}

/// Every behaviour class named by `new` or by a behaviour-returning `call`
/// anywhere under `v`. See this file's header for the rule.
fn behavior_classes(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("new") {
                if s.starts_with(ENTITY) && !s.starts_with(DECLARATIVE) {
                    out.insert(simple(s));
                }
            }
            if let Some(Value::String(s)) = m.get("call") {
                if let Some((target, desc)) = s.split_once(':') {
                    let owner = target.rsplit_once('.').map_or(target, |(o, _)| o);
                    let ret = desc.rsplit_once(')').map_or("", |(_, r)| r);
                    let owned_by_behavior =
                        owner.starts_with(BEHAVIOR) && !owner.starts_with(DECLARATIVE);
                    if owner.starts_with(ENTITY)
                        && (owned_by_behavior || BEHAVIOR_RETURNS.contains(&ret))
                    {
                        out.insert(simple(owner));
                    }
                }
            }
            for x in m.values() {
                behavior_classes(x, out);
            }
        }
        Value::Array(a) => {
            for x in a {
                behavior_classes(x, out);
            }
        }
        _ => {}
    }
}

/// Every `Pair.of(a, b)` node under `v`.
fn pairs<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            if m.get("call")
                .and_then(Value::as_str)
                .is_some_and(|c| c.starts_with("com/mojang/datafixers/util/Pair.of:"))
            {
                out.push(v);
            }
            for x in m.values() {
                pairs(x, out);
            }
        }
        Value::Array(a) => {
            for x in a {
                pairs(x, out);
            }
        }
        _ => {}
    }
}

fn getstatic_names(list: &Value, owner: &str) -> Vec<String> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|t| {
            let s = t.as_str().expect("a token");
            let prefix = format!("getstatic net/minecraft/world/entity/{owner}.");
            s.strip_prefix(&prefix)
                .unwrap_or_else(|| panic!("not a {owner} token: {s}"))
                .to_string()
        })
        .collect()
}

#[test]
fn twenty_providers_and_the_villager_and_piglin_lists_in_push_order() {
    let t = tables();
    let providers = t["providers"].as_object().expect("providers");
    assert_eq!(
        providers.len(),
        20,
        "{:?}",
        providers.keys().collect::<Vec<_>>()
    );
    let villager = &providers["npc/villager/Villager"];
    assert_eq!(
        villager["memories"].as_array().unwrap().len(),
        0,
        "the two-argument provider"
    );
    assert_eq!(
        getstatic_names(&villager["sensors"], "ai/sensing/SensorType"),
        [
            "NEAREST_LIVING_ENTITIES",
            "NEAREST_PLAYERS",
            "NEAREST_ITEMS",
            "NEAREST_BED",
            "HURT_BY",
            "VILLAGER_HOSTILES",
            "VILLAGER_BABIES",
            "SECONDARY_POIS",
            "GOLEM_DETECTED",
        ]
    );
    assert!(villager["supplier"]["lambda"].is_string());
    let piglin = &providers["monster/piglin/Piglin"];
    assert_eq!(
        getstatic_names(&piglin["memories"], "ai/memory/MemoryModuleType"),
        [
            "UNIVERSAL_ANGER",
            "ATE_RECENTLY",
            "SPEAR_FLEEING_TIME",
            "SPEAR_FLEEING_POSITION",
            "SPEAR_CHARGE_POSITION",
            "SPEAR_ENGAGE_TIME",
        ]
    );
    assert_eq!(
        getstatic_names(&piglin["sensors"], "ai/sensing/SensorType"),
        [
            "NEAREST_LIVING_ENTITIES",
            "NEAREST_PLAYERS",
            "NEAREST_ITEMS",
            "HURT_BY",
            "PIGLIN_SPECIFIC_SENSOR",
        ]
    );
    for (name, p) in providers {
        if p.get("branches").is_some() {
            continue;
        }
        let _ = getstatic_names(&p["sensors"], "ai/sensing/SensorType");
        let _ = getstatic_names(&p["memories"], "ai/memory/MemoryModuleType");
        assert!(p.get("supplier").is_some(), "{name}");
    }
}

#[test]
fn every_brain_package_behaviour_is_transcribed_or_named_pending() {
    let t = tables();
    let transcribed: BTreeSet<&str> = TRANSCRIBED.iter().copied().collect();
    let pending: BTreeSet<&str> = PENDING.iter().copied().collect();
    assert!(
        transcribed.is_disjoint(&pending),
        "a class in both lists: {:?}",
        transcribed.intersection(&pending).collect::<Vec<_>>()
    );
    // Positive control: the packages must still name every class this ledger
    // covers (a generator that stopped finding them would otherwise pass
    // vacuously), and every one of them must be in one of the two lists —
    // drop a class from both and this goes red. The floor is ratcheted: 131
    // is what the twenty packages named on 2026-09-05, and the villager and
    // piglin two named 71 of them.
    let named_floor = 131;
    let mut named = BTreeSet::new();
    for body in t["packages"].as_object().expect("packages").values() {
        behavior_classes(body, &mut named);
    }
    assert!(
        named.len() >= named_floor,
        "the twenty brain packages named {} behaviour classes, floor {named_floor}",
        named.len()
    );
    // The narrow scope this test had until the fifth lane, kept as its own
    // floor so a widening that silently dropped the old set is refused.
    let mut villager_and_piglin = BTreeSet::new();
    for cls in [
        "ai/behavior/VillagerGoalPackages",
        "monster/piglin/PiglinAi",
    ] {
        behavior_classes(&t["packages"][cls], &mut villager_and_piglin);
    }
    assert!(
        villager_and_piglin.len() >= 71,
        "the villager and piglin packages named {}, floor 71",
        villager_and_piglin.len()
    );
    assert!(
        villager_and_piglin.is_subset(&named),
        "the widened walk lost a class the narrow one found"
    );
    let unknown: Vec<&String> = named
        .iter()
        .filter(|n| !transcribed.contains(n.as_str()) && !pending.contains(n.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "behaviour classes neither transcribed nor pending: {unknown:?}"
    );
    let stale: Vec<&&str> = pending.iter().filter(|p| !named.contains(**p)).collect();
    assert!(
        stale.is_empty(),
        "pending classes the packages no longer name: {stale:?}"
    );
    assert_eq!(
        named.len(),
        named
            .iter()
            .filter(|n| transcribed.contains(n.as_str()))
            .count()
            + named
                .iter()
                .filter(|n| pending.contains(n.as_str()))
                .count()
    );
}

#[test]
fn every_priority_pair_carries_an_integer_priority_and_the_core_packages_are_as_read() {
    let t = tables();
    let mut all = Vec::new();
    for body in t["packages"].as_object().expect("packages").values() {
        pairs(body, &mut all);
    }
    assert!(all.len() > 60, "{} pairs", all.len());
    let mut priority_pairs = 0;
    for p in &all {
        let args = p["args"].as_array().expect("two operands");
        assert_eq!(args.len(), 2);
        if let Some(s) = args[0].as_str() {
            if s.parse::<i32>().is_ok() {
                priority_pairs += 1;
                assert!(
                    args[1].is_object(),
                    "a priority pair's behaviour is a construction or call: {p}"
                );
            }
        }
    }
    assert!(priority_pairs > 40, "{priority_pairs} priority pairs");
    // VillagerGoalPackages.getCorePackage(profession, speed): the first four
    // entries, as the bytecode pushes them.
    let core = t["packages"]["ai/behavior/VillagerGoalPackages"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(k, _)| k.contains(" getCorePackage("))
        .map(|(_, v)| v)
        .expect("getCorePackage");
    let entries = core["result"]["args"].as_array().unwrap();
    let head: Vec<String> = entries
        .iter()
        .take(4)
        .map(|e| {
            let b = &e["args"][1];
            format!(
                "{}:{}",
                e["args"][0].as_str().unwrap(),
                b.get("new")
                    .or_else(|| b.get("call"))
                    .unwrap()
                    .as_str()
                    .unwrap()
            )
        })
        .collect();
    assert_eq!(
        head,
        [
            "0:net/minecraft/world/entity/ai/behavior/Swim",
            "0:net/minecraft/world/entity/ai/behavior/InteractWithDoor.create:()Lnet/minecraft/world/entity/ai/behavior/BehaviorControl;",
            "0:net/minecraft/world/entity/ai/behavior/LookAtTargetSink",
            "0:net/minecraft/world/entity/ai/behavior/VillagerPanicTrigger",
        ]
    );
    assert_eq!(
        entries[0]["args"][1]["args"],
        serde_json::json!(["float 0.8f"])
    );
    assert_eq!(
        entries[2]["args"][1]["args"],
        serde_json::json!(["45", "90"])
    );
}

/// Every sensor any of the twenty providers names is either transcribed —
/// `brain::sensors::create` answers `Some` for its `SensorType` — or listed
/// in `PENDING_SENSORS`, in which case the factory must answer `None`.
///
/// Both halves are load-bearing. Remove an arm from that factory and this
/// goes red for the type it covered; add an arm without moving the name out
/// of `PENDING_SENSORS` and it goes red too, so the ledger cannot drift in
/// either direction.
#[test]
fn every_provider_sensor_is_transcribed_or_named_pending() {
    use rustmc_entity::brain::sensor::SensorType;
    use rustmc_entity::brain::sensors::create;
    use rustmc_entity::testing::TestMob;
    let t = tables();
    let providers = t["providers"].as_object().expect("providers");
    let pending: BTreeSet<&str> = PENDING_SENSORS.iter().copied().collect();
    let mut slots = 0;
    let mut named = BTreeSet::new();
    for p in providers.values() {
        for token in getstatic_names(&p["sensors"], "ai/sensing/SensorType") {
            let name = token.to_ascii_lowercase();
            let ty = SensorType::ALL
                .iter()
                .copied()
                .find(|s| s.name() == name)
                .unwrap_or_else(|| panic!("no SensorType for {token}"));
            match create::<TestMob>(ty) {
                Some(sensor) => {
                    assert_eq!(sensor.sensor_type(), ty);
                    assert!(
                        !pending.contains(token.as_str()),
                        "{token} is transcribed but still in PENDING_SENSORS"
                    );
                }
                None => assert!(
                    pending.contains(token.as_str()),
                    "{token} ({name}) has no transcription and is not pending"
                ),
            }
            named.insert(token);
            slots += 1;
        }
    }
    assert_eq!(slots, 87, "sensor slots across the twenty providers");
    assert_eq!(named.len(), 23, "distinct sensor types: {named:?}");
    let stale: Vec<&&str> = pending.iter().filter(|s| !named.contains(**s)).collect();
    assert!(
        stale.is_empty(),
        "pending sensors no provider names: {stale:?}"
    );
    // The villager's nine and the piglin's five, which this test covered
    // alone until the fifth lane, must all still be transcribed.
    for key in ["npc/villager/Villager", "monster/piglin/Piglin"] {
        for token in getstatic_names(&providers[key]["sensors"], "ai/sensing/SensorType") {
            assert!(!pending.contains(token.as_str()), "{token} regressed");
        }
    }
}
