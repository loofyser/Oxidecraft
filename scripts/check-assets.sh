#!/usr/bin/env bash
# Fails when a Mojang binary, jar, sound, image, language file, class file, or local
# reference tree is tracked.
#
# Matches are case-insensitive and run against every tracked path, so neither a
# subdirectory nor a differently-cased extension can slip past the guard.
#
# Images are refused anywhere: the only screenshots this project produces live under
# the git-ignored refs/ tree, so a committed image is never legitimate.
set -euo pipefail

for tool in git grep; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "check-assets: required tool '$tool' is not installed" >&2
    exit 1
  fi
done

if ! tracked="$(git ls-files)"; then
  echo "check-assets: git ls-files failed, cannot check the tracked tree" >&2
  exit 1
fi

# No game file is ever tracked, at any depth: jar, sound, class, image and language
# files, glyph tables, asset-index JSON at any nesting, and local reference trees.
# A broken grep (exit status 2 or more) is a guard failure, not a pass.
binary_pattern='\.jar$|\.ogg$|\.class$|\.png$|\.lang$|glyph_sizes\.bin$|assets/indexes/.*\.json$|(^|/)(refs|vanilla)/'
status=0
matches="$(grep -Ei "$binary_pattern" <<<"$tracked")" || status=$?
if [ "$status" -gt 1 ]; then
  echo "check-assets: grep failed (exit $status) while scanning the tracked tree" >&2
  exit 1
fi
if [ -n "$matches" ]; then
  echo "forbidden tracked files:" >&2
  echo "$matches" >&2
  exit 1
fi

status=0
count="$(grep -c . <<<"$tracked")" || status=$?
if [ "$status" -gt 1 ]; then
  echo "check-assets: grep failed (exit $status) while counting the tracked tree" >&2
  exit 1
fi

echo "asset guard: clean ($count tracked files checked)"
