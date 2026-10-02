# Handoff — M2 (textured terrain); Task 15 in progress, the fluids round stopped mid-run

Written 2026-10-01, at the owner's request to stop work mid-milestone. `main` carries fourteen of
M2's sixteen tasks closed; Task 15's acceptance work is in progress, and its seventh scoped fix
round (fluids) was stopped mid-run — that round is the resume point.

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client
(GPL-3.0, multiplayer-first v1). The current milestone is **M2, "Textured terrain"** (spec section 13
of `docs/specs/oxidecraft-v1-design.md` v5): the client renders the world with the real textures,
models, biome tints, lighting, fog, sky and clouds, proven by screenshot parity against a vanilla
1.8.9 client. Task 15 runs the acceptance: the three rig scenes (wall, mark, ground) captured from
both clients and compared under the parity metric, every remaining divergence fixed or recorded as a
sanctioned class. The post-v1 programme (owner-directed; spec section 17: singleplayer and Java mod
compatibility) starts only after v1.

## Context you need

- Repo: https://github.com/loofyser/Oxidecraft, working copy at
  `/home/lucy/Desktop/Software/Projects/Oxidecraft`, branch `main`, tags `m0`, `m1`; HEAD `e919711`
  plus this handoff's `docs:` commit.
- Read first, in this order: `docs/STATE.md`; the plan
  `docs/plans/2026-09-27-m2-textured-terrain.md`; the ledger
  (local scratch space, git-ignored; the recovery map — trust it and `git log` over recollection); `refs/m2-cloud-arm/report.md` and
  `refs/m2-cloud-arm/masks/mask-mark.txt` (the last round's evidence and corrected records).
- How the work runs: one task at a time; per task a brief (`task-N-brief.md`), the work, a report, a
  frozen diff (`review-BASE..HEAD.diff`), an independent review, scoped fix rounds; every ruling as
  a `Ruling:` line in the ledger; checkpoint pushes at chosen stops.
- Rules that must not be broken: GPL-3.0; zero code copied from RustCraft; no Mojang asset, jar,
  `.class`, `.ogg` or `.png` committed and no `.class` read (the decompiled source under
  `refs/_src/MCP-919` is read for values and names only); `refs/` and `vanilla/` git-ignored and
  never committed; `git add` with explicit paths only; no push inside a task's work or its review;
  the six-command gate green immediately before every commit; normal English in code, comments,
  commits and documents; Conventional Commits with an `(M2)` suffix.

## Current stage

- **Milestone M2 — in progress.** Tasks 1–14 closed and reviewed (the ledger holds each range and
  review). Task 15 (the parity metric and the acceptance baseline) is in progress.
- The acceptance exposed five rendering defect classes; six scoped fix rounds are landed and
  pushed: keepalive `6959280`; clock `14ca52f`; terrain origin `72526bf`; camera `bd83c0c`;
  atmosphere fog `a6760bd` (review: compliant/approved, 4 minors — all record-corrected); cloud
  at-or-above arm `e919711` (review: compliant/approved with findings — 3 Important/4 Minor, all
  record-corrected; the full-band mark mask is **withdrawn**; the honest conservative reading is
  0.0484/0.0175, a fail that is the round's recorded finding).
- **The seventh round (fluids) was stopped mid-run on 2026-10-01 at ~02:10 CDT**, ~45 minutes in —
  just after its mesher tests reached GREEN and its gate re-ran clean. At stop it had: found a candidate defect — the liquid and model quad UVs map into an
  animated sprite's whole strip instead of the current frame's rect (the mesher maps a v span of
  0.25 where the frame's rect is 0.125); captured RED evidence (`refs/m2-fluids/red.log`: two
  failing `mesher_liquids` tests plus one failing `mesher` test); written a WIP fix over six files
  (`crates/oxide-assets/src/atlas.rs` and its tests; `crates/oxide-game/src/mesher.rs` and
  `mesher/liquid.rs`; `mesher.rs`'s and `mesher_liquids.rs`'s tests; snapshot at
  `refs/m2-fluids/wip-stopped-round.diff`) that **converged to GREEN by the stop** (16/16 in
  `mesher_liquids`; `refs/m2-fluids/green.log`), with `cargo fmt` applied and the full gate re-run to
  `GATE OK` (`refs/m2-fluids/gate.log`); it was starting the ignored render tests when stopped. **No rig run happened; no commit was made; nothing was
  pushed.** Whether the candidate defect explains both acceptance symptoms (the glass-walled cells
  invisible; the ground water colour) was not yet established — the next round re-verifies the cause
  against the source before fixing.
- The WIP was snapshotted, then the six files were reset to HEAD: `refs/m2-fluids/wip-stopped.diff`
  (635 lines; the full tree) and `refs/m2-fluids/wip-stopped-round.diff` (531 lines; the six files
  only — `git apply` it to adopt the WIP). The stopped attempt's live log is kept outside the
  repository on this machine (path recorded in the ledger).
- Working tree: Task 15's trio only — `crates/oxide-world/tests/behaviour.rs` modified (the
  `EXTRA_CELLS` list, Task 15 Step 3's), `scripts/parity-diff.py` and `docs/perf.md` untracked.
  **Keep them; never commit or revert them separately.** The rig is stopped; ports 25565/25566
  free; no leftover processes.
- Tests / gate at this stop: the round's own gate run was `GATE OK` (`refs/m2-fluids/gate.log`),
  and the six-command gate was re-run on the reset tree before the stop's `docs:` commit (`GATE OK`;
  `refs/m2-fluids/gate-pause.log`). The last full numbers before the stop (the cloud-arm
  review's re-run): workspace 504 passing / 0 failed / 21 ignored; the GPU ignored suite 13/13
  across its two suites.
- Known broken or unfinished: the fluids round (above); continuation #3 (the full re-capture) not
  run; Task 15's close and Task 16 not done. Carried to continuation #3: the mark pair's remaining
  true classes (the near-terrain shading divergence, ~10,193 px; the terrain-visibility-through-fog
  class, ~3,065 px) and the vanilla reference frame's under-soak (73 s against the ~376 s mesh
  drain — re-soak).

## Exact next step

Resume **ROUND 3 (fluids)** with a fresh attempt, the round's brief updated by the stopped
attempt's findings:

1. Brief it with: the original scope (the wall's four glass-walled liquid cells — x=0,1 water /
   x=4,5 lava at y=62, currently not rendered; the ground water body's colour, ours (45,47,29) vs
   vanilla (57,69,126)) **plus** the stopped attempt's candidate cause, RED evidence and snapshots,
   and the instruction to re-verify the cause against `refs/_src/MCP-919`
   (`BlockFluidRenderer`/`BlockLiquid`/`Block.java:468-471`/`BlockBreakable.java:52`) before fixing.
   Decide adopt-vs-clean: `git apply refs/m2-fluids/wip-stopped-round.diff` to adopt the WIP (it is
   GREEN and gate-clean; if adopted, confirm its tests against the source, then finish the round:
   the ignored-suite run, the live captures + metric, the report, the commit), or start from HEAD.
2. Then: RED→GREEN at the mesher layer; live wall+ground captures (both clients; `/weather clear`
   immediately before every capture; our soak ≥ 8–10 min; the vanilla reference soaked past the
   ~376 s mesh drain; capture rect checked clear of the overlay window at screen (1720-1919,
   1034-1079)); the metric re-derived with the fluid cells unmasked; gate `refs/m2-fluids/gate.sh`
   green immediately before ONE `fix: … (M2)` commit; report `refs/m2-fluids/report.md`; no push.
3. Scoped review of the frozen diff (fresh reviewer); fix round for findings; push on clean.
4. **Continuation #3**: the full acceptance re-capture (all three pairs), carrying the cloud-arm
   review's items (re-derive the mark mask per the acceptance's own derivation; re-soak the vanilla;
   overlay-free rect; the shading and terrain-visibility classes).
5. Task 15 close (`test: add the parity metric tool and the M2 baseline (M2)`, including the
   retained trio and Task 15's review); then Task 16 (docs + milestone close), the whole-branch
   review, and tag `m2`.

## Open questions for the project owner

None outstanding. The mark-pair cloud-arm question was answered on 2026-10-01 (implement as a
scoped round; landed as `e919711`). The fluids round's adopt-vs-clean decision is made at resume (recommended default: re-verify the
cause first, then adopt the tests only if they pin source literals).

## Environment notes

- CachyOS; niri Wayland (desktop 4480×1440 as of 2026-10-01); the 1.8.9 rig client runs via
  `xwayland-satellite` (DISPLAY=:1). Screenshots: focus the window, then
  `niri msg action screenshot-screen --write-to-disk true --show-pointer false --path <file>` (the
  portal route fails here).
- Desktop size changed (4480×1440 now; the acceptance recorded 3640×1920): the rig's capture tool
  `refs/rig/tools/capture_window.sh` takes `PLACE_X`/`PLACE_Y` overrides. An overlay window sits at
  screen (1720-1919, 1034-1079) — keep the capture rect clear of it, or a screen-space field
  contaminates every frame and inflates censuses.
- The store: `~/.local/share/oxidecraft` (`OXIDECRAFT_STORE`); the real-tree tests are `#[ignore]`d
  and gate on it. The decompiled source: `refs/_src/MCP-919` (HEAD `1717f75`). The rig: `refs/rig/`
  (`README.md`; server via `start.sh`/`stop.sh` — stop saves the world). Server console commands go
  through the FIFO `refs/rig/server/server.stdin`.
- The machine's LAN address is DHCP (10.0.0.84 at the acceptance run — re-check before using the
  proxy).

## Verification to run before declaring done

- The six-command gate (`cargo test --workspace`; `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets -- -D warnings`; `cargo deny check`;
  `bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test`;
  `bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test`) plus the ignored GPU
  suite locally (`-- --ignored`, 13/13 on the T500).
- For ROUND 3 specifically: the RED→GREEN logs; the wall pair's four fluid cells present and
  unmasked; the ground water rect's numbers against the bar (masked over-8 ≤ 2 %, over-24 ≤ 1 %);
  the round's ONE commit with its cause-true message.
- CI green on `main` after each push (`gh run list --limit 5`).
