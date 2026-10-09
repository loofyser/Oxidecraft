# M5 HUD and Inventory Implementation Plan

> **How to work this plan:** one task at a time, in order. Run each step's verification before moving on, and tick the checkboxes (`- [ ]`) as you go. Every commit is made on `main` with explicit `git add` paths.
>
> **Plan style:** this plan pins every interface, constant, path, and rule the tasks must agree on, and carries code sketches wherever a byte layout, formula, or data shape is the deliverable. Algorithm bodies that mirror vanilla behaviour are derived from the cited MCP-919 source files (`refs/_src/MCP-919`, local and uncommitted; the tree is present from the previous milestones, HEAD `1717f75902c6184a1ed1bfcd7880404aab4da503`) by the task that needs them, and every derived constant is pinned by a test. Where this plan and a cited document disagree, the document governs and the plan text is corrected first. Pin the source's expression and cite the method that applies it where the two differ — a factor can live in the applier, not the computer.

**Goal:** The HUD and the inventory. The player sees their own things and uses them: the hotbar with item icons, counts and durability bars; the health, hunger, armour, experience and air rows with the source's blink and variant rules; the selected-item popup and item tooltips; every server-openable container — the player's own inventory, chests and generic 54-slot windows, hoppers, dispensers and droppers, furnaces, brewing stands, crafting tables, enchanting tables, anvils, beacons, villagers and horses — opens as the source's screen with drag and split click semantics over the live wire; sign editing and book reading work; the creative inventory browses, searches and gives.

**Architecture:** The window family of packets feeds a session-owned window state: window 0 (the player's own 45 slots) always exists, the server opens one window at a time, the cursor rides window id −1, and the session publishes a full snapshot on every change beside the existing events — the window keeps no merge logic. The wire codecs grow the slot payload with its bounded NBT tail plus a read-only NBT reader for display; `oxide-world` gains the inventory model the containers operate on; the session gains the click machine (the source's modes, drag state and stack arithmetic) and sends the serverbound actions. The assets layer bakes the item model set — the generated spans, the block-item fallbacks and the display transforms — and the client's item registry table resolves every id to its sheet or model; the renderer grows GUI item draws (a no-mip sampler for sprites, a GUI-space `display.gui` path for block items), a first-person held-item pass between the scene draws and the dim quad, and equipment layers over the M4 entity pass; the HUD pass grows the hotbar, the bars, the tooltip path and the item draw kinds over the shared text builder; a minimal screen framework carries the container screens, the sign editor, the book reader and the creative screen, routing input and handing the pointer back while a screen is open. The hold on the wire is the source's own: the client never predicts server containers — it sends the click and takes what the server answers.

**Tech Stack:** Rust 2024 edition (rust-version 1.85, developed on 1.99), the M4 stack unchanged. **No new dependencies.** The spec's appendix B plans `simdnbt` for NBT; this milestone needs only a bounded read for display (tooltips, book pages) plus a byte-exact raw echo of slot payloads, so a hand-rolled read-only reader lands in `oxide-proto-v47` instead — the spec's own fallback clause, recorded as Decision 2. No new crate edges — every task stays inside the section 5.1 table.

**Spec:** `docs/specs/oxidecraft-v1-design.md` (v6). M5 is the section 13 row "HUD and inventory", with exit *"Inventory and container round-trips match the vanilla client screen for screen"*. Read section 11.2 (the HUD and container lists) and `docs/research/render-parity-survey.md` §3.1–3.4 (the screens, HUD elements, F3 and container-interaction tables) before Tasks 14–23; section 10 and the survey's §1.4 (the atlas rule) before Tasks 7–11; section 8 (the item stack) and `docs/research/protocol-47-reference.md` §2 (the window, sign and experience rows) before Tasks 1–6; section 5.3 before Tasks 4–9; section 9 before Tasks 5–6; the distilled HUD parity facts at `refs/m5-homework/hud-render-parity.md` (mirrored there before Task 14) before Tasks 14–17; section 16's DIVERGENCES obligation before Tasks 22–23. Vanilla citations are files under `refs/_src/MCP-919` (uncommitted), the same source the research reports cite. The M4 final review's "what will fight M5" list (`refs/m4-task-23/final-review.md` §2.4 — the carry notes in `docs/STATE.md` itemise it) is this plan's standing risk list, and each item is answered by a task below.

## Global Constraints

- License GPL-3.0. Adapted third-party code must be recorded in `NOTICE`.
- Zero code copied from RustCraft. It is a read-only reference only.
- No Mojang asset, jar, `.class` file, `.ogg`, or `.png` may ever be committed. Nothing under `refs/` or `vanilla/` is committed; textures, models, fonts and colormaps are read from the user's own store at runtime only. No test fixture may embed a Mojang pixel: fixtures are synthetic (generated in the test) or read from the store by an ignored test.
- Never read `.class` files from the jar at runtime. Design invariant.
- rust-version 1.85, edition 2024. `Cargo.lock` is committed.
- CI must fail on: formatting, clippy warnings, test failure, license violations, crate-graph violations, or a tracked Mojang asset.
- This milestone adds **no dependency and no crate edge** — the allowed edges are exactly the section 5.1 table, which `scripts/check-graph.sh` keeps asserting unchanged. (Decision 2 records the NBT reader's home; the spec's appendix table row is amended by this plan.)
- Every public item carries a doc comment (workspace lint `missing_docs`); `unsafe_code` is forbidden workspace-wide.
- `git add` is always explicit with paths; never `git add -A` or `git add .`.
- Evidence (captures, screenshots, logs) lives under the git-ignored `refs/` tree. Committed documents cite it by path.
- The local gate before every push is the six-command set plus both guard self-tests and the parity self-test: `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test`, `bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test`, `python3 scripts/parity-diff.py --self-test`.
- Values that come off the wire are hostile until validated: every slot payload, window id, slot count, property count, item count, string title, sign line, merchant offer list and NBT tag is checked before it sizes an allocation or a loop; framing never panics (spec S2). The item registry's store-derived sheet names and the NBT reader's own caps follow the same rule.
- Numerical parity: every GUI layout coordinate, slot table, click-mode mapping, stack arithmetic rule, drag-machine constant, item-model transform, item-icon geometry, HUD layout value, bar segment rule and tooltip metric this milestone introduces is traceable to a cited MCP-919 file or a research report, and is pinned by a test. A constant with no citation does not land.
- The keepalive/read-loop discipline is binding and extends to every new handler: the play loop's added per-packet work stays bounded (a write into the window state or a sign map, a snapshot build on change — never per-frame layout, model or JSON work in the read pass), and the `session_replay` keepalive test must keep passing through every restructuring.

## Decisions taken in this plan

| # | Decision | Rationale |
| --- | --- | --- |
| 1 | **The window, sign and experience wire surface decodes in `oxide-proto-v47`.** Clientbound 0x2D (Open Window), 0x2E (Close Window), 0x2F (Set Slot), 0x30 (Window Items), 0x31 (Window Property), 0x32 (Confirm Transaction), 0x33 (Update Sign), 0x36 (Open Sign Editor), 0x1F (Set Experience) and 0x1D/0x1E (entity effects) decode beside the existing set; serverbound grows 0x09 (Held Item Change), 0x0D (Close Window), 0x0E (Click Window), 0x0F (Confirm Transaction), 0x10 (Creative Inventory Action), 0x11 (Enchant Item) and 0x12 (Update Sign). The merchant offer list (0x3F `MC\|TrList`) parses from the custom-payload data with the protocol §2.1 lengths checked. The protocol reference's ⚠ on Set Slot (\"a negative slot index denotes the cursor … not verified\") resolves against the source: the cursor rides **window id −1**, not a negative slot (`NetHandlerPlayClient.handleSetSlot`:1133 — its `func_149175_c() == -1` branch writes `inventory.setItemStack`), and Task 2 corrects the note. | The M4 precedent keeps all wire identifiers in the v47 crate; the ids are protocol facts. The ⚠ is the reference's own marker to resolve at this touch. |
| 2 | **Slot payloads keep their NBT as bounded raw bytes, and a read-only NBT reader lands in `oxide-proto-v47`.** `MetadataItem` gains `nbt: Option<Vec<u8>>` — the tail captured under the existing `MAX_SLOT_NBT_BYTES` cap and re-emitted byte-exact on echo (the server compares clicked stacks including NBT; a dropped tail desyncs a round-trip). Display needs (tooltips, book pages) read the tail through a new bounded `nbt` module (`NbtValue`), hostile-input capped at every level. `simdnbt` is not taken: the write path is unused, the caps discipline is this crate's own, and a dependency would buy nothing this milestone. | Server session round-trip: `PacketClickWindow`'s clicked item must byte-match or the server re-syncs; book reading and tooltips need typed access. The spec's appendix fallback clause covers the reader's home. |
| 3 | **The inventory model lives in `oxide-world`.** `oxide-world/src/inventory.rs` grows `Inventory` — 36 main slots, 4 armour slots, the cursor stack, the selected hotbar slot — shaped after `InventoryPlayer` (`entity/player/InventoryPlayer.java`:24-30, accessors :50, :121, :165). Stacks are the wire's `MetadataItem` (the allowed `oxide-world → oxide-proto-v47` edge, the M4 precedent for equipment). | Spec §5.3 keys the inventory to `oxide-world`; it is world state, not wire or render state. |
| 4 | **The session owns every window; the window replaces on snapshot.** Window 0 exists from Join Game; a server Open Window (0x2D) replaces any open window; Close Window (0x2E) or our Close (0x0D) clears it; the cursor rides window id −1; properties (0x31) and all slot writes land in the state. Every change publishes `ClientEvent::WindowSnapshot { window_id, slots, cursor, properties }` — a full copy, bounded by 45–90 slots, changes are rare, and the window side keeps no merge logic (the M4 `EntitiesTick` principle). Opens and closes publish `WindowOpened { window_id, kind, title }` / `WindowClosed { window_id }` with `WindowKind` from the type string (`minecraft:chest` … `EntityHorse`; unknown types keep `Unknown` and open the generic screen — recorded). | One owner for the wire state; the view's screens read snapshots. The source's `openContainer` model is client-side state too (`PlayerControllerMP`/`Container`) — this is its port. |
| 5 | **The click machine is the source's, and the client never predicts.** `oxide-game` grows `Container`-shaped click arithmetic: the mode/button derivation (0 single/pickup, 1 shift quick-move, 2 number-key swap, 3 creative pick, 4 drop, 5 drag, 6 double-click gather), the outside slot (−999), the drag machine (the four statuses, the remnant math, `getDragEvent`/`isValidDragMode`/`canAddItemToSlot`/`computeStackSize` ports — `Container.java`:695, :705, :722, :738; `GuiContainer.java`:66-71, :115-436), `mergeItemStack`/`transferStackInSlot` for the creative screen's own container (:597, :131), the action-number counter and the confirm loop (`handleConfirmTransaction`:1174 re-accepts a rejected window), and the send paths for 0x0E/0x0D/0x0F/0x10/0x11/0x12 plus the drop keys (0x07 actions 3/4). Server containers take the server's answer — no client-side prediction outside the creative screen (the source's own rule). | Spec P4/S2 and the exit's round-trip duty: the wire shape must match the source's own sends. The creative screen is the source's one predictive path (`GuiContainerCreative` calls the local container's `slotClick`). |
| 6 | **GUI item draws split by shape.** 2D generated-item sprites sample the atlas through a no-mipmap/no-blur sampler (`RenderItem.renderItemIntoGUI` binds the blocks atlas then `setBlurMipmap(false, false)` — survey §1.4, `RenderItem.java`:197/:318/:357); block items and the folded builtin trio draw GUI-space 3D through the baked model with the model's `display.gui` transform (composed after translate — the model JSON's own transform order), lit flat, with the enchant glint overlay as its own blended piece (`RenderItem.renderEffect`:170). The HUD pass gains `HudDraw::Item { stack, x, y }` and draws in list order. | The source draws both classes inside the GUI pass at the same z-level; the split matches its two branches and keeps the no-mip rule single-sourced. |
| 7 | **The item registry is a committed table derived from the source's registrations.** `oxide-client/src/items.rs` grows from the M4 slice to the full roster: every id `Item.registerItems` registers (`Item.java`:511, the pairs region :765-952), with its display name (the boss-name precedent — the source's en_US strings, the asset file never committed), max stack, max damage, creative tab, and one resolution: block item (its block model), generated (its `layer0…4` textures through the M4 builtin/generated path), the folded builtin trio (Decision 15), or missing. Tooltip attribute lines derive per class from `getAttributeModifiers` (swords, tools, armour). Completeness is a count against the source's own registrations plus spot pins; ids outside draw the missing sprite and are recorded. | The row says "item icons"; the registry is their resolution. The M4 limit 2 ("the non-block item table is minimal") closes here — exactly what the final review routed. |
| 8 | **The first-person item is the source's `ItemRenderer` shape.** `itemToRender`/`equippedProgress`/`prevEquippedProgress` (:37, :42-43), the equip ease, the swing translation via `getSwingProgress` (:182-195 usage; the counter pair rides the own entity's animation state), the per-class `display.firstperson` transform, the food-eating arm offsets only when their state exists — Task 12 derives the frame-limited subset and records absences. Drawn between the scene draws and the dim quad (the source draws it inside the world pass, `EntityRenderer.java`:864-876, before the overlays and with the lightmap enabled); skipped while the player sleeps (`:863`'s condition; third-person and hide-GUI do not exist yet — recorded). | One pass, one cite chain; the skip conditions are the source's own. |
| 9 | **Equipment draws ride the M4 entity frames.** The frame's `equipment` grows to the full `MetadataItem` (with the NBT tail) so the third-person held item (`LayerHeldItem`:27, :66) and the armour layers (`LayerArmorBase.renderLayer`:45, the leggings/body picks, the leather dye tint and the enchant glint — `ItemArmor`:63, :75, :119, :129) draw over the existing pose machinery. The layered extras fold per the owner's question: the creeper and wither charge auras and the deadmau5 ears ride this task when answered yes; custom heads and the dragon detail layers re-carry (their fetch and geometry machinery is not this milestone's). | M4 decoded and stored equipment precisely so this class could close here; the frames' shape is the seam. |
| 10 | **The HUD reads events, keeps its own timers.** `Health` stays as-is (0x06); new events carry the rest: `Experience { bar, level, total }` (0x1F), `Air { air }` (the own entity's metadata index 1 — the own player receives its own metadata stream; the M4 own-id routing for entity status is the seam, `Session`'s status branch — the task re-derives the metadata path first), `Effects { effects }` (0x1D/0x1E, own player), `HeldItemSlot { slot }` (0x09 both directions). The blink counters (`healthUpdateCounter`, the food jitter, `lastSystemTime`) are view-side, exactly as `GuiIngame` keeps them (:609-660). The hotbar's `animationsToGo` (5 ticks, `NetHandlerPlayClient`:1158; the pop's partial-tick math `GuiIngame`:1043) rides the window-0 snapshot path. | The M4 split: the session owns wire state, the window owns draw state; the counters are draw state. |
| 11 | **GUI scale stays auto-only.** `ScaledResolution`'s auto rule (M4's Decision 8) is the only path; `set_gui_scale` stays wired and uncalled until M6's settings store (the M4 record 42 stands). Every M5 layout consumes the existing scaled resolution. | The settings screen is M6's; parity here is at the default scale, recorded. |
| 12 | **The input surface grows at its existing seam.** `keymap` gains E, Q, the digits 1–9 and the wheel surface (outside screens the wheel cycles the selected slot — `Minecraft.java`:1879's `changeCurrentItem`; the M4 note at `main.rs`'s wheel arm names this milestone); `input.rs`'s milestone wording (D1) and the `INPUTS_PER_PASS` pin ride the touch; screen routing (which screen consumes which key) lives with the screens. | The carry set's routing rule: each item lands at its file's next touch, in this milestone's own tasks. |
| 13 | **The acceptance is scene-driven on the rig.** The scripted-input mode grows directives for the new gestures (pointer move, click, drag, number key, wheel, close, chat) and the acceptance drives our client through the round-trips; the vanilla side's states are staged through the server console (`/replaceitem block …` for container contents, `/give` after respawn) and its screens are captured by hand by the operator where a gesture is needed. Checklist rows 7, 8, 9, 14, 15, 16, 17, 18, 19, 20, 25, 53 and 57 land their evidence; row 17's sheet-size gap closes from the store's own texture files plus the frames. | The rig's established discipline (Task 27's brief): the desktop cannot type into our own window, and the source's states must match before screens can be compared. |
| 14 | **The carried citation set lands in one task, the sweep tooling with it.** The M2 F3 remainder (items 1, 26, 31.1, 31.2, 32, 33, 36(a), 36(c), 37, 38, 39 — the F3 cluster plus row 38), the D1–D9/D12/D27 set, `view.rs`'s stray backtick, the `light.rs`:123 drift, the twelve-site `WorldClient.java` family and `clientbound.rs`:222 all resolve in Task 25, together with the extractor's range-inside grammar (the sweep-tooling round), because the sweep is the tooling's proof. The PNG-helper consolidation and the 16-bit/tRNS strip fixture land as Task 26 at the assets tests' next touch. | STATE's own routing: the sweep set rides the next comment sweep, the tooling round "stands as its own item" — kept together here so the round's tokens are re-swept by the tool it fixes. |
| 15 | **Builtin/entity items, per the owner's question: the chest trio folds, the rest records.** Chest, trapped chest and ender chest icons draw through a small box model in the GUI path (they page through the creative screen's first tab); skulls, banners and other `builtin/entity` ids draw the missing sprite this milestone and are recorded (their texture fetch and model machinery is a later pass — the same class as the unbuilt tile-entity renders). | Bounded fold with visible payoff; the rest is honest scope. |
| 16 | **Book and quill open reads.** `MC\|BOpen` opens the reader for both item kinds; an editable book shows its stored pages read-only with the editing affordances absent — a `docs/DIVERGENCES.md` entry (writing is post-v1 per §11.2), not a screen. | The §16 rule for post-v1 surfaces, applied at the one seam where 1.8 branches. |

## Open questions for the owner (answers recorded on approval)

1. **The HUD and screen extras with no other home** (all `§11.2`/F9 rows): the crosshair (row 10; the blend work is this milestone's), the damage flash overlay (the state exists since M3; only the draw is missing) *(added after execution; the item closed as a source-falsified premise — the tree has no screen flash; see Task 15's note)*, the enchanting table's glyph background (the SGA font sheet — the screen looks wrong without it), and the potion-effect list on the inventory screen (needs the effect packets, which this milestone decodes) all fold into M5's tasks. **Recommended: yes** — each is small, each sits exactly on machinery this milestone builds, and none has a later milestone that claims it.
2. **The deferred entity detail layers (M4 limit 14).** The creeper and wither charge auras and the deadmau5 ears fold into Task 13 (overlay draws and a small model add-on over the layer machinery this milestone builds); custom heads and the dragon's detail layers (eyes, death rays) re-carry with their trigger named (the mob-detail pass, with the tile-entity renders). **Recommended: yes** — the folds ride free; the re-carries need fetch and geometry machinery no milestone has built.
3. **The sign's in-world text.** Task 22 folds a minimal front-face text render (the round-trip's visible payoff — you edit, you see it) alongside the editor; the back face does not exist in 1.8. **Recommended: yes** — the task reads `TileEntitySignRenderer` anyway for the editor's preview.
4. **The book-and-quill open path (Decision 16).** Read-only pages plus a DIVERGENCES entry. **Recommended: yes** — spec §11.2 keeps writing post-v1; the alternative (not opening) hides content the server offers.
5. **Builtin/entity item icons (Decision 15).** The chest trio folds; skulls, banners and the rest record. **Recommended: yes** — bounded, visible in the creative screen's first tab, honest for the rest.
6. **Third-person views, F1's HUD hide and the spectator HUD** (checklist row 69, the section 11.2 camera and HUD items): no milestone row names them and no M5 task builds the camera modes or a spectator state; route all three to M6's camera and overlay work. **Recommended: yes** — this milestone's HUD gate already follows the source's own suppression rules, so nothing here needs rework when M6 lands the modes.

### Answers recorded on approval (2026-10-08)

| # | Question | Answer |
| --- | --- | --- |
| 1 | The HUD and screen extras with no other home | Fold all four into M5 — as recommended |
| 2 | The deferred entity detail layers | Fold the auras and the ears into Task 13; custom heads and the dragon's detail layers re-carried with their trigger — as recommended |
| 3 | The sign's in-world text | Fold the front-face text render into Task 22 — as recommended |
| 4 | The book-and-quill open path | The read-only reader plus the DIVERGENCES entry — as recommended |
| 5 | Builtin/entity item icons | The chest trio folds; skulls and banners record — as recommended |
| 6 | Third-person views, F1 and the spectator HUD | All three route to M6 — as recommended |
---

### Task 1: The slot payload and the NBT reader

**Goal:** The slot payload's NBT tail is captured and re-emitted byte-exact, and a bounded read-only NBT reader can walk it for display: `MetadataItem` grows its tail field, `oxide-proto-v47::nbt` gains `NbtValue` and `parse`, and both are hostile-input capped and test-pinned.

**Files:**
- Modify: `crates/oxide-proto-v47/src/entity.rs` — `MetadataItem` gains `nbt: Option<Vec<u8>>`; the slot decode captures the tail; a slot write helper lands beside them.
- Create: `crates/oxide-proto-v47/src/nbt.rs` — the reader, its caps and errors.
- Modify: `crates/oxide-proto-v47/src/lib.rs` — module declaration and re-exports.
- Test: `crates/oxide-proto-v47/src/nbt.rs` — inline unit tests (tags, caps, hostile cases).
- Test: `crates/oxide-proto-v47/tests/nbt_codecs.rs` — the fixture corpus (slot tails, book pages, hostile trees).

**Interfaces:**
- Produces:
  - `oxide_proto_v47::entity::MetadataItem { id: i16, count: u8, damage: i16, nbt: Option<Vec<u8>> }` — the decode consumes the tail under the standing `MAX_SLOT_NBT_BYTES = 65536` and keeps it verbatim (`None` when the tag byte is 0; a tail at the cap and one past it are both tested — past-cap still refuses as before). The M4 field set grows; every existing construction site is updated in this task.
  - `oxide_proto_v47::entity::read_slot(cursor) -> Result<Option<MetadataItem>, _>` and `write_slot(out, item: Option<&MetadataItem>) -> io::Result<()>` — encode is the byte-exact inverse of decode (id, count, damage, then the raw tail when present). `write_slot` is the type every window writer's slot field uses.
  - `oxide_proto_v47::nbt::NbtValue` — one variant per tag this milestone reads: `Byte(i8)`, `Short(i16)`, `Int(i32)`, `Long(i64)`, `Float(f32)`, `Double(f64)`, `ByteArray(Vec<u8>)`, `String(String)`, `List(Vec<NbtValue>)`, `Compound(Vec<(String, NbtValue)>)` (order preserved), `IntArray(Vec<i32>)`.
  - `oxide_proto_v47::nbt::parse(data: &[u8]) -> Result<NbtValue, NbtError>` — reads one tag (the root) from the tail; `NbtError` names each refusal (unknown tag, truncation, depth, count, string length).
  - Caps, pinned by test: `MAX_NBT_DEPTH = 16`, `MAX_NBT_STRING_BYTES = 65536`, `MAX_NBT_COLLECTION_LEN = 65536` entries, and the total bytes bound is the caller's 65536 tail. Strings decode as UTF-8 lossily — the source's `readUTF` is modified UTF-8; the one recorded divergence is an embedded NUL or an astral-plane pair (surrogate pair) inside a string, lossy-decoded with a comment (derive the exact shapes in-task; the byte-exact echo path is unaffected because it never re-encodes parsed strings).
- Consumes: the crate's cursor/error helpers; `MAX_SLOT_NBT_BYTES` as it stands.

- [ ] **Step 1: Slot-tail tests (RED).** In `tests/nbt_codecs.rs`: a slot fixture with a compound tail (a 2-entry tag), one with a 0x00 tag byte (`nbt: None`), one at the cap, one one byte past it (refusal unchanged), and an echo pair — decode then `write_slot` reproduces the input bytes exactly, tail included, for every fixture. Run: `cargo test -p oxide-proto-v47 nbt` — fails to compile (no module).
- [ ] **Step 2: Capture and re-emit.** Implement the `entity.rs` changes. Run — green; the existing `entity_codecs` suite stays green with its constructions updated.
- [ ] **Step 3: Reader tests (RED).** The tag walk: a nested compound (book-pages shape: `pages` list of strings), every numeric tag, the int-array, order preservation, and the hostile set — truncation at every position class, depth 17, a collection at the cap and one past it, a string at the cap and one past it, an unknown tag id, a NUL-containing string and a surrogate-pair string (lossy outputs recorded in the test). Run — red.
- [ ] **Step 4: Implement the reader.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate (Global Constraints). Commit:

```
feat: capture and re-emit slot NBT and read the tree (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`; `cargo fmt --all --check`; `cargo clippy -p oxide-proto-v47 --all-targets -- -D warnings`.

---

### Task 2: The window and sign codecs

**Goal:** The protocol crate can decode the window family, the sign pair, the experience update and the entity-effect pair, and can name a window's kind and parse a merchant offer list.

**Files:**
- Create: `crates/oxide-proto-v47/src/window.rs` — the structs, `WindowKind`, the offer-list parse.
- Modify: `crates/oxide-proto-v47/src/lib.rs` — module declaration and re-exports.
- Modify: `docs/research/protocol-47-reference.md` — the Set Slot note correction (the cursor note's ⚠ resolves; its own line).
- Test: `crates/oxide-proto-v47/src/window.rs` — inline unit tests.
- Test: `crates/oxide-proto-v47/tests/window_codecs.rs` — the fixture corpus.

**Interfaces:**
- Produces (all `oxide_proto_v47::window`):
  - `OpenWindow { window_id: u8, kind: WindowKind, title: String, slot_count: u8, entity_id: Option<i32> }` (0x2D; `entity_id` only for `EntityHorse`, per the protocol row); `CloseWindow { window_id: u8 }` (0x2E); `SetSlot { window_id: i8, slot: i16, item: Option<MetadataItem> }` (0x2F — window id may be −1, the cursor); `WindowItems { window_id: u8, slots: Vec<Option<MetadataItem>> }` (0x30, count-checked); `WindowProperty { window_id: u8, property: i16, value: i16 }` (0x31); `ConfirmTransaction { window_id: i8, action: i16, accepted: bool }` (0x32); `UpdateSign { x: i32, y: i32, z: i32, lines: [String; 4] }` (0x33, each line length-capped); `SignEditorOpen { x: i32, y: i32, z: i32 }` (0x36); `SetExperience { bar: f32, level: i32, total: i32 }` (0x1F); `EntityEffect { entity_id: i32, effect_id: u8, amplifier: u8, duration: i32, hide_particles: bool }` (0x1D); `RemoveEntityEffect { entity_id: i32, effect_id: u8 }` (0x1E). One decode fn per struct on the crate's cursor.
  - `WindowKind` — one variant per 1.8.9 window type the server sends (`minecraft:chest`, `minecraft:crafting_table`, `minecraft:furnace`, `minecraft:dispenser`, `minecraft:enchanting_table`, `minecraft:brewing_stand`, `minecraft:villager` / `minecraft:merchant`, `minecraft:beacon`, `minecraft:anvil`, `minecraft:hopper`, `minecraft:dropper`, `EntityHorse` — derive the exact string list from the source's `NetHandlerPlayServer`/`Container` registration sites and pin it in the test) plus `Unknown`; `WindowKind::from_type(&str)`.
  - `MerchantOffers { offers: Vec<MerchantOffer> }`, `MerchantOffer { first: Option<MetadataItem>, second: Option<MetadataItem>, output: Option<MetadataItem>, uses: i32, max_uses: i32 }` — parsed from a `MC|TrList` custom-payload body: window id, then a count (capped: `MAX_MERCHANT_OFFERS = 128`, pinned), then per offer two slot payloads, the has-second bool, the output slot, uses and max-uses ints (`MerchantRecipeList.readFromBuf` / `MerchantRecipe.readFromBuf`).
- Consumes: `read_slot`, `MetadataItem` (Task 1); the crate's cursor/string checks; the §2.1 packet ids.

- [ ] **Step 1: Fixture tests (RED).** One hand-built byte vector per struct with catching values: a chest window (kind + title + 90 slots in the 0x30 fixture), a horse window (the entity id present), a Set Slot at window −1 with a tailed item, a confirm rejected, a sign update with four non-empty lines, a property pair, an XP triple at the boundaries, both effect packets, and an offer list with 2 offers (one with a second item). Assert every field. Run: `cargo test -p oxide-proto-v47 window` — red.
- [ ] **Step 2: Implement the decoders.** Run — green.
- [ ] **Step 3: Hostile set (RED→green).** Slot count past its declared value, a declared 0x30 count that disagrees with the body, a truncation per struct class, a title past the string cap, a sign line past its cap, an offer count past the cap and a truncated offer tail. Run — green.
- [ ] **Step 4: Kind table.** Every source type string maps to its variant and back; an unlisted string returns `Unknown`; a spot table of literal pairs transcribed from the source registration sites (so a shifted table fails loudly). Run — green.
- [ ] **Step 5: The reference correction.** Edit `docs/research/protocol-47-reference.md`: the 0x2F row's note reads — the cursor rides window id −1 (`NetHandlerPlayClient.handleSetSlot`:1133, the `== -1` branch writes `inventory.setItemStack`); a negative slot never occurs. Run: `git diff --stat docs/research/protocol-47-reference.md` shows the one line. Commit it separately:

```
docs: correct the Set Slot cursor note (window id -1)
```

- [ ] **Step 6: Gate and commit.** Run the full gate. Commit:

```
feat: decode the window, sign and experience packets (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`; `cargo fmt --all --check`; `cargo clippy -p oxide-proto-v47 --all-targets -- -D warnings`.

---

### Task 3: The window and sign writers

**Goal:** The protocol crate can build every serverbound action the containers, the hotbar, the sign editor and the creative screen send, each byte-shaped exactly as the source's own packet.

**Files:**
- Modify: `crates/oxide-proto-v47/src/serverbound.rs` — the seven writers.
- Test: `crates/oxide-proto-v47/src/serverbound.rs` — inline unit tests.
- Test: `crates/oxide-proto-v47/tests/window_writers.rs` — the round-trip corpus.

**Interfaces:**
- Produces (all `oxide_proto_v47::serverbound`, the existing `impl Write`/`io::Result` shape):
  - `write_held_item_change(out, slot: i16)` — 0x09, a short.
  - `write_click_window(out, window_id: i8, slot: i16, button: i8, action: i16, item: Option<&MetadataItem>, mode: i8)` — 0x0E, the echo slot through `write_slot` (raw tail included — the byte-exact duty).
  - `write_close_window(out, window_id: u8)` — 0x0D.
  - `write_confirm_transaction(out, window_id: i8, action: i16, accepted: bool)` — 0x0F.
  - `write_creative_inventory_action(out, slot: i16, item: Option<&MetadataItem>)` — 0x10.
  - `write_enchant_item(out, window_id: u8, index: i8)` — 0x11.
  - `write_update_sign(out, x: i32, y: i32, z: i32, lines: &[String; 4])` — 0x12, lines through the crate's string writer (length rules as the codec's).
- Consumes: `write_slot` (Task 1); the crate's string writer and the existing id+payload shape.

- [ ] **Step 1: Round-trip corpus (RED).** For each writer, hand-built expected bytes (`write_*` output equals the literal vector, tail included for the two slot-carrying writers) and an echo pair: decode a slot fixture with an NBT tail through Task 1, write it through `write_click_window`, and compare against the original bytes. Run: `cargo test -p oxide-proto-v47 window_writers` — red.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: Cross-checks.** Internal round trips: `window.rs`'s decoders where the shape is bidirectionally defined (0x0F decoded == built; 0x12's lines survive), and the ≥100-char sign line clamps or refuses exactly as the wire rule (derive from the source's own write path — pin whatever the codec's string writer already enforces, don't invent). Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: write the window, sign and creative actions (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`; `cargo fmt --all --check`; `cargo clippy -p oxide-proto-v47 --all-targets -- -D warnings`.
---

### Task 4: The inventory model

**Goal:** `oxide-world` owns the player inventory: 36 main slots (hotbar first), 4 armour slots, the cursor stack and the selected hotbar slot, with the window-slot mapping the containers need and the source's own accessors.

**Files:**
- Create: `crates/oxide-world/src/inventory.rs` — the model, the mapping helpers, the accessors.
- Modify: `crates/oxide-world/src/lib.rs` — module declaration and re-exports.
- Test: `crates/oxide-world/src/inventory.rs` — inline unit tests.
- Test: `crates/oxide-world/tests/inventory.rs` — the mapping and accessor suite.

**Interfaces:**
- Produces:
  - `oxide_world::inventory::Inventory { pub main: [Option<MetadataItem>; 36], pub armor: [Option<MetadataItem>; 4], pub cursor: Option<MetadataItem>, pub selected: i16 }` — the source's own layout: `main[0..9]` is the hotbar, `main[9..36]` the 27 main slots (`InventoryPlayer.mainInventory:24` with the hotbar comment at :21-23, `armorInventory:27`, `currentItem:30`, `getItemStack` cursor :33).
  - `Inventory::get_current_item() -> &Option<MetadataItem>` (`InventoryPlayer.getCurrentItem`:50); `set_selected(slot: i16)` and `change_current_item(direction: i32) -> bool` — the wrap rule derived from `InventoryPlayer.changeCurrentItem`:165 and pinned (the magnitude clamps to one step — ±N moves exactly one slot, `:167-169`; the `-> bool` is the port's addition; the wrap derives in-task and is test-pinned).
  - The window mapping (one owner, both directions, pinned by test): window slots 9–35 ↔ `main[9..36]` (same order); window slots 36–44 ↔ `main[0..8]`; window slots 5–8 ↔ `armor[3..0]` (descending — window 5 is the helmet (`ContainerPlayer.java`:36-54 registers `getSizeInventory() − 1 − k`; `InventoryPlayer.getStackInSlot`:639-647 shifts by 36)). `Inventory::set_window_slot(window_id_0_slot: i16, item: Option<MetadataItem>) -> bool` returns whether the index mapped (5–44; the crafting slots 0–4 are not inventory state and return false — the session owns them beside the model). `Inventory::window_slot(window_index: usize) -> Option<&Option<MetadataItem>>` for the projection the snapshot path uses. Derive the exact directions from `ContainerPlayer`'s slot registration order (`inventory/ContainerPlayer.java` — the loop order fixes which window index is which: result, 4 craft, 4 armour, 27, 9).
  - `Inventory::clear_for_respawn() -> Option<MetadataItem>` — drops the cursor stack and returns it (the caller records it), keeps the main/armor (the server re-sends window 0 on respawn; this is the client's immediate-clear path — derive what the source does at the Respawned edge and pin it; if the source keeps the cursor through respawn, keep and record the finding instead).
- Consumes: `oxide_proto_v47::entity::MetadataItem` (the allowed edge).

- [ ] **Step 1: Mapping tests (RED).** Every window index 0–44 maps as pinned; an index outside returns `false`/`None`; a set at 44 lands in `main[8]`; a set at 9 lands in `main[9]`; the projection round-trips for a fully populated inventory. Run: `cargo test -p oxide-world inventory` — fails to compile.
- [ ] **Step 2: Implement the model and the mapping.** Run — green.
- [ ] **Step 3: Accessor suite (RED→green).** `get_current_item` at each selected 0–8; `change_current_item` wrap literals in the source's own argument convention ((0, +1) → 8; (8, −1) → 0 — the wire direction moves one slot per event); `clear_for_respawn`'s rule per the derived source behaviour. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the player inventory model (world)
```

**Verification:** `cargo test -p oxide-world`; `cargo fmt --all --check`; `cargo clippy -p oxide-world --all-targets -- -D warnings`.

---

### Task 5: The session's window state and the inventory events

**Goal:** The session tracks every window the server opens, keeps window 0 from Join Game, answers the confirm loop, routes the own player's air metadata, and publishes the inventory events the screens and the HUD read.

**Files:**
- Create: `crates/oxide-game/src/windows.rs` — the `Windows` state and its apply functions.
- Modify: `crates/oxide-game/src/session.rs` — the packet dispatch arms, the event emission, the own-id metadata routing, the effects store.
- Modify: `crates/oxide-game/src/lib.rs` — module declaration.
- Test: `crates/oxide-game/src/windows.rs` — inline unit tests.
- Test: `crates/oxide-game/tests/session_replay.rs` — the scripted-replay cases.

**Interfaces:**
- Produces:
  - `oxide_game::windows::Windows` — internal to the session's crate surface but named here because Tasks 6 and 22–24 touch it: `open: Option<OpenWindowState { window_id: u8, kind: WindowKind, title: String, slots: Vec<Option<MetadataItem>>, properties: Vec<i16>, entity_id: Option<i32> }>`, `player: Window0 { crafting: [Option<MetadataItem>; 5], inventory: Inventory }`, `cursor: Option<MetadataItem>`, `action_number: i16`. Apply: `apply_set_slot(window_id: i8, slot: i16, item: Option<MetadataItem>)` (window −1 → cursor; window 0 → mapped slots / crafting; the open window → its vector; an unknown window is ignored with a counter, not fatal), `apply_window_items`, `apply_property`, `apply_open`, `apply_close`, `next_action_number()`.
  - `oxide_game`'s new `ClientEvent` variants (names bind to Tasks 14–24): `WindowOpened { window_id: u8, kind: WindowKind, title: String }`, `WindowClosed { window_id: u8 }`, `WindowSnapshot { window_id: u8, slots: Vec<Option<MetadataItem>>, cursor: Option<MetadataItem>, properties: Vec<i16>, hotbar_pop: [u8; 9] }` — window 0's snapshot projects `crafting + inventory` into the 45-slot layout; opened windows project their own vector. `hotbar_pop` is the source's `animationsToGo` (set to 5 when a 0x2F lands on hotbar slots 36–44 — `NetHandlerPlayClient`:1158 — and decremented once per tick in the session's tick at the source's own decrement site, derived in-task; the HUD's pop math is `GuiIngame`:1043). `HeldItemSlot { slot: i16 }` (0x09; the session also sets `inventory.selected`), `Experience { bar: f32, level: i32, total: i32 }` (0x1F), `Air { air: i16 }` (own metadata index 1 — `Entity.java`:287's air field; the own-id metadata path extends the existing own-status branch), `Effects { effects: Vec<StatusEffect> }` with `StatusEffect { effect_id: u8, amplifier: u8, duration: i32 }` (own player only; 0x1D/0x1E), `SignEditorOpen { x: i32, y: i32, z: i32 }` (0x36), `SignTextChanged { x: i32, y: i32, z: i32, lines: [String; 4] }` (0x33; also writes the session's sign-text map the world view reads — `SignText` type in `oxide-game`).
  - The confirm rule: on `ConfirmTransaction { window_id, action, accepted }` with `accepted == false` for the live open window, the session re-sends `write_confirm_transaction(window_id, action, true)` in the same pass (the source's own loop — `NetHandlerPlayClient.handleConfirmTransaction`:1174-1196; derive the exact window guard in-task).
  - The merchant path: 0x3F whose channel is `MC|TrList` parses through Task 2's `MerchantOffers` into `ClientEvent::MerchantOffers { offers: Vec<MerchantOffer> }` (new variant; consumed by Task 19).
- Consumes: Tasks 1–4; the existing dispatch skeleton (the M4 `handle_*` pattern), the own-id status branch, the sign map seam, `Inventory`.

- [ ] **Step 1: State tests (RED).** In `windows.rs`: open/close idempotence; a set-slot into window 0's every region (crafting, armour, main, hotbar) projects into the 45-vector correctly; cursor via window −1; a set-slot for a closed/unknown window is ignored without panic; the action number wraps at `i16::MAX`; properties resize per window. Run: `cargo test -p oxide-game windows` — red.
- [ ] **Step 2: Implement `Windows`.** Run — green.
- [ ] **Step 3: Replay cases (RED).** In `session_replay.rs`, byte-level scripts: a join with a window-0 `Window Items` and a later `Set Slot` (both events + the projected snapshot asserted); an open chest (opened + snapshot), a property write, a close; a rejected confirm re-sends accepted=true (the sent bytes asserted); a `Set Experience` and an own-entity metadata air change (events in order); an effect add/remove pair; `MC|TrList` with two offers (event fields); a sign update + sign-editor open. Run — red.
- [ ] **Step 4: Implement the wiring.** Run — green; the keepalive burst test stays green (the snapshot builds are per-change, not per-frame — assert via the existing no-per-packet-work test shape).
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the session's window state and the inventory events (game)
```

**Verification:** `cargo test -p oxide-game`; `cargo test -p oxide-game --test session_replay`; `cargo fmt --all --check`; `cargo clippy -p oxide-game --all-targets -- -D warnings`.

---

### Task 6: The click machine and the action surface

**Goal:** The source's click semantics are ported once: the mode/button table, the drag machine's arithmetic, the cursor bookkeeping, the creative screen's local container, and the session actions that put them on the wire.

**Files:**
- Create: `crates/oxide-game/src/container.rs` — the click arithmetic, the drag types, the local container for the creative screen.
- Modify: `crates/oxide-game/src/input.rs` — the new `InputEvent` variants.
- Modify: `crates/oxide-game/src/session.rs` — the action dispatch and the sends.
- Test: `crates/oxide-game/src/container.rs` — inline unit tests.
- Test: `crates/oxide-game/tests/session_replay.rs` — the action wire cases.

**Interfaces:**
- Produces (all `oxide_game::container`):
  - `ClickMode` constants or the raw `i8` modes as pinned: 0 pickup/single, 1 shift quick-move, 2 number-key swap, 3 creative pick (middle), 4 drop, 5 drag, 6 double-click gather (`Container.slotClick`:140's own mode branches; the sender-side table derives from `GuiContainer`'s mouse handlers — Task 16 calls the derivation).
  - `compute_stack_size(slot: Option<&MetadataItem>, others: &[Option<MetadataItem>]) -> i32` — `Container.computeStackSize`:738 (the drag remnant's basis; the getMaxStackSize caps folded in per the source); `is_valid_drag_mode(mode: i32) -> bool` (:705); `get_drag_event(button: i32) -> i32` (:695); `can_add_item_to_slot(slot: &Option<MetadataItem>, stack: &MetadataItem, max: i32) -> bool` (:722).
  - `DragState { pub mode: i32, pub button: i32, pub slots: Vec<i16>, pub remnant: Option<MetadataItem> }` — the view's drag machine holds it; the arithmetic is here.
  - `LocalContainer` — the creative screen's own container: a slot vector with the source's `slotClick` sub-behaviour for the modes the creative area sends (modes 0–6 over a plain vector, `mergeItemStack` semantics for the double-click gather per `Container.mergeItemStack`:597; keep it minimal — only what `GuiContainerCreative` reaches).
  - `max_stack_size(item: &MetadataItem, table: &ItemTable) -> i32` — the per-item cap read from Task 8's registry (the source reads `Item.getMaxStackSize`); the container arithmetic consults it.
- `InputEvent` additions (the view → session seam; names bind to Tasks 16–24): `ClickWindow { window_id: i32, slot: i16, button: i8, mode: i8 }`; `CloseWindow { window_id: u8 }`; `CreativeAction { slot: i16, item: Option<MetadataItem> }`; `EnchantItem { window_id: u8, index: i8 }`; `UpdateSign { x: i32, y: i32, z: i32, lines: [String; 4] }`; `HeldItemChange { slot: i16 }`; `DropItem { whole: bool }`.
  - The session's sends: `ClickWindow` → `write_click_window` with the session's own action number and the source's computed carrier as the echo — the echo is `Container.slotClick`'s return in the source's own branches: mode 0 the snapshot's clicked-slot stack (`Container.java`:291-296), mode 1 the local shift result (`:266-271`, computed without mutating), modes 2–6, the −999 branch and the drag: null; it is never the cursor (`PlayerControllerMP.windowClick`:534-540 builds the packet from that return, and the server compares exactly this field — `NetHandlerPlayServer.processClickWindow`:1029); the counter and the echo live with the state that owns them — the session — and both are pinned by the wire case below; `CloseWindow` → `write_close_window` + local clear; `CreativeAction` → `write_creative_inventory_action`; `EnchantItem` → `write_enchant_item`; `UpdateSign` → `write_update_sign`; `HeldItemChange` → set `inventory.selected`, send `write_held_item_change`, emit `HeldItemSlot`; `DropItem` → `write_player_digging` with action 3 (whole) / 4 (single) (`C07PacketPlayerDigging`'s enum orders `DROP_ALL_ITEMS` = 3, `DROP_ITEM` = 4; `EntityPlayerSP.dropOneItem`:279-284 picks it; `NetHandlerPlayServer`:501-515 handles it — pin the pair).
- Consumes: Tasks 1–5; the existing digging writer; the input drain.

- [ ] **Step 1: Arithmetic tests (RED).** Mode table: each `get_drag_event`/`is_valid_drag_mode` literal from the source; `compute_stack_size` cases (empty slot, partial stacks, full stacks, cap arithmetic); `can_add_item_to_slot` truth table; the `LocalContainer`'s gather cases. Run: `cargo test -p oxide-game container` — red.
- [ ] **Step 2: Implement the arithmetic.** Run — green.
- [ ] **Step 3: Wire cases (RED).** In `session_replay.rs`: a `ClickWindow` event sends exactly one 0x0E with the expected action number, fields and echo (including a tailed item in the clicked slot — the echo is the source's computed carrier, never the cursor); two clicks increment the counter; `CloseWindow` sends 0x0D once and a second is a no-op; `HeldItemChange` sets and sends; `DropItem` sends the pinned digging action. Run — red.
- [ ] **Step 4: Implement the dispatch.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the container click machine and its actions (game)
```

**Verification:** `cargo test -p oxide-game`; `cargo test -p oxide-game --test session_replay`; `cargo fmt --all --check`; `cargo clippy -p oxide-game --all-targets -- -D warnings`.
---

### Task 7: The item model set and the display transforms

**Goal:** The assets layer bakes every item's model: the generated shape from its layer textures (the M4 path, now driven by the registry), the block-item fallback, the folded chest trio's small model, and the display transforms for every camera type each path uses.

**Files:**
- Modify: `crates/oxide-assets/src/model.rs` — the item-model resolution, the chest trio's model, the display-transform completion.
- Modify: `crates/oxide-assets/src/resources.rs` — the item-texture naming path the resolution reads.
- Test: `crates/oxide-assets/src/model.rs` — inline unit tests.
- Test: `crates/oxide-assets/tests/item_models.rs` — the fixture corpus.
- Test: `crates/oxide-assets/tests/store_items.rs` — the ignored real-store pass (the crate's standing shape).

**Interfaces:**
- Produces:
  - `ItemModelSource` resolved per registry entry: `Block(String)` (the block model), `Generated(Vec<String>)` (the `layer0…4` texture list — the M4 `builtin/generated` span path, `ItemModelGenerator`:15's `LAYERS`, :17's builder, the span collection walk `func_178393_a`:169-193), `Builtin(BuiltinItem)` with `BuiltinItem { Chest, TrappedChest, EnderChest }` (the folded trio — their small models derive from `ModelChest`'s boxes and scale: derive the box list in-task and pin it), `Missing`.
  - The chest trio's model: a box composition matching `ModelChest` (lid, base, knob — derive the exact boxes and their origins from `client/model/ModelChest.java` and pin each box in a fixture test). What the source does at draw time for these (the TESR binds the chest sheet); the icon draw uses `entity/chest/<variant>.png` sheets.
  - `Display { third_person, first_person, head, gui, ground, fixed, none }` — complete on the baked item as `[Option<Transform>; 7]` or a struct; the defaults per absent transform come from `ItemCameraTransforms`' own constants (derive every field — translation, rotation order XYZ, scale, and each type's default set — from `ItemCameraTransforms.java` and pin the constants; the apply order translate → rotate (y, then x, then z) → scale derives from `applyTransform` (`ItemCameraTransforms.java`:61-65) and is pinned by a composition test in Task 11).
  - The resolution errors degrade per the standing rule: an unresolvable model yields `Missing` with a counter, never a load failure.
- Consumes: nothing outside the crate (the resolution fn takes the model-name list, so the dependency direction is Task 8 → Task 7 — recorded in Produces).

- [ ] **Step 1: Resolution tests (RED).** A generated item (three-layer) bakes spans from synthetic layer names; a single-layer item; a block item resolves to its block model; a chest trio id resolves to the chest model; an unknown name resolves `Missing` with a counter. Run: `cargo test -p oxide-assets item_models` — red.
- [ ] **Step 2: Implement the resolution.** Run — green.
- [ ] **Step 3: Display tests (RED→green).** Every `ItemCameraTransforms` default field pinned by literal (the source's own numbers); a model JSON's explicit `display.gui` overrides the default for that type only; the chest model's boxes pinned. Run — green.
- [ ] **Step 4: Store pass.** The ignored test bakes a real item set from the store (a bounded list: a sword, a tool, a block item, a chest, a potion) and asserts non-empty geometry + resolved textures. Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-assets --test store_items -- --ignored`.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: bake the item model set with display transforms (assets)
```

**Verification:** `cargo test -p oxide-assets`; the ignored store pass above; `cargo fmt --all --check`; `cargo clippy -p oxide-assets --all-targets -- -D warnings`.

---

### Task 8: The item registry table

**Goal:** Every id the source registers resolves to its display name, its model resolution, its stack rules and its tooltip numbers — the full roster, count-checked against the source.

**Files:**
- Modify: `crates/oxide-client/src/items.rs` — the table growth (split at the data boundary if the file grows past review size; the split's second file stays in `oxide-client`).
- Test: `crates/oxide-client/src/items.rs` — inline pin tests.
- Test: `crates/oxide-client/tests/item_table.rs` — the completeness and pin suite.

**Interfaces:**
- Produces:
  - `ItemEntry { id: i16, name: &'static str, resolution: ItemModel (Task 7 — `ItemModel::source()` carries the resolved `ItemModelSource`), max_stack: u8, max_damage: i16, attributes: ItemAttributes, variants: bool }`; `ItemAttributes { attack_damage: Option<f32>, attack_speed: Option<f32>, armour_points: Option<f32>, armour_toughness: Option<f32> }` — the tooltip path reads these (Task 17); the values derive per class from the source's constructors (`ItemSword`/`ItemTool`/`ItemArmor` modifier blocks — derive and pin per material class: wood/stone/iron/gold/diamond literals).
  - The table itself: every `Item.registerItems` registration (`Item.java`:511; the pairs region :765-952) — one row each, with the source's own id. `variants: true` marks entries Task 9 expands into damage sub-items (wool, potions, spawn eggs, dyes and the class list the source's `getSubItems` overrides define).
  - Lookups: `item_entry(id: i16) -> Option<&'static ItemEntry>`; `max_stack_size` for Task 6's arithmetic reads the table (the base cap 64; the per-id overrides the source sets: tools 1, potions 1, etc.).
  - Names are the source's en_US display strings (the M4 boss-name precedent — the language file itself is never committed; the table carries the strings the tooltips, the popup and the creative search read).
- Consumes: Task 7's `ItemModelSource`; nothing else outside the crate.

- [ ] **Step 1: Completeness test (RED).** Every id the source's registration block lists has a row; the row count equals the derived count (the task counts the source's registrations and records the number); an id outside the block returns `None`; no row's id duplicates (a synthetic scan). Run: `cargo test -p oxide-client item_table` — red.
- [ ] **Step 2: The table.** Derive every row; the pin set: at least twenty literal `(id, name)` pairs across the classes (a sword, a pickaxe, an armour piece, a food, a block item, a potion, a bucket, a redstone component), so a shifted table fails loudly. Run — green.
- [ ] **Step 3: Attribute pins (RED→green).** Per material class: the sword damage literals, a pickaxe's, an armour piece's points and toughness, in both the item's own fields and the composed tooltip inputs; a non-tool item at `None`. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the item registry table
```

**Verification:** `cargo test -p oxide-client`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.

---

### Task 9: Sub-items, names and the creative lists

**Goal:** The damage sub-items (wool colours, potions, spawn eggs, dyes), the potion display names, and every creative tab's ordered item list match the source's own registries and creation order.

**Files:**
- Modify: `crates/oxide-client/src/items.rs` (or the Task 8 split's data file) — the sub-item table and the tab lists.
- Test: `crates/oxide-client/tests/item_table.rs` — the sub-item and tab suites.
- Test: `crates/oxide-client/src/items.rs` — inline pins.

**Interfaces:**
- Produces:
  - `SubItem { damage: i16, name: &'static str, tab: CreativeTab }` per variant entry — derived per class from the source's `getSubItems` overrides: `ItemCloth` (16 wool colours, names per dye), `ItemPotion` (the potion meta set: derive the exact damage list the source region populates — regular + splash, the 1.8 damage encoding pinned by literal), `ItemSpawnEgg` (every spawnable mob from the M4 roster), dyes, and the block-variant classes' 16-damage sets (wool blocks, carpet, stained glass? derive the list the source populates). The colour names derive from the dye colour table (`EnumDyeColor`'s names) — pin each.
  - Potion display names: "Potion of X" / "Splash Potion of X" composition from the source's `ItemPotion.getItemStackDisplayName` (the potion registry's names — derive the id→name table from `Potion.java`'s registrations and pin a spot set).
  - `CreativeTab` — one variant per 1.8 tab in `CreativeTabs.creativeTabArray` order (`creativetab/CreativeTabs.java`:15; the array's own order; indices per `getTabIndex`:123), the search tab and the inventory tab (`tabInventory`:97), each with its icon item (the source's `getTabIconItem` — derive and pin per tab) and its sheet name.
  - `creative_tab_items(tab: CreativeTab) -> &'static [&'static ItemStack-ish entry]` — the ordered list: the source's own creation order (the registration order in `Item.registerItems` filtered by tab, then each entry's sub-items in the class's populate order — derive the ordering rule in-task and pin it against two tabs' literal first-ten).
- Consumes: Task 8; the M4 spawn roster for the egg list.

- [ ] **Step 1: Sub-item suite (RED).** Wool's 16 damage/name pairs pinned; the potion damage set's boundaries pinned; spawn eggs count against the M4 roster's spawnable set; a damaged entry's name composition ("Orange Wool") pinned. Run — red.
- [ ] **Step 2: Implement the sub-items.** Run — green.
- [ ] **Step 3: Tab suite (RED→green).** Every tab's index, icon and sheet pinned; the building-blocks tab's first ten entries pinned in order; the search tab's filter rule (case-insensitive substring over display names, the source's own comparison at `GuiContainerCreative`:270 region) pinned with a fixture. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the item sub-items, names and creative lists
```

**Verification:** `cargo test -p oxide-client`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.

---

### Task 10: The GUI sheets, the item sprites and the icon sampler

**Goal:** The store's GUI sheets load (widgets, the container family, the enchanting book and the SGA glyph sheet), the registry's item sprites stitch into the atlas, and the HUD pass draws icons through a no-mipmap no-blur sampler.

**Files:**
- Modify: `crates/oxide-assets/src/atlas.rs` — the item-sprite stitch input (the registry's sprite list beside the block set).
- Modify: `crates/oxide-assets/src/resources.rs` — the sheet name list.
- Modify: `crates/oxide-render/src/hud.rs` — the icon sampler's atlas binding (`HudTexture::AtlasIcon` or the pass's second atlas binding — name binds to Task 11/14) and the `set_texture` registrations the client drives.
- Modify: `crates/oxide-client/src/main.rs` / `view.rs` — the sheet load calls (the M4 `set_texture` path).
- Test: `crates/oxide-assets/src/atlas.rs` — inline tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the sampler-discrimination GPU case.

**Interfaces:**
- Produces:
  - The stitched atlas carries item sprites beside blocks (one atlas — survey §1.4); `Atlas`'s sprite list input grows; the ignored store test extends to an item sprite.
  - Registered sheet names (the store keys, extensionless, through `store_key`): `gui/widgets`, `gui/icons` (existing), the container family — `gui/container/generic_54`, `gui/container/dispenser`, `gui/container/hopper`, `gui/container/furnace`, `gui/container/brewing_stand`, `gui/container/crafting_table`, `gui/container/enchanting_table`, `gui/container/anvil`, `gui/container/beacon`, `gui/container/villager` (the merchant sheet), `gui/container/horse` — each verified against the survey §3.1 sheet list and the source's own `ResourceLocation`s in-task (a wrong key fails the store test, not silently), plus `gui/book`, `gui/enchanting_table_book`, `font/ascii_sga` (the SGA glyph sheet) and the chest sheets for the trio (`entity/chest/chest`, `entity/chest/trapped_double`? no — the trio's icon sheets: `entity/chest/normal`, `entity/chest/trapped`, `entity/chest/ender` — derive the three names in-task).
  - `HudTexture::AtlasIcon` — the atlas bound with the no-mipmap, no-blur sampler (`RenderItem.renderItemIntoGUI`'s `setBlurMipmap(false, false)` before its draws — `RenderItem.java`:197/:318/:357 per survey §1.4); every icon draw (Tasks 11–14) uses it; the standing `Atlas` binding carries the atlas's own mipped pair (`Minecraft.java`:548-554 — the state the item draws toggle away from; the terrain's block-draw path is untouched).
  - The deferred atlas doc wording (the final review's row 39) lands in the atlas module's doc comment at this touch.
- Consumes: Tasks 7–8's sprite list; the existing stitch and store paths.

- [ ] **Step 1: Sheet fixtures (RED).** A synthetic sheet beside the existing fixtures registers and samples; the item sprite stitches and the atlas test's sprite count grows by the item set's size. Run: `cargo test -p oxide-assets` — red.
- [ ] **Step 2: Implement the stitch and the registrations.** Run — green; the store test loads a real item sprite and one real container sheet.
- [ ] **Step 3: The sampler case (RED).** In `pipeline_headless.rs`: an icon-sized quad drawn through `AtlasIcon` at a scale that would blend under a mipped sampler; assert its edge texels stay crisp (the discrimination: the same quad through `Atlas` blurs — record both readings; the minified-sprite mip class from the rig notes is the failure signature this case guards). Run: `cargo test -p oxide-render --test pipeline_headless atlasicon -- --ignored` — red.
- [ ] **Step 4: Implement the binding.** Run — green; the full ignored render suite stays green (the pass-level state change re-runs it whole).
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: stitch the item sprites and register the gui sheets
```

**Verification:** `cargo test -p oxide-assets`; `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-assets -- --ignored`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-assets -p oxide-render --all-targets -- -D warnings`.
---

### Task 11: The GUI item draws

**Goal:** The HUD pass draws item icons: generated sprites through the crisp sampler, block items and the chest trio as GUI-space 3D under their `display.gui` transform, enchanted items with their glint — interleaved with the 2D draws in list order.

**Files:**
- Modify: `crates/oxide-render/src/hud.rs` — `HudDraw::Item`, the item pipeline's integration, the batch flush/resume.
- Create: `crates/oxide-render/src/gui_item.rs` — the GUI item draw (the transform application and the draw construction).
- Modify: `crates/oxide-render/src/lib.rs` — module declaration.
- Test: `crates/oxide-render/src/gui_item.rs` — inline unit tests (the transform math).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the icon GPU cases.

**Interfaces:**
- Produces:
  - `HudDraw::Item { stack: Option<MetadataItem>, x: f32, y: f32 }` — the icon occupies the source's 16×16 GUI pixels at `(x, y)`; `None` draws nothing; the source's own zLevel ladder maps to list order (no depth fights between icons; recorded).
  - `GuiItemDraw` — given the baked model + the display transform, builds the vertices: the transform order translate → rotate (Y then X then Z) → scale, then the GUI projection (`RenderItem.renderItemIntoGUI`'s setup — derive the matrix from `RenderItem.java`:353-397 and the display application from `ItemCameraTransforms.applyTransform`; pin the composed matrix for the default GUI transform of a block item by literal).
  - The HUD pass integration: item draws flush the 2D batch, switch pipelines within the same render pass, and resume (list order preserved); the pass gains a depth attachment for the item pipeline only (the 2D pipelines keep depth off — the existing pass-level state).
  - Glint: an enchanted item (the source's `isItemEnchanted` rule — derive: stack NBT `ench` list non-empty; Task 8's entries can mark it) draws its model again with the glint texture and the source's blend, after the item, before the next list draw (`RenderItem.renderEffect`:170; the glint texture and UV scroll rate — derive and pin).
  - The item source reachable from the client: the HUD pass is handed a resolver (the Task 7 bake + Task 8 table); unresolvable stacks draw the missing sprite (the atlas's own).
- Consumes: Tasks 7–10; the entity pass's item mesh shapes.

- [ ] **Step 1: Transform tests (RED).** The default GUI transform for a block item composes to the pinned matrix (literal from the source's own constants); a model with an explicit `display.gui` differs exactly as its JSON says; a generated item (all transforms absent) uses the `ItemCameraTransforms` default for the type. Run: `cargo test -p oxide-render gui_item` — red.
- [ ] **Step 2: Implement the draw.** Run — green.
- [ ] **Step 3: GPU cases (RED).** In `pipeline_headless.rs`: the HUD list `[slot rect, item (block), count text, item (generated)]` — assert (a) the block item's icon pixels in its rect (distinctive face colours) and its silhouette bounds, (b) the generated item's edges crisp under the icon sampler (the Task 10 discrimination re-used at the real draw), (c) the count text over the icon (a pixel inside the text's strokes), (d) list order — the second item overwrites where they would overlap. Run: `cargo test -p oxide-render --test pipeline_headless item -- --ignored` — red.
- [ ] **Step 4: Implement the integration.** Run — green; the whole ignored render suite re-runs (pass-level state change).
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw the item icons in the hud pass
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-render --all-targets -- -D warnings`.

---

### Task 12: The first-person held item

**Goal:** The selected item draws in the player's hand: the source's `ItemRenderer` shape — the equip ease, the swing translation, the per-class `display.firstperson` transform — between the scene draws and the dim quad.

**Files:**
- Create: `crates/oxide-render/src/held_item.rs` — the pass (camera-space draw construction).
- Modify: `crates/oxide-render/src/renderer.rs` — the pass insertion (after the scene draws, before the dim quad — the frame list's own order).
- Modify: `crates/oxide-client/src/view.rs` — the frame's held-item state (the selected stack, the equip pair, the swing pair).
- Test: `crates/oxide-render/src/held_item.rs` — inline unit tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the held-item cases.

**Interfaces:**
- Produces:
  - `HeldItemFrame { stack: Option<MetadataItem>, equip: f32, equip_prev: f32, swing: f32, swing_prev: f32 }` — the view fills it each frame from the session's selected slot and the own entity's animation state (the swing pair and its advance derive from `EntityLivingBase`'s counters — `swingProgressInt`:67, the pair :85-86, advance :1408-1421; the own player's swing is client-local, set on attack/use — derive the own-swing setter path in-task and pin it).
  - The draw: `itemToRender` semantics (`ItemRenderer.java`:37); the swap lands when the ease nearly closes — `equippedProgress < 0.1F` swaps `itemToRender` (`updateEquippedItem`:609-613) — and the ease steps by the clamped delta `f2 = clamp(f1 − equippedProgress, −0.4, 0.4)` per tick (`:604-607`), rendered interpolated; the swing transforms in the source's order: `doItemUsedTransformations` (`:261-267`: translate(−0.4·sin(√s·π), 0.2·sin(√s·2π), −0.2·sin(s·π))) applied before `transformFirstPersonItem` (`:296-307`: translate(0.56, −0.52, −0.72); translate(0, equip·−0.6, 0); rotate 45° Y; then with `f = sin(s²π)`, `f1 = sin(√s·π)`: rotate(f·−20° Y), rotate(f1·−20° Z), rotate(f1·−80° X); scale 0.4) — all inside `renderItemInFirstPerson` (`:355-416`); pin each by test. The survey §5.3's own swing reading (−f1·70° Y, f·70° Z, −f·70° X) disagrees with the source: the source governs and the survey note is corrected at this touch.
  - The per-class transforms: `display.firstperson` when present; the class branches the source takes with no state here (map eating, bow… ) are swept and each absence recorded (the sweep is the deliverable — `renderItemInFirstPerson`'s branch list walked in-task).
  - Draw rules: lightmap full-bright equivalent per the pass's lighting (the source enables the lightmap before the hand — :865; the M4 entity-lighting convention), no fog, skipped while the player sleeps (the source's `:861-863` conditions minus the two modes that do not exist — recorded).
- Consumes: Tasks 7, 10; the frame's world view; the session's selected slot.

- [ ] **Step 1: Math tests (RED).** The swing curve at 0, mid-swing and 1; the ease at its start, midpoint and settle; the transform composition for a sword (first-person transform from its model JSON) by literal. Run: `cargo test -p oxide-render held_item` — red.
- [ ] **Step 2: Implement the pass.** Run — green.
- [ ] **Step 3: GPU cases (RED).** At a fixed pose with a sword selected: the blade's pixels in the frame's lower-right (silhouette bounds + a colour pin), an orientation-sensitive assertion (the blade's edge side — the count-level lesson: orientation needs side-sensitive pins), the equip ease at its midpoint draws the item (the swap rule), and the sleep state draws nothing. Run: `cargo test -p oxide-render --test pipeline_headless helditem -- --ignored` — red.
- [ ] **Step 4: Implement the wiring.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw the first-person held item
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo test -p oxide-client`; `cargo fmt --all --check`; `cargo clippy -p oxide-render -p oxide-client --all-targets -- -D warnings`.

---

### Task 13: Held items and armour on entities

**Goal:** Other entities wear what they carry: the third-person held item and the four armour slots draw over the M4 pose machinery, with the leather dye tint and the enchant glint; the owner-approved extras (the creeper and wither auras, the deadmau5 ears) ride along.

**Files:**
- Modify: `crates/oxide-render/src/entity_models/layers.rs` — the held-item layer and the armour layers.
- Modify: `crates/oxide-render/src/entity_pass.rs` — the draws (if the pass needs the equipment in its input) and the glint.
- Modify: `crates/oxide-game/src/entity_view.rs` — the frame's equipment field (the store's `equipment` is already `[Option<MetadataItem>; 5]`, `oxide-world/src/entity.rs`:232-233; the frame extracts it — no oxide-world change is owed).
- Modify: `crates/oxide-client/src/view.rs` — the equipment's conversion into the draw (the `MetadataItem` → the render-side view, per the item-icon seam's rule).
- Test: `crates/oxide-render/src/entity_models/layers.rs` — inline unit tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the equipment cases.

**Interfaces:**
- Produces:
  - `draw_held_item(model, entity, stack, swing)` — the `LayerHeldItem` shape (:27, :66): the item mounts at the arm part with the entity's own pose applied — the swing, the held-item pose and the sneak sway (no equip term exists on this path: `equippedProgress`/`prevEquippedProgress` are first-person-only, `ItemRenderer.java`:42-43) (derive the layer's arm-end math and the THIRD_PERSON transform application; pin the transform for a sword by literal). Applies to the biped roster whose renderers carry the layer (derive it from each `addLayer(new LayerHeldItem(...))` site and the `RenderBiped` constructor split — the 3-arg constructor adds the layer, the 4-arg does not; villagers have no held-item layer, the witch's is `LayerHeldItemWitch`; the sites include the player, the giant zombie, the pig zombie, the skeleton and the armor stand — and pin the list).
  - `draw_armour(model, entity, slot, stack)` — the `LayerArmorBase` shape (:45): the per-slot model pick (helmet/chest/boots on the base model, leggings on the leg model — derive the model pick per slot (`LayerArmorBase.isSlotForLeggings`:91-99) and the construction inflation pair (`LayerBipedArmor`:13-17 — 0.5 leggings / 1.0 armour; the port's `render_scale` is the per-kind pre-render scale, a different concept)), the tier sheet (layer 1 vs layer 2 per slot — the leggings flag at the bind, `LayerArmorBase`:54-56, the sheet name from `getArmorResource`:136-153), the leather dye tint (the NBT `display.color` read through Task 1's reader — derive `getColor`:135-157 (`hasColor`:127-130)), and the glint when enchanted.
  - The folded extras (owner Q2, recommended yes): `draw_charge_aura` for creeper/wither (the model redrawn with the aura sheet and the source's blend — `LayerCreeperCharge`, `LayerWitherAura`; pin the sheets and the blend) and the deadmau5 ears (`LayerDeadmau5Head`'s boxes when the entity name matches — pin the name rule and the boxes).
  - The roster rules: which M4 kinds wear armour (the biped family), which hold items, and what an unknown equipment id draws (the missing sprite, recorded).
  - The draw-schema touchpoints the final review carries (section 2.4 bullet 2): the object-family id-jitter wiring (rec 35) and the item-phase pins land at this task's touch of the entity draw path; anything unreachable re-carries with the trigger named.
- Consumes: Task 1's NBT read; Tasks 7, 10; the M4 pose/frame machinery.

- [ ] **Step 1: Layer tests (RED).** The held-item transform literal; the armour per-slot model pick and scales; the dye tint composition (the colour value through the tint rule); the aura sheet and blend constants; the ear-box literals. Run: `cargo test -p oxide-render layers` — red.
- [ ] **Step 2: Implement the layers.** Run — green.
- [ ] **Step 3: GPU cases (RED).** A zombie holding a sword at a fixed pose (blade pixels + orientation side pin); a leather-armoured biped with a red dye (pixels shift to the tint — pre/post discrimination recorded); iron armour tier sheet (a pixel of the sheet's grey); an enchanted chestplate's glint (a blended-overlay pixel); the creeper charge overlay when answered yes. Run: `cargo test -p oxide-render --test pipeline_headless equipment -- --ignored` — red.
- [ ] **Step 4: Implement the pass wiring.** Run — green; the whole ignored render suite re-runs.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw held items and armour on entities
```

**Verification:** `cargo test -p oxide-render`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-render -p oxide-world --all-targets -- -D warnings`.
---

### Task 14: The hotbar, the held-item popup and the crosshair

**Goal:** The bottom row of the screen reads the live inventory: the 182×22 bar with its sliced background, the selection highlight, every slot's icon with its count pop, the durability bars, the two-second selected-item popup, and the crosshair (owner Q1, recommended yes).

**Files:**
- Modify: `crates/oxide-client/src/view.rs` — the hotbar assembly, the popup's own state, the crosshair entry, the HUD-visibility rule.
- Modify: `crates/oxide-render/src/hud.rs` — the crosshair's inverted draw kind.
- Test: `crates/oxide-client/src/view.rs` — inline unit tests (the popup counters, the pop math, the visibility rule).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the hotbar and crosshair cases.

**Interfaces:**
- Produces:
  - The hotbar draws (source values derive in-task from `GuiIngame.java`:375-412 and the survey's §3.2 rows; pin every literal): the background slice `(0, 0, 182, 22)` at `(scaledW/2 − 91, scaledH − 22)` from `gui/widgets`; the highlight `(0, 22, 24, 22)` at `(scaledW/2 − 91 − 1 + selected × 20, scaledH − 23)`; per slot `k` (0–8) the item at `(x_k, y − 3)` plus the source's count/durability overlay (`RenderItem.renderItemOverlayIntoGUI`:455 — the count digit placement and the 13×2 durability bar with its colour ramp, `RenderItem.java`:455-505, derive + pin); the pop: the slot's item scaled about its centre by the `animationsToGo` fraction (`GuiIngame`:1043 — the `f` returned by `renderHotbarItem`:1037's h = 1 + f/5? derive the exact curve and pin).
  - The popup's state: `remaining_highlight_ticks` and the `highlighting_item_stack` (view-side; the swap detection per `updateTick`:1068-1110's comparison — item identity plus NBT tags equal, plus metadata equal for non-damageable stacks only (a damageable stack's metadata is not compared — `GuiIngame.java`:1097's `isItemStackDamageable() ||` short-circuit); a change resets to 40; the count draws only while > 0; the source's `k = ticks × 256 / 10` alpha clamp; y = `scaledH − 59` — pin the literals). `HudDraw::Text` with the alpha-carrying colour is the existing path.
  - The crosshair: `HudDraw::InvertRect { x, y, w, h }` drawing the crosshair as the source draws it: a 16×16 `gui/icons` quad at `(scaledW/2 − 7, scaledH/2 − 7)` under the `(775, 769, 1, 0)` blend, gate `showCrosshair` (`GuiIngame.java`:175-180, `:513`; the checklist row 10's "two 1-px quads" wording is superseded by the source and corrected in the acceptance; pin a discrimination case: the inverting draw over a known backdrop flips its pixels. The pass's pipeline selection follows the draw's own source path (the `hud-render-parity` reference's blend-state facts).
  - The HUD-visibility rule: the source's gate is the call site's `!hideGUI || currentScreen != null` (`EntityRenderer.java`:1166-1169) — with a screen open the overlay still draws; `renderGameOverlay` itself carries no screen condition (the chat is screen-aware only for its scroll/opacity, `GuiNewChat.java`:305-307). Pin the gate as the rule for our list — no per-entry screen suppression exists in the source.
- Consumes: Task 5's `WindowSnapshot` (hotbar slots 36–44 + `hotbar_pop` + selected via `HeldItemSlot`); Tasks 8, 11, 17's tooltip seam is not here (no HUD tooltips).

- [ ] **Step 1: View tests (RED).** The popup reset on a stack swap (same id, different metadata resets for non-damageable stacks — a damageable stack's damage change does not; identical does not); the 40-tick countdown; the alpha curve at ticks 1/10/40; the pop fraction at 5/2/0 ticks with the partial; the visibility rule table (no screen → all; container screen → the pinned subset). Run: `cargo test -p oxide-client hotbar` — red.
- [ ] **Step 2: Implement the assembly.** Run — green.
- [ ] **Step 3: GPU cases (RED).** A hotbar frame: background slice edges, the highlight at slot 4's x-offset (a pixel of the shifted highlight), an item icon in slot 0 (the Task 11 path), a count of 16 (digits' pixels), a damaged tool's durability bar (the ramp colour at the pinned fraction), the popup's text (position + alpha at a pinned tick), and the crosshair's inversion (a pixel flipped against its backdrop). Run: `cargo test -p oxide-render --test pipeline_headless hotbar -- --ignored` — red.
- [ ] **Step 4: Implement the draw kinds and the wiring.** Run — green; the whole ignored render suite re-runs.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw the hotbar, the held-item popup and the crosshair
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client -p oxide-render --all-targets -- -D warnings`.

---

### Task 15: The rows — health, hunger, armour, air, experience

**Goal:** The stat rows draw with the source's exact segments, blink and variant rules: hearts (normal, half, empty, poison, wither, absorption), food with its jitter, the armour row, the air bubbles, the experience bar and level.

**Files:**
- Modify: `crates/oxide-client/src/view.rs` — the row assembly and the blink state.
- Test: `crates/oxide-client/src/view.rs` — inline unit tests (the segment rules, the counters).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the row cases.

**Interfaces:**
- Produces (source values derive in-task from `GuiIngame.renderPlayerStats`:609-900, `renderExpBar`:414 and the survey §3.2 rows; every literal pinned):
  - The hearts row: `x = scaledW/2 − 91` (`GuiIngame.java`:643), the y and the stack above it (armour 10 px higher, air above hunger) deriving at `:643-700` and pinning to the source's own rows (hearts/hunger at `scaledH − 39`, `:645`; armour/air at `scaledH − 49`, `:650` — the checklist's `h − 22 − 21` chain is corrected to these); ten hearts; the background container/empty halves and the six variants (normal, poison, wither, absorption overlay, blink, half) all `gui/icons` slices; the health-from-S06 value, ceil'd (`MathHelper.ceiling_float_int`); absorption per the derived wire source (metadata index or properties — derive in-task; if it needs a packet outside the set, record the limit and skip the yellow row); the blink state machine (`healthUpdateCounter`, `lastSystemTime`, the 20/10 tick raises and the 3-tick flip) verbatim, view-side; the regeneration half-heart offset (`isPotionActive(regeneration)` from Task 5's `Effects`; the `updateCounter % ceil(maxHealth + 5)` rule (:657 — the max-health attribute; absorption not included)); the poison/wither colour variants from the same effects list.
  - The food row: hunger jitter when `foodLevel == 0`? derive the source's condition (the saturation-based jitter with the shared seed `rand.setSeed(updateCounter × 312871)` set once at :637 and consumed in the food loop :776-823, condition :788-791; pin seed/literal cases); the half-food rule; the hunger-effect variant (from effects).
  - The armour row: above the hearts (`GuiIngame.java`:660-685 — the armour section; `getTotalArmorValue` at :652. The air row is :885-889, a different row); full/half per point pair; the value from the equipped armour slots' `armour_points` sums (Task 8's table + window-0 slots 5–8; cap at 20 — the source's own computation, derived and pinned).
  - The air row: bubbles right-to-left from the air value (`getAir` :877 region; the 300/10 segmentation and the max-air split rule — derive and pin); gated by submersion (`isInsideOfMaterial(Material.water)`:875) — ten full bubbles draw at air 300 (`k7 = 10`, `i8 = 0`:878-879); no blink rule exists.
  - The experience bar: the two slices `(0, 64, 182, 5)` and `(0, 69, 182, 5)` at the source's y; the fill fraction from the `Experience` event; the level number centred above it (digits via the shared text path; the source's colour/side rules pinned).
  - The damage flash: closed as a source-falsified premise (2026-10-09, pre-dispatch): the tree has no screen flash — the hurt visuals are the camera roll (`EntityRenderer.hurtCameraEffect`:585-609, already drawn: the client's `hurt_roll`, M3), the per-entity tint (`RendererLivingEntity`:328-334, M4) and the hearts blink (this task's hearts row); the survey row is corrected.
- Consumes: Task 5's `Health`/`Experience`/`Air`/`Effects` events; Tasks 8, 10; the shared text path.

- [ ] **Step 1: Rule tests (RED).** The heart variant picker (health 0/1/2/19/20 with and without poison, wither, absorption, blink); the food jitter's seed literals; the air segmentation at 300/299/1/0; the bar fill at 0/0.5/1 and the level digits. Run: `cargo test -p oxide-client rows` — red.
- [ ] **Step 2: Implement the rows.** Run — green.
- [ ] **Step 3: GPU cases (RED).** One frame per family: hearts at 17.5 health (one blink pair and one half), poisoned hearts (the green overlay pixels), food at 6 with jitter offset 0 vs 1 (two cases), armour at 13 with mixed pieces, air at 7 bubbles with the fading pair, xp at level 12 / 0.42 (the level text pixels). Run: `cargo test -p oxide-render --test pipeline_headless rows -- --ignored` — red.
- [ ] **Step 4: Implement the draws.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw the health, food, armour, air and experience rows
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client -p oxide-render --all-targets -- -D warnings`.
---

### Task 16: The screen framework and the container screen base

**Goal:** Screens exist: a current-screen state with the source's input ownership and close semantics, and a container screen base with slot hit-testing, hover highlight, the full click derivation into the Task 6 machine, the drag machine's view side, the cursor stack, and the shared player-section layout every container window ends with.

**Files:**
- Create: `crates/oxide-client/src/screens/mod.rs` — the screen states, the routing, the open/close rules.
- Create: `crates/oxide-client/src/screens/container.rs` — the container screen base (`SlotPos` tables seam, hit test, click derivation, drag state, cursor draw, title, player-section layout).
- Modify: `crates/oxide-client/src/main.rs` — the screen input routing, the pointer rules, the frame seeding.
- Modify: `crates/oxide-client/src/view.rs` — the screen draws form their own group ABOVE the whole HUD/overlay group, per the source's order (the HUD overlay first, :1166-1170, then `currentScreen.drawScreen` :1185-1191 after a depth clear :1187 — the port's hud_draws sit inside the overlay group, so the screen group goes after them; the HUD gate per Task 14's rule).
- Test: `crates/oxide-client/src/screens/container.rs` — inline unit tests.
- Test: `crates/oxide-client/tests/screens.rs` — the routing and mapping suite.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the base's GPU cases.

**Interfaces:**
- Produces:
  - `ScreenState` — one variant per open screen: `Container { window_id: u8, kind: WindowKind, title: String, layout: &'static ContainerLayout }`, `Inventory`, `Sign { x, y, z }`, `Book { stack: MetadataItem }`, `Creative`, plus the `None` state (the variants are declared here; the opens for Inventory/Sign/Book/Creative land in Tasks 20–23 — T16 wires the Container kind's path only, and an open for a declared-but-unimplemented variant draws the generic frame and records). `Screens { current: Option<ScreenState> }` with `open`, `close` (sends `InputEvent::CloseWindow` with the current window id on every close — window 0 included: `GuiContainer.keyTyped`:692-696 closes on Escape AND the inventory key for every container screen, and `EntityPlayerSP.closeScreen`:330-333 sends C0D unconditionally; `EntityPlayer.closeScreen`:538-541 only resets the container, no send), and the Escape rule (`GuiScreen.keyTyped`'s own — derive what Escape does with the chat overlay open in front (the chat has no `parentScreen` in this tree — closing it returns to the game, not to a container beneath)). The input ownership is `allowUserInput` (`GuiScreen`:68 — gates `Minecraft`:1834; true for the inventory/creative screens: `GuiInventory`:28, `GuiContainerCreative`:64; containers inherit false).
  - `ContainerLayout { x_size: i32, y_size: i32, sheet: &'static str, slots: &'static [SlotPos], title: TitleKind }` with `SlotPos { index: i16, x: i32, y: i32 }` — the per-kind tables land in Tasks 18–20; the base consumes them. `TitleKind` picks the source's title source per screen (the window title text vs the fixed "Inventory"/"Crafting" labels — derive per container).
  - `ContainerScreen` (runtime): hovered slot (the source's hit rule — `getSlotAtPosition`:341-354, the first match in slot order; note the highlight loop sets `theSlot` to the LAST draw-order match (:126-138) while `getSlotAtPosition` returns the FIRST — pin a discriminating case; `isMouseOverSlot`:657-659's test is the ±1-padded 18×18 region via `isPointInRegion`:666-672, not the bare 16×16), the hover highlight (the source's own — the hovered overlay at :126-138, the semi-transparent white rect at :134), the drag state (Task 6's `DragState`), and the click derivation table: mouse button + shift → mode 1; the number keys 1–9 while hovering with the cursor EMPTY → mode 2 with the button = the key (`checkHotbarKeys`:718-731's gate); the drop key (Q) over a slot → mode 4 (Ctrl+Q → button 1); the outside rect → slot −999 with the throw/click rule (derive :359-460's exact branches); the middle button → mode 3 (the pick-block binding test — `keyBindPickBlock.getKeyCode() + 100`, :362/:410/:448, cursor empty — not a hard-coded middle click); the double-click timing → mode 6 (the source's double-click window derives — `GuiContainer.mouseClicked`:359-365's timing fields (the 250 ms window at :365)); the drag: pressed → status 0 records the slot; moved with the button held → status 1 per new slot with the remnant preview computed by Task 6's arithmetic (mutating the view's cursor copy — the source's own client-side preview, :466-510); released → the three mode-5 phases go out together (:615-625 — `(null, −999, …|0|…, 5)`, then one per dragged slot, then `(null, −999, …|2|…, 5)`; nothing is sent on press or move — 1+n+1 packets). Every send is one `InputEvent::ClickWindow { window_id, slot, button, mode }`.
  - The player-section rule: every container window's last 36 slots are the player's own; the base lays them out at the derived positions (the standard three-part block: 27 slots + 9 hotbar with the source's y gaps — derive from `ContainerChest`/`ContainerPlayer` slot coordinates and pin), and number keys route to the hotbar swap semantics inside screens (the source's `keyTyped` number path — derive).
  - The cursor draw: the carried stack draws at the pointer (`drawScreen`'s cursor draw; the source's −8/−8 offset (:149, :169 — `k2` is 16 while dragging)) after the slots, before the tooltip; the drag preview draws per the source's own preview drawing; the count overlay covers the yellow-zero case row 15 names (the source's `renderItemOverlayIntoGUI` branch — derive at `RenderItem.java`:455-505 and pin); the close rule: closing with a cursor DROPS the stack (`Container.onContainerClosed`:516-525's `dropPlayerItemWithRandomChoice`; no close-return animation exists — the `returningStack` easing at :172-187 is the touchscreen drag-return only, set at :583-608).
  - The pointer rules: while a screen is open the pointer frees and the deltas drive the cursor position (the M4 chat screen's own rule, `:256-257` scaling — reuse that machinery; pin the shared path).
- Consumes: Task 5's `WindowOpened`/`WindowSnapshot`; Task 6's `ClickWindow`/`CloseWindow`/`DragState`/arithmetic; Task 11's item draws; Tasks 8–10.

- [ ] **Step 1: Routing tests (RED).** Open/close transitions (a server close while its screen is open clears it; Escape on the inventory sends one `CloseWindow { window_id: 0 }` (every close sends C0D); Escape on a chest sends its own); a `WindowOpened` for an unknown kind opens the generic frame (recorded); the hover highlight's slot selection order. Run: `cargo test -p oxide-client screens` — red.
- [ ] **Step 2: Implement the framework and the base.** Run — green.
- [ ] **Step 3: Mapping suite (RED→green).** The click derivation table: every branch (left/right, shift, 1–9, Q/Ctrl+Q, outside, middle, double, drag start/step/end) produces the pinned `(slot, button, mode)` — a truth table transcribed from the source's handlers. Run — green.
- [ ] **Step 4: GPU cases (RED).** A container frame from a real layout fixture: sheet pixels, a slot frame, an item in slot 0, the cursor stack at (100, 50), a drag preview at the remnant count, the hover highlight. Run: `cargo test -p oxide-render --test pipeline_headless screen -- --ignored` — red.
- [ ] **Step 5: Implement the draws.** Run — green.
- [ ] **Step 6: Gate and commit.** Run the full gate. Commit:

```
feat: the screen framework and the container screen base
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.

---

### Task 17: Item tooltips

**Goal:** Hovering a stack shows the source's tooltip: the name with its rarity colour, the enchant lines, the lore, the attribute lines, the F3+H advanced appendix, drawn with the source's own background, border and placement.

**Files:**
- Create: `crates/oxide-client/src/tooltip.rs` — the line builder and the draw assembly.
- Create: `crates/oxide-client/src/enchants.rs` — the enchantment name table and the roman numerals.
- Modify: `crates/oxide-client/src/screens/container.rs` — the hover seam (slot under the pointer → lines).
- Modify: `crates/oxide-client/src/main.rs` — the F3 chord state (the H toggle while F3 is held; the M4 F3 handling is the seam) and the frame's advanced flag.
- Test: `crates/oxide-client/src/tooltip.rs` — inline unit tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the tooltip case.

**Interfaces:**
- Produces:
  - `tooltip_lines(stack: &MetadataItem) -> Vec<TooltipLine>` — `TooltipLine { text: String, colour: [f32; 4] }` (legacy `§` codes allowed; the builder composes from Task 8's entry: the display name (custom name from NBT, italic; else the table name), rarity colour (`ItemStack.getRarity`:864's EnumRarity table — pin the four colours), the enchant lines from the NBT `ench` list (name + roman level; names from the enchantment table — derive the 1.8 enchantment list and the roman numerals i–v per `Enchantment.getTranslatedName`), the lore list (purple italic), the attribute block (`When in main hand:`-shaped — derive `getTooltip`'s exact composition from `ItemStack.java`:644 and the modifier sources; pin the prefix lines), and the repair-cost? (derive whether 1.8 shows it — pin the finding).
  - `advanced_tooltip_lines(stack: &MetadataItem) -> Vec<TooltipLine>` — the F3+H appendix (`Item.getTooltip`'s advanced block — id/meta/NBT lines in dark grey; derive and pin).
  - The draw: the source's own box — pin the row-14 literals (background `0x10001000`, border gradient `0xF0100010`, 3-px padding, 10-px line spacing, the four rarity colours white/yellow/aqua `0x55FFFF`/light purple) and re-derive any the source contradicts (`GuiScreen.drawHoveringText`:189), the 3-pixel padding, the line pitch, the width = the longest line + padding, the placement right-and-below the cursor flipped at the screen edges (derive the clamp), and the shadowed title? (derive whether the name uses the shadow path). The tooltip draws last in the screen's list (over the cursor).
  - The F3+H chord: while the debug key (F3) is held, H toggles the advanced flag (derive the source's chord handling in `Minecraft`'s key loop and pin the toggle; the frame reads the flag for every tooltip).
- Consumes: Task 1's NBT read; Task 8's table; Task 16's hover seam; the shared text path (widths via the existing metrics).

- [ ] **Step 1: Builder tests (RED).** A plain item (name only); a custom-named item (italic + name); a rare item (the rarity colour); an enchanted sword (`ench` NBT fixture: two lines, the roman levels); a lore item; a diamond sword's attribute lines; the advanced appendix with a damaged metas item. Run: `cargo test -p oxide-client tooltip` — red.
- [ ] **Step 2: Implement the builder and the table.** Run — green.
- [ ] **Step 3: GPU case (RED).** A tooltip over a slot fixture: the border pixels at the pinned colours, the fill, the name row's text pixels, an enchant line's colour, the flip at the right edge (a second case at a near-edge position). Run: `cargo test -p oxide-render --test pipeline_headless tooltip -- --ignored` — red.
- [ ] **Step 4: Implement the draw and the chord.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw item tooltips
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.
---

### Task 18: The container family A — chest, hopper, dispenser, furnace, brewing, crafting

**Goal:** The first container family draws its screens: every sheet, every slot table, the title rule, and the live property draws (the furnace's flame and arrow, the brewing stand's bubbles and fuel).

**Files:**
- Modify: `crates/oxide-client/src/screens/container.rs` — the family's `ContainerLayout` tables and their property draws (or a `screens/family_a.rs` split at the layout boundary).
- Test: `crates/oxide-client/src/screens/container.rs` — inline table tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — one case per screen.

**Interfaces:**
- Produces:
  - The layouts (every coordinate derives from the source container's `addSlotToContainer` calls and the GUI's slices; pin each table literally): `CHEST_90` (6 rows, `gui/container/generic_54`, 176×222 — the title from the window's own title text), the 3-row chest (176×168? derive the source's two-size rule — the same sheet's upper slice), `HOPPER` (5 slots, `gui/container/hopper`), `DISPENSER` (3×3, `gui/container/dispenser`, shared by dropper), `FURNACE` (3 slots, `gui/container/furnace`), `BREWING` (5 + ingredient, `gui/container/brewing_stand`), `CRAFTING` (3×3 + result, `gui/container/crafting_table`). Each table: absolute slot positions (the base translates by `gui_left/top`), x_size/y_size, sheet, title rule.
  - The property draws: furnace — property 0's flame height (`value × 13 / 200`-shaped) and property 1's arrow width (`value × 24 / 200`-shaped) with the sheet slices (derive the exact scales and slices from `GuiFurnace.drawGuiContainerBackgroundLayer` and pin); brewing — property 0's bubble fill and property 1's fuel segments (derive `GuiBrewingStand`'s rules and pin); the rule that a missing property draws nothing sane (zero, recorded).
  - The title rule: container windows draw the server's own title text (the `WindowOpened` title); the fixed-label screens in this family draw the source's own label where the GUI owns it (derive per screen; pin).
- Consumes: Task 16's base; Task 5's snapshots and properties; Tasks 8, 11.

- [ ] **Step 1: Table tests (RED).** Every layout's slot count matches its `WindowKind`'s window slot count; pinned coordinates for at least three slots per layout; the chest's row-count pick by window size. Run: `cargo test -p oxide-client screens` — red.
- [ ] **Step 2: Implement the tables.** Run — green.
- [ ] **Step 3: Property tests (RED→green).** The flame at 0/100/200 (heights); the arrow at the halves; the bubbles at 0/14/28 and the fuel at 0/18; the missing-property fallbacks. Run — green.
- [ ] **Step 4: GPU cases (RED).** One frame per screen at a fixture state with an item in each functional slot (furnace mid-burn: the flame and arrow pixels at the pinned sizes; brewing mid-brew). Run: `cargo test -p oxide-render --test pipeline_headless familya -- --ignored` — red.
- [ ] **Step 5: Implement the draws.** Run — green.
- [ ] **Step 6: Gate and commit.** Run the full gate. Commit:

```
feat: the chest, hopper, dispenser, furnace, brewing and crafting screens
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.

---

### Task 19: The container family B — beacon, enchanting, villager, horse, anvil

**Goal:** The interactive family: the beacon's effect picker and confirm, the enchanting table with its offer costs, glyph book and the C11 send, the villager's offer list and trade clicks, the horse's inventory, and the anvil with its cost and rename field.

**Files:**
- Create: `crates/oxide-client/src/screens/family_b.rs` — the five screens' layouts and interactions.
- Modify: `crates/oxide-client/src/screens/mod.rs` — the kind dispatch.
- Modify: `crates/oxide-client/src/main.rs` — the text-field seam for the anvil's name field (reusing the M4 chat field engine's subset).
- Test: `crates/oxide-client/src/screens/family_b.rs` — inline tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — one case per screen.

**Interfaces:**
- Produces (all coordinates, costs and rules derive in-task from the source's GUI + container pairs; every literal pinned):
  - Beacon: layout (`gui/container/beacon`), the payment slot, the effect rows and the confirm/cancel widgets (the source's own button shapes — derive; the minimal `GuiButton` lands here or in Task 16's base per the source's widget draw: `GuiButton.drawButton`:78's sprites), the property reads (0 = levels, 1/2 = the chosen effects), the row-click rule (local selection) and the confirm's send (derive `GuiBeacon`'s done path — its click shape pinned).
  - Enchanting: layout (`gui/container/enchanting_table`), the two slots, the three offer buttons — cost text from the properties, the unaffordable colour from the `Experience` level, the click sends `InputEvent::EnchantItem { window_id, index }`; the background: the book (`gui/enchanting_table_book` — the flip/turn animation counters and the two-page draws, derive `GuiEnchantment`'s own fields) and the glyph clouds when the owner answered Q1 yes (the SGA glyph sheet `font/ascii_sga`, the three cloud lists, the per-tick random walk — derive and pin; a recorded static fallback when no).
  - Villager: layout (`gui/container/villager`), the seven visible offer rows from `MerchantOffers` (item-arrow-item, the price label, the uses text), the row click → `ClickWindow` at the derived slot index, the scroll buttons paging the list, the player section; the out-of-stock red X (checklist row 20) and the used-out redraw states the source pins.
  - Horse: layout (`gui/container/horse`), the saddle/armour slots and the chest slots by the window's own slot count, the horse's own inventory section; the entity id from `WindowOpened` for the title? (derive whether the source titles from the entity); the riding HUD (jump bar, horse hearts) is out — recorded (riding's HUD is a later pass).
  - Anvil: layout (`gui/container/anvil`), the two inputs + output, the cost text from the property (red past the level — pin), and the name field: the M4 chat field engine's subset (text, cursor, editing keys) as `NameField`; its max length and the flows derive from `GuiRepair` (a non-empty field drives the server's rename via the click on the output — no extra packet; pinned). The field is a deliverable of this task — a material gap its execution surfaces routes as a scoped finding, never a silent drop.
  - One commit per task (the fenced message); the screen order (beacon + anvil → enchanting → villager + horse) is the step order, each step gated before the next.
- Consumes: Task 16's base; Task 5's `MerchantOffers`/snapshots/properties; Tasks 8–10, 15 (the level), 17 (tooltips).

- [ ] **Step 1: Per-screen tests (RED).** The beacon's property readout and confirm send; the enchanting offers' cost/colour table (affordable/unaffordable at levels); the villager row → slot mapping (a fixture offer list); the horse's slot count variants; the anvil's cost colour and the field's max length. Run: `cargo test -p oxide-client family_b` — red.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: GPU cases (RED).** One frame per screen with a fixture state: the beacon's rows + confirm (button sprite pixels), the enchanting book + one offer (the cost text pixels + an unaffordable red), the villager's two rows (the arrow and price pixels), the horse's layout, the anvil's cost + field (the cursor seam). Run: `cargo test -p oxide-render --test pipeline_headless familyb -- --ignored` — red.
- [ ] **Step 4: Implement the draws.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the beacon, enchanting, villager, horse and anvil screens
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.
---

### Task 20: The inventory screen

**Goal:** The player's own screen: the 2×2 grid, the armour slots, the rotating player preview, the crafted-result slot, and — when the owner answered Q1 yes — the active-effect list beside it.

**Files:**
- Create: `crates/oxide-client/src/screens/inventory.rs` — the screen.
- Modify: `crates/oxide-client/src/screens/mod.rs` — the E-key open path and the kind dispatch (`Inventory` state).
- Test: `crates/oxide-client/src/screens/inventory.rs` — inline tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the screen case.

**Interfaces:**
- Produces:
  - The layout: `gui/container/inventory` sheet; the 2×2 grid + result, the four armour slots, the main/hotbar sections (Task 16's player-section rule; the source's own coordinates from `ContainerPlayer` + `GuiInventory.drawGuiContainerBackgroundLayer`; pin the table).
  - The player preview: `drawEntityOnScreen`-shaped (:96): the player model (the M4 player model machinery) at the derived position/scale, rotated by a drag on the preview region (the source's own drag state — `GuiInventory.mouseClickMove` region derives; pin the rotation step and the pitch clamp), with the pointer-relative facing rule. Live inventory changes redraw through the snapshot.
  - The effects overlay (Q1 yes): the right-side list from the `Effects` event — the icon sheet's effect sprites, the name and duration text ("Name" / "Name II" with amplifier; the mm:ss-style duration — derive `InventoryEffectRenderer`'s own format and pin), hidden entirely with no effects.
  - The open path: the E key sends `C16` client status 2 (`Minecraft.java`:2092-2102; the `OPEN_INVENTORY_ACHIEVEMENT` send at :2100) on every non-riding open — no guard exists, two opens send two C16s (pin two-opens-two; the riding branch is out of scope, recorded) — and opens the screen; window 0 renders through the same container path with `window_id` 0 (its clicks carry 0; its close sends C0D with window id 0 like every other close).
  - One commit per task (the fenced message); the layout/slots/preview and the (Q1-dependent) effects overlay are step order within it, each step gated.
- Consumes: Task 16's base; Task 5's snapshots/effects; the M4 player model; Task 15's effects data.

- [ ] **Step 1: Tests (RED).** The layout table pins; the preview's rotation step and clamps; the effect list's order (the source's own collection order — derive) and duration format literals; the one-C16-per-open rule (two opens send two). Run: `cargo test -p oxide-client inventory` — red.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: GPU case (RED).** The screen with a populated inventory: the sheet pixels, an item in the grid, the preview's silhouette region (a skin-coloured pixel at the pinned position), two effect rows (icons + text). Run: `cargo test -p oxide-render --test pipeline_headless inventory -- --ignored` — red.
- [ ] **Step 4: Implement the draws.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the inventory screen
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.

---

### Task 21: The creative inventory

**Goal:** The creative screen: the tab strip with its icons, the paged item grid, the scrollbar, the search field, the hotbar row, the delete slot, the pick behaviours, and the C10 wire.

**Files:**
- Create: `crates/oxide-client/src/screens/creative.rs` — the screen and its own container.
- Modify: `crates/oxide-client/src/screens/mod.rs` — the dispatch and the E-key path in creative mode.
- Modify: `crates/oxide-world/src/inventory.rs` — the selected-tab persistence seam if the source keeps one (derive: the tab lives in `GuiContainerCreative`'s own state; recorded).
- Test: `crates/oxide-client/src/screens/creative.rs` — inline tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the screen case.

**Interfaces:**
- Produces (all values derive from `GuiContainerCreative`, its inner `ContainerCreative`, and `CreativeTabs`; pin each):
  - The layout: the top tab strip (the sheet's two rows; the tab sprites per the source's own slices), the tab icons (`CreativeTab`'s icon item per Task 9), the item area's fixed 9×5 page, the scrollbar (the source's scroll bar drawing: the track, the thumb, the disabled state), the player hotbar row, the delete slot sprite (bottom-right).
  - The paging model: `scroll` offset over the tab's ordered list (Task 9), the wheel and the scrollbar drag (the source's own rules — derive the click-on-track jumps and the drag), the page spills (the source draws partial rows at the bottom — derive the clipping rule and pin).
  - The search: the field widget (the M4 chat engine's subset again, focused by click — the source's own focus/clear rules), the filter per Task 9's pinned rule over the search tab's list; the tab strip switches between search and the tabs (the source's own rules — derive).
  - The click behaviours: inside the item area — mode 0 places a full stack on the cursor (the source's `ContainerCreative` + `GuiContainerCreative` divide: the grid cells are the container's slots and clicks are C0E mode 0/… for creative; in the 1.8 code the creative grid IS the container's inventory and every click is a normal window click — the local prediction is the source's `inventoryContainer.slotClick` call at `GuiContainerCreative`:144 region for the player rows; derive which side predicts what and pin); the delete slot (clicking drops the cursor stack — derive: mode 0 on the delete slot clears); middle-click on any slot anywhere → mode 3 with the register (the client-side mode 3 semantics: pick block — the source's `PlayerControllerMP`/`Minecraft.clickMouse` path derives); the hotbar row inside the screen = the real inventory (window 0's slots — creative screen shows the player's hotbar AND inventory? The creative screen shows only the hotbar row (bottom) — the main 27 are not shown; derive and pin which window-0 slots map where).
  - The wire: any click on the player-side rows inside the creative screen sends `InputEvent::ClickWindow` (window 0); items taken from the grid are local predictions plus `InputEvent::CreativeAction` (C10) — derive the source's exact pairing (the source sends C10 for every creative-area click via `PlayerControllerMP.clickCreativeInventory`? in 1.8 creative mode `windowClick` sends C10 instead of C0E for window 0? derive the branch and pin the rule: which clicks produce which packet).
- Consumes: Tasks 9, 16, 17; the chat field engine; Task 5's snapshots for the hotbar row.

- [ ] **Step 1: Tests (RED).** The paging model (offset bounds, partial-row clip, page count per tab); the search filter integration; the delete slot's clear; the C10/C0E branch table transcribed from the source; the scrollbar geometry pins. Run: `cargo test -p oxide-client creative` — red.
- [ ] **Step 2: Implement the model.** Run — green.
- [ ] **Step 3: GPU case (RED).** The screen at a fixture state: the tab strip (a selected vs unselected tab pixel), a grid item, the scrollbar thumb, the search field with text, the delete slot sprite. Run: `cargo test -p oxide-render --test pipeline_headless creative -- --ignored` — red.
- [ ] **Step 4: Implement the draws.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the creative inventory screen
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.

---

### Task 22: Sign editing

**Goal:** The sign editor opens on the server's request, edits four lines with the source's own field behaviour, sends the update, and the edited sign shows its text in the world.

**Files:**
- Create: `crates/oxide-client/src/screens/sign.rs` — the editor.
- Modify: `crates/oxide-game/src/session.rs` — the sign-text map (from Task 5) consumed by the world view; the `UpdateSign` send path exists from Task 6.
- Create: `crates/oxide-render/src/sign_text.rs` (the in-world front-face text draw — built from the sign store + the M4 block draw); Modify: `crates/oxide-render/src/lib.rs` (the module registration).
- Test: `crates/oxide-client/src/screens/sign.rs` — inline tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the editor and in-world cases.

**Interfaces:**
- Produces (all derive from `GuiEditSign`, `TileEntitySign`, `TileEntitySignRenderer`; pin each):
  - The screen: opened by `SignEditorOpen` (0x36) for the position; lines from the session's sign map (the last `SignTextChanged` for that position; empty four lines otherwise — the source's own default rule); the source's four-line display with the editing line's cursor (the blink period from the editor's `updateCounter` — derive), the arrow keys/Enter line navigation, the max line length (the source's cap: the 90-pixel width test at `GuiEditSign.java`:111 — derive and pin; no character cap), the "Done" button (`GuiButton` sprite — `GuiEditSign`'s own button), and Escape's send-equals-Done rule (the M4-recorded source fact: `onGuiClosed` sends the update on every close — `GuiEditSign`:54-60; pin it).
  - The send: `InputEvent::UpdateSign { x, y, z, lines }` on close.
  - The in-world draw (Q3 yes): the sign's front-face text — four centred lines at the derived scale and offsets on the board face for floor and wall variants (the board's orientation from the block's metadata — the M4 block draw's own state), the line colour/darkening rules (the source's `TileEntitySignRenderer` — derive: text black, the selected line's highlight only in-editor, the slight light dim), built into the terrain/world draw path at the sign position (text draws as a small billboard-ish quad set over the block face — derive the source's matrix and pin the geometry). No text for positions outside the map's entries; the map clears an entry when its text is all-empty (the source's rule? derive).
- Consumes: Tasks 5, 6, 16; the M4 block machinery.

- [ ] **Step 1: Tests (RED).** The line cap; the cursor blink phase; Enter/arrow navigation; the close-sends rule on both Done and Escape; the map's empty-entry rule; the in-world geometry literals for a floor sign at rotation 0 (and one wall sign). Run: `cargo test -p oxide-client sign` — red.
- [ ] **Step 2: Implement the editor.** Run — green.
- [ ] **Step 3: GPU cases (RED).** The editor with text and a cursor (glyph + cursor pixels); an in-world floor sign's text quads at a fixed pose (text pixels on the board face; an orientation-sensitive side pin per the rig notes). Run: `cargo test -p oxide-render --test pipeline_headless sign -- --ignored` — red.
- [ ] **Step 4: Implement the in-world draw and the wiring.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: sign editing and the sign text draw
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-game`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client -p oxide-game --all-targets -- -D warnings`.

---

### Task 23: Book reading

**Goal:** The server's book open shows the reader: the page, the page-turn buttons, the author line; an editable book opens read-only, recorded.

**Files:**
- Create: `crates/oxide-client/src/screens/book.rs` — the reader.
- Modify: `crates/oxide-client/src/screens/mod.rs` — the `MC\|BOpen` dispatch (the session surfaces the custom payload as an event — add `ClientEvent::BookOpen { stack: MetadataItem }` if the M4 payload path does not already carry it; the M4 custom-payload channel list ends at `MC\|TrList`/`MC\|Brand`/`MC\|BOpen` handling sites — derive which of those M4 wired and extend at its seam).
- Test: `crates/oxide-client/src/screens/book.rs` — inline tests.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — the reader case.

**Interfaces:**
- Produces (derive from `GuiScreenBook`, its reading path, and the item's NBT; pin each):
  - The reader: the book background sheets (`gui/book`), the current page's wrapped text (the source's own wrap at the book's fixed width — derive `GuiScreenBook`'s text-width rule), the page indicator ("Page 1 of 3" — the sourced format), the author/title line? (derive which lines 1.8's reader shows beyond the page text), the Done and page-turn buttons (the source's own sprites from the book sheet), the page flip.
  - Pages from the NBT (`pages` string list through Task 1's reader — a missing or malformed list draws one empty page, the source's own default; the cap: a hostile page count is bounded by the reader's cap, recorded).
  - The editable book (Q4 yes): opens the same reader on its stored pages; no editing affordances; a `docs/DIVERGENCES.md` entry ("book-and-quill editing is post-v1; the open shows the stored pages read-only" — the file's own format, tooling-neutral).
  - The open path: the custom payload for the own player's held book (the source's handler opens for the held stack; derive its guard and pin).
- Consumes: Task 1's reader; Task 16's framework; the shared text path.

- [ ] **Step 1: Tests (RED).** The page count and index navigation (first/last bounds); the wrap rule literals at the book's width; a malformed `pages` tag's fallback; the page indicator's text. Run: `cargo test -p oxide-client book` — red.
- [ ] **Step 2: Implement the reader.** Run — green.
- [ ] **Step 3: GPU case (RED).** A three-page book at page 2: the sheet's pixels, the wrapped text pixels, the indicator text. Run: `cargo test -p oxide-render --test pipeline_headless book -- --ignored` — red.
- [ ] **Step 4: Implement the wiring and the DIVERGENCES entry.** Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the book reader
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-render --test pipeline_headless -- --ignored`; `cargo fmt --all --check`; `cargo clippy -p oxide-client --all-targets -- -D warnings`.
---

### Task 24: The inventory keys and the scripted gestures

**Goal:** The keys the new surfaces need route end to end — E opens the inventory, digits and the wheel drive the hotbar, Q drops, Escape layers correctly — and the scripted-input surface gains the container gestures the acceptance drives.

**Files:**
- Modify: `crates/oxide-client/src/keymap.rs` — the key names (E, Q, digits, the wheel step) and their bindings.
- Modify: `crates/oxide-client/src/main.rs` — the routing (screen precedence order, the C16 per-open rule, the wheel's two modes), the `ScriptDriver` directives.
- Modify: `crates/oxide-game/src/input.rs` — the new `InputEvent` variants' consumption plumbing and the `INPUTS_PER_PASS` doc pin (+ a test asserting the constant).
- Modify: `crates/oxide-game/src/session.rs` — the drop path (`DropItem` → the digging 3/4 writes; the source's own action exports).
- Test: `crates/oxide-client/tests/screens.rs` — the routing table.
- Test: `crates/oxide-client/src/main.rs` — the script parser tests.
- Test: `crates/oxide-game/src/session.rs` — the drop sends (session_replay).

**Interfaces:**
- Produces:
  - The routing table (pin it; it is the source's own key handling, derived per path): the inventory key opens the inventory screen only in the source's own states and sends one C16 per non-riding open (`Minecraft.java`:2092-2102 — no guard; pin two-opens-two); the wheel outside screens changes the held slot through `InputEvent::HeldItemChange` (one slot per wheel event — the source clamps the delta to ±1, `InventoryPlayer.java`:167-169; pinned) and inside a screen routes to the screen (the creative screen's own scroll; nothing elsewhere); the digits 1–9 are hotbar keys outside screens and swap keys inside them (Task 16 routes); Q / Ctrl+Q outside screens send `InputEvent::DropItem { whole: false/true }`; Escape closes the current screen, else the chat, else nothing; the F3-chord H handle stays Task 17's.
  - The `ScriptDriver` additions (syntax pinned here; the acceptance scripts use them): `move <x> <y>` (set the freed-pointer cursor), `click <x> <y> <button> [shift]`, `drag <x1> <y1> <x2> <y2> <button>` (the source's own intermediate-step semantics — one move-then-release like the manual path), `wheel <notches>`, `key <name>` (a named key once: `e`, `q`, `escape`, `1`…`9`, `f3`, `h`), alongside the existing directives. Each parses, validates and drives the same code path as real input (no privileged internals).
- Consumes: Tasks 5, 6, 16–21; the M4 pointer machinery.

- [ ] **Step 1: Routing tests (RED).** The table: E from no screen opens, one C16 per open (two opens send two); E behind a container screen closes it (the inventory-key branch, `GuiContainer.keyTyped`:692-696 — pin); wheel outside sends one slot per event (the ±1 clamp); digits outside set the slot; Q/Ctrl+Q sends the right action; Escape layering (screen → chat → nothing). Run: `cargo test -p oxide-client screens` — red.
- [ ] **Step 2: Implement the routing.** Run — green.
- [ ] **Step 3: Script tests (RED→green).** Each new directive parses; a `click` reaches the screen's own path (a routing fixture asserts the derived `ClickWindow`); a `drag` produces the three-step click sequence the release sends. Run: `cargo test -p oxide-client script` — green.
- [ ] **Step 4: Session drop tests (RED→green).** `DropItem` writes the pinned digging bytes; the existing suite re-runs. Run: `cargo test -p oxide-game` — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the inventory keys and the scripted gestures
```

**Verification:** `cargo test -p oxide-client`; `cargo test -p oxide-game`; `cargo fmt --all --check`; `cargo clippy -p oxide-client -p oxide-game --all-targets -- -D warnings`.

---

### Task 25: The carried citation set and the sweep-tooling round

**Goal:** The milestone's carried comment and citation items are corrected, and the extractor's range-inside grammar round lands, so the sweep covers what it could not.

**Files:**
- Modify: every file the open rows name (comment-only edits).
- Create: `refs/m5-task-25/` — the adapted tooling, the JSON sweeps, the coverage scan and the receipts (git-ignored; the working directory is the task's own).
- Test: none new — the sweep's own checks are the verification.

**Interfaces:**
- Produces:
  - The tooling round: adapt `extract_cites.py` and the scan from the T21 records (`refs/m4-task-21/` — its `extract_cites.py`, `diff_bindings.py`, `r2_scan.py`) into the task directory; extend the grammar for the range-inside cite form (the ~450-token plus ~88-token residue the T19 coverage scan recorded — T19's verdict §7.2 and rec 73b/83c); re-run: the residue enters scope and the flags it raises are the work.
  - The correction set: the open rows of `refs/m4-task-23/deferred-list.md` (D1–D9, D12, D27 per their current state, plus the F3 cluster set — rows 1, 26, 31.1, 31.2, 32, 33, 36(a), 36(c), 37, 38, 39) and of `refs/m2-final-review/deferred-minors.md` (the F3 list); the twelve-site `WorldClient.java` family and `clientbound.rs`:222's wording (the final review's own list); the `view.rs` stale-citation fixes owed; the `light.rs:123` citation drift; the M4 close's owed record corrections (sweep rows 6/34 marked landed — record-only; the item-5 trigger's routing, which is Task 26's). Every row's premise re-derives at execution (open/closed re-checked first — the lists predate two closes).
  - The receipts: pre/post JSONs, the flagged list, the re-run to zero flags on added lines, and the coverage report — the project's cite-sweep procedure (mirrored at `refs/m5-homework/cite-sweep-binding.md` before this task) throughout.
- Consumes: nothing at runtime; reads the records.

- [ ] **Step 1: Adapt the tooling and run it (RED).** The residue enters scope; every flag it raises is listed. Run: the adapted `extract_cites.py --scope <the M5-touched file set>` — flags red.
- [ ] **Step 2: Fix the flags and the recorded rows.** Comment/citation-only edits, each byte-quoted before. Run the sweep again — zero flags on added lines; the coverage report appended.
- [ ] **Step 3: Re-run on final bytes and record the receipts.** Run once more after any late edit in this task; JSONs + logs into `refs/m5-task-25/`.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
chore: sweep the carried citations and extend the extractor
```

**Verification:** the sweep's zero-flag run and the coverage report; the full gate; `git diff --stat` shows comment-only hunks.

---

### Task 26: The assets-test consolidation

**Goal:** The three copies of the PNG test helpers become one, and the 16-bit/tRNS strip path gets its fixture.

**Files:**
- Modify: `crates/oxide-assets/src/atlas.rs`, `crates/oxide-assets/src/texture.rs`, `crates/oxide-assets/src/resources.rs` — the helper consolidation (the three test-module copies the M4 close recorded).
- Create/Modify: the shared test-support module for `oxide-assets` (per the crate's own test-support pattern — derive where the sibling crates keep theirs and follow it).
- Test: the new synthetic fixture: a 16-bit PNG with a `tRNS` chunk (built programmatically — no binary committed) exercising the loader's strip path.

**Interfaces:**
- Produces: one helper set (encode/decode round-trips used by the three modules); the fixture test asserting the stripped path's bytes (the loader's own contract: the pixel kept per the M4 record; pin the expected bytes).
- Consumes: nothing.

- [ ] **Step 1: Consolidate (green-to-green refactor).** All three modules' tests pass before and after; the diff removes the copies. Run: `cargo test -p oxide-assets` — green both sides.
- [ ] **Step 2: The fixture (RED).** A synthetic 16-bit + tRNS PNG hits the strip path; the expected pixel bytes pin the contract; red first if the path mishandles it (record whatever it does — the M4 record says the second half was never verified; this task either confirms or files it).
- [ ] **Step 3: Settle the finding and gate.** If the path is wrong, the fix is scoped inside this task (loader fix + the test); the gate runs either way.
- [ ] **Step 4: Commit.** Commit:

```
test: consolidate the png test helpers and cover the trns strip
```

**Verification:** `cargo test -p oxide-assets`; the store-backed suite re-runs (`OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-assets -- --ignored`); `cargo fmt --all --check`; `cargo clippy -p oxide-assets --all-targets -- -D warnings`.
---

### Task 27: The acceptance run

**Goal:** The rig proves the milestone end to end — HUD states, every container's round-trip, the sign edit, the book, the creative screen — and the checklist rows take their evidence.

**Files:**
- Evidence: `refs/m5-acceptance/` (git-ignored; the run's frames, logs, scripts, proxy captures and the report).
- Modify: `docs/parity/checklist.md` — the rows' evidence lines, the row-10 wording correction, the §17 Gaps close.
- Test: no new repo tests — the run is the verification.

**Interfaces:**
- Produces (the run's shape — a multi-round run by nature; plan it as machinery + legs, then a FINISH pass, exactly as the M4 run did):
  - The scene set (pin it; the scripts run through Task 24's directives): HUD — hotbar populated (counts, a damaged tool, an enchanted item's glint), selection changes and the popup; health damaged/healed/blinking and absorption; hunger with its jitter; armour equipped; air under water; experience via `/xp`; the hurt camera roll from a controlled fall (a pre/post pair); the crosshair over a dark and a mid-grey backdrop. Containers — one leg per kind (chest 6-row and 3-row, hopper, dispenser, dropper, furnace mid-burn, brewing mid-brew, crafting, anvil with the cost and the "Too Expensive!" state, beacon, enchanting with the book, villager in-stock and out-of-stock, horse, the inventory with the preview). The gesture matrix per container (left/right click, shift-click, the number-key swap, the double-click gather, Q and Ctrl+Q, the outside drop, both drags) — each gesture asserted server-side via console oracles (`/replaceitem`, `/testforblock` where applicable; window states read back) AND captured for the screen comparison at matched contents; ONE proxied wire leg records the C0E shape (count and hex per the P4 wire rule). The sign round-trip (place, open, edit, done; the C12 on the wire leg; the in-world text). The book (the `MC|BOpen` path reachable from a held book, derived; reader frames). The creative screen (tabs, search, a C10 take, the middle-pick, the delete slot).
  - The checklist rows: 6, 7, 8, 9, 10, 14, 15, 16, 17, 18, 19, 20, 25, 53, 57 + the row-10 correction ("two 1-px quads" → the source's 16×16 quad, `GuiIngame.java`:175-180) + the §17 Gaps close: read the eleven container sheets' dimensions from the asset store (sizes are file facts; a reading, not a rig measurement) and record them on the row's Gaps entry.
  - The keepalive watch: one session soaking several minutes with the HUD live and a second with a container open, plus the standing burst pin — the drain and `world_age` rates recorded (the standing margin carried from M4).
  - The multi-round shape (name it): (a) machinery + the HUD legs, (b) the container legs, (c) the FINISH pass (the report at `refs/m5-acceptance/report.md`, the checklist edits, the gate, the commit) with a read-only findings triage lane in parallel when the run leaves open findings; every confirmed finding chained into its own scoped fix round.
- Consumes: Tasks 1–26; the rig rules (teleport both clients; one client per pose; weather/time re-sent per capture; give items after respawn; the capture rect re-read per round; mixed-generation pairing rules; `world_scan.py` stays barred — its column index is still broken).

- [ ] **Step 1: Machinery + HUD legs.** Rig up; the server console drives the states; the scripted client walks the HUD scene set; frames + the metric; the run log kept current with a RESUME block at the top.
- [ ] **Step 2: Container legs + the wire leg.** The gesture matrix per container; the console oracles; the proxy capture for the C0E shapes.
- [ ] **Step 3: Sign, book, creative legs.** The round-trips; the BOpen path; the creative flows.
- [ ] **Step 4: FINISH.** The report; the checklist edits (rows + the correction + the Gaps close); findings triaged (real-defect / rig-artifact / live-validation) with fix rounds as ruled; the gate; the commit:

```
docs: record the M5 acceptance evidence
```

**Verification:** the report's measured values; the checklist diff; the gate; the rig left clean (ports free, world saved, clients stopped).

---

### Task 28: The close

**Goal:** The milestone closes: the final whole-branch review, its findings folded, the docs swept, the tag pushed with CI verified.

**Files:**
- Modify: `docs/STATE.md`, `CHANGELOG.md`, `docs/handoff/<date>-m5-close.md` (new), `docs/perf.md`, `README.md`, the plan's own close checks + the `*(Executed …)*` note.

**Interfaces:**
- Produces:
  - The freeze: the milestone diff split per crate + combined + docs + the `-U10` review package, with the commit list, final stat, frozen shas and the head sha beside them; the reviewer's backlog-triage input = the backlog sweep's open/re-carried rows extracted mechanically.
  - The final whole-branch review, in the M4 shape: deferred triage first, then cross-cutting, then the criteria walk, with scratch-worktree evidence duties; findings adjudicated (Critical/Important route to fix rounds + a scoped re-review; record-only notes ride the close's recordings).
  - The docs sweep in one movement: STATE (the stage line — spec version, milestone complete, tag, reviewed head, verdict; the dated update bullet; tests/CI/live-evidence bullets; a new "M5 evidence" section with the acceptance numbers and the documented class list; caveats "carried by M5 as delivered", each with its owner; "Next actions" = the ordered M6 backlog; env facts refreshed — re-check the desktop and the local stable); CHANGELOG (the milestone section + the dependency line — none added); the new handoff; `docs/perf.md`'s summary line; README's status paragraph.
  - The tidy pass on the FINAL bytes (the committed-docs grep; the document conventions).
  - The close: content gate; the close commit; the tag `m5` with the prior tags' message shape; push `--follow-tags`; CI verified on the pushed head (and the plan-era pending run resolved first); the run-record follow-up commit once green; the ledger close.
- Consumes: every task's records; the review verdict.

- [ ] **Step 1: Freeze + final review.** The package; the dispatch; the verdict.
- [ ] **Step 2: Fold the verdict.** Fix rounds as ruled + the scoped re-review(s).
- [ ] **Step 3: The docs sweep.** All files above, content-gated before staging.
- [ ] **Step 4: Tidy pass + close.** The grep on final bytes; the gate; the close commit; the tag; the push; the CI watch.
- [ ] **Step 5: The run record.** The follow-up commit once CI is green; the ledger close; the owner's summary.

**Verification:** the reviewer's verdict; the tidy grep's clean run; `GATE OK` on the final revision; CI green on the pushed head; the tag's read-back.

---

## Coverage table

| Binding item | Where the plan answers it | Evidence |
| --- | --- | --- |
| Spec §13 row **M5 HUD and inventory** ("Hotbar, health, hunger, armour, experience, air, item icons, tooltips, every server-openable container with drag and split semantics, sign editing, book reading, creative inventory"; exit "Inventory and container round-trips match the vanilla client screen for screen") | Tasks 1–24; the exit at Task 27 | The acceptance report's screen-for-screen legs + the console oracles |
| Spec §11.2 HUD list (crosshair, hotbar, stat rows, air, hurt feedback (the roll, M3), potion overlays…) | Tasks 10–15, 17; third-person, F1 and the spectator HUD route to M6 (Q6) | Checklist rows 6–10, 14 |
| Spec §11.2 containers + sign/book/creative | Tasks 6, 16, 18–23 | Checklist rows 15–20, 57 |
| F8 — inventory, crafting and every server-openable container works behind its screen with correct click semantics | Tasks 6, 16, 18–21 | The gesture matrix asserts (Task 27) |
| F9 — HUD completeness (crosshair, hotbar, stat rows, held item) | Tasks 11–15, 17, 24 | Checklist rows; the HUD legs |
| P4/S2 — the new packets' wire shape (F6 is chat, closed in M4 — not claimed here) | Tasks 1–3 | The proxied wire leg (Task 27); the codec/writer suites |
| Checklist 6 (F3+H tooltips) | Task 17 | The F3+H case + the acceptance pair |
| Checklist 7 (hotbar geometry, icons unblurred) | Tasks 10, 11, 14 | GPU cases + the hotbar leg |
| Checklist 8 (hearts/hunger/armour/air/absorption) | Task 15 | The row cases + the states leg |
| Checklist 9 (XP bar) | Task 15 | The row cases |
| Checklist 10 (crosshair) | Task 14 | The case + the recorded check: the source draws a 16×16 quad, not two 1-px quads (`GuiIngame.java`:175-180) — the row's wording corrected at the acceptance |
| Checklist 14 (tooltips: background, border, spacing, rarity, F3+H) | Task 17 | The tooltip cases |
| Checklist 15 (container semantics closed-form) | Tasks 6, 16 | The gesture matrix + the cursor machinery |
| Checklist 16 (inventory screen, preview, no offhand) | Task 20 | The screen case + the leg |
| Checklist 17 (every container at its 1.8 size) | Tasks 18, 19; the Gaps close at 27 | The per-container legs + the store-read dimensions |
| Checklist 18 (anvil cost, "Too Expensive!") | Task 19 | The case + the leg |
| Checklist 19 (enchanting offers, glyphs, book) | Task 19 | The case + the leg (per Q1) |
| Checklist 20 (villager offers, red X) | Task 19 | The case + the leg |
| Checklist 25 (first person: display values, 6-tick swing) | Task 12 | The pose cases + the numeric swing sample |
| Checklist 53 (GUI scale) | Task 27 evaluates Auto (427×240 @ 1280×720, the M4 pin) | Re-carried: the setting + the four-scale table = M6 (rec 42); recorded |
| Checklist 57 (1.8 exclusions) | Tasks 20, 21 | The inventory/creative frames evidencing the absences |
| Checklist 55 (lang) | Recorded: the display strings land in the registry tables (the boss-name precedent); the lang loader = M6's screens (recorded) | The tables |
| §16 obligations | Task 23 (book editing → DIVERGENCES); the known-limits list | The DIVERGENCES entry; the ledger recordings |
| M4 known-limit 1 (held items/armour decoded, never drawn) | Tasks 12, 13 | The pose cases + the zoo re-leg |
| M4 known-limit 2 (non-block item table minimal) | Tasks 7–9 | The completeness tests |
| M4 known-limit 3 (potion tints unported) | Tasks 8, 11 | The tint data + the case |
| M4 known-limit 14 (deferred model layers) | Task 13 per the owner's Q2 ruling; the rest re-carried with the trigger named | The verdict recording |
| STATE backlog 1 (M5's own scope) | Tasks 1–24 | This table |
| STATE backlog 2 (the M4 known-limits) | The limit rows above + the re-carries | Recorded per row |
| STATE backlog 3 (the comment/citation sweep bundle) | Task 25 | The sweep receipts |
| STATE backlog 4 (the sweep-tooling round) | Task 25 | The extended-grammar run |
| STATE backlog 5 (the PNG-helper consolidation) | Task 26 | The consolidation diff + the fixture |
| STATE backlog 6 (the neighbour-brightness light term) | Re-carried: no M5 task touches the light path; trigger "the next light touch" (with the `light.rs`:123 fix; the RED shape is recorded in the M4 fix-a2b report) | The carry row |
| STATE backlog 7 (slab_half fixture; the T8 F7; the INPUTS_PER_PASS pin) | Task 24 pins `INPUTS_PER_PASS`; the first two re-carried with their triggers ("a protocol surface that needs the fixture"; "the next break-path packet work") | The pin test; the carry rows |
| STATE backlog 8 (fog time term; CHUNK_COORDINATE_BOUND; the keepalive margin) | Re-carried: the first two at their next touches; the margin watched at Task 27 and carried (the standing watch) | The soak rates |
| STATE backlog 9 (the operator's by-eye list) | Tasks 27, 28 — the report keeps it current for the operator | The list |
| STATE backlog 10 (later homes M6/M7/M9) | Re-carried as recorded; the new routings this plan adds (third-person + F1 → M6; the lang loader → M6) | The carry rows |
| The sweep's other re-carried rows (the store-key separator row 3; the sky horizon row 24; the celestial closing term row 25, M6's sky work; `END_OF_SESSION_WAIT` row 31.5; the landed row 34 as a record) | Re-carried with their own triggers; Task 10's new store keys ride the same mapping untouched | The carry rows |
| The M4 close's 2 Minor findings (record/route, no code defect): the sweep-row staleness (rows 6/34) and the item-5 trigger's routing | Task 25 (the record correction) and Task 26 (the consolidation the trigger routed to) | The review's own fix-shape records |
| M4 final review §2.4 (what will fight M5) | Item by item: held/armour → T12/13; the item table → T8/9; the draw schema's id-jitter wiring (rec 35) + item phase pins → T13; the second atlas upload → T10; the HUD template + the overlay pin → T14/15 (the pin consumed); `set_gui_scale` stays M6 → recorded; the input surface + D1 → T24/25; the citation family → T25; `world_scan.py` barred → the rig rules restated at T27; the keepalive margin → T27 | As above |

---

## Pre-flight (before Task 1)

- The owner's answers to the six questions above are recorded in the answers table before execution begins.
- The plan's base: HEAD `025a211` (= `origin/main`, clean at planning); the M4 close's CI green (runs `37713937089` and `37714240271`).
- The execution ledger begins (first line names this plan); the reference tree's HEAD is recorded with it: `1717f75902c6184a1ed1bfcd7880404aab4da503`.
- The two distilled references this plan cites (the HUD parity facts; the cite-sweep procedure) are mirrored to `refs/m5-homework/` before the tasks that read them (`hud-render-parity.md` before Task 14; `cite-sweep-binding.md` before Task 25).
- The conflict scan (every task pair sharing a file or interface, plus one self-consistency row per task) is written to the ledger, and each conflict is ruled on before the first dispatch; the review loop is only the net for what implementation reveals.
- `rustup update stable` before the first gate; the desktop and its scale re-checked before any rig leg; `gh auth status` before any push.
- The full gate runs clean on the base revision before Task 1's dispatch.

---

## Known limits this plan accepts

*(initial list; execution refines it, and Task 28's sweep carries the final wording)*

1. Builtin/entity item icons: the chest trio lands with Task 11 (owner Q5); skull, banner and any other `builtin/entity` parent the registry finds draw the recorded missing-sprite fallback.
2. Sign text draws on the front face only (1.8's own geometry — the read-back rule derives; nothing else exists to draw).
3. Book-and-quill editing is absent: the open shows the stored pages read-only (owner Q4; the DIVERGENCES entry).
4. The riding HUD (the horse's hearts and jump bar while mounted) stays out — a later pass; the horse screen itself is Task 19's.
5. Effect visuals exist for the own player only (the inventory overlay and the heart variants); other entities' effect draws ride the later particle pass.
6. GUI scale: Auto only, per the M4 pin; the setting and the four-scale table are M6's (rec 42).
7. Item display names come from the registry tables' embedded strings; the lang loader is M6's screens milestone (checklist row 55's route).
8. The NBT reader is display-only; strings decode per the standard UTF-8 path with the modified-UTF-8 edge recorded.
9. Absorption hearts render only if their wire source proves inside the decoded set; otherwise the row is skipped with the finding recorded (Task 15's rule).
10. The enchanting glyphs and the inventory's effects overlay land per owner Q1; otherwise recorded with the static fallbacks.
11. The anvil's name field carries the chat field engine's subset (the clipboard absence matches the chat's own recorded limit).
12. Villager trade state beyond the row-20 bits (leveling flourishes) derives and records what 1.8 actually shows.
13. The creative screen's tab state lives for the session only (the source keeps no persistence — derive and record).
14. The container screens use the minimal screen framework; the full framework (pause, options, language) is M6's.
15. Third-person views, F1's HUD hide and the spectator HUD route to M6 (owner Q6's ruling; the spectator HUD is section 11.2's third camera-class item).
16. The keepalive margin stays a standing watch (M4's carry) — Task 27 records the soak rates; any regression routes a scoped fix.
17. The crosshair row-10 wording correction and any other checklist corrections from the run are recorded, not silently rewritten.
18. Interrupted or capped acceptance rounds keep their messages and resume blocks current (the run's own discipline); re-shots are marked as later generations per the standing record rules.
