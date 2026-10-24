#!/usr/bin/env bash
# ci/t1-par-runner.sh <test-binary> [args...] — the cargo runner ci/t1-par.sh
# installs. Never run by hand. Cargo invokes it once per test binary, with the
# binary's own cwd and environment; it waits for a free slot (T1PAR_JOBS),
# starts the binary in the background with its output captured to the slot,
# prints the slot marker ci/t1-par.sh replaces, and returns 0 so cargo moves on.
# The binary's real exit status is written to the slot and judged by
# ci/t1-par.sh — cargo's belief that this succeeded is never used.
set -u
D="${T1PAR_DIR:?t1-par-runner: T1PAR_DIR unset — this is run by ci/t1-par.sh only}"
J="${T1PAR_JOBS:-12}"
# Doctests come through a runner too (cargo passes rustdoc's --runtool), as a
# `rust_out` in a rustdoc temp dir that rustdoc deletes the moment this
# returns. They are not parallelised: run in the foreground, exactly as
# cargo would have run them without a runner.
case "$1" in */rustdoctest*|rust_out|*/rust_out) exec "$@" ;; esac
# Cargo launches runners one at a time, so the counters need no lock.
n=$(( $(cat "$D/seq" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$D/seq"
while :; do
    running=0
    for p in "$D"/pid.*; do
        [ -f "$p" ] || continue
        i=${p##*.}
        [ -f "$D/rc.$i" ] || running=$((running + 1))
    done
    [ "$running" -lt "$J" ] && break
    sleep 0.2
done
printf '%s\n' "$1" > "$D/exe.$n"
(
    s=$(date +%s%N)
    "$@" > "$D/out.$n" 2>&1 </dev/null
    rc=$?
    echo $(( ( $(date +%s%N) - s ) / 1000000 )) | awk '{printf "%.1f\n", $1/1000}' > "$D/wall.$n"
    echo "$rc" > "$D/rc.$n.tmp" && mv "$D/rc.$n.tmp" "$D/rc.$n"
) >/dev/null 2>&1 </dev/null &
echo $! > "$D/pid.$n"
echo "@@T1PAR-SLOT $n@@"
exit 0
