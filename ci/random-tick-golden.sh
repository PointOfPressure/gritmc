#!/usr/bin/env bash
# random-tick-golden.sh — dump vanilla's own `Level.getBlockRandomPos`.
#
#     ./ci/random-tick-golden.sh [out.json]
#
# The position function random ticks choose blocks with, run by vanilla and
# recorded, so that `crates/rustmc-server/src/randomtick.rs` is compared
# against the method rather than against a reading of the method. See the
# header of `tools/goldengen/vanilla/RandomTickGolden.java` for which nine
# instructions this is and which two of them a plausible transcription gets
# backwards.
#
# **The output is a golden vector and is committed**, unlike the tables
# `ci/gen-gameplay-tables.sh` regenerates per tree. That is deliberate and it
# is the difference between a fixture and an extract: the extract is the
# server's own input, so a stale one silently changes what the server does,
# whereas this file is only ever *compared against* — a stale one makes a test
# red and nothing else. Regenerate it when the pinned version moves, and
# expect the diff to be the whole file when it does.
#
# Same signature-stripping as `ci/block-props-golden.sh`, for the same reason:
# the dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/random_tick_pos.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/RandomTickGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    # No `--add-exports` for `sun.misc`: it lives in `jdk.unsupported`, which
    # exports it to the unnamed module already. Naming `java.base/sun.misc`
    # instead makes the JVM print `package sun.misc not in java.base` on
    # stderr and carry on, which is a warning that would be swallowed by the
    # redirect below and look like nothing at all.
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.RandomTickGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree — the standing rule for every golden in this
# tree. It bites differently here from everywhere else it is written: this
# dumper's whole subject is a generator, so a run that seeded itself from
# anything ambient would produce a file that churns, and a churning golden
# hides a real regression inside its own noise. The seeds are constants in
# the dumper precisely so that this comparison is a real one.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of RandomTickGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "random tick positions -> $OUT (identical across two runs)"
python3 - "$OUT" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
cases = doc["cases"]
print(f"  {len(cases)} cases x {doc['draws']} draws")
PY
