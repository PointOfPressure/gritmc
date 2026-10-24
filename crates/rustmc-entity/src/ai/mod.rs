//! Vanilla's mob AI: the goal system, the controls, the ground navigation
//! and the goals, transcribed from the 26.2 jar.
//!
//! Nothing here knows what a mob *is*; every mob fact is a [`crate::host::MobHost`]
//! call. [`MobAi`] holds the AI fields `Mob` declares and drives them the
//! way `Mob.serverAiStep` does.

pub mod control;
pub mod direction;
pub mod goal;
pub mod goal_selector;
pub mod goals;
pub mod mob_ai;
pub mod navigation;
pub mod random_pos;
pub mod sensing;
pub mod tamable;
pub mod targeting;
pub mod wrapped_goal;

pub use control::{BodyRotationControl, JumpControl, LookControl, MoveControl};
pub use goal::{Flag, FlagSet, Goal};
pub use goal_selector::{GoalId, GoalSelector};
pub use mob_ai::MobAi;
pub use navigation::GroundPathNavigation;
pub use sensing::Sensing;
pub use wrapped_goal::WrappedGoal;
