//! `Sheep`: goals, wool, shearing, dye, and the grass it eats.
//!
//! Reference (26.2, `javap -p -c -constants` on the pinned jar):
//! `Sheep.registerGoals` (0-176), `Sheep.createAttributes` (`MAX_HEALTH` 8,
//! `MOVEMENT_SPEED` 0.23F), `Sheep.defineSynchedData` (`DATA_WOOL_ID`
//! `(byte) 0`), `Sheep.getColor` (`wool & 15`), `setColor` (`wool & 240 |
//! id & 15`), `isSheared` (`wool & 16`), `setSheared` (`| 16` / `& -17`),
//! `Sheep.mobInteract` (0-86), `Sheep.shear` (0-32), `Sheep.lambda$shear$0`
//! (0-109), `Sheep.readyForShearing`, `Sheep.ate` (0-22),
//! `Sheep.finalizeSpawn` (0-21), `Sheep.getRandomSheepColor`,
//! `SheepColorSpawnRules` (`<clinit>`, `getSheepColor`,
//! `getSheepColorConfiguration`, `commonColors`), `WeightedList` (`getRandom`:
//! `nextInt(totalWeight)` then the cumulative walk, `Flat` below 64 and
//! `Compact` above, the same answer), `Sheep.getBreedOffspring` (0-46),
//! `DyeColor.getMixedColor` / `findColorMixInRecipes`,
//! `DyeItem.interactLivingEntity` (0-104), `Player.interactOn` (107-203),
//! `EatBlockGoal`, `Sheep.playStepSound`, `Sheep.<clinit>`, `EntityTypes.SHEEP`.
//!
//! # The goal table, from `Sheep.registerGoals`
//!
//! ```text
//!   0  FloatGoal(this)
//!   1  PanicGoal(this, 1.25)
//!   2  BreedGoal(this, 1.0)
//!   3  TemptGoal(this, 1.1, stack -> stack.is(ItemTags.SHEEP_FOOD), false)
//!   4  FollowParentGoal(this, 1.1)
//!   5  eatBlockGoal = new EatBlockGoal(this)        (built at offset 0)
//!   6  WaterAvoidingRandomStrollGoal(this, 1.0)
//!   7  LookAtPlayerGoal(this, Player.class, 6.0F)
//!   8  RandomLookAroundGoal(this)
//! ```
//!
//! `customServerAiStep` copies the goal's `eatAnimationTick` into the
//! sheep's own field before `Animal.customServerAiStep`; that field is read
//! only by the client's head animation (`getHeadEatPositionScale`), which is
//! driven on the client by entity event 10, so nothing on the wire reads the
//! server's copy and it is not kept.
use super::host::ServerMob;
use super::interact::{Interactor, Outcome, Swing};
use super::MobKind;
use rustmc_entity::ai::goals::breed_goal::BreedGoal;
use rustmc_entity::ai::goals::eat_block_goal::EatBlockGoal;
use rustmc_entity::ai::goals::float_goal::FloatGoal;
use rustmc_entity::ai::goals::follow_parent_goal::FollowParentGoal;
use rustmc_entity::ai::goals::look_at_player_goal::LookAtPlayerGoal;
use rustmc_entity::ai::goals::panic_goal::PanicGoal;
use rustmc_entity::ai::goals::random_look_around_goal::RandomLookAroundGoal;
use rustmc_entity::ai::goals::random_stroll_goal::RandomStrollGoal;
use rustmc_entity::ai::goals::tempt_goal::TemptGoal;
use rustmc_entity::ai::mob_ai::MobAi;
use rustmc_entity::host::{EntityClass, MobHost};
use rustmc_entity::tick::MobCategory;
use rustmc_worldgen::rng::RandomSource;
use std::sync::OnceLock;

/// `EntityTypes.SHEEP`: `of(Sheep::new, CREATURE).sized(0.9F, 1.3F)
/// .eyeHeight(1.235F).passengerAttachments(1.2375F).clientTrackingRange(10)`.
pub static SHEEP: MobKind = MobKind {
    enemy: false,
    name: "minecraft:sheep",
    entity_kind: crate::entities::SHEEP,
    width: 0.9,
    height: 1.3,
    eye_height: 1.235,
    // `Sheep.BABY_DIMENSIONS`: `scalable(0.45F, 0.65F).withEyeHeight(0.65625F)`.
    baby_dimensions: Some((0.45, 0.65)),
    baby_eye_height: Some(0.656_25),
    category: MobCategory::Creature,
    allowed_in_peaceful: true,
    data: super::DataLayout::Sheep,
    ambient_sound_interval: 120,
    // `LivingEntity.getSoundVolume()`: `Sheep` does not override it.
    sound_volume: 1.0,
    voice_pitch_scale: 1.0,
    pushable: true,
    emits_sounds: true,
    remove_when_far_away: false,
};

/// `Sheep.registerGoals`. Returns the `PanicGoal`'s id.
pub fn register_goals(ai: &mut MobAi<ServerMob>) -> rustmc_entity::ai::goal_selector::GoalId {
    let float = FloatGoal::install(ai);
    ai.goal_selector.add_goal(0, Box::new(float));
    let panic = ai.goal_selector.add_goal(1, Box::new(PanicGoal::new(1.25)));
    ai.goal_selector.add_goal(2, Box::new(BreedGoal::new(1.0)));
    ai.goal_selector
        .add_goal(3, Box::new(TemptGoal::new(1.1, false)));
    ai.goal_selector
        .add_goal(4, Box::new(FollowParentGoal::new(1.1)));
    ai.goal_selector.add_goal(5, Box::new(EatBlockGoal::new()));
    ai.goal_selector
        .add_goal(6, Box::new(RandomStrollGoal::water_avoiding(1.0)));
    let look = LookAtPlayerGoal::new(ai, EntityClass::Player, 6.0);
    ai.goal_selector.add_goal(7, Box::new(look));
    ai.goal_selector
        .add_goal(8, Box::new(RandomLookAroundGoal::new()));
    panic
}

/// `Sheep.isFood` / `lambda$registerGoals$0`: `#sheep_food`.
#[must_use]
pub fn is_food(item: &str) -> bool {
    super::item_tag_contains("sheep_food", item)
}

/// `Sheep.getAmbientSound` — `SHEEP_AMBIENT`, whatever the age.
pub const AMBIENT: &str = "minecraft:entity.sheep.ambient";
/// `Sheep.playStepSound`: `playSound(SHEEP_STEP, 0.15F, 1.0F)`.
pub const STEP: &str = "minecraft:entity.sheep.step";
/// `Sheep.shear` offset 3.
pub const SHEAR: &str = "minecraft:entity.sheep.shear";
/// `DyeItem.interactLivingEntity` offset 64.
pub const DYE_USE: &str = "minecraft:item.dye.use";

/// `DyeColor`, by `getId()` — the order of the enum and of the wool nibble.
pub const DYE_COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

const WHITE: u8 = 0;
const PINK: u8 = 6;
const GRAY: u8 = 7;
const LIGHT_GRAY: u8 = 8;
const BROWN: u8 = 12;
const BLACK: u8 = 15;

/// The `DyeColor` an item's `DataComponents.DYE` names: the sixteen
/// `<colour>_dye` items, which `Items.<clinit>` builds through one lambda
/// (`lambda$static$133`) that is the only `DYE` component in the class.
#[must_use]
pub fn dye_of(item: &str) -> Option<u8> {
    let colour = item.strip_prefix("minecraft:")?.strip_suffix("_dye")?;
    DYE_COLORS
        .iter()
        .position(|c| *c == colour)
        .and_then(|i| u8::try_from(i).ok())
}

/// `getColor()`: `DyeColor.byId(wool & 15)`.
#[must_use]
pub fn color(m: &ServerMob) -> u8 {
    m.data.get_byte(super::data::id::SHEEP_WOOL) & 15
}

/// `setColor(colour)`: `wool & 240 | colour.getId() & 15`.
pub fn set_color(m: &mut ServerMob, colour: u8) {
    let b = m.data.get_byte(super::data::id::SHEEP_WOOL);
    m.data
        .set_byte(super::data::id::SHEEP_WOOL, (b & 240) | (colour & 15));
}

/// `isSheared()`: `wool & 16`.
#[must_use]
pub fn is_sheared(m: &ServerMob) -> bool {
    m.data.get_byte(super::data::id::SHEEP_WOOL) & 16 != 0
}

/// `setSheared(v)`.
pub fn set_sheared(m: &mut ServerMob, v: bool) {
    let b = m.data.get_byte(super::data::id::SHEEP_WOOL);
    let n = if v { b | 16 } else { b & !16 };
    m.data.set_byte(super::data::id::SHEEP_WOOL, n);
}

/// `readyForShearing()`: `!isSheared() && !isBaby()`.
#[must_use]
pub fn ready_for_shearing(m: &ServerMob) -> bool {
    !is_sheared(m) && !m.is_baby()
}

/// `SheepColorSpawnRules.getSheepColor(biome, level.getRandom())`.
///
/// The configuration is chosen by biome tag — warm first, then cold, else
/// temperate — and each is a `WeightedList` of providers whose weights sum to
/// 100: one `nextInt(100)` picks the provider (cumulative, in `<clinit>`
/// order), and the 82-weight one (`commonColors`) is itself a `WeightedList`
/// of `499` of its colour and `1` pink — a second `nextInt(500)`, drawn
/// only when it is the one picked.
///
/// ```text
///   temperate  black 5, gray 5, light_gray 5, brown 3, common(white) 82
///   warm       gray 5, light_gray 5, white 5, black 3,  common(brown) 82
///   cold       light_gray 5, gray 5, white 5, brown 3,  common(black) 82
/// ```
pub fn random_sheep_color(m: &ServerMob) -> u8 {
    let biome = m.level.biome_at(m.block_position());
    let table = super::mob_variants();
    let warm = table.is_some_and(|t| t.warm_farm_biomes.contains(&biome));
    let cold = !warm && table.is_some_and(|t| t.cold_farm_biomes.contains(&biome));
    let (singles, common): ([(u8, i32); 4], u8) = if warm {
        ([(GRAY, 5), (LIGHT_GRAY, 5), (WHITE, 5), (BLACK, 3)], BROWN)
    } else if cold {
        ([(LIGHT_GRAY, 5), (GRAY, 5), (WHITE, 5), (BROWN, 3)], BLACK)
    } else {
        ([(BLACK, 5), (GRAY, 5), (LIGHT_GRAY, 5), (BROWN, 3)], WHITE)
    };
    super::level_random(|r| {
        let mut i = r.next_i32_bound(100);
        for (colour, weight) in singles {
            if i < weight {
                return colour;
            }
            i -= weight;
        }
        if r.next_i32_bound(500) < 499 {
            common
        } else {
            PINK
        }
    })
}

/// `Sheep.finalizeSpawn` 0-11: `setColor(getRandomSheepColor(level,
/// blockPosition()))`, then `Animal.finalizeSpawn`.
pub fn finalize_spawn(m: &mut ServerMob) {
    let c = random_sheep_color(m);
    set_color(m, c);
}

/// `Sheep.mobInteract`, 0-86: shears that can shear shear and answer
/// `SUCCESS_SERVER`; shears that cannot answer `CONSUME` (offset 76) and
/// stop there; anything else is `Animal.mobInteract`, which `None` says.
pub fn mob_interact(m: &mut ServerMob, who: &Interactor<'_>) -> Option<Outcome> {
    if who.item != Some("minecraft:shears") {
        return None;
    }
    if !ready_for_shearing(m) {
        return Some(Outcome::Success(Swing::None));
    }
    shear(m);
    // 54-69: `gameEvent(SHEAR, player)` (no packet), then
    // `stack.hurtAndBreak(1, player, hand)` — the caller's, and nothing for a
    // creative player.
    Some(Outcome::Success(Swing::Server))
}

/// `Sheep.shear(level, PLAYERS, tool)`, 0-32.
///
/// ```text
///    0  level.playSound(null, this, SHEEP_SHEAR, PLAYERS, 1F, 1F)   sound_entity
///   12  dropFromShearingLootTable(level, SHEAR_SHEEP, tool, lambda$shear$0)
///   27  setSheared(true)
/// ```
///
/// `SHEAR_SHEEP` is `shearing/sheep`: one pool, one roll, an `alternatives`
/// of sixteen `loot_table` entries each conditioned on the colour and on
/// `sheared: false` — conditions, no draw — into `shearing/sheep/<colour>`,
/// whose one pool rolls `uniform(1, 3)` of one wool entry. The nested table
/// runs inside the outer one's `LootContext`, so the one draw,
/// `nextInt(3) + 1`, comes from the **outer** table's `random_sequence`,
/// `minecraft:shearing/sheep` (see [`super::drops`]). Each roll is a stack of
/// one, and `lambda$shear$0` spawns `count` single items per stack.
pub fn shear(m: &mut ServerMob) {
    super::play_entity_sound(m, SHEAR, crate::sounds::SOURCE_PLAYERS);
    let colour = DYE_COLORS[usize::from(color(m))];
    let wool = format!("minecraft:{colour}_wool");
    // Every alternative is conditioned on `sheared: false`, which holds here
    // (readyForShearing was checked, and setSheared comes after).
    let rolls = super::drops::random_sequence("minecraft:shearing/sheep", |r| {
        super::drops::uniform_int(r, 1, 3)
    });
    for _ in 0..rolls {
        // lambda$shear$0: `spawnAtLocation(level, stack.copyWithCount(1), 1F)`
        // then `setDeltaMovement(getDeltaMovement().add(...))` on the SHEEP's
        // random: x `(f - f) * 0.1F`, y `f * 0.05F`, z `(f - f) * 0.1F`.
        let id = super::drops::spawn_item_at((m.x(), m.y() + 1.0, m.z()), &wool);
        let a = m.random.next_f32();
        let b = m.random.next_f32();
        let dx = f64::from((a - b) * 0.1_f32);
        let dy = f64::from(m.random.next_f32() * 0.05_f32);
        let c = m.random.next_f32();
        let d = m.random.next_f32();
        let dz = f64::from((c - d) * 0.1_f32);
        super::drops::add_item_movement(id, (dx, dy, dz));
    }
    set_sheared(m, true);
}

/// `DyeItem.interactLivingEntity(stack, player, sheep, hand)`, 0-104 — the
/// stack's own answer once `Mob.interact` has passed. `None` is `PASS`.
///
/// ```text
///    0  a Sheep, alive, not sheared, a DYE component, a different colour
///   56  level.playSound(player, sheep, DYE_USE, PLAYERS, 1F, 1F)  — not to
///       the player
///   85  setColor(dye); stack.shrink(1)        the caller's shrink, and on
///                                             a creative player's COPY
///                                             (`interactOn` 129-137)
///   97  SUCCESS
/// ```
pub fn dye(m: &mut ServerMob, who: &Interactor<'_>) -> Option<Outcome> {
    if !matches!(m.kind.data, super::DataLayout::Sheep) || !m.is_alive() || is_sheared(m) {
        return None;
    }
    let d = dye_of(who.item?)?;
    if color(m) == d {
        return None;
    }
    super::play_entity_sound_except(
        m,
        DYE_USE,
        crate::sounds::SOURCE_PLAYERS,
        Some(who.entity_id),
    );
    set_color(m, d);
    Some(Outcome::Success(Swing::Client))
}

/// `Sheep.ate()`, 0-22: `Animal.ate` (a game event), `setSheared(false)`,
/// and a baby grows by `ageUp(60)` — `AgeableMob.ageUp(int)` is
/// `ageUp(seconds, false)`.
pub fn ate(m: &mut ServerMob) {
    set_sheared(m, false);
    let locked = m.data.get_bool(super::data::id::AGE_LOCKED);
    if m.is_baby() && !locked {
        super::interact::age_up(m, 60, false);
    }
}

/// `EatBlockGoal.tick` 84: `level.destroyBlock(pos, false)` on an
/// `#edible_for_sheep` plant — `Level.destroyBlock(pos, drop, null, 512)`:
/// `levelEvent(2001, pos, Block.getId(state))` (the state is never a fire
/// block here), no drops, `setBlock(pos, fluid.createLegacyBlock(), 3)` —
/// air, since none of the four plants is waterloggable — and a game event.
///
/// Flag 3's neighbour updates are not run: nothing a sheep can eat supports
/// or is shaped by a neighbour this server would have to tell about.
pub fn destroy_block(m: &mut ServerMob, pos: rustmc_data::BlockPos) {
    use crate::edits::Blocks;
    let state = m.level.state_id(pos);
    if m.level.facts.row(state).is_some_and(|r| r.air) {
        return;
    }
    let data = i32::try_from(state).unwrap_or(0);
    super::level_event_with(m, 2001, pos, data);
    let at = (pos.x, pos.y, pos.z);
    if m.level.world.set_block(at, "minecraft:air").is_some() {
        crate::broadcast::ChangedBlocks::shared().record(at);
    }
}

/// `EatBlockGoal.tick` 147-178: `levelEvent(2001, pos,
/// Block.getId(GRASS_BLOCK.defaultBlockState()))` then
/// `setBlock(pos, DIRT.defaultBlockState(), 2)` — flag 2, clients told, no
/// neighbour updates.
pub fn turn_grass_to_dirt(m: &mut ServerMob, pos: rustmc_data::BlockPos) {
    use crate::edits::Blocks;
    let world = m.level.world;
    let grass = world
        .default_state("minecraft:grass_block")
        .and_then(|s| world.state_id(&s))
        .unwrap_or(0);
    super::level_event_with(m, 2001, pos, grass);
    let Some(dirt) = world.default_state("minecraft:dirt") else {
        return;
    };
    let at = (pos.x, pos.y, pos.z);
    if world.set_block(at, &dirt).is_some() {
        crate::broadcast::ChangedBlocks::shared().record(at);
    }
}

/// `Sheep.getBreedOffspring` 14-42: the baby's colour is
/// `DyeColor.getMixedColor(level, getColor(), partner.getColor())`.
///
/// `getMixedColor` (0-30): `findColorMixInRecipes` — a 2x1 `CraftingInput`
/// of one item carrying each colour's `DYE` component, looked up in the
/// crafting recipes, and the result's own `DYE` — else `level.getRandom()
/// .nextBoolean() ? a : b`. The level draw is made only when no recipe
/// mixes the two.
pub fn mixed_color(a: u8, b: u8) -> u8 {
    if let Some(c) = colour_mix_in_recipes(a, b) {
        return c;
    }
    if super::level_random(|r| r.next_bool()) {
        a
    } else {
        b
    }
}

fn colour_mix_in_recipes(a: u8, b: u8) -> Option<u8> {
    static RECIPES: OnceLock<Option<crate::recipes::Recipes>> = OnceLock::new();
    let recipes = RECIPES
        .get_or_init(|| {
            crate::recipes::Recipes::load(
                &super::table_path("data/vanilla/recipes.json"),
                &super::table_path("data/vanilla/tags_item.json"),
            )
            .ok()
        })
        .as_ref()?;
    let item = |c: u8| format!("minecraft:{}_dye", DYE_COLORS[usize::from(c)]);
    let input = crate::recipes::CraftingInput::of(
        2,
        1,
        &[
            crate::item::ItemStack::one(&item(a)),
            crate::item::ItemStack::one(&item(b)),
        ],
    );
    let recipe = recipes.find(&input)?;
    let out = recipe.assemble(&input);
    dye_of(out.item())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dye_items_name_their_colour_by_id() {
        assert_eq!(dye_of("minecraft:white_dye"), Some(0));
        assert_eq!(dye_of("minecraft:lime_dye"), Some(5));
        assert_eq!(dye_of("minecraft:light_blue_dye"), Some(3));
        assert_eq!(dye_of("minecraft:black_dye"), Some(15));
        assert_eq!(dye_of("minecraft:white_wool"), None);
        assert_eq!(dye_of("minecraft:dye"), None);
    }
}
