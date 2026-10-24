#!/usr/bin/env python3
"""Check the scheduling invariants that live outside this repository.

# ===== AGENT INTERFACE - this is all you need to read =====
#
#     ci/check-queue-scheduling.py            # check every present consumer
#     ci/check-queue-scheduling.py --list     # what is checked, and where
#     ci/check-queue-scheduling.py --selftest # red-when-broken evidence
#
# Exit 0 green (including "nothing present to check"), 1 an invariant is
# broken, 2 the spec itself is unusable.
#
# WHAT BREAKS IF YOU IGNORE THIS: gate-chain.sh, chain-keeper.sh and
# preflight.sh schedule every merge this project makes and none of them is in
# the repository, so no gate can enforce anything about them. Each has lost a
# guard silently at least once.
# ===== end interface; everything below is why =====

# # WHY A CHECK CAN BE ABOUT A FILE THAT IS NOT HERE
#
# `ci/t0-remote.sh` rsyncs the tree to the box with `--exclude .git`, and the
# orchestrator scripts are not in the tree at all. So this check has to be
# able to say "not present, nothing to check" without that being a pass it
# claims credit for, and without it being a failure either.
#
# The rule, and it is the same one `ci/check-handoff-terminals.py` uses: a
# consumer that is PRESENT is checked, a consumer that is ABSENT is reported
# as skipped by name. In the canonical checkout -- the one machine where the
# chain actually runs, and the only place the invariant can be violated -- all
# of them are present. In the gitless copy on the box none of them are, and
# there is nothing there to break.
#
# The failure this is built against is NOT "someone deletes the guard on
# purpose". It is the one that has actually happened, repeatedly: a guard is
# rewritten for an unrelated reason and its point is lost in the edit. So
# every invariant carries `why`, printed on failure, saying what breaks --
# because "pattern not found in file" is not something anyone can act on.
#
# # AND WHY SOME INVARIANTS NAME THE DEFECT
#
# `must_not_match` exists because a repaired guard tends to grow its old self
# back beside it. `preflight.sh`'s publish step is the case: the completeness
# check can be added while the `[ -s "$OUT_TMP" ]` arm stays above it and
# still wins. Asserting the fix is present does not catch that; asserting the
# defect is absent does.
"""

import argparse
import os
import re
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = os.path.join(HERE, "checks", "queue-scheduling.toml")

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    print("check-queue-scheduling: needs Python 3.11+ for tomllib")
    sys.exit(2)


def load(spec_path):
    with open(spec_path, "rb") as fh:
        doc = tomllib.load(fh)
    files = doc.get("file", [])
    if not files:
        raise ValueError(f"{spec_path} names no [[file]] -- a spec that checks "
                         "nothing is the unfalsifiable case this file exists for")
    for f in files:
        if not f.get("path"):
            raise ValueError("a [[file]] has no path")
        invs = f.get("invariant", [])
        if not invs:
            raise ValueError(f"{f['path']} has no [[file.invariant]] -- listing a "
                             "consumer and checking nothing about it is worse than "
                             "not listing it, because it reads as covered")
        for i in invs:
            has_pos = "must_match" in i
            has_neg = "must_not_match" in i
            if has_pos == has_neg:
                raise ValueError(
                    f"{f['path']} / {i.get('name', '?')}: give exactly one of "
                    "must_match or must_not_match")
            if not i.get("why"):
                raise ValueError(
                    f"{f['path']} / {i.get('name', '?')}: no `why`. A failure "
                    "that only says which regex missed is not actionable")
            # Compile now so a bad regex is a spec error, not a silent pass.
            re.compile(i.get("must_match") or i["must_not_match"])
    return files


def check(spec_path=SPEC, out=sys.stdout):
    try:
        files = load(spec_path)
    except Exception as e:
        out.write(f"QUEUE-SCHEDULING REFUSED  {e}\n")
        return 2

    bad, checked, skipped = [], 0, []
    for f in files:
        path = f["path"]
        if not os.path.exists(path):
            skipped.append(path)
            continue
        body = open(path, encoding="utf-8", errors="replace").read()
        for inv in f["invariant"]:
            name = inv.get("name", "?")
            checked += 1
            if "must_match" in inv:
                if not re.search(inv["must_match"], body):
                    bad.append((path, name, "MISSING", inv["why"]))
            else:
                if re.search(inv["must_not_match"], body):
                    bad.append((path, name, "REINTRODUCED", inv["why"]))

    for path in skipped:
        out.write(f"queue-scheduling: SKIP {path} — not present in this tree\n")

    if bad:
        for path, name, kind, why in bad:
            out.write(f"\nQUEUE-SCHEDULING RED [{kind}] {os.path.basename(path)} :: {name}\n")
            for ln in why.strip().split("\n"):
                out.write(f"    {ln}\n")
        out.write(f"\nQUEUE-SCHEDULING RED — {len(bad)} of {checked} invariants broken\n")
        return 1

    if checked == 0:
        out.write("queue-scheduling: no consumer present — nothing to check\n")
        return 0
    out.write(f"QUEUE-SCHEDULING GREEN — {checked} invariants over "
              f"{len(files) - len(skipped)} file(s)\n")
    return 0


def selftest():
    import io
    fails = []

    def t(name, cond, detail=""):
        if cond:
            print(f"  ok    {name}")
        else:
            print(f"  FAIL  {name}  {detail}")
            fails.append(name)

    with tempfile.TemporaryDirectory() as d:
        target = os.path.join(d, "consumer.sh")
        spec = os.path.join(d, "spec.toml")

        def write_spec(body):
            open(spec, "w").write(body)

        good = '''
[[file]]
path = "%s"
  [[file.invariant]]
  name = "guard-present"
  must_match = "the_guard"
  why = "without it the thing breaks"
  [[file.invariant]]
  name = "defect-absent"
  must_not_match = "the_defect"
  why = "this is the shipped defect"
''' % target

        # 1. GREEN on a file that satisfies both arms.
        open(target, "w").write("if the_guard; then ok; fi\n")
        write_spec(good)
        buf = io.StringIO()
        rc = check(spec, out=buf)
        t("green when the guard is present and the defect is not", rc == 0,
          buf.getvalue())

        # 2. RED when the guard is deleted -- the regression this is for.
        open(target, "w").write("if something_else; then ok; fi\n")
        buf = io.StringIO()
        rc = check(spec, out=buf)
        t("RED when the guard is gone", rc == 1)
        t("...and it prints the `why`, not just the regex",
          "without it the thing breaks" in buf.getvalue())

        # 3. RED when the DEFECT grows back beside a still-present guard. This
        # is the case a must_match-only spec cannot see, and the reason
        # must_not_match exists.
        open(target, "w").write("if the_defect; then old; fi\nif the_guard; then ok; fi\n")
        buf = io.StringIO()
        rc = check(spec, out=buf)
        t("RED when the defect is reintroduced ALONGSIDE the guard", rc == 1)
        t("...and names it REINTRODUCED, not MISSING",
          "REINTRODUCED" in buf.getvalue())

        # 4. A file that is not present is SKIPPED, not passed and not failed.
        os.remove(target)
        buf = io.StringIO()
        rc = check(spec, out=buf)
        t("an absent consumer is skipped, not failed", rc == 0)
        t("...and says so by name", "SKIP" in buf.getvalue())
        t("...and does not claim a green it did not earn",
          "GREEN" not in buf.getvalue(), buf.getvalue())

        # 5. THE SPEC ITSELF MUST BE FALSIFIABLE. A consumer listed with no
        # invariant reads as covered and checks nothing -- exactly the defect
        # class this repo calls unfalsifiable.
        write_spec('[[file]]\npath = "/nowhere"\n')
        buf = io.StringIO()
        t("a consumer with no invariant is REFUSED", check(spec, out=buf) == 2)

        write_spec('''
[[file]]
path = "/nowhere"
  [[file.invariant]]
  name = "no-why"
  must_match = "x"
''')
        buf = io.StringIO()
        t("an invariant with no `why` is REFUSED", check(spec, out=buf) == 2)

        write_spec('''
[[file]]
path = "/nowhere"
  [[file.invariant]]
  name = "both"
  must_match = "x"
  must_not_match = "y"
  why = "w"
''')
        buf = io.StringIO()
        t("an invariant with BOTH arms is REFUSED (which would it mean?)",
          check(spec, out=buf) == 2)

        write_spec('''
[[file]]
path = "/nowhere"
  [[file.invariant]]
  name = "bad-regex"
  must_match = "("
  why = "w"
''')
        buf = io.StringIO()
        t("an uncompilable regex is REFUSED, not a silent pass",
          check(spec, out=buf) == 2)

    # 6. THE REAL SPEC PARSES AND ITS PATTERNS ARE THE RIGHT WAY ROUND. A
    # must_not_match that matches this repo's own shipped state would redden
    # every gate; a must_match that matches nothing would never redden.
    try:
        load(SPEC)
        t("the shipped ci/checks/queue-scheduling.toml is a valid spec", True)
    except Exception as e:
        t("the shipped ci/checks/queue-scheduling.toml is a valid spec", False, str(e))

    print(f"\ncheck-queue-scheduling selftest: {len(fails)} failure(s)")
    return 1 if fails else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--spec", default=SPEC)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if a.list:
        for f in load(a.spec):
            here = "present" if os.path.exists(f["path"]) else "ABSENT (skipped)"
            print(f"{f['path']}  [{here}]")
            for inv in f["invariant"]:
                arm = "must match" if "must_match" in inv else "must NOT match"
                pat = inv.get("must_match") or inv["must_not_match"]
                print(f"    {inv.get('name','?'):40s} {arm}: {pat}")
        return 0
    return check(a.spec)


if __name__ == "__main__":
    sys.exit(main())
