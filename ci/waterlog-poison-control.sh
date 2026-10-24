#!/usr/bin/env bash
# waterlog-poison-control.sh — does the `isWaterAt` fix fire on this window at
# all, and does reverting ONLY that arm put the window back byte-exact?
#
# WHY A CONTROL AND NOT A DIFF AGAINST VANILLA
#
# The `isWaterAt` fix is block-output-only: neither call site takes an RNG
# draw, so it cannot move a stream and cannot move a *count* of anything the
# order decomposition scores. Diffing it against the reference ensemble would
# therefore answer a question it cannot fail — and this project has already
# shipped a feature that was green for a week because no gate ever executed it.
#
# So the first question is not "how much did it improve" but **"did it run"**.
# Two dumps of OURS, differing only by a revert of the single fix commit,
# compared byte-for-byte:
#
#   * differ  -> the window exercises the fix; the positions that moved are
#                the fix's whole footprint, and a delta against the 16
#                references is then worth taking over exactly those positions.
#   * identical -> the fix is correct against the bytecode and **this window
#                cannot exercise it**. That is a reportable result, not a
#                failure, and it must be said out loud rather than left for a
#                green number to imply coverage.
#
# It is also the poison control the close criterion asks for: the reverted arm
# must return the window to its pre-fix bytes, 0 in either direction, over the
# identical position set — which `cmp` gives for free and a residual count
# does not.
#
# usage: RUSTMC_REMOTE_DIR=... ci/waterlog-poison-control.sh <fix-commit-sha>
set -uo pipefail

FIX=${1:?usage: ci/waterlog-poison-control.sh <fix-commit-sha>}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REMOTE=${RUSTMC_REMOTE_DIR:?set RUSTMC_REMOTE_DIR}
ORDER=${ORDER:-zx}
OUT=${OUT:-$ROOT/poison-control}
mkdir -p "$OUT"
cd "$ROOT" || exit 2

if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "REFUSED  working tree is dirty — the revert arm would not be a clean revert"
    exit 2
fi

# The dumps live OUTSIDE the rsync target, and that is not tidiness.
# They used to be written to ~/$REMOTE/pc-<arm>, which is inside the tree
# `sync_build_dump` rsyncs with --delete: the second arm's sync deleted the
# first arm's dump before it could be compared, and `diff -rq` then wrote
# "No such file or directory" into the result file, which the emptiness test
# read as **differing arms**. So the control reported FIRES off its own
# failure — the exact shape of defect it exists to catch, in the catcher.
DUMPS=${DUMPS:-\$HOME/sulfur-pc}

sync_build_dump() {
    local arm=$1
    rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
        --exclude 'data/vanilla' --exclude read-trace --exclude poison-control \
        --exclude '*.log' "$ROOT/" "threadripper:$REMOTE/" || return 1
    # `rsync -a` preserves mtimes, and a reverted file arrives OLDER than the
    # artefact built from the fixed one. Cargo trusts mtimes, so without this
    # the "reverted" arm is the fixed binary and the control passes vacuously.
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; mkdir -p $DUMPS; \
        rm -rf $DUMPS/pc-$arm; cd $REMOTE && \
        find crates tools ci -type f \\( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \\) \
            -print0 | xargs -0 touch && \
        cargo build --release -j 8 -p worldgen-dump 2>&1 | tail -3 && \
        RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER=$ORDER \
            ./target/release/worldgen-dump full 12345 16 $DUMPS/pc-$arm 64 64 \
            > /dev/null 2> $DUMPS/pc-$arm.err" || return 1
    # A dump that produced no region files is not an arm. Without this the
    # comparison below is between two absences, which compares equal.
    local n
    n=$(ssh -n threadripper "find $DUMPS/pc-$arm -name '*.mca' 2>/dev/null | wc -l")
    if [ "${n:-0}" -lt 1 ]; then
        echo "REFUSED  arm $arm produced no region files"
        return 1
    fi
    echo "  arm $arm: $n region files"
}

echo "=== arm: fixed (HEAD) ==="
sync_build_dump fixed || { echo "FAIL fixed arm"; exit 1; }

echo "=== arm: reverted (only $FIX) ==="
git revert --no-commit "$FIX" || { echo "REFUSED  revert of $FIX did not apply"; exit 2; }
# Prove the revert actually changed the source. A revert that applied cleanly
# but emptied to nothing would leave two identical arms and read as a pass.
if git diff --cached --quiet; then
    git revert --quit
    echo "REFUSED  reverting $FIX changed no file — there is no arm to control"
    exit 2
fi
sync_build_dump reverted
rc=$?
git revert --quit
git reset --hard HEAD >/dev/null || exit 2
[ "$rc" -eq 0 ] || { echo "FAIL reverted arm"; exit 1; }

echo
echo "=== byte comparison over the identical position set ==="
# Both arms must still exist at compare time. This is checked rather than
# assumed because the previous version of this script destroyed one of them
# and then read the resulting diff error as a difference between arms.
if ! ssh -n threadripper "test -d $DUMPS/pc-fixed && test -d $DUMPS/pc-reverted"; then
    echo "REFUSED  an arm is missing at compare time — no verdict"
    exit 2
fi
ssh -n threadripper "diff -rq $DUMPS/pc-fixed $DUMPS/pc-reverted" > "$OUT/cmp.txt" 2>&1
rc=$?
# diff exits 0 identical, 1 differing, >1 error. Only 0 and 1 are verdicts.
if [ "$rc" -gt 1 ]; then
    echo "REFUSED  diff failed (exit $rc) — this is an error, not a difference"
    cat "$OUT/cmp.txt"
    exit 2
fi
if [ "$rc" -eq 0 ]; then
    echo "IDENTICAL — the fix is byte-inert on seed 12345 r=16 centre (64,64)"
    echo "under RUSTMC_DECORATE_ORDER=$ORDER. The window does not exercise it."
    echo "Report it as such; do not let a green number imply coverage."
else
    echo "FIRES — the arms differ. Region files that moved:"
    cat "$OUT/cmp.txt"
    echo
    echo "Next: score ONLY these positions against the 16-reference set."
fi
