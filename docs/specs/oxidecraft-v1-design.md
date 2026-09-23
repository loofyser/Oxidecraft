# Oxidecraft — v1 Design Specification

| Field | Value |
| --- | --- |
| Status | Draft v3, for owner review and approval |
| Date | 2026-09-22 |
| Target | Minecraft Java Edition 1.8.9, protocol 47 |
| Repo | https://github.com/loofyser/Oxidecraft |
| License | GPL-3.0 (repo code only; no Mojang assets, no jars redistributed) |
| Revision history | v1 — initial design, approved by the owner. v2 — all 25 findings of `docs/reviews/2026-09-22-spec-review.md` applied. v3 — section 5.1: the `oxide-launcher` row gains `oxide-proto-v47`, because the launcher's status ping is spoken through the protocol crate. v4 — section 17 records the owner-directed post-v1 programme (singleplayer and Java mod compatibility) and its feasibility; no v1 scope change. |

---

## 1. Purpose and goals

Oxidecraft is a from-scratch re-implementation of the Minecraft Java Edition client in Rust.
The first release targets 1.8.9 (protocol 47) multiplayer on Linux, and is written to stay
cross-platform.

Protocol 47 covers the whole 1.8 line: there are no packet-id or field differences between
1.8.0, 1.8.8 and 1.8.9. One codec set therefore serves the entire 1.8.x series, which is why
Oxidecraft targets 1.8.9 while shipping a single protocol crate.

The goal is a client that a player cannot distinguish from the official client while
playing, and a runtime that is clearly better:

1. **Fidelity** — the same look, feel, and play behavior as vanilla 1.8.9.
2. **Performance** — more frames per second than vanilla on the same hardware.
3. **Weight** — a native binary with no JVM, using less than half of vanilla's memory.
4. **Stability** — no crashes across long sessions; recoverable network errors.
5. **Openness** — GPL-3.0 source, no proprietary components.

A launcher ships alongside the client. The launcher fetches game assets from Mojang's
public distribution service, exactly the way the official launcher does.

### 1.1 Legal posture

The project follows the verified analysis in `docs/research/launcher-assets-auth-survey.md`,
which is reconnaissance, not legal advice.

- The README and every product listing carry the required disclaimer: *NOT AN OFFICIAL
  MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.*
- The name is neutral. "Minecraft" appears only in descriptive text.
- Assets and the client jar are fetched at runtime from Mojang's distribution service and
  verified by hash. Nothing Mojang-made is committed, bundled, or redistributed.
- No `.class` file is ever read from the jar, ever. This is a design invariant, not an
  accident, and it is enforced by review and by the CI asset guard (section 12).
- The offline login path exists for the local test rig and for development. The shipped flow
  requires a genuine Microsoft login to reach online-mode servers. Oxidecraft never provides
  a login bypass for online-mode servers.

## 2. Non-goals for v1

These are explicitly out of scope. Each has a resolution rule in section 16.

- Singleplayer, the integrated server, world generation, and Anvil save loading (post-v1 programme, section 17).
- Minecraft versions other than 1.8.x.
- Mods, plugins, resource packs, shaders, and any scripting layer during v1 (Java mod compatibility is a post-v1 programme item, section 17).
- Packaging for Windows and macOS (the code must stay portable; packaging comes later).
- Server-side software. Oxidecraft is a client only.
- Anti-cheat compatibility guarantees on public servers.

## 3. Acceptance criteria

### 3.1 Functional

| ID | Criterion |
| --- | --- |
| F1 | Connect to a vanilla 1.8.9 server over the offline (no-encryption) login path |
| F2 | Render the world with the correct blocks, textures, lighting, and biome tint |
| F3 | Move, look, jump, sprint, sneak, and fly with vanilla movement behavior |
| F4 | Place and break blocks with client-side prediction and server reconciliation |
| F5 | See other players, mobs, and object entities, moving smoothly via interpolation |
| F6 | Chat: send, receive, formatting codes, click and hover events, vanilla wrapping and 20-line scrollback |
| F7 | Player list (tab) and scoreboard sidebar match the server state |
| F8 | Inventory, crafting, and every server-openable container works with correct click semantics |
| F9 | HUD: hotbar, health, hunger, armor, experience, air, crosshair, damage overlay, boss bar, nametags |
| F10 | Sound effects and music play with correct pitch, volume, and 3D attenuation |
| F11 | Particles, weather, sky, stars, clouds, fog, and the block outline and crack overlays match vanilla |
| F12 | Vanilla screens: main menu, server list, pause (including Statistics and Achievements), options, death, disconnect, F3 with its sub-modes, screenshot hotkey |
| F13 | Microsoft account login and encrypted sessions with online-mode servers (milestone M7) |
| F14 | Death and respawn, including dimension changes, leave no stale state and re-derive environment flags |

### 3.2 Fidelity: what "1-to-1" means, measurably

| ID | Criterion |
| --- | --- |
| P1 | Screenshot parity, using the capture procedure in appendix C: the same view, position, time of day, weather, and settings, compared side by side against vanilla on the rig, within the stated tolerance |
| P2 | Numeric parity: field of view (including sprint, water, flying, bow-draw, and death modifiers, and the 0.5-per-tick smoothing clamped to 0.1–1.5), mouse sensitivity mapping, movement speeds, jump apex, gravity, and camera smoothing match the formulas in `docs/research/render-parity-survey.md` |
| P3 | GUI parity against a named list: every screen and HUD element listed in section 11.2 has the same layout, textures, and widget behavior as 1.8.9, verified item by item |
| P4 | Protocol parity, using the capture procedure in appendix C: for the same action sequence, the server observes the same packet types with the same field values as from a vanilla client, allowing only the state-dependent variants enumerated in appendix C and a timing tolerance of one tick |
| P5 | Tick parity: simulation runs at 20 ticks per second; entities interpolate between the two most recent server positions by `partialTicks`, snapping when a teleport exceeds 4 blocks; the 3-tick cadence is the server's movement-packet rate, not a client smoothing window |
| P6 | Divergences are allowed only when listed both in section 4 of this document and in `docs/DIVERGENCES.md` |

### 3.3 Performance

Measured on the same machine, same world, same position, same settings, following the
protocol in appendix C, with vanilla as the baseline:

| ID | Target |
| --- | --- |
| R1 | At least 1.5x vanilla frames per second |
| R2 | Less than 50% of vanilla's resident memory |
| R3 | Cold process start to the main menu under 1 second on a warm store: no network fetch, no jar extraction, and no atlas rebuild. The first-run path is explicitly excluded and is measured separately |

The M2 milestone records the baseline numbers. The M9 milestone proves the targets.

### 3.4 Stability

| ID | Criterion |
| --- | --- |
| S1 | A 2-hour soak session passes: no crash, no unhandled error, resident memory growth under 5%, and no monotonic growth in chunk, entity, or mesh counts |
| S2 | Malformed framing never panics; the client disconnects with a clear error. A well-formed packet with an unknown id is skipped, not fatal, so modded servers do not break the client |
| S3 | A dropped connection is reported and the client returns to the main menu cleanly |
| S4 | Assets are hash-verified; a corrupted file is re-downloaded, not loaded |

## 4. Decisions recorded

| # | Question | Decision |
| --- | --- | --- |
| D1 | v1 scope | Multiplayer-first: connect, render, move, chat, place and break blocks |
| D2 | Microsoft auth timing | Phase 2, after the client plays (milestone M7) |
| D3 | License | GPL-3.0 |
| D4 | Code reuse policy | RustCraft is a read-only reference; zero code copied from it. Adaptations allowed from MIT/Apache sources (Stevenarella, Azalea) with attribution in `NOTICE` |
| D5 | Project name | Oxidecraft, crate prefix `oxide-` |
| D6 | GitHub automation | `gh` CLI authorized as loofyser; commits push to `origin/main` |
| D7 | Asset pipeline | Launcher downloads `client.jar` and asset objects into our own store, verifies SHA-1, extracts jar resources once; reuses an existing vanilla install's files when present |
| D8 | Performance targets | Comparative: 1.5x FPS, under 50% memory, under 1 second cold start (section 3.3) |
| D9 | Verification rig | Local offline-mode 1.8.9 server plus a vanilla client for parity screenshots |
| D10 | Design outline | Approved; this document expands it |
| D11 | Options file format | `options.toml`, our own format. Vanilla's `options.txt` is not read or written. Recorded as a divergence |
| D12 | Title screen version text | "Oxidecraft 1.8.9". Recorded as a divergence. The F3 first line uses the same string |
| D13 | Client brand | Send `vanilla` in `MC|Brand` for parity; recorded in `docs/DIVERGENCES.md` with the reason, because plugins read it |
| D14 | Options that are no-ops | The no-op set is closed and explicit: Use VBOs, Advanced OpenGL, 3D Anaglyph, Snooper, Super Secret Settings, and the Resource Packs button. VSync, Max Framerate, Fullscreen, Resolution, and GUI Scale are real and functional; R1's measurement protocol depends on that |
| D15 | Render distance range | v1 matches 1.8.9 exactly, up to 16 chunks. An extended range is a post-v1 item, not a v1 feature |
| D16 | Offline-mode policy | The offline path exists for the local rig and development. The shipped flow requires a genuine login for online servers; no bypass is provided |
| D17 | Legal invariants | No `.class` read, no asset committed, runtime fetch only, required README disclaimer. Enforced by review and the CI asset guard |
| D18 | Window clear colour | The sRGB-aware surface format renders the sky-blue clear colour visibly paler than vanilla 1.8.9's; recorded as entry 4 in `docs/DIVERGENCES.md`. The sky renderer revisits the colour pipeline in a later milestone |
| D19 | Post-v1 programme | Owner-directed 2026-09-23: singleplayer worlds and Java mod compatibility (Forge 1.8.9 and `.jar` mods) are in scope for the project, after v1 completes. Both need their own specs; the feasibility analysis and the shape of each track are in section 17 |

## 5. Architecture

### 5.1 Crate graph

The complete set of allowed dependencies:

| Crate | May depend on |
| --- | --- |
| `oxide-proto` | nothing |
| `oxide-proto-v47` | `oxide-proto` |
| `oxide-world` | `oxide-proto`, `oxide-proto-v47` |
| `oxide-assets` | nothing |
| `oxide-render` | `oxide-assets` |
| `oxide-game` | `oxide-proto-v47`, `oxide-world`, `oxide-assets`, `oxide-render` |
| `oxide-client` | all of the above |
| `oxide-launcher` | `oxide-assets`, `oxide-proto-v47` |

The launcher is a separate binary target. It does not depend on the client crate; it locates
and starts the client binary.

### 5.2 Layering rules

1. Only the edges in the table above exist. A CI check asserts the graph with `cargo tree` and fails the build on any unlisted edge.
2. `oxide-proto` knows about bytes, framing, compression, encryption, and the NBT codec. It never knows about blocks, chunks, or rendering.
3. `oxide-proto-v47` holds the packet definitions for protocol 47. Each further Minecraft version becomes its own protocol crate, for example `oxide-proto-v107` for 1.9. Protocol-specific chunk and packet codecs live in the protocol crate, so the world layer's storage stays version-parameterised (see section 16).
4. `oxide-world` knows chunks, blocks, light, entities, and inventory, and carries the block behaviour table (section 9). It never touches wgpu, windowing, sockets, or HTTP.
5. `oxide-render` receives prepared draw data and owns the GPU. It never reads the world directly and never runs game logic.
6. `oxide-game` owns the client state machine, physics, input, and screens. It joins the model tables from `oxide-assets` with the behaviour table from `oxide-world` and converts world state into draw lists for `oxide-render`.
7. `oxide-assets` owns all HTTP: the piston-meta fetch chain, asset and jar downloads, and remote skin textures. It never runs game logic.

### 5.3 Per-crate responsibilities

| Crate | Responsibility | Key items |
| --- | --- | --- |
| `oxide-proto` | Framing, VarInt, strings, zlib compression, AES/CFB8 and RSA handshake, connection state machine, packet traits, NBT codec | `Conn`, `Codec`, `Packet`, `Handshake`, `Encryption`, `Nbt` |
| `oxide-proto-v47` | All 74 clientbound and 26 serverbound play packets, plus handshake, status, and login states | `v47::clientbound::ChunkData`, `v47::serverbound::PlayerPosition` |
| `oxide-world` | Chunk store, section storage, lighting, block behaviour table, entities, inventory model, world time | `World`, `Chunk`, `Section`, `BlockBehaviour`, `LightEngine`, `Entity` |
| `oxide-assets` | piston-meta client, SHA-1 verified store, jar reader, extraction, atlas builder, model baker, font, sound index, skin fetch and cache | `AssetStore`, `VersionManifest`, `Jar`, `Atlas`, `BakedModel`, `Font`, `SoundIndex`, `SkinCache` |
| `oxide-render` | wgpu device and passes: sky, terrain, entities, particles, GUI, text; camera; frustum culling | `Renderer`, `TerrainPass`, `GuiPass`, `Camera`, `DrawList` |
| `oxide-game` | Tick loop, physics, input, interaction, screens, HUD, chat, sound playback, session join for online mode | `Game`, `Player`, `Physics`, `Screen`, `Hud`, `Chat` |
| `oxide-launcher` | Fetch and verify assets, manage profiles and accounts, launch the client, verify integrity | `oxide-launcher fetch`, `oxide-launcher auth`, `oxide-launcher play` |
| `oxide-client` | Window, event loop, wiring between game, renderer, and network thread | `main` |

## 6. Threading and data flow

Four execution contexts:

1. **Network thread** — blocking TCP read loop. Parses packets with `oxide-proto-v47`, pushes typed events into a channel to the tick thread, and drains a send channel. Handles the encryption stream and compression.
2. **Tick thread** — fixed 20 Hz step. Runs game logic, physics, player prediction, entity interpolation targets, light updates, and inventory state. A 1.8.9 server runs a 50 ms tick; we mirror it.
3. **Render thread** — the main thread, driven by the winit event loop. Renders at display refresh rate. Never blocks on the network and never mutates world state.
4. **Meshing pool** — a rayon pool with `cores - 1` workers. Builds chunk section meshes from immutable snapshots.

Data flow and its latency budget:

- The tick thread owns the authoritative client-side world. It publishes chunk sections as immutable `Arc` snapshots.
- The meshing pool consumes snapshots and produces vertex buffers keyed by section position and version. A finished mesh may be one tick behind the block data. That lag affects only newly edited terrain, and it is bounded by the meshing budget.
- The render thread reads published snapshots and ready meshes. It never takes a lock on the hot path; publication uses atomic pointer swaps.
- Overlay rendering does not wait for meshes: block outline, crack overlay, and entity transforms read current tick state directly, so interactive feedback is never a tick behind the simulation.
- A test measures input-to-visible-block-change latency and fails above 100 ms on the development machine.

## 7. Asset pipeline and launcher

### 7.1 Store layout

The launcher keeps its own store, so Oxidecraft never depends on a Java installation. The data
directory resolves through `XDG_DATA_HOME` when set, otherwise `~/.local/share`; on Windows and
macOS the platform equivalent is used via the `dirs` crate.

```
<data dir>/oxidecraft/
  versions/1.8.9/version.json          copied from piston-meta, hash-verified
  versions/1.8.9/client.jar            8,461,484 bytes, SHA-1 verified
  assets/indexes/1.8.json              the 1.8 asset index, 734 objects
  assets/objects/<xx>/<hash>           sounds, lang, icons, sound index
  extracted/1.8.9/                     jar resources, extracted once
  extracted/1.8.9/.manifest.json       extraction manifest, see 7.2
  skins/<hash>.png                     cached player skins
  logs/
  options.toml
  profiles.json
```

Rationale: the 1.8.9 asset index contains only sounds, language files, and icons. Every
texture, blockstate, model, font, and shader lives inside `client.jar`, so the jar is a
required download and its contents must be readable. See
`docs/research/launcher-assets-auth-survey.md` for the verified URL chain and sizes.

### 7.2 Store and fetch semantics

- **Extraction manifest**: `extracted/1.8.9/.manifest.json`, holding the jar SHA-1, the extractor schema version, and the list of extracted paths with sizes. Extraction is skipped only when all three match.
- **Atomic writes**: every downloaded object and every extracted file is written to a temporary name in the same directory and renamed into place. An interrupted fetch leaves no partial file under a final name.
- **Single instance**: a lock file in the data directory prevents two launcher runs from writing the store at once.
- **Retry policy**: three attempts with exponential backoff (1 s then 4 s between attempts) per request; a failed transfer is resumed from scratch because objects are small.
- **Disk precheck**: roughly 150 MB of free space is required before a first fetch, and the shortfall is reported before any download starts.
- **Verified store check**: `oxide-launcher fetch --verify` re-hashes every object and the jar, and reports mismatches; CI and M0 use it as the acceptance command.
- **Vanilla-install reuse**: when a vanilla install is found, matching hashed objects are copied or hard-linked instead of downloaded, with identical hash verification. The `.mcassetsroot` marker convention that vanilla uses to identify an assets root is listed as unverified in the survey; the launcher must therefore verify the directory's contents by probe (look for `assets/indexes/1.8.json` and a sample object) before trusting it, and must fall back to a self-contained download when the probe fails.

### 7.3 Jar extraction

Extracted content, using the jar census from the survey: `assets/minecraft/models` (1,595
files), `textures` (1,058), `blockstates` (340), `shaders` (87), `texts` (3), `lang` (1), and
`font` (1), plus `pack.mcmeta` and `sounds.json`. Texture `.mcmeta` sidecars are included:
they are what make lava, fire, water, and the portal animate. There is no `misc` directory at
that level; `textures/misc` is already covered by `textures`. The extracted tree feeds the
atlas builder, the model baker, and the font loader. The jar is never modified, never
redistributed, and its `.class` entries are never read.

### 7.4 Accounts and authentication

Offline mode first (v1): a username is enough for offline-mode servers and for the rig.

Milestone M7 adds Microsoft login:

1. Device code flow against `login.live.com`.
2. Xbox Live user authentication, then XSTS authorization.
3. `login_with_xbox` against `api.minecraftservices.com`, then entitlement and profile checks.
4. Session join, performed by the **client**, not the launcher: `oxide-game` posts `/session/minecraft/join` against `sessionserver.mojang.com` before the encryption response is sent. `oxide-proto`'s login state exposes an injection point that accepts the pre-computed hash.
5. The encrypted session: RSA server key, AES-128-CFB8 stream, and the Minecraft hexdigest.

Crypto invariants, each one a silent-failure trap the survey names explicitly:

- Parse the server key as DER `SubjectPublicKeyInfo` and size the ciphertext from the parsed modulus. Never assume 128 bytes; custom servers may use longer keys.
- CFB8 means an 8-bit (one byte) feedback segment size. Any other segment size silently corrupts the stream.
- Two independent cipher contexts, encrypt and decrypt, each continuous across packets.
- Padding is PKCS#1 v1.5, not OAEP.
- The "SHA-1 signature" is a hash, not a signature: the non-standard Minecraft hexdigest (a leading `-` on roughly half of digests). It ships as a named helper with the three golden vectors from the survey (`sha1("Notch")`, `sha1("jeb_")`, `sha1("simon")`) in the unit tests.

Tokens are stored in the OS keyring, never in plaintext files. This deliberately diverges
from RustCraft, which stores tokens as plaintext JSON.

M7 requires an Azure application id for the device code flow, plus a Minecraft API permission
grant. This is an external dependency with review lead time, so it is tracked from before M6
and not at M7 (section 13, section 16).

## 8. Protocol layer

Framing and codecs follow `docs/research/protocol-47-reference.md`:

- Length-prefixed frames with VarInt lengths; VarInt, string, position, angle, and UUID codecs.
- **Compression**: the threshold is the VarInt the server sends in Set Compression (login 0x03); vanilla's default is 256 bytes, and `-1` disables compression entirely. The rule is measured on the uncompressed `Packet ID + Data` size, not on the frame: at or above the threshold the frame is zlib-compressed, below it the frame carries a zero-length uncompressed marker. Set Compression and Login Success may arrive in either order. The broken play-state 0x46 variant is never used.
- **Chunk encoding is exact**, and "exact" means all six of these rules together:
  1. Block data for all included sections, in ascending Y order, as 4096 little-endian `u16` values each, `(id << 4) | meta`, with index order `(y << 8) | (z << 4) | x` — x varies fastest.
  2. Then block-light arrays for all included sections, in the same section order.
  3. Then sky-light arrays for all included sections, **only when the dimension has sky** (the Overworld). Nether and End columns carry no sky-light array at all.
  4. Then a 256-byte biome array, when the ground-up continuous flag is set.
  5. Nibble packing: even index holds the low nibble, odd index holds the high nibble. This is the single most commonly inverted detail in the format.
  6. Column size is `8192·N + 2048·N + (sky ? 2048·N : 0) + (groundUp ? 256 : 0)`, where `N` is the popcount of the primary bitmask. Fixture test: mask `0x0001` in the Overworld, ground-up ⇒ 12,544 bytes.
- The primary bitmask is 16 bits: bit *i* set means section *i* is present, covering y = 16i..16i+15. Sections outside the mask keep their previous light values; new sections start at block-light 0 and sky-light 15.
- `Map Chunk Bulk` (0x26) is ground-up with no per-column length field, per the report. The exact vanilla call site that emits it was not located during research; M1 confirms behavior against a live capture before we rely on it.
- **Connection obligations** that keep the session alive, all owned by M1 or M4:
  - Client Settings (0x15) sends the values from protocol §2.3: Locale (≤7 chars), View Distance, Chat Mode, Chat Colors, and Displayed Skin Parts.
  - Player Position And Look (0x08) **must** be answered with serverbound 0x06 carrying the same coordinates, or the server keeps teleporting the client.
  - Player List Item (0x38, action 0) must be processed before Spawn Player (0x0C) for the same UUID, or the entity never spawns.
  - Keep-alive arrives about once per second; the client answers each one, and the server disconnects after about 30 seconds of silence.
  - Client Status (0x16) is used for respawn, request statistics, and the open-inventory achievement.

Packet definitions are Rust structs with hand-written `Read`/`Write` implementations. The
alternative, a derive macro, hides the wire layout of a format we must match exactly; the
protocol is small enough that explicit code wins. Fixture tests generated from the archived
`minecraft-data` 1.8 definitions pin every packet's byte layout.

## 9. World model

- A chunk is 16x16x256 blocks: sixteen 16x16x16 sections, matching the wire format. The storage range is y = 0..255, and the primary bitmask is 16 bits wide.
- A section stores blocks as `u16` (id and metadata packed exactly as received), plus block-light and sky-light nibble arrays. Storage matches the wire format, which keeps parsing, light updates, and serialization trivial and avoids a translation layer. Section codecs live in the protocol crate, so the storage layout stays version-parameterised.
- `BlockBehaviour` is a code-defined table in `oxide-world` mapping ids and metadata to hardness, opacity and light filtering, light emission, material, render pass, and tinting kind. The values are taken from documented vanilla behaviour and cross-checked against decompiled 1.8.9 source during M2, the same way the research reports did. The table is not derived from jar JSON, because the 1.8 model format carries no behaviour fields; a resource change updates models and textures, not behaviour.
- The JSON-driven half is separate: blockstates and models are owned by `oxide-assets`, baked into `BakedModel`s, and joined with behaviour by `oxide-game`.
- `LightEngine` implements vanilla propagation:
  - Sky light at full strength (15) propagating straight **down** through a transparent block does not decrease.
  - Sky light propagating horizontally or upward, and any sky light below 15 spreading to a neighbour, decreases by one.
  - Light-filtering blocks — water, ice, leaves, cobwebs, and the rest of the report's list — reduce sky light by exactly one when it passes through.
  - Opaque blocks stop propagation. Light values live in four-bit ranges (0–15).
  - Sections outside the primary bitmask keep their previous light; the client must not assume they are dark.
  - Local recomputation is required after Block Change (0x22) and Multi Block Change (0x23); the server sends no light for those.
- Entities store the two most recent server positions, interpolated by `partialTicks`, with a snap when a teleport exceeds 4 blocks. Entity metadata, equipment, and velocity are stored alongside.
- Weather, time, difficulty, scoreboard, and dimension live in a world state struct updated from packets. Death and respawn clear entity and chunk state and re-derive the sky-light flag from the dimension.

## 10. Renderer

wgpu, one surface, one pipeline per pass. All draw data arrives as `DrawList` values built by
`oxide-game`; the renderer never inspects world structures.

| Pass | Contents |
| --- | --- |
| Sky | Sky gradient, sun, moon, stars, void fog |
| Clouds | Flat cloud layer drawn at `cloudHeight − cameraY + 0.33`, y=128, 1 block per tick westward drift, 1/2048 UV scale |
| Terrain | Three queues: opaque, cutout with alpha test, translucent sorted back to front |
| Entities | Models built from box geometry with per-part transforms, including nametags |
| Particles | Billboard quads, 40 vanilla particle types |
| World overlay | Block outline, crack overlay, water, lava, fire, and damage overlays |
| GUI | Screens and HUD from 2D quads on the vanilla GUI atlas |
| Text | Texture-based glyphs from `ascii.png` and the unicode pages |

Fidelity rules taken from `docs/research/render-parity-survey.md`:

- One terrain atlas, like vanilla's single `textures/atlas/blocks.png`, 16x16 pixel tiles, nearest-neighbour filtering, mipmaps on terrain per the vanilla mipmap setting.
- GUI item icons use a separate sampler with mipmaps and blur disabled, matching vanilla.
- Animated textures from `.mcmeta` frame sidecars, including the interpolate flag.
- Per-face quads, not greedy meshing. Smooth lighting and ambient occlusion are baked per vertex, and merging faces would change their values.
- Vertex layout carries position, texture coordinates, packed block and sky light, and tint, matching vanilla's per-vertex information.
- Fog: the default linear mode starts at 0.75 of the far plane and ends at the far plane; the sky pass uses linear from 0 to the far plane; **water is exponential (`GL_EXP`) with density 0.1, or 0.01 with Water Breathing; lava is exponential with density 2.0**. Exponential and exponential-squared are different modes; the report's values are per-mode.
- Camera: the full vanilla FOV formula — base FOV, the sprint modifier, the water modifier, the flying modifier (×1.1), the bow-draw reduction (down to ×0.85), the death zoom, and the 0.5-per-tick smoothing clamped to 0.1–1.5 — plus the vanilla mouse sensitivity formula, view bobbing, and hurt-camera roll. The projection matches `gluPerspective(fov, aspect, 0.05, farPlane * √2)`.
- Window behaviour: surface reconfiguration on resize, GUI scale recomputed against the new resolution, and fullscreen toggling, all covered by the parity checklist's multi-resolution test.
- Colour space: the surface format and lightmap scaling must reproduce vanilla's non-sRGB pipeline; the policy is fixed in appendix C before the M2 parity comparison.

Performance techniques, all of which preserve the vanilla image: frustum culling per
section, rayon-parallel meshing, one draw call per section queue rather than per face,
persistent vertex buffers, and optional multithreaded chunk upload.

## 11. Game logic and GUI

### 11.1 Movement, interaction, input

- Physics: vanilla constants and order of operations. Working values: walk 4.317, sprint 5.612, jump apex 1.2522 blocks (derived by simulating the recurrence; provenance recorded in section 12), plus the sneak and terminal-velocity values listed in the parity report, gravity, drag, liquid movement, ladders, and sneaking edge protection.
- Prediction: the client applies local movement immediately, then reconciles when the server sends a position update, exactly as vanilla does.
- Interaction: block reach is gamemode-dependent — 5.0 in creative, 4.5 otherwise; the value is re-verified against decompiled 1.8.9 during M3 and recorded in the behaviour table's notes. Break progress and tool timing follow block hardness, and placement obeys the collision rules.
- Input: vanilla default keybinds, mouse capture, the sensitivity formula, F2 screenshots, and F3 with its sub-modes (F3+B, G, H, P, A, T, and the lagometer).
- Death and respawn: on health at or below zero the death screen appears; the client sends Client Status respawn on request; a Respawn packet clears entities, chunks, and inventory state, re-derives the sky-light flag from the dimension, and the client answers the following Player Position And Look.

### 11.2 Screens and HUD

Every item below is in v1 and is verified item by item under P3.

- Screens: main menu, server list with ping, pause menu including Statistics and Achievements entries and their screens, options hub and its sub-screens (Skin Customization, Language, Chat Settings, Snooper, Resource Packs, plus the video, sound, controls, and multiplayer screens), death, disconnect, "Downloading terrain", chat with wrapping and 20-line scrollback, multiplayer sleep overlay.
- Containers, all server-openable: inventory, crafting table, furnace, chest and generic 54, hopper, dispenser and dropper, brewing stand, enchantment table, anvil, beacon, villager trading, horse, and the creative inventory. Sign editing is in v1; book and quill reading is in v1.
- HUD: hotbar, health, hunger, armor, experience, air, crosshair, boss health bar, item tooltips, held item, damage flash, block outline, crack overlay, nametags, tab list, scoreboard sidebar, spectator HUD, and third-person camera with HUD hide.
- Sound: effects and music ticker with streaming, vanilla categories, pitch randomisation, 3D attenuation, the 64-block music range, and the 100-tick fade.

Post-v1, recorded in `docs/DIVERGENCES.md`: book and quill writing, command block editing, the
win and credits screen, and an extended render distance beyond the vanilla 16 chunks.

## 12. Testing and verification

| Layer | Method |
| --- | --- |
| Codecs | Unit tests with golden byte fixtures generated from the archived `minecraft-data` 1.8 definitions, plus round-trip encode and decode. Includes the chunk-size fixture for mask 0x0001 (12,544 bytes) and the three hexdigest golden vectors |
| World | Unit tests for chunk parse, light propagation against small hand-built worlds, respawn and dimension-change state clearing, and inventory semantics |
| Physics | Test vectors against recorded vanilla values: walk 4.317, sprint 5.612, jump apex 1.2522 (provenance: derived by simulating the recurrence in the parity report, not read from source), plus sneak, terminal velocity, step-up, and collision cases |
| End-to-end | Replay harness: record a raw server byte stream once, replay it offline in tests, and assert world, entity, and HUD state with no panics |
| Live | Connect to the local 1.8.9 test server; compare packet sequence and behaviour against a vanilla client on the same server, per appendix C's capture procedure |
| Visual | The parity checklist, classified in appendix C into screenshot-diffable, behavioural, and audio items, each with its verification method |
| Performance | Criterion benchmarks for chunk decode, mesh build, and light propagation; a frame-rate harness following appendix C's protocol |
| Stability | Soak run with the appendix C pass condition, and a malformed-framing fuzz suite |
| Latency | Input-to-visible-block-change measurement, budget 100 ms |

Continuous integration runs on GitHub Actions with committed configuration:

1. `fmt` and `clippy` with warnings denied.
2. `test` on Linux, plus `cargo check` for the Windows (GNU) and macOS targets to keep portability honest from M0.
3. Release build.
4. `cargo-deny` with a committed `deny.toml` whose license allow-list covers GPL-3.0-compatible licenses; anything else fails.
5. The crate-graph check from section 5.2.
6. **The asset guard**: the build fails if any tracked file is a `.jar`, `.ogg`, `.png`, `.lang`, or asset-index JSON under an assets-shaped path, `glyph_sizes.bin`, or anything under `refs/` or `vanilla/`. A Mojang asset in git history requires a history rewrite, so the guard runs on every push.

## 13. Milestones

Each milestone ends with a commit, a `docs/STATE.md` update, and a tag.

| Milestone | Contents | Exit criteria |
| --- | --- | --- |
| **M0 Foundations** | Workspace, CI (including the asset guard and `deny.toml`), logging, error types, launcher `fetch --verify`, jar extraction with the manifest, blank wgpu window, README disclaimer, rig proven | CI green; `oxide-launcher fetch --verify` produces a fully verified store; window opens with an FPS counter; the rig's server answers a status ping and the vanilla client reaches its title screen |
| **M1 Bytes to world** | Protocol framing, handshake, status ping, offline login, compression, keepalive, the five connection obligations, Join Game, Client Settings, chunk parsing (0x21 and 0x26 verified against a live capture), world store, untextured terrain | Connect to the test server and see correct world geometry above y=128 as well as below, with block colours and an F3-style overlay |
| **M2 Textured terrain** | Atlas, block models, behaviour table cross-check, biome tint, light from nibbles with all three sky-light rules, fog, sky, clouds, colour-space policy, frustum culling, parallel meshing; the parity checklist classified into screenshot, behavioural, and audio sets; baseline recorded | The rig procedure from appendix C places both clients at the same position and time; screenshots match within tolerance; baseline numbers in `docs/perf.md` |
| **M3 Player** | Physics with the test vectors, input, camera with the full FOV formula, raycast with gamemode-dependent reach, break and place with prediction, crack overlay, movement packets, death and respawn, view bobbing | Measured movement values match the test vectors; a vanilla client sees our edits; dying and respawning leaves no stale state |
| **M4 Entities and chat** | Entity spawn, movement, metadata, skins (including skin texture fetch for online players), mob box models, object entities (arrows, thrown and dropped items, boats, minecarts, item frames, paintings, XP orbs), nametags, interpolation with the 4-block snap, chat with wrapping and click events, tab list, scoreboard, boss bar | Two clients, one vanilla and one ours, see each other, chat, and agree on entities |
| **M5 HUD and inventory** | Hotbar, health, hunger, armour, experience, air, item icons, tooltips, every server-openable container with drag and split semantics, sign editing, book reading, creative inventory | Inventory and container round-trips match the vanilla client screen for screen |
| **M6 Sound, particles, weather, screens** | Sound engine and music ticker, particles, weather, sky and stars, overlays, the full screen list from section 11.2, keybinds, F3 with sub-modes, F2 screenshots, the 1.8 render-distance range (to 16 chunks), window resize and fullscreen | The named screen list passes its item-by-item parity checks, including the F3 field set |
| **M7 Microsoft auth** | Device code flow, Xbox Live and XSTS chain, entitlement checks, keyring token storage, encrypted session with the crypto invariants, client-side session join, hexdigest helper, online-mode connection. Azure application and API permission granted before this milestone starts | Join an online-mode server with a real account |
| **M8 Launcher and packaging** | Launcher screens, single-profile management, integrity verify, desktop file, tarball and AppImage, AUR package | A defined fresh-machine test passes: the procedure in appendix C on a clean user account of the development machine |
| **M9 Hardening and performance** | Culling and meshing tuning, memory budget, soak and fuzz runs, latency measurement, crash reporting | R1–R3 proven per appendix C, S1 soak passes, and the documentation-complete list in appendix C is ticked |

## 14. Risks and mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Copying from a non-permissive source by accident | License contamination | `NOTICE` records every adapted file; review each adaptation; cargo-deny plus the review checklist |
| A Mojang asset enters git history | Unrecoverable without a history rewrite | CI asset guard (section 12) plus `.gitignore` for `refs/` and `vanilla/` |
| Microsoft changes the auth or session API | M7 blocked | Auth work isolated; Azalea's MIT implementation is a current reference |
| The Minecraft API permission is refused for the registered application | M7 cannot be tested | Registration starts before M6; fallback is a documented alternative client id or dropping online mode from v1 with a `docs/DIVERGENCES.md` entry |
| Anti-cheat flags a non-vanilla client | Cannot play on some public servers | v1 targets vanilla-compatible behavior and documents this limit; no guarantees |
| The parity rig fails on this machine (LWJGL2 under XWayland, standalone JRE 8, two GPUs) | P1–P4, R1, R2 all rest on it | The rig is proven in M0, before anything depends on it; appendix C fixes the GPU, driver, settings, and colour-space policy; a rig failure blocks M2 rather than silently weakening it |
| Colour or gamma mismatch between vanilla's OpenGL output and our Vulkan output | Screenshot parity unverifiable | Appendix C's colour-space policy is fixed and checked with a reference gradient before M2's comparison |
| Scope creep toward singleplayer | v1 slips | Section 2 keeps it out; the integrated server is a separate post-v1 project |
| Fidelity drifts as features land | "1-to-1" quietly fails | The parity checklist runs at every milestone that touches rendering |
| Mojang asset terms or usage guidelines change | Distribution path breaks | Assets are fetched at runtime only, never bundled; a policy change needs a launcher update, not a redesign; the required disclaimer is in place from M0 |
| Windows or macOS breaks silently | Portability claim rots | Cross-target `cargo check` in CI from M0 |

## 15. Repository conventions and context handoff

- Conventional Commits; trunk-based on `main`; a tag per milestone; `CHANGELOG.md` updated per milestone.
- `docs/STATE.md` carries the live project state: current milestone, completed work, next actions, open questions, environment facts, and exact commands. It is updated before every handoff and every long pause.
- `docs/handoff/` holds dated handoff notes, each self-contained enough to resume work from the repository alone.
- Research notes land in `docs/research/`, independent reviews in `docs/reviews/`.
- `refs/` and `vanilla/` are gitignored: they hold upstream clones, the rig, and the Mojang jar, none of which are redistributed.
- Dependency policy: versions pinned in a committed `Cargo.lock`, MSRV declared as Rust 1.85 (edition 2024), development on the installed 1.95; dependency updates arrive only through explicit, reviewed commits.

## 16. Deferred items with resolution rules

| Item | Resolution rule |
| --- | --- |
| Azure application id and Minecraft API permission | Starts before M6 as a tracked owner action. If refused, the owner approves a documented alternative or online mode leaves v1 |
| Book and quill writing, command block editing, win and credits screen | Post-v1, each recorded in `docs/DIVERGENCES.md` when the button or screen is present but inert |
| Extended render distance beyond 16 chunks | Post-v1 performance extra, recorded in `docs/DIVERGENCES.md` as a non-vanilla option with the settings-screen consequence stated |
| Resource packs | The button exists and is a no-op in v1 (D14); real packs are post-v1 |
| Windows and macOS packaging | After v1 ships on Linux; portability is enforced by CI cross-target checks from M0 |
| Singleplayer and the integrated server | Post-v1 programme, first track (owner-directed 2026-09-23). A separate project after v1, with its own spec: the integrated server, world generation, and Anvil save loading. Section 17.1 records the feasibility |
| Java mod loaders and `.jar` mods (Forge 1.8.9) | Post-v1 programme, second track (owner-directed 2026-09-23). Feasible only by running the Java game: a launcher-assembled vanilla-plus-Forge instance on a bundled JRE, everything fetched and verified at runtime and nothing redistributed. Re-implementing Forge's class loading, deobfuscation, and ASM patching in Rust is not a goal. Section 17.2 records the analysis |
| Other Minecraft versions | One protocol crate per version; world storage is version-parameterised and each protocol crate supplies its own chunk codec |
| `.mcassetsroot` vanilla-install detection | Verify by content probe before trusting, per section 7.2 |
| `Map Chunk Bulk` (0x26) emission | Confirm against a live capture in M1 before relying on it |
| 1.8.9 render-distance slider maximum and the exact 1.8 options set | Confirm on the rig in M6 against the vanilla options screen |
| Nametag appearance: text scale, background opacity, and distance rule | Not covered by the research reports; confirm on the rig in M4 and add a checklist item |

## 17. Post-v1 programme: singleplayer and Java mod compatibility

Owner-directed 2026-09-23. Neither item changes v1's scope: both start only after v1 is complete,
and each needs its own spec before any code. What follows is reconnaissance for those specs, not a
design, and it is separate from the v1 milestones in section 13.

### 17.1 Singleplayer worlds

Feasible within the existing architecture, and the smaller of the two tracks:

- **Integrated server.** The client already owns a tick loop (section 6). Singleplayer needs a
  server-side world with the same physics and a client that talks to it in-process rather than over
  a socket. The protocol layer is not involved.
- **World generation.** Seed-faithful generation means implementing vanilla 1.8.9's generator
  (biome layout, terrain shape, decoration). Large but well understood, and testable against worlds
  the rig server generates from the same seed.
- **Anvil save IO.** The format is already documented in `docs/research/protocol-47-reference.md`
  section 5, and M1's chunk store is the natural place to serialise from and to. The nibble and
  section conventions are the same ones the wire format uses, so nothing is relearned.
- **Legal position unchanged.** A singleplayer world is the user's own data; nothing is fetched,
  bundled, or redistributed.

### 17.2 Java mod loaders and `.jar` mods (Forge 1.8.9)

What the mods are matters more than any implementation choice: a 1.8.9 Forge mod is a compiled Java
class library built against a deobfuscated, patched `net.minecraft.client` plus the Forge API. Forge
loads the game through its own class loader, applies ASM bytecode transformers at class-load time,
and mods hook the running game through the Forge event bus and direct calls into game classes.
Rendering goes through LWJGL and OpenGL, and the world, entity, and inventory state the mods touch
are the Java objects themselves.

Consequences, stated plainly:

- A from-scratch Rust client cannot load these mods into its own process. Their bytecode must run
  against the classes they were compiled against, which requires a JVM and the vanilla-derived
  class files. There is no partial version of this that still runs real mods.
- **The feasible track** is a compatibility mode. The launcher assembles a vanilla 1.8.9 plus Forge
  instance — Forge installer artefacts and libraries fetched and hash-verified at runtime, the same
  way assets are today — and runs it on a bundled JRE. Oxidecraft supplies the launcher, the
  verified store, the account flow (M7), and the process management; the Java game supplies the play
  experience in that mode. This productises the shape the rig already uses.
- **Not a goal**: re-implementing Forge's class loading, deobfuscation, and ASM patching in Rust, or
  mixing LWJGL/OpenGL mod rendering into the wgpu renderer. Both are technically unbounded and
  neither produces compatible mods.
- **A cheaper parallel track, to be decided later**: joining modded servers. Server-side Forge mods
  are the server's problem; a client that speaks the FML handshake (plugin channel `FML|HS`) and
  applies the 1.8 registry-remapping rules can join many Forge servers with no JVM at all. That is
  bounded protocol work and gets its own spec if it is taken up.
- **A third track, if wanted later**: a native mod API of our own (Rust or WebAssembly plugins).
  Reliable and fast, but not compatible with `.jar` mods; a v2-or-later feature.
- **Legal position**: nothing Mojang-made, nothing from Forge, and nothing mod-authored is
  redistributed. The launcher fetches and verifies at runtime, and Forge's own terms are accepted by
  the user at install time, exactly as an official installation does.

## Appendix A: dependencies

| Purpose | Crate |
| --- | --- |
| Windowing and input | `winit` |
| Rendering | `wgpu` |
| Math | `glam` |
| JSON | `serde`, `serde_json` |
| TOML | `toml` |
| NBT | `simdnbt` in `oxide-proto`; hand-rolled fallback if its API or license proves unsuitable at M1 |
| Data directories | `dirs` |
| Compression | `flate2` with the zlib-ng backend (needs a C toolchain, present on this machine; verified at M0) |
| AES, RSA, SHA-1 | `aes`, `cfb8`, `rsa`, `x509-cert`, `sha1`, `hex` |
| Random | `rand` |
| Zip reading | `zip` |
| PNG decoding | `png` |
| Ogg Vorbis decoding | `lewton` |
| Audio output | `cpal` |
| Thread pool | `rayon` |
| Channels, atomics | `crossbeam-channel`, `arc-swap` |
| HTTP | `ureq` with `rustls` |
| CLI parsing | `clap` |
| Logging | `tracing`, `tracing-subscriber` |
| Error types | `thiserror` in libraries, `anyhow` in binaries |
| Test temp directories (dev) | `tempfile` |
| Keyring (M7) | `keyring` |
| Benchmarks | `criterion` |

All are permissive licenses, compatible with GPL-3.0, and all are enforced by `deny.toml`.

## Appendix B: documents

| Document | Contents |
| --- | --- |
| `docs/research/protocol-47-reference.md` | Lifecycle, all packet tables, chunk encoding, lighting, Anvil, entity metadata, pitfalls |
| `docs/research/render-parity-survey.md` | Asset split, atlas, FOV and physics formulas, particle list, screen inventory, the 57-item parity checklist |
| `docs/research/launcher-assets-auth-survey.md` | piston-meta chain with verified hashes, jar contents, Microsoft auth chain, legal notes |
| `docs/research/rustcraft-survey.md` | Reference-only survey; license analysed in detail |
| `docs/research/stevenarella-azalea-survey.md` | Reusable MIT/Apache components and their limits |
| `docs/reviews/2026-09-22-spec-review.md` | Independent review of this specification, with a disposition record for every finding |
| `docs/STATE.md` | Live project state and handoff protocol |

## Appendix C: measurement environment and protocols

### C.1 Environment

- GPU and driver for every comparison: record the exact device and driver version, and select it explicitly (Vulkan device choice, or `DRI_PRIME` for the OpenGL side). Two GPUs are present; comparisons never mix them.
- Vanilla runs under XWayland when XWayland is the only option; the fact is recorded, and the frame-rate baseline is only valid if vanilla is not XWayland-throttled (checked by comparing fullscreen and windowed frame rates).
- Vanilla settings for comparison runs: fixed render distance, fixed GUI scale, VSync off, particles and graphics settings recorded, fullscreen, and the exact `options.txt` archived alongside the screenshots.
- Colour space: our surface format and lightmap scaling reproduce vanilla's non-sRGB output; verified with a reference gradient screenshot before the first visual comparison.

### C.2 Screenshot parity procedure (P1)

1. Server commands place both clients at the same coordinates, facing, time, and weather (`/time set`, `/tp`, `/weather clear`), with fixed world seed.
2. Both clients run the same render distance, GUI scale, FOV, and graphics settings; HUD is hidden with F1 where the test targets world rendering.
3. Capture at the same resolution. Screenshots are stored under `docs/parity/YYYY-MM-DD/` with the settings archive.
4. Compare side by side, then with a per-pixel difference metric. Tolerance: no more than 2% of pixels differing by more than 8/255 per channel, with an absolute cap of 1% differing by more than 24/255. Differences caused by animated textures or entity positions are excluded by freezing the world (`/gamerule doDaylightCycle false`, mobs absent or stationary, `randomTickSpeed 0`).
5. Every checklist item is recorded as pass, fail, or deferred with an entry in `docs/DIVERGENCES.md`. Deferrals are only allowed for post-v1 features listed in section 16.

### C.3 Protocol parity procedure (P4)

1. Capture the byte stream of a vanilla client and of Oxidecraft against the same local server, in separate sessions, using a recording proxy.
2. Replay both captures, decode to packet type plus field values, and compare for the same scripted action sequence (join, walk, look, chat, place, break, open inventory, move items).
3. Allowed divergences: timing within one tick, and state-dependent packet variants explicitly enumerated here — position and look updates may be split or combined (0x03/0x04/0x05/0x06), chat may use the legacy or JSON path per the server's request, and keep-alive replies may land anywhere within the interval. Anything else is a finding.

### C.4 Performance procedure (R1–R3)

1. Warm-up: 3 minutes of play, then measure 5 minutes of a scripted route; repeat 3 times; report the median.
2. VSync off. Render distance, resolution, GUI scale, and camera path identical for both clients. Same GPU, same driver, recorded.
3. Memory: resident set size after the same 5-minute route, median of 3 runs. R3: measured with a warm store and page cache, from process start to the main menu appearing.

### C.5 Soak pass condition (S1)

Two hours on the rig server with a scripted route and periodic player joins. Pass requires: no
crash, no unhandled error in the log, resident memory growth under 5% between hour one and hour
two, and no monotonic growth in chunk, entity, or mesh counts across samples taken every 10
minutes.

### C.6 Checklist classification

The parity report's 57 items are classified in `docs/parity/checklist.md` as screenshot-diffable,
behavioural (physics feel, interpolation, mouse look, font metrics), or audio. Behavioural items
are verified by the numeric tests in section 12; audio items by a recorded comparison; only the
screenshot-diffable set feeds the automated image comparison.
