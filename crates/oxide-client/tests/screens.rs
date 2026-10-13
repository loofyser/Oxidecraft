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
// - Escape closes the current screen; with no screen the client stands the
//   pause menu while a session runs, and the M3 capture rule owns the arm
//   otherwise (release-while-grabbed, else exit). The confirm → chat → screen order
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
// arm stands the pause menu while a session runs (`Minecraft.java`:1944-1949),
// else the M3 capture rule owns it (release-while-grabbed, else exit).

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
        "no screen sends nothing: the pause menu stands above it at the client level"
    );
}

// Task 15's menu suite: the widget laws, the panorama/splash formulae, the
// main menu's layout pins and the pre-session standing rules.
//
// The widget pins mirror the source's button (`GuiButton.java`) and slider
// (`GuiOptionSlider.java`); the layout pins mirror the main menu's
// `initGui`/`addSingleplayerMultiplayerButtons` at pinned scaled sizes;
// the splash pins mirror the constructor's pick loop and the calendar
// arms. Every formula here is closed-form — no GPU needed.

use oxide_client::screens::menu::{
    MENU_MULTIPLAYER_ID, MENU_OPTIONS_ID, MENU_QUIT_ID, MENU_SINGLEPLAYER_ID, MainMenuScreen,
    SPLASH_ANGLE_DEG, SPLASH_DEFAULT, SPLASH_EXCLUDED_HASH, SPLASH_EXCLUDED_TEXT,
    calendar_override, civil_month_day, java_hash, menu_buttons, menu_labels, panorama_face,
    panorama_pitch, panorama_yaw, pick_splash, splash_scale, stands_at_startup,
};
use oxide_client::screens::widgets::{
    BUTTON_HEIGHT, BUTTON_WIDTH, Button, LABEL_DISABLED, LABEL_HOVERED, LABEL_IDLE, ROW_WIDTH,
    SLIDER_KNOB_HALF, SLIDER_KNOB_V, Slider, button_strip_v, hover_state, label_colour,
};
use oxide_render::hud::ScaledResolution;

/// The suite's scaled sizes: the wide layout pin and the minimum-scale
/// on-screen check (320x240 keeps `j = h/4+48` on screen).
fn wide() -> ScaledResolution {
    ScaledResolution {
        width: 427,
        height: 240,
        scale_factor: 2,
    }
}

fn minimum() -> ScaledResolution {
    ScaledResolution {
        width: 320,
        height: 240,
        scale_factor: 3,
    }
}

fn english() -> [String; 4] {
    [
        String::from("Singleplayer"),
        String::from("Multiplayer"),
        String::from("Options..."),
        String::from("Quit Game"),
    ]
}

#[test]
fn the_hover_state_table_pins_disabled_hovered_and_idle() {
    // `GuiButton.getHoverState`:59-67 — 0 disabled, 2 hovered, else 1.
    assert_eq!(hover_state(false, false), 0);
    assert_eq!(hover_state(false, true), 0);
    assert_eq!(hover_state(true, false), 1);
    assert_eq!(hover_state(true, true), 2);
}

#[test]
fn the_button_strip_reads_46_plus_state_times_20() {
    // `drawButton`'s two blits ride v `46 + i * 20`.
    assert_eq!(button_strip_v(0), 46.0);
    assert_eq!(button_strip_v(1), 66.0);
    assert_eq!(button_strip_v(2), 86.0);
}

#[test]
fn the_label_colours_pin_the_three_arms() {
    // `drawButton`'s colour arms: 14737632 idle, 10526880 disabled,
    // 16777120 hovered.
    assert_eq!(LABEL_IDLE, 14737632);
    assert_eq!(LABEL_DISABLED, 10526880);
    assert_eq!(LABEL_HOVERED, 16777120);
    assert_eq!(
        label_colour(true, false),
        [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0]
    );
    assert_eq!(
        label_colour(false, false),
        [160.0 / 255.0, 160.0 / 255.0, 160.0 / 255.0, 1.0]
    );
    assert_eq!(label_colour(true, true), [1.0, 1.0, 160.0 / 255.0, 1.0]);
    assert_eq!(
        label_colour(false, true),
        [160.0 / 255.0, 160.0 / 255.0, 160.0 / 255.0, 1.0]
    );
}

#[test]
fn the_button_base_is_200_by_20_and_the_row_is_150() {
    // `GuiButton.java`:36-39 build 200x20; the options-row controls build
    // 150x20 (`GuiOptionButton.java`:22, `GuiOptionSlider.java`:23).
    assert_eq!((BUTTON_WIDTH, BUTTON_HEIGHT), (200.0, 20.0));
    assert_eq!(ROW_WIDTH, 150.0);
}

#[test]
fn the_button_hit_box_is_the_full_rect_and_the_press_gates() {
    // `mousePressed`:126-129 — enabled, visible, and the whole rect.
    let mut button = Button::new(1, 100.0, 60.0, 200.0, 20.0, String::from("Singleplayer"));
    assert!(button.hit((100.0, 60.0)));
    assert!(button.hit((299.0, 79.0)));
    assert!(!button.hit((300.0, 60.0)), "the right edge is exclusive");
    assert!(!button.hit((100.0, 80.0)), "the bottom edge is exclusive");
    assert!(!button.hit((99.0, 60.0)));
    assert!(button.pressed((150.0, 70.0)));
    button.enabled = false;
    assert!(!button.pressed((150.0, 70.0)), "disabled never presses");
    button.enabled = true;
    button.visible = false;
    assert!(!button.pressed((150.0, 70.0)), "hidden never presses");
}

#[test]
fn the_slider_knob_follows_the_value_and_the_drag_latches() {
    // `mouseDragged`'s blit origin `x + value * (w - 8)`; the value law
    // `(mouseX - (x + 4)) / (w - 8)` clamped; the press-to-release latch.
    let mut slider = Slider::new(10.0, 40.0, 150.0, 20.0, 0.5);
    assert_eq!(slider.knob_x(), 10.0 + 0.5 * 142.0);
    slider.set_from_mouse(10.0 + 4.0);
    assert_eq!(slider.value, 0.0, "the left inset clamps to zero");
    slider.set_from_mouse(10.0 + 4.0 + 142.0);
    assert_eq!(slider.value, 1.0, "the right inset clamps to one");
    slider.set_from_mouse(10.0 + 4.0 + 71.0);
    assert_eq!(slider.value, 0.5);
    assert!(slider.press((50.0, 50.0)));
    assert!(slider.dragging, "the press latches");
    slider.drag_to(10.0 + 4.0);
    assert_eq!(slider.value, 0.0, "the latched drag folds");
    slider.release();
    assert!(!slider.dragging, "the release clears");
    slider.drag_to(10.0 + 4.0 + 142.0);
    assert_eq!(slider.value, 0.0, "the unlatched drag folds nothing");
}

#[test]
fn the_slider_track_never_highlights() {
    // `getHoverState` returns 0 always: the track draws the v 46 strip
    // under the pointer too, while the knob halves ride v 66.
    assert_eq!(SLIDER_KNOB_V, 66.0);
    assert_eq!(SLIDER_KNOB_HALF, 4.0);
    let slider = Slider::new(10.0, 40.0, 150.0, 20.0, 0.5);
    let draws = slider.draws();
    assert_eq!(draws.len(), 4, "the track halves and the knob halves");
}

#[test]
fn the_splash_angle_is_minus_20_degrees() {
    // `drawScreen`:562 rotates −20 about Z — the plan's "45" corrected.
    assert_eq!(SPLASH_ANGLE_DEG, -20.0);
}

#[test]
fn the_splash_scale_law_pins_period_and_amplitude() {
    // `(1.8 - |sin(2*pi*(ms%1000)/1000)| * 0.1) * 100/(string+32)`
    // (`drawScreen`:563-564): a 1 s period, ±0.1/1.8 amplitude before the
    // string-width scale.
    let wide = splash_scale(0, 100);
    assert_eq!(wide, 1.8 * 100.0 / 132.0, "sin(0) = 0 leaves 1.8");
    let quarter = splash_scale(250, 100);
    assert!(
        (quarter - 1.7 * 100.0 / 132.0).abs() < 1e-5,
        "sin(pi/2) = 1 leaves 1.7, up to float rounding"
    );
    assert_eq!(
        splash_scale(1000, 100),
        wide,
        "the millisecond remainder sets the 1 s period"
    );
    assert_eq!(
        splash_scale(500, 100),
        splash_scale(0, 100),
        "|sin(pi)| = 0 rejoins the peak"
    );
}

#[test]
fn the_panorama_laws_pin_the_source_rates() {
    // Yaw `-(timer+partial) * 0.1` at 20 Hz = 2 deg/s, a 180 s/rev
    // period; pitch `sin((timer+partial)/400) * 25 + 20`.
    assert_eq!(panorama_yaw(0, 0.0), 0.0);
    assert_eq!(panorama_yaw(20, 0.0), -2.0, "one second turns two degrees");
    assert_eq!(
        panorama_yaw(3600, 0.0),
        -360.0,
        "180 s turns one revolution"
    );
    assert_eq!(panorama_yaw(0, 0.5), -0.05, "the partial fraction folds in");
    assert_eq!(panorama_pitch(0, 0.0), 20.0, "sin(0) leaves the +20 base");
    let quarter_period = (std::f32::consts::PI / 2.0 * 400.0) as i64;
    assert!(
        (panorama_pitch(quarter_period, 0.0) - 45.0).abs() < 0.01,
        "a quarter pitch period peaks at +45"
    );
}

#[test]
fn the_panorama_face_walks_the_six_sides_in_order() {
    // The yaw folded into sextants over faces 0..5: two yaws half a
    // revolution apart face opposite sides.
    assert_eq!(panorama_face(0.0), 0);
    assert_eq!(panorama_face(-60.0), 1);
    assert_eq!(panorama_face(-120.0), 2);
    assert_eq!(panorama_face(-180.0), 3);
    assert_eq!(panorama_face(-240.0), 4);
    assert_eq!(panorama_face(-300.0), 5);
    assert_eq!(panorama_face(-360.0), 0, "one revolution rejoins face 0");
}

#[test]
fn the_java_hash_names_the_excluded_splash() {
    // The re-roll rule's verbatim constant, and the splash it names.
    assert_eq!(SPLASH_EXCLUDED_HASH, 125780783);
    assert_eq!(java_hash(SPLASH_EXCLUDED_TEXT), 125780783);
    assert_eq!(java_hash("missingno"), 1243860391);
}

#[test]
fn the_splash_pick_trims_skips_defaults_and_rerolls() {
    // The constructor's loop: trim, skip empties, `missingno` when nothing
    // survives, re-roll while the hash hits the exclusion.
    let lines = vec![
        String::from("   "),
        String::from(""),
        String::from("world"),
        String::from("  hello  "),
    ];
    assert_eq!(pick_splash(&lines, 7), "hello", "the pick trims");
    assert_eq!(pick_splash(&[], 7), SPLASH_DEFAULT);
    assert_eq!(pick_splash(&[String::from("  ")], 7), SPLASH_DEFAULT);
    let exclusion = vec![String::from(SPLASH_EXCLUDED_TEXT), String::from("safe")];
    for seed in 0..64u64 {
        assert_ne!(
            pick_splash(&exclusion, seed),
            SPLASH_EXCLUDED_TEXT,
            "the exclusion never survives while an alternative stands (seed {seed})"
        );
    }
    let single = vec![String::from("safe")];
    assert_eq!(
        pick_splash(&single, 3),
        "safe",
        "a lone survivor returns at once"
    );
    // Seed 4096 draws index 0 (the exclusion) first, so this assert — and
    // only this assert in the case — executes the re-roll branch: the
    // 0..64 loop above never draws the exclusion first and passes even
    // with the re-roll removed.
    assert_eq!(
        pick_splash(&exclusion, 4096),
        "safe",
        "seed 4096 draws the exclusion first, so the re-roll must fire"
    );
}

#[test]
fn the_calendar_windows_override_the_pick() {
    // `initGui`'s three arms: Dec 24, Jan 1, Oct 31.
    assert_eq!(calendar_override(12, 24), Some("Merry X-mas!"));
    assert_eq!(calendar_override(1, 1), Some("Happy new year!"));
    assert_eq!(calendar_override(10, 31), Some("OOoooOOOoooo! Spooky!"));
    assert_eq!(calendar_override(12, 25), None);
    assert_eq!(calendar_override(6, 15), None);
    assert_eq!(civil_month_day(0), (1, 1), "the epoch opens in January");
    assert_eq!(
        civil_month_day(20447),
        (12, 25),
        "2025-12-25 reads Christmas"
    );
}

#[test]
fn the_menu_buttons_pin_the_source_layout() {
    // `j = h/4+48`, spacing 24: single (1) `w/2-100, j` 200x20, multi (2)
    // `j+24`, options (0) `w/2-100, j+84` 98x20, quit (4) `w/2+2, j+84`
    // 98x20. Realms (14) and language (5) are omitted.
    let buttons = menu_buttons(
        &wide(),
        english()[0].clone(),
        english()[1].clone(),
        english()[2].clone(),
        english()[3].clone(),
    );
    assert_eq!(buttons.len(), 4, "realms and language are omitted");
    let at = |id: i32| {
        buttons
            .iter()
            .find(|button| button.id == id)
            .expect("the button stands")
    };
    assert_eq!(
        (at(MENU_SINGLEPLAYER_ID).x, at(MENU_SINGLEPLAYER_ID).y),
        (113.0, 108.0)
    );
    assert_eq!(
        (at(MENU_SINGLEPLAYER_ID).w, at(MENU_SINGLEPLAYER_ID).h),
        (200.0, 20.0)
    );
    assert_eq!(
        (at(MENU_MULTIPLAYER_ID).x, at(MENU_MULTIPLAYER_ID).y),
        (113.0, 132.0)
    );
    assert_eq!(
        (at(MENU_OPTIONS_ID).x, at(MENU_OPTIONS_ID).y),
        (113.0, 192.0)
    );
    assert_eq!((at(MENU_OPTIONS_ID).w, at(MENU_OPTIONS_ID).h), (98.0, 20.0));
    assert_eq!((at(MENU_QUIT_ID).x, at(MENU_QUIT_ID).y), (215.0, 192.0));
    assert_eq!((at(MENU_QUIT_ID).w, at(MENU_QUIT_ID).h), (98.0, 20.0));
    assert_eq!(at(MENU_SINGLEPLAYER_ID).label, "Singleplayer");
    assert!(
        buttons
            .iter()
            .all(|button| button.enabled && button.visible),
        "every menu button draws enabled"
    );
}

#[test]
fn the_menu_buttons_stay_on_screen_at_minimum_scale() {
    // `j = h/4+48` keeps the set on screen at 320x240: the quit row's
    // bottom edge must clear the frame.
    let buttons = menu_buttons(
        &minimum(),
        english()[0].clone(),
        english()[1].clone(),
        english()[2].clone(),
        english()[3].clone(),
    );
    for button in &buttons {
        assert!(
            button.x >= 0.0 && button.y >= 0.0,
            "button {} starts on screen",
            button.id
        );
        assert!(
            button.x + button.w <= 320.0 && button.y + button.h <= 240.0,
            "button {} ends on screen",
            button.id
        );
    }
}

#[test]
fn the_menu_click_routes_press_to_action_and_singleplayer_stays_inert() {
    // The press fires on press (`mouseClicked`); singleplayer records
    // inert (Decision 7) — the action answers, nothing opens.
    let menu = MainMenuScreen::new(&wide(), &[], 11, english());
    let single = menu_buttons(
        &wide(),
        english()[0].clone(),
        english()[1].clone(),
        english()[2].clone(),
        english()[3].clone(),
    )
    .into_iter()
    .find(|button| button.id == MENU_SINGLEPLAYER_ID)
    .expect("singleplayer stands");
    let point = (single.x + 1.0, single.y + 1.0);
    assert_eq!(
        menu.click_at(point),
        Some(oxide_client::screens::menu::MenuAction::Singleplayer)
    );
    let quit = menu_buttons(
        &wide(),
        english()[0].clone(),
        english()[1].clone(),
        english()[2].clone(),
        english()[3].clone(),
    )
    .into_iter()
    .find(|button| button.id == MENU_QUIT_ID)
    .expect("quit stands");
    assert_eq!(
        menu.click_at((quit.x + 1.0, quit.y + 1.0)),
        Some(oxide_client::screens::menu::MenuAction::Quit)
    );
    assert_eq!(
        menu.click_at((0.0, 0.0)),
        None,
        "empty space answers nothing"
    );
    assert_eq!(
        menu.click_at((113.0, 156.0)),
        None,
        "the realms gap answers nothing"
    );
}

#[test]
fn the_menu_labels_resolve_english_without_a_table() {
    // No table hands over: the store's `en_US.lang` English the buttons
    // stand on.
    assert_eq!(
        menu_labels(None),
        [
            String::from("Singleplayer"),
            String::from("Multiplayer"),
            String::from("Options..."),
            String::from("Quit Game"),
        ]
    );
}

#[test]
fn the_menu_stands_at_startup_without_a_server_only() {
    // The pre-session rule: no `--server` stands the menu, `--server`
    // bypasses it for the session join.
    assert!(stands_at_startup(None));
    assert!(!stands_at_startup(Some("127.0.0.1:25565")));
}

#[test]
fn the_multiplayer_list_geometry_pins() {
    // `GuiMultiplayer.java` (`initGui`): the list spans rows of 36 px from 32
    // down to `height - 64`.
    use oxide_client::screens::multiplayer::{
        LIST_BOTTOM_OFFSET, LIST_TOP, ROW_HEIGHT, list_bottom, row_index_at,
    };
    assert_eq!(LIST_TOP, 32);
    assert_eq!(ROW_HEIGHT, 36);
    assert_eq!(LIST_BOTTOM_OFFSET, 64);
    assert_eq!(list_bottom(600), 536);
    assert_eq!(
        row_index_at(32, 600, 4),
        Some(0),
        "the top edge opens the first row"
    );
    assert_eq!(
        row_index_at(67, 600, 4),
        Some(0),
        "a row spans its full 36 px"
    );
    assert_eq!(row_index_at(68, 600, 4), Some(1));
    assert_eq!(
        row_index_at(535, 600, 4),
        None,
        "past the entries is no row, even at the edge"
    );
    assert_eq!(row_index_at(536, 600, 4), None, "past the bottom is no row");
    assert_eq!(row_index_at(10, 600, 4), None, "above the top is no row");
    assert_eq!(row_index_at(68, 600, 1), None, "past the entries is no row");
}

#[test]
fn the_multiplayer_buttons_carry_the_source_ids_and_rects() {
    // `GuiMultiplayer.java` (`createButtons`): the seven buttons' ids and
    // rects at 800x600.
    use oxide_client::screens::multiplayer::{MultiplayerLabels, multiplayer_buttons};
    let labels = MultiplayerLabels {
        title: String::from("Play Multiplayer"),
        form_title: String::from("Edit Server Info"),
        name_caption: String::from("Server Name"),
        address_caption: String::from("Server Address"),
        buttons: [
            String::from("Edit"),
            String::from("Delete"),
            String::from("Join Server"),
            String::from("Direct Connect"),
            String::from("Add server"),
            String::from("Refresh"),
            String::from("Cancel"),
        ],
    };
    let buttons = multiplayer_buttons(800, 600, &labels);
    let rects: Vec<(i32, f32, f32, f32, f32)> = buttons
        .iter()
        .map(|button| (button.id, button.x, button.y, button.w, button.h))
        .collect();
    assert_eq!(
        rects,
        vec![
            (7, 246.0, 572.0, 70.0, 20.0),
            (2, 326.0, 572.0, 70.0, 20.0),
            (1, 246.0, 548.0, 100.0, 20.0),
            (4, 350.0, 548.0, 100.0, 20.0),
            (3, 454.0, 548.0, 100.0, 20.0),
            (8, 404.0, 572.0, 70.0, 20.0),
            (0, 480.0, 572.0, 75.0, 20.0),
        ],
        "ids and rects match the source's button build"
    );
}

#[test]
fn the_latency_level_table_pins() {
    // `ServerListEntryNormal.java`: the icon's level reads the round trip —
    // skew draws 5, the in-flight ping draws the animated branch, a failed
    // ping draws 5, then the `<150 → 0, <300 → 1, <600 → 2, <1000 → 3, else 4`
    // thresholds.
    use oxide_client::screens::multiplayer::{animated_latency_frame, latency_level};
    assert_eq!(latency_level(10, true), Some(5), "version skew draws 5");
    assert_eq!(
        latency_level(-2, false),
        None,
        "the in-flight ping animates"
    );
    assert_eq!(latency_level(-1, false), Some(5), "a failed ping draws 5");
    assert_eq!(latency_level(0, false), Some(0));
    assert_eq!(latency_level(149, false), Some(0));
    assert_eq!(latency_level(150, false), Some(1));
    assert_eq!(latency_level(299, false), Some(1));
    assert_eq!(latency_level(300, false), Some(2));
    assert_eq!(latency_level(599, false), Some(2));
    assert_eq!(latency_level(600, false), Some(3));
    assert_eq!(latency_level(999, false), Some(3));
    assert_eq!(latency_level(1000, false), Some(4));
    assert_eq!(latency_level(5000, false), Some(4));
    // The animated branch: `(systemTime / 100 + slot * 2) & 7`, mirrored past 4.
    assert_eq!(animated_latency_frame(0, 0), 0);
    assert_eq!(animated_latency_frame(500, 0), 3, "5 mirrors to 3");
    assert_eq!(animated_latency_frame(200, 1), 4);
}

#[test]
fn a_coloured_motd_renders_its_section_codes() {
    // R4: the response's raw description renders through the standing
    // `parse_json → to_legacy` path, so a coloured canned MOTD carries its
    // codes — the `text`-only model drops them.
    use oxide_client::screens::multiplayer::motd_legacy;
    use oxide_render::text::decode_legacy;
    let legacy = motd_legacy(r#"{"text":"Hi","color":"red"}"#);
    assert_eq!(legacy, "§cHi§r", "the style survives the path");
    let runs = decode_legacy(&legacy);
    assert_eq!(runs.len(), 1, "one styled run draws");
    assert_eq!(runs[0].text, "Hi");
    assert_eq!(
        runs[0].colour,
        Some(oxide_render::text::colour_code(12)),
        "red is the §c cell"
    );
}

#[test]
fn the_population_and_version_lines_pin() {
    // `OldServerPinger.java`: grey `online`, dark-grey `/`, grey `max`;
    // `???` in dark grey with no counts. `ServerListEntryNormal.java`: skew
    // draws the version name in dark red.
    use oxide_client::screens::multiplayer::{player_list_tooltip, population_text};
    assert_eq!(population_text(Some(3), Some(20)), "§73§8/§720");
    assert_eq!(population_text(None, None), "§8???");
    assert_eq!(
        player_list_tooltip(&["Ada".to_string(), "Grace".to_string()], 2),
        Some("Ada\nGrace".to_string()),
        "the sample's names join with newlines"
    );
    assert_eq!(
        player_list_tooltip(&["Ada".to_string()], 3),
        Some("Ada\n... and 2 more ...".to_string()),
        "a short sample tails the remainder"
    );
    assert_eq!(
        player_list_tooltip(&[], 0),
        None,
        "no sample draws no tooltip"
    );
}

// Task 17's pause suite: the pause menu's six-button set and layout, the
// placeholder statistics/achievements/snooper frames, and the Escape chain
// (play → pause; pause → play by button and by Escape with no send;
// pause ↔ statistics/achievements; the snooper stands with its declared
// parent).
//
// The layout pins mirror `GuiIngameMenu.java:19-39` (`i = -16`): Back to
// Game (4) 200 wide at `h/4+8`; Achievements (5) + Statistics (6) as the
// 98-wide pair at `h/4+32`; Options (0) + Open to LAN (7) as the 98-wide
// pair at `h/4+80`; Disconnect (1) 200 wide at `h/4+104`. LAN is always
// disabled (R1 — no integrated server exists, so `:38` is permanently
// false); Disconnect always reads "Disconnect" (R2). Every formula here is
// closed-form — no GPU needed.

use oxide_client::screens::pause::{
    ACHIEVEMENTS_DONE_ID, DIRT_SPAN, PAUSE_ACHIEVEMENTS_ID, PAUSE_BACK_ID, PAUSE_DISCONNECT_ID,
    PAUSE_LAN_ID, PAUSE_OPTIONS_ID, PAUSE_STATS_ID, SNOOPER_DONE_ID, STATS_DONE_ID, SnooperParent,
    achievements_title, dirt_draws, done_label, pause_buttons, pause_labels, pause_title,
    snooper_title, stats_title,
};
use oxide_client::screens::pause::{AchievementsAction, PauseAction, SnooperAction, StatsAction};

/// The pause suite's English labels in button order: back, achievements,
/// statistics, options, LAN, disconnect.
fn pause_english() -> [String; 6] {
    [
        String::from("Back to Game"),
        String::from("Achievements"),
        String::from("Statistics"),
        String::from("Options..."),
        String::from("Open to LAN"),
        String::from("Disconnect"),
    ]
}

#[test]
fn the_pause_menu_pins_the_six_button_set() {
    // `GuiIngameMenu.java:19-39` at the wide size (427x240: half 213,
    // quarter 60): the ids, the rects, the pair widths, and LAN disabled.
    let buttons = pause_buttons(&wide(), pause_english());
    let rects: Vec<(i32, f32, f32, f32, f32, bool)> = buttons
        .iter()
        .map(|button| {
            (
                button.id,
                button.x,
                button.y,
                button.w,
                button.h,
                button.enabled,
            )
        })
        .collect();
    assert_eq!(
        rects,
        vec![
            (PAUSE_DISCONNECT_ID, 113.0, 164.0, 200.0, 20.0, true),
            (PAUSE_BACK_ID, 113.0, 68.0, 200.0, 20.0, true),
            (PAUSE_OPTIONS_ID, 113.0, 140.0, 98.0, 20.0, true),
            (PAUSE_LAN_ID, 215.0, 140.0, 98.0, 20.0, false),
            (PAUSE_ACHIEVEMENTS_ID, 113.0, 92.0, 98.0, 20.0, true),
            (PAUSE_STATS_ID, 215.0, 92.0, 98.0, 20.0, true),
        ],
        "ids and rects match the source's button build; LAN alone is disabled"
    );
    assert_eq!(PAUSE_BACK_ID, 4);
    assert_eq!(PAUSE_ACHIEVEMENTS_ID, 5);
    assert_eq!(PAUSE_STATS_ID, 6);
    assert_eq!(PAUSE_OPTIONS_ID, 0);
    assert_eq!(PAUSE_LAN_ID, 7);
    assert_eq!(PAUSE_DISCONNECT_ID, 1);
}

#[test]
fn the_pause_menu_stays_on_screen_at_the_minimum() {
    // 320x240 (half 160, quarter 60): every button keeps its row and sits
    // fully inside the frame.
    let buttons = pause_buttons(&minimum(), pause_english());
    let rects: Vec<(i32, f32, f32, f32, f32)> = buttons
        .iter()
        .map(|button| (button.id, button.x, button.y, button.w, button.h))
        .collect();
    assert_eq!(
        rects,
        vec![
            (PAUSE_DISCONNECT_ID, 60.0, 164.0, 200.0, 20.0),
            (PAUSE_BACK_ID, 60.0, 68.0, 200.0, 20.0),
            (PAUSE_OPTIONS_ID, 60.0, 140.0, 98.0, 20.0),
            (PAUSE_LAN_ID, 162.0, 140.0, 98.0, 20.0),
            (PAUSE_ACHIEVEMENTS_ID, 60.0, 92.0, 98.0, 20.0),
            (PAUSE_STATS_ID, 162.0, 92.0, 98.0, 20.0),
        ]
    );
    for button in &buttons {
        assert!(
            button.x >= 0.0 && button.x + button.w <= 320.0,
            "button {} sits inside the width",
            button.id
        );
        assert!(
            button.y >= 0.0 && button.y + button.h <= 240.0,
            "button {} sits inside the height",
            button.id
        );
    }
}

#[test]
fn the_pause_labels_resolve_english_without_a_table() {
    // No table hands over: the store's `en_US.lang` English the pause menu
    // and the placeholders stand on.
    assert_eq!(
        pause_labels(None),
        [
            String::from("Back to Game"),
            String::from("Achievements"),
            String::from("Statistics"),
            String::from("Options..."),
            String::from("Open to LAN"),
            String::from("Disconnect"),
        ]
    );
    assert_eq!(pause_title(None), String::from("Game menu"));
    assert_eq!(stats_title(None), String::from("Statistics"));
    assert_eq!(achievements_title(None), String::from("Achievements"));
    assert_eq!(
        snooper_title(None),
        String::from("Machine Specs Collection")
    );
    assert_eq!(done_label(None), String::from("Done"));
}

#[test]
fn the_pause_escape_chain_stands_and_returns() {
    // Play → pause; pause → play by Escape with no send; the cursor copy
    // drops with the close.
    let mut screens = Screens::default();
    screens.open_pause(wide(), pause_english());
    assert!(screens.is_open(), "the pause stands");
    assert_eq!(
        screens.current_window_id(),
        None,
        "the pause stands on no window"
    );
    assert!(!screens.allow_user_input(), "the pause gates input");
    let mut cursor = Some(stack(1, 1));
    assert_eq!(
        screens.escape(&mut cursor),
        None,
        "pause Escape sends nothing"
    );
    assert!(!screens.is_open(), "pause Escape lands back in the game");
    assert_eq!(cursor, None, "the close drops the cursor copy");
}

#[test]
fn the_stats_and_achievements_escape_back_to_the_pause() {
    // Pause ↔ statistics/achievements: Escape answers the parent, not the
    // game; with no parent stashed the base rule closes instead.
    for open in [
        Screens::open_stats as fn(&mut Screens, ScaledResolution, String),
        Screens::open_achievements as fn(&mut Screens, ScaledResolution, String),
    ] {
        let mut screens = Screens::default();
        screens.open_pause(wide(), pause_english());
        open(&mut screens, wide(), String::from("Done"));
        let mut cursor = None;
        assert_eq!(
            screens.escape(&mut cursor),
            None,
            "the placeholder Escape sends nothing"
        );
        assert!(
            screens.pause_mut().is_some(),
            "Escape answers the pause parent"
        );
        assert_eq!(
            screens.escape(&mut cursor),
            None,
            "a second Escape lands back in the game"
        );
        assert!(!screens.is_open());
    }
    // No parent stashed (a direct open): the base rule closes to the game.
    let mut screens = Screens::default();
    screens.open_stats(wide(), String::from("Done"));
    assert_eq!(screens.escape(&mut None), None);
    assert!(!screens.is_open());
}

#[test]
fn the_snooper_stands_with_its_declared_parent() {
    // R10: the snooper lands unreachable — its only opener is T18's hub —
    // so T17 pins the declared parent only; Escape closes to the game with
    // no send (the Done return wires with the hub).
    let mut screens = Screens::default();
    screens.open_snooper(wide(), String::from("Done"));
    assert!(screens.is_open(), "the snooper stands");
    assert_eq!(
        screens.current_window_id(),
        None,
        "the snooper stands on no window"
    );
    assert_eq!(
        screens.snooper_mut().expect("the snooper stands").parent(),
        SnooperParent::Options,
        "the declared parent is the options hub"
    );
    assert_eq!(
        screens.escape(&mut None),
        None,
        "snooper Escape sends nothing"
    );
    assert!(!screens.is_open());
}

#[test]
fn the_pause_clicks_pin_their_actions_and_the_lan_stays_inert() {
    // The press fires on press (`mouseClicked`); the disabled LAN never
    // presses, so it answers nothing; Disconnect pins the action id only —
    // its teardown is the standing disconnect path.
    let mut screens = Screens::default();
    screens.open_pause(wide(), pause_english());
    let pause = screens.pause_mut().expect("the pause stands");
    let press = |x: f32, y: f32| pause.click_at((x, y));
    assert_eq!(press(213.0, 78.0), Some(PauseAction::BackToGame));
    assert_eq!(press(162.0, 102.0), Some(PauseAction::Achievements));
    assert_eq!(press(264.0, 102.0), Some(PauseAction::Statistics));
    assert_eq!(press(162.0, 150.0), Some(PauseAction::Options));
    assert_eq!(press(264.0, 150.0), None, "the disabled LAN records inert");
    assert_eq!(press(213.0, 174.0), Some(PauseAction::Disconnect));
    assert_eq!(press(0.0, 0.0), None, "empty space answers nothing");
}

#[test]
fn the_placeholder_done_buttons_pin_their_ids() {
    // Stats Done id0 150x20 at `w/2+4, h-28`; achievements Done id1 80x20
    // at `w/2+24, h/2+74`; snooper Done id2 150x20 at `w/2+2, h-30` — all
    // at the wide size.
    assert_eq!(STATS_DONE_ID, 0);
    assert_eq!(ACHIEVEMENTS_DONE_ID, 1);
    assert_eq!(SNOOPER_DONE_ID, 2);
    let mut screens = Screens::default();
    screens.open_stats(wide(), String::from("Done"));
    assert_eq!(
        screens
            .stats_mut()
            .expect("the stats stand")
            .click_at((217.0 + 1.0, 212.0 + 1.0)),
        Some(StatsAction::Done)
    );
    let mut screens = Screens::default();
    screens.open_achievements(wide(), String::from("Done"));
    assert_eq!(
        screens
            .achievements_mut()
            .expect("the achievements stand")
            .click_at((237.0 + 1.0, 194.0 + 1.0)),
        Some(AchievementsAction::Done)
    );
    let mut screens = Screens::default();
    screens.open_snooper(wide(), String::from("Done"));
    assert_eq!(
        screens
            .snooper_mut()
            .expect("the snooper stands")
            .click_at((215.0 + 1.0, 210.0 + 1.0)),
        Some(SnooperAction::Done)
    );
}

#[test]
fn the_dirt_background_tiles_at_32_px() {
    // `GuiScreen.java:688-703`: the `options_background` tile spans 32 px
    // under the (64, 64, 64) tint.
    use oxide_client::screens::pause::{DIRT_SHEET, DIRT_TINT};
    use oxide_render::hud::{HudDraw, HudTexture};
    assert_eq!(DIRT_SPAN, 32.0);
    assert_eq!(DIRT_SHEET, "gui/options_background");
    assert_eq!(DIRT_TINT, [64.0 / 255.0, 64.0 / 255.0, 64.0 / 255.0, 1.0]);
    let draws = dirt_draws(&wide());
    assert_eq!(
        draws.len(),
        14 * 8,
        "427x240 tiles ceil(427/32) by ceil(240/32)"
    );
    for draw in &draws {
        match draw {
            HudDraw::TexturedRect {
                texture: HudTexture::Named(name),
                width,
                height,
                colour,
                ..
            } => {
                assert_eq!(*name, DIRT_SHEET);
                assert!(
                    *width <= DIRT_SPAN && *height <= DIRT_SPAN,
                    "no tile spans past 32 px"
                );
                assert_eq!(*colour, DIRT_TINT);
            }
            other => panic!("the dirt draws only tiles, got {other:?}"),
        }
    }
    let covered: f32 = draws
        .iter()
        .filter_map(|draw| match draw {
            HudDraw::TexturedRect { width, height, .. } => Some(width * height),
            _ => None,
        })
        .sum();
    assert_eq!(covered, 427.0 * 240.0, "the tiles cover the frame exactly");
}

#[test]
fn the_pause_draws_carry_the_title_over_six_buttons() {
    // `GuiIngameMenu.drawScreen:108-113`: "Game menu" centred at `(w/2,
    // 40)`, then the six buttons' draws.
    use oxide_client::screens::pause::PauseScreen;
    use oxide_render::hud::HudDraw;
    let font = pause_probe_font();
    let screen = PauseScreen::new(&wide(), pause_english());
    let draws = screen.draws(&font, &wide(), None);
    let title = draws
        .iter()
        .find_map(|draw| match draw {
            HudDraw::Text { text, x, y, .. } if text == "Game menu" => Some((*x, *y)),
            _ => None,
        })
        .expect("the centred title draws");
    assert_eq!(title.1, 40.0, "the title sits at y 40 like the source");
    assert_eq!(
        title.0 + oxide_render::text::string_width(&font, "Game menu") as f32 / 2.0,
        427.0 / 2.0,
        "the title centres at w/2"
    );
    let buttons = pause_buttons(&wide(), pause_english());
    for button in &buttons {
        assert!(
            draws.iter().any(|draw| match draw {
                HudDraw::Text { text, .. } => text == &button.label,
                _ => false,
            }),
            "the {} button's label draws",
            button.label
        );
    }
}

/// The pause probe's synthetic font sheet: 128x128 fully inked white, so
/// every printable cell spans its whole 8x8 and the title centres at a
/// known advance.
///
/// Generated here; no asset store is read and no Mojang pixel is embedded.
fn pause_probe_font() -> oxide_assets::font::Font {
    oxide_assets::font::Font::load(
        &oxide_assets::texture::Texture {
            width: 128,
            height: 128,
            rgba: vec![255u8; 128 * 128 * 4],
        },
        None,
    )
    .expect("the synthetic sheet loads")
}

// Task 18's options suite: the hub's source button grid, the video screen's
// sixteen rows in source order, the option-slider laws and the cycle-button
// widget, the toggle-now/slider-at-Done save split, and the routing into
// and out of the pause menu's Options button.
//
// Layout pins mirror `GuiOptions.java:44-107` (FOV slider row 0; the
// no-world REALMS_NOTIFICATIONS placement at `:81`; fixed 150x20 pairs at
// `w/2-155`/`w/2+5`, rows `h/6+48k-6`; Done 200 at `w/2-100, h/6+168`;
// title at y 15) and `GuiVideoSettings.java:16-51` (sixteen options in
// fixed order; 150x20 pairs at `w/2-155`/`+160`, slot 25, list top 32 /
// bottom h-32, width 400; Done 200 at `h-27`; the VBO-less break dropping
// USE_VBO and ENTITY_SHADOWS). Every formula here is closed-form — no GPU
// needed.

use oxide_client::screens::options::{
    OPTIONS_CHAT_ID, OPTIONS_CONTROLS_ID, OPTIONS_DONE_ID, OPTIONS_LANGUAGE_ID, OPTIONS_REALMS_ID,
    OPTIONS_RESOURCEPACK_ID, OPTIONS_SKIN_ID, OPTIONS_SNOOPER_ID, OPTIONS_SOUNDS_ID,
    OPTIONS_VIDEO_ID, OptionsAction, OptionsScreen, SaveKind, VIDEO_OPTIONS, VideoAction,
    VideoOption, VideoScreen, ambient_occlusion_label, clouds_label, cycle_ambient_occlusion,
    cycle_clouds, cycle_gui_scale, cycle_particles, denormalize_option, fov_label,
    framerate_denormalize, framerate_label, framerate_normalize, gamma_denormalize, gamma_label,
    gamma_normalize, graphics_label, gui_scale_label, mipmap_label, normalize_option,
    options_labels, particles_label, render_distance_label, snap_to_step, toggle_label,
    video_labels, video_row_options,
};
use oxide_client::screens::widgets::{OptionButton, OptionSlider};
use oxide_game::settings::{CloudSetting, GameSettings, ParticleSetting};

/// The hub over default settings with the store's English.
fn options_english() -> OptionsScreen {
    OptionsScreen::new(&wide(), &GameSettings::default(), options_labels(None))
}

/// The video screen over default settings with the store's English and VBOs
/// present (the renderer's standing shape).
fn video_english() -> VideoScreen {
    VideoScreen::new(&wide(), &GameSettings::default(), video_labels(None), true)
}

#[test]
fn the_option_slider_laws_snap_denormalize_and_normalize() {
    // `GameSettings.java:1380-1404`: snap = step·round(v/step) (step 0
    // passes through), denormalize = snap(min+(max-min)·clamp01(v)),
    // normalize = clamp01((snap(v)-min)/(max-min)).
    assert_eq!(snap_to_step(2.4, 1.0), 2.0);
    assert_eq!(snap_to_step(2.5, 1.0), 3.0);
    assert_eq!(snap_to_step(125.0, 10.0), 130.0);
    assert_eq!(snap_to_step(0.37, 0.0), 0.37);
    assert_eq!(denormalize_option(0.0, 2.0, 32.0, 1.0), 2.0);
    assert_eq!(denormalize_option(1.0, 2.0, 32.0, 1.0), 32.0);
    assert_eq!(denormalize_option(0.5, 2.0, 32.0, 1.0), 17.0);
    assert_eq!(denormalize_option(-0.5, 2.0, 32.0, 1.0), 2.0);
    assert_eq!(denormalize_option(1.5, 2.0, 32.0, 1.0), 32.0);
    assert_eq!(denormalize_option(0.44, 10.0, 260.0, 10.0), 120.0);
    assert_eq!(denormalize_option(0.37, 0.0, 1.0, 0.0), 0.37);
    assert_eq!(normalize_option(2.0, 2.0, 32.0, 1.0), 0.0);
    assert_eq!(normalize_option(32.0, 2.0, 32.0, 1.0), 1.0);
    assert_eq!(normalize_option(17.0, 2.0, 32.0, 1.0), 0.5);
    assert_eq!(normalize_option(120.0, 10.0, 260.0, 10.0), 0.44);
    assert_eq!(normalize_option(0.0, 0.0, 1.0, 0.0), 0.0);
}

#[test]
fn the_option_slider_press_and_drag_write_per_event() {
    // `GuiOptionSlider.java:44-100`: press folds the pointer and latches,
    // each drag motion writes again, release clears; the knob rides two
    // 4x20 halves (u 0 / u 196, v 66) at `x+v*(w-8)`.
    let mut slider = OptionSlider::new(
        10.0,
        10.0,
        150.0,
        20.0,
        2.0,
        32.0,
        1.0,
        12.0,
        String::from("Render Distance: 12 chunks"),
    );
    assert_eq!(slider.value(), 12.0);
    assert_eq!(slider.knob_x(), 10.0 + (10.0 / 30.0) * 142.0);
    let write = slider.press((85.0, 20.0)).expect("the press lands");
    assert_eq!(write, 17.0, "the press writes the denormalized value");
    assert!(slider.dragging(), "the press latches the drag");
    let write = slider.drag_to(156.0).expect("the latched drag writes");
    assert_eq!(write, 32.0, "each drag motion writes per event");
    slider.release();
    assert!(!slider.dragging(), "release clears the latch");
    assert_eq!(
        slider.drag_to(10.0),
        None,
        "an unlatched drag writes nothing"
    );
    assert_eq!(
        slider.press((500.0, 500.0)),
        None,
        "a press outside writes nothing"
    );
    let draws = slider.draws(&pause_probe_font(), None);
    let knob: Vec<(f32, f32)> = draws
        .iter()
        .filter_map(|draw| match draw {
            oxide_render::hud::HudDraw::TexturedRect { x, y, width, .. } if *width == 4.0 => {
                Some((*x, *y))
            }
            _ => None,
        })
        .collect();
    assert_eq!(knob.len(), 2, "the knob draws its two 4-wide halves");
    assert_eq!(knob[1].0 - knob[0].0, 4.0, "the halves sit side by side");
}

#[test]
fn the_cycle_button_composes_label_and_value() {
    // `GuiOptionButton.java` + `getKeyBinding`: the label carries the name
    // and the value (`"Graphics: Fancy"`); a press answers the id.
    let mut button = OptionButton::new(
        101,
        10.0,
        10.0,
        150.0,
        20.0,
        String::from("Graphics"),
        String::from("Fancy"),
    );
    assert_eq!(button.label(), "Graphics: Fancy");
    assert!(button.press((20.0, 15.0)), "a press on the rect answers");
    assert!(!button.press((500.0, 500.0)), "empty space answers nothing");
    button.set_value(String::from("Fast"));
    assert_eq!(button.label(), "Graphics: Fast");
    let draws = button.draws(&pause_probe_font(), None);
    assert!(
        draws.iter().any(|draw| matches!(draw,
            oxide_render::hud::HudDraw::Text { text, .. } if text == "Graphics: Fast")),
        "the composed label draws"
    );
}

#[test]
fn the_hub_pins_the_source_button_grid() {
    // `GuiOptions.java:44-107` at the wide size (427x240: half 213):
    // the FOV slider row 0 at `w/2-155, h/6-12`, the no-world realms
    // toggle beside it, the fixed 150x20 pairs, Done 200 at `w/2-100,
    // h/6+168`.
    assert_eq!(OPTIONS_VIDEO_ID, 101);
    assert_eq!(OPTIONS_DONE_ID, 200);
    assert_eq!(OPTIONS_CONTROLS_ID, 100);
    assert_eq!(OPTIONS_LANGUAGE_ID, 102);
    assert_eq!(OPTIONS_CHAT_ID, 103);
    assert_eq!(OPTIONS_SNOOPER_ID, 104);
    assert_eq!(OPTIONS_RESOURCEPACK_ID, 105);
    assert_eq!(OPTIONS_SOUNDS_ID, 106);
    assert_eq!(OPTIONS_SKIN_ID, 110);
    assert_eq!(
        OPTIONS_REALMS_ID, 44,
        "the realms toggle carries the REALMS_NOTIFICATIONS ordinal"
    );
    let screen = options_english();
    assert_eq!(
        (screen.fov_slider().x(), screen.fov_slider().y()),
        (58.0, 28.0),
        "the FOV slider stands row 0 left"
    );
    assert_eq!(
        (screen.realms_button().x(), screen.realms_button().y()),
        (218.0, 28.0),
        "the no-world realms toggle stands row 0 right"
    );
    let rects: Vec<(i32, f32, f32, f32, f32)> = screen
        .buttons()
        .iter()
        .map(|button| (button.id, button.x, button.y, button.w, button.h))
        .collect();
    assert_eq!(
        rects,
        vec![
            (OPTIONS_SKIN_ID, 58.0, 82.0, 150.0, 20.0),
            (OPTIONS_SOUNDS_ID, 58.0, 106.0, 150.0, 20.0),
            (OPTIONS_VIDEO_ID, 58.0, 130.0, 150.0, 20.0),
            (OPTIONS_LANGUAGE_ID, 58.0, 154.0, 150.0, 20.0),
            (OPTIONS_RESOURCEPACK_ID, 58.0, 178.0, 150.0, 20.0),
            (OPTIONS_DONE_ID, 113.0, 208.0, 200.0, 20.0),
            (OPTIONS_CONTROLS_ID, 218.0, 130.0, 150.0, 20.0),
            (OPTIONS_CHAT_ID, 218.0, 154.0, 150.0, 20.0),
            (OPTIONS_SNOOPER_ID, 218.0, 178.0, 150.0, 20.0),
        ],
        "the fixed pairs stand at the source's coordinates"
    );
    for button in screen.buttons() {
        assert!(
            !screen
                .buttons()
                .iter()
                .any(|other| other.id == 107 || other.id == 8675309),
            "button {} is neither stream nor super-secret",
            button.id
        );
    }
}

#[test]
fn the_video_screen_lists_sixteen_rows_in_source_order() {
    // `GuiVideoSettings.java:16`: GRAPHICS, RENDER_DISTANCE,
    // AMBIENT_OCCLUSION, FRAMERATE_LIMIT, ANAGLYPH, VIEW_BOBBING,
    // GUI_SCALE, GAMMA, RENDER_CLOUDS, PARTICLES, USE_FULLSCREEN,
    // ENABLE_VSYNC, MIPMAP_LEVELS, BLOCK_ALTERNATIVES, USE_VBO,
    // ENTITY_SHADOWS — eight rows of two 150x20 (`w/2-155`, `+160`),
    // slot 25, list top 32 / bottom h-32, width 400; Done at `h-27`.
    assert_eq!(
        VIDEO_OPTIONS,
        [
            VideoOption::Graphics,
            VideoOption::RenderDistance,
            VideoOption::AmbientOcclusion,
            VideoOption::FramerateLimit,
            VideoOption::Anaglyph,
            VideoOption::ViewBobbing,
            VideoOption::GuiScale,
            VideoOption::Gamma,
            VideoOption::RenderClouds,
            VideoOption::Particles,
            VideoOption::UseFullscreen,
            VideoOption::EnableVsync,
            VideoOption::MipmapLevels,
            VideoOption::BlockAlternatives,
            VideoOption::UseVbo,
            VideoOption::EntityShadows,
        ]
    );
    assert_eq!(
        video_row_options(true).len(),
        16,
        "VBOs present keeps every row"
    );
    let screen = video_english();
    assert_eq!(screen.row_count(), 8, "sixteen rows pair into eight");
    assert_eq!(screen.list_top(), 32.0);
    assert_eq!(screen.list_bottom(&wide()), 208.0, "the list ends at h-32");
    assert_eq!(screen.list_width(), 400.0);
    assert_eq!(screen.row_height(), 25.0);
    let (x, y, w, h) = screen.done_rect(&wide());
    assert_eq!(
        (x, y, w, h),
        (113.0, 213.0, 200.0, 20.0),
        "Done sits at h-27"
    );
    let (left, right) = screen.row_pair(0);
    assert_eq!((left.x(), left.y(), left.w()), (58.0, 32.0, 150.0));
    assert_eq!((right.x(), right.y(), right.w()), (218.0, 32.0, 150.0));
}

#[test]
fn the_video_truncation_drops_vbo_and_entity_shadows() {
    // `GuiVideoSettings.java:34-51`: without VBO support the copy loop
    // breaks BEFORE USE_VBO, so ENTITY_SHADOWS drops with it.
    let rows = video_row_options(false);
    assert_eq!(rows.len(), 14, "the break drops two rows' worth");
    assert_eq!(
        *rows.last().expect("fourteen rows stand"),
        VideoOption::BlockAlternatives
    );
    assert!(
        !rows.contains(&VideoOption::UseVbo),
        "USE_VBO drops at the break"
    );
    assert!(
        !rows.contains(&VideoOption::EntityShadows),
        "ENTITY_SHADOWS drops with it"
    );
}

#[test]
fn the_video_labels_read_min_mid_and_max() {
    // `getKeyBinding:623` with the store's `en_US.lang` English: "N
    // chunks", "N fps" / "Unlimited", Moody / "+N%" / Bright, "OFF" / N,
    // Auto…Large, OFF/Minimum/Maximum, OFF/Fast/Fancy, All/Decreased /
    // Minimal, Fancy/Fast, ON/OFF, Normal/N/Quake Pro.
    assert_eq!(
        render_distance_label("Render Distance", 2),
        String::from("Render Distance: 2 chunks")
    );
    assert_eq!(
        render_distance_label("Render Distance", 32),
        String::from("Render Distance: 32 chunks")
    );
    assert_eq!(
        framerate_label("Max Framerate", 10),
        String::from("Max Framerate: 10 fps")
    );
    assert_eq!(
        framerate_label("Max Framerate", 120),
        String::from("Max Framerate: 120 fps")
    );
    assert_eq!(
        framerate_label("Max Framerate", 255),
        String::from("Max Framerate: Unlimited")
    );
    assert_eq!(
        framerate_normalize(255),
        1.0,
        "the sentinel normalizes to the top"
    );
    assert_eq!(
        framerate_denormalize(1.0),
        255,
        "the top step folds into the sentinel"
    );
    assert_eq!(framerate_denormalize(0.44), 120);
    assert_eq!(
        gamma_label("Brightness", 0),
        String::from("Brightness: Moody")
    );
    assert_eq!(
        gamma_label("Brightness", 100),
        String::from("Brightness: Bright")
    );
    assert_eq!(
        gamma_label("Brightness", 37),
        String::from("Brightness: +37%")
    );
    assert!((gamma_normalize(37) - 0.37).abs() < 1e-6);
    assert_eq!(gamma_denormalize(0.37), 37);
    assert_eq!(
        mipmap_label("Mipmap Levels", 0),
        String::from("Mipmap Levels: OFF")
    );
    assert_eq!(
        mipmap_label("Mipmap Levels", 4),
        String::from("Mipmap Levels: 4")
    );
    assert_eq!(
        gui_scale_label("GUI Scale", 0),
        String::from("GUI Scale: Auto")
    );
    assert_eq!(
        gui_scale_label("GUI Scale", 1),
        String::from("GUI Scale: Small")
    );
    assert_eq!(
        gui_scale_label("GUI Scale", 2),
        String::from("GUI Scale: Normal")
    );
    assert_eq!(
        gui_scale_label("GUI Scale", 3),
        String::from("GUI Scale: Large")
    );
    assert_eq!(
        ambient_occlusion_label("Smooth Lighting", 0),
        String::from("Smooth Lighting: OFF")
    );
    assert_eq!(
        ambient_occlusion_label("Smooth Lighting", 1),
        String::from("Smooth Lighting: Minimum")
    );
    assert_eq!(
        ambient_occlusion_label("Smooth Lighting", 2),
        String::from("Smooth Lighting: Maximum")
    );
    assert_eq!(
        clouds_label("Clouds", CloudSetting::Off),
        String::from("Clouds: OFF")
    );
    assert_eq!(
        clouds_label("Clouds", CloudSetting::Fast),
        String::from("Clouds: Fast")
    );
    assert_eq!(
        clouds_label("Clouds", CloudSetting::Fancy),
        String::from("Clouds: Fancy")
    );
    assert_eq!(
        particles_label("Particles", ParticleSetting::All),
        String::from("Particles: All")
    );
    assert_eq!(
        particles_label("Particles", ParticleSetting::Decreased),
        String::from("Particles: Decreased")
    );
    assert_eq!(
        particles_label("Particles", ParticleSetting::Minimal),
        String::from("Particles: Minimal")
    );
    assert_eq!(
        graphics_label("Graphics", true),
        String::from("Graphics: Fancy")
    );
    assert_eq!(
        graphics_label("Graphics", false),
        String::from("Graphics: Fast")
    );
    assert_eq!(
        toggle_label("View Bobbing", true),
        String::from("View Bobbing: ON")
    );
    assert_eq!(
        toggle_label("View Bobbing", false),
        String::from("View Bobbing: OFF")
    );
    assert_eq!(fov_label("FOV", 70), String::from("FOV: Normal"));
    assert_eq!(fov_label("FOV", 110), String::from("FOV: Quake Pro"));
    assert_eq!(fov_label("FOV", 90), String::from("FOV: 90"));
}

#[test]
fn the_cycle_orders_wrap_the_source_way() {
    // `setOptionValue`: GUI_SCALE `+v & 3`, PARTICLES / RENDER_CLOUDS /
    // AMBIENT_OCCLUSION `+v % 3`.
    assert_eq!(cycle_gui_scale(3), 0);
    assert_eq!(cycle_gui_scale(0), 1);
    assert_eq!(cycle_clouds(CloudSetting::Fancy), CloudSetting::Off);
    assert_eq!(cycle_clouds(CloudSetting::Off), CloudSetting::Fast);
    assert_eq!(
        cycle_particles(ParticleSetting::Minimal),
        ParticleSetting::All
    );
    assert_eq!(cycle_ambient_occlusion(2), 0);
    assert_eq!(cycle_ambient_occlusion(0), 1);
}

#[test]
fn the_save_split_persists_toggles_now_and_sliders_at_done() {
    // `setOptionValue:522` ends in `saveOptions` — every toggle persists
    // at once — while `setOptionFloatValue:256-360` never saves, so
    // sliders persist at the next Done/screen-switch save.
    for action in [
        OptionsAction::RealmsToggled,
        OptionsAction::Video,
        OptionsAction::Snooper,
        OptionsAction::Done,
    ] {
        assert_eq!(
            action.save_kind(),
            SaveKind::Immediate,
            "{action:?} saves at once"
        );
    }
    assert_eq!(
        OptionsAction::FovChanged.save_kind(),
        SaveKind::Deferred,
        "the hub slider defers to Done"
    );
    for action in [
        VideoAction::GraphicsCycled,
        VideoAction::AmbientOcclusionCycled,
        VideoAction::AnaglyphToggled,
        VideoAction::ViewBobbingToggled,
        VideoAction::GuiScaleCycled,
        VideoAction::CloudsCycled,
        VideoAction::ParticlesCycled,
        VideoAction::FullscreenToggled,
        VideoAction::VsyncToggled,
        VideoAction::BlockAlternativesToggled,
        VideoAction::VboToggled,
        VideoAction::EntityShadowsToggled,
        VideoAction::Done,
    ] {
        assert_eq!(
            action.save_kind(),
            SaveKind::Immediate,
            "{action:?} saves at once"
        );
    }
    for action in [
        VideoAction::RenderDistanceChanged,
        VideoAction::FramerateChanged,
        VideoAction::GammaChanged,
        VideoAction::MipmapChanged,
    ] {
        assert_eq!(
            action.save_kind(),
            SaveKind::Deferred,
            "{action:?} defers to Done"
        );
    }
}

#[test]
fn the_gui_scale_change_asks_for_relayout() {
    // `GuiVideoSettings.java:85-116`: a GUI-scale change re-runs
    // `setWorldAndResolution` after click AND release — apply owns the
    // re-layout, so the action carries the notice.
    assert!(VideoAction::GuiScaleCycled.relayout());
    for action in [
        VideoAction::GraphicsCycled,
        VideoAction::RenderDistanceChanged,
        VideoAction::Done,
    ] {
        assert!(!action.relayout(), "{action:?} lays nothing out again");
    }
}

#[test]
fn the_video_press_writes_through_the_settings() {
    // Press on a toggle cycles the store; press on a slider writes the
    // denormalized value; empty space answers nothing.
    let mut settings = GameSettings::default();
    let mut screen = video_english();
    let (left, _) = screen.row_pair(0);
    let point = (left.x() + 10.0, left.y() + 10.0);
    assert_eq!(
        screen.click_at(point, &mut settings),
        Some(VideoAction::GraphicsCycled)
    );
    assert!(
        !settings.graphics.as_bool(),
        "the graphics press cycles fancy away"
    );
    let mut settings = GameSettings::default();
    let mut screen = video_english();
    let (_, right) = screen.row_pair(0);
    let point = (right.x() + 10.0, right.y() + 10.0);
    assert_eq!(
        screen.click_at(point, &mut settings),
        Some(VideoAction::RenderDistanceChanged)
    );
    assert!(
        (2..=32).contains(&settings.render_distance),
        "the slider press writes a ranged distance"
    );
    assert_eq!(screen.click_at((0.0, 0.0), &mut settings), None);
}

#[test]
fn the_options_routing_holds_pause_video_and_snooper() {
    // Pause → hub (the pause parent stashes); hub → video; video Done
    // answers the hub; hub → snooper (the declared slot wires); hub Done
    // answers the pause; Escape on hub/video closes with no send.
    let mut screens = Screens::default();
    screens.open_pause(wide(), pause_english());
    screens.open_options(wide(), &GameSettings::default(), options_labels(None));
    let mut cursor = None;
    assert_eq!(screens.escape(&mut cursor), None);
    assert!(!screens.is_open(), "hub Escape closes with no send");
    let mut screens = Screens::default();
    screens.open_pause(wide(), pause_english());
    screens.open_options(wide(), &GameSettings::default(), options_labels(None));
    screens.open_video_settings(wide(), &GameSettings::default(), video_labels(None), true);
    assert!(screens.video_mut().is_some(), "the video screen stands");
    assert!(screens.back_to_options(), "video Done answers the hub");
    assert!(screens.options_mut().is_some());
    assert!(screens.back_to_pause(), "hub Done answers the pause");
    assert!(screens.pause_mut().is_some());
    let mut screens = Screens::default();
    screens.open_pause(wide(), pause_english());
    screens.open_options(wide(), &GameSettings::default(), options_labels(None));
    screens.open_snooper_from_options(wide(), String::from("Done"));
    assert_eq!(screens.escape(&mut None), None);
    assert!(
        screens.options_mut().is_some(),
        "snooper Escape answers the hub now"
    );
}

// Task 19's screens suite: the sound screen's nine sliders (master
// full-width, the direct law, Off/% labels), the controls screen's three
// option rows plus the 33-row binding list with the rebind machine
// (arm/Escape-unbind/key/click-negative, red-only conflicts, gated
// Reset-All), the chat screen's ten rows in source order (five float rows
// step 0, the opacity/width/height label laws), the language list with the
// Force Unicode toggle, and the skin screen's seven part buttons (bit-flip
// + relabel). Layout pins mirror `GuiScreenOptionsSounds.java:36-50`
// (master 310 at `w/2-155, h/6-12`, pairs 150 at `w/2-155`/`+5`,
// `h/6-12+24*(i>>1)`, Done 200 at `h/6+168`, title y 15),
// `GuiControls.java:38-59` (invert/sensitivity/touchscreen at y 18/42/42,
// Done 200 + Reset-All 201 at `h-29`, title y 8),
// `ScreenChatOptions.java:24-43` (ten rows `w/2-155`/`+5`,
// `h/6+24*(i>>1)`, Done 200 at `h/6+120`, title y 20),
// `GuiLanguage.java:46-52` (unicode 100 + Done 6 at `h-38`, list 32..h-61,
// title y 16, warning y h-56) and `GuiCustomizeSkin.java:24-41` (seven
// 150x20 pairs `w/2-155`/`+5`, `h/6+24*(i>>1)`, Done 200 below, title
// y 20). Every formula here is closed-form — no GPU needed.

use oxide_client::screens::options_chat::{
    CHAT_DONE_ID, ChatAction, ChatScreen, chat_height_label, chat_labels, chat_opacity_label,
    chat_visibility_label, chat_width_label,
};
use oxide_client::screens::options_controls::{
    CONTROLS_DONE_ID, CONTROLS_RESET_ALL_ID, ControlsAction, ControlsScreen, controls_labels,
    sensitivity_label_value,
};
use oxide_client::screens::options_language::{
    LANGUAGE_DONE_ID, LANGUAGE_UNICODE_ID, LanguageAction, LanguageScreen, language_labels,
};
use oxide_client::screens::options_skin::{
    SKIN_DONE_ID, SKIN_PARTS, SkinAction, SkinScreen, skin_labels, skin_part_label,
};
use oxide_client::screens::options_sound::{
    SOUND_DONE_ID, SoundAction, SoundScreen, sound_label_value, sound_labels,
};
use winit::keyboard::KeyCode;

/// The sound screen over default settings with the store's English.
fn sound_english() -> SoundScreen {
    SoundScreen::new(&wide(), &GameSettings::default(), sound_labels(None))
}

/// The controls screen over default settings with the store's English.
fn controls_english() -> ControlsScreen {
    ControlsScreen::new(
        &wide(),
        &GameSettings::default(),
        controls_labels(None),
        None,
    )
}

/// The key-entry index of the named row (the list sorts by category and
/// description, never store order).
fn row_named(screen: &ControlsScreen, name: &str) -> usize {
    (0..screen.binding_row_count())
        .find(|index| screen.binding_row(*index).name() == name)
        .unwrap_or_else(|| panic!("the {name} row stands"))
}

/// The named binding's stored code.
fn stored_code(settings: &GameSettings, name: &str) -> i32 {
    settings
        .bindings
        .iter()
        .find(|binding| binding.name == name)
        .map(|binding| binding.code)
        .unwrap_or(0)
}

/// The chat screen over default settings with the store's English.
fn chat_english() -> ChatScreen {
    ChatScreen::new(&wide(), &GameSettings::default(), chat_labels(None))
}

/// The language screen over default settings with the store's English and
/// today's locale set.
fn language_english() -> LanguageScreen {
    LanguageScreen::new(
        &wide(),
        &GameSettings::default(),
        language_labels(None),
        &[String::from("en_US")],
    )
}

/// The skin screen over default settings with the store's English.
fn skin_english() -> SkinScreen {
    SkinScreen::new(&wide(), &GameSettings::default(), skin_labels(None))
}

#[test]
fn the_sound_screen_carries_nine_sliders_master_full_width() {
    // `GuiScreenOptionsSounds.java:38-48`: master is a 310-wide slider at
    // `w/2-155, h/6-12`; the other eight are 150-wide pairs at `w/2-155`
    // and `w/2+5`, rows `h/6-12+24*(i>>1)`; Done 200 at `w/2-100, h/6+168`.
    let screen = sound_english();
    assert_eq!(screen.slider_count(), 9, "all nine categories stand");
    let master = screen.slider(0);
    assert_eq!(
        (master.x(), master.y(), master.w()),
        (213.0 - 155.0, 40.0 - 12.0, 310.0)
    );
    let music = screen.slider(1);
    assert_eq!(
        (music.x(), music.y(), music.w()),
        (213.0 - 155.0, 40.0 - 12.0 + 24.0, 150.0)
    );
    let records = screen.slider(2);
    assert_eq!(
        (records.x(), records.y(), records.w()),
        (213.0 - 155.0 + 160.0, 40.0 - 12.0 + 24.0, 150.0)
    );
    let ambient = screen.slider(8);
    assert_eq!(
        (ambient.x(), ambient.y()),
        (213.0 - 155.0 + 160.0, 40.0 - 12.0 + 96.0)
    );
    assert_eq!(screen.done_pos(), (213.0 - 100.0, 40.0 + 168.0));
    assert_eq!(SOUND_DONE_ID, 200);
}

#[test]
fn the_sound_label_runs_off_or_percent() {
    // `GuiScreenOptionsSounds.java:78-82`: zero reads the Off string,
    // anything else `(int)(level*100)%`.
    assert_eq!(
        sound_label_value("Master Volume", 0.0, "Off"),
        "Master Volume: Off"
    );
    assert_eq!(
        sound_label_value("Master Volume", 1.0, "Off"),
        "Master Volume: 100%"
    );
    assert_eq!(sound_label_value("Music", 0.37, "Off"), "Music: 37%");
}

#[test]
fn the_sound_press_writes_the_direct_law() {
    // `GuiScreenOptionsSounds.java:124-135`: the press folds the pointer
    // `(mouseX-(x+4))/(w-8)`, clamps, writes the level and saves; the
    // drag motion repeats the same fold per event.
    let mut screen = sound_english();
    let mut settings = GameSettings::default();
    let action = screen
        .click_at(
            (213.0 - 155.0 + 4.0 + 151.0, 40.0 - 12.0 + 10.0),
            &mut settings,
        )
        .expect("the master press lands");
    assert_eq!(action, SoundAction::Level(0));
    assert_eq!(settings.category_volumes[0], 0.5);
    let action = screen
        .drag_to(213.0 - 155.0 + 4.0 + 302.0, &mut settings)
        .expect("the latched drag writes");
    assert_eq!(action, SoundAction::Level(0));
    assert_eq!(settings.category_volumes[0], 1.0);
    screen.release();
    assert_eq!(screen.drag_to(0.0, &mut settings), None);
}

#[test]
fn the_sound_done_saves() {
    let mut screen = sound_english();
    let mut settings = GameSettings::default();
    let action = screen
        .click_at((213.0 - 100.0 + 50.0, 40.0 + 168.0 + 10.0), &mut settings)
        .expect("Done lands");
    assert_eq!(action, SoundAction::Done);
    assert_eq!(action.save_kind(), SaveKind::Immediate);
}

#[test]
fn the_controls_screen_pins_the_option_rows_and_done_pair() {
    // `GuiControls.java:41-58`: Done 200 + Reset-All 201 at `h-29`, title
    // y 8; invert/touchscreen buttons and the sensitivity slider at
    // `w/2-155`/`+5`, rows y 18 and 42.
    let screen = controls_english();
    assert_eq!(
        (screen.invert_pos(), screen.sensitivity_pos()),
        ((213.0 - 155.0, 18.0), (213.0 - 155.0 + 160.0, 18.0))
    );
    assert_eq!(screen.touchscreen_pos(), (213.0 - 155.0, 42.0));
    assert_eq!(screen.done_pos(), (213.0 - 155.0, 240.0 - 29.0));
    assert_eq!(
        screen.reset_all_pos(),
        (213.0 - 155.0 + 160.0, 240.0 - 29.0)
    );
    assert_eq!((CONTROLS_DONE_ID, CONTROLS_RESET_ALL_ID), (200, 201));
    assert_eq!(screen.binding_row_count(), 33, "every binding stands");
}

#[test]
fn the_sensitivity_label_runs_min_or_max_or_percent() {
    // `GameSettings.java:623`: 0 reads the min string, 1 the max string,
    // anything else `(int)(sensitivity*200)%`.
    assert_eq!(
        sensitivity_label_value("Sensitivity", 0.0, "*yawn*", "HYPERSPEED!!!"),
        "Sensitivity: *yawn*"
    );
    assert_eq!(
        sensitivity_label_value("Sensitivity", 1.0, "*yawn*", "HYPERSPEED!!!"),
        "Sensitivity: HYPERSPEED!!!"
    );
    assert_eq!(
        sensitivity_label_value("Sensitivity", 0.5, "*yawn*", "HYPERSPEED!!!"),
        "Sensitivity: 100%"
    );
}

#[test]
fn the_controls_rebind_machine_arms_and_binds() {
    // `GuiControls.java:98-152`: clicking a change button arms it; a key
    // press binds (Escape 1 unbinds to 0); a click while armed binds the
    // negative mouse button (`-100+button`); conflicts stay red-only.
    let mut screen = controls_english();
    let mut settings = GameSettings::default();
    let row = row_named(&screen, "left");
    let change = screen.binding_row(row).change_x();
    let y = screen.binding_row(row).y();
    let action = screen
        .click_at((change + 10.0, y + 10.0), &mut settings)
        .expect("the change press arms");
    assert_eq!(action, ControlsAction::Armed(row));
    assert_eq!(screen.armed(), Some(row));
    let action = screen
        .key(KeyCode::KeyF, &mut settings)
        .expect("the key press binds");
    assert_eq!(action, ControlsAction::Rebound(row));
    assert_eq!(screen.armed(), None);
    assert_eq!(stored_code(&settings, "left"), 33);
    assert_eq!(screen.binding_row(row).change_label(), "F");
}

#[test]
fn the_controls_escape_press_unbinds() {
    let mut screen = controls_english();
    let mut settings = GameSettings::default();
    let row = row_named(&screen, "forward");
    let change = screen.binding_row(row).change_x();
    let y = screen.binding_row(row).y();
    screen
        .click_at((change + 10.0, y + 10.0), &mut settings)
        .expect("the change press arms");
    let action = screen
        .key(KeyCode::Escape, &mut settings)
        .expect("Escape unbinds");
    assert_eq!(action, ControlsAction::Unbound(row));
    assert_eq!(stored_code(&settings, "forward"), 0);
}

#[test]
fn the_controls_click_while_armed_binds_negative() {
    let mut screen = controls_english();
    let mut settings = GameSettings::default();
    let row = row_named(&screen, "back");
    let change = screen.binding_row(row).change_x();
    let y = screen.binding_row(row).y();
    screen
        .click_at((change + 10.0, y + 10.0), &mut settings)
        .expect("the change press arms");
    let action = screen
        .click_button(1, (10.0, 100.0), &mut settings)
        .expect("the click binds");
    assert_eq!(action, ControlsAction::Rebound(row));
    assert_eq!(stored_code(&settings, "back"), -99);
}

#[test]
fn the_controls_conflict_is_red_only_and_reset_all_is_gated() {
    // `GuiKeyBindingList.java:131-141`: a shared code paints the change
    // button red and nothing else; `GuiControls.java:162-173`: Reset-All
    // enables only while a row differs from its default.
    let screen = controls_english();
    assert!(!screen.reset_enabled(), "defaults leave Reset-All dark");
    let mut settings = GameSettings::default();
    settings
        .bindings
        .iter_mut()
        .find(|binding| binding.name == "left")
        .expect("the left row stands")
        .code = stored_code(&settings, "forward");
    let screen = ControlsScreen::new(&wide(), &settings, controls_labels(None), None);
    assert!(screen.reset_enabled(), "a changed row lights Reset-All");
    let left = row_named(&screen, "left");
    let forward = row_named(&screen, "forward");
    assert!(
        screen.binding_row(left).conflicted(),
        "the shared code paints red"
    );
    assert!(
        screen.binding_row(forward).conflicted(),
        "every holder paints red"
    );
    let mut settings = GameSettings::default();
    settings
        .bindings
        .iter_mut()
        .find(|binding| binding.name == "left")
        .expect("the left row stands")
        .code = stored_code(&settings, "forward");
    let mut screen = ControlsScreen::new(&wide(), &settings, controls_labels(None), None);
    let action = screen
        .click_at(
            (213.0 - 155.0 + 160.0 + 10.0, 240.0 - 29.0 + 10.0),
            &mut settings,
        )
        .expect("Reset-All lands");
    assert_eq!(action, ControlsAction::ResetAll);
    assert_eq!(stored_code(&settings, "left"), 30);
}

#[test]
fn the_chat_screen_pins_ten_rows_in_source_order() {
    // `ScreenChatOptions.java:9-43`: the ten options in fixed order, 150
    // pairs at `w/2-155`/`+5`, rows `h/6+24*(i>>1)`; Done 200 at `h/6+120`;
    // the opacity/scale/height/width rows are sliders (step 0 in the
    // port's float law).
    let screen = chat_english();
    assert_eq!(screen.row_count(), 10);
    assert_eq!(screen.row_id(0), 15, "visibility leads");
    assert_eq!(screen.row_id(9), 42, "reduced debug closes");
    assert_eq!(screen.row_pos(0), (213.0 - 155.0, 40.0));
    assert_eq!(screen.row_pos(1), (213.0 - 155.0 + 160.0, 40.0));
    assert_eq!(screen.row_pos(2), (213.0 - 155.0, 40.0 + 24.0));
    assert!(screen.row_is_slider(3), "opacity slides");
    assert!(screen.row_is_slider(5), "scale slides");
    assert!(screen.row_is_slider(6), "focused height slides");
    assert!(screen.row_is_slider(7), "unfocused height slides");
    assert!(screen.row_is_slider(8), "width slides");
    assert!(!screen.row_is_slider(0), "visibility toggles");
    assert_eq!(screen.done_pos(), (213.0 - 100.0, 40.0 + 120.0));
    assert_eq!(CHAT_DONE_ID, 200);
}

#[test]
fn the_chat_label_laws_read_source_values() {
    // `GameSettings.java:623` + `GuiNewChat.java:361-371`: opacity is
    // `(int)(f*90+10)%`, widths/heights are `floor(v*280+40)` /
    // `floor(v*160+20)` px.
    assert_eq!(
        chat_visibility_label(0, "Chat", "Shown", "Commands Only", "Hidden"),
        "Chat: Shown"
    );
    assert_eq!(
        chat_visibility_label(1, "Chat", "Shown", "Commands Only", "Hidden"),
        "Chat: Commands Only"
    );
    assert_eq!(
        chat_visibility_label(2, "Chat", "Shown", "Commands Only", "Hidden"),
        "Chat: Hidden"
    );
    assert_eq!(chat_opacity_label(1.0, "Opacity"), "Opacity: 100%");
    assert_eq!(chat_opacity_label(0.0, "Opacity"), "Opacity: 10%");
    assert_eq!(chat_width_label(1.0, "Width"), "Width: 320px");
    assert_eq!(chat_width_label(0.0, "Width"), "Width: 40px");
    assert_eq!(chat_height_label(1.0, "Height"), "Height: 180px");
    assert_eq!(chat_height_label(0.0, "Height"), "Height: 20px");
}

#[test]
fn the_chat_float_press_writes_and_rewraps() {
    // `GameSettings.java:278-308`: each float write stores the value and
    // re-wraps the log; the press folds the pointer per event.
    let mut screen = chat_english();
    let mut settings = GameSettings::default();
    let pos = screen.row_pos(8);
    let action = screen
        .click_at((pos.0 + 4.0 + 71.0, pos.1 + 10.0), &mut settings)
        .expect("the width press lands");
    assert_eq!(action, ChatAction::Width);
    assert_eq!(settings.chat_width, 0.5);
}

#[test]
fn the_language_screen_pins_list_and_unicode_toggle() {
    // `GuiLanguage.java:46-52,104-106`: the list spans 32..h-61, Force
    // Unicode 100 + Done 6 sit at `h-38`, the title at y 16 and the
    // warning at y h-56.
    let screen = language_english();
    assert_eq!(screen.row_count(), 1, "today's set holds en_US");
    assert_eq!(screen.unicode_pos(), (213.0 - 155.0, 240.0 - 38.0));
    assert_eq!(screen.done_pos(), (213.0 - 155.0 + 160.0, 240.0 - 38.0));
    assert_eq!((LANGUAGE_UNICODE_ID, LANGUAGE_DONE_ID), (100, 6));
    assert_eq!(screen.selected(), Some(0));
}

#[test]
fn the_language_toggle_persists_and_selects() {
    // `GuiLanguage.java:79-90,130-141`: the toggle writes Force Unicode
    // and re-lays out; a row click writes the language and saves.
    let mut screen = language_english();
    let mut settings = GameSettings::default();
    let action = screen
        .click_at((213.0 - 155.0 + 10.0, 240.0 - 38.0 + 10.0), &mut settings)
        .expect("the unicode press lands");
    assert_eq!(action, LanguageAction::ForceUnicode);
    assert!(settings.force_unicode);
    assert_eq!(action.save_kind(), SaveKind::Immediate);
    assert!(action.relayout(), "the toggle re-lays out");
}

#[test]
fn the_skin_screen_pins_seven_part_buttons() {
    // `GuiCustomizeSkin.java:24-41`: the seven parts in enum order, 150x20
    // pairs at `w/2-155`/`+5`, rows `h/6+24*(i>>1)`; Done 200 rides two
    // slots below the odd seventh.
    let screen = skin_english();
    assert_eq!(screen.part_count(), 7);
    assert_eq!(
        SKIN_PARTS,
        [
            "cape",
            "jacket",
            "left_sleeve",
            "right_sleeve",
            "left_pants_leg",
            "right_pants_leg",
            "hat"
        ]
    );
    assert_eq!(screen.part_pos(0), (213.0 - 155.0, 40.0));
    assert_eq!(screen.part_pos(1), (213.0 - 155.0 + 160.0, 40.0));
    assert_eq!(screen.part_pos(6), (213.0 - 155.0, 40.0 + 72.0));
    assert_eq!(screen.done_pos(), (213.0 - 100.0, 40.0 + 96.0));
    assert_eq!(SKIN_DONE_ID, 200);
    assert_eq!(skin_part_label("Cape", true, "On", "Off"), "Cape: On");
    assert_eq!(skin_part_label("Hat", false, "On", "Off"), "Hat: Off");
}

#[test]
fn the_skin_press_flips_the_bit_and_relabels() {
    // `GuiCustomizeSkin.java:55-60`: the press flips the part bit, saves,
    // and rewrites the button's On/Off label.
    let mut screen = skin_english();
    let mut settings = GameSettings::default();
    assert_eq!(settings.skin_parts, 0x7F);
    let action = screen
        .click_at((213.0 - 155.0 + 10.0, 40.0 + 10.0), &mut settings)
        .expect("the cape press lands");
    assert_eq!(action, SkinAction::Part(0));
    assert_eq!(settings.skin_parts, 0x7E);
    assert_eq!(screen.part_label(0), "Cape: Off");
    assert_eq!(action.save_kind(), SaveKind::Immediate);
}

// Task 20's end-state suite: the death, disconnect, terrain and sleep
// screens plus the link-confirm promotion.
//
// Closed-form pins mirror the source windows first-hand (`GuiGameOver.java`,
// `GuiDisconnected.java`, `GuiDownloadTerrain.java`, `GuiSleepMP.java`,
// `GuiConfirmOpenLink.java` over `GuiYesNo.java`): every rect, id, colour
// and tick count below names its source line in the assertion message.

use oxide_client::screens::end_states::{
    BED_BLOCK_ID, CONFIRM_TITLE_Y, CONFIRM_WARNING_COLOUR, CONFIRM_WARNING_Y, DEATH_ENABLE_TICKS,
    DEATH_GRADIENT_BOTTOM, DEATH_GRADIENT_TOP, DEATH_RESPAWN_ID, DEATH_SCORE_Y, DEATH_TITLE_ID,
    DEATH_TITLE_SCALE, DEATH_TITLE_Y, DISCONNECT_BUTTON_ID, DISCONNECT_TITLE_COLOUR,
    DISCONNECT_WRAP_MARGIN, DisconnectAction, TERRAIN_LINE_Y_OFFSET, bed_facing_j, confirm_labels,
    death_labels, death_score_line, disconnect_button_y, leave_bed_button, sleep_dim, sleep_yaw,
};
use oxide_client::screens::{ConfirmAction, DeathAction};

/// A scaled size for the end-state pins.
fn end_scaled() -> ScaledResolution {
    ScaledResolution {
        width: 427,
        height: 240,
        scale_factor: 2,
    }
}

#[test]
fn the_death_buttons_stand_on_the_source_rects() {
    // `GuiGameOver.initGui:38-39`: Respawn id 0 at `w/2-100, h/4+72`,
    // Title id 1 at `h/4+96`, both 200x20. At 427x240: (113, 132) and
    // (113, 156).
    let mut screens = Screens::default();
    screens.open_death(&end_scaled(), &death_labels(None), 0);
    let screen = screens.death_mut().expect("death stands");
    assert_eq!(DEATH_RESPAWN_ID, 0, "the respawn id");
    assert_eq!(DEATH_TITLE_ID, 1, "the title id");
    let buttons = screen.buttons();
    assert_eq!(buttons.len(), 2, "two buttons stand");
    assert_eq!((buttons[0].x, buttons[0].y), (113.0, 132.0), "respawn rect");
    assert_eq!((buttons[1].x, buttons[1].y), (113.0, 156.0), "title rect");
    assert_eq!((buttons[0].w, buttons[0].h), (200.0, 20.0), "button size");
}

#[test]
fn the_death_buttons_hold_the_integer_row_off_the_quarter() {
    // `GuiGameOver.initGui:38-39` divides integers: at a height past the
    // quarter the pair sits on the truncated row, never the float one.
    let scaled = ScaledResolution {
        width: 427,
        height: 241,
        scale_factor: 2,
    };
    let mut screens = Screens::default();
    screens.open_death(&scaled, &death_labels(None), 0);
    let screen = screens.death_mut().expect("death stands");
    let buttons = screen.buttons();
    assert_eq!((buttons[0].x, buttons[0].y), (113.0, 132.0), "respawn row");
    assert_eq!((buttons[1].x, buttons[1].y), (113.0, 156.0), "title row");
}

#[test]
fn the_death_buttons_enable_on_the_twentieth_tick() {
    // `initGui:47-50` builds every button disabled; `updateScreen:135-147`
    // enables them all when the counter reaches exactly 20.
    assert_eq!(DEATH_ENABLE_TICKS, 20, "the enable tick");
    let mut screens = Screens::default();
    screens.open_death(&end_scaled(), &death_labels(None), 0);
    let screen = screens.death_mut().expect("death stands");
    assert!(
        screen.buttons().iter().all(|button| !button.enabled),
        "every button starts disabled"
    );
    assert!(
        screen.click_at((114.0, 133.0)).is_none(),
        "a press before the twentieth tick answers nothing"
    );
    for _ in 0..19 {
        screens.tick_death();
    }
    assert!(
        screens
            .death_mut()
            .expect("death stands")
            .buttons()
            .iter()
            .all(|button| !button.enabled),
        "nineteen ticks still hold the gate"
    );
    screens.tick_death();
    assert!(
        screens
            .death_mut()
            .expect("death stands")
            .buttons()
            .iter()
            .all(|button| button.enabled),
        "the twentieth tick enables both"
    );
    assert_eq!(
        screens
            .death_mut()
            .expect("death stands")
            .click_at((114.0, 133.0)),
        Some(DeathAction::Respawn),
        "respawn answers after the gate"
    );
}

#[test]
fn the_death_title_runs_the_quit_confirm_with_its_own_delay() {
    // `actionPerformed:80-83` stands the quit-confirm with a 20-tick button
    // delay; true quits to the menu, false respawns (`confirmClicked:87-100`).
    let mut screens = Screens::default();
    screens.open_death(&end_scaled(), &death_labels(None), 0);
    for _ in 0..DEATH_ENABLE_TICKS {
        screens.tick_death();
    }
    assert_eq!(
        screens
            .death_mut()
            .expect("death stands")
            .click_at((114.0, 157.0)),
        Some(DeathAction::QuitToTitle),
        "the title button stands the quit-confirm"
    );
    let screen = screens.death_mut().expect("death stands");
    assert!(screen.confirming(), "the confirm phase stands");
    assert!(
        screen.click_at((114.0, 157.0)).is_none(),
        "the confirm's own delay holds first"
    );
    for _ in 0..DEATH_ENABLE_TICKS {
        screens.tick_death();
    }
    assert_eq!(
        screens
            .death_mut()
            .expect("death stands")
            .click_at((114.0, 157.0)),
        Some(DeathAction::ConfirmRespawn),
        "the confirm's cancel respawns"
    );
}

#[test]
fn the_death_draw_pins_the_gradient_title_and_score() {
    // `drawScreen:107` gradients `1615855616` over `-1602211792`; the title
    // draws at 2x (`:108-113`, `deathScreen.title` "You died!"); the score
    // line is `deathScreen.score` + ": " + yellow + value at y 100 (`:120`).
    assert_eq!(
        DEATH_GRADIENT_TOP,
        [80.0 / 255.0, 0.0, 0.0, 96.0 / 255.0],
        "the top stop is 1615855616"
    );
    assert_eq!(
        DEATH_GRADIENT_BOTTOM,
        [128.0 / 255.0, 48.0 / 255.0, 48.0 / 255.0, 160.0 / 255.0],
        "the bottom stop is -1602211792"
    );
    assert_eq!(DEATH_TITLE_SCALE, 2.0, "the title scale");
    assert_eq!(DEATH_TITLE_Y, 60.0, "the title's GUI y (30 in 2x space)");
    assert_eq!(DEATH_SCORE_Y, 100.0, "the score line's y");
    let labels = death_labels(None);
    assert_eq!(labels.title, "You died!", "the title label");
    assert_eq!(
        death_score_line(&labels.score, 7),
        "Score: \u{a7}e7",
        "the score line format"
    );
}

#[test]
fn the_disconnect_screen_carries_the_lost_title_and_the_reason() {
    // `GuiDisconnected`: the fixed `disconnect.lost` title plus the kick
    // body wrapped at `width - 50`; one `gui.toMenu` button below the body
    // at `h/2 + msgH/2 + FONT_HEIGHT`.
    let labels = death_labels(None);
    let _ = labels;
    let mut screens = Screens::default();
    screens.open_disconnected(
        &end_scaled(),
        String::from("Connection Lost"),
        String::from("{\"text\":\"Kicked\"}"),
        &pause_probe_font(),
        String::from("Back to server list"),
    );
    let screen = screens.disconnected_mut().expect("disconnect stands");
    assert_eq!(screen.title(), "Connection Lost", "the fixed title");
    assert_eq!(DISCONNECT_BUTTON_ID, 0, "the button id");
    assert_eq!(DISCONNECT_WRAP_MARGIN, 50, "the wrap budget");
    assert_eq!(
        DISCONNECT_TITLE_COLOUR,
        [170.0 / 255.0, 170.0 / 255.0, 170.0 / 255.0, 1.0],
        "the title grey is 11184810"
    );
    assert_eq!(
        disconnect_button_y(240, 1, 9),
        120.0 + 4.5 + 9.0,
        "the button sits below one body line"
    );
    assert_eq!(
        screen.click_at((214.0, disconnect_button_y(240, 1, 9) + 1.0)),
        Some(DisconnectAction::ToParent),
        "the button answers the parent"
    );
}

#[test]
fn the_terrain_screen_pins_the_centred_line_and_no_buttons() {
    // `GuiDownloadTerrain.drawScreen:53-54`: dirt plus
    // `multiplayer.downloadingTerrain` centred at `h/2 - 50`; `initGui:30-33`
    // stands no buttons.
    assert_eq!(TERRAIN_LINE_Y_OFFSET, 50.0, "the line's offset");
    let mut screens = Screens::default();
    screens.open_terrain(&end_scaled(), String::from("Downloading terrain"));
    assert!(screens.is_open(), "terrain stands");
}

#[test]
fn the_confirm_buttons_stand_three_across() {
    // `GuiConfirmOpenLink.initGui:34-36`: confirm id 0, copy id 2, cancel
    // id 1 — three 100x20 buttons at `h/6+96`. At 427x240: y 136, x 58 /
    // 163 / 268.
    let labels = confirm_labels(None);
    assert_eq!(labels.confirm_untrusted, "Yes", "untrusted confirm");
    assert_eq!(labels.cancel_untrusted, "No", "untrusted cancel");
    assert_eq!(labels.confirm_trusted, "Open in browser", "trusted confirm");
    assert_eq!(labels.cancel_trusted, "Cancel", "trusted cancel");
    let mut screens = Screens::default();
    screens.open_confirm(
        &end_scaled(),
        String::from("https://a.example"),
        &pause_probe_font(),
        &confirm_labels(None),
        false,
    );
    let screen = screens.confirm_mut().expect("confirm stands");
    let buttons = screen.buttons();
    assert_eq!(buttons.len(), 3, "three buttons stand");
    assert_eq!(
        (buttons[0].id, buttons[0].x, buttons[0].y),
        (0, 58.0, 136.0),
        "confirm"
    );
    assert_eq!(
        (buttons[1].id, buttons[1].x, buttons[1].y),
        (2, 163.0, 136.0),
        "copy"
    );
    assert_eq!(
        (buttons[2].id, buttons[2].x, buttons[2].y),
        (1, 268.0, 136.0),
        "cancel"
    );
    assert_eq!(
        screen.click_at((164.0, 137.0)),
        Some(ConfirmAction::Copy),
        "the middle button copies"
    );
    assert_eq!(
        CONFIRM_WARNING_COLOUR,
        [255.0 / 255.0, 204.0 / 255.0, 204.0 / 255.0, 1.0],
        "the warning colour is 16764108"
    );
    assert_eq!(CONFIRM_TITLE_Y, 70.0, "the title's y");
    assert_eq!(CONFIRM_WARNING_Y, 110.0, "the warning's y");
}

#[test]
fn the_end_state_transitions_follow_the_session() {
    // Died stands death; the re-raise replaces whatever closed it; the
    // respawn clears it. Join stands terrain; the terrain-clear event drops
    // it. Disconnect stands the screen with its body. A link stands the
    // confirm.
    let mut screens = Screens::default();
    screens.open_death(&end_scaled(), &death_labels(None), 0);
    screens.close_death();
    assert!(!screens.is_open(), "the test cleared death");
    screens.open_death(&end_scaled(), &death_labels(None), 0);
    screens.ensure_death(&end_scaled(), &death_labels(None));
    assert!(screens.death_mut().is_some(), "death stands");
    screens.open_terrain(&end_scaled(), String::from("Downloading terrain"));
    assert!(screens.death_mut().is_none(), "terrain replaces death");
    screens.ensure_death(&end_scaled(), &death_labels(None));
    assert!(
        screens.death_mut().is_some(),
        "the re-raise stands death again"
    );
    assert!(screens.close_death(), "the respawn clears death");
    assert!(!screens.is_open(), "nothing stands after the clear");
    screens.open_terrain(&end_scaled(), String::from("Downloading terrain"));
    assert!(
        screens.close_terrain(),
        "the terrain-clear event drops terrain"
    );
    assert!(!screens.close_terrain(), "a second clear reports nothing");
    screens.open_disconnected(
        &end_scaled(),
        String::from("Connection Lost"),
        String::from("{\"text\":\"End of stream\"}"),
        &pause_probe_font(),
        String::from("Back to server list"),
    );
    assert!(screens.disconnected_mut().is_some(), "disconnect stands");
    screens.open_confirm(
        &end_scaled(),
        String::from("https://a.example"),
        &pause_probe_font(),
        &confirm_labels(None),
        false,
    );
    assert!(
        screens.confirm_mut().is_some(),
        "the link stands the confirm"
    );
    assert!(screens.cancel_confirm(), "escape cancels the confirm");
    assert!(!screens.is_open(), "nothing stands after the cancel");
}

#[test]
fn the_end_state_screens_never_close_through_the_window_path() {
    // Death, disconnect and terrain have empty `keyTyped` arms — Escape is
    // a no-op — and stand on no window, so the generic close leaves them.
    let mut screens = Screens::default();
    screens.open_death(&end_scaled(), &death_labels(None), 0);
    let mut dropped = None;
    assert!(
        screens.escape(&mut dropped).is_none(),
        "death sends nothing"
    );
    assert!(screens.death_mut().is_some(), "death stays");
    assert!(
        screens.close(&mut dropped).is_none(),
        "death closes nothing"
    );
    assert!(screens.death_mut().is_some(), "death still stays");
    screens.open_disconnected(
        &end_scaled(),
        String::from("Connection Lost"),
        String::from("{\"text\":\"x\"}"),
        &pause_probe_font(),
        String::from("Back to server list"),
    );
    assert!(
        screens.escape(&mut dropped).is_none(),
        "disconnect sends nothing"
    );
    assert!(screens.disconnected_mut().is_some(), "disconnect stays");
    screens.open_terrain(&end_scaled(), String::from("Downloading terrain"));
    assert!(
        screens.escape(&mut dropped).is_none(),
        "terrain sends nothing"
    );
    assert!(screens.is_open(), "terrain stays");
}

#[test]
fn the_sleep_cover_pins_the_leave_bed_button_darkening_and_rise() {
    // `GuiSleepMP.initGui:14-18`: "Leave Bed" id 1, 200x20 at `w/2-100,
    // h-40`. `GuiIngame:194-212`: `sleepTimer/100` ramp with the
    // `220*f << 24 | 1052704` rect and the 100-110 fade. `orientCamera`'s
    // sleeping branch: eye +1.0, 0.3 up, bed-facing yaw.
    let button = leave_bed_button(&end_scaled(), String::from("Leave Bed"));
    assert_eq!((button.x, button.y), (113.0, 200.0), "the leave-bed rect");
    assert_eq!((button.w, button.h), (200.0, 20.0), "the leave-bed size");
    assert_eq!(
        sleep_dim(0),
        [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 0.0],
        "no timer draws nothing"
    );
    assert_eq!(
        sleep_dim(50),
        [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 110.0 / 255.0],
        "half the timer is half the alpha"
    );
    assert_eq!(
        sleep_dim(100),
        [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 220.0 / 255.0],
        "the full timer is the full 220 alpha"
    );
    assert_eq!(
        sleep_dim(105),
        sleep_dim(50),
        "the wake fade mirrors the ramp"
    );
    assert_eq!(
        sleep_dim(110),
        [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 0.0],
        "the fade lands back at nothing"
    );
    assert_eq!(sleep_yaw(30.0, 0), 210.0, "south beds turn the yaw by 180");
    assert_eq!(sleep_yaw(30.0, 1), 120.0, "west beds fold their quarter");
    assert_eq!(BED_BLOCK_ID, 26, "the bed block id");
    assert_eq!(bed_facing_j(0), 0, "south reads 0");
    assert_eq!(bed_facing_j(9), 1, "head + occupied keep the facing");
}
