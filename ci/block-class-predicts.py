#!/usr/bin/env python3
"""Does a block's **class** predict its behaviour, or only its control flow?

`docs/PARITY-ROADMAP.md` §21.5 ranks block behaviour as the surface with the
most unseen vanilla, and notes that `use_without_item` scores **16 of 184
blocks but 14 of 39 classes** — so the same corpus is "4x better covered" in
one unit than the other, and the ranking of that row depends on which unit a
reader means. `ci/corpus-surface.py` produces both figures; this script asks
the question those two numbers cannot answer between them.

**The class figure is only honest if a class is one implementation.** If
`FenceGateBlock` is one method shared by twelve gates, then measuring one gate
is evidence about all twelve and the by-block figure understates the truth. If
the class instead dispatches on a per-block field, then it is not one
implementation at all, and quoting the class figure is the same misattribution
as claiming a port measured on one scenario for thirty-nine blocks.

So this script does not assume it either way. It takes the golden tables —
which record what vanilla *actually did* at every block state — groups their
rows by **declaring** class (`ci/corpus-surface.py`'s `Hierarchy`, the same
collapse that turns 47 runtime classes into 39 implementations), and compares
the members of each class against each other.

## What counts as agreement

Two members agree when their recorded answers are identical **after the
member's own block id is rewritten to `<self>`**. That rewrite is not a
convenience; without it every class in the table would trivially disagree,
because `oak_fence_gate` writes `oak_fence_gate[open=true]` where
`birch_fence_gate` writes `birch_fence_gate[open=true]`. Those are the same
rule, and a comparison that calls them different is measuring block ids.

Disagreements are then split, because the two kinds mean opposite things:

* **TARGET-DISPATCH** — the members differ *only* in a block id that is not
  their own. The control flow is shared; a per-block field picks the output.
  The class predicts the *shape* of the answer and not the answer. Covering one
  member tells you the rule fires and tells you nothing about the mapping.
* **CONTROL-DISPATCH** — the members differ in a result, a sound, a flag set,
  a consumption, a scheduled tick or the number of writes. Different behaviour
  outright. The class is not a unit of coverage at all.

## Refusals are not agreement

A cell the harness refused is unmeasured, and two members that both refuse
agree about nothing. `docs/AGENT-BRIEF.md`'s rule — a negative result is only
evidence if the mechanism was live — is mechanised here as the **UNMEASURED**
verdict: a class whose every cell is a refusal is reported as unmeasured, never
as predictive. Classes with *some* refused cells are reported predictive with
the refused fraction printed beside them, so a reader can see how much of the
agreement is carried by silence. This is the trap `docs/PARITY-ROADMAP.md` §22
found in this same table: 38 `FlowerPotBlock` rows were "constant because
*unmeasured*", and against the pre-`bindItemComponents` table this script would
have called that class predictive on 39 refusals.

## The unit that survives

The last block of output is the point. For every class the script knows whether
one member is evidence for the rest, so it can build a denominator that is
neither the block count nor the class count but the honest mixture: a
predictive class contributes **1**, a dispatching class contributes its
**member count**, and a singleton contributes 1 under every reading. Numerators
follow the same rule. That figure is quotable where neither of §21.5's two is.

Usage:
    ci/block-class-predicts.py [--jar JAR] [--table NAME]... [--quiet]
    ci/block-class-predicts.py --selftest
"""

import argparse
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
JAR = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar")


def _load_surface():
    """Import `ci/corpus-surface.py` for `Hierarchy` and `TABLE_METHOD`.

    Imported rather than reimplemented **on purpose**: two superclass walks
    that disagree would give two different class denominators for one table,
    and the whole subject of this script is that the denominator's unit has to
    be pinned down. If that file's collapse is wrong, both figures move
    together and the comparison stays honest.
    """
    path = os.path.join(HERE, "corpus-surface.py")
    spec = importlib.util.spec_from_file_location("corpus_surface", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# ---------------------------------------------------------------------------
# state strings


def parse_state(s):
    """`"minecraft:oak_fence_gate[facing=north,open=false]"` -> (id, props)."""
    if not isinstance(s, str):
        return (None, ())
    i = s.find("[")
    if i < 0:
        return (s, ())
    body = s[i + 1 :].rstrip("]")
    props = []
    for part in body.split(","):
        if not part:
            continue
        k, _, v = part.partition("=")
        props.append((k, v))
    return (s[:i], tuple(sorted(props)))


def _sub(bid, self_id):
    return "<self>" if bid == self_id else bid


# ---------------------------------------------------------------------------
# answers
#
# Each adapter turns one recorded cell into (key, answer, refused). `key` is
# what makes two members' cells comparable, `answer` is what is compared, and
# `refused` marks a cell that measured nothing.


def _answer(c, self_id):
    """One recorded cell -> (answer, refused).

    Everything vanilla was observed to do is in the tuple: the result, whether
    the stack was consumed, whether a tick was scheduled, whether a sound went
    out, what the stack became, and every write with its position, its state
    and its **flags**. Flags are in deliberately — a write with flags 3 and the
    same write with flags 10 are different behaviour and a comparison that
    ignored them would call two blocks the same implementation on the strength
    of the block they wrote.
    """
    if "refused" in c:
        return ("REFUSED",), True
    writes = tuple(
        (
            tuple(w.get("at", ())),
            _sub(parse_state(w.get("state"))[0], self_id),
            parse_state(w.get("state"))[1],
            w.get("flags"),
        )
        for w in c.get("writes", [])
    )
    stack = c.get("stackAfter")
    return (
        c.get("result"),
        c.get("consumes"),
        c.get("schedulesTick"),
        c.get("sound"),
        _sub(parse_state(stack)[0], self_id) if isinstance(stack, str) and ":" in stack else stack,
        writes,
    ), False


def cells_use_without_item(row):
    """`useWithoutItem` has no item axis, so every cell is its state's default."""
    self_id = row["block"]
    for c in row.get("states", []):
        props = parse_state(c.get("from"))[1]
        a, r = _answer(c, self_id)
        yield (props, "*"), a, r, True


def cells_use_item_on(row):
    """`use_item_on.json` records a per-state **default** plus the item groups
    that differ from it (`from` / `default` / `except[].items` / `except[].cell`).

    The item axis is materialised here rather than compared as stored, and that
    is the difference between a real answer and a shape complaint. `flower_pot`
    carries 38 `except` groups and `potted_oak_sapling` carries none; compared
    as stored those are two different-shaped rows and the comparison degenerates
    to "these look different". Compared as *functions of the item* — a member
    with no exception for item X answers X with its default — the two are
    compared at every item in the class's union, and the disagreement that
    comes back is the real one: for `oak_sapling`, the empty pot writes a
    potted block and the full pot does not.
    """
    self_id = row["block"]
    for st in row.get("states", []):
        props = parse_state(st.get("from"))[1]
        if "refused" in st:
            yield (props, "*"), ("REFUSED",), True, True
            continue
        if "default" in st:
            a, r = _answer(st["default"], self_id)
            yield (props, "*"), a, r, True
            for grp in st.get("except", []) or []:
                a, r = _answer(grp.get("cell", grp), self_id)
                for item in grp.get("items", []) or ["?"]:
                    yield (props, item), a, r, False
        else:
            a, r = _answer(st, self_id)
            yield (props, "*"), a, r, True


ADAPTERS = {
    "use_without_item": cells_use_without_item,
    "use_item_on": cells_use_item_on,
    "set_placed_by": cells_use_without_item,
}


def blur(answer):
    """The answer with every non-self block id replaced by a placeholder.

    Two members whose answers are equal under `blur` but not under the plain
    comparison differ *only* in which block they name — the TARGET-DISPATCH
    case.
    """

    def walk(x):
        if isinstance(x, tuple):
            return tuple(walk(i) for i in x)
        if isinstance(x, str) and ":" in x and x != "<self>":
            return "<blk>"
        return x

    return walk(answer)


# ---------------------------------------------------------------------------
# the measurement


class Member:
    """One block's recorded behaviour as a total function of (state, item)."""

    def __init__(self, block, adapter_out):
        self.block = block
        self.exact = {}
        self.default = {}
        self.refused = 0
        self.cells = 0
        for key, answer, is_ref, is_default in adapter_out:
            self.cells += 1
            if is_ref:
                self.refused += 1
            props, _item = key
            if is_default:
                self.default[props] = answer
            # A key reached twice with different answers would make the
            # fingerprint order-dependent; record the collision rather than
            # hide it behind a dict overwrite.
            if key in self.exact and self.exact[key] != answer:
                self.exact[key] = ("AMBIGUOUS-KEY",)
            else:
                self.exact[key] = answer

    def states(self):
        return set(self.default) | {p for p, _ in self.exact}

    def at(self, key):
        """The answer at (state, item), falling back to the state's default.

        `None` means this member has no reading for that block state at all —
        a genuine shape difference, as opposed to merely having no *exception*
        for that item.
        """
        if key in self.exact:
            return self.exact[key]
        return self.default.get(key[0])


def fingerprint(row, adapter):
    m = Member(row["block"], adapter(row))
    return m, m.refused, m.cells


def classify(members):
    """`members` is [(block_id, Member)]. Returns (verdict, witness)."""
    statesets = {frozenset(m.states()) for _, m in members}
    if len(statesets) > 1:
        sizes = sorted({len(m.states()) for _, m in members})
        return "SHAPE-DIFFER", f"members carry different block-state sets (state counts {sizes})"

    # **Sorted, not a set.** Iterating the set made the reported witness a
    # function of `PYTHONHASHSEED`: a class disagreeing at several keys named
    # a different one on different runs, so two runs of this script produced
    # two different explanations of one finding. Caught by the self-test going
    # red intermittently — and nearly missed, because the run that caught it
    # was piped through `tail` and the pipeline returned the tail's status.
    keys = sorted({k for _, m in members for k in m.exact})
    base_id, base = members[0]
    control, target = None, None
    for bid, m in members[1:]:
        for key in keys:
            a, b = base.at(key), m.at(key)
            if a == b:
                continue
            if a is None or b is None:
                return "SHAPE-DIFFER", f"{base_id} and {bid} do not share block state {key[0]}"
            if blur(a) == blur(b):
                if target is None:
                    target = (base_id, bid, key, a, b)
            elif control is None:
                control = (base_id, bid, key, a, b)
    if control:
        return "CONTROL-DISPATCH", control
    if target:
        return "TARGET-DISPATCH", target
    return "AGREE", None


def behaviour_groups(members):
    """Partition a class's members into **distinct measured behaviours**.

    This is the verdict above carried to its conclusion. `AGREE` means one
    group; a dispatching class means more than one — but *how many* more is the
    number that matters and the verdict throws it away. `FlowerPotBlock`
    control-dispatches, so charging the class figure for it is wrong; charging
    all 39 of its blocks is wrong too, because 38 of them are one behaviour
    (a full pot empties itself) and only the empty pot is the other. It is two
    implementations, and this returns the 2.

    Members are compared over the union of the class's keys through
    `Member.at`, so a member with no exception for an item is compared at its
    default rather than dropped.
    """
    keys = sorted({k for _, m in members for k in m.exact})
    groups = {}
    for bid, m in members:
        sig = tuple(m.at(k) for k in keys)
        groups.setdefault(sig, []).append(bid)
    return list(groups.values())


def witness_str(w):
    if isinstance(w, str):
        return w
    a, b, key, av, bv = w
    props, item = key
    k = ",".join(f"{n}={v}" for n, v in props) or "no properties"
    if item != "*":
        k += f"  + item {item}"
    sa, sb = str(av), str(bv)
    if len(sa) > 110:
        sa = sa[:107] + "..."
    if len(sb) > 110:
        sb = sb[:107] + "..."
    return f"{a} vs {b} at [{k}]\n         {a}: {sa}\n         {b}: {sb}"


def analyse(table_path, name, hier, adapter, held, quiet=False):
    with open(table_path) as f:
        doc = json.load(f)
    rows = doc.get("table") or doc.get("blocks") or []
    if not rows:
        print(f"  {name}: no rows")
        return None

    method = None
    surf_method = getattr(hier, "_method_for", None)
    method = surf_method(name) if surf_method else None

    groups = {}
    for r in rows:
        runtime = r.get("class")
        decl = hier.declaring(runtime, method) or runtime if method else runtime
        groups.setdefault(decl, []).append(r)

    results = []
    for cls, members in sorted(groups.items()):
        fps, refused, total = [], 0, 0
        for r in members:
            fp, ref, tot = fingerprint(r, adapter)
            fps.append((r["block"], fp))
            refused += ref
            total += tot
        if len(members) == 1:
            verdict, witness = "SINGLETON", None
        elif total and refused == total:
            verdict, witness = "UNMEASURED", f"every one of {total} cells is a refusal"
        else:
            verdict, witness = classify(fps)
        results.append(
            {
                "class": cls,
                "n": len(members),
                "blocks": [r["block"] for r in members],
                "cells": total,
                "refused": refused,
                "verdict": verdict,
                "witness": witness,
                "groups": behaviour_groups(fps),
            }
        )
    return {"name": name, "rows": len(rows), "results": results, "held": held}


def report(res, quiet=False):
    name, results = res["name"], res["results"]
    held = res["held"]
    n_blocks = sum(r["n"] for r in results)
    n_classes = len(results)

    order = {"CONTROL-DISPATCH": 0, "TARGET-DISPATCH": 1, "SHAPE-DIFFER": 2, "UNMEASURED": 3, "AGREE": 4, "SINGLETON": 5}
    counts = {}
    for r in results:
        counts[r["verdict"]] = counts.get(r["verdict"], 0) + 1

    print(f"\n== {name}: does the class predict the behaviour? ==")
    print(f"  {n_blocks} blocks in {n_classes} declaring classes")
    multi = [r for r in results if r["n"] > 1]
    print(f"  {len(multi)} classes have more than one member and are therefore testable;")
    print(f"  {n_classes - len(multi)} are singletons, where the two units are the same number")
    for v in sorted(counts, key=lambda k: order.get(k, 9)):
        print(f"    {v:18s} {counts[v]:3d} classes  ({sum(r['n'] for r in results if r['verdict']==v):4d} blocks)")

    if not quiet:
        for r in sorted(multi, key=lambda r: (order.get(r["verdict"], 9), -r["n"])):
            tag = f"{r['class']} ({r['n']} blocks -> {len(r['groups'])} behaviour"
            tag += "s" if len(r["groups"]) != 1 else ""
            tag += f", {r['cells']} cells"
            if r["refused"]:
                tag += f", {r['refused']} refused ({100*r['refused']//r['cells']}%)"
            tag += ")"
            print(f"\n  {r['verdict']:16s} {tag}")
            if len(r["groups"]) > 1:
                for g in sorted(r["groups"], key=len, reverse=True):
                    shown = ", ".join(g[:4]) + (f", ... (+{len(g)-4})" if len(g) > 4 else "")
                    print(f"         {len(g):3d}x  {shown}")
            if r["witness"]:
                print(f"       {witness_str(r['witness'])}")

    # --- the unit that survives ------------------------------------------
    #
    # A predictive class is worth 1: one member is evidence for the rest. A
    # dispatching class is worth its members: it is N implementations wearing
    # one name. An unmeasured or shape-differing class cannot be shown to
    # compress, so it is charged its members too — the conservative side, and
    # the side that cannot flatter a score.
    def covered(b):
        return b in held or b.split(":")[-1] in held

    # **An unmeasured class is charged its members, not its groups.**
    # `behaviour_groups` on a class whose every cell refused is not measuring
    # behaviour — it is measuring state shape. `SignBlock`'s 24 rows split into
    # 12 and 12 with 480 of 480 cells refused, and the split is standing signs
    # carrying `rotation` against wall signs carrying `facing`; nothing about
    # what either does was observed. Compressing those 24 to 2 would put a
    # number on the table that no measurement supports, so the conservative
    # charge applies: no evidence of compression, no compression.
    denom = num = 0
    unmeasured_blocks = 0
    for r in results:
        if r["verdict"] == "UNMEASURED":
            unmeasured_blocks += r["n"]
            denom += r["n"]
            num += sum(1 for b in r["blocks"] if covered(b))
            continue
        for g in r["groups"]:
            denom += 1
            num += 1 if any(covered(b) for b in g) else 0
    hit_blocks = sum(1 for r in results for b in r["blocks"] if covered(b))
    hit_classes = sum(1 for r in results if any(covered(b) for b in r["blocks"]))

    refused_cells = sum(r["refused"] for r in results)
    all_cells = sum(r["cells"] for r in results)

    print("\n  the corpus, in three units:")
    print(f"    by block                    {hit_blocks:4d} of {n_blocks:4d}   {100*hit_blocks/n_blocks:5.1f}%")
    print(f"    by declaring class          {hit_classes:4d} of {n_classes:4d}   {100*hit_classes/n_classes:5.1f}%")
    print(f"    by distinct behaviour       {num:4d} of {denom:4d}   {100*num/denom:5.1f}%   <- the unit that survives")
    print(
        f"\n  {refused_cells} of {all_cells} cells ({100*refused_cells/all_cells:.1f}%) are refusals, and a class that "
        "agrees\n  only across those has measured nothing — see the UNMEASURED verdict."
    )
    if unmeasured_blocks:
        print(
            f"  {unmeasured_blocks} of the {denom} in the third unit are blocks in classes measured at\n"
            "  nothing at all, charged one each because no compression was demonstrated."
        )
    # --- the work order --------------------------------------------------
    #
    # §21.5's own closing criterion is that a coverage figure which does not
    # produce a work order is decoration. Ranked by what one scenario buys:
    # covering any member of an agreeing class buys the whole class, so the
    # cheap wins are the big agreeing classes nothing has touched.
    gaps = []
    for r in results:
        if any(covered(b) for b in r["blocks"]):
            continue
        buys = 1 if r["verdict"] in ("AGREE", "SINGLETON") else len(r["groups"])
        gaps.append((r["n"], buys, r["class"], r["verdict"], r["blocks"][0]))
    gaps.sort(key=lambda g: (-g[0], g[2]))
    # **"blocks" here is the class's size, not what a scenario adds to the
    # block score.** `held` holds item ids, so covering `shulker_box` moves the
    # by-block figure by exactly one and the other sixteen shulker boxes stay
    # uncovered in that unit. What the size buys is *evidence*: in the
    # behaviour unit an agreeing class is closed by any one member, so a
    # scenario on `shulker_box` is worth all 17. Saying "buys 17 blocks" would
    # be quoting the class unit for the block one, which is the error §21.5 is
    # about.
    print("\n  uncovered classes, ranked by the evidence one scenario buys:")
    for n, buys, cls, verdict, example in gaps[:12]:
        if verdict in ("AGREE", "SINGLETON"):
            note = f"one scenario closes all {n} in the behaviour unit"
        else:
            note = f"{verdict}: closes 1 of {buys} behaviours"
        print(f"    {n:4d}-block class  {cls:32s} e.g. {example:36s} {note}")
    if len(gaps) > 12:
        print(f"    ... and {len(gaps)-12} more")

    return {
        "blocks": (hit_blocks, n_blocks),
        "classes": (hit_classes, n_classes),
        "behaviours": (num, denom),
        "refused": (refused_cells, all_cells),
        "gaps": gaps,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jar", default=JAR)
    ap.add_argument("--table", action="append", default=None)
    ap.add_argument("--quiet", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    surf = _load_surface()
    hier = surf.Hierarchy(args.jar)
    if not hier.ok:
        print(f"REFUSED  no jar at {args.jar}; the declaring-class collapse needs it")
        return 2
    hier._method_for = lambda n: surf.TABLE_METHOD.get(n)

    items = _held()
    # Through the same item -> block map `ci/corpus-surface.py` uses, so the
    # three units below cannot disagree with the two it prints.
    held = surf.placeable_blocks(items, os.path.join(ROOT, "data", "vanilla"))
    print(f"  the corpus holds {len(items)} distinct items, placing {len(held - items)} blocks under another id")

    names = args.table or ["use_without_item", "use_item_on"]
    rc = 0
    for name in names:
        p = os.path.join(ROOT, "data", "vanilla", f"{name}.json")
        if not os.path.exists(p):
            print(f"\n  {name}: absent at {p} — generate it with ci/gen-gameplay-tables.sh")
            rc = max(rc, 1)
            continue
        if name not in ADAPTERS:
            print(f"\n  {name}: no adapter; this script does not know its cell shape")
            rc = max(rc, 1)
            continue
        res = analyse(p, name, hier, ADAPTERS[name], held)
        if res:
            report(res, args.quiet)
    return rc


def _held():
    """Every item any check ever `hold`s — `ci/corpus-surface.py`'s definition.

    The corpus has no verb that places an unheld block, so this is the upper
    bound on what it can reach.
    """
    import glob

    try:
        import tomllib
    except ModuleNotFoundError:
        import tomli as tomllib

    held = set()
    for p in glob.glob(os.path.join(ROOT, "ci", "checks", "**", "*.toml"), recursive=True):
        try:
            with open(p, "rb") as f:
                t = tomllib.load(f)
        except Exception:
            continue
        for s in list(t.get("setup", [])) + list(t.get("step", [])):
            if isinstance(s, dict) and s.get("do") == "hold" and "item" in s:
                held.add(s["item"])
    return held


# ---------------------------------------------------------------------------
# self-test — fixtures only, no jar and no real table


def selftest():
    ok = True

    def expect(label, cond):
        nonlocal ok
        print(f"SELFTEST {'ok  ' if cond else 'FAIL'} [{label}]")
        ok = ok and bool(cond)

    def row(bid, cls, cells):
        return {"block": bid, "class": cls, "states": cells}

    def cell(frm, result, writes, sound=True):
        return {
            "from": frm,
            "result": result,
            "consumes": True,
            "schedulesTick": False,
            "sound": sound,
            "writes": writes,
        }

    def w(state, flags=10):
        return {"at": [0, 0, 0], "state": state, "flags": flags}

    # two gates that differ only in their own id -> AGREE
    a = row("minecraft:oak_gate", "G", [cell("minecraft:oak_gate[open=false]", "Success", [w("minecraft:oak_gate[open=true]")])])
    b = row("minecraft:birch_gate", "G", [cell("minecraft:birch_gate[open=false]", "Success", [w("minecraft:birch_gate[open=true]")])])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (a, b)]
    v, _ = classify(fps)
    expect("self-renaming makes two gates one implementation", v == "AGREE")

    # a per-block target map -> TARGET-DISPATCH, not CONTROL
    c = row("minecraft:copper", "W", [cell("minecraft:copper[x=1]", "Success", [w("minecraft:exposed")])])
    d = row("minecraft:exposed", "W", [cell("minecraft:exposed[x=1]", "Success", [w("minecraft:weathered")])])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (c, d)]
    v, wit = classify(fps)
    expect("a per-block write target is caught", v == "TARGET-DISPATCH")
    expect("target dispatch reports a witness", wit is not None)

    # a differing result -> CONTROL-DISPATCH, and it must win over target
    e = row("minecraft:p", "C", [cell("minecraft:p[x=1]", "Success", [w("minecraft:q")])])
    f = row("minecraft:r", "C", [cell("minecraft:r[x=1]", "Pass", [w("minecraft:s")])])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (e, f)]
    v, _ = classify(fps)
    expect("a differing result is control dispatch", v == "CONTROL-DISPATCH")

    # the poisoned case: agreement carried entirely by refusals
    g = row("minecraft:u", "R", [{"from": "minecraft:u[x=1]", "refused": "boom"}])
    h = row("minecraft:v", "R", [{"from": "minecraft:v[x=1]", "refused": "boom"}])
    fp1, ref1, tot1 = fingerprint(g, cells_use_without_item)
    fp2, ref2, tot2 = fingerprint(h, cells_use_without_item)
    expect("a fully-refused pair is counted as refused", ref1 + ref2 == tot1 + tot2 == 2)
    expect(
        "and it would otherwise have read as agreement",
        classify([("u", fp1), ("v", fp2)])[0] == "AGREE",
    )

    # differing state sets are reported, not silently compared
    i = row("minecraft:m", "S", [cell("minecraft:m[x=1]", "Success", [])])
    j = row("minecraft:n", "S", [cell("minecraft:n[x=1]", "Success", []), cell("minecraft:n[x=2]", "Success", [])])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (i, j)]
    v, _ = classify(fps)
    expect("members with different state sets are SHAPE-DIFFER", v == "SHAPE-DIFFER")

    # a sound difference is control flow, not a target rename
    k = row("minecraft:s1", "N", [cell("minecraft:s1[x=1]", "Success", [], sound=True)])
    l = row("minecraft:s2", "N", [cell("minecraft:s2[x=1]", "Success", [], sound=False)])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (k, l)]
    expect("a sound difference is control dispatch", classify(fps)[0] == "CONTROL-DISPATCH")

    # flags are part of the answer
    m = row("minecraft:f1", "F", [cell("minecraft:f1[x=1]", "Success", [w("minecraft:f1[x=2]", 10)])])
    n = row("minecraft:f2", "F", [cell("minecraft:f2[x=1]", "Success", [w("minecraft:f2[x=2]", 3)])])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (m, n)]
    expect("a flags difference is control dispatch", classify(fps)[0] == "CONTROL-DISPATCH")

    expect("blur leaves <self> alone", blur(("<self>", "minecraft:x")) == ("<self>", "<blk>"))

    # --- the item axis, which is where a shape complaint would hide a result -
    #
    # An empty pot has an exception for `sapling`; a full pot has none. Stored
    # shapes differ; behaviours differ too, and it is the behaviour that has to
    # come back. The control below is the same pair with the exception removed:
    # once neither member excepts anything they agree, which proves the
    # disagreement above is the exception and not the differing row length.
    def uio(bid, cls, default, excepts):
        return {
            "block": bid,
            "class": cls,
            "states": [{"from": bid, "default": default, "except": excepts}],
        }

    # The two members share a default and differ *only* at one item, so the
    # witness can only be that item if the item axis was materialised at all.
    empty = uio(
        "minecraft:pot",
        "P",
        {"result": "TRY_WITH_EMPTY_HAND", "writes": []},
        [{"items": ["minecraft:sapling"], "cell": {"result": "SUCCESS", "writes": [w("minecraft:potted_sapling", 3)]}}],
    )
    full = uio("minecraft:potted_x", "P", {"result": "TRY_WITH_EMPTY_HAND", "writes": []}, [])
    fps = [(r["block"], fingerprint(r, cells_use_item_on)[0]) for r in (empty, full)]
    v, wit = classify(fps)
    expect("an exception on one member only is a real disagreement", v == "CONTROL-DISPATCH")
    expect(
        "and the witness names the item, not the row length",
        wit and wit[2][1] == "minecraft:sapling",
    )
    # A class disagreeing at several keys must name the same one every run.
    multi_a = uio(
        "minecraft:m1",
        "M",
        {"result": "CONSUME", "writes": []},
        [{"items": ["minecraft:z"], "cell": {"result": "SUCCESS", "writes": []}}],
    )
    multi_b = uio("minecraft:m2", "M", {"result": "PASS", "writes": []}, [])
    fps = [(r["block"], fingerprint(r, cells_use_item_on)[0]) for r in (multi_a, multi_b)]
    wits = {str(classify(fps)[1]) for _ in range(8)}
    expect("a witness is deterministic across repeated classifications", len(wits) == 1)
    same = uio("minecraft:pot2", "P", {"result": "TRY_WITH_EMPTY_HAND", "writes": []}, [])
    other = uio("minecraft:pot3", "P", {"result": "TRY_WITH_EMPTY_HAND", "writes": []}, [])
    fps = [(r["block"], fingerprint(r, cells_use_item_on)[0]) for r in (same, other)]
    expect("control: two exception-free members with one default agree", classify(fps)[0] == "AGREE")

    # --- the denominator: a dispatching class is charged its behaviours, not
    # its members. One odd member out of three is 2, not 3.
    p1 = row("minecraft:a", "D", [cell("minecraft:a[x=1]", "Success", [])])
    p2 = row("minecraft:b", "D", [cell("minecraft:b[x=1]", "Success", [])])
    p3 = row("minecraft:c", "D", [cell("minecraft:c[x=1]", "Pass", [])])
    fps = [(r["block"], fingerprint(r, cells_use_without_item)[0]) for r in (p1, p2, p3)]
    g = behaviour_groups(fps)
    expect("three members with two behaviours group into two", len(g) == 2)
    expect(
        "and the majority behaviour keeps both its members",
        sorted(len(x) for x in g) == [1, 2],
    )
    expect("an agreeing class is exactly one behaviour", len(behaviour_groups(fps[:2])) == 1)
    print("SELFTEST", "GREEN" if ok else "RED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
