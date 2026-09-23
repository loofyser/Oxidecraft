# Oxidecraft v1 design specification — independent review

Conducted as an independent adversarial review, 2026-09-22.
Inputs: `docs/specs/oxidecraft-v1-design.md`, `docs/STATE.md`, `docs/DIVERGENCES.md`,
`docs/handoff/TEMPLATE.md`, and the five reports in `docs/research/`.
Method: every claim below was checked against the research reports by direct search; the spec was
not edited. Line references are to the files as of this review date.

---

## Verdict

**The specification is not ready for implementation planning as written, but it is close and the
defects are cheap to fix.** The research base it leans on is genuinely verified and the numbers
mostly reconcile exactly (jar hash and size, 734 objects, 74/26 packets, 4.317/5.612 m/s, cloud
height 128, 40 particle types, 57 parity tests). However, four of the specification's own technical
statements contradict the verified reports in ways that would send a developer down the wrong
path — the chunk is described as 128 blocks / eight sections instead of 256 / sixteen, entity
interpolation is described as a "3-tick scheme" with three stored positions where the report says
one-tick interpolation from two, the wire byte order of chunk data is stated as "exact" without the
array-major grouping, nibble order, or dimension rule that make it exact, and `BlockRegistry` is
described as being built from jar JSON that does not contain any of the properties listed. On top
of that, M0's dependency list cannot build the launcher it describes (no JSON, no TOML, no NBT
anywhere in the spec), and the acceptance criteria that matter most — P1, P4, R1–R3, S1 — have no
capture procedure, no tolerance, and no pass condition, so two competent developers could not
agree on whether any of them is met. None of these require redesign: they are a day or two of
specification surgery. Do that work, then plan M0.

---

## Cross-check results (the checks requested of this review)

| # | Claim checked | Result |
|---|---|---|
| 1 | Spec §8 chunk encoding vs protocol report §3 | **Incomplete/ambiguous** — see finding 3. The one hard constant in §9 is wrong — finding 1. |
| 2 | Compression threshold 256 | **Partly right** — the report says "Default threshold is **256 bytes** (`network-compression-threshold` in `server.properties`)" (protocol-47-reference.md:53); the value is server-supplied and the rule is measured on *uncompressed packet id + data*, not "frames". Finding 21. |
| 3 | RSA key size and AES mode | **Match** — report: RSA 1024-bit, AES/CFB8, IV = shared secret (protocol:128-135). Missing: generic DER key sizing, 8-bit segment size, two continuous cipher contexts (launcher-assets-auth-survey.md:709-739). Finding 22. |
| 4 | Asset index object count 734 | **Match** — "object count: 734" (launcher-assets-auth-survey.md:185). |
| 5 | Textures/models/blockstates/fonts live in the jar, not the index | **Match** — "for 1.8.9 the asset index contains **no** textures, **no** models, **no** blockstates, **no** shaders and **no** font files" (launcher-assets-auth-survey.md:214-216). |
| 6 | Client jar size and SHA-1 prefix | **Match** — 8 461 484 B, `3870888a6c3d349d3771a3e9d16c9bf5e076b908` (render-parity-survey.md:11; launcher-assets-auth-survey.md:20). |
| 7 | Packet counts 74 / 26 | **Match** — "74 clientbound ids (0x00–0x49), 26 serverbound ids (0x00–0x19)" (protocol:162). |
| 8 | Physics numbers | **Match, provenance caveat** — walk 4.317 / sprint 5.612 / jump 1.2522 all present (render:481, 475), but the report derives them ("my simulation of the recurrence gives 1.25220") and labels the wiki value "post-15w45a", a 1.9-dev snapshot. Finding 23. |
| 9 | Cloud height 128 | **Match** — `getCloudHeight()` = 128.0F, drawn at `cloudHeight − cameraY + 0.33` (render:336). The +0.33 is dropped in the spec. Finding 23. |
| 10 | 40 particle types | **Match** — ids 0–39 enumerated, and test 47 says "All 40 `EnumParticleTypes`" (render:398, 560). |
| 11 | FOV and mouse-sensitivity formulas | **Match by reference** — formulas exist at render:411-429; spec §10 only points at them and its summary omits the bow-draw and death modifiers. Finding 25. |
| 12 | 57 screenshot tests | **Match on count, wrong on kind** — render §7 has exactly 57 items, but several are behavioural (movement #50, interpolation #46, font widths #54) or audio (#52), not screenshots. Finding 25. |
| 13 | 1.8 → 1.8.9 share protocol 47 with no packet-id differences | **True in the report, absent from the spec** — "There are no packet-id or field differences between 1.8.0, 1.8.8 and 1.8.9" (protocol:149). The spec never states this anywhere, although it is the justification for targeting 1.8.9 and reusing the `1.8` asset index. Finding 25. |

---

## Findings

### 1. The chunk dimensions are wrong — BLOCKER

**Spec:** `docs/specs/oxidecraft-v1-design.md:248` — "A chunk is 16x16x128 blocks: eight
16x16x16 sections, matching the wire format."

**Reports:** protocol-47-reference.md:525 — Anvil `Sections`: "up to 16 sections; empty ones are
omitted". protocol:388 — vanilla's bulk path calls `getExtractedData(chunk, true, isOverworld,
65535)`, i.e. a 16-bit mask with every bit set. protocol:352 — `Size = 8192*N + 2048*N + (sky ?
2048*N : 0) + (groundUp ? 256 : 0)` where `N = popcount(mask)` and the mask is an UnsignedShort.
rustcraft-survey.md:276 — the surveyed 1.8.9 client uses `CHUNK_HEIGHT = 256`,
`SECTION_COUNT = CHUNK_HEIGHT / SECTION_SIZE`.

**Why it matters:** 1.8.9 chunks are 256 blocks tall in 16 sections; the spec's 128/eight is the
pre-Beta-1.3 shape and is simply wrong for protocol 47. A developer following §9 will size
storage, masks, light arrays and height maps for half a world and will silently drop mask bits
8–15, which is where terrain above y=128 lives. The spec's own §10 puts the cloud layer at y=128,
so the error is self-evident: the world would end at the clouds.

**Fix:** change to "16×16×256 blocks: sixteen 16×16×16 sections", add the PrimaryBitMask ↔ section
mapping ("bit i set ⇒ section i, y = 16i..16i+15") and cite protocol §3.1. Say explicitly that the
mask is 16 bits wide and that y=0..255 is the storage range.

---

### 2. Entity interpolation is described wrongly — BLOCKER

**Spec:** line 71 — "| P5 | Tick parity: simulation runs at 20 ticks per second, with entity
interpolation identical to vanilla's 3-tick scheme |". Line 252 — "Entities store the last three
server positions for vanilla interpolation".

**Report:** render-parity-survey.md:486 — "Vanilla 1.8 interpolates over **one tick**, not 3 …
the renderer interpolates with `partialTicks ∈ [0,1)` between `lastTickPos` and the current
target. The optional 3-tick / 100 ms entity-position smoothing that players remember is the
**server-side 3-tick movement packet cadence** … So a parity renderer wants: **store the last two
positions per entity, lerp by `partialTicks`, snap on teleport (> 4 blocks)**, and use 20 Hz for
all tick-driven animation."

**Why it matters:** the report calls this out as a known misconception and gives the correct
recipe (two positions, `partialTicks`, 4-block snap). The spec states the misconception as the
acceptance criterion (P5) and as the storage design (§9). Built as written, every entity will
visibly lag and jitter, and the criterion itself would be verified against the wrong behaviour.

**Fix:** rewrite P5 as "interpolate between the two most recent server positions by `partialTicks`
at 20 Hz, snapping on teleports > 4 blocks; the 3-tick cadence is the server's movement-packet
rate, not a client smoothing window" and change §9 to two stored positions. Add the >4-block snap
to §9.

---

### 3. The chunk-data layout claimed to be "exact" is missing the three things that make it exact — BLOCKER

**Spec:** line 238 — "Chunk encoding is exact: 4096 `u16` little-endian block values `(id << 4) |
meta`, then block-light nibbles, then sky-light nibbles, then a 256-byte biome array."

**Report:** protocol:345-358 — the data is grouped **array-major, not section-major**: "Block data,
**all included sections, ascending Y** … then Block light, all included sections … then Sky light,
all included sections … then Biomes". protocol:349 — sky light "present **only if the dimension has
sky** (`!provider.getHasNoSky()` ⇒ Overworld). Never sent for Nether/End". protocol:356-357 —
index order inside an array "**X varies fastest, then Z, then Y**", `(y << 8) | (z << 4) | x`.
protocol:414 — nibble order "**even index = low nibble**, odd = high nibble" (the report flags this
as one of "the classic 1.8 implementation traps" and says wiki.vg's prose claims the opposite).

**Why it matters:** read as written, a naive implementation stores section 0's blocks, then section
0's light, then section 0's sky light, then moves to section 1 — which desynchronises the stream on
the first multi-section chunk (i.e. every real chunk). The nibble order is the single most
commonly-got-wrong detail in the format and the report devotes a table to it; the dimension rule
decides whether the parser reads 12288·N or 10240·N bytes per column. "Exact" is doing work this
sentence has not earned.

**Fix:** restate as the report does — all masked sections' block arrays (ascending Y), then all
block-light arrays, then all sky-light arrays *when the dimension has sky*, then 256 biome bytes
when ground-up — plus the size formula, the `(y<<8)|(z<<4)|x` index order, the even=low nibble rule,
and the Nether/End no-sky-light rule. Add a fixture test for the worked example in protocol §8
(mask 0x0001 → 12544 bytes).

---

### 4. `BlockRegistry` is said to be built from jar JSON that does not contain the data listed — BLOCKER

**Spec:** line 250 — "A separate `BlockRegistry` maps ids and metadata to block properties: name,
solidity, opacity, light emission, tinting kind, render pass, and the model reference. It is built
from the jar's blockstates and models JSON, so a resource change does not need code changes."

**Report:** render-parity-survey.md:51-135 — blockstate JSON carries `variants` (`model`, `x`, `y`,
`uvlock`, `weight`) and model JSON carries `parent`, `textures`, `elements`, `faces`, `display`.
There is no solidity, opacity, light emission, hardness, material or render-pass field anywhere in
that format. Those live in vanilla's block classes; the reports treat them as source-derived facts
(light emission "Torch 14, Glowstone 15" and opacity "leaves 1, water/ice 3, solids 15" are listed
from a re-derived table in rustcraft-survey.md:290-295; the light-filtering rule is in
protocol:445).

**Why it matters:** this is the design's only stated source for block behaviour. Following it, M2
cannot render opacity-correct terrain, light emission or tinting, and M3/M5 cannot compute break
timing ("tool timing per block hardness", line 289) or tooltips. The "resource change does not need
code changes" rationale is only true for the model/texture half of the registry. It also collides
with the crate graph (see finding 20): `oxide-world` may not depend on `oxide-assets`, so it cannot
read the JSON the sentence points at.

**Fix:** split the registry explicitly: (a) a JSON-driven model/blockstate table owned by
`oxide-assets` (variants, elements, cullface, uvlock, tintindex, display) and baked into
`BakedModel`s; (b) a code-defined behaviour table (hardness, light emission, opacity/filtering,
material, render pass, tinting kind) sourced from documented vanilla values and cross-checked
against the decompiled 1.8.9 source, as the reports did. State which crate owns (b) and how it
crosses the allowed dependency edges.

---

### 5. Appendix A cannot build the launcher it describes — BLOCKER

**Spec:** line 117 — "The complete set of allowed dependencies"; Appendix A (lines 356-378) lists
winit, wgpu, glam, flate2, aes/cfb8/rsa/x509-cert/sha1, rand, zip, png, lewton, cpal, rayon,
crossbeam-channel/arc-swap, ureq+rustls, clap, tracing/tracing-subscriber, keyring, criterion.

**What is missing to execute §7.2's five-step fetch flow:**
- **No JSON parser.** The fetch flow parses `version_manifest_v2.json`, the 1.8.9 version JSON, the
  `1.8` asset index (734 entries), and later blockstates/models/`sounds.json`. Without
  `serde`/`serde_json` (or an explicit hand-rolled parser decision), step 1 cannot start.
- **No TOML crate**, yet line 185 is `options.toml` and §11/M6 require an options screen backed by
  it. No JSON writer either, yet `profiles.json` is planned.
- **No NBT anywhere.** The string "NBT" does not occur in the specification (verified by search),
  yet protocol 47 requires it for the Slot codec (protocol:45), Update Block Entity (0x35),
  Update Entity NBT (0x49), and item NBT for M5 tooltips.
- **No XDG/dirs crate**, while line 178 hardcodes `~/.local/share/oxidecraft/` and §16 (line 351)
  requires the code to "compile and run cross-platform from M0".
- **No version-pinning policy and no lockfile policy.** Appendix A is unversioned; wgpu/winit/zip/
  ureq have all had breaking major bumps recently; nothing says whether `Cargo.lock` is committed,
  what the MSRV is, or how updates are taken. The one pinning statement is the flate2/zlib-ng
  toolchain note (line 363), which is good but lonely.

**Fix:** extend Appendix A with the JSON, TOML and NBT libraries (or state that NBT and JSON are
hand-written and budget for it), add a `dirs`-style crate or a stated path policy, and add a short
"dependency policy" paragraph: pinned in `Cargo.lock`, committed, MSRV declared, updates only via
an explicit PR. State the extraction-manifest format here too (see finding 6).

---

### 6. M0's store, extraction manifest and CI are under-specified — MAJOR

**Spec:** line 200 — "Extract jar resources once into `extracted/1.8.9/`, recording a manifest so
later runs skip work." Line 316 — M0 exit: "CI green; `oxide-launcher fetch` produces a fully
verified store; window opens with an FPS counter."

**Why it matters:** a developer must invent: the manifest's filename, format and contents (what
invalidates it — jar SHA-1 plus extractor version, at minimum); whether extraction is atomic
(temp dir + rename) and how an interrupted fetch is detected; whether `XDG_DATA_HOME` is honoured
or the path is literally `~/.local/share` (and what happens on Windows/macOS, which §16 says must
work from M0); whether concurrent launcher runs are locked out; how downloads resume or retry
(there is no retry/backoff policy for Mojang's CDN anywhere); and what the "fully verified store"
check actually runs. The vanilla-install reuse path (line 202) additionally depends on the
`.mcassetsroot` convention that the survey explicitly lists as unverified
(launcher-assets-auth-survey.md:808-811, 1043).

**Fix:** add a short "store and fetch semantics" subsection: exact manifest file (`extracted/
1.8.9/.manifest.json` with jar sha1 + extractor schema version), atomic write-and-rename for every
object, XDG resolution order, a single-instance lock, retry/backoff (3 tries, exponential), and a
disk-space precheck (~150 MB). Mark the `.mcassetsroot` dependency as "verify before use".

---

### 7. The CI list is under-specified and omits the one check that protects the license posture — MAJOR

**Spec:** lines 307-308 — "Continuous integration: formatting, clippy with warnings denied, tests,
release build, and a license check that fails on any dependency incompatible with GPL-3.0." Line
135 — "A CI check asserts the graph with `cargo tree` and fails the build on any unlisted edge."

**Why it matters:** no provider, no config files, no target list, although §16 promises Windows and
macOS "verified by CI cross-builds" from M0 and the license check needs an allow-list to be
meaningful (which licenses are acceptable, and cargo-deny config committed under `deny.toml`).
More seriously, nothing in CI prevents the one unrecoverable mistake this project can make: a
Mojang asset entering git history. The survey is explicit that a commit requires a history
rewrite, not a delete (launcher-assets-auth-survey.md:906-907).

**Fix:** add a CI job that fails if any tracked file is a `.jar`, `.ogg`, `.png` under an
`assets/`-shaped path, the asset index JSON, `enums`/`lang`/`glyph_sizes.bin`, or anything under
`refs/`/`vanilla/`; commit `deny.toml` with the GPL-3.0-compatible allow-list; name the CI provider
and the cross-build targets in §12.

---

### 8. The light engine as described contradicts the report — MAJOR

**Spec:** line 251 — "`LightEngine` implements vanilla propagation: 15 levels, decrement by one per
step, BFS with incremental updates for block changes."

**Report:** protocol:442-447 — "full-strength (15) sky light propagating **downward** through a
transparent block does **not** decrease; propagating **horizontally or upward** (and any sky light
< 15 spreading to neighbours) **decreases by 1**; opaque blocks block propagation; 'light-filtering'
blocks (water, ice, leaves, cobwebs, …) reduce sky light by exactly 1 in Java Edition". The same
report notes that sections outside the mask are assumed block-light 0 / sky-light 15 by the vanilla
client (protocol:464) and that local recomputation is required for 0x22/0x23 (protocol:466-468).

**Why it matters:** "decrement by one per step" applied uniformly produces the classic wrong result
— a dark column under overhangs and under the open sky, and wrong water/leaf filtering — which is
directly visible in the M2 screenshot criterion. It also leaves out the "never trust absent light
data" rule and the light updates that must be applied for block changes.

**Fix:** state all three sky-light rules, the filtering blocks (with the exact list from the
report), the four-bit ranges, and the "keep previous light for sections not in the mask" rule, plus
which packets trigger local recomputation.

---

### 9. Lava fog is described with the wrong GL fog mode — MAJOR

**Spec:** line 278 — "linear default from 0.75 of the far plane, exponential water,
exponential-squared lava."

**Report:** render-parity-survey.md:349-350 — Water: "exponential (`GL_EXP`, 0x0800 = 2048);
density `0.1 − respiration*0.03`; **0.01** with Water Breathing"; Lava: "camera inside lava |
exponential, density **2.0**". Test #32 repeats "lava (2.0)" (render:545).

**Why it matters:** EXP and EXP-squared fog look completely different; being inside lava in the
wrong mode produces an obviously wrong screen, and test #32 cannot pass. The spec's own P2-style
fidelity claim rides on getting these modes right.

**Fix:** "exponential water (density 0.1, or 0.01 with Water Breathing) and exponential lava
(density 2.0)", and note the sky-pass linear case (start 0, end = farPlane) alongside the default
0.75·farPlane start.

---

### 10. The screen list omits containers and screens that the parity checklist itself requires — MAJOR

**Spec:** line 291 — "Screens for v1: main menu, server list with ping, pause, options (the 1.8
option set), death, disconnect, chat, inventory, crafting table, chest, furnace, anvil, enchantment
table, and the HUD."

**Report:** render §3.1 inventories **35** screens (render:183-221), and the acceptance tests name
several that the spec omits. Test #17 (render:530): "Every container opens the 1.8 texture from
`gui/container/*` at the correct size (**anvil, beacon, brewing stand, dispenser, enchantment,
furnace, generic_54, hopper, horse, inventory, villager, crafting_table**)." Also absent from the
spec: dispenser/dropper, hopper, beacon, villager trading, horse inventory, command block (all
server-openable), sign editing (0x33/0x36 + S 0x12), book & quill, win/credits, the multiplayer
sleep screen, the spectator screen, statistics/achievements (which are *buttons in the 1.8 pause
menu*, render:198), skin customization, and the resource-pack screen/button (render:191, 196).

**Why it matters:** the stated goal is "a client that a player cannot distinguish from the official
client", and the verification plan is the 57-test checklist; that checklist cannot pass with these
screens missing. A server can open a hopper or a villager trade at any moment, and the pause menu
cannot match vanilla without Statistics/Achievements entries.

**Fix:** either extend §11/M5/M6 to the full multiplayer-reachable container set (brewing,
dispenser, hopper, beacon, villager, horse, command block, sign, book) plus statistics/achievements
and skin customization, or move them explicitly to a post-v1 list **and** amend the checklist
accordingly with a DIVERGENCES entry. Do not leave the checklist and the scope list contradictory.

---

### 11. HUD, entity and audio coverage gaps against the checklist — MAJOR

**Spec:** F9 (line 57) lists "hotbar, health, hunger, armor, experience, air, crosshair, damage
overlay"; F5 (line 53) is "See other players and mobs"; line 292 is "Sound: Ogg Vorbis playback
with vanilla categories, pitch randomisation, and 3D attenuation"; line 290 mentions "mouse
capture".

**Missing, each with a named checklist item or report section:**
- **Boss health bar** — test #13 (render:526), geometry in render:240.
- **Nametags** — nowhere in the spec; the report carries name + visibility as entity metadata
  (protocol:587-588) and the only Rust-side mention of nametag rendering is in another project's
  survey (rustcraft-survey.md:247). Neither report's checklist tests it, so this is a gap in both
  the spec and the checklist.
- **Object entities** — arrows, thrown items, dropped items, boats, minecarts, item frames,
  paintings, XP orbs (protocol:632-650) are marked `need` in the report's MVP column but appear in
  neither F5 nor M4 ("mob box models").
- **Third-person camera (F5) and HUD hide (F1)** — test #51 (render:564); absent from the spec.
- **Chat wrapping / scale / 20-line scrollback** — report render:238; F6 covers codes and click
  events but not layout.
- **Music ticker and streaming** — `music.game/creative/menu`, records streamed, 64-block range,
  10–20 min pick with 100-tick fade (render:503-504); test #52 (render:565). The spec's sound line
  covers effects only.
- **Screenshot hotkey (F2)** and **F3 sub-modes** (F3+B/G/H/P/A/T/lagometer, render:288) — test #6
  (render:519) names them; the spec says only "F3 debug output with the vanilla field set".
- **Window resize / fullscreen / GUI-scale-at-resolution** — test #53 (render:566) tests three
  resolutions; the spec never mentions surface reconfiguration, minimise handling or fullscreen.
- **"Downloading terrain" screen** — protocol:310; not in the screen list.

**Fix:** add these to F9/F5/F10/F11 and to the M4/M5/M6 contents, and add a nametag item to the
checklist (it is currently untestable). Each is small; the risk is that they are discovered during
M9 hardening, when they are no longer small.

---

### 12. Several acceptance criteria and milestone exits are not objectively testable — MAJOR

**Spec:** line 69 — "| P3 | GUI parity: **every implemented screen** has the same layout,
textures, and widget behavior as 1.8.9 |". Line 318 — M2 exit: "Screenshot shows the same terrain
as vanilla at the same position". Line 319 — M3 exit: "Walk the world and it feels like vanilla".
Line 324 — M8 exit: "A fresh machine installs and plays from the published package". Line 325 — M9
exit: "…soak clean, documentation complete".

**Why it matters:**
- **P3 is unfalsifiable**: it quantifies over "implemented" screens, so a one-screen client
  satisfies it. It cannot fail, so it cannot be verified.
- **M3's exit is a feeling**; §12 promises physics tests "against recorded vanilla values" but no
  milestone owns producing those recordings.
- **M8's "fresh machine"** is undefined (whose machine? same hardware class? clean profile?).
- **M9's "documentation complete"** has no list.
- **M2's exit requires a specific position and time of day** but movement arrives only in M3, so M2
  must place both clients by server command; that procedure is not stated anywhere.
- **F12's "F3"** (line 60) has no milestone exit criterion at all — M1 only promises "an F3-style
  overlay".

**Fix:** define P3 over a named screen list (the M5/M6 contents); move M3's exit onto measured
values (apex 1.2522, walk 4.317, sprint 5.612, sneak 1.3, terminal 3.92, test-vector physics);
state the M2 rig procedure (`/time set`, `/tp` both clients, weather cleared, HUD hidden, fixed
settings); define "fresh machine" and "documentation complete" by lists; add "F3 field set matches
render-parity-survey.md §3.3" as an M6 exit.

---

### 13. The one-tick render-snapshot lag is asserted to "mirror vanilla" without support — MAJOR

**Spec:** line 169 — "The render snapshot lags the tick state by at most one tick, which mirrors
vanilla's own decoupling."

**Report:** render:389 — vanilla renders from the live world with `partialTicks` interpolation
between `lastTickPos` and the current target; the report describes no lagged snapshot. The report
does state that block updates arrive via packets at 20 Hz and that the client must recompute light
locally (protocol:466-468).

**Why it matters:** the sentence converts an implementation convenience (double-buffered world
snapshots for the render thread) into a fidelity claim. A full-tick lag on world state means block
placements and breaks appear up to 50 ms late and interactive feedback (block outline, crack
overlay) lives one tick behind the simulation — precisely the kind of "feel" difference the project
exists to avoid. It may be fine, but it is not evidenced.

**Fix:** either state it as a deliberate design choice with a latency budget and a test (measure
input-to-visible-block-change), or restrict the lag to *meshing* (meshes built from snapshots) while
the render thread reads current block data for outlines/overlays. Do not claim it mirrors vanilla.

---

### 14. Divergence governance does not match its own rules — MAJOR

**Spec:** line 72 — "Divergences are allowed only when listed **in this document** as intentional,
and each is user-visible in `docs/DIVERGENCES.md`." `docs/DIVERGENCES.md` states: "Anything not
listed is expected to match vanilla exactly."

**Problems:**
- `DIVERGENCES.md` entry 3 (title screen shows "Oxidecraft 1.8.9") is **not mentioned in the
  spec**, so it fails P6's "listed in this document" test. It also has an unresolved knock-on: the
  F3 first line is specified as `Minecraft 1.8.9 (1.8.9/vanilla)` by test #5 (render:518) — which
  string does Oxidecraft print there?
- `options.toml` (line 185) is a divergence from vanilla's `options.txt` and is not listed — if the
  intent is that users can copy options across, this matters; if not, it still belongs in `docs/DIVERGENCES.md`.
- `MC|Brand` is never decided. Vanilla sends `vanilla` (protocol:153); a server-visible brand is a
  behavioural divergence whichever value is chosen, and plugin channels are used by plugins.
- M9's "32-chunk render distance" (line 325) is not a 1.8 option value (see "claims I could not
  verify") and, if user-facing, contradicts both the 1.8 video-settings set (test #4, render:517)
  and `docs/DIVERGENCES.md`.

**Fix:** add the title text and options format to the spec as intentional decisions with their
rationale, add a `MC|Brand` decision (recommend `vanilla` for parity, with an entry in
`docs/DIVERGENCES.md` explaining why), and either drop the 32-chunk setting from v1 or record it in
`docs/DIVERGENCES.md` as a non-vanilla extra with the settings-screen consequence spelled out.

---

### 15. Online-mode session join, the Minecraft hexdigest and skin fetching have no architectural home — MAJOR

**Spec:** line 221 places "Session join against `sessionserver.mojang.com`" in §7.4 (launcher
auth). Line 332 says "Auth work is isolated in `oxide-launcher` and `oxide-assets`". Line 120 says
`oxide-assets` may be depended on by the launcher and renderer, and lists no auth responsibility.

**Report:** launcher-assets-auth-survey.md:602 — "This is the part a *native* client must implement
itself", i.e. the **client** posts `/session/minecraft/join` before sending Encryption Response
(§3.8, line 743-746). The same survey documents the non-standard "Minecraft hexdigest" (leading
`-` on roughly half of digests) with three golden test vectors, and states they "belong in the Rust
test suite" (line 622-631). Skins for other players come from `textures.minecraft.net/texture/<hash>`
via the profile property (render:387) — an HTTP fetch that no crate responsibility mentions.

**Why it matters:** M7 cannot be planned without deciding which crate owns the HTTP session handshake
and whether it can run it before/inside the login state machine; a plain `hex::encode` will produce
a hash the session server rejects with what looks like an auth failure; and "skins" in M4 currently
has no data path for online-mode players.

**Fix:** state that the client (in `oxide-client`/`oxide-game`) performs the session join, that
`oxide-proto`'s login state accepts a pre-computed hash injection point, and that the hash function
is a named, unit-tested helper with the three golden vectors. Add a line to §5.3 for skin texture
fetching and caching.

---

### 16. The risk table omits the legal and naming risks the survey says are live — MAJOR

**Spec:** §14 (lines 329-336) has six rows: license contamination, Microsoft API changes,
anti-cheat, scope creep, fidelity drift, Mojang asset terms.

**Report:** launcher-assets-auth-survey.md §5 — the Usage Guidelines require the disclaimer "NOT AN
OFFICIAL MINECRAFT …" to be "Prominently include[d] … on your product, listing, description,
website/webpage, and all other related materials" (line 946-947); open-sourcing moves the project
into the guidelines' "commercial" bucket (line 879-882); a neutral name with "Minecraft" only in the
description is the safe pattern (line 960-965); "adding an offline/cracked mode is the single change
that would flip" the legal reading to the wrong side (line 991); and the spec should state "no class
file is ever read, as a design invariant, not an accident" (line 1031).

**Why it matters:** none of these are mentioned. The README disclaimer is a one-line requirement
with real teeth; the v1 build *is* offline-mode-first (line 214: "Offline mode first (v1)"), which
is exactly the axis the survey flags — that is a defensible testing choice, but the spec should say
so explicitly (offline path for rig/development; the shipped flow requires a genuine login) rather
than leave it silent. The "never read a `.class`" invariant is the project's strongest legal
position and is currently an unstated implementation accident that a curious contributor could
break by "helpfully" parsing the jar's version info.

**Fix:** add risk rows and a short "legal posture" paragraph in §1 or §16 covering: the required
README disclaimer text, the naming constraint, the offline-mode policy, and the invariants (no
`.class` read, no asset committed, assets fetched at runtime only). Add the README disclaimer to
M0's exit criteria — it is free.

---

### 17. The Microsoft auth prerequisite is deferred without lead time or risk tracking — MAJOR

**Spec:** line 227 — "M7 requires an Azure application id for the device code flow. The project
owner registers a free application, or explicitly approves an alternative." §16 (line 350) repeats
this as a deferred item resolved "At M7".

**Report:** launcher-assets-auth-survey.md:408-409 — "**app registration + Minecraft API permission
request is a prerequisite, not a code task**"; the legacy public client ID probe failed
(AADSTS700016, line 394); without the Minecraft API permission `api.minecraftservices.com` returns
403 (line 405-406); and custom apps trigger the child-account restriction 2148916238 that the
official client id dodges (line 512-513). The permission grant is also listed as unverified
(line 1043-1045).

**Why it matters:** M7's exit criterion is "Join an online-mode server with a real account", which
cannot even be *tested* until the registration and the API permission exist. Deferring the request
to M7 puts an external, multi-week, non-code dependency on the critical path of the milestone that
needs it, and there is no fallback if the permission is refused.

**Fix:** make the Azure registration a tracked item that starts before M6 (owner action, with the
form URL), add a risk row "Minecraft API permission refused" with the fallback (documented
alternative / online-mode out of v1), and note the child-account limitation in the M7 scope.

---

### 18. The parity rig's own risks are absent, and it is the load-bearing instrument — MAJOR

**Spec:** line 110 (D9) — "Local offline-mode 1.8.9 server plus a vanilla client for parity
screenshots". `docs/STATE.md` — CachyOS, kernel 7.2.2, **Wayland with niri**; Vulkan 1.4.357;
**Intel Iris Xe (card2) and NVIDIA T500 (card1)**; "Java 26 system-wide; the rig uses a standalone
JRE 8 tarball".

**Report:** launcher-assets-auth-survey.md:141-151 — vanilla 1.8.9 needs LWJGL **2.9.4** plus
platform natives; on a modern Wayland compositor that means XWayland and an old GL stack. The
survey also lists `libraries.minecraft.net` as a required source.

**Why it matters:** P1, P2, P3, P4, R1 and R2 all depend on this one rig. Nothing in the spec
addresses: whether the 1.8.9 client will start at all under niri/XWayland with a standalone JRE 8;
whether screenshots are comparable between vanilla's **OpenGL** output and Oxidecraft's **Vulkan**
output (blending, mipmap generation, anisotropic filtering and, crucially, colour space/sRGB);
which GPU the comparison runs on (`DRI_PRIME`/Vulkan device selection is unaddressed despite two
GPUs); and how two clients are placed at the same position/time/settings. A rig that runs only
under XWayland at 30 fps also corrupts R1's baseline.

**Fix:** add a rig-risk row and a short "measurement environment" appendix: XWayland vs native, the
exact GPU and driver, the vanilla settings file used, screenshot resolution/GUI scale, and a gamma/
colour-space policy (state how Oxidecraft's surface format and lightmap scaling reproduce vanilla's
non-sRGB pipeline). Prove the rig works before M2 depends on it — ideally in M0.

---

### 19. Respawn and dimension changes are unaddressed — MAJOR

**Spec:** no occurrence of "respawn" anywhere in the specification (verified by search); M1-M3
cover login, chunks, movement.

**Report:** protocol:201-202 — on Update Health with health ≤ 0 the client shows the death screen;
on Respawn (0x07) "client must clear world + reply 0x16 Client Status respawn"; protocol:349 —
sky light is sent only for dimensions that have sky, so dimension changes flip the parser's byte
count for every subsequent chunk; protocol:319 — chunk unload semantics.

**Why it matters:** dying is the single most common multiplayer event. Without an explicit
world-reset path, stale entities, stale chunks and a wrong sky-light expectation survive the
respawn; and the F4 break/place pipeline would be reconciling against a world that no longer
exists. The F12 death screen exists in the spec, but nothing connects it to state.

**Fix:** add to M3 (or M4) an explicit "death and respawn" bullet: on health 0 show death screen,
send Client Status 0, on Respawn clear entities/chunks/inventory state, re-derive the sky-light
flag from the dimension, and answer Player Position And Look. Add a test in §12's world layer.

---

### 20. Crate-graph edges do not support what §5.3 and §16 claim — MAJOR

**Spec:** line 123 — "`oxide-world` | `oxide-proto`, `oxide-proto-v47`"; line 135 — "Only the edges
in the table above exist. A CI check asserts the graph." Line 353 — "Other Minecraft versions |
One protocol crate per version; **the renderer and world layers must not change**".

**Problems:**
- §9's `BlockRegistry` needs the jar's blockstates/models (finding 4), which are parsed in
  `oxide-assets` — an edge `oxide-world` is not allowed to have.
- §16 promises the world layer survives a new Minecraft version, but `oxide-world` depends on
  `oxide-proto-v47` (protocol-47 chunk layout baked into its storage: "Storage matches the wire
  format"), and the allowed-edge table is CI-enforced. Adding `oxide-proto-v2126` requires editing
  the table and the world layer — the opposite of "must not change".
- §14 line 332 says auth is isolated in `oxide-launcher` **and `oxide-assets`**, but §5.3's
  `oxide-assets` responsibility list contains no auth, and finding 15 shows the client must do part
  of it anyway.

**Fix:** make explicit which crate owns the JSON-driven block tables and how `oxide-world` sees
them (e.g. `oxide-render`/`oxide-game` join the two, with `oxide-world` holding only ids and
metadata); either soften the §16 claim ("world storage is version-parameterised; each protocol
crate supplies its own chunk codec") or accept that the world layer changes per version; and
correct the auth-ownership sentence to name the real crates.

---

### 21. Protocol-layer details are compressed into single misleading sentences — MINOR

**Spec:** line 235 — "Compression enabled by the server's Set Compression (threshold 256); frames
at or above the threshold are zlib-compressed, smaller frames carry a zero-length uncompressed
marker."

**Report:** protocol:53 — "Default threshold is **256 bytes**"; protocol:58-61 — the rule is
measured on the **uncompressed `Packet ID + Data` size**, not a "frame", and threshold `-1`
disables compression entirely; protocol:62-64 — Set Compression and Login Success may arrive in
either order and the Play-state 0x46 variant is broken and must not be used.

**Why it matters:** two developers read "threshold 256" differently — one hardcodes 256, one uses
the VarInt the server sends. On a server configured with `-1` or `1024`, the hardcoded client
breaks; measuring the *frame* instead of the payload mis-sends packets right at the boundary.

**Fix:** "read the threshold from Set Compression (login 0x03); vanilla's default is 256; the rule
is ≥ threshold ⇒ zlib, < threshold ⇒ Data Length 0; `-1` disables compression. Never use the broken
play-state 0x46."

The same section also omits four connection obligations the report marks as required to stay
online, none of which appears in any milestone:

- **Client Settings (0x15) payload values are unspecified.** The report defines Locale (≤7),
  ViewDistance, ChatMode, ChatColors and DisplayedSkinParts (protocol:295); the spec never says
  what the client sends, although M1's exit criterion depends on connecting successfully.
- **The mandatory reply to Player Position And Look is not stated.** protocol:306-308 — "the client
  *must* answer 0x08 with serverbound **0x06** (same coordinates) or the server will keep
  teleporting it."
- **Player-list-before-spawn ordering.** protocol:316 — "Player List Item (0x38, action 0) must be
  processed before Spawn Player (0x0C) for the same UUID, or the entity will not be spawned."
- **Keep-alive cadence and Client Status usage.** protocol:311 — "vanilla sends one every ~1 s and
  disconnects after ~30 s of silence"; Client Status (0x16: respawn, request stats, open-inventory
  achievement) appears in no milestone.

**Fix:** reference protocol §2.3 from M1 and add these five obligations to M1/M4 explicitly.

---

### 22. The crypto handshake omits the four traps the report explicitly names — MINOR

**Spec:** line 222 — "The encrypted session: RSA-1024 server key, AES-128-CFB8 stream, SHA-1
signature." Line 236 — "RSA-1024 PKCS#1 v1.5 for the shared secret, AES-128-CFB8 afterwards with
the shared secret as both key and IV."

**Report:** launcher-assets-auth-survey.md:709-739 — (1) the key is 1024-bit *by default* but "It is
also possible for a modified or custom server to use a longer RSA key, without breaking official
clients", so "A Rust implementation must therefore parse DER generically … never hardcode 128
bytes"; (2) CFB**8** means "set up your 'feedback/segment size' to 8 bits or 1 byte … Any other
feedback size will result in encryption mismatch"; (3) "two independent cipher contexts (encrypt
and decrypt), each continuous across packets"; (4) padding is "**PKCS#1 v1.5**, not OAEP".

**Why it matters:** all four are one-line omissions that produce silent, hard-to-debug failures
(garble that looks like a compression bug, decryption that works for one packet then dies). The
spec states the happy path and none of the invariants.

**Fix:** add one sentence to §7.4 and §8: "Parse the server key as DER `SubjectPublicKeyInfo` and
size the ciphertext from the parsed modulus (never assume 128 bytes); CFB8 at 8-bit segment size;
two continuous cipher contexts, IV = key = shared secret; PKCS#1 v1.5 padding." Add the three
golden hexdigest vectors (`sha1(Notch)`/`sha1(jeb_)`/`sha1(simon)`, survey:622-626) as a named
unit test.

---

### 23. Small numeric omissions around clouds, physics and reach — MINOR

- **Clouds:** line 263 — "Flat cloud layer at y=128, vanilla texture and scroll speed". The report
  draws it at `cloudHeight − cameraY + 0.33` with 1 block/tick westward drift and a 1/2048 UV scale
  (render:336-337). The 0.33 offset is visible when standing at cloud height.
- **Physics provenance:** line 287 states "walk 4.317, sprint 5.612, jump height 1.2522" as vanilla
  constants; the report derives them from the recurrence and flags the wiki's 1.2522 as "post-15w45a"
  (render:475, 481). Fine as a working value, but the provenance should be recorded in §12's
  physics tests so a future reader knows it is derived, not read from source.
- **Reach:** line 289 — "4.5-block block reach". Not covered by any report; in vanilla 1.8.9 the
  block reach is gamemode-dependent (creative 5.0, survival 4.5), so the flat value is incomplete
  for the creative-mode testing M5 needs. Needs a source and both values.

---

### 24. Ambiguities two competent developers would resolve differently — MINOR

1. **Line 291:** "Options that are meaningless for a native renderer, **such as** Advanced OpenGL,
   remain visible with their 1.8 layout and act as documented no-ops." The set is open-ended and
   undefined. Which of VSync, Max Framerate, Use VBOs (off by default in 1.8.9 — render:192), 3D
   Anaglyph, Fullscreen/Resolution, GUI Scale are no-ops? One developer no-ops all four; another
   implements VSync and framerate cap, which R1's measurement depends on.
2. **Line 92 (S2):** "Malformed or hostile packets never panic the client; they disconnect with a
   clear error", versus the report's guidance that "unknown ⇒ **skip/save the payload**" (protocol:188).
   Is an unknown-but-well-formed id "hostile" (disconnect) or skippable (join modded servers)?
3. **Line 70 (P4):** "a server sees the same packet sequence a vanilla client would send for the
   same actions" — exact equality, or equality up to timing and state-dependent variants (0x03 vs
   0x04/0x05/0x06)? Two readers produce different verdicts on the same run.
4. **Line 83 (R3):** "Cold start under 1 second to the main menu, **excluding asset download**" —
   does that exclude the SHA-1 verification pass over ~115 MB of objects and the atlas build, or
   only the network? On a cold page cache those two alone can exceed a second.
5. **Line 186:** `profiles.json  optional, phase 2` versus M8's "profile management" in v1 — is
   profile management in v1 (M8) or phase 2?

---

### 25. Smaller documentation-level slips — MINOR

- **Line 208:** "Extracted content: `assets/minecraft/{textures,models,blockstates,font,texts,
  shaders,misc,lang}`" — there is no `misc` directory at that level in the jar (jar census:
  `models/` 1,595 · `textures/` 1,058 · `blockstates/` 340 · `shaders/` 87 · `texts/` 3 · `lang/` 1
  · `font/` 1 — launcher-assets-auth-survey.md:278-286), and `textures/misc` is already covered by
  `textures`. Also state that `textures/**/*.mcmeta` sidecars are included (they are what makes
  lava/fire/portal animate — launcher-assets-auth-survey.md:320-332).
- **Line 303:** "The **57 side-by-side screenshot tests**" — the report's list mixes screenshot
  checks with behavioural ones (#46 interpolation, #49 mouse look, #50 movement feel, #52 audio,
  #54 font metrics). Either reclassify the list in the parity document or define which of the 57
  are screenshot-diffable and how the rest are verified.
- **Line 279:** "vanilla FOV formula including the sprint and water modifiers" — the report's
  formula also includes flying (×1.1), the bow-draw reduction (down to ×0.85), the death zoom, and
  the 0.5-per-tick smoothing clamped to [0.1, 1.5] (render:411-423; test #48 at render:561). Add
  them, and note that the projection is `gluPerspective(fov, aspect, 0.05, farPlane*√2)`.
- **Lines 7-8 / §1:** the 1.8 → 1.8.9 protocol relationship is never stated. The report is explicit
  — "There are no packet-id or field differences between 1.8.0, 1.8.8 and 1.8.9 — one codec set
  covers the whole 1.8.x line" (protocol:149) — and it is the justification for targeting 1.8.9
  while shipping a single `oxide-proto-v47`. Record it in §1 and in `oxide-proto-v47`'s description.
- **Line 137:** "for example `oxide-proto-v2126`" — no protocol number near 2126 exists (1.9 is
  107; the newest in the reports is 776). Harmless as an illustration, but it suggests the version
  roadmap is not grounded; use a real number or a placeholder.
- **Line 222:** "SHA-1 signature" — the session-server value is a hash, not a signature, and it is
  the non-standard Minecraft hexdigest (survey:615-631), not a plain SHA-1 hex string.

---

## Strongest parts (do not churn these)

1. The evidence discipline: the research reports are source-tagged, quote-verified and hedge their
   own gaps, and the spec's external numbers (jar SHA-1/size, 734 objects, 74/26 packet ids,
  4.317/5.612, threshold 256, RSA-1024/AES-CFB8, cloud 128, 40 particles, 57 tests) all reconcile.
2. The crate graph and layering rules (5.1/5.2) — small, explicit, CI-enforced, and the launcher's
   separation from the client is exactly right.
3. The decision log D1–D10 plus `DIVERGENCES.md`, `STATE.md` and the handoff template: the project
   has a real handoff discipline most specs never reach.
4. The renderer's fidelity techniques (per-face quads with baked light/AO, one atlas, `.mcmeta`
   animation, three terrain queues, component-based vertex layout) are the correct choices for
   preserving the vanilla image.
5. The asset/legal architecture (runtime fetch from Mojang only, hash-verified store, jar read as a
   read-only archive, nothing redistributed) follows the verified survey rather than folklore.

---

## Claims I could not verify

- **1.8.9's render-distance slider maximum.** Not stated in any report. M9's "32-chunk render
  distance" (line 325) therefore cannot be checked against 1.8's option set; if 1.8 tops out at 16
  (as I believe), a 32-chunk mode is a new, non-vanilla option that test #4's "exactly the 1.8
  slider set" would fail.
- **Whether all 57 parity tests are screenshot-observable.** Several are behavioural or audio
  (see finding 25); the parity report does not classify them.
- **The provenance of jump height 1.2522.** The report derives it by simulating the recurrence and
  ties it to a wiki value it labels "post-15w45a" (a 1.9 development snapshot), so its 1.8.9
  provenance is asserted rather than shown.
- **The vanilla call site for Map Chunk Bulk (0x26).** The protocol report explicitly says it could
  not locate it ("⚠ The exact vanilla call site that emits 0x26 … was not located", protocol:393-395),
  yet the spec states "`Map Chunk Bulk` (0x26) **is always** ground-up" (line 239) as settled fact.
- **The `.mcassetsroot` marker and the vanilla-install reuse path** (spec line 202) — the survey
  marks the marker's exact semantics unverified (line 1043).
- **The 1.8 "option set"** — no report enumerates the full options hub (Skin Customization, Language,
  Chat, Snooper, Resource Packs, and miscellaneous buttons such as Super Secret Settings / 3D
  Anaglyph are not covered), so the spec's "options (the 1.8 option set)" cannot be checked against
  the research and the "meaningless options" clause cannot be bounded.
- **Vanilla's default Max Framerate / VBO setting interaction with R1.** The report notes VBOs are
  off by default in 1.8.9 but gives no default framerate cap, so a "1.5× FPS" comparison cannot be
  bracketed without deciding it explicitly.
- **Nametag rendering.** Not covered by the parity report's inventory or its 57 tests; I could not
  verify the intended appearance from the research set (only that another client implements it).
- **The 4.5-block reach in line 289** — absent from every report; cannot be verified, and I believe
  it is gamemode-dependent.
- **Whether vanilla's 1.8.9 client starts under niri/XWayland with a standalone JRE 8** — a rig
  feasibility question, not answered by any document; `docs/STATE.md` says the rig is "being set up".
- **The legal reading itself** — the survey is explicit that its EULA analysis is "reconnaissance,
  not legal advice" (launcher-assets-auth-survey.md:9, 1015-1019); the spec inherits that caveat
  without restating it.

---

## Disposition record

Added 2026-09-22 after spec v2. Every finding below was applied to
`docs/specs/oxidecraft-v1-design.md`; nothing was declined.

| # | Severity | Disposition in spec v2 |
| --- | --- | --- |
| 1 | Blocker | Applied, §9: chunks are 16x16x256 in sixteen sections; mask is 16 bits; y=0..255 stated |
| 2 | Blocker | Applied, P5, §9, M4: two stored positions, `partialTicks` interpolation, 4-block teleport snap; 3-tick cadence named as server-side |
| 3 | Blocker | Applied, §8: six explicit rules (array-major grouping, index order, nibble order, dimension sky rule, biome tail, size formula) plus the 12,544-byte fixture |
| 4 | Blocker | Applied, §9 and §5.1/5.3: behaviour table is code-defined in `oxide-world`; JSON models stay in `oxide-assets`; `oxide-game` joins them |
| 5 | Blocker | Applied, appendix A and §15: serde, toml, simdnbt, dirs, hex added; dependency, lockfile, and MSRV policy stated |
| 6 | Major | Applied, §7.2: manifest path and contents, atomic writes, XDG resolution, single-instance lock, retry policy, disk precheck, verify command, content probe for vanilla-install reuse |
| 7 | Major | Applied, §12: GitHub Actions named, cross-target checks, committed `deny.toml`, and the asset guard that fails on Mojang binaries in git |
| 8 | Major | Applied, §9: all three sky-light rules, filtering blocks, four-bit ranges, mask-outside rule, 0x22/0x23 recomputation |
| 9 | Major | Applied, §10: water exponential at 0.1 (0.01 with Water Breathing), lava exponential at 2.0, sky-pass linear 0 to far plane |
| 10 | Major | Applied, §11.2 and §16: full server-openable container set, statistics and achievements, skin customization; post-v1 items recorded in `docs/DIVERGENCES.md` |
| 11 | Major | Applied, F5/F6/F9/F10/F12, §10, §11.2, M4/M5/M6: boss bar, nametags, object entities, third-person camera, chat layout, music ticker, F2 and F3 sub-modes, resize and fullscreen, downloading-terrain screen |
| 12 | Major | Applied, P3 (named list), P4, M2/M3/M6/M8/M9 exits, appendix C: measurement procedures, physics test vectors, defined fresh-machine test, documentation-complete list |
| 13 | Major | Applied, §6: snapshot lag restricted to meshing; overlays read current tick state; 100 ms input-to-visible latency budget with a test |
| 14 | Major | Applied, D11–D15, §11.2, §16: options.toml, title text, `MC|Brand = vanilla`, closed no-op set, render distance capped at the vanilla 16 |
| 15 | Major | Applied, §7.4 and §5.3: client-side session join with an injection point in the login state; hexdigest helper with three golden vectors; `SkinCache` owns skin fetch and cache |
| 16 | Major | Applied, §1.1, §14, M0 exit: legal posture section, required disclaimer text in the README, naming constraint, offline-mode policy, `.class`-never-read invariant |
| 17 | Major | Applied, §7.4, M7 row, §14, §16: Azure registration and API permission tracked from before M6, with a refusal fallback |
| 18 | Major | Applied, §14 and appendix C.1: rig risks, XWayland caveat, explicit GPU and driver selection, vanilla settings archive, colour-space policy, rig proven in M0 |
| 19 | Major | Applied, F14, §9, §11.1, §12, M3: death and respawn state clearing, sky-light flag re-derivation, Player Position And Look answer, world-layer tests |
| 20 | Major | Applied, §5.1/5.2 rules 3, 6, 7, §9, §16: version-parameterised storage with per-version codecs; auth ownership corrected; assets edge documented |
| 21 | Minor | Applied, §8: threshold read from Set Compression, −1 disables, rule measured on packet id plus data, broken play-state 0x46 never used; five connection obligations added to M1/M4 |
| 22 | Minor | Applied, §7.4: generic DER parsing with modulus-sized ciphertext, CFB8 segment size, two continuous contexts, PKCS#1 v1.5, hexdigest named as a hash with golden vectors |
| 23 | Minor | Applied, §10 (cloud 0.33 offset, drift, UV scale), §11.1 and §12 (physics provenance), reach values by gamemode with M3 verification |
| 24 | Minor | Applied, D14 (closed no-op set), S2 (unknown id skipped, malformed framing fatal), P4 (allowed variants enumerated in appendix C.3), R3 (warm store defined), §7.1 profiles |
| 25 | Minor | Applied, §1 (1.8.x protocol identity), §7.3 (jar census, `.mcmeta` sidecars, no `misc`), §10 (full FOV modifiers and projection), appendix C.6 (checklist classification), §5.2 (`oxide-proto-v107` example), §7.4 (hexdigest, not signature) |

Items the review could not verify are now tracked in section 16 of the specification and in
appendix C, each with a "confirm on the rig" rule: the 1.8.9 render-distance maximum and options
set, the `Map Chunk Bulk` call site, `.mcassetsroot` semantics, and nametag appearance.
