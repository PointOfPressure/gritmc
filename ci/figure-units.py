#!/usr/bin/env python3
"""Refuse a published number whose prose noun disagrees with its registry unit.

    ./ci/figure-units.py              # the full report
    ./ci/figure-units.py --quiet      # one summary line
    ./ci/figure-units.py --explain ID # why one figure passes or fails

# THE DEFECT CLASS

Six of the run of 2026-08-18/19's corrections were one shape: **a right number
answering a question nobody asked.**

* `108/108 falsifiable` measured **declarations**. 61 of the 108 checks had
  never moved across 64 recordings.
* `git rev-list --count` measured commit **objects** — 4,182 naive against
  **1,377 distinct changes**.
* `union_N` measured the **width of the region a mask removes** and was scored
  as a null, making a published headline of 14x that was 3.53x.
* `frozen` counted checks with an **empty** history alongside checks that
  agreed every build.

Every one was arithmetically correct. No field in `ci/figures.toml` disagreed
with any other, because the ledger recorded what each number IS and never what
it ANSWERS. So each figure now carries a `unit` from a closed vocabulary and a
`question`, and this script checks the number against the sentence it is
published in.

# THE THREE RULES

**UNIT-NOUN.** For every occurrence of a figure's `claim` in its `doc`, look at
the prose within 90 characters either side. If it puts a noun belonging to a
DIFFERENT registered unit beside the number and none of the figure's own unit's
nouns, the sentence and the ledger disagree. That is the `108/108 falsifiable`
case, whose unit is `declarations` and whose prose said "checks", and the
`1,377 unlanded` case, whose unit is `patch-ids` and whose prose said
"commits". Conflicts are DERIVED — every other unit's nouns, minus the neutral
tokens — because a hand-written conflict list is two places to edit and one
place to forget.

**QUESTION-SHAPE.** `question` must be a question: it ends in `?` and is long
enough to name a window. A figure that cannot state what it answers in one
sentence is a figure whose author did not know.

**FLOOR-N.** Pure arithmetic on fields that already exist. A `floor` whose
value is published by a registered floor figure is traced back to it, and that
source's `refs` must be this figure's own `refs` — a residual at N=6 quoted
beside a floor taken at N=5 is a comparison across conditions wearing the
clothes of a comparison within one. A figure that means to do that declares
`floor_n`, which turns a deliberate cross-N comparison from a sentence in
`reason` into a field a checker can read. That is the `union_N` shape, where a
mask's width was quoted as a null.

Its coverage is thin and the summary line says so: 13 of the 102 floors in the
ledger are traceable to a registered floor figure, because the rest were taken
from ensembles that were never registered as figures in their own right. A
number that rises is the check reaching further; a number that falls is floors
becoming untraceable.

**QUESTION-COLLISION.** Two figures answering the same question on the same
window must not carry different claims. One of them is wrong, or the window is
not recording the condition that separates them. `ow-r16-admissible-n2` and
`ow-r16-admissible-matched` ask the same question and are BOTH right, because
their windows differ in `dump_order` — which is exactly what the rule tests,
and why the comparison is on the window and not on the question alone.

# WHAT THIS CANNOT CATCH, AND IT IS NOT A SMALL RESIDUE

**A defect in the DEFINITION rather than in the label is invisible here, and
no string comparison will ever find it.** `frozen` counted checks with no
history beside checks that agreed every build. Its label was accurate, its unit
would have been "checks", and its prose said "checks". It reds on nothing in
this script and it was one of the six.

The rule that catches that class is not mechanical and is stated in
`docs/ORACLE.md` instead: **a null result and an empty result must be different
words in every instrument this project writes.** It has produced two separate
defects already — `frozen`, and `union_N` scored as a null.

Also outside this script: a figure whose `unit` is simply wrong in a way its
prose faithfully repeats — the ledger and the sentence agree and both are
wrong; a `question` that is well-formed and not the question the instrument
answers; and any figure whose `claim` does not appear near prose at all,
because a number in a bare table row has no noun beside it to disagree with.
`--quiet` reports how many figures fell in that last group, because a check
that silently examines a third of its input is a check that measures its own
input, and that count going up is the signal that this one has stopped working.

Exit 0 clean, 1 a disagreement.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "ci" / "figures.toml"
RADIUS = 90
MIN_QUESTION = 25


def word_re(noun: str) -> re.Pattern:
    """Word-bounded where the noun is word-like, literal where it is not.

    `%` and `s/chunk` have no word boundary to anchor to, and `\\b` against a
    non-word character matches in places that are not occurrences at all.
    """
    esc = re.escape(noun)
    left = r"\b" if noun[0].isalnum() else ""
    right = r"\b" if noun[-1].isalnum() else ""
    return re.compile(left + esc + right, re.I)


def beside(window: str, claim: str, noun: str) -> bool:
    """True when `noun` stands beside `claim` with no other number between them.

    "Beside" has to mean nearest, or a line carrying two numbers charges both
    of them with every noun on it. `docs/BENCH.md` warns that "the secondary
    CPU-s block reads **+18.11%** where the primary reads +7.03%" — a caveat
    doing exactly its job, in which `cpu-s` belongs to 18.11 and not to the
    registered +7.03%. Without this rule that line reds, and the rule it would
    be enforcing is "never name two instruments in one sentence", which is how
    every caveat in this repo is written.

    A digit run between the two is what says the noun has an owner already. It
    is deliberately cruder than parsing the sentence: the failure it must not
    have is a false red on correct prose, and any number in between is enough
    to make the attribution ambiguous, which is enough to decline.
    """
    cm = re.search(re.escape(claim), window)
    if not cm:
        return False
    for nm in word_re(noun).finditer(window):
        if nm.end() <= cm.start():
            between = window[nm.end(): cm.start()]
        elif nm.start() >= cm.end():
            between = window[cm.end(): nm.start()]
        else:
            continue
        if not re.search(r"\d", between):
            return True
    return False


def load():
    d = tomllib.load(open(LEDGER, "rb"))
    units = {k: v for k, v in d["units"].items() if k != "neutral"}
    neutral = {n.lower() for n in d["units"]["neutral"]}
    return d["figure"], units, neutral


def conflicts_for(name: str, units: dict, neutral: set[str]) -> list[str]:
    own = {n.lower() for n in units[name]["nouns"]}
    other: set[str] = set()
    for k, v in units.items():
        if k == name:
            continue
        # `distinct`, not `nouns`. The first draft used `nouns` and produced 74
        # reds on a ledger with no known defect in it, because "x", " s", "id",
        # "test", "table", "target" and "charge" all stand beside numbers in
        # ordinary prose. A conflict set built from those fires everywhere,
        # which is the same as firing nowhere.
        other |= {n.lower() for n in v["distinct"]}
    return sorted(other - own - neutral)


def main() -> int:
    figs, units, neutral = load()
    quiet = "--quiet" in sys.argv
    explain = None
    if "--explain" in sys.argv:
        explain = sys.argv[sys.argv.index("--explain") + 1]

    texts: dict[str, str] = {}
    reds: list[str] = []
    no_prose = 0
    examined = 0

    conf_cache = {k: conflicts_for(k, units, neutral) for k in units}
    own_cache = {k: [n for n in units[k]["nouns"]] for k in units}

    seen_q: dict[tuple[str, str], tuple[str, str]] = {}

    # Every number a registered floor figure publishes, so a `floor` quoted
    # elsewhere can be traced to the measurement it came from. Ranges are
    # indexed by both ends: "2,577..4,835" is quoted as 4,835 as often as not.
    floor_src: dict[int, list[dict]] = {}
    for g in figs:
        if g["kind"] not in ("worldgen-floor", "vanilla-self"):
            continue
        vals = set()
        if "residual" in g:
            vals.add(int(g["residual"]))
        for m in re.finditer(r"[\d,]+", g["claim"]):
            t = m.group().replace(",", "")
            if t.isdigit():
                vals.add(int(t))
        for v in vals:
            floor_src.setdefault(v, []).append(g)
    floors_traced = 0

    for f in figs:
        fid = f["id"]
        want = explain is None or explain == fid

        # -- rule QUESTION-SHAPE ------------------------------------------
        q = f.get("question", "")
        if not q.endswith("?"):
            reds.append(f"QUESTION-SHAPE  {fid}: `question` does not end in `?` — {q[:60]!r}")
        elif len(q) < MIN_QUESTION:
            reds.append(
                f"QUESTION-SHAPE  {fid}: `question` is {len(q)} characters. A "
                f"figure that cannot say what it answers in one sentence naming "
                f"its window is a figure whose author did not know."
            )

        # -- rule QUESTION-COLLISION --------------------------------------
        # The key is question + window + PROVENANCE, and provenance is not
        # decoration. `ow-deepdark-A-sculk` and `ow-deepdark-A` answer the same
        # question on the same window and publish different numbers, and both
        # are right: they are the same measurement either side of a repair. The
        # condition that separates them is the TREE, which `window` does not
        # record and `provenance` does. A first draft keyed on question+window
        # alone and reported thirty such pairs as defects, every one of them a
        # correct before/after.
        wkey = repr(sorted(f.get("window", {}).items())) + "|" + str(f.get("provenance", ""))
        key = (q, wkey)
        if key in seen_q:
            other_id, other_claim = seen_q[key]
            if other_claim != f["claim"]:
                reds.append(
                    f"QUESTION-COLLISION  {fid} and {other_id} ask the same "
                    f"question on the same window and publish different numbers "
                    f"({f['claim']!r} against {other_claim!r}). Either one is "
                    f"wrong, or the window is not recording the condition that "
                    f"separates them."
                )
        else:
            seen_q[key] = (fid, f["claim"])

        # -- rule FLOOR-N --------------------------------------------------
        if "floor" in f and "refs" in f.get("window", {}):
            # A floor is traced to a source on the SAME window, not merely to a
            # source that publishes the same integer. Matching on the value alone
            # traced every `floor = 0` in the ledger to whichever floor figure
            # happens to publish a 0 — on 2026-09-23 that was the End city window
            # and a nether r=7 window, which then "explained" floors taken on the
            # overworld at seed 12345 and red five figures for a crossing that
            # never happened.
            fw = f["window"]
            src = [
                g for g in floor_src.get(int(f["floor"]), [])
                if all(g["window"].get(k) == fw.get(k)
                       for k in ("dimension", "seed", "radius", "centre"))
            ]
            if src:
                floors_traced += 1
                derived = {g["window"].get("refs") for g in src}
                names = ", ".join(g["id"] for g in src)
                declared = str(f["floor_n"]) if "floor_n" in f else None
                if declared is not None:
                    if declared not in derived:
                        reds.append(
                            f"FLOOR-N  {fid}: declares `floor_n = {declared}` but "
                            f"the floor {f['floor']} is published by {names} at "
                            f"refs={sorted(derived)}. The declaration and the "
                            f"source disagree."
                        )
                elif f["window"]["refs"] not in derived:
                    reds.append(
                        f"FLOOR-N  {fid}: residual measured at refs="
                        f"{f['window']['refs']}, floor {f['floor']} taken at "
                        f"refs={sorted(derived)} ({names}). A residual quoted "
                        f"beside a floor from a different N is a comparison "
                        f"across conditions. If that is deliberate, declare it: "
                        f"add `floor_n = \"{sorted(derived)[0]}\"` and say why in "
                        f"`floor_basis`, so the crossing is a field and not a "
                        f"sentence in `reason`."
                    )

        # -- rule UNIT-NOUN ------------------------------------------------
        # A figure with no `unit`, or one outside the vocabulary, is a red and
        # not a traceback: on 2026-09-23 the 38 figures main carried from before
        # this vocabulary existed took the whole script down with `KeyError:
        # 'unit'`, which named one figure and hid the other 37.
        unit = f.get("unit")
        if unit not in units:
            reds.append(
                f"UNIT-MISSING  {fid}: `unit` is {unit!r}, which is not in the "
                f"`[units]` vocabulary ({', '.join(sorted(units))}). Add the one "
                f"this number counts, or register a new unit with its nouns."
            )
            continue
        doc = f["doc"]
        if doc not in texts:
            pth = ROOT / doc
            texts[doc] = pth.read_text() if pth.exists() else ""
        text = texts[doc]
        claim = f["claim"]

        # "Beside the number" means ON THE SAME LINE, then bounded by RADIUS.
        # A first draft took +/- RADIUS characters flat and reported five
        # disagreements, every one of them a noun in a NEIGHBOURING line: an
        # adjacent row of the same table describing a different unit, and a
        # wrapped sentence about the tree the measurement was taken on. A
        # window that crosses a line boundary in a wrapped markdown document or
        # a fixed-width table is not reading the sentence the number is in.
        windows = []
        for m in re.finditer(re.escape(claim), text):
            ls = text.rfind("\n", 0, m.start()) + 1
            le = text.find("\n", m.end())
            le = len(text) if le == -1 else le
            windows.append(text[max(ls, m.start() - RADIUS): min(le, m.end() + RADIUS)])
        if not windows:
            no_prose += 1
            continue
        examined += 1

        own = own_cache[unit]
        conf = conf_cache[unit]
        for w in windows:
            mine = [n for n in own if word_re(n).search(w)]
            if mine:
                continue
            theirs = [n for n in conf if beside(w, claim, n)]
            if theirs:
                owners = sorted({
                    k for k in units
                    if any(n.lower() in {x.lower() for x in units[k]["distinct"]} for n in theirs)
                })
                reds.append(
                    f"UNIT-NOUN  {fid}: registered `unit = \"{unit}\"` but the "
                    f"prose beside this number in {doc} says "
                    f"{', '.join(repr(t) for t in theirs[:4])} and none of "
                    f"{unit}'s own nouns. Those nouns belong to "
                    f"{', '.join(owners[:3])}.\n"
                    f"           ...{' '.join(w.split())[:150]}..."
                )
                break
        if explain and want:
            print(f"{fid}: unit={unit} occurrences={len(windows)}")
            print(f"  own nouns:      {', '.join(own)}")
            print(f"  conflict nouns: {', '.join(conf[:20])}{' ...' if len(conf) > 20 else ''}")

    if reds:
        print(f"FIGURE-UNITS RED — {len(reds)} disagreement(s) between a number and its sentence")
        print()
        for r in reds:
            print("  " + r)
        print()
        print("  A unit is what the number counts. If the prose is right, the")
        print("  ledger's `unit` is wrong; if the ledger is right, the number has")
        print("  migrated into a sentence about a different quantity, which is the")
        print("  defect this check exists for. `./ci/figure-units.py --explain ID`")
        print("  prints the vocabulary that decided it.")
        return 1

    line = (
        f"FIGURE-UNITS GREEN — {examined} figures checked against their prose, "
        f"{no_prose} with no prose beside the number, {len(units)} units, "
        f"{floors_traced} floors traced to a registered source"
    )
    print(line)
    if not quiet:
        print(
            "  The no-prose count is not a pass. A number in a bare table row has "
            "no noun beside it to disagree with, so it is out of this check's "
            "reach; that count rising is the signal this check has stopped working."
        )
        print(
            "  It cannot catch a defect in the DEFINITION rather than the label — "
            "`frozen` counting checks with no history had an accurate label and "
            "reds here on nothing. See docs/ORACLE.md, null against empty."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
