# Minecraft Java Edition 1.8.9 — Protocol 47 Reference (networking + world data)

Implementation-oriented reference for a Rust re-implementation of a 1.8.9 client.
Compiled 2026-09-22. Every section ends with its sources; see the **Verification key** below for how each
class of claim was established. Facts that could not be verified are explicitly marked `⚠ UNVERIFIED`.

## Verification key

| Tag | Meaning |
|---|---|
| **V** | Read directly from the decompiled vanilla **1.8.9** source (MCP-919, `net.minecraft.*`) — authoritative client/server behaviour. |
| **W** | wiki.vg protocol 47 documentation, page `Protocol&oldid=7194` (last modified 2016-01-03, when 1.8.9 was the latest stable) via Wayback. |
| **W-SMP** | wiki.vg `SMP Map Format` (oldid=6909, 2015-09-08) — the 1.8-era chunk-data article. |
| **W-EM** | wiki.vg `Entities` page (oldid=6366, 2015-01-18) — 1.8-era entity metadata. |
| **W-SLP**, **W-ENC** | wiki.vg `Server List Ping` / `Protocol_Encryption` (Feb 2016 snapshots). |
| **MCW** | minecraft.wiki (modern), used for durable specs (Anvil, light semantics, region format) and for post-1.8 contrast. |
| **D** | PrismarineJS `minecraft-data` `data/pc/1.8/protocol.json` (protocol 47, `minecraftVersion: 1.8.8`) — machine-readable cross-check of every packet id and field type. |

Where **W** and **V** disagree, **V** wins; the disagreements found are called out inline (they matter — two of
them are the classic 1.8 implementation traps).

---

## 1. Connection lifecycle

### 1.1 Transport and framing

* TCP, one connection per player. All multi-byte integers are **big-endian** except VarInt/VarLong and the
  little-endian block shorts inside chunk data (§3).
* Every packet is prefixed by its total length as a VarInt. States: **Handshaking → Status | Login → Play**.
  The state is switched by *sending* Handshake (next state field) and *receiving* Login Success.
* **VarInt**: 7 bits per byte, little-endian groups, continuation bit `0x80` on every byte except the last;
  max 5 bytes for a 32-bit signed value (negative numbers are encoded as the two's-complement 32-bit value →
  always 5 bytes). **VarLong**: same with 64 bits / max 10 bytes.
* **String**: UTF-8 bytes prefixed with the byte length as a VarInt. 1.8.9 caps vary by context
  (username ≤ 16 chars, chat message ≤ 100 chars, server address ≤ 255, plugin channel ≤ 20, locale ≤ 7,
  Chat/JSON text ≤ 32767 bytes). A "Chat" field is a String containing JSON.
* **Position** (1.8.9, **V/W**): a single 64-bit value `((x & 0x3FFFFFF) << 38) | ((y & 0xFFF) << 26) | (z & 0x3FFFFFF)`
  → x/y/z = 26/12/26 signed bits, y in the middle. Decode by sign-extending each field.
  **Pitfall:** later versions changed this packing (current docs use x→z→y), so do not port modern code blind.
* **Fixed-point**: entity coordinates in spawn/teleport packets are Ints = `floor(coord * 32)`; deltas in
  relative-move packets are Bytes = `(coord * 32 - prev * 32)` (so 1/32 block resolution, ±4 blocks).
* **Angle**: 1 byte = 1/256 of a full turn.
* **Slot**: `Short blockId`; if `blockId == -1` the slot is empty, otherwise `Byte count`, `Short damage`,
  `NBT nbt` (nbt is *optional*: a TAG_End byte 0x00 means absent).

### 1.2 Compression (1.8.9 specifics)

* Compression is enabled **in the login state** by the clientbound **Set Compression (Login 0x03, VarInt Threshold)**.
  Vanilla 1.8.9 server (`NetHandlerLoginServer`): sends Login 0x03 with
  `server.getNetworkCompressionTreshold()` **only if the threshold ≥ 0 and the channel is not a local
  (single-player/LAN) channel**, then enables the codec on the channel and continues with Login Success. (**V**)
* Default threshold is **256 bytes** (`network-compression-threshold` in `server.properties`). (**MCW**)
* Framing after enabling:
  * `Packet Length` (VarInt) = bytes that follow, i.e. size of `Data Length` + payload.
  * `Data Length` (VarInt) = uncompressed size of `Packet ID + Data`, **or 0** if the packet is sent uncompressed.
  * If `Data Length > 0`, the rest is **zlib** (RFC1950) compressed `Packet ID + Data`.
  * **Rule:** a packet whose uncompressed `Packet ID + Data` size is **≥ threshold must be compressed**;
    smaller packets **must** be sent with `Data Length = 0` and uncompressed. Violating this (sending a
    too-small payload compressed, or a too-large payload uncompressed) causes the peer to disconnect. (**W**)
  * Threshold `-1` disables compression (then use the plain framing again).
* Ordering: Set Compression and Login Success may be sent in either order, but **everything after Set
  Compression uses the compressed framing**; because Login Success switches the connection to Play, the
  packet id used for Set Compression depends on which state you are in when it is sent (**W**).
* The **Play-state Set Compression (0x46)** exists but is effectively broken/deprecated in 1.8 and was
  removed in 1.9; do not use it. (**W**)
* Compression is per-connection, not per-packet: once enabled it stays enabled for Play.

Sources: https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Definitions, §Packet format, §Login) ·
https://minecraft.wiki/w/Server.properties (network-compression-threshold) ·
vanilla 1.8.9 `NetHandlerLoginServer` / `NetworkManager` (`Marcelektro/MCP-919`).

### 1.3 Handshake + Status (server list ping)

| # | State | Dir | Packet | Fields |
|---|---|---|---|---|
| 0x00 | Handshaking | S→C | Handshake | ProtocolVersion VarInt (**47 for 1.8.9**); ServerAddress String; ServerPort Unsigned Short; NextState VarInt (**1 = status, 2 = login**) |
| 0xFE | Handshaking | S→C | Legacy Server List Ping | Payload Unsigned Byte = 1 (obsolete pre-Netty path; ignore for 1.8.9) |

Status exchange (all in Status state, ids repeat the handshake numbering reset):

| # | Dir | Packet | Fields |
|---|---|---|---|
| 0x00 | C→S | Request | — |
| 0x00 | S→C | Response | JSONResponse String |
| 0x01 | C→S | Ping | Payload Long (client uses a millisecond timestamp) |
| 0x01 | S→C | Pong | Payload Long (echo) |

Status response JSON shape (fields are a JSON object; `description` is a Chat object, not a bare string):

```json
{
  "version":  { "name": "1.8.9", "protocol": 47 },
  "players":  { "max": 100, "online": 5,
                "sample": [ { "name": "player", "id": "4566e69f-c907-48ee-8d71-000000000000" } ] },
  "description": { "text": "A Minecraft Server" },
  "favicon": "data:image/png;base64,<base64 PNG>"
}
```

`players.sample` and `favicon` are optional (`favicon` must be a base64 PNG prefixed with
`data:image/png;base64,`). The server may answer the ping with any protocol number; a 1.8.9 client should
tolerate mismatched `protocol` and just display `version.name`.

A **1.8.9 client** should send: Handshake(next=1) → Request → (read Response) → Ping(now_ms) → (read Pong).
Then, still on the same connection, the client re-sends **Handshake with next=2** to start login.

Sources: https://web.archive.org/web/20160201000000/https://wiki.vg/Server_List_Ping ·
https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Handshaking/§Status).

### 1.4 Login state and encryption

Sequence: `Handshake(next=2)` → `Login Start` → *(Encryption Request → client auth → Encryption Response →
both sides enable encryption)* → **`Set Compression` (login 0x03)** → **`Login Success` (login 0x02)** → Play.
For unauthenticated or localhost connections the server skips encryption entirely and goes straight to Login Success.

| # | Dir | Packet | Fields |
|---|---|---|---|
| 0x00 | S→C | Disconnect | Reason Chat |
| 0x01 | S→C | Encryption Request | ServerID String (empty in 1.7+); PublicKeyLength VarInt; PublicKey ByteArray (X.509/ASN.1 DER); VerifyTokenLength VarInt; VerifyToken ByteArray (4 bytes in vanilla) |
| 0x02 | S→C | Login Success | UUID **String with hyphens** (unlike the 16-byte UUID used in play packets); Username String |
| 0x03 | S→C | Set Compression | Threshold VarInt |
| 0x00 | C→S | Login Start | Name String (≤16 chars) |
| 0x01 | C→S | Encryption Response | SharedSecretLength VarInt; SharedSecret ByteArray (RSA-encrypted); VerifyTokenLength VarInt; VerifyToken ByteArray (RSA-encrypted) |

Crypto details (vanilla 1.8.9 `CryptManager`, **V**; semantics **W-ENC**):

* Server key: **RSA 1024-bit** (`KeyPairGenerator.getInstance("RSA")`, `initialize(1024)`); the public key on
  the wire is the X.509 (`SubjectPublicKeyInfo`) DER encoding.
* Client: generate a **16-byte AES shared secret** (`KeyGenerator.getInstance("AES")` → 128-bit).
* Client RSA-encrypts the shared secret *and* the verify token with the server public key. Vanilla calls
  `Cipher.getInstance("RSA")`, which the JCE resolves to **RSA/ECB/PKCS1Padding** (PKCS#1 v1.5) — implement
  with PKCS#1 v1.5 padding, not OAEP.
* Stream cipher after login: **AES/CFB8/NoPadding**, key = shared secret, **IV = the shared secret itself**
  (vanilla: `new IvParameterSpec(key.getEncoded())`). Both directions use the same key; do not re-key.
* Encryption starts after the server reads Encryption Response (client enables after sending it): every byte
  after that point is AES-encrypted at the TCP layer, *inside* which the normal compression framing applies.
* Offline-mode / `online-mode=false` servers may skip the request entirely; also note that a client must
  handle Encryption Request arriving at any time before Login Success.

Sources: https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol_Encryption ·
https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Login) ·
vanilla 1.8.9 `CryptManager`, `NetHandlerLoginServer`, `NetHandlerPlayClient` (`MCP-919`).

### 1.5 Version numbering: what "1.8.9" means

* **Protocol 47 covers 1.8 → 1.8.9.** The protocol-version table lists 1.8.9 = 47 and leaves the protocol
  column empty/merged for 1.8.8 … 1.8, i.e. all those releases share it. 1.8.9 itself was bug/security
  fixes only. **There are no packet-id or field differences between 1.8.0, 1.8.8 and 1.8.9** — one codec set
  covers the whole 1.8.x line. (Package-name detail only: client brands differ, e.g. `1.8.9`.)
* The next protocol number after 47 is **48** (15w14a snapshot); 1.9 proper is **107**. Everything in §7
  below describes the cliff at 1.9.
* Vanilla server brand strings: `vanilla` (both directions via `MC|Brand`).

Sources: https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/Protocol_version_numbers ·
https://c4k3.github.io/wiki.vg/Protocol_History.html

---

## 2. Play-state packet tables (protocol 47)

74 clientbound ids (0x00–0x49), 26 serverbound ids (0x00–0x19). The full id → name map below was
machine-verified against `minecraft-data` 1.8 (`data/pc/1.8/protocol.json`, protocol 47) — every id matches
wiki.vg exactly, so the numbering is safe to hard-code.

**MVP column**: `core` = required to reach and survive in the world · `need` = required shortly after
(rendering players/entities, inventories, health) · `opt` = cosmetic/optional for a first client.

### 2.0 Functional index (what to implement together)

| Function | Packets |
|---|---|
| Session / keepalive | C 0x00 Keep Alive · S 0x00 Keep Alive · C 0x40 Disconnect · C 0x46 Set Compression (unused) |
| Login/spawn entry | C 0x01 Join Game · C 0x41 Server Difficulty · C 0x05 Spawn Position · C 0x08 Player Position And Look · C 0x39 Player Abilities · C 0x38 Player List Item · C 0x3F Plugin Message (`MC\|Brand`) · S 0x15 Client Settings · S 0x17 Plugin Message |
| Movement / self state | S 0x03 Player · S 0x04 Player Position · S 0x05 Player Look · S 0x06 Player Position And Look · C 0x08 · C 0x06 Update Health · C 0x07 Respawn · C 0x1F Set Experience · S 0x0B Entity Action · S 0x13 Player Abilities |
| Terrain | C 0x21 Chunk Data · C 0x26 Map Chunk Bulk · C 0x22 Multi Block Change · C 0x23 Block Change · C 0x24 Block Action · C 0x25 Block Break Animation · C 0x35 Update Block Entity · C 0x33 Update Sign · C 0x36 Open Sign Editor · S 0x07 Player Digging · S 0x08 Player Block Placement · S 0x12 Update Sign |
| Entities | C 0x0C Spawn Player · C 0x0E Spawn Object · C 0x0F Spawn Mob · C 0x10 Spawn Painting · C 0x11 Spawn Experience Orb · C 0x2C Spawn Global Entity · C 0x14 Entity · C 0x15 Entity Relative Move · C 0x16 Entity Look · C 0x17 Entity Look And Relative Move · C 0x18 Entity Teleport · C 0x19 Entity Head Look · C 0x12 Entity Velocity · C 0x13 Destroy Entities · C 0x1C Entity Metadata · C 0x49 Update Entity NBT · C 0x1A Entity Status · C 0x1B Attach Entity · C 0x0D Collect Item · C 0x04 Entity Equipment · C 0x20 Entity Properties · C 0x0B Animation · S 0x02 Use Entity · S 0x0A Animation · S 0x0C Steer Vehicle · S 0x18 Spectate · C 0x43 Camera |
| Effects / status effects | C 0x1D Entity Effect · C 0x1E Remove Entity Effect · C 0x27 Explosion · C 0x28 Effect · C 0x29 Sound Effect · C 0x2A Particle · C 0x2B Change Game State (weather, gamemode, fade, credits) · C 0x42 Combat Event |
| Time / world | C 0x03 Time Update · C 0x41 Difficulty · C 0x44 World Border |
| Inventory / windows | C 0x2D Open Window · C 0x2E Close Window · C 0x2F Set Slot · C 0x30 Window Items · C 0x31 Window Property · C 0x32 Confirm Transaction · S 0x0D Close Window · S 0x0E Click Window · S 0x0F Confirm Transaction · S 0x10 Creative Inventory Action · S 0x11 Enchant Item · S 0x09 Held Item Change · C 0x09 Held Item Change · S 0x16 Client Status (open inventory achievement) |
| Chat / tab / UI | C 0x02 Chat Message · S 0x01 Chat Message · C 0x47 Player List Header And Footer · C 0x45 Title · C 0x3A / S 0x14 Tab-Complete · C 0x48 Resource Pack Send · S 0x19 Resource Pack Status |
| Scoreboard / teams | C 0x3B Scoreboard Objective · C 0x3C Update Score · C 0x3D Display Scoreboard · C 0x3E Teams |
| Maps / statistics | C 0x34 Map · C 0x37 Statistics · S 0x16 Client Status (request stats) |
| Misc/legacy | C 0x0A Use Bed · C 0x46 Set Compression (broken) |

Note: because packet ids are the natural key for a codec, §2.1/§2.2 are ordered by id; use this index when
implementing a whole feature at once. Every server packet must be *decodable* (a VarInt id switch with a
"unknown ⇒ skip/save the payload" fallback, since the payload length is known), even if you ignore most of
them.

### 2.1 Clientbound (server → client)

| ID | Name | Fields (types) | MVP |
|---|---|---|---|
| 0x00 | Keep Alive | KeepAliveID VarInt (client must echo it in 0x00 serverbound) | core |
| 0x01 | Join Game | EID Int; Gamemode UByte (0 survival, 1 creative, 2 adventure, 3 spectator; bit 0x8 = hardcore); Dimension Byte (−1 nether, 0 overworld, 1 end); Difficulty UByte; MaxPlayers UByte; LevelType String (`default`/`flat`/`largeBiomes`/`amplified`/`default_1_1`); ReducedDebugInfo Bool | core |
| 0x02 | Chat Message | JSONData Chat; Position Byte (0 chat, 1 system, 2 above hotbar) | core |
| 0x03 | Time Update | WorldAge Long; TimeOfDay Long (negative = sun frozen) | need |
| 0x04 | Entity Equipment | EID VarInt; Slot Short (0 held, 1 boots, 2 leggings, 3 chestplate, 4 helmet); Item Slot | need |
| 0x05 | Spawn Position | Location Position (compass target) | need |
| 0x06 | Update Health | Health Float; Food VarInt (0–20); Saturation Float | core |
| 0x07 | Respawn | Dimension Int; Difficulty UByte; Gamemode UByte; LevelType String (client must clear world + reply 0x16 Client Status respawn) | core |
| 0x08 | Player Position And Look | X/Y/Z Double; Yaw/Pitch Float; Flags Byte (0x01 X, 0x02 Y, 0x04 Z, 0x08 Y_ROT, 0x10 X_ROT — set bit = value is a **relative delta**) | core |
| 0x09 | Held Item Change | Slot Byte (0–8) | need |
| 0x0A | Use Bed | EID VarInt; Location Position (head of bed) | opt |
| 0x0B | Animation (cb) | EID VarInt; Animation UByte (0 swing arm, 1 damage, 2 leave bed, 3 eat/food, 4 crit, 5 magic crit) | opt |
| 0x0C | Spawn Player | EID VarInt; PlayerUUID UUID(16B); X/Y/Z Int fixed-point; Yaw/Pitch Angle; CurrentItem Short; Metadata | need |
| 0x0D | Collect Item | CollectedEID VarInt; CollectorEID VarInt | opt |
| 0x0E | Spawn Object | EID VarInt; Type Byte; X/Y/Z Int fixed-point; Pitch/Yaw Angle; Data Int; VelocityX/Y/Z Short **(present only if Data ≠ 0)** | need |
| 0x0F | Spawn Mob | EID VarInt; Type UByte; X/Y/Z Int fixed-point; Yaw/Pitch/HeadPitch Angle; VelocityX/Y/Z Short; Metadata | need |
| 0x10 | Spawn Painting | EID VarInt; Title String (≤13); Location Position (center); Direction UByte (0 −Z, 1 −X, 2 +Z, 3 +X) | opt |
| 0x11 | Spawn Experience Orb | EID VarInt; X/Y/Z Int fixed-point; Count Short | opt |
| 0x12 | Entity Velocity | EID VarInt; VelocityX/Y/Z Short | opt |
| 0x13 | Destroy Entities | Count VarInt; EntityIDs Array of VarInt | core |
| 0x14 | Entity | EID VarInt (keep-alive/no-op for an entity; confirms existence) | need |
| 0x15 | Entity Relative Move | EID VarInt; dX/dY/dZ Byte (1/32 block); OnGround Bool | core |
| 0x16 | Entity Look | EID VarInt; Yaw/Pitch Angle; OnGround Bool | core |
| 0x17 | Entity Look And Relative Move | EID VarInt; dX/dY/dZ Byte; Yaw/Pitch Angle; OnGround Bool | core |
| 0x18 | Entity Teleport | EID VarInt; X/Y/Z Int fixed-point; Yaw/Pitch Angle; OnGround Bool | core |
| 0x19 | Entity Head Look | EID VarInt; HeadYaw Angle | opt |
| 0x1A | Entity Status | EID Int; Status Byte (2 hurt, 3 dead, 6/7 taming, 9 eat accepted, 10 grass, 14 zombie villager, …) | opt |
| 0x1B | Attach Entity | EID Int; VehicleID Int (−1 = detach); Leash Bool | opt |
| 0x1C | Entity Metadata | EID VarInt; Metadata (§6) | need |
| 0x1D | Entity Effect | EID VarInt; EffectID Byte; Amplifier Byte; Duration VarInt (seconds); HideParticles Bool | opt |
| 0x1E | Remove Entity Effect | EID VarInt; EffectID Byte | opt |
| 0x1F | Set Experience | XPBar Float (0–1); Level VarInt; TotalXP VarInt | need |
| 0x20 | Entity Properties | EID VarInt; Count Int; then per property: Key String, Value Double, ModifierCount VarInt, per modifier: UUID, Amount Double, Operation Byte (0 add, 1 mul-add, 2 mul) | need |
| 0x21 | **Chunk Data** | ChunkX Int; ChunkZ Int; GroundUpContinuous Bool; PrimaryBitMask UnsignedShort; Size VarInt; Data byte[Size] → §3.1 | core |
| 0x22 | **Multi Block Change** | ChunkX Int; ChunkZ Int; RecordCount VarInt; per record: 2 bytes read as a big-endian short → high byte = `(xInChunk << 4) \| zInChunk`, low byte = `y`; then BlockID VarInt (`id << 4 \| meta`) | core |
| 0x23 | **Block Change** | Location Position; BlockID VarInt (`id << 4 \| meta`) | core |
| 0x24 | Block Action | Location Position; Byte1 UByte; Byte2 UByte; BlockType VarInt (chest open/close, note block, piston, beacon) | opt |
| 0x25 | Block Break Animation | EID VarInt; Location Position; DestroyStage Byte (0–9, else remove) | opt |
| 0x26 | **Map Chunk Bulk** | SkyLightSent Bool; ChunkColumnCount VarInt; per chunk metadata: ChunkX Int, ChunkZ Int, PrimaryBitMask UnsignedShort; then per chunk: Data (no length prefix — size is implied) → §3.2 | core |
| 0x27 | Explosion | X/Y/Z Float; Radius Float; RecordCount Int; Records byte[3] each (signed x/y/z offsets); PlayerMotionX/Y/Z Float | opt |
| 0x28 | Effect | EffectID Int; Location Position; Data Int; DisableRelativeVolume Bool (record-break 2000+, door 1003/1006/1007, particles 2000–2007) | opt |
| 0x29 | Sound Effect | SoundName String; X/Y/Z Int (**×8** fixed); Volume Float; Pitch UByte (63 = 100%) | need |
| 0x2A | Particle | ParticleID Int; LongDistance Bool; X/Y/Z Float; OffsetX/Y/Z Float; ParticleData Float; Count Int; Data Array of VarInt (depends on id) | opt |
| 0x2B | Change Game State | Reason UByte (0 invalid bed, 1 end raining, 2 begin raining, 3 change gamemode, 4 credits, 5 demo, 6 arrow hit, 7 fade value, 8 fade time, 10 mob appearance); Value Float — **"begin/end raining" lives here** | need |
| 0x2C | Spawn Global Entity | EID VarInt; Type Byte (1 lightning); X/Y/Z Int fixed-point | opt |
| 0x2D | Open Window | WindowID UByte; WindowType String (e.g. `minecraft:chest`, `minecraft:anvil`, `EntityHorse`); WindowTitle Chat; SlotCount UByte; EntityID Int **only if** WindowType = `EntityHorse` | need |
| 0x2E | Close Window | WindowID UByte | need |
| 0x2F | Set Slot | WindowID Byte; Slot Short; SlotData Slot (a negative slot index denotes the cursor in later docs — ⚠ not verified against 1.8.9 source) | need |
| 0x30 | Window Items | WindowID UByte; Count Short; Slots Array of Slot (index 0..count−1; 0–4 crafting, 5–8 armor, 9–35 main, 36–44 hotbar for player inventory) | need |
| 0x31 | Window Property | WindowID UByte; Property Short; Value Short (furnace progress/fuel, enchant levels, brewing) | opt |
| 0x32 | Confirm Transaction | WindowID Byte; ActionNumber Short; Accepted Bool | need |
| 0x33 | Update Sign | Location Position; Line1..4 Chat | opt |
| 0x34 | Map | ItemDamage VarInt; Scale Byte; IconCount VarInt; Icons[DirectionAndType Byte, X Byte, Z Byte]; Columns Byte; if Columns > 0: Rows Byte, X Byte, Z Byte, then Columns×Rows color bytes | opt |
| 0x35 | Update Block Entity | Location Position; Action UByte (1 mob spawner, 2 command block, 3 beacon, 4 skull, 5 flower pot, 6 banner, 7 structure, 8 end gateway, 9 sign); NBT Tag (absent ⇒ TAG_End) | opt |
| 0x36 | Open Sign Editor | Location Position | opt |
| 0x37 | Statistics | Count VarInt; per entry: Name String (e.g. `stat.walkOneCm`), Value VarInt | opt |
| 0x38 | Player List Item | Action VarInt (0 add, 1 gamemode, 2 latency, 3 display name, 4 remove); NumberOfPlayers VarInt; per player: UUID; then per action — 0: Name String, PropertyCount VarInt, [Name, Value, IsSigned, (Signature)], Gamemode VarInt, Ping VarInt, HasDisplayName Bool, (DisplayName Chat); 1: Gamemode VarInt; 2: Ping VarInt; 3: HasDisplayName Bool, (DisplayName Chat); 4: nothing | core |
| 0x39 | Player Abilities | Flags Byte (0x01 invulnerable, 0x02 flying, 0x04 allow flying, 0x08 creative); FlyingSpeed Float; WalkingSpeed Float (FOV modifier) | core |
| 0x3A | Tab-Complete (cb) | Count VarInt; Matches Array of String | opt |
| 0x3B | Scoreboard Objective | ObjectiveName String; Mode Byte (0 create, 1 remove, 2 update); if mode 0/2: ObjectiveValue String, Type String (`integer`\|`hearts`) | opt |
| 0x3C | Update Score | ScoreName String; Action Byte (0 create/update, 1 remove); ObjectiveName String; if action ≠ 1: Value VarInt | opt |
| 0x3D | Display Scoreboard | Position Byte (0 list, 1 sidebar, 2 below name); ScoreName String | opt |
| 0x3E | Teams | TeamName String; Mode Byte (0 create, 1 remove, 2 update info, 3 add players, 4 remove players); if 0/2: DisplayName String, Prefix String, Suffix String, FriendlyFire Byte (0/1/2), NameTagVisibility String (`always`/`hideForOtherTeams`/`hideForOwnTeam`/`never`), Color Byte (0 black…). If 3/4: PlayerCount VarInt + Players Array of String | opt |
| 0x3F | Plugin Message | Channel String; Data Byte Array | core |
| 0x40 | Disconnect | Reason Chat | core |
| 0x41 | Server Difficulty | Difficulty UByte | need |
| 0x42 | Combat Event | Event VarInt (0 enter combat, 1 end combat, 2 entity dead); 1: Duration VarInt, EntityID Int; 2: PlayerID VarInt, EntityID Int, Message String | opt |
| 0x43 | Camera | CameraID VarInt (spectator camera target) | opt |
| 0x44 | World Border | Action VarInt (0 set size → Radius Double; 1 lerp → OldRadius, NewRadius Double, Speed VarLong; 2 set center → X/Z Double; 3 initialize → X/Z, OldRadius, NewRadius Double, Speed VarLong, PortalBoundary VarInt; 4 warn → WarningTime; 5 warn distance) | opt |
| 0x45 | Title | Action VarInt (0 title → Chat; 1 subtitle → Chat; 2 times → FadeIn Int, Stay Int, FadeOut Int; 3 hide; 4 reset) | opt |
| 0x46 | Set Compression (play) | Threshold VarInt — **broken in 1.8, do not use** | opt |
| 0x47 | Player List Header And Footer | Header Chat; Footer Chat | opt |
| 0x48 | Resource Pack Send | URL String; Hash String (40-char lowercase hex SHA-1) | need |
| 0x49 | Update Entity NBT | EID VarInt; NBT Tag | opt |

### 2.2 Serverbound (client → server)

| ID | Name | Fields | MVP |
|---|---|---|---|
| 0x00 | Keep Alive | KeepAliveID VarInt | core |
| 0x01 | Chat Message | Message String (≤100 chars, raw text not JSON) | core |
| 0x02 | Use Entity | Target VarInt; Type VarInt (0 interact, 1 attack, 2 interact at); if type 2: TargetX/Y/Z Float | need |
| 0x03 | Player | OnGround Bool (position-only "nothing changed" tick, also used to confirm velocity) | core |
| 0x04 | Player Position | X Double; FeetY Double; Z Double; OnGround Bool | core |
| 0x05 | Player Look | Yaw Float; Pitch Float; OnGround Bool | core |
| 0x06 | Player Position And Look | X Double; FeetY Double; Z Double; Yaw Float; Pitch Float; OnGround Bool (**reply to clientbound 0x08**) | core |
| 0x07 | Player Digging | Status Byte (0 start, 1 cancel, 2 finish, 3 drop stack, 4 drop item, 5 shoot/finish eating); Location Position; Face Byte (0 −Y, 1 +Y, 2 −Z, 3 +Z, 4 −X, 5 +X) | core |
| 0x08 | Player Block Placement | Location Position; Face Byte; HeldItem Slot; CursorX/Y/Z Byte (0–15) | need |
| 0x09 | Held Item Change | Slot Short (0–8) | need |
| 0x0A | Animation (sb) | *(no fields)* — swing arm | need |
| 0x0B | Entity Action | EID VarInt; ActionID VarInt (0 crouch, 1 uncrouch, 2 leave bed, 3 start sprinting, 4 stop sprinting, 5 jump horse, 6 open inventory); JumpBoost VarInt (horse 0–100) | need |
| 0x0C | Steer Vehicle | Sideways Float; Forward Float; Flags UByte (0x1 jump, 0x2 unmount) | opt |
| 0x0D | Close Window | WindowID UByte | need |
| 0x0E | Click Window | WindowID UByte; Slot Short; Button Byte; ActionNumber Short; Mode Byte (0 normal, 1 shift-click, 2 number key, 3 middle, 4 drop, 5 drag, 6 double-click); ClickedItem Slot | need |
| 0x0F | Confirm Transaction | WindowID Byte; ActionNumber Short; Accepted Bool | need |
| 0x10 | Creative Inventory Action | Slot Short; ClickedItem Slot | opt |
| 0x11 | Enchant Item | WindowID Byte; Enchantment Byte (0-based slot index) | opt |
| 0x12 | Update Sign | Location Position; Line1..4 Chat | opt |
| 0x13 | Player Abilities | Flags Byte; FlyingSpeed Float; WalkingSpeed Float | need |
| 0x14 | Tab-Complete | Text String (all text behind cursor); HasPosition Bool; if true: LookedAtBlock Position | opt |
| 0x15 | Client Settings | Locale String (≤7); ViewDistance Byte; ChatMode Byte (0 enabled, 1 commands only, 2 hidden); ChatColors Bool; DisplayedSkinParts UByte (0x01 cape, 0x02 jacket, 0x04 left sleeve, 0x08 right sleeve, 0x10 left pants, 0x20 right pants, 0x40 hat) | core |
| 0x16 | Client Status | ActionID VarInt (0 perform respawn, 1 request stats, 2 open inventory achievement) | core |
| 0x17 | Plugin Message | Channel String; Data Byte Array | core |
| 0x18 | Spectate | TargetPlayer UUID | opt |
| 0x19 | Resource Pack Status | Hash String; Result VarInt (0 loaded, 1 declined, 2 failed, 3 accepted) | need |

### 2.3 Minimum viable client (ordering notes)

1. Handshake → Login → (encryption) → await **Login Success** → Play.
2. On **Join Game (0x01)**: store EID, gamemode, dimension (dimension decides whether sky light exists, §3/§4).
3. Send **Client Settings (0x15)** and **Plugin Message `MC|Brand`** early (vanilla-esque behaviour).
4. Expect **Server Difficulty (0x41)**, **Player Abilities (0x39)**, **Spawn Position (0x05)**, then
   **Player Position And Look (0x08)** — the client *must* answer 0x08 with serverbound **0x06** (same
   coordinates) or the server will keep teleporting it.
5. Terrain arrives as **0x21 / 0x26**; the world becomes renderable once the chunk containing the player is
   loaded. Until then the client shows "Downloading terrain".
6. Start a keep-alive loop: answer every clientbound **0x00** with serverbound **0x00** (vanilla sends one
   every ~1 s and disconnects after ~30 s of silence).
7. Movement loop: send serverbound **0x04 / 0x05 / 0x06** at ~20 Hz (position+look each tick is simplest),
   or **0x03** when nothing changed. Servers may not require exact 20 Hz, but 1.8 vanilla only accepts one
   position packet per tick and will "correct" with 0x08 otherwise.
8. **Player List Item (0x38, action 0)** must be processed before **Spawn Player (0x0C)** for the same
   UUID, or the entity will not be spawned. (**W**)
9. Client is responsible for unloading chunks: vanilla sends **0x21 with GroundUpContinuous = true and
   PrimaryBitMask = 0** for a chunk the player stops watching (**V**), which means "this chunk is now empty".

Sources: https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Play) ·
https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/pc/1.8/protocol.json ·
vanilla 1.8.9 `net.minecraft.server.management.PlayerManager` (`MCP-919`).

---

## 3. Chunk / terrain encoding

### 3.1 Chunk Data — clientbound 0x21

Wire layout (**V**, matches **W**):

```
Int          ChunkX
Int          ChunkZ
Boolean      GroundUpContinuous   // "full chunk": sections listed are complete, unlisted = air, biomes present
UnsignedShort PrimaryBitMask      // bit i set => section i (y = 16*i .. 16*i+15) has data in this packet
VarInt       Size                 // length of Data in bytes (1.8: VarInt; 1.9+ changed to VarInt/Int variants)
byte[Size]   Data
```

`Data` layout (this is *the* thing to get right; verified against vanilla `S21PacketChunkData.getExtractedData`
and the client's `Chunk.fillChunk`):

| Order | Content | Size per included section | Notes |
|---|---|---|---|
| 1 | **Block data, all included sections, ascending Y** | 4096 × **u16 little-endian** = 8192 B | value = `(blockId << 4) \| meta`, i.e. `byte[0] \| (byte[1] << 8)`; `id = v >> 4`, `meta = v & 0xF`. 1.8 removed the separate Metadata and "Add"/extended-id arrays: the block id is a full 12-bit id |
| 2 | **Block light, all included sections, ascending Y** | 2048 B (nibbles) | present for every bit in the mask |
| 3 | **Sky light, all included sections, ascending Y** | 2048 B (nibbles) | present **only if the dimension has sky** (`!provider.getHasNoSky()` ⇒ Overworld). Never sent for Nether/End |
| 4 | **Biomes** | 256 B (1 byte per column) | present **only if GroundUpContinuous = true**; index = `z * 16 + x` |

Byte math: `Size = 8192*N + 2048*N + (sky ? 2048*N : 0) + (groundUp ? 256 : 0)` where `N = popcount(mask)`.
Overworld: `12288*N (+256)`; Nether/End: `10240*N (+256)`. **There is no per-packet "add bitmask" in 1.8** —
if you see code expecting one, it is 1.7-era.

Index order inside a section/array: **X varies fastest, then Z, then Y**, i.e. linear index
`= (y << 8) | (z << 4) | x` (vanilla `ExtendedBlockStorage.get()` / `NibbleArray.getCoordinateIndex()`).
The 4096-entry block array is ordered identically.

GroundUpContinuous semantics (**V**):

* `true` — the listed sections are the *complete* content of the column; sections not in the mask become air,
  and the client nulls out its existing sections not in the mask. Biomes are included. Empty sections are
  normally omitted (server skips `ExtendedBlockStorage.isEmpty()` sections when it is a full send).
* `false` — "big Multi Block Change": only the listed sections are replaced; everything else is untouched
  and **biomes are absent**. Vanilla uses this when 64+ blocks change in one chunk (see §3.3).
* **Unload trick:** `GroundUpContinuous = true, PrimaryBitMask = 0, Size = 0` ⇒ "this column is empty",
  which a 1.8 client treats as unloaded. This is how vanilla tells a client it stopped watching a chunk.

### 3.2 Map Chunk Bulk — clientbound 0x26

Wire layout (**V**, matches **W**/**W-SMP**):

```
Boolean  SkyLightSent                     // one flag for the whole packet (Overworld = true)
VarInt   ChunkColumnCount                 // number of columns
repeat ChunkColumnCount:                  // metadata block, all columns first
    Int          ChunkX
    Int          ChunkZ
    UnsignedShort PrimaryBitMask
repeat ChunkColumnCount:                  // then payload block, same order
    byte[] Data                           // NO length prefix: size derived from the mask+skyLightSent
```

* Each column's `Data` uses the exact same ordering as 0x21 **with GroundUpContinuous = true implied**:
  blocks → block light → sky light (if `SkyLightSent`) → **biomes always included**. (**W-SMP**: "Continuous?
  (only in 0x21 — assumed true in 0x26)"; **V**: vanilla builds bulk entries via
  `getExtractedData(chunk, true, isOverworld, 65535)`, i.e. full=true ⇒ biomes present.)
* Vanilla's writer emits mask-filtered, non-empty sections per column (`dataSize` = that column's mask).
* **When servers use it:** bulk is the batch path used to send many columns at once — initially when the
  player first spawns into a world and when the player is teleported (i.e. the login/mass-load burst); 0x21
  is used for per-chunk updates, chunk unloading and section/light updates. (**W-SMP**; vanilla's 1.8
  PlayerManager uses 0x21 for its per-chunk and section updates, and only the mass path batches. ⚠ The exact
  vanilla call site that emits 0x26 was not located in the sources inspected — NeoForge/Spigot forks and
  plugins also send 0x26, so a client must implement both regardless.)
* Because there is no per-column length field, you **must** compute each column's payload size from its
  mask + the packet-level sky-light flag, or you will desynchronise the stream.

### 3.3 Block updates and light after chunk load

| Packet | Trigger in vanilla 1.8.9 | Light implication |
|---|---|---|
| 0x23 Block Change | 1 block changed | carries **no** light data → client recomputes locally |
| 0x22 Multi Block Change | 2–63 blocks changed in one chunk | no light data → client recomputes locally |
| 0x21 Chunk Data with `GroundUpContinuous = false` | the chunk's pending-change counter reaches its 64-entry cap (`numBlocksToUpdate == 64`), i.e. "section update" | **includes block light + sky light for the listed sections** → this is how 1.8 servers push light updates without resending a full chunk |

Emit order note: vanilla's `PlayerManager.Entry.onUpdate()` sends 0x23 for a single change, 0x21(section mask)
for a full 64-change batch, otherwise 0x22. (**V**)

### 3.4 Nibble order — three conventions, get this right

| Context | Rule | Source |
|---|---|---|
| **1.8 wire** (block light / sky light nibble arrays inside 0x21/0x26) | *vanilla* `NibbleArray`: `isLowerNibble(index) = (index & 1) == 0` → **even index = low nibble**, odd = high nibble; index = `(y<<8)\|(z<<4)\|x` | **V** (`NibbleArray.getFromIndex`) |
| **Anvil disk** (Data, Add, BlockLight, SkyLight) | same as vanilla: even = low nibble, odd = high nibble (`index%2==0 ? arr[index/2] & 0x0F : arr[index/2] >> 4`) | **MCW** |
| wiki.vg `SMP Map Format` prose | claims the opposite ("even-indexed … high bits") — treat as an error | **W-SMP** |
| PrismarineJS `prismarine-chunk` 1.8 (used by mineflayer) | reads/writes 4-bit values with `uint4`'s **LE** variant, i.e. even = high nibble — the **opposite** of vanilla | implementation cross-check |

**Recommendation:** follow the vanilla rule (**V**) — it is what the Notchian client, server, and the Anvil
on-disk format all use. If you interoperate with a parser derived from prismarine-chunk and see swapped
light values, this table is why. `⚠ UNVERIFIED` whether prismarine's 1.8 path is a long-standing bug or
compensated elsewhere in that stack; the vanilla source is unambiguous.

Sources: vanilla 1.8.9 `S21PacketChunkData`, `Chunk.fillChunk`, `NibbleArray`, `ExtendedBlockStorage`,
`PlayerManager` (`MCP-919`) ·
https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Chunk Data/§Map Chunk Bulk) ·
https://web.archive.org/web/20151022204544/https://wiki.vg/SMP_Map_Format (oldid 6909) ·
https://minecraft.wiki/w/Chunk_format/Anvil/Before_1.13 (nibble pseudo-code) ·
https://github.com/PrismarineJS/prismarine-chunk (`src/pc/1.8/*`, `uint4`).

---

## 4. Lighting semantics (1.8.9)

### 4.1 The model

* Two independent 4-bit light values per block: **sky light** and **block light**, each 0–15. "Client light"
  = `max(sky light, block light)`; rendering uses client light plus time-of-day/weather darkening
  ("internal sky light"), which is **not** sent over the wire. (**MCW**)
* **Block light**: emitted by blocks (torch 14, glowstone 15, lava 15, …) and **decreases by 1 per block of
  taxicab distance** from the source, spreading in all 6 directions. (**MCW**)
* **Sky light**: blocks vertically exposed to the sky have sky light 15. Propagation rules:
  * full-strength (15) sky light propagating **downward** through a transparent block does **not** decrease;
  * propagating **horizontally or upward** (and any sky light < 15 spreading to neighbours) **decreases by 1**;
  * opaque blocks block propagation; "light-filtering" blocks (water, ice, leaves, cobwebs, …) reduce sky
    light by exactly 1 in Java Edition;
  * sky light is not reduced at night — day/night only affects the derived internal sky light/brightness. (**MCW**)
* 1.8.9 has **no flood-fill smoothing of the stored values** and no "smoothness" data on the wire; the
  stored nibbles are the raw level values. **Smooth lighting** is a purely client-side *render* effect:
  bilinear interpolation of the 4 light samples around a face plus **ambient occlusion** darkening of
  corners. It is a video setting (off/on), and it changes nothing about the data. (**MCW**)

### 4.2 What actually arrives over the wire

| Data | Arrives via | Notes |
|---|---|---|
| Block light nibbles | **0x21** and **0x26** only | one 2048-byte array per included section |
| Sky light nibbles | **0x21** and **0x26** only, and only when the dimension has sky | Overworld: yes (both packets carry an explicit flag); Nether/End: no sky light at all |
| Light updates for a block change | **0x21 with `GroundUpContinuous = false`** (section update), or client-side recomputation for 0x22/0x23 | there is no dedicated light packet anywhere in protocol 47 |
| Biome array | **0x21** (only when ground-up continuous) / **0x26** (always) | index `z*16 + x`, 256 bytes |

Consequences for an implementation:

1. **Never trust absent light data.** Sections that are not in the primary bitmask are assumed by the vanilla
   client to be *block light 0 and sky light 15* — which is wrong underground (see MC-80966). If you load a
   save or receive a partial update, either keep the previous light for that section or recompute. (**W**/**MCW**)
2. **Do local light propagation for 0x22/0x23.** Vanilla's client owns a full lighting engine and
   recalculates affected light on block changes/placements; the packets carry no light. A minimal client can
   skip this (accepting dark/bright artifacts), but chunk section updates (0x21 with mask) should always be
   applied, as that is the server's own correction path.
3. **Sky-light-absent dimensions** (Nether/End) must be flagged from Join Game's `Dimension` (or Respawn) so
   your decoder never expects sky-light arrays.
4. Since 1.14 the protocol has dedicated Update Light packets and chunk data no longer carries light — do not
   mix reference material from 1.14+ with this format.

Sources: https://minecraft.wiki/w/Light (levels, propagation, filtering, smooth lighting/ambient occlusion) ·
https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Chunk Data — "server does not
send skylight information for nether-chunks") · https://minecraft.wiki/w/Chunk_format (empty-section light
assumption, MC-80966) · vanilla 1.8.9 `S21PacketChunkData` / `Chunk.fillChunk` / `PlayerManager` (`MCP-919`) ·
https://c4k3.github.io/wiki.vg/Protocol_History.html (1.14 light packets).

---

## 5. Anvil region format (singleplayer / world loading)

1.8.9 (and every version from Beta 1.3 through 1.12) stores chunks in **region files** `r.<X>.<Z>.mca`,
each holding a 32×32 chunk area.

### 5.1 .mca container

| Offset | Content |
|---|---|
| 0x0000 | **Location table** — 1024 × big-endian u32. Entry index `i = x + 32*z` (x, z = chunk coords mod 32, floored). 3 high bytes = sector offset from file start, low byte = sector count (**max 255 sectors = 1020 KiB**) |
| 0x1000 | **Timestamp table** — 1024 × big-endian u32 (epoch seconds of last save); a zero timestamp means the slot is unused |
| 0x2000+ | Chunk payloads, **4 KiB sector aligned** |

Payload per chunk: `Int32 length (big-endian)` = bytes of the remaining payload **including** the compression
byte; `UByte compressionType`; then `length − 1` bytes of data.

| compressionType | Method |
|---|---|
| 1 | GZip (legacy; unused in practice for .mca) |
| 2 | **Zlib** (what 1.8.9 writes/expects) |
| 3 | Uncompressed (later versions; accept it for robustness) |
| 4 / 127 | LZ4 / custom namespaced codec — post-1.8 extensions, not needed for 1.8.9 saves |

Vanilla pads the file out to a whole number of 4 KiB sectors and **refuses to read files whose final chunk is
not padded**; when writing, mirror that. If a payload exceeds 1020 KiB, later versions put it in a side
`c.<x>.<z>.mcc` file and set bit 0x80 in the compression byte — never produced by 1.8.9.

### 5.2 Chunk NBT (the "old Anvil" / pre-1.13 layout)

Root compound. `DataVersion` is **absent** in 1.8.9 (it was added in 15w32a), so do not require it. The data
lives under `Level`:

| Tag | Type | Meaning |
|---|---|---|
| `xPos`, `zPos` | Int | chunk coordinates |
| `LastUpdate` | Long | world tick of last save |
| `InhabitedTime` | Long | ticks players spent in the chunk |
| `TerrainPopulated` | Byte | generatable/post-processed flag |
| `LightPopulated` | Byte | whether the stored light is valid |
| `Biomes` | Byte[256] (may be absent) | biome ids, index `z*16 + x` |
| `HeightMap` | Int[256] | per column, **lowest y where sky light is at full strength** (used to seed skylight; regenerate if you recompute light) |
| `Sections` | List of Compound | up to 16 sections; empty ones are omitted |
| `Entities` | List of Compound | entity NBT (including `Player` entries) |
| `TileEntities` | List of Compound | block entities (chests, signs, …) |
| `TileTicks` | List of Compound | pending scheduled block updates |

Section compound:

| Tag | Type | Meaning |
|---|---|---|
| `Y` | Byte | section index 0–15 (bottom→top) |
| `Blocks` | Byte[4096] | block id low 8 bits, index `(y<<8)\|(z<<4)\|x` |
| `Add` | Byte[2048] nibbles (optional) | high 4 bits of the block id: `id = Blocks[i] \| (nibble << 8)`, so ids 0–4095 |
| `Data` | Byte[2048] nibbles | block metadata (damage) 0–15 |
| `BlockLight` | Byte[2048] nibbles | block light |
| `SkyLight` | Byte[2048] nibbles (optional) | sky light; omitted for sections fully shadowed/unloaded |

**The disk format is NOT the wire format.** On the wire (protocol 47) a block is a single little-endian u16
`(id << 4) | meta` with no `Add` array; on disk it is `Blocks` + `Add` (12-bit id) plus a separate `Data`
nibble array. Nibble order is the same on disk and on the wire (even index = low nibble, §3.4).

Reading a 1.8.9 save: `r.X.Z.mca` → location table → inflate zlib → NBT → `Level.Sections[]`. A reader that
handles this layout also reads 1.9–1.12 saves (same Anvil, plus a `DataVersion` tag); 1.13+ ("flattening")
changed section storage to palettes/`BlockStates` and is a different format.

Sources: https://minecraft.wiki/w/Region_file_format ·
https://minecraft.wiki/w/Chunk_format/Anvil/Before_1.13 ·
https://minecraft.wiki/w/Chunk_format (modern contrast: sections use `block_states` palettes).

---

## 6. Entity metadata (clientbound 0x1C, and inside 0x0C/0x0F spawns)

Format (**W-EM** = 1.8-era; **D** = machine-readable cross-check):

```
repeat:
    UByte header
        if header == 0x7F (127): end of list
        type  = header >> 5      (0..7)
        index = header & 0x1F    (0..31)
    value per type:
        0: Byte        1: Short      2: Int       3: Float
        4: String (VarInt length + UTF-8)
        5: Slot        6: Int x,y,z (unused in 1.8)
        7: Float pitch, yaw, roll (rotation)
```

* Encode a field as `(type << 5) | (index & 0x1F)`; terminate the packet's metadata section with `0x7F`.
* **The index space is 0–31 (5 bits) in 1.8**, and the highest index any vanilla 1.8 entity class uses is 22
  (Horse). Reference tables that enumerate indices 0–33 belong to 1.9+ (which reordered indices and changed
  several value types).
* Spawn packets (0x0C Spawn Player, 0x0F Spawn Mob) embed a metadata block with the same encoding.
* **Merge, don't replace:** each entry updates one field; fields not present keep their previous value.
* Metadata values are the *only* way to learn entity state (sneaking, health, name, the ItemStack of a
  dropped item, horse inventory, …).

### 6.1 Base indices (1.8.x)

| Index | Type | Meaning |
|---|---|---|
| 0 | Byte | Entity flags bitmask |
| 1 | **Short** | Air (remaining air ticks; 300 = full) |
| 2 | String | Custom name (**note:** Java is 1.8, index 2 is a String here, not an ItemStack) |
| 3 | Byte | Custom name visible |
| 4 | Byte | Silent (added in 14w30a ⇒ present in 1.8; index per later documentation — ⚠ inferred) |
| 6 | Float | Health (Living) |
| 7 | Int | Potion effect color (Living) |
| 8 | Byte | Potion effect ambient (Living) |
| 9 | Byte | Arrows in body (Living) |
| 10 | Byte | per class (Human skin parts, ArmorStand flags, Tameable flags, …) |
| 11–16 | Float×3 | per class: ArmorStand limb rotations (indices 11–16, payload shape = type 7: pitch, yaw, roll) |
| 12 | Byte | Age (Ageable, and the Wolf/Zombie families use 12 too; negative = baby) |
| 15 | Byte | NoAI (Living) |
| 16 | Int/VarInt | per-class flags (wolves, sheep, villager profession, enderman carried block, creeper fuse, …) |
| 17 | — | per-class (absorption hearts on Human, owner name on Tameable, screaming on Enderman, powered creeper) |
| 18 | — | per-class (score on Human, ocelot type, wolf health/…, collar color, rabbit type) |

Entity flags (index 0) bits: `0x01` on fire · `0x02` crouched · `0x04` unused (riding in older versions) ·
`0x08` sprinting · `0x10` using item / eating / blocking · `0x20` invisible.

### 6.2 Per-class index map (1.8.x, abbreviated)

| Class | Index → type | Meaning |
|---|---|---|
| Entity | 0 Byte / 1 Short | flags, air |
| Living | 2 String, 3 Byte, 4 Byte, 6 Float, 7 Int, 8 Byte, 9 Byte, 15 Byte | name, name visible, silent, health, potion color, potion ambient, arrows, NoAI |
| Ageable | 12 Byte | age (negative = child) |
| ArmorStand | 10 Byte | bitmask (small `0x01`, gravity `0x02`, arms `0x04`, no base plate `0x08`, marker `0x10`) |
| Human | 10 Byte, 16 Byte, 17 Float, 18 Int | skin flags, hide cape/…, absorption hearts, score |
| Bat | 16 Byte | hanging |
| Tameable (Ocelot/Wolf) | 16 Byte, 17 String, 18 Byte/Float, 19 Byte, 20 Byte | flags (sit/sitting), owner, (wolf: health, begging, collar color; ocelot: type) |
| Horse | 16 Int, 19 Byte, 20 Int, 21 String, 22 Int | flags (tamed/saddled/chested/baby/eating/…), type, variant/color+markings, owner, armor |
| Pig | 16 Byte | saddle |
| Rabbit | 18 Byte | type (0 brown … 99 killer) |
| Sheep | 16 Byte | bitmask: bits 0–3 color, bit 4 sheared (1.8 uses `& 0x0F` color + `0x10` sheared) |
| Villager | 16 Int | profession |
| Enderman | 16 Short, 17 Byte, 18 Byte | carried block id, carried block data, screaming |
| Zombie | 12 Byte, 13 Byte, 14 Byte | is child, is villager, is converting |
| ZombiePigman | — | (shares Zombie; no extra in wiki.vg's 1.8 table) |
| Blaze | 16 Byte | on fire |
| Spider | 16 Byte | climbing |
| Creeper | 16 Byte, 17 Byte | state (−1 idle, 1 fuse), powered |
| Ghast | 16 Byte | attacking |
| Skeleton/WitherSkeleton | 13 Byte | type/aggressive |

### 6.3 Spawn "entity ids"

* **Spawn Object (0x0E)** `Type` byte (1.8 list, from minecraft-data 1.8 `entities.json` + the vanilla
  client's object-type handling): 1 Boat · 2 Item · 10/11/12 Minecart (rideable; sub-type in `Data`: 0 rideable, 1 chest,
  2 furnace, 3 TNT, 4 spawner, 5 hopper, 6 command block) · 50 PrimedTnt · 51 EnderCrystal · 60 Arrow ·
  61 Snowball · 62 ThrownEgg · 63 Fireball (ghast) · 64 SmallFireball (blaze) · 65 ThrownEnderpearl ·
  66 WitherSkull · 70 FallingSand · 71 ItemFrame · 72 EyeOfEnderSignal · 73 ThrownPotion ·
  74 FallingSand (dragon egg) · 75 ThrownExpBottle · 76 FireworksRocketEntity · 77 LeashKnot · 78 ArmorStand ·
  **90 FishHook** (vanilla handles type 90; `Data` = owner EID — missing from the minecraft-data list).
  `Data` carries the object-specific payload (falling-sand/frame block id+data, minecart/boat variant, potion
  or arrow sub-type, firework) and a non-zero `Data` is what makes the three trailing velocity shorts present.
  Vanilla ignores/zeroes `Data` for frames, leash knots and large fireballs. A dropped **item** has no item
  data in this packet: the client creates an empty item entity and the actual ItemStack arrives via
  Entity Metadata (index 10).
* **Spawn Mob (0x0F)** `Type` byte (1.8 list, minecraft-data 1.8 `entities.json`): 48 Mob, 49 Monster
  (abstract class markers) · 50 Creeper · 51 Skeleton · 52 Spider · 53 Giant · 54 Zombie · 55 Slime ·
  56 Ghast · 57 PigZombie · 58 Enderman · 59 CaveSpider · 60 Silverfish · 61 Blaze · 62 LavaSlime ·
  63 EnderDragon · 64 WitherBoss · 65 Bat · 66 Witch · 67 Endermite · 68 Guardian · 90 Pig · 91 Sheep ·
  92 Cow · 93 Chicken · 94 Squid · 95 Wolf · 96 MushroomCow · 97 SnowMan · 98 Ozelot · 99 VillagerGolem ·
  100 EntityHorse · 101 Rabbit · 120 Villager. (These are *spawn* ids, unrelated to the metadata indices
  above.)

Sources: https://web.archive.org/web/20150208030456/https://wiki.vg/Entities (oldid 6366, 2015-01-18) ·
https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/pc/1.8/protocol.json
(`entityMetadataItem` types 0–7, terminator 127, bitfield type:3/key:5) ·
https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194, §Entity Metadata).

---

## 7. Versioning pitfalls for a 1.8.9 client author

| # | Topic | 1.8.9 (protocol 47) | Later versions (contrast) |
|---|---|---|---|
| 1 | Packet ids | ids as in §2, play state starts at 0x00 Keep Alive, terrain at 0x21/0x26 | 1.9 (107) renamed/reordered dozens of ids and split "Player Position And Look"; 1.13/1.14 renumbered again |
| 2 | Position (packed 64-bit) | `x(26) << 38 \| y(12) << 26 \| z(26)` | Current docs use `x(26) << 38 \| z(26) << 12 \| y(12)`; Protocol History records a Position encoding change right after 1.8.9 (protocol 48). Do not copy modern decode routines |
| 3 | Chunk section format | 4096 × u16 LE `(id<<4)\|meta`, then block light, then sky light, then biomes | 1.9+ uses paletted bit-packed longs with a "bits per block" varint (and 1.13 changed the palette to global block-state ids, then "no more than 14 bits"); 1.14 removed light from chunk data |
| 4 | Chunk packet header | Primary bitmask = **unsigned short**, data size = **VarInt**, and **no "add" mask** | 1.9 changed the bitmask type (Int/VarInt: "Changed Primary Bit Mask in Chunk Data from an Int to a VarInt (was an unsigned short in 1.8)"); 1.9+ also added the "block entity list at the end" |
| 5 | Map Chunk Bulk | 0x26 exists, batch path, implied ground-up | "Significantly reworked Map Chunk Bulk (0x26)" in protocol 60, then **removed** in protocol 62 (1.9 pre-release): 1.9+ sends everything as (multiple) Chunk Data packets |
| 6 | Light | only inside chunk data (0x21/0x26); section updates via 0x21 with mask; no light packet | 1.14 added dedicated Update Light packets and dropped light from chunk data |
| 7 | Offline save format | pre-1.13 Anvil: `Blocks`+`Add`+`Data`+`BlockLight`+`SkyLight`, no `DataVersion` | 1.13+ "flattening": sections use `block_states`/`biomes` palettes; `DataVersion` present. A 1.8.9 reader can also read 1.9–1.12 saves |
| 8 | Entity metadata | bit-packed header `(type<<5)\|index`, terminator 0x7F, indices as §6 | 1.9+ reordered indices and changed several types (e.g. 1.9 moved animal "age" and shifted mob-indices); 1.15+ added more types and eventually an entirely new "index" scheme |
| 9 | Set Compression | login 0x03 only (play 0x46 broken) | 1.9 moved compression negotiation into the play state (Login plugin requests / login ack framing came much later) |
| 10 | Handshake/status framing | VarInt length + VarInt packet id everywhere (the pre-1.7 "legacy" ping with 0xFE is legacy-only) | 1.20.2+ added a configuration state between login and play — irrelevant for 1.8.9 but breaks copy-pasted modern code |
| 11 | Inventories | window ids are bytes, slot ids are shorts, `Confirm Transaction` is a real handshake the client must answer | 1.17 added a state id to `Click Window`; 1.16.5+ moved item stacks to a different slot encoding |
| 12 | Plugin channels | `MC\|Brand`, `MC\|TrList`, `MC\|TrSel`, `MC\|BEdit`, `MC\|BSign`, `MC\|AdvCdm`, `MC\|ItemName`, `MC\|Beacon`, `MC\|BOpen`, `MC\|RPack` (exact set found in 1.8.9 vanilla client/server) | 1.13 replaced plugin channels with namespaced identifiers (`minecraft:brand`) |

Also worth remembering while implementing: 1.8.9's `Set Compression` threshold and the "must compress if
≥ threshold" rule mean your **read** path must handle both compressed and uncompressed packets *after*
compression is enabled (small packets keep coming through with `Data Length = 0`).

Sources: https://c4k3.github.io/wiki.vg/Protocol_History.html (protocol 47→107 changes, bulk removal, bitmask
type change, 1.13/1.14 chunk and light changes) ·
https://minecraft.wiki/w/Java_Edition_protocol/Packets (current Position packing for contrast) ·
https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/Protocol_version_numbers ·
vanilla 1.8.9 `NetHandlerPlayClient` / `NetHandlerPlayServer` (plugin channel strings, `MCP-919`).

---

## 8. Worked byte layouts (copy-paste sanity checks)

**Serverbound Keep Alive, KeepAliveID = 1, uncompressed:** `02 00 01` — `Length` VarInt `02` (= size of
PacketID + Data), `PacketID` VarInt `00`, `KeepAliveID` VarInt `01`. `Length` always counts the packet id
plus its data, never itself. After compression is enabled the same packet becomes
`Length | DataLength | payload`, where small packets (< threshold) keep `DataLength = 0` and stay raw.

**0x21 with mask = 0x0001, groundUp = true, overworld:**
`Size = 8192 + 2048 + 2048 + 256 = 12544`; bytes: 4096 LE u16 blocks (section 0 bottom slice), 2048 block
light, 2048 sky light, 256 biomes.

**0x26 with 2 overworld columns, skyLightSent = true, masks 0x000F and 0x0001:**
metadata = 2 × (Int, Int, UShort) = 20 bytes; payload = 4 sections × 12288 + 1 section × 12288
(= 5 × 12288, biomes included for both: +2 × 256).

**Unload example (0x21):** `GroundUpContinuous = true, PrimaryBitMask = 0x0000, Size = 0x00`.

Sources: vanilla 1.8.9 `S21PacketChunkData`, `Chunk.fillChunk`, `NibbleArray`, `PlayerManager` (`MCP-919`) ·
https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol (oldid 7194).

---

## 9. Source index

Primary protocol 47 documentation (archived wiki.vg, 1.8.9-era):
* `https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol` (oldid 7194) — full packet tables,
  framing, compression rules, login/status.
* `https://web.archive.org/web/20151022204544/https://wiki.vg/SMP_Map_Format` (oldid 6909) — chunk-data concepts
  and per-packet usage (0x21 vs 0x26).
* `https://web.archive.org/web/20150208030456/https://wiki.vg/Entities` (oldid 6366) — 1.8 entity metadata,
  object/mob id tables.
* `https://web.archive.org/web/20160201000000/https://wiki.vg/Server_List_Ping` — status JSON shape.
* `https://web.archive.org/web/20160201000000/https://wiki.vg/Protocol_Encryption` — RSA/AES scheme and
  Mojang session hashing.
* `https://c4k3.github.io/wiki.vg/Protocol_History.html` — per-version packet history (removals, id moves).
* `https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/Protocol_version_numbers` — release →
  protocol number mapping (1.8…1.8.9 = 47).

Modern minecraft.wiki:
* `https://minecraft.wiki/w/Java_Edition_protocol` and `/Packets` — base data types, current framing.
* `https://minecraft.wiki/w/Chunk_format`, `/Chunk_format/Anvil/Before_1.13` — chunk data (modern + pre-1.13 NBT).
* `https://minecraft.wiki/w/Region_file_format` — .mca container.
* `https://minecraft.wiki/w/Light`, `https://minecraft.wiki/w/Server.properties` — light semantics, compression default.

Implementation/verification cross-checks:
* Vanilla 1.8.9 decompiled source (MCP-919): `https://github.com/Marcelektro/MCP-919` —
  `network/play/server/S21PacketChunkData.java`, `S26PacketMapChunkBulk.java`, `S22PacketMultiBlockChange.java`,
  `S23PacketBlockChange.java`, `world/chunk/{Chunk,NibbleArray}.java`,
  `world/chunk/storage/ExtendedBlockStorage.java`, `server/management/PlayerManager.java`,
  `server/network/NetHandlerLoginServer.java`, `network/NetworkManager.java`, `util/CryptManager.java`,
  `client/network/NetHandlerPlayClient.java`.
* PrismarineJS `minecraft-data` 1.8 (`data/pc/1.8/protocol.json`, `version.json` → protocol 47) —
  machine-readable packet id/field cross-check; `prismarine-chunk` `src/pc/1.8/*` — independent 1.8 client
  implementation (see the nibble-order caveat in §3.4).

### Known gaps / unverified items

* The exact vanilla call site that emits **0x26 Map Chunk Bulk** was not located in the 1.8.9 sources inspected
  (PlayerManager only shows 0x21/0x22/0x23); the batch semantics come from wiki.vg and from the packet's own
  constructor. Implement 0x26 anyway — modded servers and plugins do send it.
* Mob spawn type ids (0x0F) are not enumerated here from a 1.8-era snapshot; the object ids (0x0E) are listed
  with a note about ids 75–78.
* PrismarineJS's 1.8 light-nibble order contradicts the vanilla source; vanilla is presumed correct (§3.4).
