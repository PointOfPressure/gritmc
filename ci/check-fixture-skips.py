#!/usr/bin/env python3
"""Every fixture skip must go through `rustmc_data::fixture`, and the count only falls.

# THE DEFECT, AND WHY T1'S OWN GUARD CANNOT SEE IT

`ci/t1-gate.sh` refuses a run whose tests skipped:

    SKIPS=$(grep -c "skipping: no vanilla" "$LOG")
    if [ "$SKIPS" -ne 0 ]; then ... exit 1

Its comment calls that guard "the whole point" of the stage. It is a check
that cannot fail, for two independent reasons, and either alone is fatal:

 1. **cargo captures a passing test's output.** A test that prints `skipping:
    ...` and returns *passes*, and `cargo test` never prints a passing test's
    stdout or stderr. `ci/t1.sh` runs no `--nocapture`, so the bytes the grep
    is looking for are never written to the log at all. This is stated
    outright in `crates/rustmc-data/src/fixture.rs`'s own module docs — "that
    grep structurally cannot see any of these" — and the guard was written in
    `ci/t1-gate.sh` afterwards anyway.
 2. **The pattern was copied from one example, not from the producers.**
    Measured on this tree: 119 skip-message sites in `crates/` and `tools/`,
    of which `skipping: no vanilla` matches **27**. The largest single
    wording, `skipping: no datagen extract` (36 sites), does not match. Even
    with `--nocapture` the guard would be blind to 92 of 119.

That second half is the same shape as the `  REGRESSED` fixture that matched
its emitter in 0 of 45 lines: a protective pattern written from the pattern
rather than from the producer, so it can only ever agree with itself.

# WHAT THIS CHECKS INSTEAD

`crates/rustmc-data/src/fixture.rs` already fixed this properly, by inverting
the default: `fixture::available` **panics** when a fixture is missing unless
`RUSTMC_FIXTURES=optional` is set. A panic needs no grep and no capture, so
no guard can be too narrow. It was written on 2026-08-05 and adopted at
**2 call sites**, while 118 hand-written raw skips were left in place.

So this refuses raw skips by construction: any `eprintln!("skipping...)`/
`println!("skipping...)` outside `fixture.rs` is a site that has not adopted
it. The count is a **ratchet**, declared in `ci/checks/fixture-skips.toml`,
in the shape this repo already uses for `t1-known-red.toml`:

  * measured **above** the declared total -> RED, a new raw skip was added;
  * measured **below** -> RED, the declaration is stale and must be lowered
    in the commit that did the converting. A ratchet nobody lowers stops
    being evidence, which is the failure mode `t1-known-red.toml`'s header
    names.

Per-file counts are declared too, so converting one crate cannot be masked by
another crate adding sites in the same run.

# ITS OWN RED

`--falsify` is the evidence that this can fail: it writes a raw skip into a
scratch copy of the tree and requires a refusal, then removes a declared site
and requires a refusal for staleness. Run it before trusting a green.

Usage:
    ci/check-fixture-skips.py [--declared ci/checks/fixture-skips.toml]
    ci/check-fixture-skips.py --emit      print the block the tree implies
    ci/check-fixture-skips.py --falsify   prove this check can go red

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
except ModuleNotFoundError:  # pragma: no cover - 3.10 and older
    print("check-fixture-skips: needs python 3.11 for tomllib", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DECLARED = ROOT / "ci" / "checks" / "fixture-skips.toml"

# The one legitimate emitter: the helper's own `Verdict::SkipAllowed` arm.
# Named by path rather than by wording, because a wording is exactly what this
# check exists to stop anyone depending on.
HELPER = "crates/rustmc-data/src/fixture.rs"

# `eprintln!("skipping...` / `println!("skipping...`, which is the shape every
# hand-written site uses. Anchored on the macro rather than on the word alone
# so a doc comment quoting the string is not counted as a site.
SITE = re.compile(r'\b(?:eprintln!|println!)\s*\(\s*"(skipping[^"]*)"', re.S)


def _blank_comments(src: str) -> str:
    """`src` with `//` and `/* */` comments replaced by spaces, newlines kept.

    Line numbers must survive, so every removed character becomes a space
    rather than being deleted.
    """
    out, i, n = [], 0, len(src)
    while i < n:
        two = src[i : i + 2]
        if two == "//":
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif two == "/*":
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append("".join(" " if c != "\n" else "\n" for c in src[i:j]))
            i = j
        elif src[i] == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(src[i:j])
            i = j
        else:
            out.append(src[i])
            i += 1
    return "".join(out)


def sources(root: Path) -> list[Path]:
    out = []
    for base in ("crates", "tools"):
        d = root / base
        if not d.is_dir():
            continue
        for p in sorted(d.rglob("*.rs")):
            if "target" in p.parts:
                continue
            out.append(p)
    return out


def census(root: Path) -> dict[str, list[tuple[int, str]]]:
    """{relative path: [(line, message), ...]} for every raw skip site."""
    found: dict[str, list[tuple[int, str]]] = {}
    for p in sources(root):
        rel = p.relative_to(root).as_posix()
        if rel == HELPER:
            continue
        # Scanned over the whole text, not line by line: `eprintln!(` and its
        # string are on separate lines at five sites, and a per-line scan
        # silently undercounts exactly the sites somebody reformatted.
        #
        # Comments are blanked first, and that is not tidiness. A doc comment
        # quoting the shape it replaced — which is the natural way to record
        # what a conversion commit did — otherwise counts as a live site, so
        # the ratchet would refuse to fall for a file that had been fully
        # converted and had said so. Found on `state_spec.rs`, in this lane,
        # by the ratchet holding at a number the tree had already beaten.
        text = _blank_comments(p.read_text(errors="replace"))
        hits = [
            (text[: m.start()].count("\n") + 1, m.group(1)) for m in SITE.finditer(text)
        ]
        if hits:
            found[rel] = hits
    return found


def emit(found: dict[str, list[tuple[int, str]]]) -> str:
    total = sum(len(v) for v in found.values())
    lines = [
        "# Generated by `ci/check-fixture-skips.py --emit`. Lower it in the commit",
        "# that converts the sites; never raise it to make a run green.",
        f"total = {total}",
        "",
        "[files]",
    ]
    for rel in sorted(found):
        lines.append(f'"{rel}" = {len(found[rel])}')
    return "\n".join(lines) + "\n"


def adjudicate(root: Path, declared_path: Path) -> int:
    try:
        declared = tomllib.loads(declared_path.read_text())
    except OSError as exc:
        print(f"check-fixture-skips: cannot read {declared_path}: {exc}", file=sys.stderr)
        return 2
    except tomllib.TOMLDecodeError as exc:
        print(f"check-fixture-skips: {declared_path} does not parse: {exc}", file=sys.stderr)
        return 2

    if "total" not in declared:
        print("check-fixture-skips: no `total` declared — an absent bound is a bound", file=sys.stderr)
        print("                     that cannot fail. Run --emit and commit the block.", file=sys.stderr)
        return 2

    found = census(root)
    measured = sum(len(v) for v in found.values())
    want = declared["total"]
    files = declared.get("files", {})

    bad = False
    if measured > want:
        print(f"FIXTURE-SKIPS RED — {measured} raw skip sites, {want} declared")
        print("    a new hand-written skip was added. A test that returns early")
        print("    PASSES, and cargo never prints a passing test's output, so no")
        print("    log grep can see it. Use rustmc_data::fixture::available.")
        bad = True
    elif measured < want:
        print(f"FIXTURE-SKIPS RED — {measured} raw skip sites, {want} declared (STALE)")
        print("    sites were converted without lowering the ratchet. A bound nobody")
        print("    lowers stops being evidence — see ci/checks/t1-known-red.toml.")
        print("    Run `ci/check-fixture-skips.py --emit` and commit the block.")
        bad = True

    for rel in sorted(set(files) | set(found)):
        got, expect = len(found.get(rel, [])), files.get(rel, 0)
        if got != expect:
            print(f"    {rel}: {got} sites, {expect} declared")
            for n, msg in found.get(rel, [])[:3]:
                print(f"        {rel}:{n}  {msg!r}")
            bad = True

    if bad:
        return 1
    print(f"FIXTURE-SKIPS GREEN — {measured} raw skip sites, exactly as declared")
    print(f"                      across {len(found)} files; every one is a check")
    print("                      that passes when its fixture is absent.")
    return 0


def falsify(root: Path, declared_path: Path) -> int:
    """Show this check going red before anyone is asked to trust a green."""
    rc = 0
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp) / "tree"
        subprocess.run(
            ["cp", "-a", str(root / "crates"), str(work / "crates")],
            check=True,
            stdout=subprocess.DEVNULL,
        ) if False else None
        work.mkdir()
        for base in ("crates", "tools", "ci"):
            subprocess.run(["cp", "-a", str(root / base), str(work / base)], check=True)

        print("== falsify 1: an added raw skip must be refused")
        victim = work / "crates" / "rustmc-world" / "src" / "lib.rs"
        if not victim.exists():
            victim = next((work / "crates").rglob("src/lib.rs"))
        victim.write_text(
            victim.read_text()
            + '\n#[test]\nfn added_by_falsify() {\n    eprintln!("skipping: no invented extract");\n}\n'
        )
        got = adjudicate(work, declared_path)
        print(f"   -> exit {got}")
        if got != 1:
            print("   FALSIFY FAILED: adding a raw skip did not turn this red")
            rc = 1

        print("\n== falsify 2: a converted site must be refused until the ratchet drops")
        for base in ("crates", "tools"):
            subprocess.run(["rm", "-rf", str(work / base)], check=True)
            subprocess.run(["cp", "-a", str(root / base), str(work / base)], check=True)
        found = census(work)
        rel = next(iter(sorted(found)))
        p = work / rel
        n, _msg = found[rel][0]
        lines = p.read_text(errors="replace").splitlines(keepends=True)
        lines[n - 1] = re.sub(r'"skipping', '"converted-away', lines[n - 1])
        p.write_text("".join(lines))
        got = adjudicate(work, declared_path)
        print(f"   -> exit {got}")
        if got != 1:
            print("   FALSIFY FAILED: a stale declaration did not turn this red")
            rc = 1

        print("\n== control: the unmodified tree must be green")
        for base in ("crates", "tools"):
            subprocess.run(["rm", "-rf", str(work / base)], check=True)
            subprocess.run(["cp", "-a", str(root / base), str(work / base)], check=True)
        got = adjudicate(work, declared_path)
        print(f"   -> exit {got}")
        if got != 0:
            print("   FALSIFY FAILED: the control is not green, so neither red above")
            print("                   is evidence about the mutation")
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
