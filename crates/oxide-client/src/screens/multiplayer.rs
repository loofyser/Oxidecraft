//! The multiplayer server list: the `servers.toml` file model, the list screen with its
//! add/edit/direct-connect sub-screens, and the live-ping worker.
//!
//! The layout follows `GuiMultiplayer.java`: the list spans rows of 36 px from 32 down to
//! `height - 64`, and the seven buttons carry the source's ids and rects (`createButtons`).
//! The ping follows `OldServerPinger.java`: one worker thread per refresh, results applied
//! by index, the latency icon's level table (`ServerListEntryNormal.java`).
//!
//! The list file is this port's own line format under `<store>/servers.toml`, sorted keys,
//! atomic temp+rename write (the `options.toml` precedent); `servers.dat` is never read or
//! written. A missing file is an empty list; hostile bytes are refused with documented caps.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use oxide_assets::font::Font;
use oxide_game::chat::{self, LanguageTable};
use oxide_proto_v47::status::{PingError, ping_with_latency};
use oxide_render::hud::{HudDraw, HudTexture, ScaledResolution};
use oxide_render::text::string_width;

use super::book::wrap_lines;
use super::widgets::Button;

/// The server list's file name under the store root.
pub const SERVERS_FILE: &str = "servers.toml";

/// How many servers the file may hold; past that, entries are refused.
pub const MAX_SERVERS: usize = 512;
/// How large the file may be before the load refuses it outright.
pub const MAX_FILE_BYTES: u64 = 1 << 20;
/// How long one stored name may be, in bytes.
pub const MAX_NAME_BYTES: usize = 512;
/// How long one stored address may be, in bytes.
pub const MAX_ADDRESS_BYTES: usize = 512;
/// How long one stored icon string may be, in bytes.
pub const MAX_ICON_BYTES: usize = 1 << 16;

/// The list's top edge in GUI pixels (`GuiMultiplayer.java`, `initGui`).
pub const LIST_TOP: i32 = 32;
/// The row height in GUI pixels (the `ServerSelectionList` slot height).
pub const ROW_HEIGHT: i32 = 36;
/// The list's bottom margin in GUI pixels (`initGui`).
pub const LIST_BOTTOM_OFFSET: i32 = 64;

/// One refresh's shared attempt timeout: every row's ping draws on it, so a
/// list of slow servers cannot hold the worker past it (the `status_response.rs`
/// canned value).
pub const PING_TIMEOUT: Duration = Duration::from_secs(5);

/// The name field's max length in characters (`GuiTextField`'s own default: the
/// source sets a max only on the address field).
pub const NAME_MAX_CHARS: usize = 32;
/// The address field's max length in characters (`GuiScreenAddServer.java`, `initGui`).
pub const ADDRESS_MAX_CHARS: usize = 128;

/// The icons sheet behind the latency bars (`Gui.icons`).
pub const ICONS_SHEET: &str = "gui/icons";
/// The latency bar cell: 10 wide, 8 tall, at `(k * 10, 176 + l * 8)`
/// (`ServerListEntryNormal.java`, `drawEntry`).
const BAR_W: f32 = 10.0;
const BAR_H: f32 = 8.0;
const BAR_V0: f32 = 176.0;

/// One stored server: the name and address the list shows, and the ping's base64 icon
/// string when the server sent one (`ServerData.java`, without the `data:` prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    /// The display name.
    pub name: String,
    /// The address as typed (`host`, `host:port` or `[v6]:port`).
    pub address: String,
    /// The icon's base64 bytes, without the `data:image/png;base64,` prefix.
    pub icon: Option<String>,
}

impl ServerEntry {
    /// Builds one entry.
    pub fn new(name: String, address: String, icon: Option<String>) -> Self {
        Self {
            name,
            address,
            icon,
        }
    }
}

/// The stored server list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerList {
    /// The entries in list order.
    pub entries: Vec<ServerEntry>,
}

impl ServerList {
    /// Loads the list: a missing or unreadable file is empty, unknown keys are
    /// ignored, malformed lines are skipped, and over-cap fields refuse their
    /// entry (`ServerList.java`, `loadServerList`, through this port's own
    /// format). Never panics, whatever the file holds.
    pub fn load(path: &Path) -> Self {
        let bytes = fs::read(path).unwrap_or_default();
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Self::default();
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut rows: BTreeMap<usize, PartialEntry> = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Some((index, field)) = split_key(key.trim()) else {
                continue;
            };
            if index > MAX_SERVERS {
                continue;
            }
            let Some(value) = unquote(value.trim()) else {
                continue;
            };
            let row = rows.entry(index).or_default();
            match field {
                "name" => row.name = Some(value),
                "address" => row.address = Some(value),
                "icon" => row.icon = Some(value),
                _ => {}
            }
        }
        let mut list = Self::default();
        for (_, row) in rows {
            let Some(entry) = row.complete() else {
                continue;
            };
            if list.entries.len() >= MAX_SERVERS {
                break;
            }
            list.entries.push(entry);
        }
        list
    }

    /// Saves the list with sorted keys: the bytes land in a sibling temporary
    /// file first and the rename publishes them, so a crash never leaves a
    /// half-written file behind (the `options.toml` precedent).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut text = String::new();
        for (index, entry) in self.entries.iter().enumerate() {
            text.push_str(&format!("server.{index}.name = {}\n", quote(&entry.name)));
            text.push_str(&format!(
                "server.{index}.address = {}\n",
                quote(&entry.address)
            ));
            if let Some(icon) = entry.icon.as_ref() {
                text.push_str(&format!("server.{index}.icon = {}\n", quote(icon)));
            }
        }
        let staged = path.with_extension("tmp");
        fs::write(&staged, text)?;
        if let Err(error) = fs::rename(&staged, path) {
            let _ = fs::remove_file(&staged);
            return Err(error);
        }
        Ok(())
    }

    /// Appends one entry (`ServerList.java`, `addServerData`).
    pub fn add(&mut self, entry: ServerEntry) {
        self.entries.push(entry);
    }

    /// Removes the entry at `index` (`ServerList.java`, `removeServerData`).
    pub fn remove(&mut self, index: usize) {
        self.entries.remove(index);
    }

    /// Swaps two entries (`ServerList.java`, `swapServers`, without its inline
    /// save: the caller saves, so one reorder is one write).
    pub fn swap(&mut self, first: usize, second: usize) {
        self.entries.swap(first, second);
    }
}

/// One file row while it is still missing fields.
#[derive(Debug, Default)]
struct PartialEntry {
    /// The stored name, when seen.
    name: Option<String>,
    /// The stored address, when seen.
    address: Option<String>,
    /// The stored icon, when seen.
    icon: Option<String>,
}

impl PartialEntry {
    /// Completes the row into an entry: both fields present, every present
    /// field within its cap. Anything else refuses the entry.
    fn complete(self) -> Option<ServerEntry> {
        let name = self.name?;
        let address = self.address?;
        if name.len() > MAX_NAME_BYTES || address.len() > MAX_ADDRESS_BYTES {
            return None;
        }
        if self
            .icon
            .as_ref()
            .is_some_and(|icon| icon.len() > MAX_ICON_BYTES)
        {
            return None;
        }
        Some(ServerEntry {
            name,
            address,
            icon: self.icon,
        })
    }
}

/// Splits a `server.<index>.<field>` key; anything else is not a row.
fn split_key(key: &str) -> Option<(usize, &str)> {
    let rest = key.strip_prefix("server.")?;
    let (index, field) = rest.split_once('.')?;
    if field.is_empty() {
        return None;
    }
    Some((index.parse().ok()?, field))
}

/// Unquotes a saved string; anything malformed is refused (the `options.toml`
/// precedent).
fn unquote(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
        return None;
    }
    let body = &trimmed[1..trimmed.len() - 1];
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(char) = chars.next() {
        if char == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => return None,
            }
        } else {
            out.push(char);
        }
    }
    Some(out)
}

/// Quotes a string for the file (the `options.toml` precedent).
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for char in text.chars() {
        match char {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(char),
        }
    }
    out.push('"');
    out
}

/// The list file under a store root.
pub fn servers_path(store_root: &Path) -> PathBuf {
    store_root.join(SERVERS_FILE)
}

/// Splits a typed address into host and port (`ServerAddress.java`, `fromString`):
/// `host`, `host:port` and `[v6]:port` forms, default port 25565, an unparseable or
/// unrepresentable port falling back to 25565. Answers `None` for an empty address.
/// The SRV lookup and the IDN conversion are out (recorded limits): a multi-colon
/// unbracketed host stays whole on the default port.
pub fn parse_server_address(text: &str) -> Option<(String, u16)> {
    const DEFAULT_PORT: u16 = 25565;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(bracketed) = text.strip_prefix('[') {
        if let Some(end) = bracketed.find(']') {
            let host = bracketed[..end].to_string();
            if host.is_empty() {
                return None;
            }
            let rest = bracketed[end + 1..].trim();
            let port = rest
                .strip_prefix(':')
                .filter(|port| !port.is_empty())
                .map_or(DEFAULT_PORT, |port| parse_port(port, DEFAULT_PORT));
            return Some((host, port));
        }
    }
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() > 2 {
        return Some((text.to_string(), DEFAULT_PORT));
    }
    let host = parts[0].to_string();
    if host.is_empty() {
        return None;
    }
    let port = parts
        .get(1)
        .map_or(DEFAULT_PORT, |port| parse_port(port, DEFAULT_PORT));
    Some((host, port))
}

/// Parses a port the source's way (`parseIntWithDefault`): a trimmed integer in
/// range wins, anything else falls back.
fn parse_port(text: &str, default: u16) -> u16 {
    text.trim()
        .parse::<i32>()
        .ok()
        .filter(|port| (1..=i32::from(u16::MAX)).contains(port))
        .map_or(default, |port| port as u16)
}

/// Resolves an entry's join target through [`parse_server_address`].
pub fn join_target(entry: &ServerEntry) -> Option<(String, u16)> {
    parse_server_address(&entry.address)
}

/// The latency icon's level (`ServerListEntryNormal.java`, `drawEntry`): version
/// skew draws 5, a failed ping draws 5, then the `<150 → 0, <300 → 1, <600 → 2,
/// <1000 → 3, else 4` thresholds. `None` is the in-flight ping's animated branch.
pub fn latency_level(ping_ms: i64, version_skew: bool) -> Option<u8> {
    if version_skew {
        return Some(5);
    }
    if ping_ms == -2 {
        return None;
    }
    if ping_ms < 0 {
        return Some(5);
    }
    if ping_ms < 150 {
        return Some(0);
    }
    if ping_ms < 300 {
        return Some(1);
    }
    if ping_ms < 600 {
        return Some(2);
    }
    if ping_ms < 1000 {
        return Some(3);
    }
    Some(4)
}

/// One animated-branch frame's level from the wall clock and the row index
/// (`ServerListEntryNormal.java`: `(systemTime / 100 + slot * 2) & 7`, mirrored
/// past 4).
pub fn animated_latency_frame(now_ms: u64, slot: usize) -> u8 {
    let frame = (now_ms / 100 + slot as u64 * 2) & 7;
    if frame > 4 {
        (8 - frame) as u8
    } else {
        frame as u8
    }
}

/// The icon zone's hover text (`ServerListEntryNormal.java`, `drawEntry`).
pub fn latency_tooltip(ping_ms: i64, protocol: i32) -> String {
    if protocol > oxide_proto_v47::PROTOCOL {
        return String::from("Client out of date!");
    }
    if protocol < oxide_proto_v47::PROTOCOL {
        return String::from("Server out of date!");
    }
    if ping_ms == -2 {
        return String::from("Pinging...");
    }
    if ping_ms < 0 {
        return String::from("(no connection)");
    }
    format!("{ping_ms}ms")
}

/// Whether the row's protocol skews from this client's
/// (`ServerListEntryNormal.java`, `drawEntry`).
pub fn version_skew(protocol: i32) -> bool {
    protocol != oxide_proto_v47::PROTOCOL
}

/// The top-right line: dark-red version name under skew, else the population
/// line (`ServerListEntryNormal.java`, `drawEntry`).
pub fn version_line(protocol: i32, version_name: &str, population: &str) -> String {
    if version_skew(protocol) {
        format!("§4{version_name}")
    } else {
        population.to_string()
    }
}

/// The population line: grey `online`, dark-grey `/`, grey `max`
/// (`OldServerPinger.java`), dark-grey `???` with no counts.
pub fn population_text(online: Option<i64>, max: Option<i64>) -> String {
    match (online, max) {
        (Some(online), Some(max)) => format!("§7{online}§8/§7{max}"),
        _ => String::from("§8???"),
    }
}

/// The count zone's hover text: the sample's names, with the `... and N more ...`
/// tail when the sample is short (`OldServerPinger.java`). No sample draws no tooltip.
pub fn player_list_tooltip(sample: &[String], online: i64) -> Option<String> {
    if sample.is_empty() {
        return None;
    }
    let mut text = sample.join("\n");
    if (sample.len() as i64) < online {
        text.push_str(&format!(
            "\n... and {} more ...",
            online - sample.len() as i64
        ));
    }
    Some(text)
}

/// Renders a stored raw description through the standing styled path
/// (`chat::parse_json` then `to_legacy`).
pub fn motd_legacy(description_raw: &str) -> String {
    chat::to_legacy(&chat::parse_json(description_raw))
}

/// The list's bottom edge for a screen height.
pub fn list_bottom(height: i32) -> i32 {
    height - LIST_BOTTOM_OFFSET
}

/// The row index under a pointer height, if an entry is there.
pub fn row_index_at(y: i32, height: i32, count: usize) -> Option<usize> {
    if y < LIST_TOP || y >= list_bottom(height) {
        return None;
    }
    let row = (y - LIST_TOP) as usize / ROW_HEIGHT as usize;
    if row < count { Some(row) } else { None }
}

/// The screen's labels.
#[derive(Debug, Clone)]
pub struct MultiplayerLabels {
    /// The list's title.
    pub title: String,
    /// The add/edit form's title.
    pub form_title: String,
    /// The name field's caption.
    pub name_caption: String,
    /// The address field's caption.
    pub address_caption: String,
    /// The seven button labels in id order 7/2/1/4/3/8/0.
    pub buttons: [String; 7],
}

/// Resolves the screen's labels through the language table, falling back to the
/// store's `en_US.lang` English (`menu_labels`'s shape).
pub fn multiplayer_labels(language: Option<&LanguageTable>) -> MultiplayerLabels {
    const BUTTON_KEYS: [&str; 7] = [
        "selectServer.edit",
        "selectServer.delete",
        "selectServer.select",
        "selectServer.direct",
        "selectServer.add",
        "selectServer.refresh",
        "gui.cancel",
    ];
    const BUTTON_ENGLISH: [&str; 7] = [
        "Edit",
        "Delete",
        "Join Server",
        "Direct Connect",
        "Add server",
        "Refresh",
        "Cancel",
    ];
    let text = |key: &str, english: &str| -> String {
        language
            .and_then(|table| table.get(key))
            .unwrap_or(english)
            .to_string()
    };
    let button = |index: usize| text(BUTTON_KEYS[index], BUTTON_ENGLISH[index]);
    MultiplayerLabels {
        title: text("multiplayer.title", "Play Multiplayer"),
        form_title: text("addServer.title", "Edit Server Info"),
        name_caption: text("addServer.enterName", "Server Name"),
        address_caption: text("addServer.enterIp", "Server Address"),
        buttons: [
            button(0),
            button(1),
            button(2),
            button(3),
            button(4),
            button(5),
            button(6),
        ],
    }
}

/// The seven buttons with the source's ids and rects (`GuiMultiplayer.java`,
/// `createButtons`).
pub fn multiplayer_buttons(width: i32, height: i32, labels: &MultiplayerLabels) -> Vec<Button> {
    let half = f64::from(width) / 2.0;
    let rects = [
        (7, half - 154.0, f64::from(height) - 28.0, 70.0, 20.0),
        (2, half - 74.0, f64::from(height) - 28.0, 70.0, 20.0),
        (1, half - 154.0, f64::from(height) - 52.0, 100.0, 20.0),
        (4, half - 50.0, f64::from(height) - 52.0, 100.0, 20.0),
        (3, half + 4.0 + 50.0, f64::from(height) - 52.0, 100.0, 20.0),
        (8, half + 4.0, f64::from(height) - 28.0, 70.0, 20.0),
        (0, half + 4.0 + 76.0, f64::from(height) - 28.0, 75.0, 20.0),
    ];
    rects
        .iter()
        .zip(labels.buttons.iter())
        .map(|(&(id, x, y, w, h), label)| {
            Button::new(id, x as f32, y as f32, w as f32, h as f32, label.clone())
        })
        .collect()
}

/// The name/address form behind the add and edit sub-screens (`GuiScreenAddServer.java`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerForm {
    /// The name field's text.
    pub name: String,
    /// The address field's text.
    pub address: String,
    /// Whether the name field holds the focus (Tab flips it).
    pub focus_name: bool,
}

impl Default for ServerForm {
    /// The name field holds the initial focus (`initGui`).
    fn default() -> Self {
        Self {
            name: String::new(),
            address: String::new(),
            focus_name: true,
        }
    }
}

impl ServerForm {
    /// Stands the form on an entry, the name field focused (`initGui`).
    pub fn on_entry(entry: &ServerEntry) -> Self {
        Self {
            name: entry.name.clone(),
            address: entry.address.clone(),
            focus_name: true,
        }
    }

    /// The Add button enables only with both fields non-empty (`initGui`, `keyTyped`).
    /// The source's extra `split(":").length > 0` is always true for a non-empty
    /// field, so both-non-empty is the whole rule.
    pub fn add_enabled(&self) -> bool {
        !self.name.is_empty() && !self.address.is_empty()
    }

    /// Types one character into the focused field, within its max length.
    pub fn type_char(&mut self, typed: char) {
        let (field, max) = if self.focus_name {
            (&mut self.name, NAME_MAX_CHARS)
        } else {
            (&mut self.address, ADDRESS_MAX_CHARS)
        };
        if field.chars().count() < max {
            field.push(typed);
        }
    }

    /// Deletes the focused field's last character.
    pub fn backspace(&mut self) {
        if self.focus_name {
            self.name.pop();
        } else {
            self.address.pop();
        }
    }

    /// Flips the focus between the two fields (Tab, key code 15).
    pub fn toggle_focus(&mut self) {
        self.focus_name = !self.focus_name;
    }
}

/// One ping result, applied by index so slow servers cannot reorder the list.
#[derive(Debug, Clone)]
pub struct PingResult {
    /// The row the result belongs to.
    pub index: usize,
    /// The outcome.
    pub outcome: PingOutcome,
}

/// A row's ping outcome.
#[derive(Debug, Clone)]
pub enum PingOutcome {
    /// The server answered: MOTD, population, sample, version and latency.
    Answered {
        /// The raw description JSON, when the reply carried one.
        motd_raw: Option<String>,
        /// The online player count.
        online: i64,
        /// The max player count.
        max: i64,
        /// The count zone's hover text, when the server sent a sample.
        player_list: Option<String>,
        /// The server's game version name.
        version_name: String,
        /// The server's protocol version.
        protocol: i32,
        /// The icon's base64 bytes, without the prefix.
        icon: Option<String>,
        /// The C01/S01 round-trip latency in milliseconds.
        latency_ms: i64,
    },
    /// The host did not resolve (`UnknownHostException`).
    UnknownHost,
    /// The attempt failed or timed out (any other `Exception`).
    Unreachable,
}

/// Spawns the refresh worker: one `std::thread` per refresh that pings every
/// `(host, port)` in order and sends each result back over the channel. Every
/// row draws on one shared timeout, so a list of slow servers cannot hold the
/// worker past it; results carry their index, so slow servers cannot reorder
/// the list. The screen never blocks.
pub fn spawn_ping_refresh(
    targets: Vec<(String, u16)>,
    timeout: Duration,
) -> mpsc::Receiver<PingResult> {
    let (sender, receiver) = mpsc::channel();
    let deadline = Instant::now() + timeout;
    thread::spawn(move || {
        for (index, (host, port)) in targets.into_iter().enumerate() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let outcome = if remaining.is_zero() {
                PingOutcome::Unreachable
            } else {
                ping_one(&host, port, remaining)
            };
            if sender.send(PingResult { index, outcome }).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Pings one server and maps the attempt onto a row outcome: a resolution
/// failure is the unknown host, anything else the unreachable row.
fn ping_one(host: &str, port: u16, timeout: Duration) -> PingOutcome {
    match ping_with_latency(host, port, timeout) {
        Ok((status, latency_ms)) => {
            let sample: Vec<String> = status
                .players
                .sample
                .iter()
                .map(|player| player.name.clone())
                .collect();
            let online = i64::from(status.players.online);
            PingOutcome::Answered {
                motd_raw: status.description_raw().clone(),
                online,
                max: i64::from(status.players.max),
                player_list: player_list_tooltip(&sample, online),
                version_name: status.version.name.clone(),
                protocol: status.version.protocol,
                icon: status.favicon.as_deref().and_then(strip_icon_prefix),
                latency_ms,
            }
        }
        Err(PingError::Resolution(_)) => PingOutcome::UnknownHost,
        Err(_) => PingOutcome::Unreachable,
    }
}

/// Strips the icon's `data:` prefix, refusing any other format
/// (`OldServerPinger.java`, `ping`).
fn strip_icon_prefix(favicon: &str) -> Option<String> {
    favicon
        .strip_prefix("data:image/png;base64,")
        .map(str::to_string)
}

/// The standing sub-screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubScreen {
    /// The list itself.
    List,
    /// The add form.
    Add(ServerForm),
    /// The edit form for one row.
    Edit(usize, ServerForm),
    /// The direct-connect field's text.
    Direct(String),
    /// The delete confirm for one row.
    ConfirmDelete(usize),
}

/// One row's live ping state.
#[derive(Debug, Clone)]
pub struct LiveRow {
    /// The MOTD as drawn (legacy `§` codes).
    pub motd: String,
    /// The population line as drawn.
    pub population: String,
    /// The count zone's hover text, when the server sent a sample.
    pub player_list: Option<String>,
    /// The server's game version name.
    pub version_name: String,
    /// The server's protocol version.
    pub protocol: i32,
    /// The icon's base64 bytes, without the prefix.
    pub icon: Option<String>,
    /// The C01/S01 round trip in milliseconds: `-2` while pinging, `-1` on failure.
    pub ping_ms: i64,
}

impl LiveRow {
    /// The pinging row: the animated branch until the worker answers.
    fn pinging(icon: Option<String>) -> Self {
        Self {
            motd: String::from("Pinging..."),
            population: String::new(),
            player_list: None,
            version_name: String::from("1.8.9"),
            protocol: oxide_proto_v47::PROTOCOL,
            icon,
            ping_ms: -2,
        }
    }
}

/// One click's or key's outcome for `main.rs` to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiAction {
    /// Join the address now; the direct field's text rides along when the
    /// join came from the direct sub-screen, so `main.rs` can persist it as
    /// `last_server` the way the sub-screen's close does.
    Join(String, u16, Option<String>),
    /// Leave the screen for the main menu.
    Back,
    /// The direct sub-screen closed: persist the field as `last_server`.
    DirectClosed(String),
}

/// The multiplayer screen: the list, its live ping rows, the selection, and the sub-screen.
///
/// The ping worker's receiver is neither `Debug` nor `Clone`: the manual
/// impls below skip it — a clone carries the list, the selection and the
/// rows, and never an in-flight refresh.
pub struct MultiplayerScreen {
    /// The stored list.
    list: ServerList,
    /// Where the list saves.
    store_path: PathBuf,
    /// The labels the buttons and titles draw.
    labels: MultiplayerLabels,
    /// The selected row, if one is selected.
    selected: Option<usize>,
    /// The standing sub-screen.
    sub: SubScreen,
    /// The live ping rows, one per entry.
    live: Vec<LiveRow>,
    /// The last row click's time, for the double-click rule.
    last_click_ms: u64,
    /// The last row clicked, for the double-click rule.
    last_click_row: Option<usize>,
    /// The ping worker's receiver, while a refresh runs.
    pings: Option<mpsc::Receiver<PingResult>>,
}

impl std::fmt::Debug for MultiplayerScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiplayerScreen")
            .field("list", &self.list)
            .field("store_path", &self.store_path)
            .field("labels", &self.labels)
            .field("selected", &self.selected)
            .field("sub", &self.sub)
            .field("live", &self.live)
            .field("last_click_ms", &self.last_click_ms)
            .field("last_click_row", &self.last_click_row)
            .field("pings", &self.pings.as_ref().map(|_| "..."))
            .finish()
    }
}

impl Clone for MultiplayerScreen {
    fn clone(&self) -> Self {
        Self {
            list: self.list.clone(),
            store_path: self.store_path.clone(),
            labels: self.labels.clone(),
            selected: self.selected,
            sub: self.sub.clone(),
            live: self.live.clone(),
            last_click_ms: self.last_click_ms,
            last_click_row: self.last_click_row,
            pings: None,
        }
    }
}

impl MultiplayerScreen {
    /// Opens the screen on a stored list, saving to `store_path`. Every row
    /// starts pinging; `main.rs` runs [`MultiplayerScreen::refresh`] next, the
    /// way the first draw queues the source's pings.
    pub fn open(list: ServerList, store_path: PathBuf, labels: MultiplayerLabels) -> Self {
        let live = list
            .entries
            .iter()
            .map(|entry| LiveRow::pinging(entry.icon.clone()))
            .collect();
        Self {
            list,
            store_path,
            labels,
            selected: None,
            sub: SubScreen::List,
            live,
            last_click_ms: 0,
            last_click_row: None,
            pings: None,
        }
    }

    /// The stored list.
    pub fn list(&self) -> &ServerList {
        &self.list
    }

    /// The selected row, if one is selected.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The standing sub-screen.
    pub fn sub(&self) -> &SubScreen {
        &self.sub
    }

    /// One row's live state, if the row exists.
    pub fn live(&self, index: usize) -> Option<&LiveRow> {
        self.live.get(index)
    }

    /// Refreshes every row: the live rows reset to pinging and one worker
    /// thread pings them all (`refreshServerList`'s shape, without reopening
    /// the screen).
    pub fn refresh(&mut self, timeout: Duration) {
        for (row, entry) in self.live.iter_mut().zip(self.list.entries.iter()) {
            *row = LiveRow::pinging(entry.icon.clone());
        }
        let targets: Vec<(String, u16)> = self
            .list
            .entries
            .iter()
            .map(|entry| {
                parse_server_address(&entry.address).unwrap_or((entry.address.clone(), 25565))
            })
            .collect();
        self.pings = Some(spawn_ping_refresh(targets, timeout));
    }

    /// Drains the worker's results, applying each by index.
    pub fn pump_pings(&mut self) {
        let mut results = Vec::new();
        if let Some(pings) = self.pings.as_ref() {
            while let Ok(result) = pings.try_recv() {
                results.push(result);
            }
        }
        for result in results {
            self.apply_ping(result);
        }
    }

    /// Handles one left press, answering `main.rs`'s action when the press
    /// leaves the screen or joins.
    pub fn click_at(
        &mut self,
        point: (f32, f32),
        width: i32,
        height: i32,
        last_server: &str,
        now_ms: u64,
    ) -> Option<MultiAction> {
        match self.sub.clone() {
            SubScreen::List => self.click_list(point, width, height, last_server, now_ms),
            SubScreen::Add(_) | SubScreen::Edit(..) => {
                self.click_form(point, width, height);
                None
            }
            SubScreen::Direct(_) => self.click_direct(point, width, height),
            SubScreen::ConfirmDelete(_) => {
                self.click_confirm(point, width, height);
                None
            }
        }
    }

    /// Handles one key: Tab flips the form focus, Enter confirms, F5 refreshes,
    /// the arrows move the selection, shift with the arrows reorders
    /// (`GuiMultiplayer.java`, `keyTyped`).
    pub fn key(
        &mut self,
        typed: Option<char>,
        key_code: u32,
        shift: bool,
        width: i32,
        height: i32,
        timeout: Duration,
    ) -> Option<MultiAction> {
        match self.sub.clone() {
            SubScreen::Add(_) | SubScreen::Edit(..) => self.key_form(typed, key_code),
            SubScreen::Direct(_) => self.key_direct(typed, key_code),
            SubScreen::ConfirmDelete(_) => {
                if key_code == 28 || key_code == 156 {
                    self.confirm_delete();
                } else if key_code == 1 {
                    self.cancel();
                }
                None
            }
            SubScreen::List => self.key_list(key_code, shift, width, height, timeout),
        }
    }

    /// Confirms the pending delete: the row leaves the list, the list saves,
    /// and the screen returns to the list (`confirmClicked`'s delete arm).
    pub fn confirm_delete(&mut self) {
        let SubScreen::ConfirmDelete(index) = self.sub else {
            return;
        };
        if index < self.list.entries.len() {
            self.list.remove(index);
            self.live.remove(index);
            self.save_quietly();
        }
        self.selected = None;
        self.sub = SubScreen::List;
    }

    /// Cancels whatever sub-screen stands.
    pub fn cancel(&mut self) {
        self.sub = SubScreen::List;
    }

    /// Applies one worker result by index.
    pub fn apply_ping(&mut self, result: PingResult) {
        let Some(row) = self.live.get_mut(result.index) else {
            return;
        };
        match result.outcome {
            PingOutcome::Answered {
                motd_raw,
                online,
                max,
                player_list,
                version_name,
                protocol,
                icon,
                latency_ms,
            } => {
                row.motd = motd_raw.as_deref().map_or(String::new(), motd_legacy);
                row.population = population_text(Some(online), Some(max));
                row.player_list = player_list;
                row.version_name = version_name;
                row.protocol = protocol;
                row.icon = icon;
                row.ping_ms = latency_ms;
            }
            PingOutcome::UnknownHost => {
                row.ping_ms = -1;
                row.motd = String::from("§4Can't resolve hostname");
            }
            PingOutcome::Unreachable => {
                row.ping_ms = -1;
                row.motd = String::from("§4Can't connect to server.");
            }
        }
    }

    /// Draws the screen: the title, the rows, and the buttons — or the standing
    /// sub-screen over them.
    pub fn draws(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
        now_ms: u64,
    ) -> Vec<HudDraw> {
        let width = scaled.width as i32;
        let height = scaled.height as i32;
        match &self.sub {
            SubScreen::List => self.draws_list(font, scaled, mouse, now_ms),
            SubScreen::Add(form) => self.draws_form(font, mouse, form, width, height),
            SubScreen::Edit(_, form) => self.draws_form(font, mouse, form, width, height),
            SubScreen::Direct(field) => self.draws_direct(font, mouse, field, width, height),
            SubScreen::ConfirmDelete(index) => {
                let mut draws = self.draws_list(font, scaled, mouse, now_ms);
                draws.extend(self.draws_confirm(font, mouse, *index, width, height));
                draws
            }
        }
    }

    /// One list press: the buttons first, then the rows with the double-click rule.
    fn click_list(
        &mut self,
        point: (f32, f32),
        width: i32,
        height: i32,
        last_server: &str,
        now_ms: u64,
    ) -> Option<MultiAction> {
        for button in multiplayer_buttons(width, height, &self.labels) {
            if !button.enabled || !button.hit(point) {
                continue;
            }
            // Join, Edit and Delete need a row; the rest stand alone.
            match button.id {
                1 => return self.connect_selected(),
                7 => {
                    if let Some(index) = self.selected {
                        self.sub =
                            SubScreen::Edit(index, ServerForm::on_entry(&self.list.entries[index]));
                    }
                    return None;
                }
                2 => {
                    if let Some(index) = self.selected {
                        self.sub = SubScreen::ConfirmDelete(index);
                    }
                    return None;
                }
                4 => {
                    self.sub = SubScreen::Direct(last_server.to_string());
                    return None;
                }
                3 => {
                    self.sub = SubScreen::Add(ServerForm::default());
                    return None;
                }
                8 => {
                    self.refresh(PING_TIMEOUT);
                    return None;
                }
                0 => return Some(MultiAction::Back),
                _ => return None,
            }
        }
        // The rows: a second press on the armed row within 250 ms joins
        // (`GuiSlot.java`, `handleMouseInput`).
        if let Some(row) = row_index_at(point.1 as i32, height, self.list.entries.len()) {
            if self.last_click_row == Some(row)
                && now_ms.wrapping_sub(self.last_click_ms) < 250
                && self.selected == Some(row)
            {
                self.last_click_row = None;
                return self.connect_selected();
            }
            self.selected = Some(row);
            self.last_click_row = Some(row);
            self.last_click_ms = now_ms;
        }
        None
    }

    /// Joins the selected row, if its address resolves.
    fn connect_selected(&self) -> Option<MultiAction> {
        let entry = self
            .selected
            .and_then(|index| self.list.entries.get(index))?;
        let (host, port) = join_target(entry)?;
        Some(MultiAction::Join(host, port, None))
    }

    /// One form press: the fields take the focus, Add confirms, Cancel backs out.
    fn click_form(&mut self, point: (f32, f32), width: i32, height: i32) {
        let name = field_rect(width, 66);
        let address = field_rect(width, 106);
        let form = match &mut self.sub {
            SubScreen::Add(form) | SubScreen::Edit(_, form) => form,
            _ => return,
        };
        if hit_rect(point, name) {
            form.focus_name = true;
        } else if hit_rect(point, address) {
            form.focus_name = false;
        }
        let (add, cancel) = form_buttons(width, height, 18);
        if hit_rect(point, cancel) {
            self.sub = SubScreen::List;
        } else if hit_rect(point, add) && form.add_enabled() {
            let form = form.clone();
            self.confirm_form(form);
        }
    }

    /// Confirms the add/edit form: the entry lands and the list saves
    /// (`confirmClicked`'s add and edit arms).
    fn confirm_form(&mut self, form: ServerForm) {
        match &self.sub {
            SubScreen::Add(_) => {
                self.list
                    .add(ServerEntry::new(form.name, form.address, None));
            }
            SubScreen::Edit(index, _) => {
                if let Some(entry) = self.list.entries.get_mut(*index) {
                    entry.name = form.name;
                    entry.address = form.address;
                }
            }
            _ => return,
        }
        self.save_quietly();
        self.rebuild_live();
        self.selected = None;
        self.sub = SubScreen::List;
    }

    /// One direct-connect press: Join joins, Cancel persists and backs out.
    fn click_direct(&mut self, point: (f32, f32), width: i32, height: i32) -> Option<MultiAction> {
        let SubScreen::Direct(field) = self.sub.clone() else {
            return None;
        };
        if hit_rect(point, field_rect(width, 116)) {
            return None;
        }
        let (join, cancel) = form_buttons(width, height, 12);
        if hit_rect(point, cancel) {
            self.sub = SubScreen::List;
            return Some(MultiAction::DirectClosed(field));
        }
        if hit_rect(point, join) {
            return self.join_direct(&field);
        }
        None
    }

    /// Joins the direct field's address, if it resolves.
    fn join_direct(&mut self, field: &str) -> Option<MultiAction> {
        let (host, port) = parse_server_address(field)?;
        self.sub = SubScreen::List;
        Some(MultiAction::Join(host, port, Some(field.to_string())))
    }

    /// One confirm press: the two-button reuse of the widget set.
    fn click_confirm(&mut self, point: (f32, f32), width: i32, height: i32) {
        let (delete, cancel) = confirm_buttons(width, height);
        if hit_rect(point, cancel) {
            self.sub = SubScreen::List;
        } else if hit_rect(point, delete) {
            self.confirm_delete();
        }
    }

    /// One form key: typing, Tab, Enter, Escape.
    fn key_form(&mut self, typed: Option<char>, key_code: u32) -> Option<MultiAction> {
        let form = match &mut self.sub {
            SubScreen::Add(form) | SubScreen::Edit(_, form) => form,
            _ => return None,
        };
        if key_code == 15 {
            form.toggle_focus();
        } else if key_code == 14 {
            form.backspace();
        } else if key_code == 28 || key_code == 156 {
            if form.add_enabled() {
                let form = form.clone();
                self.confirm_form(form);
            }
        } else if key_code == 1 {
            self.sub = SubScreen::List;
        } else if let Some(char) = typed {
            form.type_char(char);
        }
        None
    }

    /// One direct-connect key: typing, Enter, Escape.
    fn key_direct(&mut self, typed: Option<char>, key_code: u32) -> Option<MultiAction> {
        let SubScreen::Direct(field) = self.sub.clone() else {
            return None;
        };
        if key_code == 14 {
            let mut next = field.clone();
            next.pop();
            self.sub = SubScreen::Direct(next);
        } else if key_code == 28 || key_code == 156 {
            return self.join_direct(&field);
        } else if key_code == 1 {
            self.sub = SubScreen::List;
            return Some(MultiAction::DirectClosed(field));
        } else if let Some(char) = typed {
            let mut next = field;
            if next.chars().count() < ADDRESS_MAX_CHARS {
                next.push(char);
                self.sub = SubScreen::Direct(next);
            }
        }
        None
    }

    /// One list key: F5 refreshes, the arrows move the selection, shift with
    /// the arrows reorders, Enter joins, Escape backs out (`keyTyped`).
    fn key_list(
        &mut self,
        key_code: u32,
        shift: bool,
        _width: i32,
        _height: i32,
        timeout: Duration,
    ) -> Option<MultiAction> {
        if key_code == 63 {
            self.refresh(timeout);
            return None;
        }
        if key_code == 200 || key_code == 208 {
            let down = key_code == 208;
            let count = self.list.entries.len();
            let selected = self.selected.unwrap_or(0);
            if shift {
                // The swap+save reorder follows the selection (`keyTyped`).
                if down && selected < count.saturating_sub(1) {
                    self.list.swap(selected, selected + 1);
                    self.save_quietly();
                    self.rebuild_live();
                    self.selected = Some(selected + 1);
                } else if !down && selected > 0 && selected < count {
                    self.list.swap(selected, selected - 1);
                    self.save_quietly();
                    self.rebuild_live();
                    self.selected = Some(selected - 1);
                }
                return None;
            }
            if down {
                self.selected = if selected + 1 < count {
                    Some(selected + 1)
                } else {
                    None
                };
            } else if selected > 0 {
                self.selected = Some(selected - 1);
            } else {
                self.selected = None;
            }
            return None;
        }
        if key_code == 28 || key_code == 156 {
            return self.connect_selected();
        }
        if key_code == 1 {
            return Some(MultiAction::Back);
        }
        None
    }

    /// Saves the list, keeping a failure to the log: the screen stays usable
    /// when the store is read-only.
    fn save_quietly(&self) {
        if let Err(error) = self.list.save(&self.store_path) {
            tracing::warn!(error = %error, "the server list could not be saved");
        }
    }

    /// Rebuilds the live rows after the entries changed, re-pinging everything.
    fn rebuild_live(&mut self) {
        self.live = self
            .list
            .entries
            .iter()
            .map(|entry| LiveRow::pinging(entry.icon.clone()))
            .collect();
        let targets: Vec<(String, u16)> = self
            .list
            .entries
            .iter()
            .map(|entry| {
                parse_server_address(&entry.address).unwrap_or((entry.address.clone(), 25565))
            })
            .collect();
        self.pings = Some(spawn_ping_refresh(targets, PING_TIMEOUT));
    }

    /// Draws the list: the title, the rows, the buttons, and the hover text.
    fn draws_list(
        &self,
        font: &Font,
        scaled: &ScaledResolution,
        mouse: Option<(f32, f32)>,
        now_ms: u64,
    ) -> Vec<HudDraw> {
        let width = scaled.width as f32;
        let height = scaled.height as i32;
        let mut draws = Vec::new();
        draws.push(centred_text(
            font,
            self.labels.title.clone(),
            width / 2.0,
            20.0,
            [1.0, 1.0, 1.0, 1.0],
        ));
        for (slot, (entry, row)) in self.list.entries.iter().zip(self.live.iter()).enumerate() {
            let y = (LIST_TOP + slot as i32 * ROW_HEIGHT) as f32;
            draws.extend(self.draws_row(font, entry, row, slot, 0.0, y, width, now_ms));
        }
        for button in multiplayer_buttons(scaled.width as i32, height, &self.labels) {
            let mut button = button;
            // Join, Edit and Delete need a row (`selectServer`).
            if matches!(button.id, 1 | 7 | 2) {
                button.enabled = self.selected.is_some();
            }
            draws.extend(button.draws(font, mouse));
        }
        if let Some(text) = self.hover_text(font, mouse, width, height) {
            let (x, y) = mouse.unwrap_or((0.0, 0.0));
            draws.push(HudDraw::Text {
                text,
                x: x + 12.0,
                y,
                scale: 1.0,
                colour: [1.0, 1.0, 1.0, 1.0],
                shadow: true,
                blend: true,
            });
        }
        draws
    }

    /// Draws one row: the icon fallback, the name, two MOTD lines, the
    /// version/population line, and the latency bar (`ServerListEntryNormal.java`,
    /// `drawEntry`).
    #[allow(clippy::too_many_arguments)]
    fn draws_row(
        &self,
        font: &Font,
        entry: &ServerEntry,
        row: &LiveRow,
        slot: usize,
        x: f32,
        y: f32,
        list_width: f32,
        now_ms: u64,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        // The icon's 32x32 cell: the decoded server icon would draw here, but
        // image decode is out (recorded limit), so the unknown-server fallback
        // stands in every row.
        draws.push(HudDraw::Rect {
            x,
            y,
            width: 32.0,
            height: 32.0,
            colour: [0.15, 0.15, 0.15, 1.0],
        });
        draws.push(HudDraw::Text {
            text: entry.name.clone(),
            x: x + 32.0 + 3.0,
            y: y + 1.0,
            scale: 1.0,
            colour: [1.0, 1.0, 1.0, 1.0],
            shadow: true,
            blend: true,
        });
        let wrap_width = (list_width - 32.0 - 2.0) as i32;
        for (line, motd) in wrap_lines(font, &row.motd, wrap_width)
            .iter()
            .take(2)
            .enumerate()
        {
            draws.push(HudDraw::Text {
                text: motd.clone(),
                x: x + 32.0 + 3.0,
                y: y + 12.0 + font.height() as f32 * line as f32,
                scale: 1.0,
                colour: [0.5, 0.5, 0.5, 1.0],
                shadow: true,
                blend: true,
            });
        }
        let skew = version_skew(row.protocol);
        let top_right = version_line(row.protocol, &row.version_name, &row.population);
        let top_right_width = string_width(font, &top_right) as f32;
        draws.push(HudDraw::Text {
            text: top_right,
            x: x + list_width - top_right_width - 15.0 - 2.0,
            y: y + 1.0,
            scale: 1.0,
            colour: [0.5, 0.5, 0.5, 1.0],
            shadow: true,
            blend: true,
        });
        let (k, l) = if skew {
            (0.0, 5.0)
        } else if row.ping_ms == -2 {
            (1.0, f32::from(animated_latency_frame(now_ms, slot)))
        } else {
            (
                0.0,
                f32::from(latency_level(row.ping_ms, false).unwrap_or(5)),
            )
        };
        draws.push(HudDraw::TexturedRect {
            texture: HudTexture::Named(ICONS_SHEET),
            x: x + list_width - 15.0,
            y,
            width: BAR_W,
            height: BAR_H,
            uv: [
                k * BAR_W / 256.0,
                (BAR_V0 + l * BAR_H) / 256.0,
                (k * BAR_W + BAR_W) / 256.0,
                (BAR_V0 + l * BAR_H + BAR_H) / 256.0,
            ],
            colour: [1.0, 1.0, 1.0, 1.0],
        });
        draws
    }

    /// The hover text at the pointer: the icon zone's latency line, else the
    /// count zone's player list (`ServerListEntryNormal.java`, `drawEntry`).
    fn hover_text(
        &self,
        font: &Font,
        mouse: Option<(f32, f32)>,
        list_width: f32,
        height: i32,
    ) -> Option<String> {
        let (mx, my) = mouse?;
        let row = row_index_at(my as i32, height, self.list.entries.len())?;
        let live = self.live.get(row)?;
        let top = (LIST_TOP + row as i32 * ROW_HEIGHT) as f32;
        if my < top || my > top + 8.0 {
            return None;
        }
        if mx >= list_width - 15.0 && mx <= list_width - 5.0 {
            return Some(latency_tooltip(live.ping_ms, live.protocol));
        }
        let line = version_line(live.protocol, &live.version_name, &live.population);
        let text_width = string_width(font, &line) as f32;
        if mx >= list_width - text_width - 15.0 - 2.0 && mx <= list_width - 15.0 - 2.0 {
            return live.player_list.clone();
        }
        None
    }

    /// Draws the add/edit form (`GuiScreenAddServer.java`, `drawScreen`).
    fn draws_form(
        &self,
        font: &Font,
        mouse: Option<(f32, f32)>,
        form: &ServerForm,
        width: i32,
        height: i32,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        draws.push(centred_text(
            font,
            self.labels.form_title.clone(),
            width as f32 / 2.0,
            17.0,
            [1.0, 1.0, 1.0, 1.0],
        ));
        draws.push(field_caption(self.labels.name_caption.clone(), width, 53));
        draws.push(field_caption(
            self.labels.address_caption.clone(),
            width,
            94,
        ));
        draws.extend(field_draws(&form.name, form.focus_name, width, 66));
        draws.extend(field_draws(&form.address, !form.focus_name, width, 106));
        let (add, cancel) = form_buttons(width, height, 18);
        draws.extend(
            Button::new(0, add.0, add.1, add.2, add.3, String::from("Add"))
                .with_enabled(form.add_enabled())
                .draws(font, mouse),
        );
        draws.extend(
            Button::new(
                1,
                cancel.0,
                cancel.1,
                cancel.2,
                cancel.3,
                String::from("Cancel"),
            )
            .draws(font, mouse),
        );
        draws
    }

    /// Draws the direct-connect form (`GuiScreenServerList.java`, `drawScreen`).
    fn draws_direct(
        &self,
        font: &Font,
        mouse: Option<(f32, f32)>,
        field: &str,
        width: i32,
        height: i32,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        draws.push(centred_text(
            font,
            self.labels.buttons[3].clone(),
            width as f32 / 2.0,
            20.0,
            [1.0, 1.0, 1.0, 1.0],
        ));
        draws.push(field_caption(
            self.labels.address_caption.clone(),
            width,
            100,
        ));
        draws.extend(field_draws(field, true, width, 116));
        let (join, cancel) = form_buttons(width, height, 12);
        draws.extend(
            Button::new(
                1,
                join.0,
                join.1,
                join.2,
                join.3,
                self.labels.buttons[2].clone(),
            )
            .with_enabled(!field.is_empty())
            .draws(font, mouse),
        );
        draws.extend(
            Button::new(
                0,
                cancel.0,
                cancel.1,
                cancel.2,
                cancel.3,
                self.labels.buttons[6].clone(),
            )
            .draws(font, mouse),
        );
        draws
    }

    /// Draws the delete confirm: the question over the list and the two-button
    /// reuse of the widget set.
    fn draws_confirm(
        &self,
        font: &Font,
        mouse: Option<(f32, f32)>,
        index: usize,
        width: i32,
        height: i32,
    ) -> Vec<HudDraw> {
        let mut draws = Vec::new();
        let name = self
            .list
            .entries
            .get(index)
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        draws.push(centred_text(
            font,
            String::from("Are you sure you want to remove this server?"),
            width as f32 / 2.0,
            height as f32 / 2.0 - 40.0,
            [1.0, 1.0, 1.0, 1.0],
        ));
        draws.push(centred_text(
            font,
            format!("'{name}' will be lost forever! (A long time!)"),
            width as f32 / 2.0,
            height as f32 / 2.0 - 28.0,
            [1.0, 1.0, 1.0, 1.0],
        ));
        let (delete, cancel) = confirm_buttons(width, height);
        draws.extend(
            Button::new(
                0,
                delete.0,
                delete.1,
                delete.2,
                delete.3,
                String::from("Delete"),
            )
            .draws(font, mouse),
        );
        draws.extend(
            Button::new(
                1,
                cancel.0,
                cancel.1,
                cancel.2,
                cancel.3,
                String::from("Cancel"),
            )
            .draws(font, mouse),
        );
        draws
    }
}

/// One centred title line.
fn centred_text(font: &Font, text: String, centre: f32, y: f32, colour: [f32; 4]) -> HudDraw {
    let width = string_width(font, &text) as f32;
    HudDraw::Text {
        text,
        x: centre - width / 2.0,
        y,
        scale: 1.0,
        colour,
        shadow: true,
        blend: true,
    }
}

/// One field caption line (`drawString` grey, `10526880`).
fn field_caption(text: String, width: i32, y: i32) -> HudDraw {
    HudDraw::Text {
        text,
        x: width as f32 / 2.0 - 100.0,
        y: y as f32,
        scale: 1.0,
        colour: [160.0 / 255.0, 160.0 / 255.0, 160.0 / 255.0, 1.0],
        shadow: true,
        blend: true,
    }
}

/// One text field's draws: the box and its text with a focus cursor.
fn field_draws(text: &str, focused: bool, width: i32, y: i32) -> Vec<HudDraw> {
    let (x, y, w, h) = field_rect(width, y);
    let mut draws = vec![
        HudDraw::Rect {
            x: x - 1.0,
            y: y - 1.0,
            width: w + 2.0,
            height: h + 2.0,
            colour: [0.63, 0.63, 0.63, 1.0],
        },
        HudDraw::Rect {
            x,
            y,
            width: w,
            height: h,
            colour: [0.0, 0.0, 0.0, 1.0],
        },
    ];
    let mut shown = text.to_string();
    if focused {
        shown.push('_');
    }
    draws.push(HudDraw::Text {
        text: shown,
        x: x + 4.0,
        y: y + (h - 8.0) / 2.0,
        scale: 1.0,
        colour: [0.88, 0.88, 0.88, 1.0],
        shadow: false,
        blend: true,
    });
    draws
}

/// One form field's rect: 200 wide at `width / 2 - 100` (`initGui`).
fn field_rect(width: i32, y: i32) -> (f32, f32, f32, f32) {
    (width as f32 / 2.0 - 100.0, y as f32, 200.0, 20.0)
}

/// Whether the point stands on a rect.
fn hit_rect(point: (f32, f32), rect: (f32, f32, f32, f32)) -> bool {
    point.0 >= rect.0 && point.1 >= rect.1 && point.0 < rect.0 + rect.2 && point.1 < rect.1 + rect.3
}

/// Two button rects: the Add/Cancel pair under a form, or the delete
/// confirm's pair.
type ButtonPair = ((f32, f32, f32, f32), (f32, f32, f32, f32));

/// The Add/Cancel pair under a form (`initGui`).
fn form_buttons(width: i32, height: i32, adjust: i32) -> ButtonPair {
    let x = width as f32 / 2.0 - 100.0;
    (
        (x, height as f32 / 4.0 + 96.0 + adjust as f32, 200.0, 20.0),
        (x, height as f32 / 4.0 + 120.0 + adjust as f32, 200.0, 20.0),
    )
}

/// The delete confirm's button pair: the minimal two-button reuse of the
/// widget set.
fn confirm_buttons(width: i32, height: i32) -> ButtonPair {
    let y = height as f32 / 2.0 + 10.0;
    (
        (width as f32 / 2.0 - 100.0, y, 98.0, 20.0),
        (width as f32 / 2.0 + 2.0, y, 98.0, 20.0),
    )
}

/// A button with its enabled gate set.
trait WithEnabled {
    /// Sets the enabled gate.
    fn with_enabled(self, enabled: bool) -> Self;
}

impl WithEnabled for Button {
    fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}
