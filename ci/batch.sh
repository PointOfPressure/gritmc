#!/usr/bin/env bash
# ci/batch.sh — land N green branches for ONE gate.
#
# ===========================================================================
# WHAT THIS IS, AND WHAT IT IS NOT
# ===========================================================================
#
# `ci/handoff.sh` is the only route to main and it costs ~80 minutes. With 37
# branches queued and all 37 GREEN in `preflight.tsv`, one gate per branch is
# ~45 hours of gate. This script makes several branches share one gate.
#
# **It is not `ci/lanes.sh`.** That distinction cost a wrong claim once, so it
# is written here rather than left to be re-derived. `ci/lanes.sh` is a merge
# TRAIN: it runs LANES gates concurrently and bases each train position on its
# predecessor's *gated* tip —
#
#     position 1: base = origin/main    position 2: base = position 1's gated tip
#
# — so every queued branch still gets its own full gate. That is pipelining,
# its ceiling is the pipelining ceiling (~1.40x measured; 1.46x at N=2 and
# 1.72x at N=3 with the reference cache on, from the 21.3/21.6 min in-lock
# split in `docs/CORPUS-LOCK-HOLD.md`), and it is already counted.
#
# BATCHING is the other operation: N branches cherry-picked onto ONE throwaway
# integration tip, ONE gate over that tip, and one fast-forward that lands all
# N. Nothing else in this repo does it.
#
# ===========================================================================
# THE WIDTH IS 3, AND IT IS COMPUTED, NOT CHOSEN
# ===========================================================================
#
# A batch lands NOTHING unless EVERY member is green. At the MEASURED
# green-at-verdict rate p=0.7911 (N=158, `ci/gate-rate.py`) a 13-wide batch
# passes 4.75% of the time, so it pays for the batch gate and then pays for all
# 13 anyway:
#
#     width   gates per landed branch      P(batch passes)
#         1   1.2641  (serial baseline)             0.7911
#         2   1.1050                                0.6258
#         3   1.0596  <- best, 1.193x serial        0.4951
#         4   1.0850                                0.3917
#        13   1.3012  <- WORSE THAN SERIAL          0.0475
#
# `ci/batch-arith.py` computes that exactly, by enumerating all 2**w patterns,
# and `--selftest` is its red-when-broken evidence. The collision-feasible
# width and the throughput-optimal width are different numbers, and only the
# second one is a plan.
#
# **AND p IS NOT A CONSTANT — CHECK IT BEFORE USING THIS AT ALL.**
# `ci/batch-arith.py --break-even`: below p=0.6934 NO width beats serial.
# `ci/gate-rate.py --since` on this box, 2026-08-17: all history 0.7911 (N=158),
# since 08-12 0.5890 (N=73), since 08-14 0.5000 (N=56), since 08-17 0.9167
# (N=12). The trailing window is on the WRONG SIDE of the break-even and the
# post-preflight window has too small an N to be a rate. §2.1 of
# `docs/GATE-BATCHING.md` has the decision rule. **The lever is p, not width.**
#
# ===========================================================================
# AND EVERY NUMBER ABOVE IS IN GATES, WHICH IS THE WRONG UNIT — 2026-08-19
# ===========================================================================
#
# The table above counts gates, so it prices a RED and a GREEN the same.
# Measured, they are not: `ci/gate-rate.py --cost` reports 65.2 minutes for a
# gate that comes out GREEN and **3.4** for one that comes out RED, because a
# red gate stops at the stage that failed and a green one runs every stage. A
# failed batch is therefore nearly free, and that is the whole argument for a
# wide one — an argument the gate-counted table cannot state.
#
# Priced at 65.2/3.4, on the real 60-branch GREEN queue of 2026-08-19 planned
# by `--plan --width 8 --on-red halves` into groups 5,6,5,5,8,4,4,4 with 19
# collisions:
#
#                counting gates      priced 65.2/3.4
#     singles        1.269x              1.446x
#     bisect         1.229x              1.725x
#
# **The winner inverts with the unit.** Counting gates says width 3 singles;
# pricing minutes says width 8 halves, and the 1.725x quoted in
# `docs/ASSESSMENT-2026-08-19.md` is bisect's number at width 8, not the
# shipped default's. `ci/batch-arith.py --groups 5,6,5,5,8,4,4,4
# --collisions 19 --cost-green 65.2 --cost-red 3.4` prints both, and its
# selftest C6-C9 pins the inversion. Two defects had to be repaired before it
# would print anything at all: `--groups` was silently dropped whenever costs
# were supplied, and `observed()` could only score `batch_singles`.
#
# The mean admitted group is 5.12 against a requested 8, and the whole gap
# between 2.221x (nominal width 8) and 1.725x (this queue) is collision
# structure: 19 of 60 members would not cherry-pick onto their group's tip.
#
# The break-even moves with the unit too. `--break-even` reports p=0.6934 in
# gates and P_FLOOR below is 0.70 from that. Swept in minutes at 65.2/3.4 it
# falls between p=0.20 and p=0.25. **P_FLOOR is therefore set in the wrong
# unit and is roughly three times too strict.** It is deliberately NOT changed:
# it guards the owner's `--land` switch, the rate it is compared against
# cannot currently be measured at all, and loosening an enable gate on an
# unmeasurable input is the wrong order to do these in. Decide it with a rate
# in hand.
#
# ===========================================================================
# THE FAILURE PATH IS THE DESIGN
# ===========================================================================
#
# At p=0.7911 a 3-wide batch fails 50% of the time, so what happens on RED is not
# an afterthought. On RED this script does NOT know which member is guilty, and
# it does not guess:
#
#   `--on-red singles`  (DEFAULT) the batch dissolves back into the serial
#                       queue: every member is re-gated on its own. Costs at
#                       most 1+w gates and is the cheapest sound strategy at
#                       p=0.7911 — 1.0596 per landed branch at w=3.
#   `--on-red halves`   split into two halves and recurse. Cheaper only above
#                       p~0.94; at 0.7911 it is worse at EVERY width above 2
#                       (1.1621 vs 1.0596 at w=3), because a half is itself
#                       only all-green 25-50% of the time so the interior gate
#                       is rarely repaid. Kept because p is not a constant.
#   `--on-red none`     report and stop. For measuring a batch, not draining.
#
# **INTERACTION REDS ARE REPORTED, NOT BLAMED.** If the batch is RED and every
# member then gates GREEN alone, the members are red only in combination and
# NONE of them is landed. The tempting shortcut — after a RED batch, if the
# first w-1 members gate GREEN, condemn the last without gating it — saves
# 0.0551 gates per landed branch and is FALSE in exactly this case. It would be
# a verdict no measurement of that branch produced, which is this project's
# central defect class (AGENT-BRIEF, HARD RULE). `ci/batch-arith.py` prices it
# as `[unsound]` so the cost of refusing it is visible rather than assumed.
#
# A member whose commits will not cherry-pick onto the integration tip is
# EXCLUDED as a COLLISION and is **not** a RED. Collision and verdict are
# different findings with different fixes — a collision wants a merge driver,
# a red wants a code change — and conflating them would report a clean branch
# as broken.
#
# ===========================================================================
# THE SHARED CHECKOUT IS NEVER TOUCHED. THIS IS THE KNOWN FAILURE.
# ===========================================================================
#
# On 2026-08-17 a lane merged through `/home/sayer/grit/repo` while a chain held
# a branch there. `ci/handoff.sh:418` refuses on `git status --porcelain`, and
# the result was 25 minutes lost and **861 consecutive gate refusals**.
# `ci/lanes.sh` now refuses outright if its `$ROOT` resolves to that path. This
# script holds itself to the same rule, three ways:
#
#   1. it REFUSES to run if its own `$ROOT` is the canonical checkout;
#   2. all integration work happens in a dedicated worktree, which it also
#      refuses to place on `$ROOT` or on the canonical checkout;
#   3. it records the canonical checkout's HEAD and porcelain count at entry
#      and RE-CHECKS BOTH on every exit path, printing `SHARED-CHECKOUT` with
#      the verdict. An assertion that only runs on the happy path is not an
#      assertion.
#
# ===========================================================================
# WHY LANDING IS OFF BY DEFAULT, AND WHAT WOULD MAKE IT SAFE TO SWITCH ON
# ===========================================================================
#
# **`--land` is the owner's switch, exactly as `ci/lanes.sh` is.** Without it a
# green batch is reported and nothing moves.
#
# The reason is not caution, it is a measured hazard. **No `git fetch` exists
# anywhere in `ci/handoff.sh`, `chain.sh` or `chain-keeper.sh`** (checked
# 2026-08-17). Local `main` in the canonical checkout is advanced by exactly
# one writer — handoff's own `git merge --ff-only` — and handoff computes
#
#     BEHIND=$(git rev-list --count "$BRANCH..main")
#
# against that LOCAL ref. So a second writer of `main` creates two failures
# neither of which names its cause:
#
#   * advance origin/main only, and every later handoff measures its branch
#     against a stale local main, reports BEHIND=0 when the branch really is
#     behind, gates it anyway and then fails `git push` as non-fast-forward
#     AFTER paying a full 80-minute gate;
#   * advance local main under a worktree that has it checked out, and every
#     file of the batch reads as modified there — which is the 861-refusal bug
#     above, reintroduced by another door.
#
# So `land_tip` below takes a repo-wide main-writer mutex, refuses if `main` is
# checked out in ANY worktree, refuses unless local `main` AND `origin/main`
# AND the remote's own `ls-remote` all still stand at the SHA the batch was
# gated from, and advances the local ref with `git update-ref <ref> <new>
# <old>`, which is a compare-and-swap rather than a write. On a failed push it
# rolls the ref back.
#
# **What would make it safe to enable in `chain-keeper.sh`** — re-checked
# 2026-08-19, and three of the four now hold. Kept as a list rather than
# rewritten to "it is ready", because the one still open is the one that
# matters and a reader who sees a tidy list stops reading:
#
#   1. DONE 2026-08-19. `ci/handoff.sh` takes the same mutex around its merge
#      step, keyed on `$(git rev-parse --git-common-dir)/rustmc-main-writer.lock`,
#      and FAILS CLOSED — it refuses with exit 2 rather than merging
#      unprotected if the lock cannot be taken in 300 s. Evidence is
#      `ci/handoff-merge-tail-falsify.sh` case 13, which holds the lock from
#      another process and asserts the refusal, that the refusal names the
#      mutex, and that nothing is merged; its control requires MERGED on a free
#      lock, so a handoff that refused unconditionally would not pass.
#   2. DONE, AND THIS BLOCK WAS STALE ABOUT IT. The paragraph above still says
#      "No `git fetch` exists anywhere in `ci/handoff.sh`, `chain.sh` or
#      `chain-keeper.sh` (checked 2026-08-17)". One has been added to
#      `ci/handoff.sh` since, and it is followed by explicit handling of the
#      three ways local `main` and `origin/main` can differ — behind, ahead,
#      and genuinely diverged — with the behind case, which is the one this
#      precondition was written about, caught at entry before any stage runs.
#      `ci/handoff-merge-tail-falsify.sh` case 12 is its evidence.
#   3. STILL OPEN, AND IT IS THE OWNER'S. One measured live batch at width 2
#      with `--land`, on two branches whose loss would cost nothing, before any
#      wider run. Nothing an agent should do: `--land` advances `main`.
#   4. DONE. `ci/batch-falsify.sh` green — 54 passed, 0 failed, 0 injections
#      not caught, measured 2026-08-19 on sayer-desktop. (This line said "24
#      cases and 9 injections"; the suite has grown since.)
#
# **AND THE RATE CRITERION IS SEPARATELY NOT MET, FOR A REASON THAT IS NOT
# ABOUT BATCHING — 2026-08-19.** `ci/gate-rate.py --decide` answers "not met
# under any definition", because `~/rustmc-private/gates` holds 28 logs, all
# stamped 2026-08-19, oldest 01:05 — 11 of them reach a verdict, giving
# p = 4/11 = 0.3636 against a MIN_N of 20. The p = 0.8217 at N = 157 that
# `docs/ASSESSMENT-2026-08-19.md` quotes cannot be reproduced on this box
# because the logs it was measured from are gone. That is a fact about the
# evidence store, not about the gate, and re-accumulating it costs nothing but
# calendar.
#
# ===========================================================================
# USAGE
# ===========================================================================
#
#   ci/batch.sh [options] <branch>...
#
#   --width N          members per batch (default 3, from ci/batch-arith.py)
#   --pack             group by DISJOINT FILE SETS rather than queue order.
#                      Measured worth: 8 of 36 live branches collide in queue
#                      order at width 3, because branches touching one area
#                      arrive together and so are sliced into one group.
#   --from-preflight [FILE]
#                      take every GREEN row of preflight.tsv as a member
#   --on-red MODE      singles (default) | halves | none
#   --land             on GREEN, fast-forward main. Default: report only.
#                      REFUSED if the measured green-at-verdict rate says
#                      batching at this width is slower than gating serially.
#   --accept-rate      land anyway despite that (or despite an unmeasurable
#                      rate). Says so in the log.
#   --rate-since DATE  YYYYMMDD window for the rate measurement
#   --sweep            before gating, take ONE recording over the integration
#                      tip so every member's new gameplay scenarios get ledger
#                      entries from one ~40-minute corpus-lock acquisition
#                      instead of one each. Runs as role=run, so a production
#                      merge gate still jumps ahead. See ci/sweep-tip.sh.
#   --sweep-back       and graft each member's own rows back onto its own
#                      branch, so a RED batch does not waste the recording.
#                      Opt-in: the ledger has no merge driver, so N branches
#                      appending to it will conflict as they rebase in turn.
#   --plan             cherry-pick feasibility and ci/handoff.sh's own state
#                      checks only; run no gate at all. Seconds, not 80 minutes.
#   --base REF         default `main`, which must equal origin/main
#   --max-gates N      stop after N gates (default 0 = no cap)
#   --int-dir DIR      the throwaway integration worktree
#   --keep-int         do not remove the integration worktree at the end
#
#   RUSTMC_BATCH_GATE_CMD    default `./ci/handoff.sh %BRANCH% --dry-run`
#   RUSTMC_BATCH_REMOTE_DIR  default `rustmc-batch`; must not be a dir another
#                            live handoff owns, which is checked by reading
#                            RUSTMC_REMOTE_DIR out of /proc/<pid>/environ
#                            rather than by matching process names
#   RUSTMC_CANONICAL         default /home/sayer/grit/repo
#
# Exit: 0 every member adjudicated | 1 at least one member RED | 2 refused
#       (state) | 3 land refused (lease lost or main held) | 4 infra: a gate
#       could not run and the members are unjudged
#
# Falsified by `ci/batch-falsify.sh`.
# ===========================================================================
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

CANONICAL="${RUSTMC_CANONICAL:-/home/sayer/grit/repo}"
GATE_CMD="${RUSTMC_BATCH_GATE_CMD:-./ci/handoff.sh %BRANCH% --dry-run}"
REMOTE_DIR="${RUSTMC_BATCH_REMOTE_DIR:-rustmc-batch}"
WIDTH="${RUSTMC_BATCH_WIDTH:-3}"
ON_RED="singles"
LAND=""
PLAN=""
BASE_REF="main"
MAX_GATES=0
INT_DIR="${RUSTMC_BATCH_INT_DIR:-$(dirname "$ROOT")/batch-int}"
KEEP_INT=""
PACK=""
SWEEP=""
SWEEP_BACK=""
ACCEPT_RATE=""
RATE_SINCE=""
PREFLIGHT=""
MEMBERS=()

while [ $# -gt 0 ]; do
    case "$1" in
        --width)      WIDTH="${2:-}"; shift 2 ;;
        --on-red)     ON_RED="${2:-}"; shift 2 ;;
        --land)       LAND=1; shift ;;
        --plan)       PLAN=1; shift ;;
        --base)       BASE_REF="${2:-}"; shift 2 ;;
        --max-gates)  MAX_GATES="${2:-}"; shift 2 ;;
        --int-dir)    INT_DIR="${2:-}"; shift 2 ;;
        --keep-int)   KEEP_INT=1; shift ;;
        --pack)       PACK=1; shift ;;
        --sweep)      SWEEP=1; shift ;;
        --sweep-back) SWEEP=1; SWEEP_BACK=1; shift ;;
        --accept-rate) ACCEPT_RATE=1; shift ;;
        --rate-since) RATE_SINCE="${2:-}"; shift 2 ;;
        --from-preflight)
                      if [ $# -ge 2 ] && [ "${2#-}" = "$2" ] && [ -n "${2:-}" ]
                      then PREFLIGHT="$2"; shift 2
                      else PREFLIGHT="$(dirname "$ROOT")/preflight.tsv"; shift; fi ;;
        -h|--help)    sed -n '/^# USAGE/,/^# ====/p' "$0"; exit 0 ;;
        -*)           echo "REFUSED  unknown option: $1"; exit 2 ;;
        *)            MEMBERS+=("$1"); shift ;;
    esac
done

say() { printf '%s  %s\n' "$(date '+%H:%M:%S')" "$*"; }

case "$ON_RED" in
    singles|halves|none) ;;
    *) echo "REFUSED  --on-red '$ON_RED' is not singles, halves or none"; exit 2 ;;
esac
case "$WIDTH" in
    ''|*[!0-9]*) echo "REFUSED  --width '$WIDTH' is not a positive integer"; exit 2 ;;
    0)           echo "REFUSED  --width 0 batches nothing"; exit 2 ;;
esac
case "$MAX_GATES" in
    ''|*[!0-9]*) echo "REFUSED  --max-gates '$MAX_GATES' is not a non-negative integer"; exit 2 ;;
esac
# `--from-preflight` reads the GREEN rows of preflight.tsv. Typing 37 branch
# names by hand is not a workflow, and a mistyped one is a SKIP-MISSING that
# looks like a queue that has moved.
if [ -n "$PREFLIGHT" ]; then
    [ -r "$PREFLIGHT" ] || { echo "REFUSED  cannot read $PREFLIGHT"; exit 2; }
    while read -r b v _; do
        [ "$v" = GREEN ] || continue
        MEMBERS+=("$b")
    done < "$PREFLIGHT"
    [ "${#MEMBERS[@]}" -gt 0 ] || {
        echo "REFUSED  $PREFLIGHT has no GREEN rows — nothing to batch"; exit 2; }
    echo "from $PREFLIGHT: ${#MEMBERS[@]} GREEN branches"
fi
[ "${#MEMBERS[@]}" -gt 0 ] || { echo "REFUSED  no branches given"; exit 2; }

# --- GUARD 1: this script may not run in the canonical checkout --------------
# `ci/lanes.sh`'s `lanes-canonical-guard`, held to by name so the two cannot
# drift apart silently. A live gate owns that tree; rebasing in it is the
# 861-refusal bug.
if [ -d "$CANONICAL" ] && [ "$ROOT" -ef "$CANONICAL" ]; then
    echo "REFUSED  \$ROOT resolves to the canonical checkout $CANONICAL"
    echo "         A live gate owns that tree. Make your own worktree:"
    echo "             git -C $CANONICAL worktree add --detach <path> main"
    exit 2
fi

# --- GUARD 2: the shared checkout must be exactly as we found it -------------
# Recorded FIRST, before any other guard, and re-checked on EVERY exit path by
# the trap armed below it. Order matters and the falsifier is what proved it:
# this block used to sit third and the trap was armed near the bottom of the
# file, so `--int-dir $ROOT` refused with no SHARED-CHECKOUT line at all — an
# assertion covering only the exits below itself, which is not the "every exit
# path" the header claims. The 2026-08-17 bug was not that somebody meant to
# dirty that tree; it was that nothing looked.
SHARED_HEAD=""
SHARED_REF=""
SHARED_DIRTY=""
if [ -d "$CANONICAL/.git" ] || [ -f "$CANONICAL/.git" ]; then
    SHARED_HEAD="$(git -C "$CANONICAL" rev-parse HEAD 2>/dev/null)"
    SHARED_REF="$(git -C "$CANONICAL" symbolic-ref -q --short HEAD 2>/dev/null || echo DETACHED)"
    SHARED_DIRTY="$(git -C "$CANONICAL" status --porcelain 2>/dev/null | grep -c . || true)"
fi

RC_FINAL=0
assert_shared_clean() {
    [ -n "$SHARED_HEAD" ] || { echo "SHARED-CHECKOUT  not present at $CANONICAL — nothing to assert"; return 0; }
    local head ref dirty bad=""
    head="$(git -C "$CANONICAL" rev-parse HEAD 2>/dev/null)"
    ref="$(git -C "$CANONICAL" symbolic-ref -q --short HEAD 2>/dev/null || echo DETACHED)"
    dirty="$(git -C "$CANONICAL" status --porcelain 2>/dev/null | grep -c . || true)"
    [ "$head" = "$SHARED_HEAD" ]   || bad="HEAD moved $SHARED_HEAD -> $head"
    [ "$ref" = "$SHARED_REF" ]     || bad="${bad:+$bad; }checkout moved $SHARED_REF -> $ref"
    [ "$dirty" = "$SHARED_DIRTY" ] || bad="${bad:+$bad; }porcelain $SHARED_DIRTY -> $dirty entries"
    if [ -n "$bad" ]; then
        echo "SHARED-CHECKOUT  DIRTIED — $CANONICAL: $bad"
        echo "                 This is the 2026-08-17 failure: ci/handoff.sh:418 refuses on"
        echo "                 a dirty tree, and that cost 861 consecutive gate refusals."
        RC_FINAL=2
        return 1
    fi
    echo "SHARED-CHECKOUT  untouched — $CANONICAL on $SHARED_REF at ${SHARED_HEAD:0:8}, $SHARED_DIRTY porcelain entries"
    return 0
}

# THE TRAP MUST NOT SWALLOW AN EXIT CODE. An earlier version ended
# `trap '...; exit $RC_FINAL' EXIT`, which turned every `exit 2` refusal above
# into an exit 0 — a refusal that reports success, in a script whose whole job
# is refusing. `$?` is captured first and only a zero is replaced.
#
# `int_cleanup` is defined further down. That is fine, because a trap body is
# resolved when it fires, but it is guarded anyway: an early refusal must not
# die on a missing function on its way out.
on_exit() {
    local rc=$?
    declare -F int_cleanup >/dev/null 2>&1 && int_cleanup
    assert_shared_clean || rc=2
    [ "$rc" -eq 0 ] && rc="$RC_FINAL"
    exit "$rc"
}
trap on_exit EXIT

# --- GUARD 3: the integration worktree may not be $ROOT or the canonical ------
if [ -e "$INT_DIR" ] && [ "$INT_DIR" -ef "$ROOT" ]; then
    echo "REFUSED  --int-dir is \$ROOT ($ROOT) — the integration tip would eat this checkout"
    exit 2
fi
if [ -d "$CANONICAL" ] && [ -e "$INT_DIR" ] && [ "$INT_DIR" -ef "$CANONICAL" ]; then
    echo "REFUSED  --int-dir is the canonical checkout $CANONICAL"
    exit 2
fi

# `--untracked-files=no`, following `ci/lanes.sh:308` and for its reason: what
# makes a tree unsafe to work in is MODIFIED TRACKED FILES — an agent mid-edit —
# and a plain `--porcelain` refuses over build litter, logs and an operator's
# own scratch files. Note the deliberate asymmetry with assert_shared_clean
# above, which uses plain `--porcelain` on the CANONICAL checkout: there an
# untracked file is not litter, because `ci/handoff.sh:418` refuses on plain
# `--porcelain`, so one we left behind would stop every gate on the box.
[ -z "$(git status --porcelain --untracked-files=no)" ] || {
    echo "REFUSED  \$ROOT has modified tracked files — commit or stash before batching"
    git status --porcelain --untracked-files=no | head -5 | sed 's/^/             /'
    exit 2; }

# --- GUARD 4: nobody else's gate directory ----------------------------------
# `pkill -f` kills its own caller and the bracket form is not enough when the
# same command line also carries the plain path, so this resolves PIDs with
# pgrep, filters $$, and compares RUSTMC_REMOTE_DIR out of /proc/<pid>/environ
# — which `ci/handoff.sh:192` exports — rather than matching process names.
# Four over-broad matches cost this project real time on 2026-08-17.
remote_dir_conflict() {
    local pid owner
    for pid in $(pgrep -f '[c]i/handoff.sh' 2>/dev/null); do
        [ "$pid" = "$$" ] && continue
        [ -r "/proc/$pid/environ" ] || continue
        owner="$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null \
                 | sed -n 's/^RUSTMC_REMOTE_DIR=//p' | head -1)"
        [ -n "$owner" ] || continue
        if [ "$owner" = "$REMOTE_DIR" ]; then echo "$pid"; return 0; fi
    done
    return 1
}

# ---------------------------------------------------------------------------
# THE BASE
# ---------------------------------------------------------------------------
# `ci/handoff.sh` computes AHEAD/BEHIND against LOCAL `main`, so an integration
# tip built on anything else is either refused by it (exit 3, "not a
# fast-forward") or — worse — measured against a ref nobody will land onto. The
# base must therefore be local `main`, and local `main` must agree with
# `origin/main`, or the whole exercise is scoring a tip that cannot land.
BASE="$(git rev-parse --verify -q "$BASE_REF^{commit}")" || {
    echo "REFUSED  no such base ref: $BASE_REF"; exit 2; }
ORIGIN_MAIN="$(git rev-parse --verify -q 'refs/remotes/origin/main^{commit}' || true)"
if [ -n "$ORIGIN_MAIN" ] && [ "$BASE" != "$ORIGIN_MAIN" ]; then
    echo "REFUSED  base $BASE_REF (${BASE:0:8}) != origin/main (${ORIGIN_MAIN:0:8})"
    echo "         ci/handoff.sh measures BEHIND against local main and there is no"
    echo "         git fetch anywhere in it, chain.sh or chain-keeper.sh. A tip built"
    echo "         on a base that disagrees with origin cannot be landed by either route."
    exit 2
fi

# ---------------------------------------------------------------------------
# MEMBER RESOLUTION
# ---------------------------------------------------------------------------
# `origin/<name>` wins over a local branch of the same name: the chain fetches
# origin, AGENT-BRIEF's most expensive documented mistake is work committed and
# never pushed, and a batch that quietly gated an unpushed local tip would land
# a commit nothing downstream has. When the two differ, both SHAs are printed —
# the unpushed part is NOT in this batch and saying so is the whole point.
declare -A MSHA=()
resolve_member() {
    local name="$1" o l
    o="$(git rev-parse --verify -q "refs/remotes/origin/$name^{commit}" || true)"
    l="$(git rev-parse --verify -q "refs/heads/$name^{commit}" || true)"
    # Notes go to STDERR. This function is called in a $( ) and the first
    # version let a NOTE line be captured into the SHA — a batch would then
    # cherry-pick from a ref named "NOTE ... a1b2c3d4" and fail unexplainably.
    if [ -n "$o" ] && [ -n "$l" ] && [ "$o" != "$l" ]; then
        say "NOTE     $name: origin/${o:0:8} != local/${l:0:8} — batching origin's tip;" \
            "$(git rev-list --count "$o..$l" 2>/dev/null || echo '?') local commit(s) are NOT in this batch" >&2
    fi
    if [ -n "$o" ]; then echo "$o"; elif [ -n "$l" ]; then echo "$l"; else return 1; fi
}

# ---------------------------------------------------------------------------
# THE INTEGRATION WORKTREE
# ---------------------------------------------------------------------------
INT_MADE=""
int_prepare() {
    if [ ! -e "$INT_DIR" ]; then
        git worktree add --detach "$INT_DIR" "$BASE" >/dev/null 2>&1 || {
            echo "REFUSED  could not create the integration worktree at $INT_DIR"; return 2; }
        INT_MADE=1
    fi
    git -C "$INT_DIR" rev-parse --git-dir >/dev/null 2>&1 || {
        echo "REFUSED  $INT_DIR is not a git worktree"; return 2; }
    # `clean -fd` and NOT `-x`: the gitignored data (oracle-work, data/vanilla)
    # is expensive and shared, and this script needs none of it — ci/handoff.sh
    # stage 1 builds the gate directory's own copies on the box from $REFS and
    # $SHARED_DATA. Deleting them here would cost somebody else an extract.
    git -C "$INT_DIR" checkout --detach -q "$BASE" 2>/dev/null
    git -C "$INT_DIR" reset --hard -q "$BASE" || return 2
    git -C "$INT_DIR" clean -fdq || return 2
    # Repo-local config, so one call configures every worktree that shares this
    # .git — but it has to be made from a real checkout, which the rsynced
    # remote build tree is not (see that script's header).
    ( cd "$INT_DIR" && ./ci/setup-merge-drivers.sh ) >/dev/null 2>&1 || true
    return 0
}

# Batch branches are refs in the SHARED repo and would otherwise accumulate
# there one per group forever. Scoped by $$ so a concurrent batch's refs are
# never touched.
int_drop_branches() {
    local b
    for b in $(git for-each-ref --format='%(refname:short)' "refs/heads/batch-*-$$" 2>/dev/null); do
        git branch -D "$b" >/dev/null 2>&1 || true
    done
}

int_cleanup() {
    [ -n "$KEEP_INT" ] && return 0
    if [ -n "$INT_MADE" ]; then
        git -C "$INT_DIR" checkout --detach -q "$BASE" 2>/dev/null
        # `git worktree remove` cleans up its own admin entry. A global
        # `git worktree prune` is deliberately NOT run here: this repository has
        # 212 registered worktrees belonging to other agents, prune is
        # repository-wide with no path filter, and a lingering prunable entry
        # for a directory we deleted ourselves costs nothing. Reaching into
        # everybody else's bookkeeping to tidy our own is the wrong trade.
        git worktree remove --force "$INT_DIR" >/dev/null 2>&1 || rm -rf "$INT_DIR"
    fi
    int_drop_branches
}

# ---------------------------------------------------------------------------
# BUILDING ONE INTEGRATION TIP
# ---------------------------------------------------------------------------
# Cherry-pick, not rebase: rebasing a member would rewrite the member's own
# branch ref, and those refs belong to the agents that pushed them. Cherry-pick
# leaves every member untouched and yields a linear tip, so the land is a true
# fast-forward rather than a merge.
#
# The merge drivers are registered in the integration worktree first, because
# `.gitattributes` naming a driver git does not have falls back to the ordinary
# 3-way merge with no warning — which is exactly the collision the drivers
# exist to remove, reappearing as an unexplained COLLISION exclusion.
ADMITTED=()
EXCLUDED=()
BATCH_BR=""
build_tip() {
    local -a want=("$@")
    ADMITTED=(); EXCLUDED=()
    int_prepare || return 2
    int_drop_branches
    BATCH_BR="batch-$(date '+%Y%m%d-%H%M%S')-w${#want[@]}-$$"
    git -C "$INT_DIR" checkout -q -b "$BATCH_BR" || {
        echo "REFUSED  could not create $BATCH_BR"; return 2; }

    local name sha mb nmerge c
    for name in "${want[@]}"; do
        sha="${MSHA[$name]}"
        mb="$(git merge-base "$BASE" "$sha" 2>/dev/null)"
        if [ -z "$mb" ]; then
            EXCLUDED+=("$name:UNRELATED"); say "  EXCLUDE $name — no merge base with $BASE_REF"
            continue
        fi
        nmerge="$(git rev-list --count --merges "$mb..$sha" 2>/dev/null || echo 0)"
        if [ "$nmerge" -gt 0 ]; then
            # Dropping them with --no-merges would drop content silently, which
            # is worse than refusing the member by name.
            EXCLUDED+=("$name:MERGECOMMITS"); say "  EXCLUDE $name — $nmerge merge commit(s) in its range"
            continue
        fi
        local ok=1
        for c in $(git rev-list --reverse "$mb..$sha"); do
            if ! git -C "$INT_DIR" cherry-pick --allow-empty --keep-redundant-commits "$c" >/dev/null 2>&1; then
                git -C "$INT_DIR" cherry-pick --abort >/dev/null 2>&1
                ok=""
                break
            fi
        done
        if [ -z "$ok" ]; then
            EXCLUDED+=("$name:COLLISION")
            say "  EXCLUDE $name — will not cherry-pick onto this tip (COLLISION, not a verdict)"
            continue
        fi
        ADMITTED+=("$name")
    done

    # THE CHECK THAT MAKES "ALL N LANDED" MEAN ANYTHING.
    # `git cherry` compares by patch-id, so this asks git whether the tip really
    # carries each admitted member's commits rather than trusting that the loop
    # above did what its log says. Without it, a batch that built an empty tip
    # would gate GREEN, land, and report N branches landed — a green that
    # cannot go red, which is the one defect class this repo refuses by name.
    local tip missing
    tip="$(git -C "$INT_DIR" rev-parse HEAD)"
    for name in "${ADMITTED[@]:-}"; do
        [ -n "$name" ] || continue
        missing="$(git cherry "$tip" "${MSHA[$name]}" "$(git merge-base "$BASE" "${MSHA[$name]}")" \
                   2>/dev/null | grep -c '^+' || true)"
        if [ "${missing:-0}" -ne 0 ]; then
            echo "REFUSED  integration tip ${tip:0:8} is missing $missing commit(s) of $name"
            echo "         after cherry-pick reported success — refusing to gate a tip that"
            echo "         does not contain what this batch claims it contains."
            return 2
        fi
    done
    return 0
}

# ---------------------------------------------------------------------------
# THE RATE GUARD — refuse to land when batching would make the queue SLOWER
# ---------------------------------------------------------------------------
# Batching is not a free win, it is a win CONDITIONAL ON p, and the condition is
# not currently satisfied on this box over every window. `ci/batch-arith.py
# --break-even` puts the line at p = 0.6934: below it no width beats serial.
# `ci/gate-rate.py` measures p over the whole gate-log population and gets
# 0.7911 (N=158) all-history but 0.5000 (N=56) over the trailing three days.
#
# **So a batch run that does not look at p is a batch run that might be making
# things worse while reporting a saving.** That is precisely the shape of check
# this repo refuses: a green that cannot go red. This asks, before landing:
#
#   * `ci/gate-rate.py` for p, with its N, over the real logs;
#   * `ci/batch-arith.py --advise` for the verdict at THIS width.
#
# The threshold is not duplicated here. A hardcoded 0.6934 in a shell script is a
# figure that goes stale the first time the model changes, which is the
# lagging-caveat-list failure `docs/AGENT-BRIEF.md` names.
#
# **No rate means no land.** If the logs cannot produce a rate at N >= 20, that
# is not permission — it is the absence of the measurement the decision needs.
# `--accept-rate` is the deliberate override and it says so in the log.
RATE_DIR="${RUSTMC_BATCH_GATE_LOGDIR:-$HOME/rustmc-private/gates}"

rate_guard() {
    [ -n "$LAND" ] || return 0            # nothing is being landed; nothing to guard
    # WIDTH 1 IS SERIAL. There is no batch to judge, so there is nothing for the
    # rate to say: `--advise --width 1` correctly reports verdict=SERIAL because
    # a width-1 batch does not beat gating serially, it IS gating serially, and
    # refusing over that would block the one mode that is unconditionally safe —
    # which is also the mode `--on-red singles` falls back to. The falsifier
    # caught this via case max_gates_budget, which drains at --width 1.
    [ "$WIDTH" -gt 1 ] || { say "RATE     width 1 is serial gating; no rate question to answer"; return 0; }
    local line p n adv verdict
    if [ ! -x ./ci/gate-rate.py ] || [ ! -x ./ci/batch-arith.py ]; then
        say "RATE     ci/gate-rate.py or ci/batch-arith.py missing — cannot check p"
        [ -n "$ACCEPT_RATE" ] && { say "         --accept-rate: landing anyway"; return 0; }
        echo "REFUSED  cannot measure the green-at-verdict rate, so cannot know whether"
        echo "         batching helps. Pass --accept-rate to land regardless."
        return 2
    fi
    line="$(./ci/gate-rate.py --dir "$RATE_DIR" ${RATE_SINCE:+--since "$RATE_SINCE"} 2>/dev/null \
            | sed -n 's|^gate-rate: p = \([0-9]*\)/\([0-9]*\) = \([0-9.]*\).*|\3 \2|p' | head -1)"
    p="${line%% *}"; n="${line##* }"
    if [ -z "$p" ] || [ -z "$n" ]; then
        say "RATE     no gate log in $RATE_DIR reached a stage verdict — p is unknown"
        [ -n "$ACCEPT_RATE" ] && { say "         --accept-rate: landing anyway"; return 0; }
        echo "REFUSED  no measurable green-at-verdict rate in $RATE_DIR."
        echo "         No rate is not permission. --accept-rate overrides."
        return 2
    fi
    adv="$(./ci/batch-arith.py --advise --p "$p" --width "$WIDTH" 2>/dev/null)"
    verdict="${adv##*verdict=}"
    say "RATE     p=$p over N=$n gates in $RATE_DIR"
    say "         $adv"
    if [ "$n" -lt 20 ]; then
        say "RATE     N=$n is below 20 — that is an estimate, not a rate"
        [ -n "$ACCEPT_RATE" ] && { say "         --accept-rate: landing anyway"; return 0; }
        echo "REFUSED  N=$n gates is too few to call p a rate, and p decides whether"
        echo "         batching helps at all. --accept-rate overrides."
        return 2
    fi
    if [ "$verdict" != BATCH ]; then
        [ -n "$ACCEPT_RATE" ] && { say "RATE     --accept-rate: landing anyway despite verdict=$verdict"; return 0; }
        echo "REFUSED  at p=$p, width $WIDTH costs MORE per landed branch than gating"
        echo "         serially. Landing this batch would make the queue slower, not"
        echo "         faster. Below p=0.6934 no width beats serial at all"
        echo "         (ci/batch-arith.py --break-even)."
        echo "         Narrow the batch, wait for p to recover, or --accept-rate."
        return 2
    fi
    return 0
}

# ---------------------------------------------------------------------------
# GROUPING — WHICH branches share a gate, which is not a detail
# ---------------------------------------------------------------------------
# Taking the queue in the order it arrives and slicing it every WIDTH branches
# is the obvious thing and it is measurably not the best one. Measured
# 2026-08-17 over the 36 live GREEN branches at width 3, in preflight order:
# **8 of them, 22%, would not cherry-pick onto their group's tip** — and the
# collisions cluster, because branches that touch one area arrive together and
# are therefore sliced into the same group. A `gameplay-*` run of five lands
# three collisions; the six worldgen branches at the end of the alphabet land
# none.
#
# `--pack` groups by DISJOINT FILE SETS instead: two branches that change no
# file in common cannot conflict, whatever the merge drivers do or do not do.
# That is sufficient rather than necessary — a merge driver can resolve a shared
# file, and `docs/WORKLIST.md`'s index table is designed to — so packing is
# conservative, and its collisions-avoided figure is a floor.
#
# Greedy first-fit, which is not optimal bin packing and does not need to be:
# the groups are capped at 3 and the cost of a suboptimal packing is one extra
# gate, while the cost of an exact solver is a solver.
# `BATCH_GROUPS`, NOT `GROUPS`, AND THIS ONE IS WORTH THE COMMENT. `GROUPS` is a
# bash SPECIAL ARRAY holding the group ids the current user belongs to, and
# "assignments to GROUPS have no effect" — so `GROUPS=()` and `GROUPS+=(...)`
# are silently ignored while `"${GROUPS[@]}"` still expands to the real group
# list. This script duly built its batches, threw them away, and then ran
# `run_group 1000` — batching the operator's numeric group ids as if they were
# branch names. No error, no warning, ten groups where there should have been
# two. Caught by ci/batch-falsify.sh on the first run after the feature landed.
declare -a BATCH_GROUPS=()

# A PATH WITH A WORKING MERGE DRIVER IS NOT A COLLIDING PATH, AND LEAVING THAT
# OUT MAKES --pack USELESS. Measured 2026-08-17: packing on raw disjointness over
# the 36 live branches produced 27 groups — four of width 3, one of 2, and
# TWENTY-TWO SINGLETONS. Zero collisions, and almost no batching, which is a
# worse outcome than the collisions were. The cause is that nearly every branch
# touches `docs/WORKLIST.md`'s cold-start index, a `docs/PARITY-ROADMAP.md`
# section or `ci/figures.toml`, so after the first few groups fill, every
# remaining branch shares a path with every existing group.
#
# Those are exactly the files that carry merge drivers, and the drivers exist so
# that two branches appending to them do NOT conflict (AGENT-BRIEF: 26 of 48
# unmerged branches would not rebase onto main before the drivers, 9 after). So
# they are subtracted from the collision test.
#
# The test for "has a driver" is deliberately `git config --get
# merge.<name>.driver` and not merely "the attribute is set", because an
# attribute naming a driver git does not have falls back to the ordinary 3-way
# merge with NO WARNING — that is the whole reason
# `ci/setup-merge-drivers.sh` exists, and treating such a path as
# non-colliding would be trusting a promise the checkout has not kept.
declare -A DRIVER_OK=()
path_has_working_driver() {   # path -> 0 if a CONFIGURED merge driver owns it
    local p="$1" drv
    if [ -n "${DRIVER_OK[$p]:-}" ]; then [ "${DRIVER_OK[$p]}" = y ]; return; fi
    drv="$(git -C "$INT_DIR" check-attr merge -- "$p" 2>/dev/null | sed 's/^.*: merge: //')"
    case "$drv" in
        ""|unspecified|unset|binary|text) DRIVER_OK["$p"]=n; return 1 ;;
    esac
    if git config --get "merge.$drv.driver" >/dev/null 2>&1; then
        DRIVER_OK["$p"]=y; return 0
    fi
    DRIVER_OK["$p"]=n
    return 1
}

member_paths() {   # member-name -> changed paths that could actually collide
    local sha mb p
    sha="${MSHA[$1]}"
    mb="$(git merge-base "$BASE" "$sha" 2>/dev/null)" || return 1
    while IFS= read -r p; do
        [ -n "$p" ] || continue
        path_has_working_driver "$p" || printf '%s\n' "$p"
    done < <(git diff --name-only "$mb" "$sha" 2>/dev/null) | sort -u
}

pack_groups() {    # fills BATCH_GROUPS from RESOLVED, respecting $WIDTH
    local -a gnames=() gpaths=() ordered=()
    local name i placed n
    declare -A P=()
    for name in "${RESOLVED[@]}"; do P["$name"]="$(member_paths "$name")"; done

    # SCENARIO-BEARING BRANCHES ARE CONSIDERED FIRST, and that is not cosmetic.
    # Greedy first-fit puts whatever it sees first into the same few groups, so
    # the order decides which branches end up sharing a tip. Measured
    # 2026-08-17 over the live queue: with plain preflight order, 60 new
    # gameplay scenarios across 11 branches landed in 11 DIFFERENT groups, so
    # batching saved 11 -> 11 recordings, which is nothing. The branches that
    # bring scenarios are the ones whose grouping decides how many ~40-minute
    # corpus-lock acquisitions the queue pays, and they were being scattered by
    # an ordering that knew nothing about it.
    #
    # They go first, in queue order among themselves; everything else follows.
    for name in "${RESOLVED[@]}"; do
        [ "$(member_new_scenarios "$name" | grep -c . || true)" -gt 0 ] && ordered+=("$name")
    done
    for name in "${RESOLVED[@]}"; do
        case " ${ordered[*]:-} " in *" $name "*) ;; *) ordered+=("$name") ;; esac
    done
    [ "${#ordered[@]}" -eq "${#RESOLVED[@]}" ] || ordered=("${RESOLVED[@]}")

    for name in "${ordered[@]}"; do
        placed=""
        for i in "${!gnames[@]}"; do
            n="$(printf '%s\n' ${gnames[$i]} | grep -c .)"
            [ "$n" -lt "$WIDTH" ] || continue
            # Disjoint? comm -12 over two sorted lists; any shared path and the
            # pair may conflict, so the member goes elsewhere.
            if [ -z "$(comm -12 <(printf '%s\n' "${gpaths[$i]}") <(printf '%s\n' "${P[$name]}"))" ]; then
                gnames[$i]="${gnames[$i]} $name"
                gpaths[$i]="$(printf '%s\n%s\n' "${gpaths[$i]}" "${P[$name]}" | sort -u)"
                placed=1
                break
            fi
        done
        if [ -z "$placed" ]; then
            gnames+=("$name")
            gpaths+=("${P[$name]}")
        fi
    done
    BATCH_GROUPS=("${gnames[@]}")
}

slice_groups() {   # fills BATCH_GROUPS by taking RESOLVED in order, WIDTH at a time
    local i=0
    BATCH_GROUPS=()
    while [ "$i" -lt "${#RESOLVED[@]}" ]; do
        BATCH_GROUPS+=("${RESOLVED[*]:$i:$WIDTH}")
        i=$((i + WIDTH))
    done
}

# ---------------------------------------------------------------------------
# WHAT ci/handoff.sh WILL REFUSE, ASKED BEFORE THE 80 MINUTES ARE SPENT
# ---------------------------------------------------------------------------
# Every one of these is a check `ci/handoff.sh` makes at lines 417-423 and 450,
# and every one of them refuses in under a second there too — but only after
# the gate lock is taken and the log is opened, and in a queue where a refusal
# stops the chain. Asking first costs nothing and turns a batch that could never
# have been gated into a message rather than a slot in the queue.
#
# Note the deliberate use of PLAIN `--porcelain` on the integration worktree:
# that is what ci/handoff.sh:418 uses, so an untracked file there refuses the
# gate even though it would not refuse this script's own $ROOT.
gate_preconditions() {   # branch -> 0 if ci/handoff.sh's state checks would pass
    local br="$1" ahead behind dirty held
    if ! git rev-parse --verify -q "$br" >/dev/null; then
        echo "no such branch: $br"; return 1; fi
    dirty="$(git -C "$INT_DIR" status --porcelain 2>/dev/null | head -3)"
    if [ -n "$dirty" ]; then
        echo "the integration worktree is dirty by ci/handoff.sh:418's reckoning: $(printf '%s' "$dirty" | tr '\n' ' ')"
        return 1; fi
    ahead="$(git rev-list --count "main..$br" 2>/dev/null || echo 0)"
    behind="$(git rev-list --count "$br..main" 2>/dev/null || echo 0)"
    [ "$ahead" -gt 0 ]  || { echo "$br is 0 commits ahead of main"; return 1; }
    [ "$behind" -eq 0 ] || { echo "$br is $behind behind main — not a fast-forward"; return 1; }
    held="$(git worktree list --porcelain | awk -v b="refs/heads/$br" '
        /^worktree /{w=$2} $0=="branch "b{print w}' | grep -v -x "$INT_DIR" | head -1)"
    [ -z "$held" ] || { echo "$br is held by the worktree at $held"; return 1; }
    return 0
}

# ---------------------------------------------------------------------------
# ONE RECORDING FOR N BRANCHES' NEW SCENARIOS
# ---------------------------------------------------------------------------
# This may be worth more than the gate saving, and it was not in the original
# brief for this script.
#
# `ci/corpus-falsifiable.py --check` refuses `UNREGISTERED` — a gameplay scenario
# with no ledger entry — and it runs in T0. The entry comes from `--sweep`, which
# is free and lock-free, but the sweep needs a RECORDING, and a recording comes
# from `ci/t2-gameplay.sh` under the box-wide corpus lock at ~40 minutes a turn.
# So **the corpus lock is on the critical path for ADDING a scenario, not only
# for gating one.** Today four agents with new scenarios pay four lock
# acquisitions; two were stuck on exactly that this afternoon, one for 64
# minutes.
#
# A recording covers the scenarios present in the tree it ran against — so **the
# integration tip is already the fix.** One recording on a tip carrying N
# members' scenarios registers all of them. `ci/sweep-tip.sh` does that for a
# tree, refusing early if there is nothing to sweep and refusing to start while a
# production merge gate holds the lock; see its header for both.
#
# It changes the failure story too, which is the part that makes it worth wiring
# in rather than leaving to hand: **a batch that goes RED still leaves its
# members' scenarios swept**, so the 40 minutes is not thrown away by a verdict
# that had nothing to do with the recording. That is what `--sweep-back` is for.
#
# `SWEEP`/`SWEEP_BACK` are declared with the other defaults ABOVE the argument
# loop, not here. Declaring them here reset them to empty AFTER `--sweep` had set
# them, so the flag parsed, the help listed it, and the step silently never ran —
# five falsifier cases caught it on the first run.
LEDGER_PATH="ci/checks/gameplay-falsifiable.toml"

# The scenario files a member ADDS, by basename — which is the key the ledger
# uses (`file = "010-break-block.toml"`).
member_new_scenarios() {   # member-name -> basenames, one per line
    local sha mb
    sha="${MSHA[$1]}"
    mb="$(git merge-base "$BASE" "$sha" 2>/dev/null)" || return 1
    git diff --name-only --diff-filter=A "$mb" "$sha" -- ci/checks/gameplay/ 2>/dev/null \
        | sed 's|.*/||' | grep '\.toml$' | sort -u
}

sweep_tip_step() {
    local -a mine=("$@")
    local name n total=0 list
    for name in "${mine[@]}"; do
        n="$(member_new_scenarios "$name" | grep -c . || true)"
        [ "${n:-0}" -gt 0 ] && say "SWEEP    $name brings $n new scenario(s)"
        total=$((total + ${n:-0}))
    done
    if [ "$total" -eq 0 ]; then
        say "SWEEP    no member adds a gameplay scenario — no recording needed"
        return 0
    fi
    say "SWEEP    $total new scenario(s) across ${#mine[@]} member(s) — ONE recording covers all"
    if [ ! -x "$INT_DIR/ci/sweep-tip.sh" ]; then
        say "SWEEP    no ci/sweep-tip.sh on the integration tip — skipping"
        return 0
    fi
    ( cd "$INT_DIR" && RUSTMC_REMOTE_DIR="$REMOTE_DIR" ./ci/sweep-tip.sh ) 2>&1 \
        | sed 's/^/         | /'
    local rc=${PIPESTATUS[0]}
    if [ "$rc" -ne 0 ]; then
        say "SWEEP    ci/sweep-tip.sh exited $rc — the tip keeps whatever ledger it had"
        return "$rc"
    fi
    if [ -n "$(git -C "$INT_DIR" status --porcelain -- "$LEDGER_PATH")" ]; then
        git -C "$INT_DIR" add "$LEDGER_PATH" || return 1
        git -C "$INT_DIR" commit -q -m "ci/checks/gameplay-falsifiable.toml: sweep the batch tip

One recording on an integration tip carrying $total new scenario(s) from
${#mine[@]} branches, instead of one ~40-minute corpus-lock acquisition each.
Taken as RUSTMC_CORPUS_LOCK_ROLE=run so a production merge gate still jumps
ahead of it." || return 1
        say "SWEEP    ledger committed onto $BATCH_BR at $(git -C "$INT_DIR" rev-parse --short HEAD)"
    else
        say "SWEEP    the sweep left the ledger unchanged"
    fi
    [ -n "$SWEEP_BACK" ] && sweep_back "${mine[@]}"
    return 0
}

# Graft each member's own rows back onto its own branch, WITHOUT checking
# anything out. `git commit-tree` over a temporary index touches no worktree and
# no shared index, and `git update-ref`/`--force-with-lease` make the write a
# compare-and-swap rather than a clobber. Checking a member's branch out to do
# this would hold it, and a held branch blocks the merge gate.
sweep_back() {
    local -a mine=("$@")
    local name sha files tmpd tmpled blob tree commit old
    tmpd="$(mktemp -d "${TMPDIR:-/tmp}/batch-graft.XXXXXX")" || return 1
    for name in "${mine[@]}"; do
        files="$(member_new_scenarios "$name" | tr '\n' ' ')"
        [ -n "${files// /}" ] || continue
        sha="${MSHA[$name]}"
        # An agent may be live in it, or have unpushed work on it. Either way its
        # branch is not ours to append to.
        local held
        held="$(git worktree list --porcelain | awk -v b="refs/heads/$name" '
            /^worktree /{w=$2} $0=="branch "b{print w}' | head -1)"
        if [ -n "$held" ]; then
            say "SWEEP-BACK skip $name — held by the worktree at $held, an agent may be live"
            continue
        fi
        old="$(git rev-parse --verify -q "refs/heads/$name" || true)"
        if [ -n "$old" ] && [ "$old" != "$sha" ]; then
            say "SWEEP-BACK skip $name — local ${old:0:8} differs from origin ${sha:0:8}, unpushed work"
            continue
        fi
        tmpled="$tmpd/$name.toml"
        git show "$sha:$LEDGER_PATH" > "$tmpled" 2>/dev/null || {
            say "SWEEP-BACK skip $name — no $LEDGER_PATH at its tip"; continue; }
        # shellcheck disable=SC2086
        if ! ( cd "$INT_DIR" && ./ci/ledger-graft.py --from "$LEDGER_PATH" --to "$tmpled" $files ) \
                >"$tmpd/$name.log" 2>&1; then
            say "SWEEP-BACK skip $name — ledger-graft refused:"
            sed 's/^/           | /' "$tmpd/$name.log" | head -4
            continue
        fi
        blob="$(git hash-object -w "$tmpled")" || continue
        (
            export GIT_INDEX_FILE="$tmpd/index-$name"
            git read-tree "$sha" &&
            git update-index --cacheinfo "100644,$blob,$LEDGER_PATH" &&
            git write-tree
        ) > "$tmpd/tree-$name" 2>/dev/null || { say "SWEEP-BACK skip $name — could not build a tree"; continue; }
        tree="$(tail -1 "$tmpd/tree-$name")"
        [ -n "$tree" ] || { say "SWEEP-BACK skip $name — empty tree"; continue; }
        commit="$(git commit-tree "$tree" -p "$sha" -m \
"ci/checks/gameplay-falsifiable.toml: this branch's scenarios, swept on a batch tip

Grafted verbatim by ci/ledger-graft.py from a recording taken once over an
integration tip carrying several branches' new scenarios, rather than this
branch paying its own ~40-minute corpus-lock acquisition. The entries tie each
scenario's red-when-broken evidence to the version of the scenario it was taken
against; nothing was edited or synthesised.")" || continue
        if git push -q --force-with-lease="$name:$sha" origin "$commit:refs/heads/$name" 2>"$tmpd/push-$name"; then
            say "SWEEP-BACK $name -> ${commit:0:8} pushed ($files)"
        else
            say "SWEEP-BACK $name push refused — the branch moved under us:"
            sed 's/^/           | /' "$tmpd/push-$name" | head -3
        fi
    done
    rm -rf "$tmpd"
}

# ---------------------------------------------------------------------------
# LANDING — the only place main is written
# ---------------------------------------------------------------------------
# `git rev-parse --git-common-dir` answers `.git` relative to cwd in a main
# checkout and an absolute path in a linked worktree. Resolved here so the lock
# is one file for every worktree of this repo rather than one per cwd — a
# per-cwd lock excludes nobody, which is the shape of the gate-lock confusion
# corrected in AGENT-BRIEF.
LOCK_FILE="$(cd "$(git rev-parse --git-common-dir)" && pwd)/rustmc-main-writer.lock"
LANDED_TOTAL=0

main_held_by() {
    local w="" line
    while IFS= read -r line; do
        case "$line" in
            worktree\ *) w="${line#worktree }" ;;
            branch\ refs/heads/main) printf '%s\n' "$w" ;;
        esac
    done < <(git worktree list --porcelain)
}

land_tip() {
    local tip="$1" base="$2"; shift 2
    local -a names=("$@")
    if [ -z "$LAND" ]; then
        say "NOT LANDED  ${tip:0:8} is gated and landable (+$(git rev-list --count "$base..$tip") commits, ${#names[@]} branches: ${names[*]})"
        say "            --land is the owner's switch. To land it by hand, inside the"
        say "            main-writer mutex:  git update-ref refs/heads/main $tip $base && git push origin main"
        return 0
    fi
    exec 9>>"$LOCK_FILE" || { echo "LAND REFUSED  cannot open $LOCK_FILE"; return 3; }
    flock -x -w 300 9 || { echo "LAND REFUSED  another main-writer holds $LOCK_FILE"; return 3; }

    local held now_local now_origin remote_sha rc=0
    held="$(main_held_by | head -1)"
    if [ -n "$held" ]; then
        echo "LAND REFUSED  main is checked out at $held"
        echo "              Advancing the ref under a worktree that holds it makes every"
        echo "              file of the batch read as modified there. That is the"
        echo "              2026-08-17 bug — ci/handoff.sh:418 then refuses every gate."
        exec 9>&-; return 3
    fi
    now_local="$(git rev-parse --verify -q refs/heads/main || true)"
    now_origin="$(git rev-parse --verify -q refs/remotes/origin/main || true)"
    remote_sha="$(git ls-remote origin refs/heads/main 2>/dev/null | awk '{print $1; exit}')"
    if [ "$now_local" != "$base" ]; then
        echo "LAND REFUSED  local main moved ${base:0:8} -> ${now_local:0:8} since the gate"
        exec 9>&-; return 3
    fi
    if [ -n "$now_origin" ] && [ "$now_origin" != "$base" ]; then
        echo "LAND REFUSED  origin/main moved ${base:0:8} -> ${now_origin:0:8} since the gate"
        exec 9>&-; return 3
    fi
    if [ -z "$remote_sha" ]; then
        echo "LAND REFUSED  could not read origin's main with ls-remote — no lease, no land"
        exec 9>&-; return 3
    fi
    if [ "$remote_sha" != "$base" ]; then
        echo "LAND REFUSED  the remote's main is ${remote_sha:0:8}, not the gated base ${base:0:8}"
        exec 9>&-; return 3
    fi
    git merge-base --is-ancestor "$base" "$tip" || {
        echo "LAND REFUSED  ${base:0:8} is not an ancestor of ${tip:0:8} — not a fast-forward"
        exec 9>&-; return 3; }

    # Compare-and-swap, not a write: the third argument is the value the ref
    # must currently have, so a racing writer loses here rather than downstream.
    if ! git update-ref refs/heads/main "$tip" "$base"; then
        echo "LAND REFUSED  update-ref CAS failed — main is not at ${base:0:8} any more"
        exec 9>&-; return 3
    fi
    if ! git push -q origin main; then
        git update-ref refs/heads/main "$base" "$tip" \
            && echo "LAND FAILED   push rejected; local main rolled back to ${base:0:8}" \
            || echo "LAND FAILED   push rejected AND rollback failed — main is locally ahead at ${tip:0:8}"
        exec 9>&-; return 3
    fi
    exec 9>&-
    say "LANDED   ${#names[@]} branches -> main at ${tip:0:8} (+$(git rev-list --count "$base..$tip") commits): ${names[*]}"
    LANDED_TOTAL=$((LANDED_TOTAL + ${#names[@]}))
    BASE="$tip"          # every later group bases on what we just landed
    return 0
}

# ---------------------------------------------------------------------------
# GATING ONE TIP
# ---------------------------------------------------------------------------
GATES=0
GATE_LOG_DIR="${RUSTMC_BATCH_LOGDIR:-$HOME/rustmc-private/gates}"
mkdir -p "$GATE_LOG_DIR" 2>/dev/null

gate_tip() {
    local br="$1" cmd out rc
    if [ "$MAX_GATES" -ne 0 ] && [ "$GATES" -ge "$MAX_GATES" ]; then
        say "BUDGET   --max-gates $MAX_GATES reached; not gating $br"
        return 5
    fi
    local pid
    if pid="$(remote_dir_conflict)"; then
        echo "REFUSED  RUSTMC_REMOTE_DIR=$REMOTE_DIR is owned by a live ci/handoff.sh (pid $pid)"
        echo "         Read out of /proc/$pid/environ, not matched on a process name."
        return 2
    fi
    cmd="${GATE_CMD//%BRANCH%/$br}"
    GATES=$((GATES + 1))
    say "GATE $GATES   $br — $cmd"
    out="$GATE_LOG_DIR/batch-$br.out"
    ( cd "$INT_DIR" && RUSTMC_REMOTE_DIR="$REMOTE_DIR" bash -c "$cmd" ) >"$out" 2>&1
    rc=$?
    tail -3 "$out" | sed 's/^/         | /'
    return $rc
}

# ---------------------------------------------------------------------------
# THE RECURSION
# ---------------------------------------------------------------------------
# Verdicts accumulate here so the report can say what happened to every member
# named on the command line — including the ones a batch never reached.
declare -A VERDICT=()
RED_SEEN=0
INFRA_SEEN=0

run_group() {
    local -a want=("$@")
    local n="${#want[@]}" rc name
    [ "$n" -gt 0 ] || return 0

    build_tip "${want[@]}"; rc=$?
    if [ "$rc" -ne 0 ]; then
        for name in "${want[@]}"; do VERDICT["$name"]="REFUSED"; done
        RC_FINAL=2
        return 2
    fi
    for e in "${EXCLUDED[@]:-}"; do
        [ -n "$e" ] || continue
        VERDICT["${e%%:*}"]="EXCLUDED-${e##*:}"
    done
    if [ "${#ADMITTED[@]}" -eq 0 ]; then
        say "GROUP    nothing admitted from ${want[*]} — no gate run"
        return 0
    fi

    local tip mybr; mybr="$BATCH_BR"
    local -a mine=("${ADMITTED[@]}")
    say "GROUP    ${#mine[@]} admitted (${mine[*]}) -> ${mybr} at $(git -C "$INT_DIR" rev-parse --short HEAD)"

    # Ask ci/handoff.sh's own state checks first. They refuse in under a second
    # there too, but only after the gate lock is taken and the log is opened,
    # and a refusal in this queue stops the chain. The members are UNJUDGED, not
    # red: nothing about them was measured.
    local why
    if ! why="$(gate_preconditions "$mybr")"; then
        say "REFUSED  $mybr would be refused by ci/handoff.sh before it gated: $why"
        for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-PRECONDITION"; done
        RC_FINAL=2
        return 2
    fi

    # Before the gate, not after: the recording is the expensive resource and it
    # is worth having whatever the verdict turns out to be.
    [ -n "$SWEEP" ] && sweep_tip_step "${mine[@]}"

    # THE TIP IS READ HERE, AFTER EVERYTHING THAT CAN ADD A COMMIT TO IT.
    # It used to be read before the sweep, so a swept batch gated one tip and
    # landed a DIFFERENT, earlier one — main would have got the branches without
    # the ledger commit that the gate had actually measured. Landing a commit the
    # gate did not measure is the whole thing this script exists to not do, and
    # ci/batch-falsify.sh caught it on the first run of the sweep cases.
    tip="$(git -C "$INT_DIR" rev-parse HEAD)"
    say "GROUP    gating ${mybr} at ${tip:0:8}"

    gate_tip "$mybr"; rc=$?

    case "$rc" in
      0)
        # And the belt to that brace: whatever the gate measured is what lands.
        # A tip that moved between the gate and the land is a verdict about a
        # different commit, and the correct response is to refuse rather than to
        # re-read it.
        local now; now="$(git -C "$INT_DIR" rev-parse HEAD)"
        if [ "$now" != "$tip" ]; then
            say "REFUSED  $mybr moved ${tip:0:8} -> ${now:0:8} between the gate and the land"
            say "         The green verdict is about ${tip:0:8}, not about what is there now."
            for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-TIP-MOVED"; done
            RC_FINAL=2
            return 2
        fi
        for name in "${mine[@]}"; do VERDICT["$name"]="GREEN"; done
        land_tip "$tip" "$BASE" "${mine[@]}" || { RC_FINAL=3; return 3; }
        if [ -n "$LAND" ]; then
            for name in "${mine[@]}"; do VERDICT["$name"]="LANDED"; done
        fi
        return 0 ;;
      1)
        : ;;                    # the RED path, below
      4|255)
        say "INFRA    $mybr — the gate could not run; ${#mine[@]} member(s) unjudged"
        for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-INFRA"; done
        INFRA_SEEN=1; RC_FINAL=4
        return 4 ;;
      5)
        for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-BUDGET"; done
        return 5 ;;
      2|3)
        say "REFUSED  $mybr — the gate refused (rc $rc); ${#mine[@]} member(s) unjudged"
        for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-REFUSED"; done
        RC_FINAL=2
        return 2 ;;
      *)
        # An exit code this script does not recognise is NOT a verdict. Reading
        # it as RED would condemn branches on a status nobody has attributed —
        # the same reason ci/handoff.sh grew INFRA on its own exit code.
        say "INFRA    $mybr — unrecognised gate exit $rc; ${#mine[@]} member(s) unjudged"
        for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-EXIT$rc"; done
        INFRA_SEEN=1; RC_FINAL=4
        return 4 ;;
    esac

    # --- RED ---------------------------------------------------------------
    if [ "${#mine[@]}" -eq 1 ]; then
        say "RED      ${mine[0]} — gated alone, this is that branch's own verdict"
        VERDICT["${mine[0]}"]="RED"
        RED_SEEN=1; RC_FINAL=1
        return 1
    fi
    say "RED      $mybr (${#mine[@]} members) — no member is implicated yet"
    case "$ON_RED" in
      none)
        for name in "${mine[@]}"; do VERDICT["$name"]="UNJUDGED-BATCHRED"; done
        say "         --on-red none: stopping. Members are UNJUDGED, not red."
        RC_FINAL=1; RED_SEEN=1
        return 1 ;;
      halves)
        local a=$(( (${#mine[@]} + 1) / 2 ))
        local -a l=("${mine[@]:0:$a}") r=("${mine[@]:$a}")
        say "         --on-red halves: re-gating ${l[*]} then ${r[*]}"
        run_group "${l[@]}"
        run_group "${r[@]}"
        ;;
      singles)
        local -a members=("${mine[@]}")
        say "         --on-red singles: re-gating each of ${members[*]} alone" \
            "(${#members[@]} gates; cheapest sound strategy at the measured p=0.7911)"
        for name in "${members[@]}"; do run_group "$name"; done
        # INTERACTION RED. Every member green alone but red together: the fault
        # is the combination, and blaming any single member would be a verdict
        # no measurement produced.
        local allgreen=1
        for name in "${members[@]}"; do
            case "${VERDICT[$name]}" in GREEN|LANDED) ;; *) allgreen="" ;; esac
        done
        if [ -n "$allgreen" ]; then
            say "INTERACTION  $mybr was RED and all ${#members[@]} members gate GREEN alone."
            say "             These branches are red only in COMBINATION. None of them is"
            say "             at fault and none is condemned; they are landable apart."
            for name in "${members[@]}"; do
                case "${VERDICT[$name]}" in
                    LANDED) ;;
                    *) VERDICT["$name"]="GREEN-INTERACTION" ;;
                esac
            done
        fi
        ;;
    esac
    return 0
}

# ---------------------------------------------------------------------------
# MAIN
# ---------------------------------------------------------------------------
say "batch: ${#MEMBERS[@]} branches, width $WIDTH, on-red $ON_RED, land=${LAND:-no}"
say "       base $BASE_REF at ${BASE:0:8}   int-dir $INT_DIR   remote-dir $REMOTE_DIR"

RESOLVED=()
for m in "${MEMBERS[@]}"; do
    if sha="$(resolve_member "$m")"; then
        MSHA["$m"]="$sha"
        if [ "$(git rev-list --count "$BASE..$sha" 2>/dev/null || echo 0)" -eq 0 ]; then
            say "SKIP     $m — 0 commits ahead of $BASE_REF, nothing to land"
            VERDICT["$m"]="SKIP-EMPTY"
        else
            RESOLVED+=("$m")
        fi
    else
        say "SKIP     $m — no such branch on origin or locally"
        VERDICT["$m"]="SKIP-MISSING"
    fi
done

if [ "${#RESOLVED[@]}" -eq 0 ]; then
    say "nothing to batch"
    RC_FINAL=2
    exit 2
fi

if [ -n "$PACK" ]; then
    # The integration worktree is prepared FIRST so that `git check-attr` reads
    # the BASE's `.gitattributes` rather than whatever this checkout happens to
    # be sitting on. Which paths carry a merge driver is a property of the tree
    # being merged into, not of the operator's branch.
    int_prepare || { RC_FINAL=2; exit 2; }
    pack_groups
    say "--pack: ${#BATCH_GROUPS[@]} groups by disjoint file sets, width <= $WIDTH" \
        "(driver-managed paths do not count as collisions)"
else
    slice_groups
    say "${#BATCH_GROUPS[@]} groups by queue order, width $WIDTH (--pack groups by disjoint files instead)"
fi

if ! rate_guard; then
    RC_FINAL=2
    exit 2
fi

GN=0
SIZES=""
PLAN_SCENARIOS=0
PLAN_RECORDINGS=0
PLAN_MEMBERS_WITH_SCENARIOS=0
if [ -n "$PLAN" ]; then
    # Feasibility only: build every group's tip, ask ci/handoff.sh's own state
    # checks whether it would accept it, and report. Seconds rather than 80
    # minutes, and it is the half of the answer that does not need the box.
    say "--plan: cherry-pick feasibility and gate preconditions only, no gate will run"
    for g in "${BATCH_GROUPS[@]}"; do
        GN=$((GN + 1))
        # shellcheck disable=SC2086
        build_tip $g || true
        local_pre="ok"
        if [ "${#ADMITTED[@]}" -gt 0 ]; then
            local_pre="$(gate_preconditions "$BATCH_BR")" || local_pre="WOULD-REFUSE: $local_pre"
            [ -n "$local_pre" ] || local_pre="ok"
        else
            local_pre="no members"
        fi
        # What ONE recording on this tip would register. Counted in --plan
        # because it is the half of the saving that costs no gate to estimate,
        # and because "how many corpus-lock acquisitions does batching save"
        # is a different and possibly larger question than "how many gates".
        gscen=0
        for a in "${ADMITTED[@]:-}"; do
            [ -n "$a" ] || continue
            gn="$(member_new_scenarios "$a" | grep -c . || true)"
            gscen=$((gscen + ${gn:-0}))
            [ "${gn:-0}" -gt 0 ] && PLAN_MEMBERS_WITH_SCENARIOS=$((PLAN_MEMBERS_WITH_SCENARIOS + 1))
        done
        PLAN_SCENARIOS=$((PLAN_SCENARIOS + gscen))
        [ "$gscen" -gt 0 ] && PLAN_RECORDINGS=$((PLAN_RECORDINGS + 1))
        say "PLAN     group $GN: admitted ${#ADMITTED[@]} (${ADMITTED[*]:-none})" \
            "excluded ${#EXCLUDED[@]} (${EXCLUDED[*]:-none}) preconditions: $local_pre" \
            "new-scenarios: $gscen"
        SIZES="${SIZES:+$SIZES,}${#ADMITTED[@]}"
        for e in "${EXCLUDED[@]:-}"; do [ -n "$e" ] && VERDICT["${e%%:*}"]="EXCLUDED-${e##*:}"; done
        for a in "${ADMITTED[@]:-}"; do [ -n "$a" ] && VERDICT["$a"]="PLAN-OK"; done
    done
    say "PLAN     group sizes: $SIZES"
    say "         score them:  ci/batch-arith.py --groups $SIZES --collisions <n>"
    if [ "$PLAN_SCENARIOS" -gt 0 ]; then
        say "PLAN     $PLAN_SCENARIOS new gameplay scenario(s) across $PLAN_MEMBERS_WITH_SCENARIOS branch(es)"
        say "         serially that is $PLAN_MEMBERS_WITH_SCENARIOS corpus-lock acquisitions;" \
            "batched it is $PLAN_RECORDINGS"
        say "         at ~40 min a recording that is" \
            "$(( (PLAN_MEMBERS_WITH_SCENARIOS - PLAN_RECORDINGS) * 40 )) min saved, and it is" \
            "NOT conditional on the green-at-verdict rate"
    else
        say "PLAN     no member adds a gameplay scenario — no recording saving on this queue"
    fi
else
    for g in "${BATCH_GROUPS[@]}"; do
        # shellcheck disable=SC2086
        run_group $g
    done
fi

echo
echo "=== batch report ==="
printf '%-46s %s\n' "BRANCH" "VERDICT"
for m in "${MEMBERS[@]}"; do
    printf '%-46s %s\n' "$m" "${VERDICT[$m]:-UNREACHED}"
done
echo
echo "gates run:      $GATES"
echo "branches landed: $LANDED_TOTAL"
if [ -z "$LAND" ] && [ "$GATES" -gt 0 ]; then
    # Without --land the base never advances, so every group is gated against
    # the SAME main. Two green tips both claim to fast-forward it and only the
    # first one can. Saying so beats a report that reads as though N groups
    # were all landable at once.
    echo "note:           --land was off, so every group was gated against ${BASE:0:8}."
    echo "                Only the FIRST green tip is landable as-is; the rest need"
    echo "                rebuilding on the new main (cheap) but not re-gating unless"
    echo "                the rebuild changes their content."
fi
[ "$RED_SEEN" -eq 1 ] && [ "$RC_FINAL" -eq 0 ] && RC_FINAL=1
exit "$RC_FINAL"
