#!/usr/bin/env python3
"""Refuse shell constructs under ci/ whose exit status lies.

# ===== AGENT INTERFACE =====
#
#     ci/check-shell-footguns.py              scan ci/**/*.sh, exit 1 on a hit
#     ci/check-shell-footguns.py --selftest   prove every rule bites, and that
#                                             the footgun is real on this box
#
# Wired into ci/t0.sh and ci/checks/required-checks.toml.
# ===== end interface =====
"""

# RULE `grepq-after-pipe` — no `| grep -q` (or `--quiet`, `--silent`)
#
# Under `set -o pipefail` a pipeline's status is its last NON-ZERO member's.
# `grep -q` exits on its first match; whatever is still writing into the pipe
# then takes SIGPIPE, exits 141, and the pipeline reports failure on a MATCH.
# So `producer | grep -q pat` is a check that can answer "absent" for a line
# that is present, and which way it answers depends on scheduling:
#
#   * gate `gate-instruments` 6cd236f54, 2026-09-24 09:20, went RED at
#     ci/gameplay-substrate-falsify.sh's --console check on an unchanged
#     ci/t2-gameplay.sh. Reproduced: 10,218 false negatives in 36,000 trials
#     (24 `nice -n 19` loops on a loaded box), 0 in 3,000 unloaded.
#   * with a shell builtin as the producer (`printf '%s' "$OUT" | grep -q`) the
#     write is one call up to the 64 KiB pipe buffer and the race cannot
#     happen; above it, it ALWAYS happens: 1,000 of 1,000 at 100 KB,
#     0 of 1,000 at 20 KB. A check whose output grows past 64 KiB flips.
#
# The repair is `grep -c pat >/dev/null`: same exit status (0 iff a line was
# selected), and `-c` reads to EOF so nothing upstream is cut off.
# Measured with GNU grep 3.11 on this box, 50 of 50 each: `seq 1 300000 |
# grep -q 1` -> 141 every time, `| grep -c 1 >/dev/null` -> 0 every time.
#
# The rule applies to every ci/ shell file, pipefail or not: `set -o pipefail`
# is inherited by anything that SOURCES a file (ci/reference-cache.sh and
# ci/server-ready.sh are sourced by pipefail scripts and set none themselves),
# and by every `$(...)`. Comment lines are skipped.

import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# A single `|` or `|&` (not `||`), optional `command`, then grep/egrep/fgrep.
# `args` sits inside a lookahead so a match consumes only the `| grep` itself:
# with a plain `(?P<args>.*)` the first pipe-grep on a line swallowed the rest
# of it, so `x | grep -c a >/dev/null && y | grep -q b` — the second grep
# being the footgun — was never examined. Found on train-c, 2026-09-30: three
# such second-on-the-line sites survived the first full sweep.
PIPE_GREP = re.compile(r"(?<![|])\|&?(?![|])\s*(?:command\s+)?[ef]?grep\b(?=(?P<args>.*))")
QUIET_CLUSTER = re.compile(r"-[A-Za-z]*q[A-Za-z]*")


def quiet(args: str) -> bool:
    """Does this grep's argument list carry -q/--quiet/--silent before `--`
    or the first non-option word? Stops at the next unquoted pipe."""
    for tok in args.split():
        if tok.startswith("|") or tok in ("&&", "||", ";", "then", "do"):
            return False
        if tok == "--":
            return False
        if tok in ("--quiet", "--silent"):
            return True
        if QUIET_CLUSTER.fullmatch(tok):
            return True
        if not tok.startswith("-"):
            # first operand (the pattern) ends the options for our purposes;
            # a -q written after it is still an option to GNU grep, so keep
            # scanning only option-shaped words.
            continue
    return False


def scan_text(text: str):
    hits = []
    for n, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("#"):
            continue
        for m in PIPE_GREP.finditer(line):
            args = m.group("args")
            # only this grep's own words: cut at the next single pipe
            args = re.split(r"(?<![|])\|(?![|])", args, maxsplit=1)[0]
            if quiet(args):
                hits.append((n, line.strip()))
                break
    return hits


def scan_tree(root: Path):
    out = []
    for f in sorted((root / "ci").glob("**/*.sh")):
        for n, line in scan_text(f.read_text(errors="replace")):
            out.append((f.relative_to(root), n, line))
    return out


def selftest() -> int:
    bad = [
        "x | grep -q pat",
        "x |grep -qE 'a|b' && echo y",
        "x 2>&1 | grep -q -- --flag",
        "x |& grep --quiet pat",
        "x | grep --silent pat",
        "x | command grep -Fq pat",
        'v="$(printf %s "$o" | grep -qF lit && echo 0 || echo 1)"',
        "x | egrep -q 'a'",
        "if ! x | grep -qi pat; then :; fi",
        "x | grep -Eqx pat",
        "x | grep -c a >/dev/null && y | grep -q b",
    ]
    good = [
        "grep -q pat file",
        "x || grep -q pat file",
        "# x | grep -q pat   (a comment)",
        "x | grep -c pat >/dev/null",
        "x | grep -cE 'a|b' >/dev/null && echo y",
        "x | grep pat",
        "x | grep -v q",
        "x | sed -n 1p | grep -o 'q'",
    ]
    fails = 0
    for s in bad:
        if not scan_text(s):
            print(f"SELFTEST FAIL  not refused: {s}")
            fails += 1
    for s in good:
        if scan_text(s):
            print(f"SELFTEST FAIL  refused:     {s}")
            fails += 1
    # The mechanism, live: if this box's grep did NOT make `| grep -q` lie
    # under pipefail, the rule would be guarding nothing here — say so.
    def rc(cmd):
        return subprocess.run(["bash", "-c", "set -o pipefail; " + cmd],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode
    lie = rc("seq 1 300000 | grep -q 1")
    fixed = rc("seq 1 300000 | grep -c 1 >/dev/null")
    if lie == 0:
        print("SELFTEST FAIL  `seq 1 300000 | grep -q 1` under pipefail exited 0 here; "
              "the footgun this rule exists for did not reproduce")
        fails += 1
    if fixed != 0:
        print(f"SELFTEST FAIL  the repair `| grep -c 1 >/dev/null` exited {fixed}")
        fails += 1
    # The tree scan, on a planted file.
    with tempfile.TemporaryDirectory() as d:
        p = Path(d)
        (p / "ci" / "lib").mkdir(parents=True)
        (p / "ci" / "lib" / "planted.sh").write_text("set -uo pipefail\nfoo | grep -q bar || exit 1\n")
        (p / "ci" / "clean.sh").write_text("foo | grep -c bar >/dev/null || exit 1\n")
        hits = scan_tree(p)
        if [str(h[0]) for h in hits] != ["ci/lib/planted.sh"]:
            print(f"SELFTEST FAIL  tree scan on a planted ci/lib file returned {hits}")
            fails += 1
    print(f"SHELL-FOOTGUNS SELFTEST {'PASS' if fails == 0 else 'FAIL'} — "
          f"{len(bad)} refused forms, {len(good)} accepted forms, live pipefail lie rc={lie}, repair rc={fixed}")
    return 0 if fails == 0 else 1


def main() -> int:
    if "--selftest" in sys.argv[1:]:
        return selftest()
    hits = scan_tree(ROOT)
    for f, n, line in hits:
        print(f"GREPQ-AFTER-PIPE  {f}:{n}: {line[:160]}")
    if hits:
        print(f"SHELL-FOOTGUNS RED — {len(hits)} `| grep -q` site(s). Under pipefail a match can "
              "read as absent (SIGPIPE, 141); write `| grep -c PAT >/dev/null`.")
        return 1
    n = len(list((ROOT / "ci").glob("**/*.sh")))
    print(f"SHELL-FOOTGUNS OK — 0 `| grep -q` sites in {n} ci shell files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
