//! `net.minecraft.world.level.pathfinder.PathType`, from the 26.2 jar.
//!
//! Twenty-seven values in ordinal order, each with its `malus` from
//! `PathType.<clinit>`. `-1` is impassable; `0` is free; a positive malus
//! is added to the A* cost of every node of that type.

/// `PathType`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum PathType {
    Blocked = 0,
    Open = 1,
    Walkable = 2,
    WalkableDoor = 3,
    Trapdoor = 4,
    PowderSnow = 5,
    OnTopOfPowderSnow = 6,
    Fence = 7,
    Lava = 8,
    Water = 9,
    WaterBorder = 10,
    Rail = 11,
    UnpassableRail = 12,
    FireInNeighbor = 13,
    Fire = 14,
    DamagingInNeighbor = 15,
    Damaging = 16,
    DoorOpen = 17,
    DoorWoodClosed = 18,
    DoorIronClosed = 19,
    Breach = 20,
    Leaves = 21,
    StickyHoney = 22,
    Cocoa = 23,
    DamageCautious = 24,
    OnTopOfTrapdoor = 25,
    BigMobsCloseToDanger = 26,
}

impl PathType {
    /// `PathType.values()`.
    pub const ALL: [PathType; 27] = [
        PathType::Blocked,
        PathType::Open,
        PathType::Walkable,
        PathType::WalkableDoor,
        PathType::Trapdoor,
        PathType::PowderSnow,
        PathType::OnTopOfPowderSnow,
        PathType::Fence,
        PathType::Lava,
        PathType::Water,
        PathType::WaterBorder,
        PathType::Rail,
        PathType::UnpassableRail,
        PathType::FireInNeighbor,
        PathType::Fire,
        PathType::DamagingInNeighbor,
        PathType::Damaging,
        PathType::DoorOpen,
        PathType::DoorWoodClosed,
        PathType::DoorIronClosed,
        PathType::Breach,
        PathType::Leaves,
        PathType::StickyHoney,
        PathType::Cocoa,
        PathType::DamageCautious,
        PathType::OnTopOfTrapdoor,
        PathType::BigMobsCloseToDanger,
    ];

    /// `ordinal()`.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// `getMalus()`: the constructor's float, from `PathType.<clinit>`.
    #[must_use]
    pub const fn malus(self) -> f32 {
        match self {
            PathType::Blocked => -1.0,
            PathType::Open => 0.0,
            PathType::Walkable => 0.0,
            PathType::WalkableDoor => 0.0,
            PathType::Trapdoor => 0.0,
            PathType::PowderSnow => -1.0,
            PathType::OnTopOfPowderSnow => 0.0,
            PathType::Fence => -1.0,
            PathType::Lava => -1.0,
            PathType::Water => 8.0,
            PathType::WaterBorder => 8.0,
            PathType::Rail => 0.0,
            PathType::UnpassableRail => -1.0,
            PathType::FireInNeighbor => 8.0,
            PathType::Fire => 16.0,
            PathType::DamagingInNeighbor => 8.0,
            PathType::Damaging => -1.0,
            PathType::DoorOpen => 0.0,
            PathType::DoorWoodClosed => -1.0,
            PathType::DoorIronClosed => -1.0,
            PathType::Breach => 4.0,
            PathType::Leaves => -1.0,
            PathType::StickyHoney => 8.0,
            PathType::Cocoa => 0.0,
            PathType::DamageCautious => 0.0,
            PathType::OnTopOfTrapdoor => 0.0,
            PathType::BigMobsCloseToDanger => 4.0,
        }
    }

    /// The Java constant's name, for goldens and debug output.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PathType::Blocked => "BLOCKED",
            PathType::Open => "OPEN",
            PathType::Walkable => "WALKABLE",
            PathType::WalkableDoor => "WALKABLE_DOOR",
            PathType::Trapdoor => "TRAPDOOR",
            PathType::PowderSnow => "POWDER_SNOW",
            PathType::OnTopOfPowderSnow => "ON_TOP_OF_POWDER_SNOW",
            PathType::Fence => "FENCE",
            PathType::Lava => "LAVA",
            PathType::Water => "WATER",
            PathType::WaterBorder => "WATER_BORDER",
            PathType::Rail => "RAIL",
            PathType::UnpassableRail => "UNPASSABLE_RAIL",
            PathType::FireInNeighbor => "FIRE_IN_NEIGHBOR",
            PathType::Fire => "FIRE",
            PathType::DamagingInNeighbor => "DAMAGING_IN_NEIGHBOR",
            PathType::Damaging => "DAMAGING",
            PathType::DoorOpen => "DOOR_OPEN",
            PathType::DoorWoodClosed => "DOOR_WOOD_CLOSED",
            PathType::DoorIronClosed => "DOOR_IRON_CLOSED",
            PathType::Breach => "BREACH",
            PathType::Leaves => "LEAVES",
            PathType::StickyHoney => "STICKY_HONEY",
            PathType::Cocoa => "COCOA",
            PathType::DamageCautious => "DAMAGE_CAUTIOUS",
            PathType::OnTopOfTrapdoor => "ON_TOP_OF_TRAPDOOR",
            PathType::BigMobsCloseToDanger => "BIG_MOBS_CLOSE_TO_DANGER",
        }
    }

    /// Parse the Java constant's name.
    #[must_use]
    pub fn from_name(s: &str) -> Option<PathType> {
        PathType::ALL.into_iter().find(|t| t.name() == s)
    }
}

/// `EnumSet<PathType>`, iterating in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathTypeSet(u32);

impl PathTypeSet {
    pub const NONE: PathTypeSet = PathTypeSet(0);

    /// `add`.
    pub fn add(&mut self, t: PathType) -> bool {
        let before = self.0;
        self.0 |= 1 << t.ordinal();
        before != self.0
    }

    /// `contains`.
    #[must_use]
    pub const fn contains(self, t: PathType) -> bool {
        self.0 & (1 << t.ordinal()) != 0
    }

    /// `size`.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `iterator()`, ordinal order.
    pub fn iter(self) -> impl Iterator<Item = PathType> {
        PathType::ALL.into_iter().filter(move |t| self.contains(*t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals_are_dense_and_named() {
        for (i, t) in PathType::ALL.iter().enumerate() {
            assert_eq!(t.ordinal(), i);
            assert_eq!(PathType::from_name(t.name()), Some(*t));
        }
    }

    #[test]
    fn the_maluses_read_from_clinit() {
        assert_eq!(PathType::Blocked.malus(), -1.0);
        assert_eq!(PathType::Water.malus(), 8.0);
        assert_eq!(PathType::Fire.malus(), 16.0);
        assert_eq!(PathType::Breach.malus(), 4.0);
        assert_eq!(PathType::BigMobsCloseToDanger.malus(), 4.0);
        assert_eq!(PathType::DamageCautious.malus(), 0.0);
        assert_eq!(PathType::Leaves.malus(), -1.0);
    }

    #[test]
    fn set_iterates_in_ordinal_order() {
        let mut s = PathTypeSet::NONE;
        s.add(PathType::Water);
        s.add(PathType::Open);
        assert_eq!(s.len(), 2);
        assert_eq!(
            s.iter().collect::<Vec<_>>(),
            vec![PathType::Open, PathType::Water]
        );
    }
}
