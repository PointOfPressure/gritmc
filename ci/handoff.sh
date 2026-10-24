#!/usr/bin/env bash
# ci/handoff.sh <branch> — the whole orchestrator loop in one command.
#
# WHY THIS EXISTS. Gating, merging, pushing and marking used to be four or five
# separate orchestrator tool calls. An orchestrator's context is large and every
# tool call replays all of it, so those calls cost several times what the same
# call costs inside an agent. Measured 2026-08-03: over 3.1 hours the weekly
# allowance moved 5 points, of which ~1.6 was the agent doing the actual work
# and ~3.4 was the orchestration around it. The decisions were never the
# expensive part; the typing was.
#
# This does not move a single judgement out of the orchestrator's hands. It
# refuses to merge on anything but an unambiguous green, and it prints one line.
#
# ONE GATE DIRECTORY, NAMED ONCE.
#
# The first version of this script hardcoded `RUSTMC_REMOTE_DIR=rustmc-gate` on
# the line that ran T0 and never mentioned the gate directory again — and it ran
# T0 only, so the gameplay corpus was outside the loop entirely. That is the
# shape of the bug it is supposed to prevent: on 2026-08-03 the gate directory
# had branch-tip source, an `oracle-work` symlinked into a **different
# checkout**, and `data/vanilla` tables seeded from that other checkout with
# `cp -rn` — no-clobber, so the branch's own new table never arrived and its
# rewritten one stayed stale. Every stage ran; they did not all run on the same
# tree. The branch was reported 12/55 with 11 regressed and was not broken.
#
# So the gate directory is named exactly once, in `$REMOTE_DIR`, and every stage
# that touches the box goes through `remote()`, which cds into it. There is no
# second place to get it wrong.
#
# THE STAGES, and what each one is for:
#
#   1  the gate dir owns its own oracle-work and data/vanilla
#   2  rsync the branch tip                  — ci/t0-remote.sh --sync
#   3  gen-gameplay-tables.sh                — this branch's own generated data
#   4  T0 via ci/t0-remote.sh --run          — fmt, clippy, check, lib+bins tests
#   5  T1 via ci/t1-gate.sh                  — every test binary, ~676 s
#   6  T2 gameplay corpus                    — real vanilla as reference, ~50 min
#
# Stage 5 arrived on 2026-08-05 and closed the largest coverage hole this loop
# had: 78 test binaries — every golden vector and every integration test under
# `tests/` — had never run in any merge gate, because T1 exits 101 on two
# honestly-red goldens. See the comment on that stage for why it sits exactly
# there and not either side of it.
#
# ===========================================================================
# STAGE 3 NOW RUNS BEFORE T0, AND THAT CHANGES WHAT T0 MEANS. 2026-08-06.
# ===========================================================================
#
# This is not a tidy-up. **T0 asserts something different from what it asserted
# before, and the difference is the entire point.**
#
#   Before: T0 ran against whatever `data/vanilla` stage 1 left behind — a
#           `cp -rn` (no-clobber) copy of a *shared* directory, with the
#           provenance stamps deleted. A table this branch invented was simply
#           absent, because a no-clobber copy cannot supply what the source
#           does not have. A table this branch's own generator rewrites was
#           present, well-formed and **stale**. So T0 was a claim about this
#           branch's code measured against **some other branch's data**.
#
#   After:  T0 runs against tables generated from this tree's own source under
#           this tree's own stamps. It is a claim about this branch.
#
# That is strictly stronger and it is a *widening*: nothing that reddened T0
# before can pass now for want of a table. No test was made more tolerant, and
# none may be — a missing or stale table must stay a loud red. Supplying the
# table is this script's job, never the test's job to survive without.
#
# THE DEFECT THIS CLOSES, AND HOW LONG IT WAS OPEN.
#
# `data/vanilla` is gitignored (.gitignore:28) and excluded from the rsync, so a
# generated table can never arrive with the branch. It arrives from stage 1's
# `cp -rn`, or from stage 3, and from nowhere else. Stage 3 ran **after** T0 and
# was skipped outright when T0 was red — so the one stage that could supply the
# table was gated behind the stage that needed it. Two shapes, one defect:
#
#   * a branch that adds a **new** table gated with that table missing.
#     `redstone-power` (data/vanilla/signal.json, from ci/signal-golden.sh
#     and tools/goldengen/vanilla/SignalGolden.java, both of which the branch
#     ships, and which its own ci/gen-gameplay-tables.sh wires up correctly):
#     2 failures in `-p rustmc-server --lib`, "signal.json ... NotFound".
#
#   * a branch that adds a **column to an existing** table gated against the
#     stale shared copy. This is the nastier one — the table is present and
#     well-formed and only wrong. `tick-randomtick-selection`
#     (`randomlyTicking` in block_props.json, via a changed
#     tools/goldengen/vanilla/BlockPropsGolden.java): 8 failures in
#     `-p rustmc-behavior --lib`. That test refuses to load rather than
#     defaulting every state to non-random-ticking, which is why this was a
#     red and not a silently wrong server. Keep it that way.
#
# Neither red said one word about the branch it was reporting on, which is the
# exact failure mode the top of this file exists to refuse. The ordering has
# been wrong for the project's whole history and had never once fired, because
# until 2026-08-06 every branch reused tables the shared directory already had.
# Two branches invented one on the same day and the merge queue stopped dead.
#
# WHAT WAS CONSIDERED AND IS NOT NEEDED. "Generation needs a binary that T0
# builds" sounds true, and would have justified a bootstrap build or splitting
# the generator out of the workspace. It is **false**, and that is worth
# writing down so nobody re-derives it: not one of the generators named in
# `generator_for()` invokes cargo. Every one is `javac`/`java` against the
# version-pinned jar plus `python3`. The only thing generation takes from the
# branch is the branch's *source*, which is exactly what stage 2 ships. There
# is no cycle. Generating during stage 1 would not work either — stage 1 runs
# before the rsync, so the generator scripts and the goldengen java sitting in
# that directory are the *previous* gate's, not this branch's.
#
# THE COST, STATED HONESTLY. `cargo fmt --check` could previously reject a
# branch in seconds and now runs after generation. On a warm gate directory
# that costs only what the branch actually changed — usually one table, seconds
# to a minute, because `prov_check` regenerates only what is absent or whose
# generator digest moved. On a cold gate directory it is all of them, including
# ci/shape-golden.sh, which is minutes. That is the price of T0 meaning what it
# says.
#
# ===========================================================================
# `RED` IS A VERDICT ABOUT THE BRANCH. INFRASTRUCTURE FAILURES NOW SAY `INFRA`.
# ===========================================================================
#
# This file already says, twice and in as many words, that a failure which says
# nothing about the branch must not be reported as one — that is the argument
# for `REFUSED` on a held worktree, and it is the argument at the top for why
# stage 3 moved before T0. And then stage 1 printed `RED` when the *gate
# directory could not be prepared*, which is the same mistake in the same file.
#
# It has already cost a wrong diagnosis. A `RED` sends the reader to the
# branch: rebase it, re-read the diff, bisect it. When the actual cause was a
# missing seed directory or an ssh that never connected, every one of those is
# wasted, and the branch carries a mark it did not earn.
#
# So there is a fourth verdict, `INFRA`, on its own exit code 4:
#
#   MERGED         (0)  gate green, merged, AND pushed. origin/main has it.
#   ALREADY-MERGED (0)  the branch is already an ancestor of main. Idempotent
#                       re-run, not a fresh merge — see below.
#   GREEN          (0)  --dry-run only: gate green, nothing merged.
#   RED            (1)  the gate measured the branch and the branch failed
#   REFUSED        (2)  local state stopped the gate. Cheap to fix, cheap to re-run.
#   NOT-FF         (3)  the branch is not a fast-forward of main. Rebase it.
#   INFRA          (4)  a stage could not run. NOTHING about the branch was measured.
#   UNPUSHED       (5)  green, merged in LOCAL main, and origin does not have it.
#                       NOTHING ELSE CAN BE GATED until they agree — see below.
#
# THE VOCABULARY IS PREFIX-FREE AND THAT IS A CHECKED PROPERTY, not a
# convention. The verdict this loop got most wrong was `MERGED-NOT-PUSHED`,
# which `grep -E '^MERGED'` matches: the state meaning "the queue is about to
# wedge" was namable as the state meaning "everything worked". Two of the 32
# REDs in this loop's record arrived as another branch's MERGED.
# `ci/checks/handoff-terminals.toml` declares every word and
# `ci/check-handoff-terminals.py` fails T0 if one becomes a prefix of another,
# if a terminal prints a word its row denies, if a terminal prints nothing at
# all, or if `ci/lanes.sh` — the only in-tree consumer — cannot name one.
#
# `UNPUSHED` and `ALREADY-MERGED` are the two that exist because of what
# happens BELOW the verdict, and both were previously reported as something
# else:
#
#   * a push that fails after a successful merge printed `MERGED-NOT-PUSHED`
#     and exited 2, which is documented four lines up as "the gate could not
#     run". It had run all six stages, passed, and merged. `BEHIND` is measured
#     against LOCAL main, so one unpushed merge makes every branch behind it in
#     the queue read `N behind main` and refuse — 2026-08-16, one timed-out push
#     presenting as 37 branches needing a rebase that would not have helped.
#     The disagreement is now detected at ENTRY as well, repaired where it can
#     be (pushing changes when those commits leave, never what leaves), and
#     named where it cannot;
#   * a gate killed between the merge and the push prints NOTHING — SIGKILL
#     runs no trap — and the re-run said `0 commits ahead of main, nothing to
#     merge`, which is true, useless, and the wrong instruction.
#
# WHICH FAILURES ARE NOT THE BRANCH'S, stage by stage, and why:
#
#   stage 1  prepare      ALWAYS infra. This stage runs a heredoc written in
#                         THIS file, against $REFS and $SHARED_DATA. It reads
#                         no branch source — the branch has not even been
#                         rsynced yet, that is stage 2 — so no failure here can
#                         be evidence about the branch. That is a property of
#                         what the stage does, not a guess about why it failed.
#   stage 2  rsync        infra when t0-remote.sh reports the transfer itself
#                         failed (its own `rsync ... failed` sentence). A
#                         non-zero for any other reason stays RED: that script
#                         comes from the branch and the branch can break it.
#   any stage exit 255    infra. 255 is ssh's own "the connection failed", so
#                         the command never ran on the box.
#
# Everything else is RED and must stay RED. The list above is deliberately
# short: an over-eager INFRA is the worse failure, because it would let a real
# red slip through wearing the word "unrelated". Note also which way this
# errs — every rule here *withholds* a verdict rather than inventing one. If a
# gate script ever genuinely exits 255, this reports INFRA on a real branch
# failure, and the queue stalls with a re-run, rather than a bad branch merging.
#
# `INFRA` is not a lesser `RED`. It is not a verdict at all, and the merge does
# not happen either way. The difference is entirely in what the reader does
# next, and that is the whole value of it.
#
# WHAT DRIVES THIS FILE, and every one of them runs in T0:
#
#   ci/check-handoff-terminals.py       every `exit` here is declared in
#                                       ci/checks/handoff-terminals.toml with
#                                       the word it prints, what it leaves
#                                       behind, and the control that forces it.
#                                       The word is DERIVED from this source,
#                                       not read from the declaration.
#   ci/check-handoff-terminals-falsify.py   ten injections, one per rule above.
#   ci/handoff-state-falsify.sh         the refusals reached before the box.
#   ci/handoff-verdict-selftest.sh      the stage outcomes: which failure
#                                       reaches RED and which reaches INFRA.
#   ci/handoff-merge-tail-falsify.sh    everything BELOW the verdict — the real
#                                       merge, the real push, a real origin,
#                                       and the partial failures: merged and
#                                       not pushed, killed mid-merge, main
#                                       moved under the gate, main and
#                                       origin/main diverged.
#
# Usage:  ./ci/handoff.sh <branch> [--dry-run]
#   RUSTMC_REMOTE       host, default threadripper
#   RUSTMC_REMOTE_DIR   gate dir on that host, default rustmc-gate
#   RUSTMC_REFS         where the vanilla references live, default ~/rustmc/oracle-work
#   RUSTMC_SHARED_DATA  where a fresh gate dir seeds data/vanilla from,
#                       default ~/rustmc/data/vanilla
#   CARGO_BUILD_JOBS    per-lane cargo cap, spliced onto the remote command
#   RUSTMC_REFERENCE_CACHE  0 or 1, spliced onto stage 6's remote command
#   RUSTMC_PUSH_RETRY_GAPS  only value is 0: the three pushes back to back,
#                       for tests. It cannot change the NUMBER of attempts.
# Exit:   0 merged and pushed (or already merged, or a green --dry-run)
#         | 1 gate red (the BRANCH failed) | 2 refused (local state)
#         | 3 not a fast-forward | 4 infra (a stage could not run; the branch is
#         unjudged) | 5 unpushed (merged locally; main and origin/main disagree
#         and NOTHING else can be gated until they do)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# A SILENT TERMINAL IS A "no verdict line" IN THE RECORD. This exited 2 saying
# nothing at all. Exit 2 is `REFUSED`, so a consumer reading the code got the
# right classification and a consumer reading the OUTPUT got a blank — and this
# loop's record has 30 REDs that arrived with no verdict line, which is the same
# shape. It is very nearly unreachable (a bash that resolved and executed this
# script can normally cd into its own parent), and that is not a reason to let
# it be the one path that prints nothing: an unreachable state that speaks costs
# two lines, and an unreachable state that is silent is indistinguishable from a
# reachable one that is.
cd "$ROOT" || { echo "REFUSED  cannot cd into $ROOT — the repository root this script resolved from is unreadable"; exit 2; }

BRANCH="${1:-}"
DRY=""
# --- AN UNRECOGNISED ARGUMENT IS A REFUSAL, BECAUSE THE DEFAULT MERGES. -----
#
# This was `[ "${2:-}" = "--dry-run" ] && DRY=1` and nothing else, which means
# **every way of mistyping the flag silently merges to main**:
#
#     ./ci/handoff.sh mybranch --dryrun     typo   -> a real merge
#     ./ci/handoff.sh mybranch --dry-run=1  guess  -> a real merge
#     ./ci/handoff.sh --dry-run mybranch    order  -> a real merge, and it
#                                                     gates the branch `--dry-run`
#     ./ci/handoff.sh mybranch -n           habit  -> a real merge
#
# A flag whose absence is the destructive option must not fail open. Nothing in
# this file's output would have said the flag was dropped either: a real run and
# a dry run print different verdicts at the END, by which point the merge has
# happened. The operator finds out from `MERGED`.
#
# So the arguments are parsed rather than sniffed, and anything unrecognised
# stops the gate before it starts. The same judgement as the two environment
# variables validated below: an input this script does not understand is a
# refusal, not a default.
# The branch's own shape is checked BEFORE the flags, so `--dry-run mybranch`
# is told "the branch comes first" rather than "unrecognised argument
# 'mybranch'" — which is true, unhelpful, and names the wrong word.
case "$BRANCH" in
    -*) echo "REFUSED  '$BRANCH' looks like a flag, not a branch — the branch comes first"
        echo "         Usage: ./ci/handoff.sh <branch> [--dry-run]"
        exit 2 ;;
esac
shift 2>/dev/null || true
while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run) DRY=1 ;;
        *) echo "REFUSED  unrecognised argument '$1'"
           echo "         Usage: ./ci/handoff.sh <branch> [--dry-run]"
           echo "         The only flag is --dry-run, spelled exactly that way and given"
           echo "         AFTER the branch. It is refused rather than ignored because"
           echo "         ignoring it merges to main, which is the one outcome a mistyped"
           echo "         flag must not produce."
           exit 2 ;;
    esac
    shift
done
[ -n "$BRANCH" ] || { echo "REFUSED  no branch given"; exit 2; }

HOST="${RUSTMC_REMOTE:-threadripper}"
REMOTE_DIR="${RUSTMC_REMOTE_DIR:-rustmc-gate}"
REFS="${RUSTMC_REFS:-\$HOME/rustmc/oracle-work}"
SHARED_DATA="${RUSTMC_SHARED_DATA:-\$HOME/rustmc/data/vanilla}"
export RUSTMC_REMOTE="$HOST" RUSTMC_REMOTE_DIR="$REMOTE_DIR"

# The canonical checkout is somebody's working tree and is owned by whoever is
# in it. Gating inside it would rsync --delete over live work.
case "$REMOTE_DIR" in
    rustmc|rustmc/|"") echo "REFUSED  RUSTMC_REMOTE_DIR=$REMOTE_DIR is the canonical checkout, not a gate dir"; exit 2 ;;
esac

# --- THE GATE MUTEX ---------------------------------------------------------
#
# One gate per box directory, and one gate per laptop tree. Everything about
# why — the freshness hole, the invariant that starved it, the three concurrent
# pairs on 2026-08-06 and why there are two keys rather than one — is in
# `ci/gate-lock.sh`. The short version is that this script had no mutual
# exclusion of any kind: it refuses a dirty `$ROOT`, and two handoffs started
# ninety seconds apart both see a clean one.
#
# **It is taken here, before every other check, deliberately.** Every state
# check below reads `$ROOT`, and `release_worktree` and `git checkout` *write*
# to it and to other worktrees. A second gate racing this one would make even
# `git status --porcelain` a measurement of a tree somebody else is moving. The
# cost of holding the lock across the cheap refusals is milliseconds.
#
# It is also read from `$ROOT` **before** the branch is checked out, so the lock
# is the orchestrator's reviewed copy rather than the branch's — unlike every
# `./ci/...` below it, which is the LATENT hazard marked further down. A branch
# cannot opt out of being serialised.
#
# A missing library is a refusal, not a warning. "Gate unlocked because the file
# was not there" is the exact shape of guard this repo keeps finding switched
# off, and the file arrives in the same commit as this block.
[ -r "$ROOT/ci/gate-lock.sh" ] || { echo "REFUSED  ci/gate-lock.sh is missing — refusing to gate unlocked"; exit 2; }
. "$ROOT/ci/gate-lock.sh"
gate_lock_acquire "$HOST" "$REMOTE_DIR" "$ROOT" "$BRANCH" || exit 2
trap gate_lock_release EXIT
# The driver actually executing, captured before the checkout below replaces it
# with the branch's copy. If these two differ in the log header, the branch
# rewrote its own gate driver, and that is worth being able to see afterwards.
DRIVER_AT_ENTRY="$(git rev-parse --short "HEAD:ci/handoff.sh" 2>/dev/null || echo unknown)"

# Every remote stage, without exception, goes through here. `mkdir -p` before
# the `cd` because stage 1 is the first thing that touches the box and a gate
# directory that does not exist yet is the normal case for a new branch: this
# script was written against directories that already existed and died with
# `cd: no such file or directory` the first time it was pointed at a fresh one.
# The rsync in stage 2 would have created it, but stage 1 runs before it.
#
# `-n` is on because `remote` is called from places where ssh would otherwise
# eat the caller's stdin. That is also why stage 1 may **not** use it:
# `remote "bash -s" <<EOF` sends the heredoc to a process whose stdin is
# /dev/null, so the script arrives nowhere, `bash -s` reads EOF, and the stage
# exits **0** having done nothing at all. That is what it did on every run
# between being written and 2026-08-03 — the stage that exists to stop the gate
# reading another tree's data never ran once, and the loop reported success on
# a directory it had not prepared. Exactly the shape it was written to refuse.
# The one place an infrastructure failure is announced. It prints the cause,
# the log, and — the part that matters — a sentence saying the branch was not
# measured, so the reader does not go looking at a diff that has nothing to do
# with it. Exit 4, which no other outcome uses.
infra() {
    # `mark`/`stage_summary` are defined below, once `$LOG` exists — every
    # call site of `infra` is past that point, so these are real functions by
    # the time this runs. `infra` is the one exit path that can fire before
    # the normal post-stage-6 "end" mark exists, so it marks "end" itself:
    # without it, a stage-1 death would have marks on record and nothing that
    # reads them.
    if command -v stage_summary >/dev/null 2>&1; then
        # Two of this function's call sites run AFTER the normal post-stage-6
        # "end" mark, so only mark it here if nothing has yet — a second "end"
        # a moment after the first would show up as its own near-zero row.
        grep -q '^STAGE-TIME end ' "$LOG" 2>/dev/null || mark "end"
        stage_summary
    fi
    echo "INFRA    $BRANCH — $1"
    echo "         $LOG"
    echo "         NOT a verdict about the branch: no branch code was measured."
    echo "         Fix the infrastructure and re-run; the branch is unjudged."
    exit 4
}

# --- `UNPUSHED` (5): main and origin/main disagree. 2026-08-18. --------------
#
# The fifth verdict, and the only one that is not about the branch it names at
# all — it is about the queue. It exists because the state it reports is the
# one state this script can reach that damages every branch behind it.
#
# WHAT HAPPENED. On 2026-08-16 a gate went green, merged, and the push timed
# out. `ci/handoff.sh` printed
#
#     MERGED-NOT-PUSHED  push failed — main is ahead locally
#
# and exited 2. Three things were wrong with that line and all three cost
# something:
#
#   * **`grep -E '^MERGED'` matches it.** An anchored prefix match is the
#     obvious way to classify a verdict, and it named the wedge state as the
#     success state. Two of the 32 REDs in this loop's record arrived as
#     another branch's MERGED for exactly this reason.
#   * **it was not the first line of output.** `git push`'s own `fatal: Could
#     not read from remote repository.` is, so a consumer taking line one got
#     `fatal:`.
#   * **exit 2 is `REFUSED`,** documented in this file's own header as "local
#     state stopped the gate before it started". The gate had run all six
#     stages, passed, and merged.
#
# WHY IT IS ITS OWN VERDICT RATHER THAN AN `INFRA`. `INFRA` means the branch
# was not measured and is unjudged. This branch WAS measured and it PASSED —
# its merge is done and sitting in local `main`. Re-gating it would be wasted
# work and, worse, `AHEAD` would now be 0 and it would refuse as "nothing to
# merge". The reader's next action is not "re-run the gate", it is "push", and
# that is a different action from every other verdict here.
#
# WHY IT STOPS THE QUEUE RATHER THAN JUST THE BRANCH. `BEHIND` below is
# measured against **local** `main`. One unpushed merge therefore makes every
# other branch in the queue read `N behind main — not a fast-forward, rebase
# it`, exit 3, on a rebase none of them need. That is how one timed-out push
# became 37 branches reported as needing hands, and it is why this is checked
# at entry as well as reported at exit.
unpushed() {
    echo "UNPUSHED $BRANCH — $1"
    echo "         $LOG"
    echo "         The branch is GREEN and its merge is DONE in local main. This is not a"
    echo "         verdict about the branch and re-gating it will refuse as '0 ahead'."
    echo "         NOTHING ELSE CAN BE GATED until main and origin/main agree: every"
    echo "         branch's 'behind main' is measured against LOCAL main, so the whole"
    echo "         queue will report a rebase it does not need."
    echo "         The one thing to do:  git -C $ROOT push origin main"
    exit 5
}

# --- THE PUSH IS RETRIED, BECAUSE IT WAS NEVER RETRIED. 2026-08-18. ----------
#
# One `git push`, one attempt, and a network failure ended the queue for the
# day. Everything else in this loop that reaches the network is either retried
# or is a stage whose failure costs one branch; the push is the only single
# attempt whose failure costs all of them, and it was the only one with no
# second try.
#
# Three attempts with a widening gap. The numbers are small on purpose: this
# runs after a fifty-minute gate, so twenty-one seconds of patience is free,
# and a push that has failed three times over that spread is not a blip.
#
# It does NOT retry forever. A repository that has moved under us, a credential
# that has expired and a network that is down all look the same from here, and
# the second and third of those do not heal by waiting. Three attempts, then
# the verdict, which names the state precisely enough for the next run to
# finish the job.
#
# Each attempt's own stderr goes to the log rather than to the terminal. That
# is what stops `fatal: Could not read from remote repository.` from being the
# first line a consumer reads — the defect that made the 2026-08-16 verdict
# unnamable was not the word, it was that git spoke first.
# The gaps, and the one thing about them that is settable. `RUSTMC_PUSH_RETRY_GAPS=0`
# makes the three attempts happen back to back instead of 3 s and 8 s apart. It
# is there for `ci/handoff-merge-tail-falsify.sh`, which drives four
# failing-push cases and spent 33 s of a 64 s run asleep — and T0 runs that file
# on every gate.
#
# **It changes the length of the gaps and cannot change the NUMBER of attempts**,
# which is the property the falsifier is asserting. A knob that could reduce
# three attempts to one would let the test pass against a script that had
# stopped retrying, and this repo has a name for a check that cannot fail.
# Anything other than `0` is refused rather than interpreted, on the same
# argument as the two environment variables validated above it.
PUSH_RETRY_GAPS="3 8 0"
case "${RUSTMC_PUSH_RETRY_GAPS:-}" in
    "")  ;;
    0)   PUSH_RETRY_GAPS="0 0 0" ;;
    *)   echo "REFUSED  RUSTMC_PUSH_RETRY_GAPS='${RUSTMC_PUSH_RETRY_GAPS}' — the only accepted value is 0"
         echo "         It sets the WAIT between the three pushes to nothing, for tests that"
         echo "         drive the failing-push path. It cannot change the number of attempts."
         exit 2 ;;
esac
push_main_with_retry() {
    local what="$1" attempt=0 rc=0 delay
    for delay in $PUSH_RETRY_GAPS; do
        attempt=$((attempt + 1))
        if git push -q origin main 2>>"$LOG"; then
            [ "$attempt" -gt 1 ] && echo "handoff: push of $what succeeded on attempt $attempt" >>"$LOG"
            return 0
        fi
        rc=$?
        echo "handoff: push of $what failed on attempt $attempt (exit $rc)" >>"$LOG"
        [ "$delay" -gt 0 ] && sleep "$delay"
    done
    return 1
}

# --- THE PER-LANE JOB CAP, CARRIED ON THE COMMAND STRING. 2026-08-16. --------
#
# `ci/lanes.sh` runs N gates at once on one box. Neither `ci/t0.sh` nor
# `ci/t1.sh` passes `-j`, so cargo defaults to one job per hardware thread and
# every lane assumes all 24 of them; the cap exists so two lanes do not thrash.
# `ci/lanes.sh` sets `CARGO_BUILD_JOBS` in this script's environment, and
# nothing in this script's environment crosses the ssh boundary — ssh forwards
# only what an explicit `SendEnv`/`AcceptEnv` pair names and this box configures
# neither. So the value has to be spliced onto the command string, exactly as
# `RUSTMC_CORPUS_LOCK_ROLE=gate` is at stage 6.
#
# **IT USED TO TRAVEL IN THE TREE INSTEAD, AND THAT DESTROYED THE QUEUE.**
# `ci/lanes.sh` wrote `[build] jobs = N` into the lane's `.cargo/config.toml`
# before the rsync, so the value travelled with the checkout and no gate script
# had to know lanes existed. That file is **tracked**. Writing it left every
# lane worktree permanently modified, git refuses to rebase a dirty tree, and on
# 2026-08-16 the first drain dropped all 37 queued branches in 33 seconds — each
# one reported as `REBASE CONFLICT, needs hands` over a `lanes.log` reading
# `error: cannot rebase: You have unstaged changes.` It also **overwrote** the
# real contents of that file, which is where `target-cpu=x86-64-v2` is set, so
# every lane gate that had run would have measured a worldgen build without the
# `roundsd` the whole file exists for.
#
# The rule that generalises: a resource cap must not be carried in a tracked
# file. An environment variable spliced here is per-run, leaves no trace in the
# tree, and cannot fight the rebase.
#
# The value is validated before it reaches a remote shell. It is spliced as a
# standalone `export` statement rather than as a `VAR=x cmd` prefix because
# `remote()` is handed compound commands: a prefix binds to the first simple
# command only, so `CARGO_BUILD_JOBS=6 a && b` would leave `b` uncapped.
JOBS_ENV=""
case "${CARGO_BUILD_JOBS:-}" in
    "")            ;;
    0|*[!0-9]*)    echo "REFUSED  CARGO_BUILD_JOBS='${CARGO_BUILD_JOBS}' is not a positive integer"
                   echo "         A resource cap is spliced into a remote shell command; anything"
                   echo "         that is not a plain positive integer is a hole, not a setting."
                   exit 2 ;;
    *)             JOBS_ENV="export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}; " ;;
esac

# --- AN INPUT IS VALIDATED WHERE IT IS READ, NOT WHERE IT IS USED. 2026-08-18.
#
# This block used to sit at stage 6, immediately above the line that splices it
# onto the remote command. That reads as tidy — the validation next to the use —
# and it meant `RUSTMC_REFERENCE_CACHE=yes ./ci/handoff.sh <branch>` ran stage 1,
# the rsync, the table generation, T0 and T1 — the better part of an hour — and
# then refused on a typo in an environment variable that was wrong before the
# first ssh.
#
# It is the same class of splice as `CARGO_BUILD_JOBS` three lines above, with
# the same reason for validating rather than passing through (it is interpolated
# into a string handed to a remote shell), and it now sits in the same place.
# The rule: an argument this script rejects is rejected before it costs
# anything, because the operator who typed it is still watching.
REFCACHE_ENV=""
case "${RUSTMC_REFERENCE_CACHE:-}" in
    "")  ;;
    0|1) REFCACHE_ENV="RUSTMC_REFERENCE_CACHE=${RUSTMC_REFERENCE_CACHE} " ;;
    *)   echo "REFUSED  RUSTMC_REFERENCE_CACHE='${RUSTMC_REFERENCE_CACHE}' is not 0 or 1"
         echo "         Only 0 and 1 are values ci/reference-cache.sh distinguishes, and this"
         echo "         is spliced into a remote shell command; anything else is a hole, not"
         echo "         a setting. Refused before the gate starts rather than at stage 6,"
         echo "         which is where this check used to live and cost most of an hour."
         exit 2 ;;
esac

remote() { ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; ${JOBS_ENV}mkdir -p \$HOME/$REMOTE_DIR && cd \$HOME/$REMOTE_DIR && $*"; }
# Same, for a script on stdin. No `-n`, and the caller must supply the heredoc.
remote_script() { ssh "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; ${JOBS_ENV}mkdir -p \$HOME/$REMOTE_DIR && cd \$HOME/$REMOTE_DIR && bash -s"; }

# --- per-stage wall and CPU: one `GATE-STAGE` line per stage in $LOG ---------
#
# Until this, a gate log carried no clock at all: the train-a gate of
# 2026-09-30 took 2 h 27 min and where it went had to be rebuilt from a T1
# log's filename, the corpus lock's waiting counter and a hold-phases table.
#
#   GATE-STAGE stage=<n> name=<s> wall_s=<s> cpu_s=<s|na> lock_wait_s=<s|0> rc=<n>
#
# cpu_s is the REMOTE command's own CPU (user+sys of the remote shell and every
# child it reaped), which `remote_timed` appends to the log as a tagged line;
# stage 4 reads T0's own `T0-STAGES total_cpu_s=` instead. lock_wait_s is what
# the corpus lock printed as `ACQUIRED ... after waiting Ns` inside the stage,
# so wall minus wait is the stage's own cost. Nothing reads these lines to
# decide a verdict; they are measurement only.
remote_timed() {  # stage-number command...
    local n=$1; shift
    remote "$*; __rc=\$?; set -- \$(cat /proc/\$\$/stat); echo \"GATE-REMOTE-CPU stage=$n ticks=\$(( \${16} + \${17} )) hz=\$(getconf CLK_TCK)\"; exit \$__rc"
}
stage_clock() { _ST_W0=$(date +%s%N); _ST_OFF=$(stat -c %s "$LOG" 2>/dev/null || echo 0); }
stage_mark() {  # n name rc
    local w1 seg cpu wait tk hz
    w1=$(date +%s%N)
    seg=$(tail -c +"$(( ${_ST_OFF:-0} + 1 ))" "$LOG" 2>/dev/null)
    tk=$(printf '%s\n' "$seg" | sed -n "s/^GATE-REMOTE-CPU stage=$1 ticks=\([0-9]*\) hz=\([0-9]*\)\$/\1 \2/p" | tail -1)
    if [ -n "$tk" ]; then
        cpu=$(awk -v t="${tk% *}" -v h="${tk#* }" 'BEGIN{printf "%.2f", t/h}')
    else
        cpu=$(printf '%s\n' "$seg" | sed -n 's/^T0-STAGES .*total_cpu_s=\([0-9.]*\).*/\1/p' | tail -1)
    fi
    wait=$(printf '%s\n' "$seg" | sed -n 's/^corpus lock: ACQUIRED .* after waiting \([0-9]*\)s.*/\1/p' | awk '{s+=$1} END{print s+0}')
    printf 'GATE-STAGE stage=%s name=%s wall_s=%s cpu_s=%s lock_wait_s=%s rc=%s\n' \
        "$1" "$2" "$(awk -v a="$_ST_W0" -v b="$w1" 'BEGIN{printf "%.1f", (b-a)/1e9}')" \
        "${cpu:-na}" "$wait" "$3" >>"$LOG"
}

# --- releasing a branch from the worktree holding it ------------------------
#
# `git checkout` refuses a branch that is checked out in another worktree of
# the same repository, and this repo runs several of them: agents work in
# `git worktree` siblings of the canonical checkout so they do not rsync each
# other's half-written saves. So the two checkouts in this script — the branch
# under test below, and `main` at merge time — could both die on
#
#     fatal: 'structures-trial' is already used by worktree at '.../work-structures'
#     REFUSED  could not check out structures-trial
#
# and on 2026-08-06 that cost three gate launches in one day, with every
# remaining queued branch worktree-held, so it would have kept happening. That
# refusal says nothing whatever about the branch it names, which is the exact
# failure mode the top of this file exists to refuse. It was also a rule a
# human had to remember to apply — leave your worktree detached — and a rule
# like that is forgotten precisely when the queue is busy. The gate handles it.
#
# WHAT A DETACH ACTUALLY DOES, MEASURED RATHER THAN ASSUMED (2026-08-06).
# `git checkout --detach` rewrites HEAD to the commit that worktree is already
# sitting on and touches neither the index nor the working tree. Verified on a
# scratch repo, not reasoned about: a staged modification and an untracked file
# both survived it byte for byte, and the staged entry stayed staged. **A
# detach cannot lose a file**, so the clean case below is safe for a stated
# reason rather than by luck, and "the files are at risk" is NOT the argument
# for the dirty case.
#
# THE DIRTY CASE IS STILL A REFUSAL, and the real argument is better than the
# one about files. It is the same judgement as the `working tree dirty` check
# on ROOT: uncommitted changes mean an agent is probably still live in that
# tree, and a branch somebody is mid-edit on is not a branch to gate — the tip
# under test is not what is on disk where the work is happening, so a green
# would be a claim about neither. Detaching it would also leave that agent
# committing onto a detached HEAD without being told, and its next commits
# would land on no branch at all. So: loud REFUSED, the path, and the remedy.
#
# **REFUSED (exit 2), never RED (exit 1).** That is what keeps this
# distinguishable from a verdict about the branch: exit 1 is "gate red, the
# branch is bad", exit 2 is this script's documented "refused (state), the
# gate could not run". Nothing about the branch has been measured when this
# fires, and no stage has touched the box.
#
# If the LATENT note below is ever closed — gate from a throwaway
# `git worktree --detach` instead of checking out in the caller's tree — this
# function goes away with it, because nothing would be holding a branch.
RELEASED=""
release_worktree() {
    local want="$1" path="" wt="" line dirty n
    while IFS= read -r line; do
        if [ "${line#worktree }" != "$line" ]; then
            path="${line#worktree }"
        elif [ "$line" = "branch refs/heads/$want" ] && ! [ "$path" -ef "$ROOT" ]; then
            wt="$path"
        fi
    done < <(git worktree list --porcelain)
    [ -n "$wt" ] || return 0        # nobody holds it, or ROOT itself does

    # `-ef` compares device and inode, so a symlinked path cannot make ROOT
    # look like a different worktree and get itself detached. It is also false
    # for a path that does not exist, which is deliberate: a worktree whose
    # directory has been deleted still holds its branch — git calls it
    # `prunable`, not gone — and that is the one shape of this defect a detach
    # cannot fix, so it must reach the unreadable branch below rather than be
    # skipped here.
    #
    # A failing `git -C` must never read as clean. Assigning the output of a
    # failed status into $dirty would leave it empty, and empty means clean
    # three lines down — a silent misclassification in the one direction that
    # matters.
    if ! dirty="$(git -C "$wt" status --porcelain 2>&1)"; then
        echo "REFUSED  $want is held by the worktree at $wt, which cannot be read"
        echo "         git -C $wt status said: $dirty"
        echo "         if that worktree is gone, release the branch with:  git worktree prune"
        return 2
    fi

    if [ -n "$dirty" ]; then
        n=$(printf '%s\n' "$dirty" | wc -l)
        echo "REFUSED  $want is held by the worktree at $wt, and that worktree is dirty"
        echo "         $n uncommitted change(s) there — an agent may still be live in it:"
        printf '%s\n' "$dirty" | head -5 | while IFS= read -r line; do echo "             $line"; done
        echo "         The gate will not detach a tree an agent may still be live in. The"
        echo "         files would survive it — a detach only moves HEAD — but that agent's"
        echo "         next commits would land on a detached HEAD without it being told,"
        echo "         and a branch somebody is mid-edit on is not a branch to gate."
        echo "         Commit or stash that work, then release the branch:"
        echo "             git -C $wt status"
        echo "             git -C $wt checkout --detach"
        return 2
    fi

    git -C "$wt" checkout --detach -q || {
        echo "REFUSED  $want is held by the clean worktree at $wt and it would not detach"
        echo "         try by hand:  git -C $wt checkout --detach"
        return 2
    }
    RELEASED="released $want from the clean worktree at $wt — detached there at $(git -C "$wt" rev-parse --short HEAD), nothing removed"
    echo "handoff: $RELEASED"
    return 0
}

LOGDIR="$HOME/rustmc-private/gates"
mkdir -p "$LOGDIR" 2>/dev/null
STAMP="$(date +%Y%m%d-%H%M%S)"
# `$BRANCH` becomes a FILENAME here, not a directory component — a `/` in it
# must not be read as one. `scratch-check/*` is an established branch-naming
# convention in this repo (see docs/AGENT-BRIEF.md's own worktree examples),
# so `$BRANCH` containing a slash is not a hypothetical: `gate-$BRANCH-
# $STAMP.log` for `scratch-check/foo` resolves to `$LOGDIR/gate-scratch-check/
# foo-$STAMP.log`, a path through a directory (`gate-scratch-check/`) that was
# never created. Every `>>"$LOG"` from here on then fails with "No such file
# or directory" and the SHELL, not this script, decides what happens next —
# `set -uo pipefail` has no opinion about a failed redirect on an `echo`, so
# the script carries on with a `$LOG` that silently never receives another
# line, through every state-changing stage below (`release_worktree`, `git
# checkout -q "$BRANCH"`, the box-side rsync and gate). Found 2026-08-17
# queued behind a live gate as `scratch-check/refcache-bothways`: caught before
# it fired, but a mid-gate crash here is exactly the shape that leaves a
# checked-out branch and a dirty tree behind for the NEXT handoff to refuse on
# — the same failure mode `lanes-canonical-guard` closes for `ci/lanes.sh`.
# `//` replaces every occurrence, not just the first, so a branch two levels
# deep (`a/b/c`) still resolves to one flat filename rather than reintroducing
# the bug at the second slash.
LOG="$LOGDIR/gate-${BRANCH//\//-}-$STAMP.log"

# --- THE VERDICT IS READ OUT OF THIS FILE, SO A LOG THAT CANNOT BE WRITTEN ---
# --- IS AN INFRASTRUCTURE FAILURE AND NOT A RED. 2026-08-18. ----------------
#
# `mkdir -p "$LOGDIR"` was unchecked, and its failure is silent in the worst
# possible direction. Every stage still runs and every exit code is still
# captured — but `$T0_GREEN`, `$T1_GREEN`, `$T2_GREEN` and `$SKIPS` are
# `grep -c` over `$LOG`, and a grep of a file that does not exist counts 0. So a
# gate whose six stages all passed reaches
#
#     RED  <branch> — sync=0 tables=0 t0=0/0 t1=0/0 t2=0/0 no-vanilla-skips=0
#
# with every exit code zero, no sentence naming a cause, and the red extract
# below grepping the same missing file for the explanation. A perfectly healthy
# branch, marked bad, because `$HOME/rustmc-private` was a symlink to somewhere
# that had gone away.
#
# That is precisely the class `INFRA` was created for and it is the only place
# in this file where the *instrument* rather than a stage is what failed. It is
# checked here, before anything is run, so nobody pays for a gate whose result
# cannot be recorded — and asserted again at verdict time, because the file can
# be removed while the gate is running.
if ! : >>"$LOG" 2>/dev/null; then
    echo "INFRA    $BRANCH — the gate log cannot be written: $LOG"
    echo "         Every verdict below is a grep over that file, and a grep of a file"
    echo "         that is not there counts zero — so a gate that passed every stage"
    echo "         would report RED with all its exit codes zero and no cause named."
    echo "         NOT a verdict about the branch: nothing has been measured."
    echo "         Check that $LOGDIR exists and is writable."
    exit 4
fi

# A dropped cap is silent in the direction that matters: an uncapped lane gate
# is not an error, it is just slower and it thrashes the box with its neighbour,
# which reads as "the box is busy" rather than as "the flag did not arrive".
# One line naming the value THIS run will carry separates the two.
echo "job cap: ${JOBS_ENV:-CARGO_BUILD_JOBS unset — this gate assumes the whole box}" >>"$LOG"

# --- state checks, before anything is touched -------------------------------
git rev-parse --verify -q "$BRANCH" >/dev/null || { echo "REFUSED  no such branch: $BRANCH"; exit 2; }
# --- "AN AGENT MAY STILL BE LIVE" IS A GUESS, AND IT WAS WRONG. 2026-08-18. --
#
# The condition is right and the diagnosis was not. On 2026-08-18 a branch was
# refused here because somebody had created a scratch **git worktree inside the
# gate's own checkout**. `git status` reports that directory as untracked, the
# refusal fires correctly, and the message sends the reader looking for
# half-written source that does not exist — the cause was two directories away
# from anything this script controls, and "commit or stash it" is the wrong
# remedy for it. The same shape cost 861 refusals on 2026-08-17 when a lane left
# this checkout dirty.
#
# So the refusal now shows WHAT is dirty, and separates the one cause that is
# not edited work at all: an untracked directory carrying its own `.git` is a
# nested worktree or clone, and the remedy is to move it out, not to commit it.
DIRTY="$(git status --porcelain)"
if [ -n "$DIRTY" ]; then
    echo "REFUSED  working tree dirty — the gate will not measure a tree somebody is editing"
    printf '%s\n' "$DIRTY" | head -5 | while IFS= read -r l; do echo "             $l"; done
    NESTED=""
    while IFS= read -r l; do
        case "$l" in
            '??'*) d="${l#?? }"; d="${d%/}"
                   [ -d "$ROOT/$d" ] && [ -e "$ROOT/$d/.git" ] && NESTED="$NESTED $d" ;;
        esac
    done <<< "$DIRTY"
    if [ -n "$NESTED" ]; then
        echo "         NOT edited work:$NESTED carries its own .git — it is a nested"
        echo "         worktree or clone inside this checkout. Committing or stashing will not"
        echo "         help; move it out. \`git worktree list\` will name it if it is a worktree."
    else
        echo "         An agent may still be live here. Commit or stash, then re-run."
    fi
    exit 2
fi

# --- IS THE QUEUE ALREADY WEDGED? Checked before BEHIND is computed. --------
#
# `BEHIND` two lines down is measured against LOCAL `main`, and that is the
# correct thing for it to measure — a fast-forward merge is a local operation.
# But it means a local `main` that origin has never seen is indistinguishable,
# from down there, from a branch that genuinely needs rebasing. The branch gets
# told to rebase, the rebase does not help, and the next branch is told the
# same. One unpushed merge, the entire queue.
#
# So the disagreement is named here, once, by the run that finds it, instead of
# being re-derived as a wrong answer by every run after it.
#
# THE REPAIR IS ATTEMPTED, AND THAT NEEDS ITS OWN ARGUMENT because this pushes
# during another branch's gate. It is defensible for a reason that is worth
# stating rather than assuming: **`git push origin main` sends all of `main`
# regardless.** The very next gate that goes green would push these commits
# anyway, as a side effect of pushing its own. Pushing them here changes only
# WHEN they leave, never WHAT leaves, and it is the difference between a queue
# that drains and a queue that reports 37 spurious rebases. It is attempted
# only when `origin/main` is a strict ancestor of `main` — a real divergence is
# not something this script may resolve, and it says so and stops.
#
# A repo with no `origin/main` at all is not a wedge, it is a fixture: the
# verdict selftest builds a throwaway repo with no remote. Absent ref, no check.
if git rev-parse --verify -q origin/main >/dev/null 2>&1; then
    # Best effort, and short. `origin/main` is a cache and a stale one reads
    # as a wedge that is not there; a fetch that fails leaves the cache in
    # place and is recorded rather than fatal, because refusing to gate
    # because github was slow is the failure this whole verdict exists about.
    if ! timeout 60 git fetch -q origin main 2>>"$LOG"; then
        echo "handoff: could not fetch origin/main — the check below uses the cached ref" >>"$LOG"
    fi
    ORIGIN_MAIN=$(git rev-parse origin/main)
    LOCAL_MAIN=$(git rev-parse main)
    # THE THREE WAYS THEY CAN DIFFER, AND THEY WANT THREE DIFFERENT ANSWERS.
    #
    #   local main is an ANCESTOR of origin/main — it is simply behind, the
    #     normal state of any checkout somebody else has pushed past. Handled
    #     first, below, and it is NOT a divergence. This clause was missing
    #     when the check was written: local `main` in the real checkout was 40
    #     commits behind `origin/main` at the time, so the first version would
    #     have called every gate a divergence on its first real run.
    #   origin/main is an ancestor of local main — merged and not pushed. The
    #     wedge. Repairable by pushing, and repaired.
    #   neither — both moved. Not something a gate script may resolve.
    if [ "$ORIGIN_MAIN" != "$LOCAL_MAIN" ] && git merge-base --is-ancestor "$LOCAL_MAIN" "$ORIGIN_MAIN"; then
        # --- LOCAL main IS BEHIND origin/main -------------------------------
        #
        # Left alone this costs a whole gate and then cannot be paid for. The
        # branch is measured against the stale `main`, `BEHIND` is 0 because
        # `BEHIND` is local, the gate goes green, the ff-merge succeeds, and the
        # push is rejected as a non-fast-forward — fifty minutes to arrive at an
        # unpushable merge and a wedged local main. It has to be caught here.
        #
        # The fast-forward is safe for a stated reason rather than by
        # assumption: local `main` is a strict ANCESTOR of `origin/main`, so
        # there is nothing on it that origin lacks and nothing can be lost.
        # That is the same argument as pushing an unpushed main — it changes
        # when the checkout catches up, never what is in the history.
        #
        # Under `--dry-run` nothing is moved: `ci/lanes.sh` builds each lane's
        # `main` from `origin/main` itself and handles base movement with its
        # own VOID path, and a dry run may not write to the caller's tree.
        BEHIND_ORIGIN=$(git rev-list --count main..origin/main)
        echo "handoff: local main is $BEHIND_ORIGIN commit(s) BEHIND origin/main — somebody else has landed" >>"$LOG"
        if [ -n "$DRY" ]; then
            echo "handoff: --dry-run, so main is left where it is; the caller owns its base" >>"$LOG"
        elif [ "$(git symbolic-ref -q --short HEAD)" = "main" ]; then
            git merge --ff-only origin/main -q 2>>"$LOG" \
                && echo "handoff: fast-forwarded local main to $(git rev-parse --short main)" \
                || { echo "REFUSED  local main is $BEHIND_ORIGIN behind origin/main and would not fast-forward"
                     echo "         $LOG"
                     echo "         Gating against it would spend the whole gate and then fail to push."
                     exit 2; }
        elif ! git worktree list --porcelain | grep -c '^branch refs/heads/main$' >/dev/null; then
            git branch -qf main origin/main \
                && echo "handoff: fast-forwarded local main to $(git rev-parse --short main)"
        else
            echo "NOT-FF   local main is $BEHIND_ORIGIN commit(s) behind origin/main and is checked out elsewhere"
            echo "         $LOG"
            echo "         Gating $BRANCH against it would spend the whole gate and then fail to"
            echo "         push, because origin/main has moved on. Update main first:"
            echo "             git fetch origin && git branch -f main origin/main"
            exit 3
        fi
        LOCAL_MAIN=$(git rev-parse main)
    fi
    if [ "$ORIGIN_MAIN" != "$LOCAL_MAIN" ]; then
        if ! git merge-base --is-ancestor "$ORIGIN_MAIN" "$LOCAL_MAIN"; then
            # main and origin/main have both moved. Nothing here may pick a
            # winner: one side is somebody's merge and the other side is
            # somebody else's.
            echo "handoff: main and origin/main have DIVERGED" >>"$LOG"
            [ -n "$DRY" ] && echo "handoff: --dry-run, so the divergence is reported and not acted on" >>"$LOG"
            [ -n "$DRY" ] || unpushed "main and origin/main have diverged — local main is not a fast-forward of origin/main, so this cannot be repaired by pushing"
        fi
        WEDGE_N=$(git rev-list --count origin/main..main)
        echo "handoff: local main is $WEDGE_N commit(s) ahead of origin/main at entry — a previous gate merged and did not push" >>"$LOG"
        git log --oneline origin/main..main >>"$LOG" 2>&1
        # --- A DRY RUN MUST NOT PUSH. ---------------------------------------
        #
        # `--dry-run` is documented as gating without merging, and `ci/lanes.sh`
        # drives every one of its lanes that way — it does its own merge under
        # its own sequencer lock, with its own `origin/main` freshness check
        # (the `VOID` path), in a lane clone whose `origin` is the real remote.
        # A repair fired from inside a dry run would push `main` from a process
        # whose entire contract is that it changes nothing, and it would do it
        # underneath a driver that is already managing that exact question.
        #
        # So under `--dry-run` the disagreement is recorded and the gate carries
        # on. It is not a verdict either: a lane's local `main` is legitimately
        # ahead of `origin/main` for the moments between one lane merging and
        # pushing, and refusing there would turn a normal drain into a queue of
        # refusals — the precise failure this check was written to end.
        if [ -n "$DRY" ]; then
            echo "handoff: --dry-run, so the repair is NOT attempted; the caller owns the merge" >>"$LOG"
            echo "handoff: local main is $WEDGE_N ahead of origin/main (dry run — not pushed, not a verdict)"
        else
        echo "handoff: pushing it now; every branch behind it is being told to rebase onto a main origin has never seen"
        if push_main_with_retry "the unpushed merge found at entry"; then
            echo "handoff: origin/main repaired to $(git rev-parse --short main) — carrying on with $BRANCH"
        else
            unpushed "local main was already $WEDGE_N commit(s) ahead of origin/main when this gate started, and the push still fails"
        fi
        fi
    fi
fi

AHEAD=$(git rev-list --count "main..$BRANCH")
BEHIND=$(git rev-list --count "$BRANCH..main")

# --- WHY `AHEAD` IS A `rev-list --count` AND MUST STAY ONE. 2026-08-18. -----
#
# The standing rule in this repo is to size a branch with `git cherry` and never
# with `git rev-list --count`, and the rule is right: `ci/unlanded.sh` measured
# it repo-wide the same day and the naive count over 218 branches is 780 against
# a real 453, because this repo merges by rebase and a rebased commit carries an
# old patch under a new sha. That overstatement has already misled a lane and
# the orchestrator.
#
# **It does not apply to this line, and the proof is one line above it.**
# `git cherry main $BRANCH` marks a commit `-` when its patch-id also appears in
# `$BRANCH..main`. The check immediately above refuses unless `BEHIND` is 0, and
# `BEHIND` is exactly `git rev-list --count "$BRANCH..main"` — so at the point
# `AHEAD` is used, `$BRANCH..main` is EMPTY, there is nothing for a patch-id to
# match against, and `git cherry` cannot mark anything `-`. The two instruments
# are provably equal here.
#
# This was written the other way round first, with the cherry count printed
# beside the sha count, and the falsifier case built to force a disagreement
# could not force one. That is the useful outcome: a general rule met the one
# place it does not bind, and the reason is now written where somebody would
# otherwise re-derive it. `UNPUSHED` below is the figure in this file where the
# rule DOES bind, and it is converted.
AHEAD_TEXT="+$AHEAD commits"
# --- 0 AHEAD HAS TWO CAUSES AND THEY ARE OPPOSITE. 2026-08-18. --------------
#
# `0 commits ahead of main — nothing to merge` is true of a branch that has
# nothing on it, and equally true of a branch whose work is **already in main**.
# The first needs hands. The second is finished, and telling a queue driver it
# was `REFUSED` makes it hold a branch that has already landed.
#
# This is not hypothetical and it is not rare: it is what the re-run after an
# interrupted merge hits. A gate killed between `git merge --ff-only` and
# `git push` leaves the branch merged into local `main` and prints nothing at
# all, because SIGKILL runs no trap. The only thing that can notice is the next
# run, and the next run said `REFUSED  0 commits ahead of main — nothing to
# merge`, which is accurate, useless, and the wrong instruction.
#
# `git merge-base --is-ancestor` separates them exactly. `ALREADY-MERGED`, exit
# 0, because the state the caller asked for is the state that exists — this is
# idempotence, not success, and the line says which. The word is distinct from
# `MERGED` rather than a special case of it so that "this run merged it" and
# "somebody already had" remain two different claims in the record, and it is
# not a prefix of any other verdict.
#
# AND THE TWO CAUSES ARE NOT TWO — THEY ARE ONE, WHICH IS WHY IT WAS WRONG.
# `git rev-list --count main..$BRANCH` counts commits reachable from `$BRANCH`
# and not from `main`, so **`AHEAD` is 0 if and only if `$BRANCH` is an ancestor
# of `main`.** There is no such thing here as a branch that is 0 ahead and not
# already merged: an empty branch created off `main` is an ancestor of it, and a
# branch with work on it that main lacks has `AHEAD > 0` by definition. An
# orphan branch, which is the shape that intuitively ought to be "0 ahead and
# unrelated", counts its own root as ahead and lands on the `behind main`
# refusal below instead.
#
# So the old message was not merely unhelpful in one of two cases. It was wrong
# in the ONLY case that can reach it, every time, for the whole life of the
# script.
#
# The ancestor test below is therefore an assertion rather than a branch. It is
# kept, and its failure is a refusal rather than a silent fall-through, because
# an `AHEAD` of 0 arrived at any other way — a failed `git rev-list` leaving the
# variable empty, `$BRANCH` naming something that is not a commit — means the
# arithmetic this whole block rests on did not happen, and inventing a verdict
# from a number that was never computed is the defect this file exists to refuse.
if [ "$AHEAD" -eq 0 ] 2>/dev/null; then
    if ! git merge-base --is-ancestor "$BRANCH" main 2>/dev/null; then
        echo "REFUSED  $BRANCH counts 0 commits ahead of main but is not an ancestor of it"
        echo "         Those two cannot both be true. The count did not come from the"
        echo "         history it claims to describe — check that main and $BRANCH both"
        echo "         resolve to commits. No verdict is being issued from it."
        exit 2
    fi
    echo "ALREADY-MERGED  $BRANCH is already in main at $(git rev-parse --short "$BRANCH") — nothing left to do"
    echo "         Its commits are ancestors of main, so this is an idempotent re-run,"
    echo "         not a refusal. A gate interrupted between the merge and the push"
    echo "         leaves exactly this state and prints nothing, so the branch may have"
    echo "         been merged by a run that was killed."
    exit 0
fi
# --- EXIT 3 IS NOT A `REFUSED`, AND SAYING SO WAS COSTING A DISTINCTION -----
#
# This file's header has documented five outcomes for a long time and gave exit
# 3 its own row — `not-ff (3) the branch is behind main` — while both of the
# lines that take it printed the word `REFUSED`. So the word and the code
# disagreed: `REFUSED` is documented, four times, as "local state stopped the
# gate before it started", and one of the two exit-3 sites fires AFTER a full
# green gate has been paid for.
#
# A reader who classifies by the word cannot tell "rebase this branch" from
# "fix your tree", and those are different jobs for different people. The word
# is now `NOT-FF`, it is prefix-free against everything else in the vocabulary,
# and `ci/checks/handoff-terminals.toml` will not let it drift from exit 3
# again.
[ "$BEHIND" -eq 0 ] || { echo "NOT-FF   $BRANCH is $BEHIND behind main — not a fast-forward, rebase it"; exit 3; }

# --- AND THIS IS THE FIGURE WHERE THE RULE DOES BIND ------------------------
#
# `unpushed-on-branch-at-entry` compares the local branch against
# `origin/$BRANCH`, and unlike `main..$BRANCH` above there is no fast-forward
# guarantee between them: a branch that was rebased and force-pushed, which is
# the normal shape of work here, has commits on both sides carrying the same
# patches under different shas. `git rev-list --count` reports every one of them
# as unpushed work. `git cherry` marks them `-`.
#
# A branch that has never been pushed has no `origin/$BRANCH` to compare with,
# and `?` is the honest answer for that rather than a 0 that reads as "all
# pushed" or an $AHEAD that reads as "none of it".
if git rev-parse --verify -q "origin/$BRANCH" >/dev/null 2>&1; then
    UNPUSHED=$(git cherry "origin/$BRANCH" "$BRANCH" 2>/dev/null | grep -c '^+')
    UNPUSHED="${UNPUSHED:-?}"
else
    UNPUSHED="?"
fi
# LATENT, MARKED 2026-08-06, NOT FIXED HERE. **After this line the branch under
# test supplies this gate's own laptop-side driver.** Every `./ci/...` the
# stages below invoke is read from disk *after* this checkout, so
# `ci/t0-remote.sh` — which decides what gets shipped and what gets excluded —
# is the branch's copy, not the copy the orchestrator reviewed. A branch can
# therefore alter how it is gated. The box-side scripts (gen-gameplay-tables,
# t1-gate, t2-gameplay) coming from the branch is correct and intended; they
# must match the source under test. The laptop-side driver is a different case
# and nothing here distinguishes them.
#
# This script's own body survives the swap only because git replaces a file by
# creating a new inode, so the running bash keeps reading the old one — verified
# by experiment on 2026-08-06, not assumed. That is a filesystem implementation
# detail holding up a gate's integrity, which is not a thing to rely on.
#
# The fix is to stop checking out in the caller's tree at all: rsync from a
# throwaway `git worktree --detach` at $BRANCH, leave ROOT alone, and the
# driver is then always the reviewed copy. That is a bigger change than a stage
# reorder and belongs to its own item.
#
# The release goes here, last of the state checks and not earlier, so a branch
# that fails a cheap check above — 0 ahead, behind main, no such branch — never
# causes somebody else's worktree to be detached for a gate that was never
# going to run.
release_worktree "$BRANCH" || exit 2
git checkout -q "$BRANCH" || { echo "REFUSED  could not check out $BRANCH"; exit 2; }
# --- AND THE EXIT CODE OF A CHECKOUT IS NOT EVIDENCE THAT IT HAPPENED. -------
#
# Measured 2026-08-18, not assumed. With a directory the process cannot write
# into, `git checkout <branch>` prints
#
#     error: unable to create file sub/b.txt: Permission denied
#     Switched to branch 'br'
#
# and **exits 0.** It moves HEAD, reports the failure on stderr, and leaves the
# file missing from the working tree. So the `|| REFUSED` above is a guard that
# cannot fire in the failure mode it most plausibly guards against — a full
# disk, a permission fault, an NFS glitch — and the gate would go on to rsync a
# tree that is missing a file the branch adds and gate the wrong content
# entirely. A green from that is a green about a tree that never existed in the
# history.
#
# The post-condition is what makes it decidable, and it is two facts rather than
# an exit code: HEAD is the branch tip, and the tree is clean against it. The
# second is the one that catches this — a file git could not write shows as
# `D` in `git status --porcelain`, which is exactly the state the check above
# already refuses to START a gate in.
if [ "$(git rev-parse HEAD)" != "$(git rev-parse "$BRANCH")" ] || [ -n "$(git status --porcelain)" ]; then
    echo "REFUSED  the checkout of $BRANCH did not complete, and git still exited 0"
    echo "         HEAD is $(git rev-parse --short HEAD), $BRANCH is $(git rev-parse --short "$BRANCH")"
    git status --porcelain | head -5 | while IFS= read -r l; do echo "             $l"; done
    echo "         git reports a failed checkout on stderr and exits 0 — measured, not"
    echo "         assumed. Gating this tree would measure content that is in no commit."
    exit 2
fi
TIP=$(git rev-parse HEAD)

{
    echo "handoff: branch=$BRANCH tip=$TIP host=$HOST gate-dir=~/$REMOTE_DIR refs=$REFS"
    echo "handoff: $(date -Is)"
    # The log is opened after the state checks, so a release that happened
    # before it existed has to be carried in. A gate whose branch arrived by
    # detaching another worktree must say so where the gate is read.
    [ -n "$RELEASED" ] && echo "handoff: $RELEASED"
} > "$LOG"

# --- PROVENANCE, so a future forensic question is answerable from the log ----
#
# The gate forensics of 2026-08-07 ran into two walls, and both are this file's
# doing. **1,107 of 1,479 commits — 75% of the history — were merged by a loop
# that recorded neither the gate directory nor the tip**, so no question about
# them can ever be settled. And every cargo call in T0/T1/T2 is `-q`, so **no
# gate log in this project has ever contained a `Compiling` or a `Finished`
# line** — measured, 0 across all 108 files — which means the T0 transcript of a
# full workspace build is byte-identical to the transcript of a build that
# compiled nothing. A stale binary re-runs its own cached tests and prints the
# same counts. The log could not answer the question it was being asked.
#
# The header now carries what an instrument would otherwise have to reconstruct:
#
#   tree=        `$ROOT`'s tree hash. The tip alone is not enough — two runs of
#                the same tip have the same tree, and a tree hash is what a
#                later reader compares against a box directory's contents.
#   root=/driver= WHICH laptop tree fed this box directory, and from which
#                machine. Two trees sharing one box directory is one of the two
#                ways the freshness hole can fire; before this, nothing recorded
#                it, and the forensics had to infer it from run ordering.
#   driver=      the blob of `ci/handoff.sh` actually running, and the blob the
#                branch supplies. A branch can rewrite its own gate driver
#                (marked LATENT above); if it did, these differ, and that is now
#                visible in the artefact rather than only in the diff.
#   lock-box=/lock-tree= the two mutex keys and the anchor pid, so "were these
#                two gates concurrent, and did they share a resource" is a
#                comparison between two log headers rather than an archaeology
#                of `target/` mtimes.
#
# The build evidence is not here — it cannot be, nothing has been built yet. It
# is one line printed after the last build stage; see `compile-census` below.
ROOT_TREE="$(git rev-parse "HEAD^{tree}")"
STAMP_KEY="$(printf '%s' "$REMOTE_DIR" | tr -c 'A-Za-z0-9._-' '_')"
{
    echo "handoff: tree=$ROOT_TREE root=$ROOT driver-host=$(hostname) pid=$$"
    echo "handoff: driver=$DRIVER_AT_ENTRY branch-driver=$(git rev-parse --short "HEAD:ci/handoff.sh" 2>/dev/null || echo unknown)"
    echo "handoff: $(gate_lock_header)"
} >> "$LOG"

# --- STAGE TIMING. The gate could not measure itself: 248 logs, no wall-clock
# anywhere except the corpus-lock waiter heartbeat, so every cost figure this
# project has published (~87 min split, the 38.4 min lock figure) was
# reconstructed after the fact from chain-log diffs and another script's
# output. That is the same defect class as a check that cannot go red: an
# instrument that cannot report its own cost.
#
# `mark <label>` writes one `STAGE-TIME` line — label, epoch, ISO-8601 — and
# is called at every stage boundary below, on the skip path as much as the run
# path, so a gate that dies at stage 1 still has a stage-1 duration on record.
# A row's wall time is the gap between ITS mark and the next one, which is why
# marks are placed at the START of a stage rather than its end: the interval
# from `mark 1-prepare` to `mark 2-rsync` IS stage 1's wall time, whichever way
# stage 1 went. Consecutive marks are read back out of `$LOG` by
# `stage_summary`, never recomputed, so the summary's parts sum to the total
# by construction — integer subtraction over the same numbers, not a second
# measurement that could disagree with the first.
#
# `date +%s` (whole seconds) rather than sub-second: every stage here is
# seconds to tens of minutes, and a coarser clock is one fewer thing for the
# selftest to fake convincingly.
mark() {
    local label="$1" now
    now="$(date +%s)"
    echo "STAGE-TIME $label $now $(date -Is)" >>"$LOG"
}
# `stage_summary`: read every `STAGE-TIME` line back out of `$LOG`, in the
# order they were written — which is the order the gate reached them, not a
# fixed list of six — and print one row per interval plus a TOTAL row. Called
# at every exit path below (`infra`, RED, REFUSED-after-the-lock, dry-run
# GREEN, MERGED) so a REFUSED or RED gate reports a time exactly as a green
# one does. It does not mark "end" itself — the normal path marks it once,
# right after stage 6 finishes, so the post-stage-6 bookkeeping (the compile
# census, the lock re-assert) is not silently folded into stage 6's number.
# `infra` is the one path that can fire before that mark exists at all, which
# is why it marks "end" itself before calling this.
stage_summary() {
    {
        echo "== gate timing summary =="
        awk '
            # `BEGIN { n = 0 }` is load-bearing, not defensive padding. An
            # uninitialised awk scalar used as an array subscript is keyed by
            # its STRING value, which for an unset variable is "" — not "0" —
            # so `lbl[n]` on the very first match stores under lbl[""] while
            # every read in END loops from `lbl[0]`. Found by running this
            # against a fixture where every mark landed in the same second:
            # ep[0] read back empty, `ep[n-1] - ep[0]` became `ep[n-1] - 0`,
            # and the printed TOTAL was a raw Unix timestamp, not a duration.
            BEGIN { n = 0 }
            /^STAGE-TIME / { lbl[n]=$2; ep[n]=$3; n++ }
            END {
                if (n < 2) { print "  (fewer than two marks — nothing timed)"; exit }
                total = ep[n-1] - ep[0]
                printf "  %-16s %8s  %6s\n", "stage", "wall_s", "pct"
                for (i = 0; i < n - 1; i++) {
                    d = ep[i+1] - ep[i]
                    pct = (total > 0) ? (d * 100.0 / total) : 0
                    printf "  %-16s %8d  %5.1f%%\n", lbl[i], d, pct
                }
                printf "  %-16s %8d  %5.1f%%\n", "TOTAL", total, 100.0
            }
        ' "$LOG"
    } >>"$LOG"
}
mark "start"

# --- stage 1: the gate directory owns its own oracle-work and data/vanilla --
#
# Both were symlinked into the canonical checkout on 2026-08-03 at 10:53, and
# eight trees on that box still are. The distinction the convention misses:
#
#   * the **vanilla worldgen references** under `oracle-work/` are large,
#     expensive, twinned and read-only. Sharing them is right, and this links
#     them in one by one.
#   * `oracle-work/gameplay/` is live per-run output — the recordings, the
#     running vanilla world, the report. Two trees sharing it overwrite each
#     other mid-run.
#   * `data/vanilla/` is **branch-dependent generated data**. Sharing it means
#     a branch that adds a table gates without it and a branch that rewrites a
#     dumper gates against the old output. That is exactly what produced the
#     12/55 on placed-by-and-ticks: `set_placed_by.json` absent and
#     `use_without_item.json` 111 KB stale, because both resolved into a
#     checkout sitting on main.
#
# So the gate directory gets its own copy of `data/vanilla` — seeded from the
# shared one so the version-pinned jar extracts are not re-dumped, then made
# current for THIS branch by stage 3. `cp -rn` seeds without clobbering; the
# staleness that leaves is what the provenance stamps refuse.
#
# Nothing here writes into $REFS or into the shared data directory.
# The verdict on a stage-1 failure is INFRA, never RED, and the reason is
# structural rather than a judgement call: the heredoc below is written in this
# file and touches $REFS, oracle-work and data/vanilla. The branch has not been
# shipped yet — that is stage 2 — so there is no branch code on the box for
# this stage to fail on. A failure here is an unreachable host, a missing
# reference directory, a missing seed directory, or a full disk. `RED` here
# sent readers to bisect a branch that had not been run.
mark "1-prepare"
STAGE1_RC=0
remote_script >>"$LOG" 2>&1 <<REMOTE_EOF || STAGE1_RC=$?
set -euo pipefail
echo "== stage 1: gate dir owns its own oracle-work and data/vanilla"
if [ -L oracle-work ]; then
    echo "   oracle-work was a symlink to \$(readlink oracle-work) — removing the LINK only"
    rm oracle-work
fi
mkdir -p oracle-work/gameplay data
for e in "$REFS"/*; do
    b="\$(basename "\$e")"
    # server-failures is this gate dir's own preserved evidence from a failed
    # vanilla boot (ci/vanilla-server.sh preserve). Like gameplay, it is
    # per-tree output, not a shared read-only reference -- linking it in would
    # point the gate's evidence at another tree's.
    #
    # NO BACKTICKS ANYWHERE IN THIS HEREDOC. It is <<REMOTE_EOF, unquoted, so
    # the laptop expands it before it is sent: a pair of backticks around a
    # word in a *comment* is a command substitution, and this exact comment
    # printed "server-failures: command not found" on 2026-08-06 before the
    # quoting was fixed. Same reason every $ below is written \$.
    case "\$b" in gameplay|gameplay-selfcheck|server-failures) continue;; esac
    ln -sfn "\$e" "oracle-work/\$b"
done
echo "   \$(find oracle-work -maxdepth 1 -type l | wc -l) references linked, gameplay is a \$(stat -c %F oracle-work/gameplay)"

SHARED=""
if [ -L data/vanilla ]; then
    SHARED="\$(readlink -f data/vanilla)"
    echo "   data/vanilla was a symlink to \$SHARED — removing the LINK only and copying"
    rm data/vanilla
elif [ ! -e data/vanilla ] || [ -z "\$(ls -A data/vanilla 2>/dev/null)" ]; then
    # An **empty** data/vanilla is neither a symlink nor absent, so before
    # 2026-08-06 it matched no branch here and was seeded with nothing. The
    # gate then went red in T0 with four worldgen-assembly panics reading
    # "no block-props extract, so no structure block states" -- a red that says
    # nothing about the branch, which is the failure mode this whole script
    # exists to refuse. Anyone who pre-creates the gate dir by hand lands here.
    # A gate directory that has never been used has no data/vanilla at all, and
    # the branch above never fires. Most of that directory is NOT generated by
    # this tree -- the worldgen tables (biome_*, configured_features, gaussian,
    # ...) are jar extracts pinned to the version, shared and read-only, and
    # regenerating them here would cost minutes and could not differ. Only the
    # tables crates/rustmc-server opens are branch-dependent, and stage 3
    # regenerates those into this directory under this tree's own stamps.
    # The count is deliberately not written here: it said "eight" while the
    # derived list stood at twelve, and a number in a comment is a number
    # nobody re-checks. ci/gen-gameplay-tables.sh derives and prints it.
    SHARED="$SHARED_DATA"
    echo "   data/vanilla absent — seeding from \$SHARED"
fi
if [ -n "\$SHARED" ]; then
    [ -d "\$SHARED" ] || { echo "   no shared data at \$SHARED to seed from" >&2; exit 1; }
    mkdir -p data/vanilla
    cp -rn "\$SHARED"/. data/vanilla/ 2>/dev/null || true
    rm -rf data/vanilla/.provenance   # stamps, if any, belonged to \$SHARED
fi
[ -d data/vanilla ] || mkdir -p data/vanilla
echo "   data/vanilla is a \$(stat -c %F data/vanilla) with \$(ls data/vanilla | wc -l) entries"
echo "STAGE1 OK"
REMOTE_EOF

if [ "$STAGE1_RC" -ne 0 ]; then
    if [ "$STAGE1_RC" -eq 255 ]; then
        infra "stage 1 could not reach $HOST — ssh exited 255, so nothing ran on the box"
    fi
    infra "stage 1 could not prepare ~/$REMOTE_DIR (exit $STAGE1_RC) — the gate directory, its references or its seed data, none of which come from the branch"
fi

# The sentinel, and it is the point rather than decoration. Stage 1's exit code
# said 0 for every run it never executed, because `ssh -n` had closed the pipe
# the script travelled down. An exit code proves the ssh returned; only output
# the stage itself produced proves the stage ran. Assert the positive.
#
# This one is INFRA for the same reason and one more: a stage that did not run
# produced no evidence at all, so calling it a verdict about the branch is not
# merely wrong about the cause, it is a verdict drawn from nothing.
grep -q '^STAGE1 OK' "$LOG" || \
    infra "stage 1 exited 0 but produced no output: it did not run"

# --- stage 2: rsync the branch tip ------------------------------------------
# t0-remote.sh reads RUSTMC_REMOTE and RUSTMC_REMOTE_DIR, exported above, so it
# cannot target a different directory from the stages either side of it.
#
# `--sync` ships and runs nothing. The rsync exclude list stays in exactly one
# place in the tree: duplicating it here to get a seam would have put a second
# copy of `--exclude 'oracle-work'` in the repo, and the trailing-slash version
# of that line has already cost 200 MB and a re-extract once.
mark "2-rsync"
echo "== stage 2: rsync the branch tip into ~/$REMOTE_DIR" >>"$LOG"
stage_clock
./ci/t0-remote.sh --sync >>"$LOG" 2>&1
SYNC_RC=$?
stage_mark 2 rsync "$SYNC_RC"

# --- stage 3: this tree's own tables, BEFORE anything reads them ------------
#
# Everything about why this is here and not after T0 is at the top of the file.
# The short version: this is the only stage that can put a table this branch
# invented into the gate directory, and every test stage after it reads those
# tables. It ships nothing and builds nothing — it runs the branch's own
# javac/java generators against the version-pinned jar.
mark "3-tables"
if [ "$SYNC_RC" -eq 0 ]; then
    echo "== stage 3: this branch's own generated tables in ~/$REMOTE_DIR" >>"$LOG"
    stage_clock
    # `--drop-foreign` first: a table another branch generated here and this
    # one does not require goes back to the seed before anything reads it
    # (ci/gen-gameplay-tables.sh has the ranged/nether-parity case).
    remote_timed 3 "./ci/gen-gameplay-tables.sh --drop-foreign $SHARED_DATA && ./ci/gen-gameplay-tables.sh" >>"$LOG" 2>&1
    TABLES_RC=$?
    stage_mark 3 tables "$TABLES_RC"
else
    TABLES_RC=99
    echo "== stage 3 skipped: the rsync failed" >>"$LOG"
fi

# --- stage 4: T0 ------------------------------------------------------------
# `--run` runs T0 on what stage 2 shipped and stage 3 generated. It does not
# rsync again: a second rsync here would be harmless today but would make the
# tree T0 tests differ from the tree the tables were generated from the moment
# anyone edited between the two, which is the class of thing this file exists
# to make impossible.
mark "4-t0"
if [ "$TABLES_RC" -eq 0 ]; then
    echo "== stage 4: T0 in ~/$REMOTE_DIR" >>"$LOG"
    stage_clock
    ./ci/t0-remote.sh --run >>"$LOG" 2>&1
    T0_RC=$?
    stage_mark 4 t0 "$T0_RC"
else
    T0_RC=99
    echo "== stage 4 skipped: an earlier stage was red" >>"$LOG"
fi

# --- stage 5: T1, the test binaries nothing has ever gated ------------------
#
# ADDED 2026-08-05. T1 is `cargo test --workspace --no-fail-fast`, it exits 101
# on two honestly-red golden vectors, and a permanently-red stage gets switched
# off within a day — so it had never been in a merge gate, and **78 test
# binaries, every golden vector and every integration test under `tests/`, were
# outside every merge gate for the project's whole history.** `ci/t0.sh` is
# `--lib --bins`, which selects a subset of those targets; T1 selects all of
# them. `ci/t1-gate.sh` adjudicates the two known reds by **signature**, so a
# third failure — or one of the two changing its count, its y bands or its
# seed/chunk — is red.
#
# **No count of test binaries is written in this file, deliberately.** It said
# "95" while `ci/t1-gate.sh` was reporting 98 and then 99, which is the same
# defect as the "eight" tables that were twelve: a number in a comment is a
# number every reader takes on trust and nobody re-checks, and it drifts every
# time a crate gains a target. The figure is derived and printed —
# `ci/t1-allowlist.py` counts the binaries that reported a result and prints
# `T1-GATE GREEN — <n> test binaries, ...` into this log — and that printout is
# the only figure worth quoting. Its floor lives in
# `ci/checks/t1-known-red.toml` as `min_binaries`, which is a declaration the
# gate enforces rather than a comment it hopes for.
#
# WHERE IT GOES, AND WHY. Not a preference: a constraint, then a cost.
#
#   * **It must be after stage 3.** Four T1 binaries read `data/vanilla`
#     tables that `gen-gameplay-tables.sh` regenerates under *this branch's*
#     provenance — `block_props.json` and `items.json` in
#     `crates/rustmc-server/tests/{decorated_world,tiled_world,margin_probe}.rs`
#     and `crates/rustmc-behavior/tests/golden_survival.rs`. Stage 1 seeds
#     `data/vanilla` with `cp -rn` from a shared directory and deletes the
#     stamps; running T1 before stage 3 would gate this branch's golden
#     vectors against whichever branch last regenerated that directory. That
#     is precisely the defect that reported a healthy branch as 12/55 with 11
#     regressed on 2026-08-03.
#
#     **Since 2026-08-06 that constraint is no longer this stage's alone**, and
#     that is the better statement of it: T0 reads those tables too, from
#     `--lib` tests, so stage 3 now precedes *every* stage that reads them.
#     This bullet used to be the only place the rule was written and the rule
#     was one stage too narrow. A constraint that lives in one stage's comment
#     is a constraint the stage next door does not have.
#
#   * **Given that, cheap first.** Measured 676 s against `ci/t2-gameplay.sh`'s
#     ~50 minutes. A golden-vector regression surfaces in about eleven minutes
#     instead of an hour, and a corpus run over a tree with a broken golden
#     vector spends fifty minutes measuring a tree already known to be wrong.
#     There is no argument the other way: if T2 is red the loop stops anyway,
#     so running the expensive stage first buys nothing and costs the
#     difference.
#
# T1's own output is every binary's worth and stays in a file on the box;
# `ci/t1-gate.sh` prints one line up this log, and that line carries the count.
mark "5-t1"
if [ "$T0_RC" -eq 0 ]; then
    echo "== stage 5: T1 (every test binary) in ~/$REMOTE_DIR" >>"$LOG"
    stage_clock
    remote_timed 5 "./ci/t1-gate.sh" >>"$LOG" 2>&1
    T1_RC=$?
    stage_mark 5 t1 "$T1_RC"
else
    T1_RC=99
    echo "== stage 5 skipped: an earlier stage was red" >>"$LOG"
fi

# --- stage 6: T2 gameplay ---------------------------------------------------
#
# `RUSTMC_CORPUS_LOCK_ROLE=gate` is the only thing in this file that knows the
# lock exists, and that is the whole integration. `ci/t2-gameplay.sh` takes a
# box-wide lock so that a corpus run and a gate can never overlap — the rule
# that was written down, was correct, and was enforced by agents remembering
# it until three runs overlapped on 2026-08-06. `gate` is a priority, not a
# bypass: it still queues, it still waits out a run already in progress, but
# any run that has not yet started hands the lock back to it. This is the only
# thing on the box that can move `main`, so it is the only thing that gets it.
#
# The variable is set here rather than in `remote()` on purpose. `remote()`
# carries every stage, and stages 1-5 do not want the corpus and must not
# declare intent for it: a gate that announced itself at stage 1 would make
# every waiting agent yield through forty minutes of T0 and T1 for a corpus it
# had not reached and might never reach, because a red T1 skips stage 6.
#
# --- AN EXPORT IN THE CALLING SHELL DOES NOT REACH THE BOX. 2026-08-16. ------
#
# `remote()` builds a command STRING and hands it to `ssh`. Nothing in this
# script's environment crosses that boundary: ssh forwards only variables an
# explicit `SendEnv`/`AcceptEnv` pair names, and this box configures neither.
# So `RUSTMC_REFERENCE_CACHE=1 ./ci/handoff.sh <branch>` sets the variable in a
# shell that runs nothing on the box, `ci/t2-gameplay.sh` there sees it unset,
# and the gate boots vanilla exactly as before while the operator believes the
# cache is on.
#
# **That failure is silent in the direction that matters.** A dropped flag
# presents as a cache that never hits — which is indistinguishable, from the
# outside, from a cache that is working correctly and legitimately missing
# every time. `ci/t2-gameplay.sh` therefore echoes the value it actually
# received, on the box, before anything reads it; that line is the proof the
# flag arrived, and it is the thing to grep for when a hit does not happen.
#
# This is the same trap `ci/lanes.sh` documents for `CARGO_BUILD_JOBS`, which
# it solves by writing the value into the lane's `.cargo/config.toml` before
# the rsync so it travels with the tree. That fix suits a per-tree constant.
# This one is a per-RUN choice — the whole point is to gate the same branch
# both ways — so it is carried inline on the command string instead, exactly
# the way `RUSTMC_CORPUS_LOCK_ROLE=gate` already is on the line below.
#
# The value is validated before it is spliced into a remote shell command, and
# an unrecognised one is a refusal rather than an assignment: `0` and `1` are
# the only values `ci/reference-cache.sh` distinguishes, and anything else
# reaching a remote `sh -c` is a hole, not a setting. **That validation is not
# here.** It runs before stage 1, at the `REFCACHE_ENV=""` block beside the
# `CARGO_BUILD_JOBS` one, for the reason that block's own header gives: an
# input is rejected where it is read, not where it is used, so a typo costs
# nothing rather than most of an hour. A second copy beside this line is not
# belt and braces — `ci/check-handoff-terminals.py` counts terminal states by
# their message and reads a duplicated `is not 0 or 1` as an undeclared one.
#
# STAGES 6 AND 7 ARE ONE REMOTE CALL INSIDE ONE CORPUS-LOCK HOLD, since
# 2026-09-30. They were two, each harness acquiring and releasing the lock, and
# the lock was free between them: a run queued behind this gate took it, and a
# running run is never preempted, so the gate waited it out before stage 7 —
# 8.2 of 146.8 minutes on train-a's gate, measured by the gate-pipeline lane.
# `ci/gate-corpus-stages.sh` runs both under `ci/corpus-lock.sh hold`, and
# prints `GATE-STAGE-RC <n> <rc>` per stage it ran; the verdicts are read from
# those lines. So stage 7's applicability is decided here, before stage 6,
# which costs nothing: it reads `git diff` and nothing stage 6 produces.
mark "6-t2"
T2_RC=99; T2P_RC=99; P7=0; PERSIST_APPLIES=""
if [ "$T1_RC" -eq 0 ]; then
    PERSIST_APPLIES=$(# `main` and not a computed merge base: this script has already refused
    # any branch that is BEHIND main (`$BEHIND` must be 0 to reach here), so
    # main IS the merge base and a second computation of it could only
    # disagree with the one the gate acted on.
    ./ci/persistence-applies.py --changed-from main 2>>"$LOG")
    case "$PERSIST_APPLIES" in APPLIES|UNDECIDABLE) P7=1 ;; esac
    echo "== stage 6: T2 gameplay in ~/$REMOTE_DIR (${REFCACHE_ENV:-reference cache: not requested})" >>"$LOG"
    stage_clock
    STAGES_OUT="$(mktemp)"
    # `remote_timed 6` as every stage is, but this one call is stages 6 AND 7:
    # their wall, CPU and lock wait are one `GATE-STAGE stage=6` line, named
    # for both when stage 7 ran inside it.
    remote_timed 6 "${REFCACHE_ENV}RUSTMC_CORPUS_LOCK_ROLE=gate ./ci/corpus-lock.sh hold -- ./ci/gate-corpus-stages.sh $P7 '$REMOTE_DIR' '$PERSIST_APPLIES'" 2>&1 \
        | tee -a "$LOG" > "$STAGES_OUT"
    HOLD_RC=${PIPESTATUS[0]}
    T2_RC="$(sed -n 's/^GATE-STAGE-RC 6 \([0-9][0-9]*\)$/\1/p' "$STAGES_OUT" | tail -1)"
    T2P_RC="$(sed -n 's/^GATE-STAGE-RC 7 \([0-9][0-9]*\)$/\1/p' "$STAGES_OUT" | tail -1)"
    rm -f "$STAGES_OUT"
    # No line is not a pass: the hold refused, or the call died before the
    # stage could report. The call's own status stands in, and a zero there
    # with no line is still a failure.
    if [ -z "$T2_RC" ]; then T2_RC="$HOLD_RC"; [ "$T2_RC" -ne 0 ] || T2_RC=1; fi
    if [ "$T2_RC" -eq 0 ] && [ "$P7" = 1 ] && [ -z "$T2P_RC" ]; then
        T2P_RC="$HOLD_RC"; [ "$T2P_RC" -ne 0 ] || T2P_RC=1
    fi
    if [ -n "$T2P_RC" ]; then stage_mark 6 t2-gameplay+t2-persistence "$T2_RC"
    else stage_mark 6 t2-gameplay "$T2_RC"; T2P_RC=99; fi
else
    echo "== stage 6 skipped: an earlier stage was red" >>"$LOG"
fi
mark "7-t2p"

# --- stage 7: the persistence gates -----------------------------------------
#
# ADDED 2026-09-05, and it is the same hole stage 5 was. The scripts that
# stand behind the save path — the ones that boot a JVM and ask real vanilla
# what is in a directory or a region file we wrote, the one that signals a
# running server with a player still connected, and the one that reinserts
# three shipped defects into the byte-identity golden and requires a red for
# each — were **invoked by nothing automatic.** `ci/t2-persistence.sh`'s
# `GATES` array is the list and no count is written here. `docs/CI.md` said so in
# its own words, until this commit rewrote that section: "nothing runs them
# for you", "a gate nobody runs is a gate
# that has not run". `ci/t2-vanilla-loads-ours.sh` had been in the tree since
# August and had never once been run by a gate.
#
# A hand-run gate does not go red, it lapses, and the lapse is invisible: the
# last reading anybody has stays green while the code underneath it moves.
#
# WHY IT IS CONDITIONAL, AND WHY THE CONDITION IS NOT A HOLE
#
# The stage boots a JVM or a pair of rustmc servers once per gate and once per
# control, and rebuilds `rustmc-world` four times more. Paid on every gate
# that is minutes on top of
# a 65.2-minute serial gate, charged to the ~90% of branches that cannot touch
# the save path. `ci/persistence-applies.py` reads
# `ci/checks/persistence-paths.toml` and answers APPLIES or NOT-APPLICABLE
# from `git diff --name-only`.
#
# Two things make that honest rather than a quiet skip:
#
#   * **the skip is a third state and the verdict line says so.** `t2p=n/a` is
#     a different string from `t2p=0/1`. This is the `t1=99/0` lesson of
#     2026-08-18 applied before it could cost anything: 113 RED verdicts named
#     T1 and T2 as having failed with status 99 when neither had run.
#   * **a wrong list can only be as bad as today.** Before this stage the four
#     gates ran on nothing. A path that should be declared and is not leaves
#     that branch exactly where every branch already was.
#
# UNDECIDABLE is a fourth answer and it runs the stage. `git diff` against a
# merge base that cannot be resolved is not a branch this can decide about,
# and guessing NOT-APPLICABLE would skip the stage on exactly the branches
# whose history is strange.
#
# WHERE IT GOES. After stage 6 rather than before it, on cost: the corpus is
# 43 minutes and this is minutes, so a corpus regression surfaces first and a
# branch that fails it does not pay for the persistence gates as well. The
# reverse order would be defensible if this stage were cheaper than it is
# relative to what it catches; it is not, because it only runs on branches
# that touched the save path and those branches want the corpus verdict too.
# Stage 7 itself ran inside stage 6's hold, above; what is left here is
# saying so when it did not.
if [ "$T2_RC" -eq 0 ]; then
    if [ "$P7" != 1 ]; then
        T2P_RC=98
        echo "== stage 7 not applicable: this branch touches none of the paths in ci/checks/persistence-paths.toml, so the four persistence gates would measure a save path it did not change" >>"$LOG"
    fi
else
    T2P_RC=99
    echo "== stage 7 skipped: an earlier stage was red" >>"$LOG"
fi
mark "end"

# --- did this gate compile anything? ----------------------------------------
#
# THE PROBLEM. `-q` on every cargo call means the log cannot tell a full
# workspace build from a no-op, and the freshness hole's whole signature is "a
# green produced by compiling nothing". The three candidate fixes were dropping
# `-q`, an explicit build fingerprint, and counting what rustc wrote. This is
# the third, and the reasons are:
#
#   * **dropping `-q` does not decide it.** `Compiling` is emitted per cargo
#     *invocation*, and a gate runs clippy, check, test, T1's test and T2's
#     build — five passes over the same workspace. A cold gate is several
#     thousand lines of it and the reader still has to count, while the useful
#     answer is one number. It also lives in `ci/t0.sh`, `ci/t1.sh` and
#     `ci/t2-gameplay.sh`, three files this change does not own.
#   * **it observes the filesystem rather than cargo's narration of itself.**
#     rustc writes into `target/<profile>/{deps,.fingerprint}` and
#     `target/debug/incremental` only for units it actually rebuilds. That is
#     the same instrument the forensics used, and it separated the lab's two
#     arms cleanly: back-dated **1** entry, current-mtime **104**.
#
# TWO THINGS THE FORENSICS GOT WRONG FIRST, BOTH ENCODED HERE.
#
#   * `target/.rustc_info.json` is the one entry cargo can rewrite without
#     compiling anything — it was the *only* write in the forensics lab's
#     back-dated arm, the arm that certified a broken tree green. It is never
#     evidence of a compile, so excluding it is what makes 0 mean 0. (It is not
#     rewritten on every invocation: a warm no-op `cargo build` on the box left
#     it alone. The exclusion is required for the case where it does, and costs
#     nothing where it does not.)
#   * scoping to `target/debug/deps` produced **37 false "compiled nothing"
#     verdicts**, including a run that had built 4,074 artefacts visible only
#     under `incremental/` because the `deps/` outputs had since been
#     overwritten. So: all of `target/`, never a subdirectory.
#
# THE WINDOW. Each run leaves a stamp on the box naming the tip it loaded into
# that directory; the census counts what was written since the **previous**
# run's stamp. That window is exact rather than survivorship-limited, because
# it is read while the run is still the most recent thing to have touched the
# directory — the erosion that made the archaeology hard only happens later.
# The stamp lives in `~/.rustmc-gate-provenance/`, NOT in the gate directory,
# because stage 2 rsyncs with `--delete` and would remove it.
#
# WHAT THIS MAKES DECIDABLE, which is the point:
#
#   compiled>0                          rustc ran. Not a guess.
#   compiled=0, rust-files-changed=0    a legitimate no-op — the same tip
#                                       re-gated. Three such runs are in the
#                                       record and every one was correct.
#   compiled=0, rust-files-changed>0    the freshness hole fired, or something
#                                       else made cargo skip work it owed. This
#                                       is the case no log could previously
#                                       show, and it is now one line.
#
# IT IS NOT A VERDICT, AND THAT IS DELIBERATE. The false-positive space is not
# yet enumerated — a `.rs` outside the cargo workspace, a change entirely inside
# `#[cfg]`-ed-out code — and a merge-gate red that fires wrongly once gets
# switched off within a day; this file says so about T1. It should become a red
# when a month of logs shows the third line never appearing on a healthy gate.
# The mtime symptom itself is fixed in `ci/t0-remote.sh` on
# `gate-command-coverage`; this is the instrument that would notice if it were
# not, and the record that makes the question answerable next time.
PROV="$(remote_script 2>&1 <<PROV_EOF
set -uo pipefail
D=\$HOME/.rustmc-gate-provenance
mkdir -p "\$D"
S="\$D/$STAMP_KEY"
NEWER=""
if [ -s "\$S" ]; then
    echo "gate-dir-prev: \$(tr '\n' ' ' < "\$S")"
    NEWER="-newer \$S"
else
    echo "gate-dir-prev: none — this box directory has never been stamped"
fi
echo "gate-dir-prev-tip=\$(sed -n 's/^tip=//p' "\$S" 2>/dev/null || true)"
T="\$(mktemp)"
if [ -d target ]; then
    find target \$NEWER -type f ! -name .rustc_info.json -printf '%h\n' 2>/dev/null > "\$T" || true
fi
echo "compile-census: \$(wc -l < "\$T") artefacts written under target/ \$([ -n "\$NEWER" ] && echo 'since the previous gate in this directory' || echo '(NO PRIOR STAMP: this is every artefact in the directory, not this run alone)')"
sort "\$T" | uniq -c | sort -rn | head -4 | sed 's/^/  census:/'
rm -f "\$T"
if [ "$SYNC_RC" -eq 0 ]; then
    # Only when the rsync succeeded: if it did not, this directory still holds
    # whatever the previous run left, and a stamp claiming otherwise would be a
    # record that lies in the one direction that matters.
    { echo "tip=$TIP"; echo "tree=$ROOT_TREE"; echo "root=$ROOT"; echo "driver=$(hostname)"; echo "branch=$BRANCH"; echo "at=$(date -Is)"; } > "\$S"
    echo "gate-dir-stamped: tip=$TIP tree=$ROOT_TREE root=$ROOT"
else
    echo "gate-dir-stamped: no — the rsync failed, so this directory still holds the previous tip"
fi
PROV_EOF
)"
printf '%s\n' "$PROV" >>"$LOG"
PREV_TIP="$(printf '%s\n' "$PROV" | sed -n 's/^gate-dir-prev-tip=//p' | head -1)"
COMPILED="$(printf '%s\n' "$PROV" | sed -n 's/^compile-census: \([0-9]*\) .*/\1/p' | head -1)"
RUSTCHANGED="?"
if [ -n "$PREV_TIP" ] && git cat-file -e "$PREV_TIP^{commit}" 2>/dev/null; then
    # Scoped to what cargo actually reads. An unscoped count would call a
    # docs-only or ci-only branch suspicious, which is common here and would
    # make the line noise within a week.
    RUSTCHANGED=$(git diff --name-only "$PREV_TIP" "$TIP" -- '*.rs' '*Cargo.toml' '*Cargo.lock' '*build.rs' | wc -l)
fi
echo "handoff: compiled=${COMPILED:-?} rust-files-changed=$RUSTCHANGED since=${PREV_TIP:-unknown}" >>"$LOG"
# `compiled=0` HAS TWO OPPOSITE MEANINGS AND THEY MUST NOT SHARE A LINE.
#
#   * *cargo was asked to build and decided it had nothing to do* — the
#     freshness hole, a green issued by a build that did not happen. This is
#     what the warning is for;
#   * *no cargo command ever ran* — the gate exited before it got that far.
#     `compiled=0` is then arithmetically certain and says nothing whatever.
#
# The census cannot tell them apart by looking at `target/`, and that is not a
# gap in the implementation: a warm no-op `cargo build` on this box writes
# **nothing at all** (see `.rustc_info.json` above), so "cargo ran and skipped"
# and "cargo never ran" are byte-identical on the filesystem. No amount of
# counting artefacts will separate them. The separator has to be whether a
# cargo invocation was reached, which is not a filesystem fact.
#
# Fired on 2026-08-07 in the second sense, on a branch whose T0 exited at
# `ci/check-figures.py` — before `cargo fmt --check`, the first cargo command
# in `ci/t0.sh`. The log said "the gate compiled nothing over 12 changed
# cargo-visible files", which reads as the freshness hole and was not.
#
# `$T0_RC` decides it, and needs nothing added to `ci/t0.sh`:
#
#   * **T0_RC = 0 implies cargo ran.** `ci/t0.sh` runs `cargo fmt --check`,
#     `clippy`, `check` and `test` unconditionally once its pre-build checks
#     pass, and any of them failing exits non-zero. So a zero exit is proof the
#     workspace was built. This is the only case in which `compiled=0` is
#     evidence of anything, and it is exactly the dangerous one — a gate that
#     otherwise looks green;
#   * **T0_RC != 0 means the gate is RED already.** It will not merge, the red
#     is reported with its own cause, and a "compiled nothing" line on top of it
#     is noise that impersonates a much more serious finding. Whether cargo ran
#     before the failure is unknowable from here and does not matter, because
#     nothing is being authorised.
#
# Deliberately NOT done by adding a "cargo starts here" sentinel to `ci/t0.sh`:
# that would be a second file to keep in step for a distinction this file can
# already make, and this repo's record on corrections that live in one file is
# poor. If T0 ever stops running cargo unconditionally, the first bullet stops
# being true and this needs revisiting — which is why it is written down.
if [ "${COMPILED:-1}" = "0" ] && [ "$RUSTCHANGED" != "?" ] && [ "$RUSTCHANGED" -gt 0 ] 2>/dev/null; then
    if [ "$T0_RC" -eq 0 ]; then
        echo "handoff: WARNING — $RUSTCHANGED cargo-visible files changed since $PREV_TIP and rustc wrote NOTHING," >>"$LOG"
        echo "handoff:           and T0 exited 0, so cargo DID run and decided it had nothing to do." >>"$LOG"
        echo "handoff:           That is the signature of a green issued by a build that did not happen." >>"$LOG"
        # `handoff:` and not `WARNING`, deliberately. This line is printed on
        # stdout BEFORE the verdict — the RED or the MERGED is decided further
        # down — so a consumer that takes the first verdict-shaped line it sees
        # would name this one. It is an advisory about the build, not an outcome
        # of the gate, and `handoff:` is the prefix every other advisory in this
        # file already uses and every consumer already skips. The word WARNING
        # stays inside the sentence, where a human reads it and a regex does not.
        echo "handoff: WARNING — $BRANCH: the gate compiled nothing over $RUSTCHANGED changed cargo-visible files; see $LOG"
    else
        echo "handoff: compile-census not applicable — T0 exited $T0_RC, so the gate is red and a" >>"$LOG"
        echo "handoff:           build was never completed. compiled=0 over $RUSTCHANGED changed files is" >>"$LOG"
        echo "handoff:           arithmetic, not evidence: it says nothing about freshness." >>"$LOG"
    fi
fi

# The lock, asserted rather than assumed, at the last moment it can still
# matter. Everything below either merges or explains why not, and a merge
# performed while a second gate has been running in the same box directory is
# exactly the authorisation this whole change exists to prevent. One `kill -0`.
gate_lock_assert_anchored || { stage_summary; echo "         (checked after stage 6, before the merge)"; exit 2; }

# The same question again at the moment the counters are actually read, and
# asked a stronger way than "does the file exist".
#
# The log is written across the whole gate and can go away underneath it — a
# full disk, a cleanup job, a symlink target that moved. `[ -s "$LOG" ]` is not
# enough to catch that, and finding out why is the useful part: the provenance
# census three blocks above appends to `$LOG`, so a log deleted at any point
# before it is RECREATED, non-empty, and missing every stage's output. The
# counters then read 0 from a file that exists, which is the same RED with the
# same absent cause, now wearing a plausible-looking artefact.
#
# So the assertion is that the log still contains the header THIS run wrote into
# it. That is not a test of existence, it is a test of identity: if the header
# is gone, the file being grepped for a verdict is not the file this gate has
# been writing, and nothing in it is evidence about anything.
grep -q "^handoff: branch=$BRANCH tip=$TIP " "$LOG" 2>/dev/null || \
    infra "the gate log no longer contains this run's own header — it was truncated, replaced or removed while the gate was running, and every counter below is a grep over it"

SKIPS=$(grep -c "skipping: no vanilla" "$LOG")
T0_GREEN=$(grep -c "^T0 GREEN" "$LOG")
T1_GREEN=$(grep -c "^T1-GATE GREEN" "$LOG")
T2_GREEN=$(grep -c "^T2-GAMEPLAY GREEN" "$LOG")
# 98 is stage 7's own sentinel for "did not apply", and it is NOT 99. A
# skipped-because-an-earlier-stage-was-red and a did-not-apply are opposite
# facts — one says nothing about this branch, the other says this branch
# cannot have broken the save path — and collapsing them is exactly the
# `t1=99` conflation of 2026-08-18. The GREEN counter is forced to 1 in the
# not-applicable case so the "a sentinel was missing" rule below, which is
# what catches a stage that returned 0 without printing its verdict, does
# not fire on a stage that correctly did not run.
if [ "$T2P_RC" -eq 98 ]; then T2P_GREEN=1; else T2P_GREEN=$(grep -c "^T2-PERSISTENCE GREEN" "$LOG"); fi

# One spelling of a stage counter, used by both the RED line and the GREEN
# one. 99 is "an earlier stage was red and this never ran"; 98 is stage 7's
# "this branch touches no path the persistence gates measure". They are
# opposite facts and neither is an exit code, so neither is printed as one —
# that conflation is what made 113 RED verdicts on 2026-08-18 name T1 and T2
# as having failed with status 99 when neither had run.
rc_or_skipped() {
    case "$1" in
        99) printf 'skipped' ;;
        98) printf 'n/a' ;;
        *)  printf '%s' "$1" ;;
    esac
}

# A gate that skipped its vanilla-backed tests is not a green gate — it is a
# gate that could not fail. That distinction has cost this project a run before.
#
# Be accurate about what `$SKIPS` covers, because a guard believed to cover more
# than it does is the same failure wearing a different hat.
#
# **`$SKIPS` has no reachable emitter, and has never had one.** Measured
# 2026-08-18; see `docs/PARITY-ROADMAP.md` §73.
#
# The paragraph that stood here said "the one live emitter reachable from here
# is `ci/dump-flat-preset.sh`, via stage 3". That file contains the string
# `skipping: no vanilla` **once, in a comment about where the idiom comes
# from**, and echoes nothing. Nor does any of the other 21 scripts
# `ci/gen-gameplay-tables.sh` invokes. The only two live shell emitters in the
# tree, `ci/village-pieces.sh` and `ci/village-oracle.sh`, are called by
# nothing at all. And every Rust emitter is invisible for the reason in
# `crates/rustmc-data/src/fixture.rs`: cargo never prints a passing test's
# stderr, and a test that returns early passes.
#
# So this grep matched its emitters in **0 of 0** — the same shape as the
# `  REGRESSED` pattern that matched its emitter in 0 of 45 lines, arrived at
# from the other direction. The comment above it was careful, said "a guard
# believed to cover more than it does is the same failure wearing a different
# hat", and then named the one emitter it had, wrongly.
#
# `$SKIPS` is left computed and left in the verdict. It costs nothing, it is
# honest now that this says what it is worth, and it would start working the
# day a shell stage prints the line. What replaced it is in T0, where it needs
# nothing to have run: `ci/check-fixture-skips.py` ratchets the number of
# fixture checks that pass when their fixture is absent, and `ci/t1-gate.sh`
# refuses before running when the extracts are missing.
#
# So `$SKIPS` is structurally 0 and the `-ne 0` arm below has never been
# reachable. It is kept, because it costs a grep and it is the arm that would
# fire the day somebody adds a live emitter — but WHAT IT MAY NOT DO IS BE
# PUBLISHED AS A GUARANTEE. `no-vanilla-skips=0` on the RED line is a count and
# is honest. The literal words "0 skips" in the GREEN and MERGED verdicts were
# not: they told every reader of every merge that the gate had checked
# something it structurally could not check, which is this project's central
# defect class printed on its own results. They are gone.
#
# Stage 4 runs every `tests/` binary, so it is tempting to write that the guard
# now reaches them. **It does not.** T1's output is every binary's worth and
# `ci/t1-gate.sh` keeps it in a file on the box, sending one line up — so not a
# byte of it passes through this grep. The comment here said the opposite for
# about an hour. The guard lives where the output does: `ci/t1-gate.sh` counts
# its own skips and exits red on any, before it adjudicates anything.
#
# `ci/t2-gameplay.sh` refuses rather than skips, and the carver and surface
# goldens print `skipping: no rule extract`, which this pattern does not match
# either — their own coverage assertions and `t1-allowlist.py`'s
# `min_binaries` floor are what catch those.
#
# **AND THE GREP IS NO LONGER WHAT CATCHES A TEST-LEVEL SKIP, 2026-08-05.**
# It never was, and the account above is the record of working out how narrow it
# is. The seventeenth check-that-could-not-fail was found underneath it:
# `real_vanilla_chunks_survive_a_rewrite` — the only test claiming to check our
# chunk writer against real vanilla output — had skipped in every run of this
# loop for its whole life, because `oracle-work/t2-regiondiff/` did not exist on
# the box. No amount of widening this pattern could have seen it: `cargo`
# captures a *passing* test's stderr, so the skip line never reached this log.
#
# The fix is not a wider grep. A missing fixture is now a **test failure** by
# default (`rustmc_data::fixture`), so it arrives here as a non-zero `$T0_RC` or
# `$T1_RC` and an absent sentinel, through the channel that already works. The
# opt-out `RUSTMC_FIXTURES=optional` makes `ci/t0.sh` and `ci/t1.sh` print a
# *different* sentinel, so `$T0_GREEN`/`$T1_GREEN` go to 0 and this block
# reddens without needing to know the variable exists.
#
# `$SKIPS` stays exactly as it is, covering exactly what it covered: the shell
# stages. It is not the guard for test-level skips and never could be.
# The counters are printed in **stage order**, which is now sync, tables, T0,
# T1, T2. The old line led with `t0=` because T0 used to be the first stage
# that could fail; reading a verdict in an order the stages no longer run in is
# how a skipped stage gets mistaken for a passed one.
# --- infrastructure, checked BEFORE the red verdict is written ---------------
#
# Stage 1 is infra by construction and was handled where it runs. Stages 2-6 do
# execute branch code, so they are RED by default and only two shapes are
# carved out, each on evidence rather than on a hunch:
#
#   * **exit 255 from any of them.** That is ssh's own status for a connection
#     that failed, so the command never started on the box. `remote()` and
#     `remote_script()` are bare ssh; `ci/t0-remote.sh --run` ends in a bare
#     ssh and passes its status up. `--sync` flattens an rsync failure to 2 and
#     is covered by the second rule instead. A gate script exiting 255 of its
#     own accord would be misread here, and that is the direction to err in:
#     the branch does not merge either way, and INFRA asks for a re-run where
#     RED asks for a rewrite.
#   * **the rsync itself failing.** `ci/t0-remote.sh` prints exactly one
#     sentence for that and exits 2. Exit 2 alone is not enough — it is also
#     that script's usage error — so the sentence is required as well. A
#     transfer that did not complete has shipped no branch to measure.
#
# Checked in stage order, so the FIRST thing that went wrong is what gets
# named. A later stage's 99 (skipped) must never speak over an earlier cause.
for stage_rc in "stage 2 rsync:$SYNC_RC" "stage 3 tables:$TABLES_RC" \
                "stage 4 T0:$T0_RC" "stage 5 T1:$T1_RC" "stage 6 T2:$T2_RC" \
                "stage 7 T2-persistence:$T2P_RC"; do
    if [ "${stage_rc#*:}" = "255" ]; then
        infra "${stage_rc%%:*} could not reach $HOST — ssh exited 255, so that stage never ran on the box"
    fi
done
if [ "$SYNC_RC" -ne 0 ] && grep -qE "^t0-remote: rsync .*failed" "$LOG"; then
    infra "stage 2 could not ship the branch to $HOST — the rsync itself failed, so no branch source reached the box"
fi

if [ "$SYNC_RC" -ne 0 ] || [ "$TABLES_RC" -ne 0 ] || [ "$T0_RC" -ne 0 ] || [ "$T1_RC" -ne 0 ] || [ "$T2_RC" -ne 0 ] \
   || { [ "$T2P_RC" -ne 0 ] && [ "$T2P_RC" -ne 98 ]; } \
   || [ "$T0_GREEN" -eq 0 ] || [ "$T1_GREEN" -eq 0 ] || [ "$T2_GREEN" -eq 0 ] || [ "$T2P_GREEN" -eq 0 ] || [ "$SKIPS" -ne 0 ]; then
    # --- `t1=99/0` MEANT "NEVER RAN" AND READ LIKE AN EXIT CODE. 2026-08-18. --
    #
    # 99 is this file's own sentinel for a stage that was skipped because an
    # earlier one was red. Nothing said so. On 2026-08-18 between 09:12 and
    # 10:14 the chain produced **113 RED verdicts across 40 branches**, every
    # one of them reading
    #
    #     RED  <branch> — sync=0 tables=0 t0=1/0 t1=99/0 t2=99/0 no-vanilla-skips=0
    #
    # against a single environment fault: `ci/t0-remote.sh` rsyncs
    # `--exclude .git` and a check had started needing a repository. Every one
    # of those lines names T1 and T2 as having failed with status 99, and every
    # one of them is a parity failure that did not happen. 113 verdicts is the
    # most-travelled path this script has, and it was reporting a made-up
    # number in two of its five fields.
    #
    # So a skipped stage says `skipped`, and the line leads with the FIRST stage
    # that actually failed — which is the one fact a reader wants and had to
    # derive by scanning for the leftmost non-zero. The counters stay, because
    # `t0=1/0` and `t0=0/0` are different failures (a stage that failed, versus
    # a stage that returned 0 without printing its sentinel) and that
    # distinction is load-bearing.
    FIRST_BAD="none — a sentinel was missing"
    for stage_rc in "stage 2 rsync:$SYNC_RC" "stage 3 tables:$TABLES_RC" \
                    "stage 4 T0:$T0_RC" "stage 5 T1:$T1_RC" "stage 6 T2:$T2_RC" \
                "stage 7 T2-persistence:$T2P_RC"; do
        # 98 is stage 7's "did not apply" and is no more a failure than 99
        # is. Omitting it here would make every branch that legitimately
        # skips the persistence stage report `first failure: stage 7
        # T2-persistence (exit 98)` on any OTHER stage's red — a made-up
        # cause pointing at the one stage that did nothing.
        if [ "${stage_rc#*:}" != "0" ] && [ "${stage_rc#*:}" != "99" ] \
           && [ "${stage_rc#*:}" != "98" ]; then
            FIRST_BAD="${stage_rc%%:*} (exit ${stage_rc#*:})"
            break
        fi
    done
    stage_summary
    echo "RED      $BRANCH — first failure: $FIRST_BAD"
    echo "         sync=$SYNC_RC tables=$(rc_or_skipped "$TABLES_RC") t0=$(rc_or_skipped "$T0_RC")/$T0_GREEN t1=$(rc_or_skipped "$T1_RC")/$T1_GREEN t2=$(rc_or_skipped "$T2_RC")/$T2_GREEN t2p=$(rc_or_skipped "$T2P_RC")/$T2P_GREEN no-vanilla-skips=$SKIPS"
    echo "         (skipped = an earlier stage was red and this one never ran — NOT a failure of it;"
    echo "          n/a = this branch touches no path in ci/checks/persistence-paths.toml)"
    echo "         $LOG"
    # `vanilla-server:` is in this list because the one thing the 2026-08-05
    # hang never produced was a sentence naming a cause. Those lines carry the
    # cause and the path to the preserved crash report, and they are the only
    # reason to open the full log rather than guess at it.
    #
    # `gen-gameplay-tables` and its `  !!` lines are in this list as of
    # 2026-08-06. Stage 3 could always fail, but it ran fourth and behind a
    # green T0, so in practice it never did and nothing here named it. It now
    # runs before every test stage, so it is a likely *first* failure — a
    # branch that adds a table without adding its `generator_for()` case stops
    # exactly here — and a verdict of `tables=2` with no sentence saying why is
    # a verdict that sends you to the log to guess.
    # `corpus-lock:` is in this list because the corpus lock can refuse a run
    # before the harness has printed a word — no setpriv, or an anchor that did
    # not come up — and those refusals say `corpus-lock:`, not
    # `T2-GAMEPLAY REFUSED`. Without this the gate would print RED with t2=2 and
    # no sentence naming a cause, which is the one thing the 2026-08-05 hang
    # taught us to never ship again. Both of them are "the gate could not run",
    # not "the branch is bad".
    grep -E "^(error|REFUSED|T0 REFUSED|T1-GATE RED|T2-GAMEPLAY REFUSED|T2-PERSISTENCE RED|T2-PERSISTENCE REFUSED|corpus-lock:|vanilla-server:|test result: FAILED|failures:|  REGRESSED|gen-gameplay-tables:|  !!)" "$LOG" | head -12
    exit 1
fi

if [ -n "$DRY" ]; then
    stage_summary
    echo "GREEN    $BRANCH — $AHEAD_TEXT, T0+T1+T2 green, persistence $([ "$T2P_RC" = 98 ] && printf "n/a" || printf "green") (dry run, not merged)"
    echo "         $LOG"
    exit 0
fi

# --- merge, push ------------------------------------------------------------
# `main` can be held by a worktree exactly as the branch under test can, and
# this checkout is the one that fails *after* a full green gate has been paid
# for. The release is the same function and the same refusal: a dirty holder
# stops the merge with a remedy, the branch stays green, and re-running merges.
#
# --- EVERY REFUSAL BELOW THIS LINE COSTS A GATE THAT ALREADY PASSED ----------
#
# Above this line `REFUSED` means "the gate could not start", and re-running is
# cheap. Below it the gate has run all six stages and the branch is green, so
# the same word means something else entirely and the reader's next action is
# different. Each one therefore says what it cost and what to do, and the two
# that can leave `main` and `origin/main` disagreeing get their own verdict.
#
# --- NOTHING MAY SPEAK BEFORE THE VERDICT. 2026-08-18. ----------------------
#
# Every git command below writes its diagnostics to stderr, and a caller sees
# stdout and stderr on one stream: `ci/run-detached.sh` merges them into one
# log by design, and so does every `2>&1` an operator types. So a failing git
# got its sentence out FIRST and the verdict arrived underneath it.
#
# That is not cosmetic. It is what made the 2026-08-16 verdict unreadable —
# the line was `MERGED-NOT-PUSHED  push failed`, and the first line of output
# was `fatal: Could not read from remote repository.` A consumer taking line
# one got `fatal:`, which is in no vocabulary and classifies as nothing.
#
# So every git invocation in the merge tail sends its stderr to the log, where
# the detail belongs, and the script's own line is the first thing on the
# stream. The information is not discarded — `$LOG` is named in every one of
# these verdicts.
# --- THE MAIN-WRITER MUTEX, AND WHY THIS SIDE OF IT WAS MISSING ------------
#
# Everything from here to the push WRITES `main`: it checks it out, fast-
# forwards it and pushes it. Until 2026-08-19 that ran unprotected, and the
# comment justifying it was "it is the only writer".
#
# `ci/batch.sh` has been a second writer since it was written. Its `land_tip`
# takes `$(git rev-parse --git-common-dir)/rustmc-main-writer.lock` and its
# header names the missing other half as precondition 1 for enabling `--land`:
# "Until that lands, a batch landing can collide with a gate that has already
# paid 80 minutes, and the gate is the one that loses." A mutex only one party
# takes excludes nobody — it is the per-cwd gate-lock confusion in another
# form, and it is worse than no lock because the party that does take it is
# entitled to believe it is protected.
#
# The two failures it prevents are the ones `ci/batch.sh`'s header describes,
# and neither names its cause when it happens: a batch advancing `main` under
# this checkout makes every file of the batch read as modified here, which is
# the 2026-08-17 861-refusal bug; and a batch advancing the ref between this
# script's ff-merge and its push turns the push into a non-fast-forward after
# a full gate has been paid for.
#
# FAIL CLOSED. If the lock cannot be taken, this refuses and exits 2 rather
# than merging unprotected — the gate is green and re-running merges, so the
# cost of refusing is one re-run and the cost of proceeding is a corrupted
# `main` in a window nobody is watching. The wait is generous because the
# thing on the other side is a batch landing, which is short, and 300 s is
# what `ci/batch.sh` waits for this same lock.
MAIN_WRITER_LOCK="$(cd "$(git rev-parse --git-common-dir)" && pwd)/rustmc-main-writer.lock"
exec 9>>"$MAIN_WRITER_LOCK" || {
    echo "REFUSED  cannot open the main-writer mutex at $MAIN_WRITER_LOCK"
    echo "         The gate passed and nothing has been merged. Re-running merges."
    exit 2; }
# The wait is a variable only so `ci/handoff-merge-tail-falsify.sh` case 13 can
# assert the refusal in two seconds instead of five minutes. Nothing in normal
# operation sets it, and the default is what `ci/batch.sh` waits for this same
# lock.
if ! flock -x -w "${RUSTMC_MAIN_WRITER_WAIT:-300}" 9; then
    echo "REFUSED  another main-writer holds $MAIN_WRITER_LOCK after ${RUSTMC_MAIN_WRITER_WAIT:-300} s"
    echo "         $LOG"
    echo "         ci/batch.sh --land takes this same lock. The gate on $BRANCH is"
    echo "         GREEN and nothing has been merged, so nothing is lost: re-run and"
    echo "         it merges. Proceeding without the lock is what this refuses to do."
    exit 2
fi

release_worktree main || { echo "         (after a full green gate on $BRANCH — the gate is not lost, re-running merges)"; exit 2; }
git checkout -q main 2>>"$LOG" || {
    echo "REFUSED  could not check out main after a green gate on $BRANCH"
    echo "         $LOG"
    echo "         The gate passed and nothing has been merged. main is not checked out"
    echo "         here, so this is local state, not the branch. Re-running merges."
    exit 2; }
# The same post-condition as the branch checkout above, for the same measured
# reason, and it matters more here: a `git merge --ff-only` run in a tree that
# is not actually at `main` would either refuse confusingly or merge from the
# wrong place, after a full green gate has been paid for.
if [ "$(git rev-parse HEAD)" != "$(git rev-parse main)" ] || [ -n "$(git status --porcelain)" ]; then
    echo "REFUSED  the checkout of main did not complete after a green gate on $BRANCH, and git still exited 0"
    echo "         $LOG"
    echo "         HEAD is $(git rev-parse --short HEAD), main is $(git rev-parse --short main)"
    git status --porcelain | head -5 | while IFS= read -r l; do echo "             $l"; done
    echo "         Nothing has been merged and the gate is not lost. Fix the tree and"
    echo "         re-run; the branch is still green."
    exit 2
fi

# The entry check proved `main` was an ancestor of `$BRANCH`, so a refusal here
# is not "the branch is behind main" — it is `main` having MOVED while this gate
# was running, which is a fact about the queue. Reporting it as the bare
# `REFUSED  ff-merge failed after a green gate` sent the reader looking for
# local state in the tree, where there was none to find.
git merge --ff-only "$BRANCH" -q 2>>"$LOG" || {
    echo "NOT-FF   main moved while $BRANCH was being gated — the fast-forward no longer applies"
    echo "         $LOG"
    echo "         $BRANCH was $AHEAD ahead and 0 behind main when this gate started, so"
    echo "         main has gained $(git rev-list --count "$BRANCH..main" 2>/dev/null || echo '?') commit(s) since. The gate is not wasted and the"
    echo "         branch is not bad: rebase onto the new main and re-gate."
    exit 3; }

# From here `main` carries the branch and origin does not. This is the window
# the whole `UNPUSHED` verdict exists for.
push_main_with_retry "$BRANCH" || \
    unpushed "the gate was green, the ff-merge is DONE in local main, and three pushes to origin failed"

# --- AND THE VERDICT IS PRINTED BEFORE ANYTHING OPTIONAL RUNS ---------------
#
# `cc-budget` used to run HERE, between the successful push and the `MERGED`
# line. It is bookkeeping — it marks an agent boundary in a usage ledger — and
# it was standing between a completed, pushed merge and the only sentence that
# says so. A `cc-budget` that hangs, or that is killed with the gate, produces
# the state this file now calls "pushed but not recorded": `main` is merged,
# `origin/main` has it, everything worked, and the caller sees no verdict line
# and a non-zero exit. The queue would then re-gate a branch that had already
# landed.
#
# The rule, which is the same rule as the stderr redirects above: the verdict
# is the first thing said and nothing optional happens before it. Bookkeeping
# goes after, where its failure costs a ledger entry rather than a verdict.
stage_summary
echo "MERGED   $BRANCH -> main  $AHEAD_TEXT  total $(git rev-list --count HEAD)  unpushed-on-branch-at-entry=$UNPUSHED  T0+T1+T2 green, persistence $([ "$T2P_RC" = 98 ] && printf 'n/a' || printf 'green')"
echo "         gated in ~/$REMOTE_DIR on $HOST at $TIP — $LOG"

# Bookkeeping, after the verdict, and it may fail freely.
command -v cc-budget >/dev/null && { cc-budget --mark agent:end >/dev/null 2>&1; cc-budget --mark agent:start >/dev/null 2>&1; }
exit 0
