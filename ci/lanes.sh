#!/usr/bin/env bash
# ci/lanes.sh — drain a branch queue over N gate lanes.
#
#     ./ci/lanes.sh [-n N] <branch> [branch...]
#     ./ci/lanes.sh [-n N] --queue <file>
#
# Successor to the untracked `gate-chain.sh`, which gates one branch at a time.
# It is in the repo rather than beside it because it has a falsification sweep
# (`ci/lanes-falsify.sh`) and a sweep that is not gated is a sweep that rots.
#
# ===========================================================================
# WHAT WAS MEASURED FIRST, BECAUSE IT DECIDES N AND IT REFUTES THE PREMISE
# ===========================================================================
#
# The item that produced this file was briefed as "make the gate run N lanes,
# a correct 3-lane gate is still ~3x". **It is not ~3x. It is 1.40x, and the
# ceiling does not move with N.** Measured 2026-08-16 over the 158 gate logs in
# `~/rustmc-private/gates` and the 2,234-line `gate-chain.log`, on
# sayer-desktop (Threadripper 1920X, 24 threads, 62 GB):
#
#   * **The corpus lock is held for 38.4 minutes of every merge gate.**
#     n = 6, spread 36.3..44.2, and this is a *direct* reading rather than a
#     difference of medians: `ci/corpus-lock.sh`'s waiter heartbeat prints the
#     holder's directory and its acquisition timestamp, so for six gates the
#     exact instant stage 6 took the box-wide lock is recoverable from another
#     process's log. Held = that instant to the gate log's last write.
#   * **Stages 1-5 cost 15.5 minutes** (rsync, gen-gameplay-tables, T0, T1).
#     Same six gates, pre-lock time minus that gate's own measured lock wait:
#     14.8, 15.4, 15.1, 16.0, 21.9, and 16.0 for the one gate that never
#     waited. Consistent to about a minute across five days.
#   * So a merge gate is **71% serialised** (38.4 of 53.9) and Amdahl caps any
#     number of lanes at **1.40x**.
#
# **SCOPED TO THE UNCACHED WORLD — DO NOT QUOTE THE 38.4/53.9/1.40x TRIPLET
# FORWARD WITHOUT THIS LINE. 2026-08-17.** The 158 gates above all ran before
# `ci/reference-cache.sh` existed. `docs/CORPUS-LOCK-HOLD.md` has since
# decomposed the in-lock record work at N=106 (its own, more precise
# reading — not this file's 38.4): vanilla reference recording 21.3 min
# (49.8%, does NOT depend on the candidate), rustmc subject recording 21.6 min
# (50.2%, depends by construction), compare + floor report <0.1 min. A cache
# hit removes only the FIRST of those — `ci/t2-gameplay.sh`'s own mechanism is
# "no vanilla boots at all" — so the projected hold is ~21.6 min, not
# near-zero: **1.99x on the serialised section alone**, not a bigger number.
# The rustmc half is a FLOOR, not a lever this file's lane count can touch —
# `ci/corpus-lock.sh`'s own header is explicit that the lock is about resource
# contention rather than correctness, and an overlapped pair of subject
# recordings measured 2 scenarios in 11 minutes against ~12s solo, so a second
# lane's subject recording cannot run inside a first lane's lock window either.
# Amdahl off the corrected split (area brief "gate throughput", item 3,
# 2026-08-17): **N=2 ~1.46x, N=3 ~1.72x** — corrected from an earlier ~1.54x/
# ~1.89x guess that assumed the vanilla half was the whole win rather than
# half of it. Still above this file's own N=2 pin, so N itself may no longer
# be the saturation point once the cache is live, but the ceiling is lower
# than first estimated.
# `docs/CORPUS-LOCK-HOLD.md` §5 names the one measurement left: a live gate's
# hold with the flag on, against the ~21.6 min projection. Area brief "gate
# throughput", item 3, carries that number when it exists; this comment does
# not update
# itself.
#
# **N = 2, and the reason is that the lock saturates there.** One lane is
# always inside the 38.4-minute corpus lock; the other has 38.4 minutes of
# wall in which to do 15.5 minutes of stages 1-5 for the next branch. It
# finishes with 23 minutes to spare, so a third lane has nothing to do that a
# second lane has not already done — it would add a third 7.2 GB build
# directory (measured: `~/rustmc-stack` is 7.2 G, 7.0 G of it `target/`), a
# third of the box's threads, and a third set of vanilla JVMs, in exchange for
# throughput the lock will not issue. N is a parameter so a later measurement
# can move it; the default is the measured saturation point, not a guess.
#
# **THE LARGER WIN IS NOT THE SPEEDUP AND IT IS WORTH SAYING SO.** Of ~71
# attempts in `gate-chain.log`, only 21 merged. The single biggest loss is
# **24 attempts refused with `another gate already owns the box directory
# ~/rustmc-stack`** — one third of the queue, spent in seconds, on a mutex over
# a directory rather than on any property of a branch. Per-lane build
# directories delete that failure outright, and a lane that finds the box busy
# now waits for the lock instead of refusing the branch. A further 23 attempts
# were `SKIP — REBASE CONFLICT, needs hands`, which is what a queue that does
# not drain produces: every branch appends to the same doc index tables, so
# conflict probability rises with time spent queued. 1.40x on the gate is
# worth less here than not throwing a third of the queue away.
#
# ===========================================================================
# WHY LANES WITHOUT SPECULATION ARE WORTH EXACTLY ZERO
# ===========================================================================
#
# This is the part that decides the whole design, so it is written out.
#
# `ci/handoff.sh` refuses a branch that is `BEHIND` main (exit 3) and merges
# with `--ff-only`. Both are load-bearing: they are what makes main's new tip
# *the very tree the gate measured*. Give that up and main advances to a tree
# nothing gated.
#
# Now run two lanes naively. Lane A gates B1 on main@X; lane B gates B2 on
# main@X at the same time. A goes green and merges, main becomes X+B1. B is now
# behind, so B's 54 minutes are void and B must rebase and re-gate in full.
# **Throughput is one branch per gate, exactly as with one lane, plus the
# wasted lane.** Two lanes that both gate against the same base cannot both
# merge, and no amount of retry logic changes that.
#
# So the only sound way to overlap anything is to gate each branch against the
# tree it will actually land on — which means gating against a base that
# *includes* the branches ahead of it in the queue, before those have merged.
# That is speculative chaining (a merge train), and it is what this file does.
#
#   train position 1:  base = origin/main
#   train position 2:  base = position 1's gated tip
#   train position 3:  base = position 2's gated tip
#
# Every branch is therefore gated on exactly the tree that becomes main when
# its predecessors merge, and its merge is a fast-forward by construction. The
# gated tree and the merged tree are the same tree, which is the property the
# whole gate exists to provide, and it is preserved rather than traded away.
#
# When a predecessor goes RED it is dropped from the train, and **every
# successor's base is now a tree that will never exist**. Those successors are
# invalidated, re-based and **re-gated from stage 1**. That is the cost of
# speculation and it is paid honestly: 118 of the 145 gates in the record that
# reached a verdict were green (81%), so a two-deep train wastes a lane's work
# on roughly one branch in five.
#
# ===========================================================================
# THE PUSH RACE IS ELIMINATED, NOT RETRIED — AND WHAT A RETRY RE-RUNS
# ===========================================================================
#
# The brief for this file asked what a rebase-and-retry should re-run, given
# that a rebase after a green gate means the gated tree is no longer the tree
# being pushed. **The answer here is that no such rebase happens.** A lane
# merges only the exact commit it gated, and it merges it only when
# `origin/main` is bit-for-bit the base that commit was gated on:
#
#   * the base check and the push are inside one `flock` (`merge.lock`), so two
#     lanes cannot both pass the check and then both push;
#   * a lane at train position k waits for positions 1..k-1 to merge before it
#     even asks, so when it does ask, `origin/main` *is* its base;
#   * if the check fails anyway — anything at all moved main from outside this
#     process — the gate is declared **void** and the branch goes back to the
#     queue for a **full re-gate**. It is never merged after a rebase.
#
# **The retry re-runs every stage: rsync, gen-gameplay-tables, T0, T1 and the
# full T2 corpus. Nothing is scoped, and scoping it would be unsound here.**
# The tempting cheap retry is "re-run only what main's incoming commits
# touched". It fails on this repo for a reason that is not a matter of degree:
# several gate checks are functions of the *union* and of neither side.
#
#   * `ci/check-doc-claims.py` refuses a duplicate `(doc, number)`. Two
#     branches claiming the same `## N.` are each green alone and red
#     together. This is not hypothetical — a duplicate `## 22.` reached main
#     on 2026-08-12 (see `gate-chain.sh`'s union resolver) precisely because a
#     check read one side and not the union.
#   * `ci/figures.toml` ids must be globally unique, and the doc-union
#     resolver can produce a file that parses only because both sides added.
#   * `ci/t1-allowlist.py`'s `min_binaries` floor and the ratcheted floors in
#     `ci/corpus-falsifiable.py` are counts over the whole tree.
#
# A scoped retry would need a per-check map of what each check reads. No such
# map exists in this tree, and one written by hand would be the exact defect
# `docs/AGENT-BRIEF.md` names as this project's central one: when it is wrong
# it under-scopes **silently**, skipping a check that would have gone red,
# rather than failing. It is a guard that cannot be shown to catch anything.
# So: the expensive retry, and it is expensive on purpose.
#
# ===========================================================================
# WHAT STAYS SERIALISED, AND WHY A BLOCKED LANE HOLDS NOTHING HOSTAGE
# ===========================================================================
#
# `ci/corpus-lock.sh` stays exactly as it is and lanes **contend for it** —
# there is no lane-aware bypass and there must not be. The lock is not a CPU
# mutex. It exists because a corpus run is asleep in `settle_ms` timers and
# vanilla boots, so two concurrent runs do not share a busy core, they wait out
# each other's timers: measured 2026-08-06, three overlapping runs fell to one
# scenario per ten minutes at load average 0.02. Vanilla is the reference
# everything is measured against, and a reference recorded while something else
# perturbed its timing is a reference with no standing. Routing lanes around
# that lock would buy throughput by corrupting the only instrument this project
# has.
#
# **2026-09-24: the mechanism in that paragraph was re-measured and is wrong**
# (branch `corpus-parallel`, `docs/FINDINGS.md`): the 08-06 runs shared ONE
# vanilla through one hardcoded port pair, and recorders sharing a server kick
# each other (`duplicate_login`). The lock is now a bounded pool
# (`RUSTMC_CORPUS_LOCK_POOL`), so two lanes can record at once, each on its own
# ports and its own tree. Lanes still contend for it and still must not bypass
# it; what changed is how many it admits.
#
# A lane blocked on the corpus lock holds its own build directory and nothing
# else. Build directories are **statically assigned**, `rustmc-lane-<i>`, one
# per lane for the life of the run. A pool would be worse, not better: handing
# a blocked lane's directory to another lane means the blocked lane must
# re-rsync and re-build 7 GB of `target/` when it wakes, which is the cost the
# per-directory `ci/gate-lock.sh` mutex exists to protect. The directory a
# blocked lane is holding contains that lane's own tree, already built, and it
# is used the instant the lock frees. Nothing else can want it.
#
# ===========================================================================
# RESOURCE CAPS
# ===========================================================================
#
# Neither `ci/t0.sh` nor `ci/t1.sh` sets `-j` anywhere — measured, zero hits —
# so cargo defaults to one job per hardware thread and every lane assumes the
# whole box. `RUSTMC_LANE_JOBS` defaults to `nproc / N` and is exported as
# `CARGO_BUILD_JOBS`, which cargo honours without any gate script having to
# know lanes exist. That export does not survive the ssh into the box, and
# `ci/handoff.sh`'s `remote()` splices it onto the command string — see the
# block above `remote()` there. **Nothing about the cap touches the tree.**
#
# IT USED TO TOUCH THE TREE, AND THAT COST THE WHOLE FIRST DRAIN. 2026-08-16.
#
# This file used to write `[build] jobs = N` into the lane's own
# `.cargo/config.toml` before the rsync, so the value travelled with the
# checkout. The reasoning was sound and the file was the wrong one: it is
# **tracked**. Every lane worktree was therefore permanently modified, git
# refuses to rebase a dirty tree, and the first real drain dropped all 37
# queued branches in 33 seconds — every one of them logged as
# `SKIP <branch>: REBASE CONFLICT onto 5f7332f9, needs hands` over a
# `lanes.log` reading `error: cannot rebase: You have unstaged changes.` The
# cap and the rebase were fighting and the rebase lost on every branch,
# forever. Twenty-six of those branches were fine.
#
# Two further things it broke, neither of which had had a chance to show:
#
#   * the write **overwrote** that file rather than adding to it, and that file
#     is where `target-cpu=x86-64-v2` is set. Every lane gate would have
#     measured a worldgen build without the `roundsd` the setting exists for.
#   * `ci/handoff.sh` refuses a dirty working tree outright — "an agent may
#     still be live". Had the rebase somehow succeeded, every lane gate would
#     have been refused one line later, for the same reason.
#
# THE RULE: a resource cap must not be carried in a tracked file. If a future
# cap genuinely cannot be an environment variable, write an **untracked** file
# and prove `git status --porcelain --untracked-files=no` stays empty — which
# is now asserted below, per lane, before any branch is admitted.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# --- MUST NOT RUN AGAINST THE CHAIN'S OWN CHECKOUT. 2026-08-17 outage. -------
#
# `lanes-dirty-tree` fixed a lane leaving ITS OWN worktree dirty
# (`lane_setup` writing `.cargo/config.toml` before the rsync). That landed at
# 09:32, driven by a real `ci/lanes.sh` run — and by 09:33 the single-lane
# chain (`gate-chain.sh`, via `chain-keeper.sh`) was refusing every branch in
# the queue on `ci/handoff.sh`'s `working tree dirty — an agent may still be
# live`, because `/home/sayer/grit/repo` — the checkout `gate-chain.sh` holds a
# branch in for the whole of a gate — was itself left with 11 staged changes.
# 861 refusals over ~25 minutes, `rc=2` throughout (infrastructure, not a
# verdict, so nothing was lost — but nothing merged either) until a human
# reset the checkout by hand. Old bug, new blast radius: a dirty SHARED
# checkout instead of a dirty LANE worktree.
#
# This file's own writes to `$ROOT` are supposed to be read-only or
# ref-only — `git -C "$ROOT" fetch`, `rev-parse`, `git clone --shared`, and a
# push that targets `refs/lanes/*`, never `refs/heads/*` — and none of them
# should touch a working tree. But `$ROOT` is wherever this script's own
# `$0` resolves to, and nothing before this line has ever refused it being
# the one checkout on the box that a DIFFERENT process — `gate-chain.sh`,
# through `ci/handoff.sh` — repeatedly `git checkout`s a branch in and merges
# `main` in, in place. Any of this file's clones, fetches or ref pushes racing
# that checkout-in-place is a hazard by construction, regardless of which
# exact sequence produced the 11 staged files on 2026-08-17: the fix is to
# make the collision impossible rather than to re-prove which interleaving
# caused it.
#
# So: refuse outright rather than share the chain's checkout. Of the two
# remedies the area brief allows — give the merge step its own checkout, or
# restore the shared one to exactly the state it found — this is the former,
# taken to its root: `ci/lanes.sh` never touches `/home/sayer/grit/repo` at
# all, so there is nothing to restore. `lane_setup` already clones `$ROOT`
# with `--shared` for every lane; the fix is one clone earlier, of `$ROOT`
# itself, by whoever launches this file. `RUSTMC_CANONICAL_CHECKOUT` is
# overridable so `ci/lanes-falsify.sh` can point it at an isolated scratch
# path instead of the real one — same isolation pattern as `REFCACHE_DIR` in
# `ci/reference-cache-falsify.sh` and the private lock namespace in
# `ci/corpus-lock-falsify.sh`.
#
# `-ef` compares device and inode, matching `ci/handoff.sh`'s own
# `release_worktree` check, so a symlink or a relative `..` cannot walk around
# this by looking like a different path while resolving to the same tree.
RUSTMC_CANONICAL_CHECKOUT="${RUSTMC_CANONICAL_CHECKOUT:-/home/sayer/grit/repo}"
if [ -d "$RUSTMC_CANONICAL_CHECKOUT" ] && [ "$ROOT" -ef "$RUSTMC_CANONICAL_CHECKOUT" ]; then
    echo "REFUSED  ROOT=$ROOT is the chain's canonical checkout, not a lanes tree" >&2
    echo "         gate-chain.sh holds a branch checked out there for the whole of" >&2
    echo "         a gate; a lane clone, fetch or push racing that checkout in" >&2
    echo "         place is how 2026-08-17's 861-refusal outage happened. Run" >&2
    echo "         ci/lanes.sh from your own worktree instead." >&2
    exit 2
fi

LANES="${RUSTMC_LANES:-2}"
LANE_DIR="${RUSTMC_LANE_DIR:-$(dirname "$ROOT")/lanes}"
LANE_REMOTE_PREFIX="${RUSTMC_LANE_REMOTE_PREFIX:-rustmc-lane}"
# The gate command each lane runs. A parameter so `ci/lanes-falsify.sh` can
# stand in for the box, exactly as `ci/handoff-verdict-selftest.sh` stands in
# for the stages: this script learns what a gate did from its exit status and
# whether it printed GREEN, and a stand-in producing those is indistinguishable
# to it. `--dry-run` is not optional — the merge belongs to this file, under
# the sequencer, and `handoff.sh` merging on its own would reintroduce the race.
LANE_GATE_CMD="${RUSTMC_LANE_GATE_CMD:-./ci/handoff.sh %BRANCH% --dry-run}"

QUEUE_ARGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        -n) LANES="$2"; shift 2 ;;
        --queue) mapfile -t -O "${#QUEUE_ARGS[@]}" QUEUE_ARGS < "$2"; shift 2 ;;
        --) shift; QUEUE_ARGS+=("$@"); break ;;
        -*) echo "lanes: unknown flag $1" >&2; exit 2 ;;
        *) QUEUE_ARGS+=("$1"); shift ;;
    esac
done
[ "${#QUEUE_ARGS[@]}" -gt 0 ] || { echo "usage: ci/lanes.sh [-n N] <branch>... | --queue <file>" >&2; exit 2; }
[ "$LANES" -ge 1 ] 2>/dev/null || { echo "lanes: -n must be a positive integer, got '$LANES'" >&2; exit 2; }

NPROC="$(nproc 2>/dev/null || echo 4)"
LANE_JOBS="${RUSTMC_LANE_JOBS:-$(( NPROC / LANES ))}"
[ "$LANE_JOBS" -ge 1 ] || LANE_JOBS=1

STATE="${RUSTMC_LANE_STATE:-$LANE_DIR/.state}"
mkdir -p "$STATE" "$LANE_DIR"
LOG="${RUSTMC_LANE_LOG:-$LANE_DIR/lanes.log}"

say() { printf '%s  %s\n' "$(date '+%H:%M:%S')" "$*" | tee -a "$LOG"; }

# --- the one mutex over shared state ----------------------------------------
#
# Every read-modify-write of the train, the queue, the epoch and the
# speculative tip goes through here. A lane worker is a subshell, so nothing is
# shared but files, and a file read outside this is a file another lane may be
# halfway through writing.
STATE_LOCK="$STATE/state.lock"
: > "$STATE_LOCK" 2>/dev/null || true
with_state() {
    local rc
    exec 210>"$STATE_LOCK"
    flock -x 210
    "$@"; rc=$?
    flock -u 210
    exec 210>&-
    return $rc
}

# --- the merge sequencer ----------------------------------------------------
#
# Held across [verify origin/main is still my base] -> [push]. That interval is
# seconds, and it is the whole of the push race: without it two lanes can both
# read a matching main and both push, and the loser's push is rejected *after*
# it has been told it merged. With it, exactly one lane is ever inside the
# check-and-push, so the loser fails the check instead and its branch is
# re-queued for a full re-gate rather than silently lost.
MERGE_LOCK="$STATE/merge.lock"
: > "$MERGE_LOCK" 2>/dev/null || true

# --- state files ------------------------------------------------------------
#   queue.txt   branches not yet admitted to the train, in order
#   train.tsv   seq \t branch \t base \t tip \t status \t epoch
#   spec        the tip the next admission chains onto
#   epoch       bumped on every invalidation
#   seq         monotonic train position counter
_init_state() {
    printf '%s\n' "${QUEUE_ARGS[@]}" > "$STATE/queue.txt"
    : > "$STATE/train.tsv"
    : > "$STATE/gated"
    : > "$STATE/infra"
    echo 0 > "$STATE/epoch"
    echo 0 > "$STATE/seq"
    git -C "$ROOT" rev-parse origin/main > "$STATE/spec"
}

_read()  { cat "$STATE/$1" 2>/dev/null; }
_write() { printf '%s\n' "$2" > "$STATE/$1"; }

# --- lane checkouts ---------------------------------------------------------
#
# A lane needs its own `main` ref, because train position k's `main` is
# position k-1's gated tip and not `origin/main`. Refs are per-repository, so
# lanes cannot be `git worktree` siblings of one checkout the way agents are —
# they are clones. `--shared` puts the 301 MB object store behind an
# `alternates` pointer instead of copying it N times, so a lane costs kilobytes
# of refs rather than a third of a gigabyte.
#
# THE COST OF `--shared`, STATED: the lane clones depend on objects living in
# $ROOT's store, so a `git gc --prune=now` in $ROOT while lanes are running
# could remove an object a lane still references. `gc.auto=0` is set in each
# lane to stop the lane doing it to itself; a human running gc in the canonical
# checkout during a drain is out of this file's reach and is noted here rather
# than defended against.
lane_path() { echo "$LANE_DIR/lane-$1"; }

# --- is this lane's tree in a state a rebase can run in? --------------------
#
# `--untracked-files=no` is the whole predicate and it is deliberate. What git
# refuses a rebase on is **modified tracked files**; untracked ones do not
# block it, and a lane clone legitimately accumulates them. Widening this to
# plain `--porcelain` would refuse drains over build litter, which is how a
# correct check gets switched off. Empty output means clean.
lane_dirty() { git -C "$1" status --porcelain --untracked-files=no 2>/dev/null; }

lane_setup() {
    local i="$1" L d; L="$(lane_path "$i")"
    if [ ! -d "$L/.git" ]; then
        git clone --shared -q "$ROOT" "$L" 2>>"$LOG" || return 1
        git -C "$L" remote set-url origin "$(git -C "$ROOT" remote get-url origin)" || return 1
    fi
    git -C "$L" config gc.auto 0
    git -C "$L" config user.name  "$(git -C "$ROOT" config user.name  || echo lanes)"
    git -C "$L" config user.email "$(git -C "$ROOT" config user.email || echo lanes@local)"
    # A CLONE DOES NOT INHERIT LOCAL CONFIG, and the custom merge drivers live
    # in exactly that. `.gitattributes` naming a driver git has not been told
    # about falls back to the default 3-way merge with no warning — so until
    # 2026-08-16 every lane rebased with `docs/DETERMINISM.md merge=appendonly`
    # inert, and the doc union below was silently doing that driver's job as
    # well as its own. Registering it here is what lets UNIONABLE shrink.
    if [ -x "$L/ci/setup-merge-drivers.sh" ]; then
        ( cd "$L" && ./ci/setup-merge-drivers.sh ) >>"$LOG" 2>&1 || return 1
    fi
    # Per-lane cargo parallelism travels as `CARGO_BUILD_JOBS`, exported where
    # this lane invokes `$LANE_GATE_CMD` (see the `cd "$L" && ... CARGO_BUILD_JOBS=`
    # call below) and spliced onto the remote ssh command by `ci/handoff.sh`'s
    # own `remote()`. It never touches `.cargo/config.toml` — that file is
    # tracked and carries `target-cpu=x86-64-v2`; see "RESOURCE CAPS" above.
    #
    # THIS BLOCK USED TO WRITE `[build]\njobs = N` HERE — THE EXACT BUG THE
    # HEADER ABOVE NARRATES AS FIXED. It was not: the header was rewritten to
    # describe the env-var route and the code below it still took the tracked
    # file. That made every lane self-dirty at setup and refuse before ever
    # admitting a branch — worse than the original defect it silently
    # reintroduced, because it is 100% reproducing rather than one bad drain.
    # Found 2026-08-17 by the one proof this branch was missing: a real branch
    # through a real (non-fixture) lane, private corpus-lock namespace, no
    # merge. `ci/lanes-falsify.sh`'s `dirty_tree_is_infra` case never caught it
    # because its dirt is injected through the stub's own hook
    # (`ci/seed.txt`), which exercises `lane_dirty`'s refusal but never calls
    # this function's real write path — a check testing the guard, not the
    # thing the guard guards. Deleted; nothing here replaces it, and nothing
    # should.
    #
    # THE EARLIEST POINT AT WHICH THE 2026-08-16 DEFECT IS VISIBLE. Nothing
    # above this line writes to the tree any more, so dirt here is either a
    # human's, a crashed run's, or a future edit reintroducing the defect. It is
    # NOT repaired silently: a `reset --hard` here would restore the tree and
    # hide whatever dirtied it, which is the same shape of mistake as writing
    # the file in the first place. Refuse, name the paths, print the repair.
    d="$(lane_dirty "$L")"
    if [ -n "$d" ]; then
        say "lane $i — INFRA: $L has modified tracked files"
        printf '%s\n' "$d" | sed 's/^/             /' | tee -a "$LOG"
        say "lane $i — no rebase can run in this tree, so no branch in it can be judged."
        say "lane $i — repair with:  git -C $L checkout -- ."
        return 3
    fi
    return 0
}

# --- the doc index union, carried over from gate-chain.sh -------------------
#
# Every branch appends a row to the same handful of append-only index tables,
# so **every** rebase collides there and nowhere else — and on a train those
# collisions are worse, not better, because position 3 rebases over positions 1
# and 2 as well as over main. Those collisions are not disagreements: both rows
# belong. They are resolved by keeping BOTH sides, in order, and only for the
# listed files. Anything else conflicting is a real disagreement and the branch
# is dropped for hands, exactly as before.
#
# The safety net for the one thing this can get wrong — two branches claiming
# the same `## N.` section number — is T0's own `ci/check-doc-claims.py`, which
# refuses a duplicate. A bad union fails the gate rather than landing. That is
# also precisely why the retry above cannot be scoped: the defect exists only
# in the union.
#
# **FOUR OF THE FIVE FILES LEFT THIS LIST ON 2026-08-16, AND THE REASON IS NOT
# THAT THE COLLISIONS STOPPED.** They are handled a step earlier now, by merge
# drivers that never see a conflict at all: `merge=appendonly` on
# `docs/DETERMINISM.md` and `merge=docregions` on the other three
# (`.gitattributes`, `ci/merge-docregions.py`). Measured that day over 48
# unmerged branches, that took the branches which will not rebase onto main
# from 26 to 9, and every one of the 9 now conflicts on real code.
#
# Leaving them here as well would be worse than redundant. The union strips
# conflict markers and keeps BOTH sides unconditionally; the drivers refuse
# exactly the cases where both sides rewrote the same text, which is the whole
# value of them. A union running afterwards would override that refusal and
# reinstate the defect — a replaced row surviving beside its replacement — in
# the one place it matters most, on the merge train.
#
# `docs/PARITY.md` stays: it has no driver and no measured collision, so
# removing it would be an unmeasured behaviour change rather than a fix.
UNIONABLE="${RUSTMC_LANE_UNIONABLE:-docs/PARITY.md}"

rebase_with_doc_union() {
    local L="$1" guard=0 conflicted f
    while [ $guard -lt 40 ]; do
        guard=$((guard + 1))
        conflicted=$(git -C "$L" diff --name-only --diff-filter=U)
        [ -z "$conflicted" ] && { git -C "$L" rebase --abort 2>/dev/null; return 1; }
        for f in $conflicted; do
            case " $UNIONABLE " in
                *" $f "*) ;;
                *) git -C "$L" rebase --abort 2>/dev/null; return 1 ;;
            esac
        done
        for f in $conflicted; do
            python3 - "$L/$f" <<'PY' || exit 1
import re, sys
p = sys.argv[1]
s = open(p, encoding='utf-8').read()
s = re.sub(r'^<<<<<<< [^\n]*\n', '', s, flags=re.M)
s = re.sub(r'^=======\n',        '', s, flags=re.M)
s = re.sub(r'^>>>>>>> [^\n]*\n', '', s, flags=re.M)
# Keeping both sides is right for index ROWS and wrong for a renumbered
# HEADING: a section renumbered on the branch unions with its old self and
# lands twice under two numbers. During a rebase HEAD is the upstream side, so
# when two consecutive `## N.` headings carry identical title text the second
# is the branch's own renumber — the one matching its claim file — and the
# first is dropped.
lines, out = s.split('\n'), []
head = re.compile(r'^##\s+\d+\.\s*(.*)$')
for i, ln in enumerate(lines):
    m = head.match(ln)
    if m and i + 1 < len(lines):
        n = head.match(lines[i + 1])
        if n and n.group(1).strip() == m.group(1).strip():
            continue
    out.append(ln)
open(p, 'w', encoding='utf-8').write('\n'.join(out))
PY
            git -C "$L" add "$f"
        done
        # Keeping both sides is only right when both sides ADDED. A branch that
        # EDITS a shared line unions into a block holding the key twice, which
        # is silent corruption of the ledger the whole project quotes from.
        # tomllib is the exact test: a doubled key inside one [[figure]] is a
        # parse error, while legitimate repeats across blocks parse fine.
        if [ -f "$L/ci/figures.toml" ] && ! python3 - "$L/ci/figures.toml" <<'PY'
import sys, collections, tomllib
try: raw = open(sys.argv[1], 'rb').read()
except FileNotFoundError: sys.exit(0)
try: doc = tomllib.loads(raw.decode('utf-8'))
except Exception as e:
    print(f"figures.toml does not parse after union: {e}"); sys.exit(1)
ids = [f.get('id') for f in doc.get('figure', [])]
dup = [i for i, n in collections.Counter(ids).items() if n > 1]
if dup: print(f"figures.toml union corrupt: duplicate figure ids {dup}"); sys.exit(1)
PY
        then
            git -C "$L" rebase --abort 2>/dev/null
            return 1
        fi
        if GIT_EDITOR=true git -C "$L" rebase --continue >>"$LOG" 2>&1; then
            return 0
        fi
    done
    git -C "$L" rebase --abort 2>/dev/null
    return 1
}

# A branch stacked on another branch that has SINCE MERGED is the commonest
# false conflict, and on a train it is the common case rather than the odd one:
# position 3 legitimately carries copies of positions 1 and 2 whenever an agent
# stacked its work. `git cherry` marks a commit `-` when the base already has
# its patch, so the last such commit is the top of the borrowed stack and
# rebasing --onto from there replays only the branch's own work.
rebase_off_borrowed_stack() {
    local L="$1" base="$2" br="$3" cut
    cut=$(git -C "$L" cherry "$base" "$br" 2>/dev/null | awk '$1=="-"{c=$2} END{print c}')
    [ -z "$cut" ] && return 1
    git -C "$L" rebase --onto "$base" "$cut" "$br" >>"$LOG" 2>&1 && return 0
    git -C "$L" rebase --abort 2>/dev/null
    return 1
}

# --- admission: put the next queued branch on the train ---------------------
#
# Runs under the state lock. Assigns the next sequence number and the current
# speculative tip as this branch's base, then advances the speculative tip to
# whatever the rebase produces, so the next admission chains onto this one.
#
# A rebase that conflicts is NOT admitted and does NOT advance the tip: the
# branch is reported and dropped, exactly as `gate-chain.sh` does, because a
# conflict needs judgement and this script deliberately has none.
#
# An empty queue is NOT the end of the drain, and reading it as one loses
# branches. A lane still inside a 38-minute corpus run can go RED and re-queue
# every speculative successor behind it; a lane that had already exited on
# "queue empty" is not there to pick them up, and with N=2 that is half the
# capacity gone at exactly the moment the queue refilled. So: exit 1 means
# empty AND nothing in flight that could refill it, exit 2 means empty for now.
#
# THE REBASE HAPPENS IN HERE, UNDER THE STATE LOCK, AND THAT IS THE WHOLE
# CORRECTNESS ARGUMENT FOR THE TRAIN.
#
# The first version of this file assigned the base at admission and advanced
# the speculative tip later, after the rebase. Two lanes then admitted in the
# same instant, both read the same `spec`, and **both gated against
# origin/main** — which is the naive two-lane design this file's header
# explains is worth zero: the second lane's 54 minutes are void the moment the
# first merges. The drain log said `admitted br2 ... on base 6483ad70` where
# 6483ad70 was origin/main and not br1's tip, and the sweep caught it as two of
# four branches reaching main.
#
# So base assignment and tip advancement must be one atomic step, and the tip
# is not known until the rebase has run. A rebase is seconds against a
# 38-minute corpus run, so serialising it costs nothing measurable and buys the
# invariant that train position k's base is exactly position k-1's gated tip.
#
# The rebased tip is pushed into $ROOT's object store as `refs/lanes/<seq>`
# because a lane clone can only see objects that exist in its own store or in
# the shared one it points at through `alternates`. Lane 2's base is a commit
# lane 1 created inside lane 1's clone; without this it is simply not there,
# and the drain reported `base f83373d2 unreachable in lane`.
_admit() {
    local i="$1" L br base seq ep tip d
    L="$(lane_path "$i")"
    # A DIRTY TREE IS AN INFRASTRUCTURE FAULT, NOT A BRANCH VERDICT — and it is
    # checked HERE, before the queue is touched, so the branch is not consumed
    # by a lane that cannot judge it. Return 3 stops the lane.
    #
    # This is the second of the two guards and it is not redundant with
    # `lane_setup`'s. That one runs once, before any worker starts; this one
    # runs once per admission, so dirt that appears mid-drain — a killed gate, a
    # rebase that left remnants — is caught on the FIRST branch after it appears
    # rather than shredding every branch behind it.
    d="$(lane_dirty "$L")"
    if [ -n "$d" ]; then
        printf 'INFRA\t%s\t%s\n' "$L" "$(printf '%s' "$d" | tr '\n' ';')"
        return 3
    fi
    br="$(head -1 "$STATE/queue.txt")"
    if [ -z "$br" ]; then
        awk -F'\t' '$5=="admitted" || $5=="gating"{f=1} END{exit !f}' "$STATE/train.tsv" \
            && return 2
        return 1
    fi
    sed -i '1d' "$STATE/queue.txt"
    base="$(_read spec)"
    seq=$(( $(_read seq) + 1 )); _write seq "$seq"
    ep="$(_read epoch)"

    _drop() {
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$seq" "$br" "$base" "-" "dropped" "$ep" >> "$STATE/train.tsv"
        printf 'DROP\t%s\t%s\n' "$br" "$1"
        return 0
    }

    git -C "$L" fetch -q "$ROOT" '+refs/lanes/*:refs/lanes/*' 2>>"$LOG"
    git -C "$L" fetch -q origin "+refs/heads/$br:refs/remotes/origin/$br" 2>>"$LOG" \
        || { _drop "no such branch on origin"; return 0; }
    git -C "$L" checkout -q --detach "$base" 2>>"$LOG" \
        || { _drop "base ${base:0:8} unreachable in lane"; return 0; }
    git -C "$L" branch -f main "$base" 2>>"$LOG"
    git -C "$L" checkout -q -B "$br" "origin/$br" 2>>"$LOG" \
        || { _drop "could not check out origin/$br"; return 0; }
    if git -C "$L" merge-base --is-ancestor "$br" "$base" 2>/dev/null; then
        _drop "already contained in the base"; return 0
    fi
    if git -C "$L" rebase "$base" >>"$LOG" 2>&1 \
       || rebase_with_doc_union "$L" \
       || rebase_off_borrowed_stack "$L" "$base" "$br"; then
        :
    else
        git -C "$L" rebase --abort 2>/dev/null
        # CLASSIFY THE FAILURE BEFORE NAMING IT. A rebase that could not START
        # because the tree was dirty is not a statement about this branch, and
        # calling it `REBASE CONFLICT, needs hands` sent a human to inspect 26
        # branches that were fine on 2026-08-16. `rebase --abort` restores a
        # tree that a real conflict left mid-rebase; it cannot clean a tree that
        # was dirty before the rebase declined to start. So dirt surviving to
        # here means the rebase never ran, the branch is UNJUDGED, and it goes
        # back on the queue for a lane that can judge it.
        d="$(lane_dirty "$L")"
        if [ -n "$d" ]; then
            printf '%s\n' "$br" | cat - "$STATE/queue.txt" > "$STATE/queue.tmp" \
                && mv "$STATE/queue.tmp" "$STATE/queue.txt"
            git -C "$L" checkout -q --detach
            printf 'INFRA\t%s\t%s\n' "$L" "$(printf '%s' "$d" | tr '\n' ';')"
            return 3
        fi
        git -C "$L" checkout -q --detach
        _drop "REBASE CONFLICT onto ${base:0:8}, needs hands"; return 0
    fi
    tip="$(git -C "$L" rev-parse HEAD)"
    # Make the tip visible to every other lane BEFORE it becomes the
    # speculative base, or the next admission chains onto a commit it cannot
    # resolve. Failing this is fatal to the train, not cosmetic.
    git -C "$L" push -q "$ROOT" "+$tip:refs/lanes/$seq" 2>>"$LOG" \
        || { git -C "$L" checkout -q --detach; _drop "could not publish tip ${tip:0:8} to the shared store"; return 0; }

    printf '%s\n' "$tip" > "$STATE/spec"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$seq" "$br" "$base" "$tip" "gating" "$ep" >> "$STATE/train.tsv"
    printf 'OK\t%s\t%s\t%s\t%s\t%s\n' "$seq" "$br" "$base" "$tip" "$ep"
    return 0
}

_train_set() {   # seq field value   (fields: 4=tip 5=status)
    local seq="$1" f="$2" v="$3"
    awk -F'\t' -v OFS='\t' -v s="$seq" -v f="$f" -v v="$v" \
        '$1==s{$f=v} {print}' "$STATE/train.tsv" > "$STATE/train.tmp" \
        && mv "$STATE/train.tmp" "$STATE/train.tsv"
}

# Invalidation. A branch went RED or was voided, so every train position after
# it was gated against a tree that will never exist. Those are re-queued at the
# FRONT, in train order, and the speculative tip is reset to whatever actually
# merged. The epoch bump is what a lane in flight reads to learn its own
# result is void.
_invalidate_after() {
    local seq="$1" ep resurrect
    ep=$(( $(_read epoch) + 1 )); _write epoch "$ep"
    resurrect="$(awk -F'\t' -v s="$seq" '$1>s && $5!="merged"{print $2}' "$STATE/train.tsv")"
    if [ -n "$resurrect" ]; then
        { printf '%s\n' "$resurrect"; cat "$STATE/queue.txt"; } | grep -v '^$' > "$STATE/queue.tmp"
        mv "$STATE/queue.tmp" "$STATE/queue.txt"
    fi
    awk -F'\t' -v OFS='\t' -v s="$seq" '$1>s && $5!="merged"{$5="void"} {print}' \
        "$STATE/train.tsv" > "$STATE/train.tmp" && mv "$STATE/train.tmp" "$STATE/train.tsv"
    # The speculative tip goes back to the last thing that really merged, which
    # is origin/main. Read it from the ref rather than from our own bookkeeping:
    # bookkeeping is what just turned out to be wrong.
    git -C "$ROOT" fetch -q origin 2>>"$LOG"
    git -C "$ROOT" rev-parse origin/main > "$STATE/spec"
    printf '%s' "$resurrect" | grep -c . 2>/dev/null || true
}

# All lower train positions merged? Under the state lock.
_predecessors_done() {
    local seq="$1"
    ! awk -F'\t' -v s="$seq" '$1<s && $5!="merged" && $5!="dropped" && $5!="void"{found=1} END{exit !found}' \
        "$STATE/train.tsv"
}

# --- one lane worker --------------------------------------------------------
lane_worker() {
    local i="$1" L RD adm seq br base ep rc out
    L="$(lane_path "$i")"
    RD="$LANE_REMOTE_PREFIX-$i"

    while :; do
        adm="$(with_state _admit "$i")"; rc=$?
        if [ "$rc" -eq 2 ]; then sleep 5; continue; fi
        # A LANE THAT CANNOT REBASE ANYTHING REFUSES LOUDLY ON THE FIRST BRANCH.
        # It does not walk the queue turning every branch into a verdict it has
        # no evidence for. That is what 2026-08-16 did: 33 seconds, 37 skips,
        # exit 0, and a queue indistinguishable from a healthy empty one.
        if [ "$rc" -eq 3 ]; then
            local ld; ld="$(cut -f2 <<<"$adm")"
            say "lane $i — INFRA: $ld has modified tracked files: $(cut -f3 <<<"$adm")"
            say "lane $i — NOT a verdict about any branch: no rebase can run in this tree, so"
            say "lane $i — nothing in it was measured. This lane STOPS; the queue is left intact."
            say "lane $i — repair with:  git -C $ld checkout -- ."
            printf '%s\t%s\n' "$i" "$ld" >> "$STATE/infra"
            return 3
        fi
        if [ "$rc" -ne 0 ]; then say "lane $i — queue empty and nothing in flight, stopping"; return 0; fi
        if [ "$(cut -f1 <<<"$adm")" = DROP ]; then
            say "lane $i — SKIP $(cut -f2 <<<"$adm"): $(cut -f3 <<<"$adm")"
            continue
        fi
        seq="$(cut -f2 <<<"$adm")"; br="$(cut -f3 <<<"$adm")"
        base="$(cut -f4 <<<"$adm")"; local tip; tip="$(cut -f5 <<<"$adm")"
        ep="$(cut -f6 <<<"$adm")"

        say "lane $i — admitted $br at train position $seq, rebased ${base:0:8} -> ${tip:0:8} (epoch $ep)"

        # THE SPECULATIVE REBASE IS NEVER PUSHED BACK TO `origin/<branch>`, and
        # this is not a simplification — the first version did push it, with
        # `--force-with-lease`, copying what `gate-chain.sh` does. On a train
        # that is corruption of the author's branch and it lands a rejected
        # branch on main. Caught by `ci/lanes-falsify.sh`'s
        # `red_does_not_ride_along`, which is the case that exists for it:
        #
        #   br2's speculative tip contains br1's commit, because br1 is its
        #   base. Force-pushing that tip makes `origin/br2` carry br1's work.
        #   br1 then goes RED and is dropped, br2 is re-admitted on a corrected
        #   base — and `_admit` starts from `origin/br2`, which is no longer
        #   br2. So br2 is re-gated, goes green and merges **carrying the
        #   branch the gate had just rejected**. main ended up with br1's row in
        #   the shared index while br1's own verdict was RED.
        #
        # The gated tip does not need to be on origin. It lives in the shared
        # object store as `refs/lanes/<seq>`, `ci/handoff.sh` gates the local
        # branch ref in this lane's clone, and the author's branch on origin
        # stays exactly what the author pushed.
        say "lane $i — gating $br @ ${tip:0:8} in ~/$RD with jobs=$LANE_JOBS"
        # THE ONE THING train.tsv CANNOT TELL YOU. A branch dropped at admission
        # and a branch dropped by a RED gate both end as `dropped`, so the
        # summary counts cannot separate "the queue was judged" from "the queue
        # was destroyed". This file is the difference, and the exit status at
        # the bottom is read from it.
        printf '%s\n' "$br" >> "$STATE/gated"
        out="$LANE_DIR/gate-$br-$i.out"
        ( cd "$L" && RUSTMC_REMOTE_DIR="$RD" CARGO_BUILD_JOBS="$LANE_JOBS" \
            bash -c "${LANE_GATE_CMD//%BRANCH%/$br}" ) >"$out" 2>&1
        rc=$?

        # --- the verdict ----------------------------------------------------
        if [ "$rc" -ne 0 ] || ! grep -q '^GREEN' "$out"; then
            # EVERY WORD ci/handoff.sh CAN PRINT, and the list is enforced.
            # This alternation carried four of the eight, and the four it
            # lacked included `MERGED` — so a successful merge driven through
            # this path has always been recorded as `no verdict line`, which is
            # the exact string 30 of the 32 REDs in this loop's record arrived
            # as. `ci/check-handoff-terminals.py` reads this line and fails T0
            # if a verdict exists that it cannot name, so the duplication is
            # guarded rather than hoped for.
            local verdict; verdict="$(grep -m1 -E '^(MERGED|ALREADY-MERGED|RED|REFUSED|NOT-FF|INFRA|GREEN|UNPUSHED)' "$out")"
            say "lane $i — $br rc=$rc  ${verdict:-no verdict line}"
            local n
            n="$(with_state _invalidate_after "$seq")"
            with_state _train_set "$seq" 5 dropped
            say "lane $i — $br dropped; invalidated ${n:-0} speculative successor(s) for a full re-gate"
            git -C "$L" checkout -q --detach; continue
        fi

        # --- green: wait for predecessors, then merge under the sequencer ----
        #
        # A poll rather than a condvar, because the thing being waited on is
        # another *process* finishing a 38-minute corpus run and bash has no
        # cross-process condition variable. 5 s costs nothing against that.
        while ! with_state _predecessors_done "$seq"; do
            if [ "$(_read epoch)" != "$ep" ]; then break; fi
            sleep 5
        done

        exec 211>"$MERGE_LOCK"
        flock -x 211
        local cur void=""
        git -C "$L" fetch -q origin '+refs/heads/main:refs/remotes/origin/main' 2>>"$LOG"
        cur="$(git -C "$L" rev-parse origin/main)"
        if [ "$(_read epoch)" != "$ep" ]; then
            void="its train base was invalidated while it gated"
        elif [ "$cur" != "$base" ]; then
            void="origin/main moved to ${cur:0:8} from the gated base ${base:0:8}"
        fi
        if [ -n "$void" ]; then
            flock -u 211; exec 211>&-
            say "lane $i — VOID $br: $void — re-queued for a FULL re-gate, not merged"
            with_state bash -c "printf '%s\n' '$br' | cat - '$STATE/queue.txt' > '$STATE/queue.tmp' && mv '$STATE/queue.tmp' '$STATE/queue.txt'"
            with_state _train_set "$seq" 5 void
            # A void caused by main moving from OUTSIDE this drain also voids
            # everything chained onto this branch's tip, and unlike the epoch
            # case nobody has invalidated them yet. A void caused by our own
            # epoch bump needs no second invalidation — that bump is what told
            # us, and repeating it would re-queue the same branches twice.
            [ "$(_read epoch)" = "$ep" ] && with_state _invalidate_after "$seq" >/dev/null
            git -C "$L" checkout -q --detach; continue
        fi
        # `main` in the lane is the base; the gated tip is a descendant of it by
        # construction, so this is a fast-forward and `--ff-only` proves it.
        if git -C "$L" push -q origin "$tip:refs/heads/main" 2>>"$LOG"; then
            with_state _train_set "$seq" 5 merged
            # `spec` is NOT touched here. It is owned by admission and by
            # invalidation, and writing it back at merge time would drag the
            # speculative tip BACKWARDS whenever a later position had already
            # advanced it — breaking the chain for every branch behind us.
            say "lane $i — MERGED $br -> main @ ${tip:0:8} (train position $seq)"
            flock -u 211; exec 211>&-
        else
            flock -u 211; exec 211>&-
            say "lane $i — VOID $br: push to main rejected after a green gate — re-queued for a FULL re-gate"
            with_state bash -c "printf '%s\n' '$br' | cat - '$STATE/queue.txt' > '$STATE/queue.tmp' && mv '$STATE/queue.tmp' '$STATE/queue.txt'"
            with_state _train_set "$seq" 5 void
            with_state _invalidate_after "$seq" >/dev/null
        fi
        git -C "$L" checkout -q --detach
    done
}

# --- run --------------------------------------------------------------------
#
# ONE DRAIN AT A TIME, for the reason `gate-chain.sh` learned on 2026-08-13: a
# second drain started while the first was alive raced it for the same box
# directories and spent the whole queue on refusals inside fifty seconds. A
# flock has neither of `pgrep`'s two problems — matching its own command line,
# and being blind to the gap between one gate ending and the next starting.
DRAIN_LOCK="${RUSTMC_LANE_DRAIN_LOCK:-$LANE_DIR/.drain.lock}"
exec 209>"$DRAIN_LOCK"
if ! flock -x -n 209; then
    say "=== another drain holds the lanes; waiting for it to finish ==="
    flock -x 209
fi

git -C "$ROOT" fetch -q origin 2>>"$LOG"
with_state _init_state
say "=== drain start: $LANES lane(s), ${#QUEUE_ARGS[@]} branch(es), jobs=$LANE_JOBS/lane on $NPROC threads ==="
say "=== base origin/main $(git -C "$ROOT" rev-parse --short origin/main) ==="

for i in $(seq 1 "$LANES"); do
    lane_setup "$i"; s=$?
    [ "$s" -eq 0 ] && continue
    [ "$s" -eq 3 ] && { say "=== REFUSED before any branch was touched: lane $i cannot rebase ==="; exit 4; }
    say "lane $i — could not prepare $(lane_path "$i")"; exit 2
done

pids=()
for i in $(seq 1 "$LANES"); do
    lane_worker "$i" & pids+=("$!")
done
for p in "${pids[@]}"; do wait "$p"; done

say "=== drain done ==="
awk -F'\t' '{c[$5]++} END{for (k in c) printf "  %-10s %d\n", k, c[k]}' "$STATE/train.tsv" | tee -a "$LOG"
say "=== main is $(git -C "$ROOT" fetch -q origin; git -C "$ROOT" rev-parse --short origin/main) ==="

# --- THE EXIT STATUS ---------------------------------------------------------
#
# THE PROBLEM, MEASURED 2026-08-16: this script drained 37 branches to zero
# merges in 33 seconds and exited **0**. A supervisor reading only the status
# cannot tell that from a healthy drain of an empty queue, which is exactly why
# the fallback that should have caught it never fired. A fast clean exit was
# indistinguishable from a no-op.
#
# So the status now answers the question a supervisor actually has — "was this
# queue judged?" — and not "did the loop finish", which it always does:
#
#   0  every lane ran, and at least one branch reached a gate. Branches may
#      still have been dropped or gone red; those ARE verdicts, and a drain
#      that judged its queue and merged nothing is a legitimate 0.
#   2  bad usage, or a lane that could not be prepared at all.
#   3  branches were queued and NOT ONE reached a gate. A destroyed queue.
#   4  a lane stopped on a tree no rebase could run in. The specific, actionable
#      case of 3, and it is checked first because it names the repair. Matches
#      `ci/handoff.sh`'s INFRA exit for the same reason: neither is a verdict.
#
# `$STATE/gated` is the only witness that a gate was reached — see the comment
# where it is written. Counting `merged` rows would be wrong: a queue of
# genuinely red branches merges nothing and is not a failure of this script.
GATED="$(grep -c . "$STATE/gated" 2>/dev/null || true)";  GATED="${GATED:-0}"
INFRA_N="$(grep -c . "$STATE/infra" 2>/dev/null || true)"; INFRA_N="${INFRA_N:-0}"
QUEUED="${#QUEUE_ARGS[@]}"
say "=== $GATED of $QUEUED queued branch(es) reached a gate ==="

if [ "$INFRA_N" -gt 0 ]; then
    say "=== INFRA: $INFRA_N lane(s) stopped on a tree no rebase could run in ==="
    say "===        The branches they did not reach are UNJUDGED, not rejected. ==="
    sed 's/^/           lane /' "$STATE/infra" | tee -a "$LOG"
    exit 4
fi
if [ "$QUEUED" -gt 0 ] && [ "$GATED" -eq 0 ]; then
    say "=== REFUSED: $QUEUED branch(es) queued and not one of them reached a gate ==="
    say "===          Draining a queue to zero gates is a destroyed queue, not a  ==="
    say "===          drained one. Every skip reason is in $LOG.                  ==="
    exit 3
fi
exit 0
