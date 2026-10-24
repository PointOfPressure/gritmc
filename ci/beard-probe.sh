#!/usr/bin/env bash
# beard-probe.sh — vanilla's own `Beardifier`, as a per-component oracle.
#
#   ci/beard-probe.sh kernel
#   ci/beard-probe.sh compute <x0> <y0> <z0> <x1> <y1> <z1> <piece>...
#     piece = x0,y0,z0,x1,y1,z1,ADJUSTMENT[,groundLevelDelta]
#
# The beard is the first change in this sequence with no byte-identical gate —
# it is meant to change terrain — so the composed residual cannot say which of
# five pieces is wrong. `kernel` dumps the 13,824-entry BEARD_KERNEL as raw
# float bits; `compute` evaluates Beardifier.compute over a block range for a
# given piece list. Each is a direct oracle for one component.
#
# Runs a JVM, so it goes on the Threadripper like everything else that runs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/BeardProbe.java" 2>&1 |
    grep -v '^Note' || true

# Vanilla's bootstrap logs to stderr; the probe's own lines go to stdout, and
# the `[STDOUT]:` prefix is the logger's, not ours.
java -cp "$CP:$WORK" BeardProbe "$@" 2>/dev/null | sed 's/^.*\[STDOUT\]: //'
