#!/usr/bin/env python3
"""Reading `ci/checks/gameplay/*.toml` — one implementation, imported by three.

    import check_files as cf
    cf.load()          # {scenario name: Check}
    cf.observers()     # {kind: [check file names that name it]}

Underscore in the filename because it is imported as a module, the same reason
`ci/trace_vocabulary.py` has one.

# Why this is shared rather than copied

`tools/oracle-runner/src/compare.rs` makes the argument for its own strip and
it applies here: *two implementations of one strip are two answers waiting for
an occasion.* Three scripts on this branch needed the same two things — the
`observe` list of every check, and the inverse map from kind to checks — and
each had its own copy within an hour of the others.

The strip is not incidental. **`607`'s `observe` list carries a prose comment
that quotes `"minecraft:stone"`**, and a reader that does not remove TOML
comments takes that for an observed kind. It cost a false finding the first
time this was written: three "kinds" appeared in a census —`count`, `item` and
`minecraft:stone` — that are a field name, a field name and an item id, none
of them anything a decoder emits. Two of the three scripts here would have
reported that check as observing a kind no arm produces, which is a refusal in
`ci/projection-drops.py`, so a copy that skipped the strip would have turned a
comment into a T0 red.
"""

from __future__ import annotations

import glob
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECK_DIR = os.path.join(ROOT, "ci/checks/gameplay")


def strip_comments(text: str) -> str:
    """TOML comments removed, respecting quotes.

    A `#` inside a string is data; a `#` outside one starts a comment. Nothing
    cleverer is needed — these files have no multi-line basic strings — and
    anything cleverer would be a second TOML parser with its own bugs.
    """
    out = []
    for line in text.splitlines():
        quoted = False
        for i, ch in enumerate(line):
            if ch == '"':
                quoted = not quoted
            elif ch == "#" and not quoted:
                line = line[:i]
                break
        out.append(line)
    return "\n".join(out)


class Check:
    """One check file, as much of it as anything here reads."""

    __slots__ = ("name", "file", "observe", "observe_from", "within")

    def __init__(self, name, file, observe, observe_from, within):
        self.name = name
        self.file = file
        self.observe = observe
        self.observe_from = observe_from
        self.within = within

    def __repr__(self):
        return f"Check({self.file!r}, observe={self.observe!r}, from={self.observe_from!r})"


def load(check_dir: str | None = None) -> dict[str, Check]:
    """Every check, keyed by the scenario `name` the recordings use.

    Keyed by `name` and not by filename because that is what joins a check to
    a recording: `report.json` and both trace files carry the scenario name,
    while the numeric prefix lives only in the filename.
    """
    checks: dict[str, Check] = {}
    for path in sorted(glob.glob(os.path.join(check_dir or CHECK_DIR, "*.toml"))):
        with open(path, encoding="utf-8") as fh:
            text = strip_comments(fh.read())
        name = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
        if not name:
            continue
        obs = re.search(r"^observe\s*=\s*\[(.*?)\]", text, re.M | re.S)
        frm = re.search(r'^observe_from\s*=\s*"([a-z]+)"', text, re.M)
        wth = re.search(r"^within\s*=\s*(-?\d+)", text, re.M)
        checks[name.group(1)] = Check(
            name=name.group(1),
            file=os.path.basename(path)[:-5],
            observe=re.findall(r'"([^"]+)"', obs.group(1)) if obs else None,
            # `both` is the default, and it is the default in `scenario.rs`
            # too — a check that says nothing about which client it reads gets
            # both of them, so a missing key is not a missing declaration.
            observe_from=frm.group(1) if frm else "both",
            within=int(wth.group(1)) if wth else None,
        )
    return checks


def observers(check_dir: str | None = None) -> dict[str, list[str]]:
    """kind -> the check files that name it in their own `observe` list."""
    by_kind: dict[str, list[str]] = {}
    for check in load(check_dir).values():
        for kind in check.observe or ():
            by_kind.setdefault(kind, []).append(check.file)
    return by_kind
