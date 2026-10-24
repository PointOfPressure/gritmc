#!/usr/bin/env bash
# fluid-rules-golden.sh - dump the three `FlowingFluid` predicates that are
# about a block's kind, not about arithmetic.
#
#     ./ci/fluid-rules-golden.sh [out.json]
#
# Produces `data/vanilla/fluid_rules.json`:
#
#   canHoldAnyFluid  one bit per block state
#   canHoldWater     `canHoldSpecificFluid(..., Fluids.WATER)`, one bit per state
#   canHoldLava      the same for lava
#   stateGroup       per state, the index of its collision shape by IDENTITY,
#                    or -1 for a block with a dynamic shape
#   canPassThroughWall  per direction, one bit per ordered pair of groups
#
# All three are called **by reflection on `FlowingFluid`'s own private
# statics**, so this file records vanilla's answer rather than a reading of
# it. See `tools/goldengen/vanilla/FluidRulesGolden.java` for why the third
# one is a pair table over shape identities and not a computation.
#
# **This is a runtime extract, not a fixture.** It goes under `data/vanilla/`,
# it is gitignored, it is regenerated per tree by `ci/gen-gameplay-tables.sh`
# and stamped by `ci/provenance.sh`.
#
# Same signature-stripping as `ci/block-props-golden.sh` and the same
# unanchored-grep warning: the dumper lives in vanilla's own package, the JVM
# refuses to mix an unsigned class into a signed one, and `Bootstrap.bootStrap()`
# re-emits stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/fluid_rules.json}"

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
# `SurfaceChunkGolden` carries `bindBlockTags`/`bindFluidTags`, which
# `canHoldAnyFluid` needs: its `BlockTags.SIGNS` test throws on an unbound tag
# rather than answering false, so an unbound run would refuse every sign
# instead of quietly mis-answering. Compiled alongside for the same reason
# `ci/shape-golden.sh` does it.
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/FluidRulesGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.FluidRulesGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. The equivalence sweep draws part of its probe
# set from a `java.util.Random` with a constant seed; if that ever became
# ambient, the `mismatches` count would be sampling rather than measuring and
# a run that happened to miss a bad shape would read exactly like a clean one.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of FluidRulesGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'CHECK'
import json, sys

d = json.load(open(sys.argv[1]))
n = d["states"]
gn = d["shapeGroups"]
print(f"fluid rules -> {sys.argv[1]} (identical across two runs)")
print(f"  {n} states -> {gn} collision-shape groups by identity"
      f" ({d['dynamicStates']} dynamic, group -1)")


def bits(key):
    out = []
    for ch in d[key]:
        v = int(ch, 16)
        for b in (3, 2, 1, 0):
            out.append((v >> b) & 1)
    return out[:n]


hold = bits("canHoldAnyFluid")
water = bits("canHoldWater")
flowing = bits("canHoldFlowingWater")
lava = bits("canHoldLava")
print(f"  canHoldAnyFluid {sum(hold)}, canHoldWater {sum(water)},"
      f" canHoldFlowingWater {sum(flowing)}, canHoldLava {sum(lava)}")
print(f"  canHoldSpecificFluid refused on {d['holdRefused']} states"
      f" {d['holdRefusedBy'] if d['holdRefusedBy'] else ''}")
print(f"  representative cross-checks: {d['crossChecks']:,}")

fail = []
# The whole table is indexed by state id; a short one silently answers about
# the wrong block.
if len(d["stateGroup"]) != n:
    fail.append(f"{len(d['stateGroup'])} state groups for {n} states")
# A grouping that put every state in its own bucket would make the pair table
# quadratic in the state count and would mean the identity assumption is
# wrong. A grouping with one bucket would make every answer the same.
if gn < 10 or gn > n // 8:
    fail.append(f"{gn} shape groups out of {n} states - the identity grouping "
                f"is not doing what this table assumes")
# A pair table of the wrong width answers about the wrong pair, in silence.
for name, hexed in d["canPassThroughWall"].items():
    want = (gn * gn + 3) // 4
    if len(hexed) != want:
        fail.append(f"{name}: {len(hexed)} nibbles for {gn}x{gn} pairs, want {want}")
# **The predicates must separate.** If `canHoldAnyFluid` were all-true or
# all-false the port would flow everywhere or nowhere and every fluid test
# would still pass against it, which is this project's central defect class.
if not 0 < sum(hold) < n:
    fail.append(f"canHoldAnyFluid is constant at {sum(hold)}/{n}")
if not 0 < sum(water) < n:
    fail.append(f"canHoldWater is constant at {sum(water)}/{n}")
# Water and lava must disagree somewhere: `SimpleWaterloggedBlock` takes
# water and refuses lava, and a table where they agree everywhere has lost
# the fluid argument.
if water == lava:
    fail.append("canHoldWater and canHoldLava agree on every state - the "
                "fluid argument is not reaching canPlaceLiquid")
# **The source and the flowing form must disagree too.**
# `SimpleWaterloggedBlock.canPlaceLiquid` is `fluid == Fluids.WATER` by
# identity, so every waterloggable block takes a source and refuses a
# flowing state. If these agreed, flowing water would waterlog fences.
if water == flowing:
    fail.append("canHoldWater and canHoldFlowingWater agree on every state - "
                "the source/flowing distinction is not reaching canPlaceLiquid")
# The wall table must separate too, or it is a constant wearing a lookup's
# clothes.
for name, hexed in d["canPassThroughWall"].items():
    v = "".join(f"{int(c, 16):04b}" for c in hexed)[: gn * gn]
    if v.count("1") in (0, len(v)):
        fail.append(f"canPassThroughWall[{name}] is constant")
# The cross-check is the grouping's own proof. Too few means groups had one
# state each, so nothing was checked and the table's warrant is missing.
if d["crossChecks"] < 1000:
    fail.append(f"only {d['crossChecks']} representative cross-checks - the "
                f"grouping is not being falsified")
# Half the pair table is derived by transposing the other half. The sweep
# that recomputes a sample of it from vanilla is that derivation's only
# warrant, and a zero here means the table is half assertion.
if d["transposeChecks"] < 1000:
    fail.append(f"only {d['transposeChecks']} transpose checks - half the pair "
                f"table is derived and unverified")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
CHECK
