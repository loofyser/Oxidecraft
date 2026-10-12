# Handoff — M6 (sound, particles, weather, screens); the Tasks 5–7 wave closed

Written 2026-10-12, at the owner's request to record a clean stop with the milestone's parallel
wave closed and pushed. `main` carries Tasks 1–7 of M6's thirty-one closed, reviewed and pushed;
Task 8 (the audio engine) is the resume point.

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client
(GPL-3.0, multiplayer-first v1). The current milestone is **M6, "sound, particles, weather,
screens"** (spec section 13 of `docs/specs/oxidecraft-v1-design.md`): the sound engine and music
ticker, particles, weather, sky and stars, overlays, the full screen list, keybinds, F3 with its
sub-modes, F2 screenshots, the 1.8 render-distance range, window resize and fullscreen.

## Context you need

- Repo: https://github.com/loofyser/Oxidecraft, working copy at
  `/home/lucy/Desktop/Software/Projects/Oxidecraft`, branch `main`, HEAD `5b01136` plus this
  handoff's `docs:` commit.
- Read first, in this order: `docs/STATE.md`; the plan
  `docs/plans/2026-10-11-m6-sound-particles-weather-screens.md` (Task 8's section at line 287, then
  Tasks 9 onward as reached, and the Global Constraints); the ledger (local scratch space,
  git-ignored; the recovery map — trust it and `git log` over recollection).
- How the work runs: one task at a time; per task a brief (`task-N-brief.md`), the work, a report,
  a frozen diff (`review-BASE..HEAD.diff`), an independent review, scoped fix rounds; every ruling
  as a line in the ledger; checkpoint pushes at task closes. Read-only lanes and rig runs may
  proceed in parallel with ONE writer; file-disjoint writers may run in isolated worktrees, land
  serialised, and take one merged re-gate — the pattern this wave used.
- Rules that must not be broken: GPL-3.0; zero code copied from RustCraft; no Mojang asset, jar,
  `.class`, `.ogg` or `.png` committed and no `.class` read (the decompiled source under
  `refs/_src/MCP-919` is read for values and names only); `refs/` and `vanilla/` git-ignored and
  never committed; `git add` with explicit paths only; no push inside a task's work or its review;
  the full gate green immediately before every commit; normal English in code, comments, commits
  and documents.

## Current stage

- **Milestone M6 — in progress.** Tasks 1–7 of thirty-one closed and pushed; every pushed head's
  CI is green through `5b01136` (run `38209472618`, all seven jobs). Workspace counts at HEAD:
  2086 tests, 126 ignored. `origin/main` = `5b01136`.
- The landed chain: T1 the carried GPU pins (`d03596a`); T2 the effects codecs (`3158df6`); T3 the
  settings store (`2ef4d6b`); T4 the render settings surfaces (`d9d6092`); T5 the session's
  weather, effect events and view distance (`2eee366`, fix `46aff1a`); T6 the sounds index
  (`d0f74ab`); T7 the particle engine (`5b01136`). Two plan-text corrections landed (`6f9f958`
  the Task 7 fixtures, `f0760dd` the weather reason mapping). Reviews: T5 FIX-FIRST (the 0x2B
  reason 1/2 mapping inverted; fixed and re-reviewed PASS), T6 and T7 PASS-WITH-FINDINGS.
- **Carries bound for Task 8** (in the ledger): the Task 6 review's I-1 — the real `sounds.json`
  carries one `{"type": "event"}` redirect (`music.game.creative` → `music.game`) that the sound
  index keeps as a weight-1 file; the source delegates and lends the target's total weight (12).
  Task 8 must either add a redirect marker with pick-time delegation or record the divergence.
  And M-1 — a missing `category` refuses the document where vanilla defaults to `MASTER`.
- **Carries for the particle-render task** (not Task 8): the Task 7 review's four minors (row 0's
  dropped `+2` lifetime; the tick fan-out's per-tick vs per-spawn degrade; the lava-smoke
  pre-increment age; the residual-note wording) — fold into the next task touching
  `crates/oxide-game/src/particles.rs`.
- Nothing else changed: no ports held, no processes left, the wave's worktrees removed (branches
  `m6-task-6` @ `eb7c2b4` and `m6-task-7` @ `c491a73` remain as provenance).

## Exact next step

Run the standard pre-flight, then start Task 8:

1. `rustup update stable`; stage `refs/m6-task-8/gate.sh` (copy the previous round's script — it
   must keep the `msrv 1.85.0 check` step); run the base gate at the handoff head standalone
   (expect `GATE OK`; log to `refs/m6-task-8/gate-base.log`); check the pushed head's CI
   (`gh run list --limit 1`), the `m5` tag, and that ports 25565/25566 are free.
2. Generate `task-8-brief.md` for Task 8, "the audio engine, the mixer and the sound registry"
   (plan line 287), and BIND the two carries above into its contract.
3. Dispatch Task 8 in the established loop (brief → implement → report → frozen diff → independent
   review → fold → push), then Task 9 onward in plan order.

## Open questions for the project owner

None outstanding. (The M6 plan and its six decisions were approved 2026-10-11 — "Approve all six
as recommended — go"; the audio stack is `cpal` + `lewton` + `rand` with the silent-device
fallback.)

## Environment notes

- CachyOS; KDE Plasma on Wayland, `xwayland-satellite` on DISPLAY=:1 for the rig client; Rust
  stable current (`rustup update stable` before the pre-flight); the CI runner's stable can be
  ahead — check the runner's clippy version before diagnosing a CI-only failure.
- The gate script is `refs/m6-task-8/gate.sh` (staged from the previous round's; it carries the
  MSRV check); run it standalone with `CARGO_TARGET_DIR` unset. The worktree-capable variant reads
  `OXIDECRAFT_GATE_ROOT` for parallel lanes.
- The store: `~/.local/share/oxidecraft` (`OXIDECRAFT_STORE`); the decompiled source:
  `refs/_src/MCP-919` (values and names only); the rig: `refs/rig/` (`README.md`); screenshots on
  KWin: `spectacle -b -n -f -o <file>` with the calibrated crop (STATE.md's environment table).
- The GPU suites run one at a time (T500 via Vulkan); ignored, store-gated runs serial likewise.
- CI re-runs are serial (the workflow cancels an in-flight run when a newer one is requested).

## Verification to run before declaring done

- The full gate (`bash refs/m6-task-8/gate.sh`, ending `GATE OK`): workspace tests, fmt, clippy
  `-D warnings`, cargo-deny, the asset guard and crate-graph checks with their self-tests, the
  parity self-test, and the `msrv 1.85.0` check.
- CI green on `main` after every checkpoint push (`gh run list --limit 1`).
- For M6 overall: the plan's thirty-one tasks closed, the named screen list's item-by-item parity
  checks, and the milestone close per spec section 13's exit criteria.
