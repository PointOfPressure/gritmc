#!/usr/bin/env bash
# recipe-match-golden.sh — vanilla's own answer to "what does this grid craft".
#
# Same signature-stripping and classpath construction as the other golden
# runners, and for the same two reasons their headers give: the JVM refuses to
# mix an unsigned class into a signed jar, and the libraries have to come out of
# the **outer bundler jar** rather than `oracle-work/vanilla-$VER/libraries`,
# which the vanilla launcher does not populate until its first real start.
#
# The dumper writes to a file, never to stdout: `Bootstrap.bootStrap()`
# redirects `System.out` through log4j and a JSON document written there comes
# back with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix on every line.
#
# Never pipe the java invocation into `tail` — `$?` would then be tail's.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/recipe_match.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="${RUSTMC_VANILLA_DIR:-$ROOT/oracle-work/vanilla-$VER}"
BUNDLER="$VDIR/server.jar"
JAR="$VDIR/versions/$VER/server-$VER.jar"

if [ ! -s "$JAR" ]; then
    [ -s "$BUNDLER" ] || { echo "no jar at $BUNDLER — run ci/vanilla-server.sh fetch" >&2; exit 1; }
    mkdir -p "$(dirname "$JAR")"
    unzip -o -q "$BUNDLER" "META-INF/versions/$VER/server-$VER.jar" -d "$VDIR/.unbundle"
    cp -f "$VDIR/.unbundle/META-INF/versions/$VER/server-$VER.jar" "$JAR"
fi

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

LIBS="$VDIR/.libs"
if [ ! -d "$LIBS" ] || [ -z "$(find "$LIBS" -name '*.jar' -print -quit)" ]; then
    mkdir -p "$LIBS"
    unzip -o -q "$BUNDLER" 'META-INF/libraries/*' -d "$LIBS"
fi
LIBCOUNT="$(find "$LIBS" -name '*.jar' | wc -l)"
[ "$LIBCOUNT" -ge 10 ] || { echo "only $LIBCOUNT library jars unpacked from $BUNDLER — refusing to build a classpath that is silently short" >&2; exit 1; }

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/RecipeMatchGolden.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/world/item/crafting/RecipeMatchGolden.class" ] || { echo "RecipeMatchGolden did not compile" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"
status=0
java -Drustmc.version="$VER" \
     -Drustmc.jar="$UNSIGNED" \
     -Drustmc.recipematch.out="$OUT" \
     -cp "$CP:$WORK" \
     net.minecraft.world.item.crafting.RecipeMatchGolden || status=$?
[ "$status" -eq 0 ] || { echo "RecipeMatchGolden exited $status" >&2; exit "$status"; }
[ -s "$OUT" ] || { echo "RecipeMatchGolden wrote nothing to $OUT" >&2; exit 1; }

# REFUSALS. Each of these is a shape this dumper has actually been capable of
# producing, and each would read as a pass rather than as a broken instrument.
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
cases = d["cases"]
pos = [c for c in cases if not c["negative"] and "last_recipe" not in c]
neg = [c for c in cases if c["negative"]]
lastp = [c for c in cases if "last_recipe" in c]
hit = [c for c in pos if c["winner"] is not None]
print(f"recipe-match: {d['decoded']} recipes decoded, {d['tags_bound']} tags bound, "
      f"{len(cases)} cases, {len(d['unplaceable'])} not placeable")
print(f"recipe-match: {len(hit)} of {len(pos)} positive cases have a winner; "
      f"{sum(1 for c in neg if c['winner'] is None)} of {len(neg)} negatives have none")
if len(pos) < 1000:
    sys.exit(f"recipe-match: only {len(pos)} positive cases — the placement walk is not reaching "
             "the table; instrument failure, not a finding")
if len(hit) < len(pos) * 9 // 10:
    sys.exit(f"recipe-match: only {len(hit)} of {len(pos)} positive cases matched anything. "
             "That is the shape this dumper produces when the item tags are not bound, and it "
             "is not a result")
selfwin = sum(1 for c in hit if c["winner"] == c["recipe"])
if selfwin == len(hit):
    sys.exit("recipe-match: every positive case was won by its own recipe. The tie-break this "
             "table exists to gate is then untested — expect a few dozen cases where a "
             "lower-sorting recipe takes the grid, and suspect the probe if there are none")
print(f"recipe-match: {len(hit) - selfwin} of {len(hit)} grids are won by a DIFFERENT recipe "
      "than the one they were built from — those are the tie-break vectors")
# The `lastRecipe` branch is a BEHAVIOUR, not a cache: on every one of those
# grids, passing the built-from recipe as `lastRecipe` must change the winner.
# If it changes none of them the branch was not exercised and the vectors are
# a second copy of the plain ones.
flipped = sum(1 for c in lastp if c["winner"] == c["last_recipe"])
if lastp and flipped != len(lastp):
    sys.exit(f"recipe-match: the lastRecipe branch changed the winner on only {flipped} of "
             f"{len(lastp)} grids where a different recipe wins without it; it should change "
             "all of them, so the probe is not passing the holder it thinks it is")
print(f"recipe-match: {len(lastp)} lastRecipe vectors, all of which the branch flips")
PY
