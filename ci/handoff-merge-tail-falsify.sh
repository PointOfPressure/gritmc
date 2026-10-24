#!/usr/bin/env bash
# ci/handoff-merge-tail-falsify.sh — the terminal states BELOW the verdict.
#
# `ci/handoff-verdict-selftest.sh` drives every scenario with `--dry-run`. That
# is not an oversight in it — it is a boundary, and this file is the other side
# of the boundary. `--dry-run` returns at the `GREEN` line, so **not one of that
# file's cases has ever executed `release_worktree main`, `git checkout main`,
# `git merge --ff-only`, or `git push`.** Four terminal states, the four that
# can leave `main` and `origin/main` disagreeing, and nothing exercised any of
# them. A verdict test that stops one line before the state changes is a test of
# the half of the script that cannot damage anything.
#
# WHY THAT HALF IS THE DANGEROUS ONE. Everything above the `GREEN` line either
# merges or does not, and a wrong verdict there costs one branch. Everything
# below it has already changed `main`. A failure there is not confined to the
# branch it names: `ci/handoff.sh` computes `BEHIND` against **local** `main`,
# so a local `main` that is one commit ahead of `origin/main` makes every
# subsequent branch in the queue read `N behind main` and refuse. That happened
# on 2026-08-16 — one push timed out, and the branches behind it were reported
# as needing a rebase they did not need.
#
# WHAT IS REAL HERE AND WHAT IS A STAND-IN, stated plainly because a falsifier
# that fakes the thing under test proves nothing:
#
#   * REAL — `ci/handoff.sh` unmodified, `ci/gate-lock.sh` unmodified, a real
#     git repository, and a real `origin` (a bare repo on disk). The merge is a
#     real `git merge --ff-only` and the push is a real `git push`. Every
#     terminal state below is reached by making the real operation fail, never
#     by injecting a return code.
#   * FAKE — the box. Stages 1-6 are stand-ins that report green, because this
#     file is not about whether a gate can measure a branch; it is about what
#     the script does after it has.
#
# THE CONTRACT THIS ASSERTS, which is what makes it falsifiable rather than
# descriptive. Every terminal below the verdict must emit a word that
#
#   1. a consumer can name — first non-diagnostic line, one of the declared
#      words, never git's own stderr;
#   2. is not a PREFIX of another verdict word. `MERGED-NOT-PUSHED` is matched
#      by `grep -E '^MERGED'`, so the one state that means "main is ahead of
#      origin and the queue is about to wedge" was namable as the state that
#      means "everything worked";
#   3. carries an exit code that means what the header table says it means.
#      `MERGED-NOT-PUSHED` exits 2, and 2 is documented as "refused (state), the
#      gate could not run" — the opposite of a gate that ran, passed, and
#      merged.
#
# Run:  ./ci/handoff-merge-tail-falsify.sh [-v]
set -uo pipefail

VERBOSE=""
[ "${1:-}" = "-v" ] && VERBOSE=1

CI="$(cd "$(dirname "$0")" && pwd)"
for f in handoff.sh gate-lock.sh; do
    [ -r "$CI/$f" ] || { echo "no ci/$f beside this script"; exit 2; }
done

WORK="$(mktemp -d)"
# The bare origin is made unreadable in one case; chmod it back or the cleanup
# leaves a directory nobody can remove.
trap 'chmod -R u+rwX "$WORK" 2>/dev/null; rm -rf "$WORK"' EXIT
PASS=0
FAIL=0
: > "$WORK/first-lines.txt"

# --- the declared verdict vocabulary ----------------------------------------
# Kept here as data rather than as prose so the prefix rule below is a check
# and not a promise. A new verdict word added to `ci/handoff.sh` without being
# added here fails `unnamable`; one added here that collides with another fails
# `prefix collision`.
VERDICTS="MERGED ALREADY-MERGED RED REFUSED NOT-FF INFRA GREEN UNPUSHED"

# --- the throwaway world -----------------------------------------------------
# HOME is outside the repo for the reason `ci/handoff-verdict-selftest.sh`
# records: the gate's first action is to create `$HOME/rustmc-private/gates`,
# and inside the checkout that makes the tree dirty and the gate refuses itself.
build_world() {
    local R="$WORK/repo"
    rm -rf "$R" "$WORK/origin.git" "$WORK/home" "$WORK/held"
    mkdir -p "$R/ci" "$WORK/home"
    git init -q -b main "$R"
    git init -q --bare "$WORK/origin.git"
    cp "$CI/handoff.sh" "$CI/gate-lock.sh" "$R/ci/"
    chmod +x "$R/ci/handoff.sh" "$R/ci/gate-lock.sh"

    # The laptop-side driver: stage 2 and stage 4, both green. This file does
    # not test the stages.
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
      && git add ci \
      && git -c user.name=t -c user.email=t@t commit -qm "gate scripts" \
      && git remote add origin "$WORK/origin.git" \
      && git push -q origin main \
      && git checkout -q -b under-test \
      && echo change > file.txt && git add file.txt \
      && git -c user.name=t -c user.email=t@t commit -qm "the branch under test" \
      && git push -q origin under-test \
      && git checkout -q main ) >/dev/null 2>&1
    echo "$R"
}

# Every route to the box is ssh and every one of them reports green.
build_ssh() {
    mkdir -p "$WORK/bin" "$WORK/shim"
    cat > "$WORK/bin/ssh" <<'EOS'
#!/usr/bin/env bash
cmd="${*: -1}"
case "$cmd" in
*"bash -s"*)
    # Stage 1 and the provenance census both arrive as `bash -s` with the
    # script on stdin, so the command string cannot tell them apart. Read the
    # payload and look at it — which is also the only place this file can
    # synchronise with the gate deterministically, because the census is the
    # last thing that happens before the merge.
    payload="$(cat)"
    case "$payload" in
    *rustmc-gate-provenance*)
        [ -n "${TAIL_FALSIFY_BEFORE_MERGE:-}" ] && [ -x "$TAIL_FALSIFY_BEFORE_MERGE" ] \
            && "$TAIL_FALSIFY_BEFORE_MERGE" >/dev/null 2>&1
        echo "gate-dir-prev: none — this box directory has never been stamped"
        echo "gate-dir-prev-tip="
        echo "compile-census: 104 artefacts written under target/"
        exit 0 ;;
    *)  echo "== stage 1: gate dir owns its own oracle-work and data/vanilla"
        echo "   12 references linked, gameplay is a directory"
        echo "STAGE1 OK"; exit 0 ;;
    esac ;;
*gen-gameplay-tables.sh*) echo "gen-gameplay-tables: 12 tables current"; exit 0 ;;
*t1-gate.sh*)             echo "T1-GATE GREEN — 99 test binaries"; exit 0 ;;
*gate-corpus-stages.sh*) echo "T2-GAMEPLAY GREEN"; echo "GATE-STAGE-RC 6 0"; exit 0 ;;
*t2-gameplay.sh*)         echo "T2-GAMEPLAY GREEN"; exit 0 ;;
esac
echo "fake ssh: unrecognised command: $cmd" >&2; exit 127
EOS
    chmod +x "$WORK/bin/ssh"
}

# A `git` shim, empty by default, so a case can make ONE real git operation
# behave the way the network makes it behave. It execs the real git for
# everything else: a shim that answered more than the one call would be testing
# itself. `$WORK/shim` is first on PATH and is cleared between cases.
shim_git_push_hangs() {
    rm -f "$WORK/push-entered"
    cat > "$WORK/shim/git" <<EOS
#!/usr/bin/env bash
# \`git push origin main\` never returns — a TCP connection that is open and
# silent, which is what a timing-out GitHub looks like from here. Everything
# else is the real git.
#
# The hang announces itself: \`$WORK/push-entered\` holds the pid of the
# sleep that IS the hung push, written before it starts. That is the marker
# case [4] synchronises its kill on — the gate has returned from
# \`git merge --ff-only\` and entered the push — and the pid is what it kills,
# rather than every \`sleep 600\` on the box.
for a in "\$@"; do [ "\$a" = push ] && { echo \$\$ > "$WORK/push-entered.tmp"; mv "$WORK/push-entered.tmp" "$WORK/push-entered"; exec sleep 600; }; done
exec /usr/bin/git "\$@"
EOS
    chmod +x "$WORK/shim/git"
}
shim_clear() { rm -f "$WORK/shim/git"; }

# `env -i` so nothing from the caller's shell reaches the gate; git identity is
# supplied explicitly because there is no global config in that environment.
gate() {
    local R="$1"; shift
    ( cd "$R" && env -i \
        PATH="$WORK/shim:$WORK/bin:/usr/bin:/bin" HOME="$WORK/home" TERM=dumb \
        GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
        GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t \
        GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t \
        RUSTMC_REMOTE=fakehost RUSTMC_REMOTE_DIR=rustmc-tail-falsify-gate \
        RUSTMC_GATE_LOCK_DIR="$WORK/gatelock" \
        RUSTMC_PUSH_RETRY_GAPS=0 \
        TAIL_FALSIFY_BEFORE_MERGE="${TAIL_FALSIFY_BEFORE_MERGE:-}" \
        "$@" ./ci/handoff.sh "${GATE_BRANCH:-under-test}" ${GATE_ARGS:-} 2>&1 | tee "$WORK/last.out"
      # PIPESTATUS is read on the very next line or it is the status of
      # whatever ran after the pipeline instead. The tally below must not be
      # able to change the exit code every case asserts on.
      _rc=${PIPESTATUS[0]}
      head -1 "$WORK/last.out" >> "$WORK/first-lines.txt"
      exit "$_rc" )
}

ok()   { PASS=$((PASS + 1)); printf '  ok    %-28s %s\n' "$1" "${2:-}"; }
bad()  { FAIL=$((FAIL + 1)); printf '  FAIL  %-28s %s\n' "$1" "${2:-}"; }

# The word a consumer would name. Diagnostic noise from git and ssh goes to the
# same stream, so a consumer cannot simply take line one — it takes the first
# line that starts with a declared verdict word. This helper IS the consumer
# model, and `unnamable` is the case that fires when no line qualifies.
verdict_word() {
    local out="$1" w
    while IFS= read -r line; do
        w="${line%% *}"
        case " $VERDICTS " in *" $w "*) printf '%s' "$w"; return 0 ;; esac
    done <<< "$out"
    printf 'unnamable'
}

build_ssh

echo
echo "[0] the vocabulary itself: no verdict word may be a prefix of another"
#
# This is the whole of the `MERGED-NOT-PUSHED` defect, as a property rather
# than an anecdote. Any consumer that classifies with `grep -E '^MERGED'` — and
# an anchored prefix match is the obvious way to write it — reports the wedge
# state as a successful merge.
COLLIDE=""
for a in $VERDICTS; do
    for b in $VERDICTS; do
        [ "$a" = "$b" ] && continue
        case "$b" in "$a"*) COLLIDE="$COLLIDE $a<$b" ;; esac
    done
done
if [ -z "$COLLIDE" ]; then ok "no prefix collision" "$(echo $VERDICTS | wc -w) words"
else bad "no prefix collision" "collides:$COLLIDE"; fi

echo
echo "[1] THE CONTROL — a green gate merges, pushes, and says so"
R="$(build_world)"
OUT="$(gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
[ "$W" = MERGED ] && [ "$RC" = 0 ] \
    && ok "green merges" "MERGED exit 0" \
    || bad "green merges" "got $W exit $RC (wanted MERGED / 0)"
LOCAL="$(git -C "$R" rev-parse main 2>/dev/null)"
REMOTE="$(git -C "$WORK/origin.git" rev-parse main 2>/dev/null)"
[ -n "$LOCAL" ] && [ "$LOCAL" = "$REMOTE" ] \
    && ok "MERGED means origin has it" "${LOCAL:0:8}" \
    || bad "MERGED means origin has it" "local ${LOCAL:0:8} origin ${REMOTE:0:8}"
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo
echo "[2] THE PARTIAL FAILURE — merged locally, the push did not land"
#
# The 2026-08-16 shape. The merge is real and has happened; only the push
# fails. What the gate says here decides whether one transient network failure
# costs one branch or the whole queue.
R="$(build_world)"
mv "$WORK/origin.git" "$WORK/origin.unreachable"
OUT="$(gate "$R")"; RC=$?
mv "$WORK/origin.unreachable" "$WORK/origin.git"
W="$(verdict_word "$OUT")"
[ "$W" = UNPUSHED ] \
    && ok "push failed: the word" "UNPUSHED" \
    || bad "push failed: the word" "got '$W' — wanted UNPUSHED, a word no consumer can read as MERGED"
[ "$RC" = 5 ] \
    && ok "push failed: the exit" "5" \
    || bad "push failed: the exit" "got $RC — wanted 5; 2 means 'the gate could not run', and it ran"
printf '%s\n' "$OUT" | grep -cE '^MERGED' >/dev/null \
    && bad "push failed: not namable as MERGED" "a '^MERGED' consumer calls this a successful merge" \
    || ok "push failed: not namable as MERGED" "'^MERGED' does not match"
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo
echo "[3] the wedge is detected at ENTRY, before another branch is blamed"
#
# Case [2] left `main` ahead of `origin/main`. What the NEXT gate does with
# that is the whole of the 2026-08-16 outage. Before this, it said
# `REFUSED  <branch> is 1 behind main — not a fast-forward, rebase it` and
# exited 3: the wrong branch named, an instruction that does not help, and the
# same answer for every branch behind it in the queue.
#
# There are two shapes and they want different answers, so both are driven.
( cd "$R" && git checkout -q -b queued-behind origin/main 2>/dev/null \
  && echo other > other.txt && git add other.txt \
  && git -c user.name=t -c user.email=t@t commit -qm "the next branch in the queue" \
  && git checkout -q main ) >/dev/null 2>&1
AHEAD_OF_ORIGIN="$(git -C "$R" rev-list --count origin/main..main 2>/dev/null || echo 0)"
if [ "${AHEAD_OF_ORIGIN:-0}" -lt 1 ]; then
    bad "wedge fixture" "case [2] did not leave main ahead of origin/main; nothing to detect"
else
    ok "wedge fixture" "main is $AHEAD_OF_ORIGIN ahead of origin/main, a branch is queued behind it"

    # 3a — origin is reachable again. The gate should finish the job the
    # previous run could not: push the merge it already authorised, say so, and
    # carry on. Only then is "you are behind main" honest advice, because only
    # then has main actually moved anywhere the branch's author can see.
    OUT="$(GATE_BRANCH=queued-behind gate "$R")"; RC=$?
    if printf '%s\n' "$OUT" | grep -c 'ahead of origin/main\|repaired\|pushing it now' >/dev/null; then
        ok "3a wedge: named at entry" "the unpushed main is reported before anything else"
    else
        bad "3a wedge: named at entry" "exit $RC — the queued branch was judged without the wedge being mentioned"
    fi
    NOW_AHEAD="$(git -C "$R" rev-list --count origin/main..main 2>/dev/null || echo '?')"
    [ "$NOW_AHEAD" = 0 ] \
        && ok "3a wedge: repaired" "origin/main now carries the merge" \
        || bad "3a wedge: repaired" "main is still $NOW_AHEAD ahead of origin/main"
    [ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

    # 3b — the wedge with origin STILL unreachable, which is the live outage
    # rather than the morning after. Nothing can be repaired, so the gate must
    # name the unpushed main and stop, and must NOT tell this branch to rebase
    # onto a main that origin has never seen.
    R2="$(build_world)"
    mv "$WORK/origin.git" "$WORK/origin.unreachable"
    GATE_BRANCH=under-test gate "$R2" >/dev/null 2>&1     # wedges it
    ( cd "$R2" && git checkout -q -b queued-behind2 HEAD~1 2>/dev/null \
      && echo z > z.txt && git add z.txt \
      && git -c user.name=t -c user.email=t@t commit -qm "queued behind the wedge" \
      && git checkout -q main ) >/dev/null 2>&1
    OUT="$(GATE_BRANCH=queued-behind2 gate "$R2")"; RC=$?
    mv "$WORK/origin.unreachable" "$WORK/origin.git"
    W="$(verdict_word "$OUT")"
    [ "$W" = UNPUSHED ] && [ "$RC" = 5 ] \
        && ok "3b wedge: unrepairable is named" "UNPUSHED exit 5" \
        || bad "3b wedge: unrepairable is named" "got $W exit $RC — wanted UNPUSHED / 5"
    printf '%s\n' "$OUT" | grep -cE 'behind main — not a fast-forward, rebase it' >/dev/null \
        && bad "3b wedge: no false rebase advice" "told to rebase onto a main origin does not have" \
        || ok "3b wedge: no false rebase advice" ""
    [ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'
fi

echo "[4] killed between the merge and the push"
#
# SIGKILL, so no trap runs and nothing prints a verdict of any kind. The gate
# leaves `main` merged and unpushed and says nothing at all — the same state
# case [2] reaches by a named route, reached here by an unnamed one. Only the
# NEXT run can notice, which is why case [3]'s entry check is what makes this
# recoverable rather than a second wedge.
#
# The push is made to hang rather than to fail, because a kill has to land
# inside the window between the merge and the push and a window that closes in
# microseconds cannot be aimed at. A hung push is also the more faithful
# reproduction: the 2026-08-16 failure was a timeout, not a refusal.
#
# **The kill is synchronised on the push being ENTERED, not on main moving.**
# It used to fire the moment `main` contained the branch, polled every 250 ms.
# `git merge --ff-only` moves the ref before it has finished updating the
# index and work tree, so on a loaded box the kill could land inside the merge
# itself, leave its `index.lock` behind, and the re-run then refused the tree
# — `got REFUSED exit 2` on structures-3's T0 (~/rustmc-sl2-logs/
# t0-t0tip2.log:430-431), 37/37 alone a minute later. That is a kill in the
# middle of git, not between the merge and the push, which is what this case
# names. The hung push writes a marker before it hangs (see
# `shim_git_push_hangs`), so the kill now lands after the merge has returned
# and inside the push, every run. If the tail stops being safe — a re-run that
# cannot recognise the merged branch or does not push it — the assertions
# below still go red.
R="$(build_world)"
shim_git_push_hangs
( gate "$R" >"$WORK/killed.out" 2>&1 ) &
GPID=$!
for _ in $(seq 1 400); do
    [ -s "$WORK/push-entered" ] && break
    kill -0 "$GPID" 2>/dev/null || break
    sleep 0.25
done
MERGED_LOCALLY=0
git -C "$R" merge-base --is-ancestor under-test main 2>/dev/null && [ -s "$WORK/push-entered" ] && MERGED_LOCALLY=1
HUNG="$(cat "$WORK/push-entered" 2>/dev/null)"
pkill -9 -P "$GPID" 2>/dev/null
kill -9 "$GPID" 2>/dev/null
wait "$GPID" 2>/dev/null
[ -n "$HUNG" ] && kill -9 "$HUNG" 2>/dev/null
shim_clear
if [ "$MERGED_LOCALLY" != 1 ]; then
    bad "kill fixture" "the gate never reached the merge, so the window was not entered"
else
    ORIGIN_HAS="$(git -C "$WORK/origin.git" rev-parse main 2>/dev/null)"
    LOCAL_IS="$(git -C "$R" rev-parse main 2>/dev/null)"
    if [ "$ORIGIN_HAS" = "$LOCAL_IS" ]; then
        bad "kill fixture" "the push completed anyway; the window was not entered"
    else
        ok "killed mid-merge: fixture" "main carries the branch, origin does not"
        # The recovery is the assertion. A re-run must not report the killed
        # branch as `0 commits ahead of main — nothing to merge`, which is true
        # and useless, and must not leave origin behind for the next branch.
        OUT="$(gate "$R")"; RC=$?
        W="$(verdict_word "$OUT")"
        # The branch IS merged — the kill landed after `git merge --ff-only`.
        # So the honest answer is that it is already in main, not that there is
        # "nothing to merge", and origin must not be left behind.
        [ "$W" = ALREADY-MERGED ] && [ "$RC" = 0 ] \
            && ok "killed mid-merge: recognised" "ALREADY-MERGED exit 0" \
            || bad "killed mid-merge: recognised" "got $W exit $RC — wanted ALREADY-MERGED / 0"
        LEFT="$(git -C "$R" rev-list --count origin/main..main 2>/dev/null || echo '?')"
        [ "$LEFT" = 0 ] \
            && ok "killed mid-merge: origin caught up" "" \
            || bad "killed mid-merge: origin caught up" "main is still $LEFT ahead of origin/main"
        [ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'
    fi
fi

echo "[5] main moved under a green gate — the ff-merge cannot happen"
#
# The entry check proved `main` was an ancestor of the branch, so if the merge
# still refuses, `main` moved WHILE the gate was running. That is a statement
# about the queue, not about the branch, and it must not read as a refusal
# that happened before the gate started: the whole gate was paid for and the
# branch came back green.
#
# The move is timed off the provenance census inside the fake ssh — the last
# thing the gate does before the merge — so this is a deterministic
# reproduction rather than a race against a sleep.
R="$(build_world)"
cat > "$WORK/bin/movemain" <<EOS
#!/usr/bin/env bash
# a third party lands on main while the gate is running
cd "$R" || exit 0
/usr/bin/git checkout -q --detach main 2>/dev/null
echo interloper > interloper.txt
/usr/bin/git add interloper.txt
/usr/bin/git -c user.name=t -c user.email=t@t commit -qm "a third party landed on main"
/usr/bin/git branch -qf main HEAD
EOS
chmod +x "$WORK/bin/movemain"
OUT="$(TAIL_FALSIFY_BEFORE_MERGE="$WORK/bin/movemain" gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
if printf '%s\n' "$OUT" | grep -cE '^REFUSED  ff-merge failed after a green gate$' >/dev/null; then
    bad "main moved: names the cause" "still the bare 'REFUSED  ff-merge failed after a green gate' — reads as a refusal from before the gate ran"
elif [ "$W" = unnamable ]; then
    bad "main moved: names the cause" "unnamable, exit $RC"
else
    ok "main moved: names the cause" "$W exit $RC"
fi
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo
echo "[6] the two refusals that cost a gate that had already passed"
#
# Both fire between the green verdict and the merge, and before this neither had
# ever been executed. They are the cheap half of the merge tail — nothing is
# merged, nothing is pushed, `main` and `origin/main` still agree — but a
# consumer cannot tell that from a refusal that reads identically to one from
# before the gate started, and the difference is fifty minutes.

# 6a — a second worktree is holding `main`, and it is dirty. The same function
# and the same argument as the branch-side release: a tree an agent may still be
# live in is not a tree to move HEAD in. It happens here after the whole gate.
R="$(build_world)"
( cd "$R" && git checkout -q --detach main ) >/dev/null 2>&1
git -C "$R" worktree add -q --checkout "$WORK/main-holder" main 2>/dev/null
echo half-written > "$WORK/main-holder/agent-work-in-progress.rs"
OUT="$(gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
[ "$W" = REFUSED ] && [ "$RC" = 2 ] \
    && ok "main held by a dirty worktree" "REFUSED exit 2" \
    || bad "main held by a dirty worktree" "got $W exit $RC (wanted REFUSED / 2)"
printf '%s\n' "$OUT" | grep -c 'after a full green gate' >/dev/null \
    && ok "  and it says the gate passed" "" \
    || bad "  and it says the gate passed" "reads as a refusal from before the gate started"
git -C "$R" merge-base --is-ancestor under-test main 2>/dev/null \
    && bad "  and nothing was merged" "the branch reached main anyway" \
    || ok "  and nothing was merged" ""
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'
git -C "$R" worktree remove --force "$WORK/main-holder" 2>/dev/null

# 6b — `git checkout main` reports a failure and exits 0. Measured: with a
# directory the process cannot write into, git prints `error: unable to create
# file ...`, prints `Switched to branch ...`, and returns 0. After a green gate
# that would run `git merge --ff-only` in a tree that is not at main.
#
# `sub/` has two files on main and the branch deletes one, so the directory
# survives the checkout of the branch and can be made unwritable by the
# pre-merge hook — which runs on the provenance census, the last thing before
# the merge.
R="$(build_world)"
( cd "$R" && git checkout -q main && mkdir -p sub && echo a > sub/a.txt && echo k > sub/keep.txt \
  && git add sub && git -c user.name=t -c user.email=t@t commit -qm "main has sub/a.txt and sub/keep.txt" \
  && git push -q origin main \
  && git checkout -q under-test && git rebase -q main && git rm -q sub/a.txt \
  && git -c user.name=t -c user.email=t@t commit -qm "the branch removes sub/a.txt" \
  && git checkout -q main ) >/dev/null 2>&1
cat > "$WORK/bin/lockdir" <<EOS
#!/usr/bin/env bash
chmod 500 "$R/sub"
EOS
chmod +x "$WORK/bin/lockdir"
OUT="$(TAIL_FALSIFY_BEFORE_MERGE="$WORK/bin/lockdir" gate "$R")"; RC=$?
chmod 700 "$R/sub" 2>/dev/null
W="$(verdict_word "$OUT")"
[ "$W" = REFUSED ] && [ "$RC" = 2 ] \
    && ok "checkout of main exits 0 and lies" "REFUSED exit 2" \
    || bad "checkout of main exits 0 and lies" "got $W exit $RC (wanted REFUSED / 2)"
git -C "$R" merge-base --is-ancestor under-test main 2>/dev/null \
    && bad "  and nothing was merged" "merged from a tree that was not main" \
    || ok "  and nothing was merged" ""
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo "[7] --dry-run changes nothing, including the wedge it finds"
#
# `ci/lanes.sh` drives every lane with `--dry-run`: it does its own merge under
# its own sequencer lock, with its own `origin/main` freshness check, in a lane
# clone whose `origin` is the real remote. The entry repair added for case [3]
# must therefore not fire there — it would push `main` from a process whose
# whole contract is that it changes nothing, underneath a driver already
# managing that exact question.
#
# It must also not become a VERDICT under --dry-run. A lane's local `main` is
# legitimately ahead of `origin/main` for the moments between one lane merging
# and pushing, and refusing there would turn a normal drain into a queue of
# refusals — which is the failure this check was written to end, reintroduced
# by the fix for it.
R="$(build_world)"
mv "$WORK/origin.git" "$WORK/origin.unreachable"
gate "$R" >/dev/null 2>&1                       # wedge it: merged, not pushed
mv "$WORK/origin.unreachable" "$WORK/origin.git"
BEFORE="$(git -C "$WORK/origin.git" rev-parse main)"
( cd "$R" && git checkout -q -b dry-probe main && echo d > d.txt && git add d.txt \
  && git -c user.name=t -c user.email=t@t commit -qm "a branch to dry-run" \
  && git checkout -q main ) >/dev/null 2>&1
OUT="$(GATE_BRANCH=dry-probe GATE_ARGS=--dry-run gate "$R")"; RC=$?
AFTER="$(git -C "$WORK/origin.git" rev-parse main)"
[ "$BEFORE" = "$AFTER" ] \
    && ok "dry run pushed nothing" "origin/main unmoved" \
    || bad "dry run pushed nothing" "origin/main moved ${BEFORE:0:8} -> ${AFTER:0:8}"
W="$(verdict_word "$OUT")"
[ "$W" = GREEN ] && [ "$RC" = 0 ] \
    && ok "dry run still reaches GREEN" "GREEN exit 0" \
    || bad "dry run still reaches GREEN" "got $W exit $RC — a lane drain would see this as a refusal"
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo "[8] the log removed while the gate is running"
#
# The check at the top of `ci/handoff.sh` proves the log was writable when the
# gate started. It can still go away underneath it — a full disk, a cleanup job,
# a symlink target that moved — and the counters are read at the END. Without
# the second assertion this presents identically to a branch failure: five zero
# counters, every stage's exit code zero, and a RED with no cause.
#
# The removal is timed off the provenance census, which runs after every stage
# and before the counters are read, so this lands inside the window rather than
# racing it.
R="$(build_world)"
cat > "$WORK/bin/eatlog" <<EOS
#!/usr/bin/env bash
rm -f "$WORK/home/rustmc-private/gates/"*.log
EOS
chmod +x "$WORK/bin/eatlog"
OUT="$(TAIL_FALSIFY_BEFORE_MERGE="$WORK/bin/eatlog" gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
[ "$W" = INFRA ] && [ "$RC" = 4 ] \
    && ok "log eaten mid-gate is INFRA" "INFRA exit 4" \
    || bad "log eaten mid-gate is INFRA" "got $W exit $RC — wanted INFRA / 4, not a RED about the branch"
git -C "$R" merge-base --is-ancestor under-test main 2>/dev/null \
    && bad "  and nothing was merged" "merged on a verdict read from a log that was gone" \
    || ok "  and nothing was merged" ""
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo "[9] main and origin/main have BOTH moved — not repairable by pushing"
#
# The entry check repairs a `main` that is strictly ahead of `origin/main`,
# because pushing it changes when those commits leave and never what leaves. A
# real divergence is different in kind: one side is somebody's merge and the
# other side is somebody else's, and nothing in a gate script may pick a winner.
# It must say so and stop, and it must not attempt the push — a `--force` here
# would destroy a merge somebody else's gate had already authorised.
R="$(build_world)"
mv "$WORK/origin.git" "$WORK/origin.unreachable"
gate "$R" >/dev/null 2>&1                       # wedge: merged locally, push failed
mv "$WORK/origin.unreachable" "$WORK/origin.git"
# a second writer lands on origin/main, which local main has never seen
rm -rf "$WORK/other"
git clone -q "$WORK/origin.git" "$WORK/other" 2>/dev/null
( cd "$WORK/other" && echo elsewhere > elsewhere.txt && git add elsewhere.txt \
  && git -c user.name=o -c user.email=o@o commit -qm "another gate merged and pushed" \
  && git push -q origin main ) >/dev/null 2>&1
ORIGIN_BEFORE="$(git -C "$WORK/origin.git" rev-parse main)"
( cd "$R" && git checkout -q -b third-branch main && echo t > t.txt && git add t.txt \
  && git -c user.name=t -c user.email=t@t commit -qm "a third branch" \
  && git checkout -q main ) >/dev/null 2>&1
OUT="$(GATE_BRANCH=third-branch gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
[ "$W" = UNPUSHED ] && [ "$RC" = 5 ] \
    && ok "divergence is named" "UNPUSHED exit 5" \
    || bad "divergence is named" "got $W exit $RC (wanted UNPUSHED / 5)"
printf '%s\n' "$OUT" | grep -ci 'diverged' >/dev/null \
    && ok "  and it says diverged" "" \
    || bad "  and it says diverged" "the word 'diverged' is not in the output; the remedy is different from a plain unpushed main"
[ "$(git -C "$WORK/origin.git" rev-parse main)" = "$ORIGIN_BEFORE" ] \
    && ok "  and origin was not overwritten" "" \
    || bad "  and origin was not overwritten" "the other writer's commit was destroyed"
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo "[10] nothing speaks before the verdict, in ANY of the cases above"
#
# A caller sees stdout and stderr on one stream — `ci/run-detached.sh` merges
# them by design and so does every `2>&1` an operator types — so a failing git
# gets its sentence out first and the verdict arrives underneath it. That is
# what made the 2026-08-16 verdict unreadable: the line was
# `MERGED-NOT-PUSHED  push failed` and the FIRST line of output was
# `fatal: Could not read from remote repository.`
#
# This is asserted over every gate this file ran rather than as a case of its
# own, because the rule is about all of them and a rule tested on one path is a
# rule the next path breaks. `handoff:` lines are allowed: they are this
# script's own advisories, every consumer already skips them, and they carry
# things like the release notice that has to precede the verdict to be true.
BAD_FIRST=0
while IFS= read -r fl; do
    [ -n "$fl" ] || continue
    w="${fl%% *}"
    case " $VERDICTS " in *" $w "*) continue ;; esac
    case "$w" in handoff:) continue ;; esac
    BAD_FIRST=$((BAD_FIRST + 1))
    printf '          first line was: %s\n' "$fl"
done < "$WORK/first-lines.txt"
# `grep -c` PRINTS 0 AND EXITS 1 on no match, so `grep -c . f || echo 0` emits
# "0\n0" and the numeric comparison below fails with a syntax error rather than
# reporting a tally of zero. `ci/batch-falsify.sh:223` records the same shape
# breaking four cases on its first run and reading as a defect in ci/batch.sh.
# `awk END{print NR}` is the form that has one answer.
TOTAL_RUNS="$(awk 'END{print NR}' "$WORK/first-lines.txt" 2>/dev/null || echo 0)"
if [ "$TOTAL_RUNS" -lt 10 ]; then
    bad "first line is a verdict" "only $TOTAL_RUNS gate runs recorded — the tally is not wired up"
elif [ "$BAD_FIRST" = 0 ]; then
    ok "first line is a verdict" "$TOTAL_RUNS gate runs, every one led with a verdict or a handoff: line"
else
    bad "first line is a verdict" "$BAD_FIRST of $TOTAL_RUNS runs led with something a consumer cannot classify"
fi

echo "[11] unpushed-on-branch counts patches, not shas"
#
# The standing rule is to size a branch with `git cherry`, never with
# `git rev-list --count`, because this repo merges by rebase and a rebased
# commit carries an old patch under a new sha. Repo-wide that is 780 naive
# against 453 real.
#
# `+N commits` on the MERGED line is the one figure the rule does NOT bind on,
# and the proof is in `ci/handoff.sh` beside it: the gate refuses unless
# `BEHIND` is 0, so `$BRANCH..main` is empty and `git cherry` has nothing to
# match a patch-id against. This case was originally written to force a
# disagreement there and could not.
#
# `unpushed-on-branch-at-entry` is the figure where it does bind: there is no
# fast-forward guarantee between a local branch and `origin/<branch>`, and a
# rebase-and-force-push — the normal shape of work here — puts the same patches
# on both sides under different shas.
R="$(build_world)"
( cd "$R" && git checkout -q under-test \
  && git commit -q --amend --no-edit -m "the branch under test, reworded" \
  && git checkout -q main ) >/dev/null 2>&1
SHA_N="$(git -C "$R" rev-list --count origin/under-test..under-test)"
CHERRY_N="$(git -C "$R" cherry origin/under-test under-test | grep -c '^+')"
if [ "$SHA_N" = "$CHERRY_N" ]; then
    bad "rebased-branch fixture" "rev-list $SHA_N and cherry $CHERRY_N agree — the fixture built no duplicate patch"
else
    ok "rebased-branch fixture" "rev-list says $SHA_N unpushed, git cherry says $CHERRY_N"
    OUT="$(gate "$R")"; RC=$?
    printf '%s\n' "$OUT" | grep -c "unpushed-on-branch-at-entry=$CHERRY_N" >/dev/null \
        && ok "MERGED reports the patch count" "unpushed-on-branch-at-entry=$CHERRY_N" \
        || bad "MERGED reports the patch count" "wanted $CHERRY_N, the line says: $(printf '%s\n' "$OUT" | grep -o 'unpushed-on-branch-at-entry=[^ ]*')"
    [ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'
fi

echo "[12] a local main that is merely BEHIND origin/main is not a wedge"
#
# The control for the entry check, and it is the case that check got wrong when
# it was first written. `main` behind `origin/main` is the normal state of any
# checkout that has not pulled — in the real repository local `main` was 40
# commits behind at the time — and the first version classified it as a
# divergence, so it would have returned UNPUSHED on the first gate it ever ran.
#
# The gate must carry straight on: `BEHIND` is measured against LOCAL main and
# is the check that has something to say here.
R="$(build_world)"
rm -rf "$WORK/other2"
git clone -q "$WORK/origin.git" "$WORK/other2" 2>/dev/null
( cd "$WORK/other2" && echo ahead > ahead.txt && git add ahead.txt \
  && git -c user.name=o -c user.email=o@o commit -qm "somebody else landed on main" \
  && git push -q origin main ) >/dev/null 2>&1
# local main is untouched, so it is now strictly BEHIND origin/main
OUT="$(gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
# It is caught at ENTRY and costs seconds. Left alone it costs the whole gate
# and then cannot be paid for: BEHIND is local, so the stale base looks fine,
# the gate goes green, the ff-merge succeeds and the PUSH is rejected as a
# non-fast-forward — which is how this case first reported UNPUSHED.
[ "$W" = NOT-FF ] && [ "$RC" = 3 ] \
    && ok "behind origin/main is caught at entry" "NOT-FF exit 3, before any stage ran" \
    || bad "behind origin/main is caught at entry" "got $W exit $RC (wanted NOT-FF / 3)"
printf '%s\n' "$OUT" | grep -ci 'diverged' >/dev/null \
    && bad "  and it is not called a divergence" "a checkout that has not pulled is not a divergence" \
    || ok "  and it is not called a divergence" ""
git -C "$R" merge-base --is-ancestor under-test main 2>/dev/null \
    && bad "  and nothing was merged" "merged against a base origin had moved past" \
    || ok "  and nothing was merged" ""
[ -n "$VERBOSE" ] && printf '%s\n' "$OUT" | sed 's/^/          | /'

echo "[13] the main-writer mutex actually excludes, and refuses rather than merging"
#
# `ci/batch.sh --land` has taken
# `$(git rev-parse --git-common-dir)/rustmc-main-writer.lock` since it was
# written; `ci/handoff.sh` did not take it until 2026-08-19, on the reasoning
# that it was the only writer of `main`. It is not — that is exactly what
# batching makes false, and `ci/batch.sh`'s own header names this as
# precondition 1 for enabling `--land`.
#
# A lock nobody can be shown losing is not a lock. So this holds it from
# another process and asserts three things: the gate REFUSES, it says why in
# words naming the mutex, and — the one that matters — **nothing is merged**.
# Failing open here would be worse than having no lock, because the party that
# does take it is then entitled to believe it is protected.
R="$(build_world)"
# `git rev-parse --git-common-dir` answers RELATIVE to the repo in a main
# checkout — plain `.git` — so it has to be resolved from inside $R. Resolving
# it from here produced `/rustmc-main-writer.lock`, which flock could not open,
# and the case then went red for the right reason with the wrong cause: the
# lock was never held, so the gate merged. Worth keeping in the comment because
# it is the same relative/absolute trap ci/batch.sh's own header warns about.
LOCKF="$(cd "$R" && cd "$(git rev-parse --git-common-dir)" && pwd)/rustmc-main-writer.lock"
: > "$LOCKF"
# A childless holder the kernel reaps if this script dies, same discipline as
# ci/corpus-lock.sh's anchor. `flock` holds until the fd closes.
flock -x "$LOCKF" sleep 120 &
HOLDER=$!
# Do not race the holder: wait until the lock is genuinely unavailable rather
# than sleeping a guessed interval, or this case passes for the wrong reason
# on a slow box.
for _ in $(seq 1 100); do
    flock -x -n -E 99 "$LOCKF" true; [ $? -eq 99 ] && break; sleep 0.1
done
# `gate()` runs under `env -i`, so an assignment in front of the call is wiped.
# It forwards "$@" into the env, which is the supported route.
OUT="$(gate "$R" RUSTMC_MAIN_WRITER_WAIT=2)"; RC=$?
kill "$HOLDER" 2>/dev/null; wait "$HOLDER" 2>/dev/null
W="$(verdict_word "$OUT")"
[ "$W" = REFUSED ] && [ "$RC" = 2 ]     && ok "a held main-writer mutex refuses" "REFUSED exit 2"     || bad "a held main-writer mutex refuses" "got $W exit $RC (wanted REFUSED / 2)"
printf '%s
' "$OUT" | grep -c 'main-writer' >/dev/null     && ok "  and the refusal names the mutex" ""     || bad "  and the refusal names the mutex" "a refusal nobody can act on is a hang with a message"
git -C "$R" merge-base --is-ancestor under-test main 2>/dev/null     && bad "  and NOTHING was merged" "merged main without holding the mutex"     || ok "  and NOTHING was merged" ""
[ -n "$VERBOSE" ] && printf '%s
' "$OUT" | sed 's/^/          | /'

# The control: the same world with the lock free must still merge. Without
# this, a handoff that refused unconditionally would pass every assertion
# above.
R="$(build_world)"
OUT="$(gate "$R")"; RC=$?
W="$(verdict_word "$OUT")"
[ "$W" = MERGED ] && [ "$RC" = 0 ]     && ok "  control: a free mutex still merges" "MERGED exit 0"     || bad "  control: a free mutex still merges" "got $W exit $RC (wanted MERGED / 0)"
[ -n "$VERBOSE" ] && printf '%s
' "$OUT" | sed 's/^/          | /'

echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
echo "HANDOFF-MERGE-TAIL GREEN"
