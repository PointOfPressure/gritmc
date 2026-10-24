//! The horizontal `net.minecraft.core.Direction`s the goals use, from
//! the 26.2 jar.
//!
//! `Direction.fromYRot(double)`: `from2DDataValue(Mth.floor(d / 90.0 + 0.5) & 3)`,
//! and `BY_2D_DATA` is ordered by each direction's `data2d`:
//! `SOUTH 0, WEST 1, NORTH 2, EAST 3`. `getStepX/Z` are the unit vector;
//! `getOpposite()` swaps the sign.

use rustmc_data::BlockPos;
use rustmc_worldgen::math::floor;

/// A horizontal `Direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    North,
    South,
    West,
    East,
}

impl Direction {
    /// `Direction.fromYRot(double)`.
    #[must_use]
    pub fn from_y_rot(d: f64) -> Self {
        Self::from_2d_data_value(floor(d / 90.0 + 0.5) & 3)
    }

    /// `Direction.from2DDataValue(int)`: `BY_2D_DATA[abs(i % 4)]`.
    #[must_use]
    pub fn from_2d_data_value(i: i32) -> Self {
        match (i % 4).abs() {
            0 => Direction::South,
            1 => Direction::West,
            2 => Direction::North,
            _ => Direction::East,
        }
    }

    /// `getStepX()`.
    #[must_use]
    pub fn step_x(self) -> i32 {
        match self {
            Direction::West => -1,
            Direction::East => 1,
            _ => 0,
        }
    }

    /// `getStepZ()`.
    #[must_use]
    pub fn step_z(self) -> i32 {
        match self {
            Direction::North => -1,
            Direction::South => 1,
            _ => 0,
        }
    }

    /// `getOpposite()`.
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }

    /// `BlockPos.relative(Direction, int)`.
    #[must_use]
    pub fn relative(self, p: BlockPos, n: i32) -> BlockPos {
        BlockPos {
            x: p.x + self.step_x() * n,
            y: p.y,
            z: p.z + self.step_z() * n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_y_rot_quarters() {
        assert_eq!(Direction::from_y_rot(0.0), Direction::South);
        assert_eq!(Direction::from_y_rot(44.9), Direction::South);
        assert_eq!(Direction::from_y_rot(45.0), Direction::West);
        assert_eq!(Direction::from_y_rot(180.0), Direction::North);
        assert_eq!(Direction::from_y_rot(-90.0), Direction::East);
        assert_eq!(Direction::from_y_rot(270.0), Direction::East);
        assert_eq!(Direction::from_y_rot(-45.0), Direction::South);
        assert_eq!(Direction::from_y_rot(-135.0), Direction::East);
    }
}
