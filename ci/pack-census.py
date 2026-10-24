#!/usr/bin/env python3
"""Which files stop branches sharing a tip, and what a merge driver on them is worth.

WHY THIS EXISTS
===============
`ci/batch.sh --pack` groups branches by disjoint file sets, because two branches
that change no file in common cannot conflict. Paths carrying a working merge
driver are subtracted, since the drivers exist so that two branches appending to
those files do not conflict.

That leaves an obvious question the batching tools could not answer: **when
packing fails, WHICH file is doing it, and what would giving that file a driver
be worth?** Measured 2026-08-17 over the live queue, the answer was not the one
anybody would have guessed and it is not visible without this:

    11 branches bring 60 new gameplay scenarios.
    All 55 pairs of them collide, so they pack into 11 groups and batching
    saves NOTHING on the corpus lock.

    exempting ci/checks/gameplay-falsifiable.toml alone ->  0 pairs freed
    exempting ci/checks/gameplay-baseline.toml    alone ->  0 pairs freed
    exempting BOTH                                       -> 39 pairs freed,
                                                            11 groups -> 5,
                                                            6 recordings saved,
                                                            ~4.0 h of corpus lock

**Neither file alone is worth anything and the pair is worth four hours.** A
census that reported only single-file blockers would have said "give the ledger a
driver" and the change would have bought exactly zero, which is the kind of
result that gets a fix written off as ineffective. So this scores COMBINATIONS,
not just singles, and that is the whole reason it is a script rather than a grep.

WHAT IT DOES NOT CLAIM
  * Disjointness is sufficient for a clean cherry-pick, not necessary — a driver
    can resolve a shared file, so "pairs freed" is a floor.
  * It scores whether a driver would be POSSIBLE to write for a given file, not
    at all. `ci/merge-docregions.py` resolves keyed rows; whether a file's shape
    fits is a judgement this cannot make. It says what the win would be if one
    existed, which is the input to deciding whether to try.
  * Group counts come from greedy first-fit at the given width, the same
    packing `ci/batch.sh` performs, so they are what that tool would actually do
    rather than an optimum.

USAGE
    ci/pack-census.py --branches-from ../preflight.tsv
    ci/pack-census.py --branches-from ../preflight.tsv --scenario-branches-only
    ci/pack-census.py --what-if ci/checks/gameplay-baseline.toml --what-if ci/t0.sh
    ci/pack-census.py --selftest
"""

import argparse
import itertools
import subprocess
import sys
from collections import Counter

DRIVER_CACHE = {}


def git(*args, cwd=None):
    r = subprocess.run(("git",) + args, capture_output=True, text=True, cwd=cwd)
    return r.stdout


def has_working_driver(path):
    """A path is exempt only if its merge attribute names a driver git HAS.

    An attribute naming a driver that is not configured falls back to the
    ordinary 3-way merge with no warning, so treating such a path as
    non-colliding would be trusting a promise the checkout has not kept. Same
    test as `ci/batch.sh`.
    """
    if path in DRIVER_CACHE:
        return DRIVER_CACHE[path]
    out = git("check-attr", "merge", "--", path).strip()
    drv = out.rsplit(": ", 1)[-1] if ": " in out else ""
    ok = False
    if drv and drv not in ("unspecified", "unset", "binary", "text"):
        ok = bool(git("config", "--get", f"merge.{drv}.driver").strip())
    DRIVER_CACHE[path] = ok
    return ok


def collect(branches, base):
    """-> {branch: (changed_paths, n_new_scenarios)} for branches that resolve."""
    base_sha = git("rev-parse", base).strip()
    out = {}
    for b in branches:
        sha = git("rev-parse", "--verify", "-q", f"refs/remotes/origin/{b}").strip()
        if not sha:
            sha = git("rev-parse", "--verify", "-q", f"refs/heads/{b}").strip()
        if not sha:
            continue
        mb = git("merge-base", base_sha, sha).strip()
        if not mb:
            continue
        paths = {p for p in git("diff", "--name-only", mb, sha).split() if p}
        if not paths:
            continue
        scen = len([p for p in git("diff", "--name-only", "--diff-filter=A", mb, sha,
                                   "--", "ci/checks/gameplay/").split() if p])
        out[b] = (paths, scen)
    return out


def pack(names, paths, exempt, width):
    def collide(a, b):
        return bool((paths[a] - exempt) & (paths[b] - exempt))
    groups = []
    for n in names:
        for g in groups:
            if len(g) < width and all(not collide(n, m) for m in g):
                g.append(n)
                break
        else:
            groups.append([n])
    disjoint = sum(1 for a, b in itertools.combinations(names, 2) if not collide(a, b))
    return groups, disjoint


def report(names, paths, base_exempt, width, what_if, out=sys.stdout):
    w = out.write
    total_pairs = len(names) * (len(names) - 1) // 2
    groups0, dis0 = pack(names, paths, base_exempt, width)
    w(f"\n{len(names)} branches, width {width}, {total_pairs} pairs\n")
    w(f"  today: {dis0}/{total_pairs} disjoint pairs -> {len(groups0)} groups\n")

    blockers = Counter()
    for a, b in itertools.combinations(names, 2):
        for f in (paths[a] & paths[b]) - base_exempt:
            blockers[f] += 1
    if not blockers:
        w("  nothing blocks any pair — packing is already optimal here\n")
        return groups0
    w("\n  files shared by the most pairs (the blockers):\n")
    for f, c in blockers.most_common(8):
        w(f"    {c:4d}/{total_pairs} pairs   {f}\n")

    cands = [f for f, _ in blockers.most_common(4)] + list(what_if)
    seen, uniq = set(), []
    for c in cands:
        if c not in seen:
            seen.add(c)
            uniq.append(c)

    w("\n  what a working merge driver on these would be worth:\n")
    w(f"    {'exempting':60s} {'disjoint':>9s} {'groups':>7s} {'saved':>6s}\n")
    for r in range(1, min(3, len(uniq)) + 1):
        for combo in itertools.combinations(uniq, r):
            g, d = pack(names, paths, base_exempt | set(combo), width)
            saved = len(groups0) - len(g)
            label = " + ".join(c.split("/")[-1] for c in combo)
            mark = "  <-- the pair is load-bearing" if (r > 1 and saved > 0 and all(
                pack(names, paths, base_exempt | {c}, width)[0].__len__() == len(groups0)
                for c in combo)) else ""
            w(f"    {label:60s} {d:9d} {len(g):7d} {saved:6d}{mark}\n")
    w("\n  A combination that helps where every one of its members alone helps\n"
      "  nothing is the result worth acting on: fixing either file by itself\n"
      "  would buy zero and read as a fix that did not work.\n")
    return groups0


def selftest():
    fails = []

    def check(name, got, want):
        if got == want:
            print(f"  PASS  {name}")
        else:
            fails.append(name)
            print(f"  FAIL  {name}: got {got!r}, want {want!r}")

    # Three branches, each touching its own file plus TWO shared files. Every
    # pair collides. Exempting either shared file alone still leaves the other,
    # so nothing changes; exempting both frees every pair. This is the live
    # finding in miniature, and it is what a single-file census gets wrong.
    paths = {
        "a": {"a.rs", "L.toml", "B.toml"},
        "b": {"b.rs", "L.toml", "B.toml"},
        "c": {"c.rs", "L.toml", "B.toml"},
    }
    names = ["a", "b", "c"]
    g, d = pack(names, paths, set(), 3)
    check("all pairs collide today", (len(g), d), (3, 0))
    g, d = pack(names, paths, {"L.toml"}, 3)
    check("exempting one shared file alone changes nothing", (len(g), d), (3, 0))
    g, d = pack(names, paths, {"B.toml"}, 3)
    check("exempting the other alone changes nothing", (len(g), d), (3, 0))
    g, d = pack(names, paths, {"L.toml", "B.toml"}, 3)
    check("exempting BOTH packs them into one group", (len(g), d), (1, 3))

    # Width is respected: four mutually disjoint branches at width 3 are 2 groups.
    p2 = {x: {f"{x}.rs"} for x in "abcd"}
    g, d = pack(list("abcd"), p2, set(), 3)
    check("width caps a group", (len(g), d), (2, 6))
    g, _ = pack(list("abcd"), p2, set(), 1)
    check("width 1 is one group each", len(g), 4)

    # A branch that shares nothing is never merged into a colliding group by
    # accident, and first-fit is order-dependent in the way ci/batch.sh relies on.
    p3 = {"x": {"s"}, "y": {"s"}, "z": {"q"}}
    g, _ = pack(["x", "y", "z"], p3, set(), 3)
    check("colliding pair splits, the free one joins the first", [sorted(i) for i in g],
          [["x", "z"], ["y"]])

    print(f"\npack-census selftest: {'GREEN' if not fails else 'RED'} ({len(fails)} failed)")
    return 1 if fails else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--width", type=int, default=3)
    ap.add_argument("--branches-from", default=None,
                    help="a preflight.tsv; its GREEN rows are the branch set")
    ap.add_argument("--branch", action="append", default=[])
    ap.add_argument("--scenario-branches-only", action="store_true",
                    help="only branches that ADD a ci/checks/gameplay/*.toml")
    ap.add_argument("--what-if", action="append", default=[],
                    help="also score exempting this path")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()

    branches = list(a.branch)
    if a.branches_from:
        with open(a.branches_from) as fh:
            for line in fh:
                f = line.split()
                if len(f) >= 2 and f[1] == "GREEN":
                    branches.append(f[0])
    if not branches:
        print("pack-census: give --branches-from or --branch", file=sys.stderr)
        return 2

    data = collect(branches, a.base)
    if a.scenario_branches_only:
        data = {k: v for k, v in data.items() if v[1] > 0}
    if len(data) < 2:
        print(f"pack-census: {len(data)} branch(es) resolved — nothing to compare",
              file=sys.stderr)
        return 2
    paths = {k: v[0] for k, v in data.items()}
    scen = sum(v[1] for v in data.values())
    names = list(paths)

    every = set().union(*paths.values())
    base_exempt = {p for p in every if has_working_driver(p)}
    print(f"pack-census: {len(names)} branches over {a.base}, "
          f"{len(base_exempt)} path(s) already driver-managed")
    if scen:
        print(f"pack-census: they bring {scen} new gameplay scenario(s); each group "
              f"needs ONE ~40-minute recording, so groups are recordings here")
    report(names, paths, base_exempt, a.width, a.what_if)
    return 0


if __name__ == "__main__":
    sys.exit(main())
