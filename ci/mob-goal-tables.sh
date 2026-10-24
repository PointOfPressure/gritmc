#!/usr/bin/env bash
# mob-goal-tables.sh — every registerGoals() and attribute builder in the jar, as data.
#
# Runs ci/gen-mob-goal-tables.py against the 26.2 server jar (javap, JDK 25)
# and, with no argument, refuses when the result differs from the committed
# crates/rustmc-entity/golden/mob_goal_tables.json — the table is a
# transcription of the jar and the jar is the only thing allowed to change
# it. With --write the committed file is replaced.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/crates/rustmc-entity/golden/mob_goal_tables.json"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
JAR="$ROOT/oracle-work/vanilla-$VER/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
python3 "$ROOT/ci/gen-mob-goal-tables.py" "$JAR" "$WORK/tables.json"
if [ "${1:-}" = "--write" ]; then
    mv "$WORK/tables.json" "$OUT"
    echo "mob goal tables -> $OUT"
    exit 0
fi
if ! cmp -s "$WORK/tables.json" "$OUT"; then
    echo "mob goal tables differ from the jar's — regenerate with $0 --write" >&2
    diff -u "$OUT" "$WORK/tables.json" | head -40 >&2
    exit 1
fi
echo "mob goal tables match the jar"
