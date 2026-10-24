#!/usr/bin/env bash
# classify-20260722-order.sh — recover the harvest order of the EIGHT unstamped
# seed-20260722 r=16 overworld references, from the data, and emit a DERIVED
# stamps tree that records how each verdict was reached.
#
# WHY THIS EXISTS
#
# `ci/figures.toml:ow-r16-admissible-matched` publishes 2,450 with
# `harvest = "rows"` over eight references. That field is an ASSERTION:
# `gen-wide-20260722-r16-ow-{a..h}` carry no `HARVEST-ORDER` file at all. Only
# `-i` (rows, zx) and `-rr-a` / `-rr-b` (rows_rev, zx_zrev) are stamped, and
# those three were stamped by `ci/order-level-retake.sh` when it promoted them
# out of a scratch directory.
#
# ORACLE rule 30 is exactly about this: an unstamped reference's order is
# *recoverable* — diff it against a stamped reference of each order and read
# which population it falls in — but it is not *known*, and the anchor of the
# ensemble has to be stamped. `-i` is that anchor. `ci/classify-12345-ab.sh` is
# the pattern this follows and its header carries the argument in full.
#
# WHAT IS DIFFERENT HERE, AND WHY IT IS A SCRIPT RATHER THAN A LOG
#
# The 12345 classifier prints a verdict and stops, because two references were
# in doubt and a human read the answer. Here EIGHT of the nine are unstamped,
# and the verdict has to be consumed by `ci/floor-census.py --emit`, which
# refuses an unstamped reference rather than silently labelling one. So this
# script writes a stamps tree — NOT into `oracle-work`, which is read-only and
# shared, but under `$OUT` — in which every derived stamp carries a
# `classified_from=` line holding the two distances that decided it and the
# controls they were read against. `floor-census.py` propagates that into the
# emitted table as `harvest_classified`, so a table built this way can never be
# mistaken for one built on references stamped at generation.
#
# A CONTROL IS INCLUDED AND IT IS NOT OPTIONAL — same argument as the 12345
# classifier. `i vs rr-a` is this seed's own cross-order distance and
# `rr-a vs rr-b` its own same-order one, measured in the same run, so each
# verdict is read against a scale this window produced rather than another
# window's ~5x rule of thumb.
#
# AND A NEGATIVE CONTROL, WHICH THE 12345 CLASSIFIER DOES NOT HAVE. A
# classifier that answers `rows` for every input it is ever shown has not
# classified anything, and every real input here is expected to answer `rows`
# — so the discriminator is never exercised by the data. `rr-a` and `rr-b` are
# therefore run through the same code path as unknowns: they are stamped
# `rows_rev` and must come back `rows_rev`. If they do not, the verdicts above
# them are worthless and this script exits 1 rather than emitting a tree.
#
# INPUT. The `<ref>-vs-<cand>.pos` matrix `ci/order-level-retake.sh pairs`
# leaves at `~/rp-logs-ladder/pos`, read for its `mismatches=` line only. With
# `CLASSIFY_FROM=differ` it re-runs `region-differ` instead, vanilla first,
# which needs the release binary and costs ~2.6 s per diff.
#
#   CLASSIFY_OUT   where the stamps tree and log go  (default ~/w22-logs)
#   CLASSIFY_POS   the position matrix               (default ~/rp-logs-ladder/pos)
#   CLASSIFY_FROM  matrix|differ                     (default matrix)
#
# usage: ci/classify-20260722-order.sh
# exit 0 every unknown classified and both controls held, 1 a control failed or
# an unknown was ambiguous, 2 inputs missing.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

OUT=${CLASSIFY_OUT:-$HOME/w22-logs}
POS=${CLASSIFY_POS:-$HOME/rp-logs-ladder/pos}
FROM=${CLASSIFY_FROM:-matrix}
REFDIR=$HOME/rustmc/oracle-work
PFX=gen-wide-20260722-r16-ow-
BBOX="48 48 80 80"
STAMPS=$OUT/stamps
LOG=$OUT/classify-20260722-order.log
UNKNOWN=${CLASSIFY_UNKNOWN:-"a b c d e f g h"}
ROWS_ANCHOR=i
RREV_ANCHOR=rr-a
mkdir -p "$OUT" || exit 2

# Distance between two references, vanilla first. The matrix is symmetric in
# content but not in file name, so both orders are tried before giving up.
d() {
    if [ "$FROM" = differ ]; then
        # shellcheck disable=SC2086
        ./target/release/region-differ diff "$REFDIR/$PFX$1" "$REFDIR/$PFX$2" \
            --bbox $BBOX 2>/dev/null | grep -oP 'mismatches=\K[0-9]+' | head -1
        return
    fi
    local f
    for f in "$POS/$1-vs-$2.pos" "$POS/$2-vs-$1.pos"; do
        [ -f "$f" ] && { grep -m1 -oP 'mismatches=\K[0-9]+' "$f"; return; }
    done
}

if [ "$FROM" = matrix ] && [ ! -d "$POS" ]; then
    echo "no position matrix at $POS — run ci/order-level-retake.sh pairs, or" >&2
    echo "set CLASSIFY_FROM=differ to re-diff from the references" >&2
    exit 2
fi

rc=0
{
  echo "=== harvest order of the seed-20260722 r=16 overworld references @ $(date -Is) ==="
  echo "--- what is actually stamped on disk ---"
  for r in a b c d e f g h i rr-a rr-b; do
      s=$(grep -m1 -oP 'gen_order=\K[a-z_]+' "$REFDIR/$PFX$r/HARVEST-ORDER" 2>/dev/null)
      printf '  %-6s %s\n' "$r" "${s:-UNSTAMPED}"
  done

  echo "--- controls: this window's own same-order and cross-order distances ---"
  CROSS=$(d "$ROWS_ANCHOR" "$RREV_ANCHOR")
  SAME=$(d rr-a rr-b)
  echo "  i    vs rr-a   (rows     vs rows_rev)  $CROSS   <- cross-order scale"
  echo "  rr-a vs rr-b   (rows_rev vs rows_rev)  $SAME   <- same-order scale"
  if [ -z "$CROSS" ] || [ -z "$SAME" ]; then
      echo "  CONTROL MISSING — cannot classify without a scale"; rc=1
  elif [ "$CROSS" -le "$((SAME * 2))" ]; then
      echo "  CONTROLS DO NOT SEPARATE — cross-order is not clear of same-order."
      echo "  Every verdict below is void. ORACLE rule 30."
      rc=1
  fi

  # The negative control. Run the stamped rows_rev references through the
  # unknown path; if the discriminator cannot answer rows_rev for a reference
  # that IS rows_rev, it is not a discriminator.
  echo "--- negative control: classify the stamped rows_rev references as unknowns ---"
  for u in rr-a rr-b; do
      TR=$(d "$ROWS_ANCHOR" "$u")
      TV=$(d "$RREV_ANCHOR" "$u")
      [ "$u" = "$RREV_ANCHOR" ] && TV=$(d rr-b "$u")
      if [ -z "$TR" ] || [ -z "$TV" ]; then v=NO-DATA
      elif [ "$TR" -lt "$TV" ]; then v=rows; else v=rows_rev; fi
      printf '  %-5s vs i(rows) %-8s vs rows_rev %-8s -> %-8s expected rows_rev' \
          "$u" "$TR" "$TV" "$v"
      if [ "$v" = rows_rev ]; then echo "  OK"; else echo "  FAILED"; rc=1; fi
  done

  echo "--- the unknowns ---"
  declare -A VERD DR DV
  for u in $UNKNOWN; do
      TR=$(d "$ROWS_ANCHOR" "$u")
      TV=$(d "$RREV_ANCHOR" "$u")
      if [ -z "$TR" ] || [ -z "$TV" ]; then
          printf '  %-5s NO DATA in the matrix\n' "$u"; rc=1; continue
      fi
      if [ "$TR" -lt "$TV" ]; then v=rows; else v=rows_rev; fi
      # Separation is quoted per reference. A verdict decided by a few hundred
      # blocks against populations 100k apart would be a coin toss wearing a
      # number, and it is the reading that would say so.
      SEP=$(( TV > TR ? TV - TR : TR - TV ))
      printf '  %-5s vs i(rows) %-8s vs rr-a(rows_rev) %-8s sep %-8s -> %s\n' \
          "$u" "$TR" "$TV" "$SEP" "$v"
      if [ "$SEP" -lt "$SAME" ]; then
          echo "         AMBIGUOUS: separation is under this window's own same-order"
          echo "         distance, so the two hypotheses are not distinguishable here."
          rc=1
      fi
      VERD[$u]=$v; DR[$u]=$TR; DV[$u]=$TV
  done

  if [ "$rc" != 0 ]; then
      echo "=== CLASSIFY RED — no stamps tree written ==="
  else
      rm -rf "$STAMPS"
      mkdir -p "$STAMPS"
      for u in $UNKNOWN; do
          m="$STAMPS/$PFX$u"; mkdir -p "$m"
          { echo "gen_order=${VERD[$u]}"
            echo "seed=20260722"
            echo "radius=16"
            echo "centre=64,64"
            echo "dimension=overworld"
            case "${VERD[$u]}" in
                rows)     echo "dump_order=zx" ;;
                rows_rev) echo "dump_order=zx_zrev" ;;
            esac
            echo "classified_from=ci/classify-20260722-order.sh $(date -I): vs ${ROWS_ANCHOR}(rows)=${DR[$u]} vs ${RREV_ANCHOR}(rows_rev)=${DV[$u]}, against this window's own controls cross=$CROSS same=$SAME. DERIVED, NOT STAMPED AT GENERATION — ORACLE rule 30."
          } > "$m/HARVEST-ORDER"
      done
      # The genuinely stamped ones are copied through unchanged, so one
      # --stamps path covers the whole pool and the difference between a real
      # stamp and a derived one stays visible inside the files.
      for r in i rr-a rr-b; do
          [ -f "$REFDIR/$PFX$r/HARVEST-ORDER" ] || continue
          mkdir -p "$STAMPS/$PFX$r"
          cp "$REFDIR/$PFX$r/HARVEST-ORDER" "$STAMPS/$PFX$r/HARVEST-ORDER"
      done
      echo "--- derived stamps tree ---"
      echo "  $STAMPS   ($(ls "$STAMPS" | wc -l) references, $(echo "$UNKNOWN" | wc -w) derived)"
      echo "=== CLASSIFY GREEN ==="
  fi
} > "$LOG" 2>&1
cat "$LOG"
exit $rc
