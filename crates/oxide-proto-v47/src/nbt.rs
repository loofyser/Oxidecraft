//! The bounded read-only NBT reader: a slot payload's captured tail walked
//! into a value tree for display.
//!
//! The tail the entity codecs capture
//! ([`crate::entity::MetadataItem::nbt`](crate::entity::MetadataItem)) is a
//! full named tag — the shape `PacketBuffer.readNBTTagCompoundFromBuffer:213-227`
//! hands to `CompressedStreamTools.read` — so [`parse`] reads the root tag's
//! id, its name and its payload. Every declared length is checked against a
//! cap and the remaining bytes before a byte moves or an allocation is sized,
//! so a hostile tail cannot make the reader loop or allocate without bound,
//! and a shape that cannot be framed is refused with a named [`NbtError`]
//! rather than guessed at. The caller bounds the input: a slot tail is at
//! most [`MAX_SLOT_NBT_BYTES`](crate::entity::MAX_SLOT_NBT_BYTES) bytes,
//! captured by the entity codecs, and this reader holds no byte budget of its
//! own — it refuses to read past the slice it is given.

/// The cap on how deeply container payloads nest.
///
/// The source's own readers refuse a payload read at a depth above 512, the
/// root's payload at zero (`NBTTagCompound.java:37-40`,
/// `NBTTagList.java:48-51`). This client reads NBT only for display —
/// tooltips and book pages — where the deepest tree vanilla writes is a
/// handful of compounds, so the same root-at-zero count carries a much
/// tighter bound (the M5 plan's Task 1 cap): a container that would open at
/// depth sixteen is refused, so sixteen nested containers parse and the
/// seventeenth does not.
pub const MAX_NBT_DEPTH: usize = 16;

/// The cap on one string's declared byte length.
///
/// The source's `readUTF` caps nothing (`NBTTagString.read:35-40`), and the
/// wire's two-byte length tops out one byte under this bound; the check
/// keeps declared lengths inside the reader's own arithmetic. Both sides of
/// the boundary are pinned in this module's tests (the wire cannot reach
/// the far side, so the pin is against the check itself).
pub const MAX_NBT_STRING_BYTES: usize = 65536;

/// The cap on one collection's declared entry count: a list's elements, a
/// byte array's bytes, an int array's ints.
///
/// The count is checked before it sizes a loop or an allocation, so a
/// hostile tail cannot make the reader work or allocate past this bound (the
/// M5 plan's Task 1 cap); a caller's tail is itself bounded by
/// [`MAX_SLOT_NBT_BYTES`](crate::entity::MAX_SLOT_NBT_BYTES).
pub const MAX_NBT_COLLECTION_LEN: usize = 65536;

/// One NBT tag's value, as this milestone's reader reads it.
///
/// One variant per value tag the wire's table carries (`NBTBase.java:9`):
/// the source's ids `1..=11`, in order. The end tag (`0`) is a terminator,
/// not a value, and is never produced here.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtValue {
    /// Tag 1: a signed byte.
    Byte(i8),
    /// Tag 2: a signed big-endian short.
    Short(i16),
    /// Tag 3: a signed big-endian int.
    Int(i32),
    /// Tag 4: a signed big-endian long.
    Long(i64),
    /// Tag 5: a big-endian float.
    Float(f32),
    /// Tag 6: a big-endian double.
    Double(f64),
    /// Tag 7: a byte array.
    ByteArray(Vec<u8>),
    /// Tag 8: a modified-UTF-8 string, decoded as UTF-8 lossily (see
    /// [`parse`]'s divergence note).
    String(String),
    /// Tag 9: a list of same-typed elements, in wire order.
    List(Vec<NbtValue>),
    /// Tag 10: named children in wire order, the order preserved as a
    /// sequence because the wire may repeat a name.
    Compound(Vec<(String, NbtValue)>),
    /// Tag 11: an int array.
    IntArray(Vec<i32>),
}

/// A refusal from [`parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NbtError {
    /// The tail opens with the end tag (`0`): there is no root tag to read.
    #[error("the NBT tail opens with the end tag and carries no root")]
    NoRoot,
    /// A tag id the wire's table does not name (`1..=11`).
    #[error("unknown NBT tag id {id}")]
    UnknownTag {
        /// The unidentified tag id.
        id: u8,
    },
    /// The payload ended before the tag it declared was complete.
    #[error("the NBT payload ends inside a tag")]
    Truncated,
    /// A container payload would open past [`MAX_NBT_DEPTH`].
    #[error("the NBT payload nests deeper than {max} containers")]
    Depth {
        /// The container depth cap.
        max: usize,
    },
    /// A declared entry count is negative or past
    /// [`MAX_NBT_COLLECTION_LEN`].
    #[error("an NBT collection declares {declared} entries, outside 0..={max}")]
    Count {
        /// The declared entry count.
        declared: i64,
        /// The entry cap.
        max: usize,
    },
    /// A declared string byte length is past [`MAX_NBT_STRING_BYTES`].
    #[error("an NBT string declares {declared} bytes, past the {max} cap")]
    StringLength {
        /// The declared byte length.
        declared: usize,
        /// The byte cap.
        max: usize,
    },
    /// A list counts elements while naming no element type
    /// (`NBTTagList.read:57-59` refuses the shape).
    #[error("an NBT list counts elements with no element type")]
    ListWithoutType,
}

/// Reads one tag — the root — from a slot's captured NBT tail.
///
/// `data` is the tail exactly as the entity codecs captured it: the root
/// tag's id, its name and its payload. The returned value is the root
/// payload; the root name is framing and is dropped. Every read is checked
/// against the slice before the cursor moves and every declared length
/// against its cap before it sizes a loop or an allocation; a shape that
/// cannot be framed is refused by name.
///
/// Strings decode as UTF-8 lossily. The source's `readUTF` is *modified*
/// UTF-8, so two shapes it reads diverge here, each recorded in this
/// module's tests: an embedded NUL (`C0 80`) and an astral-plane character
/// (a CESU-8 surrogate pair) come out as replacement characters. The
/// divergence is read-only — the byte-exact echo path never re-encodes a
/// parsed string.
pub fn parse(data: &[u8]) -> Result<NbtValue, NbtError> {
    let mut reader = Reader { data, pos: 0 };
    let id = reader.u8()?;
    if id == 0 {
        return Err(NbtError::NoRoot);
    }
    let _root_name = reader.string()?;
    reader.payload(id, 0)
}

/// Checks a declared entry count against [`MAX_NBT_COLLECTION_LEN`].
fn check_count(declared: i32) -> Result<usize, NbtError> {
    if declared < 0 || declared as i64 > MAX_NBT_COLLECTION_LEN as i64 {
        return Err(NbtError::Count {
            declared: declared as i64,
            max: MAX_NBT_COLLECTION_LEN,
        });
    }
    Ok(declared as usize)
}

/// Checks a declared string byte length against [`MAX_NBT_STRING_BYTES`].
fn check_string_len(len: usize) -> Result<usize, NbtError> {
    if len > MAX_NBT_STRING_BYTES {
        return Err(NbtError::StringLength {
            declared: len,
            max: MAX_NBT_STRING_BYTES,
        });
    }
    Ok(len)
}

/// The reader's cursor: the slice and how much of it is consumed.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    /// Takes `n` bytes, refusing to read past the slice.
    fn take(&mut self, n: usize) -> Result<&[u8], NbtError> {
        let end = self.pos.checked_add(n).ok_or(NbtError::Truncated)?;
        let bytes = self.data.get(self.pos..end).ok_or(NbtError::Truncated)?;
        self.pos = end;
        Ok(bytes)
    }

    /// Reads one byte.
    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.take(1)?[0])
    }

    /// Reads a big-endian unsigned short.
    fn u16(&mut self) -> Result<u16, NbtError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a big-endian signed short.
    fn i16(&mut self) -> Result<i16, NbtError> {
        let bytes = self.take(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a big-endian signed int.
    fn i32(&mut self) -> Result<i32, NbtError> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a big-endian signed long.
    fn i64(&mut self) -> Result<i64, NbtError> {
        let bytes = self.take(8)?;
        Ok(i64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a big-endian float.
    fn f32(&mut self) -> Result<f32, NbtError> {
        let bytes = self.take(4)?;
        Ok(f32::from_bits(u32::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])))
    }

    /// Reads a big-endian double.
    fn f64(&mut self) -> Result<f64, NbtError> {
        let bytes = self.take(8)?;
        Ok(f64::from_bits(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])))
    }

    /// Reads a modified-UTF-8 string: a two-byte length, then its bytes,
    /// decoded lossily (see [`parse`]'s divergence note).
    fn string(&mut self) -> Result<String, NbtError> {
        let len = check_string_len(self.u16()? as usize)?;
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    /// Reads one tag's payload; `depth` is the payload's nesting level with
    /// the root's at zero, and a container that would open at
    /// [`MAX_NBT_DEPTH`] is refused before its contents are read.
    fn payload(&mut self, tag: u8, depth: usize) -> Result<NbtValue, NbtError> {
        match tag {
            1 => Ok(NbtValue::Byte(self.u8()? as i8)),
            2 => Ok(NbtValue::Short(self.i16()?)),
            3 => Ok(NbtValue::Int(self.i32()?)),
            4 => Ok(NbtValue::Long(self.i64()?)),
            5 => Ok(NbtValue::Float(self.f32()?)),
            6 => Ok(NbtValue::Double(self.f64()?)),
            7 => {
                let len = check_count(self.i32()?)?;
                Ok(NbtValue::ByteArray(self.take(len)?.to_vec()))
            }
            8 => Ok(NbtValue::String(self.string()?)),
            9 => {
                if depth >= MAX_NBT_DEPTH {
                    return Err(NbtError::Depth { max: MAX_NBT_DEPTH });
                }
                let element = self.u8()?;
                let count = check_count(self.i32()?)?;
                if element == 0 && count > 0 {
                    return Err(NbtError::ListWithoutType);
                }
                let mut values = Vec::new();
                for _ in 0..count {
                    values.push(self.payload(element, depth + 1)?);
                }
                Ok(NbtValue::List(values))
            }
            10 => {
                if depth >= MAX_NBT_DEPTH {
                    return Err(NbtError::Depth { max: MAX_NBT_DEPTH });
                }
                let mut children = Vec::new();
                loop {
                    let child = self.u8()?;
                    if child == 0 {
                        return Ok(NbtValue::Compound(children));
                    }
                    let name = self.string()?;
                    let value = self.payload(child, depth + 1)?;
                    children.push((name, value));
                }
            }
            11 => {
                let len = check_count(self.i32()?)?;
                let mut values = Vec::new();
                for _ in 0..len {
                    values.push(self.i32()?);
                }
                Ok(NbtValue::IntArray(values))
            }
            0 => Err(NbtError::NoRoot),
            other => Err(NbtError::UnknownTag { id: other }),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Fixed-literal tests for the reader: every expected value is hand-built
    //! from the wire shapes the source's own readers name
    //! (`NBTTagCompound.read:33-58`, `NBTTagList.read:44-74`,
    //! `NBTTagString.read:35-40`), never rebuilt with the reader under test.

    use super::{
        MAX_NBT_COLLECTION_LEN, MAX_NBT_DEPTH, MAX_NBT_STRING_BYTES, NbtError, NbtValue,
        check_count, check_string_len, parse,
    };

    /// `depth` compounds, each holding the next, closed by `depth`
    /// terminators; the root's id and name lead.
    fn nested_compounds(depth: usize) -> Vec<u8> {
        let mut data = vec![0x0a, 0x00, 0x00];
        for _ in 1..depth {
            data.extend_from_slice(&[0x0a, 0x00, 0x00]);
        }
        data.extend(std::iter::repeat_n(0x00, depth));
        data
    }

    #[test]
    fn the_caps_are_pinned() {
        assert_eq!(MAX_NBT_DEPTH, 16);
        assert_eq!(MAX_NBT_STRING_BYTES, 65536);
        assert_eq!(MAX_NBT_COLLECTION_LEN, 65536);
    }

    #[test]
    fn every_numeric_tag_reads_its_value() {
        let data = [
            0x0a, 0x00, 0x00, // root compound, empty name
            0x01, 0x00, 0x01, b'b', 0xff, // byte "b" = -1
            0x02, 0x00, 0x01, b's', 0xfe, 0xd4, // short "s" = -300
            0x03, 0x00, 0x01, b'i', 0xff, 0xff, 0xff, 0x9c, // int "i" = -100
            0x04, 0x00, 0x01, b'l', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x00, // long "l"
            0x05, 0x00, 0x01, b'f', 0x40, 0x20, 0x00, 0x00, // float "f" = 2.5
            0x06, 0x00, 0x01, b'd', 0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, // double "d"
            0x00, // end of the compound
        ];
        assert_eq!(
            parse(&data),
            Ok(NbtValue::Compound(vec![
                ("b".to_owned(), NbtValue::Byte(-1)),
                ("s".to_owned(), NbtValue::Short(-300)),
                ("i".to_owned(), NbtValue::Int(-100)),
                ("l".to_owned(), NbtValue::Long(4_294_967_296)),
                ("f".to_owned(), NbtValue::Float(2.5)),
                ("d".to_owned(), NbtValue::Double(1.0)),
            ]))
        );
    }

    #[test]
    fn a_byte_array_reads_its_bytes() {
        let data = [
            0x07, 0x00, 0x00, // root: byte array, empty name
            0x00, 0x00, 0x00, 0x03, // three bytes
            0x00, 0x7f, 0xff,
        ];
        assert_eq!(
            parse(&data),
            Ok(NbtValue::ByteArray(vec![0x00, 0x7f, 0xff]))
        );
    }

    #[test]
    fn an_int_array_reads_every_int() {
        let data = [
            0x0b, 0x00, 0x00, // root: int array, empty name
            0x00, 0x00, 0x00, 0x03, // three ints
            0xff, 0xff, 0xff, 0xff, // -1
            0x00, 0x00, 0x00, 0x00, // 0
            0x00, 0x00, 0x01, 0x00, // 256
        ];
        assert_eq!(parse(&data), Ok(NbtValue::IntArray(vec![-1, 0, 256])));
    }

    #[test]
    fn a_root_list_reads_its_elements() {
        let data = [
            0x09, 0x00, 0x00, // root: list, empty name
            0x03, // element type: int
            0x00, 0x00, 0x00, 0x02, // two elements
            0x00, 0x00, 0x00, 0x07, // 7
            0xff, 0xff, 0xff, 0xf9, // -7
        ];
        assert_eq!(
            parse(&data),
            Ok(NbtValue::List(vec![NbtValue::Int(7), NbtValue::Int(-7)]))
        );
    }

    #[test]
    fn a_root_name_is_framing() {
        // A non-empty root name is read and dropped: the value is the root
        // payload.
        let data = [0x0a, 0x00, 0x04, b'r', b'o', b'o', b't', 0x00];
        assert_eq!(parse(&data), Ok(NbtValue::Compound(Vec::new())));
    }

    #[test]
    fn an_empty_string_reads_as_empty() {
        let data = [0x08, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(parse(&data), Ok(NbtValue::String(String::new())));
    }

    #[test]
    fn strings_decode_as_modified_utf8_lossily() {
        // Plain two-byte UTF-8 passes through untouched.
        let accent = [0x08, 0x00, 0x00, 0x00, 0x02, 0xc3, 0xa9];
        assert_eq!(parse(&accent), Ok(NbtValue::String("é".to_owned())));

        // The source's `readUTF` is modified UTF-8: a NUL is `C0 80`, which
        // strict UTF-8 refuses — the recorded divergence, two replacement
        // characters where the source would have read one NUL.
        let nul = [0x08, 0x00, 0x00, 0x00, 0x02, 0xc0, 0x80];
        assert_eq!(
            parse(&nul),
            Ok(NbtValue::String("\u{fffd}\u{fffd}".to_owned()))
        );

        // An astral-plane character is a CESU-8 surrogate pair (`ED A0 BD ED
        // B8 80` is U+1F600): six replacement characters where the source
        // would have read the one astral character.
        let astral = [
            0x08, 0x00, 0x00, 0x00, 0x06, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80,
        ];
        assert_eq!(
            parse(&astral),
            Ok(NbtValue::String(
                "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}".to_owned()
            ))
        );
    }

    #[test]
    fn the_string_cap_pins_both_sides() {
        // The wire's two-byte length tops out one byte under the cap, so the
        // boundary itself is pinned against the check.
        assert_eq!(
            check_string_len(MAX_NBT_STRING_BYTES),
            Ok(MAX_NBT_STRING_BYTES)
        );
        assert_eq!(
            check_string_len(MAX_NBT_STRING_BYTES + 1),
            Err(NbtError::StringLength {
                declared: MAX_NBT_STRING_BYTES + 1,
                max: MAX_NBT_STRING_BYTES,
            })
        );
        // The largest string the wire can declare still reads.
        let mut data = vec![0x08, 0x00, 0x00, 0xff, 0xff];
        data.extend(std::iter::repeat_n(b'a', 65535));
        assert_eq!(parse(&data), Ok(NbtValue::String("a".repeat(65535))));
    }

    #[test]
    fn the_count_cap_pins_both_sides() {
        assert_eq!(
            check_count(MAX_NBT_COLLECTION_LEN as i32),
            Ok(MAX_NBT_COLLECTION_LEN)
        );
        assert_eq!(
            check_count(MAX_NBT_COLLECTION_LEN as i32 + 1),
            Err(NbtError::Count {
                declared: MAX_NBT_COLLECTION_LEN as i64 + 1,
                max: MAX_NBT_COLLECTION_LEN,
            })
        );
        assert_eq!(
            check_count(-1),
            Err(NbtError::Count {
                declared: -1,
                max: MAX_NBT_COLLECTION_LEN,
            })
        );
        // A byte array at the cap reads whole.
        let mut data = vec![0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00];
        data.extend(std::iter::repeat_n(0x5a, MAX_NBT_COLLECTION_LEN));
        assert_eq!(
            parse(&data),
            Ok(NbtValue::ByteArray(vec![0x5a; MAX_NBT_COLLECTION_LEN]))
        );
    }

    #[test]
    fn the_depth_cap_pins_both_sides() {
        assert!(
            matches!(
                parse(&nested_compounds(MAX_NBT_DEPTH)),
                Ok(NbtValue::Compound(_))
            ),
            "sixteen nested containers parse"
        );
        assert_eq!(
            parse(&nested_compounds(MAX_NBT_DEPTH + 1)),
            Err(NbtError::Depth { max: MAX_NBT_DEPTH })
        );
    }

    #[test]
    fn a_truncated_tree_is_refused_at_every_position() {
        // A tree that touches every read class: the root header, a scalar, a
        // byte array, a list, a string, an int array and a nested compound.
        // Cutting it anywhere must refuse as truncated — never panic, never
        // read past the slice, never succeed.
        let tree = [
            0x0a, 0x00, 0x00, // root compound, empty name
            0x01, 0x00, 0x01, b'b', 0x7f, // byte "b"
            0x07, 0x00, 0x01, b'a', 0x00, 0x00, 0x00, 0x02, 0x11, 0x22, // byte array "a"
            0x09, 0x00, 0x01, b'l', 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01,
            b'x', // list "l"
            0x0b, 0x00, 0x01, b'i', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x05, // int array
            0x0a, 0x00, 0x01, b'c', 0x00, // compound "c"
            0x00, // end of the root
        ];
        assert!(
            matches!(parse(&tree), Ok(NbtValue::Compound(_))),
            "the full tree parses"
        );
        for cut in 0..tree.len() {
            assert_eq!(
                parse(&tree[..cut]),
                Err(NbtError::Truncated),
                "cut at byte {cut}"
            );
        }
    }

    #[test]
    fn an_unknown_tag_id_is_refused_by_name() {
        assert_eq!(
            parse(&[0x0c, 0x00, 0x00]),
            Err(NbtError::UnknownTag { id: 12 })
        );
        // A nested unknown id too: a compound whose child header is tag 12.
        let nested = [0x0a, 0x00, 0x00, 0x0c, 0x00, 0x00];
        assert_eq!(parse(&nested), Err(NbtError::UnknownTag { id: 12 }));
    }

    #[test]
    fn the_end_tag_is_not_a_root() {
        assert_eq!(parse(&[]), Err(NbtError::Truncated));
        assert_eq!(parse(&[0x00]), Err(NbtError::NoRoot));
    }

    #[test]
    fn a_list_without_an_element_type_is_refused() {
        // `NBTTagList.read:57-59` refuses a list that counts elements with
        // no element type; the empty shape (type zero, count zero) is the
        // source's own empty list and reads.
        let hostile = [0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(parse(&hostile), Err(NbtError::ListWithoutType));
        let empty = [0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(parse(&empty), Ok(NbtValue::List(Vec::new())));
    }
}
