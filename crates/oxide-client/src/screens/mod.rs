//! The screen framework: the open screen's state, the open/close rules and
//! the input-ownership flag.
//!
//! The port reads values and names from the source's screen base
//! (`GuiScreen.java`), its player close path (`EntityPlayerSP.closeScreen`
//! :330-341) and the wire close (`NetHandlerPlayClient.handleCloseWindow`
//! :1311-1315); no source text is copied.
//!
//! [`Screens`] holds the current screen. Five variants are declared here;
//! Task 16 wires only the [`ScreenState::Container`] kind's `WindowOpened`
//! path — the inventory open landed in Task 20, the creative open in Task
//! 21, the sign open in Task 22; the book open lands in Task 23, and an
//! open for a declared-but-unimplemented variant draws the generic frame
//! (recorded).
//!
//! The close rule: every close sends
//! [`InputEvent::CloseWindow`](oxide_game::input::InputEvent::CloseWindow)
//! with the current window id — window 0 included (`GuiContainer.keyTyped`
//! :692-696 closes on Escape and the inventory key for every container
//! screen, and `EntityPlayerSP.closeScreen`:330-333 sends C0D
//! unconditionally) — and drops the view's cursor copy with it (a close
//! carrying a stack DROPS it: `Container.onContainerClosed`:516-525; no
//! close-return animation exists — the `returningStack` easing at
//! `GuiContainer`:172-187 is the touchscreen drag-return only).
//!
//! The input ownership is `allowUserInput` (`GuiScreen`:68, gating
//! `Minecraft`:1834): true for the inventory and creative screens
//! (`GuiInventory`:28, `GuiContainerCreative`:64), false for containers
//! (which inherit it). The chat carries no `parentScreen` in this tree —
//! closing it returns to the game, never to a container beneath — so the
//! chat over the inventory stashes the screen and its close drops the stash.

use std::collections::HashMap;

use oxide_game::input::InputEvent;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::WindowKind;

use book::BookScreen;
use container::{ContainerLayout, ContainerScreen};
use creative::CreativeScreen;
use sign::{SignMapEntry, SignScreen};

pub mod book;
pub mod container;
pub mod creative;
pub mod family_a;
pub mod family_b;
pub mod inventory;
pub mod sign;

/// One open screen: a variant per screen the client can stand on.
#[derive(Debug, Clone)]
pub enum ScreenState {
    /// A container window's screen, standing on the window's id (boxed: the
    /// runtime dwarfs the other variants).
    Container(Box<ContainerScreen>),
    /// The player's own inventory: window 0's 45-slot screen through the
    /// same container path (boxed like the container kind).
    Inventory(Box<ContainerScreen>),
    /// A sign's editor at the block: the `SignEditorOpen` (0x36) path stands
    /// it on the session map's lines (Task 22 owns it).
    Sign(Box<SignScreen>),
    /// A written book's screen: the reader over the held stack the
    /// `MC|BOpen` path snapshotted (Task 23 owns it).
    Book(Box<BookScreen>),
    /// The creative screen: the tab strip, the paged grid and the search
    /// field over the player's own inventory window (boxed like the
    /// container kinds).
    Creative(Box<CreativeScreen>),
}

impl ScreenState {
    /// The window the screen stands on: the container's id, window 0 for
    /// the inventory and the creative screen alike — the creative screen
    /// stands on the player's own inventory window (`GuiContainerCreative`
    /// closes through the same `closeScreen`, so its close carries C0D id
    /// 0) — none for the windowless screens.
    pub fn window_id(&self) -> Option<u8> {
        match self {
            ScreenState::Container(screen) | ScreenState::Inventory(screen) => {
                Some(screen.window_id())
            }
            ScreenState::Creative(_) => Some(0),
            ScreenState::Sign(_) | ScreenState::Book(_) => None,
        }
    }

    /// The input ownership (`allowUserInput`, `GuiScreen`:68): the inventory
    /// and creative screens take user input; containers inherit false.
    pub fn allow_user_input(&self) -> bool {
        matches!(self, ScreenState::Inventory(_) | ScreenState::Creative(_))
    }
}

/// The open screen: the current state plus the chat's cover.
///
/// The cover is the chat over the inventory: opening it stashes the screen
/// and closing it drops the stash — the game returns, never a screen
/// beneath, because the chat carries no `parentScreen`.
#[derive(Debug, Default, Clone)]
pub struct Screens {
    /// The open screen, or `None` in the game.
    current: Option<ScreenState>,
    /// The screen the chat covered, dropped when the chat closes.
    covered: Option<ScreenState>,
    /// The creative screen's remembered tab: the session-scoped UI state
    /// (`selectedTabIndex`, `GuiContainerCreative`:42, default 0).
    /// `open_creative` stands the screen on it, `close` saves it back —
    /// reopening keeps the last tab within the session.
    creative_tab: u8,
    /// The last sign text per position: every `SignTextChanged` the frame
    /// folds lands here, so the `SignEditorOpen` path stands the editor on
    /// the map's lines — four empty lines when the map holds none (the
    /// tile's own default). Cleared entries leave with the world.
    sign_texts: HashMap<(i32, i32, i32), SignMapEntry>,
}

impl Screens {
    /// The open screen.
    pub fn current(&self) -> Option<&ScreenState> {
        self.current.as_ref()
    }

    /// The open container screen, when the current screen is one: the
    /// window's own routing reads and drives it. Window 0's inventory stands
    /// on the same path — its clicks carry 0, its keys run the container's
    /// own routing — so it answers here too; the family widgets keep the
    /// container-only routing above.
    pub fn container_mut(&mut self) -> Option<&mut ContainerScreen> {
        self.screen_mut()
    }

    /// The open screen's container path, when the current screen stands on
    /// one: the server's windows and the player's own inventory (window 0)
    /// alike. The per-frame feed, the pointer and the container keys drive
    /// this; the family widgets keep the container-only routing above.
    pub fn screen_mut(&mut self) -> Option<&mut ContainerScreen> {
        match self.current.as_mut() {
            Some(ScreenState::Container(screen) | ScreenState::Inventory(screen)) => {
                Some(screen.as_mut())
            }
            _ => None,
        }
    }

    /// Whether any screen is open.
    pub fn is_open(&self) -> bool {
        self.current.is_some()
    }

    /// The open screen's window id, if it stands on one.
    pub fn current_window_id(&self) -> Option<u8> {
        self.current.as_ref().and_then(ScreenState::window_id)
    }

    /// Whether the open screen takes user input (`allowUserInput`); a closed
    /// screen takes nothing.
    pub fn allow_user_input(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(ScreenState::allow_user_input)
    }

    /// The open creative screen, when the current screen is one: the
    /// frame's feed, the pointer and the creative keys drive this; the
    /// container-only routing above never answers it.
    pub fn creative_mut(&mut self) -> Option<&mut CreativeScreen> {
        match self.current.as_mut() {
            Some(ScreenState::Creative(screen)) => Some(screen.as_mut()),
            _ => None,
        }
    }

    /// Folds one `WindowOpened` into the screens: Task 16 wires the
    /// Container kind's path only — every kind opens a container screen, and
    /// an unlisted kind draws the generic frame (recorded). The horse window
    /// carries its entity id for the preview and the chested/armour flags;
    /// every other kind carries none.
    pub fn on_window_opened(
        &mut self,
        window_id: u8,
        kind: WindowKind,
        title: String,
        slot_count: u8,
        entity_id: Option<i32>,
    ) {
        self.open_container(window_id, kind, title, slot_count, entity_id);
    }

    /// Opens the container screen on the window. The family-A tables resolve
    /// by kind in [`family_a::layout_for_kind`] — the chest's row count rides
    /// the window's slot count — the family-B five in
    /// [`family_b::layout_for_kind`], and the unlanded kinds keep the generic
    /// frame (recorded). The family-B widget state stands up with the screen
    /// from the window's kind and entity id.
    pub fn open_container(
        &mut self,
        window_id: u8,
        kind: WindowKind,
        title: String,
        slot_count: u8,
        entity_id: Option<i32>,
    ) {
        let layout = match kind {
            WindowKind::Beacon
            | WindowKind::EnchantingTable
            | WindowKind::Villager
            | WindowKind::Anvil
            | WindowKind::EntityHorse => family_b::layout_for_kind(kind, slot_count),
            _ => family_a::layout_for_kind(kind, slot_count),
        };
        let mut screen = ContainerScreen::new(window_id, kind, title, layout);
        screen.set_family(family_b::FamilyState::for_kind(kind, entity_id));
        self.current = Some(ScreenState::Container(Box::new(screen)));
    }

    /// Stands a container screen on the given slot table. Test scaffolding:
    /// the generic frame the opens use carries no slots, so hover pins stand
    /// their own table.
    pub fn test_container(&mut self, window_id: u8, layout: &'static ContainerLayout) {
        self.current = Some(ScreenState::Container(Box::new(ContainerScreen::new(
            window_id,
            WindowKind::Chest,
            String::new(),
            layout,
        ))));
    }

    /// Opens the player's own inventory screen (window 0): one
    /// [`InputEvent::OpenInventory`] — the C16 client status 2
    /// (`Minecraft.java`:2090-2103, send at :2100, display at :2101) — and
    /// the fresh 45-slot screen beside it. No guard exists: two opens send
    /// two C16s. (The riding branch is out of scope, recorded; the creative
    /// swap lands with Task 21.)
    pub fn open_inventory(&mut self) -> Option<InputEvent> {
        self.current = Some(ScreenState::Inventory(Box::new(ContainerScreen::new(
            0,
            WindowKind::Container,
            String::new(),
            &inventory::INVENTORY_LAYOUT,
        ))));
        Some(InputEvent::OpenInventory)
    }

    /// The open sign editor, when the current screen is one: the frame's
    /// feed, the pointer and the sign keys drive this.
    pub fn sign_mut(&mut self) -> Option<&mut SignScreen> {
        match self.current.as_mut() {
            Some(ScreenState::Sign(screen)) => Some(screen.as_mut()),
            _ => None,
        }
    }

    /// Opens a sign's editor at the block on the session map's lines: one
    /// `SignEditorOpen` (0x36) stands it — the lines are the last
    /// `SignTextChanged` for the position, four empty lines when the map
    /// holds none (the tile's own default). No guard exists: two opens
    /// stand two editors.
    pub fn open_sign(&mut self, x: i32, y: i32, z: i32, lines: [String; 4]) {
        self.current = Some(ScreenState::Sign(Box::new(SignScreen::new(x, y, z, lines))));
    }

    /// Folds one sign-text report into the map: the last `SignTextChanged`
    /// for the position, which the `SignEditorOpen` path reads. The block
    /// and metadata ride along: the world draw needs the board's kind and
    /// facing.
    pub fn note_sign_text(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        block_id: u16,
        metadata: u8,
        lines: [String; 4],
    ) {
        self.sign_texts.insert(
            (x, y, z),
            SignMapEntry {
                block_id,
                metadata,
                lines,
            },
        );
    }

    /// Drops one sign-text entry: the position's board is gone, so the next
    /// open stands on four empty lines.
    pub fn forget_sign_text(&mut self, x: i32, y: i32, z: i32) {
        self.sign_texts.remove(&(x, y, z));
    }

    /// The lines the next open at the position stands on: the map's last
    /// text, four empty lines when it holds none.
    pub fn sign_lines(&self, x: i32, y: i32, z: i32) -> [String; 4] {
        self.sign_texts
            .get(&(x, y, z))
            .map(|entry| entry.lines.clone())
            .unwrap_or_default()
    }

    /// The world draw's entries: the map's positions with their boards and
    /// lines — nothing else, so positions outside the map draw no text (the
    /// port's no-text rule).
    pub fn sign_entries(&self) -> Vec<oxide_render::sign_text::SignTextEntry> {
        self.sign_texts
            .iter()
            .map(
                |((x, y, z), entry)| oxide_render::sign_text::SignTextEntry {
                    x: *x,
                    y: *y,
                    z: *z,
                    block_id: entry.block_id,
                    metadata: entry.metadata,
                    lines: entry.lines.clone(),
                },
            )
            .collect()
    }

    /// The open book reader, when the current screen is one: the frame's
    /// feed, the pointer and the book keys drive this.
    pub fn book_mut(&mut self) -> Option<&mut BookScreen> {
        match self.current.as_mut() {
            Some(ScreenState::Book(screen)) => Some(screen.as_mut()),
            _ => None,
        }
    }

    /// Opens a written book's screen on the held stack: one `MC|BOpen`
    /// stands the reader on page 0 over the stack the session snapshotted
    /// (`NetHandlerPlayClient.handleCustomPayload:1855-1863` opens the
    /// held stack). No guard exists: two opens stand two readers.
    pub fn open_book(&mut self, stack: MetadataItem) {
        self.current = Some(ScreenState::Book(Box::new(BookScreen::new(stack))));
    }

    /// Opens the creative screen on the remembered tab: one
    /// [`InputEvent::OpenInventory`] — the C16 client status 2 still sends
    /// first (`Minecraft.java`:2100 sends before the display at :2101,
    /// whatever the display becomes) — beside the fresh creative screen.
    /// No guard exists: two opens send two C16s. The tab is the
    /// session-scoped UI state (`selectedTabIndex`, `GuiContainerCreative`
    /// :42, default building-blocks 0): reopening keeps the last tab, and
    /// only a client-session reset returns it to 0.
    pub fn open_creative(&mut self) -> Option<InputEvent> {
        self.current = Some(ScreenState::Creative(Box::new(CreativeScreen::new(
            self.creative_tab,
        ))));
        Some(InputEvent::OpenInventory)
    }

    /// Swaps the open survival inventory for the creative screen: the
    /// port's `GuiInventory.updateScreen`:34-42, which re-displays as
    /// `GuiContainerCreative` every tick while creative. Anything but the
    /// inventory open stays put.
    pub fn swap_to_creative(&mut self) {
        if matches!(self.current, Some(ScreenState::Inventory(_))) {
            self.current = Some(ScreenState::Creative(Box::new(CreativeScreen::new(
                self.creative_tab,
            ))));
        }
    }

    /// Closes the open screen: one `CloseWindow` carrying the screen's own
    /// window id — window 0 included — and the view's cursor copy dropped
    /// with it. The sign's close answers its own send instead: the update
    /// with the position and the four lines as edited, on EVERY close
    /// (`GuiEditSign.onGuiClosed`:52-63, the send at `:59`). A screen on no
    /// other window, and no screen at all, send nothing. Closing the
    /// creative screen remembers its tab for the next open.
    pub fn close(&mut self, cursor: &mut Option<MetadataItem>) -> Option<InputEvent> {
        if let Some(ScreenState::Creative(screen)) = self.current.as_ref() {
            self.creative_tab = screen.selected_tab();
        }
        let screen = self.current.take()?;
        *cursor = None;
        match &screen {
            ScreenState::Sign(editor) => Some(editor.close_event()),
            _ => screen
                .window_id()
                .map(|window_id| InputEvent::CloseWindow { window_id }),
        }
    }

    /// One Escape press with the screen on top: the containers close through
    /// `closeScreen` (`GuiContainer.keyTyped`:692-696), and the base screen's
    /// own Escape (`GuiScreen.keyTyped`:104-115) lands in the same place —
    /// the screen gone, C0D sent. The chat above answers first and never
    /// reaches here.
    pub fn escape(&mut self, cursor: &mut Option<MetadataItem>) -> Option<InputEvent> {
        self.close(cursor)
    }

    /// Folds one server close into the screens: a close naming the live open
    /// window clears it with no send (`handleCloseWindow`:1311-1315 reaches
    /// `closeScreenAndDropStack` only); any other id changes nothing.
    pub fn on_server_close(&mut self, window_id: u8) -> bool {
        let live = self.current.as_ref().is_some_and(|screen| {
            matches!(screen, ScreenState::Container(container) if container.window_id() == window_id)
        });
        if live {
            self.current = None;
        }
        live
    }

    /// Folds one window snapshot into the open screen's view copies. The
    /// beacon's selection reseeds from properties 1/2 (the confirmed state —
    /// local row clicks update it between snapshots), and the anvil's slot-0
    /// sync resets the name field and re-fires `MC|ItemName` on a presence
    /// change (`GuiRepair.sendSlotContents`:200-212) — the re-fire travels
    /// back for the send. Anything else answers `None`.
    pub fn apply_snapshot(
        &mut self,
        window_id: u8,
        slots: Vec<Option<MetadataItem>>,
        cursor: Option<MetadataItem>,
        properties: Vec<i16>,
    ) -> Option<InputEvent> {
        // Window 0's snapshots feed the inventory screen; any other window's
        // the open container standing on it. The inventory answers no send —
        // the beacon reseed and the anvil re-fire below are the container
        // kinds' alone. The creative screen folds window 0 into its player
        // copies through its own S2F guard (hotbar always, the rest on the
        // inventory tab only) and likewise answers no send.
        let screen = match self.current.as_mut() {
            Some(ScreenState::Container(screen)) if screen.window_id() == window_id => {
                screen.as_mut()
            }
            Some(ScreenState::Inventory(screen)) if window_id == 0 => {
                let screen = screen.as_mut();
                screen.apply_snapshot(slots, cursor, properties);
                return None;
            }
            Some(ScreenState::Creative(screen)) if window_id == 0 => {
                screen.apply_snapshot(slots);
                return None;
            }
            _ => return None,
        };
        screen.apply_snapshot(slots, cursor, properties);
        let confirmed = screen.properties().to_vec();
        let slot0 = screen.slot_stack(0).cloned().flatten();
        match screen.family_mut() {
            family_b::FamilyState::Beacon(selection) => {
                selection.primary = confirmed.get(1).copied().unwrap_or(0) as i32;
                selection.secondary = confirmed.get(2).copied().unwrap_or(0) as i32;
            }
            family_b::FamilyState::Anvil(field) => {
                return family_b::anvil_sync(field, slot0.as_ref());
            }
            family_b::FamilyState::Enchanting(_)
            | family_b::FamilyState::Villager(_)
            | family_b::FamilyState::Horse(_)
            | family_b::FamilyState::None => {}
        }
        None
    }

    /// Folds one `MC|TrList` trade list into the open villager screen: the
    /// pager clamps its selection into the fresh list. A list with no
    /// villager open changes nothing.
    pub fn set_offers(&mut self, offers: Vec<oxide_proto_v47::window::MerchantOffer>) {
        if let Some(ScreenState::Container(screen)) = self.current.as_mut() {
            if let family_b::FamilyState::Villager(pager) = screen.family_mut() {
                pager.set_offers(offers);
            }
        }
    }

    /// Steps the open screen's family-B widgets one session tick: the
    /// enchanting book and the anvil blink (`tick_family`).
    pub fn tick_family(&mut self) {
        if let Some(ScreenState::Container(screen)) = self.current.as_mut() {
            family_b::tick_family(screen);
        }
    }

    /// Steps the open sign editor's blink counter one session tick: the
    /// editor's own `updateCounter` (`GuiEditSign.updateScreen`:68-71).
    /// The fold no-ops with no sign open.
    pub fn tick_sign(&mut self) {
        if let Some(editor) = self.sign_mut() {
            editor.tick();
        }
    }

    /// The chat opens over the screen: the screen stashes and the game shows
    /// the chat. Only the inventory can sit beneath it — containers swallow
    /// the chat key (`allowUserInput` false) — and the stash is dropped, not
    /// restored, when the chat closes.
    pub fn cover_with_chat(&mut self) {
        self.covered = self.current.take();
    }

    /// The chat closes over a covered screen: the stash drops and the game
    /// returns — the chat carries no `parentScreen`.
    pub fn uncover_from_chat(&mut self) {
        self.covered = None;
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    //! The inventory open path: one C16 per open, window 0's snapshot feed
    //! and close.

    use oxide_proto_v47::entity::MetadataItem;

    use super::*;

    /// One stack's view shape for the snapshot pins.
    fn stack(id: i16) -> Option<MetadataItem> {
        Some(MetadataItem {
            id,
            count: 1,
            damage: 0,
            nbt: None,
        })
    }

    /// Four empty lines: the session map's default for a missing entry.
    fn empty_sign_lines() -> [String; 4] {
        [String::new(), String::new(), String::new(), String::new()]
    }

    #[test]
    fn two_opens_send_two_c16s() {
        // No guard exists on the open path (`Minecraft.java`:2090-2103): two
        // opens send two C16s, each opening the screen beside it (:2101).
        let mut screens = Screens::default();
        assert_eq!(screens.open_inventory(), Some(InputEvent::OpenInventory));
        assert!(matches!(screens.current(), Some(ScreenState::Inventory(_))));
        assert_eq!(screens.open_inventory(), Some(InputEvent::OpenInventory));
    }

    #[test]
    fn window_zero_snapshots_feed_the_inventory_screen() {
        // Window 0 renders through the same container path: its snapshots
        // land on the inventory screen's slots, live.
        let mut screens = Screens::default();
        screens.open_inventory();
        let mut slots = vec![None; 45];
        slots[0] = stack(264);
        slots[1] = stack(265);
        assert_eq!(
            screens.apply_snapshot(0, slots, None, Vec::new()),
            None,
            "a window-0 snapshot answers no send"
        );
        let screen = screens.screen_mut().expect("the inventory stands");
        assert_eq!(screen.window_id(), 0);
        assert_eq!(screen.slot_stack(0).cloned().flatten(), stack(264));
        assert_eq!(screen.slot_stack(1).cloned().flatten(), stack(265));
    }

    #[test]
    fn the_inventory_close_carries_window_zero() {
        // The close sends C0D with window id 0 like every other close
        // (`EntityPlayerSP.closeScreen`:330-333).
        let mut screens = Screens::default();
        screens.open_inventory();
        let mut cursor = stack(264);
        assert_eq!(
            screens.close(&mut cursor),
            Some(InputEvent::CloseWindow { window_id: 0 })
        );
        assert_eq!(cursor, None, "the close drops the cursor copy");
    }

    #[test]
    fn two_book_opens_stand_two_readers_and_the_close_sends_nothing() {
        // One `MC|BOpen` stands the reader on page 0 over the snapshotted
        // stack; two opens stand two readers. The close sends nothing — the
        // reader stands on no window, and the unsigned `MC|BEdit` send is
        // the editor's, not the reader's (recorded).
        let mut screens = Screens::default();
        screens.open_book(stack(387).expect("the written stack"));
        let reader = screens.book_mut().expect("the reader stands");
        assert_eq!(reader.current_page(), 0);
        assert_eq!(reader.stack().id, 387);
        screens.open_book(stack(386).expect("the editable stack"));
        assert_eq!(
            screens
                .book_mut()
                .expect("the second reader stands")
                .stack()
                .id,
            386
        );
        let mut cursor = stack(264);
        assert_eq!(screens.close(&mut cursor), None, "no window, no send");
        assert_eq!(cursor, None, "the close still drops the cursor copy");
        assert!(screens.book_mut().is_none(), "the reader is gone");
    }

    #[test]
    fn the_creative_open_sends_one_c16_beside_the_screen() {
        // The C16 still sends first in creative mode (`Minecraft.java`:2100
        // sends before the display at :2101, whatever the display becomes).
        let mut screens = Screens::default();
        assert_eq!(screens.open_creative(), Some(InputEvent::OpenInventory));
        assert!(matches!(screens.current(), Some(ScreenState::Creative(_))));
        assert_eq!(screens.current_window_id(), Some(0));
    }

    #[test]
    fn the_creative_open_remembers_its_tab_within_the_session() {
        // The static `selectedTabIndex` (`GuiContainerCreative`:42, default
        // 0): reopening keeps the last tab; only a session reset returns it.
        let mut screens = Screens::default();
        screens.open_creative();
        screens
            .creative_mut()
            .expect("the creative stands")
            .set_tab(5);
        let mut cursor = None;
        screens.close(&mut cursor);
        screens.open_creative();
        assert_eq!(
            screens
                .creative_mut()
                .expect("the creative stands")
                .selected_tab(),
            5
        );
    }

    #[test]
    fn window_zero_snapshots_feed_the_creative_hotbar_only_off_the_inventory_tab() {
        // The S2F guard (`handleSetSlot`:1135-1165): slots 36–44 always
        // land; the rest are suppressed off the inventory tab.
        let mut screens = Screens::default();
        screens.open_creative();
        let mut slots = vec![None; 46];
        slots[9] = stack(264);
        slots[36] = stack(265);
        assert_eq!(
            screens.apply_snapshot(0, slots, None, Vec::new()),
            None,
            "a window-0 snapshot answers no send"
        );
        let screen = screens.creative_mut().expect("the creative stands");
        assert_eq!(screen.player_slot(9), Some(&None));
        assert_eq!(screen.player_slot(36), Some(&stack(265)));
        screen.set_tab(11);
        let mut slots = vec![None; 46];
        slots[9] = stack(264);
        assert_eq!(screens.apply_snapshot(0, slots, None, Vec::new()), None);
        assert_eq!(
            screens
                .creative_mut()
                .expect("the creative stands")
                .player_slot(9),
            Some(&stack(264))
        );
    }

    #[test]
    fn the_creative_close_carries_window_zero() {
        // The creative screen stands on the player's own inventory window,
        // so its close carries C0D id 0 like every other close.
        let mut screens = Screens::default();
        screens.open_creative();
        let mut cursor = stack(264);
        assert_eq!(
            screens.close(&mut cursor),
            Some(InputEvent::CloseWindow { window_id: 0 })
        );
        assert_eq!(cursor, None, "the close drops the cursor copy");
    }

    #[test]
    fn the_update_screen_swap_replaces_only_the_inventory() {
        // `GuiInventory.updateScreen`:34-42 re-displays as creative while
        // creative; any other open stays put.
        let mut screens = Screens::default();
        screens.open_inventory();
        screens.swap_to_creative();
        assert!(matches!(screens.current(), Some(ScreenState::Creative(_))));
        let mut screens = Screens::default();
        screens.open_sign(1, 2, 3, empty_sign_lines());
        screens.swap_to_creative();
        assert!(matches!(screens.current(), Some(ScreenState::Sign(_))));
    }

    #[test]
    fn the_sign_open_seeds_the_editor_with_the_map_lines() {
        // The 0x36 path stands the editor on the last `SignTextChanged`
        // for the position — four empty lines when the map holds none.
        let mut screens = Screens::default();
        let map_lines = [
            "first".to_string(),
            String::new(),
            "third".to_string(),
            String::new(),
        ];
        screens.open_sign(4, 65, -9, map_lines.clone());
        let editor = screens.sign_mut().expect("the sign stands");
        assert_eq!(editor.position(), (4, 65, -9));
        assert_eq!(editor.lines(), &map_lines);
        assert_eq!(editor.edit_line(), 0);
    }

    #[test]
    fn the_sign_close_sends_the_update_on_done_and_on_escape() {
        // `onGuiClosed` sends on every close (`GuiEditSign`:52-63): the
        // Done button and Escape reach the same close, so both answer the
        // update — never a window close, the sign stands on no window.
        for close in [Screens::close, Screens::escape] {
            let mut screens = Screens::default();
            screens.open_sign(4, 65, -9, empty_sign_lines());
            screens
                .sign_mut()
                .expect("the sign stands")
                .type_text("hi", &sign_font());
            let mut cursor = None;
            assert_eq!(
                close(&mut screens, &mut cursor),
                Some(InputEvent::UpdateSign {
                    x: 4,
                    y: 65,
                    z: -9,
                    lines: [
                        "hi".to_string(),
                        String::new(),
                        String::new(),
                        String::new()
                    ],
                })
            );
            assert!(!screens.is_open(), "the close drops the screen");
        }
    }

    #[test]
    fn the_sign_tick_steps_the_blink_counter() {
        // The editor's own `updateCounter` (`updateScreen`:68-71): six
        // ticks hide the markers a fresh open shows.
        let mut screens = Screens::default();
        screens.open_sign(0, 64, 0, empty_sign_lines());
        assert!(screens.sign_mut().expect("the sign stands").blink_visible());
        for _ in 0..6 {
            screens.tick_sign();
        }
        assert!(!screens.sign_mut().expect("the sign stands").blink_visible());
        // Anything but the sign no-ops.
        let mut screens = Screens::default();
        screens.open_inventory();
        screens.tick_sign();
    }

    /// The synthetic sheet's measured font for the close path's typing:
    /// the `h`/`i` cells ink columns 0..=4, six font pixels each.
    fn sign_font() -> oxide_assets::font::Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        for code in ['h', 'i'] {
            let code = code as u32;
            let cell_x = (code % 16) * CELL;
            let cell_y = (code / 16) * CELL;
            for row in 0..CELL {
                for column in 0..=4 {
                    let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                    rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        oxide_assets::font::Font::load(
            &oxide_assets::texture::Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet loads")
    }
}
