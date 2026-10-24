#!/usr/bin/env bash
# The r=16 overworld window on A SECOND SEED — 12345, centre chunk (64,64),
# r=16, 33x33 = 1,089 chunks, VANILLA FIRST throughout.
#
# WHY THIS SCRIPT EXISTS, AND WHY IT IS A COPY RATHER THAN A FLAG ON THE OTHER
# ONE. Every parity figure this project publishes comes from one seed. Months
# of repairs were aimed at one window's residual and nobody could say how much
# of the headline was parity and how much was fitting to one seed's terrain.
# `ci/ow-r16-window.sh` is the published artefact for 20260722 and
# `ci/check-figures.py` reads the numbers out of its header, so parameterising
# it would put two seeds' figures in one file and make ARTEFACT-DRIFT unable
# to tell which one drifted. One artefact, one window, one set of numbers.
#
# ---------------------------------------------------------------------------
# 2026-08-10 — WHAT CHANGED, AND WHY THE FIRST VERSION COULD NOT HAVE BEEN
# QUOTED
#
# The first version of this file scored our dump against
# `gen-wide-12345-r16-ow-{a,b}` and took a twin floor from that pair. It was
# written before ORACLE rule 29 and it has two defects that no amount of
# re-running would have fixed:
#
#   1. **It never set the chunk order.** Our dump walks chunks x-major
#      (`Area::chunks()`); `ci/gen-vanilla-region.sh GEN_ORDER=rows` harvests
#      z-major. On the 20260722 window the *same instrument* charges an
#      unmodified vanilla run **117,628 blocks** for nothing but that mismatch,
#      against 3,804 for being a different run of the right order. A level taken
#      that way is an accurate measurement of a condition nobody set.
#
#   2. **Neither `-a` nor `-b` carries a `HARVEST-ORDER` stamp.** They predate
#      the file `ci/gen-vanilla-region.sh` now writes. Their order is asserted
#      in a script header and recorded nowhere in the artefact — so there was
#      nothing to match *to*, even if step 1 had been done.
#
# So this script now (a) reads the order off the references' own stamps and
# refuses if they have none, (b) takes the **innocent floor before it takes
# ours**, and (c) reports a level only when the charge clears that floor. `-a`
# and `-b` are not deleted, not regenerated and not written to; they are still
# sound inputs to a *delta*, which does not depend on the order being stated,
# and the N=2 twin arms at the end of this script are kept for exactly that.
#
# THE FLOOR COMES FIRST AND IT IS NEVER SUBTRACTED. `floor(N)` is the charge
# this instrument lays on a *known-innocent* candidate: a genuine unmodified
# vanilla run substituted for our dump and adjudicated against N of the others.
# It is a positive control, printed beside the charge. A charge *below* its
# floor is not a better result — it means the instrument has stopped
# separating us from vanilla, and the honest word for that number is
# corroboration. (`docs/DETERMINISM.md` §12 subtracted a twin-mask width from
# an unmasked charge, called the difference "45 blocks of headroom", and that
# category error cost a brief.)
#
# Every figure this prints carries its N, its harvest order and
# p_hide(N) = 1 - 0.05^(1/N): N=3 hides 63.2%, N=6 hides 39.3%, N=10 hides
# 25.9%.
# ---------------------------------------------------------------------------
#
# The geometry is identical to the 20260722 window on purpose: same radius,
# same centre, same 1,089 chunks, same `GEN_ORDER=rows` on the vanilla side
# and `RUSTMC_WORKERS=1` on ours. The two seeds are therefore comparable and
# the comparison is the point.
#
# THE ENSEMBLE. `ci/refs-12345-ensemble.sh` generated it on 2026-08-10 at
# ~57 s per reference — the cost that was believed prohibitive for months and
# had only ever been unattempted:
#
#     c d e f g h    GEN_ORDER=rows      dump_order=zx        ports 25931..25936
#     rr-a rr-b      GEN_ORDER=rows_rev  dump_order=zx_zrev   ports 25941..25942
#
# The seed's PREVIOUS pair did not survive the 2026-08-07 disk failure, and
# with it went the floor of 12,774 and the residual of 77,067 that
# `docs/WIDE-WINDOW-R16-ALL-DIMENSIONS.md` recorded. Those two numbers are not
# converted, carried or compared against what this script prints: a floor
# belongs to the pair of runs that produced it, and since §15 to its harvest
# order too.
#
# MEASURED-BLOCK-BEGIN
#
# ✅ 2026-08-10, branch `worldgen-spruce-leaves` off
# `worldgen-12345-residual-census` 1979b091, Threadripper. THE SPRUCE CANOPY
# LOOP BOUND, ported from `SpruceFoliagePlacer` bytecode. Same window, same
# sixteen stamped `rows` references c..r, same matched dump order `zx`;
# `ci/order-level-retake.sh all` at `LADDER_SEED=12345` over its own 342-diff
# matrix. The before-figures were reproduced to the block from the base
# branch's own matrix before anything was changed.
#
#   arm              dump  harvest         N     charge     exactness
#   A match          zx    rows           16      4,111    99.996160%
#   A match          zx    rows           15      4,111    99.996160%        141       590     2,241   18.1%
#   A match          zx    rows            6      4,155    99.996119%
#
# DELTA, exact to the block against a fixed reference set: 7,490 -> 4,111 at
# N=16, a fall of 3,379 and 45.1% of the whole window residual. 3,399 charged
# positions fixed, 20 newly charged. `spruce_leaves` MISS 1,353 -> 0 exactly,
# SURPLUS 1,021 -> 5 against a floor of 426 and now under it; `spruce_log`
# 65 -> 1 and 120 -> 0. Chunk (49,48) 1,008 -> 2, (48,48) 675 -> 20, (48,50)
# 365 -> 0, all against a per-chunk innocent floor of 0. THE CONTROL: chunk
# (76,58), the iceberg lump, reads 612 before and 612 after.
#
# POISON CONTROL re-run after the fix and it STILL FIRES, which is what says
# the path is live rather than merely quiet: clean 118 mismatches over the
# five NW chunks, poisoned (`getTreeHeight` base + 1) 7,285, worst chunk
# (49,48) 983. `ci/nw-corner-poison-control.sh`.
#
# CENSUS after the fix, `ci/residual-census.py` at N=6: half the charge is in 7 of 1,089 chunks
# (99 charged, was 111), worst chunk (76,58) 612 against a per-chunk floor of
# 52. The two outermost rings now hold 1,425 against an innocent 1,878 and are
# INSIDE their floor, where before they held 4,735 and were CLEAR — the edge
# concentration WAS the spruce defect. The aggregate 4,155 is itself below the
# N=5 innocent floor of 928..5,926, so only the per-block and per-chunk cuts
# are usable on this window now.
#
# ✅ 2026-08-10, branch `worldgen-12345-residual-census` off
# `worldgen-blue-ice-iceberg` acb99d72, Threadripper. THE RESIDUAL BELOW,
# CENSUSED POSITIONALLY. Same window, same eight references, same matched dump
# order; `ci/order-level-retake.sh all` at `LADDER_SEED=12345` reproduced
# 7,940 / 4,835 / 13,645 / 5,378 to the block from its own dumps and its own
# 72-diff matrix before anything was concluded. `ci/residual-census.py`.
#
#     half the charge is in 8 of 1,089 chunks; interior (ring >= 2) 3,026 is inside its floor
#
#     BY CHUNK   135 of 1,089 charged; (49,48) 1,008 floor 0; (48,48) 675 floor 0
#     BY Y BAND  y 48..79 holds 5,536 of 7,871 (70%); y 0..47 is under its floor
#     BY RING    outermost 3,666 (46.6%); interior >= 2 is 3,026 against 1,729..3,243
#
# THE RING READING IS NOT A MARGIN ARTEFACT AND IS NOT A MARGIN FINDING. A
# dump at r=18 — a 4-chunk margin instead of 2 — is byte-identical to the r=16
# one over this window, 0 mismatches, so ORACLE rule 14's convergence had
# already happened; and every `region-differ diff` against a reference prints
# `chunks=1089 (a=1370 b=1369)`, so vanilla generated its own neighbour ring
# and the harvested edge chunks were decorated with full context. The NW
# corner simply holds a snowy taiga. DO NOT RE-TAKE EITHER CONTROL.
#
# ORDER STABILITY, both sides of the same fix: the x-major census is 13,576
# over 396 chunks, but (49,48) 1,006, (48,48) 675, (76,58) 612 and (78,51) 270
# are unchanged. Every named lump is order-invariant to within two blocks and
# the whole 5,705-block order cost is a diffuse tail across 261 extra chunks.
#
# AND THE 439-BLOCK BLUE-ICE SURPLUS IS REFUTED. The 42-block clearance was an
# unmatched N: an innocent candidate has one five-reference subset and ours has
# six, and 439 was the maximum over them. Over all six ours is 288..439 against
# the innocents' 272..397. What survives is a per-chunk EXCURSION, scored over
# every one of the 262 chunks holding blue ice rather than the four the data
# pointed at:
#
#     per-chunk excursion  111 in 4 chunks  (innocent 0..59)
#
# AND RE-TAKEN AGAIN ON THE 16-REFERENCE ENSEMBLE (c..r, leave-one-out N=15,
# ci/floor-tables/ow-r16-12345-rows-n15.toml on branch floor-12345-ensemble
# 292de967), against the rebased tree 719ecda2 whose whole-window charge is
# 7,675 at N=5 and 7,606 at N=6 — 265 blocks below acb99d72's, from main's
# fluid-stage fix and not from anything here:
#
#     placed-surplus  ours 67   innocent 0..0  (N=15, sixteen references)
#     miss            ours 44   innocent 0..100
#
# THE 439 MUST NOT BE PAIRED WITH THAT 0, OR WITH THE 175 THE SAME TABLE GIVES
# AT N=5. Those are outside-UNION floors and 439 is an outside-UNANIMOUS
# charge. On the same sixteen references the matched pairs are 439 against
# 272..416 (1.06x, not an adjudication) and 67 against 0..0 (unbounded).
#
# 1.9x at N=8; at N=6, the 439's own ensemble, the floor widens to 0..100 and
# the same 111 clears by 11 blocks. No single one of the four chunks clears a
# single-chunk floor on its own (+44 against an innocent worst of -45).
# `docs/DETERMINISM.md` §17.2.
#
# ✅ 2026-08-10, branch `worldgen-blue-ice-iceberg` off `window-12345-instrument`
# 27914806, Threadripper. `minecraft:iceberg` PORTED; SAME WINDOW, SAME EIGHT
# REFERENCES, SAME MATCHED DUMP ORDER as the reading below it — only the
# generator changed, and both readings were taken in one session by one script.
#
#     admissible charge, N=5, harvest rows, dump zx:
#         7,940 of 107,053,056 — 99.992583%
#     innocent-vanilla floor at the same N over the same ensemble:
#         2,577..4,835, median 3,387, n=6   — IDENTICAL TO THE READING BELOW
#     separation: median charge 7,913 - median floor 3,387 = +4,526
#     p_hide(5) = 45.1%
#
# THE FLOOR DID NOT MOVE, and that is the load-bearing part: a floor belongs to
# the reference pair that produced it, this change did not touch the
# references, so 61,348 blocks of improvement against an unchanged floor is the
# generator and not a luckier subset. Registered `quotable = true` as
# `ci/figures.toml:ow-r16-12345-admissible-iceberg`.
#
# READ THE MARGIN AND NOT ONLY THE PERCENTAGE. This clears the worst innocent
# reading by 3,105 blocks where the reading below cleared it by 64,453 — 1.6x
# its floor against 14x. The window is close to spent as an adjudicator and a
# bigger percentage here means LESS resolving power, not more.
#
# ORDER WENT FROM 6.7% TO 41.8% OF THE RESIDUAL WITHOUT MOVING IN BLOCKS:
#
#     dump x-major (the condition unset)   13,645      99.987254%
#     dump zx (matched)                     7,940      99.992583%
#     difference                            5,705   (was 4,936)
#     order-invariant core                  5,378   (was 65,119)
#
# THE POSITIONAL DELTA, against a set that does not move when the ensemble does:
#
#     twin-stable (vanilla-c AND vanilla-d both hold blue_ice)   17,566
#     twin-stable 17,566: hit 17,522, miss 44 (innocent floor 0..151)
#     surplus outside the six-way unanimous set  439  (innocent 272..397)
#
#     block         vanilla-c   vanilla-d       ours     gap      was
#     packed_ice      947,800     947,821    947,787      -13   -26,753
#     blue_ice         17,718      17,741     17,741      +23   -17,718
#     snow_block       14,727      14,727     14,719       -8    -1,682
#     ice             104,854     104,852    105,888   +1,034    +2,960
#
# The miss count is INSIDE its innocent floor, so it is registered
# `quotable = false, disqualifier = "below floor"` — the absence is closed and
# 44 is not an accuracy claim. The 439-block surplus is 42 above the worst
# innocent reading and IS an item. `ci/iceberg-poison-control.sh` fires on both
# of its poisons (1,241 and 1,796 positions moved), so this window really is
# routed through the ported feature. `docs/DETERMINISM.md` §17.1.
#
# ---------------------------------------------------------------------------
# ⚠ SUPERSEDED BY THE BLOCK ABOVE, KEPT AS THE READING THE WINDOW WAS STOOD UP
# ON. Not withdrawn and not wrong: it is what this instrument read before
# `minecraft:iceberg` existed in the tree, and the defect it names below is
# what the port closed.
#
# ✅ THIS WINDOW ADJUDICATES, AND IT IS THE ONLY r=16 OVERWORLD WINDOW THAT
# DOES. Measured 2026-08-10 on branch `window-12345-instrument`, Threadripper.
# Seed 12345, r=16, centre chunk (64,64), 1,089 chunks, 107,053,056 blocks,
# bbox 48 48 80 80, nothing masked, vanilla first, dump order MATCHED to the
# references' own `HARVEST-ORDER` from the first measurement.
#
#     admissible charge, N=5, harvest rows, dump zx:
#         69,288 of 107,053,056 — 99.935277%
#     innocent-vanilla floor at the same N over the same ensemble:
#         2,577..4,835, median 3,387, n=6
#     separation: median charge 69,251 - median floor 3,387 = +65,864
#     p_hide(5) = 45.1%
#
# The charge is FOURTEEN TIMES the worst reading the same instrument gives a
# candidate that is genuinely unmodified vanilla. That is the opposite of the
# 20260722 window, where the matched charge of 2,450 sits inside a floor of
# 2,202..5,263 and no exactness may be published at all. Registered
# `quotable = true` as `ci/figures.toml:ow-r16-12345-admissible-matched`.
#
# WHAT THE ORDER WAS WORTH HERE, WHICH IS NOT WHAT IT WAS WORTH ON 20260722.
# Same ensemble, same N=5, the only difference being our dump's chunk order:
#
#     dump x-major (the condition unset)   74,224      99.930666%
#     dump zx      (the condition set)     69,288      99.935277%
#
# 4,936 blocks, 6.7% of the residual. On the 20260722 window the same
# comparison is 14,270 -> 2,450, which is 83%. **Chunk order is not a
# constant fraction of a residual and must not be estimated from another
# window's.** Here it is a correction; there it was the entire number.
#
# THE ORDER-INVARIANT CORE IS 65,119 — charged under BOTH dump orders against
# all eight references. On 20260722 that core is ~850.
#
# THE TWIN FLOOR, for reference, over the 15 pairs of the six `rows`
# references: 13,317..17,779, median 16,270. A floor belongs to the pair that
# produced it; none of these is "the" floor of this window.
#
# WHAT THE RESIDUAL IS MADE OF — ranked `vanilla -> ours` pairs at N=6, each
# against the WORST charge any innocent vanilla run takes for the same pair:
#
#     packed_ice -> water[level=0]     20,947   floor    14   CLEAR
#     packed_ice -> air                16,752   floor     0   CLEAR
#     blue_ice   -> packed_ice          8,189   floor    60   CLEAR
#     blue_ice   -> water[level=0]      7,892   floor    91   CLEAR
#     air        -> packed_ice          2,653   floor    13   CLEAR
#     snow_block -> air                 1,682   floor     0   CLEAR
#     andesite   -> granite                92   floor   704   FLOOR
#     diorite    -> andesite               87   floor   792   FLOOR
#
# The stone-variant pairs are under their own floors and are vanilla
# disagreeing with itself; opening an investigation on them would be measuring
# noise. Everything above them is an item.
#
# AND THE POSITIONAL CENSUS NAMES IT, which a confusion table cannot:
#
#     block         vanilla-c   vanilla-d      ours        gap
#     packed_ice      947,800     947,821     921,068    -26,753
#     ice             104,854     104,852     107,814     +2,960
#     blue_ice         17,718      17,741           0    -17,718
#     snow_block       14,727      14,727      13,045     -1,682
#
# **We write no `blue_ice` at all.** Two unmodified vanilla runs agree on its
# count to within 23 blocks, so this is not run-to-run noise and it is not an
# artefact of the instrument: it is a block this port never places. It
# accounts for 17,332 of the ranked charge above on its own.
#
# CLOSED 2026-08-10 — see the top of this block. `blue_ice` and `packed_ice`
# turned out to be ONE defect, not the two this block implies: a single
# missing `minecraft:iceberg` arm, wired up twice in the extract.
#
# MEASURED-BLOCK-END
#
# usage: ci/ow-r16-window-12345.sh <label>
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
LABEL="${1:?need a label}"
SEED=12345
OUT="$HOME/rp-ow-12345-$LABEL"
LOG="$HOME/rp-logs/ow-12345-$LABEL.log"
POSDIR="$HOME/rp-logs/ow-12345-$LABEL-pos"
mkdir -p "$HOME/rp-logs" "$POSDIR"
REFROOT="$HOME/rustmc/oracle-work"
PFX="$REFROOT/gen-wide-12345-r16-ow"
# The stamped ensemble. `a` and `b` are deliberately NOT here: unstamped.
ROWS="c d e f g h"
RREV="rr-a rr-b"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

for r in $ROWS $RREV; do
  [ -d "$PFX-$r" ] || { echo "missing reference $PFX-$r" >&2; exit 2; }
done

# THE MATCHED DUMP ORDER, READ OFF THE REFERENCES AND NEVER ASSERTED.
# Refuse rather than guess: an unstamped reference is one whose order is
# stated nowhere, and matching to a guess is the defect this rewrite exists to
# remove, not a smaller version of it.
MATCHED=""
for r in $ROWS; do
  ho="$PFX-$r/HARVEST-ORDER"
  [ -s "$ho" ] || { echo "$PFX-$r carries no HARVEST-ORDER stamp — REFUSING (ORACLE rule 29)" >&2; exit 2; }
  d=$(grep -m1 -oP '^dump_order=\K.*' "$ho")
  case "$d" in zx|zx_zrev) ;; *) echo "$PFX-$r stamps dump_order='$d' — REFUSING" >&2; exit 2;; esac
  if [ -z "$MATCHED" ]; then MATCHED="$d"
  elif [ "$MATCHED" != "$d" ]; then
    echo "the rows ensemble mixes harvest orders ($MATCHED and $d) — REFUSING" >&2; exit 2
  fi
done

{
  echo "=== OVERWORLD r=16 SEED $SEED: $LABEL @ $(date -Is) ==="
  echo "--- harnesses live at launch (count them; do NOT read load average) ---"
  pgrep -af "[o]racle-runner|[t]2-gameplay|[h]andoff" || echo "(none)"
  echo "--- tree marker ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null || echo "(no marker: rsynced trees carry no .git)"

  echo "--- ensemble, with the stamp each reference carries ---"
  for r in $ROWS $RREV; do
    printf '  %-6s %s  %s\n' "$r" \
      "$(grep -m1 '^gen_order=' "$PFX-$r/HARVEST-ORDER")" \
      "$(grep -m1 '^dump_order=' "$PFX-$r/HARVEST-ORDER")"
  done
  for r in a b; do
    [ -d "$PFX-$r" ] && printf '  %-6s %s  (EXCLUDED from every level below)\n' "$r" \
      "$(head -1 "$PFX-$r/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED)"
  done
  echo "  matched dump order for our side: RUSTMC_DECORATE_ORDER=$MATCHED"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -2

  # THE LADDER. Floors and charges at every N, both arms, from the shared
  # driver rather than a copy of it: the coverage refusal, the full ordered
  # pair matrix and the byte-distinctness check on the two dumps are lessons
  # already paid for there and a copy would re-earn the bugs, not the lessons.
  echo "=== LADDER: innocent floor at every N, then our charge beside it ==="
  LADDER_SEED=$SEED LADDER_ROWS="$ROWS" LADDER_RREV="$RREV" \
    LADDER_ANCHOR="$(echo "$ROWS" | awk '{print $1}')" \
    ./ci/order-level-retake.sh all 2>&1

  # ---------------------------------------------------------------------
  # THE N=2 TWIN ARMS, KEPT FOR DELTAS ONLY.
  # Against a FIXED reference set the admissible charge is a deterministic
  # function of our output, so a one-block change in it is real and
  # measurable even when the level it sits at is inside the floor. That is
  # what these are for. They are NOT a level: N=2 on this window is the
  # regime where the innocent floor alone spans thousands of blocks.
  # ---------------------------------------------------------------------
  REF_A="$PFX-c"
  REF_B="$PFX-d"
  rm -rf "$OUT"
  echo "=== dump full seed $SEED r=16 centre (64,64) RUSTMC_WORKERS=1 order=$MATCHED ==="
  RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER="$MATCHED" \
      ./target/release/worldgen-dump full "$SEED" 16 "$OUT" 64 64 2>&1 | tail -6

  # The label says HARVEST here, and $MATCHED is a DUMP order — the exact
  # confusion this whole artefact exists to prevent. Both sides of a twin floor
  # are vanilla, so no dump order enters it at all; what it is a property of is
  # the two references' harvest order, which is `rows` for c and d.
  echo "=== TWIN FLOOR (N=2, harvest rows, no dump order involved): vanilla c vs vanilla d, no mask ==="
  ./target/release/region-differ diff "$REF_A" "$REF_B" \
      --bbox 48 48 80 80 2>&1 | tail -20

  echo "=== unmasked vs each reference, for the admissible set (delta use) ==="
  ./target/release/region-differ diff "$REF_A" "$OUT/region" \
      --bbox 48 48 80 80 --positions > "$POSDIR/a-vs-ours.pos" 2>&1
  ./target/release/region-differ diff "$REF_B" "$OUT/region" \
      --bbox 48 48 80 80 --positions > "$POSDIR/b-vs-ours.pos" 2>&1
  ./target/release/region-differ diff "$REF_A" "$REF_B" \
      --bbox 48 48 80 80 --positions > "$POSDIR/floor.pos" 2>&1
  grep -h '^DIFF' "$POSDIR"/*.pos

  echo "=== ADMISSIBLE at N=2 — a DELTA BASELINE, not a level ==="
  python3 "$ROOT/ci/admissible-residual.py" \
      "$POSDIR/a-vs-ours.pos" "$POSDIR/b-vs-ours.pos" --floor "$POSDIR/floor.pos"
  echo "=== OVERWORLD SEED $SEED DONE: $LABEL ==="
} > "$LOG" 2>&1
echo "log: $LOG"
