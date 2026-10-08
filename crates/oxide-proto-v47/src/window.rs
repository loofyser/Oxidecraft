//! The window and sign wire surface: the window family, the sign pair, the
//! experience update and the entity-effect pair, with the window-kind table
//! and the merchant offer list.
//!
//! The layouts are pinned to `docs/research/protocol-47-reference.md` §2.1 and
//! to the packet classes' own readers
//! (`S2DPacketOpenWindow.readPacketData:51-61`,
//! `S2EPacketCloseWindow.readPacketData:32-35`,
//! `S2FPacketSetSlot.readPacketData:37-42`,
//! `S30PacketWindowItems.readPacketData:34-43`,
//! `S31PacketWindowProperty.readPacketData:36-41`,
//! `S32PacketConfirmTransaction.readPacketData:36-41`,
//! `S33PacketUpdateSign.readPacketData:31-40`,
//! `S36PacketSignEditorOpen.readPacketData:33-36`,
//! `S1FPacketSetExperience.readPacketData:28-33`,
//! `S1DPacketEntityEffect.readPacketData:42-49`,
//! `S1EPacketRemoveEntityEffect.readPacketData:27-31`). Every decoder applies
//! the crate's trailing-byte rule; every declared count is checked against
//! the bytes that remain before it sizes a loop or an allocation, and a
//! payload that ends early is an error, never a panic.

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES};
use oxide_proto::varint::read_varint;

use crate::PacketError;
use crate::entity::{MetadataItem, read_slot};

/// The window type string of a horse's inventory window.
///
/// The horse carries no `IInteractionObject`; the server sends the literal
/// (`EntityPlayerMP.displayGUIHorse:831`), and the client tests the same
/// literal before reading the entity id (`NetHandlerPlayClient:1107`).
const ENTITY_HORSE_TYPE: &str = "EntityHorse";

/// The most bytes the window type string field may carry.
///
/// `S2DPacketOpenWindow.readPacketData:54` reads the type with
/// `readStringFromBuffer(32)`; this crate applies the source's cap as a byte
/// cap, as its other string fields do.
const WINDOW_TYPE_MAX_BYTES: usize = 32;

/// The fewest bytes one slot payload occupies: the empty slot's negative id
/// short (`PacketBuffer.readItemStackFromBuffer:257-271` returns early on a
/// negative id). Used only to bound a reservation against the remaining body.
const MIN_SLOT_BYTES: usize = 2;

/// The fewest bytes one merchant offer occupies: two empty slots (2 + 2), the
/// has-second and disabled booleans (1 + 1) and the two use ints (4 + 4)
/// (`MerchantRecipeList.readFromBuf:76-106`). Used only to bound a reservation
/// against the remaining body.
const MIN_OFFER_BYTES: usize = 14;

/// The most offers one merchant list may carry.
///
/// The source's reader loops on the count byte with no ceiling of its own
/// (`MerchantRecipeList.readFromBuf:79-103`); this client caps the list so a
/// hostile count cannot stretch the payload's work, and refuses one past the
/// cap. The cap is a fixed 128, well inside the byte's range.
pub const MAX_MERCHANT_OFFERS: usize = 128;

/// The window type a server's Open Window names.
///
/// One variant per type string the 1.8.9 server sends: every
/// `IInteractionObject` registration (`minecraft:chest` —
/// `TileEntityChest.java:495`, `EntityMinecartChest.java:62`;
/// `minecraft:crafting_table` — `BlockWorkbench.java:74`; `minecraft:furnace`
/// — `TileEntityFurnace.java:460`; `minecraft:dispenser` —
/// `TileEntityDispenser.java:236`; `minecraft:enchanting_table` —
/// `TileEntityEnchantmentTable.java:167`; `minecraft:brewing_stand` —
/// `TileEntityBrewingStand.java:405`; `minecraft:beacon` —
/// `TileEntityBeacon.java:415`; `minecraft:anvil` — `BlockAnvil.java:187`;
/// `minecraft:hopper` — `TileEntityHopper.java:723`,
/// `EntityMinecartHopper.java:231`; `minecraft:dropper` —
/// `TileEntityDropper.java:15`), the server's own sends — `minecraft:container`
/// (`EntityPlayerMP.java:795`), `minecraft:villager` (`:811`) and
/// `EntityHorse` (`:831`) — and the client's handling sites
/// (`NetHandlerPlayClient.handleOpenWindow:1097`, `:1102`, `:1107`). A string
/// the table does not list reads as [`Self::Unknown`]: the source's own
/// fallback opens a generic container for an unlisted type (`:1117-1126`), so
/// an unknown window still opens.
///
/// `minecraft:merchant` reads as [`Self::Villager`] as well — the merchant
/// window under its other name; the source's own string is
/// `minecraft:villager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// `minecraft:chest`: a chest's or a chest minecart's inventory.
    Chest,
    /// `minecraft:crafting_table`: a workbench's 3×3 grid.
    CraftingTable,
    /// `minecraft:furnace`: a furnace's burn and cook state.
    Furnace,
    /// `minecraft:dispenser`: a dispenser's nine slots.
    Dispenser,
    /// `minecraft:enchanting_table`: the enchantment screen.
    EnchantingTable,
    /// `minecraft:brewing_stand`: the brewing stand's four slots.
    BrewingStand,
    /// `minecraft:villager` (or `minecraft:merchant`): the trade list.
    Villager,
    /// `minecraft:beacon`: the beacon's payment slot.
    Beacon,
    /// `minecraft:anvil`: the anvil's two input slots and the result.
    Anvil,
    /// `minecraft:hopper`: a hopper's or a hopper minecart's five slots.
    Hopper,
    /// `minecraft:dropper`: a dropper's nine slots.
    Dropper,
    /// `EntityHorse`: a horse's saddle-and-armour inventory; the only window
    /// whose type string carries an entity id (`EntityPlayerMP.java:831`).
    EntityHorse,
    /// `minecraft:container`: a plain inventory window the server opened
    /// without a named kind (`EntityPlayerMP.java:795`).
    Container,
    /// A type string outside the table.
    Unknown,
}

impl WindowKind {
    /// Names the kind a window type string carries.
    ///
    /// The comparison is the source's own: an exact, case-sensitive string
    /// test (`NetHandlerPlayClient.handleOpenWindow:1097-1107`); two type
    /// strings read as [`Self::Villager`] and every other unlisted string
    /// reads as [`Self::Unknown`].
    pub fn from_type(kind: &str) -> Self {
        match kind {
            "minecraft:chest" => Self::Chest,
            "minecraft:crafting_table" => Self::CraftingTable,
            "minecraft:furnace" => Self::Furnace,
            "minecraft:dispenser" => Self::Dispenser,
            "minecraft:enchanting_table" => Self::EnchantingTable,
            "minecraft:brewing_stand" => Self::BrewingStand,
            "minecraft:villager" | "minecraft:merchant" => Self::Villager,
            "minecraft:beacon" => Self::Beacon,
            "minecraft:anvil" => Self::Anvil,
            "minecraft:hopper" => Self::Hopper,
            "minecraft:dropper" => Self::Dropper,
            ENTITY_HORSE_TYPE => Self::EntityHorse,
            "minecraft:container" => Self::Container,
            _ => Self::Unknown,
        }
    }

    /// Names the wire string this kind carries, or `None` for [`Self::Unknown`].
    ///
    /// [`Self::Villager`] names its source string `minecraft:villager`; an
    /// unknown kind names nothing, as no string reads as it.
    pub fn as_type(self) -> Option<&'static str> {
        match self {
            Self::Chest => Some("minecraft:chest"),
            Self::CraftingTable => Some("minecraft:crafting_table"),
            Self::Furnace => Some("minecraft:furnace"),
            Self::Dispenser => Some("minecraft:dispenser"),
            Self::EnchantingTable => Some("minecraft:enchanting_table"),
            Self::BrewingStand => Some("minecraft:brewing_stand"),
            Self::Villager => Some("minecraft:villager"),
            Self::Beacon => Some("minecraft:beacon"),
            Self::Anvil => Some("minecraft:anvil"),
            Self::Hopper => Some("minecraft:hopper"),
            Self::Dropper => Some("minecraft:dropper"),
            Self::EntityHorse => Some(ENTITY_HORSE_TYPE),
            Self::Container => Some("minecraft:container"),
            Self::Unknown => None,
        }
    }
}

/// Clientbound Open Window (play id 0x2D).
///
/// `S2DPacketOpenWindow.readPacketData:51-61`: the window id, the type
/// string, the title as a chat component and the slot count; the entity id
/// follows only when the type is `EntityHorse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// The window id the server assigned.
    pub window_id: u8,
    /// The window's kind, from its type string.
    pub kind: WindowKind,
    /// The title as chat JSON, exactly as sent.
    pub title: String,
    /// The window's slot count.
    pub slot_count: u8,
    /// The horse's entity id; `None` for every other kind.
    pub entity_id: Option<i32>,
}

impl OpenWindow {
    /// The packet id.
    pub const ID: i32 = 0x2d;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let window_id = codec::read_u8(&mut cursor)?;
        let kind = codec::read_string(&mut cursor, WINDOW_TYPE_MAX_BYTES)?;
        let title = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
        let slot_count = codec::read_u8(&mut cursor)?;
        let entity_id = if kind == ENTITY_HORSE_TYPE {
            Some(codec::read_i32(&mut cursor)?)
        } else {
            None
        };
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            window_id,
            kind: WindowKind::from_type(&kind),
            title,
            slot_count,
            entity_id,
        })
    }
}

/// Clientbound Close Window (play id 0x2E).
///
/// `S2EPacketCloseWindow.readPacketData:32-35`: the window id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseWindow {
    /// The closing window's id.
    pub window_id: u8,
}

impl CloseWindow {
    /// The packet id.
    pub const ID: i32 = 0x2e;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let window_id = codec::read_u8(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { window_id })
    }
}

/// Clientbound Set Slot (play id 0x2F).
///
/// `S2FPacketSetSlot.readPacketData:37-42`: the window id as a signed byte,
/// the slot short and the slot's item. Window id −1 is the cursor — the
/// `== -1` branch writes `inventory.setItemStack`
/// (`NetHandlerPlayClient.handleSetSlot:1138-1140`) — and the slot is a real
/// window index: a negative slot never occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSlot {
    /// The window id; −1 is the cursor.
    pub window_id: i8,
    /// The slot index inside the window.
    pub slot: i16,
    /// The slot's item; `None` is the empty slot.
    pub item: Option<MetadataItem>,
}

impl SetSlot {
    /// The packet id.
    pub const ID: i32 = 0x2f;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let window_id = codec::read_u8(&mut cursor)? as i8;
        let slot = codec::read_i16(&mut cursor)?;
        let item = read_slot(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            window_id,
            slot,
            item,
        })
    }
}

/// Clientbound Window Items (play id 0x30).
///
/// `S30PacketWindowItems.readPacketData:34-43`: the window id, the slot count
/// as a short and one slot payload per index. The count is checked against
/// the bytes that remain before a single slot is read, so a declared count
/// the body cannot hold is refused up front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowItems {
    /// The window the slots belong to.
    pub window_id: u8,
    /// Every slot, index 0 through the declared count minus one.
    pub slots: Vec<Option<MetadataItem>>,
}

impl WindowItems {
    /// The packet id.
    pub const ID: i32 = 0x30;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let window_id = codec::read_u8(&mut cursor)?;
        let count = codec::read_i16(&mut cursor)?;
        if count < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(
                count.into(),
            )));
        }
        let count = count as usize;
        let remaining = body.len().saturating_sub(cursor.position() as usize);
        let max = remaining / MIN_SLOT_BYTES;
        if count > max {
            return Err(window_items_over_body(count, remaining, max));
        }
        let mut slots = Vec::with_capacity(count);
        for _ in 0..count {
            slots.push(read_slot(&mut cursor)?);
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { window_id, slots })
    }
}

/// Clientbound Window Property (play id 0x31).
///
/// `S31PacketWindowProperty.readPacketData:36-41`: the window id and the
/// property's index and value, both signed shorts — a furnace's burn time and
/// cook progress, an enchanting table's levels, a brewing stand's fuel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowProperty {
    /// The window the property belongs to.
    pub window_id: u8,
    /// The property index.
    pub property: i16,
    /// The property value.
    pub value: i16,
}

impl WindowProperty {
    /// The packet id.
    pub const ID: i32 = 0x31;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let window_id = codec::read_u8(&mut cursor)?;
        let property = codec::read_i16(&mut cursor)?;
        let value = codec::read_i16(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            window_id,
            property,
            value,
        })
    }
}

/// Clientbound Confirm Transaction (play id 0x32).
///
/// `S32PacketConfirmTransaction.readPacketData:36-41`: the window id, the
/// action number and the accepted flag. The action number echoes the click's
/// own; the client answers a rejection with a fresh accepted transaction
/// (`NetHandlerPlayClient.handleConfirmTransaction:1174-1189`). The source
/// reads the window id as an unsigned byte; this crate carries the wire byte
/// as the pinned `i8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmTransaction {
    /// The window the transaction belongs to.
    pub window_id: i8,
    /// The action number the server is confirming.
    pub action: i16,
    /// Whether the server accepted the action.
    pub accepted: bool,
}

impl ConfirmTransaction {
    /// The packet id.
    pub const ID: i32 = 0x32;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let window_id = codec::read_u8(&mut cursor)? as i8;
        let action = codec::read_i16(&mut cursor)?;
        let accepted = codec::read_bool(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            window_id,
            action,
            accepted,
        })
    }
}

/// Clientbound Update Sign (play id 0x33).
///
/// `S33PacketUpdateSign.readPacketData:31-40`: the sign's position and its
/// four lines as chat components. The lines are carried raw — composing or
/// parsing the component is the session's business — so nothing but the
/// protocol's string ceiling is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSign {
    /// The sign's world x.
    pub x: i32,
    /// The sign's world y.
    pub y: i32,
    /// The sign's world z.
    pub z: i32,
    /// The four lines as chat JSON, exactly as sent.
    pub lines: [String; 4],
}

impl UpdateSign {
    /// The packet id.
    pub const ID: i32 = 0x33;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let (x, y, z) = read_position(&mut cursor)?;
        let lines = [
            codec::read_string(&mut cursor, MAX_STRING_BYTES)?,
            codec::read_string(&mut cursor, MAX_STRING_BYTES)?,
            codec::read_string(&mut cursor, MAX_STRING_BYTES)?,
            codec::read_string(&mut cursor, MAX_STRING_BYTES)?,
        ];
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { x, y, z, lines })
    }
}

/// Clientbound Open Sign Editor (play id 0x36).
///
/// `S36PacketSignEditorOpen.readPacketData:33-36`: the sign's position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignEditorOpen {
    /// The sign's world x.
    pub x: i32,
    /// The sign's world y.
    pub y: i32,
    /// The sign's world z.
    pub z: i32,
}

impl SignEditorOpen {
    /// The packet id.
    pub const ID: i32 = 0x36;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let (x, y, z) = read_position(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { x, y, z })
    }
}

/// Clientbound Set Experience (play id 0x1F).
///
/// `S1FPacketSetExperience.readPacketData:28-33`: the bar's fill as a float,
/// then the level and the total experience, both VarInts. The wire is carried
/// as read — the bar's 0–1 range and the level's arithmetic are the consumer's
/// rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetExperience {
    /// The bar's fill, 0 through 1.
    pub bar: f32,
    /// The player's level.
    pub level: i32,
    /// The player's total experience.
    pub total: i32,
}

impl SetExperience {
    /// The packet id.
    pub const ID: i32 = 0x1f;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let bar = codec::read_f32(&mut cursor)?;
        let level = read_varint(&mut cursor)?;
        let total = read_varint(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { bar, level, total })
    }
}

/// Clientbound Entity Effect (play id 0x1D).
///
/// `S1DPacketEntityEffect.readPacketData:42-49`: the entity id as a VarInt,
/// the effect id and amplifier bytes, the duration as a VarInt, and the
/// hide-particles byte. The bytes are carried unsigned and the flag is
/// `byte != 0`, the source's own reading
/// (`S1DPacketEntityEffect.func_179707_f`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityEffect {
    /// The entity the effect landed on.
    pub entity_id: i32,
    /// The effect's id.
    pub effect_id: u8,
    /// The effect's amplifier.
    pub amplifier: u8,
    /// The effect's duration in ticks.
    pub duration: i32,
    /// Whether the swirl particles are hidden.
    pub hide_particles: bool,
}

impl EntityEffect {
    /// The packet id.
    pub const ID: i32 = 0x1d;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let entity_id = read_varint(&mut cursor)?;
        let effect_id = codec::read_u8(&mut cursor)?;
        let amplifier = codec::read_u8(&mut cursor)?;
        let duration = read_varint(&mut cursor)?;
        let hide_particles = codec::read_bool(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            entity_id,
            effect_id,
            amplifier,
            duration,
            hide_particles,
        })
    }
}

/// Clientbound Remove Entity Effect (play id 0x1E).
///
/// `S1EPacketRemoveEntityEffect.readPacketData:27-31`: the entity id as a
/// VarInt and the effect id byte, read unsigned as the source reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveEntityEffect {
    /// The entity the effect was removed from.
    pub entity_id: i32,
    /// The removed effect's id.
    pub effect_id: u8,
}

impl RemoveEntityEffect {
    /// The packet id.
    pub const ID: i32 = 0x1e;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let entity_id = read_varint(&mut cursor)?;
        let effect_id = codec::read_u8(&mut cursor)?;
        check_no_trailing(&cursor, body.len())?;
        Ok(Self {
            entity_id,
            effect_id,
        })
    }
}

/// One merchant offer, the villager trade list's unit.
///
/// `MerchantRecipeList.readFromBuf:83-94`: the item the villager buys, the
/// item it sells, the has-second flag with the second item it buys when set,
/// the disabled flag, and the use and max-use ints. The source's disabled
/// flag is read and dropped — [`MerchantOffer`] does not carry it, and the
/// reader still consumes it so the uses pair stays aligned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerchantOffer {
    /// The item the villager buys.
    pub first: Option<MetadataItem>,
    /// The second item the villager buys; `None` when the offer needs no
    /// second.
    pub second: Option<MetadataItem>,
    /// The item the villager sells.
    pub output: Option<MetadataItem>,
    /// How often the offer has been used.
    pub uses: i32,
    /// How often the offer may be used before it locks.
    pub max_uses: i32,
}

/// The merchant trade list: a `MC|TrList` custom payload's body.
///
/// `MerchantRecipeList.readFromBuf:76-106` behind
/// `NetHandlerPlayClient.handleCustomPayload:1826-1845`: a leading window id
/// as a big-endian int (the server writes it at `EntityPlayerMP.java:817`),
/// the offer count as one byte, then one offer per count. The window id is
/// read and dropped: it routes the list to the merchant screen, and the
/// offers are the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerchantOffers {
    /// The offers, in wire order.
    pub offers: Vec<MerchantOffer>,
}

impl MerchantOffers {
    /// Decodes a `MC|TrList` custom-payload body.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let _window_id = codec::read_i32(&mut cursor)?;
        let count = codec::read_u8(&mut cursor)? as usize;
        if count > MAX_MERCHANT_OFFERS {
            return Err(offers_over_cap(count));
        }
        let remaining = body.len().saturating_sub(cursor.position() as usize);
        let mut offers = Vec::with_capacity(count.min(remaining / MIN_OFFER_BYTES));
        for _ in 0..count {
            offers.push(read_offer(&mut cursor)?);
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { offers })
    }
}

/// Decodes one merchant offer (`MerchantRecipeList.readFromBuf:83-94`).
fn read_offer(cursor: &mut Cursor<&[u8]>) -> Result<MerchantOffer, PacketError> {
    let first = read_slot(cursor)?;
    let output = read_slot(cursor)?;
    let second = if codec::read_bool(&mut *cursor)? {
        read_slot(cursor)?
    } else {
        None
    };
    let _disabled = codec::read_bool(&mut *cursor)?;
    let uses = codec::read_i32(&mut *cursor)?;
    let max_uses = codec::read_i32(&mut *cursor)?;
    Ok(MerchantOffer {
        first,
        second,
        output,
        uses,
        max_uses,
    })
}

/// Reads a Location Position: x, y and z packed into one big-endian `i64`.
///
/// The packing is
/// `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)` — x and
/// z are 26 signed bits, y is 12 — and the read mirrors `BlockPos.fromLong`
/// (`util/BlockPos.java:208-214`): each field is shifted into the sign
/// position and back, which sign-extends it. This is 1.8.9's own packing; the
/// clientbound reader uses the same expression.
fn read_position(cursor: &mut Cursor<&[u8]>) -> Result<(i32, i32, i32), PacketError> {
    let raw = codec::read_i64(cursor)?;
    let x = (raw >> 38) as i32;
    let y = (raw << 26 >> 52) as i32;
    let z = (raw << 38 >> 38) as i32;
    Ok((x, y, z))
}

/// Refuses a Window Items count the remaining body cannot hold.
///
/// Every slot payload is at least [`MIN_SLOT_BYTES`] bytes, so `remaining / 2`
/// is the most slots the body can carry; a count above it is refused before a
/// slot is read.
fn window_items_over_body(count: usize, remaining: usize, max: usize) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "Window Items declares {count} slots but {remaining} byte(s) remain, holding at most {max}"
        ),
    )))
}

/// Refuses a merchant offer count past [`MAX_MERCHANT_OFFERS`], naming both.
fn offers_over_cap(count: usize) -> PacketError {
    PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("merchant offer count {count} exceeds the {MAX_MERCHANT_OFFERS} offer cap"),
    )))
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
    //! Fixed-literal tests for the packet ids, the window-kind table and the
    //! offer cap: every value is the literal the protocol reference records
    //! (`docs/research/protocol-47-reference.md` §2.1, the rows for 0x1D–0x1F,
    //! 0x2D–0x33 and 0x36) or the source's own registration site, never
    //! derived from the constants under test.

    use super::{
        CloseWindow, ConfirmTransaction, EntityEffect, MAX_MERCHANT_OFFERS, OpenWindow,
        RemoveEntityEffect, SetExperience, SetSlot, SignEditorOpen, UpdateSign, WindowItems,
        WindowKind, WindowProperty,
    };

    /// The source's window type strings, transcribed: one literal row per type
    /// string the 1.8.9 server sends — each `IInteractionObject` registration
    /// (`TileEntityChest.java:495`, `EntityMinecartChest.java:62`,
    /// `BlockWorkbench.java:74`, `TileEntityFurnace.java:460`,
    /// `TileEntityDispenser.java:236`, `TileEntityEnchantmentTable.java:167`,
    /// `TileEntityBrewingStand.java:405`, `TileEntityBeacon.java:415`,
    /// `BlockAnvil.java:187`, `TileEntityHopper.java:723`,
    /// `EntityMinecartHopper.java:231`, `TileEntityDropper.java:15`), the
    /// server's own sends (`EntityPlayerMP.java:795`, `:811`, `:831`) and the
    /// client's handling sites (`NetHandlerPlayClient.handleOpenWindow:1097`,
    /// `:1102`, `:1107`). A shifted or reordered table cannot pass it.
    const SOURCE_WINDOW_TYPES: [(&str, WindowKind); 14] = [
        ("minecraft:chest", WindowKind::Chest),
        ("minecraft:crafting_table", WindowKind::CraftingTable),
        ("minecraft:furnace", WindowKind::Furnace),
        ("minecraft:dispenser", WindowKind::Dispenser),
        ("minecraft:enchanting_table", WindowKind::EnchantingTable),
        ("minecraft:brewing_stand", WindowKind::BrewingStand),
        ("minecraft:villager", WindowKind::Villager),
        ("minecraft:merchant", WindowKind::Villager),
        ("minecraft:beacon", WindowKind::Beacon),
        ("minecraft:anvil", WindowKind::Anvil),
        ("minecraft:hopper", WindowKind::Hopper),
        ("minecraft:dropper", WindowKind::Dropper),
        ("EntityHorse", WindowKind::EntityHorse),
        ("minecraft:container", WindowKind::Container),
    ];

    #[test]
    fn the_window_kind_spot_table_is_the_source_table() {
        for (name, kind) in SOURCE_WINDOW_TYPES {
            assert_eq!(WindowKind::from_type(name), kind, "{name}");
        }
    }

    #[test]
    fn every_kind_names_its_source_string_and_maps_back() {
        let kinds = [
            WindowKind::Chest,
            WindowKind::CraftingTable,
            WindowKind::Furnace,
            WindowKind::Dispenser,
            WindowKind::EnchantingTable,
            WindowKind::BrewingStand,
            WindowKind::Villager,
            WindowKind::Beacon,
            WindowKind::Anvil,
            WindowKind::Hopper,
            WindowKind::Dropper,
            WindowKind::EntityHorse,
            WindowKind::Container,
        ];
        for kind in kinds {
            let name = kind.as_type().expect("a named kind names a string");
            assert_eq!(WindowKind::from_type(name), kind, "{name} maps back");
        }
        assert_eq!(
            WindowKind::Unknown.as_type(),
            None,
            "an unknown kind names no string"
        );
    }

    #[test]
    fn an_unlisted_window_type_is_unknown() {
        // The source's own test is an exact string comparison: an empty
        // string, a case variant and a trailing space are all unlisted.
        for name in [
            "",
            "Chest",
            "minecraft:banner",
            "minecraft:chest ",
            "EntityHorse ",
        ] {
            assert_eq!(WindowKind::from_type(name), WindowKind::Unknown, "{name:?}");
        }
    }

    #[test]
    fn the_window_packet_ids_are_the_section_ids() {
        assert_eq!(OpenWindow::ID, 0x2d, "0x2D Open Window");
        assert_eq!(CloseWindow::ID, 0x2e, "0x2E Close Window");
        assert_eq!(SetSlot::ID, 0x2f, "0x2F Set Slot");
        assert_eq!(WindowItems::ID, 0x30, "0x30 Window Items");
        assert_eq!(WindowProperty::ID, 0x31, "0x31 Window Property");
        assert_eq!(ConfirmTransaction::ID, 0x32, "0x32 Confirm Transaction");
        assert_eq!(UpdateSign::ID, 0x33, "0x33 Update Sign");
        assert_eq!(SignEditorOpen::ID, 0x36, "0x36 Open Sign Editor");
        assert_eq!(SetExperience::ID, 0x1f, "0x1F Set Experience");
        assert_eq!(EntityEffect::ID, 0x1d, "0x1D Entity Effect");
        assert_eq!(RemoveEntityEffect::ID, 0x1e, "0x1E Remove Entity Effect");
    }

    #[test]
    fn the_offer_cap_is_one_hundred_and_twenty_eight() {
        // The client's own bound on a merchant list, pinned as a literal.
        assert_eq!(MAX_MERCHANT_OFFERS, 128, "the offer cap");
    }
}
