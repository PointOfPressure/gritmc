#!/usr/bin/env bash
# village-pieces.sh — gate the jigsaw assembler against vanilla's own piece list.
#
# `JigsawPlacement.addPieces` has no draw-level harness: `DecorateGolden` runs
# features, and a structure is not a feature. The oracle is instead the piece
# list vanilla saved into its own chunk NBT, committed as
# `crates/rustmc-worldgen/golden/village_31415926.json` by
#
#     worldgen-dump jigsaw-starts oracle-work/van-village-31415926 \
#         village_savanna --json
#
# This runs the assembler against that vector and compares **index for index**
# (standing rule 10): element, rotation, position, bounding box, ground level
# delta and junction count, for all 181 pieces. It exits non-zero on the first
# divergence and prints both sides of it.
#
# It needs the datagen extract and the structure templates, so it runs where
# `oracle-work` is — the Threadripper. It no longer generates a world at all:
# the height source is `getFirstFreeHeight`, a pure noise column, and reading
# the canvas `WORLD_SURFACE_WG` instead was a real defect — it built a
# 103-piece village at seed 20260722 chunk (46,77) where vanilla builds 98.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# All three committed vectors by default; the env vars still override for a
# one-off. The two 20260722 ones are the villages this port used to drop
# entirely, and they are here rather than only in `ci/village-oracle.sh`
# because a vector in the tree outlives the 11 MB region reference it came
# from — `oracle-work` is gitignored and lives on one machine.
CASES="${VILLAGE_CASES:-}"
if [ -z "$CASES" ]; then
    CASES="31415926|8|17|crates/rustmc-worldgen/golden/village_31415926.json
20260722|46|77|crates/rustmc-worldgen/golden/village_savanna_20260722_c4677.json
20260722|81|77|crates/rustmc-worldgen/golden/village_savanna_20260722_c8177.json"
fi
STRUCTURE="${VILLAGE_STRUCTURE:-village_savanna}"

if [ ! -d "$ROOT/oracle-work/vanilla-26.2/generated-data/data/minecraft/structure" ]; then
    echo "skipping: no vanilla datagen extract"
    exit 0
fi

cargo build --release -q -p worldgen-dump || exit 2

# Single-case override keeps the old shape exactly.
if [ -n "${VILLAGE_SEED:-}" ]; then
    CASES="${VILLAGE_SEED}|${VILLAGE_CHUNK_X:-8}|${VILLAGE_CHUNK_Z:-17}|${VILLAGE_VECTOR:-crates/rustmc-worldgen/golden/village_31415926.json}"
fi

rc=0
while IFS='|' read -r seed cx cz vector; do
    [ -z "$seed" ] && continue
    echo "== seed=$seed chunk ($cx,$cz)  $(basename "$vector")"
    # NEVER pipe the gate through anything that swallows its status — a
    # pipeline exits with its LAST command, which has masked a failure twice
    # in this repo. `PIPESTATUS[0]` is the tool's own.
    ./target/release/worldgen-dump village-pieces "$seed" "$cx" "$cz" "$STRUCTURE" "$vector" \
        | grep -E '^(PIECES|MATCH|DIVERGE|MISSING|  vanilla|  ours)'
    st="${PIPESTATUS[0]}"
    [ "$st" = 0 ] || rc="$st"
done <<EOF
$CASES
EOF
[ "$rc" = 0 ] && echo "VILLAGE PIECES GREEN"
exit "$rc"
