#!/usr/bin/env bash
# corpus-lock-provenance.sh — re-derive, from the repository, what the corpus
# lock's two founding incidents were actually running.
#
#     ./ci/corpus-lock-provenance.sh              # check every claim in §11
#     ./ci/corpus-lock-provenance.sh --ports A B  # what port pair two roots derive
#     ./ci/corpus-lock-provenance.sh --at <rev>   # what guards a given rev has
#
# # WHY THIS IS A SCRIPT AND NOT A PARAGRAPH
#
# `docs/CORPUS-LOCK-HOLD.md` §8 asserted that `92c47ea3` introduced the corpus
# lock and `pick_port_pair` in one commit, and concluded from that that the
# lock's founding evidence was confounded. The assertion was wrong, and it was
# wrong in a way a reader could not catch: the sentence it rests on ("Until
# 2026-08-06 this file hardcoded 25696 and 25697") *is* in `92c47ea3`'s diff,
# as a **context** line. A comment saying the hardcoding ended is written by
# the commit that ended it and carried by every commit after.
#
# One `git show | grep` produced a claim that stood for a day and pointed a
# lane at the wrong conclusion. So the claims are executable now: this file
# re-derives them and exits non-zero when one stops holding. If somebody
# rebases this history, or §11 is misread the way §8 was, it goes red rather
# than staying plausible.
#
# # WHAT IT CANNOT DO
#
# It reads the repository. It cannot tell you what was on disk in a `rustmc-*`
# build directory at 02:02 on 2026-08-07 — those are rsync targets with no
# `.git`. The bridge from a commit to a run is the *worktree reflog*, which is
# outside this repo's content and is quoted in §11 with its timestamps. This
# file checks the half that is in the tree: which commit introduced each guard,
# and which guards a given commit contains. Treat the reflog half as evidence
# of the same standard as a log file, because that is what it is.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

RC=0
say()  { printf '%s\n' "$*"; }
ok()   { printf '  ok    %s\n' "$*"; }
bad()  { printf '  RED   %s\n' "$*"; RC=1; }

# The four guards that could each, on their own, explain a pair of corpus runs
# interfering. Each is identified by a string that only its implementation
# contains, so "does rev X have this guard" is a content question and not a
# date question.
guard_pattern() {
    case "$1" in
        ports)     printf 'pick_port_pair' ;;
        collision) printf 'PORT COLLISION on' ;;
        oraclework) printf 'is a symlink to' ;;
        console)   printf -- '--console' ;;
        lock)      printf 'corpus_lock_acquire' ;;
        *) return 1 ;;
    esac
}
guard_file() {
    case "$1" in
        collision) printf 'ci/vanilla-server.sh' ;;
        *)         printf 'ci/t2-gameplay.sh' ;;
    esac
}

# Which commit first introduced a guard. `-S` counts occurrences, so the last
# commit it reports for a pattern that has never been removed is the one that
# added it.
introduced_by() {
    local g="$1" f p
    f="$(guard_file "$g")"; p="$(guard_pattern "$g")"
    # AUTHOR date, not committer date. `movement-tick` was rebased at 20:15
    # on 2026-08-06, which rewrote every committer date on it to 20:15 and
    # would put the afternoon's incident four hours later than it happened.
    # The author date survives a rebase; the committer date does not.
    git log --format='%H %ai' -S"$p" -- "$f" 2>/dev/null | tail -1
}

has_guard() {   # has_guard <rev> <guard>
    local rev="$1" g="$2" f p
    f="$(guard_file "$g")"; p="$(guard_pattern "$g")"
    git show "$rev:$f" 2>/dev/null | grep -cF -- "$p" >/dev/null
}

port_pair() {   # port_pair <root>
    python3 - "$1" <<'PY'
import hashlib, sys
root = sys.argv[1]
base = 26000 + 2 * (int(hashlib.md5(root.encode()).hexdigest()[:8], 16) % 400)
print(base, base + 1)
PY
}

case "${1:-}" in
--ports)
    [ $# -ge 2 ] || { echo "usage: $0 --ports <root> [root...]" >&2; exit 2; }
    shift
    for r in "$@"; do printf '%-40s %s\n' "$r" "$(port_pair "$r")"; done
    exit 0 ;;
--at)
    [ -n "${2:-}" ] || { echo "usage: $0 --at <rev>" >&2; exit 2; }
    say "guards present at $2:"
    for g in ports collision oraclework console lock; do
        if has_guard "$2" "$g"; then printf '  %-11s present\n' "$g"
        else printf '  %-11s ABSENT\n' "$g"; fi
    done
    exit 0 ;;
"") ;;
*) echo "unknown argument: $1" >&2; exit 2 ;;
esac

say "== which commit introduced each guard"
for g in ports collision oraclework console lock; do
    printf '  %-11s %s\n' "$g" "$(introduced_by "$g")"
done
say

# CLAIM 1. The port fix is NOT the lock's commit. This is the whole of §8's
# error and the reason §11 exists.
say "== §11 claim 1: the port fix predates the lock, in a different commit"
LOCK_C=92c47ea3
PORT_C=36b5f48f
if git cat-file -e "$LOCK_C^{commit}" 2>/dev/null && git cat-file -e "$PORT_C^{commit}" 2>/dev/null; then
    if has_guard "$PORT_C" ports; then ok "$PORT_C has pick_port_pair"
    else bad "$PORT_C does not have pick_port_pair — §11's dating is wrong"; fi
    if has_guard "$PORT_C^" ports; then
        bad "$PORT_C's parent already had pick_port_pair — it is not the introducing commit"
    else ok "$PORT_C^ does not — so $PORT_C introduced it"; fi
    # The DEFINITION, not a mention. `92c47ea3` does add a line containing the
    # string `pick_port_pair` — a comment, explaining that the acquire is
    # placed before the ports are picked. Grepping for the bare name here
    # reproduces §8's error exactly one layer down, and this check went red on
    # its first run for precisely that reason. What settles authorship is
    # `pick_port_pair() {`.
    if git show "$LOCK_C" --format= -- "$(guard_file ports)" | grep -cE '^\+ *pick_port_pair\(\)' >/dev/null; then
        bad "$LOCK_C DEFINES pick_port_pair — §8 was right and §11 is wrong"
    else ok "$LOCK_C defines no pick_port_pair (it only mentions it, and carries the block as context)"; fi
    if git show "$PORT_C" --format= -- "$(guard_file ports)" | grep -cE '^\+ *pick_port_pair\(\)' >/dev/null; then
        ok "$PORT_C defines pick_port_pair"
    else bad "$PORT_C does not define pick_port_pair — §11's dating is wrong"; fi
    if git merge-base --is-ancestor "$PORT_C" "$LOCK_C"; then
        ok "$PORT_C is an ancestor of $LOCK_C — the ports were fixed first"
    else bad "$PORT_C is not an ancestor of $LOCK_C"; fi
else
    bad "one of $LOCK_C / $PORT_C is not in this repository"
fi
say

# CLAIM 2. Both trees in the 2026-08-07 overlap carried the guards that would
# have made a port collision or a shared work directory impossible, and did NOT
# carry the lock. That is what makes 08-07 an unconfounded observation.
say "== §11 claim 2: both 2026-08-07 trees had the guards and not the lock"
for rev in 8fbf37f9 f5dd3a06; do
    git cat-file -e "$rev^{commit}" 2>/dev/null || { bad "$rev not in this repository"; continue; }
    for g in ports collision oraclework; do
        if has_guard "$rev" "$g"; then ok "$rev has $g"
        else bad "$rev is MISSING $g — the 08-07 incident is confounded after all"; fi
    done
    if has_guard "$rev" lock; then bad "$rev already had the lock — 08-07 was not unlocked"
    else ok "$rev has no corpus lock"; fi
    if has_guard "$rev" console; then
        bad "$rev has --console — the shared-fifo explanation is back in play"
    else ok "$rev has no --console (shared-fifo explanation excluded)"; fi
done
say

# CLAIM 3. The two roots derive port pairs that are nowhere near each other, so
# the probe never had to break a tie.
say "== §11 claim 3: the two 08-07 roots derive distant port pairs"
A=/home/sayer/rustmc-chatcmd
B=/home/sayer/rustmc-gate
PA="$(port_pair "$A")"; PB="$(port_pair "$B")"
printf '  %-32s %s\n' "$A" "$PA"
printf '  %-32s %s\n' "$B" "$PB"
if [ "${PA%% *}" = "${PB%% *}" ]; then
    bad "the two roots derive the SAME base — a collision was possible"
else
    ok "distinct bases, $(( ${PA%% *} > ${PB%% *} ? ${PA%% *} - ${PB%% *} : ${PB%% *} - ${PA%% *} )) apart"
fi
# CLAIM 4. The 2026-08-06 incident is AFTER the port fix, so neither founding
# incident is confounded by shared ports. This is §14, and it is the claim that
# retires `pick_port_pair` as an explanation for any of this.
say "== §14 claim 4: the 2026-08-06 incident postdates the port fix"
PORT_AT="$(git log -1 --format='%at' "$PORT_C" 2>/dev/null)"
# The three commits in which the run's own agent recorded the outcome, on the
# afternoon of 2026-08-06. Their author dates bound the incident from above.
for rev in 0cd8621d c5f7a003 fbf0ae61; do
    if ! git cat-file -e "$rev^{commit}" 2>/dev/null; then
        bad "$rev not in this repository"; continue
    fi
    AT="$(git log -1 --format='%at' "$rev")"
    WHEN="$(git log -1 --format='%ai' "$rev")"
    if [ -n "$PORT_AT" ] && [ "$AT" -gt "$PORT_AT" ]; then
        ok "$rev authored $WHEN — after $PORT_C, so that run had per-tree ports"
    else
        bad "$rev authored $WHEN — NOT after the port fix; §14 is wrong"
    fi
done
# And the account it records must still say what §14 quotes it as saying.
if git log -1 --format='%B' fbf0ae61 2>/dev/null | grep -c 'pkill -f rustmc-movetick' >/dev/null; then
    ok "fbf0ae61 still attributes the 74/86 kill to a deliberate pkill"
else
    bad "fbf0ae61 no longer attributes the kill — §14 quotes it"
fi
# Newlines folded first: the phrase wraps between "ten" and "minutes" in the
# commit body, and a line-oriented grep for it reports the claim as missing
# from a commit that plainly makes it. This check went red on its first run for
# exactly that, which is the third time in this branch that a grep answered a
# question about text rather than about content.
if git log -1 --format='%B' 0cd8621d 2>/dev/null | tr '\n' ' ' | tr -s ' ' \
        | grep -c 'one scenario every ten minutes' >/dev/null; then
    ok "0cd8621d still records the rate collapse from the harness's own counter"
else
    bad "0cd8621d no longer records the rate — §14 quotes it"
fi
# CLAIM 5. The founding message's damage tally counts one run twice. 70/86 and
# 74/86 are the same `rustmc-movetick` run two minutes apart — a progress
# reading and its death — not two casualties.
say "== §14 claim 5: 70/86 and 74/86 are one run, not two"
body() { git log -1 --format='%B' "$1" 2>/dev/null | tr '\n' ' ' | tr -s ' '; }
if body 0cd8621d | grep -c 'reached 70/86' >/dev/null; then
    ok "0cd8621d reports 70/86"
else bad "0cd8621d no longer reports 70/86"; fi
if body 0cd8621d | grep -c 'run is still going' >/dev/null; then
    ok "0cd8621d says the run was STILL GOING at 70/86"
else bad "0cd8621d no longer says the run was still going"; fi
if body c5f7a003 | grep -c "said the run was .still running under setsid nohup.. It is not" >/dev/null; then
    ok "c5f7a003 explicitly supersedes it — same run, now dead at 74/86"
else bad "c5f7a003 no longer supersedes 0cd8621d — §14's double-count claim rests on it"; fi
if body c5f7a003 | grep -c 'vanilla arm had completed all 86' >/dev/null; then
    ok "c5f7a003 records that the vanilla arm had completed all 86"
else bad "c5f7a003 no longer records the completed vanilla arm"; fi
# The two commits are minutes apart, which is what makes "same run" the only
# reading available.
T1="$(git log -1 --format='%at' 0cd8621d 2>/dev/null)"
T2="$(git log -1 --format='%at' c5f7a003 2>/dev/null)"
if [ -n "$T1" ] && [ -n "$T2" ] && [ "$((T2 - T1))" -lt 600 ]; then
    ok "authored $((T2 - T1))s apart — one run, two readings"
else bad "the two commits are not minutes apart; re-check §14"; fi

say
[ "$RC" -eq 0 ] && say "corpus-lock-provenance: every §11 and §14 claim holds" \
                || say "corpus-lock-provenance: a §11/§14 claim no longer holds — fix the doc or the tree"
exit "$RC"
