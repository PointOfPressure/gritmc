#!/usr/bin/env bash
# floors-readjudicate.sh — reproduce every floor this campaign re-took, and
# carry the numbers it produced.
#
# `docs/PARITY.md` §2 gained a floor column on cf443588 and seven of its twelve
# floors were N=2 or absent. A floor from two runs is a range rather than a
# measurement, and it was adjudicating the project's headline claims. This is
# the re-take, and this header is the artefact `ci/figures.toml` points at.
#
# EVERY FLOOR BELOW IS UNCLAMPED. The `-clamp-` references on this box were
# harvested with `-Dmax.bg.threads=1`, which does NOT clamp the harvest:
# `Util.makeExecutor` passes `maxAllowedExecutorThreads()` as a `ForkJoinPool`
# **parallelism** rather than a thread cap, so the pool spawns compensation
# workers regardless. Mixing them would mix a condition, not tighten one.
#
# EVERY FLOOR BELOW IS QUOTED WITH ITS DEFINITION, because §2's column is two
# quantities that move in opposite directions as N grows —
# docs/floors/FLOOR-IS-TWO-QUANTITIES.md:
#
#   union_N     positions where N vanilla runs are not unanimous. Null for a
#               TWIN-MASKED residual. Non-decreasing in N.
#   innocent_N  what the instrument charges a known-innocent vanilla run. Null
#               for an ADMISSIBLE residual. Non-increasing in N.
#
# ==========================================================================
# RESULTS — 2026-08-18, branch floors-n-campaign off origin/main cf443588
# ==========================================================================
#
# overworld 31415926 r=16, centre 64,64, y full           (§2 row 6, was 🚫)
#   references     eight, floors-ow31415926r16-{a..h}, gen_order=rows
#   pairwise       34,179..54,175 median 45,842 over 28 pairs
#   union_N 113,938 at N=7
#   innocent_N MAX 8,859 at N=7
#   admissible residual 11,861..12,542 at N=7, CLEARS
#   -> 99.988284% of 107,053,056.  Was: 39,143 against 49,570 (N=2), UNDER FLOOR.
#
# the End 20260722 r=7, centre 83,62, y 0..255            (§2 row 7, was `not measured`)
#   references     five, van-end-20260722-c8362{,-b} and -open-{a,b,c}
#   pairwise 0..0 over 10 pairs; union_N = innocent_N = 0 at every N
#   admissible residual 0 at every N, EXACT floor 0
#   -> 100.000000% of 14,745,600.  The r=9 windows this row also claims are
#      defined nowhere in this repository, in any commit.
#
# nether 20260722 r=7, centre 100,100, y 0..127           (§2 row 10, was `not measured`)
#   references     eight, van-nether-20260722-c100100-open-{a,b} + floors-{c..h}
#   innocent_N MAX 38 at N=7; union_N 217; pairwise 22..164 median 74
#   admissible residual 477 at N=7, CLEARS
#   -> 99.993530% of 7,372,800, clearing by 12.6x.
#
# nether 31415926 r=7, centre 64,64, y 0..127             (§2 row 11, was 🚫)
#   references     eight, van-nether-31415926-c6464-open-{a,b,c} + floors-{d..h}
#   pairwise       104,754..151,425 median 127,206 over 28 pairs
#   admissible residual 9,560..10,728 at N=7 against innocent 22,377, under floor
#   -> verdict UNCHANGED. The published 95,240 sits BELOW the whole pairwise
#      distribution, so it was the most favourable pair rather than a typical one.
#
# overworld 20260722 r=7, centre 64,64, y full            (§2 row 1, was `not measured`)
#   references     eight, floors-ow20260722c6464r7-{a..h}, gen_order=rows
#   pairwise       6,032..17,129 median 11,344 over 28 pairs
#   union_N 30,078 at N=7
#   innocent_N MAX 3,862 at N=7
#   admissible residual 459..495 at N=7, under floor
#   -> the published 1,079 sits ~10x UNDER its own instrument's floor. The
#      order-invariant core of 0 stands; the raw residual is withdrawn.
#
# overworld 31415926 r=5, centre 64,64, y full            (§2 row 2, was `not measured`)
#   references     eight, floors-ow31415926c6464r5-{a..h}, gen_order=rows
#   pairwise       284..2,190 median 1,222 over 28 pairs
#   union_N 3,759 at N=7
#   innocent_N 31..293..527 at N=7
#   admissible residual 6 at N=7, under floor
#   -> converges to exactly the 6 §2 publishes, from an independent route, and
#      6 is 0.01x its floor. The number is right and carries no information.
#
# THE TWO FLOORS ON WINDOWS THIS CAMPAIGN DID NOT RE-TAKE, measured so the
# dashboard's rows can be compared at all. Same references, same window, one
# run; the gap is a property of the DEFINITION and not of the window:
#
#   overworld 12345 r=16, six rows references c..h
#     union_5 37,248 against innocent_5 4,835      7.7x
#   overworld 20260722 r=16, nine rows references a..i
#     union_8 86,240 against innocent_8 5,263     16.4x
#   nether 20260722 r=16, six rows references a..f
#     union_5 766,490 against innocent_5 111,015   6.9x
#
# The innocent arms reproduce ci/figures.toml's ow-r16-12345-floor-n5
# (2,577..4,835 median 3,387) and ow-r16-20260722-floor-n8 (2,202..5,263
# median 3,281) TO THE BLOCK from an independent implementation, which is also
# the reference-rot check those two ensembles needed.
#
# THE RATE DIAGNOSTIC. Per reference, near the top of each ladder:
#   ow 31415926      residual  -6%   floor -24%   ratio 0.93 -> 1.42   crossed
#   nether 20260722  residual  -1%   floor -50%   ratio 3.0  -> 12.6   clear throughout
#   nether 31415926  residual -22%   floor -27%   ratio 0.42 -> 0.48   never crosses
# A residual made of the floor's own positions falls at the floor's rate. Three
# points on the curve say whether references can ever rescue a row, which is
# cheaper than finding out by buying them.
# ==========================================================================
#
# usage: ci/floors-readjudicate.sh [window ...]   (default: all)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
L=${FLOORS_REFS:-$HOME/rustmc-floors/oracle-work}
Q=${ORACLE_REFS:-$HOME/rustmc/oracle-work}
D=${FLOORS_DUMPS:-$HOME/rp-floors}
C=${FLOORS_CACHE:-$HOME/grit/logs}
F=ci/floor-both-definitions.py

want() { [ $# -eq 0 ] && return 0; case " $* " in *" $CUR "*) return 0;; esac; return 1; }
set -- "$@"

CUR=ow31415926r16
if want "$@"; then
  R=""; for l in a b c d e f g h; do R="$R,$l=$L/floors-ow31415926r16-$l"; done
  python3 $F --refs "${R#,}" --bbox 48 48 80 80 --cache "$C/fbd-31415926-r16" \
      --ours "$D/ow31415926r16/region" --label "overworld 31415926 r=16, N=7"
fi

CUR=endr7
if want "$@"; then
  python3 $F --refs "p1=$Q/van-end-20260722-c8362,p2=$Q/van-end-20260722-c8362-b,oa=$Q/van-end-20260722-c8362-open-a,ob=$Q/van-end-20260722-c8362-open-b,oc=$Q/van-end-20260722-c8362-open-c" \
      --bbox 76 55 90 69 --ybox 0 255 --cache "$C/fbd-end-c8362" \
      --ours "$D/end20260722c8362/region" --label "the End 20260722 r=7 centre 83,62, N=4"
fi

CUR=nether100
if want "$@"; then
  R="oa=$Q/van-nether-20260722-c100100-open-a,ob=$Q/van-nether-20260722-c100100-open-b"
  for l in c d e f g h; do R="$R,$l=$L/floors-neth20260722c100100-$l"; done
  python3 $F --refs "$R" --bbox 93 93 107 107 --ybox 0 127 --cache "$C/fbd-neth20260722-c100100" \
      --ours "$D/neth20260722c100100/region" --label "nether 20260722 chunks 93..107, N=7"
fi

CUR=nether31
if want "$@"; then
  R="oa=$Q/van-nether-31415926-c6464-open-a,ob=$Q/van-nether-31415926-c6464-open-b,oc=$Q/van-nether-31415926-c6464-open-c"
  for l in d e f g h; do R="$R,$l=$L/floors-neth31415926c6464-$l"; done
  python3 $F --refs "$R" --bbox 57 57 71 71 --ybox 0 127 --cache "$C/fbd-neth31415926-c6464" \
      --ours "$D/neth31415926c6464/region" --label "nether 31415926 chunks 57..71, N=7"
fi

CUR=ow20260722r7
if want "$@"; then
  R=""; for l in a b c d e f g h; do R="$R,$l=$L/floors-ow20260722c6464r7-$l"; done
  python3 $F --refs "${R#,}" --bbox 57 57 71 71 --cache "$C/fbd-ow20260722-c6464r7" \
      --ours "$D/ow20260722c6464r7/region" --label "overworld 20260722 chunks 57..71, N=7"
fi

CUR=ow31415926r5
if want "$@"; then
  R=""; for l in a b c d e f g h; do R="$R,$l=$L/floors-ow31415926c6464r5-$l"; done
  python3 $F --refs "${R#,}" --bbox 59 59 69 69 --cache "$C/fbd-ow31415926-c6464r5" \
      --ours "$D/ow31415926c6464r5/region" --label "overworld 31415926 chunks 59..69, N=7"
fi
