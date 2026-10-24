#!/usr/bin/env bash
# plugin-hook-falsify.sh — break the plugin hook on purpose, seventeen ways,
# and require the suite to notice.
#
#     RUSTMC_REMOTE_DIR=<your-dir> ./ci/plugin-hook-falsify.sh
#
# # Why this exists rather than a coverage number
#
# The plugin hook's whole claim is that a denial from a plugin stops a block
# write the server would otherwise make. A green test suite is consistent with
# that claim and also with the hook never being reached, the verdict being
# discarded, the plugin never loading, or the world being one that cannot
# change. This project has shipped every one of those shapes at least once.
#
# So each mutation below removes or subverts exactly one link and the suite has
# to go **red**. A mutation that stays green names a link nothing is checking,
# and this script prints it as UNCAUGHT rather than passing quietly.
#
# # The prediction is written down before the run
#
# `ci/plugin-hook-mutate.py` records an `expect` of `red` or `green` for every
# mutation. `green` is a prediction that this crate's tests *cannot* see the
# mutation, with the reason — M16 is the only one, and it is green because a
# plugin under-declaring its capabilities is invisible to a host that provides
# them, which is the contract behaving correctly rather than a hole.
#
# A `green` entry coming back red is as interesting as a `red` entry coming
# back green: it means the reasoning was wrong. Both are reported.
#
# # F3, and why a mutation that does not apply is a failure and not a skip
#
# `grit-plugins`' own sweep found three mutations that stayed green, and one of
# them was a bug in its own mutation script: the anchor text had moved and the
# patch silently did nothing, so the run measured an unmutated tree and called
# it a pass. Every anchor here is required to appear **exactly once**, and a
# count of anything else aborts the sweep — an instrument that cannot bite is
# worse than no instrument, because it still emits a verdict somebody reads.
#
# Nothing compiles on the laptop: each mutation is applied locally, rsynced,
# and tested on the box. Roughly a minute per mutation.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HOST="${RUSTMC_REMOTE:-threadripper}"
DEST="${RUSTMC_REMOTE_DIR:-}"
if [ -z "$DEST" ]; then
    echo "plugin-hook-falsify: set RUSTMC_REMOTE_DIR to your own remote tree" >&2
    exit 2
fi

# The tests that speak about the hook. Deliberately the whole of the server
# crate's lib tests and not just `plugin`: a mutation that breaks the hook and
# also breaks something else is still caught, and a mutation that *only* breaks
# something else is a mutation aimed at the wrong thing.
#
# `--test plugin_tick_lock` is named as well, and that is not a convenience.
# The threading assertion has to run in a binary where nothing else can be
# holding the process-wide tick lock — it was a `--lib` test first and M13
# caught it on one sweep and missed it on the next, because another test's
# handler was supplying the `true`. See that file's header.
run_tests() {
    rsync -az --delete \
        --exclude 'target' --exclude 'oracle-work' --exclude '.git' \
        --exclude 'data/vanilla' \
        "$ROOT/" "$HOST:$DEST/" >/dev/null 2>&1 || return 99
    ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; cd $DEST && \
        cargo test -q -p rustmc-server --lib --test plugin_tick_lock 2>&1" \
        >"$LOG_DIR/$1.log" 2>&1
}

LOG_DIR="${RUSTMC_FALSIFY_LOGS:-$HOME/grit/logs/plugin-hook-falsify}"
mkdir -p "$LOG_DIR"

trap 'python3 ci/plugin-hook-mutate.py --revert' EXIT INT TERM

# One snapshot for the whole sweep, taken before anything is touched.
python3 ci/plugin-hook-mutate.py --snapshot

echo "== control: the unmutated tree must be GREEN"
python3 ci/plugin-hook-mutate.py --revert
if ! run_tests control; then
    echo "REFUSED  the unmutated tree is already red — fix that before falsifying"
    tail -30 "$LOG_DIR/control.log"
    exit 1
fi
echo "   control GREEN"

IDS=$(python3 - <<'PY'
import json, subprocess, sys, os
out = subprocess.check_output(["python3", "ci/plugin-hook-mutate.py", "--list"])
for m in json.loads(out):
    print(f"{m['id']} {m['expect']}")
PY
)

CAUGHT=0
UNCAUGHT=0
SURPRISE=0
while read -r ID EXPECT; do
    [ -n "$ID" ] || continue
    python3 ci/plugin-hook-mutate.py --apply "$ID"
    RC=$?
    if [ "$RC" = 3 ]; then
        echo "ABORT    $ID — the anchor did not apply. See the F3 note above."
        exit 1
    fi
    if run_tests "$ID"; then
        GOT=green
    else
        GOT=red
    fi
    python3 ci/plugin-hook-mutate.py --revert
    # **Which tests caught it, by name.** A count would say a mutation was
    # caught; only the names say it was caught by the mechanism its entry
    # claims. That distinction is the entire lesson of `grit-plugins`' own F5,
    # where a mutation looked caught and was being caught by a different
    # defence than the test described.
    CATCHERS=$(grep -oE '^[a-z_:0-9]+ --- FAILED' "$LOG_DIR/$ID.log" |
        sed 's/ --- FAILED//' | sort -u | tr '\n' ' ')
    FAILED=$(printf '%s' "$CATCHERS" | wc -w)
    if [ "$GOT" = "$EXPECT" ]; then
        if [ "$EXPECT" = red ]; then
            echo "CAUGHT   $ID  by $FAILED: $CATCHERS"
            CAUGHT=$((CAUGHT + 1))
        else
            echo "UNCAUGHT $ID  — predicted, see ci/plugin-hook-mutate.py"
            UNCAUGHT=$((UNCAUGHT + 1))
        fi
    else
        echo "SURPRISE $ID  expected $EXPECT, got $GOT  ($FAILED failed: $CATCHERS)"
        SURPRISE=$((SURPRISE + 1))
    fi
done <<<"$IDS"

# **The control again, at the end.** A sweep that leaves the tree or the
# remote build cache mutated would make the *next* run measure the previous
# run's binary, and that has happened here once already — see `--revert`'s
# docstring in ci/plugin-hook-mutate.py. A trailing control is the cheapest
# thing that can see it.
echo "== control again: the tree must be back where it started"
python3 ci/plugin-hook-mutate.py --revert
if ! run_tests control-after; then
    echo "REFUSED  the tree is red after the sweep — a mutation survived it"
    grep -E ' --- FAILED' "$LOG_DIR/control-after.log" | head -5
    SURPRISE=$((SURPRISE + 1))
else
    echo "   control-after GREEN"
fi

echo
echo "plugin-hook-falsify: $CAUGHT caught, $UNCAUGHT uncaught-as-predicted, $SURPRISE surprises"
echo "logs in $LOG_DIR"
# A surprise is the only thing that reddens this gate. An uncaught mutation
# that was predicted uncaught, with its reason recorded, is a documented limit
# of the suite rather than a failure of it — and the prediction is what stops
# that sentence being an excuse written after the fact.
[ "$SURPRISE" = 0 ] || exit 1
