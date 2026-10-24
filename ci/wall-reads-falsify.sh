#!/usr/bin/env bash
# wall-reads-falsify.sh — show every arm of ci/wall-reads-check.sh failing.
#
#     ./ci/wall-reads-falsify.sh
#
# # WHY
#
# `ci/wall-reads-check.sh` asks a pinned 24 MB jar how many positions
# `WallBlock.updateShape` reads, and a green run of it is indistinguishable
# from a run that greps nothing and prints OK. That is this repo's own named
# defect class and the reason `ci/probe-absences-falsify.py` exists next to
# `ci/probe-absences.py`.
#
# Each arm below hands the checker a **mutated disassembly** — the real one,
# with one edit — and refuses unless the checker reddens on it and names the
# thing that changed. The mutations are the failures that would actually
# matter, not typos:
#
#   * a second `getBlockState` in `sideUpdate` — the rule grew a fourth
#     position and every `WallBlock` row in the extract is now wrong;
#   * a `BlockPos.below` — a second position, reached a different way, which
#     a check counting only `above` would sail past;
#   * a read in `topUpdate` — `UP` joins the refusal set and 10,368 pairs
#     move class;
#   * a backward `goto` in `sideUpdate` — the call-site counts stop being
#     totals and become lower bounds, silently;
#   * `Direction.values()` and an iterator — the same defeat by a different
#     door, which is why both are asserted rather than one;
#   * a fourth dispatch arm in `updateShape`;
#   * a read in `connectsTo`, which is on the chain and is easy to forget;
#   * the arithmetic half: a props table whose walls have a different state
#     count, and a census whose `WallBlock` entry disagrees with it. That arm
#     is the one that catches a repin where the bytecode did not change and
#     the block list did.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECK="$ROOT/ci/wall-reads-check.sh"
D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT

REAL="$D/real.txt"
"$CHECK" --show > "$REAL" || { echo "wall-reads-falsify: could not disassemble" >&2; exit 1; }
[ -s "$REAL" ] || { echo "wall-reads-falsify: empty disassembly" >&2; exit 1; }

ok=1
arms=0

# arm <name> <want-rc> <expected-substring> <sed-program-over-the-disassembly>
arm() {
    local name="$1" want="$2" expect="$3" prog="$4"
    arms=$((arms + 1))
    local f="$D/$name.txt"
    sed "$prog" "$REAL" > "$f"
    if cmp -s "$REAL" "$f"; then
        printf '  %-26s NO-OP — the mutation changed nothing, so this arm proves nothing\n' "$name"
        ok=0; return
    fi
    local out rc
    out="$(RUSTMC_WALL_JAVAP="$f" "$CHECK" 2>&1)"; rc=$?
    if [ "$rc" != "$want" ]; then
        printf '  %-26s rc=%s (want %s)  <-- ARM DID NOT FIRE\n' "$name" "$rc" "$want"
        ok=0; return
    fi
    if ! printf '%s' "$out" | grep -cF "$expect" >/dev/null; then
        printf '  %-26s rc=%s but did not say %s  <-- WRONG REASON\n' "$name" "$rc" "$expect"
        ok=0; return
    fi
    printf '  %-26s rc=%s, refused on: %s\n' "$name" "$rc" "$expect"
}

echo "wall-reads-falsify: mutating the real disassembly, one edit per arm"

# The real disassembly, unmutated, must pass — otherwise every arm below is
# firing on a checker that was already red and none of them mean anything.
if out="$(RUSTMC_WALL_JAVAP="$REAL" "$CHECK" 2>&1)"; then
    printf '  %-26s rc=0, the unmutated control passes\n' "control"
else
    printf '  %-26s the UNMUTATED disassembly is refused — every arm below is void\n' "control"
    printf '%s\n' "$out" | sed 's/^/      /'
    ok=0
fi
arms=$((arms + 1))

# sideUpdate's single getBlockState, doubled. The line is inside sideUpdate
# because that is the only method on the update path that has one; the five in
# getStateForPlacement are not matched, they sit under a different signature
# and the checker splits on the declaration line.
arm side-two-reads 1 "sideUpdate getBlockState call sites" \
    '/LevelReader.getBlockState:/{ p; }'

arm side-blockpos-below 1 "sideUpdate other BlockPos navigation" \
    's|Method net/minecraft/core/BlockPos.above:|Method net/minecraft/core/BlockPos.below:|'

arm side-values 1 "sideUpdate Direction.values()" \
    's|Field net/minecraft/core/Direction.NORTH:|Method net/minecraft/core/Direction.values:|'

arm side-iterator 1 "sideUpdate iterators" \
    's|Method net/minecraft/world/level/block/state/BlockState.isFaceSturdy:|InterfaceMethod java/util/Set.iterator:|'

# A goto whose target is at or before its own offset. `0: goto 0` is the
# smallest such edit and it is a loop by the checker's definition, which is
# the definition that matters.
arm side-backward-goto 1 "sideUpdate backward branches" \
    's|^\( *\)160: invokevirtual \(.*\)BlockPos.above.*$|\1160: goto          4|'

arm top-reads-a-position 1 "topUpdate reads no position" \
    '/private net.minecraft.world.level.block.state.BlockState topUpdate/,/private net.minecraft.world.level.block.state.BlockState sideUpdate/ s|Method isConnected:|InterfaceMethod net/minecraft/world/level/LevelReader.getBlockState:|'

arm dispatch-arm-lost 1 "updateShape -> topUpdate (the UP arm)" \
    's|Method topUpdate:|Method topUpdateXX:|'

# `connectsTo` sits between its own declaration and `getStateForPlacement`,
# which is the next one javap prints. The mutation turns its `BlockState.is`
# tag lookup into a world read: the smallest edit that puts a position on a
# method the roadmap asserts has none.
arm connectsto-reads 1 "connectsTo reads no position" \
    '/private boolean connectsTo/,/public net.minecraft.world.level.block.state.BlockState getStateForPlacement/ s|Method net/minecraft/world/level/block/state/BlockState.is:|InterfaceMethod net/minecraft/world/level/BlockGetter.getBlockState:|'

# The arithmetic half, falsified against fixtures rather than the disassembly.
props="$D/props.json"; census="$D/census.json"
python3 - "$ROOT" "$props" "$census" <<'PY'
import json, sys
root, pout, cout = sys.argv[1], sys.argv[2], sys.argv[3]
props = json.load(open(f"{root}/data/vanilla/block_props.json"))
census = json.load(open(f"{root}/ci/checks/shape-refusal-census.json"))
# One wall block given one extra state: the arithmetic no longer lands on the
# census's count, and the per-block state counts are no longer uniform.
rows = props["table"]
extra = next(r for r in rows if r["block"].endswith("_wall")).copy()
extra["state"] = extra["state"] + "|falsify"
json.dump({**props, "table": rows + [extra]}, open(pout, "w"))
by = [dict(r) for r in census["byClass"]]
for r in by:
    if r["class"] == "WallBlock":
        r["pairs"] += 4
json.dump({**census, "byClass": by}, open(cout, "w"))
PY

arms=$((arms + 1))
if out="$(RUSTMC_WALL_PROPS="$props" "$CHECK" 2>&1)"; then
    printf '  %-26s rc=0  <-- ARM DID NOT FIRE\n' "props-state-count"; ok=0
else
    printf '  %-26s rc=1, refused on: %s\n' "props-state-count" \
        "$(printf '%s' "$out" | grep -oE 'wall blocks do not all have the same state count|the arithmetic and the census disagree[^<]*' | head -1)"
fi

arms=$((arms + 1))
if out="$(RUSTMC_WALL_CENSUS="$census" "$CHECK" 2>&1)"; then
    printf '  %-26s rc=0  <-- ARM DID NOT FIRE\n' "census-disagrees"; ok=0
else
    printf '  %-26s rc=1, refused on: %s\n' "census-disagrees" \
        "$(printf '%s' "$out" | grep -oE 'the arithmetic and the census disagree[^<]*' | head -1)"
fi

# The falsifier counts its own arms rather than carrying the number in a
# comment: `ci/t0.sh` said "the nine refusals" long after there were nine, and
# a constant that has to be edited alongside an arm is a constant that will not
# be. Same reason `ci/refusal-ledger-falsify.py` counts itself.
echo
if [ "$ok" = 1 ]; then
    echo "WALL-READS-FALSIFY OK — $arms arm(s), every one of them red on a mutation"
    exit 0
fi
echo "WALL-READS-FALSIFY FAILED — an arm above did not fire, so ci/wall-reads-check.sh" >&2
echo "  is not known to be able to refuse the thing it exists to refuse." >&2
exit 1
