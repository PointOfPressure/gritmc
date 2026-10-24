#!/usr/bin/env python3
"""Two shell idioms this project has now got wrong more than once.

    ./ci/shell-footguns.py            # exit 1 on an undeclared instance
    ./ci/shell-footguns.py --emit     # regenerate the ratchet

# THE TWO, AND WHY THEY ARE HERE RATHER THAN IN A COMMENT

**`pkill -f PATTERN` matches its own caller's command line.** It killed a caller
five separate times in one run. Twice it was already documented when it
happened: `ci/batch.sh:372` names the mechanism and `ci/batch-falsify.sh:67`
says "PID-resolved, never `pkill -f`". Being written down is what this class
has already had, and it is not what stops it. The safe form is in the tree
already — `pgrep -f '[c]i/handoff.sh'`, skip `$$`, then compare something out of
`/proc/<pid>/environ` — and it is not a bracketing trick: `ci/batch.sh` says
plainly that "the bracket form is not enough when the same command line also
carries the plain path".

**`cmd | grep -c X || echo 0` emits `0\\n0`.** `grep -c` PRINTS 0 and EXITS 1 on
no match, so the `||` arm fires on top of the count that was already written.
Every numeric comparison against the result then fails, and it fails as a
syntax error rather than as a tally of zero, which is why it reads as a defect
somewhere else. It broke four cases of `ci/batch-falsify.sh` on its first run
and read as a defect in `ci/batch.sh`. The form with one answer is
`awk 'END{print NR}'`.

# COMMENTS AND HEREDOCS ARE STRIPPED FIRST (via `ci/shellstrip.py`), AND THAT
# IS NOT TIDINESS

**This repo's tooling reads its own source as data, so a comment is not inert.**
Two defects came from exactly that. `ci/comment-guards-falsify.sh` was disabled
by its own advice message — a `print("./ci/comment-guards-falsify.sh")` inside
an advice string made `script_is_invoked` believe the falsifier was live. And a
table path quoted inside a *comment* silently added that table to a required
set, one character away from triggering a twenty-minute generator on every
tree.

Both are the same shape as this file's own risk: a raw grep for `pkill -f`
across this tree returns hundreds of hits, and nearly every one is a comment
warning against it — including the two above. A checker that counted those
would report the documentation of a defect as the defect, and its number would
go UP every time somebody wrote a warning. So comments and heredoc bodies are
blanked before the search, position-preserving so line numbers stay exact.

**String literals are deliberately NOT stripped**, and that is the difference
between shell and Rust. `TOTAL_RUNS="$(grep -c . f || echo 0)"` is a live
instance sitting inside double quotes: in shell a string is a place code runs.
The general rule from those two defects is "strip comments, string literals and
heredoc bodies before any such search", and it is right for Rust and for a
reference scan; applied literally to shell command substitution it would blank
the instances this file exists to find. What survives translation is the
principle — decide what is inert in THIS language before searching — and not
the list.

# THE RATCHET

`ci/checks/shell-footguns.toml`, in the shape of `ci/checks/comment-guards.toml`
and for the same reason: the live instances are in falsifier cleanup paths that
work today, and rewriting eight of them blind is a worse risk than declaring
them. A declared instance that stops being found is as red as a new one, so a
repair must lower the count rather than being absorbed silently.

Exit 0 clean, 1 an undeclared instance or a stale declaration.
"""

from __future__ import annotations

import importlib.util
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RATCHET = ROOT / "ci" / "checks" / "shell-footguns.toml"

# The stripper lives in `ci/shellstrip.py` because `ci/check-comment-guards.py`
# needs the identical one, and it needs it for a defect this file's own rule
# would otherwise have caused. See that module's header.
_spec = importlib.util.spec_from_file_location("shellstrip", ROOT / "ci" / "shellstrip.py")
shellstrip = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(shellstrip)

RULES = [
    (
        "PKILL-F-MATCHES-ITS-CALLER",
        re.compile(r"\bpkill\s+(?:-\S+\s+)*-\w*f\w*\b"),
        "`pkill -f` matches the pattern in its own caller's command line. "
        "Resolve pids with `pgrep -f '[c]i/x.sh'`, skip $$, and compare "
        "/proc/<pid>/environ — see ci/batch.sh's GUARD 4.",
    ),
    (
        "GREP-C-OR-ECHO-ZERO",
        re.compile(r"grep\s+-\w*c\w*[^|&\n]*\|\|\s*echo\s+0"),
        "`grep -c` prints 0 AND exits 1 on no match, so this emits \"0\\n0\". "
        "Use `awk 'END{print NR}'`.",
    ),
]


def shell_files():
    for pat in ("ci/*.sh", "tools/**/*.sh", "swarm/*.sh", "hooks/*.sh"):
        for p in sorted(ROOT.glob(pat)):
            yield p


def scan() -> list[tuple[str, str, str]]:
    hits = []
    for p in shell_files():
        try:
            text = p.read_text()
        except OSError:
            continue
        code = shellstrip.blank(text, comments=True, heredocs=True, strings=False)
        for name, rx, advice in RULES:
            for m in rx.finditer(code):
                line = code.count("\n", 0, m.start()) + 1
                hits.append((f"{p.relative_to(ROOT)}:{line}", name, advice))
    return sorted(hits)


def main() -> int:
    hits = scan()
    if "--emit" in sys.argv:
        print(f"total = {len(hits)}\n\n[allowed]")
        for loc, name, _ in hits:
            print(f'"{loc}" = "{name}"')
        return 0

    if not RATCHET.exists():
        print(f"SHELL-FOOTGUNS RED — no ratchet at {RATCHET}. `--emit` writes one.")
        return 1
    r = tomllib.load(open(RATCHET, "rb"))
    declared = r.get("allowed", {})
    found = {loc: name for loc, name, _ in hits}
    advice = {name: a for name, _, a in RULES}

    new = [(loc, n) for loc, n in found.items() if loc not in declared]
    stale = [loc for loc in declared if loc not in found]

    if r.get("total") != len(declared):
        print(
            f"SHELL-FOOTGUNS RED — `total = {r.get('total')}` but {len(declared)} "
            f"entries are listed. The count is the ratchet; keep them equal."
        )
        return 1

    if new or stale:
        print(f"SHELL-FOOTGUNS RED — {len(new)} undeclared, {len(stale)} stale")
        for loc, n in sorted(new):
            print(f"  NEW    {loc}  {n}")
            print(f"         {advice[n]}")
        for loc in sorted(stale):
            print(f"  STALE  {loc}  declared and not found.")
            print("         If it was repaired, delete the line and lower `total`.")
            print("         A stale declaration reads as authoritative and is worse than none.")
        return 1

    print(
        f"SHELL-FOOTGUNS OK — {len(declared)} declared, 0 new, 0 stale "
        f"({len(list(shell_files()))} shell files, comments and heredocs blanked)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
