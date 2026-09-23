# M0 Foundations Implementation Plan

> **How to work this plan:** one task at a time, in order. Run each step's verification before moving on, and tick the checkboxes (`- [ ]`) as you go.

**Goal:** Stand up the Oxidecraft workspace so that CI is green, the launcher can fetch and verify the complete 1.8.9 asset store, jar resources are extracted with a manifest, a wgpu window opens with an FPS counter, and the verification rig is proven.

**Architecture:** An eight-crate Cargo workspace with a strict, CI-enforced dependency graph. `oxide-assets` owns all HTTP and the verified on-disk store; `oxide-proto` owns bytes and framing; `oxide-render` owns the GPU; binaries are `oxide-launcher` and `oxide-client`. Every task is TDD: failing test, minimal implementation, passing test, commit.

**Tech Stack:** Rust 2024 edition (rust-version 1.85, developed on 1.95), wgpu, winit, serde/serde_json, flate2 (zlib-ng), sha1, ureq with rustls, zip, clap, tracing, thiserror/anyhow, cargo-deny on GitHub Actions.

**Spec:** `docs/specs/oxidecraft-v1-design.md` (v2). Read section 5 before Task 1, sections 7.1–7.3 before Tasks 4–5, and appendix C.1 before Task 11.

## Global Constraints

- License GPL-3.0. Adapted third-party code must be recorded in `NOTICE`.
- Zero code copied from RustCraft. It is a read-only reference only.
- No Mojang asset, jar, `.class` file, `.ogg`, or `.png` may ever be committed. The extractor must refuse to read `.class` entries.
- Never read `.class` files from the jar at runtime. Design invariant.
- rust-version 1.85, edition 2024. `Cargo.lock` is committed.
- CI must fail on: formatting, clippy warnings, test failure, license violations, crate-graph violations, or a tracked Mojang asset.
- Dependency additions are explicit commits, pinned in `Cargo.lock`; record the resolved versions in the commit message.
- Datastore paths resolve through `XDG_DATA_HOME` when set, else `~/.local/share`, else the platform equivalent via the `dirs` crate.
- Every write to the store is atomic: temp file in the same directory, then rename.

---

### Task 1: Workspace skeleton and the VarInt codec

**Files:**
- Create: `Cargo.toml` (workspace root), `rust-toolchain.toml`, `.gitignore` (update)
- Create: `crates/oxide-proto/Cargo.toml`, `crates/oxide-proto/src/lib.rs`, `crates/oxide-proto/src/varint.rs`
- Create: `crates/oxide-proto-v47/Cargo.toml` + `src/lib.rs`
- Create: `crates/oxide-world/Cargo.toml` + `src/lib.rs`
- Create: `crates/oxide-assets/Cargo.toml` + `src/lib.rs`
- Create: `crates/oxide-render/Cargo.toml` + `src/lib.rs`
- Create: `crates/oxide-game/Cargo.toml` + `src/lib.rs`
- Create: `crates/oxide-launcher/Cargo.toml` + `src/main.rs`
- Create: `crates/oxide-client/Cargo.toml` + `src/main.rs`
- Test: `crates/oxide-proto/tests/varint_vectors.rs`

**Interfaces:**
- Produces: `oxide_proto::varint::{write_varint, read_varint, VarIntError}`; every crate declares `oxide-proto` as its dependency name in `[dependencies]` with `path = "../oxide-proto"`.

- [ ] **Step 1: Create the workspace root**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = [
    "crates/oxide-proto",
    "crates/oxide-proto-v47",
    "crates/oxide-world",
    "crates/oxide-assets",
    "crates/oxide-render",
    "crates/oxide-game",
    "crates/oxide-launcher",
    "crates/oxide-client",
]

[workspace.package]
version = "0.0.0"
edition = "2024"
rust-version = "1.85"
license = "GPL-3.0-only"
repository = "https://github.com/loofyser/Oxidecraft"

[workspace.dependencies]
oxide-proto = { path = "crates/oxide-proto" }
oxide-proto-v47 = { path = "crates/oxide-proto-v47" }
oxide-world = { path = "crates/oxide-world" }
oxide-assets = { path = "crates/oxide-assets" }
oxide-render = { path = "crates/oxide-render" }
oxide-game = { path = "crates/oxide-game" }
thiserror = "2"
tracing = "0.1"

[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
all = "warn"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 2: Create the eight crate manifests and stub sources**

Each `crates/<name>/Cargo.toml` follows this shape (adjust `name` and whether it is a bin):

```toml
[package]
name = "oxide-proto"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[lints]
workspace = true

[dependencies]
thiserror.workspace = true
```

Binaries add:

```toml
[[bin]]
name = "oxide-launcher"
path = "src/main.rs"
```

Each `src/lib.rs` starts as:

```rust
//! <one-line purpose from spec section 5.3>.
```

Each `src/main.rs` starts as:

```rust
fn main() {
    println!("oxide-launcher: not yet implemented");
}
```

- [ ] **Step 3: Write the failing VarInt test**

`crates/oxide-proto/tests/varint_vectors.rs`:

```rust
use oxide_proto::varint::{VarIntError, read_varint, write_varint};

/// Boundary-complete golden vectors: 0 and 1, the 127/128 continuation boundary,
/// a two-byte value, i32::MAX, and a negative value that encodes as five bytes.
const VECTORS: &[(i32, &[u8])] = &[
    (0, &[0x00]),
    (1, &[0x01]),
    (127, &[0x7f]),
    (128, &[0x80, 0x01]),
    (255, &[0xff, 0x01]),
    (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
    (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
];

#[test]
fn encodes_known_vectors() {
    for (value, expected) in VECTORS {
        let mut out = Vec::new();
        write_varint(&mut out, *value).expect("write");
        assert_eq!(&out, expected, "encoding {value}");
    }
}

#[test]
fn decodes_known_vectors() {
    for (value, bytes) in VECTORS {
        let mut cursor = *bytes;
        assert_eq!(read_varint(&mut cursor).expect("read"), *value, "decoding {value}");
    }
}

#[test]
fn round_trips_every_value_in_sampled_range() {
    for value in (0..).step_by(997).take(4_000) {
        let mut out = Vec::new();
        write_varint(&mut out, value).expect("write");
        let mut cursor = &out[..];
        let decoded = read_varint(&mut cursor).expect("read");
        assert_eq!(decoded, value);
    }
}

#[test]
fn rejects_overlong_encoding() {
    // Five continuation bytes, one past the five-byte limit.
    let bytes = [0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    let mut cursor = &bytes[..];
    assert!(matches!(read_varint(&mut cursor), Err(VarIntError::TooLong)));
}

#[test]
fn reports_truncated_input_as_unexpected_eof() {
    // A continuation byte with nothing after it is a truncated VarInt.
    let bytes = [0x80];
    let mut cursor = &bytes[..];
    assert!(matches!(
        read_varint(&mut cursor),
        Err(VarIntError::UnexpectedEof)
    ));
}

#[test]
fn propagates_non_eof_io_errors() {
    struct FailingReader;

    impl std::io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "connection reset",
            ))
        }
    }

    let mut reader = FailingReader;
    assert!(matches!(read_varint(&mut reader), Err(VarIntError::Io(_))));
}
```

- [ ] **Step 4: Run the test and watch it fail**

Run: `cargo test -p oxide-proto --test varint_vectors`
Expected: FAIL, "unresolved import `oxide_proto::varint`".

- [ ] **Step 5: Implement the VarInt codec**

`crates/oxide-proto/src/varint.rs`:

```rust
//! Minecraft VarInt encoding: little-endian 7-bit groups, high bit means continue.

use std::io::{self, Read, Write};

/// Errors produced while decoding a VarInt.
#[derive(Debug, thiserror::Error)]
pub enum VarIntError {
    /// The stream ended in the middle of a VarInt.
    #[error("unexpected end of stream while reading VarInt")]
    UnexpectedEof,
    /// The encoding used more than five bytes.
    #[error("VarInt is longer than five bytes")]
    TooLong,
    /// The underlying reader failed for a reason other than end of stream.
    #[error("io error while reading VarInt: {0}")]
    Io(#[from] io::Error),
}

/// Writes `value` as a VarInt.
pub fn write_varint(mut out: impl Write, value: i32) -> io::Result<()> {
    let mut remaining = value as u32;
    loop {
        let byte = (remaining & 0x7f) as u8;
        remaining >>= 7;
        if remaining == 0 {
            return out.write_all(&[byte]);
        }
        out.write_all(&[byte | 0x80])?;
    }
}

/// Reads one VarInt from `input`.
pub fn read_varint(mut input: impl Read) -> Result<i32, VarIntError> {
    let mut result: u32 = 0;
    for index in 0..5 {
        let mut byte = [0u8; 1];
        input.read_exact(&mut byte).map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => VarIntError::UnexpectedEof,
            _ => VarIntError::Io(error),
        })?;
        result |= u32::from(byte[0] & 0x7f) << (7 * index);
        if byte[0] & 0x80 == 0 {
            return Ok(result as i32);
        }
    }
    Err(VarIntError::TooLong)
}
```

`crates/oxide-proto/src/lib.rs`:

```rust
//! Protocol primitives: framing, codecs, compression, encryption.

pub mod varint;
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p oxide-proto --test varint_vectors`
Expected: 3 passed.

- [ ] **Step 7: Confirm the workspace builds and the graph is what we think**

Run: `cargo build --workspace && cargo metadata --format-version 1 --no-deps | jq -r '.packages[].name' | sort`
Expected: eight crate names, no errors. Record the resolved dependency versions in the commit message.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml rust-toolchain.toml crates .gitignore
git commit -m "feat: create eight-crate workspace with VarInt codec (M0)"
```

---

### Task 2: Length-prefixed framing with compression

**Files:**
- Create: `crates/oxide-proto/src/frame.rs`
- Modify: `crates/oxide-proto/src/lib.rs` (add `pub mod frame;`)
- Test: `crates/oxide-proto/tests/frame.rs`

**Interfaces:**
- Consumes: `oxide_proto::varint`.
- Produces: `oxide_proto::frame::{write_frame, read_frame, Compression, FrameError}` with `Compression::Disabled`, `Compression::Enabled { threshold: i32 }`.

- [ ] **Step 1: Write the failing tests**

`crates/oxide-proto/tests/frame.rs`:

```rust
use std::io::Cursor;

use oxide_proto::frame::{Compression, read_frame, write_frame};

#[test]
fn round_trips_uncompressed_when_disabled() {
    let payload = b"hello minecraft";
    let mut out = Vec::new();
    write_frame(&mut out, payload, Compression::Disabled).expect("write");
    let mut cursor = Cursor::new(out);
    let read = read_frame(&mut cursor, Compression::Disabled).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn small_payload_gets_zero_length_marker_above_threshold() {
    // Payload below the threshold travels as: frame length, zero, raw bytes.
    let payload = [0xABu8; 10];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: 256 }).expect("write");
    assert_eq!(out[0] as usize, out.len() - 1, "first VarInt is the frame length");
    let data_length = oxide_proto::varint::read_varint(&out[1..]).expect("data length");
    assert_eq!(data_length, 0, "uncompressed marker is Data Length 0");
}

#[test]
fn payload_at_threshold_is_compressed_and_round_trips() {
    let payload = vec![0x5Au8; 256];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: 256 }).expect("write");
    let mut cursor = Cursor::new(out);
    let read = read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).expect("read");
    assert_eq!(read, payload);
}

#[test]
fn threshold_minus_one_never_compresses() {
    let payload = vec![0x11u8; 4096];
    let mut out = Vec::new();
    write_frame(&mut out, &payload, Compression::Enabled { threshold: -1 }).expect("write");
    // The frame length is itself a VarInt, so skip it before reading the Data Length marker.
    let mut cursor = &out[..];
    let _frame_len = oxide_proto::varint::read_varint(&mut cursor).expect("frame length");
    let data_length = oxide_proto::varint::read_varint(&mut cursor).expect("data length");
    assert_eq!(data_length, 0);
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-proto --test frame`
Expected: FAIL, `read_frame` not found.

- [ ] **Step 3: Implement framing**

`crates/oxide-proto/src/frame.rs`:

```rust
//! Length-prefixed packet framing, with the 1.8 compression rules.

use std::io::{self, Read, Write};

use flate2::Compression as ZlibLevel;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::varint::{VarIntError, read_varint, write_varint};

/// How the connection compresses frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// No compression, as before Set Compression arrives.
    Disabled,
    /// The server sent a threshold. `-1` disables compression.
    Enabled {
        /// Minimum uncompressed `Packet ID + Data` size that gets compressed.
        threshold: i32,
    },
}

/// Errors from reading or writing frames.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// Underlying IO failure.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The length prefix was malformed.
    #[error("bad varint: {0}")]
    VarInt(#[from] VarIntError),
    /// The frame exceeded the sanity limit.
    #[error("frame length {0} exceeds the {1} byte limit")]
    TooLong(usize, usize),
    /// The declared uncompressed size was not respected.
    #[error("decompressed frame does not match declared Data Length")]
    BadCompression,
}

/// Frames larger than this are rejected outright as hostile.
pub const MAX_FRAME_LEN: usize = 2 * 1024 * 1024 + 1024;

/// Writes one frame: VarInt frame length, then payload, compressing per `mode`.
pub fn write_frame(mut out: impl Write, payload: &[u8], mode: Compression) -> Result<(), FrameError> {
    let compress = match mode {
        Compression::Disabled => false,
        Compression::Enabled { threshold } => threshold >= 0 && payload.len() >= threshold as usize,
    };

    let mut body = Vec::with_capacity(payload.len() + 8);
    if compress {
        write_varint(&mut body, payload.len() as i32)?;
        let mut encoder = ZlibEncoder::new(Vec::new(), ZlibLevel::default());
        encoder.write_all(payload)?;
        body.extend_from_slice(&encoder.finish()?);
    } else if matches!(mode, Compression::Enabled { .. }) {
        write_varint(&mut body, 0)?;
        body.extend_from_slice(payload);
    } else {
        body.extend_from_slice(payload);
    }

    write_varint(&mut out, body.len() as i32)?;
    out.write_all(&body)?;
    Ok(())
}

/// Reads one frame and returns the decompressed `Packet ID + Data` payload.
pub fn read_frame(mut input: impl Read, mode: Compression) -> Result<Vec<u8>, FrameError> {
    let frame_len = read_varint(&mut input)?;
    let frame_len = frame_len.max(0) as usize;
    if frame_len > MAX_FRAME_LEN {
        return Err(FrameError::TooLong(frame_len, MAX_FRAME_LEN));
    }
    let mut body = vec![0u8; frame_len];
    input.read_exact(&mut body)?;

    match mode {
        Compression::Disabled => Ok(body),
        Compression::Enabled { .. } => {
            let mut cursor = &body[..];
            let data_len = read_varint(&mut cursor)?;
            if data_len == 0 {
                return Ok(cursor.to_vec());
            }
            let mut decoder = ZlibDecoder::new(cursor);
            let mut decoded = Vec::with_capacity(data_len as usize);
            decoder.read_to_end(&mut decoded)?;
            Ok(decoded)
        }
    }
}
```

Add `flate2 = { version = "1", features = ["zlib-ng-compat"] }` to `crates/oxide-proto/Cargo.toml`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oxide-proto --test frame`
Expected: 4 passed. If the zlib-ng backend fails to build, fall back to the default backend and note it in the commit message (the C toolchain is present on this machine, so report the real error before switching).

- [ ] **Step 5: Commit**

```bash
git add crates/oxide-proto
git commit -m "feat: add length-prefixed framing with 1.8 compression rules (M0)"
```

---

### Task 3: Handshake, status ping and the `ping` command

**Files:**
- Create: `crates/oxide-proto-v47/src/handshake.rs`, `crates/oxide-proto-v47/src/status.rs`
- Modify: `crates/oxide-proto-v47/src/lib.rs`
- Modify: `crates/oxide-launcher/src/main.rs` (CLI skeleton with clap)
- Test: `crates/oxide-proto-v47/tests/handshake_bytes.rs`

**Interfaces:**
- Consumes: `oxide_proto::frame`, `oxide_proto::varint`.
- Produces: `v47::handshake::write_handshake(out, protocol: i32, host: &str, port: u16, next_state: i32)`; `v47::status::{StatusRequest, StatusResponse, ping_server(host, port, timeout) -> Result<StatusResponse, PingError>}`; CLI `oxide-launcher ping <host:port>`.

- [ ] **Step 1: Write the failing byte-fixture test**

`crates/oxide-proto-v47/tests/handshake_bytes.rs`:

```rust
use oxide_proto_v47::handshake::handshake_payload;

#[test]
fn handshake_payload_matches_wire_format() {
    // Protocol 47, host "localhost", port 25565, next state 1 (status).
    let payload = handshake_payload(47, "localhost", 25565, 1);
    let expected: &[u8] = &[
        0x00, // packet id: handshake
        0x2f, // protocol version 47
        0x09, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't',
        0x63, 0xdd, // port 25565, big endian
        0x01, // next state: status
    ];
    assert_eq!(payload, expected);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p oxide-proto-v47 --test handshake_bytes`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement handshake and status**

`crates/oxide-proto-v47/src/handshake.rs`:

```rust
//! Serverbound Handshake (state: handshake, id 0x00).

/// Builds the handshake payload: packet id, protocol, host, port, next state.
pub fn handshake_payload(protocol: i32, host: &str, port: u16, next_state: i32) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x00);
    oxide_proto::varint::write_varint(&mut out, protocol).expect("vec write");
    write_string(&mut out, host);
    out.extend_from_slice(&port.to_be_bytes());
    oxide_proto::varint::write_varint(&mut out, next_state).expect("vec write");
    out
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    oxide_proto::varint::write_varint(out, value.len() as i32).expect("vec write");
    out.extend_from_slice(value.as_bytes());
}
```

`crates/oxide-proto-v47/src/status.rs`:

```rust
//! Status state: request, response, ping and pong.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use oxide_proto::frame::{Compression, read_frame, write_frame};
use serde::{Deserialize, Serialize};

/// The `description` field of a status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Description {
    /// Legacy colour code prefixed text, when present.
    #[serde(default)]
    pub text: Option<String>,
}

/// A server's status response, as far as Oxidecraft needs it.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    /// Version information the server advertises.
    pub version: VersionInfo,
    /// Connected player counts.
    pub players: Players,
    /// Server description.
    #[serde(default)]
    pub description: Option<Description>,
    /// Favicon, when present.
    #[serde(default)]
    pub favicon: Option<String>,
}

/// The `version` object of a status response.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    /// Human-readable version name, for example `1.8.9`.
    pub name: String,
    /// Protocol number the server speaks.
    pub protocol: i32,
}

/// The `players` object of a status response.
#[derive(Debug, Clone, Deserialize)]
pub struct Players {
    /// Maximum players the server allows.
    pub max: i32,
    /// Players currently online.
    pub online: i32,
}

/// Errors from a status ping.
#[derive(Debug, thiserror::Error)]
pub enum PingError {
    /// Network failure.
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    /// Framing failure.
    #[error("framing error: {0}")]
    Frame(#[from] oxide_proto::frame::FrameError),
    /// The JSON body was not a status response.
    #[error("bad status JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The server closed the connection without answering.
    #[error("connection closed before a status response arrived")]
    Closed,
}

/// Sends a status ping and returns the parsed response.
pub fn ping_server(host: &str, port: u16, timeout: Duration) -> Result<StatusResponse, PingError> {
    let mut stream = TcpStream::connect((host, port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let handshake = crate::handshake::handshake_payload(47, host, port, 1);
    write_frame(&mut stream, &handshake, Compression::Disabled)?;

    // Status Request: empty payload, packet id 0x00.
    write_frame(&mut stream, &[0x00], Compression::Disabled)?;

    let response = read_frame(&mut stream, Compression::Disabled)?;
    if response.first() != Some(&0x00) {
        return Err(PingError::Closed);
    }
    let mut cursor = &response[1..];
    let len = oxide_proto::varint::read_varint(&mut cursor)
        .map_err(|_| PingError::Closed)? as usize;
    let json = cursor.get(..len).ok_or(PingError::Closed)?;
    Ok(serde_json::from_slice(json)?)
}
```

Add `serde = { version = "1", features = ["derive"] }` and `serde_json = "1"` to `oxide-proto-v47`.

- [ ] **Step 4: Run the test and confirm it passes**

Run: `cargo test -p oxide-proto-v47 --test handshake_bytes`
Expected: 1 passed.

- [ ] **Step 5: Add the CLI with clap**

`crates/oxide-launcher/src/main.rs`:

```rust
//! Oxidecraft launcher: fetch and verify assets, then start the client.

use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "oxide-launcher", version, about = "Oxidecraft launcher")]
struct Cli {
    /// Override the data directory.
    #[arg(long, global = true)]
    data_dir: Option<std::path::PathBuf>,
    /// Command to run.
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch and verify the game assets for a version.
    Fetch {
        /// Minecraft version, for example 1.8.9.
        #[arg(long, default_value = "1.8.9")]
        version: String,
        /// Re-hash everything already present.
        #[arg(long)]
        verify: bool,
        /// Resolve and report without downloading.
        #[arg(long)]
        dry_run: bool,
    },
    /// Status ping a server.
    Ping {
        /// Host and port, for example 127.0.0.1:25565.
        address: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Ping { address } => {
            let (host, port) = address
                .rsplit_once(':')
                .ok_or_else(|| anyhow::anyhow!("address must be host:port"))?;
            let status = oxide_proto_v47::status::ping_server(
                host,
                port.parse()?,
                Duration::from_secs(5),
            )?;
            println!(
                "{} — protocol {} — {}/{} players",
                status.version.name, status.version.protocol, status.players.online, status.players.max
            );
            if let Some(description) = status.description.and_then(|d| d.text) {
                println!("MOTD: {description}");
            }
            Ok(())
        }
        Command::Fetch { version, verify, dry_run } => {
            let _ = (version, verify, dry_run);
            anyhow::bail!("fetch is implemented in Task 6")
        }
    }
}
```

Add `clap = { version = "4", features = ["derive"] }`, `anyhow = "1"`, `tracing-subscriber = { version = "0.3", features = ["env-filter"] }`, and `oxide-proto-v47` to `oxide-launcher`'s `[dependencies]` in the workspace dependency table first.

- [ ] **Step 6: Prove the CLI against the rig server**

Start the rig server (`refs/rig/server/start.sh`), then:

Run: `cargo run -p oxide-launcher -- ping 127.0.0.1:25565`
Expected: `1.8.9 — protocol 47 — 0/20 players` and the MOTD line. Paste the real output into the commit. If the rig is not yet up, run the command against any reachable 1.8.9 server and record which one was used instead.

- [ ] **Step 7: Commit**

```bash
git add crates/oxide-proto-v47 crates/oxide-launcher Cargo.toml
git commit -m "feat: handshake and status ping with launcher CLI (M0)"
```

---

### Task 4: The hash-verified store

**Files:**
- Create: `crates/oxide-assets/src/lib.rs`, `crates/oxide-assets/src/http.rs`, `crates/oxide-assets/src/store.rs`
- Test: `crates/oxide-assets/tests/store.rs`

**Interfaces:**
- Produces: `oxide_assets::http::{HttpClient, UreqClient, HttpError}`; `oxide_assets::store::{Store, StoreError, VerifyReport}`; `Store::open(data_dir: PathBuf)`, `Store::object_path(hash: &str) -> PathBuf`, `Store::fetch_object(&HttpClient, hash: &str, size: u64) -> Result<PathBuf, StoreError>`, `Store::verify_object(hash: &str, size: u64) -> Result<(), StoreError>`.

- [ ] **Step 1: Write the failing store tests with a fake transport**

`crates/oxide-assets/tests/store.rs`:

```rust
use std::cell::RefCell;
use std::collections::HashMap;

use oxide_assets::http::{HttpClient, HttpError};
use oxide_assets::store::Store;

struct FakeHttp {
    bodies: HashMap<String, Vec<u8>>,
    calls: RefCell<Vec<String>>,
}

impl FakeHttp {
    fn new(bodies: HashMap<String, Vec<u8>>) -> Self {
        Self { bodies, calls: RefCell::new(Vec::new()) }
    }
}

impl HttpClient for FakeHttp {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.calls.borrow_mut().push(url.to_string());
        self.bodies.get(url).cloned().ok_or_else(|| HttpError::Status { url: url.to_string(), code: 404 })
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn fetches_verifies_and_caches_an_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"first sound".to_vec();
    let hash = sha1_hex(&body);
    let url = format!("https://resources.download.minecraft.net/{}/{hash}", &hash[..2]);
    let http = FakeHttp::new(HashMap::from([(url.clone(), body.clone())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let path = store.fetch_object(&http, &hash, body.len() as u64).expect("fetch");
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).expect("read"), body);
    assert_eq!(http.calls.borrow().len(), 1);

    // Second call must not touch the network.
    store.fetch_object(&http, &hash, body.len() as u64).expect("cache hit");
    assert_eq!(http.calls.borrow().len(), 1);
}

#[test]
fn corrupted_object_is_rejected_and_not_kept() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = b"real bytes".to_vec();
    let hash = sha1_hex(&body);
    let url = format!("https://resources.download.minecraft.net/{}/{hash}", &hash[..2]);
    let http = FakeHttp::new(HashMap::from([(url, b"tampered!!".to_vec())]));
    let store = Store::open(dir.path().to_path_buf()).expect("open");

    let result = store.fetch_object(&http, &hash, body.len() as u64);
    assert!(result.is_err(), "tampered body must fail verification");
    assert!(!store.object_path(&hash).exists(), "nothing may land under the final name");
}
```

Add dev-dependency `tempfile = "3"` and dependency `sha1 = "0.10"`, `hex = "0.4"`, `ureq = { version = "3", default-features = false, features = ["rustls"] }` to `oxide-assets`.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-assets --test store`
Expected: FAIL, `oxide_assets::http` not found.

- [ ] **Step 3: Implement HTTP trait and store**

`crates/oxide-assets/src/http.rs`:

```rust
//! The one place HTTP happens. Everything else takes this trait.

/// Errors from an HTTP fetch.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// Transport failure.
    #[error("transport error for {url}: {message}")]
    Transport {
        /// Requested URL.
        url: String,
        /// Underlying message.
        message: String,
    },
    /// Non-success status code.
    #[error("HTTP {code} for {url}")]
    Status {
        /// Requested URL.
        url: String,
        /// Status code.
        code: u16,
    },
}

/// Fetches bytes by URL. Implementations must follow redirects and set sensible timeouts.
pub trait HttpClient {
    /// Performs a GET and returns the body.
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError>;
}

/// Production client backed by `ureq`, with three tries and exponential backoff.
pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    /// Builds a client with 30 second timeouts.
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build();
        Self { agent: config.into() }
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let mut delay = std::time::Duration::from_secs(1);
        let mut last = None;
        for _ in 0..3 {
            match self.agent.get(url).call() {
                Ok(response) => {
                    let mut body = Vec::new();
                    use std::io::Read;
                    response
                        .into_body()
                        .into_reader()
                        .read_to_end(&mut body)
                        .map_err(|error| HttpError::Transport {
                            url: url.to_string(),
                            message: error.to_string(),
                        })?;
                    return Ok(body);
                }
                Err(error) => {
                    last = Some(error.to_string());
                    std::thread::sleep(delay);
                    delay *= 4;
                }
            }
        }
        Err(HttpError::Transport { url: url.to_string(), message: last.unwrap_or_default() })
    }
}
```

`crates/oxide-assets/src/store.rs`:

```rust
//! The verified on-disk asset store. Every write is atomic.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::http::{HttpClient, HttpError};

/// Errors from store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Filesystem failure.
    #[error("filesystem error at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// Download failure.
    #[error("download failed: {0}")]
    Http(#[from] HttpError),
    /// The fetched bytes did not match the expected hash.
    #[error("hash mismatch for {hash}: expected {expected}, got {actual}")]
    HashMismatch {
        /// Expected hash.
        hash: String,
        /// Hash of the body actually received.
        actual: String,
        /// Expected hash again, spelled out for readability.
        expected: String,
    },
    /// The object was the wrong size.
    #[error("size mismatch for {hash}: expected {expected} bytes, got {actual}")]
    SizeMismatch {
        /// Object hash.
        hash: String,
        /// Expected size.
        expected: u64,
        /// Observed size.
        actual: u64,
    },
}

/// A report of a verification pass.
#[derive(Debug, Default)]
pub struct VerifyReport {
    /// Objects that were missing.
    pub missing: Vec<String>,
    /// Objects whose bytes did not match their hash.
    pub mismatched: Vec<String>,
}

impl VerifyReport {
    /// True when nothing is missing and nothing is mismatched.
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.mismatched.is_empty()
    }
}

/// The asset store rooted at a data directory.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens (creating if needed) the store under `data_dir`.
    pub fn open(data_dir: PathBuf) -> Result<Self, StoreError> {
        let root = data_dir;
        for sub in ["assets/objects", "assets/indexes", "versions", "extracted", "skins"] {
            let path = root.join(sub);
            fs::create_dir_all(&path)
                .map_err(|source| StoreError::Io { path, source })?;
        }
        Ok(Self { root })
    }

    /// The final path an object with `hash` lives at.
    pub fn object_path(&self, hash: &str) -> PathBuf {
        self.root.join("assets/objects").join(&hash[..2]).join(hash)
    }

    /// Fetches an object unless already present and valid, then returns its path.
    pub fn fetch_object(
        &self,
        http: &dyn HttpClient,
        hash: &str,
        size: u64,
    ) -> Result<PathBuf, StoreError> {
        let path = self.object_path(hash);
        if path.exists() {
            if self.verify_object(hash, size).is_ok() {
                return Ok(path);
            }
            // Corrupted: remove and re-fetch.
            fs::remove_file(&path)
                .map_err(|source| StoreError::Io { path: path.clone(), source })?;
        }

        let url = format!("https://resources.download.minecraft.net/{}/{hash}", &hash[..2]);
        let body = http.get(&url)?;
        if body.len() as u64 != size {
            return Err(StoreError::SizeMismatch { hash: hash.to_string(), expected: size, actual: body.len() as u64 });
        }
        let actual = hex::encode(Sha1::digest(&body));
        if !actual.eq_ignore_ascii_case(hash) {
            return Err(StoreError::HashMismatch {
                hash: hash.to_string(),
                expected: hash.to_string(),
                actual,
            });
        }
        write_atomic(&path, &body)?;
        Ok(path)
    }

    /// Re-hashes an object already on disk.
    pub fn verify_object(&self, hash: &str, size: u64) -> Result<(), StoreError> {
        let path = self.object_path(hash);
        let bytes = fs::read(&path).map_err(|source| StoreError::Io { path: path.clone(), source })?;
        if bytes.len() as u64 != size {
            return Err(StoreError::SizeMismatch { hash: hash.to_string(), expected: size, actual: bytes.len() as u64 });
        }
        let actual = hex::encode(Sha1::digest(&bytes));
        if !actual.eq_ignore_ascii_case(hash) {
            return Err(StoreError::HashMismatch {
                hash: hash.to_string(),
                expected: hash.to_string(),
                actual,
            });
        }
        Ok(())
    }
}

/// Writes bytes via a temp file in the same directory, then renames into place.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| StoreError::Io { path: parent.to_path_buf(), source })?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| StoreError::Io { path: parent.to_path_buf(), source })?;
    temp.write_all(bytes).map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;
    temp.flush().map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;
    temp.persist(path)
        .map_err(|error| StoreError::Io { path: path.to_path_buf(), source: error.error })?;
    Ok(())
}
```

`crates/oxide-assets/src/lib.rs`:

```rust
//! Asset store, Mojang distribution client, jar reader, atlas and model baking.

pub mod http;
pub mod store;
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oxide-assets --test store`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/oxide-assets Cargo.toml Cargo.lock
git commit -m "feat: hash-verified atomic asset store with HTTP trait (M0)"
```

---

### Task 5: piston-meta metadata parsing

**Files:**
- Create: `crates/oxide-assets/src/version.rs`, `crates/oxide-assets/src/asset_index.rs`
- Modify: `crates/oxide-assets/src/lib.rs`
- Test: `crates/oxide-assets/tests/metadata.rs` with fixtures under `crates/oxide-assets/tests/fixtures/`

**Interfaces:**
- Produces: `oxide_assets::version::{VersionManifest, VersionEntry, VersionJson, DownloadInfo, AssetIndexInfo, find_version(&VersionManifest, &str) -> Option<&VersionEntry>}`; `oxide_assets::asset_index::{AssetIndex, AssetObject, objects_under_size}`; constants `VERSION_MANIFEST_URL`, `CLIENT_1_8_9_SHA1`, `CLIENT_1_8_9_SIZE`.

- [ ] **Step 1: Create the fixtures by hand**

`crates/oxide-assets/tests/fixtures/version_manifest_v2.json`:

```json
{
  "latest": { "release": "1.8.9", "snapshot": "1.8.9" },
  "versions": [
    { "id": "1.8.9", "type": "release", "url": "https://launchermeta.mojang.com/v1/packages/example/1.8.9.json", "time": "2015-12-09T16:36:48+00:00", "releaseTime": "2015-12-09T16:36:48+00:00", "sha1": "d546f17000000000000000000000000000000000", "complianceLevel": 0 }
  ]
}
```

`crates/oxide-assets/tests/fixtures/1.8.9.json`:

```json
{
  "id": "1.8.9",
  "assets": "1.8",
  "assetIndex": { "id": "1.8", "sha1": "0000000000000000000000000000000000000000", "size": 151888, "totalSize": 114885064, "url": "https://launchermeta.mojang.com/v1/packages/example/1.8.json" },
  "downloads": {
    "client": { "sha1": "3870888a6c3d349d3771a3e9d16c9bf5e076b908", "size": 8461484, "url": "https://piston-data.mojang.com/v1/objects/example/client.jar" }
  },
  "javaVersion": { "component": "jre-legacy", "majorVersion": 8 }
}
```

- [ ] **Step 2: Write the failing metadata tests**

```rust
use oxide_assets::asset_index::AssetIndex;
use oxide_assets::version::{find_version, parse_manifest, parse_version_json};

const MANIFEST: &str = include_str!("fixtures/version_manifest_v2.json");
const VERSION: &str = include_str!("fixtures/1.8.9.json");

#[test]
fn finds_the_1_8_9_entry_and_its_url() {
    let manifest = parse_manifest(MANIFEST).expect("manifest");
    let entry = find_version(&manifest, "1.8.9").expect("1.8.9 present");
    assert!(entry.url.ends_with("1.8.9.json"));
}

#[test]
fn version_json_exposes_asset_index_and_client_jar() {
    let version = parse_version_json(VERSION).expect("version json");
    assert_eq!(version.asset_index.id, "1.8");
    assert_eq!(version.downloads.client.size, 8_461_484);
    assert_eq!(version.downloads.client.sha1, oxide_assets::version::CLIENT_1_8_9_SHA1);
}

#[test]
fn asset_index_parses_objects_and_builds_urls() {
    let index_json = r#"{"objects":{"minecraft/sounds/ambient/cave/cave1.ogg":{"hash":"abc123","size":42}}}"#;
    let index = AssetIndex::parse(index_json).expect("index");
    let object = index.objects.get("minecraft/sounds/ambient/cave/cave1.ogg").expect("object");
    assert_eq!(object.size, 42);
    assert_eq!(object.url(), "https://resources.download.minecraft.net/ab/abc123");
}
```

- [ ] **Step 3: Run the tests and watch them fail**

Run: `cargo test -p oxide-assets --test metadata`
Expected: FAIL, module not found.

- [ ] **Step 4: Implement metadata parsing**

`crates/oxide-assets/src/version.rs`:

```rust
//! piston-meta version metadata.

use serde::Deserialize;

/// Where the version manifest lives.
pub const VERSION_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

/// SHA-1 of the 1.8.9 client jar, verified during research and used as a constant guard.
pub const CLIENT_1_8_9_SHA1: &str = "3870888a6c3d349d3771a3e9d16c9bf5e076b908";
/// Size of the 1.8.9 client jar in bytes.
pub const CLIENT_1_8_9_SIZE: u64 = 8_461_484;

/// The version manifest, trimmed to the fields Oxidecraft reads.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    /// All published versions.
    pub versions: Vec<VersionEntry>,
}

/// One entry in the version manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionEntry {
    /// Version id, for example `1.8.9`.
    pub id: String,
    /// URL of this version's JSON document.
    pub url: String,
    /// SHA-1 of that document.
    pub sha1: String,
}

/// A version JSON document, trimmed to the fields Oxidecraft reads.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionJson {
    /// Version id.
    pub id: String,
    /// Asset index name, `1.8` for the whole 1.8 line.
    pub assets: String,
    /// Asset index descriptor.
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexInfo,
    /// Downloadable artifacts.
    pub downloads: Downloads,
}

/// Descriptor for the asset index.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexInfo {
    /// Index id, `1.8`.
    pub id: String,
    /// URL of the index.
    pub url: String,
    /// SHA-1 of the index.
    pub sha1: String,
    /// Size of the index in bytes.
    pub size: u64,
}

/// Downloadable artifacts for a version.
#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    /// The client jar.
    pub client: DownloadInfo,
}

/// One downloadable file.
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfo {
    /// URL.
    pub url: String,
    /// SHA-1.
    pub sha1: String,
    /// Size in bytes.
    pub size: u64,
}

/// Parses a version manifest.
pub fn parse_manifest(json: &str) -> Result<VersionManifest, serde_json::Error> {
    serde_json::from_str(json)
}

/// Parses a version JSON document.
pub fn parse_version_json(json: &str) -> Result<VersionJson, serde_json::Error> {
    serde_json::from_str(json)
}

/// Finds a version entry by id.
pub fn find_version<'a>(manifest: &'a VersionManifest, id: &str) -> Option<&'a VersionEntry> {
    manifest.versions.iter().find(|entry| entry.id == id)
}
```

`crates/oxide-assets/src/asset_index.rs`:

```rust
//! The hashed asset index.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The 1.8 asset index.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    /// Logical path to object metadata.
    pub objects: BTreeMap<String, AssetObject>,
}

/// One hashed object.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    /// SHA-1 hash, lowercase hex.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
}

impl AssetObject {
    /// The download URL for this object.
    pub fn url(&self) -> String {
        format!("https://resources.download.minecraft.net/{}/{}", &self.hash[..2], self.hash)
    }
}

impl AssetIndex {
    /// Parses an index document.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Total size of every object in the index.
    pub fn total_size(&self) -> u64 {
        self.objects.values().map(|object| object.size).sum()
    }
}
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p oxide-assets --test metadata`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-assets
git commit -m "feat: piston-meta metadata parsing with fixtures (M0)"
```

---

### Task 6: `oxide-launcher fetch --verify`

**Files:**
- Create: `crates/oxide-assets/src/fetch.rs`
- Modify: `crates/oxide-assets/src/lib.rs`, `crates/oxide-launcher/src/main.rs`
- Test: `crates/oxide-assets/tests/fetch_flow.rs`

**Interfaces:**
- Consumes: Task 4 store, Task 5 metadata.
- Produces: `oxide_assets::fetch::{fetch_version, FetchOptions, FetchReport, Progress}`; CLI `oxide-launcher fetch --version 1.8.9 [--verify] [--dry-run]`.

- [ ] **Step 1: Write the failing end-to-end fetch test with a fake transport**

The fake transport serves: the version manifest, the version JSON, the asset index (two objects), and the two objects plus the client jar. Assert after a run: two objects and the jar exist with correct hashes, a second run performs zero downloads, and a corrupted object is repaired instead of trusted.

```rust
#[test]
fn fetch_is_idempotent_and_repairs_corruption() {
    // Build FakeHttp with manifest, version json, index, jar, and two objects.
    // Run fetch_version once: expect 5 downloads (manifest, version, index, jar, 2 objects = 6).
    // Run again: expect 0 downloads.
    // Corrupt one object on disk, run again: expect exactly 1 download (the repaired object).
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p oxide-assets --test fetch_flow`
Expected: FAIL, `fetch_version` not found.

- [ ] **Step 3: Implement the fetch flow**

Implementation outline, in `crates/oxide-assets/src/fetch.rs`:

```rust
/// Options for a fetch run.
pub struct FetchOptions {
    /// Version to fetch.
    pub version: String,
    /// Resolve and report only.
    pub dry_run: bool,
    /// Re-hash everything after downloading.
    pub verify: bool,
}

/// What a fetch run did.
#[derive(Debug, Default)]
pub struct FetchReport {
    /// Files downloaded this run.
    pub downloaded: usize,
    /// Files already present and valid.
    pub reused: usize,
    /// Total bytes transferred.
    pub bytes: u64,
}

/// Fetches, verifies, and extracts everything the client needs for one version.
pub fn fetch_version(
    store: &Store,
    http: &dyn HttpClient,
    options: &FetchOptions,
    mut progress: impl FnMut(Progress),
) -> Result<FetchReport, FetchError>;
```

Steps inside `fetch_version`, in order: manifest → version JSON (verify SHA-1 against the manifest entry) → confirm `id == options.version`, and for 1.8.9 assert the client jar hash and size match the constants → asset index (verify SHA-1) → every object in the index (skip any already valid) → client jar → strict post-verify when `verify` is set. Each step calls `progress(Progress::step(name, done, total))` so the CLI can print. When `dry_run` is set, stop after the index and report the plan without downloading objects or the jar.

- [ ] **Step 4: Wire the CLI**

Replace the `Fetch` arm in `crates/oxide-launcher/src/main.rs` with a call to `fetch_version`, printing each progress step at info level and a final summary line. Use `dirs::data_dir()` when `--data-dir` is absent.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p oxide-assets --test fetch_flow`
Expected: 1 passed, with the download counters asserted.

- [ ] **Step 6: Prove it on the real network and record the evidence**

Run: `cargo run -p oxide-launcher -- fetch --version 1.8.9 --verify`
Expected, on a clean store: 734 objects plus the jar and index, ending with a verified-store summary. Then run it a second time and expect a zero-download report. Paste both summaries into `docs/STATE.md` under "M0 evidence" and into the commit message. Note the total bytes and the wall-clock time; these become the baseline for later startup work.

- [ ] **Step 7: Commit**

```bash
git add crates/oxide-assets crates/oxide-launcher docs/STATE.md
git commit -m "feat: full launcher fetch with verify and dry-run (M0)"
```

---

### Task 7: Jar extraction with a manifest

**Files:**
- Create: `crates/oxide-assets/src/extract.rs`
- Modify: `crates/oxide-assets/src/lib.rs`, `crates/oxide-assets/src/fetch.rs`
- Test: `crates/oxide-assets/tests/extract.rs`

**Interfaces:**
- Produces: `oxide_assets::extract::{Extractor, ExtractionManifest, EXTRACTOR_SCHEMA_VERSION}`; `Extractor::plan(&self) -> Result<Vec<String>, ExtractError>`, `Extractor::run(&self) -> Result<ExtractionReport, ExtractError>`, `Extractor::is_up_to_date(&self) -> bool`.

- [ ] **Step 1: Write the failing tests with a synthetic jar**

Build a jar in the test with the `zip` crate containing: `assets/minecraft/textures/blocks/stone.png` (fake bytes), `assets/minecraft/textures/blocks/stone.png.mcmeta`, `assets/minecraft/lang/en_US.lang`, `pack.mcmeta`, `net/minecraft/client/Minecraft.class` (must be refused), and `META-INF/MANIFEST.MF` (must be refused). Assert:

```rust
#[test]
fn extracts_resources_and_refuses_class_files() {
    // plan() lists the four asset paths and neither refused path.
    // run() writes exactly those four files.
    // No path containing ".class" or "META-INF" exists under the extraction root.
    // is_up_to_date() is true after the run and false after touching the jar.
}

#[test]
fn manifest_records_jar_hash_and_extractor_version() {
    // The manifest file exists at extracted/<version>/.manifest.json,
    // its jar_sha1 equals the synthetic jar's SHA-1,
    // and its schema_version equals EXTRACTOR_SCHEMA_VERSION.
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-assets --test extract`
Expected: FAIL, `oxide_assets::extract` not found.

- [ ] **Step 3: Implement the extractor**

Rules to encode literally: include every entry under `assets/` plus `pack.mcmeta` and `sounds.json`; skip anything ending in `.class` and anything starting with `META-INF/`; write each file atomically with `store::write_atomic`; write the manifest last, with `jar_sha1`, `schema_version`, and a sorted `entries` map of relative path to size. `is_up_to_date` returns true only when the manifest parses, its `jar_sha1` matches the jar on disk, and its `schema_version` equals `EXTRACTOR_SCHEMA_VERSION`.

```rust
/// Schema version of the extraction manifest; bump to force re-extraction.
pub const EXTRACTOR_SCHEMA_VERSION: u32 = 1;
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oxide-assets --test extract`
Expected: 2 passed.

- [ ] **Step 5: Prove it on the real jar**

Run: `cargo run -p oxide-launcher -- fetch --version 1.8.9` on a machine that already has the jar, then:

Run: `ls "$XDG_DATA_HOME/oxidecraft/extracted/1.8.9/assets/minecraft" && jq '.jar_sha1, .schema_version, (.entries | length)' "$XDG_DATA_HOME/oxidecraft/extracted/1.8.9/.manifest.json"`
Expected: the extracted directories (`blockstates`, `font`, `lang`, `models`, `shaders`, `texts`, `textures`), the jar SHA-1 matching `CLIENT_1_8_9_SHA1`, schema version 1, and an entry count in the thousands. Record the count in `docs/STATE.md`.

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-assets crates/oxide-launcher docs/STATE.md
git commit -m "feat: jar extraction with manifest and class-file refusal (M0)"
```

---

### Task 8: wgpu window with an FPS counter

**Files:**
- Create: `crates/oxide-render/src/lib.rs`, `crates/oxide-render/src/fps.rs`, `crates/oxide-render/src/renderer.rs`
- Modify: `crates/oxide-client/src/main.rs`
- Test: `crates/oxide-render/tests/fps.rs`, plus a local-only smoke test `crates/oxide-render/tests/headless.rs`

**Interfaces:**
- Produces: `oxide_render::fps::FpsCounter` with `FpsCounter::new(window: Duration)`, `tick(&mut self, elapsed: Duration)`, `fps(&self) -> f32`; `oxide_render::renderer::Renderer` with `Renderer::new(window: &winit::window::Window) -> Result<Self, RendererError>`, `resize(&mut self, size)`, `render(&mut self) -> Result<(), RendererError>`.

- [ ] **Step 1: Write the failing FPS test**

```rust
use std::time::Duration;

use oxide_render::fps::FpsCounter;

#[test]
fn counts_frames_over_one_second_windows() {
    let mut counter = FpsCounter::new(Duration::from_secs(1));
    for _ in 0..60 {
        counter.tick(Duration::from_micros(16_666));
    }
    assert!((counter.fps() - 60.0).abs() < 2.0, "got {}", counter.fps());
}

#[test]
fn reports_zero_before_a_window_completes() {
    let counter = FpsCounter::new(Duration::from_secs(1));
    assert_eq!(counter.fps(), 0.0);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p oxide-render --test fps`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement the counter and the renderer**

`FpsCounter` accumulates ticks; when the window elapses, it stores `frames / elapsed` and resets. `Renderer::new` creates the wgpu instance with `wgpu::Backends::VULKAN` on Linux and the primary backend elsewhere, requests an adapter (log which adapter was chosen, including driver info from `adapter.get_info()`), creates the device and queue, and configures the surface for the window with an sRGB-aware format choice recorded in a log line. `render` begins a render pass with the vanilla sky-blue clear colour `0.62, 0.76, 0.98` and presents.

`crates/oxide-client/src/main.rs` runs a `winit` application handler: create the window, create the renderer, on every `about_to_wait`/redraw tick the FPS counter, update the window title to `Oxidecraft — 123 fps — <adapter name>`, and exit on Escape or window close.

- [ ] **Step 4: Run the unit test and confirm it passes**

Run: `cargo test -p oxide-render --test fps`
Expected: 2 passed.

- [ ] **Step 5: Run the window and capture evidence**

Run: `cargo run -p oxide-client`
Expected: a window opens with the clear colour; the title shows a live FPS number and the Vulkan adapter name; Escape closes it cleanly with no validation errors in the log. Capture a screenshot with the desktop screenshot tool and save it to `docs/evidence/m0-window.png`, and paste the first 10 log lines (adapter, driver, surface format) into `docs/STATE.md`.

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-render crates/oxide-client docs/evidence docs/STATE.md
git commit -m "feat: wgpu window with FPS counter and adapter logging (M0)"
```

---

### Task 9: CI, cargo-deny, the crate-graph check and the asset guard

**Files:**
- Create: `.github/workflows/ci.yml`, `deny.toml`, `scripts/check-graph.sh`, `scripts/check-assets.sh`
- Test: run all four locally before pushing

**Interfaces:**
- Produces: `scripts/check-graph.sh` exits non-zero on any dependency edge outside the table in spec section 5.1; `scripts/check-assets.sh` exits non-zero when a forbidden file is tracked.

- [ ] **Step 1: Write the crate-graph check and watch it pass, then break it deliberately**

`scripts/check-graph.sh`:

```bash
#!/usr/bin/env bash
# Fails when any crate depends on something outside the allowed edges.
set -euo pipefail

allowed() {
  case "$1 -> $2" in
    "oxide-proto-v47 -> oxide-proto") return 0 ;;
    "oxide-world -> oxide-proto"|"oxide-world -> oxide-proto-v47") return 0 ;;
    "oxide-render -> oxide-assets") return 0 ;;
    "oxide-game -> oxide-proto-v47"|"oxide-game -> oxide-world"|"oxide-game -> oxide-assets"|"oxide-game -> oxide-render") return 0 ;;
    "oxide-client -> "*|"oxide-launcher -> oxide-assets"|"oxide-launcher -> oxide-proto-v47"|"oxide-launcher -> oxide-proto") return 0 ;;
    *) return 1 ;;
  esac
}

fail=0
while IFS= read -r edge; do
  from="${edge%% -> *}"
  to="${edge##* -> }"
  case "$to" in oxide-*) ;; *) continue ;; esac
  if ! allowed "$from" "$to"; then
    echo "forbidden dependency edge: $edge" >&2
    fail=1
  fi
done < <(cargo metadata --format-version 1 --no-deps \
  | jq -r '.packages[] | .name as $n | .dependencies[] | select(.name | startswith("oxide-")) | "\($n) -> \(.name)"' \
  | sort -u)

exit "$fail"
```

Verify: `bash scripts/check-graph.sh` exits 0. Then temporarily add `oxide-world` to `oxide-launcher`'s dependencies, confirm the script fails with the edge named, and revert.

- [ ] **Step 2: Write the asset guard and verify it**

`scripts/check-assets.sh`:

```bash
#!/usr/bin/env bash
# Fails when a Mojang binary, jar, sound, texture, or local reference tree is tracked.
set -euo pipefail

patterns='\.jar$|\.ogg$|glyph_sizes\.bin$|^(refs|vanilla)/|assets/indexes/.*\.json$'
if git ls-files | grep -E "$patterns" >/dev/null; then
  echo "forbidden tracked files:" >&2
  git ls-files | grep -E "$patterns" >&2
  exit 1
fi

# PNGs are allowed only outside asset-shaped paths.
if git ls-files | grep -E '\.png$' | grep -E '^(assets|src)/|/(textures|gui)/' >/dev/null; then
  echo "forbidden tracked image under an asset-shaped path:" >&2
  git ls-files | grep -E '\.png$' | grep -E '^(assets|src)/|/(textures|gui)/' >&2
  exit 1
fi
echo "asset guard: clean"
```

Verify: `bash scripts/check-assets.sh` prints clean. Then `git add -f` a dummy `test.jar`, confirm the guard fails, and undo with `git rm --cached test.jar`.

- [ ] **Step 3: Write `deny.toml`**

```toml
[licenses]
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "Unicode-3.0", "CC0-1.0", "MPL-2.0",
]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"
wildcards = "deny"

[advisories]
ignore = []
```

- [ ] **Step 4: Write the CI workflow**

`.github/workflows/ci.yml` with jobs:

1. `lint`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
2. `test`: `cargo test --workspace`.
3. `portability`: `rustup target add x86_64-pc-windows-gnu aarch64-apple-darwin` then `cargo check --workspace --target <target>` for each.
4. `policy`: `bash scripts/check-graph.sh`, `bash scripts/check-assets.sh`, and `cargo deny check` (installed with `cargo install cargo-deny --locked` or `EmbarkStudios/cargo-deny-action`).

Use `actions/checkout@v4` and `dtolnay/rust-toolchain@stable`; bump the action major versions if GitHub reports them deprecated.

- [ ] **Step 5: Run every job's command locally, then push and confirm green**

Run each of the six commands locally, then:

```bash
git add .github deny.toml scripts
git commit -m "ci: add lint, test, portability, graph, license and asset guard jobs (M0)"
git push origin main
gh run watch
```

Expected: every job green in the run summary. If `deny.toml`'s allow-list is too tight for a real transitive dependency, add the license with a one-line justification in the commit message; never loosen the list silently.

- [ ] **Step 6: Commit any fixes from the first CI run**

```bash
git add -A && git commit -m "fix: first CI run findings (M0)"
```

---

### Task 10: Repository hygiene and the M0 tag

**Files:**
- Modify: `README.md`, `docs/STATE.md`
- Create: `NOTICE`, `CHANGELOG.md`, `docs/evidence/` (directory)

- [ ] **Step 1: Add the required disclaimer to the README**

Add directly under the title, before anything else:

```markdown
**NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.**
```

Also add a "Development status" line pointing at `docs/STATE.md`, and a "Building" section with the exact commands (`cargo build --workspace`, `cargo run -p oxide-launcher -- fetch --version 1.8.9`, `cargo run -p oxide-client`).

- [ ] **Step 2: Create `NOTICE`**

State the license posture and the adaptation policy from spec section 1.1 and D4: GPL-3.0; no Mojang assets committed; RustCraft is a read-only reference with zero code copied; adapted MIT/Apache pieces from Stevenarella and Azalea get a per-file line here when they land.

- [ ] **Step 3: Create `CHANGELOG.md`**

```markdown
# Changelog

## Unreleased — M0 Foundations

- Eight-crate workspace with an enforced dependency graph.
- VarInt and framing codecs with golden-byte tests.
- Status ping and the launcher CLI skeleton.
- Hash-verified atomic asset store.
- piston-meta metadata parsing and the full `fetch --verify` flow.
- Jar extraction with a manifest and `.class` refusal.
- wgpu window with an FPS counter and adapter logging.
- CI: lint, test, portability checks, license check, graph check, asset guard.
```

- [ ] **Step 4: Update `docs/STATE.md`**

Move M0 to done with its evidence links, set M1 as next, and record: resolved dependency versions, the fetch timings and byte counts from Task 6, the extraction entry count from Task 7, the adapter and driver line from Task 8, and any CI caveats.

- [ ] **Step 5: Commit, push, and tag**

```bash
git add -A
git commit -m "docs: M0 complete — README disclaimer, NOTICE, changelog, state (M0)"
git push origin main
git tag -a m0 -m "M0 Foundations: workspace, CI, verified asset store, extraction, wgpu window"
git push origin m0
```

- [ ] **Step 6: Verify M0's four exit criteria explicitly**

1. CI green: `gh run list --limit 1` shows success.
2. Verified store: `cargo run -p oxide-launcher -- fetch --version 1.8.9 --verify` reports a clean store with zero downloads.
3. Window: `cargo run -p oxide-client` shows the FPS counter (evidence in `docs/evidence/m0-window.png`).
4. Rig: `cargo run -p oxide-launcher -- ping 127.0.0.1:25565` reports `1.8.9 — protocol 47`, and `refs/rig/README.md` records the vanilla client reaching its title screen.

---

## Self-Review

**Spec coverage for M0's scope.** Workspace and CI → Tasks 1 and 9. `oxide-launcher fetch --verify` → Tasks 4, 5, 6. Jar extraction with manifest → Task 7. Blank wgpu window with FPS counter → Task 8. README disclaimer → Task 10. Rig proof → Tasks 3 (ping) and 10 (checklist), with the rig itself built outside the plan under `refs/rig/`. Logging and error types → Tasks 1, 3 and 8 (`tracing`, `thiserror`, `anyhow`).

**Known gaps at plan level, resolved by rules rather than left silent.** `cargo-deny`'s allow-list may need a justified addition on the first run (Task 9 step 5). CI action major versions are stated as v4/stable and are to be bumped if deprecated (Task 9 step 4). The exact dependency versions are deliberately resolved at M0 by `cargo add` and pinned by the committed lockfile, because inventing version numbers in this document would be less reliable than resolving them (Task 1 step 7, Task 4).

**Type consistency check.** `Store::fetch_object(&dyn HttpClient, hash, size)` is used unchanged in Task 6. `Compression::Enabled { threshold }` is used unchanged in Tasks 2 and 3. `AssetObject::url()` is used by Task 6. `CLIENT_1_8_9_SHA1` is defined in Task 5 and asserted in Tasks 5 and 7. `EXTRACTOR_SCHEMA_VERSION` is defined and consumed inside Task 7 only.
