#!/usr/bin/env bash
# nether-fossil-golden.sh — vanilla's own `nether_fossil` starts, as a golden.
#
# Unusually for a structure, this one has a COMPLETE oracle: vanilla saves
# every start into its own chunk NBT, and `worldgen-dump jigsaw-starts` reads
# the `Children[]` array back out — chunk, template, rotation, template
# position and bounding box. Nothing here is inferred and nothing is a JVM
# re-run; it is vanilla's own saved bytes, reshaped into JSON.
#
# Usage: ci/nether-fossil-golden.sh [vanilla-region-dir] [out.json]
#
# Runs a release binary, so it goes on the build machine.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="${1:-$ROOT/oracle-work/van-nether-20260722-r7}"
OUT="${2:-$ROOT/crates/rustmc-worldgen/golden/nether_fossil_starts.json}"
[ -d "$REF" ] || { echo "no reference at $REF" >&2; exit 1; }

"$ROOT/target/release/worldgen-dump" jigsaw-starts "$REF" nether_fossil 2>/dev/null |
python3 -c '
import json, re, sys
starts = []
chunk = None
for line in sys.stdin:
    m = re.match(r"START minecraft:nether_fossil chunk \((-?\d+), (-?\d+)\)", line.strip())
    if m:
        chunk = [int(m.group(1)), int(m.group(2))]
        continue
    m = re.match(
        r"\s*0\s+minecraft:(\S+)\s+rot=(\S+)\s+TP=(-?\d+),(-?\d+),(-?\d+)\s+GD=(-?\d+)\s+"
        r"BB=\[(-?\d+) (-?\d+) (-?\d+) \.\. (-?\d+) (-?\d+) (-?\d+)\]", line)
    if m and chunk is not None:
        starts.append({
            "chunk": chunk,
            "template": m.group(1),
            "rotation": m.group(2),
            "pos": [int(m.group(3)), int(m.group(4)), int(m.group(5))],
            "ground_level_delta": int(m.group(6)),
            "bbox": [int(m.group(i)) for i in range(7, 13)],
        })
        chunk = None
starts.sort(key=lambda s: (s["chunk"][0], s["chunk"][1]))
if not starts:
    sys.exit("no nether_fossil starts in the reference — wrong directory?")
json.dump({"seed": 20260722, "source": "vanilla chunk NBT structures.starts.nether_fossil.Children[]",
           "starts": starts}, sys.stdout, indent=1)
print()
' > "$OUT"
echo "nether fossil starts -> $OUT ($(python3 -c "import json;print(len(json.load(open('$OUT'))['starts']))") starts)"
