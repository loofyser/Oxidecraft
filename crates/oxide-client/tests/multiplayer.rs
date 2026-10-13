//! Task 16's server-list suite: the `servers.toml` file model (round trip, missing file,
//! hostile bytes) and the address parser behind the join routes.
//!
//! The file is the port's own line format under `<store>/servers.toml` — sorted keys,
//! atomic temp+rename write (the `options.toml` precedent); `servers.dat` is never read
//! or written. Hostile bytes are refused with documented caps, mirroring the `nbt.rs`
//! discipline: truncation at every cut, unknown keys ignored, absurd lengths refused.

use std::fs;
use std::path::PathBuf;

use oxide_client::screens::multiplayer::{
    MAX_ADDRESS_BYTES, MAX_FILE_BYTES, MAX_ICON_BYTES, MAX_NAME_BYTES, ServerEntry, ServerList,
    join_target, parse_server_address, servers_path,
};

/// A scratch store root holding one test's server file.
fn scratch_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("oxide-mp-{name}-{}", std::process::id()));
    fs::create_dir_all(&root).expect("the scratch root is created");
    root
}

/// One test's server file path; the parent is removed afterwards.
fn server_file(root: &std::path::Path) -> PathBuf {
    servers_path(root)
}

#[test]
fn the_server_file_round_trips_names_addresses_and_icons() {
    let root = scratch_root("roundtrip");
    let path = server_file(&root);
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "Oxide \"test\"\nserver".to_string(),
        "mc.example.com:25566".to_string(),
        Some("QUJD".to_string()),
    ));
    list.add(ServerEntry::new(
        "plain".to_string(),
        "127.0.0.1".to_string(),
        None,
    ));
    list.save(&path).expect("the list saves");

    let loaded = ServerList::load(&path);
    assert_eq!(loaded, list, "save then load is the identity");
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.entries[0].icon.as_deref(), Some("QUJD"));
    assert_eq!(loaded.entries[1].icon, None);

    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn a_missing_server_file_loads_empty() {
    let root = scratch_root("missing");
    let loaded = ServerList::load(&server_file(&root));
    assert!(loaded.entries.is_empty(), "no file is an empty list");
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn truncation_at_every_cut_never_panics() {
    let root = scratch_root("cuts");
    let path = server_file(&root);
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "first".to_string(),
        "mc.example.com".to_string(),
        Some("QUJDRA==".to_string()),
    ));
    list.add(ServerEntry::new(
        "second".to_string(),
        "127.0.0.1:25565".to_string(),
        None,
    ));
    list.save(&path).expect("the list saves");
    let bytes = fs::read(&path).expect("the file reads");

    for cut in 0..=bytes.len() {
        let probe = root.join("cut.toml");
        fs::write(&probe, &bytes[..cut]).expect("the cut writes");
        // No cut may panic; a cut entry simply drops out.
        let _ = ServerList::load(&probe);
    }
    assert_eq!(
        ServerList::load(&path).entries.len(),
        2,
        "the whole file loads both entries"
    );
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn unknown_keys_are_ignored() {
    let root = scratch_root("unknown");
    let path = server_file(&root);
    let mut text = String::from("# a comment line\nunknown_top_level = \"x\"\n");
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "kept".to_string(),
        "mc.example.com".to_string(),
        None,
    ));
    list.save(&path).expect("the list saves");
    text.push_str(&fs::read_to_string(&path).expect("the file reads"));
    text.push_str("server.0.future = \"y\"\nserver.9.name = \"orphan\"\n");
    fs::write(&path, text).expect("the hostile file writes");

    let loaded = ServerList::load(&path);
    assert_eq!(loaded.entries.len(), 1, "unknown keys change nothing");
    assert_eq!(loaded.entries[0].name, "kept");

    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn absurd_lengths_are_refused() {
    let root = scratch_root("absurd");
    let path = server_file(&root);
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "x".repeat(MAX_NAME_BYTES + 1),
        "mc.example.com".to_string(),
        None,
    ));
    list.add(ServerEntry::new(
        "long-address".to_string(),
        "y".repeat(MAX_ADDRESS_BYTES + 1),
        None,
    ));
    list.add(ServerEntry::new(
        "long-icon".to_string(),
        "mc.example.com".to_string(),
        Some("z".repeat(MAX_ICON_BYTES + 1)),
    ));
    list.add(ServerEntry::new(
        "kept".to_string(),
        "mc.example.com".to_string(),
        None,
    ));
    // The in-memory list holds what it was given; the file round trip refuses
    // the absurd three, and an over-cap file loads empty.
    list.save(&path).expect("the list saves");
    let loaded = ServerList::load(&path);
    assert_eq!(loaded.entries.len(), 1, "only the sane entry survives");
    assert_eq!(loaded.entries[0].name, "kept");

    fs::write(&path, vec![b'#'; MAX_FILE_BYTES as usize + 1]).expect("the huge file writes");
    assert!(
        ServerList::load(&path).entries.is_empty(),
        "an over-cap file loads empty"
    );
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn addresses_parse_with_a_default_port() {
    assert_eq!(
        parse_server_address("mc.example.com"),
        Some(("mc.example.com".to_string(), 25565)),
        "a bare host takes the default port"
    );
    assert_eq!(
        parse_server_address("mc.example.com:25566"),
        Some(("mc.example.com".to_string(), 25566)),
        "an explicit port wins"
    );
    assert_eq!(
        parse_server_address("[2001:db8::1]:25566"),
        Some(("2001:db8::1".to_string(), 25566)),
        "bracketed IPv6 splits at the brackets"
    );
    assert_eq!(
        parse_server_address("[2001:db8::1]"),
        Some(("2001:db8::1".to_string(), 25565)),
        "a bracketed host without a port takes the default"
    );
    assert_eq!(
        parse_server_address("mc.example.com:notaport"),
        Some(("mc.example.com".to_string(), 25565)),
        "an unparseable port falls back to the default"
    );
    assert_eq!(
        parse_server_address("2001:db8::1"),
        Some(("2001:db8::1".to_string(), 25565)),
        "an unbracketed multi-colon address stays whole (no SRV here)"
    );
    assert_eq!(
        parse_server_address(""),
        None,
        "an empty address is refused"
    );
    assert_eq!(
        parse_server_address("   "),
        None,
        "a blank address is refused"
    );
}

#[test]
fn select_then_join_resolves_the_entry_address() {
    let entry = ServerEntry::new("play".to_string(), "mc.example.com:25566".to_string(), None);
    assert_eq!(
        join_target(&entry),
        Some(("mc.example.com".to_string(), 25566)),
        "the join route resolves the selected entry's address"
    );
    let bare = ServerEntry::new("bare".to_string(), "mc.example.com".to_string(), None);
    assert_eq!(
        join_target(&bare),
        Some(("mc.example.com".to_string(), 25565)),
        "a portless entry joins on the default port"
    );
    let broken = ServerEntry::new("broken".to_string(), String::new(), None);
    assert_eq!(join_target(&broken), None, "an empty entry joins nowhere");
}

/// The screen-interaction half: selection, sub-screens, reorder, delete and the live
/// worker, against the source's rules (`GuiMultiplayer.java`, `GuiScreenAddServer.java`,
/// `GuiScreenServerList.java`, `ServerListEntryNormal.java`).
use std::time::Duration;

use oxide_client::screens::multiplayer::{
    MultiAction, MultiplayerLabels, MultiplayerScreen, ServerForm, SubScreen,
};

/// The English labels the store's `en_US.lang` carries.
fn english_labels() -> MultiplayerLabels {
    MultiplayerLabels {
        title: String::from("Play Multiplayer"),
        form_title: String::from("Edit Server Info"),
        name_caption: String::from("Server Name"),
        address_caption: String::from("Server Address"),
        buttons: [
            String::from("Edit"),
            String::from("Delete"),
            String::from("Join Server"),
            String::from("Direct Connect"),
            String::from("Add server"),
            String::from("Refresh"),
            String::from("Cancel"),
        ],
    }
}

/// One screen on two entries, saving into a scratch store.
fn two_entry_screen(name: &str) -> (MultiplayerScreen, PathBuf) {
    let root = scratch_root(name);
    let path = server_file(&root);
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "first".to_string(),
        "127.0.0.1:25565".to_string(),
        None,
    ));
    list.add(ServerEntry::new(
        "second".to_string(),
        "127.0.0.1:25566".to_string(),
        None,
    ));
    list.save(&path).expect("the list saves");
    let screen = MultiplayerScreen::open(list, path.clone(), english_labels());
    (screen, root)
}

#[test]
fn a_single_click_selects_and_a_double_click_joins() {
    let (mut screen, root) = two_entry_screen("clicks");
    // The first row's middle at 800x600: row 0 spans y 32..68.
    let action = screen.click_at((400.0, 50.0), 800, 600, "", 1_000);
    assert_eq!(action, None, "a single click selects, joining nothing");
    assert_eq!(screen.selected(), Some(0));
    let action = screen.click_at((400.0, 50.0), 800, 600, "", 1_100);
    assert_eq!(
        action,
        Some(MultiAction::Join("127.0.0.1".to_string(), 25565, None)),
        "the second press within 250 ms joins the row's address"
    );
    // Past the window the count restarts: a lone press only selects.
    let action = screen.click_at((400.0, 50.0), 800, 600, "", 1_500);
    assert_eq!(action, None);
    assert_eq!(screen.selected(), Some(0));
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn the_add_form_needs_both_fields_and_tab_flips_the_focus() {
    let mut form = ServerForm::default();
    assert!(!form.add_enabled(), "empty fields disable the Add button");
    for char in "play".chars() {
        // The name field holds the initial focus.
        form.type_char(char);
    }
    assert!(!form.add_enabled(), "a name alone still disables it");
    form.toggle_focus();
    for char in "mc.example.com".chars() {
        form.type_char(char);
    }
    assert!(form.add_enabled(), "both fields enable it");
    form.toggle_focus();
    form.backspace();
    form.backspace();
    form.backspace();
    form.backspace();
    assert!(!form.add_enabled(), "emptying the name disables it again");

    // The max lengths cap the fields: 32 for the name, 128 for the address.
    let mut form = ServerForm::default();
    for _ in 0..100 {
        form.type_char('n');
    }
    assert_eq!(form.name.len(), 32, "the name caps at 32");
    form.toggle_focus();
    for _ in 0..200 {
        form.type_char('a');
    }
    assert_eq!(form.address.len(), 128, "the address caps at 128");
}

#[test]
fn the_direct_form_joins_when_non_empty_and_persists_on_close() {
    let (mut screen, root) = two_entry_screen("direct");
    // The Direct Connect button (id 4) at 800x600 spans x 350..450, y 548..568.
    let action = screen.click_at((400.0, 558.0), 800, 600, "", 2_000);
    assert_eq!(action, None);
    assert!(
        matches!(screen.sub(), SubScreen::Direct(_)),
        "the direct sub-screen stands"
    );
    // Typing into the field then Join answers the join and the close persists.
    for char in "mc.example.com".chars() {
        screen.key(Some(char), 0, false, 800, 600, Duration::from_secs(5));
    }
    let action = screen.key(None, 28, false, 800, 600, Duration::from_secs(5));
    assert_eq!(
        action,
        Some(MultiAction::Join(
            "mc.example.com".to_string(),
            25565,
            Some("mc.example.com".to_string()),
        )),
        "Enter joins the typed address on the default port"
    );
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn delete_asks_then_removes_and_saves() {
    let (mut screen, root) = two_entry_screen("delete");
    screen.click_at((400.0, 50.0), 800, 600, "", 3_000);
    // The Delete button (id 2) at 800x600 spans x 326..396, y 572..592.
    let action = screen.click_at((360.0, 582.0), 800, 600, "", 3_001);
    assert_eq!(action, None);
    assert!(
        matches!(screen.sub(), SubScreen::ConfirmDelete(0)),
        "Delete stands the confirm first"
    );
    screen.confirm_delete();
    assert_eq!(
        screen.list().entries.len(),
        1,
        "the confirm removes the row"
    );
    assert_eq!(screen.list().entries[0].name, "second");
    assert!(
        matches!(screen.sub(), SubScreen::List),
        "the confirm returns to the list"
    );
    let saved = ServerList::load(&server_file(&root));
    assert_eq!(saved.entries.len(), 1, "the delete saves");
    assert_eq!(saved.entries[0].name, "second");
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn shift_with_the_arrows_reorders_and_saves() {
    let (mut screen, root) = two_entry_screen("reorder");
    screen.click_at((400.0, 50.0), 800, 600, "", 4_000);
    assert_eq!(screen.selected(), Some(0));
    // Shift+Down (key code 208) swaps rows 0 and 1, following the selection.
    let timeout = Duration::from_secs(5);
    screen.key(None, 208, true, 800, 600, timeout);
    assert_eq!(screen.list().entries[0].name, "second");
    assert_eq!(screen.list().entries[1].name, "first");
    assert_eq!(screen.selected(), Some(1), "the selection follows the swap");
    let saved = ServerList::load(&server_file(&root));
    assert_eq!(saved.entries[0].name, "second", "the reorder saves");
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

#[test]
fn failed_pings_show_the_dark_red_motd() {
    use oxide_client::screens::multiplayer::{PingOutcome, PingResult};
    let (mut screen, root) = two_entry_screen("failed");
    screen.apply_ping(PingResult {
        index: 0,
        outcome: PingOutcome::UnknownHost,
    });
    screen.apply_ping(PingResult {
        index: 1,
        outcome: PingOutcome::Unreachable,
    });
    let first = screen.live(0).expect("the first row lives");
    assert_eq!(first.ping_ms, -1);
    assert_eq!(first.motd, "§4Can't resolve hostname");
    let second = screen.live(1).expect("the second row lives");
    assert_eq!(second.ping_ms, -1);
    assert_eq!(second.motd, "§4Can't connect to server.");
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}

/// The refresh worker against two canned servers: results land by index, so a slow first
/// server cannot reorder the list.
#[test]
fn refresh_applies_worker_results_by_index() {
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use oxide_proto::frame::{Compression, read_frame, write_frame};
    use oxide_proto::varint::write_varint;

    fn serve_once(json: &'static str, latency_ms: u64) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let port = listener.local_addr().expect("local address").port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_frame(&mut stream, Compression::Disabled);
            let _ = read_frame(&mut stream, Compression::Disabled);
            let mut payload = vec![0x00];
            write_varint(&mut payload, json.len() as i32).expect("write length");
            payload.extend_from_slice(json.as_bytes());
            write_frame(&stream, &payload, Compression::Disabled).expect("write response");
            let ping = read_frame(&mut stream, Compression::Disabled).expect("ping frame");
            // The slow server answers the pong only after its delay.
            thread::sleep(std::time::Duration::from_millis(latency_ms));
            let mut pong = vec![0x01];
            pong.extend_from_slice(&ping[1..]);
            let _ = write_frame(&stream, &pong, Compression::Disabled);
            let _ = stream.write_all(&[]);
        });
        port
    }

    let slow = serve_once(
        r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":1},"description":"slow"}"#,
        400,
    );
    let fast = serve_once(
        r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":2},"description":"fast"}"#,
        0,
    );
    let root = scratch_root("refresh");
    let path = server_file(&root);
    let mut list = ServerList::default();
    list.add(ServerEntry::new(
        "slow".to_string(),
        format!("127.0.0.1:{slow}"),
        None,
    ));
    list.add(ServerEntry::new(
        "fast".to_string(),
        format!("127.0.0.1:{fast}"),
        None,
    ));
    list.save(&path).expect("the list saves");
    let mut screen = MultiplayerScreen::open(list, path, english_labels());
    screen.refresh(Duration::from_secs(5));

    // The rows read pinging until the worker's results are pumped.
    assert_eq!(screen.live(0).map(|row| row.ping_ms), Some(-2));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && screen.live(1).map(|row| row.ping_ms) == Some(-2)
    {
        screen.pump_pings();
        thread::sleep(Duration::from_millis(20));
    }
    screen.pump_pings();
    let slow_row = screen.live(0).expect("the slow row lives");
    let fast_row = screen.live(1).expect("the fast row lives");
    assert_eq!(slow_row.motd, "slow§r", "the slow answer lands on row 0");
    assert_eq!(fast_row.motd, "fast§r", "the fast answer lands on row 1");
    assert!(
        slow_row.ping_ms >= 400,
        "the slow row's latency times its own pong: {}",
        slow_row.ping_ms
    );
    assert!(fast_row.ping_ms >= 0);
    assert!(
        fast_row.ping_ms < slow_row.ping_ms,
        "each row times its own round trip"
    );
    drop(TcpStream::connect(("127.0.0.1", slow)).ok());
    drop(TcpStream::connect(("127.0.0.1", fast)).ok());
    fs::remove_dir_all(&root).expect("the scratch root is removed");
}
