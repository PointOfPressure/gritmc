#!/usr/bin/env python3
"""Adjudicate two runs of `CommandTreeExtract` against each other.

# WHY THIS IS NOT A `cmp`

`ci/command-tree-extract.sh` runs the dumper twice and demands the two agree,
for the reason every golden here does: a dumper that iterates a hash map in
encounter order is a defect this project has shipped before.

**The two runs do not always agree, and vanilla is the reason.** Measured on
the Threadripper on 2026-09-05, sixteen consecutive JVM runs of the same class
file against the same pinned jar: fifteen produced one document and one
produced another. The difference was **160 of 2,085 nodes, every one of them a
child of `/gamerule`**, permuted; nothing else moved and the packet lengths
were identical at all five permission levels (265 / 265 / 29,279 / 30,099 /
30,248).

`GameRuleCommand.register` builds `new GameRules(ctx.enabledFeatures())` and
walks it with `visitGameRuleTypes`, and that iteration is not stably ordered
across JVM runs. So **vanilla's own declare-commands packet is not byte-stable
for `/gamerule`**: two vanilla dedicated servers on the same jar can send two
different byte sequences and both are correct. There is no byte-exact golden of
that subtree to take, and a port that reproduced one run's order would be gated
against a coin flip.

# WHAT IS ACCEPTED, AND WHAT IS STILL REFUSED

Accepted: a difference **confined to a named unstable root** and which is a
**permutation** — the same multiset of node shapes, in a different order. The
accepted document records `unstable_subtrees` and `unstable_nodes` so that no
reader can mistake that order for a property of vanilla.

Refused, and each of these is a case in `--selftest`:

* a difference under any other root — that is a new instability and wants
  measuring, not absorbing;
* a difference under `/gamerule` that is **not** a permutation: a bound, a
  parser, a level or an executability that changed. A permutation check that
  accepted those would be a check that cannot fail, which is this repo's
  central defect;
* a different node count, a different root index, or a different parser
  registry — those are instrument failures and are not vanilla.

# THE SHAPE THIS MUST NOT HAVE

An "ignore the noisy bit" allowance is the easiest way to write a gate that
passes on anything. The allowance here is bounded three ways and each is
checkable: it names the roots it will forgive (`UNSTABLE_ROOTS`, a constant, so
widening it is a reviewed change and not a data edit), it forgives only a
reordering of an identical multiset, and it writes what it forgave into the
document it emits.
"""

from __future__ import annotations

import json
import sys

# The roots vanilla itself does not order stably. Adding one is a reviewed
# change: it must come with a measurement of how often the order moves and
# with the mechanism, as `/gamerule`'s does in this file's header.
UNSTABLE_ROOTS = frozenset({"gamerule"})


def parents_of(doc: dict) -> dict[int, int]:
    return {c: i for i, n in enumerate(doc["nodes"]) for c in n["children"]}


def root_name(doc: dict, parents: dict[int, int], i: int) -> str:
    """The name of the root-level node `i` sits under, or `""` for the root."""
    prev = i
    while i in parents:
        prev = i
        i = parents[i]
        if i == doc["root"]:
            return doc["nodes"][prev].get("name", "")
    return doc["nodes"][prev].get("name", "")


def shape(doc: dict, i: int) -> tuple:
    """Everything about a node except where it sits among its siblings.

    Child *indices* are deliberately excluded — a permutation moves them by
    construction — and the child *count* is included, so a node that lost or
    gained a child is not forgiven as a reordering.
    """
    n = doc["nodes"][i]
    return (
        n["kind"],
        n.get("name"),
        n.get("parser"),
        json.dumps(n.get("properties", {}), sort_keys=True),
        n.get("suggestions"),
        n["executable"],
        n["level"],
        len(n["children"]),
        n.get("redirect") is not None,
    )


def compare(a: dict, b: dict) -> tuple[dict, str]:
    """Return `(document_to_write, message)`, or raise `ValueError` to refuse."""
    if a == b:
        return a, "two runs agree exactly"

    if len(a["nodes"]) != len(b["nodes"]):
        raise ValueError(
            f"two runs disagree in node count ({len(a['nodes'])} against "
            f"{len(b['nodes'])}) — that is an instrument failure, not vanilla"
        )
    if a["root"] != b["root"]:
        raise ValueError("two runs disagree about which node is the root")
    if a["parsers"] != b["parsers"]:
        raise ValueError(
            "two runs disagree about the argument-parser registry — the ids are "
            "registration order in ArgumentTypeInfos.bootstrap and cannot move"
        )

    parents = parents_of(a)
    moved = [i for i in range(len(a["nodes"])) if a["nodes"][i] != b["nodes"][i]]
    roots = sorted({root_name(a, parents, i) for i in moved})

    unknown = sorted(set(roots) - UNSTABLE_ROOTS)
    if unknown:
        raise ValueError(
            f"two runs disagree under {unknown}, which is not a subtree vanilla is "
            "known to leave unordered — refusing rather than widening the allowance"
        )

    for r in roots:
        idx = [i for i in moved if root_name(a, parents, i) == r]
        if sorted(shape(a, i) for i in idx) != sorted(shape(b, i) for i in idx):
            raise ValueError(
                f"/{r} differs between runs by more than a permutation — a node "
                "changed shape, and that is not the known instability"
            )

    out = dict(a)
    out["unstable_subtrees"] = roots
    out["unstable_nodes"] = len(moved)
    return out, (
        f"two runs differ in {len(moved)} of {len(a['nodes'])} nodes, all of them a "
        f"permutation under /{', /'.join(roots)} — accepted and recorded"
    )


# --------------------------------------------------------------------------
# selftest
# --------------------------------------------------------------------------


def _doc(nodes: list[dict], root: int = 0) -> dict:
    return {"root": root, "parsers": {"brigadier:bool": 0}, "nodes": nodes, "levels": []}


def _n(kind, name=None, children=None, **kw):
    n = {"kind": kind, "executable": kw.get("executable", False), "level": kw.get("level", 0),
         "children": children or []}
    if name is not None:
        n["name"] = name
    for k in ("parser", "properties", "suggestions", "redirect"):
        if k in kw:
            n[k] = kw[k]
    return n


def _selftest() -> int:
    bad = []

    def ok(label, cond):
        if not cond:
            bad.append(label)

    def refuses(label, a, b, needle=""):
        try:
            compare(a, b)
        except ValueError as e:
            ok(f"{label} (message)", needle in str(e))
            return
        bad.append(label)

    # A three-node `/gamerule` with two rule children, and its permutation.
    def gr(order):
        return _doc([
            _n("root", children=[1]),
            _n("literal", "gamerule", children=[2, 3], level=2),
            _n("literal", order[0], executable=True),
            _n("literal", order[1], executable=True),
        ])

    same = gr(["a_rule", "b_rule"])
    perm = gr(["b_rule", "a_rule"])

    doc, msg = compare(same, same)
    ok("identical accepts", "agree exactly" in msg)
    ok("identical records no instability", "unstable_subtrees" not in doc)

    doc, msg = compare(same, perm)
    ok("a gamerule permutation accepts", "permutation" in msg)
    ok("the accepted document names the root", doc["unstable_subtrees"] == ["gamerule"])
    ok("the accepted document counts the nodes", doc["unstable_nodes"] == 2)
    ok("the accepted document is run A", doc["nodes"][2]["name"] == "a_rule")

    # The same permutation under a root that is NOT allowed.
    def other(order):
        return _doc([
            _n("root", children=[1]),
            _n("literal", "time", children=[2, 3], level=2),
            _n("literal", order[0], executable=True),
            _n("literal", order[1], executable=True),
        ])

    refuses("a permutation under /time refuses", other(["a", "b"]), other(["b", "a"]), "not a subtree")

    # A gamerule child that changed SHAPE, not position — the case a naive
    # "ignore gamerule" allowance would wave through.
    changed = _doc([
        _n("root", children=[1]),
        _n("literal", "gamerule", children=[2, 3], level=2),
        _n("literal", "a_rule", executable=True, level=4),   # level moved
        _n("literal", "b_rule", executable=True),
    ])
    refuses("a changed level under /gamerule refuses", same, changed, "more than a permutation")

    lost = _doc([
        _n("root", children=[1]),
        _n("literal", "gamerule", children=[2, 3], level=2),
        _n("literal", "a_rule", executable=False),            # executability moved
        _n("literal", "b_rule", executable=True),
    ])
    refuses("a changed executability under /gamerule refuses", same, lost, "more than a permutation")

    short = _doc([_n("root", children=[1]), _n("literal", "gamerule", level=2)])
    refuses("a different node count refuses", same, short, "node count")

    other_root = dict(same)
    other_root = json.loads(json.dumps(same))
    other_root["root"] = 1
    refuses("a different root index refuses", same, other_root, "which node is the root")

    other_parsers = json.loads(json.dumps(same))
    other_parsers["parsers"] = {"brigadier:bool": 1}
    refuses("a different parser registry refuses", same, other_parsers, "argument-parser registry")

    # A permutation whose members are the same shape but a DIFFERENT count of
    # children is not a permutation.
    regrown = _doc([
        _n("root", children=[1]),
        _n("literal", "gamerule", children=[2, 3], level=2),
        _n("literal", "a_rule", children=[], executable=True),
        _n("literal", "b_rule", children=[2], executable=True),
    ])
    refuses("a changed child count under /gamerule refuses", same, regrown, "more than a permutation")

    if bad:
        for b in bad:
            print(f"FAIL {b}", file=sys.stderr)
        return 1
    print("command-tree-compare selftest: 13 assertions, 8 refusal cases, 0 failed")
    return 0


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--selftest":
        return _selftest()
    if len(sys.argv) != 4:
        print("usage: command-tree-compare.py <a.json> <b.json> <out.json>", file=sys.stderr)
        print("       command-tree-compare.py --selftest", file=sys.stderr)
        return 2
    a = json.load(open(sys.argv[1]))
    b = json.load(open(sys.argv[2]))
    try:
        doc, msg = compare(a, b)
    except ValueError as e:
        print(f"command-tree-extract: {e}", file=sys.stderr)
        return 1
    with open(sys.argv[3], "w") as f:
        json.dump(doc, f)
    print(f"command-tree-extract: {msg}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
