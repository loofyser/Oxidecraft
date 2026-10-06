# Handoff — M4 (entities and chat); Task 17 in progress, the first pass interrupted

Written 2026-10-05, at the owner's request to stop work mid-milestone. `main` carries sixteen of
M4's twenty-three tasks closed; Task 17 (the scoreboard sidebar and the below-name scores) is in
progress, and its first pass was interrupted mid-run — that work's completion is the resume point.

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client
(GPL-3.0, multiplayer-first v1). The current milestone is **M4, "entities and chat"** (spec section
13 of `docs/specs/oxidecraft-v1-design.md`): two clients — one vanilla, one ours — see each other,
chat, and agree on entities.

## Context you need

- Repo: https://github.com/loofyser/Oxidecraft, working copy at
  `/home/lucy/Desktop/Software/Projects/Oxidecraft`, branch `main`, HEAD `00df68a` plus this
  handoff's `docs:` commit.
- Read first, in this order: `docs/STATE.md`; the plan
  `docs/plans/2026-10-04-m4-entities-and-chat.md` (Task 17's section; then Tasks 18–23); the ledger
  (local scratch space, git-ignored; the recovery map — trust it and `git log` over recollection);
  `refs/m4-task-17/round-a-brief.md` and `refs/m4-task-17/wip-stopped.diff` (the interrupted work's
  requirements and its snapshot).
- How the work runs: one task at a time; per task a brief (`task-N-brief.md`), the work, a report,
  a frozen diff (`review-BASE..HEAD.diff`), an independent review, scoped fix rounds; every ruling
  as a `Ruling:` line in the ledger; checkpoint pushes at chosen stops.
- Rules that must not be broken: GPL-3.0; zero code copied from RustCraft; no Mojang asset, jar,
  `.class`, `.ogg` or `.png` committed and no `.class` read (the decompiled source under
  `refs/_src/MCP-919` is read for values and names only); `refs/` and `vanilla/` git-ignored and
  never committed; `git add` with explicit paths only; no push inside a task's work or its review;
  the six-command gate green immediately before every commit; normal English in code, comments,
  commits and documents.

## Current stage

- **Milestone M4 — in progress.** Tasks 1–16 closed and pushed (the ledger holds each range and
  review); every pushed head's CI is green through `00df68a` (run `37408432321`, all jobs).
- **Task 17 is in progress.** Its recon is complete and its first pass was interrupted 2026-10-05
  mid self-check (≈23:04). That work is held **uncommitted** in the working tree — nine files:
  `crates/oxide-client/src/{main.rs,view.rs}`,
  `crates/oxide-game/src/{entity_view.rs,scoreboard.rs,session.rs}`,
  `crates/oxide-proto-v47/{src/ui.rs,tests/ui_codecs.rs}`,
  `crates/oxide-render/{src/entity_pass.rs,tests/pipeline_headless.rs}`.
- State of that work: workspace `#[test]` 1317 → 1330 (ignores 61 unchanged; client 143→154, game
  322→324); the four crate suites' last runs were green (151/3, 323/1, 294/48, 178); three
  self-check mutations were run RED and restored (the third's outcome was re-established after the
  interruption and its bytes restored to the recorded hashes); the six-command gate was **not yet
  run**; no commit; the pass's report was never written. Snapshot:
  `refs/m4-task-17/wip-stopped.diff` (a 1,174-line full-tree diff) and `wip-stopped.status`; the
  interrupted pass's live log is kept outside the repository (path recorded in the ledger).
- Nothing else changed: no ports held, no processes left, `origin/main` = `00df68a`.
- Research notes: Task 18's verification note landed (`refs/m4-homework/t18-bossbar-scout.md`, with
  three recorded disagreements against the earlier leads); the Tasks 19–20 research note was cut
  short and needs redoing.

## Exact next step

Complete the interrupted first pass of Task 17, then proceed to its second pass:

1. Verify the tree (the nine files; the recorded hashes; `git diff` against
   `refs/m4-task-17/wip-stopped.diff`), run the six-command gate standalone (expect `GATE OK`; log
   to `refs/m4-task-17/gate-a.log`), make ONE commit
   `feat: assemble the scoreboard sidebar and the below-name lines` (explicit paths: the nine
   files; no push), and write the pass's report to `refs/m4-task-17/report-a.md` with a provenance
   note and the three mutation records. If the gate is red, repair forward minimally on the same
   tree — never restart.
2. Freeze this pass's review package (`refs/m4-task-17/review-a-00df68a..<head>.diff`) and write the
   second pass (the entity pass draws the below-name; the sidebar wires into the frame's HUD; the
   GPU cases), per `refs/m4-task-17/round-a-brief.md` and the plan section.
3. Review the combined work, close Task 17 (recordings in the plan; a `docs:` close commit; push),
   then continue with Tasks 18–23 (the boss bar; the two carry tasks; the API/citation carries; the
   acceptance run; the milestone close).

## Open questions for the project owner

None outstanding.

## Environment notes

- CachyOS; niri Wayland; the 1.8.9 rig client runs via `xwayland-satellite` (DISPLAY=:1);
  screenshots via `niri msg action screenshot-screen` (the portal route fails here).
- The gate script is `refs/m4-task-17/gate.sh` (it carries the parity self-test step); run it with
  `CARGO_TARGET_DIR` unset, as its own call. Rust 1.99.0 local; the CI runner's stable can be
  ahead — check the runner's clippy version before diagnosing a CI-only failure.
- The store: `~/.local/share/oxidecraft` (`OXIDECRAFT_STORE`); texture keys are extensionless. The
  decompiled source: `refs/_src/MCP-919`. The rig: `refs/rig/` (`README.md`).
- CI re-runs are serial (the workflow cancels an in-flight run when a newer one is requested).

## Verification to run before declaring done

- The six-command gate (`cargo test --workspace`; `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets -- -D warnings`; `cargo deny check`;
  `bash scripts/check-assets.sh` + `--self-test`; `bash scripts/check-graph.sh` + `--self-test`)
  plus the parity self-test (`python3 scripts/parity-diff.py --self-test`), ending `GATE OK`.
- The ignored GPU suite locally (`cargo test -p oxide-render -- --ignored`) and the client's store
  test (`cargo test -p oxide-client -- --ignored`).
- For M4 overall: the Task 22 acceptance — two clients (one vanilla, one ours) see each other,
  chat, and agree on entities — with every M4 checklist row evidenced under `refs/m4-acceptance/`.
- CI green on `main` after each push (`gh run list --limit 5`).
