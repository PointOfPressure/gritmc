#!/usr/bin/env bash
# plugin-hook-cost.sh — what the plugin hook costs a server with no plugins.
#
#     RUSTMC_REMOTE_DIR=<your-dir> ./ci/plugin-hook-cost.sh [control-ref] [reps]
#
# # The claim being tested
#
# "Zero plugins loaded means zero behaviour change, and the hot path does not
# gain measurable cost." The first half is the gameplay corpus' job. This is
# the second half, and it is a *comparison*, so it needs a tree without the
# hook to compare against — reading the source and counting branches is not a
# measurement.
#
# So: `git archive` the control commit into a scratch checkout, plant the same
# `tests/dig_throughput.rs` in it with its two `// HOOK-ONLY` lines deleted,
# and run the identical 200k-dig loop in both. Same box, same compiler, same
# world, same packets, same iteration count.
#
# # Interleaved, and reported as a minimum
#
# Runs alternate subject/control/subject/control so a thermal or scheduling
# drift over the run cannot land entirely on one side. The **minimum** of each
# side is quoted rather than the mean: this loop is deterministic work, so
# every sample above the minimum is somebody else's noise, and a mean is a
# measure of how busy the box was.
#
# The loop is single-threaded by construction — one connection, one session —
# so wall and CPU coincide here and the `docs/AGENT-BRIEF.md` rule about
# quoting CPU-seconds beside wall has nothing to add. That is stated rather
# than silently skipped; `/usr/bin/time` output for each run is in the log.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HOST="${RUSTMC_REMOTE:-threadripper}"
DEST="${RUSTMC_REMOTE_DIR:-}"
CONTROL_REF="${1:-}"
REPS="${2:-5}"
if [ -z "$DEST" ]; then
    echo "plugin-hook-cost: set RUSTMC_REMOTE_DIR" >&2
    exit 2
fi
if [ -z "$CONTROL_REF" ]; then
    echo "plugin-hook-cost: name the control ref (the commit before the hook)" >&2
    exit 2
fi

BASE_DEST="$DEST-base"
WORK="${TMPDIR:-/tmp}/plugin-hook-control.$$"
LOG="${RUSTMC_COST_LOG:-$HOME/grit/logs/plugin-hook-cost.log}"
mkdir -p "$(dirname "$LOG")"
: >"$LOG"

echo "plugin-hook-cost: subject=$(git rev-parse --short HEAD) control=$(git rev-parse --short "$CONTROL_REF") reps=$REPS" | tee -a "$LOG"

rm -rf "$WORK"
mkdir -p "$WORK"
git archive "$CONTROL_REF" | tar -x -C "$WORK" || { echo "archive failed" >&2; exit 2; }

# The instrument, planted in the control with the two lines a pre-hook tree
# cannot compile removed. **A control that silently kept them would fail to
# build and this script would report nothing**, which is why the grep below
# checks the plant landed rather than trusting sed.
grep -v '// HOOK-ONLY' crates/rustmc-server/tests/dig_throughput.rs \
    >"$WORK/crates/rustmc-server/tests/dig_throughput.rs"
if grep -q 'HOOK-ONLY' "$WORK/crates/rustmc-server/tests/dig_throughput.rs"; then
    echo "plugin-hook-cost: the control still has HOOK-ONLY lines — it will not build" >&2
    exit 1
fi
if grep -q 'plugins:' "$WORK/crates/rustmc-server/src/world_send.rs"; then
    echo "plugin-hook-cost: $CONTROL_REF already has the hook — that is not a control" >&2
    exit 1
fi

# The instrument builds a packet, so it needs `rustmc-proto` as a
# dev-dependency, and the control predates the line that added one. Adding it
# to the control is not tilting the comparison: a dev-dependency changes what
# the *test target* may name and nothing about the library the loop runs
# through. Without it the control does not compile and this script reports
# nothing at all — which would read as an inconclusive run rather than as a
# missing line.
if ! grep -q 'rustmc-proto' "$WORK/crates/rustmc-server/Cargo.toml"; then
    printf 'rustmc-proto.workspace = true\n' >>"$WORK/crates/rustmc-server/Cargo.toml"
fi

rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
    --exclude 'data/vanilla' "$WORK/" "$HOST:$BASE_DEST/" || exit 2
./ci/t0-remote.sh --sync >/dev/null 2>&1 || exit 2

run() { # dir -> ns_per_dig
    ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; cd $1 && \
        /usr/bin/time -v cargo test -q --release -p rustmc-server \
        --test dig_throughput -- --ignored --nocapture 2>&1"
}

echo "== warming both builds" | tee -a "$LOG"
run "$DEST" >>"$LOG" 2>&1
run "$BASE_DEST" >>"$LOG" 2>&1

SUBJ=""
CTRL=""
for i in $(seq 1 "$REPS"); do
    s=$(run "$DEST" | tee -a "$LOG" | grep -oE 'ns_per_dig=[0-9.]+' | cut -d= -f2)
    c=$(run "$BASE_DEST" | tee -a "$LOG" | grep -oE 'ns_per_dig=[0-9.]+' | cut -d= -f2)
    if [ -z "$s" ] || [ -z "$c" ]; then
        echo "plugin-hook-cost: rep $i produced no number — see $LOG" >&2
        exit 1
    fi
    echo "rep $i  subject=$s  control=$c" | tee -a "$LOG"
    SUBJ="$SUBJ $s"
    CTRL="$CTRL $c"
done

python3 - "$SUBJ" "$CTRL" <<'PY' | tee -a "$LOG"
import sys
s = [float(x) for x in sys.argv[1].split()]
c = [float(x) for x in sys.argv[2].split()]
smin, cmin = min(s), min(c)
print(f"subject min {smin:.1f} ns/dig   (all: {['%.1f' % x for x in s]})")
print(f"control min {cmin:.1f} ns/dig   (all: {['%.1f' % x for x in c]})")
d = smin - cmin
print(f"delta {d:+.1f} ns/dig  ({100.0 * d / cmin:+.2f}%)")
spread = max(max(s) - smin, max(c) - cmin)
print(f"run-to-run spread within a side: {spread:.1f} ns/dig")
if abs(d) <= spread:
    print("VERDICT: the difference is inside the run-to-run spread — not measurable here.")
else:
    print("VERDICT: the difference EXCEEDS the run-to-run spread and is a real cost.")
PY
echo "log: $LOG"
