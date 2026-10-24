#!/usr/bin/env python3
"""Decompose the 1,537-entry item registry into what vanilla's own source says it is.

WHY THIS EXISTS
---------------
`ci/vanilla-reach-census.py` prints `item  12 / 1537  0.78%`. The numerator is
a real reading of vanilla; the denominator is a registry size, and a registry
size is not a work queue unless every entry in it is a separate piece of work.
For block *states* that question has an exact answer — `gameplay-block-classes`
proved 32,366 of them were a Cartesian product of property domains with zero
exceptions — and nobody had asked it of items.

This asks it, out of `net.minecraft.world.item.Items` in the pinned jar and
nothing else. It is a **measurement of vanilla's source**, so it boots nothing,
needs no recording, and takes about ten seconds.

WHAT IT ESTABLISHES, AND THE ONE THING THAT MAKES IT A GATE
-----------------------------------------------------------
`Items` declares 1,207 static fields. 1,177 of them are typed `Item` and hold
exactly one registry entry each. The other **30 are typed `ColorCollection<Item>`
or `WeatheringCopperCollection<Item>`** — Java records, in vanilla's own source,
whose components ARE the family:

    ColorCollection            16 components (white .. black)
    WeatheringCopperCollection  2 x ByState, ByState 4 (unaffected .. oxidized)
                                = 8

So the registry decomposes as

    |minecraft:item| == |singleton fields| + SUM over collections of |collection|
    1537             == 1177               + 15*16 + 15*8
                     == 1177               + 240   + 120

and **this script exits non-zero if that identity does not hold**, which is the
half that can fail. It is the item-side twin of `len(states) == prod|domain|`:
not a heuristic grouping somebody chose, but vanilla declining to give the 360
members separate fields because it does not think of them separately either.

WHAT IT DOES NOT ESTABLISH
--------------------------
A construction site is not a behaviour. Two items built by the same helper with
the same class and the same `Properties` chain can still differ in the VALUES
in that chain — `Item` with `food` covers every food in the game — and this
script counts call shapes, not effects. The signature table it prints is an
upper bound on how many distinct code paths there are and a lower bound on
nothing. Read it as "1,537 ids are built at 1,207 sites in 102 shapes", which
is a statement about the registry's redundancy, and not as "there are 102
behaviours".
"""
import collections
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VER = os.environ.get("RUSTMC_VANILLA_VERSION", "26.2")
JAR = os.path.join(ROOT, "oracle-work", f"vanilla-{VER}", "versions", VER, f"server-{VER}.jar")
REPORT = os.path.join(ROOT, "oracle-work", f"vanilla-{VER}", "generated", "reports", "registries.json")

# The two collection record types, and how many registry entries each holds.
# Read off the classes themselves rather than written from memory — see
# `collection_arities`.
COLLECTIONS = {
    "ColorCollection": None,
    "WeatheringCopperCollection": None,
}


def die(msg):
    print(f"item-registry-decompose: {msg}", file=sys.stderr)
    sys.exit(2)


def javap(workdir, cls, verbose=False):
    args = ["javap", "-p", "-c", "-constants", "-cp", workdir, cls]
    if verbose:
        args = ["javap", "-p", "-v", "-cp", workdir, cls]
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        die(f"javap {cls} failed — is this JDK 25? {r.stderr.strip()[:200]}")
    return r.stdout


def collection_arities(workdir):
    """How many registry entries each collection record holds, from its own fields.

    `ColorCollection` is a flat record of 16 components. `WeatheringCopperCollection`
    is a record of two `ByState`, and `ByState` is a record of four — so 8. Counted
    rather than asserted, because a version that adds a weathering stage or a dye
    colour must move this number and not this script.
    """
    def components(cls):
        out = javap(workdir, cls)
        # A record's components are its private final instance fields.
        return len(re.findall(r"^  private final [\w.$<>,\[\] ]+ \w+;$", out, re.M))

    colors = components("net.minecraft.world.level.block.ColorCollection")
    outer = components("net.minecraft.world.level.block.WeatheringCopperCollection")
    inner = components("net.minecraft.world.level.block.WeatheringCopperCollection$ByState")
    if not colors or not outer or not inner:
        die("could not count the collection records' components — the class shape moved")
    return {"ColorCollection": colors, "WeatheringCopperCollection": outer * inner}


def bootstrap_classes(workdir):
    """InvokeDynamic index -> the class its lambda constructs, or the method it calls.

    `Items.<clinit>` names most concrete item classes through a `XItem::new`
    method reference rather than a `new`, so without the BootstrapMethods table
    every one of them reads as a plain `Item`.
    """
    out = javap(workdir, "net.minecraft.world.item.Items", verbose=True)
    tail = out[out.find("BootstrapMethods:"):]
    bsm, cur = {}, None
    for line in tail.splitlines():
        m = re.match(r"\s*(\d+):\s+#\d+", line)
        if m:
            cur = int(m.group(1))
        if cur is None or cur in bsm:
            continue
        m = re.search(r'REF_newInvokeSpecial ([\w/$]+)\."<init>"', line)
        if m:
            bsm[cur] = m.group(1).split("/")[-1]
            continue
        m = re.search(r"REF_invokeStatic ([\w/$]+)\.(\w+):", line)
        if m:
            cls, meth = m.group(1).split("/")[-1], m.group(2)
            # **`LambdaMetafactory.metafactory` is the bootstrap method, not
            # the target.** It is the first `REF_invokeStatic` in every single
            # entry, so a first-match-wins reader resolves all 275 lambda sites
            # to it and reports seven distinct item classes where there are
            # fifty-four. Caught because seven was implausible; the failure is
            # silent and produces a tidier-looking answer than the truth,
            # which is the shape worth remembering.
            if cls == "LambdaMetafactory":
                continue
            bsm[cur] = f"lambda:{meth}" if meth.startswith("lambda$") else f"{cls}.{meth}"
    return bsm


def parse_clinit(workdir, bsm):
    """One row per `putstatic` in `Items.<clinit>`: the field and how it was built."""
    out = javap(workdir, "net.minecraft.world.item.Items")
    body = out[out.find("  static {};"):]
    if len(body) < 1000:
        die("Items.<clinit> is missing or tiny — the class shape moved")

    PUT = re.compile(r"putstatic\s+#\d+\s+// Field ([A-Z0-9_]+):(\S+)")
    REG = re.compile(r"invokestatic\s+#\d+\s+// Method (\w*[Rr]egister\w*):\(")
    NEW = re.compile(r"\bnew\s+#\d+\s+// class ([\w/$]+)")
    IDY = re.compile(r"invokedynamic #\d+,\s+0\s+// InvokeDynamic #(\d+):")
    PROP = re.compile(r"invokevirtual\s+#\d+\s+// Method net/minecraft/world/item/Item\$Properties\.(\w+):")

    rows, cur = [], dict(news=[], reg=None, props=[], idy=[])
    for line in body.splitlines():
        m = NEW.search(line)
        if m:
            c = m.group(1).split("/")[-1]
            if c != "Item$Properties":
                cur["news"].append(c)
        m = PROP.search(line)
        if m:
            cur["props"].append(m.group(1))
        m = IDY.search(line)
        if m:
            cur["idy"].append(bsm.get(int(m.group(1)), "?"))
        m = REG.search(line)
        if m:
            cur["reg"] = m.group(1)
        m = PUT.search(line)
        if m:
            cur["field"], cur["type"] = m.group(1), m.group(2)
            rows.append(cur)
            cur = dict(news=[], reg=None, props=[], idy=[])
    return rows


def places_a_block(workdir, cls, _cache={}):
    """Does this concrete item class place a block — i.e. does it extend `BlockItem`?

    Walked rather than pattern-matched on the name. `SignItem`, `BedItem` and
    `ItemNameBlockItem` all place blocks and none of them is called
    `*BlockItem` in a way a name test would catch reliably, and `BlockItem`
    itself is the root.

    This exists to be **cross-checked against a measurement taken by a
    completely different route**: `ci/block-items-golden.sh` on branch
    `gameplay-block-classes` calls `BlockItem.getBlock()` on the pinned jar for
    all 1,537 items and reports 1,054 that place a block, 14 of them under a
    different id. That figure is a runtime reading; this one is a static walk
    of `Items.<clinit>` and the class hierarchy. They are independent, so
    agreement is corroboration and disagreement is a finding — which is the
    only reason to compute a number somebody else has already measured.
    """
    if cls in _cache:
        return _cache[cls]
    seen, cur = set(), cls
    while cur and cur not in seen:
        seen.add(cur)
        if cur == "BlockItem":
            _cache[cls] = True
            return True
        try:
            out = javap(workdir, f"net.minecraft.world.item.{cur}")
        except SystemExit:
            _cache[cls] = False
            return False
        m = re.search(r"^(?:public |final |abstract )*class [\w.$]+ extends ([\w.$]+)", out, re.M)
        cur = m.group(1).split(".")[-1] if m else None
    _cache[cls] = False
    return False


def concrete_class(r):
    named = [x for x in r["idy"] if x not in ("?",) and not x.startswith("lambda:")]
    if r["news"]:
        return r["news"][0]
    if named:
        return named[0]
    if r["reg"] == "registerSpawnEgg":
        return "SpawnEggItem"
    if r["reg"] and r["reg"].startswith("registerBlock"):
        return "BlockItem"
    return "Item"


def main():
    if not os.path.exists(JAR):
        die(f"no jar at {JAR}")
    if not os.path.exists(REPORT):
        die(f"no registry report at {REPORT} — run ci/vanilla-server.sh reports")
    if not shutil.which("javap"):
        die("no javap on PATH")

    ids = set(json.load(open(REPORT))["minecraft:item"]["entries"])

    work = tempfile.mkdtemp(prefix="item-decomp-")
    try:
        with zipfile.ZipFile(JAR) as z:
            for n in z.namelist():
                if n.startswith(("net/minecraft/world/item/", "net/minecraft/world/level/block/Color",
                                 "net/minecraft/world/level/block/Weathering")) and n.endswith(".class"):
                    z.extract(n, work)
        arity = collection_arities(work)
        rows = parse_clinit(work, bootstrap_classes(work))
        rc = main_report(rows, arity, ids, work)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return rc


def main_report(rows, arity, ids, work):
    singles = [r for r in rows if r["type"].endswith("Lnet/minecraft/world/item/Item;")]
    colls = [r for r in rows if r not in singles]
    by_kind = collections.Counter()
    for r in colls:
        for k in arity:
            if k in r["type"]:
                by_kind[k] += 1

    covered = len(singles) + sum(arity[k] * n for k, n in by_kind.items())

    print(f"== the item registry, decomposed out of Items.<clinit> ==")
    print(f"  registry entries (minecraft:item)          {len(ids):6d}")
    print(f"  static fields in Items                     {len(rows):6d}")
    print(f"    of which singleton `Item`                {len(singles):6d}   one registry entry each")
    for k, n in sorted(by_kind.items()):
        print(f"    of which {k:26s}{n:6d} x {arity[k]:2d} = {n * arity[k]:5d}")
    print()
    print(f"  singletons + collection members            {covered:6d}")
    print(f"  registry entries                           {len(ids):6d}")

    if covered != len(ids):
        print()
        print(f"  IDENTITY FAILED by {covered - len(ids):+d}. The registry is NOT")
        print( "  'one field each, plus whole collections'. Something is registered")
        print( "  outside Items, or a collection's arity moved. This is the half of")
        print( "  this script that can fail, and it just did.")
        return 1

    print(f"  IDENTITY HOLDS — zero exceptions.")
    print()
    print( "  So 1,537 is 1,177 things vanilla names one at a time and 30 it names")
    print( "  as families. The 360 members of those 30 are a colour axis and a")
    print( "  weathering axis crossed with a family, and vanilla does not give them")
    print( "  separate fields because it does not treat them separately.")
    print()

    sig = collections.Counter()
    for r in rows:
        sig[(r["reg"], concrete_class(r), tuple(sorted(set(r["props"]))))] += 1
    cls = collections.Counter(concrete_class(r) for r in rows)
    helper = collections.Counter(r["reg"] or "NONE" for r in rows)

    print(f"== construction shapes over the {len(rows)} sites ==")
    print(f"  distinct (helper, class, Properties-chain)  {len(sig):6d}")
    print(f"  distinct concrete Item classes              {len(cls):6d}")
    print()
    print( "  registration helper:")
    for k, v in helper.most_common():
        print(f"    {v:6d}  {k}")
    print()
    print( "  the ten commonest shapes:")
    for (h, c, p), v in sig.most_common(10):
        print(f"    {v:6d}  {h}/{c}{'/' + ','.join(p) if p else ''}")
    print()
    print(f"  shapes occurring exactly once               {sum(1 for v in sig.values() if v == 1):6d}")
    print()
    print( "  A shape is a CALL SHAPE, not a behaviour: `Item` with a `food` chain")
    print( "  covers every food in the game and they do not all do the same thing.")
    print( "  Read this as how redundant the registry is, never as a behaviour count.")
    print()

    # ------------------------------------------------------------------
    # THE HALF OF THE REGISTRY THAT IS THE BLOCK REGISTRY WEARING A HAT.
    # ------------------------------------------------------------------
    def ids_of(r):
        for k, n in arity.items():
            if k in r["type"]:
                return n
        return 1

    resolved = [r for r in rows if places_a_block(work, concrete_class(r))]
    placing = sum(ids_of(r) for r in resolved)
    unresolved = [r for r in rows if r not in singles]
    print("== how many of the 1,537 place a block ==")
    print(f"  ids this route can prove extend BlockItem  {placing:6d}")
    print(f"  collection sites it CANNOT resolve         {len(unresolved):6d}"
          f"   holding {sum(ids_of(r) for r in unresolved)} ids")
    print()
    print( "  **This route stops here, deliberately, and the number above is a floor")
    print( "  rather than an answer.** A collection registers its members through a")
    print( "  `BiFunction` handed to `ColorCollection.registerItems`, so the item")
    print( "  class never appears at the `putstatic` and a walk of `Items.<clinit>`")
    print( "  cannot see it. Guessing it from the family name would put a number here")
    print( "  that looks like the others and was arrived at differently.")
    print()
    print( "  THE MEASURED FIGURE IS 1054, and it is not this script's.")
    print( "  `ci/block-items-golden.sh` on branch `gameplay-block-classes` calls")
    print( "  `BlockItem.getBlock()` on the pinned jar for all 1,537 items: 1054 place")
    print( "  a block, 14 of them under a different id. That is a runtime reading of")
    print( "  the objects themselves and it is strictly better evidence than anything")
    print( "  recoverable from the bytecode of the registration walk. Quote that one.")
    print()
    print( "  What the two agree on is the shape, and the shape is the finding: about")
    print( "  two thirds of the item registry is `place this block`, parameterised by")
    print( "  a block. Those ids carry no item mechanism of their own — the behaviour")
    print( "  is the BLOCK's, and `gameplay-block-placement` has already replaced the")
    print( "  32,366 block states with 116 property-set schemas. Counting them again")
    print( "  from the item side is counting the same work twice with a bigger number")
    print( "  on it.")
    print()

    # ------------------------------------------------------------------
    # THE UNIT THAT REPLACES 1,537.
    # ------------------------------------------------------------------
    # Singleton sites only. The 30 collections are excluded rather than
    # bundled in: this route cannot tell whether a collection places a block
    # (see above), so counting their 360 ids here would put 312 block items
    # into a total labelled "not block items" and quietly double the answer.
    nonblock = [r for r in rows if r not in resolved and r in singles]
    eggs = [r for r in nonblock if r["reg"] == "registerSpawnEgg"]
    own = [r for r in nonblock if r not in eggs]
    nb_cls = collections.Counter(concrete_class(r) for r in own)
    nb_sig = collections.Counter(
        (r["reg"], concrete_class(r), tuple(sorted(set(r["props"])))) for r in own
    )
    print("== what is left once the block half is removed ==")
    print(f"  singleton sites, not BlockItem             {len(nonblock):6d}   one id each")
    print(f"    of which registerSpawnEgg                {len(eggs):6d}   one code path x an EntityType")
    print(f"    carrying an item mechanism of their own  {len(own):6d}")
    print(f"  distinct concrete classes among those      {len(nb_cls):6d}")
    print(f"  distinct call shapes among those           {len(nb_sig):6d}")
    print()
    print( "  Plus the 3 collection families that are not block items — dye, bundle")
    print( "  and harness, 16 ids each — for about 411 ids in the whole registry that")
    print( "  carry item behaviour rather than block behaviour or an entity id.")
    print()
    print( "  THE UNIT THAT REPLACES 1,537, stated as the sum it actually is:")
    print()
    print( "     ~1054  place a block   -> already counted, as 116 block schemas (PR 41)")
    print( "        88  spawn eggs      -> one code path, parameterised by an EntityType")
    print( "       360  in 30 families  -> 30 units (PR 44.1), of which most are also block items")
    print( "     the remainder          -> the only ids carrying an item mechanism of their own")
    print()
    print( "  The three groups overlap — most collection members are block items — so")
    print( "  these do not add to 1,537 and are NOT presented as a partition. What is")
    print( "  robust is the direction: every one of the three is a large block of ids")
    print( "  that is one unit of work, and none of them is 1,537 units.")
    print()
    print( "  What this CANNOT give you is the behaviour count. PARITY-ROADMAP 39")
    print( "  measured that a class-level unit is honest for `useWithoutItem` (39")
    print( "  classes, 40 behaviours) and dishonest for `useItemOn` (25 classes, 89")
    print( "  units) because refusals are not agreement. The item-side equivalent")
    print( "  needs a golden sweep, not a static read. Until one exists, quote the")
    print( "  decomposition and not a coverage percentage over it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
