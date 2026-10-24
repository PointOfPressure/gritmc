#!/usr/bin/env python3
"""Aggregate `ci/t2-gameplay-selfcheck.sh`'s pairwise reports into a verdict.

    ci/selfcheck-aggregate.py <gameplay-unstable.toml> <report-2.json> ... <report-N.json>

Exit 0 when the measured unstable set equals the declared one, 1 when it does
not. `ci/t2-gameplay-selfcheck.sh` calls this; it is a separate file so that a
saved set of reports can be re-aggregated later without re-running two and a
half hours of servers.

# WHY THIS IS A FILE AND NOT A HEREDOC

It was a heredoc inside the selfcheck until 2026-08-04. Re-aggregating an old
run then meant pasting the block into `/tmp`, and on that day two published
figures came out of exactly such a copy: **a number in a tracked document has
to come from a tracked script, with the invocation in the commit message**,
or the next person cannot check it and the version that produced it is gone
with the next reboot.

# WHAT IT MEASURES, AND THE ONE THING IT DOES NOT GATE

`verdict` is the **declared** verdict — the scenario's own `observe` list and
timing rule. That is the gated axis, in both directions: a newly unstable check
is an instrument regression, and one that has become stable means the declared
file is stale.

`strict` is reported and **not** gated. It used to be unusable rather than
merely ungated: vanilla reproduced itself under strict on **0 of 55** at
N = 12, because `ServerEntity.sendChanges` re-broadcasts every tracked entity
on `tickCount % 60 == 0` whether or not it moved, every scenario has a second
player five blocks away, and no measured window here is three seconds long — so
the broadcast landed inside the window on a coin flip, under every check at
once. A gate that fires at random gets switched off. The column existed so that
*movement* of the strict set between runs of one tree is visible, which is what
nothing was watching when a strict corpus number moved 5/55 -> 7/55 on an
unchanged tree.

**That cause was removed on 2026-08-04 and the number moved to 35 of 60.**
`compare::project` now drops a `move_entity` that failed vanilla's own movement
test — `dx² + dy² + dz² < 128` in wire units, which is `lengthSqr < 2⁻¹⁷` in
blocks, which is the exact predicate `sendChanges` applies before deciding the
entity moved — unless a check names the kind. A packet below that threshold
cleared no movement test and is on the wire only because the counter wrapped.

**It is still not gated, and the reason has changed.** 35 of 60 is not a rate a
gate can be built on either, and what is left is characterised rather than
mysterious.

**The `set_entity_motion` residual was diagnosed on 2026-08-04 and it was not
what it looked like.** It was never an unstable predicate. `compare::near`
could not bound an entity observation — twenty recorded kinds carry no `at`
field, because vanilla puts the entity's id on them and not its position — so
a dropped item from one scenario appeared in every later scenario's window,
32 blocks away, for as long as it took to despawn. `compare::place` bounds
them by where the entity was announced, and quantity A moved **35 -> 37 of
60**: `coral-dries-out-unprompted` 11/11 -> 0 and `a-lever-lights-a-lamp`
7/11 -> 0, nothing broken. The registered floor was 42 and was missed, so the
leak is a real cause and not the dominant one. See `docs/WORKLIST.md`.

**One entry in this list is not a flap at all.**
`520-a-standing-player-is-rebroadcast-on-a-timer` has no strict content once
the leak is gone — its only observations are heartbeats and strict drops
those by design — so it is `vacuous` in every run and can never reach
`match`. This script counts any non-`match` as unstable and cannot tell a
permanently vacuous check from a flapping one, which costs the strict score a
fixed 1 for as long as that is true.

**Quote this number as quantity A — vanilla against itself — and never as
parity.** It says how reproducible the reference is, which is the ceiling any
score against it could reach, and nothing about this server at all.

# THE POWER LIMIT, WHICH IS A PROPERTY OF N AND NOT OF THIS SCRIPT

A check that flaps with probability p is missed by N runs with probability
`p^N + (1-p)^N`. At the ~1-in-11 rates this corpus produces at its floor, two
honest N = 12 runs can disagree about set membership by chance, and on
2026-08-04 two did: `a-bystander-sees-a-chest-open` went 2/11 -> 0/11 and
`chest-opens-a-screen` went 0/11 -> 1/11, in opposite directions, on the same
mechanism. **A `NO LONGER UNSTABLE` line at a low rate is a statement about N,
not proof of stability.** Read the rate printed beside every name before
concluding anything from membership.

# THE POWER TEST, ADDED 2026-08-04, AND THE ASYMMETRY THAT IS ITS WHOLE POINT

The paragraph above was advice to a human reader, and it was ignored four
times by the file it advises — `chest-opens-a-screen` went in and out of
`gameplay-unstable.toml` four times, every reading honest and correctly
reported, because a twelve-run design cannot resolve a 1-in-25 event and
nothing in this script said so in a way that could stop a commit.

So the rate is now **declared machine-readably** on every entry (`flaps` and
`comparisons`) and this script does the arithmetic the header used to ask a
human to do. When a declared entry is not seen in this run:

    P(miss) = (1 - flaps/comparisons) ** C          C = this run's comparisons

Above `ALPHA` the run had no power to see it and the absence means nothing:
print `UNRESOLVED AT THIS N` **with the arithmetic**, and **do not fail**. At
or below `ALPHA` the run did have the power, the absence is evidence, and it
is `NO LONGER UNSTABLE` and a failure exactly as before.

**`NEWLY UNSTABLE` is deliberately untouched, and that asymmetry is the
point.** A flap is positive evidence at any N whatsoever — one observed
disagreement between two runs of the reference is a fact about the reference,
and no amount of small-sample argument makes it not have happened. Only
*absence* is a claim about the sampling. Applying a power test to both
directions would be the mistake this fix exists to correct, wearing the
opposite costume.

**This is the point estimate, not a Clopper-Pearson bound**, and that is a
choice rather than an oversight. `flaps/comparisons` treats the declared rate
as exact when it is itself an estimate from `comparisons` samples, so for an
entry declared at 1/25 the true rate could be lower and the real miss
probability higher — the test is anti-conservative in exactly the direction
that matters. The conservative form replaces `flaps/comparisons` with the
lower end of a binomial interval at the same alpha. It is one line and it is
**not** here, because the point estimate is what removes the observed failure
and a refinement is worth making when it proves insufficient, not before. If a
`NO LONGER UNSTABLE` is ever cashed and turns out to have been a power
artefact, that is the evidence that says to swap the estimator, and this
paragraph is where the replacement goes.

**A missing or unusable rate fails closed.** An entry with no `flaps`, no
`comparisons`, a zero denominator or zero flaps gets the old behaviour —
`NO LONGER UNSTABLE`, exit 1 — with the reason printed. An exemption that
switches itself on when its own input is absent is the defect class this
whole file exists to catch.

## The exemption was checked against the rule it exists to serve

A new exemption in a gate is a new way for the gate to stop gating, so this
one was replayed the way `ci/t2-gameplay-selfcheck.sh`'s header replays the
aggregation itself — against the saved N = 26 reports of 2026-08-04, in
`oracle-work/gameplay-selfcheck`, with four declared lists:

    the committed file, reports 2..26 (N = 26)
        -> 26 runs, declared 7, measured 7, 54/61 reproduced.       exit 0

    the committed file, reports 2..12 (N = 12) — THE OBSERVED FAILURE
        -> UNRESOLVED AT THIS N chest-opens-a-screen (0.96^11 = 0.638)
           UNRESOLVED AT THIS N a-bystander-sees-a-chest-open (0.88^11 = 0.245)
           12 runs, declared 7, measured 5, 2 declared unresolved.   exit 0
           (before this change: two NO LONGER UNSTABLE lines,        exit 1)

    a stale name declared at a HIGH rate (break-block at 24/25), N = 12
        -> NO LONGER UNSTABLE break-block (0.04^11 = 4.19e-16).      exit 1

    a real one removed (water-spreads), N = 12
        -> NEWLY UNSTABLE water-spreads (diverge, 10/11).            exit 1

    chest-opens-a-screen with its `flaps`/`comparisons` deleted, N = 12
        -> NO LONGER UNSTABLE — NO POWER TEST POSSIBLE.              exit 1

So the exemption fires exactly where the run had no power, the staleness gate
still fires where it did, the `NEWLY UNSTABLE` direction is untouched, and an
entry that does not declare a rate cannot buy an exemption with its own
silence.
"""

import json
import sys
import tomllib
from collections import Counter

# The miss probability above which a declared entry's absence says nothing.
# 0.05 is the conventional threshold and nothing here depends on the exact
# value; what matters is that there is one and that the arithmetic is printed
# beside every verdict, so a reader can disagree with the number without
# having to re-derive the test.
ALPHA = 0.05


def miss_probability(declared: dict) -> tuple[float | None, str]:
    """P(a run of `declared['C']` comparisons sees no flap), and how it was got.

    Returns `(None, reason)` when the entry does not carry a usable rate, in
    which case the caller must fail rather than exempt.
    """
    flaps, comps, c = declared["flaps"], declared["comparisons"], declared["C"]
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
        f"declared {flaps}/{comps}, so {c} comparisons miss it with probability "
        f"(1 - {flaps}/{comps})^{c} = {1 - p:.6g}^{c} = {(1 - p) ** c:.3g}"
    )


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    declared_path, reports = argv[1], argv[2:]

    fails: Counter = Counter()
    strict_fails: Counter = Counter()
    verdicts: dict[str, str] = {}
    total = 0
    for path in reports:
        outcomes = json.load(open(path))["outcomes"]
        total = len(outcomes)
        for o in outcomes:
            if o["verdict"] != "match":
                fails[o["name"]] += 1
                verdicts.setdefault(o["name"], o["verdict"])
            if o["strict"] != "match":
                strict_fails[o["name"]] += 1

    measured = set(fails)
    with open(declared_path, "rb") as fh:
        loaded = tomllib.load(fh)
    entries = loaded["unstable"]
    # **`[[harness]]` is a second declared table and this file must know about
    # it, or it reports every entry in it as `NEWLY UNSTABLE`.** Added
    # 2026-08-09 with `ci/corpus-floor.py`, which splits the one question this
    # file used to ask — "does the reference reproduce this" — into two with
    # opposite answers: the reference *does* reproduce a `[[harness]]` entry,
    # and what does not reproduce is our sampling of it. Both still show up
    # here as a non-`match` verdict, because a comparison cannot tell them
    # apart; only the boundary-blind lens can, and that lives in the other
    # file. So they are reported under their own heading rather than
    # relabelled or hidden, and they still count against the reproduced total.
    harness = {h["name"] for h in loaded.get("harness", [])}
    rates = {
        u["name"]: {
            "flaps": u.get("flaps"),
            "comparisons": u.get("comparisons"),
            "C": len(reports),
        }
        for u in entries
    }
    # **Only checks this run actually recorded may be adjudicated.**
    #
    # `ci/t2-gameplay-selfcheck.sh` can now be pointed at a subset
    # (`SELFCHECK_CHECKS`), which is what lets a new check get a
    # reproducibility figure without re-recording eighty-six of them. Without
    # this line every declared-unstable check that was *not in the subset*
    # measured zero flaps, fell into `gone`, passed the power test trivially
    # — `(1 - 25/25)^24 = 0` — and was reported `NO LONGER UNSTABLE`. A
    # six-check run therefore declared five unrelated checks fixed and exited
    # 1.
    #
    # That is a false red produced by an instrument, which this project
    # treats as exactly as serious as a false green: the honest reaction to
    # `SELFCHECK FAILED` is to go and edit `gameplay-unstable.toml`, and
    # doing so on this evidence would have deleted five real entries nobody
    # had measured. Found 2026-08-06, on the first subset run there ever was.
    #
    # A check that was not run has produced no evidence in either direction,
    # so it is neither stale nor unresolved — it is absent, and absence is
    # reported rather than interpreted.
    present = {o["name"] for path in reports for o in json.load(open(path))["outcomes"]}
    unrun = sorted(set(rates) - present)
    declared = set(rates) & present
    if unrun:
        print(
            f"  NOT IN THIS RUN  {len(unrun)} declared-unstable check(s) were "
            f"not recorded and are not adjudicated: {', '.join(unrun[:6])}"
            + (" ..." if len(unrun) > 6 else "")
        )
    n = len(reports) + 1

    ours = sorted((measured & harness) & present)
    for name in ours:
        print(
            f"  declared harness  {name}  ({fails[name]}/{len(reports)} comparisons) — "
            f"OURS, not the reference's; see ci/checks/gameplay-unstable.toml [[harness]]"
        )
    measured -= harness
    new = sorted(measured - declared)
    gone = sorted(declared - measured)
    # A flap is positive evidence at any N. Untouched by the power test, on
    # purpose — see the header.
    for name in new:
        print(
            f"  NEWLY UNSTABLE  {name}  ({verdicts[name]}, "
            f"{fails[name]}/{len(reports)} comparisons)"
        )
    stale = []
    for name in gone:
        p_miss, how = miss_probability(rates[name])
        if p_miss is not None and p_miss > ALPHA:
            print(
                f"  UNRESOLVED AT THIS N  {name} — {how} > alpha = {ALPHA}; "
                f"this run could not have seen it, so its absence is not evidence"
            )
            continue
        stale.append(name)
        detail = (
            f"{how} <= alpha = {ALPHA}; this run had the power to see it"
            if p_miss is not None
            else f"NO POWER TEST POSSIBLE — {how}"
        )
        print(f"  NO LONGER UNSTABLE  {name} — {detail}; gameplay-unstable.toml is stale")
    for name in sorted(measured & declared):
        print(f"  declared unstable  {name}  ({fails[name]}/{len(reports)} comparisons)")
    unresolved = len(gone) - len(stale)
    # `- len(ours)` and not just `- len(measured)`. A `[[harness]]` name was
    # removed from `measured` above so it is not adjudicated as the
    # reference's, and subtracting only `measured` would then count it among
    # the checks that **reproduced** — which is the opposite of what happened
    # and would inflate the one number this script exists to publish.
    print(
        f"  {n} runs, declared {len(declared)}, measured {len(measured)}, "
        f"{total - len(measured) - len(ours)}/{total} reproduced"
        + (f", {len(ours)} did not and are ours" if ours else "")
        + (f", {unresolved} declared unresolved at this N" if unresolved else "")
    )

    print("== strict reproducibility, reported and not gated")
    for name, c in sorted(strict_fails.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"  strict-unstable  {name}  ({c}/{len(reports)} comparisons)")
    print(f"  {n} runs, strict reproduced {total - len(strict_fails)}/{total}")

    if new or stale:
        print(
            "SELFCHECK FAILED: the reference's own reproducibility moved",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
