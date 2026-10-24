#!/usr/bin/env python3
"""Refuse a branch that records a finding in `docs/` and not in `docs/FINDINGS.md`.

    ./ci/findings-index.py --check            # against the merge-base with main
    ./ci/findings-index.py --check --base X   # against an explicit base
    ./ci/findings-index.py --vocab            # print the vocabulary and exit

# WHY THIS EXISTS

In the run of 2026-08-18/19 the same `figure-provenance` defect was found
independently by **five** lanes. Four area-lanes were spent on a rediscovery.
The same three packets were derived from two directions by two lanes and one
arithmetic error rotted in three places at once.

`docs/FINDINGS.md` is the fix and an index nobody appends to is worse than no
index, because it reads as evidence that nothing was found. Every lane is told
to append. Being told is what the last four workflow rules were, and each of
them regrew.

# WHAT IT ACTUALLY CHECKS, AND WHAT THAT IS NOT

A branch that ADDS a line to a tracked `docs/*.md` carrying a **status-change
marker** must also add a line to `docs/FINDINGS.md`. That is the whole rule.

The vocabulary is the shouted forms this project already uses when a claim
changes status — WITHDRAWN, RETRACTED, REFUTED, CORRECTED, SUPERSEDED,
DISQUALIFIED — plus a short list of unambiguous phrases. It was measured
against the tree rather than invented: those forms appear in three to four
documents each, and the obvious alternatives were rejected for imprecision
("silently" is in eleven documents, "was wrong" in nine, and neither marks a
finding on its own).

**THIS IS A TRIPWIRE, NOT A CENSUS.** It cannot tell whether the line appended
to `FINDINGS.md` describes the finding it just made you notice, or whether it
is any good, and it fires on nothing at all when a lane records a finding in
words outside the vocabulary. What it removes is the specific failure of
recording a retraction in prose and walking away — which is how a lane's
finding stayed invisible to the next four.

A branch that appends to `docs/FINDINGS.md` and touches nothing else is fine.
The rule is one-directional on purpose.

# NO .git, NO CHECK, AND IT SAYS SO OUT LOUD

`ci/t0.sh` runs in an rsynced build copy with no `.git` (see
`ci/t0-remote.sh --exclude '.git'`). A diff-against-base check structurally
cannot run there. It prints NO-GIT with the reason and exits 0, and
`ci/findings-index-falsify.sh` asserts that exact line, so the degradation is
tested rather than assumed — a check that quietly does nothing in the tree
where it usually runs is this project's central defect class.

Exit 0 clean or unrunnable, 1 a finding was recorded outside the index.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Shouted status changes. Case-sensitive: `withdrawn` in a sentence is prose,
# `WITHDRAWN` is this project marking a claim as no longer standing.
MARKERS = [
    "WITHDRAWN",
    "RETRACTED",
    "REFUTED",
    "CORRECTED",
    "SUPERSEDED",
    "DISQUALIFIED",
    "RESCINDED",
]

# Unambiguous lowercase phrases. Each names a defect rather than describing one,
# which is what keeps them out of ordinary explanatory prose.
PHRASES = [
    "measured the wrong",
    "measures the wrong",
    "answers a question nobody asked",
    "a check that could not fail",
    "cannot go red",
    "was never run",
]

INDEX = "docs/FINDINGS.md"

# Cold starts are per-branch handover notes and quote status changes constantly;
# they are not where a cross-lane finding belongs and never were.
EXCLUDE = re.compile(r"^docs/(FINDINGS\.md|cold-start/|claims/)")


def in_git() -> bool:
    return subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "--git-dir"], capture_output=True
    ).returncode == 0


def git(*args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(ROOT), *args], capture_output=True, text=True
    ).stdout


def added_lines(base: str, path_filter) -> dict[str, list[str]]:
    """Lines this branch ADDS, per file. Added only: quoting an existing
    retraction while moving text around is not a new finding."""
    out: dict[str, list[str]] = {}
    diff = git("diff", "--unified=0", f"{base}...HEAD", "--", "docs/")
    cur = None
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            cur = line[6:]
            continue
        if cur and line.startswith("+") and not line.startswith("+++"):
            if path_filter(cur):
                out.setdefault(cur, []).append(line[1:])
    return out


def hits(text: str) -> list[str]:
    found = [m for m in MARKERS if m in text]
    found += [p for p in PHRASES if p in text.lower()]
    return found


def main() -> int:
    if "--vocab" in sys.argv:
        print("markers:", ", ".join(MARKERS))
        print("phrases:", ", ".join(PHRASES))
        return 0

    if not in_git():
        print(
            "FINDINGS-INDEX NO-GIT — this tree has no .git, so a diff against a "
            "base cannot be taken. Expected in a ci/t0-remote.sh build copy, "
            "which is rsynced with --exclude '.git'. The check is skipped, not "
            "passed; it runs in every real checkout."
        )
        return 0

    base = None
    if "--base" in sys.argv:
        base = sys.argv[sys.argv.index("--base") + 1]
    else:
        for cand in ("origin/main", "main"):
            mb = git("merge-base", cand, "HEAD").strip()
            if mb:
                base = mb
                break
    if not base:
        print(
            "FINDINGS-INDEX NO-BASE — neither origin/main nor main resolves in "
            "this checkout, so there is nothing to diff against. Pass --base."
        )
        return 0

    docs = added_lines(base, lambda p: p.startswith("docs/") and not EXCLUDE.match(p))
    index = added_lines(base, lambda p: p == INDEX)
    appended = sum(1 for ls in index.values() for l in ls if l.strip())

    flagged: list[tuple[str, str, list[str]]] = []
    for path, lines in sorted(docs.items()):
        for line in lines:
            h = hits(line)
            if h:
                flagged.append((path, line.strip(), h))

    if not flagged:
        print(f"FINDINGS-INDEX GREEN — no status-change marker added outside {INDEX}")
        return 0
    if appended:
        print(
            f"FINDINGS-INDEX GREEN — {len(flagged)} status-change line(s) added "
            f"in docs/, and this branch appends {appended} line(s) to {INDEX}"
        )
        return 0

    print(f"FINDINGS-INDEX RED — this branch records a finding and does not index it.")
    print()
    print(f"  {len(flagged)} line(s) added to docs/ carry a status-change marker,")
    print(f"  and {INDEX} gains nothing. The next lane will not see this.")
    print()
    for path, line, h in flagged[:12]:
        print(f"  {path}  [{', '.join(h)}]")
        print(f"    {line[:110]}")
    if len(flagged) > 12:
        print(f"  ... and {len(flagged) - 12} more")
    print()
    print(f"  Append ONE line to {INDEX}: the date, one sentence, and the commit")
    print("  or file that holds the detail. The mechanism stays where you put it.")
    print("  `./ci/findings-index.py --vocab` prints what tripped this.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
