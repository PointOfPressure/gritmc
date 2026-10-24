#!/usr/bin/env bash
# ===== AGENT INTERFACE — this is all you need to read =====
#
# ci/unlanded.sh — how much work is genuinely not on main yet.
#
#     ./ci/unlanded.sh                 # all origin/* branches
#     ./ci/unlanded.sh my-branch       # one branch
#     ./ci/unlanded.sh --list          # the unlanded shas themselves
#
# WHAT BREAKS IF YOU IGNORE THIS: `git rev-list --count origin/main..<branch>`
# OVERSTATES a backlog and has already misled a lane and the orchestrator. It
# counts commits by sha, and a commit that landed on main under a rebase has a
# different sha with the same patch. Use `git cherry`, which filters those.
# ===== end interface; everything below is why =====
#
# # THE THREE NUMBERS, AND WHY ONLY THE LAST IS A BACKLOG
#
# Measured 2026-08-18 against origin/main dc9f882d, 218 branches:
#
#   sum of `git rev-list --count origin/main..<b>` over every branch    780
#     — counts the same commit once per branch that carries it. A commit on
#       five stacked branches is counted five times. This is the number people
#       reach for and it is not a quantity of anything.
#
#   `git rev-list --count <all branches> --not origin/main`             672
#     — de-duplicates by sha across branches. Still counts a commit whose
#       patch is already on main under a different sha, which is EVERY commit
#       that landed through a rebase, and this repo merges by rebase.
#
#   `git cherry origin/main <b>` summed, then `sort -u` on the shas         613
#     — cherry marks `-` a commit whose patch-id already appears on main and
#       `+` one that does not. This is the first number that means "not on
#       main".
#
#   the above, de-duplicated by `git show <sha> | git patch-id --stable`  453
#     — two branches that each cherry-picked the same fix carry two distinct
#       shas of one patch. 613 -> 453 is 160 duplicate patches across the
#       branch set. This is the backlog.
#
# The naive number is 780 against a real 453 — 1.7x — and the error is not a
# constant factor. It scales with how stacked the branch set is, so it grows
# exactly when a backlog reading matters most.
#
# # WHAT `git cherry` STILL CANNOT SEE, AND IT IS NOT NOTHING
#
# `git cherry` compares patch-ids, and a patch-id is computed over the diff
# hunks. A commit that landed on main and was then rebased over an unrelated
# edit to the SAME REGION has different context lines, therefore a different
# patch-id, therefore reads as unlanded. The number is a ceiling on the
# backlog, not an identity.
#
# # AND origin/main MOVES WHILE YOU MEASURE
#
# Learned the hard way on 2026-08-18: two runs of this measurement thirty
# minutes apart disagreed about three specific commits, and the explanation was
# not the instrument. `supervisor.sh`'s `check_stranded_merge` fetches, and the
# gate chain merges, continuously — `origin/main` had advanced between the two
# readings and taken those three commits with it. A backlog figure is a
# statement about a sha. This script prints the sha it measured against, and a
# figure quoted without one is not re-checkable.

set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"

MAIN="${UNLANDED_MAIN:-origin/main}"
MAIN_SHA=$(git rev-parse --short "$MAIN") || exit 2

list=0
case "${1:-}" in
    --list) list=1; shift ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
esac

if [ $# -gt 0 ]; then
    branches="$*"
else
    branches=$(git branch -r --format='%(refname:short)' | grep -vE "^${MAIN}\$|HEAD")
fi
nbranch=$(printf '%s\n' $branches | grep -c .)

naive=0
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
for b in $branches; do
    # `git rev-list --count` prints a number and exits 0 even for an empty
    # range, but exits non-zero on a bad ref — and `|| echo 0` would then
    # append a SECOND value to a variable that must hold one. Assign, then
    # validate. See docs/AGENT-BRIEF.md on `grep -c … || echo 0`.
    n=$(git rev-list --count "$MAIN..$b" 2>/dev/null)
    case "$n" in ''|*[!0-9]*) n=0 ;; esac
    naive=$((naive + n))
    git cherry "$MAIN" "$b" 2>/dev/null | awk '/^\+/{print $2}' >> "$tmp"
done

union=$(git rev-list --count $branches --not "$MAIN" 2>/dev/null)
case "$union" in ''|*[!0-9]*) union=0 ;; esac

cherry_sum=$(wc -l < "$tmp")
sort -u "$tmp" -o "$tmp"
cherry_distinct=$(wc -l < "$tmp")

pid_distinct=$(while read -r sha; do
        git show "$sha" 2>/dev/null | git patch-id --stable | awk '{print $1}'
    done < "$tmp" | sort -u | grep -c .)

if [ "$list" = 1 ]; then cat "$tmp"; exit 0; fi

printf 'UNLANDED against %s (%s), %s branches\n' "$MAIN" "$MAIN_SHA" "$nbranch"
printf '  %6s  sum of rev-list --count per branch   OVERSTATES: one commit per branch carrying it\n' "$naive"
printf '  %6s  union, de-duplicated by sha          still counts rebased-and-landed commits\n' "$union"
printf '  %6s  git cherry, distinct shas            not on main by patch-id\n' "$cherry_distinct"
printf '  %6s  distinct patch-ids                   THE BACKLOG\n' "$pid_distinct"
printf '  (%s cherry + lines before de-duplication)\n' "$cherry_sum"
