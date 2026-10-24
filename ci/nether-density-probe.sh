#!/usr/bin/env bash
# nether-density-probe.sh — ask vanilla's own nether router for a density column.
#
# The counterpart to `worldgen-dump nether-density`: that prints our numbers,
# this prints vanilla's, for the same column at the same seed. Diff the two and
# the answer is a fact rather than a reading.
#
#   ci/nether-density-probe.sh 20260722 915 1028 55 70
#   ci/nether-density-probe.sh 20260722 915 1028 48 72 --corners
#
# `--corners` keeps only the rows on the 8-block cell lattice (min_y 0), which
# are the values the interpolator actually samples.
#
# # What it is for
#
# The nether's residual is order-invariant and lives in the noise stage — see
# the nether section of `docs/WORKLIST.md`. A region file cannot take it
# further, because it shows the block and the question is about the number:
# "crosses zero at the wrong y", "wrong sign" and "offset" are three different
# bugs with the same block output.
#
# Evaluating through a `SinglePointContext` deliberately bypasses the
# `Interpolated` marker, so this prints the **un-interpolated** density. If the
# two columns agree, the base noise, slide and squeeze are all right and the
# defect is the interpolation lattice; if they differ, the lattice is innocent.
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

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/NetherDensityProbe.java" 2>&1 |
    grep -v '^Note' || true

# Vanilla's bootstrap logs to stderr; the probe's own lines go to stdout, and
# the `[STDOUT]:` prefix is the logger's, not ours.
java -cp "$CP:$WORK" NetherDensityProbe "$@" 2>/dev/null | sed 's/^.*\[STDOUT\]: //'
