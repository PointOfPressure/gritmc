#!/usr/bin/env python3
"""The floor, printed beside a corpus score and never subtracted from it.

    ci/corpus-floor-report.py <gameplay-floor.toml> <report.json> <proj:...> <vanilla.json>

`ci/t2-gameplay.sh` calls this immediately after `oracle-runner compare`
prints the score, so the annotation lands beside the figure it annotates.

# The fourth argument is the gate's own recording, and it is not optional

`docs/PARITY-ROADMAP.md` §15.4. The projection stamp answers "same
comparator?" and cannot answer "same recorder?", and the two are independent:
a projection can act on a field the floor's recordings never carried, in which
case it is inert on one side of the comparison and live on the other. So the
run's own `vanilla.json` is read here and its vocabulary compared with the
ensemble's. It is a positional argument rather than a flag with a default
because an optional input is how a check gets silently skipped — this file's
whole subject is checks that could not fail.

# Why this is a file and not a heredoc

It was a heredoc inside `ci/t2-gameplay.sh` until 2026-08-09, and the reason
it moved is the reason everything in this directory has a falsifier: the block
had grown into the thing that decides whether a published figure may be quoted
at all, and nothing could exercise it except a thirty-five-minute corpus run.
`ci/corpus-floor-falsify.sh` cases 16-20 now drive **this exact code** with
injected floor files in about a second. A gate whose only test is the gate is
not tested.

Nothing else about it changed. It prints to the caller's stdout, so the
annotation is still two lines under the score rather than in another tool's
output, which is the whole argument for its placement.

# It is an annotation on the denominator, not a deduction from the numerator

Nothing in `ci/checks/gameplay-floor.toml` can change a verdict, move the
ratchet in `gameplay-baseline.toml`, or make a scenario stop failing.
`ci/corpus-floor-falsify.sh` case 11 asserts that neither `ci/t2-gameplay.sh`
nor this file so much as reads the declared-unstable list. The floor makes the
score honest; it does not make it better.

That case greps for the list's filename, and the name is spelled around rather
than written here for the same reason its own header gives: a guard that fires
on the documentation trains people to delete the documentation. Case 11's
needle is a filename, and a docstring is not a `#` comment.

# A missing floor is a WARNING, not a refusal, and that is deliberate

This gate's job is to measure this server against vanilla, and refusing to
report that because a *separate* standing measurement is absent would give a
stranger cloning the tree a corpus gate that cannot run. What the warning must
not do is let the score be quoted as though it had no floor, so it says
NOT QUOTABLE in the same words either way. The exit status is always 0.
"""

import json
import sys
import tomllib
from pathlib import Path

# The same module `ci/corpus-floor.py` computes the ensemble's vocabulary with.
# Two implementations of one comparison are two answers waiting for an
# occasion, and this project has paid for that twice.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_vocabulary as tv  # noqa: E402 — must follow the sys.path line

# Every field is required, and the read is inside the `try` on purpose: a floor
# file missing one is as unquotable as no floor file at all, and a traceback
# here would leave the score printed with no annotation whatever — the one
# outcome this file exists to prevent.
#
# `projection` joined the list on 2026-08-09 and is why the list is worth
# re-reading. See below.
NEEDED = (
    "floor",
    "floor_reference",
    "floor_ours",
    "scenarios",
    "ensemble",
    "runs",
    "pairs",
    "p_hide",
    "alpha",
    "tick_ms",
    "projection",
)


def vocabulary_annotation(f, recording_file) -> None:
    """`VOCABULARY DRIFT` — the run carries a pair the ensemble never produced.

    **This is §15.4's residual and the reason a projection id is not enough.**
    `proj:58380c9e4cbeae81` covers `compare.rs`, `scenario.rs` and `trace.rs`
    and not `client.rs`, so it certifies the comparator and says nothing about
    the decoder that produced what the comparator reads. Measured 2026-08-09
    and re-measured on this branch: `twin-homog-20260809`, the ensemble the
    standing floor of 9/88 rests on, has a vocabulary of 91 `(kind, field)`
    pairs; a `vanilla.json` recorded by this tree has 95 — those 91 plus
    `add_entity.{y_rot, x_rot, y_head_rot, data}`.

    `compare::exempt_item_spawn_yaw` drops an item entity's spawn yaw. Over the
    ensemble the field is not there to drop, so the two container-throw
    scenarios in the floor were compared *with* their yaw; over this run they
    are compared *without* it. One projection id, two instruments, and the
    floor sitting between them looking quotable.

    # ONE DIRECTION, AND THE OTHER ONE IS DELIBERATELY SILENT

    Strict equality over-fires and would be switched off inside a week. `r1` of
    the standing ensemble lacks all four `take_item_entity.*` pairs because
    vanilla did not pick the item up in that run — one recording in six, a
    proper subset of its own ensemble about one kind in six. That is the
    reference varying, not the recorder changing. So a run missing pairs is
    printed as a fact and withdraws nothing; only the run's **extras** do.

    # THREE OUTCOMES, FOR THE REASON THE PROJECTION BLOCK HAS THREE

    A floor file that cannot state its vocabulary is not the same as one whose
    vocabulary differs, and reporting the first as the second would turn a
    stale artefact into a false accusation against the run.
    """
    declared = f.get("vocabulary")
    try:
        run = tv.read(recording_file)
    except tv.NotATrace as e:
        print(
            f"CORPUS  floor     VOCABULARY UNVERIFIED: {e}. The floor above is NOT "
            f"QUOTABLE beside this score — nothing here can say whether it was "
            f"measured over recordings this run's decoder is comparable with."
        )
        return
    if not declared:
        print(
            f"CORPUS  floor     NO VOCABULARY: this floor file does not record which "
            f"(kind, field) pairs its recordings carried, and this run's carry "
            f"{len(run)}. The floor above is NOT QUOTABLE beside this score: a "
            f"projection id certifies the comparator and says nothing about the "
            f"decoder that fed it. Re-measure: ci/corpus-floor.sh --update"
        )
        return
    ens = tv.parse(declared)
    drift = tv.vocabulary_drift(run, ens)
    unused = tv.vocabulary_unused(run, ens)
    if drift:
        print(
            f"CORPUS  floor     VOCABULARY DRIFT: this run's recorder emits "
            f"{len(drift)} (kind, field) pair(s) the ensemble never produced — "
            f"{', '.join(drift)}. The floor above is NOT QUOTABLE beside this score: "
            f"the projection reaches those fields here and could not reach them "
            f"there, so the two are different instruments under one projection id. "
            f"Re-record the ensemble, or re-measure: ci/corpus-floor.sh --update"
        )
    else:
        print(
            f"CORPUS  floor     vocabulary {len(run)} pair(s), all within the "
            f"ensemble's {len(ens)} — same fields were available to the comparator"
        )
    # Never a warning. Said out loud anyway, because a reader looking at a
    # 91-against-87 pair of counts with no explanation will go and look for one.
    if unused:
        print(
            f"CORPUS  floor     ({len(unused)} of the ensemble's pairs are absent from "
            f"this run: {', '.join(unused[:6])}"
            + (" ..." if len(unused) > 6 else "")
            + " — vanilla varying in what it did, not a decoder difference; "
            "quotability is unaffected)"
        )


def main(argv) -> int:
    floor_file, report_file, running, recording_file = argv[1], argv[2], argv[3], argv[4]
    try:
        with open(floor_file, "rb") as fh:
            f = tomllib.load(fh)["floor"]
        missing = [k for k in NEEDED if k not in f]
        if missing:
            raise KeyError(f"[floor] is missing {', '.join(missing)}")
    except Exception as e:  # noqa: BLE001 — any unreadable floor is one outcome
        print()
        print(f"CORPUS  floor     UNKNOWN — {floor_file}: {e}")
        print(
            "CORPUS  floor     the score above is NOT QUOTABLE without one: "
            "run ci/corpus-floor.sh"
        )
        return 0
    print()
    print(
        f"CORPUS  floor     {f['floor']}/{f['scenarios']} scenarios cannot match — "
        f"{f['floor_reference']} vanilla does not reproduce against itself, "
        f"{f['floor_ours']} this harness cannot sample"
    )
    print(
        f"CORPUS  floor     ensemble {f['ensemble']}, N = {f['runs']} recordings, "
        f"{f['pairs']} pairwise comparisons"
    )
    # **THE COMPARATOR, BEFORE ANYTHING DERIVED FROM IT.**
    #
    # A floor counts scenarios two vanilla runs *project* differently, so it is
    # a number under a projection, and until 2026-08-09 it did not say which
    # one. The staleness test at the bottom of this file was the denominator
    # alone — 88 measured against 88 run — which tests how many scenarios exist
    # and not what the comparison looks at.
    #
    # `gameplay-container-throw` then became the first branch ever to change
    # `compare::project`: it drops an item entity's spawn yaw, and two of the
    # nine names in the standing floor
    # (`container-click-throw-drops-one-item` and `-drops-the-whole-stack`) are
    # scenarios that change converts. The denominator was still 88. The gate
    # printed `CORPUS floor 9/88` beside `CORPUS declared 64/88`, the floor file
    # was internally consistent, the staleness test passed, and the figure
    # described a comparator that no longer existed — a check that could not
    # fail, in the annotation that exists to keep a score honest.
    # `ci/figures.toml` registers this figure, so the twin-masking rule bites: a
    # corpus score must not be quoted beside a floor taken under a different
    # projection.
    #
    # Three outcomes, deliberately three rather than two: a binary that cannot
    # answer is not the same as a binary that answers differently, and reporting
    # the first as the second would turn a missing tool into a false accusation
    # against the floor.
    if not running.startswith("proj:"):
        print(
            f"CORPUS  floor     PROJECTION UNVERIFIED: the floor was measured under "
            f"{f['projection']} and this binary answers {running!r} to "
            f"`projection-id`. The floor above is NOT QUOTABLE beside this score."
        )
    elif running != f["projection"]:
        print(
            f"CORPUS  floor     STALE PROJECTION: measured under {f['projection']}, "
            f"this run compares under {running}. The floor above is NOT QUOTABLE "
            f"beside this score — it counts what a DIFFERENT comparator could not "
            f"reproduce. Re-measure: ci/corpus-floor.sh --update; diagnose: "
            f"oracle-runner projection-id --explain"
        )
    else:
        print(f"CORPUS  floor     projection {running}, same comparator as this run")
    # Immediately after the projection, because it is the same question one step
    # out — "same comparator?" answered, "over the same fields?" not yet.
    vocabulary_annotation(f, recording_file)
    print(
        f"CORPUS  floor     LOWER BOUND: at this N a scenario flapping less than "
        f"{f['p_hide'] * 100:.1f}% of the time still shows {f['runs']} identical "
        f"runs (alpha {f['alpha']})"
    )
    if f.get("knife_edge"):
        print(
            f"CORPUS  floor     {len(f['knife_edge'])} knife-edge boundary(ies) under "
            f"{f['tick_ms']} ms: {', '.join(f['knife_edge'])}"
        )
    # **The floor's denominator against this run's.** A scenario added since the
    # floor was measured is outside it, so the floor is a lower bound on a
    # corpus it no longer covers — the commonest way a standing figure goes
    # quietly stale, and the one nobody looks for because the floor file itself
    # is internally consistent. It is necessary and, as the projection case
    # above proves, nowhere near sufficient.
    try:
        total = json.load(open(report_file))["declared"]["total"]
    except Exception:  # noqa: BLE001 — an unreadable report is not this file's red
        total = None
    if total is not None and total != f["scenarios"]:
        print(
            f"CORPUS  floor     STALE DENOMINATOR: measured over {f['scenarios']} "
            f"scenarios, this run has {total}. Re-measure: ci/corpus-floor.sh"
        )
    print(
        "CORPUS  floor     reported BESIDE the score and never subtracted from it; "
        "ci/checks/gameplay-floor.toml"
    )
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 5:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv))
