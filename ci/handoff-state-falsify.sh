#!/usr/bin/env bash
# ci/handoff-state-falsify.sh — the refusals that happen before the box.
#
# `ci/handoff.sh` has twenty-one terminal states. `ci/handoff-verdict-selftest.sh`
# forces eight of them and `ci/handoff-merge-tail-falsify.sh` forces six more.
# The rest are the cheap ones at the top of the file — no branch given, no such
# branch, a dirty tree, a worktree holding the branch, the canonical checkout
# named as a gate directory — and **not one of them had ever been executed by
# any test.** They are the states an operator meets most often and the only
# evidence that any of them said anything at all was that somebody had once
# read the line in the source.
#
# They are also the cheapest possible thing to force: every one of them is
# reached before the first ssh, so this file needs no box, no stub for one, and
# runs in under a second. There was never a cost argument for leaving them
# uncovered.
#
# WHY IT IS SEPARATE FROM THE VERDICT SELFTEST. That file's `run_case` hardcodes
# `--dry-run` and the branch name `under-test`, because every scenario it drives
# is a stage outcome on one fixed branch. These are not stage outcomes; several
# of them need a DIFFERENT repository shape (a second worktree, a dirty tree, a
# branch that does not exist), which is a different fixture, not a different
# switch. Forcing them through `run_case` would mean a fixture builder with a
# flag per case, and this repo's record on that shape is poor.
#
# Run:  ./ci/handoff-state-falsify.sh [-v]
set -uo pipefail

VERBOSE=""
[ "${1:-}" = "-v" ] && VERBOSE=1

CI="$(cd "$(dirname "$0")" && pwd)"
for f in handoff.sh gate-lock.sh; do
    [ -r "$CI/$f" ] || { echo "no ci/$f beside this script"; exit 2; }
done

WORK="$(mktemp -d)"
trap 'chmod -R u+rwX "$WORK" 2>/dev/null; rm -rf "$WORK"' EXIT
PASS=0; FAIL=0
ok()  { PASS=$((PASS + 1)); printf '  ok    %-26s -> %-14s exit %s\n' "$1" "$2" "$3"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL  %-26s -> %-14s exit %s   (wanted %s / exit %s)\n' "$1" "$2" "$3" "$4" "$5"; }

# A fresh repo per case. These cases mutate the tree — that is the point of
# them — so sharing one would make each case's fixture depend on the last one's
# damage, and a suite like that reports the first failure and then noise.
build() {
    local R="$WORK/repo-$1"
    rm -rf "$R"; mkdir -p "$R/ci"
    git init -q -b main "$R"
    cp "$CI/handoff.sh" "$CI/gate-lock.sh" "$R/ci/"
    chmod +x "$R/ci/handoff.sh" "$R/ci/gate-lock.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$R/ci/t0-remote.sh"
    chmod +x "$R/ci/t0-remote.sh"
    ( cd "$R" \
      && git add ci \
      && git -c user.name=t -c user.email=t@t commit -qm "gate scripts" \
      && git checkout -q -b under-test \
      && echo change > file.txt && git add file.txt \
      && git -c user.name=t -c user.email=t@t commit -qm "the branch under test" \
      && git checkout -q main ) >/dev/null 2>&1
    printf '%s' "$R"
}

# No ssh on PATH at all. Every case here must terminate before the gate reaches
# the box, and if one ever stops doing so it fails loudly with
# `ssh: command not found` rather than quietly gating against a real host.
run() {
    local R="$1"; shift
    local br="$1"; shift
    ( cd "$R" && env -i \
        PATH="/usr/bin:/bin" HOME="$WORK/home" TERM=dumb \
        GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
        GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t \
        GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t \
        RUSTMC_REMOTE=fakehost RUSTMC_REMOTE_DIR=rustmc-state-falsify-gate \
        RUSTMC_GATE_LOCK_DIR="$WORK/gatelock" \
        "$@" ./ci/handoff.sh $br 2>&1 )
}

# The word a consumer names: the first line whose first field is all-caps.
# Written this way rather than as `head -1` because `head -1` is what made the
# 2026-08-16 verdict unreadable — git's own stderr got there first.
word_of() {
    local w
    while IFS= read -r line; do
        w="${line%% *}"
        case "$w" in
            [A-Z][A-Z-]*) case "$w" in *[!A-Z-]*) ;; *) printf '%s' "$w"; return ;; esac ;;
        esac
    done <<< "$1"
    printf 'unnamable'
}

case_is() {   # case_is <name> <want-word> <want-rc> <repo> <branch> [env...]
    local name="$1" ww="$2" wr="$3" R="$4" br="$5"; shift 5
    local out rc w
    out="$(run "$R" "$br" "$@")"; rc=$?
    w="$(word_of "$out")"
    if [ "$w" = "$ww" ] && [ "$rc" = "$wr" ]; then ok "$name" "$w" "$rc"
    else bad "$name" "$w" "$rc" "$ww" "$wr"; printf '%s\n' "$out" | sed 's/^/          /'; fi
    [ -n "$VERBOSE" ] && printf '%s\n' "$out" | sed 's/^/          | /'
    return 0
}

echo
echo "[A] the arguments"
R="$(build args)"
case_is "no branch given"        REFUSED 2 "$R" ""
case_is "no such branch"         REFUSED 2 "$R" "no-such-branch-here"
case_is "gate dir is ~/rustmc"   REFUSED 2 "$R" "under-test" RUSTMC_REMOTE_DIR=rustmc
case_is "job cap not a number"   REFUSED 2 "$R" "under-test" "CARGO_BUILD_JOBS=6; rm -rf ~"
case_is "reference cache is not 0/1" REFUSED 2 "$R" "under-test" RUSTMC_REFERENCE_CACHE=yes
case_is "push retry gaps not 0"  REFUSED 2 "$R" "under-test" RUSTMC_PUSH_RETRY_GAPS=99
#
# A FLAG WHOSE ABSENCE IS THE DESTRUCTIVE OPTION MUST NOT FAIL OPEN. Every one
# of these silently merged to main before: the flag was sniffed at position 2
# and anything else was ignored. Nothing in the output would have said so — a
# real run and a dry run differ only in the verdict at the END, by which point
# the merge has happened.
case_is "--dryrun typo"          REFUSED 2 "$R" "under-test --dryrun"
case_is "--dry-run=1"            REFUSED 2 "$R" "under-test --dry-run=1"
case_is "-n instead"             REFUSED 2 "$R" "under-test -n"
case_is "flag before the branch" REFUSED 2 "$R" "--dry-run under-test"
case_is "a third argument"       REFUSED 2 "$R" "under-test --dry-run --force"

echo
echo "[B] the tree this gate is driven from"
R="$(build dirty)"
echo scribble > "$R/untracked-scribble"
case_is "working tree dirty"     REFUSED 2 "$R" "under-test"

# A SCRATCH WORKTREE INSIDE THE GATE'S OWN CHECKOUT. 2026-08-18: a branch was
# refused here for exactly this, and the message sent the reader looking for
# half-written source. The condition was right, the diagnosis was not, and the
# remedy it gave — commit or stash — does not apply to a nested repository.
R="$(build nested)"
git -C "$R" worktree add -q --detach "$R/scratch-inside" 2>/dev/null
OUT="$(run "$R" under-test)"; RC=$?
W="$(word_of "$OUT")"
if [ "$W" = REFUSED ] && [ "$RC" = 2 ]; then
    if printf '%s\n' "$OUT" | grep -c 'carries its own .git' >/dev/null; then
        ok "nested worktree is named" "$W" "$RC"
    else
        bad "nested worktree is named" "$W" "$RC" "REFUSED naming the nested worktree" 2
        printf '%s\n' "$OUT" | sed 's/^/          /'
    fi
else
    bad "nested worktree is named" "$W" "$RC" REFUSED 2
fi
git -C "$R" worktree remove --force "$R/scratch-inside" 2>/dev/null

R="$(build nolock)"
rm -f "$R/ci/gate-lock.sh"
case_is "gate-lock.sh missing"   REFUSED 2 "$R" "under-test"

echo
echo "[B2] the log the verdict is read out of"
#
# `$T0_GREEN`, `$T1_GREEN`, `$T2_GREEN` and `$SKIPS` are all `grep -c` over the
# gate log, and a grep of a file that does not exist counts 0. So a gate whose
# six stages every one passed reported
#
#     RED  <branch> — sync=0 tables=0 t0=0/0 t1=0/0 t2=0/0
#
# with every exit code zero and no cause named. A healthy branch marked bad
# because `$HOME/rustmc-private` was not a directory. INFRA is exactly the word
# for it: the instrument failed, not a stage and not the branch.
#
# The fixture makes `$HOME/rustmc-private` a regular FILE, so `mkdir -p` cannot
# create the gates directory under it.
R="$(build logdir)"
rm -rf "$WORK/home-nolog"; mkdir -p "$WORK/home-nolog"
: > "$WORK/home-nolog/rustmc-private"
OUT="$( cd "$R" && env -i PATH="/usr/bin:/bin" HOME="$WORK/home-nolog" TERM=dumb \
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
    RUSTMC_REMOTE=fakehost RUSTMC_REMOTE_DIR=rustmc-state-falsify-gate \
    RUSTMC_GATE_LOCK_DIR="$WORK/gatelock" ./ci/handoff.sh under-test 2>&1 )"
RC=$?
W="$(word_of "$OUT")"
if [ "$W" = INFRA ] && [ "$RC" = 4 ]; then ok "unwritable gate log" "$W" "$RC"
else bad "unwritable gate log" "$W" "$RC" INFRA 4; printf '%s\n' "$OUT" | sed 's/^/          /'; fi

echo
echo "[C] the shape of the branch"
R="$(build shape)"
# THE STATE THAT CANNOT BE FORCED, AND WHY IT IS RECORDED HERE RATHER THAN
# TESTED. `AHEAD` is `git rev-list --count main..$BRANCH`, which is 0 if and
# only if `$BRANCH` is an ancestor of `main`. So "0 ahead but not merged" does
# not exist: an empty branch off main IS an ancestor, and an orphan branch
# counts its own root as ahead and lands on `behind main` instead. The refusal
# `ci/handoff.sh` keeps for that combination is an assertion about its own
# arithmetic, not a reachable state, and no fixture in this file can force it.
#
# What CAN be asserted is the equivalence the assertion rests on, so this case
# is that: an orphan branch is not 0-ahead, and the gate does not silently call
# it "nothing to merge".
( cd "$R" && git checkout -q --orphan orphan-work && git rm -rq --cached . 2>/dev/null; \
  rm -f file.txt; echo orphan > orphan.txt && git add orphan.txt \
  && git -c user.name=t -c user.email=t@t commit -qm "an unrelated root" \
  && git checkout -qf main ) >/dev/null 2>&1
OUT="$(run "$R" orphan-work)"
if printf '%s\n' "$OUT" | grep -c 'nothing to merge' >/dev/null; then
    bad "orphan is not 'nothing to merge'" "$(word_of "$OUT")" "?" "any other refusal" "any"
else
    ok "orphan is not 'nothing to merge'" "$(word_of "$OUT")" "not-0-ahead"
fi
( cd "$R" && git checkout -q main && git merge -q --ff-only under-test ) >/dev/null 2>&1
case_is "already merged"         ALREADY-MERGED 0 "$R" "under-test"

R="$(build behind)"
( cd "$R" && git checkout -q main && echo m > m.txt && git add m.txt \
  && git -c user.name=t -c user.email=t@t commit -qm "main moved ahead" ) >/dev/null 2>&1
case_is "behind main"            NOT-FF  3 "$R" "under-test"

echo
echo "[C2] the checkout that reports failure and exits 0"
#
# Measured, not assumed: with a directory the process cannot write into,
# `git checkout <branch>` prints `error: unable to create file ...` and
# `Switched to branch ...`, and **exits 0**. The `|| REFUSED` guard on that line
# therefore could not fire in the failure it most plausibly guards against, and
# the gate would rsync a tree missing a file the branch adds and gate content
# that is in no commit.
#
# The fixture is a directory chmod 500. The tree stays clean to
# `git status --porcelain` beforehand — a directory's mode is not tracked — so
# nothing above this refuses first, which is what makes it reachable.
R="$(build ckout)"
( cd "$R" && git checkout -q under-test && mkdir -p sub && echo b > sub/b.txt \
  && git add sub/b.txt && git -c user.name=t -c user.email=t@t commit -qm "the branch adds a file in sub/" \
  && git checkout -q main && mkdir -p sub && chmod 500 sub ) >/dev/null 2>&1
case_is "checkout exits 0 and lies" REFUSED 2 "$R" "under-test"
chmod 700 "$R/sub" 2>/dev/null

echo
echo "[D] another worktree is holding the branch"
#
# The refusal that cost three gate launches on 2026-08-06, and the one whose
# whole argument in `ci/handoff.sh` is that it must be REFUSED and never RED —
# it says nothing about the branch. Nothing has ever driven either half of it.
R="$(build held)"
git -C "$R" worktree add -q --checkout "$WORK/held-clean" under-test 2>/dev/null
# clean holder: the gate detaches it and carries on, so it must NOT refuse here
OUT="$(run "$R" under-test)"; RC=$?
if printf '%s\n' "$OUT" | grep -c 'released under-test from the clean worktree' >/dev/null; then
    ok "clean holder is detached" "handoff:" "$RC"
else
    bad "clean holder is detached" "$(word_of "$OUT")" "$RC" "a release line" "any"
    printf '%s\n' "$OUT" | sed 's/^/          /'
fi

R="$(build helddirty)"
git -C "$R" worktree add -q --checkout "$WORK/held-dirty" under-test 2>/dev/null
echo live-agent-work > "$WORK/held-dirty/half-written.rs"
case_is "dirty holder refuses"   REFUSED 2 "$R" "under-test"

R="$(build heldgone)"
git -C "$R" worktree add -q --checkout "$WORK/held-gone" under-test 2>/dev/null
rm -rf "$WORK/held-gone"
#
# A worktree whose directory is gone still HOLDS its branch — git calls it
# prunable, not gone — and `ci/handoff.sh`'s `-ef` test is false for a path that
# does not exist, deliberately, so this reaches the unreadable-branch refusal
# rather than being skipped. That comment has been in the file since 2026-08-06
# and nothing has ever executed the line it describes.
case_is "vanished holder refuses" REFUSED 2 "$R" "under-test"

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
echo "HANDOFF-STATE GREEN"
