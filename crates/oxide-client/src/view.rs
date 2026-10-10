//! The entity view: the session's per-tick entity feed, and the draws a frame builds
//! from it.
//!
//! The view keeps the latest [`ClientEvent::EntitiesTick`] frames and the instant they
//! arrived, and interpolates them at the same frame fraction the player pose uses:
//! the elapsed time over the fifty-millisecond tick, clamped to one. A frame builds
//! one draw per tracked entity ([`View::entity_draws`]) — the window's own entity
//! skipped — reading the stored frames and the skin worker's updates only; nothing
//! looks at the world, which the window does not hold
//! (`RendererLivingEntity.doRender`'s interpolated terms are the model here).
//!
//! The same session that feeds the entities feeds the chat: [`ClientEvent::Chat`]
//! messages land in the mirror ([`ChatView`]), whose [`ChatLog`] holds the split lines,
//! the fade clocks and the scroll state, and whose [`ChatView::draws`] assembles the
//! frame's hud draw list at the scaled resolution — the box the source draws at
//! `GuiNewChat.drawChat`:30-114 and the record line above the hotbar
//! (`GuiIngame.java`:245-272).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Instant;

use oxide_assets::font::Font;
use oxide_assets::skins::{DefaultModel, default_skin};
use oxide_game::chat::{
    self, CHAT_WIDTH, ChatLog, LOG_CAP, LanguageTable, STYLE_BOLD, STYLE_ITALIC, STYLE_OBFUSCATED,
    STYLE_STRIKETHROUGH, STYLE_UNDERLINED, TextComponent,
};
use oxide_game::entity_view::{EntityExtra, EntityFrame, MobExtra, PlayerListRecord};
use oxide_game::scoreboard::{Objective, Scoreboard, format_entry};
use oxide_game::session::{ClientEvent, StatusEffect};
use oxide_proto_v47::entity::MetadataItem;
use oxide_proto_v47::nbt::NbtValue;
use oxide_render::entity_models::player::PlayerExtra;
use oxide_render::entity_models::{Pose, PoseExtra, objects};
use oxide_render::entity_pass::{
    DrawExtra, EntityDraw, EquipmentDraw, FrameContent, ModelRef, NametagDraw, SkinLookup,
    SkinTexId, TextureRef,
};
use oxide_render::gui_item::ItemIcon;
use oxide_render::held_item::{ArmSway, Equip, HeldItemFrame, Swing};
use oxide_render::hud::{HudDraw, HudTexture, ScaledResolution};
use oxide_render::text::string_width;
use oxide_world::behaviour::{self, RenderKind};
use oxide_world::entity::EntityKind;

use crate::CHAT_TEXT_CAP;
use crate::ChatInput;
use crate::skin_worker::SkinUpdate;
use oxide_client::items;
use oxide_client::screens::container::{ContainerScreen, HOVER_COLOUR, title_rgba};
use oxide_client::screens::creative::CreativeScreen;
use oxide_client::screens::family_a;
use oxide_client::screens::family_b;
use oxide_client::screens::inventory;
use oxide_client::screens::{ScreenState, Screens};
use oxide_client::tooltip;

/// The all-on parts byte every player draws with this milestone.
///
/// The byte is the player's model-parts set (`EnumPlayerModelParts`); the wire's
/// skin-flags metadata carries a player's own, and the local settings screen its
/// owner's — neither channel exists yet, so the window draws every player with every
/// part enabled (`RenderPlayer.java:81-86` gates each overlay through `isWearing`).
pub const ALL_PARTS: u8 = 0x7F;

/// The tick's step, in seconds: the fraction's divisor (the camera's own 50 ms rule).
const TICK_SECONDS: f32 = 0.05;

/// The hotbar band's own start in window 0's layout: the crafting band, the armour
/// band at 5–8 and the main twenty-seven at 9–35 ahead of it
/// (`ContainerPlayer.java`:36-67; the port's own windows.rs holds the same band at
/// 36–44).
const HOTBAR_START: usize = 36;

/// The tick-to-tick step past which the draw's position snaps instead of sliding: the
/// same four-block rule the camera's pose interpolation pins (`Render.doRender`'s
/// teleport class).
const SNAP_BLOCKS: f64 = 4.0;

/// The window's entity frame state: the feed, its arrival, and the self entity.
pub struct View {
    /// The frames the latest [`ClientEvent::EntitiesTick`] carried, in its own order.
    frames: Vec<oxide_game::entity_view::EntityFrame>,
    /// When that tick arrived, for the frame fraction.
    arrival: Option<Instant>,
    /// The window's own entity id, from [`ClientEvent::Joined`]; skipped when drawing.
    own: Option<i32>,
    /// The held item's own state: the stack, the ease and the swing.
    held: HeldItem,
    /// The survival rows' own state: the feeds `stat_rows_draws` reads.
    rows: StatRows,
}

/// The window's held-item state: the source's own `ItemRenderer` fields the hand draws
/// from.
///
/// The stack is the source's `itemToRender`, the ease its two counters
/// (`equippedProgress`/`prevEquippedProgress`, `ItemRenderer.java`:42-43) and the swing
/// the own player's (`EntityLivingBase.java`:66-86) — all three are client-local: the
/// own player's stack rides the window 0 snapshot, its selection the held-slot event,
/// and the swing is set by the window's own input (the source's `swingItem` calls from
/// `clickMouse`:1519 and `rightClickMouse`:1613). The swap rule is the source's own:
/// `itemToRender` lands when `equippedProgress < 0.1` (`updateEquippedItem`:609-613),
/// and the comparison is the stack's identity — the source's `getIsItemStackEqual`
/// compares item, damage, size and NBT; the port compares the whole
/// [`MetadataItem`], the wire's own shape (recorded).
#[derive(Debug, Clone)]
struct HeldItem {
    /// The window's selected hotbar slot, 0..8 (`InventoryPlayer.currentItem`).
    selected: usize,
    /// The window 0 snapshot's nine hotbar stacks, in slot order.
    hotbar: Vec<Option<MetadataItem>>,
    /// The pop counters the window 0 snapshot carries per hotbar slot: the
    /// source's `animationsToGo` (`GuiIngame.renderHotbarItem`:1043), ticked by
    /// the session, read here minus the frame's fraction.
    pop: [u8; 9],
    /// The popup's remaining clock (`GuiIngame.remainingHighlightTicks`).
    popup_ticks: u8,
    /// The stack the popup names (`GuiIngame.highlightingItemStack`).
    popup_stack: Option<MetadataItem>,
    /// The stack `itemToRender` holds, through the ease's swap rule.
    stack: Option<MetadataItem>,
    /// The equip ease's counters.
    equip: Equip,
    /// The swing counters.
    swing: Swing,
    /// The arm's own rotation pair (`renderArmYaw`/`renderArmPitch` and their
    /// latches on the source's own player, `EntityPlayerSP.java`:115-119).
    arm: ArmSway,
    /// The tick's own rotation, the sway's raw pair (`rotationPitch`/`rotationYaw`).
    rotation_pitch: f32,
    rotation_yaw: f32,
}

impl HeldItem {
    /// The resting state: no snapshot, the first slot selected, nothing in hand.
    fn new() -> Self {
        Self {
            selected: 0,
            hotbar: Vec::new(),
            pop: [0; 9],
            popup_ticks: 0,
            popup_stack: None,
            stack: None,
            equip: Equip::new(),
            swing: Swing::new(),
            arm: ArmSway::new(),
            rotation_pitch: 0.0,
            rotation_yaw: 0.0,
        }
    }

    /// The selected slot's stack, as the snapshot holds it (`InventoryPlayer.getCurrentItem`).
    fn current(&self) -> Option<MetadataItem> {
        self.hotbar.get(self.selected).and_then(Clone::clone)
    }

    /// One tick of the source's own three updaters: `updateEquippedItem`
    /// (`ItemRenderer.java`:581-611), `updateArmSwingProgress`
    /// (`EntityLivingBase.java`:1402-1422) and the arm pair's own chase toward the
    /// tick's rotation (`EntityPlayerSP.updateEntityActionState`:699-702).
    ///
    /// The ease's target follows whether the selected stack differs from the one in
    /// hand (`flag`), the swap lands while the ease is below its threshold, the
    /// swing's counter advances its own tick, and the arm's pair latches and moves
    /// half the way to `pitch`/`yaw` — which the frame's sway also reads as its raw
    /// pair (`rotationPitch`/`rotationYaw`).
    fn tick(&mut self, pitch: f32, yaw: f32) {
        let current = self.current();
        let differ = self.stack != current;
        self.equip.tick(differ);
        if self.equip.swap_lands() {
            self.stack = current.clone();
        }
        // The popup's own clock (`GuiIngame.updateTick`:1089-1109): an empty
        // hand clears it, the same stack counts it down while positive, and any
        // other swap resets it to forty — then the stack is stored either way.
        match current {
            None => {
                self.popup_ticks = 0;
                self.popup_stack = None;
            }
            Some(stack) => {
                let same = self
                    .popup_stack
                    .as_ref()
                    .is_some_and(|previous| same_popup_stack(&stack, previous));
                if same {
                    if self.popup_ticks > 0 {
                        self.popup_ticks -= 1;
                    }
                } else {
                    self.popup_ticks = POPUP_TICKS;
                }
                self.popup_stack = Some(stack);
            }
        }
        self.swing.tick();
        self.arm.tick(pitch, yaw);
        self.rotation_pitch = pitch;
        self.rotation_yaw = yaw;
    }

    /// Starts a swing (`swingItem`:1342-1354) — the window's own input path.
    fn swing(&mut self) {
        self.swing.swing();
    }

    /// The frame at the given fraction: the stack through the swap rule and the two
    /// rendered arguments, resolved at this seam.
    ///
    /// The ease renders the source's own `f = 1 - (prev + (cur - prev) * partial)`
    /// (`ItemRenderer.renderItemInFirstPerson`:357) and the swing `getSwingProgress`
    /// (`EntityLivingBase`:2188-2198); the arm pair renders its own interpolation
    /// (`ItemRenderer.rotateWithPlayerRotations`:124-125) and the sway's arguments
    /// are the raw current rotation minus it. The raw previous-tick latches ride
    /// along for the record. The brightness stands at the full-bright equivalent and
    /// the sleep state at false: the port has no own-player light channel (the
    /// session's feed carries a brightness per tracked entity, and the window's own
    /// player is not one) and no sleep state (recorded; the pass honours both).
    fn frame(&self, partial: f32) -> HeldItemFrame {
        let (arm_pitch, arm_yaw) = self.arm.render(partial);
        HeldItemFrame {
            // The stack's own icon conversion, the draw-list seam's helper: the id,
            // the damage and the glint flag the source's own rule gives it (the pass
            // does not draw the held item's glint — recorded).
            stack: self.stack.as_ref().map(item_icon),
            equip: self.equip.render(partial),
            equip_prev: self.equip.prev(),
            swing: self.swing.render(partial),
            swing_prev: self.swing.prev(),
            // The sway's own pair: the tick's rotation minus the arm pair at this
            // fraction (`rotateWithPlayerRotations`:127-128).
            sway_pitch: self.rotation_pitch - arm_pitch,
            sway_yaw: self.rotation_yaw - arm_yaw,
            brightness: 1.0,
            sleeping: false,
        }
    }
}

/// The widgets sheet's key, the extraction tree's `gui/widgets.png`: the hotbar's
/// background and highlight slices sample it under the name their draws carry
/// (`GuiIngame.java`:48, bound at `:370`).
const HOTBAR_WIDGETS: &str = "gui/widgets";

/// The popup clock's full count: a swapped stack resets `remainingHighlightTicks`
/// to forty (`GuiIngame.updateTick`:1104).
const POPUP_TICKS: u8 = 40;

/// The popup alpha's span: `k = ticks × 256 / 10` (`GuiIngame.java`:473).
const POPUP_ALPHA_SPAN: f32 = 10.0;

/// The pop curve's divisor: `f1 = 1 + f/5` (`GuiIngame.renderHotbarItem`:1048).
/// The live matrix reads it in the render pass; this names it for the suite that
/// pins the curve, so no frame code cites the literal.
#[allow(dead_code)]
const POP_DIVISOR: f32 = 5.0;

/// The popup's row above the screen's bottom (`GuiIngame.java`:467).
const POPUP_ABOVE: i32 = 59;

/// How much lower the popup sits outside survival and adventure
/// (`GuiIngame.java`:468-471 — `!shouldDrawHUD()`, creative and spectator).
const POPUP_CREATIVE_SHIFT: i32 = 14;

/// The durability bar's full width, in GUI pixels (`RenderItem.java`:478).
const DURABILITY_WIDTH: f64 = 13.0;

/// The durability ramp's full scale (`RenderItem.java`:479).
const DURABILITY_RAMP: f64 = 255.0;

/// The hotbar frame's inputs beside the view's own state: the font the count and
/// the popup measure with, the scaled resolution, and the frame's three gates —
/// the crosshair's `showCrosshair`, the call site's `hideGUI`/`currentScreen`
/// pair, and `shouldDrawHUD`'s survival-and-adventure arm.
pub struct HotbarInput<'a> {
    /// The measured font; without one the count and the popup (which need the
    /// string width) stay out while the slices, icons and bars still draw.
    pub font: Option<&'a Font>,
    /// The GUI-space size the frame lays out in.
    pub scaled: ScaledResolution,
    /// Whether the crosshair draws (`GuiIngame.showCrosshair`:513-544).
    pub show_crosshair: bool,
    /// The F1 state (`EntityRenderer.java`:1166).
    pub hide_gui: bool,
    /// Whether a screen is open (`EntityRenderer.java`:1166).
    pub screen_open: bool,
    /// Whether survival or adventure is in force (`shouldDrawHUD`,
    /// `PlayerControllerMP.java`:115-117).
    pub survival: bool,
}

/// Whether the HUD draws at all: the call site's own gate, not the overlay's —
/// `!hideGUI || currentScreen != null` (`EntityRenderer.java`:1166-1169). With
/// a screen open the whole overlay still draws (the screen paints over it after
/// the depth clear, `:1185-1191`); `renderGameOverlay` itself carries no screen
/// condition (`GuiIngame.java`:128-363). No per-entry screen suppression exists.
pub(crate) fn hud_visible(hide_gui: bool, screen_open: bool) -> bool {
    !hide_gui || screen_open
}

/// The screen frame's inputs beside the screens' own state: the font the
/// titles and the counts measure with, the GUI-space size, and the free
/// pointer's scaled position the cursor draw reads.
pub struct ScreenDrawInput<'a> {
    /// The measured font; without one the titles and the counts stay out
    /// while the sheet, items and bars still draw.
    pub font: Option<&'a Font>,
    /// The GUI-space size the frame lays out in.
    pub scaled: ScaledResolution,
    /// The free pointer's scaled position, or `None` before the first move.
    pub mouse: Option<(f32, f32)>,
    /// Whether F3+H has the advanced tooltips showing (`ItemStack.getTooltip`'s
    /// flag at `GuiScreen.java`:160 — the appendix Task 17 reads).
    pub advanced: bool,
    /// The player's experience level: the enchanting offer faces and click
    /// gate read it (`ContainerEnchantment.enchantItem`'s level arms).
    pub level: i32,
    /// The own player's live effects, ascending by id: the inventory overlay
    /// reads them (`ClientEvent::Effects` after 0x1D/0x1E).
    pub effects: &'a [StatusEffect],
    /// The own player's resolved skin, or `None` before the login lands it:
    /// the inventory preview samples it. Without one the preview stays out
    /// while the sheet and slots still draw.
    pub preview_skin: Option<SkinTexId>,
}

/// The screen group's own draws: the source's `currentScreen.drawScreen`
/// after the depth clear (`EntityRenderer.java`:1185-1191), over the whole
/// HUD/overlay group. The window hands this list to the screen pass — never
/// into the hud list — so the order survives.
///
/// A container screen draws the default background's gradient
/// (`GuiScreen.drawWorldBackground`:668-683 — the world-present arm), its
/// sheet blit, every slot's item, the hover highlight (the 0x80FFFFFF rect at
/// `GuiContainer.java`:134), the title lines, the carried stack at the
/// pointer minus 8 (`:149, :169`) with the drag's remnant preview, each
/// covered slot's preview count with its own white rect while a multi-slot
/// drag runs (`drawSlot`:243-303 — a lone covered slot draws nothing,
/// `:245-248`, and a capped one counts yellow), and last the hovered slot's
/// tooltip over the cursor.
///
/// The declared-but-unimplemented unknown kinds draw the generic frame —
/// the background and the title alone — until their tasks land their
/// tables (recorded). The inventory screen landed in Task 20, the creative
/// screen in Task 21, the sign editor in Task 22, the book reader in
/// Task 23.
pub fn screen_draws(screens: &Screens, input: &ScreenDrawInput<'_>) -> Vec<HudDraw> {
    let Some(screen) = screens.current() else {
        return Vec::new();
    };
    let mut draws = Vec::new();
    // The default background: the world-present gradient, one rect per half
    // (`GuiScreen.java`:668-683 paints top −1072689136 over bottom
    // −804253680 — 0xC0101010 over 0xD0101010).
    let width = input.scaled.width as f32;
    let height = input.scaled.height as f32;
    draws.push(HudDraw::Rect {
        x: 0.0,
        y: 0.0,
        width,
        height: height / 2.0,
        colour: [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 192.0 / 255.0],
    });
    draws.push(HudDraw::Rect {
        x: 0.0,
        y: height / 2.0,
        width,
        height: height - height / 2.0,
        colour: [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 208.0 / 255.0],
    });
    let container = match screen {
        ScreenState::Container(container) | ScreenState::Inventory(container) => container.as_ref(),
        ScreenState::Creative(creative) => {
            return push_creative_draws(draws, creative, input, width, height);
        }
        // The sign editor draws its own title, lines and Done button over
        // the background (Task 22); without a measured font the background
        // alone stands, like the titles that stay out. The free pointer
        // rides along so the Done button takes its hovered strip and tint
        // on the button (`GuiButton.drawButton`'s hovered arm).
        ScreenState::Sign(editor) => {
            if let Some(font) = input.font {
                draws.extend(editor.draws(font, &input.scaled, input.mouse));
            }
            return draws;
        }
        ScreenState::Book(reader) => {
            if let Some(font) = input.font {
                draws.extend(reader.draws(font, &input.scaled, input.mouse));
            }
            return draws;
        }
    };
    let layout = container.layout();
    let (gx, gy) = container.origin();
    let (gx, gy) = (gx as f32, gy as f32);
    // The background's sheet blits at the panel's top-left: the full panel,
    // or the chest's split pair (Task 18) — then the live property blits,
    // which the background layer draws before the slots
    // (`GuiFurnace.java`:44-55, `GuiBrewingStand.java`:52-82).
    for blit in family_a::background_blits(layout) {
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(layout.sheet),
            x: gx + blit.dx as f32,
            y: gy + blit.dy as f32,
            width: blit.w as f32,
            height: blit.h as f32,
            uv: [
                blit.sx as f32 / 256.0,
                blit.sy as f32 / 256.0,
                (blit.sx + blit.w) as f32 / 256.0,
                (blit.sy + blit.h) as f32 / 256.0,
            ],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
    }
    for blit in family_a::property_blits(container.kind(), container.properties()) {
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(layout.sheet),
            x: gx + blit.dx as f32,
            y: gy + blit.dy as f32,
            width: blit.w as f32,
            height: blit.h as f32,
            uv: [
                blit.sx as f32 / 256.0,
                blit.sy as f32 / 256.0,
                (blit.sx + blit.w) as f32 / 256.0,
                (blit.sy + blit.h) as f32 / 256.0,
            ],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
    }
    // The inventory's player preview over the sheet, before the slots: the
    // pointer-facing silhouette (`GuiInventory.drawScreen`:73-91).
    if matches!(screen, ScreenState::Inventory(_)) {
        push_inventory_preview(&mut draws, input, gx, gy);
    }
    // The family-B widgets over the sheet, before the slots: the background
    // layer's extras — the horse's panels, the anvil's strip and arrow, the
    // book, the offer rows, the beacon buttons, the villager pager — then the
    // slots, then the foreground labels.
    push_family_b_draws(&mut draws, container, input, gx, gy);
    // The slots in slot order, each cell's item through the icon seam. A
    // covered slot draws its preview count with the white rect; a lone
    // covered slot draws nothing at all (`drawSlot`:243-303).
    let drag_len = container.drag_slots().len();
    for pos in layout.slots {
        let x = gx + pos.x as f32;
        let y = gy + pos.y as f32;
        if let Some(preview) = container
            .preview()
            .iter()
            .find(|entry| entry.index == pos.index)
        {
            if drag_len == 1 {
                continue;
            }
            draws.push(HudDraw::Rect {
                x,
                y,
                width: 16.0,
                height: 16.0,
                colour: HOVER_COLOUR,
            });
            let template = container.cursor().cloned().unwrap_or(MetadataItem {
                id: 0,
                count: 0,
                damage: 0,
                nbt: None,
            });
            let stack = MetadataItem {
                count: preview.count.clamp(0, 255) as u8,
                ..template
            };
            draws.push(HudDraw::Item {
                stack: Some(item_icon(&stack)),
                x,
                y,
                pop: 0.0,
            });
            push_stack_overlay(
                &mut draws,
                &stack,
                preview_alt(stack.count, preview.capped).as_deref(),
                input.font,
                x,
                y,
            );
            continue;
        }
        let stack = container.slot_stack(pos.index).cloned().flatten();
        draws.push(HudDraw::Item {
            stack: stack.as_ref().map(item_icon),
            x,
            y,
            pop: 0.0,
        });
        if let Some(stack) = stack.as_ref() {
            push_stack_overlay(&mut draws, stack, None, input.font, x, y);
        }
    }
    // The hover highlight: the draw loop's last match with the
    // semi-transparent white rect (`GuiContainer.java`:126-138).
    if let Some(hovered) = container.hovered() {
        if let Some(pos) = layout.slots.iter().find(|pos| pos.index == hovered) {
            draws.push(HudDraw::Rect {
                x: gx + pos.x as f32,
                y: gy + pos.y as f32,
                width: 16.0,
                height: 16.0,
                colour: HOVER_COLOUR,
            });
        }
    }
    // The title lines over the panel (`drawGuiContainerForegroundLayer` —
    // the chest's pair, the inventory's label, the generic title). Without
    // a font the titles stay out while the sheet and items still draw.
    if input.font.is_some() {
        // The centred top line measures its display text through the same
        // font the draw measures with.
        let measure = |text: &str| input.font.map_or(0, |font| string_width(font, text));
        for line in container.title_lines(measure) {
            let text = plain_text(&chat::parse_json(&line.text));
            if !text.is_empty() {
                draws.push(HudDraw::Text {
                    text,
                    x: gx + line.x as f32,
                    y: gy + line.y as f32,
                    scale: 1.0,
                    // The line's own packed colour: the beacon's labels are
                    // grey, everything else the dark title grey.
                    colour: title_rgba(line.colour),
                    shadow: false,
                    blend: false,
                });
            }
        }
    }
    // The carried stack at the pointer minus 8, with the drag's remnant
    // preview and the yellow zero (`drawScreen`:144-170).
    if let Some(mouse) = input.mouse {
        if let Some(cursor) = container.cursor_draw(mouse) {
            draws.push(HudDraw::Item {
                stack: Some(item_icon(&cursor.stack)),
                x: cursor.x,
                y: cursor.y,
                pop: 0.0,
            });
            push_stack_overlay(
                &mut draws,
                &cursor.stack,
                cursor.alt_text.as_deref(),
                input.font,
                cursor.x,
                cursor.y,
            );
        }
    }
    // The hovered slot's tooltip, last over everything the screen drew: the
    // builder's lines through the draw assembly at the pointer
    // (`GuiContainer.drawScreen` renders the hovered stack's tooltip after
    // the cursor — and only with empty hands: `:190` renders it only when
    // `getItemStack() == null`, so the tooltip and the carried stack never
    // co-draw; the cursor stays `Some` through a port drag, so the one gate
    // covers the drag too). Without a font there is no width to place, so
    // the box stays out while the rest still draws.
    if let (Some(mouse), Some(font)) = (input.mouse, input.font) {
        if container.cursor().is_none() {
            // The family-B hover lines first: a beacon button or an
            // enchanting offer under the pointer owns the tooltip — the rows
            // never overlap a slot, so the slot's tooltip is the fallback.
            if let Some(lines) = family_b_tooltip(container, input, gx, gy) {
                draws.extend(tooltip::tooltip_draws(
                    &tooltip::plain_tooltip_lines(lines),
                    font,
                    mouse,
                    (width, height),
                ));
            } else if let Some(hovered) = container.hovered() {
                if let Some(stack) = container.slot_stack(hovered).cloned().flatten() {
                    let lines = tooltip::tooltip_lines(&stack, input.advanced);
                    draws.extend(tooltip::tooltip_draws(&lines, font, mouse, (width, height)));
                }
            }
        }
    }
    // The inventory's effects overlay, last over everything the screen drew
    // (`InventoryEffectRenderer.drawScreen`:47-55 draws the list after the
    // container). Hidden entirely with no effects.
    if matches!(screen, ScreenState::Inventory(_)) {
        push_inventory_effects(&mut draws, container, input, gx, gy);
    }
    draws
}

/// The creative screen's draws (`GuiContainerCreative.drawScreen` and
/// `drawGuiContainerBackgroundLayer`): the unselected tabs first, the panel
/// sheet second, the selected tab last (`:682-706`), the twelve tab icons
/// over the strip (`:817-828`), the title line (`:392-400`), the slots —
/// the 9×5 page plus the hotbar row, or the survival layout with the delete
/// cell on the inventory tab — the hover highlight, the scrollbar thumb
/// (`:696-704`), the search field's text, the carried stack at the pointer
/// and last the hovered tooltip.
fn push_creative_draws(
    mut draws: Vec<HudDraw>,
    screen: &CreativeScreen,
    input: &ScreenDrawInput<'_>,
    width: f32,
    height: f32,
) -> Vec<HudDraw> {
    use oxide_client::screens::creative as cr;
    let selected = screen.selected_tab();
    let tab =
        items::CreativeTab::from_index(selected).unwrap_or(items::CreativeTab::BuildingBlocks);
    let (gx, gy) = screen.origin();
    let (gx, gy) = (gx as f32, gy as f32);
    // The strip: every unselected tab, then the panel, then the selected
    // tab (`func_147051_a`'s order, `:682-706`) — one sheet blit each, in
    // the 256-wide sheet space the container path reads.
    for index in 0..cr::TAB_COUNT {
        if index == selected {
            continue;
        }
        let (sx, sy) = cr::tab_sprite(index);
        let (u, v) = cr::tab_uv(index, false);
        sheet_blit(
            &mut draws,
            cr::TABS_SHEET,
            gx,
            gy,
            sx,
            sy,
            cr::TAB_W,
            cr::TAB_H,
            u,
            v,
        );
    }
    sheet_blit(
        &mut draws,
        cr::panel_sheet(tab),
        gx,
        gy,
        0,
        0,
        cr::FRAME_W,
        cr::FRAME_H,
        0,
        0,
    );
    {
        let (sx, sy) = cr::tab_sprite(selected);
        let (u, v) = cr::tab_uv(selected, true);
        sheet_blit(
            &mut draws,
            cr::TABS_SHEET,
            gx,
            gy,
            sx,
            sy,
            cr::TAB_W,
            cr::TAB_H,
            u,
            v,
        );
    }
    // The twelve tab icons over the strip (`:817-828`).
    for index in 0..cr::TAB_COUNT {
        let icon_tab =
            items::CreativeTab::from_index(index).unwrap_or(items::CreativeTab::BuildingBlocks);
        let icon = cr::entry_stack(icon_tab.icon());
        let (ix, iy) = cr::tab_icon_pos(index);
        draws.push(HudDraw::Item {
            stack: Some(item_icon(&icon)),
            x: gx + ix as f32,
            y: gy + iy as f32,
            pop: 0.0,
        });
    }
    // The title line over the panel (`:392-400`) — none on the inventory
    // tab — untranslated: key resolution is a locale-table concern the
    // port does not carry.
    if input.font.is_some() {
        if let Some(title) = cr::tab_title(selected) {
            draws.push(HudDraw::Text {
                text: title,
                x: gx + 8.0,
                y: gy + 6.0,
                scale: 1.0,
                colour: title_rgba(4210752),
                shadow: false,
                blend: false,
            });
        }
    }
    // The slots: the page plus the hotbar row, or the survival layout with
    // the delete cell (`:512` — the bin shows the display's tmp index 0,
    // the page's first cell).
    if selected == cr::INVENTORY_TAB {
        for slot in 5..45_i16 {
            let stack = screen.player_slot(slot).cloned().flatten();
            let (sx, sy) = cr::inventory_slot_pos(slot);
            push_creative_cell(
                &mut draws,
                input.font,
                gx + sx as f32,
                gy + sy as f32,
                &stack,
            );
        }
        push_creative_cell(
            &mut draws,
            input.font,
            gx + cr::BIN_DX as f32,
            gy + cr::BIN_DY as f32,
            &screen.grid()[0],
        );
    } else {
        for (cell, stack) in screen.grid().iter().enumerate() {
            let x = gx
                + cr::GRID_LEFT as f32
                + (cell % cr::GRID_COLS as usize) as f32 * cr::CELL_STEP as f32;
            let y = gy
                + cr::GRID_TOP as f32
                + (cell / cr::GRID_COLS as usize) as f32 * cr::CELL_STEP as f32;
            push_creative_cell(&mut draws, input.font, x, y, stack);
        }
        for (k, stack) in screen.hotbar().iter().enumerate() {
            let x = gx + cr::GRID_LEFT as f32 + k as f32 * cr::CELL_STEP as f32;
            push_creative_cell(&mut draws, input.font, x, y_hotbar(gy), stack);
        }
    }
    // The hover highlight over the hovered cell (`GuiContainer.java`
    // :126-138's rect, read through the creative hit test).
    if let Some(mouse) = input.mouse {
        let rect = match screen.hover_at(mouse.0 - gx, mouse.1 - gy) {
            cr::Hover::Grid(cell) => {
                let col = (cell % cr::GRID_COLS as usize) as i32;
                let row = (cell / cr::GRID_COLS as usize) as i32;
                Some((
                    cr::GRID_LEFT + col * cr::CELL_STEP,
                    cr::GRID_TOP + row * cr::CELL_STEP,
                ))
            }
            cr::Hover::Hotbar(k) => {
                Some((cr::GRID_LEFT + k as i32 * cr::CELL_STEP, cr::HOTBAR_TOP))
            }
            cr::Hover::Player(slot) => {
                let (x, y) = cr::inventory_slot_pos(slot);
                Some((x, y))
            }
            cr::Hover::Bin => Some((cr::BIN_DX, cr::BIN_DY)),
            cr::Hover::Tab(_) | cr::Hover::Track | cr::Hover::Panel | cr::Hover::None => None,
        };
        if let Some((x, y)) = rect {
            draws.push(HudDraw::Rect {
                x: gx + x as f32,
                y: gy + y as f32,
                width: 16.0,
                height: 16.0,
                colour: HOVER_COLOUR,
            });
        }
    }
    // The scrollbar thumb over the panel (`:696-704`), on every tab but
    // the inventory one — the 232 slice while the list scrolls, 244 while
    // it fits.
    if let Some((tx, ty)) = cr::thumb_rect(screen.scroll(), selected) {
        sheet_blit(
            &mut draws,
            cr::TABS_SHEET,
            gx,
            gy,
            tx,
            ty,
            cr::THUMB_W,
            cr::THUMB_H,
            cr::thumb_sheet_x(screen.list_len(), selected),
            0,
        );
    }
    // The search field's text: the borderless white line at the field's
    // own origin (`initGui`:263-280).
    if screen.search_visible() && input.font.is_some() {
        draws.push(HudDraw::Text {
            text: screen.search_text().to_string(),
            x: gx + cr::SEARCH_DX as f32,
            y: gy + cr::SEARCH_DY as f32,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: false,
            blend: false,
        });
    }
    // The carried stack at the pointer minus 8 (`drawScreen`:144-170).
    if let Some(mouse) = input.mouse {
        if let Some(cursor) = screen.cursor_draw(mouse) {
            draws.push(HudDraw::Item {
                stack: Some(item_icon(&cursor.0)),
                x: cursor.1,
                y: cursor.2,
                pop: 0.0,
            });
            push_stack_overlay(&mut draws, &cursor.0, None, input.font, cursor.1, cursor.2);
        }
    }
    // The hovered cell's tooltip, last over everything the screen drew —
    // the delete slot's own line on the inventory tab (`:613-616`), else
    // the hovered stack's lines — and only with empty hands, so the
    // tooltip and the carried stack never co-draw.
    if let (Some(mouse), Some(font)) = (input.mouse, input.font) {
        if screen.cursor().is_none() {
            let hover = screen.hover_at(mouse.0 - gx, mouse.1 - gy);
            if hover == cr::Hover::Bin {
                draws.extend(tooltip::tooltip_draws(
                    &tooltip::plain_tooltip_lines(vec![String::from("inventory.binSlot")]),
                    font,
                    mouse,
                    (width, height),
                ));
            } else {
                let stack = match hover {
                    cr::Hover::Grid(cell) => screen.grid().get(cell).cloned().flatten(),
                    cr::Hover::Hotbar(k) => screen.hotbar().get(k).cloned().flatten(),
                    cr::Hover::Player(slot) => screen.player_slot(slot).cloned().flatten(),
                    _ => None,
                };
                if let Some(stack) = stack {
                    let lines = tooltip::tooltip_lines(&stack, input.advanced);
                    draws.extend(tooltip::tooltip_draws(&lines, font, mouse, (width, height)));
                }
            }
        }
    }
    draws
}

/// The hotbar row's top in frame units: the row rides with the frame.
fn y_hotbar(gy: f32) -> f32 {
    use oxide_client::screens::creative as cr;
    gy + cr::HOTBAR_TOP as f32
}

/// One creative cell's item with its count/durability overlay, mirroring
/// the container path's slot draws.
fn push_creative_cell(
    draws: &mut Vec<HudDraw>,
    font: Option<&Font>,
    x: f32,
    y: f32,
    stack: &Option<MetadataItem>,
) {
    draws.push(HudDraw::Item {
        stack: stack.as_ref().map(item_icon),
        x,
        y,
        pop: 0.0,
    });
    if let Some(stack) = stack.as_ref() {
        push_stack_overlay(draws, stack, None, font, x, y);
    }
}

/// The inventory's player preview: the skin-face silhouette centred on the
/// derived anchor, facing the pointer (`GuiInventory.drawScreen`:73-91 and
/// `drawEntityOnScreen`:96-134).
///
/// The entry is a GUI projection through the screen pass, not the entity
/// pass (recorded in [`inventory`]): the anchor/scale, the per-frame facing
/// angles from the pointer and the own player's skin face sampled flat —
/// the source disables world lighting for the preview, which the flat
/// sample honours by construction. The facing is derived here so the posed
/// projection reads it; the flat quad itself stays unrotated. Before the
/// first pointer move the preview faces forward (the pointer reads as the
/// anchor, both arms zero).
fn push_inventory_preview(draws: &mut Vec<HudDraw>, input: &ScreenDrawInput, gx: f32, gy: f32) {
    let Some(skin) = input.preview_skin else {
        return;
    };
    let (ax, ay) = inventory::preview_anchor(gx as i32, gy as i32);
    let (ax, ay) = (ax as f32, ay as f32);
    let (pointer_x, pointer_y) = input.mouse.unwrap_or((ax, ay));
    let (_yaw, _offset, _pitch) = inventory::preview_facing(pointer_x, pointer_y, ax, ay);
    let half = inventory::PREVIEW_SILHOUETTE / 2.0;
    // The face, then the hat overlay — the tab list's own pair over the same
    // skin space (`TAB_FACE_UV`/`TAB_HAT_UV`).
    for uv in [TAB_FACE_UV, TAB_HAT_UV] {
        draws.push(HudDraw::SkinRect {
            texture: skin,
            x: ax - half,
            y: ay - half,
            width: inventory::PREVIEW_SILHOUETTE,
            height: inventory::PREVIEW_SILHOUETTE,
            uv,
            colour: [1.0, 1.0, 1.0, 1.0],
        });
    }
}

/// The inventory's effects overlay: one row per live effect, ascending by id
/// — the source iterates a `HashMap` (no order), the port's deterministic
/// order is the recorded divergence — each on the row sprite with the status
/// icon and the name/duration pens (`InventoryEffectRenderer.java`:60-109).
/// Hidden entirely with no effects; an id outside the potion table draws no
/// row (recorded). Without a font the rows and icons still draw while the
/// texts stay out.
fn push_inventory_effects(
    draws: &mut Vec<HudDraw>,
    container: &ContainerScreen,
    input: &ScreenDrawInput,
    gx: f32,
    gy: f32,
) {
    let sheet = container.layout().sheet;
    let mut rows: Vec<&StatusEffect> = input.effects.iter().collect();
    rows.sort_by_key(|effect| effect.effect_id);
    let mut named: Vec<(&StatusEffect, &str)> = Vec::with_capacity(rows.len());
    for effect in rows {
        if let Some(row) = items::potion_name(effect.effect_id) {
            named.push((effect, row.name));
        }
    }
    if named.is_empty() {
        return;
    }
    let step = inventory::effect_step(named.len());
    let [row_sx, row_sy, row_w, row_h] = inventory::EFFECT_ROW_RECT;
    for (row, (effect, base)) in named.iter().enumerate() {
        let dy = row as i32 * step;
        sheet_blit(
            draws,
            sheet,
            gx,
            gy,
            inventory::EFFECT_ROW_DX,
            dy,
            row_w,
            row_h,
            row_sx,
            row_sy,
        );
        if let Some((col, icon_row)) = inventory::potion_icon_uv(effect.effect_id) {
            sheet_blit(
                draws,
                sheet,
                gx,
                gy,
                inventory::EFFECT_ROW_DX + inventory::EFFECT_ICON_DX,
                dy + inventory::EFFECT_ICON_DY,
                inventory::EFFECT_ICON_SIZE,
                inventory::EFFECT_ICON_SIZE,
                col * inventory::EFFECT_ICON_SIZE,
                inventory::EFFECT_ICON_TOP + icon_row * inventory::EFFECT_ICON_SIZE,
            );
        }
        if input.font.is_some() {
            panel_text(
                draws,
                inventory::effect_name(base, effect.amplifier),
                gx + (inventory::EFFECT_ROW_DX + inventory::EFFECT_NAME_DX) as f32,
                gy + dy as f32 + inventory::EFFECT_NAME_DY as f32,
                title_rgba(inventory::EFFECT_NAME_COLOUR),
                true,
            );
            panel_text(
                draws,
                inventory::effect_duration(effect.duration),
                gx + (inventory::EFFECT_ROW_DX + inventory::EFFECT_NAME_DX) as f32,
                gy + dy as f32 + inventory::EFFECT_DURATION_DY as f32,
                title_rgba(inventory::EFFECT_DURATION_COLOUR),
                true,
            );
        }
    }
}

/// One sheet blit over the panel at the centred origin.
#[allow(clippy::too_many_arguments)]
fn sheet_blit(
    draws: &mut Vec<HudDraw>,
    sheet: &'static str,
    gx: f32,
    gy: f32,
    dx: i32,
    dy: i32,
    w: i32,
    h: i32,
    sx: i32,
    sy: i32,
) {
    draws.push(HudDraw::TexturedRect {
        texture: HudTexture::Named(sheet),
        x: gx + dx as f32,
        y: gy + dy as f32,
        width: w as f32,
        height: h as f32,
        uv: [
            sx as f32 / 256.0,
            sy as f32 / 256.0,
            (sx + w) as f32 / 256.0,
            (sy + h) as f32 / 256.0,
        ],
        colour: [1.0, 1.0, 1.0, 1.0],
    });
}

/// One unblended label over the panel.
fn panel_text(
    draws: &mut Vec<HudDraw>,
    text: String,
    x: f32,
    y: f32,
    colour: [f32; 4],
    shadow: bool,
) {
    draws.push(HudDraw::Text {
        text,
        x,
        y,
        scale: 1.0,
        colour,
        shadow,
        blend: false,
    });
}

/// The family-B widgets over the sheet (`screen_draws` calls this between the
/// property blits and the slots, where the background layer paints). Every
/// rect is panel units; the pointer reads `input.mouse` in screen units, so
/// the hover arms subtract the centred origin first.
fn push_family_b_draws(
    draws: &mut Vec<HudDraw>,
    container: &ContainerScreen,
    input: &ScreenDrawInput,
    gx: f32,
    gy: f32,
) {
    use oxide_proto_v47::window::WindowKind;
    let kind = container.kind();
    let props = container.properties();
    let prop = |index: usize| props.get(index).copied().unwrap_or(0) as i32;
    let present = |index: u8| {
        container
            .slot_stack(index.into())
            .is_some_and(|slot| slot.is_some())
    };
    let mouse = input.mouse.map(|(x, y)| (x - gx, y - gy));
    let cell = mouse.map(|(x, y)| (x as i32, y as i32));
    let sheet = container.layout().sheet;
    match kind {
        WindowKind::EntityHorse => {
            // The chested panel rides the slot count's layout; the armour
            // frame the stood-up state (always true — the live horse never
            // reaches the screens — recorded in `HorseState`).
            if std::ptr::eq(container.layout(), &family_b::HORSE_CHESTED) {
                let (dx, dy, w, h) = family_b::HORSE_CHEST_RECT;
                let (sx, sy) = family_b::HORSE_CHEST_UV;
                sheet_blit(draws, sheet, gx, gy, dx, dy, w, h, sx, sy);
            }
            if let family_b::FamilyState::Horse(state) = container.family() {
                if state.armoured {
                    let (dx, dy, w, h) = family_b::HORSE_ARMOUR_RECT;
                    let (sx, sy) = family_b::HORSE_ARMOUR_UV;
                    sheet_blit(draws, sheet, gx, gy, dx, dy, w, h, sx, sy);
                }
            }
        }
        WindowKind::Anvil => {
            let (dx, dy, w, h) = family_b::ANVIL_STRIP_RECT;
            sheet_blit(
                draws,
                sheet,
                gx,
                gy,
                dx,
                dy,
                w,
                h,
                0,
                family_b::anvil_strip_v(present(0)),
            );
            if family_b::anvil_arrow_broken(present(0), present(1), present(2)) {
                let (ax, ay, aw, ah) = family_b::ANVIL_ARROW_RECT;
                let (asx, asy) = family_b::ANVIL_ARROW_UV;
                sheet_blit(draws, sheet, gx, gy, ax, ay, aw, ah, asx, asy);
            }
            if let Some(font) = input.font {
                let maximum = prop(0);
                if let Some(cost) =
                    family_b::anvil_cost(maximum, false, present(2), input.level >= maximum)
                {
                    let text = cost.value.to_string();
                    let width = string_width(font, &text);
                    panel_text(
                        draws,
                        text,
                        gx + family_b::anvil_cost_x(width) as f32,
                        gy + family_b::ANVIL_COST_Y as f32,
                        title_rgba(cost.colour),
                        true,
                    );
                }
                if let family_b::FamilyState::Anvil(field) = container.family() {
                    let enabled = family_b::name_enabled(present(0));
                    panel_text(
                        draws,
                        field.text().to_string(),
                        gx + 62.0,
                        gy + 24.0,
                        title_rgba(if enabled { 14_737_632 } else { 7_368_816 }),
                        false,
                    );
                    if enabled && field.is_focused() && field.cursor_visible() {
                        let before = field.text().get(..field.cursor()).unwrap_or("");
                        draws.push(HudDraw::Rect {
                            x: gx
                                + 62.0
                                + family_b::NAME_CURSOR_DX as f32
                                + string_width(font, before) as f32,
                            y: gy + 24.0 + family_b::NAME_CURSOR_DY as f32,
                            width: family_b::NAME_CURSOR_W as f32,
                            height: family_b::NAME_CURSOR_H as f32,
                            colour: family_b::NAME_CURSOR_COLOUR,
                        });
                    }
                }
            }
        }
        WindowKind::EnchantingTable => {
            let costs = [prop(0), prop(1), prop(2)];
            let lapis = container
                .slot_stack(1)
                .and_then(|slot| slot.as_ref())
                .map(|stack| i32::from(stack.count))
                .unwrap_or(0);
            let seed = prop(3);
            let hovered = cell.and_then(|(x, y)| {
                (0..3i32).find(|k| {
                    (family_b::ENCHANT_ROW_X..family_b::ENCHANT_ROW_X + 108).contains(&x)
                        && y >= family_b::enchant_row_y(*k)
                        && y < family_b::enchant_row_y(*k) + 19
                })
            });
            for (index, cost) in costs.iter().enumerate() {
                let row = index as i32;
                let face =
                    family_b::enchant_face(index, *cost, lapis, input.level, hovered == Some(row));
                sheet_blit(
                    draws,
                    sheet,
                    gx,
                    gy,
                    family_b::ENCHANT_ROW_X,
                    family_b::enchant_row_y(row),
                    108,
                    19,
                    0,
                    face.bg_v,
                );
                if let Some(clasp_v) = face.clasp_v {
                    let (cx, cy) = family_b::enchant_clasp_pos(row);
                    sheet_blit(
                        draws,
                        sheet,
                        gx,
                        gy,
                        cx,
                        cy,
                        16,
                        16,
                        family_b::enchant_clasp_u(row),
                        clasp_v,
                    );
                }
                if *cost > 0 {
                    if let Some(font) = input.font {
                        panel_text(
                            draws,
                            family_b::glyph_word_at(seed, index),
                            gx + 80.0,
                            gy + family_b::enchant_glyph_y(row) as f32,
                            title_rgba(face.glyph),
                            false,
                        );
                        let number = family_b::enchant_cost_text(*cost);
                        let width = string_width(font, &number);
                        panel_text(
                            draws,
                            number,
                            gx + family_b::enchant_cost_x(width) as f32,
                            gy + family_b::enchant_cost_y(row) as f32,
                            title_rgba(face.cost),
                            true,
                        );
                    }
                }
            }
            // The book's 2D stand-in: the cover always, the two page rects
            // opening from the spine by the ticked open amount.
            if let family_b::FamilyState::Enchanting(book) = container.family() {
                let (open, flip_a, flip_b) = book.frame(1.0);
                let (cx, cy, cw, ch) = family_b::BOOK_RECT;
                draws.push(HudDraw::Rect {
                    x: gx + cx as f32,
                    y: gy + cy as f32,
                    width: cw as f32,
                    height: ch as f32,
                    colour: title_rgba(family_b::BOOK_COLOUR),
                });
                let half = family_b::BOOK_PAGE_W as f32 * open;
                let left = (half * (1.0 - flip_a)) as i32;
                if left > 0 {
                    draws.push(HudDraw::Rect {
                        x: gx + (88 - left) as f32,
                        y: gy + 2.0,
                        width: left as f32,
                        height: 10.0,
                        colour: title_rgba(family_b::BOOK_PAGE_COLOUR),
                    });
                }
                let right = (half * (1.0 - flip_b)) as i32;
                if right > 0 {
                    draws.push(HudDraw::Rect {
                        x: gx + 88.0,
                        y: gy + 2.0,
                        width: right as f32,
                        height: 10.0,
                        colour: title_rgba(family_b::BOOK_PAGE_COLOUR),
                    });
                }
            }
        }
        WindowKind::Beacon => {
            // The selection seeds from the window's properties and local row
            // clicks update it between snapshots (the seam reseeds on every
            // snapshot — recorded in `Screens::apply_snapshot`).
            let selection = match container.family() {
                family_b::FamilyState::Beacon(selection) => *selection,
                _ => family_b::BeaconSelection {
                    primary: prop(1),
                    secondary: prop(2),
                },
            };
            let rows = family_b::beacon_rows(prop(0), selection.primary, selection.secondary);
            let hovered = cell.and_then(|(x, y)| family_b::beacon_hit(&rows, x, y));
            for row in &rows {
                sheet_blit(
                    draws,
                    sheet,
                    gx,
                    gy,
                    row.x,
                    row.y,
                    22,
                    22,
                    family_b::beacon_button_u(row.enabled, row.selected, hovered == Some(row.id)),
                    family_b::BEACON_STRIP_V,
                );
                if let Some(icon) = family_b::potion_icon(row.effect) {
                    let (sx, sy) = family_b::potion_icon_uv(icon);
                    sheet_blit(
                        draws,
                        family_b::INVENTORY_SHEET,
                        gx,
                        gy,
                        row.x + 2,
                        row.y + 2,
                        18,
                        18,
                        sx,
                        sy,
                    );
                }
            }
            let (mx, my) = cell.unwrap_or((-1, -1));
            let confirm_on = family_b::beacon_confirm_enabled(present(0), selection.primary);
            for (pos, icon, enabled) in [
                (
                    family_b::BEACON_CONFIRM_POS,
                    family_b::BEACON_CONFIRM_UV,
                    confirm_on,
                ),
                (
                    family_b::BEACON_CANCEL_POS,
                    family_b::BEACON_CANCEL_UV,
                    true,
                ),
            ] {
                let hov = mx >= pos.0
                    && mx < pos.0 + family_b::BEACON_BUTTON_SIDE
                    && my >= pos.1
                    && my < pos.1 + family_b::BEACON_BUTTON_SIDE;
                sheet_blit(
                    draws,
                    sheet,
                    gx,
                    gy,
                    pos.0,
                    pos.1,
                    family_b::BEACON_BUTTON_SIDE,
                    family_b::BEACON_BUTTON_SIDE,
                    family_b::beacon_button_u(enabled, false, hov),
                    family_b::BEACON_STRIP_V,
                );
                sheet_blit(
                    draws,
                    sheet,
                    gx,
                    gy,
                    pos.0 + 2,
                    pos.1 + 2,
                    18,
                    18,
                    icon.0,
                    icon.1,
                );
            }
        }
        WindowKind::Villager => {
            if let family_b::FamilyState::Villager(pager) = container.family() {
                if pager.offers.len() > 1 {
                    let (mx, my) = cell.unwrap_or((-1, -1));
                    for (pos, forward) in [
                        (family_b::VILLAGER_NEXT_POS, true),
                        (family_b::VILLAGER_PREV_POS, false),
                    ] {
                        let (sx, sy) = family_b::merchant_button_uv(
                            family_b::pager_enabled(pager.selected, pager.offers.len(), forward),
                            family_b::pager_hit(pos, mx, my),
                            forward,
                        );
                        sheet_blit(
                            draws,
                            sheet,
                            gx,
                            gy,
                            pos.0,
                            pos.1,
                            family_b::VILLAGER_BUTTON_W,
                            family_b::VILLAGER_BUTTON_H,
                            sx,
                            sy,
                        );
                    }
                }
                // The red X over both arrow lanes while the shown recipe is
                // disabled (`:160-164` draws both rects).
                if pager.current().is_some_and(|offer| offer.is_disabled()) {
                    for (_, ay) in [(83, 21), (83, 51)] {
                        let (sx, sy, sw, sh) = family_b::VILLAGER_RED_X_UV;
                        sheet_blit(draws, sheet, gx, gy, 83, ay, sw, sh, sx, sy);
                    }
                }
            }
        }
        _ => {}
    }
}

/// The family-B hover lines, if the pointer stands on a beacon button or an
/// enchanting offer with empty hands (`screen_draws` asks this before the
/// slot's own tooltip — the rows never overlap a slot).
fn family_b_tooltip(
    container: &ContainerScreen,
    input: &ScreenDrawInput,
    gx: f32,
    gy: f32,
) -> Option<Vec<String>> {
    use oxide_proto_v47::window::WindowKind;
    let (mx, my) = input
        .mouse
        .map(|(x, y)| ((x - gx) as i32, (y - gy) as i32))?;
    let props = container.properties();
    let prop = |index: usize| props.get(index).copied().unwrap_or(0) as i32;
    match container.kind() {
        WindowKind::Beacon => {
            let selection = match container.family() {
                family_b::FamilyState::Beacon(selection) => *selection,
                _ => family_b::BeaconSelection {
                    primary: prop(1),
                    secondary: prop(2),
                },
            };
            let rows = family_b::beacon_rows(prop(0), selection.primary, selection.secondary);
            if let Some(id) = family_b::beacon_hit(&rows, mx, my) {
                return family_b::beacon_tooltip(&rows, id).map(|tip| vec![tip]);
            }
            let side = family_b::BEACON_BUTTON_SIDE;
            for (pos, text) in [
                (family_b::BEACON_CONFIRM_POS, family_b::BEACON_DONE_TEXT),
                (family_b::BEACON_CANCEL_POS, family_b::BEACON_CANCEL_TEXT),
            ] {
                if mx >= pos.0 && mx < pos.0 + side && my >= pos.1 && my < pos.1 + side {
                    return Some(vec![String::from(text)]);
                }
            }
            None
        }
        WindowKind::EnchantingTable => {
            let lapis = container
                .slot_stack(1)
                .and_then(|slot| slot.as_ref())
                .map(|stack| i32::from(stack.count))
                .unwrap_or(0);
            for row in 0..3i32 {
                // The tooltip's lane is 17 tall, two shorter than the click's
                // (`drawScreen`:244 tests `108, 17`).
                if (family_b::ENCHANT_ROW_X..family_b::ENCHANT_ROW_X + 108).contains(&mx)
                    && my >= family_b::enchant_row_y(row)
                    && my < family_b::enchant_row_y(row) + 17
                {
                    let cost = prop(row as usize);
                    let clue = prop(4 + row as usize);
                    if cost > 0 && clue >= 0 {
                        let name = family_b::enchant_clue_name(clue);
                        return Some(family_b::enchant_tooltip(
                            name.as_deref(),
                            true,
                            row as usize,
                            cost,
                            lapis,
                            input.level,
                        ));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// A covered slot's overlay text: past the cap the count draws as the yellow
/// cap (`drawSlot`'s capped branch at `GuiContainer.java`:253-264 states `s`
/// as `YELLOW + cap`); inside the cap the count draws through the usual
/// white path, so there is no alt text.
fn preview_alt(count: u8, capped: bool) -> Option<String> {
    capped.then(|| format!("§e{count}"))
}

/// One stack's count and durability overlay at the cell (`x`, `y`)
/// (`RenderItem.renderItemOverlayIntoGUI`:455-494): the count at
/// `(x + 19 − 2 − width, y + 6 + 3)` — hidden at exactly one, red below one,
/// or the given alt text verbatim (the drag's yellow zero) — unblended with
/// shadow, then the durability bar's black bed, underlay and ramp at
/// `(x + 2, y + 13)`.
fn push_stack_overlay(
    draws: &mut Vec<HudDraw>,
    stack: &MetadataItem,
    alt_text: Option<&str>,
    font: Option<&Font>,
    x: f32,
    y: f32,
) {
    let text = match alt_text {
        Some(text) => Some(text.to_string()),
        None => count_text(stack),
    };
    if let (Some(text), Some(font)) = (text, font) {
        let width = string_width(font, &text) as f32;
        draws.push(HudDraw::Text {
            text,
            x: x + 17.0 - width,
            y: y + 9.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: false,
        });
    }
    if stack_damaged(stack) {
        if let Some(entry) = items::item_entry(stack.id) {
            let (fill, ramp) = durability_terms(stack.damage, entry.max_damage);
            draws.push(HudDraw::Rect {
                x: x + 2.0,
                y: y + 13.0,
                width: 13.0,
                height: 2.0,
                colour: [0.0, 0.0, 0.0, 1.0],
            });
            draws.push(HudDraw::Rect {
                x: x + 2.0,
                y: y + 13.0,
                width: 12.0,
                height: 1.0,
                colour: [((255 - ramp) / 4) as f32 / 255.0, 64.0 / 255.0, 0.0, 1.0],
            });
            draws.push(HudDraw::Rect {
                x: x + 2.0,
                y: y + 13.0,
                width: fill as f32,
                height: 1.0,
                colour: [(255 - ramp) as f32 / 255.0, ramp as f32 / 255.0, 0.0, 1.0],
            });
        }
    }
}

/// The popup text's alpha byte: `k = (int)(ticks × 256/10)`, clamped to 255
/// (`GuiIngame.java`:473-477). Forty ticks give 1024 and ten give 256 — both
/// clamp — nine gives 230, two 51 and one 25; the truncating float-to-int cast
/// is the source's own.
pub(crate) fn popup_alpha(ticks: u8) -> u8 {
    let k = (f32::from(ticks) * 256.0 / POPUP_ALPHA_SPAN) as u32;
    k.min(255) as u8
}

/// The pop's scale pair for `f = animationsToGo − partialTicks`
/// (`GuiIngame.renderHotbarItem`:1043-1051): `f1 = 1 + f/5` scales `(1/f1,
/// (f1+1)/2)` about the pivot — five ticks give (0.5, 1.5), two and a half
/// (0.6667, 1.25) — and a spent pop (`f ≤ 0`) draws unscaled. The render pass
/// carries the live matrix; this states the curve the suite pins.
#[allow(dead_code)]
pub(crate) fn pop_factors(f: f32) -> Option<(f32, f32)> {
    if f <= 0.0 {
        return None;
    }
    let f1 = 1.0 + f / POP_DIVISOR;
    Some((1.0 / f1, (f1 + 1.0) / 2.0))
}

/// The root compound of a stack's NBT tail, when the tail parses as one; an
/// unparseable tail carries no compound (the port reads the wire bytes the
/// session parsed, and a tail that is not a compound answers no key).
fn root_compound(nbt: &[u8]) -> Option<Vec<(String, NbtValue)>> {
    match oxide_proto_v47::nbt::parse(nbt) {
        Ok(NbtValue::Compound(children)) => Some(children),
        _ => None,
    }
}

/// Whether the stack's tail marks it `Unbreakable`
/// (`ItemStack.isItemStackDamageable`:252 — `getBoolean("Unbreakable")`).
fn stack_unbreakable(stack: &MetadataItem) -> bool {
    let Some(nbt) = stack.nbt.as_deref() else {
        return false;
    };
    let Some(children) = root_compound(nbt) else {
        return false;
    };
    children.iter().any(|(name, value)| {
        name == "Unbreakable" && matches!(value, NbtValue::Byte(flag) if *flag != 0)
    })
}

/// Whether the stack is damageable: a positive `maxDamage` and no `Unbreakable`
/// tag (`ItemStack.isItemStackDamageable`:252). An id outside the registry is
/// not damageable — the source's null-item arm.
fn stack_damageable(stack: &MetadataItem) -> bool {
    items::item_entry(stack.id).is_some_and(|entry| entry.max_damage > 0)
        && !stack_unbreakable(stack)
}

/// The popup's same-stack comparison (`GuiIngame.updateTick`:1097): the item,
/// the exact NBT tags (`ItemStack.areItemStackTagsEqual`:426-429 — the wire
/// bytes' own equality, which implies the compounds') and — for non-damageable
/// stacks only — the metadata. A damageable stack's damage short-circuits true,
/// so wearing a tool never resets the popup.
fn same_popup_stack(current: &MetadataItem, previous: &MetadataItem) -> bool {
    current.id == previous.id
        && current.nbt == previous.nbt
        && (stack_damageable(current) || current.damage == previous.damage)
}

/// The stack's NBT display name, when its tail names one (`ItemStack.getDisplayName`
/// :578-590 over `hasDisplayName`:639-642 — the `display.Name` string).
fn nbt_display_name(stack: &MetadataItem) -> Option<String> {
    let nbt = stack.nbt.as_deref()?;
    let children = root_compound(nbt)?;
    let (_, NbtValue::Compound(display)) = children.iter().find(|(name, _)| name == "display")?
    else {
        return None;
    };
    display
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("Name", NbtValue::String(name)) => Some(name.clone()),
            _ => None,
        })
}

/// The name the popup draws for one stack: the NBT name when the stack has one
/// (italicised, `EnumChatFormatting.ITALIC + s`, `GuiIngame.java`:460-463), else
/// the damage variant's own name (the wool colours, the potions — the registry's
/// sub-item rows), else the registration's name. An id the registry does not
/// know has no name, and the popup stays out for it.
fn popup_text(stack: &MetadataItem) -> Option<String> {
    if let Some(name) = nbt_display_name(stack) {
        return Some(format!("§o{name}"));
    }
    if let Some(variant) = items::sub_items(stack.id)
        .iter()
        .find(|item| item.damage == stack.damage)
    {
        return Some(variant.name.to_string());
    }
    items::item_entry(stack.id).map(|entry| entry.name.to_string())
}

/// The count the overlay draws for one stack, or nothing at a lone stack
/// (`RenderItem.renderItemOverlayIntoGUI`:459-466 — the hotbar passes no text,
/// so the count hides at exactly one; below one it draws red).
fn count_text(stack: &MetadataItem) -> Option<String> {
    if stack.count == 1 {
        return None;
    }
    if stack.count < 1 {
        Some(format!("§c{}", stack.count))
    } else {
        Some(stack.count.to_string())
    }
}

/// Whether the stack draws the durability bar: damageable and damaged
/// (`RenderItem.java`:476 over `ItemStack.isItemDamaged`:263-266).
fn stack_damaged(stack: &MetadataItem) -> bool {
    stack_damageable(stack) && stack.damage > 0
}

/// The durability bar's fill width and ramp value for a damaged stack
/// (`RenderItem.java`:478-479): `j = round(13 − damage·13/maxDamage)`,
/// `i = round(255 − damage·255/maxDamage)` — damage 780 of 1561 gives `j = 7`,
/// `i = 128`.
fn durability_terms(damage: i16, max_damage: i16) -> (i32, i32) {
    let damage = f64::from(damage);
    let max = f64::from(max_damage);
    let width = (DURABILITY_WIDTH - damage * DURABILITY_WIDTH / max).round() as i32;
    let ramp = (DURABILITY_RAMP - damage * DURABILITY_RAMP / max).round() as i32;
    (width, ramp)
}

impl View {
    /// A view with no feed.
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            arrival: None,
            own: None,
            held: HeldItem::new(),
            rows: StatRows::new(),
        }
    }

    /// Folds one session event in: the join records the window's own entity, the tick
    /// feed stores the frames and stamps their arrival, the window 0 snapshot carries
    /// the hotbar, the held-slot event the selection, and the tick advances the held
    /// item's own two updaters.
    pub fn apply(&mut self, event: &ClientEvent) {
        match event {
            ClientEvent::Joined { entity_id, .. } => self.own = Some(*entity_id),
            ClientEvent::EntitiesTick { entities } => {
                self.observe(entities.clone(), Instant::now());
            }
            ClientEvent::WindowSnapshot {
                window_id: 0,
                slots,
                hotbar_pop,
                ..
            } => {
                // The hotbar band of window 0's layout: the armour band at 5–8 and the
                // main slots at 9–35 sit ahead of it (`ContainerPlayer.java`:36-67; the
                // port's own windows.rs holds the same band at 36–44).
                self.held.hotbar = slots.iter().skip(HOTBAR_START).take(9).cloned().collect();
                self.held.pop = *hotbar_pop;
                // The armour band at 5–8 feeds the rows' armour sum
                // (`ContainerPlayer.java`:36-54; `GuiIngame.java`:660-685 reads the
                // worn total through `getTotalArmorValue`).
                if let Some(band) = slots.get(5..9) {
                    for (cell, stack) in band.iter().enumerate() {
                        self.rows.armour[cell] = stack.clone();
                    }
                }
            }
            ClientEvent::HeldItemSlot { slot } => {
                if let Ok(slot) = usize::try_from(*slot) {
                    if slot < 9 {
                        self.held.selected = slot;
                    }
                }
            }
            ClientEvent::PlayerTick {
                yaw,
                pitch,
                tick,
                in_water,
                hurt_time,
                ..
            } => {
                self.held.tick(*pitch, *yaw);
                self.rows.tick = *tick;
                self.rows.in_water = *in_water;
                self.rows.hurt_time = *hurt_time;
            }
            ClientEvent::Health {
                health,
                food,
                saturation,
            } => {
                self.rows.health = *health;
                self.rows.food = *food;
                self.rows.saturation = *saturation;
            }
            ClientEvent::Effects { effects } => {
                self.rows.effects.clone_from(effects);
            }
            ClientEvent::Air { air } => {
                self.rows.air = *air;
            }
            ClientEvent::Absorption { amount } => {
                self.rows.absorption = *amount;
            }
            ClientEvent::Experience { bar, level, .. } => {
                self.rows.bar = *bar;
                self.rows.level = *level;
            }
            _ => {}
        }
    }

    /// Starts the held item's swing — the window's own input path (the source's
    /// `swingItem` from `clickMouse` and `rightClickMouse`). The source's own gates
    /// stay with the session and are not carried here: `clickMouse` swings only while
    /// `leftClickCounter <= 0` (`Minecraft.java`:1524; the miss cooldown, set to 10 at
    /// `:1534`/`:1558` and ticked down at `:1897-1899`), and the per-tick digging
    /// swing also needs `!isUsingItem` (`:1503`).
    pub fn swing_held(&mut self) {
        self.held.swing();
    }

    /// The held item's frame at `now`: the stack through the swap rule and the rendered
    /// arguments at the frame's own fraction.
    pub fn held_frame(&self, now: Instant) -> HeldItemFrame {
        self.held.frame(self.partial(now))
    }

    /// The frame fraction: the elapsed time since the feed's arrival over the
    /// fifty-millisecond tick, clamped to one; zero before any feed.
    fn partial(&self, now: Instant) -> f32 {
        match self.arrival {
            Some(arrival) => (now.saturating_duration_since(arrival).as_secs_f32() / TICK_SECONDS)
                .clamp(0.0, 1.0),
            None => 0.0,
        }
    }

    /// Stores a feed and its arrival instant — the seam the tests drive the fraction
    /// through.
    fn observe(&mut self, frames: Vec<oxide_game::entity_view::EntityFrame>, arrival: Instant) {
        self.frames = frames;
        self.arrival = Some(arrival);
    }

    /// The draws for a frame at `now`: one per tracked entity but the window's own,
    /// interpolated between the stored tick's pairs.
    ///
    /// The fraction is the elapsed time since the feed's arrival over the fifty-millisecond
    /// tick, clamped to one — the same fraction the player pose uses. Each frame's position
    /// slides between its pair unless the step is a teleport (over [`SNAP_BLOCKS`]), its
    /// rotations interpolate the wrapped difference (`RendererLivingEntity.java:99-100`
    /// `interpolateRotation`), and the head's net yaw is the head's interpolated turn minus
    /// the body's. The pose carries the source's own terms: `limbSwing - limbSwingAmount *
    /// (1 - partial)` and the eased limb amount (`RendererLivingEntity.doRender`), the age,
    /// the swing grid's interpolated step (`EntityLivingBase.getSwingProgress`), and the
    /// damage and death fractions the pass gates on — one while either window is open, and
    /// the clamped square root of the source's twenty-tick ramp
    /// (`RendererLivingEntity.rotateCorpse`).
    ///
    /// `board` resolves each player frame's below-name line ([`below_name_for`]); the
    /// line's camera-dependent terms — the distance gate and the nametag chain — ride the
    /// draw pass, where the nametag's own camera terms live.
    pub fn entity_draws(
        &self,
        now: Instant,
        skins: &BTreeMap<String, SkinUpdate>,
        board: &Scoreboard,
    ) -> Vec<EntityDraw> {
        if self.arrival.is_none() {
            return Vec::new();
        }
        let partial = self.partial(now);
        let mut draws = Vec::new();
        for frame in &self.frames {
            if Some(frame.id) == self.own {
                continue;
            }
            let Some(mut draw) = draw_for(frame, partial, skins) else {
                continue;
            };
            draw.below_name = below_name_for(frame, board);
            draws.push(draw);
        }
        draws
    }

    /// The hotbar frame's draws at `now`: the widgets background and highlight
    /// slices, the nine slot items carrying the session's pop counters minus the
    /// frame's fraction, each slot's count and durability overlay, the held-item
    /// popup and the crosshair (`GuiIngame.renderTooltip`:365-394,
    /// `renderHotbarItem`:1037-1063, `renderSelectedItem`:452-492, the crosshair
    /// block `:175-180`).
    ///
    /// The list runs in the source's own overlay order — the hotbar fourth, the
    /// crosshair fifth, the popup eleventh (`GuiIngame.java`:136-362; the values
    /// file's order table). The whole list obeys the call site's gate, not the
    /// overlay's: F1 with no screen open draws nothing, a screen keeps every
    /// entry (`EntityRenderer.java`:1166-1169). The hotbar itself is not gated by
    /// `shouldDrawHUD` — it draws in creative too — while the popup sits fourteen
    /// lower outside survival and adventure (`GuiIngame.java`:468-471). A slot
    /// with no stack draws nothing: no item, no pop, no overlay (`:1039-1041`).
    /// The overlay draws outside the pop matrix, unscaled (`:1054-1061`).
    pub fn hotbar_draws(&self, now: Instant, input: &HotbarInput<'_>) -> Vec<HudDraw> {
        if !hud_visible(input.hide_gui, input.screen_open) {
            return Vec::new();
        }
        // The source's own integer halves (`sr.getScaledWidth() / 2`).
        let half_w = (input.scaled.width / 2) as f32;
        let half_h = (input.scaled.height / 2) as f32;
        let height = input.scaled.height as f32;
        let mut draws = Vec::new();
        // The 182x22 background slice `(0, 0, 182, 22)` at `(scaledW/2 − 91,
        // scaledH − 22)` (`GuiIngame.java`:375).
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(HOTBAR_WIDGETS),
            x: half_w - 91.0,
            y: height - 22.0,
            width: 182.0,
            height: 22.0,
            uv: [0.0, 0.0, 182.0 / 256.0, 22.0 / 256.0],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        // The 24x22 highlight slice `(0, 22, 24, 22)` at `(scaledW/2 − 92 +
        // selected·20, scaledH − 23)` (`:376`).
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(HOTBAR_WIDGETS),
            x: half_w - 92.0 + self.held.selected as f32 * 20.0,
            y: height - 23.0,
            width: 24.0,
            height: 22.0,
            uv: [0.0, 22.0 / 256.0, 24.0 / 256.0, 44.0 / 256.0],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        let partial = self.partial(now);
        for (slot, stack) in self.held.hotbar.iter().take(9).enumerate() {
            let Some(stack) = stack else {
                continue;
            };
            // The cell's top-left (`scaledW/2 − 88 + 20j, scaledH − 19`, `:383-387`).
            let x = half_w - 88.0 + slot as f32 * 20.0;
            let y = height - 19.0;
            // The pop's `f = animationsToGo − partialTicks`, floored at zero
            // (`:1043`); the draw scales only while positive (`:1047-1051`).
            let pop = (f32::from(self.held.pop[slot]) - partial).max(0.0);
            draws.push(HudDraw::Item {
                stack: Some(item_icon(stack)),
                x,
                y,
                pop,
            });
            // The count (`RenderItem.java`:459-473): hidden at exactly one, red
            // below one, unblended with shadow at `(x + 17 − width, y + 9)`.
            if let Some(text) = count_text(stack) {
                if let Some(font) = input.font {
                    let width = string_width(font, &text) as f32;
                    draws.push(HudDraw::Text {
                        text,
                        x: x + 17.0 - width,
                        y: y + 9.0,
                        scale: 1.0,
                        colour: [1.0, 1.0, 1.0, 1.0],
                        shadow: true,
                        blend: false,
                    });
                }
            }
            // The durability bar (`RenderItem.java`:476-494): the black bed, the
            // underlay and the ramp fill at `(x + 2, y + 13)`, plain colour quads.
            if stack_damaged(stack) {
                if let Some(entry) = items::item_entry(stack.id) {
                    let (fill, ramp) = durability_terms(stack.damage, entry.max_damage);
                    draws.push(HudDraw::Rect {
                        x: x + 2.0,
                        y: y + 13.0,
                        width: 13.0,
                        height: 2.0,
                        colour: [0.0, 0.0, 0.0, 1.0],
                    });
                    draws.push(HudDraw::Rect {
                        x: x + 2.0,
                        y: y + 13.0,
                        width: 12.0,
                        height: 1.0,
                        colour: [((255 - ramp) / 4) as f32 / 255.0, 64.0 / 255.0, 0.0, 1.0],
                    });
                    draws.push(HudDraw::Rect {
                        x: x + 2.0,
                        y: y + 13.0,
                        width: fill as f32,
                        height: 1.0,
                        colour: [(255 - ramp) as f32 / 255.0, ramp as f32 / 255.0, 0.0, 1.0],
                    });
                }
            }
        }
        // The crosshair (`GuiIngame.java`:175-180): one 16x16 `gui/icons` quad at
        // `(scaledW/2 − 7, scaledH/2 − 7)`, exactly while `showCrosshair` says so —
        // the overlay's fifth entry, ahead of the popup's eleventh.
        if input.show_crosshair {
            draws.push(HudDraw::InvertRect {
                x: half_w - 7.0,
                y: half_h - 7.0,
                w: 16.0,
                h: 16.0,
            });
        }
        // The popup (`GuiIngame.java`:452-492): the named stack centred at
        // `scaledH − 59` — fourteen lower outside survival — while its clock runs,
        // blended, at the clock's own alpha.
        if self.held.popup_ticks > 0 {
            if let (Some(font), Some(stack)) = (input.font, self.held.popup_stack.as_ref()) {
                if let Some(name) = popup_text(stack) {
                    let width = string_width(font, &name) as f32;
                    let alpha = f32::from(popup_alpha(self.held.popup_ticks)) / 255.0;
                    draws.push(HudDraw::Text {
                        text: name,
                        x: (input.scaled.width as f32 - width) / 2.0,
                        y: height - (POPUP_ABOVE as f32)
                            + if input.survival {
                                0.0
                            } else {
                                POPUP_CREATIVE_SHIFT as f32
                            },
                        scale: 1.0,
                        colour: [1.0, 1.0, 1.0, alpha],
                        shadow: true,
                        blend: true,
                    });
                }
            }
        }
        draws
    }
}

/// The stat rows' draw inputs: the font the level needs, the layout's size, the
/// mode and F1 gates, and the wall clock in milliseconds — the blink's settle
/// rule reads the same millisecond clock the source's `Minecraft.getSystemTime`
/// returns (`GuiIngame.java`:630-634).
pub struct RowsInput<'a> {
    /// The measured font; without one the level stays out while the slices
    /// still draw.
    pub font: Option<&'a Font>,
    /// The GUI-space size the frame lays out in.
    pub scaled: ScaledResolution,
    /// Whether survival or adventure is in force (`gameIsSurvivalOrAdventure`,
    /// the rows' and the bar's own gate, `GuiIngame.java`:187-189/:218-222).
    pub survival: bool,
    /// The F1 state (`EntityRenderer.java`:1166).
    pub hide_gui: bool,
    /// Whether a screen is open (`EntityRenderer.java`:1166).
    pub screen_open: bool,
    /// The wall clock in milliseconds.
    pub now_ms: u64,
}

/// The rows' shared jitter literal: `rand.setSeed(updateCounter × 312871)`, set
/// once per render before the hearts loop (`GuiIngame.java`:637) and consumed by
/// the hearts (`:711-714`) and the food (`:788-791`) loops.
const ROWS_SEED_FACTOR: i64 = 312871;

/// The max health the rows lay out against: the `maxHealth` attribute's own
/// default. No 1.7-protocol packet carries the attribute, so the port reads the
/// default the source spawns with (recorded; the regen cell and the row count
/// both key off it, `:646`/:657).
const ROWS_MAX_HEALTH: f32 = 20.0;

/// The potion ids the rows read (`Potion.java`: the own player's active map,
/// `EntityPlayer.isPotionActive` at each row's call site).
const EFFECT_REGENERATION: u8 = 10;
const EFFECT_HUNGER: u8 = 17;
const EFFECT_POISON: u8 = 19;
const EFFECT_WITHER: u8 = 20;

/// The ceil of a float health term: the source's own truncating cast plus one
/// when fractional (`MathHelper.ceiling_float_int`, `MathHelper.java`:106-110).
fn ceil_float_int(value: f32) -> i32 {
    let truncated = value as i32;
    if value > truncated as f32 {
        truncated + 1
    } else {
        truncated
    }
}

/// The ceil of a double air term: the same cast one level up
/// (`MathHelper.ceiling_double_int`, `MathHelper.java`:112-116).
fn ceil_double_int(value: f64) -> i32 {
    let truncated = value as i32;
    if value > truncated as f64 {
        truncated + 1
    } else {
        truncated
    }
}

/// The rows' shared seed for one tick: the source's own `updateCounter * 312871`
/// int arithmetic (`GuiIngame.java`:637), Java's 32-bit wrap included.
fn row_seed(tick: u64) -> i64 {
    (tick as i32).wrapping_mul(ROWS_SEED_FACTOR as i32) as i64
}

/// The source's `java.util.Random` (`Random.java`): the 48-bit linear
/// congruential stream the hearts and food jitter draw from. The port carries
/// the exact recurrence — the multiplier, the addend and the mask — because the
/// tests pin the JVM's own `nextInt(2)`/`nextInt(3)` sequences.
struct JvmRand {
    seed: u64,
}

impl JvmRand {
    /// The LCG's multiplier (`Random.java`:28).
    const MULTIPLIER: u64 = 0x5DEECE66D;
    /// The LCG's addend (`Random.java`:29).
    const ADDEND: u64 = 0xB;
    /// The 48-bit mask (`Random.java`:30).
    const MASK: u64 = (1 << 48) - 1;

    /// The stream for one seed: `setSeed`'s own xor-and-mask (`Random.java`:113).
    fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ Self::MULTIPLIER) & Self::MASK,
        }
    }

    /// The next `bits` of the stream (`Random.next`, `Random.java`:186-190).
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self
            .seed
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::ADDEND))
            & Self::MASK;
        (self.seed >> (48 - bits)) as i32
    }

    /// The bounded draw (`Random.nextInt(int)`, `Random.java`:206-240): the
    /// power-of-two fast path and the rejection loop, in the source's own
    /// wrapping int arithmetic. A non-positive bound answers zero — the source
    /// throws, but the rows only ever pass 2 and 3 (recorded).
    fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        if (bound & bound.wrapping_neg()) == bound {
            return ((bound as i64 * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }
}

/// One armour icon's sheet window: full `(34, 9)` while the pair sits under the
/// value, half `(25, 9)` on it, empty `(16, 9)` past it (`GuiIngame.java`:664-682).
fn armour_slice(points: i32, cell: i32) -> (i32, i32) {
    if cell * 2 + 1 < points {
        (34, 9)
    } else if cell * 2 + 1 == points {
        (25, 9)
    } else {
        (16, 9)
    }
}

/// The air split: the full bubbles and the one fading bubble
/// (`GuiIngame.java`:878-879 — `k7` full, `i8` popping).
fn air_split(air: i16) -> (i32, i32) {
    let full = ceil_double_int((f64::from(air) - 2.0) * 10.0 / 300.0);
    let total = ceil_double_int(f64::from(air) * 10.0 / 300.0);
    (full, total - full)
}

/// The experience fill's width: the truncated `bar × 183`
/// (`GuiIngame.java`:422 — `(int)(experience * (j + 1))`, `j = 182`).
fn exp_fill_width(bar: f32) -> i32 {
    (bar * 183.0) as i32
}

/// The bar's gate: the level's own cap (`EntityPlayer.xpBarCap`:2059-2062).
fn xp_bar_cap(level: i32) -> i32 {
    if level >= 30 {
        112 + (level - 30) * 9
    } else if level >= 15 {
        37 + (level - 15) * 5
    } else {
        7 + level * 2
    }
}

/// The survival rows' own state: every feed `stat_rows_draws` reads, folded in
/// by [`View::apply`].
#[derive(Debug, Clone)]
struct StatRows {
    /// The session's tick: the rows' `updateCounter` (`GuiIngame.java`:637).
    tick: u64,
    /// Whether the player is in water: the air row's submersion gate
    /// (`isInsideOfMaterial(Material.water)`, `:875`).
    in_water: bool,
    /// Ticks left of the hurt flash: the blink raises' own gate
    /// (`hurtResistantTime > 0`, `:619`/:624).
    hurt_time: u32,
    /// The health, food and saturation the 0x06 carried.
    health: f32,
    /// The food level the 0x06 carried.
    food: i32,
    /// The saturation the 0x06 carried.
    saturation: f32,
    /// The own player's live effects.
    effects: Vec<StatusEffect>,
    /// The air in ticks; 300 is a full breath.
    air: i16,
    /// The absorption in half-hearts, from the own metadata's index 17.
    absorption: f32,
    /// The experience bar's fill, 0 through 1.
    bar: f32,
    /// The player's level.
    level: i32,
    /// The armour band: window 0's slots 5–8.
    armour: [Option<MetadataItem>; 4],
    /// The current ceil'd health (`playerHealth`, `:634`).
    player_health: i32,
    /// The remembered ceil'd health the flash draws against (`lastPlayerHealth`).
    last_player_health: i32,
    /// The blink's deadline tick (`healthUpdateCounter`, `:615`/:622/:627).
    health_update_counter: i64,
    /// The last raise or settle's clock (`lastSystemTime`, `:620`/:625/:633).
    last_system_time: u64,
}

impl StatRows {
    /// The resting state: full health and food, full air, nothing worn, no
    /// effects — the frame a fresh spawn draws.
    fn new() -> Self {
        Self {
            tick: 0,
            in_water: false,
            hurt_time: 0,
            health: ROWS_MAX_HEALTH,
            food: 20,
            saturation: 5.0,
            effects: Vec::new(),
            air: 300,
            absorption: 0.0,
            bar: 0.0,
            level: 0,
            armour: [None, None, None, None],
            player_health: ROWS_MAX_HEALTH as i32,
            last_player_health: ROWS_MAX_HEALTH as i32,
            health_update_counter: 0,
            last_system_time: 0,
        }
    }

    /// Whether the named effect is active (`isPotionActive` at each row's site).
    fn effect_active(&self, id: u8) -> bool {
        self.effects.iter().any(|effect| effect.effect_id == id)
    }

    /// The worn armour's total, capped at twenty: window-0 slots 5–8 through
    /// Task 8's `armour_points` (`getTotalArmorValue`'s own walk, capped where
    /// the display caps).
    fn armour_points(&self) -> i32 {
        let total: f32 = self
            .armour
            .iter()
            .flatten()
            .map(|stack| {
                items::item_entry(stack.id)
                    .and_then(|entry| entry.attributes.armour_points)
                    .unwrap_or(0.0)
            })
            .sum();
        total.min(20.0) as i32
    }
}

/// One nine-by-nine `gui/icons` slice at `(x, y)` sampling `(u, v)`.
fn rows_icons_slice(x: f32, y: f32, u: i32, v: i32) -> HudDraw {
    HudDraw::TexturedRect {
        texture: HudTexture::Named(TAB_ICONS),
        x,
        y,
        width: 9.0,
        height: 9.0,
        uv: [
            u as f32 / 256.0,
            v as f32 / 256.0,
            (u + 9) as f32 / 256.0,
            (v + 9) as f32 / 256.0,
        ],
        colour: [1.0, 1.0, 1.0, 1.0],
    }
}

impl View {
    /// The player's experience level: the enchanting offer faces and click
    /// gate read it (`ContainerEnchantment.enchantItem`'s level arms).
    pub fn level(&self) -> i32 {
        self.rows.level
    }

    /// The own player's live effects, ascending by id: the inventory overlay
    /// reads them (`ClientEvent::Effects` after 0x1D/0x1E).
    pub fn effects(&self) -> &[StatusEffect] {
        &self.rows.effects
    }

    /// The survival rows' draws: the armour, hearts (with the absorption
    /// overlay), food, air and experience rows (`renderPlayerStats`:609-900 and
    /// `renderExpBar`:414-450).
    ///
    /// The list runs in the source's own section order — armour, health, food,
    /// air, then the bar and the level. The whole list obeys the call site's
    /// gates: nothing draws outside survival and adventure, and F1 with no
    /// screen open draws nothing (`gameIsSurvivalOrAdventure` at `:218-222`,
    /// `EntityRenderer.java`:1166-1169). The mount-health arm is omitted — the
    /// port tracks no ridden entity (recorded) — and the hardcore row offset
    /// stays zero for the same reason. The food's `flag1` flash never draws:
    /// the source declares it false and never sets it (`:638`, read at
    /// `:793-811`).
    ///
    /// The container's blink follows the source's phased flag rather than the
    /// pending counter (`k3 = flag ? 1 : 0`, `:700-705` — the raise's own
    /// render keeps the normal container), and the one-second settle leaves
    /// the counter untouched (`:628-636` never resets it).
    pub fn stat_rows_draws(&mut self, input: &RowsInput<'_>) -> Vec<HudDraw> {
        if !input.survival || !hud_visible(input.hide_gui, input.screen_open) {
            return Vec::new();
        }
        // The source's own integer halves (`sr.getScaledWidth() / 2`).
        let half_w = (input.scaled.width / 2) as f32;
        let height = input.scaled.height as f32;
        let i1 = half_w - 91.0;
        let j1 = half_w + 91.0;
        let k1 = height - 39.0;
        let rows = &mut self.rows;
        // The blink's bookkeeping (`GuiIngame.java`:615-636): the 20-tick raise
        // on a hurt loss and the 10-tick raise on a hurt gain, then the
        // one-second settle of the remembered health.
        let i = ceil_float_int(rows.health);
        if i < rows.player_health && rows.hurt_time > 0 {
            rows.last_system_time = input.now_ms;
            rows.health_update_counter = rows.tick as i64 + 20;
        } else if i > rows.player_health && rows.hurt_time > 0 {
            rows.last_system_time = input.now_ms;
            rows.health_update_counter = rows.tick as i64 + 10;
        }
        if input.now_ms.saturating_sub(rows.last_system_time) > 1000 {
            rows.player_health = i;
            rows.last_player_health = i;
            rows.last_system_time = input.now_ms;
        }
        rows.player_health = i;
        let j = rows.last_player_health;
        let tick = rows.tick as i64;
        let blink = rows.health_update_counter > tick;
        let flag = blink && (rows.health_update_counter - tick) / 3 % 2 == 1;
        let mut rand = JvmRand::new(row_seed(rows.tick));
        let mut draws = Vec::new();
        let max_health = ROWS_MAX_HEALTH;
        let absorption = rows.absorption;
        // The row count and the armour/air height (`:644-650`).
        let l1 = ceil_float_int((max_health + absorption) / 2.0 / 10.0);
        let i2 = (10 - (l1 - 2)).max(3);
        let j2 = k1 - (l1 - 1) as f32 * i2 as f32 - 10.0;
        // The regeneration cell (`:657`): `updateCounter % ceil(maxHealth + 5)` —
        // the max-health attribute, absorption excluded.
        let l2 = if rows.effect_active(EFFECT_REGENERATION) {
            (rows.tick % u64::from(ceil_float_int(max_health + 5.0) as u32)) as i32
        } else {
            -1
        };
        // The armour row (`:660-685`): each point pair draws full, half or
        // empty from the row's left edge — hidden entirely at zero.
        let armour = rows.armour_points();
        if armour > 0 {
            for cell in 0..10 {
                let (u, v) = armour_slice(armour, cell);
                draws.push(rows_icons_slice(i1 + cell as f32 * 8.0, j2, u, v));
            }
        }
        // The hearts row (`:687-774`).
        let poisoned = rows.effect_active(EFFECT_POISON);
        let withered = rows.effect_active(EFFECT_WITHER);
        let j6 = 16
            + if poisoned {
                36
            } else if withered {
                72
            } else {
                0
            };
        let k3 = if flag { 1 } else { 0 };
        let cells = ceil_float_int((max_health + absorption) / 2.0);
        let mut f2 = absorption;
        for i6 in (0..cells).rev() {
            let l3 = ceil_float_int((i6 + 1) as f32 / 10.0) - 1;
            let x = i1 + (i6 % 10) as f32 * 8.0;
            let mut y = k1 - l3 as f32 * i2 as f32;
            if i <= 4 {
                y += rand.next_int(2) as f32;
            }
            if i6 == l2 {
                y -= 2.0;
            }
            draws.push(rows_icons_slice(x, y, 16 + k3 * 9, 0));
            if flag {
                if i6 * 2 + 1 < j {
                    draws.push(rows_icons_slice(x, y, j6 + 54, 0));
                }
                if i6 * 2 + 1 == j {
                    draws.push(rows_icons_slice(x, y, j6 + 63, 0));
                }
            }
            if f2 > 0.0 {
                if f2 == absorption && absorption % 2.0 == 1.0 {
                    draws.push(rows_icons_slice(x, y, j6 + 153, 0));
                } else {
                    draws.push(rows_icons_slice(x, y, j6 + 144, 0));
                }
                f2 -= 2.0;
            } else {
                if i6 * 2 + 1 < i {
                    draws.push(rows_icons_slice(x, y, j6 + 36, 0));
                }
                if i6 * 2 + 1 == i {
                    draws.push(rows_icons_slice(x, y, j6 + 45, 0));
                }
            }
        }
        // The food row (`:776-823`): the half rule, the hunger-effect variant
        // and the saturation-gated jitter off the shared seed.
        let hungered = rows.effect_active(EFFECT_HUNGER);
        let l7 = if hungered { 52 } else { 16 };
        let j8 = if hungered { 13 } else { 0 };
        for k6 in 0..10 {
            let mut y = k1;
            if rows.saturation <= 0.0
                && rows.tick % (u64::from(rows.food.max(0) as u32) * 3 + 1) == 0
            {
                y += (rand.next_int(3) - 1) as f32;
            }
            let x = j1 - k6 as f32 * 8.0 - 9.0;
            draws.push(rows_icons_slice(x, y, 16 + j8 * 9, 27));
            if k6 * 2 + 1 < rows.food {
                draws.push(rows_icons_slice(x, y, l7 + 36, 27));
            }
            if k6 * 2 + 1 == rows.food {
                draws.push(rows_icons_slice(x, y, l7 + 45, 27));
            }
        }
        // The air row (`:873-891`): the 300/10 split into full and popping
        // bubbles, right-to-left at the armour's height, gated by submersion.
        if rows.in_water {
            let (full, popping) = air_split(rows.air);
            for l8 in 0..full + popping {
                let u = if l8 < full { 16 } else { 25 };
                draws.push(rows_icons_slice(j1 - l8 as f32 * 8.0 - 9.0, j2, u, 18));
            }
        }
        // The experience bar (`renderExpBar`:414-432): the background and the
        // truncated fill at `scaledH − 29`, behind the bar's own cap gate.
        if xp_bar_cap(rows.level) > 0 {
            let y = height - 29.0;
            draws.push(HudDraw::TexturedRect {
                texture: HudTexture::Named(TAB_ICONS),
                x: i1,
                y,
                width: 182.0,
                height: 5.0,
                uv: [0.0, 64.0 / 256.0, 182.0 / 256.0, 69.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            });
            let k = exp_fill_width(rows.bar);
            if k > 0 {
                draws.push(HudDraw::TexturedRect {
                    texture: HudTexture::Named(TAB_ICONS),
                    x: i1,
                    y,
                    width: k as f32,
                    height: 5.0,
                    uv: [0.0, 69.0 / 256.0, k as f32 / 256.0, 74.0 / 256.0],
                    colour: [1.0, 1.0, 1.0, 1.0],
                });
            }
        }
        // The level (`:434-450`): the number centred in `0x80FF20` with the
        // four-offset black outline at `scaledH − 35` — past level zero only.
        if rows.level > 0 {
            if let Some(font) = input.font {
                let text = rows.level.to_string();
                let width = string_width(font, &text);
                let x = ((input.scaled.width as i32 - width) / 2) as f32;
                let y = height - 35.0;
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    draws.push(HudDraw::Text {
                        text: text.clone(),
                        x: x + dx,
                        y: y + dy,
                        scale: 1.0,
                        colour: [0.0, 0.0, 0.0, 1.0],
                        shadow: false,
                        blend: false,
                    });
                }
                draws.push(HudDraw::Text {
                    text,
                    x,
                    y,
                    scale: 1.0,
                    colour: [128.0 / 255.0, 1.0, 32.0 / 255.0, 1.0],
                    shadow: false,
                    blend: false,
                });
            }
        }
        draws
    }
}

/// The draw's own texture for the item geometries: the generated shape and the block
/// meshes carry their own sheets, so the draw's `texture` field stays a present-but-unused
/// placeholder — the always-uploaded shadow sprite stands in.
const ITEM_DRAW_PLACEHOLDER: &str = "misc/shadow.png";

/// The thrown-item sheet a projectile kind draws, when the object set covers it
/// (`RenderManager.java`:177-183).
fn projectile_sprite(kind: EntityKind) -> Option<&'static str> {
    match kind {
        EntityKind::Snowball => Some("items/snowball"),
        EntityKind::Egg => Some("items/egg"),
        EntityKind::EnderPearl => Some("items/ender_pearl"),
        EntityKind::EyeOfEnder => Some("items/ender_eye"),
        EntityKind::Potion => Some("items/potion_bottle_drinkable"),
        EntityKind::XpBottle => Some("items/experience_bottle"),
        EntityKind::Firework => Some("items/fireworks"),
        _ => None,
    }
}

/// The draw one frame makes, or `None` for a kind with no model yet and for an invisible
/// entity.
///
/// The model, texture and slim flag come from the skin worker's update for the profile
/// when one exists — as it stands — else from the uuid's own default model rule
/// (`DefaultPlayerSkin.isSlimSkin`, `DefaultPlayerSkin.java:41-44`); the renderer's
/// resolver falls back the same way when the update carries no texture. The cape layer's
/// wave reads the frame pair's displacement between its ticks — the window's stand-in for
/// the smoothed chaser and camera-yaw terms its state cannot produce ([`PlayerExtra`]). An
/// invisible entity yields no draw, model and shadow alike: the source skips both for one
/// (`RendererLivingEntity.java:248-249`, `Render.java:303`), and the window's shadow rides
/// the same draw.
fn draw_for(
    frame: &EntityFrame,
    partial: f32,
    skins: &BTreeMap<String, SkinUpdate>,
) -> Option<EntityDraw> {
    if frame.invisible {
        return None;
    }
    let position = interpolate_position(frame.prev, frame.pos, partial);
    let body_yaw = interpolate_rotation(
        frame.prev_render_yaw_offset,
        frame.render_yaw_offset,
        partial,
    );
    let head = interpolate_rotation(frame.prev_head_yaw, frame.head_yaw, partial);
    let head_yaw = head - body_yaw;
    let head_pitch = frame.prev_pitch + (frame.pitch - frame.prev_pitch) * partial;

    let swing = {
        let mut step = frame.swing_progress - frame.prev_swing_progress;
        if step < 0.0 {
            step += 1.0;
        }
        frame.prev_swing_progress + step * partial
    };
    // The source's damage overlay opens while the hurt window or the death animation runs
    // (`RendererLivingEntity.doRender`'s `hurtTime > 0 || deathTime > 0`); the death ramp
    // is the clamped square root of `((deathTime + partial - 1) / 20) * 1.6`.
    let hurt = if frame.hurt_ticks > 0 || frame.death_ticks > 0 {
        1.0
    } else {
        0.0
    };
    let death = if frame.death_ticks > 0 {
        (((f32::from(frame.death_ticks) + partial - 1.0) / 20.0) * 1.6)
            .sqrt()
            .min(1.0)
    } else {
        0.0
    };

    // The kind's own channel: a player's skin and cape terms, a mob's model, sheet and
    // extras. The kinds with neither draw nothing.
    let (model, texture, draw_extra, pose_extra, child) = match &frame.extra {
        EntityExtra::Player => {
            let Some(uuid) = frame.uuid.clone() else {
                tracing::debug!(
                    id = frame.id,
                    "a player without a profile cannot be looked up"
                );
                return None;
            };
            let slim = match skins.get(&uuid) {
                Some(update) => update.model == DefaultModel::Slim,
                None => default_skin(&uuid) == DefaultModel::Slim,
            };
            // The cape layer's wave reads the frame pair's own displacement — the window's
            // stand-in for the smoothed chaser and camera-yaw terms its state cannot
            // produce (`PlayerExtra`).
            (
                ModelRef::Player {
                    slim,
                    parts: ALL_PARTS,
                },
                TextureRef::Skin { uuid, slim },
                DrawExtra::None,
                PoseExtra::Player(PlayerExtra {
                    motion: [
                        (frame.pos[0] - frame.prev[0]) as f32,
                        (frame.pos[1] - frame.prev[1]) as f32,
                        (frame.pos[2] - frame.prev[2]) as f32,
                    ],
                    // The held-item pose gate: a non-null held stack
                    // (`RenderPlayer.setModelVisibilities`:93-97).
                    held: frame.equipment[0].is_some(),
                }),
                false,
            )
        }
        EntityExtra::Mob(mob) => match mob_draw(frame.kind, frame.id, mob) {
            Some(terms) => terms,
            None => {
                tracing::debug!(kind = ?frame.kind, "the entity kind has no model yet");
                return None;
            }
        },
        EntityExtra::Item { id, count, damage } => {
            let model = match items::resolve(*id, *damage) {
                items::ItemResolution::Block(block) => ModelRef::BlockItem { block },
                items::ItemResolution::Sprite(key) => ModelRef::Sprite { key },
                items::ItemResolution::Missing => {
                    tracing::debug!(id, "the item id has no model");
                    return None;
                }
            };
            (
                model,
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Item {
                    id: *id,
                    count: *count,
                    damage: *damage,
                },
                PoseExtra::None,
                false,
            )
        }
        EntityExtra::Painting { title, facing } => (
            ModelRef::Painting {
                art: objects::art_index(title) as u8,
            },
            TextureRef::Named(objects::PAINTING_TEXTURE),
            DrawExtra::Painting { facing: *facing },
            PoseExtra::None,
            false,
        ),
        EntityExtra::ItemFrame { item, rotation } => {
            let content = match item {
                None => FrameContent::Empty,
                Some(stack) => match items::resolve(stack.id, stack.damage) {
                    items::ItemResolution::Block(block) => FrameContent::Block(block),
                    items::ItemResolution::Sprite(key) => FrameContent::Sprite(key),
                    items::ItemResolution::Missing => FrameContent::Empty,
                },
            };
            (
                ModelRef::ItemFrame { content },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Frame {
                    rotation: *rotation,
                },
                PoseExtra::None,
                false,
            )
        }
        EntityExtra::Boat => (
            ModelRef::Boat,
            TextureRef::Named(objects::BOAT_TEXTURE),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        EntityExtra::Minecart => (
            ModelRef::Minecart { body: 0 },
            TextureRef::Named(objects::MINECART_TEXTURE),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        EntityExtra::Orb => (
            ModelRef::Orb { value: 1 },
            TextureRef::Named(objects::ORB_TEXTURE),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        EntityExtra::Projectile => match frame.kind {
            EntityKind::Arrow => (
                ModelRef::Arrow,
                TextureRef::Named(objects::ARROW_TEXTURE),
                DrawExtra::None,
                PoseExtra::None,
                false,
            ),
            EntityKind::Fireball => (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Projectile {
                    billboard: objects::Billboard::Fireball,
                    scale: 2.0,
                },
                PoseExtra::None,
                false,
            ),
            // The blaze's small fireball draws under the same class at its own
            // registration scale (`RenderManager.java`:185).
            EntityKind::SmallFireball => (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Projectile {
                    billboard: objects::Billboard::Fireball,
                    scale: 0.5,
                },
                PoseExtra::None,
                false,
            ),
            EntityKind::WitherSkull => (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                DrawExtra::Projectile {
                    billboard: objects::Billboard::Fireball,
                    scale: 1.0,
                },
                PoseExtra::None,
                false,
            ),
            kind => {
                let Some(key) = projectile_sprite(kind) else {
                    tracing::debug!(kind = ?kind, "the projectile has no sprite");
                    return None;
                };
                (
                    ModelRef::Sprite { key },
                    TextureRef::Named(ITEM_DRAW_PLACEHOLDER),
                    DrawExtra::Projectile {
                        billboard: objects::Billboard::Snowball,
                        scale: 0.5,
                    },
                    PoseExtra::None,
                    false,
                )
            }
        },
        other => {
            tracing::debug!(
                kind = ?frame.kind,
                extra = ?other,
                "the entity kind has no model yet"
            );
            return None;
        }
    };

    // The dragon's flight clock rides the frame's age at the source's at-rest rate
    // (`EntityDragon.onLivingUpdate`:158-167): the wing wave advances a fifth of a tick.
    // The slowed flag's halving, the motion factor and the AI-disabled `0.5` lock are not
    // carried by the frames, so the clock runs at the at-rest rate (the exotics ledger
    // records the same pins).
    let pose_extra = match pose_extra {
        PoseExtra::Dragon { .. } => PoseExtra::Dragon {
            anim_time: (frame.age as f32 + partial) * 0.2,
        },
        other => other,
    };

    // A child's limb swing runs three times as fast before the pose reads it
    // (`RendererLivingEntity.doRender`:140-143).
    let mut limb_swing = frame.limb_swing - frame.limb_swing_amount * (1.0 - partial);
    if child {
        limb_swing *= 3.0;
    }

    let pose = Pose {
        limb_swing,
        limb_swing_amount: frame.prev_limb_swing_amount
            + (frame.limb_swing_amount - frame.prev_limb_swing_amount) * partial,
        age: frame.age as f32 + partial,
        head_yaw,
        head_pitch,
        body_yaw,
        sneak: frame.sneaking,
        swing_progress: swing,
        hurt,
        death,
        child,
        extra: pose_extra,
    };

    Some(EntityDraw {
        id: frame.id,
        model,
        position,
        body_yaw,
        head_yaw,
        head_pitch,
        pose,
        texture,
        light: frame.brightness,
        hurt,
        death,
        health: frame.health,
        // The frame carries the composed text the session resolved; a frame without a name
        // leaves the field empty and the pass writes nothing.
        nametag: frame.nametag.clone().map(|text| NametagDraw { text }),
        // The entity's own name: the deadmau5 ears' gate (`LayerDeadmau5Head.java`:24).
        name: frame.name.clone(),
        // The below-name label is the frame's slot-2 composition; the caller fills it once
        // the draw is built ([`below_name_for`]).
        below_name: None,
        // The five slots' stacks, converted through the item-icon seam's rule.
        equipment: equipment_draw(&frame.equipment),
        extra: draw_extra,
    })
}

/// The draw terms one mob frame's kind and metadata name: the model, the sheet, the
/// renderer's extras and the pose's own, plus whether the frame is a child.
///
/// The mapping is each renderer's own: the zombie's villager flag swaps in the villager
/// zombie's model and sheet (`RenderZombie.getEntityTexture`:66-69 for the sheet,
/// `RenderZombie.func_82427_a`:71-85 for the model), the skeleton draws its thin-limbed
/// model off the skeleton sheet (`RenderSkeleton`), the villager's profession picks the
/// sheet out of the renderer's table with its own default
/// (`RenderVillager.getEntityTexture`:32-54), the witch's nose gate reads the held
/// stack (`RenderWitch`, not carried by the frames; [`PoseExtra::Witch`]), the giant
/// draws the zombie model and sheet sixfold (`RenderGiantZombie`), the quadrupeds
/// draw their class sheets (`RenderPig`/`RenderCow`/`RenderSheep`/`RenderMooshroom`),
/// and the crawler families theirs (`RenderCreeper` through `RenderEndermite`), the
/// bats', cubes' and eyes layers' state riding the terms' extras.
/// `None` for a kind without a model.
fn mob_draw(
    kind: EntityKind,
    entity_id: i32,
    mob: &MobExtra,
) -> Option<(ModelRef, TextureRef, DrawExtra, PoseExtra, bool)> {
    let terms = match (kind, mob) {
        (EntityKind::Zombie, MobExtra::Zombie { villager: true }) => (
            ModelRef::ZombieVillager,
            TextureRef::Named("entity/zombie/zombie_villager.png"),
            DrawExtra::ZombieVillager,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Zombie, _) => (
            ModelRef::Zombie,
            TextureRef::Named("entity/zombie/zombie.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        // The zombie pigman draws the zombie's own model on its own sheet
        // (`RenderPigZombie.java`:15, `:11`).
        (EntityKind::PigZombie, _) => (
            ModelRef::Zombie,
            TextureRef::Named("entity/zombie_pigman.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Skeleton, _) => (
            ModelRef::Skeleton,
            TextureRef::Named("entity/skeleton/skeleton.png"),
            DrawExtra::None,
            PoseExtra::Skeleton { aimed_bow: false },
            false,
        ),
        (EntityKind::Villager, MobExtra::Villager { profession, child }) => (
            ModelRef::Villager {
                profession: *profession,
                child: *child,
            },
            TextureRef::Named(villager_sheet(*profession)),
            DrawExtra::Villager {
                profession: *profession,
                child: *child,
            },
            PoseExtra::None,
            *child,
        ),
        (EntityKind::Witch, _) => (
            ModelRef::Witch,
            TextureRef::Named("entity/witch.png"),
            DrawExtra::None,
            PoseExtra::Witch {
                holding: false,
                entity_id,
            },
            false,
        ),
        (EntityKind::Giant, _) => (
            ModelRef::Giant,
            TextureRef::Named("entity/zombie/zombie.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::SnowMan, _) => (
            ModelRef::SnowGolem,
            TextureRef::Named("entity/snowman.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::VillagerGolem, _) => (
            ModelRef::IronGolem,
            TextureRef::Named("entity/iron_golem.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Pig, MobExtra::Pig { saddle }) => (
            ModelRef::Pig { saddle: *saddle },
            TextureRef::Named("entity/pig/pig.png"),
            DrawExtra::Pig { saddle: *saddle },
            PoseExtra::None,
            false,
        ),
        (EntityKind::Cow, _) => (
            ModelRef::Cow,
            TextureRef::Named("entity/cow/cow.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Sheep, MobExtra::Sheep { wool, sheared }) => (
            ModelRef::Sheep {
                wool: *wool,
                sheared: *sheared,
            },
            TextureRef::Named("entity/sheep/sheep.png"),
            DrawExtra::Sheep {
                wool: *wool,
                sheared: *sheared,
            },
            PoseExtra::None,
            false,
        ),
        (EntityKind::MushroomCow, _) => (
            ModelRef::Mooshroom,
            TextureRef::Named("entity/cow/mooshroom.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Creeper, MobExtra::Creeper { powered }) => (
            ModelRef::Creeper,
            TextureRef::Named("entity/creeper/creeper.png"),
            DrawExtra::Creeper { powered: *powered },
            PoseExtra::None,
            false,
        ),
        (EntityKind::Spider, _) => (
            ModelRef::Spider,
            TextureRef::Named("entity/spider/spider.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::CaveSpider, _) => (
            ModelRef::CaveSpider,
            TextureRef::Named("entity/spider/cave_spider.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        // The screaming flag is not read by the frames; the pose pins it off.
        (EntityKind::Enderman, _) => (
            ModelRef::Enderman,
            TextureRef::Named("entity/enderman/enderman.png"),
            DrawExtra::None,
            PoseExtra::Enderman { attacking: false },
            false,
        ),
        // The chick's fold is not read by the frames and the flap is client-side
        // tick state they do not carry; the pose pins the flap at its rest.
        (EntityKind::Chicken, _) => (
            ModelRef::Chicken { child: false },
            TextureRef::Named("entity/chicken.png"),
            DrawExtra::None,
            PoseExtra::Chicken { flap: 0.0 },
            false,
        ),
        // The tentacle aim is client-side tick state the frames do not carry.
        (EntityKind::Squid, _) => (
            ModelRef::Squid,
            TextureRef::Named("entity/squid.png"),
            DrawExtra::None,
            PoseExtra::Squid {
                tentacle_angle: 0.0,
            },
            false,
        ),
        (EntityKind::Slime, MobExtra::Slime { size }) => (
            ModelRef::Slime { size: *size },
            TextureRef::Named("entity/slime/slime.png"),
            // The squash is client-side tick state the frames do not carry; the
            // pair rests, the draw scaling by the size alone.
            DrawExtra::Slime {
                size: *size,
                squish: 0.0,
            },
            PoseExtra::None,
            false,
        ),
        (EntityKind::LavaSlime, MobExtra::Slime { size }) => (
            ModelRef::MagmaCube { size: *size },
            TextureRef::Named("entity/slime/magmacube.png"),
            DrawExtra::None,
            PoseExtra::MagmaCube { squish: 0.0 },
            false,
        ),
        (EntityKind::Bat, MobExtra::Bat { hanging }) => (
            ModelRef::Bat { hanging: *hanging },
            TextureRef::Named("entity/bat.png"),
            DrawExtra::Bat { hanging: *hanging },
            PoseExtra::Bat { hanging: *hanging },
            false,
        ),
        (EntityKind::Silverfish, _) => (
            ModelRef::Silverfish,
            TextureRef::Named("entity/silverfish.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Endermite, _) => (
            ModelRef::EnderMite,
            TextureRef::Named("entity/endermite.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        // The exotic families. The motions the frames do not carry draw pinned at their
        // rest (the ghast's tentacle sway, the blaze's rod spin, the guardian's spike
        // and tail phases, the rabbit's hop, the wolf's health-scaled tail droop reads
        // the health the frames carry at its own watcher).
        (
            EntityKind::EntityHorse,
            MobExtra::Horse {
                variant,
                colour,
                markings,
                tamed: _,
                saddle,
                chested,
                armour,
                adult,
            },
        ) => (
            ModelRef::Horse {
                variant: *variant,
                colour: *colour,
                markings: *markings,
                saddle: *saddle,
                armour: *armour,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::horse_sheet(
                *variant, *colour,
            )),
            DrawExtra::Horse {
                markings: *markings,
                armour: *armour,
            },
            PoseExtra::Horse {
                saddle: *saddle,
                chested: *chested,
                adult: *adult,
                variant: *variant,
            },
            !*adult,
        ),
        (
            EntityKind::Wolf,
            MobExtra::Wolf {
                tamed,
                collar,
                angry,
                sitting,
                health,
            },
        ) => (
            ModelRef::Wolf {
                tamed: *tamed,
                collar: *collar,
                angry: *angry,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::wolf_sheet(
                *tamed, *angry,
            )),
            DrawExtra::Wolf {
                tamed: *tamed,
                collar: *collar,
            },
            PoseExtra::Wolf {
                tamed: *tamed,
                angry: *angry,
                sitting: *sitting,
                health: *health,
            },
            false,
        ),
        (
            EntityKind::Ozelot,
            MobExtra::Ocelot {
                variant,
                tamed,
                sitting,
            },
        ) => (
            ModelRef::Ocelot {
                variant: *variant,
                child: false,
                tamed: *tamed,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::ocelot_sheet(*variant)),
            DrawExtra::None,
            PoseExtra::Ocelot { sitting: *sitting },
            false,
        ),
        (EntityKind::Rabbit, MobExtra::Rabbit { variant, child }) => (
            ModelRef::Rabbit {
                variant: *variant,
                child: *child,
            },
            TextureRef::Named(oxide_render::entity_models::exotics::rabbit_sheet(*variant)),
            DrawExtra::None,
            PoseExtra::Rabbit { hop: 0.0 },
            *child,
        ),
        (EntityKind::Ghast, MobExtra::Ghast { shooting }) => (
            ModelRef::Ghast {
                shooting: *shooting,
            },
            TextureRef::Named(if *shooting {
                "entity/ghast/ghast_shooting.png"
            } else {
                "entity/ghast/ghast.png"
            }),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Blaze, _) => (
            ModelRef::Blaze,
            TextureRef::Named("entity/blaze.png"),
            DrawExtra::None,
            PoseExtra::None,
            false,
        ),
        (EntityKind::Guardian, MobExtra::Guardian { elder }) => (
            ModelRef::Guardian { elder: *elder },
            TextureRef::Named(if *elder {
                "entity/guardian_elder.png"
            } else {
                "entity/guardian.png"
            }),
            DrawExtra::None,
            PoseExtra::Guardian {
                spikes: 1.0,
                tail_phase: 0.0,
            },
            false,
        ),
        (EntityKind::EnderDragon, _) => (
            ModelRef::EnderDragon,
            TextureRef::Named("entity/enderdragon/dragon.png"),
            DrawExtra::None,
            PoseExtra::Dragon { anim_time: 0.0 },
            false,
        ),
        (
            EntityKind::WitherBoss,
            MobExtra::Wither {
                invul_time,
                armored,
            },
        ) => (
            ModelRef::Wither {
                invul_time: *invul_time,
            },
            // The spawn shield's flicker (`RenderWither.getEntityTexture`:33-37): while
            // the timer runs, the invulnerable sheet draws except a beat every fifth
            // tick in the first eighty.
            TextureRef::Named(
                if *invul_time > 0 && (*invul_time > 80 || (*invul_time / 5) % 2 != 1) {
                    "entity/wither/wither_invulnerable.png"
                } else {
                    "entity/wither/wither.png"
                },
            ),
            DrawExtra::Wither { armored: *armored },
            PoseExtra::None,
            false,
        ),
        _ => return None,
    };
    Some(terms)
}

/// The villager's sheet for a profession: the renderer's table indexed by the profession
/// with its own fallback (`RenderVillager.getEntityTexture`:32-54: farmer, librarian,
/// priest, smith, butcher; anything else the plain villager sheet).
fn villager_sheet(profession: u8) -> &'static str {
    match profession {
        0 => "entity/villager/farmer.png",
        1 => "entity/villager/librarian.png",
        2 => "entity/villager/priest.png",
        3 => "entity/villager/smith.png",
        4 => "entity/villager/butcher.png",
        _ => "entity/villager/villager.png",
    }
}

/// The position between the pair: the current one outright when the step is a teleport
/// (past [`SNAP_BLOCKS`]), else the slide at the fraction.
fn interpolate_position(prev: [f64; 3], cur: [f64; 3], partial: f32) -> [f64; 3] {
    let step = [cur[0] - prev[0], cur[1] - prev[1], cur[2] - prev[2]];
    if step[0] * step[0] + step[1] * step[1] + step[2] * step[2] > SNAP_BLOCKS * SNAP_BLOCKS {
        return cur;
    }
    let fraction = f64::from(partial);
    [
        prev[0] + step[0] * fraction,
        prev[1] + step[1] * fraction,
        prev[2] + step[2] * fraction,
    ]
}

/// The source's rotation lerp: the previous angle plus the wrapped difference times the
/// fraction (`interpolateRotation`, `RendererLivingEntity.java:65`).
fn interpolate_rotation(prev: f32, cur: f32, partial: f32) -> f32 {
    let mut difference = cur - prev;
    while difference >= 180.0 {
        difference -= 360.0;
    }
    while difference < -180.0 {
        difference += 360.0;
    }
    prev + difference * partial
}

/// The sixteen legacy colour codes' characters, the palette's index order
/// (`ChatComponentStyle.getFormattedText`:87-99 over `EnumChatFormatting`'s table).
const PALETTE: &[u8; 16] = b"0123456789abcdef";

/// The chat opacity the assembly reads — `GameSettings.chatOpacity`'s default
/// (`GameSettings.java`:85, `1.0F`). A settings store that owns the option is a later
/// milestone's, so the default stands.
const CHAT_OPACITY: f32 = 1.0;

/// The newest line's bar bottom, 28 pixels above the screen's bottom edge: the
/// `(2, 20)` translate (`GuiNewChat.java`:49-51) under the `height - 48` one
/// (`GuiIngame.java`:343).
const CHAT_BASE: f32 = 28.0;

/// The bar's x, the source's translate origin (`GuiNewChat.java`:49-51).
const CHAT_X: f32 = 2.0;

/// One line's pitch in pixels (`GuiNewChat.java`:348-351).
const LINE_PITCH: f32 = 9.0;

/// The bar's height: the pitch's own nine (`GuiNewChat.java`:81-82 draws `j2 - 9` to
/// `j2`).
const BAR_HEIGHT: f32 = 9.0;

/// The bar's width: the wrap budget plus four (`GuiNewChat.java`:82,
/// `getChatWidth`:343-346).
const BAR_WIDTH: f32 = CHAT_WIDTH as f32 + 4.0;

/// The record line's hold in ticks: `recordPlayingUpFor = 60`
/// (`GuiIngame.java`:1118-1122), decremented once per tick (`:1070-1073`).
const SYSTEM_HOLD: u64 = 60;

/// The field's frame: `drawRect(2, height - 14, width - 2, height - 2)` at
/// `Integer.MIN_VALUE` — black at half alpha (`GuiChat.java`:303). Two pixels in
/// from each side, twelve tall, its bottom edge two above the screen's.
const INPUT_FRAME_X: f32 = 2.0;
const INPUT_FRAME_ABOVE: f32 = 14.0;
const INPUT_FRAME_HEIGHT: f32 = 12.0;

/// The frame's colour: `Integer.MIN_VALUE` — black at half alpha
/// (`GuiChat.java`:303).
const INPUT_SHADE: [f32; 4] = [0.0, 0.0, 0.0, 128.0 / 255.0];

/// The field text's x and the text's distance above the bottom edge: the source's
/// pen `(4, height - 12)` (`GuiChat.java`:58) — the textbox's own x and y with
/// `setEnableBackgroundDrawing(false)` (`GuiChat.java`:60).
const INPUT_PEN_X: f32 = 4.0;
const INPUT_PEN_ABOVE: f32 = 12.0;

/// The field text's colour: `GuiTextField.enabledColor`, `14737632` = 0xE0E0E0,
/// opaque (`GuiTextField.java`:52).
const INPUT_TEXT_COLOUR: [f32; 4] = [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0];

/// The caret bar's colour: `-3092272` = 0xFFD0D0D0 (`GuiTextField.java`:578).
const INPUT_CARET_COLOUR: [f32; 4] = [208.0 / 255.0, 208.0 / 255.0, 208.0 / 255.0, 1.0];

/// The caret bar's height: the source draws `i1 - 1` to `i1 + 1 + 9`
/// (`GuiTextField.java`:578) — eleven pixels at the nine-pixel font line.
const INPUT_CARET_HEIGHT: f32 = 11.0;

/// The tooltip's fill: `-267386864` = 0xF0100010 (`GuiScreen.java`:230-235) — the
/// source's five gradient rects all at this colour union to one flat box.
const TOOLTIP_FILL: [f32; 4] = [16.0 / 255.0, 0.0, 16.0 / 255.0, 240.0 / 255.0];

/// The tooltip border's top stop: `1347420415` = 0x505000FF (`GuiScreen.java`:236-241).
const TOOLTIP_BORDER_TOP: [f32; 4] = [80.0 / 255.0, 0.0, 1.0, 80.0 / 255.0];

/// The tooltip border's bottom stop: `(i1 & 16711422) >> 1 | i1 & -16777216` =
/// 0x5028007F (`GuiScreen.java`:238) — the vertical gradients' lower end, drawn flat
/// as the milestone's stand-in for the two-stop gradients.
const TOOLTIP_BORDER_BOTTOM: [f32; 4] = [40.0 / 255.0, 0.0, 127.0 / 255.0, 80.0 / 255.0];

/// The confirm overlay's dim: `drawDefaultBackground`'s first gradient stop,
/// `-1072689136` = 0xC0101010 (`GuiScreen.java`:668-677) — the same first-stop
/// stand-in the death view uses.
const CONFIRM_DIM: [f32; 4] = [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 192.0 / 255.0];

/// The confirm overlay's two-key prompt, drawn in the title's slot
/// (`GuiYesNo.drawScreen`:72, centred at seventy): the source's own screen is three
/// buttons, and this milestone's two keys stand in for it.
const CONFIRM_PROMPT: &str = "Enter opens the link, Escape cancels";

/// The chat mirror: the session's chat messages, and the draws a frame shows for them.
///
/// The messages land from [`ClientEvent::Chat`]: a message at position code `2` becomes
/// the record line above the hotbar, and every other position enters the box's log —
/// `NetHandlerPlayClient.java`:849-861 sends `2` to `setRecordPlaying` and the rest to
/// `printChatMessage`. The log ([`ChatLog`]) owns the split, the fade clocks and the
/// scroll state; the mirror adds what only the frame knows: the scaled resolution and
/// the per-frame draw assembly.
///
/// The assembly is the source's chat block (`GuiIngame.java`:339-347) over
/// `GuiNewChat.drawChat`:30-114. The box: the newest line's base sits [`CHAT_BASE`]
/// pixels above the bottom edge with a [`LINE_PITCH`]-pixel pitch, each line a black
/// bar at `alpha / 2`, four pixels wider than the 320-pixel wrap budget (`:82` over
/// `GuiNewChat.getChatWidth`:343-346 and `calculateChatboxWidth`:361-366), its text one
/// pixel below the bar's top at the line's alpha (`GuiNewChat.drawChat`:85). The record
/// line is centred above the hotbar, holds for [`SYSTEM_HOLD`]
/// ticks and fades as the tick it arrived at recedes (`GuiIngame.java`:245-272,
/// `:1118-1122`, `:1166-1169`).
pub struct ChatView {
    /// The split lines, the scroll state and the fade clock.
    log: ChatLog,
    /// The measured font the wrap and the runs' widths use; the client hands it over
    /// when the asset store lands.
    font: Option<Font>,
    /// The language table translation components resolve against; empty until the
    /// client hands the store's table over.
    language: LanguageTable,
    /// Messages that arrived before the font, parsed and waiting — the mirror cannot
    /// wrap them yet. Bounded at [`LOG_CAP`], oldest dropped first: what the log itself
    /// would keep.
    pending: Vec<(TextComponent, u64)>,
    /// The system line — the newest position-2 message — and the tick it arrived at.
    system: Option<(String, u64)>,
    /// The tick a frame draws at; the record line's own fade clock.
    tick: u64,
    /// The hover tooltip the frame draws: the `show_text` component under the free
    /// pointer and the scaled point it shows at — fed by [`ChatView::feed_hover`]
    /// every frame the chat screen is the open one
    /// (`GuiChat.drawScreen`:305-310 resolves it from the free mouse).
    tooltip: Option<(TextComponent, (f32, f32))>,
    /// The confirm overlay's URL while it stands in for the replaced chat screen
    /// (`GuiScreen.java`:425-429): raised by the window's link click, ended by the
    /// overlay's two keys.
    confirm: Option<String>,
}

impl ChatView {
    /// An empty mirror, closed, at tick zero, with no font.
    pub fn new() -> Self {
        Self {
            log: ChatLog::new(),
            font: None,
            language: LanguageTable::new(),
            pending: Vec::new(),
            system: None,
            tick: 0,
            tooltip: None,
            confirm: None,
        }
    }

    /// Sets the language table translation components resolve against. Call it
    /// before [`ChatView::set_font`]: the messages waiting on the font resolve
    /// when it lands.
    pub fn set_language(&mut self, language: LanguageTable) {
        self.language = language;
    }

    /// Sets the measured font and wraps whatever arrived before it — a message can
    /// outrun the asset store by a frame.
    pub fn set_font(&mut self, font: Font) {
        for (component, tick) in self.pending.drain(..) {
            self.log
                .push(&component, tick, CHAT_WIDTH, &font, &self.language);
        }
        self.font = Some(font);
    }

    /// Folds one [`ClientEvent::Chat`] message in: position `2` replaces the record
    /// line, everything else enters the log at `tick`
    /// (`NetHandlerPlayClient.java`:849-861).
    pub fn observe(&mut self, text: &str, position: i8, tick: u64) {
        let component = chat::parse_json(text);
        if position == 2 {
            // The record line is the message's unformatted text — every element's own
            // characters, styles cut (`GuiIngame.java`:1166-1169 over
            // `ChatComponentStyle.java`:72-81) — a translation resolved first, as the
            // source's own unformatted text resolves it.
            self.system = Some((plain_text(&chat::resolve(&component, &self.language)), tick));
            return;
        }
        match &self.font {
            Some(font) => self
                .log
                .push(&component, tick, CHAT_WIDTH, font, &self.language),
            None => {
                self.pending.push((component, tick));
                if self.pending.len() > LOG_CAP {
                    self.pending.remove(0);
                }
            }
        }
    }

    /// Ages the mirror to `tick`: the log's fade reads against it and the record line's
    /// hold counts from its receipt.
    pub fn update(&mut self, tick: u64) {
        self.tick = tick;
        self.log.update(tick);
    }

    /// Sets whether the chat window is open: the drawn line count and the log's
    /// pinning follow it (`GuiNewChat.getChatOpen`:305-308). The chat screen that
    /// opens and scrolls the box is a later milestone's, so nothing calls this yet.
    #[allow(dead_code)]
    pub fn set_open(&mut self, open: bool) {
        self.log.set_open(open);
    }

    /// Scrolls the box by `amount` lines, the source's own clamp
    /// (`GuiNewChat.scroll`:222-237). The chat screen's wheel input is a later
    /// milestone's, so the tests are the only caller for now.
    #[allow(dead_code)]
    pub fn scroll(&mut self, amount: i32) {
        self.log.scroll(amount);
    }

    /// Resets the scroll (`GuiNewChat.resetScroll`:211-215). Called when the chat
    /// screen closes, which a later milestone lands.
    #[allow(dead_code)]
    pub fn reset_scroll(&mut self) {
        self.log.reset_scroll();
    }

    /// The frame's draw list at `resolution`.
    ///
    /// The record line first (`GuiIngame.java`:245-272 draws before the chat block at
    /// `:339-347`), then the box's lines newest first, then the chat screen's own
    /// furniture when its state asks for it: the open field's line and the hover
    /// tooltip (`GuiChat.drawScreen`:303-310), and the confirm overlay — which
    /// stands in for the replaced screen, so neither the field's line nor the
    /// tooltip draws under it (`GuiScreen.java`:425-429 swaps the chat screen for
    /// the confirm one, `Minecraft.java`:1010-1012). With no field line, no
    /// tooltip and no overlay the frame is the box's frame, unchanged. Empty until
    /// the font is set — nothing can be measured before it.
    pub fn draws(&self, resolution: ScaledResolution, input: &ChatInput) -> Vec<HudDraw> {
        let Some(font) = &self.font else {
            return Vec::new();
        };
        let mut draws = Vec::new();
        self.system_draws(font, resolution, &mut draws);
        self.box_draws(resolution, &mut draws);
        // The chat screen's own furniture draws while it is the screen on top: the
        // confirm overlay stands in for the replaced chat screen
        // (`GuiScreen.java`:425-429 swaps it in, `Minecraft.java`:1010-1012), so
        // neither the field's line nor the tooltip draws under it.
        if self.confirm.is_some() {
            self.confirm_draws(font, resolution, &mut draws);
        } else {
            if input.open {
                self.input_draws(font, resolution, input, &mut draws);
            }
            self.tooltip_draws(font, resolution, &mut draws);
        }
        draws
    }

    /// The record line: centred above the hotbar, white, unshadowed, while its sixty
    /// ticks have not run out (`GuiIngame.java`:245-272).
    fn system_draws(&self, font: &Font, resolution: ScaledResolution, draws: &mut Vec<HudDraw>) {
        let Some((text, received)) = &self.system else {
            return;
        };
        let remaining = SYSTEM_HOLD.saturating_sub(self.tick.saturating_sub(*received));
        if remaining == 0 {
            return;
        }
        // `l1 = (int)(f2 * 255.0F / 20.0F)` at whole ticks, clamped to 255
        // (`GuiIngame.java`:248-254), drawn while it clears eight (`:256`); the
        // source subtracts the frame's partial tick, which this port leaves to the
        // tick the frame draws at.
        let alpha = (((remaining as f32) * 255.0 / 20.0) as u32).min(255) as u8;
        if alpha <= 8 {
            return;
        }
        // The line is centred: the translate's `width / 2` minus half the text's own
        // width, both integer divisions (`GuiIngame.java`:259, `:269`).
        let half = (string_width(font, text) / 2) as f32;
        draws.push(HudDraw::Text {
            text: text.clone(),
            x: (resolution.width / 2) as f32 - half,
            y: resolution.height as f32 - 72.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, f32::from(alpha) / 255.0],
            shadow: false,
            blend: true,
        });
    }

    /// The box's drawn lines, newest first: one bar and one text each
    /// (`GuiNewChat.drawChat`:53-91).
    fn box_draws(&self, resolution: ScaledResolution, draws: &mut Vec<HudDraw>) {
        let factor = opacity_factor(CHAT_OPACITY);
        for (index, line) in self.log.drawn().iter().enumerate() {
            // The source's opacity multiply and draw gate (`GuiNewChat.java`:75-78).
            let alpha = (f32::from(line.alpha) * factor) as u8;
            if alpha <= 3 {
                continue;
            }
            // The bar's top: the box's own bottom-anchored step, shared with the
            // hit-test so the two cannot drift ([`line_top`]).
            let top = line_top(resolution, index);
            draws.push(HudDraw::Rect {
                x: CHAT_X,
                y: top,
                width: BAR_WIDTH,
                height: BAR_HEIGHT,
                colour: [0.0, 0.0, 0.0, f32::from(alpha / 2) / 255.0],
            });
            draws.push(HudDraw::Text {
                text: run_text(line.runs),
                x: CHAT_X,
                y: top + 1.0,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, f32::from(alpha) / 255.0],
                shadow: true,
                blend: true,
            });
        }
    }

    /// The open field's own line (`GuiChat.drawScreen`:303-304 over
    /// `GuiTextField.drawTextBox`:525-592): the frame across the screen's bottom,
    /// the text at the field's pen, and the caret riding the blink
    /// ([`ChatInput::cursor_visible`]).
    fn input_draws(
        &self,
        font: &Font,
        resolution: ScaledResolution,
        input: &ChatInput,
        draws: &mut Vec<HudDraw>,
    ) {
        let height = resolution.height as f32;
        draws.push(HudDraw::Rect {
            x: INPUT_FRAME_X,
            y: height - INPUT_FRAME_ABOVE,
            width: resolution.width as f32 - 2.0 * INPUT_FRAME_X,
            height: INPUT_FRAME_HEIGHT,
            colour: INPUT_SHADE,
        });
        let pen_y = height - INPUT_PEN_ABOVE;
        let text = input.text.as_str();
        if text.is_empty() {
            if input.cursor_visible() {
                draws.push(HudDraw::Text {
                    text: "_".to_owned(),
                    x: INPUT_PEN_X,
                    y: pen_y,
                    scale: 1.0,
                    colour: INPUT_TEXT_COLOUR,
                    shadow: true,
                    blend: true,
                });
            }
            return;
        }
        // `:551-558`: the prefix up to the cursor draws first; the pen after it
        // is where the caret and the rest hang.
        let prefix = &text[..input.cursor];
        let pen = INPUT_PEN_X + string_width(font, prefix) as f32;
        if !prefix.is_empty() {
            draws.push(HudDraw::Text {
                text: prefix.to_owned(),
                x: INPUT_PEN_X,
                y: pen_y,
                scale: 1.0,
                colour: INPUT_TEXT_COLOUR,
                shadow: true,
                blend: true,
            });
        }
        // `:571-582`: with the cursor at the text's end — or the field full, the
        // source's own extra condition — the caret is the bar straddling
        // `pen - 1`, and the text after the cursor draws from that stepped-back
        // pen; otherwise the caret is the underscore at the pen.
        let bar = input.cursor < text.len() || text.chars().count() >= CHAT_TEXT_CAP;
        let tail = &text[input.cursor..];
        if bar {
            if !tail.is_empty() {
                draws.push(HudDraw::Text {
                    text: tail.to_owned(),
                    x: pen - 1.0,
                    y: pen_y,
                    scale: 1.0,
                    colour: INPUT_TEXT_COLOUR,
                    shadow: true,
                    blend: true,
                });
            }
            if input.cursor_visible() {
                draws.push(HudDraw::Rect {
                    x: pen - 1.0,
                    y: pen_y - 1.0,
                    width: 1.0,
                    height: INPUT_CARET_HEIGHT,
                    colour: INPUT_CARET_COLOUR,
                });
            }
        } else if input.cursor_visible() {
            draws.push(HudDraw::Text {
                text: "_".to_owned(),
                x: pen,
                y: pen_y,
                scale: 1.0,
                colour: INPUT_TEXT_COLOUR,
                shadow: true,
                blend: true,
            });
        }
    }

    /// The hover tooltip for the state the frame's feed left
    /// (`GuiScreen.handleComponentHover`'s SHOW_TEXT branch, `:339-341`, over
    /// `GuiScreen.drawHoveringText`:189-263): the fill and border box at the cursor's
    /// point, and the hover's text in white, eight and then twelve pixels down
    /// the box's own lines.
    ///
    /// The source splits the hover at its newlines (`:245`); the port wraps the
    /// formatted text at the GUI width — the bound the source's own overflow
    /// flip names (`l1 + i > this.width`, `:218-221`) — so a long hover cannot
    /// run off the screen. The vertical gradient edges draw flat at their first
    /// stop, the milestone's stand-in as the death view records.
    fn tooltip_draws(&self, font: &Font, resolution: ScaledResolution, draws: &mut Vec<HudDraw>) {
        let Some((component, point)) = &self.tooltip else {
            return;
        };
        let (x, y) = *point;
        let lines = chat::wrap(&chat::flatten(component), resolution.width as i32, font);
        if lines.is_empty() {
            return;
        }
        let widths: Vec<i32> = lines
            .iter()
            .map(|line| string_width(font, &run_text(line)))
            .collect();
        // The source's own bounds (`:211-215`): the widest text, and eight
        // pixels plus a ten-pixel line per further line.
        let i = widths.iter().copied().max().unwrap_or(0) as f32;
        let k = if lines.len() > 1 {
            8.0 + 2.0 + (lines.len() as f32 - 1.0) * 10.0
        } else {
            8.0
        };
        let mut l1 = x + 12.0;
        let mut i2 = y - 12.0;
        if l1 + i > resolution.width as f32 {
            l1 -= 28.0 + i;
        }
        if i2 + k + 6.0 > resolution.height as f32 {
            i2 = resolution.height as f32 - k - 6.0;
        }
        // The fill: the source's five same-colour gradient rects union to one box
        // from `(l1 - 4, i2 - 4)` to `(l1 + i + 4, i2 + k + 4)` (`:230-235`).
        draws.push(HudDraw::Rect {
            x: l1 - 4.0,
            y: i2 - 4.0,
            width: i + 8.0,
            height: k + 8.0,
            colour: TOOLTIP_FILL,
        });
        // The border (`:236-241`): both edges and the top strip at the top stop,
        // the bottom strip at its own.
        draws.push(HudDraw::Rect {
            x: l1 - 3.0,
            y: i2 - 2.0,
            width: 1.0,
            height: k + 4.0,
            colour: TOOLTIP_BORDER_TOP,
        });
        draws.push(HudDraw::Rect {
            x: l1 + i + 2.0,
            y: i2 - 2.0,
            width: 1.0,
            height: k + 4.0,
            colour: TOOLTIP_BORDER_TOP,
        });
        draws.push(HudDraw::Rect {
            x: l1 - 3.0,
            y: i2 - 3.0,
            width: i + 6.0,
            height: 1.0,
            colour: TOOLTIP_BORDER_TOP,
        });
        draws.push(HudDraw::Rect {
            x: l1 - 3.0,
            y: i2 + k + 2.0,
            width: i + 6.0,
            height: 1.0,
            colour: TOOLTIP_BORDER_BOTTOM,
        });
        // The text (`:243-254`): white and shadowed, ten pixels a line with the
        // first line's own extra two.
        let mut ty = i2;
        for (index, line) in lines.iter().enumerate() {
            draws.push(HudDraw::Text {
                text: run_text(line),
                x: l1,
                y: ty,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            });
            if index == 0 {
                ty += 2.0;
            }
            ty += 10.0;
        }
    }

    /// The interim confirm overlay — the port's stand-in for the source's
    /// screen flow (`clickedLinkURI` and the swap to `GuiConfirmOpenLink`,
    /// `GuiScreen.java`:403-433): the dim of `drawDefaultBackground`'s first stop
    /// (`GuiScreen.java`:668-677), the two-key prompt in the title's slot
    /// (`GuiYesNo.drawScreen`:72) and the URL in the message's — centred, wrapped
    /// at the source's `width - 50` budget (`initGui`:55), a font line per line
    /// from ninety down (`GuiYesNo.drawScreen`:73-79).
    fn confirm_draws(&self, font: &Font, resolution: ScaledResolution, draws: &mut Vec<HudDraw>) {
        let Some(url) = &self.confirm else {
            return;
        };
        draws.push(HudDraw::Rect {
            x: 0.0,
            y: 0.0,
            width: resolution.width as f32,
            height: resolution.height as f32,
            colour: CONFIRM_DIM,
        });
        // The prompt: centred at seventy, the source's integer halving of both
        // terms (`drawCenteredString` over `GuiYesNo.drawScreen`:72).
        let half = (string_width(font, CONFIRM_PROMPT) / 2) as f32;
        draws.push(HudDraw::Text {
            text: CONFIRM_PROMPT.to_owned(),
            x: (resolution.width / 2) as f32 - half,
            y: 70.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        });
        // The URL, wrapped at `width - 50` and centred the same way, stepping a
        // font line per line (`GuiYesNo.drawScreen`:73-79, the source's
        // `fontRendererObj.listFormattedStringToWidth`).
        let runs = [chat::StyledRun {
            text: url.clone(),
            colour: None,
            styles: 0,
            click: None,
            hover: None,
        }];
        let mut y = 90.0;
        for line in &chat::wrap(&runs, resolution.width as i32 - 50, font) {
            let text = run_text(line);
            let half = (string_width(font, &text) / 2) as f32;
            draws.push(HudDraw::Text {
                text,
                x: (resolution.width / 2) as f32 - half,
                y,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            });
            y += font.height() as f32;
        }
    }

    /// Feeds one frame's hover: the tooltip of the run under `point` — shown at
    /// the point — or none. The frame calls this every frame the chat screen is
    /// the open one, with the free pointer; the feed overwrites the last frame's
    /// result, and there is no delay
    /// (`GuiChat.drawScreen`:305-310 resolves the hovered component from the
    /// free mouse the same way).
    pub fn feed_hover(&mut self, point: Option<(f32, f32)>, resolution: ScaledResolution) {
        let mut tooltip = None;
        if let Some(point) = point {
            if let Some(run) = self.run_at(point, resolution) {
                if let Some(chat::HoverEvent::ShowText(component)) = &run.hover {
                    tooltip = Some((component.as_ref().clone(), point));
                }
            }
        }
        self.tooltip = tooltip;
    }

    /// The run under a scaled-GUI point — the box's hit-test for the click path
    /// (`GuiChat.mouseClicked`:172-186 over `GuiNewChat.getChatComponent`:245-300).
    ///
    /// The point is in the frame's own GUI units. Every drawn line's vertical
    /// band is [`line_top`]'s — the same geometry [`ChatView::box_draws`] lays
    /// out, so a hit cannot drift from the draws — and within the band the runs
    /// are walked in draw order: the first whose pen the point has not passed is
    /// the one under it. A point left of the box's origin hits nothing (the
    /// source's `j < 0`), and a point past a line's last glyph hits nothing
    /// either. The source gates on `getChatOpen` (`:247-250`); in this port the
    /// field's own state is that gate, read by the callers, and a closed chat is
    /// not asked.
    pub fn run_at(
        &self,
        point: (f32, f32),
        resolution: ScaledResolution,
    ) -> Option<&chat::StyledRun> {
        let font = self.font.as_ref()?;
        let (x, y) = point;
        if x < CHAT_X {
            return None;
        }
        let drawn = self.log.drawn();
        for (index, line) in drawn.iter().enumerate() {
            let top = line_top(resolution, index);
            if y < top || y >= top + BAR_HEIGHT {
                continue;
            }
            let mut pen = CHAT_X;
            for run in line.runs {
                let width = string_width(font, &run.text) as f32;
                if x < pen + width {
                    return Some(run);
                }
                pen += width;
            }
            return None;
        }
        None
    }

    /// Raises the confirm overlay on `url` — the source's `clickedLinkURI`
    /// carrying the link into the confirm screen it swaps in
    /// (`GuiScreen.java`:403-433, `:425-429`). While it is up it stands in for
    /// the chat screen: the field's line and the tooltip do not draw under it.
    pub fn open_confirm(&mut self, url: &str) {
        self.confirm = Some(url.to_owned());
    }

    /// Whether the confirm overlay is up.
    pub fn confirm_open(&self) -> bool {
        self.confirm.is_some()
    }

    /// Cancels the confirm overlay — the source's cancel answer re-displays the
    /// chat screen it replaced (`GuiScreen.confirmClicked`:713-725 reaches
    /// `displayGuiScreen(this)` for either answer). The field beneath is
    /// untouched.
    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
    }

    /// Takes the overlay's URL, clearing it — the Enter path, which opens the
    /// link once and returns to the chat (`confirmClicked`'s true answer,
    /// `:713-719`).
    pub fn take_confirm(&mut self) -> Option<String> {
        self.confirm.take()
    }
}

/// The top edge of the drawn line at `index`, zero the newest — the box's own
/// bottom-anchored step, shared by [`ChatView::box_draws`] and
/// [`ChatView::run_at`].
///
/// The newest line's base sits [`CHAT_BASE`] pixels above the bottom edge
/// (`GuiNewChat.java`:49-51 under `GuiIngame.java`:343) and each further line
/// one [`LINE_PITCH`] up (`GuiNewChat.java`:81-82).
fn line_top(resolution: ScaledResolution, index: usize) -> f32 {
    resolution.height as f32 - CHAT_BASE - LINE_PITCH * (index as f32 + 1.0)
}

/// Puts the chat view's open state back in step with the field's — the frame's
/// own reconciliation: `chat.set_open(input.open)`.
///
/// The window's open and close drive both halves together
/// (`ClientApp::open_chat` sets the field and the view; `close_chat` clears
/// both), but the script's `chat` line drives the field alone — a rig run has
/// no window and no pointer to free — so the frame re-couples them there: the
/// scripted open draws the open chat, and the scripted send closes it
/// (`GuiChat` is the screen and its field at once in the source;
/// `Minecraft.java`:1010-1012 swaps both halves).
pub fn reconcile_chat_open(chat: &mut ChatView, input: &ChatInput) {
    chat.set_open(input.open);
}

/// The icon one hud item draw carries: the wire's item stack as the pass's own minimal
/// view, the enchant flag read from the stack's own rule.
///
/// The source's gate is `stack.hasEffect()` (`RenderItem.renderItem`:160-163 →
/// `ItemStack.hasEffect`:859-861), the item's own `hasEffect(stack)` (`Item.hasEffect`
/// :416-419): the NBT default — a root compound with an `ench` list
/// (`isItemEnchanted`:902-905, presence not contents) — with six class overrides, each
/// replacing it: the always-true `ItemEnchantedBook`:16-19 (the enchanted book, 403),
/// `ItemEditableBook`:146-149 (the written book, 387), `ItemExpBottle`:16-19 (the bottle
/// o' enchanting, 384) and `ItemSimpleFoiled`:5-8 (the nether star, 399 — the class's
/// own registration at `Item.java`:909); the golden apple's `stack.getMetadata() > 0`
/// (`ItemAppleGold`:18-21, 322) and the potion's effect list non-empty
/// (`ItemPotion`:330-334, 373 — [`potion_has_effects`]'s damage decode; the stack's own
/// `CustomPotionEffects` tag is not read here). The ids are the registrations' own
/// (`Item.registerItems`:831, :883, :894, :897, :909, :913).
///
/// The conversion is the draw-list seam: a slot's stack becomes this view when a frame
/// builds its draws, and nothing else of the stack reaches the pass — the icon draw
/// reads the id, the damage and the flag, and the pass gates the glint the way the
/// source's draw does (a builtin shape never glints, `RenderItem.renderItem`:154-165).
/// The held item's own frame calls it; the hotbar draws that call it are the next
/// task's.
pub(crate) fn item_icon(stack: &MetadataItem) -> ItemIcon {
    ItemIcon {
        id: stack.id,
        damage: stack.damage,
        enchanted: stack_has_effect(stack),
    }
}

/// The five equipment slots a frame's stacks convert to: each slot through the same
/// item-icon seam rule — a slot's stack becomes the draw's reduced view, and nothing else
/// of the stack crosses the crate edge.
fn equipment_draw(equipment: &[Option<MetadataItem>; 5]) -> [Option<EquipmentDraw>; 5] {
    std::array::from_fn(|slot| equipment[slot].as_ref().map(equipment_stack))
}

/// One stack's conversion: the id, damage and effect flag the icon seam reads, the raw
/// `display.color` int the leather dye reads and the cross flag the held item's block
/// branch reads (`LayerHeldItem.java`:52-59's `getRenderType() == 2` test, folded by the
/// behaviour table's own cross kind).
fn equipment_stack(stack: &MetadataItem) -> EquipmentDraw {
    EquipmentDraw {
        id: stack.id,
        damage: stack.damage,
        enchanted: stack_has_effect(stack),
        colour: display_colour(stack),
        cross: matches!(
            items::resolve(stack.id, stack.damage),
            items::ItemResolution::Block(block)
                if behaviour::behaviour(block).is_some_and(|entry| entry.render == RenderKind::Cross)
        ),
    }
}

/// The raw `display.color` int of a stack's NBT tail, when the tag carries one
/// (`ItemArmor.hasColor`:127-130's `hasKey("display", 10)` fold through
/// `getColor`:135-157): the root compound's `display` child's `color` int. A tail that
/// is not a compound, or does not parse, carries none.
fn display_colour(stack: &MetadataItem) -> Option<i32> {
    let nbt = stack.nbt.as_deref()?;
    let Ok(NbtValue::Compound(children)) = oxide_proto_v47::nbt::parse(nbt) else {
        return None;
    };
    let Some((_, NbtValue::Compound(display))) =
        children.iter().find(|(name, _)| name == "display")
    else {
        return None;
    };
    display
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("color", NbtValue::Int(colour)) => Some(*colour),
            _ => None,
        })
}

/// The source's `hasEffect` rule for one stack (`Item.hasEffect`:416-419 and its six
/// overrides), by registration id: each override replaces the default rather than
/// extending it.
fn stack_has_effect(stack: &MetadataItem) -> bool {
    match stack.id {
        // The always-true registrations: the enchanted book, the written book, the
        // bottle o' enchanting and the nether star.
        403 | 387 | 384 | 399 => true,
        // The enchanted golden apple: any metadata above the plain apple's 0.
        322 => stack.damage > 0,
        // Potions: the effect list non-empty.
        373 => potion_has_effects(stack.damage),
        // Everything else: the NBT default.
        _ => stack.nbt.as_deref().is_some_and(root_has_ench),
    }
}

/// The union of the source's thirteen potion requirement patterns over a damage's low
/// four bits (`PotionHelper.potionRequirements`:581-593, read through
/// `parsePotionEffects`:220-341): bit `n` set means a damage whose low nibble is `n`
/// answers at least one requirement, i.e. its effect list is non-empty. The holes are
/// 0 (water), 7 and 15.
const POTION_EFFECT_FLAGS: u16 = 0x7F7E;

/// The potion arm's damage decode: whether `ItemPotion.getEffects`:40-70's damage path
/// — `PotionHelper.getPotionEffects(meta, false)`:386-449 — answers with at least one
/// effect, which is what `ItemPotion.hasEffect`:330-334 tests. Each requirement string
/// spells flag terms over the damage's low four bits (the parser's `&`/`!`/`+` terms:
/// a required bit set, its negated neighbours clear; the trailing `+6` terms only add
/// to a count the required bit already carries), so the thirteen strings reduce to
/// [`POTION_EFFECT_FLAGS`]'s patterns. The splash bit (16384) and the tier bits (32,
/// 64) sit above the nibble and never change the answer. The metadata is the source's
/// own non-negative domain (`ItemStack.setItemDamage`:278-284 clamps).
fn potion_has_effects(damage: i16) -> bool {
    let flags = damage.max(0) as u16 & 0xF;
    (POTION_EFFECT_FLAGS >> flags) & 1 == 1
}

/// Whether the raw NBT tail's root compound carries an `ench` list
/// (`ItemStack.isItemEnchanted`:902-905's `hasKey("ench", 9)`); a tail that is not a
/// compound, or does not parse, is not enchanted.
fn root_has_ench(nbt: &[u8]) -> bool {
    matches!(
        oxide_proto_v47::nbt::parse(nbt),
        Ok(NbtValue::Compound(children))
            if children
                .iter()
                .any(|(name, value)| name == "ench" && matches!(value, NbtValue::List(_)))
    )
}

/// One component's unformatted text: every element's own characters, `§` codes and
/// styles as sent, depth first — `ChatComponentStyle.getUnformattedText`:72-81, the
/// walk the record line reads.
fn plain_text(component: &TextComponent) -> String {
    let mut text = component.text.clone();
    for child in &component.children {
        text.push_str(&plain_text(child));
    }
    text
}

/// One line's runs as the `§`-coded string the text builder draws: the source's
/// `getFormattedText` shape (`ChatComponentStyle.java`:87-99) with
/// `ChatStyle.getFormattingCode`:306-346 — each run's colour and style codes in the
/// source's order, its characters, then a reset.
fn run_text(runs: &[chat::StyledRun]) -> String {
    let mut text = String::new();
    for run in runs {
        if let Some(index) = run.colour {
            text.push('§');
            text.push(char::from(PALETTE[usize::from(index.min(15))]));
        }
        if run.styles & STYLE_BOLD != 0 {
            text.push_str("§l");
        }
        if run.styles & STYLE_ITALIC != 0 {
            text.push_str("§o");
        }
        if run.styles & STYLE_UNDERLINED != 0 {
            text.push_str("§n");
        }
        if run.styles & STYLE_OBFUSCATED != 0 {
            text.push_str("§k");
        }
        if run.styles & STYLE_STRIKETHROUGH != 0 {
            text.push_str("§m");
        }
        text.push_str(&run.text);
        text.push_str("§r");
    }
    text
}

/// The source's chat-opacity factor (`GuiNewChat.java`:38): `chatOpacity * 0.9 + 0.1`,
/// all f32. At the settings default `1.0` (`GameSettings.java`:85) the sum lands on
/// `1.0` exactly, so the fade byte passes through untouched.
fn opacity_factor(chat_opacity: f32) -> f32 {
    chat_opacity * 0.9 + 0.1
}

// ---- the tab list ----

// The held player list: the window's Tab edge sets the held state, the frame drains the
// session's events into it and appends its draws after the chat's, and the tests drive the
// whole assembly.

/// The tab list's top margin in pixels: the source's own `k1 = 10`
/// (`GuiPlayerTabOverlay.renderPlayerlist`:120).
const TAB_TOP: i32 = 10;

/// One row's pitch: the source's own nine-pixel step (`GuiPlayerTabOverlay.renderPlayerlist`:166).
const TAB_ROW_PITCH: i32 = 9;

/// The gap between columns: the source's own five (`GuiPlayerTabOverlay.renderPlayerlist`:119).
const TAB_GUTTER: i32 = 5;

/// The width the whole block is held under: the source's own fifty-pixel margin
/// (`GuiPlayerTabOverlay.renderPlayerlist`:118, `:127`, `:137`).
const TAB_SIDE_MARGIN: i32 = 50;

/// The rows a column holds before the split: the source's own twenty
/// (`GuiPlayerTabOverlay.renderPlayerlist`:94).
const TAB_ROWS_PER_COLUMN: i32 = 20;

/// The most entries the list keeps, after the sort: the source's own `min(size, 80)`
/// (`GuiPlayerTabOverlay.renderPlayerlist`:89).
const TAB_CAP: usize = 80;

/// The head column's own nine pixels in the cell-width arithmetic
/// (`GuiPlayerTabOverlay.renderPlayerlist`:118) and the name's offset past the head (`:195`).
const TAB_HEAD: i32 = 9;

/// A cell's fixed padding beyond the head, name and score fields: the source's own thirteen
/// (`GuiPlayerTabOverlay.renderPlayerlist`:118) — a pixel of name-to-score gap, one more to the
/// ping, the ping's ten and a right margin.
const TAB_PADDING: i32 = 13;

/// The score field's width under a hearts objective: the source's own ninety
/// (`GuiPlayerTabOverlay.renderPlayerlist`:104-106).
const TAB_HEARTS_FIELD: i32 = 90;

/// One cell background's height, inside the nine-pixel row pitch
/// (`GuiPlayerTabOverlay.renderPlayerlist`:167).
const TAB_CELL_HEIGHT: i32 = 8;

/// The grid, header and footer backgrounds: `Integer.MIN_VALUE` = 0x80000000 — black at half
/// alpha (`GuiPlayerTabOverlay.renderPlayerlist`:147, `:159`, `:226`).
const TAB_PANEL: [f32; 4] = [0.0, 0.0, 0.0, 128.0 / 255.0];

/// One entry's cell background: `553648127` = 0x20FFFFFF — white at alpha 32
/// (`GuiPlayerTabOverlay.renderPlayerlist`:167).
const TAB_CELL: [f32; 4] = [1.0, 1.0, 1.0, 32.0 / 255.0];

/// The header, footer, name and score text colour: the source's `-1` for the header, footer and
/// name (`GuiPlayerTabOverlay.renderPlayerlist`:152, `:205`, `:231`) and `16777215` for the
/// score (`GuiPlayerTabOverlay.drawScoreboardValues`:366`) — both opaque white, the score's own yellow coming from its `§e` prefix.
const TAB_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// A spectator's name colour: `-1862270977` = 0x90FFFFFF — white at alpha 144
/// (`GuiPlayerTabOverlay.renderPlayerlist`:201).
const TAB_SPECTATOR: [f32; 4] = [1.0, 1.0, 1.0, 144.0 / 255.0];

/// The opaque white the heads and icons tint through: the source's own `color(1, 1, 1, 1)`
/// before every entry's textures (`GuiPlayerTabOverlay.renderPlayerlist`:168, `drawPing`:239-240).
const TAB_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// The icon sheet's registered name — the `gui/icons.png` the latency bars and the heart glyphs
/// sample (`GuiPlayerTabOverlay.drawPing`:240, `drawScoreboardValues`:280).
const TAB_ICONS: &str = "gui/icons";

/// The icon sheet's texel size: the legacy sheet is 256×256.
const ICONS_SHEET: f32 = 256.0;

/// The face sub-rect of a skin sheet, eight-by-eight at (8, 8), over the source's sixty-four
/// texel modal space (`GuiPlayerTabOverlay.renderPlayerlist`:186).
const TAB_FACE_UV: [f32; 4] = [8.0 / 64.0, 8.0 / 64.0, 16.0 / 64.0, 16.0 / 64.0];

/// The hat overlay's sub-rect, eight-by-eight at (40, 8), over the same modal space
/// (`GuiPlayerTabOverlay.renderPlayerlist`:192).
const TAB_HAT_UV: [f32; 4] = [40.0 / 64.0, 8.0 / 64.0, 48.0 / 64.0, 16.0 / 64.0];

/// The gamemode byte a spectator carries: `SPECTATOR(3, "spectator")` (`WorldSettings.java`:141).
const SPECTATOR_GAMEMODE: u8 = 3;

/// The latency icon's level from a response time in milliseconds: the source's own walk —
/// negative is the no-signal five, then the 150/300/600/1000 steps
/// (`GuiPlayerTabOverlay.drawPing`:244-267).
fn latency_level(latency: i32) -> u8 {
    if latency < 0 {
        5
    } else if latency < 150 {
        0
    } else if latency < 300 {
        1
    } else if latency < 600 {
        2
    } else if latency < 1000 {
        3
    } else {
        4
    }
}

/// One latency icon's uv: the ten-by-eight sub-rect at `(0, 176 + 8 * level)` of the icon sheet
/// (`GuiPlayerTabOverlay.drawPing`:270) — the no-signal X at level five, `(0, 216)`.
fn latency_uv(level: u8) -> [f32; 4] {
    let v = 176.0 + 8.0 * f32::from(level);
    [
        0.0,
        v / ICONS_SHEET,
        10.0 / ICONS_SHEET,
        (v + 8.0) / ICONS_SHEET,
    ]
}

/// One heart glyph: the nine-by-nine sub-rect at `(u, 0)` of the icon sheet
/// (`GuiPlayerTabOverlay.drawScoreboardValues`:317-345).
fn heart_glyph(u: f32, x: f32, y: f32) -> HudDraw {
    HudDraw::TexturedRect {
        texture: HudTexture::Named(TAB_ICONS),
        x,
        y,
        width: 9.0,
        height: 9.0,
        uv: [
            u / ICONS_SHEET,
            0.0,
            (u + 9.0) / ICONS_SHEET,
            9.0 / ICONS_SHEET,
        ],
        colour: TAB_TINT,
    }
}

/// A float as `Float.toString` writes the hearts number
/// (`GuiPlayerTabOverlay.drawScoreboardValues`:352): the shortest round-trip digits with a
/// forced fraction — `29.0`, `28.5` — and scientific form at ten million and up, where Java
/// prints `1.0E7`.
fn java_float_text(value: f32) -> String {
    if value.abs() >= 1.0e7 {
        let scientific = format!("{value:e}");
        let (mantissa, exponent) = scientific.split_once('e').expect("a scientific form");
        let mantissa = if mantissa.contains('.') {
            mantissa.to_owned()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exponent}")
    } else {
        let text = format!("{value}");
        if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        }
    }
}

/// The points an entry carries in an objective: the stored value, zero where the board holds
/// none — the source reads through `getValueFromObjective`, which creates the missing score at
/// its zero default (`Scoreboard.getValueFromObjective`:96-120).
fn points_of(board: &Scoreboard, record: &PlayerListRecord, objective: &Objective) -> i32 {
    board
        .scores
        .get(&record.name)
        .and_then(|scores| scores.get(&objective.name))
        .copied()
        .unwrap_or(0)
}

/// A header or footer side's wrapped lines, when the side draws: the raw chat JSON parsed and
/// flattened, judged empty on its formatted text — the source nulls a side whose formatted text
/// has no characters (`NetHandlerPlayClient.java`:1600-1604) — and word-wrapped at `budget`
/// (`FontRenderer.java`:844-868), each line back as the `§`-coded string
/// the text builder draws.
fn tab_lines(raw: &str, budget: i32, font: &Font) -> Option<Vec<String>> {
    let component = chat::parse_json(raw);
    let runs = chat::flatten(&component);
    if run_text(&runs).is_empty() {
        return None;
    }
    let lines = chat::wrap(&runs, budget, font);
    Some(lines.iter().map(|line| run_text(line)).collect())
}

/// One row's score cell for an objective that is not hearts: the number in `§e` yellow,
/// right-aligned at the field's right edge, shadowed
/// (`GuiPlayerTabOverlay.drawScoreboardValues`:363-367).
fn score_text_draws(points: i32, right: i32, y: i32, font: &Font, draws: &mut Vec<HudDraw>) {
    let text = format!("§e{points}");
    draws.push(HudDraw::Text {
        x: (right - string_width(font, &text)) as f32,
        y: y as f32,
        scale: 1.0,
        colour: TAB_TEXT,
        shadow: true,
        text,
        blend: true,
    });
}

/// One row's score cell for a hearts objective
/// (`GuiPlayerTabOverlay.drawScoreboardValues`:278-362): the glyph row while the per-heart scale
/// stays over three pixels, the plain number otherwise.
///
/// The source's blink state machine and the per-entry prior value it reads (`:282-307`) are
/// fields this frame does not carry — no flash pass draws, and the prior value stands at its
/// zero default.
fn heart_draws(points: i32, left: i32, right: i32, y: i32, font: &Font, draws: &mut Vec<HudDraw>) {
    // The half-heart count and the slot count with its ten-slot floor (`:305-306`).
    let hearts = ((points.max(0) as f32) / 2.0).ceil() as i32;
    let slots = (points / 2).max(10);
    if hearts <= 0 {
        return;
    }
    let scale = (((right - left - 4) as f32) / (slots as f32)).min(9.0);
    if scale > 3.0 {
        // The empty containers beyond the filled hearts, then the filled row: a full heart
        // while its odd slot sits inside the points, the half heart on it (`:315-346`).
        for slot in hearts..slots {
            draws.push(heart_glyph(
                16.0,
                left as f32 + slot as f32 * scale,
                y as f32,
            ));
        }
        for index in 0..hearts {
            let (full, half) = if index >= 10 {
                (160.0, 169.0)
            } else {
                (52.0, 61.0)
            };
            let u = if index * 2 + 1 == points { half } else { full };
            draws.push(heart_glyph(u, left as f32 + index as f32 * scale, y as f32));
        }
    } else {
        // The number branch: the points over two, `hp` when the suffixed text still fits, in
        // the red-to-green health blend (`:348-360`).
        let fraction = (points as f32 / 20.0).clamp(0.0, 1.0);
        let red = (((1.0 - fraction) * 255.0) as u32) as f32 / 255.0;
        let green = ((fraction * 255.0) as u32) as f32 / 255.0;
        let mut text = java_float_text(points as f32 / 2.0);
        if right - string_width(font, &format!("{text}hp")) >= left {
            text.push_str("hp");
        }
        draws.push(HudDraw::Text {
            x: ((right + left) / 2 - string_width(font, &text) / 2) as f32,
            y: y as f32,
            scale: 1.0,
            colour: [red, green, 0.0, 1.0],
            shadow: true,
            text,
            blend: true,
        });
    }
}

/// The tab list's state and its frame assembly.
///
/// The session's events land here: [`ClientEvent::PlayerList`] replaces the entry set and
/// [`ClientEvent::TabText`] the header and footer pair, both as the session reports them, and
/// [`TabState::observe`] folds them in. The list draws while [`TabState::open`] — the held key
/// the window wires — and one frame's assembly is [`TabState::tab_draws`], the source's
/// `renderPlayerlist` (`GuiPlayerTabOverlay.renderPlayerlist`:70-235) in its own painter order,
/// as one draw list.
///
/// The head column is a deliberate difference: the source gates it on an integrated server or
/// an encrypted connection (`GuiPlayerTabOverlay.renderPlayerlist`:99;
/// `Minecraft.isIntegratedServerRunning`:2983; `NetworkManager.getIsencrypted`:412), and
/// against an offline server it draws no heads at all — this port draws every profile's head,
/// resolved through the skin registry's default fallback.
pub struct TabState {
    /// Whether the list draws this frame: the held state the window wires.
    pub open: bool,
    /// The player-list records as the latest [`ClientEvent::PlayerList`] reported them, in the
    /// event's own ascending-uuid order.
    pub entries: Vec<PlayerListRecord>,
    /// The header's raw chat JSON as the latest [`ClientEvent::TabText`] carried it.
    pub header: String,
    /// The footer's raw chat JSON, likewise.
    pub footer: String,
}

impl TabState {
    /// An unheld list with no entries and no header or footer.
    pub fn new() -> Self {
        Self {
            open: false,
            entries: Vec::new(),
            header: String::new(),
            footer: String::new(),
        }
    }

    /// Folds one session event in: [`ClientEvent::PlayerList`] replaces the entries,
    /// [`ClientEvent::TabText`] the header and footer pair. Every other event is ignored — the
    /// scoreboard the assembly reads travels beside the state, as the frame's own argument.
    pub fn observe(&mut self, event: &ClientEvent) {
        match event {
            ClientEvent::PlayerList { entries } => self.entries = entries.clone(),
            ClientEvent::TabText { header, footer } => {
                self.header = header.clone();
                self.footer = footer.clone();
            }
            _ => {}
        }
    }

    /// The held list's frame draws at `resolution`, one ordered list in the source's painter
    /// order (`GuiPlayerTabOverlay.renderPlayerlist`:145-234): the header block, the grid
    /// background, each row in turn — cell background, head, hat overlay, name, score, ping —
    /// and the footer block.
    ///
    /// The head textures resolve through `skins`, the same resolver the entity pass draws
    /// through; `slim` picks the default texture for a profile with nothing uploaded
    /// ([`default_skin`]'s own rule).
    pub fn tab_draws(
        &self,
        board: &Scoreboard,
        font: &Font,
        resolution: ScaledResolution,
        skins: &impl SkinLookup,
    ) -> Vec<HudDraw> {
        let head_id = |uuid: &str| {
            skins
                .resolve(uuid, default_skin(uuid) == DefaultModel::Slim)
                .id()
        };
        self.tab_draws_with(board, font, resolution, &head_id)
    }

    /// The assembly behind [`TabState::tab_draws`], its head resolution lifted to `head_id` so
    /// a test drives every draw without a registry.
    fn tab_draws_with(
        &self,
        board: &Scoreboard,
        font: &Font,
        resolution: ScaledResolution,
        head_id: &dyn Fn(&str) -> SkinTexId,
    ) -> Vec<HudDraw> {
        if !self.open {
            return Vec::new();
        }
        let width = resolution.width as i32;
        // The source's own order (`GuiPlayerTabOverlay.java`:392-397):
        // non-spectators first, then the team's registered name — the empty string when the
        // entry has no team — then the profile name; the eighty-entry cap follows the sort
        // (`GuiPlayerTabOverlay.renderPlayerlist`:89).
        let mut sorted: Vec<&PlayerListRecord> = self.entries.iter().collect();
        sorted.sort_by(|left, right| {
            (left.gamemode == SPECTATOR_GAMEMODE)
                .cmp(&(right.gamemode == SPECTATOR_GAMEMODE))
                .then_with(|| team_name(board, left).cmp(team_name(board, right)))
                .then_with(|| left.name.cmp(&right.name))
        });
        let capped = &sorted[..sorted.len().min(TAB_CAP)];
        let total = capped.len() as i32;
        // The name and score fields measure the sorted set before the cap
        // (`GuiPlayerTabOverlay.renderPlayerlist`:77-87).
        let objective = board.display[0]
            .as_deref()
            .and_then(|name| board.objectives.get(name));
        let mut name_width = 0;
        let mut score_width = 0;
        for record in &sorted {
            name_width = name_width.max(string_width(font, &self.name_of(board, record)));
            if let Some(objective) = objective {
                if objective.kind != "hearts" {
                    let points = points_of(board, record, objective);
                    score_width = score_width.max(string_width(font, &format!(" {points}")));
                }
            }
        }
        let score_field = match objective {
            None => 0,
            Some(objective) if objective.kind == "hearts" => TAB_HEARTS_FIELD,
            Some(_) => score_width,
        };
        // The column break (`GuiPlayerTabOverlay.renderPlayerlist`:90-97): the smallest column
        // count whose rows fit twenty.
        let mut columns = 1;
        let mut rows = total;
        while rows > TAB_ROWS_PER_COLUMN {
            columns += 1;
            rows = (total + columns - 1) / columns;
        }
        // The cell width and the grid's left edge (`GuiPlayerTabOverlay.renderPlayerlist`:118-121).
        let cell = (columns * (TAB_HEAD + name_width + score_field + TAB_PADDING))
            .min(width - TAB_SIDE_MARGIN)
            / columns;
        let grid_left = width / 2 - (cell * columns + (columns - 1) * TAB_GUTTER) / 2;
        let mut panel = cell * columns + (columns - 1) * TAB_GUTTER;
        let mut top = TAB_TOP;
        // The header and footer sides (`GuiPlayerTabOverlay.renderPlayerlist`:125-143), both
        // widening the panel and neither moving the top margin.
        let header = tab_lines(&self.header, width - TAB_SIDE_MARGIN, font);
        let footer = tab_lines(&self.footer, width - TAB_SIDE_MARGIN, font);
        for line in header.as_deref().unwrap_or(&[]) {
            panel = panel.max(string_width(font, line));
        }
        for line in footer.as_deref().unwrap_or(&[]) {
            panel = panel.max(string_width(font, line));
        }
        let mut draws = Vec::new();
        if let Some(lines) = &header {
            let height = lines.len() as i32 * TAB_ROW_PITCH;
            draws.push(HudDraw::Rect {
                x: (width / 2 - panel / 2 - 1) as f32,
                y: (top - 1) as f32,
                width: (panel + 2) as f32,
                height: (height + 1) as f32,
                colour: TAB_PANEL,
            });
            // The header's lines follow their own rect: blend stays off
            // (`GuiPlayerTabOverlay.java`:147-152).
            for (index, line) in lines.iter().enumerate() {
                draws.push(HudDraw::Text {
                    text: line.clone(),
                    x: (width / 2 - string_width(font, line) / 2) as f32,
                    y: (top + index as i32 * TAB_ROW_PITCH) as f32,
                    scale: 1.0,
                    colour: TAB_TEXT,
                    shadow: true,
                    blend: false,
                });
            }
            top += height + 1;
        }
        draws.push(HudDraw::Rect {
            x: (width / 2 - panel / 2 - 1) as f32,
            y: (top - 1) as f32,
            width: (panel + 2) as f32,
            height: (rows * TAB_ROW_PITCH + 1) as f32,
            colour: TAB_PANEL,
        });
        let grid_top = top;
        for (index, record) in capped.iter().enumerate() {
            let index = index as i32;
            let column = index / rows;
            let row = index % rows;
            let cell_x = grid_left + column * cell + column * TAB_GUTTER;
            let cell_y = grid_top + row * TAB_ROW_PITCH;
            draws.push(HudDraw::Rect {
                x: cell_x as f32,
                y: cell_y as f32,
                width: cell as f32,
                height: TAB_CELL_HEIGHT as f32,
                colour: TAB_CELL,
            });
            let head = head_id(&record.uuid);
            draws.push(HudDraw::SkinRect {
                texture: head,
                x: cell_x as f32,
                y: cell_y as f32,
                width: 8.0,
                height: 8.0,
                uv: TAB_FACE_UV,
                colour: TAB_TINT,
            });
            // The hat overlay draws with every part enabled — the byte this milestone pins
            // (`EnumPlayerModelParts.java`:14, `:24`; [`ALL_PARTS`]) — as the source's second
            // eight-by-eight pass over the face (`GuiPlayerTabOverlay.renderPlayerlist`:188-193).
            draws.push(HudDraw::SkinRect {
                texture: head,
                x: cell_x as f32,
                y: cell_y as f32,
                width: 8.0,
                height: 8.0,
                uv: TAB_HAT_UV,
                colour: TAB_TINT,
            });
            let name_x = cell_x + TAB_HEAD;
            let spectator = record.gamemode == SPECTATOR_GAMEMODE;
            let name = self.name_of(board, record);
            let (text, colour) = if spectator {
                // The source prefixes its own ITALIC code and draws the alpha colour
                // (`GuiPlayerTabOverlay.renderPlayerlist`:198-202).
                (format!("§o{name}"), TAB_SPECTATOR)
            } else {
                (name, TAB_TEXT)
            };
            draws.push(HudDraw::Text {
                text,
                x: name_x as f32,
                y: cell_y as f32,
                scale: 1.0,
                colour,
                shadow: true,
                blend: true,
            });
            if let Some(objective) = objective {
                if !spectator {
                    let left = name_x + name_width + 1;
                    let right = left + score_field;
                    if right - left > 5 {
                        let points = points_of(board, record, objective);
                        if objective.kind == "hearts" {
                            heart_draws(points, left, right, cell_y, font, &mut draws);
                        } else {
                            score_text_draws(points, right, cell_y, font, &mut draws);
                        }
                    }
                }
            }
            // The ping draws for every row (`GuiPlayerTabOverlay.renderPlayerlist`:219): the
            // icon sheet at the cell's right edge, ten wide and a pixel in.
            draws.push(HudDraw::TexturedRect {
                texture: HudTexture::Named(TAB_ICONS),
                x: (cell_x + cell - 11) as f32,
                y: cell_y as f32,
                width: 10.0,
                height: 8.0,
                uv: latency_uv(latency_level(record.latency)),
                colour: TAB_TINT,
            });
        }
        if let Some(lines) = &footer {
            let footer_top = grid_top + rows * TAB_ROW_PITCH + 1;
            let height = lines.len() as i32 * TAB_ROW_PITCH;
            draws.push(HudDraw::Rect {
                x: (width / 2 - panel / 2 - 1) as f32,
                y: (footer_top - 1) as f32,
                width: (panel + 2) as f32,
                height: (height + 1) as f32,
                colour: TAB_PANEL,
            });
            // The footer's lines follow their own rect too
            // (`GuiPlayerTabOverlay.java`:226-231).
            for (index, line) in lines.iter().enumerate() {
                draws.push(HudDraw::Text {
                    text: line.clone(),
                    x: (width / 2 - string_width(font, line) / 2) as f32,
                    y: (footer_top + index as i32 * TAB_ROW_PITCH) as f32,
                    scale: 1.0,
                    colour: TAB_TEXT,
                    shadow: true,
                    blend: false,
                });
            }
        }
        draws
    }

    /// One entry's name, the source's `getPlayerName` (`GuiPlayerTabOverlay.getPlayerName`:48-51):
    /// the display name's formatted text when the record carries one, else the team-composed
    /// profile name ([`format_entry`]).
    fn name_of(&self, board: &Scoreboard, record: &PlayerListRecord) -> String {
        match &record.display_name {
            Some(raw) => run_text(&chat::flatten(&chat::parse_json(raw))),
            None => format_entry(board, &record.name, &record.name),
        }
    }
}

/// The team an entry belongs to, registered name only: the sort's second key
/// (`GuiPlayerTabOverlay.java`:396) — the empty string when the entry has no team.
fn team_name<'a>(board: &'a Scoreboard, record: &PlayerListRecord) -> &'a str {
    board
        .member_of
        .get(&record.name)
        .map(String::as_str)
        .unwrap_or("")
}

// ---- the scoreboard: the sidebar and the below-name lines ----

/// The most rows the sidebar draws: the source's own fifteen
/// (`GuiIngame.renderScoreboard:563`).
const SIDEBAR_ROWS: usize = 15;

/// One row's pitch: the font's own height, `FontRenderer.FONT_HEIGHT = 9`
/// (`FontRenderer.java:35`; `GuiIngame.renderScoreboard:581`, `:593`).
const SIDEBAR_ROW_PITCH: i32 = 9;

/// The sidebar's right margin: the source's own `k1 = 3`
/// (`GuiIngame.renderScoreboard:583`).
const SIDEBAR_MARGIN: i32 = 3;

/// The row background: `1342177280` = 0x50000000 — black at alpha 80
/// (`GuiIngame.renderScoreboard:595`), the title separator's own value too (`:603`).
const SIDEBAR_BAND: [f32; 4] = [0.0, 0.0, 0.0, 80.0 / 255.0];

/// The title band's background: `1610612736` = 0x60000000 — black at alpha 96
/// (`GuiIngame.renderScoreboard:602`).
const SIDEBAR_TITLE_BAND: [f32; 4] = [0.0, 0.0, 0.0, 96.0 / 255.0];

/// The name, number and title colour: `553648127` = 0x20FFFFFF — white at
/// alpha 32 (`GuiIngame.renderScoreboard:596-597`, `:604`).
const SIDEBAR_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 32.0 / 255.0];

/// The display name an objective draws with: its value, and the registry name
/// where the wire sent none — the source's own default, the name the
/// constructor holds until a display name arrives (`ScoreObjective.java:18`).
fn objective_display(objective: &Objective) -> &str {
    if objective.value.is_empty() {
        &objective.name
    } else {
        &objective.value
    }
}

/// `String.compareToIgnoreCase` (`Score.java:13`): equal characters pass,
/// otherwise the pair compares through its uppercase forms first and its
/// lowercase forms on a tie — the JDK's own fold order.
fn compare_ignore_case(left: &str, right: &str) -> Ordering {
    let mut left = left.chars();
    let mut right = right.chars();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => {
                let (a, b) = case_pair(a, b);
                if a != b {
                    return a.cmp(&b);
                }
            }
        }
    }
}

/// One character pair's case fold for [`compare_ignore_case`]:
/// `Character.toUpperCase` first, `Character.toLowerCase` where uppercase
/// leaves the pair equal.
fn case_pair(a: char, b: char) -> (char, char) {
    let upper = |c: char| c.to_uppercase().next().unwrap_or(c);
    let (a, b) = (upper(a), upper(b));
    if a != b {
        return (a, b);
    }
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    (lower(a), lower(b))
}

/// The objective the sidebar shows for the window's own account name: the own
/// team's colour slot when one resolves, else slot 1 (`GuiIngame.java:319-336`).
///
/// The team's colour index `i` names slot `3 + i`
/// (`Scoreboard.getObjectiveDisplaySlotNumber:479-486`); the objective the
/// board resolves there wins, and every other outcome — no team, the no-colour
/// sentinel, an empty or unresolvable slot — falls back to slot 1, as the
/// source's null check does. Neither slot resolving names no sidebar.
fn sidebar_objective<'a>(board: &'a Scoreboard, own: &str) -> Option<&'a Objective> {
    let resolved = |slot: usize| {
        board
            .display
            .get(slot)
            .and_then(Option::as_deref)
            .and_then(|name| board.objectives.get(name))
    };
    let from_colour = board
        .team_of(own)
        .and_then(|team| team.colour)
        .and_then(|colour| resolved(3 + usize::from(colour)));
    from_colour.or_else(|| resolved(1))
}

/// The scoreboard sidebar's draws for one frame: the chosen display slot's
/// objective, its entries, and the title band, as one ordered draw list in the
/// source's own painter order (`GuiIngame.renderScoreboard:551-607`).
///
/// `own` is the window's own account name, the name the source reads its team
/// through (`GuiIngame.java:320`); the objective's slot is the team's colour
/// choice ([`sidebar_objective`]). Rows draw bottom first — the draw list runs
/// from the lowest points to the highest — and the title band closes it in the
/// top row's own iteration. Every draw carries no shadow, as the source's
/// four-argument `drawString` does not (`FontRenderer.java:333-336`). The
/// frame seeds its HUD draw list with these draws, ahead of the chat's and
/// the held list's — the source's own painter order.
pub fn sidebar_draws(
    board: &Scoreboard,
    own: &str,
    font: &Font,
    resolution: ScaledResolution,
) -> Vec<HudDraw> {
    let Some(objective) = sidebar_objective(board, own) else {
        return Vec::new();
    };
    let width = resolution.width as i32;
    let height = resolution.height as i32;
    // The collection the source sorts (`Scoreboard.getSortedScores:124-140`):
    // every entry with a score in the objective, points ascending and equal
    // points by name descending case-insensitively (`Score.java`:9-15).
    let mut sorted: Vec<(&str, i32)> = board
        .scores
        .iter()
        .filter_map(|(entry, scores)| {
            scores
                .get(&objective.name)
                .map(|points| (entry.as_str(), *points))
        })
        .collect();
    sorted.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| compare_ignore_case(right.0, left.0))
    });
    // The filter (`GuiIngame.renderScoreboard:555-561`): an entry whose name is
    // null — none can be, a recorded absence — or starts with `#` never draws.
    let filtered: Vec<(&str, i32)> = sorted
        .iter()
        .copied()
        .filter(|(entry, _)| !entry.starts_with('#'))
        .collect();
    // The clamp and its quirk (`:563-570`): when the filtered list outgrows
    // fifteen, the source skips `unfiltered - 15` from the FILTERED list — the
    // skip reads the collection size before the reassignment, so a filter that
    // removed `k` entries draws `15 - k` rows.
    let drawn: Vec<(&str, i32)> = if filtered.len() > SIDEBAR_ROWS {
        filtered
            .iter()
            .copied()
            .skip(sorted.len() - SIDEBAR_ROWS)
            .collect()
    } else {
        filtered
    };
    if drawn.is_empty() {
        return Vec::new();
    }
    let count = drawn.len() as i32;
    // The measured width (`:572-579`): the title, and each drawn row's composed
    // line — the name, `": "`, then the red number — whose separator and number
    // count although the draw splits them off.
    let title = objective_display(objective);
    let mut widest = string_width(font, title);
    for (entry, points) in &drawn {
        let line = format!("{}: §c{}", format_entry(board, entry, entry), points);
        widest = widest.max(string_width(font, &line));
    }
    // The geometry (`:581-585`, `:593-595`): the block is `9n` tall from the
    // `H/2 + 9n/3` baseline, the text column sits `W - i - 3`, each row's right
    // edge `W - 1`, and the row tops step nine pixels down from the baseline —
    // one is the bottom row.
    let block = count * SIDEBAR_ROW_PITCH;
    let baseline = height / 2 + block / 3;
    let left = width - widest - SIDEBAR_MARGIN;
    let right = width - SIDEBAR_MARGIN + 2;
    let mut draws = Vec::new();
    for (index, (entry, points)) in drawn.iter().enumerate() {
        let row = index as i32 + 1;
        let top = baseline - row * SIDEBAR_ROW_PITCH;
        draws.push(HudDraw::Rect {
            x: (left - 2) as f32,
            y: top as f32,
            width: (right - left + 2) as f32,
            height: SIDEBAR_ROW_PITCH as f32,
            colour: SIDEBAR_BAND,
        });
        // The scoreboard's glyph runs draw unblended: the row rects end
        // `disableBlend()` (`Gui.java`:82-83) and `renderScoreboard` never re-enables it
        // (`GuiIngame.java`:551-607).
        draws.push(HudDraw::Text {
            text: format_entry(board, entry, entry),
            x: left as f32,
            y: top as f32,
            scale: 1.0,
            colour: SIDEBAR_TEXT,
            shadow: false,
            blend: false,
        });
        // The number is the red one unconditionally (`:577`, `:592`): no
        // render-kind branch lives in the sidebar — the kind's own branch is
        // the tab list's (`GuiPlayerTabOverlay.drawScoreboardValues:278-362`).
        let number = format!("§c{points}");
        // Unblended like the name: the digit's core draws pure `§c` red.
        draws.push(HudDraw::Text {
            x: (right - string_width(font, &number)) as f32,
            y: top as f32,
            scale: 1.0,
            colour: SIDEBAR_TEXT,
            shadow: false,
            text: number,
            blend: false,
        });
        // The title band draws in the last row's own iteration (`:599-605`):
        // the band above the row, its one-pixel separator, then the title,
        // centred by integer division.
        if index + 1 == drawn.len() {
            draws.push(HudDraw::Rect {
                x: (left - 2) as f32,
                y: (top - SIDEBAR_ROW_PITCH - 1) as f32,
                width: (right - left + 2) as f32,
                height: SIDEBAR_ROW_PITCH as f32,
                colour: SIDEBAR_TITLE_BAND,
            });
            draws.push(HudDraw::Rect {
                x: (left - 2) as f32,
                y: (top - 1) as f32,
                width: (right - left + 2) as f32,
                height: 1.0,
                colour: SIDEBAR_BAND,
            });
            // Unblended, the same run (the title draws in the top row's iteration).
            draws.push(HudDraw::Text {
                text: title.to_owned(),
                x: (left + widest / 2 - string_width(font, title) / 2) as f32,
                y: (top - SIDEBAR_ROW_PITCH) as f32,
                scale: 1.0,
                colour: SIDEBAR_TEXT,
                shadow: false,
                blend: false,
            });
        }
    }
    draws
}

/// The below-name line one frame shows under its nametag, when it shows one:
/// the slot-2 objective's points and display name, `"<points> <display name>"`
/// (`RenderPlayer.renderOffsetLivingLabel:149`).
///
/// The line is a player's alone — the override is `RenderPlayer`'s — and never
/// shows while sneaking: the source's sneaking branch bypasses the override
/// entirely (`RendererLivingEntity.java:507-538`). The slot-2 objective must
/// resolve (`RenderPlayer.java:144`); a missing score is no veto — the source's
/// lookup creates the score at its zero default
/// (`Scoreboard.getValueFromObjective:96-120`) — so a player with no stored
/// score draws `"0 <display name>"`. A frame with no account name resolves
/// none: the score lookup reads the entry name (`RenderPlayer.java:148`), and
/// a player the list never named has none — the same no-record rule the
/// nametag follows. The distance gate (`RenderPlayer.java:141`) and the
/// nametag-visibility chain ride the entity pass, where the nametag's own
/// camera-dependent terms live.
fn below_name_for(frame: &EntityFrame, board: &Scoreboard) -> Option<String> {
    if frame.kind != EntityKind::Player || frame.sneaking {
        return None;
    }
    let name = frame.name.as_deref()?;
    let objective = board.display[2]
        .as_deref()
        .and_then(|name| board.objectives.get(name))?;
    let points = board
        .scores
        .get(name)
        .and_then(|scores| scores.get(&objective.name))
        .copied()
        .unwrap_or(0);
    Some(format!("{points} {}", objective_display(objective)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::time::Duration;

    use oxide_assets::skins::DefaultModel;

    use oxide_game::entity_view::{EntityExtra, EntityFrame, MobExtra, display_name};
    use oxide_proto_v47::entity::MetadataItem;
    use oxide_render::entity_models::PoseExtra;
    use oxide_render::entity_models::player::{PlayerExtra, cape_rotation};
    use oxide_render::entity_pass::{FrameContent, ModelRef, NametagDraw, TextureRef};
    use oxide_render::hud::scaled_resolution;
    use oxide_world::entity::EntityKind;

    use crate::ChatInput;

    /// The uuid whose default model the rule calls wide (its last bit is zero).
    const UUID_WIDE: &str = "00000000-0000-0000-0000-000000000000";
    /// The uuid whose default model the rule calls slim (its last bit is one).
    const UUID_SLIM: &str = "00000000-0000-0000-0000-000000000001";

    /// The tick's 50 ms step, as the fraction's divisor.
    const TICK: Duration = Duration::from_millis(50);

    /// A player frame whose pairs all move, so one interpolation pins them all: the
    /// position slides 0 -> 2 on x, the pitch 0 -> 20, the head yaw 0 -> 10, the render
    /// (body) yaw 0 -> 90, and the limb pair and swing progress move too.
    fn player_frame(id: i32, uuid: &str) -> EntityFrame {
        EntityFrame {
            id,
            kind: EntityKind::Player,
            uuid: Some(uuid.to_owned()),
            name: None,
            equipment: std::array::from_fn(|_| None),
            prev: [0.0, 0.0, 0.0],
            pos: [2.0, 0.0, 0.0],
            prev_yaw: 0.0,
            yaw: 0.0,
            prev_pitch: 0.0,
            pitch: 20.0,
            prev_head_yaw: 0.0,
            head_yaw: 10.0,
            render_yaw_offset: 90.0,
            prev_render_yaw_offset: 0.0,
            on_ground: true,
            invisible: false,
            sneaking: false,
            age: 100,
            limb_swing: 1.0,
            limb_swing_amount: 0.5,
            prev_limb_swing_amount: 0.25,
            swing_progress: 0.5,
            prev_swing_progress: 0.25,
            hurt_ticks: 0,
            death_ticks: 0,
            brightness: 0.65,
            health: None,
            nametag: None,
            extra: EntityExtra::Player,
        }
    }

    /// The join event that names the window's own entity.
    fn joined(entity_id: i32) -> ClientEvent {
        ClientEvent::Joined {
            entity_id,
            gamemode: 0,
            dimension: 0,
            difficulty: 1,
            max_players: 20,
            level_type: "default".to_owned(),
        }
    }

    /// The tick feed event carrying the frames.
    fn ticked(entities: Vec<EntityFrame>) -> ClientEvent {
        ClientEvent::EntitiesTick { entities }
    }

    /// The draws at `elapsed` past the arrival, against no scoreboard.
    fn draws_at(view: &View, arrival: Instant, elapsed: Duration) -> Vec<EntityDraw> {
        view.entity_draws(arrival + elapsed, &BTreeMap::new(), &Scoreboard::new())
    }

    #[test]
    fn the_draws_interpolate_at_the_arrivals_fraction() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(vec![player_frame(8, UUID_SLIM)], t0);
        let draws = draws_at(&view, t0, TICK / 2);
        assert_eq!(draws.len(), 1);
        let draw = &draws[0];
        // The position slides half way in one half tick's fraction, the rotations lerp and
        // the head's net yaw is the head's turn minus the body's.
        assert_eq!(draw.position, [1.0, 0.0, 0.0]);
        assert!((draw.body_yaw - 45.0).abs() < 1.0e-3);
        assert!((draw.head_yaw + 40.0).abs() < 1.0e-3);
        assert!((draw.head_pitch - 10.0).abs() < 1.0e-3);
        // The pose's own terms: `limbSwing - limbSwingAmount * (1 - partial)`, the eased
        // limb amount, the age, and the swing grid's step.
        assert!((draw.pose.limb_swing - 0.75).abs() < 1.0e-4);
        assert!((draw.pose.limb_swing_amount - 0.375).abs() < 1.0e-4);
        assert!((draw.pose.age - 100.5).abs() < 1.0e-4);
        assert!((draw.pose.swing_progress - 0.375).abs() < 1.0e-4);
        // Nothing hurts or dies, the light is the frame's brightness, and no update means
        // the uuid's default model — this uuid's bit is one, so slim.
        assert_eq!(draw.hurt, 0.0);
        assert_eq!(draw.death, 0.0);
        assert_eq!(draw.light, 0.65);
        assert_eq!(
            draw.model,
            ModelRef::Player {
                slim: true,
                parts: 0x7F
            }
        );
        assert_eq!(
            draw.texture,
            TextureRef::Skin {
                uuid: UUID_SLIM.to_owned(),
                slim: true
            }
        );
    }

    #[test]
    fn a_frames_nametag_maps_onto_the_draw_and_an_absent_one_stays_none() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut named = player_frame(8, UUID_SLIM);
        named.nametag = Some(Arc::from("Notch"));
        view.observe(vec![named], t0);
        assert_eq!(
            draws_at(&view, t0, TICK / 2)[0].nametag,
            Some(NametagDraw {
                text: Arc::from("Notch")
            })
        );
        // A frame without a name leaves the draw's field empty.
        let plain = player_frame(8, UUID_SLIM);
        view.observe(vec![plain], t0);
        assert_eq!(draws_at(&view, t0, TICK / 2)[0].nametag, None);
    }

    #[test]
    fn the_jump_over_four_blocks_snaps() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut far = player_frame(8, UUID_WIDE);
        far.pos = [5.0, 0.0, 0.0];
        view.observe(vec![far], t0);
        // A five-block step is a teleport: the draw is the current position, not a blur
        // between the two.
        assert_eq!(draws_at(&view, t0, TICK / 2)[0].position, [5.0, 0.0, 0.0]);
        // Three blocks interpolate.
        let mut near = player_frame(8, UUID_WIDE);
        near.pos = [3.0, 0.0, 0.0];
        view.observe(vec![near], t0);
        assert_eq!(draws_at(&view, t0, TICK / 2)[0].position, [1.5, 0.0, 0.0]);
    }

    #[test]
    fn the_step_at_exactly_four_blocks_still_slides() {
        // The snap starts only past the boundary (spec P5, `docs/specs/oxidecraft-v1-design.md:92`,
        // restated in section 9 at `:315`: "snapping when a teleport exceeds 4 blocks").
        let mut view = View::new();
        let t0 = Instant::now();
        let mut edge = player_frame(8, UUID_WIDE);
        edge.pos = [4.0, 0.0, 0.0];
        view.observe(vec![edge], t0);
        assert_eq!(
            draws_at(&view, t0, TICK / 2)[0].position,
            [2.0, 0.0, 0.0],
            "a step of exactly four blocks interpolates"
        );
        let mut over = player_frame(8, UUID_WIDE);
        over.pos = [4.1, 0.0, 0.0];
        view.observe(vec![over], t0);
        assert_eq!(
            draws_at(&view, t0, TICK / 2)[0].position,
            [4.1, 0.0, 0.0],
            "a step past four blocks snaps"
        );
    }

    #[test]
    fn the_cape_motion_reads_the_frames_displacement() {
        let mut view = View::new();
        let t0 = Instant::now();
        // The pair moves two blocks on x: the draw's cape motion term is the pair's own
        // displacement, and at pose level the wave turns the box off its rest [6, 180, 0].
        view.observe(vec![player_frame(8, UUID_WIDE)], t0);
        let draw = &draws_at(&view, t0, Duration::ZERO)[0];
        assert_eq!(
            draw.pose.extra,
            PoseExtra::Player(PlayerExtra {
                motion: [2.0, 0.0, 0.0],
                held: false,
            }),
            "the cape's motion term is the frame pair's displacement"
        );
        let PoseExtra::Player(cape) = draw.pose.extra else {
            panic!("the player's draw carries the cape motion");
        };
        let moved = cape_rotation(&draw.pose, cape.motion);
        assert_eq!(moved, [6.0, 280.0, -100.0]);
        assert_ne!(moved, cape_rotation(&draw.pose, [0.0; 3]));
        // A still pair carries no motion at all.
        let mut still = player_frame(8, UUID_WIDE);
        still.prev = still.pos;
        view.observe(vec![still], t0);
        assert_eq!(
            draws_at(&view, t0, Duration::ZERO)[0].pose.extra,
            PoseExtra::Player(PlayerExtra {
                motion: [0.0, 0.0, 0.0],
                held: false,
            })
        );
    }

    #[test]
    fn the_own_entity_is_skipped() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.apply(&joined(7));
        view.apply(&ticked(vec![
            player_frame(7, UUID_WIDE),
            player_frame(8, UUID_SLIM),
        ]));
        let draws = view.entity_draws(t0, &BTreeMap::new(), &Scoreboard::new());
        assert_eq!(draws.len(), 1, "the window's own entity draws nothing");
        assert_eq!(
            draws[0].texture,
            TextureRef::Skin {
                uuid: UUID_SLIM.to_owned(),
                slim: true
            }
        );
    }

    #[test]
    fn an_invisible_frame_draws_nothing() {
        let mut view = View::new();
        let t0 = Instant::now();
        // The flag alone: the model and the shadow both hang off the draw, and the
        // source skips both for an invisible entity (`RendererLivingEntity.java:248-249`,
        // `Render.java:303`).
        let mut unseen = player_frame(8, UUID_WIDE);
        unseen.invisible = true;
        view.observe(vec![unseen], t0);
        assert_eq!(
            draws_at(&view, t0, Duration::ZERO).len(),
            0,
            "an invisible entity draws nothing"
        );
        view.observe(vec![player_frame(8, UUID_WIDE)], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO).len(), 1);
    }

    #[test]
    fn the_texture_falls_back_by_uuid() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![player_frame(8, UUID_WIDE), player_frame(9, UUID_SLIM)],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(
            draws[0].texture,
            TextureRef::Skin {
                uuid: UUID_WIDE.to_owned(),
                slim: false
            }
        );
        assert_eq!(
            draws[1].texture,
            TextureRef::Skin {
                uuid: UUID_SLIM.to_owned(),
                slim: true
            }
        );
        assert_eq!(
            draws[0].model,
            ModelRef::Player {
                slim: false,
                parts: 0x7F
            }
        );
        // A stored update overrides the rule as it stands, as-is.
        let mut skins = BTreeMap::new();
        skins.insert(
            UUID_WIDE.to_owned(),
            SkinUpdate {
                uuid: UUID_WIDE.to_owned(),
                texture: None,
                cape: None,
                model: DefaultModel::Slim,
            },
        );
        let draw = &view.entity_draws(t0, &skins, &Scoreboard::new())[0];
        assert_eq!(
            draw.model,
            ModelRef::Player {
                slim: true,
                parts: 0x7F
            }
        );
        assert_eq!(
            draw.texture,
            TextureRef::Skin {
                uuid: UUID_WIDE.to_owned(),
                slim: true
            }
        );
    }

    #[test]
    fn the_light_reads_the_frames_brightness() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut frame = player_frame(8, UUID_WIDE);
        frame.brightness = 0.2;
        view.observe(vec![frame], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO)[0].light, 0.2);
        let mut frame = player_frame(8, UUID_WIDE);
        frame.brightness = 1.0;
        view.observe(vec![frame], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO)[0].light, 1.0);
    }

    #[test]
    fn the_death_ramp_pins_three_points() {
        let mut view = View::new();
        let t0 = Instant::now();
        // One death tick, half a tick in: sqrt(((1 + 0.5 - 1) / 20) * 1.6) = 0.2.
        let mut frame = player_frame(8, UUID_WIDE);
        frame.death_ticks = 1;
        view.observe(vec![frame], t0);
        let draw = &draws_at(&view, t0, TICK / 2)[0];
        assert!((draw.death - 0.2).abs() < 1.0e-4, "got {}", draw.death);
        // The death's own gate: the damage overlay opens with it, as the source's flag
        // does (`hurtTime > 0 || deathTime > 0`).
        assert_eq!(draw.hurt, 1.0);
        // Thirteen ticks, on the tick: sqrt((12 / 20) * 1.6) = 0.9797959.
        let mut frame = player_frame(8, UUID_WIDE);
        frame.death_ticks = 13;
        view.observe(vec![frame], t0);
        let draw = &draws_at(&view, t0, Duration::ZERO)[0];
        assert!(
            (draw.death - 0.979_795_9).abs() < 1.0e-4,
            "got {}",
            draw.death
        );
        // Well past the ramp's end the fraction clamps at one.
        let mut frame = player_frame(8, UUID_WIDE);
        frame.death_ticks = 40;
        view.observe(vec![frame], t0);
        assert_eq!(draws_at(&view, t0, Duration::ZERO)[0].death, 1.0);
    }

    /// A mob frame: the player template's pairs with the kind's own channel.
    fn mob_frame(id: i32, kind: EntityKind, extra: EntityExtra) -> EntityFrame {
        let mut frame = player_frame(id, UUID_WIDE);
        frame.uuid = None;
        frame.kind = kind;
        frame.extra = extra;
        frame
    }

    #[test]
    fn the_crawler_families_map_to_their_models_sheets_and_extras() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(
                    1,
                    EntityKind::Creeper,
                    EntityExtra::Mob(MobExtra::Creeper { powered: false }),
                ),
                mob_frame(2, EntityKind::Spider, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(3, EntityKind::CaveSpider, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    4,
                    EntityKind::Enderman,
                    EntityExtra::Mob(MobExtra::Enderman),
                ),
                mob_frame(5, EntityKind::Chicken, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(6, EntityKind::Squid, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    7,
                    EntityKind::Slime,
                    EntityExtra::Mob(MobExtra::Slime { size: 3 }),
                ),
                mob_frame(
                    8,
                    EntityKind::LavaSlime,
                    EntityExtra::Mob(MobExtra::Slime { size: 2 }),
                ),
                mob_frame(
                    9,
                    EntityKind::Bat,
                    EntityExtra::Mob(MobExtra::Bat { hanging: true }),
                ),
                mob_frame(
                    10,
                    EntityKind::Silverfish,
                    EntityExtra::Mob(MobExtra::Other),
                ),
                mob_frame(11, EntityKind::Endermite, EntityExtra::Mob(MobExtra::Other)),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 11, "every mapped mob draws");
        let expected = [
            (ModelRef::Creeper, "entity/creeper/creeper.png"),
            (ModelRef::Spider, "entity/spider/spider.png"),
            (ModelRef::CaveSpider, "entity/spider/cave_spider.png"),
            (ModelRef::Enderman, "entity/enderman/enderman.png"),
            (ModelRef::Chicken { child: false }, "entity/chicken.png"),
            (ModelRef::Squid, "entity/squid.png"),
            (ModelRef::Slime { size: 3 }, "entity/slime/slime.png"),
            (
                ModelRef::MagmaCube { size: 2 },
                "entity/slime/magmacube.png",
            ),
            (ModelRef::Bat { hanging: true }, "entity/bat.png"),
            (ModelRef::Silverfish, "entity/silverfish.png"),
            (ModelRef::EnderMite, "entity/endermite.png"),
        ];
        for (draw, (model, sheet)) in draws.iter().zip(expected) {
            assert_eq!(draw.model, model);
            assert_eq!(draw.texture, TextureRef::Named(sheet));
        }
        // The pinned extras ride the draws: the creeper's marker, the bat's hang on
        // both terms, the cubes' sizes with their squash pairs at rest, and the
        // client-side-only states held off.
        assert_eq!(draws[0].extra, DrawExtra::Creeper { powered: false });
        assert_eq!(
            draws[3].pose.extra,
            PoseExtra::Enderman { attacking: false }
        );
        assert_eq!(draws[4].pose.extra, PoseExtra::Chicken { flap: 0.0 });
        assert_eq!(
            draws[5].pose.extra,
            PoseExtra::Squid {
                tentacle_angle: 0.0
            }
        );
        assert_eq!(
            draws[6].extra,
            DrawExtra::Slime {
                size: 3,
                squish: 0.0
            }
        );
        assert_eq!(draws[7].pose.extra, PoseExtra::MagmaCube { squish: 0.0 });
        assert_eq!(draws[8].extra, DrawExtra::Bat { hanging: true });
        assert_eq!(draws[8].pose.extra, PoseExtra::Bat { hanging: true });
    }

    #[test]
    fn every_new_kind_maps_to_its_model_sheet_and_extras() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(
                    1,
                    EntityKind::Zombie,
                    EntityExtra::Mob(MobExtra::Zombie { villager: false }),
                ),
                mob_frame(
                    2,
                    EntityKind::Zombie,
                    EntityExtra::Mob(MobExtra::Zombie { villager: true }),
                ),
                mob_frame(3, EntityKind::Skeleton, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    4,
                    EntityKind::Villager,
                    EntityExtra::Mob(MobExtra::Villager {
                        profession: 4,
                        child: false,
                    }),
                ),
                mob_frame(5, EntityKind::Witch, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(6, EntityKind::Giant, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(7, EntityKind::SnowMan, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    8,
                    EntityKind::VillagerGolem,
                    EntityExtra::Mob(MobExtra::Other),
                ),
                mob_frame(
                    9,
                    EntityKind::Pig,
                    EntityExtra::Mob(MobExtra::Pig { saddle: true }),
                ),
                mob_frame(10, EntityKind::Cow, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    11,
                    EntityKind::Sheep,
                    EntityExtra::Mob(MobExtra::Sheep {
                        wool: 9,
                        sheared: false,
                    }),
                ),
                mob_frame(
                    12,
                    EntityKind::MushroomCow,
                    EntityExtra::Mob(MobExtra::Other),
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 12, "every mapped mob draws");
        let expected = [
            (ModelRef::Zombie, "entity/zombie/zombie.png"),
            (
                ModelRef::ZombieVillager,
                "entity/zombie/zombie_villager.png",
            ),
            (ModelRef::Skeleton, "entity/skeleton/skeleton.png"),
            (
                ModelRef::Villager {
                    profession: 4,
                    child: false,
                },
                "entity/villager/butcher.png",
            ),
            (ModelRef::Witch, "entity/witch.png"),
            (ModelRef::Giant, "entity/zombie/zombie.png"),
            (ModelRef::SnowGolem, "entity/snowman.png"),
            (ModelRef::IronGolem, "entity/iron_golem.png"),
            (ModelRef::Pig { saddle: true }, "entity/pig/pig.png"),
            (ModelRef::Cow, "entity/cow/cow.png"),
            (
                ModelRef::Sheep {
                    wool: 9,
                    sheared: false,
                },
                "entity/sheep/sheep.png",
            ),
            (ModelRef::Mooshroom, "entity/cow/mooshroom.png"),
        ];
        for (draw, (model, sheet)) in draws.iter().zip(expected) {
            assert_eq!(draw.model, model);
            assert_eq!(draw.texture, TextureRef::Named(sheet));
        }
        // The extras: the zombie villager's flag, the villager's profession pair, the
        // pig's saddle and the sheep's wool; the pose carries the skeleton's aim state
        // and the witch's hold gate seeded by the entity's own id
        // (`ModelWitch.setRotationAngles`:51-60).
        assert_eq!(draws[1].extra, DrawExtra::ZombieVillager);
        assert_eq!(
            draws[3].extra,
            DrawExtra::Villager {
                profession: 4,
                child: false
            }
        );
        assert_eq!(draws[8].extra, DrawExtra::Pig { saddle: true });
        assert_eq!(
            draws[10].extra,
            DrawExtra::Sheep {
                wool: 9,
                sheared: false
            }
        );
        assert_eq!(
            draws[2].pose.extra,
            PoseExtra::Skeleton { aimed_bow: false }
        );
        assert_eq!(
            draws[4].pose.extra,
            PoseExtra::Witch {
                holding: false,
                entity_id: 5
            }
        );
        // Every draw names a zone the window and the pass share; the non-villager mobs
        // carry no child term.
        assert!(draws.iter().all(|draw| !draw.pose.child));
    }

    /// The exotic families' sheets and extras, per their renderers: the horse's colour,
    /// type and marking/armour tables with its saddle and chest terms
    /// (`RenderHorse.getEntityTexture`:51-78), the wolf's tamed/angry sheet pair with the
    /// collar byte (`RenderWolf.getEntityTexture`:46-49), the ocelot's and the rabbit's
    /// variant tables (`RenderOcelot.getEntityTexture`:23-40,
    /// `RenderRabbit.getEntityTexture`:27-62), the ghast's shooting sheet
    /// (`RenderGhast.getEntityTexture`:21-24), the guardian's elder sheet
    /// (`RenderGuardian.getEntityTexture`:177-180), the dragon, and the wither's
    /// spawn-shield flicker (`RenderWither.getEntityTexture`:33-37).
    #[test]
    fn the_exotic_families_map_to_their_models_sheets_and_extras() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(
                    1,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 1,
                        markings: 2,
                        tamed: true,
                        saddle: true,
                        adult: true,
                        chested: true,
                        armour: 3,
                    }),
                ),
                mob_frame(
                    2,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    3,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 4,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    4,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 1,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    5,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 2,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    6,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 3,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    7,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 4,
                        colour: 0,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: true,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    8,
                    EntityKind::EntityHorse,
                    EntityExtra::Mob(MobExtra::Horse {
                        variant: 0,
                        colour: 3,
                        markings: 0,
                        tamed: false,
                        saddle: false,
                        adult: false,
                        chested: false,
                        armour: 0,
                    }),
                ),
                mob_frame(
                    9,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: true,
                        collar: 14,
                        angry: false,
                        sitting: false,
                        health: 20.0,
                    }),
                ),
                mob_frame(
                    10,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: false,
                        collar: 14,
                        angry: true,
                        sitting: false,
                        health: 20.0,
                    }),
                ),
                mob_frame(
                    11,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: false,
                        collar: 14,
                        angry: false,
                        sitting: false,
                        health: 20.0,
                    }),
                ),
                mob_frame(
                    12,
                    EntityKind::Wolf,
                    EntityExtra::Mob(MobExtra::Wolf {
                        tamed: true,
                        collar: 3,
                        angry: false,
                        sitting: true,
                        health: 8.0,
                    }),
                ),
                mob_frame(
                    13,
                    EntityKind::Ozelot,
                    EntityExtra::Mob(MobExtra::Ocelot {
                        variant: 0,
                        tamed: false,
                        sitting: false,
                    }),
                ),
                mob_frame(
                    14,
                    EntityKind::Ozelot,
                    EntityExtra::Mob(MobExtra::Ocelot {
                        variant: 2,
                        tamed: true,
                        sitting: false,
                    }),
                ),
                mob_frame(
                    15,
                    EntityKind::Ozelot,
                    EntityExtra::Mob(MobExtra::Ocelot {
                        variant: 3,
                        tamed: true,
                        sitting: true,
                    }),
                ),
                mob_frame(
                    16,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 0,
                        child: false,
                    }),
                ),
                mob_frame(
                    17,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 3,
                        child: false,
                    }),
                ),
                mob_frame(
                    18,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 99,
                        child: false,
                    }),
                ),
                mob_frame(
                    19,
                    EntityKind::Rabbit,
                    EntityExtra::Mob(MobExtra::Rabbit {
                        variant: 1,
                        child: true,
                    }),
                ),
                mob_frame(
                    20,
                    EntityKind::Ghast,
                    EntityExtra::Mob(MobExtra::Ghast { shooting: false }),
                ),
                mob_frame(
                    21,
                    EntityKind::Ghast,
                    EntityExtra::Mob(MobExtra::Ghast { shooting: true }),
                ),
                mob_frame(22, EntityKind::Blaze, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(
                    23,
                    EntityKind::Guardian,
                    EntityExtra::Mob(MobExtra::Guardian { elder: false }),
                ),
                mob_frame(
                    24,
                    EntityKind::Guardian,
                    EntityExtra::Mob(MobExtra::Guardian { elder: true }),
                ),
                mob_frame(
                    25,
                    EntityKind::EnderDragon,
                    EntityExtra::Mob(MobExtra::Other),
                ),
                mob_frame(
                    26,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither {
                        invul_time: 0,
                        armored: false,
                    }),
                ),
                mob_frame(
                    27,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither {
                        invul_time: 100,
                        armored: false,
                    }),
                ),
                mob_frame(
                    28,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither {
                        invul_time: 5,
                        armored: false,
                    }),
                ),
                mob_frame(
                    29,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither {
                        invul_time: 10,
                        armored: false,
                    }),
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 29, "every exotic draws");
        let expected = [
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 1,
                    markings: 2,
                    saddle: true,
                    armour: 3,
                },
                "entity/horse/horse_creamy.png",
            ),
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_white.png",
            ),
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 4,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_black.png",
            ),
            (
                ModelRef::Horse {
                    variant: 1,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/donkey.png",
            ),
            (
                ModelRef::Horse {
                    variant: 2,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/mule.png",
            ),
            (
                ModelRef::Horse {
                    variant: 3,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_zombie.png",
            ),
            (
                ModelRef::Horse {
                    variant: 4,
                    colour: 0,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_skeleton.png",
            ),
            (
                ModelRef::Horse {
                    variant: 0,
                    colour: 3,
                    markings: 0,
                    saddle: false,
                    armour: 0,
                },
                "entity/horse/horse_brown.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: true,
                    collar: 14,
                    angry: false,
                },
                "entity/wolf/wolf_tame.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: false,
                    collar: 14,
                    angry: true,
                },
                "entity/wolf/wolf_angry.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: false,
                    collar: 14,
                    angry: false,
                },
                "entity/wolf/wolf.png",
            ),
            (
                ModelRef::Wolf {
                    tamed: true,
                    collar: 3,
                    angry: false,
                },
                "entity/wolf/wolf_tame.png",
            ),
            (
                ModelRef::Ocelot {
                    variant: 0,
                    child: false,
                    tamed: false,
                },
                "entity/cat/ocelot.png",
            ),
            (
                ModelRef::Ocelot {
                    variant: 2,
                    child: false,
                    tamed: true,
                },
                "entity/cat/red.png",
            ),
            (
                ModelRef::Ocelot {
                    variant: 3,
                    child: false,
                    tamed: true,
                },
                "entity/cat/siamese.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 0,
                    child: false,
                },
                "entity/rabbit/brown.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 3,
                    child: false,
                },
                "entity/rabbit/white_splotched.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 99,
                    child: false,
                },
                "entity/rabbit/caerbannog.png",
            ),
            (
                ModelRef::Rabbit {
                    variant: 1,
                    child: true,
                },
                "entity/rabbit/white.png",
            ),
            (
                ModelRef::Ghast { shooting: false },
                "entity/ghast/ghast.png",
            ),
            (
                ModelRef::Ghast { shooting: true },
                "entity/ghast/ghast_shooting.png",
            ),
            (ModelRef::Blaze, "entity/blaze.png"),
            (ModelRef::Guardian { elder: false }, "entity/guardian.png"),
            (
                ModelRef::Guardian { elder: true },
                "entity/guardian_elder.png",
            ),
            (ModelRef::EnderDragon, "entity/enderdragon/dragon.png"),
            (
                ModelRef::Wither { invul_time: 0 },
                "entity/wither/wither.png",
            ),
            (
                ModelRef::Wither { invul_time: 100 },
                "entity/wither/wither_invulnerable.png",
            ),
            (
                ModelRef::Wither { invul_time: 5 },
                "entity/wither/wither.png",
            ),
            (
                ModelRef::Wither { invul_time: 10 },
                "entity/wither/wither_invulnerable.png",
            ),
        ];
        for (draw, (model, sheet)) in draws.iter().zip(expected) {
            assert_eq!(draw.model, model);
            assert_eq!(draw.texture, TextureRef::Named(sheet));
        }
        // The extras: the horse's marking and armour terms, the wolf's collar byte, and
        // the two pose terms the arms pin at rest (the ghast's sway, the blaze's spin,
        // the guardian's spikes, the dragon's flight clock, the rabbit's hop).
        assert_eq!(
            draws[0].extra,
            DrawExtra::Horse {
                markings: 2,
                armour: 3
            }
        );
        assert_eq!(
            draws[0].pose.extra,
            PoseExtra::Horse {
                saddle: true,
                chested: true,
                adult: true,
                variant: 0
            }
        );
        assert_eq!(
            draws[1].extra,
            DrawExtra::Horse {
                markings: 0,
                armour: 0
            }
        );
        assert!(draws[7].pose.child, "the horse's growing age folds it");
        assert_eq!(
            draws[8].extra,
            DrawExtra::Wolf {
                tamed: true,
                collar: 14
            }
        );
        assert_eq!(
            draws[8].pose.extra,
            PoseExtra::Wolf {
                tamed: true,
                angry: false,
                sitting: false,
                health: 20.0
            }
        );
        assert_eq!(
            draws[9].pose.extra,
            PoseExtra::Wolf {
                tamed: false,
                angry: true,
                sitting: false,
                health: 20.0
            }
        );
        assert_eq!(
            draws[11].pose.extra,
            PoseExtra::Wolf {
                tamed: true,
                angry: false,
                sitting: true,
                health: 8.0
            }
        );
        assert_eq!(draws[12].pose.extra, PoseExtra::Ocelot { sitting: false });
        assert_eq!(draws[14].pose.extra, PoseExtra::Ocelot { sitting: true });
        assert_eq!(draws[15].pose.extra, PoseExtra::Rabbit { hop: 0.0 });
        assert!(draws[18].pose.child, "the rabbit's growing age folds it");
        assert_eq!(
            draws[22].pose.extra,
            PoseExtra::Guardian {
                spikes: 1.0,
                tail_phase: 0.0
            }
        );
        assert_eq!(draws[24].pose.extra, PoseExtra::Dragon { anim_time: 20.0 });
    }

    /// The dragon's wing clock (`PoseExtra::Dragon`): the frames carry the age, and the
    /// view advances the clock at the source's at-rest rate, `0.2` a tick
    /// (`EntityDragon.onLivingUpdate`:158-167).
    #[test]
    fn the_dragon_wing_clock_advances_with_the_frames_age() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut frame = mob_frame(
            1,
            EntityKind::EnderDragon,
            EntityExtra::Mob(MobExtra::Other),
        );
        frame.age = 40;
        view.observe(vec![frame], t0);
        let anim_time = |draws: Vec<EntityDraw>| match draws[0].pose.extra {
            PoseExtra::Dragon { anim_time } => anim_time,
            _ => panic!("the dragon's draw carries its flight clock"),
        };
        // Forty ticks at the at-rest rate: eight waves in.
        let anim = anim_time(draws_at(&view, t0, Duration::ZERO));
        assert!(
            (anim - 8.0).abs() < 1.0e-4,
            "the clock at {anim} against 8.0"
        );
        // The partial tick walks the clock on with the frame's fraction.
        let anim = anim_time(draws_at(&view, t0, TICK / 4));
        assert!(
            (anim - 8.05).abs() < 1.0e-4,
            "the clock at {anim} against 8.05"
        );
    }

    /// The roster's own gate: every mob §6.3's spawn-mob table names has a draw — no
    /// kind falls through to the debug-log arm.
    ///
    /// The list is the table's own roster transcribed — creeper `50` through guardian
    /// `68`, pig `90` through rabbit `101`, villager `120` — each member paired with
    /// the extras its metadata extracts to (the session's own per-kind results). A
    /// member the wire can spawn but the mapping cannot draw fails here.
    #[test]
    fn every_roster_mob_maps_to_a_draw() {
        let cases: &[(EntityKind, MobExtra)] = &[
            // 50..=68.
            (EntityKind::Creeper, MobExtra::Creeper { powered: false }),
            (EntityKind::Skeleton, MobExtra::Other),
            (EntityKind::Spider, MobExtra::Other),
            (EntityKind::Giant, MobExtra::Other),
            (EntityKind::Zombie, MobExtra::Zombie { villager: false }),
            (EntityKind::Slime, MobExtra::Slime { size: 1 }),
            (EntityKind::Ghast, MobExtra::Ghast { shooting: false }),
            (EntityKind::PigZombie, MobExtra::Other),
            (EntityKind::Enderman, MobExtra::Enderman),
            (EntityKind::CaveSpider, MobExtra::Other),
            (EntityKind::Silverfish, MobExtra::Other),
            (EntityKind::Blaze, MobExtra::Other),
            (EntityKind::LavaSlime, MobExtra::Slime { size: 1 }),
            (EntityKind::EnderDragon, MobExtra::Other),
            (
                EntityKind::WitherBoss,
                MobExtra::Wither {
                    invul_time: 0,
                    armored: false,
                },
            ),
            (EntityKind::Bat, MobExtra::Bat { hanging: false }),
            (EntityKind::Witch, MobExtra::Other),
            (EntityKind::Endermite, MobExtra::Other),
            (EntityKind::Guardian, MobExtra::Guardian { elder: false }),
            // 90..=101.
            (EntityKind::Pig, MobExtra::Pig { saddle: false }),
            (
                EntityKind::Sheep,
                MobExtra::Sheep {
                    wool: 0,
                    sheared: false,
                },
            ),
            (EntityKind::Cow, MobExtra::Other),
            (EntityKind::Chicken, MobExtra::Other),
            (EntityKind::Squid, MobExtra::Other),
            (
                EntityKind::Wolf,
                MobExtra::Wolf {
                    tamed: false,
                    collar: 14,
                    angry: false,
                    sitting: false,
                    health: 20.0,
                },
            ),
            (EntityKind::MushroomCow, MobExtra::Other),
            (EntityKind::SnowMan, MobExtra::Other),
            (
                EntityKind::Ozelot,
                MobExtra::Ocelot {
                    variant: 0,
                    tamed: false,
                    sitting: false,
                },
            ),
            (EntityKind::VillagerGolem, MobExtra::Other),
            (
                EntityKind::EntityHorse,
                MobExtra::Horse {
                    variant: 0,
                    colour: 0,
                    markings: 0,
                    tamed: false,
                    saddle: false,
                    adult: true,
                    chested: false,
                    armour: 0,
                },
            ),
            (
                EntityKind::Rabbit,
                MobExtra::Rabbit {
                    variant: 0,
                    child: false,
                },
            ),
            // 120.
            (
                EntityKind::Villager,
                MobExtra::Villager {
                    profession: 0,
                    child: false,
                },
            ),
        ];
        assert_eq!(cases.len(), 32, "§6.3's spawn-mob roster is 32 ids");
        for (index, (kind, mob)) in cases.iter().enumerate() {
            assert!(
                mob_draw(*kind, index as i32 + 1, mob).is_some(),
                "{kind:?} has no model mapping — it would fall through to the debug-log arm"
            );
        }
    }

    #[test]
    fn a_child_villager_runs_its_limbs_threefold_and_carries_the_child_term() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![mob_frame(
                4,
                EntityKind::Villager,
                EntityExtra::Mob(MobExtra::Villager {
                    profession: 0,
                    child: true,
                }),
            )],
            t0,
        );
        let draw = &draws_at(&view, t0, TICK / 2)[0];
        assert_eq!(
            draw.model,
            ModelRef::Villager {
                profession: 0,
                child: true
            }
        );
        assert_eq!(
            draw.texture,
            TextureRef::Named("entity/villager/farmer.png")
        );
        assert!(draw.pose.child);
        // `RendererLivingEntity.doRender`:140-143: a child's limb swing runs threefold
        // before the pose reads it — (1 - 0.5 * (1 - 0.5)) * 3 = 2.25.
        assert!(
            (draw.pose.limb_swing - 2.25).abs() < 1.0e-4,
            "the child's limb swing: {}",
            draw.pose.limb_swing
        );
    }

    #[test]
    fn an_unmapped_kind_draws_nothing() {
        let mut view = View::new();
        let t0 = Instant::now();
        // A wire kind the session could not name, and a mapped kind whose channel is not
        // a mob's: both draw nothing.
        view.observe(
            vec![
                mob_frame(1, EntityKind::Unknown, EntityExtra::Mob(MobExtra::Other)),
                mob_frame(2, EntityKind::Pig, EntityExtra::None),
            ],
            t0,
        );
        assert_eq!(draws_at(&view, t0, Duration::ZERO).len(), 0);
    }

    /// An object frame: the player template's pairs with the object kind and its own
    /// channel.
    fn object_frame(id: i32, kind: EntityKind, extra: EntityExtra) -> EntityFrame {
        let mut frame = player_frame(id, UUID_WIDE);
        frame.uuid = None;
        frame.kind = kind;
        frame.extra = extra;
        frame
    }

    /// The item entities: the wire table's own split — a block id draws the baked
    /// model, a sprite id the generated shape, and an id no entry names nothing at all
    /// (`EntityItem`'s stack, resolved through the renderer's own model table).
    #[test]
    fn the_item_entities_resolve_through_the_wire_table() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                object_frame(
                    1,
                    EntityKind::Item,
                    EntityExtra::Item {
                        id: 5,
                        count: 3,
                        damage: 9,
                    },
                ),
                object_frame(
                    2,
                    EntityKind::Item,
                    EntityExtra::Item {
                        id: 280,
                        count: 2,
                        damage: 0,
                    },
                ),
                object_frame(
                    3,
                    EntityKind::Item,
                    EntityExtra::Item {
                        id: 1000,
                        count: 1,
                        damage: 0,
                    },
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 2, "the id with no model draws nothing");
        assert_eq!(draws[0].model, ModelRef::BlockItem { block: 5 });
        assert_eq!(
            draws[0].extra,
            DrawExtra::Item {
                id: 5,
                count: 3,
                damage: 9,
            }
        );
        assert_eq!(draws[1].model, ModelRef::Sprite { key: "items/stick" });
        assert_eq!(
            draws[1].extra,
            DrawExtra::Item {
                id: 280,
                count: 2,
                damage: 0,
            }
        );
    }

    /// Every projectile kind maps to its class's billboard: the arrow's own geometry,
    /// the snowball family's generated shape under `RenderSnowball`'s transform, and
    /// the fireball family's icon quad under `RenderFireball`'s own scale — the
    /// ghast's `2.0`, the blaze's small `0.5` and the wither skull's `1.0`
    /// (`RenderManager.java`:177-186).
    #[test]
    fn every_projectile_kind_maps_to_its_billboard() {
        use oxide_render::entity_models::objects::Billboard;

        let mut view = View::new();
        let t0 = Instant::now();
        let kinds = [
            EntityKind::Arrow,
            EntityKind::Snowball,
            EntityKind::Egg,
            EntityKind::EnderPearl,
            EntityKind::EyeOfEnder,
            EntityKind::Potion,
            EntityKind::XpBottle,
            EntityKind::Firework,
            EntityKind::Fireball,
            EntityKind::SmallFireball,
            EntityKind::WitherSkull,
        ];
        view.observe(
            kinds
                .iter()
                .enumerate()
                .map(|(index, kind)| object_frame(index as i32 + 1, *kind, EntityExtra::Projectile))
                .collect(),
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), kinds.len(), "every projectile kind draws");
        let thrown = || DrawExtra::Projectile {
            billboard: Billboard::Snowball,
            scale: 0.5,
        };
        let fireball = |scale: f32| DrawExtra::Projectile {
            billboard: Billboard::Fireball,
            scale,
        };
        let expected: [(ModelRef, TextureRef, DrawExtra); 11] = [
            (
                ModelRef::Arrow,
                TextureRef::Named("entity/arrow.png"),
                DrawExtra::None,
            ),
            (
                ModelRef::Sprite {
                    key: "items/snowball",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite { key: "items/egg" },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/ender_pearl",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/ender_eye",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/potion_bottle_drinkable",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/experience_bottle",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireworks",
                },
                TextureRef::Named("misc/shadow.png"),
                thrown(),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named("misc/shadow.png"),
                fireball(2.0),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named("misc/shadow.png"),
                fireball(0.5),
            ),
            (
                ModelRef::Sprite {
                    key: "items/fireball",
                },
                TextureRef::Named("misc/shadow.png"),
                fireball(1.0),
            ),
        ];
        for (index, (draw, (model, texture, extra))) in
            draws.iter().zip(expected.iter()).enumerate()
        {
            assert_eq!(&draw.model, model, "kind {index}'s model");
            assert_eq!(&draw.texture, texture, "kind {index}'s sheet");
            assert_eq!(&draw.extra, extra, "kind {index}'s billboard");
        }
    }

    /// A painting's draw: the art's own table index and the hanging's facing byte
    /// (`EntityPainting`'s spawn, folded by `EntityHanging`'s yaw rule); an unknown
    /// title falls back to `Kebab`, the table's first art.
    #[test]
    fn a_painting_maps_its_art_and_facing() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            (0..4u8)
                .map(|facing| {
                    object_frame(
                        i32::from(facing) + 1,
                        EntityKind::Painting,
                        EntityExtra::Painting {
                            title: Arc::from("Wither"),
                            facing,
                        },
                    )
                })
                .collect(),
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 4);
        for (facing, draw) in draws.iter().enumerate() {
            assert_eq!(draw.model, ModelRef::Painting { art: 19 });
            assert_eq!(draw.texture, TextureRef::Named(objects::PAINTING_TEXTURE));
            assert_eq!(
                draw.extra,
                DrawExtra::Painting {
                    facing: facing as u8
                }
            );
        }
        // The unknown title falls back to the first art.
        view.observe(
            vec![object_frame(
                9,
                EntityKind::Painting,
                EntityExtra::Painting {
                    title: Arc::from("no such art"),
                    facing: 0,
                },
            )],
            t0,
        );
        assert_eq!(
            draws_at(&view, t0, Duration::ZERO)[0].model,
            ModelRef::Painting { art: 0 }
        );
    }

    /// The item frame's content resolution: the empty frame, a block stack's small
    /// block, an item stack's generated shape, an id with no model's empty frame, and
    /// the rotation slot riding the draw.
    #[test]
    fn the_frame_maps_its_content_and_rotation() {
        let mut view = View::new();
        let t0 = Instant::now();
        let frame = |id: i32, item: Option<MetadataItem>, rotation: u8| {
            object_frame(
                id,
                EntityKind::ItemFrame,
                EntityExtra::ItemFrame { item, rotation },
            )
        };
        view.observe(
            vec![
                frame(1, None, 0),
                frame(
                    2,
                    Some(MetadataItem {
                        id: 5,
                        count: 1,
                        damage: 0,
                        nbt: None,
                    }),
                    3,
                ),
                frame(
                    3,
                    Some(MetadataItem {
                        id: 280,
                        count: 1,
                        damage: 0,
                        nbt: None,
                    }),
                    7,
                ),
                frame(
                    4,
                    Some(MetadataItem {
                        id: 1000,
                        count: 1,
                        damage: 0,
                        nbt: None,
                    }),
                    1,
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 4);
        assert_eq!(
            draws[0].model,
            ModelRef::ItemFrame {
                content: FrameContent::Empty
            }
        );
        assert_eq!(
            draws[1].model,
            ModelRef::ItemFrame {
                content: FrameContent::Block(5)
            }
        );
        assert_eq!(
            draws[2].model,
            ModelRef::ItemFrame {
                content: FrameContent::Sprite("items/stick")
            }
        );
        assert_eq!(
            draws[3].model,
            ModelRef::ItemFrame {
                content: FrameContent::Empty
            }
        );
        assert_eq!(draws[1].extra, DrawExtra::Frame { rotation: 3 });
        assert_eq!(draws[2].extra, DrawExtra::Frame { rotation: 7 });
    }

    /// The vehicles and the orb map to their own models and sheets.
    ///
    /// The wire's cart sub-types are not carried by the game's frames yet — the
    /// session folds every cart kind to one (`oxide-game`'s spawn mapping) — so every
    /// cart draws the plain body here; the cargo table itself is pinned in `objects.rs`
    /// and the pass's own case.
    #[test]
    fn the_vehicles_and_orb_map_to_their_models_and_sheets() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                object_frame(1, EntityKind::Boat, EntityExtra::Boat),
                object_frame(2, EntityKind::Minecart, EntityExtra::Minecart),
                object_frame(3, EntityKind::XpOrb, EntityExtra::Orb),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws.len(), 3);
        assert_eq!(draws[0].model, ModelRef::Boat);
        assert_eq!(draws[0].texture, TextureRef::Named(objects::BOAT_TEXTURE));
        assert_eq!(draws[1].model, ModelRef::Minecart { body: 0 });
        assert_eq!(
            draws[1].texture,
            TextureRef::Named(objects::MINECART_TEXTURE)
        );
        assert_eq!(draws[2].model, ModelRef::Orb { value: 1 });
        assert_eq!(draws[2].texture, TextureRef::Named(objects::ORB_TEXTURE));
    }

    // ---- the chat mirror ----

    /// A 128x128 synthetic sheet whose `'A'` cell is inked in columns 0..=4: the same
    /// metric the game's chat suite and the render-side text tests measure with — `'A'`
    /// advances six font pixels, the space the source's own four, every other blank
    /// cell one.
    fn chat_font() -> Font {
        const SIDE: u32 = 128;
        const CELL: u32 = 8;
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        let code = 'A' as u32;
        let cell_x = (code % 16) * CELL;
        let cell_y = (code / 16) * CELL;
        for row in 0..CELL {
            for column in 0..=4 {
                let offset = (((cell_y + row) * SIDE + cell_x + column) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        Font::load(
            &oxide_assets::texture::Texture {
                width: SIDE,
                height: SIDE,
                rgba,
            },
            None,
        )
        .expect("the synthetic sheet loads")
    }

    /// The scaled resolution the draws assemble against: the 1280x720 window at the
    /// settings default, 427x240 (`ScaledResolution.java`:27-30, `:37-40`).
    fn chat_resolution() -> ScaledResolution {
        scaled_resolution(1280, 720, 0)
    }

    /// The mirror's draws at `tick`: the log's fade and the record line's clock read
    /// the tick the frame draws at.
    fn chat_draws(chat: &mut ChatView, tick: u64) -> Vec<HudDraw> {
        chat.update(tick);
        chat.draws(chat_resolution(), &ChatInput::default())
    }

    /// A text draw's own fields, for the pins.
    fn chat_text(draw: &HudDraw) -> (String, f32, f32, f32, [f32; 4], bool) {
        match draw {
            HudDraw::Text {
                text,
                x,
                y,
                scale,
                colour,
                shadow,
                ..
            } => (text.clone(), *x, *y, *scale, *colour, *shadow),
            other => panic!("a text draw: {other:?}"),
        }
    }

    /// A text draw's blend marker, for the pins.
    fn text_blend(draw: &HudDraw) -> bool {
        match draw {
            HudDraw::Text { blend, .. } => *blend,
            other => panic!("a text draw: {other:?}"),
        }
    }

    /// A rect draw's own fields, for the pins.
    fn chat_rect(draw: &HudDraw) -> (f32, f32, f32, f32, [f32; 4]) {
        match draw {
            HudDraw::Rect {
                x,
                y,
                width,
                height,
                colour,
            } => (*x, *y, *width, *height, *colour),
            other => panic!("a rect draw: {other:?}"),
        }
    }

    #[test]
    fn a_chat_line_draws_at_its_tick_and_not_past_the_fade() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        // A position-code-1 message — the box's own kind — logged at tick 1000.
        chat.observe("\"A\"", 1, 1_000);

        assert_eq!(
            (chat_resolution().width, chat_resolution().height),
            (427, 240)
        );
        let draws = chat_draws(&mut chat, 1_000);
        assert_eq!(draws.len(), 2, "one bar and its text");
        // The bar: x 2, top 240 - 37, 324 = the 320 wrap budget plus four wide, nine
        // tall — the pitch's own — and black at 255 / 2 = 127 over 255
        // (`GuiNewChat.java`:49-51, `:81-82`).
        assert_eq!(
            chat_rect(&draws[0]),
            (
                2.0,
                240.0 - 37.0,
                324.0,
                9.0,
                [0.0, 0.0, 0.0, 127.0 / 255.0]
            )
        );
        // The text: x 2, one pixel below the bar's top, scale one, the line's runs as
        // their legacy string, white at 255 over 255, shadowed
        // (`GuiNewChat.java`:83-85).
        assert_eq!(
            chat_text(&draws[1]),
            (
                "A§r".to_owned(),
                2.0,
                240.0 - 36.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );

        // The fade at whole ticks (`GuiNewChat.java`:63-68, `:78`): age 197 leaves the
        // last drawable alpha, five, and its bar halves to two; ages 198 and 200 are
        // gone — the arithmetic leaves two and zero, under the gate.
        let draws = chat_draws(&mut chat, 1_000 + 197);
        assert_eq!(draws.len(), 2, "the last drawn tick");
        assert_eq!(
            chat_rect(&draws[0]).4,
            [0.0, 0.0, 0.0, 2.0 / 255.0],
            "5 / 2 = 2 at age 197"
        );
        assert_eq!(chat_text(&draws[1]).4, [1.0, 1.0, 1.0, 5.0 / 255.0]);
        assert!(chat_draws(&mut chat, 1_000 + 198).is_empty());
        assert!(chat_draws(&mut chat, 1_000 + 200).is_empty());
    }

    #[test]
    fn the_scroll_shifts_the_drawn_slice() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        for index in 1..=12 {
            chat.observe(&format!("\"x{index}\""), 1, 0);
        }
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(draws.len(), 20, "ten lines closed, two draws each");
        assert_eq!(chat_text(&draws[1]).0, "x12§r", "newest first");
        assert_eq!(chat_text(&draws[19]).0, "x3§r", "the tenth kept line");

        // Two lines of scroll move the slice two older (`GuiNewChat.scroll`:222-237);
        // the closed window still shows ten, so the oldest kept line is in view.
        chat.scroll(2);
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(draws.len(), 20);
        assert_eq!(chat_text(&draws[1]).0, "x10§r");
        assert_eq!(chat_text(&draws[19]).0, "x1§r", "the oldest kept line");

        // Resetting walks it back: the slice starts at the newest again.
        chat.reset_scroll();
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(chat_text(&draws[1]).0, "x12§r");
    }

    #[test]
    fn a_position_two_message_draws_above_the_hotbar_and_stays_out_of_the_box() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        chat.observe("\"hi\"", 1, 1_000);
        chat.observe("\"tip\"", 2, 1_000);

        // The record line draws first (`GuiIngame.java`:245-272 runs before the chat
        // block at `:339-347`), centred: 427 / 2 - 3 / 2 = 212 (`:259`, `:269`), four
        // pixels above the box's `height - 68` line, white at full alpha, no shadow
        // (`:269`).
        let draws = chat_draws(&mut chat, 1_000);
        assert_eq!(draws.len(), 3);
        assert_eq!(
            chat_text(&draws[0]),
            (
                "tip".to_owned(),
                212.0,
                240.0 - 72.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                false
            )
        );
        // The position-1 message is not lost: it is the box's own line, and the
        // position-2 message never entered the box.
        assert_eq!(
            chat_rect(&draws[1]),
            (
                2.0,
                240.0 - 37.0,
                324.0,
                9.0,
                [0.0, 0.0, 0.0, 127.0 / 255.0]
            )
        );
        assert_eq!(chat_text(&draws[2]).0, "hi§r");

        // The record line holds sixty ticks (`GuiIngame.java`:1118-1122): its last
        // full one reads (int)(1 * 255 / 20) = 12, and the next tick is gone.
        let draws = chat_draws(&mut chat, 1_000 + 59);
        assert_eq!(chat_text(&draws[0]).4, [1.0, 1.0, 1.0, 12.0 / 255.0]);
        assert_eq!(draws.len(), 3, "the box's own line is still fading");
        let draws = chat_draws(&mut chat, 1_000 + 60);
        assert_eq!(draws.len(), 2, "the record line's sixty ticks are up");
        assert_eq!(chat_text(&draws[1]).0, "hi§r");
    }

    #[test]
    fn a_line_recomposes_its_styles_into_the_legacy_string() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        // The runs recompose the way `getFormattedText` does
        // (`ChatComponentStyle.java`:87-99, `ChatStyle.getFormattingCode`:306-346):
        // the colour code, the style codes in the source's order, the characters,
        // then a reset.
        chat.observe(
            "{\"text\":\"A\",\"color\":\"red\",\"bold\":true,\"italic\":true}",
            1,
            0,
        );
        let draws = chat_draws(&mut chat, 0);
        assert_eq!(chat_text(&draws[1]).0, "§c§l§oA§r");
    }

    #[test]
    fn the_opacity_factor_is_one_at_the_settings_default() {
        // `chatOpacity * 0.9F + 0.1F` (`GuiNewChat.java`:38) at the source's default
        // 1.0F (`GameSettings.java`:85): the f32 sum lands on 1.0 exactly, so the
        // fade byte passes through untouched.
        assert_eq!(opacity_factor(1.0), 1.0);
    }

    #[test]
    fn a_message_before_the_font_waits_for_it() {
        // The window can see chat before the asset store lands: the mirror holds the
        // parsed component until the font arrives, then wraps it.
        let mut chat = ChatView::new();
        chat.observe("\"A\"", 0, 7);
        assert!(chat_draws(&mut chat, 7).is_empty(), "nothing measures yet");
        chat.set_font(chat_font());
        let draws = chat_draws(&mut chat, 7);
        assert_eq!(draws.len(), 2);
        assert_eq!(chat_text(&draws[1]).0, "A§r");
    }

    // ---- the chat screen: the field's line, the hover and the confirm overlay ----

    /// The open field with `text` typed and the cursor at its end: what a T-open
    /// and a typed sentence leave behind.
    fn open_text(text: &str) -> ChatInput {
        let mut field = ChatInput::default();
        field.open("");
        field.type_text(text);
        field
    }

    /// The open field's own line — the frame, the text and the caret
    /// (`GuiChat.drawScreen`:303-304 over `GuiTextField.drawTextBox`:525-592).
    ///
    /// The frame is the source's `drawRect(2, height - 14, width - 2, height - 2)`
    /// at `Integer.MIN_VALUE` (`GuiChat.java`:303); the text sits at the field's pen
    /// `(4, height - 12)` (`:58`) at the enabled colour — `14737632` =
    /// `0xE0E0E0` (`GuiTextField.java`:52); the caret rides the blink (`:540`):
    /// with the cursor at the text's end the underscore at the pen (`:582`), with
    /// text after it a bar straddling the stepped-back pen (`:571-578`,
    /// `i1 - 1` to `i1 + 1 + FONT_HEIGHT`).
    #[test]
    fn the_input_line_draws_its_text_and_the_caret_in_both_blink_phases() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        let enabled = [224.0 / 255.0, 224.0 / 255.0, 224.0 / 255.0, 1.0];

        // The cursor at the end, the blink lit: the underscore at the pen.
        let field = open_text("AA");
        let draws = chat.draws(chat_resolution(), &field);
        assert_eq!(draws.len(), 3, "the frame, the text and the caret");
        assert_eq!(
            chat_rect(&draws[0]),
            (
                2.0,
                240.0 - 14.0,
                427.0 - 4.0,
                12.0,
                [0.0, 0.0, 0.0, 128.0 / 255.0]
            ),
            "the field's frame at Integer.MIN_VALUE"
        );
        assert_eq!(
            chat_text(&draws[1]),
            ("AA".to_owned(), 4.0, 240.0 - 12.0, 1.0, enabled, true),
            "the text at the field's pen"
        );
        assert_eq!(
            chat_text(&draws[2]),
            ("_".to_owned(), 4.0 + 12.0, 240.0 - 12.0, 1.0, enabled, true),
            "the end caret is the underscore at the pen"
        );

        // The cursor mid-text, the blink lit: the bar straddles pen - 1, and the
        // text after the cursor draws from that stepped-back pen (`:571-575`).
        let mut field = open_text("AA");
        field.left();
        let draws = chat.draws(chat_resolution(), &field);
        assert_eq!(
            draws.len(),
            4,
            "the frame, the prefix, the tail and the bar"
        );
        assert_eq!(chat_text(&draws[1]).0, "A", "the prefix at the pen");
        assert_eq!(
            chat_text(&draws[2]),
            ("A".to_owned(), 9.0, 240.0 - 12.0, 1.0, enabled, true),
            "the tail from the stepped-back pen"
        );
        assert_eq!(
            chat_rect(&draws[3]),
            (
                9.0,
                240.0 - 13.0,
                1.0,
                11.0,
                [208.0 / 255.0, 208.0 / 255.0, 208.0 / 255.0, 1.0]
            ),
            "the caret bar: pen - 1, i1 - 1 to i1 + 1 + FONT_HEIGHT"
        );

        // The blink down: the text stays, the caret goes (`:540`'s
        // `cursorCounter / 6 % 2 == 0`).
        let mut field = open_text("AA");
        for _ in 0..6 {
            field.tick();
        }
        let draws = chat.draws(chat_resolution(), &field);
        assert_eq!(draws.len(), 2, "the frame and the text, no caret");
        assert_eq!(chat_text(&draws[1]).0, "AA");
    }

    /// The hover tooltip (`GuiScreen.handleComponentHover`'s SHOW_TEXT branch
    /// over `GuiScreen.drawHoveringText`:189-263): the fill and border box at the cursor's
    /// point — fill `-267386864`, the `0x505000FF` top stop and its halved
    /// `0x5028007F` bottom — and the hover's text in white, eight then twelve
    /// pixels down the box's lines.
    ///
    /// The source splits the hover at its newlines (`:245`); the port wraps the
    /// formatted text at the GUI width — the bound the source's own overflow
    /// flip names (`l1 + i > this.width`, `:218-221`) — so a long hover cannot
    /// run off the screen. The vertical gradient edges draw flat at their first
    /// stop, the milestone's stand-in as the death view records for its own.
    #[test]
    fn the_hover_tooltip_draws_at_the_cursor_and_wraps_at_the_gui_width() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        // The box's newest line carries a hover whose text wraps: forty 'A's, a
        // space and forty more — at the 427-pixel cap it breaks at the space.
        let hover = format!("{}{}{}", "A".repeat(40), " ", "A".repeat(40));
        chat.observe(
            &format!(
                "{{\"text\":\"AA\",\"hoverEvent\":{{\"action\":\"show_text\",\"value\":{{\"text\":\"{hover}\"}}}}}}"
            ),
            1,
            0,
        );
        // The pointer sits on the run: the newest line's text row, at its left.
        chat.feed_hover(Some((2.0, 205.0)), chat_resolution());
        let draws = chat.draws(chat_resolution(), &ChatInput::default());
        assert_eq!(
            draws.len(),
            2 + 7,
            "the box's line, then the tooltip's seven"
        );

        // The fill: (l1 - 4, i2 - 4) with l1 = 2 + 12 = 14, i2 = 205 - 12 = 193,
        // the widest line's 244 pixels (the continuation " A..." with its reset
        // `§`; space 4 + forty A's at 6), k = 8 + 2 + 10 = 20.
        let tooltip = &draws[2..];
        assert_eq!(
            chat_rect(&tooltip[0]),
            (
                10.0,
                189.0,
                252.0,
                28.0,
                [16.0 / 255.0, 0.0, 16.0 / 255.0, 240.0 / 255.0]
            ),
            "the fill: the source's five same-colour rects as one box"
        );
        // The borders (`:236-241`): the two edges, then the top and bottom strips.
        assert_eq!(
            chat_rect(&tooltip[1]),
            (
                11.0,
                191.0,
                1.0,
                24.0,
                [80.0 / 255.0, 0.0, 1.0, 80.0 / 255.0]
            ),
            "the left edge at the top stop"
        );
        assert_eq!(
            chat_rect(&tooltip[2]),
            (
                260.0,
                191.0,
                1.0,
                24.0,
                [80.0 / 255.0, 0.0, 1.0, 80.0 / 255.0]
            )
        );
        assert_eq!(
            chat_rect(&tooltip[3]),
            (
                11.0,
                190.0,
                250.0,
                1.0,
                [80.0 / 255.0, 0.0, 1.0, 80.0 / 255.0]
            )
        );
        assert_eq!(
            chat_rect(&tooltip[4]),
            (
                11.0,
                215.0,
                250.0,
                1.0,
                [40.0 / 255.0, 0.0, 127.0 / 255.0, 80.0 / 255.0]
            ),
            "the bottom strip at the halved stop"
        );
        // The lines: the first at i2, the second twelve down (`:242-252`), white
        // and shadowed.
        assert_eq!(
            chat_text(&tooltip[5]),
            (
                format!("{}§r", "A".repeat(40)),
                14.0,
                193.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );
        assert_eq!(
            chat_text(&tooltip[6]),
            (
                format!(" {}§r", "A".repeat(40)),
                14.0,
                205.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            ),
            "the wrapped continuation keeps the space it broke at"
        );

        // The pointer away from the run: the tooltip is gone again — the feed
        // overwrites it every frame, and there is no delay.
        chat.feed_hover(Some((2.0, 100.0)), chat_resolution());
        assert_eq!(
            chat.draws(chat_resolution(), &ChatInput::default()).len(),
            2
        );
    }

    /// The scripted open reconciles the box open state: the frame puts the
    /// view's open flag back in step with the field's
    /// ([`reconcile_chat_open`]) — the script's `chat` line drives the field
    /// alone, and the box must follow it (`GuiChat` is the screen and its field
    /// at once in the source; `Minecraft.java`:1010-1012 swaps both).
    #[test]
    fn the_scripted_open_reconciles_the_box_open_state() {
        // The script's `chat` line drives the field alone; the frame's
        // reconciliation is what makes the box follow it — the view a windowed
        // open leaves is what a scripted open must come to. Fifteen lines: an
        // open window draws all fifteen, a closed one the last ten, so the two
        // states are distinguishable.
        let lines: Vec<String> = (1..=15).map(|index| format!("\"x{index}\"")).collect();
        let mut windowed = ChatView::new();
        windowed.set_font(chat_font());
        for line in &lines {
            windowed.observe(line, 1, 0);
        }
        windowed.set_open(true);
        let mut scripted = ChatView::new();
        scripted.set_font(chat_font());
        for line in &lines {
            scripted.observe(line, 1, 0);
        }
        let closed = ChatInput::default();
        assert_ne!(
            scripted.draws(chat_resolution(), &closed),
            windowed.draws(chat_resolution(), &closed),
            "the scripted open is not there yet"
        );
        let field = open_text("");
        reconcile_chat_open(&mut scripted, &field);
        assert_eq!(
            scripted.draws(chat_resolution(), &field),
            windowed.draws(chat_resolution(), &field),
            "the reconciliation brings the box up to the windowed open"
        );
        // And the close: the scripted send closes the field, and the frame
        // takes the box back down the same way — to the never-opened view.
        let mut closed_only = ChatView::new();
        closed_only.set_font(chat_font());
        for line in &lines {
            closed_only.observe(line, 1, 0);
        }
        reconcile_chat_open(&mut scripted, &closed);
        assert_eq!(
            scripted.draws(chat_resolution(), &closed),
            closed_only.draws(chat_resolution(), &closed),
            "the open reconcile comes back down with the field"
        );
    }

    /// The confirm overlay — the port's stand-in for the source's confirm screen
    /// (`GuiScreen.java`:403-433 stores the link and swaps the screen in;
    /// `GuiYesNo.drawScreen`:69-79 draws it): the dim of
    /// `drawDefaultBackground`'s first stop (`GuiScreen.java`:668-677), the
    /// two-key prompt in the title's slot (centred at seventy,
    /// `GuiYesNo.drawScreen`:72) and the URL wrapped at `width - 50`
    /// (`GuiYesNo.initGui`:55) from ninety down, a font line per line.
    #[test]
    fn the_confirm_overlay_draws_the_dim_the_url_and_the_prompt() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        chat.open_confirm("https://a.example");
        let draws = chat.draws(chat_resolution(), &ChatInput::default());
        assert_eq!(draws.len(), 3, "the dim, the prompt and the url");
        assert_eq!(
            chat_rect(&draws[0]),
            (
                0.0,
                0.0,
                427.0,
                240.0,
                [16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0, 192.0 / 255.0]
            ),
            "the dim at the gradient's first stop"
        );
        // The prompt, centred with the source's integer halves: 427 / 2 - 51 / 2
        // = 213 - 25 = 188; the url's formatted width 16 (one reset `§`) halves
        // to eight, so 213 - 8 = 205 at the message's ninety.
        assert_eq!(
            chat_text(&draws[1]),
            (
                "Enter opens the link, Escape cancels".to_owned(),
                188.0,
                70.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );
        assert_eq!(
            chat_text(&draws[2]),
            (
                "https://a.example§r".to_owned(),
                205.0,
                90.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );

        // A url past the width - 50 budget wraps, one font line per line: the
        // hundred 'A's break at 62 (62 * 6 = 372 <= 377), so 213 - 372 / 2 = 27
        // and, nine down, 213 - 228 / 2 = 99.
        chat.cancel_confirm();
        chat.open_confirm(&"A".repeat(100));
        let draws = chat.draws(chat_resolution(), &ChatInput::default());
        assert_eq!(draws.len(), 4, "the dim, the prompt and two url lines");
        assert_eq!(
            chat_text(&draws[2]),
            (
                format!("{}§r", "A".repeat(62)),
                27.0,
                90.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );
        assert_eq!(
            chat_text(&draws[3]),
            (
                format!("{}§r", "A".repeat(38)),
                99.0,
                99.0,
                1.0,
                [1.0, 1.0, 1.0, 1.0],
                true
            )
        );

        // While the overlay stands in for the replaced screen the field's own
        // line is not drawn under it (`GuiScreen.java`:425-429 swaps the chat
        // screen out through `Minecraft.java`:1010-1012).
        let field = open_text("AA");
        let draws = chat.draws(chat_resolution(), &field);
        assert_eq!(draws.len(), 4, "no field line under the overlay");
        assert_eq!(chat_text(&draws[2]).0, format!("{}§r", "A".repeat(62)));
    }

    /// The hit-test: the run under a scaled-GUI point
    /// (`GuiChat.mouseClicked`:172-186 over `GuiNewChat.getChatComponent`:245-300,
    /// which reads the raw mouse position against the box; the port reads the
    /// frame's scaled units against the drawn bars, so a hit cannot drift from
    /// the draws).
    ///
    /// The two lines sit at the box's own steps: the newest bar top at
    /// `240 - 37 = 203` with its text row at 204, the second at 194. A point on
    /// a line's band hits its run — the first run whose pen it reaches — and a
    /// point left of the box's origin or past a line's last glyph hits nothing.
    #[test]
    fn the_hit_test_maps_a_point_to_the_run_under_it() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        chat.observe("\"AA\"", 1, 0);
        chat.observe("\"AAAA\"", 1, 0);
        let resolution = chat_resolution();
        let run_at = |point| chat.run_at(point, resolution).map(|run| run.text.clone());

        // The newest line's run: "AAAA" draws 24 pixels from the bar's left.
        assert_eq!(run_at((2.0, 205.0)).as_deref(), Some("AAAA"));
        assert_eq!(
            run_at((25.9, 205.0)).as_deref(),
            Some("AAAA"),
            "the run's last column"
        );
        assert_eq!(run_at((26.0, 205.0)), None, "one past the text is no run");
        assert_eq!(run_at((1.0, 205.0)), None, "left of the box's origin");
        assert_eq!(run_at((2.0, 212.0)), None, "below the newest bar");

        // The line boundary: 203 opens the newest line's band, 202 the one under.
        assert_eq!(run_at((2.0, 203.0)).as_deref(), Some("AAAA"));
        assert_eq!(
            run_at((2.0, 202.0)).as_deref(),
            Some("AA"),
            "the second line's band"
        );
        assert_eq!(run_at((2.0, 194.0)).as_deref(), Some("AA"), "its first row");
        assert_eq!(run_at((2.0, 193.0)), None, "above the box");
    }

    /// The absent-state frame is byte-stable: with no field line, no tooltip and
    /// no overlay set, `draws` is exactly the pre-screen frame the box always
    /// made — the same two draws the fade test pins.
    #[test]
    fn the_absent_state_frame_is_the_old_frame() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        chat.observe("\"AA\"", 1, 0);
        let draws = chat.draws(chat_resolution(), &ChatInput::default());
        assert_eq!(
            draws,
            vec![
                HudDraw::Rect {
                    x: 2.0,
                    y: 240.0 - 37.0,
                    width: 324.0,
                    height: 9.0,
                    colour: [0.0, 0.0, 0.0, 127.0 / 255.0],
                },
                HudDraw::Text {
                    text: "AA§r".to_owned(),
                    x: 2.0,
                    y: 240.0 - 36.0,
                    scale: 1.0,
                    colour: [1.0, 1.0, 1.0, 1.0],
                    shadow: true,
                    blend: true,
                },
            ],
            "the closed field, no tooltip, no overlay: the old draws, byte for byte"
        );

        // A fontless mirror draws nothing even with the field open: nothing
        // measures before the sheet lands.
        let field = open_text("AA");
        assert!(ChatView::new().draws(chat_resolution(), &field).is_empty());
    }

    /// The scripted `chat` drives the field machine directly — open, type and
    /// send on one tick — so the frame reconciles the mirror's open window from
    /// the field it draws ([`reconcile_chat_open`]): a stale open window
    /// follows a closed field, and an open field keeps it open.
    #[test]
    fn the_views_window_reconciles_from_the_field() {
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        for index in 1..=15 {
            chat.observe(&format!("\"x{index}\""), 1, 0);
        }
        chat.set_open(true);
        let closed = ChatInput::default();
        let draws = chat.draws(chat_resolution(), &closed);
        assert_eq!(draws.len(), 30, "the open window draws the fifteen lines");
        reconcile_chat_open(&mut chat, &closed);
        let draws = chat.draws(chat_resolution(), &closed);
        assert_eq!(draws.len(), 20, "the closed field's ten-line window");
        let field = open_text("");
        reconcile_chat_open(&mut chat, &field);
        let draws = chat.draws(chat_resolution(), &field);
        assert_eq!(
            draws.len(),
            30 + 2,
            "an open field keeps the fifteen-line window, its line on top"
        );
    }

    // ---- the tab list ----

    /// A record for the tab fixtures: its name alone carries meaning — a fresh uuid off the
    /// name, gamemode zero and a low ping.
    fn tab_record(name: &str) -> PlayerListRecord {
        PlayerListRecord {
            uuid: format!("uuid-{name}"),
            name: name.to_owned(),
            properties: Vec::new(),
            gamemode: 0,
            latency: 1,
            display_name: None,
        }
    }

    /// The tab fixture's draws: held, at the chat resolution, with one stand-in head id.
    fn tab_frame(tab: &TabState, board: &Scoreboard, font: &Font) -> Vec<HudDraw> {
        tab.tab_draws_with(board, font, chat_resolution(), &|_| SkinTexId::new(7))
    }

    /// A skin draw's own texture id, position, size and uv, for the pins.
    fn tab_skin(draw: &HudDraw) -> (SkinTexId, f32, f32, f32, f32, [f32; 4]) {
        match draw {
            HudDraw::SkinRect {
                texture,
                x,
                y,
                width,
                height,
                uv,
                ..
            } => (*texture, *x, *y, *width, *height, *uv),
            other => panic!("a skin draw: {other:?}"),
        }
    }

    /// A textured draw's own texture, position, size and uv, for the pins.
    fn tab_texture(draw: &HudDraw) -> (HudTexture, f32, f32, f32, f32, [f32; 4]) {
        match draw {
            HudDraw::TexturedRect {
                texture,
                x,
                y,
                width,
                height,
                uv,
                ..
            } => (*texture, *x, *y, *width, *height, *uv),
            other => panic!("a textured draw: {other:?}"),
        }
    }

    #[test]
    fn a_closed_tab_draws_nothing() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.entries = vec![tab_record("AAAA")];
        assert!(tab_frame(&tab, &Scoreboard::new(), &font).is_empty());
    }

    #[test]
    fn three_entries_draw_one_column_at_the_pinned_x() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA"); 3];
        let draws = tab_frame(&tab, &Scoreboard::new(), &font);
        // The names measure twenty-four: the cell is 9 + 24 + 13 = 46, the grid's left edge
        // 427 / 2 - 46 / 2 = 190, and the panel spans it plus a pixel per side.
        assert_eq!(
            chat_rect(&draws[0]),
            (189.0, 9.0, 48.0, 28.0, TAB_PANEL),
            "the grid background"
        );
        for row in 0..3usize {
            let base = 1 + 5 * row;
            let y = 10.0 + row as f32 * 9.0;
            assert_eq!(
                chat_rect(&draws[base]),
                (190.0, y, 46.0, 8.0, TAB_CELL),
                "row {row} cell"
            );
            let (id, x, top, width, height, uv) = tab_skin(&draws[base + 1]);
            assert_eq!(id, SkinTexId::new(7), "row {row} head id");
            assert_eq!(
                (x, top, width, height, uv),
                (
                    190.0,
                    y,
                    8.0,
                    8.0,
                    [8.0 / 64.0, 8.0 / 64.0, 16.0 / 64.0, 16.0 / 64.0]
                ),
                "row {row} face"
            );
            assert_eq!(
                tab_skin(&draws[base + 2]).5,
                [40.0 / 64.0, 8.0 / 64.0, 48.0 / 64.0, 16.0 / 64.0],
                "row {row} hat"
            );
            assert_eq!(
                chat_text(&draws[base + 3]),
                ("AAAA".to_owned(), 199.0, y, 1.0, TAB_TEXT, true),
                "row {row} name"
            );
            let (texture, x, top, width, height, uv) = tab_texture(&draws[base + 4]);
            assert_eq!(
                (texture, x, top, width, height),
                (HudTexture::Named(TAB_ICONS), 225.0, y, 10.0, 8.0),
                "row {row} ping"
            );
            assert_eq!(uv, latency_uv(0), "row {row} ping level");
        }
        assert_eq!(draws.len(), 16, "the grid, then three five-draw rows");
    }

    #[test]
    fn twenty_one_entries_split_two_columns_of_eleven() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = (0..21).map(|_| tab_record("A")).collect();
        let draws = tab_frame(&tab, &Scoreboard::new(), &font);
        // The six-wide names: the cell is min(2 * 28, 377) / 2 = 28, eleven rows and the
        // grid's left edge 213 - (56 + 5) / 2 = 183.
        assert_eq!(
            chat_rect(&draws[0]),
            (182.0, 9.0, 63.0, 100.0, TAB_PANEL),
            "the two-column grid"
        );
        // Column-major: entry 10 closes the first column, entry 11 opens the second.
        assert_eq!(chat_rect(&draws[1 + 5 * 10]).0, 183.0, "entry 10 x");
        assert_eq!(chat_rect(&draws[1 + 5 * 10]).1, 100.0, "entry 10 y");
        assert_eq!(chat_rect(&draws[1 + 5 * 11]).0, 216.0, "entry 11 x");
        assert_eq!(chat_rect(&draws[1 + 5 * 11]).1, 10.0, "entry 11 y");
        assert_eq!(chat_rect(&draws[1 + 5 * 20]).0, 216.0, "entry 20 x");
        assert_eq!(chat_rect(&draws[1 + 5 * 20]).1, 91.0, "entry 20 y");
        assert_eq!(draws.len(), 1 + 21 * 5);
    }

    #[test]
    fn twenty_entries_hold_one_column() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = (0..20).map(|_| tab_record("A")).collect();
        let draws = tab_frame(&tab, &Scoreboard::new(), &font);
        // min(28, 377) = 28 in one column: the left edge 213 - 14 = 199.
        assert_eq!(
            chat_rect(&draws[0]),
            (198.0, 9.0, 30.0, 181.0, TAB_PANEL),
            "the single-column grid"
        );
        let last = 1 + 5 * 19;
        assert_eq!(chat_rect(&draws[last]), (199.0, 181.0, 28.0, 8.0, TAB_CELL));
    }

    #[test]
    fn the_list_keeps_eighty_entries_after_the_sort() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = (0..81)
            .map(|index| tab_record(&format!("u{index:02}")))
            .collect();
        let draws = tab_frame(&tab, &Scoreboard::new(), &font);
        // Names u00..u80 sort ascending; the eighty-first falls and eighty rows remain over
        // four columns of twenty.
        assert_eq!(draws.len(), 1 + 80 * 5, "the cap held");
        assert_eq!(
            chat_rect(&draws[0]).3,
            20.0 * 9.0 + 1.0,
            "four columns of twenty"
        );
        assert_eq!(
            chat_text(&draws[1 + 5 * 79 + 3]).0,
            "u79",
            "the last kept name"
        );
        assert!(
            !draws
                .iter()
                .any(|draw| matches!(draw, HudDraw::Text { text, .. } if text == "u80")),
            "the dropped name never draws"
        );
    }

    #[test]
    fn non_spectators_sort_before_spectators_by_team_then_name() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_team("a", "A", "", "", 0, "always", None);
        board.set_team("b", "B", "", "", 0, "always", None);
        board.add_team_players("a", &["Beta".to_owned(), "Mid".to_owned()]);
        board.add_team_players("b", &["Alpha".to_owned()]);
        let mut tab = TabState::new();
        tab.open = true;
        let mut spectator = tab_record("Mid");
        spectator.gamemode = SPECTATOR_GAMEMODE;
        spectator.latency = -1;
        tab.entries = vec![
            tab_record("Zulu"),
            spectator,
            tab_record("Alpha"),
            tab_record("Beta"),
        ];
        let draws = tab_frame(&tab, &board, &font);
        // No team ("") < team "a" < team "b", and the spectator last despite its team.
        let names: Vec<String> = (0..4)
            .map(|row| chat_text(&draws[1 + 5 * row + 3]).0)
            .collect();
        assert_eq!(names, vec!["Zulu", "Beta", "Alpha", "§oMid"]);
        assert_eq!(
            chat_text(&draws[4]).1,
            206.0,
            "the name past the head column"
        );
        assert_eq!(
            chat_text(&draws[1 + 5 * 3 + 3]).4,
            [1.0, 1.0, 1.0, 144.0 / 255.0],
            "the spectator's name colour"
        );
        // The no-signal X draws on its row like any other ping (`:248-251`).
        assert_eq!(tab_texture(&draws[1 + 5 * 3 + 4]).5, latency_uv(5));
    }

    #[test]
    fn the_header_and_footer_sit_at_the_pinned_positions() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA"); 3];
        tab.header = "{\"text\":\"hi\"}".to_owned();
        tab.footer = "{\"text\":\"bye\"}".to_owned();
        let draws = tab_frame(&tab, &Scoreboard::new(), &font);
        // The header block at the top margin: its panel one pixel wider than the grid per
        // side and one taller than its line, the line centred.
        assert_eq!(
            chat_rect(&draws[0]),
            (189.0, 9.0, 48.0, 10.0, TAB_PANEL),
            "the header background"
        );
        assert_eq!(
            chat_text(&draws[1]),
            ("hi§r".to_owned(), 212.0, 10.0, 1.0, TAB_TEXT, true),
            "the header line"
        );
        // The grid follows one pixel under the block, its rows from y 20.
        assert_eq!(chat_rect(&draws[2]), (189.0, 19.0, 48.0, 28.0, TAB_PANEL));
        assert_eq!(
            chat_rect(&draws[3]).1,
            20.0,
            "the first row under the header"
        );
        // The footer: one pixel under the grid block, 20 + 27 + 1 = 48.
        assert_eq!(
            chat_rect(&draws[18]),
            (189.0, 47.0, 48.0, 10.0, TAB_PANEL),
            "the footer background"
        );
        assert_eq!(
            chat_text(&draws[19]),
            ("bye§r".to_owned(), 212.0, 48.0, 1.0, TAB_TEXT, true),
            "the footer line"
        );
        assert_eq!(draws.len(), 20);
    }

    #[test]
    fn a_list_objective_scores_right_aligned_in_yellow() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_objective("points", "Points", "integer");
        board.set_display(0, Some("points"));
        board.set_score("AAAA", "points", 10);
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA"); 3];
        let draws = tab_frame(&tab, &board, &font);
        // The score field measures width(" 10") = 6: the cell is 9 + 24 + 6 + 13 = 52, and the
        // number right-aligns into the field ending at 221 + 6.
        for row in 0..3usize {
            let (text, x, y, scale, colour, shadow) = chat_text(&draws[1 + 6 * row + 4]);
            assert_eq!(text, "§e10", "row {row}");
            assert_eq!(x, 225.0, "row {row}");
            assert_eq!(y, 10.0 + row as f32 * 9.0, "row {row}");
            assert_eq!((scale, colour, shadow), (1.0, TAB_TEXT, true), "row {row}");
        }
        assert_eq!(draws.len(), 1 + 3 * 6, "six draws to a scored row");
    }

    #[test]
    fn a_score_field_five_wide_or_narrower_skips_the_scores() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_objective("points", "Points", "integer");
        board.set_display(0, Some("points"));
        board.set_score("AAAA", "points", 5);
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA")];
        let draws = tab_frame(&tab, &board, &font);
        // width(" 5") = 5 fails the source's `l5 - k5 > 5` gate: the row draws without a
        // score.
        assert_eq!(draws.len(), 6, "the grid and one five-draw row");
        assert!(
            !draws
                .iter()
                .any(|draw| matches!(draw, HudDraw::Text { text, .. } if text.starts_with("§e"))),
            "no number drew"
        );
    }

    #[test]
    fn a_spectator_row_keeps_its_ping_but_draws_no_score() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_objective("points", "Points", "integer");
        board.set_display(0, Some("points"));
        board.set_score("AAAA", "points", 10);
        let mut tab = TabState::new();
        tab.open = true;
        let mut spectator = tab_record("AAAA");
        spectator.gamemode = SPECTATOR_GAMEMODE;
        tab.entries = vec![tab_record("AAAA"), spectator];
        let draws = tab_frame(&tab, &board, &font);
        assert_eq!(
            draws.len(),
            1 + 6 + 5,
            "the spectator's row loses the score"
        );
        assert_eq!(chat_text(&draws[1 + 4]).0, "§e10", "the player's number");
        assert_eq!(chat_text(&draws[7 + 3]).0, "§oAAAA", "the spectator's name");
        assert_eq!(
            chat_text(&draws[7 + 3]).4,
            TAB_SPECTATOR,
            "its alpha colour"
        );
        assert!(
            matches!(draws[7 + 4], HudDraw::TexturedRect { .. }),
            "the ping closes its row"
        );
    }

    #[test]
    fn the_latency_levels_are_the_sources_thresholds() {
        assert_eq!(latency_level(-1), 5, "the no-signal X");
        assert_eq!(latency_level(0), 0);
        assert_eq!(latency_level(149), 0);
        assert_eq!(latency_level(150), 1);
        assert_eq!(latency_level(299), 1);
        assert_eq!(latency_level(300), 2);
        assert_eq!(latency_level(599), 2);
        assert_eq!(latency_level(600), 3);
        assert_eq!(latency_level(999), 3);
        assert_eq!(latency_level(1000), 4);
        assert_eq!(latency_level(i32::MAX), 4);
        // The rects: `(0, 176 + 8 * level, 10, 8)` over the 256 sheet.
        assert_eq!(
            latency_uv(0),
            [0.0, 176.0 / 256.0, 10.0 / 256.0, 184.0 / 256.0]
        );
        assert_eq!(
            latency_uv(4),
            [0.0, 208.0 / 256.0, 10.0 / 256.0, 216.0 / 256.0]
        );
        assert_eq!(
            latency_uv(5),
            [0.0, 216.0 / 256.0, 10.0 / 256.0, 224.0 / 256.0],
            "the X"
        );
    }

    #[test]
    fn a_hearts_objective_draws_the_glyph_row() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_objective("health", "Health", "hearts");
        board.set_display(0, Some("health"));
        board.set_score("AAAA", "health", 15);
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA")];
        let draws = tab_frame(&tab, &board, &font);
        // The ninety-wide field spans 179..269; eight half-heart slots at 86 / 10, two empty
        // containers beyond them, then the ping.
        let scale = 86.0f32 / 10.0;
        assert_eq!(
            draws.len(),
            1 + 4 + 10 + 1,
            "glyphs between the name and the ping"
        );
        // The containers first (slots 8 and 9), then the filled row (`:315-346`).
        assert_eq!(
            tab_texture(&draws[5]),
            (
                HudTexture::Named(TAB_ICONS),
                179.0 + 8.0 * scale,
                10.0,
                9.0,
                9.0,
                [16.0 / 256.0, 0.0, 25.0 / 256.0, 9.0 / 256.0]
            ),
            "the first empty container"
        );
        assert_eq!(
            tab_texture(&draws[6]).1,
            179.0 + 9.0 * scale,
            "the second container"
        );
        assert_eq!(
            tab_texture(&draws[7]),
            (
                HudTexture::Named(TAB_ICONS),
                179.0,
                10.0,
                9.0,
                9.0,
                [52.0 / 256.0, 0.0, 61.0 / 256.0, 9.0 / 256.0]
            ),
            "the first full heart"
        );
        // The half heart rides the last odd slot: 2 * 7 + 1 == 15.
        assert_eq!(
            tab_texture(&draws[14]),
            (
                HudTexture::Named(TAB_ICONS),
                179.0 + 7.0 * scale,
                10.0,
                9.0,
                9.0,
                [61.0 / 256.0, 0.0, 70.0 / 256.0, 9.0 / 256.0]
            ),
            "the half heart"
        );
    }

    #[test]
    fn a_long_heart_row_draws_the_number_instead() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_objective("health", "Health", "hearts");
        board.set_display(0, Some("health"));
        board.set_score("AAAA", "health", 58);
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA")];
        let draws = tab_frame(&tab, &board, &font);
        // Twenty-nine slots push the per-heart scale to 86 / 29 = 2.97, under three: the
        // number branch, green at the saturated fraction, the suffixed text still fitting.
        assert_eq!(draws.len(), 1 + 5 + 1, "the number, then the ping");
        assert_eq!(
            chat_text(&draws[5]),
            (
                "29.0hp".to_owned(),
                221.0,
                10.0,
                1.0,
                [0.0, 1.0, 0.0, 1.0],
                true
            ),
            "the health number"
        );
        assert!(
            matches!(draws[6], HudDraw::TexturedRect { .. }),
            "the ping closes the row"
        );
    }

    #[test]
    fn the_hearts_number_formats_like_java() {
        assert_eq!(java_float_text(29.0), "29.0");
        assert_eq!(java_float_text(28.5), "28.5");
        assert_eq!(java_float_text(7.0), "7.0");
        assert_eq!(java_float_text(1.0e7), "1.0E7");
        assert_eq!(java_float_text(2.5e7), "2.5E7");
    }

    #[test]
    fn the_events_feed_the_list_and_the_pair() {
        let mut tab = TabState::new();
        let record = tab_record("AAAA");
        tab.observe(&ClientEvent::PlayerList {
            entries: vec![record.clone()],
        });
        assert_eq!(tab.entries, vec![record]);
        tab.observe(&ClientEvent::TabText {
            header: "{\"text\":\"top\"}".to_owned(),
            footer: "{\"text\":\"bottom\"}".to_owned(),
        });
        assert_eq!(tab.header, "{\"text\":\"top\"}");
        assert_eq!(tab.footer, "{\"text\":\"bottom\"}");
        // Every other event leaves the tab state alone.
        tab.observe(&ClientEvent::ScoreboardChanged {
            board: Scoreboard::new(),
        });
        assert_eq!(tab.entries, vec![tab_record("AAAA")]);
        // A later report replaces the whole set, an empty one included.
        tab.observe(&ClientEvent::PlayerList {
            entries: Vec::new(),
        });
        assert!(tab.entries.is_empty());
    }

    #[test]
    fn the_name_is_the_display_name_or_the_team_composed_one() {
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_team("a", "A", "§4<", ">", 0, "always", None);
        board.add_team_players("a", &["AAAA".to_owned()]);
        let mut tab = TabState::new();
        tab.open = true;
        let mut nick = tab_record("BBBB");
        nick.display_name = Some("{\"text\":\"Nick\",\"color\":\"red\"}".to_owned());
        tab.entries = vec![tab_record("AAAA"), nick];
        let draws = tab_frame(&tab, &board, &font);
        // No team sorts first: the display name's formatted text draws as sent; the team's
        // prefix and suffix wrap the profile name.
        assert_eq!(chat_text(&draws[4]).0, "§cNick§r");
        assert_eq!(chat_text(&draws[9]).0, "§4<AAAA>");
    }

    #[test]
    fn each_head_carries_the_id_its_resolution_returns() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("Alpha"), tab_record("Beta")];
        let draws = tab.tab_draws_with(&Scoreboard::new(), &font, chat_resolution(), &|uuid| {
            assert!(uuid.starts_with("uuid-"), "the record's own uuid: {uuid}");
            SkinTexId::new(if uuid == "uuid-Alpha" { 1 } else { 2 })
        });
        assert_eq!(tab_skin(&draws[2]).0, SkinTexId::new(1), "the first face");
        assert_eq!(tab_skin(&draws[3]).0, SkinTexId::new(1), "its hat");
        assert_eq!(tab_skin(&draws[7]).0, SkinTexId::new(2), "the second face");
    }

    #[test]
    fn an_empty_held_list_draws_the_degenerate_grid() {
        let font = chat_font();
        let mut tab = TabState::new();
        tab.open = true;
        let draws = tab_frame(&tab, &Scoreboard::new(), &font);
        // The grid background draws unconditionally (`:159`): a one-pixel strip with nothing
        // under it.
        assert_eq!(draws.len(), 1);
        assert_eq!(chat_rect(&draws[0]), (201.0, 9.0, 24.0, 1.0, TAB_PANEL));
    }

    // ---- the scoreboard sidebar ----

    /// A board whose slot-1 objective carries `count` entries named `e00`… with
    /// ascending points above `filtered` `#`-named ones — the clamp cases' fixture.
    fn filtered_board(count: usize, filtered: usize) -> Scoreboard {
        let mut board = Scoreboard::new();
        board.set_objective("side", "Side", "integer");
        board.set_display(1, Some("side"));
        for index in 0..filtered {
            board.set_score(&format!("#{index:02}"), "side", index as i32 + 1);
        }
        for index in 0..count {
            board.set_score(
                &format!("e{index:02}"),
                "side",
                filtered as i32 + index as i32 + 1,
            );
        }
        board
    }

    /// The sidebar's text draws, in draw order.
    fn sidebar_texts(draws: &[HudDraw]) -> Vec<String> {
        draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.clone()),
                HudDraw::Rect { .. } | HudDraw::TexturedRect { .. } | HudDraw::SkinRect { .. } => {
                    None
                }
                HudDraw::Item { .. } | HudDraw::InvertRect { .. } => None,
            })
            .collect()
    }

    #[test]
    fn the_scoreboard_objective_choice_reads_the_own_teams_colour_slot() {
        let font = chat_font();
        // The own team's colour index `i` names slot `3 + i`
        // (`Scoreboard.getObjectiveDisplaySlotNumber:479-486`); the objective the
        // board resolves there wins, and every other outcome — no team, the
        // no-colour sentinel, an empty or unresolvable slot — falls back to slot 1
        // (`GuiIngame.java:319-336`). The drawn title names the objective that won.
        let title_of = |board: &Scoreboard, own: &str| {
            let draws = sidebar_draws(board, own, &font, chat_resolution());
            chat_text(draws.last().expect("a title")).0
        };
        let mut board = Scoreboard::new();
        board.set_objective("side", "Side", "integer");
        board.set_display(1, Some("side"));
        board.set_score("Alpha", "side", 3);
        board.set_objective("red-side", "Red Side", "integer");
        board.set_score("Alpha", "red-side", 4);
        // No team at all reads slot 1.
        assert_eq!(title_of(&board, "Alpha"), "Side");
        // A team without a colour reads slot 1.
        board.set_team("red", "Red", "", "", 0, "always", None);
        board.add_team_players("red", &["Alpha".to_owned()]);
        assert_eq!(title_of(&board, "Alpha"), "Side", "the no-colour sentinel");
        // Red is colour index 12 (`EnumChatFormatting.java:24`): slot 15 wins
        // with its objective even though slot 1 is set too.
        board.set_team("red", "Red", "", "", 0, "always", Some(12));
        board.set_display(15, Some("red-side"));
        assert_eq!(
            title_of(&board, "Alpha"),
            "Red Side",
            "the colour slot wins"
        );
        // The colour slot set but cleared falls back to slot 1.
        board.set_display(15, None);
        assert_eq!(title_of(&board, "Alpha"), "Side", "the empty colour slot");
        // A colour slot naming an objective the board does not hold falls back too.
        board.set_display(15, Some("ghost"));
        assert_eq!(title_of(&board, "Alpha"), "Side", "the dangling name");
        // Nothing set names no sidebar; a slot-1 name the board does not resolve
        // is no objective either.
        assert!(
            sidebar_draws(&Scoreboard::new(), "Alpha", &font, chat_resolution()).is_empty(),
            "nothing set"
        );
        let mut dangling = Scoreboard::new();
        dangling.set_display(1, Some("ghost"));
        assert!(sidebar_draws(&dangling, "Alpha", &font, chat_resolution()).is_empty());
    }

    #[test]
    fn the_scoreboard_geometry_follows_the_sources_baseline() {
        let font = chat_font();
        // The source's own numbers at 427x240 (`GuiIngame.renderScoreboard:581-585`,
        // `:593-595`): three rows, the widest measured line "AAA: §c3" spans 24, so
        // j1 = 120 + 27/3 = 129, the row tops step 129 - 9j = 120/111/102, the text
        // column is 427 - 24 - 3 = 400, and each row's right edge 427 - 1 = 426.
        // Rows draw bottom first; the title band closes the last row's iteration.
        let mut board = Scoreboard::new();
        board.set_objective("t", "T", "integer");
        board.set_display(1, Some("t"));
        board.set_score("A", "t", 1);
        board.set_score("AA", "t", 2);
        board.set_score("AAA", "t", 3);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(chat_resolution().width, 427);
        assert_eq!(chat_resolution().height, 240);
        assert_eq!(draws.len(), 3 * 3 + 3, "three rows and the title band");
        assert_eq!(
            chat_rect(&draws[0]),
            (398.0, 120.0, 28.0, 9.0, SIDEBAR_BAND),
            "the bottom row's background: [l1-2, l) x [k, k+9)"
        );
        assert_eq!(
            chat_text(&draws[1]),
            ("A".to_owned(), 400.0, 120.0, 1.0, SIDEBAR_TEXT, false),
            "the bottom name at the text column"
        );
        assert_eq!(
            chat_text(&draws[2]),
            ("§c1".to_owned(), 425.0, 120.0, 1.0, SIDEBAR_TEXT, false),
            "the number right-aligned at l - width"
        );
        assert_eq!(
            chat_rect(&draws[3]),
            (398.0, 111.0, 28.0, 9.0, SIDEBAR_BAND)
        );
        assert_eq!(chat_text(&draws[4]).0, "AA");
        assert_eq!(chat_text(&draws[4]).2, 111.0, "the middle row's top");
        assert_eq!(chat_text(&draws[5]).0, "§c2");
        assert_eq!(
            chat_rect(&draws[6]),
            (398.0, 102.0, 28.0, 9.0, SIDEBAR_BAND)
        );
        assert_eq!(chat_text(&draws[7]).0, "AAA");
        assert_eq!(chat_text(&draws[8]).0, "§c3");
        assert_eq!(
            chat_rect(&draws[9]),
            (398.0, 92.0, 28.0, 9.0, SIDEBAR_TITLE_BAND),
            "the title band spans [k-10, k-1)"
        );
        assert_eq!(
            chat_rect(&draws[10]),
            (398.0, 101.0, 28.0, 1.0, SIDEBAR_BAND),
            "the one-pixel separator spans [k-1, k)"
        );
        assert_eq!(
            chat_text(&draws[11]),
            ("T".to_owned(), 412.0, 93.0, 1.0, SIDEBAR_TEXT, false),
            "the title at l1 + i/2 - width/2, y k - 9"
        );
    }

    #[test]
    fn the_scoreboard_sort_is_points_ascending_and_names_descending_within_a_tie() {
        let font = chat_font();
        // The collection order (`Score.scoreComparator`, `Score.java:9-15`): points
        // ascending, equal points by name descending case-insensitively. Rows draw
        // bottom first (`GuiIngame.renderScoreboard:587-593`), so the screen reads
        // points descending top-to-bottom with the tie alphabetically ascending.
        let mut board = Scoreboard::new();
        board.set_objective("side", "Side", "integer");
        board.set_display(1, Some("side"));
        board.set_score("Ann", "side", 1);
        board.set_score("Bob", "side", 5);
        board.set_score("Alice", "side", 5);
        board.set_score("Zed", "side", 9);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(
            sidebar_texts(&draws),
            [
                "Ann", "§c1", "Bob", "§c5", "Alice", "§c5", "Zed", "§c9", "Side"
            ],
            "bottom row first"
        );
        let top_down: Vec<String> = (0..4)
            .rev()
            .map(|row| chat_text(&draws[1 + 3 * row]).0)
            .collect();
        assert_eq!(top_down, ["Zed", "Alice", "Bob", "Ann"], "top-to-bottom");
    }

    /// The unblended glyph runs carry their marker with the source's values kept: the
    /// scoreboard's rects leave blend off (`Gui.java`:82-83) and `renderScoreboard` never
    /// re-enables it (`GuiIngame.java`:551-607), so its name, number and title draws run
    /// unblended; the tab list's header and footer lines follow their own rects
    /// (`GuiPlayerTabOverlay.java`:147-152, `:226-231`); its row text and the chat stay
    /// blended (`:170-173`, `GuiNewChat.java`:84).
    #[test]
    fn the_unblended_glyph_runs_carry_their_marker() {
        let font = chat_font();
        // The sidebar: one entry plus the title band, so every run draws.
        let mut board = Scoreboard::new();
        board.set_objective("demo", "Demo", "integer");
        board.set_display(1, Some("demo"));
        board.set_score("AA", "demo", 7);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(chat_text(&draws[1]).0, "AA", "the name");
        assert!(!text_blend(&draws[1]), "the name's run is unblended");
        assert_eq!(chat_text(&draws[2]).0, "§c7", "the number");
        assert_eq!(chat_text(&draws[2]).4, SIDEBAR_TEXT, "its colour kept");
        assert!(!text_blend(&draws[2]), "the number's run is unblended");
        assert_eq!(chat_text(&draws[5]).0, "Demo", "the title");
        assert_eq!(chat_text(&draws[5]).4, SIDEBAR_TEXT, "its colour kept");
        assert!(!text_blend(&draws[5]), "the title's run is unblended");
        // The tab list: the header and footer lines follow their rects; a row's name
        // keeps the blend its row loop enabled.
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA"); 3];
        tab.header = "{\"text\":\"hi\"}".to_owned();
        tab.footer = "{\"text\":\"bye\"}".to_owned();
        let tab_draws = tab_frame(&tab, &Scoreboard::new(), &font);
        assert_eq!(chat_text(&tab_draws[1]).0, "hi§r", "the header line");
        assert!(!text_blend(&tab_draws[1]), "the header line is unblended");
        assert_eq!(chat_text(&tab_draws[19]).0, "bye§r", "the footer line");
        assert!(!text_blend(&tab_draws[19]), "the footer line is unblended");
        let name = tab_draws
            .iter()
            .find(|draw| matches!(draw, HudDraw::Text { text, .. } if text == "AAAA"))
            .expect("the row's name draw");
        assert!(text_blend(name), "the row's name keeps the blend");
        // The chat: the log re-enables blend around its own text (`GuiNewChat.java`:84).
        let mut chat = ChatView::new();
        chat.set_font(chat_font());
        chat.observe("\"A\"", 1, 1_000);
        let line_draws = chat_draws(&mut chat, 1_000);
        assert_eq!(chat_text(&line_draws[1]).0, "A§r");
        assert!(text_blend(&line_draws[1]), "the chat line stays blended");
    }

    #[test]
    fn the_scoreboard_clamps_sixteen_entries_to_the_sources_fifteen() {
        let font = chat_font();
        // The clamp (`GuiIngame.renderScoreboard:563`): past fifteen, the draw
        // list skips the difference, keeping the highest points.
        let board = filtered_board(16, 0);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(draws.len(), 3 * 15 + 3, "fifteen rows and the title band");
        assert_eq!(
            chat_text(&draws[1]).0,
            "e01",
            "the bottom row past the skip"
        );
        assert_eq!(chat_text(&draws[3 * 14 + 1]).0, "e15", "the top row");
        assert!(!sidebar_texts(&draws).iter().any(|text| text == "e00"));
    }

    #[test]
    fn the_scoreboard_filter_quirk_over_skips_by_the_filtered_count() {
        let font = chat_font();
        // The clamp's skip reads the UNFILTERED length — the source evaluates
        // `collection.size()` before the reassignment
        // (`GuiIngame.renderScoreboard:565`) — so a filter that removed `k`
        // entries draws `15 - k` rows.
        let board = filtered_board(18, 2);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(
            draws.len(),
            3 * 13 + 3,
            "eighteen filtered, two # rows: thirteen"
        );
        assert_eq!(chat_text(&draws[1]).0, "e05", "the first row past the skip");
        assert_eq!(chat_text(&draws[3 * 12 + 1]).0, "e17", "the top row");
        assert!(
            !sidebar_texts(&draws)
                .iter()
                .any(|text| text.starts_with('#'))
        );
        let board = filtered_board(18, 3);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(
            draws.len(),
            3 * 12 + 3,
            "eighteen filtered, three # rows: twelve"
        );
        assert_eq!(
            chat_text(&draws[1]).0,
            "e06",
            "the skip stepped with the filtered count"
        );
    }

    #[test]
    fn the_scoreboard_draws_the_same_red_number_for_every_objective_kind() {
        let font = chat_font();
        // The sidebar's number is RED unconditionally
        // (`GuiIngame.renderScoreboard:577`, `:592`); the render kind's branch
        // belongs to the tab list (`GuiPlayerTabOverlay.drawScoreboardValues:278-362`).
        // The wire's `integer` and `hearts` kinds — the source's `dummy` and
        // `health` criteria — draw the same literal.
        for kind in ["integer", "hearts"] {
            let mut board = Scoreboard::new();
            board.set_objective("side", "Side", kind);
            board.set_display(1, Some("side"));
            board.set_score("Alpha", "side", 7);
            let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
            assert_eq!(
                chat_text(&draws[2]),
                ("§c7".to_owned(), 425.0, 114.0, 1.0, SIDEBAR_TEXT, false),
                "the {kind} number"
            );
        }
    }

    #[test]
    fn the_scoreboard_name_carries_its_teams_composition() {
        let font = chat_font();
        // Sidebar names compose through the team clauses like the tab list's
        // (`ScorePlayerTeam.java`:95-106).
        let mut board = Scoreboard::new();
        board.set_team("greens", "Greens", "§a<", "§r", 0, "always", Some(10));
        board.add_team_players("greens", &["Alpha".to_owned()]);
        board.set_objective("side", "Side", "integer");
        board.set_display(1, Some("side"));
        board.set_score("Alpha", "side", 4);
        let draws = sidebar_draws(&board, "Alpha", &font, chat_resolution());
        assert_eq!(chat_text(&draws[1]).0, "§a<Alpha§r");
    }

    #[test]
    fn the_scoreboard_composition_feeds_the_nametag_the_tab_and_the_sidebar() {
        // The three name surfaces read one source — the team clauses around the
        // fallback (`format_entry`, `ScorePlayerTeam.formatString:95-98`): the
        // frame's nametag composition ([`display_name`]), the tab list's row and
        // the sidebar's row.
        let font = chat_font();
        let mut board = Scoreboard::new();
        board.set_team("greens", "Greens", "§a<", "§r", 0, "always", Some(10));
        board.add_team_players("greens", &["AAAA".to_owned()]);
        board.set_objective("side", "Side", "integer");
        board.set_display(1, Some("side"));
        board.set_score("AAAA", "side", 2);
        // The frame's composition, straight through `entity_view`.
        let record = tab_record("AAAA");
        let nametag = display_name(Some(&record), &board).expect("the record resolves");
        assert_eq!(nametag, "§a<AAAA§r", "the team's clauses around the name");
        // The tab list's first row, the same record.
        let mut tab = TabState::new();
        tab.open = true;
        tab.entries = vec![tab_record("AAAA")];
        let tab_draws = tab_frame(&tab, &board, &font);
        assert_eq!(chat_text(&tab_draws[4]).0, nametag, "the tab name");
        // The sidebar's first row.
        let sidebar = sidebar_draws(&board, "AAAA", &font, chat_resolution());
        assert_eq!(chat_text(&sidebar[1]).0, nametag, "the sidebar name");
    }

    // ---- the below-name lines ----

    /// A player frame with the account name set: the name the below-name score
    /// lookup reads.
    fn named_frame(id: i32, name: &str) -> EntityFrame {
        let mut frame = player_frame(id, UUID_SLIM);
        frame.name = Some(Arc::from(name));
        frame
    }

    #[test]
    fn the_scoreboard_below_name_line_composes_the_points_and_the_display_name() {
        // The composed label is `points + " " + display name`
        // (`RenderPlayer.renderOffsetLivingLabel:149`); a missing score is no
        // veto — the lookup creates the zero default
        // (`Scoreboard.getValueFromObjective:96-120`) — and an objective with
        // no display value draws its registry name (`ScoreObjective.java:18`).
        let mut board = Scoreboard::new();
        board.set_objective("below", "Below", "integer");
        board.set_display(2, Some("below"));
        board.set_score("Alpha", "below", 7);
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(vec![named_frame(8, "Alpha")], t0);
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(draws[0].below_name.as_deref(), Some("7 Below"));
        board.remove_score("Alpha", "below");
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(
            draws[0].below_name.as_deref(),
            Some("0 Below"),
            "the missing score draws at the zero default"
        );
        let mut unnamed = Scoreboard::new();
        unnamed.set_objective("below", "", "integer");
        unnamed.set_display(2, Some("below"));
        let draws = view.entity_draws(t0, &BTreeMap::new(), &unnamed);
        assert_eq!(
            draws[0].below_name.as_deref(),
            Some("0 below"),
            "the registry-name default"
        );
    }

    #[test]
    fn the_scoreboard_below_name_skips_mobs_and_sneaking_players() {
        // The line is `RenderPlayer`'s override, so only players carry it; the
        // base renderer's sneaking branch bypasses the override entirely
        // (`RendererLivingEntity.java:507-538`).
        let mut board = Scoreboard::new();
        board.set_objective("below", "Below", "integer");
        board.set_display(2, Some("below"));
        board.set_score("Alpha", "below", 7);
        let mut mob = mob_frame(8, EntityKind::Cow, EntityExtra::Mob(MobExtra::Other));
        mob.name = Some(Arc::from("Alpha"));
        let mut sneaking = named_frame(9, "Alpha");
        sneaking.sneaking = true;
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(vec![mob, sneaking], t0);
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(draws.len(), 2, "both frames draw");
        assert_eq!(draws[0].below_name, None, "the mob carries no line");
        assert_eq!(
            draws[1].below_name, None,
            "the sneaking player carries none"
        );
    }

    #[test]
    fn no_scoreboard_slot_two_objective_gets_no_below_name_line() {
        // No slot-2 objective resolves → no lines, even with scores stored; a
        // slot-2 name the board does not hold resolves none either; and a player
        // the list never named resolves none — the same no-record rule as the
        // nametag (`RenderPlayer.java:148` reads the account name the record
        // alone carries).
        let mut board = Scoreboard::new();
        board.set_objective("below", "Below", "integer");
        board.set_score("Alpha", "below", 7);
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(vec![named_frame(8, "Alpha")], t0);
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(draws[0].below_name, None, "no slot-2 objective, no line");
        board.set_display(2, Some("below"));
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(draws[0].below_name.as_deref(), Some("7 Below"));
        board.set_display(2, Some("ghost"));
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(draws[0].below_name, None, "the dangling name resolves none");
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(vec![player_frame(8, UUID_SLIM)], t0);
        let draws = view.entity_draws(t0, &BTreeMap::new(), &board);
        assert_eq!(draws[0].below_name, None, "no record name, no line");
    }

    /// The stack conversion's NBT default: the root compound's own `ench` list
    /// (`Item.hasEffect`:416-419 over `ItemStack.isItemEnchanted`:902-905) — presence,
    /// not contents; a tag-less stack, a compound without the key, a non-list `ench`
    /// and an unparseable tail are all unenchanted, and the id and damage ride through
    /// untouched.
    #[test]
    fn the_item_icon_reads_the_enchant_rule() {
        let stack = |nbt: Option<Vec<u8>>| MetadataItem {
            id: 276,
            count: 1,
            damage: 3,
            nbt,
        };
        // The root compound with an `ench` list — empty here, and the source's rule
        // counts it (`hasKey("ench", 9)` is presence).
        let enchanted = [
            0x0A, 0x00, 0x00, // the root: a compound, no name
            0x09, 0x00, 0x04, b'e', b'n', b'c', b'h', // a list named `ench`
            0x0A, 0x00, 0x00, 0x00, 0x00, // compounds, none of them
            0x00, // the root ends
        ];
        assert_eq!(
            item_icon(&stack(Some(enchanted.to_vec()))),
            ItemIcon {
                id: 276,
                damage: 3,
                enchanted: true,
            }
        );
        assert!(!item_icon(&stack(None)).enchanted);
        // An empty root compound, no `ench` at all.
        let bare = [0x0A, 0x00, 0x00, 0x00];
        assert!(!item_icon(&stack(Some(bare.to_vec()))).enchanted);
        // An `ench` that is not a list (a string here): the type id must match, 9.
        let string_ench = [
            0x0A, 0x00, 0x00, 0x08, 0x00, 0x04, b'e', b'n', b'c', b'h', 0x00, 0x01, b'x', 0x00,
        ];
        assert!(!item_icon(&stack(Some(string_ench.to_vec()))).enchanted);
        // A tail that does not parse is not enchanted either.
        assert!(!item_icon(&stack(Some(vec![0x0A]))).enchanted);
    }

    /// The class arms of `hasEffect`, which replace the NBT default rather than
    /// extending it: the always-true registrations — the enchanted book 403
    /// (`ItemEnchantedBook`:16-19), the written book 387 (`ItemEditableBook`:146-149),
    /// the bottle o' enchanting 384 (`ItemExpBottle`:16-19) and the nether star 399
    /// (`ItemSimpleFoiled`:5-8, the class's own registration at `Item.java`:909) —
    /// glint with no NBT at all, the golden apple 322 is `stack.getMetadata() > 0`
    /// (`ItemAppleGold`:18-21) and the potion 373 is its effect list non-empty
    /// (`ItemPotion`:330-334). The ids are `Item.registerItems`' own literals
    /// (`Item.java`:831, :883, :894, :897, :909, :913).
    #[test]
    fn the_item_icon_reads_the_class_arms() {
        let stack = |id: i16, damage: i16, nbt: Option<Vec<u8>>| MetadataItem {
            id,
            count: 1,
            damage,
            nbt,
        };
        for id in [403, 387, 384, 399] {
            let icon = item_icon(&stack(id, 0, None));
            assert!(
                icon.enchanted,
                "the always-foiled registration {id} glints without NBT: {icon:?}"
            );
        }
        // The golden apple: metadata 0 the plain apple, anything above it the enchanted
        // one — the arm is `> 0`, not the sub-item pair's membership.
        let plain = item_icon(&stack(322, 0, None));
        assert!(
            !plain.enchanted,
            "the plain golden apple does not glint: {plain:?}"
        );
        let golden = item_icon(&stack(322, 1, None));
        assert!(
            golden.enchanted,
            "the enchanted golden apple glints: {golden:?}"
        );
        let golden = item_icon(&stack(322, 2, None));
        assert!(
            golden.enchanted,
            "the arm is `metadata > 0`, so damage 2 glints too: {golden:?}"
        );
        // An override replaces the default: the enchanted book glints with or without
        // the tag, while a water potion and a plain golden apple carrying `ench` do not.
        let ench = [
            0x0A, 0x00, 0x00, 0x09, 0x00, 0x04, b'e', b'n', b'c', b'h', 0x0A, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let book = item_icon(&stack(403, 0, Some(ench.to_vec())));
        assert!(
            book.enchanted,
            "the enchanted book glints with or without the tag: {book:?}"
        );
        let water = item_icon(&stack(373, 0, Some(ench.to_vec())));
        assert!(
            !water.enchanted,
            "the water potion's tag does not survive its arm: {water:?}"
        );
        let plain = item_icon(&stack(322, 0, Some(ench.to_vec())));
        assert!(
            !plain.enchanted,
            "the plain golden apple's tag does not survive its arm: {plain:?}"
        );
        // The default arm for any other id: the NBT rule still answers.
        let other = item_icon(&stack(276, 0, Some(ench.to_vec())));
        assert!(other.enchanted, "the default arm reads the tag: {other:?}");
        let other = item_icon(&stack(276, 0, None));
        assert!(!other.enchanted, "the tag-less default does not: {other:?}");
        // A builtin-shaped stack's flag comes from this rule too — the chest's own block
        // item has no override, so its `ench` tag sets the flag; the draw-side gate (a
        // builtin shape never glints) is the pass's own.
        let chest = item_icon(&stack(54, 0, Some(ench.to_vec())));
        assert!(chest.enchanted, "the chest's tag sets the flag: {chest:?}");
    }

    /// The equipment conversion: each slot's stack becomes the draw's reduced view
    /// through the same seam the icon takes — the id and damage, the enchant flag, the
    /// raw `display.color` int the leather dye reads (`ItemArmor.hasColor`:127-130's
    /// compound fold) and the cross flag the held item's block branch reads
    /// (`LayerHeldItem.java`:52's render-type-2 test through the behaviour table's own
    /// cross kind). Empty slots convert to nothing; a tail that is not a compound, or
    /// carries no `display`, carries no colour.
    #[test]
    fn the_equipment_converts_through_the_item_icon_seam() {
        // A leather chestplate (299) with a red dye: the root compound's `display`
        // child's `color` int, 0xFF0000.
        let dyed = [
            0x0A, 0x00, 0x00, // the root: a compound, no name
            0x0A, 0x00, 0x07, b'd', b'i', b's', b'p', b'l', b'a',
            b'y', // a compound `display`
            0x03, 0x00, 0x05, b'c', b'o', b'l', b'o', b'r', // an int `color`
            0x00, 0xFF, 0x00, 0x00, // 0xFF0000
            0x00, // the display ends
            0x00, // the root ends
        ];
        let slots: [Option<MetadataItem>; 5] = std::array::from_fn(|slot| {
            (slot == 3).then(|| MetadataItem {
                id: 299,
                count: 1,
                damage: 0,
                nbt: Some(dyed.to_vec()),
            })
        });
        let converted = equipment_draw(&slots);
        assert_eq!(
            converted[3],
            Some(EquipmentDraw {
                id: 299,
                damage: 0,
                enchanted: false,
                colour: Some(0xFF0000),
                cross: false,
            })
        );
        assert!(
            converted[..3].iter().all(Option::is_none) && converted[4].is_none(),
            "empty slots convert to nothing"
        );
        // A `display` without a `color`, and a root that is not a compound: no colour.
        let bare_display = [
            0x0A, 0x00, 0x00, 0x0A, 0x00, 0x07, b'd', b'i', b's', b'p', b'l', b'a', b'y', 0x00,
            0x00,
        ];
        let stack = MetadataItem {
            id: 299,
            count: 1,
            damage: 0,
            nbt: Some(bare_display.to_vec()),
        };
        assert_eq!(equipment_stack(&stack).colour, None);
        let not_compound = MetadataItem {
            id: 299,
            count: 1,
            damage: 0,
            nbt: Some(vec![0x03, 0x00, 0x01, b'x', 0x00, 0x00, 0x00, 0x05]),
        };
        assert_eq!(equipment_stack(&not_compound).colour, None);

        // The cross flag: tall grass (31) is the cross family, stone (1) is not, and a
        // non-block item never is (`ItemBlock`'s own gate).
        let grass = MetadataItem {
            id: 31,
            count: 1,
            damage: 0,
            nbt: None,
        };
        assert!(
            equipment_stack(&grass).cross,
            "tall grass is the cross family"
        );
        let stone = MetadataItem {
            id: 1,
            count: 1,
            damage: 0,
            nbt: None,
        };
        assert!(!equipment_stack(&stone).cross);
        let sword = MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        };
        assert!(!equipment_stack(&sword).cross);
    }

    /// The equipment and the extras ride the draw: a held stack lifts the player's
    /// `held` pose flag (`RenderPlayer.setModelVisibilities`:93-97's non-null gate), the
    /// creeper's charge (byte 17) and the wither's armour flag ride the draw's extra,
    /// and the five slots convert onto the draw's own array.
    #[test]
    fn the_equipment_and_extras_ride_the_draw() {
        let mut view = View::new();
        let t0 = Instant::now();
        let mut frame = player_frame(1, UUID_WIDE);
        frame.equipment[0] = Some(MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        });
        view.observe(vec![frame.clone()], t0);
        let draws = draws_at(&view, t0, Duration::ZERO);
        let draw = &draws[0];
        assert_eq!(draw.id, 1, "the draw carries the frame's id");
        assert!(
            matches!(
                draw.pose.extra,
                PoseExtra::Player(PlayerExtra { held: true, .. })
            ),
            "a held stack lifts the flag: {:?}",
            draw.pose.extra
        );
        assert_eq!(
            draw.equipment[0].map(|stack| stack.id),
            Some(276),
            "the held slot converts onto the draw"
        );
        // An empty hand leaves the flag off.
        frame.equipment[0] = None;
        view.observe(vec![frame], t0);
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert!(matches!(
            draws[0].pose.extra,
            PoseExtra::Player(PlayerExtra { held: false, .. })
        ));

        // The creeper's charge and the wither's armour ride the draw's extra.
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(
            vec![
                mob_frame(
                    1,
                    EntityKind::Creeper,
                    EntityExtra::Mob(MobExtra::Creeper { powered: true }),
                ),
                mob_frame(
                    2,
                    EntityKind::WitherBoss,
                    EntityExtra::Mob(MobExtra::Wither {
                        invul_time: 0,
                        armored: true,
                    }),
                ),
            ],
            t0,
        );
        let draws = draws_at(&view, t0, Duration::ZERO);
        assert_eq!(draws[0].extra, DrawExtra::Creeper { powered: true });
        assert_eq!(draws[1].extra, DrawExtra::Wither { armored: true });
    }

    /// The potion arm's own decode: `ItemPotion.hasEffect`:330-334 is its effect list
    /// non-empty (`getEffects`:40-70), which for a stack without a
    /// `CustomPotionEffects` tag is the damage's `PotionHelper.getPotionEffects`
    /// answer (`:386-449` over the thirteen `potionRequirements` strings, `:581-593`).
    /// The T9 sub-item data carries the source's own population: the water bottle at
    /// damage 0 and the 62 effect-carrying damages, every one of which glints; the
    /// patterns 0, 7 and 15, and the splash bit alone, match no requirement.
    #[test]
    fn the_item_icon_decodes_the_potion_damages() {
        let stack = |damage: i16| MetadataItem {
            id: 373,
            count: 1,
            damage,
            nbt: None,
        };
        let items = oxide_client::items::sub_items(373);
        assert_eq!(items.len(), 63, "the water bottle plus the 62 potions");
        for item in items {
            let icon = item_icon(&stack(item.damage));
            if item.damage == 0 {
                assert!(!icon.enchanted, "the water bottle does not glint: {icon:?}");
            } else {
                assert!(
                    icon.enchanted,
                    "{} at damage {} carries effects and glints: {icon:?}",
                    item.name, item.damage
                );
            }
        }
        for damage in [0, 7, 15, 16384] {
            let icon = item_icon(&stack(damage));
            assert!(
                !icon.enchanted,
                "damage {damage} matches none of the source's requirements: {icon:?}"
            );
        }
    }

    /// A tick event for the held item's own state machine: only the tick's own number
    /// reaches it.
    fn held_tick(tick: u64) -> ClientEvent {
        held_tick_at(tick, 0.0, 0.0)
    }

    /// A tick whose camera has turned: the same event with the given pitch and yaw.
    fn held_tick_at(tick: u64, pitch: f32, yaw: f32) -> ClientEvent {
        ClientEvent::PlayerTick {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw,
            pitch,
            on_ground: true,
            sprinting: false,
            sneaking: false,
            flying: false,
            in_water: false,
            tick,
            snapped: false,
            hurt_time: 0,
            attacked_at_yaw: 0.0,
        }
    }

    /// A window 0 snapshot whose hotbar carries the given stacks: the band at 36–44 of
    /// the forty-five-slot layout.
    fn held_snapshot(stacks: [Option<MetadataItem>; 9]) -> ClientEvent {
        held_snapshot_pop(stacks, [0; 9])
    }

    /// The Task 14 hotbar assembly's own snapshot: the nine stacks plus the
    /// session's per-slot pop counters (`animationsToGo`, `GuiIngame.java`:1043).
    fn held_snapshot_pop(stacks: [Option<MetadataItem>; 9], pop: [u8; 9]) -> ClientEvent {
        let mut slots = vec![None; 36];
        slots.extend(stacks);
        ClientEvent::WindowSnapshot {
            window_id: 0,
            slots,
            cursor: None,
            properties: Vec::new(),
            hotbar_pop: pop,
        }
    }

    /// The Task 14 assembly's inputs at the chat suite's resolution, the font
    /// optional so the no-font row reads.
    fn hotbar_input(font: Option<&Font>) -> HotbarInput<'_> {
        HotbarInput {
            font,
            scaled: chat_resolution(),
            show_crosshair: true,
            hide_gui: false,
            screen_open: false,
            survival: true,
        }
    }

    /// The hotbar draws at `elapsed` past the observed arrival, against the
    /// Task 14 inputs.
    fn hotbar_at(
        view: &View,
        arrival: Instant,
        elapsed: Duration,
        font: Option<&Font>,
    ) -> Vec<HudDraw> {
        view.hotbar_draws(arrival + elapsed, &hotbar_input(font))
    }

    /// One hotbar stack: the id, the count and the damage the overlay reads.
    fn hotbar_stack(id: i16, count: u8, damage: i16) -> Option<MetadataItem> {
        Some(MetadataItem {
            id,
            count,
            damage,
            nbt: None,
        })
    }

    /// The popup's swap rule (`GuiIngame.updateTick`:1089-1109): a changed stack
    /// resets to 40, an identical one counts down, and an empty hand clears.
    #[test]
    fn the_popup_resets_to_forty_on_a_stack_swap() {
        let sword = MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        };
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = Some(sword.clone());
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        assert_eq!(view.held.popup_ticks, 0, "nothing selected yet");
        view.apply(&held_tick(1));
        assert_eq!(view.held.popup_ticks, 40, "the swap resets the clock");
        view.apply(&held_tick(2));
        assert_eq!(
            view.held.popup_ticks, 39,
            "the identical stack counts down, not resets"
        );
        // A different id swaps again.
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = Some(MetadataItem {
            id: 3,
            count: 1,
            damage: 0,
            nbt: None,
        });
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(3));
        assert_eq!(view.held.popup_ticks, 40, "a new id resets the clock");
        // An empty hand clears it outright (`itemstack == null` → 0).
        let empty: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        view.apply(&held_snapshot(empty));
        view.apply(&held_tick(4));
        assert_eq!(view.held.popup_ticks, 0, "no stack, no popup");
        assert_eq!(view.held.popup_stack, None);
    }

    /// The damageable short-circuit (`GuiIngame.java`:1097): a damageable
    /// stack's damage is not compared, so a damaged sword counts down; a
    /// non-damageable stack's metadata is, so a recoloured wool resets.
    #[test]
    fn the_popup_ignores_damage_on_damageable_stacks_only() {
        let mut view = View::new();
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(276, 1, 0);
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(1));
        assert_eq!(view.held.popup_ticks, 40);
        // The same sword, damaged: no reset.
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(276, 1, 5);
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(2));
        assert_eq!(
            view.held.popup_ticks, 39,
            "a damageable stack's damage change does not reset"
        );
        // Wool is not damageable: the same id at another metadata resets.
        let mut view = View::new();
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(35, 1, 0);
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(1));
        assert_eq!(view.held.popup_ticks, 40);
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(35, 1, 1);
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(2));
        assert_eq!(
            view.held.popup_ticks, 40,
            "a non-damageable stack's metadata change resets"
        );
        // The same wool, untouched, counts down.
        view.apply(&held_tick(3));
        assert_eq!(view.held.popup_ticks, 39);
    }

    /// The tag arm of the swap comparison (`ItemStack.areItemStackTagsEqual`,
    /// `ItemStack.java`:426-429): a tag change resets even when the id and the
    /// damage match, and an `Unbreakable` tag makes a sword non-damageable so
    /// its damage compares again.
    #[test]
    fn the_popup_compares_the_nbt_tags() {
        let bare = [0x0Au8, 0x00, 0x00, 0x00];
        let named = [
            0x0Au8, 0x00, 0x00, // the root, no name
            0x0A, 0x00, 0x07, b'd', b'i', b's', b'p', b'l', b'a', b'y', // `display`
            0x08, 0x00, 0x04, b'N', b'a', b'm', b'e', 0x00, 0x05, b'S', b'w', b'o', b'r',
            b'd', // `Name` = "Sword"
            0x00, // the display compound ends
            0x00, // the root ends
        ];
        let mut view = View::new();
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = Some(MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: Some(bare.to_vec()),
        });
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(1));
        assert_eq!(view.held.popup_ticks, 40);
        // The same sword with a name tag: the tags differ, so it resets.
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = Some(MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: Some(named.to_vec()),
        });
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(2));
        assert_eq!(
            view.held.popup_ticks, 40,
            "a tag change resets like any other swap"
        );
        // The named stack draws under its NBT name, italicised
        // (`hasDisplayName` → `ITALIC + s`, `GuiIngame.java`:460-463).
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        let popup = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .expect("the popup draws");
        assert_eq!(popup, "§oSword", "the NBT name, italicised");
    }

    /// The 40-tick countdown: the reset lands on 40 and same-stack ticks walk
    /// it to zero, where it stays (`updateTick`:1095-1099).
    #[test]
    fn the_popup_counts_down_forty_ticks() {
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(276, 1, 0);
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(1));
        for tick in 2..=41 {
            view.apply(&held_tick(tick));
        }
        assert_eq!(view.held.popup_ticks, 0, "forty ticks run the clock out");
        view.apply(&held_tick(42));
        assert_eq!(view.held.popup_ticks, 0, "zero holds while the stack stays");
    }

    /// The popup's alpha curve (`GuiIngame.java`:473-485): `k = ticks × 256 /
    /// 10`, clamped to 255 — 40 and 10 both saturate, 9, 2 and 1 do not.
    #[test]
    fn the_popup_alpha_follows_the_sources_curve() {
        assert_eq!(popup_alpha(40), 255, "1024 clamps to 255");
        assert_eq!(popup_alpha(10), 255, "256 clamps to 255");
        assert_eq!(popup_alpha(9), 230);
        assert_eq!(popup_alpha(2), 51);
        assert_eq!(popup_alpha(1), 25);
        assert_eq!(popup_alpha(0), 0);
    }

    /// The pop's fraction (`GuiIngame.renderHotbarItem`:1043-1051): `f =
    /// animationsToGo − partial`, and while positive `f1 = 1 + f/5` scales
    /// `(1/f1, (f1+1)/2)`; at zero or below the item draws unscaled.
    #[test]
    fn the_pop_fraction_follows_the_sources_curve() {
        let (sx, sy) = pop_factors(5.0).expect("a live pop scales");
        assert!((sx - 0.5).abs() < 1e-6, "1/2 at five ticks: {sx}");
        assert!((sy - 1.5).abs() < 1e-6, "(2+1)/2 at five ticks: {sy}");
        let (sx, sy) = pop_factors(2.5).expect("a mid pop scales");
        assert!((sx - 2.0 / 3.0).abs() < 1e-6, "1/1.5: {sx}");
        assert!((sy - 1.25).abs() < 1e-6, "2.5/2: {sy}");
        assert_eq!(pop_factors(0.0), None, "spent pops draw unscaled");
        assert_eq!(pop_factors(-0.25), None, "overrun partials draw unscaled");
    }

    /// The pop the slot item carries: the snapshot's counter minus the frame's
    /// fraction, floored at zero — the draw at a zero fraction carries the
    /// whole counter, mid-frame it carries the remainder.
    #[test]
    fn the_slot_item_carries_the_pop_minus_the_partial() {
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(3, 1, 0);
        let mut view = View::new();
        view.apply(&held_snapshot_pop(slots, [5, 0, 0, 0, 0, 0, 0, 0, 0]));
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        let item = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Item { x, y, pop, .. } => Some((*x, *y, *pop)),
                _ => None,
            })
            .expect("slot 0 draws its item");
        assert_eq!(item, (125.0, 221.0, 5.0), "the cell and the whole counter");
        let draws = hotbar_at(&view, t0, Duration::from_millis(25), Some(&font));
        let pop = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Item { pop, .. } => Some(*pop),
                _ => None,
            })
            .expect("slot 0 draws its item");
        assert!(
            (pop - 4.5).abs() < 1e-6,
            "the counter minus the half-tick fraction: {pop}"
        );
    }

    /// The HUD-visibility rule is the call site's gate
    /// (`EntityRenderer.java`:1166-1169): the overlay draws unless F1 hides it
    /// with no screen open — a screen never suppresses an entry.
    #[test]
    fn the_hud_visibility_rule_is_the_call_sites_gate() {
        assert!(hud_visible(false, false), "plain play draws");
        assert!(
            hud_visible(false, true),
            "a screen draws over the HUD, not instead of it"
        );
        assert!(hud_visible(true, true), "F1 with a screen open still draws");
        assert!(!hud_visible(true, false), "F1 alone hides the overlay");
        // The assembly obeys it: nothing draws under F1 alone, everything does
        // with a screen open.
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(3, 1, 0);
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let hidden = HotbarInput {
            hide_gui: true,
            ..hotbar_input(Some(&font))
        };
        assert!(
            view.hotbar_draws(t0, &hidden).is_empty(),
            "F1 alone draws no hotbar entry"
        );
        let screened = HotbarInput {
            hide_gui: true,
            screen_open: true,
            ..hotbar_input(Some(&font))
        };
        assert!(
            !view.hotbar_draws(t0, &screened).is_empty(),
            "a screen keeps every entry"
        );
    }

    /// The hotbar's geometry at the chat suite's 427x240 resolution
    /// (`GuiIngame.renderTooltip`:372-387): the 182x22 background slice
    /// `(0, 0, 182, 22)` at `(213 − 91, 240 − 22)`, and with slot 4 selected
    /// the 24x22 highlight `(0, 22, 24, 22)` at `(213 − 92 + 4 × 20, 240 − 23)`.
    #[test]
    fn the_hotbar_geometry_pins_the_sources_slices() {
        let mut view = View::new();
        view.apply(&held_snapshot(std::array::from_fn(|_| None)));
        view.apply(&ClientEvent::HeldItemSlot { slot: 4 });
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        assert_eq!(
            draws[0],
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/widgets"),
                x: 122.0,
                y: 218.0,
                width: 182.0,
                height: 22.0,
                uv: [0.0, 0.0, 182.0 / 256.0, 22.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            "the background slice"
        );
        assert_eq!(
            draws[1],
            HudDraw::TexturedRect {
                texture: HudTexture::Named("gui/widgets"),
                x: 201.0,
                y: 217.0,
                width: 24.0,
                height: 22.0,
                uv: [0.0, 22.0 / 256.0, 24.0 / 256.0, 44.0 / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            "the highlight shifted to slot 4"
        );
    }

    /// The slot overlay (`RenderItem.renderItemOverlayIntoGUI`:455-497): a
    /// 16-stack counts (`stackSize != 1`, right-aligned at `x + 17 − width`,
    /// `y + 9`, unblended with shadow), and a damaged sword bars
    /// (`isItemDamaged`, `j = round(13 − 780 × 13/1561) = 7`, `i =
    /// round(255 − 780 × 255/1561) = 128`): black 13x2, `(31, 64, 0)` 12x1 and
    /// `(127, 128, 0)` 7x1 at `(x + 2, y + 13)` — while a lone sword counts
    /// nothing.
    #[test]
    fn the_slot_overlay_pins_the_count_and_the_ramp() {
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(3, 16, 0);
        slots[1] = hotbar_stack(276, 1, 780);
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        // Slot 0's cell: the item at (125, 221), the count right-aligned at
        // (125 + 17 − width("16"), 221 + 9), unblended and shadowed.
        let width = string_width(&font, "16") as f32;
        let count = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Text { text, x, y, .. } if text == "16" => Some((*x, *y)),
                _ => None,
            })
            .expect("the 16-stack counts");
        assert_eq!(count, (125.0 + 17.0 - width, 230.0));
        let count_draw = draws
            .iter()
            .find(|draw| matches!(draw, HudDraw::Text { text, .. } if text == "16"))
            .expect("the 16-stack counts");
        match count_draw {
            HudDraw::Text {
                colour,
                shadow,
                blend,
                ..
            } => {
                assert_eq!(*colour, [1.0, 1.0, 1.0, 1.0], "opaque white");
                assert!(*shadow, "the count draws with shadow");
                assert!(!*blend, "the count draws unblended");
            }
            other => panic!("a text draw: {other:?}"),
        }
        // Slot 1's cell at x = 145: the three durability rects at (147, 234).
        let rects: Vec<(f32, f32, f32, f32, [f32; 4])> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Rect {
                    x,
                    y,
                    width,
                    height,
                    colour,
                } if *x >= 147.0 && *x < 160.0 => Some((*x, *y, *width, *height, *colour)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rects,
            vec![
                (147.0, 234.0, 13.0, 2.0, [0.0, 0.0, 0.0, 1.0]),
                (
                    147.0,
                    234.0,
                    12.0,
                    1.0,
                    [31.0 / 255.0, 64.0 / 255.0, 0.0, 1.0]
                ),
                (
                    147.0,
                    234.0,
                    7.0,
                    1.0,
                    [127.0 / 255.0, 128.0 / 255.0, 0.0, 1.0]
                ),
            ],
            "the black bed, the underlay and the ramp fill"
        );
        // The lone sword counts nothing: no other count text draws.
        let counts = draws
            .iter()
            .filter(|draw| matches!(draw, HudDraw::Text { text, .. } if text == "1"))
            .count();
        assert_eq!(counts, 0, "a lone stack draws no count");
    }

    /// The popup's draw (`GuiIngame.renderSelectedItem`:452-492): the
    /// selected sword's name centred at `y = scaledH − 59` (one tick after the
    /// swap, alpha 255), fading to 230 after thirty-one ticks, and sitting
    /// fourteen lower outside survival (`:468-471`).
    #[test]
    fn the_popup_text_pins_position_and_alpha() {
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = hotbar_stack(276, 1, 0);
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        view.apply(&held_tick(1));
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        let width = string_width(&font, "Diamond Sword") as f32;
        let popup = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Text {
                    text, x, y, colour, ..
                } if text == "Diamond Sword" => Some((*x, *y, *colour)),
                _ => None,
            })
            .expect("the popup names the held sword");
        assert_eq!(
            popup,
            ((427.0 - width) / 2.0, 181.0, [1.0, 1.0, 1.0, 1.0]),
            "centred, above the hotbar, fully opaque at forty ticks"
        );
        // Thirty-one ticks on: nine remain, alpha 230.
        for tick in 2..=32 {
            view.apply(&held_tick(tick));
        }
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        let alpha = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Text { text, colour, .. } if text == "Diamond Sword" => Some(colour[3]),
                _ => None,
            })
            .expect("the popup still draws at nine ticks");
        assert!(
            (alpha - 230.0 / 255.0).abs() < 1e-6,
            "the faded alpha: {alpha}"
        );
        // Outside survival the line sits fourteen lower.
        let creative = HotbarInput {
            survival: false,
            ..hotbar_input(Some(&font))
        };
        let draws = view.hotbar_draws(t0, &creative);
        let y = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::Text { text, y, .. } if text == "Diamond Sword" => Some(*y),
                _ => None,
            })
            .expect("the popup draws outside survival");
        assert_eq!(y, 195.0, "scaledH − 59 + 14");
    }

    /// The crosshair (`GuiIngame.java`:175-180): one 16x16 `gui/icons` quad at
    /// `(213 − 7, 120 − 7)`, drawn exactly while `showCrosshair` says so.
    #[test]
    fn the_crosshair_pins_the_sources_quad() {
        let mut view = View::new();
        view.apply(&held_snapshot(std::array::from_fn(|_| None)));
        let font = chat_font();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        let draws = hotbar_at(&view, t0, Duration::ZERO, Some(&font));
        assert!(
            draws.contains(&HudDraw::InvertRect {
                x: 206.0,
                y: 113.0,
                w: 16.0,
                h: 16.0,
            }),
            "the inverting quad at the centre: {draws:?}"
        );
        let hidden = HotbarInput {
            show_crosshair: false,
            ..hotbar_input(Some(&font))
        };
        assert!(
            !view
                .hotbar_draws(t0, &hidden)
                .iter()
                .any(|draw| matches!(draw, HudDraw::InvertRect { .. })),
            "no crosshair while the gate says hide"
        );
    }

    // ---- the stat rows (Task 15) ----

    /// One window-0 snapshot carrying the armour band: slots 5-8 hold the
    /// pieces, everything else stays empty (`ContainerPlayer.java`:36-54).
    fn rows_snapshot(ids: [Option<i16>; 4]) -> ClientEvent {
        let mut slots: Vec<Option<MetadataItem>> = vec![None; 45];
        for (band, id) in ids.iter().enumerate() {
            slots[5 + band] = id.map(|id| MetadataItem {
                id,
                count: 1,
                damage: 0,
                nbt: None,
            });
        }
        ClientEvent::WindowSnapshot {
            window_id: 0,
            slots,
            cursor: None,
            properties: Vec::new(),
            hotbar_pop: [0; 9],
        }
    }

    /// One player tick naming the rows' clock, water and hurt state.
    fn rows_tick(tick: u64, in_water: bool, hurt_time: u32) -> ClientEvent {
        ClientEvent::PlayerTick {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            sprinting: false,
            sneaking: false,
            flying: false,
            in_water,
            tick,
            snapped: false,
            hurt_time,
            attacked_at_yaw: 0.0,
        }
    }

    /// The health feed: the packet's own triple (`Health`, clientbound 0x06).
    fn rows_health(health: f32, food: i32, saturation: f32) -> ClientEvent {
        ClientEvent::Health {
            health,
            food,
            saturation,
        }
    }

    /// The effects feed holding exactly the named ids.
    fn rows_effects(ids: &[u8]) -> ClientEvent {
        ClientEvent::Effects {
            effects: ids
                .iter()
                .map(|id| oxide_game::session::StatusEffect {
                    effect_id: *id,
                    amplifier: 0,
                    duration: 200,
                })
                .collect(),
        }
    }

    /// The rows' inputs at the chat suite's resolution, the font optional, the
    /// wall clock explicit so the blink's settle rule stays deterministic.
    fn rows_input(font: Option<&Font>, now_ms: u64) -> RowsInput<'_> {
        RowsInput {
            font,
            scaled: chat_resolution(),
            survival: true,
            hide_gui: false,
            screen_open: false,
            now_ms,
        }
    }

    /// The rows' draws for a view fed exactly as the arguments say: the tick
    /// first (the clock, water and hurt), then health, effects, air,
    /// absorption and experience.
    #[allow(clippy::too_many_arguments)]
    fn rows_draws(
        view: &mut View,
        font: Option<&Font>,
        now_ms: u64,
        tick: u64,
        in_water: bool,
        hurt_time: u32,
        health: f32,
        food: i32,
        saturation: f32,
        effect_ids: &[u8],
        air: i16,
        absorption: f32,
        bar: f32,
        level: i32,
    ) -> Vec<HudDraw> {
        view.apply(&rows_tick(tick, in_water, hurt_time));
        view.apply(&rows_health(health, food, saturation));
        view.apply(&rows_effects(effect_ids));
        view.apply(&ClientEvent::Air { air });
        view.apply(&ClientEvent::Absorption { amount: absorption });
        view.apply(&ClientEvent::Experience {
            bar,
            level,
            total: 0,
        });
        view.stat_rows_draws(&rows_input(font, now_ms))
    }

    /// A healthy, dry, unhurt frame's rows: health and food full, no effects,
    /// full air, no absorption, no experience.
    fn rows_healthy(view: &mut View, font: Option<&Font>, now_ms: u64) -> Vec<HudDraw> {
        rows_draws(
            view,
            font,
            now_ms,
            0,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        )
    }

    /// The `(x, y, uv)` of every icons-slice draw, in list order.
    fn rows_slices(draws: &[HudDraw]) -> Vec<(f32, f32, [f32; 4])> {
        draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::TexturedRect { x, y, uv, .. } => Some((*x, *y, *uv)),
                _ => None,
            })
            .collect()
    }

    /// The `(text, x, y, colour, shadow)` of every text draw, in list order.
    fn rows_texts(draws: &[HudDraw]) -> Vec<(String, f32, f32, [f32; 4], bool)> {
        draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text {
                    text,
                    x,
                    y,
                    colour,
                    shadow,
                    ..
                } => Some((text.clone(), *x, *y, *colour, *shadow)),
                _ => None,
            })
            .collect()
    }

    /// The ceil pair is the source's own truncating cast plus one when
    /// fractional (`MathHelper.ceiling_float_int`/`ceiling_double_int`,
    /// `MathHelper.java`:106-116).
    #[test]
    fn the_rows_ceil_rules_follow_the_sources_casts() {
        assert_eq!(ceil_float_int(20.0), 20);
        assert_eq!(ceil_float_int(17.5), 18);
        assert_eq!(ceil_float_int(0.1), 1);
        assert_eq!(ceil_float_int(0.0), 0);
        assert_eq!(ceil_double_int(10.0), 10);
        assert_eq!(ceil_double_int(9.0667), 10);
        assert_eq!(ceil_double_int(0.0334), 1);
        assert_eq!(
            ceil_double_int(-0.0334),
            0,
            "a negative fraction ceils to zero"
        );
        assert_eq!(ceil_double_int(0.0), 0);
    }

    /// The rows' geometry at the chat suite's 427x240 resolution: the hearts
    /// row at `scaledW/2 − 91` (`GuiIngame.java`:643), hearts and hunger at
    /// `scaledH − 39` (:645), armour and air ten above (:650), the bar at
    /// `scaledH − 29` (:425) and the level at `scaledH − 35` (:441).
    #[test]
    fn the_rows_geometry_pins_the_sources_rows() {
        let mut view = View::new();
        view.apply(&rows_snapshot([None, None, None, None]));
        let font = chat_font();
        let draws = rows_healthy(&mut view, Some(&font), 5_000);
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(122.0, 201.0, [16.0 / 256.0, 0.0, 25.0 / 256.0, 9.0 / 256.0])),
            "the first heart's container at (122, 201): {slices:?}"
        );
        assert!(
            slices.contains(&(
                295.0,
                201.0,
                [16.0 / 256.0, 27.0 / 256.0, 25.0 / 256.0, 36.0 / 256.0]
            )),
            "the first food cell at j1 − 9 = 295, same row: {slices:?}"
        );
        assert!(
            slices.contains(&(
                122.0,
                211.0,
                [0.0, 64.0 / 256.0, 182.0 / 256.0, 69.0 / 256.0]
            )),
            "the experience background at (122, 211): {slices:?}"
        );
    }

    /// The heart variant picker (`GuiIngame.java`:689-698, :756-767): the
    /// ceil'd health picks full (`j6 + 36`) below and half (`j6 + 45) at the
    /// boundary over the always-drawn container, with the base at 16, 52
    /// under poison and 88 under wither — poison winning over wither.
    #[test]
    fn the_rows_heart_variants_pin_the_slice_windows() {
        let font = chat_font();
        // Full health: ten containers each carrying a full heart.
        let mut view = View::new();
        let draws = rows_healthy(&mut view, Some(&font), 5_000);
        let hearts: Vec<(f32, f32, [f32; 4])> = rows_slices(&draws)
            .into_iter()
            .filter(|(_, y, _)| *y == 201.0)
            .filter(|(x, _, _)| *x < 200.0)
            .collect();
        assert_eq!(
            hearts.len(),
            20,
            "ten containers plus ten full hearts: {hearts:?}"
        );
        for cell in 0..10 {
            let x = 122.0 + cell as f32 * 8.0;
            assert!(
                hearts.contains(&(x, 201.0, [16.0 / 256.0, 0.0, 25.0 / 256.0, 9.0 / 256.0])),
                "cell {cell}'s container: {hearts:?}"
            );
            assert!(
                hearts.contains(&(x, 201.0, [52.0 / 256.0, 0.0, 61.0 / 256.0, 9.0 / 256.0])),
                "cell {cell}'s full heart: {hearts:?}"
            );
        }
        // Nineteen health: nine full, the last cell a half.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            false,
            0,
            19.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let hearts: Vec<(f32, f32, [f32; 4])> = rows_slices(&draws)
            .into_iter()
            .filter(|(_, y, _)| *y == 201.0)
            .filter(|(x, _, _)| *x < 200.0)
            .collect();
        assert_eq!(
            hearts.len(),
            20,
            "ten containers, nine full, one half: {hearts:?}"
        );
        assert!(
            hearts.contains(&(194.0, 201.0, [61.0 / 256.0, 0.0, 70.0 / 256.0, 9.0 / 256.0])),
            "the last cell's half heart: {hearts:?}"
        );
        // Two health: one full heart over the first container, nine bare ones.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            0,
            false,
            0,
            2.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let fulls = rows_slices(&draws)
            .into_iter()
            .filter(|(_, _, uv)| *uv == [52.0 / 256.0, 0.0, 61.0 / 256.0, 9.0 / 256.0])
            .count();
        assert_eq!(fulls, 1, "a single full heart at two health: {draws:?}");
        // Zero health: ten bare containers, no heart slice at all.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            0,
            false,
            0,
            0.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        assert!(
            rows_slices(&draws)
                .iter()
                .filter(|(x, _, _)| *x >= 122.0 && *x <= 194.0)
                .all(|(_, _, uv)| uv[0] < 50.0 / 256.0),
            "no full or half slice draws at zero health: {draws:?}"
        );
        // Poison shifts the pair to 88/97, wither to 124/133, poison winning.
        for (effects, full, half, what) in [
            (&[19u8][..], 88.0, 97.0, "poison"),
            (&[20u8][..], 124.0, 133.0, "wither"),
            (&[19u8, 20u8][..], 88.0, 97.0, "poison over wither"),
        ] {
            let mut view = View::new();
            let draws = rows_draws(
                &mut view,
                Some(&font),
                5_000,
                0,
                false,
                0,
                19.0,
                20,
                5.0,
                effects,
                300,
                0.0,
                0.0,
                0,
            );
            let slices = rows_slices(&draws);
            assert!(
                slices.contains(&(
                    122.0,
                    201.0,
                    [full / 256.0, 0.0, (full + 9.0) / 256.0, 9.0 / 256.0]
                )),
                "{what}'s full heart: {slices:?}"
            );
            assert!(
                slices.contains(&(
                    194.0,
                    201.0,
                    [half / 256.0, 0.0, (half + 9.0) / 256.0, 9.0 / 256.0]
                )),
                "{what}'s half heart: {slices:?}"
            );
        }
    }

    /// The blink state machine (`GuiIngame.java`:615-636): the 20-tick raise
    /// on a hurt loss and the 10-tick raise on a hurt gain, the 3-tick flip
    /// and the one-second settle — with the raises gated by the hurt window.
    #[test]
    fn the_rows_blink_state_follows_the_counter_and_clock() {
        // A hurt loss raises by twenty: the raise's own frame (diff 20) is
        // phase-false so the container stays normal; a later phase-true
        // frame (diff 17) blinks and the flash pair draws against the
        // remembered twenty.
        let mut view = View::new();
        let font = chat_font();
        let _ = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            100,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            100,
            false,
            5,
            16.5,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(122.0, 201.0, [16.0 / 256.0, 0.0, 25.0 / 256.0, 9.0 / 256.0])),
            "the raise's own frame keeps the normal container: {slices:?}"
        );
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            103,
            false,
            5,
            16.5,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(194.0, 201.0, [70.0 / 256.0, 0.0, 79.0 / 256.0, 9.0 / 256.0])),
            "the flash full pair against the remembered twenty: {slices:?}"
        );
        assert!(
            slices.contains(&(186.0, 201.0, [61.0 / 256.0, 0.0, 70.0 / 256.0, 9.0 / 256.0])),
            "the current half heart over its flash: {slices:?}"
        );
        // Past the second the settle lands: the remembered health becomes the
        // current one, but the counter still runs ahead phase-true — so the
        // source keeps that frame's flash (`:628-636` never resets the
        // counter). The flash ends with the counter instead.
        let draws = rows_draws(
            &mut view,
            Some(&font),
            6_001,
            103,
            false,
            0,
            16.5,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(186.0, 201.0, [79.0 / 256.0, 0.0, 88.0 / 256.0, 9.0 / 256.0])),
            "the settle keeps that frame's flash: {slices:?}"
        );
        // At the counter the blink ends: plain containers, no flash.
        let draws = rows_draws(
            &mut view,
            Some(&font),
            6_001,
            120,
            false,
            0,
            16.5,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            !slices
                .iter()
                .any(|(_, _, uv)| uv[0] == 70.0 / 256.0 || uv[0] == 79.0 / 256.0),
            "no flash slice survives the counter: {slices:?}"
        );
        // A loss outside the hurt window raises nothing: plain containers.
        let mut view = View::new();
        let _ = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            100,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            103,
            false,
            0,
            16.5,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            !slices.iter().any(|(_, _, uv)| uv[0] == 25.0 / 256.0),
            "no blink container without the hurt window: {slices:?}"
        );
        // A hurt gain raises by ten instead: the flag still runs.
        let mut view = View::new();
        let _ = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            100,
            false,
            0,
            10.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            100,
            false,
            5,
            16.5,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.iter().any(|(_, _, uv)| uv[0] == 25.0 / 256.0),
            "the hurt gain's raise recolours the container: {slices:?}"
        );
    }

    /// The absorption overlay (`GuiIngame.java`:743-755): the cell count grows
    /// by the absorption, the overlay fills from the rightmost cell, and an
    /// odd total shows the half slice (`j6 + 153`) on the first cell only.
    #[test]
    fn the_rows_absorption_overlay_fills_from_the_right() {
        let font = chat_font();
        // Four absorption: twelve cells, the two rightmost overlaid full.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            4.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                130.0,
                191.0,
                [160.0 / 256.0, 0.0, 169.0 / 256.0, 9.0 / 256.0]
            )),
            "the rightmost cell's full overlay, one row up: {slices:?}"
        );
        assert!(
            slices.contains(&(
                122.0,
                191.0,
                [160.0 / 256.0, 0.0, 169.0 / 256.0, 9.0 / 256.0]
            )),
            "the second cell's full overlay: {slices:?}"
        );
        assert!(
            slices.contains(&(122.0, 201.0, [52.0 / 256.0, 0.0, 61.0 / 256.0, 9.0 / 256.0])),
            "the leftmost cell stays a normal full heart: {slices:?}"
        );
        // Three absorption: the first overlaid cell shows the odd half slice.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            3.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                130.0,
                191.0,
                [169.0 / 256.0, 0.0, 178.0 / 256.0, 9.0 / 256.0]
            )),
            "the odd total's half overlay on the first cell only: {slices:?}"
        );
        assert!(
            !slices
                .iter()
                .any(|(x, y, uv)| (*x, *y) != (130.0, 191.0) && uv[0] == 169.0 / 256.0),
            "no other cell carries the half slice: {slices:?}"
        );
    }

    /// The regeneration bob (`GuiIngame.java`:653-658): the cell matching
    /// `updateCounter % ceil(maxHealth + 5)` sits two higher — max health, so
    /// absorption never moves it.
    #[test]
    fn the_rows_regen_bob_reads_max_health() {
        let font = chat_font();
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            8,
            false,
            0,
            20.0,
            20,
            5.0,
            &[10],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.iter().any(|(x, y, _)| *x == 186.0 && *y == 199.0),
            "cell eight bobs at tick eight of twenty-five: {slices:?}"
        );
        assert!(
            slices
                .iter()
                .filter(|(x, y, _)| *x == 122.0 && *y == 201.0)
                .all(|(_, y, _)| *y == 201.0),
            "cell zero stays on the row: {slices:?}"
        );
        // Absorption excluded: twenty-six ticks still pick cell one.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            26,
            false,
            0,
            20.0,
            20,
            5.0,
            &[10],
            300,
            4.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices
                .iter()
                .any(|(x, y, _)| *x == 130.0 && (*y == 199.0 || *y == 189.0)),
            "cell one bobs with absorption held: {slices:?}"
        );
    }

    /// The shared seed and the low-health jitter (`GuiIngame.java`:637,
    /// :711-714): the seed is set once per render from `updateCounter ×
    /// 312871`, and each cell draws `nextInt(2)` while the ceil'd health is
    /// four or below — the JVM's own sequence.
    #[test]
    fn the_rows_shared_seed_feeds_the_low_health_jitter() {
        assert_eq!(ROWS_SEED_FACTOR, 312871);
        assert_eq!(row_seed(0), 0);
        assert_eq!(row_seed(1), 312871);
        // Health two at tick zero: the first ten `nextInt(2)` draws of seed
        // zero read 1,1,0,1,1,0,1,0,1,1 from the highest cell down.
        let font = chat_font();
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            0,
            false,
            0,
            2.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let jittered: Vec<(f32, f32)> = rows_slices(&draws)
            .into_iter()
            .filter(|(x, _, _)| *x >= 122.0 && *x <= 194.0)
            .map(|(x, y, _)| (x, y))
            .collect();
        for (cell, want) in [(9u32, 202.0), (8, 202.0), (7, 201.0), (0, 202.0)] {
            let x = 122.0 + cell as f32 * 8.0;
            assert!(
                jittered.iter().any(|(hx, hy)| *hx == x && *hy == want),
                "cell {cell} sits at {want}: {jittered:?}"
            );
        }
        // Full health draws no jitter at all: every heart cell sits on 201.
        let mut view = View::new();
        let draws = rows_healthy(&mut view, Some(&font), 60_000);
        assert!(
            rows_slices(&draws)
                .iter()
                .filter(|(x, y, _)| *x < 200.0 && (*y == 201.0 || *y == 202.0))
                .all(|(_, y, _)| *y == 201.0),
            "no jitter above four health: {draws:?}"
        );
    }

    /// The food row (`GuiIngame.java`:776-823): the half rule, the
    /// hunger-effect variant, and the saturation-gated jitter with the shared
    /// seed — the JVM's own `nextInt(3)` sequences at the qualifying ticks.
    #[test]
    fn the_rows_food_pins_the_half_and_the_jitter() {
        let font = chat_font();
        // Six food at tick thirty-eight: 38 % 19 == 0, and seed 38 × 312871
        // opens 1,1,2 — offsets 0,0,+1 across the first three cells.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            38,
            false,
            0,
            20.0,
            6,
            0.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                295.0,
                201.0,
                [52.0 / 256.0, 27.0 / 256.0, 61.0 / 256.0, 36.0 / 256.0]
            )),
            "cell zero's full haunch, unshifted: {slices:?}"
        );
        assert!(
            slices.contains(&(
                279.0,
                202.0,
                [52.0 / 256.0, 27.0 / 256.0, 61.0 / 256.0, 36.0 / 256.0]
            )),
            "cell two's full haunch shifted one lower: {slices:?}"
        );
        assert!(
            !slices
                .iter()
                .any(|(x, _, uv)| *x == 271.0 && uv[0] == 52.0 / 256.0),
            "cell three stays empty at six food: {slices:?}"
        );
        // Seven food: the fourth cell carries the half haunch.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            1,
            false,
            0,
            20.0,
            7,
            0.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                271.0,
                201.0,
                [61.0 / 256.0, 27.0 / 256.0, 70.0 / 256.0, 36.0 / 256.0]
            )),
            "cell three's half haunch: {slices:?}"
        );
        // Saturation above zero suppresses the jitter even on a qualifying tick.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            38,
            false,
            0,
            20.0,
            6,
            1.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        assert!(
            rows_slices(&draws)
                .iter()
                .filter(|(x, _, _)| *x >= 223.0)
                .all(|(_, y, _)| *y == 201.0),
            "no jitter while saturation lasts: {draws:?}"
        );
        // A non-qualifying tick draws the row straight at zero saturation.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            39,
            false,
            0,
            20.0,
            6,
            0.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        assert!(
            rows_slices(&draws)
                .iter()
                .filter(|(x, _, _)| *x >= 223.0)
                .all(|(_, y, _)| *y == 201.0),
            "no jitter off the modulo: {draws:?}"
        );
        // The hunger effect shifts the background to 133 and the pair to 88/97.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            60_000,
            1,
            false,
            0,
            20.0,
            6,
            0.0,
            &[17],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                295.0,
                201.0,
                [133.0 / 256.0, 27.0 / 256.0, 142.0 / 256.0, 36.0 / 256.0]
            )),
            "the hungered background: {slices:?}"
        );
        assert!(
            slices.contains(&(
                295.0,
                201.0,
                [88.0 / 256.0, 27.0 / 256.0, 97.0 / 256.0, 36.0 / 256.0]
            )),
            "the hungered full haunch: {slices:?}"
        );
    }

    /// The armour row (`GuiIngame.java`:660-685): the value sums window-0
    /// slots 5–8 through Task 8's `armour_points`, and each point pair draws
    /// full (34, 9), half (25, 9) or empty (16, 9) from the row's left edge —
    /// hidden entirely at zero.
    #[test]
    fn the_rows_armour_sums_slots_five_to_eight() {
        let font = chat_font();
        // Iron helm 2, chain chest 5, iron legs 5, leather boots 1: thirteen.
        let mut view = View::new();
        view.apply(&rows_snapshot([Some(306), Some(303), Some(308), Some(301)]));
        let draws = rows_healthy(&mut view, Some(&font), 5_000);
        let slices = rows_slices(&draws);
        for cell in 0..6 {
            assert!(
                slices.contains(&(
                    122.0 + cell as f32 * 8.0,
                    191.0,
                    [34.0 / 256.0, 9.0 / 256.0, 43.0 / 256.0, 18.0 / 256.0]
                )),
                "icon {cell} full at thirteen armour: {slices:?}"
            );
        }
        assert!(
            slices.contains(&(
                170.0,
                191.0,
                [25.0 / 256.0, 9.0 / 256.0, 34.0 / 256.0, 18.0 / 256.0]
            )),
            "the thirteenth point halves icon six: {slices:?}"
        );
        assert!(
            slices.contains(&(
                178.0,
                191.0,
                [16.0 / 256.0, 9.0 / 256.0, 25.0 / 256.0, 18.0 / 256.0]
            )),
            "icon seven stays empty: {slices:?}"
        );
        // An unknown id contributes nothing: a lone leather cap reads one.
        let mut view = View::new();
        view.apply(&rows_snapshot([Some(9999), None, None, Some(298)]));
        let draws = rows_healthy(&mut view, Some(&font), 5_000);
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                122.0,
                191.0,
                [25.0 / 256.0, 9.0 / 256.0, 34.0 / 256.0, 18.0 / 256.0]
            )),
            "one point halves the first icon: {slices:?}"
        );
        // Bare: no armour slice draws anywhere.
        let mut view = View::new();
        view.apply(&rows_snapshot([None, None, None, None]));
        let draws = rows_healthy(&mut view, Some(&font), 5_000);
        assert!(
            !rows_slices(&draws)
                .iter()
                .any(|(_, y, uv)| *y == 191.0 && (uv[1] - 9.0 / 256.0).abs() < 1e-9),
            "the row hides at zero armour: {draws:?}"
        );
        // The display caps at ten full icons past twenty points.
        assert_eq!(armour_slice(25, 9), (34, 9));
        assert_eq!(armour_slice(20, 9), (34, 9));
        assert_eq!(armour_slice(13, 6), (25, 9));
    }

    /// The air row (`GuiIngame.java`:873-891): the 300/10 split into full and
    /// popping bubbles, right-to-left at the armour's height, gated by
    /// submersion — ten full bubbles at full air, none dry.
    #[test]
    fn the_rows_air_segmentation_pins_the_split() {
        assert_eq!(air_split(300), (10, 0));
        assert_eq!(air_split(299), (10, 0));
        assert_eq!(air_split(272), (9, 1));
        assert_eq!(air_split(270), (9, 0));
        assert_eq!(air_split(31), (1, 1));
        assert_eq!(air_split(30), (1, 0));
        assert_eq!(air_split(1), (0, 1));
        assert_eq!(air_split(0), (0, 0));
        // Submerged at 182: six full bubbles and the fading pair.
        let font = chat_font();
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            true,
            0,
            20.0,
            20,
            5.0,
            &[],
            182,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        for cell in 0..6 {
            assert!(
                slices.contains(&(
                    295.0 - cell as f32 * 8.0,
                    191.0,
                    [16.0 / 256.0, 18.0 / 256.0, 25.0 / 256.0, 27.0 / 256.0]
                )),
                "bubble {cell} full: {slices:?}"
            );
        }
        assert!(
            slices.contains(&(
                247.0,
                191.0,
                [25.0 / 256.0, 18.0 / 256.0, 34.0 / 256.0, 27.0 / 256.0]
            )),
            "the leftmost bubble pops: {slices:?}"
        );
        // Dry: no bubble draws whatever the air reads.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            182,
            0.0,
            0.0,
            0,
        );
        assert!(
            !rows_slices(&draws)
                .iter()
                .any(|(_, _, uv)| (uv[1] - 18.0 / 256.0).abs() < 1e-9),
            "no bubbles out of water: {draws:?}"
        );
    }

    /// The experience bar (`GuiIngame.java`:414-450): the background
    /// `(0, 64, 182, 5)` and the truncated fill `(0, 69, k, 5)` with `k =
    /// (int)(bar × 183)` at `scaledH − 29`, and above it the level in
    /// `0x80FF20` with the four-offset black outline at `scaledH − 35` —
    /// drawn only past level zero.
    #[test]
    fn the_rows_exp_fill_and_level_pin_the_bar() {
        assert_eq!(exp_fill_width(0.0), 0);
        assert_eq!(exp_fill_width(0.5), 91);
        assert_eq!(exp_fill_width(0.42), 76);
        assert_eq!(exp_fill_width(1.0), 183);
        let font = chat_font();
        // 0.42 full at level twelve: the background, the 76-wide fill and the
        // five level draws.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.42,
            12,
        );
        let slices = rows_slices(&draws);
        assert!(
            slices.contains(&(
                122.0,
                211.0,
                [0.0, 64.0 / 256.0, 182.0 / 256.0, 69.0 / 256.0]
            )),
            "the bar background: {slices:?}"
        );
        assert!(
            slices.contains(&(
                122.0,
                211.0,
                [0.0, 69.0 / 256.0, 76.0 / 256.0, 74.0 / 256.0]
            )),
            "the truncated fill: {slices:?}"
        );
        let texts = rows_texts(&draws);
        assert_eq!(
            texts.len(),
            5,
            "the outline four plus the main line: {texts:?}"
        );
        let green = [128.0 / 255.0, 1.0, 32.0 / 255.0, 1.0];
        assert!(
            texts.contains(&("12".to_owned(), 212.0, 205.0, green, false)),
            "the level centred in 0x80FF20: {texts:?}"
        );
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            assert!(
                texts.contains(&(
                    "12".to_owned(),
                    212.0 + dx,
                    205.0 + dy,
                    [0.0, 0.0, 0.0, 1.0],
                    false
                )),
                "the outline at ({dx}, {dy}): {texts:?}"
            );
        }
        // An empty bar draws no fill; level zero draws no text.
        let mut view = View::new();
        let draws = rows_draws(
            &mut view,
            Some(&font),
            5_000,
            0,
            false,
            0,
            20.0,
            20,
            5.0,
            &[],
            300,
            0.0,
            0.0,
            0,
        );
        let slices = rows_slices(&draws);
        assert!(
            !slices
                .iter()
                .any(|(_, _, uv)| (uv[1] - 69.0 / 256.0).abs() < 1e-9),
            "no fill slice at zero: {slices:?}"
        );
        assert!(
            rows_texts(&draws).is_empty(),
            "no level text at zero: {draws:?}"
        );
    }

    /// The rows' gates: outside survival and adventure nothing draws, and the
    /// call site's F1 rule hides the rows with the rest of the overlay.
    #[test]
    fn the_rows_gates_follow_the_overlay_rules() {
        let mut view = View::new();
        view.apply(&rows_snapshot([Some(306), Some(303), Some(308), Some(301)]));
        let font = chat_font();
        let creative = RowsInput {
            survival: false,
            ..rows_input(Some(&font), 5_000)
        };
        assert!(
            view.stat_rows_draws(&creative).is_empty(),
            "no rows outside survival and adventure"
        );
        let hidden = RowsInput {
            hide_gui: true,
            ..rows_input(Some(&font), 5_000)
        };
        assert!(
            view.stat_rows_draws(&hidden).is_empty(),
            "F1 alone hides the rows"
        );
        let screened = RowsInput {
            hide_gui: true,
            screen_open: true,
            ..rows_input(Some(&font), 5_000)
        };
        assert!(
            !view.stat_rows_draws(&screened).is_empty(),
            "a screen keeps the rows"
        );
    }

    /// The held item's swap rule: the ease runs down while the selection differs and
    /// the stack in hand lands only under its tenth (`updateEquippedItem`:581-613).
    #[test]
    fn the_held_item_swaps_under_the_eases_threshold() {
        let sword = MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        };
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = Some(sword.clone());
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        // The ease runs up to one over four ticks with nothing changing (a step of two
        // fifths): the first tick's swap lands the stack at a zero progress.
        for tick in 1..=6 {
            view.apply(&held_tick(tick));
        }
        assert_eq!(view.held.equip.progress(), 1.0);
        assert_eq!(view.held.stack, Some(sword.clone()));
        // The selection moves to an empty slot: the ease runs down, but the stack
        // stays in hand until the progress passes under a tenth.
        view.apply(&ClientEvent::HeldItemSlot { slot: 1 });
        view.apply(&held_tick(7));
        assert_eq!(view.held.equip.progress(), 0.6, "the first step down");
        assert_eq!(view.held.stack, Some(sword), "still in hand");
        for tick in 8..=9 {
            view.apply(&held_tick(tick));
        }
        assert_eq!(view.held.equip.progress(), 0.0, "two more steps down");
        assert_eq!(view.held.stack, None, "the swap lands at zero");
        // The frame at a partial of zero renders `1 - prev`: the pair's lower end.
        let frame = view.held_frame(Instant::now());
        assert!(
            (frame.equip - 0.8).abs() < 1e-6,
            "the frame's rendered ease: {}",
            frame.equip
        );
        assert_eq!(frame.stack, None, "the frame carries the swapped stack");
    }

    /// The swing's setter path: a press starts the counter, the ticks advance it, and
    /// the frame's rendered value follows `getSwingProgress`
    /// (`EntityLivingBase`:2188-2198).
    #[test]
    fn the_held_swing_advances_with_the_ticks() {
        let mut slots: [Option<MetadataItem>; 9] = std::array::from_fn(|_| None);
        slots[0] = Some(MetadataItem {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let mut view = View::new();
        view.apply(&held_snapshot(slots));
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        view.apply(&held_tick(1));
        view.swing_held();
        view.apply(&held_tick(2));
        view.apply(&held_tick(3));
        // At the tick's own start (a partial of zero) the rendered swing reads the
        // pair's previous latch, and at its end the current value: the swing's second
        // tick covers zero to a sixth (the first lands on zero of six).
        let frame = view.held_frame(t0);
        assert_eq!(frame.swing, 0.0, "the swing starts at zero");
        let frame = view.held_frame(t0 + Duration::from_millis(50));
        assert!(
            (frame.swing - 1.0 / 6.0).abs() < 1e-6,
            "the swing's second tick renders a sixth: {}",
            frame.swing
        );
        assert!(
            frame.swing_prev.abs() < 1e-6,
            "the pair's own previous latch: {}",
            frame.swing_prev
        );
    }

    /// The arm sway's own chase and the frame's rendered pair: the source's
    /// `renderArmPitch`/`renderArmYaw` move half the way toward the camera's
    /// rotation each tick (`EntityPlayerSP.updateEntityActionState`:699-702), and
    /// the frame's pair is the raw rotation minus the arm's own at the frame's
    /// fraction (`ItemRenderer.rotateWithPlayerRotations`:124-128).
    #[test]
    fn the_held_frame_lags_the_camera_by_the_arm_sway() {
        let mut view = View::new();
        let t0 = Instant::now();
        view.observe(Vec::new(), t0);
        // The camera turns to pitch 20 and yaw 40 and two ticks chase it: the arm
        // pair reaches (10, 20) then (15, 30) — half the way each tick.
        view.apply(&held_tick_at(1, 20.0, 40.0));
        view.apply(&held_tick_at(2, 20.0, 40.0));
        // At the tick's own start (a partial of zero) the pair's previous latch
        // renders: the sway's arguments are the camera minus (10, 20).
        let frame = view.held_frame(t0);
        assert!(
            (frame.sway_pitch - 10.0).abs() < 1e-6,
            "the sway's pitch argument: {}",
            frame.sway_pitch
        );
        assert!(
            (frame.sway_yaw - 20.0).abs() < 1e-6,
            "the sway's yaw argument: {}",
            frame.sway_yaw
        );
        // At the tick's end the current pair renders: the camera minus (15, 30).
        let frame = view.held_frame(t0 + Duration::from_millis(50));
        assert!(
            (frame.sway_pitch - 5.0).abs() < 1e-6,
            "the rendered current pair: {}",
            frame.sway_pitch
        );
        assert!(
            (frame.sway_yaw - 10.0).abs() < 1e-6,
            "the rendered current yaw pair: {}",
            frame.sway_yaw
        );
        // A third tick boundary: the arm moves to (17.5, 35), halving the yaw term
        // again — the chase keeps closing on the camera.
        view.apply(&held_tick_at(3, 20.0, 40.0));
        let frame = view.held_frame(t0 + Duration::from_millis(50));
        assert!(
            (frame.sway_pitch - 2.5).abs() < 1e-6,
            "the third tick's pitch argument: {}",
            frame.sway_pitch
        );
        assert!(
            (frame.sway_yaw - 5.0).abs() < 1e-6,
            "the third tick's yaw argument: {}",
            frame.sway_yaw
        );
    }

    // ---- the screen group ----

    #[test]
    fn the_capped_preview_draws_the_yellow_cap() {
        assert_eq!(
            preview_alt(1, true),
            Some(String::from("§e1")),
            "past the cap the count draws yellow"
        );
        assert_eq!(
            preview_alt(32, false),
            None,
            "inside the cap the white count path draws"
        );
    }

    /// The screen group's own frame: no screen draws nothing, a declared
    /// variant draws the generic frame's background alone, and a container
    /// draws the background, the centred sheet, the title and the carried
    /// stack at the pointer minus 8 (`EntityRenderer.java`:1185-1191;
    /// `GuiContainer.java`:149/:169).
    #[test]
    fn no_screen_draws_nothing() {
        let font = chat_font();
        let screens = Screens::default();
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled: chat_resolution(),
                mouse: Some((100.0, 50.0)),
                advanced: false,
                level: 0,
                effects: &[],
                preview_skin: None,
            },
        );
        assert!(draws.is_empty(), "no screen owns no draws");
    }

    #[test]
    fn the_sign_editor_draws_its_title_lines_and_done() {
        use oxide_client::screens::sign::{SIGN_DONE_TEXT, SIGN_EDIT_TITLE, SIGN_WIDGETS_SHEET};
        let font = chat_font();
        let mut screens = Screens::default();
        screens.open_sign(
            0,
            64,
            0,
            [String::new(), String::new(), String::new(), String::new()],
        );
        let scaled = chat_resolution();
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled,
                mouse: Some((100.0, 50.0)),
                advanced: false,
                level: 0,
                effects: &[],
                preview_skin: None,
            },
        );
        // The background halves, the board backing, the title, the four
        // lines, the Done blit and its label: 2 + 1 + 1 + 4 + 2.
        assert_eq!(draws.len(), 10, "the editor's full chrome: {draws:?}");
        let texts: Vec<&str> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&SIGN_EDIT_TITLE),
            "the title draws: {texts:?}"
        );
        assert!(
            texts.contains(&SIGN_DONE_TEXT),
            "the Done label draws: {texts:?}"
        );
        assert!(
            draws.iter().any(|draw| matches!(
                draw,
                HudDraw::TexturedRect {
                    texture: HudTexture::Named(SIGN_WIDGETS_SHEET),
                    ..
                }
            )),
            "the Done button blits the widgets sheet: {draws:?}"
        );
        // The pointer at (100.0, 50.0) stands off the Done button, so the
        // threaded pointer leaves the idle strip (v 66) on the blit.
        let blit = draws
            .iter()
            .find_map(|draw| match draw {
                HudDraw::TexturedRect {
                    texture: HudTexture::Named(SIGN_WIDGETS_SHEET),
                    uv,
                    ..
                } => Some(*uv),
                _ => None,
            })
            .expect("the Done blit");
        assert_eq!(
            blit,
            [0.0, 66.0 / 256.0, 200.0 / 256.0, 86.0 / 256.0],
            "the idle strip off the button: {draws:?}"
        );
    }

    #[test]
    fn the_book_reader_draws_its_sheet_indicator_and_done() {
        use oxide_client::screens::book::BOOK_SHEET;
        let font = chat_font();
        let mut screens = Screens::default();
        screens.open_book(MetadataItem {
            id: 386,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let scaled = chat_resolution();
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled,
                mouse: Some((100.0, 50.0)),
                advanced: false,
                level: 0,
                effects: &[],
                preview_skin: None,
            },
        );
        // The background halves, the 192x192 sheet, the indicator, the one
        // empty page's line, the Done blit and its label: 2 + 1 + 1 + 1 + 2.
        // No arrow draws: the single page shows neither.
        assert_eq!(draws.len(), 7, "the reader's full chrome: {draws:?}");
        let texts: Vec<&str> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"Page 1 of 1"),
            "the indicator draws: {texts:?}"
        );
        assert!(
            draws.iter().any(|draw| matches!(
                draw,
                HudDraw::TexturedRect {
                    texture: HudTexture::Named(BOOK_SHEET),
                    ..
                }
            )),
            "the reader blits the book sheet: {draws:?}"
        );
    }

    #[test]
    fn the_inventory_effects_overlay_lists_rows_in_ascending_id() {
        // Two live effects, handed descending: the overlay lists Speed II
        // (id 1) before Strength (id 5) — the port's deterministic order over
        // the source's `HashMap` iteration (the recorded divergence) — each
        // with its name/duration pens, after the screen's own title.
        let font = chat_font();
        let mut screens = Screens::default();
        screens.open_inventory();
        let scaled = chat_resolution();
        let effects = [
            StatusEffect {
                effect_id: 5,
                amplifier: 0,
                duration: 1200,
            },
            StatusEffect {
                effect_id: 1,
                amplifier: 1,
                duration: 3600,
            },
        ];
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled,
                mouse: Some((100.0, 50.0)),
                advanced: false,
                level: 0,
                effects: &effects,
                preview_skin: None,
            },
        );
        let texts: Vec<String> = draws
            .iter()
            .filter_map(|draw| match draw {
                HudDraw::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            ["Crafting", "Speed II", "3:00", "Strength", "1:00"],
            "the title, then the rows in ascending id: {texts:?}"
        );
        let rows = draws
            .iter()
            .filter(|draw| {
                matches!(
                    draw,
                    HudDraw::TexturedRect {
                        texture: HudTexture::Named("gui/container/inventory"),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            rows, 5,
            "the panel, two row sprites and two icons share the sheet: {draws:?}"
        );
    }

    #[test]
    fn the_container_frame_centres_sheet_title_and_cursor() {
        use oxide_proto_v47::window::WindowKind;

        let font = chat_font();
        let mut screens = Screens::default();
        screens.open_container(
            3,
            WindowKind::Unknown,
            String::from("{\"text\":\"Chest\"}"),
            0,
            None,
        );
        screens
            .container_mut()
            .expect("the open is a container")
            .set_screen_size(427, 240);
        screens.apply_snapshot(
            3,
            Vec::new(),
            Some(MetadataItem {
                id: 1,
                count: 4,
                damage: 0,
                nbt: None,
            }),
            Vec::new(),
        );
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled: chat_resolution(),
                mouse: Some((100.0, 50.0)),
                advanced: false,
                level: 0,
                effects: &[],
                preview_skin: None,
            },
        );
        // The 427x240 frame centres the 176x166 panel at (125, 37); the
        // generic layout carries no slots, so the background pair, the sheet,
        // the title and the carried stack with its count is the whole list.
        assert_eq!(
            draws.len(),
            6,
            "background, sheet, title, item, count: {draws:?}"
        );
        assert!(
            matches!(
                draws[2],
                HudDraw::TexturedRect {
                    x: 125.0,
                    y: 37.0,
                    width: 176.0,
                    height: 166.0,
                    ..
                }
            ),
            "the sheet blits at the centred origin: {:?}",
            draws[2]
        );
        assert!(
            matches!(
                &draws[3],
                HudDraw::Text { text, x: 133.0, y: 43.0, .. } if text == "Chest"
            ),
            "the generic title at (8, 6) over the panel: {:?}",
            draws[3]
        );
        assert!(
            matches!(
                draws[4],
                HudDraw::Item {
                    x: 92.0,
                    y: 42.0,
                    ..
                }
            ),
            "the carried stack at the pointer minus 8: {:?}",
            draws[4]
        );
        assert!(
            matches!(
                &draws[5],
                HudDraw::Text { text, y: 51.0, .. } if text == "4"
            ),
            "the count overlay under the carried stack: {:?}",
            draws[5]
        );
    }

    #[test]
    fn the_hovered_tooltip_waits_for_the_empty_cursor() {
        use oxide_client::screens::container::{ContainerLayout, SlotPos, TitleKind};
        use oxide_game::container::BaseStackCaps;

        static ONE: &[SlotPos] = &[SlotPos {
            index: 0,
            x: 8,
            y: 18,
            block: oxide_client::screens::container::SlotBlock::Container,
        }];
        static ONE_LAYOUT: ContainerLayout = ContainerLayout {
            x_size: 176,
            y_size: 166,
            sheet: "unit/panel",
            slots: ONE,
            title: TitleKind::Generic,
            background: oxide_client::screens::container::BackgroundKind::Full,
        };
        fn stack(id: i16, count: u8) -> MetadataItem {
            MetadataItem {
                id,
                count,
                damage: 0,
                nbt: None,
            }
        }
        fn has_text(draws: &[HudDraw], want: &str) -> bool {
            draws
                .iter()
                .any(|draw| matches!(draw, HudDraw::Text { text, .. } if text == want))
        }

        let font = chat_font();
        let mut screens = Screens::default();
        screens.test_container(3, &ONE_LAYOUT);
        let container = screens.container_mut().expect("the stood container");
        container.set_screen_size(427, 240);
        container.apply_snapshot(vec![Some(stack(276, 1))], Some(stack(1, 4)), Vec::new());
        container.mouse_moved(16.0, 26.0, &BaseStackCaps);
        assert_eq!(
            container.hovered(),
            Some(0),
            "the pointer hovers the sword's slot"
        );
        // A stack carried over the hovered slot: the tooltip stays out
        // (`GuiContainer.java:190` renders it only when `getItemStack() ==
        // null` — tooltip and carried stack never co-draw).
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled: chat_resolution(),
                mouse: Some((141.0, 63.0)),
                advanced: false,
                level: 0,
                effects: &[],
                preview_skin: None,
            },
        );
        assert!(
            !has_text(&draws, "Diamond Sword§r"),
            "no tooltip under the carried stack: {draws:?}"
        );
        // Released, the pointer unmoved: the hovered slot's tooltip returns.
        screens
            .container_mut()
            .expect("the stood container")
            .apply_snapshot(vec![Some(stack(276, 1))], None, Vec::new());
        let draws = screen_draws(
            &screens,
            &ScreenDrawInput {
                font: Some(&font),
                scaled: chat_resolution(),
                mouse: Some((141.0, 63.0)),
                advanced: false,
                level: 0,
                effects: &[],
                preview_skin: None,
            },
        );
        assert!(
            has_text(&draws, "Diamond Sword§r"),
            "the tooltip returns on release: {draws:?}"
        );
    }
}
