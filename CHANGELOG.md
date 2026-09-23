# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.0] — Milestone M0: foundations

Released 2026-09-23. The first tagged milestone. The repository has no releases to upgrade from;
this entry records what the milestone established.

### Added

- Eight-crate Cargo workspace with an enforced dependency graph.
- VarInt codec and length-prefixed framing with the 1.8 compression rules, both with golden-byte
  tests.
- Handshake, status ping, and the launcher CLI skeleton.
- Hash-verified atomic asset store.
- piston-meta metadata chain: version manifest, version document and asset index, each verified by
  hash.
- Full `fetch --verify` flow against the real distribution endpoints.
- Jar extraction with a manifest, refusing `.class` entries.
- wgpu window with an FPS counter and adapter logging.
- CI with format, lint, test, MSRV, portability, crate-graph, asset-guard, and licence jobs.
