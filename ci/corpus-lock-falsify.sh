#!/usr/bin/env bash
# corpus-lock-falsify.sh — prove `ci/corpus-lock.sh` by breaking it.
#
#     ./ci/corpus-lock-falsify.sh
#
# Twenty-nine cases and twenty-four injections (counted 2026-09-30, not incremented),
# about seventeen minutes, in a throwaway lock namespace so this
# can never queue behind, or wedge, real work on the box. Each case drives real
# processes through the real acquire path; the payload is a sleep rather than a
# corpus because the lock cannot tell the difference and a fifty-minute payload
# would make this unrunnable.
#
# (The count in this header said "fourteen cases" against a `run_suite` of
# fifteen, and `docs/AGENT-BRIEF.md` said twelve. Counted rather than
# incremented, 2026-08-12.)
#
# **The injections are the point, not the cases.** A green case only says the
# instrument agreed with the code; an injection says the instrument would have
# noticed had the code been wrong. So every case is required to go red under at
# least one deliberate break, and an injection that is NOT caught is reported
# as loudly as a failure.
#
# # This suite lied on its first run, and the shape is worth keeping
#
# The first version reported `PASS exclusion: 4 concurrent runs serialised` for
# a library with the mutex removed, and missed three injections because of it.
# Two instrument bugs compounded:
#
#   * `spawn` was called as `$(spawn ...)`, so the background payloads were
#     children of a **command-substitution subshell**. The parent could not
#     `wait` for them; `wait` returned 127 at once and the case read an events
#     file that was still empty;
#   * reading it did `n=$(grep -c ENTER f || echo 0)`, which on no matches
#     yields `"0\n0"`. `[ "0\n0" -ne 4 ]` is not false, it is an **error** —
#     and `if` treats a non-zero status as false, so the guard skipped its own
#     failure branch and fell through to the `ok`.
#
# A guard that errors reads exactly like a guard that passes. That is rule 7 —
# when a measurement clears every candidate, suspect the measurement — and it
# is why `case_exclusion` now also asserts **elapsed wall time**, which no
# empty events file can satisfy: four 3-second holds cannot serialise in under
# 12 seconds, whatever the log says.
#
# Once it stopped lying it earned its keep immediately: the second run put
# three cases red on a design that had looked obviously correct, because the
# lock fd was inherited by the payload's children and so outlived the payload.
# That is what put the anchor in `ci/corpus-lock.sh`. The payload below still
# spawns a long-lived child **with no special care at all**, because "a spawn
# site needs no discipline" is the anchor's central claim and the suite has to
# be able to falsify it.
#
# # The fairness cases, and why one of them is built by hand
#
# Seven cases were added on 2026-08-12 for the run queue, one per claim, and the
# shape of the first is the part worth reading. `case_fifo_no_barge` does not
# start several runs and hope the kernel picks the wrong one — it CONSTRUCTS the
# state the overtaking was measured in: an older run queued, and the mutex free.
# `$WORK/queued.sh` registers as a waiter and then does nothing else, which is
# not an artificial process: it is exactly what a real waiter looks like in the
# moment after it hands the mutex back to a gate, and that moment repeating
# every fifty minutes is how one branch lost the box for 1h58m.
#
# That construction is what makes the case DETERMINISTIC. `flock -x -n` on a
# free lock always succeeds, so before the fix this case fails every time and
# after it, it cannot fail at all. The alternative — several real runs racing —
# is a case that goes red only sometimes, and a test that is red only sometimes
# is a test nobody can read a fix off.
#
# Two of the injections wedge the lock on purpose (`yield-to-any-run-not-just-
# older`, `park-while-holding-the-mutex`), which is why every fairness case
# waits with `bounded_wait` rather than `wait`. That is the same lesson the
# `await_enter` comment above records, arrived at from the other direction: a
# suite that hangs on a wedge reports nothing, and reporting nothing is what a
# suite that was never run also does.
#
# The suite gives its anchors a private `CORPUS_LOCK_ANCHOR_NAME` and cleans up
# by that name. Some injections deliberately produce an anchor that outlives
# its run, and killing those by the production name would kill the anchor of a
# real corpus run on the same box — which is exactly the accident this whole
# file exists to prevent.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB_REAL="$ROOT/ci/corpus-lock.sh"
# Every case written before 2026-09-24 is about a MUTEX, and a pool of one is
# that mutex byte for byte (slot 0 is `run.lock`). So they run at a pool of one
# whatever the library's default is, and the pool has cases of its own below
# that pass `RUSTMC_CORPUS_LOCK_POOL=2` explicitly.
export RUSTMC_CORPUS_LOCK_POOL=1

WORK="$(mktemp -d "${TMPDIR:-/tmp}/corpus-lock-falsify.XXXXXX")"
# Private anchor name. Never the production one: injections 9 and 10 leave an
# anchor running on purpose, and cleaning those up by the production name would
# kill a real corpus run's anchor on this box.
export CORPUS_LOCK_ANCHOR_NAME="corpus-lock-falsify-anchor-$$"
cleanup() {
    pkill -P $$ >/dev/null 2>&1
    pkill -f "$CORPUS_LOCK_ANCHOR_NAME" >/dev/null 2>&1
    # Heartbeat tickers are forked subshells of a payload and keep its argv, so
    # they are only reachable by the (unique) work path. An injection that stops
    # tickers being killed leaks them on purpose; they must not outlive the sweep.
    pkill -f "$WORK" >/dev/null 2>&1
    sleep 1
    rm -rf "$WORK"
}
trap cleanup EXIT

PASS=0; FAIL=0; MISSED=0
declare -a FAILED_CASES=() MISSED_INJECTIONS=()

say()  { printf '%s\n' "$*"; }
ok()   { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); FAILED_CASES+=("$*"); printf '  FAIL  %s\n' "$*"; }

# --- the payload ------------------------------------------------------------
#
# Deliberately shaped like `ci/t2-gameplay.sh`: it takes the lock, starts a
# long-lived background "server" the way any harness would, checks the lock is
# still anchored, holds, and leaves.
cat > "$WORK/payload.sh" <<'PAYLOAD'
#!/usr/bin/env bash
set -euo pipefail
ID="$1"; HOLD="$2"
ROOT="${TREE:-$PWD}"
. "${CORPUS_LOCK_LIB}"
corpus_lock_acquire
# A long-lived child, spawned with NO special care — no `200>&-`, nothing.
# Under the anchor design it cannot inherit the lock, and that is the claim
# this suite has to be able to falsify.
sleep 300 &
printf '%s\n' "$!" > "$RUSTMC_CORPUS_LOCK_DIR/child.$ID"
printf '%s\n' "${_CL_ANCHOR:-}" > "$RUSTMC_CORPUS_LOCK_DIR/anchor.$ID"
corpus_lock_assert_anchored
printf 'ENTER %s %s\n' "$ID" "$(date +%s.%N)" >> "$EVENTS"
# Re-assert every second, so a lock that goes free mid-run is caught in the
# act rather than inferred afterwards.
for _ in $(seq 1 "$HOLD"); do sleep 1; corpus_lock_assert_anchored; done
printf 'EXIT %s %s\n' "$ID" "$(date +%s.%N)" >> "$EVENTS"
kill "$(cat "$RUSTMC_CORPUS_LOCK_DIR/child.$ID")" 2>/dev/null || true
corpus_lock_release
PAYLOAD
chmod +x "$WORK/payload.sh"

new_ns() { mktemp -d "$WORK/ns.XXXXXX"; }

# Sets $SPID. NOT called in a command substitution — that was the bug that
# made this suite lie. A `$(...)` subshell owns the job and the parent cannot
# wait for it.
SPID=""
spawn() { # spawn <ns> <lib> <role> <id> <hold>
    local ns="$1" lib="$2" role="$3" id="$4" hold="$5"
    RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" EVENTS="$ns/events" \
        TREE="$ROOT" RUSTMC_CORPUS_LOCK_ROLE="$role" CORPUS_LOCK_HEARTBEAT=2 \
        "$WORK/payload.sh" "$id" "$hold" > "$ns/log.$id" 2>&1 &
    SPID=$!
}

# A process that is QUEUED and nothing else: it registers as a waiter, holds its
# place, and never contends for the mutex. That is not an artificial state — it
# is exactly what a real waiter looks like in the moment after it hands the
# mutex back to a gate, and it is the state in which the 2026-08-11 barge was
# measured: an older run queued, and the mutex free. Constructing it directly is
# what makes `case_fifo_no_barge` deterministic instead of a race the suite
# would have to run fifty times and hope.
# The same payload at a pool of two, each run in a tree of its own unless one is
# named: the tree lock is per-tree, and a shared TREE would serialise the pool
# cases for a reason that is not the pool.
spawn_pool() { # spawn_pool <ns> <lib> <role> <id> <hold> [tree]
    local ns="$1" lib="$2" role="$3" id="$4" hold="$5" tree="${6:-$1/tree.$4}"
    mkdir -p "$tree"
    RUSTMC_CORPUS_LOCK_POOL=2 RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" \
        EVENTS="$ns/events" TREE="$tree" RUSTMC_CORPUS_LOCK_ROLE="$role" CORPUS_LOCK_HEARTBEAT=2 \
        "$WORK/payload.sh" "$id" "$hold" > "$ns/log.$id" 2>&1 &
    SPID=$!
}

# Most runs inside one instant, from payload timestamps: the pool's bound.
max_concurrent() {
    python3 - "$1" <<'PY'
import sys
ev = []
for line in open(sys.argv[1]):
    k, i, t = line.split()
    ev.append((float(t), 0 if k == "EXIT" else 1))
n = m = 0
for t, d in sorted(ev):
    n += 1 if d else -1
    m = max(m, n)
print(m)
PY
}

cat > "$WORK/queued.sh" <<'QUEUED'
#!/usr/bin/env bash
set -uo pipefail
ID="$1"; HOLD="$2"
ROOT="${TREE:-$PWD}"
. "${CORPUS_LOCK_LIB}"
_cl_setup
_cl_register_waiter "${RUSTMC_CORPUS_LOCK_ROLE:-run}"
printf '%s\n' "$$" > "$RUSTMC_CORPUS_LOCK_DIR/queued.$ID"
# The fd close is not tidiness. `sleep` would inherit the queue token and keep
# it alive after a SIGKILL of this shell, which would make this helper a worse
# citizen than the library it is standing in for.
#
# The `:-9` is what lets this run against a library with no queue in it at all,
# which is how `case_fifo_no_barge` was checked against the pre-2026-08-12
# `ci/corpus-lock.sh` rather than only against an injection modelling it. There
# is no token to protect there, and closing an unused fd 9 costs nothing.
eval "sleep \"\$HOLD\" ${_CL_WAIT_FD:-9}>&-"
_cl_unregister_waiter
QUEUED
chmod +x "$WORK/queued.sh"

# A run that is OLDER than one already queued, published after that one started
# blocking — and which then blocks on the mutex itself.
#
# This exists for one branch of the acquire and would be unreachable without it.
# A run at the back of the queue never contends for the mutex, so the only way
# to hold the mutex and still find somebody ahead of you is for them to have
# registered while you were blocked: `seq` is taken at the top of
# `_cl_register_waiter` and the record is published a rename later, so two
# processes registering together can be published in the opposite order to their
# places. That window is sub-millisecond and nothing outside can aim at it.
#
# So it is constructed instead of raced for: this registers normally, taking a
# real token, then rewrites its own record **in place** — `>` truncates the
# inode rather than replacing it, so the flock survives, where `sed -i` would
# silently drop the token by writing a new file — with the oldest `seq` there
# is. Everything the protocol reads is genuine: a live PID, a matching
# `pidstart`, a held token. Only its place in the order is chosen.
#
# Then it blocks on the mutex, which is what makes the deadlock a deadlock: a
# process yielding to this one while still holding the mutex is waiting for a
# token held by a process waiting for the mutex it is holding.
cat > "$WORK/older.sh" <<'OLDER'
#!/usr/bin/env bash
set -uo pipefail
ID="$1"; DELAY="$2"; HOLD="$3"
ROOT="${TREE:-$PWD}"
. "${CORPUS_LOCK_LIB}"
_cl_setup
_cl_register_waiter run
{
    printf 'role=run\npid=%s\n' "$$"
    printf 'pidstart=%s\n' "$(_cl_pidstart $$ || echo '')"
    printf 'tree=%s\nhost=%s\n' "$ROOT" "$(hostname)"
    printf 'since=%s\n' "$(date +%s)"
    printf 'seq=1\nfairv=1\n'
} > "$CORPUS_LOCK_DIR/waiters/$$"
printf '%s\n' "$$" > "$CORPUS_LOCK_DIR/older.$ID"
eval "sleep \"\$DELAY\" $_CL_WAIT_FD>&-"
printf 'BLOCKING\n' >> "$CORPUS_LOCK_DIR/older.$ID"
_cl_wait_run_lock
printf 'GOT\n' >> "$CORPUS_LOCK_DIR/older.$ID"
# Our place is given up the moment we stop waiting for it, exactly as the
# library does — anything parked behind us is released here and not before.
_cl_unregister_waiter
eval "sleep \"\$HOLD\" $_CL_RUN_FD>&-"
flock -u "$_CL_RUN_FD"
OLDER
chmod +x "$WORK/older.sh"

OPID=""
older_run() { # older_run <ns> <lib> <id> <delay-before-blocking> <hold>
    local ns="$1" lib="$2" id="$3" delay="$4" hold="$5"
    RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" TREE="$ROOT" \
        "$WORK/older.sh" "$id" "$delay" "$hold" > "$ns/olog.$id" 2>&1 &
    OPID=$!
}

QPID=""
queued() { # queued <ns> <lib> <role> <id> <hold>
    local ns="$1" lib="$2" role="$3" id="$4" hold="$5"
    RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" \
        TREE="$ROOT" RUSTMC_CORPUS_LOCK_ROLE="$role" \
        "$WORK/queued.sh" "$id" "$hold" > "$ns/qlog.$id" 2>&1 &
    QPID=$!
}

n_enters() { local n; n="$(grep -c '^ENTER' "$1" 2>/dev/null || true)"; printf '%s' "${n:-0}"; }

# CPU burned by every child this suite has reaped, in clock ticks, read from
# this shell's own /proc entry. `times` cannot be used: it is a builtin, and in
# the `$(...)` needed to capture it the subshell reports its own children, which
# are none. Deltas of this are the spin detector — a livelock that still makes
# progress is invisible to a wall-clock bound and obvious here.
children_cpu() {
    local s
    s="$(cat "/proc/$$/stat" 2>/dev/null)" || { printf '0'; return; }
    s="${s##*) }"
    printf '%s\n' "$s" | awk '{print $14 + $15}'
}

await_file() { # await_file <path> <max-seconds>
    local f="$1" max="$2" i=0
    while [ "$i" -lt "$max" ]; do
        [ -s "$f" ] && return 0
        sleep 1; i=$((i+1))
    done
    return 1
}

# `wait` with a bound, and the reason is the same one written against
# `await_enter` above: two of the injections below WEDGE the lock on purpose,
# and a bare `wait` on a wedged payload stalls this suite forever instead of
# failing it. A stalled case reports nothing at all, which is indistinguishable
# from a suite that was never run. Everything is reaped either way, because the
# CPU delta in `case_burst` is read from this shell's reaped-children counters
# and an unreaped payload contributes nothing to them.
bounded_wait() { # bounded_wait <max-seconds> <pid...>
    local max="$1"; shift
    local i=0 p alive rc=0
    while [ "$i" -lt "$max" ]; do
        alive=""
        for p in "$@"; do if kill -0 "$p" 2>/dev/null; then alive=1; break; fi; done
        [ -n "$alive" ] || break
        sleep 1; i=$((i+1))
    done
    if [ -n "${alive:-}" ]; then
        rc=1
        for p in "$@"; do kill -9 "$p" 2>/dev/null; done
        sleep 1
    fi
    for p in "$@"; do wait "$p" 2>/dev/null; done
    return $rc
}

# Overlap is computed from timestamps the payloads wrote themselves, never from
# anything the lock said about itself. A lock that lied in its own log would
# still be caught here.
overlaps() {
    python3 - "$1" <<'PY'
import sys
ev = {}
try:
    lines = open(sys.argv[1]).read().split('\n')
except OSError:
    sys.exit(1)
for line in lines:
    if not line.strip():
        continue
    k, i, t = line.split()
    ev.setdefault(i, {})[k] = float(t)
iv = sorted(((v['ENTER'], v.get('EXIT', float('inf')), i) for i, v in ev.items() if 'ENTER' in v))
bad = []
for a in range(len(iv)):
    for b in range(a + 1, len(iv)):
        if iv[b][0] < iv[a][1]:
            bad.append(f"{iv[a][2]} [{iv[a][0]:.3f},{iv[a][1]:.3f}] overlaps {iv[b][2]} [{iv[b][0]:.3f},{iv[b][1]:.3f}]")
print('\n'.join(bad))
sys.exit(0 if bad else 1)
PY
}

enter_order() { sort -k3 -g "$1" 2>/dev/null | awk '$1=="ENTER"{printf "%s ", $2}'; }

lock_state() { RUSTMC_CORPUS_LOCK_DIR="$1" bash "$2" status 2>&1; }

# Wait for <id> to ENTER, bounded. Returns the seconds it took, or "" if it
# never did. `wait` on the payload was used here and it was wrong: a wedged
# lock makes the payload block forever, so `wait` STALLS the suite instead of
# failing it — and a stalled case reports nothing at all, which is how the
# `anchor-without-pdeathsig` injection came back MISSED against a break that
# demonstrably wedges the lock. A bound turns a wedge into a red.
await_enter() { # await_enter <events> <id> <max-seconds>
    local f="$1" id="$2" max="$3" i=0
    while [ "$i" -lt "$max" ]; do
        grep -q "^ENTER $id " "$f" 2>/dev/null && { printf '%s' "$i"; return 0; }
        sleep 1; i=$((i+1))
    done
    printf ''
    return 1
}

# ===========================================================================
# Cases
# ===========================================================================

case_exclusion() { # 4 runs at once must serialise, and must take the time
    local lib="$1" ns pids=() p t0 t1 el n o
    ns="$(new_ns)"; : > "$ns/events"
    t0=$(date +%s)
    for i in 1 2 3 4; do spawn "$ns" "$lib" run "r$i" 3; pids+=("$SPID"); done
    for p in "${pids[@]}"; do wait "$p" 2>/dev/null; done
    t1=$(date +%s); el=$((t1 - t0))
    n="$(n_enters "$ns/events")"
    if [ "$n" -ne 4 ]; then bad "exclusion: $n/4 runs entered"; return 1; fi
    # The independent check. Four 3s holds cannot serialise in under 12s, so
    # this cannot be satisfied by an empty or unread log — which is exactly how
    # the first version of this suite passed a library with no mutex in it.
    if [ "$el" -lt 12 ]; then
        bad "exclusion: 4x3s holds finished in ${el}s — they cannot have serialised"; return 1; fi
    o="$(overlaps "$ns/events")"
    if [ -n "$o" ]; then bad "exclusion: runs overlapped — $o"; return 1; fi
    ok "exclusion: 4 concurrent runs serialised, no overlap, ${el}s wall (>=12 required)"
}

case_waits_not_fails() { # every queued run must finish, exit 0
    local lib="$1" ns pids=() p rc bad_rc=0 n
    ns="$(new_ns)"; : > "$ns/events"
    for i in 1 2 3; do spawn "$ns" "$lib" run "w$i" 2; pids+=("$SPID"); done
    for p in "${pids[@]}"; do wait "$p" 2>/dev/null; rc=$?; [ "$rc" -eq 0 ] || bad_rc=$rc; done
    if [ "$bad_rc" -ne 0 ]; then
        bad "waits-not-fails: a queued run exited $bad_rc instead of waiting
$(cat "$ns"/log.w* 2>/dev/null | tail -20)"; return 1; fi
    n="$(n_enters "$ns/events")"
    if [ "$n" -ne 3 ]; then bad "waits-not-fails: only $n/3 queued runs got in"; return 1; fi
    ok "waits-not-fails: all 3 queued runs waited, then ran, rc=0"
}

case_gate_priority() { # a gate declared last must still go next
    local lib="$1" ns order i wins=0 tries=4
    for i in $(seq 1 $tries); do
        ns="$(new_ns)"; : > "$ns/events"
        spawn "$ns" "$lib" run  A 8    # holder
        sleep 2
        spawn "$ns" "$lib" run  B 2    # queued first
        sleep 2
        spawn "$ns" "$lib" gate G 2    # queued last, must go next
        wait 2>/dev/null
        order="$(enter_order "$ns/events")"
        if [ "$order" = "A G B " ]; then wins=$((wins+1)); else
            say "        try $i order was: '$order' (want 'A G B ')"; fi
    done
    if [ "$wins" -ne "$tries" ]; then
        bad "gate-priority: gate went next in only $wins/$tries tries"; return 1; fi
    ok "gate-priority: gate queued last went next in $wins/$tries tries"
}

case_gate_beats_crowd() { # one gate against four already-waiting runs
    local lib="$1" ns order i
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 8
    sleep 2
    for i in 1 2 3 4; do spawn "$ns" "$lib" run "b$i" 1; done
    sleep 2
    spawn "$ns" "$lib" gate G 1
    wait 2>/dev/null
    order="$(enter_order "$ns/events")"
    case "$order" in
        "A G "*) ok "gate-beats-crowd: gate beat 4 waiting runs — order: $order" ;;
        *) bad "gate-beats-crowd: order was '$order', wanted A then G"; return 1 ;;
    esac
}

case_gates_serialise() { # two gates must not overlap, and must not deadlock
    local lib="$1" ns o n
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" gate G1 3
    spawn "$ns" "$lib" gate G2 3
    wait 2>/dev/null
    n="$(n_enters "$ns/events")"
    if [ "$n" -ne 2 ]; then bad "gates-serialise: $n/2 gates got in — deadlock?"; return 1; fi
    o="$(overlaps "$ns/events")"
    if [ -n "$o" ]; then bad "gates-serialise: two gates overlapped — $o"; return 1; fi
    ok "gates-serialise: two concurrent gates serialised, both ran"
}

case_kill9_frees() { # SIGKILL the holder: the lock must free, fast
    local lib="$1" ns hp bp el
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 120; hp=$SPID
    sleep 3
    kill -9 "$hp" 2>/dev/null
    spawn "$ns" "$lib" run B 1; bp=$SPID
    el="$(await_enter "$ns/events" B 20)"
    kill -9 "$bp" 2>/dev/null
    kill -9 "$(cat "$ns/child.A" 2>/dev/null)" 2>/dev/null
    if [ -z "$el" ]; then
        bad "kill9-frees: 20s after the holder was SIGKILLed the next acquirer STILL had not got in — something is holding the lock past its run"
        return 1; fi
    ok "kill9-frees: holder SIGKILLed mid-hold, next acquirer in after ${el}s"
}

case_kill9_after_waiting() { # a run that QUEUED, then ran, then died
    local lib="$1" el
    local ns; ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 5          # holder, uncontended: no ticker
    sleep 2
    spawn "$ns" "$lib" run B 60; local bpid=$SPID   # queues, so a ticker exists
    sleep 12                            # A has gone, B now holds
    if ! grep -q 'ACQUIRED' "$ns/log.B" 2>/dev/null; then
        bad "kill9-after-waiting: B never acquired, so the case tests nothing"
        kill -9 "$bpid" 2>/dev/null; return 1; fi
    kill -9 "$bpid" 2>/dev/null
    spawn "$ns" "$lib" run C 1; local cpid=$SPID
    el="$(await_enter "$ns/events" C 20)"
    kill -9 "$cpid" 2>/dev/null
    kill -9 "$(cat "$ns/child.B" 2>/dev/null)" 2>/dev/null
    if [ -z "$el" ]; then
        bad "kill9-after-waiting: 20s on, C still had not got in — something from B's wait (its heartbeat ticker) still holds the lock"
        return 1; fi
    ok "kill9-after-waiting: a run that queued, ran, then was SIGKILLed left nothing holding the lock (${el}s)"
}

case_kill9_with_live_server() { # the realistic kill: a server outlives the harness
    local lib="$1" ns hp cp out el t0
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 120; hp=$SPID
    sleep 3
    cp="$(cat "$ns/child.A" 2>/dev/null || true)"
    kill -9 "$hp" 2>/dev/null
    sleep 1
    if [ -z "$cp" ] || ! kill -0 "$cp" 2>/dev/null; then
        bad "kill9-live-server: the background server did not survive the kill — case proves nothing"
        return 1
    fi
    out="$(lock_state "$ns" "$lib")"
    kill -9 "$cp" 2>/dev/null
    if ! grep -q 'state:   FREE' <<<"$out"; then
        bad "kill9-live-server: lock still HELD by a surviving server after the harness died
$out"; return 1; fi
    ok "kill9-live-server: server outlived the SIGKILLed harness and the lock still freed"
}

case_anchor_is_sole_holder() { # exactly one process holds the lock, and it is the anchor
    local lib="$1" ns out anchor holders hp
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 8; hp=$SPID
    sleep 3
    anchor="$(cat "$ns/anchor.A" 2>/dev/null || true)"
    out="$(lock_state "$ns" "$lib")"
    # ONLY the fd-holder lines. Grepping the whole of `status` matched the
    # `record:` line, which names the harness by design, and scored a correct
    # library as red. Second instrument bug in this file with the same shape:
    # the assertion was looking at more text than the claim was about.
    local fdlines; fdlines="$(grep '^    pid ' <<<"$out" || true)"
    holders="$(printf '%s' "$fdlines" | grep -c . || true)"
    wait "$hp" 2>/dev/null
    if [ -z "$anchor" ]; then bad "anchor-sole: no anchor pid was recorded"; return 1; fi
    if [ "${holders:-0}" -ne 1 ]; then
        bad "anchor-sole: $holders processes hold the lock fd, want exactly 1:
$out"; return 1; fi
    if ! grep -q "pid $anchor " <<<"$fdlines"; then
        bad "anchor-sole: the one holder is not the anchor (want pid $anchor):
$out"; return 1; fi
    if grep -q "pid $hp " <<<"$fdlines"; then
        bad "anchor-sole: the harness itself still holds the lock fd:
$out"; return 1; fi
    ok "anchor-sole: exactly one fd holder and it is the anchor (pid $anchor), not the harness"
}

case_child_cannot_inherit() { # a child spawned with no care must not hold the lock
    local lib="$1" ns out cp hp
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 8; hp=$SPID
    sleep 3
    cp="$(cat "$ns/child.A" 2>/dev/null || true)"
    out="$(lock_state "$ns" "$lib")"
    wait "$hp" 2>/dev/null
    if [ -z "$cp" ] || ! grep -q "$cp" <<<"$(ls /proc 2>/dev/null)"; then
        : # the child may already be reaped; the assertion below still holds
    fi
    if grep '^    pid ' <<<"$out" | grep -c "pid $cp " >/dev/null; then
        bad "child-inherit: a child spawned with no fd discipline is holding the lock:
$out"; return 1; fi
    ok "child-inherit: an ordinary background child (pid $cp) did not inherit the lock"
}

case_dead_anchor_refused() { # a lock that quietly goes free must be caught in the act
    local lib="$1" ns anchor hp rc out
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 20; hp=$SPID
    sleep 3
    anchor="$(cat "$ns/anchor.A" 2>/dev/null || true)"
    if [ -z "$anchor" ]; then bad "dead-anchor: no anchor pid recorded"; kill -9 "$hp" 2>/dev/null; return 1; fi
    kill -9 "$anchor" 2>/dev/null
    wait "$hp" 2>/dev/null; rc=$?
    out="$(cat "$ns/log.A" 2>/dev/null)"
    kill -9 "$(cat "$ns/child.A" 2>/dev/null)" 2>/dev/null
    if [ "$rc" -eq 0 ]; then
        bad "dead-anchor: the run carried on after its lock went free (rc=0)"; return 1; fi
    if ! grep -q 'anchor .* is gone' <<<"$out"; then
        bad "dead-anchor: exited $rc but did not say the anchor had gone:
$out"; return 1; fi
    ok "dead-anchor: anchor SIGKILLed mid-run, the run refused (rc=$rc) rather than measuring unlocked"
}

case_anchor_failure_refuses() { # if the anchor cannot come up, refuse — never run unlocked
    local lib="$1" ns broken rc out n
    broken="$WORK/lib.brokenanchor.sh"
    # Derived from the library under test, so an injection that removes the
    # verification composes with this and the case goes red.
    sed 's/setpriv --pdeathsig KILL/setpriv --no-such-option-at-all/' "$lib" > "$broken"
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$broken" run A 3
    wait "$SPID" 2>/dev/null; rc=$?
    out="$(cat "$ns/log.A" 2>/dev/null)"
    n="$(n_enters "$ns/events")"
    kill -9 "$(cat "$ns/child.A" 2>/dev/null)" 2>/dev/null
    if [ "$n" -ne 0 ]; then
        bad "anchor-failure: the run proceeded ($n entered) with no anchor holding the lock:
$out"; return 1; fi
    if [ "$rc" -eq 0 ]; then
        bad "anchor-failure: exited 0 despite the anchor never coming up:
$out"; return 1; fi
    ok "anchor-failure: anchor could not come up, run refused (rc=$rc) instead of running unlocked"
}

case_stale_record_is_pid_not_timeout() {
    local lib="$1" ns hp out cp
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 120; hp=$SPID
    sleep 3
    kill -9 "$hp" 2>/dev/null
    cp="$(cat "$ns/child.A" 2>/dev/null || true)"
    kill -9 "$cp" 2>/dev/null
    sleep 1
    # The holder record is still there and is seconds old. A timeout-based
    # check would call it live; a PID check must call it gone.
    if [ ! -s "$ns/holder" ]; then bad "stale-record: no holder record was ever written"; return 1; fi
    out="$(lock_state "$ns" "$lib")"
    if ! grep -q 'THAT PROCESS IS GONE' <<<"$out"; then
        bad "stale-record: status did not notice the recorded holder was dead:
$out"; return 1; fi
    if ! grep -q 'state:   FREE' <<<"$out"; then
        bad "stale-record: lock reported HELD after every holder died:
$out"; return 1; fi
    ok "stale-record: a seconds-old record of a dead PID reads as gone, lock reads FREE"
}

case_observable() { # a wait must never be silent
    local lib="$1" ns out log why=""
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 9
    sleep 2
    spawn "$ns" "$lib" run B 1
    sleep 4
    out="$(lock_state "$ns" "$lib")"
    log="$(cat "$ns/log.B" 2>/dev/null)"
    grep -q 'state:   HELD'                <<<"$out" || why="$why status-not-HELD"
    grep -q 'record:  run pid '            <<<"$out" || why="$why status-no-holder-identity"
    grep -qE 'run pid [0-9]+ in .*waiting' <<<"$out" || why="$why status-does-not-list-the-waiter"
    grep -q 'corpus lock: WAITING'         <<<"$log" || why="$why waiter-log-silent"
    grep -q 'held by'                      <<<"$log" || why="$why waiter-log-does-not-name-holder"
    grep -q 'still waiting'                <<<"$log" || why="$why no-heartbeat"
    wait 2>/dev/null
    if [ -n "$why" ]; then
        bad "observable:$why
--- status ---
$out
--- waiter log ---
$log"; return 1; fi
    ok "observable: status names holder and waiters; the waiter logs the wait and heartbeats"
}

case_dead_waiter_reaped() { # a killed waiter must not haunt the waiter list
    local lib="$1" ns hp wp out
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 30; hp=$SPID
    sleep 2
    spawn "$ns" "$lib" run B 1;  wp=$SPID
    sleep 3
    if [ ! -s "$ns/waiters/$wp" ]; then
        bad "dead-waiter: the waiter never registered itself"
        kill -9 "$hp" "$wp" 2>/dev/null; return 1; fi
    kill -9 "$wp" 2>/dev/null
    sleep 1
    out="$(RUSTMC_CORPUS_LOCK_DIR="$ns" bash "$lib" waiters 2>&1)"
    kill -9 "$hp" 2>/dev/null
    kill -9 "$(cat "$ns/child.A" 2>/dev/null)" 2>/dev/null
    if grep -q "pid $wp" <<<"$out"; then
        bad "dead-waiter: a SIGKILLed waiter is still listed as waiting: $out"; return 1; fi
    ok "dead-waiter: a SIGKILLed waiter is reaped by PID liveness, not by a timeout"
}

# ===========================================================================
# Fairness among runs — the four properties, one case each at least
# ===========================================================================

# PROPERTY 1, the deterministic half: a newer run must not take a FREE mutex
# while an older run is queued.
#
# This is the whole of the measured defect. `flock -x -n` on a free lock always
# succeeds, so before the queue check this case failed 100% of the time and
# after it, it cannot fail at all — no repetition, no probability, no hoping the
# kernel picks the wrong waiter. That is why the older run here is a queued-only
# process rather than one blocked on the mutex: a run blocked on the mutex is
# woken by the kernel and usually wins, which is what made this look like a
# rare race rather than the every-time overtaking it was.
case_fifo_no_barge() {
    local lib="$1" ns qp cp el
    ns="$(new_ns)"; : > "$ns/events"
    queued "$ns" "$lib" run S 12; qp=$QPID
    if ! await_file "$ns/queued.S" 6; then
        bad "fifo-no-barge: the older run never queued — the case tests nothing"
        kill -9 "$qp" 2>/dev/null; return 1
    fi
    spawn "$ns" "$lib" run C 1; cp=$SPID
    # The mutex is free and nobody holds it. C must still wait.
    if await_enter "$ns/events" C 5 >/dev/null; then
        bad "fifo-no-barge: a newer run took the free mutex while an older run was queued — this is the 20:01-over-18:07 overtaking, unfixed"
        kill -9 "$qp" "$cp" 2>/dev/null; bounded_wait 5 "$qp" "$cp"; return 1
    fi
    # And must get in of its own accord once the older run leaves.
    el="$(await_enter "$ns/events" C 25)"
    kill -9 "$qp" 2>/dev/null
    bounded_wait 10 "$qp" "$cp"
    if [ -z "$el" ]; then
        bad "fifo-no-barge: C deferred correctly but never got in after the older run left — the queue does not release
$(tail -5 "$ns/log.C" 2>/dev/null)"; return 1; fi
    ok "fifo-no-barge: newer run left a free mutex alone while an older run was queued, then took it when the queue cleared"
}

# PROPERTY 1, end to end: three runs queued behind a holder are served in
# arrival order.
case_fifo_service_order() {
    local lib="$1" ns order pids=()
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 6; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run B 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run C 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run D 1; pids+=("$SPID")
    if ! bounded_wait 45 "${pids[@]}"; then
        bad "fifo-service-order: 45s on, somebody was still queued — wedged, not merely misordered (order so far: '$(enter_order "$ns/events")')"
        return 1; fi
    order="$(enter_order "$ns/events")"
    if [ "$order" != "A B C D " ]; then
        bad "fifo-service-order: served '$order', wanted 'A B C D '"; return 1; fi
    ok "fifo-service-order: three runs queued behind a holder were served oldest-first"
}

# PROPERTY 2: a gate declared LAST still goes first, against an established
# queue rather than against a crowd that arrived together.
case_gate_beats_the_queue() {
    local lib="$1" ns order pids=()
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 8; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run B 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run C 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" gate G 1; pids+=("$SPID")
    if ! bounded_wait 45 "${pids[@]}"; then
        bad "gate-beats-queue: 45s on, somebody was still queued (order so far: '$(enter_order "$ns/events")')"; return 1; fi
    order="$(enter_order "$ns/events")"
    if [ "$order" != "A G B C " ]; then
        bad "gate-beats-queue: served '$order', wanted 'A G B C ' — a gate must jump an ordered queue, and the queue must survive it"; return 1; fi
    ok "gate-beats-queue: gate queued last went next, and the run queue kept its order behind it"
}

# PROPERTY 2, the case the brief singled out: TWO gates against an ordered run
# queue. Both must precede every run, they must not overlap, and the queue must
# still be in order underneath them.
case_two_gates_beat_the_queue() {
    local lib="$1" ns order o pids=()
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 8; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run B 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run C 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" gate G1 1; pids+=("$SPID")
             spawn "$ns" "$lib" gate G2 1; pids+=("$SPID")
    if ! bounded_wait 45 "${pids[@]}"; then
        bad "two-gates-beat-queue: 45s on, somebody was still queued (order so far: '$(enter_order "$ns/events")')"; return 1; fi
    order="$(enter_order "$ns/events")"
    case "$order" in
        "A G1 G2 B C "|"A G2 G1 B C ") ;;
        *) bad "two-gates-beat-queue: served '$order', wanted both gates then B then C"; return 1 ;;
    esac
    o="$(overlaps "$ns/events")"
    if [ -n "$o" ]; then bad "two-gates-beat-queue: overlap — $o"; return 1; fi
    ok "two-gates-beat-queue: two gates both preceded an ordered run queue, serialised, order held: $order"
}

# PROPERTY 3: a burst of simultaneous arrivals, runs and gates together.
# Everybody gets in, nobody overlaps, it finishes, and it does not SPIN.
#
# The CPU assertion is the one that matters and it is not decoration. A yield
# implemented by looping instead of blocking still completes — it would satisfy
# every other check in this file while burning a core per waiter — and the whole
# hazard the brief names is two waiters yielding to each other. Wall time cannot
# see that. Reaped-child CPU can.
case_burst() {
    local lib="$1" ns i t0 t1 el n o c0 c1 cpu pids=() wedged=""
    ns="$(new_ns)"; : > "$ns/events"
    c0="$(children_cpu)"; t0=$(date +%s)
    for i in 1 2 3 4 5 6 7 8; do spawn "$ns" "$lib" run "r$i" 1; pids+=("$SPID"); done
    sleep 1
    spawn "$ns" "$lib" gate g1 1; pids+=("$SPID")
    spawn "$ns" "$lib" gate g2 1; pids+=("$SPID")
    bounded_wait 60 "${pids[@]}" || wedged=1
    t1=$(date +%s); el=$((t1 - t0)); c1="$(children_cpu)"
    cpu=$(( (c1 - c0) / 100 ))
    n="$(n_enters "$ns/events")"
    # The CPU check comes FIRST because it is the only one a spin fails while
    # everything else about the run still looks right, and because a wedge
    # reached by spinning should be reported as the spin it is.
    if [ "$cpu" -gt 30 ]; then
        bad "burst: ${cpu} CPU-seconds for 10 one-second holds — waiters are spinning, not blocking (${n}/10 in, ${el}s)"; return 1; fi
    if [ -n "$wedged" ]; then
        bad "burst: 60s on, ${n}/10 arrivals had got in and somebody was still stuck — deadlock or a lost wakeup"; return 1; fi
    if [ "$n" -ne 10 ]; then
        bad "burst: $n/10 arrivals got in — deadlock, livelock or a lost wakeup"; return 1; fi
    o="$(overlaps "$ns/events")"
    if [ -n "$o" ]; then bad "burst: exclusion broke under load — $o"; return 1; fi
    if [ "$el" -lt 10 ]; then
        bad "burst: 10x1s holds finished in ${el}s — they cannot have serialised"; return 1; fi
    if [ "$el" -gt 45 ]; then
        bad "burst: took ${el}s for 10 one-second holds — something is not being woken promptly"; return 1; fi
    ok "burst: 8 runs + 2 gates, all 10 in, no overlap, ${el}s wall, ${cpu} CPU-s (spin bound 30)"
}

# PROPERTY 3, the one branch nothing else in this file reaches: a run that takes
# the mutex and THEN finds somebody ahead of it must hand the mutex back BEFORE
# it parks. Park first and you are waiting for a token held by a process that is
# waiting for the mutex you are holding.
#
# The first version of this suite could not reach that branch at all and scored
# `park-while-holding-the-mutex` as MISSED — correctly, and the reason is worth
# keeping: the pre-contend park means a run at the back of the queue never
# touches the mutex, so holding it while somebody is ahead of you only happens
# when they registered while you were blocked. `older_run` above constructs
# exactly that, with a real token and a chosen place, and then blocks on the
# mutex so that a park done in the wrong order cannot resolve itself.
#
# The ordering is picked so nothing is a race: A holds until ~10s, B queues at
# 2s and is the only contender when the mutex frees, and S does not start
# blocking on the mutex until 16s — long after B has had to decide.
case_post_acquire_hand_back() {
    local lib="$1" ns ap bp op el
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 10; ap=$SPID
    sleep 2
    spawn "$ns" "$lib" run B 1;  bp=$SPID    # queues, and scans before S exists
    sleep 2
    older_run "$ns" "$lib" S 12 2; op=$OPID  # older place, taken just now
    if ! await_file "$ns/older.S" 6; then
        bad "post-acquire-hand-back: the older run never took a place — the case tests nothing"
        kill -9 "$ap" "$bp" "$op" 2>/dev/null; bounded_wait 5 "$ap" "$bp" "$op"; return 1
    fi
    el="$(await_enter "$ns/events" B 45)"
    kill -9 "$ap" "$bp" "$op" 2>/dev/null
    kill -9 "$(cat "$ns/child.A" 2>/dev/null)" 2>/dev/null
    kill -9 "$(cat "$ns/child.B" 2>/dev/null)" 2>/dev/null
    bounded_wait 10 "$ap" "$bp" "$op"
    if [ -z "$el" ]; then
        bad "post-acquire-hand-back: B took the mutex, found an older run ahead of it, and never got out again — it parked while still holding the mutex
--- B ---
$(tail -4 "$ns/log.B" 2>/dev/null)
--- S ---
$(cat "$ns/older.S" 2>/dev/null)"; return 1; fi
    # A green here that never entered the branch would be measuring nothing, so
    # the case asserts it was entered rather than assuming it.
    if ! grep -q 'yielding to an older waiting run' "$ns/log.B" 2>/dev/null; then
        bad "post-acquire-hand-back: B got in without ever yielding to the older run — the branch this case exists for was not taken
$(tail -4 "$ns/log.B" 2>/dev/null)"; return 1; fi
    if ! grep -q '^GOT' "$ns/older.S" 2>/dev/null; then
        bad "post-acquire-hand-back: the older run never got the mutex it was handed"; return 1; fi
    ok "post-acquire-hand-back: a run holding the mutex found an older run ahead of it, handed the mutex back before parking, and got in after ${el}s"
}

# PROPERTY 4: a queued run that is SIGKILLed must not stall the runs queued
# behind it. Nothing sweeps and nothing times out — the kernel drops the dead
# process's queue token, exactly as it drops the holder's lock.
case_dead_waiter_no_stall() {
    local lib="$1" ns ap bp cp el
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 40; ap=$SPID
    sleep 2
    spawn "$ns" "$lib" run B 40; bp=$SPID     # queues behind A
    sleep 2
    spawn "$ns" "$lib" run C 1;  cp=$SPID     # queues behind B
    sleep 3
    if [ ! -s "$ns/waiters/$bp" ]; then
        bad "dead-waiter-no-stall: B never queued, so C is not parked behind anything"
        kill -9 "$ap" "$bp" "$cp" 2>/dev/null; bounded_wait 5 "$ap" "$bp" "$cp"; return 1; fi
    kill -9 "$bp" 2>/dev/null      # the middle of the queue dies
    sleep 1
    kill -9 "$ap" 2>/dev/null      # and the holder goes too, so the box is free
    el="$(await_enter "$ns/events" C 25)"
    kill -9 "$cp" 2>/dev/null
    kill -9 "$(cat "$ns/child.A" 2>/dev/null)" 2>/dev/null
    kill -9 "$(cat "$ns/child.B" 2>/dev/null)" 2>/dev/null
    kill -9 "$(cat "$ns/child.C" 2>/dev/null)" 2>/dev/null
    bounded_wait 10 "$ap" "$bp" "$cp"
    if [ -z "$el" ]; then
        bad "dead-waiter-no-stall: 25s after the run ahead of it was SIGKILLed, C is still parked behind a corpse
$(tail -5 "$ns/log.C" 2>/dev/null)"; return 1; fi
    ok "dead-waiter-no-stall: a SIGKILLed queued run released its place at once; the run behind it got in after ${el}s"
}

# PROPERTY 4, the other half: killing the middle of a queue must not reorder
# what is left. A sweeper-based design can drop the wrong entry here.
case_queue_survives_a_death() {
    local lib="$1" ns cp order pids=()
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run A 8; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run B 1; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run C 1; cp=$SPID; pids+=("$SPID")
    sleep 2; spawn "$ns" "$lib" run D 1; pids+=("$SPID")
    sleep 1
    kill -9 "$cp" 2>/dev/null
    if ! bounded_wait 45 "${pids[@]}"; then
        bad "queue-survives-death: 45s on, somebody was still queued behind the corpse (order so far: '$(enter_order "$ns/events")')"; return 1; fi
    order="$(enter_order "$ns/events")"
    if [ "$order" != "A B D " ]; then
        bad "queue-survives-death: served '$order', wanted 'A B D ' (C was SIGKILLed while queued)"; return 1; fi
    ok "queue-survives-death: the middle of a three-deep queue was SIGKILLed and the rest kept their order"
}


# ===========================================================================
# The pool (2026-09-24). A pool of two must admit two and never three, must
# take WHICHEVER slot frees first, must still put a gate ahead of queued runs,
# and must still keep two runs of one tree apart.
# ===========================================================================

case_pool_bound() { # 4 runs, pool 2: exactly two at once, never three
    local lib="$1" ns pids=() p t0 el m n
    ns="$(new_ns)"; : > "$ns/events"
    t0=$(date +%s)
    for i in 1 2 3 4; do spawn_pool "$ns" "$lib" run "p$i" 4; pids+=("$SPID"); done
    bounded_wait 90 "${pids[@]}"
    el=$(( $(date +%s) - t0 ))
    n="$(n_enters "$ns/events")"; m="$(max_concurrent "$ns/events")"
    if [ "$n" -ne 4 ]; then bad "pool-bound: $n/4 runs entered"; return 1; fi
    if [ "$m" -ne 2 ]; then bad "pool-bound: $m runs held the pool at once, wanted exactly 2"; return 1; fi
    if [ "$el" -lt 8 ] || [ "$el" -ge 16 ]; then
        bad "pool-bound: 4x4s holds took ${el}s — two at a time is 8..15s"; return 1; fi
    ok "pool-bound: 4 runs at a pool of 2 ran exactly two at a time, ${el}s wall"
}

case_pool_any_slot() { # the queue head takes whichever slot frees first
    local lib="$1" ns a b c s
    ns="$(new_ns)"; : > "$ns/events"
    spawn_pool "$ns" "$lib" run long 12; a=$SPID
    await_enter "$ns/events" long 10 >/dev/null
    spawn_pool "$ns" "$lib" run short 2; b=$SPID
    await_enter "$ns/events" short 10 >/dev/null
    spawn_pool "$ns" "$lib" run next 1; c=$SPID
    s="$(await_enter "$ns/events" next 8)"
    bounded_wait 40 "$a" "$b" "$c"
    if [ -z "$s" ]; then
        bad "pool-any-slot: 'next' did not enter within 8s — it waited on the long-held slot, not the free one"
        return 1; fi
    ok "pool-any-slot: the freed slot was taken in ${s}s while the other stayed held"
}

case_pool_gate_first() { # both slots busy, two runs queued, a gate arrives last: the gate goes next
    local lib="$1" ns pids=() order
    ns="$(new_ns)"; : > "$ns/events"
    spawn_pool "$ns" "$lib" run h1 4; pids+=("$SPID")
    spawn_pool "$ns" "$lib" run h2 4; pids+=("$SPID")
    await_enter "$ns/events" h2 10 >/dev/null; await_enter "$ns/events" h1 10 >/dev/null
    spawn_pool "$ns" "$lib" run q1 1; pids+=("$SPID"); sleep 1
    spawn_pool "$ns" "$lib" run q2 1; pids+=("$SPID"); sleep 1
    spawn_pool "$ns" "$lib" gate G 1; pids+=("$SPID")
    bounded_wait 60 "${pids[@]}"
    order="$(enter_order "$ns/events")"
    case "$order" in
        "h1 h2 G "*|"h2 h1 G "*) ok "pool-gate-first: served '$order' — the gate went before both queued runs" ;;
        *) bad "pool-gate-first: served '$order', wanted the gate third"; return 1 ;;
    esac
}

case_pool_one_per_tree() { # a pool of two still keeps one tree to one run
    local lib="$1" ns pids=() o
    ns="$(new_ns)"; : > "$ns/events"; mkdir -p "$ns/shared"
    spawn_pool "$ns" "$lib" run t1 3 "$ns/shared"; pids+=("$SPID")
    spawn_pool "$ns" "$lib" run t2 3 "$ns/shared"; pids+=("$SPID")
    bounded_wait 40 "${pids[@]}"
    [ "$(n_enters "$ns/events")" -eq 2 ] || { bad "pool-one-per-tree: not both runs entered"; return 1; }
    o="$(overlaps "$ns/events")"
    if [ -n "$o" ]; then bad "pool-one-per-tree: two runs of ONE tree overlapped — $o"; return 1; fi
    ok "pool-one-per-tree: two runs of one tree serialised although a slot was free"
}

# ===========================================================================
# The hold (2026-09-30). `ci/handoff.sh` runs stage 6 and stage 7 as two
# harnesses inside ONE `corpus-lock.sh hold`; a run queued behind the gate must
# not get in between them, and the variable that carries the hold must mean
# nothing outside it.
# ===========================================================================

cat > "$WORK/holdcmd.sh" <<'HOLDCMD'
#!/usr/bin/env bash
# Two harnesses back to back with a gap between them, as stages 6 and 7 are.
"$PAYLOAD" A 3 || exit 1
sleep 2
"$PAYLOAD" B 3
HOLDCMD
chmod +x "$WORK/holdcmd.sh"

case_hold_spans_harnesses() { # a run queued during the hold enters after BOTH inner harnesses
    local lib="$1" ns h r order
    ns="$(new_ns)"; : > "$ns/events"
    RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" EVENTS="$ns/events" \
        TREE="$ROOT" RUSTMC_CORPUS_LOCK_ROLE=gate CORPUS_LOCK_HEARTBEAT=2 PAYLOAD="$WORK/payload.sh" \
        bash "$lib" hold -- "$WORK/holdcmd.sh" > "$ns/log.hold" 2>&1 &
    h=$!
    await_enter "$ns/events" A 10 >/dev/null
    spawn "$ns" "$lib" run R 1; r=$SPID
    bounded_wait 40 "$h" "$r"
    order="$(enter_order "$ns/events")"
    if [ "$order" != "A B R " ]; then
        bad "hold-spans-harnesses: served '$order', wanted 'A B R ' — the queued run got in between the hold's harnesses"
        return 1; fi
    ok "hold-spans-harnesses: a run queued during the hold waited out both inner harnesses and the gap"
}

case_held_by_outside_the_hold() { # a copied RUSTMC_CORPUS_LOCK_HELD_BY must not admit a stranger
    local lib="$1" ns a b anc st o
    ns="$(new_ns)"; : > "$ns/events"
    spawn "$ns" "$lib" run owner 6; a=$SPID
    await_enter "$ns/events" owner 10 >/dev/null
    anc="$(cat "$ns/anchor.owner" 2>/dev/null)"
    st="$(s="$(cat "/proc/$anc/stat" 2>/dev/null)"; s="${s##*) }"; printf '%s\n' "$s" | awk '{print $20}')"
    # A live anchor that really holds slot 0 — only its ancestry is wrong.
    export RUSTMC_CORPUS_LOCK_HELD_BY="$anc $st 0"
    spawn "$ns" "$lib" run stranger 1; b=$SPID
    unset RUSTMC_CORPUS_LOCK_HELD_BY
    bounded_wait 40 "$a" "$b"
    [ "$(n_enters "$ns/events")" -eq 2 ] || { bad "held-by-outside-the-hold: not both entered"; return 1; }
    o="$(overlaps "$ns/events")"
    if [ -n "$o" ]; then bad "held-by-outside-the-hold: a stranger carrying another run's anchor ran beside it — $o"; return 1; fi
    ok "held-by-outside-the-hold: a stranger naming a live, holding anchor it is not inside still queued"
}

# A gate drains the pool (2026-10-01, `lock-pool-lanes`). Lanes run three at
# once in their own trees; a gate arriving while all three run waits for every
# one of them, records alone, and a lane queued after it goes after it. The
# gate is started with NO pool setting at all — the default — because that is
# how `ci/handoff.sh` starts it, while the lanes ask for three.
spawn_pool_n() { # spawn_pool_n <ns> <lib> <role> <id> <hold> <pool|-> [tree]
    local ns="$1" lib="$2" role="$3" id="$4" hold="$5" pool="$6" tree="${7:-$1/tree.$4}"
    mkdir -p "$tree"
    if [ "$pool" = - ]; then
        env -u RUSTMC_CORPUS_LOCK_POOL RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" \
            EVENTS="$ns/events" TREE="$tree" RUSTMC_CORPUS_LOCK_ROLE="$role" CORPUS_LOCK_HEARTBEAT=2 \
            "$WORK/payload.sh" "$id" "$hold" > "$ns/log.$id" 2>&1 &
    else
        RUSTMC_CORPUS_LOCK_POOL="$pool" RUSTMC_CORPUS_LOCK_DIR="$ns" CORPUS_LOCK_LIB="$lib" \
            EVENTS="$ns/events" TREE="$tree" RUSTMC_CORPUS_LOCK_ROLE="$role" CORPUS_LOCK_HEARTBEAT=2 \
            "$WORK/payload.sh" "$id" "$hold" > "$ns/log.$id" 2>&1 &
    fi
    SPID=$!
}

case_pool_gate_exclusive() { # three lanes overlap; a gate overlaps none of them; a later lane waits for the gate
    local lib="$1" ns pids=() verdict
    ns="$(new_ns)"; : > "$ns/events"
    for i in 1 2 3; do spawn_pool_n "$ns" "$lib" run "L$i" 5 3; pids+=("$SPID"); done
    for i in 1 2 3; do await_enter "$ns/events" "L$i" 10 >/dev/null; done
    spawn_pool_n "$ns" "$lib" gate G 2 -; pids+=("$SPID"); sleep 1
    spawn_pool_n "$ns" "$lib" run L4 1 3; pids+=("$SPID")
    bounded_wait 60 "${pids[@]}"
    verdict="$(python3 - "$ns/events" <<'PY'
import sys
ev = {}
for line in open(sys.argv[1]):
    k, i, t = line.split()
    ev.setdefault(i, {})[k] = float(t)
need = {"L1", "L2", "L3", "L4", "G"}
if not need <= {i for i, v in ev.items() if "ENTER" in v and "EXIT" in v}:
    print(f"not every run entered and exited: {sorted(ev)}"); sys.exit()
g = ev["G"]
hit = [i for i in ev if i != "G" and ev[i]["ENTER"] < g["EXIT"] and g["ENTER"] < ev[i]["EXIT"]]
if hit:
    print(f"the gate overlapped lane(s) {hit}"); sys.exit()
first = [ev[i] for i in ("L1", "L2", "L3")]
if not max(v["ENTER"] for v in first) < min(v["EXIT"] for v in first):
    print("the three lanes did not all run at once"); sys.exit()
if not ev["L4"]["ENTER"] >= g["EXIT"]:
    print("the lane queued after the gate went before it"); sys.exit()
print("ok")
PY
)"
    if [ "$verdict" != ok ]; then bad "pool-gate-exclusive: $verdict"; return 1; fi
    ok "pool-gate-exclusive: 3 lanes at once, the gate (default pool) waited all three out and ran alone, the next lane after it"
}

run_suite() {
    local lib="$1"
    case_exclusion                    "$lib"
    case_waits_not_fails              "$lib"
    case_gate_priority                "$lib"
    case_gate_beats_crowd             "$lib"
    case_gates_serialise              "$lib"
    case_kill9_frees                  "$lib"
    case_kill9_after_waiting          "$lib"
    case_kill9_with_live_server       "$lib"
    case_anchor_is_sole_holder        "$lib"
    case_child_cannot_inherit         "$lib"
    case_dead_anchor_refused          "$lib"
    case_anchor_failure_refuses       "$lib"
    case_stale_record_is_pid_not_timeout "$lib"
    case_observable                   "$lib"
    case_dead_waiter_reaped           "$lib"
    case_fifo_no_barge                "$lib"
    case_fifo_service_order           "$lib"
    case_gate_beats_the_queue         "$lib"
    case_two_gates_beat_the_queue     "$lib"
    case_burst                        "$lib"
    case_post_acquire_hand_back       "$lib"
    case_dead_waiter_no_stall         "$lib"
    case_queue_survives_a_death       "$lib"
    case_pool_bound                   "$lib"
    case_pool_any_slot                "$lib"
    case_pool_gate_first              "$lib"
    case_pool_one_per_tree            "$lib"
    case_hold_spans_harnesses         "$lib"
    case_held_by_outside_the_hold     "$lib"
    case_pool_gate_exclusive          "$lib"
}

# ===========================================================================
# Injections
# ===========================================================================
inject() { # inject <name> <perl-expr...>  -> patched lib path, or STALE/COMMENT-ONLY
    local name="$1"; shift
    local out="$WORK/lib.$name.sh" s
    cp "$LIB_REAL" "$out"
    for s in "$@"; do perl -0pi -e "$s" "$out"; done
    if cmp -s "$LIB_REAL" "$out"; then printf 'STALE\n'; return 0; fi
    # A PATCH THAT ONLY REWROTE COMMENTS HAS BROKEN NOTHING, and reporting the
    # resulting green as a MISS accuses correct code of a hole it does not have.
    # This is not hypothetical: `anchor-without-pdeathsig` came back MISSED
    # twice. `ci/corpus-lock.sh` documents the anchor by quoting the exact
    # `setpriv --pdeathsig KILL bash -c ...` command line in its header, that
    # quote sits ABOVE the code, and `perl -0pe s///` without /g rewrites only
    # the FIRST match — so the injection patched the prose and left the call
    # site alone. `cmp` saw a changed file and was satisfied. Two hours of
    # chasing a defect that was never in the code.
    # `diff` exits 1 for "files differ", and this script runs under `pipefail`,
    # so putting it at the head of a pipeline makes the WHOLE pipeline exit 1
    # whatever the greps decide. Written that way first, and it reported
    # COMMENT-ONLY for all thirteen injections — an implausible number is the
    # only reason it was caught in one run rather than believed. The diff is
    # captured first, and the decision is a single grep with no pipeline.
    local changed
    changed="$(diff "$LIB_REAL" "$out" | grep -E '^[<>]' || true)"
    if ! grep -qvE '^[<>][[:space:]]*(#|$)' <<<"$changed"; then
        printf 'COMMENT-ONLY\n'; return 0
    fi
    printf '%s\n' "$out"
}

# `CORPUS_LOCK_FALSIFY_DRYRUN=1` applies every patch and runs no case. It exists
# because the expensive half of this sweep proves the LIBRARY and the cheap half
# proves the SWEEP: a patch that has stopped matching is a hole whether or not
# the box has twenty minutes spare, and the refactor that added the run queue
# made four of these regexes stale in one commit. Two seconds, and it is the
# check to run after touching `ci/corpus-lock.sh`.
check_injection() { # check_injection <name> <lib> <case-fn...>
    local name="$1" lib="$2"; shift 2
    if [ -n "${CORPUS_LOCK_FALSIFY_DRYRUN:-}" ]; then
        case "$lib" in
            STALE|COMMENT-ONLY)
                printf '  PATCH-DEAD  %-38s %s\n' "$name" "$lib"
                MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name ($lib)") ;;
            *)  printf '  patch-ok    %-38s -> %s\n' "$name" "${lib##*/}" ;;
        esac
        return
    fi
    if [ "$lib" = "STALE" ]; then
        printf '  MISSED  %s  <-- the patch did not apply; it is stale, not the code\n' "$name"
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name (patch did not apply)"); return
    fi
    if [ "$lib" = "COMMENT-ONLY" ]; then
        printf '  MISSED  %s  <-- the patch hit only comments, so it broke nothing\n' "$name"
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name (patch hit only comments)"); return
    fi
    local before=$FAIL c
    for c in "$@"; do "$c" "$lib" >/dev/null 2>&1; done
    if [ "$FAIL" -gt "$before" ]; then
        printf '  CAUGHT  %s\n' "$name"
        FAIL=$before
        if [ "$before" -eq 0 ]; then FAILED_CASES=(); else
            FAILED_CASES=("${FAILED_CASES[@]:0:$before}"); fi
    else
        printf '  MISSED  %s  <-- the suite did not notice this break\n' "$name"
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name")
    fi
}

say "== control, before the sweep"
if [ -z "${CORPUS_LOCK_FALSIFY_DRYRUN:-}" ]; then run_suite "$LIB_REAL"; else say "  (dry run: patches only, no cases)"; fi
CONTROL_BEFORE=$FAIL

say ""
# The COMMENT-ONLY guard is itself an instrument, and it lied on its first run.
# So it is checked against a patch of each kind before it is trusted to judge
# thirteen others.
say "== self-test: the COMMENT-ONLY guard"
GUARD_OK=1
if [ "$(inject selftest_comment 's/^# corpus-lock\.sh — one corpus run.*$/# corpus-lock.sh — REWRITTEN COMMENT/m')" = "COMMENT-ONLY" ]; then
    say "  PASS  a comment-only patch is reported COMMENT-ONLY"
else
    say "  FAIL  a comment-only patch was NOT reported COMMENT-ONLY"; GUARD_OK=""; fi
if [ "$(inject selftest_code 's/^_cl_now\(\)  \{ date \+%s; \}$/_cl_now()  { date +%s; }  # touched/m')" = "COMMENT-ONLY" ]; then
    say "  FAIL  a real code patch was wrongly reported COMMENT-ONLY"; GUARD_OK=""
else
    say "  PASS  a real code patch is not reported COMMENT-ONLY"; fi
if [ -z "$GUARD_OK" ]; then
    say "  the injection sweep below cannot be trusted while the guard is wrong"
    FAIL=$((FAIL+1)); FAILED_CASES+=("COMMENT-ONLY guard self-test")
fi
say ""

say "== injections (each MUST go red)"

# 1. No mutual exclusion at all.
check_injection "no-mutex-at-all" \
    "$(inject nomutex 's/flock -x -n "\$_CL_RUN_FD"/true/g; s/^_cl_wait_run_lock\(\).*$/_cl_wait_run_lock() { true; }/m')" \
    case_exclusion

# 2. The mutex taken shared instead of exclusive — the classic wrong flag.
check_injection "shared-instead-of-exclusive" \
    "$(inject shared 's/flock -x -n "\$_CL_RUN_FD"/flock -s -n "\$_CL_RUN_FD"/g; s/flock -x "\$_CL_RUN_FD"/flock -s "\$_CL_RUN_FD"/g')" \
    case_exclusion

# 3. Waiting stops being the default: the acquire gives up instead.
check_injection "acquire-aborts-instead-of-waiting" \
    "$(inject abort 's/^_cl_wait_run_lock\(\).*$/_cl_wait_run_lock() { flock -x -n "\$_CL_RUN_FD" || exit 7; }/m')" \
    case_waits_not_fails

# 4. The gate-priority hand-back is removed: a run that wins the race keeps it.
#    Aimed at the predicate rather than at the shape of the `if` around it —
#    the shape moved once already, when the queue check was added below it, and
#    an injection pinned to punctuation is one refactor away from STALE.
check_injection "no-gate-yield" \
    "$(inject noyield 's/^_cl_gate_declared\(\) \{/_cl_gate_declared() { return 1;/m')" \
    case_gate_priority case_gate_beats_crowd case_gate_beats_the_queue case_two_gates_beat_the_queue

# 5. The gate never declares intent, so nothing can know to yield to it.
#    Both the fast path and the blocking path must be neutered — patching only
#    the blocking one leaves the fast path still declaring intent, and the
#    first version of this sweep scored that as a MISS when the suite was right.
check_injection "gate-declares-no-intent" \
    "$(inject nointent 's/flock -x -n "\$_CL_GATE_FD"/true/g; s/^_cl_declare_gate_intent\(\).*$/_cl_declare_gate_intent() { true; }/m')" \
    case_gate_priority case_gate_beats_crowd case_gate_beats_the_queue case_two_gates_beat_the_queue

# 6. Liveness always says "alive" — the timeout-shaped mistake this design
#    exists to avoid.
check_injection "liveness-always-true" \
    "$(inject alwaysalive 's/^_cl_alive\(\) \{/_cl_alive() { return 0;/m')" \
    case_stale_record_is_pid_not_timeout case_dead_waiter_reaped

# 7. The heartbeat ticker keeps its inherited lock fds AND is never killed. It
#    shares the open file description, so it holds the lock after the run that
#    made it has gone.
#
#    Both halves are needed and finding that out cost a MISSED. Removing only
#    the fd close is inert under the anchor design, because the ticker is killed
#    a moment later anyway — so the sweep reported it uncaught, correctly. That
#    is not a hole in the suite; it says the fd close is defence in depth behind
#    the kill. This injection removes the defence AND the thing it backs up,
#    which is the claim actually worth testing: no path leaves an fd-holding
#    process alive. Only a run that QUEUED ever has a ticker, hence
#    case_kill9_after_waiting.
check_injection "ticker-outlives-the-run" \
    "$(inject leakyticker 's/eval "exec \$_CL_RUN_FD>&- \$_CL_GATE_FD>&- \$_CL_WAIT_FD>&-" 2>\/dev\/null \|\| true  # ticker drops every lock fd/true/; s/^_cl_stop_ticker\(\) \{/_cl_stop_ticker() { return 0;/m')" \
    case_kill9_after_waiting

# 8. No anchor at all: the harness holds the lock itself, so its children
#    inherit it and outlive it. This is the design the first draft had.
check_injection "no-anchor-harness-holds-the-fds" \
    "$(inject noanchor 's/^_cl_anchor_start\(\) \{/_cl_anchor_start() { return 0;/m')" \
    case_kill9_with_live_server case_anchor_is_sole_holder case_child_cannot_inherit

# 9. The anchor is forked but the harness keeps its own copies of the fds, so
#    every later child still inherits the lock.
check_injection "harness-keeps-fds-after-anchoring" \
    "$(inject keepfds 's/    eval "exec \$_CL_RUN_FD>&- \$_CL_GATE_FD>&-" 2>\/dev\/null \|\| true\n    _cl_gate_drop_all\n\}/    true\n}/')" \
    case_kill9_with_live_server case_child_cannot_inherit

# 10. The anchor has no --pdeathsig, so it survives the run that made it and
#     holds the lock forever. This is the wedge requirement 3 forbids.
check_injection "anchor-without-pdeathsig" \
    "$(inject nopdeath 's/setpriv --pdeathsig KILL bash -c/setsid bash -c/g')" \
    case_kill9_frees

# 11. The anchored-ness assertion is disarmed, so a run whose lock has gone
#     free carries on measuring as if it had not.
check_injection "assert-anchored-always-passes" \
    "$(inject noassert 's/^corpus_lock_assert_anchored\(\) \{/corpus_lock_assert_anchored() { return 0;/m')" \
    case_dead_anchor_refused

# 12. The anchor is never verified before the harness drops its own fds, so a
#     failed setpriv leaves the run holding nothing while believing it holds
#     the box. Ordering bug, and the most dangerous one available here.
#     BOTH guards, and that is the finding rather than a convenience. Removing
#     the pre-drop verification ALONE is still caught — by
#     `corpus_lock_assert_anchored`, which the harness calls moments later — so
#     the run still refuses and the sweep scored it MISSED. That is not a blind
#     suite: it is two independent guards, and the honest claim is "a failed
#     anchor is caught by at least one of them". Removing both is what tests it,
#     and with both gone a run proceeds having measured a whole corpus while
#     holding nothing.
check_injection "both-anchor-guards-removed" \
    "$(inject noverify 's/    if ! kill -0 "\$_CL_ANCHOR" 2>\/dev\/null \|\| \[ "\$have" != "\$want" \]; then/    if false; then/; s/^corpus_lock_assert_anchored\(\) \{/corpus_lock_assert_anchored() { return 0;/m')" \
    case_anchor_failure_refuses

# 13. The wait goes silent.
check_injection "silent-wait" \
    "$(inject silent 's/^(\s*)echo "corpus lock: WAITING.*$/${1}true/gm; s/^(\s*)echo "corpus lock:   held by .*$/${1}true/gm')" \
    case_observable

# --- fairness among runs, added 2026-08-12 ---------------------------------
#
# Four properties were claimed; each of the six below removes one and must turn
# a case red. Two of them are the hazards the item was briefed with by name —
# a yield loop that livelocks, and a dead waiter that blocks the queue — and
# both are reproduced here on purpose rather than argued away in a comment.

# 14. The order is never consulted: a queued run is never seen as being ahead of
#     anybody. This is the code as it stood on 2026-08-11, in one line.
check_injection "no-fifo-ordering" \
    "$(inject nofifo 's/^_cl_older_run_ahead\(\) \{/_cl_older_run_ahead() { return 0;/m')" \
    case_fifo_no_barge case_fifo_service_order

# 15. The fast path stops looking at the queue. This is the actual 2026-08-11
#     defect and nothing else: everything else about the ordering still works,
#     and an arrival that finds the mutex free still walks past a queue that has
#     been there for two hours.
check_injection "fast-path-barges-past-the-queue" \
    "$(inject barge 's/^_cl_any_live_waiter\(\) \{/_cl_any_live_waiter() { return 1;/m')" \
    case_fifo_no_barge

# 16. THE LIVELOCK THE BRIEF NAMED. The relation stops being "strictly older"
#     and becomes "anybody else", so it is no longer antisymmetric and two
#     waiters can each be ahead of the other. Because a yield BLOCKS rather than
#     spins, this surfaces as a deadlock — which is the honest shape of it: with
#     a blocking yield, a broken order cannot burn CPU forever, it stops. That
#     is a better failure than a livelock and it is still a failure.
check_injection "yield-to-any-run-not-just-older" \
    "$(inject anyrun 's/        _cl_is_older "\$_CL_R_seq" "\$_CL_R_pid" "\$myseq" "\$mypid" \|\| continue/        true/')" \
    case_fifo_service_order case_burst

# 17. The mutex is held across the park. The one ordering constraint the queue
#     adds, removed: we park on a run that is itself blocked on the mutex we are
#     holding, and neither of us can move.
#
#     THIS ONE CAME BACK MISSED ON ITS FIRST RUN and the reason is the finding,
#     not the fix: no case in this file could reach the branch it breaks,
#     because the pre-contend park means a run at the back of the queue never
#     touches the mutex at all. `case_post_acquire_hand_back` was written for
#     it, with a fixture that constructs an older place taken while somebody
#     else is already blocked. An injection whose case list is "everything that
#     sounds related" is how a hole gets reported as a pass.
check_injection "park-while-holding-the-mutex" \
    "$(inject parkheld 's/                    flock -u "\$_CL_RUN_FD"   # RELEASE BEFORE PARKING — see header/                    true/')" \
    case_post_acquire_hand_back

# 18. The yield stops blocking and starts polling. Everything still completes,
#     so every other assertion in this file is satisfied — this is exactly the
#     shape of defect the CPU bound in `case_burst` was added for.
check_injection "park-does-not-block" \
    "$(inject spinpark 's/        flock -s "\$_CL_PROBE_FD" 2>\/dev\/null \|\| true/        true/')" \
    case_burst

# 19. Both answers to "the waiter is dead" removed at once — PID liveness always
#     says alive, and the kernel is no longer asked whether the token is still
#     held. Either one alone is caught by the other, which is a finding rather
#     than a gap (the same shape as injection 12), so the injection that tests
#     the claim has to remove both. With both gone, a run parks behind a corpse
#     whose token the kernel already released, finds it still "ahead", and goes
#     round again forever.
check_injection "dead-waiter-blocks-the-queue" \
    "$(inject deadblocks 's/^_cl_alive\(\) \{/_cl_alive() { return 0;/m; s/^_cl_record_is_queued\(\) \{/_cl_record_is_queued() { return 0;/m')" \
    case_dead_waiter_no_stall

check_injection "pool-admits-one-more-than-its-bound" \
    "$(inject poolplus1 's/^(_cl_try_slots\(\) \{\n    local k\n    for k in \$\(seq 0 \$\(\(CORPUS_LOCK_POOL) - 1\)\)\)/${1})))/m')" \
    case_pool_bound

check_injection "pool-waits-on-slot-zero-only" \
    "$(inject slotzero 's/^_cl_wait_run_lock\(\)  \{ if \[ "\$CORPUS_LOCK_POOL" -gt 1 \]; then _cl_wait_any_slot; else/_cl_wait_run_lock()  { if false; then _cl_wait_any_slot; else/m')" \
    case_pool_any_slot

check_injection "pool-without-the-tree-lock" \
    "$(inject notree 's/^_cl_tree_lock\(\) \{/_cl_tree_lock() { return 0;/m')" \
    case_pool_one_per_tree

# The hold (2026-09-30). The first is the defect the hold exists for: the
# gate's two harnesses each acquiring for themselves, which is exactly what
# `ci/handoff.sh` did before it. The second removes only the ancestry test,
# so a live anchor that really holds the slot is trusted by a stranger.
check_injection "hold-takes-no-lock" \
    "$(inject holdnolock 's/            corpus_lock_acquire\n            RUSTMC_CORPUS_LOCK_HELD_BY=/            RUSTMC_CORPUS_LOCK_HELD_BY=/')" \
    case_hold_spans_harnesses

check_injection "held-by-without-ancestry" \
    "$(inject noancestry 's/\[ -n "\$ppid" \] && \[\[ .* \]\] && return 0/return 0/')" \
    case_held_by_outside_the_hold

# The gate takes one slot like any run — the pool as it was before
# 2026-10-01: a gate recording beside lanes.
check_injection "gate-takes-one-slot" \
    "$(inject gateoneslot 's/_cl_gate_take_all -n; then/_cl_try_slots; then/; s/            _cl_gate_take_all\n/            _cl_wait_run_lock\n/')" \
    case_pool_gate_exclusive

say ""
say "== control, after the sweep"
[ -n "${CORPUS_LOCK_FALSIFY_DRYRUN:-}" ] || run_suite "$LIB_REAL"

say ""
say "=================================================================="
printf 'cases: %d passed, %d failed  (control before sweep: %d failed)\n' \
    "$PASS" "$FAIL" "$CONTROL_BEFORE"
printf 'injections: %d not caught\n' "$MISSED"
if [ "$FAIL" -ne 0 ]; then say "FAILED CASES:"; printf '  - %s\n' "${FAILED_CASES[@]}"; fi
if [ "$MISSED" -ne 0 ]; then
    say "INJECTIONS NOT CAUGHT (holes in this suite — report every one):"
    printf '  - %s\n' "${MISSED_INJECTIONS[@]}"
fi
if [ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]; then
    say "CORPUS-LOCK-FALSIFY GREEN"
    exit 0
fi
exit 1
