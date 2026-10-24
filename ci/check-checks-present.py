#!/usr/bin/env python3
"""Refuse a tree that has stopped running one of its own checks.

# WHAT THIS IS FOR, AND WHAT ALREADY COVERS THE REST

On 2026-08-17 two instances of one hole turned up within an hour, both by
accident. `regiondiff-live-fixture` sat on origin with two committed
`||||||| base` markers and its body repeated five times. A gameplay branch had
been rebased onto a main that PREDATED `ci/check-conflict-markers.py`, so its
marker check did not fail — it never ran, and a check that never ran is
indistinguishable from one that passed. A sweep of all 33 queued tips that
morning found **no committed markers anywhere** and **four branch tips whose
trees carry no marker check at all**.

**Those four were NOT passing vacuously in `preflight.tsv`, and the first
version of this header said they were.** Corrected the same day by running
it: `preflight.sh` rebases each branch onto main *before* it adjudicates, so
the tree it checks carries main's `ci/` whatever the tip carried, and
`gameplay-block-placement` — no `ci/check-conflict-markers.py` at its tip —
has it after the rebase and its GREEN was sound. The vacuous pass is real
wherever a tip is evaluated **unrebased**: an agent running `./ci/t0.sh` in
its own worktree before rebasing, which is how the instance the corpus agent
found arose.

Most of that is closed by things that are not this file:

* **A branch behind main cannot gate.** `ci/handoff.sh` computes `BEHIND` and
  refuses at exit 3, so a tree with a `ci/` older than main's cannot reach
  main. That is why the vacuous pass was confined to queue triage.
* **Queue triage refuses it too, since 2026-08-17.** `preflight.sh` used to
  read `[ -f "$W/ci/$script" ] || continue`; it now reports `missing:<check>`
  and additionally runs MAIN's copy of the marker scanner against the branch's
  tree, so a tree lacking the script is still adjudicated.

**What neither catches is a branch that removes a check and its invocation in
the same commit.** Nothing is inconsistent then: no script, no call, no error,
and a green T0 that has stopped looking. It is not behind main, so `handoff.sh`
is happy, and it carries every check `preflight.sh` knows to ask for except the
one it deleted.

# THE FOUR REFUSALS

1. **Presence** — every `script` in `ci/checks/required-checks.toml` exists.
2. **Wiring** — every file in that entry's `wired_in` exists AND invokes the
   script. Each check declares where it runs rather than this guard guessing.
3. **Discovery** — every `ci/check-*.py` in the tree is either in the manifest
   or invoked somewhere under `ci/` by something other than its own falsify
   script. A check that exists and nothing calls is the same defect from the
   other direction, and a new one should not be able to arrive unwired.
4. **The ratchet** — the manifest must carry at least `REQUIRED_FLOOR` entries.

# WHY `wired_in` IS DECLARED AND NOT DISCOVERED

The first two versions of this guard walked the call graph out of `ci/t0.sh`,
and both were wrong in the way this repo keeps being wrong:

* crediting any mention made a planted orphan look wired, because
  `ci/check-checks-present-falsify.sh` **names the orphan it plants**. The
  guard was reading its own test fixture as coverage;
* excluding falsify scripts then made `ci/check-conflict-markers.py` look
  wired with its real invocation commented out, because **this file's own
  header names it in a sentence**. A docstring is not a `#` comment, so prose
  was scoring as wiring.

Both are the defect being refused, committed by the refuser. A declared
`wired_in` is decidable, reviewable in a diff, and cannot be satisfied by
anything writing prose. It also states a fact the call graph gets wrong:
`ci/check-use-probe.py` is applied to real data by `ci/use-golden.sh` and
`ci/use-item-on-golden.sh`, the dumpers — inside T0 only its falsify script
runs it, and only against synthetic fixtures. Discovering that would have
produced either a false red or a false green depending on the heuristic; the
manifest simply records it.

# WHAT AN INVOCATION IS

A non-comment line where the path is preceded by something that runs it:
`python3 ci/x.py`, `./ci/x.sh`, `bash ci/x.sh`, `"$ROOT/ci/x.py"`. A path in
backticks in a sentence is a mention. The distinction is the whole point of
rule 2 and it is `ci/check-checks-present-falsify.sh` case 8.

**No git and no network.** T0 runs on a tree rsynced without `.git`, so a rule
needing `origin/main` would be undecidable exactly where it has to run.
Everything here is decidable from the tree alone.
"""

from __future__ import annotations

import glob
import os
import re
import sys
import tomllib

# The number of entries ci/checks/required-checks.toml must carry. Lower it
# only in the same commit that removes a check, and say why in that commit.
#
# **RAISE IT IN THE SAME COMMIT THAT ADDS ONE, TOO** — 2026-09-05. This was
# 14 against a manifest of 14; two checks were added and the floor was left
# alone, and the manifest then had a row of slack. Nothing was inconsistent:
# `--check` counts 16 against a floor of 14 and passes, and the guard's own
# case 3 still refuses a deleted check. What went quiet was the ratchet —
# `ci/check-checks-present-falsify.sh` case 4 deletes a check, its `ci/t0.sh`
# invocation AND its manifest row, the one removal the tree is self-consistent
# after, and the floor is the only thing left to catch it. At 14 of 16 the
# removal lands on 15, clears the floor, and case 4 goes red saying "a removed
# check went unrefused". So the falsifier caught it, which is the arrangement
# working: a floor that trails its manifest is a ratchet with slack, and slack
# is where a silent removal fits.
# Rebased onto main at 3230, which carried 15 of its own: 15 + 2 = 17.
# `persistence-2` rebased onto main at 3611, which carried 19: its own
# `ci/persistence-applies.py` row makes 20.
# train-a (main 3745 + recorder-housekeeping): main's 20 plus
# `ci/ambient-falsify.py` = 21.
# gate-pipeline (main 3953): main's 21 plus `ci/t0-keepgoing-falsify.sh` and
# `ci/t1-par-falsify.sh` = 23. Measured the way the comment above predicts:
# at 21 with 23 rows, case 4 of the falsifier went red in this branch's T0.
# train-e (main 4032 + explosions): main's 23 plus
# `ci/explosion-falsify.sh` = 24.
# train-g (main 16529d006 + item-components, train-e, land-batch-1, ...):
# main's 23 plus train-e's `ci/explosion-falsify.sh` and land-batch-1's five —
# `ci/check-shell-footguns.py`, `ci/check-vacuous-green.py` and its falsifier,
# `ci/cross-check.sh` and its falsifier — = 29, counted in the manifest
# (`grep -c '^\[\[required\]\]'`). At 24 the floor trailed by five, the slack
# case 4 of `ci/check-checks-present-falsify.sh` exists to refuse.
REQUIRED_FLOOR = 29

MANIFEST = "ci/checks/required-checks.toml"


def live_lines(path: str) -> list[str]:
    """A file's lines with comment-only lines dropped."""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return [ln for ln in f if not ln.lstrip().startswith("#")]
    except OSError:
        return []


def invocation_re(script: str) -> re.Pattern[str]:
    """Matches a line that RUNS `script`, not one that mentions it."""
    p = re.escape(script)
    return re.compile(
        r"(?:(?:python3?|bash|sh|source)\s+[\"']?(?:\./)?" + p + r")"
        r"|(?:\./" + p + r")"
        r"|(?:[\"']?\$\{?ROOT\}?/" + p + r")"
    )


def invokes(path: str, script: str) -> bool:
    pat = invocation_re(script)
    return any(pat.search(ln) for ln in live_lines(path))


def run(root: str = ".", floor: int | None = None) -> int:
    floor = REQUIRED_FLOOR if floor is None else floor
    here = os.getcwd()
    if root != ".":
        os.chdir(root)
    try:
        bad = 0
        if not os.path.exists(MANIFEST):
            print(f"check-checks-present: RED {MANIFEST} is absent — the "
                  "manifest of checks that may not be silently dropped is "
                  "itself the thing a dropping branch would delete")
            return 1

        required = tomllib.load(open(MANIFEST, "rb")).get("required", [])

        if len(required) < floor:
            print(f"check-checks-present: RED {MANIFEST} lists "
                  f"{len(required)} checks, the floor in this script is "
                  f"{floor}. Removing a required check means "
                  "lowering REQUIRED_FLOOR in the same commit, so that it is "
                  "a reviewed change and not a data edit.")
            bad += 1

        declared = set()
        for entry in required:
            script = entry.get("script", "")
            if not script:
                print("check-checks-present: RED a [[required]] entry has no script")
                bad += 1
                continue
            declared.add(script)
            if not os.path.exists(script):
                print(f"check-checks-present: RED MISSING {script} — required "
                      f"by {MANIFEST}: {entry.get('why', 'no reason recorded')}")
                bad += 1
                continue
            wired_in = entry.get("wired_in", [])
            if not wired_in:
                print(f"check-checks-present: RED {script} declares no "
                      "wired_in — say where it runs, or it is not required")
                bad += 1
                continue
            for host in wired_in:
                if not os.path.exists(host):
                    print(f"check-checks-present: RED {script} is declared "
                          f"wired in {host}, which does not exist")
                    bad += 1
                elif not invokes(host, script):
                    print(f"check-checks-present: RED UNWIRED {script} — "
                          f"{host} exists but no line of it RUNS the script. "
                          "A name in a comment or in prose is not an "
                          "invocation, and a check nothing runs is a green "
                          "that cannot be turned red.")
                    bad += 1

        # Discovery: a check in the tree that nothing calls. Its own falsify
        # script does not count — that names it as a fixture.
        hosts = sorted(glob.glob("ci/*.sh") + glob.glob("ci/*.py"))
        for found in sorted(glob.glob("ci/check-*.py")):
            if found in declared:
                continue
            own_falsify = found.replace(".py", "-falsify")
            if any(invokes(h, found) for h in hosts
                   if h != found and not h.startswith(own_falsify)):
                continue
            print(f"check-checks-present: RED UNWIRED {found} — a check in "
                  "the tree that nothing under ci/ invokes, its own falsify "
                  "script aside. Register it in the manifest, wire it in, or "
                  "delete it; a check that never runs is worse than none "
                  "because its presence reads as coverage.")
            bad += 1

        if bad:
            print(f"check-checks-present: RED — {bad} failing rule(s)")
            return 1
        print(f"check-checks-present: {len(required)} required checks present "
              f"and invoked where they declare (floor {floor}), "
              f"{len(glob.glob('ci/check-*.py'))} ci/check-*.py all accounted for")
        return 0
    finally:
        os.chdir(here)


def selftest() -> int:
    """Every refusal, against synthetic trees built here rather than found."""
    import shutil
    import tempfile

    fails = 0

    def build_base(d: str) -> None:
        os.makedirs(os.path.join(d, "ci", "checks"))
        with open(os.path.join(d, "ci", "t0.sh"), "w") as f:
            f.write("#!/usr/bin/env bash\n")
            for i in range(REQUIRED_FLOOR):
                f.write(f"python3 ci/stub-{i}.py || exit 1\n")
        rows = []
        for i in range(REQUIRED_FLOOR):
            open(os.path.join(d, "ci", f"stub-{i}.py"), "w").write("#\n")
            rows.append(f'[[required]]\nscript = "ci/stub-{i}.py"\n'
                        f'wired_in = ["ci/t0.sh"]\nwhy = "s"\n')
        open(os.path.join(d, "ci", "checks", "required-checks.toml"), "w").write(
            "\n".join(rows))

    def case(label: str, build, expect_rc: int, floor: int | None = None) -> None:
        nonlocal fails
        d = tempfile.mkdtemp(prefix="ccp-selftest-")
        try:
            build_base(d)
            build(d)
            rc = run(d, floor)
            if rc != expect_rc:
                print(f"SELFTEST FAIL {label}: rc={rc} expected {expect_rc}")
                fails += 1
            else:
                print(f"selftest ok  {label}")
        finally:
            shutil.rmtree(d, ignore_errors=True)

    def t0_without(d: str, drop: int) -> None:
        open(os.path.join(d, "ci", "t0.sh"), "w").write(
            "#!/usr/bin/env bash\n" + "".join(
                f"python3 ci/stub-{i}.py || exit 1\n"
                for i in range(REQUIRED_FLOOR) if i != drop))

    def manifest_without(d: str, drop: int) -> None:
        open(os.path.join(d, "ci", "checks", "required-checks.toml"), "w").write(
            "\n".join(f'[[required]]\nscript = "ci/stub-{i}.py"\n'
                      f'wired_in = ["ci/t0.sh"]\nwhy = "s"\n'
                      for i in range(REQUIRED_FLOOR) if i != drop))

    case("a complete tree is green", lambda d: None, 0)
    case("a required script that is absent is red",
         lambda d: os.unlink(os.path.join(d, "ci", "stub-0.py")), 1)
    case("a required script present but never invoked is red",
         lambda d: t0_without(d, 0), 1)
    # NOTE the tuple. Written first as `write(...) or t0_without(d, 0)`, which
    # short-circuits on the truthy byte count, so the real invocation was never
    # removed and the case passed for the wrong reason. A test builder can be
    # unfalsifiable too.
    case("an invocation demoted to a COMMENT is red",
         lambda d: (t0_without(d, 0),
                    open(os.path.join(d, "ci", "t0.sh"), "a").write(
                        "# python3 ci/stub-0.py\n")), 1)
    case("a PROSE mention where the invocation was is red",
         lambda d: (t0_without(d, 0),
                    open(os.path.join(d, "ci", "t0.sh"), "a").write(
                        'echo "see ci/stub-0.py for the rule"\n')), 1)
    case("script, invocation AND manifest row all removed is red (the ratchet)",
         lambda d: (os.unlink(os.path.join(d, "ci", "stub-0.py")),
                    t0_without(d, 0), manifest_without(d, 0)), 1)
    # The ratchet must refuse SILENCE, not change: doing the same removal
    # honestly, by lowering the constant, has to go green. A ratchet nobody can
    # legitimately lower is one people route around by deleting the guard.
    case("the same removal WITH the floor lowered would be green",
         lambda d: (os.unlink(os.path.join(d, "ci", "stub-0.py")),
                    t0_without(d, 0), manifest_without(d, 0)), 0,
         floor=REQUIRED_FLOOR - 1)
    case("a wired_in file that does not exist is red",
         lambda d: open(os.path.join(d, "ci", "checks", "required-checks.toml"),
                        "a").write('\n[[required]]\nscript = "ci/stub-0.py"\n'
                                   'wired_in = ["ci/nowhere.sh"]\nwhy = "s"\n'), 1)
    case("a ci/check-*.py nothing invokes is red",
         lambda d: open(os.path.join(d, "ci", "check-newcomer.py"), "w").write("#\n"), 1)
    case("the manifest itself deleted is red",
         lambda d: os.unlink(os.path.join(d, "ci", "checks", "required-checks.toml")), 1)

    if fails:
        print(f"check-checks-present --selftest: RED, {fails} case(s)")
        return 1
    print("check-checks-present --selftest: GREEN, 10 cases")
    return 0


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(selftest())
    sys.exit(run())
