# Survey: Stevenarella and Azalea as references for a new Rust MC JE 1.8.9 client (protocol 47)

Scope: reconnaissance only. Both repositories were cloned `--depth 1` into
`/home/lucy/Desktop/Software/Projects/mc-rust-refs/repos/` and read, not built. No code was executed
from either project.

Target under evaluation: a new Rust client for Minecraft Java Edition **1.8.9 (protocol 47)** with a
**wgpu** renderer and a **custom launcher** (so: own auth + own token plumbing required).

| | Stevenarella | Azalea |
|---|---|---|
| Repo | `iceiix/stevenarella` (verified, 1540★/62 forks) | `azalea-rs/azalea` (verified, 777★/117 forks) |
| Language / edition | Rust 2021, `rust-version = "1.64.0"`, works on stable | Rust 2024, **nightly required** (`rust-toolchain.toml` = `nightly`, `#![feature(...)]`) |
| Licence | `MIT/Apache-2.0` (dual) | `MIT` |
| Protocol 47 (1.8.9) | Supported, first-class, in-tree id table | Not supported at all (latest MC only) |
| Renderer | OpenGL via `glow`/`glutin` | None (headless, no graphics deps) |
| Auth | Legacy Mojang Yggdrasil (dead endpoints) | Microsoft / Xbox Live device-code |

---

## 1. Stevenarella

### 1.1 Licence

`Cargo.toml` (root): `license = "MIT/Apache-2.0"`, `name = "stevenarella"`, `version = "0.0.1"`.
Two licence files are present: `LICENSE-MIT` ("Copyright (c) 2015 The steven Developers") and
`LICENSE-APACHE`. README confirms: "Dual-licensed MIT and ApacheV2". Dual MIT/Apache-2.0 is the most
permissive of the two projects and is compatible with a same-licence or MIT-only derivative as long
as attribution is kept.

### 1.2 Protocol layer design

Crates (all path dependencies, **none published on crates.io** — `GET https://crates.io/api/v1/crates/steven_protocol`
and `.../stevenarella` both return "not found"):

- `steven_protocol` — `protocol/` (packet definitions, wire format, compression, encryption, mojang auth client)
- `steven_blocks` — `blocks/` (block model, collision boxes, materials)
- `steven_shared` — `shared/` (`Position`, `Direction`, `Axis`)
- `steven_resources` — `resources/` (build-time embedded assets, `build.rs` → `include!(concat!(env!("OUT_DIR"), "/resources.rs"))`)
- `std_or_web` — wasm/native shim

**Packet definition style: macros over hand-written structs, not codegen.** `protocol/src/protocol/packet.rs`
(3609 lines, 141 KB) is one big `state_packets!` invocation (line 17) in which each packet is declared as a
`packet Name { field name: Type =, ... }` block. `state_packets!` is defined in
`protocol/src/protocol/mod.rs:64` and expands to: a `Packet` enum per `(state, direction)`, an
`internal_ids` module with one constant per packet (built by the `create_ids!` macro in
`protocol/src/protocol/macros.rs`), plus `read`/`write`/`packet_id(&self, version: i32)` impls.
Fields may be conditional on other fields via `when(|p: &ChunkData_Biomes3D_VarInt| p.new)`.

**Version differences are handled by internal-id translation tables.** `protocol/src/protocol/versions/`
contains 22 per-version files (`v1_7_10.rs` … `v1_18_2.rs`, plus snapshots `v15w39c`, `v18w50a`, `v19w02a`).
Each file is a `protocol_packet_ids!(state Direction { 0xNN => InternalName })` table
(`protocol/src/protocol/versions/v1_8_9.rs`). `protocol/src/protocol/versions.rs` maps
`"1.8.9" => 47` (line 58) and resolves the internal id with
`translate_internal_packet_id_for_version(version, state, dir, id, to_internal)`. Supported versions are
listed as `SUPPORTED_PROTOCOLS: [i32; 27] = [758, …, 74, 47, 5]` at `protocol/src/protocol/mod.rs:46`.
README states the design intent explicitly: multi-protocol so client dev is not in lock-step with server
versions, "Support for older protocols will not be dropped as newer protocols are added."

**What exists specifically for protocol 47:**

- Full packet-id tables: `protocol/src/protocol/versions/v1_8_9.rs` — serverbound play `0x00..0x19`
  (`0x00 => KeepAliveServerbound_VarInt`, `0x01 => ChatMessage`, `0x04 => PlayerPosition`,
  `0x08 => PlayerBlockPlacement_u8_Item`, …) and clientbound play `0x00..0x3d`
  (`0x00 => KeepAliveClientbound_VarInt`, `0x01 => JoinGame_i8`, `0x08 => TeleportPlayer_NoConfirm`,
  `0x21 => ChunkData_NoEntities_u16`, `0x26 => ChunkDataBulk`, `0x22 => MultiBlockChange_VarInt`,
  `0x23 => BlockChange_VarInt`, `0x20 => EntityProperties_VarInt`, …), plus handshake `0x00 => Handshake`
  and the login/status states. This is a complete, reviewable 1.8.9 id catalogue.
- Packet structs for those ids live in `protocol/src/protocol/packet.rs` (e.g. `ChunkData_NoEntities_u16`
  with `chunk_x/z`, `new: bool`, `bitmask: u16`, `data: LenPrefixedBytes<VarInt>`; `ChunkDataBulk` with
  `skylight: bool`, `chunk_meta: LenPrefixed<VarInt, ChunkMeta>`, `chunk_data: Vec<u8>` — `ChunkMeta` at
  packet.rs:2561). Structs are the *internal/generic* shape shared across versions, so even the 1.8-relevant
  ones carry fields for other versions; you would use them as a field-layout reference rather than a drop-in 1.8 API.
- Packet handling for 1.8 in the client: `src/server/mod.rs:610` (`ChunkData_NoEntities_u16 => on_chunk_data_no_entities_u16`),
  handler at `src/server/mod.rs:2150`.
- Chunk-data parsing for 1.8: `on_chunk_data_no_entities_u16` → `World::load_chunks18`
  (`src/world/mod.rs:612`) → `load_chunk18` (`src/world/mod.rs:660`). That function reads the classic
  1.8 layout: per section (16 of them, driven by the `u16` bitmask) 4096 × `u16` little-endian block ids,
  then per-section block-light byte arrays, then per-section sky-light arrays, then `16*16` biome bytes when
  `new`, and finally recomputes the heightmap (`Chunk::calculate_heightmap`, `src/world/mod.rs:1345`).
- Caveat on tests: `test/` contains chunk fixtures only for 1.12.2–1.18.2
  (`test/chunk_1.12.2.bin`, `test/chunk_1.18.2.bin`, …); there is **no 1.8 fixture**, and the in-file
  chunk tests (`src/world/mod.rs` tests, ~line 1700) all target ≥1.12.2. The 1.8 path is exercised only by
  live play.

### 1.3 World / chunk model

- `World` (`src/world/mod.rs:47`) owns `chunks: HashMap<CPos, Chunk>` (plus a `VanillaIDMap` per protocol
  version, block-entity action queue, light-update bookkeeping).
- `Chunk` (`src/world/mod.rs:1319`): `position`, `sections: HashMap<i32, Section>`, `biomes: [u8; 16*16]`,
  `heightmap: [u8; 16*16]`, `block_entities: HashMap<Position, ecs::Entity>`.
- `Section` (`src/world/mod.rs:1475`): `cull_info`, **`render_buffer: render::ChunkBuffer`**,
  `blocks: storage::BlockStorage`, `block_light: nibble::Array`, `sky_light: nibble::Array`, `dirty`,
  `building`.
- Block storage is palette-compressed: `BlockStorage` (`src/world/storage.rs`) keeps a `bit::Map` of
  palette indices plus `block_map`/`rev_block_map` (FNV-hashed), starting at 4 bits/entry and resizing.
- Block state type is a **rich typed enum**, not a numeric id: `steven_blocks::Block`
  (`blocks/src/lib.rs`, 299 KB, macro-generated variants with per-block properties), with
  `Material { collidable, ... }` (`blocks/src/material.rs`) and
  `Block::get_collision_boxes() -> Vec<Aabb3<f64>>` (`blocks/src/lib.rs:261`, default = unit cube).
  Vanilla-id ↔ `Block` mapping is per protocol version (`VanillaIDMap::by_vanilla_id`,
  `blocks/src/lib.rs:59`); the crate is hand-maintained macro-generated Rust (no in-repo generator; only a
  runtime probe `blocks/src/bin/dump_block.rs`), which is a maintenance smell for our purposes.
- Light **is** stored (4-bit arrays per section, `protocol/src/types/nibble.rs`), so the model already
  carries what a renderer needs.
- GPU suitability: partially. The mesher exists and is threaded — `src/chunk_builder.rs` with
  `NUM_WORKERS = 8`, meshing dirty sections into `render::ChunkBuffer` GL buffers — and sections carry
  `CullInfo`. But the world type is **coupled to the renderer**: `Section` owns a `render::ChunkBuffer`,
  `World::compute_render_list` (`src/world/mod.rs:352`) takes `&mut render::Renderer`, and
  `src/world/mod.rs` references `render` 28 times. Reusing `World` for a wgpu renderer means either
  keeping a stub `ChunkBuffer` type or patching `Section`/`compute_render_list` out. It is a client, not a
  bot, so sections are dirtied/meshed with rendering in mind rather than for pathfinding.

### 1.4 Physics / collision

Present, and written for a first-person player (not just bots):

- `check_collisions(world, position, last_position, bounds) -> (Aabb3, bool)` at `src/entity/player.rs:781`.
  It expands the entity AABB by 1 block on each axis, walks candidate blocks, skips non-collidable material,
  and for each block collision box resolves penetration with `move_out_of`.
- `trait Collidable { collides, move_out_of }` at `src/entity/player.rs:819` with the `Aabb3<f64>` impl at
  `:824` — axis-separated penetration resolution with `0.0001` epsilon, driven by the movement direction.
  This is a **discrete resolve, not a swept/continuous solver** (no vanilla-style ordered y→step→x→z sweep,
  no step-up), which is the main fidelity gap for a 1.8-accurate player.
- Gravity / velocity for non-player entities: `ApplyGravity` system at `src/entity/systems.rs:50`
  (`-0.03`/tick, clamped to `-0.3`); the local player handles its own physics (`MovementHandler`,
  `src/entity/player.rs:590+`), including input→velocity mapping (`calculate_movement`, `:553`),
  double-tap-jump flight toggle, on-ground detection via a probe AABB at `src/entity/player.rs:766`.
- Dependencies: `collision = "0.20.1"` (AABB/frustum crate) in the root `Cargo.toml`; block collision
  geometry comes from `steven_blocks`.
- Applicability: the *structure* (AABB expand → candidate blocks → resolve) transfers directly to our
  player, but the code is ~100 lines of ad-hoc logic and would need rewriting against 1.8 movement
  constants and vanilla collision ordering for fidelity.

### 1.5 Rendering

- OpenGL, via `glow = "0.11.2"` + `glutin = "0.29.0"` (root `Cargo.toml`), wrapped by `src/gl/mod.rs`
  (970 lines, `use glow as gl`). There is no `glium` dependency any more despite older mentions.
- `src/render/mod.rs` (1500+ lines) holds `Camera`, `Renderer`, shader/atlas/cloud/UI managers; chunk
  meshing in `src/chunk_builder.rs`; block/item models in `src/model/`, `src/render/model.rs`.
- wasm32 target exists (`www/`, `web-sys`, `console_error_panic_hook`), and CI builds Windows/Linux/macOS
  (`.github/workflows/build.yaml`).
- Ripping out rendering and keeping protocol/world/physics is **possible but not clean**: the coupling is
  `Section.render_buffer` + `World::compute_render_list(&mut Renderer)`. Protocol and `steven_blocks` are
  renderer-free; `World`/`Section` are not. Nothing in the codebase is wgpu-shaped — a wgpu renderer would
  mean replacing `src/gl`, `src/render`, and rewriting `chunk_builder.rs` as the mesher.

### 1.6 Auth

Legacy Mojang Yggdrasil only:

- `protocol/src/protocol/mojang.rs`: `LOGIN_URL = "https://authserver.mojang.com/authenticate"`,
  `REFRESH_URL`/`VALIDATE_URL` on `authserver.mojang.com`, `JOIN_URL = "https://sessionserver.mojang.com/session/minecraft/join"`,
  with `Profile { username, id, access_token }` and `join_server` (SHA-1 digest signing).
- `src/auth.rs` (63 lines) is only cvar plumbing (`cl_username`, `cl_uuid`, `AUTH_TOKEN`, `AUTH_CLIENT_TOKEN`,
  `AUTH_CLIENT_TOKEN`), and `src/screen/login.rs` is a username/password form calling `mojang::login`.
- A repo-wide grep for `microsoft|xbox|device_code|live.com` over `src/` and `protocol/src/` returns **no
  matches**: there is no Microsoft/Xbox authentication. The Yggdrasil endpoints have been dead since the
  Microsoft account migration, so authenticated play is effectively broken; the client is realistically
  offline-mode only today. **Nothing here is reusable for our launcher.**

### 1.7 Maintenance status

- Remote tip of `master` = `815ac883389a871a888ea4436ad1af192cfeca7b`, "Update dependencies (#786)",
  **2025-11-14** (confirmed with `git ls-remote origin refs/heads/master`; the local shallow HEAD matches).
- Only **2 commits in 2025**, both dependency bumps (`673bfbc` "Update wasm-bindgen to fix Rust
  incompatibility (#785)", 2025-11-10; `815ac88`, 2025-11-14). The last substantive feature commit is
  `ecf829c` "Update dependencies: reqwest, serde, serde_json (#755)", 2022-12-31.
- Commits per year over the last 650 commits: 2015:25, 2016:74, 2017:3, 2018:145, 2019:132, 2020:148,
  2021:80, 2022:41, 2025:2 → the project stalled after 2022.
- GitHub metadata: 1540★, 62 forks, 79 open issues, not archived; `pushed_at` 2026-09-19 is bot noise —
  the branch list is dominated by `dependabot/*` and `renovate/*` branches, and there are no releases at
  all. Feature branches for `1.19`, `1.16.1`, `1.13_assets`, `renderbuffer`, `async_tcp` exist but are not merged.
- Roadmap/positioning: README says "Don't expect it to go anywhere, just doing this for fun", promises that
  older protocols will not be dropped, and lists 1.8.9/protocol 47 as **✓ supported**. But the newest
  supported version is 758 (1.18.2), i.e. four years behind current Minecraft, and nothing has improved
  since 2022. 1.8.9 is a *live but frozen* target: the code paths exist and are complete enough to play on,
  but nobody is maintaining that path (no 1.8 chunk test fixture, no recent fixes).

### 1.8 Verdict — Stevenarella

Worth depending on / copying (MIT/Apache-2.0, so copying with attribution is fine):

- **`protocol/src/protocol/versions/v1_8_9.rs`** — the single most valuable artifact here: a complete,
  hand-verified 1.8.9 packet-id table for all four states and both directions. Use it as the source of
  truth to cross-check our own packet registry.
- **`protocol/src/protocol/packet.rs`** — field layouts for the 1.8-era packets (`ChunkData_NoEntities_u16`,
  `ChunkDataBulk`/`ChunkMeta`, `MultiBlockChange_VarInt`, `BlockChange_VarInt`, `JoinGame_i8`, movement
  packets, window/inventory packets, metadata `protocol/src/types/metadata.rs`). Treat as a specification
  to re-type, not as an API to import.
- **`src/world/mod.rs::load_chunk18` + `Section`/`BlockStorage`/`nibble::Array`** — a working decode of the
  1.8 chunk wire format (u16 ids → palette storage, then block light, sky light, biomes) and a sane
  in-memory section layout for a renderer.
- **`src/entity/player.rs` collision + `src/entity/systems.rs` gravity** — a starting skeleton for player
  AABB collision (expand → candidate blocks → resolve), to be re-implemented with proper sweep ordering.
- Nice-to-know structures: `shared/src/position.rs`; the `when(...)` conditional-field trick in
  `state_packets!` is a neat pattern, though we do not need multi-version support.

Not worth reusing:

- **Auth** (`src/auth.rs`, `protocol/src/protocol/mojang.rs`) — dead Yggdrasil endpoints, no Microsoft flow.
- **The renderer** (`src/gl`, `src/render`, `src/chunk_builder.rs`) — OpenGL/glow, mesh format built for
  that pipeline, and coupled into `World` via `Section.render_buffer`. We replace it with wgpu.
- **`steven_blocks`** (299 KB macro-generated `enum Block` with `Vec<Aabb3>` collision boxes and
  per-version `VanillaIDMap` tables) — allocation-heavy, no generator in-repo, and built for 1.7–1.18
  rather than precisely 1.8.9; a purpose-built 1.8 block table is smaller and faster.
- **The custom ECS** (`src/ecs/mod.rs`) and the UI/screen stack (`src/ui`, `src/screen`) — bot/UI plumbing
  with no relevance to a wgpu client.
- **The multi-protocol translation machinery** (`versions.rs`, `translate_internal_packet_id_for_version`,
  the generic `when(...)` packet shape) — valuable as a design study for how to isolate version differences,
  but we only need protocol 47 and generic structs cost clarity.

---

## 2. Azalea

### 2.1 Licence

MIT. Root `Cargo.toml` sets `license = "MIT"` in `[workspace.package]` and every member crate inherits via
`license.workspace = true` (e.g. `azalea-protocol/Cargo.toml`). `LICENSE.md` is the standard MIT text,
"Copyright (c) 2022 mat". MIT-only is compatible with reuse as long as the notice is preserved; note it is
*not* dual-licensed, unlike Stevenarella.

### 2.2 Protocol layer design

Crate names and layout: a workspace of `azalea-*` crates (`azalea`, `azalea-client`, `azalea-protocol`,
`azalea-buf`, `azalea-world`, `azalea-physics`, `azalea-block`, `azalea-entity`, `azalea-core`,
`azalea-registry`, `azalea-auth`, `azalea-chat`, `azalea-crypto`, `azalea-inventory`, `azalea-brigadier`,
`azalea-language`), plus macro crates (`azalea-protocol-macros`, `azalea-buf-macros`, …) and a
`codegen/` Python pipeline.

**Published on crates.io:** all `azalea-*` runtime crates are published, at
**`0.16.0+mc26.1`** (`azalea-protocol`, `-buf`, `-world`, `-physics`, `-client`, `-auth`, `-block`,
`-core`, `-entity`, `-registry`, … — crates.io API `max_version` = `0.16.0+mc26.1`, `updated_at`
2026-03-28). The git `main` is ahead of the registry: workspace version is `0.16.0+mc26.2` and the in-tree
constants are `PROTOCOL_VERSION: i32 = 776; VERSION_NAME: &str = "26.2";`
(`azalea-protocol/src/packets/mod.rs:11-12`). So depending on crates.io gets MC **26.1**; depending on git
main gets 26.2.

**Version handling: single-version by design.** `azalea-protocol/README.md` says: "A low-level crate for
sending and receiving Minecraft packets. **Only the latest Minecraft version is supported.**" The root
README lists "Supporting multiple versions of Minecraft at the same time" under **Non-goals** (pointing at
the separate `azalea-viaversion` ViaProxy plugin), and the version to codegen for is a single value read
from the README line `_Currently supported Minecraft version: `26.2`._` (`codegen/lib/code/version.py`).
There is **no protocol 47 / 1.8.9 support** anywhere: the packet set is the modern one (configuration state
exists, see `azalea-protocol/src/packets/config/`), and `azalea-block`'s block states are the modern
registry. Historically the repo *did* hold per-version branches (`1.18.2`, `1.19.2`, `1.20.1`, `1.21.1`, …
in `git ls-remote --heads`) — each MC version gets its own branch and old versions are abandoned, not
supported in parallel.

**Packet definition style: one file per packet + derive macros + Python codegen.**

- Each packet is a plain struct: `#[derive(AzBuf, ClientboundGamePacket)] pub struct ClientboundBlockUpdate
  { pub pos: BlockPos, pub block_state: BlockState }` (`azalea-protocol/src/packets/game/c_block_update.rs`).
  264 packet files exist under `azalea-protocol/src/packets/{handshake,status,login,config,game}/`.
- The per-state enums and the id→parser mapping are generated by `declare_state_packets!`
  (`azalea-protocol/azalea-protocol-macros/src/lib.rs:188`), invoked from generated module files, e.g.
  `azalea-protocol/src/packets/game/mod.rs` whose header reads "NOTE: This file is @generated automatically
  by codegen/packet.py. Don't edit it directly!" and which lists `Clientbound => [bundle_delimiter,
  add_entity, …]`. The derives (`ClientboundGamePacket`, …) come from
  `azalea-protocol/azalea-protocol-macros/src/lib.rs:86+`.
- The id tables come from **Mojang's own data reports for the current version**: `codegen/genpackets.py`
  calls `lib.extract.get_packets_report(version_id)` (`codegen/lib/extract.py:39`), which is the vanilla
  `reports/packets.json` for the version pinned in the README. That is a genuinely good idea worth copying
  for 1.8.9 (Mojang publishes the same report format for 1.8.9-era data via its data generator), but the
  generated code here is 26.2-shaped.
- Wire primitives live in `azalea-buf` (`AzBuf` trait + derive, VarInt/VarLong, `Vec`, `Option`, UUID,
  NBT/`simdnbt`); read/write/compression/encryption in `azalea-protocol/src/{read,write,connect}.rs`.

### 2.3 World / chunk model

`azalea-world`:

- `Chunk { pub sections: Box<[Section]>, pub heightmaps: HashMap<HeightmapKind, Heightmap> }`
  (`azalea-world/src/chunk/mod.rs:32`).
- `Section { block_count: u16, fluid_count: u16, states: PalettedContainer<BlockState>,
  biomes: PalettedContainer<Biome>, … }` (mod.rs:42-54).
- Paletted containers: `Palette<S> = SingleValue | Linear | Hashmap | Global`
  (`azalea-world/src/palette/mod.rs:16`) over packed `BitStorage` (`azalea-world/src/bit_storage.rs`).
- Block state type: `azalea_block::BlockState`, a newtype over `BlockStateIntegerRepr = u16`
  (`azalea-block/src/block_state.rs:20,30`) that derefs to `&dyn BlockTrait`, with the block/state data
  generated for the current version into `azalea-block/src/generated.rs` (212 KB, from
  `codegen/genblocks.py` → Mojang block-state reports + Pumpkin/burger data). Behaviour (hardness, shapes,
  sounds) is per-block trait impls, similar in spirit to Stevenarella's `enum Block` but registry-id shaped
  and resolved lazily.
- Chunk storage is abstracted: `trait ChunkStorageTrait { min_y, height, get, upsert, chunks }`
  (`azalea-world/src/chunk/storage.rs:22`) with `ChunkStorage` (boxed impl), `PartialChunkStorage`
  (loaded/unloaded staging) and `WeakChunkStorage`, handing out `Arc<RwLock<Chunk>>`. A `World` owns one
  and exposes `get_block_state(pos) -> Option<BlockState>` (`azalea-world/src/world.rs:115`).
- **Light data is not stored at all.** The packet type exists
  (`azalea-protocol/src/packets/game/c_light_update.rs`: sky/block y-masks, empty masks, and
  `sky_updates`/`block_updates` arrays), but the client handler is a documented no-op:
  `pub fn light_update(&mut self, _p: &ClientboundLightUpdate) {}`
  (`azalea-client/src/plugins/packet/game/mod.rs:551`), and a case-insensitive grep for "light" across
  `azalea-world/src/` returns nothing. Bots do not need light; a renderer does.
- GPU suitability: it is a **bot-oriented model** — chunks behind `Arc<RwLock<_>>`, pull-based
  `get_block_state`, no mesher, no dirty-section tracking, no light. The *data layout* (paletted sections +
  heightmaps) is exactly what a mesher wants, and we could read `Section::states` directly, but nothing in
  the crate is designed to feed a GPU: expect to add light, dirty tracking, and our own mesher — or, more
  realistically, to use this as a design reference while building our 1.8 layout (flat `u16` block arrays +
  separate nibble light arrays, per `1.8` semantics).

### 2.4 Physics / collision

Real, vanilla-derived physics at `azalea-physics` (no graphics, no bot assumptions):

- `azalea-physics/src/collision/mod.rs`: `collide()` (line 47) and `move_colliding()` (line 135) implement
  the vanilla ordering — y movement, then step-up attempts (`step_to_delta`, `directly_up_delta`,
  `target_movement` via repeated `collide_bounding_box` calls), then x/z with `movement.y != collided.y`
  bookkeeping. Files: `collision/aabb.rs`, `collision/entity_collisions.rs` (AABB queries against entities),
  `clip.rs` (raycast/`clip` for block+entity picking), `fluids.rs` (water/lava pushing, `in_water_state`),
  `travel.rs` (entity travel: friction, jumping, swim/dive, fall damage), `client_movement.rs`
  (`ClientMovementState`, `WalkDirection`, `SprintDirection` — the vanilla *client-side* movement state,
  including position remainder/delta accumulation).
- Wired to the client in `azalea-client/src/plugins/movement.rs` (uses `azalea_physics::{...}`,
  `ClientMovementState`, `PhysicsSystems`, sends movement packets when the delta is large enough),
  and `azalea-client/src/plugins/{attack,mining,loading}.rs` consume `PhysicsSystems`/`physics.velocity`.
- Applicability to a first-person player: yes in principle — this is the vanilla movement model, not a
  bot-only approximation, and `ClientMovementState` is explicitly the client-side input model, i.e. exactly
  what our player controller needs. Two caveats: (a) it is ECS-shaped (`PhysicsState`, `Input`,
  `ClientMovementState` components + systems on top of `bevy_ecs`, and it depends on `azalea-world`,
  `azalea-entity`, `azalea-registry`), which is a heavy pull-in; (b) it encodes **modern** movement
  constants and features (1.13+ friction/slipperiness, sprint swimming, 1.21-era changes), so a 1.8.9
  client would need the constants and edge cases re-derived from 1.8. The README also admits gaps: "some
  features like entity pushing and sprint swimming aren't implemented yet".

### 2.5 Rendering

**There is no rendering layer, and that is deliberate.**

- A grep for `wgpu|bevy_render|glium|glow|vulkan` in every `Cargo.toml` in the workspace returns no real
  hit (only `rustc-hash` matching the pattern). `azalea-client/Cargo.toml` depends on `bevy_app`,
  `bevy_ecs`, `bevy_tasks`, `bevy_time` (and `bevy_log` optionally) — never `bevy_render` — and its own
  description string is `"A headless Minecraft client."` (`azalea-client/Cargo.toml`).
- Root README "Non-goals" lists **Graphics** with a note pointing to a third-party project
  (`urisinger/azalea-graphics`), so a renderer is out of tree and unofficial.
- Consequence for us: the stack is already split the way we want — `azalea-protocol` (wire),
  `azalea-world` (chunks), `azalea-physics` (movement), `azalea-client` (connection/ECS glue) can each be
  taken alone, and `azalea-protocol` is usable standalone (its README: "A low-level crate for sending and
  receiving Minecraft packets. See `crate::connect::Connection` for usage"; features `connecting`,
  `online-mode`, `bevy_ecs`). Nothing needs to be torn out to add wgpu — but everything we take is
  version-pinned to modern MC, which is the real blocker.

### 2.6 Auth

Microsoft authentication is present and complete, in `azalea-auth` (published on crates.io, MIT):

- `azalea-auth/src/auth.rs` (19 KB): `AuthOpts { …, client_id: Option<&'a str> }`, `auth(cache_key, opts)`,
  `AuthResult`, `MinecraftTokenResponse`, `XboxLiveAuthResponse`, `GameOwnershipResponse`, `ProfileResponse`.
- Flow endpoints: Microsoft device-code (`https://login.live.com/oauth20_connect.srf`,
  `response_type=device_code`, auth.rs:333), token polling (`oauth20_token.srf`, :370 and :437),
  Xbox Live user auth (`https://user.auth.xboxlive.com/user/authenticate`, :487),
  XSTS authorize (`https://xsts.auth.xboxlive.com/xsts/authorize`, :525).
- Default `const CLIENT_ID: &str = "00000000441cc96b";` (auth.rs:285) — the legacy Minecraft client id —
  overridable per call via `AuthOpts::client_id`, which matters for a custom launcher (we would supply our
  own Azure application id).
- Supporting modules: `cache.rs` (token cache keyed by the vanilla launcher path via
  `minecraft_folder_path`), `certs.rs` (profile public-key signature verification), `sessionserver.rs`
  (join server / has-joined), `game_profile.rs`; feature gate `online-mode` (default on).
- Contrast with Stevenarella: Azalea is the only one of the two that can actually authenticate today.

### 2.7 Maintenance status

- Very active. Remote tip of `main` = `b65fa8cf1bb957976cefa926b9b500d44767d806`, "Migrate from
  `generic_const_exprs` to `generic_const_args` (#380)", **2026-09-07** (confirmed by `git ls-remote`).
  Recent history: `6b60d81` 2026-09-07, `153c90a` 2026-08-26 ("Fix Rust 1.100.0 warnings, force older
  trait solver").
- Commits per month (last 400 commits): 2025-08:37, 09:28, 10:35, 11:10, 12:67, 2026-01:77, 02:14, 03:37,
  04:1, 05:14, 06:10, 07:8, 08:1, 09:2 — continuous activity, several contributors (mat/mat-1,
  EightFactorial, Cmothersell, wbbradley).
- Releases (from `CHANGELOG.md`): `0.14.0+mc1.21.8` (2025-09-28), `0.15.0`, `0.16.0+mc26.1` (2026-03-27);
  changelog entries follow Keep a Changelog with explicit breaking-change notes and a standing warning:
  "Many parts of Azalea are still unfinished and will receive breaking changes in the future."
- Roadmap/goals (README): support everything a vanilla client can do, intuitive API, many bots, don't
  trigger anti-cheats, **support the latest Minecraft version**, be fast. Non-goals: multi-version support,
  graphics, Bedrock. There is a plugin ecosystem (`azalea-viaversion` for older servers, `azalea-hax`) and
  funding via GitHub Sponsors; maintenance is essentially one person plus contributors.
- Toolchain note: the workspace **requires nightly Rust** — `rust-toolchain.toml` sets `channel = "nightly"`,
  and crates use `#![feature(min_specialization)]` (`azalea-buf/src/lib.rs`),
  `#![feature(error_generic_member_access)]` (`azalea-protocol/src/lib.rs`), plus
  `generic_const_exprs`/`generic_const_args` migrations in recent PRs. Depending on these crates means
  building with nightly and riding upstream feature-gate churn.
- Protocol 47 status: **not a target at all**, live or legacy. 1.8-era connectivity for Azalea bots exists
  only through the separate ViaProxy-based plugin.

### 2.8 Verdict — Azalea

Worth depending on (crates.io `0.16.0+mc26.1`, MIT):

- **`azalea-auth`** — the most directly reusable artifact: Microsoft device-code auth, XSTS exchange,
  token caching, session-server join, with an overridable `client_id` (`AuthOpts`). Usable as a crates.io
  dependency for our launcher if its dependency weight and nightly toolchain are acceptable; otherwise it is
  the reference implementation to copy (MIT).
- **`azalea-buf`** — `AzBuf` derive + VarInt/VarLong/UUID/String/NBT primitives, small and version-agnostic.
  Reasonable to depend on, or to re-derive from (the derive macro is
  `azalea-buf/azalea-buf-macros`; note the nightly `min_specialization` feature).
- **`azalea-core`** — `Aabb` (`azalea-core/src/aabb.rs`), `position.rs` (`Vec3`, `BlockPos`, chunk coords),
  `cursor3d.rs`, `direction.rs`, `bitset.rs`: version-neutral math/geometry worth depending on or copying.

Worth studying, not depending on:

- **`azalea-physics`** — the best available Rust model of vanilla collision ordering (sweep/step-up/fluids)
  and of a client-side movement state (`client_movement.rs`, `travel.rs`, `clip.rs`). Read it to structure
  our player physics, then re-implement for 1.8 constants; depending on it drags in `bevy_ecs`,
  `azalea-world` (modern chunk format) and `azalea-registry`.
- **`azalea-protocol` + `codegen/`** — the per-packet-file + derive + `declare_state_packets!` pattern and,
  more importantly, the *codegen approach* of deriving packet ids from Mojang's own version-specific data
  reports (`codegen/lib/extract.py::get_packets_report`, `codegen/genpackets.py`). Copy the pipeline idea
  for 1.8.9; the generated code itself is 26.2-only.
- **`azalea-world` paletted containers / `bit_storage.rs` / `heightmap.rs`** — good prior art for
  palette-compressed storage, but 1.8 does not use palettes on the wire (flat `u16` arrays + separate light),
  so our section layout will differ.

Not worth reusing:

- **`azalea-protocol`'s actual packet definitions** — protocol 776 only, modern packet set
  (configuration state, chunk batches, data components), unusable for protocol 47 without a full rewrite of
  every struct; there is no version-parameterisation to hook into.
- **`azalea-block` generated data + `BlockState`** — modern block-state ids and shapes; our 1.8 block table
  must come from 1.8 data.
- **`azalea-client`** — bot/ECS-shaped (swarm, pathfinder, auto-reconnect, mining/attack plugins), headless,
  no dirty-section or meshing concept. Its packet-handler layer is also a stubbed shell for anything visual
  (e.g. `light_update` no-op).
- **`azalea-world` as our chunk model** — no light storage, `Arc<RwLock<Chunk>>` per chunk, modern section
  semantics; fine as a reference, wrong as a dependency for a 1.8 renderer.
- **The nightly-toolchain requirement / bevy dependency surface generally** — a deliberate cost to accept
  only if we adopt Azalea crates wholesale.

---

## 3. Cross-cutting conclusions for the spec

1. **Protocol 47 has exactly one usable Rust reference in the surveyed set: Stevenarella.** Its
   `versions/v1_8_9.rs` id table plus the packet structs in `packet.rs` and the 1.8 chunk decode in
   `World::load_chunk18` are the pieces to mine (MIT/Apache-2.0, attribution required). Azalea offers
   nothing for 1.8.9 and its multi-version story is explicitly a non-goal.
2. **Neither project gives us a wgpu renderer.** Stevenarella's renderer is OpenGL/glow and is entangled
   with `World` through `Section::render_buffer`; Azalea has no renderer at all. Either way the mesher and
   GPU layer are ours to write.
3. **Light is the sharpest divergence:** Stevenarella stores per-section nibble sky/block light (good
   precedent, matches 1.8 wire format); Azalea drops light on the floor (`light_update` is a no-op). For a
   1.8 renderer, follow Stevenarella's model.
4. **Physics: take the shape from Azalea, the 1.8 constants from vanilla.** Azalea's
   `collision/mod.rs` + `travel.rs` + `client_movement.rs` encode vanilla ordering properly (sweep, step-up,
   fluids) but for modern versions; Stevenarella's `check_collisions` is 1.8-era in spirit but a crude
   discrete resolve. A correct 1.8.9 player needs vanilla's ordered y→step→x→z sweep with 1.8 constants.
5. **Auth must be built fresh either way, with Azalea as the template.** Stevenarella's Yggdrasil code is
   dead; Azalea's `azalea-auth` (MIT) implements the modern Microsoft device-code → Xbox Live → XSTS →
   `sessionserver` join chain and accepts a custom `client_id`, which is what a custom launcher needs.
6. **Licence posture:** Stevenarella is MIT/Apache-2.0 dual (most flexible, safest to copy from);
   Azalea is MIT-only. Both permit a closed or open derivative with attribution; MIT/Apache-2.0 code
   copied into an MIT project is the least friction.
7. **Maintenance reality check:** Stevenarella is effectively abandoned (last real work 2022, two
   dependency commits in 2025) — treat it as a frozen, well-documented archaeology site. Azalea is
   healthy and fast-moving but requires nightly Rust and only ever targets the newest Minecraft, so it is a
   reference and a source of small, version-neutral crates (`azalea-buf`, `azalea-core`, `azalea-auth`),
   not a foundation to build a 1.8.9 client on.

### Evidence index (paths relative to each repo root)

| Claim | Stevenarella | Azalea |
|---|---|---|
| Licence | `Cargo.toml` (`license = "MIT/Apache-2.0"`), `LICENSE-MIT`, `LICENSE-APACHE` | `Cargo.toml` (`license = "MIT"`), `LICENSE.md` |
| Protocol crate | `protocol/` (`steven_protocol`) | `azalea-protocol/` (+ `azalea-protocol/azalea-protocol-macros/`) |
| Packet definition | `protocol/src/protocol/packet.rs` (`state_packets!`), `protocol/src/protocol/mod.rs:64,173`, `protocol/src/protocol/macros.rs` | `azalea-protocol/src/packets/*/*.rs` (264 files), `azalea-protocol/src/packets/game/mod.rs`, `azalea-protocol-macros/src/lib.rs:188` |
| Version handling | `protocol/src/protocol/versions.rs`, `versions/*.rs`, `SUPPORTED_PROTOCOLS` (`protocol/src/protocol/mod.rs:46`) | `azalea-protocol/README.md`, `azalea-protocol/src/packets/mod.rs:11-12`, `codegen/lib/code/version.py` |
| Protocol 47 ids | `protocol/src/protocol/versions/v1_8_9.rs` | none |
| 1.8 chunk decode | `src/world/mod.rs:612,660` (`load_chunks18`/`load_chunk18`), `src/server/mod.rs:2150` | none (modern sections only) |
| Chunk storage | `src/world/mod.rs:47,1319,1475`, `src/world/storage.rs`, `src/types/nibble.rs` | `azalea-world/src/chunk/mod.rs:32-54`, `azalea-world/src/palette/mod.rs:16`, `azalea-world/src/bit_storage.rs`, `azalea-world/src/chunk/storage.rs:22` |
| Block state type | `blocks/src/lib.rs` (`enum Block`, `get_collision_boxes` at :261), `blocks/src/material.rs` | `azalea-block/src/block_state.rs:20,30`, `azalea-block/src/generated.rs` |
| Light storage | `src/world/mod.rs:1481-1482` (`block_light`/`sky_light` nibble arrays) | not stored; `azalea-client/src/plugins/packet/game/mod.rs:551` no-op |
| Physics | `src/entity/player.rs:781,819` + `src/entity/systems.rs:50` | `azalea-physics/src/collision/mod.rs:47,135`, `travel.rs`, `client_movement.rs`, `clip.rs`, `fluids.rs` |
| Rendering | `src/gl/mod.rs`, `src/render/mod.rs`, `src/chunk_builder.rs`; `glow`/`glutin` in `Cargo.toml` | none; `azalea-client/Cargo.toml` (`"A headless Minecraft client."`), README non-goals |
| Auth | `protocol/src/protocol/mojang.rs` (Yggdrasil), `src/auth.rs`, `src/screen/login.rs` | `azalea-auth/src/auth.rs` (device code :333, XSTS :525, `CLIENT_ID` :285, `AuthOpts` :20) |
| Last commit | `815ac883` 2025-11-14 | `b65fa8cf` 2026-09-07 |
| crates.io | not published | `0.16.0+mc26.1` (published 2026-03-28); git main is `0.16.0+mc26.2` |
