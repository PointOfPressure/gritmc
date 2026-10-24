#!/usr/bin/env bash
# aquifer-probe.sh — ask vanilla's own aquifer for a centre or a fluid level.
#
# The counterpart to `RUSTMC_TRACE_AQUIFER` / `RUSTMC_TRACE_AQUIFER_CENTRE`:
# those print our numbers, this prints vanilla's, for the same lattice cell at
# the same seed. Diff the two and the answer is a fact rather than a reading.
#
#   ci/aquifer-probe.sh 20260722 centre:60,1,67 spread:60,0,67 flood:968,17,1076
#
# Unlike the golden scripts this writes no vector file. It is a probe, not a
# gate: the cells worth asking about are the ones a residual points at, and
# freezing an arbitrary set of them would gate the wrong thing.
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

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/AquiferProbe.java" 2>&1 |
    grep -v '^Note' || true

# Vanilla's bootstrap logs to stderr; the probe's own lines go to stdout, and
# the `[STDOUT]:` prefix is the logger's, not ours.
java -cp "$CP:$WORK" AquiferProbe "$@" 2>/dev/null | sed 's/^.*\[STDOUT\]: //'
