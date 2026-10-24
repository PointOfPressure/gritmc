#!/usr/bin/env python3
"""The classification of packet families is declared, measured, and falsifiable.

    ci/ambient-falsify.py --check                       # the T0 gate; no recordings
    ci/ambient-falsify.py --sweep --vanilla-a A.json \\
        --vanilla-b B.json --ours O.json --scenarios DIR
    ci/ambient-falsify.py --falsify                     # red-when-broken evidence

# WHAT THIS GUARDS, AND THE DEFECT IT WAS BUILT FROM

`tools/oracle-runner/src/trace.rs` used to carry a function called
`is_housekeeping`, and it dropped twelve clientbound packet types **at record
time** — three stages before `compare::project`, which is where that file's own
module docs say leniency belongs. The list was defended by a paragraph. Nothing
read it, nothing could turn it red, and two branches whose entire subject is
the chunk stream could not write a gradeable scenario at all, because the
recorder had thrown the answer away before any decoder ran.

That is this project's central defect class wearing a new hat: not a check that
cannot fail, but a **question that cannot be asked**. The repair is a declared
table — `trace::FAMILIES` — with a `class`, a `tag` naming the mechanism, and
a stated `reason` per row; one filter in `compare::project` that acts on it; and
this file, which refuses a tree where any of that has come apart.

# THE FOUR REFUSALS

* **R1 — a family is silently dropped without a declaration.** No `return None`
  in `observe`, and no kind-literal suppression anywhere in the runner. The only
  permitted drop is `trace::is_ambient_kind`, and it must actually be called.
* **R2 — a declared family is dropped for a different reason than declared.**
  Every row's `tag` is a claim about a measurement, and `TAG_RULES` below says
  which measurement. A row tagged `no-emitter` whose census counted vanilla
  emitting it is refused, naming both numbers.
* **R3 — an opened family is not actually compared.** A `Class::Graded` row
  whose kind reached no projection over the swept recordings is a family opened
  in the table and read by nothing, which is worth less than leaving it shut,
  because it *looks* like coverage.
* **R4 — a row states no reason, or states a placeholder.**

Two of the four injections are shown ACCEPTED by `main`'s own classifier reader
before they are shown refused here, which is the bar `ci/unfalsifiable-falsify.py`
sets. The other two are marked `inexpressible`: `main` has no `class` and no
`tag`, so a wrong reason and an unread open are not defects it could commit.
That is new coverage rather than a repaired miss, and conflating the two would
be a small unfalsifiable claim inside the falsifier.

# WHY `--check` NEEDS NO RECORDINGS, AND WHAT THAT COSTS

Exactly the split `ci/corpus-falsifiable.py` makes. `--sweep` needs three fresh
recordings and the box; it writes `ci/checks/ambient-families.toml`. `--check`
reads that file plus the two Rust sources and re-derives every rule, so it runs
inside `ci/t0.sh` in milliseconds. The cost is that the evidence can go stale
against the servers, and the mitigations are the ones this repo already uses:
the evidence file records the corpus it was taken over and the commit that took
it, and a row present in the table and absent from the evidence is a refusal
rather than a skip.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TRACE = os.path.join(ROOT, "tools", "oracle-runner", "src", "trace.rs")
COMPARE = os.path.join(ROOT, "tools", "oracle-runner", "src", "compare.rs")
EVIDENCE = os.path.join(ROOT, "ci", "checks", "ambient-families.toml")

# What each tag claims, as a predicate over the census row. The whole of R2.
#
# A tag is only worth having if it can be wrong, so each of these is a
# statement the sweep can contradict. `not-self-delimiting` is the one that is
# a property of the tree rather than of the servers — it says no arm exists —
# and it is checked against `Family::arm`, which its own unit test
# (`families_declare_their_own_arms`) keeps true of `decode`.
TAG_RULES = {
    "graded": (
        lambda r: r["self_consistent_stepped"]
        and r["obs_vanilla"] > 0
        and r["obs_ours"] > 0,
        "a graded family must be self-consistent across two vanilla runs "
        "INCLUDING the step each observation landed after — which is what "
        "`compare::project` puts into the compared value — and must be "
        "emitted by both servers",
    ),
    "timer": (
        lambda r: r["measured"] and not r["self_consistent_stepped"],
        "a family claimed to arrive on a timer must have been EMITTED and must "
        "NOT be self-consistent across two runs. The `measured` half was added "
        "2026-09-06 after `update_advancements` sat on this tag with zero "
        "observations: a silent family satisfies `not self_consistent` "
        "vacuously, so without it `timer` and `unexercised` were both "
        "satisfiable by the same row and the tag could be simply wrong and "
        "still pass",
    ),
    "client-driven": (
        lambda r: r["measured"] and not r["self_consistent_stepped"],
        "a family claimed to be driven by a bit the client sent must have been "
        "emitted and must NOT be self-consistent across two runs — same "
        "vacuity guard as `timer`",
    ),
    "order-not-self-consistent": (
        lambda r: r["measured"] and not r["self_consistent_stepped"],
        "the tag names the measurement: the family must have been emitted and "
        "the two runs must disagree. If they agree, use another tag; if it was "
        "silent, it is `unexercised`",
    ),
    "no-emitter": (
        lambda r: r["obs_vanilla"] == 0 and r["emitter_sites"] == 0,
        "a family claimed to have no emitter must be counted zero times on "
        "vanilla's side AND have zero `new` sites anywhere under net/ in the "
        "jar. The census half alone is absence of evidence over eighteen "
        "scenarios; the jar half is the claim itself "
        "(`ci/packet-emitter-sites.py`)",
    ),
    "unexercised": (
        # Distinct from `no-emitter` and the distinction is the whole value of
        # the tag. `light_update` has a construction site (`ChunkHolder`) and
        # vanilla emitted none of it over the censused set, because nothing
        # there changes a light section. That is a statement about the CORPUS,
        # not about the packet, and it must not be filed under a tag that says
        # vanilla cannot send it — nor under one that says two runs disagreed,
        # which they did not; they were both silent.
        lambda r: not r["measured"] and r["emitter_sites"] > 0,
        "a family claimed unexercised must have been counted zero times AND "
        "have a construction site in the jar. If vanilla emitted it, it is "
        "measurable and wants a different tag; if it has no site, it is "
        "`no-emitter`",
    ),
    "rare-stray": (
        # A family whose only in-window emission ever seen is a rare stray.
        # `update_advancements` read 1/0, 0/0, 0/1, 0/0 over four censuses, so
        # `timer` (needs an emission) and `unexercised` (needs none) each held
        # on about half the re-takes and the row flipped the gate on the
        # other half. This tag is exact instead: AT MOST ONE observation per
        # census, never in both vanilla runs, only at a site already seen and
        # pinned in `RARE_STRAY_SITES` below, and a construction site in the
        # jar. A family provoked by a scenario shows up in both runs or twice,
        # and either reddens it; so does a stray somewhere new.
        lambda r: r["emitter_sites"] > 0
        and r["obs_vanilla"] <= 1
        and (r["obs_vanilla_a"] == 0 or r["obs_vanilla_b"] == 0)
        and "vanilla_sites" in r
        and set(r["vanilla_sites"]) <= RARE_STRAY_SITES.get(r["kind"], set()),
        "a family claimed to be a rare stray must have been observed AT MOST "
        "ONCE in the census, never in both vanilla runs, only at a site pinned "
        "in `RARE_STRAY_SITES`, and have a construction site in the jar. Two "
        "observations, both runs, or a new site means something provokes it: "
        "measure it and pick the tag that says what",
    ),
    "not-self-delimiting": (
        lambda r: not r["arm"],
        "a family claimed to be undecodable must have no decoder arm; write "
        "the arm or change the tag",
    ),
}


# **Where each `rare-stray` family has been seen, and nowhere else.** A site is
# `<scenario name>@<step>`. Widening it is a code change with a reviewer, the
# same answer `MAX_UNSWEPT` gives: a new site is a new fact about vanilla and
# should be read by somebody, not absorbed by a data edit.
#
# `update_advancements`: the two observations in four censuses (6dd091ab9 run
# A at 2027 ms, 90f2a3ed4 run B at 2090 ms) were both here — the FIRST
# scenario of a fresh world, step 0.
RARE_STRAY_SITES = {
    "update_advancements": {"dropping-an-item-throws-it-on-the-floor@0"},
}


class Refused(Exception):
    """One rule, with the numbers that broke it."""


def die(msg):
    print(f"AMBIENT-FALSIFY RED — {msg}", file=sys.stderr)
    sys.exit(1)


# --------------------------------------------------------------------------
# Parsing the table out of the tree. One reader, used by every mode.
# --------------------------------------------------------------------------


def families(trace_src=None):
    """`trace::FAMILIES` as a list of dicts, in declaration order."""
    if trace_src is None:
        trace_src = open(TRACE).read()
    tbl = re.search(r"pub const FAMILIES: &\[Family\] = &\[.*?\n\];", trace_src, re.S)
    if not tbl:
        raise Refused("could not find `pub const FAMILIES` in trace.rs")
    rows = []
    for body in re.findall(r"Family \{(.*?)\n    \}", tbl.group(0), re.S):
        kind = re.search(r'kind:\s*(?:"([a-z_0-9]+)"|([A-Z_]+))', body)
        if not kind:
            raise Refused("a FAMILIES row names no kind")
        name = kind.group(1)
        if name is None:
            # `kind: HEARTBEAT` — resolve the const out of the same file.
            const = kind.group(2)
            m = re.search(
                r'pub const %s: &str = "([a-z_0-9]+)"' % const, trace_src
            )
            if not m:
                raise Refused(f"a FAMILIES row names {const}, which is not a &str const")
            name = m.group(1)
        cls = re.search(r"class:\s*Class::(\w+)", body)
        tag = re.search(r'tag:\s*"([A-Za-z-]+)"', body)
        arm = re.search(r"arm:\s*(true|false)", body)
        reason = re.search(r'reason:\s*"(.*)",\s*\Z', body, re.S)
        pid = re.search(r"id:\s*Some\(ids::play::cb::([A-Z_0-9]+)\)", body)
        if not (cls and tag and arm and reason):
            raise Refused(f"the FAMILIES row for {name} is missing a column")
        rows.append(
            {
                "kind": name,
                "class": cls.group(1),
                "tag": tag.group(1),
                "arm": arm.group(1) == "true",
                "reason": re.sub(r'\\\s*\n\s*', " ", reason.group(1)).strip(),
                "id_name": pid.group(1) if pid else None,
            }
        )
    if len(rows) < 8:
        raise Refused(f"FAMILIES parsed only {len(rows)} rows — the reader has gone stale")
    return rows


def read_evidence(path=None):
    """`ci/checks/ambient-families.toml` -> {kind: row}, plus the header."""
    path = path or EVIDENCE
    try:
        import tomllib

        with open(path, "rb") as fh:
            doc = tomllib.load(fh)
    except FileNotFoundError:
        raise Refused(
            f"{os.path.relpath(path, ROOT)} does not exist. The classification is "
            "unmeasured: run `ci/ambient-census.sh`, which records vanilla twice and "
            "this server once and writes it."
        )
    rows = {r["kind"]: r for r in doc.get("family", [])}
    return doc.get("census", {}), rows


# --------------------------------------------------------------------------
# R1 — the source scan
# --------------------------------------------------------------------------

# A SUPPRESSION IS A KIND TEST INSIDE A FILTER, AND ONLY THERE.
#
# The first version of this regex matched any `o.kind == "..."` anywhere and
# immediately fired on `compare.rs:964`, which is `place`'s choice of *where to
# seed an entity's position from* — a selection, not a drop. Refusing that
# would have made the guard something a lane switches off, which is how a gate
# stops being a gate. So the scan is scoped to the constructs that can remove
# an observation from a projection: `.filter(`, `.retain(`, `.take_while(` and
# `.skip_while(` closures, plus an early `return None` in `observe`.
SUPPRESSORS = re.compile(r"\.(?:filter|retain|take_while|skip_while)\(")
KIND_LITERAL = re.compile(r"o\.kind\s*(?:==|!=)\s*(?:\"|crate::trace::[A-Z_]+|trace::[A-Z_]+)")


def scan_sources(trace_src=None, compare_src=None):
    """R1: nothing drops a packet except the one table-driven filter."""
    trace_src = open(TRACE).read() if trace_src is None else trace_src
    compare_src = open(COMPARE).read() if compare_src is None else compare_src
    problems = []

    # `observe` must record everything. Slice the function and look for the
    # early return that used to be there.
    m = re.search(r"pub fn observe\(.*?\n\}\n", trace_src, re.S)
    if not m:
        problems.append("could not find `pub fn observe` in trace.rs")
    elif re.search(r"\n\s*return None;", m.group(0)):
        problems.append(
            "`observe` returns None for some packet — that is a drop at RECORD "
            "time, which is the defect this table replaced: the recording then "
            "has no answer to `did the server send this`, and no projection can "
            "give it one back"
        )

    for name, src in (("compare.rs", compare_src), ("trace.rs", trace_src)):
        for n, line in enumerate(src.splitlines(), 1):
            if not SUPPRESSORS.search(line):
                continue
            if not KIND_LITERAL.search(line):
                continue
            if "is_ambient_kind" in line:
                continue
            problems.append(
                f"{name}:{n} drops observations by naming a kind outside "
                f"`trace::FAMILIES` — {line.strip()[:80]!r}. The classification is "
                "the table; add a row and let `is_ambient_kind` do it"
            )

    # And the table has to actually drive something. A declared classification
    # nothing consults is the same nothing, more expensively.
    if "is_ambient_kind" not in compare_src:
        problems.append(
            "`compare.rs` never calls `trace::is_ambient_kind`, so the "
            "classification is declared and inert"
        )
    if problems:
        raise Refused("; ".join(problems))
    return True


# --------------------------------------------------------------------------
# R2/R3/R4 — the table against the evidence
# --------------------------------------------------------------------------


def check_table(rows, census, ev):
    problems = []
    for r in rows:
        # R4
        if "PLACEHOLDER" in r["reason"] or "PLACEHOLDER" in r["tag"]:
            problems.append(f"{r['kind']}: the row is a placeholder, not a classification")
            continue
        if len(r["reason"]) < 40:
            problems.append(f"{r['kind']}: states no reason")
        if r["tag"] not in TAG_RULES:
            problems.append(
                f"{r['kind']}: tag {r['tag']!r} is not one of {sorted(TAG_RULES)}"
            )
            continue
        if (r["class"] == "Graded") != (r["tag"] == "graded"):
            problems.append(
                f"{r['kind']}: class {r['class']} and tag {r['tag']!r} disagree — "
                "`graded` is the tag of a compared family and of no other"
            )
        row = ev.get(r["kind"])
        if row is None:
            problems.append(
                f"{r['kind']}: classified in trace.rs and absent from the census. "
                "A class with no measurement behind it is the thing this file exists "
                "to refuse"
            )
            continue
        row = dict(row, arm=r["arm"], kind=r["kind"])
        # R2
        pred, why = TAG_RULES[r["tag"]]
        if not pred(row):
            problems.append(
                f"{r['kind']}: tagged {r['tag']!r} but the census says "
                f"self_consistent={row['self_consistent']}, "
                f"obs_vanilla={row['obs_vanilla']}, obs_ours={row['obs_ours']}, "
                f"arm={row['arm']}"
                + (f", vanilla_sites={row['vanilla_sites']}" if "vanilla_sites" in row else "")
                + f" — {why}"
            )
        # R3
        if r["class"] == "Graded" and row.get("projected", 0) <= 0:
            problems.append(
                f"{r['kind']}: opened (Class::Graded) and reached no projection over "
                f"{census.get('scenarios', '?')} scenarios. A family nothing compares "
                "reads as coverage and is not"
            )
    for kind in ev:
        if kind not in {r["kind"] for r in rows}:
            problems.append(
                f"{kind}: measured in the census and classified nowhere in trace.rs"
            )
    if problems:
        raise Refused("; ".join(problems))
    return True


def cmd_check(argv):
    rows = families()
    scan_sources()
    census, ev = read_evidence()
    check_table(rows, census, ev)
    graded = [r["kind"] for r in rows if r["class"] == "Graded"]
    ambient = [r["kind"] for r in rows if r["class"] == "Ambient"]
    print(
        f"AMBIENT GREEN — {len(rows)} families declared, {len(graded)} graded, "
        f"{len(ambient)} ambient, every row measured over "
        f"{census.get('scenarios', '?')} scenarios at {census.get('commit', '?')[:9]}"
    )
    if graded:
        print(f"  graded:  {', '.join(sorted(graded))}")
    print(f"  ambient: {', '.join(sorted(ambient))}")
    return 0


# --------------------------------------------------------------------------
# --sweep — the measurement
# --------------------------------------------------------------------------


def obs_of(doc):
    """{scenario: [obs]} for a recording."""
    return {s["name"]: s.get("obs") or [] for s in doc.get("scenarios") or []}


def signature(obs, kind, stepped=False):
    """What "vanilla is self-consistent about this family" is measured on.

    The **ordered** sequence of the family's own decoded fields within a
    scenario, `rest` included. Order is part of it deliberately: a set
    comparison would call two runs consistent while they disagreed about which
    chunk was forgotten first, and the whole reason the chunk-order golden
    exists is that order is the content in this area.

    # TWO SIGNATURES, AND THE SECOND ONE IS THE ONE A CHECK LIVES OR DIES BY

    `at_ms` is wall clock and no scenario compares it by default, so it is out
    of both. **`step` is not so easy.** It is the index of the last measured
    step issued before the observation arrived, and `compare::project` puts it
    into the compared value — so a family whose *values* reproduce perfectly
    while landing after a different step in each run would pass a
    fields-only test and flap in a real check.

    That is not hypothetical for this classification: every one of the
    chunk-view families is queued by `PlayerChunkSender.sendNextChunks` on a
    server tick, and which of the actor's steps has gone out by then is a race
    between the harness's writes and the 20 Hz clock.

    So both are measured. `self_consistent` is fields alone and says whether
    vanilla computes the same answer; `self_consistent_stepped` adds `step`
    and says whether a check could read it. **Only the stepped one may open a
    family** — `TAG_RULES["graded"]` reads it — and the unstepped one is kept
    beside it because the gap between them is the interesting number: it
    separates "vanilla is nondeterministic here" from "vanilla is fine and our
    harness cannot pin when it arrives", which have opposite fixes.
    """
    return [
        (o["step"], o["fields"]) if stepped else o["fields"]
        for o in obs
        if o["kind"] == kind
    ]


def cmd_sweep(argv):
    ap = argparse.ArgumentParser(prog="ambient-falsify.py --sweep")
    ap.add_argument("--sweep", action="store_true")
    ap.add_argument("--vanilla-a", required=True)
    ap.add_argument("--vanilla-b", required=True)
    ap.add_argument("--ours", required=True)
    ap.add_argument("--scenarios", required=True)
    ap.add_argument(
        "--emitters",
        required=True,
        help="ci/packet-emitter-sites.py --json output. Required, not optional: "
        "the `no-emitter` tag is a claim about the jar and a census cannot "
        "make it.",
    )
    ap.add_argument("--out", default=EVIDENCE)
    a = ap.parse_args(argv)

    emitters = json.load(open(a.emitters))
    va = json.load(open(a.vanilla_a))
    vb = json.load(open(a.vanilla_b))
    ou = json.load(open(a.ours))
    A, B, O = obs_of(va), obs_of(vb), obs_of(ou)
    shared = sorted(set(A) & set(B) & set(O))
    if not shared:
        die("the three recordings share no scenario — they were not taken over one set")

    rows = families()
    # **A census with no tree behind it is a measurement nobody can reproduce.**
    #
    # This ran `git rev-parse HEAD` in `ROOT` and wrote `commit = ""` on the
    # first real census, because a census runs in the RSYNCED remote directory
    # and `ci/t0-remote.sh` excludes `.git` — so the one place this is ever
    # taken is the one place git cannot answer. `RUSTMC_CENSUS_COMMIT` is what
    # the caller passes; the git probe is the fallback for a run taken in a
    # real checkout; and an empty answer is a REFUSAL rather than an empty
    # string in the file, because ledger rule 18 exists for exactly the shape
    # where a provenance field is present and says nothing.
    commit = os.environ.get("RUSTMC_CENSUS_COMMIT", "").strip()
    if not commit:
        commit = subprocess.run(
            ["git", "-C", ROOT, "rev-parse", "HEAD"], capture_output=True, text=True
        ).stdout.strip()
    if not commit:
        die(
            "no commit for this census: `git rev-parse HEAD` failed in "
            f"{ROOT} (a census runs in the rsynced remote dir, which has no "
            "`.git`) and RUSTMC_CENSUS_COMMIT is unset. Pass it — "
            "`ci/ambient-census.sh` does — rather than writing a provenance "
            "field that says nothing."
        )

    # Which kinds any scenario names in `observe` — the only way an ambient
    # family reaches a projection today, and what `projected` counts for a
    # graded one.
    #
    # **`projected` is counted per naming scenario, not per corpus.** It was
    # `obs_vanilla if kind in named else 0`: any scenario file naming the kind
    # opened the WHOLE census's count, including observations from scenarios
    # that never name it and a naming scenario the recordings do not contain.
    # A graded row could then pass R3 on a check that reads nothing — a
    # scenario naming the kind whose own window vanilla sends none of it in.
    # Found 2026-09-24 while adding `548`, the first scenario written to
    # satisfy R3 rather than to precede it. Now only observations inside a
    # scenario that names the kind, in the censused set, count.
    named = set()
    named_in = {}
    for fn in sorted(os.listdir(a.scenarios)):
        if not fn.endswith(".toml"):
            continue
        src = open(os.path.join(a.scenarios, fn)).read()
        m = re.search(r"^observe\s*=\s*\[(.*?)\]", src, re.M | re.S)
        n = re.search(r'^name\s*=\s*"([^"]+)"', src, re.M)
        if m:
            kinds = set(re.findall(r'"([a-z_0-9:]+)"', m.group(1)))
            named |= kinds
            if n:
                named_in.setdefault(n.group(1), set()).update(kinds)

    out = []
    for r in rows:
        kind = r["kind"]
        oa = sum(len(signature(A[s], kind)) for s in shared)
        ob = sum(len(signature(B[s], kind)) for s in shared)
        oo = sum(len(signature(O[s], kind)) for s in shared)
        touched_v = sum(1 for s in shared if signature(A[s], kind) or signature(B[s], kind))
        touched_o = sum(1 for s in shared if signature(O[s], kind))
        agree = [s for s in shared if signature(A[s], kind) == signature(B[s], kind)]
        agree_st = [
            s
            for s in shared
            if signature(A[s], kind, True) == signature(B[s], kind, True)
        ]
        # **A family neither run emitted is not "self-consistent".** Every
        # empty sequence equals every other one, so a silent family would
        # score a perfect 1.0 and read as the most stable thing in the corpus,
        # which is the flattering direction and the exact shape of an
        # instrument that has stopped looking. It is reported as `unmeasured`.
        emitted = oa + ob > 0
        out.append(
            {
                "kind": kind,
                "id": r["id_name"],
                "obs_vanilla": oa + ob,
                "obs_vanilla_a": oa,
                "obs_vanilla_b": ob,
                "obs_ours": oo,
                "scenarios_vanilla": touched_v,
                "scenarios_ours": touched_o,
                "agreeing_scenarios": len(agree),
                "agreeing_scenarios_stepped": len(agree_st),
                "self_consistent": bool(emitted and len(agree) == len(shared)),
                "self_consistent_stepped": bool(
                    emitted and len(agree_st) == len(shared)
                ),
                "measured": emitted,
                "emitter_sites": int(emitters.get(kind, {}).get("total", -1)),
                "projected": sum(
                    len(signature(A[s], kind)) + len(signature(B[s], kind))
                    for s in shared
                    if kind in named_in.get(s, ())
                ),
                "named_by_a_scenario": kind in named,
                # Every `<scenario>@<step>` vanilla emitted the family at, for
                # a family emitted at most twice — the only ones a site list
                # can say anything about, and all `rare-stray` needs. Empty
                # for a silent family; omitted above two, so a busy family
                # cannot be read as having a short site list.
                **(
                    {
                        "vanilla_sites": sorted(
                            f"{sc}@{o['step']}"
                            for sc in shared
                            for doc in (A, B)
                            for o in doc[sc]
                            if o["kind"] == kind
                        )
                    }
                    if oa + ob <= 2
                    else {}
                ),
            }
        )

    with open(a.out, "w") as fh:
        fh.write(
            "# ci/checks/ambient-families.toml — the measurement behind every\n"
            "# `trace::FAMILIES` row. WRITTEN BY `ci/ambient-census.sh`; do not\n"
            "# hand-edit. `ci/ambient-falsify.py --check` reads it in T0.\n"
            "#\n"
            "# `self_consistent` is TWO INDEPENDENT VANILLA RUNS agreeing about\n"
            "# the ordered sequence of this family's decoded fields, scenario by\n"
            "# scenario, over the whole censused set. A family neither run\n"
            "# emitted scores `self_consistent = false` and `measured = false`,\n"
            "# never a vacuous true: every empty sequence equals every other one,\n"
            "# so silence would otherwise read as the most stable family here.\n"
            "#\n"
            "# `self_consistent_stepped` adds the STEP each observation landed\n"
            "# after, which `compare::project` puts into the compared value. It\n"
            "# is the one a check lives or dies by and the one `graded` requires;\n"
            "# the gap between the two separates `vanilla is nondeterministic`\n"
            "# from `vanilla is fine and the harness cannot pin the arrival`.\n"
            "#\n"
            "# `emitter_sites` is `new` instructions for the packet's class\n"
            "# anywhere under net/ in the pinned jar, from\n"
            "# `ci/packet-emitter-sites.py`. -1 means the scan did not cover the\n"
            "# family (the two synthesised kinds have no class of their own).\n"
            "#\n"
            "# `projected` counts vanilla observations inside the censused\n"
            "# scenarios that name the kind in their own `observe` list — only\n"
            "# those reach a projection. A `Class::Graded` row with\n"
            "# `projected = 0` is refused.\n\n"
        )
        fh.write("[census]\n")
        fh.write(f'commit = "{commit}"\n')
        fh.write(f"scenarios = {len(shared)}\n")
        fh.write(f'vanilla_a = "{os.path.basename(a.vanilla_a)}"\n')
        fh.write(f'vanilla_b = "{os.path.basename(a.vanilla_b)}"\n')
        fh.write(f'recorder = "{va.get("recorder", "?")}"\n')
        for r in out:
            fh.write("\n[[family]]\n")
            for k, v in r.items():
                if isinstance(v, bool):
                    fh.write(f"{k} = {str(v).lower()}\n")
                elif isinstance(v, list):
                    fh.write(f"{k} = [{', '.join(json.dumps(x) for x in v)}]\n")
                elif isinstance(v, int):
                    fh.write(f"{k} = {v}\n")
                elif v is None:
                    fh.write(f'{k} = ""\n')
                else:
                    fh.write(f'{k} = "{v}"\n')

    print(f"swept {len(shared)} scenario(s) -> {os.path.relpath(a.out, ROOT)}")
    w = max(len(r["kind"]) for r in out)
    print(
        f"  {'family'.ljust(w)}  van_a  van_b   ours  scn_v  scn_o  emit  "
        f"agree  agree+step  self-consistent"
    )
    for r in out:
        if not r["measured"]:
            sc = "unmeasured"
        elif r["self_consistent_stepped"]:
            sc = "yes (values and step)"
        elif r["self_consistent"]:
            sc = "values only — STEP FLAPS"
        else:
            sc = "no"
        print(
            f"  {r['kind'].ljust(w)}  {r['obs_vanilla_a']:5d}  {r['obs_vanilla_b']:5d}  "
            f"{r['obs_ours']:5d}  {r['scenarios_vanilla']:5d}  "
            f"{r['scenarios_ours']:5d}  {r['emitter_sites']:4d}  "
            f"{r['agreeing_scenarios']:5d}  {r['agreeing_scenarios_stepped']:10d}  {sc}"
        )
    return 0


# --------------------------------------------------------------------------
# --falsify — red-when-broken evidence, and the shipped-blindness half
# --------------------------------------------------------------------------


def _shipped_sources():
    """`main`'s trace.rs and compare.rs, for the was-it-green half."""
    out = {}
    for name, path in (
        ("trace", "tools/oracle-runner/src/trace.rs"),
        ("compare", "tools/oracle-runner/src/compare.rs"),
    ):
        r = subprocess.run(
            ["git", "-C", ROOT, "show", f"origin/main:{path}"],
            capture_output=True,
            text=True,
        )
        if r.returncode != 0:
            return None
        out[name] = r.stdout
    return out


def _shipped_blind(name, shipped, mutate):
    """Did `main`'s own instruments notice this injection? They did not.

    **Only two of the four injections can be asked this way, and saying so is
    the point.** `main` has no `class` and no `tag`, so "a family dropped for a
    different reason than declared" and "an opened family nothing compares"
    are not defects it could commit — there is nothing there to be wrong. Those
    two are NEW coverage, not a repair of a missed catch, and reporting them as
    "main was blind" would be true in the useless sense and misleading in every
    other. `INJECTIONS` marks them `inexpressible` and this function is not
    called for them.

    The bar `ci/unfalsifiable-falsify.py` sets is that a guard be shown
    catching a **real past instance**. So for each injection this applies the
    same edit to `main`'s sources and runs the instrument that existed there —
    `ci/corpus-surface.py`'s `trace_sets`, which is the only thing in the tree
    that ever read the classification — and asserts it comes back clean. A
    guard that cannot be shown green-before is a guard nobody can size.
    """
    if shipped is None:
        return "no origin/main to compare against"
    src = mutate(dict(shipped))
    sys.path.insert(0, os.path.join(ROOT, "ci"))
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "cs_main", os.path.join(ROOT, "ci", "corpus-surface.py")
    )
    mod = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(mod)
    except SystemExit:
        return "corpus-surface.py could not be loaded"
    # `main`'s reader looked for `fn is_housekeeping`; run it over the injected
    # `main` source and see whether anything at all complains.
    try:
        hk = re.search(r"fn is_housekeeping\(.*?\n\}", src["trace"], re.S)
        if not hk:
            return "unexpectedly refused: no is_housekeeping"
        keep = set(re.findall(r"cb::([A-Z_0-9]+)", hk.group(0)))
        return None if len(keep) >= 8 else "unexpectedly refused: too few ids"
    except SystemExit:
        return "unexpectedly refused"


# Each injection: a name, an edit to the CURRENT sources, the rule it must
# break, and the same edit expressed against `main`'s sources for the
# was-it-green half.
def _inject_undeclared_record_drop(src):
    src["trace"] = src["trace"].replace(
        "    if let Some((kind, mut f, consumed)) = decode(norm, states, items, id, body) {",
        "    if id == ids::play::cb::SOUND {\n        return None;\n    }\n"
        "    if let Some((kind, mut f, consumed)) = decode(norm, states, items, id, body) {",
        1,
    )
    return src


def _inject_undeclared_projection_drop(src):
    src["compare"] = src["compare"].replace(
        "        .filter(|(o, _)| !crate::trace::is_ambient_kind(&o.kind) || observed.contains(&o.kind))",
        "        .filter(|(o, _)| !crate::trace::is_ambient_kind(&o.kind) || observed.contains(&o.kind))\n"
        '        .filter(|(o, _)| o.kind != "sound")',
        1,
    )
    return src


def _inject_wrong_reason(src):
    # `keep_alive` is tagged `timer`, and the census measures it emitted. Claim
    # instead that vanilla has no emitter for it — the row still reads as a
    # classification, and the reason is now false.
    src["trace"] = re.sub(
        r'(kind: "keep_alive",\n(?:.*\n)*?\s*tag: )"timer"',
        r'\1"no-emitter"',
        src["trace"],
        count=1,
    )
    return src


def _inject_unread_open(src):
    # Open `tab_list` — which vanilla never emits, so nothing can compare it.
    src["trace"] = re.sub(
        r'(kind: "tab_list",\n\s*class: )Class::Ambient(,\n\s*tag: )"no-emitter"',
        '\\1Class::Graded\\2"graded"',
        src["trace"],
        count=1,
    )
    return src


def _shipped_record_drop(src):
    src["trace"] = src["trace"].replace(
        "            || x == cb::UPDATE_ADVANCEMENTS", "            || x == cb::SOUND\n            || x == cb::UPDATE_ADVANCEMENTS", 1
    )
    return src


# name, injection into THIS tree, and how the same defect looks on `main` —
# either the analogous edit (asserted accepted there) or `None`, meaning the
# defect is inexpressible on main because the concept it corrupts did not
# exist.
INJECTIONS = [
    (
        "a family is silently dropped without a declaration (record time)",
        _inject_undeclared_record_drop,
        _shipped_record_drop,
    ),
    (
        "a family is silently dropped without a declaration (projection time)",
        _inject_undeclared_projection_drop,
        _shipped_record_drop,
    ),
    (
        "a declared family is dropped for a different reason than declared",
        _inject_wrong_reason,
        None,
    ),
    (
        "an opened family is not actually compared",
        _inject_unread_open,
        None,
    ),
]


# **Evidence injections: the census contradicting a `rare-stray` row.** The
# source injections above cannot reach this tag, because what makes it wrong is
# a measurement, not an edit. Each case rewrites the tagged row's census the
# way a provoked family would read, and the gate must refuse it; the last is
# the positive half, one stray at the pinned site, which must pass, so the
# tag is shown able to go green as well as red.
def _stray_row(ev, **kw):
    ev = {k: dict(v) for k, v in ev.items()}
    kind = next(k for k in RARE_STRAY_SITES if k in ev)
    ev[kind].update(kw)
    ev[kind]["obs_vanilla"] = ev[kind]["obs_vanilla_a"] + ev[kind]["obs_vanilla_b"]
    ev[kind]["measured"] = ev[kind]["obs_vanilla"] > 0
    return ev


def _pinned_site():
    return sorted(next(iter(RARE_STRAY_SITES.values())))[0]


EVIDENCE_INJECTIONS = [
    (
        "a rare stray appears in BOTH vanilla runs",
        lambda ev: _stray_row(
            ev, obs_vanilla_a=1, obs_vanilla_b=1, vanilla_sites=[_pinned_site()] * 2
        ),
        True,
    ),
    (
        "a rare stray appears TWICE in one run",
        lambda ev: _stray_row(
            ev, obs_vanilla_a=2, obs_vanilla_b=0, vanilla_sites=[_pinned_site()] * 2
        ),
        True,
    ),
    (
        "a rare stray appears at a site never seen",
        lambda ev: _stray_row(
            ev, obs_vanilla_a=1, obs_vanilla_b=0, vanilla_sites=["attacking-another-player@1"]
        ),
        True,
    ),
    (
        "a census with no site list for a rare stray",
        lambda ev: _stray_row(ev, obs_vanilla_a=0, obs_vanilla_b=0, vanilla_sites=None),
        True,
    ),
    (
        "one stray at the pinned site (must PASS)",
        lambda ev: _stray_row(
            ev, obs_vanilla_a=0, obs_vanilla_b=1, vanilla_sites=[_pinned_site()]
        ),
        False,
    ),
]


def _run_check(src, census, ev):
    """The whole gate over injected sources. Returns None if it passed."""
    try:
        scan_sources(src["trace"], src["compare"])
        check_table(families(src["trace"]), census, ev)
    except Refused as e:
        return str(e)
    return None


def cmd_falsify(argv):
    base = {"trace": open(TRACE).read(), "compare": open(COMPARE).read()}
    census, ev = read_evidence()
    shipped = _shipped_sources()

    clean = _run_check(dict(base), census, ev)
    if clean is not None:
        die(f"the SHIPPED tree does not pass its own gate: {clean}")
    print("  control: the shipped tree passes")

    bad = 0
    for name, mutate, shipped_mutate in INJECTIONS:
        why = _run_check(mutate(dict(base)), census, ev)
        if why is None:
            print(f"  MISSED   {name}")
            bad += 1
            continue
        if shipped_mutate is None:
            note = "inexpressible on main — it has no class and no tag"
        else:
            blind = _shipped_blind(name, shipped, shipped_mutate)
            note = "main accepted the same edit" if blind is None else f"main: {blind}"
        print(f"  refused  {name}  [{note}]")
    for name, mutate_ev, want_red in EVIDENCE_INJECTIONS:
        ev2 = mutate_ev(ev)
        for k in list(ev2):
            if ev2[k].get("vanilla_sites", 0) is None:
                del ev2[k]["vanilla_sites"]
        why = _run_check(dict(base), census, ev2)
        if want_red and why is None:
            print(f"  MISSED   {name}")
            bad += 1
        elif not want_red and why is not None:
            print(f"  WRONGLY REFUSED  {name}: {why[:200]}")
            bad += 1
        else:
            print(f"  {'refused ' if want_red else 'accepted'} {name}")
    if bad:
        die(f"{bad} of {len(INJECTIONS) + len(EVIDENCE_INJECTIONS)} injections were not caught")
    expressible = sum(1 for _, _, sm in INJECTIONS if sm is not None)
    print(
        f"AMBIENT-FALSIFY GREEN — {len(INJECTIONS)} source injections, every one refused, "
        f"and {len(EVIDENCE_INJECTIONS)} census injections on `rare-stray`, each judged as it must be; "
        f"{expressible} of them are edits `main` accepts silently and "
        f"{len(INJECTIONS) - expressible} are defects main could not commit at all"
    )
    return 0


def main(argv):
    if "--sweep" in argv:
        return cmd_sweep(argv)
    if "--falsify" in argv:
        return cmd_falsify(argv)
    try:
        return cmd_check(argv)
    except Refused as e:
        die(str(e))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
