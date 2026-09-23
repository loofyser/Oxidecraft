# Oxidecraft — project state

Updated: 2026-09-22

This file is the live state of the project. Keep it current before every handoff, long pause,
and milestone boundary. Anyone picking the work up should be able to read this file plus the
specification and continue without asking questions that are already answered here.

## Where we are

- Stage: milestone M0 in progress. Spec v2 is approved, and M0 runs from
  `docs/plans/2026-09-22-m0-foundations.md`, one task at a time. The working ledger (gitignored)
  is `.superpowers/sdd/2026-09-22-m0-foundations/progress.md`.
- M0 tasks complete: T1 workspace and VarInt codec; T2 framing with the 1.8 compression rules
  (fix round 1 applied); T3 handshake, status ping and the launcher CLI. T3's fix round landed as
  `4c0db19`; its scoped re-review is the immediate next action.
- Tests: 36 passing workspace-wide, zero failures (`cargo test --workspace`).
- Live evidence: our own `cargo run -p oxide-launcher -- ping 127.0.0.1:25565` answers
  `1.8.9 — protocol 47 — 0/20 players`; the vanilla client title screen is captured at
  `refs/rig/evidence/minecraft-1.8.9-title-screen.png`.
- Remaining M0 tasks: T4 hash-verified store, T5 piston-meta parsing, T6 `fetch --verify`,
  T7 jar extraction, T8 wgpu window, T9 CI and guards, T10 hygiene and the `m0` tag.
- Review: `docs/reviews/2026-09-22-spec-review.md`, with a disposition record for every finding.
- Research: five evidence-backed reports in `docs/research/`, indexed in Appendix B of the spec.
- Parity checklist classification: `docs/parity/checklist.md`.
- Verification rig: under `refs/rig/` (offline-mode 1.8.9 server plus a vanilla client, see
  `refs/rig/README.md`).
- Repo: https://github.com/loofyser/Oxidecraft — `main` pushed; the last code commit is `4c0db19`,
  documentation commits follow it. Confirm HEAD with `git log --oneline -3`.

## Decisions locked

See spec section 4 for the full table. The short version: multiplayer-first v1; Microsoft
auth at M7; GPL-3.0; RustCraft is a read-only reference with zero code copied; assets come
from piston-meta into our own verified store with vanilla-install reuse; performance targets
are comparative (1.5x FPS, under 50% memory, under 1 second cold start).

## Next actions

1. Generate the Task 3 re-review package over `1a06e52..4c0db19`, dispatch the scoped re-review, and
   mark Task 3 complete in the ledger when every finding is addressed.
2. Continue the M0 plan from Task 4 (the hash-verified store) in order, one task per dispatch, with
   the task review and fix loop after each.
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
