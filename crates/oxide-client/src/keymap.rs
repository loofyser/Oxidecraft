//! The window's physical keys, mapped to the game's own, and the rebinding
//! machine.
//!
//! The table is the source's default binding list (`GameSettings.java:127-151`):
//! W/A/S/D move, Space jumps, left shift sneaks, left control is the sprint
//! key. The chat's keys ride the same table: T and slash open the field
//! (`:139`, `:141`), and Tab, Enter, Backspace and the four arrows are the
//! open field's own editing keys (`GuiChat.keyTyped`:87-138) — the source
//! reads those straight off the device, past the bindings, so they stay
//! physical here too and never rebind. Escape stays unbound — the capture
//! and chat rules route it before this table. The match is by physical
//! position, so the movement keys keep their places on a non-QWERTY
//! layout — winit's `physical_key` is the source's own `Keyboard.KEY_*`
//! position. The source binds many more keys than this client carries; a key
//! with no binding here maps to `None` and is never sent.
//!
//! The bindings themselves live in the settings store
//! ([`GameSettings::bindings`](oxide_game::settings::GameSettings::bindings)):
//! this module only translates through them. [`translate`] maps a physical
//! key to the game key its bound action drives; [`lwjgl_code`] is the
//! winit-to-LWJGL leg the rebinding flow writes through, [`rebind_key`] and
//! [`rebind_button`] are its two writes, and [`conflict_codes`] is the
//! lookup the list re-reads after every write (the
//! `resetKeyBindingArrayAndHash` half of `setOptionKeyBinding`,
//! `GameSettings.java`:244-248).

use std::collections::HashMap;

use oxide_game::chat::LanguageTable;
use oxide_game::input::Key;
use oxide_game::settings::{BINDING_CATEGORY_ORDER, BINDING_TABLE, Binding, GameSettings};
use winit::keyboard::KeyCode;

/// The Escape code: arming a row and pressing it unbinds to 0 instead of
/// binding (`GuiControls.keyTyped`:127-152).
pub const KEY_ESCAPE: i32 = 1;

/// Maps a winit physical key to its LWJGL code, or `None` for a key the
/// source's `Keyboard` never named.
pub fn lwjgl_code(code: KeyCode) -> Option<i32> {
    Some(match code {
        KeyCode::Escape => 1,
        KeyCode::Digit1 => 2,
        KeyCode::Digit2 => 3,
        KeyCode::Digit3 => 4,
        KeyCode::Digit4 => 5,
        KeyCode::Digit5 => 6,
        KeyCode::Digit6 => 7,
        KeyCode::Digit7 => 8,
        KeyCode::Digit8 => 9,
        KeyCode::Digit9 => 10,
        KeyCode::Digit0 => 11,
        KeyCode::Minus => 12,
        KeyCode::Equal => 13,
        KeyCode::Backspace => 14,
        KeyCode::Tab => 15,
        KeyCode::KeyQ => 16,
        KeyCode::KeyW => 17,
        KeyCode::KeyE => 18,
        KeyCode::KeyR => 19,
        KeyCode::KeyT => 20,
        KeyCode::KeyY => 21,
        KeyCode::KeyU => 22,
        KeyCode::KeyI => 23,
        KeyCode::KeyO => 24,
        KeyCode::KeyP => 25,
        KeyCode::BracketLeft => 26,
        KeyCode::BracketRight => 27,
        KeyCode::Enter => 28,
        KeyCode::ControlLeft => 29,
        KeyCode::KeyA => 30,
        KeyCode::KeyS => 31,
        KeyCode::KeyD => 32,
        KeyCode::KeyF => 33,
        KeyCode::KeyG => 34,
        KeyCode::KeyH => 35,
        KeyCode::KeyJ => 36,
        KeyCode::KeyK => 37,
        KeyCode::KeyL => 38,
        KeyCode::Semicolon => 39,
        KeyCode::Quote => 40,
        KeyCode::Backquote => 41,
        KeyCode::ShiftLeft => 42,
        KeyCode::Backslash => 43,
        KeyCode::KeyZ => 44,
        KeyCode::KeyX => 45,
        KeyCode::KeyC => 46,
        KeyCode::KeyV => 47,
        KeyCode::KeyB => 48,
        KeyCode::KeyN => 49,
        KeyCode::KeyM => 50,
        KeyCode::Comma => 51,
        KeyCode::Period => 52,
        KeyCode::Slash => 53,
        KeyCode::ShiftRight => 54,
        KeyCode::AltLeft => 56,
        KeyCode::Space => 57,
        KeyCode::F1 => 59,
        KeyCode::F2 => 60,
        KeyCode::F3 => 61,
        KeyCode::F4 => 62,
        KeyCode::F5 => 63,
        KeyCode::F6 => 64,
        KeyCode::F7 => 65,
        KeyCode::F8 => 66,
        KeyCode::F9 => 67,
        KeyCode::F10 => 68,
        KeyCode::Numpad7 => 71,
        KeyCode::Numpad8 => 72,
        KeyCode::Numpad9 => 73,
        KeyCode::Numpad4 => 75,
        KeyCode::Numpad5 => 76,
        KeyCode::Numpad6 => 77,
        KeyCode::Numpad1 => 79,
        KeyCode::Numpad2 => 80,
        KeyCode::Numpad3 => 81,
        KeyCode::Numpad0 => 82,
        KeyCode::F11 => 87,
        KeyCode::F12 => 88,
        KeyCode::NumpadEnter => 156,
        KeyCode::ControlRight => 157,
        KeyCode::AltRight => 184,
        KeyCode::ArrowUp => 200,
        KeyCode::ArrowLeft => 203,
        KeyCode::ArrowRight => 205,
        KeyCode::ArrowDown => 208,
        _ => return None,
    })
}

/// The game key a bound action name drives, or `None` for an action the
/// client carries no key for (the mouse-button attacks, the stream rows and
/// their inert company).
fn key_for_action(name: &str) -> Option<Key> {
    Some(match name {
        "forward" => Key::W,
        "left" => Key::A,
        "back" => Key::S,
        "right" => Key::D,
        "jump" => Key::Space,
        "sneak" => Key::ShiftLeft,
        "sprint" => Key::ControlLeft,
        "chat" => Key::T,
        "command" => Key::Slash,
        // The screenshot row reads through the store's table like every other
        // row, so the bindings list owns it: a later pass can drive every
        // action from data without touching the call sites.
        "screenshot" => Key::Screenshot,
        "inventory" => Key::E,
        "drop" => Key::Q,
        "hotbar.1" => Key::Digit1,
        "hotbar.2" => Key::Digit2,
        "hotbar.3" => Key::Digit3,
        "hotbar.4" => Key::Digit4,
        "hotbar.5" => Key::Digit5,
        "hotbar.6" => Key::Digit6,
        "hotbar.7" => Key::Digit7,
        "hotbar.8" => Key::Digit8,
        "hotbar.9" => Key::Digit9,
        _ => return None,
    })
}

/// The open field's own keys, read straight off the device past the
/// bindings (`GuiChat.keyTyped`:87-138): Tab completes, Enter sends,
/// Backspace deletes, the arrows move the cursor and recall, and the numpad
/// answers its main-row twins.
fn fixed_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::Tab => Key::Tab,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::Numpad1 => Key::Digit1,
        KeyCode::Numpad2 => Key::Digit2,
        KeyCode::Numpad3 => Key::Digit3,
        KeyCode::Numpad4 => Key::Digit4,
        KeyCode::Numpad5 => Key::Digit5,
        KeyCode::Numpad6 => Key::Digit6,
        KeyCode::Numpad7 => Key::Digit7,
        KeyCode::Numpad8 => Key::Digit8,
        KeyCode::Numpad9 => Key::Digit9,
        _ => return None,
    })
}

/// The first binding holding `code` in table order, or `None` when nothing
/// does (a conflict reads the first row; the list paints every holder red).
pub fn action_for_code(bindings: &[Binding], code: i32) -> Option<&str> {
    bindings
        .iter()
        .find(|binding| binding.code == code)
        .map(|binding| binding.name)
}

/// Translates one physical key through the store's bindings, or `None`
/// when the client binds nothing to it.
pub fn translate(code: KeyCode, bindings: &[Binding]) -> Option<Key> {
    if let Some(key) = fixed_key(code) {
        return Some(key);
    }
    let lwjgl = lwjgl_code(code)?;
    let name = action_for_code(bindings, lwjgl)?;
    key_for_action(name)
}

/// The English label per binding name (`en_US.lang`'s `key.*` rows): the
/// controls list's fallback when the handed table holds no translation.
pub const BINDING_LABELS: [(&str, &str); 33] = [
    ("attack", "Attack/Destroy"),
    ("use", "Use Item/Place Block"),
    ("forward", "Walk Forwards"),
    ("left", "Strafe Left"),
    ("back", "Walk Backwards"),
    ("right", "Strafe Right"),
    ("jump", "Jump"),
    ("sneak", "Sneak"),
    ("sprint", "Sprint"),
    ("drop", "Drop Item"),
    ("inventory", "Inventory"),
    ("chat", "Open Chat"),
    ("playerlist", "List Players"),
    ("pickItem", "Pick Block"),
    ("command", "Open Command"),
    ("screenshot", "Take Screenshot"),
    ("togglePerspective", "Toggle Perspective"),
    ("smoothCamera", "Toggle Cinematic Camera"),
    ("streamStartStop", "Start/Stop Stream"),
    ("streamPauseUnpause", "Pause/Unpause Stream"),
    ("streamCommercial", "Show Stream Commercials"),
    ("streamToggleMic", "Push To Talk/Mute"),
    ("fullscreen", "Toggle Fullscreen"),
    ("spectatorOutlines", "Highlight Players (Spectators)"),
    ("hotbar.1", "Hotbar Slot 1"),
    ("hotbar.2", "Hotbar Slot 2"),
    ("hotbar.3", "Hotbar Slot 3"),
    ("hotbar.4", "Hotbar Slot 4"),
    ("hotbar.5", "Hotbar Slot 5"),
    ("hotbar.6", "Hotbar Slot 6"),
    ("hotbar.7", "Hotbar Slot 7"),
    ("hotbar.8", "Hotbar Slot 8"),
    ("hotbar.9", "Hotbar Slot 9"),
];

/// The binding's label through the handed table, falling back to English.
pub fn binding_label(name: &str, language: Option<&LanguageTable>) -> String {
    let english = BINDING_LABELS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, label)| *label)
        .unwrap_or(name);
    let key = format!("key.{name}");
    match language {
        Some(table) => table.get(&key).unwrap_or(english).to_string(),
        None => english.to_string(),
    }
}

/// The binding's category rank in [`BINDING_CATEGORY_ORDER`]: the list's
/// major sort key (`KeyBinding.compareTo`'s localized category).
fn category_rank(category: &str) -> usize {
    BINDING_CATEGORY_ORDER
        .iter()
        .position(|known| *known == category)
        .unwrap_or(BINDING_CATEGORY_ORDER.len())
}

/// The binding names in list order: the localized category, then the label
/// (`KeyBinding.compareTo`, `KeyBinding.java`:143-151).
pub fn sorted_binding_names(language: Option<&LanguageTable>) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = BINDING_TABLE.iter().map(|(name, _, _)| *name).collect();
    names.sort_by_key(|name| {
        let category = GameSettings::binding_category(name).unwrap_or("");
        (category_rank(category), binding_label(name, language))
    });
    names
}

/// Writes `code` into `name`'s row: Escape unbinds to 0, any other code
/// binds (`GuiControls.keyTyped`:127-152). Answers false for an unknown
/// name and writes nothing.
pub fn rebind_key(settings: &mut GameSettings, name: &str, code: i32) -> bool {
    let code = if code == KEY_ESCAPE { 0 } else { code };
    settings.set_binding(name, code)
}

/// Writes a click into `name`'s row: `-100 + button`
/// (`GuiKeyBindingList.mouseClicked`:98-110). Answers false for an unknown
/// name and writes nothing.
pub fn rebind_button(settings: &mut GameSettings, name: &str, button: u8) -> bool {
    settings.set_binding(name, -100 + i32::from(button))
}

/// Restores one row's default (`GuiKeyBindingList.java`:164-169). Answers
/// false for an unknown name and writes nothing.
pub fn reset_binding(settings: &mut GameSettings, name: &str) -> bool {
    match GameSettings::default_binding(name) {
        Some(default) => settings.set_binding(name, default),
        None => false,
    }
}

/// The nonzero codes two or more rows share, each with its holders: the
/// conflict map the list rebuilds after every write — red labels only,
/// never blocked (`GuiKeyBindingList.java`:132-153).
pub fn conflict_codes(bindings: &[Binding]) -> HashMap<i32, Vec<&str>> {
    let mut holders: HashMap<i32, Vec<&str>> = HashMap::new();
    for binding in bindings {
        if binding.code != 0 {
            holders.entry(binding.code).or_default().push(binding.name);
        }
    }
    holders.retain(|_, names| names.len() > 1);
    holders
}

/// Names one LWJGL code the way the change button does
/// (`KeyBinding.getKeyDisplayString`:223-228 over `Keyboard.getKeyName` and
/// `Mouse.getButtonName`): `Button N` for a mouse button, `NONE` for
/// unbound, the keyboard name otherwise.
pub fn display_code(code: i32) -> String {
    if code < 0 {
        return format!("Button {}", code + 101);
    }
    if code == 0 {
        return String::from("NONE");
    }
    match code {
        1 => String::from("ESC"),
        2 => String::from("1"),
        3 => String::from("2"),
        4 => String::from("3"),
        5 => String::from("4"),
        6 => String::from("5"),
        7 => String::from("6"),
        8 => String::from("7"),
        9 => String::from("8"),
        10 => String::from("9"),
        11 => String::from("0"),
        12 => String::from("MINUS"),
        13 => String::from("EQUALS"),
        14 => String::from("BACK"),
        15 => String::from("TAB"),
        16 => String::from("Q"),
        17 => String::from("W"),
        18 => String::from("E"),
        19 => String::from("R"),
        20 => String::from("T"),
        21 => String::from("Y"),
        22 => String::from("U"),
        23 => String::from("I"),
        24 => String::from("O"),
        25 => String::from("P"),
        26 => String::from("LBRACKET"),
        27 => String::from("RBRACKET"),
        28 => String::from("RETURN"),
        29 => String::from("LCONTROL"),
        30 => String::from("A"),
        31 => String::from("S"),
        32 => String::from("D"),
        33 => String::from("F"),
        34 => String::from("G"),
        35 => String::from("H"),
        36 => String::from("J"),
        37 => String::from("K"),
        38 => String::from("L"),
        39 => String::from("SEMICOLON"),
        40 => String::from("APOSTROPHE"),
        41 => String::from("GRAVE"),
        42 => String::from("LSHIFT"),
        43 => String::from("BACKSLASH"),
        44 => String::from("Z"),
        45 => String::from("X"),
        46 => String::from("C"),
        47 => String::from("V"),
        48 => String::from("B"),
        49 => String::from("N"),
        50 => String::from("M"),
        51 => String::from("COMMA"),
        52 => String::from("PERIOD"),
        53 => String::from("SLASH"),
        54 => String::from("RSHIFT"),
        56 => String::from("LMENU"),
        57 => String::from("SPACE"),
        59 => String::from("F1"),
        60 => String::from("F2"),
        61 => String::from("F3"),
        62 => String::from("F4"),
        63 => String::from("F5"),
        64 => String::from("F6"),
        65 => String::from("F7"),
        66 => String::from("F8"),
        67 => String::from("F9"),
        68 => String::from("F10"),
        87 => String::from("F11"),
        88 => String::from("F12"),
        156 => String::from("NUMPADENTER"),
        157 => String::from("RCONTROL"),
        184 => String::from("RMENU"),
        200 => String::from("UP"),
        203 => String::from("LEFT"),
        205 => String::from("RIGHT"),
        208 => String::from("DOWN"),
        _ => format!("KEY{code}"),
    }
}

#[cfg(test)]
mod tests {
    //! The data-driven binding table and the rebinding machine.

    use oxide_game::settings::default_bindings;

    use super::{
        KEY_ESCAPE, action_for_code, binding_label, conflict_codes, display_code, fixed_key,
        key_for_action, lwjgl_code, rebind_button, rebind_key, reset_binding, sorted_binding_names,
        translate,
    };
    use oxide_game::input::Key;
    use winit::keyboard::KeyCode;

    #[test]
    fn the_default_bindings_drive_the_game_keys() {
        // The source's default bindings (`GameSettings.java:127-133`): the
        // movement keys and the sprint key, by physical position, read
        // through the store.
        let bindings = default_bindings();
        assert_eq!(translate(KeyCode::KeyW, &bindings), Some(Key::W));
        assert_eq!(translate(KeyCode::KeyA, &bindings), Some(Key::A));
        assert_eq!(translate(KeyCode::KeyS, &bindings), Some(Key::S));
        assert_eq!(translate(KeyCode::KeyD, &bindings), Some(Key::D));
        assert_eq!(translate(KeyCode::Space, &bindings), Some(Key::Space));
        assert_eq!(
            translate(KeyCode::ShiftLeft, &bindings),
            Some(Key::ShiftLeft)
        );
        assert_eq!(
            translate(KeyCode::ControlLeft, &bindings),
            Some(Key::ControlLeft)
        );
    }

    #[test]
    fn an_unbound_key_maps_to_nothing() {
        // A key no binding holds maps to nothing and never reaches the
        // session. Escape is not bound either: the capture and chat rules
        // route it before this table.
        let bindings = default_bindings();
        assert_eq!(translate(KeyCode::Escape, &bindings), None);
        assert_eq!(translate(KeyCode::ShiftRight, &bindings), None);
        assert_eq!(translate(KeyCode::ControlRight, &bindings), None);
        assert_eq!(translate(KeyCode::KeyF, &bindings), None);
    }

    #[test]
    fn the_chat_and_editing_keys_map_to_the_game_keys() {
        // The chat's openers read through their bindings (`keyBindChat` 20
        // and `keyBindCommand` 53, `GameSettings.java`:139, `:141`); the
        // field's editing keys stay physical past the bindings
        // (`GuiChat.keyTyped`:91-137).
        let bindings = default_bindings();
        assert_eq!(translate(KeyCode::KeyT, &bindings), Some(Key::T));
        assert_eq!(translate(KeyCode::Slash, &bindings), Some(Key::Slash));
        assert_eq!(translate(KeyCode::Tab, &bindings), Some(Key::Tab));
        assert_eq!(translate(KeyCode::Enter, &bindings), Some(Key::Enter));
        assert_eq!(translate(KeyCode::NumpadEnter, &bindings), Some(Key::Enter));
        assert_eq!(
            translate(KeyCode::Backspace, &bindings),
            Some(Key::Backspace)
        );
        assert_eq!(
            translate(KeyCode::ArrowLeft, &bindings),
            Some(Key::ArrowLeft)
        );
        assert_eq!(
            translate(KeyCode::ArrowRight, &bindings),
            Some(Key::ArrowRight)
        );
        assert_eq!(translate(KeyCode::ArrowUp, &bindings), Some(Key::ArrowUp));
        assert_eq!(
            translate(KeyCode::ArrowDown, &bindings),
            Some(Key::ArrowDown)
        );
    }

    #[test]
    fn the_inventory_and_drop_and_hotbar_keys_map_to_the_game_keys() {
        // E opens (`keyBindInventory`, `:134`, code 18), Q drops
        // (`keyBindDrop`, `:136`, code 16) and the digits 1–9 are the
        // hotbar bindings (`keyBindsHotbar[0..8]`, `:151`, codes 2–10) —
        // the top row through the store, the numpad physical.
        let bindings = default_bindings();
        assert_eq!(translate(KeyCode::KeyE, &bindings), Some(Key::E));
        assert_eq!(translate(KeyCode::KeyQ, &bindings), Some(Key::Q));
        assert_eq!(translate(KeyCode::Digit1, &bindings), Some(Key::Digit1));
        assert_eq!(translate(KeyCode::Digit3, &bindings), Some(Key::Digit3));
        assert_eq!(translate(KeyCode::Digit9, &bindings), Some(Key::Digit9));
        assert_eq!(translate(KeyCode::Numpad1, &bindings), Some(Key::Digit1));
        assert_eq!(translate(KeyCode::Numpad5, &bindings), Some(Key::Digit5));
        assert_eq!(translate(KeyCode::Numpad9, &bindings), Some(Key::Digit9));
    }

    #[test]
    fn the_screenshot_key_maps_through_its_binding() {
        // F2 captures (`keyBindScreenshot`, `GameSettings.java:142`, code
        // 60) through the store's table like every other row, so rebinding
        // the row moves the capture key with it.
        let bindings = default_bindings();
        assert_eq!(translate(KeyCode::F2, &bindings), Some(Key::Screenshot));
        let mut settings = oxide_game::settings::GameSettings::default();
        assert!(rebind_key(&mut settings, "screenshot", 33));
        assert_eq!(
            translate(KeyCode::KeyF, &settings.bindings),
            Some(Key::Screenshot)
        );
        assert_eq!(translate(KeyCode::F2, &settings.bindings), None);
    }

    #[test]
    fn a_rebound_key_drives_its_new_action() {
        // The table is data-driven: rebinding forward to F moves W's game
        // key onto the F physical key.
        let mut settings = oxide_game::settings::GameSettings::default();
        assert!(rebind_key(&mut settings, "forward", 33));
        assert_eq!(translate(KeyCode::KeyF, &settings.bindings), Some(Key::W));
        assert_eq!(translate(KeyCode::KeyW, &settings.bindings), None);
    }

    #[test]
    fn escape_unbinds_and_clicks_bind_negative() {
        // `GuiControls.keyTyped`: Escape on an armed row writes 0;
        // `GuiKeyBindingList.mouseClicked`: a click writes `-100 + button`.
        let mut settings = oxide_game::settings::GameSettings::default();
        assert!(rebind_key(&mut settings, "forward", KEY_ESCAPE));
        assert_eq!(settings.binding("forward"), 0);
        assert!(rebind_button(&mut settings, "attack", 0));
        assert_eq!(settings.binding("attack"), -100);
        assert!(rebind_button(&mut settings, "use", 1));
        assert_eq!(settings.binding("use"), -99);
        assert!(rebind_button(&mut settings, "pickItem", 2));
        assert_eq!(settings.binding("pickItem"), -98);
        assert!(!rebind_key(&mut settings, "no_such_key", 33));
        assert!(!rebind_button(&mut settings, "no_such_key", 0));
    }

    #[test]
    fn conflicts_hold_every_sharer_and_reset_restores_rows() {
        // A nonzero code two rows share is a conflict — red labels only,
        // never blocked — and each row resets to its own default.
        let mut settings = oxide_game::settings::GameSettings::default();
        assert!(conflict_codes(&settings.bindings).is_empty());
        assert!(rebind_key(&mut settings, "sprint", 17));
        let conflicts = conflict_codes(&settings.bindings);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[&17], vec!["forward", "sprint"]);
        assert!(reset_binding(&mut settings, "sprint"));
        assert_eq!(settings.binding("sprint"), 29);
        assert!(conflict_codes(&settings.bindings).is_empty());
        assert!(!reset_binding(&mut settings, "no_such_key"));
        // Zero never conflicts, however many rows hold it.
        assert!(!conflict_codes(&settings.bindings).contains_key(&0));
    }

    #[test]
    fn the_list_sorts_by_category_then_label() {
        // `KeyBinding.compareTo` over the English table: Gameplay, then
        // Inventory, Miscellaneous, Movement, Multiplayer, Streaming.
        let names = sorted_binding_names(None);
        assert_eq!(names.len(), 33);
        assert_eq!(&names[0..4], &["attack", "drop", "pickItem", "use"]);
        assert_eq!(
            &names[4..14],
            &[
                "hotbar.1",
                "hotbar.2",
                "hotbar.3",
                "hotbar.4",
                "hotbar.5",
                "hotbar.6",
                "hotbar.7",
                "hotbar.8",
                "hotbar.9",
                "inventory"
            ]
        );
        assert_eq!(
            &names[14..19],
            &[
                "spectatorOutlines",
                "screenshot",
                "smoothCamera",
                "fullscreen",
                "togglePerspective"
            ]
        );
        assert_eq!(
            &names[19..26],
            &[
                "jump", "sneak", "sprint", "left", "right", "back", "forward"
            ]
        );
        assert_eq!(&names[26..29], &["playerlist", "chat", "command"]);
        assert_eq!(
            &names[29..33],
            &[
                "streamPauseUnpause",
                "streamToggleMic",
                "streamCommercial",
                "streamStartStop"
            ]
        );
    }

    #[test]
    fn the_change_button_names_codes() {
        // `getKeyDisplayString`: mouse buttons read `Button N`, 0 reads
        // NONE, keys read their keyboard names.
        assert_eq!(display_code(-100), "Button 1");
        assert_eq!(display_code(-99), "Button 2");
        assert_eq!(display_code(-98), "Button 3");
        assert_eq!(display_code(0), "NONE");
        assert_eq!(display_code(17), "W");
        assert_eq!(display_code(57), "SPACE");
        assert_eq!(display_code(63), "F5");
        assert_eq!(display_code(15), "TAB");
        assert_eq!(binding_label("forward", None), "Walk Forwards");
        assert_eq!(binding_label("hotbar.9", None), "Hotbar Slot 9");
        assert_eq!(action_for_code(&default_bindings(), 17), Some("forward"));
        assert_eq!(action_for_code(&default_bindings(), 99), None);
        assert_eq!(lwjgl_code(KeyCode::KeyW), Some(17));
        assert_eq!(lwjgl_code(KeyCode::Space), Some(57));
        assert_eq!(lwjgl_code(KeyCode::Escape), Some(KEY_ESCAPE));
        assert_eq!(key_for_action("forward"), Some(Key::W));
        assert_eq!(key_for_action("attack"), None);
        assert_eq!(fixed_key(KeyCode::Tab), Some(Key::Tab));
        assert_eq!(fixed_key(KeyCode::KeyW), None);
    }

    #[test]
    fn rebound_bindings_round_trip_through_the_store_file() {
        // The rebinding writes persist through the same save path — no
        // second file — and reload clamped.
        let path = std::env::temp_dir().join(format!(
            "oxide-keymap-rebind-{}-{}.toml",
            std::process::id(),
            "roundtrip"
        ));
        let mut settings = oxide_game::settings::GameSettings::default();
        assert!(rebind_key(&mut settings, "forward", 33));
        assert!(rebind_key(&mut settings, "jump", KEY_ESCAPE));
        settings
            .save(&path)
            .expect("the store writes through the same path");
        let loaded = oxide_game::settings::GameSettings::load(&path);
        assert_eq!(loaded, settings);
        assert_eq!(translate(KeyCode::KeyF, &loaded.bindings), Some(Key::W));
        assert_eq!(translate(KeyCode::Space, &loaded.bindings), None);
        let _ = std::fs::remove_file(&path);
    }
}
