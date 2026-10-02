# Handoff — starting M3 (after M2's close)

Self-contained: the work can resume without the previous conversation.

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client: its
own framing, packet codecs, world model, renderer and UI, driven by the same wire protocol the real
client speaks, with no code from RustCraft and no Mojang binaries in the repository. Version 1 is
the multiplayer-first client described in `docs/specs/oxidecraft-v1-design.md`; the milestones in
its section 13 run M0 to M9.

## Where things stand (2026-10-02)

- **Milestone M2 (textured terrain) is complete and tagged `m2`.** The annotated tag marks the
  close-out commit `e90a30d`; `14b182f` recorded its CI run, and the same day's tidy pass and close
  decisions sit above it on `main`. Worktree clean; CI green on every pushed head (the close-out's
  run: `36999837908`, all seven jobs; check the current head with `gh run list --limit 1`).
- The final whole-branch review over the milestone returned PASS: no Critical or Important
  findings, and all 37 deferred items triaged safe to carry. The acceptance residue stands as
  documented — all three scene pairs fail the masked bar as written, every class recorded in
  `refs/rig/evidence/m2/acceptance-notes.md`; the close proceeded with the classes documented per
  the owner's ruling.
- **Milestone M3 (spec section 13) is next. Its plan is not yet written.**

## Read first, in order

1. `docs/STATE.md` — the live state: the stage, the "M2 evidence" section, the caveats, the
   ordered M3 backlog ("Next actions"), and the environment facts.
2. `docs/handoff/2026-10-02-m2-close.md` — the milestone close note: mission, context, exact next
   step, open questions, environment notes, verification.
3. `docs/specs/oxidecraft-v1-design.md` (v6) — section 13 (the milestone table; the M3 row) and
   section 5.1 (the crate graph).
4. `refs/m2-final-review/review-report.md` — the final review, including the "what will fight M3"
   list in its section 2.
5. The local scratch space (git-ignored) holds the milestone ledger — the per-task history and
   every ruling taken while M2 ran. Trust the ledger and `git log` over recollection, and never
   redo a task it marks complete.

## Exact next step

Author the M3 plan (`docs/plans/<date>-m3-<name>.md`) from the spec's M3 row — player physics,
input, the camera's FOV formula, raycast, break/place, the two block-change packets, death and
respawn — plus the seven-item backlog in STATE. Have the plan reviewed and approved by the owner
before any code. Then execute it task by task in the established loop: carry out each task per the
plan's steps and its global constraints, have its frozen diff reviewed independently, fix and
re-review where findings require it, keep the ledger current, and keep the gate green before every
push and CI green after every checkpoint push.

## Rules that must not be broken

- GPL-3.0; no code copied from RustCraft; no Mojang asset, jar, `.class`, `.ogg` or `.png`
  committed, and no `.class` read; `refs/` and vanilla data stay local and uncommitted; `git add` with explicit
  paths; normal English.
- Keep the local Rust stable current (`rustup update stable`; `rust-toolchain.toml` says
  `channel = "stable"`). A six-release gap once hid a CI clippy failure for five days — check CI
  after every push.

## Environment notes

- Desktop: CachyOS, KWin (since 2026-09-29; was niri). Screenshots: `spectacle -b -n -f -o <file>`
  full-screen, cropped to the window content rect (the M2 rect: window at (2000, 728, 1280×720));
  keep crops clear of the overlay at screen (1720-1919, 1034-1079). `wtype` cannot reach KWin;
  `xdotool` delivered F1 but not F3 — the fps-readout gap is M9's.
- Rig (`refs/rig/`): Temurin JRE 8; vanilla 1.8.9 server on 25565 (25566 behind the recorder
  proxy for captures); start/stop via `refs/rig/server/start.sh` and `stop.sh`; console commands
  through `server.stdin`; run-stamped consoles under `logs/`.
- Acceptance runs (when needed): scene setup plus `/time set 6000` and `/weather clear` before
  every capture; both clients soaked past the mesh drain; masks derived per the recorded policy,
  never widened; metric `python3 scripts/parity-diff.py`.

## Verification before declaring anything done

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test
bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test
python3 scripts/parity-diff.py --self-test
cargo test --workspace -- --ignored
gh run list --limit 5
```

## Decisions taken (2026-10-02)

The owner accepted the close recommendations on 2026-10-02:

1. No acceptance-residue class becomes M3 scope — the classes stay documented and owned by the
   later milestones: the cloud band and the fluids phase ride M6's sky and weather work, the slope
   edge is re-measured when M6 touches the cloud layer, and the chest cell is the missing
   tile-entity path with no M3 interaction.
2. CI keeps tracking the runner's stable toolchain, and the local stable is kept current (a
   six-release gap once hid a clippy failure for five days).
3. The M3 plan folds backlog item 1 — the block-kind border read — into the milestone's light work;
   the other six items carry as task-scoped notes.

## Pitfalls

- Build with the repo-local target directory: an inherited `CARGO_TARGET_DIR` once produced a
  stale binary that faked "no movement" in a capture.
- Re-send `/weather clear` and the scene setup immediately before every capture; keep the capture
  rect clear of the overlay.
- Derive masks per the recorded policy, justify each rect, census-check; never widen.
- A local stable behind the runner's hides new lints: align it before pushing.
