#!/usr/bin/env python3
"""The structure surface, enumerated from the jar and only then intersected
with ours.

# ===== AGENT INTERFACE - this is all you need to read =====
#
#     ci/structure-surface.py                 the whole surface, four tables
#     ci/structure-surface.py --sets          the 20 structure sets
#     ci/structure-surface.py --elements      pool element types, and ours
#     ci/structure-surface.py --features      feature_pool_element -> feature
#     ci/structure-surface.py --arms          jar feature types with no arm
#     ci/structure-surface.py --processors    processor + predicate types
#     ci/structure-surface.py --reach         unreachable types a biome DOES
#                                             reach, which is the ranked hole
#     ci/structure-surface.py --dispatch      which of the 20 sets we place
#     ci/structure-surface.py --fixtures      structure probes, and whether
#                                             their fixture exists on this box
#     ci/structure-surface.py --check         exit 1 if a table drifted
#     ci/structure-surface.py --selftest      the positive controls
#
# WHAT BREAKS IF YOU IGNORE THIS: you enumerate from `data/vanilla/` or from
# our own registry, both of which are things we wrote, and the census agrees
# with the code because it was derived from it. That closed loop has cost this
# project twice. Every list here starts at `oracle-work/.../generated-data`,
# which is Mojang's own datagen output, and our side is only ever the
# *intersection* - never the enumeration.
# ===== end interface; everything below is why =====

# # WHY A TOOL AND NOT A DOCUMENT
#
# The counts in docs/PARITY.md that this replaces were each produced by a
# one-off shell pipeline, quoted into prose, and then re-quoted from the prose.
# Three of them were wrong by the time they were re-quoted and nothing could
# tell, because re-deriving them meant reconstructing the pipeline. `--check`
# re-derives every number in one call and exits non-zero on drift, so a stale
# figure is a red gate rather than a paragraph somebody has to doubt.
#
# # WHY THE ARM TABLE USES A REAL PARSE AND NOT A GREP
#
# The first version of `--arms` grepped `"minecraft:x" =>` out of
# configured.rs and reported 20 unhandled types. Two of those 20 were handled
# by an alternation arm - `"minecraft:vegetation_patch" |
# "minecraft:waterlogged_vegetation_patch" =>` - which the grep could not see,
# so the tool over-reported the gap by exactly the arms that were written
# carefully. The pattern below matches the whole alternation. That is the
# difference between 19 and 20, and the 20 was wrong.
"""

import json
import glob
import os
import re
import sys
import collections

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
JAR = os.path.join(
    ROOT, "oracle-work/vanilla-26.2/generated-data/data/minecraft"
)
CONFIGURED_RS = os.path.join(
    ROOT, "crates/rustmc-worldgen/src/feature/configured.rs"
)
GENERATE_RS = os.path.join(ROOT, "tools/worldgen-assembly/src/generate.rs")
PLACE_RS = os.path.join(ROOT, "crates/rustmc-worldgen/src/structure/place.rs")

# The figures this file asserts. `--check` re-derives and compares.
EXPECT = {
    "sets": 20,
    "structure_entries": 34,
    "template_pools": 188,
    # 1198 TOP-LEVEL elements. A grep for `"element_type"` answers 1204,
    # because the 3 `list_pool_element`s nest their own elements and the grep
    # counts those too. 1198 is also the figure jigsaw.rs already carries
    # ("375 of the 1198 pool entries write their list inline"), so the two
    # independent counts agree and the grep is the one that was wrong.
    "elements_total": 1198,
    "feature_elements": 36,
    "feature_pools": 14,
    # 18 and 31 WITH the block_pile port. Without it they are 19 and 36:
    # block_pile is 5 configured_features and is the single feature type
    # standing behind all 36 feature_pool_elements (see --features).
    "processor_lists": 40,
    # 0 on `structures-live` (2026-10-01): `capped` and `block_rot` were the
    # two, ported for trail ruins and outposts.
    "processor_types_missing": 0,
    # 0 with the `axis_aligned_linear_pos` port. It was 1, and that 1 was the
    # whole of the bastion's processor gap: `Rule::parse` refused any rule
    # carrying a `position_predicate`, and a refused rule refuses its list.
    "predicate_types_missing": 0,
    # Exactly one `position_predicate` exists in the whole of 26.2, one pool
    # element names the list that holds it, and that element is under
    # `bastion/`. The third figure is what makes `PieceOrigin::unthreaded`
    # falsifiable: if a future datapack puts a live position predicate on a
    # list some other structure resolves, this goes non-zero and the four
    # unthreaded call sites are wrong.
    "position_predicates": 1,
    "position_predicate_pools": 1,
    "position_predicate_pools_outside_bastion": 0,
    # The four properties the bastion port rests on, as ONE type, ONE
    # projection and a list count. `bastion_element_types` going to 2 means a
    # pool gained a `feature_pool_element` or a `list_pool_element` and the
    # port will silently refuse a piece; `bastion_projections` going to 2
    # means one gained `terrain_matching` and now needs a real height source.
    # Both read in a block diff as a hole in a rampart and in no gate at all.
    "bastion_pools": 60,
    "bastion_elements": 176,
    "bastion_element_types": 1,
    "bastion_projections": 1,
    "bastion_processor_lists": 12,
    "reachable_cf": 196,
    "biome_reached_unported_types": 17,
    "biome_reached_unported_cf": 23,
    # 9 and 23 with `nether_complexes` dispatched. **23 is the count of
    # structures inside the sets we read, and it is one more than the number
    # we write blocks for.** `nether_complexes` holds two: `bastion_remnant`,
    # which now has a driver, and `fortress`, which is attempted so that the
    # weighted contention is right and is then never written. Naming that here
    # rather than fixing the number, because the figure this key measures IS
    # dispatch, and quietly subtracting one would make it measure neither.
    # 17 and 31 on `structures-live` (2026-10-01): buried_treasures, igloos,
    # desert_pyramids, jungle_temples, swamp_huts, pillager_outposts,
    # trail_ruins and strongholds are
    # read and written; ocean_ruins and shipwrecks were already counted whole
    # here while placing one of their two structures each, and now place both.
    # 18 and 32 on `structures-mansion`: woodland_mansions.
    "sets_placed": 20,
    "structures_placed": 34,
    "unreachable_types": 18,
    "unreachable_configured": 31,
}


def have_jar():
    return os.path.isdir(os.path.join(JAR, "worldgen/structure_set"))


def sets():
    out = []
    for p in sorted(glob.glob(JAR + "/worldgen/structure_set/*.json")):
        d = json.load(open(p))
        pl = d["placement"]
        out.append(
            {
                "name": os.path.basename(p)[:-5],
                "placement": pl["type"].split(":")[-1],
                "spacing": pl.get("spacing"),
                "separation": pl.get("separation"),
                "salt": pl.get("salt"),
                "frequency": pl.get("frequency"),
                "structures": [
                    e["structure"].split(":")[-1] for e in d["structures"]
                ],
            }
        )
    return out


def pool_elements():
    """Every element of every template pool, with its type and pool."""
    out = []
    for p in glob.glob(JAR + "/worldgen/template_pool/**/*.json", recursive=True):
        rel = os.path.relpath(p, JAR + "/worldgen/template_pool")
        d = json.load(open(p))
        for e in d.get("elements", []):
            el = e.get("element", {})
            out.append((rel, el.get("element_type"), el))
    return out


def our_arms():
    """Every `minecraft:` literal in match-arm position in configured.rs,
    including every branch of an alternation arm."""
    src = open(CONFIGURED_RS).read()
    arms = set()
    for m in re.finditer(
        r'((?:"minecraft:[a-z_]+"\s*\|\s*)*"minecraft:[a-z_]+")\s*=>', src
    ):
        arms.update(re.findall(r'"(minecraft:[a-z_]+)"', m.group(1)))
    return arms


def configured_types():
    c = collections.Counter()
    for p in glob.glob(JAR + "/worldgen/configured_feature/*.json"):
        c[json.load(open(p)).get("type")] += 1
    return c


def resolve_feature(name):
    """placed_feature -> configured_feature -> type. Returns (type, cf name)."""
    n = name.split(":")[-1]
    pf = f"{JAR}/worldgen/placed_feature/{n}.json"
    if not os.path.exists(pf):
        return ("NO-placed_feature", None)
    f = json.load(open(pf)).get("feature")
    if not isinstance(f, str):
        return ("inline", None)
    cf = f"{JAR}/worldgen/configured_feature/{f.split(':')[-1]}.json"
    if not os.path.exists(cf):
        return ("NO-configured_feature", f)
    return (json.load(open(cf)).get("type"), f)


def table_sets():
    print("== structure sets, from the jar ==")
    print(f"{'set':26s} {'placement':16s} {'geom':9s} {'salt':11s} structures")
    for s in sets():
        geom = (
            f"{s['spacing']}/{s['separation']}"
            if s["placement"] == "random_spread"
            else "rings"
        )
        extra = f"  freq={s['frequency']}" if s["frequency"] is not None else ""
        print(
            f"{s['name']:26s} {s['placement']:16s} {geom:9s} "
            f"{str(s['salt']):11s} {len(s['structures'])}: "
            f"{','.join(s['structures'])}{extra}"
        )
    n = sum(len(s["structures"]) for s in sets())
    print(f"\n{len(sets())} sets, {n} structure entries")


def table_elements():
    els = pool_elements()
    c = collections.Counter(t for _, t, _ in els)
    pools = len(set(r for r, _, _ in els))
    print("== template pool elements, from the jar ==")
    for t, n in c.most_common():
        print(f"{n:5d}  {t}")
    print(f"\n{sum(c.values())} elements across {pools} pools")


def table_features():
    els = [(r, e) for r, t, e in pool_elements()
           if t == "minecraft:feature_pool_element"]
    arms = our_arms()
    byfeat = collections.Counter()
    for _, e in els:
        f = e.get("feature")
        byfeat[f if isinstance(f, str) else "<inline>"] += 1
    print("== feature_pool_element -> placed_feature -> configured_feature ==")
    print(f"{'n':>3s}  {'feature':28s} {'configured type':26s} arm")
    blocked = collections.Counter()
    for f, n in byfeat.most_common():
        t, _ = resolve_feature(f)
        ok = t in arms
        if not ok:
            blocked[t] += n
        print(f"{n:3d}  {f.split(':')[-1]:28s} {str(t):26s} "
              f"{'yes' if ok else 'NO'}")
    print(f"\n{sum(byfeat.values())} feature elements across "
          f"{len(set(r for r, _ in els))} pools")
    if blocked:
        print("blocked by a missing feature arm:")
        for t, n in blocked.most_common():
            print(f"  {n:3d} elements  {t}")
    else:
        print("every feature element resolves to a feature we have an arm for")


def table_arms():
    jar = configured_types()
    arms = our_arms()
    missing = [(t, n) for t, n in jar.most_common() if t not in arms]
    print("== jar feature types with no arm in configured.rs ==")
    for t, n in missing:
        print(f"{n:4d}  {t}")
    print(f"\n{len(missing)} of {len(jar)} types unreachable, "
          f"covering {sum(n for _, n in missing)} of {sum(jar.values())} "
          f"configured_features")


# Every structure test that reads a fixture from outside the repo, with the
# path it reads and whether that path is regenerable. All three of the first
# group skip with an `eprintln!` and `return`, so they PASS while testing
# nothing, and none of their fixtures exists in any tree on this box.
FIXTURES = [
    ("probe_cluster_boxes",
     "oracle-work/van-ruin--58_28, oracle-work/van-ruin--57_42",
     "regenerable: a vanilla run at those two chunks"),
    ("probe_ruin_height",
     "/tmp/ruin-w/region",
     "DEAD: a scratch path under /tmp, not a durable reference"),
    ("golden_village_start",
     "oracle-work/van-village-31415926",
     "regenerable: worldgen-dump jigsaw-starts at seed 31415926"),
]


def parsed_processor_names():
    """The `"minecraft:x" =>` arms inside `Processor::parse` and
    `RuleTest::parse` in place.rs.

    Deliberately NOT the enum variants. `Processor::BlockAge` exists, is
    implemented in full, and has no arm -- it is constructed only from Rust,
    by ruined_portal.rs, so a `minecraft:block_rot` in a processor_list
    document cannot produce one. Counting variants would call that ported.
    What matters is whether the DATA can reach the code."""
    src = open(PLACE_RS).read()
    return set(re.findall(r'"(minecraft:[a-z_]+)"\s*=>', src))


def table_processors():
    ours = parsed_processor_names()
    pt, pred = collections.Counter(), collections.Counter()
    n = 0
    for p in glob.glob(JAR + "/worldgen/processor_list/*.json"):
        n += 1
        for pr in json.load(open(p)).get("processors", []):
            pt[pr.get("processor_type")] += 1
            for r in pr.get("rules", []):
                for slot in ("input_predicate", "location_predicate",
                             "position_predicate"):
                    if slot in r:
                        pred[r[slot].get("predicate_type")] += 1
    print(f"== {n} processor_list documents in the jar ==")
    print("processor types:")
    for t, c in pt.most_common():
        print(f"  {c:4d} {t:42s} {'ours' if t in ours else 'MISSING'}")
    print("rule predicate types:")
    for t, c in pred.most_common():
        print(f"  {c:4d} {t:42s} {'ours' if t in ours else 'MISSING'}")
    miss = [t for t in pt if t not in ours]
    mp = [t for t in pred if t not in ours]
    print(f"\n{len(miss)} processor type(s) and {len(mp)} predicate type(s) "
          f"cannot be built from data: "
          f"{', '.join(sorted(miss + mp))}")


def bastion_pools():
    """The `bastion/` pool tree: pools, elements, element types, projections
    and the processor lists it names.

    The bastion port rests on four claims about this tree and every one of
    them is a property of the datapack rather than of the code: no element
    type it does not already have, no projection needing a height source, no
    processor type it does not already have, and one position predicate. A
    datapack update that put a `feature_pool_element` in a bastion pool would
    make the port silently refuse a piece, which reads in a block diff as a
    hole in a rampart and in no gate at all."""
    pools = 0
    types, proj, lists = (collections.Counter() for _ in range(3))
    root = JAR + "/worldgen/template_pool/bastion"
    for p in glob.glob(root + "/**/*.json", recursive=True):
        pools += 1
        for e in json.load(open(p)).get("elements", []):
            el = e["element"]
            types[el.get("element_type")] += 1
            proj[el.get("projection")] += 1
            pr = el.get("processors")
            lists["INLINE" if not isinstance(pr, str) else pr] += 1
    return pools, types, proj, lists


def table_bastion():
    pools, types, proj, lists = bastion_pools()
    ours = parsed_processor_names()
    print(f"== the bastion/ pool tree: {pools} pools, {sum(types.values())} elements ==")
    print("element types:")
    for t, c in types.most_common():
        print(f"  {c:4d} {t:42s} {'ours' if t == 'minecraft:single_pool_element' else 'CHECK'}")
    print("projections:")
    for t, c in proj.most_common():
        # `rigid` needs no height source. A `terrain_matching` element would,
        # and `bastion_starts` passes a total function precisely so that a
        # datapack change is a measurement and not a panic.
        print(f"  {c:4d} {t}")
    print(f"processor lists named: {len(lists)} distinct")
    missing = []
    for name, c in lists.most_common():
        if name == "INLINE":
            print(f"  {c:4d} (inline)")
            continue
        path = f"{JAR}/worldgen/processor_list/{name.split(':')[-1]}.json"
        kinds = set()
        for pr in json.load(open(path)).get("processors", []):
            kinds.add(pr.get("processor_type"))
        bad = sorted(k for k in kinds if k not in ours)
        missing += bad
        print(f"  {c:4d} {name:42s} {','.join(sorted(kinds))}"
              f"{'  MISSING ' + ','.join(bad) if bad else ''}")
    print()
    if missing:
        print(f"{len(missing)} processor type(s) a bastion needs and this port "
              f"cannot build from data: {', '.join(sorted(set(missing)))}")
    else:
        print("Every processor type a bastion pool names has a parse arm, every")
        print("element is a `single_pool_element` and every projection is")
        print("`rigid` — so a bastion needs no element type, no projection and")
        print("no processor type this port does not already have.")
    return len(missing)


def position_predicates():
    """Every `position_predicate` in the jar, and which pools can reach it.

    This exists to make `PieceOrigin::unthreaded` falsifiable rather than
    merely commented. `place_piece` takes a `referencePos` only from callers
    that thread one; the other four say `unthreaded` at the call site, and
    that is correct only while no processor list they resolve carries a live
    `PosRuleTest`. That is a property of the DATAPACK, so it is asserted from
    the datapack here and not argued in a doc comment.

    Returns (per-type counts, {list name: [pools that name it]})."""
    types = collections.Counter()
    carriers = set()
    for p in glob.glob(JAR + "/worldgen/processor_list/*.json"):
        name = "minecraft:" + os.path.basename(p)[:-5]
        for pr in json.load(open(p)).get("processors", []):
            for r in pr.get("rules", []):
                pp = r.get("position_predicate")
                if pp is None:
                    continue
                t = pp.get("predicate_type")
                types[t] += 1
                # `PosAlwaysTrueTest` written out explicitly is not a live
                # predicate: it is the codec's own default and spends no draw,
                # so a list carrying only those is safe for an unthreaded
                # caller. None of 26.2's does, but the distinction is the
                # whole point of the check.
                if t != "minecraft:always_true":
                    carriers.add(name)
    users = collections.defaultdict(list)
    for p in glob.glob(JAR + "/worldgen/template_pool/**/*.json", recursive=True):
        rel = os.path.relpath(p, JAR + "/worldgen/template_pool")[:-5]
        for e in json.load(open(p)).get("elements", []):
            pr = e["element"].get("processors")
            if isinstance(pr, str) and pr in carriers:
                users[pr].append(rel)
    return types, {k: sorted(v) for k, v in users.items()}


def table_positions():
    types, users = position_predicates()
    total = sum(types.values())
    print(f"== {total} position_predicate(s) in the jar's 40 processor lists ==")
    ours = parsed_processor_names()
    for t, c in types.most_common():
        print(f"  {c:4d} {t:42s} {'ours' if t in ours else 'MISSING'}")
    print()
    outside = []
    for lst, pools in sorted(users.items()):
        print(f"  {lst} is named by {len(pools)} pool element(s):")
        for pool in pools:
            print(f"      {pool}")
            if not pool.startswith("bastion/"):
                outside.append((lst, pool))
    print()
    if outside:
        print("REACHABLE OUTSIDE bastion/ - an unthreaded caller may now hit a")
        print("live position predicate, and PieceOrigin::unthreaded is wrong:")
        for lst, pool in outside:
            print(f"      {lst} <- {pool}")
    else:
        print("Every live position predicate is reachable only from `bastion/`")
        print("pools, which only `bastion_starts` opens and which thread a")
        print("reference position. `PieceOrigin::unthreaded` is therefore safe")
        print("for the ruined portal, the shipwreck, the End city and the")
        print("nether fossil - by the data, not by assertion.")
    return len(outside)


def _cf_docs():
    return {
        os.path.basename(p)[:-5]: json.load(open(p))
        for p in glob.glob(JAR + "/worldgen/configured_feature/*.json")
    }


def _pf_to_cf():
    out = {}
    for p in glob.glob(JAR + "/worldgen/placed_feature/*.json"):
        f = json.load(open(p)).get("feature")
        if isinstance(f, str):
            out[os.path.basename(p)[:-5]] = f.split(":")[-1]
    return out


def _children(doc):
    """Every configured_feature this doc names, at any depth of its config.

    A flat scan of biome feature lists is not enough: `random_selector`,
    `simple_random_selector` and `random_boolean_selector` nest whole features
    inside their config, and both huge mushrooms are reached only that way. A
    census that missed the nesting would report them as dead when they are
    placed in two biomes."""
    out = []

    def walk(v):
        if isinstance(v, dict):
            for k, x in v.items():
                if k == "feature" and isinstance(x, str):
                    out.append(x.split(":")[-1])
                else:
                    walk(x)
        elif isinstance(v, list):
            for x in v:
                walk(x)

    walk(doc.get("config", {}))
    return out


def biome_reachable():
    """Configured features reachable from any biome's feature list,
    transitively through nested selectors."""
    cf, pf = _cf_docs(), _pf_to_cf()
    stack = []
    for p in glob.glob(JAR + "/worldgen/biome/*.json"):
        for step in json.load(open(p)).get("features", []):
            for x in step:
                n = pf.get(x.split(":")[-1])
                if n:
                    stack.append(n)
    seen = set()
    while stack:
        n = stack.pop()
        if n in seen or n not in cf:
            continue
        seen.add(n)
        for k in _children(cf[n]):
            if k in cf:
                stack.append(k)
            elif k in pf:
                stack.append(pf[k])
    return seen, cf


def table_reach():
    seen, cf = biome_reachable()
    arms = our_arms()
    bad = collections.Counter()
    for n in seen:
        t = cf[n].get("type")
        if t not in arms:
            bad[t] += 1
    print("== unported feature types that a biome actually reaches ==")
    for t, n in bad.most_common():
        print(f"  {n:3d} configured_feature(s)  {t}")
    alltypes = {d.get("type") for d in cf.values()}
    never = sorted(t for t in alltypes if t not in arms and t not in bad)
    print(f"\n{len(seen)} of {len(cf)} configured_features are reachable from "
          f"some biome.")
    print(f"{len(bad)} unported types covering {sum(bad.values())} of them are "
          f"reached, and are the ranked hole.")
    print(f"unported but never reached from any biome: "
          f"{', '.join(never) if never else 'none'}")


def placed_sets():
    """The structure_set documents the assembly tool actually opens.

    Read off the `structure_set/<name>.json` paths in generate.rs rather than
    off a list of driver function names: a `<name>_starts` function can exist,
    be exported, be unit-tested and never be called from the generator, and
    counting those would report a surface we do not place. The path literal is
    the thing that cannot be present without the set being read."""
    src = open(GENERATE_RS).read()
    return set(re.findall(r'structure_set/([a-z_]+)\.json', src))


def table_dispatch():
    placed = placed_sets()
    allsets = {s["name"]: s["structures"] for s in sets()}
    yes = sorted(n for n in allsets if n in placed)
    no = sorted(n for n in allsets if n not in placed)
    print("== which of the jar's 20 structure sets the assembly tool reads ==")
    ny = 0
    for n in yes:
        ny += len(allsets[n])
        print(f"  place  {n:22s} {len(allsets[n])}: {','.join(allsets[n])}")
    nn = 0
    for n in no:
        nn += len(allsets[n])
        print(f"  NO     {n:22s} {len(allsets[n])}: {','.join(allsets[n])}")
    print(f"\n{len(yes)} of {len(allsets)} sets, {ny} of {ny + nn} structures")
    unknown = placed - set(allsets)
    if unknown:
        print(f"read but not a jar set: {sorted(unknown)}")


def table_fixtures():
    print("== structure probes and their out-of-repo fixtures ==")
    inert = 0
    for name, path, note in FIXTURES:
        first = path.split(",")[0].strip()
        full = first if first.startswith("/") else os.path.join(ROOT, first)
        here = os.path.isdir(full)
        if not here:
            inert += 1
        print(f"{'present' if here else 'ABSENT ':8s} {name:24s} {first}")
        print(f"{'':8s} {'':24s} {note}")
    print(f"\n{inert} of {len(FIXTURES)} structure probes are inert on this "
          f"box. Each skips with an eprintln and returns, so it is GREEN in "
          f"T1 while asserting nothing.")
    return inert


def derive():
    els = pool_elements()
    feat = [(r, e) for r, t, e in els if t == "minecraft:feature_pool_element"]
    jar = configured_types()
    arms = our_arms()
    missing = [(t, n) for t, n in jar.items() if t not in arms]
    placed = placed_sets()
    allsets = {s["name"]: s["structures"] for s in sets()}
    ours_p = parsed_processor_names()
    pt_, pred_ = set(), set()
    nlists = 0
    for p in glob.glob(JAR + "/worldgen/processor_list/*.json"):
        nlists += 1
        for pr in json.load(open(p)).get("processors", []):
            pt_.add(pr.get("processor_type"))
            for r in pr.get("rules", []):
                for slot in ("input_predicate", "location_predicate",
                             "position_predicate"):
                    if slot in r:
                        pred_.add(r[slot].get("predicate_type"))
    seen, cfd = biome_reachable()
    arms2 = our_arms()
    reached_bad = collections.Counter()
    for n in seen:
        t = cfd[n].get("type")
        if t not in arms2:
            reached_bad[t] += 1
    pp_types, pp_users = position_predicates()
    b_pools, b_types, b_proj, b_lists = bastion_pools()
    return {
        "bastion_pools": b_pools,
        "bastion_elements": sum(b_types.values()),
        "bastion_element_types": len(b_types),
        "bastion_projections": len(b_proj),
        "bastion_processor_lists": len([k for k in b_lists if k != "INLINE"]),
        "position_predicates": sum(pp_types.values()),
        "position_predicate_pools": sum(len(v) for v in pp_users.values()),
        "position_predicate_pools_outside_bastion": sum(
            1
            for pools in pp_users.values()
            for pool in pools
            if not pool.startswith("bastion/")
        ),
        "processor_lists": nlists,
        "processor_types_missing": len([t for t in pt_ if t not in ours_p]),
        "predicate_types_missing": len([t for t in pred_ if t not in ours_p]),
        "reachable_cf": len(seen),
        "biome_reached_unported_types": len(reached_bad),
        "biome_reached_unported_cf": sum(reached_bad.values()),
        "sets_placed": len(placed & set(allsets)),
        "structures_placed": sum(
            len(v) for k, v in allsets.items() if k in placed
        ),
        "sets": len(sets()),
        "structure_entries": sum(len(s["structures"]) for s in sets()),
        "template_pools": len(
            glob.glob(JAR + "/worldgen/template_pool/**/*.json", recursive=True)
        ),
        "elements_total": len(els),
        "feature_elements": len(feat),
        "feature_pools": len(set(r for r, _ in feat)),
        "unreachable_types": len(missing),
        "unreachable_configured": sum(n for _, n in missing),
    }


def check():
    got = derive()
    bad = 0
    for k, want in sorted(EXPECT.items()):
        have = got[k]
        flag = "ok " if have == want else "DRIFT"
        if have != want:
            bad += 1
        print(f"{flag} {k:24s} expect {want:6d}  got {have:6d}")
    if bad:
        print(f"\n{bad} figure(s) drifted. Re-derive the prose that cites "
              f"them before changing EXPECT.")
    return 1 if bad else 0


def selftest():
    """Positive controls. Each must FAIL the thing it names, or the check it
    guards is vacuous - the defect this repo keeps finding in its own gates."""
    fails = 0

    def case(name, ok):
        nonlocal fails
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            fails += 1

    # 1. The arm parser must see an alternation arm. This is the exact shape
    #    that made the first version report 20 instead of 19.
    src = 'match x { "minecraft:aaa" | "minecraft:bbb" => 1, _ => 0 }'
    import tempfile
    global CONFIGURED_RS
    keep = CONFIGURED_RS
    with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False) as fh:
        fh.write(src)
        CONFIGURED_RS = fh.name
    got = our_arms()
    CONFIGURED_RS = keep
    os.unlink(fh.name)
    case("alternation arm yields both branches",
         got == {"minecraft:aaa", "minecraft:bbb"})

    # 2. A grep that only takes the literal touching `=>` must MISS the first
    #    branch - the control that proves case 1 is not vacuous.
    naive = set(re.findall(r'"(minecraft:[a-z_]+)"\s*=>', src))
    case("the naive grep this replaces does miss it",
         naive == {"minecraft:bbb"})

    # 3. --check must refuse a wrong figure.
    if have_jar():
        keep_e = EXPECT["sets"]
        EXPECT["sets"] = keep_e + 1
        import io
        import contextlib
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = check()
        EXPECT["sets"] = keep_e
        case("--check refuses a deliberately wrong figure", rc == 1)
        case("--check passes on the real figures", check() == 0)

        # 4. The position-predicate walk must SEE a live predicate that a
        #    naive grep for the field name would also see -- and must NOT
        #    count a `position_predicate` that is spelled out as
        #    `always_true`, because that one is the codec's own default,
        #    spends no draw, and does not make an unthreaded caller wrong.
        #    Without this control `position_predicate_pools_outside_bastion`
        #    could be 0 because the walk finds nothing at all.
        types, users = position_predicates()
        case("the walk finds the jar's one live position predicate",
             types.get("minecraft:axis_aligned_linear_pos") == 1)
        case("and attributes it to a pool", sum(len(v) for v in users.values()) == 1)

        import json as _json
        import tempfile as _tf
        global JAR
        keep_jar = JAR
        root = _tf.mkdtemp()
        os.makedirs(root + "/worldgen/processor_list")
        os.makedirs(root + "/worldgen/template_pool/village")
        _json.dump(
            {"processors": [{"processor_type": "minecraft:rule", "rules": [
                {"input_predicate": {"predicate_type": "minecraft:always_true"},
                 "location_predicate": {"predicate_type": "minecraft:always_true"},
                 "position_predicate": {"predicate_type": "minecraft:always_true"},
                 "output_state": {"Name": "minecraft:air"}}]}]},
            open(root + "/worldgen/processor_list/harmless.json", "w"),
        )
        _json.dump(
            {"elements": [{"element": {
                "element_type": "minecraft:single_pool_element",
                "processors": "minecraft:harmless"}}]},
            open(root + "/worldgen/template_pool/village/houses.json", "w"),
        )
        JAR = root
        t2, u2 = position_predicates()
        case("a spelled-out always_true is counted but charges no pool",
             sum(t2.values()) == 1 and not u2)

        # And the same fixture with a LIVE predicate must charge the pool,
        # which is the half that proves the half above is not vacuous.
        _json.dump(
            {"processors": [{"processor_type": "minecraft:rule", "rules": [
                {"input_predicate": {"predicate_type": "minecraft:always_true"},
                 "location_predicate": {"predicate_type": "minecraft:always_true"},
                 "position_predicate": {
                     "predicate_type": "minecraft:axis_aligned_linear_pos",
                     "max_chance": 0.05, "max_dist": 100},
                 "output_state": {"Name": "minecraft:air"}}]}]},
            open(root + "/worldgen/processor_list/harmless.json", "w"),
        )
        t3, u3 = position_predicates()
        JAR = keep_jar
        case("a live one on a non-bastion pool IS charged to it",
             u3.get("minecraft:harmless") == ["village/houses"])
    else:
        print("skip  jar-dependent controls: no extract at " + JAR)

    print(f"\n{fails} failure(s)")
    return 1 if fails else 0


def main():
    args = sys.argv[1:]
    if "--selftest" in args:
        return selftest()
    if not have_jar():
        # Exit 2, not 0: a check that could not read its input has not
        # passed (ci/check-vacuous-green.py). No script invokes this one, so
        # no caller reads the old 0.
        print(f"STRUCTURE-SURFACE REFUSED - no vanilla extract at {JAR} "
              f"(jar-derived, gitignored)", file=sys.stderr)
        return 2
    if "--check" in args:
        return check()
    if "--sets" in args:
        table_sets()
        return 0
    if "--elements" in args:
        table_elements()
        return 0
    if "--features" in args:
        table_features()
        return 0
    if "--arms" in args:
        table_arms()
        return 0
    if "--processors" in args:
        table_processors()
        return 0
    if "--positions" in args:
        return 1 if table_positions() else 0
    if "--bastion" in args:
        return 1 if table_bastion() else 0
    if "--reach" in args:
        table_reach()
        return 0
    if "--dispatch" in args:
        table_dispatch()
        return 0
    if "--fixtures" in args:
        table_fixtures()
        return 0
    for f in (table_sets, table_elements, table_features, table_arms,
              table_processors, table_positions, table_bastion, table_reach,
              table_dispatch, table_fixtures):
        f()
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
