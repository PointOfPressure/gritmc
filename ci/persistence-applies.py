#!/usr/bin/env python3
"""persistence-applies.py — does this branch need `ci/t2-persistence.sh`?

Reads `ci/checks/persistence-paths.toml` and a list of changed paths, and
prints one of two words on stdout:

    APPLIES          at least one changed path is under a declared prefix
    NOT-APPLICABLE   none is

with the matching prefixes, or the count of paths considered, on stderr so a
gate log records what the decision was made on. Exit status is 0 in both
cases: neither answer is an error, and a caller that read the exit status
instead of the word would be unable to tell them apart from a crash.

`--check` validates the ledger itself and is what `ci/t0.sh` runs. It needs no
git and no branch:

  * every declared prefix must match something that exists in the tree.
    A prefix nothing matches reads as coverage and provides none — the
    `ci/t2-regiondiff` fixture that "ran" for its whole life against a
    directory that was never there is the same defect one level down.
  * the count must be at least `meta.min_paths`. An empty list makes the
    stage skip on every branch and is byte-identical in the log to a tree
    where nobody touched the save path.
  * every `gates` entry must name a script that exists, or the literal
    "all".

Usage:
    ci/persistence-applies.py --check
    ci/persistence-applies.py --changed-from <base>        # git diff --name-only
    git diff --name-only main | ci/persistence-applies.py -
"""

import os
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - py<3.11 is not on this box
    import tomli as tomllib  # type: ignore

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LEDGER = os.path.join(ROOT, "ci", "checks", "persistence-paths.toml")

# The gate scripts a `gates` entry may name. Derived from the manifest in
# `ci/t2-persistence.sh` rather than repeated here as a literal list, because
# two hand-maintained copies of one list is how the "eight tables" that were
# twelve happened.
STAGE = os.path.join(ROOT, "ci", "t2-persistence.sh")


def known_gates():
    """The gate basenames `ci/t2-persistence.sh` actually runs."""
    names = set()
    with open(STAGE, encoding="utf-8") as fh:
        inside = False
        for line in fh:
            if line.startswith("GATES=("):
                inside = True
                continue
            if inside:
                if line.startswith(")"):
                    break
                row = line.strip().strip('"')
                if row.startswith("ci/"):
                    script = row.split("|", 1)[0]
                    names.add(os.path.basename(script)[: -len(".sh")])
    return names


def load():
    with open(LEDGER, "rb") as fh:
        return tomllib.load(fh)


def check():
    doc = load()
    paths = doc.get("path", [])
    floor = doc.get("meta", {}).get("min_paths", 0)
    bad = []
    if len(paths) < floor:
        bad.append(f"{len(paths)} declared paths, floor is {floor}")
    gates = known_gates()
    if not gates:
        bad.append("ci/t2-persistence.sh declares no gates — the GATES manifest could not be read")
    seen = set()
    for entry in paths:
        prefix = entry.get("prefix", "")
        if not prefix:
            bad.append("a [[path]] with no prefix")
            continue
        if prefix in seen:
            bad.append(f"{prefix}: declared twice")
        seen.add(prefix)
        if not os.path.exists(os.path.join(ROOT, prefix)):
            bad.append(f"{prefix}: nothing in the tree matches it")
        if not entry.get("why", "").strip():
            bad.append(f"{prefix}: no `why`")
        for gate in entry.get("gates", []):
            if gate != "all" and gate not in gates:
                bad.append(
                    f"{prefix}: names gate '{gate}', which ci/t2-persistence.sh does not run"
                )
    if bad:
        print("persistence-paths: REFUSED")
        for line in bad:
            print(f"  !! {line}")
        return 1
    print(
        f"persistence-paths: {len(paths)} declared paths (floor {floor}), "
        f"{len(gates)} gates, all present"
    )
    return 0


def decide(changed):
    doc = load()
    prefixes = [e["prefix"] for e in doc.get("path", []) if e.get("prefix")]
    hits = sorted({p for p in prefixes for c in changed if c == p or c.startswith(p.rstrip("/") + "/")})
    if hits:
        print("APPLIES")
        print(
            f"persistence-applies: {len(changed)} changed path(s) touch "
            f"{len(hits)} declared prefix(es): {' '.join(hits)}",
            file=sys.stderr,
        )
    else:
        print("NOT-APPLICABLE")
        print(
            f"persistence-applies: {len(changed)} changed path(s), none under any "
            f"of the {len(prefixes)} declared prefixes",
            file=sys.stderr,
        )
    return 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "--check":
        return check()
    if len(argv) >= 3 and argv[1] == "--changed-from":
        out = subprocess.run(
            ["git", "-C", ROOT, "diff", "--name-only", f"{argv[2]}...HEAD"],
            capture_output=True,
            text=True,
            check=False,
        )
        if out.returncode != 0:
            # A branch whose merge base cannot be found is not a branch this
            # can decide about, and guessing NOT-APPLICABLE would silently
            # skip the stage. Say so and let the caller treat it as APPLIES.
            print("UNDECIDABLE")
            print(f"persistence-applies: git diff failed: {out.stderr.strip()}", file=sys.stderr)
            return 0
        changed = [line for line in out.stdout.splitlines() if line]
        return decide(changed)
    if len(argv) >= 2 and argv[1] == "-":
        return decide([line.strip() for line in sys.stdin if line.strip()])
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
