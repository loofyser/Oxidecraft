//! The entity wire surface: the spawn packets, the metadata block, and the
//! byte-to-unit conversions their handlers apply.
//!
//! The spawn type tables (`MobType`, `ObjectType`, `GlobalType`) and the
//! conversion helpers are pinned to `docs/research/protocol-47-reference.md`
//! §2.1 and §6.3; the conversions themselves are the expressions the source's
//! own spawn and velocity handlers apply (`NetHandlerPlayClient.java:300-302`,
//! `:410-411`, `:508`, `:536-537`), kept here so a decoded field is already in
//! the unit the rest of the client works in.

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES};
use oxide_proto::varint::read_varint;

use crate::PacketError;

/// Reads a wire angle byte as degrees.
///
/// The source's spawn handlers convert each angle byte with
/// `(byte * 360) / 256`: `NetHandlerPlayClient.java:536-537` for a spawning
/// player and `:410-411` for a spawning object. The byte is read unsigned
/// here, so the result lies in `[0, 360)`; where the source reads a signed
/// byte the two differ by exactly one whole turn.
pub fn read_angle(b: u8) -> f32 {
    b as f32 * 360.0 / 256.0
}

/// Reads a fixed-point coordinate as blocks.
///
/// The wire carries 1/32-block units: the spawn handlers divide each int by
/// `32.0` (`NetHandlerPlayClient.java:300-302`), and the relative-move handler
/// applies the same divisor to the accumulated server position (`:613-628`).
pub fn read_fixed_point(v: i32) -> f64 {
    v as f64 / 32.0
}

/// Reads a velocity short as blocks per tick.
///
/// The wire carries 1/8000-block units: the velocity handler divides each
/// short by `8000.0` (`NetHandlerPlayClient.java:508`), and the spawn-object
/// handler applies the same factor to a spawning object's momentum (`:439`).
pub fn read_velocity(v: i16) -> f64 {
    v as f64 / 8000.0
}

/// The spawn-mob type byte of clientbound Spawn Mob (0x0F).
///
/// One variant per spawn id of §6.3's roster (creeper `50` through guardian
/// `68`, pig `90` through rabbit `101`, villager `120`); the abstract class
/// markers `48` and `49` are not spawnable entities and have no variant.
/// `from_id` refuses every id the roster does not list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobType {
    /// Spawn id 50.
    Creeper,
    /// Spawn id 51.
    Skeleton,
    /// Spawn id 52.
    Spider,
    /// Spawn id 53.
    Giant,
    /// Spawn id 54.
    Zombie,
    /// Spawn id 55.
    Slime,
    /// Spawn id 56.
    Ghast,
    /// Spawn id 57.
    PigZombie,
    /// Spawn id 58.
    Enderman,
    /// Spawn id 59.
    CaveSpider,
    /// Spawn id 60.
    Silverfish,
    /// Spawn id 61.
    Blaze,
    /// Spawn id 62.
    LavaSlime,
    /// Spawn id 63.
    EnderDragon,
    /// Spawn id 64.
    WitherBoss,
    /// Spawn id 65.
    Bat,
    /// Spawn id 66.
    Witch,
    /// Spawn id 67.
    Endermite,
    /// Spawn id 68.
    Guardian,
    /// Spawn id 90.
    Pig,
    /// Spawn id 91.
    Sheep,
    /// Spawn id 92.
    Cow,
    /// Spawn id 93.
    Chicken,
    /// Spawn id 94.
    Squid,
    /// Spawn id 95.
    Wolf,
    /// Spawn id 96.
    MushroomCow,
    /// Spawn id 97.
    SnowMan,
    /// Spawn id 98.
    Ozelot,
    /// Spawn id 99.
    VillagerGolem,
    /// Spawn id 100.
    EntityHorse,
    /// Spawn id 101.
    Rabbit,
    /// Spawn id 120.
    Villager,
}

impl MobType {
    /// The mob a spawn id names, or `None` for an id outside the roster.
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            50 => Self::Creeper,
            51 => Self::Skeleton,
            52 => Self::Spider,
            53 => Self::Giant,
            54 => Self::Zombie,
            55 => Self::Slime,
            56 => Self::Ghast,
            57 => Self::PigZombie,
            58 => Self::Enderman,
            59 => Self::CaveSpider,
            60 => Self::Silverfish,
            61 => Self::Blaze,
            62 => Self::LavaSlime,
            63 => Self::EnderDragon,
            64 => Self::WitherBoss,
            65 => Self::Bat,
            66 => Self::Witch,
            67 => Self::Endermite,
            68 => Self::Guardian,
            90 => Self::Pig,
            91 => Self::Sheep,
            92 => Self::Cow,
            93 => Self::Chicken,
            94 => Self::Squid,
            95 => Self::Wolf,
            96 => Self::MushroomCow,
            97 => Self::SnowMan,
            98 => Self::Ozelot,
            99 => Self::VillagerGolem,
            100 => Self::EntityHorse,
            101 => Self::Rabbit,
            120 => Self::Villager,
            _ => return None,
        })
    }

    /// The spawn id this mob rides as.
    pub fn id(self) -> u8 {
        match self {
            Self::Creeper => 50,
            Self::Skeleton => 51,
            Self::Spider => 52,
            Self::Giant => 53,
            Self::Zombie => 54,
            Self::Slime => 55,
            Self::Ghast => 56,
            Self::PigZombie => 57,
            Self::Enderman => 58,
            Self::CaveSpider => 59,
            Self::Silverfish => 60,
            Self::Blaze => 61,
            Self::LavaSlime => 62,
            Self::EnderDragon => 63,
            Self::WitherBoss => 64,
            Self::Bat => 65,
            Self::Witch => 66,
            Self::Endermite => 67,
            Self::Guardian => 68,
            Self::Pig => 90,
            Self::Sheep => 91,
            Self::Cow => 92,
            Self::Chicken => 93,
            Self::Squid => 94,
            Self::Wolf => 95,
            Self::MushroomCow => 96,
            Self::SnowMan => 97,
            Self::Ozelot => 98,
            Self::VillagerGolem => 99,
            Self::EntityHorse => 100,
            Self::Rabbit => 101,
            Self::Villager => 120,
        }
    }
}

/// The spawn-object type byte of clientbound Spawn Object (0x0E).
///
/// One variant per id of §6.3's object roster. Minecarts are the roster's
/// `10`/`11`/`12`: the vanilla client reads a cart's cargo from the packet's
/// `Data` field on id `10` (`NetHandlerPlayClient.java:305-308`; `0`
/// rideable, `1` chest, `2` furnace, and on), and the per-sub-type ids keep
/// their 1.8-era names here. `from_id` refuses every id the roster does not
/// list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    /// Spawn id 1.
    Boat,
    /// Spawn id 2.
    Item,
    /// Spawn id 10.
    Minecart,
    /// Spawn id 11, the storage cart.
    MinecartStorage,
    /// Spawn id 12, the powered cart.
    MinecartPowered,
    /// Spawn id 50.
    PrimedTnt,
    /// Spawn id 51.
    EnderCrystal,
    /// Spawn id 60.
    Arrow,
    /// Spawn id 61.
    Snowball,
    /// Spawn id 62.
    ThrownEgg,
    /// Spawn id 63, the ghast's fireball.
    Fireball,
    /// Spawn id 64, the blaze's fireball.
    SmallFireball,
    /// Spawn id 65.
    ThrownEnderpearl,
    /// Spawn id 66.
    WitherSkull,
    /// Spawn id 70.
    FallingSand,
    /// Spawn id 71.
    ItemFrame,
    /// Spawn id 72.
    EyeOfEnderSignal,
    /// Spawn id 73.
    ThrownPotion,
    /// Spawn id 74, the dragon egg's falling form.
    FallingDragonEgg,
    /// Spawn id 75.
    ThrownExpBottle,
    /// Spawn id 76.
    FireworksRocketEntity,
    /// Spawn id 77.
    LeashKnot,
    /// Spawn id 78.
    ArmorStand,
    /// Spawn id 90.
    FishHook,
}

impl ObjectType {
    /// The object a spawn id names, or `None` for an id outside the roster.
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            1 => Self::Boat,
            2 => Self::Item,
            10 => Self::Minecart,
            11 => Self::MinecartStorage,
            12 => Self::MinecartPowered,
            50 => Self::PrimedTnt,
            51 => Self::EnderCrystal,
            60 => Self::Arrow,
            61 => Self::Snowball,
            62 => Self::ThrownEgg,
            63 => Self::Fireball,
            64 => Self::SmallFireball,
            65 => Self::ThrownEnderpearl,
            66 => Self::WitherSkull,
            70 => Self::FallingSand,
            71 => Self::ItemFrame,
            72 => Self::EyeOfEnderSignal,
            73 => Self::ThrownPotion,
            74 => Self::FallingDragonEgg,
            75 => Self::ThrownExpBottle,
            76 => Self::FireworksRocketEntity,
            77 => Self::LeashKnot,
            78 => Self::ArmorStand,
            90 => Self::FishHook,
            _ => return None,
        })
    }

    /// The spawn id this object rides as.
    pub fn id(self) -> u8 {
        match self {
            Self::Boat => 1,
            Self::Item => 2,
            Self::Minecart => 10,
            Self::MinecartStorage => 11,
            Self::MinecartPowered => 12,
            Self::PrimedTnt => 50,
            Self::EnderCrystal => 51,
            Self::Arrow => 60,
            Self::Snowball => 61,
            Self::ThrownEgg => 62,
            Self::Fireball => 63,
            Self::SmallFireball => 64,
            Self::ThrownEnderpearl => 65,
            Self::WitherSkull => 66,
            Self::FallingSand => 70,
            Self::ItemFrame => 71,
            Self::EyeOfEnderSignal => 72,
            Self::ThrownPotion => 73,
            Self::FallingDragonEgg => 74,
            Self::ThrownExpBottle => 75,
            Self::FireworksRocketEntity => 76,
            Self::LeashKnot => 77,
            Self::ArmorStand => 78,
            Self::FishHook => 90,
        }
    }
}

/// The global-entity type byte of clientbound Spawn Global Entity (0x2C).
///
/// §6.3 carries no global roster beyond the packet row's lightning, so the
/// table is that one entry; `from_id` returns `None` for every other id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalType {
    /// Spawn id 1, lightning.
    Lightning,
}

impl GlobalType {
    /// The global entity a spawn id names, or `None` for an id outside the
    /// table.
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            1 => Self::Lightning,
            _ => return None,
        })
    }

    /// The spawn id this global entity rides as.
    pub fn id(self) -> u8 {
        match self {
            Self::Lightning => 1,
        }
    }
}

/// The cap on one metadata block's entries.
///
/// The source reads the block until its terminator with no ceiling of its
/// own; this client refuses a block that piles up more than this many
/// entries, so a hostile block cannot grow the list without bound.
pub const MAX_METADATA_ENTRIES: usize = 64;

/// The cap on the bytes of one slot's NBT tail.
///
/// The tail is skipped, never read into a value tree; the cap keeps that skip
/// bounded, and a slot whose tail would run past it is refused.
pub const MAX_SLOT_NBT_BYTES: usize = 65536;

/// One entity's metadata block: the wire's `(index, value)` pairs in order.
///
/// The block is a run of header bytes closed by the `0x7f` terminator
/// (`DataWatcher.readWatchedListFromPacketBuffer:303-308`). The same index
/// carries different tag types between entity classes, so the block stays a
/// flat list in wire order rather than a table keyed by index.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    /// The entries in wire order; an index can repeat when the block carries
    /// two values for it.
    pub entries: Vec<(u8, MetadataValue)>,
}

/// One metadata value, tagged as the wire carries it (§6.1's tag table).
#[derive(Debug, Clone, PartialEq)]
pub enum MetadataValue {
    /// Tag 0: a signed byte.
    Byte(i8),
    /// Tag 1: a signed short.
    Short(i16),
    /// Tag 2: a signed int.
    Int(i32),
    /// Tag 3: a float.
    Float(f32),
    /// Tag 4: a varint-length UTF-8 string.
    String(String),
    /// Tag 5: an item slot; `None` is the empty slot
    /// (`PacketBuffer.readItemStackFromBuffer:229-242`).
    Item(Option<MetadataItem>),
    /// Tag 6: a block position, three ints. No 1.8 entity class writes it
    /// (§6.1), but the wire shape still decodes.
    Position {
        /// The x coordinate.
        x: i32,
        /// The y coordinate.
        y: i32,
        /// The z coordinate.
        z: i32,
    },
    /// Tag 7: a rotation, three floats. Like the position triple, no 1.8
    /// entity class writes it, and the wire shape still decodes.
    Rotation {
        /// The pitch in degrees.
        pitch: f32,
        /// The yaw in degrees.
        yaw: f32,
        /// The roll in degrees.
        roll: f32,
    },
}

/// One slot's item data: the id, the stack count and the damage value.
///
/// The slot's NBT tail is skipped by the decoder rather than carried here:
/// nothing downstream of the spawn codecs reads item NBT yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataItem {
    /// The item id.
    pub id: i16,
    /// The stack count, as the wire's byte.
    pub count: u8,
    /// The item's damage or metadata value.
    pub damage: i16,
}

/// The metadata block's terminator (`DataWatcher.java:303-308`).
const METADATA_TERMINATOR: u8 = 0x7f;

/// The header's low five bits carry the entry index
/// (`DataWatcher.java:314-315`).
const METADATA_INDEX_MASK: u8 = 0x1f;

/// The shift that lifts the value's tag out of a metadata header.
const METADATA_TAG_SHIFT: u32 = 5;

/// Decodes one entity metadata block.
///
/// Entries are kept in wire order; the decode is refused past
/// [`MAX_METADATA_ENTRIES`] entries, and every payload rides the crate's own
/// checks — the string rule, the slot shape, and the slot-NBT skip.
fn read_metadata(cursor: &mut Cursor<&[u8]>) -> Result<Metadata, PacketError> {
    let mut entries = Vec::new();
    loop {
        let header = codec::read_u8(&mut *cursor)?;
        if header == METADATA_TERMINATOR {
            return Ok(Metadata { entries });
        }
        if entries.len() >= MAX_METADATA_ENTRIES {
            return Err(PacketError::Codec(codec::CodecError::Io(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "entity metadata block carries more than {MAX_METADATA_ENTRIES} entries"
                    ),
                ),
            )));
        }
        let index = header & METADATA_INDEX_MASK;
        let value = read_metadata_value(header >> METADATA_TAG_SHIFT, cursor)?;
        entries.push((index, value));
    }
}

/// Decodes one metadata value: the payload the header's tag selects.
fn read_metadata_value(tag: u8, cursor: &mut Cursor<&[u8]>) -> Result<MetadataValue, PacketError> {
    match tag {
        0 => Ok(MetadataValue::Byte(codec::read_u8(&mut *cursor)? as i8)),
        1 => Ok(MetadataValue::Short(codec::read_i16(&mut *cursor)?)),
        2 => Ok(MetadataValue::Int(codec::read_i32(&mut *cursor)?)),
        3 => Ok(MetadataValue::Float(codec::read_f32(&mut *cursor)?)),
        4 => Ok(MetadataValue::String(codec::read_string(
            &mut *cursor,
            MAX_STRING_BYTES,
        )?)),
        5 => Ok(MetadataValue::Item(read_slot(cursor)?)),
        6 => Ok(MetadataValue::Position {
            x: codec::read_i32(&mut *cursor)?,
            y: codec::read_i32(&mut *cursor)?,
            z: codec::read_i32(&mut *cursor)?,
        }),
        7 => Ok(MetadataValue::Rotation {
            pitch: codec::read_f32(&mut *cursor)?,
            yaw: codec::read_f32(&mut *cursor)?,
            roll: codec::read_f32(&mut *cursor)?,
        }),
        other => Err(unknown_metadata_tag(other)),
    }
}

/// The refusal for a metadata tag outside the wire's eight.
///
/// The header's three-bit field cannot produce one, but the value carries a
/// whole byte into this boundary; a widened value is refused by name rather
/// than guessed at.
fn unknown_metadata_tag(tag: u8) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("unknown entity metadata tag type {tag}"),
    )))
}

/// Decodes one slot: item id, stack count, damage, then the NBT tail.
///
/// A negative id is the empty slot and closes the value
/// (`PacketBuffer.readItemStackFromBuffer:229-242`); otherwise the tail is
/// consumed by [`skip_slot_nbt`], which builds no value for it.
fn read_slot(cursor: &mut Cursor<&[u8]>) -> Result<Option<MetadataItem>, PacketError> {
    let id = codec::read_i16(&mut *cursor)?;
    if id < 0 {
        return Ok(None);
    }
    let count = codec::read_u8(&mut *cursor)?;
    let damage = codec::read_i16(&mut *cursor)?;
    skip_slot_nbt(cursor)?;
    Ok(Some(MetadataItem { id, count, damage }))
}

/// The depth past which the source's own NBT readers refuse to descend
/// (`NBTTagList.java`, `NBTTagCompound.java`: a payload read above 512).
const NBT_MAX_DEPTH: usize = 512;

/// One open container in the NBT walk.
#[derive(Clone, Copy)]
enum NbtFrame {
    /// A compound payload: children arrive until a zero tag id closes it.
    Compound {
        /// The depth of this payload, the root's at zero.
        depth: usize,
    },
    /// A list payload: `remaining` uniform element payloads still follow.
    List {
        /// The element type every remaining element carries.
        element: u8,
        /// How many elements still follow.
        remaining: usize,
        /// The depth of this payload, the root's at zero.
        depth: usize,
    },
}

/// Skips one slot's NBT tail, if there is one.
///
/// `PacketBuffer.readNBTTagCompoundFromBuffer:257-271` reads a zero byte as
/// "no data"; any other byte starts a full named tag that closes the slot's
/// payload. The tail is consumed as framing only — no value is ever built:
/// tag ids drive a walk that keeps its own stack, every length is checked
/// against the remaining body and [`MAX_SLOT_NBT_BYTES`] before a byte moves,
/// and a tag id or shape that cannot be framed is refused rather than guessed
/// at.
fn skip_slot_nbt(cursor: &mut Cursor<&[u8]>) -> Result<(), PacketError> {
    let mut budget = MAX_SLOT_NBT_BYTES;
    let root = read_nbt_byte(cursor, &mut budget)?;
    if root == 0 {
        return Ok(());
    }
    read_nbt_name(cursor, &mut budget)?;
    let mut stack: Vec<NbtFrame> = Vec::new();
    let mut pending = Some((root, 0usize));
    loop {
        if let Some((tag, depth)) = pending.take() {
            skip_nbt_payload(cursor, &mut budget, tag, depth, &mut stack)?;
            continue;
        }
        let Some(top) = stack.last().copied() else {
            return Ok(());
        };
        match top {
            NbtFrame::Compound { depth } => {
                let child = read_nbt_byte(cursor, &mut budget)?;
                if child == 0 {
                    stack.pop();
                } else {
                    read_nbt_name(cursor, &mut budget)?;
                    pending = Some((child, depth + 1));
                }
            }
            NbtFrame::List {
                element,
                remaining,
                depth,
            } => {
                if remaining == 0 {
                    stack.pop();
                } else {
                    stack.pop();
                    stack.push(NbtFrame::List {
                        element,
                        remaining: remaining - 1,
                        depth,
                    });
                    pending = Some((element, depth + 1));
                }
            }
        }
    }
}

/// Consumes one NBT payload — everything after its header — against the
/// budget, pushing the frame it opens when the tag is a container.
fn skip_nbt_payload(
    cursor: &mut Cursor<&[u8]>,
    budget: &mut usize,
    tag: u8,
    depth: usize,
    stack: &mut Vec<NbtFrame>,
) -> Result<(), PacketError> {
    match tag {
        // The fixed scalars: byte, short, int, long, float and double.
        1 => skip_nbt_bytes(cursor, budget, 1),
        2 => skip_nbt_bytes(cursor, budget, 2),
        3 => skip_nbt_bytes(cursor, budget, 4),
        4 => skip_nbt_bytes(cursor, budget, 8),
        5 => skip_nbt_bytes(cursor, budget, 4),
        6 => skip_nbt_bytes(cursor, budget, 8),
        // A byte array: a four-byte length, then that many bytes.
        7 => {
            let len = read_nbt_length(cursor, budget)?;
            skip_nbt_bytes(cursor, budget, len)
        }
        // A string: a two-byte length, then that many bytes.
        8 => {
            let len = read_nbt_u16(cursor, budget)?;
            skip_nbt_bytes(cursor, budget, len)
        }
        // A list: an element type and a four-byte count, then that many
        // element payloads; the elements carry no header of their own.
        9 => {
            if depth > NBT_MAX_DEPTH {
                return Err(nbt_too_deep());
            }
            let element = read_nbt_byte(cursor, budget)?;
            let count = read_nbt_length(cursor, budget)?;
            if element == 0 && count > 0 {
                return Err(nbt_error(
                    "NBT list counts elements with no element type".to_owned(),
                ));
            }
            stack.push(NbtFrame::List {
                element,
                remaining: count,
                depth,
            });
            Ok(())
        }
        // A compound: children arrive until a zero tag id closes it.
        10 => {
            if depth > NBT_MAX_DEPTH {
                return Err(nbt_too_deep());
            }
            stack.push(NbtFrame::Compound { depth });
            Ok(())
        }
        // An int array: a four-byte length, then four bytes per element.
        11 => {
            let len = read_nbt_length(cursor, budget)?;
            let bytes = len.checked_mul(4).ok_or_else(nbt_overflow)?;
            skip_nbt_bytes(cursor, budget, bytes)
        }
        other => Err(unknown_nbt_tag(other)),
    }
}

/// Reads one NBT framing byte against the budget.
fn read_nbt_byte(cursor: &mut Cursor<&[u8]>, budget: &mut usize) -> Result<u8, PacketError> {
    if *budget < 1 {
        return Err(nbt_over_cap());
    }
    let byte = codec::read_u8(&mut *cursor)?;
    *budget -= 1;
    Ok(byte)
}

/// Reads an NBT name: a two-byte length, then that many bytes, skipped.
fn read_nbt_name(cursor: &mut Cursor<&[u8]>, budget: &mut usize) -> Result<(), PacketError> {
    let len = read_nbt_u16(cursor, budget)?;
    skip_nbt_bytes(cursor, budget, len)
}

/// Reads a two-byte NBT length against the budget.
fn read_nbt_u16(cursor: &mut Cursor<&[u8]>, budget: &mut usize) -> Result<usize, PacketError> {
    if *budget < 2 {
        return Err(nbt_over_cap());
    }
    let value = codec::read_u16(&mut *cursor)?;
    *budget -= 2;
    Ok(value as usize)
}

/// Reads a four-byte NBT length against the budget, refusing a negative one.
fn read_nbt_length(cursor: &mut Cursor<&[u8]>, budget: &mut usize) -> Result<usize, PacketError> {
    if *budget < 4 {
        return Err(nbt_over_cap());
    }
    let len = codec::read_i32(&mut *cursor)?;
    *budget -= 4;
    usize::try_from(len).map_err(|_| PacketError::Codec(codec::CodecError::NegativeLength(len)))
}

/// Skips `n` bytes of NBT payload against both the budget and the body.
///
/// Nothing is materialized and the checks run before the cursor moves, so a
/// declared length can never outrun the body or the cap.
fn skip_nbt_bytes(
    cursor: &mut Cursor<&[u8]>,
    budget: &mut usize,
    n: usize,
) -> Result<(), PacketError> {
    if n > *budget {
        return Err(nbt_over_cap());
    }
    let start = cursor.position() as usize;
    let remaining = cursor.get_ref().len().saturating_sub(start);
    if n > remaining {
        return Err(PacketError::Codec(codec::CodecError::Io(
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the stream ended inside a value",
            ),
        )));
    }
    cursor.set_position(cursor.position() + n as u64);
    *budget -= n;
    Ok(())
}

/// The refusal for a slot tail past [`MAX_SLOT_NBT_BYTES`].
fn nbt_over_cap() -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("entity slot NBT exceeds {MAX_SLOT_NBT_BYTES} bytes"),
    )))
}

/// The refusal for an NBT tag id no table names.
fn unknown_nbt_tag(id: u8) -> PacketError {
    nbt_error(format!("unknown NBT tag id {id}"))
}

/// The refusal for a payload deeper than the source's own guard.
fn nbt_too_deep() -> PacketError {
    nbt_error(format!("NBT payload nests deeper than {NBT_MAX_DEPTH}"))
}

/// The refusal for an array length whose byte count cannot fit at all.
fn nbt_overflow() -> PacketError {
    nbt_error("NBT array length overflows its byte count".to_owned())
}

/// An NBT framing refusal.
fn nbt_error(message: String) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    )))
}

/// Clientbound Spawn Player (play id 0x0C).
///
/// A spawning player's identity and pose (`S0CPacketSpawnPlayer.readPacketData`).
/// The UUID is the wire's 16 bytes written out hyphenated, and the
/// coordinates are fixed-point (§1.1).
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnPlayer {
    /// The entity id the server will use for this player.
    pub entity_id: i32,
    /// The player's profile UUID, hyphenated.
    pub uuid: String,
    /// The x coordinate in blocks.
    pub x: f64,
    /// The y coordinate in blocks.
    pub y: f64,
    /// The z coordinate in blocks.
    pub z: f64,
    /// The yaw in degrees.
    pub yaw: f32,
    /// The pitch in degrees.
    pub pitch: f32,
    /// The item id in the player's held slot.
    pub current_item: i16,
    /// The player's metadata block.
    pub metadata: Metadata,
}

impl SpawnPlayer {
    /// The packet id.
    pub const ID: i32 = 0x0c;
}

/// Decodes the Spawn Player payload after the packet id.
pub fn decode_spawn_player(body: &[u8]) -> Result<SpawnPlayer, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let uuid = format_uuid(&codec::read_uuid(&mut cursor)?);
    let x = read_fixed_point(codec::read_i32(&mut cursor)?);
    let y = read_fixed_point(codec::read_i32(&mut cursor)?);
    let z = read_fixed_point(codec::read_i32(&mut cursor)?);
    let yaw = read_angle(codec::read_u8(&mut cursor)?);
    let pitch = read_angle(codec::read_u8(&mut cursor)?);
    let current_item = codec::read_i16(&mut cursor)?;
    let metadata = read_metadata(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnPlayer {
        entity_id,
        uuid,
        x,
        y,
        z,
        yaw,
        pitch,
        current_item,
        metadata,
    })
}

/// Clientbound Spawn Object (play id 0x0E).
///
/// A spawning non-mob entity (`S0EPacketSpawnObject.readPacketData`). The
/// `Data` field carries the object-specific payload (a falling block's id, a
/// cart's cargo, a projectile's thrower) and gates the trailing velocity
/// triple: the source reads those shorts only when `Data > 0`
/// (`S0EPacketSpawnObject.java:101`), and otherwise this client keeps the
/// velocity zeroed.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnObject {
    /// The entity id the server will use for this object.
    pub entity_id: i32,
    /// The object's type.
    pub kind: ObjectType,
    /// The x coordinate in blocks.
    pub x: f64,
    /// The y coordinate in blocks.
    pub y: f64,
    /// The z coordinate in blocks.
    pub z: f64,
    /// The pitch in degrees.
    pub pitch: f32,
    /// The yaw in degrees.
    pub yaw: f32,
    /// The object-specific payload; its meaning depends on `kind`.
    pub data: i32,
    /// The velocity in blocks per tick; zeroed when the wire omitted it.
    pub velocity: [f64; 3],
}

impl SpawnObject {
    /// The packet id.
    pub const ID: i32 = 0x0e;
}

/// Decodes the Spawn Object payload after the packet id.
pub fn decode_spawn_object(body: &[u8]) -> Result<SpawnObject, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let type_id = codec::read_u8(&mut cursor)?;
    let kind =
        ObjectType::from_id(type_id).ok_or_else(|| unknown_spawn_type("Spawn Object", type_id))?;
    let x = read_fixed_point(codec::read_i32(&mut cursor)?);
    let y = read_fixed_point(codec::read_i32(&mut cursor)?);
    let z = read_fixed_point(codec::read_i32(&mut cursor)?);
    let pitch = read_angle(codec::read_u8(&mut cursor)?);
    let yaw = read_angle(codec::read_u8(&mut cursor)?);
    let data = codec::read_i32(&mut cursor)?;
    let mut velocity = [0.0; 3];
    if data > 0 {
        velocity = [
            read_velocity(codec::read_i16(&mut cursor)?),
            read_velocity(codec::read_i16(&mut cursor)?),
            read_velocity(codec::read_i16(&mut cursor)?),
        ];
    }
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnObject {
        entity_id,
        kind,
        x,
        y,
        z,
        pitch,
        yaw,
        data,
        velocity,
    })
}

/// Clientbound Spawn Mob (play id 0x0F).
///
/// A spawning living entity (`S0FPacketSpawnMob.readPacketData`): position,
/// three angle bytes (yaw, pitch and the head's yaw that §2.1 labels
/// HeadPitch), a velocity triple that is always present, and a metadata
/// block.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnMob {
    /// The entity id the server will use for this mob.
    pub entity_id: i32,
    /// The mob's type.
    pub kind: MobType,
    /// The x coordinate in blocks.
    pub x: f64,
    /// The y coordinate in blocks.
    pub y: f64,
    /// The z coordinate in blocks.
    pub z: f64,
    /// The yaw in degrees.
    pub yaw: f32,
    /// The pitch in degrees.
    pub pitch: f32,
    /// The head's yaw in degrees.
    pub head_yaw: f32,
    /// The velocity in blocks per tick.
    pub velocity: [f64; 3],
    /// The mob's metadata block.
    pub metadata: Metadata,
}

impl SpawnMob {
    /// The packet id.
    pub const ID: i32 = 0x0f;
}

/// Decodes the Spawn Mob payload after the packet id.
pub fn decode_spawn_mob(body: &[u8]) -> Result<SpawnMob, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let type_id = codec::read_u8(&mut cursor)?;
    let kind = MobType::from_id(type_id).ok_or_else(|| unknown_spawn_type("Spawn Mob", type_id))?;
    let x = read_fixed_point(codec::read_i32(&mut cursor)?);
    let y = read_fixed_point(codec::read_i32(&mut cursor)?);
    let z = read_fixed_point(codec::read_i32(&mut cursor)?);
    let yaw = read_angle(codec::read_u8(&mut cursor)?);
    let pitch = read_angle(codec::read_u8(&mut cursor)?);
    let head_yaw = read_angle(codec::read_u8(&mut cursor)?);
    let velocity = [
        read_velocity(codec::read_i16(&mut cursor)?),
        read_velocity(codec::read_i16(&mut cursor)?),
        read_velocity(codec::read_i16(&mut cursor)?),
    ];
    let metadata = read_metadata(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnMob {
        entity_id,
        kind,
        x,
        y,
        z,
        yaw,
        pitch,
        head_yaw,
        velocity,
        metadata,
    })
}

/// The longest painting title the wire carries: the source reads the title
/// with a cap of 13, the length of its longest art name
/// (`EntityPainting.java:171`; `S10PacketSpawnPainting.readPacketData`).
const PAINTING_TITLE_MAX_BYTES: usize = 13;

/// Clientbound Spawn Painting (play id 0x10).
///
/// A hanging painting (`S10PacketSpawnPainting.readPacketData`): its art
/// title, the block it hangs at, and the horizontal facing §2.1 numbers
/// `0` −Z, `1` −X, `2` +Z, `3` +X.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnPainting {
    /// The entity id the server will use for this painting.
    pub entity_id: i32,
    /// The art's title (a name from the art table, at most 13 bytes).
    pub title: String,
    /// The block's x coordinate.
    pub x: i32,
    /// The block's y coordinate.
    pub y: i32,
    /// The block's z coordinate.
    pub z: i32,
    /// The wire's direction byte; the source masks it with `& 3` when it
    /// resolves the face (`NetHandlerPlayClient.handleSpawnPainting`).
    pub facing: u8,
}

impl SpawnPainting {
    /// The packet id.
    pub const ID: i32 = 0x10;
}

/// Decodes the Spawn Painting payload after the packet id.
pub fn decode_spawn_painting(body: &[u8]) -> Result<SpawnPainting, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let title = codec::read_string(&mut cursor, PAINTING_TITLE_MAX_BYTES)?;
    let (x, y, z) = read_position(&mut cursor)?;
    let facing = codec::read_u8(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnPainting {
        entity_id,
        title,
        x,
        y,
        z,
        facing,
    })
}

/// Clientbound Spawn Experience Orb (play id 0x11).
///
/// A spawning experience orb (`S11PacketSpawnExperienceOrb.readPacketData`):
/// its position and the experience it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnXpOrb {
    /// The entity id the server will use for this orb.
    pub entity_id: i32,
    /// The x coordinate in blocks.
    pub x: f64,
    /// The y coordinate in blocks.
    pub y: f64,
    /// The z coordinate in blocks.
    pub z: f64,
    /// The experience the orb carries.
    pub count: i16,
}

impl SpawnXpOrb {
    /// The packet id.
    pub const ID: i32 = 0x11;
}

/// Decodes the Spawn Experience Orb payload after the packet id.
pub fn decode_spawn_xp_orb(body: &[u8]) -> Result<SpawnXpOrb, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let x = read_fixed_point(codec::read_i32(&mut cursor)?);
    let y = read_fixed_point(codec::read_i32(&mut cursor)?);
    let z = read_fixed_point(codec::read_i32(&mut cursor)?);
    let count = codec::read_i16(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnXpOrb {
        entity_id,
        x,
        y,
        z,
        count,
    })
}

/// Clientbound Spawn Global Entity (play id 0x2C).
///
/// A spawning global entity — in 1.8.9, lightning
/// (`S2CPacketSpawnGlobalEntity.readPacketData`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnGlobal {
    /// The entity id the server will use for this entity.
    pub entity_id: i32,
    /// The global entity's type.
    pub kind: GlobalType,
    /// The x coordinate in blocks.
    pub x: f64,
    /// The y coordinate in blocks.
    pub y: f64,
    /// The z coordinate in blocks.
    pub z: f64,
}

impl SpawnGlobal {
    /// The packet id.
    pub const ID: i32 = 0x2c;
}

/// Decodes the Spawn Global Entity payload after the packet id.
pub fn decode_spawn_global(body: &[u8]) -> Result<SpawnGlobal, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let type_id = codec::read_u8(&mut cursor)?;
    let kind =
        GlobalType::from_id(type_id).ok_or_else(|| unknown_spawn_type("global entity", type_id))?;
    let x = read_fixed_point(codec::read_i32(&mut cursor)?);
    let y = read_fixed_point(codec::read_i32(&mut cursor)?);
    let z = read_fixed_point(codec::read_i32(&mut cursor)?);
    check_no_trailing(&cursor, body.len())?;
    Ok(SpawnGlobal {
        entity_id,
        kind,
        x,
        y,
        z,
    })
}

/// Clientbound Entity Metadata (play id 0x1C).
///
/// One entity's metadata block (§6), the only way most entity state — a
/// mob's health, a dropped item's stack, a horse's inventory — reaches the
/// client. The payload is the entity id and the block, nothing else
/// (`S1CPacketEntityMetadata.readPacketData`).
#[derive(Debug, Clone, PartialEq)]
pub struct EntityMetadata {
    /// The entity the block updates.
    pub entity_id: i32,
    /// The block, in wire order.
    pub metadata: Metadata,
}

impl EntityMetadata {
    /// The packet id.
    pub const ID: i32 = 0x1c;
}

/// Decodes the Entity Metadata payload after the packet id.
pub fn decode_entity_metadata(body: &[u8]) -> Result<EntityMetadata, PacketError> {
    let mut cursor = Cursor::new(body);
    let entity_id = read_varint(&mut cursor)?;
    let metadata = read_metadata(&mut cursor)?;
    check_no_trailing(&cursor, body.len())?;
    Ok(EntityMetadata {
        entity_id,
        metadata,
    })
}

/// The refusal for a spawn type byte no roster names.
fn unknown_spawn_type(packet: &str, id: u8) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("unknown {packet} type id {id}"),
    )))
}

/// Writes the play-state UUID's 16 bytes out hyphenated.
///
/// The wire carries a UUID as two big-endian longs (`PacketBuffer.readUuid`);
/// this is the same lowercase hyphenated text `UUID.toString` renders.
fn format_uuid(bytes: &[u8; 16]) -> String {
    const HEX: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(HEX[(byte >> 4) as usize]);
        out.push(HEX[(byte & 0x0f) as usize]);
    }
    out
}

/// Reads a Location Position: x, y and z packed into one big-endian `i64`.
///
/// The packing is
/// `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)` — x and
/// z are 26 signed bits, y is 12 — and the read mirrors `BlockPos.fromLong`
/// (`util/BlockPos.java:208-214`): each field is shifted into the sign
/// position and back, which sign-extends it. This is 1.8.9's own packing;
/// later versions reordered it, so it must not be "updated" from modern
/// documentation.
fn read_position(cursor: &mut Cursor<&[u8]>) -> Result<(i32, i32, i32), PacketError> {
    let raw = codec::read_i64(&mut *cursor)?;
    let x = (raw >> 38) as i32;
    let y = (raw << 26 >> 52) as i32;
    let z = (raw << 38 >> 38) as i32;
    Ok((x, y, z))
}

/// Refuses a payload that decoded with bytes to spare.
fn check_no_trailing(cursor: &Cursor<&[u8]>, len: usize) -> Result<(), PacketError> {
    let consumed = cursor.position() as usize;
    let remaining = len.saturating_sub(consumed);
    if remaining > 0 {
        return Err(PacketError::Trailing(remaining));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Fixed-literal tests for the conversion helpers and the metadata block:
    //! every expected value is the source's own arithmetic on a hand-picked
    //! literal, never rebuilt with the helper under test.

    use std::io::Cursor;

    use super::{
        MAX_METADATA_ENTRIES, MAX_SLOT_NBT_BYTES, Metadata, MetadataItem, MetadataValue,
        read_angle, read_fixed_point, read_metadata, read_metadata_value, read_velocity,
    };
    use crate::PacketError;

    /// Decodes one metadata block from a literal payload.
    fn decode_metadata(payload: &[u8]) -> Result<Metadata, PacketError> {
        read_metadata(&mut Cursor::new(payload))
    }

    #[test]
    fn read_angle_converts_the_byte_angle() {
        // `handleSpawnPlayer` converts yaw and pitch with
        // `(byte * 360) / 256` (`NetHandlerPlayClient.java:536-537`), and the
        // spawn-object handler applies the same expression (`:410-411`).
        assert_eq!(read_angle(0x00), 0.0, "north is zero");
        assert_eq!(read_angle(0x40), 90.0, "a quarter turn");
        assert_eq!(read_angle(0xc0), 270.0, "three quarters");
        assert_eq!(read_angle(0x01), 1.40625, "one byte step");
    }

    #[test]
    fn read_fixed_point_divides_by_32() {
        // The spawn handlers read fixed-point coordinates as `int / 32.0`
        // (`NetHandlerPlayClient.java:300-302`).
        assert_eq!(read_fixed_point(1), 0.03125, "one thirty-second");
        assert_eq!(read_fixed_point(-32), -1.0, "a whole negative block");
        assert_eq!(read_fixed_point(96), 3.0, "three blocks");
    }

    #[test]
    fn read_velocity_divides_by_8000() {
        // The velocity handler reads each short as `short / 8000.0`
        // (`NetHandlerPlayClient.java:508`).
        assert_eq!(read_velocity(8000), 1.0, "one block per tick");
        assert_eq!(read_velocity(-4000), -0.5, "half a block backwards");
        assert_eq!(read_velocity(1), 1.0 / 8000.0, "the smallest step");
    }

    #[test]
    fn metadata_decodes_every_value_type() {
        // One entry per tag type, each at its own index. The header packs the
        // tag in the top three bits and the index in the low five
        // (`DataWatcher.java:314-315`), the slot is `PacketBuffer.java:229-256`,
        // and the position and rotation payloads are §6.1's three-int and
        // three-float rows.
        let payload = [
            0x00, 0xfe, // byte, index 0: -2
            0x21, 0x01, 0x23, // short, index 1: 291
            0x42, 0x00, 0x00, 0x2a, 0x2a, // int, index 2: 10794
            0x63, 0x3f, 0xc0, 0x00, 0x00, // float, index 3: 1.5
            0x84, 0x02, b'h', b'i', // string, index 4: "hi"
            0xa5, 0x01, 0x14, 0x01, 0x00, 0x00, 0x00, // slot, index 5: 276 x1, no NBT
            0xc6, 0x00, 0x00, 0x00, 0x01, 0xff, 0xff, 0xff, 0xfe, 0x00, 0x00, 0x00,
            0x03, // position, index 6: (1, -2, 3)
            0xe7, 0x3e, 0x80, 0x00, 0x00, 0xbf, 0x00, 0x00, 0x00, 0x3f, 0x40, 0x00,
            0x00, // rotation, index 7: (0.25, -0.5, 0.75)
            0x7f, // the list terminator (`DataWatcher.java:303-308`)
        ];
        let metadata = decode_metadata(&payload).expect("every tag decodes");
        assert_eq!(
            metadata.entries,
            vec![
                (0, MetadataValue::Byte(-2)),
                (1, MetadataValue::Short(291)),
                (2, MetadataValue::Int(0x2a2a)),
                (3, MetadataValue::Float(1.5)),
                (4, MetadataValue::String("hi".to_owned())),
                (
                    5,
                    MetadataValue::Item(Some(MetadataItem {
                        id: 276,
                        count: 1,
                        damage: 0,
                    })),
                ),
                (6, MetadataValue::Position { x: 1, y: -2, z: 3 },),
                (
                    7,
                    MetadataValue::Rotation {
                        pitch: 0.25,
                        yaw: -0.5,
                        roll: 0.75,
                    },
                ),
            ]
        );
    }

    #[test]
    fn metadata_accepts_the_empty_block() {
        // A terminator right away is the no-metadata case
        // (`DataWatcher.java:303-308`).
        let metadata = decode_metadata(&[0x7f]).expect("an empty block decodes");
        assert!(metadata.entries.is_empty());
    }

    #[test]
    fn metadata_requires_the_terminator() {
        // One byte entry and then the payload simply ends: the terminator is
        // nowhere, so the decode is refused rather than guessed at.
        assert!(decode_metadata(&[0x00, 0x01]).is_err());
    }

    #[test]
    fn metadata_refuses_a_truncated_value() {
        // An int header with only two payload bytes behind it.
        assert!(decode_metadata(&[0x42, 0x00, 0x2a]).is_err());
        // A slot header cut off inside the item id.
        assert!(decode_metadata(&[0xa5, 0x01]).is_err());
    }

    #[test]
    fn metadata_refuses_more_than_the_entry_cap() {
        assert_eq!(MAX_METADATA_ENTRIES, 64);
        let mut at_cap = Vec::new();
        for _ in 0..64 {
            at_cap.extend_from_slice(&[0x00, 0x01]); // byte, index 0
        }
        at_cap.push(0x7f);
        let metadata = decode_metadata(&at_cap).expect("64 entries sit within the cap");
        assert_eq!(metadata.entries.len(), 64);

        let mut past_cap = Vec::new();
        for _ in 0..65 {
            past_cap.extend_from_slice(&[0x00, 0x01]);
        }
        past_cap.push(0x7f);
        assert!(decode_metadata(&past_cap).is_err());
    }

    #[test]
    fn metadata_strings_ride_the_length_rule() {
        // 32767 bytes of string is the largest the rule allows; the declaration
        // alone is refused one past it, before any byte is read.
        let mut at_cap = vec![0x80]; // string, index 0
        at_cap.extend_from_slice(&[0xff, 0xff, 0x01]); // varint 32767
        at_cap.extend(std::iter::repeat_n(b'a', 32767));
        at_cap.push(0x7f);
        let metadata = decode_metadata(&at_cap).expect("the longest string decodes");
        assert_eq!(
            metadata.entries,
            vec![(0, MetadataValue::String("a".repeat(32767)))]
        );

        let mut past_cap = vec![0x80, 0x80, 0x80, 0x02]; // varint 32768
        past_cap.extend(std::iter::repeat_n(b'a', 10));
        past_cap.push(0x7f);
        assert!(decode_metadata(&past_cap).is_err());
    }

    #[test]
    fn metadata_slots_ride_the_wire_shape_of_the_stack() {
        // An id below zero is the empty slot and consumes nothing more
        // (`PacketBuffer.java:229-242`); the following entry proves the
        // cursor stayed aligned.
        let payload = [
            0xa0, 0xff, 0xff, // slot, index 0: id -1, empty
            0x01, 0x01, // byte, index 1: 1
            0xa2, 0x01, 0x14, 0x02, 0x00, 0x2a, // slot, index 2: 276 x2 damage 42
            0x0a, 0x00, 0x00, // NBT: compound, empty name
            0x01, 0x00, 0x01, b'x', 0x7f, // child: byte "x" = 127
            0x00, // end of the compound
            0x7f, // terminator
        ];
        let metadata = decode_metadata(&payload).expect("both slot shapes decode");
        assert_eq!(
            metadata.entries,
            vec![
                (0, MetadataValue::Item(None)),
                (1, MetadataValue::Byte(1)),
                (
                    2,
                    MetadataValue::Item(Some(MetadataItem {
                        id: 276,
                        count: 2,
                        damage: 42,
                    })),
                ),
            ]
        );
    }

    #[test]
    fn a_tag_type_that_is_not_one_of_the_eight_is_refused() {
        // The three-bit header field cannot produce a ninth tag on the wire;
        // the refusal keeps a widened header from being guessed at, and names
        // the value it saw.
        let error =
            read_metadata_value(8, &mut Cursor::new(&[][..])).expect_err("a ninth tag is refused");
        assert!(
            error.to_string().contains("unknown entity metadata tag"),
            "named refusal, saw: {error}"
        );
    }

    #[test]
    fn nbt_tails_are_skipped_without_recursion() {
        // A slot whose NBT tail nests 500 lists deep must be skipped in
        // constant stack: the walk keeps its own frames.
        let mut payload = vec![0xa0, 0x01, 0x14, 0x01, 0x00, 0x00];
        payload.extend_from_slice(&[0x09, 0x00, 0x00]); // root: list, empty name
        for _ in 0..500 {
            payload.extend_from_slice(&[0x09, 0x00, 0x00, 0x00, 0x01]); // one nested list
        }
        payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00]); // innermost: empty list
        payload.push(0x7f);
        let metadata = decode_metadata(&payload).expect("deep nesting is skipped");
        assert_eq!(
            metadata.entries,
            vec![(
                0,
                MetadataValue::Item(Some(MetadataItem {
                    id: 276,
                    count: 1,
                    damage: 0,
                })),
            )]
        );
    }

    #[test]
    fn nbt_past_the_source_depth_guard_is_refused() {
        // `NBTTagCompound.java`/`NBTTagList.java` refuse any payload read at
        // a depth above 512; 600 levels of nesting trip the same guard.
        let mut payload = vec![0xa0, 0x01, 0x14, 0x01, 0x00, 0x00];
        payload.extend_from_slice(&[0x09, 0x00, 0x00]);
        for _ in 0..600 {
            payload.extend_from_slice(&[0x09, 0x00, 0x00, 0x00, 0x01]);
        }
        payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00]);
        payload.push(0x7f);
        assert!(decode_metadata(&payload).is_err());
    }

    #[test]
    fn nbt_refuses_an_unknown_tag_id() {
        // Root tag id 12 exists in no tag table; the tail cannot be skipped
        // past it, so the decode is refused and names what it saw.
        let payload = [
            0xa0, 0x01, 0x14, 0x01, 0x00, 0x00, // slot with an NBT tail
            0x0c, 0x00, 0x00, // root tag id 12
        ];
        let error = decode_metadata(&payload).expect_err("tag id 12 is refused");
        assert!(
            error.to_string().contains("unknown NBT tag"),
            "named refusal, saw: {error}"
        );
    }

    #[test]
    fn nbt_riding_the_byte_cap() {
        assert_eq!(MAX_SLOT_NBT_BYTES, 65536);
        // An NBT tail of exactly the cap: compound(3) + byte-array header(3)
        // + length(4) + 65525 payload bytes + end(1) = 65536.
        let mut payload = vec![0xa0, 0x01, 0x14, 0x01, 0x00, 0x00];
        payload.extend_from_slice(&[0x0a, 0x00, 0x00]);
        payload.extend_from_slice(&[0x07, 0x00, 0x00]);
        payload.extend_from_slice(&65525i32.to_be_bytes());
        payload.extend(std::iter::repeat_n(0u8, 65525));
        payload.push(0x00); // end of the compound
        payload.push(0x7f);
        assert!(decode_metadata(&payload).is_ok(), "the cap itself is fine");

        // One byte more must refuse.
        let mut past_cap = vec![0xa0, 0x01, 0x14, 0x01, 0x00, 0x00];
        past_cap.extend_from_slice(&[0x0a, 0x00, 0x00]);
        past_cap.extend_from_slice(&[0x07, 0x00, 0x00]);
        past_cap.extend_from_slice(&65526i32.to_be_bytes());
        past_cap.extend(std::iter::repeat_n(0u8, 65526));
        past_cap.push(0x00);
        past_cap.push(0x7f);
        assert!(
            decode_metadata(&past_cap).is_err(),
            "one past the cap is refused"
        );
    }
}
