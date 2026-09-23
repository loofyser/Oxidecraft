#!/usr/bin/env bash
# Fails when a Mojang binary, jar, sound, class file, or local reference tree is tracked.
#
# Matches are case-insensitive and run against every tracked path, so neither a
# subdirectory nor a differently-cased extension can slip past the guard.
set -euo pipefail

if ! command -v git >/dev/null 2>&1; then
  echo "check-assets: required tool 'git' is not installed" >&2
  exit 1
fi

if ! tracked="$(git ls-files)"; then
  echo "check-assets: git ls-files failed, cannot check the tracked tree" >&2
  exit 1
fi

# Game binaries are never tracked, at any depth.
binary_pattern='(^|/)assets/indexes/[^/]*\.json$|\.jar$|\.ogg$|\.class$|glyph_sizes\.bin$|(^|/)(refs|vanilla)/'
matches="$(grep -Ei "$binary_pattern" <<<"$tracked" || true)"
if [ -n "$matches" ]; then
  echo "forbidden tracked files:" >&2
  echo "$matches" >&2
  exit 1
fi

# PNGs are allowed only outside asset-shaped paths.
png_pattern='(^|/)(assets|textures|gui)/.*\.png$|(^|/)src/.*\.png$'
matches="$(grep -Ei "$png_pattern" <<<"$tracked" || true)"
if [ -n "$matches" ]; then
  echo "forbidden tracked image under an asset-shaped path:" >&2
  echo "$matches" >&2
  exit 1
fi

echo "asset guard: clean ($(grep -c . <<<"$tracked") tracked files checked)"
