#!/usr/bin/env bash
# ci/t1-par.sh <cargo args...> — run `cargo <args>` with its TEST BINARIES in
# parallel, producing the log a serial run would have produced.
#
# WHY. T1 is `cargo test --workspace --no-fail-fast`, and cargo runs test
# binaries one at a time. Measured on main 606551b54 in ~/rustmc-gpipe
# (sayer-desktop, 2026-09-30): 158 binaries, 1429.7 s of test wall run
# serially, 12 s of compile, 2893 s user CPU over 1445 s wall — two of the
# box's 24 threads. Four binaries (golden_surface_chunk 227 s,
# golden_carver_chunk 223 s, growing_world 209 s, tiled_world 204 s) hold 60%
# of it and carry 0-3 tests each, so libtest's own threads cannot help.
#
# HOW, AND WHY IT CANNOT CHANGE WHAT A TEST SEES. Cargo is told to launch every
# test binary through `ci/t1-par-runner.sh` (CARGO_TARGET_<host>_RUNNER).
# Cargo still decides which binaries run, in what order, with what arguments,
# in which working directory and with which environment — the runner inherits
# all of it and only execs the binary in the background, returning at once and
# printing a slot marker where the binary's output would have gone. When cargo
# is done this script waits for every binary, replaces each marker with that
# binary's captured output, and computes the exit status: cargo's own if it is
# non-zero, else 101 if ANY binary exited non-zero (cargo's code for a failed
# test run), else 0. Cargo believed every runner succeeded, so its own
# `error: N targets failed:` trailer is re-emitted here from the real statuses.
# Doctests do not go through a runner; cargo runs them itself, unchanged.
#
# T1_JOBS=N binaries at once (default 12); T1_JOBS=1 is plain `cargo <args>`.
# Measured through ci/t1-gate.sh on main 606551b54 + this branch, ~/rustmc-gpipe:
# serial 1445.1 s, T1_JOBS=6 494.4 s, T1_JOBS=12 400.2 s, each
# `T1-GATE GREEN — 158 test binaries, 3324 passed, 10 ignored, 2 known red`;
# peak box memory in use 16 and 17 GB of 62.
# Falsified by ci/t1-par-falsify.sh.
set -uo pipefail
JOBS="${T1_JOBS:-12}"
case "$JOBS" in ''|*[!0-9]*) echo "t1-par: T1_JOBS='$JOBS' is not a count" >&2; exit 2 ;; esac
if [ "$JOBS" -le 1 ]; then exec cargo "$@"; fi
HERE="$(cd "$(dirname "$0")" && pwd)"
HOST=$(rustc -vV | sed -n 's/^host: //p')
[ -n "$HOST" ] || { echo "t1-par: cannot read the host triple from rustc -vV" >&2; exit 2; }
VAR="CARGO_TARGET_$(printf '%s' "$HOST" | tr 'a-z.-' 'A-Z__')_RUNNER"
if [ -n "${!VAR:-}" ]; then
    echo "t1-par: $VAR is already set ('${!VAR}'); running serially rather than replacing it" >&2
    exec cargo "$@"
fi
D=$(mktemp -d "${TMPDIR:-/tmp}/t1par.XXXXXX")
cleanup() {
    local p
    for p in "$D"/pid.*; do
        [ -f "$p" ] || continue
        pkill -TERM -P "$(cat "$p")" 2>/dev/null; kill "$(cat "$p")" 2>/dev/null
    done
    rm -rf "$D"
}
trap cleanup EXIT
trap 'exit 143' TERM INT
export T1PAR_DIR="$D" T1PAR_JOBS="$JOBS"
export "$VAR=$HERE/t1-par-runner.sh"
T_START=$(date +%s)
cargo "$@" > "$D/cargo.out" 2>&1
CRC=$?
# Every slot cargo started must finish. A slot with no rc file and no live
# process died without reporting: that is a failure, never a pass.
n=$(cat "$D/seq" 2>/dev/null || echo 0)
while :; do
    pending=0
    for i in $(seq 1 "$n"); do
        [ -f "$D/rc.$i" ] && continue
        if [ -f "$D/pid.$i" ] && kill -0 "$(cat "$D/pid.$i")" 2>/dev/null; then pending=1
        else echo 255 > "$D/rc.$i"; echo "t1-par: slot $i died without an exit status" >> "$D/out.$i"; fi
    done
    [ "$pending" = 0 ] && break
    sleep 0.5
done
# The log, in cargo's order, markers replaced by the binaries' own output.
awk -v d="$D" '
    /^@@T1PAR-SLOT [0-9]+@@$/ {
        n = $2; sub(/@@$/, "", n); f = d "/out." n
        while ((getline l < f) > 0) print l
        close(f); next
    }
    { print }' "$D/cargo.out"
FAILED=0; FAILED_LIST=""
for i in $(seq 1 "$n"); do
    rc=$(cat "$D/rc.$i")
    if [ "$rc" != 0 ]; then
        FAILED=$((FAILED + 1))
        FAILED_LIST="$FAILED_LIST    \`$(basename "$(cat "$D/exe.$i")")\` (exit $rc)"$'\n'
    fi
done
if [ "$FAILED" -gt 0 ]; then
    printf 'error: %s targets failed:\n%s' "$FAILED" "$FAILED_LIST"
fi
for i in $(seq 1 "$n"); do
    printf 'T1-PAR slot=%s wall_s=%s rc=%s exe=%s\n' "$i" "$(cat "$D/wall.$i" 2>/dev/null || echo na)" \
        "$(cat "$D/rc.$i")" "$(basename "$(cat "$D/exe.$i")")"
done
echo "T1-PAR binaries=$n jobs=$JOBS failed=$FAILED cargo_rc=$CRC wall_s=$(( $(date +%s) - T_START ))"
if [ "$CRC" != 0 ]; then exit "$CRC"; fi
if [ "$FAILED" -gt 0 ]; then exit 101; fi
exit 0
