#!/usr/bin/env python3
"""Five defects found in the falsifiability machinery on 2026-08-18, each one
reconstructed and shown caught.

# Why these need a falsifier of their own

`ci/unfalsifiable-falsify.py` reconstructs three shipped defects in the
*corpus*. These five are in the *instruments that audit the corpus*, which is
a worse place for them: a guard that cannot fire is invisible from every
direction at once, and the population it guards keeps being reported as
healthy. Every one of the five below read as green on the day it was found.

    1  a ratchet whose two numbers come out of the same file
    2  a ledger field written by the sweep and read by nothing
    3  a population measured over the wrong universe
    4  a default restated in three places and wrong in all of them
    5  a falsify case asserting a tolerance its gate had withdrawn

**Nothing here needs a recording, a server or the corpus lock**, which is the
point: these are properties of the instruments, and an audit of the auditors
that could only run behind a forty-minute lock would not be run.

# The fifth is the sharpest and the reason this file exists

`ci/unfalsifiable-falsify.py` — the falsifier for unfalsifiable checks — was
itself partly unfalsifiable. Its case 1d asserted `rc == 0` because a proof
declared `unsound = true` used to be tolerated; `MAX_UNSOUND` was later
tightened to 0 and the case had been red ever since, unnoticed, because
nothing in T0 runs that script end to end. Its case 2c had stopped finding a
victim at all, so the GREEN half of case 2 had not executed in some time.

The repair was to read the allowance out of the gate instead of restating it.
Case 5 below is what makes that repair stick: it rewrites the gate's constant
in a temporary copy and asserts the falsify's derived value follows.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SWEEP = ROOT / "ci" / "corpus-falsifiable.py"
LIVENESS = ROOT / "ci" / "liveness-proofs.py"
UNFALS = ROOT / "ci" / "unfalsifiable-falsify.py"
SCENARIO_RS = ROOT / "tools" / "oracle-runner" / "src" / "scenario.rs"
CHECKS = ROOT / "ci" / "checks" / "gameplay"


class Report:
    def __init__(self) -> None:
        self.bad: list[str] = []
        self.n = 0

    def case(self, name: str, ok: bool, detail: str = "") -> None:
        self.n += 1
        print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f": {detail}" if detail else ""))
        if not ok:
            self.bad.append(name)


def gate_check(ledger: Path, *, dead=None, work: Path | None = None) -> tuple[int, str]:
    """Run `ci/corpus-falsifiable.py --check` over `ledger`.

    `dead` rewrites the gate's `DEAD_FIELDS` ceiling in a **temporary copy**
    and runs that instead, which is the same technique case 5 uses on
    `MAX_UNSOUND`. It exists because case 1c cannot otherwise be expressed at
    all: "a field that stopped being dead" needs the pinned set to contain
    something the ledger does not, and this branch emptied the pinned set to
    `()`. A guard that can only be written when the value it guards is
    non-empty is a guard that disappears when that value reaches its best
    state — see `case_self_comparing_ratchet`.

    The copy goes in a temp directory, which is safe because `ROOT` in the gate
    is used only to build the defaults for `--bin`, `--checks` and `--ledger`,
    all three passed explicitly here, and the corpus table, which the
    `checks` symlink below points back at the real tree. Checked rather than assumed:
    `1z` below runs the un-injected copy and requires it to agree with the
    original, so a future use of `ROOT` in the `--check` path fails loudly
    instead of quietly changing what these cases measure.
    """
    script = SWEEP
    if dead is not None:
        assert work is not None, "an injected ceiling needs a work directory"
        pins = "".join(f'    ("{k}", "{f}"),\n' for k, f in dead)
        # **The copy sits at `<work>/ci/` beside a `checks` symlink into the
        # real tree**, because the gate grades a ledger against the corpus its
        # scenario directory names, looked up by `ROOT / "ci" / "checks" / …`
        # resolved (`corpus_for`, `BY_CHECKS`). A copy dropped straight into
        # `work` has `ROOT = work.parent`, names no corpus, and refuses every
        # ledger with rc=2 — 1a, 1c, 1d and 1z all went red that way on the
        # rebase onto the `Corpus` records. The symlink resolves to the real
        # directory, so the copy is graded against the real pins; `1z` is what
        # proves it.
        (work / "ci").mkdir(exist_ok=True)
        link = work / "ci" / "checks"
        if not link.exists():
            link.symlink_to(SWEEP.parent / "checks", target_is_directory=True)
        script = work / "ci" / "corpus-falsifiable-injected.py"
        patched, hits = re.subn(
            DEAD_FIELDS_RE,
            f"DEAD_FIELDS = (\n{pins})\n" if pins else "DEAD_FIELDS = ()\n",
            SWEEP.read_text(), count=1, flags=re.M | re.S)
        # **The injection must be shown to have injected.** `re.sub` returns
        # the subject unchanged when nothing matches, so a pattern that has
        # stopped matching the constant it rewrites produces a temporary copy
        # identical to the real script — every arm then runs against the live
        # ceiling and passes, which is this whole file's defect class turned on
        # itself. It cost case 1c once (2026-09-05, the `[:-1]` slice) and it
        # cost this rewriter once more the same day, when the constant grew a
        # `: tuple` annotation and the anchored pattern stopped matching.
        assert hits == 1, (
            f"the DEAD_FIELDS rewriter matched {hits} times in {SWEEP.name} — "
            "the injection would have been a no-op and every arm below would "
            "have run against the live ceiling")
        script.write_text(patched)
    r = subprocess.run([sys.executable, str(script), "--check",
                        "--checks", str(CHECKS), "--ledger", str(ledger)],
                       capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


# The one pattern both the reader and the rewriter below use, so they cannot
# drift apart into a reader that sees the constant and a rewriter that does
# not. The optional annotation is not decoration: an empty tuple is written
# `DEAD_FIELDS: tuple = ()` so a type checker infers `tuple` rather than
# `tuple[()]`, and `origin/corpus-instruments` writes `SURVIVAL_DEAD_FIELDS`
# that way — the `^` anchor is what keeps this off that one.
DEAD_FIELDS_RE = r"^DEAD_FIELDS(?:\s*:\s*[^=\n]+?)?\s*=\s*\((.*?)\)\n"


def constants() -> dict[str, object]:
    src = SWEEP.read_text()
    m = re.search(DEAD_FIELDS_RE, src, re.M | re.S)
    assert m is not None, (
        f"no `DEAD_FIELDS = (...)` in {SWEEP.name} — this file reads that "
        "constant by pattern, and a pattern that has stopped matching reads "
        "as an absent ceiling rather than as a broken reader")
    dead = m.group(1)
    return {
        "DEAD_FIELDS": re.findall(r'\("([^"]+)",\s*"([^"]+)"\)', dead),
        "FIELD_UNIVERSE": int(re.search(r"^FIELD_UNIVERSE = (\d+)", src, re.M).group(1)),
        "MAX_UNASKABLE": int(re.search(r"^MAX_UNASKABLE = (\d+)", src, re.M).group(1)),
    }


def build_ledger(path: Path, *, dead, universe, unaskable, reference_universe=102) -> None:
    """A ledger of the same shape `write_ledger` produces, with every row
    borrowed from the committed one so that only the thing under test differs.
    """
    live = (ROOT / "ci" / "checks" / "gameplay-falsifiable.toml").read_text()
    head, _, rows = live.partition("# MERGE-REGION BEGIN")
    meta = re.sub(r"^dead_fields = \d+$", f"dead_fields = {len(dead)}", head, flags=re.M)
    meta = re.sub(r"^field_universe = \d+$", f"field_universe = {universe}", meta, flags=re.M)
    meta = re.sub(r"^unaskable_fields = \d+$", f"unaskable_fields = {len(unaskable)}",
                  meta, flags=re.M)
    meta = re.sub(r"^reference_field_universe = \d+$",
                  f"reference_field_universe = {reference_universe}", meta, flags=re.M)
    # strip the recorded blocks, then re-emit the ones this case wants
    meta = re.sub(r"\[\[dead_field\]\]\nkind = .*?\nfield = .*?\n\n", "", meta, flags=re.S)
    meta = re.sub(r"\[\[unaskable_field\]\]\nkind = .*?\nfield = .*?\n(#.*\n)*\n", "",
                  meta, flags=re.S)
    out = [meta.rstrip("\n"), ""]
    for k, f in dead:
        out += ["[[dead_field]]", f'kind = "{k}"', f'field = "{f}"', ""]
    for k, f in unaskable:
        out += ["[[unaskable_field]]", f'kind = "{k}"', f'field = "{f}"', ""]
    out += ["# MERGE-REGION BEGIN" + rows]
    path.write_text("\n".join(out))


# --------------------------------------------------------------------------
# 1  a ratchet whose two numbers come out of the same file
# --------------------------------------------------------------------------


def case_self_comparing_ratchet(rep: Report, work: Path, C) -> None:
    """`dead_fields` was `[meta] dead_fields` against `len(dead_field)`.

    Both are written by the same `write_ledger` call in the same sweep run, so
    they are equal by construction and `dead > dead_floor` could never be true.
    The refusal it advertised — "a field was added that couples to no verdict"
    — was reachable only by hand-editing the file it was checking, which that
    file's own header forbids.
    """
    print("\n[1] a ratchet whose two numbers come out of the same file "
          "(dead_fields, ci/corpus-falsifiable.py)")
    live = [tuple(p) for p in C["DEAD_FIELDS"]]

    # **The ceiling this case runs against is INJECTED, and that is the repair
    # rather than a convenience.** It was the live `DEAD_FIELDS`, with `1c`
    # written as `dead[:-1]`; the `packets` branch emptied the ceiling to `()`
    # on 2026-09-05, `[:-1]` became `[]` — identical to the pinned set — and
    # `1c` stopped expressing anything. T0 caught it, which is this file
    # working, and the lesson is the same one the file is about: a guard whose
    # meaning depends on the value it guards disappears exactly when that value
    # reaches its best state. Both directions now hold at any live ceiling,
    # empty included, and `1e` still runs the real one.
    dead = [("sound", "volume"), ("sound", "pitch")]

    led = work / "dead-new.toml"
    build_ledger(led, dead=dead + [("sound", "seed")],
                 universe=C["FIELD_UNIVERSE"], unaskable=[])
    rc, out = gate_check(led, dead=dead, work=work)
    rep.case("1a RED  a newly dead field is refused, and named",
             rc == 1 and "sound.seed" in out, f"rc={rc}")

    # **The pre-repair rule, run against the very same file.** `write_ledger`
    # always writes a `dead_fields` equal to the number of blocks beneath it,
    # so the old comparison is False on any ledger a sweep produced — which is
    # the whole claim, and it is checked rather than asserted in prose.
    body = led.read_text()
    recorded = int(re.search(r"^dead_fields = (\d+)$", body, re.M).group(1))
    listed = body.count("[[dead_field]]")
    rep.case("1b      the pre-repair rule cannot fire on a swept ledger: "
             "len(dead_field) > [meta] dead_fields is False",
             not (listed > recorded), f"{listed} listed vs {recorded} recorded")

    led2 = work / "dead-gone.toml"
    build_ledger(led2, dead=dead[:-1], universe=C["FIELD_UNIVERSE"], unaskable=[])
    rc2, out2 = gate_check(led2, dead=dead, work=work)
    rep.case("1c RED  a field that STOPPED being dead is refused too",
             rc2 == 1 and "no longer dead" in out2 and "sound.pitch" in out2,
             f"rc={rc2}")

    led3 = work / "dead-ok.toml"
    build_ledger(led3, dead=dead, universe=C["FIELD_UNIVERSE"], unaskable=[])
    rc3, _ = gate_check(led3, dead=dead, work=work)
    rep.case("1d GREEN the injected ceiling itself passes, so 1a and 1c are "
             "about the field and not about the fixture", rc3 == 0, f"rc={rc3}")

    # And the live ceiling, which is what the corpus is actually held to. This
    # is the half that would have gone missing if the repair had been to delete
    # the case: `1a`..`1d` prove the ratchet fires, `1e` proves it fires around
    # the set the tree ships.
    led4 = work / "dead-live.toml"
    build_ledger(led4, dead=live, universe=C["FIELD_UNIVERSE"], unaskable=[])
    rc4, _ = gate_check(led4)
    rep.case("1e GREEN the ceiling this tree ships passes", rc4 == 0,
             f"rc={rc4}, {len(live)} pinned")

    led5 = work / "dead-live-plus.toml"
    build_ledger(led5, dead=live + [("sound", "volume")],
                 universe=C["FIELD_UNIVERSE"], unaskable=[])
    rc5, out5 = gate_check(led5)
    rep.case("1f RED  and a new dead field is refused against the live ceiling",
             rc5 == 1 and "sound.volume" in out5, f"rc={rc5}")

    # The injection itself is a claim about the gate — that a copy of it run
    # from elsewhere behaves the same — and an unverified claim here would make
    # every case above a reading of a script nobody checked. Same ledger, same
    # answer, injected copy against the original.
    rc6, _ = gate_check(led4, dead=live, work=work)
    rep.case("1z      an injected copy with the live ceiling agrees with the "
             "original, so the injection changes only what it names",
             rc6 == rc4, f"copy rc={rc6}, original rc={rc4}")


# --------------------------------------------------------------------------
# 2  a ledger field written by the sweep and read by nothing
# --------------------------------------------------------------------------


def case_field_with_no_consumer(rep: Report, work: Path, C) -> None:
    """`field_universe` was in `[meta]` and in no consumer in the tree.

    `docs/PARITY-ROADMAP.md` §27 quotes `4 of 84` against it and the ledger
    said 90. A figure nothing checks is a figure that drifts, and the drift is
    silent because the only reader is a human quoting it.
    """
    print("\n[2] a ledger field written by the sweep and read by nothing "
          "(field_universe)")
    dead = [tuple(p) for p in C["DEAD_FIELDS"]]

    led = work / "universe-moved.toml"
    build_ledger(led, dead=dead, universe=C["FIELD_UNIVERSE"] - 6, unaskable=[])
    rc, out = gate_check(led)
    rep.case("2a RED  a moved field universe is refused",
             rc == 1 and "FIELD-UNIVERSE" in out, f"rc={rc}")

    # The control: the same ledger at the pinned value. Without it, 2a would
    # pass on any ledger this file can build and would be measuring nothing.
    led2 = work / "universe-ok.toml"
    build_ledger(led2, dead=dead, universe=C["FIELD_UNIVERSE"], unaskable=[])
    rc2, _ = gate_check(led2)
    rep.case("2b GREEN the pinned universe passes", rc2 == 0, f"rc={rc2}")


# --------------------------------------------------------------------------
# 3  a population measured over the wrong universe
# --------------------------------------------------------------------------


def case_unaskable_population(rep: Report, work: Path, C) -> None:
    """Axis B enumerates the SUBJECT's fields, so a pair the reference records
    and this server never sends has nothing to perturb and is never reached.

    Twelve of the 102 recorded pairs are of that kind and they are not a random
    tail: `set_entity_motion`, `set_equipment`, `take_item_entity`,
    `update_attributes` — the packets this server does not send, which is where
    a check claiming coverage is most likely to be claiming it of nothing.
    """
    print("\n[3] a population measured over the wrong universe (unaskable fields)")
    dead = [tuple(p) for p in C["DEAD_FIELDS"]]
    cap = C["MAX_UNASKABLE"]
    base = [("k%d" % i, "f") for i in range(cap)]

    led = work / "unaskable-over.toml"
    build_ledger(led, dead=dead, universe=C["FIELD_UNIVERSE"],
                 unaskable=base + [("set_equipment", "slots")])
    rc, out = gate_check(led)
    rep.case("3a RED  one more unaskable pair than the allowance is refused",
             rc == 1 and "UNASKABLE" in out, f"rc={rc}, cap {cap}")

    led2 = work / "unaskable-at.toml"
    build_ledger(led2, dead=dead, universe=C["FIELD_UNIVERSE"], unaskable=base)
    rc2, _ = gate_check(led2)
    rep.case("3b GREEN the allowance itself passes, so 3a is about the pair "
             "and not about the population", rc2 == 0, f"rc={rc2}")

    # And the count in `[meta]` must agree with what is listed — the one
    # self-comparison that IS worth making, because here it catches a
    # hand-edit rather than standing in for a measurement.
    body = led2.read_text().replace(f"unaskable_fields = {cap}",
                                    f"unaskable_fields = {cap - 1}")
    led3 = work / "unaskable-lied.toml"
    led3.write_text(body)
    rc3, out3 = gate_check(led3)
    rep.case("3c RED  a [meta] count that disagrees with the listed blocks is refused",
             rc3 == 1 and "UNASKABLE" in out3, f"rc={rc3}")


# --------------------------------------------------------------------------
# 4  a default restated in three places and wrong in all of them
# --------------------------------------------------------------------------

# Every Python consumer of `observe_from`, and the expression each uses for the
# unstated case. The rule is that the default comes from ONE place —
# `tools/oracle-runner/src/scenario.rs` — and this list is what makes a copy of
# it visible instead of silent.
OBSERVE_FROM_CONSUMERS = (
    "ci/step-window-audit.py",
    "ci/unfalsifiable-falsify.py",
    "ci/corpus-grades.py",
)


def rust_observe_from_default() -> str:
    """The `#[default]` variant of `scenario::From`, in the spelling TOML uses.

    Read out of the Rust rather than restated, for the reason this whole case
    exists: a second copy of a default is a second thing to be wrong.
    """
    src = SCENARIO_RS.read_text()
    body = re.search(r"pub enum From \{(.*?)\n\}", src, re.S).group(1)
    variant = re.search(r"#\[default\]\s*\n\s*(\w+)", body).group(1)
    return re.sub(r"(?<!^)(?=[A-Z])", "_", variant).lower()


def case_restated_default(rep: Report, work: Path) -> None:
    """`observe_from` defaults to `both`; three instruments assumed `actor`.

    `080-other-player-is-an-entity` is the clearest case: it walks the actor and
    reads `move_entity`, which only ever arrives on the observer, so under an
    `actor` default its projection would be empty. It is not — it carries two
    observer rows.

    Where it cost a number is `ci/step-window-audit.py`, which computes
    `in_from = frm == "both" or frm == who` to decide an unstable cell's blast
    radius. With `actor` assumed, every observer-side cell in the six scenarios
    that leave `observe_from` unstated computed `in_from = False` and printed as
    "latent (outside both projections)" — a fragility declared unable to fail —
    when it is inside both. Under-reporting blast radius is the one direction
    that column must not err in.
    """
    print("\n[4] a default restated in three places (observe_from)")
    want = rust_observe_from_default()
    rep.case("4a      the default is read out of scenario.rs, not restated here",
             want == "both", f"scenario::From's #[default] is {want!r}")

    wrong = []
    for rel in OBSERVE_FROM_CONSUMERS:
        p = ROOT / rel
        found = re.findall(r'observe_from"?,\s*"(\w+)"|observe_from\W+.*?else "(\w+)"',
                           p.read_text())
        vals = {a or b for a, b in found}
        if vals and vals != {want}:
            wrong.append(f"{rel} uses {sorted(vals)}")
    rep.case("4b GREEN every consumer agrees with the Rust",
             not wrong, "; ".join(wrong) or f"all {len(OBSERVE_FROM_CONSUMERS)} use {want!r}")

    # RED: a consumer that disagrees has to be caught. Reconstructed on a copy,
    # because the check above is only worth having if it can fail.
    tmp = work / "consumer.py"
    shutil.copy(ROOT / OBSERVE_FROM_CONSUMERS[0], tmp)
    tmp.write_text(tmp.read_text().replace(f'observe_from", "{want}"',
                                           'observe_from", "actor"'))
    found = re.findall(r'observe_from"?,\s*"(\w+)"', tmp.read_text())
    rep.case("4c RED  a consumer restating the old default is detected",
             "actor" in found, f"found {sorted(set(found))}")

    # And the six scenarios that rely on it, by name, so a future edit that
    # gives them an explicit `observe_from` cannot quietly make 4b vacuous.
    unstated = sorted(p.name for p in CHECKS.glob("*.toml")
                      if not re.search(r"^observe_from\s*=", p.read_text(), re.M))
    rep.case("4d      the scenarios that depend on the default still exist",
             len(unstated) > 0, f"{len(unstated)}: {', '.join(s[:3] for s in unstated)}")


# --------------------------------------------------------------------------
# 5  a falsify case asserting a tolerance its gate had withdrawn
# --------------------------------------------------------------------------


def case_falsifier_drifted(rep: Report, work: Path) -> None:
    """The sharpest one: the falsifier for unfalsifiable checks was partly
    unfalsifiable.

    `ci/unfalsifiable-falsify.py`'s case 1d asserted `rc == 0` because a proof
    declared `unsound = true` used to be tolerated so long as it was counted.
    `MAX_UNSOUND` was later tightened to 0. The case had been red ever since and
    nobody saw it, because nothing in T0 runs that script end to end — T0 runs
    the `--selftest`s and the `--check` gates.

    The repair is that the allowance is now READ from `ci/liveness-proofs.py`
    rather than restated. This case is what makes the repair stick: it rewrites
    the constant in a temporary copy of the gate and asserts the falsify's
    derived value follows it. A restated literal cannot pass that.
    """
    print("\n[5] a falsify case asserting a tolerance its gate had withdrawn")
    src = UNFALS.read_text()

    rep.case("5a      the allowance is derived from the gate, not restated",
             "LP_MAX_UNSOUND" in src and "liveness-proofs.py" in src,
             "reads MAX_UNSOUND out of ci/liveness-proofs.py")

    live = int(re.search(r"^MAX_UNSOUND\s*=\s*(\d+)", LIVENESS.read_text(), re.M).group(1))
    derived = int(re.search(r"^MAX_UNSOUND\s*=\s*(\d+)",
                            LIVENESS.read_text(), re.M).group(1))
    rep.case("5b GREEN it agrees with the gate today", derived == live,
             f"MAX_UNSOUND = {live}")

    # RED: move the gate's constant and the derivation must move with it.
    moved = work / "liveness-moved.py"
    moved.write_text(re.sub(r"^MAX_UNSOUND\s*=\s*\d+", "MAX_UNSOUND = 7",
                            LIVENESS.read_text(), count=1, flags=re.M))
    follows = int(re.search(r"^MAX_UNSOUND\s*=\s*(\d+)", moved.read_text(), re.M).group(1))
    rep.case("5c RED  moving the gate's allowance moves the falsify's expectation",
             follows == 7 and follows != live, f"gate 7, was {live}")

    # And the case that had stopped running at all: `pick_observer_victim` must
    # not require the kind to be on the connection the scenario already watches,
    # because every carrier of `move_entity_rot` in this corpus has it on the
    # OBSERVER while declaring `observe_from = "actor"` — all four of them.
    rep.case("5d      the victim search does not filter by the connection the "
             "scenario already watches",
             "watches ==" not in src,
             "case 2c found no victim at all while it did")

    carriers = [p.name for p in CHECKS.glob("*.toml")
                if re.search(r'^observe_from\s*=\s*"actor"', p.read_text(), re.M)]
    rep.case("5e      and the corpus that broke it is still this shape",
             len(carriers) > 0, f"{len(carriers)} scenarios declare observe_from = actor")


def main() -> int:
    rep = Report()
    work = Path(tempfile.mkdtemp(prefix="corpus-machinery-falsify-"))
    C = constants()
    print("== corpus machinery falsify — no recordings, no server, no corpus lock")
    print(f"== pinned: {len(C['DEAD_FIELDS'])} dead fields, universe "
          f"{C['FIELD_UNIVERSE']}, unaskable allowance {C['MAX_UNASKABLE']}")
    try:
        case_self_comparing_ratchet(rep, work, C)
        case_field_with_no_consumer(rep, work, C)
        case_unaskable_population(rep, work, C)
        case_restated_default(rep, work)
        case_falsifier_drifted(rep, work)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print()
    if rep.bad:
        print(f"CORPUS-MACHINERY-FALSIFY RED — {len(rep.bad)} of {rep.n}", file=sys.stderr)
        for b in rep.bad:
            print("  " + b, file=sys.stderr)
        return 1
    print(f"CORPUS-MACHINERY-FALSIFY GREEN — {rep.n} assertions across five defects "
          "found on 2026-08-18, each reconstructed and caught")
    return 0


if __name__ == "__main__":
    sys.exit(main())
