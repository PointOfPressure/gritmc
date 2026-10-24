#!/usr/bin/env bash
# bench-rss-blockentities.sh — does resident memory come back when a tile
# carrying block entities retires?
#
#   ci/bench-rss-blockentities.sh [runs] [chests]
#
# # What this measures, and the arm that makes it mean something
#
# `crate::tiles` retires a tile nobody can see, and until 2026-08-06 that
# returned the tile's canvas and left its **block entities** resident for the
# life of the process. `crate::edits`' overlay is kept on purpose; the block
# entities were kept by omission, and `World::load_from` inserts every one in
# every saved chunk of every tile it publishes — so the store grew at the rate
# a player could walk and never shrank.
#
# The fix is `BlockEntities::evict_chunks`, gated on the tile having been
# written to disk first. This is the measurement that it returns memory rather
# than the argument that it should.
#
# **Two arms, alternated, one process each**, because a single "after" reading
# proves nothing: RSS falls at a retirement for reasons that have nothing to
# do with block entities, starting with the canvas the retirement was always
# returning. Both arms fill one tile with the same number of chests and retire
# it; they differ only in whether the world has a directory to save into,
# which is the exact condition the eviction is gated on.
#
#   on    persistence on  — saved, evicted, `malloc_trim`med
#   off   persistence off — kept, which is the same choice the overlay makes
#
# One process per reading, and the arms alternate rather than running in two
# blocks: a fill of 50,000 chests leaves the allocator in a state the next
# reading would inherit, and a block layout would confound the arm with
# whichever of them ran while the box was busiest.
#
# `docs/BENCH.md` is where the numbers go. This prints the raw JSON lines and
# the median of each arm; nothing here asserts, because a threshold would be
# a gate on the Threadripper's allocator.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RUNS="${1:-5}"
CHESTS="${2:-50000}"
OUT="${BENCH_OUT:-$ROOT/oracle-work/bench-boot/rss-blockentities.jsonl}"
mkdir -p "$(dirname "$OUT")"

TEST=bench_rss_after_retiring_a_tile_full_of_chests

echo "############ block-entity eviction, runs=$RUNS, chests=$CHESTS"
echo "load at start: $(cut -d' ' -f1-3 /proc/loadavg)"

# Built once, so the first run of the first arm is not also paying for a
# compile — which would land entirely on whichever arm went first.
cargo test --release -p rustmc-server --lib --no-run > /dev/null 2>&1

for i in $(seq 1 "$RUNS"); do
    for arm in on off; do
        # `grep -o`, not `grep '^{'`. libtest writes `test <name> ... ` with
        # **no newline** and fills in `ok` after the test returns, so anything
        # the test prints lands on the end of that line rather than at the
        # start of its own. An anchored pattern here silently matches nothing
        # and the script exits with no output at all, which is how this was
        # found.
        line=$(RUSTMC_EVICT_BENCH_ARM="$arm" RUSTMC_EVICT_BENCH_CHESTS="$CHESTS" \
            cargo test --release -p rustmc-server --lib "$TEST" \
            -- --ignored --exact "tiles::tests::$TEST" --nocapture --test-threads=1 \
            2>/dev/null | grep -o '{"arm".*}' || true)
        [ -n "$line" ] || { echo "run $i arm $arm produced no reading" >&2; exit 1; }
        echo "$line" | tee -a "$OUT"
    done
    echo "  run $i/$RUNS done (load $(cut -d' ' -f1 /proc/loadavg))"
done

python3 - "$OUT" "$RUNS" <<'PY'
import json, statistics, sys
path, runs = sys.argv[1], int(sys.argv[2])
rows = [json.loads(l) for l in open(path) if l.strip()]
rows = rows[-2 * runs:]
print()
print(f"{'arm':<5} {'n':>3} {'held':>7} {'left':>7} "
      f"{'rss_before_mb':>14} {'rss_after_mb':>13} {'freed_mb':>9} {'spread_mb':>10}")
for arm in ("on", "off"):
    a = [r for r in rows if r["arm"] == arm]
    if not a:
        continue
    # `null` is the bench saying RSS is unavailable on this platform (no
    # /proc/self/status). Refuse rather than print a median of nothing.
    if any(r["rss_before_kb"] is None or r["rss_after_kb"] is None for r in a):
        sys.exit(f"bench-rss-blockentities: arm {arm}: RSS unavailable on this platform "
                 "(the bench printed null); nothing to report")
    before = [r["rss_before_kb"] / 1024 for r in a]
    after = [r["rss_after_kb"] / 1024 for r in a]
    freed = [b - f for b, f in zip(before, after)]
    spread = max(freed) - min(freed)
    print(f"{arm:<5} {len(a):>3} {a[0]['held']:>7} {a[0]['left']:>7} "
          f"{statistics.median(before):>14.1f} {statistics.median(after):>13.1f} "
          f"{statistics.median(freed):>9.1f} {spread:>10.1f}")
print()
print("`held` is how many block entities were resident before the retirement "
      "and `left` how many after. The `off` arm is the control: same fill, "
      "same retirement, nothing evicted.")
PY
