//! `FoodData` — hunger, saturation, exhaustion, and the regeneration timer.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.food.FoodData` (`<init>`, `add`, `eat(int,float)`,
//! `eat(FoodProperties)`, `tick(ServerPlayer)`, `addExhaustion`,
//! `hasEnoughFood`, `needsFood`, `readAdditionalSaveData`,
//! `addAdditionalSaveData`), `net.minecraft.world.food.FoodConstants`,
//! `net.minecraft.util.Mth.clamp(int,int,int)` and `clamp(float,float,float)`,
//! `net.minecraft.world.entity.player.Player.causeFoodExhaustion`.
//!
//! # What main had before this file
//!
//! Nothing. `PlayerState` in `crate::player` is a position, a rotation, two
//! collision flags, the sneak key and a teleport acknowledgement; there is no
//! health field, no food field and no `set_health` packet on the wire. So
//! every constant here is new and none of it replaces a guess.
//!
//! # Every number is out of `FoodConstants` and none of them is written twice
//!
//! `FoodConstants` is a class of `public static final` literals with one
//! method, and each is transcribed once into [`consts`] with the bytecode name
//! it carries. `4.0` appears in `FoodData.tick` as `ldc #62` and in
//! `FoodConstants.EXHAUSTION_DROP`; it is the same number and it is written
//! here once.
//!
//! # The tick is a chain of `else if`, and the order is load-bearing
//!
//! `FoodData.tick`'s offsets 96..324 are one `if/else if/else if/else`
//! ladder, not four independent tests:
//!
//! ```text
//!    96  regen && saturation > 0 && isHurt && foodLevel >= 20   -> fast heal
//!   179  regen && foodLevel >= 18 && isHurt                     -> slow heal
//!   238  foodLevel <= 0                                         -> starve
//!   319  otherwise                                              -> tickTimer = 0
//! ```
//!
//! A player at 20 food who is hurt with no saturation therefore takes the
//! **second** arm, not the first, and a player at 19 food takes the second arm
//! whatever the saturation is. Writing the four as separate `if`s would let a
//! saturated player heal twice in one tick and would reset `tickTimer` under
//! the healing branches that had just incremented it.
//!
//! # `heal` is the caller's, and that is not a shortcut
//!
//! `tick` reaches out of `FoodData` exactly three times — `ServerPlayer.heal`,
//! `ServerPlayer.getHealth` and `ServerPlayer.hurtServer` — and every one of
//! them is on the entity rather than on the food. [`FoodData::tick`] therefore
//! takes what it reads ([`TickInputs`]) and returns what it does
//! ([`FoodAction`]); the self-mutation (exhaustion, food level, the timer)
//! happens here because in vanilla it happens here.
//!
//! The one ordering fact that survives that split: in the fast-heal arm
//! vanilla calls `heal(f / 6.0)` **before** `addExhaustion(f)`, and in the
//! starve arm it calls `hurtServer` **before** `tickTimer = 0`. Neither the
//! heal nor the hurt can be observed by anything else this method later reads,
//! so returning the action rather than performing it in place cannot reorder
//! anything. If a later change makes `heal` re-enter the food data — it does
//! not in 26.2 — this comment is the thing that stops being true.

/// `net.minecraft.world.food.FoodConstants` — every literal it declares.
///
/// Transcribed as a block so that a reader can compare it against one `javap`
/// dump, and so that no other file in this crate carries one of these numbers.
pub mod consts {
    /// `MAX_FOOD` — the ceiling `add` clamps the food level to.
    pub const MAX_FOOD: i32 = 20;
    /// `MAX_SATURATION`. Not read by `FoodData`: `add` clamps saturation to
    /// the **food level**, not to this, so a full player's saturation ceiling
    /// is 20.0 by coincidence of `MAX_FOOD` and never by this constant.
    pub const MAX_SATURATION: f32 = 20.0;
    /// `START_SATURATION` — `FoodData.<init>` puts this in `saturationLevel`.
    pub const START_SATURATION: f32 = 5.0;
    /// `SATURATION_FLOOR`. Declared and not read by `FoodData`.
    pub const SATURATION_FLOOR: f32 = 2.5;
    /// `EXHAUSTION_DROP` — the exhaustion one saturation (or one food) costs.
    pub const EXHAUSTION_DROP: f32 = 4.0;
    /// `HEALTH_TICK_COUNT` — ticks between unsaturated heals, and between
    /// starvation hits.
    pub const HEALTH_TICK_COUNT: i32 = 80;
    /// `HEALTH_TICK_COUNT_SATURATED` — ticks between saturated heals.
    pub const HEALTH_TICK_COUNT_SATURATED: i32 = 10;
    /// `HEAL_LEVEL` — the food level at or above which the slow heal runs.
    pub const HEAL_LEVEL: i32 = 18;
    /// `SPRINT_LEVEL` — `hasEnoughFood` is `getFoodLevel() > SPRINT_LEVEL`.
    pub const SPRINT_LEVEL: f32 = 6.0;
    /// `STARVE_LEVEL` — the starve arm runs at food levels **not above** this.
    pub const STARVE_LEVEL: i32 = 0;
    /// The damage a starving tick deals — `fconst_1` at `FoodData.tick`
    /// offset 306, the argument to
    /// `hurtServer(level, damageSources().starve(), 1.0f)` at 307.
    ///
    /// Not a `FoodConstants` field: vanilla writes the literal at the call
    /// site. It is named here so the caller in [`crate::playertick`] does not
    /// have to.
    pub const STARVE_DAMAGE: f32 = 1.0;
    /// `FOOD_SATURATION_POOR`.
    pub const FOOD_SATURATION_POOR: f32 = 0.1;
    /// `FOOD_SATURATION_LOW`.
    pub const FOOD_SATURATION_LOW: f32 = 0.3;
    /// `FOOD_SATURATION_NORMAL`.
    pub const FOOD_SATURATION_NORMAL: f32 = 0.6;
    /// `FOOD_SATURATION_GOOD`.
    pub const FOOD_SATURATION_GOOD: f32 = 0.8;
    /// `FOOD_SATURATION_MAX`.
    pub const FOOD_SATURATION_MAX: f32 = 1.0;
    /// `FOOD_SATURATION_SUPERNATURAL`.
    pub const FOOD_SATURATION_SUPERNATURAL: f32 = 1.2;
    /// `EXHAUSTION_HEAL` — what the slow heal costs, and the cap the fast heal
    /// takes its saturation from.
    pub const EXHAUSTION_HEAL: f32 = 6.0;
    /// `EXHAUSTION_JUMP`.
    pub const EXHAUSTION_JUMP: f32 = 0.05;
    /// `EXHAUSTION_SPRINT_JUMP`.
    pub const EXHAUSTION_SPRINT_JUMP: f32 = 0.2;
    /// `EXHAUSTION_MINE`.
    pub const EXHAUSTION_MINE: f32 = 0.005;
    /// `EXHAUSTION_ATTACK`.
    pub const EXHAUSTION_ATTACK: f32 = 0.1;
    /// `EXHAUSTION_WALK` — **zero in 26.2.** Walking costs nothing.
    pub const EXHAUSTION_WALK: f32 = 0.0;
    /// `EXHAUSTION_CROUCH` — zero.
    pub const EXHAUSTION_CROUCH: f32 = 0.0;
    /// `EXHAUSTION_SPRINT` — per metre of sprinting, applied by `Player.move`.
    pub const EXHAUSTION_SPRINT: f32 = 0.1;
    /// `EXHAUSTION_SWIM`.
    pub const EXHAUSTION_SWIM: f32 = 0.01;

    /// The ceiling in `FoodData.addExhaustion`, `ldc #169`. It is a literal in
    /// that method and **not** a `FoodConstants` field, so it is named here
    /// rather than left as a bare `40.0` at the one site that uses it.
    pub const MAX_EXHAUSTION: f32 = 40.0;
}

/// `net.minecraft.world.Difficulty`.
///
/// Re-exported rather than declared: `rustmc_entity::host::Difficulty` already
/// carries the four members in ordinal order and `rustmc-server` already
/// depends on that crate. A second enum of the same name in one workspace is
/// two things that have to be kept equal, and nothing would notice if they
/// stopped being.
pub use rustmc_entity::host::Difficulty;

use crate::javamath;

/// What [`FoodData::tick`] reads off the player, gathered before the call.
///
/// Every field is one `invokevirtual` in `FoodData.tick`, named for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickInputs {
    /// `ServerLevel.getDifficulty()`.
    pub difficulty: Difficulty,
    /// `GameRules.get(NATURAL_HEALTH_REGENERATION)`. The 26.2 id is
    /// `minecraft:natural_health_regeneration`; the pre-registry name was
    /// `naturalRegeneration`, and reading it out of memory is how a gamerule
    /// silently stops applying (see `docs/AGENT-BRIEF.md`).
    pub natural_regeneration: bool,
    /// `ServerPlayer.isHurt()`.
    pub is_hurt: bool,
    /// `ServerPlayer.getHealth()`. Read only by the starve arm.
    pub health: f32,
}

/// What one call to [`FoodData::tick`] asks the caller to do to the player.
///
/// `FoodData.tick` performs exactly one of these per call, and never two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FoodAction {
    /// Neither branch fired.
    None,
    /// `ServerPlayer.heal(amount)`.
    Heal(f32),
    /// `ServerPlayer.hurtServer(level, damageSources().starve(), 1.0f)`.
    Starve,
}

/// `net.minecraft.world.food.FoodData`.
///
/// The four fields are the four the class declares, in its own order, and the
/// four `readAdditionalSaveData` restores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodData {
    food_level: i32,
    saturation_level: f32,
    exhaustion_level: f32,
    tick_timer: i32,
}

impl Default for FoodData {
    /// `FoodData.<init>` — `foodLevel = 20`, `saturationLevel = 5.0f`.
    ///
    /// `exhaustionLevel` and `tickTimer` are left at Java's field defaults,
    /// which the class also declares as `DEFAULT_EXHAUSTION_LEVEL = 0.0f` and
    /// `DEFAULT_TICK_TIMER = 0`.
    fn default() -> Self {
        Self {
            food_level: consts::MAX_FOOD,
            saturation_level: consts::START_SATURATION,
            exhaustion_level: 0.0,
            tick_timer: 0,
        }
    }
}

impl FoodData {
    /// A fresh food bar, as `FoodData.<init>` leaves it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The `foodLevel`, `saturationLevel`, `exhaustionLevel` and `tickTimer` a
    /// save carries — `readAdditionalSaveData`, whose defaults for a missing
    /// key are exactly `<init>`'s.
    #[must_use]
    pub const fn restore(
        food_level: i32,
        saturation_level: f32,
        exhaustion_level: f32,
        tick_timer: i32,
    ) -> Self {
        Self {
            food_level,
            saturation_level,
            exhaustion_level,
            tick_timer,
        }
    }

    /// `getFoodLevel()`.
    #[must_use]
    pub const fn food_level(&self) -> i32 {
        self.food_level
    }

    /// `getSaturationLevel()`.
    #[must_use]
    pub const fn saturation_level(&self) -> f32 {
        self.saturation_level
    }

    /// `exhaustionLevel` — no getter in vanilla, but the save data writes it
    /// and the corpus needs to be able to read it.
    #[must_use]
    pub const fn exhaustion_level(&self) -> f32 {
        self.exhaustion_level
    }

    /// `tickTimer` — likewise no getter, likewise in the save data.
    #[must_use]
    pub const fn tick_timer(&self) -> i32 {
        self.tick_timer
    }

    /// `setFoodLevel(int)` — a bare store, with no clamp. `/effect` and the
    /// `/hunger`-shaped commands reach the bar this way and can put it out of
    /// range; that is vanilla's behaviour and not an oversight here.
    pub const fn set_food_level(&mut self, level: i32) {
        self.food_level = level;
    }

    /// `setSaturation(float)` — likewise unclamped.
    pub const fn set_saturation(&mut self, saturation: f32) {
        self.saturation_level = saturation;
    }

    /// `add(int, float)` — private in vanilla, and the whole of both `eat`
    /// overloads.
    ///
    /// The saturation ceiling is `(float) this.foodLevel` **after** the food
    /// level has been raised, so eating raises the ceiling before it clamps
    /// against it. Reading the two lines in the other order caps a full meal
    /// at the old food level.
    fn add(&mut self, nutrition: i32, saturation: f32) {
        self.food_level = javamath::clamp_i(nutrition + self.food_level, 0, consts::MAX_FOOD);
        #[expect(
            clippy::cast_precision_loss,
            reason = "vanilla's own i2f at FoodData.add offset 28; the value is 0..=20"
        )]
        let ceiling = self.food_level as f32;
        self.saturation_level = javamath::clamp_f(saturation + self.saturation_level, 0.0, ceiling);
    }

    /// `eat(int, float)` — nutrition and a *saturation modifier*, the form the
    /// `/effect`-free paths use.
    ///
    /// `FoodConstants.saturationByModifier(n, m)` is `n * m * 2.0f`, so the
    /// modifier is per-half-haunch and the doubling is vanilla's.
    pub fn eat_modifier(&mut self, nutrition: i32, saturation_modifier: f32) {
        self.add(
            nutrition,
            saturation_by_modifier(nutrition, saturation_modifier),
        );
    }

    /// `eat(FoodProperties)` — nutrition and the **already-computed**
    /// saturation off the item's `food` component. No modifier, no doubling.
    ///
    /// The two overloads are one letter apart in vanilla and two different
    /// numbers here, which is why they are not one function with a flag.
    pub fn eat_food(&mut self, nutrition: i32, saturation: f32) {
        self.add(nutrition, saturation);
    }

    /// `addExhaustion(float)` — saturating at
    /// [`consts::MAX_EXHAUSTION`].
    pub fn add_exhaustion(&mut self, exhaustion: f32) {
        self.exhaustion_level =
            javamath::min_f(self.exhaustion_level + exhaustion, consts::MAX_EXHAUSTION);
    }

    /// `hasEnoughFood()` — `getFoodLevel() > 6.0f`, compared as a float
    /// because vanilla widens the int before the comparison.
    #[must_use]
    pub fn has_enough_food(&self) -> bool {
        #[expect(
            clippy::cast_precision_loss,
            reason = "vanilla's own i2f at hasEnoughFood offset 4"
        )]
        let level = self.food_level as f32;
        level > consts::SPRINT_LEVEL
    }

    /// `needsFood()` — `foodLevel < 20`, an int comparison this time.
    #[must_use]
    pub const fn needs_food(&self) -> bool {
        self.food_level < consts::MAX_FOOD
    }

    /// `tick(ServerPlayer)`.
    ///
    /// Offsets 10..78 are the exhaustion drain and run unconditionally;
    /// offsets 96..324 are the four-arm ladder described in the module docs.
    pub fn tick(&mut self, inputs: TickInputs) -> FoodAction {
        // 10..78 — one unit of exhaustion is spent per 4.0 accumulated, and it
        // is spent on saturation if there is any and on the food level
        // otherwise. `if` and not `while`: at most one unit leaves per tick,
        // however much exhaustion has piled up.
        if self.exhaustion_level > consts::EXHAUSTION_DROP {
            self.exhaustion_level -= consts::EXHAUSTION_DROP;
            if self.saturation_level > 0.0 {
                self.saturation_level = javamath::max_f(self.saturation_level - 1.0, 0.0);
            } else if inputs.difficulty != Difficulty::Peaceful {
                self.food_level = javamath::max_i(self.food_level - 1, 0);
            }
        }

        let regen = inputs.natural_regeneration;

        // 96 — the saturated heal. Note `foodLevel >= 20`, not `> 18`: a
        // player one haunch down cannot reach this arm however saturated.
        if regen
            && self.saturation_level > 0.0
            && inputs.is_hurt
            && self.food_level >= consts::MAX_FOOD
        {
            self.tick_timer += 1;
            if self.tick_timer >= consts::HEALTH_TICK_COUNT_SATURATED {
                let f = javamath::min_f(self.saturation_level, consts::EXHAUSTION_HEAL);
                self.tick_timer = 0;
                self.add_exhaustion(f);
                return FoodAction::Heal(f / consts::EXHAUSTION_HEAL);
            }
            return FoodAction::None;
        }

        // 179 — the ordinary heal.
        if regen && self.food_level >= consts::HEAL_LEVEL && inputs.is_hurt {
            self.tick_timer += 1;
            if self.tick_timer >= consts::HEALTH_TICK_COUNT {
                self.tick_timer = 0;
                self.add_exhaustion(consts::EXHAUSTION_HEAL);
                return FoodAction::Heal(1.0);
            }
            return FoodAction::None;
        }

        // 238 — starvation. `ifgt 319` means the arm is entered when the food
        // level is **not** greater than zero, so a negative level (which
        // `setFoodLevel` permits) starves too.
        if self.food_level <= consts::STARVE_LEVEL {
            self.tick_timer += 1;
            if self.tick_timer >= consts::HEALTH_TICK_COUNT {
                // 264..297. The three disjuncts in vanilla's order; the last
                // is a conjunction, which is what stops NORMAL killing.
                let may_hurt = inputs.health > 10.0
                    || inputs.difficulty == Difficulty::Hard
                    || (inputs.health > 1.0 && inputs.difficulty == Difficulty::Normal);
                self.tick_timer = 0;
                if may_hurt {
                    return FoodAction::Starve;
                }
            }
            return FoodAction::None;
        }

        // 319 — the else. A player who is neither healing nor starving loses
        // the timer entirely rather than keeping it for later.
        self.tick_timer = 0;
        FoodAction::None
    }
}

/// `FoodConstants.saturationByModifier(int, float)` — `n * m * 2.0f`.
///
/// The multiply order is vanilla's: `((float) n * m) * 2.0f`. Float multiply
/// is not associative, so `n * (m * 2.0f)` is a different function.
#[must_use]
pub fn saturation_by_modifier(nutrition: i32, modifier: f32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla's own i2f at saturationByModifier offset 0"
    )]
    let n = nutrition as f32;
    n * modifier * 2.0
}

#[cfg(test)]
mod tests {
    use super::{consts, Difficulty, FoodAction, FoodData, TickInputs};

    fn inputs(difficulty: Difficulty, regen: bool, is_hurt: bool, health: f32) -> TickInputs {
        TickInputs {
            difficulty,
            natural_regeneration: regen,
            is_hurt,
            health,
        }
    }

    fn healthy() -> TickInputs {
        inputs(Difficulty::Normal, true, false, 20.0)
    }

    #[test]
    fn a_fresh_bar_is_twenty_and_five() {
        let f = FoodData::new();
        assert_eq!(f.food_level(), 20);
        assert!((f.saturation_level() - 5.0).abs() < f32::EPSILON);
        assert!((f.exhaustion_level() - 0.0).abs() < f32::EPSILON);
        assert_eq!(f.tick_timer(), 0);
    }

    #[test]
    fn eating_raises_the_saturation_ceiling_before_clamping_to_it() {
        // A player at 0 food with 0 saturation eating a 20-nutrition,
        // 20-saturation meal ends at 20/20. If `add` clamped against the OLD
        // food level the saturation would come back 0.
        let mut f = FoodData::restore(0, 0.0, 0.0, 0);
        f.eat_food(20, 20.0);
        assert_eq!(f.food_level(), 20);
        assert!((f.saturation_level() - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn saturation_is_capped_by_the_food_level_not_by_max_saturation() {
        let mut f = FoodData::restore(0, 0.0, 0.0, 0);
        f.eat_food(3, 20.0);
        assert_eq!(f.food_level(), 3);
        assert!((f.saturation_level() - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_modifier_overload_doubles_and_the_properties_one_does_not() {
        // Cooked porkchop in 26.2 terms: nutrition 8. Through the modifier
        // overload with 0.8 that is 8 * 0.8 * 2 = 12.8 saturation, clamped to
        // the food level.
        let mut a = FoodData::restore(0, 0.0, 0.0, 0);
        a.eat_modifier(8, consts::FOOD_SATURATION_GOOD);
        assert!((a.saturation_level() - 8.0).abs() < f32::EPSILON);

        let mut b = FoodData::restore(0, 0.0, 0.0, 0);
        b.eat_food(8, consts::FOOD_SATURATION_GOOD);
        assert!((b.saturation_level() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn exhaustion_saturates_at_forty() {
        let mut f = FoodData::new();
        for _ in 0..100 {
            f.add_exhaustion(1.0);
        }
        assert!((f.exhaustion_level() - consts::MAX_EXHAUSTION).abs() < f32::EPSILON);
    }

    #[test]
    fn four_exhaustion_spends_one_saturation_not_one_food() {
        let mut f = FoodData::restore(20, 5.0, 4.5, 0);
        f.tick(healthy());
        assert!((f.exhaustion_level() - 0.5).abs() < 1e-6);
        assert!((f.saturation_level() - 4.0).abs() < f32::EPSILON);
        assert_eq!(f.food_level(), 20);
    }

    #[test]
    fn with_no_saturation_the_food_level_goes_instead() {
        let mut f = FoodData::restore(20, 0.0, 4.5, 0);
        f.tick(healthy());
        assert_eq!(f.food_level(), 19);
    }

    #[test]
    fn peaceful_spends_the_exhaustion_and_keeps_the_food() {
        // The drain still happens — exhaustion falls by 4.0 — but the food
        // level does not follow it. A reading that skipped the whole block on
        // PEACEFUL would leave exhaustion at 4.5.
        let mut f = FoodData::restore(20, 0.0, 4.5, 0);
        f.tick(inputs(Difficulty::Peaceful, true, false, 20.0));
        assert_eq!(f.food_level(), 20);
        assert!((f.exhaustion_level() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn exactly_four_exhaustion_does_not_drain() {
        // `fcmpl; ifle` — strictly greater than 4.0.
        let mut f = FoodData::restore(20, 5.0, 4.0, 0);
        f.tick(healthy());
        assert!((f.exhaustion_level() - 4.0).abs() < f32::EPSILON);
        assert!((f.saturation_level() - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_saturated_heal_fires_on_the_tenth_tick() {
        let mut f = FoodData::restore(20, 6.0, 0.0, 0);
        let hurt = inputs(Difficulty::Normal, true, true, 10.0);
        for _ in 0..9 {
            assert_eq!(f.tick(hurt), FoodAction::None);
        }
        assert_eq!(f.tick(hurt), FoodAction::Heal(1.0));
        assert_eq!(f.tick_timer(), 0);
        // heal cost 6.0 exhaustion, which is min(saturation, 6.0).
        assert!((f.exhaustion_level() - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_thin_saturation_heals_a_fraction_of_a_heart() {
        let mut f = FoodData::restore(20, 3.0, 0.0, 9);
        let hurt = inputs(Difficulty::Normal, true, true, 10.0);
        assert_eq!(f.tick(hurt), FoodAction::Heal(0.5));
    }

    #[test]
    fn nineteen_food_takes_the_slow_arm_however_saturated() {
        // The distinguishing case for the ladder: with four independent `if`s
        // this player would heal on tick 10.
        let mut f = FoodData::restore(19, 20.0, 0.0, 0);
        let hurt = inputs(Difficulty::Normal, true, true, 10.0);
        for _ in 0..79 {
            assert_eq!(f.tick(hurt), FoodAction::None);
        }
        assert_eq!(f.tick(hurt), FoodAction::Heal(1.0));
        assert!((f.exhaustion_level() - consts::EXHAUSTION_HEAL).abs() < f32::EPSILON);
    }

    #[test]
    fn a_full_but_unsaturated_player_takes_the_slow_arm_too() {
        let mut f = FoodData::restore(20, 0.0, 0.0, 79);
        let hurt = inputs(Difficulty::Normal, true, true, 10.0);
        assert_eq!(f.tick(hurt), FoodAction::Heal(1.0));
    }

    #[test]
    fn the_gamerule_off_stops_both_heals() {
        let mut f = FoodData::restore(20, 20.0, 0.0, 0);
        let hurt = inputs(Difficulty::Normal, false, true, 10.0);
        for _ in 0..200 {
            assert_eq!(f.tick(hurt), FoodAction::None);
        }
        // And the timer is reset by the else arm, never accumulated.
        assert_eq!(f.tick_timer(), 0);
    }

    #[test]
    fn an_unhurt_player_never_heals() {
        let mut f = FoodData::restore(20, 20.0, 0.0, 0);
        for _ in 0..200 {
            assert_eq!(f.tick(healthy()), FoodAction::None);
        }
    }

    #[test]
    fn starvation_hits_on_the_eightieth_tick() {
        let mut f = FoodData::restore(0, 0.0, 0.0, 0);
        let starving = inputs(Difficulty::Hard, true, true, 20.0);
        for _ in 0..79 {
            assert_eq!(f.tick(starving), FoodAction::None);
        }
        assert_eq!(f.tick(starving), FoodAction::Starve);
        assert_eq!(f.tick_timer(), 0);
    }

    #[test]
    fn easy_stops_starving_at_ten_hearts_worth() {
        // health > 10 hurts; at 10.0 exactly, EASY does not.
        let mut f = FoodData::restore(0, 0.0, 0.0, 79);
        assert_eq!(
            f.tick(inputs(Difficulty::Easy, true, true, 10.0)),
            FoodAction::None
        );
        let mut g = FoodData::restore(0, 0.0, 0.0, 79);
        assert_eq!(
            g.tick(inputs(Difficulty::Easy, true, true, 10.5)),
            FoodAction::Starve
        );
    }

    #[test]
    fn normal_starves_down_to_one_and_hard_below_it() {
        let mut n = FoodData::restore(0, 0.0, 0.0, 79);
        assert_eq!(
            n.tick(inputs(Difficulty::Normal, true, true, 1.0)),
            FoodAction::None
        );
        let mut n2 = FoodData::restore(0, 0.0, 0.0, 79);
        assert_eq!(
            n2.tick(inputs(Difficulty::Normal, true, true, 1.5)),
            FoodAction::Starve
        );
        let mut h = FoodData::restore(0, 0.0, 0.0, 79);
        assert_eq!(
            h.tick(inputs(Difficulty::Hard, true, true, 0.5)),
            FoodAction::Starve
        );
    }

    #[test]
    fn the_timer_is_thrown_away_between_states() {
        // 79 ticks of starving, then one meal, and the heal timer starts from
        // zero rather than inheriting 79.
        let mut f = FoodData::restore(0, 0.0, 0.0, 0);
        let starving = inputs(Difficulty::Hard, true, true, 20.0);
        for _ in 0..79 {
            f.tick(starving);
        }
        assert_eq!(f.tick_timer(), 79);
        f.eat_food(10, 0.0);
        // food 10: not healing (needs 18), not starving (>0) -> else arm.
        assert_eq!(f.tick(starving), FoodAction::None);
        assert_eq!(f.tick_timer(), 0);
    }

    #[test]
    fn set_food_level_is_not_clamped_and_a_negative_starves() {
        let mut f = FoodData::new();
        f.set_food_level(-5);
        assert_eq!(f.food_level(), -5);
        let starving = inputs(Difficulty::Hard, true, true, 20.0);
        for _ in 0..79 {
            f.tick(starving);
        }
        assert_eq!(f.tick(starving), FoodAction::Starve);
    }

    /// A misread `tick`, built to order, so that the assertions above can be
    /// shown to separate it from the real one.
    ///
    /// This is the positive control for the file. Every test above runs one
    /// implementation and would pass just as happily against a `tick` that
    /// never healed anybody, so one of them has to be shown catching a
    /// specific wrong reading rather than asserted to. Each variant is one
    /// documented misreading, applied alone.
    #[derive(Clone, Copy)]
    enum Misreading {
        /// The four-arm ladder written as four independent `if`s.
        LadderAsFourIfs,
        /// The third starve disjunct written `health > 1.0 || NORMAL` rather
        /// than `&&`.
        StarveFloorDisjunct,
        /// The exhaustion drain written `>=` rather than `>`.
        DrainAtExactlyFour,
    }

    /// `FoodData::tick` with exactly one misreading applied. Only the parts a
    /// misreading can reach are re-derived; everything else defers to the real
    /// implementation, so a variant that changed nothing would be caught by
    /// the `assert_ne!` rather than hidden by it.
    fn tick_misread(f: &mut FoodData, inp: TickInputs, which: Misreading) -> FoodAction {
        let drains = match which {
            Misreading::DrainAtExactlyFour => f.exhaustion_level >= consts::EXHAUSTION_DROP,
            _ => f.exhaustion_level > consts::EXHAUSTION_DROP,
        };
        if drains {
            f.exhaustion_level -= consts::EXHAUSTION_DROP;
            if f.saturation_level > 0.0 {
                f.saturation_level = crate::javamath::max_f(f.saturation_level - 1.0, 0.0);
            } else if inp.difficulty != Difficulty::Peaceful {
                f.food_level = crate::javamath::max_i(f.food_level - 1, 0);
            }
        }
        let regen = inp.natural_regeneration;
        let saturated_arm =
            regen && f.saturation_level > 0.0 && inp.is_hurt && f.food_level >= consts::MAX_FOOD;
        if saturated_arm {
            f.tick_timer += 1;
            if f.tick_timer >= consts::HEALTH_TICK_COUNT_SATURATED {
                let amount = crate::javamath::min_f(f.saturation_level, consts::EXHAUSTION_HEAL);
                f.tick_timer = 0;
                f.add_exhaustion(amount);
                return FoodAction::Heal(amount / consts::EXHAUSTION_HEAL);
            }
            if !matches!(which, Misreading::LadderAsFourIfs) {
                return FoodAction::None;
            }
        }
        if regen && f.food_level >= consts::HEAL_LEVEL && inp.is_hurt {
            f.tick_timer += 1;
            if f.tick_timer >= consts::HEALTH_TICK_COUNT {
                f.tick_timer = 0;
                f.add_exhaustion(consts::EXHAUSTION_HEAL);
                return FoodAction::Heal(1.0);
            }
            if !matches!(which, Misreading::LadderAsFourIfs) {
                return FoodAction::None;
            }
        }
        if f.food_level <= consts::STARVE_LEVEL {
            f.tick_timer += 1;
            if f.tick_timer >= consts::HEALTH_TICK_COUNT {
                let may_hurt = match which {
                    Misreading::StarveFloorDisjunct => {
                        inp.health > 10.0
                            || inp.difficulty == Difficulty::Hard
                            || inp.health > 1.0
                            || inp.difficulty == Difficulty::Normal
                    }
                    _ => {
                        inp.health > 10.0
                            || inp.difficulty == Difficulty::Hard
                            || (inp.health > 1.0 && inp.difficulty == Difficulty::Normal)
                    }
                };
                f.tick_timer = 0;
                if may_hurt {
                    return FoodAction::Starve;
                }
            }
            return FoodAction::None;
        }
        f.tick_timer = 0;
        FoodAction::None
    }

    /// Each misreading, on the input the corresponding test above uses, must
    /// disagree with the transcription. A `tick` that had been written from
    /// memory would fail one of these four rather than all of the others.
    #[test]
    fn every_misreading_this_file_guards_against_is_separable() {
        // 1. The ladder. A full, saturated, hurt player is inside the FIRST
        //    arm, so the misreading shows up as the timer being advanced
        //    twice in one tick — once by the saturated arm and again by the
        //    ordinary one it falls through into.
        let hurt = inputs(Difficulty::Normal, true, true, 10.0);
        let mut real = FoodData::restore(20, 20.0, 0.0, 8);
        let mut bad = FoodData::restore(20, 20.0, 0.0, 8);
        real.tick(hurt);
        tick_misread(&mut bad, hurt, Misreading::LadderAsFourIfs);
        assert_eq!(real.tick_timer(), 9);
        assert_ne!(real.tick_timer(), bad.tick_timer());

        // 2. The ceiling inside `add`. Eating 20/20.0 at zero food: the
        //    transcription lands 20 saturation, the misreading lands 0.
        let mut ok = FoodData::restore(0, 0.0, 0.0, 0);
        ok.eat_food(20, 20.0);
        let ceiling_before = crate::javamath::clamp_f(20.0 + 0.0, 0.0, 0.0);
        assert_ne!(ok.saturation_level(), ceiling_before);

        // 3. The starve floor. NORMAL at exactly one heart: vanilla spares,
        //    the flattened disjunction kills.
        let one_heart = inputs(Difficulty::Normal, true, true, 1.0);
        let mut real = FoodData::restore(0, 0.0, 0.0, 79);
        let mut bad = FoodData::restore(0, 0.0, 0.0, 79);
        assert_ne!(
            real.tick(one_heart),
            tick_misread(&mut bad, one_heart, Misreading::StarveFloorDisjunct)
        );

        // 4. The drain comparison. Exactly 4.0 exhaustion: vanilla keeps it,
        //    `>=` spends it.
        let calm = healthy();
        let mut real = FoodData::restore(20, 5.0, 4.0, 0);
        let mut bad = FoodData::restore(20, 5.0, 4.0, 0);
        real.tick(calm);
        tick_misread(&mut bad, calm, Misreading::DrainAtExactlyFour);
        assert_ne!(real.saturation_level(), bad.saturation_level());
    }
}
