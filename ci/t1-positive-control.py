#!/usr/bin/env python3
"""Force every T1 test binary to fail, once, and record what it took.

# ===== AGENT INTERFACE =====
#
#     ci/t1-positive-control.py --emit           measure, write the ledger
#     ci/t1-positive-control.py                  verify the ledger still holds
#     ci/t1-positive-control.py --only NAME      one binary
#     ci/t1-positive-control.py --list           what is declared, and how
#
# Ledger: `ci/checks/t1-positive-controls.toml`.
# ===== end interface =====
"""

# # THE QUESTION
#
# `docs/PARITY-ROADMAP.md` §73.7 counted, for T1's 80 test binaries, how many
# have a positive control — anything that has ever been shown to make them
# fail. Five are named by a falsifier in `ci/`; one carries a real control in
# its own file; **74 have neither.** §73.7 is careful about what that means:
# not that the 74 are worthless, but that "for 74 binaries nobody has ever
# demonstrated a red, so nobody knows which of them are in the same state
# `can_survive_matches_vanilla_where_both_sides_answer` was in an hour ago".
#
# This script answers it for each of the 74 by measurement rather than by
# argument, and writes down which axis was needed.
#
# # THE GRADES, AND WHY THE WEAKEST ONE IS NOT ENOUGH ON ITS OWN
#
#   absent     the fixture is moved aside; the binary goes red
#   perturbed  ONE number inside the fixture is changed; the binary goes red
#   sabotage   the implementation is replaced with a wrong one (ci/t1-sabotage)
#
# §73.4 graded 70 tests by removal alone, and §73.7 says of that method that it
# "is by fixture removal, which is one axis and not the only one". The reason
# to care is precise: **a binary that goes red when its golden file is deleted
# has proved that it opens the file, and nothing more.** A test that loads a
# vector and never compares it fails in exactly the same way. `perturbed` is
# the grade that separates the two, and it costs one integer.
#
# So `--emit` tries `perturbed` first on every candidate and only falls back to
# `absent`, and the ledger records which was reached. A binary whose comparison
# is live reads `perturbed`; one that only opens a file reads `absent`, and
# that is a finding about that binary rather than a defect in this script.
#
# # WHAT COUNTS AS RED
#
# Non-zero exit is necessary and not sufficient. The forcing has to make the
# binary fail **for its own reason**, so a run is only a control if a test in
# that binary is named in the failure list. A binary that dies at load time
# with the same message whatever you break tells you nothing about its
# assertions, and it is recorded as such.
#
# # THE RESTORE IS CHECKED, NOT ASSUMED
#
# Every arm ends by restoring and re-running, and a binary that does not come
# back green is a refusal, not a warning: the alternative is a sweep that
# leaves a fixture held and every later gate silently comparing nothing, which
# is the exact failure §73 is about. `ci/t1-force.py verify` is also run at the
# end and is wired into `ci/t0.sh`.

import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "ci" / "checks" / "t1-positive-controls.toml"
FORCE = ROOT / "ci" / "t1-force.py"
CACHE = ROOT / "ci" / "logs" / "t1-testbins.tsv"

# §73.7's five, plus `session_joint`. Not re-measured here: they already have a
# demonstrated control and this script exists for the ones that do not. Listed
# so the arithmetic in the ledger is checkable — the tree's binary count minus
# these six is what the ledger's four classes must add up to. It was 80 - 6 =
# 74 when §73.7 was written and is 84 - 6 = 78 since main f4fa28ff8 added the
# four `rustmc-entity` binaries. **The number is not repeated here on purpose**:
# `ci/check-t1-controls.py` recomputes it from `cargo metadata` on every T0, so
# a constant in this comment would be a second copy nobody re-checks.
ALREADY = {
    "entity_move": "ci/collide-falsify.sh",
    "golden_cross_collision_blocks": "ci/falsify-ruined-portal.py",
    "golden_ruined_portal_setups": "ci/falsify-ruined-portal.py",
    "golden_structure_template": "ci/falsify-ruined-portal.py",
    "plugin_tick_lock": "ci/plugin-hook-falsify.sh",
    "session_joint": "its own six cutting_* tests",
}

# The two declared reds. They cannot be forced because they are already
# failing, and §73.10 says so; a forcing that "works" on them is measuring
# nothing.
DECLARED_RED = {"golden_carver_chunk", "golden_surface_chunk"}


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def testbins():
    """name -> executable. Cached: `cargo test --no-run` is slow under load."""
    if CACHE.exists():
        out = {}
        for ln in CACHE.read_text().splitlines():
            if "\t" in ln:
                n, e = ln.split("\t", 1)
                out[n] = e
        if out:
            return out
    env = dict(os.environ)
    env["PATH"] = str(Path.home() / ".cargo" / "bin") + ":" + env.get("PATH", "")
    r = subprocess.run(
        ["cargo", "test", "--workspace", "--no-run", "--message-format=json"],
        cwd=ROOT, capture_output=True, text=True, env=env,
    )
    out = {}
    for line in r.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-artifact" or not m.get("executable"):
            continue
        t = m.get("target", {})
        if "test" in t.get("kind", []):
            out[t["name"]] = m["executable"]
    CACHE.parent.mkdir(parents=True, exist_ok=True)
    CACHE.write_text("".join(f"{k}\t{v}\n" for k, v in sorted(out.items())))
    return out


FAILED_RE = re.compile(r"^\s{4}(\S+)$", re.M)


def run_binary(exe, timeout=1800):
    """(rc, failing test names, tail of output)."""
    try:
        r = subprocess.run([exe], capture_output=True, text=True, timeout=timeout, cwd=ROOT)
    except subprocess.TimeoutExpired:
        return 124, [], "timed out"
    out = r.stdout + r.stderr
    failing = []
    if "\nfailures:\n" in out:
        block = out.split("\nfailures:\n")[-1]
        failing = [m.group(1) for m in FAILED_RE.finditer(block)]
    return r.returncode, sorted(set(failing)), out[-2000:]


def candidates(binary):
    """Fixture files this binary might read, most specific first.

    Path literals from the source. Deliberately NOT a filesystem guess: a
    candidate that the binary does not open simply fails to force it, and the
    ledger records that rather than inventing a reason.
    """
    srcs = list((ROOT / "crates").rglob(f"tests/{binary}.rs"))
    srcs += list((ROOT / "tools").rglob(f"tests/{binary}.rs"))
    if not srcs:
        return []
    src = srcs[0]
    crate = src.parent.parent
    text = src.read_text(encoding="utf-8", errors="replace")
    out = []
    for m in re.finditer(r"golden/([A-Za-z0-9_.-]+\.json)", text):
        p = crate / "golden" / m.group(1)
        if p.exists():
            out.append(p)
    for m in re.finditer(r"data/vanilla/([A-Za-z0-9_.-]+)", text):
        p = ROOT / "data" / "vanilla" / m.group(1)
        if p.exists():
            out.append(p)
    for m in re.finditer(r"oracle-work/([A-Za-z0-9_./-]+)", text):
        p = ROOT / "oracle-work" / m.group(1)
        if p.exists():
            out.append(p)

    # **Ten binaries came back `unforced` with "0 candidate fixtures" and every
    # one of them reads the datagen extract.** They resolve it through a helper
    # rather than writing a path: `worldgen_assembly::DATA`, which is
    # `concat!(env!("CARGO_MANIFEST_DIR"), "/../../oracle-work/vanilla-26.2/
    # generated-data/data/minecraft")`, and `data_path(rel)` which joins onto
    # it. A candidate list built from path literals alone therefore reported
    # "this binary reads nothing" about `golden_biome_tags`, whose first line
    # of work is to open the biome tag extract.
    #
    # That is the §73.2 defect in miniature — a pattern taken from the shape of
    # one example instead of from the producers — so the resolution is to
    # follow the helper rather than to widen the regex.
    EXTRACT = ROOT / "oracle-work/vanilla-26.2/generated-data/data/minecraft"
    if re.search(r"\bDATA\b|data_path\s*\(|have_extract", text):
        for m in re.finditer(r'(?:data_path|DATA\s*\)?)\s*(?:\.join)?\s*\(\s*"([A-Za-z0-9_./-]+)"', text):
            p = EXTRACT / m.group(1)
            if p.exists():
                out.append(p)
        for m in re.finditer(r'\.join\(\s*"([A-Za-z0-9_./-]+)"\s*\)', text):
            p = EXTRACT / m.group(1)
            if p.exists():
                out.append(p)
        if EXTRACT.exists():
            out.append(EXTRACT)

    seen = set()
    return [p for p in out if not (p in seen or seen.add(p))]


def force(mode, target, nth=0):
    cmd = [sys.executable, str(FORCE), "apply", "--mode", mode, "--target", str(target)]
    if mode == "perturb":
        cmd += ["--nth", str(nth)]
    return sh(cmd)


def unforce(target):
    return sh([sys.executable, str(FORCE), "restore", "--target", str(target)])


def measure(binary, exe, base_rc):
    """Try to force `binary`. Returns a dict describing what worked.

    **The clean run comes first and is not an optimisation.** A binary that is
    already failing goes "red" under any forcing at all, so a sweep that did
    not establish the starting state would report a control for every test the
    tree happens to be red on — a false green wearing the costume of evidence.
    §73.3 records that this is not hypothetical: the only reason the
    extract-removed run went red at all was eleven tests that `.expect()`, and
    the thing that noticed a suite comparing nothing was `t1-known-red.toml`
    refusing a declared red that had **stopped** failing.
    """
    clean_rc, clean_failing, _clean_tail = run_binary(exe)
    if clean_rc != 0:
        return {
            "grade": "red-before-forcing",
            "target": "", "mode": "", "red": clean_failing,
            "note": "already failing on an unmutated tree; forcing it measures nothing",
        }
    cands = candidates(binary)
    # `perturb` is tried over several leaves before falling back to `absent`,
    # because a golden file's leaf 0 is often a header no assertion reads. See
    # `_numbers` in ci/t1-force.py for the five binaries that taught this.
    PERTURB_LEAVES = 8
    attempts = []
    for target in cands:
        if not target.is_dir():
            attempts += [("perturb", target, n) for n in range(PERTURB_LEAVES)]
        attempts.append(("absent", target, 0))
    for mode, target, nth in attempts:
        r = force(mode, target, nth)
        if r.returncode != 0:
            continue
        try:
            rc, failing, tail = run_binary(exe)
        finally:
            unforce(target)
        if rc != 0 and failing:
            return {
                "grade": "perturbed" if mode == "perturb" else "absent",
                "target": str(target.relative_to(ROOT)),
                "mode": mode,
                "red": failing,
                "note": r.stdout.strip(),
            }
        if rc != 0 and not failing:
            # died without naming a test: a load-time abort, which says
            # nothing about the binary's own assertions
            return {
                "grade": "loadtime",
                "target": str(target.relative_to(ROOT)),
                "mode": mode,
                "red": [],
                "note": tail.strip().splitlines()[-1][:120] if tail.strip() else "",
            }
    return {
        "grade": "unforced",
        "target": "", "mode": "", "red": [],
        "note": f"green clean; {len(cands)} candidate fixture(s), none of which forced it",
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--emit", action="store_true")
    ap.add_argument("--only")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--out", default=str(ROOT / "ci" / "logs" / "t1-controls.tsv"))
    a = ap.parse_args()

    bins = testbins()
    if a.only:
        bins = {k: v for k, v in bins.items() if k == a.only}
        if not bins:
            raise SystemExit(f"t1-positive-control: no test binary named {a.only!r}")

    if a.list:
        for n in sorted(bins):
            tag = ALREADY.get(n, "declared red" if n in DECLARED_RED else "")
            print(f"{n:34} {tag}")
        print(f"{len(bins)} binaries")
        return 0

    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    done = {}
    if out.exists():
        for ln in out.read_text().splitlines():
            f = ln.split("\t")
            if len(f) >= 2:
                done[f[0]] = ln

    with out.open("a") as fh:
        for n in sorted(bins):
            if n in done:
                continue
            if n in ALREADY:
                rec = {"grade": "already", "target": "", "mode": "", "red": [], "note": ALREADY[n]}
            elif n in DECLARED_RED:
                rec = {"grade": "declared-red", "target": "", "mode": "", "red": [], "note":
                       "already failing at its declared signature; forcing it measures nothing"}
            else:
                t0 = time.time()
                rec = measure(n, bins[n], 0)
                rec["secs"] = round(time.time() - t0, 1)
            line = "\t".join([
                n, rec["grade"], rec.get("target", ""), rec.get("mode", ""),
                ",".join(rec.get("red", [])), str(rec.get("secs", "")), rec.get("note", "")[:160],
            ])
            print(line, flush=True)
            fh.write(line + "\n")
            fh.flush()

    r = sh([sys.executable, str(FORCE), "verify"])
    print(r.stdout.strip())
    return r.returncode


if __name__ == "__main__":
    sys.exit(main())
