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
//! path — the opens for Inventory, Sign, Book and Creative land in Tasks
//! 20-23, and an open for a declared-but-unimplemented variant draws the
//! generic frame (recorded).
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

use oxide_game::input::InputEvent;
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::window::WindowKind;

use container::{ContainerScreen, GENERIC_LAYOUT};

pub mod container;

/// One open screen: a variant per screen the client can stand on.
#[derive(Debug, Clone)]
pub enum ScreenState {
    /// A container window's screen, standing on the window's id (boxed: the
    /// runtime dwarfs the other variants).
    Container(Box<ContainerScreen>),
    /// The player's own inventory (window 0; Task 20 owns its screen).
    Inventory,
    /// A sign's editor at the block (Task 22 owns it).
    Sign {
        /// The sign's world x.
        x: i32,
        /// The sign's world y.
        y: i32,
        /// The sign's world z.
        z: i32,
    },
    /// A written book's screen (Task 23 owns it).
    Book {
        /// The book stack being read.
        stack: MetadataItem,
    },
    /// The creative screen (Task 21 owns it).
    Creative,
}

impl ScreenState {
    /// The window the screen stands on: the container's id, window 0 for
    /// the inventory, none for the windowless screens.
    pub fn window_id(&self) -> Option<u8> {
        match self {
            ScreenState::Container(screen) => Some(screen.window_id()),
            ScreenState::Inventory => Some(0),
            ScreenState::Sign { .. } | ScreenState::Book { .. } | ScreenState::Creative => None,
        }
    }

    /// The input ownership (`allowUserInput`, `GuiScreen`:68): the inventory
    /// and creative screens take user input; containers inherit false.
    pub fn allow_user_input(&self) -> bool {
        matches!(self, ScreenState::Inventory | ScreenState::Creative)
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
}

impl Screens {
    /// The open screen.
    pub fn current(&self) -> Option<&ScreenState> {
        self.current.as_ref()
    }

    /// The open container screen, when the current screen is one: the
    /// window's own routing reads and drives it.
    pub fn container_mut(&mut self) -> Option<&mut ContainerScreen> {
        match self.current.as_mut() {
            Some(ScreenState::Container(screen)) => Some(screen.as_mut()),
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

    /// Folds one `WindowOpened` into the screens: Task 16 wires the
    /// Container kind's path only — every kind opens a container screen, and
    /// an unlisted kind draws the generic frame (recorded).
    pub fn on_window_opened(&mut self, window_id: u8, kind: WindowKind, title: String) {
        self.open_container(window_id, kind, title);
    }

    /// Opens the container screen on the window. The per-kind tables land in
    /// Tasks 18-20; until then every container draws the generic frame
    /// (recorded) while the kind still records which table it will take.
    pub fn open_container(&mut self, window_id: u8, kind: WindowKind, title: String) {
        self.current = Some(ScreenState::Container(Box::new(ContainerScreen::new(
            window_id,
            kind,
            title,
            &GENERIC_LAYOUT,
        ))));
    }

    /// Opens the player's own inventory screen (window 0).
    pub fn open_inventory(&mut self) {
        self.current = Some(ScreenState::Inventory);
    }

    /// Opens a sign's editor at the block. The editor lands in Task 22; the
    /// variant draws the generic frame until then (recorded).
    pub fn open_sign(&mut self, x: i32, y: i32, z: i32) {
        self.current = Some(ScreenState::Sign { x, y, z });
    }

    /// Opens a written book's screen. The reader lands in Task 23; the
    /// variant draws the generic frame until then (recorded).
    pub fn open_book(&mut self, stack: MetadataItem) {
        self.current = Some(ScreenState::Book { stack });
    }

    /// Opens the creative screen. It lands in Task 21; the variant draws the
    /// generic frame until then (recorded).
    pub fn open_creative(&mut self) {
        self.current = Some(ScreenState::Creative);
    }

    /// Closes the open screen: one `CloseWindow` carrying the screen's own
    /// window id — window 0 included — and the view's cursor copy dropped
    /// with it. A screen on no window, and no screen at all, send nothing.
    pub fn close(&mut self, cursor: &mut Option<MetadataItem>) -> Option<InputEvent> {
        let screen = self.current.take()?;
        *cursor = None;
        screen
            .window_id()
            .map(|window_id| InputEvent::CloseWindow { window_id })
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

    /// Folds one window snapshot into the open screen's view copies.
    pub fn apply_snapshot(
        &mut self,
        window_id: u8,
        slots: Vec<Option<MetadataItem>>,
        cursor: Option<MetadataItem>,
    ) {
        if let Some(ScreenState::Container(screen)) = self.current.as_mut() {
            if screen.window_id() == window_id {
                screen.apply_snapshot(slots, cursor);
            }
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
