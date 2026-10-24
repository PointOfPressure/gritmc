#!/usr/bin/env bash
# village-oracle.sh — gate the village jigsaw walk against vanilla's own saved
# `Children[]`, piece for piece. No JVM and no generated world on either side:
# vanilla's answer is already in the region files it wrote.
#
# # The first invocation is the one that matters, and it is not the obvious one
#
# The r=16 run is FIRST because it is the only one that exercises the search
# reach. Its window is chunks 48..80 and both `village_savanna` starts —
# (46,77) and (81,77) — sit **outside** it, so a village is only found because
# `village_starts` searches `reach` chunks past the area. The two focused runs
# below put the start inside their own window, so they would pass with the
# reach set to zero.
#
# That is not hypothetical: a falsification sweep set `reach = 0` and both
# focused oracles stayed green, while the r=16 world lost both villages and
# 13,069 blocks. AGENT-BRIEF — *"a gate a window cannot exercise proves
# nothing"*. If you add a case, ask which of its decisions the window can
# actually reach.
#
# # What it compares, and the field that was nearly missed
#
# Element **type**, element **id**, rotation, position, ground level delta and
# bounding box, index for index. A `feature_pool_element` has no `location`,
# so its identity is `pool_element.feature` and not `pool_element.location`;
# comparing the wrong one scored 84/98 on a walk that was exact. Both are
# compared now, each about the kind vanilla writes it for.
#
# Roughly 90 s, almost all of it the r=16 noise passes that supply
# `getFirstFreeHeight`. Runs where `oracle-work` is — the Threadripper.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

if [ ! -d "$ROOT/oracle-work/gen-wide-20260722-r16-ow-a" ]; then
    echo "skipping: no vanilla wide reference"
    exit 0
fi

cargo build --release -q -p worldgen-dump || exit 2
DUMP=./target/release/worldgen-dump

# reference                            seed      r  cx cz   expected
CASES="
gen-wide-20260722-r16-ow-a|20260722|16|64|64|2/2 starts, 171/171 pieces
van-village-c4576|20260722|7|45|76|1/1 starts, 98/98 pieces
van-village-c8076|20260722|7|80|76|1/1 starts, 73/73 pieces
van-village-31415926|31415926|7|8|17|1/1 starts, 181/181 pieces
"

rc=0
echo "$CASES" | while IFS='|' read -r ref seed r cx cz want; do
    [ -z "$ref" ] && continue
    [ -d "$ROOT/oracle-work/$ref" ] || { echo "  skip $ref (absent)"; continue; }
    got=$("$DUMP" village-oracle "$seed" "oracle-work/$ref" "$r" "$cx" "$cz" 2>&1 | tail -1)
    # The expected string is compared in full, including the denominator. A
    # match on the numerator alone would let 0/0 pass as a result, which is
    # the vacuous verdict this project has been bitten by.
    case "$got" in
        *"$want") echo "  OK   $ref — $want" ;;
        *) echo "  FAIL $ref"; echo "       want: ...$want"; echo "       got:  $got"; exit 1 ;;
    esac
done
rc=$?
[ "$rc" = 0 ] && echo "VILLAGE ORACLE GREEN"
exit "$rc"
