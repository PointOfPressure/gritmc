#!/usr/bin/env bash
# refs-12345-ensemble.sh — build the seed-12345 r=16 overworld reference
# ensemble, every reference stamped with the harvest order that produced it.
#
# WHY THIS EXISTS
#
# `ci/ow-r16-window-12345.sh` was written against the pair
# `gen-wide-12345-r16-ow-{a,b}`, generated 2026-08-09. **Neither of them
# carries a `HARVEST-ORDER` file.** They predate the stamp
# `ci/gen-vanilla-region.sh` now writes, so their chunk order is asserted in a
# script header and nowhere in the artefact itself.
#
# That is disqualifying for a LEVEL and the price is known: ORACLE rule 29 and
# `docs/DETERMINISM.md` §15 measured the same instrument charging an
# *unmodified vanilla run* 117,628 blocks on the 20260722 window for nothing
# but a mismatched chunk order, against 3,804 for being a different run of the
# right one. A number taken against an unstamped reference is a number taken
# under a condition nobody set, and this project has now paid for that twice.
#
# So rather than trust the header, this script generates a fresh ensemble whose
# order is *stamped by the generator that produced it*, and leaves `a` and `b`
# untouched — they are somebody else's directories and they are still perfectly
# good inputs to a delta, which does not depend on the order being stated.
#
# a and b are not deleted, not regenerated, and not written to. They are read
# in the pair matrix so that their harvest order can be *classified from the
# data* rather than asserted, which is the only honest way to recover an
# unstamped reference.
#
# GEOMETRY, identical to the 20260722 ensemble and to
# `ci/ow-r16-window-12345.sh` on purpose: r=16, centre chunk (64,64),
# 33x33 = 1,089 chunks, bbox 48 48 80 80.
#
# LABELS AND PORTS. Ports are allocated from 25931 upward, disjoint from the
# 25921/25922 the existing pair used and from the 25701 default, because
# `ci/gen-vanilla-region.sh` derives its scratch server directory from the port
# and two runs sharing one would collide. One port is used once, ever.
#
#   rows       (dump_order=zx)       c d e f g h    ports 25931..25936
#   rows_rev   (dump_order=zx_zrev)  rr-a rr-b      ports 25941..25942
#
# Six same-order references gives N up to 6 with a and b excluded, which is
# p_hide(6) = 39.3%; two of the other order are what make an order-varied
# ensemble possible at all, and §15's finding was that two order-varied
# references are worth far more than a ninth same-order one.
#
# REFERENCES ARE CHEAP AND THAT IS A 2026-08-09 CORRECTION. This was believed
# to cost hours for months and was never attempted. Each one is ~70 s.
#
# This script is IDEMPOTENT BY REFUSAL: a reference directory that already
# exists is skipped and never regenerated, so a resume costs only what it has
# not already done and no existing reference can be destroyed by re-running it.
#
# usage: ci/refs-12345-ensemble.sh
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

DEST=${REFS_DEST:-$HOME/rustmc/oracle-work}
LOG=${REFS_LOG:-$HOME/rp-logs/refs-12345-ensemble.log}
mkdir -p "$(dirname "$LOG")" "$DEST"

# label:port:GEN_ORDER
SPEC="c:25931:rows
d:25932:rows
e:25933:rows
f:25934:rows
g:25935:rows
h:25936:rows
rr-a:25941:rows_rev
rr-b:25942:rows_rev"

rc_all=0
{
  echo "=== seed-12345 r=16 overworld ensemble @ $(date -Is) ==="
  for line in $SPEC; do
    lab=${line%%:*}; rest=${line#*:}; port=${rest%%:*}; ord=${rest#*:}
    out="$DEST/gen-wide-12345-r16-ow-$lab"
    if [ -d "$out" ]; then
      echo "SKIP  $lab  exists at $out ($(head -1 "$out/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED))"
      continue
    fi
    echo "--- $lab port=$port GEN_ORDER=$ord -> $out"
    t0=$(date +%s)
    GEN_ORDER="$ord" "$ROOT/ci/gen-vanilla-region.sh" 12345 16 "$out" "$port" 64 64 overworld
    rc=$?
    echo "    rc=$rc  $(( $(date +%s) - t0 ))s  $(ls "$out"/*.mca 2>/dev/null | wc -l) region files  $(head -1 "$out/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED)"
    [ "$rc" = 0 ] || rc_all=1
  done
  echo "=== ENSEMBLE DONE rc=$rc_all @ $(date -Is) ==="
} > "$LOG" 2>&1
echo "log: $LOG rc=$rc_all"
exit $rc_all
