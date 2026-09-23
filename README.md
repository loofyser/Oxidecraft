# Oxidecraft

**NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.**

Oxidecraft is an independent project. It is not affiliated with, endorsed by, or associated with
Mojang Studios or Microsoft, and it ships no game assets or code; running it requires a legitimate
copy of Minecraft Java Edition that you own.

A from-scratch re-implementation of the Minecraft Java Edition client, written in Rust.

Oxidecraft targets Java Edition 1.8.9 (protocol 47) first. The goal is a client that
plays, feels and looks the same as the official Java Edition client, while being
faster, more stable, lighter, and open source.

## Development status

Pre-alpha, and past the groundwork: milestone M0 (foundations) is complete — the workspace, the
verified asset pipeline, jar extraction and the wgpu window all work, and CI is green. Milestone M1
(bytes to world) is next. See `docs/STATE.md` for where the project stands, what is verified, and
the evidence.

## Building

Requires a Rust toolchain 1.85 or newer (edition 2024).

```bash
cargo build --workspace
cargo run -p oxide-launcher -- fetch --version 1.8.9
cargo run -p oxide-client
```

## Scope of the first release

- A Linux-native client (cross-platform by design) with a `wgpu` renderer.
- A launcher that fetches the 1.8.9 asset index and asset objects from Mojang's
  piston-meta service.
- Multiplayer: connect to a vanilla 1.8.9 server, render the world, move, look,
  chat, and place or break blocks.
- Microsoft account login is planned as a separate later milestone.

## Repository layout

| Path | Contents |
| --- | --- |
| `crates/oxide-proto` | Protocol framing, serialization, compression, encryption |
| `crates/oxide-proto-v47` | Packet definitions for protocol 47 (1.8.9) |
| `crates/oxide-world` | Chunk storage, lighting, block registry, entities |
| `crates/oxide-assets` | piston-meta client, asset store, texture atlas, models, fonts |
| `crates/oxide-render` | `wgpu` renderer: terrain, entities, GUI, text, sky |
| `crates/oxide-game` | Client logic: physics, input, GUI, HUD, gameplay |
| `crates/oxide-launcher` | Launcher binary: assets, profiles, accounts, launch |
| `crates/oxide-client` | Client binary |
| `docs/specs` | Design specifications |
| `docs/research` | Research notes gathered during design |

## Legal

Game assets used at runtime are downloaded from Mojang's public distribution service by the user's
own launcher, exactly as the official launcher does. The client jar is used only as a local
resource source and is never redistributed. Nothing Mojang-made is committed to this repository.

## License

GNU General Public License v3.0 only (GPL-3.0-only). See `LICENSE`; third-party attribution notes
are in `NOTICE`.
