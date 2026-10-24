#!/usr/bin/env python3
"""The vocabulary of a recording: which `(kind, field)` pairs its decoder emits.

    ci/trace_vocabulary.py <recording.json> [<recording.json> ...]

Prints the sorted union as a JSON list of `kind.field` strings. Imported as a
module by `ci/corpus-floor.py` and `ci/corpus-floor-report.py`, which is the
whole reason the filename has an underscore in it.

# WHY THIS EXISTS: A PROJECTION IDENTITY IS NECESSARY AND IT IS NOT SUFFICIENT

`docs/PARITY-ROADMAP.md` §15.2 gave the corpus floor a `projection` stamp, so
a floor measured under one comparator can no longer be quoted beside a score
taken under another. §15.4 is the residual it left, and it is this file.

**The same comparator over recordings made by two different decoders is two
instruments.** A field the projection acts on can be absent from the
recordings entirely, and then the projection is inert over them — it is
present in the binary, it is named in the stamp, and it does nothing. Measured
2026-08-09 and still true in this tree: the `twin-homog-20260809` ensemble the
standing floor rests on has a vocabulary of **91** pairs; a recording taken by
today's recorder has **95** — those 91 plus
`add_entity.{y_rot, x_rot, y_head_rot, data}`.

`compare::exempt_item_spawn_yaw` drops an item entity's spawn yaw. Over the
ensemble it cannot reach the field, so the two container-throw scenarios are
compared *with* their yaw; over a current run it can, so they are compared
*without* it. Two instruments, one projection id, and the floor of 9/88 sitting
between them looking quotable.

# THE DIRECTION IS ASYMMETRIC AND THAT IS MEASURED, NOT A SOFTENING

Strict equality over-fires and would be switched off inside a week. `r1` of
`twin-homog-20260809` legitimately lacks all four `take_item_entity.*` pairs —
vanilla did not pick the item up in that run — so one recording in six is a
proper subset of its own ensemble's union about one kind in six. A check that
reds on that is reporting normal variation in what vanilla *did*, not a
difference in what the recorder can *see*.

The dangerous direction is the other one. A pair in the run and never in the
ensemble means the run's decoder names something the ensemble's could not, so
the projection is reaching a field on one side of the comparison and not the
other. `vocabulary_drift` below is that test and only that test.

# WHAT A PAIR IS, AND WHY NOT A DIGEST

`(kind, field)` over every observation of every scenario:
`obs[].kind` against the keys of `obs[].fields`. It is content-derived, so two
trees that emit the same fields have the same vocabulary — which is exactly
what a tree hash cannot do, and why §15.4 calls the vocabulary the stronger
half of the question and `recorder=` the weaker one. A digest differs between
every pair of trees and so can only ever say "different", never "comparable".

The pairs are joined with `.` for a floor file a human reads and greps. That
join is only unambiguous if neither half contains one, so a dot in a kind or a
field is a refusal rather than a guess.
"""

from __future__ import annotations

import json
import sys

SEP = "."


class NotATrace(Exception):
    """The file is not an `oracle-runner` recording, and says which way."""


def pairs_of(doc: dict) -> set[tuple[str, str]]:
    """Every `(kind, field)` the recording contains.

    Refuses a dot in either half — see the header. The check is here rather
    than at the join so that a caller holding pairs cannot format them into an
    ambiguous string later.
    """
    out: set[tuple[str, str]] = set()
    for scen in doc.get("scenarios") or []:
        for obs in scen.get("obs") or []:
            kind = obs.get("kind")
            if kind is None:
                continue
            for field in (obs.get("fields") or {}):
                if SEP in str(kind) or SEP in str(field):
                    raise NotATrace(
                        f"({kind!r}, {field!r}) contains {SEP!r}, so `kind{SEP}field` "
                        f"would not parse back. Teach this file a new join before "
                        f"naming a packet field with a dot in it."
                    )
                out.add((str(kind), str(field)))
    return out


def read(path: str) -> set[tuple[str, str]]:
    """One recording's vocabulary, with the refusals `ci/corpus-floor.sh` makes.

    The report-not-a-recording case is repeated from there on purpose: that
    trap is closed wherever the paths come from, and this file is now a second
    place they come from.
    """
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except Exception as e:  # noqa: BLE001 — every unreadable input is one refusal
        raise NotATrace(f"{path} is not readable JSON: {e}") from e
    if "outcomes" in doc and "scenarios" not in doc:
        raise NotATrace(
            f"{path} is a comparison REPORT, not a recording. The `r*.json` glob "
            f"picks up `report-N.json`; use `r[0-9]*.json`."
        )
    if not isinstance(doc.get("scenarios"), list) or "recorder" not in doc:
        raise NotATrace(f"{path} is not an oracle-runner trace")
    v = pairs_of(doc)
    # **A vacuous vocabulary must not pass.** An empty set is a subset of every
    # ensemble, so `vocabulary_drift` would return "no drift" over a recording
    # that observed nothing at all — an instrument reporting a clean result
    # because it had stopped looking, which is the defect this whole directory
    # exists to catch. A recording with no observations is also not a reading
    # of the corpus, and `ci/t2-gameplay.sh` would have refused it upstream.
    if not v:
        raise NotATrace(
            f"{path} has an EMPTY vocabulary — no observation in any scenario "
            f"carries a field. A recording that saw nothing cannot certify that "
            f"a run's fields match an ensemble's."
        )
    return v


def fmt(v) -> list[str]:
    """`{(kind, field)}` -> sorted `["kind.field", ...]`, the on-disk form."""
    return sorted(f"{k}{SEP}{f}" for k, f in v)


def parse(names) -> set[tuple[str, str]]:
    """The on-disk form back to pairs. `split(SEP, 1)` — a kind holds no dot."""
    out = set()
    for n in names:
        k, _, f = str(n).partition(SEP)
        out.add((k, f))
    return out


def vocabulary_drift(run, ensemble) -> list[str]:
    """The pairs the RUN carries that the ENSEMBLE never produced.

    This is the whole asymmetry, in one line, in one place. The reverse
    difference is returned by `vocabulary_unused` and is not a warning.
    """
    return fmt(set(run) - set(ensemble))


def vocabulary_unused(run, ensemble) -> list[str]:
    """The ensemble's pairs this run did not produce — reported, never a red.

    `r1` of `twin-homog-20260809` lacks `take_item_entity.*` because vanilla
    did not pick the item up. That is the reference varying, not the recorder
    changing, and a check that fires on it is a check somebody turns off.
    """
    return fmt(set(ensemble) - set(run))


def main(argv) -> int:
    if len(argv) < 2:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    union: set[tuple[str, str]] = set()
    for path in argv[1:]:
        try:
            union |= read(path)
        except NotATrace as e:
            print(f"trace-vocabulary: {e}", file=sys.stderr)
            return 2
    print(json.dumps(fmt(union)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
