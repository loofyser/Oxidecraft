//! The player state the session's ticks carry.

use crate::input::SprintTap;

/// The standing eye height above the player's feet, in blocks.
///
/// `EntityPlayer.getEyeHeight` (`EntityPlayer.java:2326-2340`) bases the eye
/// on `float f = 1.62F`. The source lowers it to 0.2 while sleeping and by
/// 0.08 while sneaking (`:2330-2337`); M3 carries the standing value, and the
/// sleeping eye arrives with M6's screens.
const EYE_HEIGHT: f64 = 1.62;

/// The player: the state one tick steps and the window draws.
///
/// The fields are the surface the later M3 tasks extend — each extension is
/// declared in its own task. `position`, `last_tick_position`, `tick`, the
/// look and the flags are what the per-tick `PlayerTick` event reports;
/// `motion` and `on_ground` are the physics core's input (Task 2);
/// `flying` and `in_water` gate the movement rules (Tasks 2, 3).
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// The feet position in the world.
    pub position: [f64; 3],
    /// The feet position at the previous tick.
    pub last_tick_position: [f64; 3],
    /// Look yaw in degrees.
    pub yaw: f32,
    /// Look pitch in degrees, positive looking down.
    pub pitch: f32,
    /// Velocity in blocks per tick.
    pub motion: [f64; 3],
    /// Whether the player stands on something.
    pub on_ground: bool,
    /// Whether the player is sprinting.
    pub sprinting: bool,
    /// Whether the player is sneaking.
    pub sneaking: bool,
    /// Whether the player is flying.
    pub flying: bool,
    /// Whether the player is in water.
    pub in_water: bool,
    /// Ticks since the session started.
    pub tick: u64,
    /// The double-tap window the sprint binding keeps across ticks.
    pub sprint_tap: SprintTap,
}

impl Player {
    /// A new player at the origin, looking south, nothing held.
    pub fn new() -> Player {
        Player {
            position: [0.0; 3],
            last_tick_position: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            motion: [0.0; 3],
            on_ground: false,
            sprinting: false,
            sneaking: false,
            flying: false,
            in_water: false,
            tick: 0,
            sprint_tap: SprintTap::default(),
        }
    }

    /// The eye's height above the feet, in blocks.
    ///
    /// The one place the constant is defined; the interaction raycast
    /// (Task 7) and the camera (Task 11) both consume it.
    pub fn eye_height(&self) -> f64 {
        EYE_HEIGHT
    }
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

#[cfg(test)]
mod tests {
    //! The pinned literals of the player state.

    use super::Player;

    #[test]
    fn a_new_player_is_at_the_origin_and_the_eye_is_the_sources_own_height() {
        let player = Player::new();
        assert_eq!(player.position, [0.0, 0.0, 0.0]);
        assert_eq!(player.last_tick_position, [0.0, 0.0, 0.0]);
        assert_eq!(player.motion, [0.0, 0.0, 0.0]);
        assert_eq!(player.tick, 0);
        assert!(!player.on_ground);
        assert!(!player.sprinting);
        assert!(!player.sneaking);
        assert!(!player.flying);
        assert!(!player.in_water);
        // `EntityPlayer.java:2326-2340`: `float f = 1.62F`.
        assert_eq!(player.eye_height(), 1.62);
    }
}
