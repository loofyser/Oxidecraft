# RustCraft — Technical Survey (reconnaissance for a new Rust 1.8.9 client, wgpu + custom launcher)

Survey date: 2026-09-22. All claims cite repository paths inside the shallow clone at
`/home/lucy/Desktop/Software/Projects/mc-rust-refs/repos/RustCraft`.

## 0. Repository identity / provenance

- **The URL originally given for RustCraft is wrong.** `https://github.com/Snowiiii/RustCraft` does not exist:
  `git clone` fails with `fatal: could not read Username for 'https://github.com'` (GitHub's
  404-as-credential-prompt behaviour) and `https://api.github.com/repos/Snowiiii/RustCraft` returns
  a JSON body with all fields `null` (404). GitHub's API for the account instead shows the owner's
  active project is `Snowiiii/Pumpkin` (a Rust Minecraft *server*).
- The real project is **`RustCraftMC/RustCraft-Public`** — description "Rust rewrite Minecraft with
  Java", language Rust, default branch `main`, 83 stars, 8 forks, 2 contributors, created
  2026-07-17T08:06:06Z, `pushed_at` 2026-08-11T07:19:41Z, `archived: false`, GitHub-classified
  licence **"Other" / NOASSERTION**.
- Clone: `git clone --depth 1 https://github.com/RustCraftMC/RustCraft-Public RustCraft` → HEAD
  `d1c861f972ffa2b48e8f90004ce32706892d39fa` ("docs: update public renderer documentation",
  2026-08-09 21:08:36 +0800). Tag `v1.0.0` points at the same commit; tag `v0.1.0` at `ae8c8cba`.
- This is a **snapshot repo** (created 2026-07-17, one release spike, then quiet). Treat it as a
  code sample, not a maintained upstream.

---

## 1. License

`LICENSE` (295 lines, 9 832 bytes) is a **custom, non-OSI licence**:

> "RustCraft Noncommercial Source-Available License / Version 1.0 / Copyright (c) 2026 RustCraft
> Contributors / All rights reserved except as expressly granted under this License." (`LICENSE:1-5`)

Substantive terms, with line evidence:

- **§2 GRANT OF PERMISSION** (`LICENSE:36-54`): non-exclusive, worldwide, royalty-free,
  non-transferable licence to (a) view/study the source, (b) use it for "personal, educational,
  research, experimental, and other noncommercial purposes", (c) copy noncommercially,
  (d) modify for personal/noncommercial purposes, (e) keep private modified versions, (f) distribute
  only when *all* requirements of the licence are satisfied.
- **§3 NONCOMMERCIAL USE ONLY** (`LICENSE:56-88`): explicit bar on selling, paid access/downloads,
  inclusion in paid products/services, paid hosting, revenue generation, and use in a proprietary
  commercial product without separate written permission. Commercial use requires separate written
  permission.
- **§5 SOURCE CODE REQUIREMENT FOR DISTRIBUTION** (`LICENSE:106-135`): if a modified version is
  distributed to any third party, the *complete Corresponding Source of that exact distributed
  version* must be published publicly, free, without approval, identifying modifications, retaining
  copyright notices and an unmodified copy of the licence. Binary-only distribution is prohibited.
- **§6 NO CLOSED-SOURCE DISTRIBUTION** (`LICENSE:138-152`).
- **§7/§8** (`LICENSE:155-181`): licence + attribution must be preserved; modified versions must not
  claim to be official RustCraft; "Based on the RustCraft project." is the allowed descriptive use.
- **§9 THIRD-PARTY CONTENT** (`LICENSE:184-220`): Minecraft textures, sounds, language files,
  resource indexes, asset objects, skins, models, game data, fonts and trademarks are **not**
  licensed here — the user must obtain them separately. Project disclaims affiliation with Mojang.
- **§10** (`LICENSE:223-240`): the Licensor reserves the right to grant separate (commercial)
  licences — i.e. it is effectively a dual-licence model with the free tier being noncommercial.
- **§11-§15**: contributions fall under the same licence, no warranty, automatic termination on
  material violation.

Per-file headers: **none.** `grep -rl 'SPDX-License-Identifier' --include='*.rs'` returns 0 matches,
so the licence is repo-level only. Also, none of the four `Cargo.toml` manifests contains a
`license`, `description` or `repository` field (`crates/game/Cargo.toml`,
`crates/renderer/Cargo.toml`, `crates/renderer/backend/vulkan/Cargo.toml`,
`crates/renderer/backend/dx12/Cargo.toml`) — which is why crates.io would reject them anyway
(§2 below).

**Bottom line for the new project:** this is not MIT/Apache. Any code reuse drives the derived
work into a noncommercial, source-available, attributing posture.

---

## 2. Repo layout, crates, LOC, publication status

Root `Cargo.toml` (444 bytes):

```toml
[workspace]
resolver = "2"
members = ["crates/game", "crates/renderer",
           "crates/renderer/backend/dx12", "crates/renderer/backend/vulkan"]
default-members = ["crates/game"]
```

Profiles: dev `opt-level = 2`; release `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`,
`panic = "abort"`, `strip = true`.

| Crate (package name) | Version | Lib/bin name | Path | Deps of note |
|---|---|---|---|---|
| `rustcraft-game` | 1.0.0 | bin `rustcraft` (`src/main.rs`), `build.rs`, `default-run = "rustcraft"` | `crates/game/` | winit 0.30, mlua 0.11 (lua54, vendored, serialize), rodio 0.19, fontdue 0.9, image 0.25, flate2 1, zip 2, reqwest 0.12 (blocking, json), aes 0.8, rsa 0.9, sha1/sha2, rayon 1.10, serde/serde_json, gilrs 0.11 |
| `rustcraft-renderer` | 0.1.0 | lib `renderer` | `crates/renderer/` | no external deps |
| `rustcraft-renderer-vulkan` | 0.1.0 | lib `renderer_vulkan` | `crates/renderer/backend/vulkan/` | ash 0.38 (`loaded`), ash-window 0.13, gpu-allocator 0.28, raw-window-handle 0.6, shaderc 0.10 |
| `rustcraft-renderer-dx12` | 0.1.0 | lib `renderer_dx12` | `crates/renderer/backend/dx12/` | windows 0.62, spirv_cross 0.23.1 (`hlsl`), shaderc 0.10; Windows-only via `[target.'cfg(windows)']` |

Rough LOC (`find -name '*.rs' | xargs wc -l`):

- `crates/game` — 251 files, **99 918** lines
- `crates/renderer` — 38 files, **14 075** lines (all backends included)
- whole workspace — 289 files, **113 993** lines
- Inside `crates/game`: `net/` 4 746, `world/` 12 613, `scripting/` 10 810 (27 files)

**Published on crates.io? No.** `GET https://crates.io/api/v1/crates/{rustcraft-game,
rustcraft-renderer, rustcraft-renderer-vulkan, rustcraft-renderer-dx12}` → **HTTP 404 for all four**.
Combined with the missing `license`/`description` metadata, `cargo add rustcraft-renderer` is not
possible; only a path or `git = "..."` dependency, which drags the noncommercial licence into the
downstream crate graph.

`Cargo.lock` resolves **555 packages**. `wgpu` is **not** in the tree (`grep -c 'name = "wgpu"' →
0`) — the renderer is hand-written on `ash`/DX12.

---

## 3. Protocol 47 implementation

**Location:** `crates/game/src/net/` (`protocol.rs` 530, `packet.rs` 535,
`packet/clientbound/play.rs` 1 035, `packet/clientbound/login.rs` 36, `packet/serverbound.rs` 295,
`connection.rs` 494, `nbt.rs` 303, `slot.rs` 195, `metadata.rs` 76, `dynamic_packet.rs` 461,
`player_list.rs` 103, `translation/` 600+). Send helpers live in
`crates/game/src/client/network/outbound.rs` (705) and per-domain handlers in
`client/network/{session,world_packets,entity,inventory,effects}.rs`.

**Serialisation approach: hand-rolled, no serde derives and no macros.** A `PacketBuffer` type in
`net/protocol.rs` exposes `read_varint / read_string / read_varint_count / read_bytes / read_int_count
/ write_*` primitives; packets are plain Rust enums written with explicit field-by-field reads and
writes. Evidence: `net/protocol.rs:9` `pub const PROTOCOL_VERSION: i32 = 47;`,
`net/protocol.rs:31` `encode_varint`, `:47` `decode_varint`, `:66` `read_packet`, `:71`
`read_packet_with_compression` (zlib via `flate2`, `ZlibDecoder`/`ZlibEncoder`), `:11`
`MAX_PACKET_BYTES = (1 << 21) - 1`, `:14` `MAX_STRING_BYTES = 32_767 * 4`, `:15`
`MAX_PACKET_COLLECTION_ITEMS = 65_536`. There are **no `macro_rules!`** anywhere in `net/` and the
only serde use in `net/` is `net/dynamic_packet.rs` (the plugin-channel/dynamic-packet subsystem,
`:3`), plus `serde_json` for asset/cache files.

Framing: `VarInt length + VarInt packet id + payload`, big-endian (`net/protocol.rs:3-4`), with the
post-login compression wrapper validated (decompressed-length mismatch and below-threshold cases are
rejected, `:86-113`). Encryption: `EncryptedReader`/`EncryptedWriter` implement `Read`/`Write`
(`net/connection.rs:334`, `:353`) using the `aes` + `rsa` + `sha1` deps, so online-mode servers are
supported.

**Clientbound play-state coverage — 73 of 74 vanilla 1.8.9 packets (IDs 0x00–0x48).** Parsed in
`net/packet/clientbound/play.rs` (73 `0xNN =>` arms; routed from
`net/packet/clientbound/mod.rs:12-19`, unknown IDs fall through to `ClientboundPacket::Unknown { id }`
at `play.rs` tail):

```
0x00 KeepAlive            0x01 JoinGame             0x02 ChatMessage          0x03 TimeUpdate
0x04 EntityEquipment      0x05 SpawnPosition        0x06 UpdateHealth         0x07 Respawn
0x08 PlayerPositionAndLook 0x09 HeldItemChange       0x0A UseBed               0x0B Animation
0x0C SpawnPlayer*         0x0D CollectItem          0x0E SpawnObject*         0x0F SpawnMob*
0x10 SpawnPainting        0x11 ExperienceOrbSpawn    0x12 EntityVelocity       0x13 DestroyEntities
0x14 Entity               0x15 EntityMove           0x16 EntityLook           0x17 EntityMoveLook
0x18 EntityTeleport       0x19 EntityHeadLook       0x1A EntityStatus         0x1B AttachEntity
0x1C EntityMetadata       0x1D EntityEffect         0x1E RemoveEntityEffect    0x1F SetExperience
0x20 EntityProperties     0x21 ChunkData            0x22 MultiBlockChange      0x23 BlockChange
0x24 BlockAction          0x25 BlockBreakAnimation  0x26 MapChunkBulk          0x27 Explosion
0x28 Effect               0x29 SoundEffect          0x2A Particle             0x2B ChangeGameState
0x2C SpawnGlobalEntity    0x2D OpenWindow           0x2E CloseWindow          0x2F SetSlot
0x30 WindowItems          0x31 WindowProperty       0x32 ConfirmTransaction    0x33 UpdateSign
0x34 MapData              0x35 UpdateBlockEntity    0x36 SignEditorOpen       0x37 Statistics
0x38 PlayerListItem       0x39 PlayerAbilities      0x3A TabComplete          0x3B ScoreboardObjective
0x3C UpdateScore          0x3D DisplayScoreboard    0x3E Teams*               0x3F PluginMessage
0x40 Disconnect           0x41 ServerDifficulty     0x42 CombatEvent*          0x43 Camera
0x44 WorldBorder*         0x45 Title*               0x46 SetCompression        0x47 PlayerListHeaderFooter
0x48 ResourcePackSend
```

`*` = handled by a dedicated helper parser rather than an inline arm: `parse_teams`, `parse_title`,
`parse_world_border`, `parse_combat_event`, `parse_entity_properties`; `0x0C/0x0E/0x0F` all collapse
into the single `ClientboundPacket::EntitySpawn` variant.

**Missing:** exactly one — **`0x49 UpdateEntityNBT`**.

**Login state** (`net/packet/clientbound/login.rs`): `0x00 Disconnect`, `0x01 EncryptionRequest`,
`0x02 LoginSuccess`, `0x03 SetCompression` — complete.

**Serverbound** (`net/packet/serverbound.rs`, free functions returning raw payloads; packet IDs are
bound at the call sites in `client/network/outbound.rs`, e.g. `send_play_packet(0x01, …)` for chat,
`0x0B`, `0x0D`, `0x0F`, `0x10`, `0x11`, `0x12`, `0x14`, `0x15`): keep_alive, player_position,
player_look, player_on_ground, player_position_and_look, chat_message, held_item_change,
click_window, confirm_transaction, enchant_item, use_entity, use_entity_interact_at, entity_action,
player_input (0x0C riding input), client_status, player_abilities, creative_inventory_action,
close_window, player_digging, player_block_placement, animation, client_settings, tab_complete,
update_sign, plugin_message (25 visible; the file reports 30 functions total).

**Gaps / caveats:** `0x49` unimplemented; unknown-ID leniency means partially-parsed packets are
tolerated rather than rejected; the file set has no status-state (server list ping) *response*
parser in `net/`, so the ping/pong path is handled elsewhere (`client/server_list.rs`). Packet-body
field-level parity with vanilla was spot-checked (e.g. `0x27 Explosion` reads records +
`player_motion`, `0x28 Effect` unpacks the packed position long) but not exhaustively verified
within this budget.

---

## 4. Renderer

**API: Vulkan (`ash`) as the primary backend, plus an experimental Direct3D 12 backend on Windows.
No wgpu, no OpenGL.** Note this directly conflicts with a wgpu-based plan — the renderer abstraction
is bespoke, not wgpu-shaped.

Structure:

- `crates/renderer/src/{lib.rs, rhi/, core/}` — a hand-written RHI + render-graph layer:
  `rhi/{device,buffer,command,pipeline,shader,texture,types}.rs` and
  `core/{renderer.rs, render_graph.rs, renderer/graph.rs, renderer/legacy.rs, renderer/submit.rs,
  resource.rs, resource_api.rs, material.rs, pipeline.rs, sky.rs, queue.rs}`. Public surface:
  `CoreRenderer, FrameDesc, FrameGraphPlan, FrameGraphResources, FrameInputs, FrameTargetHandles,
  RenderGraph, Renderer, RendererRuntime, ResourceManager, ToneMappingConfig` (`renderer/src/lib.rs`).
- Vulkan backend `crates/renderer/backend/vulkan/`: `context.rs` (instance/surface creation
  `:513-521`, physical device selection `:531`, device creation `:557`), `device.rs`,
  `pipeline.rs` (descriptor-set layouts and `vk::DescriptorType` mapping `:49-57`, stage flags
  `:61-79`, push-constant merging `:26`), `shader.rs`, `resources.rs`, `command.rs`, `swapchain.rs`.
- **Shaders are GLSL compiled at runtime with `shaderc`** (`shader.rs:249-283`:
  `shaderc::Compiler`, `SourceLanguage::GLSL`, `TargetEnv::Vulkan`, `EnvVersion::Vulkan1_3`,
  `compile_into_spirv`), with a custom include resolver (`shader.rs:276`). Source files live in
  `crates/renderer/backend/vulkan/shaders/`: `basic`, `entity`, `gui`, `panorama`, `sky`,
  `tone_mapping` — each `.vert` + `.frag` (12 GLSL files).
- DX12 backend `crates/renderer/backend/dx12/` cross-compiles SPIR-V to HLSL with `spirv_cross`
  (Hlsl feature) and has an `unsupported.rs` stub for non-Windows targets.
- A tone-mapping pass and a `panorama` pass (main-menu background) exist in the shader set.

**Chunk meshing** — `crates/game/src/world/mesh/builder.rs` (1 784 lines, entry point
`pub fn build_chunk_mesh` `:232`) plus `world/mesh/{types,fluid,lighting}.rs` and the JSON block-model
system `world/block_models.rs` (1 666). Strategy:

- **Per-block, per-face emission driven by vanilla JSON block models** (blockstates + models with
  elements and per-face `cullface`): `builder.rs:448-475` and `:496-532` use `face.cullface` and
  `face_visible_with_state(...)` to decide visibility; helpers `face_visible_with_state` `:1234`,
  `internal_shape_face_covered` `:1259`, `face_visible_between` `:1308`, `face_visible` `:1348`,
  `same_fluid` `:1366`, `liquid_side_visible` `:1375`.
- **No greedy meshing** — no greedy algorithm appears anywhere in the mesher; the unit-tested culling
  rules are instead vanilla-faithful special cases (stairs, slabs, glass panes with per-colour
  boundary preservation, slime blocks, fancy leaves, liquids, skulls, barriers — see the test names
  at `builder.rs:1388-1760`).
- OptiFine-flavoured extras behind `MeshOptions` (`world/mesh/types.rs`): `smooth_lighting`,
  `better_grass`, `connected_textures` (seamless glass via `connected_glass_uvs` `builder.rs:111`,
  texel-inset edge UVs `:108`).
- **Lighting and AO are baked into the vertex**: `world/mesh/types.rs` defines
  `Vertex { pos, normal, uv, block_type, sky_light, block_light, ambient_occlusion }`, 48-byte
  stride, 7 attributes (`:1-30`); per-face light comes from `world/mesh/lighting.rs`
  (`face_light`, `smooth_vertex_light`).
- Meshes are split opaque-then-transparent via `ChunkMesh.transparent_start`, carry a precomputed
  world AABB, and are built **off the main thread with rayon**:
  `world/mesh_jobs.rs` (`schedule_background_meshes` `:76`, `rayon::spawn` `:137`,
  `build_pending_meshes(budget)` `:175`, `poll_finished_meshes` `:15`), queued by
  `world/mesh_queue.rs` (`enqueue_chunk_mesh_with_neighbors` `:53`, `…_at_block` `:61`).
- Upload strategy advertised in `README.md:38`: "GPU-local static buffers, asynchronous staging
  uploads, and coarse chunk batching".

**Text / GUI:** `fontdue` 0.9 (game `Cargo.toml`) rasterises glyphs into a runtime-built bitmap font
atlas that is uploaded as a texture — see `crates/game/src/render/gui_renderer.rs` (font atlas
upload at `:598-605` and `:918-925`, label `"game.gui.font-atlas"`; nametag text batching `:453-494`).
The shipped font asset is `assets/fonts/default.ttf`. GUI screens live in
`crates/game/src/render/screens/{basic,advanced,lists,dispatch,state,context}.rs` with a separate
`crates/game/src/ui/` module; HUD/overlays in `render/hud/` (incl. `hud/inventory/`,
`hud/overlays/tooltip/`); item icons in `render/item_icons.rs` and `client/block_icon.rs`.

**Entity rendering:** `crates/game/src/render/entity_renderer/{mod,gpu,api,state,submit}.rs`, with
model families in `render/entity_renderer/models/{biped,monster,passive,quadruped,misc,helpers}.rs`
and mesh/sync subdirectories `render/entity_renderer/{meshes,sync}/`; armour in `client/armor.rs`,
first-person/player model in `render/first_person.rs` + `client/player_model.rs`, skins in
`client/skin_cache.rs` (34.7 KB) and `render/skin.rs`.

**Particles:** `crates/game/src/client/particles.rs` (59.1 KB) + `render/particles.rs` +
`render/particle_mesh.rs`. **Sky:** `render/sky.rs`, `render/custom_sky.rs`, plus the panorama pass.

**Shader packs (OptiFine/Iris):** `crates/game/src/render/shader_pack.rs` (format constant
`SHADER_PACK_FORMAT: u32 = 1`, `SHADER_PACK_DIR = "shaderpacks"`, `:27-31`) and
`render/shaders.rs`; README `:65` marks this **Work in Progress**.

---

## 5. World / chunk representation and lighting

`crates/game/src/world/` (12 613 lines): `chunk.rs` (209), `light.rs` (702), `lighting.rs` (177),
`mesh/lighting.rs` (282), `shape.rs` (2 111), `block_models.rs` (1 666), `network.rs` (428),
`snapshot.rs`, `state.rs`, `queries.rs`, `helpers.rs`, `entities.rs`, `item.rs` (774),
`block/*` (block ids, states, properties, materials, sounds), `tests.rs` (502 lines of unit tests).

**Chunk storage** (`world/chunk.rs`): full-height chunk with
`CHUNK_SIZE = 16`, `CHUNK_HEIGHT = 256`, `SECTION_COUNT = CHUNK_HEIGHT / SECTION_SIZE`,
`CHUNK_VOLUME = 16*256*16`, `BIOME_COUNT = 256` (`:6-14`). Storage is **split**: one byte per voxel
for the block id (`self.blocks[idx] as u8`) plus **nibble-packed metadata**
(`metadata_nibble: Box<[u8; CHUNK_VOLUME_NIBBLE]>` `:30`, helpers `nibble_get`/`nibble_set` `:17/:22`)
and nibble-packed light; a combined 12-bit-ish "state" is `(block << 4) | metadata`
(`get` `:56-63`, `state` `:73`, `set_state` `:84`). Network-supplied light is tracked with
`finish_network_light(has_sky_light, data_valid)` `:91` and `has_valid_network_light()` `:100`.

**Lighting** (`world/light.rs`) is the most directly reusable *knowledge* in the repo:

- Doc comment (`:1-8`): "MC 1.8.9-style lighting. Network chunks keep the server-provided sky and
  block light nibbles. Locally changed chunks are recomputed with vanilla opacity and propagation
  rules." and "Light is computed locally (not just relying on server values)."
- Per-chunk light store documented as **16 × 256 × 16 × 2 (sky + block)** (`:118`).
- Vanilla-derived tables: `sky_light_brightness`/`block_light_brightness` from a generated
  `generate_brightness_table()` reproducing `WorldProvider.generateLightBrightnessTable()`
  (`:17-38`), `block_light_emission()` per block (`:41-65`, e.g. Torch 14, Glowstone 15, Redstone
  Torch 7, Nether Portal 11, Beacon 15), `light_opacity()` (`:70-99`, leaves 1, water/ice 3, solids
  15), and `LightLevel::brightness(sky_brightness) = max(sky_table[sky]*sky_brightness,
  block_table[block])` (`:111-115`).
- **Incremental propagation**: `compute_chunk()` `:175`, `update_around(...) -> HashSet<(i32,i32)>`
  (returns the set of changed chunk columns) `:182`, `update_kind()` `:243` performing a BFS with
  `VecDeque` + `HashSet` work list (`:251-255`) — i.e. classic flood-fill re-light of a bounded
  region plus neighbour dirtying, rather than a full-world relight.

No integrated server, no world generation, no save-format handling exists anywhere in `world/` — the
world is populated purely from `0x21/0x22/0x23/0x26` packets (`world/network.rs`,
`client/network/world_packets.rs`).

---

## 6. Authentication (Microsoft / Mojang)

`crates/game/src/auth/` (7 files). Full modern chain is implemented:

1. **Microsoft OAuth2 (authorization-code + loopback redirect)** —
   `MICROSOFT_AUTHORIZE_URL = "https://login.live.com/oauth20_authorize.srf"` and
   `MICROSOFT_TOKEN_URL = "https://login.live.com/oauth20_token.srf"`, redirect
   `http://localhost:9812/<very-long-path>` (`auth/oauth.rs:11-14`); a local HTTP listener parses the
   redirect request line and query string (`oauth.rs:140-143`); code→token exchange (`oauth.rs:37`,
   `:185-196`) and `microsoft_refresh()` for refresh-token renewal (`oauth.rs:201`).
2. **Xbox Live** — `XBL_AUTH_URL = "https://user.auth.xboxlive.com/user/authenticate"` and
   `XSTS_AUTH_URL = "https://xsts.auth.xboxlive.com/xsts/authorize"` (`auth/xbox.rs:3-4`), with
   `RelyingParty = "rp://api.minecraftservices.com/"` (`xbox.rs:144`) and friendly error strings for
   "no Xbox account" / unsupported region (`xbox.rs:196`, `:206`).
3. **Minecraft services** (`auth/minecraft.rs:3-6`) —
   `POST https://api.minecraftservices.com/authentication/login_with_xbox`,
   `GET https://api.minecraftservices.com/entitlements/mcstore` (ownership check `:85`),
   `GET https://api.minecraftservices.com/minecraft/profile` (`:115`), and
   `POST https://sessionserver.mojang.com/session/minecraft/join` (server session join).
4. **Orchestration** — `auth/service.rs` drives the flow and stores the resulting
   `minecraft_access_token` (`:150`), refresh handling and expiry checks in `auth/models.rs`
   (`microsoft_token_expiry` `:40`, `is_logged_in()` `:36`).

**Token storage: plaintext JSON on disk.** `auth/cache.rs`: `save_account()` `:13`,
`save_store()` writes `serde_json::to_string_pretty(store)` to `ACCOUNTS_FILE` `:124-125`,
with legacy fallbacks probing `CACHE_FILE`/`ACCOUNTS_FILE` `:94-97`. `AuthAccount` keeps
`microsoft_refresh_token` and `minecraft_access_token` as plain `Option<String>` fields
(`auth/models.rs:6-7`, `:73`). No OS keyring, no file encryption.

`auth/cache.rs` is therefore a good *reference* for endpoint order, but the storage design is a
security gap for a new client.

---

## 7. Asset handling

**There is no launcher, no piston-meta usage, and no client.jar parsing in the codebase.**

- Searches for `piston`, `version_manifest`, `launcher_profiles`, `asset_index`, `libraries`,
  `download` over `crates/game/src/**/*.rs` return only false positives (the word "Piston" as a
  *block name* in `client/app/block_interaction.rs` and friends).
- The README instead instructs the user to stage assets by hand — `README.md` §"Preparing Minecraft
  Assets" (`:152` onward): (1) run vanilla 1.8.9 once via the official launcher and copy
  `assets/` (containing `indexes/`, `objects/`) into RustCraft's `assets/` directory
  (`README.md:200-227`), and (2) extract `assets/minecraft/{blockstates,lang,models,shaders,textures}`
  from the user's own `~/.minecraft/versions/1.8.9/1.8.9.jar` (`README.md:229-285`). The README warns
  against third-party asset mirrors and states assets are excluded from the repo and releases "for
  copyright and licensing reasons" (`README.md:103`, `:154-160`).
- The repo ships only 5 asset files: `assets/icon.ico`, `assets/icon.rc`,
  `assets/fonts/default.ttf`, `assets/minecraft/lang/en_US.lang`, `assets/minecraft/lang/zh_CN.lang`.
- Runtime consumption: `crates/game/src/assets/index.rs` loads
  `{assets_dir}/indexes/{version}.json` (e.g. `1.8.json`) in **Mojang asset-index shape**
  (`{"objects": {logical_path: {hash, size}}}`, `:20-24`) and resolves each object to
  `{objects_dir}/{hash[0:2]}/{hash}` (`:48-73`). Path resolution is relative-or-workspace-root via
  `assets/mod.rs:14-31` (`resolve_path`, `CARGO_MANIFEST_DIR/../..` fallback).
- Resource packs: `assets/resolver.rs` — zip packs under `resourcepacks/` containing `pack.mcmeta`
  and an `assets/minecraft/` tree, layered over the vanilla base (`:4-5`, `:151-220`), with
  `assets/resource_pack.rs` as the extracted-pack variant.
- Audio: `crates/game/src/audio.rs` (20.3 KB) + `assets/sound.rs` — rodio `OutputStream`/`Sink`
  playback, a `SoundRegistry`, `SoundCategory` enum, `AudioBackend` trait, and per-protocol-id sound
  event mapping. Minecraft `sounds.json`/ogg files come from the same staged asset tree.
- `crates/game/build.rs` compiles `assets/icon.rc` on Windows (`embed-resource`) and copies a
  "FidelityFX runtime" next to the binary.

**Implication for the new project:** the piston-meta asset downloader plus client.jar extraction is
100 % greenfield work. The good news is RustCraft's expected on-disk layout (`indexes/` + `objects/` +
`minecraft/`) is exactly what a piston-meta-driven launcher would produce, so the *consumer* side can
be modelled on `assets/index.rs`.

---

## 8. Client-side Lua scripting

- **Crate/version:** implemented inside `rustcraft-game` (not a separate crate) under
  `crates/game/src/scripting/` — 27 files, 10 810 lines — on **`mlua` 0.11 with
  `lua54`, `vendored`, `serialize`** (`crates/game/Cargo.toml`).
- Self-described as a "**Sandboxed, client-only Lua mod runtime**" (`scripting/mod.rs:1`) with
  `pub const API_VERSION: u32 = 1;`.
- Modules: `manager.rs` (2 339 lines — `ScriptManager`, `LoadReport`, `LoadedModInfo`,
  `QueuedUiCommand`), `loader.rs`, `manifest.rs` (`ModId`, `ModManifest`), `permissions.rs`
  (`Permission`, `PermissionPolicy`, `PermissionSet`, loaded from `mods/permissions.json`, see
  `mods/permissions.example.json`), `event_bus.rs` (`ScriptEvent`, `PlannedCallback`,
  `EventOutcome`), `runtime.rs` (444), `scheduler.rs`, `profiler.rs`, `protocol.rs`, `config.rs`,
  `errors.rs`, `callback.rs`.
- Exposed API surface (`scripting/api/`): `client`, `world`, `render`, `ui`, `input`, `player`,
  `network`, `protocol`, `animation`, `config`, `context`, `resources`, `storage` — including
  `ClientCommand`, `UiCommand`, `ResourceRegistration`, world/entity/block snapshots.
- **Hook points:** the game pushes per-frame snapshots and drains queued commands —
  `manager.rs:202 update_client_snapshot`, `:247/:251 update_world_snapshot[_reusing_blocks]`,
  `:211 drain_client_commands`, `:55/:59 ScriptCommand push/drain`, and
  `has_callbacks(event_name)` `:157` to skip work when no mod subscribes. The bridge lives in
  `crates/game/src/client/app/script_bridge.rs` with wiring in `client/app/mod.rs`.
- Example bundled mods: `mods/keystrokes_hud/`, `mods/old_animations/`, `mods/pvptweaks/`, each
  `{manifest.json, scripts/client.lua}`. Manifest shape (verbatim from
  `mods/pvptweaks/manifest.json`): `id`, `name`, `version`, `api_version: 1`,
  `entrypoints: { client: "scripts/client.lua" }`, `permissions: ["client.read", "client.modify"]`.
- Security note for a 1.8.9 client aimed at real servers: the scripting layer exposes the network
  and protocol APIs, so mods are effectively privileged client code (the "sandbox" is the project's
  own permission model over a vendored Lua 5.4, not an OS-level sandbox). Anti-cheat considerations
  are the adopter's problem, not RustCraft's.

---

## 9. Maintenance status

| Signal | Value |
|---|---|
| HEAD commit | `d1c861f9` — "docs: update public renderer documentation", 2026-08-09T13:08:36Z |
| Repo `pushed_at` | 2026-08-11T07:19:41Z (release/tag activity on cut code) |
| Age at survey | last code commit ~6 weeks before 2026-09-22 |
| Repo created | 2026-07-17T08:06:06Z (≈ 3.5 weeks of history total) |
| Open issues | **0** |
| Stars / forks / contributors | 83 / 8 / 2 |
| Archived | false |
| Releases | `v1.0.0` "V1.0.0-Preview" 2026-08-11 → `RustCraft-1.0.0-x86_64.AppImage`, `RustCraft-Linux-x86_64.zip`, `RustCraft-windows-x86_64.zip` (+`.1.zip`); `v0.1.0` 2026-07-18 |
| CI | single workflow `.github/workflows/build-packages.yml` ("Build public packages", 19.4 KB) |
| CI jobs | `Windows x86_64` (`windows-latest`, uses `humbletim/setup-vulkan-sdk@v1.2.1`, builds `cargo build --release --locked`) and `Linux x86_64 AppImage` (`ubuntu-22.04`) |
| CI history | 18 total runs; latest three: **success** (v1.0.0, 2026-08-11), **success** (main, 2026-08-09), cancelled (main, 2026-08-09) |
| Linux build | Yes — CI produces a Linux AppImage + Linux zip from source on `ubuntu-22.04`; README `:59-63` claims "Windows and Linux support" and "Linux AppImage packaging" |

Not built locally (out of scope for this survey). No `rustfmt`/`clippy`/test workflow exists — only the
packaging workflow — so lint/test hygiene is unverified upstream. The shallow clone (`--depth 1`)
means commit-volume history is not available locally; API counts above are authoritative.

---

## 10. Verdict

### (a) What a new project could depend on directly from RustCraft, as a crate

**Realistically: nothing.** Concretely:

- **Not published:** all four packages 404 on crates.io (`rustcraft-game`, `rustcraft-renderer`,
  `rustcraft-renderer-vulkan`, `rustcraft-renderer-dx12`), and none of the four manifests carries a
  `license` field, so a `cargo add` route does not exist and a future publish would be blocked by
  crates.io metadata requirements.
- **Licence:** a `git = "https://github.com/RustCraftMC/RustCraft-Public"` dependency would import
  the Noncommercial Source-Available Licence into the downstream crate graph — §3 forbids commercial
  use, §5/§6 require publishing complete corresponding source of anything distributed. That is
  incompatible with most intended uses of a new client.
- **Technical mismatch:** the only genuinely reusable-looking crate, `rustcraft-renderer` (the RHI +
  render-graph layer, ~14 k lines including backends), is built on **ash/DX12, not wgpu**
  (`crates/renderer/backend/vulkan/Cargo.toml`, and `grep 'name = "wgpu"' Cargo.lock` → 0). Adopting
  it would replace the wgpu premise rather than support it. Its `rhi/` facade is nonetheless worth
  reading as a checklist of what a wgpu-based design gets for free (pipelines, descriptor sets,
  resource lifetimes, staging uploads, render-graph pass ordering).

### (b) What would require copying code, and the licence implications

Any protocol tables, mesh culling rules, light tables, or auth plumbing copied verbatim — or even
closely paraphrased — brings you under §2 (§b–§d noncommercial scope) and triggers §5–§8 obligations
on distribution: publish the complete corresponding source of the distributed version, keep the
unmodified licence and all notices, do not present the result as an official RustCraft release, and
use of the name only descriptively ("Based on the RustCraft project."). Private, noncommercial
modification with no distribution is explicitly permitted (§4). If the new client is meant to be
commercially usable or closed-source, the only clean options are (i) independent reimplementation
from the protocol specification and public vanilla behaviour, or (ii) negotiate a separate licence
(§10).

Highest-value *knowledge* to re-derive independently rather than copy: the vanilla brightness table
formula (`world/light.rs:17-38`), light-opacity/emission values (`:41-99`), packet field layouts and
the VarInt/compression framing (`net/protocol.rs`), the asset-index directory convention
(`assets/index.rs:20-73`), and the MS→XBL→XSTS→Minecraft-services endpoint order (`auth/*.rs`).
Algorithms, wire formats and endpoint lists are facts; the specific Rust expression of them is
copyrightable, so write your own.

### (c) What is missing compared to vanilla 1.8.9

- **Singleplayer / integrated server: absent.** There is no world generator, no level format
  handling, no local server; the world arrives purely from network packets (`world/network.rs`,
  `client/network/world_packets.rs`). Only the multiplayer path exists — server list
  (`client/server_list.rs`, `ServerList::load_default`, `client/app/menu_actions/server.rs:182`) and
  direct connect. A singleplayer-capable 1.8.9 client must supply its own worldgen + save format.
- **No launcher / asset pipeline: absent** (see §7). No piston-meta client, no library/version
  resolution, no runtime (Java) management, no asset hashing/download/extraction, no client.jar
  access. Everything the description calls "custom launcher that downloads assets from Mojang
  piston-meta" is greenfield. RustCraft's consumer-side expectations (`assets/indexes/{ver}.json` +
  `assets/objects/xx/hash` + `assets/minecraft/...`) are compatible with that output, which is a
  useful interface constraint.
- **Protocol:** `0x49 UpdateEntityNBT` unimplemented; no status-state (ping) parser inside `net/`;
  unknown packets are swallowed rather than surfaced.
- **GUI surface is broad but not vanilla-complete:** screens (`render/screens/basic.rs`,
  `advanced.rs`, `lists.rs`), HUD with inventory + tooltips, chat, server list, sign/book editing
  (`client/app/book.rs`, `client/book.rs`), keybind/config menus, and gamepad support exist;
  exact vanilla screen parity (e.g. every container's layout, death screen flow, statistics screen)
  is not verified. Inventories are implemented (`client/inventory.rs` 47.2 KB,
  `client/app/inventory_interaction.rs`, `render/hud/inventory/`) including creative actions,
  window property/transaction confirmation.
- **Audio: present** (rodio + `SoundRegistry` + positional categories, `audio.rs` / `assets/sound.rs`).
- **Particles: present** (`client/particles.rs` 59.1 KB, `render/particles.rs`, `particle_mesh.rs`).
- **Entities: present and substantial** (biped/monster/passive/quadruped models, armour, skins,
  metadata + translation tables, entity interpolation; `world/entities.rs`,
  `render/entity_renderer/**`, `net/metadata.rs`, `net/translation/**`).
- **Physics/collision: present** — AABB collision and movement in `client/physics.rs` (45.6 KB) with
  fence/wall/plant special cases and item-use tick planning in `client/app/block_interaction.rs`.
- **Anti-cheat / server compatibility:** the client speaks online-mode (`EncryptedReader/Writer`) so
  login works, and there is no evidence of cheat-detection bypass logic; however the Lua mod layer
  exposes network/protocol APIs (`scripting/api/network.rs`, `api/protocol.rs`), which is the kind of
  surface many server anti-cheats and server rules treat as disallowed. Ships publicly as
  "experimental".
- **Unfinished by its own admission:** shader-pack support (`README.md:65`), and packaging on Linux
  depends on an AppImage recipe inside the CI workflow rather than a maintained release process.
- **Not verified within budget:** deep field-level parity of every packet body; no build was
  performed (per instructions), so "builds on Linux" rests on CI evidence (successful
  `ubuntu-22.04` AppImage job, `build-packages.yml`), not on a local compile.

**Overall:** RustCraft is the closest existing 1.8.9 Rust client and an excellent *reference* for
packet layouts, light tables, meshing culling rules, auth endpoint ordering and the expected asset
directory layout. It is a poor *dependency*: unpublished, noncommercially licensed, Vulkan-bound, and
missing precisely the pieces the new project needs most (launcher/asset download, singleplayer,
`0x49`). Plan on independent reimplementation on wgpu, using RustCraft strictly as evidence.
