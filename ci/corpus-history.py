#!/usr/bin/env python3
"""Has anything ever moved this check? Asked of every recording on the box.

# Why this is free evidence nobody was collecting

`ci/corpus-falsifiable.py` establishes what a check *would* notice by breaking
a recording. `ci/corpus-grades.py` establishes it by breaking the server. This
third question needs neither, because it has already been answered dozens of
times and nobody wrote the answer down:

**every gate directory on this box holds a recording of our server, taken
against a different build.**

Hold the reference fixed and swap the subject across all of them, and each
scenario's report signature is a time series over real code. A signature that
moved was moved by a difference an implementation actually produced — the
strongest evidence there is, and it costs one comparator invocation per
recording and no lock at all.

# What a frozen signature does and does not mean

**And `frozen` requires having been compared at all.** A check absent from
every recording on the box has `N = 0`, and until 2026-08-18 the state chain
fell through to `frozen` on it — so a check nothing had ever compared printed
the same word as one twenty builds had agreed on. Every frozen total this
script printed before that date is inflated by however many checks were younger
than its newest recording, which is worst exactly when a branch has just added
some. Those rows are now `unseen`, are excluded from the denominator, and are
listed at the bottom.

**It is a screen, not a verdict.** A check can be frozen because our server has
simply been right about it since before the oldest recording on the box. So a
frozen entry is a candidate for `ci/corpus-grades.py` to settle and never a
finding on its own; the output says `frozen`, not `unfalsifiable`.

**A moved signature is likewise not a verdict, for the opposite reason.** It
says a difference reddened the check; it does not say the difference was the
one the check is named for. `710-a-daylight-detector-inverts-when-right-
clicked` moves in 3 of the 27 recordings that carry it and is still a check
whose `power` half nothing can falsify, because what moved was the rest of the
scenario. The two instruments are complementary and neither subsumes the other.

# Counting runs, and saying N

A scenario absent from a recording is reported by the comparator as `error —
the scenario is missing from a trace`, and counting those as agreement would
inflate every N by the recordings taken before the check was written. They are
excluded and the surviving count is printed per scenario, because a
distribution difference is not a divergence and neither is a single pair.
"""

from __future__ import annotations

import collections
import glob
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Where a gate leaves its subject recording. Every gate directory on this box
# follows it; `subset/` is `ci/t2-gameplay-subset.sh`'s.
PATTERNS = (
    "/home/sayer/rustmc-*/oracle-work/gameplay/rustmc.json",
    "/home/sayer/rustmc-*/oracle-work/gameplay/subset/rustmc.json",
    "/home/sayer/conf*/oracle-work/gameplay/rustmc.json",
)

# A recording with fewer than this many scenarios is a probe, not a corpus run,
# and its absences would dominate every N.
MIN_SCENARIOS = 50

# The GATED reading. `strict` is not in it: `ci/selfcheck-aggregate.py` says
# "`strict` is reported and **not** gated" and `ci/t2-gameplay.sh` never
# mentions it. A check whose `strict` moved across builds and whose declared
# reading never did has not been shown falsifiable — nothing would have gone
# red — so it is counted separately and reported as `strict-only`.
FIELDS = ("verdict", "subject_obs", "measured_subject_obs",
          "extra_kinds", "missing_kinds", "first_difference")


def signature(outcomes):
    return {
        o["name"]: ((
            o.get("verdict"),
            o.get("subject_obs"), o.get("measured_subject_obs"),
            tuple(o.get("extra_kinds") or ()), tuple(o.get("missing_kinds") or ()),
            o.get("first_difference"),
        ), o.get("strict"))
        for o in outcomes
    }


def subjects():
    out = []
    for p in sorted({q for pat in PATTERNS for q in glob.glob(pat)}):
        try:
            n = len(json.load(open(p)).get("scenarios", []))
        except (OSError, ValueError):
            continue
        if n >= MIN_SCENARIOS:
            out.append((p, n))
    return out


# **The smallest N at which "did it ever move?" has an answer.** With N
# recordings there are N-1 opportunities for the signature to differ from the
# first; at N = 1 there are zero. That is arithmetic, not a power calculation,
# and it is deliberately the lowest threshold that means anything — this
# exemption must be as narrow as it can be, because everything above it is
# still counted against the frozen ceiling.
HISTORY_POWER_N = 2


def classify(seen, strict_seen, n=None):
    """Which of the five states a check's signature set is in.

    # `unresolved` is not `frozen` either, and it is the same defect one N on

    2026-09-05. `unseen` was split out of `frozen` on 2026-08-18 because
    "`frozen` at N=67 is sixty-seven agreements and `frozen` at N=0 is none" —
    same word, opposite amount of evidence. **`frozen` at N=1 is none as
    well.** One recording is one comparison, and one comparison has ZERO
    opportunities to differ; calling it an agreement is calling a sample of
    one a distribution, which this module's own header already refuses in so
    many words ("a distribution difference is not a divergence and neither is
    a single pair") and which the report below has been PRINTING as a caveat
    since the split, under the line "surfaced rather than acted on". This acts
    on it.

    Measured on the redstone branch, 2026-09-05: its frozen population is
    `{N=1: 9, N=24: 17, N=28: 43}` and `main`'s is `{18: 13, 19: 1, 61: 2,
    65: 45}`. There is nothing between 1 and 18 in either. So this threshold
    separates exactly the nine scenarios that branch had just written from
    every row that carries real agreement, and moves no row on main at all.

    **The rejected alternative was exempting a frozen check on its
    falsifiability tier**, and the numbers refuse it: `ci/checks/gameplay-
    falsifiable.toml` reads tier 2 for **108 of 108** checks, so "count only
    frozen checks below tier 2" counts ZERO, forever, on any branch. That is
    the ratchet deleted rather than corrected, and it is the shape
    `ci/corpus-grades.py` was written after finding — its opening paragraph
    says why tier 2 is the weaker claim: the sweep mutates a RECORDING, "every
    recording has bytes at every step, so mutating those bytes moves the
    verdict of very nearly anything". Tier 2 is the right floor for a check
    with no history YET (see `ci/corpus-grades.py`, where `unresolved` rows
    must carry it); it is not grounds to stop counting one that has been
    compared eighteen times and never moved.

    # `unseen` is not `frozen`, and conflating them inflated every frozen total
    # this script had ever printed

    `seen` is empty when every recording either lacked the scenario or reported
    `error` for it — a check younger than the newest recording on the box. The
    chain here used to test `len(seen) > 1` and then `len(strict_seen) > 1`,
    both false on an empty set, and land on `frozen`. So a check **nothing had
    ever compared** printed the same word as one every build agreed on. Same
    word, opposite amount of evidence: `frozen` at N=67 is sixty-seven
    agreements and `frozen` at N=0 is none.

    It is the unfalsifiable shape in its purest form. The screen exists to find
    checks no implementation difference has ever moved; a check no
    implementation has ever *run* satisfies that vacuously — and the error grows
    with exactly the thing that should improve coverage, because every new check
    a branch writes is another N=0 row counted as agreement.

    A named function, with `classify_unfixed` beside it, because the whole
    defect lived in the order of one conditional chain and **a changed number is
    not evidence that it was the right change**. `--selftest` runs both over the
    four cases and asserts exactly where they disagree.
    """
    if not seen:
        return "unseen"
    if len(seen) > 1:
        return "moved"
    if len(strict_seen) > 1:
        return "strict-only"
    # Movement is decided BEFORE this: a check that moved at N=1 is impossible,
    # and one whose strict lens moved is already `strict-only`. So this only
    # ever reclassifies a would-be `frozen`, which is the only state whose
    # meaning depends on how many times it was asked.
    if n is not None and n < HISTORY_POWER_N:
        return "unresolved"
    return "frozen"


def classify_unfixed(seen, strict_seen):
    """The chain as it stood until 2026-08-18. Kept only so the selftest can
    show what it did: with an empty `seen` both `len(seen) > 1` and
    `len(strict_seen) > 1` are false, so it falls through to `frozen`."""
    return ("moved" if len(seen) > 1
            else "strict-only" if len(strict_seen) > 1
            else "frozen")


def selftest():
    """The positive control. Every case carries the N it was asked at, because
    `frozen` is the one state whose meaning depends on that."""
    cases = [
        # label,                        seen,       strict,     N,  fixed,      unfixed
        ("nothing ever compared it",    set(),      set(),      0,  "unseen",     "frozen"),
        ("compared ONCE, one signature", {"a"},     {"s"},      1,  "unresolved", "frozen"),
        ("compared twice, agreeing",    {"a"},      {"s"},      2,  "frozen",     "frozen"),
        ("compared 65 times, agreeing", {"a"},      {"s"},     65,  "frozen",     "frozen"),
        ("two gated signatures",        {"a", "b"}, {"s"},      2,  "moved",      "moved"),
        # movement wins over N, and it has to: a signature cannot differ from
        # itself at N=1, so a `moved` at low N is a real difference and would
        # be thrown away by an N test applied first.
        ("moved at the floor",          {"a", "b"}, {"s"},      2,  "moved",      "moved"),
        ("gated frozen, strict moved",  {"a"}, {"s", "t"},      2,  "strict-only","strict-only"),
        ("strict moved at N=1",         {"a"}, {"s", "t"},      1,  "strict-only","strict-only"),
        # N is optional, and omitting it must not silently reclassify: the
        # writer passes it and nothing else may.
        ("no N given stays frozen",     {"a"},      {"s"},   None,  "frozen",     "frozen"),
    ]
    bad = 0
    for label, seen, strict, n, want, want_old in cases:
        got, got_old = classify(seen, strict, n), classify_unfixed(seen, strict)
        ok = got == want and got_old == want_old
        bad += not ok
        mark = "ok  " if ok else "FAIL"
        note = "  <- THE DEFECT" if want != want_old else ""
        print(f"  {mark} {label:28} N={str(n):4} fixed={got:12} unfixed={got_old:12}{note}")
    if bad:
        print(f"CORPUS-HISTORY-SELFTEST RED — {bad} case(s) wrong")
        return 1
    print(f"CORPUS-HISTORY-SELFTEST GREEN — {len(cases)} cases. The two classifiers")
    print("  disagree on exactly two, and both are the same defect at a different N:")
    print("  an empty `seen` is `unseen` (2026-08-18) and a single comparison is")
    print("  `unresolved` (2026-09-05). Neither is an agreement.")
    return 0


def main(argv):
    if len(argv) > 1 and argv[1] == "--selftest":
        return selftest()
    if len(argv) < 4:
        print("usage: corpus-history.py <checks-dir> <reference.json> <out.json> [--bin PATH]",
              file=sys.stderr)
        print("       corpus-history.py --selftest", file=sys.stderr)
        return 2
    checks, ref, out = Path(argv[1]), Path(argv[2]), Path(argv[3])
    binary = Path(argv[5]) if len(argv) > 5 and argv[4] == "--bin" else \
        ROOT / "target" / "release" / "oracle-runner"
    tmp = out.parent / "_history-compare.json"
    sigs = {}
    for path, n in subjects():
        r = subprocess.run(
            [str(binary), "compare", str(checks), "--reference", str(ref),
             "--subject", path, "--json", str(tmp)],
            capture_output=True, text=True)
        if not tmp.exists():
            print(f"  skipped {path} (rc={r.returncode})", file=sys.stderr)
            continue
        sigs[path] = signature(json.loads(tmp.read_text())["outcomes"])

    rows = []
    for name in sorted({k for s in sigs.values() for k in s}):
        seen, strict_seen, verdicts, moved = set(), set(), collections.Counter(), set()
        base = None
        for s in sigs.values():
            entry = s.get(name)
            if entry is None:
                continue
            gated, strict = entry
            # `error` is "this recording predates the check", not agreement.
            if gated[0] == "error":
                continue
            verdicts[gated[0]] += 1
            seen.add(json.dumps(gated))
            strict_seen.add(json.dumps(strict))
            if base is None:
                base = gated
            else:
                moved |= {f for f, a, b in zip(FIELDS, base, gated) if a != b}
        # See `classify` for why `unseen` is a state of its own.
        state = classify(seen, strict_seen, sum(verdicts.values()))
        rows.append({
            "name": name, "N": sum(verdicts.values()),
            "distinct": len(seen), "verdicts": dict(verdicts),
            "moved_fields": sorted(moved),
            "state": state,
        })
    out.write_text(json.dumps({"builds": len(sigs), "rows": rows}, indent=1))
    mv = [r for r in rows if r["state"] == "moved"]
    so = [r for r in rows if r["state"] == "strict-only"]
    fz = [r for r in rows if r["state"] == "frozen"]
    ur = [r for r in rows if r["state"] == "unresolved"]
    un = [r for r in rows if r["state"] == "unseen"]
    # The denominator every one of these numbers is out of. Printed as
    # `len(rows) - len(un)` rather than as `len(rows)` because a check no
    # recording carries is not a check this screen has an opinion about, and
    # quoting it inside the total is how "of 108" became a number nobody could
    # re-derive.
    compared = len(rows) - len(un)
    print(f"== {len(sigs)} recordings, {len(rows)} checks, {compared} of them compared at least once")
    print(f"== moved under the reading the gate acts on: {len(mv)}")
    print(f"== moved ONLY under the ungated strict lens: {len(so)}")
    print(f"== frozen across every build:                {len(fz)} of {compared}")
    print(f"== compared once, so never asked (N<{HISTORY_POWER_N}):     {len(ur)}  <- NOT frozen")
    print(f"== never compared by any recording (N=0):    {len(un)}  <- NOT frozen")
    if fz:
        ns = sorted(r["N"] for r in fz)
        print(f"   frozen N: min {ns[0]}, median {ns[len(ns) // 2]}, max {ns[-1]}")
    if ur:
        print(f"\n-- unresolved: compared {HISTORY_POWER_N - 1} time(s) or fewer, so the screen was")
        print("   never actually asked. With N recordings there are N-1 opportunities")
        print("   for the signature to differ; at N=1 there are none. These read as")
        print("   `frozen` until 2026-09-05 — the line above them said so and called")
        print("   it \"surfaced rather than acted on\". This is acting on it.")
        for r in sorted(ur, key=lambda r: r["name"]):
            print(f"   N={r['N']:3}  {r['name']:62} {r['verdicts']}")
    if un:
        print("\n-- never compared: younger than every recording on this box. These")
        print("   read as `frozen` until 2026-08-18 and are the whole of the gap")
        print("   between the inflated total and this one.")
        for r in sorted(un, key=lambda r: r["name"]):
            print(f"   N=  0  {r['name']}")
    if so:
        print("\n-- strict-only: a real build difference reached the corpus and this")
        print("   check did not read it. Nothing would have gone red.")
        for r in sorted(so, key=lambda r: -r["N"]):
            print(f"   N={r['N']:3}  {r['name']:62} {r['verdicts']}")
    print("\n-- frozen (a candidate for ci/corpus-grades.py, not a finding)")
    for r in sorted(fz, key=lambda r: -r["N"]):
        print(f"   N={r['N']:3}  {r['name']:62} {r['verdicts']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
