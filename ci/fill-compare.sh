#!/usr/bin/env bash
# fill-compare.sh — attribute a block disagreement to a STAGE, not to a guess.
#
#   ci/fill-compare.sh <seed> <chunk-x> <chunk-z> [label]
#
# # The gap this fills
#
# Every instrument in this tree compares *finished* worlds. A finished world
# is the noise fill, then the surface rule, then carvers, then decoration, and
# a block that disagrees could have been written wrong by any of them or
# written right and then overwritten. `ci/block-kind-census.sh` says *which
# positions* disagree; nothing said *which stage owns them*, so every item
# opened on a census has begun by guessing a stage and reading its source.
# Two of the last three such guesses were wrong.
#
# This compares three worlds at one chunk:
#
#   VANILLA FILL   `NetherRealFillProbe --overworld` — vanilla's own private
#                  `doFill` over a real `ProtoChunk`, i.e. the noise fill and
#                  the aquifer and the ore veins and NOTHING ELSE.
#   OUR FILL       our `RUSTMC_THROUGH=noise` dump of the same chunk.
#   VANILLA FINAL  the finished reference region.
#
# and splits every disagreeing position into exactly one of:
#
#   FILL  our fill differs from vanilla's fill. Ours, and it is a density,
#         aquifer or ore-vein defect — the three things the fill does.
#   POST  the two fills AGREE and the finished reference differs from both.
#         A later vanilla stage moved it. Not a fill defect, and no amount of
#         reading `Aquifer` will find it.
#
# The FILL half needs no twin and has no floor: `doFill` is pure noise and a
# positional RNG, single-threaded, with no decoration order to vary. That is
# checked rather than asserted — `--twice` runs the probe twice and diffs.
# The POST half is a residual of the finished world and does carry the usual
# floor, so it is reported as an attribution and never as a score.
#
# # BOTH FILLS ARE BEARDLESS, AND THAT IS THE FIRST THING TO KNOW ABOUT THIS
#
# `NoiseChunk`'s constructor adds `BeardifierMarker` to `finalDensity`
# unconditionally — it is never in a datapack, in any version — so a real
# server's noise stage raises terrain around every structure whose
# `terrain_adaptation` is not `none`. Neither side of the default comparison
# has that:
#
#   * the probe is run with the marker, because `NetherRealFillProbe` is given
#     no `StructureManager`;
#   * our `RUSTMC_THROUGH=noise` world is beardless because
#     `worldgen-assembly::generate` gates `village_starts` and
#     `trial_chamber_starts` behind `through >= Through::Decoration`, so a
#     noise-, surface- or carver-stage dump has no bearding start to give the
#     beardifier at all.
#
# So a default GREEN says "our fill matches vanilla's fill where neither is
# bearded". It does NOT say the noise stage is right, and on seed 20260722 it
# said GREEN over a chunk holding 217 blocks of missing beard.
#
# `--beard` closes that. It reads the reference's own piece boxes with
# `region-differ starts`, keeps the bearding structures within the kernel's
# 12 blocks of the chunk, and hands them to the probe, which then builds a
# real `Beardifier`. The FILL count under `--beard` is the beard our noise
# stage is missing, and it is a debt rather than a fill defect for as long as
# that `Through::Decoration` gate stands.
#
# VANILLA FIRST is the argument order of every diff below.
#
# Runs a JVM, so it goes on the Threadripper like everything else that runs.
set -uo pipefail
SEED="${1:?need a seed}"
CX="${2:?need a chunk x}"
CZ="${3:?need a chunk z}"
LABEL="${4:-$CX-$CZ}"
TWICE=0
BEARD=0
for a in "$@"; do
    [ "$a" = "--twice" ] && TWICE=1
    [ "$a" = "--beard" ] && BEARD=1
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="${REF:-$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-a}"
# The chunk window `--beard` searches for structure STARTS. It is not the
# compared window — that is one chunk — and it has to be wider than one chunk
# because a start is recorded in its own start chunk, which is routinely tens
# of chunks from the chunk its beard reaches.
BBOX="${BBOX:-48 48 80 80}"
OUT="$HOME/rp-logs/fill-compare-$LABEL"
WORLD="$HOME/rp-fillcmp-$LABEL"
mkdir -p "$OUT"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

BX=$((CX * 16))
BZ=$((CZ * 16))

echo "=== fill compare: seed $SEED chunk ($CX,$CZ) @ $(date -Is) ==="
echo "  reference $REF"

# The bearding pieces, from the REFERENCE's own NBT rather than from our
# placement: a beard built out of boxes this port derived cannot adjudicate
# this port's beard. `region-differ starts` needs the tool built, so this runs
# after the build below is not an option — build it here.
SPECS=""
if [ "$BEARD" = 1 ]; then
    cargo build --release -p region-differ 2>&1 | tail -1 || exit 2
    # Only the structures that actually beard. `mineshaft`, `shipwreck` and
    # `ocean_ruin` declare no adaptation, and vanilla's own predicate at
    # `forStructuresInChunk` offset 2 drops them, so including them here would
    # invent a beard vanilla does not have.
    ADAPT="${ADAPT:-encapsulate}"
    MATCH="${BEARDMATCH:-trial}"
    SPECS=$(./target/release/region-differ starts "$REF" --bbox $BBOX --match "$MATCH" 2>/dev/null |
        awk -v x0=$((BX - 12)) -v x1=$((BX + 15 + 12)) \
            -v z0=$((BZ - 12)) -v z1=$((BZ + 15 + 12)) -v a="$ADAPT" \
            'BEGIN { OFS = "," }
             # PIECE <structure> <startChunk> <piece> x1 y1 z1 x2 y2 z2
             # The box is $5..$10. It was $4..$9 before the start chunk joined
             # the line, and reading it one field left silently selected zero
             # pieces and printed a beard of nothing.
             $1 == "PIECE" && $5 <= x1 && $8 >= x0 && $7 <= z1 && $10 >= z0 {
                 printf "%s,%s,%s,%s,%s,%s,%s ", $5, $6, $7, $8, $9, $10, a }')
    echo "=== beard: $(echo "$SPECS" | wc -w) reference pieces within 12 of the chunk, $ADAPT ==="
    if [ -z "$SPECS" ]; then
        echo "REFUSED  --beard was asked for and no reference piece is in range;" \
             "a beard of nothing is the beardless run under a different name"
        exit 2
    fi
fi

echo "=== vanilla's own doFill ==="
# shellcheck disable=SC2086  # SPECS is a deliberate list of argv words
./ci/nether-real-fill-probe.sh "$SEED" "$BX" "$BZ" --overworld $SPECS > "$OUT/van.fill" 2>/dev/null
grep -c '^BLOCK ' "$OUT/van.fill" | sed 's/^/  non-air blocks: /'
if [ "$(grep -c '^BLOCK ' "$OUT/van.fill")" -eq 0 ]; then
    echo "REFUSED  the probe produced no blocks — a silent probe is not a reference"
    exit 2
fi
if [ "$TWICE" = 1 ]; then
    # shellcheck disable=SC2086
    ./ci/nether-real-fill-probe.sh "$SEED" "$BX" "$BZ" --overworld $SPECS > "$OUT/van.fill2" 2>/dev/null
    N=$(diff "$OUT/van.fill" "$OUT/van.fill2" | grep -c '^[<>]')
    echo "  determinism: $N lines differ between two runs of the probe"
    [ "$N" -ne 0 ] && { echo "REFUSED  doFill is not reproducing itself"; exit 2; }
fi

echo "=== our fill (RUSTMC_THROUGH=noise) ==="
cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -1 || exit 2
D=./target/release/region-differ
rm -rf "$WORLD"
RUSTMC_WORKERS=1 RUSTMC_THROUGH=noise \
    ./target/release/worldgen-dump full "$SEED" 1 "$WORLD" "$CX" "$CZ" 2>&1 | tail -1

$D census "$WORLD/region" --bbox "$CX" "$CZ" "$CX" "$CZ" --positions 2>/dev/null > "$OUT/ours.fill"
$D census "$REF"          --bbox "$CX" "$CZ" "$CX" "$CZ" --positions 2>/dev/null > "$OUT/van.final"
wc -l < "$OUT/ours.fill" | sed 's/^/  our census rows:   /'
wc -l < "$OUT/van.final" | sed 's/^/  final census rows: /'

python3 - "$OUT" "$BEARD" <<'PY'
import sys, pathlib, collections
out = pathlib.Path(sys.argv[1])
bearded = sys.argv[2] == '1'

def strip(name):
    # `Block{minecraft:lava}[level=0]` and `minecraft:lava[level=0]` both mean
    # lava here: the fill's own state bits are not what a stage attribution is
    # about, and comparing them would charge `level=` differences that no
    # later reader of this number cares about.
    name = name.strip()
    if name.startswith('Block{'):
        name = name[len('Block{'):].split('}')[0]
    return name.split('[')[0]

def from_probe(path):
    d = {}
    for line in path.read_text().split('\n'):
        p = line.split()
        if len(p) >= 5 and p[0] == 'BLOCK':
            d[(int(p[1]), int(p[2]), int(p[3]))] = strip(p[4])
    return d

def from_census(path):
    d = {}
    for line in path.read_text().split('\n'):
        p = line.split()
        if len(p) == 4:
            n = strip(p[3])
            if n != 'minecraft:air':
                d[(int(p[0]), int(p[1]), int(p[2]))] = n
    return d

vfill = from_probe(out / 'van.fill')
ofill = from_census(out / 'ours.fill')
vfin  = from_census(out / 'van.final')
AIR = 'minecraft:air'

keys = set(vfill) | set(ofill) | set(vfin)
fill_bad = {}
post = {}
for k in keys:
    a, b, c = vfill.get(k, AIR), ofill.get(k, AIR), vfin.get(k, AIR)
    if a != b:
        fill_bad[k] = (a, b, c)
    elif a != c:
        post[k] = (a, c)

def hist(d, key):
    return collections.Counter(key(v) for v in d.values()).most_common(8)

print()
print(f'FILL  {len(fill_bad):6d} positions where OUR fill differs from VANILLA fill')
for (a, b), n in hist(fill_bad, lambda v: (v[0], v[1])):
    print(f'         {n:6d}  vanilla {a}   ours {b}')
print(f'POST  {len(post):6d} positions where the two fills AGREE and the finished'
      f' reference differs')
for (a, c), n in hist(post, lambda v: (v[0], v[1])):
    print(f'         {n:6d}  fill {a}   final {c}')

if fill_bad:
    print('\n   first FILL disagreements (y-sorted):')
    for k in sorted(fill_bad, key=lambda t: (t[1], t[0], t[2]))[:12]:
        a, b, c = fill_bad[k]
        print(f'      {k[0]} {k[1]} {k[2]}   vanilla-fill {a}   ours-fill {b}   final {c}')

ys = collections.Counter(k[1] for k in post)
if ys:
    print('\n   POST y-histogram (top 10):')
    for y, n in ys.most_common(10):
        print(f'      y={y:<6} {n}')

print()
tag = 'FILL COMPARE (BEARDED)' if bearded else 'FILL COMPARE'
print(f'{tag} GREEN' if not fill_bad else f'{tag} RED  {len(fill_bad)} charged')
if bearded and fill_bad:
    print('   under --beard a RED is the beard our noise stage is missing, not a'
          ' density defect —')
    print('   `village_starts`/`trial_chamber_starts` are gated behind'
          ' `through >= Through::Decoration`.')
sys.exit(0 if not fill_bad else 1)
PY
S=$?
echo "=== kept under $OUT ==="
exit $S
