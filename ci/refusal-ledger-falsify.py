#!/usr/bin/env python3
"""Fire every refusal in `ci/refusal-ledger.py`, on tables built to be wrong.

    ci/refusal-ledger-falsify.py

`ci/refusal-ledger.py` is a check whose whole value is its refusals, and a
refusal nobody exercises is a refusal that has stopped working. Every arm
below is shown **failing on the unrepaired state** — a table mutated to carry
exactly the defect the arm exists to catch — and the same arm is then shown
passing on the tree's own tables, so a green run is not a run that found
nothing to look at.

The fixtures are synthetic and tiny: no jar, no `data/vanilla`, no box. That
matters because this has to run in T0, where most trees have never generated a
table at all. The one arm that needs the real tables (`green control`) is
skipped, loudly, when the tree has none — "could not be checked" and "checked
and fine" are different answers and the log says which.

# THE DEFECT EACH ARM PINS

  * `rows-vs-summary` — a generator whose per-row refusal cells and its own
    summary field disagree. That is a number nobody derived, and it is exactly
    how `use_without_item.json`'s doc block came to claim 199 refusals in a
    header, 231 across its three cause rows, and 160 in the table itself.
  * `partition` — a partition that does not add up. Without it, "not in the
    table" cannot be told apart from "answered silently", which is this
    project's oldest failure mode.
  * `normaliser-collision` — two raw reasons that fold to one cause. The
    arithmetic still balances, because the crosscheck sums the generator's own
    rows; the cause table quietly loses a bucket.
  * `drift` — a refusal population that moved without the ledger moving.
  * `missing` — a tree with no tables must exit 2 and say so, not report a
    ledger of zeroes.
"""
import copy
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER = ROOT / "ci" / "refusal-ledger.py"
REAL = ROOT / "data" / "vanilla"


# --- the smallest tables that are internally consistent ----------------------
#
# Hand-built rather than trimmed from the real ones, so a fixture cannot
# inherit a defect from the extract it was cut out of.

def _update_shape():
    return {
        "rng": "update_shape", "states": 4,
        "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
        "sensitive": 10, "connective": 3, "refusedPairs": 5,
        "refused": [
            {"reads": "updateShape read 0,1,0, not its own neighbour",
             "pairs": 4, "blocks": ["minecraft:vine"]},
            {"reads": "updateShape read BlockAndLightGetter.getLightEngine",
             "pairs": 1, "blocks": ["minecraft:wheat"]},
        ],
        "classes": ["AAAA"],
        "table": [{"state": f"minecraft:stone{i}", "dir": 0, "to": "minecraft:air",
                   "schedules": False, "class": 0} for i in range(2)],
    }


def _signal():
    return {
        "rng": "signal", "states": 10,
        "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
        "quiet": 5, "sources": 2, "refusedStates": 3,
        "refused": [
            {"reads": "getSignal read 0,1,0, not its own position",
             "states": 2, "blocks": ["minecraft:redstone_wire"]},
            {"reads": "getSignal read a block entity, which holds the value it is being asked for",
             "states": 1, "blocks": ["minecraft:comparator"]},
        ],
        "unanswerable": [
            {"state": "minecraft:redstone_wire[a=1]", "id": 1, "source": True,
             "reads": "getSignal read 0,1,0, not its own position"},
            {"state": "minecraft:redstone_wire[a=2]", "id": 2, "source": True,
             "reads": "getSignal read 0,1,0, not its own position"},
            {"state": "minecraft:comparator[a=1]", "id": 3, "source": True,
             "reads": "getSignal read a block entity, which holds the value it is being asked for"},
        ],
        "table": [{"state": f"minecraft:lever{i}", "id": 4 + i, "source": True,
                   "signal": [0] * 6, "direct": [0] * 6} for i in range(2)],
    }


def _celltable(what):
    return {
        "what": what, "blocksWithBehaviour": 2, "statesProbed": 10,
        "statesWriting": 3, "refusals": 2, "randomBlocks": 0,
        "probeDependentBlocks": 0, "worldSensitiveBlocks": 0,
        "refusedBy": {"UnsupportedOperationException: UseGolden's level was asked "
                      "for getChunkSource — a use rule reads something this harness d": 2},
        "blocks": 2, "classes": {}, "superclasses": {},
        "table": [
            {"block": "minecraft:piston", "class": "PistonBaseBlock", "random": False,
             "worldSensitive": False, "states": [
                 {"from": "minecraft:piston[extended=true]",
                  "refused": "UnsupportedOperationException: UseGolden's level was asked "
                             "for getChunkSource — a use rule reads something this harness d"},
                 {"from": "minecraft:piston[extended=false]",
                  "refused": "UnsupportedOperationException: UseGolden's level was asked "
                             "for getChunkSource — a use rule reads something this harness d"},
             ]},
            {"block": "minecraft:crafter", "class": "CrafterBlock", "random": False,
             "worldSensitive": False, "states": [
                 {"from": "minecraft:crafter[crafting=true]", "schedulesTick": True,
                  "tickDelay": 4, "tickPriority": "NORMAL", "tickAt": [0, 0, 0],
                  "tickKind": "block", "writes": []},
             ]},
        ],
    }


def _use_item_on():
    return {
        "blocksProbed": 2, "statesProbed": 20, "statesRecorded": 6,
        "blocksWithBehaviour": 2, "refusedStates": 3000, "partlyRefusedStates": 1,
        "refusedBy": {"UseItemOnGolden's level was asked for getBlockEntity — "
                      "a use rule reads something this harness does not m": 3001},
        "table": [],
    }


def _update_shape3(**over):
    """The three-position extract, sized to `_update_shape()`'s refusal set.

    Its population is that table's 5 refused pairs, not a universe of its own,
    which is the one thing about this extract a reader gets wrong first. The
    fixture is built that way so an arm that breaks the tie breaks something
    real.
    """
    doc = {
        "rng": "update_shape3", "states": 4,
        "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
        "neighbourBreadth": 4, "extraBreadth": 2, "extraClassCap": 128,
        "only": "", "probes": 1234, "refusedPairs": 5, "rows": 3,
        "verdicts": {"closed": 3, "connective": 1, "inert": 0, "refused": 1},
        "byOffset": [
            {"at": "0,1,0", "pairs": 4, "verdicts": {"closed": 3, "connective": 1}},
            {"at": "(method)", "pairs": 1, "verdicts": {"refused": 1}},
        ],
        "byClass": [
            {"class": "WallBlock", "pairs": 4,
             "verdicts": {"closed": 3, "connective": 1}},
            {"class": "FireBlock", "pairs": 1, "verdicts": {"refused": 1}},
        ],
        "byExtraClassCount": {"1": 2, "2": 2},
        "stillRefused": [
            {"cause": "updateShape read BlockAndLightGetter.getLightEngine",
             "pairs": 1, "blocks": ["minecraft:wheat"]},
        ],
        "table": [{"state": f"minecraft:vine{i}", "dir": 0, "at": "0,1,0",
                   "extra": "AA==", "to": "minecraft:air", "schedules": False,
                   "neighbours": "AAAA"} for i in range(3)],
    }
    doc.update(over)
    return doc


def fixtures():
    return {
        "update_shape.json": _update_shape(),
        "signal.json": _signal(),
        "use_without_item.json": _celltable("BlockState.useWithoutItem, per state"),
        "set_placed_by.json": _celltable("Block.setPlacedBy, per state"),
    }


def big_fixtures():
    """Fixtures whose counts clear the coverage scan's four-digit threshold.

    The scan ignores anything under 1,000 on purpose — `12` and `160` occur
    all over a codebase for unrelated reasons — so the small fixtures cannot
    exercise it and a bigger set is not decoration.
    """
    t = fixtures()
    u = t["update_shape.json"]
    u["sensitive"], u["connective"], u["refusedPairs"] = 10000, 3000, 5000
    u["refused"] = [
        {"reads": "updateShape read 0,1,0, not its own neighbour",
         "pairs": 4000, "blocks": ["minecraft:vine"]},
        {"reads": "updateShape read BlockAndLightGetter.getLightEngine",
         "pairs": 1000, "blocks": ["minecraft:wheat"]},
    ]
    u["table"] = [{"state": f"minecraft:stone{i}", "dir": 0, "to": "minecraft:air",
                   "schedules": False, "class": 0} for i in range(2000)]
    return t


def build(tmp, tables, ledger=None, census=None, citations=None, files=None):
    root = pathlib.Path(tmp)
    (root / "data" / "vanilla").mkdir(parents=True, exist_ok=True)
    (root / "ci" / "checks").mkdir(parents=True, exist_ok=True)
    for name, doc in tables.items():
        (root / "data" / "vanilla" / name).write_text(json.dumps(doc))
    if ledger is not None:
        (root / "ci" / "checks" / "table-refusal-ledger.json").write_text(
            json.dumps(ledger))
    if census is not None:
        (root / "ci" / "checks" / "shape-refusal-census.json").write_text(
            json.dumps(census))
    if citations is not None:
        (root / "ci" / "checks" / "refusal-citations.toml").write_text(citations)
    for rel, text in (files or {}).items():
        f = root / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
    return root


def run(root, *args):
    env = dict(os.environ, REFUSAL_LEDGER_ROOT=str(root))
    p = subprocess.run([sys.executable, str(LEDGER), *args],
                       capture_output=True, text=True, env=env)
    return p.returncode, p.stdout + p.stderr


# How many arms ran. Counted rather than written down, because the one number
# a falsifier must not carry as a constant is how much it covers: an arm that
# is deleted takes the constant with it and the log still says the old figure.
FIRED = []


def arm(name, want_rc, want_text, tables, args=(), ledger=None, census=None,
        citations=None, files=None):
    FIRED.append(name)
    with tempfile.TemporaryDirectory() as tmp:
        root = build(tmp, tables, ledger, census, citations, files)
        rc, out = run(root, *args)
    ok = rc == want_rc and want_text in out
    print(f"  {'ok  ' if ok else 'FAIL'} {name:<22} rc={rc} "
          f"{'(saw ' + want_text + ')' if ok else '— wanted rc=' + str(want_rc) + ' and ' + repr(want_text)}")
    if not ok:
        print("\n".join("      " + l for l in out.splitlines()[:12]))
    return ok


def main():
    print("refusal-ledger-falsify: every arm on a table built to carry its defect")
    ok = True

    # A control first. If the untouched fixtures do not pass, nothing below
    # means anything — a red arm would be the fixture, not the check.
    ok &= arm("control-consistent", 0, "update_shape.json", fixtures())

    t = fixtures()
    t["use_without_item.json"]["refusals"] = 7
    ok &= arm("rows-vs-summary", 1,
              "the summary's refusals — got 2, want 7", t)

    t = fixtures()
    t["signal.json"]["quiet"] = 4
    ok &= arm("partition", 1, "carried + refused + quiet == states", t)

    t = fixtures()
    t["update_shape.json"]["connective"] = 99
    ok &= arm("partition-pairs", 1,
              "carried + connective + refused == sensitive", t)

    # Two raw reasons that fold to one cause. The pair counts still sum to
    # `refusedPairs`, so only the collision arm can see it.
    t = fixtures()
    t["update_shape.json"]["refused"] = [
        {"reads": "updateShape read 0,1,0, not its own neighbour",
         "pairs": 3, "blocks": ["minecraft:vine"]},
        {"reads": "updateShape read 0,1,0, not its own neighbour either",
         "pairs": 2, "blocks": ["minecraft:fire"]},
    ]
    ok &= arm("normaliser-collision", 1,
              "no two raw reasons normalise to one cause", t)

    # The shape census describes some other extract. `ci/shape-reads.sh` takes
    # five minutes and cannot run in T0, so its output is committed and this is
    # the cheap half — if its narrow probe's four numbers have moved, every
    # verdict in it is about a different table.
    def shape_census(**over):
        c = {"sensitive": 10, "refusedPairs": 5, "carried": 2, "connective": 3,
             "discoveryMissed": 0, "discoveryLateRefusal": 0}
        c.update(over)
        return c

    ok &= arm("census-agrees", 0, "update_shape.json", fixtures(),
              census=shape_census())
    ok &= arm("census-stale", 1, "the census's refusedPairs == the table's",
              fixtures(), census=shape_census(refusedPairs=4))
    ok &= arm("census-absent", 0, "update_shape.json", fixtures())

    # The partition balances and is still not exhaustive. This is the defect
    # `update_shape.json` actually had: `carried + connective + refused ==
    # sensitive` held for months while 164 pairs that a non-default neighbour
    # moves sat outside every term in it, because discovery swept the 1,196
    # block defaults. Discovery is exhaustive now; this arm is what stops it
    # being narrowed again without anybody noticing.
    ok &= arm("census-not-exhaustive", 1, "the census finds nothing discovery dropped",
              fixtures(), census=shape_census(discoveryMissed=164))
    ok &= arm("census-late-refusal", 1, "nothing that reaches past the harness late",
              fixtures(), census=shape_census(discoveryLateRefusal=1))

    # Drift: a ledger written from one set of tables, checked against another.
    with tempfile.TemporaryDirectory() as tmp:
        root = build(tmp, fixtures())
        rc, out = run(root, "--write")
        if rc != 0:
            print(f"  FAIL drift-setup          --write returned {rc}\n{out}")
            ok = False
        else:
            written = json.loads(
                (root / "ci" / "checks" / "table-refusal-ledger.json").read_text())
            moved = copy.deepcopy(written)
            moved["tables"]["signal.json"]["refused"] = 999
            t = fixtures()
            ok &= arm("drift", 1, "REFUSAL-LEDGER DRIFTED", t,
                      args=("--check",), ledger=moved)
            ok &= arm("check-green", 0, "REFUSAL-LEDGER OK", fixtures(),
                      args=("--check",), ledger=written)

    # The citation arms. The five stale figures this branch repaired by hand
    # were each a sentence quoting a number the table had stopped carrying, and
    # each would have been caught here on the commit that broke it.
    #
    # `use_without_item.json` in the fixtures refuses 2, so a doc saying 2 is
    # right and a doc saying 7 is the `199`-against-`160` case exactly.
    def cites(quote):
        return ('[[cite]]\nfile = "docs/X.md"\n'
                'value = "use_without_item.json.refused"\n'
                f'quote = "{quote}"\n')

    ok &= arm("citation-green", 0, "REFUSAL-CITATIONS OK", fixtures(),
              args=("--cite",), citations=cites("the table refuses {} states"),
              files={"docs/X.md": "the table refuses 2 states\n"})
    ok &= arm("citation-stale", 1, "quotes '7' where the tables say 2", fixtures(),
              args=("--cite",), citations=cites("the table refuses {} states"),
              files={"docs/X.md": "the table refuses 7 states\n"})
    ok &= arm("citation-gone", 1, "reworded or moved", fixtures(),
              args=("--cite",), citations=cites("the table refuses {} states"),
              files={"docs/X.md": "the extraction declined 2 states\n"})
    ok &= arm("citation-unresolved", 1, "no such value in the ledger", fixtures(),
              args=("--cite",),
              citations=('[[cite]]\nfile = "docs/X.md"\n'
                         'value = "use_without_item.json.no_such_field"\n'
                         'quote = "anything {}"\n'),
              files={"docs/X.md": "anything 2\n"})
    # A stale citation must also redden `--check`, not only `--cite`: T0 runs
    # the former and nothing would reach the latter.
    ok &= arm("citation-reddens-check", 1, "CITATION  docs/X.md", fixtures(),
              args=("--check",), citations=cites("the table refuses {} states"),
              files={"docs/X.md": "the table refuses 7 states\n"})

    # The two self-assertions. `ci/check-figures.py`'s header names the defect
    # class both exist to keep this out of: register nothing and it passes,
    # register only what you already verified and it passes forever.
    def meta(**kw):
        body = "[meta]\n"
        for k, v in kw.items():
            body += f"{k} = {v!r}\n".replace("'", '"')
        return body

    scan_one = meta(min_cites=0, scan=["docs/X.md"])
    ok &= arm("citation-floor", 1, "emptying this ledger must be red",
              fixtures(), args=("--cite",), citations=meta(min_cites=3),
              files={"docs/X.md": "nothing\n"})
    ok &= arm("citation-unregistered", 1,
              "5,000 appears here and no citation covers it",
              big_fixtures(), args=("--cite",), citations=scan_one,
              files={"docs/X.md": "the table refuses 5,000 pairs\n"})
    # **A derived value inside a longer number is not a mention of it**, and a
    # plain `in` said it was. `5,000` sits inside `15,000` and inside `5,000.5`,
    # and this arm is the live case that found it: `set_placed_by.json.answered`
    # went 1,684 -> 2,010 on 2026-08-18 and the scan immediately reported
    # `ShapeGolden.java` as quoting it, off the `22,010` in its own comment.
    # A scan with a false positive is a scan somebody switches off.
    ok &= arm("citation-scan-substring", 0, "REFUSAL-CITATIONS OK",
              big_fixtures(), args=("--cite",), citations=scan_one,
              files={"docs/X.md": "unrelated: 15,000 and 5,000.5 and 45,000\n"})
    ok &= arm("citation-scan-covered", 0, "REFUSAL-CITATIONS OK",
              big_fixtures(), args=("--cite",),
              citations=scan_one + ('[[cite]]\nfile = "docs/X.md"\n'
                                    'value = "update_shape.json.refused"\n'
                                    'quote = "the table refuses {} pairs"\n'),
              files={"docs/X.md": "the table refuses 5,000 pairs\n"})
    ok &= arm("citation-scan-gone", 1, "listed for scanning and not in the tree",
              fixtures(), args=("--cite",), citations=scan_one)

    # The optional fifth table. Nothing under `crates/` opens
    # `use_item_on.json`, so most trees never generate it — and a ledger that
    # records it must not redden every tree that did not.
    with tempfile.TemporaryDirectory() as tmp:
        five = fixtures()
        five["use_item_on.json"] = _use_item_on()
        root = build(tmp, five)
        rc, out = run(root, "--write")
        with_five = json.loads(
            (root / "ci" / "checks" / "table-refusal-ledger.json").read_text())
        root = build(tmp + "-x", fixtures())
        rc2, out2 = run(root, "--write")
        without_five = json.loads(
            (pathlib.Path(tmp + "-x") / "ci" / "checks"
             / "table-refusal-ledger.json").read_text())
        shutil.rmtree(tmp + "-x", ignore_errors=True)
    ok &= arm("optional-skipped", 0, "use_item_on.json skipped", fixtures(),
              args=("--check",), ledger=with_five)
    ok &= arm("optional-unrecorded", 1, "DRIFTED  use_item_on.json", five,
              args=("--check",), ledger=without_five)
    ok &= arm("optional-consistent", 0, "REFUSAL-LEDGER OK", five,
              args=("--check",), ledger=with_five)

    # **`--write` in a tree without the optional table must CARRY it, not drop
    # it.** `--write` derives from the tables present on disk, so writing the
    # derivation straight out deleted `use_item_on.json` — a whole recorded
    # population, 4,537 states and two causes — in a tree that had simply not
    # built it. `--check` then passed, because a table absent from both the
    # ledger and the tree is not drift. It is amnesia, and it reads as a clean
    # run.
    #
    # Found on 2026-08-18 by a one-field edit: `--write` moved
    # `set_placed_by.json`'s `answered` 2010 -> 2046 and took the fifth table
    # with it. Asserted here in the direction that matters — the entry is still
    # there afterwards, byte for byte.
    with tempfile.TemporaryDirectory() as tmp:
        root = build(tmp, fixtures(), ledger=with_five)
        rc, out = run(root, "--write")
        after = json.loads(
            (root / "ci" / "checks" / "table-refusal-ledger.json").read_text())
    carried = after.get("tables", {}).get("use_item_on.json")
    kept = carried == with_five["tables"]["use_item_on.json"]
    print(f"  {'ok  ' if kept else 'FAIL'} {'write-carries-optional':<22} rc={rc} "
          + ("(use_item_on.json survived --write unchanged)" if kept
             else f"— --write dropped or altered it: {carried!r}"))
    ok &= kept

    # A citation of the optional table, in a tree that does not have it, must
    # skip rather than redden — otherwise the optional table costs exactly what
    # leaving it out was meant to avoid.
    opt_cite = ('[[cite]]\nfile = "docs/X.md"\n'
                'value = "use_item_on.json.refused"\nquote = "refuses {}"\n')
    ok &= arm("optional-cite-skipped", 0, "its table is not generated in this tree",
              fixtures(), args=("--cite",), citations=opt_cite,
              files={"docs/X.md": "refuses 3\n"})
    five_only = fixtures()
    five_only["use_item_on.json"] = _use_item_on()
    ok &= arm("optional-cite-checked", 1, "quotes '3' where the tables say 3,000",
              five_only, args=("--cite",), citations=opt_cite,
              files={"docs/X.md": "refuses 3\n"})

    # ---- the three-position extract ------------------------------------
    #
    # Every refusal in `update_shape3()` and in `three()`, on an extract built
    # to carry it. The tie to `update_shape.json` is the one that matters: the
    # extract's population IS that table's refusal set, so an extract that
    # widened a different set reports closure figures about something else and
    # nothing in its output says so.
    three = fixtures()
    three["update_shape3.json"] = _update_shape3()
    ok &= arm("three-consistent", 0, "update_shape3.json", three)
    ok &= arm("three-absent", 0, "update_shape.json", fixtures())
    ok &= arm("three-partition", 1,
              "closed + connective + inert + refused == refusedPairs",
              dict(three, **{"update_shape3.json": _update_shape3(
                  verdicts={"closed": 3, "connective": 1, "inert": 0, "refused": 0})}))
    ok &= arm("three-offset-sum", 1, "pairs summed by the position read == refusedPairs",
              dict(three, **{"update_shape3.json": _update_shape3(byOffset=[
                  {"at": "0,1,0", "pairs": 4, "verdicts": {"closed": 3}}])}))
    ok &= arm("three-cause-sum", 1, "still-refused pairs summed by cause == the refused verdict",
              dict(three, **{"update_shape3.json": _update_shape3(stillRefused=[
                  {"cause": "updateShape read BlockAndLightGetter.getLightEngine",
                   "pairs": 7, "blocks": []}])}))
    # `--only WallBlock` is how the cost of the run was measured, and its
    # output is the same JSON with a different meaning. Publishing one as if it
    # were a full run is a partial measurement wearing a complete name.
    ok &= arm("three-partial-run", 1, "the extract covered every block class",
              dict(three, **{"update_shape3.json": _update_shape3(only="WallBlock")}))
    ok &= arm("three-other-refusal-set", 1,
              "the extract's refusal set == update_shape.json's",
              dict(three, **{"update_shape3.json": _update_shape3(refusedPairs=4)}))
    ok &= arm("three-other-universe", 1, "the extract's universe == update_shape.json's",
              dict(three, **{"update_shape3.json": _update_shape3(states=8)}))
    # Per-class closure is what §83's headline claims quote — `WallBlock` alone
    # is 41,472 of the population — so a byClass table that does not cover the
    # population is a set of per-class figures with a hole nobody can see.
    ok &= arm("three-class-hole", 1, "every class's verdicts sum to the whole population",
              dict(three, **{"update_shape3.json": _update_shape3(byClass=[
                  {"class": "WallBlock", "pairs": 4,
                   "verdicts": {"closed": 3, "connective": 1}}])}))

    # A tree with no tables refuses; it does not publish a ledger of zeroes.
    with tempfile.TemporaryDirectory() as tmp:
        root = build(tmp, {})
        rc, out = run(root)
        good = rc == 2 and "run ci/gen-gameplay-tables.sh" in out
        print(f"  {'ok  ' if good else 'FAIL'} {'missing':<22} rc={rc}")
        ok &= good

    # The green control against this tree's real tables, when it has any.
    if all((REAL / n).is_file() for n in
           ("update_shape.json", "signal.json", "use_without_item.json",
            "set_placed_by.json")):
        rc, out = run(ROOT)
        good = rc == 0
        print(f"  {'ok  ' if good else 'FAIL'} {'green-control':<22} rc={rc} "
              f"(this tree's own four tables)")
        if not good:
            print("\n".join("      " + l for l in out.splitlines()[:12]))
        ok &= good
    else:
        print("  skip green-control          — this tree has no data/vanilla tables; "
              "the synthetic arms above still ran")

    print(f"refusal-ledger-falsify: " +
          (f"all {len(FIRED)} arms fired" if ok else "AN ARM DID NOT FIRE"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
