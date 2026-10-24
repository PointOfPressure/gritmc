//! `net.minecraft.world.entity.ai.control.{Control, LookControl, MoveControl,
//! JumpControl, BodyRotationControl}`, from the 26.2 jar.
//!
//! Each control in the jar holds `Mob mob` and writes the mob's rotation
//! and movement fields directly. Here each takes the mob as a parameter;
//! the fields it writes are the same ones, through [`MobHost`].
//!
//! # Order of the RNG and of the writes
//!
//! None of the four controls draws from the mob's random. Their observable
//! is the sequence of writes to `yRot`, `xRot`, `yHeadRot`, `yBodyRot`,
//! `speed`, `zza`, `xxa` and `jumping`, and that sequence is the bytecode's.
//! `Mob.serverAiStep` ticks them move, look, jump, after the navigation.

use rustmc_data::BlockPos;

use super::navigation::GroundPathNavigation;
use crate::host::{MobHost, PathLevel};
use crate::mth::{
    atan2, clamp_f32, cos, degrees_difference, java_max_f32, rotate_if_necessary, sin, sqrt_f32,
    wrap_degrees,
};
use crate::pathfinder::PathType;
use crate::phys::block_pos_containing;

/// `Control.rotateTowards(float current, float target, float maxDelta)`:
///
/// ```text
/// d = Mth.degreesDifference(current, target)
/// c = Mth.clamp(d, -maxDelta, maxDelta)
/// return current + c
/// ```
#[must_use]
pub fn rotate_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    let d = degrees_difference(current, target);
    let c = clamp_f32(d, -max_delta, max_delta);
    current + c
}

/// `180 / PI` as the jar's double literal `57.2957763671875` — which is
/// `(double) (180.0f / (float) Math.PI)`, a float widened, not
/// `180.0 / Math.PI`.
pub const RAD_TO_DEG: f64 = 57.295_776_367_187_5;

/// `LookControl`.
#[derive(Debug, Clone, Default)]
pub struct LookControl {
    /// `yMaxRotSpeed`, `xMaxRotAngle`.
    y_max_rot_speed: f32,
    x_max_rot_angle: f32,
    /// `lookAtCooldown`.
    look_at_cooldown: i32,
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
}

impl LookControl {
    /// `LookControl(Mob)`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `setLookAt(double, double, double)`: with the mob's
    /// `getHeadRotSpeed()` and `getMaxHeadXRot()`.
    pub fn set_look_at<M: MobHost>(&mut self, mob: &M, x: f64, y: f64, z: f64) {
        #[allow(clippy::cast_precision_loss)]
        self.set_look_at_with(
            x,
            y,
            z,
            mob.head_rot_speed() as f32,
            mob.max_head_x_rot() as f32,
        );
    }

    /// `setLookAt(Entity)`: `setLookAt(e.getX(), e.getEyeY(), e.getZ())`.
    pub fn set_look_at_entity<M: MobHost>(&mut self, mob: &M, x: f64, eye_y: f64, z: f64) {
        self.set_look_at(mob, x, eye_y, z);
    }

    /// `setLookAt(double, double, double, float, float)`: store the target
    /// and the limits, `lookAtCooldown = 2`.
    pub fn set_look_at_with(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        y_max_rot_speed: f32,
        x_max_rot_angle: f32,
    ) {
        self.wanted_x = x;
        self.wanted_y = y;
        self.wanted_z = z;
        self.y_max_rot_speed = y_max_rot_speed;
        self.x_max_rot_angle = x_max_rot_angle;
        self.look_at_cooldown = 2;
    }

    /// `tick()`, offsets 0..92. `nav_done` is `mob.getNavigation().isDone()`
    /// for `clampHeadRotationToBody`.
    pub fn tick<M: MobHost>(&mut self, mob: &mut M, nav_done: bool) {
        if self.reset_x_rot_on_tick() {
            mob.set_x_rot(0.0);
        }
        if self.look_at_cooldown > 0 {
            self.look_at_cooldown -= 1;
            if let Some(y) = self.get_y_rot_d(mob) {
                let r = rotate_towards(mob.y_head_rot(), y, self.y_max_rot_speed);
                mob.set_y_head_rot(r);
            }
            if let Some(x) = self.get_x_rot_d(mob) {
                let r = rotate_towards(mob.x_rot(), x, self.x_max_rot_angle);
                mob.set_x_rot(r);
            }
        } else {
            let r = rotate_towards(mob.y_head_rot(), mob.y_body_rot(), 10.0);
            mob.set_y_head_rot(r);
        }
        self.clamp_head_rotation_to_body(mob, nav_done);
    }

    /// `clampHeadRotationToBody()`: only while the navigation is not done.
    fn clamp_head_rotation_to_body<M: MobHost>(&self, mob: &mut M, nav_done: bool) {
        if !nav_done {
            #[allow(clippy::cast_precision_loss)]
            let r = rotate_if_necessary(
                mob.y_head_rot(),
                mob.y_body_rot(),
                mob.max_head_y_rot() as f32,
            );
            mob.set_y_head_rot(r);
        }
    }

    /// `resetXRotOnTick()`: `iconst_1`.
    fn reset_x_rot_on_tick(&self) -> bool {
        true
    }

    /// `isLookingAtTarget()`: `lookAtCooldown > 0`.
    #[must_use]
    pub fn is_looking_at_target(&self) -> bool {
        self.look_at_cooldown > 0
    }

    /// `getWantedX()`, `getWantedY()`, `getWantedZ()`.
    #[must_use]
    pub fn wanted(&self) -> (f64, f64, f64) {
        (self.wanted_x, self.wanted_y, self.wanted_z)
    }

    /// `getXRotD()`: pitch toward the wanted point, or empty when both the
    /// vertical and the horizontal offsets are within `1.0E-5f`.
    ///
    /// `(float) (-(Mth.atan2(dy, h) * 57.2957763671875))`.
    fn get_x_rot_d<M: MobHost>(&self, mob: &M) -> Option<f32> {
        let dx = self.wanted_x - mob.x();
        let dy = self.wanted_y - mob.eye_y();
        let dz = self.wanted_z - mob.z();
        let h = (dx * dx + dz * dz).sqrt();
        if dy.abs() > 9.999_999_747_378_752E-6 || h.abs() > 9.999_999_747_378_752E-6 {
            Some((-(atan2(dy, h) * RAD_TO_DEG)) as f32)
        } else {
            None
        }
    }

    /// `getYRotD()`: `(float) (Mth.atan2(dz, dx) * 57.2957763671875) - 90.0f`.
    fn get_y_rot_d<M: MobHost>(&self, mob: &M) -> Option<f32> {
        let dx = self.wanted_x - mob.x();
        let dz = self.wanted_z - mob.z();
        if dz.abs() > 9.999_999_747_378_752E-6 || dx.abs() > 9.999_999_747_378_752E-6 {
            Some((atan2(dz, dx) * RAD_TO_DEG) as f32 - 90.0)
        } else {
            None
        }
    }
}

/// `MoveControl.Operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Wait,
    MoveTo,
    Strafe,
    Jumping,
}

/// `MoveControl`.
#[derive(Debug, Clone)]
pub struct MoveControl {
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
    speed_modifier: f64,
    strafe_forwards: f32,
    strafe_right: f32,
    operation: Operation,
}

impl Default for MoveControl {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveControl {
    /// `MIN_SPEED`, `MIN_SPEED_SQR`, `MAX_TURN`.
    pub const MIN_SPEED: f32 = 5.0E-4;
    pub const MIN_SPEED_SQR: f32 = 2.500_000_3E-7;
    pub const MAX_TURN: i32 = 90;

    /// `MoveControl(Mob)`: `operation = WAIT`.
    #[must_use]
    pub fn new() -> Self {
        MoveControl {
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier: 0.0,
            strafe_forwards: 0.0,
            strafe_right: 0.0,
            operation: Operation::Wait,
        }
    }

    /// `hasWanted()`: `operation == MOVE_TO`.
    #[must_use]
    pub fn has_wanted(&self) -> bool {
        self.operation == Operation::MoveTo
    }

    /// `getSpeedModifier()`.
    #[must_use]
    pub fn speed_modifier(&self) -> f64 {
        self.speed_modifier
    }

    /// `setWantedPosition(double, double, double, double)`: store, and
    /// `operation = MOVE_TO` unless currently `JUMPING`.
    pub fn set_wanted_position(&mut self, x: f64, y: f64, z: f64, speed: f64) {
        self.wanted_x = x;
        self.wanted_y = y;
        self.wanted_z = z;
        self.speed_modifier = speed;
        if self.operation != Operation::Jumping {
            self.operation = Operation::MoveTo;
        }
    }

    /// `strafe(float, float)`: `operation = STRAFE`, `speedModifier = 0.25`.
    pub fn strafe(&mut self, forwards: f32, right: f32) {
        self.operation = Operation::Strafe;
        self.strafe_forwards = forwards;
        self.strafe_right = right;
        self.speed_modifier = 0.25;
    }

    /// `tick()`, offsets 0..583.
    pub fn tick<M: MobHost>(
        &mut self,
        mob: &mut M,
        nav: &GroundPathNavigation,
        jump: &mut JumpControl,
    ) {
        match self.operation {
            Operation::Strafe => {
                let speed_attr = mob.movement_speed() as f32;
                let speed = self.speed_modifier as f32 * speed_attr;
                let mut f = self.strafe_forwards;
                let mut r = self.strafe_right;
                let mut len = sqrt_f32(f * f + r * r);
                if len < 1.0 {
                    len = 1.0;
                }
                len = speed / len;
                f *= len;
                r *= len;
                let s = sin(f64::from(mob.y_rot() * 0.017_453_292));
                let c = cos(f64::from(mob.y_rot() * 0.017_453_292));
                let fx = f * c - r * s;
                let fz = r * c + f * s;
                if !Self::is_walkable(mob, nav, fx, fz) {
                    self.strafe_forwards = 1.0;
                    self.strafe_right = 0.0;
                }
                mob.set_speed_and_zza(speed);
                mob.set_zza(self.strafe_forwards);
                mob.set_xxa(self.strafe_right);
                self.operation = Operation::Wait;
            }
            Operation::MoveTo => {
                self.operation = Operation::Wait;
                let dx = self.wanted_x - mob.x();
                let dz = self.wanted_z - mob.z();
                let dy = self.wanted_y - mob.y();
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 < 2.500_000_277_905_201E-7 {
                    mob.set_zza(0.0);
                    return;
                }
                let yaw = (atan2(dz, dx) * RAD_TO_DEG) as f32 - 90.0;
                let r = Self::rotlerp(mob.y_rot(), yaw, 90.0);
                mob.set_y_rot(r);
                mob.set_speed_and_zza((self.speed_modifier * mob.movement_speed()) as f32);
                let pos = mob.block_position();
                let facts = mob.level().block_facts(pos);
                let shape_max_y = mob.level().collision_shape_max_y(pos);
                let jump_for_step = dy > f64::from(mob.max_up_step())
                    && dx * dx + dz * dz < f64::from(java_max_f32(1.0, mob.bb_width()));
                let jump_for_shape = match shape_max_y {
                    None => false,
                    Some(top) => {
                        mob.y() < top + f64::from(pos.y) && !facts.is_door_tag && !facts.is_fence
                    }
                };
                if jump_for_step || jump_for_shape {
                    jump.jump();
                    self.operation = Operation::Jumping;
                }
            }
            Operation::Jumping => {
                mob.set_speed_and_zza((self.speed_modifier * mob.movement_speed()) as f32);
                if mob.on_ground() || (mob.is_in_liquid() && mob.is_affected_by_fluids()) {
                    self.operation = Operation::Wait;
                }
            }
            Operation::Wait => {
                mob.set_zza(0.0);
            }
        }
    }

    /// `isWalkable(float, float)`: the navigation's evaluator classifies
    /// `(mob.x + fx, mob.blockY, mob.z + fz)`; anything but `WALKABLE`
    /// refuses.
    fn is_walkable<M: MobHost>(mob: &M, nav: &GroundPathNavigation, fx: f32, fz: f32) -> bool {
        let pos = block_pos_containing(
            mob.x() + f64::from(fx),
            f64::from(mob.block_y()),
            mob.z() + f64::from(fz),
        );
        nav.path_type_at(mob, pos) == PathType::Walkable
    }

    /// `MoveControl.rotlerp(float, float, float)`, offsets 0..70: clamp the
    /// wrapped difference, add, then fold into `[0, 360]` — **not**
    /// `Mob.rotlerp`, which does not fold.
    #[must_use]
    pub fn rotlerp(current: f32, target: f32, max: f32) -> f32 {
        let mut d = wrap_degrees(target - current);
        if d > max {
            d = max;
        }
        if d < -max {
            d = -max;
        }
        let mut r = current + d;
        if r < 0.0 {
            r += 360.0;
        } else if r > 360.0 {
            r -= 360.0;
        }
        r
    }

    /// `getWantedX()`, `getWantedY()`, `getWantedZ()`.
    #[must_use]
    pub fn wanted(&self) -> (f64, f64, f64) {
        (self.wanted_x, self.wanted_y, self.wanted_z)
    }

    /// `setWait()`.
    pub fn set_wait(&mut self) {
        self.operation = Operation::Wait;
    }

    /// The current operation, for tests.
    #[must_use]
    pub fn operation(&self) -> Operation {
        self.operation
    }
}

/// `JumpControl`.
#[derive(Debug, Clone, Default)]
pub struct JumpControl {
    jump: bool,
}

impl JumpControl {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `jump()`: `jump = true`.
    pub fn jump(&mut self) {
        self.jump = true;
    }

    /// The `jump` field as it stands before `tick` consumes it, for tests.
    #[must_use]
    pub fn wants_jump(&self) -> bool {
        self.jump
    }

    /// `tick()`: `mob.setJumping(jump); jump = false`.
    pub fn tick<M: MobHost>(&mut self, mob: &mut M) {
        mob.set_jumping(self.jump);
        self.jump = false;
    }
}

/// `BodyRotationControl`.
#[derive(Debug, Clone, Default)]
pub struct BodyRotationControl {
    /// `headStableTime`.
    head_stable_time: i32,
    /// `lastStableYHeadRot`.
    last_stable_y_head_rot: f32,
}

impl BodyRotationControl {
    /// `HEAD_STABLE_ANGLE`, `DELAY_UNTIL_STARTING_TO_FACE_FORWARD`,
    /// `HOW_LONG_IT_TAKES_TO_FACE_FORWARD`.
    pub const HEAD_STABLE_ANGLE: i32 = 15;
    pub const DELAY_UNTIL_STARTING_TO_FACE_FORWARD: i32 = 10;
    pub const HOW_LONG_IT_TAKES_TO_FACE_FORWARD: i32 = 10;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `clientTick()`, offsets 0..116.
    pub fn client_tick<M: MobHost>(&mut self, mob: &mut M) {
        if Self::is_moving(mob) {
            mob.set_y_body_rot(mob.y_rot());
            Self::rotate_head_if_necessary(mob);
            self.last_stable_y_head_rot = mob.y_head_rot();
            self.head_stable_time = 0;
        } else if Self::not_carrying_mob_passengers(mob) {
            if (mob.y_head_rot() - self.last_stable_y_head_rot).abs() > 15.0 {
                self.head_stable_time = 0;
                self.last_stable_y_head_rot = mob.y_head_rot();
                Self::rotate_body_if_necessary(mob);
            } else {
                self.head_stable_time += 1;
                if self.head_stable_time > Self::DELAY_UNTIL_STARTING_TO_FACE_FORWARD {
                    self.rotate_head_towards_front(mob);
                }
            }
        }
    }

    /// `rotateBodyIfNecessary()`.
    fn rotate_body_if_necessary<M: MobHost>(mob: &mut M) {
        #[allow(clippy::cast_precision_loss)]
        let r = rotate_if_necessary(
            mob.y_body_rot(),
            mob.y_head_rot(),
            mob.max_head_y_rot() as f32,
        );
        mob.set_y_body_rot(r);
    }

    /// `rotateHeadIfNecessary()`.
    fn rotate_head_if_necessary<M: MobHost>(mob: &mut M) {
        #[allow(clippy::cast_precision_loss)]
        let r = rotate_if_necessary(
            mob.y_head_rot(),
            mob.y_body_rot(),
            mob.max_head_y_rot() as f32,
        );
        mob.set_y_head_rot(r);
    }

    /// `rotateHeadTowardsFront()`:
    ///
    /// ```text
    /// n = headStableTime - 10
    /// f = clamp((float) n / 10.0f, 0, 1)
    /// g = (float) maxHeadYRot * (1 - f)
    /// yBodyRot = rotateIfNecessary(yBodyRot, yHeadRot, g)
    /// ```
    fn rotate_head_towards_front<M: MobHost>(&self, mob: &mut M) {
        let n = self.head_stable_time - Self::DELAY_UNTIL_STARTING_TO_FACE_FORWARD;
        #[allow(clippy::cast_precision_loss)]
        let f = clamp_f32(n as f32 / 10.0, 0.0, 1.0);
        #[allow(clippy::cast_precision_loss)]
        let g = mob.max_head_y_rot() as f32 * (1.0 - f);
        let r = rotate_if_necessary(mob.y_body_rot(), mob.y_head_rot(), g);
        mob.set_y_body_rot(r);
    }

    /// `notCarryingMobPassengers()`: `!(getFirstPassenger() instanceof Mob)`.
    fn not_carrying_mob_passengers<M: MobHost>(mob: &M) -> bool {
        !mob.first_passenger_is_mob()
    }

    /// `isMoving()`: `dx*dx + dz*dz > 2.500000277905201E-7` with
    /// `d = pos - old pos`.
    fn is_moving<M: MobHost>(mob: &M) -> bool {
        let dx = mob.x() - mob.xo();
        let dz = mob.z() - mob.zo();
        dx * dx + dz * dz > 2.500_000_277_905_201E-7
    }
}

/// `Mob.rotlerp(float current, float target, float max)`, offsets 0..34:
/// the wrapped difference clamped to `±max`, added — no fold into
/// `[0, 360]`, unlike `MoveControl.rotlerp`.
#[must_use]
pub fn mob_rotlerp(current: f32, target: f32, max: f32) -> f32 {
    let mut d = wrap_degrees(target - current);
    if d > max {
        d = max;
    }
    if d < -max {
        d = -max;
    }
    current + d
}

/// `Mob.lookAt(Entity, float maxYaw, float maxPitch)`, offsets 0..155:
/// the target's eye y for a living entity, its box's mid-height otherwise.
pub fn mob_look_at<M: MobHost>(
    mob: &mut M,
    target_x: f64,
    target_y_for_pitch: f64,
    target_z: f64,
    max_yaw: f32,
    max_pitch: f32,
) {
    let dx = target_x - mob.x();
    let dz = target_z - mob.z();
    let dy = target_y_for_pitch - mob.eye_y();
    let h = (dx * dx + dz * dz).sqrt();
    let yaw = (atan2(dz, dx) * RAD_TO_DEG) as f32 - 90.0;
    let pitch = (-(atan2(dy, h) * RAD_TO_DEG)) as f32;
    let xr = mob_rotlerp(mob.x_rot(), pitch, max_pitch);
    mob.set_x_rot(xr);
    let yr = mob_rotlerp(mob.y_rot(), yaw, max_yaw);
    mob.set_y_rot(yr);
}

/// A `BlockPos` helper the controls share.
#[must_use]
pub fn block_pos_of(x: f64, y: f64, z: f64) -> BlockPos {
    block_pos_containing(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_towards_clamps_the_step() {
        assert_eq!(rotate_towards(0.0, 90.0, 10.0), 10.0);
        assert_eq!(rotate_towards(0.0, -90.0, 10.0), -10.0);
        assert_eq!(rotate_towards(170.0, -170.0, 30.0), 190.0);
        assert_eq!(rotate_towards(5.0, 8.0, 10.0), 8.0);
    }

    /// The two rotlerps differ only in the fold.
    #[test]
    fn move_control_rotlerp_folds_and_mob_rotlerp_does_not() {
        assert_eq!(MoveControl::rotlerp(350.0, 20.0, 90.0), 20.0);
        assert_eq!(mob_rotlerp(350.0, 20.0, 90.0), 380.0);
        assert_eq!(MoveControl::rotlerp(10.0, -30.0, 90.0), 330.0);
        assert_eq!(mob_rotlerp(10.0, -30.0, 90.0), -30.0);
        // wrapDegrees(180) is -180, so a half turn is always taken the
        // negative way, and the fold lands it at 270.
        assert_eq!(MoveControl::rotlerp(0.0, 180.0, 90.0), 270.0);
        assert_eq!(MoveControl::rotlerp(0.0, -180.0, 90.0), 270.0);
        assert_eq!(mob_rotlerp(0.0, 180.0, 90.0), -90.0);
    }

    #[test]
    fn set_wanted_position_does_not_interrupt_a_jump() {
        let mut mc = MoveControl::new();
        mc.set_wanted_position(1.0, 2.0, 3.0, 1.5);
        assert!(mc.has_wanted());
        mc.operation = Operation::Jumping;
        mc.set_wanted_position(4.0, 5.0, 6.0, 1.0);
        assert_eq!(mc.operation(), Operation::Jumping);
        assert_eq!(mc.wanted(), (4.0, 5.0, 6.0));
        mc.strafe(1.0, 0.0);
        assert_eq!(mc.operation(), Operation::Strafe);
        assert_eq!(mc.speed_modifier(), 0.25);
    }
}
