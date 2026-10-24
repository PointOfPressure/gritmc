//! The per-tick body of a living mob: `Mob.tick` down through
//! `LivingEntity.tick`, `Entity.baseTick`, `LivingEntity.aiStep`,
//! `travel` and `Entity.move`.
//!
//! Reference (26.2), each transcribed from `javap -p -c -constants` on the
//! pinned jar and named at its function: `Mob.tick`, `Mob.baseTick`,
//! `Mob.aiStep`, `LivingEntity.tick`, `LivingEntity.baseTick`,
//! `LivingEntity.aiStep`, `LivingEntity.applyInput`, `LivingEntity.travel`,
//! `travelInAir`, `handleRelativeFrictionAndCalculateMovement`,
//! `getFrictionInfluencedSpeed`, `getFlyingSpeed`, `jumpFromGround`,
//! `getJumpPower`, `getEffectiveGravity`, `computeModifiedFriction`,
//! `LivingEntity.tickHeadTurn` (`Mob`'s override), `pushEntities`,
//! `Entity.push(Entity)`, `Entity.baseTick`, `Entity.computeSpeed`,
//! `Entity.move`, `Entity.checkFallDamage`, `LivingEntity.checkFallDamage`,
//! `EntityFluidInteraction.update`, `Entity.getBlockJumpFactor`,
//! `AgeableMob.aiStep`, `Animal.aiStep`, `Animal.customServerAiStep`.
//!
//! # What is here and what is named as not
//!
//! Everything a pig standing, turning, walking, jumping and falling on
//! dry land runs, in the jar's order, with every draw from the mob's own
//! random in the jar's order. Not here, each at the line where vanilla
//! would run it, because the mechanic behind it is a different lane's:
//!
//! * **damage** — `hurtServer` from suffocation, drowning, freezing, fall
//!   and cramming. `player-systems` owns damage; the *conditions* are
//!   evaluated where they cost a draw or a synched value (air supply is
//!   decremented and synced; the drowning hit is not dealt).
//! * **inside-block effects** — `applyEffectsFromBlocks` (cobweb, berry
//!   bush, powder snow, fire). No block on the corpus's superflat has one.
//! * **fluid currents** — `EntityFluidInteraction.applyCurrentTo`: the flow
//!   vector needs `FlowingFluid.getFlow`. Heights, the in-fluid flags, the
//!   splash (its draws and its sound) and `travelInFluid` are all here;
//!   the push is not applied.
//! * **portals, leashes, riding, sleeping, effects, equipment** — none
//!   exists for these two kinds.
use super::host::{ServerMob, MOVEMENT_EFFICIENCY, MOVEMENT_SPEED};
use super::level::MobLevel;
use crate::collide::Vec3 as CVec3;
use crate::physics;
use rustmc_data::BlockPos;
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::host::{MobHost, PathLevel};
use rustmc_entity::mth;
use rustmc_entity::phys::Vec3;
use rustmc_worldgen::rng::RandomSource;

/// `Entity.baseTick`, `Mob.baseTick` around `LivingEntity.baseTick`.
fn base_tick(ai: &mut MobAi<ServerMob>) {
    // LivingEntity.baseTick 0-5: oAttackAnim = attackAnim.
    ai.mob.o_attack_anim = ai.mob.attack_anim;
    // 8-25: first-tick sleeping position — no bed. 28-47: enchantment
    // effects tick — no equipment.
    entity_base_tick(&mut ai.mob);
    // Entity.baseTick 248-280, its last act: `Leashable.tickLeash`.
    super::leash::tick_leash(ai);
    // LivingEntity.baseTick 67-450: alive and on a server level.
    if ai.mob.is_alive() {
        // 96-116: `isInWall()` → suffocation damage. Not dealt: damage is
        // unported (player-systems). The wall test itself is on the
        // eye box against `isSuffocating` shapes, which no pig meets on
        // open ground; it is not evaluated because its only effect is the
        // hit.
        // 206-419: air supply.
        let eye_in_water = ai.mob.eyes_in_fluid[0];
        if eye_in_water && !eye_in_bubble_column(&ai.mob) {
            // `canBreatheUnderwater()` is the CAN_BREATHE_UNDER_WATER tag,
            // which holds neither kind; no water breathing effect.
            let air = ai.mob.data.get_varint(super::data::id::AIR_SUPPLY);
            let next = decrease_air_supply(&mut ai.mob, air);
            ai.mob.data.set_varint(super::data::id::AIR_SUPPLY, next);
            // 299-331: `shouldTakeDrowningDamage()` (`air <= -20`) →
            // `setAirSupply(0)`, entity event 67 and a drowning hit.
            // Only the reset is applied; the event and the hit are damage.
            if next <= -20 {
                ai.mob.data.set_varint(super::data::id::AIR_SUPPLY, 0);
            }
        } else {
            let air = ai.mob.data.get_varint(super::data::id::AIR_SUPPLY);
            if air < 300 {
                ai.mob
                    .data
                    .set_varint(super::data::id::AIR_SUPPLY, (air + 4).min(300));
            }
        }
        // 419-447: lastPos / onChangedBlock (enchantment location effects).
        let bp = ai.mob.block_position();
        if ai.mob.last_pos != Some(bp) {
            ai.mob.last_pos = Some(bp);
        }
    }
    // 450-466: hurtTime, 467-491: invulnerableTime.
    if ai.mob.hurt_time > 0 {
        ai.mob.hurt_time -= 1;
    }
    if ai.mob.invulnerable_time > 0 {
        ai.mob.invulnerable_time -= 1;
    }
    // 491-513: `isDeadOrDying() && shouldTickDeath` → tickDeath. No
    // damage, no death.
    // 513-538: lastHurtByPlayer memory.
    if ai.mob.last_hurt_by_player_memory_time > 0 {
        ai.mob.last_hurt_by_player_memory_time -= 1;
    }
    // 538-560: lastHurtMob cleared when dead.
    if let Some(id) = ai.mob.last_hurt_mob {
        if !ai.mob.facts_of(id).is_some_and(|f| f.alive) {
            ai.mob.last_hurt_mob = None;
        }
    }
    // 560-603: lastHurtByMob: cleared when dead or after 100 ticks.
    if let Some(id) = ai.mob.last_hurt_by_mob {
        let alive = ai.mob.facts_of(id).is_some_and(|f| f.alive);
        if !alive || ai.mob.tick_count - ai.mob.last_hurt_by_mob_timestamp > 100 {
            ai.mob.last_hurt_by_mob = None;
        }
    }
    // 603-606: tickEffects — no effects. 607-638: the rotation history.
    ai.mob.y_head_rot_o = ai.mob.y_head_rot;
    ai.mob.y_body_rot_o = ai.mob.y_body_rot;
    ai.mob.y_rot_o = ai.mob.y_rot;
    ai.mob.x_rot_o = ai.mob.x_rot;

    // Mob.baseTick 17-58: the ambient sound draw, when alive. A `Mob`
    // override: an armor stand's `baseTick` is `LivingEntity`'s.
    if ai.mob.is_alive() && ai.mob.kind.is_mob() {
        let draw = ai.mob.random.next_i32_bound(1000);
        let t = ai.mob.ambient_sound_time;
        ai.mob.ambient_sound_time += 1;
        if draw < t {
            // `resetAmbientSoundTime()`: `-getAmbientSoundInterval()`.
            ai.mob.ambient_sound_time = -ai.mob.kind.ambient_sound_interval;
            play_ambient_sound(&mut ai.mob);
        }
    }
}

/// `Mob.playAmbientSound` → `makeSound(getAmbientSound())` →
/// `playSound(sound, getSoundVolume(), getVoicePitch())`.
fn play_ambient_sound(m: &mut ServerMob) {
    let Some(event) = super::ambient_sound(m) else {
        return;
    };
    let volume = m.kind.sound_volume;
    let pitch = voice_pitch(m);
    play_sound(m, event, volume, pitch);
}

/// `LivingEntity.getVoicePitch`, with `Bat.getVoicePitch`'s `* 0.95f`.
pub fn voice_pitch(m: &mut ServerMob) -> f32 {
    let a = m.random.next_f32();
    let b = m.random.next_f32();
    let base = if m.is_baby() {
        (a - b) * 0.2 + 1.5
    } else {
        (a - b) * 0.2 + 1.0
    };
    base * m.kind.voice_pitch_scale
}

/// `Entity.playSound(SoundEvent, volume, pitch)`: refused when silent,
/// else `level.playSound(null, x, y, z, event, getSoundSource(), volume,
/// pitch)`, which broadcasts to every player within `volume > 1 ? 16 *
/// volume : 16` blocks.
pub fn play_sound(m: &ServerMob, event: &str, volume: f32, pitch: f32) {
    if m.data.get_bool(super::data::id::SILENT) {
        return;
    }
    super::play_level_sound(m, event, volume, pitch);
}

/// `EntityFluidInteraction.update` reads no bubble column; the
/// `baseTick` test is `getBlockState(eye pos).is(BUBBLE_COLUMN)`.
fn eye_in_bubble_column(m: &ServerMob) -> bool {
    let p = rustmc_entity::phys::block_pos_containing(m.x(), m.eye_y(), m.z());
    m.level.block_name(p) == Some("minecraft:bubble_column")
}

/// `LivingEntity.decreaseAirSupply(int)`: the `OXYGEN_BONUS` attribute
/// instance (created here, and syncable) gates a `nextDouble` draw.
fn decrease_air_supply(m: &mut ServerMob, air: i32) -> i32 {
    // `IronGolem.decreaseAirSupply(int)` is `iload_1; ireturn`: no attribute
    // instance, no draw.
    if let super::DataLayout::IronGolem = m.kind.data {
        return air;
    }
    let bonus = m
        .attributes
        .instance("minecraft:oxygen_bonus")
        .map(|_| ())
        .and_then(|()| m.attributes.value("minecraft:oxygen_bonus"))
        .unwrap_or(0.0);
    if bonus > 0.0 && m.random.next_f64() >= 1.0 / (bonus + 1.0) {
        return air;
    }
    air - 1
}

/// `Entity.baseTick`, offsets 0-289, the server branches.
fn entity_base_tick(m: &mut ServerMob) {
    // 13: computeSpeed.
    let pos = m.position();
    let last = m.last_known_position.unwrap_or(pos);
    m.last_known_speed = pos.subtract(last);
    m.last_known_position = Some(pos);
    // 17-19: inBlockState = null. 22-60: passenger / boarding cooldown.
    // 60: handlePortal — no portal process. 64-72: sprint particle — not
    // sprinting.
    m.was_in_powder_snow = m.is_in_powder_snow;
    m.is_in_powder_snow = false;
    // 88-96: wasEyeInWater = isEyeInFluid(WATER).
    m.was_eye_in_water = m.eyes_in_fluid[0];
    // 99: updateFluidInteraction.
    update_fluid_interaction(m);
    // 104: updateSwimming — not sprinting, never swims.
    // 125-194: fire ticks; `clearFire` on a non-server level only.
    if m.remaining_fire_ticks > 0 {
        // fireImmune is false for both kinds; the periodic hit is damage.
        m.remaining_fire_ticks -= 1;
    }
    // 194-213: lava halves fall distance.
    if m.is_in_lava() {
        m.fall_distance *= 0.5;
    }
    // 213: checkBelowWorld: `y < minY - 64` → discard.
    if m.y() < f64::from(m.level.min_y() - 64) {
        m.discard();
    }
    // 227-240: setSharedFlagOnFire(remainingFireTicks > 0).
    let flags = m.data.get_byte(super::data::id::SHARED_FLAGS);
    let on_fire = m.remaining_fire_ticks > 0;
    let next = if on_fire { flags | 1 } else { flags & !1 };
    m.data.set_byte(super::data::id::SHARED_FLAGS, next);
    // 243-245.
    m.first_tick = false;
    // 265-280: Leashable.tickLeash — needs the goal selector and the
    // navigation, so `base_tick` calls it straight after this returns.
}

/// `EntityFluidInteraction.update(entity, !isPushedByFluid())` followed by
/// `Entity.updateFluidInteraction`'s own body: the water flag, the fall
/// distance reset, and (not applied) the current.
fn update_fluid_interaction(m: &mut ServerMob) {
    // Tracker reset: every tracker's height to 0, eyesInside false.
    m.fluid_height = [0.0; 2];
    m.eyes_in_fluid = [false; 2];
    let bb = m.bounding_box();
    // `getFluidInteractionBox()`: `getBoundingBox().deflate(0.001)`.
    let fb = bb.inflate(-0.001, -0.001, -0.001);
    let i = floor(fb.min_x);
    let j = floor(fb.min_y);
    let k = floor(fb.min_z);
    let l = ceil(fb.max_x) - 1;
    let i1 = ceil(fb.max_y) - 1;
    let j1 = ceil(fb.max_z) - 1;
    // `hasFluidAndLoaded(level, i - 1, j, k - 1, l + 1, i1, j1 + 1)`: every
    // chunk in that range loaded, else return without updating.
    let (cx0, cx1) = ((i - 1) >> 4, (l + 1) >> 4);
    let (cz0, cz1) = ((k - 1) >> 4, (j1 + 1) >> 4);
    for cx in cx0..=cx1 {
        for cz in cz0..=cz1 {
            if !m.level.is_chunk_loaded(cx, cz) {
                return;
            }
        }
    }
    let min_y = bb.min_y;
    let bx = floor(m.x());
    let eye_y = m.eye_y();
    let bz = floor(m.z());
    for x in i..=l {
        for y in j..=i1 {
            for z in k..=j1 {
                let pos = BlockPos { x, y, z };
                let Some(row) = m.level.row(pos) else {
                    continue;
                };
                if row.fluid_kind == 0 {
                    continue;
                }
                let d = f64::from(y);
                // `FluidState.getHeight(level, pos)`: 1 with the same fluid
                // above, else `getOwnHeight`.
                let above = m.level.row(BlockPos { x, y: y + 1, z });
                let h = if above.is_some_and(|a| a.fluid_kind == row.fluid_kind) {
                    1.0_f32
                } else {
                    row.fluid_own_height
                };
                let top = d + f64::from(h);
                if top < fb.min_y {
                    continue;
                }
                let t = usize::from(row.fluid_kind == 2);
                if x == bx && z == bz && eye_y >= d && eye_y <= top {
                    m.eyes_in_fluid[t] = true;
                }
                let height = (top - min_y).max(m.fluid_height[t]);
                m.fluid_height[t] = height;
                // `!pushed` is false here (mobs are pushed by fluid), so
                // the flow is accumulated: `FluidState.getFlow` is not
                // ported and `applyCurrentTo` is not applied.
            }
        }
    }
    // Entity.updateFluidInteraction 24-70.
    let in_water = m.fluid_height[0] > 0.0;
    if in_water {
        m.fall_distance = 0.0;
        if !m.was_touching_water && !m.first_tick {
            water_splash_effect(m);
        }
    }
    m.was_touching_water = in_water;
}

fn floor(d: f64) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "a block coordinate")]
    {
        d.floor() as i32
    }
}

fn ceil(d: f64) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "a block coordinate")]
    {
        d.ceil() as i32
    }
}

/// `Mob.tick()`: `LivingEntity.tick()`, then the control-flag refresh.
pub fn tick(ai: &mut MobAi<ServerMob>) {
    living_tick(ai);
    // `Mob.tick`'s tail; an armor stand's `tick` is `LivingEntity`'s.
    if ai.mob.kind.is_mob() {
        ai.tick();
    }
    // `CopperGolem.tick` 1-50: `super.tick()`, then on the server
    // `updateWeathering(level, level.getRandom(), level.getGameTime())`.
    if let super::DataLayout::CopperGolem = ai.mob.kind.data {
        super::golem::copper_golem_update_weathering(&mut ai.mob);
    }
}

/// `LivingEntity.tick()`, offsets 0-663.
fn living_tick(ai: &mut MobAi<ServerMob>) {
    // 0-1: Entity.tick → baseTick (virtual: Mob's).
    base_tick(ai);
    // 4-7: updatingUsingItem — not using. 8-11: updateSwimAmount — a
    // client animation field. 22-124: arrow and stinger counts, both 0.
    // 124: detectEquipmentUpdates — no equipment. 128-142: combat tracker
    // recheck every 20 ticks — nothing to recheck. 145-166: sleeping.
    // 170-178.
    if !ai.mob.removed {
        ai_step(ai);
    }
    // 181-305: the body rotation from this tick's movement.
    let dx = ai.mob.x() - ai.mob.old_pos.0;
    let dz = ai.mob.z() - ai.mob.old_pos.2;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let f5 = (dx * dx + dz * dz) as f32;
    let mut f6 = ai.mob.y_body_rot;
    if f5 > 0.002_500_000_2 {
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
        let f7 = (mth::atan2(dz, dx) as f32) * 57.295_776 - 90.0;
        let f8 = (mth::wrap_degrees(ai.mob.y_rot) - f7).abs();
        if 95.0 < f8 && f8 < 265.0 {
            f6 = f7 - 180.0;
        } else {
            f6 = f7;
        }
    }
    if ai.mob.attack_anim > 0.0 {
        f6 = ai.mob.y_rot;
    }
    // 320-323: tickHeadTurn(f6) — Mob's: bodyRotationControl.clientTick().
    // The argument is unused by the override.
    let _ = f6;
    if let super::DataLayout::ArmorStand = ai.mob.kind.data {
        super::armor_stand::tick_head_turn(&mut ai.mob);
    } else {
        ai.tick_head_turn();
    }
    // 343-591: the range checks on the four rotation histories.
    while ai.mob.y_rot - ai.mob.y_rot_o < -180.0 {
        ai.mob.y_rot_o -= 360.0;
    }
    while ai.mob.y_rot - ai.mob.y_rot_o >= 180.0 {
        ai.mob.y_rot_o += 360.0;
    }
    while ai.mob.y_body_rot - ai.mob.y_body_rot_o < -180.0 {
        ai.mob.y_body_rot_o -= 360.0;
    }
    while ai.mob.y_body_rot - ai.mob.y_body_rot_o >= 180.0 {
        ai.mob.y_body_rot_o += 360.0;
    }
    while ai.mob.x_rot - ai.mob.x_rot_o < -180.0 {
        ai.mob.x_rot_o -= 360.0;
    }
    while ai.mob.x_rot - ai.mob.x_rot_o >= 180.0 {
        ai.mob.x_rot_o += 360.0;
    }
    while ai.mob.y_head_rot - ai.mob.y_head_rot_o < -180.0 {
        ai.mob.y_head_rot_o -= 360.0;
    }
    while ai.mob.y_head_rot - ai.mob.y_head_rot_o >= 180.0 {
        ai.mob.y_head_rot_o += 360.0;
    }
    // 598-623: fallFlyTicks (never fall-flying), sleeping pitch.
    ai.mob.fall_fly_ticks = 0;
    // 635: refreshDirtyAttributes → onAttributeUpdated for each; the two
    // that do anything (MAX_HEALTH clamp, MAX_ABSORPTION) never move here.
    let updated = ai.mob.attributes.take_to_update();
    for a in updated {
        if a == super::host::MAX_HEALTH {
            let max = ai.mob.max_health();
            if ai.mob.health() > max {
                ai.mob.set_health(max);
            }
        }
    }
    // 639-643: elytra animation — client. 646-660: impulse grace.
}

/// The `aiStep` chain: `Animal.aiStep` → `AgeableMob.aiStep` →
/// `Mob.aiStep` → `LivingEntity.aiStep`, each running its super first.
fn ai_step(ai: &mut MobAi<ServerMob>) {
    living_ai_step(ai);
    // Mob.aiStep 4-15: BURN_IN_DAYLIGHT — neither kind. 31-211: looting —
    // `canPickUpLoot()` false for both.
    if ai.mob.kind.data.is_animal() {
        // AgeableMob.aiStep 74-116 (server).
        if ai.mob.is_alive() {
            let mut age = ai.mob.get_age();
            // `canAgeUp()`: `isBaby() && !isAgeLocked()`.
            let locked = ai.mob.data.get_bool(super::data::id::AGE_LOCKED);
            if ai.mob.is_baby() && !locked {
                age += 1;
                ai.mob.set_age(age);
            } else if age > 0 {
                age -= 1;
                ai.mob.set_age(age);
            }
        }
        // 116-133: `ageLockParticleTimer = makeAgeLockedParticle(level, this,
        // ageLockParticleTimer, isAgeLocked())`. The particle is a client
        // effect, but the countdown is not: offsets 0-104 decrement any
        // positive timer on either side, and the golden dandelion's
        // `canUseGoldenDandelion` waits for it to reach 0.
        if ai.mob.age_lock_particle_timer > 0 {
            ai.mob.age_lock_particle_timer -= 1;
        }
        // Animal.aiStep 4-118.
        if ai.mob.get_age() != 0 {
            ai.mob.in_love = 0;
        }
        if ai.mob.in_love > 0 {
            ai.mob.in_love -= 1;
            if ai.mob.in_love % 10 == 0 {
                // Three gaussian draws for the heart particle's velocity;
                // the particle is a client effect and is not sent.
                let _ = ai.mob.random.next_gaussian() * 0.02;
                let _ = ai.mob.random.next_gaussian() * 0.02;
                let _ = ai.mob.random.next_gaussian() * 0.02;
            }
        }
    }
    // `IronGolem.aiStep` 4-60, after `super.aiStep()`. `AbstractGolem`
    // does not override it; `SnowGolem`'s override is named unported in
    // `super::golem`.
    if let super::DataLayout::IronGolem = ai.mob.kind.data {
        super::golem::iron_golem_ai_step(&mut ai.mob);
    }
    // Chicken.aiStep 4-268, after `super.aiStep()`.
    if let super::DataLayout::Chicken = ai.mob.kind.data {
        super::chicken::ai_step_tail(&mut ai.mob);
    }
}

/// `LivingEntity.aiStep()`, offsets 0-877, server branches.
fn living_ai_step(ai: &mut MobAi<ServerMob>) {
    // 0-14.
    if ai.mob.no_jump_delay > 0 {
        ai.mob.no_jump_delay -= 1;
    }
    // 17-52: not interpolating; `canSimulateMovement()` is true on the
    // server for a mob, so the 0.98 scale is skipped. 55-81: lerpHeadSteps
    // is 0. 84-89: equipment tick — none.
    // 92-199: the small-velocity clamp, the non-player arm.
    let v = ai.mob.delta_movement();
    let mut x = v.x;
    let mut y = v.y;
    let mut z = v.z;
    if v.x.abs() < 0.003 {
        x = 0.0;
    }
    if v.z.abs() < 0.003 {
        z = 0.0;
    }
    if v.y.abs() < 0.003 {
        y = 0.0;
    }
    ai.mob.set_delta_movement(x, y, z);
    // 217: applyInput.
    ai.mob.xxa *= 0.98;
    ai.mob.zza *= 0.98;
    // 221-284.
    if ai.mob.is_dead_or_dying() {
        ai.mob.jumping = false;
        ai.mob.xxa = 0.0;
        ai.mob.zza = 0.0;
    } else if ai.mob.kind.is_mob()
        && ai.mob.data.get_byte(super::data::id::MOB_FLAGS) & super::host::MOB_FLAG_NO_AI == 0
    {
        // An armor stand's `serverAiStep` is `LivingEntity`'s empty one.
        // `isEffectiveAi()`: server side and `!isNoAi()`.
        let kind = ai.mob.kind;
        ai.server_ai_step_with(|inner| custom_server_ai_step(kind, inner));
    }
    // 301-479: jump.
    if ai.mob.jumping && ai.mob.is_affected_by_fluids() {
        let d9 = if ai.mob.is_in_lava() {
            ai.mob.fluid_height[1]
        } else {
            ai.mob.fluid_height[0]
        };
        let in_water = ai.mob.is_in_water() && d9 > 0.0;
        let threshold = ai.mob.fluid_jump_threshold();
        if in_water && (!ai.mob.on_ground() || d9 > threshold) {
            // jumpInLiquid(WATER): + 0.04 on y.
            let d = ai.mob.delta_movement();
            ai.mob
                .set_delta_movement(d.x, d.y + 0.039_999_999_105_930_33, d.z);
        } else if ai.mob.is_in_lava() && (!ai.mob.on_ground() || !is_in_shallow_lava(&ai.mob)) {
            let d = ai.mob.delta_movement();
            ai.mob
                .set_delta_movement(d.x, d.y + 0.039_999_999_105_930_33, d.z);
        } else if (ai.mob.on_ground() || in_water && d9 <= threshold) && ai.mob.no_jump_delay == 0 {
            jump_from_ground(&mut ai.mob);
            ai.mob.no_jump_delay = 10;
        }
    } else {
        ai.mob.no_jump_delay = 0;
    }
    // 496-506: fall flying. 507-535.
    let input = Vec3::new(
        f64::from(ai.mob.xxa),
        f64::from(ai.mob.yya),
        f64::from(ai.mob.zza),
    );
    // 537-560: slow falling / levitation reset fall distance — no effects.
    // 561-619: no controlling player; `canSimulateMovement && isEffectiveAi`.
    // `isEffectiveAi()`: `Mob`'s is `!isNoAi()`; `ArmorStand`'s is
    // `LivingEntity`'s (server side) `&& hasPhysics()`, and its `travel`
    // returns at once without physics as well.
    let effective = if ai.mob.kind.is_mob() {
        ai.mob.data.get_byte(super::data::id::MOB_FLAGS) & super::host::MOB_FLAG_NO_AI == 0
    } else {
        super::armor_stand::has_physics(&ai.mob)
    };
    if effective {
        travel(&mut ai.mob, input);
    }
    // 620-640: applyEffectsFromBlocks — inside-block effects, unported (see
    // the module docs). 641-658: calculateEntityAnimation — client.
    // 666-779: freezing.
    let frozen = ai.mob.data.get_varint(super::data::id::TICKS_FROZEN);
    if !ai.mob.is_in_powder_snow {
        // `setTicksFrozen(max(0, ticksFrozen - 2))`.
        ai.mob
            .data
            .set_varint(super::data::id::TICKS_FROZEN, (frozen - 2).max(0));
    }
    // 725: removeFrost — `getAttribute(MOVEMENT_SPEED)`: the instance is
    // created here, syncable, and that is the whole reason a fresh mob's
    // second tick carries an `update_attributes`.
    if ai.mob.attributes.instance(MOVEMENT_SPEED).is_some() {
        // No powder-snow modifier to remove.
    }
    // 729: tryAddFrost — `getBlockStateOnLegacy().isAir()` else with
    // ticksFrozen > 0 a transient speed modifier. Never frozen here.
    // 733-771: the freeze hit every 40 ticks — damage.
    // 789-816: autoSpinAttackTicks. 816: pushEntities.
    push_entities(&mut ai.mob);
    // 827-876: water sensitivity — neither kind.
}

/// `customServerAiStep(ServerLevel)` per kind, at `Mob.serverAiStep`'s
/// offsets 175-196.
fn custom_server_ai_step(kind: &'static super::MobKind, ai: &mut MobAi<ServerMob>) {
    match kind.data {
        super::DataLayout::Pig
        | super::DataLayout::Cow
        | super::DataLayout::Mooshroom
        | super::DataLayout::Sheep
        | super::DataLayout::Chicken => {
            // Animal.customServerAiStep 0-12, then AgeableMob's → Mob's
            // bare return. (`Sheep.customServerAiStep` first copies its
            // goal's `eatAnimationTick` into a field only the client's head
            // animation reads — see `crate::mob::sheep`.)
            if ai.mob.get_age() != 0 {
                ai.mob.in_love = 0;
            }
        }
        super::DataLayout::Bat => super::bat::custom_server_ai_step(ai),
        // `AbstractGolem` → `PathfinderMob` → `Mob.customServerAiStep`: the
        // bare return. `CopperGolem`'s override ticks its brain, which is not
        // ported (see `super::golem`).
        super::DataLayout::IronGolem
        | super::DataLayout::SnowGolem
        | super::DataLayout::CopperGolem => {}
        // Not reached: the caller skips `serverAiStep` for a non-`Mob`.
        super::DataLayout::ArmorStand => {}
    }
}

/// `isInShallowFluid(LAVA)`: `getFluidHeight(tag) < getFluidJumpThreshold()`.
fn is_in_shallow_lava(m: &ServerMob) -> bool {
    m.fluid_height[1] < m.fluid_jump_threshold()
}

/// `LivingEntity.jumpFromGround()`, offsets 0-94.
fn jump_from_ground(m: &mut ServerMob) {
    let power = jump_power(m);
    if power <= 1.0E-5_f32 {
        return;
    }
    let d = m.delta_movement();
    m.set_delta_movement(d.x, f64::from(power).max(d.y), d.z);
    // 40-88: sprinting — never.
    m.needs_sync = true;
}

/// `getJumpPower()` → `getJumpPower(1.0F)`:
/// `(float) getAttributeValue(JUMP_STRENGTH) * f * getBlockJumpFactor() + getJumpBoostPower()`.
fn jump_power(m: &ServerMob) -> f32 {
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let strength = m.attribute_value(super::host::JUMP_STRENGTH) as f32;
    strength * 1.0 * block_jump_factor(m) + 0.0
}

/// `Entity.getBlockJumpFactor()`: the block at the feet, or when that is
/// 1.0 the block below that affects movement.
fn block_jump_factor(m: &ServerMob) -> f32 {
    let here = m.block_position();
    let f = m.level.row(here).map_or(1.0, |r| r.jump_factor);
    let below = block_pos_below_that_affects_my_movement(m);
    let g = m.level.row(below).map_or(1.0, |r| r.jump_factor);
    if f64::from(f) == 1.0 {
        g
    } else {
        f
    }
}

/// `level.getBlockState(getBlockPosBelowThatAffectsMyMovement()).getBlock()
/// .getFriction()`.
pub(super) fn block_friction_below(m: &ServerMob) -> f32 {
    let below = block_pos_below_that_affects_my_movement(m);
    m.level.props.friction(m.level.state_id(below))
}

/// `Entity.getBlockPosBelowThatAffectsMyMovement()`: `getOnPos(0.500001F)`.
fn block_pos_below_that_affects_my_movement(m: &ServerMob) -> BlockPos {
    physics::block_pos_below_that_affects_my_movement(
        &m.body,
        |p| m.level.props.is_fence(m.level.state_id(p)),
        |p| m.level.props.is_wall_or_gate(m.level.state_id(p)),
    )
}

/// `LivingEntity.travel(Vec3)`: the fluid branch when in water or lava
/// (`shouldTravelInFluid`: `(isInWater() || isInLava()) && isAffectedByFluids()
/// && !canStandOnFluid(state)`), else `travelInAir`.
fn travel(m: &mut ServerMob, input: Vec3) {
    if (m.is_in_water() || m.is_in_lava()) && m.is_affected_by_fluids() {
        travel_in_fluid(m, input);
        return;
    }
    travel_in_air(m, input);
}

/// `LivingEntity.travelInFluid(Vec3)`, offsets 0-61: the falling flag and
/// the y before the move are taken first, then water or lava.
fn travel_in_fluid(m: &mut ServerMob, input: Vec3) {
    let is_falling = m.delta_movement().y <= 0.0;
    let y0 = m.y();
    let gravity = effective_gravity(m);
    if m.is_in_water() {
        travel_in_water(m, input, gravity, is_falling, y0);
        // `floatInWaterWhileRidden`: CAN_FLOAT_WHILE_RIDDEN and a vehicle —
        // neither kind carries a rider.
    } else {
        travel_in_lava(m, input, gravity, is_falling, y0);
    }
}

/// `travelInWater(Vec3, double gravity, boolean falling, double y0)`,
/// offsets 0-195. `getWaterSlowDown()` is `0.8F`; the mob is never
/// sprinting and has no Dolphin's Grace.
fn travel_in_water(m: &mut ServerMob, input: Vec3, gravity: f64, falling: bool, y0: f64) {
    let mut f7 = 0.8_f32;
    let mut f8 = 0.02_f32;
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let mut f9 = m.attribute_value("minecraft:water_movement_efficiency") as f32;
    if !m.on_ground() {
        f9 *= 0.5;
    }
    if f9 > 0.0 {
        f7 += (0.546_000_06 - f7) * f9;
        f8 += (m.speed - f8) * f9;
    }
    m.move_relative(f8, input);
    move_self(m);
    let d = m.delta_movement();
    // 124-160: a climbable under horizontal collision — nothing climbs.
    let d = Vec3::new(
        d.x * f64::from(f7),
        d.y * 0.800_000_011_920_929,
        d.z * f64::from(f7),
    );
    let adjusted = fluid_falling_adjusted_movement(gravity, falling, d);
    m.set_delta_movement(adjusted.x, adjusted.y, adjusted.z);
    jump_out_of_fluid(m, gravity, y0);
}

/// `travelInLava(Vec3, double, boolean, double)`, offsets 0-116.
fn travel_in_lava(m: &mut ServerMob, input: Vec3, gravity: f64, falling: bool, y0: f64) {
    m.move_relative(0.02, input);
    move_self(m);
    if is_in_shallow_lava(m) {
        let d = m.delta_movement();
        let d = Vec3::new(d.x * 0.5, d.y * 0.800_000_011_920_929, d.z * 0.5);
        let a = fluid_falling_adjusted_movement(gravity, falling, d);
        m.set_delta_movement(a.x, a.y, a.z);
    } else {
        let d = m.delta_movement().scale(0.5);
        m.set_delta_movement(d.x, d.y, d.z);
    }
    if gravity != 0.0 {
        let d = m.delta_movement();
        m.set_delta_movement(d.x, d.y + -gravity / 4.0, d.z);
    }
    jump_out_of_fluid(m, gravity, y0);
}

/// `LivingEntity.getFluidFallingAdjustedMovement(double gravity, boolean
/// falling, Vec3 v)`, offsets 0-100: with gravity and not sprinting,
/// `y - gravity / 16`, snapped to `-0.003` when a falling entity is
/// already at the terminal band.
fn fluid_falling_adjusted_movement(gravity: f64, falling: bool, v: Vec3) -> Vec3 {
    if gravity == 0.0 {
        return v;
    }
    let dy = if falling && (v.y - 0.005).abs() >= 0.003 && (v.y - gravity / 16.0).abs() < 0.003 {
        -0.003
    } else {
        v.y - gravity / 16.0
    };
    Vec3::new(v.x, dy, v.z)
}

/// `jumpOutOfFluid(double y0)`, offsets 0-57: against a wall with the box
/// free one block up (`isFree(dx, dy + 0.6 - y + y0, dz)`), lift at 0.3.
fn jump_out_of_fluid(m: &mut ServerMob, gravity: f64, y0: f64) {
    let _ = gravity;
    let v = m.delta_movement();
    if m.body.horizontal_collision
        && is_free(m, v.x, v.y + 0.600_000_023_841_857_9 - m.y() + y0, v.z)
    {
        m.set_delta_movement(v.x, 0.300_000_011_920_928_96, v.z);
    }
}

/// `Entity.isFree(double, double, double)` → `isFree(AABB)`:
/// `level.noCollision(this, box) && !level.containsAnyLiquid(box)`.
fn is_free(m: &ServerMob, dx: f64, dy: f64, dz: f64) -> bool {
    let bb = m.bounding_box().moved(dx, dy, dz);
    if !m.level.no_collision(m.id, &bb) {
        return false;
    }
    // `Level.containsAnyLiquid(AABB)`: every block position the box
    // touches, floor of the mins to ceil of the maxes exclusive.
    let (x0, y0, z0) = (floor(bb.min_x), floor(bb.min_y), floor(bb.min_z));
    let (x1, y1, z1) = (ceil(bb.max_x), ceil(bb.max_y), ceil(bb.max_z));
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                if m.level.has_fluid(BlockPos { x, y, z }) {
                    return false;
                }
            }
        }
    }
    true
}

/// `Entity.doWaterSplashEffect()`, run by `updateFluidInteraction` on the
/// tick a mob first touches water: the splash sound at a volume from the
/// speed, then the bubble and splash particle loops — `Level.addParticle`
/// is a no-op on a `ServerLevel`, but every draw the loops make comes off
/// the mob's random and is made here in the jar's order: two `nextFloat`
/// for the pitch, then per bubble three `nextDouble`, then per splash two.
fn water_splash_effect(m: &mut ServerMob) {
    // 0-26: no controlling passenger → `this`, factor 0.2F.
    let v = m.delta_movement();
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let speed =
        (v.x * v.x * 0.200_000_002_980_232_24 + v.y * v.y + v.z * v.z * 0.200_000_002_980_232_24)
            .sqrt() as f32;
    let f4 = (speed * 0.2).min(1.0);
    let a = m.random.next_f32();
    let b = m.random.next_f32();
    // Both branches are GENERIC_SPLASH; the split is the volume threshold.
    let _ = f4 < 0.25;
    play_sound(
        m,
        "minecraft:entity.generic.splash",
        f4,
        1.0 + (a - b) * 0.4,
    );
    let width = m.dimensions().0;
    let mut i = 0;
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    while (i as f32) < 1.0 + width * 20.0 {
        let _ = (m.random.next_f64() * 2.0 - 1.0) * f64::from(width);
        let _ = (m.random.next_f64() * 2.0 - 1.0) * f64::from(width);
        let _ = m.random.next_f64() * 0.200_000_002_980_232_24;
        i += 1;
    }
    let mut i = 0;
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    while (i as f32) < 1.0 + width * 20.0 {
        let _ = (m.random.next_f64() * 2.0 - 1.0) * f64::from(width);
        let _ = (m.random.next_f64() * 2.0 - 1.0) * f64::from(width);
        i += 1;
    }
    // 435: gameEvent(SPLASH) — no packet.
}

/// `LivingEntity.travelInAir(Vec3)`, offsets 0-263.
fn travel_in_air(m: &mut ServerMob, input: Vec3) {
    let below = block_pos_below_that_affects_my_movement(m);
    let friction = if m.on_ground() {
        let block_friction = m.level.props.friction(m.level.state_id(below));
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
        let modifier = m.attribute_value(super::host::FRICTION_MODIFIER) as f32;
        compute_modified_friction(block_friction, modifier)
    } else {
        1.0
    };
    let moved = handle_relative_friction_and_calculate_movement(m, input, friction);
    let mut dy = moved.y;
    // 57-101: levitation — no effect. 101-161: on the server, gravity
    // applies only while `level.hasChunkAt(below)`; in an unloaded chunk
    // the entity sinks at -0.1 above the world's floor and stops at it.
    if m.level.is_chunk_loaded(below.x >> 4, below.z >> 4) {
        dy -= effective_gravity(m);
    } else if m.y() > f64::from(m.level.min_y()) {
        dy = -0.1;
    } else {
        dy = 0.0;
    }
    // 161-187: shouldDiscardFriction.
    if m.discard_friction {
        m.set_delta_movement(moved.x, dy, moved.z);
        return;
    }
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let air_drag_modifier = m.attribute_value("minecraft:air_drag_modifier") as f32;
    let f9 = compute_modified_friction(0.91, air_drag_modifier);
    let f10 = friction * f9;
    // `omnidirectionalAirMover()` is false for both kinds.
    let f11 = compute_modified_friction(0.98, air_drag_modifier);
    m.set_delta_movement(
        moved.x * f64::from(f10),
        dy * f64::from(f11),
        moved.z * f64::from(f10),
    );
}

/// `LivingEntity.computeModifiedFriction(float, float)`:
/// `Mth.clamp(1 - (1 - friction) * modifier, 0, 1)`.
fn compute_modified_friction(friction: f32, modifier: f32) -> f32 {
    (1.0 - (1.0 - friction) * modifier).clamp(0.0, 1.0)
}

/// `LivingEntity.getEffectiveGravity()`: `getGravity()`, halved by slow
/// falling when descending — no effect here.
fn effective_gravity(m: &ServerMob) -> f64 {
    if m.data.get_bool(super::data::id::NO_GRAVITY) {
        0.0
    } else {
        m.attribute_value("minecraft:gravity")
    }
}

/// `handleRelativeFrictionAndCalculateMovement(Vec3, float)`, offsets 0-93.
fn handle_relative_friction_and_calculate_movement(
    m: &mut ServerMob,
    input: Vec3,
    friction: f32,
) -> Vec3 {
    let speed = friction_influenced_speed(m, friction);
    m.move_relative(speed, input);
    // handleOnClimbable: not on a climbable.
    move_self(m);
    let d = m.delta_movement();
    // 38-92: the climbing / powder-snow lift when colliding or jumping:
    // `onClimbable() || (wasInPowderSnow && canEntityWalkOnPowderSnow)`,
    // and nothing here climbs.
    if (m.body.horizontal_collision || m.jumping)
        && m.was_in_powder_snow
        && m.is_powder_snow_walkable()
    {
        return Vec3::new(d.x, 0.2, d.z);
    }
    d
}

/// `getFrictionInfluencedSpeed(float)`: on the ground
/// `getSpeed() * (0.21600002F / (f * f * f))`, else `getFlyingSpeed()`
/// (`0.02F` without a controlling player).
fn friction_influenced_speed(m: &ServerMob, friction: f32) -> f32 {
    if m.on_ground() {
        if f64::from(friction) > 0.6 {
            m.speed * (0.216_000_02 / (friction * friction * friction))
        } else {
            m.speed
        }
    } else {
        0.02
    }
}

/// `Entity.move(MoverType.SELF, getDeltaMovement())` for a living mob:
/// `Entity.move`'s body in the jar's order, with the two limbs the
/// item/player port in `crate::physics::move_entity` does not run —
/// `checkFallDamage` (offsets 508-546) and the movement emission
/// (618-648) — in their places.
pub fn move_self(m: &mut ServerMob) {
    let vec = m.body.movement;
    let profile = m.profile();
    let level = m.level;
    // 130: stuckSpeedMultiplier is zero. 143: maybeBackOffFromEdge is the
    // identity for a mob. 149: collide.
    let box_ = profile.bounding_box(m.body.pos);
    let moved = match crate::collide::collide(
        level.shapes,
        &level,
        vec,
        box_,
        profile.max_up_step,
        m.body.on_ground,
    ) {
        Ok(v) => v,
        // A dynamic-shape block in the sweep: refused rather than guessed,
        // and the mob stands still this tick.
        Err(_) => return,
    };
    let moved_len = moved.length_sqr();
    if moved_len > 1.0E-7 || vec.length_sqr() - moved_len < 1.0E-7 {
        m.body.pos = CVec3::new(
            m.body.pos.x + moved.x,
            m.body.pos.y + moved.y,
            m.body.pos.z + moved.z,
        );
    }
    let x_collided = !physics::mth_equal(vec.x, moved.x);
    let z_collided = !physics::mth_equal(vec.z, moved.z);
    m.body.horizontal_collision = x_collided || z_collided;
    let moved_vertically = vec.y.abs() > 0.0;
    m.body.vertical_collision = vec.y != moved.y;
    m.body.vertical_collision_below = m.body.vertical_collision && vec.y < 0.0;
    let on_ground = m.body.vertical_collision_below;
    m.body.on_ground = on_ground;
    if physics::check_supporting_block(
        level.shapes,
        &level,
        &profile,
        &mut m.body,
        on_ground,
        Some(moved),
    )
    .is_err()
    {
        return;
    }
    // 508-546: `getOnPosLegacy()`, its state, `checkFallDamage(moved.y,
    // onGround, state, pos)` — `isLocalInstanceAuthoritative` is true.
    let legacy = physics::on_pos_legacy(
        &m.body,
        |p| level.props.is_fence(level.state_id(p)),
        |p| level.props.is_wall_or_gate(level.state_id(p)),
    );
    let support_state = level.state_id(legacy);
    check_fall_damage(m, moved.y, on_ground, support_state);
    if m.removed {
        return;
    }
    // 563-598: restitution, `canSimulateMovement`.
    if (moved_vertically && m.body.vertical_collision) || m.body.horizontal_collision {
        let gravity = effective_gravity(m);
        physics::restitute_movement_after_collisions(
            &mut m.body,
            &profile,
            level
                .props
                .bounce_restitution_for_entity(support_state, true),
            false,
            gravity,
            x_collided,
            z_collided,
            moved,
        );
    }
    // 618-648: movement emission and the step sound, `isLocalInstanceAuthoritative`.
    if m.kind.emits_sounds {
        movement_emission(m, moved, legacy, support_state);
    }
    // 651-: `getBlockSpeedFactor()` — `LivingEntity`'s lerps `Entity`'s by
    // MOVEMENT_EFFICIENCY: `Mth.lerp(eff, super, 1.0F)`.
    let base = physics::block_speed_factor(level.props, &level, &m.body);
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let eff = m.attribute_value(MOVEMENT_EFFICIENCY) as f32;
    let f = base + eff * (1.0 - base);
    m.body.movement = CVec3::new(
        m.body.movement.x * f64::from(f),
        m.body.movement.y,
        m.body.movement.z * f64::from(f),
    );
}

/// `LivingEntity.checkFallDamage` over `Entity.checkFallDamage`.
fn check_fall_damage(m: &mut ServerMob, dy: f64, on_ground: bool, support_state: u32) {
    // LivingEntity 0-11: `if (!isInWater()) updateFluidInteraction()`.
    if !m.is_in_water() {
        update_fluid_interaction(m);
    }
    // 33-285: on the ground with a fall distance: `onChangedBlock`, the
    // landing particles (`calculateFallPower`), then `super`. The
    // particles are a `level_particles` broadcast from a fall of more
    // than a few blocks; not sent: a `Level.sendParticles` port belongs
    // with the damage the same landing deals.
    // Entity 0-24: accumulate.
    if !m.is_in_water() && dy < 0.0 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vanilla's own d2f then f2d"
        )]
        let step = f64::from(dy as f32);
        m.fall_distance -= step;
    }
    // 25-102: landing — `Block.fallOn` (farmland, turtle eggs: block
    // writes a superflat's grass never makes), the HIT_GROUND event, then
    // `resetFallDistance`.
    if on_ground {
        let _ = support_state;
        m.fall_distance = 0.0;
    }
}

/// `Entity.applyMovementEmissionAndPlaySound(emission, movement, onPos,
/// onState)` for `MovementEmission.ALL`: the walk cadence and, when it
/// fires, the kind's own `playStepSound`.
///
/// `vibrationAndSoundEffectsFromBlock` fires on the ground (never
/// climbing, crouching or on rails here) and `playStepSound` is `Pig`'s:
/// the sound set's step sound at `0.15F, 1.0F`.
fn movement_emission(m: &mut ServerMob, moved: CVec3, on_pos: BlockPos, on_state: u32) {
    let solid_below = m.level.facts.row(on_state).is_some_and(|r| !r.air);
    let _ = on_pos;
    // 73-81 and 211-220: `flyDist`, and a chicken's flap off an air block.
    if let super::DataLayout::Chicken = m.kind.data {
        let length = (moved.x * moved.x + moved.y * moved.y + moved.z * moved.z).sqrt();
        super::chicken::movement_emission(m, length, !solid_below);
    }
    if m.walk
        .advance(moved.x, moved.z, m.body.on_ground, solid_below)
    {
        if let Some((step, volume, pitch)) = super::step_sound(m) {
            play_sound(m, step, volume, pitch);
        }
    }
}

/// `LivingEntity.pushEntities()`, offsets 0-191, and `Bat`'s empty
/// override.
fn push_entities(m: &mut ServerMob) {
    if !m.kind.pushable {
        return;
    }
    let bb = m.bounding_box();
    let others = super::pushable_entities(m.id, &bb);
    if others.is_empty() {
        return;
    }
    // 40-156: `maxEntityCramming` — the hit at 24+ is damage; the
    // `nextInt(4)` draw that gates it is made when the crowd is over the
    // limit, which two entities never are.
    let cramming = crate::gamerules::GameRules::shared()
        .get("max_entity_cramming")
        .and_then(|v| v.serialize().parse::<i32>().ok())
        .unwrap_or(24);
    if cramming > 0 && others.len() > usize::try_from(cramming - 1).unwrap_or(0) {
        let _ = m.random.next_i32_bound(4);
    }
    for other in others {
        // `IronGolem.doPush` runs its retarget draw before `super.doPush`.
        if let super::DataLayout::IronGolem = m.kind.data {
            super::golem::iron_golem_do_push(m, other);
        }
        // `doPush(other)` → `other.push(this)`: `Entity.push(Entity)` run
        // with `this = other`.
        super::push_pair(other, m);
    }
}

/// `Entity.push(Entity)` from the pushed entity's point of view: `self`
/// is the entity whose `push(other)` is called, `other` the mob doing
/// the pushing. Both sides move (`push(dx, 0, dz)` each), gated by
/// `isPushable`.
///
/// Offsets 24-169: horizontal offset, `absMax >= 0.01`, normalise by the
/// square root, cap the inverse at 1, scale by 0.05, then each side's
/// `push`.
pub fn push_math(self_x: f64, self_z: f64, other_x: f64, other_z: f64) -> Option<(f64, f64)> {
    let mut dx = other_x - self_x;
    let mut dz = other_z - self_z;
    let mut d = abs_max(dx, dz);
    if d < 0.009_999_999_776_482_582 {
        return None;
    }
    d = d.sqrt();
    dx /= d;
    dz /= d;
    let mut inv = 1.0 / d;
    if inv > 1.0 {
        inv = 1.0;
    }
    dx *= inv;
    dz *= inv;
    dx *= 0.050_000_000_745_058_06;
    dz *= 0.050_000_000_745_058_06;
    Some((dx, dz))
}

/// `Mth.absMax(double, double)`.
fn abs_max(mut a: f64, mut b: f64) -> f64 {
    if a < 0.0 {
        a = -a;
    }
    if b < 0.0 {
        b = -b;
    }
    if a > b {
        a
    } else {
        b
    }
}

/// `Entity.push(double, double, double)`: add to the delta movement and
/// set `needsSync`, when finite.
pub fn push_by(m: &mut ServerMob, dx: f64, dy: f64, dz: f64) {
    if dx.is_finite() && dy.is_finite() && dz.is_finite() {
        let d = m.delta_movement();
        m.set_delta_movement(d.x + dx, d.y + dy, d.z + dz);
        m.needs_sync = true;
    }
}

/// `LivingEntity.isPushable()`: alive, not a spectator, not climbing;
/// `Bat`'s is false.
pub fn is_pushable(m: &ServerMob) -> bool {
    m.kind.pushable && m.is_alive()
}

impl ServerMob {
    /// `blockPosition()`.
    #[must_use]
    pub fn block_position(&self) -> BlockPos {
        rustmc_entity::phys::block_pos_containing(self.x(), self.y(), self.z())
    }
}

/// The level, so `MobLevel` can be handed to the collision functions by
/// reference where they take `&W: CollisionBlocks`.
impl MobLevel {
    /// `getFluidState(pos).isEmpty()` negated.
    #[must_use]
    pub fn has_fluid(&self, pos: BlockPos) -> bool {
        self.row(pos).is_some_and(|r| r.fluid_kind != 0)
    }
}
