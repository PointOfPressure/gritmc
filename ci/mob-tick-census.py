#!/usr/bin/env python3
"""mob-tick-census.py — how much mob there is in the jar, and how much in us.

# ===== AGENT INTERFACE =====
#
#     ci/mob-tick-census.py              the census
#     ci/mob-tick-census.py --check      refuse if any pinned figure moved
#     ci/mob-tick-census.py --explain    what it over-approximates, and why
#
# It reads the pinned jar and this workspace's own Rust sources. It takes no
# corpus lock, starts no server and needs no recording.
# ===== end interface =====

# # WHY THIS EXISTS: AN INVENTORY CAN ONLY COUNT WHAT SOMEBODY WROTE DOWN

`ci/unported-inventory.py --open` reports **one** entity-behaviour gap in the
whole server — `entities.rs:19`, "per-kind entity behaviour not written" — and
that is a true statement about the crate's *refusal markers*. It is not a
statement about how much mob is missing, and the two are not close.

A marker needs a host line. It is written *inside* the module that implements
the surrounding mechanic, by the person who implemented it, at the point where
they declined to go further. `power.rs` carries redstone's markers because
`power.rs` is 164 KB of ported redstone. A subsystem with **no module at all**
cannot carry markers proportional to its size — it carries however many
paragraphs a neighbouring module's author chose to write about it, which for
mobs is one, in `entities.rs`'s module doc, plus one more in
`rustmc-entity/src/lib.rs` that the inventory cannot see because its scan root
is `crates/rustmc-server/src` and nothing else.

So the inventory's resolution on any subsystem is bounded by how much of that
subsystem is *written*, not by how much is *missing*. That is not a defect in
the inventory; it is what a marker census is. This file is the other
direction: it asks the jar how big the thing is.

# # WHAT IT MEASURES

Four figures, each derived and none typed in:

* **the population** — every `EntityType` registration in `EntityTypes`, with
  the `MobCategory` it was built with, and every subtype of `Mob`;
* **the per-kind surface** — every method declaration in the `Mob` hierarchy,
  and how many classes override each hook. This is the literal referent of
  `entities.rs:19`'s phrase "per-kind entity behaviour";
* **the tick surface** — what a mob's tick reaches, from `Mob.tick`,
  `Mob.aiStep`, `Mob.serverAiStep` and `Mob.checkDespawn`;
* **the spawn surface** — what `NaturalSpawner` reaches.

and then **ours**: the `EntityKind` constants this server declares, and the
intersection of those with the jar's mob set.

# # THE POSITIVE CONTROL, WHICH IS NOT OPTIONAL

The interesting output of this script is a **zero** — no mob kind in the
crate — and a grep that finds nothing is indistinguishable from a grep that is
broken. `710` went green for two months over a mechanic that never ran.

So the reader is proved live on every run: the same parser that looks for mob
kinds must find `PLAYER` and `ITEM` in `entities.rs` **and** match both to
real registrations in the jar. If it cannot, the script refuses and reports
nothing, because a census whose reader is dead is not a measurement.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)

JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
PIN = os.path.join(HERE, "checks", "mob-surface.json")

MOB = "net/minecraft/world/entity/Mob"
AI_PKG = re.compile(r"^net/minecraft/world/entity/ai/")
PF_PKG = re.compile(r"^net/minecraft/world/level/pathfinder/")
ENTITY_TYPES = "net/minecraft/world/entity/EntityTypes"
NATURAL_SPAWNER = "net/minecraft/world/level/NaturalSpawner"

# The tick roots. `Mob.tick` alone is not the mob's tick: `serverAiStep` is
# reached through `LivingEntity.aiStep`'s server branch and `checkDespawn` is
# called by the level's own per-entity lambda *before* the range gate, not by
# `tick` at all. Naming all four is what makes the surface the mob's rather
# than one method's.
TICK_ROOTS = [
    (MOB, "tick", "()V"),
    (MOB, "aiStep", "()V"),
    (MOB, "serverAiStep", "()V"),
    (MOB, "checkDespawn", "()V"),
]

SPAWN_ROOTS = [
    (NATURAL_SPAWNER, "spawnForChunk",
     "(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/level/chunk/LevelChunk;"
     "Lnet/minecraft/world/level/NaturalSpawner$SpawnState;Ljava/util/List;)V"),
]

# Hooks quoted in the report. Chosen because each is a *per-kind* decision —
# a class that declares one is a class whose behaviour differs from `Mob`'s.
QUOTED_HOOKS = [
    ("registerGoals", "()V"),
    ("aiStep", "()V"),
    ("tick", "()V"),
    ("customServerAiStep", "(Lnet/minecraft/server/level/ServerLevel;)V"),
    ("finalizeSpawn", None),
    ("mobInteract", None),
    ("createNavigation", None),
    ("checkSpawnRules", None),
]


def jar_path():
    for base in (ROOT, os.path.expanduser("~/rustmc")):
        p = os.path.join(base, JAR_REL)
        if os.path.exists(p):
            return p
    return None


def load_graph(jar):
    from jarcallgraph import JarGraph
    return JarGraph(jar)


# ------------------------------------------------------------------- caching
# Only the JAR-derived half is cached, and the key is the jar's own path, size
# and mtime. The jar is pinned — `docs/AGENT-BRIEF.md`'s clean-room rule names
# one file and this project has never had two — so a cache keyed on it cannot
# go stale without the pin having moved, which is a much louder event than a
# stale cache.
#
# **The crate-derived half is never cached, and that is the whole point.**
# `our_kinds()` and the positive control read `entities.rs` on every run. If
# they were cached, the falsifier's injected faults would be answered from a
# file written before the fault existed and every assertion would pass
# vacuously — which is the failure this project has now seen in four separate
# instruments. `--no-cache` exists for anyone who does not want to trust the
# paragraph above.
CACHE_DIR = os.path.join(
    os.environ.get("TMPDIR", "/tmp"), "rustmc-mob-census-cache")


def cache_key(jar):
    st = os.stat(jar)
    return "%d-%d" % (st.st_size, int(st.st_mtime))


def jar_figures(jar, use_cache=True):
    """The jar-derived half of the census. Cached; see above."""
    path = os.path.join(CACHE_DIR, cache_key(jar) + ".json")
    if use_cache and os.path.exists(path):
        try:
            return json.load(open(path))
        except Exception:
            pass
    out = _derive_jar_figures(jar)
    if use_cache:
        try:
            os.makedirs(CACHE_DIR, exist_ok=True)
            json.dump(out, open(path, "w"))
        except OSError:
            pass
    return out


def _derive_jar_figures(jar):
    g = load_graph(jar)
    pop = population(jar)
    subs = sorted(g.subtypes()[MOB])
    hier = set(subs) | {MOB}
    decls = sum(len(g.declares[c]) for c in hier)
    sigs = Counter()
    for c in hier:
        for nd in g.declares[c]:
            sigs[nd] += 1
    despawn = {}
    out2 = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-cp", jar,
         "net.minecraft.world.entity.MobCategory"],
        capture_output=True, text=True).stdout
    body = out2.split("static {};", 1)
    if len(body) == 2:
        # Each enum constant is `ldc "NAME"`, an ordinal, `ldc "name"`,
        # `ldc "ABBREV"`, then four numbers of which the LAST is
        # `despawnDistance` — the constructor is
        # `(String, int, String, String, int, boolean, boolean, int)`.
        # Keyed on the FIRST string of the group, which is the enum constant;
        # the first version of this keyed on the last and produced {MO: 128,
        # C: 128, ...}, right values under the abbreviations.
        cur = None
        vals = []
        for ln in body[1].splitlines():
            m = re.search(r'// String ([A-Za-z_]+)$', ln.strip())
            if m:
                if cur is None:
                    cur = m.group(1)
                    vals = []
                continue
            m = re.search(r'(?:sipush|bipush)\s+(\d+)', ln)
            if m and cur is not None:
                vals.append(int(m.group(1)))
            if '"<init>":(Ljava/lang/String;ILjava/lang/String;' in ln and cur:
                despawn[cur] = vals[-1]
                cur = None
    nod = re.search(
        r'public int getNoDespawnDistance\(\);\s*Code:\s*\d+: (?:bipush|sipush)\s+(\d+)',
        out2)
    # The spawn yaw draw, because three separate findings rest on it and none
    # of them had checked that it is the ONLY one. `EntityType`'s whole class
    # contains exactly one `RandomSource.nextFloat` call site; it is in
    # `create(ServerLevel, PostSpawnProcessor, BlockPos, EntitySpawnReason,
    # boolean, boolean)` at offset 103, `* 360.0f`, into `Entity.snapTo`
    # fifteen instructions later, from an unseeded generator.
    #
    # Pinned as a LIST of offsets rather than a count: if a second draw ever
    # appears, "the spawn yaw draw" stops being a definite article and every
    # sentence built on it has to be re-read.
    out3 = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-cp", jar,
         "net.minecraft.world.entity.EntityType"],
        capture_output=True, text=True).stdout
    yaw = []
    cur = None
    for ln in out3.splitlines():
        if re.match(r"^  \S", ln) and ln.rstrip().endswith(";") and "(" in ln:
            cur = ln.strip()
        if cur and "RandomSource.nextFloat" in ln:
            m = re.match(r"\s*(\d+):", ln)
            yaw.append("%s@%s" % (cur.split("(")[0].split()[-1],
                                  m.group(1) if m else "?"))
    overriders = sorted(
        c for c, ds in g.declares.items() if ("checkDespawn", "()V") in ds
        and c != "net/minecraft/world/entity/Entity")
    tick = g.reach(list(TICK_ROOTS))
    spawn = g.reach(list(SPAWN_ROOTS))
    tick_ai = [m for m in tick if AI_PKG.match(m[0])]
    tick_pf = [m for m in tick if PF_PKG.match(m[0])]
    cats = Counter(c for _, c in pop)
    return {
        "population": pop,
        "categories": dict(cats),
        "mob_types": [n for n, c in pop if c != "MISC"],
        "mob_subtypes": len(subs),
        "hierarchy_classes": len(hier),
        "hierarchy_declarations": decls,
        "hierarchy_signatures": len(sigs),
        "hierarchy_overridden": sum(1 for v in sigs.values() if v >= 2),
        "hooks": {"%s|%s" % (n, d): v for (n, d), v in sigs.items()},
        "tick_reach_ai_methods": len(tick_ai),
        "tick_reach_ai_classes": len({m[0] for m in tick_ai}),
        "tick_reach_pathfinder_methods": len(tick_pf),
        "tick_reach_pathfinder_classes": len({m[0] for m in tick_pf}),
        "tick_reach_unrestricted": len(tick),
        "spawn_reach_unrestricted": len(spawn),
        "jar_methods": len(g.calls),
        "despawn_distance": despawn,
        "no_despawn_distance": int(nod.group(1)) if nod else None,
        "check_despawn_overriders": overriders,
        "entitytype_nextfloat_sites": sorted(yaw),
    }


# ---------------------------------------------------------------- population
_REG = re.compile(
    r"Field net/minecraft/world/entity/EntityTypeIds\.([A-Z0-9_]+):"
)
_CAT = re.compile(r"Field net/minecraft/world/entity/MobCategory\.([A-Z_]+):")


def population(jar):
    """(name, category) for every registration in `EntityTypes`, from javap.

    Read with `javap -p -c -constants`, the clean-room instrument, rather than
    from the call graph: the registration is a *constant* pattern — a
    `getstatic` of the id, then a `getstatic` of the category — and reading it
    as text is checkable by hand in a way a parser's output is not.
    """
    import subprocess
    out = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-cp", jar,
         ENTITY_TYPES.replace("/", ".")],
        capture_output=True, text=True).stdout
    body = out.split("static {};", 1)
    if len(body) != 2:
        return []
    rows, pending = [], None
    for ln in body[1].splitlines():
        m = _REG.search(ln)
        if m:
            pending = m.group(1)
            continue
        m = _CAT.search(ln)
        if m and pending:
            rows.append(("minecraft:" + pending.lower(), m.group(1)))
            pending = None
    return rows


# --------------------------------------------------------------------- ours
_KIND = re.compile(r'^pub const ([A-Z0-9_]+): EntityKind = EntityKind \{',
                   re.M)
_NAME = re.compile(r'name:\s*"([^"]+)"')


def our_kinds():
    """The `EntityKind` constants this server declares, name and registry id.

    Deliberately parses the source rather than importing anything: the point
    of the measurement is what the *crate* declares, and a test binary that
    linked would be a different question.
    """
    p = os.path.join(ROOT, "crates", "rustmc-server", "src", "entities.rs")
    src = open(p).read()
    out = []
    for m in _KIND.finditer(src):
        tail = src[m.end():m.end() + 400]
        n = _NAME.search(tail)
        out.append((m.group(1), n.group(1) if n else None))
    return out


# ------------------------------------------- the crate's own transcription
# `crates/rustmc-entity/src/tick.rs` writes down three things it read out of
# the bytecode and that nothing has ever re-checked: which classes override
# `Entity.checkDespawn`, `MobCategory.getDespawnDistance`'s per-category value,
# and `getNoDespawnDistance`'s constant. All three were correct on 2026-08-19.
#
# A correct transcription that nothing checks is a correct transcription until
# somebody edits it. These are cheap to hold, so they are held: the jar side is
# derived and the crate side is parsed, and `--check` compares them.
_DESPAWN = re.compile(r'Self::WaterAmbient => (\d+),')
_DESPAWN_DEFAULT = re.compile(r'_ => (\d+),')
_NODESPAWN = re.compile(r'pub const fn no_despawn_distance\(self\) -> i32 \{\s*(\d+)')


def our_despawn_constants():
    p = os.path.join(ROOT, "crates", "rustmc-entity", "src", "tick.rs")
    if not os.path.exists(p):
        return {}
    src = open(p).read()
    out = {}
    m = _DESPAWN.search(src)
    if m:
        out["water_ambient"] = int(m.group(1))
    m = _DESPAWN_DEFAULT.search(src)
    if m:
        out["default"] = int(m.group(1))
    m = _NODESPAWN.search(src)
    if m:
        out["no_despawn"] = int(m.group(1))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--explain", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--no-cache", action="store_true",
                    help="re-derive the jar half instead of reading the cache")
    a = ap.parse_args()

    if a.explain:
        print(__doc__)
        print(EXPLAIN)
        return 0

    jar = jar_path()
    if not jar:
        print("mob-tick-census: no pinned jar under %s — nothing measured"
              % JAR_REL, file=sys.stderr)
        return 2

    J = jar_figures(jar, use_cache=not a.no_cache)
    pop = [tuple(x) for x in J["population"]]

    # ---- POSITIVE CONTROL, before any finding is printed -----------------
    ours = our_kinds()
    names = {n for _, n in ours if n}
    jarnames = {n for n, _ in pop}
    control = [n for n in ("minecraft:player", "minecraft:item") if n in names]
    matched = [n for n in control if n in jarnames]
    if len(control) != 2 or len(matched) != 2:
        print("mob-tick-census: REFUSED — the reader is not live.", file=sys.stderr)
        print("  entities.rs kinds read : %r" % (sorted(names),), file=sys.stderr)
        print("  jar registrations read : %d" % len(jarnames), file=sys.stderr)
        print("  control matched        : %r" % (matched,), file=sys.stderr)
        print("  A zero from a dead reader is not a finding. Nothing printed.",
              file=sys.stderr)
        return 3

    sigs = Counter()
    for k, v in J["hooks"].items():
        n, d = k.split("|", 1)
        sigs[(n, d)] = v
    cats = Counter(J["categories"])
    mob_types = J["mob_types"]

    ours_mob = sorted(set(names) & set(mob_types))

    # The crate's own transcription, against the jar it was transcribed from.
    oc = our_despawn_constants()
    jd = J.get("despawn_distance", {})
    jar_default = sorted({v for k, v in jd.items() if k != "WATER_AMBIENT"})
    transcription = []
    if oc:
        if len(jar_default) == 1 and oc.get("default") != jar_default[0]:
            transcription.append(
                ("MobCategory default despawn distance",
                 jar_default[0] if jar_default else None, oc.get("default")))
        if oc.get("water_ambient") != jd.get("WATER_AMBIENT"):
            transcription.append(
                ("WATER_AMBIENT despawn distance",
                 jd.get("WATER_AMBIENT"), oc.get("water_ambient")))
        if oc.get("no_despawn") != J.get("no_despawn_distance"):
            transcription.append(
                ("getNoDespawnDistance",
                 J.get("no_despawn_distance"), oc.get("no_despawn")))

    fig = {
        "entity_types": len(pop),
        "mob_category_types": len(mob_types),
        "misc_category_types": cats.get("MISC", 0),
        "mob_subtypes": J["mob_subtypes"],
        "hierarchy_classes": J["hierarchy_classes"],
        "hierarchy_declarations": J["hierarchy_declarations"],
        "hierarchy_signatures": J["hierarchy_signatures"],
        "hierarchy_overridden": J["hierarchy_overridden"],
        "tick_reach_ai_methods": J["tick_reach_ai_methods"],
        "tick_reach_ai_classes": J["tick_reach_ai_classes"],
        "tick_reach_pathfinder_methods": J["tick_reach_pathfinder_methods"],
        "tick_reach_pathfinder_classes": J["tick_reach_pathfinder_classes"],
        "our_entity_kinds": len(ours),
        "our_mob_kinds": len(ours_mob),
        "check_despawn_overriders": len(J["check_despawn_overriders"]),
        "transcription_disagreements": len(transcription),
        "entitytype_nextfloat_sites": J.get("entitytype_nextfloat_sites", []),
    }

    if a.json:
        print(json.dumps(fig, indent=1, sort_keys=True))
        return 0

    if a.check:
        return do_check(fig)

    print("== the jar ==")
    print("  entity types registered      %4d" % fig["entity_types"])
    for c, n in cats.most_common():
        print("      %-28s %4d" % (c, n))
    print("  types in a spawning category %4d" % fig["mob_category_types"])
    print("  subtypes of Mob              %4d" % fig["mob_subtypes"])
    print()
    print("== per-kind behaviour, which is what entities.rs:19 names ==")
    print("  classes in the Mob hierarchy %4d" % fig["hierarchy_classes"])
    print("  method declarations          %4d" % fig["hierarchy_declarations"])
    print("  distinct signatures          %4d" % fig["hierarchy_signatures"])
    print("  signatures 2+ classes declare%4d" % fig["hierarchy_overridden"])
    print("  per-kind hooks:")
    for name, desc in QUOTED_HOOKS:
        n = sum(v for (nm, d), v in sigs.items()
                if nm == name and (desc is None or d == desc))
        print("      %-28s %4d classes declare their own" % (name, n))
    print()
    print("== what a mob tick enters, restricted to the AI machinery ==")
    print("  ai/**        %5d methods across %3d classes"
          % (fig["tick_reach_ai_methods"], fig["tick_reach_ai_classes"]))
    print("  pathfinder/  %5d methods across %3d classes"
          % (fig["tick_reach_pathfinder_methods"],
             fig["tick_reach_pathfinder_classes"]))
    print("  (the unrestricted closure is %d methods of the jar's %d and is"
          % (J["tick_reach_unrestricted"], J["jar_methods"]))
    print("   NOT quoted as a finding — devirtualisation makes it most of the")
    print("   jar, which measures the approximation and not the mob.)")
    print()
    print("== the crate's transcription, against the jar it came from ==")
    print("  classes overriding Entity.checkDespawn   %d   %s"
          % (len(J["check_despawn_overriders"]),
             ", ".join(c.rsplit("/", 1)[-1] for c in J["check_despawn_overriders"])))
    if not oc:
        print("  crates/rustmc-entity/src/tick.rs: no constants found to check")
    elif not transcription:
        print("  MobCategory despawn distances and getNoDespawnDistance in")
        print("  crates/rustmc-entity/src/tick.rs agree with the jar: "
              "default %s, WATER_AMBIENT %s, no-despawn %s"
              % (jar_default[0] if jar_default else "?",
                 jd.get("WATER_AMBIENT"), J.get("no_despawn_distance")))
    else:
        print("  DISAGREEMENT between the crate and the jar:")
        for what, want, got in transcription:
            print("    %-38s jar %s, crate %s" % (what, want, got))
    print()
    print("  EntityType nextFloat call sites          %s"
          % ", ".join(J.get("entitytype_nextfloat_sites", [])) or "(none)")
    print("    the spawn yaw. One draw, unseeded — and the same field behind")
    print("    080's 15/15 floor flap, spawn_mobs being a null, and the whole")
    print("    4-of-92 corpus floor being entity scenarios.")
    print()
    print("== ours ==")
    print("  EntityKind constants declared%4d   %s"
          % (fig["our_entity_kinds"], ", ".join(k for k, _ in ours)))
    print("  of which are mobs            %4d   %s"
          % (fig["our_mob_kinds"], ", ".join(ours_mob) or "(none)"))
    print("  positive control             OK     %s matched jar registrations"
          % ", ".join(matched))
    print()
    print("  ci/unported-inventory.py --open reports 1 entity-behaviour gap.")
    print("  The hierarchy it stands for declares %d methods."
          % fig["hierarchy_declarations"])
    return 0


EXPLAIN = """
# # WHAT THIS OVER- AND UNDER-COUNTS

* **`reach` is an over-approximation** and `ci/jarcallgraph.py`'s header says
  why: an `invokevirtual` resolves to every subtype declaring the signature.
  The direction is safe for the claim made here — the claim is that the
  surface is *large*, and an over-approximated edge set cannot make a large
  surface small. It is not safe for any claim that a particular method IS
  reached, and no such claim is made.
* **`hierarchy_declarations` counts declarations, not behaviours.** A class
  that overrides `getAmbientSound` to return one constant is one declaration
  and almost no work. The figure is an upper bound on per-kind behaviour and
  the report says so rather than dressing it as an estimate of cost.
* **The population is read from `EntityTypes`' static initialiser only.** A
  type registered anywhere else would be missed. None is, in 26.2, and the
  count is cross-checkable against `data/vanilla/entity_types.json`.
* **`our_mob_kinds` is an intersection of names, not of behaviour.** A crate
  that declared `minecraft:pig` as an `EntityKind` and did nothing else would
  count as one, and the figure would be right to move: the point of it is
  that today the intersection is empty, so no mob can exist at all and every
  mob mechanic is unreachable by construction rather than by omission.
"""


# Not every pinned figure means the same thing when it moves, and a gate that
# does not say which is a gate the next agent reads as "I broke something".
#
# `our_entity_kinds` and `our_mob_kinds` are supposed to move. The day someone
# lands `SpawnEggItem` and a pig, `our_mob_kinds` goes 0 -> 1 and this check
# goes red — and that is the gate WORKING: it forces the re-pin into the same
# commit as the port, so §94's "the intersection is empty" cannot quietly stop
# being true while the sentence stays in the roadmap.
#
# Everything else is jar-derived. The jar is pinned, so those cannot move
# without either the pin moving or the derivation breaking, and both are
# defects.
# ---------------------------------------------------- figures quoted in prose
# `ci/check-figures.py` exists because this project's numbers drift away from
# the artefacts that produced them, and it caught five such drifts in two days.
# Its coverage scan matches `[0-9]+\.[0-9]{4,}%` — **percentages**. Every
# figure this lane publishes is a COUNT, so none of them is scanned by
# anything, and on 2026-08-19 four of them had already drifted: the N
# correction from 20 to 17 landed in two documents and not in the other two,
# and `31..150` outlived the number it came from in three places. Found by
# hand, which is the way this project has found every one of these.
#
# So the same discipline, scoped to counts and to this lane: a figure, its
# formatted form, and the files that quote it. `--check` requires the string
# to be present in each. It is deliberately a presence test and not a parse —
# the point is to make a re-pin fail loudly until the prose is fixed, which
# presence does, and a parser over English would be the more fragile half.
QUOTED = {
    "hierarchy_declarations": ("{:,}", [
        "docs/PARITY-ROADMAP.md", "docs/cold-start/mob-tick-surface.md",
        "docs/WORKLIST.md", "docs/SERVER.md"]),
    "entity_types": ("{}", [
        "docs/PARITY-ROADMAP.md", "docs/cold-start/mob-tick-surface.md",
        "docs/WORKLIST.md"]),
    "mob_subtypes": ("{}", [
        "docs/PARITY-ROADMAP.md", "docs/cold-start/mob-tick-surface.md",
        "docs/WORKLIST.md", "docs/SERVER.md"]),
    "hierarchy_classes": ("{}", [
        "docs/PARITY-ROADMAP.md", "docs/cold-start/mob-tick-surface.md"]),
    "mob_category_types": ("{}", [
        "docs/PARITY-ROADMAP.md", "docs/cold-start/mob-tick-surface.md",
        "docs/SERVER.md"]),
    "tick_reach_ai_methods": ("{:,}", [
        "docs/PARITY-ROADMAP.md", "docs/cold-start/mob-tick-surface.md"]),
}


# **A BARE COUNT IS A SUBSTRING OF LONGER NUMBERS, AND THIS RULE WAS GREEN ON
# ONE — 2026-09-05.** `mob_subtypes = 114` is declared as quoted in
# `docs/SERVER.md` and has never been written there. The rule passed anyway,
# because that file's line 787 reads `12716 carried, 11491 connective, 79111
# refused` and `11491` contains `114`. It surfaced only when `stack-worldgen`
# rewrote those counts and the coincidence went away — which is to say the
# rule reddened for the right figure at a moment that had nothing to do with
# it, and would otherwise have gone on passing indefinitely.
#
# Measured before narrowing rather than after: over all three landing stacks
# and all six declared figures, `mob_subtypes` in `docs/SERVER.md` is the ONLY
# pair where the substring test and the bounded test disagree. So this costs
# one prose fix, and buys a rule that means what its own comment says it
# means.
#
# The boundary excludes digits, commas and dots on both sides, because the
# formats here are `{}` and `{:,}` — `114` must not match inside `11491` or
# `1,148`, and `3,870` must not match inside `13,870`.
def quoted_drift(fig):
    """[(figure, value, file)] for every quoted figure missing from a doc."""
    out = []
    for key, (fmt, files) in sorted(QUOTED.items()):
        if key not in fig:
            continue
        want = fmt.format(fig[key])
        pat = re.compile(r"(?<![0-9,.])" + re.escape(want) + r"(?![0-9,.])")
        for rel in files:
            path = os.path.join(ROOT, rel)
            if not os.path.exists(path):
                out.append((key, want, rel + " (missing)"))
                continue
            if not pat.search(open(path).read()):
                out.append((key, want, rel))
    return out


EXPECTED_TO_MOVE = {
    "our_entity_kinds": "this server gained or lost an EntityKind — re-pin in "
                        "the same commit, and check docs/PARITY-ROADMAP.md §94",
    "our_mob_kinds": "A MOB LANDED. Re-pin here, and re-read §94 and "
                     "docs/SERVER.md, both of which say the intersection is "
                     "empty",
    "transcription_disagreements": "the crate and the jar now disagree about "
                                   "MobCategory — this is a DEFECT in "
                                   "crates/rustmc-entity/src/tick.rs, not a "
                                   "figure to re-pin",
}


def do_check(fig):
    if not os.path.exists(PIN):
        print("mob-tick-census --check: no pin at %s" % PIN, file=sys.stderr)
        return 2
    pinned = json.load(open(PIN))["figures"]
    bad = []
    for k, v in sorted(pinned.items()):
        got = fig.get(k)
        if got != v:
            bad.append((k, v, got))
    if bad:
        print("mob-tick-census --check: REFUSED, %d figure(s) moved" % len(bad),
              file=sys.stderr)
        for k, want, got in bad:
            print("    %-30s pinned %s, measured %s" % (k, want, got),
                  file=sys.stderr)
            if k in EXPECTED_TO_MOVE:
                print("    %-30s -> %s" % ("", EXPECTED_TO_MOVE[k]),
                      file=sys.stderr)
        if any(k in EXPECTED_TO_MOVE for k, _, _ in bad):
            print("  At least one of these is a figure that is SUPPOSED to move"
                  " as work lands.", file=sys.stderr)
            print("  Re-pin it in the same commit as the change, and fix the"
                  " prose it contradicts.", file=sys.stderr)
        else:
            print("  Every figure above is jar-derived, and the jar is pinned."
                  " One of these moving", file=sys.stderr)
            print("  means the pin moved or the derivation broke. Neither is a"
                  " number to edit.", file=sys.stderr)
        return 1
    drift = quoted_drift(fig)
    if drift:
        print("mob-tick-census --check: REFUSED — %d figure(s) quoted in prose"
              " no longer appear there" % len(drift), file=sys.stderr)
        for k, want, rel in drift:
            print("    %-28s %-8s not found in %s" % (k, want, rel),
                  file=sys.stderr)
        print("  ci/check-figures.py scans percentages; every figure this lane"
              " publishes is a", file=sys.stderr)
        print("  count, so nothing else is watching these. Fix the prose, or"
              " drop the file from", file=sys.stderr)
        print("  QUOTED if it genuinely no longer needs to carry the number.",
              file=sys.stderr)
        return 1
    print("mob-tick-census --check: %d figures match the pin, %d quoted in"
          " prose and present" % (len(pinned), len(QUOTED)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
