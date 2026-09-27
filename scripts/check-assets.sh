#!/usr/bin/env bash
# Fails when a Mojang binary, jar, sound, image, language file, class file, or local
# reference tree is tracked.
#
# Matches are case-insensitive and run against every tracked path, so neither a
# subdirectory nor a differently-cased extension can slip past the guard.
#
# Images are refused anywhere: the only screenshots this project produces live under
# the git-ignored refs/ tree, so a committed image is never legitimate.
#
# The scan runs over a path list, so `--self-test` can feed it fixture lists and
# assert that a clean list passes while forbidden paths are refused.
set -euo pipefail

for tool in git grep; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "check-assets: required tool '$tool' is not installed" >&2
    exit 1
  fi
done

# No game file is ever tracked, at any depth: jar, sound, class, image and language
# files, glyph tables, asset-index JSON at any nesting, and local reference trees.
# A broken grep (exit status 2 or more) is a guard failure, not a pass.
binary_pattern='\.jar$|\.ogg$|\.class$|\.png$|\.lang$|glyph_sizes\.bin$|assets/indexes/.*\.json$|(^|/)(refs|vanilla)/'

# Scans a path list on stdin. Prints every forbidden path and returns non-zero when
# the list contains one; a broken grep stays a guard failure, not a pass.
scan_paths() {
  local status=0
  local matches
  matches="$(grep -Ei "$binary_pattern")" || status=$?
  if [ "$status" -gt 1 ]; then
    echo "check-assets: grep failed (exit $status) while scanning the path list" >&2
    return 1
  fi
  if [ -n "$matches" ]; then
    echo "forbidden tracked files:" >&2
    echo "$matches" >&2
    return 1
  fi
  return 0
}

# Feeds fixture lists to scan_paths and exits non-zero unless every case behaves as
# stated. The fixture paths stand in for tracked paths; none touches the disk.
self_test() {
  local fail=0
  if printf '%s\n' 'src/main.rs' 'docs/STATE.md' | scan_paths; then
    echo "self-test: clean list passes"
  else
    echo "self-test: clean list was refused" >&2
    fail=1
  fi
  if printf '%s\n' 'src/main.rs' 'assets/indexes/1.8.json' | scan_paths >/dev/null 2>&1; then
    echo "self-test: asset index was not refused" >&2
    fail=1
  else
    echo "self-test: asset index refused"
  fi
  if printf '%s\n' 'src/main.rs' 'refs/rig/evidence/m1/shot.png' | scan_paths >/dev/null 2>&1; then
    echo "self-test: refs path was not refused" >&2
    fail=1
  else
    echo "self-test: refs path refused"
  fi
  return "$fail"
}

if [ "$#" -gt 0 ]; then
  if [ "$#" -gt 1 ] || [ "$1" != "--self-test" ]; then
    echo "check-assets: unrecognised arguments: $*" >&2
    exit 1
  fi
  if ! self_test; then
    echo "check-assets: self-test failed" >&2
    exit 1
  fi
  exit 0
fi

if ! tracked="$(git ls-files)"; then
  echo "check-assets: git ls-files failed, cannot check the tracked tree" >&2
  exit 1
fi

if ! printf '%s\n' "$tracked" | scan_paths; then
  exit 1
fi

status=0
count="$(grep -c . <<<"$tracked")" || status=$?
if [ "$status" -gt 1 ]; then
  echo "check-assets: grep failed (exit $status) while counting the tracked tree" >&2
  exit 1
fi

echo "asset guard: clean ($count tracked files checked)"
