# M7 Online Play Implementation Plan

> **How to work this plan:** one task at a time, in order. Run each step's verification before moving on, and tick the checkboxes (`- [ ]`) as you go. Every commit is made on `main` with explicit `git add` paths.
>
> **Plan style:** this plan pins every interface, constant, path, and rule the tasks must agree on, and carries code sketches wherever a byte layout, formula, or data shape is the deliverable. Algorithm bodies that mirror vanilla behaviour are derived from the cited MCP-919 source files (`refs/_src/MCP-919`, local and uncommitted; HEAD `1717f75902c6184a1ed1bfcd7880404aab4da503`) by the task that needs them, and every derived constant is pinned by a test. Where this plan and a cited document disagree, the document governs and the plan text is corrected first. Pin the source's expression and cite the method that applies it where the two differ — a factor can live in the applier, not the computer.

**Goal:** The Microsoft sign-in chain and the online-mode join. The launcher gains `oxide-launcher auth` — the device-code flow through the Entra ID token, the Xbox Live user authenticate, the XSTS authorize, `login_with_xbox`, and the entitlement and profile checks — and the account lands in the OS keyring with its non-secret record beside it. The protocol crate gains the Minecraft hexdigest helper, the AES-128-CFB8 stream ciphers and the RSA/PKCS#1 v1.5 encryption over the server's parsed `SubjectPublicKeyInfo`; the framed connection wraps its stream in the two continuous cipher contexts. The client's login flow answers an Encryption Request the way the source does: generate the shared secret, compute the hash, post the session join through the signed-in account, send the Encryption Response, enable the ciphers, and continue into an encrypted session — Login Success and everything after it arriving through the cipher. The signed-in player's own skin arrives from the profile endpoint, the remaining rows of the M4 sound limit close, and the player list with its header and footer — the last owed draw of the multiplayer screens — lands. Every carry the M6 close routed here is disposed: answered, landed at a touch, or re-carried with its trigger re-stated.

**Architecture:** A new `auth` module in `oxide-assets` owns the whole sign-in chain and the session's join call: one typed step per endpoint over the crate's HTTP client (extended with POST, bearer and error-body support), with the endpoint strings injectable so every step is testable against a local stub. An `accounts` module keeps the refresh token in the OS keyring — tiered: keyring, or no persistence — and the non-secret account records in the store's data directory. `oxide-launcher` drives the device-code flow and the account listing. `oxide-proto` gains `crypto`: the hexdigest helper with its golden vectors, the CFB8 cipher pair, and the RSA encryption; `Conn` gains an encryption mode that decrypts after each stream read and encrypts before each stream write, so the framed bytes — length field included — travel through the ciphers untouched by the framing code. `oxide-proto-v47` gains the Encryption Response writer. `oxide-game`'s session login flow consumes both: on the server's Encryption Request it generates the secret, computes the hash, calls the injected `JoinServer` seam (production wires the `oxide-assets` join call; tests record), sends the response, enables the connection's encryption and proceeds — Set Compression and Login Success arriving through the ciphers. `oxide-client` resolves the account at startup (a silent refresh through the store), builds the session account, and after login fetches the signed-in player's own skin through the profile endpoint; the player-list draw runs in the client's HUD path over the M1/M4 data events (`PlayerList` from 0x38, the header/footer pair from 0x47), and the entity-sound census closes M4 row 11.

**Tech Stack:** Rust 2024 edition (rust-version 1.85, developed on the installed stable), M6's stack plus the dependencies the spec's appendix A plans for exactly this milestone: `aes` + `cfb8` (the session stream ciphers), `rsa` (PKCS#1 v1.5 over the parsed modulus) and `keyring` (the token store). `rand` (already in the lock via `oxide-game`) is added to `oxide-proto`'s shelf for the shared-secret generation. If the in-task DER derivation shows the `SubjectPublicKeyInfo` parse needs structure beyond `rsa`'s own entry point, `x509-cert` (also appendix A) lands with it and the addition is recorded; the expectation is that it does not. All are new manifests rows. No crate edges beyond section 5.1: the auth chain, the join call, the profile-skin fetch and the account store live in `oxide-assets` (section 5.2 rule 7 names it the HTTP owner; its key-item list already covers remote skin textures), the game crate keeps the session join (its own section 5.3 key item) through its existing `oxide-assets` edge, and the launcher's `auth` command is section 5.3's own key item for `oxide-launcher`. `deny.toml` and the CI dependency surface are updated in the landing tasks.

**Spec:** `docs/specs/oxidecraft-v1-design.md` (v6). M7 is the section 13 row "Microsoft auth", with scope *"Device code flow, Xbox Live and XSTS chain, entitlement checks, keyring token storage, encrypted session with the crypto invariants, client-side session join, hexdigest helper, online-mode connection. Azure application and API permission granted before this milestone starts"* and exit *"Join an online-mode server with a real account"*. Read section 7.4 (the auth architecture and the crypto invariants — quoted into the decisions below), section 8 (the protocol layer's login frame rules), section 5.2 rule 7 and section 5.3 (the crate keys and edges), `docs/research/launcher-assets-auth-survey.md` §3.0–3.8 (the chain, its live probes, the golden vectors and the failure tables), §4.3 (token storage) and §6 (the open items this plan closes), and `docs/research/protocol-47-reference.md`'s login packet rows before the matching tasks; `docs/parity/checklist.md` row 58 before Task 12; appendix C before the acceptance; section 16 (the Azure item) at the close. The M6 final review's "M7 friction" list (`refs/m6-task-31/final-review.md` line 155 — itemised in `docs/STATE.md` item 1) is this plan's standing risk list, and each item is answered by the coverage table below. Vanilla citations are files under `refs/_src/MCP-919` (uncommitted), the same source the research reports cite.

## Global Constraints

- License GPL-3.0. Adapted third-party code must be recorded in `NOTICE`.
- Zero code copied from RustCraft. It is a read-only reference only. RustCraft stores tokens as plaintext JSON; that design is deliberately not followed.
- No Mojang asset, jar, `.class` file, `.ogg`, or `.png` may ever be committed. Nothing under `refs/` or `vanilla/` is committed; textures, models, fonts, colormaps, sounds and the sound index are read from the user's own store at runtime only. No test fixture may embed a Mojang pixel or sample: fixtures are synthetic (generated in the test) or read from the store by an ignored test.
- No token, session code, or account credential may be committed, logged, or written into any evidence file: keyring contents and access/refresh tokens are redacted in every receipt (the device code itself is short-lived and single-use; receipts record its exchange, never its reuse). The refresh token is never written to a plaintext file of any name.
- Never read `.class` files from the jar at runtime. Design invariant.
- rust-version 1.85, edition 2024. `Cargo.lock` is committed. The new dependencies (`aes`, `cfb8`, `rsa`, `keyring`; `rand` into `oxide-proto`) must resolve within 1.85: pin the newest versions whose declared rust-version fits, the `wgpu` precedent (record the resolution in the landing tasks). `x509-cert` lands only if Task 1's derivation needs it.
- CI must fail on: formatting, clippy warnings, test failure, license violations, crate-graph violations, or a tracked Mojang asset. `deny.toml` gains each new crate's licence in the same commit that adds it.
- This milestone adds **no crate edge** — the allowed edges are exactly the section 5.1 table, which `scripts/check-graph.sh` keeps asserting unchanged. (`oxide-game` already may depend on `oxide-assets`; the join seam and the auth chain use existing edges.)
- Every public item carries a doc comment (workspace lint `missing_docs`); `unsafe_code` is forbidden workspace-wide.
- `git add` is always explicit with paths; never `git add -A` or `git add .`.
- Evidence (captures, screenshots, logs, wire recordings, auth exchanges with redactions) lives under the git-ignored `refs/` tree. Committed documents cite it by path.
- The local gate before every push is the command set plus both guard self-tests, the parity self-test and the declared-MSRV check: `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `bash scripts/check-assets.sh && bash scripts/check-assets.sh --self-test`, `bash scripts/check-graph.sh && bash scripts/check-graph.sh --self-test`, `python3 scripts/parity-diff.py --self-test`, `cargo +1.85.0 check --workspace --all-targets`.
- Every server reply and every auth response is hostile until validated: body sizes capped before parse, unknown JSON fields ignored rather than rejected (the survey's `profileActions` note), error bodies carried but never trusted, and no parse path may panic — a malformed response is a typed error (spec S2's spirit). Values that come off the wire are hostile until validated: the Encryption Request's DER key (parsed generically; a parse failure is a typed error), its verify token (echoed, capped), the server id string (hashed, capped), and every auth response field.
- Numerical parity: every timeout, poll interval, cap, and step constant this milestone introduces is traceable to a cited document (the survey's probes and quotes dominate here) or is derived in-task from the cited source and pinned by a test. A constant with no citation does not land.
- The keepalive/read-loop discipline is binding and extends to every new handler: the join call runs inside login, before the play loop exists, with a bounded timeout — a slow session service fails the login with a typed error rather than hanging the flow — and once play begins, no new per-packet work is added to the read pass by this milestone (the player-list draw is a draw, not a read-pass job).
- The auth chain never runs on the client's frame path: the launcher's device-code flow is its own process, the client's refresh happens at startup before the session spawns, and the play loop sees only the resulting token strings.

## Decisions taken in this plan

| # | Decision | Rationale |
| --- | --- | --- |
| 1 | **The sign-in chain lives in `oxide-assets` (`src/auth.rs`), and both binaries drive it.** `Auth` carries the HTTP client, the endpoint set (injectable; `Default` = the survey's URLs) and the client id; each step is a typed method. | Spec §5.2 rule 7: "`oxide-assets` owns all HTTP: the piston-meta fetch chain, asset and jar downloads, and remote skin textures." §5.3 names `oxide-launcher auth` and `oxide-game`'s "session join for online mode" as key items. Both crates already depend on `oxide-assets`; placing the chain here adds no edge. |
| 2 | **The session join is a seam: `oxide-game` computes the hash and calls a `JoinServer` trait; the production implementation lives in the client over `oxide-assets`' `join_session` call; tests record the call.** `SessionConfig` gains `account: Option<Arc<SessionAccount>>`. | Spec §7.4 bullet 4: "Session join, performed by the **client**, not the launcher: `oxide-game` posts `/session/minecraft/join` against `sessionserver.mojang.com` before the encryption response is sent. `oxide-proto`'s login state exposes an injection point that accepts the pre-computed hash." The login flow lives in `oxide-game` since M1; the plan reads "login state" as that flow and lands the injection point as the join seam — the reading is recorded at the close rather than moving the flow. HTTP stays in `oxide-assets`; the seam keeps the login flow testable headless. |
| 3 | **The crypto lives in `oxide-proto::crypto` (`src/crypto.rs`): the hexdigest helper, the CFB8 cipher pair, the RSA encryption over a parsed `SubjectPublicKeyInfo`, and the shared-secret generation.** The Encryption Response writer lands in `oxide-proto-v47::serverbound`. | §5.3 keys `Encryption` to `oxide-proto` ("framing, codecs, compression, encryption"); §8 is the protocol layer; the version-specific packet layout belongs with the version crate (`C01`-family writers). The invariants quoted below are §7.4's own. |
| 4 | **Encryption wraps the raw stream inside `Conn`, below framing: reads decrypt after the stream read, writes encrypt before the stream write, and the framed bytes — length field included — travel through the ciphers untouched.** Enabled once, after the Encryption Response is flushed; both contexts are continuous across packets and never re-initialised. | the survey §3.8, verbatim (the invariants §7.4 states in its own words): "From this point forward, everything is encrypted, including the length field, packet ID, and data length (if compression is enabled)"; "the AES cipher is updated continuously, not finished and restarted every packet." The source's own layering (`NetworkManager.enableEncryption` adds encoder/decoder above the framing handlers, `NetworkManager.java:405-410`) is the same cut. |
| 5 | **Tokens: keyring only in v1 — tiers 1 and 3.** Tier 1: the OS keyring (Secret Service on this machine) holds the AAD refresh token; tier 3: when no keyring is available, nothing persists and a fresh device-code sign-in is required, with a clear message. Tier 2 (an encrypted-at-rest file with a passphrase) is not implemented this milestone — owner Q1. | §7.4: "Tokens are stored in the OS keyring, never in plaintext files. This deliberately diverges from RustCraft." The survey §4.3 records the three-tier policy and why a plaintext fallback is worse than none; tier 2 is the only tier needing new at-rest crypto, and v1 defers it. |
| 6 | **The account records are non-secret: `accounts.json` carries uuid, name and xuid only; secrets live behind a `SecretStore` trait (production: the keyring; tests: memory); no token value ever appears in a file, log, error message, or receipt.** | §7.4's "never in plaintext files", applied to every artifact this milestone writes. The survey §4.3: the refresh token "must never be written into a `launcher_profiles.json`-shaped file that other launchers, backup tools, or sync clients might read." |
| 7 | **The Azure application id is a committed constant (`AUTH_CLIENT_ID`) with an environment override (`OXIDECRAFT_AUTH_CLIENT_ID`) for stub runs and the un-granted window.** The id is public, not a secret; owner Q2 recorded the registered id 2026-10-14 (`f4e40e88-357e-416d-9351-2a87a814c323`; whitelist submitted, pending). | §7.4: "M7 requires an Azure application id for the device code flow, plus a Minecraft API permission grant. This is an external dependency with review lead time"; survey §3.0's probe (the legacy public id fails; a new app must register). The override lets every test and the acceptance-waiting code run before the owner's answer lands (owner Q2). |
| 8 | **The device-code UX is the launcher's; the client never prompts.** The client resolves an account at startup (silent refresh through the store) and, when a server demands online mode without one — or when a join fails — routes to the disconnected screen carrying the reason and the sign-in instruction (`oxide-launcher auth`). | The launcher owns "a CLI login experience" (§5.3's key items; the survey §4's account-flow framing); the client "stays a renderer of state". The source's own failure texts (`disconnect.loginFailedInfo.*`, `NetHandlerLoginClient.java:71-82`) set the shape of the messages. |
| 9 | **Offline play is untouched and remains the default.** The rig stays `online-mode=false` outside the acceptance; the acceptance flips it (backup, run, revert) — owner Q4. | M1–M6's whole regression surface runs offline; the exit criterion needs exactly one real online-mode join, and the local server satisfies it (appendix C's rig discipline). |
| 10 | **The signed-in player's own skin is fetched from the profile endpoint at sign-in** (`GET /session/minecraft/profile/<uuid>`, survey §3.6) through the existing property decode and cache; offline sessions keep the default skin. | M4's deferred row 7: "Skin fetch's live evidence is the ignored endpoint test — re-carried M7 online-mode join / a live fetch". The pipeline exists (`skins.rs`: `decode_profile_property`, `SkinCache`); this milestone gives it the live leg and the own-player source. |
| 11 | **The sounds remainder re-derives before any work.** M4 row 11 ("No entity sounds until M7") is a carry-note hypothesis: Task 11 first censuses the source's client-side sound call sites against the M6 landed set, then wires the residual or closes the row as a finding with the census as its evidence. | The planning rule: a carried note is a hypothesis, not a finding — re-derive its mechanism in the reference tree before writing work. |
| 12 | **The player-list draw lands per parity checklist row 58: the tab-held list with its header and footer over the existing data events**, geometry from `GuiIngame.java:348-358` as the row records it. The M6 close's "server-list roster half" is read as this item — the roster (player list) draw half of M6 deferred rows 55/8 (owner Q5). | M6 final review line 155: "tab-list draw absent (rows 55/8 home: the draw)"; checklist row 58 is the binding text: "Player list (tab): header/footer, entry columns, heads, names, scores, latency bars". |
| 13 | **The carried families keep their triggers, re-stated:** acceptance F8 (destroy-stage re-take — next capture lap), acceptance F9 (focus-gated synthetic mouse — its own tooling round), acceptance F11 (first-person fire overlay — its own view-overlay round), acceptance F13 (portal texture — the Nether-transit round), acceptance F14 (teleport-look — the routed look-verified session matrix), T25 M1/M2 (perspective/fullscreen touches), T28 M3 (fullscreen touch), T14 M-b (`set_fov` — renderer touch), the lagometer riders (`lagometer.rs` touch), the unicode stepdown, the window icon, Fancy clouds (a clouds round), legacy skins (row 6), and the comment/citation rows (their files' touches). Several of those rows' files are opened by tasks here (`session.rs`, `serverbound.rs`, `atlas.rs`, `main.rs`); none of their triggers fires at a plain touch (they are sweep/round triggers), each touching task's report lists the rows it passed, and the close re-checks the rest. | STATE's itemised carries; each row's trigger stands and the coverage table restates it, so no item is silently dropped or force-fitted. |
| 14 | **New dependencies land with MSRV-fit resolution recorded**: `aes`, `cfb8`, `rsa` to `oxide-proto`; `keyring` to `oxide-assets`; `rand` added to `oxide-proto`'s shelf; `x509-cert` only if the SPKI derivation needs it (owner Q3). | Appendix A plans all of them for this milestone; the 1.85 fit is checked at landing (the `wgpu` precedent). `keyring`'s Linux backend needs a running session Secret Service — the dev machine has one; headless runs take tier 3 (Decision 5). |

## Open questions for the owner (answers recorded on approval)

1. **The keyring fallback policy (Decision 5): tiers 1 and 3 only this milestone — keyring, or no persistence with a fresh sign-in.** Tier 2 (an encrypted-at-rest file behind a passphrase) is post-v1, recorded. **Recommended: yes** — §7.4's "OS keyring, never plaintext" is fully satisfied; tier 2 adds at-rest crypto that nothing else in v1 needs.
2. **The Azure application id and the Minecraft API permission (Decision 7): registered, and the id provided at approval — or the §16 fallback.** The live acceptance legs cannot run without a granted app; the code, tests and every stub leg land regardless (the constant + env override). If the permission is refused, §16's rule applies: the owner approves a documented alternative, or online mode leaves v1 with a `docs/DIVERGENCES.md` entry. **Recommended: register now (or confirm already done) and answer with the id; treat refusal as the recorded fallback.**
3. **The new dependencies (Decision 14): `aes`, `cfb8`, `rsa` (proto), `keyring` (assets) land; `rand` joins `oxide-proto`; `x509-cert` only if needed.** All except `x509-cert` are appendix A's own planning for this milestone. **Recommended: yes.**
4. **The acceptance's online leg (Decision 9): flip the rig server to `online-mode=true` for the real-account join** (backup taken, reverted after; the owner signs in during the run — the machine's keyring gets the refresh token it will keep). The public-server leg stays recorded out (anti-cheat risk, spec §14). **Recommended: yes** — the local server satisfies the exit verbatim, and nothing leaves the machine.
5. **The "server-list roster half" reading (Decision 12): it is the player-list (tab) draw half plus its header/footer — M6 deferred rows 55/8, checklist row 58.** **Recommended: yes, with the reading recorded in the close's caveats** so the phrase's origin (the M6 close document) and this landing stay traceable; if the owner reads it as a different server-list item, the row re-routes before Task 1.

### Answers recorded on approval

*(Recorded 2026-10-14 — the owner's answers; the M6 plan's table shape.)*

| # | Question | Answer |
| --- | --- | --- |
| 1 | Keyring fallback | **Tiers 1 and 3 only** — keyring or fresh sign-in; tier 2 (passphrase file) deferred past v1. |
| 2 | Azure application id | Registered — client id `f4e40e88-357e-416d-9351-2a87a814c323` (public). The Minecraft API permission is submitted and **pending** (the owner's form is in progress) — it gates Task 13's live legs only. |
| 3 | New dependencies | **Approved as pinned** (`aes`, `cfb8`, `rsa`, `rand` → proto, `keyring`; `x509-cert` only if needed). |
| 4 | Acceptance online leg | **Yes** — the rig flips to `online-mode=true` (backup + revert); the owner signs in during the run. |
| 5 | Roster-half reading | **Yes** — the player-list (Tab) draw half + its header/footer. |

---
### Task 1: The Minecraft hash and the session ciphers (proto)

**Goal:** `oxide-proto` ships the cryptographic primitives the encrypted session needs, each pinned by its own evidence: the non-standard Minecraft hexdigest (the three golden vectors), the AES-128-CFB8 cipher pair exactly as the source builds it (key = IV = shared secret, 8-bit segments, continuous), the RSA PKCS#1 v1.5 encryption over a generically parsed `SubjectPublicKeyInfo` (ciphertext sized from the parsed modulus, never a hardcoded 128), and the shared-secret generation.

**Files:**
- Create: `crates/oxide-proto/src/crypto.rs` — the module, its types and inline unit tests.
- Modify: `crates/oxide-proto/src/lib.rs` — `pub mod crypto;` and the re-exports.
- Modify: `crates/oxide-proto/Cargo.toml` — `aes`, `cfb8`, `rsa`, `rand` (workspace paths); the workspace `Cargo.toml` gains the same three version rows (`aes`, `cfb8`, `rsa`; `rand` already has a row).
- Modify: `deny.toml` — the crates' licences in this same commit.
- Test: `crates/oxide-proto/tests/crypto.rs` — the golden vectors, the cipher KAT and continuity cases, the RSA cases, the hostile DER set.

**Interfaces:**
- Produces (all `oxide_proto::crypto`):
  - `pub const SHARED_SECRET_LEN: usize = 16;` — the source generates a 128-bit AES key (`CryptManager.createNewSharedKey`, `CryptManager.java:36-48`); the survey §3.8: "a random 16-byte (128-bit) shared secret".
  - `pub fn new_secret_key() -> [u8; SHARED_SECRET_LEN]` — from the OS randomness source; the source's `KeyGenerator` analogue.
  - `pub fn hexdigest(data: impl AsRef<[u8]>) -> String` — SHA-1 over `data`, printed as the survey records verbatim (§3.7): "treating the sha1 output bytes as one large integer in two's complement and then printing the integer in base 16, placing a minus sign if the interpreted number is negative". Lowercase hex; a leading `-` on roughly half of digests; an all-zero digest prints `0`.
  - `pub fn server_id_hash(server_id: &str, secret: &[u8; SHARED_SECRET_LEN], public_key_der: &[u8]) -> String` — `hexdigest(server_id_bytes || secret || public_key_der)` in that order (`CryptManager.getServerIdHash`, `CryptManager.java:72-83`; the id is read as ISO-8859-1/ASCII bytes — identical for ASCII; cap the string with the crate's own string-length rule and pin a non-empty pure-ASCII case; §3.7: "The server ID is now sent as an empty string" for protocol 47 — the request's own string is hashed, whatever it carries).
  - `pub fn parse_server_key(der: &[u8]) -> Result<rsa::RsaPublicKey, CryptoError>` — DER `SubjectPublicKeyInfo` only (never PEM); verify first-hand which entry point of the `rsa` crate parses SPKI directly (`RsaPublicKey::from_public_key_der` is the expectation) and record the finding in the module doc; if the derivation shows the parse needs the `x509-cert` crate's structure pass (Decision 14), it lands here and the addition is recorded — otherwise it does not land.
  - `pub fn encrypt_rsa(key: &rsa::RsaPublicKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>` — PKCS#1 v1.5 padding (never OAEP); the output length equals the parsed modulus's byte length (`key.size()`), not a constant.
  - `pub struct CipherPair` — two independent AES-128-CFB8 contexts (encrypt, decrypt), key = IV = the shared secret, an 8-bit segment size, continuous across calls:
    ```rust
    // The source: Cipher.getInstance("AES/CFB8/NoPadding"), init with an
    // IvParameterSpec over the key's own bytes (CryptManager.java:209-216);
    // one instance per direction (NetworkManager.enableEncryption adds an
    // encoder and a decoder, NetworkManager.java:405-410).
    pub struct CipherPair { /* an encrypt context and a decrypt context */ }
    impl CipherPair {
        pub fn new(secret: &[u8; SHARED_SECRET_LEN]) -> Self; // key == IV == secret
        pub fn encrypt(&mut self, bytes: &mut [u8]);          // continuous: never re-init per call
        pub fn decrypt(&mut self, bytes: &mut [u8]);          // the other context
    }
    ```
  - `pub enum CryptoError` (thiserror) — at minimum `KeyParse { message: String }` and `Encrypt { message: String }`; no variant may carry key material.
- Consumes: nothing new (the crate's own layout rules; `sha1` and `hex` rows already exist in the workspace manifest).

- [ ] **Step 1: The golden vectors (RED).** Pin the survey's three vectors verbatim (§3.7): `hexdigest(b"Notch")` → `4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48`; `hexdigest(b"jeb_")` → `-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1`; `hexdigest(b"simon")` → `88e16a1019277b15d58faf0541e11910eb756f6`. Run: `cargo test -p oxide-proto crypto` — red.
- [ ] **Step 2: Implement `hexdigest` and `server_id_hash`.** The signed formatting sketch: take the 20-byte digest; when the high bit of the first byte is set, negate the big-endian value (two's complement) and remember the sign; print the magnitude's significant bytes as lowercase hex with a `-` prefix when negative. Pin the empty-input digest too (whatever it computes to — record it) and an all-zero case via a private formatting test seam (feed digest bytes directly), so the `0` rule is pinned without forging SHA-1. Run — green.
- [ ] **Step 3: The cipher pair (RED→green).** One KAT: generate a fixed vector once with `openssl enc -aes-128-cfb8 -K <hex-key> -iv <hex-iv> -in <fixed 64-byte plaintext>` (record the exact command and the OpenSSL version beside the fixture as its provenance; the fixture itself is synthetic bytes, generated in the test or checked in as a constant — no Mojang bytes exist here at all); assert `CipherPair::encrypt` reproduces it. Continuity: encrypting the 64 bytes as one call equals encrypting 16 + 48 in two calls (context state carries), and `decrypt(encrypt(x)) == x` on the same pair. Run — red first.
- [ ] **Step 4: The RSA cases (RED→green).** An in-test keypair (1024-bit; plus one 2048-bit case to prove the ciphertext is modulus-sized, not fixed) generated with `rsa`; `parse_server_key` round-trips the public key through DER (`to_public_key_der`); `encrypt_rsa` then the matching private-key decrypt reproduces the plaintext; the ciphertext length equals `key.size()`; an oversized plaintext (≥ modulus − 11) and an empty DER are typed errors. Run — red first.
- [ ] **Step 5: The hostile DER set (RED→green).** Truncated DER, a DER blob with trailing garbage (record the decided rule: reject), a non-RSA OID if constructible, and a zero-length input: each a typed `CryptoError`, no panic. Run — green.
- [ ] **Step 6: Gate and commit.** Run the full gate. Commit:

```
feat: the Minecraft hash and the session ciphers (proto)
```

**Verification:** `cargo test -p oxide-proto`; `cargo fmt --all --check`; `cargo clippy -p oxide-proto --all-targets -- -D warnings`; `cargo deny check` green with the new licences; the three goldens byte-exact against the survey.

---

### Task 2: Stream encryption for framed connections (proto)

**Goal:** `Conn` can wrap its stream in the cipher pair: every byte read off the stream is decrypted before framing sees it, every byte written to the stream is encrypted after framing produces it — so the frame's length field travels encrypted, exactly as §3.8 records ("everything is encrypted, including the length field, packet ID, and data length") — and the framing/compression modes compose unchanged over the encrypted bytes.

**Files:**
- Modify: `crates/oxide-proto/src/conn.rs` — the cipher field, `enable_encryption`, `is_encrypted`, the `fill`/`flush` integration, the `into_inner` doc note.
- Test: `crates/oxide-proto/src/conn.rs` inline tests (the memory-stream harness may gain a small `Read + Write` fake beside the existing test seams) and/or `crates/oxide-proto/tests/conn_encryption.rs` when the harness outgrows the inline module.

**Interfaces:**
- Produces (`oxide_proto::conn::Conn`):
  - `pub fn enable_encryption(&mut self, secret: &[u8; oxide_proto::crypto::SHARED_SECRET_LEN])` — installs the pair; called once, between frames (the login boundary); documented as such (a second call installs fresh contexts and is outside every flow).
  - `pub fn is_encrypted(&self) -> bool` — for the login flow's assertions and tests.
  - The read path: `fill` decrypts exactly the newly read window in place after the stream read succeeds; the write path: `flush` encrypts the staged bytes (the frame the framing code just produced) before the single stream write, in place, exactly once. A failed read leaves the window empty and consumes nothing (the existing invariant stands, now over the cipher); a failed flush leaves the connection unusable (the existing doc stands).
- Consumes: `oxide_proto::crypto::CipherPair` (Task 1); the existing `read_frame`/`write_frame` layering and `Compression` modes (untouched — the cipher sits below them).

- [ ] **Step 1: The harness and the byte-level case (RED).** A memory-stream pair (two shared buffers; `Read`/`Write` impls in the test). Write one frame through an enabled `Conn`; take the raw bytes off the far buffer; assert they differ from the plaintext framing (and specifically: the raw stream's first varint is not the plaintext length — decrypting is required to recover it); decrypt the raw bytes with an independently constructed `CipherPair` (the test builds its own from Task 1) and assert they byte-equal a plaintext `Conn`'s output for the same payload. Run: `cargo test -p oxide-proto conn` — red.
- [ ] **Step 2: The read direction (RED→green).** The test encrypts a hand-built frame with its own pair, feeds it through the far buffer, and asserts `recv` returns the payload — byte-equal across three frames back-to-back (continuity: no per-frame re-init). Run — green.
- [ ] **Step 3: Composition with compression.** Same harness with `set_compression` enabled mid-stream (below and above the threshold): frames round-trip; the compressed frame's bytes still pass through the cipher (the compression layer never sees raw stream bytes). Run — green.
- [ ] **Step 4: The loopback case.** Two `Conn`s over the memory pair, both enabled with the same secret: N mixed-size payloads both directions; each side receives exactly what the other sent; `is_encrypted()` true on both after enable, false before. Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: stream encryption for framed connections (proto)
```

**Verification:** `cargo test -p oxide-proto`; fmt/clippy; the existing framing/compression suites unchanged and green (the cipher is opt-in — every existing test passes with it never enabled).

---

### Task 3: The encrypted login flow and the session join seam (game)

**Goal:** The session's login flow answers an Encryption Request the way the source does: parse the server key, generate the shared secret, compute the hash, hand the hash and the account to the injected `JoinServer` seam (the join completed before the response is sent), send the Encryption Response, enable the connection's ciphers, and continue — Set Compression and Login Success arriving through the encrypted stream. Without an account the refusal stands with its new message; a join failure is a typed error and no response is sent.

**Files:**
- Modify: `crates/oxide-proto-v47/src/serverbound.rs` — `ENCRYPTION_RESPONSE_ID` and `write_encryption_response`.
- Modify: `crates/oxide-proto-v47/src/lib.rs` — the re-export if the crate's convention needs one.
- Modify: `crates/oxide-game/src/session.rs` — `SessionAccount`, the `JoinServer` trait, `SessionConfig.account`, the login-flow rework, the `SessionError` additions, the inline login-suite updates.
- Test: `crates/oxide-game/tests/encrypted_login.rs` — the mock-server harness; plus the inline unit additions.
- Rider (T14 M-c): the drain dead-arm ordering pin — re-derive the record from `refs/m6-task-14/` (and the M6 deferred-list row) and land the pin its fix shape names.
- Rider (T27 F3): the session `GameType`/`CameraClip` case — re-derive the case from `refs/m6-task-27/` and pin it per that record.

**Interfaces:**
- Produces (`oxide_proto_v47::serverbound`):
  - `pub const ENCRYPTION_RESPONSE_ID: i32 = 0x01;` — the login-state serverbound id (protocol §2.1's login table; the source class `C01PacketEncryptionResponse`).
  - `pub fn write_encryption_response(mut out: impl Write, encrypted_secret: &[u8], encrypted_token: &[u8]) -> io::Result<()>` — the id, then two length-prefixed byte arrays in that order (the ciphertexts from `encrypt_rsa`); the source's write order derives in-task from `C01PacketEncryptionResponse` (verify both arrays are written in the order the read side expects forever after; a byte fixture pins it).
- Produces (`oxide_game::session`):
  - `pub trait JoinServer: Send + Sync { fn join(&self, uuid_hex: &str, access_token: &str, server_id: &str) -> Result<(), String>; }` — deliberately a plain message on the error side: it is shown verbatim on the disconnected screen, and the classification lives in the production implementation (Task 9 maps the typed auth errors to it).
  - `pub struct SessionAccount { pub uuid_hex: String, pub access_token: String, pub joiner: Arc<dyn JoinServer> }` — a manual `Debug` (uuid + a placeholder for the joiner; never the token).
  - `SessionConfig` gains `pub account: Option<Arc<SessionAccount>>`; `username`'s doc is corrected: "the name Login Start sends — the signed-in profile's name when an account is present, else the offline name".
  - `SessionError` changes: `EncryptionRequired`'s message becomes `"this server requires a signed-in account — run `oxide-launcher auth`, then start the client with --account"`; new `JoinFailed { message: String }` (the seam's text, shown on disconnect); new `Crypto(#[from] oxide_proto::crypto::CryptoError)`.
  - The flow, in the pinned order (source: `NetHandlerLoginClient.handleEncryptionRequest`, `NetHandlerLoginClient.java:45-93`): on `LoginPacket::EncryptionRequest` — `parse_server_key(&request.public_key)?`; `let secret = new_secret_key();`; `let hash = server_id_hash(&request.server_id, &secret, &request.public_key);`; then — when `config.account` is `Some` — `joiner.join(&uuid_hex, &access_token, &hash)` (an `Err` returns `SessionError::JoinFailed` **before any response byte is sent**), else `EncryptionRequired`; then `payload_of(|out| write_encryption_response(out, &encrypted_secret, &encrypted_token))` and the payload sent+flushed on the same path Login Start takes (`session.rs:1314`); then `conn.enable_encryption(&secret)`; then continue the loop (the next frame read decrypts — Set Compression and Login Success arrive encrypted; the enable happens after the response's flush, the source's own ordering: its listener fires `enableEncryption` when the send completes, `NetHandlerLoginClient.java:86-92`).
- Consumes: `oxide_proto::crypto::{parse_server_key, new_secret_key, server_id_hash, encrypt_rsa, SHARED_SECRET_LEN}` (Task 1); `Conn::enable_encryption` (Task 2); `LoginPacket::EncryptionRequest { server_id, public_key, verify_token }` (already decoded, `crates/oxide-proto-v47/src/clientbound.rs:48`); the existing login flow structure in `session.rs` (`fn login`, ~`:3593`, including today's refusal arm at `:3629` — which has no test today — the decode test `an_encryption_request_is_decoded_so_it_can_be_refused_clearly` (`crates/oxide-proto-v47/tests/login_packets.rs:60`) stands; this task adds the session-side refusal case as a new test (Step 3, case (a)).

- [ ] **Step 1: The writer's byte fixture (RED).** A hand-built fixture: id `0x01`, a 128-byte secret ciphertext run, a 128-byte token run, both length-prefixed as VarInts; assert exact bytes; a second case with a 256-byte run (a longer modulus shape) proves the writer is length-driven, not fixed-size. Run: `cargo test -p oxide-proto-v47` — red.
- [ ] **Step 2: The mock login server (RED).** `tests/encrypted_login.rs`: a thread listening on an ephemeral port; reads the handshake and Login Start (assert the name); writes an Encryption Request built in-test (a fresh 1024-bit keypair's SPKI DER, a fixed verify token, `server_id = ""`); then reads the client's response from the plaintext stream: RSA-decrypts both arrays with the private key; asserts the secret round-trips and the token byte-equals the request's; asserts the recorded join call (uuid, token, hash string) — the expected hash recomputed independently in the test from the same secret + DER; then enables a matching `CipherPair` on its side, sends Set Compression and an encoded Login Success **through the encrypted stream**, and closes. The session under test: harness `JoinServer` records the call; the client must return `LoginOutcome::Accepted { uuid, username }` matching the Login Success, with `conn.is_encrypted()` true. Run — red.
- [ ] **Step 3: The three refusal/failure cases (RED→green).** (a) No account: `EncryptionRequired` with the new message — this case IS the session-side refusal test (none exists today; it is added here); the mock server sees **no response bytes** (it reads EOF or timeout, and the case asserts nothing was written); (b) join failure: the harness joiner returns `Err("...")`; `JoinFailed` carries the text; again no response bytes; (c) a malformed public key in the request: `Crypto(KeyParse)`; no response bytes. Run — red, then green.
- [ ] **Step 4: The riders.** T14 M-c: re-derive the record, pin the drain dead-arm ordering per its fix shape. T27 F3: re-derive and pin the session `GameType`/`CameraClip` case. Both land in this task's commit and are recorded in its report. Run — green.
- [ ] **Step 5: The offline regression.** Every existing session suite (the offline login path, the replay suites, the keepalive test) unchanged and green; the new refusal case green (and the `login_packets` decode test still green). Run: `cargo test -p oxide-game` — green.
- [ ] **Step 6: Gate and commit.** Run the full gate. Commit:

```
feat: the encrypted login flow and the session join seam (game)
```

**Verification:** the mock harness's four cases; the writer fixture; both riders' pins; `cargo test -p oxide-game -p oxide-proto-v47`; fmt/clippy; the diff touches only the listed files.

---

### Task 4: POST, bearer and error bodies in the HTTP client (assets)

**Goal:** The one place HTTP happens grows the request shapes the sign-in chain needs — POST with JSON and form bodies, a bearer-header GET — and surfaces non-2xx response bodies as typed errors (the auth chain's failure handling depends on reading them), while the existing GET behaviour, retry law and error taxonomy for current callers stay byte-identical.

**Files:**
- Modify: `crates/oxide-assets/src/http.rs` — the trait methods, the `UreqClient` implementation, the error extension.
- Test: `crates/oxide-assets/src/http.rs` inline (a local stub server over `std::net::TcpListener` serving canned raw responses; the crate already prefers inline test seams).
- Rider: `crates/oxide-assets/tests/atlas.rs` — the atlas-count re-survey (STATE: `the_real_extraction_tree_stitches` reads 379 against the expected 376; "re-survey, name the 3, bump the count at the next assets touch").

**Interfaces:**
- Produces (`oxide_assets::http`):
  - `HttpClient` becomes `pub trait HttpClient: Send + Sync` (the bound Task 9's `Arc<dyn HttpClient>` → `Arc<dyn JoinServer>` chain needs across the session thread; `UreqClient` and the existing doubles — `skins.rs`'s test seams, `fetch.rs`'s `Transfer`, the tests' `FakeHttp`s — all gain it in this commit), and gains — with doc comments stating each one's wire shape — `pub fn post_json(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, HttpError>` (sets `Content-Type: application/json` and `Accept: application/json` — the survey §3.2/§3.4 record that XBL and Minecraft Services "will complain" without both), `pub fn post_form(&self, url: &str, fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError>` (an `application/x-www-form-urlencoded` body; the encoder percent-encodes, so the survey's documented body `client_id=…&scope=XboxLive.signin%20offline_access` is reproduced byte-exactly — pin the encoder against that string), and `pub fn get_with_bearer(&self, url: &str, bearer: &str) -> Result<Vec<u8>, HttpError>` (sets `Authorization: Bearer <token>` — the trailing space is the survey's mandatory detail; the token is never logged).
  - `HttpError::Status` gains a body: `Status { url: String, code: u16, body: Vec<u8> }` — the message text is unchanged; `is_retryable` is unchanged (4xx never retried, 5xx retried); every existing call site and test updates to the new field in this commit (a compile-and-grep pass, no behaviour change for GET).
  - The `UreqClient` reads non-2xx bodies (verify ureq's configuration path first-hand — `http_status_as_error(false)` is the expectation — and record which mechanism was used in the module doc); bodies are capped before they travel (`MAX_ERROR_BODY_BYTES` — pick a cap in-task, pin it, and record why: the error bodies the chain reads are small JSON documents).
- Consumes: the existing `with_retries`/`classify`/`is_retryable` seams; the survey §3's live-probed failure shapes (403 `ForbiddenOperationException`, 401 empty bodies, `AADSTS…` JSON bodies).

- [ ] **Step 1: The stub server and the request shapes (RED).** A `TcpListener` on an ephemeral port serving one canned response per connection; for each method, assert the request bytes: request line, `Content-Type`/`Accept` headers, the body (JSON bytes; the exact form string `scope=XboxLive.signin%20offline_access`; the `Bearer ` header with its trailing space). Run: `cargo test -p oxide-assets http` — red.
- [ ] **Step 2: Error bodies (RED→green).** A 400 with a JSON body → `Status { code: 400, body }` carrying those bytes; a 401 with an empty body → `Status { code: 401, body: [] }`; a 204 → `Ok(vec![])`; a 500 → retried (the stub counts attempts: 3 tries, then the final error), a 400 → exactly one attempt. Run — red, then green.
- [ ] **Step 3: The existing GET law untouched.** The current GET tests (retry, scheme, status) still green; a `git diff` read confirms no semantic drift beyond the added field. Run — green.
- [ ] **Step 4: Rider — the atlas-count re-survey.** Run: `OXIDECRAFT_STORE=~/.local/share/oxidecraft cargo test -p oxide-assets --test atlas -- --ignored the_real_extraction_tree_stitches` — name the three paths the local extraction tree resolves beyond the M2 author's 376, bump the expected count to the re-derived value, and record the survey (which three, and the store-side drift finding) in the test's comment and the task's report (the same file hosts M4 row D5's live cite at `atlas.rs:1065`, trigger the comment sweeps — passed, not fired, recorded as such). Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: POST, bearer and error-body support in the HTTP client (assets)
```

**Verification:** `cargo test -p oxide-assets`; the stub cases; the rider's named paths and the new count; fmt/clippy; `cargo deny check` unchanged (no new crates here).

---

### Task 5: The device-code flow and the Xbox Live chain (assets)

**Goal:** `oxide-assets::auth` carries the first half of the sign-in chain, each step a typed method over the crate's HTTP client with injectable endpoints: the device-code request, the token poll with its `authorization_pending`/`slow_down`/`expired_token` handling, the Xbox Live user authenticate with its exact request body, the XSTS authorize with the XErr table — every request shape byte-pinned against the survey's live-probed bodies, every failure a typed error.

**Files:**
- Create: `crates/oxide-assets/src/auth.rs` — the module, its types, the request bodies, inline unit tests.
- Modify: `crates/oxide-assets/src/lib.rs` — `pub mod auth;`.
- Test: `crates/oxide-assets/tests/auth_chain.rs` — the stub-endpoint corpus (a `HttpClient` test implementation answering from a script of canned responses; no sockets needed) with the survey's verbatim response bodies.

**Interfaces:**
- Produces (`oxide_assets::auth`):
  - `pub struct AuthEndpoints { pub device_code: String, pub token: String, pub xbl_authenticate: String, pub xsts_authorize: String, pub mc_login: String, pub mc_entitlements: String, pub mc_profile: String, pub session_join: String }` — `impl Default` carries the survey's URLs verbatim (§3.1 `https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode` and `…/token`; §3.2 `https://user.auth.xboxlive.com/user/authenticate`; §3.3 `https://xsts.auth.xboxlive.com/xsts/authorize`; §3.4 `https://api.minecraftservices.com/authentication/login_with_xbox`; §3.5 `…/entitlements/mcstore` and `…/minecraft/profile`; §3.7 `https://sessionserver.mojang.com/session/minecraft/join`). The `consumers` tenant in the two Entra URLs is the survey's hard requirement ("you must use the `consumers` AAD tenant", §3.1) — pin both strings in a test.
  - `pub struct DeviceCode { pub device_code: String, pub user_code: String, pub verification_uri: String, pub expires_in: u64, pub interval: u64, pub message: String }` — the field set verbatim from Microsoft Learn via §3.1.
  - `pub struct MsaToken { pub access_token: String, pub refresh_token: Option<String>, pub expires_in: u64 }`; `pub struct XboxToken { pub token: String, pub user_hash: String }`; `pub struct XstsToken { pub token: String, pub user_hash: String }`.
  - `pub struct Auth { /* http + endpoints + client id */ }` — `pub fn new(http: Arc<dyn HttpClient>, endpoints: AuthEndpoints, client_id: String) -> Self`.
  - `pub fn begin_device_code(&self) -> Result<DeviceCode, AuthError>` — POST form `client_id=…&scope=XboxLive.signin offline_access` (`offline_access` is what yields the refresh token, §3.1); the `%20` form encoding lands via Task 4's encoder.
  - `pub fn poll_token(&self, code: &DeviceCode, sleep: &mut dyn FnMut(Duration)) -> Result<MsaToken, AuthError>` — POST form `grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id=…&device_code=…` (§3.1, verbatim); a 400 whose error is `authorization_pending` keeps polling at `interval`; `slow_down` increases the interval (derive the exact bump from the wiki text in-task and pin it — the survey flags the interval field; record the derived rule); `expired_token` (or the 15-minute window, §3.1) is a typed error; success parses `access_token`/`refresh_token`/`expires_in`.
  - `pub fn refresh_msa(&self, refresh_token: &str) -> Result<MsaToken, AuthError>` — POST form `grant_type=refresh_token&refresh_token=…&client_id=…`; §3.1: "no Xbox/Minecraft state is needed to refresh — a launcher can silently refresh on every start".
  - `pub fn authenticate_xbox(&self, msa_access_token: &str) -> Result<XboxToken, AuthError>` — POST JSON, exact body (§3.2 verbatim): `{"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":"d=<access token>"},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"}` — the `d=` prefix is mandatory ("make sure that it is prefixed with 'd='"); parses `Token` and `DisplayClaims.xui[0].uhs`.
  - `pub fn authorize_xsts(&self, xbl: &XboxToken) -> Result<XstsToken, AuthError>` — POST JSON (§3.3 verbatim): `{"Properties":{"SandboxId":"RETAIL","UserTokens":["<xbl token>"]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"}` — the `rp://` scheme is the exact string; parses `Token` and the same `uhs`.
  - `pub enum AuthError` (thiserror, all variants doc-commented, none carrying a token): `Http(#[from] HttpError)`; `Status { step: AuthStep, code: u16, body: String }` (body lossy-UTF-8, capped; §3.2: XBL errors carry no body — the message reports the status plus the `x-xblcorrelationid` header when present; the stub provides one); `DeviceFlow { error: String }`; `Xsts { xerr: u32, message: String, redirect: Option<String> }`; `Shape { step: AuthStep, message: String }`. `pub enum AuthStep { DeviceCode, Token, XboxLive, Xsts }`.
  - The XErr table (in the `Xsts` construction, §3.3 verbatim texts): `2148916227` banned; `2148916233` no Xbox account; `2148916235` region; `2148916236`/`2148916237` (South Korea adult verification); `2148916238` child account — the family note is important here: §3.3 records that this "only seems to occur when using a custom Microsoft Azure application", which this project by §3.0 must use; `2148916262` rare/unknown. Unknown codes: a generic message carrying the code. Each row a test fixture.
- Consumes: `oxide_assets::http` (Task 4's POST/form/json support and error bodies); the survey §3.1–3.3's verbatim bodies as the fixtures.

- [ ] **Step 1: The fixtures (RED).** One canned response per step from the survey's verbatim JSON (devicecode fields; a token success; the XBL `Token`+`uhs`; the XSTS `Token`); each request's exact bytes asserted at the stub (the `d=` prefix, the `rp://` relying party, the scope string, the grant types). Run: `cargo test -p oxide-assets auth` — red.
- [ ] **Step 2: Implement the steps in order.** Run — green.
- [ ] **Step 3: The poll state machine (RED→green).** Scripts: pending → pending → success (interval respected, sleeps recorded by the injected sleeper); a `slow_down` bump; an `expired_token`; a non-JSON body (Shape error, no panic). Run — red, then green.
- [ ] **Step 4: The failure corpus (RED→green).** XBL 401 with empty body → `Status { code: 401 }` with the correlation id in the message; XSTS 400 + `XErr 2148916238` → the child-family message; an unknown `XErr`; unknown extra fields in every success body tolerated (§3.6's `profileActions` lesson applied here). Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the device-code flow and the Xbox Live chain (assets)
```

**Verification:** `cargo test -p oxide-assets`; the request bodies byte-checked against the survey's quoted shapes; fmt/clippy; no token value appears in any error's `Display` (asserted on a constructed error).

---

### Task 6: The Minecraft sign-in stage — token, entitlements, profile (assets)

**Goal:** The second half: `login_with_xbox` with the exact `XBL3.0 x=<uhs>;<xsts>` concatenation, the entitlement check that separates an owning account from a merely-working Microsoft account, the profile fetch that supplies the real uuid and name, the two drivers (`complete_sign_in`, `silent_sign_in`), and the session join call with its documented failure shapes.

**Files:**
- Modify: `crates/oxide-assets/src/auth.rs` — the stage-two methods, `SignedIn`, the join call, more fixtures.
- Test: `crates/oxide-assets/tests/auth_chain.rs` — the stage-two corpus.

**Interfaces:**
- Produces (`oxide_assets::auth`, continuing):
  - `pub struct McProfile { pub uuid_hex: String, pub name: String }` — `uuid_hex` is the 32-char no-dashes form (the survey's join-stage tail, §3.7: "the profile id … has format `11111111222233334444555555555555` which needs to be changed into format `11111111-2222-…` **before going into Login Success**" — the dashed conversion is needed where Login Success consumes it; provide `pub fn uuid_with_dashes(uuid_hex: &str) -> Option<String>` beside the type and use it at the consumption site; validate the input as 32 hex digits — hostile input, typed error).
  - `pub struct Entitlements { pub owns_minecraft: bool, pub items: Vec<String> }` — from the `/entitlements/mcstore` `items` array's `name` fields (§3.5: `product_minecraft`, `game_minecraft`); `owns_minecraft` is true iff one of those two is present — record the derivation; an empty `items` array is the survey's crux: "Everything until here works with a normal Microsoft account!" — the driver must refuse with a clear typed error.
  - `pub fn login_with_xbox(&self, uhs: &str, xsts_token: &str) -> Result<MsaToken, AuthError>` — POST JSON `{"identityToken":"XBL3.0 x=<uhs>;<xsts_token>"}` — §3.4: "literal `XBL3.0 x=`, the `uhs` hash, a `;`, then the XSTS token"; parses `access_token` and `expires_in` (86400 = 24 h); §3.4's note that the response's `username` field "is not the uuid" is recorded in the doc (the profile fetch is the identity source).
  - `pub fn fetch_entitlements(&self, mc_access_token: &str) -> Result<Entitlements, AuthError>` — GET with bearer (§3.5; the `Bearer ` trailing space mandatory).
  - `pub fn fetch_profile(&self, mc_access_token: &str) -> Result<McProfile, AuthError>` — GET with bearer; parses `id` (no dashes) and `name`; unknown fields tolerated.
  - `pub struct SignedIn { pub mc_token: MsaToken, pub profile: McProfile, pub entitlements: Entitlements, pub refreshed_msa_refresh_token: Option<String> }` — the account-ready record.
  - `pub fn complete_sign_in(&self, msa: MsaToken) -> Result<SignedIn, AuthError>` — XBL → XSTS → login → entitlements (refuse when not owned) → profile, in that order.
  - `pub fn silent_sign_in(&self, refresh_token: &str) -> Result<SignedIn, AuthError>` — `refresh_msa` then `complete_sign_in`; the whole client-start path (§3.1: silent refresh needs no Xbox/Minecraft state).
  - `pub fn join_session(&self, profile_uuid_hex: &str, access_token: &str, server_id: &str) -> Result<(), JoinError>` — POST JSON to `endpoints.session_join`: `{"accessToken":"…","selectedProfile":"<uuid without dashes>","serverId":"<hash>"}` (§3.7 verbatim shape; the series server returns **204 No Content** on success — accept an empty body); `Content-Type: application/json` is mandatory ("you *must* have the Content-Type header set to application/json or you will get a 415 … or 403", §3.7).
  - `pub enum JoinError` (thiserror): `Http(#[from] HttpError)`; `ForbiddenOperation` (the `{"error":"ForbiddenOperationException"}` shape); `InsufficientPrivileges` (no server message exists to carry — the operator sentence renders from the error name plus the survey's `// Xbox multiplayer disabled` annotation); `Banned { message: String }` (`UserBannedException` with its `errorMessage`); `Shape { message: String }`. Each renders the operator-facing sentence for the disconnect screen.
  - `pub const AUTH_CLIENT_ID: &str = "f4e40e88-357e-416d-9351-2a87a814c323";` — the registered application's id (owner Q2, recorded at approval 2026-10-14; the value is public, never a secret), plus `pub fn client_id_from_env() -> String` — `OXIDECRAFT_AUTH_CLIENT_ID` when set and non-empty, else the constant; every test uses the override (asserted), so the constant's value never affects a test.
- Consumes: Task 5's `Auth`, tokens and error type; the survey §3.4–3.5, §3.7 verbatim bodies.

- [ ] **Step 1: Stage-two fixtures (RED).** The login response (verbatim §3.4, `expires_in: 86400`); the entitlements 200 with the two items and the empty-`items` case; the profile 200 (§3.6's shape); the join's 204, 403 `ForbiddenOperationException`, `InsufficientPrivilegesException` and `UserBannedException` bodies; every request asserted at the stub (`identityToken` exact concat; the join body's three keys and the no-dashes uuid). Run: `cargo test -p oxide-assets auth` — red.
- [ ] **Step 2: Implement the stage and both drivers.** Run — green.
- [ ] **Step 3: The refusal and tolerance cases (RED→green).** Empty entitlements → the no-ownership typed error; a 401 profile → `Status`; unknown fields appended to every success body tolerated; `uuid_with_dashes` on a malformed id → None (never a panic, never an invented uuid). Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the Minecraft sign-in stage (assets)
```

**Verification:** `cargo test -p oxide-assets`; fmt/clippy; the doc examples for `complete_sign_in`/`silent_sign_in` written but never run against the live service here (that is Task 13's leg).

---

### Task 7: The account store — keyring secrets and account records (assets)

**Goal:** The refresh token lives in the OS keyring and nowhere else; the account records (uuid, name, xuid) live in `accounts.json`; when no keyring is available nothing persists and the outcome says so; a mockable secret backend keeps every branch testable; no token value ever reaches a file, a log, or an error string.

**Files:**
- Create: `crates/oxide-assets/src/accounts.rs` — the store, the trait, the tiers, inline tests.
- Modify: `crates/oxide-assets/src/lib.rs` — `pub mod accounts;`.
- Modify: `crates/oxide-assets/Cargo.toml` — `keyring`; the workspace `Cargo.toml` gains its row.
- Modify: `deny.toml` — the crate's licence in this same commit.
- Test: `crates/oxide-assets/tests/accounts.rs` — the store-level cases with a memory backend.

**Interfaces:**
- Produces (`oxide_assets::accounts`):
  - `pub struct AccountRecord { pub uuid_hex: String, pub name: String, pub xuid: Option<String> }` — the non-secret half; nothing else persists.
  - `pub trait SecretStore: Send + Sync { fn get(&self, key: &str) -> Result<Option<String>, StoreError>; fn set(&self, key: &str, value: &str) -> Result<(), StoreError>; fn delete(&self, key: &str) -> Result<(), StoreError>; }` — the key is the account's uuid; the value is the refresh token.
  - `pub struct KeyringSecrets` — the production backend over the `keyring` crate: service `oxidecraft`; the Linux backend is Secret Service (the survey §4.3's caveats are the doc comment: a session bus plus kwallet/gnome-keyring must run; per-user entries are enumerable by other processes running as the user — the threat model is stated, not solved).
  - `pub struct AccountStore { /* data dir + a boxed SecretStore */ }` — `pub fn open(data_dir: PathBuf) -> Result<Self, StoreError>` (keyring backend) and `pub fn open_with(data_dir: PathBuf, secrets: Box<dyn SecretStore>) -> Result<Self, StoreError>` (tests).
  - `pub fn list(&self) -> Vec<AccountRecord>`; `pub fn record(&self, uuid_hex: &str) -> Option<AccountRecord>`; `pub fn refresh_token(&self, uuid_hex: &str) -> Result<Option<String>, StoreError>`; `pub fn remove(&self, uuid_hex: &str) -> Result<(), StoreError>`; `pub fn save(&self, record: &AccountRecord, refresh_token: &str) -> Result<SaveOutcome, StoreError>`.
  - `pub enum SaveOutcome { Saved, SessionOnly { reason: String } }` — tier 1 writes the secret then the record; a keyring availability failure yields `SessionOnly` with a short reason and writes **nothing** (no orphan record; the next `list` does not show it — pin that).
  - File shape: `<data_dir>/accounts.json`, `{"version": 1, "accounts": [ {uuid_hex, name, xuid} … ]}`; written atomically (temp file + rename — reuse the store's existing atomic-write helper if one exists; else a small local one); malformed or unknown-field files load tolerantly (unknown fields dropped; a corrupt file is backed up beside itself and treated as empty — record the decided rule and pin it).
- Consumes: `oxide_assets::store`'s data-dir conventions (the M6 options file's home); the survey §4.3's policy text; the keyring crate (MSRV check: newest 4.x declaring ≤ 1.85; record the resolved version).

- [ ] **Step 1: Round-trip and the no-secret assertion (RED).** `save` → `refresh_token` → `list` via the memory backend; then read `accounts.json`'s raw bytes and assert the refresh token's value does **not** occur; assert the record's fields do. Run: `cargo test -p oxide-assets accounts` — red.
- [ ] **Step 2: The tier-3 path (RED→green).** A backend returning the keyring-unavailable error shape → `SessionOnly` with a reason; no file appears; `list` empty; the error text contains no secret. Run — red, then green.
- [ ] **Step 3: Tolerance and removal (RED→green).** A corrupt file → empty list + the backup beside it; unknown fields dropped; `remove` deletes both halves and is idempotent; `save` for an existing uuid replaces both halves. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the account store (assets)
```

**Verification:** `cargo test -p oxide-assets`; `cargo deny check` green with keyring's licence; a grep over the crate for the token field names confirms no logging of secrets (the redaction rule audited at review).

---

### Task 8: oxide-launcher auth (launcher)

**Goal:** The launcher gains the sign-in command: `auth` runs the device-code flow end to end (prints the user code and the verification URI, polls, saves the account), `auth --status` lists the signed-in accounts without a secret in sight, and `auth --logout <name|uuid>` removes both halves.

**Files:**
- Modify: `crates/oxide-launcher/src/main.rs` — the `Command::Auth` variant and its dispatch.
- Create: `crates/oxide-launcher/src/auth.rs` — the command body (flow, prompts, status, logout) so `main.rs` stays a dispatcher.
- Test: `crates/oxide-launcher/src/auth.rs` inline (CLI shapes; status/logout against a temp dir with the memory backend; a full flow against a scripted stub `HttpClient`).

**Interfaces:**
- Produces: `Command::Auth { /// List the signed-in accounts. #[arg(long)] status: bool, /// Sign an account out by name or uuid. #[arg(long)] logout: Option<String> }`.
  - `auth` (no flags): resolve the store dir with ONE precedence shared with the client — `--data-dir` (when given) > `OXIDECRAFT_STORE` (when set — the client's `STORE_VAR`, `crates/oxide-client/src/assets.rs:40`) > `<data dir>/oxidecraft` (the launcher's `default_data_dir`, `oxide-launcher/src/main.rs:44-49`, gains the env honour in this task); `begin_device_code`; print the message + user code + `verification_uri` (the survey §3.1's fields; no browser is opened — the survey's flow is print-and-poll); `poll_token`; `complete_sign_in`; `save`; print the signed-in name and uuid and the outcome (`Saved`, or the tier-3 `SessionOnly` reason).
  - `auth --status`: the account list (name + uuid), or "no accounts" .
  - `auth --logout <name|uuid>`: `remove`; a clear message either way.
  - The client id: `oxide_assets::auth::AUTH_CLIENT_ID` with the `OXIDECRAFT_AUTH_CLIENT_ID` env override (Decision 7).
- Consumes: Tasks 5–7; the launcher's existing `dirs`-based store resolution; `anyhow` for the binary's surface errors (the typed texts pass through; no secret renders).

- [ ] **Step 1: CLI parse tests (RED).** `auth`, `auth --status`, `auth --logout X` parse; unknown flags refuse. Run: `cargo test -p oxide-launcher` — red.
- [ ] **Step 2: Status and logout (RED→green).** Against a temp dir + memory backend: empty list message; a seeded account listed (name + uuid, no token); logout removes and a second logout reports cleanly; a case with `OXIDECRAFT_STORE` set proves the launcher resolves the same root the client would. Run — red, then green.
- [ ] **Step 3: The full flow against the script.** A scripted stub `HttpClient` answers the whole chain (device code → pending → token → XBL → XSTS → login → entitlements → profile); the command saves the account; assert the record fields, the absence of the token in every output buffer produced by the flow (capture the command's strings and grep), and the printed user code. Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: oxide-launcher auth (launcher)
```

**Verification:** `cargo test -p oxide-launcher` (including the store-root case with `OXIDECRAFT_STORE` set proving the launcher and client resolve the same root); fmt/clippy; `cargo run -p oxide-launcher -- auth --status` against the real (empty) store prints the no-accounts line and touches nothing.

---

### Task 9: The online join path in the client (client)

**Goal:** The client can join online-mode servers: `--account <name-or-uuid>` resolves a stored account (silent refresh through the store), builds the session account with the production join implementation, sends the profile's name in Login Start, and routes every auth failure to the disconnected screen with its sentence — while the offline default path is untouched.

**Files:**
- Create: `crates/oxide-client/src/accounts.rs` — the resolution helper, `HttpJoinServer`, their tests.
- Modify: `crates/oxide-client/src/main.rs` — the `--account` flag on the CLI struct (`username`'s neighbour), the resolution before both `spawn_session` call sites (`:1748` the server-list join, `:4761` the in-game one), and the disconnected-screen mapping for the two auth errors.
- Rider (T27 F2): the spectator call-site case — re-derive from `refs/m6-task-27/` and pin per that record (the next `main.rs` round).
- Test: `crates/oxide-client/src/accounts.rs` inline; the `main.rs` CLI defaults test (~`:8531`) gains the flag's default.

**Interfaces:**
- Produces (`oxide_client::accounts`):
  - `pub struct ResolvedAccount { pub username: String, pub session: Arc<oxide_game::session::SessionAccount> }`.
  - `pub fn resolve(store: &oxide_assets::accounts::AccountStore, query: &str, auth: Arc<oxide_assets::auth::Auth>) -> Result<ResolvedAccount, String>` — find the record by name or uuid (`AccountStore::list`/`record`); read `refresh_token`; `auth.silent_sign_in`; build the session: `uuid_hex` + freshly minted mc access token + `HttpJoinServer`. Failure sentences (pinned): no match → `"no signed-in account matches `{query}` — run `oxide-launcher auth` first"`; no keyring secret → the tier-3 sentence; a chain failure → the typed text (never a token).
  - `pub struct HttpJoinServer { auth: Arc<Auth> }` — `impl oxide_game::session::JoinServer for HttpJoinServer`: calls `auth.join_session(uuid, token, server_id)` and renders `JoinError`'s operator sentence (`ForbiddenOperation` → `"the session service refused the join (the account's login is no longer valid)"`; `InsufficientPrivileges` → `"this account's Xbox settings do not allow multiplayer"`; `Banned` → its message; `Shape`/`Http` → their text). The sentences' shapes follow the source's `disconnect.loginFailedInfo.*` family (`NetHandlerLoginClient.java:71-82`).
- Consumes: Task 6's `Auth`/`silent_sign_in`/`join_session`; Task 7's `AccountStore`; Task 3's `SessionConfig.account` + `SessionAccount` + `JoinServer`.
- `spawn_session` (`crates/oxide-client/src/main.rs:6045`) gains one parameter, `account: Option<Arc<oxide_game::session::SessionAccount>>`, threaded into `SessionConfig.account`; both call sites (`:1748`, `:4761`) pass the resolved account or `None`; `username` carries the profile name when the account resolved. The session thread's error arms (`:6063-6073`, today log-plus-generic-`Disconnected`) route `SessionError::EncryptionRequired` and `SessionError::JoinFailed`'s message into the `Disconnected` event's `reason` (the path the screen consumes) so the sentence reaches the disconnected screen.
- The CLI: `/// Sign in as a stored account for online-mode servers. #[arg(long)] account: Option<String>`; with `--account` set, the client builds one `Auth` (`UreqClient` + `AuthEndpoints::default()` + `client_id_from_env()`), resolves once at startup, and hands both spawn paths the same `Arc<SessionAccount>`; `SessionConfig.username` becomes the resolved profile name (the offline `OxideDev` default is untouched without the flag). The `SessionError::EncryptionRequired` / `JoinFailed` messages reach the disconnected screen through the existing error→screen path (`open_disconnected`, `main.rs:2364`) — verify that path first-hand and extend the mapping where it drops the message.

- [ ] **Step 1: Resolution cases (RED).** A temp store + memory backend + a scripted stub `HttpClient` answering the silent chain: resolve by name and by uuid; a missing account's sentence; a chain failure's text; assert no token appears in any returned string. Run: `cargo test -p oxide-client accounts` — red.
- [ ] **Step 2: The CLI and the two spawn sites.** Flag parse/default; both call sites receive the account; a unit pins that the session config built in each path carries `account: Some(…)` and the profile name when resolved, and `None` plus the offline name otherwise. Run — green.
- [ ] **Step 3: The failure routing (RED→green).** A unit drives a session error (`EncryptionRequired`, `JoinFailed`) through the screen path and asserts the disconnected screen holds the message text (the existing offline disconnect tests are the harness). Run — red, then green.
- [ ] **Step 4: Rider T27 F2.** Re-derive the spectator call-site case from its record; pin it. Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: the online join path in the client (client)
```

**Verification:** `cargo test -p oxide-client`; the offline paths' suites unchanged; fmt/clippy; a manual `--help` read confirms the flag's sentence.

---

### Task 10: The signed-in player's skin from the profile endpoint (client)

**Goal:** With an account active, the client fetches the signed-in player's own textures from the session server's profile endpoint and renders them — through the existing property decode and skin cache — while offline sessions keep the default skin; the fetch runs off the frame path and its failure is one recorded line, never a stall.

**Files:**
- Modify: `crates/oxide-assets/src/skins.rs` — `fetch_profile_textures` plus its inline parser test and the ignored live case.
- Modify: `crates/oxide-client/src/main.rs` — the own-model resolution on `ClientEvent::LoggedIn` with an account (the `LoggedIn` handler ~`:6434`; the own-skin sites near `SkinCache::new`, `:1744`/`:4757`).

**Interfaces:**
- Produces (`oxide_assets::skins`): `pub fn fetch_profile_textures(http: &dyn HttpClient, uuid_hex: &str) -> Result<Vec<ProfileTexture>, SkinError>` — GET `https://sessionserver.mojang.com/session/minecraft/profile/<uuid_hex>` (the URL constant carries the §3.6 cite); parse the `properties` array, `decode_profile_property` each textures entry (existing, `skins.rs:122`); tolerate `profileActions` and unknown fields (§3.6's own note: "A 1.8.9 client must ignore unknown fields rather than reject them"); a 204/404 is an empty list (an account may have no textures); the body is capped before parse.
- Consumes: Task 4's client; `SkinCache::fetch` (`skins.rs:248`) and the existing cache path; the client's `LoggedIn` event (`ClientEvent::LoggedIn { uuid, username }`, `main.rs:6434`).
- Behaviour: on `LoggedIn` with an account, a small worker fetches `fetch_profile_textures` → `SkinCache::fetch(URL)` → the own model swaps in when it lands; a failure logs one line and keeps the default (`DefaultModel::default_skin`, `skins.rs:345`); offline sessions never call the endpoint.

- [ ] **Step 1: The parser fixture (RED).** §3.6's verbatim response shape (truncated textures payload) parsed to a `ProfileTexture`; the `profileActions` field and an invented extra field tolerated; a malformed base64 → typed error; a body-less 204 and a 404 → an empty list (pinned — the survey documents neither code for this endpoint). Run: `cargo test -p oxide-assets skins` — red.
- [ ] **Step 2: The fetch and the client wiring.** Run — green.
- [ ] **Step 3: The ignored live case.** Extend the file's existing ignored network test pattern with a profile-endpoint case (store/network gated like its neighbours; runs at Task 13 against the real account). Run — green (ignored).
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit:

```
feat: the signed-in player's skin from the profile endpoint (client)
```

**Verification:** `cargo test -p oxide-assets -p oxide-client`; the ignored case listed for Task 13; fmt/clippy.

---

### Task 11: The entity sound remainder (game)

**Goal:** M4 row 11's residual — "No entity sounds until M7" — is re-derived first-hand: the source's client-side sound call sites are censused against the M6 landed set, each residual site the port can trigger is wired with its tests, and if none remains the row closes as a finding with the census as its pinned evidence.

**Files:**
- Modify: as the census names them — expected homes: `crates/oxide-game/src/audio/sounds.rs` and the call-site modules (`session.rs`, `player.rs`, `hud.rs`); the task's report records every file it actually touched and why.
- Test: the census table as a pinned test (the final artifact either way) + per-site cases for anything wired.
- Evidence: the census and the receipt logs under `refs/m7-task-11/` (the M6 mixer-dump/event-log pattern).

**Interfaces:**
- No new public interfaces pinned; the census's table shape is pinned here for the report: `call site (file:method) | trigger | server-sent or client-side | landed by M6? | disposition (wired here / already covered / out of v1)`.
- Consumes: M4's row-11 record (`refs/m4-task-23/deferred-list.md`) and the M6 T9 landed-set record (`refs/m6-task-9/`), then a first-hand enumeration of the client source's sound call sites (`refs/_src/MCP-919`'s client tree; the net handler's 0x29 path already plays server-sent sounds via the M6 engine — the census is about the client's own call sites).

- [ ] **Step 1: The census.** Read the two records; enumerate; classify. Write the table into the report. No code yet.
- [ ] **Step 2: Wire the residual (RED first per site).** For each residual the port can trigger, a failing behavioural case first (the engine log/dump line as the observable), then the wiring. Where a site needs an event the port does not have, record the gap rather than force it.
- [ ] **Step 3: The census pin.** The classification table lands as a test asserting the covered set (so a future regression that drops a site fails loudly), plus the receipt run (`--audio-dump` or the event log over the scripted scene that exercises the closed sites). Run — green.
- [ ] **Step 4: Gate and commit.** Run the full gate. Commit one of — the census's outcome decides, and the report records which and why:

```
feat: close the entity sound remainder (game)
```

```
test: pin the client's sound call-site census (game)
```

**Verification:** `cargo test -p oxide-game`; the receipt log; fmt/clippy; the report's table cites every row's source site.

---

### Task 12: The player list with its header and footer (client)

**Goal:** The tab-held player list draws — the last owed half of the M6 tab-list work and parity checklist row 58: the column-split layout, the entry cells with their heads, names, scores and latency bars, and the centred header and footer over the existing `PlayerList`/header-footer data events, with the F1-adjacent draw case pinned alongside.

**Files:**
- Modify: `crates/oxide-client/src/main.rs` — the HUD draw path (the held data arms at `:2130`/`:6488`, the skin fan-out at `:6098-6117` are the data halves); the draw's home re-derives first from the M6 deferred rows 55/8's notes (the "held pair landed; draw half absent" record) — record which file the draw half actually lands in.
- Rider (T27 F1): the draw-level overlay-under-F1 case — re-derive from `refs/m6-task-27/` and pin per that record (the next `view.rs` round's case; if the draw lands elsewhere, the case lands beside it and the trigger note records the deviation).
- Test: the layout functions' unit fixtures (inline) + an ignored GPU case (the T16 precedent: one frame with a known list).

**Interfaces:**
- The draw consumes existing session events only (`PlayerList { entries }` from 0x38; the tab-list header/footer state from 0x47) — no session changes, no new wire work.
- Layout constants derive from `GuiPlayerTabOverlay.java` — the class that owns every number (`renderPlayerlist` `:70-234`: the pitch, cells, column split, cap, gutter, margins, panels, head, name and score fields; `drawPing` `:237-272`: the latency levels; `drawScoreboardValues` `:274` onwards); the checklist row's pointer `GuiIngame.java:348-358` is the held-Tab gate only (`:350-357`), kept as the gate cite, and the row's pointer corrects where the source differs (recorded). Pin each constant first-hand in the task (the plan pins the row's recorded set: 9 px pitch, 8 px cells, columns of ≤ 20 rows, cap 80 after the sort, gutter 5, margin 50, top 10; panels `0x80000000`; entry cells `0x20FFFFFF`; header/footer centred and shadowed; an 8×8 head at the cell's left from the skin). The sort's comparator and the sub-element set (names, scores, latency bars) derive from the source in-task and are pinned by tests.
- The layout math lands as pure functions (column split, per-cell rects, header/footer line centres) so the fixtures run headless.

- [ ] **Step 1: The layout fixtures (RED).** Column split at 21 and 41 entries; the cap at 81; a one-entry list; zero entries (nothing drawn); header/footer centring offsets against the cited constants. Run: `cargo test -p oxide-client` — red.
- [ ] **Step 2: The draw.** Panels, cells, heads (through the existing skin fan-out), names, scores, latency bars; the Tab-held gate. Run — green.
- [ ] **Step 3: The ignored GPU case.** One frame with a header, a footer and two entries carrying distinct skins; store-gated. Run — green (ignored).
- [ ] **Step 4: Rider T27 F1.** Re-derive and pin the overlay-under-F1 draw case per its record. Run — green.
- [ ] **Step 5: Gate and commit.** Run the full gate. Commit:

```
feat: draw the player list with its header and footer (client)
```

**Verification:** `cargo test -p oxide-client`; the GPU case listed for Task 13; fmt/clippy; the parity row 58's elements each traced to a case; the row's own text corrected in `docs/parity/checklist.md` only if the source contradicts it (recorded, per the standing rule).

---

### Task 13: The M7 acceptance

**Goal:** The exit sentence proven with receipts: join an online-mode server with a real account. The sign-in runs on this machine, the rig's own server flips to `online-mode=true`, the client joins it as the signed-in player, the encrypted session is witnessed by the proxy's boundary capture, the skin leg runs live, and the offline default is restored and re-verified. Any defect the run surfaces becomes its own scoped fix round before the close.

**Files:**
- Evidence: `refs/m7-acceptance/` — `report.md` (the RESUME convention), the client/server logs, the proxy capture, the screenshot, the skin receipts.
- No source changes are expected; a defect gets its own scoped fix (its own commit, then a scoped re-run), recorded in the report.

**Interfaces:**
- Consumes: Tasks 1–12 at their landed commits; `oxide-launcher auth` (Task 8) with a real account; the rig (`refs/rig/`, appendix C's procedures — server on :25565, the recorder proxy on :25566, the quiet-desktop capture discipline); the owner's sign-in cooperation (owner Q2/Q4).

- [ ] **Step 0: Preconditions.** The owner's Azure answer is in and the Minecraft API permission is granted (recorded; if still pending, the live legs stop at a recorded block); the final bytes' suite is green; the ignored sets re-run (including the Task 10 live profile case — network-gated); the desktop and scale checked before any rig leg; `gh auth status` before any later push is the standing check, not this task's.
- [ ] **Step 1: The sign-in leg.** `oxide-launcher auth` on this machine; the owner completes the browser side; `auth --status` lists the account; the keyring's entry exists (receipt: the status output plus the keyring item's name; values never read into evidence).
- [ ] **Step 2: The rig flip.** Back up `refs/rig/server/server.properties`; set `online-mode=true`; restart the server (it stays at :25565); start the recorder proxy LISTENING on :25566 with upstream :25565 (`python3 refs/rig/tools/record_proxy.py --listen 127.0.0.1:25566 --upstream 127.0.0.1:25565` — the M6 wire-leg arrangement); both ports free before, the startup log captured.
- [ ] **Step 3: The join.** Client `--server 127.0.0.1:25566 --account <name>` (the proxy's port — the capture exists only when the join travels through it; the direct :25565 path is not used this run): receipts — the client's log (join posted with the hash recorded, response sent, ciphers enabled, Login Success, skin fetched), the server console's authenticated join line (the uuid), the proxy capture (the plaintext handshake through the enablement boundary; the bytes after it recorded as opaque — state that plainly in the report), one screenshot of the joined world (the standing screenshot route; no second client session during capture — the rig rule).
- [ ] **Step 4: The skin leg.** The own profile texture fetched live (the cache file recorded by its stored name, plus the drawn result; the profile URL itself is public and may be cited); the Task 10 ignored case green against the live endpoint.
- [ ] **Step 5: The offline regression.** Revert `online-mode=false`; restart; the standing suites re-run; one offline join re-made (the M1–M6 path untouched); both ports free after.
- [ ] **Step 6: The refusal legs.** (a) Without `--account` against the online-mode server: the disconnected screen's `EncryptionRequired` sentence (one screenshot; live — it needs no token). (b) The join-failure sentence is proven at unit level (Task 9's harness — a wrong secret cannot be injected live without clobbering the stored refresh token, Decision 6), so its live leg is recorded out with that reason. Both records land in the report.
- [ ] **Step 7: The report.** `refs/m7-acceptance/report.md`: the exit sentence checked against each receipt; the by-eye list for the operator; every leg's evidence path; defects and their fix rounds (or none); the public-server leg recorded out (§14's anti-cheat risk; the rig satisfies the exit).

**Verification:** the exit sentence against the Step 3 receipts; the offline regression green; the report complete with RESUME FINAL once the run (and any fix) closes.

---

### Task 14: The close

**Goal:** M7 closes the M6 way: the branch frozen and reviewed whole, the verdict folded, the documents swept, the gate green, one close commit, the annotated tag, the push, CI watched to green, the run recorded, the ledger closed.

**Files:**
- Modify: `docs/STATE.md` (the stage line, the M7 evidence section, the caveats, the M8 next actions), `CHANGELOG.md`, a new `docs/handoff/<date>-m7-close.md`, `docs/perf.md` (only if the acceptance measured anything worth recording), `README.md` (only if touched), `docs/DIVERGENCES.md` (the milestone's recorded divergences: the roster reading, the §7.4 crate-name reading, the public-server leg), the plan itself (its task checks and the `*(Executed …)*` note).
- Evidence: `refs/m7-task-14/` — the freeze hashes, the final review, the content-gate receipts, the gate log.

- [ ] **Step 1: The freeze.** Hash the reviewed range's diff (`git diff <base>..<head>` → size + sha256); reviewers see only the frozen bytes.
- [ ] **Step 2: The final whole-branch review.** One independent pass over the frozen range: the bar is no Critical and no Important findings; the report carries the per-item verdicts, the mutation back-outs, the count re-derivation, and the cross-cutting section on what M8 and M9 will trip on (the next friction list). Written incrementally into `refs/m7-task-14/final-review.md`.
- [ ] **Step 3: Fold.** Record-only and minor rulings into the close drafts; any Critical/Important routes fix rounds and the close stops until they clear.
- [ ] **Step 4: The docs sweep.** The files listed above, each entry checked: STATE's stage line moves to M8; the M7 evidence section cites the acceptance report and the review; the caveats carry every re-carry and recorded divergence; the handoff is self-contained (the M6 handoff is its shape).
- [ ] **Step 5: The content gate.** The banned-vocabulary scan over added lines and the new documents; the asset guard; both clean.
- [ ] **Step 6: The gate.** The full command set to `GATE OK`; the exit taken via `PIPESTATUS[0]`; the log's gate-root line confirmed to name the repository.
- [ ] **Step 7: The close commit.** Explicit paths; commit:

```
docs: close out milestone M7 (online play)
```

- [ ] **Step 8: The tag and the push.** `git tag -a m7 -m "M7: online play"`; `git push origin main --follow-tags`; read back `git ls-remote` after the push.
- [ ] **Step 9: The CI watch.** The workflow run on the close head watched to completion; every job green.
- [ ] **Step 10: The run record.** Commit and push:

```
docs: record the M7 close-out CI run
```

- [ ] **Step 11: The ledger close.** The execution ledger's close block: the shas, the counts, the receipts, the passed state.

**Verification:** the reviewer's verdict; the content gate's clean run; `GATE OK` on the final revision; CI green on the pushed head; the tag's read-back; the run-record commit pushed and read back.

---

## Coverage table

*(Task numbers final at assembly; every binding item disposed.)*

| Binding item | Where the plan answers it | Evidence |
| --- | --- | --- |
| Spec §13 row **M7 Microsoft auth** — device code flow (5), Xbox Live and XSTS chain (5), entitlement checks (6), keyring token storage (7, owner Q1), encrypted session with the crypto invariants (1–4), client-side session join (3, 9), hexdigest helper (1), online-mode connection (9, 13); "Azure application and API permission granted before this milestone starts" (owner Q2; the close records the status) | 1–9, 13 | The stub corpora; the mock login harness; the acceptance receipts |
| Exit **"Join an online-mode server with a real account"** | 13 | The join receipts (Step 3) |
| §7.4 bullet 1 (device code flow) | 5, 8 | The stub corpus + the live leg (13) |
| §7.4 bullet 2 (XBL then XSTS) | 5 | The exact-body fixtures |
| §7.4 bullet 3 (`login_with_xbox`, entitlement and profile checks) | 6 | The stage-two corpus |
| §7.4 bullet 4 ("Session join, performed by the **client** … before the encryption response is sent" + "an injection point that accepts the pre-computed hash") | 3 (the seam, the ordering), 9 (the production join) — the plan reads "oxide-proto's login state" as the login flow's injection seam, landed where the flow lives (`oxide-game`); the reading is recorded at the close (Decision 2) | The mock harness's ordering case; the acceptance capture |
| §7.4 bullet 5 + the five crypto invariants (SPKI parse with modulus-sized ciphertext; CFB8 8-bit segments; two continuous contexts; PKCS#1 v1.5; the hexdigest with its three goldens) | 1 (each its own case), 2 (continuity at the stream), 3 (the ordering) | The golden vectors; the KAT; the continuity cases; the modulus-sized case |
| §7.4's token sentence ("OS keyring, never in plaintext files") | 7 (Decision 5/6; owner Q1) | The no-secret file assertion; the tier-3 case |
| §16 Azure row (owner action; fallback "a documented alternative or online mode leaves v1") | Owner Q2; the close's records | The answers table; the close's caveats |
| Spec §12 testing row — "the three hexdigest golden vectors" | 1 | The goldens |
| Spec §3 **F13** — "Microsoft account login and encrypted sessions with online-mode servers (milestone M7)" | 1–10, 13 | The suite plus the acceptance |
| §5.3 key items — `oxide-launcher auth` (8), `oxide-game` "session join for online mode" (3, 9), `oxide-proto` encryption (1, 2), `oxide-assets` "remote skin textures" (10) | 1–3, 8–10 | The task suites |
| STATE backlog 1 — M7's own scope: "the online-mode fetch and session wiring" (3, 9, 10 — the fetch's live evidence at 13), "the sounds' remaining rows" (11), "the skin fetch" (10), "the server-list roster half" (12; the reading = owner Q5) | 3, 9–13 | The task records |
| STATE backlog 2 — the acceptance-triaged render gaps: F8 (re-carried: next capture lap — the acceptance's captures are its own legs; no new re-take opened here), F9 (re-carried: its own tooling round; no capture-tooling work in this plan), F11 (re-carried: the view-overlay round — not opened here; Task 12's draw is HUD, not the render overlay), F13 (re-carried: the Nether-transit round), F14 (re-carried: the routed look-verified session matrix; this acceptance's legs are join-focused) | Re-carried as stated; the close re-states each trigger | The close's caveats |
| STATE backlog 3 — the coverage pins: T27 F1 (12), T27 F2 (9), T27 F3 (3); T25 M1/M2 (re-carried: the next perspective/fullscreen touch — no such file opened here), T14 M-b (re-carried: the next renderer touch — `set_fov`'s home is not opened here), T14 M-c (3); the lagometer riders (re-carried: the next `lagometer.rs` touch), the atlas-count re-survey (4) | As listed | The task records + the close's caveats |
| STATE backlog 4 — the still-open comment/citation rows (the out-of-scope stale cites; D14/D16/D19/D26/D30; 73b/73c; 78b; 83a/83c; A1–A3; Backlog 7/10; T8-F7; the half-slab fixture): each keeps its per-file trigger; several cite files this milestone opens (`session.rs`, `serverbound.rs`, `atlas.rs`, `main.rs`) without firing at the touch (their triggers are sweeps/rounds — e.g. D5's `atlas.rs:1065`, the comment sweeps); each touching task's report lists the rows it passed, and the close re-checks; the half-slab fixture activates only when a protocol surface needs it (none here) | Re-carried; the close re-checks | The close's caveats |
| STATE backlog 5 — the click-action mouse-button binds (re-carried: the next keymap round), the tab-list/header-footer draws (12) | As listed | Task 12's cases |
| STATE backlog 6 — the operator's list | 13 keeps it current | The report |
| STATE backlog 7 — later homes recorded for M9 (the animation sweep incl. M4 row 10's mob roster, overflows, interest management, the F3 FPS-readout gap, the vanilla frame rate) | Stand as recorded; the close names M4 row 10 explicitly | The close |
| M6 final review line 155 (the friction list): tab-list draw absent (12); Fancy clouds unported (re-carried: a clouds round); legacy skins unhandled (re-carried: row 6's own trigger — M7's skin work is the modern 64×64 path); F11-adjacent pins T25 M2 / T28 M3 (re-carried: their touches); session `GameType`/`CameraClip` pins (3); the fire-overlay render gap (re-carried: the view-overlay round); the teleport-look matrix (re-carried: its routed matrix); the window icon (re-carried: recorded out with its T29 receipt; store plumbing still unowned); the sounds remainder (11); the unicode stepdown (re-carried: one-font reason recorded) | As listed | The task records + the close's caveats |
| M4 deferred rows at this milestone's files: 7 (skin fetch → M7) → 10 + 13; 11 (sounds → M7) → 11; 55/8 (header/footer + tab-list draw halves) → 12. Every other M4/M5/M6-era row stands per `refs/m6-task-31/deferred-list.md`; none revisits a file this milestone opens | As listed | The close's sweep |
| Checklist row 58 (player list: header/footer, entry columns, heads, names, scores, latency bars) | 12 (the constants' home `GuiPlayerTabOverlay.java`; the row's pointer corrected where the source differs — recorded) | The layout fixtures + the GPU case |
| Checklist row 12 (scoreboard sidebar) — untouched this milestone; its M4 acceptance evidence stands | Record | — |
| The sound rows — standing since M6's engine; the entity residual closes at 11 | 11 | The census + receipts |
| Dependency additions: `aes`, `cfb8`, `rsa`, `rand`→proto (1); `keyring` (7); `x509-cert` only if Task 1's derivation needs it; each with its licence in `deny.toml` in the same commit | 1, 7, owner Q3 | `cargo deny check` green |
| The recorded divergences/readings this plan introduces: the §7.4 crate-name reading (Decision 2), the roster reading (Decision 12 / Q5), the public-server leg recorded out | The close's DIVERGENCES/STATE sweep | The close commit |

---

## Pre-flight (before Task 1)

- The owner's answers to the five questions above are recorded in the answers table before execution begins.
- The plan's base: `24fb38a` (= `origin/main`, clean at planning; the M6 close-out run-record commit; the close commit `c90313b` carries the annotated tag `m6`; the M6 close CI run `38496133039` green, all seven jobs; suite 2500 tests / 167 ignored at the close).
- The execution ledger begins (first line names this plan); the reference tree's HEAD is recorded with it: `1717f75902c6184a1ed1bfcd7880404aab4da503`.
- The conflict scan (every task pair sharing a file or interface, plus one self-consistency row per task) is written to the ledger, and each conflict is ruled on before the first dispatch.
- Dependency resolution dry-run: candidate `aes`/`cfb8`/`rsa`/`keyring` versions checked against MSRV 1.85 (the newest whose declared rust-version fits; record the resolution before the first landing commit); `keyring`'s Linux backend (Secret Service) noted against this machine's session.
- The client id: RECORDED at approval (owner Q2 — the answers table); it lands as `AUTH_CLIENT_ID`'s value in Task 6. The Minecraft API permission is submitted and pending: Task 13's live legs wait on it, while Tasks 1–12 land regardless (the environment override drives every test; the constant's value never affects one).
- `rustup update stable` before the first gate; the desktop and its scale re-checked before any rig leg; `gh auth status` before any push.
- The full gate runs clean on the base revision before Task 1's dispatch.
- The rig's `server.properties` backup is taken by Task 13's own step; nothing pre-flips.

---

## Known limits this plan accepts

*(initial list; execution refines it, and Task 14's sweep carries the final wording)*

1. The public-server leg is recorded out: the exit is proven on the rig's own server with `online-mode=true`; playing on public servers is §14's documented non-guarantee (anti-cheat risk noted, nothing attempted).
2. The keyring persists only where a session Secret Service runs (kwallet/gnome-keyring); everywhere else the tier-3 rule applies — no persistence, a fresh sign-in each session. Tier 2 (encrypted-at-rest file) is not implemented (owner Q1).
3. The keyring threat model is the survey's: other processes running as this user can enumerate entries; the mitigation is the OS session's own access control, not new cryptography.
4. Child/region/banned accounts surface the XErr sentences (§3.3's table, `2148916238`'s custom-app note included); the fix is the account holder's, not code.
5. The join's `ip`/`prevent-proxy-connections` corners and the server's `hasJoined` side are exercised only through the rig's own server behaviour; the vanilla server remains the comparison witness.
6. The proxy capture cannot meaningfully record the encrypted portion (by construction); the evidence is the boundary — the plaintext handshake up to enablement, then liveness of the encrypted stream.
7. The device code's 15-minute window paces the acceptance; a lapsed code is retried, recorded.
8. The own-skin live leg needs the network and the account's profile to have a texture; an account without one records the default-skin fallback as the finding.
9. The player list's sub-elements (scores, latency bars) derive from the source and its row; any sub-element the 1.8 source's multiplayer path cannot populate records as a finding rather than invented content.
10. The sounds remainder may close as the census alone (no residual sites) — the task's outcome decides which commit lands, and the census is the evidence either way.
11. The "server-list roster half" reading (Q5) and the §7.4 crate-name reading are recorded readings, not source facts; the close keeps both traceable.
12. Every re-carried family (acceptance F8/F9/F11/F13/F14, T25 M1/M2, T28 M3, T14 M-b, the lagometer riders, the unicode stepdown, the window icon, Fancy clouds, legacy skins, the comment rows, the half-slab fixture) stands with its trigger; none is silently closed.
13. Microsoft-side drift is the standing §14 risk: the chain is isolated behind `Auth`/`AuthEndpoints` so a change lands in one module; nothing here can freeze Mojang's endpoints.
14. The acceptance's screenshot legs follow the rig's capture discipline (quiet desktop, no second client); a missed shot is retried, then recorded — never faked.

- The Minecraft API permission (owner Q2): the client id is recorded; the whitelist is submitted and pending — Task 13's live legs wait on it (a still-pending state is recorded as a block, not a pass).
