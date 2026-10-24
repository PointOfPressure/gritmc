#!/usr/bin/env bash
# binary-is-current.sh — is this release binary built from the tree it sits in?
#
# The check every worldgen artefact needs, because a binary older than the tree
# produces self-consistent dumps and a delta of exactly zero, which reads as
# "the change did nothing" rather than as "nothing was rebuilt".
#
# `ci/nether-biome-window.sh` introduced it as:
#
#     find crates tools -type f \( -name '*.rs' -o -name '*.toml' \) -newer "$b"
#
# and that FALSE-REFUSES, which this branch hit twice in ten minutes and lost
# two rebuild cycles to. The scope is wrong: it compares one binary against
# EVERY source file in the workspace, and `cargo` does not relink a binary
# whose own dependency graph is unchanged. So editing `region-differ`'s source
# makes `worldgen-dump` look stale for ever — the guard cannot be satisfied by
# rebuilding, only by touching something, which is exactly the reflex a
# staleness guard exists to prevent.
#
# The right scope is the binary's OWN dependency graph, and cargo already
# writes it: `target/release/<name>.d` is a make rule whose prerequisites are
# every source file that binary was compiled from. Comparing against those is
# both stricter (a `.d` lists generated sources a `find` over `crates tools`
# would miss) and correct (an unrelated crate cannot red-line it).
#
# Falls back to the workspace-wide find when no `.d` exists, because a missing
# dep file must not silently turn the check off — a guard that cannot fail is
# not a guard.
#
# Usage:  ci/binary-is-current.sh <path-to-binary>  || exit 2
set -uo pipefail
B=${1:?usage: binary-is-current.sh <binary>}
[ -x "$B" ] || { echo "  $B is missing — cargo build --release. REFUSING." >&2; exit 2; }
D="${B}.d"
if [ -f "$D" ]; then
    # Everything after the first colon, whitespace-separated. Paths with
    # spaces would break this and cargo does not emit them for this workspace;
    # if one ever appears the `[ -f ]` below simply skips it and the fallback
    # find still guards the tree.
    newer=""
    while read -r dep; do
        [ -f "$dep" ] || continue
        if [ "$dep" -nt "$B" ]; then newer=$dep; break; fi
    done < <(sed 's/^[^:]*://' "$D" | tr ' ' '\n' | sed '/^$/d')
    if [ -n "$newer" ]; then
        echo "  $B is OLDER than $newer, which it was compiled from — it does not contain this tree. REFUSING." >&2
        exit 2
    fi
    exit 0
fi
echo "  $B has no dependency file beside it; falling back to a workspace-wide check." >&2
newer=$(find crates tools -type f \( -name '*.rs' -o -name '*.toml' \) -newer "$B" -print -quit 2>/dev/null)
[ -z "$newer" ] || { echo "  $B is OLDER than $newer — it does not contain this tree. REFUSING." >&2; exit 2; }
