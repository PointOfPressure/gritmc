#!/usr/bin/env bash
# Attribute the r=16 residual to `ruined_portal`, corroborate it with a census
# that shares no mechanism with the diff, and render the portal by eye.
#
# The three answers are deliberately produced by three different instruments:
#
#   * the positional diff says WHICH positions disagree;
#   * the census says HOW MANY of each block each side holds, and shares no
#     code with the diff's position walk — a diff and a census agreeing is
#     evidence, a diff agreeing with itself is not;
#   * the render says whether the thing looks like a ruined portal, which is
#     the check the owner applies first and the only one that catches "every
#     block is individually plausible and the structure is nonsense".
#
# Vanilla FIRST in every diff. The bbox is the measured window, 48..80.
#
# usage: portal-attribute.sh <ours-region-dir> <label>
set -uo pipefail
OURS="${1:?need our region dir}"
LABEL="${2:?need a label}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-a"
REFB="$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-b"
OUT="$HOME/rp-logs/attr-$LABEL"
mkdir -p "$OUT"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"
D=./target/release/region-differ
W=./target/release/worldgen-dump

{
  echo "=== portal attribution: $LABEL @ $(date -Is) ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -2

  echo
  echo "=== 1. vanilla's OWN saved ruined_portal starts (the piece boxes) ==="
  # Vanilla's starts, not ours: attributing our residual to our own boxes
  # would score a structure we never placed as "outside every box".
  $W jigsaw-starts "$REF" ruined_portal | tee "$OUT/starts.txt"

  echo
  echo "=== 2. residual positions, VANILLA FIRST, twin-masked ==="
  $D diff "$REF" "$OURS" --twin "$REFB" --bbox 48 48 80 80 --positions \
      > "$OUT/positions.txt" 2>&1
  tail -6 "$OUT/positions.txt"
  grep -cE '^-?[0-9]+ -?[0-9]+ -?[0-9]+$' "$OUT/positions.txt" \
      | sed 's/^/positions listed: /'

  echo
  echo "=== 3. how many of those land in a ruined_portal PIECE box ==="
  # margin 0: the piece box itself, not a suspect list. The 16-block margin
  # this tool defaults to is what turned 24 chamber blocks into "11,623".
  python3 ci/attribute-residual.py "$OUT/positions.txt" "$OUT/starts.txt" 0 \
      2>&1 | tail -20

  echo
  echo "=== 4. census corroboration, portal blocks over the whole window ==="
  # Markers a ruined portal cannot avoid. `region-differ census` needs --bbox
  # AND --match, and the census must carry the SAME bbox as the diff.
  for M in obsidian crying_obsidian netherrack magma_block gold_block \
           stone_bricks cracked_stone_bricks mossy_stone_brick lava; do
    V=$($D census "$REF"  --bbox 48 48 80 80 --match "$M" 2>/dev/null | tail -1)
    O=$($D census "$OURS" --bbox 48 48 80 80 --match "$M" 2>/dev/null | tail -1)
    printf '  %-24s vanilla: %-28s ours: %s\n' "$M" "$V" "$O"
  done
  echo "=== portal attribution done: $LABEL ==="
} > "$HOME/rp-logs/attr-$LABEL.log" 2>&1
echo "wrote $HOME/rp-logs/attr-$LABEL.log"
