# Protocol 47 live capture — what the vanilla 1.8.9 server actually sends

Run A: 2026-09-23, 17:10:33–17:13:58 (local). Run B: 2026-09-26, 16:30:08–16:33:37.
Both sessions used the local verification rig: the vanilla 1.8.9 server from `refs/rig/server/`
(offline mode, seed `oxidecraft`, world `parity`, view distance 10, compression threshold 256)
and the vanilla 1.8.9 client from `refs/rig/client/`, playing as `OxideRef`.

The raw byte logs and the machine-readable reports stay under `refs/rig/evidence/m1/`:
`capture-run-a.{client,server}.bin` with `capture-report.json`, and
`capture-run-b.{client,server}.bin` with `capture-report-b.json`. The captures are 1.9 MB of
server-to-client bytes each; they are not committed to the repository.

## 1. Procedure

The client always connects to `127.0.0.1:25565`. A recording proxy owns that port and forwards
the TCP stream in both directions to an upstream server, writing each direction's raw bytes in
arrival order (`<out>.client.bin`, `<out>.server.bin`) together with a sidecar timeline of
arrival times (`<out>.timeline.jsonl`). The proxy is `refs/rig/tools/record_proxy.py`; it is
kept out of the repository. It accepts one connection and exits when both directions have
closed, after letting the server's final bytes flush.

The server was moved behind the proxy for both runs — `server-port=25566` in
`refs/rig/server/server.properties` — and the port was restored to 25565 afterwards (section 6).

Run A used a loopback upstream, so the server saw the proxy as a local peer:

```bash
python3 refs/rig/tools/record_proxy.py --listen 127.0.0.1:25565 --upstream 127.0.0.1:25566 \
  --out refs/rig/evidence/m1/capture-run-a --accept-timeout 900 --drain-timeout 30
cd refs/rig/client && bash launch-client.sh --join
```

Run B used the machine's LAN address as the upstream, so the server saw a non-loopback peer.
The plan named `10.1.34.142` for this; by the time of the run that address was no longer carried
by this machine (it is DHCP-assigned and had changed), so the run used the address the machine
had — `10.0.0.84`:

```bash
python3 refs/rig/tools/record_proxy.py --listen 127.0.0.1:25565 --upstream 10.0.0.84:25566 \
  --out refs/rig/evidence/m1/capture-run-b --accept-timeout 900 --drain-timeout 30
cd refs/rig/client && bash launch-client.sh --join
```

In both runs the client waited in the world until the chunk load had settled (roughly three and a
half minutes), was killed with `flatpak kill org.prismlauncher.PrismLauncher`, and the proxy was
allowed to drain and exit. Each run's peer address is in the server log: run A
`OxideRef[/127.0.0.1:38226]`, run B `OxideRef[/10.0.0.84:43536]` — the loopback and LAN paths are
therefore certain, not assumed. Run A was recorded by an earlier session of this task and is used
as recorded; its timeline window, spawn coordinates and logged peer address match that server log
exactly.

Both captures were read back with `refs/rig/tools/analyse_capture.py`, first exercised against a
synthetic capture pair built from the specification (both framing paths, the compression switch,
the always-present biome array, and the refusal of malformed or truncated streams). Each reading
pass exits non-zero on a frame it cannot parse; both exited zero:

```bash
python3 refs/rig/tools/analyse_capture.py refs/rig/evidence/m1/capture-run-a --json refs/rig/evidence/m1/capture-report.json
python3 refs/rig/tools/analyse_capture.py refs/rig/evidence/m1/capture-run-b --json refs/rig/evidence/m1/capture-report-b.json
```

## 2. Observed login sequence

Both runs are identical in shape:

```
client: Handshake(next state 2), Login Start("OxideRef")
server: Set Compression (login 0x03), Login Success (login 0x02)
```

Byte for byte, from run A (run B is identical):

* Handshake frame — `00 2f 09 "127.0.0.1" 63 dd 02`: protocol 47, the address string the client
  dialled, port 25565, next state 2. It is the first 16 bytes on the wire.
* Login Start — `00 08 "OxideRef"`.
* Set Compression — payload `03 80 02`: the VarInt threshold 256, still in the plain framing.
* Login Success — payload `02 24 "8b097698-930f-3a9f-b5fa-2518a8b5f7db" 08 "OxideRef"` (47
  bytes): the UUID as a hyphenated string, then the player name, sent under the compressed
  framing with data length 0.

There is no Encryption Request in either run, as expected of an offline-mode server. The
ordering is Set Compression **before** Login Success in both runs, and the switch to compressed
framing is immediate: the very next server frame (Login Success) already carries the data-length
field, set to 0 because the packet is smaller than the threshold.

**Both runs carried compression.** The plan expected the loopback run to skip Set Compression
because the server would see a local peer; the recording shows the server sending it anyway, with
the same threshold the rig configures (256). Whatever exemption the server applies to its own
in-process single-player channel, it does not extend to a loopback TCP peer. There is no capture
of an uncompressed play session in this evidence set; the uncompressed framing is still exercised
by the login phase of these captures, by every sub-threshold frame (below), and by the synthetic
capture pair used to check the reading pass.

The threshold rule held exactly, in both runs and both directions:

* every frame whose uncompressed size is ≥ 256 was sent as a zlib stream (the frame's data-length
  field carries the uncompressed size);
* every smaller frame carried data length 0 and raw bytes;
* only the terrain packets were ever compressed — 53 frames in run A (44 Map Chunk Bulk plus
  9 Chunk Data) and 44 in run B (42 plus 2). The largest raw frame payload was 234 bytes (198 in
  run B); the smallest compressed frame decompressed to 61 711 bytes;
* the client sent nothing larger than 34 bytes, so every client frame after the login was raw
  with data length 0 — including all 3 719 / 4 269 play frames.

## 3. Chunk packets

**The vanilla server emitted Map Chunk Bulk.** Run A carried 9 Chunk Data (0x21) and 44 Map Chunk
Bulk (0x26) frames, 405 of its 414 columns inside bulk packets; run B carried 2 and 42, with 414
of 416 columns inside bulk. No other packet type was ever compressed.

Bulk is the mass-delivery path: batches of up to 10 columns, most of them full ten (39 of 44
batches in run A, 41 of 42 in run B). The wire shape observed for a bulk frame matches the
reference exactly: id `26`, one sky-light flag byte (`01`, Overworld), the column count, then per
column an Int x, an Int z and an unsigned-short mask, then all the column payloads back to back
with no length fields. The first bulk frame of run A carries 10 columns and decompresses to
617 063 bytes = 103 header bytes + 10 × 61 696; its first column is the player's spawn column
(0, 11), and its first block values are all bedrock (the bottom layer starts solid).

Single-column Chunk Data frames are how columns that miss the initial burst arrive later: nine of
them in run A between +5 s and +178 s, two in run B at +33 s and +47 s. Their masks are 0x001f
and 0x003f, sizes always 12 288 × popcount(mask) + 256.

Column masks seen across both runs: 0x000f, 0x001f, 0x003f, 0x007f. Every mask's declared size
matched 12 288 × popcount(mask) + 256 in every frame of both captures — that is, every column
(0x21 ground-up and every bulk column alike) carried block light and sky light for each included
section and the 256-byte biome array. For bulk columns the biome array is always present; the
0x26 path must be sized with it or the stream desynchronises one column later.

The load is not instantaneous and not complete at the moment the player stops moving. The view
square around the spawn chunk is 21 × 21 = 441 columns; run A delivered 414 of them, run B 416.
The absent columns are consistently the outer ring's tail — eleven or so along the west edge
(x = −10), a few cells on the north and south edges. All 441 columns exist in the saved world, so
nothing was cut off at the source. The second session delivered exactly the two columns the first
session had still been missing, as lone Chunk Data packets, which shows the tail arriving one
column at a time well into a session. A client must therefore render a world that fills in
progressively and must not wait for a fixed column count before showing anything.

The world is live between sessions: 239 of the 414 common columns were byte-identical across the
two captures and 175 differed, in the way of random ticks and block updates (at the spawn column
itself, two tall-grass halves present on 23 September were gone by the 26th).

## 4. Obligations observed

**Keep Alive.** The server sends play-state Keep Alive (0x00) every ~2.05 s: 99 of them in run A
over 204.6 s and 101 in run B over 209.1 s; intervals ranged 2.02–2.08 s, median 2.05 s. The
vanilla client echoed every one, id for id, with no gaps and within a couple of milliseconds of
the server's transmission — an answering packet is expected promptly, and the id must be
repeated.

**Client Settings and brand.** The client's first play packet is Client Settings (serverbound
0x15), sent 0.42 s (run A) / 0.53 s (run B) after Join Game arrived and before the first chunk
column reached it (0.66 s before, in run A). Its payload is `15 05 "en_US" 0c 00 01 7f`: view
distance 12, chat mode 0, chat colours on, all skin parts. Immediately after it (about a
millisecond later) comes the brand plugin message: channel `MC|Brand`, data `07 "vanilla"`. Both
are what the server's own startup expects to see from a client that intends to be treated as
vanilla.

**Position echo.** Join is followed by Player Position And Look (0x08) with the teleport to
(15.5, 64.0, 178.5). The client answers with a Player Position And Look (serverbound 0x06)
carrying exactly those coordinates (and on-ground false), 0.39 s later in run A and 0.53 s later
in run B. The server repeats the teleport until the echo comes back: in run A it re-sent it five
more times within a millisecond, 0.9 s after the first, and the client answered all five; in run
B a single teleport drew two echoes. A client must answer every received teleport, and keep
answering if the server repeats it. Two details worth keeping: the teleport's yaw and pitch are
the player's saved look angles (run B arrived with −4.2° / +23.1°, run A with zeros for the fresh
player), and the client's ordinary movement packets while standing still are the short 0x03 form
(position and rotation unchanged, on-ground flag only), thousands of them, with the full
0x04/0x06 forms only when something moved.

## 5. Fixtures

Two columns are committed under `crates/oxide-proto-v47/tests/fixtures/m1-capture/`, both cut
straight out of the recorded server stream and named by their chunk coordinates.

**`column-0_11.bin` — the player's spawn column**, from run A (one of the ten columns of the
first Map Chunk Bulk frame). SHA-1 `4911f7854aba9c2c3331b0b7942efdea4f3a7f9d`, 61 696 bytes,
mask `0x001f` (five sections), sky light true, ground-up true. Its block-id counts
(id: count; they sum to 20 480 = 5 × 4 096):

```
0:8327  1:9919  2:139  3:792  7:785  8:2  9:47  11:89  12:17  13:2  14:11  15:95
16:149  21:5  31:14  56:3  73:12  169:2  204:1  208:1  221:3  238:3  240:1  255:5
2560:1  2992:2  3003:1  3258:3  3264:1  3276:3  3328:2  3520:2  3531:2  3549:6
3566:2  3584:2  3804:1  3822:10  3840:3  4095:15
```

**`column-4_15.bin` — a bulk payload from run B**, five chunks east of spawn. SHA-1
`c024c2f7a91fa5aa0c426e97bad3a3e880a591fa`, 86 272 bytes, mask `0x007f` (seven sections), sky
light true, ground-up true. Its block-id counts (they sum to 28 672 = 7 × 4 096):

```
0:13254  1:12972  2:162  3:1001  5:2  6:2  7:754  13:129  14:5  15:53  16:139
18:8  21:10  31:39  56:10  73:31  86:2  103:2  161:13  162:7  175:5
1092:2  1365:4  1383:2  1638:6  1911:6  3328:1  3584:3  3822:1  3824:1  3839:1
3840:9  4080:8  4095:28
```

Both files were checked against the world saved by the server. Every block slot, block-light
byte, sky-light byte and biome byte of `column-4_15.bin` matches the region file for chunk
(4, 15), and the same holds for run B's payload of the spawn column. `column-0_11.bin` matches
its region file except for the two tall-grass halves noted in section 3 — it is a snapshot of the
world as it stood on 23 September. The same chunk in both captures carries identical bytes for
`column-4_15.bin`, so it doubles as a cross-session check.

The counts include block identifiers well above the ordinary vanilla range (4095, 3840, 3822 and
others around them): the world itself contains them and the server passed them through unchanged,
so the reader must take the full 12-bit identifier field rather than assuming an eight-bit id.
The counts also match the settlement: solid stone and air dominate, bedrock occupies exactly one
layer, and the terrain stock (grass, dirt, water, ores, trees) is present in plausible amounts.

`manifest.json` in the same directory records both columns with their masks, sizes, SHA-1s and
counts, together with the capture's summary numbers. Its `packets` counts are run A's
server-to-client play histogram for the four packet types named (0x21: 9, 0x26: 44, 0x00: 99,
0x08: 6).

## 6. Rig changes

* `refs/rig/server/server.properties` was moved to `server-port=25566` for the two runs and has
  been restored to `server-port=25565`. The server and the recording proxy are stopped, and
  nothing is listening on 25565 or 25566.
* Run B's upstream address was `10.0.0.84` rather than the plan's `10.1.34.142`, which this
  machine no longer carries; see section 1.
* The client launch script was run as `bash launch-client.sh --join`; it does not carry an
  executable bit in this checkout.
* The world `parity` changes between sessions (section 3). Fixtures and counts are snapshots of
  the session named against them.
