#!/usr/bin/env bash
# piston-golden.sh — regenerate the piston push table from the pinned jar.
#
# `PistonBaseBlock.isPushable` reads three per-state facts nothing else in
# `data/vanilla` carries: `getPistonPushReaction()`, whether the block is
# unbreakable (`getDestroySpeed == -1.0F`) and `hasBlockEntity()`. Without
# them a port has to default the reaction, and defaulting it to NORMAL is a
# working-looking piston that is silently wrong for exactly the blocks that
# make a piston interesting.
#
# Same signature-stripping as `ci/block-props-golden.sh` and for the same
# reason: the dumper lives inside vanilla's own package and the JVM refuses to
# mix an unsigned class into a signed one.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# System.out through log4j, which re-emits it with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so `grep -E "^SOMETHING"`
# matches nothing and exits 1 — indistinguishable from a clean run that found
# nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/piston_push.json}"

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
    "$ROOT/tools/goldengen/vanilla/PistonGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.PistonGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree — the same guard `ci/block-props-golden.sh`
# carries, for the same reason: an unstable iteration order does not announce
# itself, it just churns the file until a real regression hides inside the
# churn.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of PistonGolden disagree — something in it iterates a map" >&2
    exit 1
}

# A dump that found no DESTROY and no BLOCK block is a dump that ran against
# a broken classpath and answered the default for everything. Refuse it: an
# all-NORMAL table is precisely the silently-wrong artefact this script exists
# to prevent, and it would gate green.
for want in DESTROY BLOCK PUSH_ONLY; do
    grep -q "\"$want\"" "$WORK/out.json" || {
        echo "piston_push.json contains no $want reaction — the dump is not measuring anything" >&2
        exit 1
    }
done

# It must parse. `Bootstrap.bootStrap()` reroutes stderr through log4j onto
# **stdout**, so anything the dumper writes to either stream ends up in this
# file; a stray line is trailing garbage that only a parser notices. Checked
# here rather than trusted, because the two-run comparison above passes
# happily on two identically-corrupt files.
python3 -c "import json,sys; d=json.load(open(sys.argv[1])); \
    print('piston push: %d blocks, %d split states' % (len(d['blocks']), len(d['states'])))" \
    "$WORK/out.json" || {
    echo "piston_push.json does not parse — something printed outside the JSON" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "piston push -> $OUT (identical across two runs)"
