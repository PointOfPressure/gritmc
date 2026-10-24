#!/usr/bin/env bash
# WHAT THE PIECE STREAM BUYS — the block-entity half, kept in its own script.
#
# Separate from `ci/jigsaw-stream-cost.sh` for a reason that is not tidiness.
# The side channel is only recording under `RUSTMC_RECORD_BLOCK_ENTITIES=1`,
# and the published BLOCK-STATE figures on this window are taken without it —
# so folding the two measurements into one set of dumps would either take the
# block-state numbers under a setting they were never taken under, or take the
# `bediff` under a setting where nothing is recorded and report the resulting
# no-change as a finding. `docs/DETERMINISM.md` §42.5's identity gate is what
# makes it sound to take them off different dumps: the same window generates
# byte-identical block states with recording on and off.
#
# VANILLA FIRST in every differ call. Two arms, one binary:
#
#     off      RUSTMC_JIGSAW_STREAM unset  — no jigsaw call site spends a draw,
#                                            so no container is recorded
#     stream   RUSTMC_JIGSAW_STREAM=1      — the draws, and nothing else
#
# THE POSITIVE CONTROL, and this script refuses without it. A `bediff` that
# did not move could mean the stream bought nothing OR that the channel was
# never recording, and those want opposite fixes. So both arms print the
# emitter's own `EmitStats` line, and the `off` arm must show recording ON with
# zero container payloads while the `stream` arm shows the same window with
# more — an `off` arm that recorded nothing at all is a channel that is off,
# not a stream that is missing, and is REFUSED.
#
# usage: ci/jigsaw-stream-bediff.sh <label>
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
LABEL="${1:?need a label}"
SEED=12345
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
# --- the positive control, as a function so it can be falsified ------------
#
# The four conditions live here rather than inline so `--selftest` can drive
# each of them with a crafted dump log and prove it refuses. A check that has
# never been shown to fail is this repo's central defect, and the check being
# fixed here IS a control that could not fail — so it does not get to land
# without one.
#
# Echoes a space-separated list of failures, empty when the arm is sound.
assess_arm() {                # assess_arm <arm> <dump-rc> <dump-log>
    local arm="$1" drc="$2" log="$3" bad="" spent
    [ "$drc" -eq 0 ] || bad="$bad arm=$arm:dump-rc=$drc"
    grep -qiE 'block entit|recorded|derived|orphan|incomplete|loot table and no seed' \
        "$log" 2>/dev/null || bad="$bad arm=$arm:no-emit-line"
    spent=$(awk '/^JIGSAW-STREAM/ {for(i=1;i<=NF;i++) if($i ~ /^draws=/) {split($i,a,"="); t+=a[2]}} \
                 END {print t+0}' "$log" 2>/dev/null)
    case "$arm" in
      off)    [ "${spent:-0}" -eq 0 ] || bad="$bad arm=off:spent-$spent-draws-with-knob-unset" ;;
      stream) [ "${spent:-0}" -gt 0 ] || bad="$bad arm=stream:spent-no-draws-knob-did-not-turn" ;;
    esac
    echo "$bad"
}

if [ "${1:-}" = "--selftest" ]; then
    # Drives assess_arm with crafted logs. Runs no dump, opens no reference and
    # needs no built binary, so the proof costs milliseconds.
    T=$(mktemp -d); rc=0
    emit='BLOCK ENTITIES emitted 5 (derived 1 recorded 4), dropped: orphaned 0 incomplete 0'
    mk() { printf '%s\n' "$emit" > "$T/l"; [ -n "$1" ] && printf 'JIGSAW-STREAM structure=x draws=%s\n' "$1" >> "$T/l"; }
    want() {   # want <label> <expect-empty|expect-bad> <verdict>
        local label="$1" mode="$2" v="$3"
        if { [ "$mode" = empty ] && [ -z "$v" ]; } || { [ "$mode" = bad ] && [ -n "$v" ]; }; then
            echo "  PASS  $label -> '${v:-<sound>}'"
        else
            echo "  FAIL  $label -> '${v:-<sound>}' (wanted $mode)"; rc=1
        fi
    }
    mk 0;   want "off, clean (rc 0, emit line, 0 draws)"        empty "$(assess_arm off 0 "$T/l")"
    mk 121; want "stream, clean (rc 0, emit line, 121 draws)"   empty "$(assess_arm stream 0 "$T/l")"
    mk 0;   want "off, dump crashed"                            bad   "$(assess_arm off 3 "$T/l")"
    mk 121; want "stream, dump crashed"                         bad   "$(assess_arm stream 3 "$T/l")"
    : > "$T/l"; printf 'JIGSAW-STREAM structure=x draws=121\n' >> "$T/l"
            want "stream, no emit line (channel off)"           bad   "$(assess_arm stream 0 "$T/l")"
    mk 7;   want "off, knob leaked (spent 7 draws)"             bad   "$(assess_arm off 0 "$T/l")"
    mk 0;   want "stream, knob did not turn (0 draws)"          bad   "$(assess_arm stream 0 "$T/l")"
    want "a missing log is not a sound arm"                     bad   "$(assess_arm stream 0 "$T/nope")"
    rm -rf "$T"
    [ $rc -eq 0 ] && echo "SELFTEST GREEN — every condition refuses, and only when it should" \
                  || echo "SELFTEST RED — the control cannot fail on some condition"
    exit $rc
fi

LOG="$HOME/rp-logs/jigsaw-bediff-$LABEL.log"
OUTDIR="$HOME/rp-logs/jigsaw-bediff-$LABEL-out"
mkdir -p "$HOME/rp-logs" "$OUTDIR"
PFX="$HOME/rustmc/oracle-work/gen-wide-12345-r16-ow"
REF="$PFX-c"
BBOX="--bbox 48 48 80 80"

[ -d "$REF" ] || { echo "missing reference $REF" >&2; exit 2; }
ho="$REF/HARVEST-ORDER"
[ -s "$ho" ] || { echo "$REF carries no HARVEST-ORDER stamp — REFUSING (ORACLE rule 29)" >&2; exit 2; }
MATCHED=$(grep -m1 -oP '^dump_order=\K.*' "$ho")
case "$MATCHED" in zx|zx_zrev) ;; *) echo "$REF stamps dump_order='$MATCHED' — REFUSING" >&2; exit 2;; esac

{
  echo "=== JIGSAW PIECE STREAM, BLOCK-ENTITY HALF: $LABEL @ $(date -Is) ==="
  echo "seed $SEED, centre (64,64), r=16, bbox 48 48 80 80, vanilla first ($REF)"
  echo "matched dump order off the reference's own stamp: RUSTMC_DECORATE_ORDER=$MATCHED"
  echo "--- tree marker ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null || echo "(rsynced tree)"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -2
  [ -x ./target/release/worldgen-dump ] || { echo "FAIL: no worldgen-dump binary"; exit 3; }

  CONTROL_BAD=""
  for arm in off stream; do
    case "$arm" in off) KNOB="" ;; stream) KNOB="1" ;; esac
    OUT="$HOME/rp-jigsaw-be-$LABEL-$arm"
    rm -rf "$OUT"
    echo "=== dump arm=$arm RUSTMC_JIGSAW_STREAM='${KNOB}' RECORD=1 ==="
    RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER="$MATCHED" \
      RUSTMC_RECORD_BLOCK_ENTITIES=1 RUSTMC_JIGSAW_STREAM="$KNOB" \
      RUSTMC_JIGSAW_TRACE=1 \
      ./target/release/worldgen-dump full "$SEED" 16 "$OUT" 64 64 \
        > "$OUTDIR/dump-$arm.txt" 2>&1
    drc=$?
    ARM_BAD="$(assess_arm "$arm" "$drc" "$OUTDIR/dump-$arm.txt")"
    CONTROL_BAD="$CONTROL_BAD$ARM_BAD"
    # The header above says this script REFUSES without its positive control.
    # Until 2026-08-18 it only PRINTED it: a run whose dumps both died on a
    # stale `data/vanilla` reported "(no emit line found)", "trace lines: 0"
    # and then ran `bediff` against directories that did not exist, printing
    # `FAIL: No such file or directory` twice and exiting 0. A control that
    # cannot refuse is not a control, so each condition now sets CONTROL_BAD
    # and the script exits non-zero at the end naming which one failed.
    if [ $drc -ne 0 ]; then
        echo "  DUMP FAILED rc=$drc — last lines:"
        tail -3 "$OUTDIR/dump-$arm.txt" | sed 's/^/    /'
    fi
    echo "--- the emitter's own account (recorded / derived / orphaned / incomplete) ---"
    grep -iE 'block entit|recorded|derived|orphan|incomplete|loot table and no seed' \
        "$OUTDIR/dump-$arm.txt" | head -12 || echo "  (no emit line found)"
    echo "--- draws spent on jigsaw piece streams ---"
    awk '/^JIGSAW-STREAM/ {for(i=1;i<=NF;i++) if($i ~ /^draws=/) {split($i,a,"="); t+=a[2]; n++}} \
         END {printf "  trace lines: %d, draws spent: %d\n", n+0, t+0}' "$OUTDIR/dump-$arm.txt"
    # The two arms must disagree in the direction the measurement claims, or a
    # zero in the bediff below means nothing: `off` spending draws would mean
    # the knob leaks, and `stream` spending none would mean it never turned.
    # Both are the "went quiet for the wrong reason" failure, and both are
    # judged by `assess_arm` above — see `--selftest` for the proof that each
    # of its conditions can actually refuse.
    [ -z "$ARM_BAD" ] || echo "  CONTROL FAILED for this arm:$ARM_BAD"
    awk '/^JIGSAW-STREAM/ {for(i=1;i<=NF;i++) if($i ~ /^structure=/) {split($i,a,"="); s[a[2]]++} \
         for(i=1;i<=NF;i++) if($i ~ /^draws=/) {split($i,b,"="); d[a[2]]+=b[2]}} \
         END {for (k in s) printf "  %-18s groups=%d draws=%d\n", k, s[k], d[k]}' \
      "$OUTDIR/dump-$arm.txt"
  done

  echo
  echo "############################################################"
  echo '# bediff, VANILLA FIRST. missing down is the figure; payload and'
  echo '# id staying 0 is the condition: a payload wrong on one key is a'
  echo "# worse answer than an absence, which is why the emitter refuses a"
  echo "# seedless table rather than writing one."
  echo "############################################################"
  for arm in off stream; do
    echo "=== bediff arm=$arm ==="
    ./target/release/region-differ bediff "$REF" "$HOME/rp-jigsaw-be-$LABEL-$arm/region" \
      $BBOX 2>&1 | tail -8
  done

  echo
  echo "=== BLOCK STATES ACROSS THE TWO RECORDING ARMS (should be identical: §42.5) ==="
  echo "# Not a substitute for ci/jigsaw-stream-cost.sh's figure — that one is"
  echo "# taken with recording OFF, which is how every published block-state"
  echo "# number on this window was taken. This is the same comparison under"
  echo "# recording, and the two agreeing is what makes the split sound."
  ./target/release/region-differ diff \
    "$HOME/rp-jigsaw-be-$LABEL-off/region" "$HOME/rp-jigsaw-be-$LABEL-stream/region" \
    $BBOX 2>&1 | tail -12

  if [ -n "$CONTROL_BAD" ]; then
      echo
      echo "########## POSITIVE CONTROL FAILED — NO FIGURE FROM THIS RUN ##########"
      echo "# failures:$CONTROL_BAD"
      echo "# Every bediff number above is unquotable. The header of this"
      echo "# script says the control is what makes a zero mean anything."
      echo "#######################################################################"
  fi
  echo "=== JIGSAW BLOCK-ENTITY HALF DONE: $LABEL ==="
  [ -z "$CONTROL_BAD" ]
} > "$LOG" 2>&1
rc=$?
echo "log: $LOG"
[ $rc -eq 0 ] || echo "POSITIVE CONTROL FAILED — see the tail of $LOG"
exit $rc
