# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
