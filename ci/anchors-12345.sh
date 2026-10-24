#!/usr/bin/env bash
# anchors-12345.sh — the 2n-3 anchor diffs `ci/floor-anchor.py` reads, for the
# seed-12345 r=16 overworld ensemble.
#
# WHY THIS IS NOT A STAGE OF ci/refs-12345-widen2.sh
#
# Two reasons, and the second is the one worth writing down.
#
# 1. It is a different resource. The reference boots take the corpus lock; this
#    takes nothing, needs no JVM, and excludes nobody.
# 2. **`ci/refs-12345-widen2.sh` was already running when this was written, and
#    bash reads a script incrementally from a file offset.** Editing a running
#    shell script makes the interpreter resume at the same byte offset in
#    different text. That is not a hypothetical: it is why this is a new file
#    rather than a fourth stage in a file that had room for one. If you are
#    tempted to add a stage to a script an agent is currently inside, don't.
#
# WHAT IT PRODUCES, AND WHY IT IS 2n-3 RATHER THAN n(n-1)
#
# `docs/DETERMINISM.md` §24.7. `region-differ diff c R --positions` prints BOTH
# blocks at every differing position, so one anchor recovers every reference's
# spelling everywhere:
#
#     val_R(p) = the R-column of c-vs-R at p, or c(p) if p is absent
#
# and therefore the admissible set, the floor and the charge. The anchor cannot
# be its own candidate — its self-diff is empty and it would score a silent
# zero — so a second anchor covers it. Two anchors, n-1 and n-2 diffs.
#
# The CANDIDATE costs nothing here: `<anchor>-vs-ours.pos` already exists from
# the measurement that produced the 1,329, and widening the ensemble does not
# require re-diffing our dump against anything. Verified: 1,329 and the
# registered 141/590/2,241 both fall out of 29 diffs on the sixteen-reference
# pool, checked candidate-by-candidate against the 240-diff matrix.
#
# usage: anchors-12345.sh [refs...]      (default: the full 40-label pool)
# exit 0 ok, 2 preconditions, 3 an unstamped or wrong-order reference.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${REFS_DEST:-$HOME/rustmc/oracle-work}"
PFX="gen-wide-12345-r16-ow"
OUT="${ANCHOR_OUT:-$HOME/rp-logs-floor12345-w2}"
SEED_POS="${SEED_POS:-$HOME/rp-iceberg912/12345/overworld/pos}"
BBOX="48 48 80 80"
G="$ROOT/target/release/region-differ"
A1="${ANCHOR:-c}"
A2="${ANCHOR2:-d}"

DEFAULT="c d e f g h i j k l m n o p q r s t u v w x y z aa ab ac ad ae af ag ah ai aj ak al am an ao ap"
REFS="${*:-$DEFAULT}"

[ -x "$G" ] || { echo "no $G — cargo build --release -p region-differ" >&2; exit 2; }
mkdir -p "$OUT/pos"

# Only score references that actually exist; the pool grows over several runs
# and a half-harvested pool is a normal intermediate state, not an error.
have=""
for r in $REFS; do [ -d "$DEST/$PFX-$r" ] && have="$have $r"; done
[ -n "$have" ] || { echo "no reference directories found under $DEST" >&2; exit 2; }

# AN ASSERTION IS NOT A STAMP — ORACLE rule 30. Widening ACROSS harvest orders
# made a floor worse by 1,293 (§18.4), so a mixed-order pool is a refusal here
# rather than a caveat in a later report.
for r in $have; do
    h="$DEST/$PFX-$r/HARVEST-ORDER"
    [ -f "$h" ] || { echo "REFUSED: $PFX-$r has no HARVEST-ORDER stamp" >&2; exit 3; }
    grep -qx 'gen_order=rows' "$h" || {
        echo "REFUSED: $PFX-$r is not gen_order=rows" >&2; exit 3; }
    grep -qx 'dump_order=zx' "$h" || {
        echo "REFUSED: $PFX-$r is not dump_order=zx" >&2; exit 3; }
done
echo "ANCHORS: $(echo $have | wc -w) references present, all rows/zx"

reused=0; made=0; t0=$(date +%s)
for a in "$A1" "$A2"; do
    for r in $have; do
        [ "$r" = "$a" ] && continue
        p="$OUT/pos/$a-vs-$r.pos"
        [ -s "$p" ] && continue
        # Reuse the existing matrix where it already holds this pair.
        if [ -s "$SEED_POS/$a-vs-$r.pos" ]; then
            ln -s "$SEED_POS/$a-vs-$r.pos" "$p"; reused=$((reused + 1)); continue
        fi
        "$G" diff "$DEST/$PFX-$a" "$DEST/$PFX-$r" --bbox $BBOX --positions \
            > "$p" 2>&1 || { echo "FAIL diff $a $r" >&2; exit 2; }
        made=$((made + 1))
    done
done
t1=$(date +%s)

# The candidate's own diff, symlinked from the measurement that produced the
# 1,329. Its anchor column is cross-checked against the reference diffs by
# ci/floor-anchor.py, which refuses on a mismatch rather than reporting a
# number taken against a different anchor run.
CPOS="${CHARGE_POS:-$HOME/rp-ice912-final/12345/overworld/pos}"
for a in "$A1" "$A2"; do
    s="$CPOS/$a-vs-ours.pos"; d="$OUT/pos/$a-vs-ours.pos"
    [ -s "$s" ] && [ ! -e "$d" ] && ln -s "$s" "$d"
done

echo "ANCHORS done: $made new diffs, $reused reused, $((t1 - t0))s"
if [ $made -gt 0 ]; then
    echo "ANCHORS cost: $(awk -v a=$((t1-t0)) -v b=$made 'BEGIN{printf "%.2f", a/b}') s/diff"
fi
echo "ANCHORS pos: $OUT/pos ($(ls "$OUT/pos" | wc -l) files)"
