#!/usr/bin/env python3
"""What a per-delta reference cache would actually buy, measured not argued.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/refcache-reuse.py                 # against preflight.tsv's GREEN rows
#     ci/refcache-reuse.py --since '7 days ago'
#
# Prints, for the live queue: how many branches move each component of
# `refcache_components`, and how often `main` itself moves one. Pure git, no
# box, no build, seconds.
#
# WHAT BREAKS IF YOU IGNORE THIS: you build the per-delta cache. It addresses
# the component that moves LEAST, and the hit rate stays at approximately
# zero, which is where it is now.
# ===== end interface; everything below is why =====

# # THE QUESTION, AND WHY THE OBVIOUS ANSWER IS THE WRONG ONE
#
# `docs/ASSESSMENT-2026-08-19.md` §4.3 says the reference cache "stored four
# entries and hit approximately never, because its key hashes the scenario set
# by path and content and every branch edits scenarios", and asks for either a
# per-delta key -- a branch touching three scenarios reuses the recording for
# the other 105 -- or deletion.
#
# The premise is measurable and it is mostly false. Measured 2026-08-19 over
# the 60 GREEN rows of `preflight.tsv`, against 108 scenarios on main:
#
# (108 is the top-level `*.toml` count, which is what `refcache_components`
# calls `n_checks`. Its `scenarios` hash covers 111 files recursively,
# including `open/160-water-spreads.toml` and an `open/README.md` -- so a
# README edit under `ci/checks/gameplay/` moves the scenarios component and
# the count it is printed beside does not move. Not the cause of anything
# here, but worth knowing before reading the two numbers as one.)
#
#     40 of 60 branches (67%) touch ZERO scenarios
#     mean touched 1.98, median 0; only three branches touch more than 7
#     38 of 60 (63%) move NONE of scenarios, harness or recorder
#
# So two thirds of the queue should hit the cache TODAY, with the key exactly
# as it is. They do not. The scenario set is not what is stopping them, and a
# per-delta key would change nothing for the 63% whose scenario hash is
# already identical to main's.
#
# # WHAT IS ACTUALLY INVALIDATING IT
#
# The key has eight components and a branch is not the only thing that moves
# them. `main` moves them, and every branch rebases onto `main`. Over the seven
# days to 2026-08-19, across 350 commits on main:
#
#     harness    9 commits   (ci/t2-gameplay.sh, vanilla-server.sh, reference-cache.sh)
#     recorder   8 commits   (tools/oracle-runner/src/*.rs)
#     scenarios  5 commits   (ci/checks/gameplay/*.toml)
#
# That is 22 invalidations in 7 days, about one every 7.6 hours, against a gate
# that takes 65.2 minutes. An entry survives roughly six gates before main
# invalidates it for every tree on the box at once -- and because each tree
# rebases onto a different main tip, each computes a key no other tree will
# ever compute again. That is the "four entries, hits approximately never"
# observation, and its cause is the rebase-onto-moving-main landing model, not
# the scenario hash.
#
# The live store agrees. Six entries on 2026-08-19; `jar` and `seed` are
# constant across all six, `tables` takes two values, and `harness` takes
# THREE. Two entries are identical in every component except `harness`.
#
# # SO THE PER-DELTA ESTIMATE, WHICH IS THE THING THAT WAS ASKED FOR
#
# Per-delta keying addresses the `scenarios` component and nothing else.
#
#     it addresses    5 of 22 weekly invalidations        23%
#     it leaves       17 of 22 (harness + recorder)       77%
#
# and it leaves them for a structural reason rather than an incidental one: a
# recorder change alters how EVERY scenario is recorded, so it invalidates all
# 108 regardless of how the key is sliced. Per-delta cannot be made to help
# there by any amount of care.
#
# The reuse it would buy on the 20 branches that do touch scenarios is real --
# a mean of 5.95 touched among those, so ~102 of 108 recordings reusable --
# but it only pays on a branch that ALSO did not move the harness or the
# recorder, and it requires the recording to become sliceable per scenario,
# which is a change to the recorder's output format and to ci/t2-gameplay.sh.
#
# **Building it would be building the smaller half of the fix and measuring no
# hits at the end of it.** That is the estimate the assessment asked for, and
# it refuses the work on its own numbers.
#
# # WHAT THE MEASUREMENT POINTS AT INSTEAD, AND WHY IT IS NOT DONE HERE
#
# `harness` is the single largest invalidator and it is the crudest component:
# the concatenation of three whole files, one of which is
# `ci/reference-cache.sh` ITSELF. So editing a comment in the cache invalidates
# every entry the cache holds. `ci/t2-gameplay.sh` is a ~1,000-line
# orchestrator whose lock handling, verdict strings and gate logic cannot
# affect what vanilla records, and every edit to any of it evicts the store.
#
# That is a defect independent of the per-delta question, and the fix is to
# hash a declared format version plus the functions that actually produce a
# recording rather than three files whole.
#
# **It is deliberately not done in this commit.** Narrowing a cache key is
# exactly the change that turns "hits approximately never" into "serves a
# stale reference", and the property that has made a stale serve impossible so
# far is that every component is fail-closed: an unreadable input prints
# `MISSING` and `refcache_lookup` refuses rather than treating it as a match.
# Any narrowing has to keep that and has to come with cases in
# `ci/reference-cache-falsify.sh` showing the narrowed key still refuses a
# recording taken under a harness that could have changed the recording. That
# is a measured piece of work, not a one-liner, and it should be done with the
# hit rate instrumented so the result is a number rather than a hope.
#
# # THE DECISION
#
# Do NOT build per-delta: measured, it addresses 23% of invalidations and
# cannot lift the hit rate. Do NOT delete the cache either -- deleting it
# would discard a fail-closed key that is correct, and the reason it does not
# pay is the harness component, which is repairable. Leave it as it is, with
# this measurement recorded, and let the next lane repair `harness` with a
# falsifier and an instrumented hit rate.

"""

from __future__ import annotations

import argparse
import subprocess
import sys

# Exactly the paths `refcache_components` reads that a branch can move.
# `jar` and `seed` are constant; `tables` and `projection` are derived rather
# than tracked and are reported separately by ci/reference-cache.sh --explain.
COMPONENTS = {
    "scenarios": ["ci/checks/gameplay"],
    "harness": ["ci/t2-gameplay.sh", "ci/vanilla-server.sh", "ci/reference-cache.sh"],
    "recorder": ["tools/oracle-runner/src"],
}


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True).stdout


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--preflight", default="/home/sayer/grit/preflight.tsv")
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--since", default="7 days ago")
    a = ap.parse_args()

    try:
        rows = [l.split("\t") for l in open(a.preflight).read().splitlines() if l.strip()]
    except OSError as e:
        print(f"refcache-reuse: cannot read {a.preflight}: {e}", file=sys.stderr)
        return 2
    branches = [r[0] for r in rows if len(r) > 1 and r[1] == "GREEN"]
    if not branches:
        print("refcache-reuse: no GREEN rows — nothing to measure", file=sys.stderr)
        return 2

    n_scen = len([l for l in git("ls-tree", "-r", "--name-only", a.base,
                                 "ci/checks/gameplay/").splitlines()
                  if l.endswith(".toml")])

    moves = {k: 0 for k in COMPONENTS}
    clean = 0
    touched_counts = []
    for b in branches:
        files = set(git("diff", "--name-only", f"{a.base}...origin/{b}").splitlines())
        moved_any = False
        for comp, paths in COMPONENTS.items():
            hit = any(f == p or f.startswith(p + "/") for f in files for p in paths)
            if hit:
                moves[comp] += 1
                moved_any = True
        touched_counts.append(sum(
            1 for f in files
            if f.startswith("ci/checks/gameplay/") and f.endswith(".toml")))
        if not moved_any:
            clean += 1

    n = len(branches)
    print(f"queue: {n} GREEN branches against {a.base}, {n_scen} scenarios on it")
    for comp in COMPONENTS:
        print(f"  move {comp:<10} {moves[comp]:3d} ({100*moves[comp]/n:4.0f}%)")
    print(f"  move NOTHING     {clean:3d} ({100*clean/n:4.0f}%)"
          "  <- these already hit under the key as it stands")
    mean_t = sum(touched_counts) / n
    nz = [t for t in touched_counts if t]
    print(f"  scenarios touched: mean {mean_t:.2f} over all, "
          f"mean {sum(nz)/len(nz) if nz else 0:.2f} over the {len(nz)} that touch any")

    print(f"\nmain's own moves in the last {a.since!r} "
          f"({git('rev-list', '--count', f'--since={a.since}', a.base).strip()} commits):")
    total = 0
    for comp, paths in COMPONENTS.items():
        c = int(git("rev-list", "--count", f"--since={a.since}", a.base,
                    "--", *paths).strip() or 0)
        total += c
        print(f"  {comp:<10} {c:3d} commits")
    print(f"  total      {total:3d} invalidations of every entry on the box")
    if total:
        share = 100 * int(git("rev-list", "--count", f"--since={a.since}", a.base,
                              "--", *COMPONENTS["scenarios"]).strip() or 0) / total
        print(f"\nper-delta keying addresses the scenarios component only: "
              f"{share:.0f}% of invalidations.")
        print("The other components invalidate ALL scenarios at once — a recorder "
              "change alters how every one is recorded — so no slicing of the key "
              "reaches them. See this file's header for the decision.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
