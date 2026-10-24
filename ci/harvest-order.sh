#!/usr/bin/env bash
# harvest-order.sh — establish the dump order a reference was harvested in, or
# refuse.
#
# ORACLE rule 29 is "read the order off the reference, never assert it", and
# `ci/nether-biome-window.sh` obeys it by grepping `dump_order=` out of the
# reference's own `HARVEST-ORDER`. That works in the nether because every
# nether reference carries one. IT DOES NOT WORK IN THE OVERWORLD, and the
# reason is worth stating rather than working around:
#
#   EIGHT OF THE NINE `gen-wide-20260722-r16-ow-*` REFERENCES CARRY NO
#   `HARVEST-ORDER` AT ALL. `-a` through `-h` are region data and nothing
#   else. Their order is not lost — it was DERIVED on 2026-08-10 by
#   `ci/classify-20260722-order.sh`, which scored each against the stamped
#   `-i` and against `-rr-a`, and the result was written to
#   `~/w22-logs/stamps/gen-wide-20260722-r16-ow-<x>/HARVEST-ORDER`: a
#   PARALLEL DIRECTORY TREE holding stamps whose data lives somewhere else.
#
# So the rule is satisfiable only by a script that knows to look somewhere
# else. One does: `ci/floor-census-20260722.sh` runs the classifier first and
# passes `--stamps "$STAMPS"` into `ci/floor-census.py`, so its N=8 floor is
# scored against a measured order and not an asserted one. THAT WAS STATED
# BACKWARDS IN THIS HEADER WHEN IT LANDED and is corrected here rather than
# quietly edited: the claim was that the script read no order from any of the
# eight, and it reads all eight.
#
# `ci/ow-r16-window.sh` is the one that does not. It hard-codes
# `gen-wide-20260722-r16-ow-{a,b}`, reads no order, sets no
# `RUSTMC_DECORATE_ORDER`, and has no existence guard on either reference — so
# if they vanish it does not refuse, and while they are there it dumps in
# whatever order the binary defaults to. `ci/figures.toml` already records the
# consequence as `dump_order = "x-major (UNMATCHED)"` on `ow-r16-admissible-n2`,
# which is worth 32,127 against 146,169 by the classifier's own controls.
#
# This resolver is the single place that knows the stamp can live apart from
# its data. It prints the order on stdout and the provenance on stderr, and it
# REFUSES rather than defaulting — a derived order is usable and an absent one
# is not, and the two must not look alike to a caller.
#
# Usage:   ord=$(ci/harvest-order.sh <reference-dir>) || exit 2
# Output:  one of `zx`, `zx_zrev` on stdout.
# Exit:    0 established, 2 could not be established.
#
# `STAMP_TREES` is a colon list searched in order when the reference has no
# stamp of its own. A stamp found there is reported as `derived` even when the
# file itself does not say so, because a stamp that does not sit with its data
# cannot be shown to describe it.
set -uo pipefail
REF=${1:?usage: harvest-order.sh <reference-dir>}
STAMP_TREES=${STAMP_TREES:-$HOME/w22-logs/stamps}
name=$(basename "$REF")

read_order() {  # <harvest-order-file>
    grep -m1 -oP '^dump_order=\K\S+' "$1" 2>/dev/null
}

if [ ! -d "$REF" ]; then
    echo "harvest-order: no reference at $REF — REFUSING." >&2
    exit 2
fi
if ! ls "$REF"/*.mca >/dev/null 2>&1 && ! ls "$REF"/region/*.mca >/dev/null 2>&1; then
    echo "harvest-order: $REF holds no region data — it is a stamp, not a reference. REFUSING." >&2
    exit 2
fi

ord=""; prov=""
if [ -f "$REF/HARVEST-ORDER" ]; then
    ord=$(read_order "$REF/HARVEST-ORDER")
    prov="stamped at generation, beside its data"
else
    IFS=: read -r -a trees <<< "$STAMP_TREES"
    for t in "${trees[@]}"; do
        if [ -f "$t/$name/HARVEST-ORDER" ]; then
            ord=$(read_order "$t/$name/HARVEST-ORDER")
            prov="DERIVED — stamp found in $t/$name, not beside the data"
            break
        fi
    done
fi

case "$ord" in
    zx|zx_zrev) ;;
    "") echo "harvest-order: no order for $name in the reference or in $STAMP_TREES — REFUSING rather than guessing. A figure taken in the wrong order is a different measurement, not a worse one." >&2
        exit 2 ;;
    *)  echo "harvest-order: $name stamps dump_order='$ord', which no RUSTMC_DECORATE_ORDER reproduces — REFUSING." >&2
        exit 2 ;;
esac

echo "harvest-order: $name -> $ord ($prov)" >&2
echo "$ord"
