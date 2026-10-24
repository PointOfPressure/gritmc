#!/usr/bin/env bash
# refs-12345-widen.sh — widen the seed-12345 r=16 overworld ensemble, so the
# window's floor can be re-taken at a larger N.
#
# WHY, AND WHY IT IS WORTH THE BOX
#
# `ci/refs-12345-ensemble.sh` built six `rows` references (c..h) and two
# `rows_rev` (rr-a, rr-b). The floor those six give at N=5 is
# **2,577..4,835** (`ow-r16-12345-floor-n5`), and after `minecraft:iceberg`
# landed, the window's residual is 7,940 — a margin of 1.6x where it used to
# be 14x. The item this script exists for is buying the margin back, and it
# was NOT started until the floor had been censused, because two of the three
# available moves would have been wasted:
#
#   * `ci/floor-census.py` shows the floor is **per-run noise, not a fixed
#     set**: the union of the six innocent charges is 21,650 positions and
#     exactly 2 of them are charged twice. There is no hard set to pin, so
#     "find and pin the nondeterminism" has no target — but noise that is
#     independent per run is exactly what more references thin, so THIS move
#     is the one with a mechanism behind it.
#   * the same census shows the floor's N-dependence measured within the
#     existing pool: max 17,779 / 9,896 / 7,074 / 5,891 / 4,835 at N=1..5,
#     which is 17,779 * N^-0.81 to within a few per cent. It predicts
#     **~1,990 at N=15**. That is a falsifiable prediction, made before the
#     box was spent, and re-taking the floor on the widened pool tests it.
#
# THE EFFECT THAT IS NOT ASSUMED. A larger ensemble can WIDEN the innocent
# range rather than narrow it, and on this very window it demonstrably does
# when the pool mixes harvest orders: pooling the six `rows` with the two
# `rows_rev` moves the floor from 2,577..4,835 to 1,476..6,128 — the median
# falls but **the max rises**, because a minority-order candidate is charged
# for its order rather than for being a different run. So this script adds
# `rows` references ONLY. A floor belongs to the ensemble AND the harvest
# order that produced it, and widening across orders is not widening.
#
# GEOMETRY, identical to `ci/refs-12345-ensemble.sh` on purpose: seed 12345,
# r=16, centre chunk (64,64), 33x33 = 1,089 chunks, bbox 48 48 80 80,
# overworld, GEN_ORDER=rows (dump_order=zx).
#
# PORTS. 25951..25960, disjoint from every port that ensemble script used
# (25931..25936, 25941..25942) and from 25921/25922 and the 25701 default.
# `ci/gen-vanilla-region.sh` derives its scratch server directory from the
# port, so one port is used once, ever.
#
# LABELS. i j k l m n o p q r. Ten new `rows` references take the pool from 6
# to 16 and the leave-one-out N from 5 to 15, which takes p_hide(N) =
# 1 - 0.05^(1/N) from 45.1% to 18.1%.
#
# THE BOX, AND HOW LONG THIS HOLDS IT
#
# Each reference is a real vanilla boot and generation run, ~57-70 s, so the
# generation stage is **about twelve minutes** and it takes the corpus lock as
# role `run` for exactly that long. That is the sanctioned reason for a
# non-corpus job to sit inside the corpus lock and AGENT-BRIEF asks the window
# to be declared in the script header, which is this paragraph. A real merge
# gate declares as role `gate` and jumps ahead; that is intended.
#
# The `pairs` stage is NOT under the lock and does not need to be: a
# `region-differ diff --positions` over this window is **2.6 s** (measured
# 2026-08-10, Threadripper, and it reproduces the existing `c-vs-d.pos` byte
# for byte), so the full 240-diff ordered matrix is about ten minutes of
# ordinary CPU. It is idempotent by refusal like everything else here — an
# existing non-empty `.pos` is never regenerated — and it links in the 30
# reference-against-reference diffs that already exist rather than re-taking
# them.
#
# IDEMPOTENT BY REFUSAL. A reference directory that already exists is skipped
# and never regenerated. AGENT-BRIEF: never regenerate or overwrite an
# existing vanilla reference; new reference, new directory. The unstamped
# `a`/`b` pair is not touched, read, or referred to.
#
# usage: ci/refs-12345-widen.sh [refs|pairs|all]
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

STAGE=${1:-all}
DEST=${REFS_DEST:-$HOME/rustmc/oracle-work}
OUT=${WIDEN_OUT:-$HOME/rp-logs-floor12345}
LOG=${REFS_LOG:-$OUT/refs-12345-widen.log}
PFX=gen-wide-12345-r16-ow
BBOX="48 48 80 80"
G=./target/release/region-differ

# The existing matrix, reference-against-reference only. Its `ours`/`zx`
# columns belong to whichever tree took them and are not reused here.
SEED_POS=${SEED_POS:-$HOME/rp-logs-ladder/12345/overworld/pos}

OLD=${WIDEN_OLD:-c d e f g h}
NEW=${WIDEN_NEW:-i j k l m n o p q r}
ALL="$OLD $NEW"

mkdir -p "$OUT/pos" "$(dirname "$LOG")" "$DEST"

# label:port, ten of them, one port used once ever.
SPEC="i:25951
j:25952
k:25953
l:25954
m:25955
n:25956
o:25957
p:25958
q:25959
r:25960"

rc_all=0

if [ "$STAGE" = all ] || [ "$STAGE" = refs ]; then
    # shellcheck disable=SC1090
    . "$ROOT/ci/corpus-lock.sh"
    RUSTMC_CORPUS_LOCK_ROLE=run corpus_lock_acquire
    {
        echo "=== widen seed-12345 r=16 ow ensemble @ $(date -Is) ==="
        for line in $SPEC; do
            lab=${line%%:*}; port=${line##*:}
            case " $NEW " in *" $lab "*) ;; *) continue ;; esac
            out="$DEST/$PFX-$lab"
            if [ -d "$out" ]; then
                echo "SKIP  $lab  exists ($(head -1 "$out/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED))"
                continue
            fi
            echo "--- $lab port=$port GEN_ORDER=rows -> $out"
            t0=$(date +%s)
            GEN_ORDER=rows "$ROOT/ci/gen-vanilla-region.sh" 12345 16 "$out" "$port" 64 64 overworld
            rc=$?
            echo "    rc=$rc  $(( $(date +%s) - t0 ))s  $(ls "$out"/*.mca 2>/dev/null | wc -l) region files  $(head -1 "$out/HARVEST-ORDER" 2>/dev/null || echo UNSTAMPED)"
            [ "$rc" = 0 ] || rc_all=1
        done
        echo "=== REFS DONE rc=$rc_all @ $(date -Is) ==="
    } >> "$LOG" 2>&1
    # Hand the box back before the pairs stage. The header promises the lock is
    # held only for the vanilla boots, and a promise in a header that the code
    # does not keep is how "a bench must stay outside the corpus lock" stops
    # meaning anything. Pairs is ordinary CPU and excludes nobody.
    corpus_lock_release
fi

if [ "$STAGE" = all ] || [ "$STAGE" = pairs ]; then
    [ -x "$G" ] || { echo "no $G — cargo build --release -p region-differ" >&2; exit 2; }
    {
        echo "=== widen pairs @ $(date -Is) ==="
        # Reuse, never re-take. The reference-against-reference diffs among the
        # existing pool are deterministic functions of two reference
        # directories that nothing has written to, and they are 30 of the 240.
        linked=0
        for r1 in $OLD; do
            for r2 in $OLD; do
                [ "$r1" = "$r2" ] && continue
                src="$SEED_POS/$r1-vs-$r2.pos"; dst="$OUT/pos/$r1-vs-$r2.pos"
                [ -s "$dst" ] && continue
                [ -s "$src" ] && { ln -sf "$src" "$dst"; linked=$((linked+1)); }
            done
        done
        echo "linked $linked existing reference-pair diffs from $SEED_POS"
        made=0
        for r1 in $ALL; do
            for r2 in $ALL; do
                [ "$r1" = "$r2" ] && continue
                p="$OUT/pos/$r1-vs-$r2.pos"
                [ -s "$p" ] && continue
                # THE FULL ORDERED MATRIX, NOT HALF OF IT. The position set of
                # `diff A B` and `diff B A` is the same, but field 4 is the
                # FIRST argument's block, and every unanimous/split and
                # per-block reading in ci/floor-census.py keys on it. Read
                # backwards that field is the candidate's block and the table
                # is silently wrong while every count stays right.
                # shellcheck disable=SC2086
                $G diff "$DEST/$PFX-$r1" "$DEST/$PFX-$r2" --bbox $BBOX --positions > "$p" 2>&1
                grep -q '^DIFF chunks=' "$p" || { echo "BAD $r1-vs-$r2"; rm -f "$p"; rc_all=1; }
                made=$((made+1))
            done
            echo "  pairs from $r1 done ($made new so far)"
        done
        echo "=== PAIRS DONE rc=$rc_all made=$made @ $(date -Is) ==="
    } >> "$LOG" 2>&1
fi

echo "log: $LOG rc=$rc_all"
exit $rc_all
