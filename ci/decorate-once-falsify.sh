#!/usr/bin/env bash
# decorate-once-falsify.sh — show `ci/decorate-once.sh`'s truncation guard
# REFUSING a truncated draw log before showing it accepting a whole one.
#
# The guard exists because a `DecorateGolden` run that dies part way through
# leaves a complete-looking draw log: `-Ddecorate.draws` streams to stderr as
# the run goes, so everything before the failure is already on disk and nothing
# says the rest is missing. On 2026-08-18 a five-centre `trees_jungle` run
# reported three chunks that way and was read as "only three chunks here have
# jungle"; it reports fifteen once the harness can finish.
# `docs/DETERMINISM.md` §52.
#
# A guard that has never been shown failing is a guard nobody can trust, and
# this one cannot be exercised by running the real dumper — the failure it
# catches needs a broken `GenLevel`. So the *logic* is lifted out and driven
# against logs written here. That is a weaker test than mutating the subject
# and it is stated as weaker: it proves the predicate discriminates, not that
# `decorate-once.sh` calls it correctly. The second half is covered by the
# guard having fired on a real run and named the right chunk — ten BEGINs
# against two ENDs, which is how the terminator's own first placement was
# found to be wrong.
#
# Usage: ci/decorate-once-falsify.sh
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
fail=0
pass=0

# The predicate, copied from `ci/decorate-once.sh`. Copied rather than sourced
# because that script runs a JVM at import time; a divergence between the two
# is caught by the `grep` below, which refuses if the shapes stop matching.
if ! grep -q "DRAW TRACE TRUNCATED" "$ROOT/ci/decorate-once.sh"; then
    echo "REFUSING: ci/decorate-once.sh no longer carries the truncation guard" >&2
    exit 2
fi

guard() {  # <logfile> <args...> -> 0 paired / 3 truncated / 0 unchecked
    local log=$1; shift
    local arg b e
    arg=$(printf '%s\n' "$@" | grep -m1 -- '-Ddecorate.draws=' || true)
    [ -n "$arg" ] || return 0
    b=$(grep -c 'DRAWS-BEGIN ' "$log" || true)
    e=$(grep -c 'DRAWS-END ' "$log" || true)
    [ "$b" = "$e" ] || return 3
    return 0
}

check() {  # <name> <expected-status> <logfile> <args...>
    local name=$1 want=$2 log=$3; shift 3
    guard "$log" "$@"
    local got=$?
    if [ "$got" = "$want" ]; then
        echo "ok   $name (status $got)"
        pass=$((pass + 1))
    else
        echo "FAIL $name — wanted status $want, got $got" >&2
        fail=$((fail + 1))
    fi
}

printf 'DRAWS-BEGIN f chunk 1,1 record=true\nDRAW 0 nextInt(2) -> 1\nDRAWS-END f chunk 1,1 draws=1\n' \
    > "$TMP/whole.log"
printf 'DRAWS-BEGIN f chunk 1,1 record=true\nDRAW 0 nextInt(2) -> 1\n' \
    > "$TMP/truncated.log"
# Two chunks, the second unterminated — the real shape, where the log looks
# fine right up to the end.
printf 'DRAWS-BEGIN f chunk 1,1 record=true\nDRAWS-END f chunk 1,1 draws=0\nDRAWS-BEGIN f chunk 2,1 record=true\nDRAW 0 nextInt(2) -> 1\n' \
    > "$TMP/second-died.log"
: > "$TMP/empty.log"

# THE REFUSALS FIRST.
check "a truncated log is refused"              3 "$TMP/truncated.log"   -Ddecorate.draws=f
check "a log whose SECOND chunk died is refused" 3 "$TMP/second-died.log" -Ddecorate.draws=f

# THEN THE ACCEPTANCES, so a guard that refuses everything cannot pass.
check "a whole log is accepted"                 0 "$TMP/whole.log"       -Ddecorate.draws=f
check "an empty log is accepted"                0 "$TMP/empty.log"       -Ddecorate.draws=f
check "no -Ddecorate.draws means no check"      0 "$TMP/truncated.log"   -Ddecorate.seed=1
# The flag has to be recognised wherever it sits in the argument list.
check "the flag is found after other arguments" 3 "$TMP/truncated.log" \
    -Ddecorate.seed=1 -Ddecorate.centers=0,0 -Ddecorate.draws=f

echo "decorate-once-falsify: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
