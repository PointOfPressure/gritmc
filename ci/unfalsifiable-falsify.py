#!/usr/bin/env python3
"""Reconstruct the historical defects and watch the guards refuse them.

A guard that cannot be shown catching a real past instance is itself a check
that cannot fail, and would be the joke this branch exists to end. So this
takes the two guards — `ci/corpus-falsifiable.py` and `ci/liveness-proofs.py` —
reconstructs defects this project actually shipped, and asserts **red, then
green**: the guard refuses the reconstruction, and accepts its repair.

Three cases, each a defect with a date:

  1. **The readiness proof that cannot say "not ready".** `ci/t2-gameplay.sh`
     waits for `listening on`, which `crates/rustmc-server/src/main.rs` prints
     at 162, before `status::listen` at 170. A bind that fails prints
     `listen: Address already in use` and exits — after the grep has already
     succeeded. A server that never bound reads as ready and a whole corpus is
     recorded against a dead port.

     RED: an unregistered wait loop is found and refused by name; registered
     with the real predicate, it is refused again for accepting a log from a
     server that never bound. GREEN: the predicate is repaired in the script
     and the registry together, and the same negative fixture is rejected.

  2. **A recorded field that couples to no verdict.** The container `window`
     counter coupled zero checks — a server that never increments it is
     invisible — and `ACCEPT_TELEPORTATION` decoded into a field read by
     nothing outside its own unit tests, behind a doc comment describing a
     gate no line implemented.

     RED: a counter field is implanted into observations of a kind the
     baseline sweep already reports unread, in both recordings; the sweep
     finds nothing reads it either and the population rises by exactly one;
     and the ratchet, pinned at the **baseline** population, refuses. GREEN:
     one scenario's `observe` list is widened to name that kind, the same
     sweep over the same recordings finds the field live, and the gate passes.
     Same recordings, same comparator, one line of TOML, both directions.

     **Carries its own control (2b').** The same ceiling must be green on the
     unimplanted corpus. Without that, lowering a ceiling under a corpus that
     already has dead fields refuses whether or not the implant exists — which
     is what the first version did, and it passed for that reason.

  3. **A check that scores two silences.** `607-container-click-pickup-all-
     gathers-matching-stacks` passed for its whole life by comparing its own
     silence against vanilla's at the step it was named for.

     RED: a victim's measured step is silenced on **both** sides, the sweep
     grades it tier 1 — only a structural mutation moves it — and `--check`
     refuses it by name. GREEN: the silence is lifted and it returns to tier 2.

Case 3 is also the reachability proof for the tier-1 refusal arm. Axis A
finds no tier-1 scenario in the corpus as it stands, and an arm that has never
fired is indistinguishable from an arm that cannot: this fires it.

    ci/unfalsifiable-falsify.py --bin PATH VANILLA.json RUSTMC.json

Exit 0 if every case holds, 1 if any does not, 2 if it could not be run — the
same three-way split `ci/selfcheck-recompare.sh` uses, and for the same
reason: a control that cannot run must not read as a control that passed.

No server is started, no vanilla is booted and the corpus lock is not taken.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SWEEP = ROOT / "ci" / "corpus-falsifiable.py"

# Read out of the gate rather than restated here: a falsify that pins its own
# copy of a gate's allowance stops testing the gate the moment the two drift,
# which is how 1d came to assert a tolerance that had been withdrawn.
LP_MAX_UNSOUND = int(
    re.search(r"^MAX_UNSOUND\s*=\s*(\d+)", (ROOT / "ci" / "liveness-proofs.py").read_text(),
              re.M).group(1))
LIVENESS = ROOT / "ci" / "liveness-proofs.py"
CHECKS = ROOT / "ci" / "checks" / "gameplay"

# The log a server that never bound writes. Both lines are real: the first is
# from ~/rustmc-apiseal/oracle-work/gameplay/rustmc.log, the second is what
# `main.rs` prints when `status::listen` returns an error.
NEVER_BOUND = ("rustmc 26.2 listening on 0.0.0.0:26783 (not saving)\n"
               "listen: Address already in use (os error 98)\n")
DID_BIND = "rustmc 26.2 listening on 0.0.0.0:26783 (not saving)\n"

BROKEN_LINE = "grep -q 'listening on' \"$LOG\" 2>/dev/null && break"
FIXED_LINE = ("grep -q 'listening on' \"$LOG\" 2>/dev/null && "
              "! grep -q '^listen: ' \"$LOG\" && break")


_KEEP: list[bool] = []


def die(msg: str) -> None:
    _KEEP.append(True)
    print(f"UNFALSIFIABLE-FALSIFY REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


def _tidy(work: Path) -> None:
    import os
    if _KEEP or os.environ.get("UNFALSIFIABLE_FALSIFY_KEEP"):
        print(f"== work directory kept: {work}", file=sys.stderr)
    else:
        shutil.rmtree(work, ignore_errors=True)


class Report:
    def __init__(self) -> None:
        self.failures: list[str] = []
        self.skipped: list[str] = []

    def skip(self, name: str, detail: str) -> None:
        """A case that could not be exercised on THIS corpus — a fact about
        the corpus's current content, not about the guard under test. Printed
        and counted separately from `failures` so it neither passes silently
        nor fails a gate for a reason the guard cannot fix. See
        `pick_implant_kind`'s docstring for the case this exists for."""
        print(f"  SKIP  {name}: {detail}")
        self.skipped.append(f"{name}: {detail}")

    def case(self, name: str, ok: bool, detail: str) -> None:
        print(f"  {'PASS' if ok else 'FAIL'}  {name}: {detail}")
        if not ok:
            self.failures.append(f"{name}: {detail}")


# --------------------------------------------------------------------------
# 1. the readiness proof
# --------------------------------------------------------------------------


def _toml_str(s: str) -> str:
    """A TOML basic string. `json.dumps` escapes newlines, quotes and
    backslashes exactly as TOML wants them — hand-rolling it left literal
    newlines in and every case in [1] failed at the parser rather than at the
    assertion, which is a control reporting the wrong colour."""
    return json.dumps(s)


def liveness_case(rep: Report, work: Path) -> None:
    print("\n[1] the readiness proof that cannot say \"not ready\" "
          "(ci/t2-gameplay.sh:504, main.rs:162 before :170)")
    root = work / "case1"
    ci = root / "ci"
    ci.mkdir(parents=True)
    script = ci / "probe.sh"

    def write_script(line: str) -> None:
        script.write_text("#!/usr/bin/env bash\n"
                          "for _ in $(seq 1 60); do\n"
                          f"    {line}\n"
                          "    sleep 1\n"
                          "done\n")

    def registry(line: str, predicate: str, extra: str = "") -> Path:
        p = ci / "reg.toml"
        p.write_text(
            "[[proof]]\n"
            'script = "ci/probe.sh"\n'
            'what = "the server is bound"\n'
            f"source_line = {_toml_str(line)}\n"
            f"predicate = {_toml_str(predicate)}\n"
            f'positive = {_toml_str(DID_BIND)}\n'
            f'negative = {_toml_str(NEVER_BOUND)}\n'
            + extra)
        return p

    def run(reg: Path) -> tuple[int, str]:
        r = subprocess.run([sys.executable, str(LIVENESS), "--check",
                            "--ci", str(ci), "--registry", str(reg)],
                           capture_output=True, text=True)
        return r.returncode, r.stdout + r.stderr

    # RED (a): the proof exists and nobody registered it
    write_script(BROKEN_LINE)
    empty = ci / "empty.toml"
    empty.write_text("")
    rc, out = run(empty)
    rep.case("1a RED  unregistered readiness proof",
             rc == 1 and "UNREGISTERED" in out,
             f"rc={rc}, {'named' if 'probe.sh' in out else 'NOT named'}")

    # RED (b): registered, with the real predicate — it swallows a dead server
    rc, out = run(registry(BROKEN_LINE, "grep -q 'listening on' \"$LOG\""))
    rep.case("1b RED  the historical predicate accepts a server that never bound",
             rc == 1 and "UNSOUND" in out, f"rc={rc}")

    # GREEN: repair the predicate in the script and the registry together
    write_script(FIXED_LINE)
    rc, out = run(registry(FIXED_LINE,
                           "grep -q 'listening on' \"$LOG\" && ! grep -q '^listen: ' \"$LOG\""))
    rep.case("1c GREEN the repaired predicate rejects it and still accepts a live server",
             rc == 0, f"rc={rc}: {out.strip().splitlines()[-1] if out.strip() else ''}")

    # and the repair must be real, not a declaration: claiming soundness while
    # still swallowing the fixture has to stay red.
    #
    # **This asserted `rc == 0` until 2026-08-18 and had been failing since
    # `MAX_UNSOUND` was tightened to 0.** A declaration used to be tolerated so
    # long as it was counted; the allowance is zero now, so it is counted AND
    # refused, and the case is stronger for it. The two halves are both
    # required: the refusal has to be the RATCHET (a declared-unsound proof is
    # not a broken registry) and it has to name the allowance it exceeded.
    write_script(BROKEN_LINE)
    rc, out = run(registry(BROKEN_LINE, "grep -q 'listening on' \"$LOG\"",
                           extra="unsound = true\n"))
    rep.case("1d      declaring it unsound is not a repair — it is counted against an allowance of 0",
             rc == 1 and "RATCHET" in out and f"allowance is {LP_MAX_UNSOUND}" in out,
             f"rc={rc}, allowance {LP_MAX_UNSOUND}")


# --------------------------------------------------------------------------
# shared: run the sweep and the gate over a corpus/recording pair
# --------------------------------------------------------------------------


# **Every corpus this suite builds is a deliberately mutated COPY of the
# gameplay corpus, so it is graded against the gameplay pins by name.**
# `ci/corpus-falsifiable.py` keys its pins on the scenario directory
# (f1af31b15) and refuses one it does not know, which is right for a real
# new corpus and made every case here exit 2 at `checks-dead` instead of
# testing anything: the gate `gate-stack-ci-20260924-124330` went red on it
# the first time T0 ran this suite with a recording pair present. The CLI has
# no way to name the pins for an unknown directory, and giving it one would
# reopen the door that refusal closes. So the suite calls `sweep` and `check`
# with `corpus=GAMEPLAY` directly, which is how that script's own selftest
# grades its fixture trees.
#
# **And `--check` takes the field universe from the ledger it is handed.**
# `FIELD_UNIVERSE` pins how many (kind, field) pairs the COMMITTED ledger's
# recordings carried (90). A recording pair taken later by a newer recorder
# carries more — the gate directory's pair on 2026-09-24 carries 93 — so every
# control here (2b', 2c, 3d) went red on FIELD-UNIVERSE, a check none of these
# cases is about, and each RED case could pass for that reason instead of its
# own. Implanting a field (case 2) moves the universe by construction, too.
# The committed ledger is still held to the constant by `--check` in T0.
#
# **`DEAD_FIELDS` is the same trap, found by the train-c gate on 2026-09-30.**
# `check` refuses a ledger whose dead set is not the pinned tuple, and the
# pinned tuple describes the COMMITTED ledger's recordings. The recording pair
# a gate directory holds is whatever its last gate recorded: on 2026-09-30 it
# swept to 4 dead (the rotation pairs) against a tree pinning `()` since
# `mob-in-world`, so 2b' and 3d — the two GREEN controls — went red on a dead
# set none of these cases is about, and 2b could have passed for the same
# reason instead of its own. So every gate call here pins the dead set — and
# the unaskable ceiling, which is the same trap one constant over — from the
# BASELINE sweep of the pair actually present. 2b still has to name the
# implanted field, which the baseline does not carry, so it stays attributable
# to the implant.
_DRIVER = r"""
import importlib.util, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location("corpus_falsifiable", sys.argv[1])
cf = importlib.util.module_from_spec(spec)
sys.modules["corpus_falsifiable"] = cf
spec.loader.exec_module(cf)
a = [Path(x) for x in sys.argv[3:]]
if sys.argv[2] in ("sweep-AB", "sweep-A", "sweep-B"):
    axes = sys.argv[2].split("-")[1]
    sys.exit(cf.sweep(a[0], a[1], a[2], a[3], a[4], corpus=cf.GAMEPLAY, axes=axes))
if sys.argv[2] == "compose":
    # Axis A's rows from one ledger, axis B from another, written by the
    # guard's own `write_ledger`. See `compose` below.
    import tomllib
    rows, b = (tomllib.loads(x.read_text()) for x in a[:2])
    rm, bm = rows["meta"], b["meta"]
    pairs = lambda doc, key: [(d["kind"], d["field"]) for d in doc.get(key, [])]
    over = {r["name"]: r for r in
            (tomllib.loads(a[3].read_text()).get("scenario", []) if len(a) > 3 else [])}
    names = {r["name"] for r in rows.get("scenario", [])}
    if set(over) - names:
        sys.exit(f"override rows not in the ledger: {sorted(set(over) - names)}")
    entries = [{"name": r["name"], "file": r["file"], "digest": r["digest"],
                "killed_by": list(r.get("killed_by", [])), "tier": r["tier"],
                "localised": r.get("localised", False), "why": r.get("why", "")}
               for r in (over.get(x["name"], x) for x in rows.get("scenario", []))]
    # `floor` and `weak_floor` as `sweep` derives them from its rows, not
    # copied: an override can move both. The baseline composed with itself
    # proves this is the sweep's own arithmetic (see `main`).
    floor = sum(1 for e in entries if e["tier"] == 2)
    weak = sum(1 for e in entries if e["killed_by"] and not e["localised"])
    cf.write_ledger(a[2], entries, rm.get("unswept", []), pairs(b, "dead_field"),
                    bm["field_universe"], floor, weak,
                    pairs(b, "undetermined_field"), pairs(b, "unaskable_field"),
                    bm["reference_field_universe"], cf.GAMEPLAY.title)
    sys.exit(0)
# The field universe is pinned from the ledger under test, not from
# FIELD_UNIVERSE: see the paragraph above _DRIVER.
import copy, re
corp = copy.copy(cf.GAMEPLAY)
m = re.search(r"^field_universe\s*=\s*(\d+)", a[1].read_text(), re.M)
if m:
    corp.field_universe = int(m.group(1))
# The dead set is pinned from the BASELINE sweep of the recording pair present,
# for the reason the universe is pinned from the ledger: see the paragraph
# above _DRIVER on DEAD_FIELDS.
if len(a) > 2:
    import tomllib
    base = tomllib.loads(a[2].read_text())
    corp.dead_fields = tuple(
        (d["kind"], d["field"])
        for d in base.get("dead_field", []))
    # And the unaskable ceiling, for the same reason: MAX_UNASKABLE describes
    # the COMMITTED ledger's recordings, and a gate directory's pair recorded
    # by another tree reads its own count (15 against a pinned 14 on
    # land-batch-1's gate, 2026-10-02), which reddened 2b' and 3d on a ceiling
    # neither case is about.
    n = base.get("meta", {}).get("unaskable_fields")
    if isinstance(n, int):
        corp.max_unaskable = max(corp.max_unaskable, n)
sys.exit(cf.check(a[0], a[1], corpus=corp))
"""


def sweep(binary: Path, checks: Path, ref: Path, sub: Path, ledger: Path,
          axes: str = "AB") -> str:
    """The guard's own sweep. `axes="B"` is case 2's (see `dead_field_case`),
    `axes="A"` case 3's (see `DOWNSTREAM`)."""
    if axes not in ("AB", "A", "B"):
        die(f"no such sweep: axes={axes!r}")
    t = time.monotonic()
    r = subprocess.run([sys.executable, "-c", _DRIVER, str(SWEEP),
                        f"sweep-{axes}", str(binary),
                        str(checks), str(ref), str(sub), str(ledger)],
                       capture_output=True, text=True)
    if not ledger.exists():
        die(f"sweep wrote no ledger (rc={r.returncode})\n{r.stdout}\n{r.stderr}")
    print(f"    (sweep {axes:<2} {ledger.name}: {time.monotonic() - t:.0f} s)")
    return r.stdout + r.stderr


def compose(rows: Path, axis_b: Path, out: Path, override: Path | None = None) -> None:
    """A ledger carrying `rows`'s axis-A rows and `axis_b`'s axis-B sections.

    Case 2's implant changes nothing axis A can see that any case-2 assertion
    reads, so case 2 sweeps axis B alone and borrows the baseline's rows for
    the gate it hands the result to — making 2b the baseline ledger with
    exactly one thing changed, which is a tighter control than the full
    re-sweep it replaces. `main` proves the composition loses nothing before
    trusting it: the baseline composed with itself must come back byte for
    byte.

    `override`'s rows replace `rows`'s by name; case 3 uses it to put the
    re-graded reach into the baseline."""
    r = subprocess.run([sys.executable, "-c", _DRIVER, str(SWEEP), "compose",
                        str(rows), str(axis_b), str(out),
                        *([str(override)] if override else [])],
                       capture_output=True, text=True)
    if r.returncode != 0 or not out.exists():
        die(f"could not compose {out.name} (rc={r.returncode})\n{r.stdout}\n{r.stderr}")


# Set by `main` once the baseline sweep exists; see the DEAD_FIELDS paragraph
# above _DRIVER.
BASELINE: Path | None = None


def gate(checks: Path, ledger: Path, pins: Path | None = None) -> tuple[int, str]:
    pins = pins if pins is not None else BASELINE
    extra = [str(pins)] if pins is not None else []
    r = subprocess.run([sys.executable, "-c", _DRIVER, str(SWEEP), "check",
                        str(checks), str(ledger), *extra],
                       capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


def corpus_copy(work: Path, tag: str) -> Path:
    dst = work / f"checks-{tag}"
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(CHECKS, dst)
    return dst


def scenario_file(checks: Path, name: str) -> Path:
    for p in sorted(checks.glob("*.toml")):
        if re.search(rf'^name\s*=\s*"{re.escape(name)}"', p.read_text(), re.M):
            return p
    die(f"no scenario file declares name = {name!r}")
    raise SystemExit(2)


# --------------------------------------------------------------------------
# 2. a recorded field that couples to no verdict
# --------------------------------------------------------------------------

DEAD_FIELD = "teleport_id"


def pick_implant_kind(baseline: Path, checks: Path, sub: Path) -> str | None:
    """A kind the baseline sweep already reports a DEAD field on, and that
    some scenario in THIS corpus could actually be widened to observe.

    **Two wrong answers were tried before this one, and both made case 2 pass
    or fail for a reason about the choice of kind rather than about the gate.**

    A hard-coded `entity_position` landed in the sweep's `undetermined` bucket
    — its carrier diverges, so a change to it need not show — and 2c could
    never go green. Picking "any kind carried only by matching scenarios" then
    chose `block_entity_data`, which *is* read: the implanted field went live
    the instant it existed, 2a failed, and 2c passed without ever having been
    dead.

    Worse, **2b passed anyway**, because it lowered the dead-field ceiling
    under the corpus's own standing population and the gate refused on the
    four fields that were already there. A control that goes red whether or
    not the defect is present is the exact thing this branch exists to refuse,
    committed inside the suite that demonstrates the refusal.

    So the target is read off the baseline sweep: a kind that already carries a
    dead field is, by that sweep's own construction, both unread and
    *detectable* — `dead` requires every carrier of the kind to match.

    **A THIRD wrong answer, found the same way as the first two: `kinds[0]`
    alone.** It crashed case 2c on the live corpus, unnoticed, because
    nothing runs this file. Both real recordings tried while diagnosing this
    (`gameplay-entity-coverage`, 113 scenarios; `gameplay-container-menus`,
    123) picked `move_entity_rot` first — alphabetically first among the four
    dead kinds on both — and no scenario in the current
    `ci/checks/gameplay/` declares it in an `observe` list at all, so
    `pick_observer_victim` found none and its own terminal `die()` took the
    whole script down two cases early with exit 2, never reaching case 3.
    That is not evidence the corpus is fine; it is the harness failing to
    notice it could not check. So this tries each dead kind in the sweep's
    own order and uses the first with a real victim.

    **Returns `None`, does not `die()`, if none of them has one — a fourth
    wrong answer, found wiring this into T0 the same day.** The live corpus's
    four dead fields are ALL rotation (`move_entity_rot` x2, `rotate_head`
    x2, `docs/cold-start/ci-unfalsifiable-root.md`'s own known, already-
    tracked debt), and none currently has a widenable observer — so a hard
    `die()` here made this whole script permanently red on every real tree,
    for a fact about the corpus's content that is not this guard's to fix and
    not a defect in it either. `two_silences_case` does not depend on this
    function's answer at all, so a `die()` here was also refusing case 3 for
    a reason that has nothing to do with case 3. `dead_field_case` reports
    this as a named, non-fatal `SKIP` and `main` runs case 3 regardless.
    """
    import tomllib
    doc = tomllib.loads(baseline.read_text())
    kinds = [d["kind"] for d in doc.get("dead_field", [])]
    if not kinds:
        # **An EMPTY dead set is the same fact as a dead set with no widenable
        # observer, and gets the same answer.** Found on train-c over main 3953:
        # `mob-in-world` (2026-09-05) emptied `DEAD_FIELDS` — the rotation
        # pairs moved to undetermined once a pig and a bat stood in every
        # recording — so this `die()` made the whole script exit 2 on every
        # tree carrying a recording pair, refusing case 3 as well for a reason
        # that has nothing to do with it. It only ever ran where T0 found a
        # pair, which no merge gate on main had, so it was latent there.
        print("    the baseline sweep reports no dead field — nothing to implant beside")
        return None
    skipped = []
    for k in kinds:
        if pick_observer_victim(checks, sub, k, required=False) is not None:
            if skipped:
                print(f"    skipped {', '.join(skipped)} — no scenario in this corpus "
                      f"declares `observe` and could ever be widened to read them")
            return k
        skipped.append(k)
    print(f"    none of the {len(kinds)} dead kinds ({', '.join(kinds)}) has a scenario "
          f"in {checks} that could be widened to observe it")
    return None


def standing_dead_kind(binary: Path, checks: Path, ref: Path, sub: Path,
                       work: Path) -> tuple[Path, Path, str | None]:
    """The pair case 2 can run on: the one given, or a copy with one unread
    kind's observations taken out of the scenarios that do not match.

    **Case 2 needs a standing dead field, and train-g's pair has none.** Its
    baseline sweep reports 0 dead of 201, 360 tier-2, so `pick_implant_kind`
    returned None and 2a-2c were SKIPPED in every gate on 2026-10-02 (5 of 5
    in ~/rustmc-private/gates/, "1 skipped" on each T0 line): the arm that
    finds a field no check reads went a whole landing day with no
    red-when-broken evidence, printed as a pass.

    A dead field is one whose kind no check reads and whose every carrier
    matches — the second half is what makes a null result a claim (see
    `sweep`'s `undetermined`). So this looks for a kind that

      * no scenario names in `observe`, and that removing outright from the
        subject changes no outcome, field for field, projections included —
        it is invisible to the comparator as recorded;
      * is carried, in both traces, by a matching scenario that declares
        `observe`, so 2c has a check to widen,

    and takes it out of the non-matching scenarios on both sides. The copy is
    accepted only if every outcome is unchanged, field for field. Only case
    2 uses it, and only for axis B — the shared baseline and case 3 stay on
    the pair as recorded — and the
    baseline sweep remains the judge of what is dead: if the kind does not
    come out dead, that is the sweep failing to see a field this function
    showed unread, and case 2 says so as a FAILURE rather than a skip. Fewest
    observations taken out wins; a pair that already has such a kind loses
    none and is used as given."""
    import tomllib
    rdoc, sdoc = json.loads(ref.read_text()), json.loads(sub.read_text())
    base = compare_report(binary, checks, ref, sub, work / "prep-report.json")
    by = {o["name"]: o for o in base}
    match = {n for n, o in by.items()
             if o.get("verdict") == "match" and o.get("strict") == "match"}
    observed, declares = set(), set()
    for p in checks.glob("*.toml"):
        txt = p.read_text()
        d = tomllib.loads(txt)
        observed |= set(d.get("observe") or [])
        if re.search(r"^observe = \[", txt, re.M):
            declares.add(d.get("name"))

    def drops(doc: dict, kind: str) -> list[tuple[str, int]]:
        return [(sc["name"], i) for sc in doc["scenarios"] if sc["name"] not in match
                for i, o in enumerate(sc["obs"]) if o.get("kind") == kind]

    def carried(doc: dict, kind: str) -> bool:
        return any(o.get("kind") == kind for sc in doc["scenarios"]
                   if sc["name"] in match and sc["name"] in declares for o in sc["obs"])

    kinds = sorted({o["kind"] for sc in sdoc["scenarios"] for o in sc["obs"]
                    if o.get("fields")} - observed)
    plans = []
    for k in kinds:
        if not (carried(rdoc, k) and carried(sdoc, k)):
            continue
        dr, ds = drops(rdoc, k), drops(sdoc, k)
        plans.append((len(dr) + len(ds), k, dr, ds))

    def without(doc: dict, kind: str, where) -> dict:
        cut = set(where) if where is not None else None
        d = dict(doc)
        d["scenarios"] = [dict(sc, obs=[o for i, o in enumerate(sc["obs"])
                                        if o.get("kind") != kind or
                                        (cut is not None and (sc["name"], i) not in cut)])
                          for sc in doc["scenarios"]]
        return d

    for n, k, dr, ds in sorted(plans)[:8]:
        gone = work / "prep-gone.json"
        gone.write_text(json.dumps(without(sdoc, k, None)))
        if compare_report(binary, checks, ref, gone, work / "prep-gone-report.json") != base:
            continue
        if n == 0:
            print(f"== standing dead kind: `{k}` — carried only by matching scenarios already")
            return ref, sub, k
        r0, s0 = work / "ref0.json", work / "sub0.json"
        r0.write_text(json.dumps(without(rdoc, k, dr)))
        s0.write_text(json.dumps(without(sdoc, k, ds)))
        if compare_report(binary, checks, r0, s0, work / "prep-check-report.json") != base:
            continue
        print(f"== standing dead kind: `{k}` taken out of the scenarios that do not match "
              f"({len(dr)} vanilla, {len(ds)} ours); every outcome unchanged — case 2 "
              f"sweeps axis B over this copy")
        return r0, s0, k
    print("== standing dead kind: none can be made from this pair — case 2 will skip "
          "unless the baseline sweep finds one")
    return ref, sub, None


def dead_count(ledger: Path) -> int:
    import tomllib
    return len(tomllib.loads(ledger.read_text()).get("dead_field", []))


def dead_field_case(rep: Report, work: Path, binary: Path, ref: Path, sub: Path,
                    baseline: Path) -> None:
    print("\n[2] a recorded field nothing reads "
          "(the container `window` counter; ACCEPT_TELEPORTATION)")
    # **The prepared kind has to come out dead, or the sweep is blind.** Every
    # input `sweep` uses to call a field dead was checked when it was chosen:
    # no check names it, taking it out moves no outcome, and every carrier
    # matches. Before this, an axis B that never called anything dead made
    # `pick_implant_kind` return None and case 2 SKIP — green.
    #
    # **Case 2 runs on its own copy of the pair, axis B only.** The shared
    # baseline's rows still go to every gate below — `compose` carries them —
    # so nothing here sweeps axis A, and case 3 never sees the copy.
    DEAD_KIND = pick_implant_kind(baseline, CHECKS, sub)
    if DEAD_KIND is None:
        r0, s0, prepared = standing_dead_kind(binary, CHECKS, ref, sub, work)
        if prepared is not None:
            ref, sub = r0, s0
            sweep(binary, CHECKS, ref, sub, work / "prepared-b.toml", axes="B")
            compose(baseline, work / "prepared-b.toml", work / "prepared.toml")
            baseline = work / "prepared.toml"
            import tomllib
            got = sorted({d["field"] for d in
                          tomllib.loads(baseline.read_text()).get("dead_field", [])
                          if d["kind"] == prepared})
            rep.case("2- control: the unread kind prepared for this case is reported dead",
                     bool(got),
                     f"`{prepared}`: {', '.join(got) if got else 'NO field reported dead'}")
            DEAD_KIND = pick_implant_kind(baseline, CHECKS, sub)
    if DEAD_KIND is None:
        rep.skip("2a/2b/2b'/2c", "no standing dead field in this corpus has a scenario "
                 "that could be widened to observe it — a fact about the corpus's "
                 "current content (see pick_implant_kind), not about this guard")
        return
    D0 = dead_count(baseline)
    print(f"    implanting `{DEAD_FIELD}` into `{DEAD_KIND}` observations; "
          f"baseline dead population {D0}")

    def implant(src: Path, dst: Path) -> int:
        doc = json.loads(src.read_text())
        n = 0
        for s in doc["scenarios"]:
            for o in s["obs"]:
                if o.get("kind") == DEAD_KIND:
                    # A counter, so that the reconstruction is the `window`
                    # shape rather than a random blob: it advances, it is
                    # recorded, and no check is coupled to it.
                    o.setdefault("fields", {})[DEAD_FIELD] = n
                    n += 1
        dst.write_text(json.dumps(doc))
        return n

    r2, s2 = work / "ref2.json", work / "sub2.json"
    nr, ns = implant(ref, r2), implant(sub, s2)
    if nr == 0 or ns == 0:
        die(f"no `{DEAD_KIND}` observations in the recordings — nothing to implant")

    # RED: nothing reads it, and the ratchet refuses.
    #
    # **Axis B only, and that is every assertion case 2 makes.** 2a reads the
    # `[[dead_field]]` list and its count, 2c reads the list, and 2b's refusal
    # has to come from the dead-field arm — all axis B. A full sweep here
    # re-ran ~2,700 axis-A mutations whose rows no case-2 line reads, twice,
    # and that was half of this script's 64 minutes on train-g's 360-scenario
    # pair (2026-10-02). The gate in 2b still needs rows, so it is handed the
    # baseline's: see `compose`.
    checks = corpus_copy(work, "dead")
    led = work / "dead.toml"
    out = sweep(binary, checks, r2, s2, work / "dead-b.toml", axes="B")
    compose(baseline, work / "dead-b.toml", led)
    body = led.read_text()
    listed = f'field = "{DEAD_FIELD}"' in body
    D1 = dead_count(led)
    rep.case("2a RED  the implanted field is reported dead",
             listed and D1 == D0 + 1,
             f"{nr}+{ns} observations carry it; "
             f"{'listed' if listed else 'NOT listed'}; dead population {D0} -> {D1}")

    # **The ceiling is the BASELINE population, not this run's.** Setting it to
    # `D1 - 1` refuses whether or not the implant is what pushed it over — the
    # corpus's own standing dead fields would do it — and a control that goes
    # red without the defect present measures nothing. Pinned at `D0`, this arm
    # can only fire on the field just implanted.
    led.write_text(re.sub(r"^dead_fields = \d+$", f"dead_fields = {D0}",
                          body, count=1, flags=re.M))
    rc, gout = gate(checks, led, pins=baseline)
    # **The refusal has to NAME the implanted field, and that is a stronger
    # assertion than the one it replaces.** This used to accept any output
    # containing `RATCHET`, which was a count ceiling and could not fire — both
    # of its numbers came out of the same `write_ledger` call, so they were
    # equal by construction and only a hand-edit of the ledger could separate
    # them. That is why the ceiling is rewritten three lines up. The refusal is
    # a comparison against the set pinned in `ci/corpus-falsifiable.py` now, so
    # it fires on the implant itself; and the sweep over an implanted recording
    # also moves `field_universe`, which refuses too. Asking for the field's
    # own name is what keeps this arm attributable to the dead-field logic
    # rather than to whichever refusal happens to fire first.
    #
    # **And it has to be the ONLY refusal.** The ledger handed to the gate is
    # the baseline's with axis B swapped for the implanted sweep's, and 2b'
    # below shows the baseline itself green, so any other arm firing here is
    # the gate refusing something that is not the implant.
    arms = sorted({ln.split()[0] for ln in _refusals(gout)})
    rep.case("2b RED  the dead-field refusal names the field just implanted",
             rc == 1 and DEAD_FIELD in gout and arms == ["DEAD-FIELDS"],
             f"rc={rc}, ceiling {D0} vs {D1} dead, "
             f"{'named' if DEAD_FIELD in gout else 'NOT named'} in the refusal, "
             f"refused by {', '.join(arms) or 'nothing'}")

    # ...and the same ceiling must be green on the same corpus WITHOUT the
    # implant, or 2b is measuring the corpus rather than the implant.
    rc0, _ = gate(CHECKS, baseline, pins=baseline)
    rep.case("2b' control: the unimplanted corpus passes at that same ceiling",
             rc0 == 0, f"rc={rc0}, {D0} dead against a ceiling of {D0}")

    # GREEN: couple one check to that kind. Same recordings, same comparator,
    # one line of TOML.
    checks2 = corpus_copy(work, "live")
    victim = pick_observer_victim(checks2, s2, DEAD_KIND)
    txt = victim.read_text()
    # **`observe_from` is widened with `observe`, and leaving it out cost this
    # case entirely.** Every scenario in today's corpus that carries
    # `move_entity_rot` carries it only on the OBSERVER, and all four of them
    # declare `observe_from = "actor"` — so a victim search that required the
    # observation to be on the connection the scenario already watches found
    # none at all, and this script died at `pick_observer_victim` before
    # reaching its GREEN arm. The corpus is copied, so the copy may be made
    # suitable rather than the search made to fail.
    txt = re.sub(r'^observe_from\s*=\s*"[^"]+"\n', "", txt, flags=re.M)
    txt = re.sub(r"^observe = \[", f'observe_from = "both"\nobserve = ["{DEAD_KIND}", ',
                 txt, count=1, flags=re.M)
    victim.write_text(txt)
    led2 = work / "live.toml"
    out2 = sweep(binary, checks2, r2, s2, led2, axes="B")
    still = f'field = "{DEAD_FIELD}"' in led2.read_text()
    rep.case("2c GREEN one scenario names that kind and the field is live",
             not still,
             f"{victim.name} now observes {DEAD_KIND}; field "
             f"{'STILL dead' if still else 'no longer dead'}")

    if "dead fields" in out and "dead fields" in out2:
        a = re.search(r"dead fields\s+(\d+)/", out)
        b = re.search(r"dead fields\s+(\d+)/", out2)
        if a and b:
            print(f"          dead-field population {a.group(1)} -> {b.group(1)}")


def _refusals(gout: str) -> list[str]:
    """The refusal lines `check` printed: the indented block under its RED
    banner, up to the blank line before its how-to-clear footer."""
    lines = gout.splitlines()
    if "CORPUS-FALSIFIABLE RED" not in lines:
        return []
    out = []
    for ln in lines[lines.index("CORPUS-FALSIFIABLE RED") + 1:]:
        if not ln.strip():
            break
        out.append(ln.strip())
    return out


def pick_observer_victim(checks: Path, sub: Path, DEAD_KIND: str,
                         required: bool = True) -> Path | None:
    """A scenario that would actually PROJECT `DEAD_KIND` once it names it.

    **`observe_from` is the whole of this function and leaving it out cost a
    run.** A scenario carrying `move_entity_rot` only on the *observer*
    connection, while declaring `observe_from = "actor"`, still filters every
    one of them out after its `observe` list is widened — so the implanted
    field stays dead, and 2c fails for a reason about the choice of victim
    rather than about the gate. `180-stairs-make-a-corner-when-two-meet` was
    exactly that.

    So the observation has to be on the connection the scenario watches, and
    `required=False` returns `None` instead of dying — the probe
    `pick_implant_kind` uses to try the sweep's dead kinds in order without
    taking the whole script down on the first one with no victim.

    `observe_from` defaults to **`both`** when unstated:
    `tools/oracle-runner/src/scenario.rs` declares `From` with `#[default]
    Both`. This said `actor`, which narrows the victim search rather than
    widening it — a scenario watching both connections and carrying
    `DEAD_KIND` only on the observer was skipped as unsuitable when it is
    exactly suitable. Six scenarios in the corpus leave it unstated.
    """
    doc = json.loads(sub.read_text())
    for s in doc["scenarios"]:
        try:
            p = scenario_file(checks, s["name"])
        except SystemExit:
            continue
        txt = p.read_text()
        if not re.search(r"^observe = \[", txt, re.M):
            continue
        if any(o.get("kind") == DEAD_KIND for o in s["obs"]):
            return p
    if not required:
        return None
    die(f"no scenario both sees `{DEAD_KIND}` on the connection it watches "
        f"and declares `observe`")
    raise SystemExit(2)


# --------------------------------------------------------------------------
# 3. a check that scores two silences
# --------------------------------------------------------------------------


def two_silences_case(rep: Report, work: Path, binary: Path, ref: Path, sub: Path,
                      baseline: Path) -> None:
    print("\n[3] a check that scores two silences "
          "(607-container-click-pickup-all-gathers-matching-stacks)")
    checks = CHECKS
    led = baseline
    rows = parse_rows(led)

    # The victim has to be `match` at baseline with BOTH sides speaking at its
    # measured step, or the case tests the choice of victim rather than the
    # gate: silencing a scenario that was already silent there changes nothing,
    # and silencing a diverging one lands on the `discriminating` promotion.
    base = compare_report(binary, checks, ref, sub, work / "base-report.json")
    cands = [o for o in base
             if (o.get("verdict") == "match" and o.get("strict") == "match"
                 and (o.get("measured_subject_obs") or 0) > 0
                 and (o.get("measured_reference_obs") or 0) > 0
                 and rows.get(o["name"], {}).get("tier") == 2)]
    if not cands:
        die("no matching tier-2 scenario with output on both sides to silence")
    victim, step, reach = pick_silence_victim(cands, rows, ref, sub)

    def silence(src: Path, dst: Path) -> int:
        doc = json.loads(src.read_text())
        n = 0
        for s in doc["scenarios"]:
            if s["name"] != victim:
                continue
            before = len(s["obs"])
            s["obs"] = [o for o in s["obs"] if o.get("step") != step]
            n = before - len(s["obs"])
        dst.write_text(json.dumps(doc))
        return n

    r3, s3 = work / "ref3.json", work / "sub3.json"
    nr, ns = silence(ref, r3), silence(sub, s3)

    led3 = work / "silence.toml"
    reach = check_reach_premises(reach, binary, checks, ref, sub, r3, s3, work, base, rows)
    if reach is None:
        sweep(binary, checks, r3, s3, led3)
    else:
        part = work / "silence-reach.toml"
        sweep(binary, cut_corpus(work, "silence-reach", reach), r3, s3, part, axes="A")
        compose(baseline, baseline, led3, override=part)
    rows3 = parse_rows(led3)
    got = rows3.get(victim, {}).get("tier")
    rep.case("3a RED  both sides silent at the measured step grades tier 1",
             got == 1,
             f"{victim} step {step}: -{nr} vanilla obs, -{ns} ours, tier {got}")

    rc, gout = gate(checks, led3)
    # ...and names nobody else: an UNFALSIFIABLE line for any other scenario
    # would be the gate refusing something the silence did not do.
    others = [ln for ln in _refusals(gout) if ln.startswith("UNFALSIFIABLE") and victim not in ln]
    rep.case("3b RED  the gate refuses it by name",
             rc == 1 and "UNFALSIFIABLE" in gout and victim in gout and not others,
             f"rc={rc}, {len(others)} other scenario(s) refused as unfalsifiable")

    # localisation: silencing one check must not regrade the others
    moved = [n for n in rows if n != victim and rows[n]["tier"] != rows3.get(n, {}).get("tier")]
    graded = (f"of {len(reach) - 1} re-graded downstream of it, {len(rows) - len(reach)} "
              f"upstream and unmoved by construction" if reach is not None
              else f"of {len(rows) - 1} re-graded")
    rep.case("3c      and no other scenario is regraded",
             not moved, f"{len(moved)} moved {graded}"
                        f"{': ' + ', '.join(moved[:3]) if moved else ''}")

    rc, gout = gate(checks, led)
    why = "" if rc == 0 else "; " + " | ".join(l.strip() for l in gout.splitlines() if l.strip())[:600]
    rep.case("3d GREEN the unsilenced corpus passes the same gate",
             rc == 0, f"rc={rc}, {sum(1 for r in rows.values() if r['tier'] == 2)} tier-2{why}")


# **Case 3 re-grades only what a silenced step can reach, and this is why
# that is the same answer.** `compare::compare_with`
# (tools/oracle-runner/src/compare.rs) computes each scenario's outcome from
# its own scenario file, its own observations on each side, and the `Places`
# that `scenario_starts` accumulates over the scenarios recorded BEFORE it in
# each trace — nothing else. And `sweep` grades a scenario's tier from its own
# outcome under mutations of its own observations. So silencing the victim's
# step cannot change the tier of any scenario recorded before it in both
# traces: every input to that tier is byte-identical. Only the victim and what
# follows it in either trace can move, and those are swept in full, through
# the real comparator, on a corpus cut to them.
#
# The victim is the latest-recorded candidate that still leaves DOWNSTREAM
# scenarios to re-grade, so 3c keeps asking a real question of real
# neighbours instead of none. The first candidate in file order — what this
# chose before — was recorded first, which put the whole corpus downstream of
# it and made case 3 a second full sweep: 32 of this script's 64 minutes on
# train-g's 360-scenario pair (2026-10-02).
#
# Both premises are re-checked on every run, and either failing sweeps the
# whole corpus as before and says so: see `check_reach_premises`.
DOWNSTREAM = 16


def pick_silence_victim(cands: list[dict], rows: dict, ref: Path, sub: Path):
    """(victim, measured step, the names its silencing can reach) — the last
    set is None when the traces do not support the argument above, and the
    first candidate in file order is used and swept whole, as before."""
    orders = []
    for t in (ref, sub):
        names = [s["name"] for s in json.loads(t.read_text())["scenarios"]]
        if len(set(names)) != len(names):
            print("    a trace records a scenario twice — sweeping case 3 whole")
            return cands[0]["name"], cands[0]["measured_step"], None
        orders.append({n: i for i, n in enumerate(names)})

    def reach(v: str) -> set[str]:
        return {n for n in rows if any(o[n] >= o[v] for o in orders)}

    sized = [(len(reach(o["name"])), i, o) for i, o in enumerate(cands)
             if all(o["name"] in od for od in orders)]
    fit = [x for x in sized if x[0] >= DOWNSTREAM]
    if not fit:
        print(f"    no candidate leaves {DOWNSTREAM} scenarios downstream — sweeping case 3 whole")
        return cands[0]["name"], cands[0]["measured_step"], None
    _, _, o = min(fit, key=lambda x: (x[0], x[1]))
    return o["name"], o["measured_step"], reach(o["name"])


def check_reach_premises(reach, binary, checks, ref, sub, r3, s3, work, base, rows):
    """`reach`, or None if either premise above fails on THIS run.

    1. Silencing changed no outcome outside `reach`: the whole corpus is
       compared over the silenced pair and every scenario upstream must come
       back field for field, projections included, as it did unsilenced.
    2. Cutting the corpus changes no row: the reach is swept over the
       UNSILENCED pair and every row must equal the baseline's."""
    if reach is None:
        return None
    again = {o["name"]: o for o in compare_report(binary, checks, r3, s3,
                                                    work / "silence-report.json")}
    moved = [o["name"] for o in base if o["name"] not in reach and again.get(o["name"]) != o]
    if moved:
        print(f"    PREMISE 1 FAILED — silencing moved {len(moved)} upstream outcome(s) "
              f"({', '.join(moved[:3])}); the comparator is not prefix-local, sweeping whole")
        return None
    ctl = work / "reach-control.toml"
    sweep(binary, cut_corpus(work, "reach-control", reach), ref, sub, ctl, axes="A")
    got = _rows_full(ctl)
    want = {n: r for n, r in _rows_full(BASELINE).items() if n in reach}
    if got != want:
        bad = sorted(n for n in want if got.get(n) != want[n])
        print(f"    PREMISE 2 FAILED — cutting the corpus to the reach changed "
              f"{len(bad)} row(s) ({', '.join(bad[:3])}); sweeping whole")
        return None
    print(f"    victim's reach: {len(reach)} of {len(rows)} scenarios; both premises hold")
    return reach


def cut_corpus(work: Path, tag: str, keep: set[str]) -> Path:
    import tomllib
    dst = corpus_copy(work, tag)
    for p in dst.glob("*.toml"):
        if tomllib.loads(p.read_text()).get("name") not in keep:
            p.unlink()
    return dst


def _rows_full(ledger: Path) -> dict[str, dict]:
    import tomllib
    return {r["name"]: r for r in tomllib.loads(ledger.read_text()).get("scenario", [])}


def parse_rows(ledger: Path) -> dict[str, dict]:
    import tomllib
    doc = tomllib.loads(ledger.read_text())
    return {r["name"]: {"tier": r.get("tier"), "killed_by": r.get("killed_by", [])}
            for r in doc.get("scenario", [])}


def compare_report(binary: Path, checks: Path, ref: Path, sub: Path, out: Path) -> list[dict]:
    """The real comparator's report — for `measured_step`, which the ledger
    does not carry and which case 3 has to silence."""
    subprocess.run([str(binary), "compare", str(checks), "--reference", str(ref),
                    "--subject", str(sub), "--json", str(out)],
                   capture_output=True, text=True)
    if not out.exists():
        die("compare wrote no report")
    return json.loads(out.read_text())["outcomes"]


# --------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    global CHECKS
    args = argv[1:]
    binary = ROOT / "target" / "release" / "oracle-runner"
    if "--bin" in args:
        i = args.index("--bin")
        binary = Path(args[i + 1])
        del args[i:i + 2]
    if len(args) != 2:
        die("usage: ci/unfalsifiable-falsify.py [--bin PATH] VANILLA.json RUSTMC.json")
    ref, sub = Path(args[0]), Path(args[1])
    for p in (binary, SWEEP, LIVENESS, CHECKS, ref, sub):
        if not p.exists():
            die(f"missing {p}")

    work = Path(tempfile.mkdtemp(prefix="unfalsifiable-falsify-"))
    # **Removed on GREEN, kept on anything else.** Every T0 runs this, and the
    # work directory — four trace copies, two corpus copies, five ledgers —
    # was left in /tmp by every one of them. A red or refused run keeps it and
    # says where, because that is the run somebody has to read.
    # `UNFALSIFIABLE_FALSIFY_KEEP=1` keeps a green one too.
    import atexit
    atexit.register(_tidy, work)
    rep = Report()
    print(f"== corpus     {CHECKS}")
    print(f"== reference  {ref}")
    print(f"== subject    {sub}")

    liveness_case(rep, work)

    # **The corpus is cut to the scenarios the pair recorded.** The pair is a
    # FIXTURE here — the guards are the subject — and a gate directory's pair
    # is whatever its last gate recorded, which on a branch that adds a
    # scenario is main's corpus, one short. `check` then refuses the tree's
    # new scenario as unswept, and 2b' and 3d, the two GREEN controls, go red
    # on a scenario none of these cases is about (land-batch-1 on main 4032,
    # 2026-10-01: `531-a-block-broken-out-of-view-is-not-broadcast`, 180 files
    # against 179 recorded). The DEAD_FIELDS paragraph above _DRIVER is the
    # same trap one level up. Whether the TREE's corpus is all swept is
    # `corpus-falsifiable.py --check`'s job against the committed ledger, and
    # it still runs in T0. The cut is printed so it cannot pass unseen.
    #
    # **And to the scenarios that ran on both sides.** A gate directory's pair
    # is also whatever a FAILED gate recorded: train-g's red T2 of 2026-10-02
    # 07:34 left a pair with 42 errored scenarios on our side (one clock-thread
    # panic) and 5 on vanilla's, and the next gate's T0 read it. An errored run
    # has no measured step to grade, so the baseline sweep has 318 tier-2 of
    # 360 and 3d, the unsilenced GREEN control, went red on scenarios no case
    # here is about (gate-train-g-20261002-143224.log). One failed train must
    # not redden every later gate's T0. Cutting them leaves 3d falsifiable: an
    # errored scenario never reaches the sweep, so it can neither excuse nor
    # accuse the gate, and 3a-3c still silence a scenario that ran on both
    # sides and must see it refused. Both cuts are printed by name.
    try:
        rdoc = json.loads(ref.read_text())["scenarios"]
        sdoc = json.loads(sub.read_text())["scenarios"]
        recorded = {s["name"] for s in rdoc} & {s["name"] for s in sdoc}
        errored = {s["name"]: side for side, doc in (("vanilla", rdoc), ("ours", sdoc))
                   for s in doc if s.get("error")}
        both = {s["name"] for s in rdoc if s.get("error")} & {s["name"] for s in sdoc if s.get("error")}
    except (ValueError, KeyError, TypeError, AttributeError) as e:
        die(f"cannot read scenario names from the pair: {e}")
    import tomllib
    present = work / "checks-recorded"
    shutil.copytree(CHECKS, present)
    cut, cut_err = [], []
    for p in sorted(present.glob("*.toml")):
        name = tomllib.loads(p.read_text()).get("name")
        if name not in recorded:
            cut.append(p.stem)
            p.unlink()
        elif name in errored:
            cut_err.append(f"{p.stem} ({'both' if name in both else errored[name]})")
            p.unlink()
    kept = len(list(present.glob("*.toml")))
    if kept == 0:
        die("the pair recorded none of this tree's scenarios without an error — "
            "nothing to falsify against")
    print(f"== cut {len(cut)} scenario(s) the pair did not record"
          + (f": {', '.join(cut)}" if cut else ""))
    print(f"== cut {len(cut_err)} scenario(s) whose run errored on either side"
          + (f": {', '.join(cut_err)}" if cut_err else ""))
    print(f"== {kept} kept")
    CHECKS = present

    # ONE baseline sweep, shared. Cases 2 and 3 both need it — 2 to know which
    # kinds are already unread and what the standing dead population is, 3 to
    # know which scenarios are tier 2 — and taking it twice cost 95 s for two
    # copies of the same number.
    global BASELINE
    baseline = work / "baseline.toml"
    sweep(binary, CHECKS, ref, sub, baseline)
    BASELINE = baseline
    # Case 2 trusts `compose` to carry rows and axis B across unchanged; the
    # baseline composed with itself is the check that it does.
    compose(baseline, baseline, work / "baseline-recomposed.toml")
    if (work / "baseline-recomposed.toml").read_bytes() != baseline.read_bytes():
        die("composing the baseline ledger with itself did not reproduce it — "
            "case 2's ledger would not be the one a sweep writes")
    print(f"\n== baseline sweep: {dead_count(baseline)} dead fields, "
          f"{sum(1 for r in parse_rows(baseline).values() if r['tier'] == 2)} tier-2")

    t = time.monotonic()
    dead_field_case(rep, work, binary, ref, sub, baseline)
    print(f"    (case 2: {time.monotonic() - t:.0f} s)")
    t = time.monotonic()
    two_silences_case(rep, work, binary, ref, sub, baseline)
    print(f"    (case 3: {time.monotonic() - t:.0f} s)")

    print()
    if rep.failures:
        _KEEP.append(True)
        print("UNFALSIFIABLE-FALSIFY RED", file=sys.stderr)
        for f in rep.failures:
            print("  " + f, file=sys.stderr)
        return 1
    skipped = f", {len(rep.skipped)} skipped (corpus content, not this guard)" if rep.skipped else ""
    print("UNFALSIFIABLE-FALSIFY GREEN — every reconstructed defect was refused, "
          f"and every repair accepted{skipped}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
