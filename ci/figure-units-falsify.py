#!/usr/bin/env python3
"""Prove `ci/figure-units.py` can go red, and that it does not on correct prose.

    python3 ci/figure-units-falsify.py       # ~1 s, no build, no jar, no box

# WHY THIS EXISTS

`ci/figure-units.py` is green on all 169 live figures. A rule proven by the
absence of a violation is not proven — `ci/check-figures-falsify.py`'s header
says so about rules 9-13 and it is the same argument here, only sharper: this
checker was tuned DOWN from 74 reds to 0 across four narrowing passes, and
every one of those passes could have gone one step too far and left a rule that
fires on nothing. So each fault is injected into a hermetic fixture and the
expected rule is asserted by name.

# THE TWO CASES THAT ARE THE WHOLE ARGUMENT ARE NOT IN THE LEDGER

`108/108 falsifiable` and `1,377 unlanded` are the defects this check was built
for, and neither number is registered in `ci/figures.toml`. Tested against the
live ledger the check would be green whether it could catch them or not. Both
are constructed here, with the exact prose noun that made them wrong:
`unit = "declarations"` beside prose saying "checks", and
`unit = "patch-ids"` beside prose saying "commits".

# THE NEGATIVE CONTROLS ARE THE OTHER HALF, AND THEY ARE WHY THE CHECK IS USABLE

Each one is a narrowing that a cruder version of this rule got wrong, and each
one reds if its narrowing is removed:

  own-noun-wins   a percentage of blocks in a line that also says "chunks"
  proximity       a caveat naming a second instrument beside a second number
  line-bound      a table whose NEIGHBOURING row is about a different unit
  before-after    one question, one window, two trees, two numbers

`proximity` and `line-bound` are the two that took the check from 74 reds to 1.
`before-after` is the one that took it from 30 collisions to 2, and the two
that survived were a real defect: three published figures whose recorded
conditions were byte-identical and whose numbers differed by two points.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAIL = 0

VOCAB = '''
[units]
neutral = ["%", "percent", "of", "total", "count", "number", "value", "figure"]

[units.blocks]
nouns = ["block", "blocks", "position", "positions", "charge"]
distinct = ["blocks", "block positions"]
means = "block positions"

[units.percent-of-blocks]
nouns = ["block", "blocks", "exact", "exactness"]
distinct = []
means = "share of block positions"

[units.chunks]
nouns = ["chunk", "chunks"]
distinct = ["chunks"]
means = "chunk columns"

[units.checks]
nouns = ["check", "checks"]
distinct = ["checks"]
means = "checks that can move"

[units.declarations]
nouns = ["declaration", "declarations"]
distinct = ["declarations"]
means = "declared entries"

[units.commits]
nouns = ["commit", "commits"]
distinct = ["commits"]
means = "commit objects"

[units.patch-ids]
nouns = ["patch-id", "patch-ids", "distinct change", "distinct changes"]
distinct = ["patch-ids", "distinct changes"]
means = "distinct changes"

[units.cpu-seconds-per-chunk]
nouns = ["cpu-s", "cpu seconds"]
distinct = ["cpu-s", "cpu seconds"]
means = "CPU-seconds per chunk"

[units.percent-attribution]
nouns = ["attribut", "of generation"]
distinct = []
means = "share of runtime attributed"
'''

Q = "On this window, what does the instrument report?"


def fig(fid, unit, claim, question=Q, provenance="aaa1111", window='seed = "1"',
        kind="worldgen", extra=""):
    return f'''
[[figure]]
id = "{fid}"
kind = "{kind}"
unit = "{unit}"
question = "{question}"
doc = "docs/D.md"
claim = "{claim}"
provenance = "{provenance}"
{extra}window = {{ {window} }}
'''


# A registered floor figure at N=5, and a figure quoting its top against a
# residual at N=6. This is `union_N`'s shape: a number from one condition
# standing beside a number from another with nothing in the ledger saying so.
FLOOR_AT_5 = fig(
    "floor5", "blocks", "2,577..4,835", kind="worldgen-floor",
    question="At N=5, what charge does the instrument lay on an innocent run?",
    window='seed = "1", refs = "5"', extra="residual = 4835\n",
)


def run(figures: str, doc: str) -> tuple[int, str]:
    tmp = Path(tempfile.mkdtemp())
    try:
        (tmp / "ci").mkdir()
        (tmp / "docs").mkdir()
        shutil.copy(ROOT / "ci" / "figure-units.py", tmp / "ci" / "figure-units.py")
        (tmp / "ci" / "figures.toml").write_text(VOCAB + figures)
        (tmp / "docs" / "D.md").write_text(doc)
        p = subprocess.run(
            [sys.executable, str(tmp / "ci" / "figure-units.py")],
            capture_output=True, text=True,
        )
        return p.returncode, p.stdout + p.stderr
    finally:
        shutil.rmtree(tmp)


def case(label, figures, doc, want_rc, want_rule=None):
    global FAIL
    rc, out = run(figures, doc)
    if rc != want_rc:
        print(f"FAIL {label}: exit {rc}, wanted {want_rc}")
        print("     " + out.replace("\n", "\n     ")[:600])
        FAIL = 1
        return
    if want_rule and want_rule not in out:
        print(f"FAIL {label}: exit {rc} is right but the rule named is not {want_rule}")
        print("     " + out.replace("\n", "\n     ")[:600])
        FAIL = 1
        return
    print(f"ok   {label}")


print("-- the two defects this check was built for, neither of them registered --")
case(
    "108/108: unit=declarations, prose says 'checks'",
    fig("f", "declarations", "108/108"),
    "The corpus is 108/108 checks falsifiable today.\n",
    1, "UNIT-NOUN",
)
case(
    "1,377 unlanded: unit=patch-ids, prose says 'commits'",
    fig("f", "patch-ids", "1,377"),
    "The backlog stands at 1,377 commits not yet landed.\n",
    1, "UNIT-NOUN",
)

print("-- the same two, labelled correctly, must be green --")
case(
    "108/108: prose says 'declarations'",
    fig("f", "declarations", "108/108"),
    "The manifest carries 108/108 declarations today.\n",
    0,
)
case(
    "1,377 unlanded: prose says 'distinct changes'",
    fig("f", "patch-ids", "1,377"),
    "The backlog stands at 1,377 distinct changes not yet landed.\n",
    0,
)

print("-- the question rules --")
case(
    "a question that is not a question",
    fig("f", "blocks", "42", question="This measures the admissible charge."),
    "The charge is 42 blocks.\n", 1, "QUESTION-SHAPE",
)
case(
    "a question too short to name a window",
    fig("f", "blocks", "42", question="How many?"),
    "The charge is 42 blocks.\n", 1, "QUESTION-SHAPE",
)
case(
    "one question, one window, one tree, two numbers",
    fig("a", "blocks", "42") + fig("b", "blocks", "99"),
    "The charge is 42 blocks and also 99 blocks.\n", 1, "QUESTION-COLLISION",
)

print("-- negative controls: each is a narrowing, and reds if the narrowing goes --")
case(
    "own-noun-wins: 'blocks' present beside a line that also says chunks",
    fig("f", "percent-of-blocks", "99.9%"),
    "That is 99.9% of blocks over 1,089 chunks.\n", 0,
)
case(
    "proximity: a caveat naming a second instrument beside a second number",
    fig("f", "percent-attribution", "+7.03%"),
    "The secondary CPU-s block reads +18.11% where the primary reads +7.03% here.\n", 0,
)
case(
    "line-bound: the NEIGHBOURING table row is about a different unit",
    fig("f", "percent-attribution", "+7.03%"),
    "| trial chambers | +7.03% |\n| the cpu-s block | +18.11% |\n", 0,
)
case(
    "before-after: one question, one window, two trees, two numbers",
    fig("a", "blocks", "42", provenance="aaa1111")
    + fig("b", "blocks", "99", provenance="bbb2222"),
    "The charge is 42 blocks, and after the repair 99 blocks.\n", 0,
)

print("-- FLOOR-N: a residual quoted beside a floor from another N --")
case(
    "undeclared: residual at N=6, floor traced to a source at N=5",
    FLOOR_AT_5 + fig("f", "blocks", "7,871", window='seed = "1", refs = "6"',
                     question="At N=6, how much charge is there on this window?",
                     extra='floor = 4835\n'),
    "The charge is 7,871 blocks against 2,577..4,835.\n",
    1, "FLOOR-N",
)
case(
    "declared: the same pair with `floor_n = \"5\"` is GREEN",
    FLOOR_AT_5 + fig("f", "blocks", "7,871", window='seed = "1", refs = "6"',
                     question="At N=6, how much charge is there on this window?",
                     extra='floor = 4835\nfloor_n = "5"\n'),
    "The charge is 7,871 blocks against 2,577..4,835.\n",
    0,
)
case(
    "a declaration that disagrees with its own source is RED",
    FLOOR_AT_5 + fig("f", "blocks", "7,871", window='seed = "1", refs = "6"',
                     question="At N=6, how much charge is there on this window?",
                     extra='floor = 4835\nfloor_n = "8"\n'),
    "The charge is 7,871 blocks against 2,577..4,835.\n",
    1, "FLOOR-N",
)
case(
    "NEGATIVE CONTROL: same N on both sides needs no declaration",
    FLOOR_AT_5 + fig("f", "blocks", "7,871", window='seed = "1", refs = "5"',
                     question="At N=5, how much charge is there on this window?",
                     extra='floor = 4835\n'),
    "The charge is 7,871 blocks against 2,577..4,835.\n",
    0,
)

print("-- and the narrowings really are load-bearing: remove one, it reds --")
src = (ROOT / "ci" / "figure-units.py").read_text()
for name, needle in (
    ("proximity (beside())", "def beside("),
    ("line-bound (windows clipped to the line)", 'text.rfind("\\n", 0, m.start())'),
    ("before-after (provenance in the collision key)", 'str(f.get("provenance"'),
):
    if needle in src:
        print(f"ok   {name} is present in ci/figure-units.py")
    else:
        print(f"FAIL {name} has been removed — its negative control above is now vacuous")
        FAIL = 1

if FAIL == 0:
    print()
    print("FIGURE-UNITS-FALSIFY GREEN — 15 cases: both unregistered headline defects")
    print("  red by name, both green when labelled correctly, three question rules")
    print("  fire, FLOOR-N catches an undeclared cross-N floor and accepts a")
    print("  declared one, and four narrowings hold correct prose green")
else:
    print("FIGURE-UNITS-FALSIFY RED")
sys.exit(FAIL)
