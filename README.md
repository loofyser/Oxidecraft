# Oxidecraft

A from-scratch re-implementation of the Minecraft Java Edition client, written in Rust.

Oxidecraft targets Java Edition 1.8.9 (protocol 47) first. The goal is a client that
plays, feels and looks the same as the official Java Edition client, while being
faster, more stable, lighter, and open source.

## Status

Pre-alpha. This repository currently contains design documentation only.

## Scope of the first release

- A Linux-native client (cross-platform by design) with a `wgpu` renderer.
- A launcher that fetches the 1.8.9 asset index and asset objects from Mojang's
  piston-meta service.
- Multiplayer: connect to a vanilla 1.8.9 server, render the world, move, look,
  chat, and place or break blocks.
- Microsoft account login is planned as a separate later milestone.

## Repository layout (planned)

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

Oxidecraft is not affiliated with Mojang Studios or Microsoft. It ships no Mojang
code, textures, sounds or other game assets. Assets used at runtime are downloaded
from Mojang's public distribution service by the user's own launcher, exactly as
the official launcher does. The client jar is used only as a local resource source
and is never redistributed.

## License

GNU General Public License v3.0. See `LICENSE`.
