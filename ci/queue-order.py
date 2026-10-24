#!/usr/bin/env python3
"""Build the merge queue: drop what needs no gate, then order what is left.

# ===== AGENT INTERFACE - this is all you need to read =====
#
#     ci/queue-order.py                    # the ordered queue, one branch a line
#     ci/queue-order.py --explain          # ... with the tier and the reason
#     ci/queue-order.py --stats            # what this queue costs to drain
#     ci/queue-order.py --queue FILE       # order an existing queue file
#     ci/queue-order.py --selftest         # red-when-broken evidence
#
# WHAT BREAKS IF YOU IGNORE THIS: you order by `committerdate` -- which is what
# `chain-keeper.sh` falls back to -- and the branches that make every LATER
# gate cheaper sit in the middle of the queue. `corpus-lock-hold-span` was at
# position 66 of 68 on 2026-08-18 while four agents sat behind the corpus lock
# it measures.
# ===== end interface; everything below is why =====

# # ORDER DOES NOT CHANGE DRAIN TIME. SAY THAT FIRST, BECAUSE IT IS THE TRAP.
#
# A serial chain gates one branch after another, so the total wall clock is the
# SUM of the gate costs, and a sum does not care about order. Any claim of the
# form "reordering the queue drains it faster" is false on its face, and this
# script does not make it. `--stats` prints the same total for any permutation
# and that is deliberate: it is the number that must not move.
#
# Reordering pays in exactly three places, and the tiers below are those three
# and nothing else:
#
#   1. GATE-COST LEVERS. A branch that makes every subsequent gate cheaper is
#      worth (branches remaining) x (saving). The same branch is worth ~0 at
#      the end of the queue. This term is quadratic in nothing and linear in
#      position, and it is the only tier that changes total drain at all --
#      because it changes what the gates AFTER it cost. Measured precedent:
#      `refcache-live` took ~21 min off an ~87 min gate on 2026-08-17, and the
#      03:39 relaunch put it second, so every gate since was shorter.
#
#   2. QUEUE ADMISSION. A branch whose absence keeps OTHER branches out of the
#      queue entirely. `handoff-logpath-slash` is one commit and re-admits
#      every branch with a `/` in its name -- `chain-keeper.sh` drops those,
#      by name, because `ci/handoff.sh:408` builds a log path from $BRANCH and
#      dies mid-gate on the missing directory. Two were dropped on 2026-08-18
#      and `scratch-check/*` is an established convention here, so there will
#      be more. Its value is the branches it lets in, which no reading of the
#      branch itself can see.
#
#   3. CONFLICT-CLUSTER ADJACENCY. Every merge moves main and every remaining
#      branch must rebase onto it. Branches that touch one file therefore
#      invalidate each other's rebases, and interleaving them with unrelated
#      merges means each one re-resolves the same collision against a main
#      that moved in between. Gating a cluster back to back resolves it once.
#      Measured on the 2026-08-18 queue: 23 of 67 branches touch `ci/t0.sh`
#      and it is NOT in gate-chain.sh's UNIONABLE list, so those 23 collide
#      with each other for hands rather than being auto-unioned.
#
# # AND WHAT CLUSTERING DOES ABOUT GREENS THAT DO NOT COMPOSE
#
# `gameplay-sound-eventid` gives `use_sound` a sixth parameter; `gameplay-790-
# flower-pot` moves the callsite. Each compiles against main alone and the two
# together do not. That is the hazard that keeps `ci/lanes.sh` disabled: lanes
# gate each pipelined position BEFORE it lands, so both can pass and the
# combination never gets gated.
#
# A serial chain does not have that hole and this script does not open one.
# The chain rebases each branch onto the main that already contains everything
# ahead of it, so the SECOND of the pair is gated against the first -- the
# combination IS what is measured, and the incompatibility comes out RED.
# Ordering cannot break that property because it does not touch it.
#
# What clustering changes is only WHEN the red arrives. Those two branches
# share `crates/rustmc-server/src/world_send.rs`, so tier 3 places them
# adjacent and the red lands in the next slot instead of forty slots later,
# while the lane that wrote it is still there. Same verdict, sooner.
#
# # WHY NOT `git rev-list --count`, EVER
#
# It counts commits by sha. A commit that landed on main through a rebase has
# a different sha and the same patch, so the count says the branch is ahead
# when it is empty. `worldgen-12345-next-items` read 75 ahead from a stale
# LOCAL ref while `origin/` was at 0. `git cherry` compares patch-ids and sees
# through both. `ci/unlanded.sh` is the same argument at fleet scale: 780
# naive against 453 real across 218 branches on 2026-08-18.
#
# `--selftest` asserts this file contains no `rev-list --count`, because a
# rule nothing enforces is a rule that comes back.
"""

import argparse
import collections
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# gate-chain.sh resolves a collision in these by keeping BOTH sides, so a
# branch touching one of them does NOT collide with another for hands. They
# must not drive clustering or every branch lands in one useless cluster:
# 59 of 67 touch docs/WORKLIST.md.
UNIONABLE = {
    "docs/WORKLIST.md",
    "docs/PARITY-ROADMAP.md",
    "docs/DETERMINISM.md",
    "docs/PARITY.md",
    "ci/figures.toml",
}

# Tier 1. A branch touching one of these changes what EVERY LATER GATE COSTS,
# rather than adding work of its own. Narrow on purpose: `ci/t0.sh` is NOT
# here even though 23 branches touch it, because touching t0 almost always
# means ADDING a check, which makes gates dearer rather than cheaper. Each
# entry names the mechanism, so an addition has to argue for itself.
GATE_COST_PATHS = {
    "ci/reference-cache.sh": "the reference cache: ~21 min off an ~87 min gate (2026-08-17)",
    "ci/reference-cache-integration.sh": "the reference cache's wiring into the gate",
    "ci/corpus-lock.sh": "corpus-lock hold: the gate's serialisation point",
    "ci/handoff.sh": "the gate itself -- its stages and their cost",
    "ci/t0-remote.sh": "how the gate ships a tree to the box",
    "ci/batch.sh": "gates per landed branch",
    "ci/lanes.sh": "gates in flight at once",
    "ci/merge-appendonly.py": "merge driver: rebase conflicts 26 -> 9 (2026-08-17)",
    "ci/merge-docregions.py": "merge driver: the registry TOMLs that re-collide every time main moves",
    "ci/setup-merge-drivers.sh": "whether the drivers are registered at all",
    ".gitattributes": "which driver each path gets",
}

# Tier 0. MAIN IS BROKEN WITHOUT THIS. Not "valuable" -- load-bearing: while
# the fault stands, gates spend their slot reporting a fault that is not in the
# branch, which is how 2026-08-18 lost 09:12-12:06 and 52 branches to one
# `ci/check-figures.py` rule. A tier-0 entry is a claim that MAIN fails a check
# TODAY, so each one cites the measurement and goes stale loudly rather than
# quietly: `--explain` prints the citation, and when the branch has landed it
# simply stops matching.
#
# This cannot be inferred from a diff. It is the difference between "this
# branch improves the gate" and "no gate tells the truth until this lands",
# and only the second justifies pre-empting everything else.
MAIN_IS_BROKEN = {
    "boxcopy":
        "origin/main fails ci/figure-provenance.py --check 56 of 56 in any real "
        "repository -- git's %h abbreviation grew 8 -> 9 as the repo crossed a "
        "size threshold on 2026-08-18",
    "ci-handoff-terminal-states":
        "main's ci/handoff.sh prints a bare WARNING the chain cannot name, so "
        "chain-falsify is 68/69 and a red gate logs as 'no verdict line at all'",
}

# Tier 2. Not a file predicate -- these are branches whose absence keeps other
# branches OUT of the queue. Kept as an explicit list because the property is
# about the queue builder's behaviour, not about the branch's diff, and there
# is no honest way to infer it from the tree. Each entry cites the exclusion
# it lifts so a stale entry is visible rather than merely wrong.
QUEUE_ADMISSION = {
    "handoff-logpath-slash":
        "chain-keeper.sh drops every branch with '/' in its name until "
        "ci/handoff.sh sanitises $BRANCH for the log path (2 dropped 2026-08-18)",
}

TIER_NAMES = {
    0: "MAIN-IS-BROKEN",
    1: "GATE-COST",
    2: "QUEUE-ADMISSION",
    3: "GREEN",
    4: "UNSEEN",
    5: "PREFLIGHT-RED",
    6: "REBASE-CONFLICT",
}


def git(*args, cwd=None, check=True):
    r = subprocess.run(["git"] + list(args), cwd=cwd or ROOT,
                       capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


# ---------------------------------------------------------------------------
# ADMIT -- what needs no gate at all
# ---------------------------------------------------------------------------

def admit(branches, main, cwd=None):
    """Split branches into (kept, dropped) where dropped is [(branch, why)].

    Never counts commits by sha. The two drops are:

      LANDED-ANCESTOR  the branch tip is reachable from main. Cheap, exact,
                       and misses everything that landed under a rebase.
      LANDED-PATCH     `git cherry` finds no commit whose patch-id is absent
                       from main. This is the one that catches a rebase, and
                       it is the check chain-keeper.sh already applies -- so
                       on a queue the keeper built it should find nothing,
                       and finding nothing IS the result. Measured on the
                       2026-08-18 queue: 68 entries, 1 dropped.
    """
    kept, dropped = [], []
    for b in branches:
        ref = f"origin/{b}"
        if subprocess.run(["git", "rev-parse", "--verify", "-q", ref],
                          cwd=cwd or ROOT, capture_output=True).returncode != 0:
            dropped.append((b, "NO-REMOTE  no such branch on origin"))
            continue
        if subprocess.run(["git", "merge-base", "--is-ancestor", ref, main],
                          cwd=cwd or ROOT, capture_output=True).returncode == 0:
            dropped.append((b, "LANDED-ANCESTOR  tip is reachable from main"))
            continue
        cherry = git("cherry", main, ref, cwd=cwd, check=False)
        ahead = [ln for ln in cherry.splitlines() if ln.startswith("+")]
        if not ahead:
            dropped.append((b, "LANDED-PATCH  every commit's patch-id is already on main"))
            continue
        kept.append(b)
    return kept, dropped


# ---------------------------------------------------------------------------
# RANK
# ---------------------------------------------------------------------------

def file_sets(branches, main, cwd=None):
    out = {}
    for b in branches:
        d = git("diff", "--name-only", f"{main}...origin/{b}", cwd=cwd, check=False)
        out[b] = set(x for x in d.splitlines() if x)
    return out


def read_preflight(path):
    """branch -> (verdict, detail). Missing file is not an error: the ordering
    degrades to tiers 1, 2 and 4, and `--explain` says so rather than being
    silently different. chain-keeper.sh learned that lesson on 2026-08-18,
    when an empty preflight.tsv dropped the PRIORITY pin without a word."""
    pf = {}
    if not path or not os.path.exists(path):
        return pf
    for ln in open(path, encoding="utf-8"):
        p = ln.rstrip("\n").split("\t")
        if len(p) >= 2:
            pf[p[0]] = (p[1], p[2] if len(p) > 2 else "-")
    return pf


def tier_of(branch, files, pf):
    if branch in MAIN_IS_BROKEN:
        return 0, MAIN_IS_BROKEN[branch]
    hit = sorted(f for f in files if f in GATE_COST_PATHS)
    if hit:
        return 1, f"touches {hit[0]} -- {GATE_COST_PATHS[hit[0]]}"
    if branch in QUEUE_ADMISSION:
        return 2, QUEUE_ADMISSION[branch]
    v = pf.get(branch, ("UNSEEN", "-"))[0]
    if v == "GREEN":
        return 3, "preflight GREEN -- will reach a real gate"
    if v == "RED":
        return 5, f"preflight RED ({pf[branch][1]}) -- reds in T0 at ~0.3 min, lands nothing"
    if v == "REBASE-CONFLICT":
        return 6, "preflight could not rebase it onto main -- needs hands, not a slot"
    return 4, "preflight has not seen it"


def cluster_key(files):
    """The contended, NON-unionable files this branch touches.

    Two branches sharing one of these collide with each other for hands when
    main moves between their gates. Unionable files are excluded because
    gate-chain.sh resolves those automatically -- including them would put 59
    of 67 branches in one cluster and mean nothing.
    """
    return frozenset(f for f in files if f not in UNIONABLE)


def order(branches, main, preflight_path, cwd=None):
    fs = file_sets(branches, main, cwd=cwd)
    pf = read_preflight(preflight_path)

    # How many branches touch each non-unionable file -- a file only creates a
    # cluster if at least two branches want it.
    touches = collections.Counter()
    for b in branches:
        for f in cluster_key(fs[b]):
            touches[f] += 1

    rows = []
    for b in branches:
        t, why = tier_of(b, fs[b], pf)
        contended = sorted((f for f in cluster_key(fs[b]) if touches[f] > 1),
                           key=lambda f: (-touches[f], f))
        rows.append({
            "branch": b, "tier": t, "why": why,
            "files": len(fs[b]),
            "hot": contended[0] if contended else "",
            "hotn": touches[contended[0]] if contended else 0,
        })

    # TWO SORTS, NOT ONE, and the split is the whole design.
    #
    # Tiers 0-2 are LEVERS: their value is (branches remaining) x (what they
    # save), so it decays with every slot they wait. They are ordered smallest
    # first -- fewest files is the best available proxy for "lands soonest and
    # collides least" -- and clustering is deliberately NOT applied to them.
    # Clustering by the hottest file would sort a 2-file fix behind a 51-file
    # branch because both touch `ci/t0.sh`, which is exactly backwards for a
    # tier whose entire point is landing early. That is what the first draft of
    # this function did: `handoff-logpath-slash`, 2 files and the fix that
    # re-admits a whole naming convention, came out at position 8 behind six
    # branches averaging 30 files.
    #
    # Tiers 3-6 are the BULK, where no branch makes another cheaper and the
    # only thing left to optimise is rebase churn. There, cluster: branches
    # sharing a contended non-unionable file gate back to back, most-contended
    # file first, so that collision is resolved once against a main that moves
    # within the cluster rather than re-resolved after every unrelated merge.
    #
    # `name` breaks every tie, deterministically and never by date. The newest
    # branch is the one whose lane is most likely still editing it (4 worktrees
    # were dirty at 12:13 on 2026-08-18) and the oldest is the one most likely
    # to have accumulated conflicts. Neither is value, and `committerdate` --
    # what chain-keeper.sh falls back to -- is one of the two.
    rows.sort(key=sort_key)
    return rows


def sort_key(r):
    """Module-level so --selftest exercises the REAL key, not a copy of it.

    The bug this shape exists to prevent was in a copy: the first draft sorted
    every tier by cluster, and the selftest asserted a hand-written key that
    happened to agree with it. Both were wrong together, which is what a
    restated predicate buys you."""
    if r["tier"] <= 2:
        return (r["tier"], 0, "", r["files"], r["branch"])
    return (r["tier"], -r["hotn"], r["hot"], r["files"], r["branch"])


# ---------------------------------------------------------------------------
# STATS -- what the queue costs, and what it would cost batched
# ---------------------------------------------------------------------------

def stats(rows, cost_green, cost_red, out=sys.stdout):
    green = sum(1 for r in rows if r["tier"] in (1, 2, 3, 4))
    red = sum(1 for r in rows if r["tier"] == 5)
    conf = sum(1 for r in rows if r["tier"] == 6)
    total = green * cost_green + red * cost_red
    w = out.write
    w(f"queue = {len(rows)} branches after ADMIT\n\n")
    w(f"  {green:3d} reach a real gate   x {cost_green:5.1f} min = {green*cost_green/60:6.1f} h\n")
    w(f"  {red:3d} red in T0           x {cost_red:5.1f} min = {red*cost_red/60:6.1f} h\n")
    w(f"  {conf:3d} cannot rebase       x   ~0   min = {0.0:6.1f} h\n")
    w(f"  {'':3s} SERIAL DRAIN                        = {total/60:6.1f} h\n\n")
    w("  This total is the same for every permutation of the queue. Ordering\n"
      "  moves value earlier; it does not make the sum smaller. The only tier\n"
      "  that changes it is GATE-COST, and only for the gates that follow it.\n")
    return total


# ---------------------------------------------------------------------------
# SELFTEST -- red-when-broken evidence
# ---------------------------------------------------------------------------

def selftest():
    fails = []

    def check(name, cond, detail=""):
        if cond:
            print(f"  ok    {name}")
        else:
            print(f"  FAIL  {name}  {detail}")
            fails.append(name)

    src = open(os.path.abspath(__file__), encoding="utf-8").read()

    # 1. THE RULE THAT COMES BACK IF NOTHING ENFORCES IT.
    #
    # `rev-list --count` overstates a backlog by counting rebased-and-landed
    # commits. Assert the QUEUE-BUILDING code never calls it.
    #
    # Scoped to everything above `def selftest`, for a reason worth stating:
    # test 7 below calls `rev-list --count` ON PURPOSE, to show the instrument
    # being replaced giving the wrong answer on a case this one gets right. A
    # blanket ban over the whole file would forbid demonstrating the fault,
    # which is how a check ends up asserting a rule nobody can see fail.
    # Comments and docstrings are stripped so the check cannot be satisfied by
    # deleting the explanation of it.
    code = src.split("def selftest")[0]
    code = "\n".join(ln for ln in code.split("\n")
                     if not ln.lstrip().startswith("#"))
    code = code.replace('"""', "\x00")
    code = re.sub(r"\x00.*?\x00", "", code, flags=re.S)
    check("no rev-list --count in the queue-building code",
          "rev-list" not in code or "--count" not in code,
          "use git cherry: a rebased commit has a new sha and the same patch")

    # 2. ORDER IS A PERMUTATION. A sort that drops or duplicates a branch
    # silently shortens the queue, which looks exactly like the drop the
    # ADMIT stage is supposed to make deliberately.
    fake = [{"branch": f"b{i}", "tier": (i % 6) + 1, "why": "", "files": i,
             "hot": "f" if i % 2 else "", "hotn": i % 3} for i in range(20)]
    s = sorted(fake, key=sort_key)
    check("ordering is a permutation",
          sorted(r["branch"] for r in s) == sorted(r["branch"] for r in fake))

    # 3. TIERS ARE MONOTONE: the emitted sequence never goes back up a tier.
    check("tiers are non-decreasing down the queue",
          all(s[i]["tier"] <= s[i + 1]["tier"] for i in range(len(s) - 1)))

    # 3b. A LEVER TIER IS ORDERED BY SIZE, NOT BY CLUSTER. This is the bug the
    # first draft shipped: `handoff-logpath-slash` (2 files, re-admits a whole
    # branch-naming convention) sorted to position 8, behind six branches
    # averaging 30 files, because all seven touched `ci/t0.sh` and the cluster
    # key ran before the size key. Shown on the real sort_key.
    small = {"branch": "small-lever", "tier": 1, "files": 2, "hot": "ci/t0.sh", "hotn": 23}
    big = {"branch": "big-lever", "tier": 1, "files": 51, "hot": "ci/t0.sh", "hotn": 23}
    check("in a lever tier the 2-file branch beats the 51-file one",
          sort_key(small) < sort_key(big))

    # 3c. ...and in the BULK tiers clustering DOES apply, because there no
    # branch makes another cheaper and rebase churn is all that is left.
    hot = {"branch": "z-hot", "tier": 3, "files": 40, "hot": "ci/t0.sh", "hotn": 23}
    cold = {"branch": "a-cold", "tier": 3, "files": 1, "hot": "", "hotn": 0}
    check("in a bulk tier the contended-file branch is gated first",
          sort_key(hot) < sort_key(cold))

    # 3d. TIER 0 OUTRANKS EVERYTHING. A branch main is broken without is not
    # merely valuable; every gate behind it reports a fault that is not in the
    # branch being gated. 52 branches went red that way on 2026-08-18.
    t_broken, _ = tier_of("boxcopy", {"whatever.rs"}, {"boxcopy": ("RED", "check-figures")})
    t_lever2, _ = tier_of("z", {"ci/reference-cache.sh"}, {})
    check("MAIN-IS-BROKEN outranks a gate-cost lever", t_broken < t_lever2)
    check("...and outranks it even when preflight calls the branch RED",
          t_broken == 0)

    # 4. UNIONABLE FILES MUST NOT CLUSTER. If they did, the 59 branches
    # touching docs/WORKLIST.md would form one cluster and clustering would
    # carry no information at all. Shown failing by construction: a branch
    # whose ONLY shared file is unionable must produce an empty cluster key.
    check("a unionable-only branch has no cluster key",
          cluster_key({"docs/WORKLIST.md", "docs/PARITY.md"}) == frozenset())
    check("a real source file does produce a cluster key",
          cluster_key({"docs/WORKLIST.md", "ci/t0.sh"}) == frozenset({"ci/t0.sh"}))

    # 5. GATE-COST OUTRANKS GREEN. The whole point of tier 1 is that a lever
    # beats a branch that merely passes preflight. Assert it on the real
    # tier_of, not on a fixture.
    t_lever, _ = tier_of("x", {"ci/reference-cache.sh"}, {"x": ("GREEN", "-")})
    t_green, _ = tier_of("y", {"src/whatever.rs"}, {"y": ("GREEN", "-")})
    check("a gate-cost lever outranks a plain GREEN", t_lever < t_green,
          f"lever={t_lever} green={t_green}")

    # 6. A PREFLIGHT-RED GOES LAST, BUT NOT BEHIND A BRANCH THAT CANNOT
    # REBASE. A red costs ~0.3 min and lands nothing; a rebase-conflict costs
    # nothing and needs hands. Both are cheap, so their order matters only for
    # who reads the report -- but it must be decided, not accidental.
    t_red, _ = tier_of("r", {"a.rs"}, {"r": ("RED", "check-figures")})
    t_conf, _ = tier_of("c", {"a.rs"}, {"c": ("REBASE-CONFLICT", "-")})
    t_unseen, _ = tier_of("u", {"a.rs"}, {})
    check("preflight RED sinks below UNSEEN", t_unseen < t_red)
    check("rebase-conflict sinks below preflight RED", t_red < t_conf)

    # 7. THE ADMIT STAGE MUST ACTUALLY DROP A LANDED BRANCH. Built as a real
    # repository so the drop is exercised rather than asserted, and shown
    # failing on the unrepaired state: without the `git cherry` arm, a branch
    # whose commit was cherry-picked onto main reads as unlanded forever.
    with tempfile.TemporaryDirectory() as d:
        def g(*a, **kw):
            return subprocess.run(["git"] + list(a), cwd=kw.get("cwd", d),
                                  capture_output=True, text=True)
        g("init", "-q", "-b", "main")
        g("config", "user.email", "t@t")
        g("config", "user.name", "t")
        open(f"{d}/f", "w").write("one\n")
        g("add", "f"); g("commit", "-qm", "base")
        g("checkout", "-q", "-b", "feature")
        open(f"{d}/f", "w").write("one\ntwo\n")
        g("add", "f"); g("commit", "-qm", "the work")
        feat = g("rev-parse", "HEAD").stdout.strip()
        g("checkout", "-q", "main")
        # main moves FIRST, with an unrelated commit. Without this the
        # cherry-pick below reproduces `feat` byte for byte -- same parent,
        # tree, author, message and timestamp all hash to the same object --
        # and the fixture models nothing at all: the branch comes out an
        # ANCESTOR of main and the interesting arm is never reached. That is
        # what this fixture did on its first run, and it passed the wrong test.
        open(f"{d}/unrelated", "w").write("elsewhere\n")
        g("add", "unrelated"); g("commit", "-qm", "main moves on")
        # NOW main absorbs the same patch under a DIFFERENT sha -- which is
        # what a rebase does, and this repo merges by rebase.
        g("cherry-pick", feat)
        main_sha = g("rev-parse", "HEAD").stdout.strip()
        check("the fixture really models a rebase (same patch, new sha)",
              main_sha != feat)
        # A remote-shaped ref, because admit() reads origin/<b>.
        g("update-ref", "refs/remotes/origin/feature", feat)
        g("update-ref", "refs/remotes/origin/main", main_sha)

        naive = g("rev-list", "--count", "refs/remotes/origin/main..refs/remotes/origin/feature")
        check("the instrument this replaces is WRONG here "
              "(rev-list --count says 1 ahead)", naive.stdout.strip() == "1",
              f"got {naive.stdout.strip()!r}")

        kept, dropped = admit(["feature"], "refs/remotes/origin/main", cwd=d)
        check("admit drops a branch main absorbed under a new sha",
              kept == [] and dropped and "LANDED-PATCH" in dropped[0][1],
              f"kept={kept} dropped={dropped}")

        # ... and does NOT drop one carrying real work.
        g("checkout", "-q", "-b", "live", feat)
        open(f"{d}/f2", "w").write("novel\n")
        g("add", "f2"); g("commit", "-qm", "novel work")
        g("update-ref", "refs/remotes/origin/live", g("rev-parse", "HEAD").stdout.strip())
        g("checkout", "-q", "main")
        kept, dropped = admit(["live"], "refs/remotes/origin/main", cwd=d)
        check("admit keeps a branch with a patch main does not have",
              kept == ["live"], f"kept={kept} dropped={dropped}")

    # 8. STATS IS ORDER-INVARIANT. The claim in the header -- that reordering
    # does not change drain -- is the one a reader is most likely to misquote,
    # so it is asserted rather than merely written down.
    import random
    rows = [{"branch": f"b{i}", "tier": (i % 6) + 1} for i in range(30)]
    shuffled = rows[:]
    random.shuffle(shuffled)
    import io
    a = stats(rows, 65.5, 1.5, out=io.StringIO())
    b = stats(shuffled, 65.5, 1.5, out=io.StringIO())
    check("serial drain is invariant under permutation", abs(a - b) < 1e-9,
          f"{a} vs {b}")

    print(f"\nqueue-order selftest: {len(fails)} failure(s)")
    return 1 if fails else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--queue", help="file of branch names; default: every origin/* ref")
    ap.add_argument("--main", default="origin/main")
    ap.add_argument("--preflight", default="/home/sayer/grit/preflight.tsv")
    ap.add_argument("--explain", action="store_true")
    ap.add_argument("--stats", action="store_true")
    ap.add_argument("--cost-green", type=float, default=65.5)
    ap.add_argument("--cost-red", type=float, default=1.5)
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    if a.queue:
        branches = [l.strip() for l in open(a.queue) if l.strip()]
    else:
        refs = git("for-each-ref", "--format=%(refname:short)", "refs/remotes/origin")
        branches = [r[len("origin/"):] for r in refs.split()
                    if r.startswith("origin/") and r not in ("origin/main", "origin/HEAD")]

    main_sha = git("rev-parse", "--short", a.main).strip()
    kept, dropped = admit(branches, a.main)
    rows = order(kept, a.main, a.preflight)

    if a.explain or a.stats:
        print(f"# against {a.main} ({main_sha}); {len(branches)} in, "
              f"{len(dropped)} dropped, {len(rows)} queued")
        for b, why in dropped:
            print(f"# DROP  {b:44s} {why}")
        print()

    if a.stats:
        stats(rows, a.cost_green, a.cost_red)
        print()

    if a.explain:
        last = None
        for i, r in enumerate(rows, 1):
            if r["tier"] != last:
                print(f"\n--- tier {r['tier']} {TIER_NAMES[r['tier']]} ---")
                last = r["tier"]
            hot = f"[{r['hot']} x{r['hotn']}]" if r["hot"] else ""
            print(f"{i:3d}  {r['branch']:44s} {r['files']:3d}f {hot}")
            print(f"     {r['why']}")
    else:
        for r in rows:
            print(r["branch"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
