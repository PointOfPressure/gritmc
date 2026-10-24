#!/usr/bin/env python3
"""mob-projection-search.py — is there a projection of the mob check vanilla agrees with itself on?

# ===== AGENT INTERFACE =====
#
#     ci/mob-projection-search.py           the search, over the recordings on this box
#     ci/mob-projection-search.py --check   refuse if the ledger's answer has moved,
#                                           over the COMMITTED corpus, never the box
#     ci/mob-projection-search.py --extract re-declare that corpus from the box
#
# Reads recordings that already exist. Takes no lock, boots nothing.
# ===== end interface =====

# # WHY THIS AND NOT ANOTHER RUN

`ci/mob-observability.py` establishes that `080-spawn-egg-makes-a-mob` compares
6 observations of the 29..150 vanilla sends and carries no behaviour channel.
The obvious next move — widen `observe` — is the one that has to be measured
before it is tried, because `ci/checks/gameplay-floor.toml` already records the
scenario flapping 15/15 pairs, and a wider projection of an unstable scenario
is a more unstable scenario.

This searches the recordings already on disk instead. Every candidate
projection is applied to each recording and the runs are compared **pairwise**,
so the output is "vanilla agreed with itself in n of N-choose-2 pairs" rather
than a verdict against our server. No server is involved at all.

# # TWO METHOD POINTS THAT CHANGED THE ANSWER

* **De-duplicate before counting.** 21 `vanilla.json` files on this box carry
  the scenario and they are 17 distinct recordings — the reference cache serves
  one recording to several trees, and copied trees carry older ones. Counting
  files inflates N by 24%, in one direction, systematically.
* **Hold the recorder schema constant.** One of the 17 was taken by a decoder
  that emits `add_entity` without `data` or `x_rot`. Mixed in, it makes every
  projection look 74% stable when the truth is 100% or 0%. It is excluded and
  the exclusion is reported, not silent.

Sixteen distinct recordings, one schema, **120 pairs**.

# # WHAT THE SEARCH FOUND

    observe as declared                          0/120     0.0%
    observe, minus y_rot + y_head_rot           34/120    28.3%
    observe, minus y_rot + y_head_rot + rest   120/120   100.0%
    {add_entity} alone                           1/120     0.8%
    {add_entity} minus y_rot + y_head_rot      120/120   100.0%
    {set_entity_data} alone                     34/120    28.3%

**`080`'s projection as declared is 0 of 120.** Not "mostly stable" — vanilla
never once agreed with itself over the whole population. And **`add_entity`
minus the two yaw fields is 120 of 120**: everything else about the
announcement — the position, the kind id, `data`, `x_rot`, the encoded
remainder — is exact across sixteen cold servers.

So the mob announcement is measurable, and exactly two fields stand in the way.

# # EVERY UNSEEDED DRAW A SPAWNED PIG MAKES, AND WHICH ONES REACH A CLIENT

Read from the pinned jar, then checked against the recordings — in that order,
because the first three of these were inferred from bytecode and **the fourth
inference was wrong**, which is the useful part.

| draw | call site | packet | on the wire? |
|---|---|---|---|
| spawn yaw | `EntityType.create` @103, `nextFloat() * 360.0f` | `add_entity` | **yes**, and it is the whole of that packet's instability |
| sound variant | `Pig.finalizeSpawn` -> `PigSoundVariants.pickRandomSoundVariant` -> `Registry.getRandom` | `set_entity_data` | **yes** |
| left-handed | `Mob.finalizeSpawn` -> `nextFloat()` -> `setLeftHanded` | `set_entity_data` | **yes** |
| age | `AgeableMob.finalizeSpawn` -> `nextFloat()` -> `setAge` | `set_entity_data` | **yes** |
| follow-range bonus | `Mob.finalizeSpawn` -> `RandomSource.triangle(0.0, 0.11228)` on `FOLLOW_RANGE` | `update_attributes` | **NO** |

**The last row is the one worth the space.** A continuous `triangle` draw into
an attribute modifier looks like the worst possible case — a real-valued
divergence on a packet the corpus already sees — and it is not a case at all.
`update_attributes` is **120/120 identical** across the same sixteen runs.
The jar says why: `Attributes`' static initialiser registers `FLYING_SPEED`
through `RangedAttribute.setSyncable(true)` at offsets 420-424 and registers
`FOLLOW_RANGE` at 430-451 **with no `setSyncable` call at all**, so it keeps
the default and `Attribute.isClientSyncable()` is false. The draw happens on
every spawn and no client is ever told.

A divergence in the server is not a divergence on the wire, and only the
measurement could tell which this was. The bytecode said "unseeded draw"; the
recordings said "exact in every pair"; the bytecode then said why.

The three that DO reach `set_entity_data` are consistent with what is measured
there — 5 distinct payloads over 16 runs at 8/3/3/1/1 reads as three sound
variants with two rarer left-handed variants beside them, `setLeftHanded` being
a 5% draw whose expectation over 16 runs is 0.8. Consistent with, not proven:
the recorded field is a digest, so the decomposition is inference and is
labelled as such.

# # THE SECOND DRAW, WHICH NOBODY HAD NAMED

The yaw is known: `EntityType.create` offset 103, `getRandom().nextFloat()`
times `360.0f`, into `Entity.snapTo`. It rides on `add_entity` and it is why
the floor file, the `spawn_mobs` null and `080` all flap.

`set_entity_data` is **a second, independent one**, and dropping the yaw does
not touch it — that is what the 28.3% row means. Read out of the bytecode:

    Pig.finalizeSpawn -> PigSoundVariants.pickRandomSoundVariant(
                             registryAccess(), level.getRandom())
                      -> Registry.getRandom(RandomSource)
                      -> setSoundVariant(..)

a uniform pick over the `minecraft:pig_sound_variant` registry, which the jar
ships with three entries: `big`, `classic`, `mini`. It lands in synched data
and therefore on `set_entity_data`.

**It does not fully account for what is measured, and this file says so rather
than rounding.** The payload takes **5 distinct values over 16 runs**, at
8/3/3/1/1, and a uniform 1-of-3 cannot produce five. At least one further
source is in that blob and identifying it is open work — `Animal.finalizeSpawn`
and `VariantUtils.selectVariantToSpawn` are where to look first, the latter
being biome-keyed and therefore expected to be constant on a superflat.

# # WHAT THIS DOES NOT SAY

It does not say a field exemption is the right fix. `origin/entity-parity`
ruled out a type-keyed yaw exemption twice, and the reason survives: a pig's
yaw is random only on the `EntityType.create` path, and the wire does not carry
which path spawned it — so an exemption blinds every other path too. What this
adds is the cost side of that decision, measured: exempting `y_rot` and
`y_head_rot` on `add_entity` takes the announcement from 0/120 to 120/120, and
without a second decision about `set_entity_data` it takes `080` to 28.3% and
no further.
"""

import argparse
import glob
import hashlib
import itertools
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
LEDGER = os.path.join(HERE, "checks", "mob-projection.json")
SCENARIO = "spawn-egg-makes-a-mob"
# THE GATE READS A COMMITTED CORPUS, NOT THE BOX (gate-pipeline, 2026-09-30).
# `--check` used to search every tree's recordings at gate time, so its verdict
# for the tree under test moved with what OTHER lanes had recorded — on 23-24
# Sep a recorder change in one tree reddened every T0 on the box, main's
# included (see the comment in load()). `--check` now reads CORPUS: the
# de-duplicated recordings of SCENARIO, reduced to the kinds CANDIDATES
# project, as they stood at the last `--extract`. The ad-hoc search still
# reads the box.
CORPUS = os.path.join(HERE, "checks", "mob-projection-corpus.json")
BOX_GLOB = "/home/sayer/rustmc*/oracle-work/gameplay*/vanilla.json"

CANDIDATES = [
    ("observe as declared", ("add_entity", "set_entity_data"), ()),
    ("observe, minus y_rot + y_head_rot", ("add_entity", "set_entity_data"),
     ("y_rot", "y_head_rot")),
    ("observe, minus y_rot + y_head_rot + rest",
     ("add_entity", "set_entity_data"), ("y_rot", "y_head_rot", "rest")),
    ("{add_entity} alone", ("add_entity",), ()),
    ("{add_entity} minus y_rot + y_head_rot", ("add_entity",),
     ("y_rot", "y_head_rot")),
    ("{set_entity_data} alone", ("set_entity_data",), ()),
    ("{update_attributes} alone", ("update_attributes",), ()),
]


def load():
    """Distinct recordings of the scenario, split by recorder schema."""
    seen = {}
    files = 0
    for f in sorted(glob.glob(BOX_GLOB)):
        try:
            d = json.load(open(f))
        except Exception:
            continue
        s = [x for x in d.get("scenarios", []) if x.get("name") == SCENARIO]
        if not s:
            continue
        files += 1
        h = hashlib.sha256(json.dumps(s[0], sort_keys=True).encode()).hexdigest()
        if h in seen:
            continue
        ae = [o for o in s[0]["obs"] if o["kind"] == "add_entity"]
        # The schema is the recorder's add_entity FIELD VOCABULARY, not one
        # field's presence. Until 2026-09-24 it was `"data" in fields`, which
        # split the one older decoder off and then compared every later one
        # against every other. The recorder on main 26ffd4965 decodes
        # `movement` as well, so the first recording it made on this box
        # (~/rustmc-stack) disagreed with all 33 others on the field NAMES,
        # and `{add_entity} minus y_rot + y_head_rot` went from 120/120 to
        # 528/561: a decoder difference read as vanilla disagreeing with
        # itself, reddening every T0 on this box including main's. Pairs are
        # now taken within one vocabulary only, which is the rule the "older"
        # split already applied, generalised.
        schema = (tuple(sorted(ae[0]["fields"])) if ae and "data" in ae[0]["fields"]
                  else "older")
        seen[h] = (f.split("/")[3], schema, s[0]["obs"])
    rows = list(seen.values())
    return rows, files


def project(obs, kinds, drop):
    return [(o["who"], o["kind"],
             tuple(sorted((k, json.dumps(v, sort_keys=True))
                          for k, v in o["fields"].items() if k not in drop)))
            for o in obs if o["kind"] in kinds]


def agree(runs, kinds, drop):
    ps = [project(r, kinds, drop) for r in runs]
    total = same = 0
    for a, b in itertools.combinations(ps, 2):
        total += 1
        same += (a == b)
    return same, total


def kinds_projected():
    k = set()
    for _label, kinds, _drop in CANDIDATES:
        k.update(kinds)
    return sorted(k)


def load_corpus():
    """The committed corpus as load() rows, or None if the file is absent."""
    if not os.path.exists(CORPUS):
        return None
    d = json.load(open(CORPUS))
    rows = [(r["tree"], r["schema"] if r["schema"] == "older" else tuple(r["schema"]),
             r["obs"]) for r in d["recordings"]]
    return rows, d["files"]


def write_extract():
    rows, files = load()
    keep = set(kinds_projected())
    recs = [{"tree": t, "schema": sch if sch == "older" else list(sch),
             "obs": [o for o in obs if o["kind"] in keep]} for t, sch, obs in rows]
    doc = {"_about": ("What ci/mob-projection-search.py --check searches: the "
                      "distinct recordings of %s on the box at --extract time, "
                      "reduced to the kinds CANDIDATES project (%s). Tracked so "
                      "that another lane's recording cannot move this tree's "
                      "verdict; re-declare with --extract in a reviewed commit."
                      % (SCENARIO, ", ".join(sorted(keep)))),
           "files": files, "recordings": recs}
    json.dump(doc, open(CORPUS, "w"), sort_keys=True, separators=(",", ":"))
    open(CORPUS, "a").write("\n")
    print("wrote %s — %d distinct recording(s) from %d file(s)" % (CORPUS, len(recs), files))
    return 0


def figures(source=None):
    rows, files = source if source is not None else load()
    groups = {}
    for r in rows:
        if r[1] != "older":
            groups.setdefault(r[1], []).append(r[2])
    cur = [g for g in groups.values()]
    n_cur = sum(len(g) for g in cur)
    out = {
        "files": files,
        "distinct_recordings": len(rows),
        "current_schema_recordings": n_cur,
        "older_schema_excluded": len(rows) - n_cur,
        "recorder_vocabularies": len(cur),
        "pairs": sum(len(g) * (len(g) - 1) // 2 for g in cur),
        "results": {},
    }
    for label, kinds, drop in CANDIDATES:
        s = t = 0
        for g in cur:
            gs, gt = agree(g, kinds, drop)
            s += gs
            t += gt
        out["results"][label] = [s, t]
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--extract", action="store_true")
    a = ap.parse_args()
    if a.extract:
        return write_extract()

    if a.check:
        corpus = load_corpus()
        if corpus is None:
            print("mob-projection-search --check: REFUSED — no committed corpus"
                  " at %s. It is a tracked file; its absence is a broken tree,"
                  " not a fact about this box. Rebuild with --extract where the"
                  " recordings are and commit it." % CORPUS, file=sys.stderr)
            return 2
        fig = figures(corpus)
    else:
        fig = figures()

    # (The "NOT RE-MEASURED" exit for a box with no recordings is gone: --check
    # no longer reads the box, so it measures the same corpus on every machine,
    # and the absent-corpus case above is a refusal.)

    # The control. A search whose population is one recording compares nothing
    # and every projection comes back 0/0, which prints as a table of zeros and
    # reads like a finding.
    if fig["current_schema_recordings"] < 5 or fig["pairs"] < 10:
        print("mob-projection-search: REFUSED — not enough to compare",
              file=sys.stderr)
        print("    recordings %d, pairs %d"
              % (fig["current_schema_recordings"], fig["pairs"]), file=sys.stderr)
        print("  A pairwise search over fewer than five runs produces a table"
              " of zeros", file=sys.stderr)
        print("  that reads exactly like a finding. Nothing printed.",
              file=sys.stderr)
        return 3

    if a.json:
        print(json.dumps(fig, indent=1, sort_keys=True))
        return 0

    if a.check:
        if not os.path.exists(LEDGER):
            print("mob-projection-search --check: no ledger at %s" % LEDGER,
                  file=sys.stderr)
            return 2
        want = json.load(open(LEDGER))["results"]
        moved = []
        for k, v in sorted(want.items()):
            got = fig["results"].get(k)
            if got is None:
                moved.append("%s: gone" % k)
            elif (got[0] == 0) != (v[0] == 0) or (got[0] == got[1]) != (v[0] == v[1]):
                # Gate the SHAPE — never, sometimes, always — and not the
                # fraction. The fraction moves whenever a tree is made or
                # deleted on this box, and a gate that reddens on housekeeping
                # is a gate somebody turns off. The shape is the claim.
                moved.append("%s: was %d/%d, now %d/%d"
                             % (k, v[0], v[1], got[0], got[1]))
        if moved:
            print("mob-projection-search --check: REFUSED — a projection's"
                  " shape changed", file=sys.stderr)
            for m in moved:
                print("    %s" % m, file=sys.stderr)
            return 1
        print("mob-projection-search --check: %d projections, shapes unchanged,"
              " %d pairs" % (len(want), fig["pairs"]))
        return 0

    print("== population ==")
    print("  vanilla.json files carrying the scenario   %3d" % fig["files"])
    print("  distinct recordings                        %3d" % fig["distinct_recordings"])
    print("  on the current recorder schema             %3d" % fig["current_schema_recordings"])
    print("  excluded, older decoder                    %3d" % fig["older_schema_excluded"])
    print("  pairs compared                             %3d" % fig["pairs"])
    print()
    print("== does vanilla agree with ITSELF under this projection? ==")
    for label, _, _ in CANDIDATES:
        s, t = fig["results"][label]
        print("  %-42s %4d/%-4d %6.1f%%" % (label, s, t, 100.0 * s / t if t else 0))
    print()
    print("  `080`'s projection as declared is 0 of %d. Not mostly unstable —" % fig["pairs"])
    print("  vanilla never once agreed with itself. And `add_entity` minus the")
    print("  two yaw fields is exact across every pair, so everything else about")
    print("  the announcement already matches: position, kind id, data, x_rot,")
    print("  and the encoded remainder.")
    print()
    print("  Two unseeded draws stand in the way, not one:")
    print("    add_entity      EntityType.create@103, nextFloat()*360 -> snapTo")
    print("    set_entity_data Pig.finalizeSpawn -> PigSoundVariants")
    print("                    .pickRandomSoundVariant -> Registry.getRandom,")
    print("                    uniform over 3 entries (big, classic, mini)")
    print("  The second does NOT fully account for what is measured — the")
    print("  payload takes 5 distinct values over 16 runs and a 1-of-3 draw")
    print("  cannot produce five. At least one further source is open.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
