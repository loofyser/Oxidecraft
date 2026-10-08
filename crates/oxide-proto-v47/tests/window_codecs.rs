//! The window and sign codec fixture corpus: one hand-built byte vector per
//! window, sign, experience and entity-effect packet, and the merchant offer
//! list.
//!
//! Every byte is packed by hand from the layouts the protocol reference
//! records (`docs/research/protocol-47-reference.md` §2.1) and from the packet
//! classes' own readers (`S2DPacketOpenWindow.readPacketData:51-61`,
//! `S2EPacketCloseWindow.readPacketData:32-35`,
//! `S2FPacketSetSlot.readPacketData:37-42`,
//! `S30PacketWindowItems.readPacketData:34-43`,
//! `S31PacketWindowProperty.readPacketData:36-41`,
//! `S32PacketConfirmTransaction.readPacketData:36-41`,
//! `S33PacketUpdateSign.readPacketData:31-40`,
//! `S36PacketSignEditorOpen.readPacketData:33-36`,
//! `S1FPacketSetExperience.readPacketData:28-33`,
//! `S1DPacketEntityEffect.readPacketData:42-49`,
//! `S1EPacketRemoveEntityEffect.readPacketData:27-31`,
//! `MerchantRecipeList.readFromBuf:76-106`), never rebuilt with the decoder's
//! arithmetic, so a wrong field order or a wrong shift cannot be confirmed by
//! its own twin.

use std::io::ErrorKind;

use oxide_proto::codec::CodecError;
use oxide_proto_v47::PacketError;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::{
    CloseWindow, ConfirmTransaction, EntityEffect, MerchantOffers, OpenWindow, RemoveEntityEffect,
    SetExperience, SetSlot, SignEditorOpen, UpdateSign, WindowItems, WindowKind, WindowProperty,
};

/// Open Window (0x2D), a single chest: window 7, `minecraft:chest`, the chat
/// JSON `{"text":"Chest"}`, 27 slots and — the type is not `EntityHorse` — no
/// entity id.
const OPEN_CHEST: &[u8] = &[
    0x07, // window id 7
    0x0f, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'c', b'h', b'e', b's',
    b't', // "minecraft:chest", 15 bytes
    0x10, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'C', b'h', b'e', b's', b't', b'"',
    b'}', // `{"text":"Chest"}`, 16 bytes
    0x1b, // slot count 27: a single chest's inventory
];

/// Open Window (0x2D), a horse: window 5, `EntityHorse`, the chat JSON
/// `{"text":"Horse"}`, 17 slots and the entity id 456 — present only because
/// the type is `EntityHorse` (`S2DPacketOpenWindow.readPacketData:58-61`).
const OPEN_HORSE: &[u8] = &[
    0x05, // window id 5
    0x0b, b'E', b'n', b't', b'i', b't', b'y', b'H', b'o', b'r', b's',
    b'e', // "EntityHorse", 11 bytes
    0x10, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'H', b'o', b'r', b's', b'e', b'"',
    b'}', // `{"text":"Horse"}`, 16 bytes
    0x11, // slot count 17: a chested horse's skeleton plus its saddle and armour
    0x00, 0x00, 0x01, 0xc8, // entity id 456
];

/// Open Window (0x2D), a type outside the source's table: window 9,
/// `minecraft:banner`, the chat JSON `{"text":"?"}`, one slot; the kind reads
/// as [`WindowKind::Unknown`] and no entity id follows.
const OPEN_UNKNOWN: &[u8] = &[
    0x09, // window id 9
    0x10, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'b', b'a', b'n', b'n', b'e',
    b'r', // "minecraft:banner", 16 bytes
    0x0c, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'?', b'"',
    b'}', // `{"text":"?"}`, 12 bytes
    0x01, // slot count 1
];

/// Close Window (0x2E): window 7, the chest opened above.
const CLOSE_WINDOW: &[u8] = &[
    0x07, // window id 7
];

/// Set Slot (0x2F) on the cursor: window id −1, slot 36, item 300 ×1 with
/// damage 42 and a compound NBT tail — one string entry `k` = `v` — captured
/// verbatim.
const SET_SLOT_CURSOR: &[u8] = &[
    0xff, // window id -1: the cursor
    0x00, 0x24, // slot 36
    0x01, 0x2c, // item id 300
    0x01, // count 1
    0x00, 0x2a, // damage 42
    0x0a, 0x00, 0x00, // NBT: root compound, empty name
    0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', // string `k` = `v`
    0x00, // end of the compound
];

/// Set Slot (0x2F) on window 0: slot 44 holds nothing — the empty slot is the
/// negative id and nothing else.
const SET_SLOT_EMPTY: &[u8] = &[
    0x00, // window id 0: the player's own inventory
    0x00, 0x2c, // slot 44
    0xff, 0xff, // the empty slot
];

/// The filled slots of the 90-slot Window Items fixture: the slot index, the
/// item id, the count and the damage. Every other index is empty.
const WINDOW_ITEMS_FILLED: [(usize, i16, u8, i16); 5] = [
    (0, 1, 64, 0),      // stone x64
    (35, 258, 1, 250),  // iron pickaxe
    (44, 353, 12, 0),   // sugar x12
    (53, 276, 1, 1560), // diamond sword, worn
    (89, 322, 2, 0),    // two golden apples
];

/// One slot's wire bytes: the item id and the damage as big-endian shorts
/// around the count, then the tail's no-data tag byte; the empty slot is the
/// negative id alone (`PacketBuffer.readItemStackFromBuffer:257-271`).
fn push_slot(out: &mut Vec<u8>, item: Option<(i16, u8, i16)>) {
    match item {
        Some((id, count, damage)) => {
            out.extend_from_slice(&id.to_be_bytes());
            out.push(count);
            out.extend_from_slice(&damage.to_be_bytes());
            out.push(0x00); // NBT: no data
        }
        None => out.extend_from_slice(&(-1i16).to_be_bytes()),
    }
}

/// The 90-slot Window Items (0x30) body: window 3, the count as a big-endian
/// short, then ninety slots — the table's five filled and the rest empty. A
/// 90-slot vector is generated from the literal table rather than typed
/// twice; every byte still comes from the table, written big-endian.
fn window_items_90() -> Vec<u8> {
    let mut body = vec![
        0x03, // window id 3
        0x00, 0x5a, // slot count 90
    ];
    for index in 0..90 {
        let filled = WINDOW_ITEMS_FILLED
            .iter()
            .find(|(slot, ..)| *slot == index)
            .map(|&(_, id, count, damage)| (id, count, damage));
        push_slot(&mut body, filled);
    }
    body
}

/// Window Property (0x31): window 2, property 258, value −1000 — both shorts
/// are read signed.
const WINDOW_PROPERTY: &[u8] = &[
    0x02, // window id 2
    0x01, 0x02, // property 258
    0xfc, 0x18, // value -1000
];

/// Confirm Transaction (0x32), rejected: window 15, action 258, accepted
/// false.
const CONFIRM_REJECTED: &[u8] = &[
    0x0f, // window id 15
    0x01, 0x02, // action 258
    0x00, // accepted: false
];

/// Update Sign (0x33): the position (100, 64, −200) packed by
/// `BlockPos.asLong` and four non-empty lines, each the chat JSON the client
/// keeps as text.
const SIGN_UPDATE: &[u8] = &[
    0x00, 0x00, 0x19, 0x01, 0x03, 0xff, 0xff, 0x38, // (100, 64, -200) packed
    0x13, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'l', b'i', b'n', b'e', b' ', b'o',
    b'n', b'e', b'"', b'}', // line 1: {"text":"line one"}
    0x13, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'l', b'i', b'n', b'e', b' ', b't',
    b'w', b'o', b'"', b'}', // line 2: {"text":"line two"}
    0x15, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'l', b'i', b'n', b'e', b' ', b't',
    b'h', b'r', b'e', b'e', b'"', b'}', // line 3: {"text":"line three"}
    0x14, b'{', b'"', b't', b'e', b'x', b't', b'"', b':', b'"', b'l', b'i', b'n', b'e', b' ', b'f',
    b'o', b'u', b'r', b'"', b'}', // line 4: {"text":"line four"}
];

/// Open Sign Editor (0x36): the position (12, 65, −3) packed by
/// `BlockPos.asLong`.
const SIGN_EDITOR: &[u8] = &[
    0x00, 0x00, 0x03, 0x01, 0x07, 0xff, 0xff, 0xfd, // (12, 65, -3) packed
];

/// Set Experience (0x1F): the bar halfway, level 30, 1395 total experience —
/// the level and the total are VarInts.
const SET_EXPERIENCE: &[u8] = &[
    0x3f, 0x00, 0x00, 0x00, // bar 0.5
    0x1e, // level 30
    0xf3, 0x0a, // total 1395
];

/// Set Experience (0x1F) at the boundaries: the bar at its 0–1 ceiling, the
/// level at zero and the total at the VarInt's positive top (`0x7FFFFFFF`,
/// five bytes).
const XP_EDGES: &[u8] = &[
    0x3f, 0x80, 0x00, 0x00, // bar 1.0
    0x00, // level 0
    0xff, 0xff, 0xff, 0xff, 0x07, // total 2147483647
];

/// Entity Effect (0x1D): entity 42, effect 9, amplifier 1, duration 2400
/// ticks and the hide-particles byte set.
const ENTITY_EFFECT: &[u8] = &[
    0x2a, // entity id 42
    0x09, // effect id 9
    0x01, // amplifier 1
    0xe0, 0x12, // duration 2400
    0x01, // hide particles: true
];

/// Entity Effect (0x1D) with the particles shown: entity 42, effect 9,
/// amplifier 0, duration 600, the hide byte zero.
const ENTITY_EFFECT_SHOWN: &[u8] = &[
    0x2a, // entity id 42
    0x09, // effect id 9
    0x00, // amplifier 0
    0xd8, 0x04, // duration 600
    0x00, // hide particles: false
];

/// Remove Entity Effect (0x1E): entity 42, effect 9.
const REMOVE_EFFECT: &[u8] = &[
    0x2a, // entity id 42
    0x09, // effect id 9
];

/// The merchant list (`MC|TrList`): the window id 7 written as a big-endian
/// int (`EntityPlayerMP.java:817`), two offers, and the count byte. Offer 1
/// carries a second item (emeralds for a diamond with coal); offer 2 has none
/// and is disabled.
const MERCHANT_OFFERS: &[u8] = &[
    0x00, 0x00, 0x00, 0x07, // window id 7 as an int
    0x02, // two offers
    0x01, 0x84, // first: emerald 388
    0x03, // count 3
    0x00, 0x00, // damage 0
    0x00, // NBT: no data
    0x01, 0x08, // output: diamond 264
    0x01, // count 1
    0x00, 0x00, // damage 0
    0x00, // NBT: no data
    0x01, // has second item: true
    0x01, 0x07, // second: coal 263
    0x08, // count 8
    0x00, 0x00, // damage 0
    0x00, // NBT: no data
    0x00, // disabled: false
    0x00, 0x00, 0x00, 0x02, // uses 2
    0x00, 0x00, 0x00, 0x07, // max uses 7
    0x00, 0x01, // first: stone 1
    0x20, // count 32
    0x00, 0x00, // damage 0
    0x00, // NBT: no data
    0x01, 0x0e, // output: wood pickaxe 270
    0x01, // count 1
    0x00, 0x00, // damage 0
    0x00, // NBT: no data
    0x00, // has second item: false
    0x01, // disabled: true
    0x00, 0x00, 0x00, 0x07, // uses 7
    0x00, 0x00, 0x00, 0x07, // max uses 7
];

#[test]
fn an_open_window_decodes_the_kind_the_title_and_the_slot_count() {
    let chest = OpenWindow::decode(OPEN_CHEST).expect("the chest fixture decodes");
    assert_eq!(chest.window_id, 7, "the chest window id");
    assert_eq!(chest.kind, WindowKind::Chest, "minecraft:chest");
    assert_eq!(chest.title, r#"{"text":"Chest"}"#, "the raw title JSON");
    assert_eq!(chest.slot_count, 27, "the chest's 27 slots");
    assert_eq!(chest.entity_id, None, "no entity id off a horse window");

    let horse = OpenWindow::decode(OPEN_HORSE).expect("the horse fixture decodes");
    assert_eq!(horse.window_id, 5, "the horse window id");
    assert_eq!(horse.kind, WindowKind::EntityHorse, "EntityHorse");
    assert_eq!(horse.title, r#"{"text":"Horse"}"#, "the raw title JSON");
    assert_eq!(horse.slot_count, 17, "the horse window's slots");
    assert_eq!(horse.entity_id, Some(456), "the horse's entity id");

    let unknown = OpenWindow::decode(OPEN_UNKNOWN).expect("the unknown-kind fixture decodes");
    assert_eq!(unknown.window_id, 9, "the unknown window id");
    assert_eq!(unknown.kind, WindowKind::Unknown, "an unlisted type string");
    assert_eq!(unknown.title, r#"{"text":"?"}"#, "the raw title JSON");
    assert_eq!(unknown.slot_count, 1, "the window's slots");
    assert_eq!(unknown.entity_id, None, "no entity id off an unlisted type");
}

#[test]
fn a_close_window_decodes_the_window_id() {
    let close = CloseWindow::decode(CLOSE_WINDOW).expect("the fixture decodes");
    assert_eq!(close.window_id, 7, "the window id");
}

#[test]
fn a_set_slot_decodes_the_cursor_window_and_the_tailed_item() {
    let set = SetSlot::decode(SET_SLOT_CURSOR).expect("the cursor fixture decodes");
    assert_eq!(set.window_id, -1, "window id -1: the cursor");
    assert_eq!(set.slot, 36, "the slot index");
    assert_eq!(
        set.item,
        Some(MetadataItem {
            id: 300,
            count: 1,
            damage: 42,
            nbt: Some(vec![
                0x0a, 0x00, 0x00, // root compound, empty name
                0x08, 0x00, 0x01, b'k', 0x00, 0x01, b'v', // string `k` = `v`
                0x00, // end of the compound
            ]),
        }),
        "the item and its verbatim NBT tail"
    );

    let empty = SetSlot::decode(SET_SLOT_EMPTY).expect("the empty fixture decodes");
    assert_eq!(empty.window_id, 0, "window id 0");
    assert_eq!(empty.slot, 44, "the slot index");
    assert_eq!(empty.item, None, "the empty slot");
}

#[test]
fn window_items_decodes_ninety_slots() {
    let body = window_items_90();
    let items = WindowItems::decode(&body).expect("the 90-slot fixture decodes");
    assert_eq!(items.window_id, 3, "the window id");
    assert_eq!(items.slots.len(), 90, "the declared 90 slots");

    // Every slot, against the literal table: five filled, the rest empty.
    let expected: Vec<Option<MetadataItem>> = (0..90)
        .map(|index| {
            WINDOW_ITEMS_FILLED
                .iter()
                .find(|(slot, ..)| *slot == index)
                .map(|&(_, id, count, damage)| MetadataItem {
                    id,
                    count,
                    damage,
                    nbt: None,
                })
        })
        .collect();
    assert_eq!(items.slots, expected, "all ninety slots");

    // Spot checks against the literals, so a systematically mirrored filler
    // cannot pass: a filled slot, its empty neighbour, and the last slot.
    assert_eq!(items.slots[0].as_ref().expect("filled").id, 1);
    assert_eq!(items.slots[0].as_ref().expect("filled").count, 64);
    assert_eq!(items.slots[35].as_ref().expect("filled").damage, 250);
    assert_eq!(items.slots[36], None, "an empty slot beside a filled one");
    assert_eq!(items.slots[44].as_ref().expect("filled").id, 353);
    assert_eq!(items.slots[53].as_ref().expect("filled").damage, 1560);
    assert_eq!(items.slots[89].as_ref().expect("filled").count, 2);
}

#[test]
fn a_window_property_decodes_the_pair() {
    let property = WindowProperty::decode(WINDOW_PROPERTY).expect("the fixture decodes");
    assert_eq!(property.window_id, 2, "the window id");
    assert_eq!(property.property, 258, "the property index");
    assert_eq!(property.value, -1000, "the value, read signed");
}

#[test]
fn a_confirm_transaction_decodes_a_rejection() {
    let confirm = ConfirmTransaction::decode(CONFIRM_REJECTED).expect("the fixture decodes");
    assert_eq!(confirm.window_id, 15, "the window id");
    assert_eq!(confirm.action, 258, "the action number");
    assert!(!confirm.accepted, "the rejected transaction");
}

#[test]
fn an_update_sign_decodes_the_position_and_four_lines() {
    let sign = UpdateSign::decode(SIGN_UPDATE).expect("the fixture decodes");
    assert_eq!(
        (sign.x, sign.y, sign.z),
        (100, 64, -200),
        "the sign position"
    );
    assert_eq!(
        sign.lines,
        [
            r#"{"text":"line one"}"#.to_string(),
            r#"{"text":"line two"}"#.to_string(),
            r#"{"text":"line three"}"#.to_string(),
            r#"{"text":"line four"}"#.to_string(),
        ],
        "the four lines in order"
    );
}

#[test]
fn a_sign_editor_open_decodes_the_position() {
    let editor = SignEditorOpen::decode(SIGN_EDITOR).expect("the fixture decodes");
    assert_eq!(
        (editor.x, editor.y, editor.z),
        (12, 65, -3),
        "the sign position"
    );
}

#[test]
fn a_set_experience_decodes_the_bar_the_level_and_the_total() {
    let experience = SetExperience::decode(SET_EXPERIENCE).expect("the fixture decodes");
    assert_eq!(experience.bar, 0.5, "the bar");
    assert_eq!(experience.level, 30, "the level");
    assert_eq!(experience.total, 1395, "the total experience");

    let edges = SetExperience::decode(XP_EDGES).expect("the boundary fixture decodes");
    assert_eq!(edges.bar, 1.0, "the bar at its ceiling");
    assert_eq!(edges.level, 0, "the level at zero");
    assert_eq!(edges.total, i32::MAX, "the total at the VarInt's top");
}

#[test]
fn entity_effects_decode_both_packets() {
    let effect = EntityEffect::decode(ENTITY_EFFECT).expect("the effect fixture decodes");
    assert_eq!(effect.entity_id, 42, "the entity id");
    assert_eq!(effect.effect_id, 9, "the effect id");
    assert_eq!(effect.amplifier, 1, "the amplifier");
    assert_eq!(effect.duration, 2400, "the duration");
    assert!(effect.hide_particles, "the particles hidden");

    let shown = EntityEffect::decode(ENTITY_EFFECT_SHOWN).expect("the shown fixture decodes");
    assert_eq!(shown.entity_id, 42, "the entity id");
    assert_eq!(shown.effect_id, 9, "the effect id");
    assert_eq!(shown.amplifier, 0, "the amplifier");
    assert_eq!(shown.duration, 600, "the duration");
    assert!(!shown.hide_particles, "a zero means the particles show");

    let remove = RemoveEntityEffect::decode(REMOVE_EFFECT).expect("the remove fixture decodes");
    assert_eq!(remove.entity_id, 42, "the entity id");
    assert_eq!(remove.effect_id, 9, "the effect id");
}

#[test]
fn merchant_offers_decode_two_offers() {
    let offers = MerchantOffers::decode(MERCHANT_OFFERS).expect("the offer fixture decodes");
    assert_eq!(offers.offers.len(), 2, "two offers");

    let first = &offers.offers[0];
    assert_eq!(
        first
            .first
            .as_ref()
            .map(|item| (item.id, item.count, item.damage)),
        Some((388, 3, 0)),
        "the first slot: the emeralds the villager buys"
    );
    assert_eq!(
        first
            .second
            .as_ref()
            .map(|item| (item.id, item.count, item.damage)),
        Some((263, 8, 0)),
        "the second slot: the coal, present"
    );
    assert_eq!(
        first
            .output
            .as_ref()
            .map(|item| (item.id, item.count, item.damage)),
        Some((264, 1, 0)),
        "the output slot: the diamond the villager sells"
    );
    assert_eq!(first.uses, 2, "two uses");
    assert_eq!(first.max_uses, 7, "seven max uses");

    let second = &offers.offers[1];
    assert_eq!(
        second
            .first
            .as_ref()
            .map(|item| (item.id, item.count, item.damage)),
        Some((1, 32, 0)),
        "the first slot: the stone"
    );
    assert_eq!(second.second, None, "no second item");
    assert_eq!(
        second
            .output
            .as_ref()
            .map(|item| (item.id, item.count, item.damage)),
        Some((270, 1, 0)),
        "the output slot: the wood pickaxe"
    );
    assert_eq!(second.uses, 7, "seven uses");
    assert_eq!(second.max_uses, 7, "seven max uses");
}

/// Unwraps a named refusal, asserting it is the invalid-data class, and
/// returns its message.
fn refusal_message(error: PacketError) -> String {
    match error {
        PacketError::Codec(CodecError::Io(io)) => {
            assert_eq!(
                io.kind(),
                ErrorKind::InvalidData,
                "a named refusal is invalid data"
            );
            io.to_string()
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
}

#[test]
fn window_items_refuses_a_negative_count() {
    // The count short read as 0xFFFF: the source's `new ItemStack[-1]` dies
    // on the negative size; this client refuses it by name.
    assert!(matches!(
        WindowItems::decode(&[0x03, 0xff, 0xff]),
        Err(PacketError::Codec(CodecError::NegativeLength(-1)))
    ));
}

#[test]
fn window_items_refuses_a_count_the_body_cannot_hold() {
    // The 90-slot body with the count bumped to 200: the 200 bytes that
    // remain hold at most 100 empty slots (each slot payload is at least its
    // two-byte empty form), so the count is refused before a slot is read.
    let mut body = window_items_90();
    body[1] = 0x00;
    body[2] = 0xc8; // count 200
    let message = refusal_message(WindowItems::decode(&body).expect_err("refused"));
    assert_eq!(
        message,
        "Window Items declares 200 slots but 200 byte(s) remain, holding at most 100"
    );
}

#[test]
fn window_items_refuses_slots_past_the_declared_count() {
    // The 90-slot body with the count lowered to 89: the last slot — index
    // 89, one of the filled six-byte ones — is past the declared count and
    // its six bytes are refused as trailing.
    let mut body = window_items_90();
    body[2] = 0x59; // count 89
    assert!(matches!(
        WindowItems::decode(&body),
        Err(PacketError::Trailing(6))
    ));
}

#[test]
fn an_open_window_refuses_a_type_and_a_title_past_their_caps() {
    // The type field's source cap of 32 (`S2DPacketOpenWindow:54`), applied
    // as a byte cap: a declared 33 is refused before its bytes are read.
    assert!(matches!(
        OpenWindow::decode(&[0x07, 0x21]),
        Err(PacketError::Codec(CodecError::TooLong { len: 33, max: 32 }))
    ));
    // The title's protocol ceiling: a declared 32768.
    let mut body = vec![0x07, 0x0f];
    body.extend_from_slice(b"minecraft:chest");
    body.extend_from_slice(&[0x80, 0x80, 0x02]); // title length 32768
    assert!(matches!(
        OpenWindow::decode(&body),
        Err(PacketError::Codec(CodecError::TooLong {
            len: 32768,
            max: 32767
        }))
    ));
}

#[test]
fn an_update_sign_refuses_a_line_past_the_crate_cap() {
    // The position and one good line, then a line that declares 32768 bytes:
    // refused before its bytes are read.
    let mut body = Vec::from(&SIGN_UPDATE[..8]);
    body.extend_from_slice(&[0x13]);
    body.extend_from_slice(br#"{"text":"line one"}"#);
    body.extend_from_slice(&[0x80, 0x80, 0x02]); // line 2 length 32768
    assert!(matches!(
        UpdateSign::decode(&body),
        Err(PacketError::Codec(CodecError::TooLong {
            len: 32768,
            max: 32767
        }))
    ));
}

#[test]
fn merchant_offers_refuse_a_count_past_the_cap() {
    // Window 7, a count of 129 and two readable offers: the count is past
    // the 128 offer cap and is refused up front.
    let mut body = vec![0x00, 0x00, 0x00, 0x07, 0x81];
    body.extend_from_slice(&MERCHANT_OFFERS[5..]);
    let message = refusal_message(MerchantOffers::decode(&body).expect_err("refused"));
    assert_eq!(
        message,
        "merchant offer count 129 exceeds the 128 offer cap"
    );
}

#[test]
fn a_truncated_offer_tail_is_refused() {
    // The fixture cut inside the second offer's use ints.
    let cut = &MERCHANT_OFFERS[..MERCHANT_OFFERS.len() - 3];
    assert!(MerchantOffers::decode(cut).is_err(), "a cut offer tail");
}

#[test]
fn every_window_decoder_refuses_a_trailing_byte() {
    let window_items = window_items_90();
    let cases: [(&[u8], &str); 14] = [
        (OPEN_CHEST, "an open window"),
        (OPEN_UNKNOWN, "an unknown-kind window"),
        (CLOSE_WINDOW, "a close"),
        (SET_SLOT_CURSOR, "a tailed set slot"),
        (SET_SLOT_EMPTY, "an empty set slot"),
        (window_items.as_slice(), "window items"),
        (WINDOW_PROPERTY, "a property"),
        (CONFIRM_REJECTED, "a confirm"),
        (SIGN_UPDATE, "a sign update"),
        (SIGN_EDITOR, "a sign editor"),
        (SET_EXPERIENCE, "an experience"),
        (ENTITY_EFFECT, "an effect"),
        (REMOVE_EFFECT, "a removed effect"),
        (MERCHANT_OFFERS, "an offer list"),
    ];
    for (body, what) in cases {
        let mut trailing = body.to_vec();
        trailing.push(0x00);
        let refused = match what {
            "an open window" => {
                matches!(OpenWindow::decode(&trailing), Err(PacketError::Trailing(1)))
            }
            "an unknown-kind window" => {
                matches!(OpenWindow::decode(&trailing), Err(PacketError::Trailing(1)))
            }
            "a close" => matches!(
                CloseWindow::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a tailed set slot" => {
                matches!(SetSlot::decode(&trailing), Err(PacketError::Trailing(1)))
            }
            "an empty set slot" => {
                matches!(SetSlot::decode(&trailing), Err(PacketError::Trailing(1)))
            }
            "window items" => matches!(
                WindowItems::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a property" => matches!(
                WindowProperty::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a confirm" => matches!(
                ConfirmTransaction::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a sign update" => {
                matches!(UpdateSign::decode(&trailing), Err(PacketError::Trailing(1)))
            }
            "a sign editor" => matches!(
                SignEditorOpen::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "an experience" => matches!(
                SetExperience::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "an effect" => matches!(
                EntityEffect::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            "a removed effect" => matches!(
                RemoveEntityEffect::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
            _ => matches!(
                MerchantOffers::decode(&trailing),
                Err(PacketError::Trailing(1))
            ),
        };
        assert!(refused, "one trailing byte after {what} is refused");
    }
}

#[test]
fn every_prefix_of_every_fixture_is_refused() {
    // The truncation sweep: every strict prefix of every fixture — the whole
    // window family, the sign pair, the experience, the effect pair and the
    // offer list — is an error, never a panic.
    /// A decoder under the sweep: `true` when the cut payload is refused.
    type CutCheck = fn(&[u8]) -> bool;
    let window_items = window_items_90();
    let fixtures: [(&[u8], &str, CutCheck); 15] = [
        (OPEN_CHEST, "an open window", |body| {
            OpenWindow::decode(body).is_err()
        }),
        (OPEN_HORSE, "a horse window", |body| {
            OpenWindow::decode(body).is_err()
        }),
        (OPEN_UNKNOWN, "an unknown-kind window", |body| {
            OpenWindow::decode(body).is_err()
        }),
        (CLOSE_WINDOW, "a close", |body| {
            CloseWindow::decode(body).is_err()
        }),
        (SET_SLOT_CURSOR, "a tailed set slot", |body| {
            SetSlot::decode(body).is_err()
        }),
        (SET_SLOT_EMPTY, "an empty set slot", |body| {
            SetSlot::decode(body).is_err()
        }),
        (window_items.as_slice(), "window items", |body| {
            WindowItems::decode(body).is_err()
        }),
        (WINDOW_PROPERTY, "a property", |body| {
            WindowProperty::decode(body).is_err()
        }),
        (CONFIRM_REJECTED, "a confirm", |body| {
            ConfirmTransaction::decode(body).is_err()
        }),
        (SIGN_UPDATE, "a sign update", |body| {
            UpdateSign::decode(body).is_err()
        }),
        (SIGN_EDITOR, "a sign editor", |body| {
            SignEditorOpen::decode(body).is_err()
        }),
        (SET_EXPERIENCE, "an experience", |body| {
            SetExperience::decode(body).is_err()
        }),
        (XP_EDGES, "the experience edges", |body| {
            SetExperience::decode(body).is_err()
        }),
        (ENTITY_EFFECT, "an effect", |body| {
            EntityEffect::decode(body).is_err()
        }),
        (MERCHANT_OFFERS, "an offer list", |body| {
            MerchantOffers::decode(body).is_err()
        }),
    ];
    for (fixture, what, refused) in fixtures {
        for cut in 0..fixture.len() {
            assert!(
                refused(&fixture[..cut]),
                "the cut at byte {cut} of {what} is refused, never a panic"
            );
        }
    }
}
