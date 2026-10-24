#!/usr/bin/env python3
"""Refuse a tree whose T1 positive-control ledger no longer describes it.

# ===== AGENT INTERFACE =====
#
#     ci/check-t1-controls.py           the cheap checks; wired into ci/t0.sh
#     ci/check-t1-controls.py --emit    print the counts a sweep implies
#
# Three checks, all milliseconds. Re-running the 61 controls themselves is
# `ci/t1-positive-control.py --emit` and takes minutes; this is what a merge
# gate can afford on every run.
# ===== end interface =====
"""

# # WHAT IT REFUSES, AND WHY EACH ONE
#
# 1. **The arithmetic.** `demonstrated + instrument + unforceable +
#    declared_red` must equal `total_binaries - already_had_one`, and
#    `total_binaries` must equal the number of test targets `cargo` actually
#    builds. §73.7's whole finding was a count; a count nobody re-checks is how
#    `min_binaries` sat at 95 while the tree had 106. A binary added without a
#    control moves the total and this refuses until the ledger says which class
#    it is in.
#
# 2. **The sabotage feature is off everywhere by default.** `rustmc_data::
#    sabotage` replaces the RNG under every worldgen stream with a wrong one. A
#    control that can be switched on in production is not a control, it is a
#    backdoor, and the only thing standing between the two is that no
#    `[features] default` line names it. That is exactly the kind of invariant
#    that survives by nobody happening to break it, which is not a guard.
#
# 3. **No fixture is still held.** `ci/t1-force.py` moves a fixture aside to
#    force a red and moves it back. A crashed sweep leaves it held, and a held
#    fixture is one the next gate silently does not compare against — which is
#    §73's entire subject, reintroduced by the instrument written to measure it.
#
# # WHY THE COUNTS ARE COMPARED EXACTLY AND NOT AS FLOORS
#
# `demonstrated` may be raised freely: more controls is strictly better. Every
# other field is compared for equality, because each is a *finding* rather than
# a budget. A binary drifting from `demonstrated` into `instrument` would be an
# assertion quietly becoming a `println!`, and a floor would not see it. Same
# two-way exactness `t1-known-red.toml` applies to a declared red that stops
# failing.

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "ci" / "checks" / "t1-positive-controls.toml"


def scalars_and_sections(text):
    """Top-level `key = N` and the size of each `[section]`. No TOML dep: this
    runs inside `ci/t0.sh`, which may not have one."""
    scal, sect, cur = {}, {}, None
    for ln in text.splitlines():
        s = ln.strip()
        if not s or s.startswith("#"):
            continue
        m = re.fullmatch(r"\[([a-z_]+)\]", s)
        if m:
            cur = m.group(1)
            sect.setdefault(cur, 0)
            continue
        m = re.match(r"([a-z_]+)\s*=\s*(\d+)\s*$", s)
        if m and cur is None:
            scal[m.group(1)] = int(m.group(2))
            continue
        if cur is not None and re.match(r'"[^"]+"\s*=', s):
            sect[cur] += 1
    return scal, sect


def cargo_test_targets():
    """How many test binaries cargo builds. `None` if cargo is unavailable —
    a refusal, not a pass: see the exit-2 note in main()."""
    env = dict(os.environ)
    env["PATH"] = str(Path.home() / ".cargo" / "bin") + ":" + env.get("PATH", "")
    try:
        r = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT, capture_output=True, text=True, env=env, timeout=120,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    try:
        meta = json.loads(r.stdout)
    except ValueError:
        return None
    n = 0
    for pkg in meta.get("packages", []):
        for t in pkg.get("targets", []):
            # An integration test target: `kind` is ["test"] and the source
            # lives under `tests/`. Bench and lib-test targets are neither.
            if t.get("kind") == ["test"]:
                n += 1
    return n


DEFAULT_FEATURE = re.compile(r"^\s*default\s*=\s*\[([^\]]*)\]", re.M)


def sabotage_in_a_default_set():
    bad = []
    for man in sorted(ROOT.glob("crates/*/Cargo.toml")) + sorted(ROOT.glob("tools/*/Cargo.toml")) \
            + sorted(ROOT.glob("plugins/*/Cargo.toml")):
        text = man.read_text()
        for m in DEFAULT_FEATURE.finditer(text):
            if "t1-sabotage" in m.group(1):
                bad.append(man.relative_to(ROOT))
    return bad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--emit", action="store_true")
    a = ap.parse_args()

    if not LEDGER.exists():
        print("T1-CONTROLS REFUSED — ci/checks/t1-positive-controls.toml is absent")
        print("                     that says nothing about the branch; produce it with")
        print("                     ci/t1-positive-control.py --emit")
        return 2

    scal, sect = scalars_and_sections(LEDGER.read_text())
    need = ["demonstrated", "instrument", "unforceable", "declared_red",
            "total_binaries", "already_had_one"]
    missing = [k for k in need if k not in scal]
    if missing:
        print(f"T1-CONTROLS REFUSED — the ledger declares no {', '.join(missing)}")
        return 2

    if a.emit:
        n = cargo_test_targets()
        print(f"cargo test targets: {n}")
        for k in need:
            print(f"{k} = {scal[k]}")
        for k, v in sorted(sect.items()):
            print(f"[{k}] {v} entries")
        return 0

    rc = 0

    # 1 — the arithmetic
    classed = sum(scal[k] for k in ("demonstrated", "instrument", "unforceable", "declared_red"))
    owed = scal["total_binaries"] - scal["already_had_one"]
    if classed != owed:
        print(f"T1-CONTROLS RED — {classed} binaries classified, {owed} owed a control")
        print(f"                 demonstrated {scal['demonstrated']} + instrument {scal['instrument']}"
              f" + unforceable {scal['unforceable']} + declared_red {scal['declared_red']}"
              f" != {scal['total_binaries']} - {scal['already_had_one']}")
        rc = 1

    for name, key in (("instrument", "instrument"), ("unforceable", "unforceable"),
                      ("declared_red", "declared_red")):
        if sect.get(name, 0) != scal[key]:
            print(f"T1-CONTROLS RED — [{name}] has {sect.get(name, 0)} entries, "
                  f"{key} declares {scal[key]}")
            print("                 every one must be named individually, never as a ceiling.")
            rc = 1

    built = cargo_test_targets()
    if built is None:
        print("T1-CONTROLS REFUSED — cargo metadata would not run, so the binary count")
        print("                     could not be checked. That is an environment fault and")
        print("                     says nothing about the branch.")
        return 2
    if built != scal["total_binaries"]:
        print(f"T1-CONTROLS RED — cargo builds {built} test binaries, the ledger declares "
              f"{scal['total_binaries']}")
        print("                 a binary added or removed changes what is owed a control;")
        print("                 re-run ci/t1-positive-control.py --emit and say which class.")
        rc = 1

    # 2 — the feature is off everywhere
    bad = sabotage_in_a_default_set()
    for man in bad:
        print(f"T1-CONTROLS RED — {man} has t1-sabotage in a default feature set")
        print("                 rustmc_data::sabotage replaces the RNG under every worldgen")
        print("                 stream with a wrong one. On by default it is a backdoor.")
        rc = 1

    # 3 — nothing left held
    held = subprocess.run([sys.executable, str(ROOT / "ci" / "t1-force.py"), "verify"],
                          capture_output=True, text=True)
    if held.returncode != 0:
        sys.stdout.write(held.stdout)
        print("T1-CONTROLS RED — a fixture is still held aside by ci/t1-force.py")
        rc = 1

    if rc == 0:
        print(f"T1-CONTROLS OK — {scal['demonstrated']} demonstrated, "
              f"{scal['instrument']} instruments, {scal['unforceable']} unforceable, "
              f"{scal['declared_red']} declared red, over {built} binaries")
    return rc


if __name__ == "__main__":
    sys.exit(main())
