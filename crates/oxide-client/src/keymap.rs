//! The window's physical keys, mapped to the game's own.
//!
//! The table is the source's default binding list (`GameSettings.java:127-133`):
//! W/A/S/D move, Space jumps, left shift sneaks, left control is the sprint
//! key. The chat's keys ride the same table: T and slash open the field
//! (`:139`, `:141`), and Tab, Enter, Backspace and the four arrows are the
//! open field's own editing keys (`GuiChat.keyTyped`:87-138); the gameplay
//! path filters them out (`Key::is_gameplay`), and Escape stays unbound — the
//! capture and chat rules route it before this table. The match is by
//! physical position, so the movement keys keep their places on a non-QWERTY
//! layout — winit's `physical_key` is the source's own `Keyboard.KEY_*`
//! position. The source binds many more keys than this client carries; a key
//! with no binding here maps to `None` and is never sent.

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
        // The chat's keys: the two openers (`GameSettings.java`:139, `:141`)
        // and the editing keys the open field reads (`GuiChat.keyTyped`:87-138).
        KeyCode::KeyT => Some(Key::T),
        KeyCode::Slash => Some(Key::Slash),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::NumpadEnter => Some(Key::Enter),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::ArrowLeft => Some(Key::ArrowLeft),
        KeyCode::ArrowRight => Some(Key::ArrowRight),
        KeyCode::ArrowUp => Some(Key::ArrowUp),
        KeyCode::ArrowDown => Some(Key::ArrowDown),
        // The inventory keys Task 24 routes: E opens (`keyBindInventory`,
        // `GameSettings.java`:134), Q drops (`keyBindDrop`, `:136`) and the
        // digits 1–9 are the hotbar bindings (`keyBindsHotbar[0..8]`, `:151`,
        // key codes 2–10) — the top row and the numpad alike.
        KeyCode::KeyE => Some(Key::E),
        KeyCode::KeyQ => Some(Key::Q),
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(Key::Digit1),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(Key::Digit2),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(Key::Digit3),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(Key::Digit4),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(Key::Digit5),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some(Key::Digit6),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some(Key::Digit7),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some(Key::Digit8),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some(Key::Digit9),
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
        // Movement keys are bound; the right-hand modifiers are not, and must
        // never reach the session. Escape is not bound either: the capture
        // and chat rules route it before this table.
        assert_eq!(translate(KeyCode::Escape), None);
        assert_eq!(translate(KeyCode::ShiftRight), None);
        assert_eq!(translate(KeyCode::ControlRight), None);
    }

    #[test]
    fn the_chat_and_editing_keys_map_to_the_game_keys() {
        // The chat's openers and the field's editing keys, by physical
        // position: T and slash are the source's chat and command keys
        // (`GameSettings.java`:139, `:141`), and the rest are the keys
        // `GuiChat.keyTyped` reads — Tab, Enter (the main and keypad ones),
        // Backspace and the four arrows (`GuiChat.java`:91-137).
        assert_eq!(translate(KeyCode::KeyT), Some(Key::T));
        assert_eq!(translate(KeyCode::Slash), Some(Key::Slash));
        assert_eq!(translate(KeyCode::Tab), Some(Key::Tab));
        assert_eq!(translate(KeyCode::Enter), Some(Key::Enter));
        assert_eq!(translate(KeyCode::NumpadEnter), Some(Key::Enter));
        assert_eq!(translate(KeyCode::Backspace), Some(Key::Backspace));
        assert_eq!(translate(KeyCode::ArrowLeft), Some(Key::ArrowLeft));
        assert_eq!(translate(KeyCode::ArrowRight), Some(Key::ArrowRight));
        assert_eq!(translate(KeyCode::ArrowUp), Some(Key::ArrowUp));
        assert_eq!(translate(KeyCode::ArrowDown), Some(Key::ArrowDown));
    }

    #[test]
    fn the_inventory_and_drop_and_hotbar_keys_map_to_the_game_keys() {
        // The inventory keys Task 24 routes: E opens (`keyBindInventory`,
        // `GameSettings.java`:134, key code 18), Q drops (`keyBindDrop`,
        // `:136`, key code 16) and the digits 1–9 are the hotbar bindings
        // (`keyBindsHotbar[0..8]`, `:151`, key codes 2–10) — the top row and
        // the numpad alike.
        assert_eq!(translate(KeyCode::KeyE), Some(Key::E));
        assert_eq!(translate(KeyCode::KeyQ), Some(Key::Q));
        assert_eq!(translate(KeyCode::Digit1), Some(Key::Digit1));
        assert_eq!(translate(KeyCode::Digit2), Some(Key::Digit2));
        assert_eq!(translate(KeyCode::Digit3), Some(Key::Digit3));
        assert_eq!(translate(KeyCode::Digit4), Some(Key::Digit4));
        assert_eq!(translate(KeyCode::Digit5), Some(Key::Digit5));
        assert_eq!(translate(KeyCode::Digit6), Some(Key::Digit6));
        assert_eq!(translate(KeyCode::Digit7), Some(Key::Digit7));
        assert_eq!(translate(KeyCode::Digit8), Some(Key::Digit8));
        assert_eq!(translate(KeyCode::Digit9), Some(Key::Digit9));
        assert_eq!(translate(KeyCode::Numpad1), Some(Key::Digit1));
        assert_eq!(translate(KeyCode::Numpad5), Some(Key::Digit5));
        assert_eq!(translate(KeyCode::Numpad9), Some(Key::Digit9));
    }
}
