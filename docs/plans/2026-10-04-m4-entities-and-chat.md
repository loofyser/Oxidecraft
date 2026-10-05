# M4 Entities and Chat Implementation Plan

> **How to work this plan:** one task at a time, in order. Run each step's verification before moving on, and tick the checkboxes (`- [ ]`) as you go. Every commit is made on `main` with explicit `git add` paths.
>
> **Plan style:** this plan pins every interface, constant, path, and rule the tasks must agree on, and carries code sketches wherever a byte layout, formula, or data shape is the deliverable. Algorithm bodies that mirror vanilla behaviour are derived from the cited MCP-919 source files (`refs/_src/MCP-919`, local and uncommitted; the tree is present from the previous milestones, HEAD `1717f75902c6184a1ed1bfcd7880404aab4da503`) by the task that needs them, and every derived constant is pinned by a test. Where this plan and a cited document disagree, the document governs and the plan text is corrected first. Pin the source's expression and cite the method that applies it where the two differ — a factor can live in the applier, not the computer.

**Goal:** Entities and chat. The client sees the rest of the world: other players, mobs and object entities spawn, move with the source's two-position interpolation, and render as the source's box models with skins, nametags, hurt and death states; dropped items, arrows, boats and the rest of the object set draw; the HUD reads the server: chat with wrapping, formatting and click events, the tab list, the scoreboard sidebar and the boss bar.

**Architecture:** The session's store grows the entity table (`oxide-world`), the wire codecs grow the spawn, movement, metadata and UI surfaces (`oxide-proto-v47`), and the session tracks everything the server reports, ticking animation counters and publishing one full entity snapshot per tick beside `PlayerTick`; player-list, scoreboard and header/footer state change rarely and publish snapshots on change. The window keeps the mirrored state, resolves skins (fetched off-thread through the new `oxide-assets` cache), builds per-frame entity draws with the same tick-fraction interpolation the player uses, and feeds two new render passes: an entity pass placed between the terrain's solid and translucent layers, and a HUD pass at the source's scaled resolution. Chat input flows where the M3 keys flow; the mouse hands over to the chat window while it is open, exactly as the source's screen does.

**Tech Stack:** Rust 2024 edition (rust-version 1.85, developed on 1.99), the M3 stack unchanged. **Two new dependencies:** `base64` 0.23 (new to the workspace; the profile-property decode — 0.23 keeps one copy in the lock, which `ureq` already brings at 0.23.1) and `serde_json` (already in the workspace table; new as a direct dependency of `oxide-game`, the chat-JSON parse). No new crate edges — every task stays inside the section 5.1 table.

**Spec:** `docs/specs/oxidecraft-v1-design.md` (v6). M4 is the section 13 row "Entities and chat", with exits *"Two clients, one vanilla and one ours, see each other, chat, and agree on entities"*. Read section 9 before Tasks 4–5, section 6 (the four-context picture and the read discipline) before Tasks 5–6, section 10 and `docs/research/render-parity-survey.md` §1–2 and §4.5–4.6 before Tasks 8–14, section 11.2 and the survey's §3.2 before Tasks 13–18, and `docs/research/protocol-47-reference.md` §2 (the entity and UI rows and §2.3's ordering notes) and §6 (the metadata tables and spawn ids) before Tasks 1–3. The skin and profile material is `docs/research/launcher-assets-auth-survey.md` §3.6–3.8. Vanilla citations are files under `refs/_src/MCP-919` (uncommitted), the same source the research reports cite. The M3 final review's "what will fight M4" list (`refs/m3-final-review/` — the carry notes in `docs/STATE.md` itemise it) is this plan's standing risk list, and each item is answered by a task below.

## Global Constraints

- License GPL-3.0. Adapted third-party code must be recorded in `NOTICE`.
- Zero code copied from RustCraft. It is a read-only reference only.
- No Mojang asset, jar, `.class` file, `.ogg`, or `.png` may ever be committed. Nothing under `refs/` or `vanilla/` is committed; textures, models, fonts and colormaps are read from the user's own store at runtime only. No test fixture may embed a Mojang pixel: fixtures are synthetic (generated in the test) or read from the store by an ignored test. Skin textures fetched at runtime are cached under the user's store, never in the repository.
- Never read `.class` files from the jar at runtime. Design invariant.
- rust-version 1.85, edition 2024. `Cargo.lock` is committed.
- CI must fail on: formatting, clippy warnings, test failure, license violations, crate-graph violations, or a tracked Mojang asset.
- This milestone adds **two dependencies** — `base64` 0.23 (workspace table + `oxide-assets`; the lock's existing copy) and `serde_json` as a direct dependency of `oxide-game` (already in the workspace table). Both are permissive (MIT/Apache-2.0) and covered by `deny.toml`; the milestone adds **no crate edge** — the allowed edges are exactly the section 5.1 table, which `scripts/check-graph.sh` keeps asserting unchanged.
- Every public item carries a doc comment (workspace lint `missing_docs`); `unsafe_code` is forbidden workspace-wide.
- `git add` is always explicit with paths; never `git add -A` or `git add .`.
- Evidence (captures, screenshots, logs) lives under the git-ignored `refs/` tree. Committed documents cite it by path.
- The local gate before every push is the six-command set plus both guard self-tests and the parity self-test: `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test`, `bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test`, `python3 scripts/parity-diff.py --self-test`.
- Values that come off the wire are hostile until validated: every length, mask, declared size, string, metadata entry count and NBT block is checked before it sizes an allocation or a loop; framing never panics (spec S2). The same rule covers everything read out of the jar and everything decoded from a fetched profile property, a fetched skin PNG, or a chat JSON tree.
- Numerical parity: every movement, animation, camera, GUI-layout and skin-rule constant this milestone introduces (interpolation snap distance, swing and limb-swing steps, hurt alpha, death tilt, nametag scale and distance, chat layout arithmetic, GUI scale rule, tab-list geometry, boss-bar geometry, default-skin rule) is traceable to a cited MCP-919 file or a research report, and is pinned by a test. A constant with no citation does not land.
- The keepalive/read-loop discipline is binding and extends to every new handler: the play loop's added per-packet work stays bounded (a write into the entity table or a state map), no mesh, layout, or JSON work runs in the read pass, the per-tick entity snapshot is built once per tick, and the `session_replay` keepalive test must keep passing through every restructuring.

## Decisions taken in this plan

| # | Decision | Rationale |
| --- | --- | --- |
| 1 | **The entity store lives in `oxide-world`.** `oxide-world/src/entity.rs` grows `Entities` (the id-keyed table), `Entity` (position and last-tick position, rotation pair, head yaw, velocity, on-ground, flags, the raw metadata map with typed accessors, equipment, per-kind data, and the animation counters: age, limb swing pair, swing progress, hurt ticks, death ticks) and the closed `EntityKind` enum with its per-kind data. The wire type tables (`MobType`, `ObjectType`, `GlobalType`) live in `oxide-proto-v47` id-keyed beside the decoders; the session maps wire type → `EntityKind`. The store holds no render knowledge; its raw metadata rides the allowed `oxide-world → oxide-proto-v47` edge (the value as received; the typed accessors derive from it). | Spec §5.3 keys `Entity` to `oxide-world` and §9 fixes the two-position contract; the v47 crate owns wire identifiers because ids are protocol facts, `oxide-world` owns the world model — the existing split. |
| 2 | **Remote entities interpolate like the player.** Every entity keeps the two most recent server positions; render lerps previous → current by the same tick fraction as `PlayerTick`, snapping when a jump exceeds 4 blocks (spec P5; the same predicate the M3 pose lerp pins). The pair travels in the tick frame; the window does no history of its own. | Spec §9's sentence is the same rule for every entity; reusing the M3 fraction keeps one clock for everything the frame draws. |
| 3 | **One event carries every entity, once per tick.** `ClientEvent::EntitiesTick { entities: Vec<EntityFrame> }` follows `PlayerTick` in the same tick batch and carries a full snapshot; the window replaces its view each tick (spawn, despawn, and every update fall out of set replacement — no merge logic on the window side). The frame carries id, kind, the position pair, the rotation pair and head yaw, on-ground, the flag byte's render bits, the animation counters, the composed nametag text when visible, and the per-kind render payload. Cost is O(tracked entities) per tick, bounded by the server's view distance; the snapshot is built once per tick, in the tick phase, never per packet (Global Constraints). | Matches `PlayerTick`'s established pattern (Decision 2 of the M3 plan) and keeps all merge and expiry logic in the one component that owns the state. The alternative — per-packet events with window-side merging — spreads the same bookkeeping across both threads for no milestone payoff. |
| 4 | **Known kinds draw; unknown kinds are tracked.** `EntityKind` is a closed enum over the renderable roster plus `Unknown`; an id the tables cannot name (a future or modded type) is stored and carried as `Unknown`, drawn not at all, and never fatal (spec S2's spirit — a packet the client cannot name must not end the session). | The M1/M2 precedent for unknown ids; the entity filter on 0x25 needs every known id even when undrawn. |
| 5 | **Names travel as composed text.** The session composes each entity's display text — players: the player-list name with team prefix, suffix and colour applied the way the source's `getDisplayName().getFormattedText()` composes it; mobs: the custom name with its formatting — and ships it as legacy `§`-coded text in the frame's nametag field (`None` when the name must not show). The window renders `§`-coded text through one shared decoder. Task 6 extends the composition function to consult teams; Tasks 13, 16 and 17 consume it. | One composition point (the session owns list, teams, custom names) and one decoding point (the text renderer) — the window never re-derives display logic, and chat/tab/nametags share the `§` machinery. |
| 6 | **Skins are a cache, a rule, and a worker.** `oxide-assets/src/skins.rs` grows `SkinCache` (disk under the store's `skins/<hash>.png` plus a memory map), the default-skin rule mirroring `DefaultPlayerSkin` (the UUID hash picks Steve/Alex; the model byte picks wide/slim — golden vectors pinned), and the profile-property decode (`base64` of the `textures` property → the JSON → the URL and the slim flag). Fetches go through the existing HTTP layer (`ureq` agent rules unchanged) and are run by a small client-side worker thread that feeds decoded textures back over a channel; a fetch or decode failure falls back to the default skin for that UUID, once — no retry loop. The ignored live-endpoint test exercises the real fetch. | The M0 HTTP and M2 PNG machinery exist; the profile property is the only place base64 appears, and this is the milestone where skins are a deliverable. Threading matches the M3 rule: the window never blocks on network. |
| 7 | **Chat is a model, a log, and a layout in `oxide-game`.** `oxide-game/src/chat.rs` grows the component model (text, colour, the four style flags, click and hover events, children), a hostile-input parser over `serde_json::Value` (hand-walking, never `derive`-trusting; malformed JSON degrades to plain text), the legacy `§`-code parser (shared with names), the layout pass (wrapping by font metrics per the source's split rule), and the chat log (retention, fade tick stamps, scroll offset). Receive ships the raw JSON string plus the position byte (`ClientEvent::Chat { text: String, position: i8 }`); send is the raw text over serverbound 0x01 (≤ 100 chars). Message options are the `GameSettings` defaults (chatScale 1.0, chatWidth 1.0, chatHeightFocused 1.0, chatHeightUnfocused 0.44366196, chatOpacity 1.0, chatColours on — `GameSettings.java:85-109`); the options screen that edits them is M6's. | Spec §5.3 keys chat to `oxide-game`; layout needs the font metrics `oxide-assets` already exposes, and the window cannot own model logic the tests need to reach. `serde_json` is already trusted for JSON elsewhere; `Value`-walking keeps "hostile until validated" literal. |
| 8 | **The HUD is a pass at the scaled resolution, and the text builder is shared.** `oxide-render` grows `hud.rs`: a GUI-space pass sized by the source's `ScaledResolution` rule (auto scale — the `GameSettings` default 0 means auto: the largest factor keeping ≥ 320 × 240 at 1280×720 gives factor 3 (427×240); at 1920×1080 the auto loop lands on factor 4 — 480×270, there is no ≤3 clamp in the auto path — both pinned by tests; the unicode-font halving (`ScaledResolution.java:32-35`) is skipped, this client has no unicode font, recorded). Its text path is the debug overlay's glyph machinery extracted into a shared builder with position, scale, colour and shadow parameters; the overlay's own output is unchanged (its tests stay green) and it remains its own pass until M6 retires it into the screen framework. | The M2 stand-in overlay was always temporary; this milestone needs real GUI-space drawing for four surfaces, and one shared text path keeps `§` styles, shadow and scale in one place. The scale rule is a parity constant, so it is pinned, not configured. |
| 9 | **The entity pass draws between the terrain's solid and translucent layers, fogged and lit per entity.** `scene_draws` gains `SceneDraw::Entities`; the terrain pass splits into its solid (opaque + cutout) and translucent draws so entities draw where the source draws them (after solid terrain, before water). The pass reuses the frame's fog uniforms; each draw carries the frame's brightness (the source's float `Entity.getBrightness` chain, sampled in the session at the entity's feet); boxes are lit by texture × brightness with no directional face term (the source's model shading); shadows are the flat shadow quad. | The 1.8 order is observable (water covers entities); drawing entities after translucent would be a visible divergence the acceptance would catch. |
| 10 | **The roster and its layers are enumerated, and the deferred list is explicit.** The mob roster is every 1.8.9 spawn type `MobType` names (32 entries, protocol §6.3); layers in scope are the identity set (sheep wool and shearing, wolf collar, slime and magma gel, spider/enderman eyes, mooshroom mushroom, snow golem head, iron golem flower, saddles on pig and horse, witch hat); held items and armour on entities, custom heads, deadmau5 ears, the creeper charge aura, the wither aura and the dragon's detail layers are **out** — equipment (0x04) is decoded and stored, drawn never (M5's item models close the class). Subject to Question 1. | The row says "mob box models"; the checklist's item 45 names the roster, and the checklist is the coverage target. The deferred set is what needs machinery this milestone does not build. |
| 11 | **The object set is enumerated.** In scope: dropped items (block items via their baked block models, item items via the source's generated-item shape — derive it from `ItemModelGenerator`), XP orbs, arrows, thrown items (snowball, egg, ender pearl, eye of ender, potion, experience bottle, firework), fireballs (both sizes and the wither skull), paintings (the `EnumArt` table), item frames, boats, and minecarts (plain plus chest, furnace, TNT and hopper — the cargo reuses the baked block models). Dropped item and dropped-frame items resolve through a pinned minimal item-name table; ids outside it draw the atlas's missing sprite and are recorded. Out: the fishing bobber, primed TNT, falling sand, ender crystals, leash knots, lightning (tracked, undrawn) and armour stands. Subject to Question 2. | The row names "arrows, thrown and dropped items, boats, minecarts, item frames, paintings, XP orbs"; the snowball family is the source's own one-renderer cluster, so it rides free; the rest needs machinery no milestone has built yet. |
| 12 | **Nametags are the source's rule, measured on the rig.** Scale 1/37.5 (`0.02666667`), background `rgba(0,0,0,0.25)`, the distance cutoff and the see-through rule derive in-task from `RendererLivingEntity.renderName`; visible names draw above the entity, not through terrain where the source holds them. Spec §16's obligation — "confirm on the rig in M4 and add a checklist item" — lands in Task 22: the vanilla frame at a known distance is measured and the checklist row is finalised. | The section 16 rule is binding; the measurement closes it. |
| 13 | **The boss bar is the 1.8 single status.** The entity pass sets a `BossStatus`-shaped value while a wither or dragon draws (name, health fraction, colour modifier); it holds for 100 frames after the last set, exactly as `BossStatus` does; the HUD draws the two `widgets.png` slices. Multiple bosses overwrite each other — that is the 1.8 rule; if checklist item 13's text claims stacking (a later-version behaviour), the pre-flight corrects the checklist and records why. | `BossStatus.java` is a single static; parity is to 1.8.9. |
| 14 | **The tab list renders from the session's player-list state.** Task 5 owns the player-list state (0x38, all five actions — the decode extension lands in Task 3); Task 6 publishes the list event and owns 0x47 and its own state; the window draws on the held Tab key (source default, key 15), with heads from the skin textures, latency bars from `gui/icons.png`, the list objective's scores beside names, and the header and footer centred. | The row names the tab list; its state is the player list, which Task 6 already owns for skins and names. |
| 15 | **The scoreboard is session state, drawn on the sidebar and below names.** Tasks 3 and 6 own decode and state (objectives, scores, display slots, teams); Task 17 draws the sidebar (≤ 15 lines, the source's x rule, the title centred, the red-numbers rule) and the below-name scores, and the team clauses colour names in nametags and the tab list. | Spec §9: scoreboard lives in the world state struct updated from packets; teams colour every surface at once, so one owner. |
| 16 | **The dig-overlay carry lands here.** The stage map re-keys to the breaker id (`RenderGlobal.java:126-127`), the 0x25 handler filters to known entities, the client's crack map follows (each entry draws; two breakers on one block multiply twice, as the source does), and the wiring-level discriminator the M3 review asked for is added at this touch (two legs: construction-level descriptor pins — the outline and crack pipelines' depth states, each P1/P2 flipped reds — and the event→consumed-state test driving an aim change and a breaker-keyed stage pair through the client's own wiring path; the builders' pixel pins stand as-is). | The carried items' routing rule: at the next overlay touch. |
| 17 | **The player carry lands here.** `physics::step` reports the horizontal-collision flag in its return; the sprint release gains the source's third clause (`EntityPlayerSP.java:818-821`); the sneak eye offset lands (`EntityPlayer.getEyeHeight`, `:2326-2341` — `1.62 − 0.08` while sneaking) and both its consumers (the camera's render eye and the interaction raycast) follow it; the M3 plan's pin line and `player.rs`'s doc cites are amended in a docs commit. | The state backlog's items 2 and 3, both small and player-domain; the milestone that tracks other entities is the one that can test collision release against a watched wall. |
| 18 | **The rig grows a chat macro and hands the mouse to an open chat.** The scripted-input mode gains a `chat <text>` line (open, type through the text path, send) and the rule that while chat is open the `look` deltas move the cursor instead of the camera (the source's own screen rule); Tab joins the key set. The acceptance scenes, their setup and their evidence are Task 22's; the give-after-respawn rule and the window-free capture lesson carry. | The desktop cannot deliver synthetic typing to our own window (the M2/M3 notes); the in-client path is the only repeatable evidence route. |

## Open questions for the owner (answers recorded on approval)

1. **The roster and its layers (Decision 10).** The full 32-type MobType roster with the identity layer set; held items, armour, custom heads, deadmau5 ears, charge/aura/dragon detail layers out; equipment decoded and stored but never drawn. **Recommended: yes** — the row says "mob box models", checklist item 45 names the roster, and the deferred set is exactly what item-model machinery would unlock.
2. **The object set (Decision 11).** The named set plus the snowball family and the fireballs; block drops render through the baked block models; a pinned minimal item-name table for non-block drops; the listed exotics out. **Recommended: yes** — one `RenderSnowball`-shaped billboard covers most of the throwables, and the named list is the row's own text.
3. **OPEN_URL click events (Decision 7's surface).** Vanilla's confirm screen is M6's; here the link opens only through an interim confirm overlay (the URL shown, Enter opens through the system opener, Esc cancels) — not the source's `GuiConfirmOpenLink` proper until M6. **Recommended: yes** — never auto-open a server-supplied URL, and never silently drop the event.

### Answers recorded on approval (2026-10-04)

| # | Question | Answer |
| --- | --- | --- |
| 1 | Roster and layers | Yes — the full 32-type roster with the identity layer set; equipment decoded and stored, never drawn (as recommended) |
| 2 | Object set | Yes — the recommended set as enumerated in Decision 11 |
| 3 | OPEN_URL handling | Yes — the interim confirm overlay; the source's `GuiConfirmOpenLink` proper is M6's |

---
### Task 1: The entity codecs I — spawns and the metadata block

**Goal:** The protocol crate can name every entity type and decode every spawn packet and the metadata block: the wire type tables (`MobType`, `ObjectType`, `GlobalType`), the six spawn decoders, the metadata entry decoder with all its tags, and the unit helpers the entity wire format leans on (fixed-point, angles, velocity).

**Files:**
- Create: `crates/oxide-proto-v47/src/entity.rs` — the type tables, the spawn structs and decoders, the metadata types and decoder, the conversion helpers.
- Modify: `crates/oxide-proto-v47/src/lib.rs` — module declaration and re-exports.
- Test: `crates/oxide-proto-v47/src/entity.rs` — inline unit tests (helpers, per-tag metadata cases, hostile cases).
- Test: `crates/oxide-proto-v47/tests/entity_codecs.rs` — the fixture corpus: one hand-built byte vector per spawn struct with realistic values, and the table completeness suite.

**Interfaces:**
- Produces:
  - `oxide_proto_v47::entity::MobType` — one variant per 1.8.9 spawn-mob id, exactly the protocol §6.3 roster (32 entries: creeper 50 through guardian 68, pig 90 through rabbit 101, villager 120 — derive each id from §6.3 and pin it in the table's test; do not trust this plan's memory of any single id). `from_id(id: u8) -> Option<MobType>`, `id(self) -> u8`.
  - `oxide_proto_v47::entity::ObjectType` — one variant per §6.3 spawn-object id (boat 1 through fishhook 90's block on the table; the full §6.3 set), same `from_id`/`id` shape.
  - `oxide_proto_v47::entity::GlobalType` — the global-entity table (lightning 1; the rest per §6.3), same shape.
  - `oxide_proto_v47::entity::{SpawnPlayer, SpawnObject, SpawnMob, SpawnPainting, SpawnXpOrb, SpawnGlobal}` — fields: `SpawnPlayer { entity_id: i32, uuid: String, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, current_item: i16, metadata: Metadata }`; `SpawnObject { entity_id: i32, kind: ObjectType, x: f64, y: f64, z: f64, pitch: f32, yaw: f32, data: i32, velocity: [f64; 3] }`; `SpawnMob { entity_id: i32, kind: MobType, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, head_yaw: f32, velocity: [f64; 3], metadata: Metadata }`; `SpawnPainting { entity_id: i32, title: String, x: i32, y: i32, z: i32, facing: u8 }`; `SpawnXpOrb { entity_id: i32, x: f64, y: f64, z: f64, count: i16 }`; `SpawnGlobal { entity_id: i32, kind: GlobalType, x: f64, y: f64, z: f64 }`. One `decode_*` fn per struct on the crate's existing cursor type, returning the crate's existing error type.
  - `oxide_proto_v47::entity::{Metadata, MetadataValue, MetadataItem}` — `Metadata { entries: Vec<(u8, MetadataValue)> }` (empty-or-not per source); `MetadataValue` with one variant per §6.1 tag — byte, short, int, float, string, item stack, the three-int position and the three-float rotation — payloads exactly as §6.1 lists them; `MetadataItem { id: i16, count: u8, damage: i16 }` (the slot's payload; the NBT tail is skipped under a pinned byte cap — `MAX_SLOT_NBT_BYTES = 65536` — and never parsed). The decoder enforces `MAX_METADATA_ENTRIES = 64` (this plan's cap, pinned by test) and the crate's existing string-length rule, refuses an unknown tag at a named error, and requires the 0x7F terminator. `decode_entity_metadata` for clientbound 0x1C.
  - `oxide_proto_v47::entity::{read_angle, read_fixed_point, read_velocity}` — `read_angle(b: u8) -> f32 = b as f32 * 360.0 / 256.0` (the byte-angle conversion the source applies in its packet handlers — `NetHandlerPlayClient.java:410-411` and `:536-537`; the factors live in the appliers, per plan-style); `read_fixed_point(v: i32) -> f64 = v as f64 / 32.0` (the applier: `NetHandlerPlayClient.java:300-302` and the relative-move handler `:613-628`); `read_velocity(v: i16) -> f64 = v as f64 / 8000.0` (the applier: `NetHandlerPlayClient.java:508`; `S12PacketEntityVelocity` itself carries only the raw short — pin the applier's factor by test).
- Consumes: the crate's existing cursor/error/string helpers and the §2.1 packet ids (0x0C, 0x0E, 0x0F, 0x10, 0x11, 0x2C, 0x1C).

- [ ] **Step 1: Helper tests (RED).** In `entity.rs`: angle literals (`0x00 → 0.0`, `0x40 → 90.0`, `0xC0 → 270.0`, `0x01 → 1.40625`); fixed-point literals (`1 → 0.03125`, `−32 → −1.0`, `96 → 3.0`); velocity literals (`8000 → 1.0`, `−4000 → −0.5`, `1 → 1/8000.0`). Run: `cargo test -p oxide-proto-v47 entity` — fails to compile (no module).
- [ ] **Step 2: Implement the helpers.** Run — green.
- [ ] **Step 3: Spawn fixtures (RED).** In `tests/entity_codecs.rs`: one byte vector per spawn struct, values hand-picked to catch unit errors (a fixed-point coordinate with a fraction, a negative angle byte, a mob with two metadata entries incl. the terminator, an object with nonzero velocity on all axes, a painting facing 3, an orb count 32767). Assert every field. Run — red.
- [ ] **Step 4: Implement the spawn decoders.** Run — green.
- [ ] **Step 5: Metadata suite (RED).** Every tag: a payload byte vector, wrong-tag, truncated payload mid-entry, missing terminator, 65 entries, a string at the length cap and one past it, a slot with a −1 id (empty) and one with the NBT tail. Run — red.
- [ ] **Step 6: Implement the metadata decoder.** Run — green.
- [ ] **Step 7: Table completeness.** For `MobType`, `ObjectType`, `GlobalType`: every id §6.3 lists maps to the named variant and back; an id §6.3 does not list returns `None`; a spot table of at least eight literal id ↔ name pairs transcribed from §6.3 (so a shifted table fails loudly). Run — green.
- [ ] **Step 8: Gate and commit.** Run the full gate (Global Constraints). Commit:

```
feat: decode the entity spawns and the metadata block (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`; `cargo fmt --all --check`; `cargo clippy -p oxide-proto-v47 --all-targets -- -D warnings`.

---

### Task 2: The entity codecs II — movement, lifecycle, and the tracked-state packets

**Goal:** The rest of the entity wire surface: velocity, destroy, the relative-move/look family, teleport, head look, collect, attach, equipment and the swing animation — decoded to converted units, with the hostile cases refused.

**Files:**
- Modify: `crates/oxide-proto-v47/src/entity.rs` — the movement and lifecycle structs and decoders.
- Modify: `crates/oxide-proto-v47/src/lib.rs` — re-exports.
- Test: `crates/oxide-proto-v47/src/entity.rs` — inline tests (conversions, hostile cases).
- Test: `crates/oxide-proto-v47/tests/entity_codecs.rs` — extend the fixture corpus.

**Interfaces:**
- Produces (fields after conversion — decode converts once, nobody re-converts):
  - `EntityVelocity { entity_id: i32, velocity: [f64; 3] }` (0x12; `read_velocity` per axis).
  - `DestroyEntities { entity_ids: Vec<i32> }` (0x13; one VarInt count then ids — derive the exact count encoding from §2.1 and pin it; refuse a count > `MAX_DESTROY_BATCH = 1024`).
  - `Entity { entity_id: i32 }` (0x14; the no-op the source answers by ignoring — decoded, and the session's handler does nothing but log at debug).
  - `EntityRelativeMove { entity_id: i32, delta: [f64; 3] }` (0x15; each axis a signed byte divided by 32 exactly once — `d as f64 / 32.0`; pin the factor with ±127 → ±3.96875).
  - `EntityLook { entity_id: i32, yaw: f32, pitch: f32 }` (0x16; angle bytes).
  - `EntityLookAndRelativeMove { entity_id: i32, delta: [f64; 3], yaw: f32, pitch: f32 }` (0x17).
  - `EntityTeleport { entity_id: i32, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, on_ground: bool }` (0x18; fixed-point coordinates).
  - `EntityHeadLook { entity_id: i32, head_yaw: f32 }` (0x19).
  - `CollectItem { collected: i32, collector: i32 }` (0x0D).
  - `AttachEntity { attached: i32, holder: i32, leash: bool }` (0x1B).
  - `EntityEquipment { entity_id: i32, slot: i16, item: Option<MetadataItem> }` (0x04; the slot id per §2.1 — derive its range and refuse outside it; `None` for the empty slot id).
  - `Animation { entity_id: i32, animation: u8 }` (0x0B).
- Consumes: Task 1's `MetadataItem`, `read_angle`, `read_fixed_point`, `read_velocity`; the crate's cursor helpers.

- [ ] **Step 1: Fixtures (RED).** Extend `tests/entity_codecs.rs`: per struct a byte vector, including a velocity with negatives, a destroy batch of three ids, a relative move of (−128, 0, 127), a look pair with the wrap angle, a teleport on a fixed-point boundary, equipment in a non-zero slot, and `None` equipment. Run — red.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: Hostile suite.** Truncated bodies, an oversized destroy count, an out-of-range equipment slot, a trailing byte after each body (refused as the crate refuses trailing bytes elsewhere — match the crate's existing convention). Run — green.
- [ ] **Step 4: Gate and commit.** Commit:

```
feat: decode the entity movement and lifecycle packets (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`.

---

### Task 3: The UI codecs — chat, the player list, the tab header, the scoreboard, and teams

**Goal:** The remaining server surfaces M4 reads — the chat message, the player list's remaining actions, the tab-list header/footer, and the four scoreboard packets with every mode — plus the serverbound chat writer.

**Files:**
- Create: `crates/oxide-proto-v47/src/ui.rs` — the structs, their decoders, and the chat writer's payload encoder.
- Modify: `crates/oxide-proto-v47/src/clientbound.rs` — `PlayerListItem` gains actions 1–4 (gamemode, latency, display name, remove) beside today's add-action-only decode, which refuses them (`:437-448`); keep the existing type names and extend.
- Modify: `crates/oxide-proto-v47/src/lib.rs`; `crates/oxide-proto-v47/src/serverbound.rs` — the chat packet (0x01) arm if the crate's write side lives there (follow the crate's existing split for write helpers).
- Test: `crates/oxide-proto-v47/src/ui.rs` — inline tests; `crates/oxide-proto-v47/tests/ui_codecs.rs` — fixtures.

**Interfaces:**
- Produces:
  - `ChatMessage { text: String, position: i8 }` (0x02; `text` is the raw JSON string — the session ships it verbatim, parsing is `oxide-game`'s; refuse nothing but a length past the crate's string cap).
  - `PlayerListItem` extension: the action's remaining modes — gamemode (u8), latency (i32, kept as sent including negatives), display name (present or null), remove — derived beside the existing `PlayerListEntry` per §2.1; the action-0 struct's fields stay exactly as today, and one fixture per action is pinned.
  - `TabHeaderFooter { header: String, footer: String }` (0x47; raw JSON strings).
  - `ScoreboardObjective { name: String, mode: u8, value: Option<String>, kind: Option<String> }` (0x3B; mode 0 carries value + kind, 1 removes, 2 carries value + kind (the source reads both for modes 0/2) — derive the mode table from §2.1 and refuse an unlisted mode).
  - `ScoreboardScore { entry: String, mode: u8, objective: Option<String>, value: Option<i32> }` (0x3C; mode 0 set, 1 remove).
  - `ScoreboardDisplay { slot: u8, objective: Option<String> }` (0x3D; the slot byte is hostile — refuse anything outside the §2.1 set).
  - `ScoreboardTeam { name: String, mode: u8, display_name: Option<String>, prefix: Option<String>, suffix: Option<String>, friendly_flags: Option<u8>, name_tag_visibility: Option<String>, colour: Option<u8>, players: Option<Vec<String>> }` (0x3E; modes 0 create, 1 remove, 2 update info, 3 add players, 4 remove players — derive the per-mode field set from §2.1 and pin one fixture per mode; player-list length capped at `MAX_TEAM_PLAYERS = 512`).
  - `oxide_proto_v47::ui::write_chat(message: &str) -> Vec<u8>` (the 0x01 payload: a VarInt-length string; the 100-char cap is the caller's field rule, the encoder writes what it is given and the crate's string length rule still applies — pin that split with a test).
- Consumes: the crate's cursor, string and VarInt helpers.

- [ ] **Step 1: Fixtures (RED).** Per struct and per mode a byte vector (an objective create with value and kind; a score set negative; a display set and one clearing; a team create with colour, flags, visibility and three players; team add/remove-players; a team update with only prefix). Run — red.
- [ ] **Step 2: Implement the decoders.** Run — green.
- [ ] **Step 3: The player-list actions.** Fixtures first (red): gamemode, latency with a negative and a zero, display name present and null, remove; today's refusal test flips to acceptance. Then implement the extension (green).
- [ ] **Step 4: Hostile suite.** Unknown modes, missing optional fields in the modes that require them, a display slot outside the set, a 513-player team, truncated strings. Run — green.
- [ ] **Step 5: The chat writer.** Test the byte layout (`0x01` id byte + VarInt length + UTF-8) and a round-trip through the crate's own reader. Run — green.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: decode the chat, player-list and scoreboard packets (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`.

---
### Task 4: The entity store

**Goal:** `oxide-world` gains the entity table: the closed `EntityKind` enum with its per-kind data, the `Entity` state (pose pairs, velocity, flags, raw metadata, equipment, animation counters), and the mutation and tick operations the session applies. No session wiring yet.

**Files:**
- Create: `crates/oxide-world/src/entity.rs` — `Entities`, `Entity`, `EntityKind`, `KindData`, the operations and `tick`.
- Modify: `crates/oxide-world/src/lib.rs` — module declaration and re-exports.
- Test: `crates/oxide-world/src/entity.rs` — inline unit tests; `crates/oxide-world/tests/entities.rs` — the scripted scenario.

**Interfaces:**
- Produces:
  - `oxide_world::entity::EntityKind` — closed enum: `Player`, `Item`, `XpOrb`, `Arrow`, `Snowball`, `Egg`, `EnderPearl`, `EyeOfEnder`, `Potion`, `XpBottle`, `Firework`, `Fireball`, `SmallFireball`, `WitherSkull`, `Painting`, `ItemFrame`, `Boat`, `Minecart`, `Global` (lightning and friends — tracked, undrawn), one variant per `MobType` member (Task 1's roster, e.g. `Creeper … Villager`), and `Unknown`. `oxide-game` maps wire type → kind; the store never sees a wire type.
  - `oxide_world::entity::KindData` — the spawn-given extras: `None`, `Item { id: i16, count: u8, damage: i16 }`, `Painting { title: Arc<str>, facing: u8 }`, `XpOrb { count: i16 }`, `Minecart` (plain flag; the cargo variants are their own kinds), `Boat`. Everything else the renderer needs that arrives in metadata stays in the raw metadata map and is extracted by the session (Task 5) — the store does not grow an accessor per mob field.
  - `oxide_world::entity::Entity` — public fields: `id: i32`, `kind: EntityKind`, `uuid: Option<String>` (players only), `position: [f64; 3]`, `last_tick_position: [f64; 3]`, `yaw: f32`, `last_tick_yaw: f32`, `pitch: f32`, `last_tick_pitch: f32`, `head_yaw: f32`, `last_tick_head_yaw: f32`, `render_yaw_offset: f32`, `prev_render_yaw_offset: f32`, `velocity: [f64; 3]`, `on_ground: bool`, `metadata: oxide_proto_v47::entity::Metadata` (the raw map; `oxide-world` already takes the v47 edge), `equipment: [Option<MetadataItem>; 5]`, `attachment: Option<Attachment>` where `Attachment { holder: i32, leash: bool }`, `age: u32`, `limb_swing: f32`, `limb_swing_amount: f32`, `last_limb_swing_amount: f32`, `swing_progress: f32`, `last_swing_progress: f32`, the swing state machine's private counter, `hurt_ticks: u16`, `death_ticks: u16`, and `data: KindData`.
  - `oxide_world::entity::Entities` — `new()`, `clear()`, `len()`, `is_empty()`, `get(id)`, `get_mut(id)`, `iter()` (ascending id — deterministic iteration for tests and the feed), `insert(entity)` (upsert: a spawn for a live id replaces it), `remove(ids: &[i32]) -> usize`, `apply_relative_move(id, delta: [f64; 3])` (adds; called with the already-converted /32 deltas), `apply_look(id, yaw, pitch)`, `apply_head_look(id, head_yaw)`, `apply_teleport(id, position, yaw, pitch, on_ground)` (absolute), `apply_velocity(id, velocity)`, `apply_metadata(id, metadata)` (merge by index — present indices replace, absent stay), `apply_status(id, status: i8)` (the source's `handleStatusUpdate` map, derived in-task: 2 → `hurt_ticks` = the source's hurt window, 3 → the death tick starts; every other status is a debug log, and the statuses with render meaning outside this milestone are listed in the doc comment), `set_equipment(id, slot: i16, item: Option<MetadataItem>)`, `set_attachment(...)`, and `tick()`.
  - `Entities::tick()` — once per session tick: for every entity, copy the pose pairs (`last_tick_position = position`; body yaw, pitch, head yaw and `render_yaw_offset` each to its tick-partner), advance `age`, advance the limb-swing pair and `swing_progress` per the source's own updates — `EntityLivingBase.onLivingUpdate` for the limb pair, `updateArmSwingProgress` for the swing state, `onEntityUpdate` for the hurt countdown *(corrected after execution — the original function cites)*; derive the exact steps: the `×4.0` distance factor and the `0.4` easing for the limb pair; the swing state machine's counter, period and easing — the stored progress runs `0, 1/6 … 5/6` and resets, never reaching `1.0` *(corrected after execution — the source's own sequence)*; pin each with a literal test. Count `hurt_ticks` down to zero and `death_ticks` up from a zero seed set when status 3 arrives — the source's `deathTime` *(corrected after execution)*. Advance `render_yaw_offset` per the source's own chase, both component paths *(corrected after execution — the target is not the head yaw)*: the base path (players) eases `0.3` of the wrapped difference toward the tick's movement direction, or the body yaw while `swingProgress > 0`, else held, then bounds the result within `±75` of the body yaw and adds the past-`50` release; the mob path hands the body toward the head through the body helper — moving, the body snaps to the body yaw and the head bounds to `±75` of it; still, the body bounds to `±75` of the head under the held-head decay — port `EntityBodyHelper` exactly, its two fields included. After the tick, fold every copied pose pair within `±180` of its current value (the source's own range checks). Derive the wrap-around handling everywhere and pin the seam crossing with a test.
- Consumes: `oxide_proto_v47::entity::{Metadata, MetadataItem}` (the allowed `oxide-world -> oxide-proto-v47` edge), `oxide-proto` basics.

- [ ] **Step 1: Store operations (RED).** Inline tests: insert/get/iter order; upsert replaces; remove reports its count; relative move adds exactly (`0.5 + 127/32`); teleport overwrites (and does not touch the pair — the pair copies at tick); look/head-look set; metadata merges (index 2 replaced, index 0 kept); status 2 and 3 set their counters and an unknown status changes nothing; equipment slots land. Run: `cargo test -p oxide-world entity` — fails to compile.
- [ ] **Step 2: Implement `entity.rs`.** Run — green on Step 1.
- [ ] **Step 3: Tick arithmetic (RED).** The limb pair against hand-computed literals: from rest, a steady 0.2-blocks-per-tick straight-line walk for one tick gives the source's amounts (compute `sqrt(0.04) × 4` → clamped 0.8 → amount 0.32 for the first tick, then the eased sequence to the fixed point); the swing: `swingItem()` then tick through the cycle — the stored progress runs `0, 1/6 … 5/6` then resets, never `1.0` (pin the sequence, the restart-after-halfway guard and the 6-tick period *(corrected after execution — the source's own sequence)*); hurt counts down from the source's window; death counts up from its zero seed; the render-yaw seam crossing (body at `170°` against a head at `−170°`) follows both chase paths' own source values — the mob bound-snap and the player ease. Run — red.
- [ ] **Step 4: Implement `tick()`.** Run — green.
- [ ] **Step 5: The scripted scenario.** In `tests/entities.rs`: spawn a player, a mob and an item; move them for 40 ticks with relative moves and one teleport; metadata updates midway; one status; despawn one; assert the end state and that the pose pairs track the last tick exactly. Run — green.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: add the entity table to the world model
```

**Verification:** `cargo test -p oxide-world`; clippy clean on the crate.

---

### Task 5: The session's entity tracking and the per-tick feed

**Goal:** The session applies every entity packet into the store, ticks the store with the tick loop, and publishes one `EntitiesTick` per tick carrying the frame every window surface draws from; the player list decodes fully and names compose.

**Files:**
- Create: `crates/oxide-game/src/entity_view.rs` — `EntityFrame`, `EntityExtra`, the snapshot builder, and the per-kind metadata extraction (§6.2 index tables).
- Modify: `crates/oxide-game/src/session.rs` — the new decode arms, the store field, the tick body's `entities.tick()` and feed emission, the extended 0x38 handling, the display-name composition, the respawn/clearing rules.
- Modify: `crates/oxide-game/src/lib.rs` — module declarations.
- Test: `crates/oxide-game/src/entity_view.rs` — inline (extraction tables); `crates/oxide-game/tests/session_replay.rs` — spawn/move/despawn, feed cadence, the keepalive burst.

**Interfaces:**
- Produces:
  - `oxide_game::entity_view::EntityFrame` — `{ id: i32, kind: EntityKind, uuid: Option<String>, prev: [f64; 3], pos: [f64; 3], prev_yaw: f32, yaw: f32, prev_pitch: f32, pitch: f32, prev_head_yaw: f32, head_yaw: f32, render_yaw_offset: f32, prev_render_yaw_offset: f32, on_ground: bool, invisible: bool, sneaking: bool, age: u32, limb_swing: f32, limb_swing_amount: f32, prev_limb_swing_amount: f32, swing_progress: f32, prev_swing_progress: f32, hurt_ticks: u16, death_ticks: u16, brightness: f32, health: Option<(f32, f32)>, nametag: Option<Arc<str>>, extra: EntityExtra }` (derive `Debug, Clone, PartialEq`). `invisible` and `sneaking` are the metadata flag byte's own bits — derive which bits (`Entity.java`'s flag setters/spec §6.2) and pin them. `prev_head_yaw` and `prev_limb_swing_amount` are interpolation partners (the store's `last_tick_*` values). `brightness` is the light sample at the entity's feet, computed here in the session — the client holds no world snapshot; the session reads the same light data the mesher consumes (derive the source's float `Entity.getBrightness` chain and pin it). `health` is the living-entity health with the kind's maximum (§6.2 index; the boss kinds' maxima are their class constants) — the boss bar's input (Task 18).
  - `oxide_game::entity_view::EntityExtra` — `None`, `Player`, `Item { id: i16, count: u8, damage: i16 }`, `Painting { title: Arc<str>, facing: u8 }`, `ItemFrame { item: Option<MetadataItem>, rotation: u8 }`, `Boat`, `Minecart`, `Projectile`, `Orb`, and `Mob(MobExtra)`; `MobExtra` is an enum with one variant per mob whose render reads metadata — at minimum `Sheep { wool: u8, sheared: bool }`, `Wolf { tamed: bool, collar: u8 }`, `Slime { size: u8 }`, `Ocelot { variant: u8 }`, `Rabbit { variant: u8 }`, `Villager { profession: u8, child: bool }`, `Bat { hanging: bool }`, `Pig { saddle: bool }`, `Horse { variant: u8, colour: u8, tamed: bool, saddle: bool, adult: bool }`, `Zombie { villager: bool }`, `Creeper`, `Enderman`, `Other`. Every field's metadata index comes from protocol §6.2 and is pinned by one extraction test per variant (build a `Metadata` from the §6.2 index table, assert the mapped fields; build an empty map, assert the defaults). The frame's `health` pair and `brightness` get their pins here too.
  - `oxide_game::entity_view::snapshot(&Entities) -> Vec<EntityFrame>` — one frame per entity, ascending id; the nametag composed by the session's display function; `None` when the name must not show (`name_visible` false and no custom name, per the source's `RendererLivingEntity.renderName`-side rule — the "always show" flag is metadata, derive its index).
  - `ClientEvent::EntitiesTick { entities: Vec<EntityFrame> }` — emitted once per tick, immediately after `PlayerTick`; identical consecutive ticks are still emitted (the window's interpolation needs the cadence; no change-detection — that is `PlayerTick`'s own rule).
  - Extended player list: the 0x38 arm handles all five actions (Task 3 extends `PlayerListItem`'s decode to actions 1–4 — today's refuses everything past action 0, `clientbound.rs:437-448`; the session stops skipping the non-zero actions): a store `PlayerList` (`BTreeMap<String /*uuid*/, PlayerListRecord { name: String, properties: Vec<(String, String)>, gamemode: u8, latency: i32, display_name: Option<String /*raw JSON*/> }>` — deliberately distinct from the wire `PlayerListEntry`, which stays the transport shape), with removal, gamemode/latency/display-name updates, and login-time population.
  - The display-name composition: `fn display_name(...)` — for players: the list entry's display name when present, else the account name; composed into `§`-coded text (Task 6 extends it with the team clauses; the function's signature takes what it needs and is the only composition point).
  - Respawn/clearing: a dimension rebuild (`WorldCleared`) also clears the entity store and the list; a same-dimension respawn keeps both (the source's `handleRespawn` rule; screen the disconnect path too).
- Consumes: the store (Task 4), the codecs (Tasks 1–2), the existing tick body and event channel.

- [ ] **Step 1: Extraction tables (RED).** In `entity_view.rs`: the per-variant tests above; the flag-bit pins; the nametag visibility rule (three cases: no name, name without the always-show flag, name with it). Run: `cargo test -p oxide-game entity_view` — fails to compile.
- [ ] **Step 2: Implement `entity_view.rs`.** Run — green.
- [ ] **Step 3: Session arms (RED).** In `session_replay.rs`: a scripted server sends spawn player + spawn mob + spawn object + metadata + relative moves + look + teleport + head look + velocity + status + equipment + attach + collect + destroy + the 0x14 no-op (ignored, logged) + a spawn for an id the tables cannot name (tracked as `Unknown`, session continues); assert store state per event and that the keepalive stays green. Run — red.
- [ ] **Step 4: Implement the arms and the store field.** Run — green.
- [ ] **Step 5: Feed cadence.** Assert exactly one `EntitiesTick` per tick, immediately after `PlayerTick`, carrying a full set (add/remove reflected in the next tick's set), and that a burst of relative moves between ticks does not emit extra events. Run — green.
- [ ] **Step 6: The player list.** Session tests: add/update/remove actions; a spawn player whose list entry arrived first keeps its name (the §8 ordering obligation); a name composed from a display name; the list survives an ordinary respawn and dies with a rebuild. Run — green.
- [ ] **Step 7: The keepalive burst.** Extend the existing burst test with a flood of entity packets; the keepalive behaviour is unmodified. Run — green.
- [ ] **Step 8: Gate and commit.** Commit:

```
feat: track entities in the session and publish a per-tick feed
```

**Verification:** `cargo test -p oxide-game`; `cargo test -p oxide-world`.

---

### Task 6: The session's player list, scoreboard, and tab text

**Goal:** The remaining session-side UI state: the scoreboard model with its teams, the three snapshots published on change, the team clauses in the display-name composition, and the carried `i64::MIN` Time Update pin.

**Files:**
- Create: `crates/oxide-game/src/scoreboard.rs` — the state model and the team-formatting functions.
- Modify: `crates/oxide-game/src/session.rs` — the 0x3B/0x3C/0x3D/0x3E/0x47 arms, the state fields, the change-detected events, the composition extension, the `i64::MIN` receive guard and its test.
- Modify: `crates/oxide-game/src/lib.rs`; `crates/oxide-game/src/entity_view.rs` — the composition call sites gain the team source.
- Test: `crates/oxide-game/src/scoreboard.rs` (inline) and `crates/oxide-game/tests/session_replay.rs`.

**Interfaces:**
- Produces:
  - `oxide_game::scoreboard::Scoreboard` — `objectives: BTreeMap<String, Objective>` with `Objective { name: String, value: String, kind: String }`; `scores: BTreeMap<String /*entry*/, BTreeMap<String /*objective*/, i32>>`; `display: [Option<String>; 3]` (slot 0 list, 1 sidebar, 2 below-name — the §2.1 slot table); `teams: BTreeMap<String, Team>` with `Team { display_name: String, prefix: String, suffix: String, friendly_flags: u8, name_tag_visibility: String, colour: Option<u8>, players: BTreeSet<String> }`; and the reverse membership `member_of: BTreeMap<String, String>` kept consistent by the mutators (add/remove/update per the packet modes; a removal drops memberships with it). All mutators are `&mut self` with per-mode tests.
  - `oxide_game::scoreboard::format_entry(board: &Scoreboard, entry: &str, fallback: &str) -> String` — the team composition: `[prefix][colour][fallback][suffix]` per the source's applied rules (derive from `Team.java` and the `getFormattedText` call sites; the colour applies only where the source applies it — pin with fixtures: a team with prefix and colour, a team with suffix only, no team).
  - `scoreboard::entry_colour(board, entry) -> Option<u8>` — for the tab list and nametag colour paths (Task 17's own consumers).
- Events, all change-detected (emitted only when the value actually changed — the tests count events):
  - `ClientEvent::PlayerList { entries: Vec<PlayerListRecord> }` — the full entry set (the Task 5 struct), ascending uuid; emitted on any add/update/remove.
  - `ClientEvent::ScoreboardChanged { board: Scoreboard }` — the full state (it is small; snapshot-on-change keeps the window free of merge logic).
  - `ClientEvent::TabText { header: String, footer: String }` — raw JSON strings from 0x47.
- The composition extension: `display_name` (Task 5) consults `format_entry`; player nametags and the tab list therefore carry team formatting from one place.
- The carried pins (STATE item 8, both routed here): the Time Update receive path guards `i64::MIN` before the negation so the frozen marker cannot overflow (derive the exact source rule from `S03PacketTimeUpdate` and the session's existing receive path; add the guard + a session test that feeds `i64::MIN` and asserts no panic and the defined state); and the mesh queue's end-drain expiry path gains its pin per the M2 deferred item's own text (`refs/m2-final-review/deferred-minors.md` item 31(8) — read its exact wording first; assert the bounded expiry by a test on the session's end path, or where the surface cannot synthesise the case, record that finding and re-carry it explicitly rather than landing a weak pin).
- Consumes: codecs (Task 3), Task 5's composition and list.

- [ ] **Step 1: The state model (RED).** Inline tests: objective create/update/remove; scores set/remove per objective; display set/clear per slot; team create/update/add-players/remove-players/remove, with membership consistency after each; `format_entry` fixtures (prefix+colour, suffix-only, none, a name that is itself `§`-coded). Run: `cargo test -p oxide-game scoreboard` — fails to compile.
- [ ] **Step 2: Implement `scoreboard.rs`.** Run — green.
- [ ] **Step 3: The session arms (RED).** `session_replay.rs`: scripted scoreboard and team sequences drive the three events; a redundant packet emits nothing (change detection); the display composition of a player on a team carries the prefix/colour/suffix in the `EntitiesTick` frame and the `PlayerList` entry. Run — red.
- [ ] **Step 4: Implement the arms, events and composition call.** Run — green.
- [ ] **Step 5: The `i64::MIN` pin.** The guard + test (feed the marker; assert the frozen state and no panic). Run — green.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: track the scoreboard and team state in the session
```

**Verification:** `cargo test -p oxide-game`.

---

### Task 7: Skins — the cache, the default rule, and the fetch

**Goal:** `oxide-assets` gains the skin pipeline — the default-skin rule, the profile-property decode, the disk cache and the fetch — and the client gains the worker thread that runs it off the window thread.

**Files:**
- Create: `crates/oxide-assets/src/skins.rs` — `SkinCache`, `default_skin`, `decode_profile_property`, `fetch_skin`.
- Modify: `crates/oxide-assets/src/lib.rs`; `crates/oxide-assets/Cargo.toml` and the root `Cargo.toml` — the `base64 = "0.23"` dependency (workspace table + the crate; 0.23 matches the lock's existing copy — `ureq` already brings 0.23.1, and 0.22 would duplicate the crate).
- Create: `crates/oxide-client/src/skin_worker.rs` — the worker and its messages.
- Modify: `crates/oxide-client/src/main.rs` — worker spawn on session start and the request feed from player-list changes (`PlayerList` events arm).
- Test: `crates/oxide-assets/src/skins.rs` (inline); `crates/oxide-assets/tests/skins_live.rs` — the ignored live fetch; `crates/oxide-client/src/skin_worker.rs` (inline, with a stub fetcher).

**Interfaces:**
- Produces:
  - `oxide_assets::skins::DefaultModel` — `Wide | Slim`.
  - `oxide_assets::skins::default_skin(uuid: &str) -> DefaultModel` — the `DefaultPlayerSkin` rule: parse the hyphenated UUID to its two 64-bit halves (a hand parser; refuse anything not in the canonical shape with `None`-style handling — the caller falls back to `Wide`), compute the Java `UUID.hashCode` (derive the exact fold and the hash-to-model bit from `UUID.hashCode` + `isSlimSkin` and pin golden vectors), and map the low bit exactly as `isSlimSkin` does (the source's method names: `getDefaultSkin`/`getSkinType`/`isSlimSkin`, `:25-44`). Golden vectors: at least three UUIDs with hand-computed outcomes, including one offline-mode-shaped UUID.
  - `oxide_assets::skins::decode_profile_property(value_b64: &str) -> Result<ProfileTexture, SkinError>` — base64 (the new dependency; the decoder configured strictly — reject invalid characters, honour padding) → UTF-8 → a `serde_json` walk of the property JSON: `textures.SKIN.url` (string, `http`/`https` shape checked) and `textures.SKIN.metadata.model` (`"slim"` or absent). Caps: input ≤ 4 KiB decoded, URL ≤ 512 bytes. A malformed value is an error, never a panic; the caller falls back to the default. (The `CAPE` entry is decoded alongside the skin — field `cape_url: Option<String>`; the worker fetches it and the pass draws it per Task 8's rule.)
  - `oxide_assets::skins::SkinCache` — `new(store: &Store)` resolving `skins/` under the store; `path_for(url) -> Option<PathBuf>` (the URL's last path segment after a hex/length check — refuse a shaped-wrong URL); `load(url)` from disk when present; `store(url, png_bytes)` after decode (atomic write via the existing tempfile pattern); `fetch(url)` through the existing `http.rs` layer (its agent and retry rules unchanged), decode through the existing PNG path, size rule: 64×64 accepted; anything else is an error with the dimensions named (the 64×32 legacy conversion is a recorded limit, Listed in Known limits).
  - `oxide_client::skin_worker` — `SkinRequest { uuid: String, property: Option<String> }`, `SkinUpdate { uuid: String, texture: Option<Arc<Texture>>, cape: Option<Arc<Texture>>, model: DefaultModel }` (`None` texture = use the default; the worker decodes the property and fetches the cape's URL through the same fetcher when one is present — a failed skin or cape fetch is `None` and never retried for that uuid); `spawn(fetch: impl Fn(&str) -> Result<Arc<Texture>, SkinError> + Send + 'static) -> (Sender<SkinRequest>, Receiver<SkinUpdate>)` and the loop function, generic over the fetcher so the tests run a stub; dedupe by (uuid, property-hash) so list churn does not refetch; stop when the request channel closes. The client sends a request per player list entry with a `textures` property and receives updates into a map the Task 8 renderer uploads.
- Consumes: `http.rs`, `store.rs`, `texture.rs`/the PNG decode, `serde_json`; the client's event loop.

- [ ] **Step 1: The default rule (RED).** Golden vectors and the refuse cases (a short string, a non-hex segment). Run: `cargo test -p oxide-assets skins` — fails to compile.
- [ ] **Step 2: Implement `default_skin`.** Run — green.
- [ ] **Step 3: The property decode (RED).** Synthetic fixtures: the test builds the property JSON, base64-encodes it with the new dependency, and asserts the decoded url/model; a slim variant; a cape-bearing variant (asserts `cape_url: Some` — a cape-less one asserts `None`); malformed cases (not base64, not JSON, missing SKIN, a URL with a bad scheme, an oversized blob). Run — red.
- [ ] **Step 4: Implement the decode and `SkinCache`.** Run — green. (The cache tests use a `tempfile` store and a synthetic 64×64 PNG encoded in-test.)
- [ ] **Step 5: The worker.** Inline tests with the stub fetcher: requests dedupe; a failing uuid yields one `None` update and no retry; a cape-bearing request fetches both URLs (the stub counts the two fetches) and a failing cape fetch still yields the skin update with `cape: None`; channel close ends the thread. Run — green.
- [ ] **Step 6: The client wiring.** `main.rs` spawns the worker when the session starts and forwards each player-list entry with a textures property; updates land in the client's `skins: BTreeMap<String, SkinUpdate>` (Task 8 renders from it); on session end the worker's channels drop and join at exit. Run — `cargo test -p oxide-client`.
- [ ] **Step 7: The ignored live fetch.** `tests/skins_live.rs`, `#[ignore]`: fetch one fixed public skin URL through `SkinCache::fetch`, assert a 64×64 texture decodes. Run: `cargo test -p oxide-assets -- --ignored skins_live`.
- [ ] **Step 8: Gate and commit.** Commit:

```
feat: fetch and cache player skins
```

**Verification:** `cargo test -p oxide-assets`; `cargo deny check` (the new dependency).

---
### Task 8: The entity pass and the player model

**Goal:** The renderer gains the box-model framework, the texture registry, the entity pass drawn between the terrain's solid and translucent layers, and the player model (wide and slim, the skin-part overlays, the cape, hurt and death states, the walk/head-swing pose, the sneak pose) — and the client builds per-frame entity draws from the tick feed and uploads skins as they arrive. After this task two clients on the rig see each other.

**Files:**
- Create: `crates/oxide-render/src/entity_models/mod.rs` (the framework: box/part tables, the pose input, the transform builder, the vertex builder, the model registry); `crates/oxide-render/src/entity_models/player.rs` (ModelPlayer — the source's skin/overlay conventions; Tasks 9–11 add sibling modules).
- Create: `crates/oxide-render/src/entity_pass.rs` — `EntityPass`, `EntityDraw`, `TextureRef`, `ModelRef`, `DrawExtra`.
- Modify: `crates/oxide-render/src/renderer.rs` — the texture registry (`set_entity_texture`, `set_default_skins`, `set_skin`), `set_entities`, the `SceneDraw::Entities` slot and the terrain split, the pass's invocation.
- Modify: `crates/oxide-render/src/lib.rs`, `crates/oxide-render/src/terrain_pass.rs` (the solid/translucent split).
- Create: `crates/oxide-client/src/view.rs` — the entity view (frames + arrival + fraction), the draw assembly (interpolation, snap, light sample, texture resolution, own-entity skip), the skin upload path.
- Modify: `crates/oxide-client/src/main.rs` — the `EntitiesTick` arm, the per-frame draw build, the skin-update arm, the static entity-texture upload at startup.
- Test: `entity_models/mod.rs` inline (the transform and vertex builders); `entity_models/player.rs` inline (geometry and pose literals); `entity_pass.rs` inline; the ignored GPU file(s) the M3 plan's Task 12 created (new cases).

**Interfaces:**
- Produces:
  - `oxide_render::entity_models::{Model, Part, Box, Rot, Pose, build_vertices}` — the framework: a model is a tree of parts (pivot point, rest rotation, a rotation slot the model's pose function writes, child parts) with per-part box lists (origin, size, UV cell, inflate, mirror, all in the source's 1/16 model units); `Pose` carries the shared inputs (limb swing pair, age, head yaw/pitch, body yaw, sneak, swing progress, hurt and death fractions, child flag) plus a per-model extension the tasks declare; `build_vertices(model, part transforms, texture size) -> (positions, uvs, shaded-face order)` emits the six quads per non-degenerate box with the source's UV conventions (derive the face UV arithmetic from `ModelBox`/`TexturedQuad`; pin one box's 24 UVs and 24 positions by literal test, mirror included). The registry also pairs every kind with its source entity-class height (`f32` — the size constant its own class sets; the player's `1.8` (`EntityPlayer.java:580`) is declared here, Tasks 9–11 pin theirs with their models — the nametag offset's source, read by Task 13). The vertex output feeds the entity pass's one shared pipeline; the pass owns the buffers.
  - `oxide_render::entity_models::player::MODEL_PLAYER_WIDE` / `MODEL_PLAYER_SLIM` — the `ModelPlayer` geometry: the source's boxes with the slim arm variant (3-px arms and their UV shift) and the overlay parts (hat, jacket, sleeves, pant legs) with their inflate values and overlay UVs; a box-count test plus at least six literal box/UV pins across body, head, arms and one overlay. The pose function: the source's `setRotationAngles` for the biped — head yaw/pitch clamp, the limb-swing arm and leg sways, the sneak adjustments (body lean and offsets), the arm-swing term over `swing_progress`, and the **cape**: `LayerCape`'s box animated by the source's own wave rule (derive; pin a two-frame literal), drawn only when the parts byte enables it and a cape texture is present.
  - `oxide_render::entity_pass::{ModelRef, TextureRef, DrawExtra, EntityDraw}` — `ModelRef` starts with `Player { slim: bool, parts: u8 }` — the parts byte is the model-parts set (`EnumPlayerModelParts`; the source gates the overlays through `isWearing`, whose backing byte is the player's own skin-flags metadata — `RenderPlayer.java:81-86`); this milestone pins the all-on default (`0x7F`) for every player with the settings-screen seam (the per-player byte and the local settings source are recorded in Known limits; Tasks 9–11 extend; every extension is declared in its own task); `TextureRef { Named(&'static str), Skin { uuid: String, slim: bool } }`; `DrawExtra` starts with `None`; `EntityDraw` — `{ model: ModelRef, position: [f64; 3], body_yaw: f32, head_yaw: f32, head_pitch: f32, pose: Pose, texture: TextureRef, light: f32, hurt: f32, death: f32, health: Option<(f32, f32)>, extra: DrawExtra }` (the nametag field arrives with Task 13; `health` is the boss kinds' pair from the frame — the pass raises the boss status from it, Task 18).
  - `oxide_render::entity_pass::EntityPass` — `draw(frame inputs)` consuming the renderer's current `Vec<EntityDraw>`; per draw: translate to the interpolated position, rotate `180 − body_yaw` (the source's `renderLivingAt` composition — derive and pin), apply the sneak drop and the death tilt, scale by 1/16, draw the model's parts through the shared pipeline at the entity brightness with the frame's fog; shadow quad from `misc/shadow.png` at the source's size and alpha per kind (derive; the item class's `0.15`/`0.75` are checklist item 43's numbers and are re-derived here); hurt overlay = a second draw of the same geometry with the source's `(1, 0, 0)` colour and alpha gated on `hurt_ticks` (derive the alpha and the depth/state handling; checklist item 45's numbers are the target); death tilt per checklist item 45's `20`-tick, `1.6`-factor formula (derive; pin a three-point literal test).
  - `Renderer::{set_entity_texture, set_default_skins, set_skin, set_entities}` and the `SkinLookup` trait — `resolve(uuid, slim)` returning the uploaded skin or the wide/slim default (the resolver the entity pass and the tab list share; Task 16 consumes it) — implemented over the registry's skin map; the registry: named textures uploaded once (`Named` keys are texture paths the client passes; a missing key is a pinned placeholder — the atlas's missing sprite governs — and a debug log, never a panic); default skins for the wide/slim fallback; per-uuid skins — each skin update's skin and cape textures together (re-upload replaces; an absent cape clears it); `set_entities` replaces the frame's list (the same discipline as the other setters: before `render`, window-side each frame).
  - `Renderer` scene order: `scene_draws` yields sky → clouds → terrain solid (opaque + cutout) → **entities** → terrain translucent → world overlay; the terrain pass gains the two-call split (`draw_solid`, `draw_translucent`) with its queues unchanged; the pass draws depth-testing and depth-writing, before Dim/Overlay as today.
  - `oxide_client::view::View` (the client's frame state beside `HudState`): `apply(&mut self, event: &ClientEvent)` — the `EntitiesTick` arm stores the frames and the arrival instant; `entity_draws(&self, now, &skins) -> Vec<EntityDraw>` — fraction from the same arrival the player pose uses, per-entity prev→cur lerp with the >4-block snap, rotation lerp for the pairs, `light` from the frame's `brightness` (the session computes it at the entity's feet; the client holds no world snapshot), `TextureRef::Skin` resolution through the renderer's `SkinLookup` (default fallback by uuid), and the own-entity skip (`id == own entity_id`; the third-person camera is M6's). The per-frame draws read the frame only — no per-frame world lookups.
- Consumes: the tick feed (Task 5), `oxide-assets` textures/font, the existing light and fog uniforms, the client's snapshot and event loop.

- [ ] **Step 1: Framework tests (RED).** Transform composition: a part at pivot (2, 12, 2) rotated 90° about X maps a known box corner to a hand-computed point; a child inherits the parent's transform (one composite literal); the box builder's UV and position literals including a mirrored box and an inflated overlay box; the degenerate-box rule. Run: `cargo test -p oxide-render entity_models` — fails to compile.
- [ ] **Step 2: Implement the framework.** Run — green.
- [ ] **Step 3: The player geometry (RED).** Box counts (wide vs slim), the six literal pins, the slim arm UV shift, the overlay inflates. Run — red.
- [ ] **Step 4: Implement `ModelPlayer` and its pose.** Pose literals: arm swing at `swing_progress = 1.0`; the limb-swing extreme; head pitch passed through as it arrives (`ModelBiped.setRotationAngles` clamps nothing — `ModelBiped.java:131-132`; pin the absence); the sneak offsets; the cape's two-frame rule. Run — green.
- [ ] **Step 5: The pass and the registry.** Draw a player at a known spot through a GPU case with a synthetic 64×64 skin generated in-test: assert the silhouette differs from empty output and a chest-region pixel matches the synthetic colour (the texture, orientation and translate chain in one probe); the hurt tint moves that pixel toward red; the death tilt changes the silhouette's bounding box between two death fractions. Run the crate's ignored GPU suite on the T500: `cargo test -p oxide-render -- --ignored`.
- [ ] **Step 6: The scene-order case.** The GPU case where a translucent quad (synthetic water colour) sits between the camera and the entity: the entity pixel equals the blend, not the raw entity (the source's order); the same case with the order inverted as a fixture must differ (a canary that the case can fail). Run — green.
- [ ] **Step 7: The client view (RED).** Unit tests in `view.rs`: interpolation with two frames and a mid fraction; the snap at >4 blocks; the own-entity skip; the texture resolution fallback; the brightness query on a synthetic snapshot. Run: `cargo test -p oxide-client view` — red.
- [ ] **Step 8: Implement the view, the arms, and the startup upload.** The client uploads the M4 entity-texture set (this task's: `misc/shadow.png`, the default skins; Tasks 9–12 extend the static list in their own tasks, each with its registry test) and the arrow's — no: this task's list only. Wire the skin updates into `set_skin`. Run — green.
- [ ] **Step 9: The rig smoke (manual, optional).** Against the running rig with a second client connected, confirm the other player appears with a nametag-less body and plausible pose (the acceptance proper is Task 22). Record the attempt in the task notes either way.
- [ ] **Step 10: Gate and commit.** Commit:

```
feat: draw entities and the player model
```

**Verification:** `cargo test -p oxide-render`; the ignored GPU suite green; `cargo test -p oxide-client`; fmt/clippy clean.

---

### Task 9: Mob models I — the biped family and the core quadrupeds

**Goal:** The first model family: the zombie, skeleton, villager, witch, giant, snow golem and iron golem, and the quadruped family (pig, cow, sheep, mooshroom) — geometry, pose functions, the layer framework, and the identity layers (zombie arms, sheep wool and shearing, snow golem's head, the golem's flower, the mooshroom's mushrooms, saddles on pigs).

**Files:**
- Create: `crates/oxide-render/src/entity_models/bipeds.rs`, `crates/oxide-render/src/entity_models/quadrupeds.rs`, `crates/oxide-render/src/entity_models/layers.rs`.
- Modify: `crates/oxide-render/src/entity_models/mod.rs` (registry); `entity_pass.rs` (`ModelRef`, `DrawExtra` extensions below); `oxide-client/src/view.rs` (kind → model mapping and the per-kind extra mapping).
- Test: inline per module; the GPU file (one case per new model family). `draws`; the texture set extension test.

**Interfaces:**
- Produces:
  - `entity_models::layers` — the layer framework: a layer draws extra geometry for a model under a condition (a second texture, a tint colour from `DrawExtra`, or a colour byte from a 16-colour palette table — the wool and collar palettes derive from `ItemDye`/`EntitySheep.getDyeColorRgb`-class sources and are pinned by a 16-entry literal test). Layers draw in the source's order after the base model.
  - `ModelRef` extensions: `Zombie`, `Skeleton`, `ZombieVillager`, `Villager { profession: u8, child: bool }`, `Witch`, `Giant`, `SnowGolem`, `IronGolem`, `Pig { saddle: bool }`, `Cow`, `Sheep { wool: u8, sheared: bool }`, `Mooshroom`.
  - `DrawExtra` extensions: `Villager { profession: u8, child: bool }`, `Sheep { wool: u8, sheared: bool }`, `Pig { saddle: bool }`, `ZombieVillager` (as a flag), `None` continues for the rest.
  - Geometry and pose, per the cited classes: `ModelZombie` (the raised-arms pose — `setRotationAngles`' constant arm angle when not attacking — derive and pin the literals), `ModelSkeleton` (the aim pose; derive what state the source reads and pin both states), `ModelZombieVillager`, `ModelVillager` (crossed arms, the child proportions), `ModelWitch` (hat and nose as part of the model; the hold state), `ModelGiant` (the biped at its scale), `ModelSnowMan` (head boxes + arms), `ModelIronGolem` (walk and arm sways), `ModelQuadruped` base (the four-legged limb swing with the source's leg ordering), `ModelPig`, `ModelCow` (+ `ModelCow`'s horns/udder), `ModelSheep2` (body + head + legs; the wool layer as a scaled copy with the palette colour; the sheared variant), `ModelMooshroom` (cow + the mushroom layer — the mushroom draws as the source's block-model re-use; if the source's layer draws a block state, reuse the baked red-mushroom block model through the entity pass's block-item path stub — if that path is Task 12's, this layer defers with it and is recorded).
  - Texture keys added to the pass's static set: `entity/zombie/zombie.png`, `entity/zombie/zombie_villager.png`, `entity/skeleton/skeleton.png`, `entity/villager/villager.png` + the profession overlay keys (derive the table), `entity/witch.png`, `entity/zombie/zombie.png` (the giant renders on the zombie sheet), `entity/snowman.png`, `entity/iron_golem.png`, `entity/pig/pig.png`, `entity/cow/cow.png`, `entity/cow/mooshroom.png`, `entity/sheep/sheep.png` + `entity/sheep/sheep_fur.png` — listed in the registry test; the giant, iron golem and sheep-fur keys corrected after execution (Known limits).
  - The client mapping: `EntityKind` (plus `EntityExtra::Mob` payload) → `ModelRef`/`DrawExtra`, covering this task's kinds; an unmapped kind draws nothing and logs at debug (the gate for Tasks 10–11).
  - Each kind's height as its source entity-class constant, declared in the registry beside its models — pinned per kind (the nametag offset's input; Task 13 reads it).
- Consumes: Task 8's framework and pass; Task 5's frame extras.

- [ ] **Step 1: The layer framework (RED).** Palette literals; layer ordering; the tint path. Run — red.
- [ ] **Step 2: Implement `layers.rs`.** Run — green.
- [ ] **Step 3: Geometry tests (RED).** For every model above: box count + ≥4 literal boxes transcribed from its class (origin/size/UV); child-part pivots. Run — red.
- [ ] **Step 4: Implement the geometry.** Run — green.
- [ ] **Step 5: Pose tests (RED).** The zombie arm literals; the skeleton's two states; the villager's crossed arms; the quadruped limb-swing order; the sheep's wool layer colour at two palette entries; the sheared state's layer set; the giant's scale. Run — red.
- [ ] **Step 6: Implement the poses and layers.** Run — green.
- [ ] **Step 7: The pass cases and the client mapping.** One GPU case per model class (non-empty, distinct from the player's silhouette); the registry test (every `ModelRef` in this task resolves to present texture keys against the store — the ignored variant reads the store; the base test checks the static list); the view-mapping tests (kind → `ModelRef` for each new variant). Run — green.
- [ ] **Step 8: Gate and commit.** Commit:

```
feat: draw the biped and quadruped mob families
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-client`; the GPU suite green.

---

### Task 10: Mob models II — the crawlers, the cubes, and the arthropods

**Goal:** The second family: creeper, spider and cave spider (with the source's leg rule and the eyes layer), enderman (with its eyes layer), chicken, squid, slime and magma cube (the gel layers, the sizes, the squash), bat, silverfish and endermite.

**Files:**
- Create: `crates/oxide-render/src/entity_models/crawlers.rs`.
- Modify: the registry, `entity_pass.rs` (`ModelRef`/`DrawExtra`), `oxide-client/src/view.rs` (mappings).
- Test: inline; the GPU file; the texture-set extension.

**Interfaces:**
- Produces:
  - `ModelRef` extensions: `Creeper`, `Spider`, `CaveSpider`, `Enderman`, `Chicken { child: bool }`, `Squid`, `Slime { size: u8 }`, `MagmaCube { size: u8 }`, `Bat { hanging: bool }`, `Silverfish`, `Endermite`.
  - `DrawExtra` extensions: `Slime { size: u8, squish: f32 }`, `Bat { hanging: bool }`, `Chicken { child: bool }`, `Creeper`.
  - The pose rules per source: `ModelCreeper` (the four-leg swing), `ModelSpider` (the eight-leg swing rule and its body/head sway — derive and pin), `ModelEnderman` (the long-armed walk; the carried-block pose is deferred with the held-item class and noted), `ModelChicken` (the head bob, wing flapping from the source's `oFlap`/`flapSpeed`/`flap` inputs — where those read entity fields the frame does not yet carry, derive whether they are pose-visible for a passively rendered chicken and either carry the two numbers in `DrawExtra` or record the simplification), `ModelSquid` (the tentacle rotation inputs — `rotationYaw`-driven; derive), `ModelSlime` (the gel layer; the size scale; **squash**: the source entity's squash pair fed from the client-side counter — derive how the squash reaches the renderer in 1.8 and carry it, or record the simplification), `ModelBat` (the hanging fold and the wing flap), `ModelSilverfish` (the body-segment sway), `ModelEndermite`.
  - Layers: spider/cave-spider eyes and enderman eyes (the emissive overlay rule — draw with the overlay texture at full brightness, the source's own toggle — derive); slime and magma gel (the second scaled body layer).
  - Texture keys: `entity/creeper/creeper.png`, `entity/spider/spider.png` + eyes, `entity/spider/cave_spider.png`, `entity/enderman/enderman.png` + eyes, `entity/chicken.png`, `entity/squid.png`, `entity/slime/slime.png`, `entity/slime/magmacube.png`, `entity/bat.png`, `entity/silverfish.png`, `entity/endermite.png` — in the registry test.
  - Each kind's height as its source entity-class constant, declared in the registry beside its models — pinned per kind (the nametag offset's input).
- Consumes: Task 8/9 machinery.

- [ ] **Step 1: Geometry tests (RED).** Per model, the box-count + literal set; the enderman's proportions; the spider's eight legs. Run — red.
- [ ] **Step 2: Implement the geometry.** Run — green.
- [ ] **Step 3: Pose tests (RED).** The spider leg rule at two phases; the enderman walk; the chicken's head bob; the squid's tentacle rotation at a yaw; the slime squash at two values (or the recorded simplification, asserted as such); the bat's hanging fold. Run — red.
- [ ] **Step 4: Implement the poses and layers.** Run — green.
- [ ] **Step 5: The pass cases, the mapping tests, the texture registry.** Run — green.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: draw the crawler, cube and arthropod mob families
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-client`; the GPU suite green.

---

### Task 11: Mob models III — the large and the exotic

**Goal:** The rest of the roster: horse, wolf, ocelot, rabbit, ghast, blaze, guardian, ender dragon and wither — geometry, variants, the saddle and collar layers, and the poses that need the source's own arithmetic (the dragon's flight, the ghast's float, the blaze's rods, the guardian's spikes).

**Files:**
- Create: `crates/oxide-render/src/entity_models/exotics.rs`.
- Modify: the registry, `entity_pass.rs`, `oxide-client/src/view.rs`.
- Test: inline; the GPU file; the texture-set extension.

**Interfaces:**
- Produces:
  - `ModelRef` extensions: `Horse { variant: u8, colour: u8, markings: u8, saddle: bool, armoured: bool }` (armour itself deferred — the flag carries so the mapping is stable), `Wolf { tamed: bool, collar: u8, angry: bool }`, `Ocelot { variant: u8, child: bool }`, `Rabbit { variant: u8, child: bool }`, `Ghast`, `Blaze`, `Guardian`, `EnderDragon`, `Wither`.
  - `DrawExtra` extensions as above (the mapping payloads).
  - Pose rules per source: `ModelHorse` (the large leg swing; the neck/head inputs — derive where the client's values come from; saddle and horse-armour layer boxes; the textures: the horse's colour/marking texture table — derive the selection rule and pin it; the undead horse variants), `ModelWolf` (the tail and head; the collar layer tinted by the palette; the angry state's eyes — derive whether the marker is client-visible and carry or record), `ModelOcelot` (the sneak and pounce-relevant leg offsets in their resting or walking states), `ModelRabbit` (the hop and the ear inputs; the variant texture table), `ModelGhast` (the tentacle sway), `ModelBlaze` (the rod spin — derive the `age`-driven rotation), `ModelGuardian` (the spikes' retract/extend state — derive; the eye track), `ModelEnderDragon` (the wing flap and the model's 8× scale; the wingspread input — derive what 1.8's client reads at rest and pin; the detail layers are deferred), `ModelWither` (the head bob and the invulnerability tint — derive; if the tint is a second texture, defer it with a note).
  - Texture keys: the horse colour/marking table, `entity/wolf/wolf.png` + collar key, the ocelot table, the rabbit fur/skin table, `entity/ghast/ghast.png` (+ shooting variant if trivially reachable — else defer), `entity/blaze.png`, `entity/guardian.png` (+ elder variant — derive), `entity/enderdragon/dragon.png` (+ `dragon_eyes.png` if the eyes layer lands), `entity/wither/wither.png` — in the registry test.
  - Each kind's height as its source entity-class constant, declared in the registry beside its models — pinned per kind (the nametag offset's input).
- Consumes: Task 8's framework; the frames' metadata extras.

- [ ] **Step 1: Geometry tests (RED).** Per model box-count + literals (the dragon and horse at their largest box counts — at least eight pins each). Run — red.
- [ ] **Step 2: Implement the geometry.** Run — green.
- [ ] **Step 3: Pose and variant tests (RED).** The horse texture selection literals; the dragon wing flap at two phases; the blaze rod spin; the wolf collar tint; the rabbit variant table. Run — red.
- [ ] **Step 4: Implement the poses, layers and variant tables.** Run — green.
- [ ] **Step 5: The pass cases, mapping tests, texture registry.** Run — green.
- [ ] **Step 6: The roster completeness test.** A test asserting the client's kind → `ModelRef` mapping covers **every** `MobType` member of the Task 1 table (no mob falls through to the debug-log arm) — the roster's own gate. Run — green.
- [ ] **Step 7: Gate and commit.** Commit:

```
feat: draw the exotic mob families
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-client`; the GPU suite green.

---
### Task 12: Object entities

**Goal:** The object set draws: dropped items (block items through the baked block models, item items through the source's generated shape), XP orbs, arrows, the thrown-item billboards, the fireballs, paintings, item frames, boats and the four minecart bodies — with the bob, spin and scale rules the source gives each.

**Files:**
- Create: `crates/oxide-render/src/entity_models/objects.rs` — the object geometry: the arrow, boat, minecart and painting models, the art table, and the billboard set with its sprite keys.
- Create: `crates/oxide-client/src/items.rs` — the minimal wire-item resolution (`Block(id) | Sprite(&'static str) | Missing`) and its table.
- Modify: `crates/oxide-render/src/entity_pass.rs` — `ModelRef`/`DrawExtra` extensions, the block-item mesh cache, the billboard draw primitive; `crates/oxide-render/src/renderer.rs` — `set_block_models(Arc<BlockModelSet>)` (the client passes the baked set once; the entity pass builds its cached block-item meshes from it); `oxide-client/src/view.rs`/`main.rs` — the mappings and the startup call.
- Test: inline per module; the GPU file; the item-table tests.

**Interfaces:**
- Produces:
  - `ModelRef` extensions: `BlockItem { block: u16 }`, `Sprite { key: &'static str }` (the generated-item shape; the sheet UV resolved from the atlas), `Arrow`, `Painting { art: u8 }`, `ItemFrame { content: FrameContent }` where `FrameContent = Empty | Block(u16) | Sprite(&'static str)`, `Boat`, `Minecart { body: MinecartBody }` (`Plain | Chest | Furnace | Tnt | Hopper`).
  - `DrawExtra` extensions: `Item { id: i16, count: u8, damage: i16 }`, `Painting { facing: u8 }`, `ItemFrame { rotation: u8 }`, `Orb`, `Projectile`.
  - The generated-item shape: built per `ItemModelGenerator`'s output — the source's front/back faces plus edge strips from the sprite's alpha, at the generated thickness — derive and pin one item's vertex/UV set by literal (a full-alpha sprite and an alpha-cut sprite differ; both pinned).
  - The block-item path: `entity_pass::BlockItemCache` — one cached vertex set per distinct block state, built from the baked `BlockModelSet` the client supplies (the same models the terrain bakes; state selection by `block` id with metadata folded the way the source's `Block.getStateById` folds it — derive the fold: item `damage` low bits are the block metadata, pinned by a test); the cache is bounded (a pinned cap, evicted never — distinct block items in view are few).
  - Per-class rules: item entities — the bob and spin derive from `RenderEntityItem` (no `EntityItem` update method carries them — the rotation lives at `:47-48`; derive the exact rate and pin the literals; checklist item 43's '1 rev / 2 s' wording is reconciled at Task 22, correction recorded); the drawn scale is the model's own `ground` display transform (derive it with the models — if the bake dropped `display`, restore that field for the item classes this task needs and record the pipeline change); the renderer applies the bob in the draw assembly (it is per-frame, `age`-driven); XP orbs — the `RenderXPOrb` billboard and its colour math (derive); arrows — the `RenderArrow` geometry (derive; pin a box count and angle); paintings — the `EnumArt` table (26 entries with their pixel sizes and texture keys, pinned by literal test) drawn as the source's art quad + the back panel + frame boxes; item frames — the frame box plus the nested content drawn at the frame's scale and rotation (the source's rule: block contents draw as small blocks, item contents as the generated shape — derive); boats — `ModelBoat`; minecarts — `ModelMinecart` plus the cargo (chest, furnace and hopper block models through the block-item path; the TNT flash derives from `age` per the source — if it reads non-networked state, record the simplification).
  - The billboard primitive in the pass: a camera-facing sprite quad with the source's transform (`RenderSnowball`-shaped: face the camera, then the entity's yaw — derive) and the per-kind sprite keys (`items/snowball`, `items/egg`, `items/ender_pearl`, `items/ender_eye`, `items/potion` with the recorded tint limit, `items/experience_bottle`, `items/fireworks`, plus the fireball sheet `entity/fireball`-class keys — derive); other players' — no: nothing else draws through it.
  - `oxide_client::items::resolve(id: i16, damage: i16) -> ItemResolution` — id < 256 resolves to `Block` when the block table knows the id (the fold above), else the pinned sprite table's `Sprite`, else `Missing` (the atlas's missing sprite; recorded). The table holds at least: stick, apple, coal, iron ingot, gold ingot, diamond, diamond sword, iron sword, bow, arrow, snowball, egg, ender pearl, ender eye, potion, experience bottle, and the two firework sprites — each a literal `id → asset path` pair pinned by test (the full registry rides M5).
  - The texture keys this task adds (registry test): the painting `painting/*` set (the 26), the boat/minecart keys, the item sprites above, the fireball keys, `entity/xporb.png`.
- Consumes: Task 8's framework and pass; the baked block models (assets); the item sprites in the atlas (M2).

- [ ] **Step 1: The art table and the item table (RED).** 26 art literals; the resolve table literals; the block/metadata fold. Run: `cargo test -p oxide-render objects` and `cargo test -p oxide-client items` — red.
- [ ] **Step 2: Implement both tables and the fold.** Run — green.
- [ ] **Step 3: Geometry (RED).** The arrow, boat, minecart and painting-frame literals; the generated shape's two pins; the billboard transform literal. Run — red.
- [ ] **Step 4: Implement `objects.rs` and the pass extensions** (cache, billboard, nested frame content). Run — green.
- [ ] **Step 5: The rules (RED).** The item bob/spin literals; the orb colour; the frame rotation at two angles; the TNT flash at two ages (or the recorded simplification). Run — red.
- [ ] **Step 6: Implement the rules.** Run — green.
- [ ] **Step 7: GPU cases and mappings.** One case per class (non-empty, silhouette-distinct); the view-mapping tests for every object kind (block item, sprite item, projectile, painting, frame, boat, each minecart body). Run — green.
- [ ] **Step 8: Gate and commit.** Commit:

```
feat: draw the object entities
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-client`; the GPU suite green.

---

### Task 13: Nametags

**Goal:** Names over entities: the composed `§`-text drawn as world-space billboard glyphs with the source's background, scale, distance rule and see-through handling — and the shared text builder extracted from the debug overlay into `oxide-render/src/text.rs`, output unchanged.

**Files:**
- Create: `crates/oxide-render/src/text.rs` — the shared builder: a buffer of text draws (glyph quads with position, scale, colour, shadow flag) plus the `§` run decoder shared with chat; the overlay's glyph path re-points here with identical output.
- Modify: `crates/oxide-render/src/entity_pass.rs` — the nametag draw inside the pass (world-space billboards), the `EntityDraw`'s `nametag: Option<NametagDraw>` field (declared here), `DrawExtra` untouched; `oxide-client/src/view.rs` — the nametag pass-through (frame text → draw).
- Test: `text.rs` (glyph quads vs the overlay's old literals — the move is behaviour-preserving; the `§` decoder suite); entity pass inline; GPU case.

**Interfaces:**
- Produces:
  - `oxide_render::text::{TextBuilder, draw_runs, decode_legacy}` — `TextBuilder::push(text: &str, at: [f32; 3], scale: f32, colour: [f32; 4], shadow: bool)`; `decode_legacy(text) -> Vec<StyledRun>` (colour codes, the four styles, `§r` reset, and the hex-lookalike non-code cases — pin the decoder's table against the font renderer's own `§` handling, including the digits/letters it swallows); the overlay's line drawing re-implemented on it with byte-identical quads (its existing tests stand and are the proof).
  - The nametag draw in the pass: for each entity draw with a nametag, a billboarded text at `height + 0.5` above the feet (the entity's own height read from the model registry — each model family pinned its kind's height there; the source's `RendererLivingEntity` offsets the name by `entity.height`), billboarded on the view sphere (the source rotates by `RenderManager.playerViewY` and `playerViewX` — the camera's yaw and pitch, not the entity's — then the scale; derive the exact composition and pin it at a known pair), scale = the source's `0.016666668 × 1.6`, background `rgba(0,0,0,0.25)` sized to the text extent + the source's margins, text drawn through `TextBuilder` at full-bright white with the composed `§` runs.
  - The rule set, pinned: render distance 64 blocks, 32 while the entity is sneaking (the source's two range constants — `RendererLivingEntity.renderName`'s f-double check); the "name visible" resolution already composed session-side (the frame's `Option`); the second see-through pass exactly as the source holds it (derive what the source does — the order and depth-state of the two passes — and pin the behaviour with the GPU case: a nametag behind a wall is absent/as present as the source's rule states).
- Consumes: Task 8's pass; the frames' composed text (Task 5).

- [ ] **Step 1: The extraction (RED first as a move).** The overlay's text tests are the baseline; `text.rs` is written to pass them unchanged; new `§` decoder tests (each code, the reset, a mid-string switch, the non-codes). Run: `cargo test -p oxide-render text` — green only when the overlay is provably unchanged (its suite runs in the same command).
- [ ] **Step 2: The nametag draw.** Literals: the scale, the background alpha, the two ranges, the billboard composition at a known yaw/pitch pair. Run — green.
- [ ] **Step 3: The GPU case.** A synthetic entity with a nametag draws text pixels above it; another at 65 blocks does not; a sneaking one at 33 does not and at 31 does; the see-through case matches the pinned source rule. Run the ignored suite.
- [ ] **Step 4: The view pass-through.** Mapping tests: a frame with a name renders a nametag; without, none. Run — green.
- [ ] **Step 5: Gate and commit.** Commit:

```
feat: draw nametags
```

**Verification:** `cargo test -p oxide-render`; the overlay's suite unchanged; the GPU suite green.

---

### Task 14: The HUD pass and the chat rendering

**Goal:** The GUI-space pass at the scaled resolution, and the chat's full receive-to-pixels path: the component model and its hostile parser, the `§`-aware flattening, the wrapping layout, the log with its fade and scrollback, and the drawing — all reading the new `Chat` event.

**Files:**
- Create: `crates/oxide-render/src/hud.rs` — `HudPass`: the scaled resolution (the source's auto rule), the primitive set (solid rect, textured rect from the atlas or any registered texture, text via `TextBuilder` with scale/colour/shadow), the frame's draw list.
- Modify: `crates/oxide-render/src/renderer.rs` — the pass's invocation (after Dim, before the debug Overlay), `set_hud(draws)`, the scaled-resolution input; `oxide-render/src/lib.rs`; `oxide-game/src/chat.rs` (created — below); `oxide-game/src/session.rs` — the 0x02 arm emitting `ClientEvent::Chat { text: String, position: i8 }` (raw JSON untouched); `oxide-client/src/view.rs` — the chat mirror (log, fade clocks, scroll offset) and the per-frame chat draw assembly; `oxide-client/src/main.rs` — the arm and the wiring; `oxide-game/Cargo.toml` — `serde_json`.
- Test: `chat.rs` inline (parser, flatten, wrap, log); `hud.rs` inline (the scale rule at 1280×720 → 427×240 and 1920×1080 → 480×270, both pinned — the auto loop's own results; no ≤3 clamp in the auto path (the guiScale=3 contrast value noted in the test)); the GPU file (a chat line, the fade series); the view tests; `session_replay.rs` (the Chat event).

**Interfaces:**
- Produces:
  - `oxide_game::chat::{TextComponent, parse_json, flatten, wrap, ChatLog}`:
    - `TextComponent { text: String, colour: Option<u8>, bold/italic/underlined/strikethrough/obfuscated: bool, click: Option<ClickEvent>, hover: Option<HoverEvent>, children: Vec<TextComponent> }` — a `serde_json::Value` walk (never a derive; hostile-tolerant: unknown keys ignored, wrong types degraded, depth capped at `MAX_COMPONENT_DEPTH = 16`, totals capped); `parse_json(&str) -> TextComponent` (a bare string is itself the text; a malformed tree becomes a plain-text component of the raw input, clipped).
    - `flatten(&TextComponent) -> Vec<StyledRun>` where `StyledRun { text: String, colour: Option<u8>, styles: u8, click: Option<ClickEvent>, hover: Option<HoverEvent> }` — style inheritance and the source's colour precedence (explicit JSON colours, then `§` codes inside text — the decoder handles both, deriving the precedence order the source's `FontRenderer` applies); `ClickEvent { action: ClickAction, value: String }` with `ClickAction { RunCommand, SuggestCommand, OpenUrl }` (unknown actions dropped); `HoverEvent::ShowText(TextComponent)` (other actions dropped).
    - `wrap(runs, width, font) -> Vec<Vec<StyledRun>>` — the source's split rule (derive from `GuiUtilRenderComponents.splitText`/`FontRenderer.listFormattedStringToWidth`: the wrap point search, the explicit-newline handling, the width equality case; pin ≥4 literals at width 320 with the M2 font metrics).
    - `ChatLog { lines: VecDeque<LoggedLine { runs: Vec<StyledRun>, received_tick: u64, // ticks }>, cap }` — the source's retention (derive the cap and the drawn-when-open count: the task pins both from `GuiNewChat`), `push(component, tick)`, `update(tick)` (age); the fade rule from `GuiNewChat.drawChat` (`:59-77`: full alpha through ~tick 180, then the quadratic fade over the last ~20 ticks — gone at ~200, i.e. ≈10 s total lifetime and ≈1 s fade — derive the exact arithmetic and pin four points: t0, end-of-full-alpha, mid-fade, gone; checklist item 11's wording is reconciled at Task 22); the scroll offset state (clamped, per the source's own).
  - `oxide_render::hud::HudPass` — the scale rule (`ScaledResolution`'s auto loop: the largest factor keeping `width ≥ 320·f` and `height ≥ 240·f`, from `GameSettings.guiScale` 0 = auto; the ceilings; the unicode-font halving branch is skipped — no unicode font — recorded), primitives taken by the client's per-frame build; text draws land through `TextBuilder`; the pass draws after Dim and before the debug overlay, blending like the overlay (the same pipeline conventions).
  - The chat draw (client-side assembly, pinned shapes): background quads per line at the source's alpha (chatOpacity-scaled — pin the arithmetic), the runs drawn as text with shadow (the source's default), bottom-anchored above the hotbar (`y = height − 48` class rule — derive and pin), the focused/unfocused history heights (180/90 at defaults), the width 320, and the position-1/position-2 variants (system-piped lines render outside the box when closed — derive the source's two special rules and pin them).
- Consumes: `TextBuilder` (Task 13), the font metrics, the tick clock; the new event.

- [ ] **Step 1: The parser (RED).** Suite: every style flag; nested children with inheritance; a colour override at each level; click and hover payloads; the unknown-action drop; depth bomb; 200 KB string; malformed JSON; a bare `"text"`; `§` inside text with a colour override nearby. Run: `cargo test -p oxide-game chat` — fails to compile.
- [ ] **Step 2: Implement the parser and flattening.** Run — green.
- [ ] **Step 3: The wrap (RED).** The four literals; a style-carrying wrap (the carried styles persist across the break and the colour reset behaves per source); a long word. Run — red.
- [ ] **Step 4: Implement `wrap` and `ChatLog`.** Run — green.
- [ ] **Step 5: The scale rule and the pass.** `hud.rs` tests (427×240, 480×270); the GPU case: a synthetic chat line's pixels; the fade series pixels at the four alphas. Run — green.
- [ ] **Step 6: The session event and the view.** `session_replay.rs`: a 0x02 emits the event with the raw JSON and the position byte; the view tests: a log line renders at tick t and not at t+hold+fade; scroll changes the drawn slice. Run — green.
- [ ] **Step 7: Gate and commit.** Commit:

```
feat: draw the hud and the chat
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-game`; `cargo test -p oxide-client`; the GPU suite green; `cargo deny check` (serde_json).

---

### Task 15: The chat input and its interactions

**Goal:** The chat becomes usable: T, `/` and the send path; the field's text editing and cursor; Escape and the capture interplay; wheel scroll; the click events that act (run, suggest, open-with-confirm) and the hover tooltip; and the scripted `chat` macro the acceptance needs.

**Files:**
- Modify: `crates/oxide-client/src/main.rs` — the text-event path (winit character input), the field state machine, the send, the key routing (the chat open/closed split), the script's `chat` line and cursor-mode `look`, the wheel arm.
- Modify: `crates/oxide-client/src/keymap.rs` — `Key::{T, Slash, Tab, Enter, Backspace, ArrowLeft, ArrowRight, ArrowUp, ArrowDown, Escape}` (the additions this task needs; Escape's new chat-closed behaviour unchanged).
- Modify: `crates/oxide-game/src/input.rs` — `InputEvent::SendChat { text: String }`; `crates/oxide-game/src/session.rs` — the drain arm sending 0x01 via Task 3's writer (the ≤ 100 check is the field's; the session keeps a debug-log guard).
- Modify: `crates/oxide-client/src/view.rs` — the input-line draw (text, cursor blink) and the hover tooltip; the URL confirm overlay state.
- Test: inline in the touched modules; `session_replay.rs` (SendChat → 0x01 bytes on the wire).

**Interfaces:**
- Produces:
  - The field state machine: `ChatInput { text: String, cursor: usize /* byte index, pinned to char boundaries */, open: bool, blink: u64 }` — open via T (`text = ""`) or `/` (`text = "/"`); characters append at the cursor (control characters filtered — derive the source's `GuiTextField` filter); backspace/delete; left/right; the 100-char cap (the source's `GuiTextField` max — refuse beyond, pinned); Enter sends `SendChat` with the raw text and closes; Escape closes without sending; the source's arrow-history over its own sent-messages list (`GuiChat.java:275-292` on `GuiNewChat`'s list — implement it and pin the recall order); tab-completion is deferred (Known limits — both completion surfaces need machinery this milestone does not build) and Tab while open is swallowed, not forwarded.
  - The open/closed routing: while open — keys go to the field (movement bindings do not fire; the session receives nothing but the send), the captured look deltas move the window's cursor instead of the camera (the client stops forwarding `MouseDelta` to the session while open and moves a cursor position instead; on open the cursor frees — the captured state releases per the M3 capture rules' chat carve-out; on close it recaptures), the wheel scrolls the chat (clamped), and clicks hit-test the chat: a click on a run with a click event acts; a click elsewhere closes nothing (the source's screen holds focus). Escape closes the chat and does nothing else — the capture release rule applies only when the chat is closed (pin the carve-out with the M3 rule's tests extended).
  - Click actions: `RunCommand` → `SendChat("/the-value")` through the same path (the server is the authority); `SuggestCommand` → replace the field text and move the cursor to the end; `OpenUrl` → the interim confirm overlay (Question 3): a dimmed frame with the URL and the two-key prompt; Enter opens through the system opener (`xdg-open` — a spawn, recorded as the interim mechanism, the confirm screen proper is M6's) and Esc cancels; nothing else opens a link, ever.
  - Hover: `ShowText` renders the tooltip at the cursor (background + border per the source's tooltip colours, derived and pinned; a two-line tooltip wraps per `wrap` at the tooltip width).
  - The cursor's blink: the source's `updateCounter`-driven rule (derive; pin two phases).
  - The script additions: `--input-script` gains `<tick> chat <text>` (opens via the same state machine as T, types `text` through the character path, sends on the line's own tick — the text path, not a shortcut), and the documented rule that `look` lines move the chat cursor while the chat is open (`refs/rig/`'s script notes and the parser's doc comment both say so; the macro is a rig surface, not a product one).
  - The acceptance scenes this unlocks (Task 22 executes): send-and-see on the vanilla client; a `/tellraw` line's tooltip and its `run_command` click proven by the server log; the wheel-scroll frame.
- Consumes: Task 14's chat state and drawing; the M3 capture rules; `InputEvent` plumbing.

- [ ] **Step 1: The field (RED).** Append/backspace/cursor cases including the char-boundary pin; the 100 cap; open-by-T vs open-by-slash; Enter sends exactly the field text; Escape closes silently; the history rule per the derived source behaviour. Run: `cargo test -p oxide-client chat_input` — fails to compile.
- [ ] **Step 2: Implement the field and key routing.** Run — green.
- [ ] **Step 3: The send path.** `session_replay.rs`: `SendChat` writes exactly Task 3's bytes for the text; a 101-char input never leaves the field (the cap test above) and the session's guard logs. Run — green.
- [ ] **Step 4: The interactions.** Click hit-tests (a run rect click runs the command; a suggest replaces the text; a URL opens only through the confirm state — assert no opener runs on Esc and that Enter calls it exactly once — the opener is injected for testability); the hover tooltip state; Run — green.
- [ ] **Step 5: The script macro.** Parser tests for `chat <text>` (the open-type-send sequence against the state machine; a `chat` line while the chat is already open is refused with a log). Run — green.
- [ ] **Step 6: The GPU frame.** One frame with the input line and cursor; one with an open chat and a scrolled history (the drawn slice changes). Run the ignored suite.
- [ ] **Step 7: Gate and commit.** Commit:

```
feat: chat input, click events and the script chat macro
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-game`; the GPU suite green; the M3 capture-rule tests green with the carve-out.

---
### Task 16: The tab list

**Goal:** Hold Tab and the player list draws: the centred header and footer, the entry columns with heads, names (team-coloured), list-objective scores and latency bars — and the checklist gains the row this surface has never had.

**Files:**
- Modify: `crates/oxide-client/src/view.rs` — the tab state (the held key from the input path; the draw assembly from `PlayerList` + `Scoreboard` + `TabText` + the skins map).
- Modify: `crates/oxide-client/src/main.rs` — the Tab held-state wiring (closed-chat semantics: Tab opens the list while held; Task 15's swallow applies only while the chat is open).
- Modify: `crates/oxide-render/src/hud.rs` — any primitive the layout needs that Task 14's set lacks (Slice draws from `gui/icons.png`, per-texture sub-rect blits with alpha).
- Modify: `docs/parity/checklist.md` — the new tab-list row (see below).
- Test: `view.rs` inline (layout literals, ordering); the GPU file (two entries with a header).

**Interfaces:**
- Produces:
  - The tab state: `TabState { open: bool, entries: Vec<PlayerListRecord>, header: String, footer: String }` fed by the Task 6 events; `tab_draws(&self, board: &Scoreboard, skins: &impl SkinLookup) -> Vec<HudDraw>`.
  - The layout, per `GuiPlayerTabOverlay` (derive and pin): the row height and column pitch arithmetic; the column break (20 rows per column, the ≤ 4-column arrangement for larger lists); the per-entry cell background `553648127` (`0x20FFFFFF`, `:167` — distinct from the grid/header/footer rects' `Integer.MIN_VALUE`-class, both derived); the header centred at the top and the footer at the bottom (drawn with shadow per source and placed at the two margin rules — pin both y positions at 427×240); the entries in the source's own order (derive: the gamemode-and-name ordering the source's `getPlayerList`/render path applies; pin with a 4-entry fixture including a spectator).
  - The entry contents: the name (the Task 6 composed display name — team colours included; the source's own truncation rule to the column width, derive and pin); the list-objective score (the `display` slot 0 objective: drawn right of the name; the number renders `YELLOW` (`GuiPlayerTabOverlay.java:365`), and the health-class kind's own branch — derive what the source draws for it, the hearts glyph or the plain number, and pin it); the head: an 8×8 sub-rect at the skin's `(8, 8)` (the face region — pin the UVs) plus the hat overlay sub-rect at `(40, 8)` drawn as a second pass when the player's parts byte carries the HAT bit (`GuiPlayerTabOverlay.java:188-193` gates on `isWearing(EnumPlayerModelParts.HAT)` — the byte is Task 8's pinned all-on default); the latency bars: five levels from `gui/icons.png` — pin each level's rect `(0, 176 + 8·level, 10, 8)` (`GuiPlayerTabOverlay.java:270`; the level thresholds 150/300/600/1000 shape the mapping, `:244-263`) — and the no-signal X at level 5, `(0, 216, 10, 8)` (no response time draws the X per source).
  - The head texture resolution: through the `SkinLookup` resolver Task 8 declares (the same one the entity pass uses — the default fallback included).
  - The checklist row (committed): `| N | Player list (tab): header/footer, entry columns, heads, names, scores, latency bars | screenshot | ... expected values ... | ... evidence ... |` — numbered at the end of the table (its current last item is 57), classified per the established practice, its expected-values column citing the source's `GuiPlayerTabOverlay` geometry, its evidence cell pointing at Task 22's frames, and the table's summary line (the classified count and composition) refreshed with the added row. The pre-flight note on the §16 nametag row carries the same table's style.
- Consumes: Task 6's data, Task 14's HUD primitives, the skin map.

- [ ] **Step 1: The layout (RED).** Literals: 3 entries → one column at the pinned x; 21 entries → two columns; a spectator sorts last; the header/footer y positions; the truncation cut. Run: `cargo test -p oxide-client tab` — fails to compile.
- [ ] **Step 2: Implement the draw assembly.** Run — green.
- [ ] **Step 3: The icon and head UVs.** Pins for the five bar levels, the X, the face rect and the hat rect; the fallback path draws the default skin's face. Run — green.
- [ ] **Step 4: The GPU case.** A frame with a header and two entries (one with a score and full bars) — pixels present, the header centred (a sampled column of pixels differs from empty at the centre and not at the margin). Run the ignored suite.
- [ ] **Step 5: The checklist row.** Add it and refresh the summary line's counts and composition; run `python3 scripts/parity-diff.py --self-test` (the checklist's edits never break the script; run it as the formality it is) and re-read the row for the established column style.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: draw the tab list
```

**Verification:** `cargo test -p oxide-client`; the GPU suite green; the checklist parses (it is markdown — the gate's docs checks stand).

---

### Task 17: The scoreboard rendering

**Goal:** The sidebar and the below-name scores: the display-slot objectives drawn with the source's geometry, ordering, colours and red-numbers rule; the teams' colour reaches every name surface (they already reach nametags and the tab list through the composed text — this task completes the set with the sidebar's own formatting).

**Files:**
- Modify: `crates/oxide-client/src/view.rs` — the sidebar assembly and the below-name collection; `crates/oxide-render/src/entity_pass.rs` — the `EntityDraw::below_name: Option<String>` field (declared here) drawn as a second world-space line under the nametag by the same path.
- Test: `view.rs` inline (sorting, arithmetic, red-numbers cases, below-name collection); the GPU file (sidebar frames).

**Interfaces:**
- Produces:
  - The sidebar assembly per `GuiIngame.renderScoreboard` (derive and pin): slot 1's objective; the entries sorted by score descending and by name ascending on ties (derive the tie rule; pin a 4-entry fixture); at most 15 drawn (the source's own clamp — pin a 16-entry clamp case); the title centred above the list; the width from the longest rendered entry + the source's margins; the x at the source's `width − size − 3`-class rule (pin at 427×240); the y centred; the background the source's semi-transparent black (pin the value); each line's number right-aligned; the numbers draw red — the source sets `RED` unconditionally for the sidebar (`GuiIngame.java:577`, `:592`; no kind check lives there), so pin the red literal for both a `dummy` and a `health` objective (checklist item 12's wording is reconciled at Task 22; the kind's own branch — hearts or plain — lives in the tab list, Task 16).
  - The entry formatting: the composed name per Task 6 (`format_entry`) — the sidebar's entries colour through their teams like the tab list's (verify against the source: if the source's sidebar does **not** apply team colour to the name, pin that negative instead; the task records which it found and pins the observed behaviour either way).
  - The below-name line: for each entity draw whose entry has a score in slot 2's objective, `below_name = Some(the number)` drawn under the nametag at the source's own offset and rules (derive the visibility conditions — the source draws it for entities it considers name-showing; pin each condition with a fixture: a player with a score and a name, one without a name, one without a score).
  - Team-colour completeness check: a test walking the three surfaces' inputs (nametag text, tab name, sidebar name) asserting one colour source (Task 6) feeds all three — the "one composition point" decision's own regression guard.
- Consumes: Task 6 (state + formatting), Task 13 (world text), Task 14 (HUD).

- [ ] **Step 1: Sorting and arithmetic (RED).** The fixtures above; the 16-entry clamp; the x and width literals at 427×240. Run: `cargo test -p oxide-client scoreboard` — red.
- [ ] **Step 2: Implement the sidebar assembly.** Run — green.
- [ ] **Step 3: The red-number and formatting cases (RED).** The red literal for both objective kinds; the team-colour observation pinned; the below-name three fixtures. Run — red.
- [ ] **Step 4: Implement the formatting, the red rule, and the below-name path.** Run — green.
- [ ] **Step 5: The GPU cases.** A sidebar frame (title, three lines, a red number) — sample pixels for the background band, a number's colour, and the right alignment; a below-name frame. Run the ignored suite.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: draw the scoreboard sidebar and below-name scores
```

**Verification:** `cargo test -p oxide-client`; the GPU suite green.

---

### Task 18: The boss bar

**Goal:** The wither's and the dragon's health bar: the entity pass raises a `BossStatus`-shaped value while either draws, the frame's countdown runs it down, and the HUD draws the bar and its name.

**Files:**
- Modify: `crates/oxide-render/src/entity_pass.rs` — the pass raises the status (wither and dragon draws set it; last-drawn wins, the 1.8 static's own behaviour).
- Modify: `crates/oxide-render/src/renderer.rs` — the status cell and its per-frame countdown (`BossStatus { name: String, health_fraction: f32, colour_modifier: bool, time: u32 }`; each `render()` the pass may overwrite; after the pass, an untouched status decrements; zero hides).
- Modify: `crates/oxide-render/src/hud.rs` — the draw: the two `gui/widgets.png` slices, the name above, the colour modifier tint.
- Test: inline (the wither fraction math — the wither's max health constant and a 150/300 literal; the dragon's own fraction rule; the countdown edges: set at frame f draws through f+99 and not f+100); the GPU file (a half-full bar at a sampled pixel; the tint case).

**Interfaces:**
- Produces:
  - The status rule: the wither sets `name` from its display name (the composed name or the source's default — derive what a vanilla wither's bar shows), `health_fraction = health / max_health` (the wither's max is its own class constant — derive, pin `300.0`-class literal), `colour_modifier = true` (the wither's own flag — the source sets it for the wither alone among this set; derive and pin); the dragon sets its fraction from the source's own part or overall rule (derive — the 1.8 `RenderDragon`'s bar reads a specific piece of the dragon's health; pin what it reads); both draw through the source's `BossStatus.setBossStatus` shape with the 100-frame hold unless re-set.
  - The draw per `GuiIngame.renderBossHealth` (derive and pin): the background slice and the fill slice from `gui/widgets.png` (their source rects and the 182×5-class sizes pinned), the fill clipped to `health_fraction` (the pixel boundary at 0.5 pinned by sample), the name centred above (the source's y offset pinned), the bar's x centred at the scaled width, the tint for the colour modifier (the exact colour derived; pin the tinted and untinted pixel).
  - The reconciliation note (committed with this task's docs touch): if the in-tree checklist item 13 claims later-version behaviours (per-frame stacking), it is corrected to the 1.8 single-status rule with the correction recorded the way the M3 pre-flight recorded checklist corrections.
- Consumes: the pass (Task 8+), Task 14's HUD.

- [ ] **Step 1: The status rules (RED).** The wither literals; the dragon rule; the countdown edges; the overwrite case (wither then dragon in one frame → dragon's name). Run: `cargo test -p oxide-render boss` — fails to compile.
- [ ] **Step 2: Implement the status cell and the countdown.** Run — green.
- [ ] **Step 3: The draw (RED).** The slice rects; the name offset; the tint colour. Run — red.
- [ ] **Step 4: Implement the draw and the checklist reconciliation.** Run — green.
- [ ] **Step 5: The GPU cases.** A half bar (the boundary pixel), a full bar, the tinted variant; the countdown's disappearance frame. Run the ignored suite.
- [ ] **Step 6: Gate and commit.** Commit:

```
feat: draw the boss bar
```

**Verification:** `cargo test -p oxide-render`; the GPU suite green.

---
### Task 19: The dig carry — the stage map re-key, the entity filter, and the wiring pin

**Goal:** The M3 known-limit closes: the break-stage map keys by breaker id exactly as the source's `damagedBlocks` does, the 0x25 handler filters to known entities, two breakers on one block behave as the source, and the overlay's wiring gains the discriminator the M3 review asked for.

**Files:**
- Modify: `crates/oxide-game/src/interaction.rs` — `BreakStages` re-keys: `BTreeMap<i32 /*breaker id*/, BreakEntry>` with `BreakEntry { pos: [i32; 3], stage: u8, last_update: u32 }` (derive the current position-keyed shape and the counter's type and keep the counter semantics: expiry when `counter − last_update > 400`, swept every 20 ticks — `RenderGlobal.java:1124-1146`; the known-limits item 7 text said position-keyed "until M4" — this is that; the entry type is shared with the window).
- Modify: `crates/oxide-game/src/session.rs` — the own-dig writes carry the session's own entity id (`Joined.entity_id`); the 0x25 arm filters: a breaker id the store does not know is logged and dropped (the source's `getEntityByID` non-null rule), a known one inserts; `ClientEvent::BreakStage` and `BreakCleared` gain `breaker: i32` (the window's map mirrors the session's entry-for-entry; two entries on one position are two entries).
- Modify: `crates/oxide-render/src/world_overlay.rs` — the crack builder takes the entry list (`&[(pos, stage)]`, sorted by (pos, breaker) for determinism; it derives its current input shape and re-points it); one crack draw per entry (two same-position entries multiply twice — the source's `damagedBlocks` iteration does exactly this through its render path; the derive-and-pin requirement is on the builder's input, the multiply semantics are the existing pass's own).
- Modify: `crates/oxide-client/src/main.rs` — the window map and the wiring; the new `overlay_wiring` test and the descriptor pins (below).
- Test: `interaction.rs`/`session_replay.rs` inline additions; `oxide-client`'s `overlay_wiring` test; the existing GPU overlay cases unchanged.

**Interfaces:**
- Produces:
  - The re-keyed `BreakStages` with its API: `insert(breaker, pos, stage, counter)`, `clear(breaker)`, `sweep(counter)` (the 20-tick cadence stays where it is), `entries() -> impl Iterator<Item = (i32, &BreakEntry)>`.
  - The events: `ClientEvent::BreakStage { breaker: i32, x: i32, y: i32, z: i32, stage: u8 }` and `ClientEvent::BreakCleared { breaker: i32, x: i32, y: i32, z: i32 }` — the M3 shapes plus the breaker.
  - The wiring discriminator (the M3 review's D11/T12-F1 item, at the touch it asked for): the review's ask was construction-level — the outline pipeline's depth state (P1) and the crack pipeline's retained depth state (P2) both stayed green when flipped — so the pin has two legs. **Leg one (construction):** assertions on the overlay's own pipeline descriptors — the outline and crack descriptors carry the values the M3 evaluation selected, and each of P1/P2, flipped, fails the test. **Leg two (wiring):** a client unit test `overlay_wiring` that drives the real client functions — `apply_session_event` with an `Aim` change (aim at a block → the outline's consumed state names that block; aim to none → it clears) and with a breaker-keyed `BreakStage` pair (two breakers, one position → the consumed crack list holds both; clearing one breaker leaves the other) — and asserts the state the world-overlay pass consumes before drawing. The pass's own pixel pins stand unchanged; if the descriptor surface cannot express leg one as a test, the recorded note the review explicitly allowed, with its reasoning — not silent.
- Consumes: the store (Task 4) for the filter, the session's own id, the existing sweep and events.

- [ ] **Step 1: The re-key (RED).** Unit tests: two breakers on one position both live; one cleared, the other stands; the sweep expires per entry (two entries with different ages); a stage outside `0..=9` refused as before. Run: `cargo test -p oxide-game break` — red.
- [ ] **Step 2: Implement the re-key and the event fields.** Run — green.
- [ ] **Step 3: The filter (RED).** Session tests: 0x25 for an unknown id is dropped with the log; for a spawned player it inserts with that breaker; the own-dig path carries the own id. Run — red.
- [ ] **Step 4: Implement the filter and the own-dig id.** Run — green.
- [ ] **Step 5: The overlay builder and the window map.** Re-point the builder; update the client's map; the builders' existing tests stay green. Run — green.
- [ ] **Step 6: The wiring pins.** Both legs as above; canary each — comment the wiring line (leg two reds), flip the outline descriptor and the crack descriptor (leg one reds, twice) — record the canaries in the task notes, then restore. Run — green.
- [ ] **Step 7: Gate and commit.** Commit:

```
fix: key the break-stage map by breaker and filter 0x25 to known entities
```

**Verification:** `cargo test -p oxide-game`; `cargo test -p oxide-client`; `cargo test -p oxide-render`; the GPU suite green.

---

### Task 20: The player carry — the collision flag, the sprint release, and the sneak eye

**Goal:** The state backlog's items 2, 3 and 4: `physics::step` reports the horizontal-collision flag, the sprint release gains the source's third clause, the sneak eye offset lands with both its consumers, the M3 plan's pin and `player.rs`'s cites are amended, and `attacked_at_yaw` closes with its source finding pinned.

**Files:**
- Modify: `crates/oxide-game/src/physics.rs` — `step` returns `StepOutcome { collided_horizontally: bool }` (the exact internal flag the source's `isCollidedHorizontally` corresponds to — derive which step of the move loop sets it and expose that, not a re-derivation).
- Modify: `crates/oxide-game/src/input.rs` (the sprint state machine's home — derive its current location; M3's Task 1 put it with the input layer) — the release condition gains the collision clause (`EntityPlayerSP.java:818-821`: `sprinting && (moveForward < f || isCollidedHorizontally || !flag3)`; the other two clauses exist — extend, and pin the full three-clause truth table).
- Modify: `crates/oxide-game/src/session.rs` — the tick order: the step's outcome feeds the sprint update in the same tick (the source's `onLivingUpdate` order: move first, sprint logic after — reorder if today's session runs the sprint update before the step; the reorder is part of this task and its tests).
- Modify: `crates/oxide-game/src/player.rs` — `eye_height()`: `1.62` normally, `1.54` while sneaking (the source's `1.62 − 0.08`, `EntityPlayer.getEyeHeight`, `:2326-2341`; sleeping is not modelled — there is no sleep state); the doc comment names both consumers again and drops the stale cite (the D20 carry) — the constant becomes `EYE_HEIGHT`/`EYE_HEIGHT_SNEAK` with the derivation named.
- Modify: `docs/plans/2026-10-02-m3-player.md` — the pin amendment the M3 close routed: the Decision 6 / Task 11 text's "feet + 1.62" gains the sneak clause (the text is corrected, not rewritten; the amendment is listed in the commit).

**Finding — `attacked_at_yaw` (backlog item 4) closes as a non-defect.** The source's client never computes a hit direction in multiplayer: `performHurtAnimation` (`EntityLivingBase.java:1183-1187`, "Only used by packets in multiplayer") zeroes `attackedAtYaw` on every hurt status, `EntityPlayerSP.attackEntityFrom` refuses damage client-side (`:140-143`), and the only setter is the server-side attacker branch of `attackEntityFrom` (`source.getEntity()` path, `:915-966`) — never reached on a client. The source-faithful value is therefore zero, and the M3 carry's premise ("hit direction rides that") is falsified by the source: no wiring lands; the parameter stays zero by design and gains the pin — a comment at the parameter's definition citing the finding, and a test asserting a hurt status leaves the camera's term zero.
- Test: `physics.rs` (the flag's three cases: free walk false, wall true, diagonal-into-wall true), `input.rs` (the truth table), `session_replay.rs` (sprint releases within one tick of the wall), the existing eye/camera/raycast tests gain the sneak literals.

**Interfaces:**
- Produces:
  - `oxide_game::physics::StepOutcome { pub collided_horizontally: bool }` — returned by `step`; every existing caller updates (the session; the physics tests).
  - The sprint release: fully the source's condition, with the collision term live; the M3 sprint tests extended, none weakened.
  - `Player::eye_height()` — the two-value rule; the camera and the raycast consume it unchanged (they read the method already; their tests gain the crouch literals: the render eye at `1.54 + displacement` while sneaking, the raycast origin at feet + 1.54).
- Consumes: the existing move loop, the sprint machine, the camera and raycast readers.

- [ ] **Step 1: The flag (RED).** The three physics cases against the synthetic view; assert the flag is the collided walk's own output (one case with `dx > 0` blocked on x only). Run: `cargo test -p oxide-game physics` — red.
- [ ] **Step 2: Implement `StepOutcome` and the callers.** Run — green.
- [ ] **Step 3: The release (RED).** The truth table; the one-tick wall release through `session_replay` (walk into a wall, the sprint packet edge arrives per the M3 rule). Run — red.
- [ ] **Step 4: Implement the clause and the tick-order check.** Run — green.
- [ ] **Step 5: The eye (RED then green).** The eye literals; the camera and raycast crouch cases. Run — green.
- [ ] **Step 6: The docs amendment.** Edit the M3 plan's pin text and the `player.rs` doc; re-read both for the one-movement rule (the M3 plan is amended only where the pin was wrong — nothing else changes).
- [ ] **Step 7: Gate and commit.** Commit:

```
fix: close the player carry (collision flag, sprint release, sneak eye)
```

**Verification:** `cargo test -p oxide-game`; `cargo test -p oxide-client` (camera tests); fmt/clippy clean.

---

### Task 21: The carried API and citation items

**Goal:** The M2-era carries that are neither behaviours nor new features: `bake_variant`'s precondition note, the negative face-rotation error shape, the six-column test name, and the comment/citation sweep with its plan-line amendment.

**Files:**
- Read first: `refs/m2-final-review/deferred-minors.md` (its items 12, 16, 26 and 27 are this task's) and `refs/m3-final-review/deferred-list.md` (the items' own texts — the authoritative ask), `docs/STATE.md` "Next actions" item 9, and the M3 coverage row "Backlog 4 — citation pass + the plan line ~553 amendment — carried (partial)" for what the close already did (Task 10's area notes).
- Modify: per the items — `crates/oxide-assets/src/model.rs` (or wherever `bake_variant` lives) for its precondition doc; the negative-rotation error's construct site and message shape; the six-column test's name; the citation spots the two deferred lists name (e.g. `sky.rs`'s comment scopes); and the M2 plan's line for its amendment.
- Test: the touched suites stay green; comment-only changes are verified by the review, not by new tests; the renamed test runs under its new name.

**Interfaces:**
- Produces: no new public API; each item lands as its deferred text asks, and every item's disposition is recorded (landed, or re-carried with the reason) so the coverage table can point at something.

- [ ] **Step 1: The inventory.** Transcribe the four items' texts into the task notes; for each: the file, the current shape, the ask.
- [ ] **Step 2: The code-shaped items.** `bake_variant`'s note and the error shape (if the error item turns out to be a code change beyond its message, keep it in this commit only if it stays within the item's own text; otherwise split a scoped `fix:` commit — record which).
- [ ] **Step 3: The sweep.** The citation pass' named spots + the M2 plan-line amendment (explicit paths; the plan text is corrected only where the item says).
- [ ] **Step 4: Gate and commit.** Commit:

```
docs: close the carried API and citation items
```

(plus the scoped `fix:` commit if Step 2 split one).

**Verification:** the touched crates' suites green; `cargo fmt --all --check`; the review checks the comment scopes.

---
### Task 22: The acceptance run

**Goal:** The exit, measured: two clients on the rig — one vanilla, one ours — see each other, chat, and agree on entities; every M4 checklist row leaves with evidence; the report lands under `refs/m4-acceptance/`.

**Files:**
- Create (uncommitted evidence): `refs/m4-acceptance/` — the scene scripts, captures, logs, and `report.md`, following the M3 acceptance layout (its `shot.sh` pattern, executable — the 644 lesson carries).
- Modify: `crates/oxide-client/src/main.rs` — the script-mode tick log gains a compact per-tick entity section (each tracked entity's id and position, one line per entity) so the motion leg has a numeric record (a rig surface, documented beside the existing script directives).
- Modify: `docs/parity/checklist.md` — the M4 rows' evidence and status: items 11, 12, 13, 43, 44, 45, 46, the new tab-list row, and the §16 nametag row (finalised below); the review-found wording corrections land in the same touch — item 11 (the fade is ≈10 s total, ≈1 s fade), item 12 (sidebar numbers are red unconditionally; the kind's branch is the tab list's), item 43 (the item timing and scale are the source's own), item 53 (the auto scale has no ≤3 clamp) — and the summary line's counts and composition are refreshed for the new row.

**Scenes and evidence (each: setup → captures → numbers):**
1. **Mutual view.** Both clients teleported to fixed poses ~12 blocks apart facing each other (`/tp` with exact angles; window-free captures after the activation-kick workaround carries), `/time set 6000` + `/weather clear` re-sent before each capture, both soaked past the mesh drain. Evidence: our frame of the vanilla player and the vanilla frame of ours (front, full body, nametag visible), the pair through the crop-and-metric treatment, by-eye, and the **nametag measurement**: from the vanilla frame at the known distance and pose, measure the name's height, the background's alpha (a pixel probe), the above-head offset, and the range rule — the client renamed to sneak at 33 blocks (no name) and at 31 (name) — and record all numbers into the checklist's nametag row (spec §16's obligation; the task finalises the row it added at close of Task 13's checklist touch).
2. **The zoo (static).** `/summon` a representative set with `NoAI:1` on a flat line at known coordinates — at least: zombie, skeleton, creeper, spider, enderman, slime, sheep (dyed once and sheared in a second pass), cow, pig, chicken, villager, wolf; and the objects: dropped stone (`/summon Item` with the NBT), a dropped diamond sword, an XP orb, a painting on a wall, an item frame with an item, a boat, a minecart, an arrow. Both clients at identical poses; frame pairs; the entity crops computed from the scene's world coordinates through the shared camera pose and compared with the metric within the M3 tolerance, plus by-eye on the full frames (the HUD bands differ by design — ours has no hotbar yet; crops clear of the overlay region carry). The roster's remaining families are captured in a second sweep (the exotics row) with by-eye only.
3. **The motion leg.** A wandering mob and a `/tp`-scripted sequence of known points: the per-tick entity log from our client checked against the `/tp` coordinates (numeric), the interpolation visible across two captured frames (by-eye), and the log's cadence asserted (one entity line set per tick).
4. **Chat, send and receive.** Our client sends `hello from oxide` through the script `chat` macro; the vanilla frame shows the line; the console log carries it. The console `/say` line and a `/tellraw` line (styled: colour, bold, and an explicit `§`-free JSON) arrive in our frame with the styles visible (by-eye + the exact strings in the log).
5. **Chat, click and hover.** `/tellraw @a` with a `run_command` `CLICKME` and a `show_text` `HOVERME`; the script opens the chat, moves the cursor onto the tooltip (capture), clicks (the server console shows the executed `/say clicked` — the click event proven end-to-end), and a `suggest_command` run against the field (the field's text after the click appears in the next frame).
6. **The tab list.** Our script holds Tab (down, capture, up) with both players online; the frame shows both entries, the heads, the latency bars, the header/footer absence recorded (the vanilla rig cannot set them — no command; the header/footer cases are unit/GPU only, recorded in the row). The Tab capture is ours; the vanilla side is the by-eye companion from its player list — or recorded as such if the vanilla capture is not obtainable (the KWin limitation noted at the close).
7. **The scoreboard.** Console: `objectives add` (`dummy` sidebar `demo`, plus a `health`-kind objective for the tab list's branch check — the sidebar numbers are red regardless of kind, per the source), scores for both players, a team with a colour, prefix and suffix applied to our account; `setdisplay sidebar`, `setdisplay list`, `setdisplay belowName` in three captures. Both clients' frames; the pair compared; the red-numbers case by-eye with a pixel probe; the team colour visible on the sidebar, the tab list and the nametag in one sweep.
8. **The boss bar.** A wither in a barrier cage (`/fill` the box first): both clients capture the bar (name, fraction, the wither's tint); `/kill` it and capture the decay frames; the fraction at capture time recorded against the wither's health (`/entitydata` read).
9. **The summary.** Counts of what the zoo drew (the entity log), the persistence leg (an entity in and out of view — the log's add/remove; by-eye), and the multi-scene pass list. `report.md` carries: every scene's setup, the frame paths, the metric numbers, the measurements, the by-eye list (the operator's), the deviations found, and the corrections log — then the checklist's rows are updated to their final states (evidence pointers, any corrected expectations) and the milestone's Known limits check off against what the run found (each limit either exercised-as-limited or untouched, recorded).
- The rig is stopped cleanly afterwards: server stopped (world saved), both clients killed, ports 25565/25566 confirmed free.

- [ ] **Step 1: The rig and the machinery.** Server up, both clients in and soaked, `shot.sh` executable, the scene scripts written under `refs/m4-acceptance/`.
- [ ] **Step 2: Scenes 1–3.** Frames, logs, the nametag measurement table.
- [ ] **Step 3: Scenes 4–5.** The chat frames, the console excerpts, the click proof.
- [ ] **Step 4: Scenes 6–8.** Tab, scoreboard, boss.
- [ ] **Step 5: The log extension.** The entity tick-log lines land (code change, committed with its tests per the script-mode rules).
- [ ] **Step 6: The report and the checklist.** `report.md` complete; the checklist rows final; deviations routed (a code fix lands as its own scoped commit before the close if the run finds one — the M3 practice).
- [ ] **Step 7: Stop and verify.** Ports free; the gate green on the last committed content.
- [ ] **Step 8: Commit.** Commit:

```
test: add the M4 acceptance evidence machinery to the script log
```

(plus any scoped fix commits the run forced).

**Verification:** every frame path in the report exists; the metric numbers re-run from the report's own commands; `gh run list --limit 1` green on the pushed head before the close begins.

---

### Task 23: The close-out

**Goal:** Freeze, review, document, tag — the M3 close's shape, one milestone on.

**Files:**
- Modify: `docs/STATE.md` — the M4 evidence section, the caveats (what this run could not close), the ordered M5 backlog (assembled from this milestone's carries: the fog colour's time term at the next client-fog touch, `CHUNK_COORDINATE_BOUND` if the footprint changes, the `slab_half` fixture when a protocol surface needs it, T8's F7 and the remaining deferred minors, the new M4 carries), and the environment facts (the rig lessons this run added).
- Create: `docs/handoff/2026-XX-XX-m4-close.md` — the handoff for the fresh session (the M3 handoff's shape: mission, context, current stage, exact next step, open questions, environment, verification gates).
- Modify: `CHANGELOG.md` — the milestone's entry (the M2/M3 style: what landed, the two dependencies, the divergences and their retirements).
- Modify (only if a ruling corrects it): `docs/specs/oxidecraft-v1-design.md` — the revision-history line bumps; otherwise untouched and the close says so.

- [ ] **Step 1: Final whole-branch review.** Freeze the milestone diff (`m3^{commit}`..HEAD) to a file; run one final whole-branch review over it plus the ledger's deferred-minor and parked lines; verify its claims against primary sources; on findings, one fix dispatch and one scoped re-review; adjudicate residuals. (The close bar carries: no Critical or Important findings; every deferred item triaged with a disposition.)
- [ ] **Step 2: The docs sweep** (the files above, one movement; the handoff written for a fresh session).
- [ ] **Step 3: The content gate + truth pass.** The six-command set on the committed content; the milestone-boundary prose sweep over `docs/` (the established pass, run at each close); explicit paths staged.
- [ ] **Step 4: Close commit, tag, push.** `docs: close out milestone M4 (entities and chat)`; annotated tag `m4` with message `M4: entities and chat`; `git push origin main --follow-tags`; `git ls-remote origin main` read back.
- [ ] **Step 5: CI on the pushed head.** `gh run list --commit <sha>` — all jobs green; a failure routes back through a scoped fix.
- [ ] **Step 6: The run-record follow-up.** Mirror M3's: update STATE's CI bullet with the run id; commit `docs: record the M4 close-out CI run`; push; confirm green.
- [ ] **Step 7: Ledger close.** Mark the plan complete in the ledger; collect the milestone's rulings for the owner's summary.

**Verification:** `gh run list --commit` on both pushed heads; `git ls-remote origin main` equals local HEAD; worktree clean; both rig ports free.

---

## Coverage table

The binding row is spec section 13's **M4 Entities and chat** row: *"Entity spawn, movement, metadata, skins (including skin texture fetch for online players), mob box models, object entities (arrows, thrown and dropped items, boats, minecarts, item frames, paintings, XP orbs), nametags, interpolation with the 4-block snap, chat with wrapping and click events, tab list, scoreboard, boss bar"*, with exits *"Two clients, one vanilla and one ours, see each other, chat, and agree on entities"*.

| Binding item | Task(s) | Evidence |
| --- | --- | --- |
| Entity spawn, movement, metadata (§13 row; §9) | 1, 2, 4, 5 | The fixture corpus; the store's scenario suite; the session's scripted replay |
| Skins incl. fetch for online players (§13 row; checklist 44; launcher survey §3.6–3.8) | 7, 8 | The default-rule golden vectors; the property fixtures; the ignored live fetch; scene 1's frames |
| Mob box models (§13 row; checklist 45) | 9, 10, 11 | Per-model geometry and pose pins; the roster completeness test; the zoo frames; the animation comparisons over the zoo subset (the full sweep is M9's — recorded on the row) |
| Object entities (§13 row; checklist 43) | 12 | The table and rule pins; the GPU cases; the zoo frames |
| Nametags (§13 row; §16's obligation) | 13, 22 | The GPU cases; the rig measurement written into the checklist row |
| Interpolation with the 4-block snap (§9; checklist 46) | 4, 5, 8 | The pair/tick tests; the lerp and snap tests; the motion leg |
| Chat with wrapping and click events (§13 row; §11.2; F6; checklist 11) | 3, 14, 15 | The parser/wrap/fade suites; scenes 4–5 (incl. the click proven at the server) |
| Tab list (§13 row; §11.2; F7; the new checklist row) | 3, 5, 6, 16 | The layout pins; scene 6's frames |
| Scoreboard (§13 row; F7; checklist 12) | 3, 6, 17 | The state and formatting suites; scene 7's frames |
| Boss bar (§13 row; checklist 13) | 18 | The fraction and decay pins; the GPU cases; scene 8's frames |
| Exit: two clients see each other | 8, 22 | Scene 1 |
| Exit: chat | 14, 15, 22 | Scenes 4–5 |
| Exit: agree on entities | 5, 8, 12, 22 | Scene 2's crops + the log; scene 3's numbers |
| §6 keepalive and read-loop discipline | 5, 6 | The burst tests stay green; the tick-feeds-once tests |
| §8 ordering (list item before spawn) | 5 | The session fixture |
| F5 (see other players, mobs and objects with interpolation) | 8–12, 22 | Scene 2 |
| F7's player list and sidebar | 16, 17, 22 | Scenes 6–7 |
| Backlog 1 — the stage map re-key + 0x25 filter | 19 | The re-key suite; the filter fixtures |
| Backlog 2 — sprint release on collision | 20 | The truth table; the one-tick wall case |
| Backlog 3 — the sneak eye + the plan amendment | 20 | The eye literals; the camera and raycast cases; the amended pin |
| Backlog 4 — `attacked_at_yaw` | 20 | **Finding:** the source's client never computes a hit direction in multiplayer (`performHurtAnimation`, `EntityLivingBase.java:1183-1187`, zeroes it; the only setter is the server-side attacker branch), so the source-faithful value is zero — pinned by a comment and the camera test; the carry closes as a non-defect |
| Backlog 5 — the overlay wiring pin | 19 | `overlay_wiring` plus the descriptor pins (both P1/P2-sensitive) |
| Backlog 6 — the fog colour's time term | carried | No M4 task edits the fog-colour computation; the note routes to the next client-fog touch (M6's sky weather pass) |
| Backlog 7 — `CHUNK_COORDINATE_BOUND` | carried | No M4 task changes the light recompute footprint |
| Backlog 8 — the `i64::MIN` receive rule + end-drain expiry pin | 6 | The guard test; the drain pin (per the M2 item's own text — landed or re-carried with the reason recorded) |
| Backlog 9 — the carried API and citation items | 21 | Each item's disposition recorded |
| Backlog 10 — `slab_half`, T8's F7, the deferred minors | close-out | Recorded with their triggers on the M5 backlog; the triage stays per the M3 close |
| Dependencies (`base64`, `serde_json`) | 7, 14 | `deny` green; the commits carry the lockfile |

The M3 final review's risk list is answered in turn: the stage-map re-key and the 0x25 entity filter (Task 19), the sprint-release collision flag (Task 20), the sneak-eye offset and the plan pin (Task 20), `attacked_at_yaw` (Task 20 — closed by the source's own finding above), the overlay wiring discriminator (Task 19), the fog colour's time term (carried with its trigger, recorded above), and `CHUNK_COORDINATE_BOUND` (carried with its condition, recorded above).

## Pre-flight (before Task 1)

- The owner's answers are recorded in the table above; the plan is committed together with the pre-flight document corrections the plan review surfaced (one docs commit, explicit paths): the checklist corrections this review routed to their touch points are confirmed against the plan (item 13 — Task 18; items 11/12/43/53 and the summary counts — Task 22; the nametag and tab rows — Tasks 16 and 22), and any citation the review's source-verification pass corrected is re-checked once.
- The milestone's execution ledger begins (first line names this plan); the reference tree's HEAD is recorded with it: `1717f75902c6184a1ed1bfcd7880404aab4da503`.
- The conflict scan (every task pair sharing a file or interface, plus one self-consistency row per task) is written to the ledger, and each conflict is ruled on before the first dispatch; the review loop is only the net for what implementation reveals.
- `rustup update stable`; the local gate is green on the base commit; `gh run list --limit 1` confirms the base is green in CI.
- The worktree is clean and the `m3` tag is intact; the rig is idle (both ports free).
- The `base64` addition (Task 7) and the `serde_json` dependency (Task 14) carry their `Cargo.lock` updates and `deny.toml` stays green in their own commits.

## Known limits this plan accepts

1. Held items and armour on entities are decoded and stored but never drawn (M5's item models close the class); the rig's scenes give nobody equipment.
2. The non-block item table is minimal (the pinned set); ids outside it draw the missing sprite; the full registry is M5's.
3. Thrown potions draw the base sprite without the brewed tint (the potion-colour table is M5's).
4. No chat tab-completion: the source's own completes player names (`GuiChat.java`'s Tab check at `:91`, completion path `:203-266`) and, for commands, leans on server-driven data — both need machinery this milestone does not build. Tab is swallowed while the chat is open.
5. `OPEN_URL` opens only through the interim confirm overlay (Enter/Esc, the system opener); the source's `GuiConfirmOpenLink` screen is M6's.
6. Legacy 64×32 skins are not converted at load (rare; the decode-refusal is recorded rather than guessed at).
7. The skin fetch's live evidence is the ignored endpoint test alone — the rig is offline-mode, so no session exercises a fetch live.
8. The tab-list evidence is our client's frames; the vanilla side cannot hold Tab synthetically on this desktop, and the header/footer have no vanilla command (unit/GPU only) — both recorded on the row.
9. The boss bar is the 1.8 single status; a second boss overwrites (checklist 13 reconciled).
10. The roster's animations are geometry/pose literals plus the zoo frames; the checklist's frame-by-frame animation comparisons run over the zoo subset — the full per-mob animation sweep is M9's.
11. No entity sounds until M7; entities are silent.
12. The pickup animation (0x0D) is stored, not animated; the item disappears on the destroy packet, as the server sends both.
13. Lightning (global entities) is tracked, never drawn. Primed TNT, falling sand, ender crystals, leash knots, fishing bobbers and armour stands are outside the covered object set and draw nothing.
14. The deferred model layers: creeper charge aura, wither aura, dragon detail layers (eyes, death rays), custom heads, deadmau5 ears, mob armour — recorded with the item-model class.
15. Packets outside the decoded set (entity properties, effects, NBT updates, titles) stay skipped per the S2 rule.
16. The entity feed is O(tracked entities) per tick; interest management, if ever needed, is M9's.
17. The scoreboard's objective `kind` strings and team colour semantics are pinned to what the 1.8 source reads; a modded server's unlisted values degrade as the source's own fallbacks do (recorded per case).
18. The operator's by-eye list for M4's frames stands as the M3 practice's continuation (the capture list is the report's own).
19. The player part toggles (cape, hat, jacket, sleeves, pant legs) draw the pinned all-on default (`0x7F`) for every player this milestone: the per-player skin-flags byte and the local settings source arrive with M6's settings work.

20. The texture keys, as executed: the giant renders on the zombie sheet — `entity/zombie/zombie.png` (`RenderGiantZombie.java`:13); the iron golem's sheet is `entity/iron_golem.png` (`RenderIronGolem.java`:11); the sheep's wool layer reads `entity/sheep/sheep_fur.png` (`LayerSheepWool.java`:12). The plan's `entity/giant.png`, `entity/iron_golem/iron_golem.png` and `entity/sheep/sheep_wool.png` were refuted during execution. *(added after execution; certified by the Task 9 review)*

21. Store-facing texture names go through the assets layer's `store_key` mapping: the `TextureSet` keys are extensionless (`misc/shadow`), while the pass registers `.png` names; the mapping strips a `.png` suffix and passes anything else through. The mismatch — every entity-texture lookup missing — was found and fixed during Task 9. *(added after execution; found and fixed in Task 9)*

22. The part transform carries an `offset` field for the source's block-unit offsets, weighted ×16 into the model's 1/16 units at composition time (the source translates unscaled after the model scale — `ModelRenderer.java`:137); the witch's nose is its first user. *(added after execution; introduced by Task 9)*

23. The three block-drawing identity layers defer to Task 12's block-item path (recorded in the layers module): the snow golem's jack-o'-lantern (`LayerSnowmanHead.java`:25-30), the iron golem's rose (`LayerIronGolemFlower.java`:24-43), the mooshroom's mushrooms (`LayerMooshroomMushroom.java`:25-51). *(added after execution; certified by the Task 9 review)*
