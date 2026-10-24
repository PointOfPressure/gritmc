#!/usr/bin/env bash
# gate-lock.sh — two merge gates can no longer share a box directory, and one
# box directory can no longer be fed by two laptop trees at the same time.
#
#     . "$ROOT/ci/gate-lock.sh"
#     gate_lock_acquire "$HOST" "$REMOTE_DIR" "$ROOT" "$BRANCH" || exit 2
#
#     ./ci/gate-lock.sh status          # from outside, for a human
#
# # What this is protecting, stated as the mechanism rather than the rule
#
# `ci/t0-remote.sh` ships the tree with `rsync -a`, which preserves mtimes, and
# `target/` is excluded from the transfer. Cargo decides freshness by comparing
# a unit's source mtimes against its fingerprint's mtime, so a source file that
# lands in the gate directory **older** than that directory's last build is
# `Fresh`: `cargo clippy -D warnings`, `cargo check` and `cargo test` all report
# success on a file no compiler read, and the gate issues a green for a tree
# that does not build. Reproduced in a lab (`docs/GATE-FORENSICS.md`): identical
# broken bytes, back-dated arm gave `T0 GREEN` with **1** entry written under
# `target/`; current-mtime arm gave clippy/check/test 101 with **104**.
#
# The mtime symptom is fixed on `gate-command-coverage` — `ci/t0-remote.sh` now
# touches exactly what it transferred. This file is about the cause.
#
# **Why the hole never fired in 61 classifiable gate runs**, and it is not luck:
#
# > The gate directory's content **is** a copy of `$ROOT` taken by the previous
# > run in that same directory. So the set of files that differ — which is
# > exactly the set rsync transfers — is the set git has rewritten in `$ROOT`
# > since that copy, and a rewrite stamps the current time. The hole needs an
# > mtime older than the gate directory's build, and the build follows the copy.
#
# That invariant is load-bearing, undocumented and unasserted, and it has
# exactly one failure mode: **a file written in `$ROOT` between a run's stage-2
# rsync and that run's last compile.** Equivalently — two laptop trees feeding
# one box directory, or two gates overlapping in time. Both are in the record.
# Five gate directories have been alternated between; three concurrent gate
# pairs ran on 2026-08-06, all in one afternoon, and one of them put a checkout
# inside another run's build window and came within minutes of firing. All three
# were checked and all three were clean, which is the good luck, not the design.
#
# Until now `ci/handoff.sh` had no mutual exclusion of any kind. It refuses a
# dirty `$ROOT`; two handoffs started ninety seconds apart both see a clean one.
#
# # Where the lock keys, and why there are two keys rather than one
#
# **The box directory is the resource being corrupted**, so it is the first key.
# The naive alternative — a lockfile in `$ROOT` — is the one placement that
# cannot work: two laptop worktrees have two different `$ROOT`s, and it is
# precisely those two trees sharing one box directory that ships an aged file
# into a warm build cache. A per-tree lock would let that through while looking
# like protection.
#
# **`$ROOT` is the second key, and the record is the reason.** The near-miss on
# 2026-08-06 14:36 was *not* two gates in one box directory. It was two gates in
# **different** box directories (`rustmc-gate` and `rustmc-gate-tables`) driven
# from the **same** `$ROOT`: the second gate's `git checkout` moved `$ROOT` to
# `4e9abc0` at 14:36:13, inside the first gate's build window, so the files the
# first gate shipped at 15:16 carried that older mtime instead of a fresh one.
# A lock keyed only on the box directory would have permitted that pair. So both
# ends of the pipe are keyed: **one gate per box directory, one gate per tree.**
#
# Both keys are taken non-blocking and in a fixed order, so no ordering of
# gates can deadlock — but the fixed order is written down anyway, because a
# later change to blocking acquisition would make it load-bearing.
#
# # Refuse, do not queue — and the reason is not impatience
#
# `ci/corpus-lock.sh` blocks, because a corpus run that waits forty minutes
# still produces the measurement it was asked for and the alternative throws
# away a preflight and a build. A gate is the opposite: the second gate's
# `$BRANCH` is a moving target, `main` is about to advance under it, and a gate
# that queues for two hours and then merges is gating a rebase that never
# happened. `AHEAD`/`BEHIND` were checked against a `main` that has since moved.
#
# So a second gate **REFUSES, exit 2, naming the holder** — the same verdict and
# the same exit code `ci/handoff.sh` already uses for a worktree-held branch,
# and for the same reason: nothing about the branch has been measured, so this
# must not read as `RED`. Re-run it when the queue is clear; that is one line in
# the refusal.
#
# # Staleness — kernel-enforced, because every invented liveness check in this
# # repo has eventually been found to be measuring itself
#
# A gate has already been killed mid-flight by something outside it: the Claude
# Code process died at 13:35 on 2026-08-06 and took three agents and a running
# gate with it. "A dead gate must not wedge the queue" is therefore a
# requirement, not a nicety.
#
# `flock` needs no liveness check at all. **The lock is a property of an open
# file description, and a file descriptor cannot outlive the process holding
# it.** `kill -9` closes it and the kernel drops the lock, on every exit path
# there is, including the ones no script gets to run code on.
#
# **Inheritance is the one thing that can break that, and for a merge gate it is
# not hypothetical.** An fd survives `fork`, so every child `ci/handoff.sh`
# spawns would hold the lock too — and its children are `ssh` and `rsync`
# sessions, one of which sits for fifty minutes waiting out a gameplay corpus.
# An ssh that hangs after its parent is killed would hold a box-wide gate lock
# for as long as its TCP connection took to notice, which is the wedge this file
# exists to forbid, reintroduced by the mechanism meant to prevent it. The same
# defect was measured, not reasoned about, in `ci/corpus-lock.sh`'s first draft:
# three falsification cases red, and a killed run's debris holding the lock for
# 120 s.
#
# So the design is `ci/corpus-lock.sh`'s, unchanged, because it is the same
# problem: after acquiring, fork a childless **anchor**
#
#     setpriv --pdeathsig KILL bash -c 'exec -a rustmc-gate-lock-anchor sleep infinity'
#
# which inherits the lock fds, then close our own copies. From that instant the
# anchor is the only process in the world holding the lock; every `ssh` the gate
# spawns afterwards is forked from a process that no longer has the fd, so none
# of them can inherit it; and `--pdeathsig KILL` makes the kernel kill the
# anchor the moment `ci/handoff.sh` dies, by any means including `kill -9`.
#
# The invariant is one sentence: **exactly one process holds this lock, and the
# kernel kills it when the gate does.** `gate_lock_assert_anchored` checks it at
# runtime, because a lock that has quietly gone free is indistinguishable from a
# held one right up until two gates collide.
#
# # What this does NOT exclude, said plainly
#
# The lock lives on the machine `ci/handoff.sh` runs on, which is the laptop.
# It therefore excludes **every gate**, because every gate is `ci/handoff.sh`
# and there is exactly one acquire site in it — but it does not exclude a human
# or an agent who ssh'es to the box and builds in the gate directory by hand,
# and it would not exclude a second laptop driving the same box directory if
# this project ever had one. Both are outside the shape the record shows.
#
# Putting the lock file on the box instead would close that, and the reason it
# is not there is a straight trade: the anchor would have to be kept alive
# across a two-hour gate by an ssh session, and "sshd notices the socket closed"
# is not a kernel guarantee the way `--pdeathsig` is. That would buy coverage of
# a driver that does not exist by weakening the staleness guarantee that the
# only observed failure — a killed gate — actually needs. The gate directory is
# still what the lock is *keyed on*, which is what makes two trees collide.
#
# Falsified by `ci/gate-lock-falsify.sh`, which drives the real `ci/handoff.sh`
# and is required to catch every injected break in this file.

# --- where the lock lives ---------------------------------------------------
#
# A well-known per-user path on whichever machine the gate driver runs on.
# Deliberately NOT under any tree: the whole point is that two trees share one
# lock. The override exists so the falsification sweep can exercise the protocol
# in a private namespace without wedging a real gate.
GATE_LOCK_DIR="${RUSTMC_GATE_LOCK_DIR:-$HOME/.rustmc-gate-lock}"

# fds 202/203 rather than 9/10: low numbers get reused by ordinary redirection
# in a script that did not know this file existed. 200/201 are `ci/corpus-lock.sh`'s
# and are left alone so that a future caller can hold both without either
# silently closing the other's.
_GL_BOX_FD=202
_GL_TREE_FD=203

_GL_HELD=""          # set once both locks are ours, so release is idempotent
_GL_ANCHOR=""        # the childless process that actually holds them
_GL_ANCHOR_START=""  # its /proc start time, so a recycled PID cannot impersonate it
_GL_BOX_KEY=""       # the two keys, exported for the log header
_GL_TREE_KEY=""

# The anchor's argv[0]. A thing holding a queue-wide lock should say what it is
# in `ps` rather than read as `sleep infinity`. Overridable for one reason only:
# `ci/gate-lock-falsify.sh` deliberately leaves broken anchors behind and has to
# clean up after itself, and a sweep that cleaned up by the production name
# would kill the anchor of a REAL gate running on the same laptop.
_GL_ANCHOR_NAME="${GATE_LOCK_ANCHOR_NAME:-rustmc-gate-lock-anchor}"

_gl_now() { date +%s; }

# A key is readable AND injective: the readable part is what a human sees in
# `status` and in a refusal, and the digest is what stops two different
# resources whose punctuation happens to squash the same way from sharing a
# lock. Readability alone would be a collision waiting to be found by the one
# pair of paths nobody tried.
_gl_key() { # _gl_key <kind> <string>
    local kind="$1" s="$2" safe dig
    safe="$(printf '%s' "$s" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-64)"
    dig="$(printf '%s' "$s" | sha1sum | cut -c1-8)"
    printf '%s.%s.%s\n' "$kind" "$safe" "$dig"
}

# The start time of a PID, from field 22 of /proc/<pid>/stat, in clock ticks
# since boot. Recorded beside the PID so that "the holder is still alive" can
# never be answered by a *different* process that inherited the number.
_gl_pidstart() {
    local p="$1" s
    [ -r "/proc/$p/stat" ] || return 1
    # comm can contain spaces and brackets, so cut from the last ')'.
    s="$(cat "/proc/$p/stat")" || return 1
    s="${s##*) }"
    printf '%s\n' "$s" | awk '{print $20}'
}

_gl_alive() {
    local p="$1" want="$2" have
    [ -n "$p" ] || return 1
    kill -0 "$p" 2>/dev/null || return 1
    [ -n "$want" ] || return 0
    have="$(_gl_pidstart "$p" 2>/dev/null || true)"
    [ "$have" = "$want" ]
}

# --- the holder record ------------------------------------------------------
#
# The record says who CLAIMED the lock; the kernel says who still holds the fd.
# Those two are shown separately and never conflated: the record is what makes a
# refusal name a branch and a tree instead of a pid, and the kernel is what
# makes it true.
_gl_write_holder() { # _gl_write_holder <keyfile-base> <box> <tree> <branch>
    local base="$1" box="$2" tree="$3" branch="$4" tmp="$1.holder.$$"
    {
        printf 'pid=%s\n'      "$$"
        printf 'pidstart=%s\n' "$(_gl_pidstart $$ || echo '')"
        printf 'anchor=%s\n'   "$_GL_ANCHOR"
        printf 'driver=%s\n'   "$(hostname)"
        printf 'tree=%s\n'     "$tree"
        printf 'gatedir=%s\n'  "$box"
        printf 'branch=%s\n'   "$branch"
        # `roothead`, NOT `tip`. The lock is taken before `git checkout
        # "$BRANCH"` — deliberately, since that checkout is itself one of the
        # writes a second gate must not interleave with — so `$ROOT` is still on
        # whatever it was on. Calling this field `tip` would make it read as the
        # commit under test, which it is not. The tip is in the log header.
        printf 'roothead=%s\n' "$(git -C "$tree" rev-parse --short HEAD 2>/dev/null || echo unknown)"
        printf 'since=%s\n'    "$(_gl_now)"
        printf 'sinceiso=%s\n' "$(date -Is)"
    } > "$tmp" && mv -f "$tmp" "$base.holder" || true
}

_gl_describe_holder() { # _gl_describe_holder <keyfile-base>
    local f="$1.holder" pid ps tree gatedir branch head since driver age
    [ -s "$f" ] || { echo "someone who left no holder record (see ./ci/gate-lock.sh status)"; return 0; }
    pid="$(sed      -n 's/^pid=//p'      "$f")"
    ps="$(sed       -n 's/^pidstart=//p' "$f")"
    tree="$(sed     -n 's/^tree=//p'     "$f")"
    gatedir="$(sed  -n 's/^gatedir=//p'  "$f")"
    branch="$(sed   -n 's/^branch=//p'   "$f")"
    head="$(sed     -n 's/^roothead=//p' "$f")"
    since="$(sed    -n 's/^since=//p'    "$f")"
    driver="$(sed   -n 's/^driver=//p'   "$f")"
    age=""
    [ -n "$since" ] && age="$(( ($(_gl_now) - since) / 60 ))m"
    if _gl_alive "$pid" "$ps"; then
        printf 'gate on %s, pid %s on %s, tree %s (was at %s), gate-dir %s, started %s (%s ago)\n' \
            "${branch:-?}" "${pid:-?}" "${driver:-?}" "${tree:-?}" "${head:-?}" \
            "${gatedir:-?}" "$(date -d "@${since:-0}" -Is 2>/dev/null || echo '?')" "${age:-?}"
    else
        printf 'gate on %s, pid %s in %s — THAT PROCESS IS GONE, but the lock is still held; ./ci/gate-lock.sh status names what holds it\n' \
            "${branch:-?}" "${pid:-?}" "${tree:-?}"
    fi
}

# Which live PIDs actually hold a lock file open. The kernel's answer rather
# than the record's. `fuser`/`lsof` are not guaranteed present; scanning /proc
# needs neither.
_gl_fd_holders() {
    local target="$1" p link out=""
    for p in /proc/[0-9]*; do
        for link in "$p"/fd/*; do
            [ -L "$link" ] || continue
            if [ "$(readlink "$link" 2>/dev/null)" = "$target" ]; then
                out="$out ${p#/proc/}"
                break
            fi
        done
    done
    printf '%s\n' "${out# }"
}

# --- the anchor -------------------------------------------------------------
#
# Hand both locks to a childless process the kernel kills when we die, then drop
# our own fds. After this returns, exactly one process holds the locks and
# nothing the gate spawns can inherit them.
_gl_anchor_start() {
    # `--pdeathsig` is set by setpriv *after* the fork, so there is a window of
    # a few microseconds in which the parent can die before the kernel has been
    # told to care — and the anchor that comes up in that window would hold the
    # queue forever, which is the one outcome this whole file forbids. The
    # `$PPID` test closes it from the other side: a child that has already been
    # reparented has PPID 1, and exits rather than becoming an orphan holding a
    # lock. The two guards cover each other's window. `$BASHPID`, not `$$`, so
    # this is still the forking process if a caller ever sources this in a
    # subshell.
    # `$BASHPID` is captured HERE, into a variable, and not written inline in
    # the background command. Bash forks first and expands the command's words
    # in the child, so an inline `$BASHPID` is the *anchor's own* pid — the test
    # would compare the child against itself, always match, and assert nothing.
    # Measured, not reasoned about: inline, the guard printed MISMATCH and shot
    # every anchor it ever started. `$$` would work today and would be wrong the
    # first time a caller sourced this inside a subshell.
    local me="$BASHPID"
    setpriv --pdeathsig KILL bash -c \
        '[ "$PPID" = "$1" ] || exit 0; exec -a "$2" sleep infinity' \
        gate-lock-anchor "$me" "$_GL_ANCHOR_NAME" &
    _GL_ANCHOR=$!

    # VERIFY BEFORE DROPPING. Closing our fds first and checking afterwards is
    # the one ordering that can lose the lock silently: if `setpriv` exits —
    # wrong version, exec failure, fork limit — the anchor is gone, and a gate
    # that had already closed its own copies would run for two hours believing
    # it held the queue. Checking first costs a fifth of a second and cannot
    # lose: we still hold both locks, so a refusal here is safe and a success is
    # real. `kill -0` alone is not enough immediately after `fork` — it succeeds
    # before `exec` has had a chance to fail — so this waits, then asks the
    # kernel the question that actually matters: does that process hold these
    # two lock files on these two fds?
    sleep 0.2
    local ok=1 fd want have
    kill -0 "$_GL_ANCHOR" 2>/dev/null || ok=""
    for fd in "$_GL_BOX_FD:$_GL_BOX_KEY" "$_GL_TREE_FD:$_GL_TREE_KEY"; do
        want="$(readlink -f "$GATE_LOCK_DIR/${fd#*:}.lock" 2>/dev/null || true)"
        have="$(readlink "/proc/$_GL_ANCHOR/fd/${fd%%:*}" 2>/dev/null || true)"
        [ -n "$want" ] && [ "$have" = "$want" ] || ok=""
    done
    if [ -z "$ok" ]; then
        echo "REFUSED  the gate lock anchor did not come up (pid ${_GL_ANCHOR:-none})."
        echo "         Expected it to hold $GATE_LOCK_DIR/$_GL_BOX_KEY.lock on fd $_GL_BOX_FD"
        echo "         and $GATE_LOCK_DIR/$_GL_TREE_KEY.lock on fd $_GL_TREE_FD."
        echo "         Refusing rather than gating while only believing the queue is ours."
        echo "         Nothing has been touched; see ci/gate-lock.sh."
        _gl_anchor_stop
        return 2
    fi
    # Its start time, so a recycled PID can never answer "is the anchor alive".
    _GL_ANCHOR_START="$(_gl_pidstart "$_GL_ANCHOR" 2>/dev/null || echo '')"

    # Now drop OUR copies. The locks are on the open file descriptions, which
    # the anchor still refers to, so closing these releases nothing — it only
    # removes the fds every later `fork` would otherwise copy.
    eval "exec $_GL_BOX_FD>&- $_GL_TREE_FD>&-" 2>/dev/null || true
    return 0
}

_gl_anchor_stop() {
    [ -n "$_GL_ANCHOR" ] && kill "$_GL_ANCHOR" 2>/dev/null
    _GL_ANCHOR=""; _GL_ANCHOR_START=""
    return 0
}

# `--pdeathsig` is what makes the anchor safe; without it the anchor would
# outlive the gate and hold the queue forever, which is worse than no anchor at
# all. So it is checked, not assumed, and the check is a refusal.
_gl_require_setpriv() {
    if ! command -v setpriv >/dev/null 2>&1; then
        echo "REFUSED  setpriv(1) is missing — the gate lock cannot be held safely."
        echo "         util-linux >= 2.33 provides it. Install it rather than gating"
        echo "         unlocked; see ci/gate-lock.sh."
        return 2
    fi
    if ! setpriv --help 2>&1 | grep -c -- '--pdeathsig' >/dev/null; then
        echo "REFUSED  this setpriv has no --pdeathsig, so a killed gate would leave"
        echo "         the lock held forever. Refusing. See ci/gate-lock.sh."
        return 2
    fi
    return 0
}

# --- the invariant, checked rather than trusted -----------------------------
#
# "Exactly one process holds this lock, and the kernel kills it when the gate
# does" is the whole design, so it is asserted while the gate is live rather
# than only at acquire. A lock that has gone free — anchor caught by a stray
# `pkill -f`, `setpriv` behaving differently after an upgrade — is
# indistinguishable from a held one right up until two gates collide. One
# `kill -0` and deliberately nothing more: the O(every fd on the box) /proc scan
# belongs in `status`, where somebody is asking.
gate_lock_assert_anchored() {
    [ -n "$_GL_HELD" ] || return 0
    if ! _gl_alive "$_GL_ANCHOR" "$_GL_ANCHOR_START"; then
        echo "REFUSED  the gate lock anchor (pid $_GL_ANCHOR) is gone."
        echo "         This gate is no longer holding the queue, so a second gate can"
        echo "         start on top of it — and the two of them sharing $_GL_BOX_KEY"
        echo "         is exactly what puts a back-dated file into a warm build cache."
        echo "         Nothing measured after this point would be trustworthy."
        return 2
    fi
    return 0
}

# --- the acquire ------------------------------------------------------------
#
# Non-blocking, both keys, box first. Returns 0 holding both, or prints a
# `REFUSED` naming the holder and returns 2. It never waits: see the header.
gate_lock_acquire() { # gate_lock_acquire <host> <remote-dir> <root> <branch>
    local host="$1" dir="$2" tree="$3" branch="$4" base

    _gl_require_setpriv || return 2

    # The box key names the resource on the far end: the same directory on the
    # same host is the same resource however the caller spelled the path.
    _GL_BOX_KEY="$(_gl_key box "$host:$dir")"
    # `-P` so that two paths differing only by a symlink hop are one tree.
    _GL_TREE_KEY="$(_gl_key tree "$(hostname):$(cd "$tree" 2>/dev/null && pwd -P || printf '%s' "$tree")")"

    mkdir -p "$GATE_LOCK_DIR" || {
        echo "REFUSED  cannot use the gate lock directory $GATE_LOCK_DIR"
        return 2
    }
    # `>>` never truncates. Truncating would be harmless to the lock itself —
    # the lock is on the inode, not the bytes — but these files are also where a
    # human looks, and emptying them under a live holder throws away the only
    # record of who that is.
    eval "exec $_GL_BOX_FD>>\"\$GATE_LOCK_DIR/\$_GL_BOX_KEY.lock\"" || return 2
    eval "exec $_GL_TREE_FD>>\"\$GATE_LOCK_DIR/\$_GL_TREE_KEY.lock\"" || return 2

    if ! flock -x -n "$_GL_BOX_FD"; then
        base="$GATE_LOCK_DIR/$_GL_BOX_KEY"
        echo "REFUSED  another gate already owns the box directory ~/$dir on $host"
        echo "         holder: $(_gl_describe_holder "$base")"
        echo "         Two gates in one box directory is how a source file arrives"
        echo "         older than that directory's last build, and cargo calls it"
        echo "         Fresh. Nothing about $branch has been measured — this is not a"
        echo "         verdict. Re-run when that gate finishes, or gate into a"
        echo "         different directory with RUSTMC_REMOTE_DIR."
        echo "         ./ci/gate-lock.sh status"
        return 2
    fi
    if ! flock -x -n "$_GL_TREE_FD"; then
        base="$GATE_LOCK_DIR/$_GL_TREE_KEY"
        # Release the box lock we did take. Held-and-refused is the one state
        # that would wedge the next gate for no reason at all.
        flock -u "$_GL_BOX_FD" 2>/dev/null || true
        echo "REFUSED  another gate is already driving this tree ($tree)"
        echo "         holder: $(_gl_describe_holder "$base")"
        echo "         A second gate here moves \$ROOT with 'git checkout' while the"
        echo "         first one is mid-build, and the files that first gate ships"
        echo "         next carry the older mtime. That is the 2026-08-06 14:36"
        echo "         near-miss, and it happened across two DIFFERENT box"
        echo "         directories — which is why this key exists as well as that"
        echo "         one. Nothing about $branch has been measured."
        echo "         ./ci/gate-lock.sh status"
        return 2
    fi

    # WRITE THE RECORD BEFORE THE ANCHOR, then again after. The anchor takes
    # about a fifth of a second to verify, and a second gate arriving inside
    # that window would find the lock held and no record to name — it printed
    # "someone who left no holder record" on the falsification sweep's first
    # run. A refusal that cannot say who holds the lock is most of the value of
    # refusing at all. Nothing else can be writing these files: we hold both
    # locks, which is the entire precondition for being here.
    _gl_write_holder "$GATE_LOCK_DIR/$_GL_BOX_KEY"  "$host:~/$dir" "$tree" "$branch"
    _gl_write_holder "$GATE_LOCK_DIR/$_GL_TREE_KEY" "$host:~/$dir" "$tree" "$branch"

    # Hand both to the anchor and drop our own fds BEFORE returning to a script
    # that is about to fork an ssh which waits out a fifty-minute corpus.
    _gl_anchor_start || return 2
    _GL_HELD=1
    # Again, now that `anchor=` is knowable. `status` reads that field to say
    # which of the fd holders is the one that matters.
    _gl_write_holder "$GATE_LOCK_DIR/$_GL_BOX_KEY"  "$host:~/$dir" "$tree" "$branch"
    _gl_write_holder "$GATE_LOCK_DIR/$_GL_TREE_KEY" "$host:~/$dir" "$tree" "$branch"
    return 0
}

# The kernel already does the important half of this when we exit; this makes it
# immediate and tidies the record, so `status` says FREE rather than naming a
# process that has gone. A caller that forgets it is not a bug.
gate_lock_release() {
    [ -n "$_GL_HELD" ] || return 0
    _GL_HELD=""
    rm -f "$GATE_LOCK_DIR/$_GL_BOX_KEY.holder" "$GATE_LOCK_DIR/$_GL_TREE_KEY.holder"
    _gl_anchor_stop
    eval "exec $_GL_BOX_FD>&- $_GL_TREE_FD>&-" 2>/dev/null || true
    return 0
}

# One line for the gate log header: which keys this run holds and what is
# holding them. A future forensic question about two gates overlapping is
# answerable from this line plus the timestamps, with no archaeology.
gate_lock_header() {
    printf 'lock-box=%s lock-tree=%s anchor=%s\n' \
        "${_GL_BOX_KEY:-none}" "${_GL_TREE_KEY:-none}" "${_GL_ANCHOR:-none}"
}

gate_lock_status() {
    local f base free holders p anchor tag n=0
    mkdir -p "$GATE_LOCK_DIR" 2>/dev/null || true
    echo "gate lock: $GATE_LOCK_DIR"
    for f in "$GATE_LOCK_DIR"/*.lock; do
        [ -f "$f" ] || continue
        n=$((n + 1))
        base="${f%.lock}"
        # Definitive, from the kernel: a non-blocking exclusive acquire that
        # succeeds proves the lock was free at that instant. Taken on a
        # throwaway fd in a subshell so that asking cannot disturb a gate.
        free=""
        if ( exec 210>>"$f"; flock -x -n 210 ) 2>/dev/null; then free=1; fi
        printf '  %s\n' "$(basename "$base")"
        if [ -n "$free" ]; then
            printf '    state:  FREE\n'
        else
            printf '    state:  HELD\n'
        fi
        printf '    record: %s\n' "$(_gl_describe_holder "$base")"
        holders="$(_gl_fd_holders "$(readlink -f "$f")")"
        anchor="$(sed -n 's/^anchor=//p' "$base.holder" 2>/dev/null || true)"
        if [ -n "$holders" ]; then
            for p in $holders; do
                tag=""
                [ "$p" = "$anchor" ] && tag="   <-- THE ANCHOR: this is what holds the lock"
                printf '    fd:     pid %-8s %s%s\n' "$p" \
                    "$(tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | cut -c1-70)" "$tag"
            done
        else
            printf '    fd:     nobody has it open\n'
        fi
    done
    [ "$n" -gt 0 ] || echo "  no keys yet — no gate has ever run on this machine"
}

# --- as a command -----------------------------------------------------------
# Sourced: everything above is defined and nothing runs. Executed: `status`.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -uo pipefail
    case "${1:-status}" in
        status)
            if [ "${2:-}" = "--watch" ]; then
                while :; do clear 2>/dev/null || true; date -Is; gate_lock_status; sleep 10; done
            else
                gate_lock_status
            fi ;;
        *)
            echo "usage: $0 [status [--watch]]" >&2
            echo "       . $0   then  gate_lock_acquire <host> <dir> <root> <branch>" >&2
            exit 2 ;;
    esac
fi
