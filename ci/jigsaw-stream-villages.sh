#!/usr/bin/env bash
# THE ARM THE SEED-12345 WINDOW CANNOT EXERCISE.
#
# `ci/jigsaw-stream-cost.sh` measures the stream on seed 12345, centre (64,64),
# r=16 — and its own trace says what it did NOT measure: every one of the 52
# structure groups it found is `trial_chambers`, and every group has `starts=1`.
# So on that window:
#
#   * no VILLAGE start is reached, and the village pools are the only ones in
#     vanilla that hold a `minecraft:feature` element — the single path by which
#     the piece stream can move a block state in a jigsaw structure;
#   * no structure has two starts in one chunk, so the `LongOpenHashSet` start
#     order the `faithful` arm exists to test reorders nothing.
#
# **A gate a window cannot exercise proves nothing, and a zero taken there must
# not be reported as though it covered the village path.** This script is the
# other half. Seed 20260722 is the window whose two savanna villages carry the
# 22 unported `feature_pool_element`s `caveats()` names, so it reaches both.
#
# IT PUBLISHES NO LEVEL AND NEEDS NO REFERENCE, DELIBERATELY. Every arm here is
# OURS AGAINST OURS off one binary, so the comparison is exact, has no floor to
# clear and cannot be mistaken for a parity figure — which matters because the
# seed-20260722 r=16 overworld window carries no quotable exactness figure at
# all (`docs/DETERMINISM.md` §15, §16). A delta on it is sound; a level is not,
# and this script cannot accidentally produce one because no vanilla reference
# is opened.
#
# usage: ci/jigsaw-stream-villages.sh <label> [radius]
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
LABEL="${1:?need a label}"
R="${2:-16}"
SEED=20260722
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
# --- the positive control, as a function so it can be falsified ------------
#
# The header calls the trace a POSITIVE CONTROL and the body only PRINTED it:
# a run where the knob never turned printed "NO TRACE LINES — the knob did not
# turn" and then compared three arms that were all the same arm, reporting
# `mismatches 0` as a finding. Three zeros from one arm is the "went quiet for
# the wrong reason" answer, and it is indistinguishable from the real one in
# the output.
#
# Echoes a space-separated list of failures, empty when the arm is sound.
assess_arm() {                # assess_arm <arm> <dump-rc> <dump-log>
    local arm="$1" drc="$2" log="$3" bad="" spent
    [ "$drc" -eq 0 ] || bad="$bad arm=$arm:dump-rc=$drc"
    spent=$(awk '/^JIGSAW-STREAM/ {for(i=1;i<=NF;i++) if($i ~ /^draws=/) {split($i,a,"="); t+=a[2]}} \
                 END {print t+0}' "$log" 2>/dev/null)
    case "$arm" in
      off)              [ "${spent:-0}" -eq 0 ] || bad="$bad arm=off:spent-$spent-draws-with-knob-unset" ;;
      stream|faithful)  [ "${spent:-0}" -gt 0 ] || bad="$bad arm=$arm:spent-no-draws-knob-did-not-turn" ;;
    esac
    echo "$bad"
}

if [ "${1:-}" = "--selftest" ]; then
    T=$(mktemp -d); rc=0
    mk() { : > "$T/l"; [ -n "$1" ] && printf 'JIGSAW-STREAM structure=x draws=%s\n' "$1" >> "$T/l"; }
    want() {
        local label="$1" mode="$2" v="$3"
        if { [ "$mode" = empty ] && [ -z "$v" ]; } || { [ "$mode" = bad ] && [ -n "$v" ]; }; then
            echo "  PASS  $label -> '${v:-<sound>}'"
        else
            echo "  FAIL  $label -> '${v:-<sound>}' (wanted $mode)"; rc=1
        fi
    }
    mk 0;   want "off, clean (no draws with the knob unset)"    empty "$(assess_arm off 0 "$T/l")"
    mk 292; want "stream, clean (292 draws)"                    empty "$(assess_arm stream 0 "$T/l")"
    mk 292; want "faithful, clean (292 draws)"                  empty "$(assess_arm faithful 0 "$T/l")"
    mk 292; want "off, knob leaked"                             bad   "$(assess_arm off 0 "$T/l")"
    mk 0;   want "stream, knob did not turn"                    bad   "$(assess_arm stream 0 "$T/l")"
    mk 0;   want "faithful, knob did not turn"                  bad   "$(assess_arm faithful 0 "$T/l")"
    mk 292; want "stream, dump crashed"                         bad   "$(assess_arm stream 3 "$T/l")"
    want "a missing log is not a sound arm"                     bad   "$(assess_arm faithful 0 "$T/nope")"
    rm -rf "$T"
    [ $rc -eq 0 ] && echo "SELFTEST GREEN — every condition refuses, and only when it should" \
                  || echo "SELFTEST RED — the control cannot fail on some condition"
    exit $rc
fi

LOG="$HOME/rp-logs/jigsaw-villages-$LABEL.log"
OUTDIR="$HOME/rp-logs/jigsaw-villages-$LABEL-out"
mkdir -p "$HOME/rp-logs" "$OUTDIR"
# The bbox is the window itself, r chunks either side of centre chunk (64,64).
LO=$((64 - R)); HI=$((64 + R))
BBOX="--bbox $LO $LO $HI $HI"

{
  echo "=== JIGSAW PIECE STREAM, THE VILLAGE ARM: $LABEL @ $(date -Is) ==="
  echo "seed $SEED, centre chunk (64,64), r=$R, bbox $LO $LO $HI $HI"
  echo "OURS AGAINST OURS ONLY — no reference is opened, so no level is produced."
  echo "--- tree marker ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null || echo "(rsynced tree)"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -2
  [ -x ./target/release/worldgen-dump ] || { echo "FAIL: no worldgen-dump binary"; exit 3; }

  # No `RUSTMC_DECORATE_ORDER` is set and that is correct rather than an
  # omission: both arms are our own dumps, so they share whatever order the
  # default is, and a chunk order only has to be MATCHED against a reference.
  # There is no reference here.
  CONTROL_BAD=""
  for arm in off stream faithful; do
    case "$arm" in off) KNOB="" ;; stream) KNOB="1" ;; faithful) KNOB="2" ;; esac
    OUT="$HOME/rp-jigsaw-vil-$LABEL-$arm"
    rm -rf "$OUT"
    echo "=== dump arm=$arm RUSTMC_JIGSAW_STREAM='${KNOB}' ==="
    RUSTMC_WORKERS=1 RUSTMC_JIGSAW_STREAM="$KNOB" RUSTMC_JIGSAW_TRACE=1 \
      ./target/release/worldgen-dump full "$SEED" "$R" "$OUT" 64 64 \
        > "$OUTDIR/dump-$arm.txt" 2>&1
    drc=$?
    ARM_BAD="$(assess_arm "$arm" "$drc" "$OUTDIR/dump-$arm.txt")"
    CONTROL_BAD="$CONTROL_BAD$ARM_BAD"
    [ -z "$ARM_BAD" ] || {
        echo "  CONTROL FAILED for this arm:$ARM_BAD"
        [ "$drc" -eq 0 ] || tail -3 "$OUTDIR/dump-$arm.txt" | sed 's/^/    /'
    }
    echo "--- POSITIVE CONTROL: which structures spent draws, and how many starts each group held ---"
    awk '/^JIGSAW-STREAM/ {
           for (i=1;i<=NF;i++) {
             if ($i ~ /^structure=/) {split($i,a,"="); s=a[2]}
             if ($i ~ /^starts=/)    {split($i,b,"="); st=b[2]}
             if ($i ~ /^draws=/)     {split($i,c,"="); d=c[2]}
           }
           for (i=1;i<=NF;i++) {
             if ($i ~ /^refusals=/)        {split($i,e,"="); rf=e[2]}
             if ($i ~ /^downstream_draws=/){split($i,f,"="); dd=f[2]}
           }
           groups[s]++; draws[s]+=d; refus[s]+=rf; down[s]+=dd; if (st>1) multi[s]++
         }
         END {
           if (length(groups)==0) print "  NO TRACE LINES — the knob did not turn";
           for (k in groups)
             printf "  %-18s groups=%d draws=%d groups-with-2+-starts=%d refusals=%d downstream_draws=%d\n", \
                    k, groups[k], draws[k], multi[k]+0, refus[k]+0, down[k]+0
         }' "$OUTDIR/dump-$arm.txt"
    echo "--- DOWNSTREAM DRAWS: container draws spent AFTER a refused feature"
    echo "    pool element, i.e. on a stream position vanilla has already"
    echo "    moved past. These are wrong LootTableSeeds, not absent ones,"
    echo "    and no block-state instrument in this repo can see them."
    awk '/^JIGSAW-STREAM/ {
           for (i=1;i<=NF;i++) {
             if ($i ~ /^downstream_draws=/){split($i,f,"="); t+=f[2]}
             if ($i ~ /^draws=/)           {split($i,g,"="); a+=g[2]}
           }
         }
         END {printf "  total draws=%d  of which downstream of a refusal=%d\n", a+0, t+0}' \
      "$OUTDIR/dump-$arm.txt"
    echo "--- refusals naming a feature pool element (these are the desync sites) ---"
    grep -c 'feature pool element' "$OUTDIR/dump-$arm.txt" | sed 's/^/  count: /'
    grep -m3 'feature pool element' "$OUTDIR/dump-$arm.txt" || echo "  (none)"
  done

  echo
  echo "############################################################"
  echo "# OURS AGAINST OURS. Exact quantities: no reference, no floor."
  echo "############################################################"
  for pair in "off stream" "off faithful" "stream faithful"; do
    set -- $pair
    echo "=== $1 vs $2 ==="
    ./target/release/region-differ diff \
      "$HOME/rp-jigsaw-vil-$LABEL-$1/region" "$HOME/rp-jigsaw-vil-$LABEL-$2/region" \
      $BBOX 2>&1 | tail -12
  done

  if [ -n "$CONTROL_BAD" ]; then
      echo
      echo "########## POSITIVE CONTROL FAILED — NO FIGURE FROM THIS RUN ##########"
      echo "# failures:$CONTROL_BAD"
      echo "# Every arm-against-arm zero above is unquotable: three arms that"
      echo "# were all the same arm agree trivially, and that is exactly what"
      echo "# a knob that never turned looks like from here."
      echo "#######################################################################"
  fi
  echo "=== JIGSAW VILLAGE ARM DONE: $LABEL ==="
  [ -z "$CONTROL_BAD" ]
} > "$LOG" 2>&1
rc=$?
echo "log: $LOG"
[ $rc -eq 0 ] || echo "POSITIVE CONTROL FAILED — see the tail of $LOG"
exit $rc
