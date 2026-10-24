#!/usr/bin/env bash
# wall-reads-check.sh — how many positions `WallBlock.updateShape` reads,
# asked of the pinned jar rather than of a paragraph.
#
#     ./ci/wall-reads-check.sh          # refuse if the jar stopped agreeing
#     ./ci/wall-reads-check.sh --show   # print the disassembly this reads
#
# # WHY A SCRIPT AND NOT THE SENTENCE THAT WAS ALREADY WRITTEN
#
# `WallBlock` is 41,472 of `update_shape.json`'s 79,113 refusals — more than
# the other four large classes together — and the roadmap settled it from the
# bytecode because the box was not available for the 45-minute sweep. That
# reading is correct and it was done by a person reading `javap` once.
#
# A reading done once is a claim about a jar that nothing rechecks. The jar is
# pinned, so it will not drift on its own; it is **repinned** on every version
# bump, and on that day the sentence "`sideUpdate` reads exactly one position
# past the subject and the changed neighbour" becomes a thing somebody has to
# remember to re-derive. Nobody will, because it reads like prose and not like
# a figure. This is that sentence with a `diff` behind it.
#
# It is not a substitute for `ci/shape-three.sh --only WallBlock`. The sweep
# says what the pairs resolve *to*; this says how many positions the rule
# *reads*, which is the cheaper question and the one that scopes the extract.
# One second against forty-five minutes, so it can sit in front of the sweep.
#
# # WHAT IS ACTUALLY ASSERTED, AND WHY EACH ONE
#
#   1. `updateShape` dispatches three ways: `DOWN` to `Block.updateShape`,
#      `UP` to `topUpdate`, everything else to `sideUpdate`. If a fourth arm
#      appears, the horizontals are no longer the whole refusal set.
#   2. `topUpdate` reads **no** position. The block above IS the changed
#      neighbour on that direction, so it is handed in. One read here and
#      `UP` joins the refusals.
#   3. `sideUpdate` has exactly one `BlockPos.above` and exactly one
#      `getBlockState`. That single pair is the entire reason a THREE-position
#      extract answers this class; two of either makes it a four-position one.
#   4. No backward branch, no `Direction.values()`, no iterator on the path.
#      A call-site count only bounds the reads if nothing repeats them, and
#      that is the assumption a count silently makes. The class does contain
#      one iterator — in `lambda$makeShapes$0`, which builds voxel shapes —
#      so "no iterator in the class" would be false and is not what is checked.
#   5. The four horizontals are unrolled as four `getstatic` branches.
#
# # THE ARITHMETIC IS THE INDEPENDENT HALF
#
# Everything above is one reading of one disassembly. The count of wall states
# comes from a different artefact — this tree's own `block_props.json` — and
# 32 wall blocks x 324 states x 4 horizontal directions has to equal the
# refusal census's `WallBlock` entry to the pair. Two artefacts agreeing is
# what makes this evidence rather than a careful look.
#
# # THE THREE INPUTS ARE OVERRIDABLE, WHICH IS THE ONLY WAY THIS IS FALSIFIABLE
#
# A check whose input is a 24 MB signed jar cannot be shown to fail: there is
# no way to hand it a `WallBlock` that reads two positions. So the disassembly,
# the props table and the census are all overridable by environment variable,
# and `ci/wall-reads-falsify.sh` hands it mutated copies of each and asserts
# every arm reddens. Unset, it reads the pinned jar and this tree's tables, and
# an override is never a skip.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SHOW=no
[ "${1:-}" = --show ] && SHOW=yes

D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT

if [ -n "${RUSTMC_WALL_JAVAP:-}" ]; then
    [ -s "$RUSTMC_WALL_JAVAP" ] || {
        echo "wall-reads: RUSTMC_WALL_JAVAP=$RUSTMC_WALL_JAVAP is empty or absent" >&2
        exit 1; }
    cp -f "$RUSTMC_WALL_JAVAP" "$D/all.txt"
    JAR="$RUSTMC_WALL_JAVAP (RUSTMC_WALL_JAVAP override)"
else
    VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
    VDIR="$ROOT/oracle-work/vanilla-$VER"
    JAR="$VDIR/versions/$VER/server-$VER-unsigned.jar"
    [ -s "$JAR" ] || JAR="$VDIR/versions/$VER/server-$VER.jar"
    [ -s "$JAR" ] || { echo "wall-reads: no jar under $VDIR" >&2; exit 1; }
    javap -p -c -cp "$JAR" net.minecraft.world.level.block.WallBlock > "$D/all.txt" 2>&1 || {
        echo "wall-reads: javap failed on WallBlock" >&2; sed -n '1,5p' "$D/all.txt" >&2; exit 1; }
fi
[ "$SHOW" = yes ] && { cat "$D/all.txt"; exit 0; }

# One method's body, by name, from the signature line to the next one. Split on
# the *declaration* rather than on a blank line: javap puts blank lines inside a
# body (LineNumberTable, StackMapTable) and a blank-line split silently
# truncates a method to its first table, which reads as "no call sites here".
body() {
    awk -v want="$1" '
        /^  [a-zA-Z].*\(.*\);$/ { inm = (index($0, want) > 0) }
        inm { print }
    ' "$D/all.txt"
}

fail=0
say() { printf '  %-58s %s\n' "$1" "$2"; }
must() {  # must <label> <got> <want>
    if [ "$2" = "$3" ]; then say "$1" "$2"; else
        say "$1" "$2 (WANT $3)  <-- REFUSED"; fail=1
    fi
}

echo "wall-reads: $JAR"

# 1. the dispatch
body 'protected net.minecraft.world.level.block.state.BlockState updateShape' > "$D/us.txt"
must "updateShape -> Block.updateShape (the DOWN arm)" \
     "$(grep -c 'Method net/minecraft/world/level/block/Block.updateShape:' "$D/us.txt")" 1
must "updateShape -> topUpdate (the UP arm)" \
     "$(grep -c 'Method topUpdate:' "$D/us.txt")" 1
must "updateShape -> sideUpdate (everything else)" \
     "$(grep -c 'Method sideUpdate:' "$D/us.txt")" 1
must "updateShape reads no position itself" \
     "$(grep -cE 'getBlockState|Method net/minecraft/core/BlockPos\.' "$D/us.txt")" 0

# 2. topUpdate
body 'private net.minecraft.world.level.block.state.BlockState topUpdate' > "$D/top.txt"
must "topUpdate reads no position" \
     "$(grep -cE 'getBlockState|Method net/minecraft/core/BlockPos\.' "$D/top.txt")" 0

# 3+4+5. sideUpdate
body 'private net.minecraft.world.level.block.state.BlockState sideUpdate' > "$D/side.txt"
must "sideUpdate BlockPos.above call sites" \
     "$(grep -c 'Method net/minecraft/core/BlockPos.above:' "$D/side.txt")" 1
must "sideUpdate getBlockState call sites" \
     "$(grep -cE '(LevelReader|BlockGetter)\.getBlockState:' "$D/side.txt")" 1
must "sideUpdate other BlockPos navigation" \
     "$(grep -cE 'Method net/minecraft/core/BlockPos\.(below|relative|north|south|east|west|offset):' "$D/side.txt")" 0
must "sideUpdate horizontal getstatic branches" \
     "$(grep -cE 'Field net/minecraft/core/Direction\.(NORTH|EAST|SOUTH|WEST):' "$D/side.txt")" 4
must "sideUpdate Direction.values() (would defeat the count)" \
     "$(grep -c 'Direction.values' "$D/side.txt")" 0
must "sideUpdate iterators (would defeat the count)" \
     "$(grep -c 'iterator:' "$D/side.txt")" 0

# A forward `goto` is a ternary; a backward one is a loop, and a loop makes
# every call-site count above a lower bound instead of a total. Compare each
# goto's target against its own offset rather than counting gotos.
back="$(awk '/^ *[0-9]+: goto +[0-9]+/ {
        split($0, a, ":"); here = a[1] + 0;
        n = split($0, b, "goto"); tgt = b[n] + 0;
        if (tgt <= here) c++
    } END { print c + 0 }' "$D/side.txt")"
must "sideUpdate backward branches (a loop)" "$back" 0

# The whole chain: nothing else on it may read a position either.
for m in 'private boolean connectsTo' \
         'private net.minecraft.world.level.block.state.BlockState updateShape(net.minecraft.world.level.LevelReader' \
         'private boolean shouldRaisePost' \
         'private net.minecraft.world.level.block.state.BlockState updateSides'; do
    body "$m" > "$D/m.txt"
    short="$(printf '%s' "$m" | sed 's/.* //; s/(.*//')"
    must "$short reads no position" \
         "$(grep -cE '(LevelReader|BlockGetter)\.getBlockState:' "$D/m.txt")" 0
done

# The arithmetic, from a different artefact.
PROPS="${RUSTMC_WALL_PROPS:-$ROOT/data/vanilla/block_props.json}"
CENSUS="${RUSTMC_WALL_CENSUS:-$ROOT/ci/checks/shape-refusal-census.json}"
if [ -s "$PROPS" ] && [ -s "$CENSUS" ]; then
    python3 - "$PROPS" "$CENSUS" <<'PY' || fail=1
import collections, json, sys
props = json.load(open(sys.argv[1]))
census = json.load(open(sys.argv[2]))
c = collections.Counter(r["block"] for r in props["table"]
                        if r["block"].endswith("_wall"))
per = set(c.values())
states = sum(c.values())
horizontals = [d for d in props["directionOrder"] if d not in ("UP", "DOWN")]
pairs = states * len(horizontals)
want = {r["class"]: r["pairs"] for r in census["byClass"]}.get("WallBlock")
print(f"  {'block_props: wall blocks x states':<58} "
      f"{len(c)} x {sorted(per)} = {states}")
print(f"  {'x horizontal directions':<58} {len(horizontals)} -> {pairs}")
print(f"  {'refusal census WallBlock pairs':<58} {want}")
if len(per) != 1:
    print("  wall blocks do not all have the same state count  <-- REFUSED")
    sys.exit(1)
if pairs != want:
    print(f"  the arithmetic and the census disagree: {pairs} vs {want}"
          "  <-- REFUSED")
    sys.exit(1)
PY
else
    echo "  block_props.json or the refusal census is absent — the arithmetic half"
    echo "  was NOT checked. Run ci/gen-gameplay-tables.sh and ci/shape-reads.sh."
fi

if [ "$fail" != 0 ]; then
    echo "WALL-READS REFUSED — the jar no longer reads the way the roadmap's" >&2
    echo "  section 83.10 says it does. Every WallBlock figure in the three-position" >&2
    echo "  extract is scoped by that reading; re-derive it before trusting them." >&2
    exit 1
fi
echo "WALL-READS OK — updateShape reads exactly one position past the subject and"
echo "  the changed neighbour, on the four horizontals only, with no loop."
