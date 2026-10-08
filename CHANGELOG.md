# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Milestone M4: entities and chat

Delivered and tagged `m4` on 2026-10-07. The client is a participant among entities: the entity
spawn, metadata, movement and lifecycle packets; the entity table and the per-tick feed; skins
with the cape channel; the mob families (biped, quadruped, crawler, cube, arthropod and exotic)
with their walk, look and swing animations; the object entities (dropped and thrown items,
arrows, boats, minecarts, item frames, paintings, XP orbs); nametags with the source's distance
rule; and the HUD surfaces — chat with wrapping and click and hover events, the input field, the
tab list, the scoreboard sidebar with the below-name lines, and the boss bar — carried through a
live acceptance run whose every headline number was re-derived from the recorded artifacts; the
defects the run surfaced (a snow layer's fallback cube, the barrier's render kind, the scripted
click's cursor space, the sidebar's blended glyph run, the chat translation components, and the
barrier's translucent read) were fixed in the milestone's scoped fix rounds. The documented
classes and the operator's by-eye list are in `docs/STATE.md`.

### Added

- The entity packet set — spawns, metadata, movement, lifecycle and the object spawns — with the
  entity table, the per-tick feed and the session tracking.
- Skins: the fetch, cache and fallback rules, the cape channel and the part toggles' default.
- The entity draws: the player model, the biped/quadruped/crawler/cube/arthropod/exotic mob
  families, the object entities, and nametags with the source's scale, background and 64/32 range
  rules.
- The chat surfaces: the parser with translation components, wrapping, fade and scrollback; the
  input field with key routing, the send path and the click and hover event handling (the click
  proven end-to-end at the server).
- The HUD surfaces: the tab list (entries, heads, latency bars), the scoreboard sidebar with the
  red scores and the below-name lines, and the boss bar (the 1.8 single status).
- The acceptance machinery: the script-mode entity tick log and the scene scripts under
  `refs/m4-acceptance/` (local evidence, not shipped).

New dependencies: `base64` 0.23.1 (`oxide-assets`, the skin and cape property decode);
`serde_json` becomes a direct `oxide-game` dependency. The milestone's corrected documents ride
its commits.

### Milestone M3: the player

Delivered and tagged `m3` on 2026-10-04. The client is a participant in the world: the 20 Hz tick
loop and input path, the movement model with its environment rules, the collision view of the
world, gamemode-dependent reach, digging and placing with prediction, the crack overlay and block
outline, movement packets, death and respawn, and the full FOV camera with view bobbing and the
hurt roll — carried through a live acceptance run whose every headline number was re-derived from
the recorded artifacts; the defect the run surfaced (a mid-session game-mode change going
untracked) was fixed in the milestone's scoped fix round. The documented classes and the
operator's by-eye list are in `docs/STATE.md`.

### Added

- The tick loop, the keybind map and the scripted-input capture path (`--input-script`).
- The player movement model with the recorded test vectors — walk/sprint/jump, water and lava
  drag, ladders, sneaking edge protection and creative flight — and the collision view of the
  world.
- The movement packets and self-state (0x03–0x06, abilities), the block-change packets
  (0x22/0x23) with their light hooks, and block placement (0x08) with optimistic prediction and
  reconcile.
- Death, respawn and the interim death view; the dig machine with break progress and prediction;
  the aim raycast with gamemode-dependent reach.
- The camera's full FOV formula, view bobbing and the hurt roll; the crack overlay and the block
  outline.
- The live acceptance run (movement vectors, the creative leg, death/respawn, the serverbound
  spot-check, the latency proxy) with its recorded numbers and the vanilla-visible placement
  captures.

No new dependencies added; the milestone's corrected documents ride its commits.

### Milestone M2: textured terrain

Delivered and tagged `m2` on 2026-10-02. The client draws the world from the real 1.8.9 jar — the
atlas, block models, biome tints, the light engine, the sky, fog and clouds — with the parity metric
and the acceptance baseline recorded; the acceptance's documented residue and every caveat are in
`docs/STATE.md`.

### Added

- PNG texture loader, `.mcmeta` parser and `TextureSet`; the blockstate and model loader with the
  1.8 baker.
- The texture atlas: the client's mip chain and blend kernel, the sprite index, animation frames
  and the fallback sprite.
- The block behaviour table (73 covered ids) and the biome table with the tint path (the Perlin and
  `java.util.Random` ports, the colour-map lookup, the nine-sample average, and the swamp, mesa and
  roofed-forest overrides).
- The light engine — vanilla's sky and block light rules with the two-way recomputation and the
  mesher's query.
- The model-driven mesh core: the column snapshot, the model join with the world-position variant
  choice, atlas UVs, cullface, per-vertex light and the full ambient-occlusion path; the liquids,
  the Fast leaves rule and the three terrain layers.
- The atlas-textured terrain pipelines, the lightmap and brightness pipeline, the linear fog, and
  the non-sRGB colour-space policy (`docs/DIVERGENCES.md` entry 4 retired).
- The world clock, the sky pass (band, sun, moon phase, stars, celestial rotation) and the flat
  cloud layer.
- The rayon mesh pool with the generation-tracked mesh queue and the bounded end drain; the asset
  bootstrap, the jar font and the overlay's geometry cache.
- The parity metric (`scripts/parity-diff.py`), the acceptance baseline (`docs/perf.md`) and the
  sample-wall cells.

New dependencies: `png` 0.18.1 and `rayon` 1.12.0; `docs/DIVERGENCES.md` entry 4 is retired; the
milestone's corrected documents ride its commits.

### Milestone M1: bytes to world

Delivered and tagged `m1` on 2026-09-26. The client connects to the 1.8.9 rig server, speaks the
full connection lifecycle, parses and stores the world it receives, and draws block-coloured
terrain with an F3-style overlay; the acceptance run's evidence is recorded in `docs/STATE.md`.

### Added

- Buffered framed connection and the 1.8 compression handover, with golden-byte tests.
- Login-state and play-state packet codecs for protocol 47, including Client Settings, Keep-alive,
  Player Position And Look, Client Status and Plugin Message.
- Live capture of the rig's 1.8.9 server with its findings document and committed chunk fixtures;
  the capture answers the `0x26` question (the server does emit it) and fixes the
  `Set Compression`/`Login Success` ordering.
- Chunk column decoder for `0x21` and `0x26`, ground-truthed against the rig's saved world.
- Chunk store with the wire-to-store application rules (block, light and biome merges).
- Session state machine: offline login, the five connection obligations, world application, and the
  mesh deferral that keeps keep-alives answered under load.
- Block palette and face-culled section mesher with per-face brightness.
- Terrain vertex types and the camera (`glam`).
- Depth-tested wgpu terrain pipeline with a text overlay pass.
- Client `--server` wiring with the session thread and the F3-style debug overlay.
- Negative self-tests for the asset and crate-graph guards, run in CI before each guard's normal
  check.

## [0.0.0] — Milestone M0: foundations

Released 2026-09-23. The first tagged milestone. The repository has no releases to upgrade from;
this entry records what the milestone established.

### Added

- Eight-crate Cargo workspace with an enforced dependency graph.
- VarInt codec and length-prefixed framing with the 1.8 compression rules, both with golden-byte
  tests.
- Handshake, status ping, and the launcher CLI skeleton.
- Hash-verified atomic asset store.
- piston-meta metadata chain: version manifest, version document and asset index, each verified by
  hash.
- Full `fetch --verify` flow against the real distribution endpoints.
- Jar extraction with a manifest, refusing `.class` entries.
- wgpu window with an FPS counter and adapter logging.
- CI with format, lint, test, MSRV, portability, crate-graph, asset-guard, and licence jobs.
