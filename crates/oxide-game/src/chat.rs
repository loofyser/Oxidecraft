//! The chat message pipeline: the component model, its hostile parser, the
//! `§`-aware flattening, the source's wrapping, and the log's retention, fade
//! and scroll state.
//!
//! The model follows the source's chat path: `IChatComponent.Serializer`
//! (`IChatComponent.java`:61-178) parses a message into a component tree —
//! `text` plus `extra` children, each carrying a `ChatStyle`
//! (`ChatStyle.java`:499-567) — and `GuiNewChat.setChatLine`
//! (`GuiNewChat.java`:140-176) splits it at the chat box's width and keeps the
//! split lines, newest first, capped at one hundred. The fade is
//! `GuiNewChat.drawChat`:59-77. The pixels are the window's — which is
//! why the chatOpacity factor
//! (`GuiNewChat.drawChat`:38, `:75`; 1.0F at the default `GameSettings.java`:85) stays
//! out of the pinned values.
//!
//! # Hostile input
//!
//! Everything here is fed by the wire, so nothing panics and nothing grows
//! without bound: the JSON walk ignores unknown keys, degrades wrong-typed
//! ones, caps nesting at [`MAX_COMPONENT_DEPTH`], and caps the whole tree at
//! [`MAX_COMPONENT_TOTAL`] components and [`MAX_TEXT_BYTES`] of taken text.
//! A payload that is not JSON at all becomes a plain-text component of its
//! own text, clipped the same way.
//!
//! # Colour precedence
//!
//! The source draws a line as the concatenation, per element, of the
//! element's formatting codes, its text, and a `§r`
//! (`ChatComponentStyle.getFormattedText`:87-99), rendered left to right by
//! `FontRenderer.renderStringAtPos`:392-455`. So the element's own style — the
//! JSON colour and flags, inherited through the tree
//! (`ChatStyle.getColor`:129-132) — is the state its runs start in, and a `§`
//! code inside the text overrides it from that position on: the text wins over
//! the JSON. A colour code clears the styles, `§r` returns the run to the
//! draw's base colour (`FontRenderer.java`:444-452), and an unknown code is the source's clamp
//! to white (`FontRenderer.java`:410-413).

use oxide_assets::font::Font;
use oxide_render::text::string_width;
use serde_json::Value;

/// The deepest nesting the JSON walk descends; the root is depth zero.
///
/// The cap is this port's own hostile-input rule: a payload nested deeper is
/// cut at the cap, never walked to exhaustion.
pub const MAX_COMPONENT_DEPTH: usize = 16;

/// The most components one parse builds, the root included.
///
/// The cap is this port's own hostile-input rule against width rather than
/// depth: an `extra` array longer than the budget keeps yielding siblings only
/// until the cap.
pub const MAX_COMPONENT_TOTAL: usize = 4096;

/// The most text one parse keeps, in bytes, across the whole tree: every
/// string the walk takes — text, click values, hover payloads — draws on one
/// budget, and a longer string is clipped at a character boundary.
///
/// The cap is this port's own hostile-input rule, not the protocol's: the
/// wire's string ceiling is the codec's business, not this layer's.
pub const MAX_TEXT_BYTES: usize = 65536;

/// The wrap budget at the default chat settings: `floor(chatWidth * (320-40)
/// + 40)` is 320, and the chat's scale of 1.0 leaves it there
/// (`GuiNewChat.java`:361-366 and `:147`; the defaults `chatWidth` 1.0F and
/// `chatScale` 1.0F at `GameSettings.java`:106-107).
pub const CHAT_WIDTH: i32 = 320;

/// The log's cap on kept lines, counted in split lines: the source's own one
/// hundred (`GuiNewChat.java`:162-165).
pub const LOG_CAP: usize = 100;

/// The lines an unfocused chat draws: the unfocused box of 90 over the
/// nine-pixel pitch (`GuiNewChat.java`:348-351, `:375-378`; the height
/// formula `:368-373` at `chatHeightUnfocused` 0.44366196F,
/// `GameSettings.java`:108).
pub const LINES_CLOSED: usize = 10;

/// The lines a focused chat draws: the focused box of 180 over the
/// nine-pixel pitch (`GuiNewChat.java`:375-378, `:368-373` at
/// `chatHeightFocused` 1.0F, `GameSettings.java`:109).
pub const LINES_OPEN: usize = 20;

/// The lifetime of a line in ticks: the fade's own 200
/// (`GuiNewChat.drawChat`:59-61), ten seconds at twenty ticks per second.
pub const LINE_LIFETIME: u64 = 200;

/// The bold style bit, set by `§l` (`FontRenderer.renderStringAtPos`:428-431).
pub const STYLE_BOLD: u8 = 1;

/// The italic style bit, set by `§o` (`FontRenderer.renderStringAtPos`:440-443).
pub const STYLE_ITALIC: u8 = 2;

/// The underlined style bit, set by `§n` (`FontRenderer.renderStringAtPos`:436-439).
pub const STYLE_UNDERLINED: u8 = 4;

/// The strikethrough style bit, set by `§m` (`FontRenderer.renderStringAtPos`:432-435).
pub const STYLE_STRIKETHROUGH: u8 = 8;

/// The obfuscated style bit, set by `§k` (`FontRenderer.renderStringAtPos`:424-427).
pub const STYLE_OBFUSCATED: u8 = 16;

/// The draw gate's floor: a line draws only while its alpha stays above 3
/// (`GuiNewChat.drawChat`:78) — alpha zero, were it drawn, would come
/// back opaque from the font renderer (`FontRenderer.java`:582-585).
const FADE_DRAW_FLOOR: u8 = 3;

/// The `§` code table in the source's own order (`FontRenderer.java`:400):
/// the sixteen colour codes, then the six style codes.
const CODES: &str = "0123456789abcdefklmnor";

/// One parsed chat component: a text run with a style, its events, and the
/// children that draw after it.
///
/// The fields are the source's `ChatComponentText` plus the resolved
/// `ChatStyle` (`ChatStyle.java`:129-172): the style a component carries here
/// is already inherited through the tree, so no `None`/`false` means
/// "inherit" — it means "in force". Unknown JSON keys are ignored and
/// wrong-typed ones degrade, per the module's hostile-input rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextComponent {
    /// The component's own text, `§` codes included as sent.
    pub text: String,
    /// The colour as the palette's index 0..16, when one is in force.
    pub colour: Option<u8>,
    /// Whether the text is bold.
    pub bold: bool,
    /// Whether the text is italic.
    pub italic: bool,
    /// Whether the text is underlined.
    pub underlined: bool,
    /// Whether the text is struck through.
    pub strikethrough: bool,
    /// Whether the text is obfuscated.
    pub obfuscated: bool,
    /// The click event in force, inherited from the nearest ancestor setting
    /// one.
    pub click: Option<ClickEvent>,
    /// The hover event in force, inherited the same way.
    pub hover: Option<HoverEvent>,
    /// The child components, drawn after this component's own text, depth
    /// first.
    pub children: Vec<TextComponent>,
}

/// The click actions this round acts on: the source's own gate keeps
/// `OPEN_URL`, `RUN_COMMAND` and `SUGGEST_COMMAND` for chat
/// (`ClickEvent.java`:102-105); the port narrows that to the three it can
/// perform, and every other action is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    /// `run_command`: send the value as a chat message.
    RunCommand,
    /// `suggest_command`: put the value into the input field.
    SuggestCommand,
    /// `open_url`: open the value as a link.
    OpenUrl,
}

/// One click event: the action and its value (`ClickEvent.java`:11-32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickEvent {
    /// What a click does.
    pub action: ClickAction,
    /// The action's argument.
    pub value: String,
}

/// One hover event. Only `show_text` is kept — the other actions
/// (`show_item`, `show_entity`, `show_achievement`, `HoverEvent.java`:86-89)
/// need surfaces this milestone does not build, so they are dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverEvent {
    /// `show_text`: the component a hover renders. The box is the type's own
    /// recursion breaker: a hover payload is a component, and a component can
    /// carry a hover.
    ShowText(Box<TextComponent>),
}

/// One flattened style run: the characters a single style state drew, the
/// `§` codes that selected it removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledRun {
    /// The run's characters, `§` codes removed.
    pub text: String,
    /// The colour in force, the palette's index 0..16; `None` is the draw's
    /// own colour, where the source keeps `this.red/green/blue`
    /// (`FontRenderer.java`:592-596).
    pub colour: Option<u8>,
    /// The style bits in force, the [`STYLE_BOLD`] family.
    pub styles: u8,
    /// The click event in force.
    pub click: Option<ClickEvent>,
    /// The hover event in force.
    pub hover: Option<HoverEvent>,
}

/// One line of the log: its split runs and the tick it was received at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedLine {
    /// The line's runs, in draw order.
    pub runs: Vec<StyledRun>,
    /// The tick counter the line was logged at (`ChatLine`'s
    /// `updateCounterCreated`, `ChatLine.java`:7-21).
    pub received_tick: u64,
}

/// One line a draw shows: its runs and the alpha the fade leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnLine<'a> {
    /// The line's runs, in draw order.
    pub runs: &'a [StyledRun],
    /// The alpha byte the fade arithmetic leaves, 255 while the chat is open.
    pub alpha: u8,
}

/// The chat log: the split lines, newest first, and the scroll state.
///
/// The retention is the source's (`GuiNewChat.java`:151-165): each split line
/// is pushed to the front, the list is capped at [`LOG_CAP`], and while the
/// chat is open and scrolled a new line pins the view. The fade and the
/// drawn-when-open counts are `drawChat`'s own (`:53-91`).
pub struct ChatLog {
    /// The lines, newest at the front.
    lines: std::collections::VecDeque<LoggedLine>,
    /// The line cap.
    cap: usize,
    /// The scroll offset in lines, `scrollPos`'s own.
    scroll: i32,
    /// Whether the view is scrolled and holding (`isScrolled`).
    scrolled: bool,
    /// Whether the chat window is open, `getChatOpen`'s own
    /// (`GuiNewChat.java`:305-308).
    open: bool,
    /// The tick the fade reads against, set by [`ChatLog::update`].
    tick: u64,
}

impl ChatLog {
    /// An empty log: capped at [`LOG_CAP`], closed, at tick zero.
    pub fn new() -> Self {
        Self {
            lines: std::collections::VecDeque::new(),
            cap: LOG_CAP,
            scroll: 0,
            scrolled: false,
            open: false,
            tick: 0,
        }
    }

    /// Sets whether the chat window is open — `getChatOpen`'s own
    /// (`GuiNewChat.java`:305-308), the window's state in this port. It picks
    /// the drawn line count and pins [`push`](Self::push)ed lines. Closing
    /// the window is the caller's moment to [`reset_scroll`](Self::reset_scroll),
    /// as the source's screen does on close (`GuiChat.java`:69-73).
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Ages the log to the tick the fade reads against — the drawn counter
    /// `drawChat` takes (`GuiNewChat.java`:59). Nothing is dropped by age: an
    /// open chat draws an old line at full alpha (`:61`, `:70-73`).
    pub fn update(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// Logs one message: the component is [`flatten`]ed and [`wrap`]ped at
    /// `width`, and each split line is pushed newest first with its receipt
    /// stamped `tick`. While the chat is open and scrolled, each push pins
    /// the view by one line (`GuiNewChat.java`:151-160); the log is capped at
    /// [`LOG_CAP`] (`:162-165`).
    pub fn push(&mut self, component: &TextComponent, tick: u64, width: i32, font: &Font) {
        for line in wrap(&flatten(component), width, font) {
            if self.open && self.scroll > 0 {
                self.scrolled = true;
                self.scroll(1);
            }
            self.lines.push_front(LoggedLine {
                runs: line,
                received_tick: tick,
            });
        }
        while self.lines.len() > self.cap {
            self.lines.pop_back();
        }
    }

    /// Scrolls the view by `amount` lines with the source's own clamp
    /// (`GuiNewChat.scroll`:222-237): the ceiling is the kept count minus the
    /// drawn count, and reaching zero clears the held flag.
    pub fn scroll(&mut self, amount: i32) {
        self.scroll += amount;
        let ceiling = self.lines.len() as i32 - Self::line_count(self.open) as i32;
        if self.scroll > ceiling {
            self.scroll = ceiling;
        }
        if self.scroll <= 0 {
            self.scroll = 0;
            self.scrolled = false;
        }
    }

    /// Resets the scroll: the source's own (`GuiNewChat.resetScroll`:211-215).
    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.scrolled = false;
    }

    /// The scroll offset in lines, `scrollPos`'s own.
    pub fn scroll_offset(&self) -> usize {
        self.scroll.max(0) as usize
    }

    /// Whether a [`push`](Self::push) pinned a scrolled view, `isScrolled`'s
    /// own (`GuiNewChat.java`:155).
    pub fn is_scrolled(&self) -> bool {
        self.scrolled
    }

    /// The kept lines, newest first — the source's `drawnChatLines` order
    /// (`GuiNewChat.java`:159).
    pub fn lines(&self) -> &std::collections::VecDeque<LoggedLine> {
        &self.lines
    }

    /// The number of kept lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether no line is kept.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The lines a draw shows: twenty while the chat is open, ten closed —
    /// `getLineCount`'s own (`GuiNewChat.java`:375-378; the box height
    /// `:368-373` at the two `chatHeight*` settings, and
    /// `GameSettings.java`:108-109).
    pub fn line_count(open: bool) -> usize {
        if open { LINES_OPEN } else { LINES_CLOSED }
    }

    /// The fade's alpha at `age` ticks — the source's own arithmetic
    /// (`GuiNewChat.drawChat`:59-73): `1 - age/200`, times ten, clamped to
    /// `0..=1`, squared, times 255 — and a full 255 while the chat is open.
    pub fn fade_alpha(age: u64, open: bool) -> u8 {
        let mut d0 = 1.0 - age as f64 / LINE_LIFETIME as f64;
        d0 *= 10.0;
        d0 = d0.clamp(0.0, 1.0);
        d0 *= d0;
        let alpha = (255.0 * d0) as u8;
        if open { 255 } else { alpha }
    }

    /// The lines a draw shows, newest first, each with its alpha: the
    /// focused-or-unfocused window from the scroll offset, an aged line
    /// still drawn while the chat is open, and a line dropped from the draw
    /// once its alpha no longer clears the draw gate
    /// (`GuiNewChat.drawChat`:53-91).
    pub fn drawn(&self) -> Vec<DrawnLine<'_>> {
        let mut drawn = Vec::new();
        let count = Self::line_count(self.open);
        for line in self.lines.iter().skip(self.scroll_offset()).take(count) {
            let age = self.tick.saturating_sub(line.received_tick);
            if !self.open && age >= LINE_LIFETIME {
                continue;
            }
            let alpha = Self::fade_alpha(age, self.open);
            if alpha <= FADE_DRAW_FLOOR {
                continue;
            }
            drawn.push(DrawnLine {
                runs: &line.runs,
                alpha,
            });
        }
        drawn
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new()
    }
}

/// The parse's hostile-input budget: a count of components left to build and
/// a count of text bytes left to take.
struct Budget {
    /// Components left to build.
    nodes: usize,
    /// Text bytes left to take.
    text_bytes: usize,
}

impl Budget {
    /// A full budget.
    fn new() -> Self {
        Self {
            nodes: MAX_COMPONENT_TOTAL,
            text_bytes: MAX_TEXT_BYTES,
        }
    }

    /// Takes `text` against the budget, clipped at a character boundary when
    /// it does not fit.
    fn take_text(&mut self, text: &str) -> String {
        let end = floor_char_boundary(text, text.len().min(self.text_bytes));
        self.text_bytes -= end;
        text[..end].to_owned()
    }
}

/// The largest character-boundary index at or below `index`.
fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Parses one chat message — the raw JSON of a chat packet — into a
/// [`TextComponent`].
///
/// The shapes are the source's (`IChatComponent.deserialize`:61-178): a bare
/// string is its own text, a primitive of any other kind reads through its
/// JSON text, an array is its elements in order, and an object takes `text`
/// with `extra` children and a style. A payload that does not parse as JSON
/// at all becomes a plain-text component of its own text, clipped like every
/// other.
pub fn parse_json(text: &str) -> TextComponent {
    let mut budget = Budget::new();
    match serde_json::from_str::<Value>(text) {
        Ok(value) => parse_value(&value, &TextComponent::default(), 0, &mut budget),
        Err(_) => TextComponent {
            text: budget.take_text(text),
            ..TextComponent::default()
        },
    }
}

/// A fresh component carrying `parent`'s style and nothing else: the shape
/// style inheritance gives a child before its own JSON is read
/// (`ChatStyle.getColor`:129-132 resolves through the parent chain).
fn inherit(parent: &TextComponent) -> TextComponent {
    TextComponent {
        text: String::new(),
        colour: parent.colour,
        bold: parent.bold,
        italic: parent.italic,
        underlined: parent.underlined,
        strikethrough: parent.strikethrough,
        obfuscated: parent.obfuscated,
        click: parent.click.clone(),
        hover: parent.hover.clone(),
        children: Vec::new(),
    }
}

/// Walks one JSON value into a component, style inherited from `parent`.
fn parse_value(
    value: &Value,
    parent: &TextComponent,
    depth: usize,
    budget: &mut Budget,
) -> TextComponent {
    if budget.nodes == 0 {
        return TextComponent::default();
    }
    budget.nodes -= 1;
    match value {
        Value::String(text) => TextComponent {
            text: budget.take_text(text),
            ..inherit(parent)
        },
        Value::Number(number) => TextComponent {
            text: budget.take_text(&number.to_string()),
            ..inherit(parent)
        },
        Value::Bool(flag) => TextComponent {
            text: budget.take_text(&flag.to_string()),
            ..inherit(parent)
        },
        Value::Array(items) => {
            let mut component = inherit(parent);
            for item in items {
                if budget.nodes == 0 {
                    break;
                }
                let child = parse_value(item, &component, depth + 1, budget);
                component.children.push(child);
            }
            component
        }
        Value::Object(map) => parse_object(map, parent, depth, budget),
        Value::Null => inherit(parent),
    }
}

/// Walks one JSON object into a component: `text`, the style keys, the events
/// and the `extra` children (`IChatComponent.deserialize`:97-177; the style keys
/// at `ChatStyle.java`:499-567).
fn parse_object(
    map: &serde_json::Map<String, Value>,
    parent: &TextComponent,
    depth: usize,
    budget: &mut Budget,
) -> TextComponent {
    let mut component = inherit(parent);
    if let Some(value) = map.get("text") {
        component.text = match value {
            Value::String(text) => budget.take_text(text),
            Value::Number(number) => budget.take_text(&number.to_string()),
            Value::Bool(flag) => budget.take_text(&flag.to_string()),
            _ => String::new(),
        };
    }
    if let Some(name) = map.get("color").and_then(Value::as_str) {
        if let Some(index) = colour_index(name) {
            component.colour = Some(index);
        }
    }
    if let Some(flag) = map.get("bold").and_then(Value::as_bool) {
        component.bold = flag;
    }
    if let Some(flag) = map.get("italic").and_then(Value::as_bool) {
        component.italic = flag;
    }
    if let Some(flag) = map.get("underlined").and_then(Value::as_bool) {
        component.underlined = flag;
    }
    if let Some(flag) = map.get("strikethrough").and_then(Value::as_bool) {
        component.strikethrough = flag;
    }
    if let Some(flag) = map.get("obfuscated").and_then(Value::as_bool) {
        component.obfuscated = flag;
    }
    if let Some(event) = map.get("clickEvent").and_then(Value::as_object) {
        if let Some(parsed) = parse_click(event, budget) {
            component.click = Some(parsed);
        }
    }
    if depth < MAX_COMPONENT_DEPTH {
        if let Some(event) = map.get("hoverEvent").and_then(Value::as_object) {
            if event.get("action").and_then(Value::as_str) == Some("show_text") {
                if let Some(value) = event.get("value") {
                    let shown = parse_value(value, &TextComponent::default(), depth + 1, budget);
                    component.hover = Some(HoverEvent::ShowText(Box::new(shown)));
                }
            }
        }
        if let Some(Value::Array(items)) = map.get("extra") {
            for item in items {
                if budget.nodes == 0 {
                    break;
                }
                let child = parse_value(item, &component, depth + 1, budget);
                component.children.push(child);
            }
        }
    }
    component
}

/// Reads a click event's `action` and `value` strings
/// (`ChatStyle.java`:534-550); the actions this port keeps are
/// [`ClickAction`]'s three, everything else — the source's own chat gate for
/// `OPEN_FILE` included (`ClickEvent.java`:102-105) — is dropped.
fn parse_click(event: &serde_json::Map<String, Value>, budget: &mut Budget) -> Option<ClickEvent> {
    let action = event.get("action").and_then(Value::as_str)?;
    let value = event.get("value").and_then(Value::as_str)?;
    let action = match action {
        "run_command" => ClickAction::RunCommand,
        "suggest_command" => ClickAction::SuggestCommand,
        "open_url" => ClickAction::OpenUrl,
        _ => return None,
    };
    Some(ClickEvent {
        action,
        value: budget.take_text(value),
    })
}

/// The palette index of a JSON colour name.
///
/// The name is normalised the way the source's lookup normalises it —
/// lower-cased with every non-letter removed (`EnumChatFormatting.java`:59-62)
/// — and matched against the sixteen colour names of the table
/// (`EnumChatFormatting.java`:12-27). Anything else, `reset` included, reads
/// as no colour.
fn colour_index(name: &str) -> Option<u8> {
    const NAMES: [&str; 16] = [
        "black",
        "dark_blue",
        "dark_green",
        "dark_aqua",
        "dark_red",
        "dark_purple",
        "gold",
        "gray",
        "dark_gray",
        "blue",
        "green",
        "aqua",
        "red",
        "light_purple",
        "yellow",
        "white",
    ];
    let normalised: String = name
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect();
    NAMES
        .iter()
        .position(|candidate| candidate.replace('_', "") == normalised)
        .map(|index| index as u8)
}

/// Flattens a component tree into styled runs, in draw order.
///
/// The order and the style resolution are the source's: a component's own
/// text first, then its children depth first
/// (`ChatComponentStyle.iterator`:64-67), each drawn with its resolved
/// style (`ChatComponentStyle.getFormattedText`:87-99), and a `§` code inside a text overrides from its position on.
pub fn flatten(component: &TextComponent) -> Vec<StyledRun> {
    let mut runs = Vec::new();
    flatten_into(component, &mut runs);
    runs
}

/// Flattens one component and its subtree into `runs`.
fn flatten_into(component: &TextComponent, runs: &mut Vec<StyledRun>) {
    decode_text(component, runs);
    for child in &component.children {
        flatten_into(child, runs);
    }
}

/// Splits one component's text at its `§` codes into runs, the component's
/// own style the state the first run starts in
/// (`FontRenderer.renderStringAtPos`:392-455).
fn decode_text(component: &TextComponent, runs: &mut Vec<StyledRun>) {
    let mut colour = component.colour;
    let mut styles = component_styles(component);
    let mut current = String::new();
    let mut characters = component.text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '§' {
            if let Some(code) = characters.peek().copied() {
                if !current.is_empty() {
                    runs.push(StyledRun {
                        text: std::mem::take(&mut current),
                        colour,
                        styles,
                        click: component.click.clone(),
                        hover: component.hover.clone(),
                    });
                }
                let code = code.to_ascii_lowercase();
                match CODES.find(code) {
                    Some(index) if index < 16 => {
                        colour = Some(index as u8);
                        styles = 0;
                    }
                    Some(16) => styles |= STYLE_OBFUSCATED,
                    Some(17) => styles |= STYLE_BOLD,
                    Some(18) => styles |= STYLE_STRIKETHROUGH,
                    Some(19) => styles |= STYLE_UNDERLINED,
                    Some(20) => styles |= STYLE_ITALIC,
                    Some(21) => {
                        colour = None;
                        styles = 0;
                    }
                    _ => {
                        colour = Some(15);
                        styles = 0;
                    }
                }
                characters.next();
                continue;
            }
        }
        current.push(character);
    }
    if !current.is_empty() {
        runs.push(StyledRun {
            text: current,
            colour,
            styles,
            click: component.click.clone(),
            hover: component.hover.clone(),
        });
    }
}

/// The style bits a component's flags carry.
fn component_styles(component: &TextComponent) -> u8 {
    let mut styles = 0;
    if component.bold {
        styles |= STYLE_BOLD;
    }
    if component.italic {
        styles |= STYLE_ITALIC;
    }
    if component.underlined {
        styles |= STYLE_UNDERLINED;
    }
    if component.strikethrough {
        styles |= STYLE_STRIKETHROUGH;
    }
    if component.obfuscated {
        styles |= STYLE_OBFUSCATED;
    }
    styles
}

/// Wraps styled runs into lines that fit `width`, measured by `font`.
///
/// The source's rules, from `GuiNewChat`'s own call into
/// `GuiUtilRenderComponents.splitText` (`GuiNewChat.java`:147-148;
/// `GuiUtilRenderComponents.java`:17-104): an explicit `\n` ends the line it
/// was found on; a piece that does not fit the line is cut at the last space
/// of its longest fitting prefix; an over-long word alone on its line moves
/// whole to the next one; and a word that still does not fit is hard-cut
/// where the budget runs out. Widths are the source's own `getStringWidth`
/// (`FontRenderer.java`:607-653) — a bold run measures a pixel per character
/// heavier — and the cut its `trimStringToWidth` (`:708-768`).
///
/// The source splits the components it was handed; this splits the runs
/// [`flatten`] made out of them, so a run keeps its own style across a split.
/// The chat box's budget at the default settings is [`CHAT_WIDTH`].
pub fn wrap(runs: &[StyledRun], width: i32, font: &Font) -> Vec<Vec<StyledRun>> {
    let mut pieces: Vec<StyledRun> = runs.to_vec();
    let mut lines: Vec<Vec<StyledRun>> = Vec::new();
    let mut line: Vec<StyledRun> = Vec::new();
    let mut used = 0;
    let mut index = 0;
    while index < pieces.len() {
        let fit = fit(&pieces[index], used, width, font);
        if let Some(tail) = fit.newline_tail {
            pieces.insert(index + 1, tail);
        }
        if let Some(tail) = fit.cut_tail {
            pieces.insert(index + 1, tail);
        }
        if !fit.head.text.is_empty() {
            used += measured_width(font, &fit.head.text, fit.head.styles & STYLE_BOLD != 0);
            line.push(fit.head);
        }
        if fit.flush {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        index += 1;
    }
    lines.push(line);
    lines
}

/// One piece's fit against the line's remaining budget.
struct Fit {
    /// What the piece adds to the current line, a trailing `\n` removed.
    head: StyledRun,
    /// The part after the piece's first newline, inserted behind the head.
    newline_tail: Option<StyledRun>,
    /// The part the over-budget cut left over, inserted right behind the
    /// head, before the newline's remainder.
    cut_tail: Option<StyledRun>,
    /// Whether the line ends after this head.
    flush: bool,
}

/// Fits one piece against the line's remaining budget, cutting it the way
/// `splitText`'s loop cuts a component (`GuiUtilRenderComponents.java`:24-99).
fn fit(piece: &StyledRun, used: i32, width: i32, font: &Font) -> Fit {
    let bold = piece.styles & STYLE_BOLD != 0;
    // :30-39 — the piece splits at its first newline: the head up to and
    // including it stays, an equal-styled piece takes the rest, and the line
    // flushes after the head.
    let mut head = piece.clone();
    let mut newline_tail = None;
    let mut flush = false;
    if let Some(index) = piece.text.find('\n') {
        flush = true;
        head.text.truncate(index + 1);
        let mut tail = piece.clone();
        tail.text = piece.text[index + 1..].to_owned();
        if !tail.text.is_empty() {
            newline_tail = Some(tail);
        }
    }
    // :41-43 — a fitting head draws without its trailing newline.
    let delta = measured_width(
        font,
        head.text.strip_suffix('\n').unwrap_or(&head.text),
        bold,
    );
    let mut cut_tail = None;
    if used + delta > width {
        // :49-76 — trim at the budget and hand the rest back as a piece.
        let kept = trim_to_width(font, &head.text, width - used, bold);
        let mut keep = head.text[..kept].to_owned();
        let mut rest = head.text[kept..].to_owned();
        if !rest.is_empty() {
            // :54-66 — the last space of the kept text takes the rest of the
            // piece with it when something draws before it.
            let moved = match keep.rfind(' ') {
                Some(space) if measured_width(font, &keep[..space], bold) > 0 => {
                    rest = head.text[space..].to_owned();
                    keep.truncate(space);
                    true
                }
                _ => false,
            };
            // :67-71 — a word alone on its line that still does not fit
            // moves whole to the next.
            if !moved && used > 0 && !head.text.contains(' ') {
                rest = head.text.clone();
                keep.clear();
            }
            if !rest.is_empty() {
                let mut tail = piece.clone();
                tail.text = rest;
                cut_tail = Some(tail);
            }
        }
        head.text = keep;
        // The source re-measures here for its append guard (`:78`, `:84`);
        // a trimmed head always fits the budget it was trimmed to, so the
        // append itself stands.
        flush = true; // :81
    }
    // The trailing newline is the line break, never drawn (`:42`).
    if head.text.ends_with('\n') {
        head.text.pop();
    }
    Fit {
        head,
        newline_tail,
        cut_tail,
        flush,
    }
}

/// The width `text` adds to a line, measured the way the source measures a
/// component: its formatting prefix joined to its text
/// (`GuiUtilRenderComponents.java`:41-43), so a bold text measures a pixel per
/// character heavier. The lone `§` a text can end on keeps the source's
/// minus-one quirk (`FontRenderer.getStringWidth`:607-653).
fn measured_width(font: &Font, text: &str, bold: bool) -> i32 {
    if bold {
        string_width(font, &format!("§l{text}"))
    } else {
        string_width(font, text)
    }
}

/// The byte length of the longest prefix of `text` that fits in `width`,
/// with the source's own cut (`FontRenderer.trimStringToWidth`:708-768): a
/// character that would pass the budget is not taken, and the character after
/// a `§` is consumed as a code — zero width, and the width's bold flag turns
/// on at `l`, off only at `r`.
fn trim_to_width(font: &Font, text: &str, width: i32, bold: bool) -> usize {
    let mut used = 0;
    let mut pending = false;
    let mut weighted = bold;
    let mut kept = 0;
    for (index, character) in text.char_indices() {
        if used >= width {
            break;
        }
        let advance = if character == '§' {
            -1
        } else {
            font.advance(character) as i32
        };
        if pending {
            pending = false;
            if character != 'l' && character != 'L' {
                if character == 'r' || character == 'R' {
                    weighted = false;
                }
            } else {
                weighted = true;
            }
        } else if advance < 0 {
            pending = true;
        } else {
            used += advance;
            if weighted {
                used += 1;
            }
        }
        if used > width {
            break;
        }
        kept = index + character.len_utf8();
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxide_assets::texture::Texture;

    /// A 128x128 synthetic sheet whose `'A'` cell is inked in columns 0..=4:
    /// the same metric the render-side text tests measure with, so `'A'`
    /// advances six font pixels, the space the source's own 4, and every other
    /// blank cell 1.
    fn font() -> Font {
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
        let texture = Texture {
            width: SIDE,
            height: SIDE,
            rgba,
        };
        Font::load(&texture, None).expect("the synthetic sheet loads")
    }

    /// A plain run of text with the given style, no events.
    fn run(text: &str, colour: Option<u8>, styles: u8) -> StyledRun {
        StyledRun {
            text: text.to_owned(),
            colour,
            styles,
            click: None,
            hover: None,
        }
    }

    /// The concatenated text of one wrapped line.
    fn line_text(line: &[StyledRun]) -> String {
        line.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn every_style_flag_sets_its_own_style_bit() {
        for (key, bit) in [
            ("bold", STYLE_BOLD),
            ("italic", STYLE_ITALIC),
            ("underlined", STYLE_UNDERLINED),
            ("strikethrough", STYLE_STRIKETHROUGH),
            ("obfuscated", STYLE_OBFUSCATED),
        ] {
            let component = parse_json(&format!("{{\"text\":\"x\",\"{key}\":true}}"));
            let runs = flatten(&component);
            assert_eq!(runs.len(), 1, "one run for {key}");
            assert_eq!(runs[0].styles, bit, "the {key} bit alone");
        }
        let all = parse_json(
            "{\"text\":\"x\",\"bold\":true,\"italic\":true,\"underlined\":true,\
             \"strikethrough\":true,\"obfuscated\":true}",
        );
        assert!(all.bold && all.italic && all.underlined && all.strikethrough && all.obfuscated);
        assert_eq!(
            flatten(&all)[0].styles,
            STYLE_BOLD | STYLE_ITALIC | STYLE_UNDERLINED | STYLE_STRIKETHROUGH | STYLE_OBFUSCATED
        );
    }

    #[test]
    fn children_inherit_the_nesting_styles() {
        let component = parse_json(
            "{\"text\":\"a\",\"color\":\"red\",\"bold\":true,\"extra\":[\
             {\"text\":\"b\"},{\"text\":\"c\",\"italic\":true,\"extra\":[{\"text\":\"d\"}]}]}",
        );
        let runs = flatten(&component);
        let texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c", "d"]);
        for run in &runs {
            assert_eq!(run.colour, Some(12), "red inherits into {}", run.text);
            assert_eq!(
                run.styles & STYLE_BOLD,
                STYLE_BOLD,
                "bold inherits into {}",
                run.text
            );
        }
        assert_eq!(runs[2].styles, STYLE_BOLD | STYLE_ITALIC);
        assert_eq!(runs[3].styles, STYLE_BOLD | STYLE_ITALIC);
    }

    #[test]
    fn a_colour_override_at_each_level_wins() {
        let component = parse_json(
            "{\"text\":\"a\",\"color\":\"red\",\"extra\":[\
             {\"text\":\"b\",\"color\":\"blue\"},{\"text\":\"c\"}]}",
        );
        let runs = flatten(&component);
        assert_eq!(runs[0].colour, Some(12), "red");
        assert_eq!(runs[1].colour, Some(9), "blue");
        assert_eq!(runs[2].colour, Some(12), "the sibling still inherits red");
        // An explicit false clears an inherited flag; a wrong-typed one keeps it.
        let component = parse_json(
            "{\"text\":\"a\",\"bold\":true,\"extra\":[\
             {\"text\":\"b\",\"bold\":false},{\"text\":\"c\",\"bold\":\"yes\"}]}",
        );
        let runs = flatten(&component);
        assert_eq!(
            runs[1].styles, 0,
            "explicit false cleared the inherited bold"
        );
        assert_eq!(
            runs[2].styles, STYLE_BOLD,
            "a wrong-typed flag kept the inherited bold"
        );
    }

    #[test]
    fn click_and_hover_payloads_land_on_the_runs() {
        let component = parse_json(
            "{\"text\":\"x\",\"clickEvent\":{\"action\":\"run_command\",\"value\":\"/say hi\"},\
             \"hoverEvent\":{\"action\":\"show_text\",\"value\":{\"text\":\"tip\",\"color\":\"gray\"}},\
             \"extra\":[{\"text\":\"y\"}]}",
        );
        let runs = flatten(&component);
        assert_eq!(
            runs[0].click,
            Some(ClickEvent {
                action: ClickAction::RunCommand,
                value: "/say hi".into(),
            })
        );
        assert_eq!(
            runs[1].click, runs[0].click,
            "the click inherits into children"
        );
        match &runs[0].hover {
            Some(HoverEvent::ShowText(shown)) => {
                assert_eq!(shown.text, "tip");
                assert_eq!(shown.colour, Some(7), "gray");
            }
            other => panic!("show_text expected: {other:?}"),
        }
        let suggest = parse_json(
            "{\"text\":\"x\",\"clickEvent\":{\"action\":\"suggest_command\",\"value\":\"/msg \"}}",
        );
        assert_eq!(
            flatten(&suggest)[0].click,
            Some(ClickEvent {
                action: ClickAction::SuggestCommand,
                value: "/msg ".into(),
            })
        );
        let open = parse_json(
            "{\"text\":\"x\",\"clickEvent\":{\"action\":\"open_url\",\"value\":\"https://example.com\"}}",
        );
        assert_eq!(
            flatten(&open)[0].click,
            Some(ClickEvent {
                action: ClickAction::OpenUrl,
                value: "https://example.com".into(),
            })
        );
    }

    #[test]
    fn unknown_click_and_hover_actions_are_dropped() {
        for action in ["open_file", "twitch_user_info", "change_page", "garbage"] {
            let component = parse_json(&format!(
                "{{\"text\":\"x\",\"clickEvent\":{{\"action\":\"{action}\",\"value\":\"v\"}}}}"
            ));
            assert_eq!(
                flatten(&component)[0].click,
                None,
                "click {action} is not kept"
            );
        }
        for action in ["show_item", "show_entity", "show_achievement", "garbage"] {
            let component = parse_json(&format!(
                "{{\"text\":\"x\",\"hoverEvent\":{{\"action\":\"{action}\",\"value\":\"v\"}}}}"
            ));
            assert_eq!(
                flatten(&component)[0].hover,
                None,
                "hover {action} is not kept"
            );
        }
        // A dropped child event leaves the inherited one in force.
        let component = parse_json(
            "{\"text\":\"a\",\"clickEvent\":{\"action\":\"run_command\",\"value\":\"/x\"},\
             \"extra\":[{\"text\":\"b\",\"clickEvent\":{\"action\":\"open_file\",\"value\":\"f\"}}]}",
        );
        assert_eq!(
            flatten(&component)[1].click,
            Some(ClickEvent {
                action: ClickAction::RunCommand,
                value: "/x".into(),
            })
        );
    }

    #[test]
    fn a_deep_payload_stops_at_the_depth_cap_or_before_it() {
        // One bomb builder for both cases: an `extra` chain `levels` long,
        // which is two JSON containers deep per level.
        fn bomb(levels: usize) -> String {
            let mut json = String::from("{\"text\":\"a\"");
            for _ in 0..levels {
                json.push_str(",\"extra\":[{\"text\":\"a\"");
            }
            for _ in 0..levels {
                json.push_str("}]");
            }
            json.push('}');
            json
        }
        fn count(component: &TextComponent) -> usize {
            1 + component.children.iter().map(count).sum::<usize>()
        }
        // Sixty levels pass the json crate's own nesting limit, and the walk
        // cuts the chain at the cap: the root plus sixteen steps.
        let component = parse_json(&bomb(60));
        assert_eq!(
            count(&component),
            17,
            "the root plus the cap's sixteen steps"
        );
        assert_eq!(
            flatten(&component).len(),
            17,
            "the root plus one level per cap step"
        );
        // Past the json crate's own limit the payload is refused whole and
        // degrades to its raw text — never walked, never a panic.
        let component = parse_json(&bomb(200));
        assert_eq!(count(&component), 1);
        assert_eq!(component.text, bomb(200));
    }

    #[test]
    fn a_two_hundred_kilobyte_string_clips_at_the_total() {
        let big = "a".repeat(200_000);
        let component = parse_json(&format!("\"{big}\""));
        assert_eq!(component.text.len(), MAX_TEXT_BYTES);
        assert!(big.starts_with(&component.text));
        // The clip backs up to a character boundary: with the budget one short
        // of the two-byte `é`, the run of `a`s ends where the `é` would start.
        let mixed = format!("{}{}", "a".repeat(MAX_TEXT_BYTES - 1), "ééé");
        let clipped = parse_json(&format!("\"{mixed}\"")).text;
        assert_eq!(clipped.len(), MAX_TEXT_BYTES - 1);
        assert!(clipped.ends_with('a'));
    }

    #[test]
    fn malformed_json_becomes_a_plain_text_component() {
        let raw = "{\"text\":\"unterminated";
        let component = parse_json(raw);
        assert_eq!(component.text, raw);
        assert_eq!(component.colour, None);
        assert!(!component.bold && !component.italic && !component.underlined);
        assert!(!component.strikethrough && !component.obfuscated);
        assert!(component.children.is_empty());
        // The fallback clips like every other text: a bare word is not JSON.
        let big = "x".repeat(200_000);
        assert_eq!(parse_json(&big).text.len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn bare_strings_primitives_and_arrays_take_the_sources_shapes() {
        assert_eq!(parse_json("\"hello\"").text, "hello");
        assert_eq!(parse_json("42").text, "42");
        assert_eq!(parse_json("true").text, "true");
        let runs = flatten(&parse_json("[\"a\",{\"text\":\"b\"}]"));
        let texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(texts, ["a", "b"]);
    }

    #[test]
    fn section_codes_inside_text_override_the_json_style() {
        // A colour code beats the component's own colour; §r returns the run
        // to the draw's base.
        let runs = flatten(&parse_json("{\"text\":\"a§bb§r\",\"color\":\"red\"}"));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], run("a", Some(12), 0));
        assert_eq!(runs[1], run("b", Some(11), 0));
        // A style code applies to the following runs; a colour code clears the
        // styles a §l had set.
        let runs = flatten(&parse_json("{\"text\":\"a§l§bb§lc\",\"color\":\"red\"}"));
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0], run("a", Some(12), 0));
        assert_eq!(runs[1], run("b", Some(11), 0));
        assert_eq!(runs[2], run("c", Some(11), STYLE_BOLD));
        // The codes read through their upper-case forms.
        let runs = flatten(&parse_json("{\"text\":\"§LX\"}"));
        assert_eq!(runs, vec![run("X", None, STYLE_BOLD)]);
        // An unknown code is the source's clamp to white with the styles
        // cleared.
        let runs = flatten(&parse_json(
            "{\"text\":\"§za\",\"color\":\"red\",\"bold\":true}",
        ));
        assert_eq!(runs, vec![run("a", Some(15), 0)]);
        // A trailing § with nothing after it is ordinary text.
        assert_eq!(flatten(&parse_json("{\"text\":\"x§\"}"))[0].text, "x§");
    }

    #[test]
    fn wrong_types_degrade_and_unknown_keys_are_ignored() {
        assert_eq!(parse_json("{\"text\":42}").text, "42");
        assert_eq!(parse_json("{\"text\":true}").text, "true");
        assert_eq!(parse_json("{\"text\":{\"a\":1}}").text, "");
        assert_eq!(parse_json("{\"text\":[\"a\"]}").text, "");
        let component = parse_json(
            "{\"text\":\"x\",\"bold\":\"yes\",\"color\":5,\"frobnicate\":{\"deep\":[1,2]},\
             \"extra\":[]}",
        );
        assert!(!component.bold);
        assert_eq!(component.colour, None);
        assert_eq!(component.text, "x");
        assert!(component.children.is_empty());
        // A wrong-typed clickEvent is dropped, not inherited from nowhere.
        let component = parse_json("{\"text\":\"x\",\"clickEvent\":\"nope\"}");
        assert_eq!(flatten(&component)[0].click, None);
    }

    #[test]
    fn a_whole_tree_total_caps_the_components() {
        let mut json = String::from("{\"text\":\"r\",\"extra\":[");
        for index in 0..MAX_COMPONENT_TOTAL + 100 {
            if index > 0 {
                json.push(',');
            }
            json.push_str("{\"text\":\"s\"}");
        }
        json.push_str("]}");
        let component = parse_json(&json);
        fn count(component: &TextComponent) -> usize {
            1 + component.children.iter().map(count).sum::<usize>()
        }
        assert_eq!(count(&component), MAX_COMPONENT_TOTAL);
    }
    #[test]
    fn the_wrap_breaks_at_the_last_fitting_space() {
        // "aaaa " is 8 px ('a' = 1, space = 4); forty of them reach exactly
        // 320 and the forty-first tips the piece over. The trim stops at the
        // budget, the split backs up to the last space, and — the chat's own
        // `p3 = false` — the tail keeps that space.
        let runs = vec![run(&"aaaa ".repeat(41), None, 0)];
        let lines = wrap(&runs, 320, &font());
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "aaaa ".repeat(39) + "aaaa");
        assert_eq!(line_text(&lines[1]), " aaaa ");
        assert_eq!(lines[1], vec![run(" aaaa ", None, 0)]);
    }

    #[test]
    fn a_line_fits_at_exactly_the_budget() {
        // 53 'A's reach 318 font pixels; "aa" takes the line to exactly 320,
        // which fits (`i + i1 <= budget` appends); the next "a" breaks.
        let runs = vec![run(&("A".repeat(53) + "aaa"), None, 0)];
        let lines = wrap(&runs, 320, &font());
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "A".repeat(53) + "aa");
        assert_eq!(line_text(&lines[1]), "a");
        // And a piece whose own width is exactly the budget is one line.
        let runs = vec![run(&("A".repeat(53) + "aa"), None, 0)];
        let lines = wrap(&runs, 320, &font());
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "A".repeat(53) + "aa");
    }

    #[test]
    fn an_overlong_word_moves_to_the_next_line_and_then_hard_cuts() {
        // "short " is 9 px on the line; the 360-px word has no space to break
        // at, so it moves whole to a fresh line, where it is hard-cut at 318.
        let runs = vec![run("short ", None, 0), run(&"A".repeat(60), None, 0)];
        let lines = wrap(&runs, 320, &font());
        assert_eq!(lines.len(), 3);
        assert_eq!(line_text(&lines[0]), "short ");
        assert_eq!(line_text(&lines[1]), "A".repeat(53));
        assert_eq!(line_text(&lines[2]), "A".repeat(7));
    }

    #[test]
    fn an_explicit_newline_flushes_the_line() {
        let lines = wrap(&[run("one\ntwo", None, 0)], 320, &font());
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "one");
        assert_eq!(line_text(&lines[1]), "two");
        // A leading newline flushes an empty line first; the newline itself
        // never draws.
        let lines = wrap(&[run("\nxx", None, 0)], 320, &font());
        assert_eq!(lines.len(), 2);
        assert!(lines[0].is_empty(), "the flushed line is empty");
        assert_eq!(line_text(&lines[1]), "xx");
    }

    #[test]
    fn a_bold_run_keeps_its_style_across_the_break() {
        // Bold adds one pixel per drawn character (the source's own +1), so 45
        // 'A's reach 315 and the 46th would pass 320. Both halves of the break
        // keep the run's colour and flags, and the next run keeps its own.
        let bold = run(&"A".repeat(60), Some(4), STYLE_BOLD);
        let lines = wrap(&[bold, run("tail", None, 0)], 320, &font());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], vec![run(&"A".repeat(45), Some(4), STYLE_BOLD)]);
        assert_eq!(
            lines[1],
            vec![
                run(&"A".repeat(15), Some(4), STYLE_BOLD),
                run("tail", None, 0)
            ]
        );
    }
    #[test]
    fn the_log_caps_its_split_lines_at_a_hundred() {
        // 5363 'A's split into 101 lines of 53 and a last line of 10.
        let component = parse_json(&format!("\"{}\"", "A".repeat(53 * 101 + 10)));
        let mut log = ChatLog::new();
        log.push(&component, 7, 320, &font());
        assert_eq!(log.lines().len(), LOG_CAP);
        assert_eq!(log.lines().front().unwrap().runs[0].text, "A".repeat(10));
        assert_eq!(
            log.lines().back().unwrap().runs[0].text,
            "A".repeat(53),
            "the two oldest split lines fell off the back"
        );
        assert!(log.lines().iter().all(|line| line.received_tick == 7));
    }

    #[test]
    fn a_push_splits_the_component_at_the_budget() {
        let component = parse_json(&format!("\"{}\"", "aaaa ".repeat(41)));
        let mut log = ChatLog::new();
        log.push(&component, 3, 320, &font());
        let lines: Vec<String> = log
            .lines()
            .iter()
            .map(|line| line_text(&line.runs))
            .collect();
        // The split lines push newest-first inside the message too: the
        // wrap's last line sits at the front and its first at the back
        // (`GuiNewChat.setChatLine`:151-160 adds each at index zero).
        assert_eq!(
            lines,
            vec![" aaaa ".to_owned(), "aaaa ".repeat(39) + "aaaa"]
        );
    }

    #[test]
    fn the_fade_is_the_sources_arithmetic() {
        // The four pinned points: creation, the last fully drawn tick at the
        // boundary, the mid-fade, and the last tick a closed chat draws.
        assert_eq!(ChatLog::fade_alpha(0, false), 255);
        assert_eq!(ChatLog::fade_alpha(179, false), 255);
        assert_eq!(
            ChatLog::fade_alpha(180, false),
            254,
            "the first non-full tick"
        );
        assert_eq!(ChatLog::fade_alpha(190, false), 63, "mid-fade");
        assert_eq!(ChatLog::fade_alpha(199, false), 0, "gone");
        // The quadratic tail either side of the pins.
        assert_eq!(ChatLog::fade_alpha(181, false), 230);
        assert_eq!(ChatLog::fade_alpha(185, false), 143);
        assert_eq!(ChatLog::fade_alpha(195, false), 15);
        assert_eq!(
            ChatLog::fade_alpha(197, false),
            5,
            "the last drawn tick closed"
        );
        assert_eq!(ChatLog::fade_alpha(198, false), 2, "below the draw gate");
        // An open chat pins every line to full alpha.
        assert_eq!(ChatLog::fade_alpha(0, true), 255);
        assert_eq!(ChatLog::fade_alpha(300, true), 255);
    }

    #[test]
    fn a_closed_log_draws_only_the_lines_the_fade_leaves() {
        let component = parse_json("\"x\"");
        let mut log = ChatLog::new();
        log.push(&component, 100, 320, &font());
        log.update(100);
        let drawn = log.drawn();
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].alpha, 255);
        // Age 197 is the last closed-chat draw; 198's alpha 2 fails the
        // `l1 > 3` gate of the same block.
        log.update(297);
        let drawn = log.drawn();
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].alpha, 5);
        log.update(298);
        assert!(log.drawn().is_empty());
        // The line itself is never dropped by age, and an open chat draws it
        // at full alpha again.
        log.update(400);
        assert!(log.drawn().is_empty());
        assert_eq!(log.lines().len(), 1);
        log.set_open(true);
        let drawn = log.drawn();
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].alpha, 255);
    }

    #[test]
    fn the_scroll_offset_clamps_to_the_source_rule() {
        let component = parse_json("\"x\"");
        let mut log = ChatLog::new();
        for _ in 0..30 {
            log.push(&component, 0, 320, &font());
        }
        log.set_open(true);
        log.scroll(5);
        assert_eq!(log.scroll_offset(), 5);
        assert!(
            !log.is_scrolled(),
            "only a pinned push sets the scrolled flag"
        );
        assert_eq!(
            log.drawn().len(),
            LINES_OPEN,
            "the open draw shows a full window"
        );
        // Open, the ceiling is size - 20 (the focused line count); closed, it
        // is size - 10.
        log.scroll(100);
        assert_eq!(log.scroll_offset(), 10);
        log.set_open(false);
        log.scroll(100);
        assert_eq!(log.scroll_offset(), 20);
        log.scroll(-1_000);
        assert_eq!(log.scroll_offset(), 0);
        assert!(!log.is_scrolled());
    }

    #[test]
    fn a_new_line_pins_a_scrolled_view() {
        let component = parse_json("\"x\"");
        let mut log = ChatLog::new();
        for _ in 0..30 {
            log.push(&component, 0, 320, &font());
        }
        log.set_open(true);
        log.scroll(5);
        log.push(&component, 1, 320, &font());
        assert_eq!(log.scroll_offset(), 6, "the push carried the view along");
        assert!(log.is_scrolled());
        // Not scrolled, the push leaves the offset alone.
        log.reset_scroll();
        log.push(&component, 2, 320, &font());
        assert_eq!(log.scroll_offset(), 0);
        assert!(!log.is_scrolled());
    }
}
