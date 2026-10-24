#!/usr/bin/env python3
"""Prove `ci/shell-footguns.py` can go red, and that its stripper does not lie.

    python3 ci/shell-footguns-falsify.py     # ~1 s, no build, no jar, no box

# THE CASE THAT IS THE WHOLE ARGUMENT is `comment-is-not-code`. A raw grep for
# `pkill -f` over this tree returns twelve hits in `ci/` and four of them are
# COMMENTS WARNING AGAINST IT, including the two that document the mechanism.
# A checker without the stripper would report the documentation of a defect as
# the defect, and — worse — its number would rise every time somebody wrote a
# warning, so the ratchet would punish recording the lesson.
#
# That is not a hypothetical about this checker. It is the class:
# `ci/comment-guards-falsify.sh` was disabled by its own advice message,
# because a `print("./ci/comment-guards-falsify.sh")` inside an advice string
# made `script_is_invoked` believe the falsifier was live. This repo's tooling
# reads its own source as data, so a comment is not inert.
#
# `hash-in-a-parameter-expansion` is the control on the stripper going too far.
# `${x#y}` and `a#b` are not comments, and an over-eager blank would silently
# stop finding instances after them — which is the failure mode the ratchet
# cannot see, because a declaration that goes stale looks like a repair.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAIL = 0

RATCHET = '''total = {n}

[allowed]
{entries}
'''


def run(files: dict[str, str], ratchet: str) -> tuple[int, str]:
    tmp = Path(tempfile.mkdtemp())
    try:
        (tmp / "ci" / "checks").mkdir(parents=True)
        shutil.copy(ROOT / "ci" / "shell-footguns.py", tmp / "ci" / "shell-footguns.py")
        # The stripper is a sibling module, not a stdlib import. A fixture that
        # forgot it would fail every case with a traceback rather than a
        # verdict, which is a falsifier reporting on its own plumbing.
        shutil.copy(ROOT / "ci" / "shellstrip.py", tmp / "ci" / "shellstrip.py")
        for name, body in files.items():
            (tmp / "ci" / name).write_text(body)
        (tmp / "ci" / "checks" / "shell-footguns.toml").write_text(ratchet)
        p = subprocess.run(
            [sys.executable, str(tmp / "ci" / "shell-footguns.py")],
            capture_output=True, text=True,
        )
        return p.returncode, p.stdout + p.stderr
    finally:
        shutil.rmtree(tmp)


def case(label, files, ratchet, want_rc, must=None):
    global FAIL
    rc, out = run(files, ratchet)
    if rc != want_rc:
        print(f"FAIL {label}: exit {rc}, wanted {want_rc}")
        print("     " + out.replace("\n", "\n     ")[:500])
        FAIL = 1
        return
    if must and must not in out:
        print(f"FAIL {label}: exit is right but the output never says {must!r}")
        print("     " + out.replace("\n", "\n     ")[:500])
        FAIL = 1
        return
    print(f"ok   {label}")


EMPTY = RATCHET.format(n=0, entries="")

print("-- the two idioms are found --")
case("pkill -f is found",
     {"a.sh": 'cleanup() {\n    pkill -f "$WORK"\n}\n'},
     EMPTY, 1, "PKILL-F-MATCHES-ITS-CALLER")
case("pkill -9 -f is found (flags before -f)",
     {"a.sh": "pkill -9 -f 'sleep 600'\n"},
     EMPTY, 1, "PKILL-F-MATCHES-ITS-CALLER")
case("grep -c ... || echo 0 is found",
     {"a.sh": 'N="$(grep -c . "$W/f" 2>/dev/null || echo 0)"\n'},
     EMPTY, 1, "GREP-C-OR-ECHO-ZERO")

print("-- declaring one silences it, and the count must match --")
case("a declared instance is green",
     {"a.sh": 'cleanup() {\n    pkill -f "$WORK"\n}\n'},
     RATCHET.format(n=1, entries='"ci/a.sh:2" = "PKILL-F-MATCHES-ITS-CALLER"'), 0)
case("a declaration that is no longer found is STALE",
     {"a.sh": "echo nothing here\n"},
     RATCHET.format(n=1, entries='"ci/a.sh:2" = "PKILL-F-MATCHES-ITS-CALLER"'), 1, "STALE")
case("total disagreeing with the entry count is RED",
     {"a.sh": "echo nothing here\n"},
     RATCHET.format(n=7, entries=""), 1, "keep them equal")

print("-- the stripper: comments and heredocs are not code --")
case("comment-is-not-code: a WARNING about pkill -f is not an instance",
     {"a.sh": "# never use `pkill -f`: it matches its own caller's command line\n"
              "#     pkill -f \"$WORK\"\n"
              "echo fine\n"},
     EMPTY, 0)
case("comment-is-not-code: a trailing comment on a real line",
     {"a.sh": "echo fine   # pkill -f would be wrong here\n"},
     EMPTY, 0)
case("heredoc-is-not-code: an example inside a heredoc body",
     {"a.sh": "cat <<'EOF'\npkill -f \"$WORK\"\nEOF\necho fine\n"},
     EMPTY, 0)
case("a warning ABOUT grep -c || echo 0 is not an instance",
     {"a.sh": "# `grep -c . f || echo 0` emits \"0\\n0\" — use awk\n"},
     EMPTY, 0)

print("-- the stripper does not go too far (the failure a ratchet cannot see) --")
case("hash-in-a-parameter-expansion: ${x#y} does not start a comment",
     {"a.sh": 'v=${x#y}; pkill -f "$WORK"\n'},
     EMPTY, 1, "PKILL-F-MATCHES-ITS-CALLER")
case("a # inside a quoted string does not start a comment",
     {"a.sh": 'echo "tag #1"; pkill -f "$WORK"\n'},
     EMPTY, 1, "PKILL-F-MATCHES-ITS-CALLER")
case("code AFTER a heredoc terminator is still scanned",
     {"a.sh": "cat <<'EOF'\nharmless\nEOF\npkill -f \"$WORK\"\n"},
     EMPTY, 1, "PKILL-F-MATCHES-ITS-CALLER")

print("-- negative controls: the near neighbours that are NOT the defect --")
case("pgrep -f is the RECOMMENDED form and must not be flagged",
     {"a.sh": "for pid in $(pgrep -f '[c]i/handoff.sh'); do :; done\n"},
     EMPTY, 0)
case("grep -c with no `|| echo 0` is fine",
     {"a.sh": 'N="$(grep -c . "$W/f")"\n'},
     EMPTY, 0)
case("awk END{print NR} — the repair — is fine",
     {"a.sh": 'N="$(awk \'END{print NR}\' "$W/f" 2>/dev/null || echo 0)"\n'},
     EMPTY, 0)

if FAIL == 0:
    print()
    print("SHELL-FOOTGUNS-FALSIFY GREEN — 16 cases: both idioms found, the ratchet")
    print("  reds on new and on stale and on a count that disagrees, comments and")
    print("  heredoc bodies are not instances, a `#` that is not a comment does not")
    print("  blank the rest of its line, and pgrep/awk/plain grep -c stay green")
else:
    print("SHELL-FOOTGUNS-FALSIFY RED")
sys.exit(FAIL)
