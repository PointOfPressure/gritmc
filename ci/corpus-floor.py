#!/usr/bin/env python3
"""The corpus floor — how much of this corpus vanilla cannot reproduce, at what N.

    ci/corpus-floor.py measure    <ensemble.json> --out <floor.json>
    ci/corpus-floor.py adjudicate <floor.json> [--declared ci/checks/gameplay-unstable.toml]
    ci/corpus-floor.py emit       <floor.json> [--only <name>]

`ci/corpus-floor.sh` builds the ensemble and calls `measure` and `adjudicate`;
this file is separate for the reason `ci/selfcheck-aggregate.py` is — a number
in a tracked document has to come from a tracked script, and a saved
measurement has to be re-adjudicable years later without re-running servers.

# WHY THIS EXISTS

ORACLE rule 6 says a residual is a defect only if two independently generated
vanilla references agree on it. The worldgen differ has had that mechanised
for months as `--twin`, and every worldgen figure in `docs/PARITY.md` is
quoted beside the **floor** it could not have resolved.

The gameplay corpus had no such thing. It had `ci/checks/gameplay-unstable.toml`,
which is the right *idea* — a declared list of checks the reference fails
against itself — populated ad hoc by whichever agent last tripped over an
entry. In one night three separate agents each spent part of a run
rediscovering the same fact from recordings that happened to be lying around:
`530` is unmatchable because `Holder$Reference` has no `hashCode`; `050` is
bimodal because the RNG decides whether the pickup happens at all; and `040`'s
step 2/3 boundary sits at 381 ms **inside vanilla's own 322-383 ms spread**.

Two of those are facts about vanilla. One is a bug in this harness. They read
identically in a report and they have opposite fixes.

# THE FOUR THINGS THIS ANSWERS

## 1. How many runs, and the number is not the point

`p_hide(N) = 1 - alpha**(1/N)` is the largest minority probability that can
still produce N identical runs more often than `alpha`. At alpha = 0.05:

    N =  2   p_hide = 77.6%
    N =  3   p_hide = 63.2%
    N =  6   p_hide = 39.3%
    N = 12   p_hide = 22.1%
    N = 26   p_hide = 10.9%
    N = 77   p_hide =  3.8%

The corpus's own smallest measured rate is `chest-opens-a-screen` at 1 in 26.
Resolving *that* needs **N = 77** — `(1 - 1/26)^76` is 0.0508, just outside
alpha, so the honest figure is the next one up — which is about fifteen hours
of vanilla boots. **So stability is not declarable at any N this project can
afford, and a protocol that pretended otherwise would be the defect it exists
to prevent.**

What is affordable is a floor with its N attached and its resolving power
printed beside it, which is exactly the discipline `docs/PARITY.md` §4 already
imposes on every worldgen number. `docs/PARITY.md` records the trap in its
sharpest form: for the End, **two references agreeing gave a floor of 0 and
six gave 252**. Two samples agreeing is not the same as there being nothing to
disagree about. So this reports:

  * `floor` — scenarios measured not to reproduce. A **lower bound**, always.
  * `p_hide` — what this N could not have seen.

and refuses to print one without the other.

## 2. The corpus analogue of the twin-mask floor

A worldgen residual is quoted against the number of positions two vanilla runs
spell differently. A corpus score is quoted against the number of **scenarios**
two vanilla runs answer differently. `declared 62/88` with a floor of 9 means
at most 79 were reachable, and the 26 that did not match include 9 that could
not have.

`ci/t2-gameplay.sh` prints the floor beside the score, out of
`ci/checks/gameplay-floor.toml`. When that file is missing or unparseable it
prints `floor UNKNOWN` and `the score above is NOT QUOTABLE without one` and
carries on, which is deliberate rather than soft: that gate's job is to measure
this server against vanilla, and refusing to report it because a *separate*
standing measurement is absent would hand a stranger cloning the tree a corpus
gate that cannot run. What the warning must not do is let a bare score look
complete, and it says so in the same words either way. Staleness is caught
where it can be caught properly — `adjudicate` re-derives every field of that
file from a live measurement and reds on any disagreement.

**The floor is reported beside the score and never subtracted from it.** A
`[[unstable]]` entry cannot change a corpus verdict, cannot move the baseline
ratchet, and cannot make a scenario stop failing. That was already true and
`ci/corpus-floor-falsify.sh` case 11 now asserts it.

## 3. Entries carry their evidence, and it is re-derivable

The evidence is **generated and lives apart from the claim**, which is the
arrangement that makes it hard to forge. `ci/checks/gameplay-floor.toml` is
written by `update` and never by hand: the ensemble, the sha256 and mtime of
every recording in it, and per floor scenario the class, the rate, the number
of **distinct** projections and the first difference.
`ci/checks/gameplay-unstable.toml` holds only what a measurement cannot supply
— the class, the rate, and the mechanism read out of the pinned jar.
`adjudicate` re-derives every field of the first from a live measurement and
cross-checks the second against it, so hand-editing either is caught the next
time anyone runs `ci/corpus-floor.sh`.

Three clauses do the work, and the first is the cheapest:

* **An entry must show at least two distinct projections.** A stable scenario
  produces exactly one across every run, so no hand-written `flaps = 15`
  survives contact with the measurement. Falsifier cases 4 and 5.
* **An entry must carry a mechanism, not just arithmetic.** `class`,
  `measured`, `why` and `field`/`repair` must be present and must not still be
  a placeholder. `emit` fills in every number and prints those as `TODO` on
  purpose, because a cause is read out of bytecode and no measurement can
  supply one. Cases 14 and 15.
* **A rate must not contradict its own named bytes.** When an entry's
  `evidence.recordings` are exactly the ensemble in front of us, its
  `distinct_projections` is re-derived and a mismatch is a red.

## 4. Ours against vanilla's, mechanically

A projected observation carries `who`, `step`, `kind`, `fields`, `when`. Of
those `step` and `when` are **ours** — which of our actions was in flight, and
which bucket of our own timing rule. So every measurement runs twice, once
under `Lens::Declared` and once under `Lens::BoundaryBlind` with those two
blanked, and the classes are:

  * `reference`  — differs under both lenses. Vanilla said different things.
                   Absorbable into `[[unstable]]`, with its cause.
  * `boundary`   — differs under `declared` only. Vanilla said the same things
                   and we filed them under different steps. **Ours.**
  * `silent`     — every run that produced anything produced the
                   byte-identical thing, and the rest produced nothing at all.
                   Nothing vanilla said varied; whether we recorded it did.
                   **Ours**, and it is the class `compare` calls `vacuous`.
  * `mixed`      — differs under both, but less under blind. Part of it is
                   ours. Filed as vanilla's, because the vanilla half is real
                   whatever we do about our half.

`boundary` and `silent` go in `[[harness]]`, which is a list of repair orders,
never in `[[unstable]]`, which is a list of facts about vanilla. A name in both
is refused, and a scenario the measurement calls ours while the file calls it
vanilla's is a red — in both directions. That is the whole of the answer to
"these have opposite fixes".

**The lens is sound in one direction only, and that is stated rather than
glossed.** A `boundary` verdict is proof the disagreement is ours: the two runs
are byte-identical once our own two fields are removed. A `reference` verdict
is *not* proof the disagreement is vanilla's — it only proves it is not the
boundary class. `silent` exists because the first scenario this protocol was
pointed at, `540`, was exactly such a case: `reference` by the lens, ours by
the mechanism. An entry's `field` and `why` are still a human reading the
bytecode, and they are what makes the claim, not the class.

# THE HETEROGENEITY CONTROL, WHICH IS NOT OPTIONAL

Recordings taken by different trees were taken by different **recorders**. A
disagreement between two of them is not necessarily vanilla disagreeing with
itself. S4 handled this by hand on 2026-08-08 — it quoted its N = 3 finding
alongside "the N = 2 run over the two recordings whose `trace.rs` is
byte-identical" — and that is mechanised here: the ensemble carries each run's
recorder digest, and every measurement reports the **homogeneous subset**
beside the full one. A flap present in the full ensemble and absent from the
largest homogeneous subset is labelled `recorder-suspect` and is not
promotable.

# THE POWER EXPONENT IS THE RUN COUNT, NEVER THE PAIR COUNT

This measures all pairs — `N(N-1)/2` of them — because "two independent
vanilla runs agree" is literally what rule 6 says and all-pairs is its
symmetric estimator, with no privileged reference run. But 15 pairs from 6
runs are **not** 15 independent samples, and feeding 15 into `(1-p)**C` would
make every absence look resolved. The exponent is `runs - 1`, which is what
`ci/selfcheck-aggregate.py` has always used (25 comparisons at N = 26), so the
rates already declared there remain valid arithmetic under this file.
"""

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

# The vocabulary rule, imported rather than restated. `ci/corpus-floor-report.py`
# imports the same module, and that is the point: the ensemble's vocabulary and
# the gate run's are two readings of one quantity only if one function takes
# both. `sys.path` is prepended because `ci/` is not a package and this file is
# run by path from four scripts and from `ci/t0.sh`.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_vocabulary as tv  # noqa: E402 — must follow the sys.path line

# The miss probability above which an absence says nothing. Same value and
# same reasoning as `ci/selfcheck-aggregate.py`; they must agree, because the
# two files adjudicate the same declared list.
ALPHA = 0.05

# Classes a measured scenario can land in. `unmeasured` is not a class of
# instability; it is the absence of evidence, reported and not interpreted.
REFERENCE, BOUNDARY, SILENT, MIXED, STABLE, DEAD, UNMEASURED = (
    "reference",
    "boundary",
    "silent",
    "mixed",
    "stable",
    "dead",
    "unmeasured",
)

# The two classes that are OURS. Both belong in `[[harness]]`, which is a list
# of repair orders; neither may enter `[[unstable]]`, which is a list of facts
# about vanilla.
OURS = (BOUNDARY, SILENT)

# One server tick. A step boundary with less than a tick of observed slack in
# front of it is not a boundary, it is a coin whose bias nobody has measured:
# vanilla does its work on the tick and any two events inside one tick can
# arrive in either order and either step.
#
# **This is the instrument that would have caught `040` before it cost a run.**
# On main's own harness, over the six recordings of 2026-08-09, the
# `recipe_book_add` for the furnace arrives at 333/335/345/381/531/538 ms and
# the first step-3 block update at 416/463/466/470/784/791 — so the tightest of
# six runs leaves **35 ms**, less than one tick, between the last observation of
# step 2 and the first of step 3. `040` has never flapped. It is one shortened
# window away from flapping, and on the `chat-commands` branch a 60 ms
# `Step::Hold` shortened it: the boundary moved to 381 ms, inside vanilla's own
# 322-383 ms spread, and read as a branch regression.
#
# A flap-only instrument cannot say that, because there is no flap. This can,
# from recordings that already exist, and the declared knife-edge set in
# `ci/checks/gameplay-floor.toml` makes it standing rather than rediscovered.
TICK_MS = 50


def p_hide(runs: int, alpha: float = ALPHA) -> float:
    """Largest minority rate that survives `runs` identical runs at `alpha`."""
    if runs < 2:
        return 1.0
    return 1.0 - alpha ** (1.0 / runs)


def sig(projection) -> str:
    return hashlib.sha256(
        json.dumps(projection, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def render(obs) -> str:
    return json.dumps(obs, sort_keys=True, separators=(",", ":"))


def first_difference(a: list, b: list) -> str:
    """Where two projections part company, in the words `compare` uses."""
    for i in range(max(len(a), len(b))):
        x = a[i] if i < len(a) else None
        y = b[i] if i < len(b) else None
        if x == y:
            continue
        if x is None:
            return f"at {i}: one run said nothing more, the other {render(y)}"
        if y is None:
            return f"at {i}: one run {render(x)}, the other said nothing more"
        return f"at {i}: one run {render(x)}, the other {render(y)}"
    return "the two projections are equal"


def step_moves(declared_a, declared_b, times_a, times_b):
    """Observations that moved between steps, with the arrival times either side.

    Only meaningful when the two runs agree under the boundary-blind lens, in
    which case the sequences line up element for element and the only field
    that can differ is `step` or `when`. That is the `Step::Hold` class, and
    the arrival times are the repair order: "this packet lands at 322-383 ms
    across the ensemble and our boundary is inside that" is actionable, where
    "step 2 in one run and step 3 in another" is only a symptom.
    """
    out = []
    for i, (x, y) in enumerate(zip(declared_a, declared_b)):
        if x == y:
            continue
        moved = {k: (x.get(k), y.get(k)) for k in ("step", "when") if x.get(k) != y.get(k)}
        if not moved:
            continue
        ta = times_a[i] if i < len(times_a) else None
        tb = times_b[i] if i < len(times_b) else None
        out.append(
            {
                "index": i,
                "who": x.get("who"),
                "kind": x.get("kind"),
                "moved": {k: list(v) for k, v in moved.items()},
                "at_ms": [ta, tb],
            }
        )
    return out


def boundary_margins(projection, times):
    """Observed slack at each step boundary, within one stream, in one run.

    A boundary is two consecutive projected observations on the same `who`
    whose `step` differs. The slack is the gap between their arrival times: how
    much later the first observation of the next step could have arrived, or
    how much earlier our own action for it could have been sent, before the two
    changed places.

    Returns `[(gap_ms, from_step, to_step, kind_before, kind_after), ...]`.
    Empty when the times could not be aligned — see
    `collapse_repeats_with_times`, which clears them rather than guess.
    """
    out = []
    if len(times) != len(projection):
        return out
    for i in range(len(projection) - 1):
        a, b = projection[i], projection[i + 1]
        if a.get("who") != b.get("who"):
            continue
        if a.get("step") == b.get("step") or a.get("step") is None or b.get("step") is None:
            continue
        gap = times[i + 1] - times[i]
        if gap < 0:
            continue
        out.append((gap, a["step"], b["step"], a.get("kind"), b.get("kind")))
    return out


def load_projection(path: str, lens: str) -> dict:
    doc = json.load(open(path))
    if doc.get("lens") != lens:
        raise SystemExit(
            f"corpus-floor: {path} was projected under lens {doc.get('lens')!r}, "
            f"expected {lens!r} — the label travels with the number and this pair "
            f"does not agree"
        )
    return {s["name"]: s for s in doc["scenarios"]}


def measure_group(names, runs, declared_by_run, blind_by_run, times_by_run):
    """Per-scenario measurement over one set of runs. See the header."""
    out = {}
    for name in names:
        usable = [
            r
            for r in runs
            if declared_by_run[r].get(name)
            and declared_by_run[r][name]["present"]
            and not declared_by_run[r][name].get("error")
        ]
        n = len(usable)
        rec = {
            "name": name,
            "runs": n,
            "run_ids": usable,
            "pairs": n * (n - 1) // 2,
            "flaps": 0,
            "blind_flaps": 0,
            "distinct": 0,
            "blind_distinct": 0,
            "class": UNMEASURED,
            "first_difference": None,
            "step_moves": [],
            "silent_runs": [],
        }
        # The margin is a per-run measurement and does not need a pair, so it
        # is taken before the `n < 2` return: a single recording still says how
        # close that run came, and refusing to report it would be the instrument
        # withholding the one thing it could still answer.
        tight = None
        for r in usable:
            for gap, s0, s1, k0, k1 in boundary_margins(
                declared_by_run[r][name]["declared"], declared_by_run[r][name]["at_ms"]
            ):
                if tight is None or gap < tight["margin_ms"]:
                    tight = {
                        "margin_ms": gap,
                        "run": r,
                        "between_steps": [s0, s1],
                        "kinds": [k0, k1],
                    }
        rec["tightest_boundary"] = tight
        rec["knife_edge"] = bool(tight and tight["margin_ms"] < TICK_MS)
        if n < 2:
            out[name] = rec
            continue
        dproj = {r: declared_by_run[r][name]["declared"] for r in usable}
        bproj = {r: blind_by_run[r][name]["declared"] for r in usable}
        dsig = {r: sig(dproj[r]) for r in usable}
        bsig = {r: sig(bproj[r]) for r in usable}
        rec["distinct"] = len(set(dsig.values()))
        rec["blind_distinct"] = len(set(bsig.values()))
        worst = None
        for i in range(n):
            for j in range(i + 1, n):
                a, b = usable[i], usable[j]
                if dsig[a] != dsig[b]:
                    rec["flaps"] += 1
                    if worst is None:
                        worst = (a, b)
                if bsig[a] != bsig[b]:
                    rec["blind_flaps"] += 1
        if rec["flaps"] == 0:
            # A projection that is empty in every run is not a reproducible
            # check, it is a dead one. `ci/selfcheck-aggregate.py` calls this
            # `vacuous` and counts it as unstable; its own header records that
            # as costing the strict score a fixed 1. The reference agreed with
            # itself perfectly here, so the honest report is "dead", and it is
            # reported rather than absorbed.
            rec["class"] = DEAD if all(len(dproj[r]) == 0 for r in usable) else STABLE
            out[name] = rec
            continue
        a, b = worst
        rec["first_difference"] = first_difference(dproj[a], dproj[b])
        rec["silent_runs"] = sorted(r for r in usable if len(dproj[r]) == 0)
        if rec["blind_flaps"] == 0:
            rec["class"] = BOUNDARY
            rec["step_moves"] = step_moves(
                dproj[a],
                dproj[b],
                times_by_run[a][name]["at_ms"],
                times_by_run[b][name]["at_ms"],
            )
        elif rec["silent_runs"] and len({dsig[r] for r in usable if dproj[r]}) == 1:
            # **Nothing vanilla said varied. Whether we recorded it did.**
            #
            # Every run that produced anything produced the byte-identical
            # thing, and the rest produced nothing at all. A reference that
            # varied its behaviour would not agree with itself perfectly on the
            # runs where it spoke, so the quantity that moved is on our side of
            # the wire: the scenario did not happen, or the client that was
            # supposed to watch it was not watching.
            #
            # This is the class `compare` reports as `vacuous`, and it is why
            # `ci/t2-gameplay-selfcheck.sh`'s five-hour N = 26 run aborted at
            # report-5 on 2026-08-05 — `540` came back vacuous "the first time
            # both sides took the same branch of a coin flip". Absorbing it
            # into `gameplay-unstable.toml` would record a fact about vanilla
            # that the measurement refutes.
            rec["class"] = SILENT
        elif rec["blind_flaps"] < rec["flaps"]:
            rec["class"] = MIXED
        else:
            rec["class"] = REFERENCE
        # The arrival spread of every observation in this scenario, across the
        # ensemble. Cheap, and it is what turns "unstable" into "unstable and
        # here is how close our window came".
        spread = [t for r in usable for t in times_by_run[r][name]["at_ms"]]
        rec["at_ms_range"] = [min(spread), max(spread)] if spread else None
        out[name] = rec
    return out


def cmd_measure(args) -> int:
    ensemble = json.load(open(args.ensemble))
    runs = ensemble["runs"]
    ids = [r["id"] for r in runs]
    if len(ids) < 2:
        print(
            f"corpus-floor: need at least 2 recordings, got {len(ids)} — "
            f"one recording measures nothing and a floor of 0 from it would be a lie",
            file=sys.stderr,
        )
        return 2
    declared_by_run = {r["id"]: load_projection(r["declared"], "declared") for r in runs}
    blind_by_run = {r["id"]: load_projection(r["blind"], "boundary-blind") for r in runs}

    # --- the ensemble's vocabulary --------------------------------------------
    #
    # **What the recordings could say, before anything about what they said.**
    # `projection` names the comparator; this names the decoder that produced
    # what the comparator reads. `ci/trace_vocabulary.py`'s header has the whole
    # argument; the short form is that a projection acting on a field the
    # recordings do not carry is inert over them, so the same projection id over
    # two ensembles can be two instruments.
    #
    # Taken from files `ci/corpus-floor.sh` wrote into the measurement
    # directory, not from the recordings, for the reason `declared` and `blind`
    # are: a saved measurement must stay re-adjudicable when the recordings it
    # came from have been deleted, and one of them already has been.
    vocab_by_run: dict[str, list[str]] = {}
    for r in runs:
        src = r.get("vocabulary")
        if not src:
            print(
                f"corpus-floor: REFUSED — run {r['id']} carries no vocabulary. The "
                f"ensemble was built by a `ci/corpus-floor.sh` older than "
                f"docs/PARITY-ROADMAP.md §15.4; re-run it. A floor whose recordings "
                f"cannot say what their decoder emitted is quotable beside a score it "
                f"is not comparable with, which is the defect the field was added for.",
                file=sys.stderr,
            )
            return 2
        try:
            with open(src) as fh:
                vocab_by_run[r["id"]] = sorted(json.load(fh))
        except Exception as e:  # noqa: BLE001 — an unreadable vocabulary is one refusal
            print(f"corpus-floor: REFUSED — {src}: {e}", file=sys.stderr)
            return 2
        if not vocab_by_run[r["id"]]:
            print(
                f"corpus-floor: REFUSED — run {r['id']} has an EMPTY vocabulary. The "
                f"empty set is a subset of every ensemble, so a floor carrying it "
                f"could never warn about anything.",
                file=sys.stderr,
            )
            return 2
    vocab_union: set[tuple[str, str]] = set()
    vocab_common: set[tuple[str, str]] | None = None
    for rid in ids:
        here = tv.parse(vocab_by_run[rid])
        vocab_union |= here
        vocab_common = here if vocab_common is None else (vocab_common & here)

    # Only scenarios every run recorded. A scenario a recording predates is
    # missing from one side, and reading that as an instability would be an
    # instrument manufacturing its own finding — the failure this file exists
    # to detect, by our own hand. `ci/selfcheck-recompare.sh` makes the same
    # intersection for the same reason and prints how many it dropped.
    common = None
    for r in ids:
        here = {n for n, s in declared_by_run[r].items() if s["present"]}
        common = here if common is None else (common & here)
    everything = set()
    for r in ids:
        everything |= set(declared_by_run[r])
    dropped = sorted(everything - common)

    full = measure_group(sorted(common), ids, declared_by_run, blind_by_run, declared_by_run)

    # The homogeneity control. See the header: recordings from different trees
    # were taken by different recorders, and a disagreement between two of
    # them is not automatically vanilla's.
    by_recorder: dict[str, list[str]] = {}
    for r in runs:
        by_recorder.setdefault(r.get("recorder_digest", "unknown"), []).append(r["id"])
    homogeneous = max(by_recorder.values(), key=len)
    homogeneous_digest = next(k for k, v in by_recorder.items() if v is homogeneous)
    # A tie between two equally large recorder groups means "largest" was a
    # coin toss, and the margin and the corroboration column both come out of
    # whichever side it landed on. Deterministic — `by_recorder` keeps the
    # ensemble's own order — but arbitrary, so it is said rather than hidden.
    tied = [k for k, v in by_recorder.items() if len(v) == len(homogeneous)]
    subset = (
        measure_group(
            sorted(common), homogeneous, declared_by_run, blind_by_run, declared_by_run
        )
        if len(homogeneous) >= 2
        else {}
    )
    # **The knife-edge margin belongs to ONE harness and must be measured
    # inside one**, which the first version of this got wrong and the
    # heterogeneity control caught. In the first real ensemble, four recordings
    # were taken with `Step::Hold` returning 60 ms and two with 250 ms — the
    # 2026-08-09 repair, `599aed4b`. `040`'s tightest boundary is **35 ms**
    # under the old window and **253 ms** under the new one, so a margin taken
    # across the mixture publishes a fragility the tree has already fixed,
    # which is the stale-caveat failure this repository keeps paying for.
    #
    # So the margin comes from the largest one-recorder subset, and the file
    # records which runs and which recorder it came from. With a homogeneous
    # ensemble that is every run and the field is a formality; with a mixed
    # one it is the difference between a warning and a fossil.
    margin_source = homogeneous if len(homogeneous) >= 1 else ids
    for name in full:
        tight, run_ids = None, [r for r in margin_source if r in full[name]["run_ids"]]
        for r in run_ids:
            for gap, s0, s1, k0, k1 in boundary_margins(
                declared_by_run[r][name]["declared"], declared_by_run[r][name]["at_ms"]
            ):
                if tight is None or gap < tight["margin_ms"]:
                    tight = {
                        "margin_ms": gap,
                        "run": r,
                        "between_steps": [s0, s1],
                        "kinds": [k0, k1],
                    }
        full[name]["tightest_boundary"] = tight
        full[name]["knife_edge"] = bool(tight and tight["margin_ms"] < TICK_MS)

    for name, rec in full.items():
        sub = subset.get(name)
        rec["homogeneous_flaps"] = None if sub is None else sub["flaps"]
        rec["homogeneous_pairs"] = None if sub is None else sub["pairs"]
        # A flap the homogeneous subset had the pairs to see and did not see is
        # not admissible as vanilla's. It is not called stable either — it is
        # called suspect, which is what it is.
        rec["recorder_suspect"] = bool(
            rec["flaps"] > 0 and sub is not None and sub["pairs"] >= 1 and sub["flaps"] == 0
        )

    floor = sorted(n for n, r in full.items() if r["flaps"] > 0)
    knife = sorted(n for n, r in full.items() if r.get("knife_edge"))
    # **Which comparator this floor was taken under.** `ci/corpus-floor.sh`
    # reads it out of the binary that did the projecting. A measurement made
    # before that field existed carries `unknown`, and `cmd_update` refuses to
    # write a floor file from one: an unattributed figure is the thing the
    # field was added to stop, and blessing it with a placeholder would make
    # the check unfalsifiable on exactly the files that most need it.
    projection = ensemble.get("projection", "unknown")
    doc = {
        "projection": projection,
        # The union, because the floor is a property of the ensemble and not of
        # any one recording in it. `vocabulary_partial` is the other half: pairs
        # the union has that some recording lacks, which is *normal* and is
        # exactly why `ci/corpus-floor-report.py` tests one direction only.
        "vocabulary": tv.fmt(vocab_union),
        "vocabulary_partial": tv.fmt(vocab_union - (vocab_common or set())),
        "vocabulary_by_run": vocab_by_run,
        "knife_edge": knife,
        "knife_edge_from": margin_source,
        "knife_edge_recorder": homogeneous_digest,
        "tick_ms": TICK_MS,
        "ensemble": {
            "id": ensemble.get("id", "unnamed"),
            "runs": runs,
            "n": len(ids),
            "pairs": len(ids) * (len(ids) - 1) // 2,
            "recorders": len(by_recorder),
            "homogeneous_subset": homogeneous,
            "homogeneous_recorder": homogeneous_digest,
            "homogeneous_tied": len(tied),
        },
        "alpha": ALPHA,
        "p_hide": p_hide(len(ids)),
        "scenarios_common": len(common),
        "scenarios_dropped": dropped,
        "floor": len(floor),
        "floor_names": floor,
        "by_class": {
            c: sorted(n for n, r in full.items() if r["class"] == c)
            for c in (REFERENCE, BOUNDARY, SILENT, MIXED, STABLE, DEAD, UNMEASURED)
        },
        "measured": full,
    }
    Path(args.out).write_text(json.dumps(doc, indent=1, sort_keys=True))
    report(doc)
    print(f"wrote {args.out}")
    return 0


def report(doc) -> None:
    e = doc["ensemble"]
    n, common = e["n"], doc["scenarios_common"]
    cls = doc["by_class"]
    print()
    print(
        f"CORPUS FLOOR  {doc['floor']}/{common} scenarios do not reproduce "
        f"between two vanilla runs"
    )
    print(
        f"              {len(cls[REFERENCE])} reference, {len(cls[MIXED])} mixed "
        f"(vanilla's) + {len(cls[BOUNDARY])} boundary, {len(cls[SILENT])} silent "
        f"(ours); {len(cls[DEAD])} dead, {len(cls[STABLE])} reproduced"
    )
    print(f"              N = {n} recordings, {e['pairs']} pairwise comparisons")
    print(
        f"              RESOLVING POWER: a scenario flapping less than "
        f"{doc['p_hide'] * 100:.1f}% of the time can still show {n} identical runs "
        f"(alpha = {doc['alpha']})."
    )
    print(
        "              THE FLOOR IS A LOWER BOUND AND GROWS WITH N. "
        "docs/PARITY.md §4: the End's floor was 0 across two references and 252 across six."
    )
    if e["recorders"] > 1:
        print(
            f"              HETEROGENEOUS: {e['recorders']} distinct recorders in this "
            f"ensemble; largest homogeneous subset is {len(e['homogeneous_subset'])} "
            f"({', '.join(e['homogeneous_subset'])})"
        )
    else:
        print(
            f"              HOMOGENEOUS: one recorder ({e['homogeneous_recorder']}) took "
            f"all {n} recordings, so no disagreement here can be a decoder difference"
        )
    if e.get("homogeneous_tied", 1) > 1:
        print(
            f"              TIED SUBSETS: {e['homogeneous_tied']} recorder groups of "
            f"{len(e['homogeneous_subset'])}; the one used for the margin and the "
            f"corroboration column is the ensemble's first and the choice is arbitrary"
        )
    if len(e["homogeneous_subset"]) < 2:
        print(
            "              NO HOMOGENEITY CONTROL: no two recordings share a recorder, so "
            "nothing below is corroborated within one decoder and none of it is promotable"
        )
    # **The vocabulary, beside the recorder digest and saying a different thing.**
    # The digest answers "was this the same code", which differs between every
    # pair of trees and so can only ever say no. The vocabulary answers "could
    # the same fields be seen", which is content-derived and comparable — the
    # reason §15.4 calls it the stronger half.
    vocab = doc.get("vocabulary") or []
    partial = doc.get("vocabulary_partial") or []
    print(
        f"              VOCABULARY: {len(vocab)} (kind, field) pairs across the "
        f"ensemble; {len(vocab) - len(partial)} in every recording"
    )
    if partial:
        print(
            f"              {len(partial)} pair(s) not in every recording — normal, and "
            f"why the gate warns on the RUN's extras only: {', '.join(partial[:6])}"
            + (" ..." if len(partial) > 6 else "")
        )
    if doc["scenarios_dropped"]:
        print(
            f"              {len(doc['scenarios_dropped'])} scenario(s) not in every "
            f"recording and not adjudicated: {', '.join(doc['scenarios_dropped'][:6])}"
            + (" ..." if len(doc["scenarios_dropped"]) > 6 else "")
        )
    print()
    for name in doc["floor_names"]:
        r = doc["measured"][name]
        tag = "recorder-suspect" if r["recorder_suspect"] else r["class"]
        homo = (
            ""
            if r["homogeneous_flaps"] is None
            else f", homogeneous {r['homogeneous_flaps']}/{r['homogeneous_pairs']}"
        )
        print(
            f"  {tag:<16} {name}  ({r['flaps']}/{r['pairs']} pairs, "
            f"{r['distinct']} distinct projections{homo})"
        )
        if r["class"] == SILENT:
            print(
                f"                     OURS: {len(r['silent_runs'])} of {r['runs']} runs "
                f"({', '.join(r['silent_runs'])}) recorded nothing at all; every run that "
                f"recorded anything is byte-identical"
            )
        if r["class"] in (BOUNDARY, MIXED) and r["step_moves"]:
            for m in r["step_moves"][:3]:
                print(
                    f"                     OURS: {m['kind']} moved "
                    f"{m['moved']} at {m['at_ms']} ms — our boundary is inside "
                    f"vanilla's arrival spread"
                )
    for name in doc["by_class"][DEAD]:
        print(f"  {'dead':<16} {name}  (the reference's projection is empty in all {n} runs)")
    if doc.get("knife_edge"):
        print()
        print(
            f"  KNIFE-EDGE BOUNDARIES — under {doc['tick_ms']} ms of slack, which is one "
            f"server tick. Not instabilities: instabilities waiting for a shorter window."
        )
        print(
            f"  measured inside ONE harness ({', '.join(doc['knife_edge_from'])}, recorder "
            f"{doc['knife_edge_recorder']}) — a margin is a property of the recorder that "
            f"took the recording, not of the reference"
        )
        for name in doc["knife_edge"]:
            t = doc["measured"][name]["tightest_boundary"]
            print(
                f"  {'knife-edge':<16} {name}  ({t['margin_ms']} ms between step "
                f"{t['between_steps'][0]} and step {t['between_steps'][1]} in {t['run']}: "
                f"{t['kinds'][0]} then {t['kinds'][1]})"
            )


def declared_rate(entry, runs):
    """`(p_miss, how)` for an entry not seen in a run of `runs` recordings.

    Exponent is `runs - 1` and never the pair count. See the header.
    """
    flaps, comps = entry.get("flaps"), entry.get("comparisons")
    c = max(runs - 1, 1)
    if flaps is None or comps is None:
        return None, "the entry declares no flaps/comparisons rate"
    if not isinstance(flaps, int) or not isinstance(comps, int):
        return None, f"flaps/comparisons are not integers ({flaps!r}/{comps!r})"
    if comps <= 0:
        return None, f"declared comparisons is {comps}, which cannot be a denominator"
    if flaps <= 0:
        return None, f"declared flaps is {flaps} — an entry with no measured flap has no rate"
    if flaps > comps:
        return None, f"declared {flaps}/{comps} — more flaps than comparisons"
    p = flaps / comps
    return (1.0 - p) ** c, (
        f"declared {flaps}/{comps}, so {c} independent runs miss it with probability "
        f"(1 - {flaps}/{comps})^{c} = {1 - p:.6g}^{c} = {(1 - p) ** c:.3g}"
    )


def check_floor_file(path: str, doc) -> list[str]:
    """The standing floor file against the measurement in front of us.

    A floor is a published figure — `ci/t2-gameplay.sh` prints it beside every
    corpus score — so a stale one is the failure mode `ci/check-figures.py`
    exists for, one document over. Every field is re-derived and the file has
    no authority of its own: it is a cache of a measurement, and a cache that
    disagrees with the thing it caches is a red.
    """
    bad = []
    try:
        with open(path, "rb") as fh:
            f = tomllib.load(fh)
    except FileNotFoundError:
        print(f"  NO FLOOR FILE  {path} is missing — a corpus score cannot be quoted without it")
        return ["<floor-file>"]
    print(f"== standing floor against measured, {path}")
    want = {
        "floor": doc["floor"],
        "floor_reference": len(doc["by_class"][REFERENCE]) + len(doc["by_class"][MIXED]),
        "floor_ours": len(doc["by_class"][BOUNDARY]) + len(doc["by_class"][SILENT]),
        "runs": doc["ensemble"]["n"],
        "pairs": doc["ensemble"]["pairs"],
        "scenarios": doc["scenarios_common"],
        "tick_ms": doc["tick_ms"],
    }
    got = f.get("floor", {})
    for k, v in want.items():
        if got.get(k) != v:
            print(f"  FLOOR STALE  [floor].{k} declares {got.get(k)!r}, measured {v!r}")
            bad.append(f"floor.{k}")
    if got.get("knife_edge") != doc["knife_edge"]:
        print(
            f"  FLOOR STALE  [floor].knife_edge declares {got.get('knife_edge')}, "
            f"measured {doc['knife_edge']}"
        )
        bad.append("floor.knife_edge")
    rows = {e["name"]: e for e in f.get("evidence", [])}
    if sorted(rows) != doc["floor_names"]:
        print(
            f"  FLOOR STALE  the evidence rows are {sorted(rows)}, "
            f"the measured floor is {doc['floor_names']}"
        )
        bad.append("floor.evidence")
    for name, row in rows.items():
        m = doc["measured"].get(name)
        if m is None:
            continue
        for k, v in (
            ("class", m["class"]),
            ("flaps", m["flaps"]),
            ("pairs", m["pairs"]),
            ("distinct_projections", m["distinct"]),
            ("blind_flaps", m["blind_flaps"]),
        ):
            if row.get(k) != v:
                print(f"  EVIDENCE STALE  {name}.{k} declares {row.get(k)!r}, measured {v!r}")
                bad.append(f"{name}.{k}")
    # The ensemble is named by digest, so a floor quoted from recordings nobody
    # can name is not quotable. This does not require the *same* ensemble as
    # the one in front of us — re-measuring on new recordings is the point —
    # only that the file says which one it came from.
    if not f.get("recording"):
        print("  NO PROVENANCE  the floor file names no recordings")
        bad.append("floor.recording")
    # **And which comparator.** Unlike the ensemble, this one *must* be the
    # same: the recordings are evidence and may be replaced, but a floor and
    # the measurement it is being checked against are two readings of one
    # quantity only if they were taken through the same instrument. A file
    # that names none at all is the pre-2026-08-09 shape and is refused rather
    # than excused, because every floor written before that field existed was
    # written under a comparator nobody recorded.
    # **And what the recordings could say.** Same argument as `projection` one
    # step further out: the recordings are evidence and may be replaced, but a
    # floor file and the measurement it is checked against are two readings of
    # one quantity only if the same fields existed to be read. A file with no
    # `vocabulary` at all is the pre-§15.4 shape and is refused rather than
    # excused — `ci/corpus-floor-report.py` cannot warn about drift it has no
    # baseline for, so an absent vocabulary silently disables the check that
    # exists to keep the floor comparable.
    if not got.get("vocabulary"):
        print(
            "  NO VOCABULARY  the floor file does not say which (kind, field) pairs its "
            "recordings carried — re-measure with ci/corpus-floor.sh"
        )
        bad.append("floor.vocabulary")
    elif sorted(got["vocabulary"]) != doc.get("vocabulary", []):
        was, now = set(got["vocabulary"]), set(doc.get("vocabulary", []))
        print(
            f"  VOCABULARY STALE  the floor file declares {len(was)} (kind, field) "
            f"pair(s), this measurement has {len(now)}. Only in the file: "
            f"{sorted(was - now) or '-'}. Only in the measurement: {sorted(now - was) or '-'}."
        )
        bad.append("floor.vocabulary")
    here = doc.get("projection", "unknown")
    if not f.get("floor", {}).get("projection"):
        print(
            "  NO PROJECTION  the floor file does not say which comparator measured it — "
            "re-measure with ci/corpus-floor.sh"
        )
        bad.append("floor.projection")
    elif got.get("projection") != here:
        print(
            f"  STALE PROJECTION  the floor was measured under {got['projection']}, "
            f"this measurement under {here}. `oracle-runner projection-id --explain` "
            f"says which source moved."
        )
        bad.append("floor.projection")
    if not bad:
        print(
            f"  floor {got.get('floor')}/{got.get('scenarios')} at N = {got.get('runs')}, "
            f"p_hide {got.get('p_hide')}, {len(rows)} evidence row(s) — all reproduce"
        )
    print()
    return bad


# A field whose value still says this is not a claim yet.
TODO = ("todo", "tbd", "xxx", "fixme", "?", "")


def unmechanised(entry: dict, table: str) -> str | None:
    """Why this entry is arithmetic with no claim attached, or `None`.

    **The clause that stops `emit` from being a one-command way to silence a
    scenario.** Everything else here checks that the numbers are real; this
    checks that somebody said what the numbers are *of*. A rate is a
    measurement and this tool produces it; `field` and `why` are a mechanism
    read out of the pinned 26.2 jar and no measurement can supply them, which
    is exactly why `emit` prints them as `TODO` and refuses to guess.

    `ci/checks/gameplay-unstable.toml`'s own header makes the point in its
    strongest form — "these two runs differed" is a symptom and that file is a
    list of causes — and it has been true by convention since the file was
    written. Convention is what this whole protocol is replacing.
    """
    need = ("class", "measured", "why") + (
        ("field",) if table == "unstable" else ("repair",)
    )
    # `[<table>.evidence]` with an `ensemble` is the window, and it is required
    # for the reason every window field in `ci/figures.toml` is required: a rate
    # belongs to the ensemble that produced it, and two ensembles' rates in one
    # list read as one list. `ci/corpus-floor.py emit` has printed this
    # sub-table since it was written; the five entries that predate it had none,
    # and the cost was not cosmetic — see `cmd_adjudicate`, where the
    # EVIDENCE DOES NOT REPRODUCE check is guarded on
    # `evidence.distinct_projections is not None` and was therefore unreachable
    # on every live entry, five out of five.

    for key in need:
        v = entry.get(key)
        if v is None:
            return f"no `{key}`"
        if not isinstance(v, str):
            return f"`{key}` is not text"
        head = v.strip().lower()
        if head in TODO or head.startswith(tuple(f"{t} " for t in TODO if t)):
            return f"`{key}` is still {v.strip()!r}"
    return None


def cmd_adjudicate(args) -> int:
    doc = json.load(open(args.floor))
    measured = doc["measured"]
    n = doc["ensemble"]["n"]
    with open(args.declared, "rb") as fh:
        decl = tomllib.load(fh)
    unstable = {u["name"]: u for u in decl.get("unstable", [])}
    harness = {h["name"]: h for h in decl.get("harness", [])}
    bad = []

    report(doc)
    print()
    if args.floor_file:
        bad += check_floor_file(args.floor_file, doc)
    print(f"== declared against measured, {args.declared}")

    # A name in both tables is the one thing neither table can mean. `unstable`
    # says the reference cannot reproduce this; `harness` says the reference
    # can and we cannot sample it. Both at once is not a strong claim, it is
    # an unfalsifiable one.
    for name in sorted(set(unstable) & set(harness)):
        print(f"  BOTH TABLES  {name} — an entry is a reference fact or a repair order, never both")
        bad.append(name)

    # Before any arithmetic, in both tables: an entry has to say what it is a
    # claim ABOUT. Checked even for a name this ensemble never recorded — the
    # measurement cannot excuse a missing mechanism and must not look as though
    # it did.
    # THE WINDOW, BEFORE THE MECHANISM. A rate belongs to the ensemble that
    # produced it — `docs/PARITY.md` 4.1 states the rule for the corpus floor's
    # 9 and 7 — and this file held two ensembles' rates in one list with nothing
    # saying which was which. It was not cosmetic: the EVIDENCE DOES NOT
    # REPRODUCE check below is guarded on `evidence.distinct_projections`, and
    # with no `[evidence]` table at all it was unreachable on five entries out
    # of five. `ci/corpus-floor.py emit` has printed the sub-table since it was
    # written; the entries that predate it are the ones that had none.
    for table, rows in (("unstable", unstable), ("harness", harness)):
        for name, entry in sorted(rows.items()):
            ev_t = entry.get("evidence")
            if not isinstance(ev_t, dict) or not str(ev_t.get("ensemble", "")).strip():
                print(
                    f"  UNSTAMPED  {name} — no `[{table}.evidence]` naming the "
                    f"`ensemble` this rate was measured over. Two ensembles' "
                    f"rates in one list read as one list, and a rate without its "
                    f"ensemble is not a weaker claim than one with it — it is a "
                    f"different one. `ci/corpus-floor.py emit` prints the field."
                )
                bad.append(name)

    for table, rows in (("unstable", unstable), ("harness", harness)):
        for name, entry in sorted(rows.items()):
            if (why := unmechanised(entry, table)) is not None:
                print(
                    f"  NO MECHANISM  {name} — {why}. An entry is a cause, not a rate; "
                    f"`ci/corpus-floor.py emit` prints those fields as TODO on purpose "
                    f"and cannot fill them in."
                )
                bad.append(name)

    for name in sorted(unstable):
        m = measured.get(name)
        if m is None or m["runs"] < 2:
            print(f"  NOT IN THIS RUN  {name} — no usable pair in this ensemble")
            continue
        if m["flaps"] == 0:
            p_miss, how = declared_rate(unstable[name], n)
            if p_miss is not None and p_miss > ALPHA:
                print(
                    f"  UNRESOLVED AT THIS N  {name} — {how} > alpha = {ALPHA}; "
                    f"this ensemble could not have seen it, so its absence is not evidence"
                )
                continue
            detail = (
                f"{how} <= alpha = {ALPHA}; this ensemble had the power to see it"
                if p_miss is not None
                else f"NO POWER TEST POSSIBLE — {how}"
            )
            print(
                f"  NO LONGER UNSTABLE  {name} — {m['distinct']} distinct projection(s) "
                f"in {m['runs']} runs; {detail}; the entry is stale"
            )
            bad.append(name)
            continue
        if m["class"] in OURS:
            why = (
                "0 pairs differ under boundary-blind — vanilla said the same things and "
                "we filed them under different steps"
                if m["class"] == BOUNDARY
                else f"{len(m['silent_runs'])} of {m['runs']} runs recorded nothing at all "
                f"and every run that recorded anything is byte-identical — nothing vanilla "
                f"said varied, only whether we saw it"
            )
            print(
                f"  MISCLASSIFIED  {name} — declared a fact about vanilla, measured as "
                f"ours ({m['class']}): {why}. It belongs in [[harness]] as a repair order."
            )
            bad.append(name)
            continue
        if m["distinct"] < 2:
            print(f"  NO EVIDENCE  {name} — {m['flaps']} flaps but 1 distinct projection")
            bad.append(name)
            continue
        ev = unstable[name].get("evidence", {})
        claimed = ev.get("distinct_projections")
        # An entry measured over THESE recordings and not saying how many
        # distinct projections it saw switches off the only check that can
        # contradict it. An entry from another ensemble is a different sample
        # and owes nothing; the two cases must not look alike.
        if claimed is None and _same_ensemble(ev, doc):
            print(
                f"  UNVERIFIABLE  {name} — the entry's evidence names exactly "
                f"these recordings and records no `distinct_projections`, so "
                f"EVIDENCE DOES NOT REPRODUCE cannot reach it. A rate measured "
                f"over the ensemble in front of us is checkable and must be "
                f"checked; `ci/corpus-floor.py emit` prints the field."
            )
            bad.append(name)
            continue
        if claimed is not None and claimed != m["distinct"] and _same_ensemble(ev, doc):
            print(
                f"  EVIDENCE DOES NOT REPRODUCE  {name} — the entry claims "
                f"{claimed} distinct projection(s) over these exact recordings and this "
                f"measurement finds {m['distinct']}"
            )
            bad.append(name)
            continue
        suspect = " RECORDER-SUSPECT" if m["recorder_suspect"] else ""
        print(
            f"  declared unstable  {name}  ({m['flaps']}/{m['pairs']} pairs, "
            f"{m['distinct']} distinct){suspect}"
        )

    for name in sorted(harness):
        m = measured.get(name)
        if m is None or m["runs"] < 2:
            print(f"  NOT IN THIS RUN  {name} — no usable pair in this ensemble")
            continue
        if m["flaps"] == 0:
            # **Power-tested exactly like `[[unstable]]`, and it is worth saying
            # why rather than letting the symmetry look like tidiness.**
            #
            # "This entry is stale" and "this run was too small to see it" are
            # the same two claims here as there, and reporting them identically
            # is the mistake that moved `chest-opens-a-screen` in and out of
            # the declared list four times. It is if anything worse on this
            # table: a `HARNESS REPAIRED` line is *good news*, so it is the one
            # a reader is least likely to interrogate before deleting the entry
            # and, with it, the only record that the harness ever had the bug.
            p_miss, how = declared_rate(harness[name], n)
            if p_miss is not None and p_miss > ALPHA:
                print(
                    f"  UNRESOLVED AT THIS N  {name} — {how} > alpha = {ALPHA}; "
                    f"this ensemble could not have seen it, so it is not repaired, "
                    f"it is unmeasured"
                )
                continue
            detail = (
                f"{how} <= alpha = {ALPHA}; this ensemble had the power to see it"
                if p_miss is not None
                else f"NO POWER TEST POSSIBLE — {how}"
            )
            print(
                f"  HARNESS REPAIRED  {name} — 1 distinct projection in {m['runs']} "
                f"runs; {detail}. Remove the entry."
            )
            bad.append(name)
            continue
        if m["class"] not in OURS:
            print(
                f"  MISCLASSIFIED  {name} — declared a repair order, measured as "
                f"{m['class']}: {m['blind_flaps']}/{m['pairs']} pairs still differ with "
                f"step and timing blanked and the runs that spoke did not agree, so "
                f"vanilla is saying different things and no window of ours will fix it"
            )
            bad.append(name)
            continue
        if declared_class := harness[name].get("class"):
            if declared_class != m["class"]:
                print(
                    f"  WRONG REPAIR  {name} — declared class {declared_class!r}, measured "
                    f"{m['class']!r}. The two have different repairs: a boundary moves a "
                    f"window, a silent run means the scenario did not happen."
                )
                bad.append(name)
                continue
        print(
            f"  declared harness   {name}  ({m['class']}, {m['flaps']}/{m['pairs']} pairs, "
            f"ours to fix)"
        )

    # Positive evidence at any N, untouched by the power test. Same asymmetry
    # and same reason as `ci/selfcheck-aggregate.py`: an observed disagreement
    # is a fact about the ensemble whatever its size, and only absence is a
    # claim about the sampling.
    for name in doc["floor_names"]:
        if name in unstable or name in harness:
            continue
        m = measured[name]
        table = "[[harness]]" if m["class"] in OURS else "[[unstable]]"
        print(
            f"  NEWLY UNSTABLE  {name}  ({m['class']}, {m['flaps']}/{m['pairs']} pairs, "
            f"{m['distinct']} distinct) — belongs in {table}"
        )
        # The whole point of the protocol is that this stops being hand-work,
        # so the command that writes the entry is printed beside the finding
        # rather than left in a header somebody has to remember. It fills in
        # every number and every piece of provenance and leaves exactly the
        # mechanism, which is the part that must not be automatic.
        print(
            f"                  ci/corpus-floor.py emit {args.floor} --only {name}"
        )
        bad.append(name)

    print(
        f"  {n} recordings, floor {doc['floor']}/{doc['scenarios_common']}, "
        f"declared {len(unstable)} unstable + {len(harness)} harness, "
        f"p_hide {doc['p_hide'] * 100:.1f}%"
    )
    if bad:
        print(
            f"CORPUS FLOOR FAILED: {len(bad)} scenario(s) where the declared set and the "
            f"measurement disagree",
            file=sys.stderr,
        )
        return 1
    return 0


def _same_ensemble(evidence, doc) -> bool:
    """Whether an entry's evidence names exactly the recordings in front of us.

    Only then is a rate mismatch a contradiction rather than two honest
    measurements of different samples.
    """
    claimed = evidence.get("recordings")
    if not claimed:
        return False
    have = {r.get("sha256") for r in doc["ensemble"]["runs"]}
    return set(claimed) == have


def cmd_emit(args) -> int:
    """Print the TOML an unstable or harness scenario should be filed under.

    Printed rather than written into the file: the two prose fields — `field`
    and `why` — are a mechanism read out of vanilla's bytecode, and no
    measurement can supply them. What this removes is the arithmetic, the
    provenance and the evidence, which is every part that has been got wrong
    by hand. An entry pasted from here with `field = "TODO"` still fails
    `ci/corpus-floor-falsify.sh` case 8.
    """
    doc = json.load(open(args.floor))
    e = doc["ensemble"]
    digests = [r["sha256"] for r in e["runs"]]
    sources = [f"{r['id']}={r['source']}" for r in e["runs"]]
    names = [args.only] if args.only else doc["floor_names"]
    for name in names:
        m = doc["measured"].get(name)
        if m is None:
            print(f"# {name}: not in this measurement", file=sys.stderr)
            return 2
        if m["flaps"] == 0:
            # The refusal. A scenario the reference reproduced cannot be given
            # an entry by this tool at any N, and saying why is more useful
            # than saying no.
            print(
                f"# REFUSED  {name}: {m['distinct']} distinct projection over "
                f"{m['runs']} runs and {m['pairs']} pairs — the reference reproduced it. "
                f"There is nothing to declare.",
                file=sys.stderr,
            )
            return 2
        table = "harness" if m["class"] in OURS else "unstable"
        print(f"[[{table}]]")
        print(f'name = "{name}"')
        print(f"flaps = {m['flaps']}")
        print(f"comparisons = {m['pairs']}")
        print(f"runs = {m['runs']}")
        print(f'class = "{m["class"]}"')
        print(
            f'measured = "{m["flaps"]}/{m["pairs"]} pairs over N = {m["runs"]} '
            f'independent vanilla recordings, ensemble {e["id"]}, '
            f'p_hide {doc["p_hide"] * 100:.1f}% at alpha {doc["alpha"]}"'
        )
        if table == "unstable":
            print('field = "TODO — the packet and the field that moves"')
            print('why = """TODO — the mechanism, read from the pinned 26.2 jar."""')
        elif m["class"] == BOUNDARY:
            print('repair = "TODO — which window or step boundary must move, and to what"')
        else:
            print(
                'repair = "TODO — why the scenario produced nothing in '
                f'{len(m["silent_runs"])} of {m["runs"]} runs"'
            )
        print("")
        print(f"[{table}.evidence]")
        print(f'ensemble = "{e["id"]}"')
        print(f"distinct_projections = {m['distinct']}")
        print(f"blind_flaps = {m['blind_flaps']}")
        print(
            f"homogeneous_flaps = {m['homogeneous_flaps']}  "
            f"# of {m['homogeneous_pairs']} pairs in the largest one-recorder subset"
        )
        if m.get("at_ms_range"):
            print(f"at_ms_range = {m['at_ms_range']}")
        print("recordings = [")
        for d in digests:
            print(f'    "{d}",')
        print("]")
        print("sources = [")
        for s in sources:
            print(f'    "{s}",')
        print("]")
        if m["silent_runs"]:
            print(f"silent_runs = {json.dumps(m['silent_runs'])}")
        print(f"first_difference = {json.dumps(m['first_difference'])}")
        for mv in m["step_moves"][:3]:
            print(
                f"# OURS: {mv['kind']} moved {mv['moved']} at {mv['at_ms']} ms"
            )
        print("")
    return 0


FLOOR_HEADER = '''# THE STANDING CORPUS FLOOR — GENERATED. Do not hand-edit.
#
#     ci/corpus-floor.py update <floor.json> --out ci/checks/gameplay-floor.toml
#
# How much of the gameplay corpus **vanilla cannot reproduce against itself**,
# measured over the ensemble named below. This is the corpus analogue of the
# twin-mask floor every worldgen figure in `docs/PARITY.md` is quoted beside,
# and it exists for the same reason: a residual smaller than the floor is not
# a defect, it is the reference disagreeing with itself.
#
# FOUR THINGS TO READ BEFORE QUOTING ANYTHING HERE.
#
# 0. **`projection` is which comparator measured it, and it is the field that
#    decides whether the rest may be quoted at all.** A floor is a count of
#    scenarios two vanilla runs project differently, so it is a number *under a
#    projection*, and until 2026-08-09 it did not say which one.
#    `ci/t2-gameplay.sh` checked this file's `scenarios` against the run's — 88
#    against 88 — and called that staleness; it did not check the comparator.
#    `gameplay-container-throw` then became the first branch ever to change
#    `compare::project`, dropping an item entity's spawn yaw, and two of the
#    nine names below are scenarios that change converts. The denominator was
#    still 88, so the gate printed `9/88` and stayed green while the figure
#    described a comparator that no longer existed — a check that could not
#    fail, in the file that exists to catch them. `ci/t2-gameplay.sh` now
#    re-derives the identity from the binary it is about to run and prints
#    `STALE PROJECTION` beside the score, withdrawing the floor's quotability,
#    when it does not match. `oracle-runner projection-id --explain` says which
#    source moved.
#
# 1. **The floor is a LOWER BOUND and it grows with N.** `docs/PARITY.md` §4
#    records the trap in its sharpest form: for the End, two references
#    agreeing gave a floor of 0 and six gave 252. `p_hide` below is what this
#    N could not have seen — at N = 6 a scenario flapping less than 39% of the
#    time can still show six identical runs. Quote the floor with its N and its
#    `p_hide` or do not quote it.
#
# 2. **It is reported beside a corpus score and never subtracted from one.**
#    Nothing here changes a verdict, moves the baseline ratchet, or makes a
#    scenario stop failing. `ci/t2-gameplay.sh` prints
#    `declared X/N ... floor F` and the floor is an annotation on the
#    denominator, not a deduction from the numerator.
#
# 3. **`class` says whose the instability is.** `reference` and `mixed` are
#    vanilla's and belong in `ci/checks/gameplay-unstable.toml`; `boundary` and
#    `silent` are OURS and belong in that file's `[[harness]]` table, which is
#    a list of repair orders. `ci/corpus-floor.py adjudicate` refuses a claim
#    filed under the wrong one, in both directions.
#
# `knife_edge` is the half a flap-only instrument cannot reach: a step boundary
# with less than one server tick of observed slack in front of it. Those
# scenarios have never flapped. They are one shortened window from it.
'''


def cmd_update(args) -> int:
    """Write the standing floor file from a measurement. Never by hand."""
    doc = json.load(open(args.floor))
    e = doc["ensemble"]
    projection = doc.get("projection", "unknown")
    if not str(projection).startswith("proj:"):
        print(
            f"corpus-floor: REFUSED — this measurement carries projection "
            f"{projection!r}. A floor file that cannot name the comparator it was "
            f"taken under is the defect `projection` was added to catch; re-run "
            f"ci/corpus-floor.sh with a binary that answers `projection-id`.",
            file=sys.stderr,
        )
        return 2
    lines = [FLOOR_HEADER, "[floor]"]
    lines += [
        f'ensemble = "{e["id"]}"',
        # First field after the ensemble name on purpose: it is the one that
        # decides whether any of the numbers below may be quoted beside a
        # corpus score, and a reader skimming should not have to find it.
        f'projection = "{projection}"',
        f"runs = {e['n']}",
        f"pairs = {e['pairs']}",
        f"scenarios = {doc['scenarios_common']}",
        f"floor = {doc['floor']}",
        # The split, because "cannot match" conflates two different repairs.
        # A reader quoting the floor beside a corpus score wants the total; a
        # reader deciding what to work on next wants to know how much of it is
        # ours.
        f"floor_reference = {len(doc['by_class'][REFERENCE]) + len(doc['by_class'][MIXED])}",
        f"floor_ours = {len(doc['by_class'][BOUNDARY]) + len(doc['by_class'][SILENT])}",
        f"alpha = {doc['alpha']}",
        f"p_hide = {doc['p_hide']:.6f}",
        f"tick_ms = {doc['tick_ms']}",
        f"recorders = {e['recorders']}",
        f"homogeneous_subset = {json.dumps(e['homogeneous_subset'])}",
        f"knife_edge = {json.dumps(doc['knife_edge'])}",
        f"knife_edge_from = {json.dumps(doc['knife_edge_from'])}",
        f'knife_edge_recorder = "{doc["knife_edge_recorder"]}"',
        f"not_in_every_recording = {json.dumps(doc['scenarios_dropped'])}",
        "",
    ]
    # **The vocabulary, one pair per line.** It is long — 91 pairs over
    # `twin-homog-20260809` — and a single-line array would be an
    # eight-hundred-character diff on any change, which is how a reviewer stops
    # reading a generated file. One per line makes the four `add_entity`
    # rotation fields that §15.4 is about show up as four added lines.
    lines += [
        "# The (kind, field) pairs THIS ENSEMBLE's decoder emitted. Not what the",
        "# comparator does with them — that is `projection` above. A gate run whose",
        "# own recording carries a pair absent from this list is being compared with",
        "# a field the floor's runs never had, and the floor is not quotable beside",
        "# its score; `ci/corpus-floor-report.py` says so and names the pairs. The",
        "# reverse (a run missing one of these) is normal and is not a warning — see",
        "# `vocabulary_not_in_every_recording`, which is the same thing happening",
        "# INSIDE this ensemble.",
        "vocabulary = [",
    ]
    lines += [f'  "{p}",' for p in doc["vocabulary"]]
    lines += [
        "]",
        f"vocabulary_not_in_every_recording = {json.dumps(doc['vocabulary_partial'])}",
        "",
    ]
    for r in e["runs"]:
        lines += [
            "[[recording]]",
            f'id = "{r["id"]}"',
            f'sha256 = "{r["sha256"]}"',
            f'recorded = "{r["recorded"]}"',
            f'tree = "{r["tree"]}"',
            f'recorder_digest = "{r["recorder_digest"]}"',
            "",
        ]
    for name in doc["floor_names"]:
        m = doc["measured"][name]
        lines += [
            "[[evidence]]",
            f'name = "{name}"',
            f'class = "{m["class"]}"',
            f"flaps = {m['flaps']}",
            f"pairs = {m['pairs']}",
            f"runs = {m['runs']}",
            f"distinct_projections = {m['distinct']}",
            f"blind_flaps = {m['blind_flaps']}",
            f"homogeneous_flaps = {json.dumps(m['homogeneous_flaps'])}",
            f"homogeneous_pairs = {json.dumps(m['homogeneous_pairs'])}",
            f"recorder_suspect = {json.dumps(m['recorder_suspect'])}",
        ]
        if m["silent_runs"]:
            lines.append(f"silent_runs = {json.dumps(m['silent_runs'])}")
        if m.get("at_ms_range"):
            lines.append(f"at_ms_range = {json.dumps(m['at_ms_range'])}")
        lines.append(f"first_difference = {json.dumps(m['first_difference'])}")
        lines.append("")
    for name in doc["knife_edge"]:
        t = doc["measured"][name]["tightest_boundary"]
        lines += [
            "[[knife_edge]]",
            f'name = "{name}"',
            f"margin_ms = {t['margin_ms']}",
            f'run = "{t["run"]}"',
            f"between_steps = {json.dumps(t['between_steps'])}",
            f"kinds = {json.dumps(t['kinds'])}",
            "",
        ]
    Path(args.out).write_text("\n".join(lines))
    print(f"wrote {args.out}: floor {doc['floor']}/{doc['scenarios_common']} at N = {e['n']}")
    return 0


def main(argv) -> int:
    ap = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("measure")
    m.add_argument("ensemble")
    m.add_argument("--out", required=True)
    m.set_defaults(fn=cmd_measure)
    a = sub.add_parser("adjudicate")
    a.add_argument("floor")
    a.add_argument("--declared", default="ci/checks/gameplay-unstable.toml")
    a.add_argument("--floor-file", default="ci/checks/gameplay-floor.toml")
    a.set_defaults(fn=cmd_adjudicate)
    em = sub.add_parser("emit")
    em.add_argument("floor")
    em.add_argument("--only")
    em.set_defaults(fn=cmd_emit)
    up = sub.add_parser("update")
    up.add_argument("floor")
    up.add_argument("--out", default="ci/checks/gameplay-floor.toml")
    up.set_defaults(fn=cmd_update)
    args = ap.parse_args(argv[1:])
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
