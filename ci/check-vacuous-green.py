#!/usr/bin/env python3
"""Refuse a NEW check whose green cannot be turned red.

    python3 ci/check-vacuous-green.py            # T0
    python3 ci/check-vacuous-green.py --write    # re-derive the census first

# WHAT THIS GATES

`ci/vacuous-green-census.py` derives, from the source, every script under `ci/`
that declares a verdict and that nothing runs, and every exit-**0** path whose
own message says the check could not do its job. This file reads that census,
compares it against `DECLARED` below, and reds T0 on:

* an instance the census finds and `DECLARED` does not carry — a NEW one;
* a `cmd; VAR=$?` capture under an active `set -e`, whose handler is dead
  code. That one is ceilinged at **zero**: it is never a design choice, and
  it shipped in `ci/t0.sh` itself three times (254, 300, 934) plus once in
  `ci/loot-golden.sh`, all repaired in the commit that added the detector;
* a `DECLARED` entry the census no longer finds — an instance that was
  repaired, or moved, with its declaration left standing. A stale exemption is
  not harmless: it stands ready to cover the next instance that lands in the
  same script, which is exactly what `DORMANT_GENERATORS` refuses on the
  generator side (`ci/check-generator-arms.sh`, 2026-09-05);
* a committed census that no longer matches the derived one;
* a census that has stopped finding anything. **This is the refusal that
  matters most**, and it is here because of what happened on 2026-09-05: one
  falsifier passed green by mutating an empty dict, and another read a failed
  build as a caught mutation. A guard whose population can go to zero without
  a word is the defect it is refusing.

# ONE ENTRY PER LINE, WITH A REASON

`DECLARED` is a dict literal with one key per line and the reason on the lines
above it, so that two lanes adding entries touch different lines and merge
without a conflict. The shape is `ci/check-generator-arms.sh`'s
`DORMANT_GENERATORS`, for the reason stated there: an entry with no reason is
indistinguishable from an entry somebody added to get green.

# WHAT IS DELIBERATELY NOT GATED

The census also reports `no_population_floor` — invoked verdict scripts with no
visible refusal of an empty or undersized population. That is a **candidate**
list, not a finding: it is a static proxy and it has not been adjudicated one
entry at a time. Ratcheting it would produce reds a lane can only clear by
raising a number, which teaches nobody anything and is a ritual, not a gate.
It is printed on every run so it can be worked through, and it is not a red.
"""

from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GENERATOR = ROOT / "ci" / "vacuous-green-census.py"
CENSUS = ROOT / "ci" / "checks" / "vacuous-green.toml"

# ===========================================================================
# THE FLOORS — what makes this guard unable to pass by finding nothing
# ===========================================================================
#
# 320 scripts and 94 verdict-declaring ones, measured on the tree at
# 1d109abcf by `python3 ci/vacuous-green-census.py --write`. The floors sit
# under those with room for a lane deleting a handful of scripts, and above
# zero by a wide margin, because the failure this exists to catch is the
# census returning an empty population and every comparison below then
# succeeding vacuously.
SCAN_FLOOR = 280
VERDICT_FLOOR = 80

# ===========================================================================
# THE CEILINGS — the ratchet
# ===========================================================================
#
# Every one of these may fall freely and may not rise. A rise is a new
# instance of this project's central defect class and has to be either fixed
# or declared below with its evidence.
#
# RAISED ONCE, ON THE REBASE ONTO 1049a12d3 (2026-09-24): 18 -> 22 uninvoked,
# 7 -> 8 green-on-refusal scripts, 8 -> 9 sites. Every one of the rise is a
# script that landed on main after this guard was written and before it
# landed, each declared by name in DECLARED with its reason: four new
# uninvoked hand instruments and falsifiers, ci/structure-surface.py repaired
# from green-on-refusal to exit 2 (and so now counted as uninvoked), less
# ci/chunk-charge-floor.py, which ci/sulfur-r18-adjudicate.sh now invokes, and
# ci/refusal-ledger.py --table, a false positive on a column header.
#
# RAISED AGAIN ON train-c (2026-09-30), over main 3745 + stack-ci +
# grepq-pipefail + portability: 22 -> 30 uninvoked, 8 -> 11 green-on-refusal
# scripts, 9 -> 12 sites. Every one of the rise is named in DECLARED under
# "FOUND ON train-c" with its reason (nine uninvoked, two of them false
# negatives of the loop-blind invocation reader; four green-on-refusal, none
# reachable as a gate verdict), less three declarations retired because the
# census no longer finds them (check-cited-audits.py, figure-provenance.py,
# block-class-predicts.py). One errexit-dead capture stack-ci carried,
# ci/t0.sh `python3 ci/check-t1-controls.py; T1C_RC=$?`, was REPAIRED rather
# than declared, so that ceiling stays zero.
# 30 -> 31 on train-c's rebase onto main 3755: ci/recipe-match-falsify.sh,
# declared above; 31 -> 32 over main 3953 for ci/sweep-guard.py, likewise.
# 32 -> 33 on nether-parity for ci/nether-parity-windows.py, declared below.
# main's ci/recipe-match-golden.sh carried a `status=$?`
# under set -e, repaired to `|| status=$?`, so errexit-dead stays 0.
CEILINGS = {
    "uninvoked_verdict_scripts": 33,
    "green_on_refusal_scripts": 11,
    "green_on_refusal_sites": 12,
    # ZERO, and it is the only count here that is. `cmd; VAR=$?` under an
    # active `set -e` is never deliberate — the handler it captures for cannot
    # run — so there is no honest reason to declare one, and a ceiling of zero
    # says that. All four instances on this tree were repaired in the commit
    # that added the detector: ci/t0.sh 254, 300, 934 and ci/loot-golden.sh 150.
    "errexit_dead_captures": 0,
    "errexit_dead_scripts": 0,
}

# ===========================================================================
# DECLARED — every instance standing on this tree, with why it stands
# ===========================================================================
#
# Key is `"<kind> <script>"`. Kinds are `uninvoked` and `green-on-refusal`.
# ONE ENTRY PER LINE. The reason goes in the comment block above the line.
DECLARED: dict[str, str] = {
    # ---- uninvoked: a script that declares a verdict and nothing runs -------
    #
    # THE FOUR ORPHANED FALSIFIERS. Each is a red-when-broken proof for
    # something that ships, and each runs nowhere: not T0, not ci/handoff.sh,
    # not the chain. This is the shape `ci-gate-stage-falsifiability` found in
    # `ci/handoff-verdict-selftest.sh`, whose own header records it reporting
    # 0 passed / 17 failed on unmodified main, found by somebody running it by
    # hand. Being green while nothing runs them is the state that produced
    # that. Wiring each needs its runtime measured first, which is not this
    # branch's budget; they are declared so the count cannot grow.
    "uninvoked ci/batching-selftests.sh": "orphaned selftest for the batching driver; nothing invokes it",
    "uninvoked ci/chat-commands-falsify.sh": "orphaned falsifier for the chat-command surface; nothing invokes it",
    "uninvoked ci/movement-falsify.sh": "orphaned falsifier for the movement checks; nothing invokes it",
    "uninvoked ci/falsify-ruined-portal.py": "orphaned falsifier for the ruined-portal port; nothing invokes it, and its own NO-OP arm exits 0 when its patterns stop matching",
    #
    # THE RATCHET NOBODY RUNS. `ci/decoded-kind-reach.py` carries a committed
    # baseline at `ci/checks/decoded-kind-reach.json` and ratchets the number
    # of decoded packet kinds no check reaches. A ratchet with a baseline and
    # no caller is a number that can only be right. `docs/cold-start/
    # commands-surface.md` names wiring it into `ci/t2-gameplay.sh` as that
    # branch's own next item, so it is owned elsewhere and declared here.
    "uninvoked ci/decoded-kind-reach.py": "ratchet with a committed baseline that no gate invokes; wiring it is owned by docs/cold-start/commands-surface.md",
    #
    # HAND INSTRUMENTS. Each of these is meant to be run by a person, prints a
    # number, and is quoted in a document; none of them gates anything and
    # none of them should. They declare a verdict only in the sense of
    # refusing to print a number they cannot compute, which is the behaviour
    # this project wants. They are listed rather than filtered because the
    # difference between "a probe nobody runs" and "a gate nobody runs" is a
    # judgement, and a judgement belongs in a reviewed line, not in a regex.
    "uninvoked ci/biome-palette-order.sh": "hand probe: biome palette traversal order, quoted in docs/DETERMINISM.md",
    "uninvoked ci/corpus-history.py": "hand instrument: reads the recordings already on the box",
    "uninvoked ci/dig-probe.py": "hand probe: dig timing against a live server",
    "uninvoked ci/feature-body-shift.py": "hand instrument: feature body shift attribution",
    "uninvoked ci/fill-compare.sh": "hand probe: fill comparison against a reference",
    "uninvoked ci/floor-ladder.py": "hand instrument: the floor-vs-N ladder",
    "uninvoked ci/fluid-tape.sh": "hand probe: fluid tick tape",
    # A measurement, not a gate: it dumps every nether window (minutes each at
    # r=16) and diffs it against every reference, which no tier can afford.
    # Its refusals (stale binary, mixed orders, < 2 references) exit 2.
    "uninvoked ci/nether-parity-windows.py": "hand instrument: every nether window against every reference",
    "uninvoked ci/pack-census.py": "hand instrument: datapack census",
    "uninvoked ci/sulfur-read-trace.sh": "hand probe: sulfur read trace",
    "uninvoked ci/tick-section-classify.py": "hand instrument: tick-section classification, quoted in docs/PARITY-ROADMAP.md",
    "uninvoked ci/waterlog-poison-control.sh": "hand probe: waterlogging poison control",
    #
    # LANDED ON MAIN AFTER THIS GUARD WAS WRITTEN, found on the rebase onto
    # 1049a12d3. The one falsifier is the orphaned-falsifier shape above; the
    # three bench scripts are hand instruments by the benches-stay-outside-gates
    # rule in docs/AGENT-BRIEF.md.
    "uninvoked ci/be-channel-falsify.sh": "orphaned falsifier for ci/be-channel-identical.sh's two guards; docs/CI.md describes it, nothing invokes it",
    "uninvoked ci/bench-attrib-through.sh": "hand bench: the four-arm RUSTMC_THROUGH attribution run, quoted in docs/BENCH.md",
    "uninvoked ci/bench-boot-onecore.sh": "hand bench: one-core boot pairs, quoted in docs/BENCH.md",
    "uninvoked ci/bench-stamp-sha.sh": "hand bench helper: verifies a bench directory's source sha before ci/bench-attrib-through.sh reads it",
    # ci/structure-surface.py was green-on-refusal (exit 0 on a missing jar);
    # repaired to exit 2 in the same commit as this line, which is what makes
    # the census see it as a verdict script, and no script invokes it.
    "uninvoked ci/structure-surface.py": "hand instrument: structure surface census against the datagen extract; refuses at exit 2 with no extract",
    #
    #
    # ---- green-on-refusal: an exit-0 path that says it could not run --------
    #
    # THE TWO GATE-DIRECTORY BLINDNESSES, which are one defect at two callers.
    # `ci/t0-remote.sh` ships the tree with `--exclude .git` on purpose, so
    # every gate directory has no object database. Both of these print an
    # honest sentence and exit 0, so the gate's exit status learns nothing and
    # the only reader who could tell is a person. `ci/figure-provenance.py` is
    # repaired on `origin/figure-provenance-gate` (d80ffd9df) by stamping the
    # answer into `.figure-provenance` from the tree that has history;
    # `ci/check-cited-audits.py` is repaired the same way on this branch.
    # BOTH DECLARATIONS WERE RETIRED ON train-c (2026-09-30): the census no
    # longer finds either. stack-ci ships the object store and routes the
    # remaining stand-down through ci/lib/box_env.py, and the empty-ledger arm
    # of ci/check-cited-audits.py refuses at 2.
    #
    # A CHECK WITH NOTHING TO RE-MEASURE. `ci/mob-projection-search.py
    # --check` exits 0 when no recording on this machine carries the scenario,
    # standing on the pinned projections instead. In a gate directory
    # `oracle-work` is excluded, so this is the arm that always runs there.
    # It refuses (exit 2) when there is no ledger EITHER, which is the half
    # that keeps it from being vacuous outright.
    "green-on-refusal ci/mob-projection-search.py": "exits 0 with no recording, standing on the pinned ledger; refuses at exit 2 when there is no ledger either",
    #
    # NOT CHECKS. Each of these is an instrument or a helper whose exit-0
    # refusal is the honest answer and whose green nothing reads as a verdict.
    "green-on-refusal ci/bench-pair-summary.py": "bench reporter: prints NOT CAPTURED when perf is unavailable rather than a number it does not have",
    "green-on-refusal ci/hold-phases.sh": "instrument: says it cannot write its log, and the run it wraps is not a gate",
    "green-on-refusal ci/reference-cache.sh": "cache helper: declines to store when an input is unavailable; not storing is not a verdict",
    "green-on-refusal ci/village-oracle.sh": "dumper: skips with no vanilla wide reference; its output is consumed by a golden test that panics on a missing fixture",
    "green-on-refusal ci/village-pieces.sh": "dumper: skips with no vanilla datagen extract, same shape as ci/village-oracle.sh",
    #
    # A FALSE POSITIVE, declared rather than filtered. `ci/refusal-ledger.py
    # --table` prints a markdown table whose column header is the word
    # "refused" and then returns 0; that return ends a report, not a verdict.
    # Its `--check` path exits non-zero on a missing table.
    "green-on-refusal ci/refusal-ledger.py": "report mode: --table prints a column headed 'refused' and returns 0; not a verdict",
    #
    # FOUND ON train-c, 2026-09-30: stack-ci (233 commits) and main 3611 -> 3745
    # landed these after 461786aaf re-derived the census. Nine uninvoked, four
    # green-on-refusal; none is a gate that exits 0 in a gate directory.
    #
    # Two are FALSE NEGATIVES of the invocation reader: ci/t0.sh:1979 runs
    # them from a `for f in ...` list, which it does not parse.
    "uninvoked ci/lockfile-falsify.sh": "invoked by the `for f in` loop over the environment falsifiers in ci/t0.sh (box-env / lockfile / shared-refs); the census reads literal invocations only",
    "uninvoked ci/shared-refs-falsify.sh": "invoked by the same `for f in` loop in ci/t0.sh as ci/lockfile-falsify.sh; the census reads literal invocations only",
    # A falsifier nothing runs: the orphaned-falsifier shape. It needs a built
    # server and recordings, so it is not a T0 stage; docs/checks cite it.
    "uninvoked ci/schema-check-falsify.py": "orphaned falsifier for scenarios 960-963, run by hand against recordings; ci/checks/gameplay-baseline.toml cites its result",
    # Hand instruments and probes, by the benches-stay-outside-gates rule.
    "uninvoked ci/block-schema-queue.py": "hand instrument: the 116 block-property schemas as a ranked work queue, docs/PARITY-ROADMAP.md",
    "uninvoked ci/corpus-delay-curve.py": "hand instrument: runqueue delay against box load from ci/corpus-holder-rate.sh samples, docs/CORPUS-LOCK-HOLD.md",
    "uninvoked ci/corpus-exclusion-cost.sh": "hand instrument: prices the falsifier rework a relaxed corpus lock would need, docs/CORPUS-LOCK-HOLD.md 18",
    "uninvoked ci/place-subset-probe.sh": "hand probe: 730/731/732 stability before spending a full corpus run; takes the corpus lock as role run",
    "uninvoked ci/subset-stability-locked.sh": "hand wrapper: one corpus lock around a ci/subset-stability.sh ensemble",
    # main 3755's recipe falsifier (f0a69d6d6): six source mutations, each a
    # rebuild of three targets, so a hand falsifier and not a T0 stage; its
    # result is quoted in ci/checks/test-targets.toml and docs/FINDINGS.md.
    "uninvoked ci/recipe-match-falsify.sh": "hand falsifier for the recipe gates: six mutations of recipes.rs, each a three-target rebuild; caught 6 survived 0 per docs/FINDINGS.md",
    # main 3953 (train-a): an agent-side guard run between a --sweep and its
    # commit; its own header says it has no place in T0.
    "uninvoked ci/sweep-guard.py": "agent-side guard around a sweep (swept vs committed ledger), not a T0 check by its own header; used by this train before committing its sweep",
    "uninvoked ci/useitemon-unit.py": "hand instrument: the useItemOn unit-of-work census, docs/PARITY-ROADMAP.md",
    # Not refusals that a gate reads.
    "green-on-refusal ci/check-source-citations.py": "--fix mode: lists the citations it left for a human and returns 0 after rewriting the rest; the T0 mode (no flag) exits 1 on any STALE and on the bare ceiling",
    "green-on-refusal ci/corpus-dead-observe.py": "hand instrument nothing invokes: says 'no recordings found' and returns 0; there is no verdict to be vacuous",
    "green-on-refusal ci/entity-leak-audit.py": "false positive: the matched line is the report's legend for `placeable 0`, printed after the counts, not a refusal",
    "green-on-refusal ci/entity-reach-falsify.sh": "the data/vanilla-absent skip is unreachable from a gate: ci/t0.sh reds on that condition first in RUSTMC_FIXTURES=required, as the script's own comment says",
}


def derive() -> dict:
    """Run the generator and read its census back as TOML.

    Run as a SUBPROCESS rather than imported, so that a generator that cannot
    even start is a non-zero exit here instead of an ImportError three frames
    down that a caller might read as a missing optional dependency.
    """
    r = subprocess.run(
        [sys.executable, str(GENERATOR)], capture_output=True, text=True, cwd=str(ROOT)
    )
    if r.returncode != 0:
        print(
            "VACUOUS-GREEN RED — the census generator failed:\n"
            + (r.stderr or r.stdout)[-2000:],
            file=sys.stderr,
        )
        raise SystemExit(2)
    try:
        return tomllib.loads(r.stdout)
    except tomllib.TOMLDecodeError as e:
        print(f"VACUOUS-GREEN RED — the census generator emitted invalid TOML: {e}",
              file=sys.stderr)
        raise SystemExit(2)


def keys_of(c: dict) -> set[str]:
    ks = {"uninvoked " + u["script"] for u in c.get("uninvoked", [])}
    ks |= {"green-on-refusal " + g["script"] for g in c.get("green_on_refusal", [])}
    ks |= {"errexit-dead " + d["script"] for d in c.get("errexit_dead", [])}
    return ks


def main(argv: list[str]) -> int:
    if "--write" in argv:
        subprocess.run([sys.executable, str(GENERATOR), "--write"], cwd=str(ROOT), check=True)

    derived = derive()
    m = derived.get("meta", {})

    # ---- 1. the census must have found a population ------------------------
    scanned = m.get("scripts_scanned", 0)
    verdicts = m.get("verdict_scripts", 0)
    if scanned < SCAN_FLOOR or verdicts < VERDICT_FLOOR:
        print(
            f"VACUOUS-GREEN RED — the census has stopped finding anything: "
            f"{scanned} scripts scanned (floor {SCAN_FLOOR}), {verdicts} declare "
            f"a verdict (floor {VERDICT_FLOOR}).\n"
            "    Every comparison below succeeds vacuously over an empty "
            "population, which is the\n"
            "    exact defect this guard exists to refuse. Fix the generator "
            "or lower the floor in a\n"
            "    reviewed commit that says why the tree really did shrink.",
            file=sys.stderr,
        )
        return 1

    # ---- 2. the committed census must match the derived one ----------------
    if not CENSUS.exists():
        print(f"VACUOUS-GREEN RED — {CENSUS.relative_to(ROOT)} is absent; run "
              "`python3 ci/vacuous-green-census.py --write`", file=sys.stderr)
        return 1
    committed = tomllib.loads(CENSUS.read_text())
    if committed.get("meta") != m or keys_of(committed) != keys_of(derived):
        print("VACUOUS-GREEN RED — the committed census is stale.\n"
              f"    committed meta: {committed.get('meta')}\n"
              f"    derived   meta: {m}\n"
              "    Re-derive it with `python3 ci/vacuous-green-census.py "
              "--write` and commit the result.", file=sys.stderr)
        return 1

    # ---- 3. the errexit-dead handler, which is never declarable ------------
    #
    # Refused before the DECLARED comparison and with its own sentence, because
    # it is the one shape here that is never a design choice: under an active
    # `set -e`, `cmd; VAR=$?` exits AT cmd and the handler is unreachable. There
    # is nothing to declare and no reason to, so the message must not offer a
    # DECLARED line as the way out.
    # The census emits one flat `[[errexit_dead]]` table per site, not one per
    # script with a nested list; reading it as nested printed "0 exit handler
    # that cannot run" over a real one, which is a report that cannot say what
    # it found. Caught by case 6 of the falsifier, which asserts on the file
    # name and not on the exit code.
    dead = derived.get("errexit_dead", [])
    if dead:
        print(f"VACUOUS-GREEN RED — {len(dead)} exit handler(s) that cannot "
              "run, under an active `set -e`:", file=sys.stderr)
        for site in dead:
            print(f"  !! {site['script']}:{site['line']}  ({site['form']})  "
                  f"{site['text']}", file=sys.stderr)
        print("    `cmd; VAR=$?` is a sequential list and errexit is NOT "
              "suspended in one: the shell\n"
              "    exits AT cmd with its status and the case/if below it never "
              "runs, so the stage\n"
              "    prints no verdict at all.\n"
              "        $ bash -c 'set -euo pipefail; false; RC=$?; echo reached'"
              "; echo \"exit=$?\"\n"
              "        exit=1        # nothing printed\n"
              "    Write it as `VAR=0; cmd || VAR=$?`. There is no declaration "
              "for this one.", file=sys.stderr)
        return 1

    # ---- 4. no undeclared instance ----------------------------------------
    found = keys_of(derived)
    declared = set(DECLARED)
    new = sorted(found - declared)
    if new:
        print(f"VACUOUS-GREEN RED — {len(new)} instance(s) this tree does not "
              "declare:", file=sys.stderr)
        for k in new:
            print(f"  !! {k}", file=sys.stderr)
        print("    A check nothing invokes, or a check that exits 0 saying it "
              "could not run, is a\n"
              "    green that cannot be turned red. Repair it, or add ONE LINE "
              "to DECLARED in\n"
              "    ci/check-vacuous-green.py with the reason on the lines "
              "above it.", file=sys.stderr)
        return 1

    # ---- 5. no stale declaration ------------------------------------------
    stale = sorted(declared - found)
    if stale:
        print(f"VACUOUS-GREEN RED — {len(stale)} declaration(s) name an "
              "instance the census no longer finds:", file=sys.stderr)
        for k in stale:
            print(f"  !! {k} — {DECLARED[k]}", file=sys.stderr)
        print("    The instance was repaired, renamed or deleted and the "
              "exemption was left standing.\n"
              "    An exemption kept past its reason is one that will silently "
              "cover the next instance\n"
              "    that lands in the same script. Delete the line.",
              file=sys.stderr)
        return 1

    # ---- 6. the ratchet ----------------------------------------------------
    risen = {k: (m.get(k, 0), v) for k, v in CEILINGS.items() if m.get(k, 0) > v}
    if risen:
        print("VACUOUS-GREEN RED — a ratcheted count rose:", file=sys.stderr)
        for k, (got, ceil) in risen.items():
            print(f"  !! {k}: {got} > {ceil}", file=sys.stderr)
        return 1

    nofloor = derived.get("no_population_floor_scripts", [])
    print(
        f"VACUOUS-GREEN — {scanned} scripts scanned, {verdicts} declare a verdict, "
        f"{m['uninvoked_verdict_scripts']} invoked by nothing and "
        f"{m['green_on_refusal_sites']} exit-0 refusal site(s) in "
        f"{m['green_on_refusal_scripts']} script(s), every one declared "
        f"(ceilings {CEILINGS})."
    )
    print(
        f"                 {len(nofloor)} invoked verdict script(s) show no floor "
        "on their own population — a CANDIDATE list, not gated; see this file's "
        "docstring."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
