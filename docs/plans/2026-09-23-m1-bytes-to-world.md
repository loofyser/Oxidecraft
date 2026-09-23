# M1 Bytes to World Implementation Plan

> **How to work this plan:** one task at a time, in order. Run each step's verification before moving on, and tick the checkboxes (`- [ ]`) as you go. Every commit is made on `main` with explicit `git add` paths.

**Goal:** Reach the world. The client speaks the full connection lifecycle against the rig's 1.8.9 server — framing, handshake, offline login, compression, keepalive and the five connection obligations — parses the world it receives (Chunk Data `0x21` and Map Chunk Bulk `0x26`), stores it, and draws untextured terrain with block colours plus an F3-style overlay, with correct geometry above and below y=128.

**Architecture:** `oxide-proto` gains a buffered framed connection (`Conn`) and the primitive/string codecs. `oxide-proto-v47` gains the login and play packet codecs, plus the column decoder that turns chunk payloads into typed column data. `oxide-world` gains the chunk store and the wire-to-store application rules. `oxide-game` gains the session state machine (network thread, obligations, world application, terrain meshing, debug HUD lines). `oxide-render` gains the depth-tested terrain pipeline, a camera, and a minimal text/overlay pass. `oxide-client` wires the session thread to the window: events in, mesh uploads and overlay text out.

**Tech Stack:** Rust 2024 edition (rust-version 1.85, developed on 1.95), the M0 stack plus two new dependencies: `glam` (matrices, `oxide-render`) and `crossbeam-channel` (session-to-window events, `oxide-game` and `oxide-client`). No `bytemuck`: the workspace forbids `unsafe_code`, which derive-generated `unsafe impl`s trip, so vertex data is serialised by hand (Task 7).

**Spec:** `docs/specs/oxidecraft-v1-design.md` (v3). M1 is the section 13 row "Bytes to world". Read section 8 before Tasks 3–6, section 9 before Task 6, sections 6 and 10 before Tasks 7–11, and appendix C.3 before Task 12. Byte-level detail comes from `docs/research/protocol-47-reference.md`: section 1.4 before Task 3, sections 2.1–2.3 and 3.1–3.4 before Tasks 4–6, section 8 for the worked byte layouts.

## Global Constraints

- License GPL-3.0. Adapted third-party code must be recorded in `NOTICE`.
- Zero code copied from RustCraft. It is a read-only reference only.
- No Mojang asset, jar, `.class` file, `.ogg`, or `.png` may ever be committed. The extractor must refuse to read `.class` entries.
- Never read `.class` files from the jar at runtime. Design invariant.
- rust-version 1.85, edition 2024. `Cargo.lock` is committed.
- CI must fail on: formatting, clippy warnings, test failure, license violations, crate-graph violations, or a tracked Mojang asset.
- Dependency additions are explicit commits, pinned in `Cargo.lock`; record the resolved versions in the commit message.
- Every write to the asset store is atomic: temp file in the same directory, then rename.
- Datastore paths resolve through `XDG_DATA_HOME` when set, else `~/.local/share`, else the platform equivalent via the `dirs` crate.
- The allowed crate edges are exactly the section 5.1 table: `oxide-world → {oxide-proto, oxide-proto-v47}`; `oxide-game → {oxide-proto-v47, oxide-world, oxide-assets, oxide-render}`; `oxide-client → all of the above`; `oxide-render → oxide-assets`. `scripts/check-graph.sh` fails the build on anything else.
- Every public item carries a doc comment (workspace lint `missing_docs`); `unsafe_code` is forbidden workspace-wide.
- No AI or tooling language in commits, code, comments, or committed documents.
- `git add` is always explicit with paths; never `git add -A` or `git add .`.
- Evidence (captures, screenshots, logs) lives under the git-ignored `refs/` tree. Committed documents cite it by path.
- The local gate before every commit is: `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `bash scripts/check-assets.sh`, `bash scripts/check-graph.sh`.
- Values that come off the wire are hostile until validated: every length, mask, and declared size is checked before it sizes an allocation; framing never panics (spec S2).

## Decisions taken in this plan

| # | Decision | Rationale |
| --- | --- | --- |
| 1 | The NBT codec is not in M1 | Protocol 47's Chunk Data and Map Chunk Bulk end at the biome array; the trailing block-entity NBT list arrived in 1.9 (`protocol-47-reference.md` §7 row 4, §3.1). M1 reads no NBT anywhere, so the `simdnbt` decision (appendix A) moves to the first milestone that reads it |
| 2 | Both chunk packets are implemented and both are fixture-tested; the live capture decides which one the rig server actually emits | Spec §16 defers `0x26` emission to "confirm against a live capture in M1". A client must handle both regardless (modded servers send `0x26`), so the capture documents the truth rather than gating the codec |
| 3 | M1 runs network, world store, and meshing on the session thread; the main thread only uploads meshes and draws | Spec §6's four contexts arrive as the features that need them: the 20 Hz tick thread with M3's physics, the meshing pool with M2. M1 keeps one owner per piece of state and a channel between them |
| 4 | Section meshes are rebuilt for a column and its four neighbours when a column is applied | Blunt but correct: border faces must be culled once the neighbour arrives, and z-fighting duplicates are visible otherwise. M2 replaces this with the parallel mesher and dirty-section tracking |
| 5 | Face shading is baked into vertex colours using vanilla's per-face brightness (top 1.0, bottom 0.5, north/south 0.8, east/west 0.6) | Untextured terrain needs face discrimination to be readable at all, and these are the values M2's lighting reuses |
| 6 | Unknown block ids render magenta (1.0, 0.0, 1.0) | A missing palette entry must be unmistakable, not plausible |
| 7 | The debug overlay uses a small embedded 5×7 bitmap font drawn as quads | The real font pipeline (jar `ascii.png`, atlas geometry) is M2's; an embedded font keeps M1 self-contained and untextured, and is throwaway scaffolding |
| 8 | Water and other transparent blocks draw in the single opaque pass in M1 | The three terrain queues (opaque/cutout/translucent) are M2's; M1 only needs correct geometry and colours |
| 9 | The camera is the server's position plus the 1.62 eye height, and the far plane is `view_distance_chunks * 16 * √2` with the vanilla 70° vertical FOV, near 0.05 | Spec §10 pins the projection shape; using it now avoids a rework in M2 |
| 10 | Vertex data is serialised by hand with a pinned byte-layout test; `bytemuck` is not used | `unsafe_code = "forbid"` cannot be overridden by `allow`, and derive-generated `unsafe impl`s trip it. A 20-line serialiser with a test is cheaper than fighting the lint. The serialiser is Task 7 |
| 11 | The client's connection is optional: without `--server` it keeps M0's window behaviour | Keeps the M0 smoke run and its evidence reproducible |
| 12 | One small captured chunk payload is committed as a test fixture, with provenance; the full capture stays under `refs/` | Makes "verified against a live capture" a test that runs in CI, while the bulk of the capture stays out of the repository. The payload is server-generated world data for our own fixed-seed world, not a Mojang asset, and the asset guard is unaffected |

## Open questions for the owner (answers recorded on approval)

1. **NBT codec deferred (Decision 1).** Confirmed as not needed for M1?
2. **`0x26` verification path (Decision 2).** If the live capture shows the vanilla server never emits `0x26` on this rig, the codec's verification rests on a constructed fixture plus the capture's negative evidence. Acceptable?
3. **Exercise compression live.** The vanilla server skips Set Compression for loopback connections (`NetworkManager.isLocalChannel()`); the plan therefore captures and runs acceptance twice — once with a loopback upstream (uncompressed) and once with the proxy's upstream bound to the machine's LAN address (compressed, threshold 256). Confirm this is the agreed way to see the compressed path on the rig.
4. **Embedded debug font (Decision 7).** Confirm the M1 overlay may use the throwaway 5×7 font rather than the jar font.
5. **Committed capture fixture (Decision 12).** Confirm one ~50–120 KB captured column payload may be committed under `crates/oxide-proto-v47/tests/fixtures/`.
6. **New dependencies (Decision 10 and the Tech Stack line).** Confirm `glam` and `crossbeam-channel` as the only additions, with no `bytemuck`.
7. **Live evidence includes a vanilla-side capture of the same structure.** Optional but cheap: the vanilla client teleported to the same mark and photographed for eyeball comparison. Include it, or leave it to M2's parity work?

### Answers recorded on approval (2026-09-23)

| # | Question | Answer |
| --- | --- | --- |
| 1 | NBT codec in M1 | Out of M1. Protocol 47 chunk data carries no block-entity NBT; the `simdnbt` decision moves to the milestone that first reads NBT |
| 2 | `0x26` verification standard | Implement and fixture-test both codecs; the live capture decides what the rig server emits, and a negative result is recorded as evidence |
| 3 | Compression's live evidence | Accepted as proposed: capture and acceptance run once with a loopback upstream (uncompressed) and once with the proxy's upstream over the LAN address (compressed, threshold 256) |
| 4 | Debug overlay font | Accepted: embedded 5×7 bitmap font for M1 |
| 5 | Committed capture fixture | Yes: one or two captured columns plus a provenance manifest |
| 6 | New dependencies | Confirmed: `glam` and `crossbeam-channel` only, no `bytemuck` |
| 7 | Vanilla comparison capture | Yes: Task 12 also photographs the vanilla client at the same mark, for eyeball comparison only |

Owner directive recorded after approval (2026-09-23): singleplayer worlds and Java mod compatibility
(Forge 1.8.9 and `.jar` mods) are added as the project's post-v1 programme, recorded in spec section
17 (v4). Neither item changes M1's scope or the tasks below.

---

### Task 1: The buffered connection and the primitive codecs

**Files:**
- Create: `crates/oxide-proto/src/conn.rs`
- Create: `crates/oxide-proto/src/codec.rs`
- Modify: `crates/oxide-proto/src/lib.rs` (add `pub mod codec;` and `pub mod conn;`)
- Test: `crates/oxide-proto/tests/conn.rs`
- Test: `crates/oxide-proto/tests/codec.rs`

**Interfaces:**
- Consumes: `oxide_proto::frame::{write_frame, read_frame, Compression, FrameError}` and `oxide_proto::varint`.
- Produces: `oxide_proto::conn::Conn<S>` with `new`, `compression`, `set_compression`, `send`, `recv`, and `Read`/`Write` implementations; `oxide_proto::codec::{CodecError, MAX_STRING_BYTES, read_u8, read_u16, read_i16, read_i32, read_i64, read_f32, read_f64, read_bool, read_string, read_uuid, write_u8, write_u16, write_i16, write_i32, write_i64, write_f32, write_f64, write_bool, write_string, write_uuid}`.

- [ ] **Step 1: Write the failing connection tests**

`crates/oxide-proto/tests/conn.rs`:

```rust
//! Tests for the buffered framed connection: write coalescing, buffered reads,
//! and the compression switch.

use std::io::{Read, Write};

use oxide_proto::conn::Conn;
use oxide_proto::frame::Compression;

/// An in-memory stream that records how the connection calls the underlying
/// reader and writer: one entry per `read` call and one byte count per `write`
/// call, plus everything written.
#[derive(Default)]
struct CountingStream {
    to_read: std::collections::VecDeque<u8>,
    written: Vec<u8>,
    write_calls: Vec<usize>,
    read_calls: usize,
}

impl CountingStream {
    fn with_input(bytes: &[u8]) -> Self {
        Self {
            to_read: bytes.iter().copied().collect(),
            ..Self::default()
        }
    }
}

impl Read for CountingStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.read_calls += 1;
        let mut n = 0;
        while n < out.len() {
            match self.to_read.pop_front() {
                Some(byte) => {
                    out[n] = byte;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }
}

impl Write for CountingStream {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.write_calls.push(data.len());
        self.written.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn one_send_reaches_the_stream_as_one_write() {
    // The M0 carry-over: the VarInt path issued one write per byte. A buffered
    // writer must hand the whole frame to the stream in a single call.
    let stream = CountingStream::default();
    let mut conn = Conn::new(stream);
    conn.send(&[0x00, 0x7f]).expect("send");
    let writes = conn.into_inner().write_calls;
    assert_eq!(writes.len(), 1, "writes: {writes:?}");
}

#[test]
fn frames_round_trip_through_the_connection() {
    let stream = CountingStream::with_input(&[]);
    let mut conn = Conn::new(stream);
    conn.send(b"first").expect("send");
    conn.send(b"second").expect("send");
    let stream = conn.into_inner();

    let mut reading = Conn::new(CountingStream::with_input(&stream.written));
    assert_eq!(reading.recv().expect("recv"), b"first");
    assert_eq!(reading.recv().expect("recv"), b"second");
    // The second frame was already in the buffer, so the stream was read once.
    assert_eq!(reading.into_inner().read_calls, 1);
}

#[test]
fn compression_applies_after_the_switch() {
    let payload = vec![0x5a_u8; 4096];
    let mut sending = Conn::new(CountingStream::default());
    sending.set_compression(Compression::Enabled { threshold: 256 });
    sending.send(&payload).expect("send");
    let bytes = sending.into_inner().written;

    let mut receiving = Conn::new(CountingStream::with_input(&bytes));
    receiving.set_compression(Compression::Enabled { threshold: 256 });
    assert_eq!(receiving.recv().expect("recv"), payload);
}

#[test]
fn a_truncated_stream_is_an_error_not_a_panic() {
    let mut conn = Conn::new(CountingStream::with_input(&[0x05, 0x01]));
    assert!(conn.recv().is_err(), "a short frame must be reported");
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-proto --test conn`
Expected: FAIL, "unresolved import `oxide_proto::conn`".

- [ ] **Step 3: Implement the connection**

`crates/oxide-proto/src/conn.rs`:

```rust
//! A framed connection over one stream: buffered reads and coalesced writes.

use std::io::{self, Read, Write};

use crate::frame::{Compression, FrameError, read_frame, write_frame};

/// Bytes pulled from the stream in one read call.
const READ_CHUNK: usize = 8 * 1024;

/// A connection that frames packets, buffering both directions.
///
/// Reads fill an internal buffer and are served from it, so a burst of packets
/// costs one stream read rather than one per field. Writes are appended to a
/// buffer and flushed once per [`Conn::send`], so a frame reaches the stream in
/// a single write call.
#[derive(Debug)]
pub struct Conn<S> {
    /// The underlying stream.
    stream: S,
    /// The read buffer and the window of it already consumed.
    read_buf: Vec<u8>,
    read_start: usize,
    read_end: usize,
    /// Bytes staged for the next flush.
    write_buf: Vec<u8>,
    /// The framing mode in force.
    compression: Compression,
}

impl<S: Read + Write> Conn<S> {
    /// Wraps `stream`.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_buf: vec![0; READ_CHUNK],
            read_start: 0,
            read_end: 0,
            write_buf: Vec::new(),
            compression: Compression::Disabled,
        }
    }

    /// The framing mode in force.
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Switches the framing mode, as Set Compression requires.
    pub fn set_compression(&mut self, mode: Compression) {
        self.compression = mode;
    }

    /// Frames and sends one `Packet ID + Data` payload.
    pub fn send(&mut self, payload: &[u8]) -> Result<(), FrameError> {
        // `compression` is `Copy`: hoist it before the reborrow of `*self`, or the
        // borrow checker refuses the immutable read during the mutable one.
        let compression = self.compression;
        write_frame(&mut *self, payload, compression)?;
        self.flush().map_err(FrameError::Io)
    }

    /// Reads one frame and returns the decompressed payload.
    pub fn recv(&mut self) -> Result<Vec<u8>, FrameError> {
        let compression = self.compression;
        read_frame(&mut *self, compression)
    }

    /// Returns the wrapped stream.
    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Refills the read buffer from the stream.
    fn fill(&mut self) -> io::Result<()> {
        self.read_start = 0;
        self.read_end = self.stream.read(&mut self.read_buf)?;
        Ok(())
    }
}

impl<S: Read + Write> Read for Conn<S> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.read_start == self.read_end {
            self.fill()?;
        }
        let available = self.read_end - self.read_start;
        let n = available.min(out.len());
        out[..n].copy_from_slice(&self.read_buf[self.read_start..self.read_start + n]);
        self.read_start += n;
        Ok(n)
    }
}

impl<S: Read + Write> Write for Conn<S> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.write_buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.write_buf.is_empty() {
            self.stream.write_all(&self.write_buf)?;
            self.write_buf.clear();
        }
        self.stream.flush()
    }
}
```

- [ ] **Step 4: Write the failing codec tests**

`crates/oxide-proto/tests/codec.rs`:

```rust
//! Tests for the primitive and string codecs, including their hostile-input paths.

use std::io::Cursor;

use oxide_proto::codec::{
    CodecError, MAX_STRING_BYTES, read_bool, read_i32, read_string, read_u16, write_i32,
    write_string,
};

#[test]
fn integers_are_written_big_endian() {
    let mut out = Vec::new();
    write_i32(&mut out, -2).expect("write");
    assert_eq!(out, [0xff, 0xff, 0xff, 0xfe]);
}

#[test]
fn a_string_is_length_prefixed_utf8() {
    let mut out = Vec::new();
    write_string(&mut out, "OxideDev").expect("write");
    assert_eq!(out, b"\x08OxideDev");

    let mut cursor = Cursor::new(out);
    assert_eq!(read_string(&mut cursor, 16).expect("read"), "OxideDev");
}

#[test]
fn a_string_longer_than_its_cap_is_refused() {
    // 17 bytes against a 16-byte username cap.
    let mut bytes = vec![17u8];
    bytes.extend_from_slice(b"0123456789abcdefg");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::TooLong { max: 16, .. })
    ));
}

#[test]
fn a_string_longer_than_the_protocol_cap_is_refused() {
    let mut bytes = Vec::new();
    oxide_proto::varint::write_varint(&mut bytes, (MAX_STRING_BYTES + 1) as i32)
        .expect("write length");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, MAX_STRING_BYTES),
        Err(CodecError::TooLong { .. })
    ));
}

#[test]
fn invalid_utf8_is_refused() {
    let mut bytes = vec![2u8];
    bytes.extend_from_slice(&[0xff, 0xfe]);
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::BadUtf8)
    ));
}

#[test]
fn a_negative_string_length_is_refused() {
    let mut bytes = Vec::new();
    oxide_proto::varint::write_varint(&mut bytes, -1).expect("write length");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::NegativeLength(-1))
    ));
}

#[test]
fn a_truncated_string_is_reported_as_eof() {
    let mut bytes = vec![6u8];
    bytes.extend_from_slice(b"abc");
    let mut cursor = Cursor::new(bytes);
    assert!(matches!(
        read_string(&mut cursor, 16),
        Err(CodecError::Io(_))
    ));
}

#[test]
fn booleans_read_any_non_zero_byte_as_true() {
    for (byte, expected) in [(0u8, false), (1u8, true), (0x7f, true)] {
        let mut cursor = Cursor::new(vec![byte]);
        assert_eq!(read_bool(&mut cursor).expect("read"), expected);
    }
}

#[test]
fn unsigned_and_signed_widths_are_read_back_correctly() {
    let mut cursor = Cursor::new(vec![0xff, 0xff]);
    assert_eq!(read_u16(&mut cursor).expect("read"), 0xffff);
    // Two bytes cannot fill an i32; the same value needs four.
    let mut cursor = Cursor::new(vec![0x00, 0x00, 0xff, 0xff]);
    assert_eq!(read_i32(&mut cursor).expect("read"), 65_535);
}
```

- [ ] **Step 5: Implement the codec module**

`crates/oxide-proto/src/codec.rs`:

```rust
//! Primitive and string codecs: the fixed-width types, and length-prefixed UTF-8.

use std::io::{self, Read, Write};

use crate::varint::{VarIntError, read_varint, write_varint};

/// The largest string the protocol allows in any field (the Chat cap).
pub const MAX_STRING_BYTES: usize = 32767;

/// Errors from decoding a primitive or a string.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// The stream ended in the middle of the value, or another IO failure occurred.
    #[error("io error while decoding: {0}")]
    Io(#[from] io::Error),
    /// A VarInt inside the value was malformed.
    #[error("bad varint: {0}")]
    VarInt(#[from] VarIntError),
    /// A length-prefixed value was longer than its cap allows.
    #[error("value of {len} bytes exceeds the {max} byte cap")]
    TooLong {
        /// The length the stream declared.
        len: usize,
        /// The cap in force.
        max: usize,
    },
    /// A declared length was negative.
    #[error("negative length: {0}")]
    NegativeLength(i32),
    /// The bytes were not valid UTF-8.
    #[error("string is not valid UTF-8")]
    BadUtf8,
}

/// Reads a fixed-width big-endian integer of the requested shape.
macro_rules! read_int {
    ($name:ident, $ty:ty, $width:expr) => {
        /// Reads a big-endian value.
        pub fn $name(input: impl Read) -> Result<$ty, CodecError> {
            let mut bytes = [0u8; $width];
            read_exact(input, &mut bytes)?;
            Ok(<$ty>::from_be_bytes(bytes))
        }
    };
}

read_int!(read_u16, u16, 2);
read_int!(read_i16, i16, 2);
read_int!(read_i32, i32, 4);
read_int!(read_i64, i64, 8);
read_int!(read_f32, f32, 4);
read_int!(read_f64, f64, 8);
```

Continue the same module with the plain-byte and write-side items:

```rust
/// Reads one byte.
pub fn read_u8(input: impl Read) -> Result<u8, CodecError> {
    let mut byte = [0u8; 1];
    read_exact(input, &mut byte)?;
    Ok(byte[0])
}

/// Reads a boolean: zero is false, anything else is true, as vanilla reads it.
pub fn read_bool(input: impl Read) -> Result<bool, CodecError> {
    Ok(read_u8(input)? != 0)
}

/// Reads a length-prefixed UTF-8 string, refusing anything over `max_bytes`.
pub fn read_string(mut input: impl Read, max_bytes: usize) -> Result<String, CodecError> {
    let len = read_varint(&mut input)?;
    if len < 0 {
        return Err(CodecError::NegativeLength(len));
    }
    let len = len as usize;
    if len > max_bytes {
        return Err(CodecError::TooLong {
            len,
            max: max_bytes,
        });
    }
    let mut bytes = vec![0u8; len];
    read_exact(&mut input, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| CodecError::BadUtf8)
}

/// Reads a 16-byte UUID in the play-state wire form.
pub fn read_uuid(input: impl Read) -> Result<[u8; 16], CodecError> {
    let mut bytes = [0u8; 16];
    read_exact(input, &mut bytes)?;
    Ok(bytes)
}

/// Writes a big-endian value.
pub fn write_i32(mut out: impl Write, value: i32) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

/// Writes a length-prefixed UTF-8 string.
pub fn write_string(mut out: impl Write, value: &str) -> io::Result<()> {
    write_varint(&mut out, value.len() as i32)?;
    out.write_all(value.as_bytes())
}

/// Writes a UUID in the play-state wire form.
pub fn write_uuid(mut out: impl Write, uuid: &[u8; 16]) -> io::Result<()> {
    out.write_all(uuid)
}

/// Reads exactly `bytes.len()` bytes, mapping a short read to `UnexpectedEof`.
fn read_exact(mut input: impl Read, bytes: &mut [u8]) -> Result<(), CodecError> {
    input.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            CodecError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the stream ended inside a value",
            ))
        } else {
            CodecError::Io(error)
        }
    })
}
```

Add the remaining writers (`write_u8`, `write_u16`, `write_i16`, `write_i64`, `write_f32`, `write_f64`, `write_bool`) in the same style: `out.write_all(&value.to_be_bytes())`, and `write_bool` writing one byte, 1 for true and 0 for false. Write the full set so no code later hand-rolls a `to_be_bytes` call outside this module.

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p oxide-proto`
Expected: the new conn (4) and codec (9) tests pass alongside the M0 suites.

- [ ] **Step 7: Commit**

```bash
git add crates/oxide-proto/src/conn.rs crates/oxide-proto/src/codec.rs crates/oxide-proto/src/lib.rs crates/oxide-proto/tests/conn.rs crates/oxide-proto/tests/codec.rs
git commit -m "feat: add the buffered framed connection and the primitive codecs (M1)"
```

---

### Task 2: The live capture — proxy, capture runs, findings, and fixtures

**Files:**
- Create (rig, git-ignored): `refs/rig/tools/record_proxy.py`
- Create (rig, git-ignored): `refs/rig/tools/analyse_capture.py`
- Create: `docs/research/protocol-47-live-capture.md`
- Create: `crates/oxide-proto-v47/tests/fixtures/m1-capture/manifest.json`
- Create: `crates/oxide-proto-v47/tests/fixtures/m1-capture/column-<cx>_<cz>.bin` (one file per extracted column; the first one is the player's spawn column)
- Create (rig, git-ignored): `refs/rig/evidence/m1/capture-run-a-*.bin`, `refs/rig/evidence/m1/capture-run-b-*.bin`, `refs/rig/evidence/m1/capture-report.json`

**Interfaces:**
- Produces: the fixture files and their manifest, consumed by Task 5's decoder tests; the findings document, cited by Task 12 and by `docs/STATE.md` at close-out.
- The manifest's exact shape (Task 5 reads it):

```json
{
  "captured": "2026-09-23",
  "server": { "version": "1.8.9", "jar_sha1": "b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd", "seed": "oxidecraft" },
  "player": { "name": "OxideRef", "x": -23.5, "y": 71.0, "z": 118.5 },
  "packets": { "0x21": 0, "0x26": 0, "0x00": 0, "0x08": 0 },
  "compression": { "run_a": { "set_compression": false, "threshold": null },
                   "run_b": { "set_compression": true, "threshold": 256 } },
  "columns": [
    { "chunk_x": 0, "chunk_z": 0, "file": "column-0_0.bin", "mask": "0x00ff",
      "sky": true, "ground_up": true, "size": 98432, "sha1": "<hex>",
      "block_counts": { "0": 0, "1": 0, "2": 0, "3": 0, "7": 16 } }
  ]
}
```

- [ ] **Step 1: Write the recording proxy**

`refs/rig/tools/record_proxy.py` — a TCP proxy that listens on `--listen` (default `127.0.0.1:25565`) and forwards to `--upstream` (default `127.0.0.1:25566`), logging each direction's raw bytes into `--out`-prefixed files:

```python
#!/usr/bin/env python3
"""Recording proxy for the rig: forwards TCP both ways, logging raw bytes.

Usage:
  python3 record_proxy.py --listen 0.0.0.0:25565 --upstream 127.0.0.1:25566 \
      --out refs/rig/evidence/m1/capture-run-a
Writes <out>.client.bin (client to server) and <out>.server.bin (server to client),
each a raw byte log in arrival order. The analyser replays the framing.
"""
import argparse, socket, threading

def pump(src, dst, sink_path, done):
    with open(sink_path, "wb") as sink:
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                sink.write(data)
                sink.flush()
                dst.sendall(data)
        except OSError:
            pass
        finally:
            done.set()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--listen", default="127.0.0.1:25565")
    ap.add_argument("--upstream", default="127.0.0.1:25566")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    lhost, lport = args.listen.rsplit(":", 1)
    uhost, uport = args.upstream.rsplit(":", 1)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((lhost, int(lport)))
    listener.listen(1)
    print(f"listening on {args.listen}, upstream {args.upstream}", flush=True)
    client, addr = listener.accept()
    print(f"connection from {addr}", flush=True)
    server = socket.create_connection((uhost, int(uport)))
    done = threading.Event()
    threading.Thread(target=pump, args=(client, server, args.out + ".client.bin", done), daemon=True).start()
    threading.Thread(target=pump, args=(server, client, args.out + ".server.bin", done), daemon=True).start()
    done.wait()
    client.close(); server.close(); listener.close()

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Write the analyser**

`refs/rig/tools/analyse_capture.py` — an independent implementation of the framing rules (this is the point: it must not share code with the Rust client). It reads a `<name>.client.bin` / `<name>.server.bin` pair, tracks the handshaking → login → play state and the compression switch, splits frames, decodes each frame's VarInt packet id, prints a histogram per state, detects the login packet order, computes keepalive intervals, and for each `0x21` frame extracts `(chunk_x, chunk_z, ground_up, mask, size)` plus optionally writes the column payload out to a file. Requirements:

- Uncompressed frames: `VarInt length`, then `Packet ID + Data`.
- After a frame whose id is `0x03` in the login state, switch to compressed framing: `VarInt length`, `VarInt data length`, then either raw (data length 0) or zlib.
- Report a desync loudly: a frame it cannot parse prints the byte offset reached and exits non-zero.
- `--extract-column cx cz <out.bin>` writes one column's `Data` bytes (the declared `size` bytes after the header fields) and prints mask, ground_up and size as JSON.
- `--json <path>` writes the full report used to fill the manifest.
- `--block-counts <column.bin>` prints per-block-id counts, reading the LE u16 array (useful to fill `block_counts` and to pick fixture assertions).

Run it with the system Python: `python3 refs/rig/tools/analyse_capture.py ...` (no third-party modules; `zlib` is in the standard library).

- [ ] **Step 3: Point the server at the proxy port and start both runs**

The server's port is a rig setting, not a repository setting:

```bash
cd /home/lucy/Desktop/Software/Projects/Oxidecraft/refs/rig/server
# Move the server behind the proxy: the proxy takes 25565, the server takes 25566.
# Edit server.properties: server-port=25566  (record the change in the findings document)
./stop.sh 2>/dev/null; ./start.sh
```

Run A (loopback upstream; expect no Set Compression, because the server sees a loopback peer):

```bash
cd /home/lucy/Desktop/Software/Projects/Oxidecraft
python3 refs/rig/tools/record_proxy.py --listen 127.0.0.1:25565 --upstream 127.0.0.1:25566 \
  --out refs/rig/evidence/m1/capture-run-a &
cd refs/rig/client && ./launch-client.sh --join    # vanilla client joins 127.0.0.1:25565 through the proxy
```

Wait for the vanilla client to reach the world (its window shows terrain), then quit it, then stop the proxy. Run B (upstream over the LAN address; the server then treats the peer as remote and sends Set Compression):

```bash
ip -4 addr show scope global | awk '/inet /{print $2}' | cut -d/ -f1    # the machine's LAN address
python3 refs/rig/tools/record_proxy.py --listen 127.0.0.1:25565 \
  --upstream <lan-address>:25566 --out refs/rig/evidence/m1/capture-run-b &
cd refs/rig/client && ./launch-client.sh --join
```

If no usable LAN address exists, record that in the findings document and note that the compressed path's live evidence is the synthetic test suite instead. Do not change `online-mode` or any other rig invariant.

- [ ] **Step 4: Analyse both runs and extract the fixtures**

```bash
python3 refs/rig/tools/analyse_capture.py refs/rig/evidence/m1/capture-run-a --json refs/rig/evidence/m1/capture-report.json
python3 refs/rig/tools/analyse_capture.py refs/rig/evidence/m1/capture-run-b --json refs/rig/evidence/m1/capture-report-b.json
```

Then extract the player's spawn column from run A (and, when present, one `0x26` payload from run B) into `crates/oxide-proto-v47/tests/fixtures/m1-capture/`, compute each file's SHA-1 with `sha1sum`, and fill the manifest. Keep the extracted files to the two or three columns the tests need — the whole 441-column load is 40+ MB and stays under `refs/`.

- [ ] **Step 5: Write the findings document**

`docs/research/protocol-47-live-capture.md`, sections:

1. **Procedure** — the proxy, the two runs, the server-port change, and the exact commands.
2. **Observed login sequence** — the order of Login Success and Set Compression in each run, the threshold value in run B, and whether run A carried compression at all.
3. **Chunk packets** — how many `0x21` and `0x26` frames each run carried. The M0 carry-over and spec §16 defer `0x26` emission to this evidence: state plainly whether the vanilla server emitted it. If the capture carries `0x26`, extract a payload and test it (Task 5 consumes it); if not, record the negative result and cite it in the section, and note that the codec is verified against the constructed fixture and kept for modded servers.
4. **Obligations observed** — the keepalive interval, the order of Client Settings relative to Join Game (both observed from the vanilla client), and the position echo after Player Position And Look — each as byte-level observations of what a correct client must answer.
5. **Fixtures** — the extracted files, their SHA-1s, their masks and sizes, and the block-id counts the tests assert.
6. **Rig changes** — the `server-port=25566` edit and how to revert it (`server-port=25565`, restart, or leave it: the findings document says which).

No AI or tooling language anywhere in this document.

- [ ] **Step 6: Commit**

```bash
git add docs/research/protocol-47-live-capture.md crates/oxide-proto-v47/tests/fixtures/m1-capture
git commit -m "docs: add the 1.8.9 live capture findings and the chunk fixtures (M1)"
```

Note in the commit message the resolved state of the rig (server port 25566 behind the proxy) so the next task's operator is not surprised.

---

### Task 3: Login-state packets and the compression handover

**Files:**
- Create: `crates/oxide-proto-v47/src/clientbound.rs`
- Create: `crates/oxide-proto-v47/src/serverbound.rs`
- Modify: `crates/oxide-proto-v47/src/lib.rs` (add `pub mod clientbound;` and `pub mod serverbound;`, and the shared `PacketError`)
- Test: `crates/oxide-proto-v47/tests/login_packets.rs`

**Interfaces:**
- Consumes: `oxide_proto::codec` and `oxide_proto::varint`.
- Produces: `oxide_proto_v47::{PacketError}`, `clientbound::{LoginPacket, decode_login}`, `serverbound::{write_login_start, LOGIN_START_ID}`, `pub const PROTOCOL: i32 = 47`, `pub const NEXT_STATE_LOGIN: i32 = 2`, and `clientbound::read_packet_id(&[u8]) -> Result<(i32, &[u8]), PacketError>`.

- [ ] **Step 1: Write the failing login tests**

`crates/oxide-proto-v47/tests/login_packets.rs`:

```rust
//! Golden-byte tests for the login-state packets, including the hostile paths.

use oxide_proto_v47::clientbound::{LoginPacket, decode_login};
use oxide_proto_v47::serverbound::{LOGIN_START_ID, write_login_start};

#[test]
fn login_start_is_id_then_a_length_prefixed_name() {
    let mut out = Vec::new();
    write_login_start(&mut out, "OxideDev").expect("write");
    assert_eq!(out, b"\x00\x08OxideDev");
    assert_eq!(LOGIN_START_ID, 0x00);
}

#[test]
fn set_compression_carries_the_threshold_as_a_varint() {
    // 256 encodes as 0x80 0x02.
    let mut payload = vec![0x03];
    oxide_proto::varint::write_varint(&mut payload, 256).expect("write");
    match decode_login(&payload).expect("decode") {
        LoginPacket::SetCompression { threshold } => assert_eq!(threshold, 256),
        other => panic!("expected Set Compression, got {other:?}"),
    }
}

#[test]
fn login_success_carries_a_hyphenated_uuid_and_the_name() {
    let body = "069a79f4-44e9-4726-a5be-fca90e38aaf5OxideDev";
    let mut payload = vec![0x02, body.len() as u8];
    payload.extend_from_slice(body.as_bytes());
    match decode_login(&payload).expect("decode") {
        LoginPacket::LoginSuccess { uuid, username } => {
            assert_eq!(uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
            assert_eq!(username, "OxideDev");
        }
        other => panic!("expected Login Success, got {other:?}"),
    }
}

#[test]
fn a_disconnect_carries_chat_json() {
    let json = r#"{"text":"You are not whitelisted"}"#;
    let mut payload = vec![0x00, json.len() as u8];
    payload.extend_from_slice(json.as_bytes());
    match decode_login(&payload).expect("decode") {
        LoginPacket::Disconnect { reason } => assert_eq!(reason, json),
        other => panic!("expected Disconnect, got {other:?}"),
    }
}

#[test]
fn an_encryption_request_is_decoded_so_it_can_be_refused_clearly() {
    let mut payload = vec![0x01, 0x00, 0x03, 0xaa, 0xbb, 0xcc, 0x04, 0x01, 0x02, 0x03, 0x04];
    match decode_login(&payload).expect("decode") {
        LoginPacket::EncryptionRequest {
            server_id,
            public_key,
            verify_token,
        } => {
            assert_eq!(server_id, "");
            assert_eq!(public_key, vec![0xaa, 0xbb, 0xcc]);
            assert_eq!(verify_token, vec![1, 2, 3, 4]);
        }
        other => panic!("expected Encryption Request, got {other:?}"),
    }
}

#[test]
fn a_truncated_payload_is_an_error() {
    assert!(decode_login(&[0x02, 0x10, b'x']).is_err());
    assert!(decode_login(&[]).is_err());
}

#[test]
fn trailing_bytes_are_refused() {
    // Strictness is a bug-catcher: a payload that decodes with leftovers means
    // the field list and the wire disagree.
    let mut payload = vec![0x03];
    oxide_proto::varint::write_varint(&mut payload, 100).expect("write");
    payload.push(0x00);
    assert!(decode_login(&payload).is_err());
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-proto-v47 --test login_packets`
Expected: FAIL, "unresolved import `oxide_proto_v47::clientbound`".

- [ ] **Step 3: Implement the error type and the login packets**

`crates/oxide-proto-v47/src/lib.rs`:

```rust
//! All 74 clientbound and 26 serverbound play packets, plus handshake, status, and login states.

pub mod clientbound;
pub mod handshake;
pub mod serverbound;
pub mod status;

use oxide_proto::codec::CodecError;

/// The protocol version these codecs speak.
pub const PROTOCOL: i32 = 47;

/// The handshake `next_state` that begins a login.
pub const NEXT_STATE_LOGIN: i32 = 2;

/// Errors from decoding a packet payload.
#[derive(Debug, thiserror::Error)]
pub enum PacketError {
    /// A field was malformed or the payload ended early.
    #[error("malformed packet: {0}")]
    Codec(#[from] CodecError),
    /// The payload carried bytes the field list does not account for.
    #[error("{0} trailing byte(s) after the last field")]
    Trailing(usize),
    /// A chunk column's declared size does not match its mask.
    #[error("column data is {got} bytes, the mask and flags imply {expected}")]
    BadColumnSize {
        /// The size the stream declared.
        got: usize,
        /// The size the mask, sky flag and biome flag imply.
        expected: usize,
    },
}
```

`crates/oxide-proto-v47/src/clientbound.rs`:

```rust
//! Clientbound packets: the login state now, the play state from Task 4 on.

use std::io::Cursor;

use oxide_proto::codec::{self, MAX_STRING_BYTES};
use oxide_proto::varint::{VarIntError, read_varint};

use crate::{PacketError, PROTOCOL};

/// Reads a packet id as the VarInt it is, returning it with the remaining bytes.
///
/// Every dispatcher goes through this, so an id is never assumed to be one byte.
pub fn read_packet_id(payload: &[u8]) -> Result<(i32, &[u8]), PacketError> {
    let mut cursor = Cursor::new(payload);
    let id = match read_varint(&mut cursor) {
        Ok(id) => id,
        Err(VarIntError::UnexpectedEof) => {
            return Err(PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the payload ended inside the packet id",
            ))));
        }
        Err(error) => return Err(PacketError::Codec(error.into())),
    };
    let consumed = cursor.position() as usize;
    Ok((id, &payload[consumed..]))
}

/// A packet received during the login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginPacket {
    /// The server refused the login; the reason is chat JSON.
    Disconnect {
        /// Chat JSON for the kick screen.
        reason: String,
    },
    /// The server asked for an encrypted session. M1 refuses this.
    EncryptionRequest {
        /// The server id (empty in 1.7 and later).
        server_id: String,
        /// The server's DER-encoded public key.
        public_key: Vec<u8>,
        /// The token that must come back RSA-encrypted.
        verify_token: Vec<u8>,
    },
    /// The login was accepted; the connection switches to the play state.
    LoginSuccess {
        /// The account UUID, hyphenated, as a string.
        uuid: String,
        /// The account name.
        username: String,
    },
    /// Compression is now enabled with this threshold.
    SetCompression {
        /// The threshold the server sent. A negative value disables compression.
        threshold: i32,
    },
}

/// Decodes a login-state packet whose id has already been read.
pub fn decode_login(body: &[u8]) -> Result<LoginPacket, PacketError> {
    // The id is consumed by `read_packet_id` before this is called; the tests
    // call `decode_login` with the id still in place, so strip it here.
    let (id, body) = read_packet_id(body)?;
    let mut cursor = Cursor::new(body);
    let packet = match id {
        0x00 => LoginPacket::Disconnect {
            reason: codec::read_string(&mut cursor, MAX_STRING_BYTES)?,
        },
        0x01 => {
            let server_id = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
            let key_len = read_varint(&mut cursor)?;
            let public_key = read_bytes(&mut cursor, key_len)?;
            let token_len = read_varint(&mut cursor)?;
            let verify_token = read_bytes(&mut cursor, token_len)?;
            LoginPacket::EncryptionRequest { server_id, public_key, verify_token }
        }
        0x02 => LoginPacket::LoginSuccess {
            uuid: codec::read_string(&mut cursor, 36)?,
            username: codec::read_string(&mut cursor, 16)?,
        },
        0x03 => LoginPacket::SetCompression { threshold: read_varint(&mut cursor)? },
        other => {
            return Err(PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown login packet id {other:#04x}"),
            ))));
        }
    };
    check_no_trailing(&cursor, body.len())?;
    Ok(packet)
}

/// Reads a length-prefixed byte array, refusing a negative length.
fn read_bytes(cursor: &mut Cursor<&[u8]>, len: i32) -> Result<Vec<u8>, PacketError> {
    if len < 0 {
        return Err(PacketError::Codec(codec::CodecError::NegativeLength(len)));
    }
    let len = len as usize;
    let start = cursor.position() as usize;
    let end = start + len;
    let bytes = body_slice(cursor, start, end)?;
    Ok(bytes.to_vec())
}

/// The bytes between two offsets of the cursor's own buffer.
fn body_slice(cursor: &Cursor<&[u8]>, start: usize, end: usize) -> Result<&[u8], PacketError> {
    cursor
        .get_ref()
        .get(start..end)
        .ok_or_else(|| {
            PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the payload ended inside a byte array",
            )))
        })
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
```

Note on `read_bytes`: after reading, the cursor must advance. Use `cursor.set_position(end as u64)` after a successful slice, so `check_no_trailing` sees the true consumption. The test for the Encryption Request pins this: `public_key` is read, then the token length, then the token, with no trailing bytes left.

`crates/oxide-proto-v47/src/serverbound.rs`:

```rust
//! Serverbound packets: the login state now, the play state from Task 4 on.

use std::io::{self, Write};

use oxide_proto::codec::write_string;

/// Serverbound Login Start (id 0x00).
pub const LOGIN_START_ID: i32 = 0x00;

/// Writes Login Start: the packet id, then the name (16 characters at most).
pub fn write_login_start(mut out: impl Write, username: &str) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, LOGIN_START_ID)?;
    write_string(&mut out, username)
}
```

The handshake module keeps its own `write_handshake`; note in the commit message that `handshake::write_handshake(47, host, port, NEXT_STATE_LOGIN)` is the login entry point (`NEXT_STATE_LOGIN` is 2).

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oxide-proto-v47`
Expected: the seven new login tests pass alongside the M0 suites.

- [ ] **Step 5: Commit**

```bash
git add crates/oxide-proto-v47/src/lib.rs crates/oxide-proto-v47/src/clientbound.rs crates/oxide-proto-v47/src/serverbound.rs crates/oxide-proto-v47/tests/login_packets.rs
git commit -m "feat: add the login-state packets and the packet-id reader (M1)"
```

---

### Task 4: The play-state packets and the connection obligations

**Files:**
- Modify: `crates/oxide-proto-v47/src/clientbound.rs` (append the play packets)
- Modify: `crates/oxide-proto-v47/src/serverbound.rs` (append the play packets)
- Test: `crates/oxide-proto-v47/tests/play_packets.rs`

**Interfaces:**
- Consumes: Task 3's `read_packet_id`, `PacketError`, `codec`.
- Produces, in `clientbound`: `KeepAlive { id: i32 }` (`PLAY_KEEP_ALIVE_ID = 0x00`), `JoinGame { entity_id: i32, gamemode: u8, dimension: i8, difficulty: u8, max_players: u8, level_type: String, reduced_debug_info: bool }` (0x01), `PlayerPositionAndLook { x: f64, y: f64, z: f64, yaw: f32, pitch: f32, flags: u8 }` (0x08), `PlayerListItem { entries: Vec<PlayerListEntry> }` (0x38, add-action only in M1), `PlayDisconnect { reason: String }` (0x40), `PluginMessage { channel: String, data: Vec<u8> }` (0x3F), each with `pub const ID: i32` and `pub fn decode(body: &[u8]) -> Result<Self, PacketError>` taking the body *after* the id.
- Produces, in `serverbound`: `write_keep_alive(out, id)`, `write_player_position_and_look(out, x, y, z, yaw, pitch, on_ground)`, `write_client_settings(out, &ClientSettings)`, `write_client_status(out, action)`, `write_plugin_message(out, channel, data)`, and:
  - `pub struct ClientSettings { pub locale: String, pub view_distance: u8, pub chat_mode: u8, pub chat_colors: bool, pub skin_parts: u8 }` with `impl Default` giving `en_US`, 8, 0, true, `0x7F`.
  - `pub enum ClientStatusAction { Respawn = 0, RequestStats = 1, OpenInventory = 2 }`.
  - `pub const KEEP_ALIVE_ID: i32 = 0x00; pub const PLAYER_POSITION_AND_LOOK_ID: i32 = 0x06; pub const CLIENT_SETTINGS_ID: i32 = 0x15; pub const CLIENT_STATUS_ID: i32 = 0x16; pub const PLUGIN_MESSAGE_ID: i32 = 0x17;`
  - `pub fn client_settings_payload(settings: &ClientSettings) -> Vec<u8>` and a matching `player_position_and_look_payload(...) -> Vec<u8>`, so the session can send exact bytes and tests can assert them.

Field layouts come from `docs/research/protocol-47-reference.md` §2.1 (clientbound) and §2.2 (serverbound); the two fixture lines in §8 are the shape to test against.

- [ ] **Step 1: Write the failing play-packet tests**

`crates/oxide-proto-v47/tests/play_packets.rs` — include at least these assertions, in this style:

```rust
//! Golden-byte tests for the play-state packets M1 needs.

use std::io::Cursor;

use oxide_proto_v47::clientbound::{self, JoinGame, KeepAlive, PlayDisconnect, PlayerListItem, PlayerPositionAndLook};
use oxide_proto_v47::serverbound::{ClientSettings, ClientStatusAction, client_settings_payload, write_client_settings, write_client_status, write_keep_alive, write_player_position_and_look};

#[test]
fn a_serverbound_keep_alive_echoes_the_id() {
    // The worked layout: Length 02, id 00, id 01.
    let mut out = Vec::new();
    write_keep_alive(&mut out, 1).expect("write");
    assert_eq!(out, [0x00, 0x01]);
}

#[test]
fn join_game_decodes_all_seven_fields() {
    let mut body = vec![0x01];
    body.extend_from_slice(&20i32.to_be_bytes());     // entity id
    body.push(0);                                     // gamemode: survival
    body.push(0);                                     // dimension: overworld
    body.push(1);                                     // difficulty: easy
    body.push(20);                                    // max players
    body.push(7);                                     // level type length
    body.extend_from_slice(b"default");
    body.push(0);                                     // reduced debug info
    match JoinGame::decode(&body[1..]).expect("decode") {
        JoinGame { entity_id, gamemode, dimension, difficulty, max_players, level_type, reduced_debug_info } => {
            assert_eq!((entity_id, gamemode, dimension), (20, 0, 0));
            assert_eq!((difficulty, max_players), (1, 20));
            assert_eq!(level_type, "default");
            assert!(!reduced_debug_info);
        }
    }
}

#[test]
fn player_position_and_look_reads_flags_for_relative_axes() {
    let mut body = vec![0x08];
    body.extend_from_slice(&8.5f64.to_be_bytes());
    body.extend_from_slice(&65.0f64.to_be_bytes());
    body.extend_from_slice(&(-12.25f64).to_be_bytes());
    body.extend_from_slice(&90.0f32.to_be_bytes());
    body.extend_from_slice(&(-30.0f32).to_be_bytes());
    body.push(0x01); // X is relative
    match PlayerPositionAndLook::decode(&body[1..]).expect("decode") {
        PlayerPositionAndLook { x, y, z, yaw, pitch, flags } => {
            assert_eq!((x, y, z), (8.5, 65.0, -12.25));
            assert_eq!((yaw, pitch), (90.0, -30.0));
            assert_eq!(flags, 0x01);
        }
    }
}

#[test]
fn the_position_echo_is_the_same_absolute_value() {
    let mut out = Vec::new();
    write_player_position_and_look(&mut out, 8.5, 65.0, -12.25, 90.0, -30.0, true).expect("write");
    assert_eq!(out[0], 0x06);
    let mut cursor = Cursor::new(&out[1..]);
    assert_eq!(f64::from_be_bytes(cursor_read8(&mut cursor)), 8.5);
    // on_ground is the last byte
    assert_eq!(*out.last().unwrap(), 1);
}

#[test]
fn client_settings_matches_the_specified_defaults() {
    let settings = ClientSettings::default();
    let payload = client_settings_payload(&settings);
    assert_eq!(payload[0], 0x15);
    assert_eq!(&payload[1..7], b"\x05en_US"); // locale, five bytes
    assert_eq!(payload[7], 8);                // view distance
    assert_eq!(payload[8], 0);                // chat mode: enabled
    assert_eq!(payload[9], 1);                // chat colours
    assert_eq!(payload[10], 0x7F);            // skin parts
    assert_eq!(payload.len(), 11);
}

#[test]
fn client_status_actions_carry_their_ids() {
    let mut out = Vec::new();
    write_client_status(&mut out, ClientStatusAction::Respawn).expect("write");
    assert_eq!(out, [0x16, 0x00]);
    let mut out = Vec::new();
    write_client_status(&mut out, ClientStatusAction::RequestStats).expect("write");
    assert_eq!(out, [0x16, 0x01]);
}

#[test]
fn a_player_list_add_entry_decodes_name_and_uuid() {
    let mut body = vec![0x38];
    oxide_proto::varint::write_varint(&mut body, 0).expect("action add");
    oxide_proto::varint::write_varint(&mut body, 1).expect("one entry");
    body.extend_from_slice(&[0xab; 16]);
    body.push(8);
    body.extend_from_slice(b"OxideDev");
    oxide_proto::varint::write_varint(&mut body, 0).expect("no properties");
    oxide_proto::varint::write_varint(&mut body, 0).expect("gamemode");
    oxide_proto::varint::write_varint(&mut body, 0).expect("ping");
    body.push(0); // no display name
    match PlayerListItem::decode(&body[1..]).expect("decode") {
        PlayerListItem { entries } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].uuid, [0xab; 16]);
            assert_eq!(entries[0].name.as_deref(), Some("OxideDev"));
            assert_eq!(entries[0].gamemode, Some(0));
            assert_eq!(entries[0].ping, Some(0));
        }
    }
}

#[test]
fn a_play_disconnect_carries_its_reason() {
    let json = r#"{"text":"Server closed"}"#;
    let mut body = vec![0x40, json.len() as u8];
    body.extend_from_slice(json.as_bytes());
    match PlayDisconnect::decode(&body[1..]).expect("decode") {
        PlayDisconnect { reason } => assert_eq!(reason, json),
    }
}

#[test]
fn the_packet_id_reader_returns_the_id_and_the_remaining_body() {
    let (id, rest) = clientbound::read_packet_id(&[0x7f, 0xaa, 0xbb]).expect("id");
    assert_eq!(id, 0x7f);
    assert_eq!(rest, &[0xaa, 0xbb]);
}

#[test]
fn a_multi_byte_packet_id_is_read_as_a_varint_not_a_byte() {
    // No M1 packet id is above 0x7f, but the reader must not assume that.
    let (id, rest) = clientbound::read_packet_id(&[0x80, 0x01, 0x00]).expect("id");
    assert_eq!(id, 128);
    assert_eq!(rest, &[0x00]);
}
```

Add a helper `fn cursor_read8(cursor: &mut Cursor<&[u8]>) -> [u8; 8]` in the test file.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-proto-v47 --test play_packets`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the clientbound play packets**

Append to `crates/oxide-proto-v47/src/clientbound.rs`:

```rust
/// Clientbound Keep Alive (play id 0x00).
pub const PLAY_KEEP_ALIVE_ID: i32 = 0x00;

/// A keepalive the client must echo back with the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAlive {
    /// The id to echo.
    pub id: i32,
}

impl KeepAlive {
    /// The packet id.
    pub const ID: i32 = PLAY_KEEP_ALIVE_ID;

    /// Decodes the fields after the packet id.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let id = read_varint(&mut cursor)?;
        Ok(Self { id })
    }
}

/// Clientbound Join Game (play id 0x01).
#[derive(Debug, Clone, PartialEq)]
pub struct JoinGame {
    /// The player's entity id.
    pub entity_id: i32,
    /// Gamemode; the 0x08 bit means hardcore.
    pub gamemode: u8,
    /// Dimension: -1 nether, 0 overworld, 1 end.
    pub dimension: i8,
    /// Difficulty.
    pub difficulty: u8,
    /// Maximum player count the server advertises.
    pub max_players: u8,
    /// Level type, for example `default`.
    pub level_type: String,
    /// Whether the server asks for reduced debug info.
    pub reduced_debug_info: bool,
}
```

with `impl JoinGame { pub const ID: i32 = 0x01; pub fn decode(body: &[u8]) -> Result<Self, PacketError> { ... } }` reading the fields in order (Int, UByte, Byte, UByte, UByte, String(16), Bool) and ending with the trailing-bytes check. Add `PlayerPositionAndLook` (Double, Double, Double, Float, Float, UByte; plus `pub const ABSOLUTE: u8 = 0x00;` and flag constants `FLAG_X`, `FLAG_Y`, `FLAG_Z`, `FLAG_YAW`, `FLAG_PITCH` = 0x01, 0x02, 0x04, 0x08, 0x10), `PlayDisconnect` (Chat String), `PluginMessage` (String channel, rest of the payload as data), and:

```rust
/// One entry of a Player List Item add block.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerListEntry {
    /// The player's UUID.
    pub uuid: [u8; 16],
    /// The name, present for the add action.
    pub name: Option<String>,
    /// The gamemode, present for the add action.
    pub gamemode: Option<i32>,
    /// The ping, present for the add action.
    pub ping: Option<i32>,
    /// The display name, when the entry carries one.
    pub display_name: Option<String>,
}

/// Clientbound Player List Item (play id 0x38), add action.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerListItem {
    /// The entries in this packet.
    pub entries: Vec<PlayerListEntry>,
}

impl PlayerListItem {
    /// The packet id.
    pub const ID: i32 = 0x38;
    /// The add action.
    pub const ACTION_ADD: i32 = 0;

    /// Decodes an add-action packet. Any other action is refused: M1 has no use
    /// for them, and a silent partial read would desynchronise the stream.
    pub fn decode(body: &[u8]) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let action = read_varint(&mut cursor)?;
        if action != Self::ACTION_ADD {
            return Err(PacketError::Codec(codec::CodecError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported Player List Item action {action}"),
            ))));
        }
        let count = read_varint(&mut cursor)?;
        if count < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(count)));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let uuid = codec::read_uuid(&mut cursor)?;
            let name = codec::read_string(&mut cursor, 16)?;
            let properties = read_varint(&mut cursor)?;
            for _ in 0..properties.max(0) {
                let _name = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
                let _value = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
                let is_signed = codec::read_bool(&mut cursor)?;
                if is_signed {
                    let _signature = codec::read_string(&mut cursor, MAX_STRING_BYTES)?;
                }
            }
            let gamemode = read_varint(&mut cursor)?;
            let ping = read_varint(&mut cursor)?;
            let has_display_name = codec::read_bool(&mut cursor)?;
            let display_name = if has_display_name {
                Some(codec::read_string(&mut cursor, MAX_STRING_BYTES)?)
            } else {
                None
            };
            entries.push(PlayerListEntry {
                uuid,
                name: Some(name),
                gamemode: Some(gamemode),
                ping: Some(ping),
                display_name,
            });
        }
        check_no_trailing(&cursor, body.len())?;
        Ok(Self { entries })
    }
}
```

The `properties` count is an Int-safe VarInt: clamp negative to zero with `max(0)` as written, and note in the code comment why. Whatever the reading order, every field of the add action must be consumed, or the trailing check fires — that check is what makes this codec safe against the next packet arriving mid-payload.

- [ ] **Step 4: Implement the serverbound play packets**

Append to `crates/oxide-proto-v47/src/serverbound.rs`:

```rust
/// Serverbound Keep Alive (play id 0x00).
pub const KEEP_ALIVE_ID: i32 = 0x00;

/// Serverbound Player Position And Look (play id 0x06): the reply to clientbound 0x08.
pub const PLAYER_POSITION_AND_LOOK_ID: i32 = 0x06;

/// Serverbound Client Settings (play id 0x15).
pub const CLIENT_SETTINGS_ID: i32 = 0x15;

/// Serverbound Client Status (play id 0x16).
pub const CLIENT_STATUS_ID: i32 = 0x16;

/// Serverbound Plugin Message (play id 0x17).
pub const PLUGIN_MESSAGE_ID: i32 = 0x17;

/// The client settings the connection uses until a settings screen exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettings {
    /// Locale, at most 7 characters, for example `en_US`.
    pub locale: String,
    /// View distance in chunks.
    pub view_distance: u8,
    /// Chat mode: 0 enabled, 1 commands only, 2 hidden.
    pub chat_mode: u8,
    /// Whether chat keeps its colours.
    pub chat_colors: bool,
    /// Displayed skin parts, a bitmask.
    pub skin_parts: u8,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            locale: "en_US".to_string(),
            view_distance: 8,
            chat_mode: 0,
            chat_colors: true,
            skin_parts: 0x7F,
        }
    }
}

/// The actions Client Status can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientStatusAction {
    /// Perform a respawn.
    Respawn = 0,
    /// Ask for statistics.
    RequestStats = 1,
    /// The open-inventory achievement.
    OpenInventory = 2,
}

/// Writes the packet id and a VarInt.
fn write_id_and_varint(out: &mut impl Write, id: i32, value: i32) -> io::Result<()> {
    oxide_proto::varint::write_varint(out, id)?;
    oxide_proto::varint::write_varint(out, value)
}
```

with the concrete writers:

```rust
/// Writes Keep Alive with `id`.
pub fn write_keep_alive(mut out: impl Write, id: i32) -> io::Result<()> {
    write_id_and_varint(&mut out, KEEP_ALIVE_ID, id)
}

/// Writes Player Position And Look.
pub fn write_player_position_and_look(
    mut out: impl Write,
    x: f64, y: f64, z: f64, yaw: f32, pitch: f32, on_ground: bool,
) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLAYER_POSITION_AND_LOOK_ID)?;
    out.write_all(&x.to_be_bytes())?;
    out.write_all(&y.to_be_bytes())?;
    out.write_all(&z.to_be_bytes())?;
    out.write_all(&yaw.to_be_bytes())?;
    out.write_all(&pitch.to_be_bytes())?;
    out.write_all(&[u8::from(on_ground)])
}

/// Writes Client Settings.
pub fn write_client_settings(mut out: impl Write, settings: &ClientSettings) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, CLIENT_SETTINGS_ID)?;
    write_string(&mut out, &settings.locale)?;
    out.write_all(&[settings.view_distance, settings.chat_mode, u8::from(settings.chat_colors), settings.skin_parts])
}

/// Writes Client Status with `action`.
pub fn write_client_status(mut out: impl Write, action: ClientStatusAction) -> io::Result<()> {
    write_id_and_varint(&mut out, CLIENT_STATUS_ID, action as i32)
}

/// Writes Plugin Message on `channel`.
pub fn write_plugin_message(mut out: impl Write, channel: &str, data: &[u8]) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, PLUGIN_MESSAGE_ID)?;
    write_string(&mut out, channel)?;
    out.write_all(data)
}

/// The Client Settings payload as bytes, for byte-exact assertions.
pub fn client_settings_payload(settings: &ClientSettings) -> Vec<u8> {
    let mut out = Vec::new();
    write_client_settings(&mut out, settings).expect("writing to a Vec cannot fail");
    out
}

/// The position-echo payload as bytes, for byte-exact assertions.
pub fn player_position_and_look_payload(x: f64, y: f64, z: f64, yaw: f32, pitch: f32, on_ground: bool) -> Vec<u8> {
    let mut out = Vec::new();
    write_player_position_and_look(&mut out, x, y, z, yaw, pitch, on_ground).expect("writing to a Vec cannot fail");
    out
}
```

Also cap the locale at 7 characters by refusing a longer one (spec §8: Locale ≤ 7 chars): make `write_client_settings` return `io::ErrorKind::InvalidInput` for a locale longer than 7 bytes and cover it with a test.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p oxide-proto-v47`
Expected: all play-packet tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-proto-v47/src/clientbound.rs crates/oxide-proto-v47/src/serverbound.rs crates/oxide-proto-v47/tests/play_packets.rs
git commit -m "feat: add the play-state packets for the connection obligations (M1)"
```

---

### Task 5: The column decoder (Chunk Data `0x21` and Map Chunk Bulk `0x26`)

**Files:**
- Create: `crates/oxide-proto-v47/src/column.rs`
- Modify: `crates/oxide-proto-v47/src/lib.rs` (add `pub mod column;`)
- Modify: `crates/oxide-proto-v47/src/clientbound.rs` (append `ChunkData` and `MapChunkBulk`)
- Test: `crates/oxide-proto-v47/tests/column.rs`
- Test: `crates/oxide-proto-v47/tests/column_capture.rs` (the committed fixture)

**Interfaces:**
- Produces: `oxide_proto_v47::column::{ColumnData, SectionData, block_index, column_size, unpack_nibble, unpack_block}` and `clientbound::{ChunkData, MapChunkBulk, BulkColumn}`.

```rust
/// One section of a column, exactly as the wire delivers it.
pub struct SectionData {
    /// 4096 blocks, `(id << 4) | meta`, little-endian on the wire.
    pub blocks: Box<[u16; 4096]>,
    /// 2048 nibbles of block light.
    pub block_light: Box<[u8; 2048]>,
    /// 2048 nibbles of sky light, absent for dimensions without sky.
    pub sky_light: Option<Box<[u8; 2048]>>,
}

/// A decoded chunk column: the sections the packet carried, by section index.
pub struct ColumnData {
    /// The primary bitmask the packet declared.
    pub mask: u16,
    /// Sections indexed by `y >> 4`; `None` for sections outside the mask.
    pub sections: [Option<SectionData>; 16],
    /// The biome array, present when the packet carried one.
    pub biomes: Option<[u8; 256]>,
}
```

- [ ] **Step 1: Write the failing decoder tests**

`crates/oxide-proto-v47/tests/column.rs`:

```rust
//! Tests for the column decoder: the size formula, nibble order, index order,
//! both packet shapes, and the hostile paths.

use oxide_proto_v47::clientbound::{ChunkData, MapChunkBulk};
use oxide_proto_v47::column::{block_index, column_size, parse_column, unpack_nibble};

/// Builds one section's payload: blocks first, then block light, then sky light.
fn build_section(block: u16, at: (usize, usize, usize)) -> Vec<u8> {
    let mut out = vec![0u8; 8192];
    let index = block_index(at.0, at.1, at.2);
    out[index * 2..index * 2 + 2].copy_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&[0u8; 2048]);        // block light
    out.extend_from_slice(&[0xFFu8; 2048]);     // sky light: every nibble 15
    out
}

#[test]
fn the_size_formula_matches_the_spec_fixture() {
    // Mask 0x0001, overworld, ground-up: 12,544 bytes.
    assert_eq!(column_size(0x0001, true, true), 12_544);
    // Nether: no sky light array.
    assert_eq!(column_size(0x0001, false, true), 10_496);
    // A section update carries no biomes.
    assert_eq!(column_size(0x0001, true, false), 12_288);
    // The unload shape.
    assert_eq!(column_size(0x0000, true, true), 256);
}

#[test]
fn even_indices_are_the_low_nibble() {
    let mut array = [0u8; 2048];
    array[0] = 0xF3; // index 0 low = 3, index 1 high = 15
    assert_eq!(unpack_nibble(&array, 0), 3);
    assert_eq!(unpack_nibble(&array, 1), 15);
}

#[test]
fn the_index_order_is_x_then_z_then_y() {
    // (y << 8) | (z << 4) | x, x varying fastest.
    assert_eq!(block_index(0, 0, 0), 0);
    assert_eq!(block_index(15, 0, 0), 15);
    assert_eq!(block_index(0, 0, 1), 16);
    assert_eq!(block_index(0, 1, 0), 256);
    assert_eq!(block_index(15, 15, 15), 4095);
}

#[test]
fn a_single_section_decodes_into_the_right_slot() {
    let mut data = build_section(0x0017, (1, 2, 3)); // id 1, meta 7, at (1,2,3)
    data.extend_from_slice(&[0u8; 256]); // biomes
    let column = parse_column(&data, 0x0001, true, true).expect("decode");
    let section = column.sections[0].as_ref().expect("section 0 present");
    let value = section.blocks[block_index(1, 2, 3)];
    assert_eq!(value, 0x0017);
    assert_eq!(value >> 4, 1);
    assert_eq!(value & 0x0F, 7);
    assert_eq!(section.sky_light.as_ref().expect("sky")[0], 0xFF);
    assert!(column.sections[1].is_none(), "sections outside the mask stay absent");
    assert!(column.biomes.is_some());
}

#[test]
fn sections_are_assigned_by_bit_index() {
    let mut data = Vec::new();
    data.extend_from_slice(&build_section(0x0001, (0, 0, 0)));
    data.extend_from_slice(&build_section(0x0002, (0, 0, 0)));
    data.extend_from_slice(&[3u8; 256]);
    let column = parse_column(&data, 0x0003, true, true).expect("decode");
    assert!(column.sections[0].is_some());
    assert!(column.sections[1].is_some());
    assert_eq!(column.sections[0].as_ref().unwrap().blocks[0], 0x0001);
    assert_eq!(column.sections[1].as_ref().unwrap().blocks[0], 0x0002);
}

#[test]
fn a_nether_column_has_no_sky_light() {
    let mut data = build_section(0x0001, (0, 0, 0));
    data.truncate(8192 + 2048); // drop the sky-light half
    data.extend_from_slice(&[0u8; 256]);
    let column = parse_column(&data, 0x0001, false, true).expect("decode");
    assert!(column.sections[0].as_ref().unwrap().sky_light.is_none());
}

#[test]
fn a_size_that_does_not_match_the_mask_is_refused() {
    let data = vec![0u8; 100];
    let error = parse_column(&data, 0x0001, true, true).expect_err("100 bytes cannot be a column");
    let _ = error;
}

#[test]
fn a_truncated_column_is_refused() {
    let mut data = build_section(0x0001, (0, 0, 0));
    data.truncate(4000);
    assert!(parse_column(&data, 0x0001, true, true).is_err());
}

#[test]
fn chunk_data_decodes_the_unload_shape() {
    // Ground-up, mask 0, size 0: "this column is empty".
    let mut body = vec![0x21];
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes());
    body.push(1);                     // ground-up
    body.extend_from_slice(&0u16.to_be_bytes());
    oxide_proto::varint::write_varint(&mut body, 0).expect("size");
    let decoded = ChunkData::decode(&body[1..]).expect("decode");
    assert_eq!(decoded.mask, 0);
    assert!(decoded.ground_up);
    assert_eq!(decoded.column.sections.iter().filter(|s| s.is_some()).count(), 0);
}

#[test]
fn map_chunk_bulk_reads_each_column_at_its_computed_offset() {
    // Two overworld columns, masks 0x000F and 0x0001, biomes always present.
    let mut body = vec![0x26, 1]; // sky-light sent, 2 columns
    oxide_proto::varint::write_varint(&mut body, 2).expect("count");
    for (cx, mask) in [(0i32, 0x000Fu16), (1, 0x0001)] {
        body.extend_from_slice(&cx.to_be_bytes());
        body.extend_from_slice(&0i32.to_be_bytes());
        body.extend_from_slice(&mask.to_be_bytes());
    }
    let mut payload = Vec::new();
    for mask in [0x000Fu16, 0x0001] {
        for bitten in 0..mask.count_ones() {
            payload.extend_from_slice(&build_section((bitten as u16) + 1, (0, 0, 0)));
        }
        payload.extend_from_slice(&[5u8; 256]);
    }
    body.extend_from_slice(&payload);
    let decoded = MapChunkBulk::decode(&body[1..]).expect("decode");
    assert!(decoded.sky_light);
    assert_eq!(decoded.columns.len(), 2);
    assert_eq!(decoded.columns[0].chunk_x, 0);
    assert_eq!(decoded.columns[0].mask, 0x000F);
    assert_eq!(decoded.columns[1].mask, 0x0001);
    // The second column starts where the first one ended: the first block there
    // is the section we wrote for it.
    assert_eq!(decoded.columns[1].column.sections[0].as_ref().unwrap().blocks[0], 1);
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-proto-v47 --test column`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the column decoder**

`crates/oxide-proto-v47/src/column.rs`, with the size formula, the nibble and index helpers, and the parser as specified in the Interfaces block. The parser must:

1. Compute `expected = column_size(mask, sky, biomes_present)` and refuse a payload whose length differs (`PacketError::BadColumnSize`).
2. Read blocks for each set bit in ascending section order, 4096 little-endian `u16` each.
3. Read block light for each set bit, 2048 bytes each.
4. Read sky light for each set bit when `sky`, 2048 bytes each.
5. Read 256 biome bytes when `biomes_present`.
6. Assign the arrays to `sections[bit]` in mask order, leaving every other slot `None`.
7. Never index past the payload: all offsets are checked against `data.len()` first, and a short payload returns `PacketError::BadColumnSize` rather than panicking.

Add `pub fn unpack_block(blocks: &[u16; 4096], x: usize, y: usize, z: usize) -> (u16, u8)` returning `(id, meta)` for convenience, and unit tests for it in the module.

- [ ] **Step 4: Implement the two packet wrappers**

Append to `crates/oxide-proto-v47/src/clientbound.rs`:

```rust
/// Clientbound Chunk Data (play id 0x21).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkData {
    /// The column's chunk x.
    pub chunk_x: i32,
    /// The column's chunk z.
    pub chunk_z: i32,
    /// Whether the packet replaces the whole column.
    pub ground_up: bool,
    /// The primary bitmask.
    pub mask: u16,
    /// The decoded column. For `ground_up` with an empty mask this carries no
    /// sections and is the unload shape.
    pub column: ColumnData,
}

impl ChunkData {
    /// The packet id.
    pub const ID: i32 = 0x21;

    /// Decodes the fields after the packet id, given the dimension's sky flag.
    ///
    /// The sky flag is a property of the world, not of the packet: the caller
    /// keeps it from Join Game or Respawn.
    pub fn decode(body: &[u8], sky_light_sent: bool) -> Result<Self, PacketError> {
        let mut cursor = Cursor::new(body);
        let chunk_x = codec::read_i32(&mut cursor)?;
        let chunk_z = codec::read_i32(&mut cursor)?;
        let ground_up = codec::read_bool(&mut cursor)?;
        let mask = codec::read_u16(&mut cursor)?;
        let size = read_varint(&mut cursor)?;
        if size < 0 {
            return Err(PacketError::Codec(codec::CodecError::NegativeLength(size)));
        }
        let size = size as usize;
        let start = cursor.position() as usize;
        let data = body.get(start..start + size).ok_or(PacketError::BadColumnSize {
            got: body.len().saturating_sub(start),
            expected: size,
        })?;
        cursor.set_position((start + size) as u64);
        check_no_trailing(&cursor, body.len())?;
        let column = if ground_up && mask == 0 {
            ColumnData::empty()
        } else {
            column::parse_column(data, mask, sky_light_sent, ground_up)?
        };
        Ok(Self { chunk_x, chunk_z, ground_up, mask, column })
    }
}
```

`ColumnData::empty()` returns `mask: 0`, all slots `None`, `biomes: None`. `MapChunkBulk` follows the same shape with `SkyLightSent Bool`, `ChunkColumnCount VarInt`, the metadata block for every column first, then the payload block in the same order, each column's payload sized by its own mask (biomes always present). Its decoded form is `pub struct BulkColumn { pub chunk_x: i32, pub chunk_z: i32, pub mask: u16, pub column: ColumnData }` and `MapChunkBulk { pub sky_light: bool, pub columns: Vec<BulkColumn> }` with `pub const ID: i32 = 0x26;`.

Should you chase the documented pitfall of `0x26`'s "no per-column length field": yes — the decoder must compute each column's size from its mask and the packet-level sky flag, and the test above pins that the second column is read at the right offset.

- [ ] **Step 5: Add the capture-fixture test**

`crates/oxide-proto-v47/tests/column_capture.rs`:

```rust
//! Decodes the real column payloads captured from the rig server (Task 2) and
//! asserts the invariants the capture's manifest records.

use serde_json::Value;

const MANIFEST: &str = include_str!("fixtures/m1-capture/manifest.json");

#[test]
fn the_manifest_parses_and_every_column_decodes() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("manifest parses");
    let columns = manifest["columns"].as_array().expect("columns array");
    assert!(!columns.is_empty(), "the capture produced at least one column");
    for column in columns {
        let file = column["file"].as_str().expect("file name");
        let bytes = std::fs::read(format!("tests/fixtures/m1-capture/{file}")).expect("fixture file");
        let mask = u16::from_str_radix(
            column["mask"].as_str().expect("mask").trim_start_matches("0x"),
            16,
        )
        .expect("mask parses");
        let sky = column["sky"].as_bool().expect("sky flag");
        let ground_up = column["ground_up"].as_bool().expect("ground-up flag");
        assert_eq!(bytes.len(), column["size"].as_u64().expect("size") as usize);
        let decoded = oxide_proto_v47::column::parse_column(&bytes, mask, sky, ground_up)
            .unwrap_or_else(|error| panic!("{file} does not decode: {error}"));
        assert_eq!(decoded.mask, mask);
        // A vanilla world has bedrock at the bottom of the spawn column.
        if let Some(section) = decoded.sections[0].as_ref() {
            let bedrock = column["block_counts"]["7"].as_u64().unwrap_or(0);
            if bedrock > 0 {
                assert_eq!(
                    section.blocks[0] >> 4,
                    7,
                    "{file}: the first block is bedrock in a generated world"
                );
            }
        }
    }
}

#[test]
fn the_manifest_records_the_capture_provenance() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("manifest parses");
    assert_eq!(manifest["server"]["version"], "1.8.9");
    assert_eq!(
        manifest["server"]["jar_sha1"],
        "b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd"
    );
    assert!(manifest["packets"]["0x21"].as_u64().is_some());
}
```

Add `serde_json` to `[dev-dependencies]` of `crates/oxide-proto-v47/Cargo.toml` (it is already a normal dependency, so `serde_json.workspace = true` under dev-dependencies is not needed — reuse the existing dependency entry; do not add a duplicate).

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p oxide-proto-v47`
Expected: the column tests and the capture tests pass. If the capture test fails on a real payload, the bug is in the decoder — the capture is ground truth; report the mismatch rather than adjusting the fixture.

- [ ] **Step 7: Commit**

```bash
git add crates/oxide-proto-v47/src/column.rs crates/oxide-proto-v47/src/lib.rs crates/oxide-proto-v47/src/clientbound.rs crates/oxide-proto-v47/tests/column.rs crates/oxide-proto-v47/tests/column_capture.rs crates/oxide-proto-v47/Cargo.toml
git commit -m "feat: add the chunk column decoder for 0x21 and 0x26 (M1)"
```

---

### Task 6: The world store

**Files:**
- Create: `crates/oxide-world/src/chunk.rs`
- Create: `crates/oxide-world/src/world.rs`
- Modify: `crates/oxide-world/src/lib.rs` (add `pub mod chunk;` and `pub mod world;`)
- Modify: `crates/oxide-world/Cargo.toml` (add `oxide-proto-v47` and `oxide-proto` as path dependencies)
- Test: `crates/oxide-world/tests/store.rs`

**Interfaces:**
- Consumes: `oxide_proto_v47::clientbound::{ChunkData, MapChunkBulk}` and `oxide_proto_v47::column::{ColumnData, SectionData, block_index}`.
- Produces:

```rust
/// Blocks per axis in a section.
pub const SECTION_SIZE: usize = 16;
/// Sections in a column: y = 0..256.
pub const SECTION_COUNT: usize = 16;

/// One 16x16x16 section of block ids and light.
pub struct Section { /* private: blocks, block_light, sky_light */ }

impl Section {
    /// A section of air: block light 0, sky light 15 when the dimension has sky.
    pub fn air(has_sky: bool) -> Self;
    /// The packed block value, `(id << 4) | meta`.
    pub fn block(&self, x: usize, y: usize, z: usize) -> u16;
    /// Sets the packed block value. Indices must be 0..16.
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, value: u16);
    /// The block-light nibble, 0..15.
    pub fn block_light(&self, x: usize, y: usize, z: usize) -> u8;
    /// The sky-light nibble, 0..15; 0 when the dimension has no sky.
    pub fn sky_light(&self, x: usize, y: usize, z: usize) -> u8;
}

/// A 16x16x256 column.
pub struct Chunk { /* private: x, z, has_sky, sections, biomes */ }

impl Chunk {
    /// An empty column for the given dimension.
    pub fn new(x: i32, z: i32, has_sky: bool) -> Self;
    /// Applies a decoded column.
    ///
    /// `ground_up` replaces the whole column: sections in the mask are replaced
    /// by the packet's data, every other section becomes air, and the biome
    /// array is replaced when the packet carried one. A section update replaces
    /// only the sections in the mask; every other section keeps its blocks and
    /// its light, and the biomes are left alone.
    pub fn apply(&mut self, data: &ColumnData, ground_up: bool, has_sky: bool);
    /// The packed block at world-local coordinates; y 0..256.
    pub fn block(&self, x: usize, y: usize, z: usize) -> u16;
    /// The block-light nibble at world-local coordinates; 0 for a section that
    /// is not present, and 0 outside 0..256.
    pub fn block_light_at(&self, x: usize, y: usize, z: usize) -> u8;
    /// The sky-light nibble at world-local coordinates; 15 for an absent section
    /// in a dimension with sky, 0 in one without.
    pub fn sky_light_at(&self, x: usize, y: usize, z: usize) -> u8;
    /// Whether the column holds no blocks at all.
    pub fn is_empty(&self) -> bool;
    /// The biome id at a column position.
    pub fn biome(&self, x: usize, z: usize) -> u8;
}

/// The client's chunk store.
pub struct World { /* private: has_sky, chunks */ }

impl World {
    /// A world for a dimension with or without sky light.
    pub fn new(has_sky: bool) -> Self;
    /// Whether the dimension has sky light.
    pub fn has_sky(&self) -> bool;
    /// Applies Chunk Data. The unload shape (ground-up, empty mask) removes the
    /// column and reports `false`; anything else reports `true`.
    pub fn apply_chunk_data(&mut self, packet: &ChunkData) -> bool;
    /// Applies Map Chunk Bulk; returns how many columns it carried.
    pub fn apply_bulk(&mut self, packet: &MapChunkBulk) -> usize;
    /// The column at those chunk coordinates, if it is loaded.
    pub fn chunk(&self, cx: i32, cz: i32) -> Option<&Chunk>;
    /// The packed block at world coordinates; air (0) when the column is not
    /// loaded or y is outside 0..256.
    pub fn block(&self, x: i32, y: i32, z: i32) -> u16;
    /// Removes a column, reporting whether one was present.
    pub fn unload(&mut self, cx: i32, cz: i32) -> bool;
    /// The loaded chunk coordinates, for tests and diagnostics.
    pub fn loaded(&self) -> Vec<(i32, i32)>;
}
```

- [ ] **Step 1: Write the failing store tests**

`crates/oxide-world/tests/store.rs` — cover:

```rust
//! Tests for the chunk store: ground-up replacement, section updates, the
//! unload shape, light defaults, and coordinate handling.

// 1. A ground-up column replaces everything: load a column with section 0 and
//    section 3 present, then load a ground-up column with only section 5 present;
//    sections 0 and 3 are now air, and their sky light reads 15.
// 2. A section update (ground_up false) replaces only the listed sections and
//    keeps everything else, including light.
// 3. Sections outside the mask start at block light 0 and sky light 15.
// 4. The unload shape removes the column and `block` returns air afterwards.
// 5. `block` outside the loaded area, above y=255, and below y=0 returns 0.
// 6. Packed values round-trip: id in the high 12 bits, meta in the low 4.
// 7. Nether columns (has_sky false): sky light reads 0 and a decoded column with
//    sky_light None does not panic.
// 8. A bulk applies every column it carries.
```

Write each as a real test with hand-built `ColumnData` values (construct `SectionData` directly — the type is public for exactly this reason), for example:

```rust
#[test]
fn a_ground_up_column_turns_unlisted_sections_into_air() {
    let mut world = World::new(true);
    let first = column_with_sections(&[(0, 1), (3, 1)]); // stone in sections 0 and 3
    world.apply_column(0, 0, &first, true);
    assert_eq!(world.block(0, 0, 0) >> 4, 1);

    let second = column_with_sections(&[(5, 2)]);
    world.apply_column(0, 0, &second, true);
    assert_eq!(world.block(0, 0, 0), 0, "section 0 is air now");
    assert_eq!(world.block(0, 16 * 5, 0) >> 4, 2, "section 5 carries the stone");
    let chunk = world.chunk(0, 0).expect("loaded");
    assert_eq!(chunk.sky_light_at(0, 0, 0), 15, "air keeps the sky-light default");
}
```

Provide `fn column_with_sections(entries: &[(usize, u16)]) -> ColumnData` in the test file. Because `World::apply_chunk_data` takes a packet type, add a small public helper on `World` — `pub fn apply_column(&mut self, cx: i32, cz: i32, data: &ColumnData, ground_up: bool) -> bool` — that `apply_chunk_data` and `apply_bulk` call, and use that in the tests. Keep `apply_chunk_data` as the packet-facing entry point so the session never hand-builds a `ChunkData`.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-world`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the section and the chunk**

`crates/oxide-world/src/chunk.rs`: the `Section` and `Chunk` types from the Interfaces block. Light nibble storage uses `crate::column`-equivalent helpers — call `oxide_proto_v47::column::{block_index, unpack_nibble}` rather than copying them, so the nibble convention lives in one place. Setting a light nibble is not needed in M1; the arrays are stored and read by index.

`Chunk::apply` implements the rules from the Interfaces block, with these exact details:

- `ground_up == true`: for every slot not in the mask, set it to `None` when the packet's mask says so... — but the *light defaults* must survive: keep the slot as `None` and let the accessors answer air/block-light-0/sky-light-15 for `None`. That is the simplest faithful representation, and `Section::air` exists for code that needs an owned section.
- `ground_up == false`: only masked slots are replaced; unmasked slots keep their existing `Section` (or stay `None`).
- Biomes: replaced when `ground_up` and `data.biomes.is_some()`; otherwise untouched.
- `Chunk::block(x, y, z)`: `sections[y / 16]` if present, else air; indices `x % 16`, `y % 16`, `z % 16`.

- [ ] **Step 4: Implement the world**

`crates/oxide-world/src/world.rs`: the `World` type, a `HashMap<(i32, i32), Chunk>` private field, and the coordinate maths: `x.div_euclid(16)` and `x.rem_euclid(16)` so negative coordinates land in the right chunk. `block` returns 0 for an unloaded column, for y outside 0..256, and for any chunk outside the loaded set.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p oxide-world`
Expected: all store tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-world/Cargo.toml crates/oxide-world/src/lib.rs crates/oxide-world/src/chunk.rs crates/oxide-world/src/world.rs crates/oxide-world/tests/store.rs
git commit -m "feat: add the chunk store and the wire-to-store rules (M1)"
```

Note the new dependency edges in the commit message: `oxide-world → oxide-proto-v47` (and transitively `oxide-proto`), both allowed by spec section 5.1 row 5.

---

### Task 7: Terrain data types and the camera

**Files:**
- Create: `crates/oxide-render/src/terrain.rs`
- Create: `crates/oxide-render/src/camera.rs`
- Modify: `crates/oxide-render/src/lib.rs` (add `pub mod camera;` and `pub mod terrain;`)
- Modify: `crates/oxide-render/Cargo.toml` (add `glam`)
- Test: `crates/oxide-render/tests/terrain_data.rs` (unit-level; no GPU)

**Interfaces:**
- Consumes: nothing from M1's earlier tasks; this task creates the types the mesher (Task 8) and the pipeline (Task 10) both use.
- Produces:

```rust
/// One terrain vertex: a position and a colour with the face brightness baked in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// World-space position.
    pub position: [f32; 3],
    /// Linear colour, already multiplied by the face brightness.
    pub color: [f32; 3],
}

/// The size of one vertex in the byte stream the GPU receives.
pub const VERTEX_BYTES: usize = 24;

/// Serialises vertices into the byte layout the vertex buffer uses.
pub fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8>;

/// A section's geometry, ready to be uploaded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChunkMesh {
    /// Vertices, in triangle order.
    pub vertices: Vec<Vertex>,
    /// Indices into `vertices`.
    pub indices: Vec<u32>,
}

impl ChunkMesh {
    /// Whether the mesh draws nothing.
    pub fn is_empty(&self) -> bool;
    /// The number of vertices.
    pub fn vertex_count(&self) -> usize;
}

/// Identifies one section's mesh: chunk x, chunk z, section index.
pub type SectionKey = (i32, i32, u8);
```

```rust
/// Where the camera is and where it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    /// Feet position, as the server reports it.
    pub position: [f64; 3],
    /// Yaw in degrees: 0 faces south (+Z), 90 faces west (-X), as vanilla.
    pub yaw: f32,
    /// Pitch in degrees: positive looks down, as vanilla.
    pub pitch: f32,
}

/// A perspective camera following a pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where the camera is.
    pub pose: CameraPose,
    /// Vertical field of view in degrees.
    pub fov_degrees: f32,
    /// The near plane.
    pub near: f32,
    /// The far plane in chunks; the plane itself is `far_chunks * 16 * √2`.
    pub far_chunks: f32,
}

/// The eye height above the feet position, as vanilla uses.
pub const EYE_HEIGHT: f32 = 1.62;
/// Vanilla's default vertical field of view.
pub const DEFAULT_FOV: f32 = 70.0;
/// Vanilla's near plane.
pub const NEAR_PLANE: f32 = 0.05;

impl Camera {
    /// The eye position: the pose's feet position plus [`EYE_HEIGHT`].
    pub fn eye(&self) -> glam::Vec3;
    /// The unit forward vector for the pose's yaw and pitch.
    pub fn forward(&self) -> glam::Vec3;
    /// The view matrix.
    pub fn view(&self) -> glam::Mat4;
    /// The projection matrix for an aspect ratio. The far plane is
    /// `far_chunks * 16 * √2`, matching the vanilla projection's depth range for
    /// the configured render distance (spec section 10).
    pub fn projection(&self, aspect: f32) -> glam::Mat4;
    /// The combined view-projection matrix.
    pub fn view_projection(&self, aspect: f32) -> glam::Mat4;
}
```

- [ ] **Step 1: Write the failing tests**

`crates/oxide-render/tests/terrain_data.rs`:

```rust
//! Tests for the vertex layout and the camera maths. No GPU is involved.

use oxide_render::camera::{Camera, CameraPose, DEFAULT_FOV, EYE_HEIGHT, NEAR_PLANE};
use oxide_render::terrain::{VERTEX_BYTES, Vertex, vertex_bytes};

#[test]
fn a_vertex_is_twenty_four_bytes_of_little_endian_floats() {
    let vertex = Vertex {
        position: [1.0, 2.0, 3.0],
        color: [0.5, 0.25, 0.0],
    };
    let bytes = vertex_bytes(&[vertex]);
    assert_eq!(bytes.len(), VERTEX_BYTES);
    let read = |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!((read(0), read(4), read(8)), (1.0, 2.0, 3.0));
    assert_eq!((read(12), read(16), read(20)), (0.5, 0.25, 0.0));
}

#[test]
fn two_vertices_are_twice_the_bytes() {
    let mesh = vec![
        Vertex { position: [0.0; 3], color: [0.0; 3] },
        Vertex { position: [1.0; 3], color: [1.0; 3] },
    ];
    assert_eq!(vertex_bytes(&mesh).len(), 2 * VERTEX_BYTES);
}

fn pose(yaw: f32, pitch: f32) -> CameraPose {
    CameraPose { position: [0.0, 64.0, 0.0], yaw, pitch }
}

#[test]
fn yaw_zero_faces_south_and_ninety_faces_west() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let forward = camera.forward();
    assert!((forward.z - 1.0).abs() < 1e-5, "yaw 0 is south: {forward:?}");
    assert!(forward.x.abs() < 1e-5);

    let camera = Camera { pose: pose(90.0, 0.0), ..camera };
    let forward = camera.forward();
    assert!((forward.x + 1.0).abs() < 1e-5, "yaw 90 is west: {forward:?}");
}

#[test]
fn positive_pitch_looks_down() {
    let camera = Camera {
        pose: pose(0.0, 90.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    assert!(camera.forward().y < -0.99, "pitch 90 looks down");
}

#[test]
fn the_eye_sits_one_and_a_six_above_the_feet() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    assert!((camera.eye().y - (64.0 + EYE_HEIGHT)).abs() < 1e-5);
}

#[test]
fn a_point_ahead_projects_to_the_centre_and_near_maps_to_zero_depth() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let view_projection = camera.view_projection(16.0 / 9.0);
    // Ten blocks ahead along +Z, at eye height.
    let ahead = view_projection * glam::Vec4::new(0.0, camera.eye().y as f32, 10.0, 1.0);
    let ndc = ahead.truncate() / ahead.w;
    assert!(ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4, "centre of view: {ndc:?}");
    assert!(ndc.z > 0.0 && ndc.z < 1.0, "inside the depth range: {ndc:?}");

    // Just beyond the near plane: depth is almost zero.
    let near_point = view_projection
        * glam::Vec4::new(0.0, camera.eye().y as f32, (NEAR_PLANE as f64 + 0.001) as f32, 1.0);
    let near_ndc = near_point.truncate() / near_point.w;
    assert!(near_ndc.z < 0.01, "near maps to zero: {near_ndc:?}");
}

#[test]
fn a_point_behind_the_camera_has_negative_w() {
    let camera = Camera {
        pose: pose(0.0, 0.0),
        fov_degrees: DEFAULT_FOV,
        near: NEAR_PLANE,
        far_chunks: 8.0,
    };
    let behind = camera.view_projection(1.0)
        * glam::Vec4::new(0.0, camera.eye().y as f32, -5.0, 1.0);
    assert!(behind.w < 0.0, "behind the camera clips: {behind:?}");
}
```

Add `glam` to `[dev-dependencies]` of `oxide-render`? No: `glam` is a normal dependency, so the test target already sees it.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-render --test terrain_data`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the terrain data types and the camera**

Implement both modules exactly as the Interfaces block specifies. Notes that matter:

- `vertex_bytes` writes each `f32` with `to_le_bytes`, position first then colour, in vertex order, so the layout matches the pipeline's `array_stride: 24` with attributes at offsets 0 and 12. Document that the test and the pipeline descriptor must change together.
- Use `glam::Mat4::perspective_rh` (the 0..1 depth range wgpu expects) and `glam::Mat4::look_to_rh` with `glam::Vec3::Y` as up. Do not use the `*_gl` variants: they produce OpenGL's -1..1 range and the depth test then fails.
- `forward()`: `-sin(yaw) * cos(pitch)`, `-sin(pitch)`, `cos(yaw) * cos(pitch)`, in degrees converted to radians.
- `projection()`: `Mat4::perspective_rh(fov.to_radians(), aspect.max(0.01), near, far_chunks * 16.0 * std::f32::consts::SQRT_2)`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oxide-render`
Expected: seven new tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/oxide-render/Cargo.toml crates/oxide-render/src/lib.rs crates/oxide-render/src/terrain.rs crates/oxide-render/src/camera.rs crates/oxide-render/tests/terrain_data.rs Cargo.lock
git commit -m "feat: add the terrain vertex types and the camera (M1). New dependency: glam (MIT/Apache-2.0)"
```

---

### Task 8: The block palette and the section mesher

**Files:**
- Create: `crates/oxide-game/src/palette.rs`
- Create: `crates/oxide-game/src/mesher.rs`
- Modify: `crates/oxide-game/src/lib.rs` (add `pub mod mesher;` and `pub mod palette;`)
- Modify: `crates/oxide-game/Cargo.toml` (add `oxide-world`, `oxide-render`, `oxide-proto-v47`)
- Test: `crates/oxide-game/tests/mesher.rs`

**Interfaces:**
- Consumes: `oxide_render::terrain::{ChunkMesh, Vertex}`; `oxide_world::world::World` and `oxide_world::chunk::{SECTION_SIZE, SECTION_COUNT}`; `oxide_proto_v47::column::{block_index, unpack_nibble}`.
- Produces:

```rust
// palette.rs

/// The six faces of a block, named as vanilla names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// +Y.
    Top,
    /// -Y.
    Bottom,
    /// -Z.
    North,
    /// +Z.
    South,
    /// -X.
    West,
    /// +X.
    East,
}

impl Face {
    /// Every face, in a fixed order.
    pub const ALL: [Face; 6];
    /// The outward unit normal.
    pub fn normal(self) -> [f32; 3];
    /// The neighbouring block this face looks at, as an offset.
    pub fn offset(self) -> (i32, i32, i32);
    /// Vanilla's brightness for the face: top 1.0, bottom 0.5, north and south
    /// 0.8, east and west 0.6.
    pub fn brightness(self) -> f32;
}

/// The colour for a block, before face brightness.
pub fn block_color(id: u16, meta: u8, face: Face) -> [f32; 3];

/// What an id with no palette entry renders as: unmistakable magenta.
pub const UNKNOWN_COLOR: [f32; 3] = [1.0, 0.0, 1.0];
```

```rust
// mesher.rs

/// Builds one section's mesh, or `None` when the section draws nothing.
pub fn build_section_mesh(world: &World, cx: i32, cz: i32, sy: usize) -> Option<ChunkMesh>;

/// Builds every section of a column: index `sy`, `None` for a section that
/// draws nothing, so the caller can remove a mesh that is now empty.
pub fn build_column_meshes(world: &World, cx: i32, cz: i32) -> Vec<(usize, Option<ChunkMesh>)>;
```

- [ ] **Step 1: Write the failing mesher tests**

`crates/oxide-game/tests/mesher.rs` — the tests build small worlds directly:

```rust
//! Tests for the mesher: face counts, culling across section and chunk borders,
//! winding, and the palette.

use oxide_game::mesher::{build_column_meshes, build_section_mesh};
use oxide_game::palette::{Face, UNKNOWN_COLOR, block_color};
use oxide_world::chunk::Section;
use oxide_world::world::World;

/// A world with one column whose section 0 carries `blocks`, each entry a
/// `(x, y, z, id << 4 | meta)` tuple.
fn world_with(blocks: &[(usize, usize, usize, u16)]) -> World {
    let mut world = World::new(true);
    let mut column = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    for (x, y, z, value) in blocks {
        section.set_block(*x, *y, *z, *value);
    }
    column.sections[0] = Some(oxide_world::chunk::data_from_section(&section));
    world.apply_column(0, 0, &column, true);
    world
}

#[test]
fn a_single_block_has_six_faces() {
    let world = world_with(&[(0, 0, 0, 0x0010)]); // stone at the section origin
    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    assert_eq!(mesh.vertices.len(), 24, "six faces of four vertices");
    assert_eq!(mesh.indices.len(), 36, "six faces of two triangles");
    assert_eq!(&mesh.indices[..6], &[0, 1, 2, 0, 2, 3]);
}

#[test]
fn two_stacked_blocks_share_no_face() {
    let world = world_with(&[(0, 0, 0, 0x0010), (0, 1, 0, 0x0010)]);
    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    // Twelve faces less the two that touch: ten.
    assert_eq!(mesh.vertices.len(), 10 * 4);
}

#[test]
fn a_neighbour_across_a_section_border_culls_the_face() {
    // Section 1, at the matching position, sees the section-0 block below.
    let mut column = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    section.set_block(0, 0, 0, 0x0010);
    column.sections[1] = Some(oxide_world::chunk::data_from_section(&section));
    let mut world2 = world_with(&[(0, 15, 0, 0x0010)]);
    world2.apply_column(0, 0, &column, false);
    let mesh = build_section_mesh(&world2, 0, 0, 1).expect("a mesh");
    assert_eq!(mesh.vertices.len(), 5 * 4, "the bottom face is culled");
}

#[test]
fn a_neighbour_across_a_chunk_border_culls_the_face() {
    let mut world = World::new(true);
    let mut left = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    section.set_block(15, 0, 0, 0x0010);
    left.sections[0] = Some(oxide_world::chunk::data_from_section(&section));
    world.apply_column(0, 0, &left, true);
    let mut right = oxide_proto_v47::column::ColumnData::empty();
    let mut section = Section::air(true);
    section.set_block(0, 0, 0, 0x0010);
    right.sections[0] = Some(oxide_world::chunk::data_from_section(&section));
    world.apply_column(1, 0, &right, true);

    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    assert_eq!(mesh.vertices.len(), 5 * 4, "the east face meets its neighbour");
}

#[test]
fn an_empty_section_builds_nothing() {
    let world = World::new(true);
    assert!(build_section_mesh(&world, 0, 0, 0).is_none());
}

#[test]
fn a_column_reports_every_section_slot() {
    let world = world_with(&[(0, 0, 0, 0x0010)]);
    let meshes = build_column_meshes(&world, 0, 0);
    assert_eq!(meshes.len(), 16);
    assert!(meshes[0].1.is_some(), "section 0 draws");
    assert!(meshes[1].1.is_none(), "section 1 is empty");
}

#[test]
fn every_face_winds_counter_clockwise_from_outside() {
    for face in Face::ALL {
        // The corner table lives in the mesher; a face whose first triangle
        // winds the other way would be culled by the pipeline.
        let corners = oxide_game::mesher::face_corners(face);
        let [a, b, c, _] = corners;
        let first = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let second = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            first[1] * second[2] - first[2] * second[1],
            first[2] * second[0] - first[0] * second[2],
            first[0] * second[1] - first[1] * second[0],
        ];
        let normal = face.normal();
        let dot = cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2];
        assert!(dot > 0.5, "{face:?} winds the wrong way: {cross:?}");
    }
}

#[test]
fn the_palette_colours_the_common_blocks() {
    let grass_top = block_color(2, 0, Face::Top);
    assert!(grass_top[1] > grass_top[0] && grass_top[1] > grass_top[2], "grass top is green");
    let grass_side = block_color(2, 0, Face::North);
    assert!(grass_side[0] > grass_side[2], "the side is earthy");
    let stone = block_color(1, 0, Face::Top);
    assert!((stone[0] - stone[1]).abs() < 0.01 && (stone[1] - stone[2]).abs() < 0.01, "stone is grey");
    assert_eq!(block_color(19_999, 0, Face::Top), UNKNOWN_COLOR, "an unknown id is loud");
}

#[test]
fn brightness_is_baked_into_the_vertex_colour() {
    let world = world_with(&[(0, 0, 0, 0x0010)]);
    let mesh = build_section_mesh(&world, 0, 0, 0).expect("a mesh");
    let base = block_color(1, 0, Face::Top);
    let top_vertex = mesh
        .vertices
        .iter()
        .find(|vertex| vertex.position[1] == 1.0)
        .expect("a top vertex");
    assert!(
        (top_vertex.color[0] - base[0]).abs() < 1e-5,
        "top brightness is 1.0"
    );
    let bottom_vertex = mesh
        .vertices
        .iter()
        .find(|vertex| vertex.position[1] == 0.0)
        .expect("a bottom vertex");
    assert!(
        (bottom_vertex.color[0] - base[0] * 0.5).abs() < 1e-5,
        "bottom brightness is 0.5"
    );
}
```

`SectionData` for each test world. The conversions live in `oxide-world` (the protocol crate cannot name `oxide_world::chunk::Section`): add `pub fn section_from_data(data: &SectionData, has_sky: bool) -> Section` and `pub fn data_from_section(section: &Section) -> oxide_proto_v47::column::SectionData` to `crates/oxide-world/src/chunk.rs`, with the exact names the tests use, and document both as the wire-to-store bridge for tests and for any later code that needs them.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-game --test mesher`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the palette**

`crates/oxide-game/src/palette.rs`: the `Face` enum with `ALL = [Top, Bottom, North, South, East, West]`, `normal()` (`[0,1,0]`, `[0,-1,0]`, `[0,0,-1]`, `[0,0,1]`, `[-1,0,0]`, `[1,0,0]`), `offset()` (`(0,1,0)`, `(0,-1,0)`, `(0,0,-1)`, `(0,0,1)`, `(-1,0,0)`, `(1,0,0)`), `brightness()` (1.0, 0.5, 0.8, 0.8, 0.6, 0.6). Then `block_color(id, meta, face)` as a `match id` table. Include at least these entries (id: face-specific or uniform colour):

| id | Block | Colour |
| --- | --- | --- |
| 1 | stone | 0.50, 0.50, 0.50 |
| 2 | grass block | top 0.35, 0.61, 0.26; sides 0.48, 0.36, 0.25; bottom dirt |
| 3 | dirt | 0.48, 0.36, 0.25 |
| 4 | cobblestone | 0.44, 0.44, 0.44 |
| 5 | planks | 0.65, 0.53, 0.34 |
| 7 | bedrock | 0.30, 0.30, 0.30 |
| 8, 9 | water | 0.25, 0.40, 0.85 |
| 10, 11 | lava | 0.95, 0.51, 0.12 |
| 12 | sand | 0.86, 0.81, 0.59 |
| 13 | gravel | 0.53, 0.51, 0.50 |
| 14 | gold ore | 0.62, 0.58, 0.42 |
| 15 | iron ore | 0.62, 0.58, 0.55 |
| 16 | coal ore | 0.42, 0.42, 0.42 |
| 17 | log | top and bottom 0.66, 0.53, 0.35; sides 0.42, 0.32, 0.19; meta 1 spruce sides 0.32, 0.23, 0.13; meta 2 birch sides 0.68, 0.62, 0.50 |
| 18 | leaves | 0.32, 0.55, 0.24; meta 1 spruce 0.28, 0.42, 0.30; meta 2 birch 0.45, 0.62, 0.30 |
| 20 | glass | 0.85, 0.92, 0.95 |
| 21 | lapis ore | 0.42, 0.45, 0.62 |
| 24 | sandstone | 0.86, 0.82, 0.65 |
| 31 | tall grass | 0.35, 0.65, 0.25 |
| 35 | wool | meta 0 white 0.93, 0.93, 0.93; meta 1 0.95, 0.60, 0.20; meta 4 0.92, 0.85, 0.20; meta 11 0.25, 0.30, 0.85; meta 14 red 0.65, 0.20, 0.20; meta 15 black 0.10, 0.10, 0.10; any other meta: white |
| 41 | gold block | 0.95, 0.80, 0.25 |
| 42 | iron block | 0.87, 0.87, 0.87 |
| 45 | bricks | 0.60, 0.36, 0.30 |
| 46 | tnt | 0.85, 0.30, 0.25 |
| 47 | bookshelf | 0.60, 0.50, 0.35 |
| 48 | mossy cobblestone | 0.35, 0.45, 0.35 |
| 49 | obsidian | 0.12, 0.10, 0.18 |
| 50 | torch | 0.85, 0.70, 0.35 |
| 54 | chest | 0.55, 0.40, 0.22 |
| 56 | diamond ore | 0.45, 0.70, 0.70 |
| 57 | diamond block | 0.35, 0.85, 0.85 |
| 58 | crafting table | 0.50, 0.36, 0.22 |
| 61, 62 | furnace | 0.44, 0.44, 0.44 |
| 73 | redstone ore | 0.55, 0.35, 0.35 |
| 79 | ice | 0.55, 0.70, 0.95 |
| 80 | snow | 0.95, 0.97, 0.98 |
| 81 | cactus | 0.35, 0.55, 0.22 |
| 82 | clay | 0.63, 0.65, 0.68 |
| 83 | sugar cane | 0.55, 0.72, 0.42 |
| 87 | netherrack | 0.60, 0.25, 0.25 |
| 88 | soul sand | 0.40, 0.33, 0.26 |
| 89 | glowstone | 0.95, 0.85, 0.55 |
| 98 | stone bricks | 0.47, 0.47, 0.47 |
| 110 | mycelium | 0.55, 0.50, 0.55 |
| 129 | emerald ore | 0.42, 0.65, 0.50 |
| 155 | quartz block | 0.93, 0.92, 0.88 |
| 162 | log (acacia/dark oak) | as id 17's sides |
| 175 | double plants | 0.35, 0.65, 0.25 |

Anything else: `UNKNOWN_COLOR`. Document at the top of the file that these are the M1 stand-ins for the texture atlas and are replaced in M2.

- [ ] **Step 4: Implement the mesher**

`crates/oxide-game/src/mesher.rs`:

```rust
//! Turning world sections into terrain geometry: one quad per visible face.

use oxide_render::terrain::{ChunkMesh, Vertex};
use oxide_world::chunk::{SECTION_COUNT, SECTION_SIZE};
use oxide_world::world::World;

use crate::palette::{Face, block_color};

/// The four corners of one face, counter-clockwise seen from outside the block.
///
/// The winding is load-bearing: the pipeline culls back faces, so a face wound
/// the other way disappears. [`face_corners`] returns this table and the test
/// suite asserts every face's cross product points along its normal.
fn face_corners(face: Face) -> [[f32; 3]; 4] {
    match face {
        Face::Top => [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
        Face::Bottom => [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        Face::North => [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        Face::South => [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
        Face::West => [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]],
        Face::East => [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]],
    }
}
```

(Declare it `pub fn face_corners` since the test asserts on it, and document it.)

`build_section_mesh(world, cx, cz, sy)`:

1. Section position: `y_base = sy * SECTION_SIZE`; the section cube in world space spans `x = cx*16 .. cx*16+16`, `y = y_base .. y_base+16`, `z = cz*16 .. cz*16+16`.
2. For each of the 4096 block positions in that cube (indices via `block_index(x, y, z)`), read the packed value from `world.block(world_x, world_y, world_z)`; skip when the id is 0.
3. For each of the 6 faces, read the neighbour through `world.block` at the face offset; draw the face when the neighbour's id is 0.
4. Push four vertices with positions `world position + face_corners` and colour `block_color(id, meta, face) * brightness(face)`; push indices `[base, base+1, base+2, base, base+2, base+3]`.
5. Return `None` when no face was emitted; `Some(mesh)` otherwise.

`build_column_meshes` loops sections 0..16 and returns every index with `None` for empty ones.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p oxide-game`
Expected: all mesher tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-game/Cargo.toml crates/oxide-game/src/lib.rs crates/oxide-game/src/palette.rs crates/oxide-game/src/mesher.rs crates/oxide-world/src/chunk.rs crates/oxide-game/tests/mesher.rs
git commit -m "feat: add the block palette and the section mesher (M1)"
```

Note the new edges in the commit message: `oxide-game → oxide-world`, `oxide-game → oxide-render` (both already allowed) and the conversion helpers in `oxide-world`.

---

### Task 9: The session — login, obligations, world application, meshing

**Files:**
- Create: `crates/oxide-game/src/session.rs`
- Modify: `crates/oxide-game/src/lib.rs` (add `pub mod session;`)
- Modify: `crates/oxide-game/Cargo.toml` (add `oxide-proto`, `crossbeam-channel`)
- Test: `crates/oxide-game/tests/session_replay.rs`

**Interfaces:**
- Consumes: Task 1's `oxide_proto::conn::Conn` and `Compression`; Tasks 3–5's codecs; Task 6's `World`; Task 8's `build_column_meshes`.
- Produces:

```rust
/// What the session needs to connect and behave like a vanilla client.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Server host.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// The offline-mode player name.
    pub username: String,
    /// The settings sent in Client Settings.
    pub settings: ClientSettings,
}

/// Everything the session reports to the window.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent {
    /// Login succeeded.
    LoggedIn { uuid: String, username: String },
    /// Join Game arrived: the world exists and settings have been sent.
    Joined { entity_id: i32, gamemode: u8, dimension: i8, difficulty: u8, max_players: u8, level_type: String },
    /// The server placed or moved the player.
    PlayerPosition { x: f64, y: f64, z: f64, yaw: f32, pitch: f32 },
    /// A column's meshes were (re)built; `None` means the section now draws nothing.
    ChunkUpdated { cx: i32, cz: i32, sections: Vec<(usize, Option<ChunkMesh>)> },
    /// A column was unloaded by the server.
    ChunkUnloaded { cx: i32, cz: i32 },
    /// A keepalive was answered.
    KeepAlive { id: i32 },
    /// The server closed the session, with its reason.
    Disconnected { reason: String },
}

/// Something went wrong in the session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The connection or framing failed.
    #[error("connection error: {0}")]
    Frame(#[from] oxide_proto::frame::FrameError),
    /// A packet could not be decoded.
    #[error("packet error: {0}")]
    Packet(#[from] oxide_proto_v47::PacketError),
    /// The server asked for encryption; M1 does not implement it.
    #[error("the server requires an encrypted session, which M1 does not implement")]
    EncryptionRequired,
    /// A write of our own reply failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A client session over one framed connection.
pub struct Session<S> { /* private */ }

impl Session<std::net::TcpStream> {
    /// Connects and returns the session; nothing is sent until [`Session::run`].
    pub fn connect(config: &SessionConfig) -> Result<Self, SessionError>;
}

impl<S: std::io::Read + std::io::Write> Session<S> {
    /// Wraps an existing connection.
    pub fn new(conn: oxide_proto::conn::Conn<S>, config: SessionConfig) -> Self;
    /// Runs the session to completion: handshake, login, then the play loop.
    ///
    /// Returns `Ok(())` when the server closed the connection or the stream
    /// ended. Malformed packets are reported as an error, never a panic.
    pub fn run_over(self, events: &crossbeam_channel::Sender<ClientEvent>) -> Result<(), SessionError>;
}

impl Session<std::net::TcpStream> {
    /// Connects and runs; the entry point the client thread calls.
    pub fn run(self, events: &crossbeam_channel::Sender<ClientEvent>) -> Result<(), SessionError>;
}
```

- [ ] **Step 1: Write the failing replay tests**

`crates/oxide-game/tests/session_replay.rs` — a scripted server byte stream over an in-memory duplex, asserting the client's exact replies and the events it emits. Build the stream with the M1 codecs so the test is a real decode exercise:

```rust
//! Replay tests: a scripted 1.8.9 server stream drives the session, and the
//! client's own traffic and events are asserted byte for byte.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use oxide_game::session::{ClientEvent, Session, SessionConfig};
use oxide_proto::conn::Conn;
use oxide_proto::frame::Compression;
use oxide_proto_v47::serverbound::ClientSettings;

/// A duplex stream: what the client writes is inspected, what the script put
/// there is read back.
struct Duplex {
    incoming: std::io::Cursor<Vec<u8>>,
    outgoing: Arc<Mutex<Vec<u8>>>,
}

impl Read for Duplex {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.incoming.read(out)
    }
}

impl Write for Duplex {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.outgoing.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The compressed login and play frames a scripted server sends.
fn scripted_server_stream() -> Vec<u8> {
    use oxide_proto::frame::write_frame;
    let mut out = Vec::new();
    // Login Success, under compression as the server sends it.
    let mut login_success = vec![0x02];
    let body = "069a79f4-44e9-4726-a5be-fca90e38aaf5OxideDev";
    login_success.push(body.len() as u8);
    login_success.extend_from_slice(body.as_bytes());
    write_frame(&mut out, &login_success, Compression::Enabled { threshold: 256 }).unwrap();
    // Join Game.
    let mut join = vec![0x01];
    join.extend_from_slice(&20i32.to_be_bytes());
    join.extend_from_slice(&[0, 0, 1, 20, 7]);
    join.extend_from_slice(b"default");
    join.push(0);
    write_frame(&mut out, &join, Compression::Enabled { threshold: 256 }).unwrap();
    // Player Position And Look: absolute.
    let mut position = vec![0x08];
    position.extend_from_slice(&0.5f64.to_be_bytes());
    position.extend_from_slice(&65.0f64.to_be_bytes());
    position.extend_from_slice(&(-12.5f64).to_be_bytes());
    position.extend_from_slice(&0.0f32.to_be_bytes());
    position.extend_from_slice(&0.0f32.to_be_bytes());
    position.push(0);
    write_frame(&mut out, &position, Compression::Enabled { threshold: 256 }).unwrap();
    // Keep Alive.
    let mut keep_alive = vec![0x00];
    oxide_proto::varint::write_varint(&mut keep_alive, 7).unwrap();
    write_frame(&mut out, &keep_alive, Compression::Enabled { threshold: 256 }).unwrap();
    // One chunk column: ground-up, mask 0x0001, one stone block at the origin.
    let mut column = Vec::new();
    column.extend_from_slice(&[0u8; 8192]);
    column[..2].copy_from_slice(&0x0010u16.to_le_bytes());
    column.extend_from_slice(&[0u8; 2048]);      // block light
    column.extend_from_slice(&[0xFFu8; 2048]);   // sky light
    column.extend_from_slice(&[1u8; 256]);       // biomes
    let mut chunk = vec![0x21];
    chunk.extend_from_slice(&0i32.to_be_bytes());
    chunk.extend_from_slice(&0i32.to_be_bytes());
    chunk.push(1);
    chunk.extend_from_slice(&0x0001u16.to_be_bytes());
    oxide_proto::varint::write_varint(&mut chunk, column.len() as i32).unwrap();
    chunk.extend_from_slice(&column);
    write_frame(&mut out, &chunk, Compression::Enabled { threshold: 256 }).unwrap();
    out
}
```

The stream is scripted, not conversational: the server's frames sit at the front of `incoming`, and the client's own writes land in `outgoing`, where the test reads them back. That works because the session reads and writes independently — nothing in `run_over` depends on reading its own writes. Assert the write order the same way the session sends it.

```rust
#[test]
fn the_session_logs_in_joins_and_answers_every_obligation() {
    let outgoing = Arc::new(Mutex::new(Vec::new()));
    let duplex = Duplex { incoming: std::io::Cursor::new(scripted_server_stream()), outgoing: Arc::clone(&outgoing) };
    let config = SessionConfig {
        host: "127.0.0.1".into(),
        port: 25565,
        username: "OxideDev".into(),
        settings: ClientSettings::default(),
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    let session = Session::new(Conn::new(duplex), config);
    session.run_over(&sender).expect("the session runs to the end of the stream");

    let events: Vec<ClientEvent> = receiver.try_iter().collect();
    assert!(matches!(events[0], ClientEvent::LoggedIn { .. }));
    assert!(matches!(events[1], ClientEvent::Joined { entity_id: 20, dimension: 0, .. }));
    assert!(matches!(events[2], ClientEvent::PlayerPosition { x, .. } if x == 0.5));
    assert!(matches!(events[3], ClientEvent::KeepAlive { id: 7 }));
    match &events[4] {
        ClientEvent::ChunkUpdated { cx: 0, cz: 0, sections } => {
            let mesh = sections[0].1.as_ref().expect("section 0 draws");
            assert_eq!(mesh.vertices.len(), 24, "one stone block, six faces");
        }
        other => panic!("expected a chunk update, got {other:?}"),
    }

    let written = outgoing.lock().unwrap().clone();
    // Replay the client's traffic under the same framing and check the packets.
    let mut cursor = &written[..];
    let handshake = oxide_proto::frame::read_frame(&mut cursor, Compression::Disabled).unwrap();
    assert_eq!(handshake[0], 0x00, "handshake first");
    assert_eq!(oxide_proto::varint::read_varint(&handshake[1..]).unwrap(), 47);
    let login_start = oxide_proto::frame::read_frame(&mut cursor, Compression::Disabled).unwrap();
    assert_eq!(login_start, b"\x00\x08OxideDev");
    // Everything after Set Compression is compressed framing, even when small.
    let settings = oxide_proto::frame::read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).unwrap();
    assert_eq!(settings[0], 0x15);
    let brand = oxide_proto::frame::read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).unwrap();
    assert_eq!(&brand[..2], b"\x17\x08");
    assert_eq!(&brand[2..], b"MC|Brandvanilla");
    let echo = oxide_proto::frame::read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).unwrap();
    assert_eq!(echo[0], 0x06);
    let keep_alive = oxide_proto::frame::read_frame(&mut cursor, Compression::Enabled { threshold: 256 }).unwrap();
    assert_eq!(keep_alive, [0x00, 0x07]);
    assert!(cursor.is_empty(), "no further packets were sent");
}
```

`scripted_server_stream` also has to include the Set Compression frame the server sends before it switches: write it *uncompressed* (the server's own framing is plain until it enables compression) as the first frame of the stream, before Login Success — which is why the test above adds it at the top of `scripted_server_stream` as `[0x03, VarInt 256]` under `Compression::Disabled`. Task 2's capture fixes the observed ordering; if the capture shows Set Compression after Login Success, the test follows the capture and the session must handle both orders.

Second test — the unload shape and a malformed packet:

```rust
#[test]
fn an_unload_packet_removes_the_column() {
    // Preceded by the same login sequence, then a ground-up chunk with mask 0.
    // Assert: ClientEvent::ChunkUnloaded { cx, cz }.
}

#[test]
fn a_malformed_packet_is_an_error_not_a_panic() {
    // A Join Game frame cut short: run_over returns Err(SessionError::Packet(_)).
}

#[test]
fn a_server_disconnect_reports_its_reason() {
    // A Play Disconnect (0x40) frame: run_over returns Ok(()) and the last
    // event is ClientEvent::Disconnected { reason }.
}
```

Write all three out with the same helpers.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-game --test session_replay`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the session**

`crates/oxide-game/src/session.rs`, per the Interfaces block, with these rules:

- `run_over` first writes the handshake (`write_handshake(PROTOCOL, host, port, NEXT_STATE_LOGIN)`) and Login Start, then loops `conn.recv()`:
  - Login phase: `clientbound::decode_login(&payload)`; `SetCompression` → `set_compression(Compression::from_server_threshold(threshold))` and log the threshold; `LoginSuccess` → emit `LoggedIn`, switch to play; `Disconnect` → emit `Disconnected` and return `Ok(())`; `EncryptionRequest` → return `Err(SessionError::EncryptionRequired)`; anything else → warn and skip.
  - Play phase: dispatch on the id read with `read_packet_id`, then decode and handle: `0x00` keepalive (echo, emit), `0x01` join game (store, build the `World` with `has_sky = dimension == 0`, send Client Settings and `MC|Brand`, emit `Joined`), `0x08` position (apply relative flags against the current position, send the echo with the same absolute values, emit), `0x21` chunk data (apply; on the unload shape unload and emit `ChunkUnloaded`; otherwise rebuild the meshes of the column *and its four neighbours*, emitting `ChunkUpdated` for each), `0x26` bulk (same per column), `0x38` player list (store names; nothing is emitted in M1), `0x3f` plugin message (log at debug), `0x40` play disconnect (emit and return `Ok(())`), `0x46` (warn, ignore), anything else (log at debug, skip).
  - End of stream (`FrameError` carrying `UnexpectedEof`) → log and return `Ok(())`.
- The position the session tracks is updated by `0x08` and used for the relative flags; there is no movement in M1, so it is also the camera.
- Every reply is written with `conn.send`, so compression is applied automatically once enabled.
- A malformed packet of a *known* id returns `Err(SessionError::Packet(_))` with the id in the log line. A packet of an *unknown* id is skipped, never fatal (spec S2).
- `run(self, events)` = `Session::connect` + `run_over`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p oxide-game`
Expected: all replay tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/oxide-game/Cargo.toml crates/oxide-game/src/session.rs crates/oxide-game/src/lib.rs crates/oxide-game/tests/session_replay.rs Cargo.lock
git commit -m "feat: add the session state machine and the connection obligations (M1). New dependency: crossbeam-channel (MIT/Apache-2.0)"
```

---

### Task 10: The terrain pipeline, the depth buffer, and the overlay pass

**Files:**
- Create: `crates/oxide-render/src/terrain_pass.rs`
- Create: `crates/oxide-render/src/debug_text.rs`
- Create: `crates/oxide-render/src/overlay.rs`
- Modify: `crates/oxide-render/src/renderer.rs` (own the passes, the depth texture, and the mesh table)
- Modify: `crates/oxide-render/src/lib.rs` (add the three modules)
- Test: `crates/oxide-render/tests/pipeline_headless.rs` (ignored, needs a GPU) and unit tests in the modules

**Interfaces:**
- Consumes: Task 7's `terrain::{ChunkMesh, SectionKey, vertex_bytes}` and `camera::Camera`.
- Produces:

```rust
// renderer.rs additions
impl Renderer {
    /// Replaces the mesh for a section; `None` removes it.
    pub fn set_section_mesh(&mut self, key: SectionKey, mesh: Option<&ChunkMesh>);
    /// Sets the camera for the next frame.
    pub fn set_camera(&mut self, camera: Camera);
    /// Sets the overlay lines drawn this frame; empty hides the overlay.
    pub fn set_overlay_lines(&mut self, lines: Vec<String>);
}
```

```rust
// debug_text.rs
/// The width of one glyph cell in font pixels.
pub const GLYPH_WIDTH: usize = 5;
/// The height of one glyph cell in font pixels.
pub const GLYPH_HEIGHT: usize = 7;
/// The advance between glyphs in font pixels.
pub const GLYPH_ADVANCE: usize = 6;

/// The five column bytes of a character, bit 0 of each the top row.
pub fn glyph(character: char) -> [u8; GLYPH_WIDTH];

/// One quad of the overlay, in physical pixels: x, y, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelQuad {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Lays out one line of text as one quad per set pixel.
pub fn line_quads(text: &str, x: f32, y: f32, scale: f32) -> Vec<PixelQuad>;

/// Lays out several lines, each on its own row `(GLYPH_HEIGHT + 2) * scale`
/// lower than the last.
pub fn block_quads(lines: &[String], x: f32, y: f32, scale: f32) -> Vec<PixelQuad>;
```

- [ ] **Step 1: Write the failing text tests and the headless pipeline test**

`crates/oxide-render/src/debug_text.rs` gets module tests:

```rust
#[cfg(test)]
mod tests {
    use super::{GLYPH_ADVANCE, GLYPH_HEIGHT, GLYPH_WIDTH, block_quads, glyph, line_quads};

    #[test]
    fn the_glyph_table_covers_printable_ascii() {
        for code in 32u8..=126 {
            let character = code as char;
            let columns = glyph(character);
            assert_eq!(columns.len(), GLYPH_WIDTH);
        }
        // Space draws nothing; every other printable character draws something.
        assert_eq!(glyph(' '), [0; GLYPH_WIDTH]);
        for code in 33u8..=126 {
            let character = code as char;
            assert_ne!(glyph(character), [0; GLYPH_WIDTH], "{character:?} is blank");
        }
    }

    #[test]
    fn known_glyphs_match_their_shapes() {
        // A hyphen is the middle row, all five columns.
        assert_eq!(glyph('-'), [0x08, 0x08, 0x08, 0x08, 0x08]);
        // A full stop is the bottom row of the middle column.
        assert_eq!(glyph('.'), [0x00, 0x00, 0x40, 0x00, 0x00]);
        // A vertical bar is the whole middle column.
        assert_eq!(glyph('|'), [0x00, 0x00, 0x7f, 0x00, 0x00]);
    }

    #[test]
    fn an_unknown_character_renders_as_a_space() {
        assert_eq!(glyph('\u{1F600}'), [0; GLYPH_WIDTH]);
    }

    #[test]
    fn a_line_lays_out_five_quads_per_set_pixel() {
        // '|' has seven set pixels, '.' one.
        let quads = line_quads("|.", 0.0, 0.0, 1.0);
        assert_eq!(quads.len(), 8);
        assert!(quads.iter().all(|quad| quad.width == 1.0 && quad.height == 1.0));
        // The full stop sits at the bottom row of the second cell.
        let dot = quads.last().expect("the dot");
        assert_eq!(dot.y, (GLYPH_HEIGHT - 1) as f32);
        assert_eq!(dot.x, GLYPH_ADVANCE as f32 + 2.0, "third column of the second cell");
    }

    #[test]
    fn scale_multiplies_every_measurement() {
        let quads = line_quads("|", 10.0, 20.0, 2.0);
        assert_eq!(quads.len(), 7);
        assert!(quads.iter().all(|quad| quad.width == 2.0 && quad.height == 2.0));
        assert_eq!(quads[0].x, 10.0 + 2.0 * 2.0, "column 2, scaled");
        assert_eq!(quads[0].y, 20.0, "top row");
    }

    #[test]
    fn block_quads_stack_the_lines() {
        let lines = vec!["A".to_string(), "B".to_string()];
        let quads = block_quads(&lines, 0.0, 0.0, 1.0);
        let per_line = line_quads("A", 0.0, 0.0, 1.0).len();
        assert_eq!(quads.len(), 2 * per_line);
        let second = &quads[per_line..];
        let line_height = (GLYPH_HEIGHT + 2) as f32;
        assert!(second.iter().all(|quad| quad.y >= line_height));
    }
}
```

The glyphs in these tests are the pinned shapes: with bit 0 as the top row, `0x08` is row 3 (the middle of seven), `0x40` is row 6 (the bottom), `0x7F` is every row. Write the glyph table so these hold.

`crates/oxide-render/tests/pipeline_headless.rs` (ignored): build a headless device (copy the pattern from `tests/headless.rs`), create `TerrainPass` and `OverlayPass` for `Rgba8Unorm` plus a depth texture, render one stone block's mesh from a camera looking at it, read the centre pixel back and assert it is the stone colour (grey, ~0.5 × 255) and the corner pixel is the sky colour; then render an overlay line and assert some non-sky pixels appear in the top-left region. Mark it `#[ignore = "needs a GPU adapter; run locally with -- --ignored"]` like the M0 test.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-render`
Expected: FAIL, unresolved imports in the new tests.

- [ ] **Step 3: Implement the debug font and the overlay layout**

`crates/oxide-render/src/debug_text.rs`: a `const GLYPHS: [[u8; 5]; 95]` table for ASCII 32..=126 (five column bytes per glyph, bit 0 the top row, bit 6 the bottom), plus `glyph`, `line_quads`, `block_quads` per the Interfaces block. Replace any character outside the range with a space. Document at the top that this font is M1 scaffolding replaced by the jar font pipeline in M2, and that the three pinned glyphs in the tests fix the bit convention.

- [ ] **Step 4: Implement the terrain pass**

`crates/oxide-render/src/terrain_pass.rs`:

- `TerrainPass::new(device, format)` builds a pipeline with: vertex layout from `VERTEX_BYTES` (position `Float32x3` at offset 0, colour `Float32x3` at offset 12), `front_face: Ccw`, `cull_mode: Some(Back)`, depth `Less` with depth writes on, format `Depth32Float`, one uniform buffer holding the view-projection matrix, and the WGSL shader inline (`include_str!` or a `const SHADER: &str = r#"..."#;`). The shader takes position and colour and passes the colour through; no texturing, no lighting.
- `TerrainPass::set_camera(queue, camera, aspect)` writes the matrix into the uniform buffer.
- `upload(device, queue, key, mesh)`/`remove(key)`: a `HashMap<SectionKey, GpuMesh>` holding the vertex buffer, the index buffer, and the index count; `draw(pass)` binds each and issues `draw_indexed`.
- Document the winding contract: the mesher's faces are counter-clockwise seen from outside, and the pipeline culls back faces; the headless test's read-back fails loudly if either side changes.

- [ ] **Step 5: Implement the overlay pass**

`crates/oxide-render/src/overlay.rs`:

- `OverlayPass::new(device, format)` with a pipeline that has no depth attachment (the overlay draws over the world), `cull_mode: None`, alpha blending off (colors are opaque), vertices of `position: Float32x2` in physical pixels plus `color: Float32x3`, and an orthographic matrix mapping `(0,0)` to the top-left corner and `(width, height)` to the bottom-right.
- `set_size(width, height)` rebuilds the ortho matrix.
- `upload_text(device, queue, lines)`: `block_quads(lines, 4.0, 4.0, 2.0)` at scale 2, with a shadow: a second copy at `+1` pixel offset in a dark colour (0.05, 0.05, 0.05) drawn first. Store as one vertex/index pair.
- The overlay's scale of 2 and its 4-pixel margin are M1's stand-ins; M6 owns the real F3 layout.

- [ ] **Step 6: Wire the passes into the renderer**

`renderer.rs`: add the depth texture (created at the surface size, recreated in `resize` and `reconfigure` when the size changed), a `TerrainPass`, an `OverlayPass`, and the mesh table through the pass; `render()` clears colour and depth, then draws the terrain pass, then the overlay pass. Keep `classify_surface_error`, the frame-counting contract (unchanged: only presented frames count), and the clear colour (the sky blue stays; the sky pass is M2's).

- [ ] **Step 7: Run the tests, including the ignored GPU test on this machine**

Run: `cargo test -p oxide-render`
Run: `cargo test -p oxide-render --test pipeline_headless -- --ignored --nocapture`
Expected: unit tests pass; the headless test renders and reads back the expected pixels on the T500.

- [ ] **Step 8: Commit**

```bash
git add crates/oxide-render/src/lib.rs crates/oxide-render/src/renderer.rs crates/oxide-render/src/terrain_pass.rs crates/oxide-render/src/overlay.rs crates/oxide-render/src/debug_text.rs crates/oxide-render/tests/pipeline_headless.rs
git commit -m "feat: add the terrain pipeline, the depth buffer and the overlay pass (M1)"
```

---

### Task 11: The debug lines and the client wiring

**Files:**
- Create: `crates/oxide-game/src/hud.rs`
- Modify: `crates/oxide-game/src/lib.rs` (add `pub mod hud;`)
- Modify: `crates/oxide-client/src/main.rs`
- Modify: `crates/oxide-client/Cargo.toml` (add `oxide-game`, `oxide-world`, `oxide-proto`, `oxide-proto-v47`, `clap`, `crossbeam-channel`)
- Test: `crates/oxide-game/tests/hud.rs`, plus CLI tests in `main.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:

```rust
// hud.rs

/// What the debug overlay reports about the session this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HudState {
    /// The frame rate the counter last measured.
    pub fps: f32,
    /// The player's position as the server reports it.
    pub position: [f64; 3],
    /// Yaw in degrees.
    pub yaw: f32,
    /// Pitch in degrees.
    pub pitch: f32,
    /// The dimension: -1 nether, 0 overworld, 1 end.
    pub dimension: i8,
    /// The server address shown on the line.
    pub server: String,
    /// The entity id the server assigned.
    pub entity_id: i32,
}

/// The overlay's lines, in draw order.
pub fn debug_lines(state: &HudState) -> Vec<String>;
```

The exact output, pinned by tests:

```text
Oxidecraft 1.8.9
<fps:.0> fps
x/y/z: <x:.3> / <y:.5> / <z:.3>
Block: <floor x> <floor y> <floor z>
Chunk: <chunk x> <chunk z> in <x mod 16, floored> <z mod 16, floored>
Facing: <south|west|north|east> (<yaw:.1> / <pitch:.1>)
Dimension: <Overworld|Nether|The End>
Server: <server> (protocol 47)
Entity: <entity id>
```

`Facing` maps yaw 0 to `south`, 90 to `west`, 180 and -180 to `north`, 270 and -90 to `east` (vanilla's mapping).

- [ ] **Step 1: Write the failing HUD tests**

`crates/oxide-game/tests/hud.rs`:

```rust
//! The overlay's exact text.

use oxide_game::hud::{HudState, debug_lines};

fn state() -> HudState {
    HudState {
        fps: 60.2,
        position: [-23.5, 71.0625, 118.5],
        yaw: 0.0,
        pitch: 12.34,
        dimension: 0,
        server: "127.0.0.1:25565".into(),
        entity_id: 20,
    }
}

#[test]
fn the_overlay_reports_the_vanilla_style_lines_in_order() {
    let lines = debug_lines(&state());
    assert_eq!(lines[0], "Oxidecraft 1.8.9");
    assert_eq!(lines[1], "60 fps");
    assert_eq!(lines[2], "x/y/z: -23.500 / 71.06250 / 118.500");
    assert_eq!(lines[3], "Block: -24 71 118");
    assert_eq!(lines[4], "Chunk: -2 7 in 8 6");
    assert_eq!(lines[5], "Facing: south (0.0 / 12.3)");
    assert_eq!(lines[6], "Dimension: Overworld");
    assert_eq!(lines[7], "Server: 127.0.0.1:25565 (protocol 47)");
    assert_eq!(lines[8], "Entity: 20");
}

#[test]
fn negative_coordinates_use_floor_and_euclidean_remainders() {
    let mut state = state();
    state.position = [-0.5, 64.0, -0.5];
    let lines = debug_lines(&state);
    assert_eq!(lines[3], "Block: -1 64 -1");
    assert_eq!(lines[4], "Chunk: -1 -1 in 15 15");
}

#[test]
fn facing_follows_the_vanilla_compass() {
    for (yaw, expected) in [(0.0, "south"), (90.0, "west"), (180.0, "north"), (-90.0, "east")] {
        let mut state = state();
        state.yaw = yaw;
        assert!(
            debug_lines(&state)[5].starts_with(&format!("Facing: {expected} ")),
            "yaw {yaw}"
        );
    }
}

#[test]
fn the_dimension_names_match_vanilla() {
    for (dimension, expected) in [(-1i8, "Nether"), (0, "Overworld"), (1, "The End")] {
        let mut state = state();
        state.dimension = dimension;
        assert_eq!(debug_lines(&state)[6], format!("Dimension: {expected}"));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p oxide-game --test hud`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement the HUD lines**

`crates/oxide-game/src/hud.rs` per the Interfaces block. Use `f64::floor()` and `rem_euclid(16.0).floor()` for the chunk-relative coordinates so the negative cases pass. Vanilla's F3 line writes the entity id as the player's id; keep the last line.

- [ ] **Step 4: Wire the client**

`crates/oxide-client/src/main.rs`:

- CLI (clap derive, matching the launcher's style):

```rust
#[derive(Parser)]
#[command(name = "oxide-client", version, about = "Oxidecraft client")]
struct Cli {
    /// Server to join, for example 127.0.0.1:25565. Without it the client
    /// shows the window and connects to nothing.
    #[arg(long)]
    server: Option<String>,
    /// The offline-mode player name.
    #[arg(long, default_value = "OxideDev")]
    username: String,
    /// Stop after this many presented frames.
    #[arg(long)]
    frames: Option<u64>,
}
```

- `--frames` also sets the existing `OXIDECRAFT_MAX_FRAMES` behaviour: the flag wins when present, the environment variable is the fallback. Keep the smoke run's semantics (only presented frames count).
- With `--server host:port`: parse, split with `rsplit_once(':')`, and spawn the session thread:

```rust
let (sender, receiver) = crossbeam_channel::unbounded();
std::thread::spawn(move || {
    let config = SessionConfig { host, port, username, settings: ClientSettings::default() };
    match Session::connect(&config) {
        Ok(session) => {
            if let Err(error) = session.run(&sender) {
                tracing::error!(error = ?error, "the session ended with an error");
            }
        }
        Err(error) => tracing::error!(error = ?error, "the connection could not be opened"),
    }
    let _ = sender.send(ClientEvent::Disconnected { reason: "the session thread ended".into() });
});
```

- Each frame, in `draw`: drain `receiver.try_iter()`, and:
  - `LoggedIn` / `Joined`: log; store the join parameters for the overlay.
  - `PlayerPosition`: store the position and look.
  - `ChunkUpdated { cx, cz, sections }`: for each `(sy, mesh)`, `renderer.set_section_mesh((cx, cz, sy as u8), mesh.as_ref())`.
  - `ChunkUnloaded`: remove every section of that column (`set_section_mesh(key, None)` for `sy in 0..16`).
  - `KeepAlive`: log at debug (the count is visible in the run log; Task 12 counts them).
  - `Disconnected`: log the reason.
  - Then: `renderer.set_camera(Camera { pose: CameraPose { position, yaw, pitch }, fov_degrees: DEFAULT_FOV, near: NEAR_PLANE, far_chunks: 8.0 })` and `renderer.set_overlay_lines(if overlay_visible { debug_lines(&hud_state) } else { Vec::new() })`.
- F3 toggles `overlay_visible`; it starts hidden until a session exists, then starts visible. Log the toggle.
- The title keeps the M0 format; when connected, append the server: `Oxidecraft — {fps:.0} fps — {adapter} — {server}` is acceptable if the M0 title line is preserved for the unconnected case.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p oxide-client -p oxide-game`
Expected: the HUD tests and the CLI tests pass (add CLI parse tests in the same style as the launcher's: `--frames` parses, a bad `--server` without a colon fails at connect time with a clear error, and the default username is `OxideDev`).

- [ ] **Step 6: Commit**

```bash
git add crates/oxide-game/src/hud.rs crates/oxide-game/src/lib.rs crates/oxide-game/tests/hud.rs crates/oxide-client/Cargo.toml crates/oxide-client/src/main.rs Cargo.lock
git commit -m "feat: join the session to the window and add the debug overlay lines (M1)"
```

---

### Task 12: The acceptance run on the rig

**Files:**
- Create (rig, git-ignored): `refs/rig/evidence/m1/client-run.log`, `refs/rig/evidence/m1/terrain-both-halves.png`, `refs/rig/evidence/m1/terrain-ground.png`, `refs/rig/evidence/m1/oxide-client-traffic.*`, `refs/rig/evidence/m1/acceptance-notes.md`
- No committed changes to code unless the run uncovers a defect (then fix, test, commit as usual).

**Interfaces:**
- Consumes: the built client (Task 11) and the rig from Task 2 (server on port 25566 behind the proxy on 25565, or the server restored to 25565 — record which).
- Produces: the evidence the milestone's exit criterion rests on, and the material `docs/STATE.md` records at close-out.

- [ ] **Step 1: Restore the plain rig and start the server**

```bash
cd /home/lucy/Desktop/Software/Projects/Oxidecraft/refs/rig/server
# Set server.properties back to server-port=25565 unless the capture rig is wanted.
./start.sh
python3 status_ping.py     # expect protocol 47
```

- [ ] **Step 2: Build and run our client against it**

```bash
cd /home/lucy/Desktop/Software/Projects/Oxidecraft
cargo build -p oxide-client
mkdir -p refs/rig/evidence/m1
./target/debug/oxide-client --server 127.0.0.1:25565 --username OxideDev 2>&1 | tee refs/rig/evidence/m1/client-run.log
```

Let it run for at least **two minutes** without touching it, so keepalives are exercised (the server disconnects after about 30 seconds of silence — surviving two minutes proves the obligation). Verify in the log and by reading `refs/rig/server/logs/latest.log` for the server's own view of the join.

- [ ] **Step 3: Build the mark above y=128 and teleport the client to it**

Through the server console (the rig's console FIFO, as `stop.sh` uses for `stop`):

```
/fill <x-6> 140 <z-6> <x+6> 140 <z+6> minecraft:wool 14
/fill <x-6> 141 <z-6> <x+6> 145 <z+6> minecraft:stone
/tp OxideDev <x+16> 150 <z+16>
```

Choose `x` and `z` near the spawn column (the capture manifest records the player's spawn coordinates; use them) so the terrain below is the same ground the capture covers. The client answers the teleport with its position echo, so the camera follows without any movement code.

- [ ] **Step 4: Capture the screenshots**

```bash
gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop \
  --method org.freedesktop.portal.Screenshot.Screenshot "" "{'interactive': <false>, 'handle_token': <'m1shot1'>}"
# then move the newest ~/Pictures/Screenshot-*.png to refs/rig/evidence/m1/terrain-both-halves.png
```

Capture twice: once from the teleport point looking down at the mark and the ground (this frame shows geometry above and below y=128 together, with the overlay's `y` line as the proof), and once from ground level near the terrain (`/tp OxideDev <x> 71 <z>`, looking at the landscape) so the block colours are visible across grass, dirt, stone, and water. Both captures need the overlay visible; if F3 starts hidden, press it once.

- [ ] **Step 5: Capture our client's own traffic through the proxy**

Run the client once more with `record_proxy.py` in front of the server (the same two-run setup as Task 2) and confirm from the analysis: handshake with protocol 47 and next-state 2, Login Start, Client Settings with the agreed values, `MC|Brand` = `vanilla`, the position echo, and keepalive replies over the whole run. Save the capture and the analyser's report under `refs/rig/evidence/m1/`.

- [ ] **Step 6: Write the acceptance note**

`refs/rig/evidence/m1/acceptance-notes.md`: what ran, when, the exact commands, the log excerpts that show the join, the keepalive count over the run, the teleport, the mark's coordinates, the screenshot file names, and — honestly — anything that did not work or needed a workaround. Include the vanilla comparison capture if open question 7 was answered yes.

- [ ] **Step 7: Inspect the screenshots**

Open both captures and check, before claiming anything: geometry above and below y=128 in one frame; the overlay legible and showing the y coordinate; block colours that match the terrain types in view (grass green on top surfaces, dirt brown on the sides, stone grey, water blue); no magenta blocks below y=128, and magenta only where the mark's wool meta has no palette entry (palette id 35 meta 14 is red — so expect red wool, or choose meta 0 white and expect white). If any of that fails, the defect goes back through the normal loop as a task.

---

### Task 13: The guard self-tests and the milestone close-out

**Files:**
- Modify: `scripts/check-assets.sh` (a `--self-test` mode)
- Modify: `scripts/check-graph.sh` (a `--self-test` mode)
- Modify: `.github/workflows/ci.yml` (run both self-tests in their jobs)
- Modify: `docs/STATE.md`
- Modify: `CHANGELOG.md`
- Create: `docs/handoff/2026-09-23-m1-close.md`
- Tag: `m1`

**Interfaces:**
- Consumes: the carry-over list from the M0 handoff ("the guards have no negative self-test yet; add one").
- Produces: the M1 close-out.

- [ ] **Step 1: Add the asset guard's negative self-test**

Refactor `scripts/check-assets.sh` so the scan is a function that takes a path list on stdin (`scan_paths`), used by the normal path with `git ls-files` and by `--self-test` with a fixture list. The self-test asserts three things and exits non-zero if any fails:

1. A clean list (`src/main.rs`, `docs/STATE.md`) passes.
2. A list containing `assets/indexes/1.8.json` fails.
3. A list containing `refs/rig/evidence/m1/shot.png` fails.

Print one line per case (`self-test: clean list passes`, `self-test: asset index refused`, `self-test: refs path refused`) so CI's log states what ran.

- [ ] **Step 2: Add the crate-graph check's negative self-test**

Same shape in `scripts/check-graph.sh`: extract the edge check into a function over "from -> to" lines, then `--self-test` asserts a clean edge list (`oxide-proto-v47 -> oxide-proto`) passes, and both `oxide-proto -> oxide-world` and `oxide-game -> oxide-launcher` fail.

- [ ] **Step 3: Wire both into CI**

In `.github/workflows/ci.yml`, add a step to the `graph` job (`bash scripts/check-graph.sh --self-test`) and one to the `assets` job (`bash scripts/check-assets.sh --self-test`), each before the normal run of its script.

- [ ] **Step 4: Run the full gate**

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
bash scripts/check-assets.sh
bash scripts/check-assets.sh --self-test
bash scripts/check-graph.sh
bash scripts/check-graph.sh --self-test
```

Expected: everything clean; the test count grows by the M1 suites (record the exact count in the commit message).

- [ ] **Step 5: Update the state document**

`docs/STATE.md`: the stage line (M1 complete, `m1` tag), a new "M1 evidence" section carrying the acceptance run's numbers (join against the rig, keepalive count, teleport, the two screenshots, our client's captured traffic, the capture's `0x21`/`0x26` findings), the resolved dependency versions table gains `glam` and `crossbeam-channel` with their versions, the caveats list gains whatever this milestone genuinely carries (expect at least: the debug font is scaffolding; water draws opaque; meshing is single-threaded and rebuilds neighbours; the `0x26` emission answer from the capture), and the next-actions section points at M2.

- [ ] **Step 6: Update the changelog**

`CHANGELOG.md`: a new section under `[Unreleased]` titled `Milestone M1: bytes to world` with an `### Added` list written from the milestone's deliverables (framed connection, codecs, login and play packets, column decoder, chunk store, session, mesher, terrain pipeline, overlay). Match the M0 entry's style.

- [ ] **Step 7: Write the handoff**

`docs/handoff/2026-09-23-m1-close.md` from `docs/handoff/TEMPLATE.md`: where the project stands, what M1 delivered with its evidence, what is verified versus planned, the environment facts that changed (the rig's server port if it stayed at 25566), the commands worth knowing, and what M2 starts with. Fully self-contained.

- [ ] **Step 8: Tag and push**

```bash
git add scripts/check-assets.sh scripts/check-graph.sh .github/workflows/ci.yml docs/STATE.md CHANGELOG.md docs/handoff/2026-09-23-m1-close.md
git commit -m "docs: close out milestone M1 (bytes to world)"
git tag -a m1 -m "M1: bytes to world"
git push origin main --follow-tags
```

- [ ] **Step 9: Confirm CI is green on the tagged head**

```bash
gh run list --limit 5
```

Expected: the run for the close-out commit is green on all seven jobs. Record the run id in `docs/STATE.md`'s CI line (a second small commit is fine and expected here).

---

## Milestone exit criteria (spec section 13, M1 row)

| Criterion | Where it is met |
| --- | --- |
| Protocol framing, handshake, status ping, offline login | Tasks 1, 3, 9; M0's status ping unchanged |
| Compression | Tasks 1 (framing), 3 (handover), 2 (live evidence of both paths), 9 (switch in the session) |
| Keepalive and the five connection obligations | Task 4 (codecs), Task 9 (behaviour and replay assertions) |
| Join Game, Client Settings | Tasks 4, 9, 11 |
| Chunk parsing `0x21` and `0x26` verified against a live capture | Task 2 (capture and fixtures), Task 5 (decoder and capture tests) |
| World store | Task 6 |
| Untextured terrain with block colours and an F3-style overlay | Tasks 7, 8, 10, 11 |
| Connect to the test server and see correct geometry above and below y=128 | Task 12 |

## Carry-over resolutions from the M0 handoff

| Item | Resolution |
| --- | --- |
| One write per byte on the VarInt path | Task 1: `Conn` buffers and flushes once per packet, with a test asserting a single write call |
| Confirm `0x26` emission against a live capture | Task 2 captures both runs and states the answer; Task 5 keeps both codecs fixture-tested either way |
| Framing suite as the M9 fuzz corpus seed | Left in place: the M0 framing tests are unchanged and remain the seed; no M1 work required |
| Guard negative self-tests | Task 13 |
| The frame counter counts only presented frames | Unchanged: Task 10 does not touch the counting path, and Task 11 reports the same value to the overlay |
| Store path builders take normalised ids | Unchanged: M1 adds no store paths |

## Verification notes for the milestone review

- Every task's tests must pass under `cargo test --workspace`, including the new suites, and the two ignored GPU tests must pass on this machine (`-- --ignored`).
- The capture fixtures are ground truth: a decoder failure against them is a decoder bug, never a fixture edit.
- Anything the plan could not know (an unexpected server packet, a rig quirk, a capture that contradicts the reference) is recorded in the task's report and, if it changes a decision here, in `docs/STATE.md`'s caveats with the reason.

