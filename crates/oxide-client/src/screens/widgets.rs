//! The menu widget set: the button and the slider from the widgets sheet.
//!
//! The port reads values and names from the source's button
//! (`client/gui/GuiButton.java`), its options-row button and slider
//! (`GuiOptionButton.java`, `GuiOptionSlider.java`); no source text is
//! copied.
//!
//! The button is the 200x20 base (`GuiButton.java`:36-39); the options-row
//! controls are 150x20 (`GuiOptionButton.java`:22,
//! `GuiOptionSlider.java`:23) — the plan's "152" is corrected here
//! (first-hand: no `152` names a control width anywhere under the source's
//! `gui/` tree).
//!
//! The list background (`GuiSlot.java`:222-320 — dirt `optionsBackground`
//! tiled 32 px with the scroll-tied offset, the content box, the top/bottom
//! black gradient fades, the scrollbar) is DEFERRED: the main menu stands
//! no list, so no list code ships in this round; the multiplayer screen
//! (Task 16) stands the first list and carries the background with it.
//!
//! The widgets draw as [`HudDraw`](oxide_render::hud::HudDraw) records —
//! `TexturedRect` over [`WIDGETS_SHEET`] plus `Text` — so no `HudDraw`
//! growth was needed for them (the splash's rotation is the only new draw
//! kind, and it rides its own variant). The API is option-agnostic: the
//! slider carries a 0..1 value in and out, and the settings import stays
//! with the task that owns the option (the T4 no-game-edge precedent).

use oxide_assets::font::Font;
use oxide_render::hud::{HudDraw, HudTexture};
use oxide_render::text::string_width;

/// The widgets sheet the button and the slider blit: the extraction tree's
/// `gui/widgets.png`, the sheet `GuiButton` draws from.
pub const WIDGETS_SHEET: &str = "gui/widgets";
/// The button's base width: `GuiButton.java`:36-39 builds 200x20.
pub const BUTTON_WIDTH: f32 = 200.0;
/// The button's base height (see [`BUTTON_WIDTH`]).
pub const BUTTON_HEIGHT: f32 = 20.0;
/// The options-row controls' width: `GuiOptionButton.java`:22 and
/// `GuiOptionSlider.java`:23 build 150x20.
pub const ROW_WIDTH: f32 = 150.0;
/// The button label's idle colour: 14737632 (`drawButton`'s default arm).
pub const LABEL_IDLE: u32 = 14737632;
/// The button label's disabled colour: 10526880 (`drawButton`'s
/// `!enabled` arm).
pub const LABEL_DISABLED: u32 = 10526880;
/// The button label's hovered colour: 16777120 (`drawButton`'s hovered
/// arm).
pub const LABEL_HOVERED: u32 = 16777120;
/// The press sound: `gui.button.press` (`GuiButton.playPressSound`
/// :143-146).
pub const BUTTON_PRESS_SOUND: &str = "gui.button.press";
/// The slider knob's strip: two 4x20 halves at v 66 (`mouseDragged`).
pub const SLIDER_KNOB_V: f32 = 66.0;
/// The slider knob halves' width in GUI pixels (see [`SLIDER_KNOB_V`]).
pub const SLIDER_KNOB_HALF: f32 = 4.0;
/// The option slider's left knob half's sheet column: u 0
/// (`GuiOptionSlider.mouseDragged`'s first blit).
pub const OPTION_KNOB_LEFT_U: f32 = 0.0;
/// The option slider's right knob half's sheet column: u 196 (see
/// [`OPTION_KNOB_LEFT_U`]).
pub const OPTION_KNOB_RIGHT_U: f32 = 196.0;

/// Snaps the value to the step grid: `step·round(v/step)`
/// (`Options.snapToStep`; a zero step passes the value through, which is
/// the continuous-slider arm — GAMMA's step is 0.0F).
pub fn snap_to_step(value: f32, step: f32) -> f32 {
    if step > 0.0 {
        step * (value / step).round()
    } else {
        value
    }
}

/// Denormalizes the 0..1 slider position into the option's value:
/// `snap(min+(max-min)·clamp01(v))` (`Options.denormalizeValue`).
pub fn denormalize_option(value01: f32, min: f32, max: f32, step: f32) -> f32 {
    let folded = min + (max - min) * value01.clamp(0.0, 1.0);
    snap_to_step(folded, step).clamp(min, max)
}

/// Normalizes the option's value into the 0..1 slider position:
/// `clamp01((snap(v)-min)/(max-min))` (`Options.normalizeValue`).
pub fn normalize_option(value: f32, min: f32, max: f32, step: f32) -> f32 {
    ((snap_to_step(value, step) - min) / (max - min)).clamp(0.0, 1.0)
}

/// One packed label colour as the straight RGBA the text draw carries.
fn unpack_rgb(packed: u32) -> [f32; 4] {
    [
        ((packed >> 16) & 255) as f32 / 255.0,
        ((packed >> 8) & 255) as f32 / 255.0,
        (packed & 255) as f32 / 255.0,
        1.0,
    ]
}

/// The label colour for the state: disabled greys the text, hovered warms
/// it, idle sits between (`GuiButton.drawButton`'s three arms).
pub fn label_colour(enabled: bool, hovered: bool) -> [f32; 4] {
    if !enabled {
        unpack_rgb(LABEL_DISABLED)
    } else if hovered {
        unpack_rgb(LABEL_HOVERED)
    } else {
        unpack_rgb(LABEL_IDLE)
    }
}

/// The hover state: 0 disabled, 2 hovered, else 1
/// (`GuiButton.getHoverState`:59-67).
pub fn hover_state(enabled: bool, mouse_over: bool) -> u8 {
    if !enabled {
        0
    } else if mouse_over {
        2
    } else {
        1
    }
}

/// The button strip's top edge on the sheet: `46 + i * 20`
/// (`GuiButton.drawButton`'s two blits).
pub fn button_strip_v(state: u8) -> f32 {
    46.0 + f32::from(state) * 20.0
}

/// One menu button: the id the click answers, the rect in GUI pixels, the
/// label, and the enabled/visible gates (`GuiButton.java`:36-46).
#[derive(Debug, Clone)]
pub struct Button {
    /// The id the press answers (`actionPerformed` reads it).
    pub id: i32,
    /// The left edge in GUI pixels.
    pub x: f32,
    /// The top edge in GUI pixels.
    pub y: f32,
    /// The width in GUI pixels.
    pub w: f32,
    /// The height in GUI pixels.
    pub h: f32,
    /// The label the centred text draws.
    pub label: String,
    /// The enabled gate: a disabled button draws the grey strip and label
    /// and never presses (`getHoverState`, `mousePressed`).
    pub enabled: bool,
    /// The visible gate: a hidden button draws nothing and never presses.
    pub visible: bool,
}

impl Button {
    /// Stands a button on the rect with the label, enabled and visible
    /// (`GuiButton.java`:36-46 build enabled and visible).
    pub fn new(id: i32, x: f32, y: f32, w: f32, h: f32, label: String) -> Self {
        Self {
            id,
            x,
            y,
            w,
            h,
            label,
            enabled: true,
            visible: true,
        }
    }

    /// Whether the GUI-space point stands on the button: the full rect,
    /// edges inclusive on the top-left and exclusive on the bottom-right
    /// (`mousePressed`:126-129 hit-tests the whole rect).
    pub fn hit(&self, point: (f32, f32)) -> bool {
        point.0 >= self.x
            && point.1 >= self.y
            && point.0 < self.x + self.w
            && point.1 < self.y + self.h
    }

    /// Whether the point hovers the button: on the rect while visible (the
    /// draw's own `hovered` assignment reads the same rect).
    pub fn hovered(&self, mouse: Option<(f32, f32)>) -> bool {
        self.visible && mouse.is_some_and(|point| self.hit(point))
    }

    /// The hover state at the pointer: 0 disabled, 2 hovered, else 1.
    pub fn hover_state(&self, mouse: Option<(f32, f32)>) -> u8 {
        hover_state(self.enabled, self.hovered(mouse))
    }

    /// Whether the point presses the button: enabled, visible, and on the
    /// rect (`mousePressed`:126-129 gates all three).
    pub fn pressed(&self, point: (f32, f32)) -> bool {
        self.enabled && self.visible && self.hit(point)
    }

    /// The button's draws: the two-half blit at `46 + i * 20` (the left
    /// half from u 0, the right half from u `200 - w/2`) and the centred
    /// label at `x + w/2, y + (h - 8)/2` in the state's colour. A hidden
    /// button draws nothing.
    pub fn draws(&self, font: &Font, mouse: Option<(f32, f32)>) -> Vec<HudDraw> {
        if !self.visible {
            return Vec::new();
        }
        let state = self.hover_state(mouse);
        let v = button_strip_v(state);
        let half = self.w / 2.0;
        let mut draws = vec![
            HudDraw::TexturedRect {
                texture: HudTexture::Named(WIDGETS_SHEET),
                x: self.x,
                y: self.y,
                width: half,
                height: self.h,
                uv: [0.0, v / 256.0, half / 256.0, (v + self.h) / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named(WIDGETS_SHEET),
                x: self.x + half,
                y: self.y,
                width: self.w - half,
                height: self.h,
                uv: [
                    (200.0 - half) / 256.0,
                    v / 256.0,
                    200.0 / 256.0,
                    (v + self.h) / 256.0,
                ],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
        ];
        let width = string_width(font, &self.label);
        draws.push(HudDraw::Text {
            text: self.label.clone(),
            x: self.x + self.w / 2.0 - width as f32 / 2.0,
            y: self.y + (self.h - 8.0) / 2.0,
            scale: 1.0,
            colour: label_colour(self.enabled, self.hovered(mouse)),
            shadow: true,
            blend: true,
        });
        draws
    }
}

/// One menu slider: the rect in GUI pixels, the 0..1 value, the drag latch
/// and the gates. Option-agnostic: the value travels in and out as a
/// plain float, and the option normalise/denormalise pair stays with the
/// settings owner — the port carries no settings import here.
#[derive(Debug, Clone)]
pub struct Slider {
    /// The left edge in GUI pixels.
    pub x: f32,
    /// The top edge in GUI pixels.
    pub y: f32,
    /// The width in GUI pixels.
    pub w: f32,
    /// The height in GUI pixels.
    pub h: f32,
    /// The 0..1 value (`sliderValue`).
    pub value: f32,
    /// The drag latch: set on press, cleared on release
    /// (`GuiOptionSlider.java`:78-100).
    pub dragging: bool,
    /// The enabled gate (the button's own).
    pub enabled: bool,
    /// The visible gate (the button's own).
    pub visible: bool,
}

impl Slider {
    /// Stands a slider on the rect at the value, latch clear.
    pub fn new(x: f32, y: f32, w: f32, h: f32, value: f32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            value: value.clamp(0.0, 1.0),
            dragging: false,
            enabled: true,
            visible: true,
        }
    }

    /// The knob's left edge: `x + value * (w - 8)` (`mouseDragged`'s blit
    /// origin).
    pub fn knob_x(&self) -> f32 {
        self.x + self.value * (self.w - 8.0)
    }

    /// Folds the pointer's x into the value: `(mouseX - (x + 4)) / (w - 8)`
    /// clamped 0..1 (`mousePressed`, `mouseDragged`).
    pub fn set_from_mouse(&mut self, mouse_x: f32) {
        self.value = ((mouse_x - (self.x + 4.0)) / (self.w - 8.0)).clamp(0.0, 1.0);
    }

    /// One press at the point: the button's own gate, then the value folds
    /// and the latch sets (`mousePressed`).
    pub fn press(&mut self, point: (f32, f32)) -> bool {
        let hit = self.enabled
            && self.visible
            && point.0 >= self.x
            && point.1 >= self.y
            && point.0 < self.x + self.w
            && point.1 < self.y + self.h;
        if hit {
            self.set_from_mouse(point.0);
            self.dragging = true;
        }
        hit
    }

    /// One drag motion at the pointer's x: folds only while latched
    /// (`mouseDragged`'s `dragging` arm).
    pub fn drag_to(&mut self, mouse_x: f32) {
        if self.dragging {
            self.set_from_mouse(mouse_x);
        }
    }

    /// One release anywhere: the latch clears (`mouseReleased`).
    pub fn release(&mut self) {
        self.dragging = false;
    }

    /// The slider's draws: the track blit (the v 46 strip — `getHoverState`
    /// returns 0 always, so the track never highlights) and the knob's two
    /// 4x20 halves (u 0 and u 196 at v 66) at the value's x. A hidden
    /// slider draws nothing.
    pub fn draws(&self) -> Vec<HudDraw> {
        if !self.visible {
            return Vec::new();
        }
        let half = self.w / 2.0;
        vec![
            HudDraw::TexturedRect {
                texture: HudTexture::Named(WIDGETS_SHEET),
                x: self.x,
                y: self.y,
                width: half,
                height: self.h,
                uv: [0.0, 46.0 / 256.0, half / 256.0, (46.0 + self.h) / 256.0],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named(WIDGETS_SHEET),
                x: self.x + half,
                y: self.y,
                width: self.w - half,
                height: self.h,
                uv: [
                    (200.0 - half) / 256.0,
                    46.0 / 256.0,
                    200.0 / 256.0,
                    (46.0 + self.h) / 256.0,
                ],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named(WIDGETS_SHEET),
                x: self.knob_x(),
                y: self.y,
                width: SLIDER_KNOB_HALF,
                height: self.h,
                uv: [
                    0.0,
                    SLIDER_KNOB_V / 256.0,
                    SLIDER_KNOB_HALF / 256.0,
                    (SLIDER_KNOB_V + self.h) / 256.0,
                ],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
            HudDraw::TexturedRect {
                texture: HudTexture::Named(WIDGETS_SHEET),
                x: self.knob_x() + SLIDER_KNOB_HALF,
                y: self.y,
                width: SLIDER_KNOB_HALF,
                height: self.h,
                uv: [
                    196.0 / 256.0,
                    SLIDER_KNOB_V / 256.0,
                    200.0 / 256.0,
                    (SLIDER_KNOB_V + self.h) / 256.0,
                ],
                colour: [1.0, 1.0, 1.0, 1.0],
            },
        ]
    }
}

/// One option slider: the source's `GuiOptionSlider` — the option-agnostic
/// [`Slider`] base plus the option's range law and the composed label.
///
/// The value travels as the option's own (denormalized) value: the press
/// and each drag motion answer it, and the caller writes it through the
/// settings on every event (`mousePressed` / `mouseDragged` write through
/// `setOptionFloatValue` per event). The settings import stays with the
/// task that owns the option (the T4 no-game-edge precedent) — this type
/// carries no settings.
#[derive(Debug, Clone)]
pub struct OptionSlider {
    /// The option-agnostic base: the rect, the 0..1 position, the drag
    /// latch and the gates.
    slider: Slider,
    /// The option's floor (`valueMin`).
    min: f32,
    /// The option's ceiling (`valueMax`).
    max: f32,
    /// The option's step (`valueStep`; 0 is the continuous arm).
    step: f32,
    /// The composed label the centred text draws (`displayString`).
    label: String,
}

impl OptionSlider {
    /// Stands an option slider on the rect at the option's `value` with
    /// `label`, the latch clear (the constructor normalizes the stored
    /// value into the position).
    #[allow(clippy::too_many_arguments)] // The rect, the law triple and the seed: one arg each.
    pub fn new(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        min: f32,
        max: f32,
        step: f32,
        value: f32,
        label: String,
    ) -> Self {
        Self {
            slider: Slider::new(x, y, w, h, normalize_option(value, min, max, step)),
            min,
            max,
            step,
            label,
        }
    }

    /// The left edge in GUI pixels.
    pub fn x(&self) -> f32 {
        self.slider.x
    }

    /// The top edge in GUI pixels.
    pub fn y(&self) -> f32 {
        self.slider.y
    }

    /// The width in GUI pixels.
    pub fn w(&self) -> f32 {
        self.slider.w
    }

    /// The height in GUI pixels.
    pub fn h(&self) -> f32 {
        self.slider.h
    }

    /// The option's own value at the position (see [`denormalize_option`]).
    pub fn value(&self) -> f32 {
        denormalize_option(self.slider.value, self.min, self.max, self.step)
    }

    /// The 0..1 slider position (the law's own domain): the fold the
    /// next store write reads.
    pub fn position(&self) -> f32 {
        self.slider.value
    }

    /// Moves the slider's rect (the row list re-seats every control on
    /// scroll).
    pub fn set_pos(&mut self, x: f32, y: f32) {
        self.slider.x = x;
        self.slider.y = y;
    }

    /// Reseats the slider at the option's `value` (the constructor's own
    /// normalization, the `mouseDragged` re-read after a write).
    pub fn set_value(&mut self, value: f32) {
        self.slider.value = normalize_option(value, self.min, self.max, self.step);
    }

    /// The composed label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Replaces the composed label (the caller recomposes it after a
    /// write, `displayString`'s re-read).
    pub fn set_label(&mut self, label: String) {
        self.label = label;
    }

    /// Whether the drag latch stands.
    pub fn dragging(&self) -> bool {
        self.slider.dragging
    }

    /// The knob's left edge (see [`Slider::knob_x`]).
    pub fn knob_x(&self) -> f32 {
        self.slider.knob_x()
    }

    /// One press at the point: the base's gate, then the option's value
    /// answers (`mousePressed` folds, writes, and latches).
    pub fn press(&mut self, point: (f32, f32)) -> Option<f32> {
        if self.slider.press(point) {
            Some(self.value())
        } else {
            None
        }
    }

    /// One drag motion at the pointer's x: answers the option's value only
    /// while latched (`mouseDragged`'s `dragging` arm).
    pub fn drag_to(&mut self, mouse_x: f32) -> Option<f32> {
        if self.slider.dragging {
            self.slider.drag_to(mouse_x);
            Some(self.value())
        } else {
            None
        }
    }

    /// One release anywhere: the latch clears (`mouseReleased`).
    pub fn release(&mut self) {
        self.slider.release();
    }

    /// The option slider's draws: the base's track (the v 46 strip — the
    /// hover state reads 0 always, so the track never highlights) and the
    /// knob's two 4x20 halves (u 0 and u 196 at v 66), with the composed
    /// label centred over them (`drawButton`'s label arm). A hidden
    /// slider draws nothing.
    pub fn draws(&self, font: &Font, mouse: Option<(f32, f32)>) -> Vec<HudDraw> {
        let mut draws = self.slider.draws();
        if !self.slider.visible {
            return draws;
        }
        let width = string_width(font, &self.label);
        draws.push(HudDraw::Text {
            text: self.label.clone(),
            x: self.slider.x + self.slider.w / 2.0 - width as f32 / 2.0,
            y: self.slider.y + (self.slider.h - 8.0) / 2.0,
            scale: 1.0,
            colour: label_colour(
                self.slider.enabled,
                self.slider.enabled
                    && mouse.is_some_and(|point| {
                        point.0 >= self.slider.x
                            && point.1 >= self.slider.y
                            && point.0 < self.slider.x + self.slider.w
                            && point.1 < self.slider.y + self.slider.h
                    }),
            ),
            shadow: true,
            blend: true,
        });
        draws
    }
}

/// One option cycle button: the source's `GuiOptionButton` — a [`Button`]
/// whose label composes the option's name and its current value (the
/// `getKeyBinding` prefix rule, `"Graphics: Fancy"`).
///
/// The button carries the name so the caller recomposes the label with one
/// call after a cycle (`displayString`'s re-read); the cycle itself stays
/// with the task that owns the option.
#[derive(Debug, Clone)]
pub struct OptionButton {
    /// The button proper: the id, the rect, the composed label and the
    /// gates.
    pub button: Button,
    /// The option's name, the label's head (`enumString`'s translation).
    pub name: String,
}

impl OptionButton {
    /// Stands a cycle button on the rect with the composed `"name: value"`
    /// label, enabled and visible.
    pub fn new(id: i32, x: f32, y: f32, w: f32, h: f32, name: String, value: String) -> Self {
        let label = format!("{name}: {value}");
        Self {
            button: Button::new(id, x, y, w, h, label),
            name,
        }
    }

    /// The composed label.
    pub fn label(&self) -> &str {
        &self.button.label
    }

    /// The left edge in GUI pixels.
    pub fn x(&self) -> f32 {
        self.button.x
    }

    /// The top edge in GUI pixels.
    pub fn y(&self) -> f32 {
        self.button.y
    }

    /// The width in GUI pixels.
    pub fn w(&self) -> f32 {
        self.button.w
    }

    /// The height in GUI pixels.
    pub fn h(&self) -> f32 {
        self.button.h
    }

    /// Moves the button's rect (the row list re-seats every control on
    /// scroll).
    pub fn set_pos(&mut self, x: f32, y: f32) {
        self.button.x = x;
        self.button.y = y;
    }

    /// Recomposes the label around the new value after a cycle.
    pub fn set_value(&mut self, value: String) {
        self.button.label = format!("{}: {value}", self.name);
    }

    /// Whether the point presses the button: enabled, visible, and on the
    /// rect (the row's `mousePressed` gate).
    pub fn press(&mut self, point: (f32, f32)) -> bool {
        self.button.pressed(point)
    }

    /// The cycle button's draws: the button's own (the two-half blit and
    /// the composed label).
    pub fn draws(&self, font: &Font, mouse: Option<(f32, f32)>) -> Vec<HudDraw> {
        self.button.draws(font, mouse)
    }
}
