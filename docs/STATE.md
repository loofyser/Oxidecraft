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
  T4 the hash-verified atomic store (`ad563f5`, fix round 1); T5 piston-meta parsing (`0fc73cb`).
  T6 `fetch --verify` is implemented on `main` with fix round 1 applied, awaiting its re-review.
- Tests: 85 passing workspace-wide, 2 ignored (both require the live endpoints), zero failures
  (`cargo test --workspace`).
- Live evidence: our own `cargo run -p oxide-launcher -- ping 127.0.0.1:25565` answers
  `1.8.9 — protocol 47 — 0/20 players`; the vanilla client title screen is captured at
  `refs/rig/evidence/minecraft-1.8.9-title-screen.png`; the full fetch run is recorded under
  "M0 evidence" below.
- Remaining M0 tasks: T7 jar extraction, T8 wgpu window, T9 CI and guards, T10 hygiene and the
  `m0` tag.
- Review: `docs/reviews/2026-09-22-spec-review.md`, with a disposition record for every finding.
- Research: five evidence-backed reports in `docs/research/`, indexed in Appendix B of the spec.
- Parity checklist classification: `docs/parity/checklist.md`.
- Verification rig: under `refs/rig/` (offline-mode 1.8.9 server plus a vanilla client, see
  `refs/rig/README.md`).
- Repo: https://github.com/loofyser/Oxidecraft — `main` pushed; the last code commit carries the
  fetch flow, and documentation commits follow it. Confirm HEAD with `git log --oneline -3`.

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

## Decisions locked

See spec section 4 for the full table. The short version: multiplayer-first v1; Microsoft
auth at M7; GPL-3.0; RustCraft is a read-only reference with zero code copied; assets come
from piston-meta into our own verified store with vanilla-install reuse; performance targets
are comparative (1.5x FPS, under 50% memory, under 1 second cold start).

## Next actions

1. Re-review the Task 6 fix round on `main`; mark T6 complete in the ledger when every finding is
   addressed.
2. Continue the M0 plan from Task 7 (jar extraction) in order, one task per dispatch, with the task
   review and fix loop after each.
3. Every dispatch carries the standing rules: no AI or tooling language in committed files, commit
   messages or code comments; explicit `git add <paths>`; never touch `.superpowers/`.

## Environment facts (development machine)

| Fact | Value |
| --- | --- |
| OS / session | CachyOS, kernel 7.2.2, Wayland with GNOME (mutter). XWayland is provided by mutter's `Xwayland :1`, so the 1.8.9 rig client (LWJGL 2) runs without extra setup. Earlier notes say niri; the desktop was switched to GNOME on 2026-09-22 |
| Rust | rustc 1.95.0, cargo 1.95.0 |
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
