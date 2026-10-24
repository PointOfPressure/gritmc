#!/usr/bin/env python3
"""ci/check-handoff-terminals-falsify.py — prove the terminal check can go red.

`ci/check-handoff-terminals.py` asserts that every way `ci/handoff.sh` can stop
is declared, named and forced. A check like that is worth exactly as much as its
ability to fail, and this repo's recurring defect is a check that could not: the
verdict selftest it sits beside had rotted to 0 of 17 unnoticed, and the
seventeenth check-that-could-not-fail was found underneath a grep pattern nobody
had driven.

So every rule the checker enforces gets an injection here, and the injection has
to make it red. The control — an untouched tree staying green — matters as much
as the injections: without it a checker that had crashed on import would "pass"
all nine.

Each case copies the real `ci/` into a throwaway root, damages exactly one
thing, and runs the real checker against it. Nothing here stubs the checker.

Run:  ./ci/check-handoff-terminals-falsify.py
"""
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

CI = Path(__file__).resolve().parent
CHECK = "ci/check-handoff-terminals.py"

PASS = 0
FAIL = 0


def run_in(root: Path) -> tuple[int, str]:
    p = subprocess.run([sys.executable, str(root / CHECK)],
                       capture_output=True, text=True, cwd=root)
    return p.returncode, p.stdout + p.stderr


def fresh() -> Path:
    root = Path(tempfile.mkdtemp())
    shutil.copytree(CI, root / "ci")
    # `docs/AGENT-BRIEF.md` carries the second verdict pattern the checker
    # enforces, so the throwaway root needs it too. Copied rather than stubbed:
    # a stub would let an injection into the real file go unnoticed.
    (root / "docs").mkdir()
    shutil.copy(CI.parent / "docs" / "AGENT-BRIEF.md", root / "docs" / "AGENT-BRIEF.md")
    return root


def case(name: str, want_red: bool, damage) -> None:
    """`damage` is called with the throwaway root and edits one file in it."""
    global PASS, FAIL
    root = fresh()
    try:
        damage(root)
        rc, out = run_in(root)
        red = rc != 0
        if red == want_red:
            PASS += 1
            print(f"  ok    {name:<46} {'red' if red else 'green'}")
        else:
            FAIL += 1
            print(f"  FAIL  {name:<46} {'red' if red else 'green'} "
                  f"(wanted {'red' if want_red else 'green'})")
            for line in out.strip().splitlines()[:6]:
                print(f"          {line}")
    finally:
        shutil.rmtree(root, ignore_errors=True)


def edit(root: Path, rel: str, old: str, new: str) -> None:
    p = root / rel
    t = p.read_text()
    if old not in t:
        raise SystemExit(f"falsifier fixture is stale: {old!r} not in {rel}")
    p.write_text(t.replace(old, new, 1))


print()
print("[control] an untouched tree is green — without this every case below is vacuous")
case("unmodified tree", False, lambda r: None)

print()
print("[1] a terminal the declaration has never heard of")
# The rule the whole file rests on. A new `exit` must not be addable in silence.
case("an undeclared exit is red", True, lambda r: edit(
    r, "ci/handoff.sh",
    'TIP=$(git rev-parse HEAD)',
    '[ -n "${RUSTMC_UNDECLARED:-}" ] && { echo "REFUSED  a new terminal"; exit 7; }\nTIP=$(git rev-parse HEAD)'))

print()
print("[2] the word is derived from the source, not read from the declaration")
# This is the `MERGED-NOT-PUSHED` defect as a rule: a declaration that merely
# restated the source would have agreed with it.
case("code prints a word the row denies", True, lambda r: edit(
    r, "ci/handoff.sh",
    'echo "REFUSED  no branch given"',
    'echo "MERGED  no branch given"'))

print()
print("[3] a silent terminal")
case("a terminal that prints nothing", True, lambda r: edit(
    r, "ci/handoff.sh",
    '[ -n "$BRANCH" ] || { echo "REFUSED  no branch given"; exit 2; }',
    '[ -n "$BRANCH" ] || { exit 2; }'))

print()
print("[4] the vocabulary stops being prefix-free")
case("one verdict prefixes another", True, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    '[[verdict]]\nword = "RED"',
    '[[verdict]]\nword = "REDX"\nexit = 1\njudged = "x"\nnext = "x"\n\n[[verdict]]\nword = "RED"'))

print()
print("[5] the word and the exit code drift apart")
case("declared exit contradicts the code", True, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    'word = "NOT-FF"\nexit = 3\nwhen = "state checks"',
    'word = "NOT-FF"\nexit = 2\nwhen = "state checks"'))

print()
print("[6] a control that is not there")
case("control names a missing file", True, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    'control = "ci/handoff-state-falsify.sh:no branch given"',
    'control = "ci/handoff-no-such-falsifier.sh:no branch given"'))
case("control names a missing label", True, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    'control = "ci/handoff-state-falsify.sh:no branch given"',
    'control = "ci/handoff-state-falsify.sh:a case nobody ever wrote"'))

print()
print("[7] a consumer forgets a word — ci/lanes.sh records it as no verdict line, the brief's waiter hangs")
# `ci/lanes.sh` could not name `MERGED` for the whole life of this loop, and a
# verdict it cannot name is recorded as `no verdict line`.
case("lanes.sh drops a verdict", True, lambda r: edit(
    r, "ci/lanes.sh", "^(MERGED|ALREADY-MERGED|", "^(ALREADY-MERGED|"))

case("AGENT-BRIEF's waiter drops a verdict", True, lambda r: edit(
    r, "docs/AGENT-BRIEF.md", "^(MERGED|ALREADY-MERGED|", "^(ALREADY-MERGED|"))
# The out-of-tree consumer, checked only where it exists. Two cases, because
# BOTH halves of "only where it exists" can be wrong: a present-and-wrong file
# must go red, and an absent one must NOT — a gate copy does not ship
# `gate-chain.sh` and must not be reddened by its absence.
def _stale_chain(r):
    (r / "gate-chain.sh").write_text(
        "line=$(x | grep -E '^(MERGED|MERGED-NOT-PUSHED|RED|REFUSED|INFRA)' | tail -1)\n")
    edit(r, "ci/checks/handoff-terminals.toml",
         'path = "../gate-chain.sh"', 'path = "gate-chain.sh"')


case("an out-of-tree consumer drops a verdict", True, _stale_chain)
case("an ABSENT out-of-tree consumer is not red", False, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    'path = "../gate-chain.sh"', 'path = "no-such-chain-script.sh"'))

print()
print("[8] an aggregate terminal gains a cause and does not declare it")
# `infra` and `unpushed` are one exit with many call sites. A control on the
# word alone proves only that the word can be printed.
case("a new infra() cause is undeclared", True, lambda r: edit(
    r, "ci/handoff.sh",
    'grep -q \'^STAGE1 OK\' "$LOG" || \\',
    'grep -q \'^A NEW CONDITION\' "$LOG" || \\\n    infra "a cause nobody declared"\ngrep -q \'^STAGE1 OK\' "$LOG" || \\'))

print()
print("[8b] the unforced LIST, not only the count")
# A bare ceiling is satisfied by forcing one terminal and un-forcing another,
# and cannot tell "a repair added two new terminals" from "two controls rotted
# away". Removing a control must be loud even when the count still fits.
case("un-forcing a terminal quietly", True, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    'control = "ci/handoff-state-falsify.sh:behind main"',
    'control = "none: somebody decided this was not worth a fixture after all"'))

print()
print("[9] a declaration that has rotted past the code it describes")
case("a stale row is red", True, lambda r: edit(
    r, "ci/checks/handoff-terminals.toml",
    "match = 'REFUSED  no branch given'",
    "match = 'REFUSED  a sentence this script has never printed'"))

print()
print(f"{PASS} passed, {FAIL} failed")
if FAIL:
    sys.exit(1)
print("HANDOFF-TERMINALS-FALSIFY GREEN")
