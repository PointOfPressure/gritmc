#!/usr/bin/env python3
"""Prove `ci/check-figures.py` rules 9-18 and the bench window rule can go red.

    python3 ci/check-figures-falsify.py     # ~0.5 s, no build, no jar, no box

# WHY THIS EXISTS AND WHY IT IS NOT OPTIONAL

`ci/check-figures.py`'s own header names the defect class it must not join:
"a check that could not fail", found twenty-one times on this project. Rules
1-8 answer that with `[meta] min_figures`, `min_unquotable` and
`min_compare_pairs` — floors asserting that live entries exercise each path.

Rules 9-13 cannot be defended that way alone, and the reason is the awkward
one: **the fix that closed each fault removed the only entry that violated
it.** After 2026-08-13 nothing in `ci/figures.toml` fails rule 9, 10, 11, 12 or
13. `min_restatements` keeps the *relation* alive so rules 9 and 10 stay
reachable, but reachable is not the same as working, and a rule proven by the
absence of a violation is not proven. So each fault is injected here and the
expected rule is asserted by name.

# THE FIXTURE IS SYNTHETIC, AND ONE CASE IS THE WHOLE ARGUMENT

Following `ci/check-use-probe-falsify.sh`: a hermetic tree in `mktemp -d`
holding a copy of the checker, a five-entry ledger and the documents and
artefacts it cites. Run against the real ledger this would be hostage to which
figures happen to be registered on the day.

The case that matters most is a GREEN one. `cmp-not-a-restatement` compares
against a disqualified target and is quotable, and it must stay green — that
is `use-table-probe-moves-20260811`'s exact shape, an independent quantity
measured on one window whose target is disqualified as an `upper bound`. One
mutation away — give it the target's own number — it must go red. If rule 9
fired on both, it would be indistinguishable from "a restatement may not be
quotable" AND from "nothing may compare against a disqualified figure", and
only the first of those is true. A rule that reds on everything is as useless
as one that reds on nothing, and this pair is what separates them.

# THE BENCH WINDOW RULE, ADDED 2026-08-18

`bench` is the only kind whose entry in `REQUIRED_WINDOW` is `[]`, with the
constraint written as prose beside it — "checked below: needs radius+chunks OR
a level/view_distance set". That is the shape of an unfalsifiable check and it
was never tested, because all eleven live bench entries satisfy one arm or the
other and a rule proven by the absence of a violation is not proven. It was
tested here and the promise is TRUE: the disjunction reds on neither arm, on
half the generation arm and on half the running-server arm, and stays green on
the running-server arm alone. That last one is a control and not a formality —
`boot-superflat` and the three `rss-*` entries carry no radius and no chunk
count, so anyone "tightening" the rule by moving radius+chunks into
`REQUIRED_WINDOW["bench"]` would red four honest entries, and this case is what
makes that read as a break rather than as a fix.

What the rule does NOT constrain is a separate matter and is not fixed by
testing it: a bench figure is not required to name the commit it was taken on,
nor the size of the worker pool it ran against. See `REQUIRED_WINDOW["bench"]`.

Exit 0 all cases behaved, 1 a case did not, 2 the harness could not run.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from copy import deepcopy
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHECKER = ROOT / "ci" / "check-figures.py"

# The window every worldgen entry in the fixture carries. Eleven keys, the same
# shape `ow-r16-admissible-n2` carries in the real ledger, because rule 10 is
# about a restatement covering ALL of its target's keys and a two-key fixture
# would not exercise that.
WINDOW = {
    "dimension": "overworld",
    "seed": "1",
    "radius": "16",
    "centre": "0,0",
    "chunks": "1089",
    "refs": "2",
    "harvest": "rows",
    "dump_order": "zx (MATCHED)",
    "mask": "none",
    "order": "vanilla-first",
    "ybox": "full",
}

# `src` is disqualified; `restate` republishes its number and inherits that;
# `cmp-not-a-restatement` compares against it with a number of its own and is
# quotable, which is the discrimination case; `lvl` is an ordinary quotable
# level with a floor for rule 6 to fire on.
BASE: list[dict] = [
    {
        "id": "src",
        "unit": "percent-of-blocks",
        "question": "On the fixture window, what share of block positions is the generator charged for differing on?",
        "kind": "worldgen",
        "doc": "docs/OWNER.md",
        "claim": "90.000000%",
        "claim_count": 1,
        "percent": "90.000000%",
        "residual": 100,
        "denominator": 1000,
        "floor": 50,
        "quotable": False,
        "disqualifier": "uncontrolled chunk order",
        "reason": "the fixture's withdrawn measurement",
        "baseline": "unmodified",
        "artefact": "ci/fixture-artefact.sh",
        "artefact_claim": "SRC 100 of 1000",
        "provenance": "FIXTURE_SHA",
        "window": dict(WINDOW),
    },
    {
        "id": "restate",
        "unit": "percent-of-blocks",
        "question": "On the fixture window, what share of block positions is the generator charged for differing on, as README restates it?",
        "kind": "worldgen",
        "doc": "README.md",
        "claim": "90.000000%",
        "claim_count": 1,
        "percent": "90.000000%",
        "residual": 100,
        "denominator": 1000,
        "floor": 50,
        "quotable": False,
        "disqualifier": "uncontrolled chunk order",
        "reason": "inherits src's withdrawal because it is src's number",
        "baseline": "unmodified",
        "compare_to": "src",
        "compare_keys": sorted(WINDOW),
        "artefact": "ci/fixture-artefact.sh",
        "artefact_claim": "SRC 100 of 1000",
        "provenance": "FIXTURE_SHA",
        "window": dict(WINDOW),
    },
    {
        "id": "cmp-not-a-restatement",
        "unit": "table-rows",
        "question": "On the fixture window, how many of the 184 table rows differ, as a comparison that is not a restatement?",
        "kind": "worldgen",
        "doc": "docs/OWNER.md",
        "claim": "17 of 184 rows",
        "claim_count": 1,
        "percent": "80.000000%",
        "residual": 200,
        "denominator": 1000,
        "floor": 50,
        "quotable": True,
        "baseline": "unmodified",
        "compare_to": "src",
        "compare_keys": ["dimension", "seed", "radius"],
        "artefact": "ci/fixture-artefact.sh",
        "artefact_claim": "CMP 200 of 1000",
        "provenance": "FIXTURE_SHA",
        "window": dict(WINDOW),
    },
    # A `bench` entry, because `bench` is the one kind whose window is NOT
    # declared in `REQUIRED_WINDOW` — it is `[]` there, and the rule that
    # actually constrains it is a disjunction written in `check_one`. An empty
    # required-window list is the shape of an unfalsifiable check, so the
    # disjunction is the thing that has to be shown firing. The generation arm
    # (radius+chunks) is the fixture's default; the running-server arm
    # (level+view_distance) is exercised as a GREEN control below, because a
    # rule that reds on both arms would be an AND wearing an OR's comment.
    {
        "id": "bch",
        "unit": "cpu-seconds-per-chunk",
        "question": "On the fixture square, how many CPU-seconds does the bench spend per chunk?",
        "kind": "bench",
        "doc": "docs/OWNER.md",
        "claim": "40.0 CPU-seconds",
        "claim_count": 1,
        "quotable": True,
        "baseline": "unmodified",
        "artefact": "ci/fixture-artefact.sh",
        "artefact_claim": "BCH 40.0 CPU-seconds",
        "provenance": "FIXTURE_SHA",
        "window": {
            "radius": "7",
            "chunks": "225",
            "seed": "1",
            "heap": "4G",
            "machine": "fixture",
            "workers": "12",
        },
    },
    {
        "id": "lvl",
        "unit": "percent-of-blocks",
        "question": "On the fixture window at its own N, what share of block positions is the generator charged for differing on?",
        "kind": "worldgen",
        "doc": "docs/OWNER.md",
        "claim": "97.000000%",
        "claim_count": 1,
        "percent": "97.000000%",
        "residual": 30,
        "denominator": 1000,
        "floor": 10,
        "quotable": True,
        "baseline": "unmodified",
        "artefact": "ci/fixture-artefact.sh",
        "artefact_claim": "LVL 30 of 1000",
        "provenance": "FIXTURE_SHA",
        "window": dict(WINDOW),
    },
]

META = {
    "audited": "fixture",
    "min_figures": 5,
    "min_unquotable": 2,
    "min_compare_pairs": 2,
    "min_restatements": 1,
    "max_quotable_without_dump_order": 0,
    "max_unregistered_dashboard_rows": 0,
    "max_quotable_bench_without_workers": 0,
    # 2026-08-17. Both keys are MANDATORY in the checker — an absent floor is
    # red — so the fixture has to carry them or the unmutated fixture is not
    # green and every case below becomes meaningless. That guard fired the first
    # time these rules ran, which is the harness doing its job.
    #
    # `min_external_artefacts` is 0 here ON PURPOSE and it is the one place that
    # value is honest: the fixture's artefacts are all fixture documents, so
    # there is nothing external to count. In the real ledger 0 was the bug.
    "min_external_artefacts": 0,
    # The fixture points `artefact` at documents other than each figure's own
    # `doc`, which is the permitted shape, so no entry is self-artefacted.
    "max_self_artefact": 0,
    # The fixture's two documents each mention the other's figure WITH the
    # disqualifier beside it, so there are no unqualified echoes to allow for.
    # 0 is the honest fixture value; in the real ledger it is 24 and a ratchet.
    "max_unqualified_echoes": 0,
}

DOCS = {
    # `src` and `cmp` live here. The disqualifier has to be printed beside the
    # claim or rule 4's UNQUALIFIED fires, which is itself worth having in a
    # fixture: it is the rule that keeps an unquotable figure from being
    # published bare.
    "docs/OWNER.md": (
        "# owner\n\n"
        "src reads 90.000000% — 100 of 1000 — not quotable: uncontrolled chunk order.\n"
        "cmp reads 17 of 184 rows on the same window.\n"
        "lvl reads 97.000000%.\n"
        "bch reads 40.0 CPU-seconds.\n"
    ),
    # Rule 19 reads `docs/PARITY.md` 2 specifically, because that table is the
    # dashboard and is made entirely of counts. The fixture carries a two-row
    # one: `lvl`'s residual, which is registered, and a `not measured` row,
    # which is the honest-absence shape the rule must not count.
    "docs/PARITY.md": (
        "# parity\n\n"
        "## 2. Where parity stands\n\n"
        "| dimension | seed | window | blocks | residual | floor (N) |\n"
        "|---|---|---|---|---|---|\n"
        "| overworld | 1 | r=16 | 1000 | 30 | 10 (N=2) |\n"
        "| nether | 1 | r=7 | 500 | not measured | not measured |\n\n"
        "## 3. after\n"
    ),
    "README.md": (
        "# readme\n\n"
        "restated here as 90.000000% — 100 of 1000 — not quotable, "
        "uncontrolled chunk order.\n"
    ),
}

GREEN = "(green control)"

ARTEFACT = (
    "#!/bin/sh\n# SRC 100 of 1000\n# CMP 200 of 1000\n# LVL 30 of 1000\n"
    "# BCH 40.0 CPU-seconds\n"
    # Rule 20's case mutates `lvl`'s artefact_claim to carry a `corpus=` stamp;
    # rule 2 still has to find it here, or the case would go red for rule 2's
    # reason instead of rule 20's and prove nothing.
    "# LVL 30 of 1000 corpus=92\n"
)


def emit(value) -> str:
    """The subset of TOML this fixture needs: str, int, bool, list[str]."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(emit(v) for v in value) + "]"
    return '"' + str(value).replace("\\", "\\\\").replace('"', '\\"') + '"'


FIXTURE_SHA = "0000000"


def ledger(figures: list[dict], meta: dict) -> str:
    # A `coverage` key in `meta` is emitted as its own `[coverage]` TABLE rather
    # than as a scalar — added 2026-08-17 for the COVERAGE-GLOB-EMPTY case. All
    # `[meta]` scalars are written before it, which is what keeps the TOML valid.
    coverage = dict(meta.get("coverage") or {})
    out = ["[meta]"]
    for k, v in meta.items():
        if k == "coverage":
            continue
        out.append(f"{k} = {emit(v)}")
    if coverage:
        out.append("\n[coverage]")
        for k, v in coverage.items():
            out.append(f"{k} = {emit(v)}")
    # Otherwise no `[coverage]` table: the coverage scan and the gate-floor scan
    # are rules 1-8's business and are exercised by the real ledger every T0.
    # The one exception is the membership of the scan itself — a `doc_globs`
    # entry matching no file — which no real ledger can demonstrate, because a
    # real ledger's globs match.
    for fig in figures:
        out.append("\n[[figure]]")
        for k, v in fig.items():
            if k == "window":
                inner = ", ".join(f"{wk} = {emit(wv)}" for wk, wv in v.items())
                out.append(f"window = {{ {inner} }}")
            elif k == "provenance":
                out.append(f"provenance = {emit(str(v).replace('FIXTURE_SHA', FIXTURE_SHA))}")
            else:
                out.append(f"{k} = {emit(v)}")
    return "\n".join(out) + "\n"


def run(
    work: Path, figures: list[dict], meta: dict, docs: dict | None = None
) -> tuple[int, str]:
    # Rewritten every call. A case that needs a different document must not
    # leak it into the next one, and rule 13's case needs exactly that.
    for rel, body in {**DOCS, **(docs or {})}.items():
        (work / rel).write_text(body, encoding="utf-8")
    (work / "ci" / "figures.toml").write_text(ledger(figures, meta), encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, str(work / "ci" / "check-figures.py")],
        capture_output=True,
        text=True,
    )
    return proc.returncode, proc.stdout + proc.stderr


def by_id(figures: list[dict], fid: str) -> dict:
    return next(f for f in figures if f["id"] == fid)


def main() -> int:
    if not CHECKER.exists():
        print("check-figures-falsify: ci/check-figures.py is missing", file=sys.stderr)
        return 2

    work = Path(tempfile.mkdtemp(prefix="figures-falsify."))
    try:
        (work / "ci" / "checks").mkdir(parents=True)
        # Rule 15 resolves a provenance sha against the repository, so the
        # fixture needs to BE one — otherwise the rule silently degrades to its
        # shape half and the case below would prove nothing about the arm that
        # matters. One empty commit is enough, and its sha is substituted into
        # every fixture entry's provenance.
        for cmd in (
            ["git", "init", "-q", str(work)],
            ["git", "-C", str(work), "config", "user.email", "f@f"],
            ["git", "-C", str(work), "config", "user.name", "fixture"],
            ["git", "-C", str(work), "commit", "-q", "--allow-empty", "-m", "fixture"],
        ):
            if subprocess.run(cmd, capture_output=True).returncode != 0:
                print("check-figures-falsify: could not build the git fixture "
                      "rule 18 needs", file=sys.stderr)
                return 2
        global FIXTURE_SHA
        FIXTURE_SHA = subprocess.run(
            ["git", "-C", str(work), "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True,
        ).stdout.strip()
        (work / "docs").mkdir()
        shutil.copy(CHECKER, work / "ci" / "check-figures.py")
        # `ci/check-figures.py` reads its object store through `ci/lib/box_env.py`
        # since 2026-08-18, so the fixture tree needs it too. Without it the
        # copied checker dies on `ModuleNotFoundError` and every case below
        # reports its injected fault as having gone green — which is what
        # happened, and is why the fixture is assembled by naming files rather
        # than by copying a directory: a new dependency has to be added here on
        # purpose. The alternative, copying `ci/` wholesale, would make the
        # fixture quietly track the tree and stop being hermetic.
        (work / "ci" / "lib").mkdir()
        shutil.copy(ROOT / "ci" / "lib" / "box_env.py", work / "ci" / "lib" / "box_env.py")
        (work / "ci" / "fixture-artefact.sh").write_text(ARTEFACT, encoding="utf-8")
        for rel, body in DOCS.items():
            (work / rel).write_text(body, encoding="utf-8")

        # -- the fixture must be GREEN before any injection means anything ---
        code, out = run(work, BASE, META)
        if code != 0:
            print("check-figures-falsify: the unmutated fixture is not green — "
                  "every case below would be meaningless\n" + out, file=sys.stderr)
            return 2

        cases: list[tuple[str, str, list[dict], dict, dict]] = []

        def case(name: str, rule: str, mutate, docs: dict | None = None) -> None:
            figs = deepcopy(BASE)
            # `deepcopy`, not `dict(META)` — 2026-08-17. A shallow copy was
            # correct while every META value was a scalar, and the
            # COVERAGE-GLOB-EMPTY case puts a nested dict in there. A mutation
            # reaching through a shallow copy into shared state is the
            # test-isolation defect this file already had once, in its documents
            # rather than its meta, so it is closed by construction instead of by
            # every future case remembering.
            meta = deepcopy(META)
            mutate(figs, meta)
            # THE DOCUMENT LEAK, CLOSED BY CONSTRUCTION RATHER THAN BY LUCK.
            # `run()` rewrites every key of `DOCS` on every call, so a case that
            # overrides one cannot leak into the next — which holds only as long
            # as the override IS a DOCS key. A case introducing a new file would
            # write it once and leave it on disk for every later case, green or
            # red, and nothing would say so. That is the same shape as the leak
            # already fixed here, so it is refused rather than documented.
            unknown = sorted(set(docs or {}) - set(DOCS))
            if unknown:
                raise AssertionError(
                    f"case {name!r} supplies document(s) {unknown} that are not "
                    "in DOCS; run() would not rewrite them for later cases and "
                    "the file would leak forward. Add them to DOCS."
                )
            cases.append((name, rule, figs, meta, docs or {}))

        # -- rule 9 ----------------------------------------------------------
        def m_restate_quotable(figs, _meta):
            f = by_id(figs, "restate")
            f["quotable"] = True
            f.pop("disqualifier")
            f.pop("reason")

        case(
            "a restatement marked quotable while its source is withdrawn "
            "(the 2026-08-10..13 fault, exactly)",
            "RESTATED-QUOTABILITY",
            m_restate_quotable,
        )

        # `republishes()` is an OR of two arms and the case above exercises
        # only the `percent` one. Strip the percentages and leave the residual
        # over the denominator identical: the entries then publish the same
        # number in a form that has no percentage at all, which is what most
        # of the real ledger's counts look like (`89,104`, `7 of 88`,
        # `115 blocks deep`). If only the first arm worked, rules 9 and 10
        # would silently cover the percentage-bearing third of the file.
        def m_restate_no_percent(figs, _meta):
            for fid in ("src", "restate"):
                f = by_id(figs, fid)
                f.pop("percent")
                f["claim"] = "100 of 1000"
                f["claim_count"] = 1
            r = by_id(figs, "restate")
            r["quotable"] = True
            r.pop("disqualifier")
            r.pop("reason")

        case(
            "the same fault on a figure with no percentage — matched on "
            "residual over denominator instead",
            "RESTATED-QUOTABILITY",
            m_restate_no_percent,
        )

        # -- rule 10 ---------------------------------------------------------
        def m_drop_dump_order(figs, _meta):
            f = by_id(figs, "restate")
            f["compare_keys"] = [k for k in f["compare_keys"] if k != "dump_order"]

        case(
            "a restatement omitting `dump_order` from compare_keys — the key "
            "its source was disqualified on",
            "RESTATED-WINDOW",
            m_drop_dump_order,
        )

        def m_drop_any_key(figs, _meta):
            f = by_id(figs, "restate")
            f["compare_keys"] = [k for k in f["compare_keys"] if k != "ybox"]

        case(
            "a restatement omitting any one of its source's window keys",
            "RESTATED-WINDOW",
            m_drop_any_key,
        )

        # -- rule 11 ---------------------------------------------------------
        def m_no_floor(figs, _meta):
            by_id(figs, "lvl").pop("floor")

        case(
            "a quotable worldgen level with no floor and no floor_note",
            "FLOOR-UNDECLARED",
            m_no_floor,
        )

        # -- rule 12 ---------------------------------------------------------
        def m_no_dump_order(figs, _meta):
            by_id(figs, "lvl")["window"].pop("dump_order")

        case(
            "a quotable worldgen level whose window records no dump_order",
            "DUMP-ORDER-UNRECORDED",
            m_no_dump_order,
        )

        # -- rule 13 ---------------------------------------------------------
        #
        # The fixture's own docs/OWNER.md qualifies `src` correctly, so rule 4
        # is satisfied and stays satisfied here. What changes is that the same
        # withdrawn number also appears on a line marked as settled — which is
        # exactly how `docs/PARITY.md` read on 2026-08-13, rule 4 green and a
        # reader landing on an endorsement.
        def m_endorsed(figs, _meta):
            by_id(figs, "src")["claim_count"] = 2

        case(
            "a withdrawn figure printed on a line marked as settled, with "
            "rule 4 still green",
            "VERDICT-MARKER",
            m_endorsed,
            {
                "docs/OWNER.md": DOCS["docs/OWNER.md"]
                + "\n✅ **settled: 90.000000% is the one that fits.**\n"
            },
        )

        # -- the floor that keeps rules 9 and 10 reachable -------------------
        def m_no_restatements(figs, _meta):
            r = by_id(figs, "restate")
            r["residual"] = 101
            r["percent"] = "89.900000%"
            r["claim"] = "89.900000%"
            r["claim_count"] = 0

        case(
            "the last restatement stops being one, leaving rules 9 and 10 "
            "unreachable",
            "RULE-UNEXERCISED",
            m_no_restatements,
        )

        # -- the two floors that had no code behind them until 2026-08-17 ----
        #
        # `min_external_artefacts` was declared in `ci/figures.toml` from the
        # start and `git log -S` finds no commit in the checker's history that
        # read it. These cases exist so it cannot go back to being a number
        # nothing reads: if either floor is dropped from the checker, the case
        # below stops going red and this harness says so.
        def m_external_floor(_figs, meta):
            meta["min_external_artefacts"] = 99

        case(
            "the external-artefact floor is raised above what the ledger has",
            "EXTERNAL-ARTEFACTS",
            m_external_floor,
        )

        def m_external_floor_absent(_figs, meta):
            meta.pop("min_external_artefacts")

        case(
            "the external-artefact floor is deleted outright — an absent floor "
            "is how this key spent its whole life being ignored",
            "MALFORMED",
            m_external_floor_absent,
        )

        # SELF-ARTEFACT: `artefact` equal to `doc` makes rule 2 re-read the file
        # rule 1 already read, so it cannot fail independently. Seven real
        # entries are that shape and the ceiling holds them; what may not happen
        # is an eighth.
        def m_self_artefact(figs, _meta):
            f = by_id(figs, "lvl")
            f["artefact"] = f["doc"]
            f["artefact_claim"] = f["claim"]

        case(
            "a figure's artefact is set to its own doc, making rule 2 vacuous "
            "on it",
            "SELF-ARTEFACT",
            m_self_artefact,
        )

        def m_self_ceiling_absent(_figs, meta):
            meta.pop("max_self_artefact")

        case(
            "the self-artefact ceiling is deleted, leaving the ratchet with no "
            "ceiling to exceed",
            "MALFORMED",
            m_self_ceiling_absent,
        )

        # -- rule 4's PASS was as dangerous as its fail ----------------------
        #
        # `floor >= residual` says nothing about whether the two numbers came
        # from the same instrument. On the real ledger they did not:
        # ow-r16-admissible-n2's residual is a no-mask charge and its floor is
        # the twin-mask width, so rule 4 read 29,560 < 29,605, declined to
        # disqualify, and reported the gap as resolving power — the "45 blocks of
        # headroom" docs/DETERMINISM.md §14 killed, generated by the checker
        # itself. Flipping that entry quotable on main's checker is GREEN, exit 0.
        def m_floor_not_comparable(figs, _meta):
            f = by_id(figs, "lvl")
            f["floor_comparable"] = False
            f["floor_basis"] = "the width of a mask this instrument does not use"

        case(
            "a quotable level whose floor and residual come from different "
            "instruments — rule 4 PASSES this and calls it resolving power",
            "FLOOR-NOT-COMPARABLE",
            m_floor_not_comparable,
        )

        def m_floor_basis_missing(figs, _meta):
            by_id(figs, "src")["floor_comparable"] = False

        case(
            "floor_comparable = false with no floor_basis — the flag without the "
            "mismatch it is supposed to name",
            "MALFORMED",
            m_floor_basis_missing,
        )

        # -- rules 4 and 13 outside a figure's own `doc` ---------------------
        #
        # Both rules read `texts[fig["doc"]]` and nothing else, so a withdrawn
        # figure endorsed in another document was invisible to them. In the real
        # tree that is how docs/WORKLIST.md called the withdrawn 29,605 "the
        # quotable figure" for a week. ECHO-VERDICT-MARKER has ZERO violations on
        # the real ledger, so this case is the only thing keeping it from being a
        # rule proven by an absence — contrast it with the VERDICT-MARKER case
        # above, which endorses the claim in the figure's OWN doc.
        def m_echo_endorsed(_figs, _meta):
            pass

        case(
            "a withdrawn figure endorsed in a document that is NOT its own — "
            "invisible to rules 4 and 13, which read only the registered doc",
            "ECHO-VERDICT-MARKER",
            m_echo_endorsed,
            docs={
                "README.md": DOCS["README.md"]
                + "\n\u2705 **settled: 90.000000% is the one that fits.**\n"
            },
        )

        def m_echo_ceiling_absent(_figs, meta):
            meta.pop("max_unqualified_echoes")

        case(
            "the unqualified-echo ceiling is deleted, leaving that ratchet with "
            "no ceiling to exceed",
            "MALFORMED",
            m_echo_ceiling_absent,
        )

        # A bare restatement in another document, far enough from any mention of
        # the disqualifier that a reader landing there finds only the number.
        # The padding is the point: rule 4's window is 4,000 characters, so a
        # short fixture would pass for the wrong reason.
        def m_echo_bare(_figs, _meta):
            pass

        case(
            "a withdrawn figure's number restated bare in another document, "
            "beyond the disqualifier's reach",
            "ECHO-UNQUALIFIED",
            m_echo_bare,
            docs={
                "README.md": DOCS["README.md"]
                + ("\nfiller. " * 700)
                + "\nand the answer is 90.000000%.\n"
            },
        )

        # -- the coverage scan's own membership ------------------------------
        #
        # A `doc_globs` entry matching nothing is a scan over an empty set, which
        # is the `min_external_artefacts` fault in another costume: it runs, it
        # passes, and it looked at no file.
        def m_empty_glob(_figs, meta):
            meta["coverage"] = {
                "docs": [],
                "doc_globs": ["docs/no-such-directory/*.md"],
                "pattern": r"[0-9]+\.[0-9]{4,}%",
            }

        case(
            "a coverage doc_glob that matches no file — a scan over an empty "
            "set cannot go red",
            "COVERAGE-GLOB-EMPTY",
            m_empty_glob,
        )

        # -- two of rules 1-8, to prove the fixture is faithful to the real
        #    checker rather than to a stripped-down parse of it --------------
        def m_doc_drift(figs, _meta):
            by_id(figs, "lvl")["claim_count"] = 2

        case("a claim_count that no longer matches the document", "DOC-DRIFT", m_doc_drift)

        def m_window_mismatch(figs, _meta):
            by_id(figs, "cmp-not-a-restatement")["window"]["radius"] = "7"

        case(
            "an r=7 measurement compared against an r=16 target",
            "WINDOW-MATCH",
            m_window_mismatch,
        )

        # -- the bench window disjunction, the one kind with no declared
        #    REQUIRED_WINDOW ------------------------------------------------
        #
        # `REQUIRED_WINDOW["bench"]` is `[]`, and the comment beside it says the
        # constraint is "checked below: needs radius+chunks OR a
        # level/view_distance set". An empty required-window list next to a
        # prose promise is the exact shape this project has found unfalsifiable
        # twenty-one times, and the promise had never been tested: all eleven
        # live bench entries satisfy one arm or the other, so the rule was
        # proven only by the absence of a violation. These four cases inject
        # the violations.

        def m_bench_no_window(figs, _meta):
            by_id(figs, "bch")["window"] = {"seed": "1", "heap": "4G",
                                            "machine": "fixture"}

        case(
            "a bench figure carrying neither arm — no square and no server",
            "WINDOW-INCOMPLETE",
            m_bench_no_window,
        )

        # Each arm is a PAIR, and half of one is the likeliest real mistake:
        # `radius` reads like the whole window of a generation bench and
        # `chunks` looks derivable from it. It is not derivable — a radius says
        # how far, a chunk count says how much was actually walked, and the
        # r=16 rows in the live ledger carry both because 1,089 is a fact about
        # the run and not about the radius.
        def m_bench_half_gen(figs, _meta):
            by_id(figs, "bch")["window"].pop("chunks")

        case(
            "a bench figure with a radius but no chunk count — half the "
            "generation arm is not the arm",
            "WINDOW-INCOMPLETE",
            m_bench_half_gen,
        )

        def m_bench_half_run(figs, _meta):
            w = by_id(figs, "bch")["window"]
            w.pop("radius")
            w.pop("chunks")
            w["level"] = "superflat"

        case(
            "a bench figure with a level but no view distance — half the "
            "running-server arm is not the arm either",
            "WINDOW-INCOMPLETE",
            m_bench_half_run,
        )

        # -- rule 17: the worker pool, required on BOTH arms ---------------
        #
        # The disjunction above constrains the square or the server. It does not
        # reach the pool width, and CPU-seconds is a function of the pool
        # width — 78.54 / 82.69 / 98.48 at 1 / 12 / 24 threads on one build.
        # `workers` is therefore required of every bench entry whichever arm it
        # satisfies, which is what these two cases pin: dropping it from a
        # generation-arm entry and from a running-server-arm entry must red
        # identically.
        def m_bench_no_workers(figs, _meta):
            by_id(figs, "bch")["window"].pop("workers")

        case(
            "a bench figure on the generation arm with no worker count",
            "WINDOW-INCOMPLETE",
            m_bench_no_workers,
        )

        def m_bench_run_no_workers(figs, _meta):
            w = by_id(figs, "bch")["window"]
            w.pop("radius")
            w.pop("chunks")
            w.pop("workers")
            w["level"] = "superflat"
            w["view_distance"] = "10"

        case(
            "a bench figure on the running-server arm with no worker count — "
            "the arm does not decide whether the pool matters",
            "WINDOW-INCOMPLETE",
            m_bench_run_no_workers,
        )

        # The ratchet. `unrecorded` is an accepted VALUE — banning it would push
        # an honest "we did not record it" back into a missing field, which is
        # the opposite of the point — so what stops it spreading is a ceiling
        # that may only fall, exactly as rule 12 does for `dump_order`.
        def m_bench_unrecorded(figs, _meta):
            by_id(figs, "bch")["window"]["workers"] = "unrecorded"

        case(
            "a quotable bench figure going `unrecorded` above the ratchet",
            "BENCH-WORKERS-UNRECORDED",
            m_bench_unrecorded,
        )

        # -- rule 18: a provenance must name a commit ----------------------
        #
        # Twenty-five entries carried a bare date. Both arms are tested because
        # they fail differently: a date is an honest omission and a bad sha is a
        # confident lie, and the second is the one nobody would notice.
        def m_prov_date(figs, _meta):
            by_id(figs, "lvl")["provenance"] = "2026-08-06"

        case(
            "a provenance that is a date rather than a commit",
            "PROVENANCE-NO-COMMIT",
            m_prov_date,
        )

        # `20260812` is eight hex-shaped digits and appears in four live
        # provenances as part of an ensemble name. Shape alone accepts it; only
        # resolution rejects it, which is why this rule asks git and does not
        # pattern-match.
        def m_prov_datelike(figs, _meta):
            by_id(figs, "lvl")["provenance"] = "measured on the 20260812 ensemble"

        case(
            "a provenance whose only hex-shaped run is a date in disguise",
            "PROVENANCE-NO-COMMIT",
            m_prov_datelike,
        )

        def m_prov_bad_sha(figs, _meta):
            by_id(figs, "lvl")["provenance"] = "branch floor-ladder-retake deadbee1"

        case(
            "a provenance naming a well-formed sha that is not an object here — "
            "a sha typed rather than copied",
            "PROVENANCE-NO-COMMIT",
            m_prov_bad_sha,
        )

        # -- rule 19: the dashboard is made of counts ----------------------
        #
        # `[coverage] pattern` matches `[0-9]+\.[0-9]{4,}%`, so the scan that
        # guards every percentage in seven documents could not see one number in
        # `docs/PARITY.md` 2 — the table a reader actually lands on, every cell
        # of which is a bare count. Five live residuals were in no ledger entry
        # when this was written, two of them the RAW halves of rows whose
        # order-invariant cores are fully registered.
        def m_dash_unregistered(figs, _meta):
            pass  # the document changes, not the ledger

        case(
            "a dashboard row publishing a residual no figure holds",
            "DASHBOARD-UNREGISTERED",
            m_dash_unregistered,
            {
                "docs/PARITY.md": DOCS["docs/PARITY.md"].replace(
                    "| nether | 1 | r=7 | 500 | not measured | not measured |",
                    "| nether | 1 | r=7 | 500 | 8,472 | 48 (N=2) |",
                )
            },
        )

        # -- rule 20: the run's own stamp against the typed window ---------
        #
        # Rule 2 asserts the `artefact_claim` still appears in the artefact and
        # stops there, so an entry could cite one run and describe another —
        # and eleven did, all of them saying 93 where the stamp said 92 or 106.
        # `docs/PARITY-ROADMAP.md` 4479 had already published the correction:
        # "The denominator is 92, and every document that led here said 93." It
        # reached the documents and not the ledger.
        def m_stamp_disagrees(figs, _meta):
            f = by_id(figs, "lvl")
            f["artefact_claim"] = "LVL 30 of 1000 corpus=92"
            f["window"] = dict(f["window"], scenarios="93")

        case(
            "an entry whose window says 93 and whose artefact stamps corpus=92",
            "STAMP-DISAGREES",
            m_stamp_disagrees,
            {},
        )

        passed = failed = 0
        for name, rule, figs, meta, docs in cases:
            code, out = run(work, figs, meta, docs)
            hit = f"FIGURES RED [{rule}]" in out
            if code != 0 and hit:
                passed += 1
                print(f"  ok    {rule:<22} {name}")
            else:
                failed += 1
                print(f"  FAIL  {rule:<22} {name}")
                print(f"        exit {code}, rule {'not ' if not hit else ''}reported")
                for line in out.splitlines():
                    print(f"        | {line}")

        # -- the discrimination control, which must stay GREEN ---------------
        #
        # `cmp-not-a-restatement` is quotable and compares against a withdrawn
        # target on three keys. Nothing here may fire on it. Then one mutation
        # — give it the target's own number — and rule 9 must fire on the same
        # entry, unchanged in every other respect.
        code, out = run(work, BASE, META)
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} a comparison against a withdrawn target, "
                  "publishing its own number, stays quotable")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} the baseline fixture went red")
            for line in out.splitlines():
                print(f"        | {line}")

        figs = deepcopy(BASE)
        c = by_id(figs, "cmp-not-a-restatement")
        c["percent"] = "90.000000%"
        c["residual"] = 100
        c["claim"] = "90.000000%"
        code, out = run(work, figs, META)
        if code != 0 and "FIGURES RED [RESTATED-QUOTABILITY] cmp-not-a-restatement" in out:
            passed += 1
            print(f"  ok    {'RESTATED-QUOTABILITY':<22} ...and reds the moment it "
                  "republishes that target's number instead")
        else:
            failed += 1
            print(f"  FAIL  {'RESTATED-QUOTABILITY':<22} the discrimination pair does not "
                  "separate a comparison from a restatement")
            for line in out.splitlines():
                print(f"        | {line}")

        # -- the OTHER green control, and it is the counterexample named in
        #    `republishes()`'s docstring ------------------------------------
        #
        # `use-table-probe-moves-20260812` publishes the IDENTICAL sentence to
        # its predecessor — "17 of 184 rows move when the player does" — and is
        # an independent re-measurement from a different generator, which
        # deliberately drops `generator` from its compare_keys because the
        # generator is what changed. Matching restatements on the claim STRING
        # would demand it compare on the one key its whole finding is about.
        # So: same claim string, different number, must stay green.
        figs = deepcopy(BASE)
        c = by_id(figs, "cmp-not-a-restatement")
        c["claim"] = "90.000000%"  # src's claim string, verbatim
        code, out = run(work, figs, META)
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} an entry repeating its target's claim "
                  "SENTENCE with a number of its own is not a restatement")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} claim-string identity was treated as a "
                  "restatement — the use-table-probe-moves-20260812 shape")
            for line in out.splitlines():
                print(f"        | {line}")

        # -- and the OR must really be an OR --------------------------------
        #
        # `boot-superflat` and the three `rss-*` entries are running-server
        # benches with no radius and no chunk count at all. If the disjunction
        # were an AND — or if someone "tightened" it by moving radius+chunks
        # into REQUIRED_WINDOW["bench"] — those four would go red and the
        # tightening would look like a fix. This control is what makes that
        # visible as a break instead.
        figs = deepcopy(BASE)
        w = by_id(figs, "bch")["window"]
        w.pop("radius")
        w.pop("chunks")
        w["level"] = "superflat"
        w["view_distance"] = "10"
        code, out = run(work, figs, META)
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} a bench figure on the running-server arm "
                  "alone — no radius, no chunks — stays green")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} the bench window rule is an AND, not the "
                  "OR its comment promises")
            for line in out.splitlines():
                print(f"        | {line}")

        # -- and the ratchet must not be a ban ------------------------------
        #
        # Two ways `unrecorded` is legitimate and neither may red: under the
        # ceiling, and on an entry that is not quotable in the first place. The
        # live ledger has both shapes — eight quotable at a ceiling of eight,
        # and `rss-fix-likeforlike`, which is `unrecorded` and unquotable for a
        # reason that has nothing to do with the pool.
        figs = deepcopy(BASE)
        by_id(figs, "bch")["window"]["workers"] = "unrecorded"
        code, out = run(work, figs, {**META, "max_quotable_bench_without_workers": 1})
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} an `unrecorded` pool under the ratchet is "
                  "an honest answer, not a red")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} the ratchet is behaving as a ban on "
                  "`unrecorded`")
            for line in out.splitlines():
                print(f"        | {line}")

        figs = deepcopy(BASE)
        b = by_id(figs, "bch")
        b["window"]["workers"] = "unrecorded"
        b["quotable"] = False
        b["disqualifier"] = "pool width unrecorded"
        b["reason"] = "the fixture's unquotable bench row"
        code, out = run(work, figs, META, {
            "docs/OWNER.md": DOCS["docs/OWNER.md"].replace(
                "bch reads 40.0 CPU-seconds.",
                "bch reads 40.0 CPU-seconds — not quotable: pool width unrecorded.",
            )
        })
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} an UNQUOTABLE bench figure may say "
                  "`unrecorded` freely — the ratchet counts quotable ones")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} the ratchet counted an unquotable entry")
            for line in out.splitlines():
                print(f"        | {line}")

        # -- and prose provenance must stay legal ---------------------------
        #
        # The live ledger's provenances are paragraphs: branch names, dates,
        # machine names and a sha, in any order. A rule that only accepted a
        # bare sha would force forty entries to throw away what they know.
        figs = deepcopy(BASE)
        by_id(figs, "lvl")["provenance"] = (
            "branch floor-ladder-retake off chunk-order-residual FIXTURE_SHA, "
            "2026-08-10, Threadripper; own dumps, ensemble 20260812"
        )
        code, out = run(work, figs, META)
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} a prose provenance carrying a real sha "
                  "among dates and branch names is accepted")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} rule 18 rejects the prose shape the whole "
                  "ledger is written in")
            for line in out.splitlines():
                print(f"        | {line}")

        # -- and `not measured` must stay an honest answer ------------------
        #
        # If the rule counted `not measured` it would be a demand that every
        # window in the table be measured before the table may exist, which is
        # the opposite of what the column is for. Four live rows say it.
        code, out = run(work, BASE, META)
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} a dashboard row reading `not measured` is "
                  "an honest absence, not an unregistered figure")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} rule 19 counts `not measured` as a "
                  "published number")
            for line in out.splitlines():
                print(f"        | {line}")

        # -- and an entry with no stamp is not required to invent one -------
        #
        # Most artefacts are scripts and censuses that print no `corpus=`. The
        # rule reaches an entry only when the run stamped itself, or it would
        # be a demand that every figure acquire a corpus size it never had.
        figs = deepcopy(BASE)
        by_id(figs, "lvl")["window"] = dict(by_id(figs, "lvl")["window"],
                                            scenarios="93")
        code, out = run(work, figs, META)
        if code == 0:
            passed += 1
            print(f"  ok    {GREEN:<22} an artefact that stamps no corpus size "
                  "leaves rule 20 with nothing to compare")
        else:
            failed += 1
            print(f"  FAIL  {GREEN:<22} rule 20 fired without a stamp to fire on")
            for line in out.splitlines():
                print(f"        | {line}")

        print(f"\ncheck-figures-falsify: {passed} passed, {failed} failed")
        return 1 if failed else 0
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
