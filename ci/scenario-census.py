#!/usr/bin/env python3
"""Counts over `ci/checks/gameplay/*.toml`, derived — and the prose that quotes them.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/scenario-census.py           the census
#     ci/scenario-census.py --check   refuse if a quoted number has gone stale
#     ci/scenario-census.py --list    every quantity, its value, and who quotes it
#
# WHAT BREAKS IF YOU IGNORE THIS: you write "thirty-four checks drive
# `container_click`" into a check's `about` field, the corpus grows to 39
# steps in 15 scenarios, and the sentence stays. `720` carried exactly that
# from the day it was written until 2026-08-18.
# ===== end interface; everything below is why =====

# # THE SENTENCE THIS EXISTS TO KILL

`ci/checks/gameplay/720-a-container-click-puts-the-item-in-your-cursor.toml`
said, in its `about`:

    Thirty-four checks drive `container_click` and not one of them can tell a
    server that moved the item into the cursor from one that moved it
    somewhere else

Measured against the same tree: **39 `container_click` steps across 15
scenario files.** Neither 34 checks nor 34 steps — the figure was never right
in the units it was written in, and the corpus grew afterwards on top of that.

The brief's phrasing for this class is exact: *an audit that quotes a figure
out of the prose it is auditing has measured the prose.* The number was
inherited from an earlier sentence rather than counted, and nothing in the
tree could tell the difference between a count and a recollection.

# # THE CONVENTION IS THE HARD PART, NOT THE COUNTING

`ci/unported-inventory.py` makes this argument first and it applies here
verbatim: markers and gaps are a many-to-many relation, so a hand count
silently picks a convention and never states it. `container_click` is the same
shape three ways over:

  * a **file** may drive it many times — 15 files carry 39 uses;
  * a use may sit in `[[setup]]` (arranging the world) or in `[[step]]` (the
    thing being observed), and only the second is what "drives" means in the
    sentence 720 was making;
  * "checks" and "scenarios" are used interchangeably in the prose and are the
    same thing, while "steps" is not.

So each quantity below names its convention in one line, and the number falls
out of it. Quote whichever you mean and say which.

# # WHY THE LEDGER IS IN THIS FILE AND NOT BESIDE IT

`ci/check-source-citations.py` needs no ledger at all, because a citation
states its own oracle. A prose count cannot: "39" in a sentence does not say
what was counted. So a ledger is unavoidable here — but it is **inlined**,
because a ledger in a separate data file is a second artefact that drifts, and
three of today's corrections were exactly that. Fifteen lines of QUANTITIES
next to the code that derives them cannot fall out of step with a lane that
edits one and not the other.

# # WHAT THIS DELIBERATELY DOES NOT DO

It does not count anything that needs a **recording**. `720`'s other figure —
"`set_cursor_item`, which vanilla sends 221 times across eight scenarios" —
is a measurement over the corpus recordings, not over this repository, and
re-deriving it needs the corpus lock. It is left alone and flagged by
`--list` as `NEEDS-CORPUS` rather than silently treated as checkable, because
a checker that quietly skips the figures it cannot reach is the vacuous-green
shape in another costume.

# # THE DEFECT CLASS THIS MUST NOT JOIN

A census with no quantities passes. `MIN_QUANTITIES` floors it, `--check`
requires every quantity to be quoted by at least one live document (an
unquoted quantity is a rule nothing exercises), and `--selftest` poisons both
directions: a number that drifts, and an anchor that stops matching because
the sentence was reworded around it.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GAMEPLAY = ROOT / "ci" / "checks" / "gameplay"

MIN_QUANTITIES = 3


# --- the derivations -------------------------------------------------------


def _blocks(text: str) -> list[tuple[str, str]]:
    """[(table-name, body)] for each `[[name]]` block in a scenario file.

    Written by hand rather than with `tomllib` because a `do` key is legal in
    both `[[setup]]` and `[[step]]`, and the distinction between them is the
    whole convention; a parsed dict of lists loses which array a row came
    from unless you keep both, which is what this does more directly.
    """
    out: list[tuple[str, str]] = []
    name, buf = None, []
    for line in text.splitlines():
        m = re.match(r"^\[\[([a-z_]+)\]\]", line)
        if m:
            if name is not None:
                out.append((name, "\n".join(buf)))
            name, buf = m.group(1), []
        elif name is not None:
            buf.append(line)
    if name is not None:
        out.append((name, "\n".join(buf)))
    return out


def _falsifiable_meta(key: str) -> int:
    """One `[meta]` value out of ci/checks/gameplay-falsifiable.toml.

    Read with a regex rather than `tomllib` on purpose: this must report a
    number for a key that is missing as an exception rather than as a
    silently-absent quantity, and the file is also read by
    `ci/corpus-falsifiable.py`, which owns its schema. Parsing it fully here
    would be a second opinion about a format that already has an owner.
    """
    text = (ROOT / "ci" / "checks" / "gameplay-falsifiable.toml").read_text()
    m = re.search(rf"^{re.escape(key)}\s*=\s*(\d+)\s*$", text, re.M)
    if not m:
        raise SystemExit(
            f"scenario-census: ci/checks/gameplay-falsifiable.toml has no "
            f"`{key}` — the ledger's [meta] block changed shape and this "
            f"census is quoting a key that no longer exists"
        )
    return int(m.group(1))


def scenarios() -> dict[str, str]:
    return {p.name: p.read_text(encoding="utf-8", errors="replace")
            for p in sorted(GAMEPLAY.glob("*.toml"))}


def _do_counts(kind: str, table: str | None) -> tuple[int, int]:
    """(uses, files) of `do = "<kind>"`, optionally restricted to one table."""
    uses = files = 0
    for _name, text in scenarios().items():
        n = 0
        for tname, body in _blocks(text):
            if table is not None and tname != table:
                continue
            n += len(re.findall(rf'^\s*do\s*=\s*"{re.escape(kind)}"\s*$', body, re.M))
        if n:
            files += 1
            uses += n
    return uses, files


# name -> (convention, derivation, [(doc, anchor-regex with one \d+ group)])
#
# The anchor must contain exactly one `(\d+)` — the number being adjudicated —
# and enough surrounding words that a rewrite of the sentence makes it stop
# matching rather than match something else. UNANCHORED is a red, not a skip:
# a sentence that no longer contains its own claim has not become true.
QUANTITIES: dict[str, tuple[str, callable, list[tuple[str, str]]]] = {
    "container_click_steps": (
        "uses of `do = \"container_click\"` inside [[step]], across every "
        "scenario file — what 'drives container_click' means",
        lambda: _do_counts("container_click", "step")[0],
        [("ci/checks/gameplay/720-a-container-click-puts-the-item-in-your-cursor.toml",
          r"(\d+) `container_click` steps")],
    ),
    "container_click_scenarios": (
        "scenario files carrying at least one `container_click` step — "
        "'checks' and 'scenarios' are the same thing, 'steps' is not",
        lambda: _do_counts("container_click", "step")[1],
        [("ci/checks/gameplay/720-a-container-click-puts-the-item-in-your-cursor.toml",
          r"in (\d+) scenarios")],
    ),
    "dead_fields": (
        "`(kind, field)` pairs read by no check — `dead_fields` in "
        "ci/checks/gameplay-falsifiable.toml, the ledger ci/corpus-falsifiable.py "
        "gates. Derived from the artefact, not from first principles: that "
        "ledger IS the measurement, so citing it is citing the artefact",
        lambda: _falsifiable_meta("dead_fields"),
        [("docs/PARITY-ROADMAP.md", r"\*\*(\d+) of \d+ `\(kind, field\)` pairs are read by no check in the corpus"),
         ("docs/WORKLIST.md", r"\*\*(\d+) of \d+ `\(kind, field\)` pairs are read by no check\*\*")],
    ),
    "field_universe": (
        "the denominator those dead fields are out of — `field_universe` in "
        "the same ledger. It moved 84 -> 90 in `19e106af` and three live "
        "claims still said 84 nine days later, because the NUMERATOR did not "
        "move and the sentence went on reading plausibly",
        lambda: _falsifiable_meta("field_universe"),
        [("docs/PARITY-ROADMAP.md", r"\*\*\d+ of (\d+) `\(kind, field\)` pairs are read by no check in the corpus"),
         ("docs/WORKLIST.md", r"\*\*\d+ of (\d+) `\(kind, field\)` pairs are read by no check\*\*")],
    ),
    "scenario_files": (
        "files in ci/checks/gameplay/ — the corpus denominator",
        lambda: len(scenarios()),
        [],
    ),
}

# A QUOTATION OF A PAST CLAIM IS NOT A CLAIM, AND MUST NOT BE "CORRECTED".
#
# "4 of 84" appears six times in this tree. Three are live claims. The other
# three are `docs/cold-start/worklist-merge-driver.md`,
# `docs/cold-start/docs-figures-ledger.md` and `ci/merge-docregions.py`
# quoting the strings `10 of 84` and `4 of 84` as evidence in a story about a
# union-merge that published one cold start twice. Rewriting those to 90 would
# destroy the evidence and make the story incoherent.
#
# The anchors above therefore name the live sites explicitly rather than
# sweeping for a number. A checker that cannot tell a claim from a quotation
# of a claim would have to be switched off the first time it was right.

# Figures that are quoted in this tree and CANNOT be derived from it, because
# deriving them needs the corpus recordings and therefore the corpus lock.
# Listed so that "not checked" is a visible state rather than an absent one.
NEEDS_CORPUS = [
    ("720 `about`", "`set_cursor_item` … vanilla sends 221 times across eight "
                    "scenarios — a count over recordings, not over this repo"),
]


def derive() -> dict[str, int]:
    return {k: v[1]() for k, v in QUANTITIES.items()}


def check(values: dict[str, int], root: Path | None = None) -> list[str]:
    """`root` is a parameter only so the self-test can adjudicate a fixture.

    Production always passes None and uses the module constant, for the same
    reason `ci/check-doc-claims.py`'s `ceiling` is a parameter: a self-test
    that runs against the real tree passes or fails for reasons that have
    nothing to do with the rule it is testing, which is exactly what happened
    on the first run of this file.
    """
    base = root or ROOT
    reds: list[str] = []
    for name, (_conv, _fn, anchors) in QUANTITIES.items():
        want = values[name]
        for rel, pat in anchors:
            p = base / rel
            if not p.exists():
                reds.append(f"MISSING {name}: {rel} does not exist")
                continue
            text = p.read_text(encoding="utf-8", errors="replace")
            found = re.findall(pat, text)
            if not found:
                reds.append(
                    f"UNANCHORED {name}: {rel} no longer contains a sentence "
                    f"matching /{pat}/. The claim was reworded around its own "
                    f"number; re-anchor it or restate it. Derived value is {want}"
                )
                continue
            for got in found:
                if int(got) != want:
                    reds.append(
                        f"DRIFT {name}: {rel} says {got}, derived is {want} "
                        f"({_conv})"
                    )
    return reds


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return _selftest()

    values = derive()
    if len(QUANTITIES) < MIN_QUANTITIES:
        print(f"RED: {len(QUANTITIES)} quantities, floor is {MIN_QUANTITIES}. "
              "A census with nothing in it passes every rule below it.")
        return 1

    for name, (conv, _fn, anchors) in QUANTITIES.items():
        print(f"{name:<28} {values[name]:>5}   {conv}")
        if a.list:
            for rel, pat in anchors:
                print(f"{'':>34} quoted by {rel}")
            if not anchors:
                print(f"{'':>34} quoted by nothing — derived for reference only")
    if a.list:
        print("\nNEEDS-CORPUS — quoted here, not derivable from this repository:")
        for where, what in NEEDS_CORPUS:
            print(f"  {where}: {what}")

    if a.check:
        reds = check(values)
        for r in reds:
            print("\n" + r)
        if reds:
            print(f"\nscenario-census: {len(reds)} quoted figure(s) disagree "
                  "with the tree they describe.")
            return 1
        quoted = sum(len(v[2]) for v in QUANTITIES.values())
        print(f"\nscenario-census: {quoted} quoted figure(s) agree with the tree")
    return 0


def _selftest() -> int:
    ok = True

    def expect(label, cond):
        nonlocal ok
        if not cond:
            print(f"SELFTEST FAIL [{label}]", file=sys.stderr); ok = False
        else:
            print(f"SELFTEST ok   [{label}]")

    sample = (
        '[[setup]]\ndo = "hold"\nitem = "x"\n\n'
        '[[step]]\ndo = "container_click"\nslot = 54\n\n'
        '[[step]]\ndo = "container_click"\nslot = 55\n'
    )
    b = _blocks(sample)
    expect("blocks are split by table name", [n for n, _ in b] == ["setup", "step", "step"])
    expect("a setup `do` is not counted as a step `do`",
           sum(len(re.findall(r'^\s*do\s*=\s*"container_click"\s*$', body, re.M))
               for n, body in b if n == "step") == 2)
    expect("and the setup row is still visible under its own table",
           sum(len(re.findall(r'^\s*do\s*=\s*"hold"\s*$', body, re.M))
               for n, body in b if n == "setup") == 1)

    # Both failure directions, against a FIXTURE tree. The first version of
    # this self-test anchored on the real `720` and so failed for a reason
    # that had nothing to do with the rule being tested — a self-test that
    # depends on the tree it audits reports on the tree, not on itself.
    import tempfile
    global QUANTITIES
    real = QUANTITIES
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        (d / "ci").mkdir()
        (d / "ci" / "about.toml").write_text(
            'about = "39 `container_click` steps in 15 scenarios"\n')
        try:
            QUANTITIES = {"q": ("c", lambda: 39,
                                [("ci/about.toml", r"(\d+) `container_click` steps")])}
            expect("a figure that agrees is silent", check({"q": 39}, d) == [])
            reds = check({"q": 40}, d)
            expect("a figure that has drifted is caught",
                   len(reds) == 1 and reds[0].startswith("DRIFT"))
            expect("and the red prints both numbers",
                   reds and "says 39" in reds[0] and "derived is 40" in reds[0])

            QUANTITIES = {"q": ("c", lambda: 39,
                                [("ci/about.toml", r"(\d+) wombats per fortnight")])}
            reds = check({"q": 39}, d)
            expect("a sentence reworded around its number is UNANCHORED, not skipped",
                   len(reds) == 1 and reds[0].startswith("UNANCHORED"))

            QUANTITIES = {"q": ("c", lambda: 1, [("docs/NOPE.md", r"(\d+)")])}
            reds = check({"q": 1}, d)
            expect("an anchor pointing at a file that does not exist is caught",
                   len(reds) == 1 and reds[0].startswith("MISSING"))

            # Two quotes of one quantity, one right and one wrong: the rule
            # must adjudicate every occurrence, not the first.
            (d / "ci" / "two.toml").write_text(
                "39 `container_click` steps here and 34 `container_click` steps there\n")
            QUANTITIES = {"q": ("c", lambda: 39,
                                [("ci/two.toml", r"(\d+) `container_click` steps")])}
            reds = check({"q": 39}, d)
            expect("a second, wrong occurrence of the same claim is still caught",
                   len(reds) == 1 and "says 34" in reds[0])
        finally:
            QUANTITIES = real

    print("SELFTEST " + ("PASS" if ok else "FAIL"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
