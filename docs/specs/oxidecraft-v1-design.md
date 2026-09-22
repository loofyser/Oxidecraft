# Oxidecraft — v1 Design Specification

| Field | Value |
| --- | --- |
| Status | Draft, for review and approval |
| Date | 2026-09-22 |
| Target | Minecraft Java Edition 1.8.9, protocol 47 |
| Repo | https://github.com/loofyser/Oxidecraft |
| License | GPL-3.0 (repo code only; no Mojang assets, no jars redistributed) |
| Author | Hermes Agent, reviewed by the project owner |

---

## 1. Purpose and goals

Oxidecraft is a from-scratch re-implementation of the Minecraft Java Edition client in Rust.
The first release targets 1.8.9 (protocol 47) multiplayer on Linux, and is written to stay
cross-platform.

The goal is a client that a player cannot distinguish from the official client while
playing, and a runtime that is clearly better:

1. **Fidelity** — the same look, feel, and play behavior as vanilla 1.8.9.
2. **Performance** — more frames per second than vanilla on the same hardware.
3. **Weight** — a native binary with no JVM, using less than half of vanilla's memory.
4. **Stability** — no crashes across long sessions; recoverable network errors.
5. **Openness** — GPL-3.0 source, no proprietary components.

A launcher ships alongside the client. The launcher fetches game assets from Mojang's
public distribution service, exactly the way the official launcher does.

## 2. Non-goals for v1

These are explicitly out of scope. Each has a resolution rule in section 16.

- Singleplayer, the integrated server, world generation, and Anvil save loading.
- Minecraft versions other than 1.8.9.
- Mods, plugins, resource packs, shaders, and any scripting layer.
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
| F5 | See other players and mobs, moving smoothly via interpolation |
| F6 | Chat: send, receive, formatting codes, click and hover events |
| F7 | Player list (tab) and scoreboard sidebar match the server state |
| F8 | Inventory, crafting, and container screens work with correct click semantics |
| F9 | HUD: hotbar, health, hunger, armor, experience, air, crosshair, damage overlay |
| F10 | Sound effects play with correct pitch, volume, and 3D attenuation |
| F11 | Particles, weather, sky, stars, clouds, and fog match vanilla |
| F12 | Vanilla screens: main menu, server list, pause, options, death, disconnect, F3 |
| F13 | Microsoft account login and encrypted sessions with online-mode servers (milestone M7) |

### 3.2 Fidelity: what "1-to-1" means, measurably

| ID | Criterion |
| --- | --- |
| P1 | Screenshot parity: the same view, same position, same time of day, same settings, compared side by side against vanilla on the rig |
| P2 | Numeric parity: field of view, mouse sensitivity mapping, movement speeds, jump height, gravity, and camera smoothing match the formulas in `docs/research/render-parity-survey.md` |
| P3 | GUI parity: every implemented screen has the same layout, textures, and widget behavior as 1.8.9 |
| P4 | Protocol parity: a server sees the same packet sequence a vanilla client would send for the same actions |
| P5 | Tick parity: simulation runs at 20 ticks per second, with entity interpolation identical to vanilla's 3-tick scheme |
| P6 | Divergences are allowed only when listed in this document as intentional, and each is user-visible in `docs/DIVERGENCES.md` |

### 3.3 Performance

Measured on the same machine, same world, same position, same settings, and compared
against vanilla as the baseline:

| ID | Target |
| --- | --- |
| R1 | At least 1.5x vanilla frames per second |
| R2 | Less than 50% of vanilla's resident memory |
| R3 | Cold start under 1 second to the main menu, excluding asset download |

The M2 milestone records the baseline numbers. The M9 milestone proves the targets.

### 3.4 Stability

| ID | Criterion |
| --- | --- |
| S1 | A 2-hour soak session on the test server with no crash, no leak, and no unbounded growth |
| S2 | Malformed or hostile packets never panic the client; they disconnect with a clear error |
| S3 | A dropped connection is reported and the client returns to the main menu cleanly |
| S4 | Assets are hash-verified; a corrupted file is re-downloaded, not loaded |

## 4. Decisions recorded

These are the answers to the open questions, recorded for the decision log.

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
| `oxide-launcher` | `oxide-assets` |

The launcher is a separate binary target. It does not depend on the client crate; it locates
and starts the client binary.

### 5.2 Layering rules

1. Only the edges in the table above exist. A CI check asserts the graph with `cargo tree` and fails the build on any unlisted edge.
2. `oxide-proto` knows about bytes, framing, compression, and encryption. It never knows about blocks, chunks, or rendering.
3. `oxide-proto-v47` holds the packet definitions for protocol 47. A future version becomes its own crate, for example `oxide-proto-v2126`. Adding a version never touches the renderer.
4. `oxide-world` knows chunks, blocks, light, entities, and inventory. It never touches wgpu, windowing, or sockets.
5. `oxide-render` receives prepared draw data and owns the GPU. It never reads the world directly and never runs game logic.
6. `oxide-game` owns the client state machine, physics, input, and screens. It converts world state into draw lists for `oxide-render`.

### 5.3 Per-crate responsibilities

| Crate | Responsibility | Key items |
| --- | --- | --- |
| `oxide-proto` | Framing, VarInt, strings, zlib compression, AES/CFB8 and RSA handshake, connection state machine, packet traits | `Conn`, `Codec`, `Packet`, `Handshake`, `Encryption` |
| `oxide-proto-v47` | All 74 clientbound and 26 serverbound play packets, plus handshake, status, and login states | `v47::clientbound::ChunkData`, `v47::serverbound::PlayerPosition` |
| `oxide-world` | Chunk store, section storage, lighting, block registry, entities, player inventory model, world time | `World`, `Chunk`, `Section`, `BlockRegistry`, `LightEngine`, `Entity` |
| `oxide-assets` | piston-meta client, SHA-1 verified store, jar reader, extraction, atlas builder, model baker, font, sound index | `AssetStore`, `VersionManifest`, `Jar`, `Atlas`, `BakedModel`, `Font`, `SoundIndex` |
| `oxide-render` | wgpu device and passes: sky, terrain, entities, particles, GUI, text; camera; frustum culling | `Renderer`, `TerrainPass`, `GuiPass`, `Camera`, `DrawList` |
| `oxide-game` | Tick loop, physics, input, interaction, screens, HUD, chat, sound playback | `Game`, `Player`, `Physics`, `Screen`, `Hud`, `Chat` |
| `oxide-launcher` | Fetch and verify assets, manage profiles and accounts, launch the client, verify integrity | `oxide-launcher fetch`, `oxide-launcher auth`, `oxide-launcher play` |
| `oxide-client` | Window, event loop, wiring between game, renderer, and network thread | `main` |

## 6. Threading and data flow

Four execution contexts:

1. **Network thread** — blocking TCP read loop. Parses packets with `oxide-proto-v47`, pushes typed events into a channel to the tick thread, and drains a send channel. Handles the encryption stream and compression.
2. **Tick thread** — fixed 20 Hz step. Runs game logic, physics, player prediction, entity interpolation targets, light updates, and inventory state. A 1.8.9 server runs a 50 ms tick; we mirror it.
3. **Render thread** — the main thread, driven by the winit event loop. Renders at display refresh rate. Never blocks on the network and never mutates world state.
4. **Meshing pool** — a rayon pool with `cores - 1` workers. Builds chunk section meshes from immutable snapshots.

Data flow:

- The tick thread owns the authoritative client-side world. It publishes chunk sections as immutable `Arc` snapshots.
- The meshing pool consumes snapshots and produces vertex buffers keyed by section position and version.
- The render thread reads published snapshots and ready meshes. It never takes a lock on the hot path; publication uses atomic pointer swaps.
- The render snapshot lags the tick state by at most one tick, which mirrors vanilla's own decoupling.

## 7. Asset pipeline and launcher

### 7.1 Store layout

The launcher keeps its own store, so Oxidecraft never depends on a Java installation:

```
~/.local/share/oxidecraft/
  versions/1.8.9/version.json          copied from piston-meta, hash-verified
  versions/1.8.9/client.jar            8,461,484 bytes, SHA-1 verified
  assets/indexes/1.8.json              the 1.8 asset index, 734 objects
  assets/objects/<xx>/<hash>           sounds, lang, icons, sound index
  extracted/1.8.9/                     jar resources, extracted once
  logs/
  options.toml
  profiles.json                        optional, phase 2
```

Rationale: the 1.8.9 asset index contains only sounds, language files, and icons. Every
texture, blockstate, model, font, and shader lives inside `client.jar`, so the jar is a
required download and its contents must be readable. See
`docs/research/launcher-assets-auth-survey.md` for the verified URL chain and sizes.

### 7.2 Fetch and verify

1. Fetch `version_manifest_v2.json`.
2. Fetch the 1.8.9 version JSON and verify its SHA-1 against the manifest.
3. Fetch `client.jar` and verify SHA-1 `3870888a...`.
4. Fetch the `1.8` asset index from `launchermeta.mojang.com` and download each object from `resources.download.minecraft.net/<first two hex>/<hash>`, verifying SHA-1.
5. Extract jar resources once into `extracted/1.8.9/`, recording a manifest so later runs skip work.

If an existing vanilla install is found (`~/.minecraft` or a path the user provides), copy
or hard-link matching objects from it instead of downloading. Hash verification is
identical in both paths.

### 7.3 Jar extraction

Extracted content: `assets/minecraft/{textures,models,blockstates,font,texts,shaders,misc,lang}`.
The extracted tree feeds the atlas builder, the model baker, and the font loader. The jar is
never modified and never redistributed.

### 7.4 Accounts and authentication

Offline mode first (v1): a username is enough for offline-mode servers.

Milestone M7 adds Microsoft login:

1. Device code flow against `login.live.com`.
2. Xbox Live user authentication, then XSTS authorization.
3. `login_with_xbox` against `api.minecraftservices.com`, then entitlement and profile checks.
4. Session join against `sessionserver.mojang.com` for online-mode servers.
5. The encrypted session: RSA-1024 server key, AES-128-CFB8 stream, SHA-1 signature.

Tokens are stored in the OS keyring, never in plaintext files. This deliberately diverges
from RustCraft, which stores tokens as plaintext JSON.

M7 requires an Azure application id for the device code flow. The project owner registers a
free application, or explicitly approves an alternative. This is tracked in section 16.

## 8. Protocol layer

Framing and codecs follow `docs/research/protocol-47-reference.md`:

- Length-prefixed frames with VarInt lengths; VarInt, string, position, angle, and UUID codecs.
- Compression enabled by the server's Set Compression (threshold 256); frames at or above the threshold are zlib-compressed, smaller frames carry a zero-length uncompressed marker.
- Login encryption: RSA-1024 PKCS#1 v1.5 for the shared secret, AES-128-CFB8 afterwards with the shared secret as both key and IV.
- All 74 clientbound and 26 serverbound play packets for protocol 47. The MVP subset that M1 and M2 need is listed in the research report.
- Chunk encoding is exact: 4096 `u16` little-endian block values `(id << 4) | meta`, then block-light nibbles, then sky-light nibbles, then a 256-byte biome array. There is no Add bitmask and no separate metadata array in 1.8.
- `Map Chunk Bulk` (0x26) is always ground-up and carries no per-column length field.

Packet definitions are Rust structs with hand-written `Read`/`Write` implementations. The
alternative, a derive macro, hides the wire layout of a format we must match exactly; the
protocol is small enough that explicit code wins. Fixture tests generated from the archived
`minecraft-data` 1.8 definitions pin every packet's byte layout.

## 9. World model

- A chunk is 16x16x128 blocks: eight 16x16x16 sections, matching the wire format.
- A section stores blocks as `u16` (id and metadata packed exactly as received), plus block-light and sky-light nibble arrays. Storage matches the wire format, which keeps parsing, light updates, and serialization trivial and avoids a translation layer.
- A separate `BlockRegistry` maps ids and metadata to block properties: name, solidity, opacity, light emission, tinting kind, render pass, and the model reference. It is built from the jar's blockstates and models JSON, so a resource change does not need code changes.
- `LightEngine` implements vanilla propagation: 15 levels, decrement by one per step, BFS with incremental updates for block changes. Height maps track skylight column state.
- Entities store the last three server positions for vanilla interpolation, plus metadata, equipment, and velocity.
- Weather, time, difficulty, and scoreboard live in a world state struct updated from packets.

## 10. Renderer

wgpu, one surface, one pipeline per pass. All draw data arrives as `DrawList` values built by
`oxide-game`; the renderer never inspects world structures.

| Pass | Contents |
| --- | --- |
| Sky | Sky gradient, sun, moon, stars, void fog |
| Clouds | Flat cloud layer at y=128, vanilla texture and scroll speed |
| Terrain | Three queues: opaque, cutout with alpha test, translucent sorted back to front |
| Entities | Models built from box geometry with per-part transforms |
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
- Fog modes and distances per the vanilla formulas: linear default from 0.75 of the far plane, exponential water, exponential-squared lava.
- Camera: vanilla FOV formula including the sprint and water modifiers, vanilla mouse sensitivity formula, view bobbing, and hurt-camera roll.

Performance techniques, all of which preserve the vanilla image: frustum culling per
section, rayon-parallel meshing, one draw call per section queue rather than per face,
persistent vertex buffers, and optional multithreaded chunk upload.

## 11. Game logic and GUI

- Physics: vanilla constants and order of operations — walk 4.317, sprint 5.612, jump height 1.2522 blocks, gravity, drag, liquid movement, ladders, sneaking edge protection.
- Prediction: the client applies local movement immediately, then reconciles when the server sends a position update, exactly as vanilla does.
- Interaction: 4.5-block block reach, break progress and tool timing per block hardness, placement rules including collision checks, block outline, and crack overlay.
- Input: vanilla default keybinds, mouse capture, sensitivity formula, and F3 debug output with the vanilla field set.
- Screens for v1: main menu, server list with ping, pause, options (the 1.8 option set), death, disconnect, chat, inventory, crafting table, chest, furnace, anvil, enchantment table, and the HUD. Options that are meaningless for a native renderer, such as Advanced OpenGL, remain visible with their 1.8 layout and act as documented no-ops.
- Sound: Ogg Vorbis playback with vanilla categories, pitch randomisation, and 3D attenuation.

## 12. Testing and verification

| Layer | Method |
| --- | --- |
| Codecs | Unit tests with golden byte fixtures generated from the archived `minecraft-data` 1.8 definitions, plus round-trip encode and decode |
| World | Unit tests for chunk parse, light propagation against small hand-built worlds, and inventory semantics |
| Physics | Tests against recorded vanilla values: speeds, jump arcs, step-up, and collision cases |
| End-to-end | Replay harness: record a raw server byte stream once, replay it offline in tests, and assert world, entity, and HUD state with no panics |
| Live | Connect to the local 1.8.9 test server, and compare packet sequence and behaviour against a vanilla client logged into the same server |
| Visual | The 57 side-by-side screenshot tests from the parity report, run against the vanilla rig, diffed with a tolerance threshold |
| Performance | Criterion benchmarks for chunk decode, mesh build, and light propagation; a frame-rate harness that runs both clients on the same world and records the comparison in `docs/perf.md` |
| Stability | Soak runs and a malformed-packet fuzz suite |

Continuous integration: formatting, clippy with warnings denied, tests, release build, and a
license check that fails on any dependency incompatible with GPL-3.0.

## 13. Milestones

Each milestone ends with a commit, a `docs/STATE.md` update, and a tag.

| Milestone | Contents | Exit criteria |
| --- | --- | --- |
| **M0 Foundations** | Workspace, CI, logging, error types, launcher `fetch`, jar extraction, blank wgpu window | CI green; `oxide-launcher fetch` produces a fully verified store; window opens with an FPS counter |
| **M1 Bytes to world** | Protocol framing, handshake, status ping, offline login, compression, keepalive, Join Game, chunk parsing, world store, untextured terrain | Connect to the test server and see correct world geometry with block colours and an F3-style overlay |
| **M2 Textured terrain** | Atlas, block models, biome tint, light from nibbles, fog, sky, frustum culling, parallel meshing; record the performance baseline | Screenshot shows the same terrain as vanilla at the same position; baseline numbers in `docs/perf.md` |
| **M3 Player** | Physics, input, camera, raycast, break and place with prediction, crack overlay, movement packets, view bobbing | Walk the world and it feels like vanilla; edits stay in sync with the server and a vanilla client sees them |
| **M4 Entities and chat** | Entity spawn, movement, metadata, skins, mob box models, interpolation, chat with formatting and click events, tab list, scoreboard | Two clients, one vanilla and one ours, see each other and chat correctly |
| **M5 HUD and inventory** | Hotbar, health, hunger, armour, experience, air, item icons, tooltips, inventory and container screens with drag and split semantics | Inventory round-trip matches the vanilla client screen for screen |
| **M6 Sound, particles, weather, screens** | Sound engine, particles, weather, sky and stars, overlays, main menu, server list, pause, options, death, disconnect, keybinds | The implemented screens pass their side-by-side parity checks |
| **M7 Microsoft auth** | Device code flow, Xbox Live and XSTS chain, entitlement checks, keyring token storage, encrypted session, online-mode connection | Join an online-mode server with a real account |
| **M8 Launcher and packaging** | Launcher screens, profile management, integrity verify, desktop file, tarball and AppImage, AUR package | A fresh machine installs and plays from the published package |
| **M9 Hardening and performance** | Culling and meshing tuning, memory budget, 32-chunk render distance, soak and fuzz runs, crash reporting | Performance targets met, soak clean, documentation complete |

## 14. Risks and mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Copying from a non-permissive source by accident | License contamination | `NOTICE` records every adapted file; review each adaptation; cargo-deny plus a review checklist |
| Microsoft changes the auth or session API | M7 blocked | Auth work is isolated in `oxide-launcher` and `oxide-assets`; Azalea's MIT implementation is a current reference |
| Anti-cheat flags a non-vanilla client | Cannot play on some public servers | v1 targets vanilla-compatible behavior, and documents this limit; no guarantees |
| Scope creep toward singleplayer | v1 slips | Section 2 keeps it out; the integrated server is a separate post-v1 project |
| Fidelity drifts as features land | "1-to-1" quietly fails | The parity checklist is run at every milestone that touches rendering |
| Mojang asset terms change | Distribution path breaks | Assets are fetched at runtime only, never bundled, so a policy change needs a launcher update, not a redesign |

## 15. Repository conventions and context handoff

- Conventional Commits; trunk-based on `main`; a tag per milestone; `CHANGELOG.md` updated per milestone.
- `docs/STATE.md` carries the live project state: current milestone, completed work, next actions, open questions, environment facts, and exact commands. It is updated before every context handoff and every long pause.
- `docs/handoff/` holds dated handoff prompts, each self-contained enough to start a fresh context with no compaction.
- Long-running work is delegated to subagents; their reports land in `docs/research/` and `refs/`.
- `refs/` and `vanilla/` are gitignored: they hold upstream clones and the Mojang jar, none of which are redistributed.

## 16. Deferred items with resolution rules

| Item | Resolution rule |
| --- | --- |
| Azure application id for device code login | At M7: the owner registers a free Azure application. If declined, the owner explicitly approves a documented alternative |
| Windows and macOS packaging | After v1 ships on Linux; the code must compile and run cross-platform from M0, verified by CI cross-builds |
| Singleplayer and the integrated server | A separate project after v1, with its own spec |
| Other Minecraft versions | One protocol crate per version; the renderer and world layers must not change |
| Resource packs | Out of scope until fidelity work is complete |

## Appendix A: planned dependencies

| Purpose | Crate |
| --- | --- |
| Windowing and input | `winit` |
| Rendering | `wgpu` |
| Math | `glam` |
| Compression | `flate2` with the zlib-ng backend (needs a C toolchain, present on this machine; verified at M0) |
| AES, RSA, SHA-1 | `aes`, `cfb8`, `rsa`, `x509-cert`, `sha1` |
| Random | `rand` |
| Zip reading | `zip` |
| PNG decoding | `png` |
| Ogg Vorbis decoding | `lewton` |
| Audio output | `cpal` |
| Thread pool | `rayon` |
| Channels, atomics | `crossbeam-channel`, `arc-swap` |
| HTTP in the launcher | `ureq` with `rustls` |
| CLI parsing | `clap` |
| Logging | `tracing`, `tracing-subscriber` |
| Keyring (M7) | `keyring` |
| Benchmarks | `criterion` |

All are permissive licenses, compatible with GPL-3.0.

## Appendix B: research documents

| Document | Contents |
| --- | --- |
| `docs/research/protocol-47-reference.md` | Lifecycle, all packet tables, chunk encoding, lighting, Anvil, entity metadata, pitfalls |
| `docs/research/render-parity-survey.md` | Asset split, atlas, FOV and physics formulas, particle list, screen inventory, 57 acceptance tests |
| `docs/research/launcher-assets-auth-survey.md` | piston-meta chain with verified hashes, jar contents, Microsoft auth chain, legal notes |
| `docs/research/rustcraft-survey.md` | Reference-only survey; license analysed in detail |
| `docs/research/stevenarella-azalea-survey.md` | Reusable MIT/Apache components and their limits |
