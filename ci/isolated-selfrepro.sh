#!/usr/bin/env bash
# Is one binary's `RUSTMC_DECORATE_ISOLATED=1` arm a pure function of its
# inputs? `docs/DETERMINISM.md` §32.3 records ~200 blocks of movement in that
# arm across three builds of `worldgen-generate-once` against changes that
# touched only the other arm, and reports the arm byte-identical to itself on
# a sample of two. Two is the sample size at which a scheduling-dependent
# defect is most likely to look clean.
#
# This runs the same binary N times at one seed, one window and one model,
# cross-diffs every pair, and reports the spread. It takes no vanilla
# reference and makes no parity claim: an ours-vs-ours diff of one binary
# either is zero or the arm is not a function of the seed.
#
# The worker count is swept as well, because the isolated driver is a flat
# work queue over `worker_threads()` and a scheduling dependence shows up
# there before it shows up anywhere else.
#
# usage: ci/isolated-selfrepro.sh <seed> <radius> <cx> <cz> <model> <n> [workers...]
set -uo pipefail
SEED="${1:?seed}"; R="${2:?radius}"; CX="${3:?cx}"; CZ="${4:?cz}"
MODEL="${5:?isolated|extent|shared}"; N="${6:?runs}"
shift 6
WORKERS=("$@"); [ "${#WORKERS[@]}" -gt 0 ] || WORKERS=(0)
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT" || exit 2
OUT="$HOME/rp-selfrepro"; mkdir -p "$HOME/rp-logs"
LOG="$HOME/rp-logs/selfrepro-$MODEL-$SEED-$R-$CX-$CZ.log"
LO_X=$((CX - R)); HI_X=$((CX + R)); LO_Z=$((CZ - R)); HI_Z=$((CZ + R))
{
  echo "=== SELF-REPRODUCIBILITY $MODEL seed $SEED r=$R centre ($CX,$CZ) @ $(date -Is) ==="
  echo "=== tree $(git rev-parse --short HEAD) workers ${WORKERS[*]} runs $N ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -3
  D=./target/release/worldgen-dump; G=./target/release/region-differ
  names=()
  for W in "${WORKERS[@]}"; do
    for i in $(seq 1 "$N"); do
      tag="w${W}r${i}"; d="$OUT/$MODEL-$tag"; rm -rf "$d"
      case "$MODEL" in
        shared)   E=(env RUSTMC_DECORATE_ISOLATED=0) ;;
        isolated) E=(env RUSTMC_DECORATE_ISOLATED=1) ;;
        extent)   E=(env -u RUSTMC_DECORATE_ISOLATED) ;;
        *) echo "unknown model $MODEL"; exit 2 ;;
      esac
      [ "$W" = 0 ] || E+=("RUSTMC_WORKERS=$W")
      echo "--- dump $tag ---"
      "${E[@]}" "$D" full "$SEED" "$R" "$d" "$CX" "$CZ" 2>&1 | tail -2
      [ -d "$d/region" ] || { echo "$tag: dump failed"; exit 2; }
      names+=("$tag")
    done
  done
  echo "=== cross-diff every pair (ours vs ours; a zero is the only pass) ==="
  for a in "${names[@]}"; do
    for b in "${names[@]}"; do
      [[ "$a" < "$b" ]] || continue
      printf '%-10s %-10s ' "$a" "$b"
      "$G" diff "$OUT/$MODEL-$a/region" "$OUT/$MODEL-$b/region" \
          --bbox "$LO_X" "$LO_Z" "$HI_X" "$HI_Z" 2>&1 | grep -E '^DIFF'
    done
  done
  echo "=== SELF-REPRODUCIBILITY DONE ==="
} > "$LOG" 2>&1
echo "$LOG"
