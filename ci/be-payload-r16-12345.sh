#!/usr/bin/env bash
# be-payload-r16-12345.sh — the block-entity PAYLOAD reading on the quotable
# overworld window, and the artefact behind
# `ow-r16-12345-be-payload-20260817`.
#
# WHAT IT MEASURES, AND WHAT IT REFUSES TO CALL A SCORE.
#
# `region-differ bediff` splits a block-entity comparison four ways: a position
# where only vanilla has one (MISSING), only we do (SPURIOUS), both do and the
# type differs (ID), both do and the saved keys differ (PAYLOAD). This script
# reports all four and quotes exactly one of them as a figure: **of the
# positions where both sides write a block entity, how many agree key for key
# and type for type.**
#
# The union-based `exactness` line bediff also prints is NOT quoted, here or in
# the docs. Its denominator mixes presence with payload — most of the union is
# positions where a class is not ported at all — so it moves when a class lands
# and moves when a payload is fixed, and a number that answers two questions
# answers neither. `docs/DETERMINISM.md` §43.3.
#
# WHY THE PAYLOAD HALF IS ADMISSIBLE AND THE PRESENCE HALF IS NOT, HERE.
#
# `ow-r16-12345-be-floor` measured what two unmodified vanilla runs of this seed
# disagree about, over all 120 unordered pairs of sixteen references: 0..13
# positions, and **0 payload mismatches and 0 id mismatches in every pair**. So
# the presence half has a floor and the payload half has a floor of exactly
# zero. That is what makes a 100% payload claim meaningful where the same claim
# on block states would be inside its own noise, and it is measured rather than
# assumed.
#
# ORACLE RULE 29 IS NOT OPTIONAL HERE. The reference carries a `HARVEST-ORDER`
# stamp and the dump's `RUSTMC_DECORATE_ORDER` is set from it. An unstamped
# reference is refused rather than guessed at: chunk order is worth 83% of the
# residual on one window and 6.7% on another, so it is a condition of the
# comparison and not a detail.
#
# NEEDS NO VANILLA BOOT and does not take the corpus lock — the reference is the
# existing 2026-08-10 rows harvest. One dump of ours, ~4 minutes on the
# Threadripper, plus a two-second bediff.
# THE READING THIS PRODUCED, 2026-08-17, branch worldgen-block-entities off main
# 8d06bb06, Threadripper: vanilla_be=173 ours_be=71 union=176, missing=105
# spurious=3 id=0 payload=0 exact=68 — that is
# 68 of 68 positions where both have a block entity, key for key
# and it is the reading `ow-r16-12345-be-payload-20260817` publishes. Recorded
# here so that a re-run which disagrees is a visible disagreement rather than a
# fresh number nobody can compare.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

SEED=${SEED:-12345}
R=${R:-16}
CX=${CX:-64}
CZ=${CZ:-64}
BBOX=${BBOX:-"48 48 80 80"}
REF=${REF:-$ROOT/oracle-work/gen-wide-12345-r16-ow-c}
OUT=${OUT:-$ROOT/oracle-work/be-payload-r16-12345}
DUMP="$ROOT/target/release/worldgen-dump"
DIFFER="$ROOT/target/release/region-differ"

for bin in "$DUMP" "$DIFFER"; do
    [ -x "$bin" ] || {
        echo "REFUSED  $bin is not built — cargo build --release -p worldgen-dump -p region-differ" >&2
        exit 2
    }
done
[ -d "$REF" ] || { echo "REFUSED  no reference at $REF" >&2; exit 2; }

# The order the reference was harvested in, and the dump order that matches it.
STAMP="$REF/HARVEST-ORDER"
if [ ! -f "$STAMP" ]; then
    echo "REFUSED  $REF carries no HARVEST-ORDER stamp. Its chunk order is"
    echo "         recoverable but not KNOWN, and an unstamped anchor is not an"
    echo "         anchor — ORACLE rule 30."
    exit 2
fi
ORDER=$(grep -oP '^dump_order=\K\S+' "$STAMP")
[ -n "$ORDER" ] || { echo "REFUSED  $STAMP names no dump_order"; exit 2; }
echo "reference $(basename "$REF"): $(tr '\n' ' ' < "$STAMP")"
echo "dump order: $ORDER"

rm -rf "$OUT"
RUSTMC_WORKERS=${RUSTMC_WORKERS:-6} \
RUSTMC_DECORATE_ORDER="$ORDER" \
RUSTMC_RECORD_BLOCK_ENTITIES=1 \
    "$DUMP" full "$SEED" "$R" "$OUT" "$CX" "$CZ" > "$OUT.out" 2> "$OUT.err"
rc=$?
if [ "$rc" -ne 0 ]; then
    echo "FAIL  the dump exited $rc"
    tail -20 "$OUT.err"
    exit 1
fi

# Vanilla first. The sign of every one of these numbers depends on it.
# shellcheck disable=SC2086
"$DIFFER" bediff "$REF" "$OUT/region" --bbox $BBOX > "$OUT.bediff" 2>&1
grep -E '^BEDIFF|^  (missing|exactness)' "$OUT.bediff"
LINE=$(grep -m1 '^  missing=' "$OUT.bediff")
get() { echo "$LINE" | grep -oP "$1=\K[0-9]+"; }
MISSING=$(get missing); SPURIOUS=$(get spurious)
IDS=$(get id); PAYLOAD=$(get payload); EXACT=$(get exact)
BOTH=$((EXACT + IDS + PAYLOAD))

if [ "$BOTH" -eq 0 ]; then
    echo "VACUOUS  no position has a block entity on both sides, so there is no"
    echo "         payload to compare. Not a pass."
    exit 3
fi

echo
echo "positions where both sides write a block entity: $BOTH"
echo "  id mismatches      $IDS"
echo "  payload mismatches $PAYLOAD"
echo "  exact              $EXACT"
if [ "$IDS" -eq 0 ] && [ "$PAYLOAD" -eq 0 ]; then
    echo "$EXACT of $BOTH positions where both have a block entity, key for key"
else
    echo "$EXACT of $BOTH positions where both have a block entity agree; $IDS id"
    echo "and $PAYLOAD payload mismatch(es) — see the by-key breakdown above,"
    echo "which is what found TAG_Int against putShort on the spawner counts."
fi
echo
echo "presence, quoted beside it and never folded into it: ours vs vanilla's 173,"
echo "missing=$MISSING spurious=$SPURIOUS. The presence floor on this window is"
echo "0..13 positions at median 5 (ow-r16-12345-be-floor, n=120 pairs); the"
echo "payload floor is 0 in every one of those pairs."
