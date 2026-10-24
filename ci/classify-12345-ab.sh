#!/usr/bin/env bash
# classify-12345-ab.sh — recover the harvest order of the two UNSTAMPED
# seed-12345 references, from the data rather than from a script header.
#
# `gen-wide-12345-r16-ow-a` and `-b` were generated 2026-08-09 and carry no
# `HARVEST-ORDER` file. `ci/ow-r16-window-12345.sh`'s original header asserts
# they were made with `GEN_ORDER=rows`. That assertion is exactly the kind this
# project keeps finding to be false, and it does not have to be an assertion:
# ORACLE rule 29 measured two vanilla runs of one seed and window differing by
# ~32,000 blocks in the same chunk order and ~146,000 across orders — a 4.6x
# gap with nothing in it. So the order is READ OFF the matrix.
#
# Method: diff each unknown against one reference of each KNOWN order and
# compare. The unknown belongs to whichever order it is closer to. Vanilla
# first in every call; nothing is written to a or b.
#
#   c     GEN_ORDER=rows      (stamped)
#   rr-a  GEN_ORDER=rows_rev  (stamped)
#
# WHY IT MATTERS RATHER THAN BEING TIDINESS. `ci/figures.toml:ow-r16-seed12345`
# publishes 77,067 with `harvest = "unrecorded"`. If a and b turn out to share
# an order, the old figure was at least internally consistent and its only
# defect is that our dump's order was never matched to theirs. If they do NOT,
# then the pair itself straddled two orders and its floor of 12,774 — the
# number that made seed 12345 "the only one of the three that ever had
# resolving power" — was measuring the order gap and not the run gap.
#
# A CONTROL IS INCLUDED AND IT IS NOT OPTIONAL. `c vs d` (two known `rows`) and
# `c vs rr-a` (known `rows` against known `rows_rev`) are measured in the same
# run, so the verdict is read against this seed's own same-order and
# cross-order distances rather than against 20260722's. A classification with
# no scale under it is a guess with a number next to it.
#
# usage: ci/classify-12345-ab.sh
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
G=./target/release/region-differ
P="$HOME/rustmc/oracle-work/gen-wide-12345-r16-ow"
BBOX="48 48 80 80"
LOG=${CLASSIFY_LOG:-$HOME/rp-logs/classify-12345-ab.log}
mkdir -p "$(dirname "$LOG")"

d() { # shellcheck disable=SC2086
    "$G" diff "$P-$1" "$P-$2" --bbox $BBOX 2>/dev/null \
        | grep -oP 'mismatches=\K[0-9]+' | head -1
}

{
  echo "=== harvest order of the unstamped seed-12345 references @ $(date -Is) ==="
  for r in a b c d rr-a rr-b; do
      printf '  %-6s %s\n' "$r" \
          "$(head -1 "$P-$r/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED)"
  done
  echo "--- controls: this seed's own same-order and cross-order distances ---"
  SAME=$(d c d);  echo "  c  vs d     (rows    vs rows    ) $SAME"
  CROSS=$(d c rr-a); echo "  c  vs rr-a  (rows    vs rows_rev) $CROSS"
  echo "--- the unknowns ---"
  for u in a b; do
      TR=$(d c "$u"); TV=$(d rr-a "$u")
      if [ "$TR" -lt "$TV" ]; then v=rows; else v=rows_rev; fi
      printf '  %-4s vs c(rows) %-8s vs rr-a(rows_rev) %-8s  -> %s\n' "$u" "$TR" "$TV" "$v"
  done
  echo "  a  vs b     $(d a b)   (the pair the retired 77,067 was taken against)"
  echo "=== CLASSIFY DONE ==="
} > "$LOG" 2>&1
cat "$LOG"
