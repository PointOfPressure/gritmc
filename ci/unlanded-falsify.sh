#!/usr/bin/env bash
# ===== AGENT INTERFACE — this is all you need to read =====
#
# ci/unlanded-falsify.sh — prove `rev-list --count` overstates and `git cherry`
# does not, on a repo built to contain exactly one already-landed commit.
#
#     ./ci/unlanded-falsify.sh        # ~2 s, no box, no network, no corpus lock
#
# WHAT BREAKS IF YOU IGNORE THIS: `docs/AGENT-BRIEF.md` tells agents to count a
# backlog with `git cherry`. If that advice were wrong, or right for a reason
# that has since stopped holding, nothing would say so — a claim about git's
# behaviour written in a doc is a claim nobody re-checks.
# ===== end interface; everything below is why =====
#
# The suite builds a scratch repo rather than reading the real one, because the
# real one's numbers move: the gate chain merges continuously, and a test whose
# expected value changes under it would be turned off within a day. Here the
# expected values are constructions:
#
#   case 1  a commit whose patch IS on main under a different sha
#           rev-list counts it; cherry marks it `-`. This is the whole claim.
#   case 2  a commit whose patch is NOT on main
#           both count it. Without this, a `git cherry` that marked everything
#           `-` would pass case 1 — the check would be measuring nothing.
#   case 3  one commit on two stacked branches
#           the per-branch SUM double-counts it; the union does not.
#   case 4  the same patch on two branches under two shas
#           cherry counts two; patch-id de-duplication counts one.
#   case 5  ci/unlanded.sh's own output on that repo
#           three branches — feature, stacked (on feature), twin (one patch of
#           feature's under its own sha) — make its four numbers 8, 5, 4, 3 by
#           construction. The naive 8 against a real 3 is the whole point in
#           miniature, and it is the same shape as 780 against 453 on the real
#           repo. Asserting the SCRIPT and not just git's behaviour is what
#           stops a correct explanation shipping beside a wrong implementation.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
W=$(mktemp -d /tmp/unlanded-falsify.XXXXXX)
trap 'rm -rf "$W"' EXIT

pass=0; fail=0
ok()  { echo "ok   $1"; pass=$((pass+1)); }
bad() { echo "FAIL $1 — $2"; fail=$((fail+1)); }
eq()  { if [ "$2" = "$3" ]; then ok "$1 ($2)"; else bad "$1" "want $3, got $2"; fi; }

export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b main "$W/upstream"
cd "$W/upstream"
echo base > base.txt && git add base.txt && git commit -qm base

# `shared` is the patch that will land on main under a different sha.
git checkout -q -b feature
echo shared > shared.txt && git add shared.txt && git commit -qm "the shared patch"
SHARED_ON_FEATURE=$(git rev-parse HEAD)
echo only-here > only.txt && git add only.txt && git commit -qm "not on main"
echo more > only2.txt && git add only2.txt && git commit -qm "also not on main"

# main takes the shared patch by cherry-pick — a new sha, the same diff. This is
# what a rebase-merge does to every commit it lands.
git checkout -q main
# main advances first. Without this the cherry-pick would reproduce the SAME
# parent, tree, message and author date and git would hand back the identical
# sha — the case would then be testing nothing, which is why the assertion
# below exists rather than a comment saying it cannot happen.
echo moved > moved.txt && git add moved.txt && git commit -qm "main moved on"
git cherry-pick "$SHARED_ON_FEATURE" >/dev/null
SHARED_ON_MAIN=$(git rev-parse HEAD)

[ "$SHARED_ON_FEATURE" != "$SHARED_ON_MAIN" ] \
    || { bad "setup" "the two shas are identical; the case tests nothing"; exit 1; }

# `stacked` sits on top of feature, so it carries feature's commits too.
git checkout -q -b stacked feature
echo s > stacked.txt && git add stacked.txt && git commit -qm "stacked work"

# `twin` carries the same patch as one of feature's, under its own sha.
git checkout -q -b twin main
git cherry-pick "$(git rev-parse feature~1)" >/dev/null   # "not on main", re-applied

# --- case 1 --------------------------------------------------------------
rl=$(git rev-list --count main..feature)
ch=$(git cherry main feature | grep -c '^+')
eq "case 1: rev-list counts the already-landed commit" "$rl" 3
eq "case 1: git cherry does not" "$ch" 2

# --- case 2 --------------------------------------------------------------
minus=$(git cherry main feature | grep -c '^-')
eq "case 2: cherry marks exactly the landed one '-', not everything" "$minus" 1

# --- case 3 --------------------------------------------------------------
sum=$(( $(git rev-list --count main..feature) + $(git rev-list --count main..stacked) ))
union=$(git rev-list --count feature stacked --not main)
eq "case 3: per-branch sum double-counts the stack" "$sum" 7
eq "case 3: the union does not"                    "$union" 4

# --- case 4 --------------------------------------------------------------
shas=$( { git cherry main feature; git cherry main twin; } | awk '/^\+/{print $2}' | sort -u | wc -l)
pids=$( { git cherry main feature; git cherry main twin; } | awk '/^\+/{print $2}' | sort -u \
        | while read -r s; do git show "$s" | git patch-id --stable | awk '{print $1}'; done \
        | sort -u | wc -l)
eq "case 4: two shas of one patch across branches" "$shas" 3
eq "case 4: patch-id de-duplicates them"           "$pids" 2

# --- case 5: the real script, on this repo -------------------------------
git remote add origin "$W/upstream" 2>/dev/null
git update-ref refs/remotes/origin/main main
git update-ref refs/remotes/origin/feature feature
git update-ref refs/remotes/origin/stacked stacked
git update-ref refs/remotes/origin/twin twin
mkdir -p ci && cp "$ROOT/ci/unlanded.sh" ci/
out=$(./ci/unlanded.sh 2>&1)
got=$(echo "$out" | awk '/rev-list --count per branch/{a=$1} /de-duplicated by sha/{b=$1} \
                         /distinct shas/{c=$1} /distinct patch-ids/{d=$1} \
                         END{print a" "b" "c" "d}')
eq "case 5: ci/unlanded.sh prints 8 5 4 3 on a repo built to be 8 5 4 3" "$got" "8 5 4 3"

n=$((pass+fail))
if [ "$fail" -eq 0 ]; then echo "UNLANDED-FALSIFY GREEN — $n assertions, 0 failed"
else echo "UNLANDED-FALSIFY RED — $n assertions, $fail failed"; fi
exit $((fail > 0))
