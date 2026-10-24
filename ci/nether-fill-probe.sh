#!/usr/bin/env bash
# nether-fill-probe.sh — run vanilla's own nether noise fill and print a column.
#
# The counterpart to `worldgen-dump full-nether` under `RUSTMC_THROUGH=noise`:
# that writes our noise-stage world, this prints vanilla's, for the same column
# at the same seed. Diff the two and the answer is a fact rather than a reading.
#
#   ci/nether-fill-probe.sh 20260722 915 1028 50 75
#
# # What it settles that `ci/nether-density-probe.sh` cannot
#
# The density probe proved our nether `finalDensity` bit-for-bit identical to
# vanilla's, including at all eight lattice corners of the disputed cell. It
# evaluates through a single-point context, which bypasses the `Interpolated`
# marker, so it prints the value the cell lattice is built FROM and never the
# trilinear blend the filler writes blocks by. An exact density can still make
# a different world if the lattice, its origin, or the blend differ.
#
# This runs vanilla's real fill loop over a real `NoiseChunk` and prints the
# block. Run it BEFORE porting any nether feature: if the fill also disagrees,
# feature work would bury a terrain bug and the number would improve for the
# wrong reason.
#
# Like `ci/aquifer-probe.sh` this writes no vector file. It is a probe, not a
# gate: the columns worth asking about are the ones a residual points at, and
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

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/NetherFillProbe.java" 2>&1 |
    grep -v '^Note' || true

# Vanilla's bootstrap logs to stderr; the probe's own lines go to stdout, and
# the `[STDOUT]:` prefix is the logger's, not ours.
java -cp "$CP:$WORK" NetherFillProbe "$@" 2>/dev/null | sed 's/^.*\[STDOUT\]: //'
