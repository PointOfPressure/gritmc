#!/usr/bin/env python3
"""One composite parity figure, its unit spread, and the refusals that gate it.

    ci/parity-aggregate.py --census <census.json> --report <report.json> \
        --floor ci/checks/gameplay-floor.toml --binary <oracle-runner>

`docs/PARITY-ROADMAP.md` 21 built the denominators and then deliberately
refused to collapse them, because the answer moves **55x** between units and
the spread is the finding. That refusal was right and it is not sufficient any
more: the project is asked "how far from finished", a number will be quoted
with or without us, and an unowned number is worse than a stated one.

So this composes, and it states what the composition cost.

    parity  =  reach  x  match

* **reach** — what fraction of vanilla's *reachable* surface the corpus
  provokes at all, per unit, from `ci/vanilla-reach-census.py --json`.
* **match** — of the corpus, what fraction this server reproduces, from
  `oracle-runner compare --json` at its own base and projection.

NOTHING HERE IS A NEW MEASUREMENT. Both halves are already measured; this is
composition only, and composition is where the errors are. Every one of the
following is a mistake this file exists to not make.


# 1. REACH IS AGAINST THE CEILING, NOT THE REGISTRY

`21.1` divided by the registry — 1,968 sound events, 1,196 blocks, 32,366
block states — and the resulting 1.47% is not wrong, it is a fraction of a
number nobody can reach. `ci/vanilla-reach-census.py` grew a `ceiling` per row
on 2026-08-12 (ROADMAP 29): the sound row is 256 reachable against 1,968
registered, so `29/1968` and `29/256` are both true and differ by 7.7x.

**This script divides by the ceiling and refuses a row that has none.** Three
registry rows — `block`, `block_state`, `entity_type` — carry
`ceiling = null` because no jar rule was found to cap them, and a row whose
denominator is unknown cannot enter a composite at any weight. They are
printed as excluded, with their registry gap, so that their absence is visible
rather than silent.

Measured cost of the correction, on this ensemble: the spread across the
surviving units falls from **17.3x** (registry) to **5.6x** (ceiling). The
spread survives; what changes is which rows are catastrophic.

**A ceiling has an owner and the owner changes what the gap means.** A `jar`
ceiling is a fact about vanilla and is nobody's to spend money on. An `ours`
ceiling is a restriction this harness imposes — superflat, peaceful, creative,
`spawn_mobs=false` — and is therefore spendable work. The owner is carried
through to the output and into the lever ranking, because "19 block entity
types to go" and "227 sound events to go" are not the same kind of debt.


# 2. THE ESTIMATOR IS THE MEDIAN, AND THAT IS DERIVED RATHER THAN ASSERTED

A mean over `{80.56%, 3.60%, 0.27%}` is arithmetic, not information. But so is
a median, if it is chosen because it looks moderate. Three estimators are
computed and all three are printed; the headline is the median because it wins
a test that is run every time this script runs.

The test is a drop-one jackknife: recompute with each unit removed in turn,
and report how far the answer swings as a fraction of itself. On the
2026-08-12 inputs:

    median   10.16%    8.74.. 11.58%    27.9% of value
    mean     11.43%    8.06.. 13.95%    51.6% of value
    pooled   12.53%    8.60.. 18.27%    77.1% of value

The pooled sum is the worst and the reason is worth stating, because it is the
one a reader will reach for as "the obvious total": it sums packet types to
menus to sound events, and its weights are then the sizes of Mojang's
registries. `sound_event`'s ceiling of 256 is **60.2%** of the pooled weight —
not because sound is 60% of the game, but because that registry is large. Swap
that one row's ceiling for its registry size and pooled moves 12.53% -> 2.49%
while the median moves 10.16% -> 7.72%. A statistic that a single denominator
can move 5x is measuring the denominator.

The median is not thereby *right*. It is the least sensitive of the three to
the arbitrary part, which is the only property any of them can be argued to
have, and the two alternatives are printed beside it so a reader can see what
the choice cost.


# 3. PRESENCE IS NOT COVERAGE, AND THE DISCOUNT IS PRINTED

`21.4`: a packet observed once is not a mechanic checked. Every reach
numerator here is *presence* — vanilla did this thing during the corpus — and
`reach x match` silently credits us for reproducing surface that no check even
reads.

The size of that credit is measured, on the one unit where it can be: of the
36 clientbound types vanilla emitted, **29** are named by at least one check.
So a third factor exists,

    parity  =  reach  x  read  x  match

and it is printed beside the two-factor answer. `read` is measured on UNIT 1
only; extending it to the sound, menu and block-entity rows is an assumption
and is labelled as one. The honest reading is the pair: the two-factor number
is an upper bound on what this composition can claim, the three-factor number
is what it can claim if the packet unit's read-rate generalises, and the
difference between them is the price of 21.4.


# 4. THE HALVES MUST HAVE MEASURED THE SAME VANILLA

`reach` is measured over an ensemble of vanilla recordings. `match`'s floor is
measured over an ensemble of vanilla recordings. If those are different
ensembles the product is two numbers about two worlds, and nothing in either
artefact says so — the census names its recordings by path, the floor names
them by sha256.

So this hashes the census's recordings and requires the multiset to equal the
floor's. It is the cheapest arm here and the one most likely to be the reason
a future composite is wrong, because both halves will look individually fine.


# 5. A FIGURE THAT KEEPS PRINTING THROUGH A PROJECTION CHANGE IS NOT A GATE

15 and 20 are both about this. `match` is a count under a comparator; the
floor beside it is a count under a comparator; `oracle-runner compare --json`
records **neither**. So the projection identity cannot be recovered from the
score artefact and must be supplied — from the binary that produced it, not
from the tree standing here, which is the half that survives `rsync -a`
leaving sources older than the artefacts built from them.

`--binary` derives it and additionally refuses a binary *newer* than the
report it is being asked to vouch for, which is the only cheap evidence that
the two belong together.

Measured 2026-08-12: `~/rustmc-stack`'s 81/93 is the newest and best-looking
corpus score on the box and it is exactly the one that cannot be quoted — its
comparator moved to `proj:f7fbb29926074663` and the standing floor was never
re-measured under it. The highest score coherent with the floor is
`~/rustmc-pot`'s 71/92 strict. A composite that quoted the bigger number would
have been wrong in the flattering direction, silently, and this is the arm
that stops it.


# 6. THREE EXIT STATES, BECAUSE "STALE" AND "INCOHERENT" ARE DIFFERENT

    0   the figure is current: every denominator agrees with the live tree.
    3   the figure is QUOTABLE AS OF its own stamp but is NOT current — the
        corpus has moved under it. The number is still printed, loudly marked,
        because a figure taken at a stated state is what every worldgen
        exactness in this repository already is. A window is not staleness.
    2   REFUSED. No number is printed at all.

The distinction matters because collapsing them produces one of two failures
and there is no third option: refuse on drift and the instrument prints
nothing on almost every day, so nobody runs it; ignore drift and it prints a
figure describing a corpus that no longer exists.


# WHAT THIS IS NOT

**It is not a worldgen number.** Worldgen is a separate axis with its own
history, and it is refused here rather than omitted — see `worldgen_row()`.
The refusal is structural: worldgen has *match* figures (99.935277% at seed
12345) and no *reach* denominator whatever. Nobody has censused what fraction
of vanilla's feature, structure and carver surface a 121-chunk window
provokes, so there is no `reach` to multiply and the product is undefined.
Folding an exactness-over-a-window into a gameplay reach would be the same
category error as 21.1's registry denominators, one level up.

**It is not a claim that a system is tested.** Every positive tier under it is
presence. See 3 above and 21.4.

**It has no floor of its own, and does not pretend to.** `match` carries the
standing corpus floor, which is printed as a band and never subtracted. `reach`
is a census rather than a comparison, so ORACLE rule 6 has no premise to fire
on; what stands in its place is the census's own per-run reproducibility
spread, propagated here into the low band. `ci/corpus-floor.py` and 21.4 are
the two headers that argue this.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import statistics
import subprocess
import sys
import tempfile

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - 3.10 and older
    import tomli as tomllib  # type: ignore[no-redef]

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FLOOR = os.path.join(ROOT, "ci", "checks", "gameplay-floor.toml")
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")

PROJ_RE = re.compile(r"^proj:[0-9a-f]{16}$")

# Every key `ci/corpus-floor-report.py` calls NEEDED, plus `projection`.
# A floor missing any of them is not a weaker floor, it is not one: `runs`,
# `alpha` and `p_hide` are the resolving power, and a bound quoted without its
# resolving power is the defect 21.4 and DETERMINISM 5 are both about.
FLOOR_KEYS = (
    "floor", "floor_reference", "floor_ours", "scenarios", "ensemble",
    "runs", "pairs", "p_hide", "alpha", "projection",
)

EXIT_OK = 0
EXIT_REFUSED = 2
EXIT_NOT_CURRENT = 3


class Refused(Exception):
    """One refusal arm, with the arm's tag so the output is greppable."""

    def __init__(self, arm: str, detail: str) -> None:
        super().__init__(detail)
        self.arm = arm


# --- inputs -----------------------------------------------------------------


def load_floor(path):
    """The standing corpus floor. ARM `NO-FLOOR`."""
    try:
        with open(path, "rb") as fh:
            doc = tomllib.load(fh)
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise Refused("NO-FLOOR", f"{path}: {e}") from e
    if "floor" not in doc:
        raise Refused("NO-FLOOR", f"{path} has no [floor] table")
    f = doc["floor"]
    missing = [k for k in FLOOR_KEYS if k not in f]
    if missing:
        raise Refused(
            "NO-FLOOR",
            f"{path} [floor] is missing {', '.join(missing)} — a floor without its "
            f"N and its resolving power is not a floor",
        )
    if f["runs"] < 2:
        raise Refused(
            "NO-FLOOR",
            f"{path} was taken over {f['runs']} run(s); a floor is a disagreement "
            f"between two innocent runs and needs at least two",
        )
    f = dict(f)
    f["recordings"] = [r["sha256"] for r in doc.get("recording", [])]
    return f


def load_report(path):
    """`oracle-runner compare --json`. Carries no projection: see 5 above."""
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError) as e:
        raise Refused("NO-REPORT", f"{path}: {e}") from e
    for basis in ("declared", "strict"):
        if basis not in doc or "total" not in doc.get(basis, {}):
            raise Refused("NO-REPORT", f"{path} has no [{basis}] counts — not a compare report")
    return doc


def load_census(path):
    """`ci/vanilla-reach-census.py --json`. ARM `NO-CEILINGS`."""
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError) as e:
        raise Refused("NO-CENSUS", f"{path}: {e}") from e
    for k in ("protocol", "registry", "recordings"):
        if k not in doc:
            raise Refused("NO-CENSUS", f"{path} has no `{k}` — not a reach census")
    has = "ceiling" in doc["protocol"] or any("ceiling" in v for v in doc["registry"].values())
    if not has:
        raise Refused(
            "NO-CEILINGS",
            f"{path} reports registry sizes, not reachable ceilings. Every row's "
            f"denominator would be a number nobody can reach — see 1 above and "
            f"ROADMAP 29. Re-run ci/vanilla-reach-census.py from a tree whose census "
            f"emits `ceiling`.",
        )
    return doc


def projection_of(binary, report_path):
    """Derive the comparator identity from the binary, and prove it is the right one.

    ARM `BINARY-NEWER-THAN-REPORT`. A binary rebuilt after the report it is
    vouching for answers about a comparator the report never ran under, and
    nothing in either file would say so.
    """
    try:
        out = subprocess.run([binary, "projection-id"], capture_output=True,
                             text=True, check=False).stdout.strip()
    except OSError as e:
        raise Refused("NO-PROJECTION", f"{binary}: {e}") from e
    if not out:
        raise Refused(
            "NO-PROJECTION",
            f"{binary} answered nothing to `projection-id` — a binary too old to "
            f"state its own comparator is too old to be trusted about the rest",
        )
    try:
        b_m, r_m = os.path.getmtime(binary), os.path.getmtime(report_path)
    except OSError as e:
        raise Refused("NO-PROJECTION", f"{e}") from e
    if b_m > r_m:
        raise Refused(
            "BINARY-NEWER-THAN-REPORT",
            f"{binary} was built after {report_path} was written "
            f"({_ts(b_m)} > {_ts(r_m)}); it cannot state the comparator that "
            f"report ran under. Pass --projection explicitly if you know it.",
        )
    return out


def _ts(t):
    import datetime  # noqa: PLC0415

    return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# --- the arms ---------------------------------------------------------------


def check_projection(projection, floor):
    """ARMS `PROJECTION-MALFORMED`, `STALE-PROJECTION`."""
    if not PROJ_RE.match(projection or ""):
        raise Refused(
            "PROJECTION-MALFORMED",
            f"{projection!r} is not proj:<16 hex>. The empty string must not compare "
            f"equal to a missing stamp.",
        )
    if projection != floor["projection"]:
        raise Refused(
            "STALE-PROJECTION",
            f"the floor was measured under {floor['projection']} and this score was "
            f"compared under {projection}. `match` and its floor then count what two "
            f"DIFFERENT comparators could not reproduce, and their ratio is not a "
            f"number. Re-measure: ci/corpus-floor.sh --update; diagnose: "
            f"oracle-runner projection-id --explain",
        )


def check_denominator(floor, report):
    """ARM `DENOMINATOR-MISMATCH` — internal coherence of the match half."""
    fs, dt, st = floor["scenarios"], report["declared"]["total"], report["strict"]["total"]
    if not fs == dt == st:
        raise Refused(
            "DENOMINATOR-MISMATCH",
            f"the floor is over {fs} scenarios, the report declares {dt} and scores "
            f"{st} strict. `match` and its floor must share a denominator or the "
            f"floor is an annotation on a different corpus.",
        )


def check_ensemble(census, floor):
    """ARM `ENSEMBLE-DRIFT` — the two halves must have measured the same vanilla.

    The census names its recordings by path and the floor names them by
    sha256, so neither artefact can see the other. Hashing is the join.
    """
    if not floor["recordings"]:
        raise Refused(
            "ENSEMBLE-DRIFT",
            "the floor lists no [[recording]] tables, so there is nothing to join "
            "the reach half to. A floor that cannot name its recordings cannot be "
            "shown to be about the same vanilla as the census.",
        )
    got = []
    for p in census["recordings"]:
        try:
            with open(p, "rb") as fh:
                got.append(hashlib.sha256(fh.read()).hexdigest())
        except OSError as e:
            raise Refused(
                "ENSEMBLE-DRIFT",
                f"the census names {p} and it cannot be read ({e}), so the two halves "
                f"cannot be shown to be about the same vanilla",
            ) from e
    if sorted(got) != sorted(floor["recordings"]):
        extra = sorted(set(got) - set(floor["recordings"]))
        gone = sorted(set(floor["recordings"]) - set(got))
        raise Refused(
            "ENSEMBLE-DRIFT",
            f"reach was censused over {len(got)} recording(s) and the floor was "
            f"measured over {len(floor['recordings'])}, and they are not the same "
            f"vanilla: {len(extra)} only in the census, {len(gone)} only in the "
            f"floor. The product would be two numbers about two worlds.",
        )


# --- the units --------------------------------------------------------------


class Unit:
    """One lens on 'how much of vanilla does the corpus touch'."""

    def __init__(self, name, observed, ceiling, registry, owner, rule, per_run=None):
        self.name, self.observed, self.ceiling = name, observed, ceiling
        self.registry, self.owner, self.rule, self.per_run = registry, owner, rule, per_run

    @property
    def reach(self):
        return self.observed / self.ceiling

    @property
    def reach_registry(self):
        return self.observed / self.registry

    @property
    def reach_low(self):
        """The census's per-run spread, standing in for a floor it cannot have.

        **A unit with no per-run reading falls back to its pooled value, which
        asserts a reproducibility it did not measure.** The census reports a
        per-run spread for the registry rows only; the packet unit has none, so
        its contribution to the low band is its own point estimate. That makes
        the low band an *under*-statement of the uncertainty rather than an
        over-statement, which is the wrong direction to be silent in — so
        `has_spread` exists and the caller prints which rows lack one.
        """
        return (min(self.per_run) / self.ceiling) if self.per_run else self.reach

    @property
    def has_spread(self):
        return bool(self.per_run)


def units_of(census):
    """Split the census into what can enter a composite and what cannot.

    Returns (live, excluded). An exclusion is printed with its reason; a row
    that is silently dropped is a row a reader will assume was counted.
    """
    live, excluded = [], []
    p = census["protocol"]
    if p.get("ceiling"):
        live.append(Unit("clientbound packet", p["emitted"], p["ceiling"], p["denominator"],
                         p.get("ceiling_owner", "unknown"),
                         "types an unmodified server can construct, minus this harness's "
                         "housekeeping drops"))
    else:
        excluded.append(("clientbound packet", "UNKNOWN CEILING", f"{p['emitted']} of "
                         f"{p['denominator']} registered"))

    # UNIT 2 has a ceiling and NO numerator, which is the opposite failure and
    # must not be confused with it: nothing here booted a server, so the count
    # of sections that RAN is not measured. 21.4's rule is that `unclassified`
    # means unknown and must never be read as covered — so it cannot be a
    # numerator either, in either direction.
    t = census.get("tick", {})
    if t:
        unc = t.get("unclassified")
        n_unc = len(unc) if isinstance(unc, (list, tuple)) else unc
        excluded.append((
            "tick section",
            "NO NUMERATOR",
            f"ceiling {t.get('ceiling', '?')} of {t.get('sections', '?')} is known, but no "
            f"server is booted by this census, so what fraction of it RAN is not measured. "
            f"{n_unc} sections are unclassified, which is unknown and is not evidence in "
            f"either direction (21.4).",
        ))

    per_run = census.get("per_run", {})
    for key in sorted(census["registry"]):
        row = census["registry"][key]
        if row.get("ceiling"):
            live.append(Unit(key, row["touched"], row["ceiling"], row["denominator"],
                             row.get("ceiling_owner", "unknown"),
                             row.get("ceiling_rule", ""), per_run.get(key)))
        else:
            excluded.append((key, "UNKNOWN CEILING",
                             f"{row['touched']} of {row['denominator']} registered; "
                             f"{row.get('ceiling_rule') or 'no ceiling rule'}"))
    return live, excluded


def worldgen_row():
    """Worldgen is refused here, structurally, rather than omitted.

    It has `match` figures and no `reach` denominator: nobody has censused
    what fraction of vanilla's feature, structure and carver surface a window
    provokes. `reach x match` is therefore undefined, not small.

    This is a function rather than a comment so that the refusal appears in
    the output and in the JSON. A reader who is told nothing about worldgen
    concludes the headline covers it.
    """
    return ("worldgen", "NO REACH DENOMINATOR",
            "worldgen carries match figures (DETERMINISM 17: 99.935277% at seed 12345, "
            "floor 2,577..4,835 at N=5) and no reach census exists for it at all. There "
            "is no denominator to divide by, so the product is undefined rather than "
            "small. It is a separate axis and this headline does not cover it.")


# --- the composition --------------------------------------------------------


def estimators(live, factor):
    """median / mean / pooled, all three, every time.

    `pooled` is included precisely because it is the one a reader reaches for
    and the one the jackknife kills; printing only the winner would hide the
    argument that chose it.
    """
    vals = [u.reach * factor for u in live]
    pooled = sum(u.observed for u in live) / sum(u.ceiling for u in live) * factor
    return {
        "median": statistics.median(vals),
        "mean": statistics.fmean(vals),
        "pooled": pooled,
    }


def jackknife(live, factor):
    """Drop each unit in turn. The swing, as a fraction of the value, chooses.

    This is the whole of the argument for the median in 2 above, and it is
    recomputed on every run rather than quoted, so a future ensemble that
    makes a different estimator the stable one will say so.
    """
    base = estimators(live, factor)
    out = {}
    for name in base:
        vals = [estimators(live[:i] + live[i + 1:], factor)[name] for i in range(len(live))]
        lo, hi = min(vals), max(vals)
        out[name] = (lo, hi, (hi - lo) / base[name] if base[name] else float("inf"))
    return base, out


def levers(live, factor, estimator="median"):
    """The ranked work order, derived from the composition rather than asserted.

    Two quantities, because they rank differently and a reader needs both:

    * **marginal** — how far the headline moves per ONE more item of that
      unit's surface observed. This is the derivative, evaluated by
      perturbation rather than by algebra so that it stays correct if the
      estimator changes.
    * **full** — where the headline lands if that unit's whole gap closes.

    They disagree, and the disagreement is the finding. Under a median the
    marginal lever of the *extreme* units is exactly ZERO: a median is
    insensitive to its ends by construction, so the row with the biggest gap
    can be the row where the next hour of work moves the headline not at all.
    That is a real cost of choosing the median in 2 above and it is printed
    rather than buried — `--basis` cannot hide it and neither should this.

    It is also why `full` is printed beside it: `full` is what a reader means
    by "what would move it most", and `marginal` is what they should spend the
    next hour on.
    """
    def score(rows):
        vals = [u.reach * factor for u in rows]
        if estimator == "median":
            return statistics.median(vals)
        if estimator == "mean":
            return statistics.fmean(vals)
        return sum(u.observed for u in rows) / sum(u.ceiling for u in rows) * factor

    def perturb(i, observed):
        return [Unit(v.name, observed if j == i else v.observed, v.ceiling, v.registry,
                     v.owner, v.rule, v.per_run) for j, v in enumerate(live)]

    base = score(live)
    rows = []
    for i, u in enumerate(live):
        gap = u.ceiling - u.observed
        rows.append({
            "unit": u.name,
            "gap": gap,
            "owner": u.owner,
            "marginal": score(perturb(i, u.observed + 1)) - base if gap else 0.0,
            "full": score(perturb(i, u.ceiling)),
        })
    rows.sort(key=lambda r: (-r["marginal"], -r["full"], r["unit"]))
    return base, rows


# --- output -----------------------------------------------------------------


def emit(census, floor, report, projection, basis, live, excluded, checks_n, out=sys.stdout):
    w = out.write
    m, tot = report[basis]["matched"], report[basis]["total"]
    match = m / tot
    read = census["protocol"]["both"] / census["protocol"]["emitted"]
    fl = floor["floor"]

    current = checks_n is None or checks_n == floor["scenarios"]

    w("== inputs, and the stamp this figure carries ==\n")
    w(f"  jar          {census['jar']}\n")
    w(f"  ensemble     {floor['ensemble']}, N = {floor['runs']} recordings, "
      f"{floor['pairs']} pairwise\n")
    w(f"  projection   {projection}  (floor and score, verified equal)\n")
    w(f"  match        {basis} {m}/{tot} = {100 * match:.2f}%\n")
    w(f"  floor        {fl}/{floor['scenarios']} scenarios two vanilla runs do not "
      f"reproduce ({floor['floor_reference']} vanilla's, {floor['floor_ours']} the harness's)\n")
    w(f"  LOWER BOUND  at N = {floor['runs']} a scenario flapping less than "
      f"{floor['p_hide'] * 100:.1f}% of the time still shows {floor['runs']} identical "
      f"runs (alpha {floor['alpha']})\n")
    if not current:
        w(f"  NOT CURRENT  the live corpus holds {checks_n} scenarios and this figure is "
          f"stamped at {floor['scenarios']}\n")

    w("\n== reach, per unit, against the CEILING and against the registry ==\n")
    w(f"  {'unit':22s} {'observed':>8s} {'ceiling':>8s} {'reach':>8s} {'owner':>9s}  "
      f"registry\n")
    for u in live:
        w(f"  {u.name:22s} {u.observed:8d} {u.ceiling:8d} {100 * u.reach:7.2f}% "
          f"{u.owner:>9s}  {u.observed}/{u.registry} = {100 * u.reach_registry:.2f}%\n")
    for name, why, detail in excluded + [worldgen_row()]:
        w(f"  {name:22s} EXCLUDED — {why}\n        {detail}\n")

    w("\n== the composite ==\n")
    base, jk = jackknife(live, match)
    w(f"  parity = reach x match, over {len(live)} units with a known ceiling\n\n")
    w(f"  {'estimator':10s} {'value':>8s}   {'drop-one range':>18s}   swing\n")
    for name in ("median", "mean", "pooled"):
        lo, hi, sw = jk[name]
        star = " <-- headline" if name == "median" else ""
        w(f"  {name:10s} {100 * base[name]:7.2f}%   {100 * lo:7.2f}..{100 * hi:7.2f}%   "
          f"{100 * sw:5.1f}% of value{star}\n")

    vals = sorted(u.reach * match for u in live)
    w(f"\n  SPREAD       {100 * vals[0]:.2f}% .. {100 * vals[-1]:.2f}%  "
      f"({vals[-1] / vals[0]:.1f}x across units)\n")
    reg = sorted(u.reach_registry * match for u in live)
    w(f"  same, vs the REGISTRY instead of the ceiling: "
      f"{100 * reg[0]:.2f}% .. {100 * reg[-1]:.2f}% ({reg[-1] / reg[0]:.1f}x)\n")
    w(f"  -> the ceiling correction takes the spread {reg[-1] / reg[0]:.1f}x -> "
      f"{vals[-1] / vals[0]:.1f}x. It does not remove it.\n")

    lo_band = statistics.median([u.reach_low for u in live]) * match
    hi_band = statistics.median([u.reach for u in live]) * ((m + fl) / tot)
    w(f"\n  BAND         {100 * lo_band:.2f}% .. {100 * hi_band:.2f}%\n")
    w(f"    low  = reach at the census's per-run minimum (its only stand-in for a "
      f"floor, 21.4)\n")
    nospread = [u.name for u in live if not u.has_spread]
    if nospread:
        w(f"           {', '.join(nospread)} report NO per-run spread, so they enter the "
          f"low band\n           at their pooled value. The low band is therefore an "
          f"UNDER-statement of the\n           uncertainty, not an over-statement.\n")
    w(f"    high = match with the floor's {fl} unreproducible scenarios admitted "
      f"({m}+{fl})/{tot} — an annotation on the denominator, never subtracted\n")

    three = estimators(live, read * match)["median"]
    w(f"\n  PRESENCE DISCOUNT (21.4)\n")
    w(f"    two-factor   reach x match                 {100 * base['median']:.2f}%\n")
    w(f"    three-factor reach x read x match          {100 * three:.2f}%   "
      f"read = {census['protocol']['both']}/{census['protocol']['emitted']} = "
      f"{100 * read:.2f}%\n")
    w(f"    `read` is measured on the packet unit ONLY; extending it to the other "
      f"units is an assumption.\n")
    w(f"    The honest reading is the pair: {100 * three:.1f}% .. {100 * base['median']:.1f}%.\n")

    w("\n== ranked levers — what would move the composite most ==\n")
    lbase, rows = levers(live, match)
    w(f"  headline {100 * lbase:.2f}%; ranked by the MARGINAL move, per one more item "
      f"observed\n")
    w(f"  {'unit':22s} {'gap':>6s} {'owner':>9s} {'per item':>10s} {'if closed':>10s}\n")
    for r in rows:
        w(f"  {r['unit']:22s} {r['gap']:6d} {r['owner']:>9s} "
          f"{100 * r['marginal']:+9.3f}% {100 * r['full']:9.2f}%\n")
    pbase, prows = levers(live, match, estimator="pooled")
    w(f"  under `pooled` instead the order is "
      f"{' > '.join(r['unit'] for r in prows)} — the estimators disagree\n")
    w("  an `ours` owner is spendable work; a `jar` owner is a fact about vanilla.\n")
    zero = [r["unit"] for r in rows if r["marginal"] == 0 and r["gap"]]
    if zero:
        w(f"  MARGINAL ZERO: {', '.join(zero)} — a median is insensitive to its ends by\n"
          f"  construction, so these rows have open gaps that the next item of work does\n"
          f"  not move at all. That is a cost of the estimator, not a fact about them.\n")
    w("  rows with no ceiling are absent from this ranking BECAUSE they have no "
      "ceiling,\n  not because they are small — `block` alone is 1,153 untouched "
      "blocks (21.5 ranks it 1st).\n")

    verdict = "CURRENT" if current else "AS OF"
    w(f"\nPARITY-AGGREGATE {verdict}  {basis} {100 * base['median']:.2f}%  "
      f"band {100 * lo_band:.2f}..{100 * hi_band:.2f}%  "
      f"spread {100 * vals[0]:.2f}..{100 * vals[-1]:.2f}%  "
      f"units {len(live)}  ensemble {floor['ensemble']}  {projection}\n")
    return base, jk, lo_band, hi_band, three, current


# --- main -------------------------------------------------------------------


def run(args, out=sys.stdout):
    floor = load_floor(args.floor)
    report = load_report(args.report)
    census = load_census(args.census)

    projection = args.projection
    if args.binary:
        projection = projection_of(args.binary, args.report)
    check_projection(projection, floor)
    check_denominator(floor, report)
    check_ensemble(census, floor)

    live, excluded = units_of(census)
    if len(live) < 3:
        raise Refused(
            "TOO-FEW-UNITS",
            f"{len(live)} unit(s) have both a numerator and a known ceiling. A median "
            f"over two is an average and a median over one is that one — neither is a "
            f"composite. Excluded: "
            + "; ".join(f"{n} ({w})" for n, w, _ in excluded),
        )

    checks_n = None
    if args.checks_dir:
        try:
            checks_n = sum(1 for f in os.listdir(args.checks_dir) if f.endswith(".toml"))
        except OSError:
            checks_n = None

    base, jk, lo, hi, three, current = emit(
        census, floor, report, projection, args.basis, live, excluded, checks_n, out)

    if args.json:
        m, tot = report[args.basis]["matched"], report[args.basis]["total"]
        with open(args.json, "w") as fh:
            json.dump({
                "headline": base["median"],
                "estimator": "median",
                "basis": args.basis,
                "band": [lo, hi],
                "three_factor": three,
                "estimators": base,
                "jackknife": {k: list(v) for k, v in jk.items()},
                "spread": sorted(u.reach * (m / tot) for u in live),
                "units": [{"unit": u.name, "observed": u.observed, "ceiling": u.ceiling,
                           "registry": u.registry, "owner": u.owner, "reach": u.reach}
                          for u in live],
                "excluded": [{"unit": n, "why": w, "detail": d}
                             for n, w, d in excluded + [worldgen_row()]],
                "ensemble": floor["ensemble"],
                "projection": floor["projection"],
                "scenarios": floor["scenarios"],
                "match": {"matched": m, "total": tot},
                "floor": floor["floor"],
                "current": current,
                "jar": census["jar"],
            }, fh, indent=1, sort_keys=True)
        out.write(f"wrote {args.json}\n")

    return EXIT_OK if current else EXIT_NOT_CURRENT


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--census", help="ci/vanilla-reach-census.py --json output")
    ap.add_argument("--report", help="oracle-runner compare --json output")
    ap.add_argument("--floor", default=FLOOR)
    ap.add_argument("--projection", help="proj:<16 hex>, from the binary that made --report")
    ap.add_argument("--binary", help="derive --projection from this oracle-runner, and "
                                     "refuse it if it is newer than --report")
    ap.add_argument("--checks-dir", default=CHECKS,
                    help="the live corpus, for the currency check; '' to skip")
    ap.add_argument("--basis", choices=("strict", "declared"), default="strict")
    ap.add_argument("--json", help="write the composite as JSON to this path")
    ap.add_argument("--selftest", action="store_true", help="fire every refusal arm; then exit")
    args = ap.parse_args(argv)

    if args.selftest:
        return selftest()
    if not args.census or not args.report:
        ap.error("--census and --report are required")
    if not args.projection and not args.binary:
        ap.error("one of --projection or --binary is required: `oracle-runner compare` "
                 "records no comparator identity, so it cannot be recovered from --report")
    try:
        return run(args)
    except Refused as e:
        print(f"parity-aggregate: REFUSED [{e.arm}] {e}", file=sys.stderr)
        return EXIT_REFUSED


# --- the falsifier ----------------------------------------------------------
#
# "A gate that cannot fail is not a gate." Every arm above is fed an input
# that should trip it, and the run fails if one comes back green. The GREEN
# control is run first and last: an arm that fires on everything is as useless
# as one that fires on nothing, and a suite that only ever proves refusals
# cannot tell the two apart.


def _fixture(td, **over):
    """A minimally coherent input set, on disk, that the arms should pass."""
    recs = []
    for i in range(6):
        p = os.path.join(td, f"r{i + 1}.json")
        with open(p, "w") as fh:
            fh.write(json.dumps({"recorder": "x", "scenarios": [], "n": i}))
        recs.append(p)
    shas = [hashlib.sha256(open(p, "rb").read()).hexdigest() for p in recs]

    census = {
        "jar": "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar",
        "recordings": recs, "scenarios": 93,
        "protocol": {"denominator": 141, "emitted": 36, "observed": 31, "both": 29,
                     "ceiling": 129, "ceiling_owner": "ours"},
        "tick": {"sections": 120, "ceiling": 79, "unclassified": 79},
        "registry": {
            "sound_event": {"touched": 29, "denominator": 1968, "ceiling": 256,
                            "ceiling_owner": "ours", "ceiling_rule": "r"},
            "menu": {"touched": 3, "denominator": 25, "ceiling": 20,
                     "ceiling_owner": "jar+ours", "ceiling_rule": "r"},
            "block_entity_type": {"touched": 1, "denominator": 49, "ceiling": 20,
                                  "ceiling_owner": "jar", "ceiling_rule": "r"},
            "block": {"touched": 43, "denominator": 1196, "ceiling": None,
                      "ceiling_owner": "unknown", "ceiling_rule": "no jar rule"},
        },
        "per_run": {"sound_event": [27, 28, 27, 27, 27, 27], "menu": [3] * 6,
                    "block_entity_type": [1] * 6},
    }
    census.update(over.get("census", {}))
    cp = os.path.join(td, "census.json")
    json.dump(census, open(cp, "w"))

    counts = {"total": 92, "matched": 71, "diverged": 0, "absent": 0, "vacuous": 0,
              "mute": 0, "inert": 0, "errored": 0}
    report = {"declared": dict(counts, matched=75), "strict": dict(counts), "outcomes": []}
    report.update(over.get("report", {}))
    rp = os.path.join(td, "report.json")
    json.dump(report, open(rp, "w"))

    floor = {
        "ensemble": "fixture-n6", "projection": "proj:7cefde0fb56d9f3e", "runs": 6,
        "pairs": 15, "scenarios": 92, "floor": 4, "floor_reference": 4, "floor_ours": 0,
        "alpha": 0.05, "p_hide": 0.393038,
    }
    floor.update(over.get("floor", {}))
    for k in over.get("floor_drop", ()):
        floor.pop(k, None)
    fp = os.path.join(td, "floor.toml")
    with open(fp, "w") as fh:
        for k, v in floor.items():
            fh.write(f"{k} = {json.dumps(v)}\n" if not isinstance(v, str)
                     else f'{k} = "{v}"\n')
        body = "\n".join(f'[[recording]]\nid = "r{i + 1}"\nsha256 = "{s}"\n'
                         for i, s in enumerate(over.get("shas", shas)))
        fh.write("\n" + body)
    # `[floor]` must come first and the writer above emits bare keys, so wrap.
    txt = open(fp).read()
    head, _, tail = txt.partition("\n[[recording]]")
    with open(fp, "w") as fh:
        fh.write("[floor]\n" + head + "\n[[recording]]" + tail)
    return cp, rp, fp


def _args(census, report, floor, **kw):
    ns = argparse.Namespace(census=census, report=report, floor=floor,
                            projection="proj:7cefde0fb56d9f3e", binary=None,
                            checks_dir="", basis="strict", json=None, selftest=False)
    for k, v in kw.items():
        setattr(ns, k, v)
    return ns


def selftest():
    import io  # noqa: PLC0415

    fails = []

    def case(name, expect_arm, **kw):
        with tempfile.TemporaryDirectory() as td:
            over = kw.pop("over", {})
            c, r, f = _fixture(td, **over)
            a = _args(c, r, f, **kw)
            buf = io.StringIO()
            try:
                rc = run(a, out=buf)
            except Refused as e:
                got = e.arm
            else:
                got = f"GREEN(rc={rc})"
            ok = got == expect_arm
            print(f"  {'ok  ' if ok else 'FAIL'}  {name:34s} expect {expect_arm:26s} got {got}")
            if not ok:
                fails.append(name)
            return buf.getvalue()

    print("parity-aggregate --selftest: every arm, and a green control at both ends")
    good = case("green control", "GREEN(rc=0)")

    case("projection malformed", "PROJECTION-MALFORMED", projection="deadbeef")
    case("projection empty", "PROJECTION-MALFORMED", projection="")
    case("projection stale", "STALE-PROJECTION", projection="proj:0000000000000000")
    case("floor missing p_hide", "NO-FLOOR", over={"floor_drop": ["p_hide"]})
    case("floor missing its projection", "NO-FLOOR", over={"floor_drop": ["projection"]})
    case("floor at N=1", "NO-FLOOR", over={"floor": {"runs": 1}})
    case("denominator moved 92->93", "DENOMINATOR-MISMATCH",
         over={"report": {"declared": {"total": 93, "matched": 75}, "strict": {"total": 93, "matched": 71}}})
    case("ensemble drift", "ENSEMBLE-DRIFT", over={"shas": ["0" * 64] * 6})
    case("census has no ceilings", "NO-CEILINGS", over={"census": {
        "protocol": {"denominator": 141, "emitted": 36, "observed": 31, "both": 29},
        "registry": {"sound_event": {"touched": 29, "denominator": 1968}}}})
    case("only two units survive", "TOO-FEW-UNITS", over={"census": {"registry": {
        "menu": {"touched": 3, "denominator": 25, "ceiling": 20, "ceiling_owner": "jar"},
        "block": {"touched": 43, "denominator": 1196, "ceiling": None,
                  "ceiling_owner": "unknown", "ceiling_rule": "no jar rule"}}}})

    # The currency arm is not a refusal: it must still print a number.
    with tempfile.TemporaryDirectory() as td:
        c, r, f = _fixture(td)
        os.mkdir(os.path.join(td, "checks"))
        for i in range(93):
            open(os.path.join(td, "checks", f"{i:03d}.toml"), "w").close()
        buf = io.StringIO()
        rc = run(_args(c, r, f, checks_dir=os.path.join(td, "checks")), out=buf)
        ok = rc == EXIT_NOT_CURRENT and "AS OF" in buf.getvalue() and "NOT CURRENT" in buf.getvalue()
        print(f"  {'ok  ' if ok else 'FAIL'}  {'corpus moved under it':34s} "
              f"expect {'rc=3 + a number':26s} got rc={rc}")
        if not ok:
            fails.append("currency")

    # Re-runnable: the same inputs must give the same answer, byte for byte.
    with tempfile.TemporaryDirectory() as td:
        c, r, f = _fixture(td)
        a, b = io.StringIO(), io.StringIO()
        run(_args(c, r, f), out=a)
        run(_args(c, r, f), out=b)
        ok = a.getvalue() == b.getvalue() and a.getvalue() != ""
        print(f"  {'ok  ' if ok else 'FAIL'}  {'deterministic on same inputs':34s} "
              f"expect {'byte-identical':26s} got {'identical' if ok else 'DIFFERENT'}")
        if not ok:
            fails.append("determinism")

    # A green control that has stopped exercising the composition is the
    # failure ci/t0.sh ran for weeks: assert the output actually contains one.
    for want in ("PARITY-AGGREGATE CURRENT", "SPREAD", "PRESENCE DISCOUNT",
                 "ranked levers", "EXCLUDED — NO REACH DENOMINATOR",
                 "EXCLUDED — UNKNOWN CEILING", "EXCLUDED — NO NUMERATOR"):
        ok = want in good
        print(f"  {'ok  ' if ok else 'FAIL'}  {'green control prints ' + want:34.34s} "
              f"expect {'present':26s} got {'present' if ok else 'ABSENT'}")
        if not ok:
            fails.append(f"green:{want}")

    case("green control, again", "GREEN(rc=0)")

    print(f"\nparity-aggregate selftest: {'GREEN' if not fails else 'RED — ' + ', '.join(fails)}")
    return 0 if not fails else 1


if __name__ == "__main__":
    sys.exit(main())
