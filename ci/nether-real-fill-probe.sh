#!/usr/bin/env bash
# nether-real-fill-probe.sh — run vanilla's OWN `doFill` over a real ProtoChunk.
#
#   ci/nether-real-fill-probe.sh 20260722 915 1028
#
# The control for `ci/nether-fill-probe.sh`. That probe drives the cell walk
# with a loop transcribed out of `NoiseBasedChunkGenerator.doFill`'s bytecode;
# this one builds a real `ProtoChunk`, lets `NoiseChunk.forChunk` construct the
# noise chunk exactly as the server does, and calls the private `doFill`
# itself. Same output format, so the two `diff` directly.
#
# It exists because the transcribed probe became load-bearing: it is the
# evidence behind "the nether fill is byte-identical over 65,536 blocks" in
# `docs/PARITY.md`. Corroborated is not verified, and an instrument that has
# been quoted onward should be the first thing checked, not the last.
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

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/NetherRealFillProbe.java" 2>&1 |
    grep -v '^Note' || true

# Vanilla's bootstrap logs to stderr; the probe's own lines go to stdout, and
# the `[STDOUT]:` prefix is the logger's, not ours.
java -cp "$CP:$WORK" NetherRealFillProbe "$@" 2>/dev/null | sed 's/^.*\[STDOUT\]: //'
