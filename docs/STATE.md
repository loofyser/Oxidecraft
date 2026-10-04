# Oxidecraft — project state

Updated: 2026-10-02

This file is the live state of the project. Keep it current before every handoff, long pause, and
milestone boundary. Anyone picking the work up should be able to read this file plus the
specification and continue without asking questions that are already answered here.

## Where we are

- Stage: **milestone M3 (the player) is complete and tagged `m3`** — spec section 13 of
  `docs/specs/oxidecraft-v1-design.md` (v6), plan `docs/plans/2026-10-02-m3-player.md` (fourteen
  tasks plus a scoped fix round; base `551565b`; all closed and independently reviewed). The
  annotated tag marks this close-out commit; the reviewed code head is `2276d22`, and the final
  whole-branch review over the milestone — 26 commits, `551565b..2276d22` (+20,246/−602, 40
  files) — returned **approved**: 0 code must-fixes, the deferred-minor triage 0 must-fix / 31
  safe-to-leave, and the one evidence item (the stage-4 crack capture) closed by the completion
  re-shoot. The task chain: T1 the tick loop and input (`e48dc2e`, fix `8ab624e`); T2 the
  movement model (`ca845c2`, docs `2b5821d`, fix `6ac88e0`); T3 the collision view (`b76e431`,
  docs `6edd892`, fix `fea5221`); T4 client input wiring (`1d4c782`); T5 movement packets and
  self-state (`2d5f41e`, docs `9336922`); T6 block-change packets and light hooks (`7939971`,
  fix `bb08dd8`, docs `37dd104`); T7 the aim raycast (`385a6be`, fix `8c65932`, docs `cba5aad`);
  R1 the light-bounds fix (`f5116b0`); T8 digging and break progress (`70acda2`); T9 placing
  with prediction (`91af829`, docs `0a5b1a9`); T10 death and respawn (`df87db4`, fix `6522f1e`);
  T11 the FOV camera, bobbing and the hurt roll (`b3ab2f7`); T12 the crack overlay and outline
  (`de1baea`); T13 the acceptance run with its defect fix (`2276d22`). M2 (textured terrain) is
  complete and tagged `m2` — the annotated tag marks its close-out commit `e90a30d`, its
  reviewed code head was `38ac081`, and its final whole-branch review returned PASS (its task
  chain lives in `docs/plans/2026-09-27-m2-textured-terrain.md` and its 2026-10-02 update bullet
  below). M1 is complete and tagged `m1` (the annotated tag marks the close-out commit `930ccab`;
  the reviewed code head is `854ca85` and its final whole-branch review returned ready to merge).
  M0 is complete and tagged `m0`. Every task's brief, report, review and every ruling live in the
  milestone ledgers (local, git-ignored; M3's in `.superpowers/sdd/2026-10-02-m3-player/`).
- **2026-10-02 update — M2 closed.** After the stop, the fluids round resumed and closed
  (`f366236`, reviewed clean); the full acceptance re-capture (continuations #3/#3b) followed; the
  residual round refuted the carried ~1 % frame-scale class on the current frames and fixed two
  real causes (`d66b597` variant world-position hash, `8f0bb5f` depth LEQUAL, reviewed clean); the
  terrain-shading round found and fixed the plain-cutout sampler cause (`2e3d962` — vanilla draws
  the plain `CUTOUT` layer at level 0 with a nearest sampler; ours sampled mips; reviewed clean);
  Task 15 closed with the trio committed (`54e51d4` the sample-wall cells, `82d3090` the parity
  tool and the baseline). The final whole-branch review over M2's 67 commits returned PASS. A CI
  clippy failure (the runner's newer stable `for_kv_map`, red since 2026-09-27) was found at
  close-out prep and fixed (`38ac081`; reviewed clean; CI run `36999029133` green, all seven jobs).
  **The acceptance state:** all three scene pairs fail the masked bar (wall 0.0217/0.0129, mark
  0.0152/0.0125, ground 0.1769/0.0814; bar over-8 ≤ 0.02, over-24 ≤ 0.01) with every class
  documented — the close proceeded with the classes documented per the owner's ruling. The
  milestone's record: `refs/rig/evidence/m2/acceptance-notes.md`, `refs/m2-terrain-shading/report.md`,
  and the round dirs under `refs/`.
- **2026-10-04 update — M3 closed.** The acceptance run finished with its recorded corrections in
  `refs/m3-acceptance/report.md` (the movement record-file fix; the creative leg's give-then-
  respawn ruling; the `shot.sh` mode fix; plus the completion addendum); controller verification
  re-derived every headline number from the artifacts, and the run's one real defect — a
  mid-session game-mode change going untracked (stale survival reach) — closed in the scoped fix
  `2276d22` (reviewed MET, no fix wave). The completion run added the accepted placement with the
  vanilla-visible before/after frames, the burst numbers (raw — the removal straddle was not
  observed), and the FPS release-vs-debug answer (the movement collapse is debug-build-specific).
  The final whole-branch review over the milestone returned **approved** (0 code must-fixes;
  31/31 deferred items safe-to-leave; the stage-4 crack evidence item completed by the close-out
  re-shoot). The milestone's record: `refs/m3-acceptance/`, `refs/m3-task-13-completion/`,
  `refs/m3-task-13-crack/`, `refs/m3-final-review/`, and the M3 ledger
  (`.superpowers/sdd/2026-10-02-m3-player/`).
- M0 delivered and verified: the eight-crate workspace with its enforced dependency graph; the
  VarInt codec and length-prefixed framing with the 1.8 compression rules; handshake, status ping
  and the launcher CLI; the hash-verified atomic store; piston-meta metadata parsing; the full
  `fetch --verify` flow, run against the real endpoints; jar extraction with a manifest; the wgpu
  window with the FPS counter and adapter logging; CI with the crate-graph, asset and licence
  guards and the release build.
- M1 delivered and verified: the buffered framed connection with the compression handover; the 1.8.9
  login and play packet codecs; the live capture with its findings document and chunk fixtures; the
  `0x21`/`0x26` column decoder; the chunk store; the session with the five connection obligations;
  the block palette and the section mesher; the terrain types and camera; the wgpu terrain pipeline
  with the depth buffer and the text overlay; the client's `--server` wiring with the F3-style
  overlay; and the negative self-tests for the asset and crate-graph guards, wired into CI.
- Tests: 786 passing workspace-wide, 25 ignored, zero failures at the M3 reviewed head
  (`cargo test --workspace`; `#[test]` 811 − 25 `#[ignore]`; the final review's gate re-run EXIT 0,
  `.superpowers/sdd/2026-10-02-m3-player/final-review.md`; the milestone's base `de1baea` reads
  781 passing / 0 failed / 25 ignored, 806 − 25). The ignored set at the close is 25 — M2's 21 plus
  three new render GPU cases and one new store case: 16 GPU tests (fifteen in `pipeline_headless`,
  one in `headless`; they pass locally, 16/16 on the T500), 6 store-dependent tests (five in
  `oxide-assets`, one in `oxide-game`) that run when `OXIDECRAFT_STORE` points at the store
  (`~/.local/share/oxidecraft`), 2 live-endpoint metadata tests, and the sample-wall rig helper in
  `oxide-world` (`behaviour.rs`); the full ignored leg reads 25/0 with the store set.
- Live evidence: the M0 ping and captures stand; M1's acceptance run — the join, 180 seconds of
  keep-alives, the mark and both screenshots, and the proxy capture of our client's own traffic — is
  recorded under "M1 evidence" below, with the raw files under `refs/rig/evidence/m1/`. M2's live
  evidence is the three acceptance scene pairs, their masks and the parity JSONs under
  `refs/rig/evidence/m2/`, with the round chains and reports under `refs/m2-fluids/`,
  `refs/m2-acceptance-residual/`, `refs/m2-terrain-shading/` and `refs/m2-task-15/`. M3's live
  evidence is the acceptance run (`refs/m3-acceptance/`: the movement vectors, the creative leg,
  death/respawn, the stream spot-check and the latency proxy with its raw numbers), the completion
  run (`refs/m3-task-13-completion/`: the accepted placement with the vanilla-visible frames, the
  burst frames, the FPS A/B) and the close-out crack capture (`refs/m3-task-13-crack/`) — numbers in
  "M3 evidence" below.
- CI: seven jobs — format/lint/test, MSRV 1.85.0, portability (`x86_64-pc-windows-gnu`), release
  build, crate graph, asset guard, licences and advisories. The M0 tagged head `fe2d3b6` is green in
  run `35873490870` (six jobs; the release build was added after the tag). The M1 reviewed head
  `854ca85` is green in run `36288859598` (all seven jobs), and the M1 close-out commit `930ccab`
  (the commit the `m1` tag marks) is green in run `36300166552` (all seven jobs). The M2 reviewed
  head `38ac081` is green in run `36999029133` (all seven jobs), and the M2 close-out commit
  `e90a30d` (the commit the `m2` tag marks) is green in run `36999837908` (all seven jobs). **M3:**
  every checkpoint was CI-checked as it landed; the pre-fix head `de1baea` — the parent of the
  milestone's fix commit — is green in run `37177842220` (all seven jobs), and the close-out
  commit `35f2d21` (the commit the `m3` tag marks) is green in run `37195043678` (all seven
  jobs). Caveat from the run: between 2026-09-27 and 2026-10-02 main was red (the runner's
  newer stable clippy, `for_kv_map` at `crates/oxide-assets/tests/atlas.rs:178`; the local stable
  was six releases behind) — fixed by `38ac081`; keep the local stable current. Caveats to carry forward:
  `aarch64-apple-darwin` is not checked on Linux runners because its C dependencies need the macOS
  SDK; the MSRV job installs the 1.85.0 toolchain per run; `cargo-deny` is pinned to 0.20.2 and
  installed per run. The M0 caveat about the guard scripts being exercised only on the happy path is
  closed: both guards carry `--self-test` modes since `854ca85`, and CI runs each one before its
  normal check.
- Review: `docs/reviews/2026-09-22-spec-review.md`, with a disposition record for every finding; the
  per-task reviews, the fix-round reviews and each milestone's final whole-branch review are in the
  milestone ledgers and round dirs (local, git-ignored: M2's in
  `refs/m2-final-review/review-report.md`; M3's in `.superpowers/sdd/2026-10-02-m3-player/`,
  including the approved `final-review.md`).
- Research: five evidence-backed reports in `docs/research/`, indexed in Appendix B of the spec.
- Parity checklist classification: `docs/parity/checklist.md`.
- Verification rig: under `refs/rig/` (offline-mode 1.8.9 server plus a vanilla client, see
  `refs/rig/README.md`).
- Repo: https://github.com/loofyser/Oxidecraft — `main` pushed and tagged (`m0`, `m1`, `m2`, `m3`), with
  the M3 chain pushed through the close-out this record closes. Confirm HEAD with `git log --oneline -3`.

## M0 evidence

Acceptance run: `fetch --verify`, real endpoints, default store at `<data dir>/oxidecraft`, debug
build (`cargo run -p oxide-launcher -- fetch --version 1.8.9 --verify`), 2026-09-23:

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
- Post-review fix (2026-09-23): the store lock is now an operating-system lock over
  `<store root>/lock`, held for the run and released when the process dies, so a killed run cannot
  lock out the next one; the file itself stays behind with the last run's process id in it.

Acceptance run: jar extraction, the real client jar already in the store, default store at
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

Acceptance run: wgpu window, a real window on this machine's GNOME/Wayland desktop, debug build
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
  `WAYLAND_DISPLAY` unset) reached the same title and a steady 60 fps, so both display paths work.
- Escape and window-close exits were not exercised end to end: window enumeration is
  unavailable on this desktop and synthetic key events are not delivered to the XWayland client.
  The run above exits through the frame limit, which reaches the same `event_loop.exit()`; the
  escape rule itself is unit-tested.
- Post-review fix (2026-09-23): a stale surface (`Outdated`, `Lost`) is now reconfigured and the
  frame retried once instead of stopping the client, a `Timeout` frame is dropped, the error logs
  carry the full cause chain, and the clear colour's sRGB difference is recorded in
  `docs/DIVERGENCES.md` (entry 4).

## M1 evidence

Acceptance run: the client against the rig server, `./target/debug/oxide-client --server
127.0.0.1:25565 --username OxideDev`, 2026-09-26 (evening, CDT; log timestamps are UTC), raw files
under `refs/rig/evidence/m1/`:

```
[20:55:45] OxideDev[/127.0.0.1:58520] logged in with entity id 3757 at (20.5, 64.0, 174.5)   (server)
2026-09-27T01:55:46.086719Z  INFO oxide_client: joined the world entity_id=3757 gamemode=0 dimension=0 …
2026-09-27T01:55:48.058929Z DEBUG oxide_client: keepalive answered id=22184315                (client;
2026-09-27T01:58:44.209128Z DEBUG oxide_client: keepalive answered id=22360615                 87 lines)
```

- Joined the world as entity 3757; **87 keep-alive echoes over 180 s** (~2.05 s apart) with no
  server-side kick, then stopped deliberately (the server logs the stop as `Disconnected`, not a
  fault).
- The mark: `/fill 9 140 172 21 140 184 minecraft:wool 14` and `/fill 9 141 172 21 145 184
  minecraft:stone` placed a 13×13 stone column over its red-wool base above y=128; `/tp OxideDev 31
  150 194 135 20` teleported the client onto it (`Teleported OxideDev to 31.5, 150.0, 194.5`), and a
  second teleport (`15 71 178 0 0`) returned it to the ground (15.5, 71.0, 178.5).
- Screenshots (niri's own focused-output capture; see the Environment facts): `terrain-both-halves.png`
  — the mark as a grey stone block over the red wool base, grass, dirt, water, sand and trees below,
  the overlay legible with `x/y/z: 31.500 / 150.00000 / 194.500`, `Facing: north (135.0 / 20.0)`,
  `Server: 127.0.0.1:25565 (protocol 47)` — and `terrain-ground.png` (overlay `x/y/z: 15.500 /
  71.00000 / 178.500`). Both show geometry above and below y=128, with no magenta. The vanilla client
  was teleported to the same two spots for eyeball comparison (`vanilla-on-mark.png`,
  `vanilla-ground.png`).
- Our client's own traffic (recorder proxy on 25565, server behind it on 25566, non-loopback upstream
  so the server negotiates compression; `oxide-client-traffic.*`): the handshake
  `0f 00 2f 09 "127.0.0.1" 63 dd 02` (protocol 47, next state 2); `0x03 Set Compression threshold=256`
  **before** `0x02 Login Success`, the client's first compressed frame being #2; the brand payload
  `07 76 61 6e 69 6c 6c 61` (`\x07vanilla`); Client Settings `en_US`, view distance 8; the position
  echo; and **56/56 keep-alive echoes, none missing, none unmatched, median interval 2.0499 s**.
- The capture's chunk findings: the rig's vanilla server **does emit `0x26`** — run A 9×`0x21` +
  44×`0x26` (414 columns), run B 2×`0x21` + 42×`0x26` (416 columns); our client's own session
  received 1×`0x21` + 43×`0x26` (431 distinct columns, zero size-formula mismatches). The findings
  document is `docs/research/protocol-47-live-capture.md` and the committed fixtures are under
  `crates/oxide-proto-v47/tests/fixtures/m1-capture/`.
- Two defects the acceptance run uncovered, both fixed on `main` with covering tests: `8b14e4e`
  (keep-alive starvation — the mesh rebuilds ran inside the read loop; they are now queued and
  drained between reads) and `0187ae8` (palette coverage — 22 block ids the rig's world carries had
  no palette entry).

## M2 evidence

Acceptance: the three scene pairs (ours vs the vanilla client on the rig, placed per appendix C),
measured by `scripts/parity-diff.py`; bar: over-8 <= 0.02, over-24 <= 0.01 of the frame, masked per
the derived rect sets (each rect justified and census-checked; never widened). Poses: wall
`/tp 7 57 174 180 0`; mark `/tp 31.5 150 194.5 135 20`; ground `/tp 15 71 178 0 0`; every capture
with the scene setup + `/time set 6000` + `/weather clear` re-sent and both clients soaked past the
mesh drain.

- Final numbers (masked over-8 / over-24; raw figures in the parity JSONs): **wall 0.0217 / 0.0129**,
  **mark 0.0152 / 0.0125**, **ground 0.1769 / 0.0814** - all three fail the bar as written; every
  class is documented in `refs/rig/evidence/m2/acceptance-notes.md` (the round-4 top section carries
  the close note; earlier sections keep their history with correction notes). The close proceeded
  with the classes documented per the owner's ruling (2026-10-02).
- The documented classes at the close: the wall's right-slope edge (close-range plants + the cloud
  band; the sampler cannot act at level 0 - recorded open), the chest cell (11, 58, 164; the missing
  tile-entity path; not masked), the fluids' static phase (ours draws the animation's first frame;
  the floor is measured and named), the ground's remaining tallgrass/terrain-shading residue (the
  sampler fix moved the in-region class 0.3307/0.1796 -> 0.2801/0.1330), and the mark's
  deck-over-terrain phase overlap (attributed, not masked).
- The wall, for the record: 122/122 cells checked across start and teardown
  (`refs/rig/tools/sample_wall_cells.txt`).
- The metric's tool: `scripts/parity-diff.py` (committed with the baseline; `--self-test` runs in
  the gate, 19 checks).
- Baseline (`docs/perf.md`, the C.4 route, build `72526bf`): our client mark 10.4 / wall 12.2 /
  ground 5.6 / far end 15.2 fps frames-delta; VmRSS flat at 515-534 MiB across the route; cold
  start 7.7 s; the vanilla client's memory 1.19-1.22 GB and its frame rate recorded as M9's to
  complete (no working input path for the F3 readout on this desktop). M9 re-runs the full protocol
  on the final build.
- Atlas on the acceptance run: 2048x2048, 5 levels, 377 sprites (`oxide-client-capture2.log:2`).

## M3 evidence

Acceptance: the Task 13 run (`refs/m3-acceptance/report.md` — original + the correction blocks +
the completion addendum + the crack addendum) on the rig (server + our debug client; two clients
for the creative leg), recorded 2026-10-03/04. The headline numbers, all re-derived from the
artifacts at verification:

- Movement (scripted, from the `*.script.log` tick logs): walk **4.317181 m/s** (dev 0.004 %),
  sprint **5.612335** (0.006 %), jump apex **1.249187** (t269–270) — against the design vectors
  4.317 / 5.612 / 1.24919 within the ±0.1 % / ±0.01 tolerances.
- The creative leg (give-on-live rule): placement accepted server-side — console probe
  `Successfully found the block at 99,71,102.`, exactly one 0x08 (frame #1495, target (100,71,102)
  west, cursor [0,9,8]); the vanilla client's before/after frames show the edits
  (`refs/m3-task-13-completion/vanilla-placed-{before,after}.png`).
- Death/respawn: `/kill` → death view → scripted respawn; no stale dig/aim; the respawn-cleared
  hand is the recorded give-then-respawn class (known-limit 9).
- The serverbound stream spot-check (recorder proxy, one combined script): the packet counts
  reproduce from the capture (`verify_spotcheck.py`; movement cadence, the dig [status 0, status
  2], sprint 0x0B edges, one 0x08, 0x16 after the place; chain dig #848 < place #1073 < respawn
  #1781).
- The latency proxy: stage timeline at 0.75 s spacing (20 Hz same-pass cadence); the removal
  straddle was not observed — all burst frames land 0.02–0.03 s pre-removal inside the fixed
  window (reported raw; fine-grained measurement is M9's full protocol).
- The crack overlay (close-out re-shoot, final-review item E1 — produced, not waived): stages 0→8
  landed + completion at ~0.75 s cadence; mid-dig frames `our-crack-stage-1..7.png` with the
  calibrated dark-share rising over the target face (baseline 5.74 → 11.25 / 25.24 / 31.24; the
  last three post-completion); wire check one dig start + one finish at (100,71,100) west, 0
  failures; probes stone → air. The re-shoot re-posed to the acceptance's own vanilla crack pose
  (`vanilla-crack-pose.commands`, 3.64 blocks) after the original step-4 sightline (~4.6 blocks)
  proved outside survival reach (4.5) live; the target cell and scene are the acceptance's.
  `refs/m3-task-13-crack/` carries the frames, the target-region crops (the operator's by-eye
  aid), the metrics JSON and the wire JSON; the step-4 sprite identity is separately proven by
  Task 12's GPU case.
- The FPS side question: under the identical movement segment the debug build collapses (moving
  min/median/max 6.9/80.8/131.9) where the release build holds a flat ≈144 (95.7/144.1/224.3);
  idle 74.7/126.0/193.3 vs 117.3/144.5/167.2 — debug-build-specific; the counter is honest
  (`refs/m3-task-13-completion/fps/`).

The defect the run surfaced — a mid-session game-mode change going untracked (stale survival
reach) — closed in the milestone's scoped fix `2276d22` (reviewed MET, 0C/0I/2M; the two cosmetic
minors went to the final review). The operator's by-eye list (the vanilla placement frames, the
death-view frame, the crack crops) stands as listed under Caveats.

## Caveats

Carried by M1 as delivered (each confirmed by the final whole-branch review):

- **Set Compression arriving after Login Success is unhandled by construction.** The session switches
  to the play state on Login Success, so a late `0x03` lands in the play wildcard and fails cleanly.
  A heuristic cannot distinguish it from a play-state Time Update on a server that legitimately runs
  with compression disabled; the capture and the rig fix the order as "before". A modded server that
  reorders compression breaks with a clear error — M1's scope is the rig.
- **An unload does not re-mesh neighbours**: a column that unloads can leave transient border holes
  in a loaded neighbour's mesh until that neighbour next rebuilds.
- **Meshing is single-threaded** and runs on the session thread between reads; applying a column
  rebuilds that column plus its four neighbours.
- **A server that never leaves a ≥20 ms idle gap starves the mesh queue** (unbounded growth): the
  idle read waits at most one 20 ms tick and drains one batch per wait. Fine for the rig; recorded
  for M2.
- **The debug font is scaffolding** (Decision 7): a small embedded 5×7 bitmap table, one opaque quad
  per set pixel; the jar's font (`ascii.png`) replaces it in M2.
- **Water draws opaque**, like every block: M1 renders flat colours with no transparency or light.
- **No live uncompressed-play capture exists on this rig**: run A's loopback premise was refuted (the
  server negotiated compression over the proxy anyway). Uncompressed framing is covered by the login
  phase, every sub-threshold frame and the synthetic self-test.
- **The acceptance run's raw server-console output was not preserved**: the rig's log rotation keeps
  only `latest.log` plus 2-line gz archives, so the acceptance note's console quotes are corroborated
  by the client log, the screenshots and the capture, but they cannot be re-verified from the
  artifacts (see the backlog: fix the rotation).

Carried by M2 as delivered (each confirmed by the final whole-branch review):

- **Fast graphics only** - the option that flips the setting, the cloud prisms and the per-corner
  tint ride with the options screen (M6); the acceptance and every M2 session ran Fast.
- **No weather** - rain strength is a parameter fixed to 0 until the weather packets land in M6;
  the rain-darkened fog and sky are unverified.
- **The sunrise/sunset band is not implemented** - the acceptance captures at noon where it is
  invisible; M6's sky work adds it with a dawn capture.
- **Clouds' offset rule** - the client-local counter is the documented stand-in for the missing
  tick loop; the cloud band's masking in the metric is derived and recorded.
- **Translucent sorting is per section and static within a section** - a large water surface may
  show ordering artefacts the metric does not chase.
- **The atlas is rebuilt at startup** - R3's "no atlas rebuild" clause needs a cache, which is
  M9's.
- **The baseline departs from C.4** (no warm-up, no median; vanilla's frame rate deferred to M9)
  and **the capture path departs from C.2 step 3** (the KWin/spectacle route replaces the portal) -
  both recorded with their reasons in `docs/perf.md` and the rig notes.
- **The uinput HUD-hide attempt failed on this desktop** (no virtual-keyboard protocol for KWin;
  `/dev/uinput` not usable non-interactively) - recorded in the plan and the rig notes; our own
  captures run `--no-overlay`.
- **The acceptance residue** - all three pairs fail the masked bar as written; every class is
  documented in `refs/rig/evidence/m2/acceptance-notes.md`, and the close proceeded with the
  classes documented per the owner's ruling (2026-10-02).
- **The M1 caveats this milestone closed**: unload re-meshes neighbours and meshing runs on a rayon
  pool (Task 13); the mesh-queue starvation bound (Task 13 + the keepalive fix `6959280`); the
  debug font (Task 14); water draws opaque (Task 9).

Carried by M3 as delivered (each confirmed by the final whole-branch review's criteria walk,
`.superpowers/sdd/2026-10-02-m3-player/final-review.md`):

- **The interactive Escape / mouse-capture-release live half is a documented non-pass**: no
  synthetic input path reaches KWin (the uinput attempt failed; `wtype` cannot connect), so the
  live half of that acceptance row rests on the unit-tested rule and the operator item below —
  recorded at the close rather than silently passed.
- **The crack overlay's live frames come from the close-out re-shoot** (final-review E1 — produced,
  not waived): the acceptance's own Step-4 sightline is ~4.6 blocks, outside the 4.5 survival reach
  (live-proven in the re-shoot), so it re-posed to the acceptance's own vanilla crack pose on the
  same target cell (numbers in "M3 evidence").
- **Crack stages are keyed by block position** — M4's breaker-id work re-keys the map.
- **`attacked_at_yaw` stays zero** until M4 tracks attackers (hit direction rides that).
- **The overlay wiring (aim target + stage map) has no wiring-level pin** — recorded for the next
  overlay touch.
- **Sprint-release-on-collision is not expressible against `physics::step`'s current return** — the
  collision-flag API change rides M4.
- **The sneak-eye offset (−0.08) is unwritten** and the plan's pin still says flat — the clause and
  the pin amendment are M4 items.
- **The latency proxy's removal straddle was not observed** (frames land 0.02–0.03 s early inside
  the fixed window) — reported raw; fine-grained measurement is M9's full protocol.
- **The replaceable refusals** (snow layer 78, vine 106, fire 51, the double-plant's rose side):
  plan known-limit 16.
- **The operator's by-eye list** (cannot be closed agent-side): the vanilla placement frames
  (`refs/m3-task-13-completion/vanilla-placed-{before,after}.png`), the death-view frame
  (`refs/m3-acceptance/our-death-view.png`), and the close-out crack crops
  (`refs/m3-task-13-crack/our-crack-*-targetregion.png`).

## Decisions locked

See spec section 4 for the full table. The short version: multiplayer-first v1; Microsoft
auth at M7; GPL-3.0; RustCraft is a read-only reference with zero code copied; assets come
from piston-meta into our own verified store with vanilla-install reuse; performance targets
are comparative (1.5x FPS, under 50% memory, under 1 second cold start).

## Next actions

Milestone M3 is complete and tagged; milestone M4 (spec section 13: entities and chat) is the live
milestone. The M4 plan is written and approved: `docs/plans/2026-10-04-m4-entities-and-chat.md`
(twenty-three tasks; independently reviewed, all review findings folded and re-verified; owner
approved 2026-10-04 with all three open questions answered as recommended). Execution starts on the
owner's explicit go, task by task in the established loop. The M3 close handoff is
`docs/handoff/2026-10-04-m3-close.md`.

The ordered M4 backlog (from the M3 final review's triage and carry-forward; the full list is
`refs/m3-final-review/deferred-list.md`, each item triaged safe-to-leave in
`.superpowers/sdd/2026-10-02-m3-player/final-review.md`):

1. The stage map is keyed by block position — M4's breaker ids re-key it (crack-overlay carry).
2. Express sprint-release-on-collision: give `physics::step` a collision flag in its return.
3. Add the sneak-eye offset clause (−0.08) and amend the plan's pin (the T11 note).
4. Wire `attacked_at_yaw` when M4 tracks attackers (hit direction rides that).
5. A wiring-level pin for the overlay wiring (aim target + stage map) at the next overlay touch.
6. The fog colour's time term (lags Time Updates) at the next client-fog touch.
7. `CHUNK_COORDINATE_BOUND` re-derivation note if the light recompute footprint changes.
8. Pin the `i64::MIN` receive rule and the end-drain expiry path (the M2 backlog's item 3 carries).
9. API polish: `bake_variant`'s precondition note; the negative face-rotation error shape; the
   six-column test name (the M2 backlog's item 5 carries); the M2-era comment/citation sweep and
   its plan-line amendment (the D10 carry).
10. The `slab_half` fixture when a later protocol surface needs it (the T3 note); T8's F7 and the
    remaining deferred minors per the deferred list.

The close recommendations (recorded 2026-10-04, for the owner's review): all 31 deferred items
stay safe-to-leave; the operator's by-eye list stands as listed (the vanilla placement frames, the
death-view frame, the close-out crack frames, and the interactive Escape live half as a documented
non-pass).

The milestone's acceptance evidence lives under `refs/rig/evidence/m2/` and `refs/m3-acceptance/`
(the completion and crack runs under `refs/m3-task-13-completion/` and `refs/m3-task-13-crack/`);
the rig runs against the store at `~/.local/share/oxidecraft` (`OXIDECRAFT_STORE`).

Then, as always: keep `docs/STATE.md` and `CHANGELOG.md` current and tag each milestone when it
closes; run the local gate before every push (`cargo test --workspace`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`,
`bash scripts/check-assets.sh`, `bash scripts/check-graph.sh`). The post-v1 programme (owner-directed
2026-09-23, spec section 17: singleplayer worlds and Java mod compatibility) starts only after v1
completes and does not affect the M2–M9 sequence.

## Resolved dependency versions

Pinned in `Cargo.lock` (committed). Direct dependencies as resolved at M3 (the M0 set, plus the M1
and M2 additions; M3 added none):

| Crate | Version | Notes |
| --- | --- | --- |
| `wgpu` | 26.0.1 | MSRV pin: the newest release whose declared rust-version (1.84) fits the workspace's 1.85 floor (`wgpu-hal` 26.0.6, `naga` 26.0.0) |
| `winit` | 0.30.13 | declares rust-version 1.70 |
| `ureq` | 3.4.2 | rustls, no default features (`rustls` 0.23.45) |
| `zip` | 7.2.0 | deflate only |
| `flate2` | 1.1.10 | `zlib-ng-compat` backend |
| `serde` / `serde_json` | 1.0.229 / 1.0.151 | derive |
| `clap` | 4.6.7 | derive |
| `thiserror` / `anyhow` | 2.0.20 / 1.0.104 | `thiserror` 1.0.69 is also present transitively |
| `tracing` / `tracing-subscriber` | 0.1.44 / 0.3.23 | env-filter |
| `sha1` / `hex` | 0.10.7 / 0.4.3 | |
| `tempfile` / `fs4` / `dirs` | 3.27.0 / 1.1.0 / 7.0.0 | |
| `glam` | 0.32.1 | `oxide-render` (terrain vertices, the camera). Do not bump past 0.32 under `-D warnings`: `Mat4::perspective_rh`/`look_to_rh` are deprecated from 0.33.1 (migrating to `glam::camera` is mechanical but a deliberate change) |
| `crossbeam-channel` | 0.5.17 | `oxide-game` and `oxide-client`, the session-to-window event channel |
| `png` | 0.18.1 | `oxide-assets`, the PNG texture loader (M2 Task 2) |
| `rayon` | 1.12.0 | `oxide-game`, the per-session mesh pool (M2 Task 13) |

## Environment facts (development machine)

| Fact | Value |
| --- | --- |
| OS / desktop | CachyOS, kernel 7.2.8, Wayland; **KWin (KDE Plasma) since 2026-09-29** (was niri 26.04; the M2 acceptance ran on KWin) on **two outputs**; `xwayland-satellite` on DISPLAY=:1, so the 1.8.9 rig client (LWJGL 2) runs without extra setup |
| Screenshots | On KWin (current): `spectacle -b -n -f -o <file>` full-screen, then crop to the window content rect (the M2 acceptance rect: window at (2000, 728, 1280×720); the overlay sits at screen (1720-1919, 1034-1079) — keep crops clear of it). On niri (past): the portal fails (niri asserts a single output); the substitute was `niri msg action screenshot-screen --write-to-disk true --show-pointer false --path <file>`. HUD keys: `wtype` cannot reach KWin (no virtual-keyboard protocol); `xdotool` delivered F1 but not F3 — the fps-readout gap is M9's. `computer_use` cannot enumerate windows unless `CUA_DRIVER_RS_ENABLE_WAYLAND=1` is set |
| Rust | rustc 1.99.0, cargo 1.99.0 (aligned to the CI runner's stable on 2026-10-02; `rust-toolchain.toml` says `channel = "stable"` — keep it current) |
| Dependency pin | `wgpu 26.0.1` is the newest release whose declared rust-version (1.84) fits the workspace's 1.85 floor (wgpu 27.0.0 declares 1.88, 28.0.0 declares 1.92, 29 and 30 declare 1.87), so cargo's resolver picked it; the pin is not a hand-downgrade. `winit 0.30.13` declares 1.70 (crates.io index metadata, 2026-09-23) |
| Vulkan | instance 1.4.357; Intel Iris Xe (card2) and NVIDIA T500 (card1) |
| Java | Java 26 system-wide; the rig uses a standalone JRE 8 tarball |
| gh CLI | 2.101.0, authorized as loofyser, scopes repo, read:org, gist |
| git push | uses gh as the credential helper for github.com (`gh auth setup-git` has been run; plain `git push` works now) |
| Minecraft installs | none on this machine; assets are downloaded into the Oxidecraft store |
| sudo | password required, and only usable as the leading command of a foreground call |
| Verification rig | `refs/rig/` — Temurin JRE 8, vanilla 1.8.9 server listening on 25565 (offline mode, seed `oxidecraft`, view-distance 10, compression threshold 256; capture runs move it to 25566 behind the recorder proxy on 25565), Prism instance `OxideRef-1.8.9` with an offline account. The proxy's upstream uses the machine's LAN address (10.0.0.84 on wlan0 at the acceptance run — DHCP, re-check before use). `refs/rig/README.md` documents start/stop; its screenshots section still describes the portal route — use the Screenshots row above instead. M2 added: run-stamped consoles (`logs/console-<UTC>.log`), the sample wall (`refs/rig/tools/sample_wall_cells.txt`), and the metric (`scripts/parity-diff.py`). M3 added the acceptance machinery (`refs/m3-acceptance/`; `shot.sh` is now executable — a 644 mode silently broke a burst run), the completion machinery (`refs/m3-task-13-completion/`), and two rig rules: give items AFTER any respawn (a give to a dead player is discarded by the respawn), and a window activation kicks the vanilla camera (~+18°/−4.4° per activation — re-issue the pose teleport and take captures window-free) |

## Local-only directories

`refs/` (upstream clones, rig) and `vanilla/` (Mojang client jar and asset index) are
gitignored and never pushed. Neither is ever redistributed.

## Handoff protocol

1. Update this file: stage, what changed, next actions, any new environment facts.
2. Commit and push everything, so the next person can pull it.
3. Write `docs/handoff/YYYY-MM-DD-<topic>.md` from `docs/handoff/TEMPLATE.md`, fully
   self-contained: a reader should need nothing beyond the repository.
4. State plainly in the handoff what is verified and what is only planned.
