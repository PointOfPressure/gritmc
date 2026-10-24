#!/usr/bin/env bash
# refcache-serve-bench.sh — what a reference-cache HIT costs to serve.
#
# docs/CORPUS-LOCK-HOLD.md §4 leaves one row open: the hold of a gate with
# RUSTMC_REFERENCE_CACHE=1, at an N. That row needs the lock and it needs
# gates. This script closes the *other* half of it, which needs neither.
#
# A hit hold is (port pick) + (this serve) + (rustmc boot) + (subject
# recording) + (compare). Every term but one is already measured or is measured
# by ci/hold-phases.sh on any run. The one term that exists ONLY on a hit is
# the serve — and a serve is a cache lookup, which boots nothing, binds nothing
# and takes no lock. So it is measured here, at N=20, in a private
# REFCACHE_DIR, in about four seconds.
#
# WHY IT IS WORTH MEASURING AT ALL, given the answer is small. Because "small"
# was the assumption, and the key hashes a 25 MB server jar, a 108-file corpus,
# three generated tables and the recorder's own source on EVERY serve — by
# design, since ci/reference-cache.sh re-derives the key rather than trusting a
# stored one. A key that re-hashes 25 MB could plausibly have cost seconds, and
# a serve that cost as much as a boot would have changed the arithmetic the
# whole cache rests on. It does not: it costs 0.09 s.
#
#     ./ci/refcache-serve-bench.sh [n]
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
N="${1:-20}"
BIN="$ROOT/target/release/oracle-runner"

W="$(mktemp -d "${TMPDIR:-/tmp}/refcache-serve.XXXXXX")"
trap 'rm -rf "$W"' EXIT
export REFCACHE_DIR="$W/cache"
export RUSTMC_REFERENCE_CACHE=1
mkdir -p "$REFCACHE_DIR"

[ -x "$BIN" ] || { echo "refcache-serve-bench: no $BIN — cargo build -q -p oracle-runner --release" >&2; exit 2; }

# The recording to serve. Its SIZE is what a serve copies, so it must be a real
# one — a 200-byte stub would measure the syscall and not the artefact. Any
# vanilla.json on the box will do; the cache never inspects its contents.
SRC="${REFCACHE_BENCH_RECORDING:-}"
if [ -z "$SRC" ]; then
    SRC="$(ls -S "$HOME"/rustmc*/oracle-work/gameplay/vanilla.json 2>/dev/null | head -1)"
fi
[ -s "${SRC:-}" ] || { echo "refcache-serve-bench: no vanilla.json anywhere under ~/rustmc*/oracle-work/gameplay/.
    Point REFCACHE_BENCH_RECORDING at one, or run a corpus first." >&2; exit 2; }
echo "== refcache-serve-bench: serving $(du -h "$SRC" | cut -f1) from $SRC, n=$N"

. "$ROOT/ci/reference-cache.sh"
OUTF="$W/out.json"

t0=$(date +%s.%N)
refcache_store "$ROOT" "$BIN" "${GAMEPLAY_SEED:-20260803}" "$SRC" >/dev/null 2>&1 \
    || { echo "refcache-serve-bench: refcache_store refused — the tree is missing a key input:"; \
         refcache_lookup "$ROOT" "$BIN" "${GAMEPLAY_SEED:-20260803}" "$OUTF" 2>&1 | head -4; exit 2; }
t1=$(date +%s.%N)
STORE=$(echo "$t1-$t0" | bc)

TSV="$W/serve.tsv"
HITS=0
for i in $(seq 1 "$N"); do
    rm -f "$OUTF"
    t0=$(date +%s.%N)
    if refcache_lookup "$ROOT" "$BIN" "${GAMEPLAY_SEED:-20260803}" "$OUTF" >/dev/null 2>&1; then
        HITS=$((HITS + 1))
    fi
    t1=$(date +%s.%N)
    printf '%s\n' "$(echo "$t1-$t0" | bc)" >> "$TSV"
done

# Every lookup must have HIT. A bench that timed twenty misses would report a
# fast, meaningless number — a miss returns before it copies anything, so it is
# *faster* than a hit and would understate the very thing being measured.
if [ "$HITS" -ne "$N" ]; then
    echo "refcache-serve-bench: only $HITS/$N lookups hit — the timings below are"
    echo "  measuring refusals, not serves. Nothing is reported."
    exit 1
fi
awk -v st="$STORE" -v n="$N" '
    {s+=$1; if($1>mx)mx=$1; if(mn==""||$1<mn)mn=$1; v[NR]=$1}
    END{ printf "store   %.3f s\n", st
         printf "serve   %.3f s  mean of %d hits (%.3f-%.3f)\n", s/NR, n, mn, mx
         printf "\nAgainst the 21.3 min of reference recording a hit replaces, the serve is\n"
         printf "%.4f%% of what it saves.\n", 100*(s/NR)/(21.3*60) }' "$TSV"
