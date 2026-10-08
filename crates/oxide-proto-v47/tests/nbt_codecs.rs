//! The NBT codec fixture corpus: slot payloads, their raw tails, the
//! byte-exact echo pairs, and the reader's trees — the book-pages shape and
//! the hostile ones.
//!
//! Every slot fixture is a hand-built payload: the item id, the count, the
//! damage and the raw NBT tail. The echo tests decode a fixture with the
//! crate's slot reader and write it back with the slot writer, asserting the
//! output is the input byte for byte — the tail is kept verbatim, so a
//! round trip must not disturb a single byte. The capped fixtures are
//! generated in the test; a 65536-byte tail is not a source literal. The
//! reader tests walk the captured tails through `oxide_proto_v47::nbt` and
//! record each hostile refusal.

use std::io::Cursor;

use oxide_proto_v47::entity::{MAX_SLOT_NBT_BYTES, MetadataItem, read_slot, write_slot};
use oxide_proto_v47::nbt::{MAX_NBT_DEPTH, NbtError, NbtValue, parse};

/// One slot payload: item 276, one count, no damage, and a compound tail
/// with two entries — the byte "x" = 127 and the short "y" = 300.
const SLOT_WITH_COMPOUND_TAIL: &[u8] = &[
    0x01, 0x14, // item id 276
    0x01, // count 1
    0x00, 0x00, // damage 0
    0x0a, 0x00, 0x00, // NBT: root compound, empty name
    0x01, 0x00, 0x01, b'x', 0x7f, // byte "x" = 127
    0x02, 0x00, 0x01, b'y', 0x01, 0x2c, // short "y" = 300
    0x00, // end of the compound
];

/// One slot payload whose tag byte is zero: no NBT data at all
/// (`PacketBuffer.readNBTTagCompoundFromBuffer:213-227`), item 276 x2 with
/// damage 42.
const SLOT_WITHOUT_TAIL: &[u8] = &[
    0x01, 0x14, // item id 276
    0x02, // count 2
    0x00, 0x2a, // damage 42
    0x00, // NBT: no data
];

/// The empty slot: the negative id and nothing else
/// (`PacketBuffer.readItemStackFromBuffer:257-271`).
const SLOT_EMPTY: &[u8] = &[0xff, 0xff];

/// Decodes one slot fixture, expecting the non-empty shape.
fn decode_item(payload: &[u8]) -> MetadataItem {
    read_slot(&mut Cursor::new(payload))
        .expect("the slot payload decodes")
        .expect("the slot is non-empty")
}

/// Writes one slot value back to bytes.
fn echo_slot(item: Option<&MetadataItem>) -> Vec<u8> {
    let mut out = Vec::new();
    write_slot(&mut out, item).expect("writing into a Vec cannot fail");
    out
}

/// One slot payload — item 276 x1, no damage — whose compound tail is
/// exactly `tail_bytes` bytes: a byte-array child pads the tail to size.
/// The closed-form size is the compound header (1 + 2 + 0), the child header
/// (1 + 2 + 0), the array's four-byte length, the payload and the compound's
/// terminator: 11 + payload.
fn slot_with_tail_bytes(tail_bytes: usize) -> Vec<u8> {
    let array_len = tail_bytes - 11;
    let mut payload = vec![
        0x01, 0x14, // item id 276
        0x01, // count 1
        0x00, 0x00, // damage 0
        0x0a, 0x00, 0x00, // NBT: root compound, empty name
        0x07, 0x00, 0x00, // child: byte array, empty name
    ];
    payload.extend_from_slice(&(array_len as i32).to_be_bytes());
    payload.resize(payload.len() + array_len, 0x5a);
    payload.push(0x00); // end of the compound
    assert_eq!(
        payload.len(),
        5 + tail_bytes,
        "the slot prefix is five bytes"
    );
    payload
}

#[test]
fn a_slot_tail_is_captured_verbatim() {
    let item = decode_item(SLOT_WITH_COMPOUND_TAIL);
    assert_eq!(item.id, 276);
    assert_eq!(item.count, 1);
    assert_eq!(item.damage, 0);
    assert_eq!(
        item.nbt.as_deref(),
        Some(&SLOT_WITH_COMPOUND_TAIL[5..]),
        "the tail is the wire bytes after the damage, minus nothing"
    );
}

#[test]
fn a_zero_tag_byte_is_no_tail() {
    let item = decode_item(SLOT_WITHOUT_TAIL);
    assert_eq!(item.id, 276);
    assert_eq!(item.count, 2);
    assert_eq!(item.damage, 42);
    assert_eq!(item.nbt, None, "the zero tag byte carries no tree");
}

#[test]
fn the_empty_slot_stays_empty() {
    let item = read_slot(&mut Cursor::new(SLOT_EMPTY)).expect("the empty slot decodes");
    assert_eq!(item, None);
    assert_eq!(echo_slot(item.as_ref()), SLOT_EMPTY);
}

#[test]
fn the_echo_pair_reproduces_every_fixture() {
    for payload in [SLOT_WITH_COMPOUND_TAIL, SLOT_WITHOUT_TAIL, SLOT_EMPTY] {
        let item = read_slot(&mut Cursor::new(payload)).expect("the fixture decodes");
        assert_eq!(
            echo_slot(item.as_ref()),
            payload,
            "decode then write must reproduce the input bytes"
        );
    }
}

#[test]
fn a_tail_at_the_cap_is_kept_and_echoes() {
    assert_eq!(MAX_SLOT_NBT_BYTES, 65536);
    let payload = slot_with_tail_bytes(MAX_SLOT_NBT_BYTES);
    let item = decode_item(&payload);
    assert_eq!(
        item.nbt.as_ref().map(Vec::len),
        Some(MAX_SLOT_NBT_BYTES),
        "the cap itself is kept whole"
    );
    assert_eq!(echo_slot(Some(&item)), payload);
}

#[test]
fn a_tail_past_the_cap_still_refuses() {
    let payload = slot_with_tail_bytes(MAX_SLOT_NBT_BYTES + 1);
    let error = read_slot(&mut Cursor::new(payload.as_slice()))
        .expect_err("one byte past the cap is refused");
    assert!(
        error
            .to_string()
            .contains("entity slot NBT exceeds 65536 bytes"),
        "named refusal, saw: {error}"
    );
}

/// A written book's slot payload: item 387 x1, damage 0, and a compound tail
/// holding the title and the page list — the shape the source's book item
/// stores and the reader's display path walks.
const SLOT_BOOK: &[u8] = &[
    0x01, 0x83, // item id 387, the written book
    0x01, // count 1
    0x00, 0x00, // damage 0
    0x0a, 0x00, 0x00, // NBT: root compound, empty name
    0x08, 0x00, 0x05, b't', b'i', b't', b'l', b'e', // child: string "title"
    0x00, 0x05, b'T', b'i', b't', b'l', b'e', // payload "Title"
    0x09, 0x00, 0x05, b'p', b'a', b'g', b'e', b's', // child: list "pages"
    0x08, // element type: string
    0x00, 0x00, 0x00, 0x02, // two elements
    0x00, 0x05, b'h', b'e', b'l', b'l', b'o', // "hello"
    0x00, 0x0b, b's', b'e', b'c', b'o', b'n', b'd', b' ', b'p', b'a', b'g',
    b'e', // "second page"
    0x00, // end of the compound
];

/// One nested-compound tail: `depth` compounds, each holding the next, then
/// the terminators that close them all.
fn nested_compounds(depth: usize) -> Vec<u8> {
    let mut tail = vec![0x0a, 0x00, 0x00]; // root compound, empty name
    for _ in 1..depth {
        tail.extend_from_slice(&[0x0a, 0x00, 0x00]);
    }
    tail.extend(std::iter::repeat_n(0x00, depth));
    tail
}

#[test]
fn a_book_tail_reads_its_title_and_pages() {
    let item = decode_item(SLOT_BOOK);
    let tree =
        parse(item.nbt.as_deref().expect("the book keeps its tail")).expect("the book tail parses");
    assert_eq!(
        tree,
        NbtValue::Compound(vec![
            ("title".to_owned(), NbtValue::String("Title".to_owned())),
            (
                "pages".to_owned(),
                NbtValue::List(vec![
                    NbtValue::String("hello".to_owned()),
                    NbtValue::String("second page".to_owned()),
                ]),
            ),
        ])
    );
    assert_eq!(
        echo_slot(Some(&item)),
        SLOT_BOOK,
        "a book slot echoes byte-exact"
    );
}

#[test]
fn a_compound_keeps_the_wire_order() {
    // Keys out of alphabetical order: the tree keeps the wire's order, not a
    // lookup's, so a consumer can trust the sequence it walked.
    let data = [
        0x0a, 0x00, 0x00, // root compound, empty name
        0x01, 0x00, 0x01, b'z', 0x01, // byte "z" = 1
        0x01, 0x00, 0x01, b'a', 0x02, // byte "a" = 2
        0x00, // end of the compound
    ];
    assert_eq!(
        parse(&data),
        Ok(NbtValue::Compound(vec![
            ("z".to_owned(), NbtValue::Byte(1)),
            ("a".to_owned(), NbtValue::Byte(2)),
        ]))
    );
}

#[test]
fn a_deep_tail_frames_but_the_reader_refuses_it() {
    // The framing walk keeps the source's own 512-level guard, so seventeen
    // nested compounds land in the slot; the reader's display bound refuses
    // them — the seventeenth container would open past `MAX_NBT_DEPTH`.
    let mut payload = vec![0x01, 0x14, 0x01, 0x00, 0x00];
    payload.extend(nested_compounds(MAX_NBT_DEPTH + 1));
    let item = decode_item(&payload);
    let tail = item.nbt.as_deref().expect("the deep tail is kept");
    assert_eq!(parse(tail), Err(NbtError::Depth { max: MAX_NBT_DEPTH }));

    // Sixteen nest and walk.
    let mut ok_payload = vec![0x01, 0x14, 0x01, 0x00, 0x00];
    ok_payload.extend(nested_compounds(MAX_NBT_DEPTH));
    let item = decode_item(&ok_payload);
    let tail = item.nbt.as_deref().expect("the tail is kept");
    assert!(
        matches!(parse(tail), Ok(NbtValue::Compound(_))),
        "sixteen containers parse"
    );
}

#[test]
fn a_collection_past_the_cap_is_refused_before_it_sizes() {
    // A root byte array declaring 65537 entries with no payload bytes at
    // all: the count is refused before a byte of payload is read, so the
    // refusal is the count, never the truncation a sizing read would hit.
    let data = [
        0x07, 0x00, 0x00, // root: byte array, empty name
        0x00, 0x01, 0x00, 0x01, // 65537 entries
    ];
    assert_eq!(
        parse(&data),
        Err(NbtError::Count {
            declared: 65537,
            max: 65536,
        })
    );
}
