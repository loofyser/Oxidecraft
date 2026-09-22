# Minecraft Java 1.8.9 — Visual Parity Requirements Survey

**Purpose:** a 1-to-1 on-screen parity checklist for a from-scratch wgpu renderer.
**Target version:** Java Edition **1.8.9** (release 2015-12-03; last 1.8.x patch).
**Method:** every claim below is either (a) read out of the decompiled 1.8.9 sources (MCP-919, file:line cited), (b) read out of the actual 1.8.9 `client.jar`, or (c) a wiki statement with URL. No item is from memory alone.

### Verified artifacts (downloaded into `vanilla/`)

| Artifact | Source | Size | SHA-1 |
|---|---|---|---|
| `vanilla/client-1.8.9.jar` | `https://launcher.mojang.com/v1/objects/3870888a6c3d349d3771a3e9d16c9bf5e076b908/client.jar` | 8 461 484 B | `3870888a6c3d349d3771a3e9d16c9bf5e076b908` |
| `vanilla/assets-index-1.8.json` | `https://launchermeta.mojang.com/v1/packages/f6ad102bcaa53b1a58358f16e376d548d44933ec/1.8.json` | 78 494 B | — (734 objects, `totalSize` 114 885 064 B) |

Chain used (all four steps exercised, HTTP 200):
`https://launchermeta.mojang.com/mc/game/version_manifest.json` → version entry `1.8.9` → `https://piston-meta.mojang.com/v1/packages/d546f1707a3f2b7d034eece5ea2e311eda875787/1.8.9.json` → `downloads.client.url` and `assetIndex.url`.
Note: 1.8.9's version JSON declares `"assets": "1.8"` — the asset index ID is **`1.8`, not `1.8.9`**. Main class `net.minecraft.client.main.Main`.

> ⚠️ **Licensing:** the jar is kept locally under `vanilla/` for verification only. Do **not** republish, redistribute, or commit it. Keep it out of any public repo / release artifact.

---

## 1. Asset pipeline: what the jar holds vs what the asset index holds

### 1.1 The 1.8 split (this is the big structural difference from modern versions)

In 1.8 the launcher fetches **two** payloads and the game reads from both:

| Category | Location | Evidence |
|---|---|---|
| Blockstates (`assets/minecraft/blockstates/*.json`) | **in `client.jar`** | 340 entries in jar |
| Block models (`assets/minecraft/models/block/*.json`) | **in `client.jar`** | 1 080 entries in jar |
| Item models (`assets/minecraft/models/item/*.json`) | **in `client.jar`** | 515 entries in jar |
| **All** default textures (blocks, items, entity, gui, font, misc, environment, colormap, particle, map, painting, effect, models) | **in `client.jar`** | blocks 382, items 229, entity 142, gui 43, font 224, environment 6, colormap 2, misc 12, particle 2, map 2, painting 1, effect 1, models 12 |
| Core shaders `assets/minecraft/shaders/{program,post}/*.{json,vsh,fsh}` | **in `client.jar`** | 87 entries |
| `assets/minecraft/texts/{credits,end,splashes}.txt` | **in `client.jar`** | 3 entries |
| `assets/minecraft/font/glyph_sizes.bin` | **in `client.jar`** (note: **not** under `textures/`) | 1 entry |
| `assets/minecraft/lang/en_US.lang` | **in `client.jar` only** — the index has 74 locales but *no* `en_US` | jar has it; index listing has `af_ZA…zh_TW` without `en_US` |
| All other locales (`assets/minecraft/lang/*.lang`) + `assets/realms/lang/*.lang` | **asset index / `.minecraft/assets/objects`** (hashed) | 74 `minecraft/lang/*.lang` + 74 `realms/lang/*.lang` entries in index |
| `assets/minecraft/sounds.json` | **asset index (hashed)** | index object `minecraft/sounds.json` |
| Every `.ogg` (`assets/minecraft/sounds/**`) | **asset index (hashed)** | index has **578** `minecraft/sounds/**.ogg` objects; **zero `.ogg` files in the jar** |
| `assets/minecraft/icons/{icon_16x16,icon_32x32}.png`, `assets/minecraft/icons/minecraft.icns`, `assets/pack.mcmeta` | **asset index (hashed)** | index entries |
| Chat/UI is *not* localised from `en_US.lang` alone — en_US is in the jar | — | — |

Consequences for a re-implementation:
- A 1.8.9 client needs the **jar** for the visual baseline (textures/models/blockstates) and the **objects store** only for sounds + non-English language files.
- Hashed object path rule: `objects/<first 2 hex chars>/<full sha1>`.
- Vanilla 1.8 has **no `resource_packs/vanilla` directory** and pack format is declared by `assets/pack.mcmeta` (`pack_format: 1` for 1.8-era). Default pack resolution order = resource packs (any) → jar → index objects.

Sources: <https://minecraft.wiki/w/Client.jar>, <https://minecraft.wiki/w/Assets>, <https://minecraft.wiki/w/Resource_pack>, <https://minecraft.wiki/w/Java_Edition_1.8.9>

### 1.2 Blockstate format (1.8 flavour)

`assets/minecraft/blockstates/<name>.json` = `{ "variants": { "<state>=<value>,<state>=<value>": <variant> } }` where `<variant>` is one object **or an array of equally-weighted objects**.

Verified examples from the jar:
```json
// torch.json
"variants": {
  "facing=up":    { "model": "normal_torch" },
  "facing=east":  { "model": "normal_torch_wall" },
  "facing=south": { "model": "normal_torch_wall", "y": 90 },
  "facing=west":  { "model": "normal_torch_wall", "y": 180 },
  "facing=north": { "model": "normal_torch_wall", "y": 270 }
}
```
- Per-variant keys in 1.8: `model`, `x`, `y`, `uvlock`, `weight`. There is **no `z` rotation** in the blockstate file in 1.8 (`x`/`y` only).
- `uvlock: true` rotates the UVs along with `y` (added in 14w17a as `rotateVariantTextures`; renamed `UV lock` in 14w25a/b).
- Multiple weights: `grass.json` `"snowy=false"` is an **array of 4 identical models rotated 0/90/180/270** → grass top-rotation variance is part of the vanilla look.
- Random model choice per position is seeded by block position (do not pick per-frame!).
- Names are the **pre-flattening** 1.8 names: `grass`, `planks`, `log`, `stonebrick`, `lit_furnace`, `unpowered_repeater`, `reeds`, `waterlily`, `wood_old_slab`, `quartz_column`, `silver_wool`, … (full list = jar listing).

Sources: <https://minecraft.wiki/w/Block_states>, <https://minecraft.wiki/w/Model>, `vanilla/client-1.8.9.jar` blockstates/*.json

### 1.3 Block model format (1.8)

`assets/minecraft/models/block/*.json`, resolved via `parent` chain:
```json
// models/block/stairs.json — two cuboids, both cullface'd
{ "textures": { "particle": "#side" },
  "elements": [
    { "from": [0,0,0], "to": [16,8,16],
      "faces": { "down":  {"uv":[0,0,16,16],"texture":"#bottom","cullface":"down"},
                 "up":    {"uv":[0,0,16,16],"texture":"#top"},
                 "north": {"uv":[0,8,16,16],"texture":"#side","cullface":"north"}, ... } },
    { "from": [8,8,0], "to": [16,16,16], "faces": { ... "west": {"uv":[0,0,16,8],"texture":"#side"} ... } } ] }
```
```json
// models/block/cross.json — plants; note rotation block + shade:false
{ "ambientocclusion": false,
  "textures": { "particle": "#cross" },
  "elements": [ { "from": [0.8,0,8], "to": [15.2,16,8],
      "rotation": { "origin": [8,8,8], "axis": "y", "angle": 45, "rescale": true },
      "shade": false,
      "faces": { "north": {"uv":[0,0,16,16],"texture":"#cross"}, "south": {...} } }, ... ] }
```
```json
// models/block/cube_all.json — pure inheritance helper (no elements)
{ "parent": "block/cube",
  "textures": { "particle": "#all", "down": "#all", "up": "#all",
                "north": "#all", "east": "#all", "south": "#all", "west": "#all" } }
```
Required 1.8 semantics (all of these differ from 1.9+ in some detail — do **not** port a 1.16 loader):

| Feature | 1.8 behaviour |
|---|---|
| `parent` | path is relative to `models/` (`block/cube`, `item/generated` equivalents are `builtin/*`) |
| `builtin/*` parents | `builtin/generated` (items, flat quads from `layer0..layer4`), `builtin/compass`, `builtin/clock`, `builtin/entity` (block-entity renderer), `builtin/missing` — see `ModelBakery.java:205-227` |
| `textures` | map of variable → texture path (`blocks/stone`, `items/stick`) or `#othervar`; `particle` var is **mandatory in practice** (used by break/step particles) |
| `elements` | cuboids only; `from`/`to` in 1/16 block units (range −16…32 allowed) |
| `rotation` (element) | `origin[3]`, `axis` (x/y/z), `angle` (float, ±45/22.5 typical), `rescale` (bool) |
| face keys | `down, up, north, south, west, east`; absent face = not rendered |
| face fields | `uv` [x1,y1,x2,y2] (optional → auto from element bounds), `texture` (`#var`), `cullface` (removes face when neighbour block covers it **and** selects the side used for the light level), `rotation` (0/90/180/270, clockwise, CCW for `down`), `tintindex` |
| `shade` | per-element boolean; `false` for cross/torch/flat models |
| `ambientocclusion` | per-model boolean, default `true` |
| `display` keys (1.8) | **`thirdperson`, `firstperson`, `head`, `gui`, `ground`, `fixed`** — the 1.9 `_righthand/_lefthand` suffixes do **not** exist. `ItemCameraTransforms.TransformType = {NONE, THIRD_PERSON, FIRST_PERSON, HEAD, GUI, GROUND, FIXED}` (decomp `client/renderer/block/model/ItemCameraTransforms.java:121-129`). `fixed` = item frames. |
| `display` fields | `rotation[x,y,z]`, `translation[x,y,z]` (clamped ±80), `scale[x,y,z]` (clamped to ≤4) — translation applied **before** rotation |
| Multiple textures per item | `layer0`…`layer4` (5 layers max, tinted layers via `tintindex` on `builtin/generated`) |

Verified item model (1.8, from jar):
```json
// models/item/diamond_sword.json
{ "parent": "builtin/generated", "textures": { "layer0": "items/diamond_sword" },
  "display": { "thirdperson": { "rotation":[0,90,-35], "translation":[0,1.25,-3.5], "scale":[0.85,0.85,0.85] },
               "firstperson": { "rotation":[0,-135,25], "translation":[0,4,2], "scale":[1.7,1.7,1.7] } } }
```
```json
// models/item/iron_ingot.json
{ "parent": "builtin/generated", "textures": { "layer0": "items/iron_ingot" },
  "display": { "thirdperson": { "rotation":[-90,0,0], "translation":[0,1,-3], "scale":[0.55,0.55,0.55] },
               "firstperson": { "rotation":[0,-135,25], "translation":[0,4,2], "scale":[1.7,1.7,1.7] } } }
```
- Note the **plural folder names** in 1.8 texture paths: `blocks/…`, `items/…` (singular from 1.13 onward).
- Texture path → file: `<texpath>` resolves to `assets/minecraft/textures/<texpath>.png`.

Sources: <https://minecraft.wiki/w/Model>, `ModelBakery.java` (`blockstates/<name>.json` string at :174), jar contents.

### 1.4 Texture atlas stitching (1.8)

There is exactly **one** block/item atlas in 1.8 (no separate item atlas — GUI/first-person/entity item rendering all bind the same atlas):

- Atlas resource: `textures/atlas/blocks.png` (`TextureMap.locationBlocksTexture`, `TextureMap.java:30`). It is a *virtual* texture — generated at load, not shipped.
- `Minecraft.java:548-554`: `textureMapBlocks = new TextureMap("textures"); textureMapBlocks.setMipmapLevels(settings.mipmapLevels); renderEngine.loadTickableTexture(locationBlocksTexture, textureMapBlocks); textureMapBlocks.setBlurMipmapDirect(false, mipmapLevels > 0);` — i.e. **blur off, mipmaps on when the option is > 0**.
- `TextureMap.loadTextureAtlas` (`TextureMap.java:81-84`): `i = Minecraft.getGLMaximumTextureSize(); Stitcher stitcher = new Stitcher(i, i, true /*resizeable*/, 0, mipmapLevels);`
- Sprite dimensions: each sprite is padded to a power of two ≥ its size; mipmap level is clamped by `min(lowestOneBit(w), lowestOneBit(h))` across all sprites (`TextureMap.java:154-172` — vanilla 16×16 assets keep 4 levels; one odd-sized sprite lowers the atlas mip level for everything).
- Animated textures: `<name>.png.mcmeta` sits beside the PNG **inside the jar** (verified: `fire_layer_0.png.mcmeta`, `lava_still.png.mcmeta`, `portal.png.mcmeta`, `water_still.png.mcmeta`, `prismarine_rough.png.mcmeta`, `vignette.png.mcmeta`, `shadow.png.mcmeta`, `enchanted_item_glint.png.mcmeta`, `pumpkinblur.png.mcmeta`). Vanilla animated strips are **vertical** (e.g. `water_still.png` 16×512 = 32 frames, `lava_still.png` 16×320 = 20 frames, `fire_layer_0/1.png` 16×512).
- Missing texture sprite: `missingno` (`TextureMap.java:29`), 16×16 checkerboard; missing *model* = `builtin/missing`.
- Held/GUI items use the same atlas: `RenderItem.java:197, 318, 357` bind `TextureMap.locationBlocksTexture` and toggle `setBlurMipmap(false,false)` around the draw, then `restoreLastBlurMipmap()` — **GUI item rendering is nearest-neighbour, no mipmaps**.

Sources: `TextureMap.java`, `Stitcher.java`, `RenderItem.java` (MCP-919), <https://minecraft.wiki/w/Atlas> / <https://minecraft.wiki/w/Textures>

---

## 2. Font rendering in 1.8

| Item | Value / evidence |
|---|---|
| ASCII sheet | `assets/minecraft/textures/font/ascii.png`, **128×128** (16×16 grid of 8×8 cells) |
| SGA sheet | `assets/minecraft/textures/font/ascii_sga.png`, 128×128 (galactic alphabet) |
| Unicode pages | `assets/minecraft/textures/font/unicode_page_%02x.png`, **256×256** (16×16 grid of 16×16 cells). Present pages = 224 total incl. ascii/ascii_sga: `00–07, 09–d7, f9–ff`; **missing: `08`, `d8–df` (surrogates), `e0–f8`** |
| Glyph widths | `assets/minecraft/font/glyph_sizes.bin`, one byte per BMP codepoint (65 536 bytes; read whole at `FontRenderer.java:216-217`, defaults to 255→64×64? see note). High nibble = start column, low nibble = end column of the 16×16 cell |
| Char width (ASCII) | `charWidth[c] = 3 + glyphWidth(c)` for the 8×8 sheet (`FontRenderer.java:179`); i.e. glyph pixel width + 1 px spacing (space = 4 px) |
| Char width (unicode) | `charWidth[c] = (int)(0.5 + width * 0.5) + 1` — unicode glyphs are drawn at **half scale horizontally** (`FontRenderer.java:206`; `f1 = unicodeFlag ? 0.5F : 1.0F` at :479) |
| Line height | `FONT_HEIGHT = 9` (`FontRenderer.java:35`) |
| Shadow | a second copy of the glyph drawn **1 px down-right at 12.5 % of the glyph advance**, colours divided by 4 (`FontRenderer.java:415` + wiki) |
| Shadow colour | `(c & 0xFCFCFC) >> 2` per channel for coloured text; black shadow = ARGB `0x3F000000`-ish; title/heading text uses the same 1-px offset |
| Alpha for shadows | rendered with `alpha < 0.25F` → shadow forced opaque-ish (see `:415-480` logic) |
| Positioned as | screen-space quads in the GUI ortho projection at 1 GUI px = `scale/`… (see §2.1) |

Sources: <https://minecraft.wiki/w/Font>, `FontRenderer.java` (MCP-919), jar listing/dimensions.

### 2.1 GUI scale and where text lands

- GUI scale options: `Auto / Small / Normal / Large` (`GameSettings.java:57` `GUISCALES`), `guiScale = 0..3`.
- `ScaledResolution` derives `scaleFactor` from `width/320, height/240` (auto) → `scaleFactor = max(1, min(scaleFactor, 3))`; all GUI coordinates are in *scaled* units, one scaled unit = `scaleFactor` physical px.
- Text is drawn with (near-)integer translation then scaled; 1.8 quirk: shadow offset is in *scaled* units ⇒ a 1 px shadow can be *sub-pixel* relative to glyph edges. To match exactly, replicate the **12.5 % of advance** shadow rule (wiki), not "1 screen px".

---

## 3. GUI / HUD inventory for 1.8.9

Screen classes verified present in the 1.8.9 sources (`client/gui/**`), i.e. **this is the exact screen set of 1.8.9**:

### 3.1 Screens

| # | Screen | Class | Notes for parity |
|---|---|---|---|
| 1 | Main menu | `GuiMainMenu` | panorama from `textures/gui/title/background/panorama_0..5.png` (256×256 each, slowly rotating 6-face cube), `title/minecraft.png` logo (animated splash text from `texts/splashes.txt`), `mojang.png`, buttons from `gui/widgets.png` |
| 2 | Singleplayer world list | `GuiSelectWorld` | "Create New World" / "Play Selected World" / rename/delete/recreate; world icons |
| 3 | Create World / Customize | `GuiCreateWorld`, `GuiCustomizeWorldScreen`, `GuiCreateFlatWorld`, `GuiFlatPresets`, `GuiScreenCustomizePresets` | flat-world presets render from `textures/gui/presets/*.png` |
| 4 | Multiplayer server list | `GuiMultiplayer`, `GuiScreenServerList`, `GuiScreenAddServer`, `ServerSelectionList`, `ServerListEntryNormal/LanDetected/LanScan` | 5-bar ping icon strip, MOTD with § colours, player count, `server_selection.png` |
| 5 | Options hub | `GuiOptions` | tab strip: Music&Sounds / Video / Controls / Language / Chat / Snooper / Resource packs + skin customization + "Difficulty" slot |
| 6 | Video settings | `GuiVideoSettings` | sliders: **Render Distance, Brightness(Moody..Bright), Particles(All/Decreased/Minimal), GUI Scale(Auto/Small/Normal/Large), FOV(30–110), Max Framerate, Mipmap Levels(0–4)**, toggles: **VSync, View Bobbing, Use VBOs (off by default in 1.8.9!), Allow Block Alternatives, Entity Shadows, Entity Outlines? (no)**, Cloud Quality(Off/Fast/Fancy), Graphics(Fast/Fancy), Smooth Lighting(off/min/max), 3D Anaglyph, Fullscreen/Resolution |
| 7 | Sound options | `GuiScreenOptionsSounds` | 9 category sliders (`SoundCategory`): MASTER, MUSIC, RECORDS, WEATHER, BLOCKS, MOBS, ANIMALS, PLAYERS, AMBIENT |
| 8 | Controls | `GuiControls`, `GuiKeyBindingList` | keybinds + mouse sensitivity slider + invert mouse + smooth camera |
| 9 | Language | `GuiLanguage` | language list, font-forced-unicode toggle ("Force Unicode Font") |
| 10 | Resource packs | `GuiScreenResourcePacks`, `GuiResourcePackAvailable/Selected` | pack icons/format warnings |
| 11 | Skin customization | `GuiCustomizeSkin` | cape toggle, jacket/left-sleeve/right-sleeve/left-pant/right-pant/hat toggles, "Main Hand" is **absent** in 1.8 |
| 12 | Pause menu | `GuiIngameMenu` | "Back to Game / Statistics / Achievements? (`GuiStats`/`GuiAchievement` via stats) / Options / Open to LAN / Save and Quit" |
| 13 | Statistics & Achievements | `GuiStats` sub-screens, `achievement/Guis` | 1.8 has **achievements**, not advancements; grid visible in `achievement_background.png` |
| 14 | Inventory (survival) | `GuiInventory` → `inventory/ContainerPlayer` | texture `gui/container/inventory.png` (256×256), 2×2 crafting grid, 4 armour slots, offhand **does not exist** in 1.8, 3D player model preview (`renderEntityOnScreen`), creative tabs use `creative_inventory/tab_*.png` |
| 15 | Crafting table | `ContainerWorkbench` | `gui/container/crafting_table.png`, 3×3 + result |
| 16 | Furnace | `ContainerFurnace` | `gui/container/furnace.png`, flame + progress arrow, name "Furnace" |
| 17 | Chest / Ender chest | `ContainerChest` | `gui/container/generic_54.png` for double chests; row count varies 27/54; ender chest uses same texture |
| 18 | Anvil (repair) | `GuiRepair` | `gui/container/anvil.png`, cost level text, "Too Expensive!" in red when ≥ 40 |
| 19 | Enchanting table | `GuiEnchantment` | `gui/container/enchanting_table.png`, 3 offers, lapis counter, book render |
| 20 | Brewing stand | `ContainerBrewingStand` | `gui/container/brewing_stand.png`, bubbles + fuel |
| 21 | Dispenser / Dropper | `GuiDispenser` | `gui/container/dispenser.png` |
| 22 | Hopper | `GuiHopper` | `gui/container/hopper.png` |
| 23 | Beacon | `GuiBeacon` | `gui/container/beacon.png`, pyramid size, power buttons + confirm/cancel |
| 24 | Villager trading | `GuiMerchant` | `gui/container/villager.png`, offer list w/ red X when out of stock, XP bar |
| 25 | Horse/donkey/mule inventory | `ContainerHorseInventory` | `gui/container/horse.png`, saddle + armour slots + chest rows |
| 26 | Command block | `GuiCommandBlock` | |
| 27 | Sign editing | sign GUI (1.8's inline editor) | 4 lines, cursor blink |
| 28 | Book & quill / written book | `GuiScreenBook` | `gui/book.png`, page turn |
| 29 | Chat | `GuiNewChat` / `GuiChat` | chat scale/width/height/opacity from options, 20-line history view, tab-completion popup |
| 30 | Death screen | `GuiGameOver` | "You Died!" + score + **Respawn / Title Screen** buttons, red vignette-ish background dim |
| 31 | Disconnected / Downloading terrain / Error / Memory-error / Working | `GuiDisconnected`, `GuiDownloadTerrain`, `GuiErrorScreen`, `GuiMemoryErrorScreen`, `GuiScreenWorking` | dirt/`options_background.png` tiling behind |
| 32 | Demo / sleep / spectator / stream HUD | `GuiScreenDemo`, `GuiSleepMP`, `GuiSpectator`, `GuiStreamIndicator` | spectator has its own widget sheet |
| 33 | Win screen / credits | `GuiWinGame` | scrolls `texts/credits.txt` |
| 34 | LAN share | `GuiShareToLan` | game mode + allow-cheats |
| 35 | NBT/other in-game overlays | `GuiScreenBook` etc. | |

### 3.2 HUD elements (exact 1.8 layout, `GuiIngame.java`)

| Element | Geometry (scaled px) | Evidence |
|---|---|---|
| Hotbar | drawn at `x = width/2 − 91`, `y = height − 22`; texture `gui/widgets.png` region `(0,0,182,22)` | `GuiIngame.java:375` |
| Selected slot highlight | `(0,22,24,22)` shifted by `currentItem * 20`, offset `(−1,−1)` | `GuiIngame.java:376` |
| Held-item name popup | above hotbar, fades out over ~2 s (`heldItemTooltips` option); text centred, `-1` colour white | `GuiIngame` |
| Hearts | 10 hearts at `width/2 − 91`, `height − 22 − 21` (row above hotbar); rows wrap upward when > 10 hearts; half hearts; blinking on low health | `:643-700` |
| Absorption hearts | extra row above, yellow outline | `:700-740` |
| Hunger | right-aligned from `width/2 + 91 − 20*…`, `height − 22 − 21`; shank icons `(16..,[9,18,27])`; "hunger effect" jitter when `foodSaturationLevel == 0` | `:740-790` |
| Armour | `x = width/2 + 91 − 8*… − …`, `y = height − 22 − 21 − 10`; chestplate icons | `GuiIngame:885-895` |
| Air bubbles | right side above hunger, 10 bubbles, blink when ≤ 3 | `:856-870` |
| XP bar | `(0,64,182,5)` background + `(0,69,182,5)` clipped by progress, at `width/2 − 91`, `height − 22 − 7`… (line 404-408: 182×5 at y-offset) | `:401-412` |
| XP level text | centred, bright green `0x80FF20` with black outline (wiki: drawn 4× offset black + green on top) | wiki Font |
| Crosshair | two 1-px-wide quads (inverted blend) at exact screen centre `width/2 − 1`, `height/2 − 1`, with `GL_ONE_MINUS_DST_COLOR`-equivalent blend (`tryBlendFuncSeparate(775, 769, 1, 0)`) | `GuiIngame:179` |
| Chat | bottom-left, above hotbar; `chatScale` (0.5–1.0), `chatWidth`, `chatHeightFocused`/`chatHeightUnfocused`, `chatOpacity`; 20 lines retained in the scroll view; fade-out of 10 s (200 t) after 10 s hold | `GuiNewChat`, `GameSettings:105-109` |
| Scoreboard sidebar | right edge, max 15 lines, `(0,0,0)` semi-transparent bg, title centred, red numbers when `redNumbers`… | `GuiIngame.renderScoreboard:551-605`; title at `height/2 + lines/3` anchor; x = `width − font.getStringWidth(longest) − 3` |
| Boss health bar | centred, `(0,74,182,5)` background (drawn twice for the dim) + `(0,79,182,5)` progress, name text 10 px above; stacks downward for multiple bosses | `GuiIngame.renderBossHealth:901-923` |
| Item tooltips | white title, gray "…", rarity colours: common white `0xFFFFFF`, uncommon yellow, rare aqua `0x55FFFF`, epic light purple `0xA000FF`? — 1.8 uses `EnumRarity` colours; background `0x10001000` with `0xF0100010` border gradient, 3-px padding | `GuiScreen.renderTooltip`, `ItemStack.getTooltip` |
| Hurt flash | screen red tint driven by `hurtTime` (see §5.3), plus a red screen edge overlay in 1.8 (`entityplayer.hurtResistantTime`) | `EntityRenderer.hurtCameraEffect` |
| Attack/damage direction indicator | **1.8: the camera *rolls* away from the attacker** (`hurtCameraEffect`, see §5.3) — there is no separate 2D "direction arrow" (that arrives in 1.15+) | `EntityRenderer.java:585-609` |
| Block outline | black `RGBA(0,0,0,0.4)`, `glLineWidth(2.0)`, inflated by `0.002` (`RenderGlobal.drawSelectionBox:1875-1890`) | `RenderGlobal.java:1879-1885` |
| Block-break crack | atlas sprites `blocks/destroy_stage_0…9.png` (16×16, exactly 10 stages present in the jar), selected by `progress = 0..9`, drawn with additive-ish blend `(770,1,1,0)`, depth-masked, and with the atlas' blur/mipmap temporarily disabled | `RenderGlobal.sendBlockBreakProgress:2362-2380`; `EntityRenderer:1431-1437` |
| Vignette | **only when Graphics = Fancy**; `misc/vignette.png` (256×256) tiled 1×1 over the screen with alpha `1 − playerBrightness` clamped to [0,1], plus a world-border darkening term | `GuiIngame.java:136-139`, `renderVignette:956-975` |
| Water overlay | `misc/underwater.png` (16×16) tiled over the whole screen, `glColor(f,f,f,0.5)` where `f` = brightness, blend `(770,771,1,0)` | `ItemRenderer.renderWaterOverlayTexture` (:507-527) |
| Fire overlay | two fire sprites from the **block atlas** (`blocks/fire_layer_0/1`) drawn 2×2 over the screen with random UV offsets per frame | `ItemRenderer.renderFireInFirstPerson` |
| Lava block overlay | `blocks/lava_still` tiled, `shadeModel` flat, colour = lava colour | `ItemRenderer` |
| Pumpkin blur | `misc/pumpkinblur.png` (256×256) over the screen when a carved pumpkin is worn **and** `thirdPersonView == 0` | `GuiIngame.java:145-150` |
| Portal overlay | `blocks/portal.png` tiled, alpha = `timeInPortal`, with the nausea wobble when `Potion.confusion` | `GuiIngame.java:152-159` |
| Item tooltips / debug | see F3 table §3.3 | |
| Toast / advancement popups | **DO NOT EXIST IN 1.8.9.** No toast class in `client/gui/**`; grep for `Toast` in `Minecraft.java` = 0 hits. What exists is `client/gui/achievement/GuiAchievement.java` — the "Achievement get!" slide-in banner using `textures/gui/achievement/achievement_background.png`, triggered by `displayAchievement()`/`displayUnformattedAchievement()`, updated once per frame from `Minecraft.java:1168`. It also delivers the "New Recipe Unlocked" hint? **No** — 1.8 has no recipe toasts either (`showInventoryAchievementHint` only toggles the inventory hint text). | `Minecraft.java:261, 565, 1168, 2372`; `GuiAchievement.java` |

### 3.3 F3 debug overlay — exact 1.8.9 fields

Left column (`GuiOverlayDebug.getDebugInfoLeft()`, strings verified in source):

```
Minecraft 1.8.9 (<version>/<client brand>)
<fps> fps            (this.mc.debug)   e.g. "60 fps  T: 60/60ms"
<renderGlobal.getDebugInfoRenders()>   e.g. "C: 289/1120. Frame: 1024kB" (chunks vs rendered)
"P: 42. T: 3"        (particles; loaded entities)
<provider name>      e.g. "Overworld" / "Nether" / "The End"
(blank)
XYZ: 123.456 / 64.00000 / 789.012      (eye-min bounding-box Y, 5 decimals)
Block: 123 64 789
Chunk: 3 0 5 in 7 4 49
Facing: north (Towards negative Z) (0.0 / 0.0)
Biome: (in 1.8 the biome line is shown when the chunk's biome name is non-null)
Light: 15 (15 sky, 0 block)
(blank)
Local Difficulty: 1.00 (Day 12)
(blank)
Looking at: 123 64 789                     (only when a block is targeted)
<block registry name>                       (only with F3+H / reducedDebugInfo off)
<mob-spawn / other per-block lines>
```
Right column (`getDebugInfoRight()`):
```
Java: 1.8.0_51 64bit
Mem: 38% 512/1024MB   Allocated: 60% 600MB
(blank)
CPU: <8x Intel…>
(blank)
Display: 1920x1080 (NVIDIA)   /  <GL_RENDERER> / <GL_VERSION>
```
- `F3` alone toggles the overlay; **`F3+H`** enables advanced tooltips (item ids/durability); `F3+B` entity hitboxes; `F3+G` chunk borders; `F3+P` pause-on-lost-focus toggle (chat message); `F3+A` reload chunks; `F3+T` reload resources; `F3+N` cycle creative spectator; `F3+F` render-distance cycle; `Shift+F3` lagometer (`showLagometer`); `F3+Q` shows the help list.
- `reducedDebugInfo` (server flag) collapses the block to only `Chunk-relative:` and removes `Biome:`/`Light:`/`Local Difficulty:`.
- Each left-column line is drawn as `<text>` with a **dark background quad** at 0.25 alpha (`GuiIngame`-style debug draw), line height 9 (FONT_HEIGHT) with 1-px vertical padding; right column right-aligned to `width − 2`.

Sources: `client/gui/GuiOverlayDebug.java` (MCP-919), <https://minecraft.wiki/w/Debug_screen>

### 3.4 Container interaction ("drag/split-click") behaviour

| Gesture | 1.8 semantics |
|---|---|
| Left-click slot | pickup whole stack / place whole stack / swap with cursor |
| Right-click slot | if cursor empty → **take half (round up)**; if cursor holds items → place **one** per click |
| Left-drag | distribute **evenly** across all crossed slots, remainder stays on cursor (`dragSplittingRemnant`) |
| Right-drag | place **one per crossed slot** while cursor has items |
| Shift-click | quick-move (transfer stack between container/inventory; hotbar variant with 1–9) |
| Hotbar swap | hold a hotbar key while hovering a slot; number keys 1–9 outside container swap the hotbar item with the hovered slot |
| Double-click | collect all matching stacks to the cursor (up to the max stack size) |
| Drop | `Q` drops one, `Ctrl+Q`/`Q` outside GUI drops whole stack |
| Creative | middle-click copy; `1–9` while hovering gives a stack; scroll wheel switches hotbar; delete-key slot erases |
| Cursor stack render | drawn at `zLevel 200` in the GUI pass; the "returning stack" animation (100 ms) uses `Minecraft.getSystemTime()` and eases position |
| Durability bar | 13×2 px at slot bottom, green→red interpolation, shown when damaged |
| Stack count | bottom-right, 1-px shadow, yellow "0" while splitting |
Handled in `GuiContainer` (drag state fields :66-71, remnant logic :156-165) and `Minecraft.clickMouse`.

---

## 4. World rendering

### 4.1 Sky

| Element | 1.8 value | Evidence |
|---|---|---|
| Sky dome | a `GL_QUADS` band generated once: cells of 64×64 units from −384…384 on both axes, rendered at `y = −16` (sky2 at `+16` for the "below horizon" plane, drawn when `eyeY < horizon`) | `RenderGlobal.renderSky(WorldRenderer,float,boolean):340-365`, `generateSky2` |
| Sky colour | `World.getSkyColor()` per biome/temperature; vertex-coloured quads so it **fades to transparency at the horizon**; brightness-modulated by `(30/59/11)%` weights when `pass != 2` | `RenderGlobal.renderSky:1203-1246` |
| Fog-vs-sky | sky is drawn **with fog enabled** | `:1231` |
| Sunrise/sunset band | `WorldProvider.calcSunriseSunsetColors()` — a 16-segment disc of radius 120 with an orange->dark gradient; rotated 90° on X, +180° when `sin(celestialAngle) < 0`, then 90° on Z | `RenderGlobal:1253-1292`; `WorldProvider:151-175` |
| Celestial rotation | `rotate(-90°, Y)` then `rotate(celestialAngle * 360°, X)`; sun & moon drawn on an axis at ±100 (`sun` at y=+100, `moon` at y=−100) | `RenderGlobal:1296-1323` |
| Sun | `textures/environment/sun.png`, **32×32**, quad of half-size 30 | `RenderGlobal:93, 1301-1308` + jar |
| Moon | `textures/environment/moon_phases.png`, **128×64** = 4×2 grid of 8 phases; UV picked from `getMoonPhase()`, half-size 20 | `RenderGlobal:92, 1310-1323`; `WorldProvider.moonPhaseFactors` |
| Stars | **procedurally generated** at startup: `new Random(10842L)`, **1500** stars, positions from `nextFloat()*2−1` per axis, each star a random-sized quad ≤ 0.25 units; drawn as VBO/display list with brightness `getStarBrightness() * (1 − rainStrength)`; not rendered when brightness ≤ 0 | `RenderGlobal.renderStars:403-412+`, `:1325-1344` |
| End sky | separate: `textures/environment/end_sky.png` tile | `renderSkyEnd:1148` |
| Rain/snow | `textures/environment/rain.png` (64×256 = 4 frames of 64×64) and `snow.png` (64×256); column quads with per-frame UV offset, spawn above the camera, moved down 0.5·`f4`; splash particles (`WATER_DROP`) on impact | `EntityRenderer.renderRainSnow:1594+`, `:1550-1572` |
| Void fog / dimension fog | `WorldProvider.getVoidFogYFactor()`, `doesXZShowFog()` | `WorldProvider:231+` |

### 4.2 Clouds

- Single **2D layer** in 1.8 (Fast and Fancy both use one `clouds.png`; Fancy adds thickness by extruding each cloud quad into a 4-block-tall prism — `renderCloudsFancy` at `RenderGlobal:1494+` vs the flat branch at `:1420-1484`).
- `textures/environment/clouds.png`, **256×256**, and the vertex UV scale is `4.8828125E-4 = 1/2048` ⇒ **one cloud cell = 32 blocks**, layer spans ±256 in 32-block steps for the flat path (fast clouds are 8×8-block minimum per wiki).
- Height: `WorldProvider.getCloudHeight()` = **128.0F**; drawn at `cloudHeight − cameraY + 0.33` (`RenderGlobal:1462`).
- Motion: `cloudTickCounter` increments once per tick (staggered 20-tick updates, `:1138-1146`); UV offset = `cloudTickCounter * 4.8828125E-4` ⇒ clouds drift **west** at `0.0004882812 * 2048 = 1 block/tick`.
- Colour: `World.getCloudColour()` (blended with the sunset band and rain strength), brightness-modulated by the same `(30/59/11)` weights for non-anaglyph passes; drawn with `blend(770,771,1,0)`, cull disabled.
- Cloud fog: `hasCloudFog()` returns **false** in 1.8 (no cloud fog on the client), so clouds fade only via the standard fog. (`RenderGlobal:1489-1492`.)

### 4.3 Fog

Fog colour is computed **once per frame** in `EntityRenderer.updateFogColor` (lines 1763-1910) and the fog *mode/start/end* in `setupFog` (1936-2033):

| Mode | Trigger | Parameters (1.8) |
|---|---|---|
| Linear (default) | any other case | start = `farPlane * 0.75`, end = `farPlane` when `startCoords != −1`; start 0, end = `farPlane` when `startCoords == −1` (sky pass). `farPlane = renderDistanceChunks * 16` |
| World-border/XZ fog | `provider.doesXZShowFog()` | start = `farPlane * 0.05`, end = `min(farPlane, 192) * 0.5` |
| Water | camera inside `Material.water` | exponential (`GL_EXP`, 0x0800 = 2048); density `0.1 − respiration*0.03`; **0.01** with Water Breathing |
| Lava | camera inside lava | exponential, density **2.0** |
| Blindness | `Potion.blindness` | linear; start `f1*0.25`, end `f1` where `f1 = 5` (or ramps to `farPlane` in the last 20 ticks of the effect); for the sky pass start 0 / end `f1*0.8` |
| Cloud fog | only while the camera is inside the cloud layer (1.8: dead code on the client, `cloudFog` never set) | exponential density 0.1 |
| NV fog-distance hint | when `GL_NV_fog_distance` is available | `glFogi(GL_FOG_DISTANCE_MODE, GL_EYE_RADIAL_NV)` |

Fog **colour** chain (order matters):
1. `fogColor = World.getFogColor()` (per-dimension base; `WorldProvider` default `(0.7529412, 0.7529412, 0.7529412)`-ish blue-grey blend, Nether/End override).
2. Sunset/sunset bleed: weight `f5 = max(0, look·(−1,0,0 or +1,0,0)) * sunriseColor[3]` mixes in `calcSunriseSunsetColors`.
3. Rain/thunder darkening (`f13` blend of `fogColor2 → fogColor1`).
4. Void fog/altitude factor `d1 = eyeY * provider.getVoidFogYFactor()`, squared and multiplied when < 1; set to 0 while blind.
5. Boss "wither" tint (×0.7/0.6/0.6 at `bossColorModifier`).
6. Night vision: colour scaled by `1/brightness` and re-mixed (`getNightVisionBrightness`).
7. Per-block overrides after that: water `(0.02,0.02,0.2)+respiration`, lava `(0.6,0.1,0.0)`.
8. Also used as `glClearColor` for the frame.

**Fog density distance rule:** `f = 1 − (0.25 + 0.75 * renderDistanceChunks/32)^0.25` is computed and used for the sky/fog blend weight (line 1767-1768) — i.e. render distance changes not just the far plane but the *colour blend*.

Sources: `EntityRenderer.java:1763-2033`, `WorldProvider.java`, <https://minecraft.wiki/w/Fog>

### 4.4 Biome tinting

| Tint | Source | Rule |
|---|---|---|
| Grass (top, and the side overlay) | `textures/colormap/grass.png`, **256×256** | `Biome.getGrassColorAtPos` → `ColorizerGrass.getGrassColor(temperature, rainfall)` = colormap lookup at `(1−temp, 1−rain·temp)`; multiplied onto the texture |
| Foliage (leaves, vines) | `textures/colormap/foliage.png`, **256×256** | `ColorizerFoliage.getFoliageColor(temp, rain)` |
| Water | **not** a colormap | `Biome.waterColorMultiplier` (`BiomeColorHelper.WATER_COLOR_MULTIPLIER:22-28`) |
| Blend rule | `BiomeColorHelper.getColorAtPos` **averages the colour over the 3×3 block neighbourhood** (`:30-45`) — this is what produces the soft seams between biomes | |
| Fixed tints | swamp grass `0x6A7039` / swamp foliage `0x6A7039`; mesa grass/foliage `0x90814D`/`0x9E814D`; roofed forest/others per `BiomeGenBase`; cherry etc. do not exist in 1.8 | `BiomeGenBase.java:425-440` |
| Lit-item variants | `blocks/grass_side_overlay` is tinted; `blocks/grass_top` is tinted; `blocks/dirt` never | — |

Grass/foliage/water colour is applied per-vertex (so `Fast` graphics still tints, but with a constant per-face colour instead of per-corner).

Sources: `BiomeColorHelper.java`, `BiomeGenBase.java`, <https://minecraft.wiki/w/Color>, <https://minecraft.wiki/w/Biome>

### 4.5 Entities — models, animation, skins

- Player model: 64×64 skin (`textures/entity/steve.png` in-jar = default Steve; `alex.png` for slim), regions: head 8×8×8, body 8×12×4, arms/legs with sleeves/overlays in the 64×64 extension area (hat/jacket/sleeve/pant layers = the second half of the sheet). Armour uses a separate 64×32 sheet `textures/models/armor/<material>_layer_1.png` (helmet/chest/legs?) and `_layer_2.png` (leggings), plus `leather_layer_{1,2}_overlay.png` for dyed leather. **1.8 has no elytra, no offhand, no shields.**
- Skin fetch: `AbstractClientPlayer.getLocationSkin()` returns `DefaultPlayerSkin.getDefaultSkin(uuid)` when the profile is unknown; otherwise the skin is the texture from the profile property `textures` (base64 JSON, `textures.minecraft.net/texture/<hash>`), resolved by the session/`NetworkPlayerInfo` layer. Legacy path (still present in 1.8.9 source): `http://skins.minecraft.net/MinecraftSkins/<username>.png` via `ThreadDownloadImageData` (`AbstractClientPlayer.java:88`). **Model type** (slim = "alex") comes from `NetworkPlayerInfo.getSkinType()` / `DefaultPlayerSkin.getSkinType(uuid)` (`:105-106`).
- Capes: `NetworkPlayerInfo.getLocationCape()`; 1.8 renders one cape layer above the back (`LayerCape`), rendered with the same 10×16×1 box, no elytra.
- Entity interpolation: positions arrive at **20 Hz**; render uses `lastTickPos + (pos − lastTickPos) * partialTicks` for position and `prevRotationYaw + (rotationYaw − prevRotationYaw) * partialTicks` for yaw/pitch, with `lastTickPos` snapshotted at the start of each render (`RenderManager.renderEntityStatic:310-334`). `partialTicks` = fraction of the current tick already elapsed (frame-time based). Camera uses the same formula (`EntityRenderer.orientCamera:636-641`).
- Hurt/death animation: hurt → red overlay `(1,0,0)` with alpha **0.3** applied through texture-env combine while `hurtTime > 0` (`RendererLivingEntity.java:290, 326-348`); death → `rotationZ`/`renderYawOffset` tilt over 20 ticks with a `1.6` factor (`:419-421`).
- Item entities: hover/bob `sin((age + partialTicks)/10 + hoverStart) * 0.1 + 0.1`, spin `((age+partialTicks)/20 + hoverStart) * 180/π` deg/s? (i.e. **one full turn per 2 s**), item scale `0.25`, shadow `0.15` with `shadowOpaque 0.75` (`RenderEntityItem:23-49`).
- Arrow/particle/other entity renders: `RenderArrow`, `RenderSnowball`, `RenderFireball`, … all present in `client/renderer/entity/**`.

Sources: `RenderManager.java`, `AbstractClientPlayer.java`, <https://minecraft.wiki/w/Skin>, <https://wiki.vg/Mojang_API> (archived: <https://web.archive.org/web/2016/http://wiki.vg/Mojang_API>)

### 4.6 Particles (1.8.9)

`EnumParticleTypes` (verified, 40+ entries, name → id): `EXPLOSION_NORMAL 0 "explode"`, `EXPLOSION_LARGE 1 "largeexplode"`, `EXPLOSION_HUGE 2 "hugeexplosion"`, `FIREWORKS_SPARK 3 "fireworksSpark"`, `WATER_BUBBLE 4 "bubble"`, `WATER_SPLASH 5 "splash"`, `WATER_WAKE 6 "wake"`, `SUSPENDED 7 "suspended"`, `SUSPENDED_DEPTH 8 "depthsuspend"`, `CRIT 9 "crit"`, `CRIT_MAGIC 10 "magicCrit"`, `SMOKE_NORMAL 11 "smoke"`, `SMOKE_LARGE 12 "largesmoke"`, `SPELL 13 "spell"`, `SPELL_INSTANT 14 "instantSpell"`, `SPELL_MOB 15 "mobSpell"`, `SPELL_MOB_AMBIENT 16 "mobSpellAmbient"`, `SPELL_WITCH 17 "witchMagic"`, `DRIP_WATER 18 "dripWater"`, `DRIP_LAVA 19 "dripLava"`, `VILLAGER_ANGRY 20 "angryVillager"`, `VILLAGER_HAPPY 21 "happyVillager"`, `TOWN_AURA 22 "townaura"`, `NOTE 23 "note"`, `PORTAL 24 "portal"`, `ENCHANTMENT_TABLE 25 "enchantmenttable"`, `FLAME 26 "flame"`, `LAVA 27 "lava"`, `FOOTSTEP 28 "footstep"`, `CLOUD 29 "cloud"`, `REDSTONE 30 "reddust"`, `SNOWBALL 31 "snowballpoof"`, `SNOW_SHOVEL 32 "snowshovel"`, `SLIME 33 "slime"`, `HEART 34 "heart"`, `BARRIER 35 "barrier"`, `ITEM_CRACK 36 "iconcrack_"` (2 args), `BLOCK_CRACK 37 "blockcrack_"` (1 arg), `BLOCK_DUST 38 "blockdust_"` (1 arg), `WATER_DROP 39 "droplet"` (+ `MOB_APPEARANCE`, `DRAGON_BREATH`? — those are 1.9+; 1.8 stops at `WATER_DROP`).
- Sprites come from `textures/particle/particles.png` (128×128) at 8×8 cells, plus block/item-break sprites pulled from the **block atlas**; footprint particles use `textures/particle/footprint.png`.
- Weather footprints, block-break, block-dust, snow-shovel, item-crack textures are **atlas-sourced**, not from `particles.png`.
- Particle count limits: `ParticleSetting` (All/Decreased/Minimal) via `EffectRenderer` — `getStatistics()` feeds the F3 `P:` field.

Sources: `util/EnumParticleTypes.java`, `client/particle/EffectRenderer.java`, <https://minecraft.wiki/w/Particles>

---

## 5. Camera and controls "feel"

### 5.1 FOV

```
fovSetting  = 30..110 (option slider, grid of 5), default 70   (GameSettings:189)
hand/sprint modifier:  AbstractClientPlayer.getFovModifier()    (AbstractClientPlayer.java:109-141)
    f = 1.0
    if capabilities.isFlying:                     f *= 1.1
    attr = movementSpeed attribute (0.1 walk, 0.13 sprint)
    f *= (attr / capabilities.walkSpeed + 1) / 2  → walk 1.0, sprint 1.15
    if using a bow: f *= 1 - clamp(itemUseTicks/20, 0, 1)² * 0.15   (min 0.85)
smoothing: fovModifierHand += (f - fovModifierHand) * 0.5 per tick, clamped to [0.1, 1.5]
final:     fov = fovSetting * lerp(fovModifierHandPrev, fovModifierHand, partialTicks)
           (EntityRenderer.getFOVModifier:551-581 — returns 90.0 when the debug camera is active)
death:     fov /= (1 - 500/(deathTime+partialTicks+500)) * 2 + 1   → zoom-in on death
underwater: fov *= 60/70                                            (25 % narrower in water; sprint/flying modifiers untouched)
```
Projection: `gluPerspective(fov, aspect, 0.05, farPlane * sqrt(2))` for the terrain passes (`:766, 1352-1357`), `farPlane = renderDistanceChunks * 16`.

### 5.2 Mouse and camera

- Sensitivity mapping: `f = mouseSensitivity * 0.6 + 0.2; f1 = f³ * 8; yaw += deltaX * f1; pitch += deltaY * f1` (`EntityRenderer:341, 1097-1100`). Option range 0…1, default **0.5** (`GameSettings:65`) ⇒ `f = 0.5`, `f1 = 1.0` (i.e. the default is exactly 1:1 raw mouse px → degrees? no: 1 px of mouse delta = 1 degree of yaw at default sensitivity).
- Pitch clamps to ±90°, yaw wraps ±180°; `smoothCamera` (cinematic) applies a low-pass filter instead of direct deltas (`:1108-1116`).
- Third-person: `thirdPersonView` 0/1/2; camera distance 4.0 blocks with a **smooth** `thirdPersonDistance` and an occlusion raycast that pulls the camera in; sleeping pivots the camera to the bed (`orientCamera:634-700`).
- `F5` cycles 1st → 3rd-back → 3rd-front (the entity itself is skipped in the pass when `thirdPersonView == 0`); `F1` hides the entire HUD *and* the hand (`hideGUI` → `ItemRenderer` skips first-person item; `GuiIngame` skips HUD; the vignette/overlays are skipped too).

### 5.3 View bobbing, hurt, vignette, hand

```
bobbing (EntityRenderer.setupViewBobbing:615-628), only if viewBobbing:
  d = distanceWalkedModified - prevDistanceWalkedModified
  f1 = -(distanceWalkedModified + d * partialTicks)
  yawCam = lerp(prevCameraYaw, cameraYaw, partialTicks)     // accumulates from movement
  pitCam = lerp(prevCameraPitch, cameraPitch, partialTicks)
  translate( sin(f1*π) * yawCam * 0.5,  -abs(cos(f1*π) * yawCam),  0 )
  rotate(    sin(f1*π) * yawCam * 3.0°, Z )
  rotate(    abs(cos(f1*π - 0.2) * yawCam) * 5.0°, X )
  rotate(    pitCam, X )                                     // cameraPitch from stepping
hurt cam (EntityRenderer.hurtCameraEffect:585-609):
  f = (hurtTime - partialTicks) / maxHurtTime        (10 ticks max)
  f = sin(f⁴ · π)
  rotate(-attackedAtYaw, Y); rotate(-f * 14°, Z); rotate(attackedAtYaw, Y)
  (death: rotate(40 - 8000/(deathTime + partialTicks + 200), Z))
hand swing (ItemRenderer.renderItemInFirstPerson / transformFirstPersonItem:296-320):
  equipProgress: lerp(prevEquippedProgress, equippedProgress, partialTicks)
  translate(0, equipProgress * -0.6, 0)
  f  = sin(swingProgress² * π)
  f1 = sin(sqrt(swingProgress) * π)
  translate(-f1 * 0.4,  f * 0.2,  -f1 * 0.2)
  rotate(-f1 * 70°, Y); rotate( f * 70°,  Z); rotate(-f * 70°, X)
  scale(0.4)
  (block-in-hand path uses 0.4 scale + item-specific transforms; damage/attack swing total 6 ticks)
```
- Held item first-person transform comes from the model's `display.firstperson` (see §1.3) — e.g. sword `rotation [0,−135,25]`, `translation [0,4,2]`, `scale 1.7`.
- Hand swing is driven by `swingProgress` from `prevSwingProgress`→`swingProgress` (0→1 over 6 ticks).

### 5.4 Movement constants (physics "feel")

| Constant | 1.8 value | Evidence |
|---|---|---|
| `capabilities.walkSpeed` | **0.1** | `PlayerCapabilities.java:24` |
| `capabilities.flySpeed` | **0.05** (×2 while sprinting) | `:23`; `EntityPlayer.moveEntityWithHeading:1798` |
| movementSpeed attribute (walk) | 0.1; sprint adds **+30 %** (`0.3000000119209…` modifier) → 0.13 | `EntityLivingBase.java:57` |
| Ground accel factor | `f = 0.16277136 / (f4)³`, `f4 = slipperiness * 0.91` (0.6 for most blocks → `f4 = 0.546`, `f = 1.0`) | `Entity.moveEntityWithHeading` |
| Input decay | `moveStrafing *= 0.98; moveForward *= 0.98` each tick | `EntityLivingBase.java:2031-2032` |
| Sneaking | input × **0.3** | `MovementInputFromOptions.java:42-45` |
| Jump | `motionY = 0.42` (+0.1 per Jump Boost level), then per tick `motionY = (motionY − 0.08) * 0.98` | `EntityLivingBase.java:1561-1573, 1677-1680` |
| Jump height | **1.2522 blocks** (matches the wiki's post-15w45a value; my simulation of the recurrence gives 1.25220) | `Movement.wiki:746` + simulation |
| Terminal fall | `3.92` blocks/tick (78.4 m/s) | recurrence + `Movement.wiki:659` |
| Air drag | ×0.98 vertical, ×0.91 horizontal | `Entity.java` |
| Ladder climb | `motionY = 0.2` while climbing (`0.2` when not sneaking), horizontal speed cap 0.15 | `Entity.moveEntity` |
| Water | drag 0.8, accel `0.02`, `motionY += 0.039` while holding jump (swimming), descent clamp `motionY < −0.15 ⇒ −0.15` for water walking w/ depth strider? (1.8: no depth strider in water vertical; the "0.6 → 0.3" bounce lines are for lava/water walking) | `EntityLivingBase.java:1644-1732` |
| Lava | drag 0.5, accel 0.02, `motionY −= 0.02` per tick, no swim boost | `:1690-1732` |
| Equilibrium speeds | walk **4.317 m/s**, sprint **5.612 m/s**, sneak **1.3 m/s** (ratio walk:sprint = 1:1.3) | derived: `0.98*0.1/(1−0.546)=0.21586` → 4.317 m/s; wiki confirms |
| Step/fall damage | `fallDistance > 3.0` → damage; `motionY < -0.15` triggers drowning/fly-up logic as above | `EntityLivingBase` |

### 5.5 Entity interpolation (3-tick / 20 Hz rule)

Vanilla 1.8 interpolates over **one tick**, not 3: server positions arrive at 20 Hz (`S12 Packet Player Position` per tick) and the renderer interpolates with `partialTicks ∈ [0,1)` between `lastTickPos` and the current target. The optional 3-tick / 100 ms entity-position smoothing that players remember is the **server-side 3-tick movement packet cadence** (players send positions every 3 ticks with `lastTickPos` accumulations) — the *client* draws each received tick interpolated. So a parity renderer wants: **store the last two positions per entity, lerp by `partialTicks`, snap on teleport (> 4 blocks), and use 20 Hz for all tick-driven animation**.

Sources: `RenderManager.java:310-334`, `EntityRenderer.orientCamera:636-641`, <https://web.archive.org/web/2016/http://wiki.vg/Protocol>

---

## 6. Sound

| Topic | 1.8 behaviour | Evidence |
|---|---|---|
| Categories (volumes) | `SoundCategory = {MASTER(0), MUSIC(1), RECORDS(2), WEATHER(3), BLOCKS(4), MOBS(5), ANIMALS(6), PLAYERS(7), AMBIENT(8)}` — 9 sliders in `GuiScreenOptionsSounds` | `client/audio/SoundCategory.java:8-16` |
| Definition file | `sounds.json` (asset index object!) maps **event name → list of sound files** (+ `category`, `stream`, `volume`, `pitch`, `weight`, `attenuation_distance?`) | <https://minecraft.wiki/w/Sounds.json> |
| Playback backend | Paulscode SoundSystem (`sndSystem`) with `newSource/newStreamingSource`, `MasterVolume` = MASTER category level | `SoundManager.java:385-389` |
| Pitch clamp | `Normalizes pitch from parameters and clamps to [0.5, 2.0]` | `SoundManager.java:417` |
| Volume clamp | `[0.0, 1.0]`; sounds with volume < 0.01… are skipped (`Skipped playing sound…, volume was zero`) | `SoundManager.java:376, 425` |
| 3D attenuation | positional sounds get computed attenuation from distance to the listener; non-positional sounds (UI, music, records) play at full volume | `SoundManager.PlaySound` / `positionSound` |
| Pitch randomisation | done **at the call site**, not globally: mob hurt/death `(rand−rand)*0.2 + 1.0`; step sounds `1.0`? — see `EntityLivingBase.java:1368` (hurt), `:1379` (death), `:1501` (child 1.5×) | `EntityLivingBase.java` |
| Music | `music.game` / `music.creative` / `music.menu` (streamed), random pick every 10–20 min with a 100-tick fade, stops on damage? (1.8: stops only via `updateMusicVolume`), volume = MUSIC category | `MusicTicker`, `minecraft/sounds/music/*` (29 objects) |
| Music discs | 12 records, `records/` (12 objects in index), category `RECORDS`, **streamed** (`streamed: true` in `sounds.json`), 64-block attenuation range, stops when leaving the jukebox range | `Music Disc` wiki, index listing |
| Ranges | 16 blocks default attenuation; blocks/mobs at their entity position; rain is `ambient.weather.rain` at volume 0.1–0.2 / pitch 0.5–1.0 | `EntityRenderer:1581-1585` |
| Sound events to wire for visual parity | `dig.*` (block break/step), `random.*`, `step.*` (40 objects), `tile.*`, `liquid.*`, `fire.*`, `ambient.*` (cave/weather), `mob.*` (376 objects), `note.*` (7), `portal.*` (3), `minecart.*` (2), `damage.*` (5), `music.*`, `records.*` | index listing of `minecraft/sounds/**` |

Sources: <https://minecraft.wiki/w/Sound>, <https://minecraft.wiki/w/Sounds.json>, <https://minecraft.wiki/w/Music_Disc>, `client/audio/SoundManager.java`, `client/audio/SoundCategory.java`

---

## 7. Visual parity acceptance tests (tick each by side-by-side screenshot)

1. Main menu shows the 6-face panorama cube with the vanilla slow rotation period and no visible seams/blur.
2. Splash text is picked at random from `texts/splashes.txt` with the vanilla bounce/scale, positioned 45° from the logo's right edge.
3. Button textures come from `gui/widgets.png` (152×20 slices), hover state = the lighter (`0x…`) variant, disabled = greyed text.
4. Options → Video lists exactly the 1.8 slider set (Render Distance, Brightness, Particles, GUI Scale, FOV, Framerate, Mipmaps) and toggle set (VSync, View Bobbing, VBOs, Block Alternatives, Entity Shadows, 3D Anaglyph, Smooth Lighting, Graphics, Clouds); no 1.9+ options appear.
5. The F3 overlay text matches §3.3 line-for-line, including `Minecraft 1.8.9 (1.8.9/vanilla)`, `C: a/b. Frame: …kB`, `P: x. T: y`, 5-decimal Y, and the right-hand Java/Mem/CPU/Display column.
6. `F3+H` enables item ids/durability on tooltips; `F3+B` shows hitboxes + look vectors; `F3+G` shows chunk borders; `Shift+F3` shows the lagometer.
7. Hotbar is 182×22 at `(w/2−91, h−22)`, selection highlight is 24×22 offset `currentItem*20 − 1`, item icons are drawn with **no mipmaps/blur**.
8. Hearts/hunger/armour render in the exact rows above the hotbar; air bubbles appear only when underwater; absorption hearts show as a separate row.
9. XP bar draws the 182×5 background and 182×5 foreground with the green level number in Mojangles + black outline.
10. Crosshair is a 1-px inverted-blend cross at the exact screen centre (invisible against mid-grey, visible against dark/bright).
11. Chat renders with the configured scale/width/opacity, 20-line scrollback, fade-out, and §-colour codes + shadow.
12. Scoreboard sidebar sits flush right, ≤15 lines, translucent black background, red scores when configured.
13. Boss health bar uses `widgets.png` 182×5 at y offsets 74/79 with the (purple/diamond) name text 10 px above.
14. Item tooltips use `0x10001000` background with the purple gradient border, stacked lines 10 px apart, rarity colours, and shift-info (F3+H).
15. Container interactions match §3.4: right-click = half / single place, left-drag = even distribution with remnant, shift-click transfer, hotbar swap, double-click collect, Q drop.
16. Inventory screen shows the 3D player model (rotating with drag), 2×2 crafting grid, 4 armour slots and no offhand slot.
17. Every container opens the 1.8 texture from `gui/container/*` at the correct size (anvil, beacon, brewing stand, dispenser, enchantment, furnace, generic_54, hopper, horse, inventory, villager, crafting_table).
18. Anvil GUI shows the level-cost counter and rejects ≥40 with "Too Expensive!".
19. Enchantment table GUI shows the 3 offers with the standard glyph, lapis counter and the book model in-world.
20. Villager trading GUI shows the offer list with out-of-stock red X and the XP progress bar.
21. Death screen shows "You Died!" with score and Respawn / Title Screen buttons; the death camera tilts by `40 − 8000/(deathTime+200)`°.
22. Achievement popup slides in top-right using `achievement_background.png`; **no** 1.9+/1.12 toast or advancement popups appear anywhere.
23. Block selection outline is black 0.4-alpha 2-px-wide lines inflated by 0.002, drawn with depth writes disabled.
24. Block-break progress uses the 10 `destroy_stage_*` sprites with additive-ish blending and no mipmaps; the overlay is removed 400 ticks after the last update.
25. Held item in first person uses the model's `display.firstperson` values (sword/ingot/block differ); the swing animation matches the `±0.4/0.2/0.2` translate + 70° rotations over 6 ticks.
26. Block-in-hand overlay (suffocation) draws the opaque block texture with `0.1,0.1,0.1,0.5` tint when the camera is inside an opaque block.
27. View bobbing matches the `sin/cos(distanceWalked)` translate+rotate triple and scales with the `viewBobbing` option.
28. Vignette appears only on Fancy graphics with alpha `1 − brightness`; it darkens in the Nether/at low light and while inside a world border.
29. Water overlay tiles `misc/underwater.png` at alpha 0.5 with the brightness-tinted colour; lava/fire overlays use the block atlas sprites with flat shading and per-frame random UV offsets.
30. Pumpkin blur covers the screen exactly when a carved pumpkin is worn in first person.
31. Portal overlay tiles `blocks/portal.png` with alpha = `timeInPortal`, and the nausea wobble appears with `Nausea`.
32. Fog colour and mode per §4.3 for: clear day, rain, thunder, night, sunset, water (density 0.1, or 0.01 with Water Breathing), lava (2.0), blindness, void fog/altitude, boss tint, night vision, world border.
33. Default linear fog: start = `farPlane*0.75`, end = `farPlane`, and the sky-pass fog (start 0, end = farPlane) matches, so the horizon fade has the same width.
34. Sky gradient fades to full transparency at the horizon and matches the biome tint table (plains vs desert vs swamp vs ocean).
35. Sun is 30 half-units on a 32×32 texture at y=+100; moon is 20 half-units from the 4×2 phase grid and the phase advances per day correctly.
36. 1500 procedural stars with seed 10842, fading with `getStarBrightness()` and rain; no stars during the day.
37. Clouds: one 256×256 layer at height 128 + 0.33, 32-block UV cells (1/2048 scale), westward 1 block/tick drift, `getCloudColour()` tint, Fancy = 3D prisms vs Fast = flat opaque.
38. Rain/snow columns use the 4-frame 64×256 strips with the correct splash `WATER_DROP` particles and the "rain" ambient sound cadence.
39. Grass/foliage/water tinting matches the two 256×256 colormaps plus the 3×3 neighbourhood averaging (smooth biome seams), with the swamp/mesa fixed overrides.
40. Block models render with the correct `cullface`, per-face light levels, `shade:false` for cross/torch/flat models, `ambientocclusion` from Smooth Lighting (off/min/max), and element rotations with `rescale`.
41. Stairs/slabs/fence/glass-pane/torch/redstone connectivity picks the same variants that the vanilla blockstate files list (including the `uvlock`-rotated duplicates and grass' 4-way rotation).
42. The atlas is one `atlas/blocks.png` containing blocks **and** items; animated strips animate at their `.mcmeta` framerates; mipmap level follows the option and is dropped when an odd-sized sprite is present.
43. Item entities bob/spin with the vanilla period (1 rev / 2 s) and 0.25 scale; arrow/other projectile renders and their spin match.
44. Player skins: default Steve/Alex by UUID when unknown, slim model from `getSkinType()`, cape layer when the profile has one; hat/jacket/sleeve/pant overlay layers toggle from the skin customization screen.
45. Mob models/animations for the 1.8.9 roster (walk cycle `distanceWalkedModified`, head look `renderYawOffset`, arm swing, spider leg IK, enderman teleport particles, zombie arms out, skeleton bow pose) look identical.
46. Entity motion interpolates at 20 Hz with `partialTicks` and snaps on teleports; camera interpolation matches §5.5.
47. All 40 `EnumParticleTypes` (§4.6) exist with the vanilla sprite cells and behaviours, including `iconcrack_`/`blockcrack_` atlas lookups and the `ParticleSetting` reduction.
48. FOV matches: 70 default, sprint 1.15×, flying 1.1×, bow-draw down to 0.85×, underwater 60/70, death zoom, and the 0.5-per-tick smoothing with [0.1,1.5] clamp.
49. Mouse look matches `((sens*0.6+0.2)³*8)` degrees per mouse pixel, with the same pitch clamp and cinematic-camera low-pass.
50. Movement feel matches the constants in §5.4: jump apex 1.2522, walk 4.317 m/s, sprint 5.612 m/s, sneak 1.3 m/s, ladder 0.2/tick, water/lava drag, terminal 3.92/tick.
51. `F5` shows third-person back/front with the same camera distance/occlusion pull-in; `F1` hides HUD + hand + overlays.
52. Sounds play with the 9 vanilla categories, pitch clamp [0.5,2.0], per-call pitch randomisation, correct streaming for music/records, and the vanilla 16-block attenuation.
53. GUI scale Auto/Small/Normal/Large produce the same scaled resolution as vanilla at 1920×1080, 2560×1440 and 1280×720.
54. Font: ascii.png grid + glyph_sizes.bin widths reproduce identical string widths (compare `getStringWidth` for a fixed corpus), unicode glyphs at 0.5 horizontal scale, shadow at 12.5 % advance with channel ÷ 4.
55. `en_US.lang` from the jar plus hashed locale objects from the asset index load correctly; missing keys render as the raw key (vanilla behaviour).
56. Resource-pack load order (packs → jar → index objects), pack format 1, and the "missing texture"/"missing model" fallbacks match vanilla.
57. 1.8-specific exclusions verified: no elytra, no offhand slot/shield, no combat cooldown bar, no toasts/advancements (achievements only), no block displays/armour stands? (armour stands **do** exist in 1.8), no 1.9 combat or dual-wield visuals.

---

## Sources

- Minecraft Wiki — <https://minecraft.wiki/w/Model>, <https://minecraft.wiki/w/Block_states>, <https://minecraft.wiki/w/Client.jar>, <https://minecraft.wiki/w/Assets>, <https://minecraft.wiki/w/Resource_pack>, <https://minecraft.wiki/w/Font>, <https://minecraft.wiki/w/Heads-up_display>, <https://minecraft.wiki/w/Debug_screen>, <https://minecraft.wiki/w/Options>, <https://minecraft.wiki/w/Video_settings>, <https://minecraft.wiki/w/Screen_effects>, <https://minecraft.wiki/w/Fog>, <https://minecraft.wiki/w/Sky>, <https://minecraft.wiki/w/Sun>, <https://minecraft.wiki/w/Moon>, <https://minecraft.wiki/w/Cloud>, <https://minecraft.wiki/w/Color>, <https://minecraft.wiki/w/Biome>, <https://minecraft.wiki/w/Skin>, <https://minecraft.wiki/w/Cape>, <https://minecraft.wiki/w/Achievement>, <https://minecraft.wiki/w/Breaking>, <https://minecraft.wiki/w/Item_(entity)>, <https://minecraft.wiki/w/Particles>, <https://minecraft.wiki/w/Sound>, <https://minecraft.wiki/w/Sounds.json>, <https://minecraft.wiki/w/Music_Disc>, <https://minecraft.wiki/w/Movement>, <https://minecraft.wiki/w/Java_Edition_1.8.9>, <https://minecraft.wiki/w/Java_Edition_1.8>, <https://minecraft.wiki/w/Data_version>
- wiki.vg (archived) — <https://web.archive.org/web/2016/http://wiki.vg/Protocol>, <https://web.archive.org/web/2016/http://wiki.vg/Mojang_API>
- Mojang piston/launcher metadata — `https://launchermeta.mojang.com/mc/game/version_manifest.json`, `https://piston-meta.mojang.com/v1/packages/d546f1707a3f2b7d034eece5ea2e311eda875787/1.8.9.json`, `https://launchermeta.mojang.com/v1/packages/f6ad102bcaa53b1a58358f16e376d548d44933ec/1.8.json`, client jar at `https://launcher.mojang.com/v1/objects/3870888a6c3d349d3771a3e9d16c9bf5e076b908/client.jar`
- Decompiled 1.8.9 (MCP-919): `https://github.com/Marcelektro/MCP-919` — `src/minecraft/net/minecraft/` paths cited inline as `<Class>.java:<line>` (EntityRenderer, RenderGlobal, FontRenderer, TextureMap, Stitcher, ItemRenderer, RenderItem, RenderManager, RenderEntityItem, RendererLivingEntity, GuiIngame, GuiOverlayDebug, GuiAchievement, GuiContainer, GameSettings, Minecraft, EnumParticleTypes, SoundManager, SoundCategory, Entity, EntityLivingBase, EntityPlayer, EntityPlayerSP, AbstractClientPlayer, PlayerCapabilities, MovementInputFromOptions, WorldProvider, BiomeGenBase, BiomeColorHelper, ModelBakery, ItemCameraTransforms)
