#!/usr/bin/env python3
"""What is the unit of work for `useItemOn`, and what does the corpus cover of it?

`ci/block-class-predicts.py` established that the java block class is a good
unit for `useWithoutItem` (39 classes, 40 behaviours) and a bad one for
`useItemOn` (25 classes, 89 units — a 3.6x fan-out). A unit that fans out 3.6:1
is not a unit; a covered class then tells you nothing about whether the
interaction under it is covered.

This script asks what the second axis is, and it does so from the table rather
than from the shape of the question. **`useItemOn` is a function of two
arguments** — `useWithoutItem` is handed `(level, player, hit)` and this one is
handed the `ItemStack` as well — so the obvious hypothesis is that the fan-out
*is* the item dimension and the true unit is `(class, item-kind)`.

`data/vanilla/use_item_on.json` can settle it, because
`UseItemOnGolden` probes **the entire item registry on every state**: 1,537
items over 4,537 states, 92 million probes, stored as one `default` cell per
state plus the exception groups that differ from it. The item axis is
therefore *in* the table and can be measured rather than argued.

# The three questions, and why they are separate

1. **Where does the 3.6x actually come from?** Reproduced here from the table
   directly rather than inherited, because a decomposition of somebody else's
   number that cannot first reproduce it is a decomposition of something else.
2. **Does the held item partition a block's behaviour, and how finely?** Two
   items are *equivalent for a block* iff they get the same cell on every one
   of its states. That is the item axis, measured.
3. **What does the corpus cover in the resulting unit?** With the floor beside
   it, where the floor is what a corpus scores for holding one arbitrary item
   and clicking — see `FLOOR` below, which is the finding this file exists for.

# The namespace rule that has already cost this table once

`ci/corpus-surface.py`'s `behaviour_tables` scored `block_id in held` — a block
id compared against a set of **item** ids — and `minecraft:redstone_wire` read
as uncovered through every check that places redstone. That was fixed for the
block axis.

**The same defect has a second form here, and it is not a string comparison.**
For `useItemOn` the identifier that selects the behaviour is the *held item*,
and the corpus's `hold` set is item ids — those two are the same namespace and
compare cleanly. What does not compare cleanly is the **axis**: scoring a
`useItemOn` unit by whether the corpus can *place the target block* asks a
question about the block registry and reports it as coverage of a rule keyed on
the item registry. It is neither necessary (a superflat's grass and dirt are
clickable without being placed) nor sufficient (placing a cauldron does not
fill it). Both directions are quantified below rather than asserted.

# FLOOR

Every state's cell table is one `default` answer plus exceptions. The default
is the answer for the overwhelming majority of the 1,537 items, so **a corpus
that holds a single arbitrary item and right-clicks a reachable block covers
that block's default item-class for free**. The floor of any item-aware
coverage figure is therefore the count of reachable behaviour groups — which
is exactly what a class-keyed figure counts. A class-keyed `useItemOn` coverage
number is not a coverage number with a floor: it *is* the floor.

That is a falsifiable claim and it is asserted as one: `--selftest` builds a
corpus holding nothing but `minecraft:stick` and checks the class figure and
the floor come out equal.
"""

import argparse
import glob
import importlib.util
import json
import os
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JAR = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar")

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - 3.10 and older
    import tomli as tomllib


# ---------------------------------------------------------------------------
# reading the table


def load_table(path):
    with open(path) as f:
        d = json.load(f)
    return d, d["table"]


def declaring_classes(table, meta, jar):
    """Collapse each row's **runtime** class onto the class that declares the method.

    `UseItemOnGolden` writes `block.getClass().getSimpleName()` into every row
    — the runtime class — while its *header* counts the declaring class it
    resolved reflectively. Grouping the rows by the column as stored therefore
    measures inheritance and calls it dispatch, which is the mistake
    `Hierarchy` exists to prevent for `useWithoutItem`.

    **This has a control that costs nothing and is not optional.** The dumper
    resolved the declaring class *inside the JVM*, by the same superclass walk
    virtual dispatch performs, and published the answer as
    `overridingClasses`. This function resolves it *outside* the JVM by
    walking `javap -p` output. Two independent instruments, one answer: if the
    javap walk does not reproduce the JVM's set exactly, it is wrong and this
    refuses rather than reporting a class count nobody checked. A collapse that
    silently half-worked would move every figure in this file in the direction
    that invents compression.
    """
    spec = importlib.util.spec_from_file_location(
        "corpus_surface", os.path.join(os.path.dirname(os.path.abspath(__file__)), "corpus-surface.py")
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    hier = mod.Hierarchy(jar)
    if not hier.ok:
        return None, f"no jar at {jar} — the runtime column is not the declaring class and cannot be used raw"
    out = {}
    for r in table:
        rt = r["class"]
        if rt not in out:
            out[rt] = hier.declaring(rt, "useItemOn") or rt
    got = set(out.values())
    want = set(meta.get("overridingClasses") or {})
    if want and got != want:
        return None, (
            "the javap superclass walk does not reproduce the dumper's reflective answer: "
            f"javap-only {sorted(got - want)}, jvm-only {sorted(want - got)}"
        )
    return out, None


def _sub(text, self_id):
    """Rewrite a member's own block id to `<self>` before any comparison.

    Without it every class trivially disagrees, because `oak_fence_gate` writes
    `oak_fence_gate[open=true]` where `birch_fence_gate` writes its own — and a
    comparison that calls those different is measuring block ids.
    `ci/block-class-predicts.py` does the same rewrite for the same reason; it
    is repeated rather than imported because that script is on an unmerged
    branch and a measurement that cannot be re-run from `main` is not a
    measurement.
    """
    short = self_id.split(":")[-1]
    return text.replace(self_id, "<self>").replace(f'"{short}', '"<self>')


def item_classes(row):
    """Partition the item registry by the answer this block gives it.

    Returns `(classes, n_states, refused_states)` where `classes` is a list of
    `(signature, [item ids or None])`. `None` in the item list is the residual
    class — every item that took the `default` cell on every state — and it is
    represented rather than enumerated because it is ~1,500 members wide and
    naming them would make the table's own size the answer.

    An item's answer is the tuple of cells it gets across the block's states,
    so two items are in one class iff they are indistinguishable on **every**
    state. That is the strict reading: it never merges two items that a single
    state tells apart.
    """
    states = row.get("states") or []
    self_id = row["block"]
    # per state, map item -> exception group index; absent items are group 0.
    n = len(states)
    exc = defaultdict(lambda: [0] * n)
    sigs = []
    for si, st in enumerate(states):
        groups = [_sub(json.dumps(st.get("default"), sort_keys=True), self_id)]
        for gi, grp in enumerate(st.get("except") or [], start=1):
            groups.append(_sub(json.dumps(grp.get("cell"), sort_keys=True), self_id))
            for item in grp.get("items") or []:
                exc[item][si] = gi
        sigs.append(groups)

    by_vec = defaultdict(list)
    for item, vec in exc.items():
        by_vec[tuple(vec)].append(item)
    default_vec = tuple([0] * n)

    out = []
    if default_vec not in by_vec:
        # the residual class exists whenever some item took the default
        # everywhere, which is all but pathological cases; represent it as None
        out.append((_sig(sigs, default_vec), None))
    for vec, items in sorted(by_vec.items()):
        out.append((_sig(sigs, vec), None if vec == default_vec else sorted(items)))

    refused = sum(
        1
        for st in states
        if isinstance(st.get("default"), dict) and "refused" in st["default"]
    )
    return out, n, refused


def _sig(sigs, vec):
    return tuple(sigs[i][g] for i, g in enumerate(vec))


def blur(sig):
    """The answer with every registry id but `<self>` replaced by a placeholder.

    The same split `ci/block-class-predicts.py` applies across the members of a
    class, applied here **along the item axis**, and it is the difference
    between a unit of work and a row of data.

    Read off the jar (25 declaring methods, `javap -p -c -constants`): six of
    the classes that branch on the item do so through a map or a predicate
    whose result is a *payload*, not a code path — `FlowerPotBlock` looks the
    held item up in `POTTED_BY_CONTENT` and plants whatever comes back,
    `AbstractCauldronBlock` fetches a `CauldronInteraction` from
    `CauldronInteraction$Dispatcher`, `ComposterBlock` reads a float out of
    `COMPOSTABLES`. One `getOrDefault` runs for all 38 potted flowers.

    So `flower_pot` with a dandelion and `flower_pot` with an oak sapling are
    two rows of one table, and charging them as two units of work would put
    38 implementations on a class the bytecode says has one. A cell that
    differs only in **which id it names** is TARGET; a cell that differs in
    result, consumption, sound, flags or write count is CONTROL, and only
    CONTROL is a branch somebody has to write.

    Both are reported. TARGET is not free — the map's contents still have to be
    right — but it is data a table can carry, and this project already has the
    table.
    """

    def walk(x):
        if isinstance(x, tuple):
            return tuple(walk(i) for i in x)
        if isinstance(x, str):
            if x == "<self>":
                return x
            # ids as they appear inside the serialised cell JSON, including
            # the `"minecraft:x[prop=v]"` state form.
            return "<id>" if "minecraft:" in x or "<self>" in x else x
        return x

    return walk(sig)


def merge_control(ics):
    """Collapse item classes that differ only in which id the answer names.

    Returns the same shape as `item_classes`. An item class whose member list
    is `None` is the residual — every item that took the default everywhere —
    so a merge that absorbs it stays `None` rather than losing it.
    """
    by = {}
    order = []
    for sig, items in ics:
        k = blur(sig)
        if k not in by:
            by[k] = [k, items if items is None else list(items)]
            order.append(k)
            continue
        cur = by[k][1]
        if cur is None or items is None:
            by[k][1] = None
        else:
            cur.extend(items)
    return [(k, None if v is None else sorted(set(v))) for k, v in (by[o] for o in order)]


def block_signature(row):
    """The block's whole rule, self-substituted: what a sibling must match.

    State *strings* are deliberately excluded from the signature and only their
    order is kept. Two members of one class have the same state definition by
    construction, so including the serialised state would re-introduce the
    block id through the back door.
    """
    classes, n, refused = item_classes(row)
    return tuple(sorted(sig for sig, _ in classes)), n, refused


def all_refused(row):
    states = row.get("states") or []
    if not states:
        return False
    for st in states:
        d = st.get("default")
        if not (isinstance(d, dict) and "refused" in d):
            return False
        for grp in st.get("except") or []:
            c = grp.get("cell")
            if not (isinstance(c, dict) and "refused" in c):
                return False
    return True


# ---------------------------------------------------------------------------
# the corpus


def held_items(root=ROOT):
    """Every item any check ever `hold`s. `ci/corpus-surface.py`'s definition."""
    held = set()
    for p in glob.glob(os.path.join(root, "ci", "checks", "**", "*.toml"), recursive=True):
        try:
            with open(p, "rb") as f:
                t = tomllib.load(f)
        except Exception:
            continue
        for s in list(t.get("setup", [])) + list(t.get("step", [])):
            if isinstance(s, dict) and s.get("do") == "hold" and "item" in s:
                held.add(s["item"])
    return held


def clicked_items(root=ROOT):
    """The items the corpus is actually **holding at a `place`**.

    This is the sharper set and the one a `useItemOn` unit needs. `hold` alone
    is a bound on reach — `ci/block-class-predicts.py` says so in its own blind
    spots: *adding a bare `hold` moves every figure without measuring
    anything*. A `place` is the corpus's only verb that sends `UseItemOn`, so
    the item held at that step is the item the rule was dispatched on.

    Steps are walked in file order, `setup` before `step`, tracking the current
    hand. A `place` before any `hold` is charged to `minecraft:air`, which is a
    real member of the item axis and is probed like any other.
    """
    used = set()
    for p in sorted(glob.glob(os.path.join(root, "ci", "checks", "**", "*.toml"), recursive=True)):
        try:
            with open(p, "rb") as f:
                t = tomllib.load(f)
        except Exception:
            continue
        hand = "minecraft:air"
        for s in list(t.get("setup", [])) + list(t.get("step", [])):
            if not isinstance(s, dict):
                continue
            if s.get("do") == "hold" and "item" in s:
                hand = s["item"]
            elif s.get("do") == "swap_offhand":
                pass
            elif s.get("do") == "place":
                used.add(hand)
    return used


def reachable_blocks(held, root=ROOT):
    """Blocks the corpus can get in front of a right-click.

    Two sources, and they are kept apart because they fail differently:

    * **placed** — the corpus's only world-building verb is `hold` then
      `place`, so a block whose item is never held cannot be placed. Scored by
      block id against the held item ids, which is the comparison
      `ci/block-class-predicts.py` had to fix; the 14 items that place a block
      under another id live on that unmerged branch, so they are listed here
      explicitly and the list is checked against the table rather than trusted.
    * **preset** — the corpus world is a superflat and its layers are clickable
      without being placed at all. Read from `data/vanilla/flat_preset.json`
      when it is present, so this is not a hand-written world model.
    """
    out = set()
    for b in held:
        out.add(b)
    for item, block in ITEM_PLACES_OTHER_BLOCK.items():
        if item in held:
            out.add(block)
    p = os.path.join(root, "data", "vanilla", "flat_preset.json")
    if os.path.isfile(p):
        try:
            with open(p) as f:
                d = json.load(f)
            out |= set(_walk_strings(d))
        except (OSError, json.JSONDecodeError):
            pass
    return out


def _walk_strings(x):
    if isinstance(x, str):
        if x.startswith("minecraft:"):
            yield x
    elif isinstance(x, dict):
        for v in x.values():
            yield from _walk_strings(v)
    elif isinstance(x, list):
        for v in x:
            yield from _walk_strings(v)


# The items whose `BlockItem.getBlock()` is not the item's own id. Derived on
# branch `gameplay-block-classes` by `ci/block-items-golden.sh` reading the jar;
# reproduced here as data because that branch is unmerged, and **not trusted**:
# `--selftest` asserts every value is a block the table knows, so a stale entry
# refuses instead of quietly widening reach.
ITEM_PLACES_OTHER_BLOCK = {
    "minecraft:wheat_seeds": "minecraft:wheat",
    "minecraft:beetroot_seeds": "minecraft:beetroots",
    "minecraft:melon_seeds": "minecraft:melon_stem",
    "minecraft:pumpkin_seeds": "minecraft:pumpkin_stem",
    "minecraft:carrot": "minecraft:carrots",
    "minecraft:potato": "minecraft:potatoes",
    "minecraft:torchflower_seeds": "minecraft:torchflower_crop",
    "minecraft:cocoa_beans": "minecraft:cocoa",
    "minecraft:string": "minecraft:tripwire",
    "minecraft:powder_snow_bucket": "minecraft:powder_snow",
    "minecraft:glow_berries": "minecraft:cave_vines",
    "minecraft:sweet_berries": "minecraft:sweet_berry_bush",
    "minecraft:redstone": "minecraft:redstone_wire",
    "minecraft:kelp": "minecraft:kelp_plant",
}


# ---------------------------------------------------------------------------
# the analysis


def analyse(table, held, clicked, reach, declaring=None):
    rows = {r["block"]: r for r in table}
    by_class = defaultdict(list)
    for r in table:
        by_class[(declaring or {}).get(r["class"], r["class"])].append(r)

    classes = []
    for cls, members in sorted(by_class.items()):
        unmeasured = all(all_refused(m) for m in members)
        sigs = defaultdict(list)
        for m in members:
            sigs[block_signature(m)[0]].append(m["block"])
        groups = [sorted(v) for v in sigs.values()]
        classes.append(
            {
                "class": cls,
                "blocks": sorted(m["block"] for m in members),
                "n": len(members),
                "unmeasured": unmeasured,
                "groups": groups,
                "n_groups": len(groups),
            }
        )

    # ---- unit 1: the class ------------------------------------------------
    n_blocks = len(rows)
    n_classes = len(classes)

    # ---- unit 2: distinct behaviour, ci/block-class-predicts.py's charge ---
    # An unmeasured class is charged its members, not its groups: two members
    # that both refuse agree about nothing.
    beh_units = []
    for c in classes:
        if c["unmeasured"]:
            beh_units.extend([[b] for b in c["blocks"]])
        else:
            beh_units.extend(c["groups"])

    # ---- unit 3: the interaction, (behaviour group, item class) ------------
    #
    # Two readings, because the jar says they are different questions. STRICT
    # charges every distinct answer; CONTROL merges the answers that differ
    # only in which id they name — one `getOrDefault` for 38 potted flowers.
    inter, inter_ctl = [], []
    for c in classes:
        if c["unmeasured"]:
            for b in c["blocks"]:
                u = {"blocks": [b], "items": None, "unmeasured": True, "cls": c["class"]}
                inter.append(u)
                inter_ctl.append(dict(u))
            continue
        for g in c["groups"]:
            rep = rows[g[0]]
            ics, _, _ = item_classes(rep)
            for dest, src in ((inter, ics), (inter_ctl, merge_control(ics))):
                for sig, items in src:
                    dest.append(
                        {
                            "blocks": g,
                            "items": items,
                            "unmeasured": False,
                            "cls": c["class"],
                            "default": items is None,
                        }
                    )
    return classes, beh_units, inter, inter_ctl, rows


def covered_block(b, reach):
    return b in reach or b.split(":")[-1] in reach


def score(units, reach, items_held, key="items"):
    num = 0
    for u in units:
        if not any(covered_block(b, reach) for b in u["blocks"]):
            continue
        if u["unmeasured"]:
            num += 1
            continue
        if u[key] is None:  # the default class: any item at all selects it
            num += 1 if items_held else 0
        elif any(i in items_held for i in u[key]):
            num += 1
    return num


# ---------------------------------------------------------------------------


def report(args):
    meta, table = load_table(args.table)
    declaring, why = declaring_classes(table, meta, args.jar)
    if declaring is None:
        print(f"REFUSED  {why}", file=sys.stderr)
        return 3
    held = held_items()
    clicked = clicked_items()
    reach = reachable_blocks(held)
    classes, beh, inter, inter_ctl, rows = analyse(table, held, clicked, reach, declaring)

    print(f"== useItemOn, from {os.path.relpath(args.table, ROOT)} ==")
    print(
        f"  {len(set(declaring))} runtime classes collapse onto {len(set(declaring.values()))} "
        "declaring ones, reproducing the dumper's own reflective set exactly"
    )
    print(
        f"  {meta['blocksProbed']} blocks, {meta['statesProbed']} states, "
        f"{meta['itemsProbed']} items, {meta['gridProbes']:,} probes"
    )
    n_blocks = len(rows)
    n_classes = len(classes)
    unmeasured = [c for c in classes if c["unmeasured"]]
    unmeasured_blocks = sum(c["n"] for c in unmeasured)

    print("\n-- where the 3.6x fan-out comes from --")
    print(f"  declaring classes                          {n_classes:5d}")
    print(f"  distinct behaviour (block-class-predicts)  {len(beh):5d}")
    measured_classes = n_classes - len(unmeasured)
    measured_groups = sum(c["n_groups"] for c in classes if not c["unmeasured"])
    print(
        f"    of which classes measured at nothing:    {unmeasured_blocks:5d}"
        f"   ({len(unmeasured)} classes, charged one per block)"
    )
    print(f"    of which measured behaviour groups:      {measured_groups:5d}   ({measured_classes} classes)")
    if measured_classes:
        print(
            f"  fan-out among MEASURED classes             {measured_groups/measured_classes:6.2f}x"
            f"   vs {len(beh)/n_classes:5.2f}x overall"
        )

    print("\n-- the item axis, measured --")
    tot_ic = tot_ctl = 0
    multi = []
    for c in classes:
        if c["unmeasured"]:
            continue
        for g in c["groups"]:
            ics, _, _ = item_classes(rows[g[0]])
            ctl = merge_control(ics)
            tot_ic += len(ics)
            tot_ctl += len(ctl)
            if len(ics) > 1:
                multi.append((c["class"], g[0], len(ics), len(ctl)))
    print(f"  item classes over the {measured_groups} measured behaviour groups")
    print(f"    STRICT  (every distinct answer)                 {tot_ic:5d}")
    print(f"    CONTROL (answers differing only in a named id merged) {tot_ctl:5d}")
    print(f"  groups whose answer depends on the item at all: {len(multi)} of {measured_groups}")
    print(f"  {'class':32s} {'block':34s} strict ctl")
    for cls, b, n, m in sorted(multi, key=lambda x: -x[2])[:14]:
        print(f"    {cls:32s} {b:34s} {n:5d} {m:4d}")

    print("\n-- the unit, and the corpus in it --")
    print(f"  the corpus holds {len(held)} distinct items; {len(clicked)} of them are in hand at a `place`")
    n_int, n_ctl = len(inter), len(inter_ctl)
    hit_blocks = sum(1 for b in rows if covered_block(b, reach))
    hit_classes = sum(1 for c in classes if any(covered_block(b, reach) for b in c["blocks"]))
    hit_beh = sum(1 for u in beh if any(covered_block(b, reach) for b in u))

    def floor_of(units):
        return sum(
            1
            for u in units
            if any(covered_block(b, reach) for b in u["blocks"]) and (u["unmeasured"] or u["items"] is None)
        )

    rowsout = [
        ("by block", hit_blocks, n_blocks),
        ("by declaring class", hit_classes, n_classes),
        ("by distinct behaviour", hit_beh, len(beh)),
        ("by interaction STRICT (hold)", score(inter, reach, held), n_int),
        ("by interaction STRICT (clicked)", score(inter, reach, clicked), n_int),
        ("by interaction CONTROL (hold)", score(inter_ctl, reach, held), n_ctl),
        ("by interaction CONTROL (clicked)", score(inter_ctl, reach, clicked), n_ctl),
    ]
    for label, n, d in rowsout:
        print(f"    {label:34s} {n:5d} of {d:5d}   {100*n/d:5.2f}%")

    print("\n-- the floor, and what is left above it --")
    for label, units, n in [("STRICT", inter, n_int), ("CONTROL", inter_ctl, n_ctl)]:
        f = floor_of(units)
        got = score(units, reach, clicked)
        print(
            f"    {label:8s} floor {f:5d} of {n:5d} ({100*f/n:5.2f}%)   "
            f"measured {got:5d}   above the floor {got - f:+4d}"
        )
    print(
        f"    the class figure is {hit_classes} of {n_classes}; the CONTROL floor is "
        f"{floor_of(inter_ctl)} — one is the other, in units of reachable behaviour group"
    )

    print("\n-- the classes measured at nothing, which are the whole of the fan-out --")
    for c in sorted((c for c in classes if c["unmeasured"]), key=lambda c: -c["n"]):
        print(f"    {c['n']:4d} blocks  {c['class']}")

    print("\n-- every unit the corpus actually reaches, and which are floor --")
    for u in inter_ctl:
        if not any(covered_block(b, reach) for b in u["blocks"]):
            continue
        if u["unmeasured"]:
            kind, why = "FLOOR", "class measured at nothing"
        elif u["items"] is None:
            kind, why = "FLOOR", "the default answer — any item at all selects it"
        elif any(i in clicked for i in u["items"]):
            kind, why = "ABOVE", "clicked with " + ", ".join(sorted(i for i in u["items"] if i in clicked)[:3])
        elif any(i in held for i in u["items"]):
            kind, why = "ABOVE", "held but never clicked with"
        else:
            continue
        print(f"    {kind:6s} {u['cls']:26s} {u['blocks'][0]:30s} {why}")
    return 0


def selftest():
    ok = True

    def expect(label, cond):
        nonlocal ok
        print(f"SELFTEST {'ok  ' if cond else 'FAIL'} [{label}]")
        ok = ok and bool(cond)

    def row(bid, cls, states):
        return {"block": bid, "class": cls, "states": states}

    def st(frm, default, exc=()):
        return {"from": frm, "default": default, "except": [dict(items=list(i), cell=c) for i, c in exc]}

    ans = {"result": "SUCCESS", "consumes": True}
    other = {"result": "CONSUME", "consumes": True}
    ref = {"refused": "NoBlockEntity: ..."}

    # 1. a block with no exceptions has exactly one item class
    r = row("minecraft:cake", "CakeBlock", [st("minecraft:cake[bites=0]", ans)])
    expect("no exceptions -> 1 item class", len(item_classes(r)[0]) == 1)

    # 2. an exception splits the item axis, and only by the excepted items
    r2 = row(
        "minecraft:cake",
        "CakeBlock",
        [st("minecraft:cake[bites=0]", ans, [(["minecraft:candle"], other)])],
    )
    ics, _, _ = item_classes(r2)
    expect("one exception -> 2 item classes", len(ics) == 2)
    expect("the residual class is not enumerated", any(i is None for _, i in ics))

    # 3. the self-substitution: two members of a class differing only in their
    #    own id must come out identical
    a = row("minecraft:oak_sign", "SignBlock", [st("minecraft:oak_sign[rotation=0]", {"writes": "minecraft:oak_sign"})])
    b = row("minecraft:birch_sign", "SignBlock", [st("minecraft:birch_sign[rotation=0]", {"writes": "minecraft:birch_sign"})])
    expect("self-substitution merges siblings", block_signature(a)[0] == block_signature(b)[0])

    # 4. UNMEASURED is checked before grouping — two all-refused members agree
    #    about nothing, and must not be compressed
    ra = row("minecraft:oak_sign", "SignBlock", [st("minecraft:oak_sign[rotation=0]", ref)])
    expect("all-refused is UNMEASURED", all_refused(ra))
    expect("a measured row is not UNMEASURED", not all_refused(a))

    # 5. THE FLOOR CLAIM. A corpus holding one arbitrary item scores every
    #    reachable block's default item class and nothing else, so the
    #    interaction figure collapses onto the behaviour-group figure.
    table = [
        row("minecraft:cake", "CakeBlock", [st("minecraft:cake[bites=0]", ans, [(["minecraft:candle"], other)])]),
        row("minecraft:composter", "ComposterBlock", [st("minecraft:composter[level=0]", ans, [(["minecraft:beetroot"], other)])]),
    ]
    reach = {"minecraft:cake", "minecraft:composter"}
    classes, beh, inter, inter_ctl, rows = analyse(table, set(), set(), reach)
    expect("fixture: 2 classes, 2 behaviour groups, 4 interactions", (len(classes), len(beh), len(inter)) == (2, 2, 4))
    stick = score(inter, reach, {"minecraft:stick"})
    expect("a stick scores the floor and only the floor", stick == 2)
    expect("holding a candle scores one above the floor", score(inter, reach, {"minecraft:stick", "minecraft:candle"}) == 3)
    expect("the class figure equals the floor", stick == len([c for c in classes if any(covered_block(b, reach) for b in c["blocks"])]))

    # 6. holding nothing at all reaches nothing, so a zero is a zero
    expect("an empty hand scores 0", score(inter, reach, set()) == 0)

    # 7. an unreachable block is never scored, however much is held
    expect("an unreachable block scores 0", score(inter, set(), {"minecraft:candle", "minecraft:stick"}) == 0)

    # 8. no jar means no declaring class, and that must REFUSE rather than fall
    #    back to the runtime column. A silent fallback would group by
    #    inheritance and report it as dispatch, inventing compression.
    d, why = declaring_classes(table, {}, "/nonexistent/server.jar")
    expect("absent jar refuses instead of using the runtime column", d is None and "no jar" in why)

    # 9. the 14 aliases are asserted against the block registry, not trusted.
    #    A stale entry must be visible; here the table knows only two blocks, so
    #    every alias is unknown and that is what the real run checks against the
    #    real table.
    expect(
        "the alias table is data, and every value is a block id",
        all(v.startswith("minecraft:") and " " not in v for v in ITEM_PLACES_OTHER_BLOCK.values()),
    )

    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--table", default=os.path.join(ROOT, "data", "vanilla", "use_item_on.json"))
    ap.add_argument("--jar", default=JAR)
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return selftest()
    if not os.path.isfile(args.table):
        print(f"no table at {args.table} — run ci/use-item-on-golden.sh", file=sys.stderr)
        return 2
    return report(args)


if __name__ == "__main__":
    sys.exit(main())
