# M6 Sound, Particles, Weather, Screens Implementation Plan

> **How to work this plan:** one task at a time, in order. Run each step's verification before moving on, and tick the checkboxes (`- [ ]`) as you go. Every commit is made on `main` with explicit `git add` paths.
>
> **Plan style:** this plan pins every interface, constant, path, and rule the tasks must agree on, and carries code sketches wherever a byte layout, formula, or data shape is the deliverable. Algorithm bodies that mirror vanilla behaviour are derived from the cited MCP-919 source files (`refs/_src/MCP-919`, local and uncommitted; the tree is present from the previous milestones, HEAD `1717f75902c6184a1ed1bfcd7880404aab4da503`) by the task that needs them, and every derived constant is pinned by a test. Where this plan and a cited document disagree, the document governs and the plan text is corrected first. Pin the source's expression and cite the method that applies it where the two differ — a factor can live in the applier, not the computer.

**Goal:** The weather, the soundscape, the particles and the screens. The client's own menus appear — the main menu with its panorama, the server list with live pings, the pause menu, the options hub with its sub-screens, the death and disconnect and terrain-loading screens — and every option they expose takes effect and persists: the full 1.8 video set (render distance to 16 chunks, brightness, particles, GUI scale with its four-scale table, FOV, the toggles), the nine sound category sliders, the controls screen with rebindable keys, sensitivity and inversion, and the language, chat and skin lists. The world gains weather: rain and thunder arrive on the wire, darken the sky, fog and light, draw their columns and splashes, and sound on the ambient channel; the sky grows its sunrise/sunset band, and the overlays grow the pumpkin blur and the portal wobble. The sound engine plays the vanilla events from the rig's own sound index on a category mixer, streams records and music, and the music ticker runs its 10-to-20-minute schedule (a fade only if its source site is found). The particle engine renders the vanilla particle set — wire-driven and locally spawned — behind the F3 overlay's own count, and F3 grows its sub-modes (hitboxes, chunk borders, the lagometer), F2 saves screenshots, and the window resizes and goes fullscreen.

**Architecture:** A settings store lands in `oxide-game` (`GameSettings`: the vanilla option set with per-field wire/local split), and the client owns its lifetime: read from `options.toml` at startup, written on change (the M0 store layout already reserves the path), and pushed to the renderer and the session through one apply path — the session keeps its join-time `ClientSettings`, which the store now feeds; the renderer's GUI scale, brightness, particles and fullscreen state are settable mid-session. The wire layer decodes the three weather/sound packets (0x29 Sound Effect, 0x2A Particle, 0x2B Change Game State) and 0x2C Spawn Global Entity; the session accumulates the weather state (rain and thunder strength, each easing toward its target per tick) and publishes `WeatherChanged`, particle spawns and sound calls beside the existing events. A particle engine in `oxide-game` simulates a pooled, capped particle set (the vanilla behaviours, the `ParticleSetting` reduction, biome-coloured block dust) and reports each tick; the renderer draws the billboard/collision-free subset from an atlas region and the batch types from the block atlas. An audio module in `oxide-game` owns a cpal output stream and a mixer: it consumes the `sounds.json` index the assets crate parses from the store, resolves events to files, applies the category gain, the volume/pitch clamps, the call-site randomisation and the distance attenuation, streams `records.*`/`music.*`, and degrades to a silent device that keeps every behavioural path intact. The renderer gains the weather and sky pass updates (the rain/snow columns, the splash spawn rule, the sunrise/sunset band, the sky-band rain dim, the stars' brightness chain, the fog colour's rain and time terms) and the overlay draws (pumpkin blur, portal wobble, the block-in-hand tint). The client's screen framework grows the menu family: the main menu with its panorama and splash, the server list with the M1 status-ping path, the pause menu, the options hub and its sub-screens (video, sounds, controls, language, chat, skin, snooper, packs), the death/disconnect/terrain screens, the open-link confirm overlay, and the sleep overlay; a text-field engine extracted from the chat field serves the anvil-name and book-field corners; the keymap becomes data-driven from the settings store with a controls screen over it. F2 renders the frame to an offscreen copy for a PNG save; F3 grows its sub-modes and its field set; the render distance slider moves the session's view distance live (C15 re-send) and the far plane with it; and the window handles resize and fullscreen through winit with the settings store as the source of truth.

**Tech Stack:** Rust 2024 edition (rust-version 1.85, developed on 1.99), the M5 stack plus three new dependencies the spec's appendix A plans for exactly this milestone: `cpal` (audio output), `lewton` (Ogg Vorbis decode) and `rand` (the source's own randomisation — particle velocities, sound pitch variance, the local spawn rules). None of the three is in the lock yet; all are new. No crate edges beyond the section 5.1 table: the audio module lives in `oxide-game` (whose key item list already names sound playback), the sound index parses in `oxide-assets` (whose key items already name `SoundIndex`), and the settings store lives in `oxide-game` with the client owning its file. `deny.toml` and the CI dependency surface are updated in the landing task.

**Spec:** `docs/specs/oxidecraft-v1-design.md` (v6). M6 is the section 13 row "Sound, particles, weather, screens", with scope *"Sound engine and music ticker, particles, weather, sky and stars, overlays, the full screen list from section 11.2, keybinds, F3 with sub-modes, F2 screenshots, the 1.8 render-distance range (to 16 chunks), window resize and fullscreen"* and exit *"The named screen list passes its item-by-item parity checks, including the F3 field set"*. Read section 11.2 (the screen, HUD and sound lists), `docs/research/render-parity-survey.md` §3.1 (the screen inventory), §3.3 (the F3 field list), §4.1 (sky, weather), §4.6 (the particle table), §6 (the sound tables) and `docs/parity/checklist.md` (rows 1–6, 22, 24, 26, 28–32, 34–38, 47–49, 51–53, 55–57) before the matching tasks; section 8's packet rows 0x29/0x2A/0x2B/0x2C in `docs/research/protocol-47-reference.md` §2 before Task 2; appendix C.2 before the acceptance; section 16 (the Azure item and the DIVERGENCES obligations) at the close. Vanilla citations are files under `refs/_src/MCP-919` (uncommitted), the same source the research reports cite. The M5 final review's "what will fight M6" list (`refs/m5-task-28/final-review.md` §2.4 — the carry notes in `docs/STATE.md` itemise it) is this plan's standing risk list, and each item is answered by a task below.

## Global Constraints

- License GPL-3.0. Adapted third-party code must be recorded in `NOTICE`.
- Zero code copied from RustCraft. It is a read-only reference only.
- No Mojang asset, jar, `.class` file, `.ogg`, or `.png` may ever be committed. Nothing under `refs/` or `vanilla/` is committed; textures, models, fonts, colormaps, sounds and the sound index are read from the user's own store at runtime only. No test fixture may embed a Mojang pixel or sample: fixtures are synthetic (generated in the test) or read from the store by an ignored test.
- Never read `.class` files from the jar at runtime. Design invariant.
- rust-version 1.85, edition 2024. `Cargo.lock` is committed. The new dependencies (`cpal`, `lewton`, `rand`) must resolve within 1.85: pin the newest versions whose declared rust-version fits, the `wgpu` precedent (record the resolution in the landing task).
- CI must fail on: formatting, clippy warnings, test failure, license violations, crate-graph violations, or a tracked Mojang asset. `deny.toml` gains the three crates' licences in the same commit that adds them.
- This milestone adds **no crate edge** — the allowed edges are exactly the section 5.1 table, which `scripts/check-graph.sh` keeps asserting unchanged. (`oxide-game` already may depend on `oxide-assets`; the audio module uses that existing edge.)
- Every public item carries a doc comment (workspace lint `missing_docs`); `unsafe_code` is forbidden workspace-wide.
- `git add` is always explicit with paths; never `git add -A` or `git add .`.
- Evidence (captures, screenshots, logs, audio captures) lives under the git-ignored `refs/` tree. Committed documents cite it by path.
- The local gate before every push is the command set plus both guard self-tests, the parity self-test and the declared-MSRV check: `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test`, `bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test`, `python3 scripts/parity-diff.py --self-test`, `cargo +1.85.0 check --workspace --all-targets`.
- Values that come off the wire are hostile until validated: the sound name (length-capped before lookup — a missing entry plays nothing and is counted), the particle id and its argument list (the particle table's own arg counts), the particle/dust data float, the Change Game State reason and value, and every settings-file value on load (unknown keys ignored, out-of-range values clamped, the file never panics the client). Framing never panics (spec S2).
- Numerical parity: every settings default, slider range and step, screen layout coordinate, sound clamp, attenuation rule, particle behaviour constant, sky band value, fog term, overlay alpha rule and F3 field this milestone introduces is traceable to a cited MCP-919 file or a research report, and is pinned by a test. A constant with no citation does not land.
- The keepalive/read-loop discipline is binding and extends to every new handler: the play loop's added per-packet work stays bounded (a weather-state write, a particle spawn into a capped pool, a sound command send — never per-frame layout, decode or mixing work in the read pass), and the `session_replay` keepalive test must keep passing through every restructuring.
- The audio thread never blocks the tick or read threads: play calls are channel sends; device reconfiguration is the audio thread's own work; a device that cannot open degrades to silent and the client keeps running.

## Decisions taken in this plan

| # | Decision | Rationale |
| --- | --- | --- |
| 1 | **The settings store lives in `oxide-game` (`src/settings.rs`), and the client owns its file.** `GameSettings` carries the whole vanilla option set (the per-field split below); `oxide-client` reads `options.toml` from the store's data directory at startup, writes it on change (debounced to screen-close), and owns the apply path that pushes renderer state (`set_gui_scale`, brightness, particles, fullscreen, clouds, graphics, smooth lighting) and session state (view distance → the C15 re-send, the join settings) from one place. | Spec §6: the client owns the event loop and one frame of truth; §5.3 keys `Game` to settings-adjacent state and sound playback to `oxide-game`. The M5 review's §2.4 item 1 is exactly this unification (`session.rs:278`'s "until the client owns them" comment is the seam; the join-time settings field sits at `:230`). A settings type in `oxide-game` is testable headless and keeps the client thin. |
| 2 | **Settings are split per field: wire fields re-send clientbound settings; local fields apply in place.** Wire: view distance (C15 Client Settings re-send on change — the source's own behaviour derives in-task; Task 5's rule governs: if 1.8 only applies the change at reconnect, that path is the pinned rule and this wording is corrected to match), chat mode, chat colours, skin parts byte. Local: everything video/audio/controls. The render distance applies immediately: the far plane, fog and mesher follow the settings store, and the session's view distance follows the C15. | The M5 review item 3 (`ALL_PARTS = 0x7F` used `view.rs:5131`) closes here — the skin-parts byte becomes the settings' own; item 4's pointer scaling and item 1's render-distance range ride the same apply path. |
| 3 | **The particle engine is `oxide-game`'s, the draw is `oxide-render`'s.** `ParticleEngine` owns the pool (cap per setting, the vanilla `ParticleSetting` reduction), each particle's behaviour (the flat table below — billboard, friction, gravity, lifetime, the argument uses), and the per-tick integration; the renderer's particle pass reads `ParticleEngine::snapshot()` (position, age, size, colour, tex, batch index) and draws it: billboard quads from `particle/particles.png` (8×8 cells), block/item batches sampling the block atlas with the block's own colour and biome tint; wire spawns publish `ClientEvent::Particle`. Local spawn sites (block-break particles from the dig path, sprint/land clouds, splash on water impact) ride the dig/interaction taps the milestones already own. | The source splits it the same way (`EffectRenderer` simulates, the renderer draws); the session owns wire state and tick state, and the pool cap is a simulation rule. Particle count feeds F3's `P:` field from the engine's own statistics. |
| 4 | **The audio module is `oxide-game`'s (`src/audio/`), over `cpal` + `lewton`, with a channel-fed mixer on its own thread.** The tick thread sends `AudioCommand` (play with category/position/volume/pitch, category volume, music control, listener pose); the audio thread owns the device, the decoded-sample cache, active voices, the streamed music/record players, and a small resampler to the device rate. Unit-testable core: the mixer's per-voice gain math (category × volume × attenuation × fade) is a pure function; the device is a thin shell behind a trait so CI and headless runs take a silent implementation and the audible path is covered by an ignored, store-gated integration test plus the rig capture. | Spec §5.3 names sound playback under `oxide-game`; the source's `SoundManager` is exactly this shape (a system + a mixer). `lewton` decodes Ogg Vorbis synchronously; decoding is done ahead of playback on the audio thread (64 kB ogg files — the survey's index — decode in milliseconds) and cached. |
| 5 | **The sound index parses in `oxide-assets` (`src/sound.rs`) from the store's `sounds.json`.** `SoundIndex` resolves an event name to its file list plus flags (`category`, `stream`, `volume`, `pitch`, `weight`, `attenuation_distance`), validates the walk (caps on events, files per event, string lengths), and resolves `minecraft:`-style paths to store object paths through the index (the survey: `minecraft/sounds/**` are asset-index objects). The rig store contains the file at `extracted/1.8.9/assets/minecraft/sounds.json` when present, else the index object; the loader takes whatever path exists and records which. | The spec's key-item table already reserves `SoundIndex` here, and the index is a store artifact — `oxide-game` never touches paths. |
| 6 | **Weather is session state with per-tick easing; the renderer receives one parameter set.** The session accumulates rain and thunder strength from 0x2B (reasons 1/2/7/8; 7's rain-strength value and 8's thunder-strength value fold as they arrive), eases each toward its target per tick at the source's rate (derives in-task from `World.java`'s rain-strength update), and folds `World.getStarBrightness`/`getSkyColor`/`getFogColor`'s rain terms in; `WeatherChanged { rain, thunder }` publishes each change. The renderer's `SkyParams` grows `rain_strength`, `thunder_strength` and the horizon tuple; the sky band, stars, sun/moon alpha, fog colour and rain columns all read them. The client's `SkyParams` construction (the M2 stand-in at `main.rs:1392`) is replaced wholesale. | One owner (session) for wire weather; the renderer stays pure. The sky band's `1 − rain` alpha and the star chain are already documented in `sky.rs` as the M2 stand-ins — this closes them. |
| 7 | **The screen family is client-side, and M6's share of §11.2 is the multiplayer-client subset.** New screens: main menu (panorama + splash + logo), multiplayer server list (add/edit/play/refresh, the M1 ping path, MOTD with §-codes, player counts), pause menu (Back to Game, Statistics and Achievements entries that open the recorded placeholder screens — the stat sheets' full grids stay post-v1, recorded — Options, the no-op LAN entry recorded per the spec, and Disconnect), the options hub and its sub-screens (Video, Sounds, Controls, Language, Chat, Skin Customization, Snooper, Resource Packs — packs a recorded no-op), the death screen, the disconnect screen, the downloading-terrain screen, the sleep overlay (a client-side, purely visual camera-and-HUD state — v1's servers can send sleep status; the camera rise derives from the source), the open-link confirm overlay (`GuiConfirmOpenLink`), and the chat settings' tab-completion. Out (recorded): singleplayer world list/create (post-v1 §17.1), win/credits, demo, LAN share, the real resource-pack loader (§16 no-op per D14), the command-block editor (post-v1), stat grids and the achievement grid beyond their entries (recorded). | The M6 row says "the full screen list from section 11.2" — this is that list restricted to multiplayer v1's scope, item by item in the coverage table. Each exclusion is a §16/DIVERGENCES shape, recorded at the close. |
| 8 | **The F3 overlay is rebuilt to the field list, and its sub-modes land.** Left column: the M5 overlay's rows kept, plus `P:` from the particle engine's statistics, the biome line, `Light:` and `Local Difficulty:` from the world state (new decode work: biome name per column — the `0x21`/`0x26` chunk data's biome bytes (protocol §3.1/§3.2), derives in-task; the day count and difficulty from Join Game plus elapsed ticks), `Looking at:` plus the block registry name under F3+H. Right column: `Java:` (recorded value — the JRE our own client is not; the line carries our rustc/build id with the field name kept so the diff masks it, recorded), `Mem:` (process RSS via `sysinfo`-free `/proc` read on Linux, recorded fallback on other targets), `CPU:` (the machine's CPU string from `/proc/cpuinfo`), `Display:` (wgpu adapter + resolution). Sub-modes: F3+H (advanced tooltips — the M5 appendix), F3+B (hitboxes + look vectors — the entity boxes and an eye-line draw), F3+G (chunk borders — the 16×16 grid at section corners), Shift+F3 (lagometer — the tick-duration graph, fed by the session's tick timing). F3+P (pause on lost focus), F3+A (reload columns — the session's rebuild request), F3+F (render-distance cycle — the settings' own range) fold; F3+T (resource reload) is a recorded no-op (no packs in v1); F3+N (spectator cycle) is out (no spectator mode in v1, recorded); F3+Q help list folds as a chat print of the bindings. | The exit names "the F3 field set"; checklist rows 5 and 6 carry the values. `Java:`/`Mem:` shapes: the field set is the deliverable, the values are environment facts, masked in the image diff either way. |
| 9 | **F2 screenshots render the frame once more to an offscreen RGBA texture.** The renderer gains a `capture_frame` path: draw the current frame into an offscreen color texture (the same draw list and passes), copy to a buffer, map, and hand bytes to the client; the client writes a PNG (the assets crate's `png` dependency, already present) to `<store>/screenshots/<timestamp>.png` with the timestamp naming (derive the source's format, `ScreenShotHelper.java`) and logs the path. F2 (and the chat/screenshot keys) route through the keymap. | The window surface itself cannot be read back cheaply through wgpu; an offscreen re-render is one frame of work on a keypress — the source does one readback copy the same way. The capture is also the acceptance's own tool. |
| 10 | **Fullscreen and resize: winit's own modes, settings-owned, applied immediately.** The fullscreen toggle drives `Window::set_fullscreen` (Borderless on the native monitor; the mode list is the recorded substitute for the source's DisplayMode list, whose resolution cycling is recorded as a divergence of the environment), the settings store persists the boolean, `Event::Resized` already reconfigures the renderer and now re-derives the scaled resolution from the current GUI scale; the F11 default binding derives from the source's keybind (`GameSettings`' fullscreen key, `F11`) and is rebindable like the rest. | Checklist row 51-adjacent (F11 fullscreen is 1.8's own default); the resize path exists since M0 (`main.rs:4469`) and gains only the scale re-derive. |
| 11 | **Max Framerate's limiter applies at the video-settings touch, VSync applies immediately.** VSync (default on) sets the surface present mode (`Fifo` on / `AutoVsync`+`Immediate` off, the documented fallback order) and reconfigures the surface; the Max Framerate slider (10–260, cap = unlimited, the source's `limitFramerate` cycle) lands its frame limiter in the client's redraw loop — the slider is not a no-op. The limiter is a wall-clock gate on frame submission, pinned by a unit test on the gate function, not by a frame-rate measurement (that is M9's). | Checklist row 4's slider list must behave; the M2 caveats kept VSync off-topic deliberately, and the perf milestone (M9) owns measurement, not the mechanism. |
| 12 | **The render distance slider's range derives on the rig (enum base 2–16; the 64-bit path raises the max to 32 with a ≥ 1 GB heap).** The 1.8.9 options screen's own slider range derives in-task from the source (`GameSettings`/`GuiOptions` — the spec §16 rule: "confirm on the rig in M6 against the vanilla options screen"); `GameSettings.java:195-204` shows the 64-bit constructor's `setValueMax(32.0F)`. The value found on the rig governs and the constant is pinned by a test. The slider drives the settings store, the far plane, the fog range, the mesher's view distance and the session's C15 re-send. | §16's own item, resolved this milestone as the spec instructs. The plan does not pin the literal before the rig confirms; the task derives and pins it, expecting 32 on this 64-bit rig. |
| 13 | **The audio acceptance is captured at the mixer, not the speakers.** The acceptance records: (a) the engine's own event log (event name, resolved file, category, gain, pitch) for the same scripted scenes run on both clients where the vanilla side's own log is unobtainable — the engine log is the port's evidence, and it is compared against the sound index and the source's call sites; (b) a raw PCM capture of the mixer output (`--audio-dump <file>`, a debug flag writing the mixer output as a 16-bit stereo WAV) for the rain loop, a dig, and a record, checked for non-silence, the expected channel count, and the category gain law; (c) the record/music streaming legs for the streamed path. The vanilla comparison is the event mapping and cadence (verify the vanilla client's sound plays by ear is the operator's optional item, recorded). | The rig cannot record system audio cleanly on this desktop (the M5 HUD-key gaps are the same class); the mixer dump is deterministic evidence of the port's own behaviour and the operator's by-ear pass is the recorded supplement. |
| 14 | **The F-01/F-02 carries close in Task 1, and the process fix is stated.** Both pins are updated first-class (F-01 to the constant-50 rule with the same-z tie-break assert kept; F-02 by following the `builtin/*` parent chain for the two display poses — or pinning the two entries if the chain proves out of scope for the pose source, either way with the pin green), and the task also re-runs the full ignored HUD/creative set at the final head. The M6 gate template gains the ignored-run sweep instruction the M5 close identified (gates run non-ignored suites only, so ignored pins a round touches are never re-run — each task that touches render/assets code states which ignored case prefixes it re-ran). | The handoff's first work items; the process note is F-01/F-02's own lesson, carried into the gate recipe. |
| 15 | **The citation/comment bundle and the operator list keep their standing shape.** The still-open comment rows (the M5 review's §1b open set: 38, D3, D5-remainder, D12, D14–D16, D19, D26, D27, D30, D31, 73a, 73c, 78b, T18-N1, plus the plan's own new rows below) ride Task 29's sweep pass at their files' touches; the operator's by-eye list (M3's audit carries, M4's and M5's frames plus this milestone's new frames) is kept current in the acceptance report for the owner. The extractor's grammar needs no extension this time (T25's delivered grammar covers the forms in use; the rows are content fixes). | The M5 review's triage rulings; the sweep is scoped to rows, not tooling. |

## Open questions for the owner (answers recorded on approval)

1. **The audio stack (Decision 4): `cpal` + `lewton` + `rand` land as this milestone's only new dependencies, with a silent-device fallback.** The spec's appendix A plans cpal and lewton for exactly this milestone; `rand` is this plan's addition for the source's own randomisation. All three are new to the lock. The MSRV fit is checked at landing (newest versions declaring ≤ 1.85, the `wgpu` precedent). **Recommended: yes** — the sound engine is M6's scope by the section 13 row; a silent fallback keeps CI and headless runs green.
2. **The screens scope (Decision 7): the multiplayer subset of §11.2 lands; singleplayer/create-world, win/credits, demo and LAN screens stay out, recorded.** **Recommended: yes** — §2 keeps singleplayer post-v1 (§17.1) and the M6 exit's "named screen list" reads against v1's own scope; each exclusion is recorded in the coverage table and the close's caveats.
3. **The F3 right column's `Java:`/`Mem:`/`CPU:` lines carry environment values under the vanilla field names** (Decision 8). **Recommended: yes** — the exit asks for the field set; the values are masked in the image diff and their provenance is recorded (our rustc/build id, Linux `/proc` reads).
4. **Statistics and Achievements screens (the pause menu's entries): placeholder screens with the entries' frames and a recorded limit** (Decision 7). **Recommended: yes** — the stat sheets' grids and the achievement tree are their own data surfaces; the entries' existence is what the pause-menu row checks.
5. **The sleep overlay lands as the visual client state** (Decision 7): a semi-transparent overlay plus the camera rise while the server reports sleeping, derivable cleanly from the source. **Recommended: yes** — bounded, and it completes §11.2's overlay item without the server protocol corners.
6. **F3's F3+N and F3+T record as no-ops** (Decision 8; no spectator mode and no pack reloads in v1). **Recommended: yes** — both are inert by construction; the recorded no-ops keep the sub-mode table complete.

### Answers recorded on approval (2026-10-11)

| # | Question | Answer |
| --- | --- | --- |
| 1 | The audio stack | **Yes** — `cpal` + `lewton` + `rand` land with the silent fallback; the MSRV fit is checked at landing (owner, 2026-10-11). |
| 2 | The screens scope | **Yes** — the multiplayer subset of §11.2; the exclusions are recorded in the coverage table and the close's caveats. |
| 3 | F3's environment lines | **Yes** — the `Java:`/`Mem:`/`CPU:` lines carry our environment's values under the vanilla field names; masked in the image diff; the provenance recorded. |
| 4 | Statistics/Achievements placeholders | **Yes** — the entries and their frames; the grids stay post-v1. |
| 5 | The sleep overlay | **Yes** — the visual client state; unreachable corners are recorded non-passed. |
| 6 | F3+N / F3+T no-ops | **Yes** — both recorded no-ops. |
---

### Task 1: The carried GPU pins (F-01, F-02)

**Goal:** The two ignored pins the M5 close carried are green again on the reviewed rule: the HUD/creative overlap pin encodes the shipped constant-50 `icon_z_level()` with its same-z tie-break assert kept, and the compass/clock entries carry their first-person display poses through the `builtin/*` parent chain (or, where the chain is out of the pose source's reach, the two entries are pinned directly).

**Files:**
- Modify: `crates/oxide-render/tests/pipeline_headless.rs` — the two pins (`the_hud_pass_draws_the_later_icon_over_the_earlier_one`; `t21_creative_draws_strip_grid_thumb_search_and_bin`).
- Modify: `crates/oxide-assets/src/model.rs` (or the pose resolution chain the F-02 fix shape names — the task reads `refs/m5-task-28/final-review.md` §6 first and follows its fix shape; if the chain lives in `oxide-client/src/items.rs`'s table, that file) — the display-pose resolution for `builtin/compass` / `builtin/clock`.
- Test: the same files; the ignored GPU set re-runs.

**Interfaces:**
- Produces: both pins green under `cargo test -p oxide-render --test pipeline_headless -- --ignored` with `OXIDECRAFT_STORE`; the compass/clock rows' `first_person` transform equal to the source's `[0,−135,25]`/`[0,4,2]`/`1.7` (`refs/m5-task-28/final-review.md` §6's values, re-read from the store's own `models/item/compass.json`/`clock.json` — values only, never committed).
- Consumes: `icon_z_level()` as shipped (`gui_item.rs:364-366`); the M5 review §6's two fix shapes.

- [ ] **Step 1: F-01 — update the pin (RED→green).** The overlap pin asserts the constant-50 rule and keeps a tie-break assert at same z (the review's fix shape: encode the tie-break explicitly rather than deleting the overlap assert). Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-render --test pipeline_headless -- --ignored the_hud_pass_draws_the_later_icon_over_the_earlier_one` — red first (it pins the old rung), green after.
- [ ] **Step 2: F-02 — follow the chain (RED→green).** Extend the pose resolution to read the `builtin/*` parents' display transforms (the review's chain), or pin the two entries when the chain proves unreachable; the store test `the_item_icons_build_from_the_store` (`oxide-client/src/assets.rs`) goes green at 210/210 with `first_person` non-default. Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-client the_item_icons_build_from_the_store` — red first, green after.
- [ ] **Step 3: Re-run the touched ignored sets at the final bytes.** `cargo test -p oxide-render --test pipeline_headless -- --ignored` (store env, T500) and `cargo test -p oxide-client -- --ignored` (store env) — the whole suites, not the two cases, because the fix reaches shared pose code.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
fix(render): update the overlap pins and the compass display poses
```

**Verification:** the two named cases green; both ignored suites re-run green at the final bytes with the store env; the full gate; `git diff` shows the pins and the pose chain only.

---

### Task 2: The sound, particle and weather codecs

**Goal:** The protocol crate decodes the sound-effect, particle, game-state and global-entity packets with hostile-input caps and fixture pins, so the session can consume weather, particles and sounds off the wire.

**Files:**
- Create: `crates/oxide-proto-v47/src/effects_wire.rs` — the structs, the pinned particle-name table the packet's data arguments key into, the decoders.
- Modify: `crates/oxide-proto-v47/src/lib.rs` — module declaration and re-exports.
- Test: `crates/oxide-proto-v47/src/effects_wire.rs` — inline unit tests.
- Test: `crates/oxide-proto-v47/tests/effects_wire.rs` — the fixture corpus (hand-built byte vectors per struct; hostile cases).

**Interfaces:**
- Produces (all `oxide_proto_v47::effects_wire`):
  - `SoundEffect { name: String, x: i32, y: i32, z: i32, volume: f32, pitch: u8 }` — 0x29; the name length-capped (the crate's string cap), the fixed-point ×8 rule derives from the source's read (volume/pitch are the source's own scales — derive and pin: `S29PacketSoundEffect`'s constructor multiplies — the three axes ×8 at `:27-29`, the pitch ×63 at `:31`; the getters divide back at `:66-79` and `:86-89`; `handleSoundEffect` passes through unchanged (`NetHandlerPlayClient.java:1695-1699`); pin the pass-through pair once).
  - `Particle { particle_id: i32, long_distance: bool, x: f32, y: f32, z: f32, offset_x: f32, offset_y: f32, offset_z: f32, speed: f32, count: i32, data: Vec<i32> }` — 0x2A; the data run reads exactly the table's argument count per id (no wire length prefix exists — `ITEM_CRACK` 2, `BLOCK_CRACK`/`BLOCK_DUST` 1, else 0; the run is capped at `MAX_PARTICLE_DATA = 4`), the id mapped into 0–41 at decode (unknown → `BARRIER`), `count` clamped to a sane ceiling (`MAX_PARTICLE_COUNT = 512` — round-trip decides; a count past the cap is refused, test-pinned).
  - `ChangeGameState { reason: u8, value: f32 }` — 0x2B; every reason decodes (0–10), semantics per-destination, none written for 9 (no vanilla handler — `NetHandlerPlayClient` covers 1–8 and 10, 0 via the message table, 9 ignored).
  - `SpawnGlobalEntity { entity_id: i32, kind: u8, x: i32, y: i32, z: i32 }` — 0x2C (kind 1 = lightning; the entity id registers in the session's entity store for 0x13 removal — kind other than 1 keeps the entity record and draws nothing, recorded).
  - `PARTICLE_NAMES: [&str; 42]` — the full source table (`EnumParticleTypes` ids 0–41; the survey's §4.6 list stops at `WATER_DROP` (39) and misses `ITEM_TAKE` (40) / `MOB_APPEARANCE` (41) — the task pins the source's 42 rows and corrects the survey row in passing), pinned index-by-index; `particle_name(id) -> Option<&str>`; the decoder maps an id outside the table to `BARRIER` (id 35), exactly as `S2APacketParticles.readPacketData` does.
- Consumes: the crate's cursor/string helpers; §2.1's packet ids.

- [ ] **Step 1: Fixture tests (RED).** One hand-built vector per struct with catching values: a named sound at negative fixed coords with a fractional volume; a particle with 3 floats moved and a 1-arg data; a count at the cap; rain begin (reason 2) with value 0; fade value 7 and fade time 8; a lightning spawn. Assert every field. Run: `cargo test -p oxide-proto-v47 effects_wire` — red.
- [ ] **Step 2: Implement the decoders.** Run — green.
- [ ] **Step 3: Hostile set (RED→green).** A name past the string cap; a data run truncated inside the table's argument count (the source reads exactly `getArgumentCount()` varints — no wire length exists, so an over-long list cannot occur); a count past the cap; truncations per struct; a reason byte above 10 (kept, never refused — the source ignores unknown reasons; pin that); a particle id above 41 (mapped to `BARRIER` at decode — the source's own rule; an out-of-table id never reaches the session). Run — green.
- [ ] **Step 4: The name table.** Index-by-index pairs against `EnumParticleTypes` in the reference tree (all 42 rows; the survey's stop at 39 is corrected in `docs/research/render-parity-survey.md` §4.6 in this pass and the deviation recorded). Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: decode the sound, particle and weather packets (proto)
```

**Verification:** `cargo test -p oxide-proto-v47`; `cargo fmt --all --check`; `cargo clippy -p oxide-proto-v47 --all-targets -- -D warnings`.

---

### Task 3: The settings store

**Goal:** `oxide-game` owns the vanilla option set as one type — defaults, ranges, the per-field wire/local split, the derived view distance — with `options.toml` load and save under a strict parser, so the client and the options screens have one source of truth.

**Files:**
- Create: `crates/oxide-game/src/settings.rs` — `GameSettings`, the ranges, the load/save pair, the derived getters.
- Modify: `crates/oxide-game/src/lib.rs` — module declaration.
- Test: `crates/oxide-game/src/settings.rs` — inline unit tests (defaults, ranges, parse/serialize round-trips, hostile files).

**Interfaces:**
- Produces:
  - `oxide_game::settings::GameSettings` with the vanilla field set and defaults (`GameSettings.java`:67-95 — the task re-reads the field initialisers and pins each default): `render_distance: u8` (the source's constructor default: 12 on a 64-bit JVM, 8 otherwise — `GameSettings.java:204`; record both), `brightness: u8` (the source's default sits at the un-set 0.0 end — `gammaSetting` has no initialiser; its label is `options.gamma.min` = Moody — pin the mode index 0.0 maps to), `particles: ParticleSetting` (All), `gui_scale: u8` (0 = auto), `fov: u8` (70), `max_framerate: u8` (the source's default — derive; the "unlimited" sentinel included), `mipmap_levels: u8` (4), `vsync: bool` (true), `view_bobbing: bool` (true), `use_vbos` (recorded — our renderer is always on VBOs; the toggle persists and is a recorded no-op), `alternate_blocks: bool` (true; drives the mesher's own rule at its next touch — recorded), `entity_shadows: bool` (true; the renderer's shadow draws follow — the entity pass's shadow draws read it), `clouds: CloudSetting` (Fancy default; Off/Fast/Fancy), `graphics: GraphicsSetting` (Fancy; Fast = the mesher's `graphics_fast`), `smooth_lighting: u8` (the existing `SMOOTH_LIGHTING` default 2), `anaglyph: bool` (false; recorded no-op — a DIVERGENCES record), `fullscreen: bool` (false), `music_volume: f32`, `sound_volume: f32` (1.0) and the nine category volumes `[f32; 9]` (defaults 1.0; the category order pinned to `SoundCategory`'s), `mouse_sensitivity: f32` (0.5), `invert_mouse: bool` (false), `smooth_camera: bool` (false), `chat_opacity: f32` (1.0), `chat_scale: f32` (1.0), `chat_width: f32` (1.0), `chat_height: f32` (1.0), `chat_colors: bool` (true), `chat_mode: u8` (0), `skin_parts: u8` (0x7F), `language: String` (`en_US`), `force_unicode: bool` (false), `last_server: Option<String>`, `auto_jump`/`difficulty-lock` corners as the source carries (derive; drop with a record what v1 has no machine for).
  - `impl GameSettings { pub fn load(path) -> Self; pub fn save(&self, path) -> io::Result<()>; pub fn view_distance_chunks(&self) -> u8; pub fn far_plane(&self) -> f32; pub fn set_gui_scale_index(&mut self, index: u8); }` — `load` is total: unknown keys ignored, malformed lines skipped, out-of-range values clamped (each rule tested), a missing file is defaults; `save` writes the file's own format deterministically (sorted keys, one per line — the format is ours, D11: `options.toml`, vanilla's `options.txt` never read or written).
  - `oxide_game::settings::ParticleSetting` (All/Decreased/Minimal) and `CloudSetting` (Off/Fast/Fancy) — the screens' own enums, pinned to the source's option order.
  - `VIEW_DISTANCE_RANGE: (u8, u8)` — derived in-task from the source and confirmed on the rig (Decision 12; the value the rig shows governs, pinned by a test; the plan does not pre-pin it): the enum base is 2–16 and the 64-bit constructor raises the max to 32 when the heap is ≥ 1 GB (`GameSettings.java:195-204`) — expect 32 on this 64-bit rig; a shown 32 reconciles against spec D15's "to 16" at the close (record).
- Consumes: nothing at runtime; `oxide-proto-v47`'s `ClientSettings` shape for the wire mapping (a `to_client_settings(&self) -> ClientSettings` helper pinned by test).

- [ ] **Step 1: Defaults and ranges (RED).** Every default asserted against the re-read source initialisers; every range asserted at its ends and one past each (clamped). Run: `cargo test -p oxide-game settings` — fails to compile.
- [ ] **Step 2: Implement the type.** Run — green.
- [ ] **Step 3: Load/save suite (RED→green).** Round-trip: save→load equality for a fully non-default instance; a hostile file (garbage line, unknown key, wrong type, out-of-range, empty file, missing file) each lands as defaults/clamps with no panic; the file's formatting is stable across two saves. Run — green.
- [ ] **Step 4: The wire mapping.** `to_client_settings` pins: view distance → the C15 field, chat mode/colours, skin parts byte; the mapping round-trips. Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the game settings store (game)
```

**Verification:** `cargo test -p oxide-game`; `cargo fmt --all --check`; `cargo clippy -p oxide-game --all-targets -- -D warnings`.

---

### Task 4: The renderer's settings surfaces (render side)

**Goal:** The renderer can be driven from the settings store mid-session: GUI scale through `set_gui_scale`, brightness through the lightmap's gamma, particles through the pass's cap, clouds and graphics through the mesher/pass flags, and the ignored GPU cases pin each new input. (The window-level fullscreen and the client-side apply path are Task 14's and Task 28's.)

**Files:**
- Modify: `crates/oxide-render/src/renderer.rs` — the settings setters (`set_brightness`, `set_cloud_setting`, `set_graphics`, `set_smooth_lighting` beside `set_gui_scale`), the resize path's scale re-derive.
- Modify: `crates/oxide-render/src/lightmap.rs` — the brightness/gamma input (the gamma term derives from the source's own setting — `GameSettings.gammaSetting` at `GameSettings.java:171`, consumed in `EntityRenderer`'s lightmap update at `:1005-1047`; cite in-task), replacing the fixed curve.
- Modify: `crates/oxide-render/src/sky.rs` — the cloud setting's arms (Off/Fast/Fancy around the existing fast path).
- Modify: `crates/oxide-render/src/gui/*` (the text/sprite passes — wherever the current fixed scale factor lives, `hud.rs`'s `scaled_resolution` callers) — the scale-factor plumbing for brightness-free GUI (none expected; verify) and the pointer-scaling hook the client consumes.
- Test: `crates/oxide-render/src/renderer.rs` / `lightmap.rs` — unit pins (gamma ladder, scale re-derive at a resize).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored GPU cases: a brightness change alters a sampled lightmap texel; cloud Off removes the layer; Fast shows the flat layer.

**Interfaces:**
- Produces: `Renderer::set_brightness(&mut self, mode: u8)`, `Renderer::set_clouds(&mut self, setting: CloudSetting)`, `Renderer::set_graphics(&mut self, fast: bool)`, `Renderer::set_smooth_lighting(&mut self, mode: u8)`; the resize path emits the recomputed scaled resolution; `Renderer::scaled_resolution()` stays the one reader. The `wgpu` surface present-mode setter for VSync (`set_vsync(&mut self, on: bool)` — Fifo on, the fallback pair off).
- Consumes: Task 3's enums (`oxide-game`'s `ParticleSetting`/`CloudSetting` — the renderer must not import `oxide-game` (no edge): the enums' shared home is the decision: they live in `oxide-game` and the client downcasts to the renderer's own `u8`/bool setter parameters, pinned here). **Decision:** `oxide-render`'s setters take primitives (u8/bool) so the crate graph is untouched; the client maps.

- [ ] **Step 1: Unit pins (RED).** The gamma ladder (mode → the source's gamma value table); the scale re-derive at two window sizes; the present-mode mapping table. Run: `cargo test -p oxide-render renderer` — red.
- [ ] **Step 2: Implement the setters.** Run — green.
- [ ] **Step 3: Ignored GPU cases (RED→green).** The brightness texel case; the cloud Off/Fast cases; the smooth-lighting texel case (if the mesher owns AO — route per where the flag lands; the case asserts the composed output). Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-render --test pipeline_headless -- --ignored brightness`, then the cloud and smooth-lighting prefixes.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the renderer's settings surfaces (render)
```

**Verification:** the unit pins; the named ignored cases green with the store env; the full gate; the touched ignored prefixes re-run green at the final bytes.

---

### Task 5: The session's settings and weather state

**Goal:** The session consumes the new packets: weather accumulates and eases per tick and publishes `WeatherChanged`; sound effects, particles and global entities publish their events; the C15 re-send lands for view-distance changes; and the session settings come from the store instead of the fixed constants.

**Files:**
- Modify: `crates/oxide-game/src/session.rs` — the new decode arms (`0x29`, `0x2A`, `0x2B`, `0x2C`), `WeatherState` (rain/thunder current + target + easing), the `ClientEvent` variants, the `InputEvent::SetViewDistance` path (C15 re-send), the `mesh_context` settings (graphics/smooth-lighting from the store's values instead of the two constants — the constants become defaults until the client pushes).
- Test: `crates/oxide-game/tests/session_replay.rs` — the weather easing across ticks; the C15 re-send; the Keepalive discipline standing test stays untouched.

**Interfaces:**
- Produces:
  - `ClientEvent::WeatherChanged { rain: f32, thunder: f32 }` — published when the eased values change at a tick (the easing rule: each value moves toward its target at the source's per-tick rate — `World.java`'s `updateWeather`; derive the rate constant in-task, pin it).
  - `ClientEvent::Particle { particle_id: i32, long_distance: bool, pos: [f32; 3], offset: [f32; 3], speed: f32, count: i32, data: Vec<i32> }` — one per 0x2A (the client's pump forwards it into the particle engine).
  - `ClientEvent::SoundEffect { name: String, pos: [i32; 3], volume: f32, pitch: u8 }` — one per 0x29.
  - `ClientEvent::GlobalEntity { entity_id: i32, kind: u8, pos: [i32; 3] }` — one per 0x2C; the entity registers for removal (the standing entity store path) and the client's lightning leg consumes kind 1.
  - `oxide_game::session::WeatherState` — `{ rain, thunder, target_rain, target_thunder }` with `tick(&mut self)`, published through the event; the session also folds rain into the sky-side consumers it already feeds (nothing new — the client reads the event).
  - `InputEvent::SetViewDistance { chunks: u8 }` — sends C15 with the new view distance beside the standing fields (the source's own behaviour on a slider change derives in-task; if 1.8 only applies it at reconnect, the source's path is the pinned rule and the plan's Decision 12 wording records it).
- Consumes: Task 2's structs; Task 3's `GameSettings`; the standing `report`/event plumbing.

- [ ] **Step 1: Weather tests (RED).** Fixture-driven: begin-rain (**1**) eases rain 0→1 at the pinned rate over N ticks; end-rain (**2**) eases 1→0; reason 7 names the rain strength and 8 the thunder strength (the source snaps both — `setRainStrength(f)` / `setThunderStrength(f)` at `NetHandlerPlayClient.java:1414-1421` — and the session eases toward them as fade targets per the task interface); each eased change publishes exactly one event. Run: `cargo test -p oxide-game weather` — red.
- [ ] **Step 2: Implement weather.** Run — green.
- [ ] **Step 3: Event tests (RED→green).** One event per packet with values asserted; the global entity registers and its 0x13 removal lands; a sound name decodes without lookup failure. Run — green.
- [ ] **Step 4: The C15 re-send (RED→green).** `SetViewDistance` writes 0x15 with the whole settings payload (the byte shape pinned against the existing writer suite) on change only. Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the session's weather, effect events and view distance (game)
```

**Verification:** `cargo test -p oxide-game`; `cargo fmt --all --check`; `cargo clippy -p oxide-game --all-targets -- -D warnings`; the keepalive case re-run.

---

### Task 6: The sound index

**Goal:** `oxide-assets` parses the store's `sounds.json` into `SoundIndex` — the 246 events the store's copy carries, each with its category, file list, flags and weights, hostile-input capped — so the audio engine resolves events without touching paths.

**Files:**
- Create: `crates/oxide-assets/src/sound.rs` — `SoundIndex`, `SoundDef`, `SoundFile`, the parser.
- Modify: `crates/oxide-assets/src/lib.rs` — module declaration and re-exports.
- Test: `crates/oxide-assets/src/sound.rs` + `crates/oxide-assets/tests/sound_index.rs` — synthetic JSON fixtures (built in-test), hostile cases.
- Test (ignored, store-gated): `crates/oxide-assets/tests/sound_index.rs` — the real `sounds.json` from the store parses; event counts recorded (observed on this machine 2026-10-11: 246 events; categories `ambient` 8 / `weather` 2 / `player` 16 / `neutral` 86 / `hostile` 76 / `block` 31 / `record` 19 / `master` 1 / `music` 7, counted over the index object at `assets/indexes/1.8.json` — a 734-object trimmed index, the same copy PrismLauncher's tree carries; `extracted/1.8.9/assets/` holds no sounds, so the index object is the store's only copy); a spot set of pinned names resolve (`music.game` streamed; `ambient.weather.rain` present; `records.13` streamed).

**Interfaces:**
- Produces:
  - `oxide_assets::sound::SoundIndex { events: BTreeMap<String, SoundDef> }`; `SoundDef { category: String, stream: Option<bool>, sounds: Vec<SoundFile> }`; `SoundFile { name: String, volume: f32, pitch: f32, weight: i32, stream: Option<bool>, attenuation_distance: Option<i32> }` — the fields the 1.8 format carries, both stream positions included (the store's copy shows `stream` on the event level for none of its 246 and on the individual file objects for `music.*`/`records.*` — `music.menu` and `records.13` carry `{"name": ..., "stream": true}` per file; the effective rule is file-value-else-event-value-else-false, pinned by the store test against the real file; derive the exact key set against `docs/research/render-parity-survey.md` §6, the ignored store test pins the real file's counts).
  - `SoundIndex::parse(bytes) -> Result<SoundIndex, SoundError>` — caps: `MAX_SOUND_EVENTS = 4096`, `MAX_SOUND_FILES = 256` per event, the string caps; weights clamped positive; a `minecraft:` path resolves to the store's relative path untouched (the paths in the file are already store-relative, test-pinned on a sample).
  - `SoundIndex::pick(&self, name: &str, roll: f32) -> Option<&SoundFile>` — the weight-based pick (the file's own cumulative-weight rule, pinned by deterministic rolls).
- Consumes: nothing at runtime; reads the store file/object bytes at load (the caller's job — this type never reads the filesystem).

- [ ] **Step 1: Parser tests (RED).** Synthetic fixtures: two events, one streamed with two files, weights 1/2 picking deterministically at rolls 0.32/0.34; caps: 257 files refuse, an event name past the cap refuses, a malformed JSON refuses cleanly. Run: `cargo test -p oxide-assets sound` — red.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: The store-gated test.** Real file parses (its home is the asset-index object `minecraft/sounds.json` — 37,731 B, verified present on this machine; the extracted overlay holds no sounds, so the object path is the real one and it stands as the recorded resolution order: extracted path when present, else the object); events count recorded in the test's output; `music.game`, `ambient.weather.rain`, `records.13` resolve with their flags. Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-assets -- --ignored sound`.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: parse the sound index from the store (assets)
```

**Verification:** `cargo test -p oxide-assets`; the ignored store case green with the env; `cargo fmt --all --check`; `cargo clippy -p oxide-assets --all-targets -- -D warnings`.

---

### Task 7: The particle engine

**Goal:** `oxide-game` owns the particle simulation: the pooled engine with the vanilla behaviour table, `ParticleSetting` reduction, deterministic stepping, and the wire spawn path — the renderer reads `snapshot()`, F3's count reads `stats()`.

**Files:**
- Create: `crates/oxide-game/src/particles.rs` — `ParticleEngine`, `Particle`, the behaviour table, the spawn paths, the statistics.
- Create: `crates/oxide-game/src/particle_world.rs` — the `ParticleWorld` query trait (block material, collision, light, height) the engine reads; the session-side adapter lives beside it.
- Modify: `crates/oxide-game/src/lib.rs` — module declaration.
- Test: `crates/oxide-game/src/particles.rs` — inline unit tests (stepping, lifetimes, caps, setting reduction, deterministic fixtures).
- Test: `crates/oxide-game/tests/particles.rs` — the engine suite (spawn from a wire event; tick to expiry; the cap eviction order).

**Interfaces:**
- Produces:
  - `oxide_game::particles::ParticleEngine` — `new(limit)`, `set_limit(setting: ParticleSetting)`, `spawn_wire(particle_id, long_distance, pos, offset, speed, count, data, viewer: [f32; 3], world: &ParticleWorld)` (viewer = the reduction's distance-cull input; the degrade roll rides the engine's own `rand`), `spawn_local(ParticleSpawn)` (the dig/footstep/splash sites' entry), `tick(&mut self)`, `snapshot(&self) -> Vec<ParticleFrame>` (the renderer's input), `stats(&self) -> ParticleStats { count, visible }` (F3's `P:`).
  - `ParticleFrame { pos: [f32; 3], prev_pos: [f32; 3], age: f32, size: f32, colour: [f32; 4], tex: ParticleTex, light: u8 }` — `ParticleTex` is `Cell(u8)` (particles.png cell), `Atlas(u16)` (block atlas sprite index), or `ItemAtlas(...)` for item crack (route per the survey: item cracks sample the item's icon — the M5 registry's own path); the light byte rides the sampled block light so the pass can draw lit.
  - `ParticleSetting` reduction pinned: the derive homes are `EffectRenderer`'s per-list cap (4000 per `(fxLayer, alpha)` list — 8 lists; `EffectRenderer.java:146-154`) and the spawn-side rules (`RenderGlobal.spawnEntityFX:2073-2092`: `Decreased`'s one-in-three degrade and the 256-block² distance cull; `EntityRenderer:1533-1539`: the rain count's halve/zero). Derive the exact formulas and pin sampled values. A single flat pool is acceptable only as a recorded simplification with its divergence trigger pinned; otherwise eviction follows the per-list caps. Deterministic: the engine holds its own `rand` instance seeded per test.
  - The behaviour table: one row per id 0–41 (friction, gravity, collision, lifetime formula, argument use) — the derive source is the per-particle classes (`EntityFX` + subclasses, `refs/_src/MCP-919/.../client/particle/`); the table itself is a `[ParticleBehaviour; 42]` with the per-row cites; the task's RED fixtures pin a representative row per class (a smoke rise+fade, a flame's shrink-and-fade with its full-brightness ramp, a splash arc, a block-dust tint, a note's rise; the water-interaction class is pinned via a bubble's rise and its death when the block at its position is not water (corrected 2026-10-12, M6 Task 7's pre-flight)).
- Consumes: `docs/research/render-parity-survey.md` §4.6; `oxide-world`'s block query (the material/light/collision the particles read — the engine takes a `ParticleWorld` trait to keep tests headless).

- [ ] **Step 1: Stepping tests (RED).** Smoke: rises, fades on age; flame: shrinks with age (scale `1 - f*f*0.5`) and ramps to full brightness; bubble: rises and dies when its block is not water; splash: arcs with gravity; a list at its cap evicts oldest-first (or the recorded single-pool simplification's rule); `Decreased`'s one-in-three degrade, its 256-block² cull and `Minimal`'s rejection — sampled values pinned. Run: `cargo test -p oxide-game particles` — fails to compile.
- [ ] **Step 2: Implement the engine and table.** Run — green.
- [ ] **Step 3: Wire spawn (RED→green).** A 0x2A-shaped spawn with count>1 emits count particles with deterministic offsets; `ITEM_CRACK`/`BLOCK_CRACK` data args land as their tex kinds; a `BARRIER` (unknown-id) spawn draws nothing and counts. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the particle engine (game)
```

**Verification:** `cargo test -p oxide-game`; `cargo fmt --all --check`; `cargo clippy -p oxide-game --all-targets -- -D warnings`.

---

### Task 8: The audio engine, the mixer and the sound registry

**Goal:** The sound engine plays: a cpal output stream over a lewton decoder with the category mixer, the clamps, the randomisation, the attenuation and the streaming split, reached through the wire sound event and the local call sites — with the mixer's law unit-pinned and a silent fallback.

**Files:**
- Create: `crates/oxide-game/src/audio/mod.rs` — `AudioEngine`, `AudioCommand`, the device shell, the mixer.
- Create: `crates/oxide-game/src/audio/mixer.rs` — the pure mixing core (voice law, resampler, fade).
- Create: `crates/oxide-game/src/audio/decode.rs` — the lewton path + the sample cache.
- Modify: `crates/oxide-game/src/lib.rs`; `crates/oxide-game/Cargo.toml` (the three new deps); the workspace `Cargo.toml`; `Cargo.lock` (regenerated with the deps' resolution). `deny.toml` gains the licences only if the resolver shows a licence outside the current allow-list — checked in Step 5, not assumed.
- Modify: `crates/oxide-game/src/session.rs` — the 0x29 handler reports the `SoundEffect` event (Task 5's shape); the engine itself is the CLIENT's field (Decision 4), so the session thread stays audio-free and the client's tick drains the channel into the engine.
- Test: `crates/oxide-game/src/audio/mixer.rs` — inline unit tests (the voice gain law; the fade; the resampler on a synthetic ramp).
- Test: `crates/oxide-game/tests/audio.rs` — the engine over the silent device: category volume changes the mix; pitch clamps [0.5, 2.0]; a zero normalised volume skips (`SoundManager`'s `f2 == 0.0F` rule) while 0.005 plays at near-zero gain; a 16-block attenuation curve's sampled values; the streamed path stub.

**Interfaces:**
- Produces:
  - `oxide_game::audio::AudioEngine` — `new(index: Arc<SoundIndex>, cache_dir: Option<PathBuf>, device: Option<Box<dyn AudioDevice>>)`; `play(event: PlaySound { name, category_hint, pos: Option<[f32; 3]>, volume, pitch, listener: [f32; 3], yaw: f32 })`; `set_category_volume(category: SoundCategory, volume: f32)`; `set_master_volume(f32)`; `tick(&mut self, listener: ListenerPose)`; `stop_all()`; `debug_dump_start(path)`/`stop()` (Decision 13's `--audio-dump`). All play calls are channel sends (bounded channel, overflow drops oldest — never blocks).
  - `oxide_game::audio::SoundCategory` — nine variants pinned to the source's order (`MASTER…AMBIENT`, survey §6); `as_index()`.
  - The law (pinned): effective gain = category volume × master × event volume × attenuation; attenuation from the source's algorithm — derive in-task from `SoundManager`/`positionSound` (16-block default, `attenuation_distance` override, the linear/squared rule the source applies — derive and pin exact sampled values); pitch = clamp(call pitch × event pitch × randomisation(1±0.1 where the source randomises) — the per-call rule: the WIRE's 0x29 pitch arrives as the packet's own value (63→1.0 scaling derives in-task) and the call-site randomisation is the SPAWNER's job (hurt etc. — the local call sites in Task 9); final clamp [0.5, 2.0]; volume clamp [0, 1]; only a zero normalised volume is skipped (`SoundManager`'s `f2 == 0.0F` — a tiny non-zero volume plays).
  - The device shell: `trait AudioDevice { fn open(...) -> Option<Box<dyn AudioDevice>>; fn write(&mut self, frame_buffer: &mut [f32]) -> ...; }` — the cpal implementation and a silent one; the engine takes whichever opens (the silent fallback records one log line). The ignored integration test (store-gated): open the real device when present (`--ignored`, tolerable on this desktop), else skip with a record.
  - Streaming: `records.*`/`music.*` stream through the same mix with the 64-block attenuation for records (the index's `attenuation_distance`) and full-volume for music; a streamed voice decodes incrementally into a ring (lewton's `inside_ogg` reader reads ahead on the audio thread — the cache holds at most the active stream).
- Consumes: Task 6's `SoundIndex`; `rand`; `crossbeam-channel` (already a dependency).

- [ ] **Step 1: The mixer law (RED).** Pure-function tests: a voice at 1 block vs 15 vs 17 under a 16-block attenuation; category 0.5 halves; volume 0.0 skips and 0.005 plays (the source's zero-test, not a threshold); pitch 2.5 clamps to 2.0, 0.4 to 0.5; the interleaving resampler's ramp test. Run: `cargo test -p oxide-game audio` — fails to compile.
- [ ] **Step 2: Implement the mixer.** Run — green.
- [ ] **Step 3: The engine over the silent device (RED→green).** Play through a scripted sequence; the rendered PCM (dumped) is non-silent, correct channels, and its RMS changes with the category volume; the bounded channel never blocks a stalled consumer. Run — green.
- [ ] **Step 4: The device path.** cpal stream opens on this desktop (manual run, recorded), the silent fallback covers headless; the ignored integration test lands. Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-game -- --ignored audio` (records skip/open).
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the sound engine, mixer and registry (game)
```

**Verification:** `cargo test -p oxide-game`; the PCM dump receipt; `cargo deny check` clean (with the three new crates' licences handled per Step 5); `cargo +1.85.0 check --workspace --all-targets` clean (the new deps' MSRV fit); `cargo fmt --all --check`; `cargo clippy -p oxide-game --all-targets -- -D warnings`.

---

### Task 9: The music ticker and the local sound sites

**Goal:** Music starts and stops on the source's schedule (a fade only if its source site is found — see the interface note), and the world's local sounds speak: block break/place, footsteps, splash, hurt, pickup, level-up — with the call-site randomisation the source applies, routed through the engine.

**Files:**
- Create: `crates/oxide-game/src/audio/music.rs` — the `MusicTicker`.
- Modify: `crates/oxide-game/src/session.rs` (or the interaction taps' event side) — the local sound emissions: dig complete/break → the block's `dig.*`/`step.*` event (the block sound table: the M2 behaviour table's sound group — the `Block`'s `stepSound`; derive the group-to-event map in-task and pin a sample), place → the place event, splash on water entry, hurt on the own-player damage, pickup on the collect, level-up on XP level change.
- Modify: `crates/oxide-client/src/main.rs` — the client's sound pump: the `SoundEffect` event → engine, tick → listener pose, music ticker drive, category volumes from settings.
- Test: `crates/oxide-game/src/audio/music.rs` — the ticker schedule (seed-pinned: the 100-tick initial delay and the 12000–24000-tick pick range — `MusicTicker.java`; no fade pin until a source site is found for one — the survey's 100-tick fade claim is uncited and must not be pinned blind).
- Test: `crates/oxide-game/tests/audio.rs` — the call-site rules (the hurt randomisation range; the step's own pitch rules as derived).

**Interfaces:**
- Produces:
  - `MusicTicker` — `tick(&mut self, kind: MusicKind, volume_scale: f32, engine: &AudioEngine)`; `MusicKind` = Game/Creative/Menu/End per dimension and mode; the schedule: initial delay 100 ticks, a pick every 12000–24000 ticks (re-derive `MusicTicker.java`'s constants and pin). A fade-in/out exists ONLY IF the task first FINDS its source site (search `client/audio/` and the paulscode wrapper — the survey's "100-tick fade" currently has no source; with no site the ticker starts/stops without a fade and `docs/research/render-parity-survey.md` §6's claim is corrected in the same pass); `stop()` follows the same rule.
  - The local call sites' rules: hurt/death `(rand − rand) × 0.2 + 1.0` (the survey's cite — `EntityLivingBase`), child sounds 1.5× (as the source; own-player only at v1 — the own-player scope recorded), pickup `random.pop` at 0.2 pitch spread derives in-task.
- Consumes: Task 8's engine; the block behaviour table's sound groups.

- [ ] **Step 1: Ticker tests (RED).** A seeded tick stream: play lands within the window; the fade ticks count; a music stop on mode change. Run: `cargo test -p oxide-game audio::music` — red.
- [ ] **Step 2: Implement the ticker.** Run — green.
- [ ] **Step 3: Call-site tests (RED→green).** The randomisation ranges' bounds (seeded); the block group map's sample rows. Run — green.
- [ ] **Step 4: The client pump.** Wired; a manual rig check records music actually playing on the dev machine (log line + PCM dump non-silence).
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the music ticker and the local sound sites (game)
```

**Verification:** `cargo test -p oxide-game`; the PCM receipts; `cargo fmt --all --check`; `cargo clippy -p oxide-game -p oxide-client --all-targets -- -D warnings`.

---

### Task 10: The sky, weather and fog carries

**Goal:** The sky closes its carried terms — the sunrise/sunset band, the rain dim, the stars' full chain, the void floor — and the fog colour reads the weather and the consumed clock, with the water-fog light-read nuance adjudicated against the reference.

**Files:**
- Modify: `crates/oxide-render/src/sky.rs` — the band (the 16-segment disc, its transforms and gradient — `RenderGlobal`'s sunrise/sunset cites; the existing `:1253-1292`/`WorldProvider:151-175` cites re-read), the stars' alpha chain (`1 − rain`), the horizon tuple growth, the void-fog floor per the carried note.
- Modify: `crates/oxide-render/src/fog.rs` — the rain darkening term in `fog_colour` (the source chain's rain branch, `EntityRenderer.java:1803+` region — derive), and the adjudication of the carried "fog colour's time term" row: the current code already folds `celestial_angle(time_of_day)` into `surface_fog` (`fog.rs:219`) — the task RE-DERIVES the carried claim against the source and rules it: if the term is present (as read at planning), the carry closes as an already-landed record with the evidence line; if a sub-term is genuinely missing (e.g. the rain/thunder blend), that term is the task's work. Cost-if-wrong: a wrong "already present" ruling ships a missing term.
- Modify: `crates/oxide-client/src/main.rs` — the `SkyParams` construction (the M2 stand-in at `:1392`) now feeds `rain_strength`/`thunder_strength`/the horizon from the session's `WeatherChanged`; the water-fog eye-light read (the carried nuance: the reference reads the feet-neighbour max, the port the view block's own — derive the exact reference window in-task and fix or pin the ruling).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored cases: the band's presence and gradient direction at a sunset celestial angle; the stars' rained alpha; the fog colour's rain term values at pinned inputs.
- Test: `crates/oxide-render/src/fog.rs` — unit pins for the new term's ladder.

**Interfaces:**
- Produces: `SkyParams { …, rain_strength: f32, thunder_strength: f32, horizon: Option<[f32; 4]> }` (the horizon tuple's shape: the colour fade's four values — derive; the M4 row-24 carry said the horizon is a superflat artefact, but the M5 review's row-24 and the sunrise band need the band's own fade — RE-DERIVE which claim is right at the source: the "horizon" the carried row meant was `renderSky`'s below-horizon quad for `eyeY < horizon`, already shipped; this task's sunrise band is the separate `calcSunriseSunsetColors` disc. Record the disambiguation in the plan's coverage).
  - `fog_colour`'s new signature if the rain term needs it (parameter addition, callers in `main.rs` updated).
- Consumes: Task 5's `WeatherChanged`; the standing sky pass.

- [ ] **Step 1: The carry re-derivations FIRST (record).** Grep + read: the fog time term (already-present ruling); the horizon item (disambiguation above); the water-fog light read (derive the reference window first-hand; fix-or-pin). Write the rulings into the task's evidence dir before code.
- [ ] **Step 2: Band + stars cases (RED).** The sunset-angle band case; the rained-stars alpha case; each red before its implementation.
- [ ] **Step 3: Implement the band, the dim, the fog term.** Run — green on the named cases.
- [ ] **Step 4: The client wiring.** `SkyParams` fed from weather; the water-fog read fixed or pinned per Step 1's ruling; the affected ignored fog cases re-run (the displaced-eye fixtures' standing rule: after touching the camera/fog, `cargo test -p oxide-render --test pipeline_headless fog -- --ignored`).
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the sky band, the weather dim and the fog terms (render)
```

**Verification:** the named ignored cases + the fog prefix green with the store env; the full gate; the carry rulings' evidence files.

---

*(continued in part 3 — Tasks 11–21)*

### Task 11: The weather draw and the particle pass

**Goal:** Rain and snow draw as the source's columns with splash particles on impact, the particle engine's pool draws (billboards, block batches, item cracks) with the atlas and cells, and the fragment covers the F3 `P:` count's visible set — all as its own pass with pinned GPU cases.

**Files:**
- Create: `crates/oxide-render/src/weather_pass.rs` — the rain/snow columns (the 64×256 rain and snow sheets, the per-frame UV offset, the camera-following spawn box, the 0.5·f4 descent; cites `EntityRenderer.renderRainSnow:1594+`).
- Create: `crates/oxide-render/src/particle_pass.rs` — the billboard and atlas-batch draws (the 8×8 particles sheet; the atlas path for block break/dust/footprint with the block's colour; item crack through the item icon).
- Modify: `crates/oxide-render/src/renderer.rs` — the two passes in the frame order (the source's own position: weather after entities, before the hand; particles after terrain — derive the exact order from `EntityRenderer`'s sequence and pin it).
- Modify: `crates/oxide-client/src/main.rs` — the draw-list feed (the engine's `snapshot()` into the pass; the weather params).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored cases: a rain frame's column pixels at a pinned camera; a splash particle's cell at a pinned pose; a block-crack particle's atlas colour; the particle cap's visible count.
- Test: `crates/oxide-game` + `crates/oxide-render` — the splash spawn rule unit-pinned (a particle under falling rain in an open column spawns `WATER_DROP` at the source's probability — derive; a covered column spawns none).

**Interfaces:**
- Produces: `WeatherDraw { rain: f32, thunder: f32, camera: …, light: … }` and `ParticleDraw { frames: &[ParticleFrame] }` as the renderer's inputs; the splash rule's home: the client's weather tick (it reads the world's heightmap — the session's own column data through the standing world view) and spawns into the engine.
- Consumes: Tasks 7 and 10; the standing atlas/text machinery.

- [ ] **Step 1: Column case (RED).** A clear frame vs a full-rain frame at a pinned pose: the column pixels count positive, the streak length matches the source's quad extent; red first (no pass). Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-render --test pipeline_headless -- --ignored rain`.
- [ ] **Step 2: Implement the weather pass.** Run — green.
- [ ] **Step 3: Particle cases (RED→green).** The cell case, the atlas-colour case, the item-crack case; each red before its draw lands. Run the `particle` prefix.
- [ ] **Step 4: The splash rule tests.** Probabilistic spawn at the pinned rate (seeded); covered cells spawn none. Run: `cargo test -p oxide-game splash`.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the weather columns and the particle pass (render)
```

**Verification:** the named ignored cases green; the full gate; the touched ignored prefixes re-run at the final bytes.

---

### Task 12: The overlays

**Goal:** The overlay set completes: the pumpkin blur when worn in first person, the portal overlay with its alpha ramp and the nausea wobble, the fog leg verified and recorded (the source couples no fog to the overlay draws; the rain/thunder fog landed with the sky band — the lava-eye arm verified), and the block-in-hand tint when the camera is inside an opaque block — the carried watch item (the rows-440+ R/G lift) is re-measured on the current frames and ruled.

**Files:**
- Modify: `crates/oxide-client/src/view.rs` (+ the hud draws, `HudPass::TexturedRect`, following the water-overlay precedent) — the pumpkin blur quad (the worn helmet `armorItemInSlot(3)` is the pumpkin block and `thirdPersonView == 0`; `GuiIngame.java:145-150`) and the portal quad with its `timeInPortal` alpha ramp (`GuiIngame.java:152-160`, `:1003-1031`). The confusion wobble is not a quad: it is the camera's rotate/scale/rotate (`EntityRenderer.java:782-796`) on the camera-transform path.
- Modify: the first-person/hud draw path (`ItemRenderer.renderOverlays` order — before the water and fire overlays; `ItemRenderer.java:421-459`, `:472-495`) — the block-in-hand tint (the eye-inside-opaque-block condition; the source's tint `0.1,0.1,0.1,0.5`, mirrored U), not folded with `world_overlay.rs`'s scene-pass outline/crack draws.
- Modify: `crates/oxide-client/src/main.rs` — the state feed: the worn helmet kind (pumpkin block), the portal/confusion counters from the own entity's effect set (the M5 `Effects` event carries them), the `timeInPortal` accumulation (the source's rule: `+0.0125` while in a portal, `+1/150` under confusion beyond 60 ticks, `−0.05` otherwise; `EntityPlayerSP.java:732-774`), the block-in-hand condition.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored: pumpkin worn/not (+ the third-person gate); the portal alpha at two `timeInPortal` values; the block-in-hand tint's pixel.
- Test: the `timeInPortal` accumulation unit beside its implementation (a portal entry ramps it; leaving decays it — the source's rule above, with the seam ruling in the dispatch).

**Interfaces:**
- Produces: the overlay state struct the client feeds (`OverlayState { pumpkin: bool, portal_time: f32, confusion: f32, inside_block: bool, … }` — the shape the hud and camera consumers share; the wobble's values are the source's rotate/scale pair (`EntityRenderer.java:782-796`), driven by the frame counter and `partialTicks`).
- Consumes: the M5 effect state; the standing overlay pass.

- [ ] **Step 1: Cases (RED).** Each new draw's case red first. Run the `overlay`-prefixed ignored cases.
- [ ] **Step 2: Implement the draws.** Run — green.
- [ ] **Step 3: The carried watch item.** Re-measure the rows-440+ R/G lift on the CURRENT frames (the closesmoke2 re-bases; the measurement script lands in the task's evidence): if it survives the weather work, record the finding with its fix shape for the close's ruling (it mints its own round only on the close's go); if closed, record the refutation with numbers. (This is the M5 review's `refs/m5-task-28/final-review.md` §1c row 3's trigger: "if it survives M6's weather/light work".)
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the pumpkin, portal and block-in-hand overlays (render)
```

**Verification:** the cases green; the watch item's measurement recorded; the full gate.

---

### Task 13: The combat, digging and sprint particle taps

**Goal:** The local particle spawn sites land: block-break and digging particles from the dig path, block-crack crumbs while sprinting, bubbles and splash on water entry, and the crit emitters on combat — each at the source's own spawn rule. (Landing dust has no local spawn site — `EntityLivingBase.updateFallState:233-253` is server-side and arrives via the wire; and placing spawns nothing — sound only.)

**Files:**
- Modify: `crates/oxide-game/src/interaction.rs` and the session-side staging points — the dig hit tap (one `EntityDiggingFX`-shaped spawn at the 0.1-inset face cell, velocity ×0.2, scale ×0.6; `EffectRenderer.addBlockHitEffects:395-442`) and the destroy tap (64 spores at `(j/k/l + 0.5)/4` per cell, non-air only; `addBlockDestroyEffects:369-393`); the water-entry fan-outs (`1 + width*20` each of bubble and splash; `Entity.handleWaterMovement:1111-1129`). Placing spawns nothing — record-nothing with the source cited.
- Modify: `crates/oxide-game/src/session.rs` — the sprint crumbs (one `BLOCK_CRACK` per tick while sprinting and dry at `(posX ± rand·width/2, minY + 0.1, posZ ± rand·width/2)`, velocity `(-motionX·4, 1.5, -motionZ·4)`, data = the stood-on cell's state id; `Entity.createRunningParticles:1172-1188`) and the crit emitters (the S0B 4/5 animation arm plus the local attack path; 16 tries in a unit sphere, ids 9/10; `EntityParticleEmitter`). `FOOTSTEP` (id 28) has no spawn site in the source — it is registration-only (`EffectRenderer.java:90`).
- Landing needs no tap: the own-player local dust site does not exist (the `fallDistance` arm is `!isRemote`-gated) — the wire path is Task 7's.
- Test: `crates/oxide-game/tests/particles.rs` — extension: a dig tick spawns the pinned count/direction; the destroy fan-out's 64 non-air spores; the sprint gating (sprinting and dry only, one per tick); the water-entry counts; the crit emitter's 16 tries and ids (landing excluded — server-side).

**Interfaces:**
- Produces: the spawn call sites against Task 7's `spawn_local`; each rule's constants (counts, half-block spread) derive in-task and pin.
- Consumes: Task 7; the M3 dig path.

- [ ] **Step 1: Tests (RED).** Each site red first (`cargo test -p oxide-game dig_particles` class names derive from the module).
- [ ] **Step 2: Implement the taps.** Run — green.
- [ ] **Step 3: Gate and commit.** Run the full gate. Commit:

```
feat: the digging, combat and sprint particle taps (game)
```

**Verification:** the new cases; the dig/world suites re-run green; the full gate.

---

### Task 14: The settings application path

**Goal:** Changes made in any settings screen take effect immediately and persist: one client-side apply function pushes every field to its consumer (renderer, session, engine, particle engine, keymap, window), the store saves on change, and the client's startup reads the file before the first frame.

**Files:**
- Modify: `crates/oxide-client/src/main.rs` — the `SettingsState` owner (the `GameSettings` instance, the save-on-change writer, the apply path), the startup read, `spawn_session`'s settings source (the CLI `--render-distance` becomes the file's default only when the file is missing — the CLI flag's interaction pins: flag overrides file for this run, recorded), the fullscreen apply (winit), the VSync apply (Task 4's setter), the pointer-scaling hook (the cursor conversion at the screens' input edge: window px → frame units by the scale factor — the carried row 48's rule "mid-run GUI-scale change keeps the frame's unit" pinned as a test on the conversion function).
- Test: `crates/oxide-client/tests/settings_apply.rs` — the apply path's fan-out (a settings change writes each consumer's observed value; headless where possible), the cursor conversion's cases at scales 1–4, the save-on-change debounce.
- Test: `crates/oxide-client/src/main.rs` — parser tests for the CLI/file precedence.

**Interfaces:**
- Produces: `SettingsState` with `apply(&mut self, renderer, engine, particles, window, keymap)`; the save path (`<store>/options.toml`, the D11 format, written whole-file atomically — the store's own temp+rename discipline).
- Consumes: Task 3; Task 4; Tasks 8, 7.

- [ ] **Step 1: Apply tests (RED).** Each field's consumer write asserted; the precedence rule asserted; the cursor cases. Run: `cargo test -p oxide-client settings_apply` — red.
- [ ] **Step 2: Implement the path.** Run — green.
- [ ] **Step 3: Persistence round-trip.** Change → save → re-read → equal; a mid-session renderer rebuild keeps the values. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the settings application path (client)
```

**Verification:** `cargo test -p oxide-client`; the full gate; a manual run recording the file's creation and a restart keeping a changed value.

---

### Task 15: The menu framework, widgets and the main menu

**Goal:** The menu family's framework lands — the button/list/slider widgets from `widgets.png`, the panorama renderer, the splash text — and the main menu screen itself: logo, splash, the singleplayer entry (recorded inert per Decision 7), Multiplayer, Options, and the quit path.

**Files:**
- Create: `crates/oxide-client/src/screens/widgets.rs` — the widget set (button 152×20 slices with hover/disabled, the slider track/knob, the list background) and its draw/hit code over the standing text machinery.
- Create: `crates/oxide-client/src/render_menu.rs` (or the panorama inside `screens/menu.rs`) — the panorama cube: six 256×256 faces from the store, the slow rotation (the checklist's Gaps row: the rotation period is unquantified — MEASURE it on the vanilla side at the acceptance and pin the measured value; until then the task implements the cube and records the pending constant as a known limit — no blank threshold ships).
- Create: `crates/oxide-client/src/screens/menu.rs` — the main menu (`GuiMainMenu` layout: logo at the pinned position, the splash at 45° with the bounce — amplitude/period from the acceptance's measurement, the same pending-constant rule), the button set, the splash pick (seeded per launch; `texts/splashes.txt` read from the store).
- Modify: `crates/oxide-client/src/screens/mod.rs` — the new variants and the open/close rules (menus are pre-session states; the client's state machine gains the no-session screens — the main menu shows at startup and on disconnect-to-title).
- Test: `crates/oxide-client/tests/screens.rs` — the layout pins (button positions at the pinned scaled resolutions) and the splash pick.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored: a panorama frame's six-face sampling at two yaw values; the splash text's pixels; the button states' three renders.

**Interfaces:**
- Produces: `Widget` id/state set (button enabled/hover/dragging for sliders), the draw records (`HudDraw` extension for the widget sheet — the M5 GUI draw kinds grow `Sheet` rectangles); `Panorama { yaw: f32 }`; the menu screen's `ScreenState::MainMenu`.
- Consumes: the store's `gui/title/background/panorama_*`, `title/minecraft.png` (the logo — the version string draws beside it: `1.8.9` as the client's own compatibility line, the EXACT text derives from the source), `texts/splashes.txt`.

- [ ] **Step 1: Widget cases (RED).** A button renders; hover differs; disabled greys the text; a slider's knob follows its value (each an ignored GPU case + the unit layout pins). Run red.
- [ ] **Step 2: Implement the widgets.** Run — green.
- [ ] **Step 3: Panorama + splash cases.** The cube's two frames; the splash's angle (45° from the logo's right edge). Run — green; the rotation-period and bounce constants' acceptance deferral recorded.
- [ ] **Step 4: The main menu's wiring.** Buttons route (Multiplayer/Options open their screens; Singleplayer records inert; Quit exits). Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the menu framework and the main menu (client)
```

**Verification:** the cases green; the full gate; the screen's first live look recorded (main menu at startup).

---

### Task 16: The server list and ping

**Goal:** The multiplayer screen works: the saved server list (`servers.dat`-equivalent in our own file — the D11-equivalent rule), add/edit/delete, the live ping through the M1 status path (MOTD with §-codes, player count, the 5-bar latency icon), and Join connecting the session with the selected address.

**Files:**
- Create: `crates/oxide-client/src/screens/multiplayer.rs` — the list, the entry drawing, the add/edit sub-screen, the ping worker.
- Modify: `crates/oxide-client/src/screens/mod.rs` — the variant + the join path (the client's session-start flow gains a settings-supplied address; `last_server` persisted).
- Modify: `crates/oxide-client/src/main.rs` — the status-ping call path (the M1 `status` flow reused per server; a worker thread per refresh with a timeout; results as events).
- Test: `crates/oxide-client/tests/screens.rs` — the list's layout pins, the MOTD §-code rendering (through the standing styled-text path), the ping icon's level table (the checklist row for the bar thresholds — the 5-bar rule derives from `ServerListEntryNormal`/the ping math and pins).
- Test: `crates/oxide-client/tests/multiplayer.rs` — the server-file round-trip (host/port/name), hostile file handling.

**Interfaces:**
- Produces: `ServerList` (the file model + the screen), `ServerEntry { name, address, … }`; the ping result event; the join action (selected address → the session-spawn path; the client's existing `--server` flag bypasses the menu — both routes pinned).
- Consumes: the `oxide-proto-v47` status flow; the widgets (Task 15).

- [ ] **Step 1: Model tests (RED).** The file's round-trip; the ping math's table. Run red.
- [ ] **Step 2: Implement the list + the ping.** Run — green; a live ping against the rig server recorded in the task's evidence (server up briefly, one status round-trip, server down cleanly).
- [ ] **Step 3: The screen case.** Ignored GPU: the list with a motd-styled entry and each bar level. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the server list and the live ping (client)
```

**Verification:** the live ping receipt; the cases; the full gate; the rig left clean.

---

### Task 17: The pause menu and the placeholder screens

**Goal:** The pause menu (Escape from play) with Back to Game / Statistics / Achievements / Options / LAN (recorded inert) / Disconnect, and the placeholder Statistics, Achievements and Snooper screens with their frames and entries.

**Files:**
- Create: `crates/oxide-client/src/screens/pause.rs` — the pause menu + the stat/achievement/snooper placeholders (each: the dirt background, title, a Done/Back button, and a recorded placeholder body).
- Modify: `crates/oxide-client/src/screens/mod.rs` — the pause flow (Escape from play opens it; the chat/screen precedence updates — the M5 routing table's substitution "the no-screen arm is M3's capture rule" REPLACES with the pause menu; the capture-release rule unchanged: the pause screen owns the released pointer).
- Modify: `crates/oxide-client/src/main.rs` — the state (pause is not a session state: the session keeps running; the screen gates input).
- Test: `crates/oxide-client/tests/screens.rs` — the button set and layout; Escape's routing (play → pause; pause → play; screen → its parent).
- Test: the GPU case for the pause frame (ignored).

**Interfaces:**
- Produces: `ScreenState::Pause`, `::Stats`, `::Achievements`, `::Snooper`; the stats placeholder's recorded limit (the full grids post-v1); LAN inert button (the spec's own record).
- Consumes: Task 15's widgets.

- [ ] **Step 1: Routing tests (RED).** Escape layering with the pause menu in the chain (the M5 routing table's rows updated in the test). Run red.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: Gate and commit.** Run the full gate. Commit:

```
feat: the pause menu and the placeholder screens (client)
```

**Verification:** the routing table green; the pause frame case; the full gate.

---

### Task 18: The options hub and the video screen

**Goal:** The options hub with its tab strip and the Video screen: the full 1.8 slider and toggle set, every change applying through Task 14, the render-distance slider confirming the 1.8 range on the rig (Decision 12).

**Files:**
- Create: `crates/oxide-client/src/screens/options.rs` — the hub and the tab strip; the video screen's sliders/toggles and their labels (the exactly-1.8 set per checklist row 4 and survey §3.1; no 1.9+ options).
- Modify: `crates/oxide-client/src/screens/widgets.rs` — the option slider/cycle-button widgets the screen needs (extend, don't fork).
- Test: `crates/oxide-client/tests/screens.rs` — the full widget inventory of the video screen (each slider's range/step, each toggle's states — against the derived source values), the hub's tab set.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored: the video screen frame at the default scale; a scrolled state.

**Interfaces:**
- Produces: the `ScreenState::Options { tab }` machine + `ScreenState::VideoSettings`; the settings writes route through Task 14's apply (no direct renderer calls from the screen — pinned).
- Consumes: Task 14; Task 15's widgets; Task 3's ranges.

- [ ] **Step 1: Inventory tests (RED).** Each slider/toggle asserted against the derived source set; the range ends from Task 3. Run red.
- [ ] **Step 2: Implement the screen.** Run — green.
- [ ] **Step 3: The rig confirmation leg (the §16 rule).** The vanilla options screen at 1280×720: capture and read the render-distance slider's maximum (the toolbar hover text or the value readout — record the method; the source's 64-bit constructor raises the max to 32, so expect 32 — a shown 32 reconciles against the spec's "to 16" at the close), and the toggle list's exact set; the constants update per what the rig shows and the test re-pins. Evidence into the task's dir; the capture under the scene rules.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the options hub and the video screen (client)
```

**Verification:** the inventory green; the rig confirmation's recorded values; the full gate; the rig left clean.

---

### Task 19: The sound, controls, language, chat and skin screens

**Goal:** The options hub's remaining sub-screens: the nine sound sliders (live to the engine), Controls (the keybind list with rebinding, sensitivity, invert, smooth camera), Language (the list + Force Unicode), Chat Settings (the opacity/scale/width/height controls + the recorded Tab behaviour note), Skin Customization (the part toggles + cape switch — writing the skin-parts byte the wire uses, the M4 limit-19 carry), and the Snooper record.

**Files:**
- Create: `crates/oxide-client/src/screens/options_sound.rs`, `options_controls.rs`, `options_language.rs`, `options_chat.rs`, `options_skin.rs` (one file per screen; the hub gains their opens).
- Modify: `crates/oxide-client/src/keymap.rs` — the binding table becomes data-driven (`GameSettings`' bindings: the key names each action's default derives from `GameSettings.java`:127-151's own list; the physical-key translation stays winit-side; the rebinding state machine — the source's "press a key" flow — lands here).
- Modify: `crates/oxide-client/src/chat.rs` + `screens/` — the chat options consumers (opacity/width/height/scale into the chat draws — the currently-hard-coded values re-read; the chat mirror's carried entry points (M4 rec 42) wired to the chat settings' open/scroll — the route lands wired and reachable, closing the carry).
- Test: per screen's layout/behaviour pins in `tests/screens.rs`; the rebinding machine's tests (`keymap`).
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored: one frame per new screen.

**Interfaces:**
- Produces: the screens' `ScreenState` variants; the keybind model (`Binding { action, key }` list, defaults pinned to the source table's row set — the full 1.8 action list lands even where an action is inert in v1, recorded); the sensitivity's consumer (the M3 mouse path re-read: the `f³·8` law already lives there — the setting feeds it).
- Consumes: Task 3; Task 8 (sound sliders → engine); Task 14.

- [ ] **Step 1: Screen tests (RED).** Each screen's inventory + one frame per screen red first.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: The rebinding + sensitivity live check.** Rebind a key manually (recorded log), move the mouse (sensitivity change observable in the client log's yaw deltas — record).
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the remaining options screens and the rebindable controls (client)
```

**Verification:** the cases; the live checks recorded; the full gate.

---

### Task 20: The death, disconnect, terrain and sleep screens

**Goal:** The remaining overlay/screen states: the death screen (with its Respawn/Title buttons and the existing death-camera roll intact), the disconnect screen with its reason, the "Downloading terrain" screen (shown while the join's chunks load — the client's own first-frame gate), the sleep overlay (the visual state + camera rise), and the open-link confirm overlay (`GuiConfirmOpenLink`).

**Files:**
- Create: `crates/oxide-client/src/screens/end_states.rs` — the death/disconnect/terrain/sleep/confirm screens.
- Modify: `crates/oxide-client/src/screens/mod.rs`, `crates/oxide-client/src/main.rs` — the state transitions (Joined→terrain→play; death→Respawn path; disconnect→the screen + Title; chat open-link → the confirm overlay; sleeping from the server's sleep packet path or the standing state source — derive which wire signal 1.8 uses for the own-player sleep overlay and pin).
- Test: `tests/screens.rs` + ignored frames per screen.
- Test: `crates/oxide-game` — the sleep-state source (whatever packet path is derived — a decode pin if new).

**Interfaces:**
- Produces: `ScreenState::{Death, Disconnected, DownloadTerrain, ConfirmOpenLink}` + the sleep overlay's draw state; the death screen's respawn action routes to the standing `ClientStatus::Respawn` path; the disconnect screen's text = the session's `Disconnected.reason`.
- Consumes: Tasks 15–17; the M3 death camera.

- [ ] **Step 1: Tests (RED).** Transitions + frames red first.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: The live death/respawn leg.** On the rig: `/kill`, death screen frame, respawn, play continues (the M3 acceptance's own shape) — evidence recorded.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the death, disconnect, terrain and sleep screens (client)
```

**Verification:** the leg's evidence; the cases; the full gate.

---

### Task 21: The chat completion and the text-field extraction

**Goal:** Tab completion works in the chat field and the text-field engine is one machine: the chat field's behaviour is extracted so the anvil-name and book corners share it (the M5 review's §2.4 item 4's "field engines" seam), and Tab completes against the world's own player list, commands, or arguments per the source.

**Files:**
- Create/extract: the field engine (from `crates/oxide-client/src/chat.rs`'s field machine into `crates/oxide-client/src/text_field.rs`), consumers updated (chat, anvil, book search — the creative search field's own read re-verified).
- Modify: `crates/oxide-client/src/chat.rs` — the completion engine (the source's `GuiChat`/`CommandHandler` ladder: the player-name candidates, the `@`/`/` contexts, the cycling — derive the ladder's cases and pin the pure function; the candidates' sources from the session's standing player list).
- Test: the completion function (a fixture world/player list: name prefix → candidates; command prefix → the command space; cycling order preserved), the field engine's existing cases kept green.

**Interfaces:**
- Produces: `TextField` (the shared machine: insert/backspace/arrow/home/end/selection-free 1.8 subset, the blink state); `complete(input: &str, candidates) -> Option<(String, usize)>` pure function; the chat field's Tab consumes it (the M4 row-4 carry "No chat tab-completion; Tab swallowed" closes).
- Consumes: the M4 chat; the standing player list.

- [ ] **Step 1: Extraction (green-to-green).** The field machine moves; all existing chat/book/anvil cases stay green; the diff shows the move.
- [ ] **Step 2: Completion tests (RED→green).** The ladder's fixtures.
- [ ] **Step 3: Gate and commit.** Run the full gate. Commit:

```
feat: the shared text field and the chat completion (client)
```

**Verification:** the suites green through the move; the completion fixtures; the full gate.

---

*(continued in part 4 — Tasks 22–31 and the tables)*

### Task 22: The F3 overlay rebuilt

**Goal:** The F3 overlay carries the full field set — the left and right columns as the checklist's row 5 lists them, with the dynamic fields masked in the diff — and its key toggles: F3+P, F3+A, F3+F fold; F3+T and F3+N record as no-ops; F3+Q prints the binding help.

**Files:**
- Modify: `crates/oxide-client/src/debug_overlay` (wherever the M5 overlay's builder lives — `main.rs`'s overlay path) — the field set: the standing rows plus `P:` (Task 7's stats), `Biome:` (the self-derived biome name per column — the session reports the view column's biome; the wire source: `0x21`'s data ends with the 256 biome bytes when GroundUpContinuous (protocol §3.1) and `0x26` always implies GroundUp (protocol §3.2) — the task re-derives the exact decode window from the protocol reference and the source's `Chunk.getBiome`; a column without a biome byte shows the fallback name — no invented values), `Light:` (the view block's sky/block nibbles — the standing light query), `Local Difficulty:` (day count from the Join/clock state + the difficulty byte — the derived formula), `Looking at:` + the block name at F3+H.
- Modify: `crates/oxide-client/src/main.rs` — the right column: `Java:` (our rustc + build id, recorded), `Mem:` (RSS from `/proc/self/statm` on Linux; recorded fallback `n/a` elsewhere), `CPU:` (`/proc/cpuinfo`'s model string, recorded fallback), `Display:` (the wgpu adapter name + the window's size).
- Modify: `crates/oxide-client/src/main.rs` — the toggles: F3 held + key chords (P/A/F/T/N/Q; the H chord stands from M5), the advanced-tooltips flag from F3+H (the M5 appendix), F3+G's chunk-border state, F3+B's hitbox state, Shift+F3's lagometer state.
- Test: `crates/oxide-client/src/main.rs` — the field builder's deterministic rows (a fixture state → the exact line set; the dynamic values pinned where deterministic, masked otherwise), the chord routing table.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored: the overlay frame at a fixture state (background quads 0.25 alpha, line height 9 + 1 px, right column at width−2).

**Interfaces:**
- Produces: `DebugFlags { visible, advanced_tooltips, hitboxes, chunk_borders, lagometer, pause_on_focus_lost }` in the client; the field builder reading the standing state sources.
- Consumes: Task 7 (particle stats), Task 5 (difficulty/day state), the standing light/aim data.

- [ ] **Step 1: Builder tests (RED).** The fixture state's exact rows; red first.
- [ ] **Step 2: Implement the field set.** Run — green.
- [ ] **Step 3: The chord routing.** P/A/F/T/N/Q table (each a recorded action or no-op); F3+F cycles the render-distance setting (the slider chain — Tasks 3/18/28 — owns the wiring); F3+A asks the session for a rebuild (the standing mesh path's trigger). Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the F3 overlay field set and its sub-mode keys (client)
```

**Verification:** the builder fixtures; the routing table; the ignored overlay frame; the full gate.

---

### Task 23: F3's visual sub-modes

**Goal:** Hitboxes (F3+B) draw entity boxes and look vectors, chunk borders (F3+G) draw the 16×16 grid at section corners, and the lagometer (Shift+F3) draws the tick-duration graph from the session's own timings.

**Files:**
- Modify: `crates/oxide-render/src/world_overlay.rs` — the hitbox boxes (per-entity box from the frame's bounds + the look vector line — the source's `renderDebugBoundingBox` shape derives in-task), the chunk-border quads (the grid lines at y-level, the source's own color/shape), the lagometer's graph (the standing text/canvas machinery; the frame series from the session's tick report).
- Modify: `crates/oxide-game/src/session.rs` — the tick-duration series (the session's per-tick elapsed time, ring-buffered, published in `PlayerTick` or a new small event — pin the shape).
- Modify: `crates/oxide-client/src/main.rs` — the flags → pass states.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — ignored: a hitbox frame (two entities' boxes), a chunk-border frame, a lagometer frame with a pinned series.
- Test: `crates/oxide-game` — the series' ring behaviour (bounded, ordered).

**Interfaces:**
- Produces: `OverlayFlags { hitboxes, chunk_borders, lagometer }`; `TickSeries` (the client's read).
- Consumes: Task 22's flags; the standing entity frames.

- [ ] **Step 1: Cases (RED).** Each visual red first.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: Gate and commit.** Run the full gate. Commit:

```
feat: the hitbox, chunk-border and lagometer overlays (render)
```

**Verification:** the three cases; the series test; the full gate.

---

### Task 24: F2 screenshots

**Goal:** F2 saves a PNG of the current frame to the store's `screenshots/` directory with the source's timestamp name, logs the path, and prints the chat notice if 1.8 does — the offscreen readback path lands with a pinned round trip.

**Files:**
- Modify: `crates/oxide-render/src/renderer.rs` — the capture path: a `capture_frame()` that re-renders into an offscreen RGBA texture and reads it back (the same frame contents; the render-pass split reuses the frame builder — derive the seam).
- Modify: `crates/oxide-client/src/main.rs` — the F2 route (keymap → capture → PNG write): the PNG writer lands in `oxide-assets::png_write` (the assets crate owns texture-side PNG work and already carries the `png` dependency; the client → assets edge exists) and the client calls it — the client never grows its own encoder edge. The writer's round-trip decode test is in the assets crate; the client's test asserts the call path only.
- Test: `crates/oxide-render/tests/pipeline_headless.rs` — a capture round trip: a known frame's readback pixels at pinned coordinates (ignored, GPU).
- Test: `crates/oxide-assets` — the writer's round trip (encode → decode → equal).

**Interfaces:**
- Produces: `Renderer::capture_frame() -> Option<CapturedFrame { width, height, rgba }>`; `oxide_assets::png_write::write_png(path, w, h, rgba)`; the screenshot path `<data dir>/oxidecraft/screenshots/<timestamp>.png` (derive the source's name format, `ScreenShotHelper.java` — pin the format function's cases).
- Consumes: the standing frame builder.

- [ ] **Step 1: Writer test (RED).** Round trip; red first.
- [ ] **Step 2: The capture case (RED→green).** The readback pixels.
- [ ] **Step 3: Wire F2 + save a real screenshot on the dev machine.** Record the file (path + size + a manual look).
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the F2 screenshot capture (render, client)
```

**Verification:** the round trip; the real screenshot receipt; the full gate.

---

### Task 25: The keybinds

**Goal:** The full default binding list lands in the settings store with rebinding (Task 19's machine) — F2, F5, F11, the standing set — and every action routes from data, not hard-coded matches; the controls screen reads the same list.

**Files:**
- Modify: `crates/oxide-game/src/settings.rs` — the bindings table (action list + defaults per the source's `GameSettings` rows; the full list lands even where an action is inert in v1, each inert row recorded).
- Modify: `crates/oxide-client/src/keymap.rs` — the data-driven translation (physical key → action lookup from settings; the M5 hard-coded matches reduce to the defaults' data).
- Modify: `crates/oxide-client/src/main.rs` — the new routes: F2 (capture), F5 (camera modes — Task 27), F11 (fullscreen), F1 (HUD hide — Task 27), and the standing ones re-read.
- Modify: `crates/oxide-client/src/screens/options_controls.rs` — reads the table (Task 19's screen; this task completes any inert row it exposed).
- Test: the default table pinned row-by-row against the derived source list; the lookup function; a rebound key's route (a test binding change + the routed event).

**Interfaces:**
- Produces: the `Action` enum (the source's action set) + `bindings: Vec<(Action, Key)>` in `GameSettings`; `keymap::translate` becomes `lookup(settings, code) -> Option<Action>`.
- Consumes: Task 19; Task 3.

- [ ] **Step 1: Table tests (RED).** Every default; red first.
- [ ] **Step 2: Implement the lookup + routes.** Run — green.
- [ ] **Step 3: The rebind round trip.** Rebind + route + persist (test-level; the manual check in Task 19 stands).
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the full keybind table and the routed hotkeys (client)
```

**Verification:** the table's pin; the route tests; the full gate.

---

### Task 26: The remaining §11.2 screens

**Goal:** The §11.2 list's remaining screens land, and the language loader ships: the Skin Customization screen is complete (Task 19's version finished), the resource-packs screen lands as the recorded no-op surface (the button exists; selecting does nothing — the §16 rule), and the language loader (M5 §2.4 item 4, checklist row 55 — a real deliverable this milestone) lands: `en_US.lang` + the hashed locale objects load, missing keys render as the raw key, and the registry tables' embedded strings re-source from the loader where their keys resolve. The task owns the loader, its tests, and the language screen's selection (the list, Force Unicode, the selection persisting — only `en_US` has strings in practice, recorded).

**Files:**
- Create: `crates/oxide-assets/src/lang.rs` — the loader: parse the lang file (`key=value` lines, the source's own parse rules derive), merge the hashed locale objects, the missing-key passthrough (the lookup returns the key), hostile-input caps (line count, key/value lengths).
- Create: `crates/oxide-client/src/screens/options_language.rs` — the completion of Task 19's screen (list from the loader).
- Create: `crates/oxide-client/src/screens/options_packs.rs` — the packs screen's recorded no-op surface.
- Modify: the registry tables' string sourcing where the loader lands (the M5 item 7 limit: the embedded strings re-source from the loader — the task walks the tables and swaps the source; where a string has no key (derived names), it stays embedded, recorded).
- Test: the loader's suite (parse, merge, missing key, hostile) + the screen frames (ignored).

**Interfaces:**
- Produces: `oxide_assets::lang::Lang` (`get(key) -> &str` with the key passthrough) and `Lang::load(store)`; the language screen's `ScreenState` variant completes Task 19's.
- Consumes: the store's `lang/en_US.lang` + index objects.

- [ ] **Step 1: Loader tests (RED).** Synthetic fixtures + the store-gated real-file case (counts recorded). Run red.
- [ ] **Step 2: Implement the loader; swap the tables.** Run — green; the affected suites re-run.
- [ ] **Step 3: The screens.** Frames green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the language loader and the remaining option screens (assets, client)
```

**Verification:** the loader suite; the store case; the frames; the full gate.

---

### Task 27: The camera modes, F1 and the spectator HUD

**Goal:** The M5-routed items close: F5 cycles third-person back/front with the camera distance and the occlusion pull-in, F1 hides the HUD (and the hand and overlays — the source's own suppression set), and the spectator HUD's state (when the gamemode byte says spectator — however the rig can reach it) draws its recorded subset. The carried camera-mode overrides (M4 limit 37: sleeping and front-view yaw at the billboard consumers) land with them.

**Files:**
- Modify: `crates/oxide-render/src/camera.rs` — the third-person offset (distance 4.0, the smooth distance, the occlusion raycast pull-in — derive the source's rule), the front-view yaw flip.
- Modify: `crates/oxide-client/src/main.rs` — the F5/F1 routes and the HUD-suppression state; the sleeping override (the billboard consumers' view yaw at the sleep state).
- Modify: `crates/oxide-render/src/entity_pass.rs` or the billboard consumers — the mode-conditional overrides (`playerViewY` and the `+180` third-person-front pair — the standing facts file `references/entity-tick-parity.md`'s records).
- Modify: the HUD pass — the F1 suppression (HUD + hand + overlays per the source's set).
- Test: the camera offset's unit pins (distance, pull-in geometry); the F1 suppression's state table; ignored GPU frames for third-person back/front and HUD-hidden.

**Interfaces:**
- Produces: `CameraMode` (First/ThirdBack/ThirdFront) in the client; the suppression flag the HUD/held-item/overlay passes read.
- Consumes: Task 25's F5 binding; the standing billboard facts.

- [ ] **Step 1: Unit pins (RED).** The offset + pull-in; the suppression table.
- [ ] **Step 2: Implement.** Run — green.
- [ ] **Step 3: The frames.** Third-person back/front + a wall-behind pull-in + F1 hidden — ignored GPU cases, and a live capture leg on the rig for the operator's list (recorded).
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the camera modes, F1 suppression and the spectator HUD state (client, render)
```

**Verification:** the cases; the live frames; the full gate.

---

### Task 28: The window: resize and the scaled-resolution table

**Goal:** The window's resize and display path is complete: the resize arm re-derives the scaled resolution and the projection at every GUI scale (the four-scale table's values pinned at the three reference resolutions), fullscreen's runtime behaviour is verified against the settings (the apply lives in Task 14; F11's route in Task 25), and the render-distance slider's chain applies live (the far plane, fog, mesher and the C15 re-send — the C15 half is Task 5's path, this task wires the slider to it).

**Files:**
- Modify: `crates/oxide-client/src/main.rs` — the resize arm's scale re-derive (verify the standing `renderer.resize` path covers it — extend where it does not); the render-distance slider's consumer chain (settings → far plane state → session C15); fullscreen's runtime verification hooks (the apply is Task 14's; this task owns the size/mode follow-up and the record).
- Modify: `crates/oxide-render/src/renderer.rs` — the resize's full state re-derive (scaled resolution, projections, the HUD/screen resolutions).
- Test: the resize unit (two sizes → the scaled resolutions + projections, per GUI scale 0–3 — the four-scale table's derived values pinned at 1280×720/1920×1080/2560×1440 per checklist 53's Gaps row: the table's values derive from `ScaledResolution`'s formula in-task and pin; the acceptance compares against vanilla).
- Test: ignored GPU frames at two window sizes (a GUI frame's pixel geometry).

**Interfaces:**
- Produces: the resize/fullscreen behaviors on the standing winit arms; the render-distance apply chain's final wiring.
- Consumes: Tasks 3, 4, 5, 14.

- [ ] **Step 1: The scaled-resolution table (RED).** The three resolutions × four scale values pinned (the formula derivation is the source's own loop — re-derive in-task, cite). Run red.
- [ ] **Step 2: The resize wiring.** Run — green.
- [ ] **Step 3: Fullscreen manual leg.** Toggle fullscreen and back on the dev machine; record the log + a capture.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the window resize, fullscreen and the display table (client)
```

**Verification:** the table's values against a live vanilla window dump (the checklist's 53 row); the manual leg; the full gate.

---

### Task 29: The fixed-set carry bundle

**Goal:** The carried fixes that ride M6's own files land: the window-icon/demo CLI corners where the settings store touches them, the F3-overlay region of the M5 comment set at its files' touches, and the M5 review's §2.4 items not owned above are each either implemented (this plan's tasks) or recorded with their per-item disposition.

**Files:**
- Modify: the files the still-open comment rows name, at each file's touch within this milestone (the rows: the M5 review §1b's open set — 38, D3, D5 remainder, D12, D14–D16, D19, D26, D27, D30, D31, 73a, 73c, 78b, T18-N1; the plan's own new rows if any task leaves one).
- Modify: `docs/parity/checklist.md` — the rows this milestone's own evidence updates (the acceptance task owns the sweep; this task owns only rows whose file this milestone touches BEFORE the acceptance).
- Test: none new (comment-only edits); the sweep's checks are the verification.

**Interfaces:**
- Produces: the comment fixes; each row's disposition (fixed / routed / re-carried with trigger) recorded in the task's evidence for the close's records.
- Consumes: nothing at runtime; reads the review's lists.

- [ ] **Step 1: Re-derive the open rows at the current HEAD.** Every row's premise re-checked (the lists predate this milestone's edits); each row: fix here, or re-carry with its trigger. Run: the sweeps' greps, recorded.
- [ ] **Step 2: Apply the fixes.** Comment/citation-only; each byte-quoted before. Run the extractor's scan for the touched files — zero flags on added lines.
- [ ] **Step 3: Gate and commit.** Run the full gate. Commit:

```
chore: sweep the carried comments at their files' touches
```

**Verification:** the re-derived list; the sweep's zero-flag run; `git diff --stat` comment-only.

---

### Task 30: The acceptance run

**Goal:** The rig proves the milestone: the F3 field set and sub-modes, the screens' item-by-item pairs, the weather pair, the particle states, the sound receipts, the overlays, the screenshots' round trip, the resize/fullscreen legs and the render-distance range — with the checklist rows taking their evidence and every new item's parity recorded.

**Files:**
- Evidence: `refs/m6-acceptance/` (git-ignored; frames, logs, scripts, captures, the mixer dumps, the report).
- Modify: `docs/parity/checklist.md` — the rows' evidence lines (1–6, 22, 24, 26, 28–32, 34–38, 47–49, 51–53, 55–57 and the touched-row corrections); the Gaps section's closes (the panorama rotation period and splash bounce measured now; the particle count limits — closed from the source's own formulas; the moon phase mapping; the rain cadence).
- Test: no new repo tests — the run is the verification.

**Interfaces:**
- Produces (the run's shape — machinery + legs, then a FINISH pass, the M4/M5 pattern):
  - The scene set (pin it): weather — `/weather rain 60` then `clear` on a re-primed scene pair (both clients; the vanilla frames' rain streaks + ours; the frames taken under the per-capture re-send discipline with a NEW standing check: the vanilla sky state must match the scene's intended weather — a rain-drift capture is void, re-take); the particles — a break scene (both clients digging, the crack + break particles), a sprint trail, a water splash, and the wire-particle scenes from the server's `/particle` command (1.8.9 carries it — drive vanilla and ours with named effects and compare the frames); the sounds — the mixer dumps for a rain loop, a dig, a record (the set + the event log); the overlays — pumpkin blur (give + wear), portal (enter a nether portal frame), block-in-hand (camera inside a block, `tpx`), the sleep overlay if reachable (a two-player sleep leg — may be unreachable with one scripted client: record the attempt and the limit); the screens — item-by-item pairs for the ones the operator can drive on both sides (the desktop cannot type into our window for some paths: the standing rule — our side's screens render from state, so most are scripted-openable; the vanilla side's captures by hand where needed); the panorama rotation period + splash bounce (the two measured constants — timed captures on vanilla); F3 sub-modes on both sides (F3+F/A/P; B/G via the overlay frames); the HUD hide (F1 — xdotool delivers F1 per the standing note; F3's readout gap stands as M9's); the screenshot round trip (F2 on ours, file verified); the resize/fullscreen legs (window dumps at two sizes + fullscreen toggle).
  - The render-distance leg: the slider to 16 and to 2 — the far plane/fog change captured, the C15 observed on the wire (the recorder proxy: the re-send packet's bytes + the server's log of the new view distance if it logs one).
  - The checklist rows above; each row: pass / fail / deferred with the record.
  - The multi-round shape: (a) machinery + the weather/particle/sound legs; (b) the screens + overlay legs; (c) the FINISH pass (report, checklist edits, gate, commit) with the read-only findings triage in parallel; each confirmed defect chained into its scoped fix round (the standing loop).
- Consumes: Tasks 1–30; the rig rules (both clients; the per-capture re-send; the soak; the reset); the M5 rig lessons (all).

- [ ] **Step 1: Machinery + the weather/particle/sound legs.** The client pinned-build practice; the scene resets; the legs; the run log with the RESUME block.
- [ ] **Step 2: The screens + overlay legs + the two measured constants.**
- [ ] **Step 3: The wire legs (C15, sounds' 0x29 count, the particle packets' counts on a scripted scene) via the recorder proxy.**
- [ ] **Step 4: FINISH.** The report; the checklist edits; the findings triaged with fix rounds as ruled; the gate; the commit:

```
docs: record the M6 acceptance evidence
```

**Verification:** the report's measured values; the checklist diff; the gate; the rig left clean (ports free, world saved, clients stopped); the operator's by-eye list kept current.

---

### Task 31: The close

**Goal:** The milestone closes: the final whole-branch review, its findings folded, the docs swept, the tag pushed with CI verified.

**Files:**
- Modify: `docs/STATE.md`, `CHANGELOG.md`, `docs/handoff/<date>-m6-close.md` (new), `docs/perf.md`, `README.md`, the plan's own close checks + the `*(Executed …)*` note.

**Interfaces:**
- Produces:
  - The freeze: the milestone diff split per crate + combined + docs + the `-U10` review package, with the commit list, final stat, frozen shas and the head sha beside them; the reviewer's backlog-triage input = the backlog sweep's open/re-carried rows extracted mechanically.
  - The final whole-branch review, in the M5 shape: deferred triage first, then cross-cutting, then the criteria walk, with scratch-worktree evidence duties; findings adjudicated (Critical/Important route to fix rounds + a scoped re-review; record-only notes ride the close's recordings).
  - The docs sweep in one movement: STATE (the stage line — spec version, milestone complete, tag, reviewed head, verdict; the dated update bullet; tests/CI/live-evidence bullets; a new "M6 evidence" section with the acceptance numbers and the documented class list; caveats "carried by M6 as delivered", each with its owner; "Next actions" = the ordered M7 backlog; env facts refreshed — re-check the desktop and the local stable); CHANGELOG (the milestone section + the dependency lines: `cpal`, `lewton`, `rand`); the new handoff; `docs/perf.md`'s summary line; README's status paragraph.
  - The tidy pass on the FINAL bytes (the committed-docs grep; the document conventions).
  - The close: content gate; the close commit; the tag `m6` with the prior tags' message shape; push `--follow-tags`; CI verified on the pushed head (the plan-era pending runs resolved first); the run-record follow-up commit once green; the ledger close.
- Consumes: every task's records; the review verdict.

- [ ] **Step 1: Freeze + final review.** The package; the dispatch; the verdict.
- [ ] **Step 2: Fold the verdict.** Fix rounds as ruled + the scoped re-review(s).
- [ ] **Step 3: The docs sweep.** All files above, content-gated before staging.
- [ ] **Step 4: Tidy pass + close.** The grep on final bytes; the gate; the close commit; the tag; the push; the CI watch. Commit:

```
docs: close out milestone M6 (sound, particles, weather, screens)
```

The tag `m6` follows the prior tags' message shape (`M6: sound, particles, weather, screens`); push `--follow-tags`.
- [ ] **Step 5: The run record.** The follow-up commit once CI is green; the ledger close; the owner's summary.

**Verification:** the reviewer's verdict; the tidy grep's clean run; `GATE OK` on the final revision; CI green on the pushed head; the tag's read-back.

---

## Coverage table

*(Task numbers final at assembly; every binding item disposed.)*

| Binding item | Where the plan answers it | Evidence |
| --- | --- | --- |
| Spec §13 row **M6 Sound, particles, weather, screens** (sound engine and music ticker; particles; weather; sky and stars; overlays; the full screen list from §11.2; keybinds; F3 with sub-modes; F2 screenshots; the 1.8 render-distance range to 16 chunks; window resize and fullscreen; exit "the named screen list passes its item-by-item parity checks, including the F3 field set") | Sound: Tasks 8, 9; particles: 7, 11, 13; weather: 5, 10, 11; sky/stars: 10; overlays: 12; screens: 15–21, 26; keybinds: 19, 25; F3: 22, 23; F2: 24; render distance: 3, 18, 28 (the range confirmed on the rig); resize/fullscreen: 28 | Task 30's item-by-item legs + the F3 field set's frames |
| §11.2 screen list — in scope: main menu (15); server list with ping (16); pause incl. Statistics and Achievements entries + their screens (17); options hub + video/sound/controls/multiplayer/language/chat/skin/snooper/packs sub-screens (15–19, 26); death (20); disconnect (20); "Downloading terrain" (20); chat with wrapping + 20-line scrollback (standing; completion 21); multiplayer sleep overlay (20, owner Q5). Out, recorded: singleplayer list/create (§17.1 post-v1), win/credits, demo, LAN share (recorded), the pack loader (§16 no-op, 26), the command-block editor (post-v1), the stat grids beyond their entries (17, owner Q4) | As listed | Task 30's screen legs; the records in the close's caveats |
| §11.2 HUD additions: damage flash (standing M3), block outline (standing), crack overlay (standing), spectator HUD (27 — the M5-routed item), third-person camera with HUD hide (27) | 27 | Ignored cases + the live frames |
| §11.2 sound list: effects and music ticker with streaming, vanilla categories, pitch randomisation, 3D attenuation, the 64-block music range, the 100-tick fade (re-derived at 9 — pinned only if a source site is found; else the spec correction is recorded) | Tasks 8, 9 | The mixer dumps + the event logs + the ticker tests |
| F12 — vanilla screens (main menu, server list, pause incl. Statistics and Achievements, options, death, disconnect, F3 with its sub-modes, screenshot hotkey) | 15–20, 22, 23, 24 | Task 30 |
| F5/F10/§12 live rows — the acceptance's live comparisons | 30 | The report |
| P4/S2 — the new packets' wire shape (0x29/0x2A/0x2B/0x2C) | 2, 5 | The proxied wire legs + the codec suites |
| Checklist 1 (panorama) | 15; the rotation period measured at 30 | The measured constant + the frames |
| Checklist 2 (splash) | 15; the bounce measured at 30 | Same |
| Checklist 3, 4 (buttons; the video set) | 15, 18 | The frames + the inventory tests |
| Checklist 5 (F3 fields) | 22 | The builder fixtures + the frames |
| Checklist 6 (F3 sub-modes) | 22, 23 | The flags' frames (F3+B/G/Shift+F3 close here) |
| Checklist 22 (achievement popup / no toasts) | 17 (the placeholder's frame; no toast machinery anywhere — the absence asserted) | The frames |
| Checklist 24 (crack overlay removal) | standing M3; the carry row re-verified at 29 | The standing case |
| Checklist 26 (block-in-hand) | 12 | The case |
| Checklist 28 (vignette) | standing M2/M4 renderer; verified by the daytime/night frames at 30 (record) | The frames |
| Checklist 29 (water/lava/fire overlays) | standing M5; portal added at 12 | The cases + frames |
| Checklist 30 (pumpkin) | 12 | The case + the leg |
| Checklist 31 (portal + nausea) | 12 | The case + the ramp unit |
| Checklist 32 (fog conditions incl. rain) | 10 + the standing fog code; the rain pair at 30 | The rain pair |
| Checklist 34 (sky gradient to horizon) | 10 | The band case + the frames |
| Checklist 35 (sun/moon) | standing M2 sky + 10's band context; the frames at 30 | The frames |
| Checklist 36 (stars) | 10 (the rained alpha) + standing | The case + the frames |
| Checklist 37 (clouds: drift, cell size, Fancy prisms, no cloud fog) | 4 (the Off/Fast/Fancy arms) + 10 (the band's cloud context); the drift rate, cell size and prism geometry re-derive at 4's touch against the survey's §4.1 record and pin where the task touches them — any residual re-carries with its trigger at 31 | The cloud cases + the frames |
| Checklist 38 (rain/snow, splashes, cadence) | 10, 11, 13; the cadence recorded from the source + the dumps | The rain pair + the dumps |
| Checklist 47 (particle table) | 7 (ids) + 11 (the draws) + 30 (the `/particle` legs) | The engine suite + the frames |
| Checklist 48 (FOV modifiers) | standing M3 law; the setting feeds at 19 | The standing vectors |
| Checklist 49 (mouse look/sensitivity) | 19 | The live check |
| Checklist 51 (third-person, F1) | 27 | The cases + the frames |
| Checklist 52 (the nine categories, clamps, streaming, attenuation) | 8 | The mixer law tests + the dumps |
| Checklist 53 (the GUI-scale four-scale table) | 4, 14, 28 | The table pins + the live pairs (the M5 carry closes) |
| Checklist 55 (lang) | 26 | The loader + the screen |
| Checklist 56 (packs) | 26 (the recorded no-op) | The record |
| Checklist 57 (1.8 exclusions) | standing + 17's placeholder + the acceptance's frames | The frames |
| STATE backlog 1 (F-01/F-02) | 1 | The reruns |
| STATE backlog 2 (M6's own scope) | This table | — |
| STATE backlog 3 (the weather/light carries: the fog time term; the water-fog colour read; the rows-440+ lift; the neighbour-brightness term; the sky horizon; the celestial closing term) | 10 (the fog time term and the water-fog colour read re-derived and closed-or-fixed; the rows-440+ lift re-measured and ruled at 12; the neighbour-brightness term CARRIED with its recorded RED shape — immaterial to this milestone's scenes and no re-touch of `light.rs`: the carry's trigger stands "the next light touch" and this milestone's light-adjacent work is the fog and sky path, not the light engine — record; the horizon and celestial closing term answered by 10's disambiguation) | The task's evidence files |
| STATE backlog 4 (the screen/setting rows: chat tab completion and mirror; pointer scaling; part toggles; camera overrides; the render-distance option; the GUI-scale table; `set_gui_scale`'s caller; the lang loader) | 21 (completion); 19 (mirror halves wired; part toggles); 14 (pointer scaling); 27 (camera overrides); 3, 18, 28 (render distance); 4, 14, 28 (GUI scale); 26 (lang) | The task cases |
| STATE backlog 5 (the still-open comment/citation rows) | 29 | The sweep's receipts |
| STATE backlog 6 (the operator's list) | 30 keeps it current | The report |
| STATE backlog 7 (later homes: M7 sounds' remaining rows + online fetch; M9 the animation sweep, the F3 FPS-readout gap, interest management) | Re-carried as recorded; the sounds' M7 remainder re-derived: this milestone's engine covers the v1 sound scope; the row's residual (any event class not wired) named at the close | The carry rows |
| The M4 deferred rows routed here: 4 (chat tab completion — 21), 5 (OPEN_URL confirm — 20), 13 (lightning — 5's global entity + recorded no-draw), 19 (part toggles — 19), 37 (the sleeping/front-view camera-mode overrides — 27), 42 (chat mirror — 19), 47 (the link opener — re-carried: the settings passes did not replace `xdg-open`; trigger re-stated as "the link-open path's next touch" — the confirm overlay at 20 keeps the standing opener, recorded), 48 (pointer scaling — 14), 49 (wheel shift-flavour — 25's table records the remaining gap or closes; re-derive), 50 (completion — 21) | As listed | The task records |
| §16 obligations: the Azure item (tracked owner action, not this plan's; the close records its status), the render-distance range (18), the DIVERGENCES entries (anaglyph and Use VBOs no-ops, resolution cycling substitute, the screens scope exclusions, the stats placeholders — written by their tasks) | As listed | The close's sweep |
| M5 final review §2.4 items 1–6 | 1: 3, 4, 14, 18, 19, 21, 28; 2: 10, 12; 3: 27; 4: 26 (loader) + 21 (fields); 5 (the window-click prediction splits): re-carried — no prediction task exists in this plan; the standing trigger (the next window-click round) stands and the close records the re-carry; 6 (riding HUD + non-own-player effect visuals): re-carried — v1 has no riding path (the riding HUD lands with riding, post-v1) and effects draw for the own player only (the M5 limit re-stated); the close records both | As listed |
| Dependency additions | `cpal`, `lewton`, `rand` at 8; `deny.toml` in the same commit | `cargo deny check` green |

---

## Pre-flight (before Task 1)

- The owner's answers to the six questions above are recorded in the answers table before execution begins.
- The plan's base: `ee4e863` (= `origin/main`, clean at planning; the M5 close's CI green — runs `38127359527` and `38127568921`). A pre-plan scoped fix landed first in this window: `37abe42` `fix(render): keep the camera basis continuous through the look poles` (the owner-reported flip at the look-down clamp; its pins stand in `camera.rs` and Task 27 builds on the same basis).
- The execution ledger begins (first line names this plan); the reference tree's HEAD is recorded with it: `1717f75902c6184a1ed1bfcd7880404aab4da503`.
- The two distilled references this plan cites (`references/entity-tick-parity.md`'s billboard facts for Task 27; `references/rig-capture-notes.md` before Task 30) are mirrored to `refs/m6-homework/` before the tasks that read them.
- The conflict scan (every task pair sharing a file or interface, plus one self-consistency row per task) is written to the ledger, and each conflict is ruled on before the first dispatch; the review loop is only the net for what implementation reveals.
- `rustup update stable` before the first gate; the desktop and its scale re-checked before any rig leg; `gh auth status` before any push.
- The full gate runs clean on the base revision before Task 1's dispatch.
- The F-01/F-02 disposition (Task 1) and the M5 close's CI follow-up are checked against STATE before the first dispatch.

---

## Known limits this plan accepts

*(initial list; execution refines it, and Task 31's sweep carries the final wording)*

1. The anaglyph toggle and Use VBOs persist and are recorded no-ops (our renderer has no anaglyph path and is always on vertex buffers) — DIVERGENCES.
2. Max Framerate's limiter applies (Decision 11) but its effect is unmeasured this milestone; M9 owns the measurement.
3. The display-mode list is the recorded substitute: borderless fullscreen on the current monitor; the source's resolution cycling is out.
4. F3+T and F3+N are recorded no-ops (no pack reloads, no spectator mode in v1).
5. The F3 right column's `Java:`/`Mem:`/`CPU:` carry our environment's values under the vanilla field names (owner Q3).
6. The Statistics and Achievements screens are entries-and-frame placeholders; the grids are post-v1 (owner Q4).
7. The sleep overlay lands as the visual state (owner Q5); the server-side sleep path's corners (the fade timing against a real multi-player sleep) may prove unreachable on the one-client rig and would be recorded non-passed.
8. The sound acceptance rests on the mixer dumps and the event logs; the vanilla side's audio is verified by mapping and cadence, with the operator's by-ear pass as the recorded supplement (Decision 13).
9. The biome line's decode window re-derives in Task 22 (protocol §3.1/§3.2: the bytes are present on full sends); if an edge proves absent on the rig, the fallback name and the finding are recorded — no invented values.
10. The panorama rotation period and the splash bounce ride the acceptance's measurements; until Task 30 lands them, their constants are the two recorded pending values (no blank threshold ships).
11. The legacy 64×32 skin class, the wire cart sub-type/orb pins, the wing-phase exactness, the chest-offset block and the `world_scan.py` index re-derive keep their standing triggers (no M6 task reaches their files).
12. The neighbour-brightness light term keeps its standing carry (immaterial to this milestone's scenes; its RED shape is recorded in the M4 fix-a2b report).
13. The keepalive margin stays a standing watch — Task 30 records the soak rates.
14. `world_scan.py` stays barred from acceptance paths.
15. Screen-for-screen parity for path-dependent states (a server denial, an operator-only vanilla capture) rests on the same rules as M5's: reachable states pair live; unreachable ones record.
16. Interrupted or capped acceptance rounds keep their messages and resume blocks current; re-shots are marked as later generations per the standing record rules.
