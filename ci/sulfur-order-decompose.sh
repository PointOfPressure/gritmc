#!/usr/bin/env bash
# sulfur-order-decompose.sh — ORDER DECOMPOSITION FIRST, for the seed-12345
# r=16 overworld window's top per-block row.
#
# WHY THIS SCRIPT AND NOT A BYTECODE READ
#
# `ci/floor-census.py --adjudicate` ranks the window's admissible charge by
# `vanilla -> ours` pair and puts each row beside its own floor. On the
# 16-reference `rows` pool the top row is `sulfur -> cinnabar` at a ratio no
# other row comes near. A ratio that large invites a reading of the feature
# that writes the block, and on this project that is the wrong first move
# (ORACLE rule 9, and rule 29 on chunk order): the commonest cause of a large
# per-block row here is not where a block was placed but *when* the chunk
# holding it was decorated. `docs/DETERMINISM.md` §15/§16 measure that at
# 117,628 blocks on an innocent vanilla run for a single wrong knob.
#
# So this takes TWO dumps of ours that differ only in chunk visitation order —
# the default x-major raster and `zx`, which is the order the references' own
# `HARVEST-ORDER` stamps name — scores the row under each, and hands both
# position sets to `ci/order-pair-decompose.py` for the by-chunk, by-y-band and
# cross-order-vanilla decompositions.
#
# THE ORDER IS READ OFF THE REFERENCES AND NEVER ASSERTED. An unstamped
# reference is refused rather than guessed at (ORACLE rule 30: an assertion is
# not a stamp). A mixed pool is refused too — a floor belongs to the ensemble
# AND the harvest order that produced it.
#
# THE REFERENCE-VS-REFERENCE MATRIX IS BORROWED, NOT REGENERATED. 240 ordered
# pairs over the 16-reference pool already exist and cost the box an hour;
# `$REFPOS` is read-only here and this script writes only under `$OUTROOT`.
# The references themselves are never regenerated (AGENT-BRIEF, absolute).
#
# THE POSITIVE CONTROL THAT THE KNOB TURNED. Two orders that produce identical
# bytes are one order run twice, and a sweep that silently ran the default
# twice is indistinguishable from a pass (`ci/decorate-order-sweep.sh`'s
# header; `diag` writes `default`'s bytes while reporting its own digest). So
# both traversal digests are printed and the two region trees are compared,
# and a match on either is a refusal rather than a result.
#
# usage: RUSTMC_REMOTE_DIR=... ci/sulfur-order-decompose.sh <label>
# Runs on the Threadripper only. Takes no lock: it is dumps and diffs, not a
# corpus run and not a bench, and it measures no wall time.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
LABEL="${1:?need a label}"
SEED=12345
PAIR="${PAIR:-sulfur->cinnabar}"
ROWS="c d e f g h i j k l m n o p q r"
RREV="rr-a rr-b"
PFX="$HOME/rustmc/oracle-work/gen-wide-12345-r16-ow"
REFPOS="${REFPOS:-$HOME/rp-logs-floor12345/pos}"
OUTROOT="$HOME/rp-sulfur-$LABEL"
LOG="$HOME/rp-logs/sulfur-$LABEL.log"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
mkdir -p "$HOME/rp-logs" "$OUTROOT"

{
  echo "=== sulfur/cinnabar ORDER DECOMPOSITION: $LABEL @ $(date -Is) ==="
  echo "--- harnesses live at launch (count them; do NOT read load average) ---"
  pgrep -af "[o]racle-runner|[t]2-gameplay|[h]andoff" || echo "(none)"
  echo "--- tree marker ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null \
      || echo "(no marker: rsynced trees carry no .git)"

  # --- the order, read off the references ---------------------------------
  MATCHED=""
  for r in $ROWS; do
    ho="$PFX-$r/HARVEST-ORDER"
    [ -s "$ho" ] || { echo "$PFX-$r carries no HARVEST-ORDER stamp — REFUSING (ORACLE rule 29/30)"; exit 2; }
    d=$(grep -m1 -oP '^dump_order=\K.*' "$ho")
    g=$(grep -m1 -oP '^gen_order=\K.*' "$ho")
    printf '  %-6s gen_order=%-9s dump_order=%s\n' "$r" "$g" "$d"
    case "$d" in zx|zx_zrev) ;; *) echo "$PFX-$r stamps dump_order='$d' — REFUSING"; exit 2;; esac
    if [ -z "$MATCHED" ]; then MATCHED="$d"
    elif [ "$MATCHED" != "$d" ]; then
      echo "the pool mixes harvest orders ($MATCHED and $d) — REFUSING"; exit 2
    fi
  done
  for x in $RREV; do
    printf '  %-6s %s   (cross-order control only, never in the charge)\n' "$x" \
      "$(grep -m1 '^gen_order=' "$PFX-$x/HARVEST-ORDER")"
  done
  echo "  matched dump order: RUSTMC_DECORATE_ORDER=$MATCHED ; the other arm is the default raster"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -3 || exit 2

  # --- the two dumps -------------------------------------------------------
  for arm in default "$MATCHED"; do
    out="$OUTROOT/dump-$arm"
    rm -rf "$out"
    echo "=== dump seed $SEED r=16 centre (64,64) RUSTMC_WORKERS=1 order=$arm ==="
    RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER="$arm" \
        ./target/release/worldgen-dump full "$SEED" 16 "$out" 64 64 2>&1 | tail -4
  done

  # --- the positive control: the knob turned ------------------------------
  echo "=== POSITIVE CONTROL: the two arms are different worlds ==="
  if diff -rq "$OUTROOT/dump-default/region" "$OUTROOT/dump-$MATCHED/region" >/dev/null 2>&1; then
    echo "REFUSING: the two dumps are byte-identical — the order knob did not turn."
    echo "A sweep that ran one order twice is indistinguishable from a pass."
    exit 2
  fi
  diff -rq "$OUTROOT/dump-default/region" "$OUTROOT/dump-$MATCHED/region" 2>&1 | head -4
  echo "  the two region trees differ: the knob turned"

  # --- the diffs, VANILLA FIRST -------------------------------------------
  for arm in default "$MATCHED"; do
    pd="$OUTROOT/pos-$arm"
    rm -rf "$pd"; mkdir -p "$pd"
    # Borrow the reference-vs-reference matrix rather than re-taking it.
    for f in "$REFPOS"/*-vs-*.pos; do
      b=$(basename "$f")
      case "$b" in *-vs-ours.pos) continue;; esac
      ln -sfn "$f" "$pd/$b"
    done
    echo "=== diffs, VANILLA FIRST, order=$arm ==="
    for r in $ROWS; do
      ./target/release/region-differ diff "$PFX-$r" "$OUTROOT/dump-$arm/region" \
          --bbox 48 48 80 80 --positions > "$pd/$r-vs-ours.pos" 2>"$pd/$r.err"
      printf '  %-5s %s\n' "$r" "$(grep -m1 '^DIFF' "$pd/$r-vs-ours.pos" || cat "$pd/$r.err")"
    done
  done

  # --- adjudicate each arm against the per-block floor table --------------
  for arm in default "$MATCHED"; do
    echo "=== FLOOR-CENSUS ADJUDICATION, order=$arm, N=15 leave-one-out over 16 ==="
    python3 ./ci/floor-census.py "$OUTROOT/pos-$arm" \
        --refs "$(echo $ROWS | tr ' ' ',')" --adjudicate ours --top 24 \
        --label "seed-12345 r=16 ow, order=$arm" 2>&1 | tail -70
  done

  # --- the decomposition ---------------------------------------------------
  echo "=== ORDER DECOMPOSITION of \`$PAIR\` ==="
  python3 ./ci/order-pair-decompose.py \
      --a "$OUTROOT/pos-default" --b "$OUTROOT/pos-$MATCHED" \
      --refs "$(echo $ROWS | tr ' ' ',')" --cand ours \
      --label-a x-major --label-b "$MATCHED" \
      --pair "$PAIR" --top 24 --chunks 16 \
      --cross-dir "$REFPOS" --cross-anchor c \
      --cross-refs "$(echo $RREV | tr ' ' ',')" 2>&1

  echo "=== DONE: $LABEL ==="
} > "$LOG" 2>&1
echo "log: $LOG"
