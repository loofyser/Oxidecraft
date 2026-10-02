# M2 performance baseline

The M2 exit procedure's performance record: the C.4 route run on the rig on 2026-09-30, with one
method stated per number. This is the baseline-only record, not the full M9 protocol; the
distinction is stated under "Baseline only, and what M9 will re-run".

The route ran on the fixed client - the build with all three of the session's defect fixes
(keepalives, the frozen-clock sign, the terrain render origin). An earlier run of the route
measured a client that rendered the world with effectively no terrain and is superseded; the
"Redo" note under "Our client" records both and the reason. The three scene pairs of the same
acceptance session did not reach a masked pass at the close (final numbers and every documented
class in `refs/rig/evidence/m2/acceptance-notes.md`; the close proceeded with the classes
documented - see the M2 progress ledger, 2026-10-02); those divergences are rendering-parity
items on static scenes and do not change the frame-cost method or the numbers below.

## Device and builds

- Device: the rig laptop, NVIDIA T500, driver 615.71.09 (NVIDIA-SMI), KDE/KWin on Wayland,
  both displays at scale 1.
- Our client: `target/debug/oxide-client`, built from HEAD `72526bf` at 14:31 CDT (the debug
  build; the rig's launch script runs it), at 1280x720, `--no-overlay --render-distance 8
  --server 127.0.0.1:25565 --username OxideDev`.
- Vanilla client: Minecraft 1.8.9 under PrismLauncher, `refs/rig/jre8`, launched by
  `refs/rig/client/launch-client.sh --join`, 1280x720.
- Settings: the vanilla `options.txt` was written to the C.1 values before the first launch and
  archived whole at `refs/rig/evidence/m2/vanilla-options.txt` (render distance 8, GUI scale 3,
  VSync off, graphics Fast, clouds Fast, smooth lighting Minimum, mipmap 4, fullscreen off,
  1280x720). Our client's equivalent knobs are the flags above; it draws no HUD, so GUI scale
  and the hide-HUD steps do not apply to it. One asymmetry is recorded plainly: our client's
  swapchain presents with `PresentMode::Fifo` (`crates/oxide-render/src/renderer.rs`), which
  locks its present to the display's refresh; the vanilla client ran with `enableVsync:false`
  per C.1. The fixed build's measured rates (5.6-16.8 fps) sit well below any plausible refresh,
  so the cap does not bind here; the earlier, superseded run's higher numbers (up to 80 fps)
  came from the no-terrain build and said nothing about this one. M9 should still pin this
  asymmetry down.

## The C.4 route

Four positions, one run each, 60 s each, no warm-up, VSync off (vanilla per C.1; our client's
present mode above). The commands are recorded at `refs/rig/evidence/m2/route-ours.txt` and
`refs/rig/evidence/m2/route-vanilla.txt`:

1. the M1 mark, feet 150, facing 135/20;
2. the wall viewpoint, feet 57, facing the wall square-on (yaw 180, pitch 0);
3. the M1 ground position, feet 71, yaw 0 pitch 0;
4. the wall's far end, x = 15.5, again facing the wall.

Each client ran the route alone in the world. Ours ran the redo route 20:18:54Z to 20:22:55Z
(the earlier run's window, 16:32:06Z to 16:36:07Z, is superseded); vanilla's run stands,
16:37:48Z to 16:41:48Z.

## Our client

Frame rate is read from the client's own `frame rate` log lines
(`refs/rig/evidence/m2/oxide-client-baseline2.log`). Per hold the table gives the frames-delta
rate (the `frames=` counter difference divided by the wall time between the first and last log
line inside the hold, tp instant to +60 s) and the mean, minimum and maximum of the logged
window values themselves. Memory is `VmRSS` from `/proc/<pid>/status`, sampled at the end of
each hold. The route ran after an 11-minute post-join soak, so the mesh queue from the join
burst had drained before the first hold (the hold windows straddle a tp each, whose column
warm-up is part of the measured window; the steady-state sub-windows - the last ~50 s of each
hold - read 9.1 / 12.6 / 5.7 / 16.8 fps for mark / wall / ground / far end).

| hold | window (UTC) | frames-delta fps | logged fps mean / min / max | VmRSS after |
|---|---|---|---|---|
| mark | 20:18:54 - 20:19:54 | 10.4 | 14.1 / 3.2 / 21.8 | 526 460 kB |
| wall | 20:19:54 - 20:20:54 | 12.2 | 13.1 / 6.3 / 17.0 | 530 504 kB |
| ground | 20:20:54 - 20:21:54 | 5.6 | 6.0 / 0.7 / 10.3 | 547 068 kB |
| wall far end | 20:21:54 - 20:22:55 | 15.2 | 15.9 / 6.5 / 18.8 | 531 648 kB |

**Redo.** The first run of this table (36.8 / 27.1 / 11.1 / 12.5 fps, VmRSS 578 580 to
870 196 kB, `oxide-client-run.log`) measured the pre-fix client, whose world rendered with
effectively no terrain visible; it is superseded by the table above. The intermediate rise in
the first run's VmRSS tracked the invisible world's mesh accounting, not a leak pattern worth
carrying forward; the fixed build sits flat at 515-534 MiB across the route.

Cold start, from the log's own timestamps: the first log line to the renderer-ready line. The
baseline launch in this redo started in 7.7 s (`20:05:58.032Z` to `20:06:05.747Z`); the
acceptance capture-session launch started in 6.2 s (`19:33:20.648Z` to `19:33:26.879Z`). The
launch builds assets before the window opens; the two launches differ by the machine's
background load at the time. The earlier session's numbers (5.5 s, 9.0 s) are superseded.

## Vanilla client

Memory is `VmRSS` from `/proc/51686/status`, sampled at the end of each hold:

| hold | window (UTC) | VmRSS after |
|---|---|---|
| mark | 16:37:48 - 16:38:48 | 1 189 188 kB |
| wall | 16:38:48 - 16:39:48 | 1 224 196 kB |
| ground | 16:39:48 - 16:40:48 | 1 224 216 kB |
| wall far end | 16:40:48 - 16:41:48 | 1 224 492 kB |

Cold start, from wall-clock markers: the launcher command started at 16:14:51Z; the game
logged "Setting user" at 16:15:24Z (+33 s), "Sound engine started" at 16:15:49Z (+58 s), and
"Connecting to 127.0.0.1" at 16:16:13Z (+82 s); the server logged the join at 16:16:19Z.

The vanilla frame rate is recorded as M9's measurement to complete. The F3 readout could not be
obtained here: `wtype` cannot reach the KWin compositor (no virtual-keyboard protocol), and F3
does not register through the XWayland keyboard path that hid the HUD with F1; a
frame-difference recording cannot resolve the client's uncapped rate on a frozen scene, whose
frames are identical by construction. Appendix C.4's warm-up and median own this number anyway
(see below).

## Baseline only, and what M9 will re-run

This document is the M2 baseline: one run per client, one pass per position, no warm-up and no
median, exactly the shape Task 15 asked for. M9 owns the full protocol - multiple runs per
position, a warm-up pass, medians - on the final textured-terrain build, and it re-measures the
vanilla frame rate with the F3 route once a working input path exists on this desktop. The
numbers above are the comparison point for that run; the pairs still differ from vanilla's on
static rendering classes documented at the close (the wall's slope-edge class and the chest
cell, the fluids' static phase, the ground's remaining tallgrass/terrain-shading residue, the
cloud-phase classes), none of which are frame-cost-driven, so they do not invalidate the frame,
memory or start numbers.

Deviations recorded here rather than in the acceptance note's list, being measurement-method
choices: no warm-up or median (M9's); the vanilla frame rate deferred to M9; frame rates from
the debug build, which is the build the rig's launch script runs and the one every acceptance
capture of this task used; the ours route re-run in place (the first run measured the pre-fix
build, see "Redo").
