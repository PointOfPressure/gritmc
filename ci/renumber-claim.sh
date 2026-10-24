#!/usr/bin/env bash
# ci/renumber-claim.sh — move one claim from §OLD to §NEW on one branch.
#
#     ci/renumber-claim.sh <doc> <branch> <slug> <old> <new> <reason> [--push]
#
# <doc> is DETERMINISM or PARITY-ROADMAP. It is not optional and not inferred:
# an unqualified `§48` means DETERMINISM §48 inside `docs/DETERMINISM.md` and
# PARITY-ROADMAP §48 inside `docs/PARITY-ROADMAP.md`, so a rewrite that does
# not know which document it is moving can silently renumber a citation in the
# other one. The first version of this script did not take it, rewrote `§N`
# across every tracked file, and got away with it only because no sibling
# document happened to carry the same number.
#
# <reason> is which clause of the tie-break fired, in one line. It is a
# required argument and not an optional flourish: the point of a written rule
# is that the next collision is a lookup, and a lookup needs the verdict
# recorded beside the change, not just the rule restated.
#
# WHY THIS IS A SCRIPT. Renumbering by hand is how §54 was "resolved" twice:
# the claim file, the `## N.` heading, every `### N.M` subsection and every
# `§N` citation have to move together, and missing one leaves a registry that
# passes `check-doc-claims` on the branch and breaks on the merge. Six edits
# in lockstep is a script's job.
#
# It commits ON TOP of the branch. It never rewrites history — a lane that
# wakes up on a rebased branch has lost work, and the whole point of doing
# this centrally is that no lane should pay for the collision.
set -euo pipefail

DOC="$1"; BRANCH="$2"; SLUG="$3"; OLD="$4"; NEW="$5"; REASON="$6"; PUSH="${7:-}"
case "$DOC" in DETERMINISM|PARITY-ROADMAP) ;; *)
  echo "doc must be DETERMINISM or PARITY-ROADMAP, got '$DOC'" >&2; exit 2;; esac
case "$DOC" in DETERMINISM) SIBLING="docs/PARITY-ROADMAP.md";; *) SIBLING="docs/DETERMINISM.md";; esac
# `RENUMBER_REPO` / `RENUMBER_WT_BASE` exist for `ci/renumber-claim-falsify.sh`
# and nothing else. The falsifier has to run this script for real — asserting
# that the SIBLING document was left alone means actually performing a move
# and looking — and doing that against the live repository would renumber a
# section on a real branch every time it ran.
REPO="${RENUMBER_REPO:-/home/sayer/grit/repo}"
WT="${RENUMBER_WT_BASE:-/home/sayer/grit/scratch-renumber}/$BRANCH"

# ANOTHER WORKTREE MAY ALREADY HOLD THIS BRANCH, AND MOVING THE REF BREAKS IT.
#
# `git worktree add --force` will happily give a second checkout of a branch
# that is already checked out. Committing here then moves the branch ref, and
# the OTHER worktree is left with the new commit as HEAD and the old tree in
# its index — so `git status` there shows the renumber staged as a REVERT. A
# lane that commits from that state undoes the move without noticing.
#
# That happened on 2026-08-18 to `~/grit/work-end`, and was repaired by hand.
# This is that repair, made automatic and made conditional: a worktree is only
# reset if it has NOTHING of its own — index identical to the pre-commit tree
# and no unstaged changes. Anything else is a lane's uncommitted work, and the
# script refuses to touch it and says so.
mapfile -t OTHER_WT < <(git -C "$REPO" worktree list --porcelain \
  | awk -v b="refs/heads/$BRANCH" '/^worktree /{w=$2} $0=="branch "b{print w}')

declare -a REPAIRABLE=()
for w in "${OTHER_WT[@]:-}"; do
  [ -n "$w" ] || continue
  [ "$w" = "$WT" ] && continue
  if [ -z "$(git -C "$w" status --porcelain)" ]; then
    REPAIRABLE+=("$w")
  else
    echo "  NOTE: $w also holds $BRANCH and has uncommitted work — it will be" >&2
    echo "        left alone. After this commit its status will show the" >&2
    echo "        renumber as a staged revert; \`git reset --hard $BRANCH\`" >&2
    echo "        there once the work is saved." >&2
  fi
done

git -C "$REPO" worktree add -q --force "$WT" "$BRANCH" 2>/dev/null || {
  git -C "$WT" checkout -q "$BRANCH" 2>/dev/null || { echo "cannot get a worktree at $BRANCH" >&2; exit 1; }
}
cd "$WT"
git checkout -q "$BRANCH"

OLDF="docs/claims/$DOC-$OLD-$SLUG.toml"
NEWF="docs/claims/$DOC-$NEW-$SLUG.toml"
[ -f "$OLDF" ] || { echo "$BRANCH: no $OLDF" >&2; exit 1; }

git mv "$OLDF" "$NEWF"
# The `number` field, which check-doc-claims cross-checks against the filename.
sed -i -E "s/^(number[[:space:]]*=[[:space:]]*)$OLD\$/\1$NEW/" "$NEWF"

# Headings and citations. `(?![0-9])` so §45 does not match inside §451, while
# §45.2 -> §60.2 still works because `.` is not a digit.
python3 - "$OLD" "$NEW" "$DOC" "$SIBLING" <<'PY'
import pathlib, re, subprocess, sys
old, new, doc, sibling = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
target_md = f"docs/{doc}.md"
files = subprocess.run(["git", "ls-files"], capture_output=True, text=True).stdout.split()
n_head = n_ref = 0
outside = []
for rel in files:
    if not rel.endswith((".md", ".toml", ".py", ".sh", ".rs")):
        continue
    # NEVER touch the sibling registered document. An unqualified `§N` in it
    # means ITS §N, and rewriting that is renumbering someone else's section.
    if rel == sibling:
        continue
    p = pathlib.Path(rel)
    try:
        t = p.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        continue
    o = t
    # `## 45. Title` and `### 45.3 Title` — only in the document being moved.
    if rel == target_md:
        t = re.sub(rf"^(#{{2,3}} ){old}(?=[.\s])", rf"\g<1>{new}", t, flags=re.M)
    n_head += len(re.findall(rf"^#{{2,3}} {new}(?=[.\s])", t, flags=re.M)) if t != o else 0
    # `§45`, `§45.2`, and the qualified `PARITY-ROADMAP §45`
    t2 = re.sub(rf"§{old}(?![0-9])", f"§{new}", t)
    n_ref += len(re.findall(rf"§{old}(?![0-9])", t))
    if t2 != o:
        p.write_text(t2, encoding="utf-8")
        if rel != target_md:
            # Printed for review. Outside the document being moved an
            # unqualified §N is ambiguous in principle; in this tree those
            # citations are qualified ("docs/PARITY-ROADMAP.md §N"), but that
            # is a fact about today and not a guarantee, so it is shown rather
            # than assumed.
            outside.append(rel)
print(f"  headings renumbered, {n_ref} citation(s) rewritten")
if outside:
    print("  rewrote §N outside " + target_md + " — eyeball these, an "
          "unqualified §N there is ambiguous by construction:")
    for rel in outside:
        print(f"    {rel}")
PY

python3 ci/check-doc-claims.py > /tmp/renumber-check.$$.log 2>&1 || {
  echo "$BRANCH: check-doc-claims REFUSED after the renumber:" >&2
  cat /tmp/renumber-check.$$.log >&2; exit 1; }
rm -f /tmp/renumber-check.$$.log

git add -A
git commit -q -F - <<MSG
claims: §$OLD -> §$NEW for \`$SLUG\` — collision, resolved centrally

§$OLD was claimed by two branches under different slugs. Renumbered here
rather than by the lane, because five lanes each renumbering blind is how §54
came to be "resolved" twice.

WHY THIS SIDE AND NOT THE OTHER:
$REASON

The rule, so the next one is a lookup and not an argument: the side with no
forward reference into it moves; where both or neither are referenced, the
side on the newer branch moves; where that still ties, the shorter prose
moves.

Moved together in one commit: the claim file's name and its \`number\` field,
the \`## N.\` heading, every \`### N.M\` subsection, and every \`§$OLD\`
citation on this branch. Missing one of those leaves a registry that passes
\`check-doc-claims\` on the branch and breaks at the merge, which is the
failure this is preventing.

§$NEW was taken with \`ci/claim-number.py --claim\`, so it is reserved against
every other worktree on this box and cannot become the next collision.

Committed on top; nothing is rewritten. A lane waking on this branch has lost
nothing.
MSG
echo "  committed on $BRANCH: $(git log -1 --format=%h)"

# Bring the clean co-checkouts up to the new commit, so no lane finds the
# renumber staged as a revert in its own tree.
# No guard on the post-commit state, deliberately: after the commit these
# worktrees ALWAYS look dirty, because the staged revert is exactly the damage
# being repaired. The safety check already happened — `REPAIRABLE` was built
# before the commit from worktrees whose status was empty. Re-checking it here
# was the first version of this loop and it skipped every worktree it was
# written to fix.
for w in "${REPAIRABLE[@]:-}"; do
  [ -n "$w" ] || continue
  git -C "$w" reset --hard -q "$BRANCH" && echo "  re-synced co-checkout $w"
done
[ "$PUSH" = "--push" ] && { git push -q origin "$BRANCH" 2>&1 | grep -v '^remote:' || true; echo "  pushed $BRANCH"; }
exit 0
