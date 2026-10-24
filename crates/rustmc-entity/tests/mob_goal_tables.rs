//! The per-mob `registerGoals` tables and attribute builders read from the
//! jar by `ci/gen-mob-goal-tables.py` (`crates/rustmc-entity/golden/
//! mob_goal_tables.json`, checked against the jar by `ci/mob-goal-tables.sh`).
//!
//! What can go red here: a goal class the jar registers directly from
//! `net.minecraft.world.entity.ai.goal` that this crate has not transcribed
//! (the list below is the crate's, and a jar bump that adds a class fails
//! it), an entry whose operand tokens do not match its constructor's
//! parameter count (a generator regression), or a table shape change.

use std::collections::BTreeSet;

const TABLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/mob_goal_tables.json");

/// Every class under `ai/goal/` this crate transcribes, by its jar name.
const TRANSCRIBED: &[&str] = &[
    "AvoidEntityGoal",
    "BegGoal",
    "BreakDoorGoal",
    "BreathAirGoal",
    "BreedGoal",
    "CatLieOnBedGoal",
    "CatSitOnBlockGoal",
    "ClimbOnTopOfPowderSnowGoal",
    "DolphinJumpGoal",
    "DoorInteractGoal",
    "EatBlockGoal",
    "FleeSunGoal",
    "FloatGoal",
    "FollowFlockLeaderGoal",
    "FollowMobGoal",
    "FollowOwnerGoal",
    "FollowParentGoal",
    "FollowPlayerRiddenEntityGoal",
    "GolemRandomStrollInVillageGoal",
    "InteractGoal",
    "JumpGoal",
    "LandOnOwnersShoulderGoal",
    "LeapAtTargetGoal",
    "LlamaFollowCaravanGoal",
    "LookAtPlayerGoal",
    "LookAtTradingPlayerGoal",
    "MeleeAttackGoal",
    "MoveBackToVillageGoal",
    "MoveThroughVillageGoal",
    "MoveToBlockGoal",
    "MoveTowardsRestrictionGoal",
    "MoveTowardsTargetGoal",
    "OcelotAttackGoal",
    "OfferFlowerGoal",
    "OpenDoorGoal",
    "PanicGoal",
    "PathfindToRaidGoal",
    "RandomLookAroundGoal",
    "RandomStandGoal",
    "RandomStrollGoal",
    "RandomSwimmingGoal",
    "RangedAttackGoal",
    "RangedBowAttackGoal",
    "RangedCrossbowAttackGoal",
    "RemoveBlockGoal",
    "RestrictSunGoal",
    "RunAroundLikeCrazyGoal",
    "SitWhenOrderedToGoal",
    "SpearUseGoal",
    "StrollThroughVillageGoal",
    "SwellGoal",
    "TemptGoal",
    "TemptGoal$ForNonPathfinders",
    "TradeWithPlayerGoal",
    "TryFindWaterGoal",
    "UseItemGoal",
    "WaterAvoidingRandomFlyingGoal",
    "WaterAvoidingRandomStrollGoal",
    "ZombieAttackGoal",
    "target/DefendVillageTargetGoal",
    "target/HurtByTargetGoal",
    "target/NearestAttackableTargetGoal",
    "target/NearestAttackableWitchTargetGoal",
    "target/NearestHealableRaiderTargetGoal",
    "target/NonTameRandomTargetGoal",
    "target/OwnerHurtByTargetGoal",
    "target/OwnerHurtTargetGoal",
    "target/ResetUniversalAngerTargetGoal",
];

fn load() -> serde_json::Value {
    let text = std::fs::read_to_string(TABLES).expect("golden/mob_goal_tables.json");
    serde_json::from_str(&text).expect("valid JSON")
}

/// The number of parameters in a JVM method descriptor `(...)V`.
fn descriptor_params(desc: &str) -> usize {
    let inner = &desc[1..desc.find(')').expect("descriptor")];
    let b = inner.as_bytes();
    let mut i = 0;
    let mut n = 0;
    while i < b.len() {
        match b[i] {
            b'[' => {
                i += 1;
                continue;
            }
            b'L' => {
                i = inner[i..].find(';').map(|j| i + j).expect("object type") + 1;
            }
            _ => i += 1,
        }
        n += 1;
    }
    n
}

#[test]
fn every_directly_registered_goal_class_is_transcribed() {
    let doc = load();
    let mobs = doc["register_goals"].as_object().expect("register_goals");
    assert_eq!(mobs.len(), 54, "classes declaring registerGoals in 26.2");
    let mut package = BTreeSet::new();
    let mut inner = BTreeSet::new();
    let mut entries = 0;
    for goals in mobs.values() {
        for g in goals.as_array().expect("list") {
            let class = g["class"].as_str().expect("class");
            if class == "super" {
                continue;
            }
            entries += 1;
            if let Some(rest) = class.strip_prefix("net/minecraft/world/entity/ai/goal/") {
                package.insert(rest.to_string());
            } else {
                inner.insert(class.to_string());
            }
        }
    }
    let transcribed: BTreeSet<String> = TRANSCRIBED.iter().map(|s| (*s).to_string()).collect();
    let missing: Vec<&String> = package
        .iter()
        .filter(|c| !transcribed.contains(*c))
        .collect();
    assert!(
        missing.is_empty(),
        "registered goal-package classes not transcribed: {missing:?}"
    );
    assert_eq!(
        package.len(),
        58,
        "goal-package classes registered directly by a mob"
    );
    assert!(
        inner.len() >= 100,
        "mob-inner goal classes (the mob lane's): {}",
        inner.len()
    );
    assert!(entries >= 400, "addGoal entries: {entries}");
}

#[test]
fn operand_tokens_match_each_constructor() {
    let doc = load();
    let mobs = doc["register_goals"].as_object().expect("register_goals");
    for (mob, goals) in mobs {
        for g in goals.as_array().expect("list") {
            let class = g["class"].as_str().expect("class");
            if class == "super" {
                continue;
            }
            let ctor = g["ctor"].as_str().expect("ctor");
            let args = g["args"].as_array().expect("args");
            assert_eq!(
                args.len(),
                descriptor_params(ctor),
                "{mob}: {class} {ctor} args {args:?}"
            );
            assert!(g["priority"].is_i64(), "{mob}: {class} has no priority");
            let sel = g["selector"].as_str().expect("selector");
            assert!(
                sel == "goalSelector" || sel == "targetSelector",
                "{mob}: {sel}"
            );
        }
    }
}

#[test]
fn attribute_builders_name_a_base_and_valued_adds() {
    let doc = load();
    let attrs = doc["attributes"].as_object().expect("attributes");
    let builders: usize = attrs
        .values()
        .map(|v| v.as_object().map_or(0, |o| o.len()))
        .sum();
    assert_eq!(
        builders, 81,
        "static create*Attributes builders in the entity package"
    );
    let wolf = &attrs["animal/wolf/Wolf"]["createAttributes"];
    assert_eq!(
        wolf["base"].as_str().unwrap(),
        "net/minecraft/world/entity/animal/Animal.createAnimalAttributes:()Lnet/minecraft/world/entity/ai/attributes/AttributeSupplier$Builder;"
    );
    let adds = wolf["adds"].as_array().unwrap();
    assert_eq!(adds[0]["attribute"], "MOVEMENT_SPEED");
    assert_eq!(adds[0]["value"], "double 0.30000001192092896d");
    assert_eq!(adds[1]["attribute"], "MAX_HEALTH");
    assert_eq!(adds[1]["value"], "double 8.0d");
}
