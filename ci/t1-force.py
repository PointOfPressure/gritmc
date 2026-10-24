#!/usr/bin/env python3
"""Force a T1 test binary to fail, then put the tree back.

# ===== AGENT INTERFACE =====
#
#     ci/t1-force.py apply   --mode absent  --target <path>
#     ci/t1-force.py apply   --mode perturb --target <path>
#     ci/t1-force.py restore --target <path>
#     ci/t1-force.py verify                       nothing is left held
#
# `apply` is idempotent and refuses to run twice over the same target, because
# a second `absent` would move the HELD copy aside and lose the original.
# ===== end interface =====
"""

# # THE TWO MODES ARE NOT THE SAME EVIDENCE, AND THAT IS THE POINT
#
# `docs/PARITY-ROADMAP.md` §73.4 graded 70 tests by removing the fixture each
# one reads and showing it go red. §73.7 then says, of its own method, that
# "the forcing this branch did is by fixture removal, which is one axis and not
# the only one".
#
# It is worth being exact about what that axis proves. A test that goes red
# when its golden file is deleted has demonstrated that it OPENS the file. It
# has not demonstrated that it compares anything: a test that loads a vector
# and asserts nothing about it fails identically when the vector is gone. So
# `absent` is a real control and a weak one, and grading everything with it
# would report the strength of a read as though it were the strength of a
# comparison.
#
#   absent    the binary reads this file          weakest
#   perturb   the binary's comparison is live     stronger
#   sabotage  the binary detects a wrong impl     strongest (ci/t1-sabotage.sh)
#
# `perturb` changes ONE numeric leaf in the fixture and requires the binary to
# notice. Where a golden vector is a list of numbers this is nearly free, and
# it is the difference between "the file is open" and "the assertion has
# teeth". `ci/checks/t1-positive-controls.toml` records which grade each
# binary reached, and the count of each is the honest headline.
#
# # WHY THE HELD COPY IS A SIBLING AND NOT /tmp
#
# `probe_ruin_height` has read `/tmp/ruin-w/region` since it was written and
# reported `ok. 1 passed` on every run, because `/tmp` does not survive a
# reboot and the directory has not existed for months (§73.10). Holding a
# fixture in `/tmp` while a run is in flight would put this instrument's own
# safety in the one place this repo has already been burned by. The held copy
# sits beside the original under a name `.gitignore` does not cover, so a
# crashed run leaves a visible file rather than a silently absent fixture, and
# `--verify` refuses a tree with one still held.

import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUFFIX = ".t1-held"


def held_path(target: Path) -> Path:
    return target.with_name(target.name + SUFFIX)


def apply_absent(target: Path) -> str:
    held = held_path(target)
    if held.exists():
        raise SystemExit(f"t1-force: {held} already exists — a previous run did not restore")
    if not target.exists():
        raise SystemExit(f"t1-force: {target} does not exist, so removing it forces nothing")
    os.rename(target, held)
    return f"held {target.relative_to(ROOT)}"


def _numbers(node, path=(), out=None, limit=4096):
    """Every numeric leaf, depth-first, in a stable order.

    Depth-first and ordered so that `--nth K` names the same leaf on every run
    and a recorded red signature stays reproducible.

    **Why more than one.** The first draft perturbed leaf 0 only, and five
    binaries that demonstrably open their fixture came back `unforced`:
    `random_tick_pos`, `probe_population_seed`, `margin_probe`,
    `probe_disk_origin`, `probe_sulfur_gradient`. A golden file's first number
    is often a header — a count, a seed, a format version — that no assertion
    reads, so failing to force on it says nothing about the comparison. Walking
    several leaves separates "this binary's assertions are live" from "leaf 0
    was the wrong leaf", and only the first of those is a control.
    """
    if out is None:
        out = []
    if len(out) >= limit:
        return out
    if isinstance(node, bool):
        return out
    if isinstance(node, (int, float)):
        out.append(path)
        return out
    if isinstance(node, list):
        for i, v in enumerate(node):
            _numbers(v, path + (i,), out, limit)
    elif isinstance(node, dict):
        for k in node:
            _numbers(node[k], path + (k,), out, limit)
    return out


def _first_number(node, path=()):
    got = _numbers(node, path, limit=1)
    return got[0] if got else None


def _get(node, path):
    for p in path:
        node = node[p]
    return node


def _set(node, path, value):
    for p in path[:-1]:
        node = node[p]
    node[path[-1]] = value


def apply_perturb(target: Path, nth: int = 0) -> str:
    held = held_path(target)
    if held.exists():
        raise SystemExit(f"t1-force: {held} already exists — a previous run did not restore")
    if not target.is_file():
        raise SystemExit(f"t1-force: {target} is not a file; perturb needs one")
    shutil.copy2(target, held)
    raw = target.read_bytes()
    try:
        doc = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        # Not JSON. Flip one byte in the middle rather than declining: a
        # binary fixture's reader is exactly as worth measuring.
        if len(raw) < 16:
            os.remove(held)
            raise SystemExit(f"t1-force: {target} is {len(raw)} bytes, too small to perturb")
        i = len(raw) // 2
        b = bytearray(raw)
        b[i] ^= 0xFF
        target.write_bytes(bytes(b))
        return f"perturbed byte {i} of {target.relative_to(ROOT)}"
    leaves = _numbers(doc)
    if not leaves:
        os.remove(held)
        raise SystemExit(f"t1-force: {target} has no numeric leaf to perturb")
    if nth >= len(leaves):
        os.remove(held)
        raise SystemExit(f"t1-force: {target} has {len(leaves)} numeric leaves, no #{nth}")
    path = leaves[nth]
    old = _get(doc, path)
    # A change large enough that no tolerance in this repo absorbs it, and of
    # a kind that keeps the JSON type: `golden_gaussian` asserts a 1-ulp
    # ceiling and several noise gates compare with an epsilon, so `+1` on a
    # float would be inside some and outside others and the grade would depend
    # on which fixture was picked.
    new = (old + 1000) if isinstance(old, int) else (old + 1000.0)
    _set(doc, path, new)
    target.write_text(json.dumps(doc, indent=1), encoding="utf-8")
    where = "/".join(str(p) for p in path)
    return (f"perturbed leaf #{nth} {where} {old} -> {new} "
            f"in {target.relative_to(ROOT)} (of {len(leaves)} numeric leaves)")


def restore(target: Path) -> str:
    held = held_path(target)
    if not held.exists():
        raise SystemExit(f"t1-force: nothing held for {target}")
    if target.exists():
        if target.is_dir():
            shutil.rmtree(target)
        else:
            os.remove(target)
    os.rename(held, target)
    # **The mtime is bumped, and skipping it produced the worst failure this
    # instrument has had.**
    #
    # `apply_perturb` holds the original with `shutil.copy2`, which preserves
    # mtime. Renaming it back therefore restores a file that is OLDER than the
    # build that read it, and `cargo`'s fingerprints are mtime-based: for a
    # fixture pulled in by `include_str!` it saw nothing newer and did not
    # rebuild. `ci/t1-embedded-control.sh` restored `random_tick_pos.json`,
    # re-ran, and the test still failed against the PERTURBED number — while
    # `git status` was clean and every byte on disk was correct.
    #
    # That is worse than the red it was chasing. A stale binary compiled
    # against a sabotaged vector, in a tree that looks pristine, is precisely
    # the shape of green this whole branch exists to refuse — and it would have
    # been inherited by every later gate in the same directory.
    now = time.time()
    os.utime(target, (now, now))
    return f"restored {target.relative_to(ROOT)}"


def verify() -> int:
    """Nothing anywhere in the tree is still held. Run at the end of a sweep
    and by `ci/t0.sh`: a held fixture is a fixture the next gate silently does
    not compare against, which is §73's whole subject."""
    stuck = []
    for base in (ROOT / "crates", ROOT / "data", ROOT / "oracle-work", ROOT / "tools"):
        if not base.exists():
            continue
        for p in base.rglob("*" + SUFFIX):
            stuck.append(p)
    for p in stuck:
        print(f"T1-FORCE HELD {p.relative_to(ROOT)}")
        print(f"              restore with: ci/t1-force.py restore --target {str(p)[: -len(SUFFIX)]}")
    if stuck:
        return 1
    print("T1-FORCE OK — nothing held")
    return 0


# `include_str!` bakes the fixture into the binary at COMPILE time, so moving
# or editing the file at run time changes nothing. `random_tick_pos` reports
# `ok. 1 passed` with its golden vector deleted, and would have been graded
# UNVISITED for ever by a removal-only axis — the file is not absent, it is
# already inside the executable. `entity_move` is the other one, and it has a
# control already.
#
# The mode exists so the ledger can say `perturbed+rebuild` rather than
# `unforced`, which is a different claim about a different binary.
EMBEDDED = {
    "random_tick_pos": "crates/rustmc-server/golden/random_tick_pos.json",
    "entity_move": "crates/rustmc-server/golden/entity_move.json",
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["apply", "restore", "verify"])
    ap.add_argument("--mode", choices=["absent", "perturb"])
    ap.add_argument("--target")
    ap.add_argument("--nth", type=int, default=0,
                    help="which numeric leaf to perturb; see _numbers on why not always 0")
    a = ap.parse_args()

    if a.action == "verify":
        return verify()
    if not a.target:
        raise SystemExit("t1-force: --target is required")
    target = Path(a.target)
    if not target.is_absolute():
        target = ROOT / target
    if a.action == "restore":
        print(restore(target))
        return 0
    if a.mode == "absent":
        print(apply_absent(target))
    elif a.mode == "perturb":
        print(apply_perturb(target, a.nth))
    else:
        raise SystemExit("t1-force: --mode is required for apply")
    return 0


if __name__ == "__main__":
    sys.exit(main())
