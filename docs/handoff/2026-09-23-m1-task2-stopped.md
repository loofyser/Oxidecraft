# Handoff — M1 in progress; Task 2 stopped mid-run

Written 2026-09-23, at the owner's request to stop work mid-milestone.

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client
(GPL-3.0, multiplayer-first v1). The current milestone is **M1, "Bytes to world"** (spec section 13):
framing, handshake, offline login, compression, keepalive, the five connection obligations, Join
Game, Client Settings, chunk parsing (`0x21` and `0x26`), the world store, and untextured terrain.
Its exit criterion: connect to the local rig server and see correct block-coloured geometry above and
below y=128 with an F3-style overlay. The owner has also added a **post-v1 programme**:
singleplayer worlds and Java mod compatibility (spec section 17, revision v4).

## Context you need

- Repo: https://github.com/loofyser/Oxidecraft, working copy at
  `/home/lucy/Desktop/Software/Projects/Oxidecraft`, branch `main`, pushed through `b950e30` plus
  this handoff.
- Read first, in this order: `docs/handoff/2026-09-23-m1-kickoff.md` (the M0 close-out and M1
  kickoff), `docs/plans/2026-09-23-m1-bytes-to-world.md` (the binding plan: 13 tasks, the global
  constraints, the decisions, and the owner's recorded answers), `docs/STATE.md`,
  `docs/research/protocol-47-reference.md` (byte-level truth for everything on the wire), and the
  progress ledger `.superpowers/sdd/2026-09-23-m1-bytes-to-world/progress.md` (the pre-flight scan
  table, the rulings, and the per-task history — it is the recovery map after any context loss).
- How the work is run: one task at a time, in plan order. Per task: extract the brief
  (`bash <superpowers>/skills/subagent-driven-development/scripts/task-brief docs/plans/2026-09-23-m1-bytes-to-world.md N`),
  dispatch a fresh implementer on it, then an independent reviewer over the diff package
  (`scripts/review-package PLAN_FILE BASE HEAD`); fix rounds close findings; every decision taken on
  the owner's behalf is a `Ruling:` line in the ledger and must be listed in the final report.
- Rules that must not be broken: GPL-3.0; zero code copied from RustCraft (read-only reference); no
  Mojang asset, jar, `.class`, `.ogg`, or `.png` committed and no `.class` ever read at runtime;
  rust-version 1.85, edition 2024, committed `Cargo.lock`; every public item documented and
  `unsafe_code` forbidden workspace-wide; no AI or tooling language in committed files or commits;
  `git add` always with explicit paths; evidence (captures, screenshots, logs) stays under the
  git-ignored `refs/`; run the gate before every push (`cargo test --workspace`,
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo deny check`, `bash scripts/check-assets.sh`, `bash scripts/check-graph.sh`).

## Current stage

- **Milestone M1 — in progress.** Twelve tasks remain. Task 1 is complete and pushed with a clean
  review; Task 2 was stopped mid-run at the owner's request and is the resume point.
- Verified working: the whole M0 base, plus Task 1's `oxide-proto::conn::Conn` (buffered framed
  connection) and `oxide-proto::codec` (primitives and strings). At `b950e30`: 136 tests pass
  workspace-wide, `fmt` and `clippy -D warnings` clean.
- Known unfinished: Task 2's capture. Run A's raw capture exists (unanalysed), run B never ran, no
  fixtures exist, and the findings document was never written. The two rig tools the stopped
  implementer wrote are unverified — and its own synthetic self-test of `analyse_capture.py` was
  **failing** when it was interrupted (`DESYNC: 0x26 frame 5: 256 bytes left after the last column`),
  which is the known `0x26` trap: the biome array is always present in bulk columns and must be
  consumed. Fix or verify that path before trusting any analysis.
- Nothing is half-applied in git: the working tree is clean, the rig is stopped, and
  `server.properties` is back to `server-port=25565`.

## Exact next step

Resume **Task 2** (the live capture) from its Step 1 — take or replace the run A capture:

1. Point the server behind the proxy: set `server-port=25566` in `refs/rig/server/server.properties`,
   start it with `refs/rig/server/start.sh`, and run the proxy on `25565`
   (`python3 refs/rig/tools/record_proxy.py --listen 127.0.0.1:25565 --upstream 127.0.0.1:25566 --out refs/rig/evidence/m1/capture-run-a`).
   Verify `refs/rig/tools/record_proxy.py` and `analyse_capture.py` first — both are unverified.
2. Launch the vanilla client with `refs/rig/client/launch-client.sh --join`; wait for
   `OxideRef joined the game` in `refs/rig/server/logs/latest.log`; let it reach the world, then stop
   the client (`flatpak kill org.prismlauncher.PrismLauncher`) and the proxy. That is run A
   (loopback upstream — expect **no** Set Compression).
3. Run B with `--upstream 10.1.34.142:25566 --out refs/rig/evidence/m1/capture-run-b` (the LAN
   address; the server then sees a non-loopback peer — expect Set Compression, threshold 256).
4. Analyse both runs, extract one or two columns plus `manifest.json` into
   `crates/oxide-proto-v47/tests/fixtures/m1-capture/`, write
   `docs/research/protocol-47-live-capture.md`, and commit both with explicit paths.
5. Restore `server-port=25565`, stop the rig, and record the task's review as usual.

Then continue with Task 3 (login-state packets) onward. Task 2's decisive question: **does the rig's
vanilla server ever emit `0x26`?** Answer it from the packet histogram, with a clear negative if that
is what the capture shows.

## Open questions for the project owner

None outstanding. The seven plan questions were answered on approval (recorded in the plan), and the
post-v1 programme is recorded in spec section 17. The one question the plan still tracks — `0x26`
emission — is answered by Task 2's capture, and a negative answer is an acceptable, recorded result.

## Environment notes

- CachyOS, GNOME on Wayland with XWayland supplied by mutter (the 1.8.9 client is X11-only and
  works). Rig at `refs/rig/`: Temurin JRE 8, offline-mode 1.8.9 server (seed `oxidecraft`, level
  `parity`), Prism instance `OxideRef-1.8.9`.
- The machine's LAN address is **10.1.34.142**; the other addresses are VM and container bridges
  (`10.0.2.2`, `192.168.122.1`, `172.17.0.1`, `172.18.0.1`, `10.2.0.2`). `server.properties` has
  `server-ip=` empty, so the server binds every interface.
- Server console commands are delivered through the FIFO `refs/rig/server/server.stdin`; stop the
  server cleanly with `refs/rig/server/stop.sh` (it saves the world).
- Two GPUs are present (Intel Iris Xe, NVIDIA T500); the renderer asks for Vulkan only and logs the
  adapter. Screenshots on this desktop go through the portal, not ImageMagick or `x11grab`.
- `pkill -f <pattern>` also kills the invoking shell when the pattern appears in its own command
  line — use a character class (`record_prox[y]`) or kill by pid.

## Verification to run before declaring done

- The gate: `cargo test --workspace`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`,
  `bash scripts/check-assets.sh`, `bash scripts/check-graph.sh`; the two ignored GPU tests locally
  with `-- --ignored`.
- Task 2 specifically: the findings document states each run's packet histogram (the `0x21`/`0x26`
  counts) and compression state, and every fixture column decodes with the size formula
  (`8192·N + 2048·N + 2048·N + 256` for Overworld ground-up columns).
- CI green on `main` after each push (`gh run list --limit 5`).
