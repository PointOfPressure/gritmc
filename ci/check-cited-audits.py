#!/usr/bin/env python3
"""Re-verify every figure this tree publishes and did not measure.

    python3 ci/check-cited-audits.py [<cited-audits.toml>]

Reads `ci/checks/cited-audits.toml` and, for each row, asks git for
`<sha>:<path>` and asserts the recorded `witness` is still in it. No branch is
merged, no measurement is repeated and nothing is trusted because it was true
when it was typed.

# WHY A CITATION NEEDS CHECKING AT ALL

The alternative is the one this project has already paid for: a lane finishes,
its numbers live on its branch, and the ledger either does not carry them —
which is how a refuted figure stays published — or carries a hand-copied copy,
which is how a number and its evidence come apart. A hand-copied number is
indistinguishable from a measured one and ages differently.

Pinning to a SHA rather than a branch name is the part that makes it a
citation. A branch tip moves; a citation that follows the tip is a citation to
whatever that lane does next, and would go green over a finding that had been
withdrawn.

Exit 0 every citation still holds, 1 one does not, 2 the file is malformed.
"""

from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "ci" / "lib"))
import box_env  # noqa: E402  (needs ROOT on the path first)

CITED = ROOT / "ci" / "checks" / "cited-audits.toml"


def main(argv: list[str] | None = None) -> int:
    global CITED
    argv = argv if argv is not None else sys.argv[1:]
    if argv:
        CITED = Path(argv[0])
    if not CITED.exists():
        print(f"check-cited-audits: {CITED} is missing", file=sys.stderr)
        return 2
    rows = tomllib.loads(CITED.read_text()).get("cited", [])
    if not rows:
        # A refusal, not a pass (vacuous-green-audit, 8cf8f84d6, ported onto
        # stack-ci's box_env route on train-c). Every citation could be
        # de-registered by an edit, a bad merge or a merge driver dropping a
        # region, and an empty table exiting 0 would say nothing about it.
        print("check-cited-audits: no citations registered — refusing, since "
              "an empty ledger is indistinguishable from one that lost its rows",
              file=sys.stderr)
        return 2

    # SUPERSEDES THE SKIP-PROBE THAT LANDED ON MAIN IN `f82b2ebb`. Two lanes
    # reached this repair in parallel; that one asks `rev-parse --git-dir` and
    # stands down when the answer is no. This one asks `ci/lib/box_env.py`,
    # which also finds the bare object store `ci/t0-remote.sh` now ships — so
    # in a gate directory the citations are actually re-verified rather than
    # skipped, and the skip is left covering only a tree somebody assembled by
    # hand. The skip also goes through `box_env.skip`, so
    # `ci/box-skip-audit.py` counts it and reds if nothing declared it; a
    # `print` reaches the log and not the ledger, and thirteen T0 stages filter
    # their own stdout.
    # # THE ENVIRONMENT THIS CHECK ACTUALLY RUNS IN. 2026-08-18.
    #
    # `ci/t0.sh:104` calls this unconditionally, and every path that reaches
    # `ci/t0.sh` in this project ships the tree with `--exclude '.git'` first
    # (`ci/t0-remote.sh`) or strips it with `git archive` (preflight). So the
    # tree this runs in has no repository, `git show` fails for every row, and
    # the failure was printed as `CITATION UNREACHABLE ... the branch was
    # rewritten or pruned`.
    #
    # That is a red that names a parity failure which never happened, and it
    # fired on **every branch**, for as long as this file has existed. Measured
    # 2026-08-18: 0 of 4 citations failing in a git worktree, 4 of 4 in the
    # box-shaped copy the gate runs in, with identical inputs.
    #
    # `ci/lib/box_env.py` finds the bare object store `ci/t0-remote.sh` now
    # ships, so in the ordinary case this simply runs. Where a tree was
    # assembled some other way it skips **loudly** rather than silently, and a
    # store that exists and does not work is RED rather than either — see that
    # module's head for why the third state is the one worth the code.
    try:
        store = box_env.git_store(ROOT)
    except box_env.BoxEnvRed as exc:
        print(f"check-cited-audits: {exc}", file=sys.stderr)
        return 1
    if store is None:
        box_env.skip(
            "check-cited-audits",
            f"no git object store in {ROOT} — {len(rows)} citation(s) not "
            f"re-verified. Ship one with ci/t0-remote.sh, or set "
            f"RUSTMC_GIT_DIR.",
        )
        return 0
    git_prefix = ["git", *box_env.store_args(store)]

    bad = 0
    for r in rows:
        for key in ("id", "branch", "sha", "path", "value", "witness", "what"):
            if key not in r:
                print(f"MALFORMED {r.get('id', '?')}: missing `{key}`")
                return 2
        blob = subprocess.run(
            [*git_prefix, "show", f"{r['sha']}:{r['path']}"],
            capture_output=True, text=True,
        )
        if blob.returncode != 0:
            # An unreachable sha is not a soft failure. It means the branch was
            # force-pushed or pruned, and the figure in the ledger now cites
            # nothing at all.
            print(f"CITATION UNREACHABLE  {r['id']} — {r['sha']}:{r['path']} "
                  f"is not in this repository. The branch `{r['branch']}` was "
                  f"rewritten or pruned; the ledger entry citing it is now "
                  f"unsupported and must be re-pointed or withdrawn.")
            bad += 1
            continue
        text = blob.stdout
        n = text.count(r["witness"])
        want = r.get("witness_count", 1)
        if n < want:
            print(f"CITATION DRIFTED  {r['id']} — expected {want}x "
                  f"{r['witness']!r} in {r['sha']}:{r['path']}, found {n}. The "
                  f"finding moved on its own branch and this tree still "
                  f"publishes the old number ({r['value']}).")
            bad += 1
            continue
        if "witness_count" in r and n != want:
            print(f"CITATION DRIFTED  {r['id']} — {n} occurrences, recorded "
                  f"{want}")
            bad += 1
            continue
        print(f"  cited  {r['id']:<40} {r['value']:>5}  {r['branch']}@{r['sha']}")

    print(f"\ncheck-cited-audits: {len(rows)} citations, {bad} failing")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
