#!/usr/bin/env bash
# ci/gate-lock-falsify.sh — prove the gate mutex by breaking it.
#
#     ./ci/gate-lock-falsify.sh
#
# Six cases and six injections, driving the **real `ci/handoff.sh`** in throwaway
# git repos with a fake `ssh` on PATH, a private lock namespace and a private
# anchor name, so this can neither touch the box nor collide with a real gate.
#
# **The injections are the point, not the cases.** A green case only says the
# instrument agreed with the code; an injection says the instrument would have
# noticed had the code been wrong. So every case here is required to go red
# under at least one deliberate break, and an injection that is NOT caught is
# reported as loudly as a failure.
#
# This project's recurring defect, past twenty-five instances, is "a check that
# could not fail", and the gate forensics of 2026-08-07 produced another one in
# its own first instrument: scoped to `target/debug/deps` it called 37 of 61 gate
# runs "compiled nothing", including a run that had demonstrably built 4,074
# artefacts visible only under `incremental/`. That is why the census case below
# asserts exact counts in both directions — a real write and a genuine no-op —
# rather than only "greater than zero".
#
# WHAT IS FAKE AND WHAT IS REAL, stated so nobody has to guess:
#
#   * REAL — `ci/handoff.sh`, unmodified, including its lock block; `ci/gate-lock.sh`,
#     unmodified; `flock`; `setpriv --pdeathsig`; the anchor; two concurrent
#     `handoff.sh` processes racing for one key; `kill -9` of a live gate; and
#     the whole provenance stage, which the fake `ssh` **executes for real**
#     against a real directory tree, so the stamp is written and read and the
#     census is a real `find` over real files with real mtimes.
#   * FAKE — the box. The compile stages are stand-ins, because `handoff.sh`
#     learns what a stage did from exactly two things, its exit status and what
#     it wrote to the log, and a stand-in producing the real status and the real
#     sentence is indistinguishable to it. What this cannot test is whether a
#     real cargo run writes into `target/`; that is proven separately on the box
#     and recorded in the commit message.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB_REAL="$ROOT/ci/gate-lock.sh"
HANDOFF_REAL="$ROOT/ci/handoff.sh"
[ -r "$LIB_REAL" ] && [ -r "$HANDOFF_REAL" ] || { echo "no ci/gate-lock.sh or ci/handoff.sh beside this script"; exit 2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/gate-lock-falsify.XXXXXX")"
# Private anchor name, never the production one: injections deliberately leave
# anchors behind, and cleaning those up by the production name would kill the
# anchor of a REAL merge gate running on this machine.
export GATE_LOCK_ANCHOR_NAME="gate-lock-falsify-anchor-$$"
cleanup() {
    pkill -f "$GATE_LOCK_ANCHOR_NAME" >/dev/null 2>&1
    pkill -f "$WORK" >/dev/null 2>&1
    sleep 0.3
    rm -rf "$WORK"
}
trap cleanup EXIT

VERBOSE=""
[ "${1:-}" = "-v" ] && VERBOSE=1
PASS=0; FAIL=0; MISSED=0
declare -a FAILED_CASES=() MISSED_INJECTIONS=()
ok()  { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); FAILED_CASES+=("$*"); printf '  FAIL  %s\n' "$*"; }
note(){ [ -n "$VERBOSE" ] && printf '%s\n' "$*" | sed 's/^/        | /'; return 0; }

# --- the fake box -----------------------------------------------------------
#
# One `ssh` for every route `handoff.sh` has to the box: `remote_script` (a
# script on stdin — stage 1 AND the provenance stage), `remote` (stages 3, 5, 6)
# and the bare ssh inside `t0-remote.sh --run`, stood in for below.
#
# The provenance stage is NOT canned. Its script is handed to a real `bash`
# with the real command string, so `$HOME` and the `cd` resolve inside this
# sandbox and the stamp and the census are real filesystem operations. That is
# deliberate: a canned census could not be wrong, and this file exists to be
# able to be wrong.
build_ssh() {
    mkdir -p "$WORK/bin"
    cat > "$WORK/bin/ssh" <<'EOS'
#!/usr/bin/env bash
cmd="${*: -1}"
case "$cmd" in
*"bash -s"*)
    script="$(cat)"
    case "$script" in
    *rustmc-gate-provenance*)                 # provenance: run it for real
        printf '%s\n' "$script" | bash -c "$cmd" ;;
    *)                                        # stage 1: prepare
        echo "== stage 1: gate dir owns its own oracle-work and data/vanilla"
        echo "   12 references linked, gameplay is a directory"
        echo "STAGE1 OK" ;;
    esac
    exit 0 ;;
*gen-gameplay-tables.sh*) echo "gen-gameplay-tables: 12 tables current"; exit 0 ;;
*t1-gate.sh*)             echo "T1-GATE GREEN — 99 test binaries"; exit 0 ;;
*gate-corpus-stages.sh*) sleep "${GL_HOLD:-0}"; echo "T2-GAMEPLAY GREEN"; echo "GATE-STAGE-RC 6 0"; exit 0 ;;
*t2-gameplay.sh*)         sleep "${GL_HOLD:-0}"; echo "T2-GAMEPLAY GREEN"; exit 0 ;;
esac
echo "fake ssh: unrecognised command: $cmd" >&2; exit 127
EOS
    chmod +x "$WORK/bin/ssh"
}

# --- a throwaway repo -------------------------------------------------------
#
# `main`, then two commits on `under-test`, each adding a Rust file, so
# `rust-files-changed` in the provenance line is a real `git diff` over real
# blobs rather than a constant.
build_repo() { # build_repo <name> <lib> <handoff>
    # Separate statements, not one `local a=$1 b=$WORK/$a`. Bash expands every
    # word of a `local` before the builtin assigns any of them, so a forward
    # reference there is empty — and under `set -u` it aborts the function, the
    # command substitution yields "", and `cd ""` succeeds and leaves the run in
    # *this* directory. That is exactly what happened on this file's first run:
    # a case reported PASS while driving the real repo instead of a throwaway.
    local name="$1" lib="$2" hand="$3"
    local R="$WORK/repos/$name"
    rm -rf "$R"; mkdir -p "$R/ci"
    git init -q -b main "$R"
    cp "$hand" "$R/ci/handoff.sh"; chmod +x "$R/ci/handoff.sh"
    cp "$lib"  "$R/ci/gate-lock.sh"
    cat > "$R/ci/t0-remote.sh" <<'EOS'
#!/usr/bin/env bash
case "${1:-}" in
--sync) echo "t0-remote: rsync to ${RUSTMC_REMOTE:-?} ok"; exit 0 ;;
--run)  echo "T0 GREEN"; exit 0 ;;
esac
echo "usage: t0-remote.sh [--sync|--run]" >&2; exit 2
EOS
    chmod +x "$R/ci/t0-remote.sh"
    ( cd "$R" \
      && git add ci/handoff.sh ci/gate-lock.sh ci/t0-remote.sh \
      && git -c user.name=t -c user.email=t@t commit -qm "gate scripts" \
      && git checkout -q -b under-test \
      && mkdir -p src && echo 'pub fn a() {}' > src/a.rs && git add src/a.rs \
      && git -c user.name=t -c user.email=t@t commit -qm "one" \
      && echo 'pub fn b() {}' > src/b.rs && git add src/b.rs \
      && git -c user.name=t -c user.email=t@t commit -qm "two" \
      && git checkout -q main ) >/dev/null 2>&1
    printf '%s\n' "$R"
}

# --- one gate run -----------------------------------------------------------
#
# `--dry-run`, so nothing merges and nothing pushes. `env -i` so the run cannot
# inherit anything from this shell except what is named. `HOME` is what makes
# the gate log AND the fake box directory land inside the sandbox; two runs
# sharing a HOME share a box directory, which is the whole subject here.
GPID=""; GOUT=""
gate() { # gate <repo> <ref> <boxdir> <lockdir> <home> <hold> [--bg]
    local R="$1" ref="$2" box="$3" lock="$4" home="$5" hold="$6" bg="${7:-}"
    # A sandbox that failed to build must stop the sweep, never silently run the
    # gate from wherever this script happens to be. See build_repo.
    [ -n "$R" ] && [ -x "$R/ci/handoff.sh" ] || {
        echo "  BROKEN SWEEP: no throwaway repo at '${R:-}'"; exit 2; }
    mkdir -p "$home"
    if [ "$bg" = "--bg" ]; then
        # `exec`, so $! is the gate itself and not a subshell that forked it.
        # Without it `kill -9 $GPID` kills the wrapper, handoff.sh is merely
        # orphaned and keeps running, and the lock is *correctly* still held —
        # a case that reads as a wedge and is really a test bug. It read that
        # way on this file's second run.
        ( cd "$R" && exec env -i PATH="$WORK/bin:/usr/bin:/bin" HOME="$home" TERM=dumb \
            GATE_LOCK_ANCHOR_NAME="$GATE_LOCK_ANCHOR_NAME" \
            RUSTMC_GATE_LOCK_DIR="$lock" RUSTMC_REMOTE=fakehost \
            RUSTMC_REMOTE_DIR="$box" GL_HOLD="$hold" \
            ./ci/handoff.sh "$ref" --dry-run > "$WORK/bg.out" 2>&1 ) &
        GPID=$!
        return 0
    fi
    GOUT="$(cd "$R" && env -i PATH="$WORK/bin:/usr/bin:/bin" HOME="$home" TERM=dumb \
            GATE_LOCK_ANCHOR_NAME="$GATE_LOCK_ANCHOR_NAME" \
            RUSTMC_GATE_LOCK_DIR="$lock" RUSTMC_REMOTE=fakehost \
            RUSTMC_REMOTE_DIR="$box" GL_HOLD="$hold" \
            ./ci/handoff.sh "$ref" --dry-run 2>&1)"
    return $?
}

# Wait until the background gate actually owns the box key, asked of the kernel
# rather than inferred from a sleep. A `sleep 2` here would be a race that
# passes on a quiet laptop and fails on a busy one, and the failure would look
# like a lock bug.
wait_until_held() { # wait_until_held <lockdir> <boxkey> <secs>
    local lock="$1" key="$2" n=$(( ${3:-10} * 20 ))
    # The probe opens the lock file, so a directory the gate has not created yet
    # makes the open fail — and a failed open is indistinguishable from a failed
    # flock unless the file is made to exist first. Without this the wait
    # returned instantly on the very first poll, the second gate was launched
    # before the first had acquired anything, and the case failed for a reason
    # that had nothing to do with the lock. `>>` cannot truncate, and flock is
    # on the inode, so pre-creating it changes nothing for the gate.
    mkdir -p "$lock" && : >> "$lock/$key.lock" || return 1
    while [ "$n" -gt 0 ]; do
        if ! ( exec 210>>"$lock/$key.lock"; flock -x -n 210 ) 2>/dev/null; then return 0; fi
        sleep 0.05; n=$((n-1))
    done
    return 1
}
# Wait until the gate is not merely holding the lock but holding it THROUGH THE
# ANCHOR. The two are half a second apart, and the difference matters: killing
# a gate that has the fds itself proves only that closing an fd releases a
# flock, which is not in question. The case has to kill a gate in the state a
# real one spends its whole life in.
wait_until_anchored() { # wait_until_anchored <lockdir> <boxkey> <secs>
    local lock="$1" key="$2" n=$(( ${3:-10} * 20 )) a
    while [ "$n" -gt 0 ]; do
        a="$(sed -n 's/^anchor=//p' "$lock/$key.holder" 2>/dev/null)"
        if [ -n "$a" ] && kill -0 "$a" 2>/dev/null; then printf '%s\n' "$a"; return 0; fi
        sleep 0.05; n=$((n-1))
    done
    return 1
}
boxkey() { # boxkey <host> <dir>  — the library's own key function, not a copy
    ( . "$LIB_REAL"; _gl_key box "$1:$2" )
}
last_log() { ls -1t "$1"/rustmc-private/gates/*.log 2>/dev/null | head -1; }

# ============================================================================
# THE CASES
# ============================================================================

# DEMONSTRATION 2 — a single gate still runs, and its log now says what it did.
case_single() { # <lib> <handoff> ; 0 = behaved
    local R L H rc lg; L="$WORK/lock.single.$RANDOM"; H="$WORK/home.single.$RANDOM"
    R="$(build_repo single "$1" "$2")"
    gate "$R" under-test gl-scratch-single "$L" "$H" 0; rc=$?
    note "$GOUT"
    [ "$rc" = 0 ] || { echo "      (exit $rc, wanted 0)"; return 1; }
    case "$GOUT" in GREEN*) ;; *) return 1 ;; esac
    lg="$(last_log "$H")"; [ -n "$lg" ] || return 1
    note "$(cat "$lg")"
    grep -q '^handoff: tree=[0-9a-f]\{40\} root=' "$lg" || return 1
    grep -q '^handoff: lock-box=box\.' "$lg" || return 1
    grep -q '^compile-census: ' "$lg" || return 1
    # And the lock is gone afterwards, by the kernel's answer not the record's.
    ( exec 210>>"$L/$(boxkey fakehost gl-scratch-single).lock"; flock -x -n 210 ) 2>/dev/null || return 1
    return 0
}

# DEMONSTRATION 1 — two gates, one box directory. The second must refuse, by
# name, and the first must be unaffected.
case_exclusion_box() { # <lib> <handoff>
    local A B L H rc key
    L="$WORK/lock.box.$RANDOM"; H="$WORK/home.box.$RANDOM"
    A="$(build_repo boxA "$1" "$2")"; B="$(build_repo boxB "$1" "$2")"
    key="$(boxkey fakehost gl-scratch-shared)"
    gate "$A" under-test gl-scratch-shared "$L" "$H" 6 --bg
    wait_until_held "$L" "$key" 15 || { kill -9 "$GPID" 2>/dev/null; return 1; }
    gate "$B" under-test gl-scratch-shared "$L" "$H" 0; rc=$?
    note "$GOUT"
    local second="$GOUT"
    wait "$GPID"; local arc=$?
    note "$(cat "$WORK/bg.out")"
    [ "$rc" = 2 ] || { echo "      (second gate exit $rc, wanted 2)"; return 1; }
    case "$second" in REFUSED*) ;; *) return 1 ;; esac
    # It must NAME the resource and the holder, not merely decline.
    printf '%s\n' "$second" | grep -c 'gl-scratch-shared' >/dev/null || return 1
    printf '%s\n' "$second" | grep -c 'holder: gate on under-test' >/dev/null || return 1
    printf '%s\n' "$second" | grep -c "tree $A" >/dev/null || return 1
    # And it must not have been a verdict about the branch.
    printf '%s\n' "$second" | grep -ci 'has been measured' >/dev/null || return 1
    # The first gate is untouched.
    [ "$arc" = 0 ] || { echo "      (first gate exit $arc, wanted 0)"; return 1; }
    grep -q '^GREEN' "$WORK/bg.out" || return 1
    return 0
}

# The other key: one tree, two DIFFERENT box directories. This is the shape of
# the 2026-08-06 14:36 near-miss, which a box-directory lock alone would permit.
case_exclusion_tree() { # <lib> <handoff>
    local A L H rc key
    L="$WORK/lock.tree.$RANDOM"; H="$WORK/home.tree.$RANDOM"
    A="$(build_repo treeA "$1" "$2")"
    key="$(boxkey fakehost gl-scratch-t1)"
    gate "$A" under-test gl-scratch-t1 "$L" "$H" 6 --bg
    wait_until_held "$L" "$key" 15 || { kill -9 "$GPID" 2>/dev/null; return 1; }
    gate "$A" under-test gl-scratch-t2 "$L" "$H" 0; rc=$?
    note "$GOUT"
    local second="$GOUT"
    wait "$GPID"
    [ "$rc" = 2 ] || { echo "      (second gate exit $rc, wanted 2)"; return 1; }
    printf '%s\n' "$second" | grep -c 'already driving this tree' >/dev/null || return 1
    printf '%s\n' "$second" | grep -c "$A" >/dev/null || return 1
    # The box lock it took first must have been handed back: a refusal that
    # leaves a key held would wedge the next gate for no reason at all.
    ( exec 210>>"$L/$(boxkey fakehost gl-scratch-t2).lock"; flock -x -n 210 ) 2>/dev/null || {
        echo "      (the refused gate left the box key held)"; return 1; }
    return 0
}

# DEMONSTRATION 3 — a gate killed with SIGKILL must not wedge the next one.
# The orphan matters: the fake stage-6 `ssh` is still sleeping when its parent
# dies, exactly as a real ssh waiting out a fifty-minute corpus would be, and
# under a lock without an anchor it would inherit the fd and hold the queue.
case_kill() { # <lib> <handoff>
    local A L H rc key anchor
    L="$WORK/lock.kill.$RANDOM"; H="$WORK/home.kill.$RANDOM"
    A="$(build_repo killA "$1" "$2")"
    key="$(boxkey fakehost gl-scratch-kill)"
    gate "$A" under-test gl-scratch-kill "$L" "$H" 120 --bg
    wait_until_held "$L" "$key" 15 || { kill -9 "$GPID" 2>/dev/null; return 1; }
    anchor="$(wait_until_anchored "$L" "$key" 15)" || {
        kill -9 "$GPID" 2>/dev/null
        echo "      (the gate never anchored, so this case would not have tested the anchor)"
        return 1; }
    kill -9 "$GPID" 2>/dev/null
    wait "$GPID" 2>/dev/null
    sleep 0.5
    kill -0 "$anchor" 2>/dev/null && {
        echo "      (the anchor pid $anchor outlived the gate)"; return 1; }
    # The kernel's answer, before the next gate is even started.
    if ! ( exec 210>>"$L/$key.lock"; flock -x -n 210 ) 2>/dev/null; then
        echo "      (the lock is still held after SIGKILL of the gate)"
        return 1
    fi
    gate "$A" under-test gl-scratch-kill "$L" "$H" 0; rc=$?
    note "$GOUT"
    [ "$rc" = 0 ] || { echo "      (the next gate exit $rc, wanted 0 — wedged)"; return 1; }
    case "$GOUT" in GREEN*) return 0 ;; *) return 1 ;; esac
}

# The other half of the same requirement, and the sweep found it by accident:
# a gate killed *during* the acquire — inside the fifth of a second the anchor
# takes to fork, exec and be verified — must also leave nothing behind. This
# case failed twice before `_gl_anchor_start` grew its `$PPID` guard, because
# `setpriv` sets `--pdeathsig` after the fork and a parent that dies in that
# window leaves an anchor the kernel was never told to kill. The delays sweep
# both sides of the anchor's appearance on purpose.
case_kill_in_acquire() { # <lib> <handoff>
    local L key i pid d
    L="$WORK/lock.acq.$RANDOM"
    key="$(boxkey fakehost gl-scratch-acq)"
    # The probe below opens the lock file, so the file has to exist even when
    # the acquire was killed before it could create it. Without this the first
    # iteration read "cannot open" as "held" — a failure of the instrument that
    # looked exactly like a failure of the lock.
    mkdir -p "$L" && : >> "$L/$key.lock"
    for i in 0 1 2 3 4 5 6 7; do
        d="0.$(( (i * 7) % 10 ))"
        ( exec env RUSTMC_GATE_LOCK_DIR="$L" GATE_LOCK_ANCHOR_NAME="$GATE_LOCK_ANCHOR_NAME" \
            bash -c '. "$1"; gate_lock_acquire fakehost gl-scratch-acq "$PWD" acq >/dev/null 2>&1; sleep 60' \
            acquire-probe "$1" ) &
        pid=$!
        sleep "$d"
        kill -9 "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
        sleep 0.4
        if ! ( exec 210>>"$L/$key.lock"; flock -x -n 210 ) 2>/dev/null; then
            echo "      (killed ${d}s into the acquire and the lock stayed held)"
            pkill -f "$GATE_LOCK_ANCHOR_NAME" >/dev/null 2>&1
            return 1
        fi
    done
    return 0
}

# The anchor is verified before our own fds are dropped, so an anchor that does
# not come up must REFUSE rather than gate while only believing it holds the
# queue. Asserted here as a case so that the injection has something to break.
case_anchor_refuses() { # <lib> <handoff>
    local A L H rc
    L="$WORK/lock.anch.$RANDOM"; H="$WORK/home.anch.$RANDOM"
    A="$(build_repo anchA "$1" "$2")"
    gate "$A" under-test gl-scratch-anch "$L" "$H" 0; rc=$?
    note "$GOUT"
    [ "$rc" = 0 ] || return 1
    case "$GOUT" in GREEN*) ;; *) return 1 ;; esac
    return 0
}

# THE CENSUS, in both directions, with exact numbers.
#
#   run 1  cold directory, no stamp        -> reports every artefact, and says so
#   run 2  same tip, nothing written       -> exactly 0, and NO warning
#   run 3  5 files written under target/   -> exactly 5
#   run 4  only .rustc_info.json rewritten -> exactly 0   <-- the sharp edge
#   run 5  a new tip, still nothing built  -> 0 compiled over 1 changed .rs,
#                                             and the WARNING fires
case_census() { # <lib> <handoff>
    local A L H box tgt lg n
    L="$WORK/lock.cen.$RANDOM"; H="$WORK/home.cen.$RANDOM"
    A="$(build_repo cenA "$1" "$2")"
    box=gl-scratch-census; tgt="$H/$box/target"
    mkdir -p "$tgt/debug/deps" "$tgt/debug/incremental"
    local i; for i in 1 2 3; do : > "$tgt/debug/deps/old$i.o"; done
    : > "$tgt/.rustc_info.json"

    ( cd "$A" && git checkout -q under-test~1 ) >/dev/null 2>&1
    gate "$A" under-test~1 "$box" "$L" "$H" 0 || return 1
    lg="$(last_log "$H")"; note "$(grep -E '^(compile-census|gate-dir|handoff: compiled)' "$lg")"
    grep -q 'NO PRIOR STAMP' "$lg" || { echo "      (run 1 did not say it had no prior stamp)"; return 1; }

    sleep 1.1
    gate "$A" under-test~1 "$box" "$L" "$H" 0 || return 1
    lg="$(last_log "$H")"; note "$(grep -E '^(compile-census|gate-dir|handoff: compiled)' "$lg")"
    n="$(sed -n 's/^compile-census: \([0-9]*\) .*/\1/p' "$lg")"
    [ "$n" = 0 ] || { echo "      (run 2 census $n, wanted 0 on a genuine no-op)"; return 1; }
    grep -q 'WARNING' "$lg" && { echo "      (run 2 warned on a same-tip re-gate)"; return 1; }

    sleep 1.1
    for i in 1 2 3 4 5; do : > "$tgt/debug/incremental/new$i.bin"; done
    gate "$A" under-test~1 "$box" "$L" "$H" 0 || return 1
    lg="$(last_log "$H")"; note "$(grep -E '^(compile-census|handoff: compiled)' "$lg")"
    n="$(sed -n 's/^compile-census: \([0-9]*\) .*/\1/p' "$lg")"
    [ "$n" = 5 ] || { echo "      (run 3 census $n, wanted exactly 5)"; return 1; }

    sleep 1.1
    : > "$tgt/.rustc_info.json"
    gate "$A" under-test~1 "$box" "$L" "$H" 0 || return 1
    lg="$(last_log "$H")"; note "$(grep -E '^(compile-census|handoff: compiled)' "$lg")"
    n="$(sed -n 's/^compile-census: \([0-9]*\) .*/\1/p' "$lg")"
    [ "$n" = 0 ] || { echo "      (run 4 census $n, wanted 0 — .rustc_info.json must not count)"; return 1; }

    sleep 1.1
    gate "$A" under-test "$box" "$L" "$H" 0 || return 1
    lg="$(last_log "$H")"; note "$(grep -E '^(compile-census|handoff: compiled|handoff: WARNING)' "$lg")"
    grep -q '^handoff: compiled=0 rust-files-changed=1 ' "$lg" || {
        echo "      (run 5 did not report 0 compiled over 1 changed rust file)"; return 1; }
    grep -q '^handoff: WARNING' "$lg" || { echo "      (run 5 did not warn)"; return 1; }
    return 0
}

# ============================================================================
# THE INJECTIONS
# ============================================================================
inject() { # inject <name> <sed-program-over-the-library>
    local name="$1"; shift
    local out="$WORK/lib.$name.sh"
    sed "$@" "$LIB_REAL" > "$out"
    cmp -s "$out" "$LIB_REAL" && { echo "  BROKEN SWEEP: injection '$name' changed nothing"; exit 2; }
    printf '%s\n' "$out"
}
inject_handoff() { # inject_handoff <name> <sed-program>
    local name="$1"; shift
    local out="$WORK/handoff.$name.sh"
    sed "$@" "$HANDOFF_REAL" > "$out"; chmod +x "$out"
    cmp -s "$out" "$HANDOFF_REAL" && { echo "  BROKEN SWEEP: injection '$name' changed nothing"; exit 2; }
    printf '%s\n' "$out"
}

run_case()  { # run_case <label> <fn> <lib> <handoff>
    if "$2" "$3" "$4"; then ok "$1"; else bad "$1"; fi
}
run_inject() { # run_inject <label> <fn> <lib> <handoff>
    if "$2" "$3" "$4"; then
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$1")
        printf '  MISSED  %s — the case passed on broken code\n' "$1"
    else
        PASS=$((PASS+1)); printf '  caught  %s\n' "$1"
    fi
}

build_ssh
echo
echo "[1] THE THREE DEMONSTRATIONS, against the real library"
run_case "single gate runs, and its log says what it did" case_single         "$LIB_REAL" "$HANDOFF_REAL"
run_case "two gates, one box directory: second refuses by name" case_exclusion_box "$LIB_REAL" "$HANDOFF_REAL"
run_case "two gates, one tree, two box dirs: second refuses"    case_exclusion_tree "$LIB_REAL" "$HANDOFF_REAL"
run_case "SIGKILL of a live gate does not wedge the next"       case_kill      "$LIB_REAL" "$HANDOFF_REAL"
run_case "SIGKILL during the acquire leaves nothing behind"     case_kill_in_acquire "$LIB_REAL" "$HANDOFF_REAL"

echo
echo "[2] THE PROVENANCE INSTRUMENT, exact counts in both directions"
run_case "census: 0 on a no-op, 5 on 5 writes, 0 on rustc_info only, and it warns" \
         case_census "$LIB_REAL" "$HANDOFF_REAL"

echo
echo "[3] THE INJECTIONS — every case above must be able to go red"
run_inject "no flock at all -> two gates in one box dir" case_exclusion_box \
    "$(inject noflock -e 's/^\( *\)if ! flock -x -n "\$_GL_BOX_FD"; then/\1if false; then/' \
                      -e 's/^\( *\)if ! flock -x -n "\$_GL_TREE_FD"; then/\1if false; then/')" "$HANDOFF_REAL"
run_inject "a key per process -> two gates in one box dir" case_exclusion_box \
    "$(inject perpid -e "s|^    printf '%s\\.%s\\.%s\\\\n' \"\$kind\" \"\$safe\" \"\$dig\"|    printf '%s.%s.%s.%s\\\\n' \"\$kind\" \"\$safe\" \"\$dig\" \$\$|")" "$HANDOFF_REAL"
run_inject "the refusal names nobody"                     case_exclusion_box \
    "$(inject mute -e 's|^_gl_describe_holder() {|_gl_describe_holder() { echo ""; return 0;|')" "$HANDOFF_REAL"
run_inject "no anchor: children inherit the lock -> wedge" case_kill \
    "$(inject noanchor -e 's|^_gl_anchor_start() {|_gl_anchor_start() { _GL_ANCHOR=$$; _GL_ANCHOR_START="$(_gl_pidstart $$)"; return 0;|')" "$HANDOFF_REAL"
run_inject "an anchor with no --pdeathsig -> killed gate wedges" case_kill \
    "$(inject nopdeathsig -e 's|^    setpriv --pdeathsig KILL bash -c \\$|    bash -c \\|')" "$HANDOFF_REAL"
run_inject "no --pdeathsig, killed mid-acquire"            case_kill_in_acquire \
    "$WORK/lib.nopdeathsig.sh" "$HANDOFF_REAL"
run_inject "the anchor dies at once -> acquire must refuse" case_anchor_refuses \
    "$(inject deadanchor -e 's|exec -a "\$2" sleep infinity|exit 0|')" "$HANDOFF_REAL"
run_inject "the census counts .rustc_info.json"             case_census "$LIB_REAL" \
    "$(inject_handoff rustcinfo -e 's/ ! -name .rustc_info.json//')"

echo
echo "$PASS passed, $FAIL failed, $MISSED injections missed"
for c in "${FAILED_CASES[@]:-}";      do [ -n "$c" ] && echo "  failed:  $c"; done
for c in "${MISSED_INJECTIONS[@]:-}"; do [ -n "$c" ] && echo "  missed:  $c"; done
[ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ] || exit 1
echo "GATE-LOCK-FALSIFY GREEN"
