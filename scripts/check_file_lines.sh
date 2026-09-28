#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Enforce the project file-size cap: ≤256 lines per source file.
# Always scans the monorepo root (parent of scripts/), never the caller cwd.
set -euo pipefail

LIMIT=256
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT" || exit 1

echo "Checking file sizes (limit=${LIMIT}, root=${ROOT})..."

# Rust-only, per .github/RULES.md §1. A C header is a unit of
# linkage rather than a unit of thought, and CI scripts are better
# served by shellcheck than by a page-size rule borrowed from Rust.
# Owned product/source only — exclude build artifacts, VCS, agent
# noise, container overlays.
OVERSIZE=$(find . -type f -name "*.rs" \
    -not -path "*/target/*" \
    -not -path "*/dist/*" \
    -not -path "*/.git/*" \
    -not -path "*/.agents/*" \
    -not -path "*/node_modules/*" \
    -not -path "*/.local/*" \
    -not -path "*/containers/*" \
    -not -path "*/.cache/*" \
    -exec wc -l {} + \
    | awk -v limit="$LIMIT" '$1 > limit && $2 != "total" {print $1, $2}')

if [ -n "$OVERSIZE" ]; then
    echo "ERROR: The following files exceed the $LIMIT line limit:"
    echo "$OVERSIZE"
    echo ""
    echo "Entropy (S) is too high. Split these files."
    exit 1
fi

echo "Success! No oversized files found (cap=${LIMIT})."
exit 0
