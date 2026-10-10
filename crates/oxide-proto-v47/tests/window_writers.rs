//! The serverbound window and sign writer corpus: one hand-built byte vector
//! per action the containers, the hotbar, the sign editor and the creative
//! screen send, plus the decode-to-write echo pairs and the round trips back
//! through the crate's own decoders.
//!
//! Every byte is packed by hand from the packet classes' own writers
//! (`C09PacketHeldItemChange.writePacketData:32-35`,
//! `C0DPacketCloseWindow.writePacketData:40-43`,
//! `C0EPacketClickWindow.writePacketData:67-75`,
//! `C0FPacketConfirmTransaction.writePacketData:46-51`,
//! `C10PacketCreativeInventoryAction.writePacketData:44-48`,
//! `C11PacketEnchantItem.writePacketData:43-47`,
//! `C12PacketUpdateSign.writePacketData:44-53`) and from `PacketBuffer`'s own
//! slot and string framing (`writeItemStackToBuffer:232-250`,
//! `writeString:304-317`; the protocol reference's §2.2 rows for
//! 0x09 and 0x0D–0x12), never rebuilt with the writer's arithmetic, so a
//! wrong field order or a dropped NBT tail cannot be confirmed by its own
//! twin. The two slot-carrying writers are additionally driven decode-to-
//! write: the fixture's fields and its tailed slot decode through the crate's
//! readers — [`read_slot`] is the slot reader the whole play state shares —
//! and the writer's output must equal the original bytes exactly.

use std::io::{Cursor, ErrorKind};

use oxide_proto::codec;
use oxide_proto_v47::entity::{MetadataItem, read_slot};
use oxide_proto_v47::serverbound::{
    write_click_window, write_close_window, write_confirm_transaction,
    write_creative_inventory_action, write_enchant_item, write_held_item_change, write_update_sign,
};
use oxide_proto_v47::window::{CloseWindow, ConfirmTransaction, UpdateSign};

/// Held Item Change (0x09) to slot 3: the packet id, then the slot as a
/// big-endian short (`C09PacketHeldItemChange.writePacketData:34`).
const HELD_ITEM_CHANGE_SLOT_3: &[u8] = &[
    0x09, // the packet id
    0x00, 0x03, // slot 3
];

/// Held Item Change (0x09) to slot 8, the hotbar's last
/// (`NetHandlerPlayServer.processHeldItemChange:769` tests `0..9`).
const HELD_ITEM_CHANGE_SLOT_8: &[u8] = &[
    0x09, // the packet id
    0x00, 0x08, // slot 8
];

/// Click Window (0x0E) with nothing in the slot: window 0, slot 36, button 0,
/// action 5, mode 0 and the empty clicked item — the short `-1`
/// (`PacketBuffer.writeItemStackToBuffer:232-237`). The field order is the
/// writer's own (`C0EPacketClickWindow.writePacketData:69-74`: window id,
/// slot, button, action number, mode, then the clicked item).
const CLICK_WINDOW_EMPTY: &[u8] = &[
    0x0e, // the packet id
    0x00, // window id 0: the player's own inventory
    0x00, 0x24, // slot 36
    0x00, // button 0: the left mouse button
    0x00, 0x05, // action number 5
    0x00, // mode 0: a normal click
    0xff, 0xff, // the empty clicked item
];

/// Click Window (0x0E) with a tailed item: window 0, slot 36, button 1,
/// action 255, mode 4 (drop) and item 300 ×1 with damage 42 and a compound
/// NBT tail — one string entry `k` = `v` — carried verbatim.
const CLICK_WINDOW_TAILED: &[u8] = &[
    0x0e, // the packet id
    0x00, // window id 0
    0x00, 0x24, // slot 36
    0x01, // button 1: the right mouse button
    0x00, 0xff, // action number 255
    0x04, // mode 4: drop
    0x01, 0x2c, // item id 300
    0x01, // count 1
    0x00, 0x2a, // damage 42
    0x0a, 0x00, 0x00, // NBT: root compound, empty name
    0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', // string `k` = `v`
    0x00, // end of the compound
];

/// Close Window (0x0D) for window 7: the packet id and the window id byte
/// (`C0DPacketCloseWindow.writePacketData:42`).
const CLOSE_WINDOW_7: &[u8] = &[
    0x0d, // the packet id
    0x07, // window id 7: the chest the fixture corpus opens
];

/// Close Window (0x0D) for window 0, the player's own inventory — the window
/// the inventory key closes.
const CLOSE_WINDOW_0: &[u8] = &[
    0x0d, // the packet id
    0x00, // window id 0
];

/// Confirm Transaction (0x0F), accepted: window 0, action 2, the flag byte 1
/// (`C0FPacketConfirmTransaction.writePacketData:48-50`).
const CONFIRM_TRANSACTION_ACCEPTED: &[u8] = &[
    0x0f, // the packet id
    0x00, // window id 0
    0x00, 0x02, // action number 2
    0x01, // accepted
];

/// Confirm Transaction (0x0F), rejected: window 7, action 6, the flag byte 0.
const CONFIRM_TRANSACTION_REJECTED: &[u8] = &[
    0x0f, // the packet id
    0x07, // window id 7
    0x00, 0x06, // action number 6
    0x00, // rejected
];

/// Creative Inventory Action (0x10) with an empty slot: slot 36, then the
/// empty stack's short `-1` (`C10PacketCreativeInventoryAction.writePacketData:46-47`).
const CREATIVE_ACTION_EMPTY: &[u8] = &[
    0x10, // the packet id
    0x00, 0x24, // slot 36
    0xff, 0xff, // the empty stack
];

/// Creative Inventory Action (0x10) with a plain item: slot 1 holds the
/// diamond sword, id 276, ×1, damage 0, and a no-data tag byte closing the
/// stack (`PacketBuffer.writeNBTTagCompoundToBuffer:191-208` writes the zero byte for
/// a null tag — the inverse of `readNBTTagCompoundFromBuffer:213-227`).
const CREATIVE_ACTION_ITEM: &[u8] = &[
    0x10, // the packet id
    0x00, 0x01, // slot 1
    0x01, 0x14, // item id 276: the diamond sword
    0x01, // count 1
    0x00, 0x00, // damage 0
    0x00, // NBT: no data
];

/// Creative Inventory Action (0x10) with a tailed item: slot 36 holds item
/// 300 ×2 with damage 42 and the same compound tail the click fixture
/// carries.
const CREATIVE_ACTION_TAILED: &[u8] = &[
    0x10, // the packet id
    0x00, 0x24, // slot 36
    0x01, 0x2c, // item id 300
    0x02, // count 2
    0x00, 0x2a, // damage 42
    0x0a, 0x00, 0x00, // NBT: root compound, empty name
    0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', // string `k` = `v`
    0x00, // end of the compound
];

/// Enchant Item (0x11): window 0, the first offer's index 1 — two bytes
/// (`C11PacketEnchantItem.writePacketData:45-46`).
const ENCHANT_ITEM_FIRST: &[u8] = &[
    0x11, // the packet id
    0x00, // window id 0
    0x01, // enchantment index 1
];

/// Enchant Item (0x11) on window 5: the third offer's index 2.
const ENCHANT_ITEM_THIRD: &[u8] = &[
    0x11, // the packet id
    0x05, // window id 5
    0x02, // enchantment index 2
];

/// Update Sign (0x12) at (0, 65, 2): the Location Position packed as
/// `BlockPos.toLong` packs it (`util/BlockPos.java:200-203`) — the same
/// hand-derived literal the digging fixture carries — then the four lines,
/// each a length-prefixed UTF-8 string (`PacketBuffer.writeString:314-315`).
/// The lines are chat components' JSON, the strings the source serializes
/// (`IChatComponent.Serializer.componentToJson`,
/// `C12PacketUpdateSign.writePacketData:51-52`).
const UPDATE_SIGN: &[u8] = &[
    0x12, // the packet id
    0x00, 0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x02, // (0, 65, 2)
    0x0c, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'a', b'"',
    b'}', // `{"text":"a"}`, 12 bytes
    0x0c, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'b', b'"',
    b'}', // `{"text":"b"}`
    0x0c, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'c', b'"',
    b'}', // `{"text":"c"}`
    0x0c, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'd', b'"',
    b'}', // `{"text":"d"}`
];

/// The four JSON lines the Update Sign fixture carries, in wire order.
fn sign_lines() -> [String; 4] {
    ["a", "b", "c", "d"].map(|text| format!("{{\"text\":\"{text}\"}}"))
}

/// The compound tail the two tailed slot fixtures carry: root compound with
/// an empty name and the string entry `k` = `v`.
fn k_equals_v_tail() -> Vec<u8> {
    vec![
        0x0a, 0x00, 0x00, // root compound, empty name
        0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', // string `k` = `v`
        0x00, // end of the compound
    ]
}

#[test]
fn held_item_change_carries_the_slot_short() {
    let mut out = Vec::new();
    write_held_item_change(&mut out, 3).expect("writing to a Vec cannot fail");
    assert_eq!(out, HELD_ITEM_CHANGE_SLOT_3, "slot 3");

    let mut out = Vec::new();
    write_held_item_change(&mut out, 8).expect("writing to a Vec cannot fail");
    assert_eq!(out, HELD_ITEM_CHANGE_SLOT_8, "slot 8");
}

#[test]
fn click_window_carries_the_empty_clicked_item() {
    let mut out = Vec::new();
    write_click_window(&mut out, 0, 36, 0, 5, None, 0).expect("writing to a Vec cannot fail");
    assert_eq!(out, CLICK_WINDOW_EMPTY, "the empty click");
    assert_eq!(
        &out[1..8],
        &[0x00, 0x00, 0x24, 0x00, 0x00, 0x05, 0x00],
        "window id, slot, button, action number and mode, in the writer's order"
    );
    assert_eq!(&out[8..], &[0xff, 0xff], "the empty clicked item");
}

#[test]
fn click_window_carries_the_tailed_clicked_item() {
    // The clicked item as the source carries it: the stack `slotClick`
    // returned (`PlayerControllerMP.windowClick:537-538`), raw tail included.
    let item = MetadataItem {
        id: 300,
        count: 1,
        damage: 42,
        nbt: Some(k_equals_v_tail()),
    };
    let mut out = Vec::new();
    write_click_window(&mut out, 0, 36, 1, 255, Some(&item), 4)
        .expect("writing to a Vec cannot fail");
    assert_eq!(out, CLICK_WINDOW_TAILED, "the tailed click");
    assert_eq!(
        &out[1..8],
        &[0x00, 0x00, 0x24, 0x01, 0x00, 0xff, 0x04],
        "the fixed fields, mode before the item"
    );
    assert_eq!(
        &out[8..],
        &[
            0x01, 0x2c, // item id 300
            0x01, // count 1
            0x00, 0x2a, // damage 42
            0x0a, 0x00, 0x00, 0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', 0x00, // the NBT tail
        ],
        "the clicked item, tail included"
    );
}

#[test]
fn click_window_echoes_the_original_slot_bytes() {
    // The decode-to-write pair: the fixture decodes field by field — the
    // slot through the crate's shared `read_slot` — and the writer must
    // reproduce the original bytes exactly, NBT tail included.
    let mut cursor = Cursor::new(CLICK_WINDOW_TAILED);
    assert_eq!(
        codec::read_u8(&mut cursor).expect("the id"),
        0x0e,
        "0x0E Click Window"
    );
    let window_id = codec::read_u8(&mut cursor).expect("the window id") as i8;
    let slot = codec::read_i16(&mut cursor).expect("the slot");
    let button = codec::read_u8(&mut cursor).expect("the button") as i8;
    let action = codec::read_i16(&mut cursor).expect("the action number");
    let mode = codec::read_u8(&mut cursor).expect("the mode") as i8;
    let item = read_slot(&mut cursor)
        .expect("the clicked slot decodes")
        .expect("the clicked slot is filled");
    assert_eq!(
        cursor.position() as usize,
        CLICK_WINDOW_TAILED.len(),
        "the fixture is consumed whole"
    );
    assert_eq!(
        item.nbt.as_deref(),
        Some(
            &[
                0x0a, 0x00, 0x00, 0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', 0x00
            ][..]
        ),
        "the tail is captured verbatim"
    );

    let mut out = Vec::new();
    write_click_window(&mut out, window_id, slot, button, action, Some(&item), mode)
        .expect("writing to a Vec cannot fail");
    assert_eq!(out, CLICK_WINDOW_TAILED, "the echoed packet is byte-exact");
}

#[test]
fn creative_action_carries_the_slot_and_the_stack() {
    let mut out = Vec::new();
    write_creative_inventory_action(&mut out, 36, None).expect("writing to a Vec cannot fail");
    assert_eq!(out, CREATIVE_ACTION_EMPTY, "the emptied slot");

    let item = MetadataItem {
        id: 276,
        count: 1,
        damage: 0,
        nbt: None,
    };
    let mut out = Vec::new();
    write_creative_inventory_action(&mut out, 1, Some(&item))
        .expect("writing to a Vec cannot fail");
    assert_eq!(out, CREATIVE_ACTION_ITEM, "the placed sword");
    assert_eq!(&out[1..3], &[0x00, 0x01], "the slot short");
    assert_eq!(
        &out[3..],
        &[0x01, 0x14, 0x01, 0x00, 0x00, 0x00],
        "the stack, closed by the no-data tag byte"
    );
}

#[test]
fn creative_action_echoes_the_original_slot_bytes() {
    let mut cursor = Cursor::new(CREATIVE_ACTION_TAILED);
    assert_eq!(
        codec::read_u8(&mut cursor).expect("the id"),
        0x10,
        "0x10 Creative Inventory Action"
    );
    let slot = codec::read_i16(&mut cursor).expect("the slot");
    let item = read_slot(&mut cursor)
        .expect("the stack decodes")
        .expect("the stack is filled");
    assert_eq!(
        cursor.position() as usize,
        CREATIVE_ACTION_TAILED.len(),
        "the fixture is consumed whole"
    );

    let mut out = Vec::new();
    write_creative_inventory_action(&mut out, slot, Some(&item))
        .expect("writing to a Vec cannot fail");
    assert_eq!(
        out, CREATIVE_ACTION_TAILED,
        "the echoed packet is byte-exact"
    );
}

#[test]
fn close_window_carries_the_window_id() {
    let mut out = Vec::new();
    write_close_window(&mut out, 7).expect("writing to a Vec cannot fail");
    assert_eq!(out, CLOSE_WINDOW_7, "the chest's window");

    let mut out = Vec::new();
    write_close_window(&mut out, 0).expect("writing to a Vec cannot fail");
    assert_eq!(out, CLOSE_WINDOW_0, "the player's own inventory");
}

#[test]
fn confirm_transaction_carries_the_pair() {
    let mut out = Vec::new();
    write_confirm_transaction(&mut out, 0, 2, true).expect("writing to a Vec cannot fail");
    assert_eq!(out, CONFIRM_TRANSACTION_ACCEPTED, "the accepted pair");
    assert_eq!(&out[1], &0x00, "the window id");
    assert_eq!(&out[2..4], &[0x00, 0x02], "the action number");
    assert_eq!(&out[4], &0x01, "the accepted flag");

    let mut out = Vec::new();
    write_confirm_transaction(&mut out, 7, 6, false).expect("writing to a Vec cannot fail");
    assert_eq!(out, CONFIRM_TRANSACTION_REJECTED, "the rejected pair");
    assert_eq!(&out[4], &0x00, "the rejected flag");
}

#[test]
fn enchant_item_carries_the_window_and_the_index() {
    let mut out = Vec::new();
    write_enchant_item(&mut out, 0, 1).expect("writing to a Vec cannot fail");
    assert_eq!(out, ENCHANT_ITEM_FIRST, "the first offer");

    let mut out = Vec::new();
    write_enchant_item(&mut out, 5, 2).expect("writing to a Vec cannot fail");
    assert_eq!(out, ENCHANT_ITEM_THIRD, "the third offer");
}

#[test]
fn update_sign_carries_the_position_and_the_lines() {
    let lines = sign_lines();
    let mut out = Vec::new();
    write_update_sign(&mut out, 0, 65, 2, &lines).expect("writing to a Vec cannot fail");
    assert_eq!(out, UPDATE_SIGN, "the sign at (0, 65, 2)");
    assert_eq!(
        &out[1..9],
        &[0x00, 0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x02],
        "the Location Position"
    );
    assert_eq!(&out[9..22], &UPDATE_SIGN[9..22], "the first line");
    assert_eq!(&out[48..], &UPDATE_SIGN[48..], "the last line");
}

#[test]
fn close_window_decodes_back_through_the_window_decoder() {
    // The close pair is bidirectionally defined: the serverbound 0x0D and
    // the clientbound 0x2E carry the same one window id
    // (`S2EPacketCloseWindow.readPacketData:32-35`). What the writer builds
    // must decode back to what it was given.
    for window_id in [0u8, 7, 12, 255] {
        let mut out = Vec::new();
        write_close_window(&mut out, window_id).expect("writing to a Vec cannot fail");
        let decoded = CloseWindow::decode(&out[1..]).expect("the built bytes decode");
        assert_eq!(decoded, CloseWindow { window_id }, "window {window_id}");
    }
}

#[test]
fn confirm_transaction_decodes_back_through_the_window_decoder() {
    // The same shape on both sides (`S32PacketConfirmTransaction.readPacketData:36-41`):
    // the window id byte, the action short and the accepted flag.
    for (window_id, action, accepted) in [(0i8, 2i16, true), (7, 6, false), (-1, 17, true)] {
        let mut out = Vec::new();
        write_confirm_transaction(&mut out, window_id, action, accepted)
            .expect("writing to a Vec cannot fail");
        let decoded = ConfirmTransaction::decode(&out[1..]).expect("the built bytes decode");
        assert_eq!(
            decoded,
            ConfirmTransaction {
                window_id,
                action,
                accepted,
            },
            "window {window_id}, action {action}, accepted {accepted}"
        );
    }
}

#[test]
fn update_sign_decodes_back_through_the_window_decoder() {
    // The lines survive the round trip: what the writer frames as strings the
    // clientbound reader (`S33PacketUpdateSign.readPacketData:31-40`) takes
    // back verbatim.
    let lines = sign_lines();
    let mut out = Vec::new();
    write_update_sign(&mut out, -12, 70, 34, &lines).expect("writing to a Vec cannot fail");
    let decoded = UpdateSign::decode(&out[1..]).expect("the built bytes decode");
    assert_eq!(
        decoded,
        UpdateSign {
            x: -12,
            y: 70,
            z: 34,
            lines,
        },
        "the position and the four lines"
    );
}

#[test]
fn a_long_sign_line_is_not_clamped() {
    // A sign line has no 100-character rule of its own: the source's write
    // path (`C12PacketUpdateSign.writePacketData:48-52`) serializes the line
    // and writes it through `PacketBuffer.writeString:304-317`, whose only
    // ceiling is 32767 bytes. A hundred characters ride the wire whole.
    let line = "a".repeat(100);
    let lines = [line.clone(), String::new(), String::new(), String::new()];
    let mut out = Vec::new();
    write_update_sign(&mut out, 3, 64, -7, &lines).expect("writing to a Vec cannot fail");
    assert_eq!(
        out.len(),
        9 + 1 + 100 + 3,
        "id, position, the line and three empties"
    );
    assert_eq!(out[9], 0x64, "the line's length prefix: 100 bytes");
    assert_eq!(&out[10..110], line.as_bytes(), "the line rides in full");
    assert_eq!(&out[110..], &[0x00, 0x00, 0x00], "the three empty lines");

    let decoded = UpdateSign::decode(&out[1..]).expect("the built bytes decode");
    assert_eq!(decoded.lines, lines, "the long line survives");
}

#[test]
fn a_sign_line_at_the_string_ceiling_writes_and_one_past_it_refuses() {
    // The string writer's rule, as the source's own writer states it: 32767
    // bytes encoded is the most a string field carries, and one more byte is
    // refused (`PacketBuffer.writeString:308-310` throws there; the crate's
    // `codec::write_string` refuses with `InvalidInput`). The writer frames
    // the line through that rule rather than a rule of its own.
    let at_ceiling = "a".repeat(32767);
    let lines = [
        at_ceiling.clone(),
        String::new(),
        String::new(),
        String::new(),
    ];
    let mut out = Vec::new();
    write_update_sign(&mut out, 0, 65, 2, &lines).expect("the ceiling itself writes");
    assert_eq!(
        out.len(),
        9 + 3 + 32767 + 3,
        "id, position, the three-byte length and the line, then three empties"
    );
    let decoded = UpdateSign::decode(&out[1..]).expect("the ceiling decodes back");
    assert_eq!(decoded.lines, lines, "the line at the ceiling survives");

    let past_ceiling = "a".repeat(32768);
    let lines = [past_ceiling, String::new(), String::new(), String::new()];
    let mut out = Vec::new();
    let error = write_update_sign(&mut out, 0, 65, 2, &lines)
        .expect_err("one byte past the ceiling is refused");
    assert_eq!(
        error.kind(),
        ErrorKind::InvalidInput,
        "the refusal is the string rule's"
    );
}
