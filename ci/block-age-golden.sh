#!/usr/bin/env bash
# block-age-golden.sh — vanilla's OWN `BlockAgeProcessor` draw tape, as a golden.
#
# Produces `crates/rustmc-worldgen/golden/block_age_tape.json`: for every
# position the measured ruined portal hands to `BlockAgeProcessor`, the
# ordered sequence of draws vanilla's processor makes and the state it
# returns — taken by running vanilla's own class in the JVM with a recording
# `RandomSource` injected through `StructurePlaceSettings.setRandom`.
#
# # Why this can exist when almost no other structure golden can
#
# `BlockAgeProcessor` reads nothing but the block info and the settings. Its
# random is per position (`Mth.getSeed(pos)`), so every position is
# independent, and no world, chunk, generator or server is needed to drive it
# — which is what lets the tape be exhaustive over the box rather than
# sampled. `tools/goldengen/vanilla/BlockAgeGolden.java` has the detail.
#
# # What it is for
#
# The losing candidate array's draws are invisible in the output. Both arrays
# are built before `getRandomBlock` chooses, and each costs a
# `getRandomFacingStairs` — which is TWO draws, FACING then HALF. A port that
# spends the wrong number is output-correct on the first block and stream-wrong
# from the second, and no block diff, census or region comparison can tell that
# from a wrong rule. Only the ordered sequence can. This is the artefact that
# named the first divergent draw index as index 2 on 2026-08-09.
#
# usage: block-age-golden.sh [positions] [mossiness] [out.json]
#
#   positions   a `RUSTMC_BLOCK_AGE_TAPE` file, or a previous golden JSON to
#               re-derive the same position set from. Defaults to the
#               committed golden, so a re-run needs no generator output.
#   mossiness   `ruined_portal/portal_3`'s, measured 0.2 (see the header note
#               in the JSON: the tape's own floats bracket it).
#
# Runs the JVM. **JDK 25, not 21** — the pinned jar is class-file version 69
# and javac 21 dies with `wrong version 69.0, should be 65.0`. Threadripper
# work like every other extract.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:-$ROOT/crates/rustmc-worldgen/golden/block_age_tape.json}"
MOSS="${2:-0.2}"
OUT="${3:-$ROOT/crates/rustmc-worldgen/golden/block_age_tape.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }
[ -s "$SRC" ] || { echo "no positions at $SRC" >&2; exit 1; }

# The JVM refuses to mix an unsigned class into a signed jar, so the
# signatures come out first. Same dance as every other golden here.
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

# `SurfaceChunkGolden` comes along for `bindBlockTags()`, which is
# package-private and which five other goldens already share. Without it every
# `state.is(BlockTags.…)` arm of the processor is silently dead.
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/BlockAgeGolden.java" 2>&1 | grep -v '^Note' || true

# `x,y,z,<state>` per line, from either input shape. Sorted and deduplicated
# so the golden does not depend on the order a threaded generator happened to
# walk the box in.
python3 - "$SRC" > "$WORK/positions.txt" <<'PY'
import json, re, sys
src = open(sys.argv[1], encoding="utf-8").read()
rows = set()
if src.lstrip().startswith("{"):
    for e in json.loads(src)["positions"]:
        rows.add((tuple(e["pos"]), e["src"]))
else:
    for m in re.finditer(r"^TAPE pos=(-?\d+),(-?\d+),(-?\d+) seed=-?\d+ src=(\S+)", src, re.M):
        rows.add(((int(m[1]), int(m[2]), int(m[3])), m[4]))
if not rows:
    sys.exit("no TAPE lines and no golden positions in the input")
for (x, y, z), s in sorted(rows):
    print(f"{x},{y},{z},{s}")
PY

COUNT="$(wc -l < "$WORK/positions.txt")"
# `Bootstrap.bootStrap()` re-emits stdout through log4j with a prefix, so the
# tape lines are matched by an ANCHORED grep rather than trusted whole.
java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$WORK:$CP" \
    net.minecraft.world.level.levelgen.BlockAgeGolden "$MOSS" \
    < "$WORK/positions.txt" > "$WORK/raw.txt"

python3 - "$WORK/raw.txt" "$MOSS" "$COUNT" > "$OUT" <<'PY'
import json, re, sys
raw, moss, expected = open(sys.argv[1], encoding="utf-8").read(), float(sys.argv[2]), int(sys.argv[3])
out = []
for line in raw.splitlines():
    # `Bootstrap.bootStrap()` re-emits stdout through log4j, so a tape line
    # arrives as `[17:36:43] [main/INFO]: [STDOUT]: TAPE …`. The prefix is
    # matched explicitly rather than stripped by a loose `.*TAPE` — an
    # unanchored grep here would also match a tape line quoted inside a
    # stack trace or a warning, which is the failure `ci/shape-golden.sh`
    # warns about in its own header.
    m = re.match(r"^(?:\[[^\]]*\] \[[^\]]*\]: \[STDOUT\]: )?"
                 r"TAPE pos=(-?\d+),(-?\d+),(-?\d+) seed=(-?\d+) src=(\S+) draws=(\d+)(.*?) out=(\S+)$",
                 line.strip())
    if not m:
        continue
    draws = [d.split(":", 1)[1] for d in m[7].split()] if m[7].strip() else []
    if len(draws) != int(m[6]):
        sys.exit(f"draw count disagrees with the draw list: {line}")
    out.append({
        "pos": [int(m[1]), int(m[2]), int(m[3])],
        "seed": int(m[4]),
        "src": m[5],
        "draws": draws,
        "out": None if m[8] == "-" else m[8],
    })
# A harness that silently tapes fewer positions than it was given turns an
# exhaustive tape into a sampled one, which is the whole thing this artefact
# exists to avoid.
if len(out) != expected:
    sys.exit(f"asked for {expected} positions, vanilla taped {len(out)}")
out.sort(key=lambda e: (e["pos"][0], e["pos"][1], e["pos"][2]))
json.dump({
    "source": "vanilla 26.2 BlockAgeProcessor.processBlock, run in the JVM with a "
              "recording RandomSource injected via StructurePlaceSettings.setRandom",
    "generator": "ci/block-age-golden.sh + tools/goldengen/vanilla/BlockAgeGolden.java",
    "seed_note": "seed is Mth.getSeed(pos) — BlockAgeProcessor's stream is per position, "
                 "so every entry here is independent of every other",
    "world": "seed 20260722 overworld, vanilla's own saved ruined_portal/portal_3 at "
             "chunk (48,64), piece box [768 42 1032 .. 776 50 1039]",
    "mossiness": moss,
    "count": len(out),
    "positions": out,
}, sys.stdout, indent=1)
print()
PY

echo "block_age tape -> $OUT ($COUNT positions, mossiness $MOSS)"
python3 -c "
import json,collections
d=json.load(open('$OUT'))
h=collections.Counter(len(e['draws']) for e in d['positions'])
print('  draw-count histogram:', dict(sorted(h.items())))
"
