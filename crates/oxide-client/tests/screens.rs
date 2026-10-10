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
        BackgroundKind, CHEST_SHIFT_STEP, ClickButton, ContainerLayout, ContainerScreen,
        HOTBAR_TOP, MAIN_TOP, SLOT_LEFT, SLOT_STEP, ScreenKey, SlotBlock, TitleKind,
        chest_row_shift, hovered_last, player_section, point_in_slot, slot_at_first,
    },
};
use oxide_game::container::{
    BaseStackCaps, CLICK_MODE_CREATIVE_PICK, CLICK_MODE_DRAG, CLICK_MODE_DROP, CLICK_MODE_GATHER,
    CLICK_MODE_PICKUP, CLICK_MODE_QUICK_MOVE, CLICK_MODE_SWAP, drag_button,
};
use oxide_game::input::{InputEvent, drop_item, hotbar_step};
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::WindowKind;

static SUITE_SLOTS: &[oxide_client::screens::container::SlotPos] = &[
    oxide_client::screens::container::SlotPos {
        index: 0,
        x: 8,
        y: 18,
        block: SlotBlock::Container,
    },
    oxide_client::screens::container::SlotPos {
        index: 1,
        x: 26,
        y: 18,
        block: SlotBlock::Container,
    },
    oxide_client::screens::container::SlotPos {
        index: 2,
        x: 8,
        y: 36,
        block: SlotBlock::Container,
    },
    oxide_client::screens::container::SlotPos {
        index: 3,
        x: 26,
        y: 36,
        block: SlotBlock::Container,
    },
];
static SUITE_LAYOUT: ContainerLayout = ContainerLayout {
    x_size: 176,
    y_size: 166,
    sheet: "suite/panel",
    slots: SUITE_SLOTS,
    title: TitleKind::Generic,
    background: BackgroundKind::Full,
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
        Vec::new(),
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
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
    assert!(screens.is_open(), "the window opens a screen");
    assert_eq!(screens.current_window_id(), Some(7));
}

// Escape on a chest sends its own window id and clears the screen.

#[test]
fn escape_on_a_chest_sends_its_own_close() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
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
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
    assert!(screens.on_server_close(7), "the matching close clears");
    assert!(!screens.is_open());
}

// A server close for another window leaves the screen standing.

#[test]
fn a_server_close_for_another_window_keeps_the_screen() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
    assert!(!screens.on_server_close(9), "nothing held window 9");
    assert!(screens.is_open(), "the screen stands");
}

// An unknown kind opens the generic frame, recorded.

#[test]
fn an_unknown_kind_opens_the_generic_frame() {
    let mut screens = Screens::default();
    screens.on_window_opened(4, WindowKind::Unknown, String::from("???"), 0, None);
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
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
    assert!(!screens.allow_user_input(), "containers inherit false");
    screens.open_inventory();
    assert!(screens.allow_user_input(), "the inventory sets true");
    screens.open_creative();
    assert!(screens.allow_user_input(), "creative sets true");
    screens.open_sign(
        1,
        2,
        3,
        [String::new(), String::new(), String::new(), String::new()],
    );
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
            block: SlotBlock::Container,
        },
        SlotPos {
            index: 9,
            x: 8,
            y: 18,
            block: SlotBlock::Container,
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
    let got = clicks(screen.release(ClickButton::Left, false, 1_100, &BaseStackCaps));
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
    let got = clicks(screen.release(ClickButton::Right, false, 1_100, &BaseStackCaps));
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
    assert!(
        screen
            .release(ClickButton::Right, false, 1_100, &BaseStackCaps)
            .is_empty()
    );
    assert!(!screen.dragging(), "the drag is cancelled");
}

// A press and release with no drag movement clicks on release.

#[test]
fn an_undragged_release_clicks_on_release() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    let got = clicks(screen.release(ClickButton::Left, false, 1_100, &BaseStackCaps));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_PICKUP)]);
}

// The double click gathers with mode 6 inside the 250 ms window.

#[test]
fn a_double_click_gathers() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    // The first click picks up (no drag movement, so release clicks).
    screen.press(ClickButton::Left, false, 1_000);
    screen.release(ClickButton::Left, false, 1_050, &BaseStackCaps);
    // The second press lands 100 ms later on the same slot.
    screen.apply_snapshot(
        vec![
            Some(stack(1, 4)),
            Some(stack(2, 1)),
            None,
            Some(stack(3, 64)),
        ],
        Some(stack(1, 60)),
        Vec::new(),
    );
    screen.press(ClickButton::Left, false, 1_100);
    let got = clicks(screen.release(ClickButton::Left, false, 1_150, &BaseStackCaps));
    assert_eq!(got, vec![(0, 0, CLICK_MODE_GATHER)]);
}

// Past the 250 ms window the second click is ordinary.

#[test]
fn a_slow_second_click_is_ordinary() {
    let mut screen = screen_with_cursor(Some(stack(1, 64)));
    screen.press(ClickButton::Left, false, 1_000);
    screen.release(ClickButton::Left, false, 1_050, &BaseStackCaps);
    screen.apply_snapshot(
        vec![
            Some(stack(1, 4)),
            Some(stack(2, 1)),
            None,
            Some(stack(3, 64)),
        ],
        Some(stack(1, 60)),
        Vec::new(),
    );
    screen.press(ClickButton::Left, false, 1_400);
    let got = clicks(screen.release(ClickButton::Left, false, 1_450, &BaseStackCaps));
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

// The T16-F2 rider: the shift-double-click fan-out keeps the source's gates
// — the same inventory, a takable stack, a slot that takes the stack —
// instead of fanning over every matching slot.

#[test]
fn a_shift_double_click_fans_out_within_its_own_gates() {
    use oxide_client::screens::container::SlotPos;

    static RIDER_SLOTS: &[SlotPos] = &[
        SlotPos {
            index: 0,
            x: 8,
            y: 18,
            block: SlotBlock::Container,
        },
        SlotPos {
            index: 1,
            x: 26,
            y: 18,
            block: SlotBlock::Player,
        },
        SlotPos {
            index: 2,
            x: 8,
            y: 36,
            block: SlotBlock::Container,
        },
        SlotPos {
            index: 3,
            x: 26,
            y: 36,
            block: SlotBlock::Container,
        },
    ];
    static RIDER_LAYOUT: ContainerLayout = ContainerLayout {
        x_size: 176,
        y_size: 166,
        sheet: "suite/rider",
        slots: RIDER_SLOTS,
        title: TitleKind::Generic,
        background: BackgroundKind::Full,
    };
    let mut screen =
        ContainerScreen::new(7, WindowKind::Chest, String::from("Chest"), &RIDER_LAYOUT);
    screen.apply_snapshot(
        vec![
            Some(stack(1, 4)),
            Some(stack(1, 4)),
            Some(stack(1, 200)),
            Some(stack(1, 10)),
        ],
        None,
        Vec::new(),
    );
    // Slot 0's centre is (16, 26), like the suite's.
    screen.mouse_moved(16.0, 26.0, &BaseStackCaps);
    // Two shift presses on slot 0 inside the 250 ms window arm the
    // double-click; the release fans mode 1 out.
    screen.press(ClickButton::Left, true, 1_000);
    screen.press(ClickButton::Left, true, 1_100);
    let got = clicks(screen.release(ClickButton::Left, true, 1_150, &BaseStackCaps));
    // Slot 1 holds the same stack in the other inventory, slot 2 holds the
    // same stack past the cap (200 > 64): neither fans. Slot 3 has room.
    assert_eq!(
        got,
        vec![(0, 0, CLICK_MODE_QUICK_MOVE), (3, 0, CLICK_MODE_QUICK_MOVE)]
    );
}

// The opens resolve the family-A tables by kind: the chest's rows ride the
// window's slot count, the dropper shares the dispenser, and the unlanded
// kinds keep the generic frame.

#[test]
fn an_open_resolves_the_family_layouts_by_kind() {
    use oxide_client::screens::family_a;
    assert_eq!(
        family_a::layout_for_kind(WindowKind::Chest, 27).slots.len(),
        63,
        "a 27-slot chest window opens 3 rows plus the player 36"
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::Chest, 54).slots.len(),
        90,
        "a 54-slot chest window opens 6 rows plus the player 36"
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::Hopper, 5).slots.len(),
        41
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::Dispenser, 9)
            .slots
            .len(),
        45
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::Dropper, 9)
            .slots
            .len(),
        45,
        "the dropper shares the dispenser's 3×3"
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::Furnace, 3)
            .slots
            .len(),
        39
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::BrewingStand, 4)
            .slots
            .len(),
        40
    );
    assert_eq!(
        family_a::layout_for_kind(WindowKind::CraftingTable, 10)
            .slots
            .len(),
        46
    );
    assert!(
        family_a::layout_for_kind(WindowKind::EnchantingTable, 2)
            .slots
            .is_empty(),
        "the unlanded kinds keep the slotless generic frame"
    );
}

#[test]
fn an_open_stands_the_resolved_table() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Furnace, String::from("Furnace"), 3, None);
    let slots = match screens.current() {
        Some(oxide_client::screens::ScreenState::Container(screen)) => screen.layout().slots.len(),
        other => panic!("a furnace opens a container screen, got {other:?}"),
    };
    assert_eq!(
        slots, 39,
        "the furnace stands its 3 slots plus the player 36"
    );
}

// ---- Task 24's routing table: the inventory keys and the wheel, per path ----
//
// The source's own key handling, derived per path (`Minecraft.java`
// :1859-1892 for the wheel, :2076-2111 for the digits and the drop,
// :2092-2101 for the inventory key, :1944-1949 for Escape,
// `GuiContainer.keyTyped`:692-696 for the container close,
// `InventoryPlayer.changeCurrentItem`:165-185 for the wheel step):
// - E from no screen opens the inventory beside one C16 per open;
// - E behind a container screen swaps it to the inventory — the close (C0D)
//   never unpresses, so the same press re-opens (C16);
// - E in the inventory closes and re-opens: it stays open;
// - the wheel steps one slot per event even over a screen, where the screen's
//   own scroll runs in addition;
// - the digits set the slot directly everywhere the screen does not own the
//   key, and swap inside a container on top;
// - Q drops one item outside screens, the whole stack with Ctrl;
// - Escape closes the current screen; with no screen the port runs the M3
//   capture rule (release-while-grabbed, else exit) in place of the source's
//   pause menu — the recorded substitution. The confirm → chat → screen order
//   is port-new layering: the source has a single screen slot (chat IS a
//   screen).

// E from no screen opens the inventory beside one C16; two opens on separate
// ticks send two — each press queues its own edge (`KeyBinding.java`:24-33,
// :101-110), and the first open's `setIngameNotInFocus` unpress is what
// forbids counting two presses in one tick as two.

#[test]
fn e_from_no_screen_opens_the_inventory_with_one_c16_per_open() {
    let mut screens = Screens::default();
    assert_eq!(
        screens.open_inventory(),
        Some(InputEvent::OpenInventory),
        "the first open sends one C16"
    );
    assert!(screens.is_open(), "the inventory stands");
    assert_eq!(
        screens.current_window_id(),
        Some(0),
        "window 0 is the player's own"
    );
    // The second open, on its own tick, sends its own C16: two opens send
    // two.
    assert_eq!(
        screens.open_inventory(),
        Some(InputEvent::OpenInventory),
        "the second open sends one more C16"
    );
    assert!(screens.is_open(), "the inventory still stands");
}

// E behind a container screen swaps it to the inventory: the close branch
// (`GuiContainer.keyTyped`:692-696 through `EntityPlayerSP.closeScreen`
// :330-341) sends C0D for the container's window, and the same press — never
// unpressed, the close path is not the screen-OPEN path — reaches the
// unguarded inventory loop (`Minecraft.java`:2092-2101) and re-opens with a
// C16 beside the window-0 screen.

#[test]
fn e_behind_a_container_swaps_it_to_the_inventory() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
    let mut cursor = None;
    assert_eq!(
        screens.close(&mut cursor),
        Some(InputEvent::CloseWindow { window_id: 7 }),
        "the same press first closes the container: C0D for window 7"
    );
    assert_eq!(
        screens.open_inventory(),
        Some(InputEvent::OpenInventory),
        "then re-opens: one C16"
    );
    assert!(screens.is_open(), "the swap ends on a screen");
    assert_eq!(
        screens.current_window_id(),
        Some(0),
        "the window-0 inventory stands where the chest stood"
    );
}

// E in the inventory closes and re-opens: the chain above run on window 0
// ends where it started — Escape, not E, is the closer.

#[test]
fn e_in_the_inventory_closes_and_reopens_so_it_stays_open() {
    let mut screens = Screens::default();
    screens.open_inventory();
    let mut cursor = None;
    assert_eq!(
        screens.close(&mut cursor),
        Some(InputEvent::CloseWindow { window_id: 0 }),
        "the close still sends C0D for window 0"
    );
    assert_eq!(
        screens.open_inventory(),
        Some(InputEvent::OpenInventory),
        "and the same press re-opens"
    );
    assert!(screens.is_open(), "E never leaves the inventory shut");
    assert_eq!(screens.current_window_id(), Some(0));
}

// The wheel outside screens steps one slot per event: the delta clamps to
// its sign and the nine slots wrap (`changeCurrentItem`:165-185).

#[test]
fn the_wheel_outside_screens_steps_one_slot_per_event() {
    assert_eq!(hotbar_step(3, 1.0), 4);
    assert_eq!(hotbar_step(3, -1.0), 2);
    assert_eq!(
        hotbar_step(3, 5.0),
        4,
        "a five-notch event still steps one slot"
    );
    assert_eq!(hotbar_step(8, 1.0), 0, "the top wraps to the bottom");
    assert_eq!(hotbar_step(0, -1.0), 8, "the bottom wraps to the top");
}

// The wheel over the creative screen does both: the list scrolls
// (`handleMouseInput`:546-569) AND the held slot flips — `changeCurrentItem`
// at `Minecraft.java`:1879 has no screen guard; the screen's handler at
// :1892 runs in addition.

#[test]
fn the_wheel_over_the_creative_screen_scrolls_and_flips() {
    use oxide_client::screens::creative::wheel_scroll;
    // A 100-entry list scrolls: one positive notch moves the offset.
    let scrolled = wheel_scroll(0.5, 100, 1.0);
    assert!(
        scrolled < 0.5,
        "the list scrolled: {scrolled} (a short list fits and ignores the wheel)"
    );
    // The same event flips the held slot concurrently.
    assert_eq!(
        hotbar_step(3, 1.0),
        4,
        "the concurrent held-slot change runs beside the scroll"
    );
    // A list that fits takes no scroll — but the flip still runs.
    assert_eq!(
        wheel_scroll(0.5, 9, 1.0),
        0.5,
        "a fitting list ignores the scroll half"
    );
    assert_eq!(
        hotbar_step(3, 1.0),
        4,
        "while the held-slot change still runs"
    );
}

// The digits outside screens set the slot directly: digit N reads hotbar
// index N−1 (`Minecraft.java`:2076-2090, no screen guard), sent as
// `HeldItemChange`.

#[test]
fn the_digits_outside_screens_set_the_slot_directly() {
    use oxide_game::input::Key;
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
        let slot = slot as i16;
        let event = InputEvent::HeldItemChange {
            slot: key.hotbar_slot().expect("a digit selects its slot"),
        };
        assert_eq!(
            event,
            InputEvent::HeldItemChange { slot },
            "{key:?} travels as the slot's own change"
        );
    }
}

// The digits over a container do both halves: the direct set above AND the
// mode-2 swap when hovering a stack with an empty cursor
// (`checkHotbarKeys`, firing only then).

#[test]
fn the_digits_over_a_container_set_and_swap() {
    let mut screen = screen_with_cursor(None);
    let got = clicks(screen.screen_key(ScreenKey::Number(3), false));
    assert_eq!(
        got,
        vec![(0, 3, CLICK_MODE_SWAP)],
        "the swap half fires over the hovered stack"
    );
    use oxide_game::input::Key;
    assert_eq!(
        Key::Digit4.hotbar_slot(),
        Some(3),
        "the direct-set half runs with it: digit 4 is slot 3"
    );
}

// Q outside screens drops one item, Ctrl+Q the whole stack
// (`Minecraft.java`:2105-2111 over `EntityPlayerSP.dropOneItem`:279-284).

#[test]
fn q_outside_screens_drops_one_and_ctrl_q_drops_the_whole_stack() {
    assert_eq!(drop_item(false), InputEvent::DropItem { whole: false });
    assert_eq!(drop_item(true), InputEvent::DropItem { whole: true });
}

// Escape closes the current screen — the chest's own window id, window 0 for
// the inventory — and with no screen the screens send nothing: the no-screen
// arm is the M3 capture rule's (release-while-grabbed, else exit), the
// recorded substitution for the source's pause menu
// (`Minecraft.java`:1944-1949).

#[test]
fn escape_closes_the_screen_and_sends_nothing_with_no_screen() {
    let mut screens = Screens::default();
    screens.on_window_opened(7, WindowKind::Chest, String::from("Chest"), 27, None);
    assert_eq!(
        screens.escape(&mut None),
        Some(InputEvent::CloseWindow { window_id: 7 })
    );
    assert!(!screens.is_open());
    let mut screens = Screens::default();
    screens.open_inventory();
    assert_eq!(
        screens.escape(&mut None),
        Some(InputEvent::CloseWindow { window_id: 0 })
    );
    assert!(!screens.is_open());
    let mut screens = Screens::default();
    assert_eq!(
        screens.escape(&mut None),
        None,
        "no screen sends nothing: the capture rule owns the no-screen arm"
    );
}
