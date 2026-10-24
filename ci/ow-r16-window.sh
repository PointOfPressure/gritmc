#!/usr/bin/env bash
# The r=16 overworld window — the one the residual is quoted on.
#
# Seed 20260722, centre chunk (64,64), r=16, 1,089 chunks, VANILLA FIRST,
# against gen-wide-20260722-r16-ow-a with its -b twin as the mask.
#
# ALL FIGURES IN THIS HEADER ARE ON THE REGENERATED REFERENCE PAIR, re-taken
# 2026-08-09 at main 8e283f57. See the dated note at the bottom of this header
# for what happened to the previous pair and why none of its numbers survive.
#
#     regenerated-pair floor (N=2): 29,560 of 107,053,056
#     twin-masked residual 29,490 of 107,023,496
#     admissible charge 29,605 of 107,053,056, 99.972345%
#
# 🚫 THE TWIN-MASKED NUMBER THIS SCRIPT PRINTS IS NOT QUOTABLE. This window's
# vanilla-A-vs-vanilla-B floor is 29,560, which EXCEEDS that residual of
# 29,490, so ORACLE rule 6 makes the masked figure corroboration only: no
# exactness percentage derived from it may be published. Use it for DELTAS
# against the same reference, mask and denominator, which is what it is still
# good for. Note that rule 6 fires here for the same reason it did on the old
# pair and NOT by inheritance: both the floor and the residual fell when the
# references were regenerated, the floor by 5,971 and the residual by 437, and
# the verdict held because the floor fell further. It was checked, not carried.
#
# ✅ THE QUOTABLE FIGURE IS THE LAST STAGE, AND IT IS A DIFFERENT INSTRUMENT.
# `ci/admissible-residual.py` charges a position only when our block matches
# NEITHER reference. Nothing is masked, the denominator is the whole
# 107,053,056-block window, and rule 6's disqualifier has no premise because
# there is no blind spot. On main 8e283f57: admissible charge 29,605 of
# 107,053,056, 99.972345%, of which 115 sit inside what the twin mask would
# have hidden.
#
# ⚠️ AND READ THIS BEFORE QUOTING THE ADMISSIBLE FIGURE AS A WIN. 29,605
# against a floor of 29,560 means our disagreement with vanilla is 45 blocks
# above vanilla's disagreement with itself. The instrument is still valid —
# it excludes nothing, so there is no blind spot for rule 6 to be about — but
# this window is nearly exhausted, and a future change that "improves" the
# residual by a few hundred blocks is not distinguishable here from noise.
# A sharper claim needs a wider window or more references, not this one again.
#
# `ci/figures.toml` records all of it; `ci/check-figures.py` fails if a doc
# quotes a masked percentage or drifts from these numbers. Update the numbers
# in this header whenever this is re-run — the checker compares them against
# what the docs say.
#
# ⚠️ THE PREVIOUS REFERENCE PAIR IS GONE, AND WITH IT THE WHOLE PUBLISHED
# TRIO — 2026-08-09. `gen-wide-20260722-r16-ow-a` and `-b` did not survive
# the laptop-death restore. `find / -xdev -name 'gen-wide*'` matched nothing on
# the whole box, in any tree; the surviving `oracle-work/gen-20260722-*` server
# directories are an older and much smaller instrument (their overworld saves
# hold ONE structure start between them, against this window's nine). They were
# regenerated from scratch:
#
#     GEN_ORDER=rows ci/gen-vanilla-region.sh 20260722 16 \
#         ~/rustmc/oracle-work/gen-wide-20260722-r16-ow-{a,b} 2591{1,2} 64 64 overworld
#
# about 70 s each, 1089/1089 window chunks, and the regenerated -a holds
# vanilla's saved `ruined_portal` start at chunk (48,64) with the same
# `TP=768,42,1024` the four mined references gave. So the instrument is back and
# it is sound — but it is A DIFFERENT PAIR OF RUNS, and this project's own rule
# is that a floor belongs to the pair that produced it. On the destroyed pair
# this window's floor was 35,531 and the registered trio was 30,279 / 29,927 /
# 35,531 against a masked denominator of 107,017,525.
#     Was 29,964 on structures-trial, one commit before that.
# **None of those numbers is
# reproducible from this tree**, and as of 2026-08-09 none of them
# is registered as live: `ci/figures.toml` keeps them only in entries marked
# `pair = "destroyed"`. They were not wrong; their reference pair is gone.
# Deltas measured entirely within one pair are unaffected, which is why the
# ruined-portal result was taken as a before/after on the regenerated
# references rather than against the published baseline.
#
# **On the regenerated pair, the deltas that have been taken since.** Both are
# deltas within one pair, which is the one thing that survives a pair change
# intact; the twin-mask floor (N=2) on this pair is 29,560, and the destroyed
# pair's 35,531 is not comparable to it.
#
#     twin-mask floor (N=2)                     29,560
#     main 51fa7c62                  admissible 30,446 of 107,053,056
#     obsidian-lava-water-interface, off 35b3bde5:
#       before                       admissible 29,605   99.972345%   masked 29,490
#       after                        admissible 29,575   99.972374%   masked 29,460
#         -30, and 30 is exactly the charged set: 24 held as lava, 4 as water,
#         2 obsidian of our own at y=-56 where vanilla has lava.
#     aquifer-fluid-levels, off 4aa7fe88 — the beard reference box:
#       before                       admissible 29,575   99.972374%   masked 29,460
#       after                        admissible 20,244   99.981090%   masked 20,143
#         -9,331. `Structure.adjustBoundingBox` inflates a bearding start's
#         box by 12 and our reference filter did not, so every chunk that a
#         beard reached without holding a structure block was beardless.
#
# `figures-headline-swap` has since re-registered the published headline on this
# same regenerated pair, so the exactness in `ci/figures.toml` is live again;
# the two deltas above are still deltas and are not themselves registered.
#
# usage: .ow-window.sh <label>
set -uo pipefail
LABEL="${1:?need a label}"
OUT="$HOME/rp-ow-$LABEL"
LOG="$HOME/rp-logs/ow-$LABEL.log"
POSDIR="$HOME/rp-logs/ow-$LABEL-pos"
mkdir -p "$HOME/rp-logs" "$POSDIR"
REF_A="$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-a"
REF_B="$HOME/rustmc/oracle-work/gen-wide-20260722-r16-ow-b"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

# REFUSE RATHER THAN MEASURE NOTHING. Neither reference was guarded here until
# 2026-08-18: if `gen-wide-20260722-r16-ow-a` or its twin were absent this
# script ran anyway, and a missing reference is not a worse figure, it is no
# figure. The eleven directories under ~/w22-logs/stamps carry these exact
# names with no `.mca` at all, which is the precise shape of an argument that
# looks right and measures nothing.
for r in "$REF_A" "$REF_B"; do
    [ -d "$r" ] || { echo "no reference at $r — REFUSING." >&2; exit 2; }
    ls "$r"/*.mca >/dev/null 2>&1 || { echo "$r holds no region data — it is a stamp, not a reference. REFUSING." >&2; exit 2; }
done

# THE ORDER IS NOT MATCHED HERE, AND THAT IS DELIBERATE AND REGISTERED.
# `ci/figures.toml` carries `ow-r16-admissible-n2` with
# `dump_order = "x-major (UNMATCHED)"`, and this script is its artefact, so
# matching the order would stop it reproducing its own published number. What
# was missing was any statement of the fact at run time: the references' order
# is now resolved and PRINTED beside the order the dump will actually use, so
# the mismatch is visible in the log rather than only in the ledger.
# `ci/order-level-retake.sh` is the matched-order instrument.
REF_ORDER=$(./ci/harvest-order.sh "$REF_A" 2>&1 >/dev/null || true)
echo "REFERENCE ORDER: $REF_ORDER"
echo "DUMP ORDER:      whatever the binary defaults to — UNMATCHED, on purpose."
echo "                 The matched-order instrument is ci/order-level-retake.sh."

{
  echo "=== OVERWORLD r=16: $LABEL @ $(date -Is) ==="
  echo "--- harnesses live at launch (count them; do NOT read load average) ---"
  pgrep -af "[o]racle-runner|[t]2-gameplay|[h]andoff" || echo "(none)"
  echo "--- tree marker ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null || echo "(no marker: rsynced trees carry no .git)"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -2

  rm -rf "$OUT"
  echo "=== dump full seed 20260722 r=16 centre (64,64) RUSTMC_WORKERS=1 ==="
  time RUSTMC_WORKERS=1 ./target/release/worldgen-dump full 20260722 16 "$OUT" 64 64 2>&1 | tail -6

  echo "=== diff VANILLA FIRST, twin-masked (N=2) — DELTAS ONLY, NOT QUOTABLE ==="
  ./target/release/region-differ diff \
      "$REF_A" "$OUT/region" --twin "$REF_B" \
      --bbox 48 48 80 80 2>&1 | tail -20

  # The quotable arm. Two UNMASKED diffs, vanilla first in both, and their
  # positional intersection is `ours matches neither reference`. The position
  # files are kept beside the log: they are what any attribution starts from,
  # and re-deriving them costs another pair of runs.
  echo "=== unmasked vs each reference, for the admissible set ==="
  ./target/release/region-differ diff "$REF_A" "$OUT/region" \
      --bbox 48 48 80 80 --positions > "$POSDIR/a-vs-ours.pos" 2>&1
  ./target/release/region-differ diff "$REF_B" "$OUT/region" \
      --bbox 48 48 80 80 --positions > "$POSDIR/b-vs-ours.pos" 2>&1
  ./target/release/region-differ diff "$REF_A" "$REF_B" \
      --bbox 48 48 80 80 --positions > "$POSDIR/floor.pos" 2>&1
  grep -h '^DIFF' "$POSDIR"/*.pos

  echo "=== ADMISSIBLE (no mask) — THIS is the quotable one ==="
  python3 "$ROOT/ci/admissible-residual.py" \
      "$POSDIR/a-vs-ours.pos" "$POSDIR/b-vs-ours.pos" --floor "$POSDIR/floor.pos"
  echo "=== OVERWORLD DONE: $LABEL ==="
} > "$LOG" 2>&1
