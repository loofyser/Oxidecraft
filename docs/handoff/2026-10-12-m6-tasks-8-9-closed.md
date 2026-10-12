# Handoff — M6 Tasks 8 and 9 closed, Task 10 next

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client:
its own framing, packet codecs, world model, renderer and UI, driven by the same wire protocol the
real client speaks, with no code from RustCraft and no Mojang binaries in the repository. Version 1
is the multiplayer-first client described in `docs/specs/oxidecraft-v1-design.md`; its section 13
milestones run M0 to M9, and the current milestone is **M6 — sound, particles, weather, screens**.

## Context you need

- Repo: https://github.com/loofyser/Oxidecraft, working copy at
  `/home/lucy/Desktop/Software/Projects/Oxidecraft`, branch `main`, GPL-3.0. HEAD `c36b8b9` plus
  this handoff's `docs:` commit.
- Read first, in this order: `docs/STATE.md` (live state — the M6 update bullets, the caveats,
  the environment facts), the M6 plan `docs/plans/2026-10-11-m6-sound-particles-weather-screens.md`
  (Global Constraints first, then Task 10 at line 354, then the tasks as reached), then
  `docs/specs/oxidecraft-v1-design.md` (v6 — section 13's M6 row; section 5.1's crate graph) and
  the matching research under `docs/research/`. The M6 ledger is kept in the local scratch space
  (git-ignored; the recovery map — per-task history, deferred items and every ruling taken so far).
  Task evidence: `refs/m6-task-8/` (report, review, gate and store-leg records) and
  `refs/m6-task-9/` (report with its fix-round section, both reviews, the back-out logs, the rig
  dump and the client log).
- Rules that must not be broken: GPL-3.0; no code copied from RustCraft; no Mojang asset, jar,
  `.class`, `.ogg` or `.png` committed and no `.class` read at runtime; `refs/` and any vanilla
  client data stay local and uncommitted; `git add` always with explicit paths; no `sudo` except
  as the leading command of a foreground shell command; workspace lints `missing_docs` and
  `unsafe_code = "forbid"`, MSRV 1.85 (the no-let-chains trap), edition 2024, `Cargo.lock`
  committed.

## Current stage

- Milestone: **M6 in progress** — Tasks 1–9 of the plan's thirty-one closed, reviewed and pushed.
  The T1–T7 chain is in `docs/STATE.md` and the prior handoff
  `docs/handoff/2026-10-12-m6-tasks-5-7-closed.md`.
- T8 (the audio engine, mixer and registry) — closed; commit `6354f51`; reviewed **approved**
  (0 critical / 0 important; six minors deferred, in `refs/m6-task-8/review.md`). Ships: the mix
  law and the decoder, the registry and index lookup, the category and master volumes, the
  silent-device fallback, and the stereo WAV debug dump; the store-backed ignored leg passes.
- T9 (the music ticker and the local sound sites) — closed; commits `b9ddbd6` + fix `d0f9db5`.
  The first review returned issues-found (one critical: the block sound groups mis-sounded 49
  ids; the step/swim/splash emission and the pump's wire conversions unfalsified); the fix round
  re-derived the whole map id-by-id to zero mismatches and pinned both legs with their own
  falsifiability back-outs; the fix re-review returned **approved** (0/0/0). Ships: the music
  ticker (menu, game, creative, end schedules), the block-step and hurt/death/pickup/level-up/
  splash/swim emission sites, the client's per-tick pump, and the `--audio-dump` client flag.
  Live: `music.game` started ≈12 s after joining the rig; the dump's peak −18.4 dB (non-silent);
  the rig server stopped cleanly and the ports ended free in both rig runs.
- CI: the T8 push exposed that the building jobs lacked the ALSA headers the audio stack's Linux
  backend needs — three jobs red (`alsa-sys`), repair `c36b8b9` (the building jobs now install
  `libasound2-dev`). The two red runs stay on the record (`38214701959`, `38221087041`); the
  close head's run is `38221391320` — six of seven jobs green, the portability cross-check still
  running at write time; re-check with `gh run list --limit 1`.
- Known unfinished, carried (each in the ledger with its trigger):
  - T8's six review minors (the dump-format wording debt, the unbounded per-tick decode drain,
    the stream-fallback nuance, the unreachable redirect branch, no incremental ring decode, no
    per-event engine log) — trigger: the next touch of the audio module.
  - T9's evidence note: the rig WAV's header reports `nframes=0` because the run's kill bypassed
    finalisation — exit the client cleanly on the next rig run.
  - The store (`~/.local/share/oxidecraft`) lacks
    `extracted/1.8.9/assets/minecraft/sounds.json`, so live runs exercise the index-object
    fallback only; the extracted-tree branch is unit-pinned and wants one run on a store that
    has the tree.
  - The particle engine's four minors and the other pre-existing carries stand unchanged from the
    prior handoff.

## Exact next step

Task 10 — **the sky, weather and fog carries** (plan line 354). Pre-flight the same way every task
has started: `rustup update stable`; stage `refs/m6-task-10/gate.sh` (already staged from T9's
copy); run the base gate standalone (`bash refs/m6-task-10/gate.sh`, expect `GATE OK`, tee it);
check the pushed CI (`gh run list --limit 1`); confirm ports 25565/25566 free. Generate
`task-10-brief.md`, then execute the loop (brief → implement → report → frozen diff → independent
review → fold → push). Step 1 of the task is its three carry re-derivations — the fog colour's
time term, the horizon disambiguation, the water-fog light read — written into the evidence dir
before any code.

## Open questions for the project owner

None blocking.

## Environment notes

- Rust: stable 1.99.0 aligned with the CI runner; the declared minimum is 1.85.0 and the gate
  carries the `cargo +1.85.0 check --workspace --all-targets` step — let-chains compile on stable
  and fail the minimum, so keep both the step and the discipline.
- The CI's audio dependency: the building jobs install `libasound2-dev` (commit `c36b8b9`); if an
  `alsa-sys` build error ever resurfaces anywhere, that repair is the precedent.
- Store and rig: `OXIDECRAFT_STORE=~/.local/share/oxidecraft`; the rig server starts with
  `refs/rig/server/start.sh` and stops cleanly with `refs/rig/server/stop.sh` (ports 25565 and
  25566). For audio captures the client takes `--audio-dump <file>` (a 16-bit stereo WAV of the
  mix); finalise it by exiting the client cleanly — a bare `pkill -f` once matched its own caller
  and, worse, skipped the header finalisation.
- GPUs: NVIDIA T500 (card1) and Intel Iris Xe (card2), Vulkan; the ignored GPU suites run on the
  T500; store-dependent ignored runs take the store env and run one at a time.

## Verification to run before declaring done

Run the full local gate and keep every check green:

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test
bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test
python3 scripts/parity-diff.py --self-test
cargo +1.85.0 check --workspace --all-targets
gh run list --limit 5
```
