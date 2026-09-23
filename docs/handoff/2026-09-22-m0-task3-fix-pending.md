# Handoff — M0 in progress, Task 3 fix awaiting re-review

Date: 2026-09-22. Repo: https://github.com/loofyser/Oxidecraft. Working copy:
`/home/lucy/Desktop/Software/Projects/Oxidecraft`. Branch `main`, pushed, tree clean. The last code
commit is `4c0db19`; documentation commits follow it — confirm HEAD with `git log --oneline -3`.

## Mission

Oxidecraft is a from-scratch Rust re-implementation of the Minecraft Java Edition 1.8.9 client
(protocol 47), with a wgpu renderer and its own launcher. Milestone M0 (Foundations) is being
executed task by task from `docs/plans/2026-09-22-m0-foundations.md`.

## Read first, in this order

1. `docs/STATE.md` — the live project state.
2. `.superpowers/sdd/2026-09-22-m0-foundations/progress.md` — the working ledger: what is complete,
   every ruling made, the parked minors, and the exact commit ranges. This file is gitignored; it is
   the recovery map.
3. `docs/specs/oxidecraft-v1-design.md` — the approved specification (v2). Section 5 for the crate
   graph, section 12 for CI, appendix A for dependencies.
4. The plan file above, for whichever task is next.

## Rules that must not be broken

- GPL-3.0. No Mojang asset, jar, `.ogg`, `.png` or `.class` file may be committed, ever.
- Zero code copied from RustCraft (its licence is noncommercial source-available); it is a read-only
  reference. Adaptations from MIT/Apache sources get a line in `NOTICE`.
- **No AI or tooling language anywhere** — not in commit messages, code comments, docs, or reports.
- Explicit `git add <paths>`; never `git add -A`. Never commit `.superpowers/` (it is gitignored).
- No `sudo` except as the leading command of a foreground call. The rig is user-space only.
- Commits go to `main` (trunk-based is the approved convention). Push after each task.

## Current stage

Verified working:

- `cargo test --workspace` → 36 passed, 0 failed.
- T1 workspace (8 crates) and VarInt codec; T2 framing with the 1.8 compression rules; T3 handshake,
  status ping and launcher CLI — all reviewed clean (T1 and T2 after one fix round each).
- Live: `cargo run -p oxide-launcher -- ping 127.0.0.1:25565` → `1.8.9 — protocol 47 — 0/20 players`,
  against the local vanilla 1.8.9 rig server.
- Rig fully verified (server listening and answering; vanilla client reaches its title screen;
  `refs/rig/README.md` and `refs/rig/evidence/`).

Unfinished:

- T3's fix round (`4c0db19`) has NOT been re-reviewed yet. Its two Important findings were the
  timeout classification and the unbounded `connect`; the fix also carried clarity minors.
- M0 T4–T10 have not started: T4 store, T5 piston-meta parsing, T6 `fetch --verify`, T7 jar
  extraction, T8 wgpu window, T9 CI plus guards, T10 hygiene and the `m0` tag.

## Exact next step

Generate the Task 3 re-review package and dispatch the scoped re-review:

```
bash /home/lucy/.hermes/plugins/superpowers/skills/subagent-driven-development/scripts/review-package \
  docs/plans/2026-09-22-m0-foundations.md 1a06e52 4c0db19
```

Then dispatch a re-reviewer with: the task brief
`.superpowers/sdd/2026-09-22-m0-foundations/task-3-brief.md`, the report
`task-3-report.md`, and the printed diff path. Findings to verdict: (1) read timeout must be
classified as `PingError::Timeout`, not framing; (2) the timeout must bound connection
establishment (`connect_timeout` over all resolved addresses); (3) module doc reword; (4) malformed
JSON length gets `BadLength` rather than `Truncated`; (5) `Description` catch-all arm; (6) `MOTD:
(none)` line; (7) a literal-bytes reply test. On "all findings addressed", append
`Task 3: complete (commits 239af81..<head>, review clean)` to the ledger and continue with Task 4.

## Open questions for the human

None blocking. Two informational notes:

1. The rig's PrismLauncher shows a one-off setup wizard on manual launch; its Finish button skips
   the Microsoft login and is not needed (the offline account is enough).
2. The plan's `Fetch` subcommand prints a placeholder until Task 6 implements it.

## Environment notes

- CachyOS, Wayland with niri. Rust 1.95.0, edition 2024, MSRV declared 1.85.
- Rig: `refs/rig/` — Temurin JRE 8, server on 25565 (offline mode, seed `oxidecraft`), Prism
  instance `OxideRef-1.8.9`. Start/stop via `refs/rig/server/start.sh` / `stop.sh`.
- `gh` is authorized as `loofyser`; git push works through gh's credential helper.
- Subagent dispatch has no model parameter on this platform; every child inherits the session model
  (ledger ruling 3).

## Verification before declaring anything done

```
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p oxide-launcher -- ping 127.0.0.1:25565
git status --short
```
