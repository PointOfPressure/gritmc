#!/usr/bin/env python3
"""Derive the census of checks in `ci/` whose green cannot mean anything.

    python3 ci/vacuous-green-census.py            # print the census as TOML
    python3 ci/vacuous-green-census.py --write    # write ci/checks/vacuous-green.toml
    python3 ci/vacuous-green-census.py --json     # the same census as JSON

# WHY THIS EXISTS

This project's central defect is *an instrument that measures its own input, or
a verdict whose green cannot be turned red* (`docs/AGENT-BRIEF.md`, HARD RULE).
It has been found one instance at a time, always by a lane that was looking for
something else. Four more turned up on 2026-09-05 in a single night, none of
them by anybody hunting for them:

* `ci/figure-provenance.py`, called from `ci/t0.sh`, needs git history; the
  gate's rsync excludes `.git`, so in every gate directory it printed "not
  inside a git repository" and exited **0**;
* `ci/check-cited-audits.py` has the identical shape at the identical caller
  and is not repaired;
* `ci/decoded-kind-reach.py` is a ratchet with a committed baseline that no
  gate invokes;
* a declared-commands baseline shipped `provenance = "derived"`, and a derived
  baseline keeps exit 0, so a week of corpus runs printed `this run DISAGREES
  with it` and passed.

Finding them one at a time costs an agent each. This file derives the whole
population mechanically, so a new instance is refused rather than discovered.

# WHAT IT DERIVES, AND WHAT IT DELIBERATELY DOES NOT

Two shapes are decidable from the source and both are derived here:

**`uninvoked`** — a script that *declares a verdict* (it has a non-zero exit
whose adjacent message says RED / REFUSED / FAIL / UNREACHABLE) and that
nothing under `ci/`, `tools/`, `crates/`, `hooks/`, `swarm/` or the repo root
runs. A check nothing calls is green by never being asked.

**`green_on_refusal`** — an exit-**0** path whose adjacent message says the
check could not do its job ("not a git repository", "skipping", "no fixtures",
"cannot"). That is the `figure-provenance` shape exactly: the honest sentence
is printed and the exit status says PASS, so a gate reading the status learns
nothing and a gate reading the log has to be a person.

Two more shapes are real and are **not** derived, because a static derivation
of them would be a guess and a guess in this file is worse than a gap:

* a check whose failure path is gated behind a data field that disables it
  (the `provenance = "derived"` instance). Detecting it needs the semantics of
  the field.
* a check whose fixture cannot produce the thing it looks for.

Those are carried as hand-adjudicated entries in `ci/check-vacuous-green.py`'s
`DECLARED` table with the evidence that established them, and this file's job
for them is only to keep the population count honest.

# WHAT AN INVOCATION IS

The same rule `ci/check-checks-present.py` states, for the same reason: a
non-comment line where the path is preceded by something that *runs* it
(`python3 ci/x.py`, `./ci/x.sh`, `bash ci/x.sh`, `"$ROOT/ci/x.py"`). A path in
backticks in a sentence is a mention. Rust source is scanned too, because a
golden regenerator is named by the test that consumes its output and that is
the only place it is named.

**A script's own file does not count as a call site**, nor does its own
`*-falsify.sh` / `*-selftest.sh` sibling: a check exercised only by the script
written to exercise it is not wired into anything, and crediting that mention
is precisely how `ci/check-checks-present.py`'s first two versions read their
own fixtures as coverage.

# WHY VERDICT-DECLARING AND NOT EVERY SCRIPT

`ci/` holds 319 scripts and most are not checks. `ci/surface-golden.sh`
regenerates golden vectors and is *meant* to be run by hand; calling it
"uninvoked" would bury the four real instances in 170 false ones. The
discriminator is whether the script emits a verdict on a non-zero exit, which
is the thing a gate reads. It is a proxy and it is stated here so that a reader
can see what it lets through rather than having to re-derive it.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CI = ROOT / "ci"
CENSUS = ROOT / "ci" / "checks" / "vacuous-green.toml"

# Trees scanned for call sites. `target` is excluded by path check below.
SCAN_DIRS = ("ci", "tools", "crates", "hooks", "swarm", "plugins")
SCAN_SUFFIXES = (".sh", ".py", ".rs", ".toml", ".service", ".timer", "")

# `t0_spawn` / `t0_prefetched` (ci/t0-stages.sh, main 3973) are runners too:
# they start the command in a snapshot and replay it at its call site. Without
# them the census read three T0 stages as "uninvoked" on train-c.
# A runner immediately before the path. Anchored on a command position so that
# `# see ci/x.sh` and `"the ci/x.sh gate"` are mentions, not invocations.
INVOCATION = re.compile(
    r"""(?:^|[;&|(`]|\bthen\b|\bdo\b|\belse\b|\|\||&&|\$\()\s*
        (?:[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)\s+)*
        (?:timeout\s+\S+\s+)?(?:exec\s+)?(?:nohup\s+)?(?:sudo\s+)?
        (?:t0_spawn\s+|t0_prefetched\s+)?
        (?:python3?|bash|sh|zsh|source|\.|\./|)\s*
        (?:"?\$\{?(?:ROOT|CI|R|REPO|TREE|SRC|W|DIR)\}?/)?
        "?(?:\./)?(ci/[A-Za-z0-9._-]+\.(?:py|sh))""",
    re.VERBOSE,
)
# Rust and TOML name a script as a bare string; there is no runner token there.
QUOTED_PATH = re.compile(r"""["'](?:\.\./)*(ci/[A-Za-z0-9._-]+\.(?:py|sh))["']""")

# A message that says the run FAILED. Deliberately word-anchored: `RED` inside
# `REDSTONE` and `FAIL` inside `FAILURES_SEEN` are not verdicts.
VERDICT_WORD = re.compile(r"\b(RED|REFUSED|UNREACHABLE|MALFORMED|STALE)\b|\bFAIL(?:ED|S|URE)?\b")

# A message that says the check could not do its job. These are the words the
# four known instances actually used, plus the near neighbours; every match is
# reported with its line so a reader adjudicates rather than trusts the list.
REFUSAL_WORD = re.compile(
    r"\bSKIPPED\b|\bskipping\b|\bskipped\b|\bNOT RE-MEASURED\b|\bNOT CAPTURED\b|"
    r"\bnot inside\b|\bnot a git\b|\bno \.git\b|"
    r"\bcannot\b|\bcould not\b|\bunable\b|\bunavailable\b|"
    r"\bis missing\b|\bare missing\b|\bwent unchecked\b|\bnot checked\b|"
    r"\bno citations\b|\bno recordings?\b|\bno fixtures?\b|\bnothing is being\b|"
    r"\bno entries\b|\bnothing registered\b"
)

# A message the check disowns: an exit-0 whose window also carries a PASS
# verdict is a green announcement, not a refusal. `ci/check-fixture-skips.py`
# prints "every one is a check that passes when its fixture is absent" three
# lines above its `return 0`, which is prose about the check's subject and not
# a statement that this run could not run.
GREEN_WORD = re.compile(r"\b(GREEN|PASS|PASSED|OK)\b")

# ===========================================================================
# THE `set -e` DEAD HANDLER — 2026-09-06
# ===========================================================================
#
# `cmd; VAR=$?` is a sequential list, and errexit is NOT suspended in one. Under
# `set -e` the shell exits AT `cmd` with its status, and every line after it —
# the `case`, the `if`, the message that names the verdict — is dead code that
# reads exactly like a handled outcome.
#
# It shipped in `ci/t0.sh` at f0a46166a as
# `python3 ci/order-selfconsistency.py --check > /dev/null; OSC=$?` with a
# `case` handling exit 2 specially, so a stage whose designed-for outcome had
# arrived printed neither `T0 GREEN` nor `T0 RED` nor its own UNAVAILABLE
# message, and a waiter keyed on those strings saw nothing at all. Repaired on
# `origin/gate-instruments` 5adcd3f6c by rewriting it as `... || { ... }`.
#
# Proof of the shape, which needs no repository:
#
#     $ bash -c 'set -euo pipefail; false; RC=$?; echo "reached RC=$RC"'; echo "exit=$?"
#     exit=1
#
# Nothing is printed. The handler cannot run.
#
# Detected in two spellings — the same line (`cmd; VAR=$?`) and the next line
# (`cmd` then a bare `VAR=$?`) — with the errexit state tracked through every
# `set -e` / `set +e`, and with heredoc bodies skipped. **The heredoc skip is
# not a nicety.** `ci/handoff.sh` line 232 is `set -uo pipefail` and its five
# `VAR=$?` captures are correct; the `set -euo pipefail` at 1124 is inside a
# `<<REMOTE_EOF` body shipped to the box, and crediting it to the enclosing
# shell reported the whole gate pipeline as broken.
SET_E = re.compile(r"^\s*set\s+(?:-[a-zA-Z]*e[a-zA-Z]*|-o\s+errexit)\b")
SET_PLUS_E = re.compile(r"^\s*set\s+(?:\+[a-zA-Z]*e[a-zA-Z]*|\+o\s+errexit)\b")
HEREDOC = re.compile(r"<<-?\s*[\'\"]?([A-Za-z_][A-Za-z0-9_]*)[\'\"]?")
CAPTURE_SAME = re.compile(
    r"^(?!\s*#).*[^|&;];\s*(?:local\s+)?[A-Za-z_][A-Za-z0-9_]*=\$\?\s*(?:#.*)?$"
)
CAPTURE_NEXT = re.compile(r"^\s*(?:local\s+)?[A-Za-z_][A-Za-z0-9_]*=\$\?\s*(?:#.*)?$")
# A previous line that already suspends errexit for its command: a pipeline, a
# `||`/`&&` list, a continuation, or a compound-command keyword.
SUSPENDS = re.compile(r"(\|\||&&|\||\\|\bthen\b|\bfi\b|\bdo\b|\bdone\b|;|\{|\))$")


def errexit_dead_captures(lines: list[str]) -> list[dict]:
    """Every `VAR=$?` under an active `set -e` — a handler that cannot run."""
    out: list[dict] = []
    active = False
    heredoc: str | None = None
    for i, line in enumerate(lines):
        if heredoc is not None:
            if line.strip() == heredoc:
                heredoc = None
            continue
        m = HEREDOC.search(line)
        if m and not line.strip().startswith("#"):
            heredoc = m.group(1)
        if SET_E.match(line):
            active = True
            continue
        if SET_PLUS_E.match(line):
            active = False
            continue
        if not active:
            continue
        if CAPTURE_SAME.match(line):
            out.append({"line": i + 1, "form": "same-line", "text": line.strip()[:150]})
        elif CAPTURE_NEXT.match(line):
            j = i - 1
            while j >= 0 and (not lines[j].strip() or lines[j].strip().startswith("#")):
                j -= 1
            prev = lines[j].strip() if j >= 0 else ""
            if not prev or SUSPENDS.search(prev):
                continue
            if prev.split(" ")[0] in ("if", "while", "until", "!"):
                continue
            out.append(
                {"line": i + 1, "form": "next-line", "text": (prev + "  >>  " + line.strip())[:150]}
            )
    return out


SAME_LINE_NONZERO = re.compile(r"\b(?:return|exit)\s+[1-9]|\bsys\.exit\(\s*[1-9]")
EXIT_ZERO = re.compile(r"^\s*(?:sys\.)?exit\(0\)|^\s*return 0\b|^\s*exit 0\b|^\s*exit\s*$")
EXIT_NONZERO = re.compile(r"^\s*(?:sys\.)?exit\(\s*[1-9]|^\s*return [1-9]|^\s*exit [1-9]")
EMITS = re.compile(r"\b(print|echo|printf|cat >&2|warn|die)\b")

# How far back from an exit an emission still counts as its message. Six lines
# covers a wrapped f-string or a multi-line `echo` block and no more; a wider
# window starts crediting the *previous* branch's message to this exit.
WINDOW = 6

# A floor guard: the check refuses an empty or undersized population rather
# than reporting green over one. Its absence is a CANDIDATE, never a verdict.
FLOOR_GUARD = re.compile(
    r"FLOOR|_MIN\b|MIN_|at least|refuse.{0,40}empty|empty.{0,40}refuse|"
    r"len\([^)]*\)\s*==\s*0|len\([^)]*\)\s*<\s*[0-9]|"
    r"-eq 0 \]|-lt [0-9]+ \]|\bnot rows\b|\bnot entries\b|\bno entries\b"
)


def strip_comment(line: str, py: bool) -> str:
    """The code part of a line, so a `#` comment cannot look like a call."""
    s = line.strip()
    if s.startswith("#") or s.startswith("//") or s.startswith("*"):
        return ""
    return line


def scan_files() -> list[Path]:
    out: list[Path] = []
    for d in SCAN_DIRS:
        dd = ROOT / d
        if not dd.is_dir():
            continue
        for p in dd.rglob("*"):
            if not p.is_file() or "target" in p.parts or ".git" in p.parts:
                continue
            # `ci/checks/` holds DATA — ledgers, manifests, and this file's own
            # census. A name in a manifest is a declaration, never a call, and
            # crediting one is not a hypothetical: the first run of this script
            # wrote `script = "ci/decoded-kind-reach.py"` into
            # `ci/checks/vacuous-green.toml`, the next run read its own output
            # back as a call site, and `uninvoked_verdict_scripts` went 18 -> 0
            # with nothing in the tree having changed. A census that launders
            # its own findings into evidence is the defect class it exists to
            # find, committed by the finder.
            if "checks" in p.parts and p.suffix == ".toml":
                continue
            if p.suffix in (".sh", ".py", ".rs", ".toml", ".service"):
                out.append(p)
    out += [p for p in ROOT.glob("*.sh")]
    return out


def is_own_harness(caller: Path, script: str) -> bool:
    """True when `caller` is the script itself or the sibling written to test it.

    `ci/x-falsify.sh` and `ci/x-selftest.sh` exercise `ci/x.py`; that is not
    wiring, it is the test. Crediting it is how a check that nothing runs looks
    invoked. `ci/check-checks-present.py` states the same exclusion.
    """
    stem = script.rsplit("/", 1)[-1].rsplit(".", 1)[0]
    cname = caller.name
    if cname == script.rsplit("/", 1)[-1]:
        return True
    for suffix in ("-falsify.sh", "-falsify.py", "-selftest.sh", "-selftest.py"):
        if cname == stem + suffix:
            return True
    return False


def build_invocations(scripts: list[str], files: list[Path]) -> dict[str, list[tuple[str, int]]]:
    inv: dict[str, list[tuple[str, int]]] = {s: [] for s in scripts}
    known = set(scripts)
    for f in files:
        try:
            text = f.read_text(errors="replace")
        except OSError:
            continue
        py = f.suffix in (".py", ".rs")
        for i, raw in enumerate(text.splitlines(), 1):
            line = strip_comment(raw, py)
            if not line:
                continue
            hits = {m.group(1) for m in INVOCATION.finditer(line)}
            if f.suffix in (".rs", ".toml"):
                hits |= {m.group(1) for m in QUOTED_PATH.finditer(line)}
            for h in hits:
                if h in known and not is_own_harness(f, h):
                    inv[h].append((str(f.relative_to(ROOT)), i))
    return inv


def analyse(path: Path) -> dict:
    """Static reading of one script: verdicts, exit-zero refusals, floor guard."""
    try:
        lines = path.read_text(errors="replace").splitlines()
    except OSError:
        return {}
    py = path.suffix == ".py"
    if not py:
        # HEREDOC BODIES ARE NOT THIS FILE'S CODE. `ci/handoff.sh` ships a
        # `set -euo pipefail` inside `<<REMOTE_EOF` and it is not the enclosing
        # shell's option; `ci/check-vacuous-green-falsify.sh` carries the
        # python it INJECTS inside `<<'PY'`, and scanning that reported the
        # falsifier as holding the very instance it plants. Blanked rather than
        # deleted so every line number below still refers to the real file.
        lines = list(lines)
        hd: str | None = None
        for i, line in enumerate(lines):
            if hd is not None:
                if line.strip() == hd:
                    hd = None
                lines[i] = ""
                continue
            m = HEREDOC.search(line)
            if m and not line.strip().startswith("#"):
                hd = m.group(1)
    verdict_lines: list[int] = []
    refusal_sites: list[dict] = []
    for i, raw in enumerate(lines):
        if EXIT_NONZERO.match(raw):
            for j in range(max(0, i - WINDOW), i + 1):
                if EMITS.search(lines[j]) and VERDICT_WORD.search(lines[j]):
                    verdict_lines.append(i + 1)
                    break
        if EXIT_ZERO.match(raw):
            window = lines[max(0, i - WINDOW): i + 1]
            # A window holding a PASS verdict is an announcement, not a refusal.
            if any(GREEN_WORD.search(w) for w in window):
                continue
            for j in range(max(0, i - WINDOW), i + 1):
                if not (EMITS.search(lines[j]) and REFUSAL_WORD.search(lines[j])):
                    continue
                # The message must belong to THIS exit. `ci/batch.sh` prints
                # REFUSED and then `return 2`, and the function's own trailing
                # `return 0` sits four lines below it: crediting that message
                # to the success return reads a refusal as a vacuous green
                # when the refusal is the one thing that works.
                if any(EXIT_NONZERO.match(k) for k in lines[j + 1: i]):
                    continue
                # ...and a non-zero exit on the message's OWN line, which is
                # how `|| { echo "..."; return 1; }` is spelled. That one-liner
                # is a refusal that works, and the function's trailing
                # `return 0` six lines down is not its exit.
                if SAME_LINE_NONZERO.search(lines[j]):
                    continue
                msg = lines[j].strip()
                refusal_sites.append(
                    {"exit_line": i + 1, "message_line": j + 1, "message": msg[:160]}
                )
                break
    body = "\n".join(lines)
    return {
        "verdict_lines": verdict_lines,
        "refusal_sites": refusal_sites,
        "errexit_dead": errexit_dead_captures(lines) if not py else [],
        "has_floor_guard": bool(FLOOR_GUARD.search(body)),
        "lines": len(lines),
    }


def census() -> dict:
    paths = sorted(list(CI.glob("*.py")) + list(CI.glob("*.sh")), key=lambda p: p.name)
    rel = ["ci/" + p.name for p in paths]
    files = scan_files()
    inv = build_invocations(rel, files)

    scripts = []
    for p, r in zip(paths, rel):
        a = analyse(p)
        if not a:
            continue
        scripts.append(
            {
                "script": r,
                "declares_verdict": bool(a["verdict_lines"]),
                "verdict_lines": a["verdict_lines"],
                "call_sites": [f"{c}:{n}" for c, n in inv[r]],
                "refusal_sites": a["refusal_sites"],
                "errexit_dead": a["errexit_dead"],
                "has_floor_guard": a["has_floor_guard"],
            }
        )

    uninvoked = [
        s for s in scripts if s["declares_verdict"] and not s["call_sites"]
    ]
    green_on_refusal = [s for s in scripts if s["refusal_sites"]]
    errexit_dead = [s for s in scripts if s["errexit_dead"]]
    no_floor = [
        s["script"]
        for s in scripts
        if s["declares_verdict"] and s["call_sites"] and not s["has_floor_guard"]
    ]

    return {
        "meta": {
            "scripts_scanned": len(scripts),
            "verdict_scripts": sum(1 for s in scripts if s["declares_verdict"]),
            "uninvoked_verdict_scripts": len(uninvoked),
            "green_on_refusal_scripts": len(green_on_refusal),
            "green_on_refusal_sites": sum(len(s["refusal_sites"]) for s in green_on_refusal),
            "errexit_dead_scripts": len(errexit_dead),
            "errexit_dead_captures": sum(len(s["errexit_dead"]) for s in errexit_dead),
            "no_population_floor": len(no_floor),
        },
        "uninvoked": uninvoked,
        "green_on_refusal": green_on_refusal,
        "errexit_dead": errexit_dead,
        "no_population_floor": no_floor,
        "scripts": scripts,
    }


def esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def to_toml(c: dict) -> str:
    m = c["meta"]
    out = [
        "# ci/checks/vacuous-green.toml — DERIVED. Do not hand-edit.",
        "#",
        "# Regenerate with `python3 ci/vacuous-green-census.py --write`.",
        "# `ci/check-vacuous-green.py` re-derives this file in T0 and refuses a",
        "# tree where it has gone stale, so a hand edit here is a T0 red rather",
        "# than a hidden one.",
        "",
        # A BARE KEY AFTER A `[[table]]` HEADER BELONGS TO THAT TABLE, AND A
        # SECOND KEY OF THE SAME NAME INSIDE `[meta]` IS A PARSE ERROR. This
        # list was first emitted last, landed inside the final
        # `[[green_on_refusal]]` entry, and `ci/check-vacuous-green.py` read it
        # back as absent and printed "0 scripts show no floor" over a
        # population of 23. A census that reports zero because of where a line
        # sits is this file's own subject, in this file.
        "# Invoked verdict scripts with no visible floor on their own",
        "# population. A CANDIDATE list, not a verdict — see the module",
        "# docstring of ci/vacuous-green-census.py.",
        "no_population_floor_scripts = [",
    ]
    for np in c["no_population_floor"]:
        out.append(f'    "{esc(np)}",')
    out += [
        "]",
        "",
        "[meta]",
    ]
    for k in (
        "scripts_scanned",
        "verdict_scripts",
        "uninvoked_verdict_scripts",
        "green_on_refusal_scripts",
        "green_on_refusal_sites",
        "errexit_dead_scripts",
        "errexit_dead_captures",
        "no_population_floor",
    ):
        out.append(f"{k} = {m[k]}")
    out.append("")
    out.append("# A script that declares a verdict and that nothing in this tree runs.")
    for s in c["uninvoked"]:
        out.append("")
        out.append("[[uninvoked]]")
        out.append(f'script = "{esc(s["script"])}"')
        out.append(f'verdict_lines = {s["verdict_lines"]}')
    out.append("")
    out.append("# An exit-0 path whose own message says the check could not run.")
    for s in c["green_on_refusal"]:
        for r in s["refusal_sites"]:
            out.append("")
            out.append("[[green_on_refusal]]")
            out.append(f'script = "{esc(s["script"])}"')
            out.append(f'exit_line = {r["exit_line"]}')
            out.append(f'message_line = {r["message_line"]}')
            out.append(f'message = "{esc(r["message"])}"')
    out.append("")
    out.append("# `cmd; VAR=$?` under an active `set -e`: the handler cannot run.")
    for sc in c["errexit_dead"]:
        for d in sc["errexit_dead"]:
            out.append("")
            out.append("[[errexit_dead]]")
            out.append(f'script = "{esc(sc["script"])}"')
            out.append(f'line = {d["line"]}')
            out.append(f'form = "{esc(d["form"])}"')
            out.append(f'text = "{esc(d["text"])}"')
    out.append("")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    c = census()
    if "--json" in argv:
        print(json.dumps(c, indent=2))
        return 0
    text = to_toml(c)
    if "--write" in argv:
        CENSUS.parent.mkdir(parents=True, exist_ok=True)
        CENSUS.write_text(text)
        m = c["meta"]
        print(
            f"vacuous-green census: {m['scripts_scanned']} scripts, "
            f"{m['verdict_scripts']} declare a verdict, "
            f"{m['uninvoked_verdict_scripts']} uninvoked, "
            f"{m['green_on_refusal_sites']} green-on-refusal sites in "
            f"{m['green_on_refusal_scripts']} scripts, "
            f"{m['errexit_dead_captures']} errexit-dead handler(s) -> "
            f"{CENSUS.relative_to(ROOT)}"
        )
        return 0
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
