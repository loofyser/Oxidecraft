//! The input path: what the window reports, the held intent it builds, and
//! the look mapping.
//!
//! The window forwards physical key edges, mouse deltas and mouse button
//! edges as [`InputEvent`]s; the session drains them into the held [`Intent`]
//! its ticks consume. The bindings are vanilla's own (`GameSettings.java:127-133`,
//! `MovementInputFromOptions.updatePlayerMoveState`, `:14-46`): W/A/S/D move,
//! Space jumps, ShiftLeft sneaks — which scales both movement axes by 0.3
//! (`:42-46`) — and ControlLeft sprints, as does a double-tap of W inside the
//! source's seven-tick window (`EntityPlayerSP.onLivingUpdate`, `:780-820`).
//!
//! The look mapping is `EntityRenderer.updateMouse` (`:1094-1123`) followed by
//! `Entity.setAngles` (`Entity.java:389-398`): the device delta is scaled by
//! `f1 = (sensitivity × 0.6 + 0.2)³ × 8` and applied with the source's ×0.15
//! factor, the yaw added and the pitch subtracted, the pitch clamped to
//! ±90°. `InputEvent::MouseDelta` carries the window's screen convention —
//! `dx` rightward, `dy` downward — where the source's device delta grows
//! upward, so its subtracted pitch term is this convention's added one; the
//! direction test below pins the whole chain.

use oxide_proto_v47::entity::MetadataItem;

/// One physical key the client binds.
///
/// The source binds many more; these are the movement keys and the sprint key
/// M3's input surface names (`GameSettings.java:127-133`) plus the chat keys
/// M4 adds: the two openers (`keyBindChat`, `keyBindCommand` — `:139`, `:141`)
/// and the editing keys the open chat field reads (`GuiChat.keyTyped`:87-138).
/// Task 24 adds the inventory keys: E opens the inventory (`keyBindInventory`,
/// `:134`, key code 18), Q drops (`keyBindDrop`, `:136`, key code 16) and the
/// digits 1–9 are the hotbar bindings (`keyBindsHotbar[0..8]`, `:151`, key
/// codes 2–10). Escape is not here: the capture and chat rules route it before
/// the key table, so it needs no slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Forward: `keyBindForward`, key code 17.
    W,
    /// Strafe left: `keyBindLeft`, key code 30.
    A,
    /// Backward: `keyBindBack`, key code 31.
    S,
    /// Strafe right: `keyBindRight`, key code 32.
    D,
    /// Jump: `keyBindJump`, key code 57.
    Space,
    /// Sneak: `keyBindSneak`, key code 42.
    ShiftLeft,
    /// Sprint: `keyBindSprint`, key code 29.
    ControlLeft,
    /// The chat key: `keyBindChat`, key code 20; T opens the field.
    T,
    /// The command key: `keyBindCommand`, key code 53; `/` opens the field
    /// with its slash.
    Slash,
    /// Tab: the field's completion key, consumed and deferred (`GuiChat.keyTyped`:91-94).
    Tab,
    /// Enter: the field's send key (`:104-137`).
    Enter,
    /// Backspace: the field's delete key (`GuiTextField.textboxKeyTyped`:378-391).
    Backspace,
    /// The left arrow: the field's cursor moves back (`GuiTextField.textboxKeyTyped`:405-426).
    ArrowLeft,
    /// The right arrow: the field's cursor moves on (`GuiTextField.textboxKeyTyped`:428-449).
    ArrowRight,
    /// The up arrow: the field recalls its last sent message (`GuiChat.getSentHistory`:275-292).
    ArrowUp,
    /// The down arrow: the recall walks forward again (`GuiChat.getSentHistory`:275-292).
    ArrowDown,
    /// The inventory key: `keyBindInventory`, key code 18. A fresh press with
    /// no screen open stands the inventory beside one C16 (`Minecraft.java`
    /// :2092-2101); behind a container screen the same press swaps it to the
    /// inventory (the close at `GuiContainer.keyTyped`:692-696 never unpresses,
    /// so the unguarded loop consumes the same press).
    E,
    /// The drop key: `keyBindDrop`, key code 16. Outside screens a press drops
    /// one item, the whole stack with Ctrl held (`Minecraft.java`:2105-2111
    /// over `EntityPlayerSP.dropOneItem`:279-284); over a container the
    /// screen's own drop routing answers instead.
    Q,
    /// Hotbar 1: `keyBindsHotbar[0]`, key code 2.
    Digit1,
    /// Hotbar 2: `keyBindsHotbar[1]`, key code 3.
    Digit2,
    /// Hotbar 3: `keyBindsHotbar[2]`, key code 4.
    Digit3,
    /// Hotbar 4: `keyBindsHotbar[3]`, key code 5.
    Digit4,
    /// Hotbar 5: `keyBindsHotbar[4]`, key code 6.
    Digit5,
    /// Hotbar 6: `keyBindsHotbar[5]`, key code 7.
    Digit6,
    /// Hotbar 7: `keyBindsHotbar[6]`, key code 8.
    Digit7,
    /// Hotbar 8: `keyBindsHotbar[7]`, key code 9.
    Digit8,
    /// Hotbar 9: `keyBindsHotbar[8]`, key code 10.
    Digit9,
}

/// One mouse button the client binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// The attack and break button: `keyBindAttack`.
    Left,
    /// The use and place button: `keyBindUseItem`.
    Right,
}

/// One input event, as the window reports it.
///
/// The derive drops the `Copy` this enum carried through M3: [`InputEvent::SendChat`]
/// carries owned text, so an event that travels — a queued script flip, a
/// drain that keeps its events — moves or clones rather than being copied
/// implicitly.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A physical key was pressed or released.
    Key {
        /// The key.
        key: Key,
        /// Whether it went down (`true`) or up (`false`).
        pressed: bool,
    },
    /// The mouse moved.
    ///
    /// The delta is in pixels in the window's screen convention: `dx`
    /// rightward, `dy` downward.
    MouseDelta {
        /// The horizontal movement in pixels.
        dx: f64,
        /// The vertical movement in pixels.
        dy: f64,
    },
    /// A mouse button was pressed or released.
    MouseButton {
        /// The button.
        button: MouseButton,
        /// Whether it went down (`true`) or up (`false`).
        pressed: bool,
    },
    /// The window lost focus: every held key is released.
    FocusLost,
    /// The chat field sent a message: the text, exactly as the field's trim
    /// left it.
    ///
    /// The field's non-empty gate and its trim have already run (`GuiChat.keyTyped`:104-137
    /// hands `sendChatMessage` the trimmed text); the session
    /// writes the message as one Chat Message (play id 0x01) and nothing
    /// else. The field's 100-character cap is the field's; the session only
    /// logs a message that slipped past it.
    SendChat {
        /// The message text, exactly as the field sent it.
        text: String,
    },
    /// A slot click on an open container: the view's own click routing —
    /// `GuiContainer.mouseClicked:428-448` and its `handleMouseClick` table —
    /// run against the session's windows.
    ClickWindow {
        /// The window the click is for; window 0 is the player's own.
        window_id: i32,
        /// The slot index the click names, the source's `slotId`; −999 is the
        /// click outside every slot (`GuiContainer.mouseReleased:617`).
        slot: i16,
        /// The clicked button, the source's `clickedButton`; a drag's button
        /// composes the drag's event and mode in its bits
        /// (`Container.func_94534_d:700-703`).
        button: i8,
        /// The click mode, the source's `mode`/`clickType`: 0 pickup and
        /// place, 1 shift quick-move, 2 number-key swap, 3 creative pick,
        /// 4 drop, 5 drag, 6 double-click gather
        /// (`Container.slotClick:140-494`).
        mode: i8,
    },
    /// The view closed a container screen: the source's close path writes
    /// Close Window for the container the screen stood on
    /// (`EntityPlayerSP.closeScreen:330-334`, window 0 included).
    CloseWindow {
        /// The window the screen stood on; window 0 is the player's own.
        window_id: u8,
    },
    /// A creative screen's slot write: the set `PlayerControllerMP.sendSlotPacket:557-563`
    /// sends and the carried stack's own drop `sendPacketDropItem:568-574`
    /// sends.
    CreativeAction {
        /// The slot the write names; −1 is the carried stack's own drop.
        slot: i16,
        /// The stack to write, or `None` to clear the slot.
        item: Option<MetadataItem>,
    },
    /// The enchantment screen's offer click
    /// (`PlayerControllerMP.sendEnchantPacket:549-552`).
    EnchantItem {
        /// The window the enchantment screen stands on.
        window_id: u8,
        /// The offer's zero-based index.
        index: i8,
    },
    /// A custom-payload send on a registered channel: the three container
    /// screens whose confirms ride `C17PacketCustomPayload` rather than a
    /// dedicated packet — the beacon's `MC|Beacon` confirm (two big-endian
    /// i32s, `GuiBeacon.actionPerformed`:137-144), the villager's `MC|TrSel`
    /// page select (one big-endian i32, `GuiMerchant.actionPerformed`:126-132)
    /// and the anvil's `MC|ItemName` rename (the raw string, varint-prefixed
    /// UTF-8 per `PacketBuffer.writeString`, `GuiRepair.renameItem`:136-148).
    /// The payload's framing is the caller's: [`write_plugin_message`](oxide_proto_v47::serverbound::write_plugin_message)
    /// writes the channel and the data verbatim.
    CustomPayload {
        /// The payload's channel, for example `MC|Beacon`.
        channel: String,
        /// The payload's body, already framed for the channel.
        data: Vec<u8>,
    },
    /// The sign editor saved its lines (`GuiEditSign.onGuiClosed:52-60`).
    UpdateSign {
        /// The sign's x coordinate.
        x: i32,
        /// The sign's y coordinate.
        y: i32,
        /// The sign's z coordinate.
        z: i32,
        /// The four lines, each as the editor held it.
        lines: [String; 4],
    },
    /// The player's selected hotbar slot moved
    /// (`PlayerControllerMP.syncCurrentPlayItem:379-388`).
    HeldItemChange {
        /// The new selection, 0 through 8.
        slot: i16,
    },
    /// The drop key (`EntityPlayerSP.dropOneItem:279-284`).
    DropItem {
        /// Whether the whole stack drops (`true`) or one item (`false`).
        whole: bool,
    },
    /// The inventory key's open: one Client Status carrying action 2, the
    /// source's `OPEN_INVENTORY_ACHIEVEMENT` (`Minecraft.java:2090-2103`,
    /// send at :2100, screen display at :2101). The source guards nothing —
    /// every non-riding open sends exactly one — so two opens send two.
    OpenInventory,
}

/// How many physical keys the intent tracks.
///
/// The movement and sprint keys, the chat keys the field rides and the
/// inventory keys Task 24 routes; the count is the [`Key`] variant count so
/// [`Key::index`] stays total, but only the gameplay keys drive any intent.
const KEY_COUNT: usize = 27;

/// The sneak input scale: `MovementInputFromOptions.java:42-46` multiplies
/// both movement axes by 0.3 while sneak is held.
const SNEAK_SCALE: f32 = 0.3;

/// The movement intent one tick consumes.
///
/// The fields are re-derived from the held keys on every edge, so the intent
/// is always the source's `MovementInput.updatePlayerMoveState` result for
/// the keys currently down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intent {
    /// Forward input: W +1, S −1, both held cancel; scaled by sneak.
    pub forward: f32,
    /// Strafe input: A +1, D −1, both held cancel; scaled by sneak.
    pub strafe: f32,
    /// Whether Space is held.
    pub jump: bool,
    /// Whether ShiftLeft is held.
    pub sneak: bool,
    /// Whether ControlLeft is held: the sprint key.
    pub sprint: bool,
    /// The keys currently held, in the order `Key::index` returns.
    held: [bool; KEY_COUNT],
}

/// The sprint binding's double-tap window.
///
/// The state the source keeps on the player for its sprint rule
/// (`EntityPlayerSP.java:111`'s `sprintToggleTimer`, plus the previous tick's
/// input bits its rule reads at `:782-785`); [`SprintTap::update`] is one
/// tick of that rule.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SprintTap {
    /// Ticks left in the window (`sprintToggleTimer`).
    timer: i32,
    /// The previous tick's sneak, the source's `flag1`.
    prev_sneak: bool,
    /// Whether the previous tick reached forward, the source's `flag2`.
    prev_forward: bool,
}

impl Key {
    /// The slot this key's held state occupies in [`Intent`].
    fn index(self) -> usize {
        match self {
            Key::W => 0,
            Key::A => 1,
            Key::S => 2,
            Key::D => 3,
            Key::Space => 4,
            Key::ShiftLeft => 5,
            Key::ControlLeft => 6,
            Key::T => 7,
            Key::Slash => 8,
            Key::Tab => 9,
            Key::Enter => 10,
            Key::Backspace => 11,
            Key::ArrowLeft => 12,
            Key::ArrowRight => 13,
            Key::ArrowUp => 14,
            Key::ArrowDown => 15,
            Key::E => 16,
            Key::Q => 17,
            Key::Digit1 => 18,
            Key::Digit2 => 19,
            Key::Digit3 => 20,
            Key::Digit4 => 21,
            Key::Digit5 => 22,
            Key::Digit6 => 23,
            Key::Digit7 => 24,
            Key::Digit8 => 25,
            Key::Digit9 => 26,
        }
    }

    /// The hotbar slot this key selects, if it is one of the digit keys.
    ///
    /// The digits 1–9 read hotbar indices 0–8 (`Minecraft.java`:2076-2090 sets
    /// `thePlayer.inventory.currentItem` with no screen guard); every other
    /// key selects nothing.
    pub fn hotbar_slot(self) -> Option<i16> {
        match self {
            Key::Digit1 => Some(0),
            Key::Digit2 => Some(1),
            Key::Digit3 => Some(2),
            Key::Digit4 => Some(3),
            Key::Digit5 => Some(4),
            Key::Digit6 => Some(5),
            Key::Digit7 => Some(6),
            Key::Digit8 => Some(7),
            Key::Digit9 => Some(8),
            _ => None,
        }
    }

    /// Whether the gameplay intent tracks this key.
    ///
    /// The movement update reads the movement slots only — the chat keys
    /// carry slots so the index space stays total but drive no movement — and
    /// the inventory keys are the same: E, Q and the digits route to the
    /// screens and the session's own sends, never to the movement intent, so
    /// the window's gameplay translation filters on this and their edges
    /// never travel as gameplay input.
    pub fn is_gameplay(self) -> bool {
        matches!(
            self,
            Key::W | Key::A | Key::S | Key::D | Key::Space | Key::ShiftLeft | Key::ControlLeft
        )
    }
}

/// One wheel event's held-slot step: the slot the selection moves to.
///
/// The source clamps the event's delta to its sign — one slot per wheel event
/// — and wraps the nine hotbar slots (`InventoryPlayer.changeCurrentItem`
/// :165-185). A zero delta steps nowhere. The caller sends the result as
/// [`InputEvent::HeldItemChange`]; the screen's own scroll, when one is open,
/// runs in addition (`Minecraft.java`:1879/:1892).
pub fn hotbar_step(current: i16, delta: f32) -> i16 {
    let step = if delta > 0.0 {
        1
    } else if delta < 0.0 {
        -1
    } else {
        0
    };
    (current + step).rem_euclid(9)
}

/// The drop key's send for one press outside screens: one item, or the whole
/// stack with Ctrl held (`Minecraft.java`:2105-2111 over
/// `EntityPlayerSP.dropOneItem`:279-284).
pub fn drop_item(ctrl_held: bool) -> InputEvent {
    InputEvent::DropItem { whole: ctrl_held }
}

impl Intent {
    /// The neutral intent: nothing held, nothing moving.
    pub fn neutral() -> Intent {
        Intent {
            forward: 0.0,
            strafe: 0.0,
            jump: false,
            sneak: false,
            sprint: false,
            held: [false; KEY_COUNT],
        }
    }

    /// Applies one key edge, re-deriving the intent from the held keys.
    pub fn apply_key(&mut self, key: Key, pressed: bool) {
        self.held[key.index()] = pressed;
        self.update();
    }

    /// Releases every held key.
    ///
    /// A window that lost focus holds nothing; the intent returns to neutral.
    pub fn release_all(&mut self) {
        self.held = [false; KEY_COUNT];
        self.update();
    }

    /// Re-derives the movement input from the held keys.
    ///
    /// The rules are `MovementInputFromOptions.updatePlayerMoveState`'s own
    /// (`MovementInputFromOptions.java:14-46`): W and S add ±1 to forward, A
    /// and D add ±1 to strafe — both held cancel — Space is jump, ShiftLeft is
    /// sneak, and while sneak is held both axes are scaled by 0.3.
    fn update(&mut self) {
        let held = |key: Key| self.held[key.index()];
        let mut forward = 0.0;
        let mut strafe = 0.0;
        if held(Key::W) {
            forward += 1.0;
        }
        if held(Key::S) {
            forward -= 1.0;
        }
        if held(Key::A) {
            strafe += 1.0;
        }
        if held(Key::D) {
            strafe -= 1.0;
        }
        self.jump = held(Key::Space);
        self.sneak = held(Key::ShiftLeft);
        self.sprint = held(Key::ControlLeft);
        if self.sneak {
            forward *= SNEAK_SCALE;
            strafe *= SNEAK_SCALE;
        }
        self.forward = forward;
        self.strafe = strafe;
    }
}

impl SprintTap {
    /// Advances one tick of the source's sprint rule.
    ///
    /// `sprinting` is the state the previous tick left; the return value is
    /// the state this tick leaves. The rule is `EntityPlayerSP.onLivingUpdate`'s
    /// own (`EntityPlayerSP.java`:801-821); `collided_horizontally` is the
    /// previous step's own flag (`Entity.java`:818), read on the tick after
    /// the move that set it. The food gate `flag3` (`EntityPlayerSP.java`:799)
    /// and the item use and blindness gates stay unwired and recorded: the
    /// update is not handed the player's food or flight state, and no
    /// held-item or potion state exists yet.
    pub fn update(
        &mut self,
        input: &Intent,
        sprinting: bool,
        on_ground: bool,
        collided_horizontally: bool,
    ) -> bool {
        // The source reads the previous tick's input bits before this tick's
        // input updates (`:782-785`), and runs the window down first (`:727-729`).
        let prev_sneak = self.prev_sneak;
        let prev_forward = self.prev_forward;
        self.prev_sneak = input.sneak;
        self.prev_forward = input.forward >= SPRINT_FORWARD;
        if self.timer > 0 {
            self.timer -= 1;
        }

        let forward_reaches = input.forward >= SPRINT_FORWARD;
        let mut sprinting = sprinting;
        // The double-tap arm: a fresh tick that reaches forward opens the
        // window (`:801-806`); a tick that finds it open sprints (`:807-810`),
        // as does the sprint key (`:813-816`).
        if on_ground && !prev_sneak && !prev_forward && forward_reaches && !sprinting {
            if self.timer <= 0 && !input.sprint {
                self.timer = SPRINT_WINDOW_TICKS;
            } else {
                sprinting = true;
            }
        }
        if !sprinting && forward_reaches && input.sprint {
            sprinting = true;
        }
        // The release (`EntityPlayerSP.java`:818-821): the scaled input below
        // the threshold, a horizontal collision on the previous move, or the
        // food gate drops sprint — the last term is the one left unwired, as
        // the doc records.
        if sprinting && (!forward_reaches || collided_horizontally) {
            sprinting = false;
        }
        sprinting
    }
}

/// The forward input a sprint needs: `float f = 0.8F`
/// (`EntityPlayerSP.java:784`).
const SPRINT_FORWARD: f32 = 0.8;

/// The double-tap window, in ticks: `this.sprintToggleTimer = 7`
/// (`EntityPlayerSP.java:805`).
const SPRINT_WINDOW_TICKS: i32 = 7;

/// The look rotation one mouse delta produces, in degrees: `(d_yaw, d_pitch)`.
///
/// The chain is `EntityRenderer.updateMouse`'s own (`:1097-1102`):
/// `f = sensitivity × 0.6 + 0.2`, `f1 = f³ × 8`, the deltas scaled by `f1`;
/// then `Entity.setAngles` applies its ×0.15 factor, adding the yaw and
/// subtracting the pitch (`Entity.java:389-398`). At the fixed 0.5 default
/// that is 0.15° per pixel per axis.
///
/// `dx` is rightward and `dy` downward, the window's screen convention; the
/// source's device delta grows upward, so its subtracted pitch term is this
/// convention's added one and a downward delta yields a positive pitch
/// change — positive pitch looks down (`Entity.getVectorForRotation`,
/// `Entity.java:1476-1482`).
pub fn look_delta(dx: f64, dy: f64, sensitivity: f32) -> (f32, f32) {
    let f = sensitivity * 0.6 + 0.2;
    let f1 = f * f * f * 8.0;
    let scale = f1 * 0.15;
    (dx as f32 * scale, dy as f32 * scale)
}

#[cfg(test)]
mod tests {
    //! The binding table, the double-tap sprint rule and the look mapping.

    use super::{InputEvent, Intent, Key, SprintTap, drop_item, hotbar_step, look_delta};

    /// The intent with `key` alone held.
    fn press(key: Key) -> Intent {
        let mut intent = Intent::neutral();
        intent.apply_key(key, true);
        intent
    }

    /// One tick of the sprint rule on the ground: the more precise tests
    /// below pass that state explicitly.
    fn tick(tap: &mut SprintTap, intent: &Intent, sprinting: bool) -> bool {
        tap.update(intent, sprinting, true, false)
    }

    /// Asserts two look values agree to the f32 arithmetic's own rounding.
    fn near(value: f32, expected: f32) {
        assert!(
            (value - expected).abs() < 1e-6,
            "{value} is not within 1e-6 of {expected}"
        );
    }

    #[test]
    fn the_default_bindings_are_the_sources_own() {
        // `GameSettings.java:127-133` and
        // `MovementInputFromOptions.updatePlayerMoveState` (`:19-40`): W and
        // S drive forward, A and D drive strafe, Space jumps, ShiftLeft
        // sneaks, ControlLeft is the sprint key.
        assert_eq!(press(Key::W).forward, 1.0);
        assert_eq!(press(Key::S).forward, -1.0);
        assert_eq!(press(Key::A).strafe, 1.0);
        assert_eq!(press(Key::D).strafe, -1.0);
        assert!(press(Key::Space).jump);
        assert!(press(Key::ShiftLeft).sneak);
        assert!(press(Key::ControlLeft).sprint);
        // Each key drives its own field and nothing else.
        let forward = press(Key::W);
        assert_eq!(
            (forward.strafe, forward.jump, forward.sneak, forward.sprint),
            (0.0, false, false, false)
        );
        let sneak = press(Key::ShiftLeft);
        assert_eq!((sneak.forward, sneak.strafe, sneak.jump), (0.0, 0.0, false));
    }

    #[test]
    fn opposed_keys_cancel_and_a_release_restores_the_input() {
        // The source sums the two directions (`++moveForward` / `--moveForward`).
        let mut intent = Intent::neutral();
        intent.apply_key(Key::W, true);
        intent.apply_key(Key::S, true);
        assert_eq!(intent.forward, 0.0, "W and S held together cancel out");
        intent.apply_key(Key::S, false);
        assert_eq!(intent.forward, 1.0, "the release leaves W's own input");
    }

    #[test]
    fn sneak_scales_the_movement_input_by_the_sources_own_factor() {
        // `MovementInputFromOptions.java:42-46`: while sneak is held both
        // axes are multiplied by 0.3, which is what makes sneak release
        // sprint below (`EntityPlayerSP.java:818`).
        let mut intent = Intent::neutral();
        intent.apply_key(Key::W, true);
        intent.apply_key(Key::A, true);
        intent.apply_key(Key::ShiftLeft, true);
        assert!(intent.sneak);
        assert_eq!(intent.forward, 0.3);
        assert_eq!(intent.strafe, 0.3);
        intent.apply_key(Key::ShiftLeft, false);
        assert_eq!((intent.forward, intent.strafe), (1.0, 1.0));
    }

    #[test]
    fn a_second_forward_press_inside_the_window_arms_sprint() {
        // The first qualifying tick only opens the window
        // (`EntityPlayerSP.java:801-806`); a tick inside it that reaches
        // forward sprints (`:807-810`). The window's open state needs the W
        // release: the source reads the previous tick's forward bit (`:782-785`).
        let mut tap = SprintTap::default();
        let mut intent = Intent::neutral();
        intent.apply_key(Key::W, true);
        let mut sprinting = tick(&mut tap, &intent, false);
        assert!(!sprinting, "the first press only opens the window");
        intent.apply_key(Key::W, false);
        sprinting = tick(&mut tap, &intent, sprinting);
        assert!(!sprinting);
        intent.apply_key(Key::W, true);
        sprinting = tick(&mut tap, &intent, sprinting);
        assert!(sprinting, "the second press inside the window arms sprint");
    }

    #[test]
    fn a_press_after_the_window_re_arms_instead_of_sprinting() {
        // The window runs down once per tick (`EntityPlayerSP.java:727-729`):
        // after seven ticks it is expired, and a tick that reaches forward
        // opens a new window rather than sprinting (`:803-806`).
        let mut tap = SprintTap::default();
        let mut intent = Intent::neutral();
        intent.apply_key(Key::W, true);
        tick(&mut tap, &intent, false);
        intent.apply_key(Key::W, false);
        for _ in 0..7 {
            tick(&mut tap, &intent, false);
        }
        intent.apply_key(Key::W, true);
        let mut sprinting = tick(&mut tap, &intent, false);
        assert!(!sprinting, "the expired window re-arms, it does not sprint");
        // The fresh window's next qualifying tick does sprint.
        intent.apply_key(Key::W, false);
        tick(&mut tap, &intent, false);
        intent.apply_key(Key::W, true);
        sprinting = tick(&mut tap, &intent, sprinting);
        assert!(sprinting);
    }

    #[test]
    fn the_sprint_window_is_exactly_seven_ticks_wide() {
        // The border of the source's window (`sprintToggleTimer = 7`,
        // `EntityPlayerSP.java:805`; read at `:803`). The timer runs down
        // once per tick at the top of the rule (`:727-729`), so a press that
        // lands five intervening ticks after the arming press — six ticks on —
        // still finds the window open and sprints, while one intervening tick
        // later — seven ticks on — finds it expired and re-arms instead. The
        // two sides fix the width at seven.
        let sprint_after_intervening_ticks = |intervening: usize| -> bool {
            let mut tap = SprintTap::default();
            let mut intent = Intent::neutral();
            intent.apply_key(Key::W, true);
            tick(&mut tap, &intent, false); // the arming press opens the window
            intent.apply_key(Key::W, false);
            for _ in 0..intervening {
                tick(&mut tap, &intent, false);
            }
            intent.apply_key(Key::W, true);
            tick(&mut tap, &intent, false)
        };
        assert!(
            sprint_after_intervening_ticks(5),
            "a press six ticks on from the arming press is inside the window"
        );
        assert!(
            !sprint_after_intervening_ticks(6),
            "a press seven ticks on finds the window expired and re-arms"
        );
    }

    #[test]
    fn the_sprint_key_sprints_while_forward_is_held() {
        // `... && keyBindSprint.isKeyDown()) { this.setSprinting(true); }`
        // (`EntityPlayerSP.java:813-816`) needs no double tap.
        let mut tap = SprintTap::default();
        let mut intent = Intent::neutral();
        intent.apply_key(Key::W, true);
        intent.apply_key(Key::ControlLeft, true);
        assert!(
            tick(&mut tap, &intent, false),
            "the sprint key sprints on its first tick"
        );
    }

    #[test]
    fn a_sneak_that_drops_the_forward_input_releases_sprint() {
        // Sneak scales forward to 0.3 (`MovementInputFromOptions.java:42-46`),
        // below the 0.8 the release rule reads (`EntityPlayerSP.java:818`).
        let mut tap = SprintTap::default();
        let mut intent = Intent::neutral();
        intent.apply_key(Key::W, true);
        let mut sprinting = tick(&mut tap, &intent, false);
        intent.apply_key(Key::W, false);
        sprinting = tick(&mut tap, &intent, sprinting);
        intent.apply_key(Key::W, true);
        sprinting = tick(&mut tap, &intent, sprinting);
        assert!(sprinting);
        intent.apply_key(Key::ShiftLeft, true);
        assert!(
            !tick(&mut tap, &intent, sprinting),
            "sneak drops the forward input and releases sprint"
        );
    }

    #[test]
    fn the_release_carries_the_collision_clause() {
        // `EntityPlayerSP.java`:818-821: the release is
        // `isSprinting() && (moveForward < f || isCollidedHorizontally || !flag3)`.
        // The collision term is live — the flag is the previous move's own
        // (`Entity.java`:818), read on the tick after the move that set it.
        // The `!flag3` row stays unwired: food and `allow_flying` exist on the
        // player, but this update is not handed them, so the term is assumed
        // true and recorded rather than read.
        let sprinting_after = |forward: f32, collided: bool| -> bool {
            let mut tap = SprintTap::default();
            let mut intent = Intent::neutral();
            intent.forward = forward;
            tap.update(&intent, true, true, collided)
        };
        assert!(sprinting_after(1.0, false), "a clear run holds the sprint");
        assert!(!sprinting_after(0.3, false), "a short forward releases it");
        assert!(
            !sprinting_after(1.0, true),
            "a collision on the previous move releases it"
        );
        assert!(!sprinting_after(0.3, true), "either disjunct releases it");
    }

    #[test]
    fn the_sprint_threshold_is_the_sources_eight_tenths() {
        // The forward input a sprint needs is `float f = 0.8F`
        // (`EntityPlayerSP.java:784`, compared at `:785` and `:801`). The
        // bindings can hold full forward (1.0), its sneak-scaled 0.3, or
        // nothing — no value near the threshold — so it is fixed against
        // synthetic inputs the window cannot produce: 0.85 and 0.81 clear it,
        // 0.79 and 0.75 fall short.
        let sprinting_with = |forward: f32| -> bool {
            let mut tap = SprintTap::default();
            let mut intent = Intent::neutral();
            intent.forward = forward;
            intent.sprint = true;
            tick(&mut tap, &intent, false)
        };
        assert!(
            sprinting_with(0.85),
            "0.85 clears the eight-tenths threshold"
        );
        assert!(
            !sprinting_with(0.75),
            "0.75 falls short of the eight-tenths threshold"
        );
        assert!(sprinting_with(0.81), "0.81 clears it");
        assert!(!sprinting_with(0.79), "0.79 falls short of it");
    }

    #[test]
    fn a_focus_loss_releases_every_key_and_the_intent_returns_to_neutral() {
        let mut intent = Intent::neutral();
        for key in [Key::W, Key::A, Key::Space, Key::ShiftLeft, Key::ControlLeft] {
            intent.apply_key(key, true);
        }
        assert_ne!(intent, Intent::neutral(), "keys are held");
        intent.release_all();
        assert_eq!(intent, Intent::neutral(), "the intent is neutral again");
        // Every key is released, not only the movement ones.
        intent.apply_key(Key::W, true);
        assert_eq!(
            (intent.forward, intent.jump, intent.sneak, intent.sprint),
            (1.0, false, false, false)
        );
    }

    #[test]
    fn the_look_mapping_matches_the_source_literals() {
        // `EntityRenderer.java:1097-1102`: f = sensitivity × 0.6 + 0.2,
        // f1 = f³ × 8, the deltas scaled by f1; `Entity.setAngles`
        // (`Entity.java:389-398`): applied ×0.15. One pixel per axis at the
        // three pinned sensitivities.
        near(look_delta(1.0, 0.0, 0.0).0, 0.0096);
        near(look_delta(1.0, 0.0, 0.5).0, 0.15);
        near(look_delta(1.0, 0.0, 1.0).0, 0.6144);
        near(look_delta(0.0, 1.0, 0.0).1, 0.0096);
        near(look_delta(0.0, 1.0, 0.5).1, 0.15);
        near(look_delta(0.0, 1.0, 1.0).1, 0.6144);
        // Ten pixels right at the fixed 0.5 default: 1.5° of yaw, no pitch.
        near(look_delta(10.0, 0.0, 0.5).0, 1.5);
        assert_eq!(look_delta(10.0, 0.0, 0.5).1, 0.0);
    }

    #[test]
    fn moving_right_turns_right_and_moving_down_looks_down() {
        // The sign chain: `MouseHelper.mouseXYChange` reads the device deltas
        // (`MouseHelper.java:33-37`), whose y grows upward in the window's own
        // coordinate system; `EntityRenderer.updateMouse` (`:1097-1102`)
        // scales them; `Entity.setAngles` adds the yaw and subtracts the pitch
        // (`Entity.java:393-395`). This crate's deltas carry the window's
        // screen convention (y downward), so the source's subtracted pitch
        // term is this convention's added one — and positive pitch looks down
        // (`Entity.getVectorForRotation`, `Entity.java:1476-1482`). A rising
        // yaw turns right: yaw 0 faces south (+z) and 90 west (−x)
        // (`Entity.moveFlying`, `:1240-1243`).
        let right = look_delta(10.0, 0.0, 0.5);
        assert!(right.0 > 0.0, "a rightward delta raises yaw: {right:?}");
        assert_eq!(right.1, 0.0);
        let down = look_delta(0.0, 10.0, 0.5);
        assert!(down.1 > 0.0, "a downward delta looks down: {down:?}");
        assert_eq!(down.0, 0.0);
        let left = look_delta(-10.0, 0.0, 0.5);
        assert!(left.0 < 0.0, "a leftward delta turns left: {left:?}");
        let up = look_delta(0.0, -10.0, 0.5);
        assert!(up.1 < 0.0, "an upward delta looks up: {up:?}");
    }

    #[test]
    fn the_chat_keys_hold_slots_but_leave_the_intent_neutral() {
        // The keys the chat rides carry held slots so the index space stays
        // total (the enum orders the intent's `held` array); they drive no
        // movement, so any of them alone leaves the intent neutral.
        for key in [
            Key::T,
            Key::Slash,
            Key::Tab,
            Key::Enter,
            Key::Backspace,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
        ] {
            let mut intent = Intent::neutral();
            intent.apply_key(key, true);
            let motion = (
                intent.forward,
                intent.strafe,
                intent.jump,
                intent.sneak,
                intent.sprint,
            );
            assert_eq!(
                motion,
                (0.0, 0.0, false, false, false),
                "{key:?} drives no movement"
            );
            intent.apply_key(key, false);
            assert_eq!(
                (
                    intent.forward,
                    intent.strafe,
                    intent.jump,
                    intent.sneak,
                    intent.sprint
                ),
                (0.0, 0.0, false, false, false)
            );
        }
        // The gameplay set is exactly the movement keys and the sprint key.
        for key in [
            Key::W,
            Key::A,
            Key::S,
            Key::D,
            Key::Space,
            Key::ShiftLeft,
            Key::ControlLeft,
        ] {
            assert!(key.is_gameplay(), "{key:?} is gameplay input");
        }
        for key in [
            Key::T,
            Key::Slash,
            Key::Tab,
            Key::Enter,
            Key::Backspace,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
        ] {
            assert!(!key.is_gameplay(), "{key:?} is not gameplay input");
        }
    }

    #[test]
    fn the_inventory_keys_hold_slots_but_drive_no_intent() {
        // E, Q and the digits route to the screens and the session's own
        // sends (`keyBindInventory` :134, `keyBindDrop` :136,
        // `keyBindsHotbar` :151); like the chat keys they carry held slots so
        // the index space stays total but drive no movement.
        for key in [
            Key::E,
            Key::Q,
            Key::Digit1,
            Key::Digit2,
            Key::Digit3,
            Key::Digit4,
            Key::Digit5,
            Key::Digit6,
            Key::Digit7,
            Key::Digit8,
            Key::Digit9,
        ] {
            assert!(!key.is_gameplay(), "{key:?} is not gameplay input");
            let mut intent = Intent::neutral();
            intent.apply_key(key, true);
            let motion = (
                intent.forward,
                intent.strafe,
                intent.jump,
                intent.sneak,
                intent.sprint,
            );
            assert_eq!(
                motion,
                (0.0, 0.0, false, false, false),
                "{key:?} drives no movement"
            );
        }
    }

    #[test]
    fn the_key_index_space_is_total_over_all_twenty_seven_keys() {
        // The count is the variant count: every key holds a distinct slot
        // under it, so no two keys share held state.
        let keys = [
            Key::W,
            Key::A,
            Key::S,
            Key::D,
            Key::Space,
            Key::ShiftLeft,
            Key::ControlLeft,
            Key::T,
            Key::Slash,
            Key::Tab,
            Key::Enter,
            Key::Backspace,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::E,
            Key::Q,
            Key::Digit1,
            Key::Digit2,
            Key::Digit3,
            Key::Digit4,
            Key::Digit5,
            Key::Digit6,
            Key::Digit7,
            Key::Digit8,
            Key::Digit9,
        ];
        assert_eq!(keys.len(), 27, "every variant is listed");
        let mut seen = [false; 27];
        for key in keys {
            // Each key's slot is its own: pressing one and releasing another
            // leaves the first held.
            let mut intent = Intent::neutral();
            intent.apply_key(key, true);
            assert_ne!(intent, Intent::neutral(), "{key:?} holds its slot");
            seen[key.index()] = true;
        }
        assert!(seen.iter().all(|slot| *slot), "no two keys share a slot");
    }

    #[test]
    fn the_digits_map_to_the_hotbar_slots_and_nothing_else_does() {
        // `Minecraft.java`:2076-2090 reads `keyBindsHotbar[l]` as slot `l`:
        // digit 1 is slot 0 through digit 9 at slot 8.
        let digits = [
            Key::Digit1,
            Key::Digit2,
            Key::Digit3,
            Key::Digit4,
            Key::Digit5,
            Key::Digit6,
            Key::Digit7,
            Key::Digit8,
            Key::Digit9,
        ];
        for (slot, key) in digits.iter().enumerate() {
            assert_eq!(key.hotbar_slot(), Some(slot as i16), "{key:?} selects");
        }
        for key in [
            Key::W,
            Key::E,
            Key::Q,
            Key::T,
            Key::Space,
            Key::Enter,
            Key::ArrowUp,
        ] {
            assert_eq!(key.hotbar_slot(), None, "{key:?} selects nothing");
        }
    }

    #[test]
    fn the_wheel_steps_one_slot_per_event_and_wraps_the_hotbar() {
        // `InventoryPlayer.changeCurrentItem`:165-185: the delta clamps to
        // its sign — a five-notch event still steps one slot — and the nine
        // slots wrap.
        assert_eq!(hotbar_step(3, 1.0), 4);
        assert_eq!(hotbar_step(3, -1.0), 2);
        assert_eq!(
            hotbar_step(3, 5.0),
            4,
            "a five-notch event still steps one slot"
        );
        assert_eq!(hotbar_step(3, -5.0), 2);
        assert_eq!(hotbar_step(8, 1.0), 0, "the top wraps to the bottom");
        assert_eq!(hotbar_step(0, -1.0), 8, "the bottom wraps to the top");
        assert_eq!(hotbar_step(4, 0.0), 4, "a zero delta steps nowhere");
    }

    #[test]
    fn the_drop_key_sends_one_item_or_the_whole_stack() {
        // `Minecraft.java`:2105-2111 over `EntityPlayerSP.dropOneItem`
        // :279-284: plain Q drops one item, Ctrl+Q the whole stack.
        assert_eq!(drop_item(false), InputEvent::DropItem { whole: false });
        assert_eq!(drop_item(true), InputEvent::DropItem { whole: true });
    }
}
