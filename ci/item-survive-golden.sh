#!/usr/bin/env bash
# item-survive-golden.sh — `canSurvive` for every block a BlockItem can place.
#
# See `tools/goldengen/vanilla/ItemSurviveGolden.java` for what is swept and
# why. The short version: `BlockItem.canPlace` gates every placement on
# `state.canSurvive(level, pos)`, `data/vanilla/survival.json`'s subjects come
# from the **datapack** and are 51 vegetation blocks, and the 1,054 blocks a
# `BlockItem` names are a different set that includes every torch, sign,
# banner, rail, door, ladder and carpet.
#
# Run twice and `cmp`ed, like `ci/item-props-golden.sh` and for the same
# reason: this is a registry walk with an imposed order over a world this
# dumper builds itself, so a difference between two runs is a defect in the
# dumper and not a fact about vanilla. `subjects()` sorts by registry name and
# `columns()` is a fixed product, so nothing here iterates a hash.
#
# Signature-stripped jar, as `ci/survival-golden.sh` does: the dumper lives
# inside vanilla's own package and the JVM refuses to mix an unsigned class
# into a signed one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/item_survive.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }

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
# Compiled with `ItemPropsGolden`, which holds both tag binders this dumper
# calls. Two copies of a binder is how the flat block binder came to be the
# wrong one to reach for, so there is one pair and both dumpers use it.
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/ItemPropsGolden.java" \
    "$ROOT/tools/goldengen/vanilla/ItemSurviveGolden.java" 2>&1 | grep -v '^Note' || true

# `Bootstrap.bootStrap()` redirects System.out through log4j, which re-emits
# every line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix. The dumper
# prints one JSON document, so the prefix is stripped back off here rather
# than the dumper writing to a path — which is what `ci/item-props-golden.sh`
# does instead and both are fine; this one keeps the Java side free of a
# command-line argument.
run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        ItemSurviveGolden 2>/dev/null \
        | sed -E 's/^\[[0-9:]+\] \[[^]]*\]: \[STDOUT\]: //' > "$1"
    test -s "$1"
    # The dumper's own first line, so a run that printed only log noise is a
    # failure here rather than an empty table two steps later.
    grep -q '"rng": "item-survive"' "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ItemSurviveGolden disagree — something in it iterates a map" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "item_survive -> $OUT (identical across two runs)"
