#!/usr/bin/env bash
# fluid-tape.sh — take the fluid pass's tape and score it against vanilla.
#
# `RUSTMC_FLUID_TAPE=<path>` makes `stage::fluid` record one line per tick,
# per flow write and per lava conversion. This runs a window with the tape on
# and then scores it, position by position, against a vanilla reference.
#
# # Why a reference and not a golden
#
# Everything this pass writes is water, lava, obsidian, cobblestone or stone,
# and it is the last thing to touch those blocks before the region is saved.
# So a vanilla reference region **is** the printout of vanilla's own run of
# the same pass — a saved obsidian is a conversion vanilla made, and a saved
# lava under water is one it did not. (Lava joined the list on 2026-08-10,
# when the pass stopped declining it; `stone` is `LavaFluid#spreadTo`'s DOWN
# into water.) No JVM, no instrumented vanilla, and the score is
# exhaustive over the window rather than sampled. Same argument as the
# ruined-portal apron counter.
#
# # What it scores, and why both directions are needed
#
#   PRECISION  every `convert` the tape claims must land on a position where
#              vanilla holds the same block. A pass that converts too eagerly
#              — the failure mode of the cascade fix that closed this — shows
#              up here and nowhere else.
#   RECALL     every position where vanilla holds obsidian or cobblestone and
#              we hold lava is a conversion vanilla made and we did not. This
#              needs no structure exclusion: a ruined portal's obsidian is
#              obsidian on both sides and never appears.
#   FLOODS     every `flow` write of water must land where vanilla holds
#              water. A cascade that halts early shows up as *extra* floods,
#              because the water descends where an obsidian should already
#              have blocked it — which is exactly how the y=-55 interface
#              read before it was fixed: three floods against vanilla's one.
#
# The three fail independently on purpose. A count of conversions would have
# been right on the day the pass converted two blocks out of thirty.
#
# usage: ci/fluid-tape.sh <label> [radius] [centre-x centre-z]
set -uo pipefail
LABEL="${1:?need a label}"
R="${2:-16}"
CX="${3:-64}"
CZ="${4:-64}"
SEED="${SEED:-20260722}"
REF="${REF:-$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-a}"
BBOX="${BBOX:-48 48 80 80}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$HOME/rp-logs/fluid-tape-$LABEL"
WORLD="$HOME/rp-fluidtape-$LABEL"
mkdir -p "$OUT"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

echo "=== fluid tape: $LABEL @ $(date -Is) ==="
cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -2 || exit 2
D=./target/release/region-differ

# RUSTMC_WORKERS=1 is not a performance choice. Chunks append to the tape under
# a lock, so no line is ever torn, but two chunks' runs interleave and a
# cascade that cannot be read in order is not a tape.
rm -rf "$WORLD" "$OUT/tape.txt"
echo "=== dump seed $SEED r=$R centre ($CX,$CZ) RUSTMC_WORKERS=1, tape on ==="
RUSTMC_WORKERS=1 RUSTMC_FLUID_TAPE="$OUT/tape.txt" \
    ./target/release/worldgen-dump full "$SEED" "$R" "$WORLD" "$CX" "$CZ" 2>&1 | tail -3

# An empty tape and a tape that was never switched on write the same file, and
# this project has paid for that distinction more than once.
if [ ! -s "$OUT/tape.txt" ]; then
    echo "REFUSED  the tape is empty — RUSTMC_FLUID_TAPE never reached the pass," \
         "or the window has no marked fluid position at all"
    exit 2
fi
echo "=== tape ==="
awk '{print $1}' "$OUT/tape.txt" | sort | uniq -c | sort -rn

kinds() { # <region-dir> <kind> <outfile>
    $D census "$1" --bbox $BBOX --match "$2" --positions 2>/dev/null |
        awk -v k="minecraft:$2" '$4 == k { print $1, $2, $3 }' | sort > "$3"
}
for K in obsidian cobblestone lava water; do
    kinds "$REF" "$K" "$OUT/van.$K"
    kinds "$WORLD/region" "$K" "$OUT/ours.$K"
done

python3 - "$OUT" <<'PY'
import sys, pathlib
out = pathlib.Path(sys.argv[1])
def load(name):
    return {tuple(l.split()) for l in (out / name).read_text().split('\n') if l}
van = {k: load(f'van.{k}') for k in ('obsidian', 'cobblestone', 'lava', 'water')}
ours = {k: load(f'ours.{k}') for k in ('obsidian', 'cobblestone', 'lava', 'water')}

converts, floods = {}, set()
for line in (out / 'tape.txt').read_text().split('\n'):
    parts = line.split()
    if not parts:
        continue
    if parts[0] == 'convert':
        pos = tuple(parts[1:4])
        # `... -> minecraft:obsidian water=up`
        converts[pos] = parts[parts.index('->') + 1].split(':')[-1]
    elif parts[0] == 'flow' and parts[-1].split(':')[-1].startswith('water'):
        floods.add(tuple(parts[1:4]))

# The window bound: the tape covers whatever was dumped, the census covers the
# bbox. Score only where the two overlap, and say how many were dropped — an
# unbounded census once explained a 225-chunk window with its margin.
inbox = van['obsidian'] | van['cobblestone'] | van['lava'] | van['water']
dropped = [p for p in converts if p not in inbox]
scored = {p: k for p, k in converts.items() if p in inbox}

wrong = {p: k for p, k in scored.items() if p not in van[k]}
missing = (van['obsidian'] | van['cobblestone']) & ours['lava']
extra = floods & (van['obsidian'] | van['cobblestone'] | van['lava'])

print()
print(f'PRECISION  {len(scored) - len(wrong)}/{len(scored)} conversions land where vanilla agrees'
      f'   ({len(dropped)} outside the scored bbox, not charged)')
print(f'RECALL     {len(missing)} vanilla obsidian/cobblestone positions we still hold as lava')
print(f'FLOODS     {len(extra)} water writes landed where vanilla has no water')
for label, s in (('wrong', set(wrong)), ('missing', missing), ('extra-flood', extra)):
    for p in sorted(s, key=lambda t: (int(t[1]), int(t[0]), int(t[2])))[:12]:
        print(f'   {label}  {" ".join(p)}')
bad = len(wrong) + len(missing) + len(extra)
print()
print('FLUID TAPE GREEN' if bad == 0 else f'FLUID TAPE RED  {bad} charged')
sys.exit(0 if bad == 0 else 1)
PY
S=$?
echo "=== tape and position lists kept under $OUT ==="
exit $S
