#!/usr/bin/env python3
"""The golden generators' refusal ledger, derived rather than quoted.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/refusal-ledger.py            # every refusal in data/vanilla, counted
#     ci/refusal-ledger.py --check    # refuse if any population has drifted
#     ci/refusal-ledger.py --table    # markdown, for a cold start
#
# WHAT BREAKS IF YOU IGNORE THIS: you quote a refusal count from a doc and it
# is a year of drift out of date, or you quote one table's unit as another's
# and are wrong by three orders of magnitude. Both happened. See below.
# ===== end interface; everything below is why =====

# # THERE ARE TWO REFUSAL POPULATIONS AND THEY READ IDENTICALLY WHEN QUOTED

`ci/unported-inventory.py` counts the **crate's** debt: 85 markers over the
server sources, 32 gap clusters, 48 gaps once the multi-gap markers expand.

This counts the **tables'** debt: the rows four golden generators emitted
because their harness could not answer, with the reason attached.

They are not the same set, they do not have the same unit, and neither is a
proxy for the other. `signal.json` refuses 1,215 `redstone_wire` states that
`crate::power` hand-ports outright — so reading the table's refusals as the
server's debt overstates it by three orders of magnitude — while
`update_shape.json` refuses 79,113 pairs that no marker in the crate mentions
at all, so reading the crate's markers as the tables' debt hides every one of
them. The two ledgers diverge **in both directions**, which is exactly why one
cannot stand in for the other.

# # THE UNIT IS PART OF THE NUMBER

Each generator keys its table in its own unit and the ledger carries the unit
with every count, because the counts are not comparable without it:

  * `update_shape.json`   — **(state, direction) pairs**. 194,196 exist;
                            103,482 are sensitive; the refusals are of those.
  * `signal.json`         — **states**, all six directions at once. A state
                            whose rule reads the world is refused for every
                            direction or for none, deliberately: a partial row
                            reads exactly like a complete one from the call
                            site.
  * `use_without_item.json`, `set_placed_by.json`
                          — **states**, and the refusal is a *cell* inside a
                            block's row rather than a row of its own.

"79,113 refused" and "1,250 refused" are the largest and one of the smallest
numbers here and they are not on the same axis at all.

# # WHAT MAKES IT STAY HONEST

Three properties, each of which has caught something:

  1. **Every population is counted off the rows, and again off the
     generator's own summary field, and the two must agree.** A generator
     whose summary and rows disagree is emitting a number nobody derived.
  2. **Every partition must be exhaustive.** `carried + refused + quiet ==
     states` for `signal.json`; `carried + connective + refused == sensitive`
     for `update_shape.json`. This is what makes "not in the table" mean
     something: without it, a state can be missing because it was answered
     silently and a reader cannot tell.
  3. **`--check` re-derives against a committed ledger and refuses on drift.**
     Same shape as `ci/unported-inventory.py --check`. A count that moved is
     either a jar change or a generator change, and either way the docs that
     quote it are now wrong.

**It is not a gate on the server and must not become one.** A refusal is not
a defect; it is a measurement of what the harness could not see. Green here
means the ledger still describes the tables, and nothing else.

# # WHAT THIS DELIBERATELY DOES NOT COUNT, BECAUSE A COUNT CANNOT SEE IT

**A refusal is loud. An absence is not.** `DiodeBlock.setPlacedBy` schedules a
tick when `shouldTurnOn` says the world is powering it. `PlacedGolden`'s level
answers air for every position it is not told about rather than throwing, so
`shouldTurnOn` returns false, no tick is scheduled, the probe finds nothing
interesting and emits **no row at all** — for `minecraft:repeater` and
`minecraft:comparator` both. Nothing in this ledger can see that: there is no
refusal to count. A consumer reading "no row, no tick" ships a repeater that
never wakes, and the table and the server agree on the same absent input, so
no check that compares them can fire.

`ci/probe-absences.py` is the check that can, and it works the only way that
works: from the jar's own override set, not from the table. Anything this
ledger says about a generator's coverage is a statement about the rows that
exist, never about the rows that should.
"""
import json
import os
import pathlib
import re
import sys
import tomllib

# `REFUSAL_LEDGER_ROOT` so `ci/refusal-ledger-falsify.py` can point this at a
# throwaway tree of mutated tables. Same handle `ci/claim-number.py` takes for
# the same reason: a refusal arm that is never fired is an arm that has
# stopped working, and the only way to fire these is against tables that are
# deliberately wrong.
ROOT = pathlib.Path(os.environ.get("REFUSAL_LEDGER_ROOT")
                    or pathlib.Path(__file__).resolve().parent.parent)
DATA = ROOT / "data" / "vanilla"
LEDGER = ROOT / "ci" / "checks" / "table-refusal-ledger.json"
CENSUS = ROOT / "ci" / "checks" / "shape-refusal-census.json"
CITATIONS = ROOT / "ci" / "checks" / "refusal-citations.toml"


def _cells(doc):
    """Every per-state cell in a `{"table": [{"states": [...]}]}` extract."""
    for row in doc.get("table", []):
        for cell in row.get("states", []):
            yield row, cell


def _cause(text):
    """The refusal reason, normalised to the part that names what was read.

    The generators embed the exception class and truncate the message at 90
    characters, so the raw strings differ by a prefix and a ragged tail that
    say nothing. `UseGolden`'s level was asked for X` and `updateShape read X`
    are the parts a reader can act on, and grouping on the raw string splits
    one cause across two buckets when a message length changes.
    """
    for marker in ("level was asked for ", "updateShape read ", "getSignal read "):
        if marker in text:
            tail = text.split(marker, 1)[1]
            return tail.split(" —")[0].split(", not ")[0].strip()
    return text.split(":")[0].strip()


def _merge(pairs):
    """`(key, count)` into a dict, **summing** rather than overwriting.

    A dict comprehension was the first version and it is the bug this
    function exists to not have: two raw reasons that normalise to one cause
    silently drop one of the two, and the arithmetic still balances because
    the crosscheck sums the generator's own rows rather than this dict. The
    collision is reported instead, because a normaliser that merges two
    different things is a normaliser that needs fixing, not a count to bury.
    """
    out = {}
    for key, count in pairs:
        out[key] = out.get(key, 0) + count
    return out


def update_shape(doc):
    sensitive = doc["sensitive"]
    carried = len(doc["table"])
    connective = doc["connective"]
    refused = doc["refusedPairs"]
    summed = sum(r["pairs"] for r in doc["refused"])
    causes = _merge((_cause(r["reads"]), r["pairs"]) for r in doc["refused"])
    return {
        "unit": "(state, direction) pairs",
        "universe": doc["states"] * len(doc["directionOrder"]),
        "population": sensitive,
        "refused": refused,
        "answered": carried,
        "other": {"connective": connective},
        # (1) rows against summary, (2) the partition
        "crosschecks": [
            ("refused pairs summed by cause == refusedPairs", summed, refused),
            ("carried + connective + refused == sensitive",
             carried + connective + refused, sensitive),
            ("no two raw reasons normalise to one cause",
             len(causes), len(doc["refused"])),
        ],
        "causes": causes,
        # Safe as a comprehension only because the crosscheck above refuses a
        # normaliser collision; without it this would drop a cause silently.
        "blocks": {_cause(r["reads"]): r["blocks"] for r in doc["refused"]},
    }


def signal(doc):
    carried = len(doc["table"])
    refused = len(doc["unanswerable"])
    quiet = doc["quiet"]
    causes = {}
    blocks = {}
    for row in doc["unanswerable"]:
        c = _cause(row["reads"])
        causes[c] = causes.get(c, 0) + 1
        blocks.setdefault(c, set()).add(row["state"].split("[")[0])
    return {
        "unit": "states",
        "universe": doc["states"],
        "population": doc["states"],
        "refused": refused,
        "answered": carried,
        "other": {"quiet": quiet},
        "crosschecks": [
            ("len(unanswerable) == refusedStates", refused, doc["refusedStates"]),
            ("refused states summed by cause == refusedStates",
             sum(causes.values()), doc["refusedStates"]),
            ("carried + refused + quiet == states",
             carried + refused + quiet, doc["states"]),
        ],
        "causes": causes,
        "blocks": {k: sorted(v)[:8] for k, v in blocks.items()},
    }


def _celltable(doc, extra=None):
    refused = 0
    causes = {}
    blocks = {}
    for row, cell in _cells(doc):
        if not cell.get("refused"):
            continue
        refused += 1
        c = _cause(cell["refused"])
        causes[c] = causes.get(c, 0) + 1
        blocks.setdefault(c, set()).add(row["block"])
    summary = doc.get("refusals", doc.get("refusedStates"))
    out = {
        "unit": "states",
        "universe": doc["statesProbed"],
        "population": doc["statesProbed"],
        "refused": refused,
        "answered": doc["statesWriting"],
        "other": {},
        "crosschecks": [
            ("refusal cells counted == the summary's refusals", refused, summary),
            ("refusal cells summed by cause == the summary's refusals",
             sum(causes.values()), summary),
        ],
        "causes": causes,
        "blocks": {k: sorted(v)[:8] for k, v in blocks.items()},
    }
    if extra:
        out["other"].update(extra(doc))
    return out


def use_without_item(doc):
    # `soundBlocks` is derived here rather than left to prose because the one
    # sentence in the tree that quoted it had never been measured: it said 182,
    # which is `blocksWithBehaviour - 2 ported` and assumes every block that
    # does something on right-click also plays a sound. The `sound` column says
    # 70. A number obtained by subtracting two guesses reads exactly like a
    # measured one.
    sounding = {row["block"] for row, cell in _cells(doc) if cell.get("sound")}
    return _celltable(doc, lambda d: {
        "randomBlocks": d["randomBlocks"],
        "probeDependentBlocks": d["probeDependentBlocks"],
        "soundBlocks": len(sounding),
    })


def set_placed_by(doc):
    # Tick rows are not refusals and are carried here because the one lead
    # that broke this table open was a *tick* — `DiodeBlock` schedules one and
    # has no row, `CrafterBlock` schedules one and has 24. A ledger that
    # counted only refusals could not have shown the two side by side.
    ticks = 0
    tickblocks = set()
    for row, cell in _cells(doc):
        if cell.get("schedulesTick"):
            ticks += 1
            tickblocks.add(row["block"])
    return _celltable(doc, lambda d: {
        "randomBlocks": d["randomBlocks"],
        "worldSensitiveBlocks": d["worldSensitiveBlocks"],
        "tickRows": ticks,
        "tickBlocks": sorted(tickblocks),
    })


def use_item_on(doc):
    """The fifth generator, and the one that already made the repair.

    Nothing under `crates/` opens `use_item_on.json`, so
    `ci/gen-gameplay-tables.sh` never generates it and most trees do not have
    it. It is read here when it *is* present because it is the only measured
    answer to "what does refusing a block entity cost": `UseItemOnGolden`
    turns the ask itself into a refusal where `UseGolden` answers `null`, over
    the same registry and the same clicks. Its refusals are
    `use_without_item.json`'s silent absences with a reason attached.

    Its unit is a state, like the other cell tables, but its cells are
    (state, item) pairs — so a state is `refused` when *every* item refused on
    it and `partlyRefused` when some did. `refusedBy` counts both, which is
    what the crosscheck below pins.
    """
    return {
        "unit": "states (over a state x item grid)",
        "universe": doc["statesProbed"],
        "population": doc["statesProbed"],
        "refused": doc["refusedStates"],
        "answered": doc["statesRecorded"],
        "other": {"partlyRefusedStates": doc["partlyRefusedStates"],
                  "blocksWithBehaviour": doc["blocksWithBehaviour"]},
        "crosschecks": [
            ("refusedBy sums to refusedStates + partlyRefusedStates",
             sum(doc["refusedBy"].values()),
             doc["refusedStates"] + doc["partlyRefusedStates"]),
        ],
        "causes": _merge((_cause(k), v) for k, v in doc["refusedBy"].items()),
        "blocks": {},
    }


def update_shape3(doc):
    """The three-position extract, and the only population here that is a
    *subset* of another one.

    `update_shape3.json` does not describe a table of its own universe: its
    population is exactly `update_shape.json`'s refusal set, re-derived from
    the jar by its own copy of `ShapeGolden`'s narrow probe. So `population`
    here is 79,113 and not 103,482, and `refused` is what **three** positions
    still could not answer rather than what two could not. Quoting the two
    `refused` figures side by side without that sentence is how a reader
    concludes the debt doubled.

    Nothing under `crates/` opens it yet. It is listed as optional for the
    same reason `use_item_on.json` is — `ci/gen-gameplay-tables.sh` does not
    generate it, because it costs about twenty minutes and no test reads it —
    so most trees do not have it and the ledger says so out loud instead of
    reporting a population of zeroes.
    """
    v = doc["verdicts"]
    causes = _merge((_cause(r["cause"]), r["pairs"]) for r in doc["stillRefused"])
    # Per-class closure, exposed because the four classes ARE the population:
    # `WallBlock` is 41,472 of the refusals and `StairBlock` 19,200, so "the
    # extract closed N pairs" is a sentence about four block classes wearing a
    # total. A doc that names one of them needs the number tied to the table
    # the same way every other figure here is, and `byClass` in the JSON is a
    # list of verdict maps that no citation path can reach.
    by_class = {}
    for row in doc.get("byClass", []):
        for verdict, n in row["verdicts"].items():
            by_class.setdefault(verdict, {})[row["class"]] = n
    return {
        "unit": "(state, direction) pairs, of update_shape.json's refusals",
        "universe": doc["states"] * len(doc["directionOrder"]),
        "population": doc["refusedPairs"],
        "refused": v["refused"],
        "answered": v["closed"],
        "other": {"connective": v["connective"], "inert": v["inert"],
                  "rows": doc["rows"], "extraBreadth": doc["extraBreadth"],
                  "byClass": by_class},
        "crosschecks": [
            ("closed + connective + inert + refused == refusedPairs",
             v["closed"] + v["connective"] + v["inert"] + v["refused"],
             doc["refusedPairs"]),
            ("pairs summed by the position read == refusedPairs",
             sum(r["pairs"] for r in doc["byOffset"]), doc["refusedPairs"]),
            ("still-refused pairs summed by cause == the refused verdict",
             sum(r["pairs"] for r in doc["stillRefused"]), v["refused"]),
            ("no two raw causes normalise to one",
             len(causes), len(doc["stillRefused"])),
            ("every class's verdicts sum to the whole population",
             sum(n for m in by_class.values() for n in m.values()),
             doc["refusedPairs"]),
            # A partial run is a different measurement wearing the same name.
            ("the extract covered every block class",
             len(doc["only"]), 0),
        ],
        "causes": causes,
        "blocks": {_cause(r["cause"]): r["blocks"] for r in doc["stillRefused"]},
    }


TABLES = {
    "update_shape.json": update_shape,
    "signal.json": signal,
    "use_without_item.json": use_without_item,
    "set_placed_by.json": set_placed_by,
}

# Present only in a tree that generated it by hand. See `use_item_on` above and
# the arm comment in `ci/gen-gameplay-tables.sh`: nothing requires it, so
# nothing generates it, so its refusals have never been in anybody's ledger.
OPTIONAL = {
    "use_item_on.json": use_item_on,
    "update_shape3.json": update_shape3,
}


def derive():
    out = {}
    missing = []
    for name, fn in TABLES.items():
        path = DATA / name
        if not path.is_file():
            missing.append(name)
            continue
        out[name] = fn(json.loads(path.read_text()))
    for name, fn in OPTIONAL.items():
        path = DATA / name
        if path.is_file():
            out[name] = fn(json.loads(path.read_text()))
    return out, missing


def census(derived):
    """The committed shape census still describes THIS tree's update_shape.json.

    `ci/shape-reads.sh` takes about five minutes and cannot run in T0, so its
    output is committed and this is the cheap half: the census recomputes
    `sensitive`, `carried`, `connective` and `refusedPairs` with its own copy
    of `ShapeGolden`'s narrow probe, and if those four have moved then every
    verdict in it — how much of the 79,113 is owed, recoverable, connective or
    inert — is about a different extract. Costs one JSON parse.

    Returns `(table, claim, got, want)` rows, and nothing at all when the tree
    has no census: an absent census is not a drifted one.
    """
    if not CENSUS.is_file() or "update_shape.json" not in derived:
        return []
    c = json.loads(CENSUS.read_text())
    d = derived["update_shape.json"]
    return [(CENSUS.name, claim, got, want) for claim, got, want in (
        ("the census's sensitive pairs == the table's",
         c["sensitive"], d["population"]),
        ("the census's refusedPairs == the table's", c["refusedPairs"], d["refused"]),
        ("the census's carried == the table's", c["carried"], d["answered"]),
        ("the census's connective == the table's",
         c["connective"], d["other"]["connective"]),
        # The exhaustiveness half, added 2026-08-18 with the discovery
        # widening. `carried + connective + refused == sensitive` balanced for
        # months while 164 real cascades sat outside every term in it, because
        # discovery swept 1,196 block defaults and they move only on a
        # non-default neighbour. Discovery is exhaustive now, so the census's
        # own re-sweep of the dropped pairs must find nothing — and if it ever
        # finds something again, the partition is balanced and wrong again.
        ("the census finds nothing discovery dropped", c["discoveryMissed"], 0),
        ("...and nothing that reaches past the harness late",
         c["discoveryLateRefusal"], 0),
        # `resolutionRefusalAtDefaults` is deliberately NOT asserted to 0 here,
        # and the reason is §83.3 in one line: it is 2, and the number beside
        # it that IS 0 — `lateRefusalAtDefaults` — is about a different pass.
        # Adding it to this list as "must be 0" is the specific mistake that
        # section documents, so it is pinned by the committed census's byte
        # comparison instead, where 2 is a value and not an absence.
    ) if got != want]


def three(derived):
    """Tie the three-position extract to the table whose refusals it re-asks.

    `update_shape3.json`'s population is not a universe of its own: it is
    `update_shape.json`'s refusal set, re-derived from the jar by the
    extract's own copy of the narrow probe. If the two disagree then the
    extract widened a *different* refusal set than the one the server's table
    carries, and every closure figure in it is about something else.

    Exactly the shape of `census()` above, one table further on. Nothing at
    all when the tree has no extract: an absent table is not a drifted one.
    """
    if "update_shape3.json" not in derived or "update_shape.json" not in derived:
        return []
    t = derived["update_shape3.json"]
    d = derived["update_shape.json"]
    return [("update_shape3.json", claim, got, want)
            for claim, got, want in (
        ("the extract's refusal set == update_shape.json's",
         t["population"], d["refused"]),
        ("the extract's universe == update_shape.json's",
         t["universe"], d["universe"]),
    ) if got != want]


def resolve(derived, path):
    """`"signal.json.other.quiet"` -> the derived value, or `None`.

    Split on `.json.` rather than on `.`, because every table name contains a
    dot and a naive walk would look for a key called `signal`.

    `census.*` reaches into `ci/checks/shape-refusal-census.json` instead.
    Those figures — 48,079 owed, 22,010 recoverable, 164 missed by discovery —
    are the most-quoted numbers this branch produced and they belong to a file
    that takes five minutes to regenerate, which is exactly the kind that goes
    stale without anybody noticing. `census.verdicts.world`,
    `census.discoveryMissed`, `census.discoveryMissedByClass.ChestBlock`, and
    so on.
    """
    if path.startswith("census."):
        if not CENSUS.is_file():
            return None
        node = json.loads(CENSUS.read_text())
        for part in path[len("census."):].split("."):
            if not isinstance(node, dict) or part not in node:
                return None
            node = node[part]
        return node if isinstance(node, int) else None
    table, _, rest = path.partition(".json.")
    node = derived.get(table + ".json")
    # `causes` keys are refusal reasons and several contain dots —
    # `LevelReader.environmentAttributes`, `BlockAndLightGetter.getLightEngine`.
    # So everything after `causes.` is ONE key, not a path. Splitting it looked
    # for a `LevelReader` bucket and answered `None`, which `--cite` then
    # reported as CITATION-UNRESOLVED against a cause that plainly exists.
    if rest.startswith("causes."):
        node = node.get("causes") if isinstance(node, dict) else None
        node = node.get(rest[len("causes."):]) if isinstance(node, dict) else None
        return node if isinstance(node, int) else None
    for part in rest.split("."):
        if not isinstance(node, dict) or part not in node:
            return None
        node = node[part]
    return node if isinstance(node, int) else None


def _mentions(text, token):
    """Whether `token` appears as a whole number rather than inside a longer one.

    A digit, a comma or a decimal point on either side means the match is part
    of another number. `re.escape` because the token carries commas.
    """
    return re.search(r"(?<![\d,.])" + re.escape(token) + r"(?![\d,.])", text) is not None


def cite(derived):
    """Every sentence in the tree that quotes one of these counts, checked.

    The repair half of this branch fixed five stale figures by hand. This is
    the half that stops the sixth: each citation carries the sentence as a
    **template** with `{}` where the number goes, the check renders it with the
    derived value in both plain and thousands-separated form, and refuses if
    neither rendering appears in the file.

    Two failures, and they are different:

      * `CITATION` — the sentence is there with some other number in it. That
        is the `199` against a table of `160` case, caught at the sentence
        rather than at the table.
      * `CITATION-GONE` — no rendering of the template appears at all. The
        sentence was reworded or moved, so nothing here knows whether it is
        still true. A reworded claim is an unchecked claim.

    Returns `(kind, file, path, detail)` rows.
    """
    if not CITATIONS.is_file():
        return []
    doc = tomllib.loads(CITATIONS.read_text())
    bad = []
    for c in doc.get("cite", []):
        path, quote = c["value"], c["quote"]
        want = resolve(derived, path)
        if want is None:
            # A citation of an OPTIONAL table this tree never generated is not
            # an unresolved citation, it is a citation that cannot be checked
            # here — the same distinction `--check` already draws for the table
            # itself. Reddening every tree that did not run a generator nothing
            # requires would make the optional table cost exactly what leaving
            # it out was meant to avoid.
            if any(path.startswith(t.removesuffix(".json") + ".json.")
                   for t in OPTIONAL if t not in derived):
                print(f"refusal-ledger: {c['file']} [{path}] skipped — "
                      f"its table is not generated in this tree")
                continue
            bad.append(("CITATION-UNRESOLVED", c["file"], path,
                        "no such value in the ledger"))
            continue
        f = ROOT / c["file"]
        if not f.is_file():
            bad.append(("CITATION-GONE", c["file"], path, "the file is gone"))
            continue
        text = f.read_text()
        renders = [quote.replace("{}", f"{want:,}"), quote.replace("{}", str(want))]
        if any(r in text for r in renders):
            continue
        # Present with a different number? Then it is a stale figure, not a
        # reworded sentence, and saying which is the whole value of the split.
        #
        # The number is read as the run of digits and commas immediately after
        # the head, **not** by splitting on the tail. Splitting on the tail
        # needed the tail to be non-empty, so a quote ending in `{}` could
        # never report CITATION and fell through to CITATION-GONE — which the
        # `optional-cite-checked` arm found by being written that way.
        head, _, tail = quote.partition("{}")
        if head and head in text:
            at = text.index(head) + len(head)
            m = re.match(r"[0-9][0-9,]*", text[at:])
            saw = m.group(0) if m else text[at:at + 10].strip()
            bad.append(("CITATION", c["file"], path,
                        f"quotes {saw!r} where the tables say {want:,}"))
        else:
            bad.append(("CITATION-GONE", c["file"], path,
                        f"no sentence matching {quote!r} — reworded or moved, "
                        f"so nothing checks it now"))

    # --- the two self-assertions -------------------------------------------
    #
    # `ci/check-figures.py` learned both of these the expensive way and its
    # header says so: a ledger-driven checker is unusually easy to write in the
    # "could not fail" shape. Register nothing and it passes; register only
    # what you already verified and it passes forever.
    meta = doc.get("meta", {})
    floor = meta.get("min_cites", 0)
    if len(doc.get("cite", [])) < floor:
        bad.append(("CITATION-FLOOR", CITATIONS.name, "meta.min_cites",
                    f"{len(doc.get('cite', []))} citations registered, floor is "
                    f"{floor} — emptying this ledger must be red, not green"))

    # Coverage. Every occurrence of a derived value in a scanned file must be
    # inside a registered citation, or the ledger is rotting by omission while
    # the docs grow. Restricted to values of four digits or more: `12` and
    # `160` occur all over a codebase for unrelated reasons, and a scan that
    # cries wolf is a scan that gets switched off.
    scanned = meta.get("scan", [])
    # `population` and `universe` are deliberately NOT scanned. 32,366 is the
    # size of the block-state registry — a property of the jar that every one
    # of these tables uses as a denominator and that every partition crosscheck
    # already pins. Scanning it would demand a citation on every sentence that
    # says "of 32,366" and teach the next reader to silence the scan.
    big = {}
    for table, d in derived.items():
        for field, v in (("refused", d["refused"]), ("answered", d["answered"])):
            if v >= 1000:
                big.setdefault(v, f"{table}.{field}")
        for k, v in d["other"].items():
            if isinstance(v, int) and v >= 1000:
                big.setdefault(v, f"{table}.other.{k}")
    for rel in scanned:
        f = ROOT / rel
        if not f.is_file():
            bad.append(("CITATION-SCAN-GONE", rel, "meta.scan",
                        "listed for scanning and not in the tree"))
            continue
        text = f.read_text()
        covered = "".join(
            c["quote"].replace("{}", f"{resolve(derived, c['value']):,}")
            for c in doc.get("cite", [])
            if c["file"] == rel and resolve(derived, c["value"]) is not None)
        for value, name in big.items():
            token = f"{value:,}"
            # **Bounded, because a thousands-separated number is a substring of
            # a bigger one.** `2,010` sits inside `22,010` — which is
            # `ShapeGolden.java`'s count of single-alternative destruction rules
            # and has nothing to do with `set_placed_by.json` — so a plain `in`
            # reported that file as quoting a figure it has never mentioned.
            # `set_placed_by.json.answered` went 1,684 -> 2,010 on 2026-08-18
            # and the scan cried wolf the moment it did. One false positive is
            # how a scan gets switched off.
            if _mentions(text, token) and not _mentions(covered, token):
                bad.append(("CITATION-UNREGISTERED", rel, name,
                            f"{token} appears here and no citation covers it"))
    return bad


def failures(derived):
    """Every crosscheck that did not hold, as `(table, claim, got, want)`."""
    bad = []
    for name, d in derived.items():
        for claim, got, want in d["crosschecks"]:
            if got != want:
                bad.append((name, claim, got, want))
    return bad


def comparable(d):
    """The part of a derivation `--check` pins. Example blocks are not it:
    a generator that widens its 8-block sample is not a drift in the ledger."""
    return {name: {"unit": v["unit"], "universe": v["universe"],
                   "population": v["population"], "refused": v["refused"],
                   "answered": v["answered"], "other": v["other"],
                   "causes": v["causes"]}
            for name, v in d.items()}


def main():
    derived, missing = derive()
    if missing:
        print(f"refusal-ledger: {', '.join(missing)} missing from {DATA} — "
              f"run ci/gen-gameplay-tables.sh", file=sys.stderr)
        return 2

    bad = failures(derived) + census(derived) + three(derived)
    for name, claim, got, want in bad:
        print(f"INCONSISTENT  {name}: {claim} — got {got}, want {want}",
              file=sys.stderr)
    if bad:
        print("refusal-ledger: a generator's rows and its own summary disagree; "
              "nothing below can be quoted", file=sys.stderr)
        return 1

    cites = cite(derived)
    if "--check" in sys.argv or "--cite" in sys.argv:
        for kind, f, path, detail in cites:
            print(f"{kind}  {f} [{path}] — {detail}", file=sys.stderr)
    if cites:
        print(f"refusal-ledger: {len(cites)} citation(s) do not match the tables",
              file=sys.stderr)
        if "--check" in sys.argv or "--cite" in sys.argv:
            return 1
    if "--cite" in sys.argv:
        n = len(tomllib.loads(CITATIONS.read_text()).get("cite", [])) \
            if CITATIONS.is_file() else 0
        print(f"REFUSAL-CITATIONS OK — {n} sentence(s) quote these tables and "
              f"every one matches")
        return 0

    if "--check" in sys.argv:
        if not LEDGER.is_file():
            print(f"refusal-ledger: {LEDGER.relative_to(ROOT)} is missing — "
                  f"run ci/refusal-ledger.py --write", file=sys.stderr)
            return 1
        want = json.loads(LEDGER.read_text())["tables"]
        got = comparable(derived)
        drift = False
        for name in sorted(set(want) | set(got)):
            # An optional table the ledger has and this tree never generated is
            # not drift — it is a tree that did not run that generator, and
            # saying so out loud is the difference between "could not be
            # checked" and "checked and fine".
            if name in OPTIONAL and name not in got:
                print(f"refusal-ledger: {name} skipped — recorded in the ledger "
                      f"and not generated in this tree")
                continue
            if want.get(name) != got.get(name):
                drift = True
                print(f"DRIFTED  {name}", file=sys.stderr)
                w, g = want.get(name, {}), got.get(name, {})
                for k in sorted(set(w) | set(g)):
                    if w.get(k) != g.get(k):
                        print(f"    {k}: ledger {w.get(k)!r}, tables {g.get(k)!r}",
                              file=sys.stderr)
        if drift:
            print(f"REFUSAL-LEDGER DRIFTED — re-derive "
                  f"{LEDGER.relative_to(ROOT)} and fix every doc that quotes it",
                  file=sys.stderr)
            return 1
        total = sum(v["refused"] for v in got.values())
        n = len(tomllib.loads(CITATIONS.read_text()).get("cite", [])) \
            if CITATIONS.is_file() else 0
        print(f"REFUSAL-LEDGER OK — {total} refusals across {len(got)} tables, "
              f"as recorded; {n} citation(s) still quote them correctly")
        return 0

    if "--write" in sys.argv:
        # **A table this tree did not generate is CARRIED, not dropped.**
        #
        # `--write` derives from the tables present on disk, and
        # `use_item_on.json` is the optional fifth population: nothing under
        # `crates/` opens it, its dumper is ~25 minutes, and most trees never
        # build it. Writing the derivation straight out therefore deleted a
        # whole recorded population — 17 lines, 4,537 states and two causes —
        # in a tree that had simply not built it, and `--check` then passed,
        # because a table that is absent from both the ledger and the tree is
        # not drift. It is amnesia, and it looks exactly like a clean run.
        #
        # Found on 2026-08-18 by a one-field edit to `set_placed_by.json`:
        # `--write` moved `answered` 2010 -> 2046 and took `use_item_on.json`
        # with it. The `skipped — recorded in the ledger and not generated in
        # this tree` line that `--check` prints is what makes this recoverable
        # rather than silent, and it is now what `--write` reads too.
        kept = {}
        if LEDGER.is_file():
            kept = json.loads(LEDGER.read_text()).get("tables", {})
        fresh = comparable(derived)
        carried = sorted(set(kept) - set(fresh))
        merged = {**{k: kept[k] for k in carried}, **fresh}
        for name in carried:
            print(f"refusal-ledger: {name} carried unchanged — recorded in the "
                  f"ledger and not generated in this tree, so it is kept "
                  f"rather than dropped", file=sys.stderr)
        LEDGER.parent.mkdir(parents=True, exist_ok=True)
        LEDGER.write_text(json.dumps(
            {"what": "the golden generators' refusal populations, derived by "
                     "ci/refusal-ledger.py; --check refuses on drift",
             "tables": merged}, indent=2, sort_keys=True) + "\n")
        print(f"wrote {LEDGER.relative_to(ROOT)}")
        return 0

    if "--table" in sys.argv:
        print("| table | unit | population | refused | % | answered |")
        print("|---|---|---|---:|---:|---:|")
        for name, d in derived.items():
            pct = d["refused"] * 100.0 / d["population"] if d["population"] else 0.0
            print(f"| `{name}` | {d['unit']} | {d['population']} | "
                  f"{d['refused']} | {pct:.2f}% | {d['answered']} |")
        print()
        print("| table | what the harness could not answer | refused |")
        print("|---|---|---:|")
        for name, d in derived.items():
            for cause, n in sorted(d["causes"].items(), key=lambda kv: -kv[1]):
                print(f"| `{name}` | {cause} | {n} |")
        return 0

    for name, d in derived.items():
        pct = d["refused"] * 100.0 / d["population"] if d["population"] else 0.0
        print(f"{name}")
        print(f"  unit          {d['unit']}")
        print(f"  universe      {d['universe']}")
        print(f"  population    {d['population']}"
              + ("" if d["population"] == d["universe"]
                 else "   (the rest is inert and has no row)"))
        print(f"  refused       {d['refused']}   {pct:.2f}% of the population")
        print(f"  answered      {d['answered']}")
        for k, v in d["other"].items():
            print(f"  {k:<13} {v}")
        for cause, n in sorted(d["causes"].items(), key=lambda kv: -kv[1]):
            ex = ", ".join(d["blocks"].get(cause, [])[:3])
            print(f"    {n:>7}  {cause}" + (f"   e.g. {ex}" if ex else ""))
    total = sum(v["refused"] for v in derived.values())
    units = {v["unit"] for v in derived.values()}
    # Derived, not written down. The first version of this line said "four
    # tables, in three different units" and both halves stopped being true the
    # moment `use_item_on.json` was read — in a script whose entire subject is
    # counts that stop being true.
    print(f"\n{total} refusals in {len(derived)} tables, in {len(units)} "
          f"different units. Say which before you quote one.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
