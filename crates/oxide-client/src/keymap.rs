//! The window's physical keys, mapped to the game's own.
//!
//! The table is the source's default binding list (`GameSettings.java:127-133`):
//! W/A/S/D move, Space jumps, left shift sneaks, left control is the sprint
//! key. The match is by physical position, so the movement keys keep their
//! places on a non-QWERTY layout — winit's `physical_key` is the source's own
//! `Keyboard.KEY_*` position. The source binds many more keys than this client
//! carries; a key with no binding here maps to `None` and is never sent.

use oxide_game::input::Key;
use winit::keyboard::KeyCode;

/// Translates one physical key, or `None` when the client binds nothing to it.
pub(crate) fn translate(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::KeyW => Some(Key::W),
        KeyCode::KeyA => Some(Key::A),
        KeyCode::KeyS => Some(Key::S),
        KeyCode::KeyD => Some(Key::D),
        KeyCode::Space => Some(Key::Space),
        KeyCode::ShiftLeft => Some(Key::ShiftLeft),
        KeyCode::ControlLeft => Some(Key::ControlLeft),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! The binding table: every bound physical key, and unbound ones.

    use super::translate;
    use oxide_game::input::Key;
    use winit::keyboard::KeyCode;

    #[test]
    fn the_bound_physical_keys_map_to_the_game_keys() {
        // The source's default bindings (`GameSettings.java:127-133`): the
        // movement keys and the sprint key, by physical position.
        assert_eq!(translate(KeyCode::KeyW), Some(Key::W));
        assert_eq!(translate(KeyCode::KeyA), Some(Key::A));
        assert_eq!(translate(KeyCode::KeyS), Some(Key::S));
        assert_eq!(translate(KeyCode::KeyD), Some(Key::D));
        assert_eq!(translate(KeyCode::Space), Some(Key::Space));
        assert_eq!(translate(KeyCode::ShiftLeft), Some(Key::ShiftLeft));
        assert_eq!(translate(KeyCode::ControlLeft), Some(Key::ControlLeft));
    }

    #[test]
    fn an_unbound_key_maps_to_nothing() {
        // Movement keys are bound; Q, the right-hand modifiers and Escape are
        // not, and must never reach the session.
        assert_eq!(translate(KeyCode::KeyQ), None);
        assert_eq!(translate(KeyCode::Escape), None);
        assert_eq!(translate(KeyCode::ShiftRight), None);
        assert_eq!(translate(KeyCode::ControlRight), None);
    }
}
