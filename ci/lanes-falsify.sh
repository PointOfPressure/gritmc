#!/usr/bin/env bash
# ci/lanes-falsify.sh — prove `ci/lanes.sh` by breaking it.
#
#     ./ci/lanes-falsify.sh                  # every case, then every injection
#     ./ci/lanes-falsify.sh -v               # keep and print the drain log
#     LANES_FALSIFY_DRYRUN=1 ./ci/lanes-falsify.sh
#                                            # apply every patch, run no case
#
# **The injections are the point, not the cases.** A green case only says the
# instrument agreed with the code; an injection says the instrument would have
# noticed had the code been wrong. Every case here is required to go red under
# at least one deliberate break, and an injection that is NOT caught is
# reported as loudly as a failure — a patch that has stopped matching after an
# edit to `ci/lanes.sh` is a hole in the sweep whether or not anyone notices.
# Run `LANES_FALSIFY_DRYRUN=1` after touching `ci/lanes.sh` for exactly that.
#
# WHAT IS REAL AND WHAT IS FAKE, stated so nobody has to guess:
#
#   * REAL — `ci/lanes.sh`, unmodified except by a named injection; real git
#     repositories, real clones with real `--shared` alternates, real branches,
#     real rebases, real pushes to a real (local, bare) origin; `flock`; N
#     concurrent lane workers actually running at the same time; and
#     `ci/corpus-lock.sh`, unmodified, in a private lock namespace so this can
#     neither queue behind nor wedge a real corpus run.
#   * FAKE — the box. The gate itself is a stand-in, for the same reason
#     `ci/handoff-verdict-selftest.sh` stands in for the stages: `ci/lanes.sh`
#     learns what a gate did from exactly two things, its exit status and
#     whether it printed `GREEN`, and a stand-in producing those is
#     indistinguishable to it. What this cannot test is whether a real
#     `ci/handoff.sh` produces them — that is `ci/handoff-verdict-selftest.sh`'s
#     job, and it already does it.
#
# THE STAND-IN IS NOT A SLEEP. It takes the real corpus lock for its "T2"
# window and holds it, so the two properties that matter are both observable:
# lanes must **overlap** in their pre-lock work and must **not overlap** under
# the lock. A stub that only slept could show the first and never the second.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LANES_REAL="$ROOT/ci/lanes.sh"
CORPUS_REAL="$ROOT/ci/corpus-lock.sh"
[ -r "$LANES_REAL" ] || { echo "no ci/lanes.sh beside this script"; exit 2; }
[ -r "$CORPUS_REAL" ] || { echo "no ci/corpus-lock.sh beside this script"; exit 2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/lanes-falsify.XXXXXX")"
export CORPUS_LOCK_ANCHOR_NAME="lanes-falsify-anchor-$$"
# Its "locked windows never overlap" cases are about the MUTEX; the pool
# (2026-09-24) is falsified in ci/corpus-lock-falsify.sh. Pin the mutex here.
export RUSTMC_CORPUS_LOCK_POOL=1
cleanup() {
    pkill -f "$CORPUS_LOCK_ANCHOR_NAME" >/dev/null 2>&1
    pkill -f "$WORK" >/dev/null 2>&1
    sleep 0.3
    rm -rf "$WORK"
}
trap cleanup EXIT

VERBOSE=""
[ "${1:-}" = "-v" ] && VERBOSE=1
PASS=0; FAIL=0; MISSED=0
declare -a FAILED_CASES=() MISSED_INJECTIONS=()
ok()   { PASS=$((PASS+1)); printf '  PASS  %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); FAILED_CASES+=("$*"); printf '  FAIL  %s\n' "$*"; }
note() { [ -n "$VERBOSE" ] && printf '%s\n' "$*" | sed 's/^/        | /'; return 0; }

G="git -c user.name=t -c user.email=t@t -c commit.gpgsign=false"

# ---------------------------------------------------------------------------
# THE FIXTURE
# ---------------------------------------------------------------------------
#
# A bare repo standing in for GitHub, a "canonical checkout" clone standing in
# for /home/sayer/grit/repo, and one branch per queued item. Every branch
# appends a line to the SAME file, `index.md`, which is what the real doc index
# tables do and therefore what makes a stale rebase base show up as a conflict
# rather than as a silent success.
build_fixture() {
    local nbranches="$1" F="$WORK/fx"
    rm -rf "$F"; mkdir -p "$F"
    $G init -q --bare -b main "$F/origin.git"
    $G clone -q "$F/origin.git" "$F/repo" 2>/dev/null
    (
        cd "$F/repo"
        printf '# index\nbase\n' > index.md
        mkdir -p ci && printf 'seed\n' > ci/seed.txt
        $G add index.md ci/seed.txt
        $G commit -qm "base"
        $G push -q origin main
        for n in $(seq 1 "$nbranches"); do
            $G checkout -q -b "br$n" main
            printf 'row from br%s\n' "$n" >> index.md
            printf 'br%s\n' "$n" > "file$n.txt"
            $G add index.md "file$n.txt"
            $G commit -qm "br$n: one row in the shared index"
            $G push -q origin "br$n"
            $G checkout -q main
        done
    ) >/dev/null 2>&1
    echo "$F"
}

# ---------------------------------------------------------------------------
# THE STAND-IN GATE
# ---------------------------------------------------------------------------
#
# Records one row per invocation: lane build directory, branch, the tip it was
# handed, and the wall-clock window of its pre-lock work and of its locked
# work, separately. Those two windows are what cases 2 and 3 read.
#
# The verdict comes from a table the case writes, keyed by branch, so a case
# can say "br2 is red" without this script knowing anything about branches.
build_stub() {
    mkdir -p "$WORK/bin"
    cat > "$WORK/bin/stub-gate" <<EOS
#!/usr/bin/env bash
set -uo pipefail
BR="\$1"
REC="$WORK/rec.tsv"
VERDICTS="$WORK/verdicts.tsv"
HOOKS="$WORK/hooks.tsv"
DIR="\${RUSTMC_REMOTE_DIR:-UNSET}"
TIP="\$(git rev-parse HEAD)"
JOBS="\${CARGO_BUILD_JOBS:-UNSET}"

rec() { ( flock -x 220; printf '%s\t%s\t%s\t%s\t%s\t%s\n' "\$1" "\$DIR" "\$BR" "\$TIP" "\$2" "\$JOBS" >> "\$REC" ) 220>>"\$REC.lock"; }

# --- pre-lock work: this is the part that MUST overlap between lanes --------
rec prep-start "\$(date +%s.%N)"
sleep 1.2
rec prep-end "\$(date +%s.%N)"

# A hook lets a case perturb the world at a chosen moment — the external push
# that case 5 needs, fired from inside a lane's gate window so the lane really
# does discover main moved underneath it.
if [ -s "\$HOOKS" ]; then
    hook="\$(awk -F'\t' -v b="\$BR" '\$1==b{print \$2; exit}' "\$HOOKS")"
    if [ -n "\$hook" ]; then
        awk -F'\t' -v b="\$BR" '\$1!=b' "\$HOOKS" > "\$HOOKS.tmp" && mv "\$HOOKS.tmp" "\$HOOKS"
        bash -c "\$hook"
    fi
fi

# --- locked work: this is the part that MUST NOT overlap --------------------
# Inherited if the drain set one — that is what makes the \`lock_bypass\`
# injection observable. Hardcoding it here would have the stub quietly undo the
# very break the injection introduces, and the first version of this file did
# exactly that: \`lock_bypass\` gave each lane a private lock namespace and the
# stub overrode it back to the shared one, so the injection was uncatchable.
export RUSTMC_CORPUS_LOCK_DIR="\${RUSTMC_CORPUS_LOCK_DIR:-$WORK/corpuslock}"
export RUSTMC_CORPUS_LOCK_ROLE=gate
. "$CORPUS_REAL"
corpus_lock_acquire >/dev/null 2>&1
rec t2-start "\$(date +%s.%N)"
sleep 1.0
rec t2-end "\$(date +%s.%N)"
corpus_lock_release >/dev/null 2>&1

v="\$(awk -F'\t' -v b="\$BR" '\$1==b{print \$2; exit}' "\$VERDICTS" 2>/dev/null)"
if [ "\$v" = RED ]; then echo "RED      \$BR — stand-in verdict"; exit 1; fi
echo "GREEN    \$BR — stand-in verdict"
exit 0
EOS
    chmod +x "$WORK/bin/stub-gate"
}

# ---------------------------------------------------------------------------
# RUNNING ONE DRAIN
# ---------------------------------------------------------------------------
#
# The script under test is installed INTO the fixture checkout and run from
# there, rather than run in place against a foreign cwd. That is not tidiness:
# `ci/lanes.sh` derives `$ROOT` from its own path, so running the real file
# from a fixture directory would have it drive **the real repository** — which
# is exactly what the first version of this test did, and it read
# `origin/main` off the live tree and reported every fixture branch as "no such
# branch on origin". Installing it makes `$ROOT` resolution part of what is
# under test instead of something the harness quietly works around.
#
# `KEEP_LANES=1` reuses the lane clones from the previous drain instead of
# wiping them. Only `case_dirty_tree_is_infra` sets it, and only so that the
# state a real operator hits — a lane left dirty by an earlier run — can be set
# up out of real machinery rather than mocked.
#
# THE EXIT STATUS IS RECORDED, and it was not before. That is not an oversight
# being tidied: on 2026-08-16 `ci/lanes.sh` dropped 37 branches and exited 0,
# and no case in this file could have noticed, because none of them ever looked
# at what the drain returned.
run_drain() {
    local script="$1" F="$2" n="$3"; shift 3
    : > "$WORK/rec.tsv"; : > "$WORK/rec.tsv.lock"
    [ -n "${KEEP_LANES:-}" ] || rm -rf "$WORK/lanes"
    rm -rf "$WORK/corpuslock"
    mkdir -p "$WORK/lanes" "$F/repo/ci"
    cp "$script" "$F/repo/ci/lanes.sh"; chmod +x "$F/repo/ci/lanes.sh"
    (
        cd "$F/repo"
        RUSTMC_LANE_DIR="$WORK/lanes" \
        RUSTMC_LANE_STATE="$WORK/lanes/.state" \
        RUSTMC_LANE_LOG="$WORK/drain.log" \
        RUSTMC_LANE_GATE_CMD="$WORK/bin/stub-gate %BRANCH%" \
        RUSTMC_LANE_JOBS=3 \
        RUSTMC_LANE_UNIONABLE="index.md" \
        RUSTMC_CORPUS_LOCK_DIR="$WORK/corpuslock" \
        CORPUS_LOCK_ANCHOR_NAME="$CORPUS_LOCK_ANCHOR_NAME" \
        bash ./ci/lanes.sh -n "$n" "$@"
    ) > "$WORK/drain.out" 2>&1
    echo $? > "$WORK/drain.rc"
    note "$(tail -40 "$WORK/drain.out")"
}

drain_rc() { cat "$WORK/drain.rc" 2>/dev/null || echo missing; }

main_log() { $G -C "$1/origin.git" log --format='%s' main 2>/dev/null; }

# HOW MANY COMPLETED WINDOWS OF THIS KIND WERE OBSERVED AT ALL.
#
# Required by every timing case, and the reason is that the first version of
# this file shipped a check that could not fail: `case_lock_serialises` asserts
# "zero overlapping locked windows", and a drain that ran **no gates at all**
# satisfies that trivially. It duly reported PASS while every other case was
# failing because nothing had run — a green that was arithmetic, not evidence.
# Two windows is the minimum at which "did they overlap" is a question.
windows() {
    python3 - "$WORK/rec.tsv" "$1" <<'PY'
import sys
path, kind = sys.argv[1], sys.argv[2]
opens, n = set(), 0
for line in open(path):
    p = line.rstrip('\n').split('\t')
    if len(p) < 5: continue
    key = (p[1], p[2], p[3])
    if p[0] == f'{kind}-start': opens.add(key)
    elif p[0] == f'{kind}-end' and key in opens: opens.discard(key); n += 1
print(n)
PY
}

# window helpers: does any pair of rows of kind $2 overlap in time?
overlap_count() {
    awk -F'\t' -v k="$1" '
        $1==k"-start"{s[$2"|"$3"|"NR]=$5; ord[++n]=$2"|"$3"|"NR}
        $1==k"-end"  {last=$2"|"$3; e[last]=$5}
        END{ }' /dev/null 2>/dev/null
    python3 - "$WORK/rec.tsv" "$1" <<'PY'
import sys, collections
path, kind = sys.argv[1], sys.argv[2]
opens, spans = {}, []
for line in open(path):
    p = line.rstrip('\n').split('\t')
    if len(p) < 5: continue
    ev, d, br, tip, t = p[0], p[1], p[2], p[3], float(p[4])
    key = (d, br, tip)
    if ev == f'{kind}-start': opens[key] = t
    elif ev == f'{kind}-end' and key in opens:
        spans.append((opens.pop(key), t, d, br))
n = 0
for i in range(len(spans)):
    for j in range(i+1, len(spans)):
        a, b = spans[i], spans[j]
        if a[0] < b[1] and b[0] < a[1]: n += 1
print(n)
PY
}

# ---------------------------------------------------------------------------
# THE CASES
# ---------------------------------------------------------------------------
#
# Each returns 0 if the property HOLDS. An injection is caught when a case that
# passed on the real script fails on the patched one.

# case 1 — every green branch reaches main, exactly once, in train order, and
# main is a linear fast-forward chain. This is the "push race loses a commit"
# detector: a lost merge shows up as a missing subject line.
case_drain() {
    local script="$1" F; F="$(build_fixture 4)"
    : > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 2 br1 br2 br3 br4
    local got; got="$(main_log "$F" | grep -c '^br[0-9]*: one row')"
    [ "$got" -eq 4 ] || { note "main holds $got of 4 branch commits"; note "$(main_log "$F")"; return 1; }
    # linear: every commit has exactly one parent
    local merges; merges="$($G -C "$F/origin.git" log --merges --format=%h main | wc -l)"
    [ "$merges" -eq 0 ] || { note "main has $merges merge commits; ff-only was not preserved"; return 1; }
    # AND A HEALTHY DRAIN EXITS 0. This is the other side of case 8 and without
    # it that case is satisfiable by a script that always fails, which would be
    # a worse instrument than the one that always passed.
    local rc; rc="$(drain_rc)"
    [ "$rc" = 0 ] || { note "a drain that merged all 4 branches exited $rc"; return 1; }
    return 0
}

# case 2 — lanes actually run at the same time. Goes red if lanes silently
# serialise, which is the failure this whole file exists to make impossible to
# ship unnoticed: a "2-lane" drain that is really 1 lane is indistinguishable
# from a working one by its output alone.
case_concurrency() {
    local script="$1" F; F="$(build_fixture 4)"
    : > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 2 br1 br2 br3 br4
    local w; w="$(windows prep)"
    [ "${w:-0}" -ge 2 ] || { note "only $w pre-lock windows observed — nothing to compare"; return 1; }
    local n; n="$(overlap_count prep)"
    note "overlapping pre-lock gate windows: $n of $w"
    [ "${n:-0}" -ge 1 ]
}

# case 3 — two lanes never share a build directory, and never concurrently.
# `RUSTMC_REMOTE_DIR` is what keeps one lane's 7 GB target/ out of another
# lane's rsync --delete; sharing one is the 2026-08-03 defect by another door.
case_distinct_dirs() {
    local script="$1" F; F="$(build_fixture 4)"
    : > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 2 br1 br2 br3 br4
    local dirs; dirs="$(cut -f2 "$WORK/rec.tsv" | sort -u | grep -c .)"
    note "distinct build directories seen: $dirs"
    [ "$dirs" -ge 2 ] || { note "$(cut -f2 "$WORK/rec.tsv" | sort | uniq -c)"; return 1; }
    # and no single directory was in two gates at once
    local clash
    clash="$(python3 - "$WORK/rec.tsv" <<'PY'
import sys, collections
opens, spans = {}, []
for line in open(sys.argv[1]):
    p = line.rstrip('\n').split('\t')
    if len(p) < 5: continue
    ev, d, br, tip, t = p[0], p[1], p[2], p[3], float(p[4])
    if ev == 'prep-start': opens[(d,br,tip)] = t
    elif ev == 't2-end' and (d,br,tip) in opens:
        spans.append((opens.pop((d,br,tip)), t, d))
n = 0
for i in range(len(spans)):
    for j in range(i+1, len(spans)):
        a, b = spans[i], spans[j]
        if a[2] == b[2] and a[0] < b[1] and b[0] < a[1]: n += 1
print(n)
PY
)"
    note "same-directory concurrent gates: $clash"
    [ "${clash:-1}" -eq 0 ]
}

# case 4 — the corpus lock still serialises. Lanes must CONTEND for it, not
# route around it: vanilla reference boots are what everything is measured
# against and two overlapping ones perturb each other's settle_ms timers.
case_lock_serialises() {
    local script="$1" F; F="$(build_fixture 4)"
    : > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 2 br1 br2 br3 br4
    local w; w="$(windows t2)"
    [ "${w:-0}" -ge 2 ] || { note "only $w locked windows observed — the assertion is vacuous"; return 1; }
    local n; n="$(overlap_count t2)"
    note "overlapping locked windows: $n of $w (must be 0)"
    [ "${n:-1}" -eq 0 ]
}

# case 5 — A RED BRANCH MUST NEVER REACH MAIN THROUGH A SUCCESSOR.
#
# This is the sharp one and it is the real face of the push race here. br2's
# speculative base is br1's gated tip. If br1 goes RED and br2 is green, then
# br2's tip contains br1's commit and is still a fast-forward of origin/main —
# so the push SUCCEEDS and lands a branch the gate rejected. Nothing about the
# push itself refuses it; only the base check and the invalidation do.
case_red_does_not_ride_along() {
    local script="$1" F; F="$(build_fixture 3)"
    printf 'br1\tRED\n' > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 2 br1 br2 br3
    local log; log="$(main_log "$F")"
    note "$log"
    if printf '%s' "$log" | grep -c '^br1: ' >/dev/null; then
        note "br1 was RED and reached main anyway"; return 1
    fi
    # and the successors must still get in, on a corrected base
    printf '%s' "$log" | grep -c '^br2: ' >/dev/null || { note "br2 never merged after br1 was dropped"; return 1; }
    return 0
}

# case 6 — main moving from outside during a gate must void that gate, not
# merge it. The hook pushes an unrelated commit to origin/main while br1 is
# mid-gate, so br1's gated base is stale by the time it asks to merge.
case_external_move_voids() {
    local script="$1" F; F="$(build_fixture 2)"
    : > "$WORK/verdicts.tsv"
    printf 'br1\tcd %s/repo && %s checkout -q -B outside main && echo outside >> index.md && %s commit -aqm "outside: not gated here" && %s push -q origin outside:main && %s checkout -q --detach\n' \
        "$F" "$G" "$G" "$G" "$G" > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 1 br1
    local log; log="$(main_log "$F")"
    note "$log"
    printf '%s' "$log" | grep -c '^outside: ' >/dev/null || { note "the external push never landed; fixture broken"; return 1; }
    # br1 was gated against a base that no longer is main. It may be re-gated
    # and land later, but it must not have been pushed on the stale base: if it
    # had, the `outside` commit would have been overwritten or the history
    # would not contain both.
    if printf '%s' "$log" | grep -c '^br1: ' >/dev/null && ! printf '%s' "$log" | grep -c '^outside: ' >/dev/null; then
        note "br1 merged over the external commit"; return 1
    fi
    grep -q 'VOID br1' "$WORK/drain.log" 2>/dev/null || {
        note "no VOID for br1 — the stale base was not detected"; return 1; }
    return 0
}

# case 7 — A DIRTY LANE TREE IS INFRASTRUCTURE, NOT A BRANCH VERDICT.
#
# THE HOLE THIS FILE HAD, AND ITS EXACT SIZE. `ci/lanes.sh` was put into service
# on 2026-08-16 having passed this sweep, and its first drain dropped all 37
# queued branches in 33 seconds. It wrote the per-lane job cap into the lane's
# `.cargo/config.toml`, which is **tracked**; git refuses to rebase a dirty
# tree; and every branch was logged
# `SKIP <branch>: REBASE CONFLICT onto 5f7332f9, needs hands` over a `lanes.log`
# reading `error: cannot rebase: You have unstaged changes.` Twenty-six of those
# branches were fine, and a human spent a session inspecting them.
#
# Nothing here could have caught it, because every case ran a drain in lanes
# this file had just created and therefore never dirty. So the case has to make
# a lane dirty itself, and then assert the three things that were wrong:
#
#   1. it is named INFRA, not `REBASE CONFLICT` — a rebase that could not START
#      is not a statement about the branch it declined to rebase;
#   2. the lane STOPS instead of walking the queue manufacturing verdicts;
#   3. the branches it never reached are left queued, not dropped.
#
# Two arms because there are two guards at two moments: `_admit`'s runs per
# admission and catches dirt appearing mid-drain, `lane_setup`'s runs once
# before any worker starts and catches a lane left dirty by an earlier run. The
# second arm reuses the first arm's dirty lane, which is how a real operator
# meets this.
case_dirty_tree_is_infra() {
    local script="$1" F; F="$(build_fixture 3)"
    : > "$WORK/verdicts.tsv"
    # arm A — the tree goes dirty during br1's gate, so br2's rebase is the one
    # that cannot run. `ci/seed.txt` is the fixture's tracked non-branch file:
    # untracked litter must NOT trip this, and picking a tracked file is what
    # makes the arm faithful to `.cargo/config.toml`.
    printf 'br1\tprintf "jobs = 3\\n" >> ci/seed.txt\n' > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 1 br1 br2 br3
    local rc; rc="$(drain_rc)"
    note "$(grep -E 'INFRA|SKIP|REBASE|reached a gate' "$WORK/drain.out")"
    if grep -q 'REBASE CONFLICT' "$WORK/drain.out"; then
        note "a dirty tree was reported as REBASE CONFLICT — a verdict with no evidence"; return 1
    fi
    grep -q 'INFRA' "$WORK/drain.out" \
        || { note "a lane could not rebase and nothing said so"; return 1; }
    [ "$rc" != 0 ] || { note "a lane stopped on an unusable tree and the drain still exited 0"; return 1; }
    local dropped; dropped="$(awk -F'\t' '$5=="dropped"' "$WORK/lanes/.state/train.tsv" 2>/dev/null | wc -l)"
    [ "$dropped" -eq 0 ] \
        || { note "$dropped branch(es) dropped by a lane that could not judge them"; return 1; }
    local left; left="$(grep -c . "$WORK/lanes/.state/queue.txt" 2>/dev/null || true)"
    [ "${left:-0}" -ge 1 ] || { note "the unreached branches were consumed off the queue anyway"; return 1; }

    # arm B — the lane is still dirty. A drain must refuse before it touches the
    # queue at all, rather than admitting a branch it cannot rebase.
    KEEP_LANES=1
    run_drain "$script" "$F" 1 br2 br3
    KEEP_LANES=""
    rc="$(drain_rc)"
    note "$(grep -E 'INFRA|REFUSED' "$WORK/drain.out")"
    [ "$rc" != 0 ] || { note "a drain started in a dirty lane and exited 0"; return 1; }
    grep -q 'REFUSED before any branch was touched' "$WORK/drain.out" \
        || { note "lane_setup admitted a branch into a lane no rebase can run in"; return 1; }
    return 0
}

# case 8 — A QUEUE THAT REACHED NO GATE MUST EXIT NON-ZERO.
#
# The second half of the same 2026-08-16 failure and the reason it was not
# caught by the supervisor either: draining 37 branches to zero merges took 33
# seconds and returned **0**, which is byte-for-byte what a healthy drain of an
# empty queue returns. The fallback that should have fired could not tell a
# drained queue from a destroyed one.
#
# The skips here are legitimate — branches that do not exist on origin — so this
# asserts the exit rule and not the dirty-tree path: a run in which nothing was
# gated is a refusal whatever the reason, because "nothing was measured" is the
# fact a supervisor has to act on. `case_drain` holds the other side, that a
# drain which did gate its queue exits 0.
case_no_gate_reached() {
    local script="$1" F; F="$(build_fixture 2)"
    : > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"
    run_drain "$script" "$F" 2 ghost1 ghost2 ghost3
    local rc; rc="$(drain_rc)"
    local w; w="$(windows prep)"
    [ "${w:-0}" -eq 0 ] || { note "$w gate(s) ran; this case needs a drain that gated nothing"; return 1; }
    note "3 branches queued, 0 gates run, drain exited $rc"
    [ "$rc" != 0 ] || { note "every branch skipped and the drain exited 0"; return 1; }
    return 0
}

# case 9 — MUST NOT RUN AGAINST THE CHAIN'S OWN CHECKOUT.
#
# The 2026-08-17 outage: a real `ci/lanes.sh` run whose `$ROOT` resolved to
# `/home/sayer/grit/repo` — the checkout `gate-chain.sh` holds a branch
# checked out in for the whole of a gate — left it with 11 staged changes, and
# the single-lane chain refused all 36 queued branches on `working tree
# dirty — an agent may still be live`, 861 times over ~25 minutes, until a
# human reset it by hand. `run_drain` already installs the script under test
# INTO the fixture's `$F/repo` and runs it from there — the harness's own
# stand-in for the canonical checkout — so pointing `RUSTMC_CANONICAL_CHECKOUT`
# at that same path is the faithful arm: it makes THIS drain's `$ROOT`
# literally be the path the guard exists to refuse, the same way a real
# `ci/lanes.sh` invocation launched from inside `/home/sayer/grit/repo` would.
#
# Two arms. A: canonical == the fixture's checkout — refused, before any
# branch is touched, no gate command ever runs. B: canonical == the untouched
# default (which the fixture is not) — the same fixture drains normally, so
# the guard is proven scoped rather than a blanket refusal that would also
# explain arm A passing.
case_canonical_checkout_refused() {
    local script="$1" F; F="$(build_fixture 2)"
    : > "$WORK/verdicts.tsv"; : > "$WORK/hooks.tsv"

    # arm A — canonical checkout IS the tree this drain would run in.
    export RUSTMC_CANONICAL_CHECKOUT="$F/repo"
    run_drain "$script" "$F" 1 br1 br2
    local rc; rc="$(drain_rc)"
    unset RUSTMC_CANONICAL_CHECKOUT
    note "$(grep -E 'REFUSED|canonical checkout' "$WORK/drain.out")"
    [ "$rc" != 0 ] || { note "a drain run against the canonical checkout exited 0"; return 1; }
    grep -q 'REFUSED.*canonical checkout' "$WORK/drain.out" \
        || { note "refused, but not by name — a reader cannot tell this from any other refusal"; return 1; }
    local w; w="$(windows prep)"
    [ "${w:-0}" -eq 0 ] || { note "$w gate(s) ran against the canonical checkout before the refusal"; return 1; }
    [ -d "$WORK/lanes/.state" ] \
        && { note "lane state was initialised before the refusal — the guard ran too late"; return 1; }

    # arm B — same fixture, canonical checkout pointed elsewhere (the default,
    # which this fixture is not) — must drain exactly as every other case does.
    run_drain "$script" "$F" 1 br1 br2
    rc="$(drain_rc)"
    note "arm B (uncontended) rc=$rc"
    [ "$rc" = 0 ] || { note "the guard fired against a tree that was never the canonical checkout"; return 1; }
    grep -qF 'br1: one row' <(main_log "$F") || { note "br1 did not reach main with the guard silent"; return 1; }
    grep -qF 'br2: one row' <(main_log "$F") || { note "br2 did not reach main with the guard silent"; return 1; }
    return 0
}

CASES="drain concurrency distinct_dirs lock_serialises red_does_not_ride_along external_move_voids dirty_tree_is_infra no_gate_reached canonical_checkout_refused"

run_case() { case_"$1" "$2"; }

# ---------------------------------------------------------------------------
# THE INJECTIONS
# ---------------------------------------------------------------------------
#
# Each is a sed patch against a COPY of ci/lanes.sh, plus the case that must
# catch it. A patch that does not change the file is a hole in the sweep and is
# reported as `MISSED — patch did not match`.
inject() {
    local name="$1" src="$WORK/inj-$name.sh"
    cp "$LANES_REAL" "$src"
    case "$name" in
    serialise)
        # "lanes" that are really one lane: wait for each worker before
        # starting the next. Output is otherwise identical.
        perl -0pi -e 's/for i in \$\(seq 1 "\$LANES"\); do\n    lane_worker "\$i" & pids\+=\("\$!"\)\ndone/for i in \$(seq 1 "\$LANES"); do\n    lane_worker "\$i" \& pids+=("\$!"); wait "\$!"\ndone/' "$src" ;;
    shared_dir)
        perl -0pi -e 's/RD="\$LANE_REMOTE_PREFIX-\$i"/RD="\$LANE_REMOTE_PREFIX-shared"/' "$src" ;;
    # BOTH speculation guards at once, and the reason it is one injection
    # rather than two is a finding rather than a convenience.
    #
    # Disabling either one ALONE changes nothing observable, because they are
    # redundant with each other and with git's own refusal of a non-fast-forward
    # push. Measured while writing this file:
    #
    #   * epoch check off, base check on  -> lane 2 finds origin/main is not its
    #     base, voids, re-queues, re-gates on the corrected base. Correct.
    #   * epoch check on, base check off  -> lane 2 finds its epoch stale and
    #     voids before it ever looks at main. Correct.
    #   * both off                        -> lane 2 pushes base+br1+br2, which
    #     IS a fast-forward of origin/main, so git accepts it and the RED br1
    #     lands on main. Wrong, and caught.
    #
    # So neither is a single point of failure and this file does not claim they
    # are. What it claims — and proves — is that the pair is load-bearing.
    # `_predecessors_done` is deliberately NOT injected: it is an efficiency
    # measure that turns a would-be VOID into a wait, and its absence is caught
    # by nothing here. Saying so is cheaper than shipping an injection that
    # cannot be caught and calling the sweep complete.
    no_speculation_guards)
        perl -0pi -e 's/if \[ "\$\(_read epoch\)" != "\$ep" \]; then\n            void="its train base was invalidated while it gated"\n        elif \[ "\$cur" != "\$base" \]; then\n            void="origin\/main moved to \$\{cur:0:8\} from the gated base \$\{base:0:8\}"/if false; then\n            void="disabled"\n        elif false; then\n            void="disabled"/' "$src"
        perl -0pi -e 's/^_invalidate_after\(\) \{/_invalidate_after() {\n    echo 0; return 0/m' "$src" ;;
    lock_bypass)
        # A lane that "optimises" by giving each lane its own corpus lock
        # namespace — the exact shortcut the header refuses.
        perl -0pi -e 's/( +)\( cd "\$L" && RUSTMC_REMOTE_DIR="\$RD"/$1( cd "\$L" \&\& RUSTMC_CORPUS_LOCK_DIR="\$STATE\/cl-\$i" RUSTMC_REMOTE_DIR="\$RD"/' "$src" ;;

    # --- the 2026-08-16 defect, in both of its halves -----------------------
    dirty_tree_misreported)
        # THE MISREPORT ITSELF. Blind the dirty-tree predicate and everything
        # downstream reverts to the shipped behaviour: the rebase declines to
        # start, all three attempts fail, and the branch is written off as
        # `REBASE CONFLICT ... needs hands`. This is the exact defect, not an
        # analogue of it, and `case_dirty_tree_is_infra` is what it costs.
        perl -0pi -e 's/^lane_dirty\(\) \{.*\}$/lane_dirty() { :; }/m' "$src" ;;
    lane_dirties_tree)
        # THE CAUSE. A lane setup that writes a TRACKED file, which is what
        # `.cargo/config.toml` was. `ci/seed.txt` is the fixture's stand-in for
        # it; the injection is "this script dirties its own tree", and the file
        # it picks only has to be one the fixture tracks. Every rebase in every
        # lane then fails and not one branch reaches main.
        perl -0pi -e 's/(    git -C "\$L" config user\.email .*\n)/$1    printf "jobs = 3\\n" > "\$L\/ci\/seed.txt"\n/' "$src" ;;
    silent_zero_exit)
        # THE SUPERVISOR'S HALF. Drain the queue to nothing and still return 0,
        # so a destroyed queue is indistinguishable from a healthy empty one.
        perl -0pi -e 's/if \[ "\$QUEUED" -gt 0 \] && \[ "\$GATED" -eq 0 \]; then/if false; then/' "$src" ;;
    always_nonzero_exit)
        # The cheap way to satisfy `case_no_gate_reached`: never exit 0 at all.
        # An instrument that always says "failed" measures nothing, exactly as
        # one that always says "passed" does, so both directions are pinned.
        perl -0pi -e 's/^exit 0$/exit 9/m' "$src" ;;
    *) echo "unknown injection $name" >&2; return 2 ;;
    esac
    if cmp -s "$src" "$LANES_REAL"; then return 1; fi
    echo "$src"
}

declare -A CATCHER=(
    [serialise]=concurrency
    [shared_dir]=distinct_dirs
    [no_speculation_guards]=red_does_not_ride_along
    [lock_bypass]=lock_serialises
    [dirty_tree_misreported]=dirty_tree_is_infra
    [lane_dirties_tree]=drain
    [silent_zero_exit]=no_gate_reached
    [always_nonzero_exit]=drain
)

build_stub

# --- dry run: only prove every patch still matches --------------------------
if [ -n "${LANES_FALSIFY_DRYRUN:-}" ]; then
    echo "lanes-falsify: DRY RUN — applying every patch, running no case"
    rc=0
    for name in "${!CATCHER[@]}"; do
        if inject "$name" >/dev/null; then printf '  PATCH-OK   %s\n' "$name"
        else printf '  PATCH-DEAD %s — no longer matches ci/lanes.sh\n' "$name"; rc=1; fi
    done
    [ "$rc" -eq 0 ] && echo "lanes-falsify: all ${#CATCHER[@]} patches still match" \
                    || echo "lanes-falsify: a patch has stopped matching — the sweep has a hole"
    exit "$rc"
fi

echo "lanes-falsify: ${#CATCHER[@]} injections, $(printf '%s' "$CASES" | wc -w) cases"
echo
echo "-- the real ci/lanes.sh: every case must PASS --------------------------"
declare -A BASELINE=()
for c in $CASES; do
    if run_case "$c" "$LANES_REAL"; then BASELINE[$c]=pass; ok "case $c"
    else BASELINE[$c]=fail; bad "case $c — fails on the UNMODIFIED script"; fi
done

echo
echo "-- injections: the named case must go RED ------------------------------"
for name in "${!CATCHER[@]}"; do
    c="${CATCHER[$name]}"
    src="$(inject "$name")" || {
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name — patch did not match ci/lanes.sh")
        printf '  MISSED  %s — patch did not match; the sweep has a hole here\n' "$name"; continue; }
    if [ "${BASELINE[$c]:-}" != pass ]; then
        printf '  SKIP    %s — its catcher (%s) is already failing\n' "$name" "$c"; continue
    fi
    if run_case "$c" "$src"; then
        MISSED=$((MISSED+1)); MISSED_INJECTIONS+=("$name — case $c stayed green")
        printf '  MISSED  %s — case %s stayed GREEN under a deliberate break\n' "$name" "$c"
    else
        ok "injection $name caught by case $c"
    fi
done

echo
printf 'lanes-falsify: %d passed, %d failed, %d injections not caught\n' "$PASS" "$FAIL" "$MISSED"
if [ "$FAIL" -gt 0 ]; then printf '  failed: %s\n' "${FAILED_CASES[@]}"; fi
if [ "$MISSED" -gt 0 ]; then printf '  uncaught: %s\n' "${MISSED_INJECTIONS[@]}"; fi
[ "$FAIL" -eq 0 ] && [ "$MISSED" -eq 0 ]
