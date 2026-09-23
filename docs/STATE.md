# Oxidecraft — project state

Updated: 2026-09-22

This file is the live state of the project. Keep it current before every handoff, long pause,
and milestone boundary. Anyone picking the work up should be able to read this file plus the
specification and continue without asking questions that are already answered here.

## Where we are

- Stage: specification v2. Milestone M0 has not started.
- Specification: `docs/specs/oxidecraft-v1-design.md` v2 — all 25 findings of the independent
  review applied. Awaiting the owner's approval gate.
- Review: `docs/reviews/2026-09-22-spec-review.md`, with a disposition record for every finding.
- Research: five evidence-backed reports in `docs/research/`, indexed in Appendix B of the spec.
- Parity checklist classification: `docs/parity/checklist.md`.
- Verification rig: under `refs/rig/` (offline-mode 1.8.9 server plus a vanilla client, see
  `refs/rig/README.md`).
- Repo: https://github.com/loofyser/Oxidecraft — `main` pushed.

## Decisions locked

See spec section 4 for the full table. The short version: multiplayer-first v1; Microsoft
auth at M7; GPL-3.0; RustCraft is a read-only reference with zero code copied; assets come
from piston-meta into our own verified store with vanilla-install reuse; performance targets
are comparative (1.5x FPS, under 50% memory, under 1 second cold start).

## Next actions

1. Owner approves the specification, or requests changes.
2. Invoke the `writing-plans` skill to produce the M0 implementation plan.
3. Execute M0: workspace, CI (asset guard, `deny.toml`, cross-target checks), `oxide-launcher
   fetch --verify`, jar extraction with manifest, blank wgpu window, README disclaimer, rig proof.

## Environment facts (development machine)

| Fact | Value |
| --- | --- |
| OS / session | CachyOS, kernel 7.2.2, Wayland with niri |
| Rust | rustc 1.95.0, cargo 1.95.0 |
| Vulkan | instance 1.4.357; Intel Iris Xe (card2) and NVIDIA T500 (card1) |
| Java | Java 26 system-wide; the rig uses a standalone JRE 8 tarball |
| gh CLI | 2.101.0, authorized as loofyser, scopes repo, read:org, gist |
| git push | uses gh as the credential helper for github.com (`gh auth setup-git` has been run; plain `git push` works now) |
| Minecraft installs | none on this machine; assets are downloaded into the Oxidecraft store |
| sudo | password required, and only usable as the leading command of a foreground call |

## Local-only directories

`refs/` (upstream clones, rig) and `vanilla/` (Mojang client jar and asset index) are
gitignored and never pushed. Neither is ever redistributed.

## Handoff protocol

1. Update this file: stage, what changed, next actions, any new environment facts.
2. Commit and push everything, so the next person can pull it.
3. Write `docs/handoff/YYYY-MM-DD-<topic>.md` from `docs/handoff/TEMPLATE.md`, fully
   self-contained: no dependence on the previous conversation.
4. State plainly in the handoff what is verified and what is only planned.
