#!/usr/bin/env bash
# nether-anchor-floor.sh — the r=16 nether floor, taken with the estimator the
# project's own headline figure uses, from diffs that already exist.
#
# # WHY THIS EXISTS
#
# The r=16 nether carried this verdict in `ci/figures.toml`, on
# `nether-r16-n6-matched`:
#
#     the floor exceeds the residual by 7x at N=6 and the gap widens with N —
#     1,172,835 against 87,483 at N=8 — so the r=16 nether carries no level
#     claim and there is no N at which it would: the floor grows faster than
#     the residual falls.
#
# **That last clause is a property of the estimator, not of the nether**, and
# this script is the refutation. There are two floor families in this tree and
# they face opposite ways in N:
#
#   * the **union** family — `ci/floor-ensemble.sh`, `ci/floor-ladder.py` —
#     which accumulates `union over j>=2 of pos(ref1, refj)`. A union can only
#     grow with N. Every published r=16 nether floor came from it: 317,725 at
#     N=2, 668,532 and 766,490 at N=6, 1,172,835 at N=8.
#   * the **innocent leave-one-out** family — `ci/window-resolving-power.py`,
#     `ci/floor-census.py`, `ci/floor-anchor.py` — which charges a held-out
#     vanilla run against the set of spellings the others are known to produce,
#     `|{ p : val_X(p) not in V_S(p) }|`. Adding a reference can only ADD
#     spellings to `V_S(p)`, so this one is monotonically non-increasing in N.
#     Every published seed-12345 overworld floor came from it: 4,835 at N=5,
#     2,241 at N=15, 362 at N=39.
#
# So "the nether floor grows with N while the overworld floor shrinks" was
# never a fact about the two dimensions. It was two different instruments
# reported in one column, and the 766,490-against-362 gap is estimator, not
# world. `docs/DETERMINISM.md` §46.
# world. `docs/DETERMINISM.md` §33.
#
# # WHAT IT COSTS: NOTHING. NO REFERENCE IS GENERATED AND NO DIFF IS RUN.
#
# The `n(n-1)` ordered pair matrix for this window already exists — 72 `.pos`
# files over references a..f (harvested `rows`), rr-a and rr-b (harvested
# `rows_rev`) and the candidates `ours` and `zx`, left by the nether ladder.
# `ci/floor-anchor.py` needs `2n-3` of them. This is a re-read, which is why it
# was affordable to do at all while five branches contended for the corpus lock.
#
# # WHAT IT READS — measured 2026-08-12, Threadripper
#
#     FLOOR-ANCHOR nether-r16-20260722-rows
#       references            : 6, anchors a / b
#       window                : 35,684,352 blocks
#       diffs consumed        : 9 (the pair matrix would be 30)
#       VERIFIED              : all 6 candidates agree with the n(n-1) matrix
#
#           n   N       min   median      MAX
#           6   5    37,951   80,241  111,015
#
#     zx    N=5 over the six 5-subsets: 85,181..92,226 median 89,104
#     ours  N=5 over the six 5-subsets: 125,495..134,523 median 130,724
#
# The floor is **37,951..111,015, median 80,241, at N=5** against the union
# family's 766,490 at N=6 — 6.9x lower. Our matched-order charge is **89,104**,
# which is 0.80x of the floor's maximum.
#
# **The window still carries no level claim, and that has not changed. What
# changed is why, and what it would take.** Under the union estimator the
# nether was 9.7x below its floor and the ledger said no N would fix it. Under
# the estimator the project actually quotes, it is 0.80x below — which is
# exactly where seed 12345 sat at N=15 (1,329 against a max of 2,241, 0.59x)
# before more references took it to 3.53x at N=39. `docs/DETERMINISM.md` §24 is
# the ladder and §24.3 is its costing.
#
# Note the two candidates. `zx` is the order-matched dump and `ours` is the
# unmatched one, and the 89,104-against-130,724 gap between them is ORACLE
# rule 29 firing on this window too: 32% of the unmatched charge is chunk
# order. Score `zx`, never `ours`.
#
# # THE CONTROL THAT MAKES THIS BELIEVABLE
#
# `--verify-against` recomputes every candidate's floor by the direct
# `n(n-1)`-matrix intersection and refuses unless the anchor construction
# agrees **for every candidate**, not merely on the summary statistics. It is
# left on by default here. A min, a median and a max can all three be right
# while the per-candidate values are wrong, and this window has only six
# candidates, so the check is nearly free.
#
# Usage:  ci/nether-anchor-floor.sh [POSDIR]
set -euo pipefail

POSDIR="${1:-/home/sayer/rp-logs-ladder/nether/pos}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ ! -d "$POSDIR" ]; then
    echo "nether-anchor-floor.sh: no pair matrix at $POSDIR" >&2
    echo "  This script re-reads an EXISTING matrix and generates nothing." >&2
    echo "  The 72 .pos files were left by the nether ladder; if they are gone," >&2
    echo "  regenerating them is 8 references and 72 diffs, not a re-read." >&2
    exit 2
fi

echo "== the floor: six rows references, leave-one-out, N=5 =="
python3 "$HERE/ci/floor-anchor.py" \
    --pos "$POSDIR" --refs a,b,c,d,e,f --anchor a --anchor2 b --ns 6 \
    --label nether-r16-20260722-rows --verify-against "$POSDIR"

echo
echo "== our charge on the same window, same instrument, same N =="
echo "   (a candidate is not a member of the reference set, so it is scored"
echo "    against every 5-subset to face the leave-one-out floor fairly)"
POSDIR="$POSDIR" python3 - "$HERE" <<'PY'
import importlib.util, itertools, os, statistics, sys

here = sys.argv[1]
spec = importlib.util.spec_from_file_location("fa", os.path.join(here, "ci", "floor-anchor.py"))
fa = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fa)

POS = os.environ["POSDIR"]
REFS = ["a", "b", "c", "d", "e", "f"]
at, per, blocks = fa.load_anchor(POS, "a", ["b", "c", "d", "e", "f", "zx", "ours"])
print(f"   window {blocks:,} blocks; anchor a")
for cand in ("zx", "ours"):
    full = fa.floor_anchor(at, per, per[cand], REFS)
    subs = [fa.floor_anchor(at, per, per[cand], list(s))
            for s in itertools.combinations(REFS, 5)]
    print(f"   {cand:5s} N=6 charge {full:8,d}   "
          f"N=5 over the six 5-subsets: {min(subs):,}..{max(subs):,} "
          f"median {int(statistics.median(subs)):,}")
PY
