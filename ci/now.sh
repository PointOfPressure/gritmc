#!/usr/bin/env bash
# now.sh — the ten-second orientation for a session with no memory of the last.
#
# `docs/WORKLIST.md` is ~7,000 lines and exceeds a single read. Its first
# section, `## NOW`, is the exception: exactly one open item, why it is the
# next one, the command that reproduces its measurement, and what result would
# close it. Everything below `## NOW` is either the current state of item 1 or
# archive.
#
# **The `## NOW` block is rewritten, not appended to, at the end of every
# turn.** That is the whole contract. A turn that leaves it describing work
# already committed has broken the one thing a fresh session depends on.
#
#   ci/now.sh
#
# Prints the block, the last commits, and anything uncommitted — which is the
# other half of the state, since a turn can be cut off mid-increment.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

awk '/^## NOW/{f=1} f && /^## / && !/^## NOW/{exit} f' docs/WORKLIST.md

echo
echo "=== last commits ==="
git log --oneline -6

echo
echo "=== uncommitted (finish and commit this before starting anything new) ==="
if [ -z "$(git status --porcelain)" ]; then
    echo "  (clean)"
else
    git status --short
fi

echo
echo "=== gate ==="
echo "  ./ci/t0-remote.sh >/dev/null 2>&1; T=\$?; if [ \$T -eq 0 ]; then git add -A && git commit ...; fi"
echo "  (Threadripper unreachable? CARGO_BUILD_JOBS=3 ./ci/t0.sh, same guard, and say you fell back)"
