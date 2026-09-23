#!/usr/bin/env bash
# Fails when any workspace crate depends on something outside the allowed edges.
#
# The allowed edges are exactly the 16-edge table in section 5.1 of docs/specs/oxidecraft-v1-design.md.
# This script fails closed: a missing tool or unreadable metadata is an error, never a pass.
set -euo pipefail

for tool in cargo jq grep; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "check-graph: required tool '$tool' is not installed" >&2
    exit 1
  fi
done

# The crates named in the section 5.1 table. Their absence means the metadata
# we are about to read is not the workspace we think we are checking.
expected_crates=(
  oxide-proto
  oxide-proto-v47
  oxide-world
  oxide-assets
  oxide-render
  oxide-game
  oxide-launcher
  oxide-client
)

allowed() {
  case "$1 -> $2" in
    "oxide-proto-v47 -> oxide-proto") return 0 ;;
    "oxide-world -> oxide-proto" | "oxide-world -> oxide-proto-v47") return 0 ;;
    "oxide-render -> oxide-assets") return 0 ;;
    "oxide-game -> oxide-proto-v47" | "oxide-game -> oxide-world" | "oxide-game -> oxide-assets" | "oxide-game -> oxide-render") return 0 ;;
    # The client takes the rows above it in the table, not the launcher row: the
    # launcher is a separate binary target and the client does not depend on it.
    "oxide-client -> oxide-proto" | "oxide-client -> oxide-proto-v47" | "oxide-client -> oxide-world" | "oxide-client -> oxide-assets" | "oxide-client -> oxide-render" | "oxide-client -> oxide-game") return 0 ;;
    "oxide-launcher -> oxide-assets" | "oxide-launcher -> oxide-proto-v47") return 0 ;;
    *) return 1 ;;
  esac
}

if ! metadata="$(cargo metadata --format-version 1 --no-deps)"; then
  echo "check-graph: cargo metadata failed, cannot check the crate graph" >&2
  exit 1
fi

if ! members="$(jq -r '.packages[].name' <<<"$metadata" | sort)"; then
  echo "check-graph: could not read package names from cargo metadata" >&2
  exit 1
fi

missing=0
for crate in "${expected_crates[@]}"; do
  if ! grep -qx "$crate" <<<"$members"; then
    echo "check-graph: workspace crate '$crate' is missing from cargo metadata" >&2
    missing=1
  fi
done
[ "$missing" -eq 0 ] || exit 1

if ! edges="$(jq -r '.packages[] | .name as $from | .dependencies[] |
  select(.name | startswith("oxide-")) | "\($from) -> \(.name)"' <<<"$metadata" | sort -u)"; then
  echo "check-graph: could not read dependency edges from cargo metadata" >&2
  exit 1
fi

fail=0
count=0
while IFS= read -r edge; do
  [ -n "$edge" ] || continue
  count=$((count + 1))
  from="${edge%% -> *}"
  to="${edge##* -> }"
  if ! allowed "$from" "$to"; then
    echo "forbidden dependency edge: $edge" >&2
    fail=1
  fi
done <<<"$edges"

if [ "$fail" -ne 0 ]; then
  echo "crate graph check failed" >&2
  exit 1
fi

echo "crate graph: clean ($count workspace-internal edges checked)"
