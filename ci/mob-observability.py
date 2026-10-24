#!/usr/bin/env python3
"""mob-observability.py — what a mob check compares, against what vanilla sent.

# ===== AGENT INTERFACE =====
#
#     ci/mob-observability.py             the corpus side, from the tree alone
#     ci/mob-observability.py --measure   add the recordings on this box (N runs)
#     ci/mob-observability.py --check     refuse if the ledger no longer covers
#                                         every mob-bearing check
#     ci/mob-observability.py --ledger    rewrite ci/checks/mob-blind.toml
#
# `--check` is the gating form and needs no recording, no server and no lock.
# ===== end interface =====

# # THE TRAP THIS EXISTS TO NAME

`710-a-daylight-detector-...` is this project's standing example of a check
that ran while the code path did not: it clicks a daylight detector on a
superflat where vanilla measures `power=0` either way, so a server that never
recomputes `power` agrees with one that does. The check is green for a reason
that has nothing to do with the mechanic in its name.

A mob check has a second way to be that, and it is worse because it looks like
coverage. `080-spawn-egg-makes-a-mob` observes exactly

    observe = ["add_entity", "set_entity_data"]

which is the mob's **announcement**. Everything the mob then *does* —
`move_entity`, `entity_position`, `set_entity_motion`, `rotate_head`,
`move_entity_rot` — is outside the projection and is discarded before the two
servers are compared. So the check asks "did a pig appear" and never asks "did
it do anything", while `entities.rs:19`, the one entity-behaviour gap the
crate's marker set admits, is about the second question and not the first.

Measured over the 17 distinct recordings on this box, and the numbers are in
the ledger: the count of observations the check COMPARES is **6 in every run**,
and the count vanilla EMITS ranges from **29 to 150** across 15 distinct
values. The check is stable precisely because it cannot see the mob.

# # WHY THE LEDGER IS A FILE AND NOT A PRINTOUT

The measurement needs recordings, and recordings are not in the tree — so a
gate that measured would be a gate that could not run. The split is the same
one `ci/checks/gameplay-floor.toml` makes: **measure rarely, into a file with
its N and its provenance; gate on the file's completeness every time.**

What `--check` refuses is a mob-bearing check the ledger does not cover. That
is the state that would let someone add `081-a-zombie-does-something`, observe
two announcement kinds, watch it go green and report mob parity.
"""

import argparse
import glob
import hashlib
import json
import os
import re
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
CHECKS = os.path.join(HERE, "checks", "gameplay")
LEDGER = os.path.join(HERE, "checks", "mob-blind.toml")

# The wire channels a mob's BEHAVIOUR travels on, as against its announcement.
# Derived from what vanilla actually emitted for the one mob scenario in the
# corpus, not from a guess about which packets sound behavioural: every name
# below appears in `vanilla.json` for `spawn-egg-makes-a-mob` on this box.
BEHAVIOUR = {
    "move_entity",
    "move_entity_rot",
    "move_entity_heartbeat",
    "entity_position",
    "set_entity_motion",
    "rotate_head",
}
# The announcement. A mob that is spawned and never ticked emits exactly these.
ANNOUNCEMENT = {"add_entity", "set_entity_data", "update_attributes",
                "remove_entities"}

# How a scenario can put a mob in the world. Both forms are mechanical: a
# spawn egg held or given, or a `summon`. Nothing else in the corpus's step
# vocabulary creates an entity that is not a player or an item.
#
# **Except a summoned experience orb, which is not a mob** (2026-10-01,
# `xp-orbs`): `ExperienceOrb extends Entity` directly — no `LivingEntity`, no
# AI, no goal — so nothing this ledger measures (behaviour kinds a mob emits
# when it ticks) exists for it. `682`-`684` summon one; `ci/checks/
# entity-reach.toml` carries them as `route = "summon"` instead. The exclusion
# names the one type and nothing else, so `summon minecraft:pig` still trips.
MOB_MAKERS = (re.compile(r'"minecraft:[a-z_]+_spawn_egg"'),
              re.compile(r'\bsummon\b(?!\s+(?:minecraft:)?experience_orb\b)'))


def scenarios():
    """(name, file, observe list, makes_a_mob) for every loaded check."""
    out = []
    for p in sorted(glob.glob(os.path.join(CHECKS, "*.toml"))):
        src = open(p).read()
        m = re.search(r'^name\s*=\s*"([^"]+)"', src, re.M)
        if not m:
            continue
        o = re.search(r'^observe\s*=\s*\[([^\]]*)\]', src, re.M)
        obs = re.findall(r'"([^"]+)"', o.group(1)) if o else []
        mob = any(r.search(src) for r in MOB_MAKERS)
        out.append((m.group(1), os.path.basename(p), obs, mob))
    return out


def recordings():
    return sorted(glob.glob(
        "/home/sayer/rustmc*/oracle-work/gameplay*/vanilla.json"))


# ---------------------------------------------------------- DE-DUPLICATION
# **Counting files here overcounts, and on this box it overcounts by design.**
#
# 21 `vanilla.json` files under `~/rustmc*/oracle-work/gameplay*/` carry
# `spawn-egg-makes-a-mob`. They are **17 distinct recordings**. Three separate
# causes, and only the first is an accident:
#
#   * `~/rustmc-refcache-bw-A` and `-D` hold the same reference because the
#     **reference cache served it to both**. That is the cache working. It also
#     means the population of files on this box will keep drifting away from
#     the population of runs, in one direction, for as long as the cache is on;
#   * `~/rustmc`, `~/rustmc-owcensus` and `~/rustmc-proj2` hold one older
#     recording between them, copied when the trees were made;
#   * `~/rustmc-cov4` and `~/rustmc-hold2` likewise.
#
# An N quoted off the file count is therefore not a run count, and the error is
# not small: 21 against 17 is 24% high. Every figure this file reports is keyed
# on the sha256 of the scenario's own recorded block, so a run counts once
# however many trees hold it.
def dedup(rows, key):
    """One row per distinct recording, first tree wins, plus the file count."""
    seen = {}
    for r in rows:
        h = hashlib.sha256(
            json.dumps(key(r), sort_keys=True).encode()).hexdigest()
        seen.setdefault(h, r)
    return list(seen.values()), len(rows)


def verdicts(name):
    """Every report.json on this box that carries `name`, with its verdicts.

    Deliberately NOT de-duplicated, unlike `measure`. The reference cache
    serves `vanilla.json` between trees; `rustmc.json` is built and run per
    tree every time. Two reports sharing one reference are still two
    independent readings of the subject, and a verdict is a statement about
    the subject.

    The declared verdict is under the check's own `observe`; `strict` is the
    same comparison with no `observe` filter. Reading both is what separates
    "the check is red about the announcement" from "the check is red about the
    behaviour", and the audit's reach table does not distinguish them.
    """
    rows = []
    for f in sorted(glob.glob(
            "/home/sayer/rustmc*/oracle-work/gameplay*/report.json")):
        try:
            d = json.load(open(f))
        except Exception:
            continue
        o = [x for x in d.get("outcomes", []) if x.get("name") == name]
        if not o:
            continue
        x = o[0]
        rows.append({
            "tree": f.split("/")[3],
            "declared": x.get("verdict"),
            "strict": x.get("strict"),
            "reference_obs": x.get("reference_obs"),
            "subject_obs": x.get("subject_obs"),
            "missing_kinds": x.get("missing_kinds") or [],
        })
    return rows


def measure(name, observe):
    """Per-run (total obs, obs inside `observe`) for one scenario, over N runs."""
    rows = []
    for f in recordings():
        try:
            d = json.load(open(f))
        except Exception:
            continue
        s = [x for x in d.get("scenarios", []) if x.get("name") == name]
        if not s:
            continue
        obs = s[0].get("obs", [])
        c = Counter(o.get("kind") for o in obs)
        rows.append({
            "tree": f.split("/")[3],
            "total": len(obs),
            "inside": sum(v for k, v in c.items() if k in observe),
            "behaviour": sum(v for k, v in c.items() if k in BEHAVIOUR),
            "kinds": dict(c),
            "_block": s[0],
        })
    rows, files = dedup(rows, lambda r: r["_block"])
    for r in rows:
        del r["_block"]
    if rows:
        rows[0]["_files"] = files
    return rows


def report(measure_too):
    sc = scenarios()
    mobs = [s for s in sc if s[3]]
    print("== the corpus ==")
    print("  checks loaded                       %3d" % len(sc))
    print("  checks that can put a mob in the world %d   %s"
          % (len(mobs), ", ".join(s[0] for s in mobs) or "(none)"))
    beh = [s for s in sc if set(s[2]) & BEHAVIOUR]
    print("  checks observing a behaviour channel  %2d" % len(beh))
    both = [s for s in mobs if set(s[2]) & BEHAVIOUR]
    print("  checks that are BOTH                  %2d   %s"
          % (len(both), ", ".join(s[0] for s in both) or "(none)"))
    print()
    for name, f, obs, _ in mobs:
        print("== %s ==" % name)
        print("  file      %s" % f)
        print("  observe   %s" % (", ".join(obs) or "(nothing)"))
        print("  of which  announcement %d, behaviour %d"
              % (len(set(obs) & ANNOUNCEMENT), len(set(obs) & BEHAVIOUR)))
        if not measure_too:
            continue
        rows = measure(name, set(obs))
        if not rows:
            print("  MEASURED  no recording on this box carries the scenario")
            continue
        files = rows[0].pop("_files", len(rows))
        tot = [r["total"] for r in rows]
        ins = [r["inside"] for r in rows]
        bhv = [r["behaviour"] for r in rows]
        print("  MEASURED  N = %d DISTINCT vanilla recordings, from %d files"
              % (len(rows), files))
        if files != len(rows):
            print("            (%d files are duplicates — the reference cache"
                  " serves one" % (files - len(rows)))
            print("             recording to several trees, and copied trees"
                  " carry older ones.")
            print("             Counting files here would overcount N by"
                  " %.0f%%.)" % (100.0 * files / len(rows) - 100))
        print("    vanilla emitted    %d..%d observations (%d distinct counts)"
              % (min(tot), max(tot), len(set(tot))))
        print("    the check compares %d..%d" % (min(ins), max(ins)))
        print("    of vanilla's, behaviour channels: %d..%d" % (min(bhv), max(bhv)))
        agg = Counter()
        runs = Counter()
        for r in rows:
            for k, v in r["kinds"].items():
                agg[k] += v
                runs[k] += 1
        print("    %-24s %6s %6s  %s" % ("kind", "total", "runs", "compared?"))
        for k, v in sorted(agg.items(), key=lambda x: -x[1]):
            tag = "compared" if k in set(obs) else "DROPPED"
            print("    %-24s %6d %4d/%d  %s" % (k, v, runs[k], len(rows), tag))
        vr = verdicts(name)
        if vr:
            dv = Counter(r["declared"] for r in vr)
            sv = Counter(r["strict"] for r in vr)
            ro = sorted({r["reference_obs"] for r in vr})
            so = sorted({r["subject_obs"] for r in vr})
            mk = Counter(tuple(sorted(r["missing_kinds"])) for r in vr)
            # NOT de-duplicated, and the asymmetry is the reason. The
            # reference cache serves `vanilla.json`; `rustmc.json` is produced
            # by each tree's own build every time. So two reports can share a
            # reference and still be two independent measurements of the
            # SUBJECT, which is what a verdict is about.
            print("  VERDICTS  N = %d reports, %d trees (not de-duplicated: the"
                  % (len(vr), len({r["tree"] for r in vr})))
            print("            cache serves the reference, never the subject)")
            print("    declared %s" % dict(dv))
            print("    strict   %s" % dict(sv))
            print("    reference_obs %s   subject_obs %s" % (ro, so))
            for k, v in mk.most_common():
                print("    missing kinds in %d/%d runs: %s"
                      % (v, len(vr), ", ".join(k) or "(none)"))
            print("    -> what is red here is the ANNOUNCEMENT. The behaviour")
            print("       channels are not in `observe`, so no verdict on this")
            print("       check is about per-kind behaviour at all.")
    return 0


def write_ledger():
    sc = scenarios()
    mobs = [s for s in sc if s[3]]
    lines = [
        "# ci/checks/mob-blind.toml — what each mob-bearing check can and",
        "# cannot see, measured, with its N.",
        "#",
        "# Written by `ci/mob-observability.py --ledger`. `--check` refuses if a",
        "# mob-bearing check in ci/checks/gameplay/ has no entry here, which is",
        "# the state that would let a new mob check observe two announcement",
        "# kinds, go green, and be read as mob parity.",
        "#",
        "# `runs` is DISTINCT recordings and `files` is how many files hold",
        "# them. They differ because the reference cache serves one recording",
        "# to several trees and copied trees carry older ones. Quote `runs`.",
        "#",
        "# `compared` is stable and `emitted` is not, and that IS the finding:",
        "# the check is stable because it cannot see the mob. Do not read the",
        "# emitted range as a divergence — it is one server disagreeing with",
        "# itself across runs, which is why `spawn-egg-makes-a-mob` sits in",
        "# ci/checks/gameplay-floor.toml as well.",
        "",
    ]
    for name, f, obs, _ in mobs:
        rows = measure(name, set(obs))
        files = rows[0].pop("_files", len(rows)) if rows else 0
        tot = [r["total"] for r in rows] or [0]
        ins = [r["inside"] for r in rows] or [0]
        bhv = [r["behaviour"] for r in rows] or [0]
        lines += [
            "[[blind]]",
            'name = "%s"' % name,
            'file = "%s"' % f,
            "observe = [%s]" % ", ".join('"%s"' % k for k in obs),
            "observes_behaviour = %s" % ("true" if set(obs) & BEHAVIOUR else "false"),
            "runs = %d   # DISTINCT recordings" % len(rows),
            "files = %d  # vanilla.json files carrying the scenario" % files,
            "compared = [%d, %d]" % (min(ins), max(ins)),
            "emitted = [%d, %d]" % (min(tot), max(tot)),
            "emitted_distinct = %d" % len(set(tot)),
            "behaviour_emitted = [%d, %d]" % (min(bhv), max(bhv)),
            'measured_on = "sayer-desktop, every oracle-work/gameplay*/vanilla.json"',
        ]
        vr = verdicts(name)
        if vr:
            dv = Counter(r["declared"] for r in vr)
            sv = Counter(r["strict"] for r in vr)
            lines += [
                "reports = %d" % len(vr),
                "declared_verdicts = { %s }" % ", ".join(
                    '%s = %d' % (k, v) for k, v in sorted(dv.items())),
                "strict_verdicts = { %s }" % ", ".join(
                    '%s = %d' % (k, v) for k, v in sorted(sv.items())),
                "reference_obs = %s" % sorted({r["reference_obs"] for r in vr}),
                "subject_obs = %s" % sorted({r["subject_obs"] for r in vr}),
                "# What is red is the ANNOUNCEMENT. `observe` carries no",
                "# behaviour channel, so no verdict on this check is about",
                "# per-kind behaviour, which is what entities.rs:19 names.",
            ]
        lines += [
            "",
        ]
    open(LEDGER, "w").write("\n".join(lines))
    print("wrote %s — %d mob-bearing check(s)" % (LEDGER, len(mobs)))
    return 0


def check():
    sc = scenarios()
    mobs = {s[0] for s in sc if s[3]}
    if not os.path.exists(LEDGER):
        print("mob-observability --check: no ledger at %s" % LEDGER,
              file=sys.stderr)
        return 2
    src = open(LEDGER).read()
    have = set(re.findall(r'^name\s*=\s*"([^"]+)"', src, re.M))
    missing = sorted(mobs - have)
    stale = sorted(have - mobs)
    if missing:
        print("mob-observability --check: REFUSED — %d mob-bearing check(s) "
              "with no ledger entry:" % len(missing), file=sys.stderr)
        for n in missing:
            print("    %s" % n, file=sys.stderr)
        print("  A check that puts a mob in the world and is not in the ledger"
              " has never had", file=sys.stderr)
        print("  its projection measured. Run --ledger and read what it says"
              " before trusting", file=sys.stderr)
        print("  the check's verdict.", file=sys.stderr)
        return 1
    if stale:
        print("mob-observability --check: REFUSED — ledger names %d check(s) "
              "the corpus no longer has:" % len(stale), file=sys.stderr)
        for n in stale:
            print("    %s" % n, file=sys.stderr)
        return 1
    print("mob-observability --check: %d mob-bearing check(s), all in the ledger"
          % len(mobs))
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--measure", action="store_true")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--ledger", action="store_true")
    a = ap.parse_args()
    if a.check:
        return check()
    if a.ledger:
        return write_ledger()
    return report(a.measure)


if __name__ == "__main__":
    sys.exit(main())
