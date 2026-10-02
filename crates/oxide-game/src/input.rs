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

/// One physical key the client binds.
///
/// The source binds many more; these are the movement keys and the sprint key
/// M3's input surface names (`GameSettings.java:127-133`).
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

/// How many physical keys the intent tracks.
const KEY_COUNT: usize = 7;

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
        }
    }
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
    /// own (`:801-820`); the food gate `flag3`, the item use and blindness
    /// gates and the horizontal-collision release all need player state this
    /// client does not carry yet, so they are left out and recorded here.
    pub fn update(&mut self, input: &Intent, sprinting: bool, on_ground: bool) -> bool {
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
        // The release: the scaled input below the threshold drops sprint
        // (`:818-820`), which is how sneak releases it.
        if sprinting && !forward_reaches {
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

    use super::{Intent, Key, SprintTap, look_delta};

    /// The intent with `key` alone held.
    fn press(key: Key) -> Intent {
        let mut intent = Intent::neutral();
        intent.apply_key(key, true);
        intent
    }

    /// One tick of the sprint rule on the ground: the more precise tests
    /// below pass that state explicitly.
    fn tick(tap: &mut SprintTap, intent: &Intent, sprinting: bool) -> bool {
        tap.update(intent, sprinting, true)
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
}
