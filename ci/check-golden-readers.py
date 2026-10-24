#!/usr/bin/env python3
"""Every committed golden fixture is read by something, and the orphans are named.

# THE DEFECT

A file under a crate's `golden/` directory reads as a gate. It is the shape
this project uses for its strongest evidence — a vector dumped from vanilla
and compared byte for byte. `crates/rustmc-behavior/golden/` and
`crates/rustmc-data/golden/` hold six such files, committed 2026-07-21 as
"P0-5: golden vectors from the JVM oracle (19 files, verified three ways)",
and **no Rust source in this workspace opens any of them.** Their own
READMEs describe a regeneration recipe that runs against `~/minecom`, the
predecessor project, and the functions they pin — `Difficulty.effective`,
`Enchants.tableCosts`, `Beacons.validateEffects`, `Conduits.effectRange`,
`map_colors`, `sulfur_archetypes` — have no port in this workspace to gate.

They cost nothing to keep and they are not wrong. What they do is make a
directory of live gates and a directory of dead ones look identical from the
outside, which is the same failure mode as a stale caveat list: it reads as
authoritative and is worse than none.

# WHAT THIS CHECKS

Every `*.json` under any `golden/` directory must be named by at least one
`.rs` file in the workspace, either literally or through a `format!` template
whose fixed part matches — `xoroshiro128pp_seed_{suffix}.json` covers the five
`xoroshiro128pp_seed_*.json` files without any of them appearing verbatim.

Files with no reader are declared in `ci/checks/golden-readers.toml` with a
reason each. That list is exact in both directions: an undeclared orphan is
red because a fixture stopped being read, and a declared orphan that has
gained a reader is red because the declaration is now stale.

# ITS OWN RED

`--falsify` removes a reader's reference and requires a refusal, then adds a
reader to a declared orphan and requires a refusal, then checks the
unmodified tree is green.

Usage:
    ci/check-golden-readers.py [--declared ci/checks/golden-readers.toml]
    ci/check-golden-readers.py --emit
    ci/check-golden-readers.py --falsify

Exit: 0 green | 1 red | 2 could not read the inputs
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    print("check-golden-readers: needs python 3.11 for tomllib", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DECLARED = ROOT / "ci" / "checks" / "golden-readers.toml"


def goldens(root: Path) -> list[Path]:
    out: list[Path] = []
    for base in ("crates", "tools"):
        d = root / base
        if not d.is_dir():
            continue
        for p in sorted(d.rglob("golden/*.json")):
            if "target" in p.parts:
                continue
            out.append(p)
    return out


def rust_text(root: Path) -> str:
    parts = []
    for base in ("crates", "tools"):
        d = root / base
        if not d.is_dir():
            continue
        for p in sorted(d.rglob("*.rs")):
            if "target" in p.parts:
                continue
            parts.append(p.read_text(errors="replace"))
    return "\n".join(parts)


def read_by_something(name: str, text: str) -> bool:
    """Literally, or through a `format!` template with a `{...}` hole."""
    if name in text:
        return True
    stem = name[: -len(".json")]
    # `xoroshiro128pp_seed_0` -> a template `xoroshiro128pp_seed_{...}.json`
    # is a reader. Try every split point rather than guessing where the hole
    # is: the varying part is a suffix at some sites and an infix at others.
    for i in range(len(stem), 0, -1):
        head = stem[:i]
        if len(head) < 6:
            break
        if re.search(re.escape(head) + r"\{[A-Za-z_][A-Za-z0-9_]*\}[^\"]*\.json", text):
            return True
    return False


def census(root: Path) -> list[str]:
    text = rust_text(root)
    return [
        p.relative_to(root).as_posix()
        for p in goldens(root)
        if not read_by_something(p.name, text)
    ]


def emit(orphans: list[str]) -> str:
    lines = [
        "# Golden fixtures no Rust source opens. Each needs a reason; remove the",
        "# entry in the commit that gives it a reader, or deletes it.",
        "",
        "[orphans]",
    ]
    for rel in orphans:
        lines.append(f'"{rel}" = "TODO: why this is committed with no reader"')
    return "\n".join(lines) + "\n"


def adjudicate(root: Path, declared_path: Path) -> int:
    try:
        declared = tomllib.loads(declared_path.read_text()).get("orphans", {})
    except OSError as exc:
        print(f"check-golden-readers: cannot read {declared_path}: {exc}", file=sys.stderr)
        return 2
    except tomllib.TOMLDecodeError as exc:
        print(f"check-golden-readers: {declared_path} does not parse: {exc}", file=sys.stderr)
        return 2

    found = set(census(root))
    want = set(declared)
    bad = False

    for rel in sorted(found - want):
        print(f"GOLDEN-READERS RED — {rel} is not opened by any Rust source")
        print("    a golden nobody reads is a gate that is not running. Give it a")
        print("    reader, delete it, or declare it in ci/checks/golden-readers.toml")
        print("    with the reason it is kept.")
        bad = True
    for rel in sorted(want - found):
        print(f"GOLDEN-READERS RED — {rel} is declared an orphan but now HAS a reader")
        print("    the declaration is stale. Remove the entry.")
        bad = True

    if bad:
        return 1
    print(f"GOLDEN-READERS GREEN — {len(goldens(root))} golden fixtures, {len(found)} orphans,")
    print("                       each declared with a reason.")
    return 0


def falsify(root: Path, declared_path: Path) -> int:
    rc = 0
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp) / "tree"
        work.mkdir()

        def reset() -> None:
            for base in ("crates", "tools"):
                subprocess.run(["rm", "-rf", str(work / base)], check=False)
                subprocess.run(["cp", "-a", str(root / base), str(work / base)], check=True)

        reset()
        print("== falsify 1: a golden that loses its reader must be refused")
        # `gaussian_vectors.json` is named by exactly one test.
        victim = work / "crates/rustmc-worldgen/tests/golden_gaussian.rs"
        victim.write_text(victim.read_text().replace("gaussian_vectors.json", "not_a_file.json"))
        got = adjudicate(work, declared_path)
        print(f"   -> exit {got}")
        if got != 1:
            print("   FALSIFY FAILED: removing a reader did not turn this red")
            rc = 1

        reset()
        print("\n== falsify 2: a declared orphan that gains a reader must be refused")
        declared = tomllib.loads(declared_path.read_text()).get("orphans", {})
        if declared:
            name = Path(next(iter(sorted(declared)))).name
            p = work / "crates/rustmc-worldgen/tests/golden_gaussian.rs"
            p.write_text(p.read_text() + f'\n// falsify reader: {name}\n')
            got = adjudicate(work, declared_path)
            print(f"   -> exit {got}")
            if got != 1:
                print("   FALSIFY FAILED: a stale orphan declaration did not turn this red")
                rc = 1
        else:
            print("   (no orphans declared, so there is nothing to make stale)")

        reset()
        print("\n== control: the unmodified tree must be green")
        got = adjudicate(work, declared_path)
        print(f"   -> exit {got}")
        if got != 0:
            print("   FALSIFY FAILED: the control is not green")
            rc = 1
    print("\nFALSIFY " + ("OK" if rc == 0 else "FAILED"))
    return rc


def main(argv: list[str]) -> int:
    declared = DEFAULT_DECLARED
    if "--declared" in argv:
        declared = Path(argv[argv.index("--declared") + 1])
    if "--emit" in argv:
        sys.stdout.write(emit(census(ROOT)))
        return 0
    if "--falsify" in argv:
        return falsify(ROOT, declared)
    return adjudicate(ROOT, declared)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
