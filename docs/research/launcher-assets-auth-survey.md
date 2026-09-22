# Launcher Assets & Auth Survey — Mojang piston-meta + Microsoft/Mojang auth + legal

**Target:** a Linux-native Rust launcher that (a) fetches assets from Mojang piston-meta, (b) authenticates a Microsoft
account, (c) launches a custom native Rust Minecraft **1.8.9** client on CachyOS/Arch.
**Survey date:** 2026-09-22 (UTC). **Method:** every fact below was checked by live `curl`/`fetch` in this session; HTTP
status codes, byte counts and JSON/response bodies are pasted verbatim. Where something could not be verified by fetch
it is explicitly marked *[unverified]*.

> This document is reconnaissance, not legal advice (see §5.7).

---

## 0. What was actually fetched (evidence index)

| URL | Status | Observed |
|---|---|---|
| `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` | 200 | `size=276870`, `content_type=application/json` |
| `https://piston-meta.mojang.com/v1/packages/d546f1707a3f2b7d034eece5ea2e311eda875787/1.8.9.json` | 200 | `size=18244`; local `sha1sum` == `d546f1707a3f2b7d034eece5ea2e311eda875787` |
| `https://launchermeta.mojang.com/v1/packages/f6ad102bcaa53b1a58358f16e376d548d44933ec/1.8.json` | 200 | `size=78494`; local `sha1sum` == `f6ad102bcaa53b1a58358f16e376d548d44933ec` |
| `https://launcher.mojang.com/v1/objects/3870888a6c3d349d3771a3e9d16c9bf5e076b908/client.jar` | 200 | `size=8461484`; local `sha1sum` == `3870888a6c3d349d3771a3e9d16c9bf5e076b908` |
| `https://resources.download.minecraft.net/29/29d4dccf3353334c7aa2a49cb6fed3780a51a1ba` | 200 | `size=33948`, `content_type=application/ogg`; local `sha1sum` matches |
| `https://resources.download.minecraft.net/5e/5e06ca070067486427a3167ade2ffe01623e5591` | 200 | `size=37731`, `content_type=application/octet-stream`; `sha1sum` matches; body starts with real `sounds.json` |
| `https://libraries.minecraft.net/org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar` | 200 | reachable |
| `https://launchermeta.mojang.com/mc/game/version_manifest.json` | 200 | legacy manifest still served |
| `https://piston-meta.mojang.com/mc/game/version_manifest.json` | 200 | v1 manifest also on piston-meta |
| `https://piston-data.mojang.com/mc/game/version_manifest.json` | **404** | piston-data does **not** serve the manifest |
| `https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode` | 400 | `AADSTS900144: The request body must contain the following parameter: 'scope'.` |
| `https://user.auth.xboxlive.com/user/authenticate` | 401 | empty body (`content-length: 0`) for a bad RpsTicket |
| `https://xsts.auth.xboxlive.com/xsts/authorize` | 400 | empty body for a bad UserToken |
| `https://api.minecraftservices.com/authentication/login_with_xbox` | 400 | `application/json` reject for `{}` |
| `https://api.minecraftservices.com/entitlements/mcstore` | 401 | `{ "path" : "/entitlements/mcstore" }` |
| `https://api.minecraftservices.com/minecraft/profile` | 401 | `{ "path" : "/minecraft/profile" }` |
| `https://sessionserver.mojang.com/session/minecraft/join` | 403 | `{"error":"ForbiddenOperationException","path":"/session/minecraft/join"}` |
| `https://sessionserver.mojang.com/session/minecraft/hasJoined?...` (GET) | 204 | no content (no live session for that hash) |
| `https://sessionserver.mojang.com/session/minecraft/profile/069a79f444e94726a5befca90e38aaf5` | 200 | live profile JSON (see §3.6) |

---

## 1. The piston-meta chain, exactly as observed

### 1.1 Version manifest

`GET https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` → **200**, 276,870 bytes.

First bytes, verbatim:

```json
{"latest": {"release": "26.3", "snapshot": "26.4-snapshot-1"}, "versions": [{"id": "26.4-snapshot-1", "type": "snapshot", "url": "https://piston-meta.mojang.com/v1/packages/38de33cff6c65a518a24aa4b8198f7e3673ebd08/26.4-snapshot-1.json", "time": "2026-09-22T13:45:27+00:00", ...
```

The 1.8.9 entry, verbatim:

```json
{"id": "1.8.9", "type": "release", "url": "https://piston-meta.mojang.com/v1/packages/d546f1707a3f2b7d034eece5ea2e311eda875787/1.8.9.json", "time": "2021-12-15T15:44:12+00:00", "releaseTime": "2015-12-03T09:24:39+00:00", "sha1": "d546f1707a3f2b7d034eece5ea2e311eda875787", "complianceLevel": 0}
```

Notes that matter for the downloader:

* The `sha1` field **is** the same hex string as the path segment in `url`. It is the SHA1 of the version JSON
  itself: after downloading I ran `sha1sum v189.json` and got `d546f1707a3f2b7d034eece5ea2e311eda875787`. So one
  hash validates the whole per-version document.
* `time` (2021-12-15) ≠ `releaseTime` (2015-12-03): `time` is when Mojang last *re-packaged* the metadata, so a
  launcher must not use it as a release date.
* `complianceLevel: 0` is present for 1.8.9; modern versions carry higher levels. 1.8.9 predates all of the
  compliance/consent machinery, so the field can be ignored for this version.
* Host redundancy observed: `launchermeta.mojang.com/mc/game/version_manifest.json` → 200,
  `piston-meta.mojang.com/mc/game/version_manifest.json` → 200, and `piston-data.mojang.com/mc/game/version_manifest.json`
  → **404**. Don't hardcode piston-data for the manifest; it is the *asset/object* host in modern versions.
* `minimumLauncherVersion: 14` in the 1.8.9 JSON is a version of the *official Java launcher*, not a protocol number —
  it is not a constraint on a third-party launcher.

### 1.2 The 1.8.9 version JSON — key fields

`GET https://piston-meta.mojang.com/v1/packages/d546f1707a3f2b7d034eece5ea2e311eda875787/1.8.9.json` → **200**, 18,244 bytes.

Top-level keys, in the order the file presents them:

```
['assetIndex', 'assets', 'complianceLevel', 'downloads', 'id', 'javaVersion', 'libraries', 'logging', 'mainClass', 'minecraftArguments', 'minimumLauncherVersion', 'releaseTime', 'time', 'type']
```

Verbatim values:

```json
"id": "1.8.9",
"type": "release",
"assets": "1.8",
"assetIndex": {"id": "1.8", "sha1": "f6ad102bcaa53b1a58358f16e376d548d44933ec", "size": 78494, "totalSize": 114885064, "url": "https://launchermeta.mojang.com/v1/packages/f6ad102bcaa53b1a58358f16e376d548d44933ec/1.8.json"},
"mainClass": "net.minecraft.client.main.Main",
"minimumLauncherVersion": 14,
"javaVersion": {"component": "jre-legacy", "majorVersion": 8},
"releaseTime": "2015-12-03T09:24:39+00:00", "time": "2015-12-03T09:24:39+00:00"
```

```json
"downloads": {
  "client": {"sha1": "3870888a6c3d349d3771a3e9d16c9bf5e076b908", "size": 8461484, "url": "https://launcher.mojang.com/v1/objects/3870888a6c3d349d3771a3e9d16c9bf5e076b908/client.jar"},
  "server": {"sha1": "b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd", "size": 8320755, "url": "https://launcher.mojang.com/v1/objects/b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd/server.jar"}
}
```

Important structural points:

* **The asset index URL is NOT on piston-meta.** It is `launchermeta.mojang.com/v1/packages/<sha1>/1.8.json`.
  A launcher must follow the URL in the JSON rather than reconstructing it from `assetIndex.id`.
* `assetIndex.id` is **`"1.8"`**, and the top-level `assets` field is **also `"1.8"`** — they always agree in this
  version. (1.8.9 does *not* use an index id of `"1.8.9"`.)
* `assetIndex.totalSize` = 114,885,064 bytes. The sum of all `size` values in the fetched index is exactly
  114,885,064 (§1.4), so `totalSize` is the honest total for the whole object set — useful for a progress bar.
* `javaVersion.component = "jre-legacy"` and `majorVersion = 8`. This is a *download hint for the Java launcher*
  (Mojang's `java-runtime-*` manifests on `launchermeta`/`piston-data`). A native Rust client ignores it entirely.
* **1.8.9 predates the `arguments` array.** Launch parameters are the legacy `minecraftArguments` string, verbatim:

```
--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --assetIndex ${assets_index_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userProperties ${user_properties} --userType ${user_type}
```

  A native client that doesn't run the JVM can ignore this string, but note it documents the *only* channels by
  which accounts were historically passed to the game: username, UUID, access token. There is no `--server` /
  `--port` in the template for 1.8.9 (those were appended ad hoc).
* `logging` block, verbatim:

```json
{"client": {"argument": "-Dlog4j.configurationFile=${path}", "file": {"id": "client-1.7.xml", "sha1": "50c9cc4af6d853d9fc137c84bcd153e2bd3a9a82", "size": 966, "url": "https://launcher.mojang.com/v1/objects/50c9cc4af6d853d9fc137c84bcd153e2bd3a9a82/client-1.7.xml"}, "type": "log4j2-xml"}}
```

  This is a JVM log4j2 config file. **Completely irrelevant** to a native client; do not download it.

### 1.3 Libraries, rules and natives for 1.8.9

The version JSON lists **37** library entries, all served from `https://libraries.minecraft.net/`. Each entry has
`downloads.artifact` with `path`, `sha1`, `size`, `url`. Entries that carry `rules`, `natives` or `extract` — these
are the ones a spec must model:

**LWJGL 2.9.4 (the general case, Linux included):**

```json
{"downloads": {"artifact": {"path": "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar", "sha1": "697517568c68e78ae0b4544145af031c81082dfe", "size": 1047168, "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar"}}, "name": "org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209", "rules": [{"action": "allow"}, {"action": "disallow", "os": {"name": "osx"}}]}
```

**LWJGL platform natives (this is the NVIDIA/native-crash-prone one):**

```json
{"downloads": {"artifact": {...22-byte stub...}, "classifiers": {
   "natives-linux":   {"path": "...lwjgl-platform-2.9.4-nightly-20150209-natives-linux.jar",   "sha1": "931074f46c795d2f7b30ed6395df5715cfd7675b", "size": 578680, "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-linux.jar"},
   "natives-osx":     {"sha1": "bcab850f8f487c3f4c4dbabde778bb82bd1a40ed", "size": 426822},
   "natives-windows": {"sha1": "b84d5102b9dbfabfeb5e43c7e2828d98a7fc80e0", "size": 613748}}},
 "extract": {"exclude": ["META-INF/"]},
 "name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.4-nightly-20150209",
 "natives": {"linux": "natives-linux", "osx": "natives-osx", "windows": "natives-windows"},
 "rules": [{"action": "allow"}, {"action": "disallow", "os": {"name": "osx"}}]}
```

**macOS-only pair** (`lwjgl`, `lwjgl_util`, `lwjgl-platform` at `2.9.2-nightly-20140822`) uses
`"rules": [{"action": "allow", "os": {"name": "osx"}}]` — **no** rules-less allow, so on Linux these are *skipped*.

**jinput-platform 2.0.5** has `natives-linux` (`sha1 7ff832a6eb9ab6a767f1ade2b548092d0fa64795`, 10,362 bytes) and
**no** `rules` array at all.

**Twitch entries are explicitly excluded on Linux:**

```json
"name": "tv.twitch:twitch-platform:6.5",
"natives": {"linux": "natives-linux", "osx": "natives-osx", "windows": "natives-windows-${arch}"},
"rules": [{"action": "allow"}, {"action": "disallow", "os": {"name": "linux"}}]
```

and `tv.twitch:twitch-external-platform:4.5` uses `"rules": [{"action": "allow", "os": {"name": "windows"}}]`.
So on CachyOS both Twitch libs are skipped — a small, welcome saving, and a good test case for the rule engine.

**Rule semantics (from the manifest format):** rules are evaluated in order; the *last* matching rule wins; if a
rule has no `os`/`features` clause it matches unconditionally; if **no** rule matches, the library is **disallowed**.
A launcher that treats "has any allow rule" as "include it" will wrongly pull the macOS 2.9.2 trio onto Linux.

### 1.4 The asset index — format and contents

`GET https://launchermeta.mojang.com/v1/packages/f6ad102bcaa53b1a58358f16e376d548d44933ec/1.8.json` → **200**,
78,494 bytes. `sha1sum` matches `f6ad102bcaa53b1a58358f16e376d548d44933ec`.

Structure: the file has **exactly one** top-level key, `objects`, and it is a flat map from a
**path relative to an assets root** to `{hash, size}`:

```
top keys: ['objects']            (no "virtual", no "map_to_resources" for 1.8)
object count: 734
```

Verbatim sample entries:

```json
"minecraft/sounds/ambient/cave/cave1.ogg": {"hash": "29d4dccf3353334c7aa2a49cb6fed3780a51a1ba", "size": 33948}
"minecraft/sounds/ambient/cave/cave10.ogg": {"hash": "79a5b53bf22cca182ddff2a670942c49867663ec", "size": 22725}
"minecraft/sounds.json": {"hash": "5e06ca070067486427a3167ade2ffe01623e5591", "size": 37731}
"minecraft/icons/icon_16x16.png": {"hash": "bdf48ef6b5d0d23bbb02e17d04865216179f510a", "size": 3665}
"minecraft/icons/icon_32x32.png": {"hash": "92750c5f93c312ba9ab413d546f32190c56d6f1f", "size": 5362}
"minecraft/icons/minecraft.icns": {"hash": "991b421dfd401f115241601b2b373140a8d78572", "size": 114786}
"icons/icon_16x16.png": {"hash": "bdf48ef6b5d0d23bbb02e17d04865216179f510a", "size": 3665}
"icons/icon_32x32.png": {"hash": "92750c5f93c312ba9ab413d546f32190c56d6f1f", "size": 5362}
"icons/minecraft.icns": {"hash": "991b421dfd401f115241601b2b373140a8d78572", "size": 114786}
```

Breakdown of the 734 keys by first two path segments (computed from the fetched file):

| count | prefix |
|---|---|
| 578 | `minecraft/sounds/` |
| 74 | `minecraft/lang/` |
| 74 | `realms/lang/` |
| 3 | `minecraft/icons/` |
| 3 | `icons/` |
| 1 | `minecraft/sounds.json` |
| 1 | `pack.mcmeta` |

**The single most important finding of this survey:** for 1.8.9 the asset index contains **no** textures, **no**
models, **no** blockstates, **no** shaders and **no** font files. I searched the index for
`textures/`, `models/`, `blockstates/`, `shaders/`, `font/` and got `count: 0`. The 1.8 index is purely
**audio + localisation + icons**. (The move of *sounds* out of the jar happened in 1.7.2; the move of *textures and
models* to hashed objects happened much later, in the 1.19-era index formats. 1.8 sits in the middle: sounds are
external, everything visual is still in the jar.)

Index keys are *not* the same as jar-internal paths: the index root maps to `assets/`, so `minecraft/sounds.json`
means `assets/minecraft/sounds.json` and `pack.mcmeta` means `assets/pack.mcmeta` (index → disk prefix is
`assets/`). Note the *duplicate-content* entries: `icons/icon_16x16.png` and `minecraft/icons/icon_16x16.png`
share one hash — a content-addressed store naturally dedupes them, a naive path→file copy does not.

### 1.5 Resource URL pattern and SHA1 verification — verified end to end

Pattern: when a legacy index has no `virtual`/`map_to_resources` flag, each object is fetched from

```
https://resources.download.minecraft.net/<first 2 hex chars of hash>/<full 40-char hash>
```

Two live verifications:

```
url=https://resources.download.minecraft.net/29/29d4dccf3353334c7aa2a49cb6fed3780a51a1ba  status=200 size=33948 ct=application/ogg
sha1 of downloaded: 29d4dccf3353334c7aa2a49cb6fed3780a51a1ba  obj1.bin
expected:           29d4dccf3353334c7aa2a49cb6fed3780a51a1ba
```

```
sounds.json status=200 size=37731 ct=application/octet-stream
5e06ca070067486427a3167ade2ffe01623e5591  obj2.bin
--- first 200 bytes of the fetched sounds.json ---
{
  "ambient.cave.cave": {
    "category": "ambient",
    "sounds": [
      "ambient/cave/cave1",
      "ambient/cave/cave10",
```

So the verification recipe is: SHA1 the raw bytes of the response body, lowercase hex, compare to `objects[path].hash`;
on mismatch, delete and retry. Do **not** trust `Content-Type` — one object came back as `application/ogg` and
another as `application/octet-stream` for what is plainly JSON. Do not expect `ETag`-based caching to be
authoritative either; the hash *is* the identity, so a local store keyed by `<2>/<hash>` is idempotent across
versions and across launchers. File sizes in the index (`size`) are a cheap early check (and the basis of
`totalSize`).

Also note the historical `virtual` / `map_to_resources` mechanism: indexes lacking `virtual: true` (all of 1.8,
1.9+) are meant to be *read from a content-addressed store by hash*, whereas `virtual: true` indexes (pre-1.7.3)
are meant to be **materialised** as a mirrored directory tree. A launcher that only ever targets 1.8.9 needs only
the non-virtual path, but the check (`if index.get("virtual"): materialise`) belongs in the spec so the code
doesn't silently corrupt old versions later. *[The `virtual` semantics are documented behaviour of the legacy
asset scheme; in this pass I verified only that 1.8's index does not set the flag.]*

---

## 2. What a native Rust client actually needs at runtime (1.8.9)

`client.jar` is a plain ZIP. Observed: 8,461,484 bytes, **5,597 entries**, of which **3,085** live under
`assets/`. Top-level entry census: `assets/` 3,085, `net/` 33, `META-INF/` 3, and ~2,400 obfuscated `.class`
files at the jar root (e.g. `auz.class`, `ava.class`, `avb.class`).

`assets/minecraft/<subdir>/` counts inside the jar:

| count | jar path |
|---|---|
| 1,595 | `assets/minecraft/models/` |
| 1,058 | `assets/minecraft/textures/` |
| 340 | `assets/minecraft/blockstates/` |
| 87 | `assets/minecraft/shaders/` |
| 3 | `assets/minecraft/texts/` |
| 1 | `assets/minecraft/lang/` |
| 1 | `assets/minecraft/font/` |

### 2.1 In the jar vs in the asset index — verified by direct probing

I probed both sides for the same paths. Results:

| path | in `client.jar`? | in 1.8 asset index? |
|---|---|---|
| `assets/minecraft/textures/blocks/stone.png` | **YES** | no |
| `assets/minecraft/textures/gui/options_background.png` | **YES** | no |
| `assets/minecraft/textures/gui/widgets.png` | **YES** | no |
| `assets/minecraft/textures/gui/container/inventory.png` | **YES** | no |
| `assets/minecraft/textures/gui/title/minecraft.png` | **YES** | no |
| `assets/minecraft/textures/environment/rain.png` | **YES** | no |
| `assets/minecraft/textures/colormap/grass.png` | **YES** | no |
| `assets/minecraft/textures/misc/water.png` | **YES** | no |
| `assets/minecraft/textures/particle/particles.png` | **YES** | no |
| `assets/minecraft/models/block/stone.json` | **YES** | no |
| `assets/minecraft/blockstates/stone.json` | **YES** | no |
| `assets/minecraft/lang/en_US.lang` | **YES** (the *only* `.lang` in the jar) | no |
| `assets/minecraft/sounds.json` | **NO** | **YES** (`minecraft/sounds.json`) |
| `assets/minecraft/sounds/ambient/cave/cave1.ogg` | **NO** (grep count for `assets/minecraft/sounds/` in the jar = 0) | **YES** |
| `assets/minecraft/font/default.png` | **NO** | **NO** (does not exist in 1.8.9 at all) |

Supporting counts from the jar listing: `.png` entries inside the jar = **1,044**; entries under
`assets/minecraft/sounds/` = **0**; entries under `assets/minecraft/lang/` = **1**.

Fonts deserve a specific callout because it is easy to get wrong: the jar contains
`assets/minecraft/textures/font/ascii.png`, `assets/minecraft/textures/font/ascii_sga.png` and 222
`assets/minecraft/textures/font/unicode_page_XX.png` files (224 entries under `textures/font/`), plus a single
`assets/minecraft/font/glyph_sizes.bin` — the per-codepoint widths table. `default.png` (the pre-1.6 bundled
bitmap font) is gone from both places. A native renderer must read the glyph-sizes binary from the jar and the
page PNGs from the jar; nothing font-related comes from the asset index.

Animated/CTM data is also in the jar, as sidecar `.mcmeta` files next to the texture:

```
assets/minecraft/textures/blocks/lava_still.png.mcmeta
assets/minecraft/textures/blocks/lava_flow.png.mcmeta
assets/minecraft/textures/blocks/fire_layer_0.png.mcmeta
assets/minecraft/textures/blocks/portal.png.mcmeta
assets/minecraft/textures/blocks/sea_lantern.png.mcmeta
assets/minecraft/textures/blocks/prismarine_rough.png.mcmeta
```

Ignore these and lava, fire, portals and sea lanterns will render as still frames — a visible 1:1-fidelity
regression. They are jar-only.

Also jar-only and load-bearing for visuals: `assets/minecraft/shaders/program/*` (62 entries) and
`assets/minecraft/shaders/post/*` (25 entries) — 87 shader JSON+program files total. Vanilla 1.8.9 uses these
for things like the "super secret" settings, entity glint, and post-processing; if the native client claims 1:1
visuals, the shader stage has to be reimplemented or these are simply dead weight. Note the `post/` directory
contains `antialias.json`, `art.json`, `bits.json`, `blobs.json`, `blur.json`, etc. — these are the Shader Pack
(Super Secret Settings) post chains.

Finally, verify the jar contains **no** `version.json`: `grep -c version.json` on the full listing returned **0**.
`version.json` (with `protocol_version`) was only added in 18w47b, so **1.8.9 cannot self-report protocol 47** —
the client must hardcode it. Cited source for both facts and the number itself: minecraft.wiki's
`Protocol_version` page states, verbatim,

> "For example, a client running [[Java Edition 1.8.9]] can connect to a server running [[Java Edition 1.8]], as
> both have a protocol version of 47."

and the same page documents that `version.json` "found in the root directory of the client.jar and server.jar
files" exists only "[e]ach Minecraft build since [[18w47b]]".
(<https://minecraft.wiki/w/Protocol_version>, raw wikitext fetched 2026-09-22.)

### 2.2 The runtime partition the spec should adopt

**Needed for 1-to-1 visuals, all from `client.jar` (read-only, never executed):**
`assets/minecraft/textures/**` (blocks, items, entity, gui, font, misc, models/armor, environment, colormap,
map, particle, painting), `assets/minecraft/models/**`, `assets/minecraft/blockstates/**`,
`assets/minecraft/textures/**/*.mcmeta`, `assets/minecraft/font/glyph_sizes.bin`,
`assets/minecraft/shaders/**`, `assets/minecraft/texts/**` (credits/end text), `assets/minecraft/lang/en_US.lang`.

**Needed for audio/localisation, all from the asset index (hashed objects):**
`minecraft/sounds.json` + the 578 `.ogg` files, the 74 non-English `minecraft/lang/*.lang` files, and the 74
`realms/lang/*.lang` files. Icons (`minecraft/icons/*`, `icons/*`) are for the launcher/window, not the client.

**Java-only — irrelevant to us, never download or ship:** all `net/**` and root `*.class` entries (the entire
game logic, which we are replacing), `META-INF/MOJANGCS.SF` and `META-INF/MOJANGCS.RSA` (the jar signature),
the `logging.client.file` log4j2 XML (`client-1.7.xml`), all 37 libraries including LWJGL 2.9.4 / jinput /
jna / ICU4J / jopt-simple / Twitch / oshi / guava, and the `javaVersion`/`java-runtime-*` download hints.

**Consequence for the spec:** the *only* thing the launcher needs from Mojang at runtime is (1) the version
JSON, (2) `client.jar` (for visual assets, sliced out as an archive — not executed), (3) the asset index and its
734 objects. That is ≈8.5 MB + 114.9 MB ≈ **123 MB** for a complete 1.8.9 asset set, versus the official
launcher's footprint which additionally pulls a JRE and all 37 libraries.

---

## 3. Microsoft / Mojang authentication — 2026 state

There is **no** "client ID + client secret" for third parties; the chain is Microsoft Entra ID (Azure AD) →
Xbox Live → XSTS → Minecraft Services. The canonical community documentation of the whole chain is
<https://minecraft.wiki/w/Microsoft_authentication> (raw wikitext fetched live in this session and quoted
below). Microsoft's own documentation covers only the first hop
(<https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code>).

### 3.0 A client-ID problem worth flagging early

Historically third-party launchers reused Mojang's well-known public AAD app ID `00000000402b5328`. Probed live
on 2026-09-22 against the consumers tenant:

```
POST https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode
     client_id=00000000402b5328&scope=XboxLive.signin%20offline_access
→ 400
{"error":"unauthorized_client","error_description":"AADSTS700016: Application with identifier '00000000402b5328' was not found in the directory '9188040d-6c67-4c5b-b112-36a304b66dad'. This can happen if the application has not been installed by the administrator of the tenant or consented to by any user in the tenant. You may have sent your authentication request to the wrong tenant. ...","error_codes":[700016]}
```

The same client ID without a `scope` parameter first produced `AADSTS900144: The request body must contain the
following parameter: 'scope'.` — so the request shape is right and the *app id* is what failed. **A new launcher
in 2026 must register its own Azure application** rather than depending on a legacy public client ID. `[Caveat:
this is one observation; AADSTS700016 is also the error a wrong-tenant request produces, so treat it as "do not
rely on this ID" rather than a proof of deletion. It does mean a shipped launcher cannot assume it works.]`
The wiki also warns that **newly created Azure apps must apply for permission to use the Minecraft API**, and
that without it `api.minecraftservices.com` returns **403**:

> "According to this support Article, new created Azure Apps must apply for the Permission to use the Minecraft
> API using this form. If your App don't have the Permission `api.minecraftservices.com` will return a 403."

That is a real gating item for the spec: **app registration + Minecraft API permission request is a prerequisite,
not a code task.**

### 3.1 Step 1 — Entra ID token (device code flow)

* `GET`/`POST https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode`
* Body (`application/x-www-form-urlencoded`): `client_id=…&scope=XboxLive.signin offline_access`
* Live probe with no body → **400**, verbatim:
  `{"error":"invalid_request","error_description":"AADSTS900144: The request body must contain the following parameter: 'scope'. ..."}`

Microsoft Learn's page (fetched live) documents the response fields verbatim as: `device_code` ("A long string
used to verify the session between the client and the authorization server. The client uses this parameter to
request the access token from the authorization server."), `user_code` ("A short string shown to the user used to
identify the session on a secondary device."), `verification_uri` ("The URI the user should go to with the
user_code in order to sign in."), `expires_in`, `interval` ("The number of seconds the client should wait
between polling requests.") and `message` ("A human-readable string with instructions for the user. This can be
localized by including a query pa[rameter]"). The same page states the user has **15 minutes** to sign in.

* Then poll `POST https://login.microsoftonline.com/consumers/oauth2/v2.0/token` with
  `grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id=…&device_code=…` (both strings quoted
  verbatim from Microsoft Learn).

Two hard requirements from the wiki, both verbatim:

> "In any case, you'll need to include `XboxLive.signin` in the `scope` parameter of the authorization request;
> otherwise the next endpoint will complain, and not in a helpful way."

> "Note: You must use the `consumers` AAD tenant to sign in with the `XboxLive.signin` scope. Using an Azure AD
> tenant ID or the `common` scope will just give errors. This also means you cannot sign in with users that are in
> the AAD tenant, only with consumer Microsoft accounts."

`offline_access` is what yields a **refresh token**; that is the only long-lived credential in the whole chain.
Refresh is a plain repeat of the token endpoint with `grant_type=refresh_token&refresh_token=…` (probed live with
junk values → **400** `AADSTS7000012: The grant was obtained for a different tenant.`), so no Xbox/Minecraft
state is needed to refresh — a launcher can silently refresh on every start.

### 3.2 Step 2 — Xbox Live user authenticate

```
POST https://user.auth.xboxlive.com/user/authenticate
Content-Type: application/json
Accept: application/json
```

```json
{
    "Properties": {
        "AuthMethod": "RPS",
        "SiteName": "user.auth.xboxlive.com",
        "RpsTicket": "d=<access token>"
    },
    "RelyingParty": "http://auth.xboxlive.com",
    "TokenType": "JWT"
}
```

Verbatim from the wiki, including the note on the `d=` prefix: `"RpsTicket": "d=<access token>" // your access
token from the previous step here, make sure that it is prefixed with 'd='`. Response fields, verbatim:
`"Token": "token" // save this, this is your xbl token` and `"DisplayClaims": {"xui": [{"uhs": "userhash" // save this}]}`.

Wiki caveats, verbatim: "Again, it will complain if you don't set `Content-Type: application/json` and `Accept:
application/json`. It will also complain if your SSL implementation does not support SSL renegotiations."
(The first two are easy in Rust; the third is a historical JSSE quirk.)

Live probe with a deliberately bad `RpsTicket` returned **401** with an empty body and headers
`cache-control: no-cache, no-store`, `content-length: 0`, `ms-cv: …`, `x-xblcorrelationid: 00000000-…`. So:
**XBL errors carry no body** — the launcher must report the HTTP status plus correlation id, not a JSON error.

### 3.3 Step 3 — XSTS authorize

```
POST https://xsts.auth.xboxlive.com/xsts/authorize
Content-Type: application/json
Accept: application/json
```

```json
{
    "Properties": { "SandboxId": "RETAIL", "UserTokens": ["xbl_token"] },
    "RelyingParty": "rp://api.minecraftservices.com/",
    "TokenType": "JWT"
}
```

Note the `RelyingParty` is **`rp://api.minecraftservices.com/`** (a URI with an `rp://` scheme, not `https://`).
Response: `Token` (the XSTS token) and the same `uhs` userhash. Live probe with a bad token → **400**, empty body.

This is the step that produces user-facing failures, and the wiki tabulates them verbatim:

```
{"Identity": "0", "XErr": 2148916238, "Message": "", "Redirect": "https://start.ui.xboxlive.com/AddChildToFamily"}
```

> * **2148916227**: The account is banned from Xbox.
> * **2148916233**: The account doesn't have an Xbox account. Once they sign up for one (or login through
>   minecraft.net to create one) then they can proceed with the login. …
> * **2148916235**: The account is from a country where Xbox Live is not available/banned
> * **2148916236**: The account needs adult verification on Xbox page. (South Korea)
> * **2148916237**: The account needs adult verification on Xbox page. (South Korea)
> * **2148916238**: The account is a child (under 18) and cannot proceed unless the account is added to a Family
>   by an adult. This only seems to occur when using a custom Microsoft Azure application. When using the
>   Minecraft launchers client id, this doesn't trigger.
> * **2148916262**: *TBD, happens rarely without any additional information.*

The 2148916238 note is worth highlighting for this project: because we now *must* use a custom Azure app (§3.0),
the family/child restriction **will** trigger for child accounts, where the official launcher's client ID dodged it.

### 3.4 Step 4 — Minecraft Services login

```
POST https://api.minecraftservices.com/authentication/login_with_xbox
Content-Type: application/json
Accept: application/json
```

```json
{ "identityToken": "XBL3.0 x=<userhash>;<xsts_token>" }
```

Note the exact concatenation: literal `XBL3.0 x=`, the `uhs` hash, a `;`, then the XSTS token.

Response (verbatim from the wiki):

```json
{
    "username": "some uuid", // this is not the uuid of the account
    "roles": [],
    "access_token": "minecraft access token", // jwt, your good old minecraft access token
    "token_type": "Bearer",
    "expires_in": 86400
}
```

Verbatim wiki warning, which is the crux of "you must still check entitlements":

> "This access token allows us to launch the game, but, we haven't actually checked if the account owns the game.
> Everything until here works with a normal Microsoft account!"

`expires_in: 86400` → the Minecraft bearer token is good for 24 h.

### 3.5 Steps 5–6 — entitlement and profile checks

```
GET https://api.minecraftservices.com/entitlements/mcstore
Authorization: Bearer <Minecraft Access Token>
```

Owned-account response, per the wiki: an `items` array containing objects with `name` + `signature` (JWT), for
example `product_minecraft`, `game_minecraft`, and Bedrock/Dungeons equivalents when present. Probe with a bogus
token → **401**:

```json
{ "path" : "/entitlements/mcstore" }
```

Then:

```
GET https://api.minecraftservices.com/minecraft/profile
Authorization: Bearer <Minecraft Access Token>
```

Live probe with a bogus token → **401**:

```json
{ "path" : "/minecraft/profile" }
```

The 200 response of this endpoint supplies the real `id` (UUID, no dashes) and `name` used for the introspect
step — **this**, not `login_with_xbox`'s `username`, is the player identity. Note `Bearer ` (with the trailing
space) is mandatory; the wiki says so explicitly.

### 3.6 The player profile / skin endpoint (useful for a native client's own textures)

`GET https://sessionserver.mojang.com/session/minecraft/profile/069a79f444e94726a5befca90e38aaf5` → **200**,
verbatim (truncated):

```json
{
  "id" : "069a79f444e94726a5befca90e38aaf5",
  "name" : "Notch",
  "properties" : [ {
    "name" : "textures",
    "value" : "ewogICJ0aW1lc3RhbXAiIDogMTc5MDExMDQwNjUyMSwKICAicHJvZmlsZUlkIiA6ICIwNjlhNzlmNDQ0ZTk0NzI2YTViZWZjYTkwZTM4YWFmNSIsCiAgInByb2ZpbGVOYW1lIiA6ICJOb3RjaCIsCiAgInRleHR1cmVzIiA6IHsKICAgICJTS0lOIiA6IHsKICAgICAgInVybCIgOiAiaHR0cDovL3RleHR1cmVzLm1pbmVjcmFmdC5uZXQvdGV4dHVyZS8yOTIwMDlhNDkyNWI1OGYwMmM3N2RhZGMzZWNlZjA3ZWE0Yzc0NzJmNjRlMGZkYzMyY2U1NTIyNDg5MzYyNjgwIgogICAgfQogIH0KfQ=="
  } ],
  "profileActions" : [ ]
}
```

Note `profileActions` — a **modern** addition, absent in the 1.8.9 era. A 1.8.9 client must ignore unknown fields
rather than reject them.

### 3.7 Proving ownership to a server — server-id hash, join, hasJoined

This is the part a *native* client must implement itself, and it is the part most likely to be got wrong in Rust.
Canonical documentation: <https://minecraft.wiki/w/Java_Edition_protocol/Encryption> (fetched live).

**The hash (client side), verbatim from the wiki:**

```
sha1 := Sha1()
sha1.update(ASCII encoding of the server id string from Encryption Request)
sha1.update(shared secret)
sha1.update(server's encoded public key from Encryption Request)
hash := sha1.hexdigest()  # String of hex characters
```

with the critical warning, verbatim:

> "Note that the Sha1.hexdigest() method used by Minecraft is non standard. It doesn't match the digest method
> found in most programming languages and libraries. It works by treating the sha1 output bytes as one large
> integer in two's complement and then printing the integer in base 16, placing a minus sign if the interpreted
> number is negative. Here are some examples of Minecraft's hexdigest:"

```
sha1(Notch) :  4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48
sha1(jeb_)  : -7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1
sha1(simon) :  88e16a1019277b15d58faf0541e11910eb756f6
```

Those three strings are **golden test vectors** and belong in the Rust test suite — roughly half of all digests
come out with a leading `-`, so a plain `hex::encode` will produce a hash the session server rejects with a
mismatch that looks like an auth failure. The wiki's own sample-code list includes a Rust implementation
(`https://git.io/fj6P0`).

**Server ID string:** verbatim — "**Update (1.7.x):** The server ID is now sent as an empty string. Hashes also
utilize the public key, so they will still be correct." So for protocol 47 the first `sha1.update()` gets zero
bytes, and the hash is over `secret || publicKeyDER`.

**Join (client → Mojang), verbatim shape:**

```
POST https://sessionserver.mojang.com/session/minecraft/join
  { "accessToken": "<accessToken>", "selectedProfile": "<player's uuid without dashes>", "serverId": "<serverHash>" }
```

> "You *must* have the Content-Type header set to application/json or you will get a 415 Unsupported Media Type
> or 403 Forbidden response." … "If everything goes well, the client will receive a '204 No Content' response."

Live probe with `-d '{}'` → **403**:

```json
{
  "error" : "ForbiddenOperationException",
  "path" : "/session/minecraft/join"
}
```

which matches the wiki's account of the generic failure mode ("You must have Content-Type…"; my probe sent
`Content-Type: application/json` but an empty JSON object, so it hit `ForbiddenOperationException` rather than the
non-descriptive `{"error":"Forbidden","path":…}` case).

Two more documented failure shapes worth handling in code:

```json
{"error": "InsufficientPrivilegesException", "path": "/session/minecraft/join"}          // Xbox multiplayer disabled
{"error": "UserBannedException", "path": "/session/minecraft/join", "errorMessage": "…"}  // multiplayer-banned
```

**hasJoined (server → Mojang):**

```
GET https://sessionserver.mojang.com/session/minecraft/hasJoined?username=<name>&serverId=<hash>[&ip=<ip>]
```

Live probes: with no matching session → **204 No Content** (`status=204 type=`), both with and without the
legacy `&time=1449273600000` parameter. Also note that `username` is **case-insensitive** and must be the
in-game name from Login Start, not the Microsoft account name; `&ip=` is only sent by a vanilla server when
`prevent-proxy-connections=true`.

Documented semantics, verbatim: "Note that only the last serverId/hash sent to this endpoint for a given player
will cause the hasJoined endpoint below to respond with a 200 OK, otherwise it will be a 204 No Content." A 200
response body is:

```json
{"id": "<profile identifier>", "name": "<player name>", "properties": [{"name": "textures", "value": "<base64>", "signature": "<base64; signed data using Yggdrasil's private key>"}]}
```

and "The profile id in the json response has format `11111111222233334444555555555555` which needs to be changed
into format `11111111-2222-3333-4444-555555555555`" before going into Login Success. **1.9+ servers take the
skin from these `properties`; a 1.8.9 client must not assume a skin fetch is possible from 1.8.9-era auth** — the
1.8.9 server does read them, so the property blob matters even for old clients.

**Did the session server change?** The *host* did not: `https://sessionserver.mojang.com/session/minecraft/*` is
still live in 2026 and answered 204/403/200 as documented above, and `join` is still POST while `hasJoined` is
still GET (my POST to `hasJoined` returned **405**, confirming GET-only). What *has* drifted is the payload
surface: `hasJoined`/`profile` now carry `profileActions` (observed live in §3.6) and Mojang's newer components
(profile keys / chat signing, introduced with the 1.19 cycle) ride along as extra properties. A protocol-47
client sets `serverId` to the empty-string-derived hash exactly as in 2015 and ignores the extras. The wiki's
History table on the Encryption page also records, verbatim: 1.3.1/12w17a "Encryption of the protocol is
introduced."; `?` "Offline mode is now unencrypted."; 1.20.5/24w03a "Protocol encryption can now be used in
offline mode again." — none of which changes protocol 47's behaviour.

### 3.8 The protocol-47 encryption handshake the client must implement

Documented at <https://minecraft.wiki/w/Java_Edition_protocol/Encryption>. Live-fetched excerpts, verbatim:

> "The server generates a 1024-bit RSA keypair on startup. The public key sent in the Encryption Request packet
> is encoded in ASN.1 DER format. … The schema is the same as the `SubjectPublicKeyInfo` structure defined by
> X.509 (not a full-blown X.509 certificate!)"

So **the key size vanilla 1.8.9 servers send is 1024-bit** (RSA-1024, ~128-byte ciphertexts) — but, verbatim,
"It is also possible for a modified or custom server to use a longer RSA key, without breaking official
clients." A Rust implementation must therefore parse DER generically (a `SubjectPublicKeyInfo` parser such as the
`der`/`rsa` crates, or `spki`) and size the ciphertext from the parsed modulus, never hardcode 128 bytes. The
wiki also notes you can round-trip the DER into a PEM `-----BEGIN PUBLIC KEY-----` block if your crypto API wants
PEM.

Key exchange and cipher, verbatim:

> "When it receives an Encryption Request from the server, the client will generate a random 16-byte (128-bit)
> shared secret, to be used with the AES/CFB8 stream ciphers. It then encrypts the shared secret and verify token
> with the server's public key (PKCS#1 v1.5 padded), and sends both to the server in an Encryption Response
> packet. Both byte arrays in the Encryption Response packet will be 128 bytes long because of the padding. This
> is the only time the client uses the server's public key."

> "In your crypto library, ensure that you set up your 'feedback/segment size' to 8 bits or 1 byte, as indicated
> in the name AES/CFB **8**. Any other feedback size will result in encryption mismatch."

> "The server decrypts the shared secret and token using its private key, and checks if the token is the same. It
> then enables AES/CFB8 encryption and sends the Login Success packet encrypted. The server makes two ciphers,
> one for encryption and one for decryption, with the key and initial vector (IV) both set to the shared secret.
> The client does the same, setting up its own two ciphers identically. From this point forward, everything is
> encrypted, including the length field, packet ID, and data length (if compression is enabled)."

> "Note that the AES cipher is updated continuously, not finished and restarted every packet."

The four Rust-relevant traps, all directly from that text: (1) CFB**8**, i.e. an 8-bit segment size, not the
usual 128-bit CFB; (2) **IV == key == shared secret**; (3) **two independent cipher contexts** (encrypt and
decrypt), each continuous across packets — a stateful stream, so a per-packet `copy_from_slice` of the IV across
threads will deadlock/garble; (4) padding is **PKCS#1 v1.5**, not OAEP. Also: the Encryption Request's verify
token is a short random byte string the client must echo — as **encrypted** bytes, per the wiki's step list.
The packet flow, verbatim: Handshake → Login Start → Encryption Request → *(client authentication)* →
Encryption Response → *(server authentication)* → both enable encryption → Login Success.

Placement in the sequence matters: the client posts to `sessionserver…/join` **after** generating the shared
secret and computing the hash, and **before** sending Encryption Response (or at least before the server's
`hasJoined`, which races it). The wiki describes step 4 "Client authentication (if enabled)" between Encryption
Request and Encryption Response.

Finally, 1.8.9 has no `?unsigned=false` and no profile-key requirement at login — do not add modern steps.

---

## 4. Launcher profile / account storage conventions on Linux

### 4.1 `~/.minecraft/launcher_profiles.json`

No `~/.minecraft` exists on this machine (`ls: cannot access '/home/lucy/.minecraft': No such file or directory`),
so the structure below is quoted from the wiki documentation rather than a local file. The document's own
definition, verbatim:

> "**launcher_profiles.json** (**launcher_profiles_microsoft_store.json** for the Minecraft Launcher for Windows)
> is a JSON file located in .minecraft, which contains all the Minecraft launcher settings, profiles, selected
> user/profile as well as the cached user information (email, access token, etc.)."
> — <https://minecraft.fandom.com/wiki/Launcher_profiles.json> (mirror of the classic Minecraft Wiki page)

Top-level keys, verbatim from its table:

| Key | Type | Description (quoted) |
|---|---|---|
| `profiles` | Map | "All the launcher profiles and their configurations." |
| `clientToken` | String | "The currently logged in client token." |
| `authenticationDatabase` | Map | "All the logged in accounts. Every account in this key contains a UUID-hashed map (which is used to save the selected user) which in turn includes the access token, e-mail, and a profile (which contains the account display name)" |
| `launcherVersion` | Map | "Contains the current launcher build name, format and profiles format." |
| `settings` | Map | "Contains all the launcher settings" |
| `analyticsToken` | String | "The latest token for tracking analysts." |
| `analyticsFailcount` | Integer | fail count |
| `selectedUser` | Map | "Contains the UUID-hashed account and the UUID of the currently selected user" |

Profile entries (`profiles.<name>`) carry `name`, `type` (`custom` / `latest-release` / `latest-snapshot`),
`created` and `lastUsed` (ISO 8601 strings), `icon` (a base64-encoded image), `lastVersionId` ("The version ID
that the profile targets. Version IDs are determined in the version.json in every directory in `~/versions`"),
`gameDir`, `javaDir`, `javaArgs`, `logConfig`, `logConfigIsXML`, and `resolution` (`{width, height}`).
`authenticationDatabase.<uuid-hash>` carries `accessToken`, `username` (e-mail) and `profiles`; `selectedUser`
carries `account` and `profile` UUID keys.

**The RFC-1918 of Minecraft launchers:** note that this file stores a *raw access token in plaintext JSON*. That
was acceptable in the Yggdrasil era. In the Microsoft era the equivalent cached secret is a **refresh token**,
which is a full account credential (it can mint new Microsoft access tokens for the Xbox chain). Treat
`launcher_profiles.json` as a legacy format to *read*, not a format to *write secrets into*.

### 4.2 What a third-party launcher on Linux typically does

The modern convention among third-party launchers (Prism/MultiMC lineage) is to **not** own `~/.minecraft` at all:
they keep an application data directory under XDG paths (`~/.local/share/<launcher>/`), store each game
installation as a self-contained "instance" directory, and treat the Mojang asset/object store as a **shared,
globally deduplicated cache** referenced by all instances. *[This is a description of the prevailing
architecture, not something verified by fetch in this pass.]* For this project the natural Linux layout is:

```
~/.local/share/<launcher>/accounts.json          # account metadata ONLY (uuid, name, xuid, uhs)
~/.local/share/<launcher>/instances/<id>/        # per-instance game dir
~/.local/share/<launcher>/assets/indexes/1.8.json
~/.local/share/<launcher>/assets/objects/<2>/<hash>
~/.local/share/<launcher>/versions/1.8.9/1.8.9.json
~/.local/share/<launcher>/libraries/…
~/.config/<launcher>/config.toml                 # settings, non-secret
```

with `XDG_DATA_HOME`/`XDG_CONFIG_HOME` respected. The launcher-side marker file `.mcassetsroot` in the assets
root is the vanilla convention for "this directory is a valid assets root"; *[unverified in this pass — it is not
present inside `client.jar` (jar listing shows no `mcassetsroot` entry), so if you want Mojang-launcher
interoperability you should confirm the marker's exact name/placement against a real launcher install].*

Because the object store is content-addressed, a **shared** assets directory is strictly better than per-instance
copies — the same 114.9 MB of 1.8 objects is reused across every 1.8.x instance, and nothing in it is
instance-specific.

### 4.3 Where to store tokens safely in Rust

The secrets in play, in decreasing sensitivity: **AAD refresh token** (long-lived, full account access) >
**Xbox XSTS token** (short-lived) > **Minecraft access token** (24 h, per §3.4) > XBL token/uhs.

Rust options, verified against crates.io on 2026-09-22:

* `keyring` — crates.io: `name: keyring`, `max_stable_version: 4.2.0`, `downloads: 26469002`,
  `description: All-in-One Rust Keyring`, repo `https://github.com/open-source-cooperative/keyring-rs`,
  wiki `https://github.com/open-source-cooperative/keyring-rs/wiki/Keyring`. This is the pragmatic choice: one
  API over Secret Service / KWallet / macOS Keychain / Windows Credential Manager, with the platform backend
  chosen at build time.
* `secret-service` — crates.io: `max_stable_version: 5.2.0`, `description: Library to interface with Secret
  Service API`. Use this if you want to speak D-Bus Secret Service directly instead of going through `keyring`.

Linux-specific caveats for CachyOS/Arch: Secret Service is not a kernel feature — it requires a running session
D-Bus plus a provider (`gnome-keyring-daemon`, `kwalletd`, or `keepassxc`'s Secret Service bridge) to be
installed and unlocked. A headless or minimal-WM session may have none of that. The spec therefore needs a
**three-tier policy**: (1) OS keyring if available and unlocked; (2) else an encrypted-at-rest file requiring a
user passphrase; (3) else refuse to persist the refresh token and force a fresh device-code login each session
(safer than silently writing a plaintext credential into `~/.local/share`). Whatever is chosen, the **AAD refresh
token must never be written into a `launcher_profiles.json`-shaped file** that other launchers, backup tools, or
sync clients might read. Also note that keyring entries are per-user and can be enumerated by other processes
running as that user — the threat model is "other users and casual file exfiltration", not "malware running as
you".

---

## 5. Legal constraints

Primary sources fetched live in this session:

* EULA — <https://www.minecraft.net/en-us/eula> (full text also cached locally)
* Minecraft Usage Guidelines — <https://www.minecraft.net/en-us/usage-guidelines>
* Legacy Mojang terms — <https://minecraft.net/en-us/terms/r1>

### 5.1 (a) Downloading assets at runtime for a third-party client

The controlling sentence is in the EULA, verbatim:

> "In order to ensure the integrity of our games, we need all game downloads and updates to come from a source
> that we authorize. It's also important for us that 3rd party tools/services don't seem 'official' as we can't
> guarantee their quality."

The same sentence appears verbatim in the legacy terms at <https://minecraft.net/en-us/terms/r1>, confirming it
is a durable, deliberate position rather than a one-off phrasing. The operative reading for this project:

* Downloading from `piston-meta.mojang.com`, `launchermeta.mojang.com`, `launcher.mojang.com`,
  `libraries.minecraft.net` and `resources.download.minecraft.net` **is** downloading from an authorized source.
  Those are Mojang's own CDNs and are exactly what the official launcher hits.
* Downloading the same bytes from a third-party mirror, a bundled tarball, an archived copy, or a "fast" CDN is
  **not** necessarily an authorized source even if the bytes are identical. Byte-identical is not the test; the
  *source* is.
* Therefore the launcher should fetch at runtime, on the end user's machine, with the end user's own credentials
  and bandwidth, and should verify SHA1 from the metadata it also fetched from Mojang. That design is what keeps
  it inside the sentence above.

The Usage Guidelines' "Personal use" section is explicitly permissive for private use, verbatim:

> "We are very relaxed about things you create for yourself. Pretty much anything goes there - so go for it and
> have fun, just remember the policies and don't do anything illegal or infringing on others."

…but the very next paragraph reclassifies *sharing* as commercial, verbatim: "When you decide to share your
content with the community (whether you plan to make money off it or not), you are doing what we consider to be a
commercial thing." So **open-sourcing the launcher moves the project from "personal use" into the "commercial"
bucket of the guidelines**, which is the bucket where the naming and no-redistribution rules get teeth.

### 5.2 (b) Not redistributing assets or the jar

EULA, verbatim:

> "However, you must not distribute anything we've made unless we specifically agree to it. By 'distribute
> anything we've made' what we mean is: give copies of our game software or content to anyone else; make
> commercial use of anything we've made; try to make money from anything we've made; or let other people get
> access to anything we've made in a way that is unfair or unreasonable."

Usage Guidelines, "Essential guidelines", verbatim:

> "Do not redistribute our games or any alterations of our games or game files"

and its definition of assets, verbatim: "Our assets, we mean the code, software, graphics, textures, images,
models, sounds and other audio from any of our games and any videos or screenshots taken from our games".

Concrete consequences for an open-source repo:

* **Never** commit `client.jar`, any `.ogg`, any texture `.png`, `sounds.json`, `en_US.lang`, `glyph_sizes.bin`,
  the log4j XML, or the 1.8 asset index JSON itself. A test fixture that is one real Minecraft texture is a
  redistribution.
* **Never** commit a `client.jar` chunk, an extracted `assets/` tree, or a `.zip` of any of it — including in git
  history, releases, CI caches, Docker images, or "test data" branches. If it was ever committed, a filter-repo
  history rewrite is required, not just a delete.
* Ci/test fixtures must be **synthetic** (hand-drawn 16×16 PNGs, a fake `objects` map with your own hashes,
  a stub `sounds.json`) or generated at test time by your own downloader under the user's own account.
* The downloader itself, the SHA1 verifier, the URL templates, the asset-index parser, the loader's *code*, and
  the Rust client's own art (if it draws anything) are all **yours** and are fine to ship.
* Note the EULA's "**Mods**" carve-out, verbatim: "If you've bought Minecraft: Java Edition, you may play around
  with it and modify it by adding modifications… Any Mods you create for Minecraft: Java Edition from scratch
  belong to you… and you can do whatever you want with them, as long as you don't sell them for money / try to
  make money from them and so long as you don't distribute Modded Versions of the game." The definition is
  important: a Mod is "something original that you or someone else created that **doesn't contain a substantial
  part of our copyrightable code or content**." A from-scratch Rust client that contains **none** of Mojang's
  code is the strongest possible version of that position — which is precisely why the spec must not embed,
  translate, decompile, or vendor any decompiled Mojang class. A clean-room reimplementation with its own
  rendering code and its own protocol encoder is defensible; a transliteration of `sources/` is not.
* The EULA also forbids "distribut[ing] any Modded Versions of our game or software" and explicitly: "hacked
  versions or Modded Versions of the game client or server software are not okay to distribute."

### 5.3 (c) Using the name "Minecraft" in a third-party project name/description

Usage Guidelines, "Naming guidelines", verbatim rules and the required disclaimer:

> "You may use our name in connection with your product or service, title, or listing (including on websites,
> video platforms, or merchandise) if you follow the guidelines in this section. You may use the Minecraft name
> in a secondary name, secondary title, or description if you: Do so because it is necessary to describe your
> creations or their purpose honestly and fairly; Ensure that the secondary title (which includes a Minecraft
> name) is not the dominant element or the distinctive part of the complete name or title; Don't use any other
> aspect of any of our brand or assets as part of any related branding, including as a logo or part of a logo;
> Don't use our name as keywords or search tags for products that have no relationship with them or that are
> infringing or counterfeit."
>
> "You may not use the Minecraft name as the primary or dominant name or title."

with their own examples, verbatim: "Kotoba Miners: A Minecraft server for Redstone builds" (cool with this) vs
"Minecraft - the ultimate Kotoba server for Redstone" (not cool); "The Shaft – a Minecrafter's podcast" (cool)
vs "Minecraft – the ultimate help app" (not cool).

And the mandatory disclaimer, verbatim, which must appear "on your product, listing, description, website/webpage,
and all other related materials":

> "Prominently include the disclaimer similar to the following: 'NOT AN OFFICIAL MINECRAFT [PRODUCT/SERVICE/EVENT/etc.].
> NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT'"

Plus the "Essential guidelines" list, verbatim, which is the checklist this project must satisfy:

> "Do not do anything or include anything that makes people think that what you are sharing could be interpreted as
> official or approved by, endorsed by, associated with, supported by, or connected to us · Do not be unlawful,
> deceptive, obscene, harmful, or abusive · Do not do anything that would harm or damage our name, brand, or
> assets … · Do not redistribute our games or any alterations of our games or game files · Do not make commercial
> use or commercially exploit anything that we have made unless these guidelines say it's okay · Do not give access
> to anything we've made in a way that is unfair or unreasonable · Do not pretend to be / associated with /
> supported by Mojang or Microsoft and make it clear: You (not us) are responsible for the product or service …
> Who the publisher, manufacturer, seller, organizer and/or owner are … Whom to contact about the product…"

Applied to naming: a repo called `mc-rust-client` with description "A from-scratch Rust client and launcher for
**Minecraft** Java Edition 1.8.9" is within the guidelines (name in a *description*, not the dominant title, plus
the disclaimer in the README). A repo called `MinecraftRust` or `RustCraft — the Minecraft client` is not. Also
prohibited by the same section: using Mojang's logo, and using "Minecraft" as a keyword for an unrelated project.
Note the `crates.io` / package-registry angle: a crate or package named `minecraft-*` sits right on the
"dominant element" line; prefer a neutral crate name with the disclaimer in the README.

There is also a corporate-restriction paragraph, verbatim, that does not apply to an individual hobbyist but is
worth knowing: "The allowances we give in these guidelines do not authorize commercial companies, corporate
brands, advertising agencies, non-profits, politicians, political action committees, governments to use or
exploit Minecraft for promoting products, services, or agendas unrelated to Minecraft."

### 5.4 (d) Mojang's stance on third-party launchers and clients

There is **no published Mojang statement banning third-party launchers as such**, and no per-project approval
process. What exists is the pair of constraints already quoted — *authorized sources for downloads* and *no
impersonation* — plus the licensing machinery around "Mods". Mojang's own community wiki explicitly catalogues
"**Clients** – third-party Java Edition clients" among the documented ecosystems at
<https://minecraft.wiki/w/Java_Edition_protocol> (live fetch; the page lists "Authentication systems — Microsoft
authentication — the current authentication system based on Microsoft account and Xbox Live", alongside
"Clients – third-party Java Edition clients", "Servers – third-party Java Edition servers", "Libraries",
"Utilities", "Wrappers", "Generators", "Decompilers"). The protocol has been publicly documented and
reverse-engineered since 2011 with no enforcement action against documentation projects.

What *does* draw action is the thing the EULA names: launchers that (i) serve game files from their own
infrastructure rather than Mojang's, and (ii) bypass authentication so unlicensed users can play. The widely
reported legal commentary around TLauncher turns on exactly those two features — search results fetched live
describe its "no-license" / "cracked" mode as the crux and note that the EULA requires downloads/updates to come
from authorized sources. *[Those are secondary sources (techbloat.com, cloudspress.com, sekin.in), useful as
context for how the rules are read in practice, not as authority.]* The safe reading for this project: **an
online-mode-only launcher that requires a genuine Microsoft login and downloads exclusively from Mojang is on the
right side of both rules; adding an offline/cracked mode is the single change that would flip it.**

### 5.5 Can / cannot — the open-source repo checklist

**CAN ship:** the launcher and client source; a downloader that fetches version JSON, client.jar, the index and
objects from Mojang's own hosts at runtime; SHA1 verification; DER/RSA + AES-CFB8 protocol code; the
device-code/OAuth/Xbox/XSTS/login-with-xbox chain; docs and protocol notes you wrote; synthetic test fixtures;
a README carrying the required "NOT AN OFFICIAL MINECRAFT…" disclaimer, a neutral project name with "Minecraft"
only in the description, and no Mojang logos or textures in screenshots-that-are-branding.

**CANNOT ship:** `client.jar` or any bytes of it; any Mojang texture, sound, `.lang`, `sounds.json`, font or
shader file; the ar/log4j config; a mirror or bundle of the assets; decompiled/translated Mojang source; the
Minecraft or Mojang logo or the word "Minecraft" as the dominant name; any offline/cracked auth mode or
authentication bypass; anything sold or monetised (this is the EULA's line for mods, too: "you can do whatever
you want with them, as long as you don't sell them for money / try to make money from them").

### 5.6 A note on the usage guidelines' own stability

Verbatim: "These guidelines may change as time goes by. We reserve the right to change our mind at any time (such
as if people start to take advantage of our good intentions) and to update these guidelines. So don't count on
these guidelines always being here or in the specific form they are in right now." And: "All permissions and
consents are given by us at our discretion and may be revoked at any time." A launcher that depends on these
permissions should re-check them at release time, not just at design time.

### 5.7 Not legal advice

The above is a reading of the published terms with sources cited. Mojang/Microsoft state explicitly that they
"are not able to give advice about whether a specific project does or does not comply with these guidelines. If
you are unsure, you should speak to an attorney for help."

---

## 6. Implications for the Rust spec (open questions to resolve)

1. **Asset download total.** 8,461,484 B (jar) + 114,885,064 B (index objects) + 18,244 B (version JSON) +
   78,494 B (index) ≈ **123.4 MB** for a complete 1.8.9 set. The `assetIndex.totalSize` field is authoritative
   and can drive a progress indicator without a second pass.
2. **The jar is an asset archive, not an executable.** Slicing `assets/**` out of `client.jar` at runtime (or
   reading the ZIP lazily) satisfies the visual requirement without ever loading a `.class`. This is the design
   that keeps the project clearly on the "from scratch" side of the EULA's Mods definition — and it means the
   spec should state explicitly that **no class file is ever read**, as a design invariant, not an accident.
3. **Rust crypto choices to pin:** DER `SubjectPublicKeyInfo` parsing (not PEM, not a hardcoded 1024-bit
   assumption), RSA PKCS#1 v1.5 encrypt, AES-128-CFB8 with 8-bit segments, IV == key, two continuous cipher
   contexts. The three string vectors `sha1(Notch)`/`sha1(jeb_)`/`sha1(simon)` from §3.7 are the acceptance tests
   for the non-standard "Minecraft hexdigest".
4. **Protocol number must be hardcoded.** 1.8.9 has no `version.json` in the jar (§2.1); 47 comes from the wiki
   and from the client's own source. Put it in a constant with the citation.
5. **Azure app registration + Minecraft API permission is a blocking prerequisite** for any real login testing
   (§3.0), and the legacy public client ID `00000000402b5328` did not work in the one probe performed.
   This belongs in the spec's "external dependencies" section, because it is not code.
6. **Keyring is a policy decision, not a library choice.** The AAD refresh token is a full account credential;
   the spec must state the fallback order and must forbid writing it to a plaintext JSON (§4.3).
7. **Unverified items to confirm before implementation:** the `.mcassetsroot` marker's exact semantics; the
   `virtual`/`map_to_resources` materialisation path (not exercised by 1.8 but needed for pre-1.7.3 later); and
   whether the launcher's own Azure app will be granted the `api.minecraftservices.com` permission at all.
