# Oxidecraft — project state

Updated: 2026-10-12

This file is the live state of the project. Keep it current before every handoff, long pause, and
milestone boundary. Anyone picking the work up should be able to read this file plus the
specification and continue without asking questions that are already answered here.

## Where we are

- **2026-10-12 update — milestone M6: Task 10 is closed, reviewed and pushed.** T10 the sky,
  weather and fog carries (`6cc7df0`, fix `cead1f6`): the sunrise/sunset disc (the raw-centre
  fan), the `1 − rain` dim on the sun/moon alpha and the stars (the star brightness un-baked in
  `oxide-world`; the sky pass folds the rain), the sky-colour rain blend, the fog's rain and
  thunder branch (`EntityRenderer:1809-1825`), the water-fog light read fixed to the source's
  neighbour-brightness census (with the fix10 attribution correction), and the session's Sky
  re-report on weather movement — frozen-clock scenes track the weather. The first review
  returned fix-first (one important: the disc's centre baked the anaglyph greyscale in normal
  renders — fixed to the raw triple and re-reviewed approved, 0/0/0). The Step-1 carry rulings
  (the fog time term present, the horizon disambiguation, the void closure, the light-read fix)
  are recorded in `refs/m6-task-10/step1-rulings.md`. The next task is T11, the weather draw and
  the particle pass (plan line 386). HEAD `cead1f6` plus this update's docs commit. Carries: the
  light-census rows the behaviour table cannot express ride the next `behaviour.rs` touch. The
  full stop handoff is `docs/handoff/2026-10-12-m6-task-10-closed.md`.
- **2026-10-12 update — milestone M6: Tasks 8 and 9 are closed, reviewed and pushed.** T8 the
  audio engine, mixer and registry (`6354f51`) — reviewed approved, six minors deferred. T9 the
  music ticker and the local sound sites, with the client pump and the `--audio-dump` flag
  (`b9ddbd6`, fix `d0f9db5`: the first review's critical finding in the block sound groups, fixed
  by re-deriving every id of the map and pinning the step, splash, swim and pump conversions, the
  fix re-reviewed approved). Two plan-text pre-flight corrections landed (`b0febf0`, `4d04f41`);
  a CI repair followed (`c36b8b9` — the building jobs install the ALSA headers the audio stack's
  Linux backend needs). The next task is T10, the sky, weather and fog carries (plan line 354).
  HEAD `c36b8b9` plus this update's docs commit. The full stop handoff is
  `docs/handoff/2026-10-12-m6-tasks-8-9-closed.md`.
- **2026-10-12 update — milestone M6 (sound, particles, weather, screens) is in progress.** Tasks
  1–7 of the plan's thirty-one are closed, reviewed and pushed: T1 the carried GPU pins
  (`d03596a`); T2 the effects codecs (`3158df6`); T3 the settings store (`2ef4d6b`); T4 the render
  settings surfaces (`d9d6092`); T5 the session's weather, effect events and view distance
  (`2eee366`, fix `46aff1a`); T6 the sounds index (`d0f74ab`); T7 the particle engine (`5b01136`).
  Two plan-text corrections landed (`6f9f958`, `f0760dd`); the parallel wave's merged re-gate is
  green and its CI run `38209472618` is green on every job. HEAD `5b01136` plus this update's
  docs commit (workspace 2086 tests, 126 ignored). Carries bound for Task 8 (the audio engine):
  the `sounds.json` redirect entry (lend the target's weight; resolve at pick time) and the
  missing-`category` default; the particle engine's four minor findings ride the next task that
  touches `crates/oxide-game/src/particles.rs`. The full stop handoff is
  `docs/handoff/2026-10-12-m6-tasks-5-7-closed.md`.
- Stage: **milestone M5 (HUD and inventory) is complete and tagged `m5`** — spec section 13 of
  `docs/specs/oxidecraft-v1-design.md` (v6), plan `docs/plans/2026-10-08-m5-hud-and-inventory.md`
  (twenty-eight tasks plus the acceptance run's fix chain; base `025a211`; all closed and
  independently reviewed). The annotated tag marks this close-out commit; the reviewed code head
  is `8b49246`, and the final whole-branch review over the milestone — 70 commits,
  `a86f765..8b49246` (the frozen range from the `m4` tag; +61,734/−4,890, 98 files) — returned
  **approved**: 0 code must-fixes; the deferred triage 0 material / 22 landed / 6 closed as
  records / 38 open with named triggers / 4 splits; and two minor findings carried as the first
  items of the next window/GUI round (F-01 the stale HUD GPU pins, F-02 the compass/clock
  display poses; fix shapes in `refs/m5-task-28/final-review.md` §6). The task chain: T1 the slot
  NBT and the tree (`1c53c8c`); T2 the window, sign and experience decode (`a2d8230`); T3 the
  window, sign and creative writers (`42a3fc5`); T4 the player inventory model (`2031281`); T5
  the session window state and the inventory events (`95944a3`); T6 the container click machine
  and its actions (`e483176`, fix `5f4b8a2`); T7 the item model set (`9ce86f0`); T8 the item
  registry table (`a9792ab`); T9 the item sub-items, names and creative lists (`28ceb51`, fix
  `90096bc`); T10 the item sprites and the GUI sheets (`b0f8104`); T11 the HUD item icons
  (`abc7562`, fix `714b5b7`); T12 the first-person held item (`c0adafc`, fix `58d90d2`); T13 the
  held items and armour on entities (`5f046a3`); T14 the hotbar, the held-item popup and the
  crosshair (`9c71867`); T15 the health, food, armour, air and experience rows (`953a0b9`, fix
  `7a335d6`); T16 the screen framework and the container screen base (`47fe6dd`); T17 the item
  tooltips (`89dfd87`, fix `9c8e5f0`); T18 the chest, hopper, dispenser, furnace, brewing and
  crafting screens (`5a261c4`, fix `5fbe042`); T19 the beacon, enchanting, villager, horse and
  anvil screens (`4d39162`); T20 the inventory screen (`1f528b8`); T21 the creative inventory
  (`b8f235b`, fix `f135238`); T22 the sign editing and the sign text draw (`6cf3488`, fix
  `8f7d095`); T23 the book reader (`e6e673e`); T24 the inventory keys and the scripted gestures
  (`b3b79ab`); T25 the citation sweep and the extractor (`520bed4`); T26 the PNG-helper
  consolidation and the tRNS strip fixture (`1a28cff`); T27 the acceptance run (`0c09e3e` plus
  the fix chain `4238b20`…`8b49246`); T28 the close. M4 (entities and chat) is complete and
  tagged `m4` — the annotated tag marks its close-out commit `a86f765`, its reviewed code head
  was `103549e`, and its final whole-branch review returned approved. M3 (the player) is complete
  and tagged `m3` (close-out `35f2d21`, reviewed head `2276d22`, approved); M2 (textured terrain)
  complete and tagged `m2` (close-out `e90a30d`, reviewed head `38ac081`, PASS); M1 tagged `m1`
  (close-out `930ccab`, reviewed head `854ca85`); M0 tagged `m0`. Every task's brief, report,
  review and every ruling live in the milestone ledgers (local, git-ignored; the M5 ledger is
  the newest).
- **2026-10-02 update — M2 closed.** After the stop, the fluids round resumed and closed
  (`f366236`, reviewed clean); the full acceptance re-capture (continuations #3/#3b) followed; the
  residual round refuted the carried ~1 % frame-scale class on the current frames and fixed two
  real causes (`d66b597` variant world-position hash, `8f0bb5f` depth LEQUAL, reviewed clean); the
  terrain-shading round found and fixed the plain-cutout sampler cause (`2e3d962` — vanilla draws
  the plain `CUTOUT` layer at level 0 with a nearest sampler; ours sampled mips; reviewed clean);
  Task 15 closed with the trio committed (`54e51d4` the sample-wall cells, `82d3090` the parity
  tool and the baseline). The final whole-branch review over M2's 67 commits returned PASS. A CI
  clippy failure (the runner's newer stable `for_kv_map`, red since 2026-09-27) was found at
  close-out prep and fixed (`38ac081`; reviewed clean; CI run `36999029133` green, all seven jobs).
  **The acceptance state:** all three scene pairs fail the masked bar (wall 0.0217/0.0129, mark
  0.0152/0.0125, ground 0.1769/0.0814; bar over-8 ≤ 0.02, over-24 ≤ 0.01) with every class
  documented — the close proceeded with the classes documented per the owner's ruling. The
  milestone's record: `refs/rig/evidence/m2/acceptance-notes.md`, `refs/m2-terrain-shading/report.md`,
  and the round dirs under `refs/`.
- **2026-10-04 update — M3 closed.** The acceptance run finished with its recorded corrections in
  `refs/m3-acceptance/report.md` (the movement record-file fix; the creative leg's give-then-
  respawn ruling; the `shot.sh` mode fix; plus the completion addendum); controller verification
  re-derived every headline number from the artifacts, and the run's one real defect — a
  mid-session game-mode change going untracked (stale survival reach) — closed in the scoped fix
  `2276d22` (reviewed MET, no fix wave). The completion run added the accepted placement with the
  vanilla-visible before/after frames, the burst numbers (raw — the removal straddle was not
  observed), and the FPS release-vs-debug answer (the movement collapse is debug-build-specific).
  The final whole-branch review over the milestone returned **approved** (0 code must-fixes;
  31/31 deferred items safe-to-leave; the stage-4 crack evidence item completed by the close-out
  re-shoot). The milestone's record: `refs/m3-acceptance/`, `refs/m3-task-13-completion/`,
  `refs/m3-task-13-crack/`, `refs/m3-final-review/`, and the M3 ledger
  (local, git-ignored).
- **2026-10-05 update — M4 in progress; Task 17 interrupted.** Milestone M4 (entities and chat) is
  underway: tasks 1–16 are closed and pushed (head `00df68a`, CI green). Task 17 (the scoreboard
  sidebar and the below-name scores) has its recon complete; its first pass was interrupted on
  2026-10-05 mid-run, and that work is held uncommitted in the working tree (nine files,
  suite-green, pre-gate). The pause handoff
  `docs/handoff/2026-10-05-m4-task17-stopped.md` carries the resume steps: complete the first pass
  (gate, one commit, report), then the second pass, the combined review and the close, then
  Tasks 18–23 (the boss bar, the carry tasks, the acceptance run and the milestone close).
- **2026-10-07 update — M4 closed.** The acceptance run (Task 22) finished with its fix chain: the
  run's own machinery (`f59414d`) plus six scoped rounds and two comment passes (`0164f067` the
  snow layer, `1981603` the barrier's render kind, `8c3c36b` the scripted click, `d62db7b` the
  sidebar glyph run, `6ebc03d` the chat translation, `109439d` the barrier's translucent read,
  `1b20fb0`/`9cc477e` comment corrections), each RED at its layer, live-closed and gated
  standalone; the close commit `103549e` finalised the checklist rows and the plan's recordings.
  The validation legs settled the two open findings: the sneak range rule verified end-to-end on
  the wire (the s3 pair kept as the finding's record), and the persistence return re-ruled as a
  server-side intermittent stall (wire-proven; the client faithful — a valid take settles
  collision at +0.5 s and renders by +300 s). The final whole-branch review over the milestone
  returned **approved** (0 code must-fixes; the deferred triage 0 material / 2 landed / 35
  re-carried / 50 open with triggers; two record/route minors carried by the sweep notes). The
  milestone's record: `refs/m4-acceptance/`, `refs/m4-task-22/` (the fix chain and the validation
  legs), `refs/m4-task-23/` (the frozen archive and the final review), and the M4 ledger
  (local, git-ignored).
- **2026-10-11 update — M5 closed.** The acceptance run (Task 27) finished with its fix chain:
  the combined live pass (legs L1–L10) with its continuations, the bounded leg batch, two
  close-smoke legs and eleven scoped fix rounds (`4238b20` the HUD projection, `8dc1303` /
  `7767c1a` / `2d9076c` the underwater overlay and the water fog, `8b7cf87` the use-click reach,
  `316a95b` the modelled blocks, `cc1c790` the container panels and slots, `2209895` / `3e9675e`
  the creative screens, `ad92adc` the book open, `31e9d49` the water-fog falloff, `d53b7f2` the
  window drag chain, `c0f3751` the water-fog colour's eye-light term, `8b49246` the pickup-hold
  regression pins), each RED at its layer, live-closed and gated standalone; the FINISH commit
  `0c09e3e` finalised the checklist rows and the plan's recordings. The run's open questions
  settled by bounded live discriminators: the sign editor never opens under scripted aim (a
  server-side condition; no port verdict) and the enchanting pose divergence is a staging
  geometry effect, not a port defect (both recorded in `refs/m5-task-27/legbatch/report.md`);
  the pickup-hold render residual was falsified as a frame misread (the carried cursor over the
  just-emptied cell) and pinned by two regression tests (`8b49246`). The final whole-branch
  review over the milestone returned **approved** (0 code must-fixes; the triage 0 material / 22
  landed / 6 closed as records / 38 open with triggers / 4 splits; the two minors — F-01, F-02 —
  carried as the next window/GUI round's first items). The milestone's record:
  `refs/m5-task-27/` (the live pass, the leg batch and the close-smokes with their reports,
  briefs and wire captures), `refs/m5-task-28/` (the frozen archive and the final review), and
  the M5 ledger (local, git-ignored).
- M0 delivered and verified: the eight-crate workspace with its enforced dependency graph; the
  VarInt codec and length-prefixed framing with the 1.8 compression rules; handshake, status ping
  and the launcher CLI; the hash-verified atomic store; piston-meta metadata parsing; the full
  `fetch --verify` flow, run against the real endpoints; jar extraction with a manifest; the wgpu
  window with the FPS counter and adapter logging; CI with the crate-graph, asset and licence
  guards and the release build.
- M1 delivered and verified: the buffered framed connection with the compression handover; the 1.8.9
  login and play packet codecs; the live capture with its findings document and chunk fixtures; the
  `0x21`/`0x26` column decoder; the chunk store; the session with the five connection obligations;
  the block palette and the section mesher; the terrain types and camera; the wgpu terrain pipeline
  with the depth buffer and the text overlay; the client's `--server` wiring with the F3-style
  overlay; and the negative self-tests for the asset and crate-graph guards, wired into CI.
- Tests: 1873 passing workspace-wide, 121 ignored, zero failures at the M5 reviewed head
  (`cargo test --workspace`; `#[test]` 1994 − 121 `#[ignore]`; the close's gate re-run EXIT 0,
  `refs/m5-task-28/close-gate.log`; the milestone's base `a86f765` reads 1322 passing / 0 failed
  / 69 ignored, 1391 − 69). The ignored set at the close is 121 — the render GPU cases (103; the
  M5 item-icon, container-panel, HUD and creative-screen families included; they pass locally on
  the T500 — two HUD/creative pins stand red at the close, F-01, carried to the next window/GUI
  round), the store-dependent and live-endpoint cases in `oxide-assets` (9), `oxide-client`
  (5) and `oxide-game` (3 — the barrier, behaviour-store and snow-layer headless cases), and the
  sample-wall rig helper in `oxide-world` (1); the store cases run when `OXIDECRAFT_STORE` points
  at the store (`~/.local/share/oxidecraft`).
- Live evidence: the M0 ping and captures stand; M1's acceptance run — the join, 180 seconds of
  keep-alives, the mark and both screenshots, and the proxy capture of our client's own traffic — is
  recorded under "M1 evidence" below, with the raw files under `refs/rig/evidence/m1/`. M2's live
  evidence is the three acceptance scene pairs, their masks and the parity JSONs under
  `refs/rig/evidence/m2/`, with the round chains and reports under `refs/m2-fluids/`,
  `refs/m2-acceptance-residual/`, `refs/m2-terrain-shading/` and `refs/m2-task-15/`. M3's live
  evidence is the acceptance run (`refs/m3-acceptance/`: the movement vectors, the creative leg,
  death/respawn, the stream spot-check and the latency proxy with its raw numbers), the completion
  run (`refs/m3-task-13-completion/`: the accepted placement with the vanilla-visible frames, the
  burst frames, the FPS A/B) and the close-out crack capture (`refs/m3-task-13-crack/`) — numbers in
  "M3 evidence" below. M4's live evidence is the acceptance run (`refs/m4-acceptance/`: the scene
  frames and the report, the console and tick logs, the wire captures) and the fix chain's round
  evidence with the validation legs (`refs/m4-task-22/`: the re-shoots and their archives, the
  discriminator and v7 reports, the e1/e2 wire decodes) — numbers in "M4 evidence" below. M5's
  live evidence is the combined pass and its continuations (`refs/m5-task-27/livepass/`: the
  checklists, frames and wire captures for legs L1–L10), the leg batch
  (`refs/m5-task-27/legbatch/`: the enchanting, sign, R5 and beacon discriminators) and the
  close-smokes (`refs/m5-task-27/closesmoke/`, `refs/m5-task-27/closesmoke2/`: the fix7/8/9 and
  fix10/11 closes, with the pinned client's md5 recorded) — numbers in "M5 evidence" below.
- CI: seven jobs — format/lint/test, MSRV 1.85.0, portability (`x86_64-pc-windows-gnu`), release
  build, crate graph, asset guard, licences and advisories. The M0 tagged head `fe2d3b6` is green in
  run `35873490870` (six jobs; the release build was added after the tag). The M1 reviewed head
  `854ca85` is green in run `36288859598` (all seven jobs), and the M1 close-out commit `930ccab`
  (the commit the `m1` tag marks) is green in run `36300166552` (all seven jobs). The M2 reviewed
  head `38ac081` is green in run `36999029133` (all seven jobs), and the M2 close-out commit
  `e90a30d` (the commit the `m2` tag marks) is green in run `36999837908` (all seven jobs). **M3:**
  every checkpoint was CI-checked as it landed; the pre-fix head `de1baea` — the parent of the
  milestone's fix commit — is green in run `37177842220` (all seven jobs), and the close-out
  commit `35f2d21` (the commit the `m3` tag marks) is green in run `37195043678` (all seven
  jobs). Caveat from the run: between 2026-09-27 and 2026-10-02 main was red (the runner's
  newer stable clippy, `for_kv_map` at `crates/oxide-assets/tests/atlas.rs:178`; the local stable
  was six releases behind) — fixed by `38ac081`; keep the local stable current. Caveats to carry forward:
  `aarch64-apple-darwin` is not checked on Linux runners because its C dependencies need the macOS
  SDK; the MSRV job installs the 1.85.0 toolchain per run; `cargo-deny` is pinned to 0.20.2 and
  installed per run. The M0 caveat about the guard scripts being exercised only on the happy path is
  closed: both guards carry `--self-test` modes since `854ca85`, and CI runs each one before its
  normal check. **M4:** every checkpoint was CI-checked as it landed; the T22 chain's pushed head
  `103549e` is green in run `37711182425` (all seven jobs), and the close-out commit `a86f765`
  (the commit the `m4` tag marks) is green in run `37713937089` (all seven jobs). **M5:** every
  checkpoint was CI-checked as it landed; the pushed head `ad92adc` — the parent of the
  acceptance fix chain — is green in run `38088487115` (all seven jobs); the close-out commit `8ae733d`
  (the commit the `m5` tag marks) is green in run `38127359527` (all seven jobs).
- Review: `docs/reviews/2026-09-22-spec-review.md`, with a disposition record for every finding; the
  per-task reviews, the fix-round reviews and each milestone's final whole-branch review are in the
  milestone ledgers and round dirs (local, git-ignored: M2's in
  `refs/m2-final-review/review-report.md`; M3's in its milestone ledger,
  including the approved `final-review.md`; M4's in `refs/m4-task-23/final-review.md`, approved —
  0 Critical / 0 Important; M5's in `refs/m5-task-28/final-review.md`, approved — 0 Critical /
  0 Important, with the two minors F-01/F-02 carried to the next window/GUI round).
- Research: five evidence-backed reports in `docs/research/`, indexed in Appendix B of the spec.
- Parity checklist classification: `docs/parity/checklist.md`.
- Verification rig: under `refs/rig/` (offline-mode 1.8.9 server plus a vanilla client, see
  `refs/rig/README.md`).
- Repo: https://github.com/loofyser/Oxidecraft — `main` pushed and tagged (`m0`, `m1`, `m2`, `m3`, `m4`, `m5`), with
  the M5 chain pushed through the close-out this record closes. Confirm HEAD with `git log --oneline -3`.

## M0 evidence

Acceptance run: `fetch --verify`, real endpoints, default store at `<data dir>/oxidecraft`, debug
build (`cargo run -p oxide-launcher -- fetch --version 1.8.9 --verify`), 2026-09-23:

```
fetch complete: 726 downloaded, 0 reused, 123543629 bytes transferred
verify: 722 objects, 0 mismatched, 0 missing, 114708537 bytes on disk
```

- The 1.8 index lists 734 entries over 722 distinct hashes (12 entries repeat a hash), so the
  store holds 722 object files; the jar, index, version document and manifest are the other
  transfers. Wall clock: 19 seconds (09:52:10Z to 09:52:29Z). The store's client jar re-hashes to
  `3870888a6c3d349d3771a3e9d16c9bf5e076b908`, the pinned constant.
- Second run, same command: `fetch complete: 0 downloaded, 725 reused, 0 bytes transferred` in
  1.92 s, with the same clean verify. A run on a warm store downloads nothing.
- Dry run on an empty store: `dry run: 0 file(s) already present, 723 file(s) to download,
  123170021 bytes` (722 objects plus the jar); only the empty store directories are created.
- Post-review fix (2026-09-23): the store lock is now an operating-system lock over
  `<store root>/lock`, held for the run and released when the process dies, so a killed run cannot
  lock out the next one; the file itself stays behind with the last run's process id in it.

Acceptance run: jar extraction, the real client jar already in the store, default store at
`<data dir>/oxidecraft`, debug build (`cargo run -p oxide-launcher -- fetch --version 1.8.9`),
2026-09-23:

```
fetch complete: 0 downloaded, 725 reused, 0 bytes transferred
extraction: 5597 entries read, 3085 extracted, 2512 skipped, 4553815 bytes
```

- The 1.8.9 client jar holds 5,597 entries: 3,085 under `assets/`, 2,507 `.class` entries, 3 under
  `META-INF/` and 2 other root files (`pack.png`, `log4j2.xml`). The extractor takes the 3,085 and
  refuses the other 2,512, and it reads no class entry at all. The 3,085 matches the survey's
  census (`docs/research/launcher-assets-auth-survey.md:272`), and an independent pass over the
  extracted tree found every file byte-identical to its jar entry with no `.class` or `META-INF`
  path present.
- The manifest at `extracted/1.8.9/.manifest.json` records 3,085 entries totalling 4,553,815 bytes,
  the jar SHA-1 `3870888a6c3d349d3771a3e9d16c9bf5e076b908` (the pinned constant) and schema version
  1. The jar carries no root `pack.mcmeta` or `sounds.json`; the 1.8 sound index comes from the
  asset objects (`minecraft/sounds.json`), so those two include-rule entries match nothing in this
  jar.
- Extraction took 2.3 s on the first run (10:19:35Z to 10:19:37Z); a second run reports
  `extraction: up to date, nothing written` in under a second, and `fetch --verify` after the
  extraction still reports a clean store (722 objects, 0 mismatched, 0 missing).

Acceptance run: wgpu window, a real window on this machine's GNOME/Wayland desktop, debug build
(`cargo run -p oxide-client`, with `OXIDECRAFT_MAX_FRAMES=900` bounding the smoke run),
2026-09-23 10:45:11Z to 10:45:26Z:

```
GPU adapter selected adapter=NVIDIA T500 backend=Vulkan driver=NVIDIA driver_info=615.71.09 device_type=DiscreteGpu vendor_id=4318 device_id=8123
surface configured format=Bgra8UnormSrgb srgb=true width=1280 height=720 present_mode=Fifo
renderer ready adapter="NVIDIA T500"
frame rate frames=60 fps="62.1"
frame rate frames=120 fps="60.0"
frame rate frames=300 fps="59.3"
frame rate frames=600 fps="60.0"
frame rate frames=900 fps="59.8"
frame limit reached, exiting frames=900
client exiting frames=900
```

- The window title carries the live rate and the adapter name: `Oxidecraft — 60 fps — NVIDIA T500`.
  The capture is `refs/rig/evidence/m0-window.png` (whole-desktop shot; the 1280x720 client window
  is the pale sky-blue rectangle, clear colour 0.62/0.76/0.98 written through the sRGB surface
  format). The run log is `refs/rig/evidence/m0-window-run.log`.
- The instance asks for Vulkan only. Two GPUs are present (Intel Iris Xe and NVIDIA T500) and the
  discrete NVIDIA T500 was selected with driver 615.71.09 and device id 8123; this is the appendix
  C.1 device record for later parity comparisons. An X11/XWayland run of the same binary (with
  `WAYLAND_DISPLAY` unset) reached the same title and a steady 60 fps, so both display paths work.
- Escape and window-close exits were not exercised end to end: window enumeration is
  unavailable on this desktop and synthetic key events are not delivered to the XWayland client.
  The run above exits through the frame limit, which reaches the same `event_loop.exit()`; the
  escape rule itself is unit-tested.
- Post-review fix (2026-09-23): a stale surface (`Outdated`, `Lost`) is now reconfigured and the
  frame retried once instead of stopping the client, a `Timeout` frame is dropped, the error logs
  carry the full cause chain, and the clear colour's sRGB difference is recorded in
  `docs/DIVERGENCES.md` (entry 4).

## M1 evidence

Acceptance run: the client against the rig server, `./target/debug/oxide-client --server
127.0.0.1:25565 --username OxideDev`, 2026-09-26 (evening, CDT; log timestamps are UTC), raw files
under `refs/rig/evidence/m1/`:

```
[20:55:45] OxideDev[/127.0.0.1:58520] logged in with entity id 3757 at (20.5, 64.0, 174.5)   (server)
2026-09-27T01:55:46.086719Z  INFO oxide_client: joined the world entity_id=3757 gamemode=0 dimension=0 …
2026-09-27T01:55:48.058929Z DEBUG oxide_client: keepalive answered id=22184315                (client;
2026-09-27T01:58:44.209128Z DEBUG oxide_client: keepalive answered id=22360615                 87 lines)
```

- Joined the world as entity 3757; **87 keep-alive echoes over 180 s** (~2.05 s apart) with no
  server-side kick, then stopped deliberately (the server logs the stop as `Disconnected`, not a
  fault).
- The mark: `/fill 9 140 172 21 140 184 minecraft:wool 14` and `/fill 9 141 172 21 145 184
  minecraft:stone` placed a 13×13 stone column over its red-wool base above y=128; `/tp OxideDev 31
  150 194 135 20` teleported the client onto it (`Teleported OxideDev to 31.5, 150.0, 194.5`), and a
  second teleport (`15 71 178 0 0`) returned it to the ground (15.5, 71.0, 178.5).
- Screenshots (niri's own focused-output capture; see the Environment facts): `terrain-both-halves.png`
  — the mark as a grey stone block over the red wool base, grass, dirt, water, sand and trees below,
  the overlay legible with `x/y/z: 31.500 / 150.00000 / 194.500`, `Facing: north (135.0 / 20.0)`,
  `Server: 127.0.0.1:25565 (protocol 47)` — and `terrain-ground.png` (overlay `x/y/z: 15.500 /
  71.00000 / 178.500`). Both show geometry above and below y=128, with no magenta. The vanilla client
  was teleported to the same two spots for eyeball comparison (`vanilla-on-mark.png`,
  `vanilla-ground.png`).
- Our client's own traffic (recorder proxy on 25565, server behind it on 25566, non-loopback upstream
  so the server negotiates compression; `oxide-client-traffic.*`): the handshake
  `0f 00 2f 09 "127.0.0.1" 63 dd 02` (protocol 47, next state 2); `0x03 Set Compression threshold=256`
  **before** `0x02 Login Success`, the client's first compressed frame being #2; the brand payload
  `07 76 61 6e 69 6c 6c 61` (`\x07vanilla`); Client Settings `en_US`, view distance 8; the position
  echo; and **56/56 keep-alive echoes, none missing, none unmatched, median interval 2.0499 s**.
- The capture's chunk findings: the rig's vanilla server **does emit `0x26`** — run A 9×`0x21` +
  44×`0x26` (414 columns), run B 2×`0x21` + 42×`0x26` (416 columns); our client's own session
  received 1×`0x21` + 43×`0x26` (431 distinct columns, zero size-formula mismatches). The findings
  document is `docs/research/protocol-47-live-capture.md` and the committed fixtures are under
  `crates/oxide-proto-v47/tests/fixtures/m1-capture/`.
- Two defects the acceptance run uncovered, both fixed on `main` with covering tests: `8b14e4e`
  (keep-alive starvation — the mesh rebuilds ran inside the read loop; they are now queued and
  drained between reads) and `0187ae8` (palette coverage — 22 block ids the rig's world carries had
  no palette entry).

## M2 evidence

Acceptance: the three scene pairs (ours vs the vanilla client on the rig, placed per appendix C),
measured by `scripts/parity-diff.py`; bar: over-8 <= 0.02, over-24 <= 0.01 of the frame, masked per
the derived rect sets (each rect justified and census-checked; never widened). Poses: wall
`/tp 7 57 174 180 0`; mark `/tp 31.5 150 194.5 135 20`; ground `/tp 15 71 178 0 0`; every capture
with the scene setup + `/time set 6000` + `/weather clear` re-sent and both clients soaked past the
mesh drain.

- Final numbers (masked over-8 / over-24; raw figures in the parity JSONs): **wall 0.0217 / 0.0129**,
  **mark 0.0152 / 0.0125**, **ground 0.1769 / 0.0814** - all three fail the bar as written; every
  class is documented in `refs/rig/evidence/m2/acceptance-notes.md` (the round-4 top section carries
  the close note; earlier sections keep their history with correction notes). The close proceeded
  with the classes documented per the owner's ruling (2026-10-02).
- The documented classes at the close: the wall's right-slope edge (close-range plants + the cloud
  band; the sampler cannot act at level 0 - recorded open), the chest cell (11, 58, 164; the missing
  tile-entity path; not masked), the fluids' static phase (ours draws the animation's first frame;
  the floor is measured and named), the ground's remaining tallgrass/terrain-shading residue (the
  sampler fix moved the in-region class 0.3307/0.1796 -> 0.2801/0.1330), and the mark's
  deck-over-terrain phase overlap (attributed, not masked).
- The wall, for the record: 122/122 cells checked across start and teardown
  (`refs/rig/tools/sample_wall_cells.txt`).
- The metric's tool: `scripts/parity-diff.py` (committed with the baseline; `--self-test` runs in
  the gate, 19 checks).
- Baseline (`docs/perf.md`, the C.4 route, build `72526bf`): our client mark 10.4 / wall 12.2 /
  ground 5.6 / far end 15.2 fps frames-delta; VmRSS flat at 515-534 MiB across the route; cold
  start 7.7 s; the vanilla client's memory 1.19-1.22 GB and its frame rate recorded as M9's to
  complete (no working input path for the F3 readout on this desktop). M9 re-runs the full protocol
  on the final build.
- Atlas on the acceptance run: 2048x2048, 5 levels, 377 sprites (`oxide-client-capture2.log:2`).

## M3 evidence

Acceptance: the Task 13 run (`refs/m3-acceptance/report.md` — original + the correction blocks +
the completion addendum + the crack addendum) on the rig (server + our debug client; two clients
for the creative leg), recorded 2026-10-03/04. The headline numbers, all re-derived from the
artifacts at verification:

- Movement (scripted, from the `*.script.log` tick logs): walk **4.317181 m/s** (dev 0.004 %),
  sprint **5.612335** (0.006 %), jump apex **1.249187** (t269–270) — against the design vectors
  4.317 / 5.612 / 1.24919 within the ±0.1 % / ±0.01 tolerances.
- The creative leg (give-on-live rule): placement accepted server-side — console probe
  `Successfully found the block at 99,71,102.`, exactly one 0x08 (frame #1495, target (100,71,102)
  west, cursor [0,9,8]); the vanilla client's before/after frames show the edits
  (`refs/m3-task-13-completion/vanilla-placed-{before,after}.png`).
- Death/respawn: `/kill` → death view → scripted respawn; no stale dig/aim; the respawn-cleared
  hand is the recorded give-then-respawn class (known-limit 9).
- The serverbound stream spot-check (recorder proxy, one combined script): the packet counts
  reproduce from the capture (`verify_spotcheck.py`; movement cadence, the dig [status 0, status
  2], sprint 0x0B edges, one 0x08, 0x16 after the place; chain dig #848 < place #1073 < respawn
  #1781).
- The latency proxy: stage timeline at 0.75 s spacing (20 Hz same-pass cadence); the removal
  straddle was not observed — all burst frames land 0.02–0.03 s pre-removal inside the fixed
  window (reported raw; fine-grained measurement is M9's full protocol).
- The crack overlay (close-out re-shoot, final-review item E1 — produced, not waived): stages 0→8
  landed + completion at ~0.75 s cadence; mid-dig frames `our-crack-stage-1..7.png` with the
  calibrated dark-share rising over the target face (baseline 5.74 → 11.25 / 25.24 / 31.24; the
  last three post-completion); wire check one dig start + one finish at (100,71,100) west, 0
  failures; probes stone → air. The re-shoot re-posed to the acceptance's own vanilla crack pose
  (`vanilla-crack-pose.commands`, 3.64 blocks) after the original step-4 sightline (~4.6 blocks)
  proved outside survival reach (4.5) live; the target cell and scene are the acceptance's.
  `refs/m3-task-13-crack/` carries the frames, the target-region crops (the operator's by-eye
  aid), the metrics JSON and the wire JSON; the step-4 sprite identity is separately proven by
  Task 12's GPU case.
- The FPS side question: under the identical movement segment the debug build collapses (moving
  min/median/max 6.9/80.8/131.9) where the release build holds a flat ≈144 (95.7/144.1/224.3);
  idle 74.7/126.0/193.3 vs 117.3/144.5/167.2 — debug-build-specific; the counter is honest
  (`refs/m3-task-13-completion/fps/`).

The defect the run surfaced — a mid-session game-mode change going untracked (stale survival
reach) — closed in the milestone's scoped fix `2276d22` (reviewed MET, 0C/0I/2M; the two cosmetic
minors went to the final review). The operator's by-eye list (the vanilla placement frames, the
death-view frame, the crack crops) stands as listed under Caveats.

## M4 evidence

Acceptance: the Task 22 run (`refs/m4-acceptance/report.md` — the scenes, the close notes and the
re-shot canonicals) on the rig (the vanilla client and ours, both connected; the recorder proxy
for the wire legs), recorded 2026-10-06/07. The headline numbers, all re-derived from the
artifacts at verification:

- The mutual view and the nametag measurement: the name's ink 49×8 px at 12 blocks (the source's
  scale), the background alpha mean 0.249 over 12 channel reads (source 0.25), the above-head
  offset ≈0.29 blocks (model 0.2867); the range rule (64 standing / 32 sneaking, strict and
  squared) verified end-to-end on the wire — the client's `0x0B` sneak edges, the server's
  relayed `0x1C` metadata +7/+5 ms later, and the re-shot pair reproducing the branches (sneak
  33 absent; sneak 31 plate + faint only; standing opaque).
- The zoo's crop metric: 3 of 20 crops pass (slime, XP orb, painting) with every remaining crop's
  cause recorded — the cow's checkerboard was a snow layer's fallback cube (`0164f067`), the
  barrier's magenta wall was its render kind (`1981603`), and the burns and the crosshair are
  scene confounds.
- The motion leg: the marker's `/tp` steps land as discrete positions (100.5→102.5→104.5→112.5
  at ticks 11096/11180/11261; the 8-block step as a one-tick jump past the 4-block rule).
- The chat scenes: the send proven server-side; the click proven end-to-end (`[§cOxideDev§r]
  clicked` — the announcement format with the player sender; the tooltip renders); the styled rows
  re-shot after the translation fix (`6ebc03d` — the say row 4518 px of ink where the standing
  frame measured 0).
- The HUD surfaces: the sidebar's red-number probe 1216 on the re-shot canonical (pre-fix 21
  kept as the archive; vanilla 4166); the boss bar's fraction rendered from `HealF` (261.0f —
  ≈87 % at capture); the tab list's entries, heads and latency bars (the vanilla Tab capture
  unobtainable on this desktop, as predicted).
- The persistence legs: the away leg recovers within 30 s (the pre-drain mesh lag); the return's
  empty render in stalled takes is the server-side chunk-send stall (wire-proven: zero load
  columns after the return; a stalled shift poisons its session), and in a valid take the client
  settles collision at +0.5 s and renders the scene by +300 s (corr 0.965) — the client is
  faithful; no fix owed (`refs/m4-task-22/validation-v7-report.md`).

The defects the run surfaced — the snow layer's fallback cube, the barrier's render kind, the
scripted click's cursor space, the sidebar's blended glyph run, the chat translation components,
and the barrier's translucent read — closed in the milestone's scoped rounds (`0164f067`,
`1981603`, `8c3c36b`, `d62db7b`, `6ebc03d`, `109439d`; each RED at its layer, live-closed, gated
standalone), with two comment-only corrections (`1b20fb0`, `9cc477e`). The operator's by-eye list
(the acceptance's frames) stands as listed under Caveats.

## M5 evidence

Acceptance: the Task 27 run (`refs/m5-task-27/` — the combined live pass on the rig, its two
continuations, the bounded leg batch and the two close-smoke legs; the vanilla client and ours,
both connected; the recorder proxy on `:25566` for the wire legs), recorded 2026-10-09/11. The
headline numbers, all re-derived from the artifacts at verification:

- The HUD surfaces live: the F2 screenshot leg's strip diff 0.07 % / 0.00 % (satpx
  `[0,531,962,153,771,63,990,201,0]`); the hotbar's held-item popup, the selection swap and the
  wheel scroll; the health/food/armour/air/XP rows (the poison-mirror correction — 22 vs 9 —
  the blink phase and the XP level text); the crosshair.
- The container surfaces live: the inventory with the crafting grid; the chest, furnace and
  crafting table; the creative inventory's six windows (the tab labels `Building Blocks` /
  `Search Items` resolved, the compass icon, the 45/45/47-cell textured grids over the pre-fix
  6); the beacon's in-reach activation (the C08 at `(115,72,113)` face 1 from the pose
  `(115.5,75,115.5,180,61)`); the villager and horse trades (the trade-list payloads
  2120/2434; the `S2D` fires and the horse's ride leg).
- The drag and click machine: the two close-smoke legs' wire and pixel records — the
  `win2/slot0` pickups (`act1`/`mode0`, the stone-16 echo), the `1+1+1` mode-5 chain, the
  `S32 5/5 accepted=1` responses, no C0F, zero post-accept `S2F`s; the held icon at the pointer
  (`dx 0` / `dy +3`, the wash ring, no tooltip while carried — matching the reference's guard).
- The wire counts: the combined pass's protocol legs (the `C10 ×48` window stream: the set-36,
  delete-36 + set-38 and the 0-44 clear), the book chain (the reader's `Page 1 of 3` /
  `Page one alpha.`; the sky-aim all-FF `C08 ×2` followed by the `MC|BOpen ×2` sends), and the
  enchanting discriminator's crossing sequence (`z 115.99 / 115.26 / 115.33`).
- The underwater pair (the S2 close-smoke re-measure): the opening-region factors
  `1.008/1.000/1.095` (vanilla B stable across sessions; shift-robust B `1.093–1.098`), the
  dark-edge cells' B `−3.6` fog-direction against a bit-identical vanilla side, and the
  documented in-band residual (`dB +10.0` against the predicted `+20.9`; the light-read nuance
  recorded under Caveats).
- The persistence and hygiene legs: the keepalive soak rates on cadence; the L10 hygiene sweep;
  the vanilla-control legs (the furnace smelt and the horse); the R7 burn closed with the
  recorded `BurnTime 1480` seconds.

The defects the run surfaced — the HUD projection's fractional scale, the underwater overlay and
the water-fog branch, the use-click reach, the modelled blocks' fallback cube, the container
panels and slots, the creative screens' labels/icons/contents, the book open, the water-fog
falloff and its colour's eye-light term, and the window drag chain — closed in the milestone's
scoped rounds (`4238b20`…`8b49246`; each RED at its layer, live-closed, gated standalone). The
pickup-hold render residual was falsified as a frame misread and pinned by two regression tests
(`8b49246`). The operator's by-eye list (the acceptance's frames) stands as listed under
Caveats.

## Caveats

Carried by M1 as delivered (each confirmed by the final whole-branch review):

- **Set Compression arriving after Login Success is unhandled by construction.** The session switches
  to the play state on Login Success, so a late `0x03` lands in the play wildcard and fails cleanly.
  A heuristic cannot distinguish it from a play-state Time Update on a server that legitimately runs
  with compression disabled; the capture and the rig fix the order as "before". A modded server that
  reorders compression breaks with a clear error — M1's scope is the rig.
- **An unload does not re-mesh neighbours**: a column that unloads can leave transient border holes
  in a loaded neighbour's mesh until that neighbour next rebuilds.
- **Meshing is single-threaded** and runs on the session thread between reads; applying a column
  rebuilds that column plus its four neighbours.
- **A server that never leaves a ≥20 ms idle gap starves the mesh queue** (unbounded growth): the
  idle read waits at most one 20 ms tick and drains one batch per wait. Fine for the rig; recorded
  for M2.
- **The debug font is scaffolding** (Decision 7): a small embedded 5×7 bitmap table, one opaque quad
  per set pixel; the jar's font (`ascii.png`) replaces it in M2.
- **Water draws opaque**, like every block: M1 renders flat colours with no transparency or light.
- **No live uncompressed-play capture exists on this rig**: run A's loopback premise was refuted (the
  server negotiated compression over the proxy anyway). Uncompressed framing is covered by the login
  phase, every sub-threshold frame and the synthetic self-test.
- **The acceptance run's raw server-console output was not preserved**: the rig's log rotation keeps
  only `latest.log` plus 2-line gz archives, so the acceptance note's console quotes are corroborated
  by the client log, the screenshots and the capture, but they cannot be re-verified from the
  artifacts (see the backlog: fix the rotation).

Carried by M2 as delivered (each confirmed by the final whole-branch review):

- **Fast graphics only** - the option that flips the setting, the cloud prisms and the per-corner
  tint ride with the options screen (M6); the acceptance and every M2 session ran Fast.
- **No weather** - rain strength is a parameter fixed to 0 until the weather packets land in M6;
  the rain-darkened fog and sky are unverified.
- **The sunrise/sunset band is not implemented** - the acceptance captures at noon where it is
  invisible; M6's sky work adds it with a dawn capture.
- **Clouds' offset rule** - the client-local counter is the documented stand-in for the missing
  tick loop; the cloud band's masking in the metric is derived and recorded.
- **Translucent sorting is per section and static within a section** - a large water surface may
  show ordering artefacts the metric does not chase.
- **The atlas is rebuilt at startup** - R3's "no atlas rebuild" clause needs a cache, which is
  M9's.
- **The baseline departs from C.4** (no warm-up, no median; vanilla's frame rate deferred to M9)
  and **the capture path departs from C.2 step 3** (the KWin/spectacle route replaces the portal) -
  both recorded with their reasons in `docs/perf.md` and the rig notes.
- **The uinput HUD-hide attempt failed on this desktop** (no virtual-keyboard protocol for KWin;
  `/dev/uinput` not usable non-interactively) - recorded in the plan and the rig notes; our own
  captures run `--no-overlay`.
- **The acceptance residue** - all three pairs fail the masked bar as written; every class is
  documented in `refs/rig/evidence/m2/acceptance-notes.md`, and the close proceeded with the
  classes documented per the owner's ruling (2026-10-02).
- **The M1 caveats this milestone closed**: unload re-meshes neighbours and meshing runs on a rayon
  pool (Task 13); the mesh-queue starvation bound (Task 13 + the keepalive fix `6959280`); the
  debug font (Task 14); water draws opaque (Task 9).

Carried by M3 as delivered (each confirmed by the final whole-branch review's criteria walk,
its milestone ledger):

- **The interactive Escape / mouse-capture-release live half is a documented non-pass**: no
  synthetic input path reaches KWin (the uinput attempt failed; `wtype` cannot connect), so the
  live half of that acceptance row rests on the unit-tested rule and the operator item below —
  recorded at the close rather than silently passed.
- **The crack overlay's live frames come from the close-out re-shoot** (final-review E1 — produced,
  not waived): the acceptance's own Step-4 sightline is ~4.6 blocks, outside the 4.5 survival reach
  (live-proven in the re-shoot), so it re-posed to the acceptance's own vanilla crack pose on the
  same target cell (numbers in "M3 evidence").
- **Crack stages are keyed by block position** — M4's breaker-id work re-keys the map.
- **`attacked_at_yaw` stays zero** until M4 tracks attackers (hit direction rides that).
- **The overlay wiring (aim target + stage map) has no wiring-level pin** — recorded for the next
  overlay touch.
- **Sprint-release-on-collision is not expressible against `physics::step`'s current return** — the
  collision-flag API change rides M4.
- **The sneak-eye offset (−0.08) is unwritten** and the plan's pin still says flat — the clause and
  the pin amendment are M4 items.
- **The latency proxy's removal straddle was not observed** (frames land 0.02–0.03 s early inside
  the fixed window) — reported raw; fine-grained measurement is M9's full protocol.
- **The replaceable refusals** (snow layer 78, vine 106, fire 51, the double-plant's rose side):
  plan known-limit 16.
- **The operator's by-eye list** (cannot be closed agent-side): the vanilla placement frames
  (`refs/m3-task-13-completion/vanilla-placed-{before,after}.png`), the death-view frame
  (`refs/m3-acceptance/our-death-view.png`), and the close-out crack crops
  (`refs/m3-task-13-crack/our-crack-*-targetregion.png`).

Carried by M4 as delivered (each confirmed by the final whole-branch review,
`refs/m4-task-23/final-review.md`):

- **The neighbour-brightness light term is not modelled** (pre-existing; the source's
  `registerBlocks()` bootstrap sets it live) — immaterial to the milestone's noon-flat scenes; the
  comments state the source's loop and the port's scope; the future fix shape and RED are recorded
  (`refs/m4-task-22/fix-a2b-micro2-report.md`), with the `light.rs`:123 cite drift riding the same
  carry.
- **The server-side chunk-send stall is a rig condition, not a client defect** — stalled takes
  send zero load columns after a teleport-driven unload (observed intermittently; a stalled shift
  poisons its session). A stalled take must be re-taken on a fresh session and validated by the
  wire before its frames are trusted.
- **The operator's by-eye list** (cannot be closed agent-side): the acceptance's frames per the
  report's by-eye list, plus M3's carried operator items (the vanilla placement frames, the
  death-view frame, the crack crops, the interactive Escape live half).
- **The comment/citation sweep set** (D1–D9, D27, `view.rs`'s stray backtick at `:1657`, the
  `light.rs`:123 drift, the twelve-site `WorldClient.java:468-483`/`:468-482` family and
  `clientbound.rs`:222) rides the next comment sweep; the sweep-tooling round (the extractor's
  range-inside grammar) stands as its own item.
- **`world_scan.py`'s broken column index stays barred** from acceptance paths until re-derived.
- **The checklist rows rest on their recorded evidence generations**: the pre-fix frames and
  readings stay as the findings' record (the sidebar's 21-px archive; the scene-4 empty strips;
  the s3 sneak pair), the fixes are labelled follow-ups, and the re-shot frames are marked as
  later generations.

Carried by M5 as delivered (each confirmed by the final whole-branch review,
`refs/m5-task-28/final-review.md`):

- **Two ignored GPU pins stand red at the reviewed head** (the review's F-01 and F-02, carried
  as the next window/GUI round's first two items): F-01 — the HUD/creative overlap pins encode
  the pre-fix `50·(index+1)` rung rule rather than the shipped constant-50 `icon_z_level()`
  (no in-game effect; no two icon draws overlap in any screen); F-02 — the compass and clock
  entries build without their first-person display poses (the pin's 208-vs-210 catch; vanilla
  carries `[0,−135,25]`/`[0,4,2]`/`1.7` behind `builtin/*` parents the port's pose source does
  not follow — a real, small pose gap with no live impact). Fix shapes in the final review's §6.
- **The window-click prediction is unwired for modes 1, 2, 3 and 6** (shift transfer,
  number-key swap, creative pick, double-click gather) — the legacy snapshot echo is kept — and
  creative whole-stack (limit-2) drags don't arm the predictor; those modes render post-echo
  (one tick late). Trigger: the next window-click/prediction round.
- **The water-fog colour reads the view block's own light** where the reference's term takes the
  feet-neighbour maximum (~2 B-units per light step; in band at both live re-measures). Trigger:
  the next water-fog / light touch (M6's sky work).
- **The rows-440+ R/G-only lift (+4..+5, B ≈ 0)** on the underwater pair — fragment-side, not
  fog-path; a watch item that mints its own round if it survives M6's weather/light work.
- **The dark-surface cells of the S2 pair are recorded by their re-based coordinates** (the
  original `dw1`/wall-L names were never coordinate-pinned; ours B `−3.6` fog-direction, vanilla
  bit-identical).
- **The R3 residuals stand recorded**: brewing (3/3 good-aim opens fail) and the anvil
  shelf-pocket pose (3 bad poses); the sign editor's never-open is a server-side condition (no
  port verdict); the acceptance residue (R10b's grey-pattern counts partial; G3 closed, G5
  partial, G10-row57 sighted).
- **The comment/citation sweep is landed (T25 `520bed4`) with the extractor's grammar delivered**;
  the record-only review nits (reviewer-7's line pins, reviewer-11's denominator note,
  reviewer-9's `screens/container.rs` shorthand) are recorded; the fix8 f13-exclusion note is
  struck (fix10 proved the term matters at the S2 pose).
- **The operator's by-eye list** (cannot be closed agent-side): the M5 acceptance frames per the
  reports' lists, plus M3's and M4's carried operator items.
- **The M4 caveats this milestone closed**: the comment/citation sweep set and the sweep-tooling
  round (T25); the PNG-helper consolidation and the 16-bit/tRNS strip fixture (T26); the M4
  known-limits 1 (held items and armour), 2 (the non-block item table) and 3 (the potion tint).

## Decisions locked

See spec section 4 for the full table. The short version: multiplayer-first v1; Microsoft
auth at M7; GPL-3.0; RustCraft is a read-only reference with zero code copied; assets come
from piston-meta into our own verified store with vanilla-install reuse; performance targets
are comparative (1.5x FPS, under 50% memory, under 1 second cold start).

## Next actions

Milestone M5 is complete and tagged; milestone M6 (spec section 13: sound, particles, weather,
screens) is next. Start it the same way M5 started: write the M6 plan into
`docs/plans/<date>-m6-<name>.md` from the spec's M6 row plus the ordered backlog below, have it
reviewed and approved by the owner, then execute it task by task in the established loop. The M5
close handoff is `docs/handoff/2026-10-11-m5-close.md`; the M5 final review's cross-cutting
section (its §2.4) names what M6's work will trip on: the settings store is the critical path
(the GUI-scale setting and the four-scale table, `ALL_PARTS`, pointer scaling, the chat mirror
halves, the anvil-name/book field engines, the render-distance range), the sky/weather carries
(the horizon, the celestial closing term, the fog colour's day-night term, the eye-light read,
the rows-440+ lift), the camera overrides, the language loader, the prediction splits, and the
riding HUD + own-player effect limits. The Azure application id and the Minecraft API permission
(M7's external dependency) remain a tracked owner action to start before M6.

The ordered M6 backlog (from the M5 final review's triage and the deferred sweep —
`refs/m5-task-28/deferred-list.md`, each row re-derived at `8b49246` in
`refs/m5-task-28/final-review.md` §1):

1. The final review's two carried minors: F-01 the stale HUD/creative GPU pins (update to the
   constant-50 rule and keep the tie-break assert) and F-02 the compass/clock first-person
   display poses (follow the `builtin/*` parent chain or pin the two entries) — the next
   window/GUI round's first items.
2. M6's own scope: the sound engine and the music ticker, particles, weather, the sky and stars
   with the celestial term, the overlays, the full screen list from section 11.2, the keybinds,
   F3 with sub-modes, F2 screenshots, the 1.8 render-distance range to 16 chunks, window resize
   and fullscreen — the milestone's plan.
3. The weather/light-touch carries: the fog colour's time term (Backlog 6), the water-fog
   colour's light read, the rows-440+ lift, the neighbour-brightness light term (with the
   `light.rs`:123 drift), the sky horizon (row 24) and the celestial closing term (row 25).
4. The screen/setting rows the M6 screens absorb: the chat tab completion and mirror, the
   pointer scaling, the part toggles, the camera-mode overrides, the render-distance option, the
   GUI scale's four-scale table (rec 42), `set_gui_scale`'s caller and the lang loader (row 55).
5. The still-open comment/citation rows (D3, the D5 remainder, D12, D14, D15, D16, D19, D26,
   D27, D30, D31, the Backlog 7 row, T8-F7, 73a, 73c, 78b, 38, T18-N1's weaker guard) at each
   file's next touch; the `slab_half` fixture when a protocol surface needs it.
6. The operator's list: M3's three audit carries plus M4's and M5's by-eye lists (the acceptance
   reports).
7. Later homes as recorded: M7 (the sounds' remaining rows, the online-mode fetch), M9 (the full
   animation sweep, the F3 FPS-readout gap, interest management).

The close recommendations (recorded 2026-10-11, for the owner's review): the deferred triage
stands (0 material; 22 landed; 6 closed as records; 38 open with triggers; 4 splits); the two
minors (F-01/F-02) are carried per above; the operator's by-eye list stands as listed.

The milestone's acceptance evidence lives under `refs/m5-task-27/` (the live pass, the leg batch
and the close-smokes) and `refs/m5-task-28/` (the frozen archive and the final review), with the
earlier milestones' under `refs/m4-acceptance/`, `refs/m4-task-22/`, `refs/rig/evidence/m2/` and
`refs/m3-acceptance/`; the rig runs against the store at `~/.local/share/oxidecraft`
(`OXIDECRAFT_STORE`).

Then, as always: keep `docs/STATE.md` and `CHANGELOG.md` current and tag each milestone when it
closes; run the local gate before every push (`cargo test --workspace`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`,
`bash scripts/check-assets.sh` (+ `--self-test`), `bash scripts/check-graph.sh` (+ `--self-test`),
`python3 scripts/parity-diff.py --self-test`, `cargo +1.85.0 check --workspace --all-targets`).
The post-v1 programme (owner-directed 2026-09-23, spec section 17: singleplayer worlds and Java
mod compatibility) starts only after v1 completes and does not affect the M2–M9 sequence.

## Resolved dependency versions

Pinned in `Cargo.lock` (committed). Direct dependencies as resolved at M5 (the M0 set, plus the
M1–M4 additions; M5 added no new crates — `oxide-client` gained edges to the existing
`oxide-proto` and `serde_json` workspace dependencies):

| Crate | Version | Notes |
| --- | --- | --- |
| `wgpu` | 26.0.1 | MSRV pin: the newest release whose declared rust-version (1.84) fits the workspace's 1.85 floor (`wgpu-hal` 26.0.6, `naga` 26.0.0) |
| `winit` | 0.30.13 | declares rust-version 1.70 |
| `ureq` | 3.4.2 | rustls, no default features (`rustls` 0.23.45) |
| `zip` | 7.2.0 | deflate only |
| `flate2` | 1.1.10 | `zlib-ng-compat` backend |
| `serde` / `serde_json` | 1.0.229 / 1.0.151 | derive |
| `clap` | 4.6.7 | derive |
| `thiserror` / `anyhow` | 2.0.20 / 1.0.104 | `thiserror` 1.0.69 is also present transitively |
| `tracing` / `tracing-subscriber` | 0.1.44 / 0.3.23 | env-filter |
| `sha1` / `hex` | 0.10.7 / 0.4.3 | |
| `tempfile` / `fs4` / `dirs` | 3.27.0 / 1.1.0 / 7.0.0 | |
| `glam` | 0.32.1 | `oxide-render` (terrain vertices, the camera). Do not bump past 0.32 under `-D warnings`: `Mat4::perspective_rh`/`look_to_rh` are deprecated from 0.33.1 (migrating to `glam::camera` is mechanical but a deliberate change) |
| `crossbeam-channel` | 0.5.17 | `oxide-game` and `oxide-client`, the session-to-window event channel |
| `png` | 0.18.1 | `oxide-assets`, the PNG texture loader (M2 Task 2) |
| `rayon` | 1.12.0 | `oxide-game`, the per-session mesh pool (M2 Task 13) |
| `base64` | 0.23.1 | `oxide-assets`, the skin and cape property decode (M4 Task 7) |

## Environment facts (development machine)

| Fact | Value |
| --- | --- |
| OS / desktop | CachyOS, kernel 7.2.8, Wayland; **KWin (KDE Plasma)** — current at the M5 close (re-checked 2026-10-11; the desktop has changed between niri and KWin across milestones — re-check before a rig run) on **two outputs**; `xwayland-satellite` on DISPLAY=:1, so the 1.8.9 rig client (LWJGL 2) runs without extra setup |
| Screenshots | On KWin (current): `spectacle -b -n -f -o <file>` full-screen, then crop to the window content rect (the M2 acceptance rect: window at (2000, 728, 1280×720); the overlay sits at screen (1720-1919, 1034-1079) — keep crops clear of it). **Calibrate the content scale before trusting a rect** (a past desktop rendered content at 1.2× in physical captures while the fit checks stayed green; the M5-era rect places the content at (640, 379) with a 1280×720 crop — the `PLACE_X=640`/`PLACE_Y=351` overrides — re-verify each session with a marker), keep the rect free of other windows (a `keepAbove` take is the recorded workaround), and treat a preflight screen-read failure as the check's own parse until proven otherwise. On niri (past): the portal fails (niri asserts a single output); the substitute was `niri msg action screenshot-screen --write-to-disk true --show-pointer false --path <file>`. HUD keys: `wtype` cannot reach KWin (no virtual-keyboard protocol); `xdotool` delivered F1 but not F3 — the fps-readout gap is M9's. `computer_use` cannot enumerate windows unless `CUA_DRIVER_RS_ENABLE_WAYLAND=1` is set |
| Rust | rustc 1.99.0, cargo 1.99.0 (re-checked 2026-10-11, no change; aligned to the CI runner's stable on 2026-10-02; `rust-toolchain.toml` says `channel = "stable"` — keep it current) |
| Dependency pin | `wgpu 26.0.1` is the newest release whose declared rust-version (1.84) fits the workspace's 1.85 floor (wgpu 27.0.0 declares 1.88, 28.0.0 declares 1.92, 29 and 30 declare 1.87), so cargo's resolver picked it; the pin is not a hand-downgrade. `winit 0.30.13` declares 1.70 (crates.io index metadata, 2026-09-23) |
| Vulkan | instance 1.4.357; Intel Iris Xe (card2) and NVIDIA T500 (card1) |
| Java | Java 26 system-wide; the rig uses a standalone JRE 8 tarball |
| gh CLI | 2.101.0, authorized as loofyser, scopes repo, read:org, gist |
| git push | uses gh as the credential helper for github.com (`gh auth setup-git` has been run; plain `git push` works now) |
| Minecraft installs | none on this machine; assets are downloaded into the Oxidecraft store |
| sudo | password required, and only usable as the leading command of a foreground call |
| Verification rig | `refs/rig/` — Temurin JRE 8, vanilla 1.8.9 server listening on 25565 (offline mode, seed `oxidecraft`, view-distance 10, compression threshold 256; capture runs move it to 25566 behind the recorder proxy on 25565), Prism instance `OxideRef-1.8.9` with an offline account. The proxy's upstream uses the machine's LAN address (10.0.0.84 on wlan0 at the acceptance run — DHCP, re-check before use). `refs/rig/README.md` documents start/stop; its screenshots section still describes the portal route — use the Screenshots row above instead. M2 added: run-stamped consoles (`logs/console-<UTC>.log`), the sample wall (`refs/rig/tools/sample_wall_cells.txt`), and the metric (`scripts/parity-diff.py`). M3 added the acceptance machinery (`refs/m3-acceptance/`; `shot.sh` is now executable — a 644 mode silently broke a burst run), the completion machinery (`refs/m3-task-13-completion/`), and two rig rules: give items AFTER any respawn (a give to a dead player is discarded by the respawn), and a window activation kicks the vanilla camera (~+18°/−4.4° per activation — re-issue the pose teleport and take captures window-free). M4 added the acceptance machinery (`refs/m4-acceptance/`) and the fix chain's round machinery (`refs/m4-task-22/`), plus three rig rules: guard every teleport leg against the free fall (restore the player's playerdata before the run and grant Resistance V); a stalled chunk shift poisons its session (re-take on a fresh session, and validate a post-teleport take by the wire — load columns > 0 — before trusting its frames); the mesh pump is idle-gated (≈1.2 columns/s under the server's entity flood — soak past the drain before capturing). M5 added: the recorder proxy on `:25566` for wire legs; window-targeted button delivery (`xdotool click|mousedown|mouseup --window <win>` — pointer-routed buttons are state-dependently swallowed); the pinned-client practice (build, copy, record md5); re-sending the scene setup + `/time set 6000` + `/weather clear` before every capture |

## Local-only directories

`refs/` (upstream clones, rig) and `vanilla/` (Mojang client jar and asset index) are
gitignored and never pushed. Neither is ever redistributed.

## Handoff protocol

1. Update this file: stage, what changed, next actions, any new environment facts.
2. Commit and push everything, so the next person can pull it.
3. Write `docs/handoff/YYYY-MM-DD-<topic>.md` from `docs/handoff/TEMPLATE.md`, fully
   self-contained: a reader should need nothing beyond the repository.
4. State plainly in the handoff what is verified and what is only planned.
