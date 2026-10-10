# Intentional divergences from vanilla 1.8.9

Every entry here is deliberate and user-visible. Anything not listed is expected to match
vanilla exactly. Add an entry only after the project owner accepts the difference.

| # | Divergence | Reason | Effect during play |
| --- | --- | --- | --- |
| 1 | Java-specific options such as "Advanced OpenGL" keep their 1.8.9 layout but have no effect | The native renderer has no equivalent setting | None; the button stores its value and nothing changes |
| 2 | Account tokens are stored in the OS keyring instead of a launcher profile file | Security | None |
| 3 | The title screen shows "Oxidecraft 1.8.9" where vanilla shows "Minecraft 1.8.9" | Honest identification of the running program | Cosmetic; one line of text |
| 4 | Book-and-quill editing is post-v1; opening an editable book shows its stored pages read-only | The signing view and the edit sends are not built yet | An editable book opens the same reader with no editing buttons, and closing it sends nothing |

Notes:

- Title-screen artwork comes from the user's own client jar at runtime, the same way vanilla
  loads it. Nothing is bundled or redistributed.
- Nothing in this file relaxes the parity criteria in the specification. World rendering,
  GUI layout, controls, physics, and protocol behaviour must still match.
- Entry 4, recorded 2026-09-23 from the window run's capture (`refs/rig/evidence/m0-window.png`):
  the sRGB-aware surface left the clear colour visibly paler than the vanilla 1.8.9 sky captured
  at `refs/rig/evidence/minecraft-1.8.9-title-screen.png`. Retired on 2026-09-28: the M2
  pipeline matches vanilla's non-sRGB output — the surface is configured with a format that is
  not sRGB, so the client's own clear values and texture bytes reach the window unconverted —
  and the reference-gradient check `the_atlas_texels_come_back_lit_by_the_lightmap`
  (`crates/oxide-render/tests/pipeline_headless.rs`) reads the frame back against the client's
  own lightmap arithmetic byte for byte.
