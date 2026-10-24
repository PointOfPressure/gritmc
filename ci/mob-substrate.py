#!/usr/bin/env python3
"""mob-substrate.py — the world the corpus runs in, and which mob mechanics it removes.

# ===== AGENT INTERFACE =====
#
#     ci/mob-substrate.py            the substrate, and what it suppresses
#     ci/mob-substrate.py --check    refuse if ci/t2-gameplay.sh's substrate moved
#
# Tree and jar only. No server, no recording, no lock.
# ===== end interface =====

# # WHY A SUBSTRATE NEEDS ITS OWN INSTRUMENT

`ci/t2-gameplay.sh` says it plainly — "`difficulty=peaceful` is not tidiness,
it is the substrate" — and it is right, for a reason its own comment gives
only half of. The comment is about slimes: superflat is full of them, they
arrived before `doMobSpawning` took effect, one scenario recorded 32 sound
observations that had nothing to do with it. Peaceful fixed that.

What the comment does not say is what peaceful *costs*, and the cost is the
entire subject of any mob work. Read out of the jar rather than assumed:

* `EntityType.canSpawn(Level)` returns **false** when the level's difficulty
  is `PEACEFUL` and the type was built with `.notInPeaceful()`;
* **38 of the jar's 158 registrations carry that flag**, every one of them
  `MobCategory.MONSTER`, which is 38 of the 86 types in a spawning category;
* and `Mob.checkDespawn`'s first branch `discard()`s such a mob outright —
  before the entity-ticking range gate, so distance does not save it.

So under this corpus's world, 38 mob types cannot enter at all. Not by natural
spawn, not by a spawn egg, not by `/summon` — and that last clause is derived
from three named call sites rather than from the call graph, because the graph
over-approximates and "reaches" is not evidence a path runs:

* `SpawnEggItem.useOn` and `SpawnEggItem.use` both call
  `EntityType.canSpawn(Level)`, whose body is `isAllowedInPeaceful() ||
  difficulty != PEACEFUL`;
* `SummonCommand.createEntity` does not go through `canSpawn` at all — it
  calls `ServerLevel.getDifficulty()` and `EntityType.isAllowedInPeaceful()`
  itself and throws a `CommandSyntaxException`. So `/summon` is refused with a
  message rather than silently producing a mob that dies on its first tick,
  and anyone testing this by hand will see the refusal.

`Mob.checkDespawn` is the third, independent, arm: it discards such a mob
*before* the entity-ticking range gate, so a mob that got in some fourth way
would not survive a tick either.

On top of that the harness sets `spawn-monsters=false` (the server property
that feeds `ServerChunkCache.spawnEnemies`, which
`NaturalSpawner.getFilteredSpawningCategories` reads) and the game rule
`spawn_mobs=false`, which `NaturalSpawner` and `ServerChunkCache` both
consult. Either alone stops natural spawning; the corpus sets both, plus
peaceful, plus `spawn-radius=0`.

**Four independent suppressions, stacked.** The consequence for this lane is
one sentence: *no natural-spawning mechanic can be measured by this corpus at
all, and a check written to measure one would be vacuous by construction* —
`710`'s failure mode, arrived at from the world rather than from the
projection.

# # WHAT THIS IS NOT

It is not an argument for changing the substrate. Peaceful is load-bearing:
`ci/t2-gameplay.sh` records what 32 stray slime observations did to one
scenario, and `origin/survival-substrate` is a whole branch about how
expensive moving one of these settings is. It is an argument for *saying so*
before anyone reports a spawn figure.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
T2 = os.path.join(HERE, "t2-gameplay.sh")
PIN = os.path.join(HERE, "checks", "mob-substrate.json")
JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"

# What each setting removes, and the bytecode that says so. Keyed by the
# setting as it appears in ci/t2-gameplay.sh, so a renamed setting shows up as
# an unexplained one rather than silently losing its explanation.
SUPPRESSES = {
    "difficulty=peaceful": (
        "EntityType.canSpawn returns false for every .notInPeaceful() type, and "
        "Mob.checkDespawn discards one that exists"),
    "spawn-monsters=false": (
        "feeds ServerChunkCache.spawnEnemies, read by "
        "NaturalSpawner.getFilteredSpawningCategories"),
    "spawn_mobs=false": (
        "read by NaturalSpawner and ServerChunkCache; stops the per-tick and "
        "the chunk-generation spawn both"),
    "spawn-radius=0": (
        "pins ServerPlayer.fudgeSpawnLocation, which otherwise moves the join "
        "position every run"),
    "gamemode=creative": (
        "Abilities.invulnerable, so Player.hurtServer returns at offset 31 and "
        "no damage_event or set_health is sent"),
    "generate-structures=false": (
        "no structure spawns; NaturalSpawner.isInNetherFortressBounds is never "
        "true"),
    "random_tick_speed=0": (
        "no random ticks, so nothing a mob would react to changes on its own"),
    "advance_time=false": (
        "the sun does not move, so no light-level spawn condition ever changes"),
    "advance_weather=false": (
        "no rain, so no weather-gated spawn or behaviour"),
}

# Two spellings of the properties block, because it stopped being a `$'...'`
# ANSI-C string on 2026-09-05 when `difficulty=` became a variable: `$'...'`
# does not expand parameters, so a substrate that names one has to be a
# double-quoted heredoc-ish string instead. Both forms are read, and a form
# neither pattern matches lands in `control()` as zero properties.
_PROPS = re.compile(r"VANILLA_EXTRA_PROPS=(?:\$'([^']*)'|\"([^\"]*)\")")
_RULES = re.compile(r"SUBSTRATE_GAMERULES=\(([^)]*)\)")
# `GAMEPLAY_DIFFICULTY="${GAMEPLAY_DIFFICULTY:-peaceful}"` — the default the
# corpus runs at when nothing in the environment says otherwise.
_DEFAULT = re.compile(r"^(\w+)=\"\$\{\1:-([a-z_]+)\}\"", re.M)


def _harness_sources():
    """`ci/t2-gameplay.sh` and every ci/ file it sources.

    THE SUBSTRATE MOVED OUT OF ONE FILE ON 2026-09-05 and this reader went
    blind — `read 0 server properties`, `read 0 game rules`, a T0 red that
    reads like the corpus lost its suppressions. It did not: the four game
    rules went to `ci/gameplay-substrate.sh` and the difficulty to
    `ci/gameplay-difficulty.sh`, both sourced by the gate.

    So the list is DERIVED from the gate's own `. "$ROOT/ci/…"` lines rather
    than written here. A second hardcoded filename would just be a second thing
    to move. What this cannot follow is a substrate that stops being sourced by
    `ci/t2-gameplay.sh` at all — and that is the right blind spot, because a
    setting the gate does not read is not the corpus's substrate.
    """
    src = open(T2).read()
    files = [T2]
    for rel in re.findall(r'^\.\s+"\$ROOT/(ci/[A-Za-z0-9._-]+)"', src, re.M):
        p = os.path.join(ROOT, rel)
        if os.path.exists(p):
            files.append(p)
    return files


def parse_substrate():
    props, rules, world, defaults = [], [], None, {}
    for f in _harness_sources():
        src = open(f).read()
        for name, val in _DEFAULT.findall(src):
            defaults.setdefault(name, val)
        m = _PROPS.search(src)
        if m and not props:
            body = m.group(1) if m.group(1) is not None else m.group(2)
            # `$'…'` carries literal backslash-n; the quoted form carries real
            # newlines. Split on both so one branch cannot read as one property.
            props = [p.strip() for p in body.replace("\\n", "\n").split("\n") if p.strip()]
        m = _RULES.search(src)
        if m and not rules:
            rules = m.group(1).split()
        if world is None:
            flat = re.search(
                r'"\$ROOT/ci/vanilla-server\.sh" start [^\n]*"(minecraft:[a-z_]+)"', src)
            if flat:
                world = flat.group(1)

    # Resolve the parameters the substrate now names, from the defaults the
    # harness itself declares. An unresolved one is left as-is on purpose: it
    # reaches `control()` as an unexpanded property and is refused there rather
    # than quietly dropped, which would shrink the substrate by one setting and
    # look exactly like a setting nobody set.
    def expand(tok):
        for name, val in defaults.items():
            tok = tok.replace("${%s}" % name, val).replace("$" + name, val)
        return tok

    return [expand(p) for p in props], [expand(r) for r in rules], world


def jar_types():
    """(name, category, notInPeaceful) for every EntityTypes registration."""
    jar = None
    for base in (ROOT, os.path.expanduser("~/rustmc")):
        p = os.path.join(base, JAR_REL)
        if os.path.exists(p):
            jar = p
            break
    if not jar:
        return []
    out = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-cp", jar,
         "net.minecraft.world.entity.EntityTypes"],
        capture_output=True, text=True).stdout
    parts = out.split("static {};", 1)
    if len(parts) != 2:
        return []
    reg = re.compile(r"Field net/minecraft/world/entity/EntityTypeIds\.([A-Z0-9_]+):")
    cat = re.compile(r"Field net/minecraft/world/entity/MobCategory\.([A-Z_]+):")
    put = re.compile(r"Field ([A-Z0-9_]+):Lnet/minecraft/world/entity/EntityType;")
    nip = re.compile(r"Builder\.notInPeaceful")
    cur = c = None
    flag = False
    rows = []
    for ln in parts[1].splitlines():
        m = reg.search(ln)
        if m:
            cur, c, flag = m.group(1), None, False
            continue
        if cur is None:
            continue
        m = cat.search(ln)
        if m and c is None:
            c = m.group(1)
        if nip.search(ln):
            flag = True
        m = put.search(ln)
        if m and m.group(1) == cur:
            rows.append(("minecraft:" + cur.lower(), c, flag))
            cur = None
    return rows


def figures():
    props, rules, world = parse_substrate()
    rows = jar_types()
    spawning = [r for r in rows if r[1] != "MISC"]
    peace = [r for r in rows if r[2]]
    return {
        "properties": sorted(props),
        "gamerules": sorted(rules),
        "world_preset": world,
        "entity_types": len(rows),
        "spawning_category_types": len(spawning),
        "not_in_peaceful": len(peace),
        "monster_types": sum(1 for r in rows if r[1] == "MONSTER"),
        "monster_allowed_in_peaceful": sorted(
            r[0] for r in rows if r[1] == "MONSTER" and not r[2]),
    }


def control(fig):
    """Refuse rather than report if the parser read nothing.

    A substrate reader that finds no settings reports a world with no
    suppressions, which is the most misleading output this file could
    produce — it would say the corpus can measure spawning. So the shape of
    what was parsed is asserted before anything is printed.
    """
    bad = []
    if len(fig["properties"]) < 3:
        bad.append("read %d server properties from ci/t2-gameplay.sh and the files it sources"
                   % len(fig["properties"]))
    if len(fig["gamerules"]) < 3:
        bad.append("read %d game rules" % len(fig["gamerules"]))
    # An unexpanded parameter is a setting whose VALUE this reader does not
    # know, and reporting `difficulty=$GAMEPLAY_DIFFICULTY` as a suppression is
    # the same shape as reporting none: a claim about a world nobody has
    # established. Added 2026-09-05, when the substrate started naming
    # variables.
    unexpanded = [t for t in list(fig["properties"]) + list(fig["gamerules"]) if "$" in t]
    if unexpanded:
        bad.append("could not resolve %s — no default declared for it in "
                   "ci/t2-gameplay.sh or anything it sources"
                   % ", ".join(sorted(unexpanded)))
    if fig["entity_types"] < 100:
        bad.append("read %d entity registrations from the jar"
                   % fig["entity_types"])
    if fig["not_in_peaceful"] == 0:
        bad.append("found no .notInPeaceful() type, which cannot be true of 26.2")
    return bad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()

    fig = figures()
    bad = control(fig)
    if bad:
        print("mob-substrate: REFUSED — the reader is not live:", file=sys.stderr)
        for b in bad:
            print("    %s" % b, file=sys.stderr)
        print("  A substrate reader that finds nothing reports a world with no",
              file=sys.stderr)
        print("  suppressions. Nothing printed.", file=sys.stderr)
        return 3

    if a.json:
        print(json.dumps(fig, indent=1, sort_keys=True))
        return 0

    if a.check:
        if not os.path.exists(PIN):
            print("mob-substrate --check: no pin at %s" % PIN, file=sys.stderr)
            return 2
        want = json.load(open(PIN))["substrate"]
        moved = [k for k in want if fig.get(k) != want[k]]
        if moved:
            print("mob-substrate --check: REFUSED — the corpus substrate moved",
                  file=sys.stderr)
            for k in moved:
                print("    %-28s pinned %r" % (k, want[k]), file=sys.stderr)
                print("    %-28s now    %r" % ("", fig.get(k)), file=sys.stderr)
            print("  Every mob finding on this branch is conditional on the"
                  " substrate. Re-read", file=sys.stderr)
            print("  what it hides before re-pinning; do not re-pin to make a"
                  " gate green.", file=sys.stderr)
            return 1
        print("mob-substrate --check: substrate unchanged (%d properties, "
              "%d game rules)" % (len(fig["properties"]), len(fig["gamerules"])))
        return 0

    print("== the world ci/t2-gameplay.sh boots ==")
    print("  preset      %s" % fig["world_preset"])
    for p in fig["properties"]:
        print("  property    %-28s %s" % (p, SUPPRESSES.get(p, "")))
    for r in fig["gamerules"]:
        print("  game rule   %-28s %s" % (r, SUPPRESSES.get(r, "")))
    unexplained = [s for s in fig["properties"] + fig["gamerules"]
                   if s not in SUPPRESSES]
    if unexplained:
        print("  UNEXPLAINED %s" % ", ".join(unexplained))
        print("              (a setting with no entry above has never been read"
              " for what it removes)")
    print()
    print("== what that removes, derived from the jar ==")
    print("  entity types                      %3d" % fig["entity_types"])
    print("  in a spawning category            %3d" % fig["spawning_category_types"])
    print("  built .notInPeaceful()            %3d   cannot enter this world at all,"
          % fig["not_in_peaceful"])
    print("                                          by spawn, egg or /summon")
    print("  MONSTER types                     %3d   of which %d survive peaceful:"
          % (fig["monster_types"], len(fig["monster_allowed_in_peaceful"])))
    for n in fig["monster_allowed_in_peaceful"]:
        print("                                          %s" % n)
    print()
    print("  Natural spawning is off three times over: peaceful, "
          "spawn-monsters=false,")
    print("  and the spawn_mobs game rule. No spawn-selection mechanic is "
          "measurable")
    print("  by this corpus, and a check written to measure one would be "
          "vacuous by")
    print("  construction rather than by omission.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
