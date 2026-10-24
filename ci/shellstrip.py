#!/usr/bin/env python3
"""Blank the parts of a shell script that are not code, position-preserving.

Shared by `ci/shell-footguns.py` and `ci/check-comment-guards.py`, and it is
shared rather than copied because both learned the same lesson from different
directions and a second copy would drift.

# WHY THIS EXISTS

**This repo's tooling reads its own source as data, so a comment or a fixture
is not inert.** Four defects so far:

* `ci/comment-guards-falsify.sh` was disabled by its own advice message — a
  `print("./ci/comment-guards-falsify.sh")` inside an advice string made
  `script_is_invoked` report the falsifier as live.
* A table path quoted inside a *comment* silently added that table to a
  required set, one character from triggering a twenty-minute generator.
* A raw grep for `pkill -f` over `ci/` returns twelve hits and four are
  comments WARNING against it, so a class checker without this would report the
  documentation of a defect as the defect and its ratchet would rise every time
  somebody wrote the lesson down.
* And the one that produced this file, 2026-08-19: wiring
  `ci/comment-guards-falsify.sh` into `ci/t0.sh` made its arm-4 FIXTURE — an
  `echo "zzfalsify sentinel absent"` inside a heredoc that writes a stand-in
  script into a temp tree — count as a live shell emitter of that sentinel in
  the real tree. Arm 3, whose whole job is to prove the rule fires when the
  only producer is a captured Rust print, silently stopped being able to go
  red. A check switched off by another check citing it.

# WHAT IS INERT DEPENDS ON THE LANGUAGE, AND THE LIST DOES NOT TRANSLATE

In Rust a string literal is data. **In shell a string is a place code runs**:
`TOTAL_RUNS="$(grep -c . f || echo 0)"` is a live instance sitting inside double
quotes. So `strings=False` is the default and callers that want them must ask.
What survives translation from the Rust rule is the principle — decide what is
inert in THIS language before searching — and not the list of things to strip.
"""

from __future__ import annotations

import re

__all__ = ["blank"]


def blank(text: str, comments: bool = True, heredocs: bool = True,
          strings: bool = False) -> str:
    """`text` with the requested regions replaced by spaces.

    Length- and newline-preserving, so an offset into the result is the same
    offset in the original and reported line numbers stay exact.

    A `#` only starts a comment at the start of a word — `${x#y}`, `a#b` and a
    `#` inside quotes are not comments. This tracks quoting well enough to make
    that distinction, and errs toward KEEPING text: a missed comment is a false
    positive a human sees and argues with, while an over-eager blank is an
    instance that silently stops being found, which is the failure a ratchet
    structurally cannot see — a declaration going stale looks like a repair.
    """
    out = list(text)
    i, n = 0, len(text)
    quote = None
    pending: list[str] = []
    line_start = 0

    def wipe(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        if c == "\n":
            quote = None
            i += 1
            while pending:
                term = pending.pop(0)
                while i < n:
                    eol = text.find("\n", i)
                    eol = n if eol == -1 else eol
                    if text[i:eol].strip() == term:
                        i = eol + 1 if eol < n else n
                        break
                    if heredocs:
                        wipe(i, eol)
                    i = eol + 1 if eol < n else n
            line_start = i
            continue
        if quote:
            if c == "\\" and quote == '"':
                if strings:
                    wipe(i, min(i + 2, n))
                i += 2
                continue
            if c == quote:
                quote = None
                i += 1
                continue
            if strings:
                out[i] = " "
            i += 1
            continue
        if c in "'\"":
            quote = c
            i += 1
            continue
        if c == "\\":
            i += 2
            continue
        m = re.match(r"<<-?\s*[\"']?([A-Za-z_][A-Za-z0-9_]*)[\"']?", text[i:])
        if m:
            pending.append(m.group(1))
            i += m.end()
            continue
        if c == "#" and (i == line_start or text[i - 1] in " \t;&|(){}"):
            eol = text.find("\n", i)
            eol = n if eol == -1 else eol
            if comments:
                wipe(i, eol)
            i = eol
            continue
        i += 1
    return "".join(out)
