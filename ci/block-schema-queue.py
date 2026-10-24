#!/usr/bin/env python3
"""The 116 block-property schemas as a work queue, ranked, with two numerators.

`ci/vanilla-reach-census.py` prints `block_state  87 / 32366  0.27%` and that
figure is an artefact. **For every one of the 1,196 blocks in the pinned jar,
`len(states)` is exactly the product of its property-domain sizes, with zero
exceptions** — nothing is pruned and no state is unreachable by construction —
so 32,366 is a Cartesian product and holds no information beyond the list of
distinct property-set signatures and how many blocks share each. There are
**116** of those. The 32 wall blocks alone are 10,368 states, 32.0% of the
whole registry, out of one `WallBlock.makeWallState` and one
`shouldRaisePost`; a corpus that placed every wall in every state would have
written 10,368 checks for two branches' worth of behaviour.

That identity is asserted by this script rather than described, so a future
registry that prunes a state table refuses here instead of quietly reporting a
unit that has stopped meaning what it says. `docs/PARITY-ROADMAP.md` §48.

WHY THERE ARE TWO NUMERATORS AND NOT ONE
-----------------------------------------

The obvious replacement unit — count a schema reached when some recording
carried some state id of some block with that signature — is *also* pumpable,
in exactly the way the 1,196-block placement sweep is pumpable. Placing one
copper grate marks the 33-block `waterlogged` schema without the corpus ever
having put water anywhere. Placing one oak log marks the 58-block `axis` schema
without any axis but the one having existed. It is a *placement* reach wearing
a property-schema label, and a sweep holding one member of each signature would
take it most of the way to 116 without exercising a single property.

So, same denominator, two numerators:

  touched   >= 1 distinct state of the schema seen
  varied    >= 2 distinct states of the schema seen     (the property moved)

`varied` cannot be pumped by a placement. To move it, some recording has to
contain the same signature at two different property tuples, which means the
corpus made a property take a second value and vanilla broadcast it.

MEASURED, WITH THE FLOOR BESIDE IT
-----------------------------------

Six cold vanilla runs of one corpus, `~/rustmc-stackdec/oracle-work/
gameplay-selfcheck/r[1-6].json`, 2026-08-12, Threadripper:

    touched   30 / 116   25.86%      identical in all six runs
    varied    19 / 116   16.38%      identical in all six runs

**The floor is 0 at N=6, and the positive control is that the six recordings
are six different files** — distinct md5s, differing in counters and timing —
which each reach exactly 79 distinct block state ids. So the set of block
states a cold vanilla produces for a fixed corpus is deterministic, and any
movement of one schema in this figure is real rather than sampling. That is a
better resolving power than most numbers in this project have, and it is the
reason a +1 here may be quoted at all.

Do not compare across corpora. `~/rustmc-cov2` (N=6, 2026-08-10) reads 31/116
and `~/rustmc-twin` (N=6, 2026-08-09) reads 29/116; those are three different
scenario sets, not three samples of one. Use `--exclude-scenario` to take a
before and an after **out of one recording** rather than differencing two runs.

WHAT THE QUEUE SAYS, AND IT IS NOT "GO AND PLACE 85 MORE BLOCKS"
----------------------------------------------------------------

Eleven schemas are touched at exactly one state, and three of them are the
largest property-carrying signatures in the whole registry:

    58 blocks  axis                only ever axis=x       030-place-log-axis
    44 blocks  facing (4-value)    only ever facing=east   torch-on-a-wall
    21 blocks  lit                 only ever lit=true      lever, redstone torch

123 blocks whose schema is satisfied by a constant. A server whose
`RotatedPillarBlock` returns `Axis.X` unconditionally, or that lights a lamp
and never un-lights it, passes today's corpus. `--unvaried` prints the list
with the one witnessing state and the scenario that produced it.

THE EMPTY SCHEMA IS A PERMANENT REMAINDER AND IS NAMED AS ONE
-------------------------------------------------------------

The largest signature by block count is the one with **no properties at all**:
399 blocks, 34.4% of the block registry, one schema of the 116. It is counted
`touched` and always will be — any propertyless block seen anywhere sets it,
and in every recording checked the witness is `minecraft:air`, from
`break-block`. It can never be `varied`: a schema with no properties has no
second state. As a property-schema work item it is empty, and covering it is a
check that cannot fail.

It is worse than empty. It collapses 399 blocks with 399 different placement
and break behaviours into one satisfied bit, so the entry that looks largest in
this queue is the one carrying no information at all. `--exclude-empty` removes
it from both numerator and denominator; note that this **lowers** the touched
percentage, because the entry deleted was already scored as a win.

USAGE
-----

    ci/block-schema-queue.py                            # rank, no numerator
    ci/block-schema-queue.py --recordings '<glob>'      # rank and score
    ci/block-schema-queue.py --unvaried                 # single-state schemas
    ci/block-schema-queue.py --exclude-scenario <name>  # before, from the after
    ci/block-schema-queue.py --selftest                 # falsification

With no `--recordings` the script ranks and prints no numerator at all, rather
than inventing one.
"""

import argparse
import collections
import glob
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPORTS = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "generated", "reports")


def die(msg):
    print(f"block-schema-queue: {msg}", file=sys.stderr)
    return 2


def load_blocks(path):
    """blocks.json -> (schema_of, sid_block, sid_props), with §41's identity asserted.

    The Cartesian identity is re-asserted here rather than trusted from the
    census, because this script is the one that ranks schemas and a pruned
    state table would silently change the ranking as well as the denominator.
    Same refusal, same reason: see `block_schema_reach` in
    ci/vanilla-reach-census.py.
    """
    bj = json.load(open(path))
    schema_of, sid_block, sid_props = {}, {}, {}
    unpruned = True
    for name, v in bj.items():
        props = v.get("properties", {})
        prod = 1
        for p, vals in props.items():
            prod *= len(vals)
        if len(v["states"]) != prod:
            unpruned = False
        schema_of[name] = tuple(sorted((p, tuple(sorted(w))) for p, w in props.items()))
        for st in v["states"]:
            sid_block[st["id"]] = name
            sid_props[st["id"]] = tuple(sorted(st.get("properties", {}).items()))
    if not unpruned:
        raise SystemExit(die(
            "blocks.json no longer satisfies states == product(domains) — the "
            "ranking and the denominator both rest on that identity "
            "(docs/PARITY-ROADMAP.md §41); re-derive before quoting either"))
    return bj, schema_of, sid_block, sid_props


def harvest(patterns):
    """state ids the recordings carry, and the scenario that first carried each.

    Only `block_update` and `section_blocks_update` carry a state id. The
    census's docstring correction applies here in full and is repeated because
    it is the commonest misreading of this whole family of numbers: vanilla
    sends these when a block **changes**, so an unseen schema is one the corpus
    never caused a change of, NOT one the corpus's worlds did not contain.
    `dirt` and `bedrock` sit under every rig in the corpus and score zero.
    """
    files = []
    for pat in patterns:
        files.extend(sorted(glob.glob(os.path.expanduser(pat))))
    if not files:
        raise SystemExit(die(f"no recordings matched {patterns!r}"))
    by_state = collections.defaultdict(set)
    for fp in files:
        d = json.load(open(fp))
        for s in d.get("scenarios", []):
            nm = s.get("name", "?")
            for o in s.get("obs", ()):
                k, f = o.get("kind"), o.get("fields", {})
                if k == "block_update":
                    ids = [f.get("state_id")]
                elif k == "section_blocks_update":
                    ids = [b.get("state_id") for b in f.get("blocks", ())]
                else:
                    continue
                for i in ids:
                    if i is not None:
                        by_state[i].add(nm)
    if not by_state:
        raise SystemExit(die(
            f"{len(files)} recordings carried no block state ids at all — the "
            "field names have moved, or these are not recorder outputs"))
    return files, by_state


def project(by_state, drop):
    """State ids -> a witnessing scenario, with `drop` scenarios subtracted.

    A state id survives only if some scenario OTHER than the dropped ones
    produced it. This is how a before and an after are taken from **one**
    recording instead of from two runs: the cross-corpus spread in this
    script's docstring is 2 schemas wide, which is larger than a plausible
    result, so differencing two runs would be measuring the corpora.

    A dropped name that matches no scenario is a refusal, not a no-op — the
    commonest way to get a flattering before is to misspell the thing you
    meant to subtract and subtract nothing.
    """
    drop = set(drop or ())
    if drop:
        present = {n for names in by_state.values() for n in names}
        missing = drop - present
        if missing:
            raise SystemExit(die(
                "--exclude-scenario named "
                f"{', '.join(sorted(missing))}, which no recording contains; "
                "subtracting nothing would report the after as the before"))
    return {sid: sorted(names - drop)[0]
            for sid, names in by_state.items() if names - drop}


def rank(bj, schema_of, sid_block, sid_props, seen):
    """One row per schema: block count, state count, signature, and reach."""
    members = collections.defaultdict(list)
    for name, s in schema_of.items():
        members[s].append(name)
    states_at = collections.defaultdict(set)
    witness = {}
    for sid, scen in (seen or {}).items():
        name = sid_block.get(sid)
        if name is None:
            continue
        s = schema_of[name]
        states_at[s].add(sid_props[sid])
        witness.setdefault(s, []).append((name, sid_props[sid], scen))
    rows = []
    for s, blocks in members.items():
        rows.append({
            "schema": s,
            "blocks": len(blocks),
            "states": sum(len(bj[b]["states"]) for b in blocks),
            "members": sorted(blocks),
            "sig": " ".join(p for p, _ in s),
            "empty": len(s) == 0,
            "distinct_seen": len(states_at.get(s, ())),
            "witness": witness.get(s, []),
        })
    rows.sort(key=lambda r: (-r["blocks"], -r["states"], r["sig"]))
    return rows


def figures(rows, exclude_empty):
    keep = [r for r in rows if not (exclude_empty and r["empty"])]
    den = len(keep)
    touched = sum(1 for r in keep if r["distinct_seen"] >= 1)
    varied = sum(1 for r in keep if r["distinct_seen"] >= 2)
    return touched, varied, den


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--blocks", default=os.path.join(REPORTS, "blocks.json"))
    ap.add_argument("--recordings", action="append", default=[],
                    help="glob of vanilla recorder outputs; repeatable. "
                         "Omit to rank without scoring.")
    ap.add_argument("--top", type=int, default=0, help="print only the first N rows")
    ap.add_argument("--unvaried", action="store_true",
                    help="print only schemas seen at exactly one state")
    ap.add_argument("--exclude-empty", action="store_true",
                    help="drop the propertyless schema from both numerator and denominator")
    ap.add_argument("--exclude-scenario", action="append", default=[],
                    help="score as if this scenario were not in the corpus; "
                         "repeatable. Takes a before out of the after's own "
                         "recording rather than differencing two runs.")
    ap.add_argument("--json", help="write the queue as JSON to this path")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return selftest()
    if not os.path.exists(args.blocks):
        return die(f"{args.blocks} is absent — this needs the pinned jar's report")

    bj, schema_of, sid_block, sid_props = load_blocks(args.blocks)
    files, seen = ([], None)
    if args.recordings:
        files, by_state = harvest(args.recordings)
        seen = project(by_state, args.exclude_scenario)
    elif args.exclude_scenario:
        return die("--exclude-scenario needs --recordings to subtract from")
    rows = rank(bj, schema_of, sid_block, sid_props, seen)

    print(f"== {len(rows)} block-property schemas over {len(schema_of)} blocks "
          f"and {sum(r['states'] for r in rows)} states ==")
    if seen is None:
        print("   (no --recordings given: ranking only, no numerator)")
    else:
        print(f"   scored over {len(files)} recording(s), N={len(files)}; "
              f"{len(seen)} distinct state ids seen")
        if args.exclude_scenario:
            print(f"   scored AS IF absent: {', '.join(sorted(args.exclude_scenario))}")
    print()

    show = rows
    if args.unvaried:
        show = [r for r in rows if r["distinct_seen"] == 1]
    if args.top:
        show = show[:args.top]

    print(f"  {'#':>3} {'blocks':>6} {'states':>6} {'seen':>4}  {'reach':<8} signature")
    for i, r in enumerate(rows, 1):
        if r not in show:
            continue
        if seen is None:
            reach = "-"
        elif r["distinct_seen"] >= 2:
            reach = "VARIED"
        elif r["distinct_seen"] == 1:
            reach = "touched"
        else:
            reach = ""
        sig = r["sig"] or "(NO PROPERTIES)"
        ex = r["members"][0].split(":")[-1]
        print(f"  {i:3d} {r['blocks']:6d} {r['states']:6d} {r['distinct_seen']:4d}  "
              f"{reach:<8} {sig[:52]:52s} e.g. {ex}")
        if args.unvaried and r["witness"]:
            nm, props, scen = r["witness"][0]
            print(f"      ^ only {nm.split(':')[-1]}{dict(props) or ''} — from `{scen}`")
    print()

    if seen is not None:
        for label, xe in (("all 116", False), ("excluding the empty schema", True)):
            if xe and not any(r["empty"] for r in rows):
                continue
            t, v, den = figures(rows, xe)
            print(f"  {label:28s} touched {t:3d}/{den}  {100*t/den:6.2f}%    "
                  f"varied {v:3d}/{den}  {100*v/den:6.2f}%")
        print()
        print("  `touched` is the unit a placement can pump — one member block of the")
        print("  signature, in any state. `varied` needs the property to have moved.")
        print("  The empty schema is permanently touched and can never be varied — it is a")
        print("  remainder, not a target (docs/PARITY-ROADMAP.md §48).")

    if args.json:
        out = [{k: v for k, v in r.items() if k != "schema"} for r in rows]
        for r, src in zip(out, rows):
            r["properties"] = [{"name": p, "values": list(w)} for p, w in src["schema"]]
            r["witness"] = [{"block": b, "state": dict(p), "scenario": s}
                            for b, p, s in src["witness"][:4]]
        json.dump({"recordings": files, "schemas": out}, open(args.json, "w"), indent=1)
    return 0


def selftest():
    """Falsification: every claim this script makes has to be breakable.

    The cases are chosen against the two ways this instrument could be wrong in
    the direction that flatters the project: counting a schema reached that is
    not, and failing to separate `varied` from `touched`.
    """
    fails = []

    def check(name, cond):
        print(f"{'ok  ' if cond else 'FAIL'} {name}")
        if not cond:
            fails.append(name)

    # A two-block toy registry: one propertyless, one with a single boolean.
    bj = {
        "t:plain": {"states": [{"id": 0}]},
        "t:flag": {"properties": {"lit": ["false", "true"]},
                   "states": [{"id": 1, "properties": {"lit": "false"}},
                              {"id": 2, "properties": {"lit": "true"}}]},
        "t:flag2": {"properties": {"lit": ["false", "true"]},
                    "states": [{"id": 3, "properties": {"lit": "false"}},
                               {"id": 4, "properties": {"lit": "true"}}]},
    }
    import tempfile
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as fh:
        json.dump(bj, fh)
        p = fh.name
    b, schema_of, sid_block, sid_props = load_blocks(p)
    check("two schemas over three blocks", len(set(schema_of.values())) == 2)

    rows = rank(b, schema_of, sid_block, sid_props, {1: "s"})
    r = {x["sig"]: x for x in rows}
    check("one state of `lit` is touched, not varied", r["lit"]["distinct_seen"] == 1)
    t, v, den = figures(rows, False)
    check("touched 1/2, varied 0/2", (t, v, den) == (1, 0, 2))

    # The discriminating case: a SECOND BLOCK at the SAME state must not vary
    # the schema — otherwise `varied` collapses back into `touched` and the
    # whole point of the unit is lost.
    rows = rank(b, schema_of, sid_block, sid_props, {1: "s", 3: "s"})
    t, v, den = figures(rows, False)
    check("a second member at the same state does NOT vary the schema", v == 0)

    rows = rank(b, schema_of, sid_block, sid_props, {1: "s", 2: "s"})
    t, v, den = figures(rows, False)
    check("the same block at two states DOES vary it", v == 1)

    # The empty schema cannot be varied, at any input.
    rows = rank(b, schema_of, sid_block, sid_props, {0: "s", 1: "s", 2: "s"})
    e = [x for x in rows if x["empty"]][0]
    check("the empty schema is touched and never varied", e["distinct_seen"] == 1)
    t, v, den = figures(rows, True)
    check("--exclude-empty drops it from numerator and denominator", (t, v, den) == (1, 1, 1))

    # A pruned state table must refuse, not report.
    bad = {"t:x": {"properties": {"a": ["0", "1"], "b": ["0", "1"]},
                   "states": [{"id": 0, "properties": {"a": "0", "b": "0"}}]}}
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as fh:
        json.dump(bad, fh)
        q = fh.name
    try:
        load_blocks(q)
        check("a pruned state table refuses", False)
    except SystemExit:
        check("a pruned state table refuses", True)

    # Recordings that carry no state ids must refuse rather than report 0/116.
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as fh:
        json.dump({"scenarios": [{"name": "n", "obs": [{"kind": "sound", "fields": {}}]}]}, fh)
        rp = fh.name
    try:
        harvest([rp])
        check("recordings with no state ids refuse", False)
    except SystemExit:
        check("recordings with no state ids refuse", True)

    # --exclude-scenario. The before/after path is the one that decides whether
    # a movement gets published, so its failure modes are the ones that matter:
    # a state two scenarios both produced must survive the loss of one of them,
    # and a name that matches nothing must refuse rather than subtract nothing.
    by_state = {1: {"old"}, 2: {"new"}, 3: {"old", "new"}}
    after = project(by_state, [])
    before = project(by_state, ["new"])
    check("subtracting a scenario drops the states only it produced", set(before) == {1, 3})
    check("a state two scenarios share survives the subtraction", 3 in before)
    check("the unsubtracted projection keeps everything", set(after) == {1, 2, 3})
    try:
        project(by_state, ["nosuch"])
        check("subtracting an absent scenario refuses", False)
    except SystemExit:
        check("subtracting an absent scenario refuses", True)

    # And it has to move the figure, or the flag is decorative: `flag` is
    # varied only because `new` supplied its second state.
    rows_after = rank(b, schema_of, sid_block, sid_props, project({1: {"old"}, 2: {"new"}}, []))
    rows_before = rank(b, schema_of, sid_block, sid_props, project({1: {"old"}, 2: {"new"}}, ["new"]))
    check("the subtraction actually moves `varied`",
          figures(rows_after, False)[1] == 1 and figures(rows_before, False)[1] == 0)

    for f in (p, q, rp):
        os.unlink(f)
    print(f"\nBLOCK-SCHEMA-QUEUE {'GREEN' if not fails else 'RED'} — "
          f"{len(fails)} failed")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
