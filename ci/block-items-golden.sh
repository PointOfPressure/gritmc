#!/usr/bin/env bash
# block-items-golden.sh — which block each item places, off the pinned jar.
#
# Exists because `ci/corpus-surface.py` was comparing block ids against item
# ids with `==`. See `tools/goldengen/vanilla/BlockItemGolden.java` for the
# measured cost; the short version is that the corpus holds `minecraft:redstone`
# and `minecraft:redstone_wire` scored uncovered anyway.
#
# Run twice and `cmp`ed, like the other use tables: this walk is a registry
# iteration and has no business varying between runs, so a difference is a
# defect in the dumper rather than noise to be averaged away.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/block_items.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/BlockItemGolden.java" \
    2>&1 | grep -v '^Note' || true

run() {
    java -cp "$CP:$WORK" BlockItemGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
}
run "$WORK/a.txt"
run "$WORK/b.txt"
cmp -s "$WORK/a.txt" "$WORK/b.txt" || { echo "REFUSED  two runs of the registry walk differ" >&2; exit 3; }

python3 - "$WORK/a.txt" "$OUT" <<'PY'
import json, sys

rows, count = {}, None
for line in open(sys.argv[1]):
    parts = line.rstrip("\n").split("\t")
    if parts[0] == "ROW":
        rows[parts[1]] = parts[2]
    elif parts[0] == "COUNT":
        count = (int(parts[1]), int(parts[2]))

# **Refuse an empty or implausible walk rather than write it.** A dumper that
# failed to bootstrap prints no ROW lines, and an empty map is not "no block
# items" — it is a table that would silently make every block uncovered and
# read as a corpus regression nobody could attribute.
if count is None:
    raise SystemExit("REFUSED  no COUNT line; the dumper did not finish")
if not rows:
    raise SystemExit("REFUSED  the registry walk produced no block items")
if len(rows) != count[1]:
    raise SystemExit(f"REFUSED  {len(rows)} rows against a reported {count[1]}")

# The interesting half, and the reason this file is not a one-liner: the items
# whose id differs from the block they place. Written into the table so a
# reader can see the size of the confusion without recomputing it.
differing = {i: b for i, b in rows.items() if i != b}

json.dump(
    {
        "what": "BlockItem.getBlock(), per item",
        "itemsInRegistry": count[0],
        "itemsPlacingABlock": count[1],
        "itemsWhoseIdDiffersFromItsBlock": len(differing),
        "item_to_block": dict(sorted(rows.items())),
        "differing": dict(sorted(differing.items())),
    },
    open(sys.argv[2], "w"),
    indent=1,
)
print(
    f"block items -> {sys.argv[2]} "
    f"({count[1]} of {count[0]} items place a block, {len(differing)} under a different id)"
)
PY
