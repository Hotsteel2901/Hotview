#!/bin/sh
# Single source of truth for the package/release version.
#
# The release is numbered from the commit history, exactly like the Release
# workflow: tag = 10000 + commits, title = v1.(commits/100).(commits%100).
# Set HOTVIEW_VERSION to override (for example when packaging a tarball).
set -eu

if [ -n "${HOTVIEW_VERSION:-}" ]; then
    printf '%s\n' "$HOTVIEW_VERSION"
    exit 0
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

if command -v git >/dev/null 2>&1 &&
    git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    commits=$(git -C "$root" rev-list --count HEAD 2>/dev/null || true)
    if [ -n "$commits" ]; then
        printf '1.%s.%s\n' "$((commits / 100))" "$((commits % 100))"
        exit 0
    fi
fi

sed -n 's/^version *= *"\([^"]*\)".*/\1/p' \
    "$root/crates/hotview-desktop/Cargo.toml" | head -n 1
