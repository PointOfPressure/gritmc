#!/usr/bin/env python3
"""order-keyed-census.py — every place vanilla's observable output depends on
an order that is not a function of the entities' own state.

# The class of defect

Most parity questions are about a value: this block's state, that entity's
position. A different class is about a *sequence* — which of two identical
things happens first — and it has no field a state comparison can look at.
Three were already known to this project:

  * `ServerChunkCache.broadcastChangedChunks` walks a `ReferenceOpenHashSet`;
  * `ContainerOpenersCounter`, and the chunk-send order, both order-sensitive
    in ways no state comparison could see;
  * `Mob.serverAiStep` gates its goal phase on `(tickCount + getId()) % 2`,
    which makes ENTITY ID ALLOCATION ORDER behaviour-visible.

This enumerates them from the jar and puts each in one of four order classes,
because they are not the same problem and only one of them is hopeless:

  SORTED     the walk order is a function of the keys and the keys are state
             (a section coordinate, an array index). Portable by writing the
             same comparator. Nothing to measure.
  INSERTION  the walk order is the order things were added. Portable by
             adding them in the same order; the risk is a container that
             silently reorders (a `remove` + `add` moves an entry to the end).
  KEYHASH    the walk order is a function of the KEY VALUES through the
             container's hash. Deterministic — the same key set always walks
             the same way — so it is a genuine parity REQUIREMENT and a
             matchable one. It is also the sharpest form of "id allocation is
             behaviour-visible": change one id and the whole walk permutes.
  IDENTITY   the walk order is a function of `Object.hashCode()` on objects
             that do not override it. HotSpot draws those from a per-thread
             sequence at first-hash time, so the order is a function of the
             DRAW ORDER, which is run-stable exactly when the insertion
             sequence is. Measured in `ci/idhash-determinism.sh`.

The IDENTITY row is the one this branch corrects. "Identity hash" has been
read here as "random per run", and it is not: the same allocation and hashing
sequence produces the same hashes on every run of the same JVM. A site whose
insertions are run-stable therefore has a run-stable order, and vanilla will
agree with itself about it forever while we cannot reproduce it without
reproducing a JVM's PRNG. A site whose insertions are NOT run-stable inherits
that instability and vanilla disagrees with itself — which is the reading
`broadcastChangedChunks` measured at 4.24% against 1/4!.

# What is derived and what is declared

The site list is declared: which methods matter is a judgement. Everything
about each site is derived — the concrete container class comes from the
`new` in the owning class's bytecode, and whether a key type overrides
`hashCode` comes from that type's own declaration. A site whose container
cannot be found is a REFUSAL, not a skip.
"""

import argparse
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
PIN = os.path.join(HERE, "checks", "order-keyed.json")

# The order class of a container, as a property of the LIBRARY rather than of
# the server. Declared here because it is a fact about fastutil and the JDK
# that no amount of reading `net/minecraft` will produce, and because getting
# it wrong is the only way this census can be quietly wrong.
CONTAINER_ORDER = {
    "it/unimi/dsi/fastutil/ints/Int2ObjectLinkedOpenHashMap": "INSERTION",
    "it/unimi/dsi/fastutil/longs/Long2ObjectLinkedOpenHashMap": "INSERTION",
    "it/unimi/dsi/fastutil/objects/Object2ObjectArrayMap": "INSERTION",
    "it/unimi/dsi/fastutil/objects/ObjectArrayList": "INSERTION",
    "java/util/ArrayList": "INSERTION",
    "it/unimi/dsi/fastutil/ints/Int2ObjectOpenHashMap": "KEYHASH",
    "it/unimi/dsi/fastutil/longs/Long2ObjectOpenHashMap": "KEYHASH",
    "it/unimi/dsi/fastutil/longs/LongOpenHashSet": "KEYHASH",
    "it/unimi/dsi/fastutil/longs/LongAVLTreeSet": "SORTED",
    "it/unimi/dsi/fastutil/objects/ObjectOpenHashSet": "OBJHASH",
    "it/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap": "OBJHASH",
    "it/unimi/dsi/fastutil/objects/Object2BooleanOpenHashMap": "OBJHASH",
    "it/unimi/dsi/fastutil/objects/ReferenceOpenHashSet": "IDENTITY",
    "java/util/HashMap": "OBJHASH",
    "java/util/HashSet": "OBJHASH",
    "java/util/IdentityHashMap": "IDENTITY",
    # Built by a static factory rather than a `new`. `EnumMap` walks in
    # ordinal order, which is a property of the enum's declaration and so is
    # state — vanilla picked it here on purpose, and the ArrayMaps beside it
    # for the same reason. The attribute code is carefully order-safe
    # INSIDE the instance and order-blind at the two sets that reach the wire,
    # which is the whole point of listing both here.
    "java/util/EnumMap": "SORTED",
    # A plain array. Not a container the code `new`s into a field this census
    # can find — it arrives as a constructor parameter — but it needs no such
    # derivation: an array walked by index is index-ordered, and the declared
    # type says so unambiguously, which is exactly what an interface does not.
    "<array>": "SORTED",
}

# Static factories that stand in for a `new`, mapped to what they return.
FACTORY = {
    "com/google/common/collect/Maps.newEnumMap": "java/util/EnumMap",
    "com/google/common/collect/Maps.newHashMap": "java/util/HashMap",
    "com/google/common/collect/Lists.newArrayList": "java/util/ArrayList",
    "com/google/common/collect/Sets.newHashSet": "java/util/HashSet",
}

# owner class, field, the type whose hashCode decides an OBJHASH site (or
# None), how the order reaches an observer, and whether it is iterated at all.
SITES = [
    dict(owner="net.minecraft.server.level.ChunkMap", field="entityMap",
         keytype=None, iterated_in="ChunkMap.tick",
         reaches="every per-entity broadcast in a tick, in this order",
         wire=True),
    dict(owner="net.minecraft.world.level.entity.EntityTickList", field="active",
         keytype=None, iterated_in="EntityTickList.forEach",
         reaches="the entity tick order", wire=True),
    dict(owner="net.minecraft.world.level.entity.EntityLookup", field="byId",
         keytype=None, iterated_in="EntityLookup.getAllEntities",
         reaches="any full sweep of the level's entities", wire=True),
    dict(owner="net.minecraft.world.level.entity.EntitySectionStorage",
         field="sectionIds", keytype=None,
         iterated_in="EntitySectionStorage.forEachAccessibleNonEmptySection",
         reaches="the section order of every AABB entity query — x numerically "
                 "ascending, then z and y as UNSIGNED masked fields, so the "
                 "non-negative sections come before the negative ones and a "
                 "natural nested loop is wrong for any box straddling 0",
         wire=True),
    dict(owner="net.minecraft.world.level.entity.EntitySectionStorage",
         field="sections", keytype=None, iterated_in=None,
         reaches="not iterated — reached by key through sectionIds", wire=False),
    dict(owner="net.minecraft.world.entity.ai.attributes.AttributeMap",
         field="attributes", keytype="net.minecraft.core.Holder$Reference",
         iterated_in="AttributeMap.getSyncableAttributes",
         reaches="the attribute order inside the spawn-time update_attributes",
         wire=True),
    dict(owner="net.minecraft.world.entity.ai.attributes.AttributeMap",
         field="attributesToSync",
         keytype="net.minecraft.world.entity.ai.attributes.AttributeInstance",
         iterated_in="ServerEntity.sendChanges",
         reaches="the attribute order inside an incremental update_attributes",
         wire=True),
    dict(owner="net.minecraft.world.entity.ai.attributes.AttributeInstance",
         field="modifiersByOperation",
         keytype="net.minecraft.world.entity.ai.attributes.AttributeModifier$Operation",
         iterated_in=None,
         reaches="not iterated for order — calculateValue names the three "
                 "operations itself, and the container is an EnumMap anyway",
         wire=False),
    dict(owner="net.minecraft.world.entity.ai.attributes.AttributeInstance",
         field="modifierById", keytype=None,
         iterated_in="AttributeInstance.getModifiersOrEmpty",
         reaches="the summation order inside calculateValue, and double "
                 "addition is not associative",
         wire=True),
    # `reaches` here said "the order players are walked in for every
    # broadcast" until roadmap §103, and the bytecode does not say that.
    # `ChunkMap.tick`'s derived call sequence — pinned by
    # `ci/fastutil-open-hash-walk.py` under `ChunkMap.tick call sequence` — is
    #
    #   getAllPlayers -> updateChunkTracking -> ServerLevel.players
    #     -> entityMap.values -> updatePlayers -> sendChanges
    #     -> entityMap.values -> updatePlayers
    #
    # so the player SET feeds chunk tracking, and the list `updatePlayers`
    # walks is `ServerLevel.players()`, a plain `java.util.List` field. This
    # site is a real KEYHASH order on the entity ids and it reaches chunk
    # tracking, not the entity broadcast. The correction matters because it
    # changes what porting it buys: §103 ports the walk and the second
    # consumer is one this server does not have.
    dict(owner="net.minecraft.server.level.PlayerMap", field="players",
         keytype="net.minecraft.world.entity.Entity",
         iterated_in="PlayerMap.getAllPlayers, walked by ChunkMap.tick into "
                     "updateChunkTracking",
         reaches="the order players are considered for CHUNK TRACKING, once "
                 "per tick — and `Entity.hashCode` returns `getId()`, so this "
                 "is the entity ids again rather than an object hash. NOT the "
                 "entity broadcast: `TrackedEntity.updatePlayers` is handed "
                 "`ServerLevel.players()`, which is a `java.util.List`",
         wire=True),
    dict(owner="net.minecraft.util.ClassInstanceMultiMap", field="allInstances",
         keytype=None, iterated_in="EntitySection.getEntities",
         reaches="the within-section order of every AABB entity query — the "
                 "minor key under EntitySectionStorage.sectionIds, and the "
                 "order target selection and collision resolution see",
         wire=True),
    dict(owner="net.minecraft.network.syncher.SynchedEntityData", field="itemsById",
         keytype=None, iterated_in="SynchedEntityData.packDirty",
         reaches="the field order inside every set_entity_data — the biggest "
                 "entity packet there is, and vanilla walks a flat array by "
                 "index so the order is the accessor ids and nothing else",
         wire=True),
    dict(owner="net.minecraft.server.level.ServerChunkCache",
         field="chunkHoldersToBroadcast", keytype=None,
         iterated_in="ServerChunkCache.broadcastChangedChunks",
         reaches="the order changed chunks are broadcast in", wire=True),
]


def die(msg):
    print("order-keyed-census: REFUSED — %s" % msg, file=sys.stderr)
    sys.exit(3)


def jar_path():
    p = os.environ.get("RUSTMC_JAR") or os.path.join(ROOT, JAR_REL)
    return p if os.path.exists(p) else None


_cache = {}


def javap(jar, cls):
    if cls in _cache:
        return _cache[cls]
    try:
        out = subprocess.run(["javap", "-p", "-c", "-constants", "-classpath", jar, cls],
                             capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        die("javap could not run (%s)" % e)
    if out.returncode != 0 or not out.stdout.strip():
        die("javap produced nothing for %s" % cls)
    _cache[cls] = out.stdout
    return out.stdout


def declared_array(text, field):
    """`field`'s declared type, if it is an array.

    The one case where the DECLARATION settles the order and the construction
    need not be found: `SynchedEntityData.itemsById` is a
    `DataItem<?>[]` handed in through the constructor, and `packDirty` walks
    it by index. An array walked by index is index-ordered whatever built it.
    """
    for ln in text.splitlines():
        m = re.match(r"\s+(?:private|public|protected|final|static|volatile|transient|\s)*"
                     r"([a-zA-Z0-9_.$<>?, ]+)\[\]\s+%s;\s*$" % re.escape(field), ln)
        if m:
            return "<array>"
    return None


def concrete_container(text, field):
    """The class `new`ed immediately before the putfield that assigns `field`.

    Derived rather than read off the declared type, because every one of these
    fields is declared as an interface and the interface says nothing about
    the order.
    """
    lines = text.splitlines()
    for i, ln in enumerate(lines):
        if re.search(r"putfield\s+#\d+\s+// Field %s:" % re.escape(field), ln):
            for j in range(i - 1, max(-1, i - 40), -1):
                m = re.search(r"new\s+#\d+\s+// class ([a-zA-Z0-9/$]+)", lines[j])
                if m:
                    return m.group(1)
                m = re.search(r"invokestatic\s+#\d+\s+// Method ([a-zA-Z0-9/$]+\.[a-zA-Z0-9$]+)",
                              lines[j])
                if m and m.group(1) in FACTORY:
                    return FACTORY[m.group(1)]
    return None


def overrides_hashcode(jar, cls):
    """Does `cls` declare its own `hashCode()`?

    This is what separates IDENTITY from OBJHASH, and it is the reader whose
    death would be least visible — every row would still print, three of them
    with the wrong order class. `RUSTMC_ORDER_FORCE_HASHCODE=1` forces a
    "yes" so the falsifier can watch the pinned table move.
    """
    if os.environ.get("RUSTMC_ORDER_FORCE_HASHCODE") == "1":
        return True
    try:
        out = subprocess.run(["javap", "-p", "-classpath", jar, cls],
                             capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.SubprocessError) as e:
        die("javap could not run for %s (%s)" % (cls, e))
    if out.returncode != 0 or not out.stdout.strip():
        die("javap produced nothing for key type %s" % cls)
    return bool(re.search(r"\bint hashCode\(\);", out.stdout))


def hashcode_returns_getid(jar, cls):
    """Is `cls.hashCode()` literally `return this.getId();`?

    `Entity` declares exactly that, and it changes the order class of every
    hash container keyed by an entity: what looks like an object hash is a
    hash of the ENTITY ID. `PlayerMap.players` is the instance that matters —
    the set of players a broadcast walks is ordered by their ids.
    """
    text = javap(jar, cls)
    try:
        i = text.index("  public int hashCode();")
        body = text[i:text.index("\n\n", i)]
    except ValueError:
        return False
    return bool(re.search(r"invokevirtual\s+#\d+\s+// Method getId:\(\)I\s*\n"
                          r"\s*\d+: ireturn", body))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--rewrite-pin", action="store_true")
    args = ap.parse_args()

    jar = jar_path()
    if jar is None:
        die("no pinned jar under %s" % os.path.join(ROOT, JAR_REL))

    rows = []
    for s in SITES:
        text = javap(jar, s["owner"])
        cc = concrete_container(text, s["field"]) or declared_array(text, s["field"])
        if cc is None:
            die("no construction found for %s.%s — the container reader is dead"
                % (s["owner"], s["field"]))
        order = CONTAINER_ORDER.get(cc)
        if order is None:
            die("unknown container %s at %s.%s — add it to CONTAINER_ORDER "
                "with its order class rather than guessing"
                % (cc, s["owner"], s["field"]))
        keyed_by_identity = None
        if order == "OBJHASH":
            if s["keytype"] is None:
                die("%s.%s is hash-ordered over objects and no key type is "
                    "declared, so the census cannot say whether it is IDENTITY"
                    % (s["owner"], s["field"]))
            keyed_by_identity = not overrides_hashcode(jar, s["keytype"])
            if keyed_by_identity:
                order = "IDENTITY"
            elif hashcode_returns_getid(jar, s["keytype"]):
                # Not an object hash at all in any sense that matters: the
                # object's hash IS the entity id, so this walks the same way
                # ChunkMap.entityMap does and for the same reason.
                order = "KEYHASH"
            else:
                order = "OBJHASH"
        rows.append(dict(owner=s["owner"].rsplit(".", 1)[-1], field=s["field"],
                         container=cc.rsplit("/", 1)[-1], order=order,
                         iterated_in=s["iterated_in"], wire=s["wire"],
                         reaches=s["reaches"],
                         keytype_overrides_hashcode=(None if keyed_by_identity is None
                                                     else not keyed_by_identity)))

    counts = {}
    for r in rows:
        if r["wire"]:
            counts[r["order"]] = counts.get(r["order"], 0) + 1

    report = {"sites": rows, "on_the_wire_by_order_class": counts}

    if args.rewrite_pin:
        with open(PIN, "w", encoding="utf-8") as fh:
            json.dump(report, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("rewrote %s" % PIN)
        return 0

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0

    print("order-keyed behaviour in the pinned jar — %d sites" % len(rows))
    print()
    print("  %-9s  %-41s  %-27s  %-7s  %s"
          % ("order", "owner.field", "container", "on wire", "iterated in"))
    for r in sorted(rows, key=lambda r: (r["order"], r["owner"], r["field"])):
        print("  %-9s  %-41s  %-27s  %-7s  %s"
              % (r["order"], r["owner"] + "." + r["field"], r["container"],
                 "yes" if r["wire"] else "no", r["iterated_in"] or "-"))
    print()
    for r in sorted(rows, key=lambda r: (r["order"], r["owner"])):
        print("  %s.%s -> %s" % (r["owner"], r["field"], r["reaches"]))
    print()
    print("on the wire, by order class: %s"
          % ", ".join("%s %d" % (k, v) for k, v in sorted(counts.items())))

    if args.check:
        if not os.path.exists(PIN):
            die("no pin at %s; run --rewrite-pin once and commit it" % PIN)
        pinned = json.load(open(PIN, encoding="utf-8"))
        if pinned != report:
            print("order-keyed-census: the order-keyed surface moved against its pin",
                  file=sys.stderr)
            pi = {(r["owner"], r["field"]): r for r in pinned["sites"]}
            ni = {(r["owner"], r["field"]): r for r in rows}
            for k in sorted(set(pi) | set(ni)):
                if pi.get(k) != ni.get(k):
                    print("   %s.%s: pinned %r, now %r" % (k[0], k[1], pi.get(k), ni.get(k)),
                          file=sys.stderr)
            return 1
        print("order-keyed-census: %d sites read, %d on the wire, all pinned"
              % (len(rows), sum(counts.values())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
