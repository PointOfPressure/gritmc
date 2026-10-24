#!/usr/bin/env python3
"""Poison the third verdict — `compare::Verdict::Mute` — and read it back.

A control that cannot fire is not a control, and this branch is *about* that
sentence: it exists because `607-container-click-pickup-all-gathers-matching-
stacks` passed for its whole life by comparing two silences at the step it was
named for. Shipping a verdict that asserts such a thing cannot happen, without
demonstrating the verdict firing, would be the same defect one level up.

So this makes a real scenario's measured step go silent inside a real recorded
trace pair, runs the real comparator over the real corpus, and asserts that
`mute` appears **at that scenario and at no other**. No server is started and
nothing is recorded: it re-compares recordings that already exist, in seconds.

    ci/mute-falsify.py [--bin PATH] [--checks DIR] VANILLA.json RUSTMC.json

Six cases, and each one exists because the opposite mistake is available:

  1. baseline — the unpoisoned pair, so every later case has something to be
     a delta against, and so that a corpus already full of `mute` cannot be
     mistaken for a control firing.
  2. poison — delete every observation attributed to a victim's **last**
     `[[step]]` from *both* sides. The victim must become `mute`.
  3. localisation — and every other scenario's verdict must be **unchanged**.
     A verdict that reddens the corpus when one check goes quiet is not
     telling you which check went quiet.
  4. the declaration does the work — strip `measured_step = "silent"` from one
     of the four by-design negative checks and it must become `mute`. This is
     the case that proves `561`, `571`, `581` and `608` are green because
     something is declared and true, and not because the verdict cannot reach
     them. Without it, "the four stay green" is indistinguishable from "the
     verdict never fires". A subject that is not `match` at baseline cannot
     exercise this case at all — `Mute` only relabels a `Match` — so it is
     reported UNRUNNABLE rather than failed, and the suite refuses outright
     if fewer than two subjects remain.
  5. the declaration is sufficient — add `measured_step = "silent"` to the
     poisoned victim of case 2 and it must go back to its original verdict.
     Together with 4 this pins the discriminator: the same recording, the same
     comparator, one line of TOML, both directions.
  6. the wrong step — silence a victim's **first** step instead of its last
     and no new `mute` may appear anywhere. The instrument keys on the step
     the scenario is named for, not on "some step went quiet", which is the
     reading that would price rig-building out of the corpus.

Exit 0 if every case holds, 1 if any does not, 2 if it could not be run —
which is the same three-way split `ci/selfcheck-recompare.sh` uses, and for
the same reason: a control that cannot run must not read as a control that
passed.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BIN = ROOT / "target" / "release" / "oracle-runner"
DEFAULT_CHECKS = ROOT / "ci" / "checks" / "gameplay"

# The four by-design negative checks, and the one this branch closes. Named
# here so a failure reads as a sentence rather than as a diff, and asserted
# against the corpus rather than trusted: see `pick_declared_silent`.
SILENT_MARKER = 'measured_step = "silent"'


def die(msg: str, code: int = 2) -> None:
    print(f"MUTE-FALSIFY REFUSED — {msg}", file=sys.stderr)
    sys.exit(code)


def report(binary: Path, checks: Path, ref: Path, sub: Path, work: Path) -> list[dict]:
    """Every scenario's outcome, from one run of the real comparator."""
    out = work / "report.json"
    r = subprocess.run(
        [
            str(binary), "compare", str(checks),
            "--reference", str(ref), "--subject", str(sub), "--json", str(out),
        ],
        capture_output=True, text=True,
    )
    # A non-zero exit is the *normal* case here: a corpus with a `mute` in it
    # is a corpus the gate refuses, which is the whole point. The report is
    # what is read, so only its absence is fatal.
    if not out.exists():
        die(f"compare wrote no report (rc={r.returncode})\n{r.stdout}\n{r.stderr}")
    return json.loads(out.read_text())["outcomes"]


def verdicts(binary: Path, checks: Path, ref: Path, sub: Path, work: Path) -> dict[str, str]:
    return {o["name"]: o["verdict"] for o in report(binary, checks, ref, sub, work)}


def scenario_file(checks: Path, name: str) -> Path:
    for p in sorted(checks.glob("*.toml")):
        if re.search(rf'^name\s*=\s*"{re.escape(name)}"', p.read_text(), re.M):
            return p
    die(f"no scenario file declares name = {name!r}")
    raise SystemExit(2)  # unreachable; keeps the return type honest


def last_step_of(path: Path) -> int:
    return len(re.findall(r"^\[\[step\]\]", path.read_text(), re.M)) - 1


def silence(trace: Path, out: Path, name: str, step: int) -> int:
    """Copy `trace` to `out` with `name`'s observations at `step` removed."""
    doc = json.loads(trace.read_text())
    removed = 0
    for s in doc["scenarios"]:
        if s["name"] != name:
            continue
        before = len(s["obs"])
        s["obs"] = [o for o in s["obs"] if o.get("step") != step]
        removed = before - len(s["obs"])
    out.write_text(json.dumps(doc))
    return removed


def edited_corpus(checks: Path, work: Path, tag: str, name: str, add: bool) -> Path:
    """A copy of the corpus with one scenario's `silent` declaration flipped."""
    dst = work / f"checks-{tag}"
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(checks, dst)
    hit = scenario_file(dst, name)
    txt = hit.read_text()
    if add:
        if SILENT_MARKER in txt:
            die(f"{hit.name} already declares a silent measured step")
        txt = txt.replace("\nobserve = ", f"\n{SILENT_MARKER}\nobserve = ", 1)
    else:
        if SILENT_MARKER not in txt:
            die(f"{hit.name} does not declare a silent measured step")
        txt = txt.replace(f"{SILENT_MARKER}\n", "", 1)
    hit.write_text(txt)
    return dst


def main(argv: list[str]) -> int:
    args = argv[1:]
    binary, checks = DEFAULT_BIN, DEFAULT_CHECKS
    for flag, target in (("--bin", "binary"), ("--checks", "checks")):
        if flag in args:
            i = args.index(flag)
            val = Path(args[i + 1])
            if target == "binary":
                binary = val
            else:
                checks = val
            del args[i : i + 2]
    if len(args) != 2:
        die("usage: ci/mute-falsify.py [--bin PATH] [--checks DIR] VANILLA.json RUSTMC.json")
    ref, sub = Path(args[0]), Path(args[1])
    for p in (binary, checks, ref, sub):
        if not p.exists():
            die(f"missing {p}")

    work = Path(tempfile.mkdtemp(prefix="mute-falsify-"))
    failures: list[str] = []

    def check(case: str, ok: bool, detail: str) -> None:
        print(f"  {'PASS' if ok else 'FAIL'}  {case}: {detail}")
        if not ok:
            failures.append(f"{case}: {detail}")

    print(f"== corpus {checks}")
    print(f"== reference {ref}")
    print(f"== subject   {sub}")

    # --- 1. baseline -------------------------------------------------------
    outcomes = report(binary, checks, ref, sub, work)
    base = {o["name"]: o["verdict"] for o in outcomes}
    by_name = {o["name"]: o for o in outcomes}
    base_mute = sorted(n for n, v in base.items() if v == "mute")
    print(f"\n[1] baseline: {len(base)} scenarios, mute = {base_mute or 'none'}")

    # A victim that is currently `match` and not already declared silent: the
    # poison has to *move* a verdict, and a scenario that was already broken
    # cannot show that it did.
    silent_names = set()
    for p in sorted(checks.glob("*.toml")):
        txt = p.read_text()
        if SILENT_MARKER in txt:
            m = re.search(r'^name\s*=\s*"([^"]+)"', txt, re.M)
            if m:
                silent_names.add(m.group(1))
    if not silent_names:
        die("no scenario declares a silent measured step — case 4 has no subject")

    # **A victim has to have the shape the verdict is about**, and the first
    # draft of this script did not check that. Silencing the *only* step of a
    # one-step scenario empties its whole reference projection, which is
    # `vacuous` — decided inside `verdict_for`, before `Mute` is reached — and
    # three cases duly failed with `vacuous` against an expectation of `mute`.
    #
    # That is the verdicts agreeing, not disagreeing: `vacuous` is the older
    # and stronger statement and it already caught that case. `Mute` exists
    # for the narrower one `vacuous` cannot see, which is `607`'s: **a window
    # that is informative everywhere except at the step the check is named
    # for.** So a victim must keep saying something outside its last step
    # after the poison, on both sides, or the poison is testing the wrong
    # verdict.
    def shaped(name: str) -> bool:
        o = by_name[name]
        if (o.get("measured_step") or 0) < 1:
            return False
        # **Read off the declared projection, not the raw trace.** The first
        # draft asked the trace whether the scenario had observations before
        # its last step, and `a-bed-facing-north-puts-its-head-the-other-way`
        # duly qualified and duly came back `vacuous`: its earlier steps are
        # loud on the wire and silent under its own `observe` list, so the
        # poison emptied the projection after all. `reference_obs` and
        # `measured_reference_obs` are the comparator's own counts, through
        # the comparator's own projection, which is the only reading that can
        # predict the comparator's own verdict.
        for side in ("reference", "subject"):
            total, at = o[f"{side}_obs"], o[f"measured_{side}_obs"]
            if at < 1 or total - at < 1:
                return False
        return True

    victims = [
        n for n, v in sorted(base.items())
        if v == "match" and n not in silent_names and n not in base_mute and shaped(n)
    ][:3]
    if not victims:
        die(
            "no multi-step `match` scenario with observations both at and before its "
            "measured step — nothing here has the shape `mute` is about"
        )

    for victim in victims:
        last = last_step_of(scenario_file(checks, victim))

        # --- 2 & 3. poison the measured step, on both sides ----------------
        r2 = work / f"ref-{victim}.json"
        s2 = work / f"sub-{victim}.json"
        nr = silence(ref, r2, victim, last)
        ns = silence(sub, s2, victim, last)
        got = verdicts(binary, checks, r2, s2, work)
        check(
            f"[2] poison {victim} step {last}",
            got.get(victim) == "mute",
            f"removed {nr} ref / {ns} subject obs -> {got.get(victim)} (was {base[victim]})",
        )
        moved = {n for n in base if base[n] != got.get(n)} - {victim}
        check(
            f"[3] localised {victim}",
            not moved,
            "no neighbour moved" if not moved else f"ALSO MOVED: {sorted(moved)}",
        )

        # --- 5. the declaration is sufficient ------------------------------
        d5 = edited_corpus(checks, work, f"add-{victim}", victim, add=True)
        got5 = verdicts(binary, d5, r2, s2, work)
        check(
            f"[5] declaring {victim} silent restores it",
            got5.get(victim) == base[victim],
            f"{got5.get(victim)} (baseline {base[victim]})",
        )

        # --- 6. the wrong step -------------------------------------------
        if last > 0:
            r6 = work / f"ref6-{victim}.json"
            s6 = work / f"sub6-{victim}.json"
            silence(ref, r6, victim, 0)
            silence(sub, s6, victim, 0)
            got6 = verdicts(binary, checks, r6, s6, work)
            new_mute = sorted(
                n for n, v in got6.items() if v == "mute" and base.get(n) != "mute"
            )
            check(
                f"[6] silencing {victim} step 0 is not a mute",
                not new_mute,
                "no new mute" if not new_mute else f"NEW MUTE: {new_mute}",
            )

    # --- 4. the declaration is doing the work ------------------------------
    #
    # **A case-4 subject has to be `match` at baseline, and this is a
    # precondition rather than an assertion.** `Mute` only ever relabels a
    # `Match` — by construction, so that a real `Diverge` can never be
    # swallowed — so stripping the declaration from a scenario that is
    # *diverging for some unrelated reason* cannot produce a `mute` no matter
    # how well the verdict works. Asserting it anyway makes this control go
    # red for something outside its own subject, which is the shape of check
    # the next agent learns to route around.
    #
    # Measured 2026-08-11 on the `rustmc-verdict` pair: `608` diverges at its
    # step-0 `container_set_content`, a difference this branch does not
    # cause and cannot see (the stack-tip comparator scores the same
    # recordings identically), and case 4 duly reported `diverge` against an
    # expectation of `mute`. That is the precondition being absent, not the
    # verdict failing.
    #
    # So an ineligible subject is UNRUNNABLE and named, never silently
    # dropped — and the suite refuses if too few remain, because "the four
    # stay green" has to rest on more than one demonstration or it is back to
    # being a list of names. Two is the floor: one subject cannot distinguish
    # "the declaration does the work" from "this one scenario is odd".
    runnable4 = 0
    for name in sorted(silent_names):
        if base.get(name) != "match":
            print(
                f"  UNRUNNABLE  [4] {name}: baseline is {base.get(name)}, not match — "
                "`Mute` only relabels a `Match`, so this subject cannot exercise case 4"
            )
            continue
        runnable4 += 1
        d4 = edited_corpus(checks, work, f"strip-{name}", name, add=False)
        got4 = verdicts(binary, d4, ref, sub, work)
        check(
            f"[4] undeclaring {name}",
            got4.get(name) == "mute",
            f"{got4.get(name)} (declared: {base.get(name)})",
        )
    if runnable4 < 2:
        shutil.rmtree(work, ignore_errors=True)
        die(
            f"only {runnable4} of {len(silent_names)} declared-silent scenario(s) were "
            "`match` at baseline, so case 4 — the one that separates 'the declaration "
            "does the work' from 'the verdict never fires' — rests on too little. "
            "Re-run against a pair in which at least two of them match."
        )

    shutil.rmtree(work, ignore_errors=True)
    print()
    if failures:
        for f in failures:
            print(f"MUTE-FALSIFY FAILED  {f}")
        return 1
    print("MUTE-FALSIFY GREEN — the third verdict fires, and only where it should")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
