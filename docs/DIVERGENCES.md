# Intentional divergences from vanilla 1.8.9

Every entry here is deliberate and user-visible. Anything not listed is expected to match
vanilla exactly. Add an entry only after the project owner accepts the difference.

| # | Divergence | Reason | Effect during play |
| --- | --- | --- | --- |
| 1 | Java-specific options such as "Advanced OpenGL" keep their 1.8.9 layout but have no effect | The native renderer has no equivalent setting | None; the button stores its value and nothing changes |
| 2 | Account tokens are stored in the OS keyring instead of a launcher profile file | Security | None |
| 3 | The title screen shows "Oxidecraft 1.8.9" where vanilla shows "Minecraft 1.8.9" | Honest identification of the running program | Cosmetic; one line of text |
| 4 | The window's clear colour is visibly paler than vanilla 1.8.9's sky blue | The surface format is sRGB-aware, so the linear clear values 0.62/0.76/0.98 are encoded to sRGB before display, which lightens them | Cosmetic for now: the sky reads lighter than vanilla. The real renderer must match vanilla's colour pipeline in a later milestone |

Notes:

- Title-screen artwork comes from the user's own client jar at runtime, the same way vanilla
  loads it. Nothing is bundled or redistributed.
- Nothing in this file relaxes the parity criteria in the specification. World rendering,
  GUI layout, controls, physics, and protocol behaviour must still match.
- Entry 4, recorded 2026-09-23 from the window run's capture (`refs/rig/evidence/m0-window.png`):
  the sRGB surface leaves the clear colour visibly paler than the vanilla 1.8.9 sky captured at
  `refs/rig/evidence/minecraft-1.8.9-title-screen.png`. Accepted for this milestone; the sky
  renderer revisits the colour pipeline.
