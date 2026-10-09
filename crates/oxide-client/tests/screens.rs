//! Task 16's routing and mapping suite: the screen state machine and the
//! container base's click derivation.
//!
//! The routing half pins the open/close transitions — a server close while
//! its screen is open clears it, every Escape sends one `CloseWindow` with
//! the screen's own window id (window 0 included), an unknown kind opens the
//! generic frame, and the hover highlight reads the draw loop's last match.
//! The mapping half is the click derivation truth table transcribed from the
//! source's handlers (`GuiContainer.mouseClicked`:359-460,
//! `mouseClickMove`'s add at :466-510, `mouseReleased`:515-652,
//! `keyTyped`'s slot keys at :692-731): every branch produces its pinned
//! `(slot, button, mode)`.
//!
//! The layouts below are the suite's own local tables — the per-kind tables
//! land in Tasks 18-20, and the base consumes whatever table it is given
//! (`ContainerLayout.slots` is the seam).

use oxide_client::screens::{
    Screens,
    container::{
        CHEST_SHIFT_STEP, ClickButton, ContainerLayout, ContainerScreen, HOTBAR_TOP, MAIN_TOP,
        SLOT_LEFT, SLOT_STEP, ScreenKey, TitleKind, chest_row_shift, hovered_last, player_section,
        point_in_slot, slot_at_first,
    },
};
use oxide_game::container::{
    BaseStackCaps, CLICK_MODE_CREATIVE_PICK, CLICK_MODE_DRAG, CLICK_MODE_DROP, CLICK_MODE_GATHER,
    CLICK_MODE_PICKUP, CLICK_MODE_QUICK_MOVE, CLICK_MODE_SWAP, drag_button,
};
use oxide_game::input::InputEvent;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::WindowKind;

static SUITE_SLOTS: &[oxide_client::screens::container::SlotPos] = &[
    oxide_client::screens::container::SlotPos {
        index: 0,
        x: 8,
        y: 18,
    },
    oxide_client::screens::container::SlotPos {
        index: 1,
        x: 26,
        y: 18,
    },
    oxide_client::screens::container::SlotPos {
        index: 2,
        x: 8,
        y: 36,
    },
    oxide_client::screens::container::SlotPos {
        index: 3,
        x: 26,
        y: 36,
    },
];
static SUITE_LAYOUT: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "suite/panel",
    slots: SUITE_SLOTS,
    title: TitleKind::Generic,
};

fn stack(id: i16, count: u8) -> MetadataItem {
    MetadataItem {
        id,
        count,
        damage: 0,
        nbt: None,
    }
}

fn screen_with_cursor(cursor: Option<MetadataItem>) -> ContainerScreen {
    let mut screen =
        ContainerScreen::new(7, WindowKind::Chest, String::from("Chest"), &SUITE_LAYOUT);
    screen.apply_snapshot(
        vec![
            Some(stack(1, 4)),
            Some(stack(2, 1)),
            None,
            Some(stack(3, 64)),
        ],
        cursor,
    );
    // Slot 0's cell is (8..24, 18..34); its centre is (16, 26).
    screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
    screen
}

fn clicks(events: Vec<InputEvent>) -> Vec<(i16, i8, i8)> {
    events
        .into_iter()
        .map(|event| match event {
            InputEvent::ClickWindow {
                window_id,
                slot,
                button,
                mode,
            } => {
                assert_eq!(window_id, 7, "every click names the screen's window");
                (slot, button, mode)
            }
            other => panic!("the derivation sends clicks only, got {other:?}"),
        })
        .collect()
}

// The routing: opens land on the container path with the window's own id.

#[test]
fn a_window_open_opens_the_container_screen() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"));
    assert!(screens.is_open(), "the window opens a screen");
    assert_eq!(screens.current_window_id(), Some(7));
}

// Escape on a chest sends its own window id and clears the screen.

#[test]
fn escape_on_a_chest_sends_its_own_close() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"));
    let mut cursor = Some(stack(1, 3));
    let event = screens.escape(&mut cursor);
    assert_eq!(event, Some(InputEvent::CloseWindow { window_id: 7 }));
    assert_eq!(cursor, None, "the close drops the carried stack");
    assert!(!screens.is_open(), "the screen is gone");
}

// Escape on the inventory sends window 0: every close sends C0D.

#[test]
fn escape_on_the_inventory_sends_window_zero() {
    let mut screens = Screens::default();
    screens.open_inventory();
    let mut cursor = Some(stack(1, 3));
    let event = screens.escape(&mut cursor);
    assert_eq!(event, Some(InputEvent::CloseWindow { window_id: 0 }));
    assert_eq!(cursor, None, "window 0's close drops the cursor too");
    assert!(!screens.is_open());
}

// A server close while its screen is open clears it with no send.

#[test]
fn a_server_close_clears_its_open_screen() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"));
    assert!(screens.on_server_close(7), "the matching close clears");
    assert!(!screens.is_open());
}

// A server close for another window leaves the screen standing.

#[test]
fn a_server_close_for_another_window_keeps_the_screen() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"));
    assert!(!screens.on_server_close(9), "nothing held window 9");
    assert!(screens.is_open(), "the screen stands");
}

// An unknown kind opens the generic frame, recorded.

#[test]
fn an_unknown_kind_opens_the_generic_frame() {
    let mut screens = Screens::default();
    screens.on_window_opened(4, WindowKind::Unknown, String::from("???"));
    assert!(screens.is_open(), "even an unknown window opens");
    let generic = match screens.current() {
        Some(oxide_client::screens::ScreenState::Container(screen)) => screen.generic_frame(),
        other => panic!("an unknown kind opens a container screen, got {other:?}"),
    };
    assert!(generic, "the unknown kind draws the generic frame");
}

// A close with no screen open sends nothing.

#[test]
fn closing_with_no_screen_sends_nothing() {
    let mut screens = Screens::default();
    assert_eq!(screens.escape(&mut None), None);
}

// The input ownership: the inventory and creative screens take user input.

#[test]
fn only_the_inventory_and_creative_screens_take_user_input() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"));
    assert!(!screens.allow_user_input(), "containers inherit false");
    screens.open_inventory();
    assert!(screens.allow_user_input(), "the inventory sets true");
    screens.open_creative();
    assert!(screens.allow_user_input(), "creative sets true");
    screens.open_sign(1, 2, 3);
    assert!(
        !screens.allow_user_input(),
        "the sign editor leaves it false"
    );
}

// The chat over the inventory stashes it, and closing the chat lands in the
// game: the chat carries no parent screen.

#[test]
fn the_chat_over_the_inventory_returns_to_the_game() {
    let mut screens = Screens::default();
    screens.open_inventory();
    screens.cover_with_chat();
    assert!(!screens.is_open(), "the chat replaces the screen");
    screens.uncover_from_chat();
    assert!(!screens.is_open(), "closing the chat lands in the game");
}

// The hit test reads the ±1-padded 18×18 region, not the bare 16×16.

#[test]
fn the_hit_test_pads_the_cell_by_one() {
    // Slot 0 spans panel (8..24, 18..34); the pad reaches one pixel past it.
    assert!(point_in_slot(8, 18, 7, 17), "one pixel up-left still hits");
    assert!(
        point_in_slot(8, 18, 24, 34),
        "one pixel down-right still hits"
    );
    assert!(!point_in_slot(8, 18, 6, 17), "two pixels out misses");
    assert!(!point_in_slot(8, 18, 25, 34), "two pixels past misses");
}

// The click reads the first match in slot order; the highlight reads the
// last: an overlapping pair discriminates them.

#[test]
fn the_click_reads_the_first_match_and_the_highlight_the_last() {
    use oxide_client::screens::container::SlotPos;
    static OVERLAP: &[SlotPos] = &[
        SlotPos {
            index: 5,
            x: 8,
            y: 18,
        },
        SlotPos {
            index: 9,
            x: 8,
            y: 18,
        },
    ];
    // (10, 20) sits inside both cells.
    assert_eq!(slot_at_first(OVERLAP, 10.0, 20.0), Some(5));
    assert_eq!(hovered_last(OVERLAP, 10.0, 20.0), Some(9));
}

// The player section: the standard 27 + 9 block at the derived positions.

#[test]
fn the_player_section_lays_out_the_standard_block() {
    let slots = player_section(9);
    assert_eq!(slots.len(), 36, "27 main slots plus the 9 hotbar");
    assert_eq!(
        (slots[0].index, slots[0].x, slots[0].y),
        (9, SLOT_LEFT, MAIN_TOP)
    );
    assert_eq!(slots[1].x, SLOT_LEFT + SLOT_STEP);
    assert_eq!(
        slots[9].y,
        MAIN_TOP + SLOT_STEP,
        "the second row sits 18 lower"
    );
    assert_eq!(
        slots[18].y,
        MAIN_TOP + 2 * SLOT_STEP,
        "the third row sits 36 lower"
    );
    let hotbar = &slots[27..];
    assert_eq!(
        (hotbar[0].index, hotbar[0].x, hotbar[0].y),
        (36, SLOT_LEFT, HOTBAR_TOP)
    );
    assert_eq!(hotbar[8].x, SLOT_LEFT + 8 * SLOT_STEP);
}

// The chest's rows-dependent shift: (rows − 4) × 18.

#[test]
fn the_chest_shift_moves_with_the_row_count() {
    assert_eq!(chest_row_shift(3), -18);
    assert_eq!(chest_row_shift(6), 36);
    assert_eq!(CHEST_SHIFT_STEP, 18);
}

// The mapping: a left press with an empty cursor picks up.

#[test]
fn a_left_press_with_an_empty_cursor_picks_up() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.press(ClickButton::Left, false, 1_000));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_PICKUP)]);
}

// A right press with an empty cursor places with button 1.

#[test]
fn a_right_press_with_an_empty_cursor_places() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.press(ClickButton::Right, false, 1_000));
    assert_eq!(got, vec![(0, 1, CLICK_MODE_PICKUP)]);
}

// Shift held turns the press into a quick-move.

#[test]
fn a_shift_press_quick_moves() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.press(ClickButton::Left, true, 1_000));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_QUICK_MOVE)]);
}

// The pick binding with an empty cursor sends mode 3 with the middle code.

#[test]
fn the_pick_binding_press_picks_with_mode_three() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.press(ClickButton::Pick, false, 1_000));
    assert_eq!(got, vec![(0, 2, CLICK_MODE_CREATIVE_PICK)]);
}

// Outside the panel the press throws with mode 4 and slot −999.

#[test]
fn a_press_outside_throws_with_mode_four() {
    let mut screen = screen_with_cursor(None);
    screen.mouse_moved(-10.0, -10.0, &BaseStackCaps);
    let got = clicks(screen.press(ClickButton::Left, false, 1_000));
    assert_eq!(got, vec![(-999, 0, CLICK_MODE_DROP)]);
}

// Shift held outside still throws: the shift arm needs a real slot.

#[test]
fn a_shift_press_outside_still_throws() {
    let mut screen = screen_with_cursor(None);
    screen.mouse_moved(-10.0, -10.0, &BaseStackCaps);
    let got = clicks(screen.press(ClickButton::Left, true, 1_000));
    assert_eq!(got, vec![(-999, 0, CLICK_MODE_DROP)]);
}

// Inside the panel but on no slot nothing sends.

#[test]
fn a_press_on_no_slot_sends_nothing() {
    let mut screen = screen_with_cursor(None);
    // (60, 60) sits inside the 176×166 panel but on no suite slot.
    screen.mouse_moved(60.0, 60.0, &BaseStackCaps);
    assert!(screen.press(ClickButton::Left, false, 1_000).is_empty());
}

// A press with a carried stack sends nothing: it arms the drag instead.

#[test]
fn a_press_with_a_carried_stack_arms_the_drag() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    assert!(screen.press(ClickButton::Left, false, 1_000).is_empty());
    assert!(screen.dragging(), "the drag is armed");
}

// The drag release sends the three phases together: 1 + n + 1.

#[test]
fn a_drag_release_sends_start_slots_and_end() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    // Slot 2's centre is (16, 44): empty, so it joins; back on slot 0's
    // centre (16, 26), which holds the same item, joins too. Slot 1 holds a
    // different item and can never join.
    screen.mouse_moved(16.0, 44.0, &BaseStackCaps);
    screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
    let got = clicks(screen.release(ClickButton::Left, false, 1_100));
    assert_eq!(
        got,
        vec![
            (-999, drag_button(0, 0) as i8, CLICK_MODE_DRAG),
            (2, drag_button(1, 0) as i8, CLICK_MODE_DRAG),
            (0, drag_button(1, 0) as i8, CLICK_MODE_DRAG),
            (-999, drag_button(2, 0) as i8, CLICK_MODE_DRAG),
        ]
    );
}

// A right-button drag packs limit 1 into every phase.

#[test]
fn a_right_drag_packs_limit_one() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Right, false, 1_000);
    screen.mouse_moved(16.0, 44.0, &BaseStackCaps);
    screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
    let got = clicks(screen.release(ClickButton::Right, false, 1_100));
    assert_eq!(
        got,
        vec![
            (-999, drag_button(0, 1) as i8, CLICK_MODE_DRAG),
            (2, drag_button(1, 1) as i8, CLICK_MODE_DRAG),
            (0, drag_button(1, 1) as i8, CLICK_MODE_DRAG),
            (-999, drag_button(2, 1) as i8, CLICK_MODE_DRAG),
        ]
    );
}

// Releasing another button cancels the drag with no send.

#[test]
fn releasing_another_button_cancels_the_drag() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    screen.mouse_moved(16.0, 44.0, &BaseStackCaps);
    assert!(screen.release(ClickButton::Right, false, 1_100).is_empty());
    assert!(!screen.dragging(), "the drag is cancelled");
}

// A press and release with no drag movement clicks on release.

#[test]
fn an_undragged_release_clicks_on_release() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    let got = clicks(screen.release(ClickButton::Left, false, 1_100));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_PICKUP)]);
}

// The double click gathers with mode 6 inside the 250 ms window.

#[test]
fn a_double_click_gathers() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    // The first click picks up (no drag movement, so release clicks).
    screen.press(ClickButton::Left, false, 1_000);
    screen.release(ClickButton::Left, false, 1_050);
    // The second press lands 100 ms later on the same slot.
    screen.apply_snapshot(
        vec![
            Some(stack(1, 4)),
            Some(stack(2, 1)),
            None,
            Some(stack(3, 64)),
        ],
        Some(stack(1, 60)),
    );
    screen.press(ClickButton::Left, false, 1_100);
    let got = clicks(screen.release(ClickButton::Left, false, 1_150));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_GATHER)]);
}

// Past the 250 ms window the second click is ordinary.

#[test]
fn a_slow_second_click_is_ordinary() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    screen.release(ClickButton::Left, false, 1_050);
    screen.apply_snapshot(
        vec![
            Some(stack(1, 4)),
            Some(stack(2, 1)),
            None,
            Some(stack(3, 64)),
        ],
        Some(stack(1, 60)),
    );
    screen.press(ClickButton::Left, false, 1_400);
    let got = clicks(screen.release(ClickButton::Left, false, 1_450));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_PICKUP)]);
}

// The number keys swap with mode 2 while the cursor is empty.

#[test]
fn a_number_key_swaps_while_the_cursor_is_empty() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.screen_key(ScreenKey::Number(3), false));
    assert_eq!(got, vec![(0, 3, CLICK_MODE_SWAP)]);
}

// The number keys fall through while carrying: the cursor-empty gate.

#[test]
fn a_number_key_with_a_carried_stack_sends_nothing() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    assert!(screen.screen_key(ScreenKey::Number(3), false).is_empty());
}

// The drop key drops one over a hovered stack, the whole stack with Ctrl.

#[test]
fn the_drop_key_drops_over_a_hovered_stack() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.screen_key(ScreenKey::Drop, false));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_DROP)]);
    let got = clicks(screen.screen_key(ScreenKey::Drop, true));
    assert_eq!(got, vec![(0, 1, CLICK_MODE_DROP)]);
}

// The drop key over an empty slot sends nothing.

#[test]
fn the_drop_key_over_an_empty_slot_sends_nothing() {
    let mut screen = screen_with_cursor(None);
    // Slot 2's centre is (16, 44); the slot holds nothing.
    screen.mouse_moved(16.0, 44.0, &BaseStackCaps);
    assert!(screen.screen_key(ScreenKey::Drop, false).is_empty());
}

// The pick key over a hovered stack sends mode 3 with button 0.

#[test]
fn the_pick_key_sends_mode_three() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.screen_key(ScreenKey::Pick, false));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_CREATIVE_PICK)]);
}

// The remnant previews the even split across the covered slots.

#[test]
fn the_drag_previews_the_even_split() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    // Slot 2 is empty and joins; slot 0 holds the same item and joins next.
    screen.mouse_moved(16.0, 44.0, &BaseStackCaps);
    screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
    assert_eq!(screen.drag_slots(), &[2, 0]);
    // 64 across two slots is 32 each; slot 0 already holds 4 of item 1, so
    // it takes 32 + 4 = 36 and slot 2 takes 32: the remnant is 64 − 64.
    assert_eq!(screen.remnant_count(), Some(0));
}
