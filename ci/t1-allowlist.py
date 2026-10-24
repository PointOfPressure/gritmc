#!/usr/bin/env python3
"""Adjudicate a `ci/t1.sh` log against the declared known reds.

T1 is `cargo test --workspace --no-fail-fast`. It exits 101 and always has,
because two golden-vector gates are honestly red, so it has never been in a
merge gate — and **every test binary beyond `ci/t0.sh`'s `--lib --bins`
selection lives only there**. Every golden vector and every integration test
under `tests/` has been outside every merge gate for the whole life of this
project. The counts are not written here because they move as crates are
added: this script's own `T1-GATE GREEN` line prints the measured figure, and
`min_binaries` below is the floor that is actually enforced.

Wiring it in needs the two standing reds allowlisted so that a *third* failure
is what turns the stage red. That is the same shape as
`ci/checks/gameplay-baseline.toml`: a ratchet over an honestly-red suite,
never a pass list.

# WHY THIS IS A SIGNATURE AND NOT A LIST OF NAMES

A name-only allowlist lets a known failure get quietly **worse**. 106
mismatched blocks becoming 10,600 would still read as "the known red", and
the stage would report GREEN over a real regression. That is exactly the
stale-baseline defect that let a regression pass as GREEN on 2026-08-04, when
`sand-falls-when-unsupported` fell `match -> diverge` above a floor that still
said `absent`.

So the entry records the **count, the denominator, the y bands, the confusion
table and the seed/chunk of the first example**, and a change in any of them
is as red as a third failure appearing. The direction does not matter: a known
red that gets *better* is stale bookkeeping, which this repo has already
learned reads as authoritative and is worse than none.

# WHAT ELSE IT ASSERTS, AND WHY EACH ONE IS HERE

A gate that cannot fail is not a gate, and this file is easy to write in a
way that cannot. Three positive assertions, each aimed at a failure this
project has actually had:

* **`min_binaries`.** `--lib` silently stopped reaching `tools/oracle-runner`
  for the whole life of that crate — 19 tests, one of them failing, invisible
  to every green gate. A floor on the number of test binaries that reported a
  result is what notices a selector that has stopped reaching targets, or a
  suite that died after eleven binaries. It fails on a **drop**; a new crate
  raising it is a deliberate edit.
* **`min_tests` and `max_ignored`.** A binary that runs no test still reports
  `test result: ok. 0 passed; 0 failed; N ignored`, so `min_binaries` cannot
  see a test stop running. Measured 2026-08-07: a failing unit test was
  adjudicated RED, `#[ignore]` was added to that one test, and the same
  adjudicator over the new log printed GREEN with the binary count unchanged.
  A floor on tests passed and a ceiling on tests ignored are what close that;
  both are absolute, both are deliberate edits, and both are required — an
  absent declaration would default to a bound that could not fail.
* **The failure set matches exactly, in both directions.** A declared red
  that has stopped failing means this file is stale.
* **Every declared signature is compared field by field**, and an
  unparseable panic is a refusal rather than a pass.

# UPDATING IT DELIBERATELY

    ./ci/t1-allowlist.py --emit <t1.log>

prints the block the log implies. It is for pasting *after* the change has
been adjudicated by a human, so nobody hand-types a six-digit denominator —
it is not a `--fix` and it never writes the file.

Usage:
    ./ci/t1-allowlist.py <t1.log> [--declared ci/checks/t1-known-red.toml]
    ./ci/t1-allowlist.py --emit <t1.log>

Exit: 0 adjudicated green | 1 red | 2 could not read the inputs
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - 3.10 and older
    print("t1-allowlist: needs python 3.11 for tomllib", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DECLARED = ROOT / "ci" / "checks" / "t1-known-red.toml"

# `test result: ok. 12 passed; 0 failed; 3 ignored; ...` — one per test binary.
#
# `ignored` is captured because `min_binaries` is a floor on **binaries**, not
# on tests, and the difference is a hole: see `min_tests` in `main` below.
RESULT = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
)
# The same line, parsed by nobody. A cargo that changes the wording would drop
# out of RESULT silently and take the counts with it, so the two are compared:
# a strict miss is a refusal, never a zero.
RESULT_LOOSE = re.compile(r"^test result: ")
# `---- <name> stdout ----`, which cargo prints once per failing test.
STDOUT_BLOCK = re.compile(r"^---- (\S+) stdout ----\s*$")
# `thread 'name' (1263378) panicked at path:line:col:` — the thread id in
# parentheses is not always there, and an anchored `'.*' panicked at ` misses
# the form that has it. Found by the first real log rather than by the
# constructed one, which is the usual way round.
PANIC = re.compile(r"^thread .*panicked at ")

HEADER = re.compile(r"(\d+) of (\d+) blocks differ \(([\d.]+)% exact\)")
BAND = re.compile(r"^\s+y (-?\d+)\.\.(-?\d+): (\d+)\s*$")
CONFUSION = re.compile(r"^\s+(\S+) -> (\S+): (\d+)\s*$")
EXAMPLE = re.compile(r"seed=(-?\d+) chunk=\((-?\d+), *(-?\d+)\)")


def read_log(path: Path) -> list[str]:
    return path.read_text(errors="replace").splitlines()


def binaries_and_totals(lines: list[str]) -> tuple[int, int, int, int, int]:
    """(binaries that reported, passed, failed, ignored, unparsed result lines)."""
    binaries = passed = failed = ignored = loose = 0
    for line in lines:
        m = RESULT.match(line)
        if m:
            binaries += 1
            passed += int(m.group(2))
            failed += int(m.group(3))
            ignored += int(m.group(4))
        elif RESULT_LOOSE.match(line):
            loose += 1
    return binaries, passed, failed, ignored, loose


def stdout_blocks(lines: list[str]) -> dict[str, list[str]]:
    """Every `---- name stdout ----` block, by test name.

    A block runs to the next block header, the `failures:` summary, or the
    binary's `test result:` line — whichever comes first.
    """
    blocks: dict[str, list[str]] = {}
    name: str | None = None
    body: list[str] = []

    def close() -> None:
        nonlocal name, body
        if name is not None:
            blocks[name] = body
        name, body = None, []

    for line in lines:
        m = STDOUT_BLOCK.match(line)
        if m:
            close()
            name = m.group(1)
            continue
        if name is None:
            continue
        if line.startswith("test result:") or line.rstrip() == "failures:":
            close()
            continue
        body.append(line)
    close()
    return blocks


def signature(block: list[str]) -> dict | str:
    """The measured signature of one failing test, or a reason it has none.

    Anchored to the panic message rather than to the whole captured stdout:
    these tests `println!` their own diagnostics first, and some of those
    lines are shaped like the ones being parsed here.
    """
    start = next((i for i, l in enumerate(block) if PANIC.match(l)), None)
    if start is None:
        return "no panic line in the captured output"
    text = block[start:]
    joined = "\n".join(text)

    m = HEADER.search(joined)
    if not m:
        return "the panic does not carry an `N of M blocks differ` header"
    sig: dict = {
        "wrong": int(m.group(1)),
        "total": int(m.group(2)),
        "exact": m.group(3),
    }

    def section(begin: str, end: str) -> list[str]:
        try:
            i = next(k for k, l in enumerate(text) if l.startswith(begin))
        except StopIteration:
            return []
        j = next(
            (k for k, l in enumerate(text[i + 1 :], i + 1) if l.startswith(end)),
            len(text),
        )
        return text[i + 1 : j]

    sig["bands"] = [
        f"{b.group(1)}..{b.group(2)}: {b.group(3)}"
        for b in (BAND.match(l) for l in section("By y band:", "Confusion"))
        if b
    ]
    sig["confusion"] = [
        f"{c.group(1)} -> {c.group(2)}: {c.group(3)}"
        for c in (
            CONFUSION.match(l)
            for l in section("Confusion (expected -> got):", "Examples:")
        )
        if c
    ]
    ex = next(
        (EXAMPLE.search(l) for l in section("Examples:", "\x00") if EXAMPLE.search(l)),
        None,
    )
    if ex is None:
        return "the panic carries no `seed=… chunk=(…)` example"
    sig["seed"] = int(ex.group(1))
    sig["chunk"] = [int(ex.group(2)), int(ex.group(3))]
    return sig


def emit(measured: dict[str, dict], binaries: int, passed: int, ignored: int) -> None:
    print(f"min_binaries = {binaries}")
    print(f"min_tests = {passed}")
    print(f"max_ignored = {ignored}")
    print()
    for name in sorted(measured):
        s = measured[name]
        print("[[known_red]]")
        print(f'test = "{name}"')
        print(f"wrong = {s['wrong']}")
        print(f"total = {s['total']}")
        print(f'exact = "{s["exact"]}"')
        print(f"seed = {s['seed']}")
        print(f"chunk = [{s['chunk'][0]}, {s['chunk'][1]}]")
        print("bands = [")
        for b in s["bands"]:
            print(f'    "{b}",')
        print("]")
        print("confusion = [")
        for c in s["confusion"]:
            print(f'    "{c}",')
        print("]")
        print()


FIELDS = ("wrong", "total", "exact", "seed", "chunk", "bands", "confusion")


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if a != "--emit"]
    emitting = "--emit" in argv
    declared_path = DEFAULT_DECLARED
    if "--declared" in args:
        i = args.index("--declared")
        declared_path = Path(args[i + 1])
        del args[i : i + 2]
    if len(args) != 1:
        print(__doc__, file=sys.stderr)
        return 2

    log = Path(args[0])
    if not log.is_file():
        print(f"T1-GATE RED — no log at {log}")
        return 2
    lines = read_log(log)

    binaries, passed, failed, ignored, unparsed = binaries_and_totals(lines)
    blocks = stdout_blocks(lines)
    measured: dict[str, dict] = {}
    unparseable: dict[str, str] = {}
    for name, body in blocks.items():
        s = signature(body)
        if isinstance(s, str):
            unparseable[name] = s
        else:
            measured[name] = s

    if emitting:
        emit(measured, binaries, passed, ignored)
        return 0

    if not declared_path.is_file():
        print(f"T1-GATE RED — no declared allowlist at {declared_path}")
        return 2
    declared_doc = tomllib.loads(declared_path.read_text())
    declared = {e["test"]: e for e in declared_doc.get("known_red", [])}
    min_binaries = int(declared_doc.get("min_binaries", 0))

    problems: list[str] = []

    # 1. The suite has to have run. An exit code proves the process returned;
    #    only output the stage produced proves the stage ran.
    if binaries < min_binaries:
        problems.append(
            f"only {binaries} test binaries reported a result, floor is "
            f"{min_binaries} — the suite did not run, or a selector stopped "
            f"reaching targets"
        )

    # 1b. **`min_binaries` is a floor on binaries, and a binary that runs no
    #     test still reports one.** Measured 2026-08-07 on a replica of this
    #     workspace: a failing unit test was adjudicated `T1-GATE RED — a new
    #     red`; `#[ignore]` was added to that one test and nothing else changed;
    #     the same adjudicator on the new log printed `T1-GATE GREEN — 3 test
    #     binaries`. Same binary count, same declared file, one fewer test run,
    #     and the only visible difference was `2 passed` becoming `1 passed` —
    #     a number this file printed and never compared. A gate a branch can go
    #     green through by annotating the test that catches it is a gate that
    #     cannot fail, wearing an attribute.
    #
    #     So the floor is on tests executed, and the ceiling on tests skipped.
    #     Both are absolute counts and both are deliberate edits, exactly as
    #     `min_binaries` is: `--emit` prints all three from a measured log.
    #     Neither is optional — an absent declaration would default to a bound
    #     that could not fail, which is the defect being closed.
    if "min_tests" not in declared_doc:
        problems.append(
            f"{declared_path} declares no `min_tests` — run "
            f"`./ci/t1-allowlist.py --emit <log>` and paste the three floors"
        )
    elif passed < int(declared_doc["min_tests"]):
        problems.append(
            f"only {passed} tests passed, floor is {declared_doc['min_tests']} — "
            f"tests stopped running (ignored, filtered, or deleted); a binary "
            f"that runs nothing still reports a result"
        )
    if "max_ignored" not in declared_doc:
        problems.append(
            f"{declared_path} declares no `max_ignored` — run "
            f"`./ci/t1-allowlist.py --emit <log>` and paste the three floors"
        )
    elif ignored > int(declared_doc["max_ignored"]):
        problems.append(
            f"{ignored} tests were ignored, ceiling is "
            f"{declared_doc['max_ignored']} — an `#[ignore]` was added, and an "
            f"ignored test cannot fail"
        )

    # 1c. A `test result:` line the strict parser could not read is a refusal.
    #     Silently dropping it would take its passed/failed/ignored counts with
    #     it and lower every total the three bounds above are compared against.
    if unparsed:
        problems.append(
            f"{unparsed} `test result:` line(s) did not parse — the counts "
            f"above are missing those binaries, so every floor here is soft"
        )

    # 2. The failing set, both directions.
    seen = set(blocks)
    for name in sorted(seen - set(declared)):
        problems.append(f"{name}: failing and not declared — a new red")
    for name in sorted(set(declared) - seen):
        problems.append(
            f"{name}: declared a known red and did not fail — this allowlist "
            f"is stale, and a stale caveat list reads as authoritative"
        )

    # 3. A failure whose panic cannot be read is a refusal, not a pass.
    for name, why in sorted(unparseable.items()):
        problems.append(f"{name}: {why} — cannot be matched against a signature")

    # 4. Every field of every signature.
    for name in sorted(seen & set(declared)):
        if name not in measured:
            continue
        want, got = declared[name], measured[name]
        for field in FIELDS:
            if field not in want:
                problems.append(f"{name}: the declared entry has no `{field}`")
                continue
            if want[field] != got[field]:
                problems.append(
                    f"{name}: {field} was {want[field]!r}, is now {got[field]!r}"
                )

    # The two counts have to agree, or a failure without captured output
    # slipped past the block parser and was never adjudicated.
    if failed != len(seen):
        problems.append(
            f"{failed} tests reported failed but {len(seen)} produced output — "
            f"a failure was not adjudicated"
        )

    if problems:
        print(f"T1-GATE RED — {len(problems)} problem(s)")
        for p in problems:
            print(f"  {p}")
        return 1

    print(
        f"T1-GATE GREEN — {binaries} test binaries, {passed} passed, "
        f"{ignored} ignored, {len(seen)} known red at their declared signature"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
