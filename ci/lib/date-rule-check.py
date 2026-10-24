#!/usr/bin/env python3
"""Pin `ci/check-figures.py`'s date test in both directions.

Rule 18 resolves a provenance sha against the object database, and skips any
candidate that is a valid calendar date before asking (a date is never a
commit, whatever it prefix-matches). Until 2026-09-23 this test guarded a SHAPE
fallback for trees with no store; that fallback is now the `.figure-provenance`
snapshot, and the test moved to candidate selection in both writers.
Excluding every all-digit run rejected `27914806` — a real commit in this
repository that happens to carry no letter — and reddened `origin/main` on
every branch, at `ci/t0.sh:66`, with T0 exiting on first failure.

Excluding only runs that are a *valid calendar date* decides it. Both
directions matter: a test that called nothing a date would pass the interesting
case and quietly lose the rule, which is why the true cases are here too.

Called by `ci/box-env-falsify.sh` case 11. Exit 0 all classified, 1 otherwise.
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
CASES = [
    ("27914806", False, "a real commit; month 48 is not a month"),
    ("20260810", True, "a real date, and what the rule exists to reject"),
    ("20260812", True, "the date the original comment named"),
    ("99999999", False, "not a date; accepted on shape, which is all a gitless tree can do"),
    ("9a033a69", False, "has letters; never a date"),
    ("2026081", False, "seven digits; not a date shape"),
    ("18991231", False, "year below the window"),
    ("20261301", False, "month 13"),
    ("20260132", False, "day 32"),
]


def main() -> int:
    sys.path.insert(0, str(ROOT / "ci" / "lib"))
    spec = importlib.util.spec_from_file_location("cf", ROOT / "ci" / "check-figures.py")
    mod = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(mod)
    except SystemExit:
        pass
    fn = getattr(mod, "_looks_like_a_date", None)
    if fn is None:
        print("no _looks_like_a_date in ci/check-figures.py — rule 18's degraded "
              "path has been reworded and this check must follow it")
        return 1
    bad = [f"{v} -> {fn(v)}, wanted {w} ({why})" for v, w, why in CASES if fn(v) != w]
    if bad:
        print("; ".join(bad))
        return 1
    print(f"{len(CASES)} candidates classified, both directions")
    return 0


if __name__ == "__main__":
    sys.exit(main())
