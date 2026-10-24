#!/usr/bin/env bash
# ===== AGENT INTERFACE — this is all you need to read =====
#
# ci/run-detached-falsify.sh — prove ci/run-detached.sh's sentinel fires on
# every outcome, including the ones the payload has no string for.
#
#     ./ci/run-detached-falsify.sh        # ~15 s, no box, no corpus lock
#
# WHAT BREAKS IF YOU IGNORE THIS: a waiter that can only see a clean finish
# hangs on a dirty one, and a hung waiter is indistinguishable from a long job.
# That is what `ci/t2-gameplay.sh`'s missing RED line did to the waiter
# `docs/AGENT-BRIEF.md` recommended.
# ===== end interface; everything below is why =====
#
# The cases are chosen as the outcomes a payload does NOT announce:
#
#   rc=0 with output          the easy one, and the control
#   rc=1                      a failure the payload does not name
#   rc=0 with NO output       silence, which must not read as still-running
#   syntax error              refused up front, while an agent is watching
#   payload killed -9         the wrapper survives and still reports
#   name already running      refused, or two runs share one sentinel
#
# Each asserts BOTH that the sentinel appears and that the waiter predicate
# `ci/run-detached.sh` prints actually matches it — a sentinel nothing greps
# for is not a sentinel.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTMC_DETACHED_DIR
RUSTMC_DETACHED_DIR="$(mktemp -d /tmp/run-detached-falsify.XXXXXX)"
trap 'rm -rf "$RUSTMC_DETACHED_DIR"' EXIT

pass=0; fail=0
ok()   { echo "ok   $1"; pass=$((pass+1)); }
bad()  { echo "FAIL $1"; fail=$((fail+1)); }

# Wait for a sentinel exactly the way the printed waiter does, bounded so a
# broken case fails the suite instead of hanging it.
await() {
    local name="$1" log="$RUSTMC_DETACHED_DIR/$1.log" pidf="$RUSTMC_DETACHED_DIR/$1.pid" i=0
    while [ $i -lt 200 ]; do
        grep -qa "^DETACHED-DONE $name " "$log" 2>/dev/null && return 0
        kill -0 "$(cat "$pidf" 2>/dev/null || echo 0)" 2>/dev/null || { sleep 0.3
            grep -qa "^DETACHED-DONE $name " "$log" 2>/dev/null && return 0; return 1; }
        sleep 0.1; i=$((i+1))
    done
    return 2
}

want_rc() {  # want_rc <name> <expected rc>
    local line; line=$(grep -a "^DETACHED-DONE $1 " "$RUSTMC_DETACHED_DIR/$1.log" | tail -1)
    case "$line" in
        *"rc=$2 "*) return 0 ;;
        *) echo "     got: ${line:-<no sentinel>}"; return 1 ;;
    esac
}

# --- case 1: rc=0, the control -------------------------------------------
"$ROOT/ci/run-detached.sh" c1 >/dev/null <<'EOF'
echo hello
EOF
if await c1 && want_rc c1 0; then ok "rc=0 with output: sentinel present, rc forwarded"
else bad "rc=0 with output"; fi

# --- case 2: rc=1, a failure the payload never names ----------------------
"$ROOT/ci/run-detached.sh" c2 >/dev/null <<'EOF'
echo "this run says nothing about failing"
exit 1
EOF
if await c2 && want_rc c2 1; then ok "rc=1 the payload never names: sentinel present, rc=1"
else bad "rc=1 the payload never names"; fi

# --- case 3: total silence -----------------------------------------------
"$ROOT/ci/run-detached.sh" c3 >/dev/null <<'EOF'
exit 3
EOF
if await c3 && want_rc c3 3; then ok "no output at all: silence still ends the wait"
else bad "no output at all"; fi

# --- case 4: a body that does not parse is refused up front ---------------
if "$ROOT/ci/run-detached.sh" c4 >/dev/null 2>&1 <<'EOF'
if then fi done
EOF
then bad "unparseable body: accepted, would have died in a log nobody opens"
else ok "unparseable body: REFUSED while an agent is looking"; fi

# --- case 5: payload killed -9, wrapper still reports ---------------------
"$ROOT/ci/run-detached.sh" c5 >/dev/null <<'EOF'
sleep 300
EOF
sleep 1
# kill the sleep, not the wrapper: the wrapper must survive and report.
pkill -9 -P "$(cat "$RUSTMC_DETACHED_DIR/c5.pid")" 2>/dev/null
c5line=""
if await c5; then c5line=$(grep -a "^DETACHED-DONE c5 " "$RUSTMC_DETACHED_DIR/c5.log" | tail -1); fi
case "$c5line" in
    *"rc=0 "*|"") bad "payload killed -9: got '${c5line:-<no sentinel>}'" ;;
    *) ok "payload killed -9: sentinel present with a non-zero rc (${c5line##* rc=})" ;;
esac

# --- case 6: two runs under one name are refused --------------------------
"$ROOT/ci/run-detached.sh" c6 >/dev/null <<'EOF'
sleep 20
EOF
if "$ROOT/ci/run-detached.sh" c6 >/dev/null 2>&1 <<'EOF'
echo second
EOF
then bad "duplicate name: accepted, the waiter would fire on whichever finished first"
else ok "duplicate name: REFUSED"; fi
pkill -9 -P "$(cat "$RUSTMC_DETACHED_DIR/c6.pid" 2>/dev/null || echo 0)" 2>/dev/null
await c6 >/dev/null 2>&1

# --- case 7: --status fails closed on a death with no sentinel ------------
: > "$RUSTMC_DETACHED_DIR/c7.log"
if "$ROOT/ci/run-detached.sh" --status c7 >/dev/null 2>&1; then
    bad "--status on a log with no sentinel and no process: reported success"
else ok "--status fails closed when there is no sentinel and no process"; fi

# --- case 8: extra ARGV is refused ----------------------------------------
#
# The shape found on 2026-09-05: `run-detached.sh <name> bash script.sh` was
# accepted, the arguments dropped, the body read from an empty stdin, and
# `DETACHED-DONE <name> rc=0` written for a payload that never existed. The
# red-before-green for this case is the pre-repair script, which returns 0
# here and leaves a rc=0 sentinel behind.
if "$ROOT/ci/run-detached.sh" c8 bash /dev/null </dev/null >/dev/null 2>&1; then
    bad "argv payload: accepted — an empty body would report rc=0 for work that never ran"
else ok "argv payload: REFUSED"; fi
if [ -s "$RUSTMC_DETACHED_DIR/c8.log" ]; then
    bad "argv payload: a log was written for a refused run"
else ok "argv payload: no run was started"; fi

# --- case 9: an empty body is refused -------------------------------------
#
# The same green by the other door: `bash -n` passes the empty program and
# `bash ""` exits 0, so a body lost to `ssh -n` (stdin is /dev/null, which is
# not a tty and so slips the `-t 0` guard) or to a mistyped heredoc terminator
# becomes a sentinel that says the sequence succeeded.
if "$ROOT/ci/run-detached.sh" c9 </dev/null >/dev/null 2>&1; then
    bad "empty body: accepted — DETACHED-DONE rc=0 for a sequence with no commands"
else ok "empty body: REFUSED"; fi
if [ -s "$RUSTMC_DETACHED_DIR/c9.log" ]; then
    bad "empty body: a log was written for a refused run"
else ok "empty body: no run was started"; fi

# comments and blank lines are not a sequence — the shape a truncated heredoc
# leaves behind
if printf '#!/usr/bin/env bash\n\n# nothing here\n' | \
   "$ROOT/ci/run-detached.sh" c10 >/dev/null 2>&1; then
    bad "comments-only body: accepted"
else ok "comments-only body: REFUSED"; fi

n=$((pass+fail))
if [ "$fail" -eq 0 ]; then
    echo "RUN-DETACHED-FALSIFY GREEN — $n cases, 0 failed"
else
    echo "RUN-DETACHED-FALSIFY RED — $n cases, $fail failed"
fi
exit $((fail > 0))
