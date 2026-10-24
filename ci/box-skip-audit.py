#!/usr/bin/env python3
"""Count the stages that did not run, and red on any nobody declared.

    python3 ci/box-skip-audit.py --log <gate.log> [--declared ci/checks/box-skips.toml]

# WHY A LOUD SKIP IS NOT ENOUGH ON ITS OWN

`ci/lib/box_env.py` makes a check that cannot run say so, on stdout, in one
line beginning `BOX-SKIP`. That is necessary and it is not sufficient. A gate
log is 1,900 lines on a green run and nobody diffs it; "loud" that lands only
in a log is the same silence with more characters.

This project has already found eight defects of the shape *the check worked,
the channel into it did not* — `ci/t1.sh`'s `T1 GREEN` sentinel printed in 0 of
229 logs, a `REGRESSED` pattern matching its emitter in 0 of 45 lines. A skip
line nobody counts would be the ninth.

So the skips are **ratcheted**, the way every other count in this tree is: the
set that may appear is written down, a commit is what changes it, and a skip
that turns up without one is RED. The declaration carries a `why`, so the
question a reader asks — *should this stage be running here?* — has an answer
in the tree rather than in somebody's memory.

# THE HALF THAT CATCHES THE OPPOSITE MISTAKE

A declaration is also a claim that the stage **cannot** run in that
environment, and a claim that is never tested is the class this project keeps
finding. `--require-ran` names checks that must have produced no skip at all in
this environment: pass it in a run where a repository is present, and a
repo-skip becomes RED. That is the enforcement of the rule the repairs are
written to:

    a stage may skip only in an environment that could not have run it,
    and the gate fails if it skips in one that could.

Exit 0 clean, 1 an undeclared skip or a forbidden one, 2 the inputs are
malformed.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP_RE = re.compile(r"^BOX-SKIP\s+(\S+)\s+(.*)$")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("--log", required=True,
                    help="a gate log to scan for BOX-SKIP lines")
    ap.add_argument("--declared",
                    default=str(ROOT / "ci" / "checks" / "box-skips.toml"),
                    help="the TOML declaring which skips are permitted")
    ap.add_argument("--require-ran", default="",
                    help="comma-separated checks that must NOT have skipped "
                         "in this environment")
    args = ap.parse_args(argv)

    log = Path(args.log)
    if not log.exists():
        print(f"box-skip-audit REFUSED — no log at {log}", file=sys.stderr)
        return 2

    declared: dict[str, str] = {}
    dpath = Path(args.declared)
    if dpath.exists() and dpath.stat().st_size:
        try:
            doc = tomllib.loads(dpath.read_text())
        except tomllib.TOMLDecodeError as exc:
            print(f"box-skip-audit REFUSED — {dpath} is malformed: {exc}",
                  file=sys.stderr)
            return 2
        for row in doc.get("skip", []):
            if "check" not in row or "why" not in row:
                print(f"box-skip-audit REFUSED — a [[skip]] row in {dpath} is "
                      f"missing `check` or `why`. A declaration without a "
                      f"reason is a silence with a filename.", file=sys.stderr)
                return 2
            declared[row["check"]] = row["why"]

    seen: dict[str, str] = {}
    for line in log.read_text(errors="replace").splitlines():
        m = SKIP_RE.match(line.strip())
        if m:
            seen.setdefault(m.group(1), m.group(2))

    bad = 0
    for check, reason in sorted(seen.items()):
        if check in declared:
            print(f"  skip   {check:<34} {reason}")
        else:
            print(f"  UNDECLARED SKIP  {check} — {reason}\n"
                  f"      This stage did not run and nothing in "
                  f"{dpath.relative_to(ROOT) if dpath.is_relative_to(ROOT) else dpath} "
                  f"says it may. Either repair the stage so it runs here, or "
                  f"add a [[skip]] row saying why it cannot.")
            bad += 1

    must_run = [c.strip() for c in args.require_ran.split(",") if c.strip()]
    for check in must_run:
        if check in seen:
            print(f"  FORBIDDEN SKIP  {check} — this environment can run it "
                  f"and it stood down anyway ({seen[check]}). A stage may skip "
                  f"only where it could not have run.")
            bad += 1

    print(f"\nbox-skip-audit: {len(seen)} skip(s), {bad} not permitted")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
