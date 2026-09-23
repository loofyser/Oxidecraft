# Oxidecraft — project state

Updated: 2026-09-23

This file is the live state of the project. Keep it current before every handoff, long pause,
and milestone boundary. Anyone picking the work up should be able to read this file plus the
specification and continue without asking questions that are already answered here.

## Where we are

- Stage: milestone M0 in progress. Spec v2 is approved, and M0 runs from
  `docs/plans/2026-09-22-m0-foundations.md`, one task at a time. The working ledger (gitignored)
  is `.superpowers/sdd/2026-09-22-m0-foundations/progress.md`.
- M0 tasks complete: T1 workspace and VarInt codec; T2 framing with the 1.8 compression rules
  (fix round 1 applied); T3 handshake, status ping and the launcher CLI (`4c0db19`, re-reviewed);
  T4 the hash-verified atomic store (`ad563f5`, fix round 1); T5 piston-meta parsing (`0fc73cb`);
  T6 `fetch --verify` with fix round 1 applied and re-reviewed (`b3fcc51`); T7 jar extraction with
  the manifest, review clean (`8d4dac9`). T8 the wgpu window with the FPS counter is implemented on
  `main` with fix round 1 applied, acceptance run recorded below, awaiting its re-review.
- Tests: 112 passing workspace-wide, 3 ignored (two require the live endpoints, one requires a GPU
  adapter), zero failures (`cargo test --workspace`).
- Live evidence: our own `cargo run -p oxide-launcher -- ping 127.0.0.1:25565` answers
  `1.8.9 — protocol 47 — 0/20 players`; the vanilla client title screen is captured at
  `refs/rig/evidence/minecraft-1.8.9-title-screen.png`; the wgpu window is captured at
  `refs/rig/evidence/m0-window.png`; the full fetch run is recorded under "M0 evidence" below.
- Remaining M0 tasks: T10 hygiene and the `m0` tag. T9 is in review with fix round 1 applied:
  `deny.toml` and the two guard scripts are on `main` (`9ef812d`), and `.github/workflows/ci.yml` is
  pushed (`7b94c7f`), with CI run `35870226435` on that head green across all six jobs.
- Review: `docs/reviews/2026-09-22-spec-review.md`, with a disposition record for every finding.
- Research: five evidence-backed reports in `docs/research/`, indexed in Appendix B of the spec.
- Parity checklist classification: `docs/parity/checklist.md`.
- Verification rig: under `refs/rig/` (offline-mode 1.8.9 server plus a vanilla client, see
  `refs/rig/README.md`).
- Repo: https://github.com/loofyser/Oxidecraft — `main` pushed; the last code commit carries jar
  extraction, and documentation commits follow it. Confirm HEAD with `git log --oneline -3`.

## M0 evidence

Task 6 acceptance run, real endpoints, default store at `<data dir>/oxidecraft`, debug build
(`cargo run -p oxide-launcher -- fetch --version 1.8.9 --verify`), 2026-09-23:

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
- Fix round 1 (2026-09-23): the store lock is now an operating-system lock over
  `<store root>/lock`, held for the run and released when the process dies, so a killed run cannot
  lock out the next one; the file itself stays behind with the last run's process id in it.

Task 7 acceptance run, the real client jar already in the store, default store at
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

Task 8 acceptance run, a real window on this machine's GNOME/Wayland desktop, debug build
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
  `WAYLAND_DISPLAY` unset) reached the same title and a steady 60 fps, so both session paths work.
- Escape and window-close exits were not exercised on this session: the desktop-control tooling
  cannot enumerate windows here and synthetic keys are not delivered to the XWayland client. The
  run above exits through the frame limit, which reaches the same `event_loop.exit()`; the escape
  rule itself is unit-tested.

- Fix round 1 (2026-09-23): a stale surface (`Outdated`, `Lost`) is now reconfigured and the frame
  retried once instead of stopping the client, a `Timeout` frame is dropped, the error logs carry
  the full cause chain, and the clear colour's sRGB difference is recorded in `docs/DIVERGENCES.md`
  (entry 4).

## Decisions locked

See spec section 4 for the full table. The short version: multiplayer-first v1; Microsoft
auth at M7; GPL-3.0; RustCraft is a read-only reference with zero code copied; assets come
from piston-meta into our own verified store with vanilla-install reuse; performance targets
are comparative (1.5x FPS, under 50% memory, under 1 second cold start).

## Next actions

1. Review the Task 7 extraction work on `main`; mark T7 complete in the ledger when every finding is
   addressed.
2. Continue the M0 plan from Task 8 (wgpu window) in order, one task per dispatch, with the task
   review and fix loop after each.
3. Every dispatch carries the standing rules: no AI or tooling language in committed files, commit
   messages or code comments; explicit `git add <paths>`; never touch `.superpowers/`.

## Environment facts (development machine)

| Fact | Value |
| --- | --- |
| OS / session | CachyOS, kernel 7.2.2, Wayland with GNOME (mutter). XWayland is provided by mutter's `Xwayland :1`, so the 1.8.9 rig client (LWJGL 2) runs without extra setup. Earlier notes say niri; the desktop was switched to GNOME on 2026-09-22 |
| Rust | rustc 1.95.0, cargo 1.95.0 |
| Dependency pin | `wgpu 26.0.1` is the newest release whose declared rust-version (1.84) fits the workspace's 1.85 floor (wgpu 27.0.0 declares 1.88, 28.0.0 declares 1.92, 29 and 30 declare 1.87), so cargo's resolver picked it; the pin is not a hand-downgrade. `winit 0.30.13` declares 1.70 (crates.io index metadata, 2026-09-23) |
| Vulkan | instance 1.4.357; Intel Iris Xe (card2) and NVIDIA T500 (card1) |
| Java | Java 26 system-wide; the rig uses a standalone JRE 8 tarball |
| gh CLI | 2.101.0, authorized as loofyser, scopes repo, read:org, gist |
| git push | uses gh as the credential helper for github.com (`gh auth setup-git` has been run; plain `git push` works now) |
| Minecraft installs | none on this machine; assets are downloaded into the Oxidecraft store |
| sudo | password required, and only usable as the leading command of a foreground call |
| Verification rig | `refs/rig/` — Temurin JRE 8, vanilla 1.8.9 server listening on 25565 (offline mode, seed `oxidecraft`), Prism instance `OxideRef-1.8.9` with an offline account; `refs/rig/README.md` documents start/stop and screenshots |

## Local-only directories

`refs/` (upstream clones, rig) and `vanilla/` (Mojang client jar and asset index) are
gitignored and never pushed. Neither is ever redistributed.

## Handoff protocol

1. Update this file: stage, what changed, next actions, any new environment facts.
2. Commit and push everything, so the next person can pull it.
3. Write `docs/handoff/YYYY-MM-DD-<topic>.md` from `docs/handoff/TEMPLATE.md`, fully
   self-contained: no dependence on the previous conversation.
4. State plainly in the handoff what is verified and what is only planned.
